use serde_json::{Value, json};

use super::{BoxParams, ExtrudeParams, FeatureResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BoxParams>(),
        &["body_name", "body_names", "center", "corner", "height", "length", "name", "operation", "participants", "targets", "width"],
    );
    assert_keys(
        input_schema_for::<ExtrudeParams>(),
        &[
            "body",
            "body_name",
            "body_names",
            "convention",
            "curves",
            "direction",
            "distance",
            "distance2",
            "extent",
            "face",
            "face_body",
            "faces",
            "from",
            "name",
            "operation",
            "participants",
            "profiles",
            "side",
            "sketch",
            "start_offset",
            "taper",
            "targets",
            "thickness",
            "thickness2",
            "thin",
            "through_all",
            "to",
            "to_offset",
        ],
    );
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "bogus"})),
        ("solid.box", json!({"length": true, "width": 5, "height": 5})),
        ("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [1, 2]})),
        ("solid.extrude", json!({"distance": 5, "operation": "bogus"})),
        ("solid.extrude", json!({"distance": 5, "direction": "sideways"})),
        ("solid.extrude", json!({"distance": 5, "direction": "SIDEWAYS"})),
        ("solid.extrude", json!({"distance": 5, "profiles": [{"point": [1]}]})),
        ("solid.extrude", json!({"distance": 5, "profiles": [["l1"], 7]})),
        ("solid.extrude", json!({"distance": 5, "profiles": []})),
        ("solid.extrude", json!({"distance": 5, "profiles": [true]})),
        ("solid.extrude", json!({"distance": 5, "faces": []})),
        ("solid.extrude", json!({"distance": 5, "faces": null})),
        ("solid.extrude", json!({"distance": 5, "faces": [[1, 2]]})),
        ("solid.extrude", json!({"distance": 5, "profiles": {"offset_boundaries": ["a", "a"]}})),
        ("solid.extrude", json!({"distance": 5, "thin": true, "thickness": 1, "side": "middle"})),
        ("solid.extrude", json!({"distance": 5, "thin": "true"})),
        ("solid.extrude", json!({"distance": 5, "to": [1, 2]})),
        ("solid.extrude", json!({"distance": 5, "from": "x"})),
        ("solid.extrude", json!({"distance": 5, "sketch": true})),
        ("solid.extrude", json!({"distance": 5, "convention": "per_side"})),
        ("solid.extrude", json!({"distance": "  "})),
        ("solid.extrude", json!({"distance": 5, "operation": "x".repeat(1000)})),
    ]);
}

#[test]
fn feature_result_serializes_like_the_old_reply() {
    for (warning, ms) in [(None, None), (Some("a Boolean needed a retry".to_string()), Some(12.5)), (None, Some(0.0)), (None, Some(1.0 / 3.0))] {
        let old = json!({"feature": 7, "name": "Box1", "bodies": ["A", "B"], "created_bodies": ["B"], "changed_bodies": [],
            "removed_bodies": ["C"], "warning": warning, "ms": ms});
        let new = FeatureResult {
            feature: 7,
            name: "Box1".into(),
            bodies: vec!["A".into(), "B".into()],
            created_bodies: vec!["B".into()],
            changed_bodies: vec![],
            removed_bodies: vec!["C".into()],
            warning: warning.clone(),
            ms,
            param_warnings: vec![],
            unit_warnings: vec![],
        };
        assert_eq!(serde_json::to_value(&new).ok(), Some(old));
    }
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    let mut s = Session::default();
    sketch_rectangle(&mut s);
    s.param_check = ParamCheck::Warn;
    let replies = [
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})),
        s.execute("solid.extrude", &json!({"distance": 5, "operation": "join"})),
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "colour": "red"})),
    ];
    for r in replies {
        let r = r.unwrap_or_else(|e| panic!("{e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    // The check itself catches a wrong type, a wrong item type and a missing key. (The output
    // schema leaves other keys open, so a new result key doesn't break clients.)
    let ok = json!({"feature": 1, "name": "Box1", "bodies": [], "created_bodies": [], "changed_bodies": [], "removed_bodies": [], "warning": null, "ms": null});
    assert!(conforms(schema, schema, &ok));
    for (k, v) in [("feature", json!("1")), ("bodies", json!([1])), ("warning", json!(5)), ("ms", json!("1"))] {
        let mut bad = ok.clone();
        bad[k] = v;
        assert!(!conforms(schema, schema, &bad), "{bad}");
    }
    let mut bad = ok.clone();
    bad.as_object_mut().map(|o| o.remove("name"));
    assert!(!conforms(schema, schema, &bad), "{bad}");
}
