use serde_json::{Value, json};

use super::{BackdropParams, ClearParams, GroundParams, StageSaved};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A design with a box and a saved render setup, `Shot`, that renders with the GPU path tracer,
/// at the parameter-check level `level`.
fn with_a_shot(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("solid.box", json!({"length": 20, "width": 15, "height": 10, "body_name": "Subject"})),
        ("render.settings.set", json!({"name": "Shot", "set": {"width": 96, "height": 64, "job": {"method": "gpu_path", "options": {}}}})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<GroundParams>(), &["gap_mm", "margin_fraction", "name", "reflectance_linear", "replace_stage", "set"]);
    assert_keys(input_schema_for::<BackdropParams>(), &["expected_revision", "kind", "name", "reflectance_linear", "replace_stage", "set"]);
    assert_keys(input_schema_for::<ClearParams>(), &["name", "set"]);
}

/// The handlers refuse these calls after reading them; strict refuses them with their message.
/// They need a design with a body and a saved setup, and the design's revision, so the shared
/// helper, which has a sketch only, doesn't fit.
#[test]
fn strict_keeps_todays_errors() {
    let cases: [(&str, fn(u64) -> Value); 12] = [
        ("render.stage.ground", |_| json!({"name": "Shot", "margin_fraction": 11})),
        ("render.stage.ground", |_| json!({"name": "Shot", "gap_mm": 0})),
        ("render.stage.ground", |_| json!({"name": "Shot", "reflectance_linear": [2, 0, 0]})),
        ("render.stage.ground", |_| json!({"name": "Raster"})),
        ("render.stage.ground", |_| json!({"name": " Shot"})),
        ("render.stage.ground", |_| json!({"name": "Shot", "set": {"width": 0}})),
        ("render.stage.backdrop", |_| json!({"name": "Shot", "kind": "cyclorama", "expected_revision": 999_999})),
        ("render.stage.backdrop", |r| json!({"name": "Raster", "kind": "floor_wall", "expected_revision": r})),
        ("render.stage.backdrop", |r| json!({"name": "Shot", "kind": "floor_wall", "reflectance_linear": [0, -1, 0], "expected_revision": r})),
        ("render.stage.clear", |_| json!({"name": ""})),
        ("render.stage.clear", |_| json!({"name": "Shot", "set": {"width": 0}})),
        ("render.stage.clear", |_| json!({"name": "Shot", "set": 5})),
    ];
    for (id, p) in cases {
        let error = |level: ParamCheck| {
            let mut s = with_a_shot(level);
            let p = p(s.revision);
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {} should fail today", p(0));
        assert_eq!(error(ParamCheck::Strict), off, "{id} {}", p(0));
    }
    // A stage already there needs `replace_stage`, for a ground and a backdrop alike.
    for level in [ParamCheck::Off, ParamCheck::Strict] {
        let mut s = with_a_shot(level);
        s.execute("render.stage.ground", &json!({"name": "Shot"})).unwrap_or_else(|e| panic!("{e}"));
        let ground = s.execute("render.stage.ground", &json!({"name": "Shot"})).err().map(|e| e.to_string());
        let p = json!({"name": "Shot", "kind": "floor_wall", "expected_revision": s.revision});
        let backdrop = s.execute("render.stage.backdrop", &p).err().map(|e| e.to_string());
        for error in [ground, backdrop] {
            assert!(error.as_deref().is_some_and(|e| e.contains("replace_stage: true")), "{level:?}: {error:?}");
        }
    }
}

/// #11455: a ground, both backdrops and a clear reply as the output schema says, in Warn
/// without warnings; only a backdrop carries its shape and limits. Like the handlers, warn and
/// strict refuse a misspelt key.
#[test]
fn stage_commands_reply_with_their_schema() {
    let schema = output_schema_for::<StageSaved>();
    let schema = schema.as_value();
    let mut s = with_a_shot(ParamCheck::Warn);
    let run = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let p = json!({"name": "Shot", "set": {}, "margin_fraction": 0.5, "gap_mm": 1, "reflectance_linear": [0.2, 0.2, 0.2], "replace_stage": false});
    let r = run(&mut s, "render.stage.ground", p);
    assert!(r["setup"].is_object() && r.get("geometry_kind").is_none(), "{r}");
    for kind in ["floor_wall", "cyclorama"] {
        let revision = s.revision;
        let p = json!({"name": "Shot", "kind": kind, "replace_stage": true, "expected_revision": revision});
        let r = run(&mut s, "render.stage.backdrop", p);
        assert!(r["geometry_kind"].is_string() && r["limitations"].is_array(), "{r}");
    }
    run(&mut s, "render.stage.clear", json!({"name": "Shot", "set": {}}));
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = with_a_shot(level);
        let error = s.execute("render.stage.clear", &json!({"nme": "Shot"})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("nme"), "{level:?}: {error}");
    }
}
