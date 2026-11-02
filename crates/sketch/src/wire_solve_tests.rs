use solidcore_record::Vec3;

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
    sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(0.0, 0.0, 0.0), exprs: None }).unwrap();
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
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at, exprs: None }).unwrap();
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
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at: to, exprs: None }).unwrap();
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
    sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(0.0, 0.0, 0.0), exprs: None }).unwrap();
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
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at, exprs: None }).unwrap();
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
    sk.wire_constraints.push(WireConstraint {
        id: "k9".into(),
        kind: WireConstraintKind::Fix { p: WirePoint { wire: l.clone(), index: 7 }, at: v(0.0, 0.0, 0.0), exprs: None },
        param: None,
        driven: false,
    });
    assert!(sk.validate_wire_constraints().is_err());
    // As a design is opened, one naming a point its curve no longer has is dropped instead.
    assert_eq!(sk.prune_wire_constraints(), 1);
    sk.validate_wire_constraints().unwrap();
    sk.wire_constraints.push(WireConstraint {
        id: "k9".into(),
        kind: WireConstraintKind::Fix { p: WirePoint { wire: l.clone(), index: 0 }, at: v(f64::NAN, 0.0, 0.0), exprs: None },
        param: None,
        driven: false,
    });
    assert!(sk.validate_wire_constraints().is_err());
    sk.wire_constraints.clear();
    for _ in 0..2 {
        sk.wire_constraints.push(WireConstraint {
            id: "k9".into(),
            kind: WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X },
            param: None,
            driven: false,
        });
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
    sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(0.0, 0.0, 0.0), exprs: None }).unwrap();
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
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at, exprs: None }).unwrap();
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
    sk.add_wire_constraint(WireConstraintKind::Fix { p: end, at: v(10.0, 0.0, 0.0), exprs: None }).unwrap();
    let k = sk.add_wire_constraint(WireConstraintKind::Fix { p: start, at: v(11.0, 0.0, 0.0), exprs: None }).unwrap();
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
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at, exprs: None }).unwrap();
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
    sk.wire_constraints.push(WireConstraint {
        id: l.clone(),
        kind: WireConstraintKind::AlongAxis { line: l.clone(), axis: Axis::X },
        param: None,
        driven: false,
    });
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
        sk.add_wire_constraint(WireConstraintKind::Fix { p, at, exprs: None }).unwrap();
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

fn fix_all(sk: &mut Sketch, r: &str) {
    let p = sk.wire_point(r).unwrap();
    let at = sk.wire_point_at(&p).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Fix { p, at, exprs: None }).unwrap();
}

#[test]
fn a_length_dimension_stretches_a_line_along_itself() {
    // A 3-4-5 line from a fixed start, dimensioned to 10: least motion keeps its direction.
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(3.0, 4.0, 0.0));
    fix_all(&mut sk, &format!("{l}.start"));
    let (a, b) = (sk.wire_point(&format!("{l}.start")).unwrap(), sk.wire_point(&format!("{l}.end")).unwrap());
    sk.add_wire_dimension(WireConstraintKind::Distance { a, b, value: 10.0 }, Some("d1".into()), false).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    assert!(close(at(&sk, &format!("{l}.end")), v(6.0, 8.0, 0.0)), "{:?}", at(&sk, &format!("{l}.end")));
    // 6 coordinates, less the fix (3) and the length (1).
    assert_eq!(rep.dof, 2);
}

#[test]
fn an_axis_distance_keeps_its_side() {
    // b lies 7 before a along X: the dimension keeps it before a, at 20.
    let mut sk = Sketch::new();
    let qa = sk.add_drawn_wire_in(&[v(10.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let qb = sk.add_drawn_wire_in(&[v(3.0, 5.0, 1.0)], WireSpace::Component).unwrap();
    fix_all(&mut sk, &qa);
    let (a, b) = (sk.wire_point(&qa).unwrap(), sk.wire_point(&qb).unwrap());
    sk.add_wire_dimension(WireConstraintKind::AxisDistance { a, b, axis: Axis::X, reversed: true, value: 20.0 }, None, false).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    // Only x moves: y and z are free and least motion leaves them.
    assert!(close(at(&sk, &qb), v(-10.0, 5.0, 1.0)), "{:?}", at(&sk, &qb));
}

#[test]
fn an_angle_dimension_turns_the_free_line() {
    // Two lines from a fixed corner, the first fixed along X; the second at 90° after the solve.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(0.0, 0.0, 0.0), v(5.0, 0.0, 5.0));
    fix_all(&mut sk, &format!("{a}.start"));
    fix_all(&mut sk, &format!("{a}.end"));
    fix_all(&mut sk, &format!("{b}.start"));
    let mut k = sk.angle_kind(&a, &b).unwrap();
    k.set_value(std::f64::consts::FRAC_PI_2);
    sk.add_wire_dimension(k, None, false).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let end = at(&sk, &format!("{b}.end"));
    assert!(end.x.abs() < 1e-7 && end.z > 0.0, "the second line stands square to the first: {end:?}");
    let k = sk.wire_constraints.last().unwrap().kind.clone();
    assert!((sk.measure_wire_dimension(&k).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn a_radius_dimension_resizes_an_arc_through_fixed_ends() {
    // A half circle of radius 5 between fixed ends 10 apart: radius 8 puts the middle point on
    // the circle of radius 8 through the ends.
    let mut sk = Sketch::new();
    let arc = sk.add_drawn_arc_in(&[v(-5.0, 0.0, 0.0), v(0.0, 5.0, 0.0), v(5.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    fix_all(&mut sk, &format!("{arc}.start"));
    fix_all(&mut sk, &format!("{arc}.end"));
    sk.add_wire_dimension(WireConstraintKind::Radius { arc: arc.clone(), value: 8.0 }, None, false).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let w = &sk.wires[sk.wire_index(&arc).unwrap()];
    assert!((w.arc.unwrap().radius - 8.0).abs() < 1e-7, "{:?}", w.arc);
}

#[test]
fn a_driven_dimension_measures_without_constraining_and_a_second_one_is_refused() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(3.0, 4.0, 12.0));
    let (a, b) = (sk.wire_point(&format!("{l}.start")).unwrap(), sk.wire_point(&format!("{l}.end")).unwrap());
    let d = sk.add_wire_dimension(WireConstraintKind::Distance { a: a.clone(), b: b.clone(), value: 1.0 }, Some("ignored".into()), true).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok && rep.dof == 6, "a driven dimension takes no freedom: {rep:?}");
    let c = sk.wire_constraints.iter().find(|c| c.id == d).unwrap();
    assert!(c.driven && c.param.is_none());
    assert!((c.kind.value().unwrap() - 13.0).abs() < 1e-9, "it measures the 3-4-12 line: 13");
    let e = sk.add_wire_dimension(WireConstraintKind::Distance { a: b, b: a, value: 13.0 }, None, false).unwrap_err().to_string();
    assert!(e.contains(&d), "{e}");
    // Out-of-range values are refused.
    let arc = sk.add_drawn_arc_in(&[v(-5.0, 0.0, 0.0), v(0.0, 5.0, 0.0), v(5.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    assert!(sk.add_wire_dimension(WireConstraintKind::Radius { arc, value: -1.0 }, None, false).is_err());
    assert_eq!(sk.dimension_params().len(), 0);
}

/// An angle dimension of `value` degrees between lines `a` and `b`, as `sketch.dimension` adds it.
fn angle(sk: &mut Sketch, a: &str, b: &str, value: f64) -> String {
    let mut k = sk.angle_kind(a, b).unwrap();
    k.set_value(value.to_radians());
    sk.add_wire_dimension(k, None, false).unwrap()
}

#[test]
fn an_angle_at_a_joint_is_the_corner_and_turns_parallel_lines_apart() {
    // A bracket: two lines from one corner, drawn parallel (both along X). 60° opens the corner
    // to 60°, not 120°, though the second line runs back towards the first's start.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(10.0, 0.0, 0.0), v(0.0, 0.0, 0.0));
    let b = line(&mut sk, v(10.0, 0.0, 0.0), v(20.0, 0.0, 0.0));
    sk.join_drawn_ends(std::slice::from_ref(&b)).unwrap();
    fix_all(&mut sk, &format!("{a}.start"));
    fix_all(&mut sk, &format!("{a}.end"));
    let k = angle(&mut sk, &a, &b, 60.0);
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "the parallel start is nudged off: {rep:?}");
    let kind = sk.wire_constraints.iter().find(|c| c.id == k).unwrap().kind.clone();
    assert!((sk.measure_wire_dimension(&kind).unwrap() - 60f64.to_radians()).abs() < 1e-9);
    let (corner, tip, far) = (at(&sk, &format!("{b}.start")), at(&sk, &format!("{b}.end")), at(&sk, &format!("{a}.end")));
    let (u, w) = (far - corner, tip - corner);
    assert!((u.dot(w) / (u.len() * w.len()) - 0.5).abs() < 1e-9, "the corner's own angle is 60°");
    // The leg turns: it keeps about its length, in the plane it was nudged into.
    assert!((w.len() - 10.0).abs() < 2.0, "leg {} long", w.len());
}

#[test]
fn angles_of_0_and_180_degrees_hold_lines_parallel_either_way() {
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(0.0, 5.0, 0.0), v(8.0, 9.0, 3.0));
    let c = line(&mut sk, v(0.0, -5.0, 0.0), v(-8.0, -7.0, 1.0));
    fix_all(&mut sk, &format!("{a}.start"));
    fix_all(&mut sk, &format!("{a}.end"));
    fix_all(&mut sk, &format!("{b}.start"));
    fix_all(&mut sk, &format!("{c}.start"));
    angle(&mut sk, &a, &b, 0.0);
    angle(&mut sk, &a, &c, 180.0);
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let (db, dc) = (at(&sk, &format!("{b}.end")) - at(&sk, &format!("{b}.start")), at(&sk, &format!("{c}.end")) - at(&sk, &format!("{c}.start")));
    assert!(db.y.abs() < 1e-9 && db.z.abs() < 1e-9 && db.x > 0.0, "{db:?}");
    assert!(dc.y.abs() < 1e-9 && dc.z.abs() < 1e-9 && dc.x < 0.0, "{dc:?}");
    // Each end keeps its length along the line: one freedom each.
    assert_eq!(rep.dof, 2);
}

#[test]
fn a_driven_dimension_takes_any_number_it_measures_and_opens_again() {
    // A driven distance of 1.5 km and a driven radius of an arc squeezed nearly flat.
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(1.5e6, 0.0, 0.0));
    let arc = sk.add_drawn_arc_in(&[v(0.0, 0.0, 0.0), v(5.0, 1e-3, 0.0), v(10.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let (s, e) = (sk.wire_point(&format!("{l}.start")).unwrap(), sk.wire_point(&format!("{l}.end")).unwrap());
    let long = sk.add_wire_dimension(WireConstraintKind::Distance { a: s, b: e, value: 0.0 }, None, true).unwrap();
    let r = sk.add_wire_dimension(WireConstraintKind::Radius { arc: arc.clone(), value: 0.0 }, None, true).unwrap();
    assert!(sk.solve_wires(None).ok);
    let value = |sk: &Sketch, id: &str| sk.wire_constraints.iter().find(|c| c.id == id).and_then(|c| c.kind.value()).unwrap();
    assert!((value(&sk, &long) - 1.5e6).abs() < 1e-6);
    assert!(value(&sk, &r) > 1e3, "{}", value(&sk, &r));
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&sk).unwrap()).unwrap();
    back.validate_wire_constraints().unwrap();
    // A driving distance that long is refused.
    let (s, e) = (sk.wire_point(&format!("{l}.start")).unwrap(), sk.wire_point(&format!("{l}.end")).unwrap());
    sk.remove_wire_constraints(std::slice::from_ref(&long));
    assert!(sk.add_wire_dimension(WireConstraintKind::Distance { a: s, b: e, value: 1.5e6 }, None, false).is_err());
}

#[test]
fn an_angle_stalled_beside_a_solved_distance_is_still_turned() {
    // Two lines meeting at a fixed corner, drawn parallel and long; one also carries a length that
    // holds to rounding only (an oblique 1000 mm). The angle's stalled start must still open.
    let mut sk = Sketch::new();
    let dir = v(3.0, 4.0, 12.0) * (1.0 / 13.0);
    let a = line(&mut sk, v(0.0, 0.0, 0.0), dir * 1000.0);
    let b = line(&mut sk, v(0.0, 0.0, 0.0), dir * 700.0);
    sk.join_drawn_ends(std::slice::from_ref(&b)).unwrap();
    fix_all(&mut sk, &format!("{a}.start"));
    fix_all(&mut sk, &format!("{a}.end"));
    let (s, e) = (sk.wire_point(&format!("{b}.start")).unwrap(), sk.wire_point(&format!("{b}.end")).unwrap());
    sk.add_wire_dimension(WireConstraintKind::Distance { a: s, b: e, value: 700.0 }, None, false).unwrap();
    angle(&mut sk, &a, &b, 40.0);
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let w = at(&sk, &format!("{b}.end"));
    assert!((w.len() - 700.0).abs() < 1e-6 && (w.dot(dir) / w.len() - 40f64.to_radians().cos()).abs() < 1e-9, "{w:?}");
}

#[test]
fn an_angle_beside_a_length_solves_at_building_scale() {
    // Lines 1 to 20 m long, a hair off parallel, the free one also holding its length.
    for l in [1000.0f64, 5000.0, 20000.0] {
        let mut sk = Sketch::new();
        let a = line(&mut sk, v(0.0, 0.0, 0.0), v(l, 0.0, 0.0));
        let b = line(&mut sk, v(0.0, l / 2.0, 0.0), v(l, l / 2.0, 1e-10 * l));
        fix_all(&mut sk, &format!("{a}.start"));
        fix_all(&mut sk, &format!("{a}.end"));
        fix_all(&mut sk, &format!("{b}.start"));
        let (s, e) = (sk.wire_point(&format!("{b}.start")).unwrap(), sk.wire_point(&format!("{b}.end")).unwrap());
        sk.add_wire_dimension(WireConstraintKind::Distance { a: s, b: e, value: l }, None, false).unwrap();
        let k = angle(&mut sk, &a, &b, 50.0);
        let rep = sk.solve_wires(None);
        assert!(rep.ok, "{l}: {rep:?}");
        let kind = sk.wire_constraints.iter().find(|c| c.id == k).unwrap().kind.clone();
        assert!((sk.measure_wire_dimension(&kind).unwrap() - 50f64.to_radians()).abs() < 1e-9, "{l}");
    }
}

#[test]
fn parallel_perpendicular_and_equal_hold_between_free_lines() {
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(0.0, 5.0, 0.0), v(8.0, 6.0, 2.0));
    let c = line(&mut sk, v(0.0, 0.0, 0.0), v(1.0, 9.0, 3.0));
    for end in ["start", "end"] {
        fix_all(&mut sk, &format!("{a}.{end}"));
    }
    fix_all(&mut sk, &format!("{b}.start"));
    fix_all(&mut sk, &format!("{c}.start"));
    sk.add_wire_constraint(WireConstraintKind::Parallel { a: a.clone(), b: b.clone() }).unwrap();
    sk.add_wire_constraint(sk.equal_kind(&a, &b).unwrap()).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Perpendicular { a: a.clone(), b: c.clone() }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    // b runs along X from its fixed start, as long as a: it ends at (10, 5, 0).
    assert!(close(at(&sk, &format!("{b}.end")), v(10.0, 5.0, 0.0)), "{:?}", at(&sk, &format!("{b}.end")));
    assert!(at(&sk, &format!("{c}.end")).x.abs() < 1e-7, "c stands square to a");
}

#[test]
fn collinear_puts_the_second_line_on_the_first() {
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(20.0, 3.0, 4.0), v(30.0, 3.0, 4.0));
    for end in ["start", "end"] {
        fix_all(&mut sk, &format!("{a}.{end}"));
    }
    sk.add_wire_constraint(WireConstraintKind::Collinear { a: a.clone(), b: b.clone() }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    for end in ["start", "end"] {
        let q = at(&sk, &format!("{b}.{end}"));
        assert!(q.y.abs() < 1e-7 && q.z.abs() < 1e-7, "{q:?}");
    }
}

#[test]
fn a_line_turns_tangent_to_the_arc_it_leaves() {
    // A quarter arc from (10,0,0) round to (0,10,0) about the origin, fixed; a line leaving its
    // end. Tangent there means running along -X, square to the radius (0,10,0).
    let mut sk = Sketch::new();
    let d = 10.0 * std::f64::consts::FRAC_1_SQRT_2;
    let arc = sk.add_drawn_arc_in(&[v(10.0, 0.0, 0.0), v(d, d, 0.0), v(0.0, 10.0, 0.0)], WireSpace::Component).unwrap();
    for k in 0..3 {
        fix_all(&mut sk, &format!("{arc}.{k}"));
    }
    let l = line(&mut sk, v(0.0, 10.0, 0.0), v(-8.0, 13.0, 1.0));
    let k = sk.tangent_kind(&l, &arc).expect("they meet");
    sk.add_wire_constraint(k).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let end = at(&sk, &format!("{l}.end"));
    assert!((end.y - 10.0).abs() < 1e-7 && end.z.abs() < 1e-7 && end.x < 0.0, "{end:?}");
    assert!(close(at(&sk, &format!("{l}.start")), v(0.0, 10.0, 0.0)));
    // A line that does not touch the arc has no tangent to take.
    let far = line(&mut sk, v(50.0, 50.0, 0.0), v(60.0, 50.0, 0.0));
    assert!(sk.tangent_kind(&far, &arc).is_none());
}

#[test]
fn plane_relations_and_dimensions_use_the_origin_planes() {
    let mut sk = Sketch::new();
    let l = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 1.0, 7.0));
    fix_all(&mut sk, &format!("{l}.start"));
    // Parallel to XY: the end comes down to z = 0.
    sk.add_wire_constraint(WireConstraintKind::ParallelToPlane { line: l.clone(), plane: OriginPlane::Xy }).unwrap();
    let q = sk.add_drawn_wire_in(&[v(3.0, 4.0, 5.0)], WireSpace::Component).unwrap();
    sk.add_wire_constraint(WireConstraintKind::OnPlane { p: sk.wire_point(&q).unwrap(), plane: OriginPlane::Yz }).unwrap();
    let r = sk.add_drawn_wire_in(&[v(1.0, -2.0, 3.0)], WireSpace::Component).unwrap();
    let dist = WireConstraintKind::PlaneDistance { p: sk.wire_point(&r).unwrap(), plane: OriginPlane::Xz, reversed: true, value: 6.0 };
    sk.add_wire_dimension(dist, None, false).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    assert!(at(&sk, &format!("{l}.end")).z.abs() < 1e-7);
    assert!(close(at(&sk, &q), v(0.0, 4.0, 5.0)), "onto YZ: {:?}", at(&sk, &q));
    assert!(close(at(&sk, &r), v(1.0, -6.0, 3.0)), "6 behind XZ: {:?}", at(&sk, &r));
    // A line at 30° to XY, measured.
    let m = line(&mut sk, v(0.0, 0.0, 0.0), v(3.0f64.sqrt(), 0.0, 1.0));
    let k = WireConstraintKind::PlaneAngle { line: m, plane: OriginPlane::Xy, reversed: false, value: 0.0 };
    assert!((sk.measure_wire_dimension(&k).unwrap() - std::f64::consts::FRAC_PI_6).abs() < 1e-12);
    assert_eq!(OriginPlane::from_name("ZX"), Some(OriginPlane::Xz));
}

#[test]
fn angles_to_a_plane_drive_from_lying_in_it_to_standing_square() {
    // Three lines from fixed starts: one lying in XY driven to 80°, one at 30° driven to 0°, one
    // upright held at 90°.
    let mut sk = Sketch::new();
    let flat = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let slope = line(&mut sk, v(0.0, 5.0, 0.0), v(3.0f64.sqrt() * 5.0, 5.0, 5.0));
    let up = line(&mut sk, v(0.0, 9.0, 0.0), v(0.1, 9.0, 10.0));
    for l in [&flat, &slope, &up] {
        fix_all(&mut sk, &format!("{l}.start"));
    }
    for (l, deg) in [(&flat, 80.0), (&slope, 0.0), (&up, 90.0)] {
        let k = WireConstraintKind::PlaneAngle { line: l.clone(), plane: OriginPlane::Xy, reversed: false, value: f64::to_radians(deg) };
        sk.add_wire_dimension(k, None, false).unwrap();
    }
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "the flat start is nudged off the plane: {rep:?}");
    let dir = |l: &str| {
        let d = at(&sk, &format!("{l}.end")) - at(&sk, &format!("{l}.start"));
        d * (1.0 / d.len())
    };
    assert!((dir(&flat).z - 80f64.to_radians().sin()).abs() < 1e-9, "{:?}", dir(&flat));
    assert!(dir(&slope).z.abs() < 1e-9, "{:?}", dir(&slope));
    assert!((dir(&up).z - 1.0).abs() < 1e-9, "{:?}", dir(&up));
}

#[test]
fn perpendicular_turns_lines_that_start_parallel() {
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0));
    let b = line(&mut sk, v(0.0, 5.0, 0.0), v(10.0, 5.0, 0.0));
    fix_all(&mut sk, &format!("{a}.start"));
    fix_all(&mut sk, &format!("{a}.end"));
    fix_all(&mut sk, &format!("{b}.start"));
    sk.add_wire_constraint(WireConstraintKind::Perpendicular { a: a.clone(), b: b.clone() }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let d = at(&sk, &format!("{b}.end")) - at(&sk, &format!("{b}.start"));
    assert!(d.x.abs() < 1e-9 && d.len() > 1.0, "{d:?}");
}

#[test]
fn a_tangent_line_leaves_the_arc_the_way_it_runs_and_never_folds_back() {
    // A quarter arc from (10, 0) to (0, 10) about the origin, in XY. At its end it runs on in −X.
    let mut sk = Sketch::new();
    let h = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
    let arc = sk.add_drawn_arc_in(&[v(10.0, 0.0, 0.0), v(h, h, 0.0), v(0.0, 10.0, 0.0)], WireSpace::Component).unwrap();
    // A line from the arc's end drawn back over the arc (a cusp if it were only parallel).
    let l = line(&mut sk, v(0.0, 10.0, 0.0), v(8.0, 10.5, 0.0));
    sk.join_drawn_ends(std::slice::from_ref(&l)).unwrap();
    for i in 0..3 {
        fix_all(&mut sk, &format!("{arc}.{i}"));
    }
    let k = sk.tangent_kind(&l, &arc).unwrap();
    sk.add_wire_constraint(k).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let d = at(&sk, &format!("{l}.end")) - at(&sk, &format!("{l}.start"));
    assert!(d.x < 0.0 && d.y.abs() < 1e-9 && d.z.abs() < 1e-9, "runs on in −X: {d:?}");
}

#[test]
fn equal_arcs_hold_one_radius_survive_a_save_and_go_with_an_arc() {
    let mut sk = Sketch::new();
    let a = sk.add_drawn_arc_in(&[v(0.0, 0.0, 0.0), v(5.0, 5.0, 0.0), v(10.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let b = sk.add_drawn_arc_in(&[v(0.0, 20.0, 0.0), v(3.0, 23.0, 0.0), v(6.0, 20.0, 0.0)], WireSpace::Component).unwrap();
    for i in 0..3 {
        fix_all(&mut sk, &format!("{a}.{i}"));
    }
    fix_all(&mut sk, &format!("{b}.start"));
    fix_all(&mut sk, &format!("{b}.end"));
    let k = sk.equal_kind(&a, &b).unwrap();
    assert!(matches!(k, WireConstraintKind::Equal { arcs: true, .. }));
    let id = sk.add_wire_constraint(k).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let radius = |sk: &Sketch, arc: &str| sk.measure_wire_dimension(&WireConstraintKind::Radius { arc: arc.to_string(), value: 0.0 }).unwrap();
    assert!((radius(&sk, &a) - radius(&sk, &b)).abs() < 1e-7 && (radius(&sk, &a) - 5.0).abs() < 1e-9);
    // Saved, the ids stay plain wire ids.
    let text = serde_json::to_string(&sk.wire_constraints).unwrap();
    assert!(text.contains("\"arcs\":true") && !text.contains("arc:"), "{text}");
    let back: Sketch = serde_json::from_str(&serde_json::to_string(&sk).unwrap()).unwrap();
    assert_eq!(back.wire_constraints, sk.wire_constraints);
    back.validate_wire_constraints().unwrap();
    sk.remove_wires(std::slice::from_ref(&b));
    assert!(sk.wire_constraints.iter().all(|c| c.id != id));
}

#[test]
fn no_constraint_reads_more_points_than_its_jacobian_holds() {
    let p = |w: &str| WirePoint { wire: w.into(), index: 0 };
    let kinds = [
        WireConstraintKind::Equal { a: "a".into(), b: "b".into(), arcs: true },
        WireConstraintKind::Tangent { line: "l".into(), arc: "a".into(), line_end: 0, arc_end: 0 },
        WireConstraintKind::Collinear { a: "a".into(), b: "b".into() },
        WireConstraintKind::Angle { a: "a".into(), b: "b".into(), a_reversed: false, b_reversed: false, value: 1.0 },
        WireConstraintKind::PointOnLine { p: p("q"), line: "l".into() },
        WireConstraintKind::PlaneDistance { p: p("q"), plane: OriginPlane::Xy, reversed: false, value: 1.0 },
    ];
    for k in kinds {
        assert!(3 * k.points().len() <= dual::N, "{k:?}");
    }
}

#[test]
fn perpendicular_turns_lines_a_hair_off_parallel_beside_a_length() {
    // 1e-10 rad off parallel: too little for a step to register, too much for a zero gradient.
    let mut sk = Sketch::new();
    let a = line(&mut sk, v(0.0, 0.0, 0.0), v(5000.0, 0.0, 0.0));
    let b = line(&mut sk, v(0.0, 2500.0, 0.0), v(5000.0, 2500.0, 5e-7));
    fix_all(&mut sk, &format!("{a}.start"));
    fix_all(&mut sk, &format!("{a}.end"));
    fix_all(&mut sk, &format!("{b}.start"));
    let (s, e) = (sk.wire_point(&format!("{b}.start")).unwrap(), sk.wire_point(&format!("{b}.end")).unwrap());
    sk.add_wire_dimension(WireConstraintKind::Distance { a: s, b: e, value: 5000.0 }, None, false).unwrap();
    sk.add_wire_constraint(WireConstraintKind::Perpendicular { a: a.clone(), b: b.clone() }).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let d = at(&sk, &format!("{b}.end")) - at(&sk, &format!("{b}.start"));
    assert!(d.x.abs() < 1e-6 && (d.len() - 5000.0).abs() < 1e-6, "{d:?}");
}

#[test]
fn equal_arcs_from_a_semicircle_follow_a_radius() {
    // b starts as a semicircle (its least radius for that chord): its through point must move.
    let mut sk = Sketch::new();
    let a = sk.add_drawn_arc_in(&[v(0.0, 0.0, 0.0), v(3.0, 3.0, 0.0), v(6.0, 0.0, 0.0)], WireSpace::Component).unwrap();
    let b = sk.add_drawn_arc_in(&[v(0.0, 20.0, 0.0), v(3.0, 23.0, 0.0), v(6.0, 20.0, 0.0)], WireSpace::Component).unwrap();
    for arc in [&a, &b] {
        fix_all(&mut sk, &format!("{arc}.start"));
        fix_all(&mut sk, &format!("{arc}.end"));
    }
    sk.add_wire_dimension(WireConstraintKind::Radius { arc: a.clone(), value: 5.0 }, None, false).unwrap();
    sk.add_wire_constraint(sk.equal_kind(&a, &b).unwrap()).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let radius = |arc: &str| sk.measure_wire_dimension(&WireConstraintKind::Radius { arc: arc.to_string(), value: 0.0 }).unwrap();
    assert!((radius(&a) - 5.0).abs() < 1e-7 && (radius(&b) - 5.0).abs() < 1e-7, "{} {}", radius(&a), radius(&b));
}

#[test]
fn a_tangent_at_the_arc_s_start_from_the_line_s_end_runs_back_before_it() {
    // The quarter arc from (10, 0) runs on past its start in −Y; a line ending there leaves it so.
    let mut sk = Sketch::new();
    let h = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
    let arc = sk.add_drawn_arc_in(&[v(10.0, 0.0, 0.0), v(h, h, 0.0), v(0.0, 10.0, 0.0)], WireSpace::Component).unwrap();
    let l = line(&mut sk, v(11.0, -8.0, 0.5), v(10.0, 0.0, 0.0));
    sk.join_drawn_ends(std::slice::from_ref(&l)).unwrap();
    for i in 0..3 {
        fix_all(&mut sk, &format!("{arc}.{i}"));
    }
    let k = sk.tangent_kind(&l, &arc).unwrap();
    assert!(matches!(k, WireConstraintKind::Tangent { line_end: 1, arc_end: 0, .. }), "{k:?}");
    sk.add_wire_constraint(k).unwrap();
    let rep = sk.solve_wires(None);
    assert!(rep.ok, "{rep:?}");
    let d = at(&sk, &format!("{l}.start")) - at(&sk, &format!("{l}.end"));
    assert!(d.y < 0.0 && d.x.abs() < 1e-9 && d.z.abs() < 1e-9, "{d:?}");
}
