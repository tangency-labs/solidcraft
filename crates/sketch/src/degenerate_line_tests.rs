//! Dragging a sketch whose construction line has collapsed to (near) zero length (#7635).
//!
//! The reporter's design: `l1` with its midpoint pinned at the fixed origin, `l2` perpendicular
//! to `l1` at the origin, and a construction line `l3` coincident with the origin, constrained
//! horizontal, with a 50-degree angle dimension between `l3` and `l1`. `l3`'s ends sit on top of
//! each other, so its angle row's entries on `l3` are ~1e7 while the same row's entries on `l1`
//! are ~1. Dragging an endpoint of `l1` must keep the sketch solved: the cursor may not leave the
//! constraint manifold, and the rank analysis may not hide `l1`'s coupling below its tolerance.

use solidcraft_geom::Vec2;

use crate::{Analysis, ConstraintKind, Hold, Sketch, move_point_towards, solve_with};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// The reporter's sketch, at the pose saved in `Sketch bug.solidcraft`: `l3` is a construction
/// line whose ends are ~1e-6 apart at the origin.
fn collapsed_construction_sketch() -> Sketch {
    let mut sk = Sketch::new();
    let s1 = sk.add_point(v(-6.616478376301535, -7.876608451910095), Some("l1.start")).unwrap();
    let e1 = sk.add_point(v(6.616478376301535, 7.876608451910095), Some("l1.end")).unwrap();
    let e2 = sk.add_point(v(-10.656000978748368, 8.951213010548841), Some("l2.end")).unwrap();
    let s3 = sk.add_point(v(2.814486877637019e-13, -1.946249554158009e-16), Some("l3.start")).unwrap();
    let e3 = sk.add_point(v(9.597991615339335e-7, -5.159859446747845e-10), Some("l3.end")).unwrap();
    let l1 = sk.add_line_pts(s1, e1, Some("l1")).unwrap();
    let l2 = sk.add_line_pts(0, e2, Some("l2")).unwrap();
    let l3 = sk.add_line_pts(s3, e3, Some("l3")).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: 0, c: l1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Midpoint { p: 0, l: l1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Perpendicular { a: l2, b: l1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l: l3 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: s3, q: 0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Angle { a: l3, b: l1, value: 50f64.to_radians(), flip: false }, None).unwrap();
    sk
}

/// The pose the reporter saved solves.
#[test]
fn the_saved_pose_solves() {
    let mut sk = collapsed_construction_sketch();
    let rep = solve_with(&mut sk, &Hold::default(), None, Analysis::Full, None);
    assert!(rep.ok(), "the saved pose no longer solves: {:?}", rep.failing);
}

/// Dragging `l1.end` off the 50-degree line: the exact target is unreachable (the midpoint and
/// the angle dimension pin `l1`'s inclination), so the cursor must stay on the manifold: the
/// sketch solves afterwards and `l1.end` keeps the inclination the constraints pin.
#[test]
fn dragging_the_midpoint_line_end_keeps_the_sketch_solved() {
    let mut sk = collapsed_construction_sketch();
    let e1 = 2; // l1.end
    let ok = move_point_towards(&mut sk, e1, v(10.0, 4.0), 2.0, None, |_, _| true);
    assert!(ok, "the cursor gave up on a solved sketch");
    let rep = solve_with(&mut sk, &Hold::default(), None, Analysis::StatusOnly, None);
    assert!(rep.ok(), "the drag broke the sketch: {:?}", rep.failing);
    // `l1.end` stayed on the 50-degree line through the origin: the angle dimension held.
    let at = sk.points[e1].pos;
    let off = (at.y.atan2(at.x) - 50f64.to_radians()).abs();
    assert!(off < 1e-3, "l1.end went to {at:?} (off its line by {off} rad)");
}

/// An infinite reference can have very close defining points in a saved sketch. Its
/// horizontal/vertical constraint still fixes its visible direction, not just the tiny
/// distance between the defining points (#11001).
#[test]
fn an_angle_keeps_a_short_infinite_reference_horizontal_or_vertical() {
    for (vertical, sign, origin) in [
        (false, 1.0, v(0.0, 0.0)),
        (true, 1.0, v(0.0, 0.0)),
        (false, -1.0, v(0.0, 0.0)),
        (true, -1.0, v(0.0, 0.0)),
        (false, 1.0, v(25.0, -17.0)),
        (true, 1.0, v(25.0, -17.0)),
    ] {
        for length in [6.36e-8, 1.0] {
            let mut sk = Sketch::new();
            sk.points[0].pos = origin;
            let direction = (if vertical { v(0.0, 1.0) } else { v(1.0, 0.0) }) * sign;
            let end = sk.add_point(origin + direction * length, None).unwrap();
            let arm_end = sk.add_point(origin + (if vertical { v(-10.0, 12.0) } else { v(12.0, 10.0) }) * sign, None).unwrap();
            let axis = sk.add_line_pts(0, end, Some("axis")).unwrap();
            sk.curves[axis].infinite = true;
            sk.curves[axis].construction = true;
            let arm = sk.add_line_pts(0, arm_end, Some("arm")).unwrap();
            let constraint = if vertical { ConstraintKind::Vertical { l: axis } } else { ConstraintKind::Horizontal { l: axis } };
            sk.add_constraint(constraint, None).unwrap();
            sk.add_constraint(ConstraintKind::Angle { a: axis, b: arm, value: 30f64.to_radians(), flip: false }, None).unwrap();

            let report = crate::solve(&mut sk);
            assert!(report.ok(), "vertical={vertical}, length={length}: {report:?}");
            let actual = (sk.points[end].pos - origin).normalized().expect("the reference retains a direction");
            assert!(actual.dist(direction) < 1e-6, "vertical={vertical}, length={length}: reference rotated to {actual:?}");
            let arm_direction = (sk.points[arm_end].pos - origin).normalized().unwrap();
            let angle = actual.cross(arm_direction).atan2(actual.dot(arm_direction));
            assert!((angle - 30f64.to_radians()).abs() < 1e-6, "the free line must satisfy the angle: {angle}");
        }
    }
}
