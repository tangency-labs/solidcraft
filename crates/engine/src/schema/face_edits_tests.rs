use serde_json::{Value, json};

use super::{DeleteFaceParams, OffsetFaceParams};
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
