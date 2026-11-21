//! Picking without scanning every triangle and edge: each body mesh's [`MeshIndex`] (its
//! triangles and edge segments in hierarchies), kept while the mesh is in use. The viewport
//! prepares the largest visible imported mesh during open. Other indexes start on first use.
//! Imported outlines have their own index, requested only by Measure. The picker asks for the nearest triangle along the
//! cursor's ray and for the edge segments near that ray, then runs its exact screen tests on
//! those alone. A large mesh's index is made on a worker thread. While it builds, explicit picks
//! can scan the mesh. Hover and origin feedback defer the scan for very large meshes.

use solidcraft_engine::geom::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Pos2, vec2};
use solidcore_record::Vec3;
use solidcraft_engine::geom::{Mesh, MeshIndex, PointCloudIndex};

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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Body,
    DisplayOutlines,
}

type Indexes = HashMap<(usize, Kind), (Arc<Mesh>, Option<Arc<MeshIndex>>)>;

/// Meshes up to this many triangles and edge points get their index at once (it takes well under
/// a millisecond); larger ones on a worker.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const AT_ONCE: usize = 20_000;

impl PickIndexes {
    /// The index of `mesh`, or `None` while it is being made.
    pub(crate) fn of(&self, mesh: &Arc<Mesh>) -> Option<Arc<MeshIndex>> {
        self.get(mesh, Kind::Body)
    }

    /// The index for edge picking, including display outlines when the tool allows them.
    pub(crate) fn for_edges(&self, mesh: &Arc<Mesh>) -> Option<Arc<MeshIndex>> {
        self.get(mesh, if mesh.display_only_edges { Kind::DisplayOutlines } else { Kind::Body })
    }

    /// Start a large scan's index before the cursor reaches it, on the native worker.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn prefetch(&self, mesh: &Arc<Mesh>) {
        if mesh.triangles.len() > AT_ONCE {
            let _ = self.of(mesh);
        }
    }

    fn get(&self, mesh: &Arc<Mesh>, kind: Kind) -> Option<Arc<MeshIndex>> {
        let key = (Arc::as_ptr(mesh) as usize, kind);
        let build = move |mesh: &Mesh| match kind {
            Kind::DisplayOutlines => MeshIndex::segments_only(mesh),
            Kind::Body if mesh.display_only_edges => MeshIndex::triangles_only(mesh),
            Kind::Body => MeshIndex::new(mesh),
        };
        let Ok(mut map) = self.0.lock() else { return Some(Arc::new(build(mesh))) };
        if let Some((_, index)) = map.get(&key) {
            return index.clone();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let size = match kind {
            Kind::DisplayOutlines => mesh.edges.iter().map(Vec::len).sum(),
            Kind::Body if mesh.display_only_edges => mesh.triangles.len(),
            Kind::Body => mesh.triangles.len() + mesh.edges.iter().map(Vec::len).sum::<usize>(),
        };
        #[cfg(not(target_arch = "wasm32"))]
        if size > AT_ONCE {
            map.insert(key, (mesh.clone(), None));
            let (indexes, source) = (self.0.clone(), mesh.clone());
            #[cfg(test)]
            let gate = self.1.clone();
            let started = std::thread::Builder::new().name("pick-index".into()).spawn(move || {
                #[cfg(test)]
                gate.wait_open();
                let index = Arc::new(build(&source));
                if let Ok(mut map) = indexes.lock()
                    && let Some(slot) = map.get_mut(&key)
                    && slot.1.is_none()
                {
                    slot.1 = Some(index);
                }
            });
            if started.is_ok() {
                return None;
            }
        }
        let index = Arc::new(build(mesh));
        map.insert(key, (mesh.clone(), Some(index.clone())));
        Some(index)
    }

    /// Drops the indexes of meshes `keep` no longer names (a new model state).
    pub(crate) fn retain(&self, keep: impl Fn(usize) -> bool) {
        if let Ok(mut map) = self.0.lock() {
            map.retain(|(mesh, _), _| keep(*mesh));
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

/// Clouds up to this many points get their [`PointCloudIndex`] immediately; larger ones build on a
/// worker thread so hovering never blocks the window thread.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) const CLOUD_AT_ONCE: usize = 100_000;

type CloudMap = HashMap<usize, (Arc<[f32]>, Option<Arc<PointCloudIndex>>)>;

/// Spatial picking indexes for point clouds, keyed by their shared `Arc<[f32]>` storage pointer so
/// moving, rotating or filtering a cloud reuses its index without rebuilding (#8933).
#[derive(Default)]
pub(crate) struct CloudPickIndexes(Arc<Mutex<CloudMap>>);

impl CloudPickIndexes {
    /// The picking index of `values`, or `None` while a worker is building it.
    pub(crate) fn of(&self, values: &Arc<[f32]>) -> Option<Arc<PointCloudIndex>> {
        let key = Arc::as_ptr(values).cast::<f32>() as usize;
        let Ok(mut map) = self.0.lock() else { return Some(Arc::new(PointCloudIndex::new(values))) };
        if let Some((_, index)) = map.get(&key) {
            return index.clone();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if values.len() / 3 > CLOUD_AT_ONCE {
            map.insert(key, (values.clone(), None));
            let (indexes, source) = (self.0.clone(), values.clone());
            let started = std::thread::Builder::new().name("cloud-pick-index".into()).spawn(move || {
                let index = Arc::new(PointCloudIndex::new(&source));
                if let Ok(mut map) = indexes.lock()
                    && let Some(slot) = map.get_mut(&key)
                    && slot.1.is_none()
                {
                    slot.1 = Some(index);
                }
            });
            if started.is_ok() {
                return None;
            }
        }
        let index = Arc::new(PointCloudIndex::new(values));
        map.insert(key, (values.clone(), Some(index.clone())));
        Some(index)
    }

    /// Drop indexes whose point arrays are no longer in the document.
    pub(crate) fn retain(&self, keep: impl Fn(usize) -> bool) {
        if let Ok(mut map) = self.0.lock() {
            map.retain(|k, _| keep(*k));
        }
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
