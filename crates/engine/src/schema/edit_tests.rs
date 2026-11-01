use serde_json::json;

use super::{RepeatParams, SelectModeParams, SelectModeResult};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use crate::{PickMode, Session};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<RepeatParams>(), &[]);
    assert_keys(input_schema_for::<SelectModeParams>(), &["mode"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("select.mode", json!({"mode": "edges"})),
        ("select.mode", json!({"mode": "Bodies"})),
        ("select.mode", json!({"mode": 1})),
        ("select.mode", json!({"mode": null})),
        ("select.mode", json!({"mode": ["faces"]})),
    ]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<SelectModeResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for mode in PickMode::ALL {
        let r = s.execute("select.mode", &json!({"mode": mode})).unwrap_or_else(|e| panic!("{e}"));
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    let r = s.execute("select.mode", &json!({"mode": "parts", "modes": "bodies"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    for bad in [json!({"mode": "edges"}), json!({"mode": 1}), json!({})] {
        assert!(!conforms(schema, schema, &bad), "{bad}");
    }
}
