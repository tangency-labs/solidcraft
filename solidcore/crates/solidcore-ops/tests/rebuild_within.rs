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

/// The distance between `a` and `b`.
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
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
            // the ends of a clamped spline are its end control points: within the bound
            let (_, from) = spline(&source);
            let (_, to) = spline(&c);
            assert!(distance(to[0], from[0]) <= bound);
            assert!(distance(to[to.len() - 1], from[from.len() - 1]) <= bound);
        }
    }
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
