//! Regression for #1054 (WR-474): `SmCurve::CalculateTightBoundingBox` adds a B-spline's
//! tangent-discontinuous knots, and always its last knot, whatever the queried interval is,
//! so the tight box of an edge on part of a NURBS curve reaches points the edge never does.
//!
//! The fix is opt-in, `Kernel::fixes.curve_tight_box_interval_knots`: with it on, only the
//! knots strictly inside the interval are added; with it off (the default) the box keeps the
//! pinned upstream result.
//!
//! Credits: `half_quarter_circle_stops_at_its_own_45_degree_point` is `PapaEcho13`'s test
//! from #1561, run both ways. The three tests from
//! `interval_knot_fix_is_opt_in_and_tightens_a_rational_arc_subdomain` on are `ian691`'s,
//! from #1110 (`tests/issue_1054_curve_tight_box.rs`), which proposed this flag; they are
//! kept as written there.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::p1::BSplineCurveForm;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::api_curves::{create_canonical_curve, CanonicalCurveParams};
use solidcore::{Kernel, Vec3};

fn arc(x_axis: [f64; 3], y_axis: [f64; 3], start_deg: f64, end_deg: f64) -> Curve {
    let frame = Axis2Placement::new(
        Vec3::from_array([0.0, 0.0, 0.0]),
        Vec3::from_array(x_axis),
        Vec3::from_array(y_axis),
    );
    Curve::from(
        BSplineCurve::create_circle_segment(
            3,
            &frame,
            50.0,
            start_deg,
            end_deg,
            NurbCircleParam::Quadratic,
        )
        .unwrap(),
    )
}

fn kernel(fixed: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.curve_tight_box_interval_knots = fixed;
    k
}

fn corners(bx: &Extent3d) -> [f64; 6] {
    [
        bx.u_min(),
        bx.v_min(),
        bx.w_min(),
        bx.u_max(),
        bx.v_max(),
        bx.w_max(),
    ]
}

fn assert_close(got: [f64; 6], want: [f64; 6]) {
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-6, "box {got:?}, expected {want:?}");
    }
}

#[test]
fn half_quarter_circle_stops_at_its_own_45_degree_point() {
    // A quadratic NURBS quarter circle of radius 50 from (50, 0) to (0, 50), queried over
    // the first half of its interval (0 to 45 deg).
    let c = arc([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.0, 90.0);
    let ivl = c.natural_interval();
    let half = Extent1d::from_min_max(ivl.min(), 0.5 * (ivl.max() - ivl.min()) + ivl.min());
    let r = 50.0 / 2.0_f64.sqrt();

    let fixed = c
        .calculate_tight_bounding_box(&kernel(true), &half)
        .unwrap();
    assert_close(corners(&fixed), [r, 0.0, 0.0, 50.0, r, 0.0]);

    // Upstream (flag off) also adds the last knot, the 90 deg end (0, 50).
    let pinned = c
        .calculate_tight_bounding_box(&kernel(false), &half)
        .unwrap();
    assert_close(corners(&pinned), [0.0, 0.0, 0.0, 50.0, 50.0, 0.0]);
}

#[test]
fn sphere_cap_seam_box_stays_on_the_cap() {
    // The issue's seam: R 50, a cap from the pole down to polar angle 30 deg. The seam edge
    // is the first 30 deg of a quarter-circle NURBS in the xz-plane that runs from polar
    // angle -30 deg, (25, 0, 43.30), over the pole to polar angle 60 deg, (-43.30, 0, 25).
    let c = arc([0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], -30.0, 60.0);
    let k = kernel(false);
    let ivl = c.natural_interval();
    // The parameter of the pole (x = 0; x falls along the curve).
    let (mut lo, mut hi) = (ivl.min(), ivl.max());
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if c.evaluate_point(&k, mid).unwrap().x > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let pole = 0.5 * (lo + hi);
    assert!(
        (pole - ivl.min() - 0.341 * (ivl.max() - ivl.min())).abs() < 1e-3,
        "pole at {pole} of {ivl:?}"
    );
    let seam = Extent1d::from_min_max(ivl.min(), pole);
    let z30 = 50.0 * 30.0_f64.to_radians().cos();

    let fixed = c
        .calculate_tight_bounding_box(&kernel(true), &seam)
        .unwrap();
    assert_close(corners(&fixed), [0.0, 0.0, z30, 25.0, 0.0, 50.0]);

    // Upstream takes in the last knot, (-43.30, 0, 25), which the seam never reaches.
    let pinned = c.calculate_tight_bounding_box(&k, &seam).unwrap();
    assert_close(corners(&pinned), [-z30, 0.0, 25.0, 25.0, 0.0, 50.0]);
}

// ---- From #1110 (ian691). ----

fn quarter_circle() -> Curve {
    Curve::from(
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Y),
            50.0,
            0.0,
            90.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap(),
    )
}

fn parameter_at_y(curve: &Curve, k: &Kernel, target: f64, start: f64, end: f64) -> f64 {
    let mut lo = start;
    let mut hi = end;
    for _ in 0..60 {
        let mid = (lo + hi) * 0.5;
        if curve.evaluate_point(k, mid).unwrap().y < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}

/// Compares all box coordinates at relative tolerance 1e-9, with an absolute
/// floor of 1e-9 for zero-valued coordinates.
fn assert_box_close(actual: &Extent3d, expected: &Extent3d) {
    let actual = [
        actual.min().x,
        actual.min().y,
        actual.min().z,
        actual.max().x,
        actual.max().y,
        actual.max().z,
    ];
    let expected = [
        expected.min().x,
        expected.min().y,
        expected.min().z,
        expected.max().x,
        expected.max().y,
        expected.max().z,
    ];
    for (actual, expected) in actual.into_iter().zip(expected) {
        let tolerance = 1.0e-9 * actual.abs().max(expected.abs()).max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }
}

fn degree_one_peaks() -> Curve {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 8.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(3.0, 4.0, 0.0),
        Point3::new(4.0, 0.0, 0.0),
        Point3::new(5.0, 20.0, 0.0),
        Point3::new(6.0, 0.0, 0.0),
    ];
    Curve::from(
        create_canonical_curve(&CanonicalCurveParams {
            points: &points,
            knots: &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            knot_multiplicities: &[2, 1, 1, 1, 1, 1, 2],
            weights: None,
            degree: 1,
            form: BSplineCurveForm::Unspecified,
        })
        .unwrap(),
    )
}

#[test]
fn interval_knot_fix_is_opt_in_and_tightens_a_rational_arc_subdomain() {
    let mut k = Kernel::default();
    assert!(!k.fixes.curve_tight_box_interval_knots);
    let curve = quarter_circle();
    let (knots, _) = curve.knots(&k, None).unwrap();
    let start = *knots.first().unwrap();
    let end = *knots.last().unwrap();
    let t30 = parameter_at_y(&curve, &k, 25.0, start, end);
    let interval = Extent1d::from_min_max(start, t30);
    let expected_x = (50.0_f64.powi(2) - 25.0_f64.powi(2)).sqrt();

    let legacy = curve.calculate_tight_bounding_box(&k, &interval).unwrap();
    assert_box_close(
        &legacy,
        &Extent3d::from_bounds(0.0, 0.0, 0.0, 50.0, 50.0, 0.0),
    );

    k.fixes.curve_tight_box_interval_knots = true;
    let tight = curve.calculate_tight_bounding_box(&k, &interval).unwrap();
    assert_box_close(
        &tight,
        &Extent3d::from_bounds(expected_x, 0.0, 0.0, 50.0, 25.0, 0.0),
    );

    let whole = Extent1d::from_min_max(start, end);
    let full_box = curve.calculate_tight_bounding_box(&k, &whole).unwrap();
    assert_box_close(
        &full_box,
        &Extent3d::from_bounds(0.0, 0.0, 0.0, 50.0, 50.0, 0.0),
    );
}

#[test]
fn degree_one_interval_keeps_interior_extremum_and_excludes_outside_discontinuities() {
    let curve = degree_one_peaks();
    let interval = Extent1d::from_min_max(2.5, 3.5);
    let mut k = Kernel::default();

    let legacy = curve.calculate_tight_bounding_box(&k, &interval).unwrap();
    assert_eq!(legacy.min(), Point3::new(1.0, 0.0, 0.0));
    assert_eq!(legacy.max(), Point3::new(6.0, 20.0, 0.0));

    k.fixes.curve_tight_box_interval_knots = true;
    let tight = curve.calculate_tight_bounding_box(&k, &interval).unwrap();
    assert_eq!(tight.min(), Point3::new(2.5, 2.0, 0.0));
    assert_eq!(tight.max(), Point3::new(3.5, 4.0, 0.0));
}

#[test]
fn interval_end_knots_are_covered_by_endpoints_and_full_domain_is_unchanged() {
    let curve = degree_one_peaks();
    let mut k = Kernel::default();
    k.fixes.curve_tight_box_interval_knots = true;

    // Knots at 2 and 4 are interval endpoints, so they must be included through
    // endpoint evaluation even though the opt-in knot rule is strict.
    let boundary = curve
        .calculate_tight_bounding_box(&k, &Extent1d::from_min_max(2.0, 4.0))
        .unwrap();
    assert_eq!(boundary.min(), Point3::new(2.0, 0.0, 0.0));
    assert_eq!(boundary.max(), Point3::new(4.0, 4.0, 0.0));

    let whole = Extent1d::from_min_max(0.0, 6.0);
    let fixed_full = curve.calculate_tight_bounding_box(&k, &whole).unwrap();
    k.fixes.curve_tight_box_interval_knots = false;
    let default_full = curve.calculate_tight_bounding_box(&k, &whole).unwrap();
    assert_eq!(fixed_full, default_full);
    assert_eq!(fixed_full.min(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(fixed_full.max(), Point3::new(6.0, 20.0, 0.0));
}
