use serde_json::json;

use super::{AmalgamTagParams, AmalgamateParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AmalgamTagParams>(), &["add", "name", "new", "participants", "remove", "seat"]);
    assert!(input_schema_for::<AmalgamTagParams>().as_value().get("required").is_none());
}

#[test]
fn the_amalgamate_schema_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<AmalgamateParams>(),
        &["add_to", "configuration", "flip", "name", "participants", "path", "remove_from", "seeds", "tag", "turns"],
    );
    let schema = input_schema_for::<AmalgamateParams>();
    let mut required: Vec<&str> = schema.as_value()["required"].as_array().unwrap().iter().filter_map(serde_json::Value::as_str).collect();
    required.sort_unstable();
    assert_eq!(required, ["path", "seeds"]);
}

/// Calls to `solid.amalgamate` that fail before any file is read get the same message in strict.
#[test]
fn amalgamate_strict_keeps_todays_errors() {
    let at = json!([{"sketch": "S", "point": "p"}]);
    let cases = [
        json!({}),
        json!({"seeds": at}),
        json!({"path": "tool.sc"}),
        json!({"path": "", "seeds": at}),
        json!({"path": "tool.sc", "seeds": []}),
        json!({"path": "tool.sc", "seeds": 5}),
        json!({"path": "tool.sc", "seeds": [{"sketch": "S"}]}),
        json!({"path": "tool.sc", "seeds": [{"face": [1, 2]}]}),
        json!({"path": "tool.sc", "seeds": at, "turns": 4}),
        json!({"path": "tool.sc", "seeds": at, "turns": -1}),
        json!({"path": "tool.sc", "seeds": at, "flip": "yes"}),
        json!({"path": "tool.sc", "seeds": at, "configuration": true}),
        json!({"path": "tool.sc", "seeds": at, "tag": ""}),
        json!({"path": "tool.sc", "seeds": at, "remove_from": 5}),
        json!({"path": "tool.sc", "seeds": at, "add_to": [true]}),
        json!({"path": "tool.sc", "seeds": at, "nope": 1}),
    ];
    for p in &cases {
        let error = |level: ParamCheck| scene(level).execute("solid.amalgamate", p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// A box `Pocket` to tag.
fn scene(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Pocket"})).unwrap();
    s
}

/// Calls that fail today get the same message in strict.
#[test]
fn strict_keeps_todays_errors() {
    let cases = [
        json!({}),
        json!({"remove": 5}),
        json!({"remove": ["Pocket", 5]}),
        json!({"add": true}),
        json!({"new": {"body": "Pocket"}}),
        json!({"remove": null, "add": null, "new": null}),
        json!({"remove": "Pocket", "sea": 1}),
        json!({"remove": "Pocket", "seat": 5}),
        json!({"remove": "Pocket", "seat": "Origin"}),
        json!({"remove": "Pocket", "seat": {"sketch": "S"}}),
        json!({"remove": "Pocket", "seat": {"sketch": "Nope", "point": "p"}}),
        json!({"remove": "Pocket", "seat": {"face": [1, 2]}}),
        json!({"remove": "Pocket", "name": 7}),
        json!({"remove": "Pocket", "participants": 5}),
        json!({"remove": "Pocket", "body_name": "X"}),
    ];
    for p in &cases {
        let error = |level: ParamCheck| scene(level).execute("solid.amalgam_tag", p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

#[test]
fn the_reply_is_a_feature_result() {
    let mut s = scene(ParamCheck::Off);
    let reply = s.execute("solid.amalgam_tag", &json!({"remove": "Pocket"})).unwrap();
    let schema = output_schema_for::<FeatureResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
}
