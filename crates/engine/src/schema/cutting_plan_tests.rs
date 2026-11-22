use super::*;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use serde_json::json;

#[test]
fn cutting_plan_schema_matches_strict_command_reply() {
    assert_keys(input_schema_for::<LayoutParams>(), &["kerf", "labels_path", "library", "mode", "path", "stock", "trim"]);
    let mut s = crate::Session { param_check: ParamCheck::Strict, ..crate::Session::default() };
    let reply = s.execute("cutlist.layout", &json!({})).unwrap();
    let schema = output_schema_for::<Layout>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply));
}
