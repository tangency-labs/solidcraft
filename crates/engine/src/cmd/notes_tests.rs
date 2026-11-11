//! Note commands (#1631): add, edit, delete, list, undo, and persistence.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

#[test]
fn add_update_delete_and_list_round_trip() {
    let mut s = Session::default();
    let added = run(&mut s, "note.add", json!({"anchor": [10.0, 20.0, 30.0], "text": "Check clearance here."}));
    let id = added["note"].as_u64().unwrap();
    assert!(id > 0);

    let listed = run(&mut s, "note.list", json!({}));
    let notes = listed["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["id"], json!(id));
    assert_eq!(notes[0]["text"], "Check clearance here.");
    assert_eq!(notes[0]["anchor"], json!([10.0, 20.0, 30.0]));
    assert_eq!(notes[0]["hidden"], json!(false));

    run(&mut s, "note.update", json!({"note": id, "text": "Adjust this diameter after the test print.", "hidden": true}));
    let listed = run(&mut s, "note.list", json!({}));
    assert_eq!(listed["notes"][0]["text"], "Adjust this diameter after the test print.");
    assert_eq!(listed["notes"][0]["hidden"], json!(true));

    run(&mut s, "note.update", json!({"note": id, "anchor": [1.0, 2.0, 3.0]}));
    assert_eq!(run(&mut s, "note.list", json!({}))["notes"][0]["anchor"], json!([1.0, 2.0, 3.0]));

    assert_eq!(run(&mut s, "note.delete", json!({"note": id})), json!({"deleted": id}));
    assert!(run(&mut s, "note.list", json!({}))["notes"].as_array().unwrap().is_empty());
}

#[test]
fn notes_survive_a_save_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "note.add", json!({"anchor": [0.0, 0.0, 5.0], "text": "Extra thickness added for strength."}));
    let saved = serde_json::to_string(&*s.doc).unwrap();
    let reopened = Session::new(serde_json::from_str(&saved).unwrap());
    assert_eq!(reopened.doc.notes.len(), 1);
    assert_eq!(reopened.doc.notes[0].text, "Extra thickness added for strength.");
}

#[test]
fn undo_undoes_note_changes() {
    let mut s = Session::default();
    run(&mut s, "note.add", json!({"anchor": [0.0, 0.0, 0.0], "text": "first"}));
    run(&mut s, "note.add", json!({"anchor": [1.0, 0.0, 0.0], "text": "second"}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.notes.len(), 1);
    assert_eq!(s.doc.notes[0].text, "first");
    let id = s.doc.notes[0].id;
    run(&mut s, "note.update", json!({"note": id, "text": "renamed"}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.notes[0].text, "first");
}

#[test]
fn ids_are_never_reused() {
    let mut s = Session::default();
    let a = run(&mut s, "note.add", json!({"anchor": [0.0, 0.0, 0.0], "text": "a"}))["note"].as_u64().unwrap();
    run(&mut s, "note.delete", json!({"note": a}));
    let b = run(&mut s, "note.add", json!({"anchor": [0.0, 0.0, 0.0], "text": "b"}))["note"].as_u64().unwrap();
    assert_ne!(a, b);
}

#[test]
fn bad_calls_are_refused() {
    let mut s = Session::default();
    // Empty or missing text.
    assert!(s.execute("note.add", &json!({"anchor": [0.0, 0.0, 0.0], "text": "   "})).is_err());
    assert!(s.execute("note.add", &json!({"anchor": [0.0, 0.0, 0.0]})).is_err());
    // A non-finite anchor.
    assert!(s.execute("note.add", &json!({"anchor": [f64::NAN, 0.0, 0.0], "text": "x"})).is_err());
    // Unknown note id.
    assert!(s.execute("note.update", &json!({"note": 999, "text": "x"})).is_err());
    assert!(s.execute("note.delete", &json!({"note": 999})).is_err());
    // An update that changes nothing.
    run(&mut s, "note.add", json!({"anchor": [0.0, 0.0, 0.0], "text": "x"}));
    let id = s.doc.notes[0].id;
    assert!(s.execute("note.update", &json!({"note": id})).is_err());
    assert_eq!(s.doc.notes[0].text, "x");
}
