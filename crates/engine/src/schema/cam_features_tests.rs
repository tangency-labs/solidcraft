//! Tests of the `cam.features` contract: its keys, and replies that match its output schema.

use serde_json::{Value, json};

use super::{CamFeaturesParams, CamFeaturesReply};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CamFeaturesParams>(), &["axis", "bodies", "origin", "picks", "tolerance"]);
}

/// The command replies as its output schema says, in Warn without warnings; strict refuses a
/// misspelt key.
#[test]
fn replies_match_the_output_schema() {
    let matches = |schema: schemars::Schema, r: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [-20, -15, -10]}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [0, 0, -11], "operation": "cut"}));
    run(&mut s, "solid.box", json!({"length": 16, "width": 12, "height": 4, "corner": [4, -6, -4], "operation": "cut"}));
    matches(output_schema_for::<CamFeaturesReply>(), &run(&mut s, "cam.features", json!({})));
    matches(output_schema_for::<CamFeaturesReply>(), &run(&mut s, "cam.features", json!({"axis": "a"})));
    matches(output_schema_for::<CamFeaturesReply>(), &run(&mut s, "cam.features", json!({"picks": true})));
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let e = strict.execute("cam.features", &json!({"axes": "z"})).unwrap_err();
    assert!(e.to_string().contains("axes"), "{e}");
}
