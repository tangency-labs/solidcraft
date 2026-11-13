use serde_json::{Value, json};

use super::{MetadataParams, MetadataRemoveParams, MetadataRemoveResult, MetadataResult, MetadataSetParams, MetadataSetResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MetadataParams>(), &[]);
    assert_keys(input_schema_for::<MetadataSetParams>(), &["key", "value", "values"]);
    assert_keys(input_schema_for::<MetadataRemoveParams>(), &["key", "keys"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("document.metadata.set", json!({"key": 5, "value": "Gearbox"})),
        ("document.metadata.remove", json!({"key": 5})),
    ]);
}

/// Every form of the three commands, in Warn: each reply matches its output schema and carries
/// no warning; a misspelt key's warning still matches.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, reply: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{reply} doesn't match {schema}");
    };
    let set = |reply: &Value| matches(output_schema_for::<MetadataSetResult>(), reply);
    let remove = |reply: &Value| matches(output_schema_for::<MetadataRemoveResult>(), reply);
    let calls = [
        ("document.metadata.set", json!({"values": {"author": "Jane Doe", "com.acme.shop": {"shop_id": 88123}}})),
        ("document.metadata.set", json!({"key": "project", "value": "Gearbox"})),
        ("document.metadata.set", json!({"key": "project", "value": null})),
        ("document.metadata.remove", json!({"keys": ["author", "author", "not.there"]})),
        ("document.metadata.remove", json!({"key": "com.acme.shop"})),
    ];
    for (id, p) in calls {
        let reply = run(id, p.clone());
        assert!(reply.get("param_warnings").is_none(), "{id} {p}: {reply}");
        if id == "document.metadata.set" { set(&reply) } else { remove(&reply) }
    }
    run("document.metadata.set", json!({"key": "part_number", "value": "ACME-1042"}));
    let read = run("document.metadata", json!({}));
    assert_eq!(read["properties"]["part_number"], "ACME-1042");
    matches(output_schema_for::<MetadataResult>(), &read);
    let warned = run("document.metadata.set", json!({"key": "author", "value": "J", "valeu": 1}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    set(&warned);
}
