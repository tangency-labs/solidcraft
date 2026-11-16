use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::SurfaceRuledParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SurfaceRuledParams>(),
        &[
            "angle",
            "body",
            "body_name",
            "body_names",
            "corners",
            "direction",
            "distance",
            "edges",
            "face",
            "far",
            "flip",
            "mode",
            "name",
            "participants",
        ],
    );
}

/// Points on the four top edges of [`boxed`]'s `Box`, a chain round its top.
fn rim() -> Value {
    json!([[5, 0, 10], [10, 5, 10], [5, 10, 10], [0, 5, 10]])
}

/// A point on `Box`'s top face.
fn top() -> Value {
    json!([5, 5, 10])
}

/// A 10 mm cube `Box` from the origin, at `level`.
fn boxed(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"})).unwrap_or_else(|e| panic!("{e}"));
    s
}

/// Calls that fail today give the same message in strict, each on a fresh [`boxed`]: what the
/// contract checks (the body, the edges, the keywords, the direction, the face and the far
/// handles) and what it leaves to the handler (a body the design lacks, points off its edges or
/// faces, a distance or angle that doesn't evaluate or has the wrong unit, a zero distance, and
/// the angle or direction a mode needs). Left out: a missing `edges`, and a `distance` that isn't
/// a number or text, whose messages differ on purpose, as for every converted command.
#[test]
fn strict_keeps_todays_errors() {
    let with = |extra: Value| {
        let mut p = json!({"body": "Box", "edges": rim(), "distance": 2});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    let cases = [
        json!({}),
        json!({"body": 5, "edges": rim(), "distance": 2}),
        json!({"body": null, "edges": rim(), "distance": 2}),
        json!({"body": "Nope", "edges": rim(), "distance": 2}),
        with(json!({"edges": 5})),
        with(json!({"edges": []})),
        with(json!({"edges": null})),
        with(json!({"edges": ["x"]})),
        with(json!({"edges": vec![[5, 0, 10]; 10_001]})),
        with(json!({"edges": [[500, 500, 500]]})),
        json!({"body": "Box", "edges": rim()}),
        with(json!({"distance": "nonsense"})),
        with(json!({"distance": "30 deg"})),
        with(json!({"distance": 0})),
        with(json!({"mode": "sideways"})),
        with(json!({"mode": "SIDEWAYS"})),
        with(json!({"mode": "angle"})),
        with(json!({"mode": "angle", "angle": null})),
        with(json!({"mode": "angle", "angle": "5 mm"})),
        with(json!({"mode": "angle", "angle": 45, "direction": [1, 2]})),
        with(json!({"mode": "direction"})),
        with(json!({"mode": "direction", "direction": [0, 0, 0]})),
        with(json!({"mode": "direction", "direction": "up"})),
        with(json!({"corners": "square"})),
        with(json!({"face": "top"})),
        with(json!({"far": 5})),
        with(json!({"far": [5]})),
        with(json!({"far": [["x"]]})),
        with(json!({"far": [null, [[1, 2]]]})),
    ];
    for p in cases {
        let error = |level: ParamCheck| boxed(level).execute("surface.ruled", &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #13134: the contract takes what its input schema allows: each call's check finds no issue
/// exactly when the call matches the schema. A call in angle mode without `angle` matches it; the
/// handler refuses it. The one place the schema can't follow the check is a zero `direction`,
/// which it can't say isn't allowed; a last call shows it.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<SurfaceRuledParams>();
    let schema = schema.as_value();
    let p = |extra: Value| {
        let mut p = json!({"body": "B", "edges": [[1, 2, 3]], "distance": 2});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        p
    };
    for (p, takes) in [
        (p(json!({})), true),
        (
            p(
                json!({"edges": [{"x": 1, "y": 2, "z": 3}], "distance": "2 mm", "mode": "tangent", "face": [1, 2, 3], "flip": true, "corners": "round", "name": "S", "body_name": "Strip"}),
            ),
            true,
        ),
        (p(json!({"mode": "angle", "angle": "45 deg", "direction": [0, 0, 1]})), true),
        (p(json!({"mode": "direction", "direction": {"x": 0, "y": -1, "z": 0}, "corners": "miter"})), true),
        (p(json!({"far": [null, [[1, 2, 3], [4, 5, 6]], []], "body_names": ["Strip"], "participants": ["*"]})), true),
        (p(json!({"mode": null, "angle": null, "direction": null, "face": null, "flip": null, "corners": null, "far": null})), true),
        (p(json!({"mode": "angle"})), true),
        (json!({"edges": [[1, 2, 3]], "distance": 2}), false),
        (p(json!({"body": 1})), false),
        (p(json!({"edges": []})), false),
        (p(json!({"edges": 5})), false),
        (p(json!({"edges": [[1, 2]]})), false),
        (p(json!({"edges": [[1, 2, 3], "x"]})), false),
        (json!({"body": "B", "edges": [[1, 2, 3]]}), false),
        (p(json!({"distance": true})), false),
        (p(json!({"mode": "sideways"})), false),
        (p(json!({"mode": 3})), false),
        (p(json!({"angle": true})), false),
        (p(json!({"direction": [1, 2]})), false),
        (p(json!({"direction": "up"})), false),
        (p(json!({"face": "top"})), false),
        (p(json!({"flip": "yes"})), false),
        (p(json!({"corners": "square"})), false),
        (p(json!({"far": 5})), false),
        (p(json!({"far": [5]})), false),
        (p(json!({"far": [["x"]]})), false),
        (p(json!({"plane": "XY"})), false),
    ] {
        let issues = crate::schema::issues("surface.ruled", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
    let zero = p(json!({"mode": "direction", "direction": [0, 0, 0]}));
    assert!(!crate::schema::issues("surface.ruled", &zero).is_empty() && conforms(schema, schema, &zero), "{zero}");
}

/// #13134: Ruled Surface replies with `FeatureResult`, with the keys it gave before, in each mode
/// and with each optional key: in Warn, without warnings, so its contract takes each call. Each
/// call makes one surface body and changes no other.
#[test]
fn ruled_surface_replies_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let before = BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms"]);
    let calls = [
        json!({"body": "Box", "edges": rim(), "distance": "2 mm", "mode": "tangent", "face": top(), "body_name": "Flange"}),
        json!({"body": "Box", "edges": rim(), "distance": 2, "face": top()}),
        json!({"body": "Box", "edges": rim(), "distance": 2, "mode": "ANGLE", "angle": "45 deg", "corners": "round", "face": top(), "name": "Draft"}),
        json!({"body": "Box", "edges": [[5, 0, 10]], "distance": 2, "mode": "direction", "direction": [0, -1, 0], "body_name": "Lip"}),
        json!({"body": "Box", "edges": rim(), "distance": 2, "face": top(), "flip": true, "corners": "miter", "far": [null, null, null, null]}),
        json!({"body": "Box", "edges": rim(), "distance": 2, "face": top(), "mode": null, "angle": null, "direction": null, "body_names": ["Rim"]}),
    ];
    for p in calls {
        let mut s = boxed(ParamCheck::Warn);
        let r = s.execute("surface.ruled", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, before, "{p}: {r}");
        let count = |key: &str| r[key].as_array().map_or(usize::MAX, Vec::len);
        assert_eq!((count("created_bodies"), count("changed_bodies"), count("removed_bodies")), (1, 0, 0), "{p}: {r}");
    }
}
