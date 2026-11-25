//! Regression test for #21889:
//! `solidcore`: an unclamped B-spline curve fails to evaluate at its natural ends `U[p]` and `U[m-p]`.
//!
//! When knots are unclamped (`U[0] < U[p]` and `U[m-p] < U[m]`), Cox-de Boor evaluation
//! at the curve's natural domain ends `t = U[p]` and `t = U[m-p]` must succeed rather
//! than throwing `at_domain_end`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test assertions

use solidcore::curve::curve::Curve;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_basic::Curve as Nurbs;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::{Kernel, Point3, Vec3};

fn cp(x: f64, y: f64) -> CPoint {
    CPoint {
        x,
        y,
        z: 0.0,
        w: NOW,
    }
}

fn cp3(x: f64, y: f64, z: f64) -> CPoint {
    CPoint { x, y, z, w: NOW }
}

#[test]
fn unclamped_quadratic_curve_evaluates_at_natural_domain_ends() {
    let k = Kernel::default();
    // 4 control points, degree 2, knots [0.0, 0.1, 0.2, 0.5, 0.8, 0.9, 1.0]
    // U[p] = U[2] = 0.2, U[m-p] = U[4] = 0.8
    let points = vec![cp(0.1, 0.2), cp(0.5, 0.9), cp(0.9, 0.3), cp(0.6, 0.1)];
    let knots = vec![0.0, 0.1, 0.2, 0.5, 0.8, 0.9, 1.0];
    let nurb = Nurbs::new(points, 2, knots).unwrap();
    let bsp = BSplineCurve::from_nurb(nurb, 2, true);
    let c: Curve = bsp.into();

    // Natural ends must evaluate successfully
    let p_start = c.evaluate_point(&k, 0.2).unwrap();
    let p_mid = c.evaluate_point(&k, 0.5).unwrap();
    let p_end = c.evaluate_point(&k, 0.8).unwrap();

    // Closed-form values from Cox-de Boor weights:
    // At t = 0.2: 0.75 * P0 + 0.25 * P1 = (0.2, 0.375, 0.0)
    let want_start = Point3::new(0.2, 0.375, 0.0);
    assert!((p_start.x - want_start.x).abs() < 1e-12);
    assert!((p_start.y - want_start.y).abs() < 1e-12);

    // At t = 0.8: 0.25 * P2 + 0.75 * P3 = (0.675, 0.15, 0.0)
    let want_end = Point3::new(0.675, 0.15, 0.0);
    assert!((p_end.x - want_end.x).abs() < 1e-12);
    assert!((p_end.y - want_end.y).abs() < 1e-12);

    // Interior parameter is finite and smooth
    assert!(p_mid.x.is_finite() && p_mid.y.is_finite());
}

#[test]
fn unclamped_cubic_curve_evaluates_at_natural_ends_and_matches_derivatives() {
    let k = Kernel::default();
    // 5 control points, degree 3, knots [0.0, 0.1, 0.2, 0.3, 0.7, 0.8, 0.9, 1.0]
    // U[p] = U[3] = 0.3, U[m-p] = U[4] = 0.7
    let points = vec![
        cp3(0.0, 1.0, 2.0),
        cp3(1.0, 3.0, 1.0),
        cp3(2.0, 0.0, 4.0),
        cp3(3.0, 2.0, 3.0),
        cp3(4.0, 1.0, 0.0),
    ];
    let knots = vec![0.0, 0.1, 0.2, 0.3, 0.7, 0.8, 0.9, 1.0];
    let nurb = Nurbs::new(points, 3, knots).unwrap();
    let bsp = BSplineCurve::from_nurb(nurb, 3, true);
    let c: Curve = bsp.into();

    let p_start = c.evaluate_point(&k, 0.3).unwrap();
    let p_end = c.evaluate_point(&k, 0.7).unwrap();

    // Verify 0-th derivative from evaluate agrees with evaluate_point
    let d_start = c.evaluate(&k, 0.3, 1, false, false).unwrap();
    let d_end = c.evaluate(&k, 0.7, 1, false, false).unwrap();

    assert!((Vec3::from(p_start) - d_start[0]).length() < 1e-12);
    assert!((Vec3::from(p_end) - d_end[0]).length() < 1e-12);
}

#[test]
fn clamped_curves_continue_to_hold_exact_end_control_points() {
    let k = Kernel::default();
    let points = vec![cp3(1.5, 2.5, 3.5), cp3(4.0, 5.0, 6.0), cp3(7.0, 8.0, 9.0)];
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let nurb = Nurbs::new(points, 2, knots).unwrap();
    let bsp = BSplineCurve::from_nurb(nurb, 3, true);
    let c: Curve = bsp.into();

    let p_start = c.evaluate_point(&k, 0.0).unwrap();
    let p_end = c.evaluate_point(&k, 1.0).unwrap();

    assert_eq!(p_start, Point3::new(1.5, 2.5, 3.5));
    assert_eq!(p_end, Point3::new(7.0, 8.0, 9.0));
}
