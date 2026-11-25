//! #17062: `Ellipse::adjust_step_interval` refused a converted NURBS interval of zero
//! length but accepted a positive one made only of the conversion's rounding. The
//! rebuilt NURB then spanned an interval that is empty in exact arithmetic (or one the
//! conversion cannot resolve), and the next reparameterisation divided by it.
//!
//! The issue's own row (goldens `surface.torus` `issue1433_torus_perturb_corner_1e-12`)
//! no longer reaches this since #17008, so the cases are built on a public unit circle.
//! The bound refused (the doc comment of `adjust_step_interval`) is `gamma_8 (P0 + P1)`
//! for the NURBS length, `P = A + L` here (`A` the largest knot, `L` the knot span),
//! and `gamma_8 (Θ0 + Θ1)` for the STEP length, `Θ = |s| + 360` here:
//! - knots `[1e6, 1e6 + 1]`: the converter's last step `(end - start) t + start` rounds
//!   at 1e6 (one ulp 1.16e-10). STEP `[200, 200 + 3e-8]` degrees is 8.3e-11 of NURBS
//!   parameter, under one ulp, and was installed as a one-ulp interval (1.16e-10);
//!   `[10, 10 + 1e-7]` (2.8e-10) as two ulps (2.33e-10). Both are under the bound
//!   `gamma_8 * 2 (1e6 + 2) = 1.8e-9`: refused.
//! - knots `[0, 1]`: STEP `[10, 10 + 1e-13]` was installed as 2.0e-16 of NURBS
//!   parameter (2.8e-16 exact). 1e-13 degrees is under the rounding of the angle
//!   arithmetic, `gamma_8 * 2 (10 + 360) = 6.6e-13`: refused.
//!
//! Refusals leave the circle unchanged. Resolved intervals still install: 1e-6 degrees
//! on `[1e6, 1e6 + 1]` (2.8e-9, above 1.8e-9) and 1e-11 degrees on `[0, 1]`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::ellipse::Ellipse;
use solidcore::error::SolidError;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::create_analytic_circle;
use solidcore::Kernel;

/// A unit circle at the origin whose NURB runs over `knots`.
fn circle(k: &Kernel, knots: (f64, f64)) -> Ellipse {
    let mut c = create_analytic_circle(k, Point3::ZERO, 1.0).unwrap();
    c.edit_parameterization(k, &Extent1d::from_min_max(knots.0, knots.1), true, None)
        .unwrap();
    c
}

fn knots(c: &Ellipse) -> Vec<f64> {
    c.base().gw_nurb().unwrap().knots().to_vec()
}

/// `step` on a circle with NURB `knots`: the result and whether the circle is unchanged.
fn adjust(knots_in: (f64, f64), step: (f64, f64)) -> (Result<(), SolidError>, Vec<f64>) {
    let k = Kernel::default();
    let mut c = circle(&k, knots_in);
    let before = (knots(&c), c.step_interval());
    let result = c.adjust_step_interval(&k, &Extent1d::from_min_max(step.0, step.1), None);
    if result.is_err() {
        assert_eq!(
            (knots(&c), c.step_interval()),
            before,
            "{knots_in:?} {step:?}"
        );
    }
    (result, knots(&c))
}

/// The reason of an `InvalidInput` refusal.
fn reason(r: Result<(), SolidError>) -> String {
    match r {
        Err(SolidError::InvalidInput { reason, .. }) => reason.to_string(),
        other => panic!("not an InvalidInput refusal: {other:?}"),
    }
}

#[test]
fn converted_interval_of_rounding_only_is_refused() {
    for step in [(200.0, 200.0 + 3e-8), (10.0, 10.0 + 1e-7)] {
        let (r, _) = adjust((1e6, 1e6 + 1.0), step);
        assert!(
            reason(r).contains("NURBS interval no longer than the rounding"),
            "{step:?}"
        );
    }
}

#[test]
fn step_interval_below_the_angle_rounding_is_refused() {
    let (r, _) = adjust((0.0, 1.0), (10.0, 10.0 + 1e-13));
    assert!(reason(r).contains("STEP interval is no longer than the rounding"));
}

#[test]
fn resolved_small_intervals_still_install() {
    for (knots_in, step, length) in [
        ((1e6, 1e6 + 1.0), (10.0, 10.0 + 1e-6), 1e-6 / 360.0),
        ((0.0, 1.0), (10.0, 10.0 + 1e-11), 1e-11 / 360.0),
    ] {
        let (r, kn) = adjust(knots_in, step);
        r.unwrap_or_else(|e| panic!("{knots_in:?} {step:?}: {e}"));
        let got = kn[kn.len() - 1] - kn[0];
        // the conversion of a circle's angle is not linear; within 5% of the
        // proportional length, and positive
        assert!(
            got > 0.0 && (got / length - 1.0).abs() < 0.05,
            "{step:?}: {got:e}"
        );
    }
}
