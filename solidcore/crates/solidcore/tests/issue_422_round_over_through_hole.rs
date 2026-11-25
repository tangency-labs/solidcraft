//! tangency-labs/solidcore#422 (WR-212): an R4 round over a through hole returned success
//! with a body that is not a closed manifold solid (SolidCraft rig case fillets/324). The
//! round's rail on the top face runs over the hole, a cliff: since the cliff overflow is
//! the only path (tangency-labs/solidcraft#13841) the round is built as if the top went
//! on, and only what lies in the block is removed, as the issue's "Expected" asks. Before
//! that, keep features (the default since #2426) refused it.
//!
//! The case is the rig's: a 40 x 20 x 10 block minus a through hole of radius 3 about
//! (20, 14.5), rounded R4 along its top long edge at y = 20. The round covers y = 16..20
//! and the hole reaches y = 17.5.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel, Result};

/// The block with its hole, and its top long edge at y = 20.
fn block(k: &mut Kernel) -> (Brep, EdgeId) {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 20.0, 10.0).unwrap();
    let hole = create_cylinder(k, Point3::new(20.0, 14.5, -1.0), 3.0, 12.0).unwrap();
    let b = boolean_difference(k, b, hole).unwrap();
    let top: Vec<EdgeId> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            let (Some(s), Some(t)) = (s, t) else {
                return false;
            };
            let (p, q) = (vertex::point(&b, s).unwrap(), vertex::point(&b, t).unwrap());
            [p, q]
                .iter()
                .all(|v| (v.y - 20.0).abs() < 1e-9 && (v.z - 10.0).abs() < 1e-9)
                && (p.x - q.x).abs() > 39.0
        })
        .collect();
    assert_eq!(top.len(), 1);
    (b, top[0])
}

/// The block rounded with radius `r`: the block after it and the round's status.
fn round(r: f64) -> (Brep, Result<()>) {
    let mut k = Kernel::default();
    let (mut b, top) = block(&mut k);
    let result = fillet_edges(&mut k, &mut b, &[top], r, 1, 1, 1.0);
    (b, result)
}

/// A round clear of the hole (R2, the hole's wall at y = 17.5) still builds a solid whose
/// curves keep their owners (the rollback copy must not leave the kept result's curves
/// without owner, Test #61).
#[test]
fn issue_422_round_clear_of_the_hole_is_kept() {
    let (mut b, result) = round(2.0);
    result.unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    let mut k = Kernel::default();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(!lines.iter().any(|l| l.contains("Test #61")), "{lines:?}");
}

/// The R4 round into the hole is a cliff overflow: a closed manifold solid, Level 2
/// clean, of volume `40*20*10 - pi*3^2*10 - 40*(1 - pi/4)*4^2 + O`, where `O` is the part
/// of the round's corner inside the hole (removed with the hole, so counted back):
/// `O = int_16^17.5 g(y) h(y) dy`, the hole's chord `g = 2 sqrt(3^2 - (y - 14.5)^2)` (falls
/// on [16, 17.5]) times the corner's depth `h = 4 - sqrt(4^2 - (y - 16)^2)` (rises). On
/// each step `[a, b]` the integrand lies between `g(b) h(a)` and `g(a) h(b)`: summed,
/// they enclose `O` (= 0.37582964, the fixture's 7580.287315387334 in all). The result's
/// edges where the round meets the hole are fitted intersections (tolerance 4e-5), so its
/// volume is held to the closed form at 1e-8 V, as the other cliff overflows
/// (`wr631_cliff_overflow_construction`); measured 2.2e-5 (3e-9 V) above it, the
/// integrator converged to 1e-12.
#[test]
fn issue_422_round_into_the_hole_is_a_cliff_overflow() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use std::f64::consts::PI;
    let (mut b, result) = round(4.0);
    result.unwrap();
    assert!(is_manifold_solid(&b).unwrap());
    assert_eq!(b.faces().ids().count(), FACES_AFTER_R4);
    let k = Kernel::default();
    let mut kc = Kernel::default();
    let lines = check_brep(&mut kc, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    let g = |y: f64| 2.0 * (9.0 - (y - 14.5) * (y - 14.5)).max(0.0).sqrt();
    let h = |y: f64| 4.0 - (16.0 - (y - 16.0) * (y - 16.0)).max(0.0).sqrt();
    let n = 2_000_000;
    let w = 1.5 / f64::from(n);
    let (mut lo, mut hi, mut a) = (0.0, 0.0, 16.0);
    for i in 1..=n {
        let bb = 16.0 + f64::from(i) * w;
        lo += g(bb) * h(a) * w;
        hi += g(a) * h(bb) * w;
        a = bb;
    }
    // the rounding of 2e6 sums (each well under 1e-12 relative) is far below this
    let margin = 1e-6;
    let base = 40.0 * 20.0 * 10.0 - PI * 9.0 * 10.0 - 40.0 * (1.0 - PI / 4.0) * 16.0;
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-9, &attrs).unwrap();
    // the fixture's value (O to 1e-10) lies in the enclosure
    let fixture = 7_580.287_315_387_334 - base;
    assert!(
        lo - 1e-9 <= fixture && fixture <= hi + 1e-9,
        "{lo} {fixture} {hi}"
    );
    let slack = 1e-8 * v;
    let (v_lo, v_hi) = (base + lo - margin - slack, base + hi + margin + slack);
    assert!(v_lo <= v && v <= v_hi, "V {v} outside [{v_lo}, {v_hi}]");
}

/// The block's six faces, the hole's wall and the round.
const FACES_AFTER_R4: usize = 8;
