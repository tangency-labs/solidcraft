//! tangency-labs/solidcore#423: an R1 round on the rim of an r2 sphere cut at z = 1
//! builds at the expected volume (7π + π²/2), but `check_brep` reported `SmBrep` test
//! #61 ("Bad Edge fails pE->ValidateGeometry(FALSE) check") on its edges, with
//! `fillet_cleanup_keeps_target_faces` on or off. Any in-place fillet did the same (an
//! R0.5 round on a box edge too): the fillet insertion names the curves it binds to the
//! target's edges by the session's kernel identities, and the global merge never
//! rebound them to the target's own naming (the piecewise merge does), so the curve
//! owner test of `SmEdge::ValidateGeometry` failed. Upstream's owners are pointers, so
//! the C++ never fails it: a port divergence, fixed unconditionally.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::{create_box, create_sphere};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The failed checks of `b` as (reporting type code, test number).
fn failures(k: &mut Kernel, b: &mut Brep) -> Vec<(i64, u64)> {
    let check = check_brep(k, b, CheckLevel::Level2).unwrap();
    check
        .failures()
        .map(|r| (r.reporting_type, r.test_index))
        .collect()
}

/// The r2 sphere about the origin less everything above z = 1, rounded R1 along its
/// rim, with `fillet_cleanup_keeps_target_faces` as `keep` says.
fn rounded_cap(keep: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_cleanup_keeps_target_faces = keep;
    let sphere = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap();
    let cut = create_box(&mut k, Point3::new(-3.0, -3.0, 1.0), 6.0, 6.0, 3.0).unwrap();
    let mut b = boolean_difference(&mut k, sphere, cut).unwrap();
    let rim: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s, t]
                .iter()
                .flatten()
                .all(|&v| (vertex::point(&b, v).unwrap().z - 1.0).abs() < 1e-6)
        })
        .collect();
    assert!(!rim.is_empty());
    fillet_edges(&mut k, &mut b, &rim, 1.0, 1, 1, 1.0).unwrap();
    (k, b)
}

#[test]
fn issue_423_rim_round_edges_pass_the_edge_geometry_check() {
    for keep in [false, true] {
        let (mut k, mut b) = rounded_cap(keep);
        assert!(is_manifold_solid(&b).unwrap());
        let expected = 7.0 * PI + PI * PI / 2.0;
        let v = volume(&k, &mut b);
        assert!((v - expected).abs() < 1e-3 * expected, "volume {v}");
        let failed = failures(&mut k, &mut b);
        // SmBrep (16011) test #61: an edge fails SmEdge::ValidateGeometry.
        assert!(
            !failed.contains(&(16011, 61)),
            "keep {keep}: edge geometry check fails: {failed:?}"
        );
    }
}

/// The same owner check on a plain box round: the whole check passes after it.
#[test]
fn issue_423_box_edge_round_checks_valid() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 3.0, 2.0).unwrap();
    assert_eq!(failures(&mut k, &mut b), vec![]);
    let e = b.edges().ids().next().unwrap();
    fillet_edges(&mut k, &mut b, &[e], 0.5, 1, 1, 1.0).unwrap();
    assert_eq!(failures(&mut k, &mut b), vec![]);
}
