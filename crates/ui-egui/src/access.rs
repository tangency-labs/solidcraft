//! Accessibility Analysis: the model's height map seen from the access direction, for the GPU
//! to tell reachable surface from surface under something else.

use std::cell::RefCell;

use solidcraft_engine::SurfaceAnalysis;

use crate::SolveApp;
use crate::gpu::AccessMap;

thread_local! {
    /// The last map, by (model revision, direction).
    static CACHE: RefCell<Option<((u64, [u64; 3]), AccessMap)>> = const { RefCell::new(None) };
    static VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The depth map for the active Accessibility Analysis, if any.
pub fn map(app: &SolveApp) -> Option<AccessMap> {
    let Some(SurfaceAnalysis::Access { dir }) = app.session.analysis else { return None };
    let key = (app.session.revision, [dir.x.to_bits(), dir.y.to_bits(), dir.z.to_bits()]);
    if let Some(m) = CACHE.with(|c| c.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, m)| m.clone())) {
        return Some(m);
    }
    let st = crate::open_meshing::session_world(&app.session);
    let meshes: Vec<_> = st.bodies.iter().filter(|b| app.body_visible(&b.name)).map(|b| b.mesh()).collect();
    // Every new map gets a new version (the GPU uploads it once).
    let version = VERSION.with(|v| {
        v.set(v.get() + 1);
        v.get()
    });
    let m = AccessMap::build(&meshes.iter().map(|m| m.as_ref()).collect::<Vec<_>>(), dir, version)?;
    CACHE.with(|c| *c.borrow_mut() = Some((key, m.clone())));
    Some(m)
}
