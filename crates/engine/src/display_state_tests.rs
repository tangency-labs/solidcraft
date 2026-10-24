//! The display-state value (`crate::display_state`) and the commands that save, apply, rename and delete states.

use serde_json::json;

use crate::large_assembly_tests::generated_assembly;
use crate::{Session, Visibility};

fn session() -> (Session, Vec<String>) {
    let s = Session::new(generated_assembly(3));
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 12);
    (s, names)
}

fn hidden(s: &Session) -> Vec<String> {
    let mut h = s.visibility.hidden_bodies.clone();
    h.sort();
    h
}

fn sorted(v: &[String]) -> Vec<String> {
    let mut v = v.to_vec();
    v.sort();
    v
}

fn hide(s: &mut Session, names: &[&String]) {
    s.execute("browser.visibility", &json!({"items": names, "visible": false})).unwrap();
}

#[test]
fn snapshot_captures_applies_and_compares() {
    let (mut s, n) = session();
    let empty = s.capture_visibility();
    assert_eq!(empty, s.capture_visibility());
    hide(&mut s, &[&n[0]]);
    let one = s.capture_visibility();
    assert_ne!(empty, one);
    s.apply_visibility(&empty);
    assert_eq!(s.visibility, Visibility::default());
    s.apply_visibility(&one);
    assert_eq!(hidden(&s), vec![n[0].clone()]);
    assert_eq!(s.capture_visibility(), one);
}

#[test]
fn a_saved_state_comes_back_and_is_saved_with_the_design() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[1], &n[2]]);
    let r = s.execute("display.save", &json!({"name": "Without parts"})).unwrap();
    assert_eq!(r["state"], "Without parts");
    s.execute("display.save", &json!({})).unwrap();
    assert_eq!(s.doc.display_states.len(), 2);
    assert_eq!(s.doc.display_states[1].name, "Display State2");
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    let undo = s.undo.len();
    s.execute("display.apply", &json!({"state": "Without parts"})).unwrap();
    assert_eq!(hidden(&s), sorted(&[n[1].clone(), n[2].clone()]));
    assert_eq!(s.undo.len(), undo, "applying is view state, not an undo step");
    // The camera is never touched: Named Views own it.
    s.camera = None;
    s.execute("display.apply", &json!({"state": "Display State2"})).unwrap();
    assert!(s.camera.is_none());
    // Saved with the design and read back.
    let design = s.design().into_owned();
    let back = solidcraft_io::read_design(&solidcraft_io::write_design(&design)).unwrap();
    assert_eq!(back.display_states, s.doc.display_states);
    let mut reopened = Session::new(back);
    reopened.execute("display.apply", &json!({"state": "Without parts"})).unwrap();
    assert!(!reopened.body_visible(&n[1]) && reopened.body_visible(&n[0]));
}

#[test]
fn display_states_are_renamed_deleted_listed_and_undone() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[3]]);
    s.execute("display.save", &json!({"name": "A"})).unwrap();
    s.execute("display.save", &json!({"name": "B"})).unwrap();
    assert!(s.execute("display.rename", &json!({"state": "A", "name": "B"})).is_err(), "names are unique");
    assert!(s.execute("display.rename", &json!({"state": "Nope", "name": "C"})).is_err());
    assert!(s.execute("display.apply", &json!({"state": "Nope"})).is_err());
    s.execute("display.rename", &json!({"state": "A", "name": "Assembly"})).unwrap();
    let names = |s: &mut Session| -> Vec<String> {
        s.execute("display.list", &json!({})).unwrap()["states"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(names(&mut s), vec!["Assembly", "B"]);
    // Saving again under a name replaces it, in place of a second one.
    s.execute("display.save", &json!({"name": "B"})).unwrap();
    assert_eq!(names(&mut s), vec!["Assembly", "B"]);
    s.execute("display.delete", &json!({"state": "Assembly"})).unwrap();
    assert_eq!(names(&mut s), vec!["B"]);
    // Saving, renaming and deleting are design edits and undo; the visibility is not touched.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(names(&mut s), vec!["Assembly", "B"]);
    assert_eq!(hidden(&s), vec![n[3].clone()]);
}

#[test]
fn states_follow_renames_and_ignore_missing_bodies() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[4]]);
    s.execute("display.save", &json!({"name": "S"})).unwrap();
    s.execute("body.rename", &json!({"body": &n[4], "name": "Renamed"})).unwrap();
    assert_eq!(s.doc.display_states[0].snapshot.visibility.hidden_bodies, vec!["Renamed".to_string()]);
    // A body that no longer exists in the state is a no-op; bodies not in it are shown.
    s.doc_mut().display_states[0].snapshot.visibility.hidden_bodies.push("Ghost".into());
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    s.execute("display.apply", &json!({"state": "S"})).unwrap();
    assert!(!s.body_visible("Renamed") && s.body_visible(&n[0]));
}

#[test]
fn a_state_keeps_the_looks_and_the_active_one_is_found() {
    let (mut s, n) = session();
    assert_eq!(s.active_display_state(), None);
    s.execute("appearance.assign", &json!({"bodies": [&n[0]], "color": "#ff0000"})).unwrap();
    s.execute("display.save", &json!({"name": "Red"})).unwrap();
    assert_eq!(s.active_display_state(), Some("Red"));
    s.execute("appearance.assign", &json!({"bodies": [&n[0]], "color": "#0000ff"})).unwrap();
    s.execute("display.save", &json!({"name": "Blue"})).unwrap();
    assert_eq!(s.active_display_state(), Some("Blue"));
    // Applying a state brings back its looks, and hand changes make no state active.
    s.execute("display.apply", &json!({"state": "Red"})).unwrap();
    assert_eq!(s.active_display_state(), Some("Red"));
    assert_eq!(s.doc.appearances.bodies[&n[0]].color, s.doc.display_states[0].appearances.as_ref().unwrap().bodies[&n[0]].color);
    hide(&mut s, &[&n[1]]);
    assert_eq!(s.active_display_state(), None);
}

#[test]
fn updating_a_state_keeps_its_place() {
    let (mut s, n) = session();
    for name in ["A", "B", "C"] {
        s.execute("display.save", &json!({ "name": name })).unwrap();
    }
    hide(&mut s, &[&n[0]]);
    s.execute("display.save", &json!({ "name": "A" })).unwrap();
    let order: Vec<&str> = s.doc.display_states.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(order, ["A", "B", "C"]);
    assert_eq!(s.doc.display_states[0].snapshot.visibility.hidden_bodies, vec![n[0].clone()]);
}
