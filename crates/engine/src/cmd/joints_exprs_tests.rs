//! Joints whose offset, angle or values are typed as expressions follow the parameters they name.

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use super::tests::{occ, place, run};
use crate::Session;

/// Two 10 mm cubes in components A (grounded) and B, a `gap` and a `turn` parameter.
fn cubes() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let o = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(&mut s, "parameters.add", json!({"name": "turn", "expression": "0 deg"}));
    s
}

/// Where B's bottom corner (20, 0, 0 in B) is.
fn b_corner(s: &Session) -> Vec3 {
    place(s, "B", Vec3::new(20.0, 0.0, 0.0))
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn an_offset_and_an_angle_typed_as_expressions_follow_their_parameters() {
    let mut s = cubes();
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "offset": "gap", "angle": "turn", "name": "Seat"}),
    );
    let z = |s: &Session| place(s, "B", Vec3::new(25.0, 5.0, 0.0)).z;
    assert!((z(&s) - 15.0).abs() < 1e-6, "{}", z(&s));
    // The gap changes: B rides up with it, without touching the joint.
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "12 mm"}));
    assert!((z(&s) - 22.0).abs() < 1e-6, "{}", z(&s));
    // A parameter the gap is worked out from changes too.
    run(&mut s, "parameters.add", json!({"name": "base", "expression": "2 mm"}));
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "base * 3"}));
    assert!((z(&s) - 16.0).abs() < 1e-6, "{}", z(&s));
    // The angle turns B about the seat's axis (its centre stays, its corner swings round).
    let centre = place(&s, "B", Vec3::new(25.0, 5.0, 0.0));
    let before = b_corner(&s);
    run(&mut s, "parameters.change", json!({"name": "turn", "expression": "90 deg"}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), centre));
    let after = b_corner(&s);
    assert!((before - centre).dot(after - centre).abs() < 1e-6, "{before:?} {after:?}");
    // The design keeps the expressions, so they are there after saving and opening.
    let j = s.doc.assembly.joints.first().cloned().unwrap_or_else(|| panic!("no joint"));
    assert_eq!((j.exprs.offset.as_deref(), j.exprs.angle.as_deref()), (Some("gap"), Some("turn")));
    let saved = serde_json::to_value(&j).unwrap_or_default();
    assert_eq!((&saved["exprs"]["offset"], &saved["exprs"]["angle"]), (&json!("gap"), &json!("turn")), "{saved}");
    let listed = run(&mut s, "joint.list", json!({}));
    assert_eq!(listed["joints"][0]["expressions"]["offset"], json!("gap"), "{listed}");
    // A number typed over the expression drops it: B stays when the gap changes again.
    run(&mut s, "joint.edit", json!({"joint": "Seat", "offset": 4}));
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "30 mm"}));
    assert!((z(&s) - 14.0).abs() < 1e-6, "{}", z(&s));
    assert!(s.doc.assembly.joints.first().is_some_and(|j| j.exprs.offset.is_none() && j.exprs.angle.is_some()));
}

#[test]
fn a_joint_value_typed_as_an_expression_follows_it_until_it_is_driven() {
    let mut s = cubes();
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "limits": [[0, 20]], "name": "Rail"}),
    );
    let z = |s: &Session| place(s, "B", Vec3::new(25.0, 5.0, 0.0)).z;
    assert!((z(&s) - 15.0).abs() < 1e-6, "{}", z(&s));
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "8 mm"}));
    assert!((z(&s) - 18.0).abs() < 1e-6, "{}", z(&s));
    // Past the limits it is held at them, like a typed value.
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "50 mm"}));
    assert!((z(&s) - 30.0).abs() < 1e-6, "{}", z(&s));
    // Driven by hand it drops the expression (contact or a loop may stop it short of any value).
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 1}));
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "9 mm"}));
    assert!((z(&s) - 11.0).abs() < 1e-6, "{}", z(&s));
    assert!(s.doc.assembly.joints.first().is_some_and(|j| j.exprs.is_empty()));
}

#[test]
fn renaming_a_parameter_carries_into_joints_and_deleting_it_is_refused() {
    let mut s = cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "offset": "gap + 1", "name": "Seat"}));
    run(&mut s, "parameters.rename", json!({"name": "gap", "new_name": "clearance"}));
    assert_eq!(s.doc.assembly.joints.first().and_then(|j| j.exprs.offset.clone()).as_deref(), Some("clearance + 1"));
    run(&mut s, "parameters.change", json!({"name": "clearance", "expression": "9 mm"}));
    assert!((place(&s, "B", Vec3::new(25.0, 5.0, 0.0)).z - 20.0).abs() < 1e-6);
    // The joint uses it, so it can't be deleted from under the joint.
    let e = s.execute("parameters.delete", &json!({"name": "clearance"})).unwrap_err().to_string();
    assert!(e.contains("joint Seat (offset)"), "{e}");
}

#[test]
fn a_joint_whose_expression_stops_working_out_is_left_out_with_the_reason() {
    let mut s = cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "offset": "gap", "name": "Seat"}));
    let at = b_corner(&s);
    // The gap becomes an angle: the offset no longer works out as a length. B stays where it
    // was (not thrown to an old number) and the solve says why.
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "30 deg", "unit": "deg"}));
    assert!(near(b_corner(&s), at), "{:?}", b_corner(&s));
    let r = run(&mut s, "joint.solve", json!({}));
    let why = r["errors"][0]["why"].as_str().unwrap_or_default().to_string();
    assert!(why.contains("Seat") && why.contains("offset"), "{r}");
    // Fixed again, the joint places B again.
    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "2 mm", "unit": "mm"}));
    assert!((place(&s, "B", Vec3::new(25.0, 5.0, 0.0)).z - 12.0).abs() < 1e-6);
    assert_eq!(run(&mut s, "joint.solve", json!({}))["errors"], json!([]));
}

#[test]
fn a_value_that_changes_unit_with_the_joint_type_drops_its_expression() {
    let mut s = cubes();
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}));
    // The slide becomes a turn: `gap` (a length) can't be its angle, so the turn keeps a number
    // and the joint stays in the solve.
    let r = run(&mut s, "joint.edit", json!({"joint": "Rail", "type": "revolute"}));
    assert_eq!(r["errors"], json!([]), "{r}");
    assert!(s.doc.assembly.joints.first().is_some_and(|j| j.exprs.is_empty()));
}

#[test]
fn text_that_names_no_parameter_is_kept_as_a_number() {
    let mut s = cubes();
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "offset": "3 mm", "angle": "0 deg", "values": ["2 * 2 mm"]}),
    );
    let j = s.doc.assembly.joints.first().cloned().unwrap_or_else(|| panic!("no joint"));
    assert!(j.exprs.is_empty(), "{:?}", j.exprs);
    assert!((j.offset - 3.0).abs() < 1e-12 && j.values.first().is_some_and(|v| (v - 4.0).abs() < 1e-12), "{j:?}");
}
