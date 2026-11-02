use serde_json::{Value, json};

use super::{FindLimitsParams, FindLimitsResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FindLimitsParams>(), &["against", "contact_sets", "index", "joint", "occurrences", "refine", "step"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("joint.find_limits", json!({"joint": true})),
        ("joint.find_limits", json!({"joint": null})),
        ("joint.find_limits", json!({"joint": "Hinge"})),
    ]);
}

/// A 100 × 4 × 50 door hinged about Z at the origin, a block in its way and a slider beside it.
fn door() -> Session {
    let mut s = Session::default();
    for (name, corner, size) in [("Door", [0, 0, 0], [100, 4, 50]), ("Block", [60, 30, 0], [20, 20, 50]), ("Slide", [0, -40, 0], [10, 10, 10])] {
        for (id, p) in [
            ("component.activate", json!({"component": "root"})),
            ("component.create", json!({"name": name, "grounded": false})),
            ("solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner})),
        ] {
            s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
    for p in [
        json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]}, "flip": true, "name": "Hinge"}),
        json!({"type": "slider", "a": {"occurrence": 0, "point": [0, -40, 0], "z": [1, 0, 0]}, "b": {"occurrence": "Slide", "point": [0, -40, 0], "z": [1, 0, 0]}, "flip": true, "name": "Rail"}),
    ] {
        s.execute("joint.create", &p).unwrap_or_else(|e| panic!("{e}"));
    }
    s
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FindLimitsResult>();
    let schema = schema.as_value();
    let mut s = door();
    s.param_check = ParamCheck::Warn;
    for p in [json!({"joint": "Hinge"}), json!({"joint": "Rail", "index": 0, "refine": 0.1, "occurrences": ["Slide", "Door"]})] {
        let r = s.execute("joint.find_limits", &p).unwrap_or_else(|e| panic!("{e}"));
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    // A misspelt key still runs in warn, and says so.
    let r = s.execute("joint.find_limits", &json!({"joint": "Hinge", "refin": 0.1})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()) && conforms(schema, schema, &r), "{r}");
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
}
