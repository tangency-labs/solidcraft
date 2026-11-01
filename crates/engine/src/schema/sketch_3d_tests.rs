use serde_json::json;

use super::{AlongAxisParams, AlongAxisResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AlongAxisParams>(), &["axis", "line", "lines", "sketch"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.constraint.along_axis", json!({"line": "w1"})),
        ("sketch.constraint.along_axis", json!({"line": "w1", "axis": "w"})),
        ("sketch.constraint.along_axis", json!({"line": 5, "axis": "x"})),
        ("sketch.constraint.along_axis", json!({"axis": "x"})),
    ]);
}

#[test]
fn along_axis_reply_matches_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let l = run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [10, 1, 2]]}));
    let line = l["curves"][0].as_str().unwrap_or_else(|| panic!("{l}")).to_string();
    let r = run(&mut s, "sketch.constraint.along_axis", json!({"line": line, "axis": "x"}));
    let schema = output_schema_for::<AlongAxisResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
}
