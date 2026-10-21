//! Small conversions shared by the unit runners in [`crate::units`].

use solidcore::math::{Point2, Vec3};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::Brep;

/// A JSON `[x, y, z]` as a [`Vec3`].
#[must_use]
pub fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A JSON `[u, v]` as a [`Point2`].
#[must_use]
pub fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

/// A boolean as an output value: `1.0` or `0.0`.
#[must_use]
pub fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// Gives every edge curve and face surface of an imported B-rep the owner a native
/// B-rep's geometry has (its edge or face, by [`arena_identity`]; curves also the
/// B-rep), which the B-rep cache solves read (the import leaves geometry unowned).
pub fn own_arena_geometry(b: &mut Brep) {
    let edges: Vec<_> = b.edges().ids().collect();
    for id in edges {
        if let Some(curve) = b.edge_mut(id).ok().and_then(|e| e.curve.as_mut()) {
            let _ = curve.set_owner(
                Some(arena_identity(CacheObject::Edge(id))),
                Some(arena_identity(CacheObject::Brep)),
            );
        }
    }
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        if let Some(surface) = b.face_mut(id).ok().and_then(|f| f.surface.as_mut()) {
            let _ = surface.set_owner(Some(arena_identity(CacheObject::Face(id))));
        }
    }
}
