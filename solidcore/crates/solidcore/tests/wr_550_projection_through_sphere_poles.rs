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
//!
//! The same step back into a pole, where the curve's direction lies just past the sphere's
//! seam: the derivatives are taken on the curve's meridian, not on the seam's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::CurveKind;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::primitive::api_primitives::{create_cylinder_surface, create_sphere};
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p1 as bp1;
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

/// At a tolerance of 1e-5, keeping the pole's own tangent, every curve stays within it
/// (4.8e-6). (Upstream's span into the pole, built with the neighbour's tangent, left the
/// sphere by 1.26e-5 until `FlushCurve` kept a short last span whose merge fails the
/// trace's span test, the blend corner's WR-697 addendum, now always on: #2400.)
#[test]
fn wr_550_pole_tangent_keeps_the_projection_within_a_tight_tolerance() {
    let tol = 1e-5;
    let fixed = worst_off_sphere(true, tol);
    assert!(fixed <= tol, "with the fix: {fixed:e} off the sphere");
}

/// The intersection of the cylinder of radius 50 about the Z axis (z 0..40, the half roll's
/// outside) with the r 6 ball about (0, 50 - 1e-6, 20), its seam meridian turned `turn` rad
/// about its axis from +X as the boolean tests turn it: the ball's poles lie 1e-6 inside the
/// cylinder, within the trace's tolerance of the curve, and the curve runs into each pole along
/// ±X. Returns the sphere's domain and the intersection.
fn roll_and_turned_ball(turn: f64) -> (Kernel, Extent2d, SurfaceIntersection) {
    let mut k = Kernel::default();
    let mut ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 6.0).unwrap();
    let (s, c) = turn.sin_cos();
    transform_matrix(
        &mut k,
        &mut ball,
        &[
            [c, s, 0.0, 0.0],
            [-s, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 50.0 - 1e-6, 20.0, 1.0],
        ],
        true,
    )
    .unwrap();
    let face = bp1::faces(&ball, None).unwrap()[0];
    let sphere = ball
        .face(face)
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .copy(&k)
        .unwrap();
    let cylinder =
        Surface::from(create_cylinder_surface(&k, Point3::new(0.0, 0.0, 0.0), 50.0, 40.0).unwrap());
    let cylinder_domain = cylinder.natural_uv_domain_with_nurb(&k).unwrap();
    let sphere_domain = sphere.natural_uv_domain_with_nurb(&k).unwrap();
    let out = cylinder
        .global_surface_intersect(
            &k,
            &cylinder_domain,
            &sphere,
            &sphere_domain,
            [true, true],
            Some(approx_tol3d(&k, 1e-5)),
            None,
        )
        .unwrap();
    (k, sphere_domain, out)
}

/// A trace that reaches a sphere's pole takes the pole's singular parameter along the curve's
/// tangent there (the solved parameter `s*` whose meridian holds the tangent `T`) and the
/// sphere's derivatives on it. With the ball's seam turned 0.01 rad, `T` (+X) lies on the
/// meridian at `s* = -0.00176`, just past the seam from the trace's side (`u = 0.0742`), so the
/// pole point keeps the seam's parameter `u = 0`. The derivatives were taken there too, 0.011 rad
/// off `T`'s meridian: the UV tangent at the pole then had a u-rate about 6e4 times its
/// v-rate, and the ball's UV trim of the short curve from the seam crossing into the pole had a
/// control point at u = -0.118, outside the sphere's domain.
///
/// Every UV curve on the ball is a non-rational B-spline, so it lies in the sphere's domain
/// when its control points do (convex hull): that is asserted, up to the rounding of the
/// coordinates at the domain's size (turned 0.02, one control point is at u = -7e-18). Taken across the seam on
/// `s*`'s own meridian, the UV tangent at the pole has no u-rate: the sphere's `S_u` is normal
/// to the meridian plane of `s*`, which holds `T`, so `T = a S_u + b S_v` has `a = 0`. The
/// computed `a` carries the direction solve's error (`SM_EFF_ZERO`, about 1e-12 rad) times
/// `|S_v| / |S_u|` at the stepped-back point, about 1e6 for a point 1e-6 from the pole, so
/// about 1e-6 · |b| (measured 1.0e-6 here); the bound 1e-3 · |b| is that with a margin of 1e3.
/// The ball turned -0.01 and 0 rad, where the pole's parameter is not clamped, are the
/// controls.
#[test]
fn a_trace_into_a_pole_past_the_seam_takes_the_derivatives_on_the_curves_meridian() {
    for turn in [0.01, 0.02, -0.01, 0.0] {
        let (k, domain, out) = roll_and_turned_ball(turn);
        assert!(
            !out.surface2_uv_curves.is_empty(),
            "turned {turn}: no curve"
        );
        for (i, uv) in out.surface2_uv_curves.iter().enumerate() {
            let uv = uv.as_ref().unwrap();
            let CurveKind::BSpline(b) = uv.kind() else {
                panic!("turned {turn}, curve {i}: not a B-spline");
            };
            assert!(!b.is_rational().unwrap(), "turned {turn}, curve {i}");
            // the domain, widened by the rounding of the control points' coordinates (the
            // Hermite data's conversion at the domain's size, 4 ulps of it)
            let round = 4.0 * f64::EPSILON * (1.0 + domain.max_dimension());
            for p in b.control_points().unwrap() {
                assert!(
                    p.x >= domain.min().x - round
                        && p.x <= domain.max().x + round
                        && p.y >= domain.min().y - round
                        && p.y <= domain.max().y + round,
                    "turned {turn}, curve {i}: control point ({}, {}) outside the sphere's domain",
                    p.x,
                    p.y
                );
            }
            // at a pole (v at the domain's end), the tangent runs along the meridian
            let ivl = uv.natural_interval();
            for (t, from_left) in [(ivl.min(), false), (ivl.max(), true)] {
                let e = uv.evaluate(&k, t, 1, from_left, false).unwrap();
                let (p, d) = (e[0], e[1]);
                if p.y != domain.min().y && p.y != domain.max().y {
                    continue;
                }
                assert!(
                    d.x.abs() <= 1e-3 * d.y.abs(),
                    "turned {turn}, curve {i}: UV tangent ({}, {}) at the pole ({}, {})",
                    d.x,
                    d.y,
                    p.x,
                    p.y
                );
            }
        }
        // the curve from the seam crossing into the top pole reaches it (turned 0.01, 0.02)
        if turn > 0.0 {
            let at_pole = out.surface2_uv_curves.iter().flatten().any(|uv| {
                let p = uv.evaluate_point(&k, uv.natural_interval().max()).unwrap();
                p.y == domain.max().y && p.x == domain.min().x
            });
            assert!(
                at_pole,
                "turned {turn}: no curve ends at the pole on the seam"
            );
        }
    }
}
