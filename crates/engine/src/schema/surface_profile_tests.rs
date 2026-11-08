use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{SurfaceExtrudeParams, SurfaceRevolveParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SurfaceExtrudeParams>(),
        &[
            "body_name",
            "body_names",
            "convention",
            "curves",
            "direction",
            "distance",
            "distance2",
            "exact_regions",
            "name",
            "participants",
            "profiles",
            "sketch",
            "start_offset",
        ],
    );
    assert_keys(
        input_schema_for::<SurfaceRevolveParams>(),
        &[
            "angle",
            "angle2",
            "axis",
            "body_name",
            "body_names",
            "curves",
            "direction",
            "exact_regions",
            "name",
            "participants",
            "profiles",
            "sketch",
            "symmetric",
        ],
    );
}

/// Calls with one problem that fail today give the same message in strict: what the contracts
/// check (a sketch, curves, a profile pick, a direction, a convention, and each form of a revolve
/// axis that isn't one) and what they leave to the handler (a sketch, curve, profile or axis the
/// design lacks, and an expression that doesn't evaluate or isn't a length or an angle).
#[test]
fn strict_keeps_todays_errors() {
    let extrude = |p: Value| ("surface.extrude", p);
    let revolve = |p: Value| ("surface.revolve", p);
    assert_strict_keeps_todays_errors(&[
        extrude(json!({"sketch": "Nope", "distance": 5})),
        extrude(json!({"sketch": true, "distance": 5})),
        extrude(json!({"sketch": null, "distance": 5})),
        extrude(json!({"curves": 5, "distance": 5})),
        extrude(json!({"curves": ["c1", 2], "distance": 5})),
        extrude(json!({"curves": ["nope"], "distance": 5})),
        extrude(json!({"profiles": "most", "distance": 5})),
        extrude(json!({"profiles": [9], "distance": 5})),
        extrude(json!({"distance": "nonsense"})),
        extrude(json!({"distance": "30 deg"})),
        extrude(json!({"distance": 0})),
        extrude(json!({"distance": 5, "direction": "sideways"})),
        extrude(json!({"distance": 5, "convention": "per_side"})),
        extrude(json!({"distance": 5, "distance2": "nonsense"})),
        extrude(json!({"distance": 5, "start_offset": "30 deg"})),
        revolve(json!({"sketch": "Nope", "axis": "Y"})),
        revolve(json!({"curves": 5, "axis": "Y"})),
        revolve(json!({"axis": 5})),
        revolve(json!({"axis": null})),
        revolve(json!({"axis": [0, 1, 0]})),
        revolve(json!({"axis": "nope"})),
        revolve(json!({"axis": {"type": "world", "axis": "W"}})),
        revolve(json!({"axis": {"origin": [0, 0, 0]}})),
        revolve(json!({"axis": {"type": "vertex", "at": [0, 0, 0]}})),
        revolve(json!({"axis": {"dir": "up"}})),
        revolve(json!({"axis": {"dir": [0, 0, 0]}})),
        revolve(json!({"axis": {"origin": "here", "dir": [0, 1, 0]}})),
        revolve(json!({"axis": {"origin": ["nonsense", 0, 0], "dir": [0, 1, 0]}})),
        revolve(json!({"axis": {"edge": [500, 500, 500]}})),
        revolve(json!({"axis": {"type": "edge"}})),
        revolve(json!({"axis": "Y", "angle": "nonsense"})),
        revolve(json!({"axis": "Y", "angle2": "30 mm"})),
    ]);
}

/// #13081: each contract takes what its input schema allows: each call's check finds no issue
/// exactly when the call matches the schema. Keywords are lowercase here: the check ignores their
/// case, which a schema's `enum` can't say (the descriptions do).
#[test]
fn the_check_takes_what_the_schema_allows() {
    let extrude = input_schema_for::<SurfaceExtrudeParams>();
    let revolve = input_schema_for::<SurfaceRevolveParams>();
    let cases = [
        ("surface.extrude", extrude.as_value(), json!({"distance": 5}), true),
        (
            "surface.extrude",
            extrude.as_value(),
            json!({
                "distance": "4 mm", "sketch": "Profile", "curves": "side", "profiles": "all", "exact_regions": true,
                "direction": "negative", "distance2": 2, "start_offset": "1 mm", "convention": "legacy_per_side",
                "name": "Walls", "body_name": "B", "body_names": ["B"], "participants": ["*"]
            }),
            true,
        ),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "sketch": 3, "profiles": [0, 1], "curves": null, "direction": null}), true),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "profiles": [{"point": [1, 2]}], "distance2": null}), true),
        ("surface.extrude", extrude.as_value(), json!({}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": ""}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": true}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "sketch": null}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "sketch": [1]}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "curves": [1]}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "profiles": null}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "direction": "up"}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "convention": "per_side"}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "distance2": true}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "exact_regions": "yes"}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "name": 5}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "through_all": true}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "taper": 5}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "to": [0, 0, 1]}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "corner": "round"}), false),
        ("surface.extrude", extrude.as_value(), json!({"distance": 5, "operation": "join"}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y"}), true),
        (
            "surface.revolve",
            revolve.as_value(),
            json!({
                "axis": "spine", "sketch": "Profile", "curves": ["side"], "profiles": [0], "exact_regions": false,
                "angle": "90 deg", "angle2": 30, "symmetric": false, "direction": "symmetric",
                "name": "Turn", "body_name": "B", "body_names": "B", "participants": ["*"]
            }),
            true,
        ),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"type": "world", "axis": "Z"}}), true),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"dir": [0, 1, 0]}, "angle": null, "direction": "one_side"}), true),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"origin": ["w", 0, "1 mm"], "dir": {"x": 0, "y": 1, "z": 0}}}), true),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"dir": [0, 0, 0]}}), true),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"edge": [1, 2, 3]}}), true),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"face": [1, 2, 3]}}), true),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"type": "edge", "at": [1, 2, 3]}}), true),
        ("surface.revolve", revolve.as_value(), json!({}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": null}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": 5}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": [0, 1, 0]}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"type": "world", "axis": "x"}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"type": "world"}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"origin": [0, 0, 0]}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"dir": [0, 1]}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"origin": "here", "dir": [0, 1, 0]}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"edge": "here"}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": {"type": "vertex", "at": [0, 0, 0], "edge": [1, 2, 3]}}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y", "angle": true}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y", "symmetric": "yes"}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y", "direction": "negative"}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y", "direction": 1}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y", "distance": 5}), false),
        ("surface.revolve", revolve.as_value(), json!({"axis": "Y", "convention": "sketch_normal"}), false),
    ];
    for (id, schema, p, takes) in cases {
        let issues = crate::schema::issues(id, &p);
        assert_eq!(issues.is_empty(), takes, "{id} {p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{id} {p}");
    }
    // Where the schema can't follow the check: serde reads a saved edge whole.
    let p = json!({"axis": {"type": "edge", "at": "x"}});
    let schema = revolve.as_value();
    assert!(!crate::schema::issues("surface.revolve", &p).is_empty() && conforms(schema, schema, &p), "{p}");
}

/// A design with sketch `Profile` on XY, clear of the Y axis: a 5 × 6 rectangle from [5, 0] (its
/// one profile), a line `side` from [12, 0] to [12, 6] beside it, and a line `spine` along the Y
/// axis to revolve about.
fn profile_sketch() -> Session {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "Profile"})),
        ("sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [10, 6]})),
        ("sketch.line", json!({"points": [[12, 0], [12, 6]], "ids": ["side"]})),
        ("sketch.line", json!({"points": [[0, -5], [0, 10]], "ids": ["spine"]})),
        ("sketch.finish", json!({})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s
}

/// #13081: Surface Extrude and Surface Revolve reply with `FeatureResult`, with the keys they gave
/// before: from profiles and from curves, with each extent key, and about each form of axis. Each
/// lists its new body as created. In Warn, without warnings, so the contracts take each call.
#[test]
fn surface_extrude_and_revolve_reply_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let before = BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms"]);
    let mut s = profile_sketch();
    s.param_check = ParamCheck::Warn;
    for (id, p, body) in [
        ("surface.extrude", json!({"sketch": "Profile", "distance": 4, "body_name": "A"}), "A"),
        (
            "surface.extrude",
            json!({"sketch": "Profile", "curves": "side", "distance": "4 mm", "direction": "symmetric", "convention": "legacy_per_side", "body_name": "B"}),
            "B",
        ),
        (
            "surface.extrude",
            json!({"profiles": [0], "exact_regions": true, "distance": 3, "distance2": 2, "start_offset": 1, "name": "Walls", "body_names": ["C"]}),
            "C",
        ),
        ("surface.revolve", json!({"sketch": "Profile", "axis": "Y", "body_name": "D"}), "D"),
        (
            "surface.revolve",
            json!({"sketch": "Profile", "curves": ["side"], "axis": {"type": "world", "axis": "Y"}, "angle": "90 deg", "body_name": "E"}),
            "E",
        ),
        (
            "surface.revolve",
            json!({"sketch": "Profile", "curves": "side", "axis": {"origin": [0, 0, 0], "dir": [0, 1, 0]}, "angle": 90, "angle2": "45 deg", "body_name": "F"}),
            "F",
        ),
        (
            "surface.revolve",
            json!({"sketch": "Profile", "curves": ["side"], "axis": "spine", "angle": 120, "symmetric": true, "body_name": "G"}),
            "G",
        ),
        ("surface.revolve", json!({"profiles": [0], "axis": "y", "angle": 180, "direction": "Symmetric", "name": "Half", "body_name": "H"}), "H"),
    ] {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, before, "{id} {p}: {r}");
        let lists = (&r["created_bodies"], &r["changed_bodies"], &r["removed_bodies"]);
        assert_eq!(lists, (&json!([body]), &json!([]), &json!([])), "{id} {p}: {r}");
    }
}
