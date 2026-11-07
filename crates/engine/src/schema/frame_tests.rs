use serde_json::{Value, json};

use super::{ProfileListParams, ProfileListResult, ProfilePropertiesParams, ProfileSection};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// The reply carries no warnings and matches its schema.
fn assert_conforms(reply: &Value, schema: &schemars::Schema) {
    assert!(reply.get("param_warnings").is_none(), "{reply}");
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{reply} doesn't match {schema}");
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ProfileListParams>(), &[]);
    assert_keys(input_schema_for::<ProfilePropertiesParams>(), &["density", "profile"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3});
    assert_strict_keeps_todays_errors(&[
        ("frame.profile.properties", json!({"profile": 5})),
        ("frame.profile.properties", json!({"profile": {"width": 40}})),
        ("frame.profile.properties", json!({"profile": {"kind": "hexagon"}})),
        ("frame.profile.properties", json!({"profile": {"kind": "bar", "width": 40, "height": 20, "wall": 3}})),
        ("frame.profile.properties", json!({"profile": {"kind": "bar", "width": true, "height": 20}})),
        ("frame.profile.properties", json!({"profile": tube, "density": -1})),
        ("frame.profile.properties", json!({"profile": tube, "density": "steel"})),
        ("frame.profile.properties", json!({"profile": tube, "density": 2e6})),
    ]);
}

#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let list = s.execute("frame.profile.list", &json!({})).unwrap();
    assert_conforms(&list, &output_schema_for::<ProfileListResult>());
    let section = output_schema_for::<ProfileSection>();
    for kind in list["kinds"].as_array().unwrap() {
        let reply = s.execute("frame.profile.properties", &json!({"profile": kind["example"]})).unwrap();
        assert_conforms(&reply, &section);
        assert!(reply.get("mass_per_m").is_none(), "{reply}");
    }
    let tube = json!({"kind": "rect_tube", "width": "40", "height": 40, "wall": "3"});
    let reply = s.execute("frame.profile.properties", &json!({"profile": tube, "density": 7850})).unwrap();
    assert_conforms(&reply, &section);
    let area = reply["area"].as_f64().unwrap();
    assert!((reply["mass_per_m"].as_f64().unwrap() - area * 7850e-6).abs() < 1e-9, "{reply}");
}

#[test]
fn a_misspelt_key_is_reported_not_ignored() {
    let p = json!({"profile": {"kind": "round_bar", "diameter": 20}, "densty": 7850});
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let reply = s.execute("frame.profile.properties", &p).unwrap();
    assert!(reply["param_warnings"].to_string().contains("densty"), "{reply}");
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    assert!(s.execute("frame.profile.properties", &p).is_err());
}
