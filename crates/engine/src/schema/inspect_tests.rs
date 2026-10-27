use serde_json::json;

use super::DescribeParams;
use crate::Session;
use crate::schema::input_schema_for;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DescribeParams>(), &["command"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("engine.describe", json!({"command": 5})),
        ("engine.describe", json!({"command": null})),
        ("engine.describe", json!({"command": "solid.extrud"})),
    ]);
}

#[test]
fn describe_returns_both_schemas() {
    let mut s = Session::default();
    let d = s.execute("engine.describe", &json!({"command": "solid.box"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["id"], "solid.box");
    assert!(d["input"]["properties"].get("length").is_some());
    assert!(d["output"]["properties"].get("created_bodies").is_some());
    // Its own output schema lists the warnings warn mode can add.
    let own = s.execute("engine.describe", &json!({"command": "engine.describe"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(own["output"]["properties"].get("param_warnings").is_some(), "{own}");
    let d = s.execute("engine.describe", &json!({"command": "solid.revolve"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(d["input"].is_null() && d["output"].is_null(), "unconverted commands have no schemas yet");
    let e = s.execute("engine.describe", &json!({"command": "solid.extrud"})).err().map(|e| e.to_string());
    assert!(e.as_deref().is_some_and(|e| e.contains("did you mean `solid.extrude`")), "{e:?}");
}

#[test]
fn feature_effects_publish_schemas_and_check_their_contract() {
    assert_keys(input_schema_for::<super::FeatureEffectsParams>(), &["feature"]);
    let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
    let description = s.execute("engine.describe", &json!({"command": "timeline.effects"})).unwrap();
    for field in ["feature", "evaluated", "created_bodies", "changed_bodies", "removed_bodies"] {
        assert!(description["output"]["properties"].get(field).is_some(), "{field}");
    }
    assert!(s.execute("timeline.effects", &json!({"feature": 1, "typo": true})).unwrap_err().to_string().contains("unknown key"));
    let feature = s.execute("solid.box", &json!({"length": 1, "width": 1, "height": 1})).unwrap()["feature"].clone();
    s.param_check = crate::schema::ParamCheck::Warn;
    let reply = s.execute("timeline.effects", &json!({"feature": feature, "typo": true})).unwrap();
    let schema = &description["output"];
    assert!(crate::schema::tests::conforms(schema, schema, &reply), "{reply}");
}
