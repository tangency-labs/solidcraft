//! tangency-labs/solidcore#396 part 1 (WR-79): `SmSphere::IntersectWithPlane` and
//! `IntersectWithSphere` return one touching point whenever the gap to tangency is
//! within the tolerance. The circle's radius is `sqrt(2 R gap)`, though, so a circle of
//! radius 1e-4 (a gap of 5e-9 on a unit sphere) collapses to a point at its centre at a
//! tolerance of 1e-6 (ssi-corpus `c7_tiny_loop_sphere_1e-4`, `_1e-5`).
//!
//! With `Kernel::fixes.sphere_small_circle_not_touching` the point is returned only when
//! the circle's own radius is within the tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};

const TOL: f64 = 1e-6;

fn frame(z: f64) -> Axis2Placement {
    Axis2Placement::new(
        Vec3::new(0.0, 0.0, z),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    )
}

/// The unit sphere whose centre is `sqrt(1 - r^2)` above the plane `z = 0`, so they meet
/// in a circle of radius `r`, intersected with that plane.
fn sphere_plane(fix: bool, r: f64) -> SurfaceIntersection {
    let mut k = Kernel::default();
    k.fixes.sphere_small_circle_not_touching = fix;
    let sphere = Sphere::create_canonical(&k, &frame((1.0 - r * r).sqrt()), 1.0).unwrap();
    let plane = Plane::create_canonical(&frame(0.0)).unwrap();
    let (more, out) = sphere
        .intersect_with_plane(
            &k,
            &sphere.base().base().natural_uv_domain(),
            &plane,
            &plane.natural_uv_domain(),
            Some(TOL),
        )
        .unwrap();
    assert!(!more);
    out
}

/// Two unit spheres whose centres are `2 sqrt(1 - r^2)` apart, so they meet in a circle
/// of radius `r`.
fn sphere_sphere(fix: bool, r: f64) -> SurfaceIntersection {
    let mut k = Kernel::default();
    k.fixes.sphere_small_circle_not_touching = fix;
    let a = Sphere::create_canonical(&k, &frame(0.0), 1.0).unwrap();
    let b = Sphere::create_canonical(&k, &frame(2.0 * (1.0 - r * r).sqrt()), 1.0).unwrap();
    let (more, out) = a
        .intersect_with_sphere(
            &k,
            &a.base().base().natural_uv_domain(),
            &b,
            &b.base().base().natural_uv_domain(),
            Some(TOL),
        )
        .unwrap();
    assert!(!more);
    out
}

/// The largest distance of the curves' points from the z axis (the circle's radius).
fn max_radius(out: &SurfaceIntersection) -> f64 {
    let k = Kernel::default();
    let mut max = 0.0_f64;
    for c in &out.curves_3d {
        let ivl = c.natural_interval();
        for i in 0..=16 {
            let t = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 16.0;
            let p = c.evaluate_point(&k, t).unwrap();
            max = max.max((p.x * p.x + p.y * p.y).sqrt());
        }
    }
    max
}

fn is_one_touch(out: &SurfaceIntersection) -> bool {
    out.curves_3d.len() == 1 && out.curve_types == [TsectCurveType::Touching]
}

#[test]
fn sphere_plane_loop_above_tolerance_collapses_by_default() {
    for r in [1e-4, 1e-5] {
        let out = sphere_plane(false, r);
        assert!(is_one_touch(&out), "r = {r}: {:?}", out.curve_types);
    }
}

#[test]
fn sphere_plane_loop_above_tolerance_is_a_circle_with_the_fix() {
    for r in [1e-3, 1e-4, 1e-5] {
        let out = sphere_plane(true, r);
        assert!(!out.curves_3d.is_empty(), "r = {r}");
        assert!(
            out.curve_types
                .iter()
                .all(|t| *t == TsectCurveType::Crossing),
            "r = {r}: {:?}",
            out.curve_types
        );
        let got = max_radius(&out);
        assert!((got - r).abs() <= 1e-3 * r, "r = {r}: radius {got}");
    }
}

#[test]
fn sphere_plane_loop_below_tolerance_still_touches_with_the_fix() {
    let out = sphere_plane(true, 1e-7);
    assert!(is_one_touch(&out), "{:?}", out.curve_types);
}

#[test]
fn sphere_sphere_loop_above_tolerance_collapses_by_default() {
    for r in [1e-4, 1e-5] {
        let out = sphere_sphere(false, r);
        assert!(is_one_touch(&out), "r = {r}: {:?}", out.curve_types);
    }
}

#[test]
fn sphere_sphere_loop_above_tolerance_is_a_circle_with_the_fix() {
    for r in [1e-3, 1e-4, 1e-5] {
        let out = sphere_sphere(true, r);
        assert!(!out.curves_3d.is_empty(), "r = {r}");
        assert!(
            out.curve_types
                .iter()
                .all(|t| *t == TsectCurveType::Crossing),
            "r = {r}: {:?}",
            out.curve_types
        );
        let got = max_radius(&out);
        assert!((got - r).abs() <= 1e-3 * r, "r = {r}: radius {got}");
    }
}

#[test]
fn sphere_sphere_loop_below_tolerance_still_touches_with_the_fix() {
    let out = sphere_sphere(true, 1e-7);
    assert!(is_one_touch(&out), "{:?}", out.curve_types);
}
