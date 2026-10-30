use serde_json::json;

use super::{EquationCurveParams, EquationCurveResult, EquationEditParams, EquationEditResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EquationCurveParams>(), &["construction", "end", "explicit", "sketch", "start", "tolerance", "x", "y"]);
    assert_keys(input_schema_for::<EquationEditParams>(), &["curve", "end", "explicit", "sketch", "start", "tolerance", "x", "y"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.equation_curve", json!({"x": ["t"], "y": "t", "start": 0, "end": 1})),
        ("sketch.equation_curve", json!({"x": "t", "y": " ", "start": 0, "end": 1})),
        ("sketch.equation_curve", json!({"x": "t", "y": "t", "start": 0, "end": 1, "tolerance": 0})),
        ("sketch.equation_curve", json!({"x": "t", "y": "t", "start": 0, "end": 1, "tolerance": "big"})),
        ("sketch.equation_curve", json!({"x": "t", "y": "t", "start": 0, "end": 1, "explicit": 1})),
        ("sketch.equation_curve", json!({"x": "t", "y": "t", "start": 0, "end": 1e12})),
        ("sketch.equation_curve", json!({"y": "t", "start": 0, "end": 1})),
        ("sketch.equation_curve.edit", json!({"curve": 7})),
        ("sketch.equation_curve.edit", json!({"curve": "nope", "y": "t"})),
    ]);
}

#[test]
fn replies_match_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let created = output_schema_for::<EquationCurveResult>();
    let created = created.as_value();
    let r = run(&mut s, "sketch.equation_curve", json!({"x": "t * cos(t)", "y": "t * sin(t)", "start": 0, "end": "4 * pi"}));
    assert!(conforms(created, created, &r), "{r} doesn't match {created}");
    let id = r["curve"].as_str().unwrap_or_else(|| panic!("{r}")).to_string();
    let r = run(&mut s, "sketch.equation_curve", json!({"explicit": true, "y": "x", "start": 0, "end": 1, "unknown": 1}));
    assert!(conforms(created, created, &r), "{r} doesn't match {created}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    let edited = output_schema_for::<EquationEditResult>();
    let edited = edited.as_value();
    let r = run(&mut s, "sketch.equation_curve.edit", json!({"curve": id, "end": "6 * pi"}));
    assert!(conforms(edited, edited, &r), "{r} doesn't match {edited}");
}
