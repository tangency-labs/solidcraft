//! Regression test for WR-550 (the fork's tangency-labs/smlib@8d95600, capi sweeps "a
//! line projected through a sphere's poles"): a line along X at z = 40 projected along
//! -Z onto a sphere of radius 20 about the origin, at tolerance 4e-5. The projection
//! runs through both poles. `StepBackFromSingularity` copied the previous point's
//! tangent over the pole's exact one and the span into the pole came out 2.02e-4 off
//! the sphere; with `ssi_pole_point_keeps_its_tangent` every curve stays on the sphere
//! within the tolerance.
//!
//! In the port both settings stay within 4e-5 (upstream 2.3e-5, fixed 9.5e-6 off the
//! sphere), so the measurable is pinned at a tolerance of 1e-5, where upstream's span
//! into the pole leaves the sphere by more than the tolerance and the fixed one does not.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::Surface;
use solidcore::Kernel;

/// The largest distance from the sphere of the line along X at z = 40 projected along
/// -Z through the sphere's poles at tolerance `tol`, with WR-550's `fixed`.
fn worst_off_sphere(fixed: bool, tol: f64) -> f64 {
    let mut k = Kernel::default();
    k.fixes.ssi_pole_point_keeps_its_tangent = fixed;
    let sphere =
        Surface::from(Sphere::create_canonical(&k, &Axis2Placement::default(), 20.0).unwrap());
    let line = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &[Point3::new(-30.0, 0.0, 40.0), Point3::new(30.0, 0.0, 40.0)],
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let domain = sphere.natural_uv_domain_with_nurb(&k).unwrap();
    let (curves, _) = sphere
        .create_parallel_projection_curves(
            &k,
            &domain,
            &solidcore::curve::curve::Curve::from(line),
            Vec3::new(0.0, 0.0, -1.0),
            None,
            Some(tol),
            None,
        )
        .unwrap();
    assert!(!curves.is_empty());
    let mut worst: f64 = 0.0;
    for c in &curves {
        let iv = c.natural_interval();
        for i in 0..=2000 {
            let p = c
                .evaluate_point(&k, iv.evaluate(f64::from(i) / 2000.0))
                .unwrap();
            worst = worst.max((p.length() - 20.0).abs());
        }
    }
    worst
}

#[test]
fn wr_550_line_projected_through_sphere_poles_stays_on_the_sphere() {
    let tol = 4e-5;
    let worst = worst_off_sphere(true, tol);
    assert!(worst <= tol, "a projected curve is {worst} off the sphere");
}

/// At a tolerance of 1e-5 the span into the pole built with the neighbour's tangent
/// leaves the sphere by more than the tolerance (1.26e-5); keeping the pole's own
/// tangent, every curve stays within it (4.8e-6).
#[test]
fn wr_550_pole_tangent_keeps_the_projection_within_a_tight_tolerance() {
    let tol = 1e-5;
    let fixed = worst_off_sphere(true, tol);
    assert!(fixed <= tol, "with the fix: {fixed:e} off the sphere");
    let upstream = worst_off_sphere(false, tol);
    assert!(upstream > tol, "upstream: {upstream:e} off the sphere");
}
