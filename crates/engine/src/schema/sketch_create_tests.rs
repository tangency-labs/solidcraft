use serde_json::json;

use super::{SplineReverseParams, SplineReverseResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SplineReverseParams>(), &["curve", "sketch"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.spline.reverse", json!({"curve": 5})),
        ("sketch.spline.reverse", json!({"curve": null})),
        ("sketch.spline.reverse", json!({"curve": "nope"})),
        ("sketch.spline.reverse", json!({"curve": "l1"})),
    ]);
}

#[test]
fn spline_reverse_reply_matches_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let c = run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 5], [20, 0]]}));
    let spline = c["curves"][0].as_str().unwrap_or_else(|| panic!("{c}")).to_string();
    let schema = output_schema_for::<SplineReverseResult>();
    let schema = schema.as_value();
    let r = run(&mut s, "sketch.spline.reverse", json!({"curve": spline}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    let r = run(&mut s, "sketch.spline.reverse", json!({"curve": spline, "unknown": 1}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
}
