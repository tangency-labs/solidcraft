//! A curve through points with a direction at either end: a half section whose end at the
//! centreline is square to the mirror plane joins its mirror smoothly there. The curve passes
//! through every point within its bound, and leaves or reaches each given end along the given
//! direction.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::disallowed_methods
)]

use solidcore_ops::curve::closest_point;
use solidcore_ops::curve_fit::ep_curve_with_ends;
use solidcore_ops::Error;
use solidcore_record::brep::Curve;

/// A half section in the plane x = 0, from the centreline (y = 0) out and down.
const HALF: [[f64; 3]; 5] = [
    [0.0, 0.0, 900.0],
    [0.0, 250.0, 880.0],
    [0.0, 520.0, 800.0],
    [0.0, 760.0, 640.0],
    [0.0, 880.0, 420.0],
];

/// The angle between `a` and `b`, by the cross and dot products.
fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let c = cross.iter().map(|x| x * x).sum::<f64>().sqrt();
    let d = a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    c.atan2(d)
}

/// The tangent directions at the start and the end of a clamped spline: along its first and
/// last legs.
fn end_legs(c: &Curve) -> ([f64; 3], [f64; 3]) {
    let Curve::Spline { points, .. } = c else {
        panic!("a spline: {c:?}")
    };
    let leg = |a: [f64; 3], b: [f64; 3]| [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let n = points.len();
    (leg(points[0], points[1]), leg(points[n - 2], points[n - 1]))
}

/// Every point is on the curve within `tol`.
fn through(c: &Curve, tol: f64) {
    for p in HALF {
        let at = closest_point(c, p, 1e-9).unwrap().unwrap();
        assert!(at.distance <= tol, "{p:?} is {} off", at.distance);
    }
}

#[test]
fn a_half_section_square_to_the_mirror_plane_at_its_start() {
    let (c, bound) = ep_curve_with_ends(&HALF, 3, Some([0.0, 1.0, 0.0]), None).unwrap();
    assert!(bound <= 1e-9, "{bound}");
    through(&c, 1e-9);
    let (start, _) = end_legs(&c);
    assert!(angle(start, [0.0, 1.0, 0.0]) <= 1e-9, "{start:?}");
}

#[test]
fn both_ends_given_and_a_reversed_direction_kept() {
    let (c, bound) =
        ep_curve_with_ends(&HALF, 3, Some([0.0, 2.0, 0.0]), Some([0.0, 0.0, -1.0])).unwrap();
    assert!(bound <= 1e-9, "{bound}");
    through(&c, 1e-9);
    let (start, end) = end_legs(&c);
    assert!(angle(start, [0.0, 1.0, 0.0]) <= 1e-9, "{start:?}");
    assert!(angle(end, [0.0, 0.0, -1.0]) <= 1e-9, "{end:?}");
}

#[test]
fn only_the_end_given() {
    let (c, _) = ep_curve_with_ends(&HALF, 3, None, Some([0.0, 0.0, -1.0])).unwrap();
    through(&c, 1e-9);
    let (_, end) = end_legs(&c);
    assert!(angle(end, [0.0, 0.0, -1.0]) <= 1e-9, "{end:?}");
}

#[test]
fn bad_directions_are_refused() {
    for (start, end) in [
        (Some([0.0; 3]), None),
        (None, Some([f64::NAN, 0.0, 1.0])),
        (Some([0.0, 1.0, 0.0]), Some([0.0; 3])),
    ] {
        assert_eq!(
            ep_curve_with_ends(&HALF, 3, start, end).err(),
            Some(Error::Argument),
            "{start:?} {end:?}"
        );
    }
    // a degree below 2 takes no end direction
    assert_eq!(
        ep_curve_with_ends(&HALF, 1, Some([0.0, 1.0, 0.0]), None).err(),
        Some(Error::Argument)
    );
}
