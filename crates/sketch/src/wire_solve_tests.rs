use solidcraft_geom::Vec3;

use super::*;
use crate::WireSpace;

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn line(sk: &mut Sketch, a: Vec3, b: Vec3) -> String {
    sk.add_drawn_wire_in(&[a, b], WireSpace::Component).unwrap()
}

fn at(sk: &Sketch, r: &str) -> Vec3 {
    sk.wire_point_at(&sk.wire_point(r).unwrap()).unwrap()
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-7
}

#[test]
fn references_name_ends_through_points_and_fit_indices() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let a = sk.add_drawn_arc_in(&[v(0.0, 0.0, 0.0), v(5.0, 5.0, 0.0), v(10.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let q = sk.add_drawn_wire_in(&[v(1.0, 2.0, 3.0)], WireSpace::Component).unwrap();
    assert_eq!(sk.wire_point(&format!("{l}.start")), Some(WirePoint { wire: l.clone(), index: 0 }));
    assert_eq!(sk.wire_point(&format!("{l}.end")), Some(WirePoint { wire: l.clone(), index: 1 }));
    assert_eq!(sk.wire_point(&format!("{l}.1")), Some(WirePoint { wire: l.clone(), index: 1 }));
    assert_eq!(sk.wire_point(&format!("{a}.through")), Some(WirePoint { wire: a.clone(), index: 1 }));
    assert_eq!(sk.wire_point(&q), Some(WirePoint { wire: q.clone(), index: 0 }));
    // A line has no through point, no third fit point, and a line alone is not a point.
    assert_eq!(sk.wire_point(&format!("{l}.through")), None);
    assert_eq!(sk.wire_point(&format!("{l}.2")), None);
    assert_eq!(sk.wire_point(&l), None);
    assert!(sk.is_drawn_line(&l) && !sk.is_drawn_line(&a) && !sk.is_drawn_line(&q));
}

#[test]
fn a_sketch_without_3d_constraints_moves_nothing_and_counts_three_freedoms_per_point() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 3.0, 4.0));
    let before = sk.clone();
    let rep = sk.solve_wires(None);
    assert!(rep.ok);
    assert_eq!(rep.dof, 6);
    assert_eq!(sk, before);
    let _ = l;
}

#[test]
fn along_axis_turns_a_line_onto_the_axis_and_keeps_its_length_along_it() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(100.0, 2.0, -3.0));
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let (a, b) = (at(&sk, &format!("{l}.start")), at(&sk, &format!("{l}.end")));
    // Only the ends' y and z need to agree: 2 of the 6 freedoms go.
    assert!((a.y - b.y).abs() < 1e-9 && (a.z - b.z).abs() < 1e-9, "{a:?} {b:?}");
    assert!(((b.x - a.x) - 100.0).abs() < 1e-6, "least motion keeps the run along x");
    assert_eq!(rep.dof, 4);
    // The polyline follows the fit points.
    let w = &sk.wires[sk.wire_index(&l).unwrap()];
    assert!(close(w.pts[0], a) && close(w.pts[1], b));
}

#[test]
fn a_fixed_corner_holds_and_joined_lines_stay_joined() {
    // An L in 3D: a run along X, then a rise along Z, joined at the corner and fixed at the start.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(50.0, 1.0, 1.0));
    let b = line(&mut sk, v(50.0, 1.0, 1.0), v(49.0, 2.0, 40.0));
    let joints = sk.join_drawn_wires(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(joints.len(), 1);
    let start = sk.wire_point(&format!("{a}.start")).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(0.0, 0.0, 0.0) }).unwrap();
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: a.clone(), axis: Axis::X }).unwrap();
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: b.clone(), axis: Axis::Z }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    assert!(close(at(&sk, &format!("{a}.start")), v(0.0, 0.0, 0.0)));
    let corner = at(&sk, &format!("{a}.end"));
    assert!(close(corner, at(&sk, &format!("{b}.start"))), "the joint holds");
    assert!(corner.y.abs() < 1e-9 && corner.z.abs() < 1e-9, "the run lies on the X axis: {corner:?}");
    let top = at(&sk, &format!("{b}.end"));
    assert!((top.x - corner.x).abs() < 1e-9 && (top.y - corner.y).abs() < 1e-9, "the rise is vertical: {top:?}");
    // 12 coordinates; the fix takes 3, the joint 3, each axis 2: two lengths are left.
    assert_eq!(rep.dof, 2);
}

#[test]
fn a_point_on_a_line_slides_onto_it() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let q = sk.add_drawn_wire_in(&[v(4.0, 3.0, 4.0)], WireSpace::Component).unwrap();
    for end in ["start", "end"] {
        let p = sk.wire_point(&format!("{l}.{end}")).unwrap();
        let at = sk.wire_point_at(&p).unwrap();
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at }).unwrap();
    }
    sk.add_wire_constraint(WireConstraintKind::PointOnLine { p: sk.wire_point(&q).unwrap(), line: l.clone() }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    // The nearest point of the X axis: least motion drops it straight down onto the line.
    assert!(close(at(&sk, &q), v(4.0, 0.0, 0.0)), "{:?}", at(&sk, &q));
    // The line is fixed (6), the point keeps its slide along it.
    assert_eq!(rep.dof, 1);
}

#[test]
fn conflicting_constraints_fail_name_the_conflict_and_move_nothing() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    for (end, to) in [("start", v(0.0, 0.0, 0.0)), ("end", v(0.0, 10.0, 0.0))] {
        let p = sk.wire_point(&format!("{l}.{end}")).unwrap();
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at: to }).unwrap();
    }
    let k = sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X }).unwrap();
    let before = sk.wires.clone();
    let rep = sk.solve_wires(None);
    assert!(!rep.ok);
    assert!(!rep.failing.is_empty() && rep.failing.iter().any(|f| *f == k || sk.wire_constraints.iter().any(|c| c.id == *f)));
    assert_eq!(sk.wires, before);
}

#[test]
fn a_held_point_stays_where_it_was_put_and_the_rest_follows() {
    // Two joined lines; dragging the free end of the second one keeps the joint and the first
    // line's fixed start.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(10.0, 0.0, 0.0), v(10.0, 0.0, 10.0));
    sk.join_drawn_wires(&[a.clone(), b.clone()]).unwrap();
    let start = sk.wire_point(&format!("{a}.start")).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(0.0, 0.0, 0.0) }).unwrap();
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: b.clone(), axis: Axis::Z }).unwrap();
    // Drag the joint to (20, 5, 0): the rise goes with it.
    let joint = sk.wire_point(&format!("{b}.start")).unwrap();
    let w = sk.wire_index(&b).unwrap();
    sk.wires[w].fit[0] = v(20.0, 5.0, 0.0);
    let rep = sk.solve_wires(Some(&joint));
    assert!(rep.ok, "{rep:?}");
    assert!(close(at(&sk, &format!("{b}.start")), v(20.0, 5.0, 0.0)));
    assert!(close(at(&sk, &format!("{a}.end")), v(20.0, 5.0, 0.0)));
    let top = at(&sk, &format!("{b}.end"));
    assert!((top.x - 20.0).abs() < 1e-9 && (top.y - 5.0).abs() < 1e-9, "{top:?}");
}

#[test]
fn an_arc_squeezed_into_a_line_fails_instead_of_breaking_the_curve() {
    let mut sk = Sketch::new();
    let a = sk.add_drawn_arc_in(&[v(0.0, 0.0, 0.0), v(5.0, 5.0, 0.0), v(10.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    for end in ["start", "end"] {
        let p = sk.wire_point(&format!("{l}.{end}")).unwrap();
        let at = sk.wire_point_at(&p).unwrap();
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at }).unwrap();
    }
    sk.add_wire_constraint(WireConstraintKind::PointOnLine { p: sk.wire_point(&format!("{a}.through")).unwrap(), line: l.clone() }).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Coincident {
        a: sk.wire_point(&format!("{a}.start")).unwrap(),
        b: sk.wire_point(&format!("{l}.start")).unwrap(),
    })
    .unwrap();
    sk.add_wire_constraint(WireConstraintKind::Coincident {
        a: sk.wire_point(&format!("{a}.end")).unwrap(),
        b: sk.wire_point(&format!("{l}.end")).unwrap(),
    })
    .unwrap();
    let before = sk.wires.clone();
    let rep = sk.solve_wires(None);
    assert!(!rep.ok);
    assert_eq!(sk.wires, before, "the arc keeps its shape");
}

#[test]
fn constraints_are_checked_deduplicated_and_pruned_with_their_curves() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let world = sk.add_drawn_wire_in(&[v(0.0, 5.0, 0.0), v(10.0, 5.0, 0.0)], WireSpace::World).unwrap();
    let a = sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X }).unwrap();
    let again = sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X }).unwrap();
    assert_eq!(a, again, "the same relation is not added twice");
    // Different coordinate spaces, an unknown curve, a point coincident with itself.
    let start = sk.wire_point(&format!("{l}.start")).unwrap();
    let other = sk.wire_point(&format!("{world}.start")).unwrap();
    assert!(sk.add_wire_constraint(WireConstraintKind::Coincident { a: start.clone(), b: other }).is_err());
    assert!(sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: "nope".into(), axis: Axis::Y }).is_err());
    assert!(sk.add_wire_constraint(WireConstraintKind::Coincident { a: start.clone(), b: start }).is_err());
    // Removing the line removes what named it; the round trip keeps the rest.
    let text = serde_json::to_string(&sk).unwrap();
    let back: Sketch = serde_json::from_str(&text).unwrap();
    assert_eq!(back.wire_constraints, sk.wire_constraints);
    back.validate_wire_constraints().unwrap();
    sk.remove_wires(std::slice::from_ref(&l));
    assert!(sk.wire_constraints.is_empty());
}

#[test]
fn hostile_saved_constraints_are_refused() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    sk.wire_constraints
        .push(WireConstraint { id: "k9".into(), kind: WireConstraintKind::Fix { p: WirePoint { wire: l.clone(), index: 7 }, at: v(0.0, 0.0, 0.0) } });
    assert!(sk.validate_wire_constraints().is_err());
    sk.wire_constraints.clear();
    sk.wire_constraints.push(WireConstraint {
        id: "k9".into(),
        kind: WireConstraintKind::Fix { p: WirePoint { wire: l.clone(), index: 0 }, at: v(f64::NAN, 0.0, 0.0) },
    });
    assert!(sk.validate_wire_constraints().is_err());
    sk.wire_constraints.clear();
    for _ in 0..2 {
        sk.wire_constraints.push(WireConstraint { id: "k9".into(), kind: WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X } });
    }
    assert!(sk.validate_wire_constraints().is_err(), "duplicate ids");
}
