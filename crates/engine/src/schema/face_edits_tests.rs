use serde_json::{Value, json};

use super::{DeleteFaceParams, OffsetFaceParams, TransformFaceParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn session(level: ParamCheck, calls: &[(&str, Value)]) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in calls {
        s.execute(id, p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
    s
}

/// A 20 mm cube, `Box`, its top face at z = 20.
fn cube(level: ParamCheck) -> Session {
    session(level, &[("solid.box", json!({"length": 20, "width": 20, "height": 20, "body_name": "Box"}))])
}

/// A 40 × 20 × 2 mm plate, `Plate`, with a 4 mm hole through it at (10, 10): Delete Face takes the
/// hole's wall off and heals the plate.
fn holed_plate(level: ParamCheck) -> Session {
    session(
        level,
        &[
            ("solid.box", json!({"length": 40, "width": 20, "height": 2, "body_name": "Plate"})),
            ("solid.cylinder", json!({"radius": 2, "height": 4, "base": [10, 10, -1], "operation": "cut", "targets": ["Plate"]})),
        ],
    )
}

/// A point on the plate's hole wall.
fn hole_wall() -> Value {
    json!([[12, 10, 1]])
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DeleteFaceParams>(), &["body", "body_name", "body_names", "faces", "heal", "name", "participants"]);
    assert_keys(input_schema_for::<OffsetFaceParams>(), &["body", "body_name", "body_names", "distance", "faces", "name", "participants"]);
}

/// #12740: calls that fail today get the same message in strict. The faces fail on the shared
/// helper's sketch, which has no body; the distances need the cube's face.
#[test]
fn strict_keeps_todays_errors() {
    let many = vec![[0, 0, 0]; 1001];
    assert_strict_keeps_todays_errors(&[
        ("face.delete", json!({})),
        ("face.delete", json!({"faces": null})),
        ("face.delete", json!({"faces": 5})),
        ("face.delete", json!({"faces": []})),
        ("face.delete", json!({"faces": many})),
        ("face.delete", json!({"faces": [true]})),
        ("face.delete", json!({"faces": [[1, 2]]})),
        ("solid.offset_face", json!({"distance": 1})),
        ("solid.offset_face", json!({"faces": [], "distance": 1})),
        ("solid.offset_face", json!({"faces": [""], "distance": 1})),
    ]);
    for p in [
        json!({"faces": [[10, 10, 20]], "distance": true}),
        json!({"faces": [[10, 10, 20]], "distance": " "}),
        json!({"faces": [[10, 10, 20]], "distance": "nope + 1"}),
        json!({"faces": [[10, 10, 20]], "distance": "2 deg"}),
    ] {
        let off = cube(ParamCheck::Off).execute("solid.offset_face", &p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(cube(ParamCheck::Strict).execute("solid.offset_face", &p).err().map(|e| e.to_string()), off, "{p}");
    }
}

/// #12740: keys of the wrong type that the handlers ignore today (a `heal` that isn't true or
/// false is read as true; a `body` that isn't text as no body) are refused by the contracts: in
/// Warn the call still works, with a warning naming the key.
#[test]
fn the_contracts_refuse_what_the_handlers_ignore() {
    let cases: Vec<(fn(ParamCheck) -> Session, &str, Value, &str)> = vec![
        (holed_plate, "face.delete", json!({"faces": hole_wall(), "heal": "no"}), "heal"),
        (holed_plate, "face.delete", json!({"faces": hole_wall(), "body": 5}), "body"),
        (cube, "solid.offset_face", json!({"faces": [[10, 10, 20]], "distance": 5, "body": ["Box"]}), "body"),
    ];
    for (fixture, id, p, key) in cases {
        assert!(fixture(ParamCheck::Off).execute(id, &p).is_ok(), "{id} {p} works today");
        let warned = fixture(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = fixture(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id} {p}: {error}");
    }
}

/// #12740: the schemas take what the handlers take. Each call is checked against both the schema
/// and the handler, with the check off, so the two can't drift apart.
#[test]
fn the_schemas_take_what_the_handlers_take() {
    let agree = |fixture: fn(ParamCheck) -> Session, id: &str, schema: schemars::Schema, calls: &[Value]| {
        let schema = schema.as_value();
        for p in calls {
            let takes = fixture(ParamCheck::Off).execute(id, p).is_ok();
            assert_eq!(conforms(schema, schema, p), takes, "{id} {p}");
        }
    };
    agree(
        cube,
        "solid.offset_face",
        input_schema_for::<OffsetFaceParams>(),
        &[
            json!({"faces": [[10, 10, 20]], "distance": 5}),
            json!({"faces": [[10, 10, 20], [10, 10, 0]], "distance": "2 mm", "body": "Box", "name": "Push"}),
            json!({"faces": [[10, 10, 20]], "distance": ""}),
            json!({"faces": [], "distance": 5}),
            json!({"faces": [[10, 10]], "distance": 5}),
            json!({"distance": 5}),
        ],
    );
    agree(
        holed_plate,
        "face.delete",
        input_schema_for::<DeleteFaceParams>(),
        &[
            json!({"faces": hole_wall()}),
            json!({"faces": hole_wall(), "heal": true, "body": "Plate"}),
            json!({"faces": hole_wall(), "heal": false, "body_name": "Open"}),
            json!({"faces": [[12, 10]]}),
            json!({}),
        ],
    );
}

/// #12740: both commands reply as a feature command does, in Warn without warnings: the plate's
/// hole taken off and healed, and the cube's top pushed out. A misspelt key is ignored today: Warn
/// says so, and strict refuses it.
#[test]
fn face_edits_reply_with_their_schemas() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let check = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let r = check(&mut holed_plate(ParamCheck::Warn), "face.delete", json!({"faces": hole_wall(), "name": "Fill"}));
    assert_eq!((&r["name"], &r["changed_bodies"]), (&json!("Fill"), &json!(["Plate"])), "{r}");
    let r = check(&mut cube(ParamCheck::Warn), "solid.offset_face", json!({"faces": [[10, 10, 20]], "distance": 5}));
    assert_eq!(r["changed_bodies"], json!(["Box"]), "{r}");
    // A misspelt key.
    let p = json!({"faces": hole_wall(), "heel": false});
    let r = holed_plate(ParamCheck::Warn).execute("face.delete", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("heel"), "{r}");
    let error = holed_plate(ParamCheck::Strict).execute("face.delete", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`heel`"), "{error}");
}

const TRANSFORM_FACE: &str = "solid.transform_face";

#[test]
fn transform_face_schema_and_reply_cover_each_input_form() {
    assert_keys(
        input_schema_for::<TransformFaceParams>(),
        &["angle", "axis", "body", "body_name", "body_names", "face", "name", "origin", "participants", "translate"],
    );
    let schema = input_schema_for::<TransformFaceParams>();
    let output = output_schema_for::<FeatureResult>();
    let mut s = cube(ParamCheck::Warn);
    let faces = s.execute("model.faces", &json!({"body": "Box"})).unwrap();
    let name = faces["faces"].as_array().unwrap().iter().find(|f| f["point"][2].as_f64() == Some(20.0)).unwrap()["name"].as_str().unwrap();
    for p in [
        json!({"face": [10,10,20], "translate": [0,0,0]}),
        json!({"face": {"x":10,"y":10,"z":20}, "angle": 0, "axis":" x "}),
        json!({"face":{"point":[10,10,20]}, "translate":{"x":0,"y":"0 mm","z":0}, "axis":{"x":0,"y":2,"z":0}, "origin":{"x":0,"y":0,"z":20}}),
        json!({"face": name, "body": "Box", "angle": "0 deg", "axis": [2,0,0], "origin": ["2 * 1 mm",0,20], "translate": [0,0,"2 mm"], "name": "Lift"}),
    ] {
        assert!(conforms(schema.as_value(), schema.as_value(), &p), "{p}");
        assert!(crate::schema::issues(TRANSFORM_FACE, &p).is_empty(), "{p}");
        let out = s.execute(TRANSFORM_FACE, &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(output.as_value(), output.as_value(), &out), "{out}");
        assert!(out.get("param_warnings").is_none(), "{out}");
        if p["face"].is_string() {
            assert_eq!(out["changed_bodies"], json!(["Box"]));
            assert_eq!(s.doc.feature(out["feature"].as_u64().unwrap()).unwrap().face_names, [name]);
        }
    }
}

#[test]
fn transform_face_rejects_bad_shapes_and_dimensions_before_adding() {
    let schema = input_schema_for::<TransformFaceParams>();
    let base = json!({"face":[10,10,20],"angle":0});
    let with = |key: &str, value: Value| {
        let mut p = base.clone();
        p[key] = value;
        p
    };
    let shapes = [
        json!({"angle":0}),
        json!({"face":[10,10,20]}),
        with("face", Value::Null),
        with("face", json!([[10, 10, 20], [5, 5, 0]])),
        with("face", json!({"point":[10,10]})),
        with("face", json!("")),
        with("body", Value::Null),
        with("body", json!(["Box"])),
        with("translate", Value::Null),
        with("translate", json!([0, 0])),
        with("translate", json!([true, 0, 0])),
        with("angle", Value::Null),
        with("angle", json!(false)),
        with("angle", json!("")),
        with("origin", Value::Null),
        with("origin", json!({"x":0, "z":0})),
        with("origin", json!([0, 0, 0, 0])),
        with("axis", Value::Null),
        with("axis", json!([1, "0 mm", 0])),
        with("axis", json!([1, 0])),
        with("copy", json!(true)),
        with("matrix", json!([])),
        with("scale", json!(1)),
        with("bodies", json!(["Box"])),
        with("faces", json!([[10, 10, 20]])),
        with("name", json!(10)),
        with("body_name", json!(false)),
        with("body_names", json!([0])),
        with("participants", json!([false])),
    ];
    for p in &shapes {
        assert!(!conforms(schema.as_value(), schema.as_value(), p), "{p}");
        assert!(!crate::schema::issues(TRANSFORM_FACE, p).is_empty(), "{p}");
    }
    let no_motion = json!({"face":[10,10,20],"origin":[0,0,0],"axis":"Z"});
    assert!(!conforms(schema.as_value(), schema.as_value(), &no_motion));
    let values = [
        no_motion,
        with("angle", json!("2 mm")),
        with("angle", json!("asin(2)")),
        with("origin", json!([0, "4 deg", 0])),
        with("translate", json!([0, 0, "1 rad"])),
        with("translate", json!([0, "unknown_length + 2 mm", 0])),
        with("axis", json!("north")),
        with("axis", json!([0, 0, 0])),
        with("face", json!([10, 10, 10])),
        with("face", json!("no such face")),
        with("body", json!("no such body")),
    ];
    for level in [ParamCheck::Off, ParamCheck::Strict] {
        let mut s = cube(level);
        let before = s.doc.features.len();
        for p in shapes.iter().chain(&values) {
            assert!(s.execute(TRANSFORM_FACE, p).is_err(), "{level:?}: {p}");
            assert_eq!(s.doc.features.len(), before, "{level:?}: {p}");
        }
    }
}

#[test]
fn transform_face_nested_constructor_checks_the_local_face_and_owner() {
    let mut s = cube(ParamCheck::Off);
    s.execute("solid.box", &json!({"corner":[50,0,0],"length":10,"width":10,"height":10,"body_name":"Other"})).unwrap();
    let before = s.doc.features.len();
    let cmd = crate::cmd::find_command(TRANSFORM_FACE).unwrap();
    for p in [
        json!({"body":"Other","face":[10,10,20],"angle":0}),
        json!({"body":"Box","face":[10,10,10],"angle":0}),
        json!({"body":"Box","face":[10,10,20],"translate":["1 deg",0,0]}),
    ] {
        assert!(cmd.run_nested(&mut s, &p).is_err(), "{p}");
        assert_eq!(s.doc.features.len(), before, "{p}");
    }
}
