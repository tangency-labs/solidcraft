//! A line and an arc that meet at two separate points tied by `Coincident` (not at one shared
//! point), plus `Tangent`. The tangency has to be read at that touch point: the touch-free form
//! (distance from the centre equals the radius) is a double root there, so it converged to only
//! sqrt(residual) of tangent and lost a row of rank when the start already was tangent.

use solidcraft_geom::Vec2;

use crate::tests::{Lcg, v};
use crate::*;

/// Circumcentre of three points.
fn circumcentre(a: Vec2, t: Vec2, b: Vec2) -> Vec2 {
    let d = 2.0 * (a.x * (t.y - b.y) + t.x * (b.y - a.y) + b.x * (a.y - t.y));
    let (a2, t2, b2) = (a.len().powi(2), t.len().powi(2), b.len().powi(2));
    Vec2::new((a2 * (t.y - b.y) + t2 * (b.y - a.y) + b2 * (a.y - t.y)) / d, (a2 * (b.x - t.x) + t2 * (a.x - b.x) + b2 * (t.x - a.x)) / d)
}

struct Joint {
    sk: Sketch,
    line: usize,
    arc: usize,
    line_end: usize,
    arc_start: usize,
}

/// Line (0,0)-(10,0) and the arc through (10,0), (15,5), (10,10), each coordinate moved by `j`.
fn joint(j: [f64; 10]) -> Joint {
    let mut sk = Sketch::new();
    let line = sk.add_line(v(j[0], j[1]), v(10.0 + j[2], j[3]), None, None, Some("l")).unwrap();
    let (a, t, b) = (v(10.0 + j[4], j[5]), v(15.0 + j[6], 5.0 + j[7]), v(10.0 + j[8], 10.0 + j[9]));
    let arc = sk.add_arc(circumcentre(a, t, b), a, b, [None, None, None], Some("a")).unwrap();
    let (CurveKind::Line { b: line_end, .. }, CurveKind::Arc { a: arc_start, .. }) = (sk.curves[line].kind.clone(), sk.curves[arc].kind.clone())
    else {
        panic!("a line and an arc")
    };
    sk.add_constraint(ConstraintKind::Coincident { p: line_end, q: arc_start }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: line, b: arc }, None).unwrap();
    Joint { sk, line, arc, line_end, arc_start }
}

/// The centre-to-joint radius's component along the line: the tangency error in mm.
fn tangency_error(jt: &Joint) -> f64 {
    let (p0, p1) = match jt.sk.curves[jt.line].kind {
        CurveKind::Line { a, b } => (jt.sk.points[a].pos, jt.sk.points[b].pos),
        _ => panic!("line"),
    };
    let d = p1 - p0;
    (jt.sk.points[jt.arc_start].pos - jt.sk.center(jt.arc).unwrap()).dot(d) / d.len()
}

#[test]
fn coincident_tangent_holds_in_the_stored_points() {
    let mut rng = Lcg(12345);
    for amp in [0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0] {
        let mut j = [0.0; 10];
        for x in &mut j {
            *x = rng.range(-amp, amp);
        }
        let mut jt = joint(j);
        let rep = solve(&mut jt.sk);
        assert!(rep.ok(), "amp {amp}: {rep:?}");
        let err = tangency_error(&jt);
        // Was 5e-5 .. 1e-4 mm with a reported residual of 1e-10.
        assert!(err.abs() < 1e-8, "amp {amp}: tangency error {err}");
        let gap = jt.sk.points[jt.line_end].pos.dist(jt.sk.points[jt.arc_start].pos);
        assert!(gap < 1e-8, "amp {amp}: gap {gap}");
        assert_eq!(rep.dof, 6, "amp {amp}: {rep:?}");
        assert!(rep.redundant.is_empty(), "amp {amp}: {rep:?}");
    }
}

#[test]
fn coincident_tangent_dof_does_not_depend_on_the_start() {
    let rough = [0.03, -0.04, 0.02, 0.05, -0.05, 0.04, 0.04, -0.03, -0.02, 0.05];
    let mut reports = Vec::new();
    for j in [[0.0; 10], rough] {
        let mut jt = joint(j);
        reports.push(solve(&mut jt.sk));
    }
    for rep in &reports {
        // 4 (line) + 5 (arc) - 2 (coincident) - 1 (tangent).
        assert_eq!(rep.dof, 6, "{rep:?}");
        assert!(rep.redundant.is_empty(), "{rep:?}");
        assert!(rep.failing.is_empty(), "{rep:?}");
    }
}

#[test]
fn a_coincident_chain_still_finds_the_touch_point() {
    let mut jt = joint([0.03, -0.04, 0.02, 0.05, -0.05, 0.04, 0.04, -0.03, -0.02, 0.05]);
    // The line end is tied to the arc start only through a third point.
    let mid = jt.sk.add_point(v(10.0, 0.0), None).unwrap();
    jt.sk.constraints.remove(0);
    jt.sk.add_constraint(ConstraintKind::Coincident { p: jt.line_end, q: mid }, None).unwrap();
    jt.sk.add_constraint(ConstraintKind::Coincident { p: mid, q: jt.arc_start }, None).unwrap();
    let rep = solve(&mut jt.sk);
    assert!(rep.ok(), "{rep:?}");
    let err = tangency_error(&jt);
    assert!(err.abs() < 1e-8, "tangency error {err}");
}

#[test]
fn a_duplicate_dimension_beside_the_tangent_joint_is_still_redundant() {
    let mut jt = joint([0.0; 10]);
    let l = jt.line;
    jt.sk.add_constraint(ConstraintKind::Length { l, value: 10.0 }, None).unwrap();
    jt.sk.add_constraint(ConstraintKind::Length { l, value: 10.0 }, None).unwrap();
    let rep = solve(&mut jt.sk);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.redundant.len(), 1, "{rep:?}");
    assert_eq!(rep.dof, 5, "{rep:?}");
}

#[test]
fn conflicting_dimensions_beside_the_tangent_joint_still_fail() {
    let mut jt = joint([0.0; 10]);
    let l = jt.line;
    jt.sk.add_constraint(ConstraintKind::Length { l, value: 10.0 }, None).unwrap();
    jt.sk.add_constraint(ConstraintKind::Length { l, value: 12.0 }, None).unwrap();
    let rep = solve(&mut jt.sk);
    assert_eq!(rep.status, SolveStatus::Failed, "{rep:?}");
    assert!(!rep.failing.is_empty(), "{rep:?}");
}

/// A line at y = 5 and a circle of radius 5 at the origin; the contact is a free point.
struct FreePoint {
    sk: Sketch,
    line: usize,
    circle: usize,
    p: usize,
}

fn free_point_contact(j: [f64; 8], tied_to_line_end: bool) -> FreePoint {
    let mut sk = Sketch::new();
    let line =
        sk.add_line(v(-8.0 + j[0], 5.0 + j[1]), v(if tied_to_line_end { 0.0 } else { 8.0 } + j[2], 5.0 + j[3]), None, None, Some("l")).unwrap();
    let circle = sk.add_circle(v(j[4], j[5]), 5.0, None, Some("c")).unwrap();
    let p = sk.add_point(v(j[6], 5.0 + j[7]), None).unwrap();
    let CurveKind::Line { b: line_end, .. } = sk.curves[line].kind.clone() else { panic!("line") };
    if tied_to_line_end {
        // S10: line end ~ free point p, p on the circle.
        sk.add_constraint(ConstraintKind::Coincident { p, q: line_end }, None).unwrap();
    } else {
        // S2b: p on the line and on the circle.
        sk.add_constraint(ConstraintKind::PointOnCurve { p, c: line }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::PointOnCurve { p, c: circle }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: line, b: circle }, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: circle, value: 5.0 }, None).unwrap();
    FreePoint { sk, line, circle, p }
}

fn free_point_error(f: &FreePoint) -> f64 {
    let CurveKind::Line { a, b } = f.sk.curves[f.line].kind else { panic!("line") };
    let d = f.sk.points[b].pos - f.sk.points[a].pos;
    (f.sk.points[f.p].pos - f.sk.center(f.circle).unwrap()).dot(d) / d.len()
}

#[test]
fn a_free_point_contact_is_tangent_and_counted_the_same_from_every_start() {
    // 9 variables (line 4, circle 3, point 2); equations: point on circle, tangent, radius, plus either
    // the coincidence (2, S10: dof 4) or the point on the line (1, S2b: dof 5).
    let mut rng = Lcg(2024);
    for tied in [true, false] {
        for amp in [0.0, 0.01, 0.05, 0.5, 1.0] {
            let mut j = [0.0; 8];
            for x in &mut j {
                *x = if amp == 0.0 { 0.0 } else { rng.range(-amp, amp) };
            }
            let mut f = free_point_contact(j, tied);
            let rep = solve(&mut f.sk);
            assert!(rep.ok(), "tied {tied} amp {amp}: {rep:?}");
            assert_eq!(rep.dof, if tied { 4 } else { 5 }, "tied {tied} amp {amp}: {rep:?}");
            assert!(rep.redundant.is_empty(), "tied {tied} amp {amp}: {rep:?}");
            let err = free_point_error(&f);
            assert!(err.abs() < 1e-8, "tied {tied} amp {amp}: contact perpendicular {err}");
        }
    }
}

#[test]
fn a_tangent_to_a_collapsed_round_curve_fails() {
    // The line passes through the arc's centre while the joint is tangent: only a radius of 0 holds.
    let mut jt = joint([0.03, -0.04, 0.02, 0.05, -0.05, 0.04, 0.04, -0.03, -0.02, 0.05]);
    let CurveKind::Arc { c, .. } = jt.sk.curves[jt.arc].kind else { panic!("arc") };
    jt.sk.add_constraint(ConstraintKind::PointOnCurve { p: c, c: jt.line }, None).unwrap();
    let rep = solve(&mut jt.sk);
    assert_eq!(rep.status, SolveStatus::Failed, "{rep:?}");
    assert!(rep.failing.contains(&"k2".to_string()), "{rep:?}");
}

#[test]
fn an_infeasible_tangent_joint_names_the_tangent_too() {
    let mut jt = joint([0.03, -0.04, 0.02, 0.05, -0.05, 0.04, 0.04, -0.03, -0.02, 0.05]);
    let CurveKind::Arc { c, .. } = jt.sk.curves[jt.arc].kind else { panic!("arc") };
    jt.sk.add_constraint(ConstraintKind::Radius { c: jt.arc, value: 5.0 }, None).unwrap();
    jt.sk.add_constraint(ConstraintKind::PointLineDistance { p: c, l: jt.line, value: 4.0 }, None).unwrap();
    let rep = solve(&mut jt.sk);
    assert_eq!(rep.status, SolveStatus::Failed, "{rep:?}");
    // k1 is the coincident, k2 the tangent.
    assert!(rep.failing.contains(&"k2".to_string()), "{rep:?}");
}

/// The unsolved sketch of the critic's c2_t6: the tangent is violated because the Coincident is.
#[test]
fn constraint_residual_still_shows_a_violated_tying_constraint() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(-5.0, 3.0), v(5.0, 3.0), None, None, Some("l")).unwrap();
    let c = sk.add_circle(v(0.0, 0.0), 5.0, None, Some("c")).unwrap();
    let q = sk.add_point(v(0.0, 5.0), None).unwrap();
    let CurveKind::Line { b: end, .. } = sk.curves[l].kind.clone() else { panic!("line") };
    sk.add_constraint(ConstraintKind::PointOnCurve { p: q, c }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: end, q }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l, b: c }, None).unwrap();
    let r = constraint_residual(&sk, 2).unwrap();
    // The distance from the centre to the line is 3 against a radius of 5.
    assert!((r - 2.0).abs() < 1e-12, "{r}");
}

#[test]
fn a_midpoint_contact_is_tangent_and_counted_the_same_from_every_start() {
    let mut rng = Lcg(5);
    for amp in [0.0, 0.05, 0.5] {
        let mut j = [0.0; 6];
        for x in &mut j {
            *x = if amp == 0.0 { 0.0 } else { rng.range(-amp, amp) };
        }
        let mut sk = Sketch::new();
        let l = sk.add_line(v(-8.0 + j[0], 5.0 + j[1]), v(8.0 + j[2], 5.0 + j[3]), None, None, Some("l")).unwrap();
        let c = sk.add_circle(v(j[4], j[5]), 5.0, None, Some("c")).unwrap();
        let m = sk.add_point(v(0.0, 5.0), None).unwrap();
        sk.add_constraint(ConstraintKind::Midpoint { p: m, l }, None).unwrap();
        sk.add_constraint(ConstraintKind::PointOnCurve { p: m, c }, None).unwrap();
        sk.add_constraint(ConstraintKind::Tangent { a: l, b: c }, None).unwrap();
        let rep = solve(&mut sk);
        assert!(rep.ok(), "amp {amp}: {rep:?}");
        // 4 + 3 + 2 = 9 variables, midpoint 2, on circle 1, tangent 1.
        assert_eq!(rep.dof, 5, "amp {amp}: {rep:?}");
        assert!(rep.redundant.is_empty(), "amp {amp}: {rep:?}");
        let CurveKind::Line { a, b } = sk.curves[l].kind else { panic!("line") };
        let d = sk.points[b].pos - sk.points[a].pos;
        let err = (sk.points[m].pos - sk.center(c).unwrap()).dot(d) / d.len();
        assert!(err.abs() < 1e-8, "amp {amp}: {err}");
    }
}

#[test]
fn a_tangent_is_never_failing_while_the_sketch_is_solved() {
    let mut rng = Lcg(31);
    for trial in 0..60 {
        let amp = [0.05, 0.4, 1.0][trial % 3];
        let mut j = [0.0; 10];
        for x in &mut j {
            *x = rng.range(-amp, amp);
        }
        let mut jt = joint(j);
        let rep = solve(&mut jt.sk);
        if rep.status == SolveStatus::Solved {
            assert!(rep.failing.is_empty(), "trial {trial}: {rep:?}");
        }
    }
}
