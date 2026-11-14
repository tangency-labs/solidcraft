//! WR-211 (#428): a STEP-interval move across a full turn on a public ellipse.
//!
//! `Ellipse::adjust_step_interval` is reached through the public curve factories
//! ([`create_analytic_circle`], [`create_ellipse`]) and steps a `[0, 360]` arc to
//! `[360, 540]`. Upstream's polar table clamps both new ends to the arc's end, so the
//! NURBS interval collapses to one parameter and the next reparameterisation gives the
//! interior NaN knots of WR-211. The fork's turn-preserving mapping keeps the arc: half its NURB length, angles 0 to 180, every sampled point on the
//! curve and finite knots after the same reparameterisation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::{create_analytic_circle, create_ellipse};
use solidcore::Kernel;

/// A point on the unit circle (centre at the origin): its radial distance is 1.
const ON_CIRCLE: f64 = 1e-15;
/// A point's distance from its exact position on the curve (`evaluate_point`).
const ON_POINT: f64 = 1e-14;
/// A point on the 3 x 2 ellipse: the implicit residual
/// `(x/3)^2 + (y/2)^2 - 1` is zero.
const ON_ELLIPSE: f64 = 1e-15;

/// The requested STEP interval: a full turn on from `[0, 360]`.
fn past_a_turn() -> Extent1d {
    Extent1d::from_min_max(360.0, 540.0)
}

fn distance(p: Point3, q: Point3) -> f64 {
    (p - q).length()
}

/// The NURBS knots of the ellipse's B-spline base (public: `base()` + `gw_nurb()`).
fn knots(curve: &Ellipse) -> Vec<f64> {
    curve
        .base()
        .gw_nurb()
        .expect("an ellipse made by the public factories has a NURB")
        .knots()
        .to_vec()
}

/// The fork's turn-preserving mapping: the same public circle and ellipse keep angles 0 to 180 instead of collapsing.
///
/// The NURBS interval becomes `[1, 1.5]` (half the full-turn length, one NURBS length
/// per turn moved whole turns back), the points are the arc's start, quarter and end,
/// and the same reparameterisation leaves finite knots and on-curve points.
#[test]
fn a_full_turn_step_move_keeps_its_arc() {
    let k = Kernel::default();
    let past = past_a_turn();

    let mut circle = create_analytic_circle(&k, Point3::ZERO, 1.0).unwrap();
    circle.adjust_step_interval(&k, &past, None).unwrap();
    assert_eq!(circle.step_interval(), past);
    let half_turn = Extent1d::from_min_max(1.0, 1.5);
    assert_eq!(circle.base().natural_interval(), half_turn);
    assert_eq!(circle.base().natural_interval().length(), 0.5);
    assert_eq!(knots(&circle), [1.0, 1.0, 1.0, 1.25, 1.25, 1.5, 1.5, 1.5]);
    // angle 360 -> 0 degrees, 450 -> 90, 540 -> 180
    for (t, expected) in [
        (1.0, Point3::new(1.0, 0.0, 0.0)),
        (1.25, Point3::new(0.0, 1.0, 0.0)),
        (1.5, Point3::new(-1.0, 0.0, 0.0)),
    ] {
        let p = circle.base().evaluate_point(t).unwrap();
        assert!(
            distance(p, expected) < ON_POINT,
            "t = {t}: {p:?} is not {expected:?}"
        );
        assert!(
            ((p - Point3::ZERO).length() - 1.0).abs() < ON_CIRCLE,
            "{p:?}"
        );
    }

    let mut ellipse = create_ellipse(&k, Point3::ZERO, 3.0, 2.0).unwrap();
    ellipse.adjust_step_interval(&k, &past, None).unwrap();
    assert_eq!(ellipse.step_interval(), past);
    assert_eq!(ellipse.base().natural_interval(), half_turn);
    assert_eq!(knots(&ellipse), [1.0, 1.0, 1.0, 1.25, 1.25, 1.5, 1.5, 1.5]);
    for (t, expected) in [
        (1.0, Point3::new(3.0, 0.0, 0.0)),
        (1.25, Point3::new(0.0, 2.0, 0.0)),
        (1.5, Point3::new(-3.0, 0.0, 0.0)),
    ] {
        let p = ellipse.base().evaluate_point(t).unwrap();
        assert!(
            distance(p, expected) < ON_POINT,
            "t = {t}: {p:?} is not {expected:?}"
        );
        let residual = (p.x / 3.0) * (p.x / 3.0) + (p.y / 2.0) * (p.y / 2.0) - 1.0;
        assert!(
            residual.abs() < ON_ELLIPSE,
            "{p:?} off the ellipse: {residual}"
        );
    }

    // The same second edit that poisons the default curve: finite, still the half arc.
    circle.edit_parameterization(&k, &past, true, None).unwrap();
    assert_eq!(circle.base().natural_interval(), past);
    assert_eq!(
        knots(&circle),
        [360.0, 360.0, 360.0, 450.0, 450.0, 540.0, 540.0, 540.0]
    );
    let quarter = std::f64::consts::FRAC_1_SQRT_2;
    for (t, expected) in [
        (360.0, Point3::new(1.0, 0.0, 0.0)),
        (405.0, Point3::new(quarter, quarter, 0.0)),
        (450.0, Point3::new(0.0, 1.0, 0.0)),
        (495.0, Point3::new(-quarter, quarter, 0.0)),
        (540.0, Point3::new(-1.0, 0.0, 0.0)),
    ] {
        let p = circle.base().evaluate_point(t).unwrap();
        assert!(
            distance(p, expected) < ON_POINT,
            "t = {t}: {p:?} is not {expected:?}"
        );
        assert!(
            ((p - Point3::ZERO).length() - 1.0).abs() < ON_CIRCLE,
            "{p:?}"
        );
    }

    ellipse
        .edit_parameterization(&k, &past, true, None)
        .unwrap();
    assert_eq!(ellipse.base().natural_interval(), past);
    assert_eq!(
        knots(&ellipse),
        [360.0, 360.0, 360.0, 450.0, 450.0, 540.0, 540.0, 540.0]
    );
    let mid = ellipse.base().evaluate_point(450.0).unwrap();
    assert!(
        distance(mid, Point3::new(0.0, 2.0, 0.0)) < ON_POINT,
        "{mid:?}"
    );
    let residual = (mid.x / 3.0) * (mid.x / 3.0) + (mid.y / 2.0) * (mid.y / 2.0) - 1.0;
    assert!(residual.abs() < ON_ELLIPSE, "{mid:?}");
}
