use serde_json::{Value, json};

use super::{PresetParams, QualityParams, QualityPlan, StudioComposition};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A design with a box, `Subject`, and a saved render setup, `Shot`, that renders with the GPU
/// path tracer, at the parameter-check level `level`.
fn with_a_shot(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"length": 20, "width": 15, "height": 10, "body_name": "Subject"})),
        ("render.settings.set", json!({"name": "Shot", "set": {"width": 96, "height": 64, "job": {"method": "gpu_path"}}})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

/// `render.quality.prepare`'s call for the profile `id`.
fn budget(id: &str) -> Value {
    json!({"id": id, "variant": "fixed_samples_png", "acknowledge_omissions": true})
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PresetParams>(), &["expected_revision", "id", "name", "replace_lighting_and_environment", "replace_stage", "set"]);
    assert_keys(input_schema_for::<QualityParams>(), &["acknowledge_omissions", "id", "variant"]);
}

/// The handlers refuse these calls after reading them; strict refuses them with their message.
/// They need a body, a saved setup and the design's revision, so the shared helper, which has a
/// sketch only, doesn't fit. None of them reaches the bundled render library.
#[test]
fn strict_keeps_todays_errors() {
    let cases: Vec<(&str, Box<dyn Fn(u64) -> Value>)> = vec![
        ("render.quality.prepare", Box::new(|_| json!({"id": "Q-002", "variant": "full", "acknowledge_omissions": true}))),
        ("render.quality.prepare", Box::new(|_| json!({"id": "Q-002", "variant": "fixed_samples_png", "acknowledge_omissions": false}))),
        ("render.quality.prepare", Box::new(|_| budget("Q-999"))),
        ("render.quality.prepare", Box::new(|_| budget("not an id"))),
        ("render.quality.prepare", Box::new(|_| budget("Q-004"))),
        ("render.preset.resolve", Box::new(|_| json!({"id": "NATIVE-STUDIO-999", "name": "Shot"}))),
        ("render.preset.resolve", Box::new(|_| json!({"id": "NATIVE-STUDIO-001", "name": "Shot", "expected_revision": 999_999}))),
        ("render.preset.resolve", Box::new(|_| json!({"id": "NATIVE-STUDIO-001", "name": "Raster"}))),
        ("render.preset.resolve", Box::new(|_| json!({"id": "NATIVE-STUDIO-001", "name": "Shot", "set": 5}))),
        ("render.preset.resolve", Box::new(|_| json!({"id": "NATIVE-STUDIO-001", "name": " Shot"}))),
        ("render.preset.apply", Box::new(|_| json!({"id": "NATIVE-STUDIO-001", "name": "Shot", "replace_lighting_and_environment": true}))),
        ("render.preset.apply", Box::new(|r| json!({"id": "NATIVE-STUDIO-001", "name": "Shot", "expected_revision": r}))),
    ];
    for (id, p) in &cases {
        let error = |level: ParamCheck| {
            let mut s = with_a_shot(level);
            let p = p(s.revision);
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {} should fail today", p(0));
        assert_eq!(error(ParamCheck::Strict), off, "{id} {}", p(0));
    }
}

/// #11766: the quality budgets the GPU path tracer meets reply as their schema says, in Warn
/// without warnings; with the bundled render library, so do a native studio's review and its
/// application. Like the handlers, warn and strict refuse a misspelt key.
#[test]
fn preset_commands_reply_with_their_schemas() {
    let (composition, plan) = (output_schema_for::<StudioComposition>(), output_schema_for::<QualityPlan>());
    let mut s = with_a_shot(ParamCheck::Warn);
    let run = |s: &mut Session, schema: &schemars::Schema, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    // The profiles a fixed budget can meet (`studio_presets_tests` checks the others refuse).
    for id in ["Q-001", "Q-002", "Q-003"] {
        let r = run(&mut s, &plan, "render.quality.prepare", budget(id));
        assert!(r["name"].is_string() && r["gpu_options"]["denoise"].is_boolean(), "{r}");
    }
    if cfg!(feature = "bundled-render-library") {
        let revision = s.revision;
        let review = json!({"id": "NATIVE-STUDIO-004", "name": "Shot", "set": {}, "expected_revision": revision});
        let r = run(&mut s, &composition, "render.preset.resolve", review);
        assert!(r["document_changed"] == false && r["rig_projection"].is_object(), "a studio with a rig: {r}");
        let request = json!({"id": "NATIVE-STUDIO-004", "name": "Shot", "expected_revision": revision,
            "replace_lighting_and_environment": true, "replace_stage": false});
        assert_eq!(run(&mut s, &composition, "render.preset.apply", request)["document_changed"], true);
        let plain = json!({"id": "NATIVE-STUDIO-001", "name": "Plain", "set": {"job": {"method": "gpu_path"}}});
        assert!(run(&mut s, &composition, "render.preset.resolve", plain)["rig_projection"].is_null(), "a studio without a rig");
    }
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut p = budget("Q-001");
        p["acknowledge_omision"] = json!(true);
        let error = with_a_shot(level).execute("render.quality.prepare", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("`acknowledge_omision`"), "{level:?}: {error}");
    }
}
