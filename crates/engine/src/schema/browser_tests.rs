use serde_json::{Value, json};

use super::{RepairParams, RepairResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<RepairParams>(), &["body", "body_name", "body_names", "gap", "name", "participants", "patch_holes"]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<RepairResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"})).unwrap();
    s.execute("surface.unstitch", &json!({"body": "Box", "faces": [[5, 5, 10]]})).unwrap();
    s.param_check = ParamCheck::Warn;
    let r = s.execute("body.repair", &json!({"body": "Box", "gap": "0.1 mm", "patch_holes": true, "name": "Mend"})).unwrap();
    assert!(r.get("param_warnings").is_none(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    let r = s.execute("body.repair", &json!({"body": "Box", "colour": "red"}));
    assert!(r.is_err(), "nothing left to repair");
}
