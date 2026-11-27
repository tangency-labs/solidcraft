use serde_json::{Value, json};

use super::{AgingDescription, EffectsDescription, NoParams, RenderBackends, RenderSceneInfo};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// The four commands, each with its output schema.
fn commands() -> [(&'static str, schemars::Schema); 4] {
    [
        ("render.backends", output_schema_for::<RenderBackends>()),
        ("render.scene", output_schema_for::<RenderSceneInfo>()),
        ("render.aging.describe", output_schema_for::<AgingDescription>()),
        ("render.effects.describe", output_schema_for::<EffectsDescription>()),
    ]
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<NoParams>(), &[]);
}

/// #12832: each command replies as its output schema says, in Warn without warnings, on an empty
/// design and on one with a 10 mm cube. Inspect Render Scene lists the cube with its bounds, and
/// none on the empty design.
#[test]
fn render_information_replies_with_its_schemas() {
    for with_cube in [false, true] {
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        if with_cube {
            s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"})).unwrap_or_else(|e| panic!("{e}"));
        }
        for (id, schema) in commands() {
            let r = s.execute(id, &json!({})).unwrap_or_else(|e| panic!("{id}: {e}"));
            let schema = schema.as_value();
            assert!(conforms(schema, schema, &r), "{id}: {r} doesn't match {schema}");
            assert!(r.get("param_warnings").is_none(), "{id}: {r}");
        }
        let scene = s.execute("render.scene", &json!({})).unwrap_or_default();
        if with_cube {
            assert_eq!((&scene["bodies"][0]["name"], &scene["bodies"][0]["ordinal"]), (&json!("Cube"), &json!(0)), "{scene}");
            assert_eq!(scene["bounds"], json!({"min": [0.0, 0.0, 0.0], "max": [10.0, 10.0, 10.0]}), "{scene}");
        } else {
            assert_eq!(scene, json!({"bodies": [], "bounds": null, "overlays": false}));
        }
    }
    let s = &mut Session::default();
    let backends = s.execute("render.backends", &json!({})).unwrap_or_default();
    assert_eq!((&backends["schema_version"], &backends["default_backend"]), (&json!(1), &json!("builtin")), "{backends}");
    assert_eq!(backends["backends"].as_array().map(Vec::len), Some(1), "{backends}");
    for (id, list) in [("render.aging.describe", "presets"), ("render.effects.describe", "looks")] {
        let r = s.execute(id, &json!({})).unwrap_or_default();
        assert!(r[list].as_array().is_some_and(|l| !l.is_empty() && l.iter().all(Value::is_object)), "{id}: {r}");
    }
}

/// #12832: the handlers take an empty object only, and refuse any key in every mode: with their
/// own message with the check off, and in strict with the check's wording for an unknown key.
#[test]
fn any_key_is_refused_in_every_mode() {
    for (id, _) in commands() {
        let p = json!({"verbose": true});
        let off = Session::default().execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(off.contains("empty"), "{id}: {off}");
        let strict = Session { param_check: ParamCheck::Strict, ..Session::default() }.execute(id, &p).err().map(|e| e.to_string());
        assert!(strict.is_some_and(|e| e.contains("`verbose`")), "{id}");
    }
}
