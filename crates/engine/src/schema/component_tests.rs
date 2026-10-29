use std::sync::Arc;

use serde_json::json;

use super::{EditInPlaceParams, FinishEditParams};
use crate::Session;
use crate::schema::input_schema_for;
use crate::schema::tests::assert_keys;

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EditInPlaceParams>(), &["component", "path"]);
    assert_keys(input_schema_for::<FinishEditParams>(), &[]);
}

#[test]
fn editing_in_place_outside_the_window_only_checks_the_component() {
    let mut s = Session::default();
    let made = s.execute("component.create", &json!({"name": "Bracket"})).unwrap_or_else(|e| panic!("{e}"));
    let e = s.execute("component.edit_in_place", &json!({"component": made["component"]})).unwrap_err().to_string();
    assert!(e.contains("not linked"), "{e}");
    assert!(s.execute("component.edit_in_place", &json!({"component": "nothing"})).is_err());
    assert!(s.execute("component.edit_in_place", &json!({})).is_err(), "the component is required");
}

#[test]
fn finishing_an_edit_outside_the_window_changes_nothing() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    let r = s.execute("component.finish_edit", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.as_object().is_some_and(serde_json::Map::is_empty), "{r}");
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.undo.len() == undo);
}
