use serde_json::json;

use super::{AssemblyMassParams, AssemblyMassResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AssemblyMassParams>(), &[]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<AssemblyMassResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let empty = s.execute("assembly.mass_properties", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert!(conforms(schema, schema, &empty), "{empty} doesn't match {schema}");
    assert!(empty.get("param_warnings").is_none(), "{empty}");
    s.execute("component.create", &json!({"name": "Block"})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let r = s.execute("assembly.mass_properties", &json!({"unknown_extra": 1})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    s.param_check = ParamCheck::Strict;
    assert!(s.execute("assembly.mass_properties", &json!({"unknown_extra": 1})).is_err());
}
