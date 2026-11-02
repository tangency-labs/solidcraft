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
    let joints = sk.join_drawn_ends(&[a.clone(), b.clone()]).unwrap();
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
    assert_eq!(rep.failing, vec![k], "the axis between two fixed ends is the conflict");
    assert_eq!(sk.wires, before);
}

#[test]
fn a_held_point_stays_where_it_was_put_and_the_rest_follows() {
    // Two joined lines; dragging the free end of the second one keeps the joint and the first
    // line's fixed start.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(10.0, 0.0, 0.0), v(10.0, 0.0, 10.0));
    sk.join_drawn_ends(&[a.clone(), b.clone()]).unwrap();
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
    // As a design is opened, one naming a point its curve no longer has is dropped instead.
    assert_eq!(sk.prune_wire_constraints(), 1);
    sk.validate_wire_constraints().unwrap();
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

#[test]
fn curves_drawn_one_at_a_time_join_the_ends_they_meet_once() {
    // Three lines meeting at one corner, drawn one call each, and a 3D point on the far end.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 1.0, 1.0));
    let b = line(&mut sk, v(10.0, 1.0, 1.0), v(10.0, 10.0, 1.0));
    assert_eq!(sk.join_drawn_ends(std::slice::from_ref(&b)).unwrap().len(), 1);
    let c = line(&mut sk, v(10.0, 1.0, 1.0), v(10.0, 1.0, 10.0));
    // The corner is joined already: one more Coincident ties the third line in, not two.
    assert_eq!(sk.join_drawn_ends(std::slice::from_ref(&c)).unwrap().len(), 1);
    let q = sk.add_drawn_wire_in(&[v(10.0, 10.0, 1.0)], WireSpace::Component).unwrap();
    assert_eq!(sk.join_drawn_ends(std::slice::from_ref(&q)).unwrap().len(), 1);
    assert!(sk.join_drawn_ends(&[a.clone(), b.clone(), c.clone(), q.clone()]).unwrap().is_empty(), "nothing joined twice");
    // A world-space curve at the same place is not joined.
    let w = sk.add_drawn_wire_in(&[v(0.0, 0.0, 0.0), v(0.0, 5.0, 0.0)], WireSpace::World).unwrap();
    assert!(sk.join_drawn_ends(std::slice::from_ref(&w)).unwrap().is_empty());
    // Turning the first line onto X moves the corner, and all three ends go with it.
    let start = sk.wire_point(&format!("{a}.start")).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(0.0, 0.0, 0.0) }).unwrap();
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: a.clone(), axis: Axis::X }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let corner = at(&sk, &format!("{a}.end"));
    assert!(corner.y.abs() < 1e-9 && corner.z.abs() < 1e-9, "{corner:?}");
    for r in [format!("{b}.start"), format!("{c}.start")] {
        assert!(close(at(&sk, &r), corner), "{r} stays on the corner");
    }
    assert!(close(at(&sk, &q), at(&sk, &format!("{b}.end"))));
    // 4 classes of points free (12) and the world line (6); the axis takes 2.
    assert_eq!(rep.dof, 3 * 3 + 6 - 2 + 3 - 3);
}

#[test]
fn freedoms_are_counted_exactly_far_from_the_origin() {
    // A point on a long oblique line, 400 m out: one slide left, as near the origin.
    let mut sk = Sketch::new();
    let o = v(4.0e5, -3.0e5, 2.5e5);
    let l = line(&mut sk, o, o + v(3.0e5, 4.0e5, 1.2e5));
    for end in ["start", "end"] {
        let p = sk.wire_point(&format!("{l}.{end}")).unwrap();
        let at = sk.wire_point_at(&p).unwrap();
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at }).unwrap();
    }
    let q = sk.add_drawn_wire_in(&[o + v(1.0e5, 1.0e5, 3.0e4)], WireSpace::Component).unwrap();
    sk.add_wire_constraint(WireConstraintKind::PointOnLine { p: sk.wire_point(&q).unwrap(), line: l.clone() }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    assert_eq!(rep.dof, 1);
}

#[test]
fn joined_points_fixed_in_two_places_conflict() {
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0));
    sk.join_drawn_ends(std::slice::from_ref(&b)).unwrap();
    let (end, start) = (sk.wire_point(&format!("{a}.end")).unwrap(), sk.wire_point(&format!("{b}.start")).unwrap());
    sk.add_wire_constraint(WireConstraintKind::Fix { p: end, at: v(10.0, 0.0, 0.0) }).unwrap();
    let k = sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(11.0, 0.0, 0.0) }).unwrap();
    let before = sk.wires.clone();
    assert!(!sk.wire_constraints_hold());
    let rep = sk.solve_wires(None);
    assert!(!rep.ok && rep.failing == vec![k], "{rep:?}");
    assert_eq!(sk.wires, before);
}

#[test]
fn a_group_too_large_to_solve_fails_saying_so_and_moves_nothing() {
    // 520 lines end to end, each along an axis: 521 joined points, 1,563 unknowns.
    let mut sk = Sketch::new();
    let mut ids = Vec::new();
    for i in 0..520 {
        let x = i as f64;
        ids.push(line(&mut sk, v(x, 0.0, 0.0), v(x + 1.0, 0.1, 0.0)));
        if i > 0 {
            let (prev, this) = (ids[i - 1].clone(), ids[i].clone());
            let (end, start) = (sk.wire_point(&format!("{prev}.end")).unwrap(), sk.wire_point(&format!("{this}.start")).unwrap());
            let w = sk.wire_index(&this).unwrap();
            sk.wires[w].fit[0] = sk.wire_point_at(&end).unwrap();
            sk.add_wire_constraint(WireConstraintKind::Coincident { a: end, b: start }).unwrap();
        }
    }
    for id in &ids {
        sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: id.clone(), axis: Axis::X }).unwrap();
    }
    let before = sk.wires.clone();
    let rep = sk.solve_wires(None);
    assert!(!rep.ok);
    assert_eq!(rep.oversized, Some(1563));
    assert!(rep.failure(|f| f.to_string()).contains("1500"), "{}", rep.failure(|f| f.to_string()));
    assert_eq!(sk.wires, before);
}

#[test]
fn constraints_on_a_rebuilt_curve_can_be_dropped() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let m = line(&mut sk, v(0.0, 5.0, 0.0), v(10.0, 5.0, 0.0));
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X }).unwrap();
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: m.clone(), axis: Axis::X }).unwrap();
    assert_eq!(sk.drop_wire_constraints_on(&l), 1);
    assert_eq!(sk.wire_constraints.len(), 1);
}

#[test]
fn a_line_cannot_be_joined_to_itself() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let (a, b) = (sk.wire_point(&format!("{l}.start")).unwrap(), sk.wire_point(&format!("{l}.end")).unwrap());
    assert!(sk.add_wire_constraint(WireConstraintKind::Coincident { a, b }).is_err());
}

#[test]
fn a_failing_group_leaves_the_others_to_solve() {
    let mut sk = Sketch::new();
    // A conflict: a line fixed at both ends held along X off the axis.
    let bad = line(&mut sk, v(0.0, 0.0, 0.0), v(0.0, 10.0, 0.0));
    for end in ["start", "end"] {
        let p = sk.wire_point(&format!("{bad}.{end}")).unwrap();
        let at = sk.wire_point_at(&p).unwrap();
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at }).unwrap();
    }
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: bad.clone(), axis: Axis::X }).unwrap();
    // An unrelated line that can go along Z.
    let good = line(&mut sk, v(20.0, 0.0, 0.0), v(21.0, 1.0, 30.0));
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: good.clone(), axis: Axis::Z }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(!rep.ok);
    let d = at(&sk, &format!("{good}.end")) - at(&sk, &format!("{good}.start"));
    assert!(d.x.abs() < 1e-9 && d.y.abs() < 1e-9, "the other line solved: {d:?}");
    assert!(close(at(&sk, &format!("{bad}.end")), v(0.0, 10.0, 0.0)), "the failing one stays");
}

#[test]
fn a_saved_constraint_cannot_take_another_entity_s_id() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    sk.wire_constraints.push(WireConstraint { id: l.clone(), kind: WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X } });
    assert!(sk.validate_wire_constraints().is_err());
    // A direction a hair off an axis is not along it.
    assert_eq!(Axis::along(v(1.0, 1e-5, 0.0)), None);
    assert_eq!(Axis::along(v(0.0, 0.0, -3.0)), Some(Axis::Z));
}

#[test]
fn a_curve_that_cannot_be_rebuilt_holds_back_only_its_own_group() {
    // An arc pulled into a line (as in the test above), and an unrelated line held along Z.
    let mut sk = Sketch::new();
    let a = sk.add_drawn_arc_in(&[v(0.0, 0.0, 0.0), v(5.0, 5.0, 0.0), v(10.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    for end in ["start", "end"] {
        let p = sk.wire_point(&format!("{l}.{end}")).unwrap();
        let at = sk.wire_point_at(&p).unwrap();
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at }).unwrap();
    }
    sk.add_wire_constraint(WireConstraintKind::PointOnLine { p: sk.wire_point(&format!("{a}.through")).unwrap(), line: l.clone() }).unwrap();
    let good = line(&mut sk, v(20.0, 0.0, 0.0), v(21.0, 1.0, 30.0));
    sk.add_wire_constraint(WireConstraintKind::AlongAxis { line: good.clone(), axis: Axis::Z }).unwrap();
    let arc_before = sk.wires[sk.wire_index(&a).unwrap()].clone();
    let rep = sk.solve_wires(None);
    assert!(!rep.ok);
    assert_eq!(sk.wires[sk.wire_index(&a).unwrap()], arc_before, "the arc keeps its shape");
    let d = at(&sk, &format!("{good}.end")) - at(&sk, &format!("{good}.start"));
    assert!(d.x.abs() < 1e-9 && d.y.abs() < 1e-9, "the other line still solved: {d:?}");
}
