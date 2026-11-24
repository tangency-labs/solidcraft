//! Where a spline sits on a round face, and the measures of the face and the tools: each against
//! numbers worked out by hand, and hostile numbers (not finite, no length) refused or answered
//! plainly, never a panic.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::common::HOSTILE;
use crate::eval::Frame;
use crate::teeth::seat::{
    angle_zero, angles_apart, axis_gap, centre_angle, chord_gap, frame_through, into_frame,
    moved_out, nearer_start, normal, off_cylinder, points_apart, reach_along, seat_frame,
    start_candidates, RoundFace, SeatRefusal,
};
use crate::Vec3;

/// A round face along +Z: radius 10, from z = 0 to z = 30 about the axis through (1, 2, 0).
fn face() -> RoundFace {
    RoundFace {
        axis_point: Vec3::new(1.0, 2.0, 0.0),
        axis: Vec3::Z,
        radius: 10.0,
        start: 0.0,
        end: 30.0,
    }
}

/// Whether two points are within a millionth of a millimetre.
fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-9
}

/// The plane z = 0 with x along +X.
fn xy() -> Frame {
    Frame {
        origin: [0.0, 0.0, 0.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    }
}

#[test]
fn angle_zero_follows_the_world_axes() {
    // Along Z: angle 0 is +X, and an axis along -Z is turned to +Z.
    let (x, axis) = angle_zero(Vec3::new(0.0, 0.0, -2.0)).unwrap();
    assert_eq!((x, axis), (Vec3::X, Vec3::new(0.0, 0.0, 2.0)));
    // Along X: +Y. Along Y: +Z.
    assert_eq!(angle_zero(Vec3::X).unwrap().0, Vec3::Y);
    assert_eq!(angle_zero(Vec3::Y).unwrap().0, Vec3::Z);
    // A slanted axis: the reference projected onto the plane square to it, a unit vector.
    let slant = Vec3::new(0.0, 0.6, 0.8);
    let (x, _) = angle_zero(slant).unwrap();
    assert!(x.dot(slant).abs() < 1e-15 && (x.length() - 1.0).abs() < 1e-15);
    for bad in [f64::NAN, f64::INFINITY] {
        assert!(angle_zero(Vec3::new(bad, 0.0, 1.0)).is_none(), "{bad}");
    }
}

#[test]
fn a_seat_has_its_free_end_its_plane_and_the_pick_on_the_face() {
    let face = face();
    assert_eq!(off_cylinder(&face, Vec3::new(13.0, 2.0, 40.0)), 2.0);
    assert!(
        nearer_start(&face, Vec3::new(0.0, 0.0, 15.0)),
        "a tie is the start's"
    );
    assert!(!nearer_start(&face, Vec3::new(0.0, 0.0, 16.0)));
    // Picked near the top: the free end is the top, and the face runs on against +Z.
    let seat = seat_frame(&face, Vec3::new(1.0, 14.0, 25.0), None).unwrap();
    assert_eq!(seat.plane.origin, [1.0, 2.0, 30.0]);
    assert_eq!(seat.plane.x_axis, [1.0, 0.0, 0.0]);
    assert_eq!(normal(&seat.plane), Vec3::Z);
    assert_eq!((seat.inward, seat.room), (-1.0, 30.0));
    assert!(close(seat.at, Vec3::new(1.0, 12.0, 25.0)), "{:?}", seat.at);
    // A start near the bottom: the free end is the bottom.
    let seat = seat_frame(
        &face,
        Vec3::new(1.0, 14.0, 25.0),
        Some(Vec3::new(1.0, 2.0, 0.0)),
    )
    .unwrap();
    assert_eq!((seat.plane.origin, seat.inward), ([1.0, 2.0, 0.0], 1.0));
    // A pick on the axis stays where it is.
    let on_axis = Vec3::new(1.0, 2.0, 5.0);
    assert_eq!(seat_frame(&face, on_axis, None).unwrap().at, on_axis);
}

#[test]
fn a_seat_refuses_a_face_with_no_direction_or_no_size() {
    let face = face();
    let refusals = [
        (
            RoundFace {
                axis: Vec3::new(f64::NAN, 0.0, 1.0),
                ..face
            },
            SeatRefusal::NoDirection,
        ),
        (
            RoundFace {
                axis: Vec3::ZERO,
                ..face
            },
            SeatRefusal::NoPlane,
        ),
        (RoundFace { end: 0.0, ..face }, SeatRefusal::NoSize),
        (
            RoundFace {
                radius: f64::NAN,
                ..face
            },
            SeatRefusal::NoSize,
        ),
        (
            RoundFace {
                end: f64::INFINITY,
                ..face
            },
            SeatRefusal::NoSize,
        ),
    ];
    for (bad, why) in refusals {
        assert_eq!(seat_frame(&bad, Vec3::X, None), Err(why), "{bad:?}");
    }
    // Hostile numbers anywhere: a refusal or a seat, never a panic.
    for &h in &HOSTILE {
        let odd = RoundFace {
            axis_point: Vec3::new(h, 0.0, 0.0),
            axis: Vec3::new(0.0, h, 1.0),
            radius: h,
            start: -h,
            end: h,
        };
        let _ = seat_frame(&odd, Vec3::new(h, h, h), Some(Vec3::new(0.0, 0.0, h)));
        let _ = start_candidates(&odd, Vec3::new(h, 1.0, 0.0), h > 0.0);
        let _ = off_cylinder(&odd, Vec3::new(h, 1.0, 2.0));
    }
}

#[test]
fn the_start_is_looked_for_beside_the_rim_then_on_the_axis() {
    let face = face();
    let shaft = start_candidates(&face, Vec3::new(11.0, 2.0, 3.0), false).unwrap();
    assert_eq!(
        shaft,
        [
            Vec3::new(6.0, 2.0, 0.0),
            Vec3::new(8.5, 2.0, 0.0),
            Vec3::new(10.0, 2.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
        ]
    );
    let bore = start_candidates(&face, Vec3::new(11.0, 2.0, 29.0), true).unwrap();
    assert_eq!(bore[0], Vec3::new(16.0, 2.0, 30.0));
    assert_eq!(bore[3], Vec3::new(1.0, 2.0, 30.0));
    let lost = RoundFace {
        axis: Vec3::new(f64::NAN, 0.0, 0.0),
        ..face
    };
    assert!(start_candidates(&lost, Vec3::X, false).is_none());
}

#[test]
fn a_point_moves_out_from_the_axis_and_into_the_frame() {
    let plane = Frame {
        origin: [1.0, 2.0, 30.0],
        ..xy()
    };
    assert!(close(
        moved_out(&plane, Vec3::new(1.0, 12.0, 25.0), 1.5),
        Vec3::new(1.0, 13.5, 25.0)
    ));
    // On the axis it stays.
    let on_axis = Vec3::new(1.0, 2.0, 7.0);
    assert_eq!(moved_out(&plane, on_axis, 3.0), on_axis);
    let m = into_frame(&plane);
    assert_eq!(m[0], [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(m[3], [-1.0, -2.0, -30.0, 1.0]);
    // Hostile numbers: a plain answer, never a panic.
    for &h in &HOSTILE {
        let odd = Frame {
            origin: [h, 0.0, 0.0],
            x_axis: [h, 1.0, 0.0],
            y_axis: [0.0, h, 1.0],
        };
        let _ = moved_out(&odd, Vec3::new(h, h, 0.0), h);
        let _ = into_frame(&odd);
        let _ = centre_angle(&odd, Vec3::new(h, 0.0, 0.0), Vec3::new(0.0, h, 0.0));
    }
}

#[test]
fn a_frame_is_made_orthonormal_or_refused() {
    let made = frame_through(
        Vec3::ZERO,
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(1.0, 3.0, 0.0),
    )
    .unwrap();
    assert_eq!(made.x_axis, [1.0, 0.0, 0.0]);
    assert_eq!(made.y_axis, [0.0, 1.0, 0.0]);
    assert!(frame_through(Vec3::ZERO, Vec3::ZERO, Vec3::Y).is_none());
    assert!(frame_through(Vec3::ZERO, Vec3::X, Vec3::X).is_none());
    assert!(frame_through(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X, Vec3::Y).is_none());
}

#[test]
fn lines_chords_boxes_and_angles_are_measured() {
    let gap = axis_gap(Vec3::ZERO, Vec3::Z, Vec3::new(3.0, 4.0, 9.0), Vec3::Z);
    assert_eq!((gap.lean, gap.off), (0.0, 5.0));
    let across = axis_gap(Vec3::ZERO, Vec3::Z, Vec3::ZERO, Vec3::X);
    assert_eq!(across.lean, 1.0);
    let chord = chord_gap(Vec3::Z, Vec3::ZERO, Vec3::new(3.0, 0.0, 4.0));
    assert_eq!((chord.length, chord.lean), (5.0, 3.0));
    let angle = centre_angle(&xy(), Vec3::new(-1.0, 1.0, 0.0), Vec3::new(1.0, 3.0, 5.0));
    assert_eq!(angle, FRAC_PI_2);
    assert!((angles_apart(0.1, TAU - 0.1) - 0.2).abs() < 1e-12);
    assert!((angles_apart(PI - 0.1, -PI + 0.1) - 0.2).abs() < 1e-12);
    assert_eq!(angles_apart(1.0, 1.0), 0.0);
    assert_eq!(
        reach_along(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::Z,
            Vec3::new(-1.0, -1.0, -2.0),
            Vec3::new(1.0, 1.0, 5.0)
        ),
        (-3.0, 4.0)
    );
    assert_eq!(points_apart([1.0, 1.0], [4.0, 5.0]), 5.0);
    for &h in &HOSTILE {
        let _ = axis_gap(
            Vec3::new(h, 0.0, 0.0),
            Vec3::Z,
            Vec3::new(0.0, h, 0.0),
            Vec3::X,
        );
        let _ = chord_gap(Vec3::new(h, 0.0, 0.0), Vec3::ZERO, Vec3::new(h, h, h));
        let _ = angles_apart(h, 1.0);
        let _ = reach_along(
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(h, 0.0, 0.0),
            Vec3::new(0.0, h, 0.0),
        );
        let _ = points_apart([h, 0.0], [0.0, h]);
    }
}
