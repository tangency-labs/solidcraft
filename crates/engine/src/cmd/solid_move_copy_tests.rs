//! `solid.move` with `copy: true`: the original body stays where it was, a new body is
//! pushed at the transform, and an older design file (no `copy` key at all) still loads.
use super::*;

fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap()
}

fn cube20(s: &mut Session, name: &str) {
    run(s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": name}));
}

fn volume(body: &solidcraft_doc::ModelBody) -> f64 {
    solidcraft_kernel::measure(&body.body).unwrap().volume
}

/// A box of 20 mm with its corner at `corner`, as a body of its own.
fn cube20_at(s: &mut Session, name: &str, corner: [i32; 3]) {
    run(s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": name, "corner": corner}));
}

/// The copy of `source`: the body that sits where the source landed after the move. Which
/// copy is which is read from the geometry, not from the order the bodies are listed in.
fn copy_of(s: &Session, source: &str, translate: Vec3) -> String {
    let state = s.model.state();
    let from = state.body(source).expect("the source body is in the model").mesh().bounds().min;
    let want = Vec3::new(from.x + translate.x, from.y + translate.y, from.z + translate.z);
    state
        .bodies
        .iter()
        .find(|b| b.name != source && b.mesh().bounds().min.dist(want) < 1e-6)
        .map(|b| b.name.clone())
        .expect("the move must have copied the body")
}

/// The name of the body a command made, as the command reported it.
fn created(reply: &Value) -> String {
    reply["created_bodies"].as_array().and_then(|a| a.first()).and_then(Value::as_str).expect("the command must report the body it made").to_string()
}

#[test]
fn copy_keeps_the_original_and_adds_a_transformed_body() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [30, 0, 0], "copy": true}));
    let state = s.model.state();
    assert_eq!(state.bodies.len(), 2, "a copy move must add one body, not replace the original");

    let original = state.body("Cube").unwrap();
    let original_bounds = original.mesh().bounds();
    assert!(original_bounds.min.dist(Vec3::ZERO) < 1e-6, "the original body must stay at its own position: {original_bounds:?}");
    assert!(original_bounds.max.dist(Vec3::new(20.0, 20.0, 20.0)) < 1e-6, "{original_bounds:?}");

    let copy = state.bodies.iter().find(|b| b.name != "Cube").unwrap();
    let copy_bounds = copy.mesh().bounds();
    assert!(copy_bounds.min.dist(Vec3::new(30.0, 0.0, 0.0)) < 1e-6, "the copy must land at the translated position: {copy_bounds:?}");
    assert!(copy_bounds.max.dist(Vec3::new(50.0, 20.0, 20.0)) < 1e-6, "{copy_bounds:?}");

    let original_volume = volume(original);
    let copy_volume = volume(copy);
    assert!((original_volume - 8000.0).abs() < 1e-3, "original volume: {original_volume}");
    assert!((copy_volume - 8000.0).abs() < 1e-3, "copy volume: {copy_volume}");
    assert!((original_volume - copy_volume).abs() < 1e-6, "the copy must have the same mass properties as the source");

    let total: f64 = state.bodies.iter().map(volume).sum();
    assert!((total - 16000.0).abs() < 1e-3, "total volume across both bodies: {total}");
}

#[test]
fn without_copy_the_move_still_replaces_the_body_in_place() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [30, 0, 0]}));
    let state = s.model.state();
    assert_eq!(state.bodies.len(), 1, "a plain move must not create a second body");
    let moved = state.body("Cube").unwrap();
    let bounds = moved.mesh().bounds();
    assert!(bounds.min.dist(Vec3::new(30.0, 0.0, 0.0)) < 1e-6, "{bounds:?}");
}

#[test]
fn a_copied_body_keeps_the_appearance_of_the_body_it_came_from() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "appearance.assign", json!({"bodies": ["Cube"], "appearance": "ABS - Blue"}));
    let reply = run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [30, 0, 0], "copy": true}));
    let copy = created(&reply);
    let source = s.doc.appearances.bodies.get("Cube").expect("the source body keeps its look").clone();
    assert_eq!(s.doc.appearances.bodies.get(&copy), Some(&source), "the copy must keep the appearance of the body it came from");
}

/// Editing a copying Move from the History redefines it on a scratch session, where the old Move
/// is still in the timeline as a suppressed placeholder (#7741).
#[test]
fn a_copying_move_can_be_edited_from_the_history() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "appearance.assign", json!({"bodies": ["Cube"], "appearance": "ABS - Blue"}));
    let reply = run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [30, 0, 0], "copy": true}));
    let feature = reply["feature"].clone();
    let copy = created(&reply);
    let edit = json!({"feature": feature, "command": "solid.move", "params": {"bodies": ["Cube"], "translate": [40, 0, 0], "copy": true}});
    s.execute("timeline.redefine", &edit).expect("editing a copying move must keep copying");
    assert_eq!(s.model.state().bodies.len(), 2, "the edited move still makes one copy");
    assert_eq!(copy_of(&s, "Cube", Vec3::new(40.0, 0.0, 0.0)), copy, "the copy keeps its name and moves to the new place");
    let source = s.doc.appearances.bodies.get("Cube").expect("the source body keeps its look").clone();
    assert_eq!(s.doc.appearances.bodies.get(&copy), Some(&source), "the edited copy keeps the look of the body it came from");
}

#[test]
fn a_copied_body_keeps_the_material_of_the_body_it_came_from() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "material.assign", json!({"bodies": ["Cube"], "material": "Aluminum"}));
    let reply = run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [30, 0, 0], "copy": true}));
    let copy = created(&reply);
    let source = s.doc.materials.get("Cube").expect("the source body keeps its material").clone();
    assert_eq!(s.doc.materials.get(&copy), Some(&source), "the copy must keep the material of the body it came from");
}

#[test]
fn each_copied_body_keeps_the_look_and_the_material_of_the_body_it_came_from() {
    let mut s = Session::default();
    cube20(&mut s, "Alpha");
    cube20_at(&mut s, "Beta", [50, 0, 0]);
    run(&mut s, "appearance.assign", json!({"bodies": ["Alpha"], "appearance": "ABS - Blue"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Beta"], "appearance": "ABS - Red"}));
    run(&mut s, "material.assign", json!({"bodies": ["Alpha"], "material": "Aluminum"}));
    run(&mut s, "material.assign", json!({"bodies": ["Beta"], "material": "Steel"}));

    // One move, two bodies: each copy keeps the look of the body it came from, not the look of
    // the body copied before it.
    run(&mut s, "solid.move", json!({"bodies": ["Beta", "Alpha"], "translate": [30, 0, 0], "copy": true}));
    assert_eq!(s.model.state().bodies.len(), 4, "two sources and their two copies");

    let translate = Vec3::new(30.0, 0.0, 0.0);
    let alpha = copy_of(&s, "Alpha", translate);
    let beta = copy_of(&s, "Beta", translate);
    assert_ne!(alpha, beta, "the two bodies must have two different copies");

    let alpha_look = s.doc.appearances.bodies.get("Alpha").expect("the source body keeps its look").name.clone();
    let beta_look = s.doc.appearances.bodies.get("Beta").expect("the source body keeps its look").name.clone();
    assert_ne!(alpha_look, beta_look, "the two bodies must have different looks for this to mean anything");
    assert_eq!(
        s.doc.appearances.bodies.get(&alpha).map(|l| l.name.clone()),
        Some(alpha_look.clone()),
        "the copy of Alpha must keep the look of Alpha"
    );
    assert_eq!(s.doc.appearances.bodies.get(&beta).map(|l| l.name.clone()), Some(beta_look.clone()), "the copy of Beta must keep the look of Beta");
    assert_eq!(s.doc.materials.get(&alpha).map(String::as_str), Some("Aluminum"), "the copy of Alpha must keep the material of Alpha");
    assert_eq!(s.doc.materials.get(&beta).map(String::as_str), Some("Steel"), "the copy of Beta must keep the material of Beta");
}

#[test]
fn an_older_design_with_no_copy_key_at_all_still_loads_and_moves_in_place() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [5, 0, 0]}));
    let saved = s.doc.to_json();
    assert!(!saved.contains("\"copy\""), "a move without the flag must serialize with no `copy` key, as an older file would read: {saved}");

    // Simulate a design saved before this feature existed: strip the key even if present, then
    // reopen through the same loader a real design file goes through.
    let mut old: Value = serde_json::from_str(&saved).unwrap();
    let features = old.get_mut("features").and_then(Value::as_array_mut).expect("features array");
    let mut touched = false;
    for f in features.iter_mut() {
        if f.get("type").and_then(Value::as_str) == Some("move")
            && let Some(obj) = f.as_object_mut()
        {
            obj.remove("copy");
            touched = true;
        }
    }
    assert!(touched, "the move feature must be present in the saved design");

    let migrated = solidcraft_doc::Document::from_json(&old.to_string()).unwrap();
    let reopened = Session::new(migrated);
    let state = reopened.model.state();
    assert_eq!(state.bodies.len(), 1, "a pre-copy design must still move the one body in place, never duplicate it");
    let moved = state.body("Cube").unwrap();
    assert!(moved.mesh().bounds().min.dist(Vec3::new(5.0, 0.0, 0.0)) < 1e-6);
}

fn assert_bounds(s: &Session, body: &str, min: Vec3, max: Vec3) {
    let bounds = s.world_state().body(body).unwrap().mesh().bounds();
    assert!(bounds.min.dist(min) < 1e-6 && bounds.max.dist(max) < 1e-6, "{body}: {bounds:?}, expected {min:?}..{max:?}");
}

#[test]
fn explicit_origin_rotates_in_place_and_survives_reopen() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "body_name": "B"}));
    run(&mut s, "solid.move", json!({"bodies": ["B"], "origin": ["105 mm", "5 mm", 0], "angle": "90 deg"}));
    for session in [&s, &Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap())] {
        assert_bounds(session, "B", Vec3::new(100.0, 0.0, 0.0), Vec3::new(110.0, 10.0, 10.0));
    }
}

#[test]
fn matrix_moves_a_copy_and_survives_reopen() {
    for matrix in [json!([0, -1, 0, 30, 1, 0, 0, 0, 0, 0, 1, 0]), json!([[0, -1, 0, 30], [1, 0, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]])] {
        let mut s = Session::default();
        cube20(&mut s, "Cube");
        run(&mut s, "solid.move", json!({"bodies": ["Cube"], "matrix": matrix, "copy": true, "body_name": "Copy"}));
        for session in [&s, &Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap())] {
            assert_bounds(session, "Cube", Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0));
            assert_bounds(session, "Copy", Vec3::new(10.0, 0.0, 0.0), Vec3::new(30.0, 20.0, 20.0));
        }
    }
}

#[test]
fn invalid_move_transforms_are_refused_without_changing_the_design() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    let saved = s.doc.to_json();
    let rigid = json!([1, 0, 0, 30, 0, 1, 0, 0, 0, 0, 1, 0]);
    let mut cases = vec![
        json!({"origin": [1, 2]}),
        json!({"origin": [0, true, 0]}),
        json!({"origin": ["1e12 mm", 0, 0]}),
        json!({"axis": [0, 0, 0], "angle": 90}),
        json!({"axis": "no"}),
        json!({"angle": false}),
        json!({"copy": "true"}),
        json!({"matrix": [1, 2, 3]}),
        json!({"matrix": [2, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}),
        json!({"matrix": [-1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}),
        json!({"matrix": [1, 0.1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}),
        json!({"matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0.1, 1]}),
        json!({"matrix": rigid, "tolerance": 0.1}),
        json!({"tolerance": 0.001}),
    ];
    for key in ["translate", "origin", "axis", "angle"] {
        cases.push(json!({"matrix": rigid, key: null}));
    }
    for mut p in cases {
        p["bodies"] = json!(["Cube"]);
        assert!(s.execute("solid.move", &p).is_err(), "accepted {p}");
        assert_eq!(s.doc.to_json(), saved, "changed design after rejecting {p}");
    }
}

#[test]
fn matrix_axis_angle_preserves_small_and_half_turns() {
    use solidcraft_doc::{apply_point, rigid};
    for axis in [Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(0.0, 1.0, -1.0), Vec3::new(-1.0, 2.0, -3.0)] {
        for angle in [0.0, 1e-12, 0.2, std::f64::consts::PI - 1e-8, std::f64::consts::PI, std::f64::consts::PI + 1e-8] {
            let matrix = rigid(Vec3::ZERO, Vec3::ZERO, axis, angle);
            let (a, theta) = super::super::component::matrix::axis_angle(&matrix);
            let restored = rigid(Vec3::ZERO, Vec3::ZERO, a, theta);
            for point in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!(apply_point(&matrix, point).dist(apply_point(&restored, point)) < 1e-10, "{axis:?}, {angle}");
            }
        }
    }
}

#[test]
fn origin_remains_parametric_and_undoable() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "parameters.change", json!({"name": "pivot", "expression": "10 mm"}));
    run(&mut s, "solid.move", json!({"bodies": ["Cube"], "origin": ["pivot", 10, 0], "angle": "180 deg", "name": "Turn"}));
    assert_bounds(&s, "Cube", Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0));
    run(&mut s, "parameters.change", json!({"name": "pivot", "expression": "20 mm"}));
    assert_bounds(&s, "Cube", Vec3::new(20.0, 0.0, 0.0), Vec3::new(40.0, 20.0, 20.0));
    run(&mut s, "edit.undo", json!({}));
    assert_bounds(&s, "Cube", Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0));
}

#[test]
fn matrix_translation_is_in_mm_even_in_an_inch_design() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "solid.move", json!({"bodies": ["Cube"], "matrix": [1,0,0,30, 0,1,0,0, 0,0,1,0]}));
    assert_bounds(&s, "Cube", Vec3::new(30.0, 0.0, 0.0), Vec3::new(50.0, 20.0, 20.0));
}
