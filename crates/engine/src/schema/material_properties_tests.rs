use serde_json::{Value, json};

use super::{CardModulus, CardValidated, DirectionalModulusParams, ValidateCardParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// An isotropic card, as the command's own tests write one.
fn card() -> Value {
    json!({"schema_version":1,"designation":"Original synthetic card, not design data","family":"other",
        "sources":[{"id":"fixture","title":"Original analytic constants","kind":"illustrative","license":"MIT OR Apache-2.0"}],
        "properties":[{"source_id":"fixture","conditions":{"temperature_k":293.15,"state":"synthetic"},
            "data":{"kind":"density","kg_m3":700.0}},
            {"source_id":"fixture","data":{"kind":"elasticity","model":{
                "symmetry":"isotropic","young_modulus_pa":12.25e9,"poisson_ratio":0.25}}}]})
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ValidateCardParams>(), &["card"]);
    assert_keys(input_schema_for::<DirectionalModulusParams>(), &["card", "direction"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("material.properties.validate", json!({"card": {}})),
        ("material.properties.validate", json!({"card": null})),
        ("material.properties.directional", json!({"card": {}, "direction": [1, 0, 0]})),
        ("material.properties.directional", json!({"card": card(), "direction": [1, 0]})),
        ("material.properties.directional", json!({"card": card(), "direction": "x"})),
        ("material.properties.directional", json!({"card": card(), "direction": [1, "a", 0]})),
        ("material.properties.directional", json!({"card": card(), "direction": [0, 0, 0]})),
    ]);
}

/// #11036: both commands reply as their output schemas say, in Warn without warnings. Like the
/// handlers, warn and strict refuse a key they don't take, so no reply carries a warning.
#[test]
fn card_commands_reply_with_their_schemas() {
    let (validated, modulus) = (output_schema_for::<CardValidated>(), output_schema_for::<CardModulus>());
    let matches = |schema: &schemars::Schema, r: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let r = s.execute("material.properties.validate", &json!({"card": card()})).unwrap_or_else(|e| panic!("{e}"));
    matches(&validated, &r);
    assert!(r["valid"] == true && r["validation"]["isotropic_moduli"]["shear_pa"].is_number(), "{r}");
    let r = s.execute("material.properties.directional", &json!({"card": card(), "direction": [1, 1, 0]})).unwrap_or_else(|e| panic!("{e}"));
    matches(&modulus, &r);
    assert!(r["axes"].is_null() && (r["young_modulus_pa"].as_f64().unwrap_or(0.0) - 12.25e9).abs() < 1.0, "{r}");
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = Session { param_check: level, ..Session::default() };
        assert!(s.execute("material.properties.validate", &json!({"card": card(), "crd": 1})).is_err(), "{level:?}");
    }
}
