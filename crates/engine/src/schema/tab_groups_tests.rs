use serde_json::{Value, json};

use super::{TabGroupAddParams, TabGroupCreateParams, TabGroupIdParams, TabGroupNoParams, TabGroupReply, TabGroupTabParams, TabGroupUpdateParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TabGroupCreateParams>(), &["colour", "name", "tab"]);
    assert_keys(input_schema_for::<TabGroupAddParams>(), &["group", "tab"]);
    assert_keys(input_schema_for::<TabGroupTabParams>(), &["tab"]);
    assert_keys(input_schema_for::<TabGroupUpdateParams>(), &["collapsed", "colour", "group", "name"]);
    assert_keys(input_schema_for::<TabGroupIdParams>(), &["group"]);
    assert_keys(input_schema_for::<TabGroupNoParams>(), &[]);
}

#[test]
fn the_group_commands_check_and_echo_without_editing_the_design() {
    let schema = output_schema_for::<TabGroupReply>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let (before, undo) = (s.doc.to_json(), s.undo.len());
    for (id, params, echo) in [
        ("tab.group.create", json!({}), json!({})),
        ("tab.group.create", json!({"tab": 1, "name": "  Fixtures  ", "colour": "cyan"}), json!({"tab": 1, "name": "Fixtures", "colour": "cyan"})),
        ("tab.group.create", json!({"name": ""}), json!({"name": ""})),
        ("tab.group.add", json!({"group": 3}), json!({"group": 3})),
        ("tab.group.add", json!({"group": 3, "tab": 0}), json!({"group": 3, "tab": 0})),
        ("tab.group.remove", json!({}), json!({})),
        ("tab.group.remove", json!({"tab": 2}), json!({"tab": 2})),
        ("tab.group.update", json!({"group": 1, "name": "Jigs"}), json!({"group": 1, "name": "Jigs"})),
        ("tab.group.update", json!({"group": 1, "colour": "pink"}), json!({"group": 1, "colour": "pink"})),
        ("tab.group.ungroup", json!({"group": 4}), json!({"group": 4})),
        ("tab.group.list", json!({}), json!({})),
    ] {
        let r = s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
        assert_eq!(r, echo, "{id} {params}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        let takes_nothing = matches!(id, "tab.group.create" | "tab.group.remove" | "tab.group.list");
        assert_eq!(s.execute(id, &Value::Null).is_ok(), takes_nothing, "{id}: null");
    }
    assert_eq!((s.doc.to_json(), s.undo.len()), (before, undo), "nothing in the design changes");
}

#[test]
fn bad_group_requests_are_refused() {
    let long = "x".repeat(65);
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for (id, params) in [
        ("tab.group.create", json!({"colour": "red"})),
        ("tab.group.create", json!({"colour": "Blue"})),
        ("tab.group.create", json!({"name": long})),
        ("tab.group.create", json!({"name": "a\nb"})),
        ("tab.group.create", json!({"name": 5})),
        ("tab.group.create", json!({"tab": -1})),
        ("tab.group.create", json!({"unknown": 1})),
        ("tab.group.add", json!({})),
        ("tab.group.add", json!({"group": "1"})),
        ("tab.group.remove", json!({"group": 1})),
        ("tab.group.update", json!({"name": "x"})),
        ("tab.group.update", json!({"group": 1, "name": long})),
        ("tab.group.update", json!({"group": 1, "colour": 3})),
        ("tab.group.ungroup", json!({})),
        ("tab.group.ungroup", json!({"group": 1.5})),
        ("tab.group.list", json!({"all": true})),
    ] {
        assert!(s.execute(id, &params).is_err(), "{id} {params}");
    }
}

#[test]
fn the_group_commands_have_no_default_shortcut() {
    for id in ["tab.group.create", "tab.group.add", "tab.group.remove", "tab.group.update", "tab.group.ungroup", "tab.group.list"] {
        assert_eq!(crate::find_command(id).map(|c| c.shortcut), Some(None), "{id}");
    }
}
