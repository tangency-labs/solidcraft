//! The display-state value (`crate::display_state`) and the commands that save, apply, rename and delete states.

use serde_json::json;

use crate::large_assembly_tests::generated_assembly;
use crate::{Session, Visibility};

fn session() -> (Session, Vec<String>) {
    let s = Session::new(generated_assembly(3));
    let names: Vec<String> = s.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
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
    s.execute("display.apply", &json!({"state": "Without parts"})).unwrap();
    assert_eq!(hidden(&s), sorted(&[n[1].clone(), n[2].clone()]));
    // The camera is never touched: Named Views own it.
    s.camera = None;
    s.execute("display.apply", &json!({"state": "Display State2"})).unwrap();
    assert!(s.camera.is_none());
    // Saved with the design and read back.
    let design = s.design().into_owned();
    let back = solidcraft_io::read_design(&solidcraft_io::write_design(&design).unwrap()).unwrap();
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

/// Applying a state is one undo step, as hiding and showing are (`browser.visibility`): undo puts
/// back what was hidden and the looks together, redo applies the state again.
#[test]
fn applying_a_state_is_one_undo_step_that_restores_visibility_and_looks() {
    let (mut s, n) = session();
    s.execute("appearance.assign", &json!({"bodies": [&n[0]], "color": "#ff0000"})).unwrap();
    hide(&mut s, &[&n[1], &n[2]]);
    s.execute("display.save", &json!({"name": "Red, two hidden"})).unwrap();
    // Another look: a different colour, one other body hidden.
    s.execute("appearance.assign", &json!({"bodies": [&n[0]], "color": "#0000ff"})).unwrap();
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    hide(&mut s, &[&n[3]]);
    let (before_hidden, before_looks, before_visibility) = (hidden(&s), s.doc.appearances.clone(), s.visibility.clone());
    let steps = s.undo.len();

    s.execute("display.apply", &json!({"state": "Red, two hidden"})).unwrap();
    assert_eq!(s.undo.len(), steps + 1, "one undo step, not one per changed list");
    assert_eq!(hidden(&s), sorted(&[n[1].clone(), n[2].clone()]));
    assert_ne!(s.doc.appearances, before_looks);
    let applied = (s.visibility.clone(), s.doc.appearances.clone());

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(hidden(&s), before_hidden, "undo shows what was shown before");
    assert_eq!(s.visibility, before_visibility);
    assert_eq!(s.doc.appearances, before_looks, "and gives the bodies their old looks");
    assert_eq!(s.undo.len(), steps);

    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!((s.visibility.clone(), s.doc.appearances.clone()), applied);
}

#[test]
fn applying_a_state_that_changes_only_visibility_undoes_like_browser_visibility() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[5]]);
    s.execute("display.save", &json!({"name": "Hidden 5"})).unwrap();
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    assert!(s.visibility.hidden_bodies.is_empty());
    let steps = s.undo.len();
    s.execute("display.apply", &json!({"state": "Hidden 5"})).unwrap();
    assert_eq!(s.undo.len(), steps + 1);
    assert!(!s.body_visible(&n[5]));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.body_visible(&n[5]));
    // Applying what is already shown changes nothing and adds no step.
    s.execute("display.apply", &json!({"state": "Hidden 5"})).unwrap();
    let steps = s.undo.len();
    s.execute("display.apply", &json!({"state": "Hidden 5"})).unwrap();
    assert_eq!(s.undo.len(), steps);
}

/// Locking a body is not part of how the design looks: a state neither stores nor changes it.
#[test]
fn a_state_leaves_locked_bodies_alone() {
    let (mut s, n) = session();
    s.visibility.locked_bodies = vec![n[0].clone()];
    hide(&mut s, &[&n[1]]);
    s.execute("display.save", &json!({"name": "S"})).unwrap();
    assert!(s.doc.display_states[0].snapshot.visibility.locked_bodies.is_empty());
    assert_eq!(s.active_display_state(), Some("S"));
    s.visibility.locked_bodies = vec![n[2].clone()];
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    s.execute("display.apply", &json!({"state": "S"})).unwrap();
    assert_eq!(s.visibility.locked_bodies, vec![n[2].clone()]);
    assert_eq!(s.active_display_state(), Some("S"));
}

/// A state's looks follow a body rename, as its hidden list does.
#[test]
fn a_state_keeps_the_looks_of_a_renamed_body() {
    let (mut s, n) = session();
    s.execute("appearance.assign", &json!({"bodies": [&n[4]], "color": "#00ff00"})).unwrap();
    s.execute("display.save", &json!({"name": "Green"})).unwrap();
    s.execute("body.rename", &json!({"body": &n[4], "name": "Renamed"})).unwrap();
    let looks = s.doc.display_states[0].appearances.as_ref().unwrap();
    assert!(looks.bodies.contains_key("Renamed") && !looks.bodies.contains_key(&n[4]));
}
