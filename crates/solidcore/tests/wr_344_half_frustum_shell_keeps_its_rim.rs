//! Regression test for WR-344 (tangency-labs/solidcore#469): with extended convex edges
//! `EdgeuseOffset` mates the offsets of an edge's two faces along it, but upstream's
//! `PiecewiseMerge` intersects the pair again, and the new curve can stop short of the
//! mated edge, splitting the rim with spurious vertices and tiny edges. A cone frustum
//! built as two half faces (`create_partial_cone` 0..180 and 180..360, united) is
//! shelled inward, open at its top. With `offset_extended_edges_mated` the mated pairs
//! are related and the shell has its seven faces, twelve edges and eight vertices; by
//! default these frustums come out with extra edges and vertices on the rim.
//!
//! The shell's volume is held to 1e-6 of the closed form: the frustum less the cavity
//! bounded by the inner cone (the wall moved `t` along its normal, its radius smaller by
//! `t / cos a`, `a` the semi-angle) between the bottom's offset plane `z = t` and the
//! open top.
//!
//! The test only exercises the flag; the flag's code is not touched here (#1818 is
//! about the same flag on a lofted operand).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell, volume};
use solidcore::boolean::api_brep::boolean_union;
use solidcore::math::{Vec3, PI};
use solidcore::primitive::api_primitives::create_partial_cone;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::{Brep, Kernel};

/// Bottom radius, top radius, height and wall thickness of each case.
const CASES: [(f64, f64, f64, f64); 3] = [
    (20.0, 25.0, 40.0, 2.0),
    (30.0, 20.0, 60.0, 2.0),
    (20.0, 30.0, 60.0, 3.0),
];

/// The two-half frustum shelled `t` inward, open at its top, with WR-344's `fixed`.
fn shelled(fixed: bool, (r0, r1, h, t): (f64, f64, f64, f64)) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.offset_extended_edges_mated = fixed;
    let o = Vec3::new(0.0, 0.0, 0.0);
    let a = create_partial_cone(&mut k, o, r0, r1, h, 0.0, 180.0).unwrap();
    let b = create_partial_cone(&mut k, o, r0, r1, h, 180.0, 360.0).unwrap();
    let part = boolean_union(&mut k, a, b).unwrap();
    let top = planar_faces_at(&k, &part, 2, h);
    assert!(!top.is_empty(), "the top");
    let s = shell(&mut k, part, -t, &top).unwrap();
    (k, s)
}

fn closed_form((r0, r1, h, t): (f64, f64, f64, f64)) -> f64 {
    let slope = (r1 - r0) / h;
    let inner = r0 - t / slope.atan().cos();
    let cube = |z: f64| (inner + slope * z).powi(3) / (3.0 * slope);
    PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0 - PI * (cube(h) - cube(t))
}

fn counts(b: &Brep) -> (usize, usize, usize) {
    (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    )
}

#[test]
fn wr_344_mated_rims_are_not_split_with_the_fix() {
    for case in CASES {
        let (k, mut b) = shelled(true, case);
        assert!(brep_is_manifold_solid(&b).unwrap(), "{case:?}");
        assert_eq!(counts(&b), (7, 12, 8), "{case:?}");
        let (v, exact) = (volume(&k, &mut b).unwrap(), closed_form(case));
        assert!(
            ((v - exact) / exact).abs() < 1e-6,
            "{case:?}: {v} against {exact}"
        );
    }
}

/// Upstream intersects the mated pairs again and splits the rim.
#[test]
fn wr_344_default_splits_the_rim() {
    for case in CASES {
        let (_, b) = shelled(false, case);
        let (f, e, v) = counts(&b);
        assert_eq!(f, 7, "{case:?}");
        assert!(e > 12 && v > 8, "{case:?}: {e} edges, {v} vertices");
    }
}
