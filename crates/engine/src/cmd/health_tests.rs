//! `model.health`: the design's failures, warnings and history in brief.
#![allow(clippy::unwrap_used, clippy::panic)]
use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn health_lists_failing_features_and_the_history() {
    let mut s = Session::default();
    let h = run(&mut s, "model.health", json!({}));
    assert_eq!((h["features"].clone(), h["errors"].clone(), h["undo_depth"].clone()), (json!(0), json!([]), json!(0)), "{h}");
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "20", "unit": "mm"}));
    run(&mut s, "solid.box", json!({"length": "L", "width": 10, "height": 10}));
    run(&mut s, "solid.fillet", json!({"edges": [[20, 5, 10]], "radius": 3}));
    let h = run(&mut s, "model.health", json!({}));
    assert_eq!(h["errors"], json!([]), "{h}");
    assert_eq!(h["undo"].as_array().unwrap().last(), Some(&json!("Fillet")), "{h}");
    // The box too short for its fillet's edge: the fillet fails, and says so.
    run(&mut s, "parameters.change", json!({"name": "L", "expression": "2"}));
    let h = run(&mut s, "model.health", json!({}));
    let errors = h["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{h}");
    assert!(errors[0]["name"].as_str().unwrap().starts_with("Fillet") && errors[0]["error"].is_string(), "{h}");
    assert_eq!(h["dirty"], json!(true), "{h}");
    // Reading it changes nothing.
    let again = run(&mut s, "model.health", json!({}));
    assert_eq!(again["undo"], h["undo"]);
    assert_eq!(again["undo_depth"], h["undo_depth"]);
    run(&mut s, "edit.undo", json!({}));
    let h = run(&mut s, "model.health", json!({}));
    assert_eq!((h["errors"].clone(), h["redo_depth"].clone()), (json!([]), json!(1)), "{h}");
}
