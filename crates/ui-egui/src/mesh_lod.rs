//! Levels of detail for large meshes drawn as instances: coarser copies of a shared mesh
//! ([`solidcraft_engine::geom::simplify`]), made once per mesh off the window thread, so a
//! placement small on screen is drawn from far fewer triangles ([`crate::gpu::LOD_PX`] decides
//! which). Until a mesh's levels are made it is drawn in full; when they are, the scene is built
//! again with them ([`MeshLods::generation`]).
//!
//! Display only: the reference mesh (picking, measuring, exports) is never simplified.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use solidcraft_engine::geom::Mesh;

use crate::gpu::SharedTris;

/// A mesh gets levels of detail from this many triangles.
pub(crate) const MIN_TRIANGLES: usize = 20_000;
/// Fewest triangles a level keeps.
const MIN_LEVEL: usize = 1_000;
/// Meshes simplified at once.
const MAX_JOBS: usize = 2;

/// A mesh's levels: coarser copies, finest first, each with its error (mm).
pub(crate) type Levels = Arc<[(Arc<SharedTris>, f32)]>;

/// The levels made (and being made), by mesh address. A weak handle guards its identity without
/// keeping the source alive after its definition is released.
#[derive(Clone, Default)]
pub(crate) struct MeshLods(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    by_mesh: HashMap<usize, (Weak<Mesh>, Option<Levels>)>,
    /// Counts the levels made: the scene is built again when it changes.
    generation: u64,
    making: usize,
}

impl MeshLods {
    /// `mesh`'s levels when made; otherwise `None`, and they are started (once) on a worker
    /// when one is free.
    pub(crate) fn levels(&self, mesh: &Arc<Mesh>) -> Option<Levels> {
        if mesh.triangles.len() < MIN_TRIANGLES {
            return None;
        }
        let key = Arc::as_ptr(mesh) as usize;
        let mut state = self.0.lock().ok()?;
        if let Some((_, levels)) = state.by_mesh.get(&key).filter(|(held, _)| std::ptr::eq(held.as_ptr(), Arc::as_ptr(mesh))) {
            return levels.clone();
        }
        if state.making >= MAX_JOBS {
            return None;
        }
        state.by_mesh.insert(key, (Arc::downgrade(mesh), None));
        state.making += 1;
        let (shared, source) = (self.0.clone(), mesh.clone());
        let started = std::thread::Builder::new().name("mesh-lod".into()).spawn(move || {
            let identity = Arc::downgrade(&source);
            let levels: Levels = make(&source).into();
            drop(source);
            if let Ok(mut state) = shared.lock() {
                if let Some(slot) = state.by_mesh.get_mut(&key).filter(|(held, _)| Weak::ptr_eq(held, &identity)) {
                    slot.1 = Some(levels);
                    state.generation = state.generation.wrapping_add(1);
                }
                state.making = state.making.saturating_sub(1);
            }
        });
        if started.is_err() {
            // No threads (the browser): drawn in full.
            state.making = state.making.saturating_sub(1);
            if let Some(slot) = state.by_mesh.get_mut(&key) {
                slot.1 = Some(Arc::from([]));
            }
        }
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

    /// A source temporarily drawn at another refinement keeps its levels. Release them once
    /// neither the document nor a worker holds that source.
    pub(crate) fn retain(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.by_mesh.retain(|_, (source, _)| source.strong_count() > 0);
        }
    }
}

/// `mesh`'s levels: a quarter, a sixteenth and a sixty-fourth of its triangles (none under
/// [`MIN_LEVEL`]).
fn make(mesh: &Mesh) -> Vec<(Arc<SharedTris>, f32)> {
    let n = mesh.triangles.len();
    let targets = [4, 16, 64].map(|d| n / d);
    let count = targets.iter().take_while(|n| **n >= MIN_LEVEL).count();
    solidcraft_engine::geom::simplify::levels(&mesh.positions, &mesh.triangles, targets.get(..count).unwrap_or_default())
        .iter()
        .map(|l| (Arc::new(SharedTris::from_level(l)), l.error as f32))
        .collect()
}

#[cfg(test)]
#[path = "mesh_lod_tests.rs"]
mod tests;
