//! The fork's 6eecf04 (tangency-labs/smlib): a B-spline radius law's corners,
//! `SmBSplineFilletLaw::GetCornerParameters` ([`FilletLaw::corner_parameters`]): the
//! interior knots of the law curve, inside its edge map, where its slope from the left and
//! from the right differ, mapped onto the filleted edge's interval either way round: where
//! the radius the law evaluates turns. An edge interval or edge map with no length, or not
//! finite, is refused. Every expected value is hand-derived from the law's control radii
//! and knots.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::error::SolidError;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};

/// The fork capi's `law_along`: the B-spline law of `degree` with the full `knots` and
/// control radii `radii`, the radius as x and the parameter as y (control points at the
/// Greville abscissae), reversed when the edge runs the other way.
fn spline_law(degree: usize, knots: &[f64], radii: &[f64], reverse: bool) -> FilletLaw {
    let n = knots.len();
    let kv: Vec<f64> = (0..n)
        .map(|i| {
            if reverse {
                1.0 - knots[n - 1 - i]
            } else {
                knots[i]
            }
        })
        .collect();
    let control: Vec<Vec3> = (0..radii.len())
        .map(|i| {
            let g: f64 = (1..=degree).map(|j| kv[i + j]).sum::<f64>() / degree as f64;
            let r = radii[if reverse { radii.len() - 1 - i } else { i }];
            Vec3::new(r, g, 0.0)
        })
        .collect();
    let mut unique: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &t in &kv {
        if unique.last() == Some(&t) {
            *mults.last_mut().unwrap() += 1;
        } else {
            unique.push(t);
            mults.push(1);
        }
    }
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &mults,
        knots: &unique,
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

#[test]
fn a_laws_corners_are_its_knots_where_the_slope_jumps() {
    // 4 -> 2 -> 4 over the knots 0 0 0.5 1 1: slope -4 before 0.5 and +4 after, so a corner
    // at 0.5, the middle of the edge's interval whichever way the edge runs
    let dip = spline_law(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[4.0, 2.0, 4.0], false);
    let corners = |law: &FilletLaw, from: f64, to: f64, reverse: bool| {
        law.corner_parameters(&Extent1d::from_min_max(from, to), reverse)
            .unwrap()
    };
    assert_eq!(corners(&dip, 0.0, 20.0, false), vec![10.0]);
    assert_eq!(corners(&dip, 2.0, 22.0, true), vec![12.0]);
    // 4 -> 2 -> 3 -> 2 over 0 0 0.25 0.75 1 1: slopes -8, +2, -4, corners at 0.25 and 0.75,
    // on [0, 8] at 2 and 6; reversed, 8 - 0.25 * 8 = 6 and 8 - 0.75 * 8 = 2, ascending
    let two = spline_law(
        1,
        &[0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
        &[4.0, 2.0, 3.0, 2.0],
        false,
    );
    assert_eq!(corners(&two, 0.0, 8.0, false), vec![2.0, 6.0]);
    assert_eq!(corners(&two, 0.0, 8.0, true), vec![2.0, 6.0]);
    // 4 -> 3 -> 2: the same slope -2 on both sides of 0.5, no corner
    let straight = spline_law(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[4.0, 3.0, 2.0], false);
    assert!(corners(&straight, 0.0, 20.0, false).is_empty());
    // a quadratic with a double knot at 0.5 (control radii 4, 3, 2, 3, 4): slopes -4 and
    // +4 there, a corner
    let quadratic = spline_law(
        2,
        &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        &[4.0, 3.0, 2.0, 3.0, 4.0],
        false,
    );
    assert_eq!(corners(&quadratic, 0.0, 20.0, false), vec![10.0]);
    // only the edge map's part of the law is the edge's: on [0.25, 1] the knot 0.5 is a
    // third of the way, at 1 on [0, 3]; on [0.5, 1] it is the map's end, no corner
    let mut mapped = dip.clone();
    mapped.edge_map = Extent1d::from_min_max(0.25, 1.0);
    let at = corners(&mapped, 0.0, 3.0, false);
    assert_eq!(at.len(), 1);
    assert!((at[0] - 1.0).abs() <= 1e-15, "{at:?}");
    mapped.edge_map = Extent1d::from_min_max(0.5, 1.0);
    assert!(corners(&mapped, 0.0, 3.0, false).is_empty());
}

/// A degree-1 law through `(radius, y)` control points at the knots `0 0 0.5 1 1`.
fn law_through(points: [(f64, f64); 3]) -> FilletLaw {
    let control: Vec<Vec3> = points.iter().map(|&(r, y)| Vec3::new(r, y, 0.0)).collect();
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &[2, 1, 2],
        knots: &[0.0, 0.5, 1.0],
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

#[test]
fn a_corner_is_where_the_radius_the_law_evaluates_turns() {
    // x runs 4, 3, 2 in equal steps (x' = -2 on both sides of 0.5) while y turns there
    // (0, 0.2, 1). The law's radius is x at the curve parameter the edge map gives, y
    // unread, so it has no corner: on [0, 20] its slope is -2 / 20 = -0.1 on both sides
    // of 10, and corner_parameters finds none. (x' / y' would jump there, -5 to -1.25.)
    let mut law = law_through([(4.0, 0.0), (3.0, 0.2), (2.0, 1.0)]);
    let edge = Extent1d::from_min_max(0.0, 20.0);
    assert!(law.corner_parameters(&edge, false).unwrap().is_empty());
    for t in [9.0, 11.0] {
        let [_, slope, _] = law.evaluate(t, &edge, false).unwrap();
        assert!((slope + 0.1).abs() < 1e-15, "{t}: {slope}");
    }
    // x turns at 0.5 (4, 2, 4) with y straight: the radius's slope -0.2 before 10 and
    // +0.2 after, a corner at 10
    let mut dip = law_through([(4.0, 0.0), (2.0, 0.5), (4.0, 1.0)]);
    assert_eq!(dip.corner_parameters(&edge, false).unwrap(), vec![10.0]);
    for (t, want) in [(9.0, -0.2), (11.0, 0.2)] {
        let [_, slope, _] = dip.evaluate(t, &edge, false).unwrap();
        assert!((slope - want).abs() < 1e-15, "{t}: {slope}");
    }
}

#[test]
fn an_edge_interval_or_edge_map_with_no_length_or_not_finite_is_refused() {
    let law = spline_law(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[4.0, 2.0, 4.0], false);
    for ivl in [
        Extent1d::from_raw(5.0, 5.0),
        Extent1d::from_raw(0.0, f64::NAN),
        Extent1d::from_raw(f64::NEG_INFINITY, 1.0),
    ] {
        let got = law.corner_parameters(&ivl, false);
        assert!(
            matches!(got, Err(SolidError::InvalidInput { .. })),
            "{ivl:?}: {got:?}"
        );
    }
    for map in [
        Extent1d::from_raw(0.5, 0.5),
        Extent1d::from_raw(f64::NAN, 1.0),
    ] {
        let mut mapped = law.clone();
        mapped.edge_map = map;
        let got = mapped.corner_parameters(&Extent1d::from_min_max(0.0, 20.0), false);
        assert!(
            matches!(got, Err(SolidError::InvalidInput { .. })),
            "{map:?}: {got:?}"
        );
    }
}
