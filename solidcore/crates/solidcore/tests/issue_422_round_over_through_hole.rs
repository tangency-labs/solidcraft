//! tangency-labs/solidcore#422 (WR-212): an R4 round over a through hole returned success
//! with a body that is not a closed manifold solid (SolidCraft rig case fillets/324). The
//! round would have to be trimmed by the hole's wall, which SMLib does not do. Keep
//! features, the default since #2426, refuses the R4 round before its result is built, as
//! the issue's "Expected" allows; a result that left the solid open would be refused by
//! `fillet_edges` itself (WR-212). The fork's trim by the hole's wall
//! (tangency-labs/smlib@3dbe54a) is not ported.
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

/// The R4 round into the hole is refused by keep features, and the block comes back a
/// closed manifold solid.
#[test]
fn issue_422_round_into_the_hole_is_refused() {
    let (b, result) = round(4.0);
    let error = result.unwrap_err();
    assert_eq!(
        error.operation(),
        "blend::fillet_executive::session::keep_features",
        "{error:?}"
    );
    assert!(is_manifold_solid(&b).unwrap());
}
