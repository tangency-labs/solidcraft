#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::{Sel, Session};
use serde_json::{Value, json};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

/// Three placed components and a body in the design; the placements' ids.
fn parts(s: &mut Session) -> Vec<u64> {
    let mut placed = Vec::new();
    for (k, name) in ["LED1", "LED2", "USB"].into_iter().enumerate() {
        placed.push(run(s, "component.create", json!({"name": name}))["occurrence"].as_u64().unwrap());
        run(s, "solid.box", json!({"length": 5, "width": 5, "height": 2, "corner": [10.0 * k as f64, 0, 0]}));
        run(s, "component.activate", json!({"component": 0}));
    }
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 2, "corner": [0, 20, 0], "body_name": "Board"}));
    placed
}

/// #18716: several components and a body, selected, are kept under a name that says what it
/// holds; applying it selects them all again, with a component deleted since skipped.
#[test]
fn a_saved_selection_selects_its_items_again() {
    let mut s = Session::default();
    let placed = parts(&mut s);
    s.selection = placed.iter().map(|o| Sel::Occurrence { path: vec![*o] }).chain([Sel::Body { name: "Board".into() }]).collect();
    let saved = run(&mut s, "named_selection.save", json!({}));
    assert_eq!(saved["selection"], "Selection1 – 3 components, 1 body");
    assert_eq!(saved["count"], 4);
    let wanted = s.selection.clone();
    run(&mut s, "select.clear", json!({}));
    let applied = run(&mut s, "named_selection.apply", json!({"selection": "Selection1 – 3 components, 1 body"}));
    assert_eq!((applied["selected"].as_u64(), applied["skipped"].as_u64()), (Some(4), Some(0)));
    assert_eq!(s.selection, wanted);
    // A component deleted since is skipped; the rest are still selected.
    run(&mut s, "selection.delete", json!({"items": [{"type": "occurrence", "path": [placed[2]]}]}));
    let applied = run(&mut s, "named_selection.apply", json!({"selection": "Selection1 – 3 components, 1 body"}));
    assert_eq!((applied["selected"].as_u64(), applied["skipped"].as_u64()), (Some(3), Some(1)));
}

/// Update, rename and delete change the design and undo; the selections survive a save and
/// reopen; names are checked.
#[test]
fn named_selections_update_rename_delete_undo_and_are_saved() {
    let mut s = Session::default();
    let placed = parts(&mut s);
    let two: Vec<Value> = placed[..2].iter().map(|o| json!({"type": "occurrence", "path": [o]})).collect();
    run(&mut s, "named_selection.save", json!({"name": "LEDs", "items": two}));
    assert!(s.execute("named_selection.save", &json!({"name": "LEDs", "items": [{"type": "body", "name": "Board"}]})).is_err(), "names are unique");
    assert!(s.execute("named_selection.save", &json!({})).is_err(), "nothing selected, nothing to keep");
    s.selection = placed.iter().map(|o| Sel::Occurrence { path: vec![*o] }).collect();
    assert_eq!(run(&mut s, "named_selection.update", json!({"selection": "LEDs"}))["count"], 3);
    run(&mut s, "named_selection.rename", json!({"selection": "LEDs", "name": "  Front panel  "}));
    assert_eq!(s.doc.named_selections[0].name, "Front panel");
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.named_selections, s.doc.named_selections);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.named_selections[0].name, "LEDs", "rename undoes");
    run(&mut s, "named_selection.delete", json!({"selection": "LEDs"}));
    assert!(s.doc.named_selections.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.named_selections.len(), 1, "delete undoes");
    let listed = run(&mut s, "named_selection.list", json!({}));
    assert_eq!(listed["selections"][0]["items"].as_array().map(Vec::len), Some(3));
    assert!(s.execute("named_selection.apply", &json!({"selection": "nope"})).is_err());
}

/// A default name says what the selection holds, faces and edges included, and follows an
/// update; a typed name stays.
#[test]
fn default_names_count_faces_and_edges_and_follow_an_update() {
    let mut s = Session::default();
    let placed = parts(&mut s);
    let faces = json!([
        {"type": "face", "body": "Board", "index": 0},
        {"type": "face", "body": "Board", "index": 1},
        {"type": "edge", "body": "Board", "index": 0}
    ]);
    assert_eq!(run(&mut s, "named_selection.save", json!({"items": faces}))["selection"], "Selection1 – 2 faces, 1 edge");
    let two = json!([{"type": "occurrence", "path": [placed[0]]}, {"type": "occurrence", "path": [placed[1]]}]);
    let updated = run(&mut s, "named_selection.update", json!({"selection": "Selection1 – 2 faces, 1 edge", "items": two}));
    assert_eq!(updated["selection"], "Selection1 – 2 components");
    run(&mut s, "named_selection.rename", json!({"selection": "Selection1 – 2 components", "name": "LEDs"}));
    let body = json!([{"type": "body", "name": "Board"}]);
    assert_eq!(run(&mut s, "named_selection.update", json!({"selection": "LEDs", "items": body}))["selection"], "LEDs");
}
