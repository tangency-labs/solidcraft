use serde_json::{Value, json};
use solidcraft_doc::render_settings::RenderSetup;

use super::{GetParams, RenderSettingsGot, RenderSettingsSaved, SETUP_FIELDS, SetParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A design with one saved render setup, `Studio`, 800 × 600.
fn studio(level: ParamCheck) -> Session {
    let mut s = Session::default();
    s.execute("render.settings.set", &json!({"name": "Studio", "set": {"width": 800, "height": 600}})).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = level;
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<GetParams>(), &["name"]);
    assert_keys(input_schema_for::<SetParams>(), &["name", "set"]);
}

/// #12783: `SETUP_FIELDS` is `RenderSetup`'s own list of fields, which serde gives in its message
/// for a field it doesn't know. The schemas and `patched_settings`' bound both come from it.
#[test]
fn the_setup_fields_are_render_setups_fields() {
    let error = serde_json::from_value::<RenderSetup>(json!({"not_a_setup_field": 1})).err().map(|e| e.to_string()).unwrap_or_default();
    let expected = error.split_once("expected").map(|(_, list)| list).unwrap_or_default();
    let fields: Vec<&str> = expected.split('`').skip(1).step_by(2).collect();
    assert_eq!(fields, SETUP_FIELDS, "{error}");
}

/// #12783: calls that fail today get the same message in strict: serde's for a name or patch of
/// the wrong type, the setup name rules', `patched_settings`' and a missing setup. (A key left out
/// is in `a_key_left_out_is_refused_by_the_check`.)
#[test]
fn strict_keeps_todays_errors() {
    let too_many: serde_json::Map<String, Value> = (0..16).map(|i| (format!("field{i}"), json!(1))).collect();
    assert_strict_keeps_todays_errors(&[
        ("render.settings.get", json!({"name": 5})),
        ("render.settings.get", json!({"name": ""})),
        ("render.settings.get", json!({"name": " Studio"})),
        ("render.settings.get", json!({"name": "Nope"})),
        ("render.settings.set", json!({"name": null, "set": {}})),
        ("render.settings.set", json!({"name": 5, "set": {}})),
        ("render.settings.set", json!({"name": "", "set": {}})),
        ("render.settings.set", json!({"name": "Studio", "set": null})),
        ("render.settings.set", json!({"name": "Studio", "set": []})),
        ("render.settings.set", json!({"name": "Studio", "set": too_many})),
        ("render.settings.set", json!({"name": "Studio", "set": {"widht": 800}})),
        ("render.settings.set", json!({"name": "Studio", "set": {"width": "wide"}})),
        ("render.settings.set", json!({"name": "Studio", "set": {"width": 0}})),
    ]);
}

/// #12783: Save Render Settings without `name` or `set` is refused by the check itself, before
/// the handler runs: in strict with the check's "is required", where the handler says serde's
/// "missing field". The first key left out, in the struct's order, is the one named either way.
#[test]
fn a_key_left_out_is_refused_by_the_check() {
    for (p, key) in [(json!({}), "name"), (json!({"set": {}}), "name"), (json!({"name": "Studio"}), "set")] {
        let messages: Vec<String> = crate::schema::issues("render.settings.set", &p).into_iter().map(|i| i.message).collect();
        assert_eq!(messages, [format!("`{key}` is required")], "{p}");
        let error = |level: ParamCheck| studio(level).execute("render.settings.set", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error(ParamCheck::Off).ends_with(&format!("missing field `{key}`")), "{p}: {}", error(ParamCheck::Off));
        assert!(error(ParamCheck::Strict).ends_with(&format!("`{key}` is required")), "{p}: {}", error(ParamCheck::Strict));
    }
}

/// #12783: the schemas take what the handlers take. Each call is checked against both the schema
/// and the handler on the design with `Studio`, with the check off, so the two can't drift apart.
#[test]
fn the_schemas_take_what_the_handlers_take() {
    let get = input_schema_for::<GetParams>();
    let set = input_schema_for::<SetParams>();
    for (id, schema, p) in [
        ("render.settings.get", &get, json!({})),
        ("render.settings.get", &get, json!({"name": null})),
        ("render.settings.get", &get, json!({"name": "Studio"})),
        ("render.settings.get", &get, json!({"name": 5})),
        ("render.settings.get", &get, json!({"name": ""})),
        ("render.settings.set", &set, json!({"name": "Studio", "set": {"width": 1024}})),
        ("render.settings.set", &set, json!({"name": "Draft", "set": {}})),
        ("render.settings.set", &set, json!({"name": "Studio", "set": {"exposure_ev": 0.5, "job": null}})),
        ("render.settings.set", &set, json!({"name": "Studio", "set": {"widht": 1024}})),
        ("render.settings.set", &set, json!({"name": "Studio", "set": []})),
        ("render.settings.set", &set, json!({"name": "Studio"})),
    ] {
        let takes = studio(ParamCheck::Off).execute(id, &p).is_ok();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{id} {p}");
    }
}

/// #12783: both commands reply as their output schemas say, in Warn without warnings, and the
/// replies are the JSON the handlers gave before: every setup with the defaults, one setup by
/// name, and a setup changed and saved again unchanged. Unknown keys fail in every mode, as the
/// handlers' serde structs refuse them; strict refuses them in the check's own words.
#[test]
fn render_settings_reply_with_their_schemas() {
    let got = output_schema_for::<RenderSettingsGot>();
    let got = got.as_value();
    let saved = output_schema_for::<RenderSettingsSaved>();
    let saved = saved.as_value();
    let mut s = studio(ParamCheck::Warn);
    let mut run = |id: &str, p: Value, schema: &Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let all = run("render.settings.get", json!({}), got);
    let one = run("render.settings.get", json!({"name": "Studio"}), got);
    let changed = run("render.settings.set", json!({"name": "Studio", "set": {"width": 1024}}), saved);
    let unchanged = run("render.settings.set", json!({"name": "Studio", "set": {"width": 1024}}), saved);
    assert_eq!(changed, unchanged, "saved again unchanged");
    assert_eq!(changed["setup"]["width"], 1024, "{changed}");
    let setups = s.doc.render.setups.clone();
    let studio_before = {
        let mut setup = setups.get("Studio").cloned().unwrap_or_default();
        setup.width = 800;
        setup
    };
    assert_eq!(all, json!({"setups": {"Studio": studio_before}, "defaults": RenderSetup::default()}));
    assert_eq!(one, json!({"name": "Studio", "setup": studio_before}));
    assert_eq!(changed, json!({"name": "Studio", "setup": setups.get("Studio")}));
    for level in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        s.param_check = level;
        let error = s.execute("render.settings.get", &json!({"nmae": "Studio"})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("nmae"), "{level:?}: {error}");
    }
}
