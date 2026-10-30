use serde_json::json;

use super::*;
use crate::schema::tests::{assert_keys, conforms, drawing_session};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<TemplateListParams>(), &[]);
    assert_keys(input_schema_for::<TemplateApplyParams>(), &["dry_run", "template"]);
}

/// A call that already fails gets the same error in strict as with the check off.
#[test]
fn strict_keeps_todays_errors() {
    for p in [json!({"template": "iso-b2-part"}), json!({"template": ""})] {
        let error = |level| drawing_session(level).0.execute("drawing.template.apply", &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// Applying a template, as a dry run and for real, matches the output schema with no warnings in
/// warn mode.
#[test]
fn the_template_apply_reply_matches_its_output_schema() {
    let (mut s, _) = drawing_session(ParamCheck::Warn);
    let schema = output_schema_for::<TemplateApplyResult>();
    let schema = schema.as_value();
    for p in [json!({"template": "iso-a2-part", "dry_run": true}), json!({"template": "iso-a2-part"}), json!({"template": "iso-a2-part"})] {
        let r = s.execute("drawing.template.apply", &p).unwrap();
        assert!(conforms(schema, schema, &r) && r.get("param_warnings").is_none(), "{p}: {r} doesn't match {schema}");
    }
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
