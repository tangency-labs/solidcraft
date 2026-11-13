//! Regression test for WR-540 (tangency-labs/solidcore#383; the fork's
//! tangency-labs/smlib@5533bbb, capi test `booleans tangent_cylinders`): a box minus a
//! cylinder that rests on one of its faces, box (20, 5, 5)-(35, 20, 32.5) minus a z
//! cylinder at (25, 12.5, 22.5) r 5 h 25, tangent to x = 20 along a line. The boolean
//! keeps the tangency as an edge four faces share, in the right radial order; their
//! binormals tie there, and `SmEdge::AssertValid` test 9 reads the order as out of
//! sequence. With `edge_check_orders_tangent_faces` the check orders the faces by where
//! they go and passes the edge.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Kernel, Vec3};

/// The failed report lines of SMLib's check (level 0) of the cut.
fn check_lines(fixed: bool) -> Vec<String> {
    let mut k = Kernel::default();
    k.fixes.edge_check_orders_tangent_faces = fixed;
    let block = create_box(&mut k, Vec3::new(20.0, 5.0, 5.0), 15.0, 15.0, 27.5).unwrap();
    let pin = create_cylinder(&mut k, Vec3::new(25.0, 12.5, 22.5), 5.0, 25.0).unwrap();
    let mut cut = boolean_difference(&mut k, block, pin).unwrap();
    let c = check_brep(&mut k, &mut cut, CheckLevel::Level0).unwrap();
    c.lines()
}

#[test]
fn wr_540_tangent_cylinder_edge_order_passes_with_the_fix() {
    // the body is closed and only non-manifold along the tangency: the check passes it
    let fixed = check_lines(true);
    assert!(fixed.is_empty(), "{fixed:?}");
}
