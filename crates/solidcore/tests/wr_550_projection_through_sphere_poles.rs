//! Regression test for WR-550 (the fork's tangency-labs/smlib@8d95600, capi sweeps "a
//! line projected through a sphere's poles"): a line along X at z = 40 projected along
//! -Z onto a sphere of radius 20 about the origin, at tolerance 4e-5. The projection
//! runs through both poles. `StepBackFromSingularity` copied the previous point's
//! tangent over the pole's exact one and the span into the pole came out 2.02e-4 off
//! the sphere; with `ssi_pole_point_keeps_its_tangent` every curve stays on the sphere
//! within the tolerance.

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

#[test]
fn wr_550_line_projected_through_sphere_poles_stays_on_the_sphere() {
    let mut k = Kernel::default();
    k.fixes.ssi_pole_point_keeps_its_tangent = true;
    let tol = 4e-5;
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
            &solidcore::curve::curve::Curve::from(line.clone()),
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
        for i in 0..=200 {
            let p = c
                .evaluate_point(&k, iv.evaluate(f64::from(i) / 200.0))
                .unwrap();
            worst = worst.max((p.length() - 20.0).abs());
        }
    }
    assert!(worst <= tol, "a projected curve is {worst} off the sphere");
}
