use serde_json::{Value, json};

use super::{ApplyParams, CameraApplied, CameraFitted, CameraInfo, CameraSet, FitParams, GetParams, SetParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A render camera that isn't the default one, as `render.camera.set` takes it.
fn camera() -> Value {
    json!(solidcraft_render::Camera { yaw: 0.2, pitch: 0.3, roll: 0.1, ..Default::default() })
}

/// `camera()` with `key` set to `value`, or taken out when `value` is `None`.
fn camera_with(key: &str, value: Option<Value>) -> Value {
    let mut c = camera();
    if let Some(fields) = c.as_object_mut() {
        match value {
            Some(v) => fields.insert(key.into(), v),
            None => fields.remove(key),
        };
    }
    c
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SetParams>(), &["camera"]);
    assert_keys(input_schema_for::<GetParams>(), &[]);
    assert_keys(input_schema_for::<FitParams>(), &["aspect"]);
    assert_keys(input_schema_for::<ApplyParams>(), &["name"]);
}

/// #12288: calls that fail today get the same message in strict. (Inspect Render Camera fails
/// only on an unknown key, which strict refuses in the check's own words, as for every
/// converted command.)
#[test]
fn strict_keeps_todays_errors() {
    let mut cases = vec![
        ("render.camera.set", json!({})),
        ("render.camera.set", json!({"camera": null})),
        ("render.camera.set", json!({"camera": []})),
        ("render.camera.set", json!({"camera": camera_with("yaw", None)})),
        ("render.camera.set", json!({"camera": camera_with("fov_degrees", Some(json!(45)))})),
        ("render.camera.set", json!({"camera": camera_with("target", Some(json!([0, 0])))})),
    ];
    for (key, value) in [
        ("roll", json!(null)),
        ("roll", json!("invalid")),
        ("roll", json!(1e7)),
        ("up", json!("invalid")),
        ("up", json!(null)),
        ("distance", json!(0)),
        ("distance", json!(1e10)),
        ("fov", json!(-1)),
        ("fov", json!(4)),
        ("target", json!([1e10, 0, 0])),
    ] {
        cases.push(("render.camera.set", json!({"camera": camera_with(key, Some(value))})));
    }
    for aspect in [json!(0), json!(-1), json!(1e-320), json!(1e-7), json!(1e7), json!("tall"), json!(null)] {
        cases.push(("render.camera.fit", json!({"aspect": aspect})));
    }
    for name in [json!(null), json!(5), json!(""), json!(" Studio"), json!("Studio\u{7}"), json!("x".repeat(129)), json!("Nope")] {
        cases.push(("render.settings.apply", json!({"name": name})));
    }
    cases.push(("render.settings.apply", json!({})));
    assert_strict_keeps_todays_errors(&cases);
}

/// #12288: the camera's schema takes exactly what Set Render Camera takes (a saved setup's
/// camera, its keys and bounds), and the aspect's exactly what Frame Render Subject takes: each
/// case is checked against the handler too.
#[test]
fn the_camera_and_aspect_schemas_take_what_the_handlers_take() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 15, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let set = input_schema_for::<SetParams>();
    let set = set.as_value();
    let mut calls = vec![
        json!({"camera": camera()}),
        json!({}),
        json!({"camera": null}),
        json!({"camera": camera(), "width": 1}),
        json!({"camera": camera_with("roll", None)}),
        json!({"camera": camera_with("up", None)}),
        json!({"camera": camera_with("yaw", None)}),
        json!({"camera": camera_with("fov_degrees", Some(json!(45)))}),
    ];
    for (key, value) in [
        ("roll", json!(1e6)),
        ("roll", json!(1_000_001.0)),
        ("roll", json!(null)),
        ("yaw", json!(-1e6)),
        ("pitch", json!(-2e6)),
        ("up", json!("y")),
        ("up", json!("x")),
        ("distance", json!(1e-9)),
        ("distance", json!(1e9)),
        ("distance", json!(1_000_000_001.0)),
        ("distance", json!(0)),
        ("fov", json!(0)),
        ("fov", json!(3.0)),
        ("fov", json!(std::f64::consts::PI)),
        ("target", json!([1e9, -1e9, 0])),
        ("target", json!([1_000_000_001.0, 0, 0])),
        ("target", json!([0, 0, 0, 0])),
        ("target", json!("origin")),
    ] {
        calls.push(json!({"camera": camera_with(key, Some(value))}));
    }
    for p in calls {
        let takes = s.execute("render.camera.set", &p).is_ok();
        assert_eq!(conforms(set, set, &p), takes, "{p}");
    }
    let fit = input_schema_for::<FitParams>();
    let fit = fit.as_value();
    for p in [
        json!({}),
        json!({"aspect": 1}),
        json!({"aspect": 0.5}),
        json!({"aspect": 1e-6}),
        json!({"aspect": 1e6}),
        json!({"aspect": 1e-7}),
        json!({"aspect": 1e7}),
        json!({"aspect": 0}),
        json!({"aspect": "tall"}),
        json!({"aspect": null}),
        json!({"scale": 0}),
    ] {
        let takes = s.execute("render.camera.fit", &p).is_ok();
        assert_eq!(conforms(fit, fit, &p), takes, "{p}");
    }
    // Whether the setup exists is the handler's to check, so these are the schema's alone.
    let apply = input_schema_for::<ApplyParams>();
    let apply = apply.as_value();
    for (p, takes) in [
        (json!({"name": "Studio"}), true),
        (json!({}), false),
        (json!({"name": null}), false),
        (json!({"name": 5}), false),
        (json!({"name": ""}), false),
        (json!({"name": "x".repeat(129)}), false),
        (json!({"name": "Studio", "camera": camera()}), false),
    ] {
        assert_eq!(conforms(apply, apply, &p), takes, "{p}");
    }
}

/// #12288: the four commands reply as their output schemas say, in Warn without warnings:
/// Inspect before and after a camera is set, Frame with nothing to frame and then a box (wide
/// and tall), Set, and Apply of a saved setup's camera. The handlers already refuse an unknown
/// key, so a misspelt one fails in every mode; strict says so in the check's words.
#[test]
fn render_camera_commands_reply_with_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = run(&mut s, "render.camera.get", json!({}), output_schema_for::<CameraInfo>());
    assert_eq!((&r["camera"], &r["explicit_view"]), (&json!(null), &json!(false)), "{r}");
    let r = run(&mut s, "render.camera.fit", json!({}), output_schema_for::<CameraFitted>());
    assert_eq!(r["empty"], true, "nothing to frame: {r}");
    s.execute("solid.box", &json!({"length": 20, "width": 15, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let r = run(&mut s, "render.camera.fit", json!({"aspect": 0.5}), output_schema_for::<CameraFitted>());
    assert_eq!(r["empty"], false, "{r}");
    let r = run(&mut s, "render.camera.set", json!({"camera": camera()}), output_schema_for::<CameraSet>());
    assert_eq!(r["camera"], camera(), "{r}");
    let r = run(&mut s, "render.camera.get", json!({}), output_schema_for::<CameraInfo>());
    assert_eq!((&r["camera"], &r["explicit_view"]), (&camera(), &json!(true)), "{r}");
    let saved = camera_with("up", Some(json!("y")));
    s.execute("render.settings.set", &json!({"name": "Studio", "set": {"camera": saved}})).unwrap_or_else(|e| panic!("{e}"));
    let r = run(&mut s, "render.settings.apply", json!({"name": "Studio"}), output_schema_for::<CameraApplied>());
    assert_eq!((&r["name"], &r["camera"]), (&json!("Studio"), &saved), "{r}");
    // A picture far taller than wide moves the camera back beyond what a given camera may be:
    // the reply still matches its schema, which has no size bounds, and the input schema
    // refuses that camera, as Set Render Camera does.
    s.execute("solid.box", &json!({"length": 5000, "width": 5000, "height": 5000})).unwrap_or_else(|e| panic!("{e}"));
    let far = run(&mut s, "render.camera.fit", json!({"aspect": 1e-6}), output_schema_for::<CameraFitted>());
    assert!(far["camera"]["distance"].as_f64().is_some_and(|d| d > 1e9), "{far}");
    let set = input_schema_for::<SetParams>();
    let given = json!({"camera": far["camera"]});
    assert!(!conforms(set.as_value(), set.as_value(), &given), "{given}");
    assert!(s.execute("render.camera.set", &given).is_err(), "{given}");
    let misspelt = json!({"aspekt": 2});
    for level in [ParamCheck::Off, ParamCheck::Warn, ParamCheck::Strict] {
        s.param_check = level;
        let error = s.execute("render.camera.fit", &misspelt).err().map(|e| e.to_string()).unwrap_or_default();
        let expected = if level == ParamCheck::Strict { "`aspekt`" } else { "expected an empty parameter object or {aspect}" };
        assert!(error.contains(expected), "{level:?}: {error}");
    }
}
