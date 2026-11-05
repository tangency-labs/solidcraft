use serde_json::{Value, json};

use super::{
    ContactCheck, ContactCreateParams, ContactEditParams, ContactEnableParams, ContactNoParams, ContactSetCreated, ContactSetEdited, ContactSets,
    ContactSwitch, FindLimitsParams, FindLimitsResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FindLimitsParams>(), &["against", "contact_sets", "index", "joint", "occurrences", "refine", "step"]);
    assert_keys(input_schema_for::<ContactEnableParams>(), &["enabled"]);
    assert_keys(input_schema_for::<ContactNoParams>(), &[]);
    assert_keys(input_schema_for::<ContactCreateParams>(), &["name", "occurrences"]);
    assert_keys(input_schema_for::<ContactEditParams>(), &["delete", "name", "occurrences", "set", "suppressed"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let many = vec!["A"; 10_001];
    assert_strict_keeps_todays_errors(&[
        ("joint.find_limits", json!({"joint": true})),
        ("joint.find_limits", json!({"joint": null})),
        ("joint.find_limits", json!({"joint": "Hinge"})),
        ("contact.create", json!({})),
        ("contact.create", json!({"occurrences": null})),
        ("contact.create", json!({"occurrences": "Door"})),
        ("contact.create", json!({"occurrences": many})),
        ("contact.create", json!({"occurrences": [true, "Door"]})),
        ("contact.create", json!({"occurrences": ["Nope", "Door"]})),
        ("contact.create", json!({"occurrences": ["root", "Door"]})),
        ("contact.create", json!({"occurrences": [7]})),
        ("contact.edit", json!({})),
        ("contact.edit", json!({"set": true})),
        ("contact.edit", json!({"set": null})),
        ("contact.edit", json!({"set": "Nope", "delete": true})),
    ]);
}

/// The contact contracts say what their handlers need: `occurrences` and `set` are required in
/// the schemas, though the types leave them to the handlers' messages, and a list names two or
/// more occurrences.
#[test]
fn the_contact_schemas_take_what_the_handlers_read() {
    let create = input_schema_for::<ContactCreateParams>();
    let create = create.as_value();
    for (p, takes) in [
        (json!({"occurrences": ["Door", 7], "name": "Swing"}), true),
        (json!({"occurrences": ["Door"]}), false),
        (json!({"occurrences": [true, "Door"]}), false),
        (json!({"name": "Swing"}), false),
    ] {
        assert_eq!(conforms(create, create, &p), takes, "{p}");
    }
    let edit = input_schema_for::<ContactEditParams>();
    let edit = edit.as_value();
    for (p, takes) in [(json!({"set": 3, "suppressed": true}), true), (json!({"set": null}), false), (json!({"delete": true}), false)] {
        assert_eq!(conforms(edit, edit, &p), takes, "{p}");
    }
}

/// #11949: every contact command replies as its output schema says, in Warn without warnings:
/// sets made, listed, changed, suppressed and deleted, contact switched on and off, and a check
/// that finds two parts passing through each other. A misspelt key gets `param_warnings` in Warn,
/// since the handlers ignore unknown keys, and strict refuses it.
#[test]
fn contact_commands_reply_with_their_schemas() {
    let mut s = door();
    // A part through the door, so a check finds them.
    for (id, p) in [
        ("component.activate", json!({"component": "root"})),
        ("component.create", json!({"name": "Clash", "grounded": false})),
        ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [10, -3, 0]})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s.param_check = ParamCheck::Warn;
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    run("contact.list", json!({}), output_schema_for::<ContactSets>());
    let r = run("contact.create", json!({"occurrences": ["Door", "Clash"], "name": "Swing"}), output_schema_for::<ContactSetCreated>());
    assert_eq!(r["name"], "Swing", "{r}");
    let other = run("contact.create", json!({"occurrences": ["Door", "Slide", "Door"]}), output_schema_for::<ContactSetCreated>());
    let r = run("contact.check", json!({}), output_schema_for::<ContactCheck>());
    assert!(r["collisions"].as_array().is_some_and(|c| !c.is_empty()), "{r}");
    run(
        "contact.edit",
        json!({"set": "Swing", "suppressed": true, "occurrences": ["Door", "Clash", "Block"], "name": "Swing 2"}),
        output_schema_for::<ContactSetEdited>(),
    );
    let r = run("contact.list", json!({}), output_schema_for::<ContactSets>());
    assert!(r["sets"][0]["suppressed"] == true && r["sets"][1].get("suppressed").is_none(), "{r}");
    let r = run("contact.edit", json!({"set": other["set"], "delete": true}), output_schema_for::<ContactSetEdited>());
    assert_eq!(r["deleted"], true, "{r}");
    for (id, p) in [("contact.enable_sets", json!({"enabled": false})), ("contact.enable_all", json!({})), ("contact.disable_all", json!({}))] {
        run(id, p, output_schema_for::<ContactSwitch>());
    }
    let p = json!({"enable": false});
    let r = s.execute("contact.enable_sets", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("enable"), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("contact.enable_sets", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`enable`"), "{error}");
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
