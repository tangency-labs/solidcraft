use super::*;
use crate::eval::geometry::{Curve, Frame};
use crate::math::det;

/// A circle of radius 2 about +z through (1, 1, 3), its angle from +x.
fn circle() -> CurveEval {
    let c = Curve::Circle {
        frame: Frame {
            origin: [1.0, 1.0, 3.0],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        },
        radius: 2.0,
    };
    CurveEval::new(&c).unwrap()
}

fn axes() -> (Vec3, Vec3, Vec3) {
    (
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    )
}

/// A quarter of the circle from 0.2 to 1.4 rad meets the ruling at 1 rad at its own parameter 1,
/// the point on the circle there.
#[test]
fn a_piece_across_the_angle_meets_it_where_its_angle_is_that() {
    let (o, x, y) = axes();
    let Some(AngleCrossing::At { point, t }) = angle_crossing(&circle(), o, x, y, [0.2, 1.4], 1.0)
    else {
        panic!("no crossing")
    };
    assert!((t - 1.0).abs() < 1e-12, "{t}");
    assert!(
        (point - Vec3::new(1.0 + 2.0 * det::cos(1f64), 1.0 + 2.0 * det::sin(1f64), 3.0)).length()
            < 1e-12
    );
}

/// The ruling at an end's angle meets the piece at that end exactly.
#[test]
fn a_piece_meets_the_angle_of_its_end_at_that_end() {
    let (o, x, y) = axes();
    assert_eq!(
        angle_crossing(&circle(), o, x, y, [0.0, 1.0], 0.0)
            .map(|c| matches!(c, AngleCrossing::At { t, .. } if t == 0.0)),
        Some(true)
    );
}

/// A piece short of the angle gives its nearer end; one that passes the opposite angle (the
/// offsets' wrap at half a turn) does not meet it either.
#[test]
fn a_piece_short_of_the_angle_or_across_its_opposite_does_not_meet_it() {
    let (o, x, y) = axes();
    let near = angle_crossing(&circle(), o, x, y, [0.2, 0.9], 1.0);
    assert!(
        matches!(near, Some(AngleCrossing::Nearer { t, .. }) if t == 0.9),
        "{near:?}"
    );
    let across = angle_crossing(&circle(), o, x, y, [3.5, 4.5], 1.0);
    assert!(
        matches!(across, Some(AngleCrossing::Nearer { .. })),
        "{across:?}"
    );
}
