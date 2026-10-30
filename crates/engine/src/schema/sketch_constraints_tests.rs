use serde_json::json;

use super::{ToggleDiameterParams, ToggleDiameterResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ToggleDiameterParams>(), &["constraint", "diameter", "sketch"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("sketch.toggle_diameter", json!({"constraint": 5})),
        ("sketch.toggle_diameter", json!({"constraint": null})),
        ("sketch.toggle_diameter", json!({"constraint": "c1"})),
        ("sketch.toggle_diameter", json!({"constraint": "c1", "diameter": true})),
    ]);
}

#[test]
fn toggle_diameter_reply_matches_its_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: serde_json::Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let c = run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
    let circle = c["curves"][0].as_str().unwrap_or_else(|| panic!("{c}")).to_string();
    let d = run(&mut s, "sketch.dimension", json!({"entities": [circle]}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let did = g["glyphs"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|x| x["param"] == d["param"])
        .and_then(|x| x["id"].as_str())
        .unwrap_or_else(|| panic!("{g}"))
        .to_string();
    let schema = output_schema_for::<ToggleDiameterResult>();
    let schema = schema.as_value();
    let r = run(&mut s, "sketch.toggle_diameter", json!({"constraint": did}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!(r["diameter"], false, "{r}");
    let r = run(&mut s, "sketch.toggle_diameter", json!({"constraint": did, "unknown": 1}));
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
}
