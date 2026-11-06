use serde_json::{Value, json};

use super::{FairParams, OffsetParams, ReverseNormalParams, StitchParams, ThickenParams, UnstitchParams, UntrimParams};
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

/// A 10 mm cube, `Box`.
fn cube(level: ParamCheck) -> Session {
    session(level, &[("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}))])
}

/// A flat 10 × 10 mm surface on XY, `Sheet`.
fn sheet(level: ParamCheck) -> Session {
    session(
        level,
        &[
            ("sketch.create", json!({"plane": "XY", "name": "S"})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})),
            ("sketch.finish", json!({})),
            ("surface.patch", json!({"sketch": "S", "body_name": "Sheet"})),
        ],
    )
}

/// A 40 × 20 × 2 mm plate, `Plate`, with three round holes through it at x = 10, 20 and 30.
fn plate(level: ParamCheck) -> Session {
    let mut calls = vec![("solid.box", json!({"length": 40, "width": 20, "height": 2, "body_name": "Plate"}))];
    for x in [10, 20, 30] {
        calls.push(("solid.cylinder", json!({"radius": 2, "height": 4, "base": [x, 10, -1], "operation": "cut", "targets": ["Plate"]})));
    }
    session(level, &calls)
}

/// A fitted B-spline surface, `Shell`: the patch on the rim of a box whose top a sideways
/// cylinder cut away, as Fair Surface's own test makes it.
fn shell(level: ParamCheck) -> Session {
    let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
    let rim = json!([[14, 0, z(14.0)], [0, 14, z(0.0)], [-14, 0, z(-14.0)], [0, -14, z(0.0)]]);
    session(
        level,
        &[
            ("solid.box", json!({"length": 28, "width": 28, "height": 20, "corner": [-14, -14, 0], "body_name": "Saddle"})),
            ("solid.cylinder", json!({"radius": 40, "height": 120, "base": [0, -60, 50], "axis": "Y", "operation": "cut", "targets": ["Saddle"]})),
            ("surface.patch", json!({"edges": rim, "body": "Saddle", "body_name": "Shell"})),
        ],
    )
}

/// The holed plate's top face, for Untrim.
fn plate_top() -> Value {
    json!({"body": "Plate", "point": [5, 5, 2]})
}

/// `n` copies of a point.
fn points(n: usize) -> Value {
    json!(vec![[5, 5, 10]; n])
}

#[test]
fn schemas_and_the_key_check_agree() {
    let common = ["body_name", "body_names", "name", "participants"];
    let with = |own: &[&'static str]| {
        let mut keys: Vec<&str> = own.iter().chain(common.iter()).copied().collect();
        keys.sort_unstable();
        keys
    };
    assert_keys(input_schema_for::<StitchParams>(), &with(&["bodies", "tolerance"]));
    assert_keys(input_schema_for::<UnstitchParams>(), &with(&["body", "faces"]));
    assert_keys(input_schema_for::<ReverseNormalParams>(), &with(&["body", "faces"]));
    assert_keys(input_schema_for::<ThickenParams>(), &with(&["bodies", "operation", "symmetric", "targets", "thickness", "touching"]));
    assert_keys(input_schema_for::<OffsetParams>(), &with(&["body", "chain", "distance", "faces", "tolerance"]));
    assert_keys(input_schema_for::<UntrimParams>(), &with(&["face", "holes", "in_place", "outer"]));
    assert_keys(input_schema_for::<FairParams>(), &with(&["body", "faces", "iterations", "keep_edges", "strength", "tolerance"]));
}

/// As `assert_strict_keeps_todays_errors`, on the design `fixture` makes: for the calls that need
/// a body to get past the handler's first checks.
fn assert_strict_keeps_errors_on(fixture: fn(ParamCheck) -> Session, cases: &[(&str, Value)]) {
    for (id, p) in cases {
        let off = fixture(ParamCheck::Off).execute(id, p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(fixture(ParamCheck::Strict).execute(id, p).err().map(|e| e.to_string()), off, "{id} {p}");
    }
}

/// #12627: calls that fail today get the same message in strict. The first ones fail on the
/// shared helper's sketch, which has no body; the rest need the cube, the sheet, the plate or the
/// shell to get past the handler's check of the body.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("surface.stitch", json!({})),
        ("surface.stitch", json!({"bodies": null})),
        ("surface.stitch", json!({"bodies": []})),
        ("surface.stitch", json!({"bodies": 5})),
        ("surface.stitch", json!({"bodies": ["Sheet", 5]})),
        ("surface.stitch", json!({"bodies": ["Nope"]})),
        ("surface.unstitch", json!({})),
        ("surface.unstitch", json!({"body": null})),
        ("surface.unstitch", json!({"body": 5})),
        ("surface.unstitch", json!({"body": "Nope"})),
        ("surface.reverse_normal", json!({"body": ["Sheet"]})),
        ("surface.thicken", json!({"bodies": null, "thickness": 1})),
        ("surface.thicken", json!({"bodies": ["Nope"], "thickness": 1})),
        ("surface.offset", json!({"distance": 1})),
        ("surface.offset", json!({"faces": null, "distance": 1})),
        ("surface.offset", json!({"faces": [], "distance": 1})),
        ("surface.offset", json!({"faces": 5, "distance": 1})),
        ("surface.offset", json!({"faces": ["top"], "distance": 1})),
        ("surface.offset", json!({"faces": points(10_001), "distance": 1})),
        ("surface.offset", json!({"faces": [[5, 5, 10]]})),
        ("surface.offset", json!({"faces": [[5, 5, 10]], "distance": "nope + 1"})),
        ("surface.offset", json!({"faces": [[5, 5, 10]], "distance": 1, "tolerance": "2 deg"})),
        ("surface.untrim", json!({})),
        ("surface.untrim", json!({"face": null})),
        ("surface.untrim", json!({"face": 5})),
        ("surface.untrim", json!({"face": {"point": [5, 5, 2]}})),
        ("surface.untrim", json!({"face": {"body": "Nope", "point": [5, 5, 2]}})),
        ("surface.fair", json!({"body": 5})),
        ("surface.fair", json!({"body": "Nope"})),
    ]);
    assert_strict_keeps_errors_on(cube, &[("surface.unstitch", json!({"body": "Box", "faces": points(10_001)}))]);
    assert_strict_keeps_errors_on(
        sheet,
        &[
            ("surface.stitch", json!({"bodies": ["Sheet"], "tolerance": "2 deg"})),
            ("surface.thicken", json!({"bodies": ["Sheet"], "thickness": "nope"})),
            ("surface.thicken", json!({"bodies": ["Sheet"], "thickness": 1, "operation": "glue"})),
            ("surface.thicken", json!({"bodies": ["Sheet"], "thickness": 1, "targets": 5})),
        ],
    );
    let many = vec![[22, 10, 2]; 10_001];
    assert_strict_keeps_errors_on(
        plate,
        &[
            ("surface.untrim", json!({"face": {"body": "Plate"}})),
            ("surface.untrim", json!({"face": {"body": "Plate", "point": [5, 5]}})),
            ("surface.untrim", json!({"face": plate_top(), "holes": "some"})),
            ("surface.untrim", json!({"face": plate_top(), "holes": 5})),
            ("surface.untrim", json!({"face": plate_top(), "holes": [[22, 10]]})),
            ("surface.untrim", json!({"face": plate_top(), "holes": many})),
            // Nothing to untrim: no holes, and the outer boundary kept.
            ("surface.untrim", json!({"face": plate_top(), "holes": []})),
        ],
    );
    assert_strict_keeps_errors_on(
        shell,
        &[
            ("surface.fair", json!({"body": "Shell", "strength": 2})),
            ("surface.fair", json!({"body": "Shell", "strength": 0})),
            ("surface.fair", json!({"body": "Shell", "iterations": 0})),
            ("surface.fair", json!({"body": "Shell", "iterations": 1001})),
            ("surface.fair", json!({"body": "Shell", "tolerance": "nope"})),
        ],
    );
}

/// #12627: keys of the wrong type that the handlers ignore today (an item of `faces` that isn't
/// a point is skipped; a `chain`, `outer` or `strength` that isn't the right type is read as left
/// out) are refused by the contracts: in Warn the call still works, with a warning.
#[test]
fn the_contracts_refuse_what_the_handlers_ignore() {
    let fair = |extra: Value| {
        let mut p = json!({"body": "Shell", "tolerance": 1.0});
        if let (Some(p), Value::Object(extra)) = (p.as_object_mut(), extra) {
            p.extend(extra);
        }
        p
    };
    let cases: Vec<(fn(ParamCheck) -> Session, &str, Value, &str)> = vec![
        (cube, "surface.unstitch", json!({"body": "Box", "faces": [[5, 5, 10], "top"]}), "faces"),
        (cube, "surface.unstitch", json!({"body": "Box", "faces": 5}), "faces"),
        (sheet, "surface.reverse_normal", json!({"body": "Sheet", "faces": "all"}), "faces"),
        (sheet, "surface.stitch", json!({"bodies": ["Sheet"], "tolerance": true}), "tolerance"),
        (sheet, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 1, "symmetric": "yes"}), "symmetric"),
        (sheet, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 1, "operation": 5}), "operation"),
        (cube, "surface.offset", json!({"faces": [[5, 5, 10], "top"], "distance": 1}), "faces"),
        (cube, "surface.offset", json!({"faces": [[5, 5, 10]], "distance": 1, "body": 5}), "body"),
        (cube, "surface.offset", json!({"faces": [[5, 5, 10]], "distance": 1, "chain": "yes"}), "chain"),
        (cube, "surface.offset", json!({"faces": [[5, 5, 10]], "distance": 1, "tolerance": true}), "tolerance"),
        (plate, "surface.untrim", json!({"face": plate_top(), "outer": "yes"}), "outer"),
        (plate, "surface.untrim", json!({"face": plate_top(), "in_place": 1}), "in_place"),
        (shell, "surface.fair", fair(json!({"strength": "0.5"})), "strength"),
        (shell, "surface.fair", fair(json!({"iterations": "3"})), "iterations"),
        (shell, "surface.fair", fair(json!({"keep_edges": "no"})), "keep_edges"),
    ];
    for (fixture, id, p, key) in cases {
        assert!(fixture(ParamCheck::Off).execute(id, &p).is_ok(), "{id} {p} works today");
        let warned = fixture(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = fixture(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id} {p}: {error}");
    }
}

/// #12627: the schemas take what the handlers take. Each call is checked against both the schema
/// and the handler, with the check off, so the two can't drift apart: Offset Surface on the cube,
/// Untrim on the plate and Fair Surface on the shell.
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
        "surface.offset",
        input_schema_for::<OffsetParams>(),
        &[
            json!({"faces": [[5, 5, 10]], "distance": 2}),
            json!({"faces": [{"x": 5, "y": 5, "z": 10}], "distance": "2 mm", "chain": true, "tolerance": "0.05 mm", "body": "Box"}),
            json!({"faces": [[5, 5, 10]], "distance": 0, "body_name": "Copy"}),
            json!({"faces": [], "distance": 2}),
            json!({"faces": [[5, 5]], "distance": 2}),
            json!({"faces": [[5, 5, 10]], "distance": ""}),
            json!({"faces": [[5, 5, 10]]}),
        ],
    );
    agree(
        plate,
        "surface.untrim",
        input_schema_for::<UntrimParams>(),
        &[
            json!({"face": plate_top()}),
            json!({"face": plate_top(), "holes": "all"}),
            json!({"face": plate_top(), "holes": null}),
            json!({"face": plate_top(), "holes": [[22, 10, 2]], "body_name": "Two"}),
            json!({"face": {"body": "Plate", "point": [5, 5, 2], "note": "top"}}),
            json!({"face": plate_top(), "holes": "All"}),
            json!({"face": plate_top(), "holes": [[22, 10]]}),
            json!({"face": {"body": 5, "point": [5, 5, 2]}}),
            json!({"holes": "all"}),
        ],
    );
    agree(
        shell,
        "surface.fair",
        input_schema_for::<FairParams>(),
        &[
            json!({"body": "Shell", "tolerance": 1.0}),
            json!({"body": "Shell", "tolerance": 1.0, "strength": 0.5, "iterations": 2.5, "keep_edges": false}),
            json!({"body": "Shell", "tolerance": 1.0, "strength": 1, "iterations": 1}),
            json!({"body": "Shell", "tolerance": 1.0, "strength": 1.5}),
            json!({"body": "Shell", "tolerance": 1.0, "iterations": 0.5}),
            json!({"body": "Shell", "tolerance": 1.0, "faces": [[0, 0, 10]]}),
        ],
    );
}

/// #12627: each command replies as a feature command does, in Warn without warnings: a cube taken
/// apart and stitched back, a sheet turned over and thickened, a face offset, the plate's top
/// untrimmed and the shell faired. A misspelt key is ignored today: Warn says so, and strict
/// refuses it.
#[test]
fn surface_edits_reply_with_their_schemas() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let check = |s: &mut Session, id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let mut s = cube(ParamCheck::Warn);
    check(&mut s, "surface.offset", json!({"faces": [[5, 5, 10]], "distance": 2, "body_name": "Lid"}));
    let apart = check(&mut s, "surface.unstitch", json!({"body": "Box"}));
    let pieces = apart["created_bodies"].clone();
    assert_eq!(pieces.as_array().map(Vec::len), Some(6), "six faces, six surfaces: {apart}");
    let back = check(&mut s, "surface.stitch", json!({"bodies": pieces}));
    assert_eq!(
        back["created_bodies"].as_array().map(Vec::len).unwrap_or(0) + back["changed_bodies"].as_array().map(Vec::len).unwrap_or(0),
        1,
        "{back}"
    );
    let mut s = sheet(ParamCheck::Warn);
    check(&mut s, "surface.reverse_normal", json!({"body": "Sheet"}));
    check(&mut s, "surface.thicken", json!({"bodies": "Sheet", "thickness": 2, "body_name": "Slab"}));
    let mut s = plate(ParamCheck::Warn);
    check(&mut s, "surface.untrim", json!({"face": plate_top(), "body_name": "Top"}));
    let mut s = shell(ParamCheck::Warn);
    let faired = check(&mut s, "surface.fair", json!({"body": "Shell", "tolerance": 1.0, "iterations": 3}));
    assert!(faired["warning"].is_string(), "the energies before and after: {faired}");
    // A misspelt key.
    let p = json!({"faces": [[5, 5, 10]], "distance": 2, "chian": true});
    let r = cube(ParamCheck::Warn).execute("surface.offset", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("chian"), "{r}");
    let error = cube(ParamCheck::Strict).execute("surface.offset", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`chian`"), "{error}");
}
