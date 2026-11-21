//! Levels of detail for every mesh drawn as instances: coarser copies of a shared mesh
//! ([`solidcraft_engine::geom::simplify`]), made once per mesh off the window thread, so a
//! placement small on screen is drawn from far fewer triangles ([`crate::gpu::LOD_PX`] decides
//! which). Until a mesh's levels are made it is drawn in full; when they are, the viewport fills
//! them into the scene it drew, without building it again ([`MeshLods::generation`]).
//!
//! Display only: the reference mesh (picking, measuring, exports) is never simplified.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use solidcraft_engine::doc::ModelBody;
use solidcraft_engine::geom::{Mesh, simplify};
use solidcraft_engine::kernel::Body;

use crate::gpu::SharedTris;

/// Meshes simplified at once.
const MAX_JOBS: usize = 2;
/// Concurrent input budget, in triangles plus groups of three positions. This allows two
/// ordinary jobs and lets large jobs use a worker exclusively, at any supported mesh size.
const WORK_BUDGET: usize = 8_000_000;

/// A mesh's levels: coarser copies, finest first, each with its error (mm).
pub(crate) type Levels = Arc<Vec<(Arc<SharedTris>, f32)>>;

/// The levels made (and being made), by mesh address with the mesh held, so its address stays
/// its own.
#[derive(Clone, Default)]
pub(crate) struct MeshLods(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    by_mesh: HashMap<usize, (Arc<Mesh>, Option<Levels>)>,
    /// The meshes waiting for a worker, oldest first.
    queue: VecDeque<Job>,
    /// Counts the times levels were made: the viewport fills them into its scene when it changes.
    generation: u64,
    making: usize,
    reserved: usize,
}

#[derive(Clone)]
struct Job {
    key: usize,
    source: Arc<Mesh>,
    indexed: Option<Body>,
    cost: usize,
}

/// Budget for the display geometry's positions and triangles, so large jobs have exclusive
/// access to the scratch budget even when their source is indexed.
fn work_cost(positions: usize, triangles: usize) -> usize {
    triangles.saturating_add(positions.div_ceil(3)).clamp(1, WORK_BUDGET)
}

impl State {
    fn take_job(&mut self) -> Option<Job> {
        let cost = self.queue.front()?.cost;
        if self.making >= MAX_JOBS || cost > WORK_BUDGET - self.reserved {
            return None;
        }
        let job = self.queue.pop_front()?;
        self.making += 1;
        self.reserved += cost;
        Some(job)
    }

    fn release(&mut self, cost: usize) {
        self.making -= 1;
        self.reserved -= cost;
    }

    fn failed_to_start(&mut self, job: Job) {
        self.release(job.cost);
        self.queue.push_front(job);
        if self.making > 0 {
            // A running worker will pick up the job when it finishes its current mesh.
            return;
        }
        // No workers (for example, in the browser): draw the queued meshes in full.
        let empty = Arc::new(Vec::new());
        for job in self.queue.drain(..) {
            if let Some(slot) = self.by_mesh.get_mut(&job.key) {
                slot.1 = Some(empty.clone());
            }
        }
        self.generation = self.generation.wrapping_add(1);
    }
}

impl MeshLods {
    /// `mesh`'s levels when made; otherwise `None`, and they are queued (once) for a worker. Every
    /// mesh gets them, whatever its size: a level is drawn only where its measured error is under
    /// [`crate::gpu::LOD_PX`] on screen, so a mesh that simplifies badly (a box) is drawn coarser
    /// only where it is a few pixels across, and one that cannot simplify gets none.
    pub(crate) fn levels(&self, mesh: &Arc<Mesh>, body: Option<&ModelBody>) -> Option<Levels> {
        let key = Arc::as_ptr(mesh) as usize;
        let mut state = self.0.lock().ok()?;
        if let Some((_, levels)) = state.by_mesh.get(&key).filter(|(held, _)| Arc::ptr_eq(held, mesh)) {
            return levels.clone();
        }
        queue(&mut state, &self.0, key, mesh, body);
        None
    }

    /// Changes whenever a mesh's levels are made.
    pub(crate) fn generation(&self) -> u64 {
        self.0.lock().map_or(0, |s| s.generation)
    }

    /// Levels are being made (keep frames coming to pick them up).
    pub(crate) fn busy(&self) -> bool {
        self.0.lock().is_ok_and(|s| s.making > 0)
    }

    /// Let go of meshes not in `used` (by address); only jobs already running stay.
    pub(crate) fn retain(&self, used: &HashSet<usize>) {
        if let Ok(mut state) = self.0.lock() {
            let State { queue, by_mesh, .. } = &mut *state;
            let head_removed = queue.front().is_some_and(|job| !used.contains(&job.key));
            queue.retain(|job| {
                let keep = used.contains(&job.key);
                if !keep {
                    by_mesh.remove(&job.key);
                }
                keep
            });
            by_mesh.retain(|k, (_, levels)| levels.is_none() || used.contains(k));
            if head_removed {
                start_worker(&mut state, &self.0);
            }
        }
    }
}

/// Queues `mesh` (at `key`) for a worker. Called with the state locked, so a retiring worker
/// ([`work`]) either sees this request or has already released its reservation.
fn queue(state: &mut State, shared: &Arc<Mutex<State>>, key: usize, mesh: &Arc<Mesh>, body: Option<&ModelBody>) {
    state.by_mesh.insert(key, (mesh.clone(), None));
    let cost = work_cost(mesh.positions.len(), mesh.triangles.len());
    let indexed = body
        .filter(|_| mesh.triangles.len() >= crate::viewport::INSTANCE_ALONE)
        .and_then(ModelBody::flat_mesh_body_in_place)
        .filter(|body| body.triangle_mesh().is_some_and(|(_, triangles)| triangles.len() == mesh.triangles.len()))
        .cloned();
    state.queue.push_back(Job { key, source: mesh.clone(), indexed, cost });
    start_worker(state, shared);
}

fn start_worker(state: &mut State, shared: &Arc<Mutex<State>>) {
    let Some(job) = state.take_job() else { return };
    let worker_job = job.clone();
    let shared = shared.clone();
    if std::thread::Builder::new().name("mesh-lod".into()).spawn(move || work(&shared, worker_job)).is_err() {
        state.failed_to_start(job);
    }
}

/// Makes queued meshes while the work fits. Publish finished levels before starting or waiting
/// on an exclusive job, or when this worker retires, so other meshes benefit as soon as possible.
fn work(shared: &Arc<Mutex<State>>, mut job: Job) {
    loop {
        let levels = Arc::new(make(&job.source, job.indexed.as_ref()));
        let Ok(mut state) = shared.lock() else { return };
        state.release(job.cost);
        if let Some(slot) = state.by_mesh.get_mut(&job.key) {
            slot.1 = Some(levels);
        }
        let Some(next) = state.take_job() else {
            state.generation = state.generation.wrapping_add(1);
            return;
        };
        if job.cost == WORK_BUDGET || next.cost == WORK_BUDGET {
            state.generation = state.generation.wrapping_add(1);
        }
        job = next;
        start_worker(&mut state, shared);
    }
}

/// `mesh`'s levels: a quarter, a sixteenth and a sixty-fourth of its triangles, as far as it
/// simplifies (each level at least a fifth smaller than the one before, [`simplify::levels`]).
///
/// [`simplify::levels`]: solidcraft_engine::geom::simplify::levels
fn make(mesh: &Mesh, indexed: Option<&Body>) -> Vec<(Arc<SharedTris>, f32)> {
    let n = mesh.triangles.len();
    let targets: Vec<usize> = [4, 16, 64].into_iter().map(|d| n / d).filter(|t| *t >= 1).collect();
    let levels = match indexed.and_then(Body::triangle_mesh) {
        Some((positions, triangles)) => simplify::levels_from_indexed_facets(positions, triangles, &targets),
        None => simplify::levels(&mesh.positions, &mesh.triangles, &targets),
    };
    levels.iter().map(|l| (Arc::new(SharedTris::from_level(l)), l.error as f32)).collect()
}

#[cfg(test)]
#[path = "mesh_lod_tests.rs"]
mod tests;
