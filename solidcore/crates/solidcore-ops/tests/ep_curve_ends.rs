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
use solidcore_ops::curve_fit::{
    ep_curve, ep_curve_with_end_conditions, ep_curve_with_ends, EndCondition,
};
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

// The fit asked for each end's condition decides which it can hold, and says what each got
// (#17492).

/// [`HALF`] with its second point moved off the plane x = 0 to x = `dx`.
fn half_lifted(dx: f64) -> [[f64; 3]; 5] {
    let mut points = HALF;
    points[1][0] = dx;
    points
}

#[test]
fn an_end_square_to_the_points_own_plane_is_left_free_and_said() {
    // HALF lies in x = 0: leaving it square to that plane would hook out of it and back.
    let fit = ep_curve_with_end_conditions(
        &HALF,
        3,
        EndCondition::SquareTo([1.0, 0.0, 0.0]),
        EndCondition::Free,
    )
    .unwrap();
    assert_eq!(fit.start, EndCondition::Free);
    assert_eq!(fit.end, EndCondition::Free);
    assert_eq!((fit.curve, fit.bound), ep_curve(&HALF, 3).unwrap());
}

#[test]
fn points_off_the_plane_by_less_than_the_model_tolerance_are_in_it() {
    // The kernel's zone tolerance (1e-5 mm by default) decides: 1e-7 off is in the plane,
    // 1e-3 off is not.
    let square_to_x = EndCondition::SquareTo([-2.0, 0.0, 0.0]);
    let fit = ep_curve_with_end_conditions(&half_lifted(1e-7), 3, square_to_x, EndCondition::Free)
        .unwrap();
    assert_eq!(fit.start, EndCondition::Free);
    let fit = ep_curve_with_end_conditions(&half_lifted(1e-3), 3, square_to_x, EndCondition::Free)
        .unwrap();
    // the second point is at x = +1e-3: the curve leaves along +x
    assert_eq!(fit.start, EndCondition::SquareTo([1.0, 0.0, 0.0]));
}

#[test]
fn ends_square_to_planes_the_points_leave_run_along_their_normals_toward_the_curve() {
    // HALF runs from the centreline (y = 0) out (+y) and ends falling (-z). Asked square to
    // Side (the y normal, given reversed) and Top (the z normal): it leaves along +y and
    // arrives along -z.
    let fit = ep_curve_with_end_conditions(
        &HALF,
        3,
        EndCondition::SquareTo([0.0, -3.0, 0.0]),
        EndCondition::SquareTo([0.0, 0.0, 0.5]),
    )
    .unwrap();
    assert_eq!(fit.start, EndCondition::SquareTo([0.0, 1.0, 0.0]));
    assert_eq!(fit.end, EndCondition::SquareTo([0.0, 0.0, -1.0]));
    assert_eq!(
        (fit.curve.clone(), fit.bound),
        ep_curve_with_ends(&HALF, 3, Some([0.0, 1.0, 0.0]), Some([0.0, 0.0, -1.0])).unwrap()
    );
    through(&fit.curve, 1e-9);
    let (start, end) = end_legs(&fit.curve);
    assert!(angle(start, [0.0, 1.0, 0.0]) <= 1e-9, "{start:?}");
    assert!(angle(end, [0.0, 0.0, -1.0]) <= 1e-9, "{end:?}");
}

#[test]
fn the_sense_is_the_side_the_points_first_leave_the_ends_plane_to() {
    // The second point lies in the start's plane (y = 0) and the third on its -y side: the
    // curve leaves along -y. The last but one point lies in the end's plane (x = 3) and the
    // one before it at x < 3: the curve arrives along +x.
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [2.0, -1.0, 0.0],
        [3.0, -2.0, 0.0],
        [3.0, -3.0, 0.0],
    ];
    let fit = ep_curve_with_end_conditions(
        &points,
        3,
        EndCondition::SquareTo([0.0, 1.0, 0.0]),
        EndCondition::SquareTo([-1.0, 0.0, 0.0]),
    )
    .unwrap();
    assert_eq!(fit.start, EndCondition::SquareTo([0.0, -1.0, 0.0]));
    assert_eq!(fit.end, EndCondition::SquareTo([1.0, 0.0, 0.0]));
    let (start, end) = end_legs(&fit.curve);
    assert!(angle(start, [0.0, -1.0, 0.0]) <= 1e-9, "{start:?}");
    assert!(angle(end, [1.0, 0.0, 0.0]) <= 1e-9, "{end:?}");
}

#[test]
fn both_ends_free_is_the_fit_without_directions() {
    let fit =
        ep_curve_with_end_conditions(&HALF, 3, EndCondition::Free, EndCondition::Free).unwrap();
    assert_eq!(
        (fit.start, fit.end),
        (EndCondition::Free, EndCondition::Free)
    );
    assert_eq!(
        (fit.curve, fit.bound),
        ep_curve_with_ends(&HALF, 3, None, None).unwrap()
    );
}

#[test]
fn hostile_end_conditions_and_points_are_refused() {
    let square = EndCondition::SquareTo([0.0, 1.0, 0.0]);
    let free = EndCondition::Free;
    let cases: [(&[[f64; 3]], EndCondition, EndCondition); 8] = [
        (&HALF, EndCondition::SquareTo([f64::NAN, 1.0, 0.0]), free),
        (&HALF, free, EndCondition::SquareTo([0.0; 3])),
        (
            &HALF,
            EndCondition::SquareTo([f64::INFINITY, 0.0, 0.0]),
            free,
        ),
        (
            &HALF,
            EndCondition::SquareTo([f64::MAX, f64::MAX, 0.0]),
            free,
        ),
        (&HALF[..1], square, free),
        (&[], square, square),
        (&[[0.0; 3], [f64::NAN, 1.0, 0.0]], square, free),
        (&[[0.0; 3], [0.0; 3], [0.0, 1.0, 0.0]], square, free),
    ];
    for (points, start, end) in cases {
        assert_eq!(
            ep_curve_with_end_conditions(points, 3, start, end).err(),
            Some(Error::Argument),
            "{points:?} {start:?} {end:?}"
        );
    }
    // a held end needs degree 2 or more; an end left free does not
    assert_eq!(
        ep_curve_with_end_conditions(&HALF, 1, square, free).err(),
        Some(Error::Argument)
    );
    let fit = ep_curve_with_end_conditions(&HALF, 1, EndCondition::SquareTo([1.0, 0.0, 0.0]), free)
        .unwrap();
    assert_eq!(fit.start, EndCondition::Free);
}
