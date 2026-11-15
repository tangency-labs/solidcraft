use serde_json::{Value, json};

use super::{TabViewActivateParams, TabViewIdParams, TabViewMoveParams, TabViewNoParams, TabViewOpenParams, TabViewRenameParams, TabViewReply};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TabViewOpenParams>(), &["kind", "reuse", "subject"]);
    assert_keys(input_schema_for::<TabViewIdParams>(), &["id"]);
    assert_keys(input_schema_for::<TabViewActivateParams>(), &["id", "index"]);
    assert_keys(input_schema_for::<TabViewRenameParams>(), &["id", "name"]);
    assert_keys(input_schema_for::<TabViewMoveParams>(), &["from", "to"]);
    assert_keys(input_schema_for::<TabViewNoParams>(), &[]);
}

#[test]
fn the_view_commands_check_and_echo_without_editing_the_design() {
    let schema = output_schema_for::<TabViewReply>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let (before, undo) = (s.doc.to_json(), s.undo.len());
    for (id, params, echo) in [
        ("tab.view.open", json!({}), json!({"reuse": false})),
        ("tab.view.open", json!({"kind": "sketch", "subject": "7"}), json!({"kind": "sketch", "subject": "7", "reuse": true})),
        ("tab.view.open", json!({"kind": "render", "reuse": true}), json!({"kind": "render", "reuse": true})),
        ("tab.view.close", json!({}), json!({})),
        ("tab.view.close", json!({"id": 3}), json!({"id": 3})),
        ("tab.view.reopen", json!({}), json!({})),
        ("tab.view.activate", json!({"id": 2}), json!({"id": 2})),
        ("tab.view.activate", json!({"index": 0}), json!({"index": 0})),
        ("tab.view.rename", json!({"name": "  Front  "}), json!({"name": "Front"})),
        ("tab.view.rename", json!({"id": 4, "name": "Gear"}), json!({"id": 4, "name": "Gear"})),
        ("tab.view.rename", json!({"id": 4}), json!({"id": 4})),
        ("tab.view.duplicate", json!({"id": 1}), json!({"id": 1})),
        ("tab.view.move", json!({"from": 0, "to": 2}), json!({"from": 0, "to": 2})),
        ("tab.view.list", json!({}), json!({})),
    ] {
        let r = s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
        assert_eq!(r, echo, "{id} {params}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(s.execute(id, &Value::Null).is_ok(), !matches!(id, "tab.view.activate" | "tab.view.move"), "{id}: null");
    }
    assert_eq!((s.doc.to_json(), s.undo.len()), (before, undo), "nothing in the design changes");
}

#[test]
fn bad_view_requests_are_refused() {
    let long = "x".repeat(65);
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for (id, params) in [
        ("tab.view.open", json!({"kind": "bogus"})),
        ("tab.view.open", json!({"kind": 3})),
        ("tab.view.open", json!({"subject": "s".repeat(257)})),
        ("tab.view.open", json!({"reuse": "yes"})),
        ("tab.view.open", json!({"unknown": 1})),
        ("tab.view.close", json!({"id": "1"})),
        ("tab.view.close", json!({"id": -1})),
        ("tab.view.reopen", json!({"id": 1})),
        ("tab.view.activate", json!({})),
        ("tab.view.activate", json!({"id": 1, "index": 0})),
        ("tab.view.activate", json!({"index": -1})),
        ("tab.view.rename", json!({"name": ""})),
        ("tab.view.rename", json!({"name": "   "})),
        ("tab.view.rename", json!({"name": "a\nb"})),
        ("tab.view.rename", json!({"name": long})),
        ("tab.view.rename", json!({"name": 5})),
        ("tab.view.move", json!({"from": 0})),
        ("tab.view.move", json!({"from": 0, "to": 1.5})),
        ("tab.view.list", json!({"all": true})),
    ] {
        assert!(s.execute(id, &params).is_err(), "{id} {params}");
    }
}

#[test]
fn the_view_shortcuts_are_the_defaults() {
    for (id, key) in [
        ("tab.view.open", Some("Ctrl+T")),
        ("tab.view.reopen", Some("Ctrl+Shift+T")),
        ("tab.view.rename", Some("F2")),
        ("tab.view.close", None),
        ("tab.view.list", None),
    ] {
        assert_eq!(crate::find_command(id).map(|c| c.shortcut), Some(key), "{id}");
    }
}
