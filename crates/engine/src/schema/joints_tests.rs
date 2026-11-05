use serde_json::json;

use super::{EditJointParams, EditJointResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EditJointParams>(), &["a", "angle", "b", "flip", "joint", "name", "offset", "quiet", "suppressed", "type"]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<EditJointResult>();
    let schema = schema.as_value();
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("component.create", &json!({"name": "B", "grounded": false})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("joint.create", &json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}))
        .unwrap_or_else(|e| panic!("{e}"));
    let full = s.execute("joint.edit", &json!({"joint": "Stack", "flip": true})).unwrap_or_else(|e| panic!("{e}"));
    assert!(conforms(schema, schema, &full), "{full} doesn't match {schema}");
    let quiet = s.execute("joint.edit", &json!({"joint": "Stack", "quiet": true})).unwrap_or_else(|e| panic!("{e}"));
    assert!(quiet.get("dof").is_none() && conforms(schema, schema, &quiet), "{quiet}");
}
