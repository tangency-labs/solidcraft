//! tangency-labs/solidcore#933, #838 (WR-380): upstream's G2 test of
//! `smgu_EvaluateCurveContinuity` compares the first normal with itself and the raw
//! second-derivative lengths. `Kernel::fixes.geometric_g2_continuity` compares the Frenet
//! curvatures and the two normals instead. These go through `Curve::evaluate_continuity`,
//! which reads the field from the `Kernel`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::curve::curve::p2::evaluate_continuity;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::p1::BSplineCurveForm;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_curves::{create_canonical_curve, CanonicalCurveParams};
use solidcore::Kernel;

/// The quadratic Bezier through the three control points over [0, 1].
fn quadratic(points: [Point3; 3]) -> BSplineCurve {
    create_canonical_curve(&CanonicalCurveParams {
        points: &points,
        knots: &[0.0, 1.0],
        knot_multiplicities: &[3, 3],
        weights: None,
        degree: 2,
        form: BSplineCurveForm::Unspecified,
    })
    .unwrap()
}

/// Continuity at the start of two curves, both read from the right.
fn continuity(k: &Kernel, a: &BSplineCurve, b: &BSplineCurve) -> ContinuityType {
    evaluate_continuity(k, a, 0.0, false, b, 0.0, false, 0.5)
        .unwrap()
        .continuity
}

/// #933: equal tangents and curvature lengths but opposite normals (a parabola and its
/// mirror image) are not G2. Upstream calls them C1G2G3
/// (the third derivatives of a quadratic vanish).
#[test]
fn issue_933_opposite_normals_are_not_g2() {
    let up = quadratic([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
    ]);
    let down = quadratic([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, -1.0, 0.0),
    ]);
    let mut k = Kernel::default();
    k.fixes.geometric_g2_continuity = true;
    assert_eq!(continuity(&k, &up, &down), ContinuityType::C1);
    // the same curve is still G2 with the fix on
    assert_eq!(continuity(&k, &up, &up), ContinuityType::C1C2C3);
}

/// Without the field the result is SMLib's.
#[test]
fn geometric_g2_continuity_is_off_by_default() {
    let up = quadratic([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
    ]);
    let down = quadratic([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, -1.0, 0.0),
    ]);
    let k = Kernel::default();
    assert!(!k.fixes.geometric_g2_continuity);
    assert_eq!(continuity(&k, &up, &down), ContinuityType::C1G2G3);
}

/// #838: the same parabola at twice the parameter speed is geometrically G2; upstream
/// compares the raw second-derivative lengths (2 against 8) and stops at G1.
#[test]
fn issue_838_reparametrised_curve_is_geometrically_g2() {
    let slow = quadratic([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(2.0, 1.0, 0.0),
    ]);
    // y = x^2 / 4 as for `slow`, with x = 4t instead of 2t
    let fast = quadratic([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(4.0, 4.0, 0.0),
    ]);
    let default = Kernel::default();
    let mut fixed = Kernel::default();
    fixed.fixes.geometric_g2_continuity = true;
    assert_eq!(continuity(&default, &slow, &fast), ContinuityType::G1);
    assert_eq!(continuity(&fixed, &slow, &fast), ContinuityType::G1G2G3);
}
