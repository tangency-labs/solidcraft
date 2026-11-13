//! Two curves joined end to end (`curve::join_curves`): a half section and its mirror, sharing
//! the end on the Y = 0 plane where both run along Y, join into one curve that is the first then
//! the second run backward, G1 at the joint, its ends the outer ends of the two; curves whose ends
//! are apart are refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::curve::{curve_comb, join_curves};
use solidcore_ops::Error;
use solidcore_record::brep::Curve;

const TOL: f64 = 1e-6;

/// A cubic through `points` on [0, 1].
fn cubic(points: [[f64; 3]; 4]) -> Curve {
    Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: points.to_vec(),
    }
}

/// Half a roof section in the plane x = 0: from the side at y = -10 up to the crown at
/// (0, 0, 6), arriving along +Y.
fn half() -> Curve {
    cubic([
        [0.0, -10.0, 0.0],
        [0.0, -10.0, 4.0],
        [0.0, -5.0, 6.0],
        [0.0, 0.0, 6.0],
    ])
}

/// The half mirrored in y = 0: from (0, 10, 0) to the crown, arriving along -Y.
fn mirror() -> Curve {
    cubic([
        [0.0, 10.0, 0.0],
        [0.0, 10.0, 4.0],
        [0.0, 5.0, 6.0],
        [0.0, 0.0, 6.0],
    ])
}

/// The points of `c` at `n` even steps over `[a, b]` of its parameter.
fn points(c: &Curve, a: f64, b: f64, n: usize) -> Vec<[f64; 3]> {
    curve_comb(c, [a, b], n)
        .unwrap()
        .iter()
        .map(|t| t.at)
        .collect()
}

fn near(p: [f64; 3], q: [f64; 3]) -> bool {
    (0..3).all(|i| (p[i] - q[i]).abs() < 1e-9)
}

/// The first and last knot of a spline.
fn domain(c: &Curve) -> [f64; 2] {
    let Curve::Spline { knots, .. } = c else {
        panic!("a spline: {c:?}")
    };
    [knots[0], knots[knots.len() - 1]]
}

#[test]
fn a_half_section_and_its_mirror_join_into_one_curve() {
    let (a, b) = (half(), mirror());
    let joined = join_curves(&a, &b, TOL).unwrap();
    let [t0, t2] = domain(&joined);
    let t1 = t0 + 1.0;
    assert!((t2 - (t0 + 2.0)).abs() < 1e-12, "{t0} {t2}");
    // the first half as it was, then the mirror run backward
    for (p, q) in points(&joined, t0, t1, 21)
        .iter()
        .zip(points(&a, 0.0, 1.0, 21))
    {
        assert!(near(*p, q), "{p:?} {q:?}");
    }
    let mut back = points(&b, 0.0, 1.0, 21);
    back.reverse();
    for (p, q) in points(&joined, t1, t2, 21).iter().zip(back) {
        assert!(near(*p, q), "{p:?} {q:?}");
    }
    // G1 at the crown: just either side the curve is level (along Y), on either side of y = 0
    let around = points(&joined, t1 - 1e-4, t1 + 1e-4, 2);
    let (before, after) = (around[0], around[1]);
    assert!(before[1] < 0.0 && after[1] > 0.0, "{before:?} {after:?}");
    assert!((before[2] - 6.0).abs() < 1e-6 && (after[2] - 6.0).abs() < 1e-6);
    assert!((before[2] - after[2]).abs() < 1e-12, "{before:?} {after:?}");
}

#[test]
fn the_joined_curve_s_ends_are_the_outer_ends() {
    let joined = join_curves(&half(), &mirror(), TOL).unwrap();
    let Curve::Spline { points, .. } = &joined else {
        panic!("a spline: {joined:?}")
    };
    assert_eq!(points.first(), Some(&[0.0, -10.0, 0.0]));
    assert_eq!(points.last(), Some(&[0.0, 10.0, 0.0]));
    // the other way round, it runs the other way
    let other = join_curves(&mirror(), &half(), TOL).unwrap();
    let Curve::Spline { points, .. } = &other else {
        panic!("a spline: {other:?}")
    };
    assert_eq!(points.first(), Some(&[0.0, 10.0, 0.0]));
    assert_eq!(points.last(), Some(&[0.0, -10.0, 0.0]));
}

#[test]
fn curves_whose_ends_are_apart_are_refused() {
    let apart = cubic([
        [0.0, 10.0, 0.0],
        [0.0, 10.0, 4.0],
        [0.0, 5.0, 6.0],
        [0.0, 0.001, 6.0],
    ]);
    assert!(matches!(
        join_curves(&half(), &apart, TOL),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(join_curves(&half(), &mirror(), 0.0), Err(Error::Argument));
}
