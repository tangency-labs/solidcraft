//! The design's first part is grounded; dragging a free part moves it (#2804).
use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, p: Value) -> Value {
    s.execute(command, &p).unwrap_or_else(|e| panic!("{command} {p}: {e}"))
}

/// A new top-level part holding a 10 mm cube at `corner` (activates the design first, so the
/// part goes into it): (component, occurrence).
fn part(s: &mut Session, name: &str, corner: [f64; 3]) -> (u64, u64) {
    run(s, "component.activate", json!({"component": "root"}));
    let c = run(s, "component.create", json!({"name": name}));
    run(s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner}));
    run(s, "component.activate", json!({"component": "root"}));
    (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap())
}

fn grounded(s: &Session, occurrence: u64) -> bool {
    s.doc.occurrences.iter().find(|o| o.id == occurrence).unwrap().grounded
}

/// Where the placement `path` puts its origin, in world coordinates (pending moves included).
fn origin(s: &Session, path: &[u64]) -> [f64; 3] {
    let w = s.occurrence_placement(path).unwrap().world;
    [w[3][0], w[3][1], w[3][2]]
}

fn near(a: [f64; 3], b: [f64; 3]) {
    assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9), "{a:?} != {b:?}");
}

#[test]
fn the_first_top_level_part_is_grounded_and_later_parts_and_copies_are_free() {
    let mut s = Session::default();
    let (a, first) = part(&mut s, "A", [0.0, 0.0, 0.0]);
    let created = s.doc.occurrences.iter().find(|o| o.id == first).cloned().unwrap();
    assert!(created.grounded, "the first part is grounded");
    let (_, second) = part(&mut s, "B", [20.0, 0.0, 0.0]);
    assert!(!grounded(&s, second), "a later part starts free");
    let instance = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [0, 20, 0]}))["occurrence"].as_u64().unwrap();
    assert!(!grounded(&s, instance), "Paste (Instance) of the grounded part is free");
    let copy = run(&mut s, "component.paste_new", json!({"component": a, "translate": [0, 40, 0]}));
    let copy = s.doc.occurrence_of(copy["component"].as_u64().unwrap()).unwrap().id;
    assert!(!grounded(&s, copy), "Paste New of the grounded part is free");
    // A part made inside another one is placed in it, not in the design: free.
    run(&mut s, "component.activate", json!({"component": a}));
    let nested = run(&mut s, "component.create", json!({"name": "Inner"}))["occurrence"].as_u64().unwrap();
    assert!(!grounded(&s, nested));
    // Several parts may be grounded, or none.
    run(&mut s, "occurrence.ground", json!({"occurrence": second, "grounded": true}));
    assert!(grounded(&s, first) && grounded(&s, second));
    for o in [first, second] {
        run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": false}));
    }
    assert!(!s.doc.occurrences.iter().any(|o| o.grounded));
}

#[test]
fn components_from_bodies_ground_only_the_first_part() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "One"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Two", "operation": "new"}));
    let made = run(&mut s, "component.from_bodies", json!({"bodies": ["One", "Two"]}));
    let occurrence = |i: usize| s.doc.occurrence_of(made["components"][i]["component"].as_u64().unwrap()).unwrap().id;
    let (one, two) = (occurrence(0), occurrence(1));
    assert!(grounded(&s, one) && !grounded(&s, two));
}

#[test]
fn ungrounded_designs_stay_ungrounded_through_new_parts_and_reopening() {
    let mut s = Session::default();
    let (_, first) = part(&mut s, "A", [0.0, 0.0, 0.0]);
    run(&mut s, "occurrence.ground", json!({"occurrence": first, "grounded": false}));
    let (_, second) = part(&mut s, "B", [20.0, 0.0, 0.0]);
    assert!(!grounded(&s, first) && !grounded(&s, second), "a part added later grounds nothing");
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert!(!back.occurrences.iter().any(|o| o.grounded), "opening a design grounds nothing");
    // With every part gone, the next one is the design's first part again.
    for c in ["A", "B"] {
        run(&mut s, "component.delete", json!({"component": c}));
    }
    let (_, again) = part(&mut s, "C", [0.0, 0.0, 0.0]);
    assert!(grounded(&s, again));
}

#[test]
fn dragging_moves_a_free_part_by_the_grabbed_point_in_one_undo_step() {
    let mut s = Session::default();
    let (_, base) = part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let (_, free) = part(&mut s, "Free", [20.0, 0.0, 0.0]);
    let undo = s.undo.len();
    let r = run(&mut s, "occurrence.drag", json!({"occurrence": free, "at": [25, 5, 10], "point": [35, 10, 10]}));
    assert_eq!(r["moved"], json!([free]));
    near(origin(&s, &[free]), [10.0, 5.0, 0.0]);
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.undo.last().map(|u| u.label.as_str()), Some("Drag Occurrence"));
    // Saved and opened again, it is where it was dragged.
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let t = back.occurrences.iter().find(|o| o.id == free).unwrap().transform;
    near([t[3][0], t[3][1], t[3][2]], [10.0, 5.0, 0.0]);
    run(&mut s, "edit.undo", json!({}));
    near(origin(&s, &[free]), [0.0, 0.0, 0.0]);
    // A grounded part stays where it is.
    let e = s.execute("occurrence.drag", &json!({"occurrence": base, "at": [0, 0, 0], "point": [5, 0, 0]})).unwrap_err().to_string();
    assert!(e.contains("grounded"), "{e}");
    near(origin(&s, &[base]), [0.0, 0.0, 0.0]);
}

#[test]
fn a_drag_captures_its_part_and_leaves_other_pending_moves_pending() {
    let mut s = Session::default();
    part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let (_, a) = part(&mut s, "A", [20.0, 0.0, 0.0]);
    let (_, b) = part(&mut s, "B", [40.0, 0.0, 0.0]);
    run(&mut s, "occurrence.move", json!({"occurrence": a, "translate": [0, 7, 0], "capture": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 9, 0], "capture": false}));
    run(&mut s, "occurrence.drag", json!({"occurrence": a, "at": [0, 0, 0], "point": [3, 0, 0]}));
    assert!(!s.pending_moves.contains_key(&a), "the dragged part's position is captured");
    let t = s.doc.occurrences.iter().find(|o| o.id == a).unwrap().transform;
    near([t[3][0], t[3][1], t[3][2]], [3.0, 7.0, 0.0]);
    assert!(s.pending_moves.contains_key(&b), "another part's move stays pending");
    near(origin(&s, &[b]), [0.0, 9.0, 0.0]);
    run(&mut s, "edit.undo", json!({}));
    near(origin(&s, &[a]), [0.0, 7.0, 0.0]);
    assert!(s.pending_moves.contains_key(&a), "undo gives the pending move back");
}

#[test]
fn dragging_uses_the_exact_path_of_a_repeated_placement() {
    let mut s = Session::default();
    part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let (asm, first) = part(&mut s, "Asm", [0.0, 0.0, 0.0]);
    run(&mut s, "occurrence.move", json!({"occurrence": first, "axis": [0, 0, 1], "angle": "90 deg"}));
    run(&mut s, "component.activate", json!({"component": asm}));
    let leaf = run(&mut s, "component.create", json!({"name": "Leaf"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    // The second instance of the assembly moves alone.
    run(&mut s, "occurrence.drag", json!({"path": [second], "at": [100, 0, 0], "point": [100, 30, 0]}));
    near(origin(&s, &[second]), [100.0, 30.0, 0.0]);
    near(origin(&s, &[first]), [0.0, 0.0, 0.0]);
    // Its leaf, a definition shared by both instances, moves in world X through that path's
    // rotated frame, and so in both.
    let before = origin(&s, &[first, leaf]);
    run(&mut s, "occurrence.drag", json!({"path": [second, leaf], "at": [100, 30, 0], "point": [110, 30, 0]}));
    near(origin(&s, &[second, leaf]), [110.0, 30.0, 0.0]);
    near(origin(&s, &[first, leaf]), [before[0] + 10.0, before[1], before[2]]);
    // The leaf alone, without the path of the instance it is in, is ambiguous.
    let e = s.execute("occurrence.drag", &json!({"occurrence": leaf, "at": [0, 0, 0], "point": [1, 0, 0]})).unwrap_err().to_string();
    assert!(e.contains("path"), "{e}");
}

#[test]
fn joints_hold_a_part_to_something_grounded_and_carry_a_free_chain_along() {
    let mut s = Session::default();
    let (_, base) = part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let (_, arm) = part(&mut s, "Arm", [0.0, 0.0, 10.0]);
    let at = |o: u64, z: f64| json!({"occurrence": o, "point": [0, 0, z], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "a": at(base, 10.0), "b": at(arm, 10.0), "flip": true, "name": "Hinge"}));
    let e = s.execute("occurrence.drag", &json!({"occurrence": arm, "at": [0, 0, 0], "point": [5, 0, 0]})).unwrap_err().to_string();
    assert!(e.contains("grounded") && e.contains("Hinge"), "{e}");
    // Two parts jointed to each other and to nothing grounded move together.
    let (_, x) = part(&mut s, "X", [50.0, 0.0, 0.0]);
    let (_, y) = part(&mut s, "Y", [50.0, 0.0, 10.0]);
    run(&mut s, "joint.create", json!({"type": "rigid", "a": at(x, 10.0), "b": at(y, 10.0), "flip": true, "name": "Weld"}));
    let (x0, y0) = (origin(&s, &[x]), origin(&s, &[y]));
    let r = run(&mut s, "occurrence.drag", json!({"occurrence": y, "at": [0, 0, 0], "point": [0, 25, 0]}));
    assert_eq!(r["moved"].as_array().map(Vec::len), Some(2), "{r}");
    near(origin(&s, &[x]), [x0[0], x0[1] + 25.0, x0[2]]);
    near(origin(&s, &[y]), [y0[0], y0[1] + 25.0, y0[2]]);
    // Suppressed, the joint holds nothing.
    run(&mut s, "joint.edit", json!({"joint": "Hinge", "suppressed": true}));
    run(&mut s, "occurrence.drag", json!({"occurrence": arm, "at": [0, 0, 0], "point": [5, 0, 0]}));
}

#[test]
fn a_drag_and_a_world_move_leave_the_same_design() {
    // Whatever follows a moved part (projections, joints, Properties) follows a dragged one.
    let build = || {
        let mut s = Session::default();
        part(&mut s, "Base", [0.0, 0.0, 0.0]);
        let (_, free) = part(&mut s, "Free", [20.0, 0.0, 0.0]);
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.project", json!({"refs": [{"edge": [25, 0, 0]}]}));
        run(&mut s, "sketch.finish", json!({}));
        (s, free)
    };
    let (mut dragged, free) = build();
    let (mut moved, _) = build();
    run(&mut dragged, "occurrence.drag", json!({"occurrence": free, "at": [25, 0, 0], "point": [25, 15, 0]}));
    run(&mut moved, "occurrence.move", json!({"occurrence": free, "space": "world", "translate": [0, 15, 0]}));
    assert_eq!(dragged.doc.to_json(), moved.doc.to_json());
    let shown = |s: &Session| format!("{:?}", s.world_state().sketches.iter().map(|k| &k.sketch.points).collect::<Vec<_>>());
    assert_eq!(shown(&dragged), shown(&moved));
}

#[test]
fn bad_drag_parameters_are_refused() {
    let mut s = Session::default();
    part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let (_, free) = part(&mut s, "Free", [20.0, 0.0, 0.0]);
    for p in [
        json!({}),
        json!({"occurrence": free}),
        json!({"occurrence": free, "at": [0, 0, 0]}),
        json!({"occurrence": free, "at": [0, 0, 0], "point": [1e300, 0, 0]}),
        json!({"occurrence": free, "at": [0, 0, "x"], "point": [0, 0, 0]}),
        json!({"path": [], "at": [0, 0, 0], "point": [1, 0, 0]}),
        json!({"path": [free, free], "at": [0, 0, 0], "point": [1, 0, 0]}),
        json!({"path": [999_999], "at": [0, 0, 0], "point": [1, 0, 0]}),
        json!({"occurrence": "nothing", "at": [0, 0, 0], "point": [1, 0, 0]}),
    ] {
        assert!(s.execute("occurrence.drag", &p).is_err(), "{p}");
    }
    near(origin(&s, &[free]), [0.0, 0.0, 0.0]);
}

#[test]
fn moving_a_grounded_part_needs_saying_so_and_it_stays_grounded() {
    let mut s = Session::default();
    let (_, base) = part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let e = s.execute("occurrence.move", &json!({"occurrence": base, "translate": [5, 0, 0]})).unwrap_err().to_string();
    assert!(e.contains("is grounded") && e.contains("occurrence.ground"), "{e}");
    run(&mut s, "occurrence.move", json!({"occurrence": base, "translate": [5, 0, 0], "move_grounded": true}));
    near(origin(&s, &[base]), [5.0, 0.0, 0.0]);
    assert!(grounded(&s, base));
}

/// The solver uses definitions' first paths; a drag through a rotated instance must carry
/// that local change to its jointed peers, including peers that are themselves shared.
#[test]
fn shared_joint_chains_follow_the_selected_frame_and_survive_solving() {
    let mut s = Session::default();
    part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let asm = run(&mut s, "component.create", json!({"name": "Assembly"}));
    let first = asm["occurrence"].as_u64().unwrap();
    let a = run(&mut s, "component.create", json!({"name": "A"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4}));
    run(&mut s, "component.activate", json!({"component": asm["component"]}));
    let b = run(&mut s, "component.create", json!({"name": "B"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [10, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let at = |o| json!({"occurrence": o, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "a": at(a), "b": at(b), "flip": true}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": second, "angle": std::f64::consts::FRAC_PI_2, "origin": [100, 0, 0]}));
    run(&mut s, "occurrence.ground", json!({"occurrence": first, "grounded": true}));
    let undo = s.undo.len();
    let before = s.doc.to_json();
    let values = s.doc.assembly.joints[0].values.clone();
    // Drag the non-root side, through the second assembly's rotated frame.
    run(&mut s, "occurrence.drag", json!({"path": [second, b], "at": [100, 0, 0], "point": [110, 0, 0]}));
    near(origin(&s, &[second, a]), [110.0, 0.0, 0.0]);
    near(origin(&s, &[second, b]), [110.0, 0.0, 0.0]);
    near(origin(&s, &[first, a]), [0.0, -10.0, 0.0]);
    near(origin(&s, &[first, b]), [0.0, -10.0, 0.0]);
    let dragged = s.doc.to_json();
    let solution = run(&mut s, "joint.solve", json!({}));
    assert_eq!(solution["conflicts"], json!([]));
    assert_eq!(s.doc.to_json(), dragged, "solving preserves the dragged pose");
    assert_eq!(s.doc.assembly.joints[0].values, values);
    assert_eq!(s.undo.len(), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_unique_part_can_drag_a_chain_through_a_shared_peer() {
    let mut s = Session::default();
    part(&mut s, "Base", [0.0, 0.0, 0.0]);
    let (_, free) = part(&mut s, "Free", [20.0, 0.0, 0.0]);
    let asm = run(&mut s, "component.create", json!({"name": "Assembly"}));
    let first = asm["occurrence"].as_u64().unwrap();
    let peer = run(&mut s, "component.create", json!({"name": "Peer"}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first, "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    let at = |o| json!({"occurrence": o, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "rigid", "a": at(free), "b": at(peer), "flip": true}));
    run(&mut s, "occurrence.drag", json!({"occurrence": free, "at": [0, 0, 0], "point": [0, 15, 0]}));
    near(origin(&s, &[free]), [0.0, 15.0, 0.0]);
    near(origin(&s, &[first, peer]), [0.0, 15.0, 0.0]);
    near(origin(&s, &[second, peer]), [100.0, 15.0, 0.0]);
    let before = s.doc.to_json();
    run(&mut s, "joint.solve", json!({}));
    assert_eq!(s.doc.to_json(), before);
}
