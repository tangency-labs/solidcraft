use serde_json::json;

use super::*;
use crate::schema::tests::{assert_keys, conforms, drawing_session};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TemplateListParams>(), &[]);
}

/// The list matches its output schema, with no warnings in warn mode; strict refuses a key it
/// doesn't take.
#[test]
fn the_template_list_matches_its_output_schema() {
    let (mut s, _) = drawing_session(ParamCheck::Warn);
    let schema = output_schema_for::<TemplateListResult>();
    let schema = schema.as_value();
    let r = s.execute("drawing.template.list", &json!({})).unwrap();
    assert!(conforms(schema, schema, &r) && r.get("param_warnings").is_none(), "{r} doesn't match {schema}");
    assert_eq!(r["templates"].as_array().map(Vec::len), Some(10));
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("drawing.template.list", &json!({"kind": "part"})).is_err());
}
