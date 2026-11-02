#![cfg(test)]

//! The radial distance between concentric circles or arcs:
//! solved numerically, measured, and refused on non-round curves or negative values.

use solidcraft_geom::Vec2;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
use crate::*;

fn concentric_circles(centre: Vec2, r1: f64, r2: f64) -> (Sketch, usize, usize, usize) {
    let mut sk = Sketch::new();
    let c1 = sk.add_circle(centre, r1, None, None).unwrap();
    let CurveKind::Circle { c: ci, .. } = sk.curves[c1].kind else { panic!("not a circle") };
    for p in [ci] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::Radius { c: c1, value: r1 }, None).unwrap();
    let c2 = sk.add_circle(centre, r2, Some(ci), None).unwrap();
    (sk, c1, c2, ci)
}

fn kind_at(sk: &Sketch, k: usize) -> ConstraintKind {
    sk.constraints[k].kind.clone()
}

#[test]
fn concentric_distance_drives_outer_circle_radius() {
    // Inner radius 35 (diameter 70), outer radius 45 (gap 10).
    let (mut sk, c1, c2, _) = concentric_circles(v(0.0, 0.0), 35.0, 45.0);
    let k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::ConcentricDistance { a: c1, b: c2, value: 10.0 }, None).unwrap();
    assert!((solver::measure_dimension(&sk, &kind_at(&sk, k)).unwrap() - 10.0).abs() < 1e-9);
    assert!(solve(&mut sk).ok());

    // Set concentric distance to 18mm: outer radius becomes 35 + 18 = 53mm (diameter 106mm).
    sk.constraints[k].kind.set_value(18.0);
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    let r2 = sk.radius(c2).unwrap();
    assert!((r2 - 53.0).abs() < 1e-7, "r2: {r2}");
    assert!((solver::measure_dimension(&sk, &kind_at(&sk, k)).unwrap() - 18.0).abs() < 1e-7);
}

#[test]
fn concentric_distance_drives_inner_circle_when_outer_is_fixed() {
    let mut sk = Sketch::new();
    let c2 = sk.add_circle(v(0.0, 0.0), 60.0, None, None).unwrap();
    let CurveKind::Circle { c: ci, .. } = sk.curves[c2].kind else { panic!("not a circle") };
    sk.add_constraint(ConstraintKind::Fix { p: ci }, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: c2, value: 60.0 }, None).unwrap();
    let c1 = sk.add_circle(v(0.0, 0.0), 40.0, Some(ci), None).unwrap();

    let _k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::ConcentricDistance { a: c1, b: c2, value: 18.0 }, None).unwrap();
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    let r1 = sk.radius(c1).unwrap();
    assert!((r1 - 42.0).abs() < 1e-7, "r1: {r1}");
}

#[test]
fn concentric_distance_works_with_arcs() {
    let mut sk = Sketch::new();
    let a1 = sk.add_arc(v(0.0, 0.0), v(20.0, 0.0), v(0.0, 20.0), [None; 3], None).unwrap();
    let cp = sk.center_point(a1).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: cp }, None).unwrap();
    let a2 = sk.add_arc(v(0.0, 0.0), v(30.0, 0.0), v(0.0, 30.0), [Some(cp), None, None], None).unwrap();

    let _k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::ConcentricDistance { a: a1, b: a2, value: 15.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    let r1 = sk.radius(a1).unwrap();
    let r2 = sk.radius(a2).unwrap();
    assert!((r2 - r1 - 15.0).abs() < 1e-7, "{r1} {r2}");
}

#[test]
fn concentric_distance_requires_distinct_round_curves() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(40.0, 0.0), None, None, None).unwrap();
    let c = sk.add_circle(v(0.0, 0.0), 10.0, None, None).unwrap();
    assert!(sk.add_constraint(ConstraintKind::ConcentricDistance { a: l, b: c, value: 5.0 }, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::ConcentricDistance { a: c, b: l, value: 5.0 }, None).is_err());
    assert!(sk.add_constraint(ConstraintKind::ConcentricDistance { a: c, b: c, value: 5.0 }, None).is_err());
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(sk.add_constraint(ConstraintKind::ConcentricDistance { a: c, b: c, value }, None).is_err());
    }
}
