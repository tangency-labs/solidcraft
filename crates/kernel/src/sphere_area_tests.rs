//! The exact area of a cut sphere's faces (#22556): solidcore's per-face area ([`Body::face_area`]).

use std::f64::consts::PI;

use solidcore_record::Vec3;

use crate::{Body, BoolOp, FaceSurface, face_surface};

const R: f64 = 10.0;

fn ok<T>(r: crate::Result<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

fn cut(a: &Body, b: &Body) -> Body {
    match crate::boolean(a, b, BoolOp::Cut) {
        Ok(Some(r)) => r,
        other => panic!("cut: {other:?}"),
    }
}

fn ball() -> Body {
    ok(crate::sphere(Vec3::ZERO, R))
}

fn block(a: [f64; 3], b: [f64; 3]) -> Body {
    ok(crate::box_solid(Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2])))
}

/// The summed exact area of the body's faces on a sphere; every one must have an exact area.
fn sphere_area(b: &Body) -> f64 {
    let faces: Vec<usize> = (0..b.record().faces.len()).filter(|&f| matches!(face_surface(b, f), Some(FaceSurface::Sphere { .. }))).collect();
    assert!(!faces.is_empty(), "no face on a sphere");
    faces.iter().map(|&f| ok(b.face_area(f)).unwrap_or_else(|| panic!("face {f} has no exact area"))).sum()
}

fn assert_close(got: f64, want: f64) {
    assert!((got - want).abs() <= want * 1e-9, "area {got}, want {want}");
}

#[test]
fn a_zone_between_two_parallel_cuts_is_two_pi_r_h() {
    let b = cut(&cut(&ball(), &block([-20.0, -20.0, 3.0], [20.0, 20.0, 20.0])), &block([-20.0, -20.0, -20.0], [20.0, 20.0, -4.0]));
    assert_close(sphere_area(&b), 2.0 * PI * R * 7.0);
}

#[test]
fn a_cap_is_two_pi_r_h() {
    let b = cut(&ball(), &block([-20.0, -20.0, -20.0], [20.0, 20.0, 5.0]));
    assert_close(sphere_area(&b), 2.0 * PI * R * 5.0);
}

#[test]
fn the_big_side_of_one_cut_is_the_sphere_less_its_cap() {
    let b = cut(&ball(), &block([-20.0, -20.0, 5.0], [20.0, 20.0, 20.0]));
    assert_close(sphere_area(&b), 2.0 * PI * R * 15.0);
}

#[test]
fn a_face_with_corners_from_two_crossing_cuts_is_exact() {
    // Two cuts through the centre leave a quarter of the sphere, a lune with corners where the
    // cuts' circles cross, at (0, ±R, 0).
    let b = cut(&cut(&ball(), &block([0.0, -20.0, -20.0], [20.0, 20.0, 20.0])), &block([-20.0, -20.0, 0.0], [20.0, 20.0, 20.0]));
    assert_close(sphere_area(&b), PI * R * R);
    // Cutting out the quarter between the same planes leaves the other three quarters, with the
    // same corners turned the other way.
    let b = cut(&ball(), &block([0.0, -20.0, 0.0], [20.0, 20.0, 20.0]));
    assert_close(sphere_area(&b), 3.0 * PI * R * R);
}

#[test]
fn a_face_with_a_hole_is_the_zone_around_it() {
    // A band from z = -6 to 8: one face bounded by two loops.
    let b = cut(&cut(&ball(), &block([-20.0, -20.0, 8.0], [20.0, 20.0, 20.0])), &block([-20.0, -20.0, -20.0], [20.0, 20.0, -6.0]));
    assert_close(sphere_area(&b), 2.0 * PI * R * 14.0);
}

#[test]
fn a_face_has_the_same_area_on_a_ball_and_on_a_cavity() {
    // The same piece of sphere below z = 5, once bounding a ball (its normal out of the sphere)
    // and once bounding the hollow it leaves in a block (its normal into the sphere).
    let ball_side = cut(&ball(), &block([-20.0, -20.0, 5.0], [20.0, 20.0, 20.0]));
    let cavity_side = cut(&block([-20.0, -20.0, -20.0], [20.0, 20.0, 5.0]), &ball());
    let want = 2.0 * PI * R * 15.0;
    assert_close(sphere_area(&ball_side), want);
    assert_close(sphere_area(&cavity_side), want);
}
