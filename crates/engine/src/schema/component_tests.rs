use std::sync::Arc;

use serde_json::{Value, json};

use super::{EditInPlaceParams, FinishEditParams, MirrorComponentsParams, MirrorComponentsResult, OccurrenceAlignParams, OccurrenceAlignResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EditInPlaceParams>(), &["component", "path"]);
    assert_keys(input_schema_for::<FinishEditParams>(), &[]);
    assert_keys(input_schema_for::<MirrorComponentsParams>(), &["as", "components", "follow", "plane", "symmetry"]);
    assert_keys(
        input_schema_for::<OccurrenceAlignParams>(),
        &[
            "capture",
            "flip",
            "from",
            "from_face",
            "from_plane",
            "move_grounded",
            "occurrence",
            "occurrences",
            "path",
            "paths",
            "to",
            "to_face",
            "to_plane",
        ],
    );
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

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<OccurrenceAlignResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    assert!(!required.contains(&"shared"), "`shared` is left out when nothing shared moved");

    let mut s = Session::default();
    let comp = s.execute("component.create", &json!({"name": "Extrusion"})).unwrap_or_else(|e| panic!("{e}"));
    let comp_id = comp["component"].as_u64().unwrap_or(0);
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 40})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("component.activate", &json!({"component": "root"})).unwrap_or_else(|e| panic!("{e}"));
    let copy = s.execute("occurrence.copy", &json!({"component": comp_id, "translate": [30, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    let second = copy["occurrence"].as_u64().unwrap_or(0);

    s.param_check = ParamCheck::Warn;
    let r1 = s.execute("occurrence.align", &json!({"occurrence": second, "from": [30, 0, 0], "to": [10, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    assert!(conforms(schema, schema, &r1), "{r1} doesn't match {schema}");

    let r2 = s
        .execute("occurrence.align", &json!({"paths": [[second]], "from": [10, 0, 0], "to": [20, 0, 0], "unknown_extra": 1}))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(r2.get("param_warnings").is_some(), "{r2}");
    assert!(conforms(schema, schema, &r2), "{r2} doesn't match {schema}");
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("component.mirror", json!({"components": ["Part"], "plane": "YZ", "as": "sideways"})),
        ("component.mirror", json!({"components": ["Part"], "plane": "YZ", "as": 3})),
        ("component.mirror", json!({"components": ["Part"], "plane": "YZ", "symmetry": "W"})),
    ]);
}

#[test]
fn mirror_replies_match_the_output_schema() {
    let schema = output_schema_for::<MirrorComponentsResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 4, "corner": [20, 0, 0]})).unwrap();
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    for p in [
        json!({"components": ["Part"], "plane": "YZ"}),
        json!({"components": "Part:1", "plane": {"origin": [0, 0, 0], "normal": [0, 1, 0]}, "symmetry": "auto", "follow": false}),
        json!({"components": ["Part"], "plane": "XZ", "as": "opposite_hand", "symmetry": "Z"}),
        json!({"components": ["Part"], "plane": "XY", "as": "instance", "follow": true}),
    ] {
        let r = s.execute("component.mirror", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings"), None, "{r}");
    }
    assert!(!conforms(schema, schema, &json!({"occurrences": "1"})));
}
