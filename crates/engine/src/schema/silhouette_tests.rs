use std::path::PathBuf;

use serde_json::{Value, json};

use super::{BodyNames, ExportViewDxfParams, SilhouetteCamera, SilhouetteParams, SilhouetteResult, ViewDxfResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

const SIL: &str = "inspect.silhouette";
const DXF: &str = "file.export_view_dxf";

fn camera() -> Value {
    json!({"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 100, "fov": 0})
}

/// A session with one box in it, so a call the handlers accept runs to its reply.
fn with_box(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"})).unwrap_or_else(|e| panic!("{e}"));
    s
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SilhouetteParams>(), &["bodies", "camera", "rows", "view"]);
    assert_keys(input_schema_for::<ExportViewDxfParams>(), &["camera", "curves", "hidden", "path", "tolerance", "zero"]);
}

/// Strict gives the handlers' messages for the calls they refuse for a key (a missing `camera`
/// included: the handler's words, not "is required").
#[test]
fn strict_keeps_todays_errors() {
    let long = vec!["Plate"; 10_001];
    assert_strict_keeps_todays_errors(&[
        (SIL, json!({"view": "front", "camera": camera()})),
        (SIL, json!({"view": null, "camera": camera()})),
        (SIL, json!({"view": 3})),
        (SIL, json!({"view": null})),
        (SIL, json!({"view": ["top"]})),
        (SIL, json!({"view": "sideways"})),
        (SIL, json!({"camera": null})),
        (SIL, json!({"camera": "front"})),
        (SIL, json!({"camera": {}})),
        (SIL, json!({"camera": {"target": [0, 0], "yaw": 0, "pitch": 0, "distance": 5}})),
        (SIL, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0}})),
        (SIL, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": -4}})),
        (SIL, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 5, "fov": -1}})),
        (SIL, json!({"camera": {"target": [0, 0, 0], "yaw": "0", "pitch": 0, "distance": 5}})),
        (SIL, json!({"bodies": null})),
        (SIL, json!({"bodies": 7})),
        (SIL, json!({"bodies": {"a": 1}})),
        (SIL, json!({"bodies": ["Plate", 2]})),
        (SIL, json!({"bodies": long})),
        (SIL, json!({"rows": null})),
        (SIL, json!({"rows": "5"})),
        (SIL, json!({"rows": 24.5})),
        (SIL, json!({"rows": true})),
        (DXF, json!({})),
        (DXF, json!({"camera": null})),
        (DXF, json!({"camera": "top"})),
        (DXF, json!({"camera": {}})),
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0, "fov": 0}})),
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 2, "distance": 1, "fov": 0}})),
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 2e6, "pitch": 0, "distance": 1, "fov": 0}})),
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "roll": 2e6, "distance": 1, "fov": 0}})),
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 1, "fov": 3.2}})),
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "fov": 0}})),
        (DXF, json!({"camera": camera(), "hidden": "dash"})),
        (DXF, json!({"camera": camera(), "hidden": "Omit"})),
        (DXF, json!({"camera": camera(), "hidden": null})),
        (DXF, json!({"camera": camera(), "hidden": true})),
        (DXF, json!({"camera": camera(), "tolerance": 0})),
        (DXF, json!({"camera": camera(), "tolerance": 0.00001})),
        (DXF, json!({"camera": camera(), "tolerance": 1.5})),
        (DXF, json!({"camera": camera(), "tolerance": "0.1"})),
        (DXF, json!({"camera": camera(), "tolerance": null})),
        (DXF, json!({"camera": camera(), "path": ""})),
        (DXF, json!({"camera": camera(), "path": " \t"})),
        (DXF, json!({"camera": camera(), "path": 5})),
        (DXF, json!({"camera": camera(), "path": null})),
        (DXF, json!({"camera": camera(), "path": "x".repeat(4096)})),
    ]);
}

/// The contracts read each key as the handlers do: for every call, what runs today is not
/// refused in strict or warned about in warn, and what fails today fails in strict with the same
/// message. (A call the handler accepts but the contract refused would fail one of the three.)
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let dir = std::env::temp_dir().join(format!("solidcraft-dxf-schema-read-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let file = dir.join("read.dxf");
    let named = |extra: Value| {
        let mut c = camera();
        for (k, v) in extra.as_object().into_iter().flatten() {
            c[k] = v.clone();
        }
        c
    };
    let cases = [
        (SIL, json!({})),
        (SIL, Value::Null),
        (SIL, json!({"view": "front"})),
        (SIL, json!({"view": "TOP"})),
        (SIL, json!({"view": "Home"})),
        (SIL, json!({"view": "isometric"})),
        (SIL, json!({"view": "iso", "rows": 16})),
        (SIL, json!({"camera": camera()})),
        (SIL, json!({"camera": named(json!({"fov": 0.6}))})),
        (SIL, json!({"camera": {"target": [10, 10, 10], "yaw": 0.5, "pitch": 0.5, "distance": 300}})),
        (SIL, json!({"camera": named(json!({"name": "mine", "roll": 9, "up": "y", "extra": [1]}))})),
        (SIL, json!({"camera": named(json!({"pitch": 7, "yaw": 1e9}))})),
        (SIL, json!({"camera": named(json!({"fov": 3.5}))})),
        (SIL, json!({"camera": named(json!({"distance": 1e-9}))})),
        (SIL, json!({"bodies": "Plate"})),
        (SIL, json!({"bodies": ["Plate"]})),
        (SIL, json!({"bodies": []})),
        (SIL, json!({"bodies": "Nope"})),
        (SIL, json!({"rows": 0})),
        (SIL, json!({"rows": -5})),
        (SIL, json!({"rows": 5000})),
        (SIL, json!({"rows": 18446744073709551615u64})),
        (SIL, json!({"rows": 1e3})),
        (DXF, json!({"camera": camera()})),
        (DXF, json!({"camera": named(json!({"fov": 0.5}))})),
        (DXF, json!({"camera": named(json!({"roll": 1, "up": "y", "pitch": 1.5}))})),
        (DXF, json!({"camera": named(json!({"pitch": std::f64::consts::FRAC_PI_2, "yaw": 1e6, "roll": -1e6}))})),
        (DXF, json!({"camera": named(json!({"fov": 3.1}))})),
        // No `fov` is an orthographic view, as for the view commands (#13112).
        (DXF, json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 100}})),
        (DXF, json!({"camera": camera(), "hidden": "omit"})),
        (DXF, json!({"camera": camera(), "hidden": "dashed", "tolerance": 1})),
        (DXF, json!({"camera": camera(), "tolerance": 0.0001})),
        (DXF, json!({"camera": camera(), "tolerance": 1.0})),
        (DXF, json!({"camera": camera(), "path": file})),
    ];
    for (id, p) in cases {
        let run = |level: ParamCheck| with_box(level).execute(id, &p).map_err(|e| e.to_string());
        let (off, warn, strict) = (run(ParamCheck::Off), run(ParamCheck::Warn), run(ParamCheck::Strict));
        match &off {
            Ok(_) => {
                let warned = warn.as_ref().ok().and_then(|r| r.get("param_warnings"));
                assert!(warned.is_none(), "{id} {p}: {warned:?}");
                assert!(strict.is_ok(), "{id} {p}: works today, strict says {strict:?}");
            }
            Err(e) => assert_eq!(strict.as_ref().err(), Some(e), "{id} {p}"),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The types themselves: a camera, a list of bodies and the message for each refusal.
#[test]
fn the_camera_and_body_types_read_like_the_handler() {
    let camera = |v: Value| serde_json::from_value::<SilhouetteCamera>(v).map_err(|e| e.to_string());
    assert!(camera(json!({"target": [1, 2, 3], "yaw": 0.1, "pitch": 0.2, "distance": 5})).is_ok());
    assert!(camera(json!({"target": [1, 2, 3], "yaw": 0.1, "pitch": 0.2, "distance": 5, "fov": 0, "roll": 4})).is_ok());
    for bad in [json!(null), json!([]), json!({}), json!({"target": [1, 2, 3], "yaw": 0, "pitch": 0})] {
        assert!(camera(bad.clone()).is_err_and(|e| e.contains("bad camera: ")), "{bad}");
    }
    assert!(camera(json!({"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0})).is_err_and(|e| e.contains("positive distance")));
    let bodies = |v: Value| serde_json::from_value::<BodyNames>(v).ok().map(|b| b.0);
    assert_eq!(bodies(json!("A")), Some(vec!["A".to_string()]));
    assert_eq!(bodies(json!(["A", "B"])), Some(vec!["A".to_string(), "B".to_string()]));
    assert_eq!(bodies(json!([])), Some(Vec::new()));
    assert_eq!(bodies(json!(vec!["x"; 10_000])).map(|b| b.len()), Some(10_000));
    assert!(bodies(json!(vec!["x"; 10_001])).is_none() && bodies(json!([1])).is_none() && bodies(json!(null)).is_none());
}

/// What the contracts take in the schemas too: the camera is required of the export, `view` and
/// `camera` do not come together, and the plain-typed keys.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<SilhouetteParams>(),
        &[
            (json!({}), true),
            (json!({"view": "top", "bodies": ["Plate"], "rows": 100}), true),
            (json!({"camera": camera(), "bodies": "Plate"}), true),
            (json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 5}}), true),
            (json!({"view": "top", "camera": camera()}), false),
            (json!({"view": 3}), false),
            (json!({"camera": {"target": [0, 0], "yaw": 0, "pitch": 0, "distance": 5}}), false),
            (json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0}}), false),
            (json!({"bodies": [1]}), false),
            (json!({"rows": 2.5}), false),
            (json!({"colour": 1}), false),
        ],
    );
    takes(
        input_schema_for::<ExportViewDxfParams>(),
        &[
            (json!({"camera": camera()}), true),
            (json!({"camera": camera(), "hidden": "dashed", "tolerance": 0.5, "path": "a.dxf"}), true),
            (json!({}), false),
            (json!({"camera": camera(), "hidden": "both"}), false),
            (json!({"camera": camera(), "tolerance": 2}), false),
            (json!({"camera": camera(), "tolerance": 0}), false),
            (json!({"camera": camera(), "pathh": "a.dxf"}), false),
            (json!({"camera": camera(), "zero": "origin"}), true),
            (json!({"camera": camera(), "zero": "corner"}), false),
            (json!({"camera": camera(), "curves": "arcs"}), true),
            (json!({"camera": camera(), "curves": "splines"}), false),
        ],
    );
}

/// The warning for what a key gets wrong, in the handlers' words.
#[test]
fn the_check_names_what_is_wrong_with_a_key() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues(SIL, json!({"view": "top", "camera": camera()})), ["give `view` or `camera`, not both"]);
    assert_eq!(issues(SIL, json!({"view": "sideways"})), ["unknown standard view `sideways`"]);
    assert_eq!(issues(SIL, json!({"rows": 1.5})), ["`rows` must be an integer"]);
    assert_eq!(issues(SIL, json!({"bodies": [1]})), ["`bodies` must contain names"]);
    assert_eq!(issues(DXF, json!({"camera": camera(), "hidden": 1})), ["hidden must be omit or dashed"]);
    assert_eq!(issues(DXF, json!({"camera": camera(), "tolerance": 3})), ["tolerance must be 0.0001..1 mm"]);
    assert_eq!(issues(DXF, json!({"camera": camera(), "zero": "corner"})), ["zero must be target or origin"]);
    assert_eq!(issues(DXF, json!({"camera": camera(), "curves": "splines"})), ["curves must be lines or arcs"]);
    assert_eq!(issues(DXF, json!({"camera": camera(), "path": " "})), ["invalid path"]);
    assert_eq!(issues(DXF, json!({"camera": 1})), ["invalid camera"]);
    // Left out, the handler's own refusal stands in for "is required".
    assert!(issues(DXF, json!({})).is_empty());
}

fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-silhouette-schema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

/// Every form of both commands, in Warn: each real reply matches its output schema and carries
/// no warning. A misspelt key still runs, says so, and the reply still matches; strict refuses.
#[test]
fn replies_match_the_output_schema() {
    let mut s = with_box(ParamCheck::Warn);
    s.execute("solid.sphere", &json!({"radius": 5, "center": [60, 0, 0], "body_name": "Ball"})).unwrap_or_else(|e| panic!("{e}"));
    let dir = folder();
    let run = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        r
    };
    for p in [
        json!({}),
        json!({"view": "top", "rows": 40}),
        json!({"view": "ISO", "bodies": "Plate"}),
        json!({"view": "right", "bodies": ["Plate", "Ball"], "rows": 18446744073709551615u64}),
        json!({"camera": {"target": [20, 15, 5], "yaw": 0.4, "pitch": 0.3, "distance": 300}, "rows": 32}),
        json!({"camera": {"target": [20, 15, 5], "yaw": 0.4, "pitch": 0.3, "distance": 300, "fov": 0.6}, "bodies": ["Plate"]}),
    ] {
        let r = run(&mut s, SIL, p.clone());
        matches(output_schema_for::<SilhouetteResult>(), SIL, &r);
        let projection = if p["camera"]["fov"].as_f64().is_some_and(|f| f > 0.0) { "perspective" } else { "parallel" };
        assert_eq!(r["projection"], projection, "{p}");
        assert!(r["mask"].as_array().is_some_and(|m| !m.is_empty()), "{r}");
        assert_eq!(r.as_object().map(|o| o.len()), Some(10), "{r}");
    }
    let target = json!([20, 15, 5]);
    let top = json!({"target": target, "yaw": 0, "pitch": std::f64::consts::FRAC_PI_2, "distance": 150, "fov": 0});
    let file = dir.join("top.dxf");
    for p in [
        json!({"camera": top}),
        json!({"camera": top, "hidden": "omit", "tolerance": 0.5}),
        json!({"camera": top, "hidden": "dashed", "tolerance": 0.0001}),
        json!({"camera": {"target": target, "yaw": 0.5, "pitch": 0.5, "distance": 200, "fov": 0.5}, "hidden": "dashed"}),
        json!({"camera": top, "path": file}),
    ] {
        let r = run(&mut s, DXF, p.clone());
        matches(output_schema_for::<ViewDxfResult>(), DXF, &r);
        assert_eq!(r["units"], "mm", "{r}");
        if p.get("path").is_some() {
            assert!(r.get("dxf").is_none() && r["path"] == json!(file), "{r}");
            assert!(std::fs::read_to_string(&file).is_ok_and(|t| t.contains("LINE")));
        } else {
            assert!(r.get("path").is_none() && r["dxf"].as_str().is_some_and(|t| t.contains("LINE")), "{r}");
        }
        assert_eq!(r["projection"], if p["camera"]["fov"].as_f64().is_some_and(|f| f > 0.0) { "perspective" } else { "orthographic" }, "{r}");
    }
    // A misspelt key runs in warn and says so; the reply still matches. Strict refuses it.
    for (id, p, schema) in [
        (SIL, json!({"rowz": 40}), output_schema_for::<SilhouetteResult>()),
        (DXF, json!({"camera": top, "tolerence": 0.5}), output_schema_for::<ViewDxfResult>()),
    ] {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(r["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{r}");
        assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r}");
        s.param_check = ParamCheck::Strict;
        let error = s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("unknown key"), "{id}: {error}");
        s.param_check = ParamCheck::Warn;
    }
    let _ = std::fs::remove_dir_all(&dir);
}
