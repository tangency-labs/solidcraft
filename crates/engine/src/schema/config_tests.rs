use serde_json::{Value, json};

use super::{ConfigActivation, ConfigRowParams, ConfigTableParams, ConfigTableReply};
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
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("config.activate", json!({})),
        ("config.activate", json!({"row": 5})),
        ("config.activate", json!({"row": "Nope"})),
    ]);
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
