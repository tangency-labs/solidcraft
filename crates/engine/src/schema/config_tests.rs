use serde_json::{Value, json};

use super::{
    ConfigActivation, ConfigColumnParams, ConfigConfigureParams, ConfigRowEditParams, ConfigRowParams, ConfigRuleParams, ConfigTableParams,
    ConfigTableReply,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box `W` long, with `W` and the box's suppression configured, a `Big` row and a
/// `Without` row that suppresses the box.
fn configured(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "W", "expression": "10 mm"}));
    run(s, "solid.box", json!({"length": "W", "width": 10, "height": 10, "body_name": "A", "name": "Block"}));
    run(s, "config.configure", json!({"params": ["W"], "features": ["Block"]}));
    run(s, "config.row", json!({"name": "Big", "values": {"param:W": "20 mm"}}));
    run(s, "config.row", json!({"name": "Without", "values": {"suppress:Block": true}}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ConfigTableParams>(), &[]);
    assert_keys(input_schema_for::<ConfigRowParams>(), &["row"]);
    assert_keys(
        input_schema_for::<ConfigConfigureParams>(),
        &["appearances", "component_appearances", "face_appearances", "features", "materials", "params"],
    );
    assert_keys(
        input_schema_for::<ConfigColumnParams>(),
        &["appearance", "component_appearance", "delete", "face_appearance", "feature", "material", "param"],
    );
    assert_keys(input_schema_for::<ConfigRowEditParams>(), &["delete", "from", "name", "rename", "values"]);
    assert_keys(input_schema_for::<ConfigRuleParams>(), &["delete", "id", "index", "rule", "rules"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("config.activate", json!({})),
        ("config.activate", json!({"row": 5})),
        ("config.activate", json!({"row": "Nope"})),
        ("config.configure", json!({"params": 5})),
        ("config.configure", json!({"params": ["W", 1]})),
        ("config.configure", json!({"params": "Nope"})),
        ("config.configure", json!({"features": {"Block": true}})),
        ("config.configure", json!({"materials": ["Nope"]})),
        ("config.configure", json!({"component_appearances": null})),
        ("config.configure", json!({"component_appearances": [true]})),
        ("config.configure", json!({"component_appearances": ["Nope"]})),
        ("config.configure", json!({"face_appearances": ["appearance:A"]})),
        ("config.configure", json!({"face_appearances": [{"body": "A"}]})),
        ("config.configure", json!({"face_appearances": [{"body": "Nope", "point": [0, 0, 0]}]})),
        ("config.column", json!({})),
        ("config.column", json!({"param": "W", "feature": "Block"})),
        ("config.column", json!({"param": 5})),
        ("config.column", json!({"param": null})),
        ("config.column", json!({"material": 5})),
        ("config.column", json!({"param": "Nope"})),
        ("config.column", json!({"feature": "Nope"})),
        ("config.column", json!({"component_appearance": -1})),
        ("config.column", json!({"face_appearance": 5})),
        ("config.column", json!({"face_appearance": {"body": "A", "point": [0, 0]}})),
        ("config.column", json!({"face_appearance": "face_appearance:A"})),
        ("config.column", json!({"face_appearance": "face_appearance:A", "delete": true})),
        ("config.row", json!({})),
        ("config.row", json!({"name": 5})),
        ("config.row", json!({"name": "  "})),
        ("config.row", json!({"name": "R".repeat(129)})),
        ("config.row", json!({"name": "R", "values": 5})),
        ("config.row", json!({"name": "R", "values": null})),
        ("config.row", json!({"name": "R", "values": {"param:W": []}})),
        ("config.row", json!({"name": "R", "values": {"param:W": {"r": 1}}})),
        ("config.row", json!({"name": "R", "values": {"param:W": "1".repeat(4097)}})),
        ("config.row", json!({"name": "R", "values": {"param:W": "1"}})),
        ("config.row", json!({"name": "R", "from": "Nope"})),
        ("config.row", json!({"name": "R", "rename": null})),
        ("config.row", json!({"name": "R", "rename": "S"})),
        ("config.row", json!({"name": "R", "delete": true})),
        ("config.rule", json!({})),
        ("config.rule", json!({"id": 5})),
        ("config.rule", json!({"id": "r1"})),
        ("config.rule", json!({"rules": 5})),
        ("config.rule", json!({"rules": null})),
        ("config.rule", json!({"rules": vec![json!({}); 2049]})),
        ("config.rule", json!({"rule": 5})),
        ("config.rule", json!({"rule": null})),
        ("config.rule", json!({"rule": always("r1", "W", "1")})),
    ]);
}

/// A rule `id` that always drives the parameter `parameter` to `expression`.
fn always(id: &str, parameter: &str, expression: &str) -> Value {
    json!({"id": id, "name": id, "condition": {"type": "always"}, "effect": {"type": "drive_parameter", "parameter": parameter, "expression": expression}})
}

/// Moving a rule reads `index` only once the rule is found, so these calls need a design with
/// one.
#[test]
fn strict_keeps_the_rule_moves_errors() {
    for p in [json!({"id": "r1"}), json!({"id": "r1", "index": -1}), json!({"id": "r1", "index": "0"}), json!({"id": "r1", "index": 5})] {
        let error = |level: ParamCheck| {
            let mut s = Session::default();
            configured(&mut s);
            run(&mut s, "config.rule", json!({"rule": always("r1", "W", "15 mm")}));
            s.param_check = level;
            s.execute("config.rule", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #11895: each schema takes what its handler reads, and refuses what the handler refuses before
/// looking at the design: a list of names or one name, a column's one target, a face picked or
/// a face column's key, a row's cells and a rule.
#[test]
fn the_table_editing_schemas_take_what_the_handlers_read() {
    let check = |schema: schemars::Schema, p: Value, takes: bool| {
        let schema = schema.as_value();
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    };
    check(input_schema_for::<ConfigConfigureParams>(), json!({"params": "W", "features": ["Block"], "materials": null}), true);
    check(input_schema_for::<ConfigConfigureParams>(), json!({"params": [1]}), false);
    check(input_schema_for::<ConfigConfigureParams>(), json!({"component_appearances": ["Arm", 3]}), true);
    check(input_schema_for::<ConfigConfigureParams>(), json!({"component_appearances": [-3]}), false);
    check(input_schema_for::<ConfigConfigureParams>(), json!({"face_appearances": [{"body": "A", "point": [0, 0, 5]}]}), true);
    check(input_schema_for::<ConfigConfigureParams>(), json!({"face_appearances": ["face_appearance:A"]}), false);
    check(input_schema_for::<ConfigColumnParams>(), json!({"face_appearance": "face_appearance:A", "delete": true}), true);
    check(input_schema_for::<ConfigColumnParams>(), json!({"param": null}), false);
    check(input_schema_for::<ConfigRowEditParams>(), json!({"name": "R", "values": {"param:W": "20 mm", "suppress:Block": true}}), true);
    check(input_schema_for::<ConfigRowEditParams>(), json!({"values": {}}), false);
    check(input_schema_for::<ConfigRuleParams>(), json!({"rule": always("r1", "W", "1")}), true);
    check(input_schema_for::<ConfigRuleParams>(), json!({"rules": [always("r1", "W", "1"), {"id": "r2", "name": "r2"}]}), false);
    check(input_schema_for::<ConfigRuleParams>(), json!({"id": "r1", "index": 0}), true);
    check(input_schema_for::<ConfigRuleParams>(), json!({"id": "r1", "index": -1}), false);
    // The schema of `config.column` says that exactly one target is given (`conforms` doesn't
    // read `oneOf`).
    let column = input_schema_for::<ConfigColumnParams>();
    let one_of = column.as_value()["oneOf"].as_array().map(Vec::len);
    assert_eq!(one_of, Some(6), "{}", column.as_value());
}

/// A preview runs the activation, whose messages name `config.activate`, where strict names
/// `config.preview`; the message after the name is the same.
#[test]
fn a_preview_keeps_the_activations_messages_in_strict() {
    for p in [json!({}), json!({"row": 5}), json!({"row": "Nope"})] {
        let message = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            s.execute("config.preview", &p).err().map(|e| e.to_string().split_once("`: ").map(|m| m.1.to_string()))
        };
        let off = message(ParamCheck::Off);
        assert!(off.as_ref().is_some_and(Option::is_some), "{p} should fail today");
        assert_eq!(message(ParamCheck::Strict), off, "{p}");
    }
}

/// #11059: the table, the rules, an activation and a preview reply as their output schemas say,
/// in Warn without warnings for good calls and with them for a misspelt key.
#[test]
fn configuration_commands_reply_with_their_schemas() {
    let (table, activation) = (output_schema_for::<ConfigTableReply>(), output_schema_for::<ConfigActivation>());
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    matches(&table, &run(&mut s, "config.table", json!({})), false);
    configured(&mut s);
    let r = run(&mut s, "config.table", json!({}));
    matches(&table, &r, false);
    assert_eq!((r["rows"].as_array().map(Vec::len), r["targets"].as_array().map(Vec::len)), (Some(3), Some(2)), "{r}");
    matches(&table, &run(&mut s, "config.rules", json!({})), false);
    let r = run(&mut s, "config.activate", json!({"row": "Big"}));
    matches(&activation, &r, false);
    assert!(r["active"] == "Big" && r.get("preview").is_none(), "{r}");
    let r = run(&mut s, "config.preview", json!({"row": "Without"}));
    matches(&activation, &r, false);
    assert!(r["preview"] == true && r["parameters"].is_array() && r["active"] == "Without", "{r}");
    assert_eq!(s.doc.configs.active.as_deref(), Some("Big"), "a preview changes nothing");
    matches(&table, &run(&mut s, "config.table", json!({"row": "Big"})), true);
    matches(&activation, &run(&mut s, "config.activate", json!({"row": "Default", "rows": 1})), true);
}

/// #11895: configuring, a column added and deleted, a row added, edited, renamed and deleted,
/// and rules added, moved and deleted, each reply as `config.table` does, in Warn without
/// warnings; a misspelt key gets one.
#[test]
fn table_edits_reply_with_the_table() {
    let table = output_schema_for::<ConfigTableReply>();
    let schema = table.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "10 mm"}));
    run(&mut s, "parameters.add", json!({"name": "H", "expression": "5 mm"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 10, "height": "H", "body_name": "A", "name": "Block"}));
    let mut edit = |id: &str, p: Value, warned: bool| {
        let r = run(&mut s, id, p);
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
        r
    };
    let r = edit("config.configure", json!({"params": "W", "features": ["Block"], "materials": null}), false);
    assert_eq!((r["active"].as_str(), r["columns"].as_array().map(Vec::len)), (Some("Default"), Some(2)), "{r}");
    edit("config.column", json!({"param": "H"}), false);
    edit("config.column", json!({"material": "A"}), false);
    let r = edit("config.column", json!({"material": "A", "delete": true}), false);
    assert_eq!(r["columns"].as_array().map(Vec::len), Some(3), "{r}");
    edit("config.row", json!({"name": "Big", "values": {"W": "20 mm", "suppress:Block": false}}), false);
    edit("config.row", json!({"name": "Bigger", "from": "Big", "values": {"param:H": "8 mm"}}), false);
    let r = edit("config.row", json!({"name": "Big", "rename": "Large"}), false);
    let rows: Vec<&str> = r["rows"].as_array().into_iter().flatten().filter_map(|row| row["name"].as_str()).collect();
    assert_eq!(rows, ["Default", "Large", "Bigger"], "{r}");
    edit("config.row", json!({"name": "Bigger", "delete": true}), false);
    edit("config.rule", json!({"rules": [always("r1", "H", "6 mm"), always("r2", "H", "7 mm")]}), false);
    let r = edit("config.rule", json!({"id": "r2", "index": 0}), false);
    assert_eq!((r["rules"][0]["id"].as_str(), r["rules"][1]["id"].as_str()), (Some("r2"), Some("r1")), "{r}");
    let r = edit("config.rule", json!({"id": "r1", "delete": true}), false);
    assert_eq!(r["rules"].as_array().map(Vec::len), Some(1), "{r}");
    edit("config.row", json!({"name": "Default", "valuse": {"W": "12 mm"}}), true);
    edit("config.column", json!({"param": "W", "delet": true}), true);
}
