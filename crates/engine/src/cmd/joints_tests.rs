use std::f64::consts::FRAC_PI_2;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

pub(super) fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

pub(super) fn occ(s: &Session, component: &str) -> u64 {
    let c = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0)
}

pub(super) fn place(s: &Session, component: &str, p: Vec3) -> Vec3 {
    solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, occ(s, component)), p)
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn deleted_joint_ids_are_not_reused_after_reopening() {
    for command in ["joint.create", "joint.as_built"] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Part"}));
        let params = if command == "joint.create" {
            json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Part", "point": [0, 0, 0]}})
        } else {
            json!({"type": "revolute", "a": 0, "b": "Part"})
        };
        let first = run(&mut s, command, params.clone())["joint"].as_u64().unwrap();
        run(&mut s, "joint.delete", json!({"joint": first}));
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        *s.doc_mut() = doc;
        let second = run(&mut s, command, params)["joint"].as_u64().unwrap();
        assert!(second > first, "{command}: deleted joint {first} was reused as {second}");
        assert!(s.execute("joint.drive", &json!({"joint": first, "value": 45})).is_err());
    }
}

/// A hinge leaf (oracle 63's knuckle geometry): a 40 x 30 x 3 plate and an r4 knuckle along X
/// at y = 33, z = 4.
fn leaf(s: &mut Session, name: &str) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
    run(s, "solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
}

#[test]
fn circle_given_by_its_centre_takes_that_round_end() {
    // #393: `circle` at the centre of a round end, as the parameters document, was refused with
    // "no cylindrical face there" (no face lies at a centre).
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 6, "corner": [-15, -10, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Knob"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 6], "axis": [0, 0, 1], "radius": 10, "height": 10, "operation": "new"}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "cylindrical", "a": {"occurrence": "Plate", "face": [0, 0, 6]}, "b": {"occurrence": "Knob", "circle": [0, 0, 6]}, "name": "J"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // The knob's bottom circle already sits on the plate's top: its axis stays put (it may turn
    // about it, to line up the two frames' x).
    for z in [6.0, 16.0] {
        assert!(near(place(&s, "Knob", Vec3::new(0.0, 0.0, z)), Vec3::new(0.0, 0.0, z)), "{:?}", place(&s, "Knob", Vec3::new(0.0, 0.0, z)));
    }
    // The top end's centre: its axis points up, out of the knob. It is kept by the knob's wall
    // and the face its end opens onto, so it follows them.
    run(&mut s, "joint.origin", json!({"occurrence": "Knob", "circle": [0, 0, 16], "name": "Top"}));
    let top = s.doc.assembly.origins.iter().find(|o| o.name == "Top").map(|o| o.origin.clone()).unwrap();
    assert!(matches!(&top.snap, solidcraft_doc::joints::Snap::CircleCenter { face: Some(_), end: Some(_), .. }), "{top:?}");
    let m = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &top).unwrap();
    assert!(near(Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(0.0, 0.0, 16.0)) && near(Vec3::new(m[2][0], m[2][1], m[2][2]), Vec3::Z), "{m:?}");
    // Off the axis is still refused; a coupling size given by a centre is that round face's
    // radius (it follows the face).
    assert!(s.execute("joint.origin", &json!({"occurrence": "Knob", "circle": [3, 0, 6]})).is_err());
    run(&mut s, "joint.screw", json!({"joint": "J", "pitch": {"occurrence": "Knob", "circle": [0, 0, 6]}}));
}

#[test]
fn revolute_hinge_turns_about_the_knuckle() {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    let r = run(
        &mut s,
        "joint.create",
        json!({
        "type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // Same frames: B stays put at angle 0.
    assert!(near(place(&s, "LeafB", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 0.0)));
    let l = run(&mut s, "joint.list", json!({}));
    let dof = l["dof"].as_array().unwrap().iter().find(|d| d["occurrence"] == occ(&s, "LeafB")).unwrap()["dof"].clone();
    assert_eq!(dof, 1, "{l}");
    // Drive 90°: B turns about the knuckle axis (through (0, 33, 4), along the frame's z = −X).
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let want = solidcraft_doc::rigid(Vec3::ZERO, Vec3::new(0.0, 33.0, 4.0), Vec3::new(-1.0, 0.0, 0.0), FRAC_PI_2);
    let p = Vec3::new(20.0, 0.0, 0.0);
    assert!(near(place(&s, "LeafB", p), solidcraft_doc::apply_point(&want, p)), "{:?}", place(&s, "LeafB", p));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));
    // LeafA did not move; the knuckle axis is fixed.
    assert!(near(place(&s, "LeafA", p), p));
    assert!(near(place(&s, "LeafB", Vec3::new(10.0, 33.0, 4.0)), Vec3::new(10.0, 33.0, 4.0)));
    // Expressions drive too, and undo puts it back.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": "pi rad"}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 66.0, 8.0)));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));
}

/// A joint value too large says so, rather than that it is not a number (#9625).
#[test]
fn a_joint_value_out_of_range_says_so() {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    let hinge = json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"});
    run(&mut s, "joint.create", hinge);
    for value in [json!(1e9), json!(-5e15)] {
        let e = s.execute("joint.drive", &json!({"joint": "Hinge", "value": value})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("a joint value must be a number between -1e9 and 1e9") && !e.contains("numbers or expressions"), "{value}: {e}");
    }
    let e = s.execute("joint.drive", &json!({"joint": "Hinge", "value": true})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("a joint value must be a number or an expression"), "{e}");
}

/// Two 10 mm cubes in components A (grounded) and B.
fn two_cubes() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let o = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    s
}

#[test]
fn slider_limits_clamp() {
    let mut s = two_cubes();
    // B's bottom face onto A's top face (faces meet), sliding along their common normal.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "limits": [[0, 10]], "name": "Rail"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // B now sits on A.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    assert_eq!((&r["values"][0]["value"], &r["values"][0]["unit"]), (&json!(10.0), &json!("mm")), "{r}");
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 20.0)));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
    // Changing the limits clamps the current value.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 8}));
    run(&mut s, "joint.limits", json!({"joint": "Rail", "min": 0, "max": 4}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 14.0)));
    // Joints follow edits: A gets taller, B rides on its new top.
    let h = s
        .doc
        .features
        .iter()
        .find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. }))
        .and_then(|f| f.param_names.get(2).cloned())
        .unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": h, "expression": "15"}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 19.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
}

#[test]
fn rigid_cycles_solve_and_conflicts_report() {
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    // A rigid group and a closing as-built joint: a consistent loop.
    run(&mut s, "joint.rigid_group", json!({"occurrences": ["A", "B", "C"]}));
    let r = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "B", "b": "C"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "C", Vec3::new(40.0, 0.0, 0.0)), Vec3::new(40.0, 0.0, 0.0)));
    let l = run(&mut s, "joint.list", json!({}));
    assert!(l["dof"].as_array().unwrap().iter().all(|d| d["dof"] == 0), "{l}");
    // A joint that wants C elsewhere conflicts and says so.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "A", "point": [0, 0, 0]}, "b": {"occurrence": "C", "point": [40, 0, 0]}, "name": "Wrong"}),
    );
    let c = r["conflicts"].as_array().cloned().unwrap_or_default();
    assert_eq!(c.len(), 1, "{r}");
    assert!(c[0]["why"].as_str().unwrap_or("").contains("off by"), "{r}");
    // A rigid joint has nothing to drive: the refusal names the types that move (#6981).
    let e = s.execute("joint.drive", &json!({"joint": "Wrong", "value": 1})).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("does not move") && e.contains("joint.edit") && e.contains("revolute") && e.contains("cylindrical"), "{e}");
    // Suppressing it clears the conflict.
    let r = run(&mut s, "joint.edit", json!({"joint": "Wrong", "suppressed": true}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0));
    assert!(
        s.execute(
            "joint.create",
            &json!({"type": "rigid", "a": {"occurrence": "A", "point": [0, 0, 0]}, "b": {"occurrence": "A", "point": [1, 0, 0]}})
        )
        .is_err()
    );
    assert!(s.execute("joint.create", &json!({"type": "hinge"})).is_err());
}

/// A 30 x 30 x 3 plate with a through slot whose x = 20 wall carries one rib (0.3 tall, 0.8
/// wide), and a peg in the slot with 0.15 clearance all round.
fn ribbed_slot_and_peg(s: &mut Session, peg_x1: f64) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 3, "body_name": "Plate"}));
    run(s, "sketch.create", json!({"plane": "XY", "offset": -1}));
    let slot = json!([[10, 5], [20, 5], [20, 14.6], [19.7, 14.6], [19.7, 15.4], [20, 15.4], [20, 25], [10, 25]]);
    run(s, "sketch.line", json!({"points": slot, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 5, "operation": "cut", "targets": ["Plate"]}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [10.15, 5.15], "p1": [peg_x1, 24.85]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 3, "operation": "new", "body_name": "Peg"}));
}

/// The peg overlaps only the rib: 0.15 x 0.8 x 3 = 0.36 mm³ (#442).
#[test]
fn interference_finds_a_peg_overlapping_a_small_rib() {
    let mut s = Session::default();
    ribbed_slot_and_peg(&mut s, 19.85);
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert!((i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0) - 0.36).abs() < 1e-6, "{i}");
    let (lo, hi) = (&i["interferences"][0]["bbox"]["min"], &i["interferences"][0]["bbox"]["max"]);
    assert!((lo[0].as_f64().unwrap_or(0.0) - 19.7).abs() < 1e-6 && (hi[0].as_f64().unwrap_or(0.0) - 19.85).abs() < 1e-6, "{i}");
}

/// An arc revolved up to a post ends on the post's wall, where the two bodies can't be
/// intersected: the pair is listed as unchecked instead of passing as clear.
#[test]
fn interference_lists_a_pair_it_could_not_check() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 14, -5], "radius": 4, "height": 15, "body_name": "Post"}));
    run(&mut s, "sketch.create", json!({"name": "Profile", "plane": "XZ"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [15, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch": "Profile", "axis": {"origin": [0, 0, 0], "dir": [0, 0, 1]}, "to": [4, 14, 2], "body_name": "Arc"}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 0, "{i}");
    let unchecked = i["unchecked"].as_array().cloned().unwrap_or_default();
    assert_eq!(unchecked.len(), 1, "{i}");
    assert!(unchecked[0]["reason"].as_str().is_some_and(|r| r.starts_with("not verified") && !r.contains("displacement")), "{i}");
    // Bodies that are clear of each other, or overlap plainly, leave the list empty.
    let mut s = two_cubes();
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
}

#[test]
fn motion_links_and_interference() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Screw"}));
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, 0]}));
    // No overlap while B sits on A; then push B 4 mm into A.
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 0, "{i}");
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, -4]}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert!((i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0) - 400.0).abs() < 1e-3, "{i}");
    // A motion link: a second joint's value follows.
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "D"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0]}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "D", "face": [65, 5, 0]}, "name": "Follower"}),
    );
    run(&mut s, "joint.motion_link", json!({"a": "Screw", "ia": 1, "b": "Follower", "ratio": 2}));
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, 3]}));
    assert!(near(place(&s, "D", Vec3::new(65.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "D", Vec3::new(65.0, 5.0, 0.0)));
    // Saved and reopened, joints come back.
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.assembly, s.doc.assembly);
    let mut t = Session::new(back);
    let l = run(&mut t, "joint.list", json!({}));
    assert_eq!(l["joints"].as_array().map(Vec::len), Some(2));
}

/// Two faces joined mate: B's bottom face lands on A's top face, B on top of A (not inside
/// it); Flip turns B over instead.
#[test]
fn faces_mate_by_default() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Stack"}));
    // B's bottom centre (25, 5, 0) at A's top centre, B above it.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 20.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    run(&mut s, "joint.edit", json!({"joint": "Stack", "flip": true}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 0.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
}

/// Edit Joint takes new sides as joint.create does: the joint moves its attachment to them, and
/// keeps its limits, its drive and the expression its offset follows.
#[test]
fn edit_moves_a_side_and_keeps_limits_and_expressions() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "2 mm"}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "name": "Rail", "offset": "gap", "limits": [[0, 10]], "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}),
    );
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 4}));
    // B's bottom centre rides 2 mm (the gap) plus 4 mm (the drive) above A's top centre.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
    // B's top centre, where B is placed now, is the new side (`face` is a world point).
    let top = place(&s, "B", Vec3::new(25.0, 5.0, 10.0));
    let r = run(&mut s, "joint.edit", json!({"joint": "Rail", "b": {"occurrence": "B", "face": [top.x, top.y, top.z]}}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    let j = s.doc.assembly.joints.first().cloned().unwrap_or_else(|| panic!("no joint"));
    assert_eq!(j.limits, vec![Some((0.0, 10.0))]);
    assert_eq!(j.exprs.offset.as_deref(), Some("gap"));
    assert_eq!(j.values, vec![4.0]);
    assert_eq!(j.name, "Rail");
}

/// Sides left out of joint.edit stay as they are; the other parameters still change.
#[test]
fn omitted_sides_stay() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    let before = s.doc.assembly.joints[0].clone();
    run(&mut s, "joint.edit", json!({"joint": "Stack", "flip": true, "name": "Flipped"}));
    let after = &s.doc.assembly.joints[0];
    assert_eq!((&after.a, &after.b), (&before.a, &before.b));
    assert!(after.flip && after.name == "Flipped");
}

/// One undo step puts the sides back, and the parts with them.
#[test]
fn one_undo_restores_the_old_sides() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    let before = s.doc.assembly.joints[0].clone();
    let top = place(&s, "B", Vec3::new(25.0, 5.0, 10.0));
    run(&mut s, "joint.edit", json!({"joint": "Stack", "b": {"occurrence": "B", "face": [top.x, top.y, top.z]}}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 10.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.joints[0], before);
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
}

/// Moving a side re-solves the joint as joint.create would at the same pose: the turn B keeps
/// about the joint's axis follows its new origin, so B does not turn by a half turn (or any turn
/// a fresh create would not give). Both designs start from the same pose; one moves the side,
/// the other deletes the joint and creates it with the new sides.
#[test]
fn moving_a_side_places_the_part_as_create_would() {
    let a_faces = [[5, 5, 10], [5, 0, 5], [5, 10, 5], [10, 5, 5], [0, 5, 5]];
    let b_faces = [[25, 5, 0], [25, 5, 10], [25, 0, 5], [25, 10, 5], [20, 5, 5], [30, 5, 5]];
    // A point of B off the axes of every face above.
    let probe = |s: &Session| place(s, "B", Vec3::new(22.0, 3.0, 7.0));
    let mut differ = Vec::new();
    for a in a_faces {
        for b in b_faces {
            let mut edited = two_cubes();
            run(&mut edited, "joint.create", json!({"type": "rigid", "name": "J", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
            let mut created = Session::new(solidcraft_doc::Document::from_json(&edited.doc.to_json()).unwrap());
            // The new sides in world coordinates, where B is before the move.
            let (aw, bw) = (Vec3::new(a[0] as f64, a[1] as f64, a[2] as f64), place(&edited, "B", Vec3::new(b[0] as f64, b[1] as f64, b[2] as f64)));
            let side = |occurrence: &str, p: Vec3| json!({"occurrence": occurrence, "face": [p.x, p.y, p.z]});
            run(&mut edited, "joint.edit", json!({"joint": "J", "a": side("A", aw), "b": side("B", bw)}));
            run(&mut created, "joint.delete", json!({"joint": "J"}));
            run(&mut created, "joint.create", json!({"type": "rigid", "name": "J", "a": side("A", aw), "b": side("B", bw)}));
            if !near(probe(&edited), probe(&created)) {
                differ.push((a, b, probe(&edited), probe(&created)));
            }
        }
    }
    assert!(differ.is_empty(), "moved sides place B differently from create: {differ:?}");
}

/// A side that makes no joint is refused, and the joint is left as it was: a side on the other
/// side's component, a side that is not a joint origin, an unknown occurrence.
#[test]
fn an_invalid_side_is_refused_and_the_joint_is_kept() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    let before = s.doc.assembly.joints[0].clone();
    // A is a's occurrence and B is b's: each side is tried on the other's occurrence, and with
    // the faults every side can have.
    let cases = [
        ("a", json!({"occurrence": "B", "face": [25, 5, 10]})),
        ("a", json!({"occurrence": "Nope", "face": [5, 5, 10]})),
        ("a", json!({"occurrence": "A"})),
        ("a", json!(5)),
        ("b", json!({"occurrence": "A", "face": [5, 5, 0]})),
        ("b", json!({"occurrence": "Nope", "face": [25, 5, 0]})),
        ("b", json!({"occurrence": "B"})),
        ("b", json!("NoSuchOrigin")),
    ];
    for (key, side) in cases {
        let r = s.execute("joint.edit", &json!({"joint": "Stack", key: side.clone()}));
        assert!(r.is_err(), "{key}: {side} should be refused");
        assert_eq!(s.doc.assembly.joints[0], before, "{key}: {side} changed the joint");
    }
}

/// A move of a jointed part that the joint takes back: the model is evaluated for the
/// placements the solve leaves, so a mirror of the part's body, made from the root, is where a
/// fresh evaluation of the design puts it. The model was the one evaluated for the move before
/// the solve took it back, and its mirror didn't match the part.
#[test]
fn a_feature_reading_a_jointed_parts_placement_follows_the_solve() {
    let mut s = Session::default();
    for (cmd, p) in [
        ("component.create", json!({"grounded": true, "name": "Base"})),
        ("solid.box", json!({"corner": [-11, -25, 0], "height": 9, "length": 11, "width": 14})),
        ("component.create", json!({"grounded": false, "name": "Slide"})),
        ("solid.cylinder", json!({"base": [-2, -10, 7], "height": 7, "radius": 2})),
        ("component.activate", json!({"component": "root"})),
        ("joint.as_built", json!({"a": "Base:1", "b": "Slide:1", "name": "Rail", "type": "slider"})),
        ("solid.mirror", json!({"bodies": ["Body2"], "plane": "XY"})),
        ("occurrence.move", json!({"occurrence": "Slide:1", "translate": [-10, -18, 2]})),
    ] {
        s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
    }
    let boxes = |st: &solidcraft_doc::ModelState| {
        st.bodies
            .iter()
            .map(|b| (b.name.clone(), b.body.outline_bounds()))
            .map(|(n, bb)| format!("{n} {:.6?} {:.6?}", bb.min, bb.max))
            .collect::<Vec<_>>()
    };
    let mut fresh = solidcraft_doc::Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&s.doc);
    assert_eq!(boxes(&s.model.state()), boxes(&fresh.state()));
}

/// #1239: two parts as bodies of one component (a multi-body STEP import). The refusal names the
/// way out, and after Create Components from Bodies the same origins make the joint.
#[test]
fn bodies_of_one_component_get_components_first() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    let p = json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}});
    let e = s.execute("joint.create", &p).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("two different occurrences") && e.contains("component.from_bodies"), "{e}");
    run(&mut s, "component.from_bodies", json!({"bodies": ["Body1", "Body2"]}));
    run(&mut s, "joint.create", p);
    // The first joint's A side stays; B lands on top of it.
    assert!(near(place(&s, "Body2", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
}

/// #527: an assembly with its fasteners passes 200 bodies quickly; the clash check must still
/// cover the whole design. 201 separate 1 mm cubes 2 mm apart, plus one cube pushed half-way
/// into the first: exactly that pair overlaps, by 0.5 mm³.
#[test]
fn interference_checks_designs_with_more_than_200_bodies() {
    let mut s = Session::default();
    for i in 0..201 {
        run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [2 * i, 0, 0], "operation": "new"}));
    }
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [0.5, 0, 0], "operation": "new", "body_name": "Clash"}));
    assert_eq!(s.model.state().bodies.len(), 202);
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["checked"], 202, "{i}");
    assert_eq!(i["count"], 1, "{i}");
    let hit = &i["interferences"][0];
    let mut pair = [hit["a"].as_str().unwrap_or(""), hit["b"].as_str().unwrap_or("")];
    pair.sort_unstable();
    assert_eq!(pair, ["Body1", "Clash"], "{i}");
    assert!((hit["volume_mm3"].as_f64().unwrap_or(0.0) - 0.5).abs() < 1e-6, "{i}");
}

/// The bounding-box sweep behind `inspect.interference` finds exactly the pairs a check of every
/// pair finds (touching boxes count), and reports how many it left out past its limit.
#[test]
fn interference_box_sweep_matches_every_pair_check() {
    use solidcraft_geom::Aabb3;
    let cube = |x: f64, y: f64, z: f64, e: f64| Aabb3 { min: Vec3::new(x, y, z), max: Vec3::new(x + e, y + e, z + e) };
    // A deterministic scatter: overlapping, touching, and apart in only one axis.
    let boxes: Vec<Aabb3> = (0..60)
        .map(f64::from)
        .map(|i| cube((i * 7.3) % 20.0, (i * 3.1) % 9.0, (i * 5.7) % 4.0, 1.0 + (i % 3.0)))
        .chain([cube(30.0, 0.0, 0.0, 1.0), cube(31.0, 0.0, 0.0, 1.0)])
        .collect();
    let mut every = Vec::new();
    for (i, a) in boxes.iter().enumerate() {
        for (j, b) in boxes.iter().enumerate().skip(i + 1) {
            let apart = a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z;
            if !apart {
                every.push((i, j));
            }
        }
    }
    assert!(every.contains(&(60, 61)), "touching boxes are a candidate pair");
    let (pairs, skipped) = super::overlapping_pairs(&boxes, usize::MAX);
    assert_eq!((pairs, skipped), (every.clone(), 0));
    let (pairs, skipped) = super::overlapping_pairs(&boxes, 10);
    assert_eq!((pairs.len(), skipped), (10, every.len() - 10));
}

/// A small block in a new component: a part of a linkage, where only its joints matter.
pub(super) fn part(s: &mut Session, name: &str, corner: [f64; 3]) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": corner}));
}

/// A joint between two parts as they are, at a world point with its z along `z`.
pub(super) fn pin(s: &mut Session, kind: &str, name: &str, a: &str, b: &str, p: Vec3, z: Vec3) {
    let at = |o: &str| json!({"occurrence": o, "point": [p.x, p.y, p.z], "z": [z.x, z.y, z.z]});
    let r = run(s, "joint.create", json!({"type": kind, "a": at(a), "b": at(b), "flip": true, "name": name}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Where circles about `p` (radius `r`) and `q` (radius `s`) in the XY plane meet.
pub(super) fn meet(p: Vec3, r: f64, q: Vec3, s: f64) -> [Vec3; 2] {
    let d = q - p;
    let l = d.len();
    let a = (r * r - s * s + l * l) / (2.0 * l);
    let m = p + d * (a / l);
    let n = Vec3::new(-d.y, d.x, 0.0) * ((r * r - a * a).max(0.0).sqrt() / l);
    [m + n, m - n]
}

/// A crank-rocker four-bar about +Z: ground pivots O2 = (0, 0) and O4 = (45, 0), crank 20,
/// coupler 50, rocker 40, assembled with the crank at 90°. Returns the crank pin P0 and the
/// rocker pin Q0 (the upper, open assembly).
pub(super) fn four_bar() -> (Session, Vec3, Vec3) {
    let mut s = Session::default();
    let (o2, o4, p0) = (Vec3::ZERO, Vec3::new(45.0, 0.0, 0.0), Vec3::new(0.0, 20.0, 0.0));
    let [q0, _] = meet(p0, 50.0, o4, 40.0);
    for (name, c) in [("Ground", o2), ("Crank", p0), ("Coupler", (p0 + q0) * 0.5), ("Rocker", q0)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o2, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Coupler", p0, Vec3::Z);
    pin(&mut s, "revolute", "Rocker pin", "Coupler", "Rocker", q0, Vec3::Z);
    pin(&mut s, "revolute", "Rocker", "Ground", "Rocker", o4, Vec3::Z);
    (s, p0, q0)
}

/// The four-bar's rocker pin with the crank driven to `to`°, followed a degree at a time from
/// `from`° (where it is at `q`), so it stays on its branch.
fn rocker_pin(from: f64, to: f64, mut q: Vec3) -> Vec3 {
    let n = ((to - from).abs().ceil() as usize).max(1);
    for k in 1..=n {
        let th = (90.0 + from + (to - from) * k as f64 / n as f64).to_radians();
        let [m1, m2] = meet(Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0), 50.0, Vec3::new(45.0, 0.0, 0.0), 40.0);
        q = if m1.dist(q) <= m2.dist(q) { m1 } else { m2 };
    }
    q
}

#[test]
fn four_bar_follows_its_crank() {
    let (mut s, p0, q0) = four_bar();
    let mut q = q0;
    let mut at_330 = q0;
    for k in 1..=12 {
        let v = 30.0 * f64::from(k);
        let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": v}));
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{v}°: {r}");
        assert!(r["stopped_by_loop"].is_null(), "{v}°: {r}");
        // The driven joint keeps exactly the value asked for, answered in degrees as asked.
        assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - v).abs() < 1e-9 && r["values"][0]["unit"] == "deg", "{r}");
        q = rocker_pin(v - 30.0, v, q);
        let th = (90.0 + v).to_radians();
        let p = Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0);
        for (part, local, want) in [("Crank", p0, p), ("Coupler", p0, p), ("Coupler", q0, q), ("Rocker", q0, q)] {
            assert!(near(place(&s, part, local), want), "{v}°: {part} {:?} wants {want:?}", place(&s, part, local));
        }
        if k == 11 {
            at_330 = q;
        }
    }
    // Back where it started after a full turn, and undo steps back.
    assert!(near(q, q0));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "Rocker", q0), at_330), "{:?}", place(&s, "Rocker", q0));
    // One 180° drive lands on the same (open) branch as the stepped path.
    let (mut s, _, q0) = four_bar();
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 180}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 180.0, q0)), "{:?}", place(&s, "Rocker", q0));
    // A motion study playing the crank places the rocker the same way.
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 10, "keys": [{"joint": "Crank", "points": [[0, 0], [10, 120]]}]}));
    let r = run(&mut s, "motion.play", json!({"study": "Turn", "step": 5}));
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(180.0, 60.0, rocker_pin(0.0, 180.0, q0))), "{:?}", place(&s, "Rocker", q0));
}

/// The rocker swings between its toggle positions, where crank and coupler line up (|Q| = 70
/// and |Q| = 30, about 69.3° and 139.2° from the ground line); driving it further stops where
/// the loop still closes.
#[test]
fn four_bar_rocker_stops_where_the_loop_cannot_close() {
    let (mut s, _, q0) = four_bar();
    let start = q0.y.atan2(q0.x - 45.0).to_degrees();
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": 100}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let reached = r["values"][0]["value"].as_f64().unwrap_or(f64::NAN);
    // |Q|² = 45² + 40² + 2·45·40·cos φ = 30².
    let limit = ((900.0f64 - 3625.0) / 3600.0).acos().to_degrees() - start;
    assert!(reached > limit - 5.0 && reached <= limit, "stopped at {reached}°, the limit is {limit}°");
    // Inside its swing it moves freely, and the crank follows.
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": -10}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    let q = place(&s, "Rocker", q0);
    assert!((q.y.atan2(q.x - 45.0).to_degrees() - (start - 10.0)).abs() < 1e-6, "{q:?}");
    assert!(place(&s, "Coupler", q0).dist(q) < 1e-6);
    // A full turn would end where it started, but the rocker can't get there.
    let (mut s, _, _) = four_bar();
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert!(r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) <= limit, "{r}");
}

/// A loop value a motion link sets follows through the start of its link chain: a dial outside
/// the loop turns a gear, which sets the rocker's angle, so turning the crank turns the dial.
#[test]
fn four_bar_turns_the_source_of_a_linked_loop_value() {
    let linked = || {
        let (mut s, _, q0) = four_bar();
        for (name, x) in [("Dial", 80.0), ("Gear", 100.0)] {
            part(&mut s, name, [x, 0.0, -5.0]);
            pin(&mut s, "revolute", name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
        }
        run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Gear", "ratio": 1}));
        run(&mut s, "joint.motion_link", json!({"a": "Gear", "b": "Rocker", "ratio": 0.5}));
        (s, q0)
    };
    let (mut s, q0) = linked();
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let q = rocker_pin(0.0, 60.0, q0);
    assert!(near(place(&s, "Rocker", q0), q), "{:?} wants {q:?}", place(&s, "Rocker", q0));
    let turn = q.y.atan2(q.x - 45.0) - q0.y.atan2(q0.x - 45.0);
    let dial = s.doc.assembly.joints.iter().find(|j| j.name == "Dial").and_then(|j| j.values.first().copied()).unwrap_or(f64::NAN);
    assert!((dial - 2.0 * turn).abs() < 1e-9, "dial {dial}, rocker turned {turn}");
    // Driving the dial drives the rocker through the chain: two turns of the dial would turn
    // the rocker a full turn, which it can't make.
    let (mut s, _) = linked();
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 720}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Values left out of a drive are free to follow in a loop: a plate on a planar joint whose
/// travel a diagonal slider limits. Driving only the planar joint's x moves its y along.
#[test]
fn values_left_out_of_a_drive_follow_the_loop() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    part(&mut s, "Plate", [-1.0, -1.0, 0.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    let frame = |o: &str, z: [f64; 3], x: [f64; 3]| json!({"occurrence": o, "frame": {"origin": [0, 0, 0], "z": z, "x": x}});
    let flat = |o: &str| frame(o, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let r = run(&mut s, "joint.create", json!({"type": "planar", "a": flat("Ground"), "b": flat("Plate"), "flip": true, "name": "Table"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let diagonal = |o: &str| frame(o, [1.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let r = run(&mut s, "joint.create", json!({"type": "slider", "a": diagonal("Ground"), "b": diagonal("Plate"), "flip": true, "name": "Rail"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Table", "values": [10]}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    assert!(near(place(&s, "Plate", Vec3::ZERO), Vec3::new(10.0, 10.0, 0.0)), "{:?}", place(&s, "Plate", Vec3::ZERO));
}

/// A slider-crank: crank 15 about the origin, rod 40, a piston sliding along X. Returns the crank
/// pin and the wrist pin.
fn slider_crank() -> (Session, Vec3, Vec3) {
    let mut s = Session::default();
    let (o, p0) = (Vec3::ZERO, Vec3::new(0.0, 15.0, 0.0));
    let s0 = Vec3::new((40.0f64 * 40.0 - 15.0 * 15.0).sqrt(), 0.0, 0.0);
    for (name, c) in [("Ground", o), ("Crank", p0), ("Rod", s0 * 0.5), ("Piston", s0)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Rod", p0, Vec3::Z);
    pin(&mut s, "revolute", "Wrist pin", "Rod", "Piston", s0, Vec3::Z);
    pin(&mut s, "slider", "Bore", "Ground", "Piston", s0, Vec3::X);
    (s, p0, s0)
}

/// The piston sits at a·cos θ + √(b² − (a·sin θ)²).
#[test]
fn slider_crank_moves_the_piston() {
    let (mut s, p0, s0) = slider_crank();
    for v in [40.0, 135.0, 270.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": v}));
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{v}°: {r}");
        let th = (90.0f64 + v).to_radians();
        let x = 15.0 * th.cos() + (40.0f64.powi(2) - (15.0 * th.sin()).powi(2)).sqrt();
        assert!(near(place(&s, "Piston", s0), Vec3::new(x, 0.0, 0.0)), "{v}°: {:?} wants x = {x}", place(&s, "Piston", s0));
        assert!(near(place(&s, "Rod", p0), Vec3::new(15.0 * th.cos(), 15.0 * th.sin(), 0.0)));
    }
}

/// A slotted-arm drive: a clamp turns about the origin (Yaw) and carries a pin 26 from it; an arm
/// turns about a shaft 52 out on a motor fixed to the ground, and the pin slides in the arm's slot
/// (Slot: its frame at the shaft, x along the slot towards the pin). Every joint of the loop
/// moves when it does, the slot's included. The pin's face sits `gap` mm off the arm's along
/// the axes: a gap no value of the loop takes out, within the conflict check.
fn slotted_arm(yaw_first: bool, gap: f64) -> Session {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Clamp", 26.0), ("Motor", 52.0), ("Arm", 40.0)] {
        part(&mut s, name, [x - 1.0, -1.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    // The joint order sets the tree: with the yaw first, the arm hangs from the clamp through the
    // slot and the arm's shaft closes the loop; with the motor first, the slot closes it.
    if yaw_first {
        pin(&mut s, "revolute", "Yaw", "Ground", "Clamp", Vec3::ZERO, Vec3::Z);
    }
    pin(&mut s, "rigid", "Mount", "Ground", "Motor", Vec3::new(52.0, 0.0, 0.0), Vec3::Z);
    if !yaw_first {
        pin(&mut s, "revolute", "Yaw", "Ground", "Clamp", Vec3::ZERO, Vec3::Z);
    }
    pin(&mut s, "revolute", "Shaft", "Arm", "Motor", Vec3::new(52.0, 0.0, 0.0), Vec3::Z);
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "pin_slot", "a": framed("Arm", [52.0, 0.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
            "b": framed("Clamp", [26.0, 0.0, gap], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]), "flip": true, "values": [0, 26], "name": "Slot"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    s
}

/// A joint's stored values.
fn values_of(s: &Session, joint: &str) -> Vec<f64> {
    s.doc.assembly.joints.iter().find(|j| j.name == joint).map(|j| j.values.clone()).unwrap_or_default()
}

/// Driving either end of the slotted arm moves the other: the arm turns atan(26 sin θ / (52 −
/// 26 cos θ)) for a yaw θ, and the pin slides out from 26 to √(26² + 52² − 2·26·52 cos θ).
#[test]
fn slotted_arm_drives_both_ways() {
    for (yaw_first, gap) in [(false, 0.0), (true, 0.0), (false, 3e-8), (true, 3e-8)] {
        slotted_arm_drives(yaw_first, gap);
    }
}

fn slotted_arm_drives(yaw_first: bool, gap: f64) {
    let mut s = slotted_arm(yaw_first, gap);
    let (shaft, pin_at) = (Vec3::new(52.0, 0.0, 0.0), Vec3::new(26.0, 0.0, 0.0));
    let check = |s: &Session, what: &str| {
        let th = values_of(s, "Yaw").first().copied().unwrap_or(f64::NAN);
        let p = Vec3::new(26.0 * th.cos(), 26.0 * th.sin(), 0.0);
        assert!(near(place(s, "Clamp", pin_at), p), "{what}: pin {:?} wants {p:?}", place(s, "Clamp", pin_at));
        let arm = values_of(s, "Shaft").first().copied().unwrap_or(f64::NAN);
        let want = (26.0 * th.sin()).atan2(52.0 - 26.0 * th.cos());
        assert!((arm.abs() - want.abs()).abs() < 1e-6, "{what}: arm {arm} wants ±{want}");
        // The arm's slot points at the pin, which sits as far out as the slot says.
        let tip = place(s, "Arm", Vec3::new(40.0, 0.0, 0.0));
        let along = (p - shaft) * (1.0 / (p - shaft).len());
        assert!(near(tip, shaft + along * 12.0), "{what}: arm tip {tip:?}");
        let slide = values_of(s, "Slot").get(1).copied().unwrap_or(f64::NAN);
        assert!((slide.abs() - (p - shaft).len()).abs() < 1e-6, "{what}: slide {slide} wants {}", (p - shaft).len());
    };
    check(&s, &format!("yaw first {yaw_first}, gap {gap}, at rest"));
    for v in [10.0, 40.0, -40.0, 0.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Yaw", "value": v}));
        assert!(
            r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0),
            "yaw first {yaw_first}, gap {gap}, yaw {v}°: {r}"
        );
        assert!((values_of(&s, "Yaw").first().copied().unwrap_or(f64::NAN) - f64::to_radians(v)).abs() < 1e-9);
        check(&s, &format!("yaw first {yaw_first}, gap {gap}, yaw {v}°"));
    }
    let slide = values_of(&s, "Slot").get(1).copied().unwrap_or(f64::NAN).abs();
    assert!((slide - 26.0).abs() < 1e-6, "{slide}");
    for v in [15.0, -27.0, 27.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Shaft", "value": v}));
        assert!(
            r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0),
            "yaw first {yaw_first}, gap {gap}, arm {v}°: {r}"
        );
        assert!((values_of(&s, "Shaft").first().copied().unwrap_or(f64::NAN) - f64::to_radians(v)).abs() < 1e-9);
        check(&s, &format!("yaw first {yaw_first}, gap {gap}, arm {v}°"));
    }
    // The clamp's pin reaches a point on its circle, the arm following.
    let (th, at) = (20f64.to_radians(), place(&s, "Clamp", pin_at));
    let r = run(&mut s, "joint.reach", json!({"occurrence": "Clamp", "at": [at.x, at.y, at.z], "point": [26.0 * th.cos(), 26.0 * th.sin(), 0.0]}));
    assert!(r["reached"] == json!(true) && r["stopped_by_loop"].is_null(), "yaw first {yaw_first}, gap {gap}, reach: {r}");
    check(&s, &format!("yaw first {yaw_first}, gap {gap}, reach"));
    // A limit on the arm stops the yaw where the arm meets it.
    run(&mut s, "joint.limits", json!({"joint": "Shaft", "min": -20, "max": 20}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Yaw", "value": 40}));
    assert!(r["stopped_by_loop"].is_object() && r["conflicts"].as_array().map(Vec::len) == Some(0), "yaw first {yaw_first}, gap {gap}, limit: {r}");
    check(&s, &format!("yaw first {yaw_first}, gap {gap}, limit"));
    let arm = values_of(&s, "Shaft").first().copied().unwrap_or(f64::NAN).abs().to_degrees();
    assert!(arm > 19.9 && arm <= 20.0 + 1e-9, "yaw first {yaw_first}, gap {gap}: the arm stopped at {arm}°");
}

/// The four-bar with its rocker a rod on an axle (a cylindrical joint), and the rocker pin
/// joining the coupler to the rod 4 mm up the axle from where the rod sits, its axis turned
/// `tilt` rad from the others about Y. Returns the closing joint's answer and the rocker pin.
fn rod_on_an_axle(tilt: f64) -> (Session, Value, Vec3) {
    let mut s = Session::default();
    let (o2, o4, p0) = (Vec3::ZERO, Vec3::new(45.0, 0.0, 0.0), Vec3::new(0.0, 20.0, 0.0));
    let [q0, _] = meet(p0, 50.0, o4, 40.0);
    for (name, c) in [("Ground", o2), ("Crank", p0), ("Coupler", (p0 + q0) * 0.5), ("Rod", q0)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o2, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Coupler", p0, Vec3::Z);
    pin(&mut s, "cylindrical", "Axle", "Ground", "Rod", o4, Vec3::Z);
    let z = [tilt.sin(), 0.0, tilt.cos()];
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "Coupler", "point": [q0.x, q0.y, 0.0], "z": z},
            "b": {"occurrence": "Rod", "point": [q0.x, q0.y, 4.0], "z": z}, "flip": true, "name": "Rocker pin"}),
    );
    (s, r, q0)
}

/// Axes of holes picked on parts are seldom exactly parallel (#6211: a few hundredths of a
/// microradian apart on a lever). A closed loop through them leaves a gap of nanometres no value
/// takes out; it still closes, the rod sliding down its axle, and follows its crank. A tilt that
/// shows still locks it.
#[test]
fn a_loop_whose_axes_are_a_hair_out_of_parallel_still_moves() {
    let (mut s, r, q0) = rod_on_an_axle(1e-7);
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let up = Vec3::new(0.0, 0.0, 4.0);
    let close = |a: Vec3, b: Vec3| a.dist(b) < 1e-4;
    assert!(close(place(&s, "Rod", q0 + up), q0), "{:?}", place(&s, "Rod", q0 + up));
    for part in ["Crank", "Coupler", "Rod"] {
        assert_eq!(dof_of(&mut s, part), 1, "{part}");
    }
    for v in [30.0, 60.0, -45.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": v}));
        assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{v}°: {r}");
        let q = rocker_pin(0.0, v, q0);
        assert!(close(place(&s, "Rod", q0 + up), q), "{v}°: {:?} wants {q:?}", place(&s, "Rod", q0 + up));
    }
    // Dragged by its crank pin (the viewport's drag of a part joints hold), the rod follows too.
    let (at, th) = (place(&s, "Crank", Vec3::new(0.0, 20.0, 0.0)), 110f64.to_radians());
    let r = run(&mut s, "joint.reach", json!({"occurrence": "Crank", "at": [at.x, at.y, at.z], "point": [20.0 * th.cos(), 20.0 * th.sin(), 0.0]}));
    assert!(r["reached"] == json!(true) && r["stopped_by_loop"].is_null(), "{r}");
    let q = rocker_pin(0.0, 20.0, q0);
    assert!(close(place(&s, "Rod", q0 + up), q), "reach: {:?} wants {q:?}", place(&s, "Rod", q0 + up));
    // Tilted a milliradian, the loop is locked: no longer planar, it can't turn.
    let (mut s, r, _) = rod_on_an_axle(1e-3);
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert_eq!(dof_of(&mut s, "Rod"), 0);
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 30}));
    assert!(r["stopped_by_loop"].is_object() && r["values"][0]["value"].as_f64().is_some_and(|v| v.abs() < 0.1), "{r}");
}

/// A joint frame on an occurrence, by origin and axes.
fn framed(o: &str, origin: [f64; 3], z: [f64; 3], x: [f64; 3]) -> Value {
    json!({"occurrence": o, "frame": {"origin": origin, "z": z, "x": x}})
}

/// A loop open from the start closes from where it is: an arm on a revolute joint, and a rigid
/// joint that wants it turned 90°.
#[test]
fn an_open_loop_closes_from_where_it_is() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    part(&mut s, "Arm", [0.0, -1.0, 0.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Pivot", "Ground", "Arm", Vec3::ZERO, Vec3::Z);
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": framed("Ground", [0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            "b": framed("Arm", [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), "flip": true, "name": "Lock"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)), Vec3::new(0.0, 10.0, 0.0)), "{:?}", place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)));
}

/// A loop that can't close does not hold up another one: the four-bar still drives beside a
/// block whose rigid joint wants it where its pivot can't take it.
#[test]
fn a_broken_loop_does_not_stop_another() {
    let (mut s, _, q0) = four_bar();
    part(&mut s, "Block", [200.0, -1.0, -5.0]);
    pin(&mut s, "revolute", "Block pivot", "Ground", "Block", Vec3::new(200.0, 0.0, 0.0), Vec3::Z);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Ground", "point": [200, 0, 0]}, "b": {"occurrence": "Block", "point": [210, 0, 0]}, "flip": true, "name": "Wrong"}),
    );
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    let names: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, ["Wrong"], "{r}");
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 60.0, q0)), "{:?}", place(&s, "Rocker", q0));
}

/// A loop locked rigid can't move at all: driving its crank stops at once.
#[test]
fn a_locked_loop_does_not_move() {
    let (mut s, _, _) = four_bar();
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Rocker", "name": "Lock"}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert!(r["values"][0]["value"].as_f64().unwrap_or(f64::NAN).to_radians().abs() < 1e-3, "{r}");
}

/// A link that turns the rocker 100 times as far as its dial: a small turn of the dial is a full
/// turn of the rocker, which it can't make.
#[test]
fn a_geared_down_loop_value_is_followed_all_the_way() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Rocker", "ratio": 100}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 3.6}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// A suppressed dial still turns the rocker through its motion link, and a rocker whose value
/// the file leaves out starts from 0: both are followed all the way, so a full turn stops.
#[test]
fn a_full_turn_stops_from_a_suppressed_dial_or_a_missing_value() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Rocker", "ratio": 0.5}));
    run(&mut s, "joint.edit", json!({"joint": "Dial", "suppressed": true}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 720}));
    assert!(r["stopped_by_loop"].is_object() && r["moved"] == true && r.get("reason").is_none(), "{r}");
    // Undriven, the suppressed dial still follows the rocker when the crank turns.
    run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 0}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    let (mut s, _, _) = four_bar();
    if let Some(j) = s.doc_mut().assembly.joints.iter_mut().find(|j| j.name == "Rocker") {
        j.values.clear();
    }
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
}

/// Loops that only share a rigid joint are independent: an arm whose lock can't close does not
/// stop another arm, beside it on the same rigidly mounted base, from closing its own loop.
#[test]
fn loops_sharing_only_a_rigid_joint_are_solved_apart() {
    let mut s = Session::default();
    for (name, x) in [("Ground", -1.0), ("Base", -1.0), ("Arm", 0.0), ("Other", 100.0)] {
        part(&mut s, name, [x, -1.0, 0.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Base", "name": "Mount"}));
    pin(&mut s, "revolute", "Pivot", "Base", "Arm", Vec3::ZERO, Vec3::Z);
    pin(&mut s, "revolute", "Other pivot", "Base", "Other", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Ground", "point": [100, 0, 0]}, "b": {"occurrence": "Other", "point": [110, 0, 0]}, "flip": true, "name": "Wrong"}),
    );
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": framed("Ground", [0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            "b": framed("Arm", [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), "flip": true, "name": "Lock"}),
    );
    // The broken loop is reported (on whichever of its joints closes it), the arm's is not.
    let names: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert!(names.len() == 1 && ["Wrong", "Other pivot"].contains(&names[0]), "{r}");
    assert!(near(place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)), Vec3::new(0.0, 10.0, 0.0)), "{:?}", place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)));
}

/// Two links writing one value: whichever carries a driven dial's turn into the loop, the turn
/// is followed (here dial A turns gear B, which turns the rocker, before dial D rewrites B).
#[test]
fn a_turn_carried_in_by_any_link_is_followed() {
    let (mut s, _, _) = four_bar();
    for (name, x) in [("A", 80.0), ("B", 100.0), ("D", 120.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
        pin(&mut s, "revolute", name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "A", "b": "B", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "B", "b": "Rocker", "ratio": 0.5}));
    run(&mut s, "joint.motion_link", json!({"a": "D", "b": "B", "ratio": 1}));
    let r = run(&mut s, "joint.drive", json!({"joint": "A", "value": 720}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
}

/// Several links writing one value: every value a link chain starts from can move to close the
/// loop, whichever of them the link order lets through (here dial A's, before dial B rewrites
/// the gear).
#[test]
fn a_loop_value_moves_through_whichever_link_carries_it() {
    let (mut s, _, q0) = four_bar();
    for (name, x) in [("A", 80.0), ("G", 100.0), ("B", 120.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
        pin(&mut s, "revolute", name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "A", "b": "G", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "G", "b": "Rocker", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "B", "b": "G", "ratio": 1}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 60.0, q0)), "{:?}", place(&s, "Rocker", q0));
}

/// Two values of one joint can feed two independent loops: a puck's x drives a pair of parallel
/// slides, its y a slide whose rigid lock can't close. Driving x moves the healthy pair.
#[test]
fn values_of_one_joint_feed_independent_loops() {
    let mut s = Session::default();
    for (name, y) in [("Ground", -10.0), ("Puck", -20.0), ("Car", 50.0), ("Other", 100.0)] {
        part(&mut s, name, [-1.0, y, 0.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    let flat = |o: &str| framed(o, [0.0, -20.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    run(&mut s, "joint.create", json!({"type": "planar", "a": flat("Ground"), "b": flat("Puck"), "flip": true, "name": "Table"}));
    pin(&mut s, "slider", "Rail", "Ground", "Car", Vec3::new(0.0, 50.0, 0.0), Vec3::X);
    pin(&mut s, "slider", "Guide", "Ground", "Car", Vec3::new(10.0, 50.0, 0.0), Vec3::X);
    pin(&mut s, "slider", "Lift", "Ground", "Other", Vec3::new(0.0, 100.0, 0.0), Vec3::Y);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Ground", "point": [0, 100, 0]}, "b": {"occurrence": "Other", "point": [10, 100, 0]}, "flip": true, "name": "Wrong"}),
    );
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 0, "b": "Rail", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 1, "b": "Lift", "ratio": 1}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Table", "values": [10]}));
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    let names: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, ["Wrong"], "{r}");
    assert!(near(place(&s, "Car", Vec3::new(0.0, 50.0, 0.0)), Vec3::new(10.0, 50.0, 0.0)), "{:?}", place(&s, "Car", Vec3::new(0.0, 50.0, 0.0)));
}

/// Two loops can move different values of one joint: each keeps what it moved. A table's x
/// feeds one pair of parallel slides and its y another; a study drives both pairs.
#[test]
fn loops_moving_different_values_of_one_joint_keep_both() {
    let mut s = Session::default();
    for (name, y) in [("Ground", -10.0), ("Puck", -20.0), ("CarA", 50.0), ("CarB", 100.0)] {
        part(&mut s, name, [-1.0, y, 0.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    let flat = |o: &str| framed(o, [0.0, -20.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    run(&mut s, "joint.create", json!({"type": "planar", "a": flat("Ground"), "b": flat("Puck"), "flip": true, "name": "Table"}));
    for (car, y) in [("CarA", 50.0), ("CarB", 100.0)] {
        pin(&mut s, "slider", &format!("{car} rail"), "Ground", car, Vec3::new(0.0, y, 0.0), Vec3::X);
        pin(&mut s, "slider", &format!("{car} guide"), "Ground", car, Vec3::new(10.0, y, 0.0), Vec3::X);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 0, "b": "CarA rail", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 1, "b": "CarB rail", "ratio": 1}));
    let keys = json!([{"joint": "CarA guide", "points": [[0, 0], [10, 10]]}, {"joint": "CarB guide", "points": [[0, 0], [10, 20]]}]);
    run(&mut s, "motion.study", json!({"name": "Both", "steps": 10, "keys": keys}));
    let r = run(&mut s, "motion.play", json!({"study": "Both", "step": 10}));
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    let table = s.doc.assembly.joints.iter().find(|j| j.name == "Table").map(|j| j.values.clone()).unwrap_or_default();
    assert!(table.len() == 3 && (table[0] - 10.0).abs() < 1e-6 && (table[1] - 20.0).abs() < 1e-6, "{table:?}");
    // A solve with nothing driven leaves them where they are.
    run(&mut s, "joint.solve", json!({}));
    assert!(near(place(&s, "CarA", Vec3::new(0.0, 50.0, 0.0)), Vec3::new(10.0, 50.0, 0.0)), "{:?}", place(&s, "CarA", Vec3::new(0.0, 50.0, 0.0)));
    assert!(near(place(&s, "CarB", Vec3::new(0.0, 100.0, 0.0)), Vec3::new(20.0, 100.0, 0.0)), "{:?}", place(&s, "CarB", Vec3::new(0.0, 100.0, 0.0)));
}

/// A clamped link can jump: a 100:1 link to a rocker limited to a full turn reaches its limit
/// after the dial's first few degrees, back where it started. Each step is checked, so the drive
/// still stops where the rocker can't go.
#[test]
fn a_clamped_link_is_followed_step_by_step() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.limits", json!({"joint": "Rocker", "min": 0, "max": 360}));
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Rocker", "ratio": 100}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Each occurrence's degrees of freedom as `joint.list` reports them.
fn dof_of(s: &mut Session, component: &str) -> u64 {
    let o = occ(s, component);
    let l = run(s, "joint.list", json!({}));
    l["dof"].as_array().into_iter().flatten().find(|d| d["occurrence"] == o).and_then(|d| d["dof"].as_u64()).unwrap_or(99)
}

/// Loops take degrees of freedom away: a four-bar's moving parts have one between them (its
/// mobility), a part hinged to the coupler one more, a slider-crank one, a parallelogram one, and a
/// four-bar locked rigid none.
#[test]
fn loops_report_their_mobility() {
    let (mut s, p0, q0) = four_bar();
    for (part, want) in [("Ground", 0), ("Crank", 1), ("Coupler", 1), ("Rocker", 1)] {
        assert_eq!(dof_of(&mut s, part), want, "four-bar {part}");
    }
    // Still one after a drive moved it.
    run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 70}));
    assert_eq!(dof_of(&mut s, "Coupler"), 1);
    // A flag hinged to the coupler half-way along: one more.
    let mid = (p0 + q0) * 0.5;
    part(&mut s, "Flag", [mid.x, mid.y, 5.0]);
    run(&mut s, "joint.as_built", json!({"type": "revolute", "a": "Coupler", "b": "Flag", "name": "Flag hinge"}));
    assert_eq!(dof_of(&mut s, "Flag"), 2);
    // Still two when a rotor elsewhere turns the flag's hinge through a motion link.
    part(&mut s, "Rotor", [100.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Rotor", "Ground", "Rotor", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Rotor", "b": "Flag hinge", "ratio": 1}));
    assert_eq!(dof_of(&mut s, "Flag"), 2);
    // A rotor locked by a loop of its own leaves the flag only the mechanism's one.
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Rotor", "name": "Rotor lock"}));
    assert_eq!(dof_of(&mut s, "Flag"), 1);
    // Locked: nothing moves.
    let (mut s, _, _) = four_bar();
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Rocker", "name": "Lock"}));
    for part in ["Crank", "Coupler", "Rocker"] {
        assert_eq!(dof_of(&mut s, part), 0, "locked {part}");
    }
    // A rotor whose turn a motion link carries into the locked crank can't turn either.
    part(&mut s, "Rotor", [100.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Rotor", "Ground", "Rotor", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Rotor", "b": "Crank", "ratio": 1}));
    assert_eq!(dof_of(&mut s, "Rotor"), 0);
    let (mut s, _, _) = slider_crank();
    for part in ["Crank", "Rod", "Piston"] {
        assert_eq!(dof_of(&mut s, part), 1, "slider-crank {part}");
    }
    // A parallelogram (crank and rocker 20, ground and coupler 45), away from its folded poses.
    let mut s = Session::default();
    let (o2, o4) = (Vec3::ZERO, Vec3::new(45.0, 0.0, 0.0));
    let (p, q) = (Vec3::new(12.0, 16.0, 0.0), Vec3::new(57.0, 16.0, 0.0));
    for (name, c) in [("Ground", o2), ("Crank", p), ("Coupler", (p + q) * 0.5), ("Rocker", q)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o2, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Coupler", p, Vec3::Z);
    pin(&mut s, "revolute", "Rocker pin", "Coupler", "Rocker", q, Vec3::Z);
    pin(&mut s, "revolute", "Rocker", "Ground", "Rocker", o4, Vec3::Z);
    for part in ["Crank", "Coupler", "Rocker"] {
        assert_eq!(dof_of(&mut s, part), 1, "parallelogram {part}");
    }
}

/// An actuator in line with the path of its tip pin, as modelled: a panel hinged at the origin,
/// its tip pin at (30, 0) moving along Y, and a barrel and rod along Y from a root pin at
/// (30, -40). There the rod only extends and the barrel's turn is zero to first order, but the
/// barrel turns as soon as the panel does: it has one degree of freedom, at that pose too.
#[test]
fn a_part_at_a_dead_point_of_its_loop_keeps_its_mobility() {
    let mut s = Session::default();
    let (tip, root) = (Vec3::new(30.0, 0.0, 0.0), Vec3::new(30.0, -40.0, 0.0));
    for (name, c) in [("Ground", Vec3::new(0.0, -40.0, 0.0)), ("Panel", tip), ("Barrel", root), ("Rod", (tip + root) * 0.5)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Hinge", "Ground", "Panel", Vec3::ZERO, Vec3::Z);
    pin(&mut s, "revolute", "Root pin", "Ground", "Barrel", root, Vec3::Z);
    pin(&mut s, "slider", "Stroke", "Barrel", "Rod", root, Vec3::Y);
    pin(&mut s, "revolute", "Tip pin", "Rod", "Panel", tip, Vec3::Z);
    super::solve_counts::take();
    s.refresh();
    assert_eq!(super::solve_counts::take().1, 0, "refresh must not repeat the singular-pose analysis");
    for part in ["Panel", "Barrel", "Rod"] {
        assert_eq!(dof_of(&mut s, part), 1, "dead point: {part}");
    }
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 5}));
    for part in ["Panel", "Barrel", "Rod"] {
        assert_eq!(dof_of(&mut s, part), 1, "driven: {part}");
    }
}

/// A design file path in a fresh temporary folder (removed by the caller).
fn temp_design(tag: &str, ext: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-joint-visibility-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("design.{ext}")).to_string_lossy().to_string();
    (dir, path)
}

/// Two hinge leaves joined by a revolute "Hinge" and a rigid "Glue" to a third part.
fn hinged() -> Session {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    part(&mut s, "Tag", [60.0, 0.0, 0.0]);
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
    pin(&mut s, "rigid", "Glue", "LeafA", "Tag", Vec3::new(61.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    s
}

fn joint(s: &Session, name: &str) -> u64 {
    s.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.id).unwrap()
}

/// #1160: hiding a joint hides only its glyph. It is saved with the design and is an undo step
/// of its own (#2219, like other visibility), the hinge still drives its leaf while hidden, and
/// the folder hides and shows every joint at once.
#[test]
fn hidden_joints_still_drive_and_are_saved_with_the_design() {
    let (dir, path) = temp_design("saved", "json");
    let mut s = hinged();
    let (hinge, glue) = (joint(&s, "Hinge"), joint(&s, "Glue"));
    run(&mut s, "file.save", json!({"path": path}));
    let undo = s.undo.len();
    let r = run(&mut s, "browser.visibility", json!({"joints": ["Hinge"], "visible": false}));
    assert_eq!(r["changed"], 1, "{r}");
    assert!(!s.joint_visible(hinge) && s.joint_visible(glue));
    assert_eq!(s.undo.len(), undo + 1, "hiding is one undo step");
    assert!(s.is_dirty(), "hiding leaves unsaved changes");
    let l = run(&mut s, "joint.list", json!({}));
    assert_eq!(l["joints"].as_array().unwrap().iter().map(|j| j["visible"].clone()).collect::<Vec<_>>(), [json!(false), json!(true)]);

    // Hidden, the hinge still constrains and moves its leaf; undoing the drive keeps it hidden.
    let p = Vec3::new(20.0, 0.0, 0.0);
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)), "{:?}", place(&s, "LeafB", p));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "LeafB", p), p));
    assert!(!s.joint_visible(hinge), "undo keeps the hiding");
    run(&mut s, "edit.redo", json!({}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));

    // Saved and opened again, the hinge stays hidden.
    run(&mut s, "file.save", json!({}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let (hinge, glue) = (joint(&s, "Hinge"), joint(&s, "Glue"));
    assert!(!s.joint_visible(hinge) && s.joint_visible(glue), "{:?}", s.visibility);
    assert!(!s.is_dirty());
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["visibility"]["hidden_joints"], json!([hinge]));

    // The folder: all hidden, then all shown (and saved without any hidden joints).
    run(&mut s, "browser.visibility", json!({"folder": "joints", "visible": false}));
    assert!(!s.joint_visible(hinge) && !s.joint_visible(glue));
    run(&mut s, "browser.visibility", json!({"folder": "joints", "visible": true}));
    assert!(s.joint_visible(hinge) && s.joint_visible(glue));
    // By name among other items, too.
    run(&mut s, "browser.visibility", json!({"items": ["Glue"], "visible": false}));
    assert!(!s.joint_visible(glue));
    run(&mut s, "browser.visibility", json!({"items": ["Glue"], "visible": true}));
    run(&mut s, "file.save", json!({}));
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(saved.get("visibility").is_none_or(|v| v.get("hidden_joints").is_none()), "{saved}");

    // Bad requests change nothing.
    let before = s.visibility.clone();
    for p in [
        json!({"joints": ["Nope"], "visible": false}),
        json!({"joints": [], "visible": false}),
        json!({"joints": "Hinge", "visible": false}),
        json!({"joints": ["Hinge"], "folder": "joints", "visible": false}),
        json!({"joints": ["Hinge"], "items": ["Glue"], "visible": false}),
    ] {
        assert!(s.execute("browser.visibility", &p).is_err(), "{p}");
    }
    assert_eq!(s.visibility, before);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #3892: a large nested STEP assembly hides thousands of joints, and the browser asks about
/// every joint of the design on every frame: the answer must not walk the hidden-joint list,
/// so the work must not grow with the number of hidden joints. The hidden joint stays hidden,
/// the shown one stays shown, and undo and redo keep the hiding right.
#[test]
fn hidden_joint_lookups_do_not_walk_the_hidden_joint_list() {
    use crate::occurrences::visibility_work;

    let mut s = hinged();
    let (hinge, glue) = (joint(&s, "Hinge"), joint(&s, "Glue"));

    // The hidden joints of an imported assembly: thousands of ids (#3892).
    let hide_many = |s: &mut Session, n: u64| {
        s.visibility.hidden_joints.clear();
        for id in 0..n {
            s.visibility.hidden_joints.push(id);
        }
    };
    // The entries one lookup walked, for a joint that is not hidden.
    let walked_by = |s: &Session, id: u64| {
        visibility_work::take();
        let visible = s.joint_visible(id);
        let walked = visibility_work::take();
        assert!(visible, "joint {id} is not hidden here");
        walked
    };

    hide_many(&mut s, 10);
    let few = walked_by(&s, 9_000_001);
    hide_many(&mut s, 10_000);
    let many = walked_by(&s, 9_000_001);
    assert!(
        many <= few + 4,
        "a joint lookup walked {many} entries of a 10,000-entry hidden-joint list and {few} of a \
         10-entry one: the work must not grow with the number of hidden joints"
    );

    // The behaviour is unchanged: the hidden joint is hidden, the shown one is shown, and undo
    // and redo keep the hiding right.
    s.visibility.hidden_joints.clear();
    run(&mut s, "browser.visibility", json!({"joints": ["Hinge"], "visible": false}));
    assert!(!s.joint_visible(hinge) && s.joint_visible(glue));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.joint_visible(hinge), "undo shows the hinge again");
    run(&mut s, "edit.redo", json!({}));
    assert!(!s.joint_visible(hinge), "redo hides it again");
}

/// The cost of a frame's worth of joint questions, with few and with many hidden joints: the
/// manual measurement behind #3892. Run it with `--ignored --nocapture`.
#[test]
#[ignore = "manual scaling measurement"]
fn joint_visibility_lookup_cost_by_hidden_joint_count() {
    use crate::occurrences::visibility_work;

    let joints = 1_000u64;
    for hidden in [0u64, 10, 100, 1_000, 10_000] {
        let mut s = Session::default();
        for id in 0..hidden {
            s.visibility.hidden_joints.push(id);
        }
        let mut times = Vec::new();
        for _ in 0..11 {
            let start = std::time::Instant::now();
            for id in 0..joints {
                let _ = s.joint_visible(id + hidden + 1);
            }
            times.push(start.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(f64::total_cmp);
        // The index is built once per change of the view, apart from the answers themselves.
        println!(
            "hidden_joints={hidden:>6} questions={joints} median_ms={:.3} min_ms={:.3} max_ms={:.3} built_entries={}",
            times[times.len() / 2],
            times[0],
            times[times.len() - 1],
            visibility_work::built_take()
        );
    }
}

/// #1160: a design saved before joints could be hidden (format 19, no `hidden_joints`) opens
/// with every joint shown; and a new joint that takes a deleted hidden joint's id starts shown.
#[test]
fn older_designs_show_every_joint_and_new_joints_start_shown() {
    let (dir, path) = temp_design("legacy", "json");
    let mut s = hinged();
    let mut legacy: Value = serde_json::from_slice(&solidcraft_io::write_design(&s.doc)).unwrap();
    legacy["format"] = json!("solidcraft/19");
    legacy["visibility"] =
        json!({"hidden_bodies": [], "hidden_occurrences": [], "hidden_origin": ["XY"], "hidden_sketches": [], "shown_sketches": []});
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!(s.doc.assembly.joints.len(), 2);
    assert!(s.doc.assembly.joints.iter().all(|j| s.joint_visible(j.id)));
    assert_eq!(s.visibility.hidden_origin, ["XY"]);

    let glue = joint(&s, "Glue");
    run(&mut s, "browser.visibility", json!({"joints": [glue], "visible": false}));
    run(&mut s, "joint.delete", json!({"joint": glue}));
    pin(&mut s, "rigid", "Glue2", "LeafA", "Tag", Vec3::new(61.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let again = joint(&s, "Glue2");
    assert!(s.joint_visible(again), "id {again} reused from {glue}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Dials R0…R4 on revolute joints to ground, linked R0 → R1 → … → `last` (ratio 1), the links
/// stored from the end of the chain to its start.
fn reverse_link_chain(s: &mut Session, last: &str) {
    for k in 0..5 {
        let x = 100.0 + 10.0 * f64::from(k);
        part(s, &format!("R{k}"), [x, 0.0, -5.0]);
        pin(s, "revolute", &format!("R{k}"), "Ground", &format!("R{k}"), Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    for k in (0..5).rev() {
        let b = if k == 4 { last.to_string() } else { format!("R{}", k + 1) };
        run(s, "joint.motion_link", json!({"a": format!("R{k}"), "b": b, "ratio": 1}));
    }
}

/// A motion link chain carries a value through all its links, however they are stored: five
/// links stored from the end still turn the last joint with the first.
#[test]
fn a_link_chain_stored_backwards_carries_its_value_through() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Last", [200.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Last", "Ground", "Last", Vec3::new(200.0, 0.0, 0.0), Vec3::Z);
    reverse_link_chain(&mut s, "Last");
    run(&mut s, "joint.drive", json!({"joint": "R0", "value": 10}));
    let l = run(&mut s, "joint.list", json!({}));
    let last = l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == "Last").map(|j| j["values"][0]["value"].clone());
    assert!((last.and_then(|v| v.as_f64()).unwrap_or(f64::NAN) - 10.0).abs() < 1e-9, "{l}");
}

/// Driving the start of such a chain into a four-bar's crank turns the mechanism, and never
/// reports its loop as opened.
#[test]
fn a_link_chain_stored_backwards_drives_a_loop() {
    let (mut s, _, q0) = four_bar();
    reverse_link_chain(&mut s, "Crank");
    for v in [1.0, 30.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "R0", "value": v}));
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{v}: {r}");
        assert!(r["stopped_by_loop"].is_null(), "{v}: {r}");
        assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, v, q0)), "{v}: {:?}", place(&s, "Rocker", q0));
    }
}

/// A turret on a pedestal joined to nothing grounded (Rover v7's turret): the solver places
/// neither, so driving the turret moves nothing. The drive, a motion study and joint.list say
/// so, naming the pedestal, instead of answering as if it worked.
#[test]
fn a_turret_on_a_floating_pedestal_says_it_moves_nothing() {
    let mut s = Session::default();
    for (name, corner) in [("Chassis", [0.0, 0.0, 0.0]), ("Arm", [10.0, 0.0, 0.0]), ("Pedestal", [50.0, 0.0, 0.0]), ("Turret", [50.0, 0.0, 2.0])] {
        part(&mut s, name, corner);
    }
    let g = occ(&s, "Chassis");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Shoulder", "Chassis", "Arm", Vec3::new(10.0, 1.0, 1.0), Vec3::Y);
    pin(&mut s, "revolute", "Azimuth", "Pedestal", "Turret", Vec3::new(51.0, 1.0, 2.0), Vec3::Z);
    let corner = Vec3::new(52.0, 0.0, 2.0);
    let start = place(&s, "Turret", corner);
    let r = run(&mut s, "joint.drive", json!({"joint": "Azimuth", "value": 30, "quiet": true}));
    assert_eq!((&r["moved"], &r["unconnected"]), (&json!(false), &json!("Pedestal:1")), "{r}");
    assert!(r["reason"].as_str().is_some_and(|w| w.starts_with("Pedestal:1 is not connected to anything grounded")), "{r}");
    assert!(r.get("dof").is_none() && r.get("fixed").is_none(), "quiet: {r}");
    // The value is set and answered in degrees, as asked; the turret stays where it was.
    assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9 && r["values"][0]["unit"] == "deg", "{r}");
    assert!(near(place(&s, "Turret", corner), start));
    let l = run(&mut s, "joint.list", json!({}));
    assert_eq!(l["floating"], json!([{"occurrences": ["Pedestal:1", "Turret:1"], "joints": ["Azimuth"]}]), "{l}");
    // A study keying both joints moves the arm and says the turret can't move.
    let keys = json!([{"joint": "Azimuth", "points": [[0, 0], [10, 90]]}, {"joint": "Shoulder", "points": [[0, 0], [10, 45]]}]);
    run(&mut s, "motion.study", json!({"name": "Scan", "steps": 10, "keys": keys}));
    let r = run(&mut s, "motion.play", json!({"study": "Scan", "step": 10, "quiet": true}));
    assert_eq!(r["moved"], true, "{r}");
    let unmoved = r["unmoved"].as_array().cloned().unwrap_or_default();
    assert!(unmoved.len() == 1 && unmoved[0]["name"] == "Azimuth" && unmoved[0]["reason"].as_str().is_some_and(|w| w.contains("Pedestal:1")), "{r}");
    // Quiet: only the arm, which moved, has its pose in the answer.
    assert_eq!(r["positions"].as_array().map(|p| p.iter().map(|x| x["occurrence"].clone()).collect::<Vec<_>>()), Some(vec![json!("Arm:1")]), "{r}");
    // Fixed to the chassis, the pedestal is placed and the turret turns.
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Chassis", "b": "Pedestal", "name": "Mount"}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Azimuth", "value": 60}));
    assert!(r["moved"] == true && r.get("reason").is_none() && r.get("unconnected").is_none() && r["dof"].is_array(), "{r}");
    assert!(!near(place(&s, "Turret", corner), start));
    assert_eq!(run(&mut s, "joint.list", json!({}))["floating"], json!([]));
}

/// `quiet: true` leaves the per-occurrence lists out of joint answers, and `moved` says whether
/// a drive moved anything: driving a joint to where it is moves nothing, with no reason given.
#[test]
fn quiet_joint_answers_and_a_drive_to_where_it_is() {
    let mut s = two_cubes();
    let r =
        run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail", "quiet": true}));
    assert!(r.get("dof").is_none() && r.get("fixed").is_none() && r["conflicts"].is_array(), "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5, "quiet": true}));
    assert!(r.get("dof").is_none() && r["moved"] == true, "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5}));
    assert!(r["dof"].is_array() && r["moved"] == false && r.get("reason").is_none(), "{r}");
    // A suppressed joint with no motion link moves nothing, and says why.
    run(&mut s, "joint.edit", json!({"joint": "Rail", "suppressed": true}));
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 8}));
    assert!(r["moved"] == false && r["reason"] == "Rail is suppressed" && r.get("unconnected").is_none(), "{r}");
    // Nor does it store the value, as it did with an undo step: unsuppressed, the part jumped to
    // it (#9742). The reply keeps what was asked apart from what the joint holds.
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && s.undo.len() == undo, "a drive of a suppressed joint changed the design");
    assert_eq!((&r["requested"][0]["value"], &r["values"][0]["value"]), (&json!(8.0), &json!(5.0)), "{r}");
    assert!(r.get("held").is_none(), "{r}");
}

/// A screw's travel follows its turn, so a distance asked of it is not reached: the reply says
/// what was asked and why the joint holds another value, and a drive of the distance alone,
/// which moves nothing, says why. A limit is named the same way (#9742).
#[test]
fn a_drive_says_which_values_a_motion_link_or_a_limit_held() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Cy"}));
    run(&mut s, "joint.screw", json!({"joint": "Cy", "pitch": 2}));
    let follows = json!("distance follows its angle through a motion link (screw)");
    let r = run(&mut s, "joint.drive", json!({"joint": "Cy", "values": [90, 5]}));
    assert!(r["moved"] == true && r.get("reason").is_none(), "{r}");
    assert_eq!((&r["requested"][0]["value"], &r["requested"][1]["value"]), (&json!(90.0), &json!(5.0)), "{r}");
    let held = r["held"].as_array().cloned().unwrap_or_default();
    assert_eq!(held.len(), 1, "only the distance is held: {r}");
    assert_eq!(
        (&held[0]["name"], &held[0]["requested"], &held[0]["unit"], &held[0]["why"]),
        (&json!("distance"), &json!(5.0), &json!("mm"), &follows),
        "{r}"
    );
    assert!((held[0]["value"].as_f64().unwrap_or(f64::NAN) - 0.5).abs() < 1e-9, "a quarter turn at pitch 2 travels 0.5 mm: {r}");
    // The distance alone: nothing moves, and the reply says why.
    let r = run(&mut s, "joint.drive", json!({"joint": "Cy", "index": 1, "value": 4}));
    assert_eq!((&r["moved"], &r["reason"], &r["held"][0]["requested"]), (&json!(false), &follows, &json!(4.0)), "{r}");

    let mut s = two_cubes();
    run(&mut s, "joint.as_built", json!({"type": "slider", "a": "A", "b": "B", "name": "Rail", "limits": [[0, 5]]}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 20}));
    assert!(r["moved"] == true && r.get("reason").is_none(), "{r}");
    assert_eq!(
        r["held"],
        json!([{"name": "distance", "requested": 20.0, "value": 5.0, "unit": "mm", "why": "distance is held to its limits, 0 to 5 mm"}]),
        "{r}"
    );
    // Within the limits, nothing is held.
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    assert!(r.get("held").is_none() && r["requested"][0]["value"] == 3.0, "{r}");
}

/// An as-built joint takes a name and limits like any other joint.
#[test]
fn as_built_takes_a_name_and_limits() {
    let mut s = two_cubes();
    let r = run(&mut s, "joint.as_built", json!({"type": "slider", "a": "A", "b": "B", "name": "Rail", "limits": [[0, 5]]}));
    assert_eq!(r["name"], "Rail", "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 20}));
    assert_eq!((&r["values"][0]["value"], &r["values"][0]["limits"]), (&json!(5.0), &json!([0.0, 5.0])), "{r}");
    assert!(near(place(&s, "B", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 5.0)), "{:?}", place(&s, "B", Vec3::new(20.0, 0.0, 0.0)));
}

/// `value` drives a joint's first value only: the others keep theirs, as with a shorter `values`.
#[test]
fn a_single_value_keeps_the_joints_other_values() {
    let mut s = two_cubes();
    run(&mut s, "joint.as_built", json!({"type": "cylindrical", "a": "A", "b": "B", "name": "Spindle"}));
    run(&mut s, "joint.drive", json!({"joint": "Spindle", "values": [30, 5]}));
    let at = |r: &Value| [0, 1].map(|i| r["values"][i]["value"].as_f64().unwrap_or(f64::NAN));
    let r = run(&mut s, "joint.drive", json!({"joint": "Spindle", "value": 45}));
    assert!((at(&r)[0] - 45.0).abs() < 1e-9 && (at(&r)[1] - 5.0).abs() < 1e-9, "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Spindle", "values": [60]}));
    assert!((at(&r)[0] - 60.0).abs() < 1e-9 && (at(&r)[1] - 5.0).abs() < 1e-9, "{r}");
}

/// A motion link's offset is in the linked value's units, like joint values and limits: degrees
/// for an angle, mm for a distance, or an expression. `joint.list` answers it the same way, it is
/// stored in radians as before, and a saved link opens with the offset it had.
#[test]
fn a_link_offset_is_in_the_linked_values_units() {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Dial", 80.0), ("Gear", 100.0), ("Car", 120.0), ("Cart", 140.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    for (kind, name, x) in [("revolute", "Dial", 80.0), ("revolute", "Gear", 100.0), ("slider", "Car", 120.0), ("slider", "Cart", 140.0)] {
        pin(&mut s, kind, name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Gear", "ratio": 1, "offset": 10}));
    run(&mut s, "joint.motion_link", json!({"a": "Car", "b": "Cart", "ratio": 1, "offset": "5 mm"}));
    run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 20}));
    run(&mut s, "joint.drive", json!({"joint": "Car", "value": 3}));
    let value = |l: &Value, name: &str| {
        l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == name).and_then(|j| j["values"][0]["value"].as_f64()).unwrap_or(f64::NAN)
    };
    let l = run(&mut s, "joint.list", json!({}));
    assert!((value(&l, "Gear") - 30.0).abs() < 1e-9 && (value(&l, "Cart") - 8.0).abs() < 1e-9, "{l}");
    let links: Vec<(f64, &str)> =
        l["links"].as_array().into_iter().flatten().map(|k| (k["offset"].as_f64().unwrap_or(f64::NAN), k["unit"].as_str().unwrap_or(""))).collect();
    assert!(
        links.len() == 2 && (links[0].0 - 10.0).abs() < 1e-9 && links[0].1 == "deg" && (links[1].0 - 5.0).abs() < 1e-9 && links[1].1 == "mm",
        "{l}"
    );
    assert!((s.doc.assembly.links[0].offset - 10f64.to_radians()).abs() < 1e-12);
    // A design saved before offsets took degrees stored 0.1 rad, and opens turning Gear 5.73° past Dial.
    s.doc_mut().assembly.links[0].offset = 0.1;
    let (dir, path) = temp_design("link-offset", "solidcraft");
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let l = run(&mut s, "joint.list", json!({}));
    assert!((value(&l, "Gear") - value(&l, "Dial") - 0.1f64.to_degrees()).abs() < 1e-9, "{l}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link between an angle and a distance takes its ratio in b's units per a's, like its offset:
/// a pinion moves its rack 0.5 mm per degree, and a car turns its dial 2 degrees per mm.
/// `joint.list` answers the ratios the same way; they are stored per radian, as before.
#[test]
fn a_link_ratio_between_an_angle_and_a_distance_is_per_degree() {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Pinion", 80.0), ("Rack", 100.0), ("Car", 120.0), ("Dial", 140.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    for (kind, name, x) in [("revolute", "Pinion", 80.0), ("slider", "Rack", 100.0), ("slider", "Car", 120.0), ("revolute", "Dial", 140.0)] {
        pin(&mut s, kind, name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Pinion", "b": "Rack", "ratio": 0.5}));
    run(&mut s, "joint.motion_link", json!({"a": "Car", "b": "Dial", "ratio": 2}));
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 20}));
    run(&mut s, "joint.drive", json!({"joint": "Car", "value": 3}));
    let l = run(&mut s, "joint.list", json!({}));
    let value = |name: &str| {
        l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == name).and_then(|j| j["values"][0]["value"].as_f64()).unwrap_or(f64::NAN)
    };
    assert!((value("Rack") - 10.0).abs() < 1e-9 && (value("Dial") - 6.0).abs() < 1e-9, "{l}");
    let ratios: Vec<f64> = l["links"].as_array().into_iter().flatten().map(|k| k["ratio"].as_f64().unwrap_or(f64::NAN)).collect();
    assert!(ratios.len() == 2 && (ratios[0] - 0.5).abs() < 1e-9 && (ratios[1] - 2.0).abs() < 1e-9, "{l}");
    let stored: Vec<f64> = s.doc.assembly.links.iter().map(|k| k.ratio).collect();
    assert!((stored[0] - 0.5f64 / 1f64.to_radians()).abs() < 1e-9 && (stored[1] - 2f64.to_radians()).abs() < 1e-12, "{stored:?}");
}

/// A new joint starts at its values inside its limits. Limits that leave out 0 (or the values
/// given) start it at the nearer limit, which it keeps, and the answer says the part moved there.
#[test]
fn a_new_joint_starts_inside_its_limits_and_says_so() {
    let mut s = two_cubes();
    let r = run(&mut s, "joint.as_built", json!({"type": "slider", "a": "A", "b": "B", "name": "Rail", "limits": [[2, 5]]}));
    let w = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        w.len() == 1 && w[0].as_str().is_some_and(|w| w.starts_with("Rail distance starts at 2 mm, inside its limits [2, 5], not at 0 mm")),
        "{r}"
    );
    assert!(near(place(&s, "B", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 2.0)), "{:?}", place(&s, "B", Vec3::new(20.0, 0.0, 0.0)));
    assert_eq!(s.doc.assembly.joints[0].values, [2.0]);
    // Values given outside the limits start at the nearer one too; angles are answered in degrees.
    let mut s = two_cubes();
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": [90], "limits": [[0, 60]], "name": "Hinge"}),
    );
    let w = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(w.len() == 1 && w[0].as_str().is_some_and(|w| w.contains("starts at 60 deg") && w.contains("not at 90 deg")), "{r}");
    // Inside the limits, nothing to say.
    let mut s = two_cubes();
    let r = run(&mut s, "joint.as_built", json!({"type": "cylindrical", "a": "A", "b": "B", "name": "Spindle", "limits": [[-30, 30], [0, 5]]}));
    assert!(r.get("warnings").is_none(), "{r}");
}

/// The trace's points as world points.
fn traced(r: &Value) -> Vec<Vec3> {
    r["points"].as_array().into_iter().flatten().filter_map(crate::params::vec3).collect()
}

/// A point rigidly carried from segment (a0, b0) to segment (a, b) in the XY plane.
fn carried(p0: Vec3, a0: Vec3, b0: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let turn = (b.y - a.y).atan2(b.x - a.x) - (b0.y - a0.y).atan2(b0.x - a0.x);
    let d = p0 - a0;
    a + Vec3::new(d.x * turn.cos() - d.y * turn.sin(), d.x * turn.sin() + d.y * turn.cos(), d.z)
}

/// A point on the four-bar's coupler, 10 mm off the line between its pins, traces the coupler
/// curve: the crank pin on its circle, the rocker pin where the coupler and rocker circles meet
/// (on the open branch), and the point carried with the pins. A motion study turning the crank
/// traces the same curve, and the design is left as it was.
#[test]
fn four_bar_coupler_point_traces_its_closed_form_curve() {
    let (mut s, p0, q0) = four_bar();
    let d = (q0 - p0) * (1.0 / q0.dist(p0));
    let c0 = (p0 + q0) * 0.5 + Vec3::new(-d.y, d.x, 0.0) * 10.0;
    let before = s.doc.clone();
    let p = json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "joint": "Crank", "from": 0, "to": 360, "samples": 73});
    let r = run(&mut s, "motion.trace", p);
    assert_eq!(*s.doc, *before, "a trace leaves the design as it was");
    assert!(r["stopped_at"].is_null() && r["samples"] == 73, "{r}");
    let pts = traced(&r);
    assert_eq!(pts.len(), 73);
    let mut q = q0;
    for (k, got) in pts.iter().enumerate() {
        let v = 5.0 * k as f64;
        if k > 0 {
            q = rocker_pin(v - 5.0, v, q);
        }
        let th = (90.0 + v).to_radians();
        let want = carried(c0, p0, q0, Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0), q);
        assert!(near(*got, want), "{v}°: {got:?} wants {want:?}");
    }
    assert!(pts.first().zip(pts.last()).is_some_and(|(a, b)| near(*a, *b)), "a full turn closes the curve");
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 72, "keys": [{"joint": "Crank", "points": [[0, 0], [72, 360]]}]}));
    let r = run(&mut s, "motion.trace", json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "study": "Turn"}));
    let from_study = traced(&r);
    assert_eq!(from_study.len(), 73, "{r}");
    assert!(from_study.iter().zip(&pts).all(|(a, b)| near(*a, *b)), "{r}");
}

fn kept_trace_fit(s: &Session, id: u64) -> Vec<Vec3> {
    s.doc.sketch(id).unwrap().wires.first().unwrap().fit.clone()
}

#[test]
fn chebyshev_demo_regenerates_its_analytic_trajectory_without_resampling_playback() {
    let mut s = Session::default();
    s.run_script(&serde_json::from_str(include_str!("../../../../examples/chebyshev-motion-trace.json")).unwrap()).unwrap();
    let trace = s.doc.assembly.traces.first().unwrap().clone();
    let original = kept_trace_fit(&s, trace.sketch);
    assert_eq!(original.len(), 145);
    assert_eq!(s.doc.assembly.joints.len(), 4);
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    // Independent circle-intersection construction: P is the crank end, Q the
    // coupler/rocker pin, and the tracer extends the coupler by another PQ.
    let mut q = Vec3::new(40.0, 50.0, 0.0);
    for (i, got) in original.iter().enumerate() {
        let theta = (90.0 + 2.5 * i as f64).to_radians();
        let p = Vec3::new(20.0 * theta.cos(), 20.0 * theta.sin(), 0.0);
        let [a, b] = meet(p, 50.0, Vec3::new(40.0, 0.0, 0.0), 50.0);
        q = if a.dist(q) < b.dist(q) { a } else { b };
        let want = q * 2.0 - p + Vec3::Z * 10.0;
        assert!(near(*got, want), "sample {i}: {got:?} wants {want:?}");
    }
    let count = s.motion_traces.recomputations;
    for angle in [30, 90, 180, 270, 0] {
        let result = run(&mut s, "joint.drive", json!({"joint":"Crank drive","value":angle,"quiet":true}));
        assert!(result["stopped_by_loop"].is_null(), "{result}");
    }
    assert_eq!(s.motion_traces.recomputations, count);
    assert_eq!(kept_trace_fit(&s, trace.sketch), original);

    run(&mut s, "parameters.change", json!({"name":"coupler_length","expression":"55 mm"}));
    let updated = kept_trace_fit(&s, trace.sketch);
    let [q, _] = meet(Vec3::new(0.0, 20.0, 0.0), 55.0, Vec3::new(40.0, 0.0, 0.0), 50.0);
    assert!(near(updated[0], q * 2.0 - Vec3::new(0.0, 20.0, 0.0) + Vec3::Z * 10.0));
    assert_ne!(updated, original);
    assert_eq!(s.doc.assembly.traces[0].wire, trace.wire);
    assert_eq!(s.motion_traces.recomputations, count + 1, "a dimension edit resamples once");
    assert!(s.model.result(trace.sketch).unwrap().warning.is_none());

    run(&mut s, "parameters.change", json!({"changes":{"coupler_length":"2.5 * unit_length","unit_length":"24 mm"}}));
    let scaled = kept_trace_fit(&s, trace.sketch);
    assert!(scaled.iter().zip(&original).all(|(a, b)| near(*a, Vec3::new(b.x * 1.2, b.y * 1.2, 10.0))));
    run(&mut s, "parameters.change", json!({"name":"trace_sweep","expression":"180 deg"}));
    let half = kept_trace_fit(&s, trace.sketch);
    assert!(near(half[0], scaled[0]) && near(*half.last().unwrap(), scaled[72]));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
}

#[test]
fn associative_trace_reuses_pose_and_unrelated_edits_but_follows_drive_expressions() {
    let (mut s, p0, q0) = four_bar();
    let mid = (p0 + q0) * 0.5;
    run(&mut s, "parameters.add", json!({"name":"swing","expression":"90 deg"}));
    let r = run(
        &mut s,
        "motion.trace.keep",
        json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],
        "joint":"Crank","from":0,"to":"swing","samples":19}),
    );
    let id = r["sketch"].as_u64().unwrap();
    let original = kept_trace_fit(&s, id);
    let count = s.motion_traces.recomputations;
    for angle in [20, 45, 75, 10] {
        run(&mut s, "joint.drive", json!({"joint":"Crank","value":angle,"quiet":true}));
        assert_eq!(kept_trace_fit(&s, id), original);
    }
    run(&mut s, "parameters.add", json!({"name":"unrelated","expression":"123 mm"}));
    run(&mut s, "parameters.change", json!({"name":"unrelated","expression":"456 mm"}));
    assert_eq!(s.motion_traces.recomputations, count, "pose and unrelated edits must not resample");
    let mut preview = s.scratch();
    run(&mut preview, "joint.drive", json!({"joint":"Crank","value":80,"quiet":true}));
    assert_eq!(preview.motion_traces.recomputations, count, "scratch previews reuse the same valid inputs");
    run(&mut s, "parameters.change", json!({"name":"swing","expression":"180 deg"}));
    let updated = kept_trace_fit(&s, id);
    assert_ne!(updated, original);
    assert_eq!(s.motion_traces.recomputations, count + 1, "a parameter command may refresh twice; sample once");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(kept_trace_fit(&s, id), original);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(kept_trace_fit(&s, id), updated);
    assert_eq!(s.motion_traces.recomputations, count + 1, "undo and redo reuse the retained input results");
    run(&mut s, "parameters.rename", json!({"name":"swing","new_name":"travel"}));
    assert_eq!(s.motion_traces.recomputations, count + 1, "renaming a parameter leaves resolved inputs unchanged");
    let count = s.motion_traces.recomputations;
    let before = s.doc.clone();
    for invalid in [
        json!({"sketch":id,"samples":501}),
        json!({"sketch":id,"index":99}),
        json!({"sketch":id,"from":"missing_parameter"}),
        json!({"sketch":id,"joint":"Missing","from":0,"to":90}),
    ] {
        assert!(s.execute("motion.trace.edit", &invalid).is_err());
        assert_eq!(*s.doc, *before, "invalid trace edits must be atomic");
    }
    run(&mut s, "motion.trace.edit", json!({"sketch":id,"samples":19}));
    assert_eq!(s.motion_traces.recomputations, count, "an identical definition is reusable");
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc), "an identical edit must not refresh the model or add an undo step");
    run(&mut s, "motion.trace.edit", json!({"sketch":id,"auto_update":false}));
    run(&mut s, "parameters.change", json!({"name":"travel","expression":"270 deg"}));
    assert_eq!(kept_trace_fit(&s, id), updated, "detaching preserves the curve");
}

#[test]
fn associative_trace_follows_a_study_and_survives_serialization() {
    let (mut s, p0, q0) = four_bar();
    let mid = (p0 + q0) * 0.5;
    let study = run(
        &mut s,
        "motion.study",
        json!({"name":"Turn","steps":18,
        "keys":[{"joint":"Crank","points":[[0,0],[18,90]]}]}),
    );
    let r = run(&mut s, "motion.trace.keep", json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],"study":"Turn"}));
    let id = r["sketch"].as_u64().unwrap();
    let original = kept_trace_fit(&s, id);
    run(
        &mut s,
        "motion.study",
        json!({"study":study["study"],"name":"Turn","steps":36,
        "keys":[{"joint":"Crank","points":[[0,0],[36,180]]}]}),
    );
    assert_eq!(kept_trace_fit(&s, id).len(), 37);
    assert_ne!(kept_trace_fit(&s, id), original);
    let doc: solidcraft_doc::Document = serde_json::from_value(serde_json::to_value(&*s.doc).unwrap()).unwrap();
    let mut opened = Session::new(doc);
    run(&mut opened, "motion.trace.edit", json!({"sketch":id,"samples":25}));
    assert_eq!(kept_trace_fit(&opened, id).len(), 25);
    run(&mut opened, "motion.delete", json!({"study":"Turn"}));
    assert!(run(&mut opened, "motion.list", json!({}))["traces"][0]["error"].is_string());
    assert!(opened.model.result(id).unwrap().warning.as_ref().unwrap().contains("last successful"));
    let count = opened.motion_traces.recomputations;
    opened.refresh();
    assert_eq!(opened.motion_traces.recomputations, count, "a missing dependency must not retry on every refresh");
    run(&mut opened, "edit.undo", json!({}));
    assert!(run(&mut opened, "motion.list", json!({}))["traces"][0]["error"].is_null());
    assert!(opened.model.result(id).unwrap().warning.is_none());
}

#[test]
fn associative_trace_follows_a_resized_picked_face_and_keeps_downstream_paths_current() {
    let mut s = two_cubes();
    pin(&mut s, "revolute", "Pivot", "A", "B", Vec3::new(20.0, 0.0, 0.0), Vec3::Z);
    let r = run(
        &mut s,
        "motion.trace.keep",
        json!({"occurrence":"B","point":[25,5,10],
        "point_ref":{"occurrence":"B","face":[25,5,10]},"joint":"Pivot","from":0,"to":90,"samples":2}),
    );
    let id = r["sketch"].as_u64().unwrap();
    let original = kept_trace_fit(&s, id);
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "solid.pipe", json!({"path_sketch":id,"path":r["curves"],"diameter":1,"operation":"new","body_name":"TracePipe"}));
    let pipe_before = s.model.state().body("TracePipe").unwrap().outline_bounds();
    let comp = s.doc.find_component("B").unwrap();
    let height =
        s.doc.features.iter().find(|f| f.component == comp && matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. })).unwrap().param_names[2]
            .clone();
    let count = s.motion_traces.recomputations;
    run(&mut s, "parameters.change", json!({"name":height,"expression":"15 mm"}));
    let updated = kept_trace_fit(&s, id);
    assert!(updated.iter().all(|p| (p.z - 15.0).abs() < 1e-6));
    assert_ne!(original, updated);
    assert_eq!(s.motion_traces.recomputations, count + 1);
    let model = s.model.state();
    assert_eq!(model.sketch(id).unwrap().sketch.wires.first().unwrap().fit, updated);
    let pipe_after = model.body("TracePipe").unwrap().outline_bounds();
    assert!((pipe_after.min.z - pipe_before.min.z - 5.0).abs() < 1e-6);
}

#[test]
fn associative_trace_ignores_another_mechanism_on_the_same_ground() {
    let (mut s, p0, q0) = four_bar();
    part(&mut s, "Other", [100.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Other pivot", "Ground", "Other", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    let mid = (p0 + q0) * 0.5;
    run(
        &mut s,
        "motion.trace.keep",
        json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],
        "joint":"Crank","from":0,"to":180,"samples":19}),
    );
    let count = s.motion_traces.recomputations;
    run(&mut s, "joint.limits", json!({"joint":"Other pivot","min":-20,"max":30}));
    assert_eq!(s.motion_traces.recomputations, count);
}

/// The middle of a slider-crank's rod: halfway between the crank pin on its circle and the wrist
/// pin at a·cos θ + √(b² − (a·sin θ)²).
#[test]
fn slider_crank_rod_traces_its_closed_form_curve() {
    let (mut s, p0, s0) = slider_crank();
    let m0 = (p0 + s0) * 0.5;
    let r =
        run(&mut s, "motion.trace", json!({"occurrence": "Rod", "point": [m0.x, m0.y, m0.z], "joint": "Crank", "from": 0, "to": 360, "samples": 37}));
    let pts = traced(&r);
    assert_eq!(pts.len(), 37, "{r}");
    for (k, got) in pts.iter().enumerate() {
        let th = (90.0 + 10.0 * k as f64).to_radians();
        let x = 15.0 * th.cos() + (40.0f64.powi(2) - (15.0 * th.sin()).powi(2)).sqrt();
        let want = (Vec3::new(15.0 * th.cos(), 15.0 * th.sin(), 0.0) + Vec3::new(x, 0.0, 0.0)) * 0.5;
        assert!(near(*got, want), "{}°: {got:?} wants {want:?}", 10 * k);
    }
}

/// Driving the rocker past its swing stops where the loop can't close: the trace ends there.
#[test]
fn a_trace_ends_where_the_loop_stops_the_motion() {
    let (mut s, _, q0) = four_bar();
    let start = q0.y.atan2(q0.x - 45.0).to_degrees();
    let r = run(
        &mut s,
        "motion.trace",
        json!({"occurrence": "Rocker", "point": [q0.x, q0.y, q0.z], "joint": "Rocker", "from": 0, "to": 100, "samples": 21}),
    );
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    let k = r["stopped_at"].as_u64().unwrap_or(u64::MAX) as usize;
    let pts = traced(&r);
    assert!(k > 0 && k < 20 && pts.len() == k + 1 && r["samples"] == k + 1, "{r}");
    let angle = |p: &Vec3| p.y.atan2(p.x - 45.0).to_degrees();
    for (i, p) in pts.iter().take(k).enumerate() {
        assert!((angle(p) - (start + 5.0 * i as f64)).abs() < 1e-6, "sample {i}: {}", angle(p));
    }
    let limit = ((900.0f64 - 3625.0) / 3600.0).acos().to_degrees();
    let last = pts.last().map(angle).unwrap_or(f64::NAN);
    assert!(last > limit - 5.0 && last <= limit + 1e-9, "stopped at {last}°, the limit is {limit}°");
}

/// Keep makes a 3D sketch spline through the trace, in one undo step.
#[test]
fn a_kept_trace_is_a_3d_sketch_spline() {
    let (mut s, p0, q0) = four_bar();
    let c0 = (p0 + q0) * 0.5;
    let p = json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "joint": "Crank", "from": 0, "to": 180, "samples": 19});
    let pts = traced(&run(&mut s, "motion.trace", p.clone()));
    let features = s.doc.features.len();
    let r = run(&mut s, "motion.trace.keep", p);
    assert_eq!((r["name"].as_str(), r["points"].as_u64()), (Some("Motion Trace1"), Some(19)), "{r}");
    assert!(s.active_sketch.is_none(), "the sketch is left");
    let id = r["sketch"].as_u64().unwrap_or(0);
    let sk = s.doc.sketch(id).cloned().unwrap_or_default();
    assert!(sk.view.three_d && sk.wires.len() == 1, "{r}");
    let fit = sk.wires.first().map(|w| w.fit.clone()).unwrap_or_default();
    assert!(fit.len() == pts.len() && fit.iter().zip(&pts).all(|(a, b)| a.dist(*b) < 1e-9), "{fit:?}");
    let again = json!({"occurrence": "Coupler", "point": [0, 0, 0], "joint": "Crank", "from": 0, "to": 90});
    assert_eq!(run(&mut s, "motion.trace.keep", again)["name"], "Motion Trace2");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.features.len(), features);
    // A point on the ground does not move: there is nothing to keep.
    let e = s.execute("motion.trace.keep", &json!({"occurrence": "Ground", "point": [0, 0, 0], "joint": "Crank", "from": 0, "to": 90})).unwrap_err();
    assert!(e.to_string().contains("does not move"), "{e}");
}

/// A full turn kept closes exactly on its start, also when the sample before the end lies a hair
/// from it (a study keyed to 359.99999° one step before 360°).
#[test]
fn a_kept_full_turn_closes_on_its_start() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    part(&mut s, "Arm", [0.0, -1.0, 0.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Pivot", "Ground", "Arm", Vec3::ZERO, Vec3::Z);
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 5, "keys": [{"joint": "Pivot", "points": [[0, 0], [4, 359.99999], [5, 360]]}]}));
    let r = run(&mut s, "motion.trace.keep", json!({"occurrence": "Arm", "point": [1, 0, 0], "study": "Turn"}));
    let id = r["sketch"].as_u64().unwrap_or(0);
    let fit = s.doc.sketch(id).ok().and_then(|sk| sk.wires.first().map(|w| w.fit.clone())).unwrap_or_default();
    assert_eq!(fit.len(), 5, "{fit:?}");
    assert_eq!(fit.first(), fit.last(), "{fit:?}");
}

/// Kept while a component is active and shown moved (a move not captured yet): the spline goes
/// in the root, where no joint moves it, through the traced points; the component stays active.
#[test]
fn a_trace_kept_in_a_moved_component_stays_on_the_path() {
    let (mut s, p0, q0) = four_bar();
    let crank = occ(&s, "Crank");
    let c0 = (p0 + q0) * 0.5;
    let p = json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "joint": "Crank", "from": 0, "to": 90, "samples": 4});
    let pts = traced(&run(&mut s, "motion.trace", p.clone()));
    run(&mut s, "component.activate", json!({"component": "Crank", "occurrence": crank, "path": [crank]}));
    run(&mut s, "occurrence.move", json!({"occurrence": crank, "translate": [0, 0, 100], "capture": false}));
    let active = (s.active_component, s.active_occurrence_path.clone());
    let r = run(&mut s, "motion.trace.keep", p);
    assert_eq!((s.active_component, s.active_occurrence_path.clone()), active);
    let id = r["sketch"].as_u64().unwrap_or(0);
    assert_eq!(s.doc.feature(id).map(|f| f.component), Some(0), "{r}");
    let fit = s.doc.sketch(id).ok().and_then(|sk| sk.wires.first().map(|w| w.fit.clone())).unwrap_or_default();
    assert!(fit.len() == pts.len() && fit.iter().zip(&pts).all(|(a, b)| near(*a, *b)), "{fit:?} wants {pts:?}");
}

/// A part inside a sub-assembly placed twice: the occurrence path picks the instance traced. The
/// second Arm turns about its pivot and its Tip goes round; the first Arm, and so the Tip at its
/// first placement, stays put.
#[test]
fn a_trace_follows_the_instance_its_path_names() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let arm = run(&mut s, "component.create", json!({"name": "Arm"}));
    run(&mut s, "component.create", json!({"name": "Tip"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [10, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"component": arm["component"], "translate": [0, 50, 0]}))["occurrence"].as_u64().unwrap_or(0);
    let at = |o: Value| json!({"occurrence": o, "point": [0, 50, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "a": at(json!(g)), "b": at(json!(second)), "flip": true, "name": "Pivot"}));
    let tip = occ(&s, "Tip");
    let trace = |s: &mut Session, o: Value| {
        traced(&run(s, "motion.trace", json!({"occurrence": o, "point": [11, 51, 1], "joint": "Pivot", "from": 0, "to": 90, "samples": 3})))
    };
    let turned = trace(&mut s, json!([second, tip]));
    let pivot = Vec3::new(0.0, 50.0, 1.0);
    assert!(turned.iter().all(|p| (p.dist(pivot) - 122f64.sqrt()).abs() < 1e-6), "{turned:?}");
    assert!(turned.first().zip(turned.last()).is_some_and(|(a, b)| (a.dist(*b) - 244f64.sqrt()).abs() < 1e-6), "a quarter turn: {turned:?}");
    let first = trace(&mut s, json!(tip));
    assert!(first.iter().all(|p| p.dist(Vec3::new(11.0, 51.0, 1.0)) < 1e-9), "{first:?}");
    assert!(s.execute("motion.trace", &json!({"occurrence": [tip, second], "point": [0, 0, 0], "joint": "Pivot", "from": 0, "to": 90})).is_err());
    assert!(s.execute("motion.trace", &json!({"occurrence": vec![second; 65], "point": [0, 0, 0], "joint": "Pivot", "from": 0, "to": 90})).is_err());
}

/// Trace params come from scripts and the control channel: bad ones are refused, never a panic.
#[test]
fn hostile_trace_params_are_refused() {
    let (mut s, _, _) = four_bar();
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 4, "keys": [{"joint": "Crank", "points": [[0, 0], [4, 90]]}]}));
    let base = json!({"occurrence": "Coupler", "point": [0, 0, 0], "joint": "Crank", "from": 0, "to": 90});
    let with = |k: &str, v: Value| {
        let mut p = base.clone();
        p[k] = v;
        p
    };
    let without = |k: &str| {
        let mut p = base.clone();
        p.as_object_mut().map(|o| o.remove(k));
        p
    };
    for p in [
        with("samples", json!(0)),
        with("samples", json!(1)),
        with("samples", json!(501)),
        with("samples", json!(1e300)),
        with("samples", json!(-3)),
        with("samples", json!("many")),
        with("point", json!([f64::MAX, 0, 0])),
        with("point", json!([0, 0])),
        with("point", json!("here")),
        without("point"),
        with("occurrence", json!("Nowhere")),
        without("occurrence"),
        with("joint", json!("Nowhere")),
        with("index", json!(1)),
        with("index", json!(-1)),
        with("from", json!("nonsense +")),
        with("to", json!(1e300)),
        without("from"),
        without("to"),
        with("study", json!("Turn")),
        with("study", json!(null)),
        without("joint"),
    ] {
        for cmd in ["motion.trace", "motion.trace.keep"] {
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        }
    }
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Crank", "name": "Lock"}));
    assert!(s.execute("motion.trace", &with("joint", json!("Lock"))).is_err(), "a rigid joint does not move");
}

#[path = "joints_coupling_tests.rs"]
mod coupling;

#[path = "joints_cam_tests.rs"]
mod cam;

// Plots and interference of motion studies, on the four-bar and slider-crank above.
#[path = "motion_analysis_tests.rs"]
mod motion_analysis;

// Joints swept until parts touch: a door, the four-bar above and a Watt six-bar.
#[path = "motion_sweep_tests.rs"]
mod motion_sweep;

/// A pin through a hole of its own radius whose axis is a hair off the hole's (parts placed from
/// rounded coordinates): the walls are all but tangent along two lines, which the boolean can't
/// intersect. Each offset gets a checked answer: clear on the axis and within the kernel's
/// same-surface tolerance, and otherwise the crescent the pin shares with the plate, against
/// the textbook lens of two circles. The pin is chamfered, so it is not a plain prism.
#[test]
fn interference_of_a_pin_a_hair_off_its_hole() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    for offset in [0.0, 1e-6, 5.9e-4, 1e-2] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [-10, -10, 0], "body_name": "Plate"}));
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [0, 0, -2], "operation": "cut", "targets": ["Plate"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [offset, 0, -2], "operation": "new", "body_name": "Pin"}));
        run(&mut s, "solid.chamfer", json!({"edges": [[offset + 3.0, 0, 7]], "distance": 0.5, "body": "Pin"}));
        let i = run(&mut s, "inspect.interference", json!({"bodies": ["Plate", "Pin"]}));
        assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{offset}: {i}");
        if offset < 1e-5 {
            assert_eq!(i["count"], 0, "{offset}: {i}");
            continue;
        }
        assert_eq!(i["count"], 1, "{offset}: {i}");
        let want = 5.0 * crescent(3.0, offset);
        let got = i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((got - want).abs() < 1e-6 * want, "{offset}: {got} against {want}: {i}");
        assert!((i["interferences"][0]["near_coaxial"]["offset_mm"].as_f64().unwrap_or(0.0) - offset).abs() < 1e-9, "{i}");
    }
}

/// The pin's head has the radius of the bush the pin sits in, on nearly its axis but beside it:
/// two walls of one radius that never meet along the axis don't stop the crescent from counting.
#[test]
fn interference_of_a_headed_pin_a_hair_off_a_bush_of_its_heads_radius() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    let offset = 5.9e-4;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": 10, "base": [0, 0, 0], "operation": "new", "body_name": "Bush"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 14, "base": [0, 0, -2], "operation": "cut", "targets": ["Bush"]}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 8, "base": [offset, 0, -3], "operation": "new", "body_name": "Pin"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": 2, "base": [offset, 0, -5], "operation": "join", "targets": ["Pin"]}));
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["Bush", "Pin"]}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
    assert_eq!(i["count"], 1, "{i}");
    let want = 5.0 * crescent(3.0, offset);
    let got = i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
    assert!((got - want).abs() < 1e-6 * want, "{got} against {want}: {i}");
    assert!(i["interferences"][0]["near_coaxial"].is_object(), "{i}");
}

/// A headed pin made before the plate it sits in, a hair off the plate's hole: the crescent
/// whichever body comes first.
#[test]
fn interference_of_a_headed_pin_made_before_its_plate() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    let offset = 5.6e-4;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [offset, 0, -2.35], "operation": "new", "body_name": "Pin"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": 3, "base": [offset, 0, -5.35], "operation": "join", "targets": ["Pin"]}));
    run(&mut s, "solid.box", json!({"length": 60, "width": 60, "height": 2, "corner": [-30, -30, -2.35], "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [0, 0, -6], "operation": "cut", "targets": ["Plate"]}));
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["Pin", "Plate"]}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
    assert_eq!(i["count"], 1, "{i}");
    let want = 2.0 * crescent(3.0, offset);
    let got = i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
    assert!((got - want).abs() < 1e-6 * want, "{got} against {want}: {i}");
}

/// A grounded Base (a 40 x 40 x 10 box), a Pin extruded 20 mm from the sketch circle `c1` of
/// `PinSketch` (r5 at x = 100) and a Cap box; a revolute Hinge from the Base's top to the Pin's
/// top rim, its offset the parameter `gap`, and a rigid Fix between two points of Base and Cap.
fn hinge_and_fix() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "2 mm"}));
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]}));
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Pin"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PinSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cap"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 0, 0]}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "Hinge", "offset": "gap", "limits": [[-90, 90]],
               "a": {"occurrence": "Base", "face": [20, 20, 10]}, "b": {"occurrence": "Pin", "circle": [105, 0, 20]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Fix", "a": {"occurrence": "Base", "point": [0, 0, 0]}, "b": {"occurrence": "Cap", "point": [200, 0, 0]}}),
    );
    s
}

/// The reference of `role` on side `side` of an inspected joint.
fn reference<'a>(j: &'a Value, side: &str, role: &str) -> &'a Value {
    j[side]["references"].as_array().and_then(|r| r.iter().find(|r| r["role"] == role)).unwrap_or(&Value::Null)
}

/// `joint.inspect` gives each joint's definition: its kind, offset (and the expression it
/// follows), limits and values, and for each side the occurrence and component, the snap as
/// saved, and what its face names refer to: a circle's wall back to the extrude and the sketch
/// curve it was swept from.
#[test]
fn joint_inspect_gives_what_each_joint_is_attached_to() {
    let mut s = hinge_and_fix();
    let all = run(&mut s, "joint.inspect", json!({}));
    let joints = all["joints"].as_array().cloned().unwrap_or_default();
    assert_eq!(joints.iter().map(|j| j["name"].clone()).collect::<Vec<_>>(), [json!("Hinge"), json!("Fix")], "{all}");

    let hinge = run(&mut s, "joint.inspect", json!({"joint": "Hinge"}))["joints"][0].clone();
    assert_eq!((&hinge["type"], &hinge["offset"], &hinge["expressions"]["offset"]), (&json!("revolute"), &json!(2.0), &json!("gap")), "{hinge}");
    assert_eq!(hinge["values"][0]["limits"], json!([-90.0, 90.0]), "{hinge}");
    assert_eq!(hinge["dofs"], json!(["angle"]));
    assert_eq!((&hinge["a"]["occurrence_name"], &hinge["a"]["component_name"]), (&json!("Base:1"), &json!("Base")), "{hinge}");
    assert_eq!(hinge["a"]["path"], hinge["a"]["occurrence_name"]);
    assert_eq!((&hinge["a"]["snap"]["type"], &hinge["b"]["snap"]["type"]), (&json!("face_center"), &json!("circle_center")), "{hinge}");
    assert_eq!((&hinge["a"]["resolves"], &hinge["b"]["resolves"]), (&json!(true), &json!(true)), "{hinge}");
    assert_eq!(hinge["b"]["component_name"], "Pin");
    // The Pin's wall: the extrude's side swept from PinSketch's c1.
    let wall = reference(&hinge, "b", "face");
    assert_eq!(wall["feature"]["type"], "ExtrudeFeature", "{hinge}");
    assert_eq!(
        (&wall["sketch"]["name"], &wall["sketch"]["curve"], &wall["sketch"]["curve_type"]),
        (&json!("PinSketch"), &json!("c1"), &json!("circle")),
        "{hinge}"
    );
    assert_eq!(wall["name"].as_str().map(|n| n.contains(":side:c1")), Some(true), "{hinge}");
    // The rim's end face is the extrude's top.
    assert_eq!(reference(&hinge, "b", "end")["feature"], wall["feature"], "{hinge}");
    // The Base's top: a face of its box, no sketch.
    assert!(reference(&hinge, "a", "face")["feature"]["id"].is_u64() && reference(&hinge, "a", "face")["sketch"].is_null(), "{hinge}");

    let fix = &joints[1];
    assert_eq!((&fix["type"], &fix["values"], &fix["dofs"]), (&json!("rigid"), &json!([]), &json!([])), "{fix}");
    assert_eq!((&fix["b"]["snap"]["type"], &fix["b"]["snap"]["point"]), (&json!("point"), &json!([200.0, 0.0, 0.0])), "{fix}");
    assert_eq!(fix["b"]["references"], json!([]));
    assert!(fix.get("expressions").is_none(), "{fix}");

    // A joint that isn't there is an error.
    assert!(s.execute("joint.inspect", &json!({"joint": "Nope"})).is_err());
}

/// A joint whose sketch circle is replaced: its origin's face is gone, so it no longer
/// resolves; the inspection says why and that its curve is no longer in the sketch.
#[test]
fn joint_inspect_reports_a_lost_origin() {
    let mut s = hinge_and_fix();
    run(&mut s, "sketch.edit", json!({"sketch": "PinSketch"}));
    run(&mut s, "sketch.delete", json!({"entities": ["c1"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c2"}));
    run(&mut s, "sketch.finish", json!({}));
    let hinge = run(&mut s, "joint.inspect", json!({"joint": "Hinge"}))["joints"][0].clone();
    assert_eq!(hinge["b"]["resolves"], false, "{hinge}");
    let wall = reference(&hinge, "b", "face");
    let name = wall["name"].as_str().unwrap_or_default();
    assert!(hinge["b"]["error"].as_str().is_some_and(|e| e.contains(name)), "{hinge}");
    assert_eq!(wall["sketch"]["curve"], "c1", "{hinge}");
    assert!(wall["sketch"]["error"].is_string(), "{hinge}");
    assert!(!hinge["errors"].as_array().is_none_or(Vec::is_empty), "the solve says so too: {hinge}");
    // The Base side is still found.
    assert_eq!(hinge["a"]["resolves"], true);
}

/// An edge whose first face is a fillet's: the fillet face's name holds the edge it rounds
/// (`F2:blend:<face>|<face>`), so the edge's two faces are the whole blend name and the slot's
/// wall, not the blend name cut at its own `|`.
#[test]
fn joint_inspect_splits_an_edge_of_a_blend_face_into_its_two_faces() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 20, 10]], "radius": 3}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 4, "height": 20, "corner": [32, 18, -5], "operation": "cut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cap"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 0, 0]}));
    // On the slot's wall y = 18, halfway round the fillet (axis x = 37, z = 7, r = 3).
    let h = 3.0 * std::f64::consts::FRAC_1_SQRT_2;
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "OnBlend", "a": {"occurrence": "Base", "edge": [37.0 + h, 18, 7.0 + h]},
               "b": {"occurrence": "Cap", "face": [205, 5, 5]}}),
    );
    let j = run(&mut s, "joint.inspect", json!({"joint": "OnBlend"}))["joints"][0].clone();
    let edge = reference(&j, "a", "edge");
    let name = edge["name"].as_str().unwrap_or_default().to_string();
    let faces: Vec<String> = edge["faces"].as_array().into_iter().flatten().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
    // `F4:blend:F3:+x|F3:+z#[F3:-y,F5:-y]|F5:-y`: the blend piece beside the slot, and the slot's wall.
    assert!(name.split_once(':').is_some_and(|(_, r)| r.starts_with("blend:")), "the blend face comes first: {name}");
    assert_eq!(faces.len(), 2, "{edge}");
    assert_eq!(format!("{}|{}", faces[0], faces[1]), name, "{edge}");
    assert!(faces[0].contains(":blend:") && faces[0].contains('|'), "{edge}");
    assert!(!faces[1].contains('|'), "{edge}");
}

/// A joint on a pattern's copy of a pin: the copy's wall (`F3:side:c1@F4.1`) still names the
/// sketch curve it was swept from, `c1`.
#[test]
fn joint_inspect_finds_the_sketch_curve_of_a_copied_side() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Pins"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PinSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20, "name": "Pin"}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Pin"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cap"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 0, 0]}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "OnCopy", "a": {"occurrence": "Pins", "circle": [35, 0, 20]}, "b": {"occurrence": "Cap", "face": [205, 5, 5]}}),
    );
    let j = run(&mut s, "joint.inspect", json!({"joint": "OnCopy"}))["joints"][0].clone();
    let wall = reference(&j, "a", "face");
    assert!(wall["name"].as_str().is_some_and(|n| n.contains(":side:c1@F")), "a copy's wall: {j}");
    assert_eq!((&wall["sketch"]["curve"], &wall["sketch"]["name"]), (&json!("c1"), &json!("PinSketch")), "{j}");
    assert!(wall["sketch"]["error"].is_null(), "{j}");
}

/// `joint.inspect` reads only: no undo step, the design unchanged, and the id agents' gates
/// treat as reading (`*.inspect`).
#[test]
fn joint_inspect_reads_only() {
    let mut s = hinge_and_fix();
    let spec = crate::cmd::command_specs().into_iter().find(|c| c.id == "joint.inspect");
    assert!(spec.is_some_and(|c| !c.undoable && c.tab.is_empty()));
    let undo = run(&mut s, "document.inspect", json!({}))["undo"].clone();
    let before = serde_json::to_value(&s.doc).unwrap_or_default();
    run(&mut s, "joint.inspect", json!({}));
    run(&mut s, "joint.inspect", json!({"joint": "Fix"}));
    assert_eq!(run(&mut s, "document.inspect", json!({}))["undo"], undo);
    assert_eq!(serde_json::to_value(&s.doc).unwrap_or_default(), before);
}

/// Trial placements use exactly the same constraints as a full solve. Only the expensive DOF
/// report is optional: loop stops, errors and the resulting pose must not depend on it.
#[test]
fn placement_analysis_preserves_held_loop_constraints_and_diagnostics() {
    use solidcraft_doc::joints::{self, Analysis};
    use std::collections::BTreeMap;

    let (s, _, _) = four_bar();
    let rocker = s.doc.assembly.joints.iter().find(|j| j.name == "Rocker").unwrap().id;
    let held = BTreeMap::from([((rocker, 0), 0.0)]);
    for angle in [-10.0_f64, 40.0, 100.0] {
        let mut doc = (*s.doc).clone();
        doc.assembly.joints.iter_mut().find(|j| j.id == rocker).unwrap().values[0] = angle.to_radians();
        let full = joints::solve_holding(&doc, &s.model.state(), &held);
        let placement = joints::solve_holding_with_analysis(&doc, &s.model.state(), &held, Analysis::Placement);
        assert!(placement.dof.is_empty(), "trial placement must not compute DOF");
        assert_eq!(placement.world, full.world);
        assert_eq!(placement.values, full.values);
        assert_eq!(placement.fixed, full.fixed);
        assert_eq!(placement.conflicts, full.conflicts);
        assert_eq!(placement.errors, full.errors);
        let mut placed = doc.clone();
        placed.apply_joint_solution(&placement);
        let mut reference = doc;
        reference.apply_joint_solution(&full);
        assert_eq!(placed, reference);
        assert_eq!(joints::solve(&placed, &s.model.state()).dof, joints::solve(&reference, &s.model.state()).dof);
    }
    // A degenerate origin remains an error even in a placement-only trial.
    let mut broken = (*s.doc).clone();
    broken.assembly.joints[0].b.snap = solidcraft_doc::joints::Snap::Point { point: Vec3::ZERO, z: Some(Vec3::ZERO), x: None };
    let full = joints::solve(&broken, &s.model.state());
    let placement = joints::solve_holding_with_analysis(&broken, &s.model.state(), &BTreeMap::new(), Analysis::Placement);
    assert!(!full.errors.is_empty());
    assert_eq!(placement.errors, full.errors);
    assert_eq!(placement.conflicts, full.conflicts);
}

/// Ordinary commands publish one complete report; refresh only updates constraint placements.
/// Counts are deterministic work assertions, not timing thresholds tied to a test machine.
#[test]
fn joint_drive_and_refresh_do_not_repeat_dof_analysis() {
    let (mut s, _, _) = four_bar();
    for (angle, quiet) in [(17, false), (58, true), (105, false)] {
        super::solve_counts::take();
        let r = run(&mut s, "joint.drive", json!({"joint":"Crank", "value":angle, "quiet":quiet}));
        let (all, full) = super::solve_counts::take();
        assert!(all > full, "placement trials must still run");
        assert_eq!(full, usize::from(!quiet), "only a published DOF report needs full analysis");
        let expected = super::sol_json(&solidcraft_doc::joints::solve(&s.doc, &s.model.state()), &json!({"quiet":quiet}));
        for key in ["dof", "fixed", "errors", "conflicts"] {
            assert_eq!(r.get(key), expected.get(key), "{key}: {r}");
        }
        let before = s.doc.clone();
        s.refresh();
        assert_eq!(super::solve_counts::take().1, 0, "refresh discards the DOF report");
        assert_eq!(*s.doc, *before);
    }
}

/// A parameter change solves the joints once: the refresh that follows the command finds the
/// design and the model as the command's own solve left them (the sketch the parameter drives
/// has its new solution stored after that solve). A move the joints undo is still solved.
#[test]
fn a_parameter_change_solves_the_joints_once() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "300 mm"}));
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 100, "height": 1, "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": ["W / 2", 50, 1], "diameter": 6}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let bottom = r["curves"][0].as_str().unwrap_or_default().to_string();
    run(&mut s, "sketch.dimension", json!({"entities": [bottom], "value": "W / 10"}));
    run(&mut s, "sketch.finish", json!({}));
    let plate = occ(&s, "Plate");
    run(&mut s, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 2, "corner": [100, 200, 0]}));
    run(&mut s, "solid.hole", json!({"position": [110, 210, 2], "diameter": 6}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Plate", "circle": [153, 50, 0.8], "x": [1, 0, 0]},
               "b": {"occurrence": "Bracket", "circle": [113, 210, 0.4], "x": [1, 0, 0]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    run(&mut s, "component.activate", json!({"component": "root"}));
    for w in [400.0, 360.0] {
        super::solve_counts::take();
        run(&mut s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
        assert_eq!(super::solve_counts::take().0, 1, "W = {w}");
        let at = place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
        assert!(near(at, Vec3::new(w / 2.0, 50.0, 1.0)), "W = {w}: the bracket follows the hole, {at:?}");
    }
    // Moving the bracket changes no feature, so the model stays; the joint still takes it back.
    let bracket = occ(&s, "Bracket");
    super::solve_counts::take();
    run(&mut s, "occurrence.move", json!({"occurrence": bracket, "translate": [5, 0, 0], "capture": true}));
    assert!(super::solve_counts::take().0 > 0);
    assert!(near(place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0)), Vec3::new(180.0, 50.0, 1.0)));
}

#[test]
fn driving_one_cylindrical_coordinate_preserves_the_other_expression_and_reports_continuous() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "parameters.add", json!({"name":"turn","expression":"30 deg"}));
    let joint = run(
        &mut s,
        "joint.create",
        json!({"type":"cylindrical","a":{"occurrence":0,"point":[0,0,0]},"b":{"occurrence":"Part","point":[0,0,0]},"flip":true,"values":["turn",0]}),
    )["joint"]
        .as_u64()
        .unwrap();
    let before = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap().exprs.value(0).map(str::to_string);
    assert!(before.is_some());
    let reply = run(&mut s, "joint.drive", json!({"joint":joint,"index":1,"value":12,"continuous":true,"quiet":true}));
    assert_eq!(reply["continuous"], true);
    let j = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap();
    assert_eq!(j.exprs.value(0).map(str::to_string), before);
    assert!((j.values[0].to_degrees() - 30.).abs() < 1e-7);
    assert!((j.values[1] - 12.).abs() < 1e-7);
    run(&mut s, "parameters.change", json!({"name":"turn","expression":"40 deg"}));
    let j = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap();
    assert!((j.values[0].to_degrees() - 40.).abs() < 1e-7, "the preserved expression still drives the angle");
    assert!((j.values[1] - 12.).abs() < 1e-7);
    for bad in [json!(-1), json!(2), json!(1000000), json!("0")] {
        assert!(s.execute("joint.drive", &json!({"joint":joint,"index":bad,"value":0})).is_err());
    }
}
