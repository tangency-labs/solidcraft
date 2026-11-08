//! tangency-labs/solidcore#1523 (fork tangency-labs/smlib 54ca3e0, WR-561):
//! `SmFace::ComputeLoopOrientation`'s 3D path holds a loop's dimensionless area to a
//! floor of 1e-8 of the whole face's domain, so a loop far smaller than its face gets
//! no orientation.
//!
//! A 3000 x 1000 x 10 plate with a hole of radius 0.002 through it: on the top face the
//! hole is about 4e-12 of the face's domain. By default its loop's orientation is
//! unknown; with `loop_orientation_floor_from_loop_box` the floor is 1e-8 of the loop's
//! own box and the loop is oriented. The outer loop is oriented either way.
//!
//! The part is built with the flag on and only the orientation call is toggled: the
//! boolean orients loops itself, and without the flag it does not leave this top face
//! as one planar face with the hole (CI: no planar face at z = 10), so the default
//! case would fail before it reaches the call under test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::planar_faces_at;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::extent2d::Extent2d;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::face::p2::loops;
use solidcore::topology::face::p3::compute_loop_orientation_3d;
use solidcore::Kernel;

/// The top face's loops as (orientation, UV width of the deciding walk), outer first:
/// sorted by width, largest first.
fn top_loops(fix: bool) -> Vec<(OrientType, f64)> {
    let mut k = Kernel::default();
    k.fixes.loop_orientation_floor_from_loop_box = true;
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 3000.0, 1000.0, 10.0).unwrap();
    let pin = create_cylinder(&mut k, Point3::new(1500.0, 500.0, -1.0), 0.002, 12.0).unwrap();
    let part = boolean_difference(&mut k, plate, pin).unwrap();
    let top = planar_faces_at(&k, &part, 2, 10.0);
    assert_eq!(top.len(), 1);
    k.fixes.loop_orientation_floor_from_loop_box = fix;
    let mut out: Vec<(OrientType, f64)> = loops(&part, top[0], false, None)
        .unwrap()
        .into_iter()
        .map(|l| {
            let mut bounds = Extent2d::new();
            let o = compute_loop_orientation_3d(&k, &part, top[0], l, Some(&mut bounds)).unwrap();
            (o, bounds.x_length())
        })
        .collect();
    assert_eq!(out.len(), 2, "{out:?}");
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[test]
fn issue_1523_a_tiny_hole_is_not_oriented_by_default() {
    let loops = top_loops(false);
    assert_ne!(loops[0].0, OrientType::Unknown, "{loops:?}");
    assert_eq!(loops[1].0, OrientType::Unknown, "{loops:?}");
}

#[test]
fn issue_1523_a_tiny_hole_is_oriented_against_its_outer_loop() {
    let loops = top_loops(true);
    assert_ne!(loops[0].0, OrientType::Unknown, "{loops:?}");
    assert_ne!(loops[1].0, OrientType::Unknown, "{loops:?}");
    assert_ne!(loops[1].0, loops[0].0, "{loops:?}");
}
