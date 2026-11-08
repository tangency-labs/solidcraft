use serde_json::{Value, json};
use solidcraft_doc::canvas::base64_encode;

use super::{
    CanvasCalibrateParams, CanvasCalibrated, CanvasDecalParams, CanvasDeleteParams, CanvasDeleted, CanvasEditParams, CanvasEdited,
    CanvasInsertParams, CanvasPlaced, CanvasPlane, DecalFace,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A PNG header: all the engine reads of an image is its size.
fn png() -> String {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(400u32.to_be_bytes());
    b.extend(200u32.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    base64_encode(&b)
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<CanvasInsertParams>(),
        &["angle", "at", "center", "data", "flip", "name", "offset", "opacity", "path", "plane", "width"],
    );
    assert_keys(
        input_schema_for::<CanvasDecalParams>(),
        &["angle", "at", "center", "data", "face", "flip", "name", "offset", "opacity", "path", "width"],
    );
    assert_keys(input_schema_for::<CanvasEditParams>(), &["angle", "canvas", "center", "flip", "locked", "name", "opacity", "visible", "width"]);
    assert_keys(input_schema_for::<CanvasCalibrateParams>(), &["a", "b", "canvas", "distance"]);
    assert_keys(input_schema_for::<CanvasDeleteParams>(), &["canvas"]);
}

/// Strict gives the handlers' messages for the calls they refuse for a key (a missing `canvas`,
/// `face` or `distance` included: the handler's words, not "is required").
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("canvas.edit", json!({})),
        ("canvas.insert", json!({})),
        ("canvas.insert", json!({"path": "  "})),
        ("canvas.decal", json!({"face": [1, 2, 3]})),
        ("canvas.edit", json!({"canvas": true})),
        ("canvas.edit", json!({"canvas": null})),
        ("canvas.delete", json!({})),
        ("canvas.delete", json!({"canvas": ["Canvas1"]})),
        ("canvas.calibrate", json!({})),
        ("canvas.calibrate", json!({"canvas": {}})),
        ("canvas.decal", json!({"data": png()})),
        ("canvas.decal", json!({"data": png(), "face": null})),
        ("canvas.decal", json!({"data": png(), "face": [1, 2]})),
        ("canvas.decal", json!({"data": png(), "face": "top"})),
        ("canvas.decal", json!({"data": png(), "face": {"face": [1, 2, 3], "body": "Box1"}})),
        ("canvas.decal", json!({"data": png(), "face": {"face": [1, 2, 3], "fixed": 1}})),
        ("canvas.decal", json!({"data": png(), "face": {"face": [1, 2, 3], "body": "Box1", "index": -1}})),
        ("canvas.decal", json!({"data": png(), "face": {"face": [1, 2, 3], "path": [0]}})),
        ("canvas.insert", json!({"data": png(), "plane": null})),
        ("canvas.insert", json!({"data": png(), "plane": true})),
        ("canvas.insert", json!({"data": png(), "plane": [1, 0, 0]})),
        ("canvas.insert", json!({"data": png(), "plane": -1})),
        ("canvas.insert", json!({"data": png(), "plane": 1.5})),
        ("canvas.insert", json!({"data": png(), "plane": {"x_dir": [1, 0, 0]}})),
        ("canvas.insert", json!({"data": png(), "plane": {"x_dir": [1, 0, 0], "y_dir": [0, 1]}})),
        ("canvas.insert", json!({"data": png(), "plane": {"x_dir": [1, 0, 0], "y_dir": [2, 0, 0]}})),
        ("canvas.insert", json!({"data": png(), "plane": {"origin": ["w", 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("canvas.insert", json!({"data": png(), "plane": {"face": [1, 2]}})),
        ("canvas.insert", json!({"data": png(), "width": 0})),
        ("canvas.insert", json!({"data": png(), "width": -5})),
        ("canvas.insert", json!({"data": png(), "width": 1e7})),
    ]);
}

/// The same refusals for calls that need a canvas in the design (a missing one is refused first
/// with the check off).
#[test]
fn strict_keeps_todays_errors_with_a_canvas() {
    let cases = |c: u64| {
        [
            ("canvas.edit", json!({"canvas": c, "width": 0})),
            ("canvas.edit", json!({"canvas": "Canvas1", "width": -1})),
            ("canvas.calibrate", json!({"canvas": c, "b": [1, 0], "distance": 5})),
            ("canvas.calibrate", json!({"canvas": c, "a": [0, 0], "b": [1], "distance": 5})),
            ("canvas.calibrate", json!({"canvas": c, "a": null, "b": [1, 0], "distance": 5})),
            ("canvas.calibrate", json!({"canvas": c, "a": [0, 0], "b": [1, 0]})),
            ("canvas.calibrate", json!({"canvas": c, "a": [0, 0], "b": [1, 0], "distance": 0})),
            ("canvas.calibrate", json!({"canvas": c, "a": [0, 0], "b": [1, 0], "distance": "5"})),
            ("canvas.calibrate", json!({"canvas": c, "a": [0, 0], "b": [1, 0], "distance": -2})),
        ]
    };
    for index in 0..9 {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            let c = s.execute("canvas.insert", &json!({"data": png()})).unwrap()["canvas"].as_u64().unwrap();
            let (id, p) = cases(c).into_iter().nth(index).unwrap();
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let (id, p) = cases(1).into_iter().nth(index).unwrap();
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// What the contracts take and refuse in the schemas as well as the types: the image needs
/// `path` or `data`, a decal needs `face`, the plane forms and the calibration's keys.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let check = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value().clone();
        for (p, takes) in cases {
            assert_eq!(conforms(&schema, &schema, p), *takes, "{p}");
        }
    };
    check(
        input_schema_for::<CanvasInsertParams>(),
        &[
            (json!({"data": "abc"}), true),
            (json!({"path": "a.png", "plane": "XZ", "width": 80, "center": [1, 2], "at": [1, 2, 3], "offset": "w / 2"}), true),
            (json!({"data": "abc", "plane": {"origin": [0, 0, 5], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}), true),
            (json!({"data": "abc", "plane": {"face": [1, 2, 3], "body": "Box1", "index": 4}}), true),
            (json!({"data": "abc", "plane": 7}), true),
            (json!({"width": 80}), false),
            (json!({"data": "abc", "width": 0}), false),
            (json!({"data": "abc", "plane": {"origin": [0, 0, 0]}}), false),
            (json!({"data": "abc", "plane": {"origin": null, "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}), true),
            (json!({"data": "abc", "plane": {"normal": [0, 0, 1]}}), false),
            (json!({"data": "abc", "opacity": "half"}), false),
            (json!({"data": "abc", "colour": 1}), false),
        ],
    );
    check(
        input_schema_for::<CanvasDecalParams>(),
        &[
            (json!({"data": "abc", "face": [1, 2, 3]}), true),
            (json!({"path": "a.png", "face": {"face": [1, 2, 3], "fixed": true}}), true),
            (json!({"data": "abc"}), false),
            (json!({"face": [1, 2, 3]}), false),
            (json!({"data": "abc", "face": "top"}), false),
            (json!({"data": "abc", "face": [1, 2, 3], "plane": "XY"}), false),
        ],
    );
    check(
        input_schema_for::<CanvasCalibrateParams>(),
        &[
            (json!({"canvas": "Canvas1", "a": [0, 0], "b": {"x": 20, "y": 0}, "distance": 50}), true),
            (json!({"canvas": 1, "a": [0, 0], "b": [20, 0]}), false),
            (json!({"a": [0, 0], "b": [20, 0], "distance": 50}), false),
            (json!({"canvas": 1, "a": [0, 0], "b": [20, 0], "distance": 0}), false),
        ],
    );
    check(input_schema_for::<CanvasEditParams>(), &[(json!({"canvas": 2, "visible": false}), true), (json!({"visible": false}), false)]);
    check(input_schema_for::<CanvasDeleteParams>(), &[(json!({"canvas": "Canvas1"}), true), (json!({}), false)]);
}

/// The plane and face types accept what the handlers read and refuse what they refuse.
#[test]
fn the_plane_and_face_types_read_like_the_handlers() {
    let plane = |v: Value| serde_json::from_value::<CanvasPlane>(v).is_ok();
    assert!(plane(json!("XZ")) && plane(json!("Sketch Plane 1")) && plane(json!(3)));
    assert!(plane(json!({"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]})));
    assert!(plane(json!({"origin": null, "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]})));
    assert!(plane(json!({"face": [1, 2, 3], "body": "Box1", "index": 0, "path": [2]})));
    assert!(!plane(json!(null)) && !plane(json!(true)) && !plane(json!(-1)) && !plane(json!([0, 0, 1])));
    assert!(!plane(json!({"normal": [0, 0, 1]})) && !plane(json!({"face": [1, 2]})));
    let face = |v: Value| serde_json::from_value::<DecalFace>(v).is_ok();
    assert!(face(json!([1, 2, 3])) && face(json!({"x": 1, "y": 2, "z": 3})) && face(json!({"face": [1, 2, 3]})));
    assert!(!face(json!(null)) && !face(json!([1, 2])) && !face(json!({"face": [1, 2, 3], "body": "Box1"})));
}

/// Every form of every canvas command replies as its output schema says, in Warn without
/// warnings; a misspelt key still gets its warning and the reply still matches, and strict refuses
/// it.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("solid.box", &json!({"length": 30, "width": 30, "height": 10})).unwrap();
    let run = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        r
    };
    fn matches(schema: schemars::Schema, r: &Value) {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    }
    let forms = [
        json!({"data": png()}),
        json!({"data": png(), "plane": "XZ", "width": 80, "name": "Plan", "angle": 15, "opacity": 0.2, "flip": true}),
        json!({"data": png(), "plane": {"origin": [0, 0, 5], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "center": [3, 4]}),
        json!({"data": png(), "plane": {"face": [15, 15, 10]}, "at": [5, 5, 10]}),
        json!({"data": png(), "plane": "XY", "offset": 12}),
    ];
    for p in forms {
        let r = run(&mut s, "canvas.insert", p);
        matches(output_schema_for::<CanvasPlaced>(), &r);
        assert_eq!(r["pixels"], json!([400, 200]), "{r}");
    }
    assert_eq!(s.doc.canvases[1].name, "Plan");
    assert_eq!(s.execute("canvas.insert", &json!({"data": png()})).unwrap()["name"], "Canvas5");
    for p in [
        json!({"data": png(), "face": [15, 15, 10], "width": 20}),
        json!({"data": png(), "face": {"face": [15, 15, 10], "fixed": true}, "name": "Logo", "opacity": 0.8, "flip": true, "angle": 90, "center": [1, 1]}),
        json!({"data": png(), "face": {"x": 10, "y": 10, "z": 10}, "at": [12, 12, 10], "offset": 0}),
    ] {
        let r = run(&mut s, "canvas.decal", p);
        matches(output_schema_for::<CanvasPlaced>(), &r);
    }
    let first = s.doc.canvases[0].id;
    for p in [
        json!({"canvas": first, "center": [5, 5], "width": 60, "angle": 10, "opacity": 0.9, "flip": false, "visible": false, "name": "Renamed"}),
        json!({"canvas": "Renamed", "visible": true}),
        json!({"canvas": "Renamed"}),
    ] {
        let r = run(&mut s, "canvas.edit", p);
        matches(output_schema_for::<CanvasEdited>(), &r);
        assert_eq!(r["canvas"], json!(first), "{r}");
    }
    let r = run(&mut s, "canvas.calibrate", json!({"canvas": "Renamed", "a": [0, 0], "b": {"x": 20, "y": 0}, "distance": 50}));
    matches(output_schema_for::<CanvasCalibrated>(), &r);
    assert!((r["width"].as_f64().unwrap_or_default() - 150.0).abs() < 1e-9, "{r}");
    let r = run(&mut s, "canvas.delete", json!({"canvas": first}));
    matches(output_schema_for::<CanvasDeleted>(), &r);
    assert_eq!(r, json!({"deleted": first}));
    let second = s.doc.canvases[0].id;
    let r = run(&mut s, "canvas.edit", json!({"canvas": second, "opacity": 7, "name": ""}));
    assert_eq!(r, json!({"canvas": second}));
    assert_eq!((s.doc.canvases[0].opacity, s.doc.canvases[0].name.as_str()), (1.0, "Plan"));
    let r = run(&mut s, "canvas.insert", json!({"data": png(), "name": ""}));
    assert_eq!(r["name"], "");
    assert_eq!(r.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()), Some(vec!["canvas".into(), "name".into(), "pixels".into()]));
    let r = run(&mut s, "canvas.calibrate", json!({"canvas": second, "a": [0, 0], "b": [1, 0], "distance": 2}));
    assert_eq!(r.as_object().map(|o| o.len()), Some(2), "{r}");
    // A misspelt key runs in warn and says so; the reply still matches. Strict refuses it.
    let p = json!({"canvas": "Canvas2", "widht": 5});
    let r = s.execute("canvas.edit", &p).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    matches(output_schema_for::<CanvasEdited>(), &r);
    s.param_check = ParamCheck::Strict;
    let error = s.execute("canvas.edit", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`widht`"), "{error}");
}
