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

/// Four boxes A to D at the root, for Isolate's layers.
fn boxes() -> Session {
    let mut s = Session::default();
    for (i, name) in ["A", "B", "C", "D"].iter().enumerate() {
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [30 * i, 0, 0], "body_name": name})).unwrap();
    }
    s
}

fn isolate(s: &mut Session, names: &[&str]) {
    s.execute("browser.visibility", &json!({"items": names, "visible": true, "isolate": true})).unwrap();
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|n| n.to_string()).collect()
}

/// Isolating again adds a layer; leaving one puts back what was shown before it, parts hidden by
/// hand while it was active included; leaving all lands in the view from before the first.
#[test]
fn isolate_stacks_layers_and_leaving_puts_each_view_back_exactly() {
    let mut s = boxes();
    hide(&mut s, &[&"D".to_string()]);
    isolate(&mut s, &["A", "B"]);
    assert_eq!((s.isolation_depth(), hidden(&s)), (1, names(&["C", "D"])));
    // Hidden by hand while the layer is active: it belongs to the layer.
    hide(&mut s, &[&"A".to_string()]);
    isolate(&mut s, &["B"]);
    assert_eq!((s.isolation_depth(), hidden(&s)), (2, names(&["A", "C", "D"])));
    // The same isolate again hides nothing more: no new layer.
    isolate(&mut s, &["B"]);
    assert_eq!(s.isolation_depth(), 2);
    let r = s.execute("display.unisolate", &json!({})).unwrap();
    assert_eq!((r["left"].as_bool(), r["level"].as_u64()), (Some(true), Some(1)));
    assert_eq!(hidden(&s), names(&["A", "C", "D"]));
    // Leaving all of them: the view from before the first isolation, D hidden by hand included.
    isolate(&mut s, &["C"]);
    assert_eq!(s.isolation_depth(), 2);
    let r = s.execute("display.unisolate_all", &json!({})).unwrap();
    assert_eq!((r["left"].as_bool(), r["level"].as_u64()), (Some(true), Some(0)));
    assert_eq!((s.isolation_depth(), hidden(&s)), (0, names(&["D"])));
    // Not isolated: both commands do nothing, and say so.
    for cmd in ["display.unisolate", "display.unisolate_all"] {
        let steps = s.undo.len();
        assert_eq!(s.execute(cmd, &json!({})).unwrap()["left"], false);
        assert_eq!((s.undo.len(), hidden(&s)), (steps, names(&["D"])));
    }
}

/// Visibility is an undo step, so the layers go back and forth with it: one undo leaves a layer
/// (or takes back a hide made in it), and one redo isolates again.
#[test]
fn undo_and_redo_carry_the_layers_with_the_view() {
    let mut s = boxes();
    hide(&mut s, &[&"D".to_string()]);
    isolate(&mut s, &["A", "B"]);
    hide(&mut s, &[&"A".to_string()]);
    isolate(&mut s, &["B"]);
    let at = |s: &Session| (s.isolation_depth(), hidden(s));
    assert_eq!(at(&s), (2, names(&["A", "C", "D"])));
    for expected in [(1, names(&["A", "C", "D"])), (1, names(&["C", "D"])), (0, names(&["D"]))] {
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(at(&s), expected);
    }
    for expected in [(1, names(&["C", "D"])), (1, names(&["A", "C", "D"])), (2, names(&["A", "C", "D"]))] {
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(at(&s), expected);
    }
    // Leaving every layer is one step too: undo brings both layers back.
    s.execute("display.unisolate_all", &json!({})).unwrap();
    assert_eq!(at(&s), (0, names(&["D"])));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(at(&s), (2, names(&["A", "C", "D"])));
    // Leaving one layer whose view equals the one it went back to still takes the layer away.
    s.execute("display.unisolate_all", &json!({})).unwrap();
    s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
    isolate(&mut s, &["A", "B", "C"]);
    s.execute("browser.visibility", &json!({"items": ["D"], "visible": true})).unwrap();
    assert_eq!(at(&s), (1, names(&[])));
    let steps = s.undo.len();
    s.execute("display.unisolate", &json!({})).unwrap();
    assert_eq!(at(&s), (0, names(&[])));
    assert_eq!(s.undo.len(), steps + 1);
}

/// The design file, the unsaved-changes test and the recovery copy hold the view from before the
/// first layer, however the layers came and went; a lock set meanwhile is kept.
#[test]
fn the_design_is_saved_as_it_was_before_the_first_layer() {
    let mut s = boxes();
    hide(&mut s, &[&"D".to_string()]);
    s.mark_saved();
    assert!(!s.is_dirty());
    isolate(&mut s, &["A", "B"]);
    isolate(&mut s, &["A"]);
    assert_eq!(hidden(&s), names(&["B", "C", "D"]));
    assert!(!s.is_dirty(), "a layer is not an unsaved change");
    assert_eq!(s.design().visibility.hidden_bodies, names(&["D"]));
    // A save runs on a scratch copy and is taken up by `adopt_save`.
    let scratch = s.scratch();
    assert_eq!(scratch.isolation_depth(), 2);
    assert_eq!(scratch.design().visibility.hidden_bodies, names(&["D"]));
    s.visibility.locked_bodies = names(&["B"]);
    assert_eq!(s.design().visibility.locked_bodies, names(&["B"]));
    s.visibility.locked_bodies.clear();
    s.mark_saved();
    assert!(!s.is_dirty());
    s.execute("display.unisolate_all", &json!({})).unwrap();
    assert!(!s.is_dirty(), "back where the file is");
    // A hide made while isolated is not saved either, whether or not it is undone.
    isolate(&mut s, &["A", "B"]);
    hide(&mut s, &[&"A".to_string()]);
    assert!(!s.is_dirty());
    assert_eq!(s.design().visibility.hidden_bodies, names(&["D"]));
}

/// A checkpoint restores the layers it had, and a rename is followed by the views they go back to.
#[test]
fn checkpoints_and_renames_follow_the_layers() {
    let mut s = boxes();
    let id = s.checkpoint();
    isolate(&mut s, &["A"]);
    assert_eq!(s.isolation_depth(), 1);
    s.restore_checkpoint(id).unwrap();
    assert_eq!((s.isolation_depth(), hidden(&s)), (0, Vec::<String>::new()));
    hide(&mut s, &[&"D".to_string()]);
    isolate(&mut s, &["A"]);
    s.execute("body.rename", &json!({"body": "D", "name": "Dee"})).unwrap();
    s.execute("display.unisolate_all", &json!({})).unwrap();
    assert_eq!(hidden(&s), names(&["Dee"]));
}
