use serde_json::{Value, json};

use super::{PatternLimitParams, PatternLimitResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PatternLimitParams>(), &["value"]);
}

/// A limit outside 2 to 20 000, a fraction or anything but a number: strict gives the handler's
/// own message.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.pattern_limit", json!({"value": 1})),
        ("sketch.pattern_limit", json!({"value": 20_001})),
        ("sketch.pattern_limit", json!({"value": 2.5})),
        ("sketch.pattern_limit", json!({"value": -3})),
        ("sketch.pattern_limit", json!({"value": "many"})),
        ("sketch.pattern_limit", json!({"value": true})),
    ]);
}

#[test]
fn pattern_limit_replies_match_the_output_schema() {
    let schema = output_schema_for::<PatternLimitResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let reply = |s: &mut Session, p: Value| s.execute("sketch.pattern_limit", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let r = reply(&mut s, json!({}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!(r.get("param_warnings"), None, "{r}");
    assert_eq!(r["pattern_limit"], json!(500));
    for (p, limit) in [(json!({"value": 800}), 800), (json!({}), 800), (json!({"value": null}), 800), (json!({"value": 2.0}), 2)] {
        let r = reply(&mut s, p.clone());
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings"), None, "{p}: {r}");
        assert_eq!(r["pattern_limit"], json!(limit), "{p}");
    }
    assert!(!conforms(schema, schema, &json!({})));
}
