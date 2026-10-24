//! Isolate's layers and the display-state value (`crate::display_state`).

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

fn isolate(s: &mut Session, keep: &[&String]) -> serde_json::Value {
    s.execute("display.isolate", &json!({"items": keep})).unwrap()
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
fn isolate_hides_the_rest_and_exit_restores_the_exact_prior_state() {
    let (mut s, n) = session();
    // Parts hidden by hand before the isolation, and a hidden sketch/origin item.
    hide(&mut s, &[&n[1], &n[2]]);
    s.visibility.hidden_origin.push("XY".into());
    let before = s.visibility.clone();
    let r = isolate(&mut s, &[&n[0], &n[1]]);
    assert_eq!(r["level"], 1);
    assert_eq!(s.isolation_depth(), 1);
    // Everything but the selection is hidden (the selected, previously hidden part shows).
    assert_eq!(hidden(&s), sorted(&n[2..]));
    assert!(s.body_visible(&n[0]) && s.body_visible(&n[1]) && !s.body_visible(&n[2]));
    s.execute("display.unisolate", &json!({})).unwrap();
    assert_eq!(s.visibility, before);
    assert_eq!(s.isolation_depth(), 0);
    assert!(!s.body_visible(&n[1]) && !s.body_visible(&n[2]) && s.body_visible(&n[3]));
}

#[test]
fn parts_hidden_by_hand_while_a_layer_is_active_belong_to_it() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[5]]);
    isolate(&mut s, &[&n[0], &n[1], &n[2]]);
    // Hide one isolated part by hand, and show one that the layer hid.
    hide(&mut s, &[&n[1]]);
    s.execute("browser.visibility", &json!({"items": [&n[3]], "visible": true})).unwrap();
    assert!(!s.body_visible(&n[1]) && s.body_visible(&n[3]));
    s.execute("display.unisolate", &json!({})).unwrap();
    // The hand-hidden part is visible again; n[3] is shown as before; n[5] stays hidden.
    assert!(s.body_visible(&n[1]));
    assert_eq!(hidden(&s), vec![n[5].clone()]);
}

#[test]
fn layers_stack_and_leave_one_at_a_time_or_all_at_once() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[7]]);
    let base = s.visibility.clone();
    assert_eq!(isolate(&mut s, &[&n[0], &n[1], &n[2], &n[3]])["level"], 1);
    let first = s.visibility.clone();
    hide(&mut s, &[&n[3]]);
    let second_before = s.visibility.clone();
    assert_eq!(isolate(&mut s, &[&n[0], &n[1]])["level"], 2);
    assert_eq!(hidden(&s), sorted(&n[2..]));
    assert_eq!(s.execute("display.unisolate", &json!({})).unwrap()["level"], 1);
    assert_eq!(s.visibility, second_before);
    assert_ne!(s.visibility, first, "the hand edit made in layer 1 is part of layer 1's state");
    assert_eq!(s.execute("display.unisolate", &json!({})).unwrap()["level"], 0);
    assert_eq!(s.visibility, base);
    // All at once, from three levels.
    isolate(&mut s, &[&n[0], &n[1], &n[2]]);
    isolate(&mut s, &[&n[0], &n[1]]);
    isolate(&mut s, &[&n[0]]);
    assert_eq!(s.isolation_depth(), 3);
    let r = s.execute("display.unisolate_all", &json!({})).unwrap();
    assert_eq!((r["left"].as_bool(), r["level"].as_u64()), (Some(true), Some(0)));
    assert_eq!(s.visibility, base);
}

#[test]
fn nothing_to_isolate_or_to_leave_changes_nothing() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[4]]);
    let before = s.visibility.clone();
    let rev = s.revision;
    // An empty selection is a no-op, not a layer that hides everything.
    assert_eq!(isolate(&mut s, &[])["isolated"], 0);
    assert_eq!(s.execute("display.isolate", &json!({"paths": []})).unwrap()["isolated"], 0);
    assert_eq!(s.isolation_depth(), 0);
    assert_eq!(s.visibility, before);
    // Leaving when not isolated is a no-op too.
    assert_eq!(s.execute("display.unisolate", &json!({})).unwrap()["left"], false);
    assert_eq!(s.execute("display.unisolate_all", &json!({})).unwrap()["left"], false);
    assert_eq!(s.visibility, before);
    assert_eq!(s.revision, rev);
    // Unknown bodies and bad input are refused and leave no layer.
    let bogus = "No such body".to_string();
    assert!(s.execute("display.isolate", &json!({"items": [&n[0], &bogus]})).is_err());
    assert!(s.execute("display.isolate", &json!({})).is_err());
    assert!(s.execute("display.isolate", &json!({"items": [&n[0]], "paths": [[1]]})).is_err());
    assert!(s.execute("display.isolate", &json!({"paths": [[999_999]]})).is_err());
    assert_eq!(s.isolation_depth(), 0);
    assert_eq!(s.visibility, before);
}

#[test]
fn the_design_and_dirty_state_use_the_state_before_the_isolation() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[5]]);
    s.mark_saved();
    assert!(!s.is_dirty());
    isolate(&mut s, &[&n[0]]);
    // Isolating is not a change to the design.
    assert!(!s.is_dirty());
    assert_eq!(s.design().visibility.hidden_bodies, vec![n[5].clone()]);
    isolate(&mut s, &[&n[0], &n[1]]);
    hide(&mut s, &[&n[0]]);
    assert!(!s.is_dirty());
    assert_eq!(s.design().visibility.hidden_bodies, vec![n[5].clone()]);
    assert_eq!(*s.saved_view(), s.isolation[0].before.visibility);
    // Saving while isolated saves the base; leaving afterwards is still clean.
    s.mark_saved();
    s.execute("display.unisolate_all", &json!({})).unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.design().visibility.hidden_bodies, vec![n[5].clone()]);
    // A design that is not isolated and has nothing hidden writes no visibility.
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    assert!(s.is_dirty());
    assert!(s.design().visibility.is_empty());
}

#[test]
fn the_layers_follow_a_renamed_body_and_are_not_copied_to_previews() {
    let (mut s, n) = session();
    hide(&mut s, &[&n[5]]);
    isolate(&mut s, &[&n[0]]);
    s.execute("body.rename", &json!({"body": &n[5], "name": "Renamed"})).unwrap();
    assert_eq!(s.isolation[0].before.visibility.hidden_bodies, vec!["Renamed".to_string()]);
    let copy = s.scratch();
    assert_eq!(copy.isolation_depth(), 0);
    s.execute("display.unisolate", &json!({})).unwrap();
    assert_eq!(s.visibility.hidden_bodies, vec!["Renamed".to_string()]);
}

#[test]
fn isolating_placements_is_a_layer_and_browser_visibility_isolate_still_works() {
    // Eight parts: two placed components.
    let mut s = Session::new(generated_assembly(8));
    let n: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    hide(&mut s, &[&n[11]]);
    let before = s.visibility.clone();
    let occs: Vec<u64> = s.doc.occurrences.iter().map(|o| o.id).collect();
    assert!(occs.len() >= 2);
    let r = s.execute("browser.visibility", &json!({"paths": [[occs[0]]], "visible": true, "isolate": true})).unwrap();
    assert_eq!(r["isolated"], json!([[occs[0]]]));
    assert_eq!(s.isolation_depth(), 1);
    assert!(s.visibility.hidden_occurrences.contains(&vec![occs[1]]));
    let after_one = s.visibility.clone();
    // The same again hides nothing more: no new layer, as that would pretend something happened.
    assert!(s.execute("display.isolate", &json!({"paths": [[occs[0]]]})).is_err());
    assert_eq!(s.isolation_depth(), 1);
    assert_eq!(s.visibility, after_one);
    s.execute("display.unisolate_all", &json!({})).unwrap();
    assert_eq!(s.visibility, before);
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
fn states_follow_renames_and_ignore_missing_bodies_and_apply_over_a_layer() {
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
    // Applied while isolated, it belongs to the layer: leaving restores what was there before.
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    let before = s.visibility.clone();
    isolate(&mut s, &[&n[0]]);
    s.execute("display.apply", &json!({"state": "S"})).unwrap();
    assert_eq!(s.isolation_depth(), 1);
    s.execute("display.unisolate", &json!({})).unwrap();
    assert_eq!(s.visibility, before);
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
