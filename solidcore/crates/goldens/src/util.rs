//! Small conversions shared by the unit runners in [`crate::units`].

pub mod assert_reports;
pub mod primitive;
pub(crate) mod trim_curve;

use solidcore::geom::object::ObjectRef;
use solidcore::math::{Point2, Vec3};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::{Brep, EdgeId, FaceId};

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

/// Refuses a B-spline curve whose definition is invalid: the checks of upstream's
/// `N_CrvIsValid` other than its equal control point test, that is
/// `N_CrvCountsAreValid` (`n + p + 1 = m`, the degree limit, one dimension and one
/// rationality for every control point), `N_CrvWeightsAreValid` and
/// `N_KnotVectorIsValid`. A net that mixes the non-rational marker with weights is
/// read as rational by some NLib routines and as non-rational by others.
///
/// # Errors
///
/// The first failing check, as `curve: <error>`.
pub fn nurbs_curve_definition(
    c: &solidcore::nurbs::crv_basic::Curve,
) -> Result<(), crate::HarnessError> {
    c.counts_are_valid()
        .and_then(|()| c.weights_are_valid())
        .and_then(|()| c.knot_vector()?.validate(c.degree()))
        .map_err(|e| crate::HarnessError::new(format!("curve: {e}")))
}

/// [`nurbs_curve_definition`] of every inline B-spline curve in a case's inputs: each
/// object `{"bspline": {"cpts": [[x, y, z, w], ..], "p": .., "u": [..], ..}}`, at any
/// depth. The runner entry points check them; the runners' own Rust paths take any curve,
/// as the library does.
///
/// # Errors
///
/// The first invalid curve's, and a `bspline` object that is not such a curve.
pub fn inline_curve_definitions(v: &serde_json::Value) -> Result<(), crate::HarnessError> {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            if let Some(b) = map.get("bspline").filter(|b| b.get("cpts").is_some()) {
                let bad = || crate::HarnessError::new("curve: malformed inline B-spline");
                let cpts: Vec<[f64; 4]> =
                    serde_json::from_value(b.get("cpts").cloned().ok_or_else(bad)?)
                        .map_err(|_| bad())?;
                let p: usize = serde_json::from_value(b.get("p").cloned().ok_or_else(bad)?)
                    .map_err(|_| bad())?;
                let u: Vec<f64> = serde_json::from_value(b.get("u").cloned().ok_or_else(bad)?)
                    .map_err(|_| bad())?;
                let pw = cpts
                    .iter()
                    .map(|&[x, y, z, w]| solidcore::nurbs::math::CPoint::new(x, y, z, w))
                    .collect();
                let c = solidcore::nurbs::crv_basic::Curve::new(pw, p, u)
                    .map_err(|e| crate::HarnessError::new(format!("curve: {e}")))?;
                nurbs_curve_definition(&c)?;
            }
            map.values().try_for_each(inline_curve_definitions)
        }
        Value::Array(items) => items.iter().try_for_each(inline_curve_definitions),
        _ => Ok(()),
    }
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
    own_edge_curves(
        b,
        &|id| arena_identity(CacheObject::Edge(id)),
        Some(arena_identity(CacheObject::Brep)),
    );
    own_face_surfaces(b, &|id| arena_identity(CacheObject::Face(id)));
}

/// Gives every edge curve of an imported B-rep its edge as owner, under `edge_key`:
/// the name the runner hands the library for that edge (its `key_of` or cache keys).
/// The topology solver's B-rep trees and edge lookups compare owners with those names;
/// `brep` is the owner edge's B-rep as `Curve::set_owner` takes it.
pub fn own_edge_curves(
    b: &mut Brep,
    edge_key: &dyn Fn(EdgeId) -> ObjectRef,
    brep: Option<ObjectRef>,
) {
    let edges: Vec<_> = b.edges().ids().collect();
    for id in edges {
        if let Some(curve) = b.edge_mut(id).ok().and_then(|e| e.curve.as_mut()) {
            let _ = curve.set_owner(Some(edge_key(id)), brep);
        }
    }
}

/// Gives every face surface of an imported B-rep its face as owner, under `face_key`
/// (as [`own_edge_curves`] for edges).
pub fn own_face_surfaces(b: &mut Brep, face_key: &dyn Fn(FaceId) -> ObjectRef) {
    let faces: Vec<_> = b.faces().ids().collect();
    for id in faces {
        if let Some(surface) = b.face_mut(id).ok().and_then(|f| f.surface.as_mut()) {
            let _ = surface.set_owner(Some(face_key(id)));
        }
        let _ = b.sync_face_surface_tolerance(id);
    }
}
