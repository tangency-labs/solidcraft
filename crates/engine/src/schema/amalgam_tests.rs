use serde_json::json;

use super::AmalgamTagParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AmalgamTagParams>(), &["add", "name", "new", "participants", "remove"]);
    assert!(input_schema_for::<AmalgamTagParams>().as_value().get("required").is_none());
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
        json!({"remove": "Pocket", "seat": "Origin"}),
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
