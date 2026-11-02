use serde_json::{Value, json};

use super::{DocumentUnitsParams, DocumentUnitsResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DocumentUnitsParams>(), &["units"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[("document.units", json!({})), ("document.units", json!({"units": "furlong"}))]);
}

/// A change that keeps a parameter's value with the old unit, one that keeps none, and the same
/// units again, in Warn: each reply matches the output schema and carries no warning.
#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<DocumentUnitsResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |p: Value| s.execute("document.units", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let mut check = |p: Value| {
        let reply = run(p.clone());
        assert!(reply.get("param_warnings").is_none() && conforms(schema, schema, &reply), "{p}: {reply} doesn't match {schema}");
        reply
    };
    check(json!({"units": "cm"}));
    assert_eq!(check(json!({"units": "cm"}))["kept"], json!([]));
    check(json!({"units": "in"}));
}

/// A unit-less parameter a length reads is kept with the old unit, and the reply names it.
#[test]
fn the_reply_names_the_parameters_kept_in_the_old_unit() {
    let mut s = Session::default();
    s.execute("parameters.add", &json!({"name": "w", "expression": "10", "unit": ""})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": "w", "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let reply = s.execute("document.units", &json!({"units": "in"})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<DocumentUnitsResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["kept"], json!(["w"]), "{reply}");
}
