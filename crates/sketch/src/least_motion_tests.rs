//! A solve with geometry held moves the rest as little as it can.

use solidcraft_geom::Vec2;

use crate::{Analysis, ConstraintKind, Hold, Sketch, solve_with};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn an_existing_short_angular_reference_still_solves() {
    let mut sk = Sketch::new();
    let reference = sk.add_line(v(0.0, 0.0), v(1e-7, 0.0), Some(0), None, None).unwrap();
    let line = sk.add_line(v(0.0, 0.0), v(10.0, 10.0), Some(0), None, None).unwrap();
    sk.curves[reference].construction = true;
    sk.add_constraint(ConstraintKind::Horizontal { l: reference }, None).unwrap();
    sk.add_constraint(ConstraintKind::Angle { a: reference, b: line, value: std::f64::consts::FRAC_PI_4, flip: false }, None).unwrap();
    let report = crate::solve(&mut sk);
    assert!(report.ok(), "an unchanged short reference is not a new collapse: {report:?}");
}

#[test]
fn dragging_an_angular_constrained_line_keeps_its_construction_reference() {
    for target in [v(11.0, 11.0), v(10.0, 20.0), v(-10.0, 20.0), v(-10.0, -10.0), v(10.0, -10.0)] {
        let mut sk = Sketch::new();
        let reference = sk.add_line(v(0.0, 0.0), v(20.0, 0.0), Some(0), None, None).unwrap();
        sk.curves[reference].construction = true;
        let line = sk.add_line(v(0.0, 0.0), v(10.0, 10.0), Some(0), None, None).unwrap();
        sk.add_constraint(ConstraintKind::Horizontal { l: reference }, None).unwrap();
        sk.add_constraint(ConstraintKind::Angle { a: reference, b: line, value: std::f64::consts::FRAC_PI_4, flip: false }, None).unwrap();
        assert!(crate::solve(&mut sk).ok());
        let crate::CurveKind::Line { b: endpoint, .. } = sk.curves[line].kind else { panic!("line") };
        for step in 1..=20 {
            let before: Vec<_> = sk.points.iter().map(|p| p.pos).collect();
            sk.points[endpoint].pos = v(10.0, 10.0) + (target - v(10.0, 10.0)) * (step as f64 / 20.0);
            let report = solve_with(&mut sk, &Hold { points: vec![endpoint], radii: vec![] }, Some(&before), Analysis::StatusOnly, None);
            if target == v(11.0, 11.0) {
                assert!(report.ok(), "a compatible drag must still solve: {report:?}");
            }
            if report.ok() {
                let crate::CurveKind::Line { a, b } = sk.curves[reference].kind else { panic!("line") };
                assert!(sk.points[a].pos.dist(sk.points[b].pos) > 1e-4, "construction line collapsed toward {target:?} at step {step}: {report:?}");
            }
        }
    }
}

/// A rectangle with a divider whose ends are held on the bottom and top sides (point on curve).
/// Dragging the top-left corner tilts the top side: the divider's top end slides along with it,
/// the far corner stays.
#[test]
fn a_point_held_on_a_line_slides_instead_of_swinging_the_line() {
    let mut sk = Sketch::new();
    let c: Vec<usize> = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)].iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    let sides: Vec<usize> = (0..4).map(|i| sk.add_line_pts(c[i], c[(i + 1) % 4], None).unwrap()).collect();
    let bottom = sk.add_point(v(20.0, 0.0), None).unwrap();
    let top = sk.add_point(v(20.0, 30.0), None).unwrap();
    sk.add_line_pts(bottom, top, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: bottom, c: sides[0] }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: top, c: sides[2] }, None).unwrap();
    let before: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
    sk.points[c[3]].pos = v(1.0, 31.0);
    let rep = solve_with(&mut sk, &Hold { points: vec![c[3]], radii: vec![] }, Some(&before), Analysis::StatusOnly, None);
    assert!(rep.ok(), "{rep:?}");
    let moved = |i: usize| sk.points[i].pos.dist(before[i]);
    // The top side through (1, 31) and (40, 30) passes x = 20 at y ≈ 30.51: the divider's end
    // needs about half a millimetre. The far corner needs nothing.
    assert!(moved(top) < 0.6, "divider end moved {}", moved(top));
    assert!(moved(c[2]) < 0.3, "far corner moved {}", moved(c[2]));
    assert!(moved(c[1]) < 1e-6 && moved(bottom) < 1e-6, "the bottom has no reason to move");
}

/// The same rectangle, dragging the top-right corner down: the top side turns about its other
/// corner, which stays, and the divider's end slides down with it.
#[test]
fn the_far_corner_stays_when_the_side_a_point_is_held_on_turns() {
    let mut sk = Sketch::new();
    let c: Vec<usize> = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)].iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    let sides: Vec<usize> = (0..4).map(|i| sk.add_line_pts(c[i], c[(i + 1) % 4], None).unwrap()).collect();
    let bottom = sk.add_point(v(20.0, 0.0), None).unwrap();
    let top = sk.add_point(v(20.0, 30.0), None).unwrap();
    sk.add_line_pts(bottom, top, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: bottom, c: sides[0] }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointOnCurve { p: top, c: sides[2] }, None).unwrap();
    // Down in steps, as a live drag goes: each solve starts from the last.
    for k in 1..=10 {
        let before: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
        sk.points[c[2]].pos = v(40.0, 30.0 - k as f64);
        let rep = solve_with(&mut sk, &Hold { points: vec![c[2]], radii: vec![] }, Some(&before), Analysis::StatusOnly, None);
        assert!(rep.ok(), "{rep:?}");
    }
    let corner = sk.points[c[3]].pos;
    assert!(corner.dist(v(0.0, 30.0)) < 0.1, "the far corner went to {corner:?}");
    // On the turned side, near where it was.
    let (end, a, b) = (sk.points[top].pos, corner, sk.points[c[2]].pos);
    let off_side = ((b - a).cross(end - a) / a.dist(b)).abs();
    assert!(off_side < 1e-6 && (end.x - 20.0).abs() < 1.0, "the divider's end is at {end:?}");
}
