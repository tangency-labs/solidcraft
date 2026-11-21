use serde_json::{Value, json};

use super::{
    TabViewActivateParams, TabViewBesideParams, TabViewIdParams, TabViewLayoutParams, TabViewLinkParams, TabViewMoveParams, TabViewNoParams,
    TabViewOpenParams, TabViewRenameParams, TabViewReply, TabViewWindowParams,
};
use crate::Session;
use crate::schema::tab_windows::{
    TabViewMoveToWindowParams, TabWindowCleanParams, TabWindowCloseReply, TabWindowIdParams, TabWindowListParams, TabWindowListReply,
    TabWindowMoveReply,
};
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TabViewOpenParams>(), &["kind", "reuse", "subject", "window"]);
    assert_keys(input_schema_for::<TabViewLayoutParams>(), &["layout", "window"]);
    assert_keys(input_schema_for::<TabViewBesideParams>(), &["edge", "id", "placement", "window"]);
    assert_keys(input_schema_for::<TabViewIdParams>(), &["id"]);
    assert_keys(input_schema_for::<TabViewActivateParams>(), &["id", "index", "window"]);
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
        ("tab.view.open", json!({"window": 0}), json!({"reuse": false, "window": 0})),
        ("tab.view.open", json!({"window": null}), json!({"reuse": false})),
        ("tab.view.close", json!({}), json!({})),
        ("tab.view.close", json!({"id": 3}), json!({"id": 3})),
        ("tab.view.reopen", json!({}), json!({})),
        ("tab.view.activate", json!({"id": 2}), json!({"id": 2})),
        ("tab.view.activate", json!({"index": 0}), json!({"index": 0})),
        ("tab.view.activate", json!({"id": 3, "window": 1}), json!({"id": 3, "window": 1})),
        ("tab.view.rename", json!({"name": "  Front  "}), json!({"name": "Front"})),
        ("tab.view.rename", json!({"id": 4, "name": "Gear"}), json!({"id": 4, "name": "Gear"})),
        ("tab.view.rename", json!({"id": 4}), json!({"id": 4})),
        ("tab.view.duplicate", json!({"id": 1}), json!({"id": 1})),
        ("tab.view.move", json!({"from": 0, "to": 2}), json!({"from": 0, "to": 2})),
        ("tab.view.layout", json!({"layout": "grid"}), json!({"layout": "grid"})),
        ("tab.view.layout", json!({"layout": "single"}), json!({"layout": "single"})),
        ("tab.view.layout", json!({"layout": "side_by_side", "window": 3}), json!({"layout": "side_by_side", "window": 3})),
        ("tab.view.beside", json!({}), json!({"edge": "right"})),
        ("tab.view.beside", json!({"window": 4}), json!({"edge": "right", "window": 4})),
        ("tab.view.beside", json!({"id": 2, "edge": "bottom"}), json!({"id": 2, "edge": "bottom"})),
        (
            "tab.view.beside",
            json!({"id": 2, "placement": {"kind":"split", "target":1, "edge":"bottom"}}),
            json!({"id": 2, "placement": {"kind":"split", "target":1, "edge":"bottom"}}),
        ),
        ("tab.view.list", json!({}), json!({})),
        ("tab.view.pop_out", json!({"id": 2}), json!({"id": 2})),
    ] {
        let r = s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
        assert_eq!(r, echo, "{id} {params}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(s.execute(id, &Value::Null).is_ok(), !matches!(id, "tab.view.activate" | "tab.view.move" | "tab.view.layout"), "{id}: null");
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
        ("tab.view.activate", json!({"id": 1, "window": -1})),
        ("tab.view.activate", json!({"id": 1, "window": 4_294_967_296_u64})),
        ("tab.view.rename", json!({"name": ""})),
        ("tab.view.rename", json!({"name": "   "})),
        ("tab.view.rename", json!({"name": "a\nb"})),
        ("tab.view.rename", json!({"name": long})),
        ("tab.view.rename", json!({"name": 5})),
        ("tab.view.move", json!({"from": 0})),
        ("tab.view.move", json!({"from": 0, "to": 1.5})),
        ("tab.view.layout", json!({})),
        ("tab.view.layout", json!({"layout": "triple"})),
        ("tab.view.beside", json!({"edge": "diagonal"})),
        ("tab.view.beside", json!({"id": "2"})),
        ("tab.view.beside", json!({"placement": {"kind": "empty", "index": 0}})),
        ("tab.view.beside", json!({"id": 2, "edge": "top", "placement": {"kind": "empty", "index": 0}})),
        ("tab.view.beside", json!({"id": 2, "placement": {"kind": "empty", "index": -1}})),
        ("tab.view.beside", json!({"id": 2, "placement": {"kind": "empty", "index": 0, "target": 1}})),
        ("tab.view.list", json!({"all": true})),
        ("tab.view.pop_out", json!({"id": "2"})),
        ("tab.view.open", json!({"window": -1})),
        ("tab.view.layout", json!({"layout": "grid", "window": 4_294_967_296_u64})),
        ("tab.view.beside", json!({"window": "0"})),
    ] {
        assert!(s.execute(id, &params).is_err(), "{id} {params}");
    }
}

#[test]
fn window_commands_check_and_echo_requests_without_editing_the_design() {
    assert_keys(input_schema_for::<TabViewMoveToWindowParams>(), &["display", "h", "index", "level", "placement", "view", "w", "window", "x", "y"]);
    assert_keys(input_schema_for::<TabWindowIdParams>(), &["window"]);
    assert_keys(input_schema_for::<TabWindowListParams>(), &[]);
    assert_keys(input_schema_for::<TabViewWindowParams>(), &["window"]);
    assert_keys(input_schema_for::<TabViewLinkParams>(), &["mode", "window"]);
    assert_keys(input_schema_for::<TabWindowCleanParams>(), &["on", "window"]);

    let mut session = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let before = session.doc.to_json();
    let undo = session.undo.len();
    for (command, params, echo, schema) in [
        (
            "tab.view.move_to_window",
            json!({"view": 2}),
            json!({"view": 2, "window": "new", "level": "views_only"}),
            output_schema_for::<TabWindowMoveReply>(),
        ),
        (
            "tab.view.move_to_window",
            json!({"view": 2, "window": "new", "display": "Display 2", "x": -12.5, "y": 5, "w": 640, "h": 420, "level": "views_only"}),
            json!({"view": 2, "window": "new", "display": "Display 2", "x": -12.5, "y": 5.0, "w": 640.0, "h": 420.0, "level": "views_only"}),
            output_schema_for::<TabWindowMoveReply>(),
        ),
        (
            "tab.view.move_to_window",
            json!({"view": 2, "display": 0}),
            json!({"view": 2, "window": "new", "display": 0, "level": "views_only"}),
            output_schema_for::<TabWindowMoveReply>(),
        ),
        ("tab.view.move_to_window", json!({"view": 2, "window": 0}), json!({"view": 2, "window": 0}), output_schema_for::<TabWindowMoveReply>()),
        (
            "tab.view.move_to_window",
            json!({"view": 2, "window": 0, "index": 3}),
            json!({"view": 2, "window": 0, "index": 3}),
            output_schema_for::<TabWindowMoveReply>(),
        ),
        ("tab.view.equal_sizes", json!({"window": 0}), json!({"window": 0}), output_schema_for::<TabViewReply>()),
        ("tab.view.reset_cameras", Value::Null, json!({}), output_schema_for::<TabViewReply>()),
        ("tab.view.link", json!({"mode": "orientation"}), json!({"mode": "orientation"}), output_schema_for::<TabViewLinkParams>()),
        ("tab.view.link", json!({"window": 1, "mode": "both"}), json!({"window": 1, "mode": "both"}), output_schema_for::<TabViewLinkParams>()),
        ("tab.window.clean", Value::Null, json!({}), output_schema_for::<TabWindowCleanParams>()),
        ("tab.window.clean", json!({"window": 1, "on": false}), json!({"window": 1, "on": false}), output_schema_for::<TabWindowCleanParams>()),
        ("tab.window.list", Value::Null, json!({}), output_schema_for::<TabWindowListReply>()),
        ("tab.window.focus", json!({"window": 0}), json!({"window": 0}), output_schema_for::<TabWindowIdParams>()),
        ("tab.window.close", json!({"window": 1}), json!({"window": 1}), output_schema_for::<TabWindowCloseReply>()),
    ] {
        let result = session.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
        assert_eq!(result, echo, "{command} {params}");
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &result), "{result} doesn't match {schema}");
    }
    assert_eq!(session.doc.to_json(), before);
    assert_eq!(session.undo.len(), undo);
}

#[test]
fn window_schemas_cover_host_replies() {
    for (value, schema) in [
        (json!({"view":2,"window":1,"created":true,"from":0,"closed_source":false,"placement":null}), output_schema_for::<TabWindowMoveReply>()),
        (
            json!({"windows":[{"id":0,"level":"full","views":[1],"active":1,"layout":"single","display":null,"placement":null,"tool_window":true}]}),
            output_schema_for::<TabWindowListReply>(),
        ),
        (json!({"returned":[2,3]}), output_schema_for::<TabWindowCloseReply>()),
    ] {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &value), "{value} doesn't match {schema}");
    }
}

#[test]
fn window_commands_refuse_malformed_requests() {
    let mut session = Session::default();
    for (command, params) in [
        ("tab.view.move_to_window", json!({})),
        ("tab.view.move_to_window", json!({"view": -1})),
        ("tab.view.move_to_window", json!({"view": "2"})),
        ("tab.view.move_to_window", json!({"view": 2, "unknown": true})),
        ("tab.view.move_to_window", json!({"view": 2, "window": "1"})),
        ("tab.view.move_to_window", json!({"view": 2, "window": {"new": null}})),
        ("tab.view.move_to_window", json!({"view": 2, "window": -1})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 4_294_967_296_u64})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 1, "display": 0})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 1, "x": 0})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 1, "level": "views_only"})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 1, "index": -1})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 1, "index": "0"})),
        ("tab.view.move_to_window", json!({"view": 2, "window": "new", "index": 0})),
        ("tab.view.move_to_window", json!({"view": 2, "index": 0})),
        ("tab.view.move_to_window", json!({"view": 2, "placement": {"kind": "empty", "index": 0}})),
        ("tab.view.move_to_window", json!({"view": 2, "window": 1, "index": 0, "placement": {"kind": "empty", "index": 0}})),
        ("tab.view.move_to_window", json!({"view": 2, "level": "clean"})),
        ("tab.view.move_to_window", json!({"view": 2, "display": -1})),
        ("tab.view.move_to_window", json!({"view": 2, "display": 4_294_967_296_u64})),
        ("tab.view.move_to_window", json!({"view": 2, "x": 1e100})),
        ("tab.view.move_to_window", json!({"view": 2, "y": -1e100})),
        ("tab.view.move_to_window", json!({"view": 2, "h": 1e100})),
        ("tab.view.equal_sizes", json!({"window": -1})),
        ("tab.view.reset_cameras", json!({"window": "main"})),
        ("tab.view.link", json!({})),
        ("tab.view.link", json!({"mode": "position"})),
        ("tab.view.link", json!({"mode": "zoom", "window": 4_294_967_296_u64})),
        ("tab.window.clean", json!({"on": "yes"})),
        ("tab.window.clean", json!({"window": 1, "unknown": true})),
        ("tab.window.list", json!({"window": 1})),
        ("tab.window.focus", json!({})),
        ("tab.window.focus", json!({"window": 4_294_967_296_u64})),
        ("tab.window.focus", json!({"window": 1, "unknown": true})),
        ("tab.window.close", json!({"window": -1})),
        ("tab.window.close", json!({"window": "1"})),
    ] {
        assert!(session.execute(command, &params).is_err(), "{command} {params}");
    }
    for size in [json!({"view": 2, "w": 0}), json!({"view": 2, "h": -1})] {
        assert!(session.execute("tab.view.move_to_window", &size).unwrap_err().to_string().contains("bad_geometry"));
    }
    assert!(session.execute("tab.window.close", &json!({"window": 0})).unwrap_err().to_string().contains("cannot_close_main_window"));
}

#[test]
fn the_view_shortcuts_are_the_defaults() {
    for (id, key) in [
        ("tab.view.open", Some("Ctrl+T")),
        ("tab.view.reopen", Some("Ctrl+Shift+T")),
        ("tab.view.rename", Some("F2")),
        ("tab.view.close", None),
        ("tab.view.list", None),
        ("tab.view.pop_out", Some("Ctrl+Alt+O")),
        ("tab.view.move_to_window", Some("Ctrl+Alt+W")),
        ("tab.window.clean", Some("F11")),
    ] {
        assert_eq!(crate::find_command(id).map(|c| c.shortcut), Some(key), "{id}");
    }
}
