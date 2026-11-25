//! A curve rebuilt to a degree and a tolerance instead of a span count: the rebuilt curve is a
//! clamped spline of that degree on `[0, 1]` within the tolerance of the curve at every
//! parameter (the kernel's certified bound), and leaves or reaches each given end along the
//! given direction.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::disallowed_methods
)]

use solidcore_ops::curve_fit::{closed_cv_curve, ep_curve, ep_curve_with_ends, rebuild_within};
use solidcore_ops::Error;
use solidcore_record::brep::{Curve, Natural, MAX_SPLINE_DEGREE};

/// Nine points on a circle of radius 50 about the origin, from 0 to 120 degrees.
fn arc_points() -> Vec<[f64; 3]> {
    (0..=8)
        .map(|i| {
            let t = f64::from(i) / 8.0 * 2.0 * std::f64::consts::FRAC_PI_3;
            [50.0 * t.cos(), 50.0 * t.sin(), 0.0]
        })
        .collect()
}

/// A half section in the plane x = 0, from the centreline (y = 0) out and down.
const HALF: [[f64; 3]; 5] = [
    [0.0, 0.0, 900.0],
    [0.0, 250.0, 880.0],
    [0.0, 520.0, 800.0],
    [0.0, 760.0, 640.0],
    [0.0, 880.0, 420.0],
];

/// The knots and control points of a spline record.
fn spline(c: &Curve) -> (&[f64], &[[f64; 3]]) {
    let Curve::Spline { knots, points } = c else {
        panic!("a spline: {c:?}")
    };
    (knots.as_slice(), points.as_slice())
}

/// The rebuilt curve is a clamped spline of `degree` on `[0, 1]`.
fn clamped_on_unit(c: &Curve, degree: usize) {
    let (knots, points) = spline(c);
    assert_eq!(knots.len(), points.len() + degree + 1, "{knots:?}");
    assert_eq!(c.natural(), Natural::Range(0.0, 1.0));
    let tail = knots.len() - degree - 1;
    assert!(knots[..=degree].iter().all(|&k| k == 0.0), "{knots:?}");
    assert!(knots[tail..].iter().all(|&k| k == 1.0), "{knots:?}");
    // the interior knots simple and increasing
    let inner = &knots[degree + 1..tail];
    assert!(inner.windows(2).all(|w| w[0] < w[1]), "{knots:?}");
}

#[test]
fn a_curve_through_points_is_rebuilt_within_the_tolerance() {
    let (source, _) = ep_curve(&arc_points(), 3).unwrap();
    for degree in [1, 2, 3, 5] {
        for tolerance in [1e-2, 1e-4] {
            let (c, bound) = rebuild_within(&source, degree, tolerance, None, None).unwrap();
            assert!(
                bound <= tolerance,
                "degree {degree}: {bound} past {tolerance}"
            );
            clamped_on_unit(&c, degree);
            // through the curve's two ends: the ends of a clamped spline are its end control
            // points, and the rebuilt curve's are the curve's own, not points within the bound
            let (_, from) = spline(&source);
            let (_, to) = spline(&c);
            assert_eq!(to[0], from[0], "degree {degree}");
            assert_eq!(to[to.len() - 1], from[from.len() - 1], "degree {degree}");
        }
    }
}

/// A span too narrow for `[0, 1]` to hold is not lost. The polyline is on `[-1, 1]`, and its
/// last span is the last float below 1: in it the polyline runs 5 on, from (1, 0, 0) to
/// (1, 5, 0). Mapped onto `[0, 1]`, the knot `1 - 2^-53` becomes 1 and that span has no
/// parameter left, so the rebuild is made on the polyline's own domain and its bound is
/// against the polyline as it is given. Its answer is a curve that ends where the polyline
/// ends, or the bound it reached. That bound cannot be small. Where the rebuilt curve's last
/// span starts (at `a`, two floats or more below 1) it is some `d` from the polyline, which is
/// by (1, 0, 0) there; at `1 - 2^-53`, halfway or more along that span to (1, 5, 0), it is
/// then at least `5/2 - d/2` from (1, 0, 0), where the polyline still is. The larger of `d`
/// and `5/2 - d/2` is never below 5/3.
#[test]
fn a_span_too_narrow_for_zero_to_one_is_not_lost() {
    let end = [1.0, 5.0, 0.0];
    let source = Curve::Spline {
        knots: vec![-1.0, -1.0, 1.0 - f64::EPSILON / 2.0, 1.0, 1.0],
        points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], end],
    };
    match rebuild_within(&source, 1, 1e-3, None, None) {
        Ok((c, bound)) => {
            let (_, to) = spline(&c);
            assert_eq!(to[to.len() - 1], end);
            assert!(bound <= 1e-3, "{bound}");
        }
        Err(Error::PastTolerance { bound }) => assert!(bound >= 1.6, "{bound}"),
        Err(e) => panic!("{e:?}"),
    }
}

/// A tolerance below what the rounding of the curve's own coordinates allows any bound to be
/// is answered with the bound of the first fit: no count of control points reaches it.
#[test]
fn a_tolerance_below_the_rounding_of_the_curve_is_past_the_tolerance() {
    let (source, _) = ep_curve(&arc_points(), 3).unwrap();
    for tolerance in [f64::MIN_POSITIVE, 1e-300, 1e-20] {
        match rebuild_within(&source, 3, tolerance, None, None) {
            Err(Error::PastTolerance { bound }) => {
                assert!(bound > tolerance && bound.is_finite(), "{bound}");
            }
            other => panic!("{tolerance}: {other:?}"),
        }
    }
}

/// An end leg only a few roundings of its coordinates long cannot lie along a direction, and
/// the rebuild says so instead of giving a curve that leaves another way. The curve is a cubic
/// of one span whose control points are steps of the grid of the floats near 2^29 (`g =
/// 2^-23`), forty steps across, its first leg the step (14, 3). The direction asked for has
/// the slope `sqrt(2) - 1.2`; the nearest steps of that grid to it, the multiples of (14, 3),
/// are 7e-5 radians off, forty times the kernel's numeric zero for angles. The same steps at
/// the origin, a unit each, are rebuilt with the first leg along the direction.
#[test]
fn an_end_leg_a_few_roundings_long_is_refused_not_turned() {
    let g = 2.0_f64.powi(-23);
    let base = 2.0_f64.powi(29);
    let steps = [[0.0, 0.0], [14.0, 3.0], [28.0, 12.0], [40.0, 40.0]];
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let d = [1.0, 2.0_f64.sqrt() - 1.2, 0.0];
    let far = Curve::Spline {
        knots: knots.clone(),
        points: steps
            .iter()
            .map(|&[x, y]| [base + x * g, base + y * g, 0.0])
            .collect(),
    };
    assert!(matches!(
        rebuild_within(&far, 3, 8.0 * g, Some(d), None),
        Err(Error::Unsupported(_))
    ));
    // without a direction the same curve is rebuilt
    let (_, bound) = rebuild_within(&far, 3, 8.0 * g, None, None).unwrap();
    assert!(bound <= 8.0 * g, "{bound}");

    let near = Curve::Spline {
        knots,
        points: steps.iter().map(|&[x, y]| [x, y, 0.0]).collect(),
    };
    let (c, bound) = rebuild_within(&near, 3, 1e-2, Some(d), None).unwrap();
    assert!(bound <= 1e-2, "{bound}");
    let (_, points) = spline(&c);
    let leg = [points[1][0] - points[0][0], points[1][1] - points[0][1]];
    // along the direction: the cross product with it is rounding against the two lengths
    let across = leg[0] * d[1] - leg[1] * d[0];
    let along = leg[0] * d[0] + leg[1] * d[1];
    assert!(along > 0.0 && across.abs() <= 1e-12 * along, "{leg:?}");
    assert_eq!(points[1][2], 0.0);
}

#[test]
fn a_curve_on_another_domain_is_rebuilt_on_zero_to_one() {
    let (knots, points) = {
        let (source, _) = ep_curve(&arc_points(), 3).unwrap();
        let (k, p) = spline(&source);
        (
            k.iter().map(|k| 2.0 + 3.0 * k).collect::<Vec<f64>>(),
            p.to_vec(),
        )
    };
    let source = Curve::Spline { knots, points };
    let (c, bound) = rebuild_within(&source, 3, 1e-3, None, None).unwrap();
    assert!(bound <= 1e-3, "{bound}");
    clamped_on_unit(&c, 3);
}

/// Given directions along the axes, the rebuilt curve's end legs are along them exactly: the
/// second control point is the first plus the reach times the unit direction, whose zero
/// coordinates add nothing, so the leg's other coordinates are zero; the sense is the
/// direction's (the reach is positive when the rebuild is within its tolerance).
#[test]
fn the_rebuilt_curve_leaves_and_reaches_its_ends_along_the_directions() {
    let (start, end) = ([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]);
    let (source, _) = ep_curve_with_ends(&HALF, 3, Some(start), Some(end)).unwrap();
    // a direction of any length: the source's start given at twice its length
    let (c, bound) = rebuild_within(&source, 3, 1e-3, Some([0.0, 2.0, 0.0]), Some(end)).unwrap();
    assert!(bound <= 1e-3, "{bound}");
    clamped_on_unit(&c, 3);
    let (_, points) = spline(&c);
    let n = points.len();
    let leg = |a: [f64; 3], b: [f64; 3]| [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let first = leg(points[0], points[1]);
    let last = leg(points[n - 2], points[n - 1]);
    assert!(
        first[0] == 0.0 && first[2] == 0.0 && first[1] > 0.0,
        "{first:?}"
    );
    assert!(
        last[0] == 0.0 && last[1] == 0.0 && last[2] < 0.0,
        "{last:?}"
    );
    // the start alone, the end alone, on a quadratic
    for (s, e) in [(Some(start), None), (None, Some(end))] {
        let (c, bound) = rebuild_within(&source, 2, 1e-3, s, e).unwrap();
        assert!(bound <= 1e-3, "{bound}");
        clamped_on_unit(&c, 2);
        let (_, points) = spline(&c);
        let n = points.len();
        if s.is_some() {
            let first = leg(points[0], points[1]);
            assert!(
                first[0] == 0.0 && first[2] == 0.0 && first[1] > 0.0,
                "{first:?}"
            );
        } else {
            let last = leg(points[n - 2], points[n - 1]);
            assert!(
                last[0] == 0.0 && last[1] == 0.0 && last[2] < 0.0,
                "{last:?}"
            );
        }
    }
}

#[test]
fn bad_arguments_are_refused() {
    let (source, _) = ep_curve(&arc_points(), 3).unwrap();
    let d = Some([0.0, 1.0, 0.0]);
    for (degree, tolerance, start, end) in [
        (0, 1e-3, None, None),
        (MAX_SPLINE_DEGREE + 1, 1e-3, None, None),
        (usize::MAX, 1e-3, None, None),
        (1, 1e-3, d, None),
        (1, 1e-3, None, d),
        (3, 0.0, None, None),
        (3, -1e-3, None, None),
        (3, f64::NAN, None, None),
        (3, f64::INFINITY, None, None),
        (3, 1e-3, Some([f64::NAN, 1.0, 0.0]), None),
        (3, 1e-3, None, Some([0.0, f64::INFINITY, 0.0])),
        (3, 1e-3, Some([0.0; 3]), None),
        (3, 1e-3, None, Some([0.0; 3])),
    ] {
        assert_eq!(
            rebuild_within(&source, degree, tolerance, start, end).err(),
            Some(Error::Argument),
            "{degree} {tolerance} {start:?} {end:?}"
        );
    }
    // a rational arc and a closed curve are not taken
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let arc = Curve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: vec![
            [10.0, 0.0, 0.0, 1.0],
            [10.0 * w, 10.0 * w, 0.0, w],
            [0.0, 10.0, 0.0, 1.0],
        ],
    };
    assert_eq!(
        rebuild_within(&arc, 3, 1e-3, None, None).err(),
        Some(Error::Argument)
    );
    let square = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];
    let closed = closed_cv_curve(&square, 3).unwrap();
    assert_eq!(
        rebuild_within(&closed, 3, 1e-3, None, None).err(),
        Some(Error::Argument)
    );
}
