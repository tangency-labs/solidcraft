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
fn a_copied_body_keeps_the_appearance_of_the_faces_of_the_body_it_came_from() {
    let mut s = Session::default();
    cube20(&mut s, "Cube");
    run(&mut s, "appearance.assign", json!({"faces": [[10, 10, 20]], "body": "Cube", "appearance": "ABS - Red"}));
    let reply = run(&mut s, "solid.move", json!({"bodies": ["Cube"], "translate": [30, 0, 0], "copy": true}));
    let copy = created(&reply);
    let looks: Vec<_> = s.doc.appearances.faces.iter().filter(|f| f.body == copy).collect();
    assert_eq!(looks.len(), 1, "the copy must carry the face looks of the body it came from");
    assert!(
        looks[0].point.dist(Vec3::new(40.0, 10.0, 20.0)) < 1e-9,
        "the face look must follow the body to where the copy landed: {:?}",
        looks[0].point
    );
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
