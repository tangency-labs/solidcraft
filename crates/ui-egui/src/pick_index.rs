//! Picking without scanning every triangle and edge: each body mesh's [`MeshIndex`] (its
//! triangles and edge segments in hierarchies), made the first time a pick reaches the body and
//! kept while the mesh is. The viewport's picker asks it for the nearest triangle along the
//! cursor's ray and for the edge segments near that ray, then runs its exact screen tests on
//! those alone. A large mesh's index takes a while to make (a third of a second for a long
//! modelled thread, #3745), so it is made on a worker thread, and picks scan the mesh until it
//! is there: a scan takes a few milliseconds, the index a frame never waits for.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Pos2, vec2};
use solidcraft_engine::geom::{Mesh, MeshIndex, Vec3};

use crate::viewport::Proj;

/// Indexes by mesh address, each with its mesh held (so the address stays its own); `None`
/// while a worker is still making it.
#[derive(Default)]
pub(crate) struct PickIndexes(Arc<Mutex<Indexes>>, #[cfg(test)] Arc<TestGate>);

/// Lets a test hold the worker before it starts making an index, so "still being made" can be
/// observed without racing the clock. Open unless a test closes it.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestGate {
    closed: Mutex<bool>,
    changed: std::sync::Condvar,
}

#[cfg(test)]
impl TestGate {
    fn wait_open(&self) {
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            closed = self.changed.wait(closed).unwrap();
        }
    }
    pub(crate) fn set_closed(&self, value: bool) {
        *self.closed.lock().unwrap() = value;
        self.changed.notify_all();
    }
}

type Indexes = HashMap<usize, (Arc<Mesh>, Option<Arc<MeshIndex>>)>;

/// Meshes up to this many triangles and edge points get their index at once (it takes well under
/// a millisecond); larger ones on a worker.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const AT_ONCE: usize = 20_000;

impl PickIndexes {
    /// The index of `mesh`, or `None` while it is being made (the caller scans the mesh).
    pub(crate) fn of(&self, mesh: &Arc<Mesh>) -> Option<Arc<MeshIndex>> {
        let key = Arc::as_ptr(mesh) as usize;
        let Ok(mut map) = self.0.lock() else { return Some(Arc::new(MeshIndex::new(mesh))) };
        if let Some((_, index)) = map.get(&key) {
            return index.clone();
        }
        let size = mesh.triangles.len() + mesh.edges.iter().map(Vec::len).sum::<usize>();
        #[cfg(not(target_arch = "wasm32"))]
        if size > AT_ONCE {
            map.insert(key, (mesh.clone(), None));
            let (indexes, source) = (self.0.clone(), mesh.clone());
            #[cfg(test)]
            let gate = self.1.clone();
            let started = std::thread::Builder::new().name("pick-index".into()).spawn(move || {
                #[cfg(test)]
                gate.wait_open();
                let index = Arc::new(MeshIndex::new(&source));
                if let Ok(mut map) = indexes.lock()
                    && let Some(slot) = map.get_mut(&key)
                {
                    slot.1 = Some(index);
                }
            });
            if started.is_ok() {
                return None;
            }
        }
        let _ = size;
        let index = Arc::new(MeshIndex::new(mesh));
        map.insert(key, (mesh.clone(), Some(index.clone())));
        Some(index)
    }

    /// Drops the indexes of meshes `keep` no longer names (a new model state).
    pub(crate) fn retain(&self, keep: impl Fn(usize) -> bool) {
        if let Ok(mut map) = self.0.lock() {
            map.retain(|k, _| keep(*k));
        }
    }

    #[cfg(test)]
    pub(crate) fn gate(&self) -> Arc<TestGate> {
        self.1.clone()
    }

    /// Indexes still being made by a worker.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> usize {
        self.0.lock().map_or(0, |m| m.values().filter(|(_, i)| i.is_none()).count())
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.lock().map_or(0, |m| m.len())
    }
}

/// How far from the ray through `pos` a point may lie and still come within `px` points of it on
/// screen: `a + b·t` at distance `t` along the ray (constant for an orthographic view, widening
/// with depth in perspective), with room to spare.
pub(crate) fn reach(proj: &Proj, pos: Pos2, px: f32) -> (f64, f64) {
    let (o, d) = proj.ray(pos);
    let (mut a, mut b) = (0.0f64, 0.0f64);
    for step in [vec2(px, 0.0), vec2(0.0, px), vec2(-px, 0.0), vec2(0.0, -px)] {
        let (o2, d2) = proj.ray(pos + step);
        a = a.max(o2.dist(o));
        b = b.max(d2.dist(d));
    }
    // A screen offset of `px` along a diagonal, and rounding: half again.
    (a * 1.5, b * 1.5)
}

/// The edge segments of `index` that may come within reach of the ray, as (edge, first point),
/// in edge order and then along each edge: the order a scan of every edge would meet them in.
pub(crate) fn segments_near(index: Option<&MeshIndex>, mesh: &Mesh, o: Vec3, d: Vec3, (a, b): (f64, f64)) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    let Some(index) = index.filter(|_| !scanning()) else {
        for (ei, e) in mesh.edges.iter().enumerate() {
            v.extend((0..e.len().max(2) - 1).map(|si| (ei, si)));
        }
        return v;
    };
    index.segments_near(o, d, a, b, |e, s| v.push((e, s)));
    v.sort_unstable();
    v
}

/// The nearest triangle of `mesh` along the ray, through its index.
pub(crate) fn raycast(index: Option<&MeshIndex>, mesh: &Mesh, o: Vec3, d: Vec3) -> Option<(f64, usize)> {
    match index.filter(|_| !scanning()) {
        Some(index) => index.raycast(mesh, o, d),
        None => mesh.raycast(o, d),
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: pick by scanning every triangle and edge, as before the indexes, to compare.
    pub(crate) static SCAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn scanning() -> bool {
    #[cfg(test)]
    {
        SCAN.with(|s| s.get())
    }
    #[cfg(not(test))]
    {
        false
    }
}

#[cfg(test)]
#[path = "pick_index_tests.rs"]
mod tests;
