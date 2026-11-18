//! Levels of detail for every mesh drawn as instances: coarser copies of a shared mesh
//! ([`solidcraft_engine::geom::simplify`]), made once per mesh off the window thread, so a
//! placement small on screen is drawn from far fewer triangles ([`crate::gpu::LOD_PX`] decides
//! which). Until a mesh's levels are made it is drawn in full; when they are, the viewport fills
//! them into the scene it drew, without building it again ([`MeshLods::generation`]).
//!
//! Display only: the reference mesh (picking, measuring, exports) is never simplified.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use solidcraft_engine::geom::Mesh;

use crate::gpu::SharedTris;

/// Meshes simplified at once.
const MAX_JOBS: usize = 2;

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
    queue: std::collections::VecDeque<(usize, Arc<Mesh>)>,
    /// Counts the times levels were made: the viewport fills them into its scene when it changes.
    generation: u64,
    making: usize,
}

impl MeshLods {
    /// `mesh`'s levels when made; otherwise `None`, and they are queued (once) for a worker. Every
    /// mesh gets them, whatever its size: a level is drawn only where its measured error is under
    /// [`crate::gpu::LOD_PX`] on screen, so a mesh that simplifies badly (a box) is drawn coarser
    /// only where it is a few pixels across, and one that cannot simplify gets none.
    pub(crate) fn levels(&self, mesh: &Arc<Mesh>) -> Option<Levels> {
        let key = Arc::as_ptr(mesh) as usize;
        let mut state = self.0.lock().ok()?;
        if let Some((_, levels)) = state.by_mesh.get(&key).filter(|(held, _)| Arc::ptr_eq(held, mesh)) {
            return levels.clone();
        }
        queue(&mut state, &self.0, key, mesh);
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

    /// Let go of the levels of meshes not in `used` (by address); those being made stay.
    pub(crate) fn retain(&self, used: &HashSet<usize>) {
        if let Ok(mut state) = self.0.lock() {
            state.by_mesh.retain(|k, (_, levels)| levels.is_none() || used.contains(k));
        }
    }
}

/// Queues `mesh` (at `key`) for a worker, starting one when fewer than [`MAX_JOBS`] run. Called
/// with the state locked, so a worker retiring ([`work`]) either takes this request or has already
/// let go of its place, which this one then starts again.
fn queue(state: &mut State, shared: &Arc<Mutex<State>>, key: usize, mesh: &Arc<Mesh>) {
    state.by_mesh.insert(key, (mesh.clone(), None));
    state.queue.push_back((key, mesh.clone()));
    if state.making < MAX_JOBS {
        state.making += 1;
        let shared = shared.clone();
        if std::thread::Builder::new().name("mesh-lod".into()).spawn(move || work(&shared)).is_err() {
            // No threads (the browser): drawn in full.
            state.making = state.making.saturating_sub(1);
            for (key, _) in std::mem::take(&mut state.queue) {
                if let Some(slot) = state.by_mesh.get_mut(&key) {
                    slot.1 = Some(Arc::new(Vec::new()));
                }
            }
        }
    }
}

/// A worker: makes the queued meshes' levels until none is left, then tells the viewport (once for
/// all it made), which fills them into its scene. It finds the queue empty and retires under one
/// lock, so a mesh queued meanwhile is never left without a worker.
fn work(shared: &Arc<Mutex<State>>) {
    loop {
        let Ok(mut state) = shared.lock() else { return };
        let Some((key, source)) = state.queue.pop_front() else {
            state.making = state.making.saturating_sub(1);
            state.generation = state.generation.wrapping_add(1);
            return;
        };
        drop(state);
        let levels = Arc::new(make(&source));
        if let Ok(mut state) = shared.lock()
            && let Some(slot) = state.by_mesh.get_mut(&key).filter(|(held, _)| Arc::ptr_eq(held, &source))
        {
            slot.1 = Some(levels);
        }
    }
}

/// `mesh`'s levels: a quarter, a sixteenth and a sixty-fourth of its triangles, as far as it
/// simplifies (each level at least a fifth smaller than the one before, [`simplify::levels`]).
///
/// [`simplify::levels`]: solidcraft_engine::geom::simplify::levels
fn make(mesh: &Mesh) -> Vec<(Arc<SharedTris>, f32)> {
    let n = mesh.triangles.len();
    let targets: Vec<usize> = [4, 16, 64].into_iter().map(|d| n / d).filter(|t| *t >= 1).collect();
    solidcraft_engine::geom::simplify::levels(&mesh.positions, &mesh.triangles, &targets)
        .iter()
        .map(|l| (Arc::new(SharedTris::from_level(l)), l.error as f32))
        .collect()
}

#[cfg(test)]
#[path = "mesh_lod_tests.rs"]
mod tests;
