use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{SplitBodyParams, SplitFaceParams};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SplitBodyParams>(),
        &["body", "body_name", "body_names", "line", "name", "participants", "plane", "sketch", "tool"],
    );
    assert_keys(
        input_schema_for::<SplitFaceParams>(),
        &["body", "body_name", "body_names", "faces", "name", "participants", "plane", "sketch", "tool"],
    );
}

/// The design the split calls work on, at `level`, and the sketch ids `Cuts` and `Ring`:
/// - `B`, a 40 x 30 x 20 mm box from the origin (its top at z = 20);
/// - `Tool`, a box whose face at y = 10, extended, crosses `B`'s top (the point [20, 10, 27] is
///   on it);
/// - the sketch `Cuts` on XY, two lines across `B` at x = 10 (id `cut`) and x = 30;
/// - the sketch `Ring` on XY, a circle `c` beside `B`, so no straight line;
/// - the construction plane `Mid`, 20 mm along X from YZ, through the middle of `B`.
fn design(level: ParamCheck) -> (Session, Value, Value, Value) {
    let mut s = Session { param_check: level, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
    run("solid.box", json!({"corner": [-5, 10, 25], "length": 50, "width": 5, "height": 5, "body_name": "Tool"}));
    let cuts = run("sketch.create", json!({"plane": "XY", "name": "Cuts"}))["sketch"].clone();
    run("sketch.line", json!({"points": [[10, -5], [10, 35]], "ids": ["cut"]}));
    run("sketch.line", json!({"points": [[30, -5], [30, 35]]}));
    run("sketch.finish", json!({}));
    let ring = run("sketch.create", json!({"plane": "XY", "name": "Ring"}))["sketch"].clone();
    run("sketch.circle.center", json!({"center": [60, 60], "radius": 2, "id": "c"}));
    run("sketch.finish", json!({}));
    let mid = run("construct.plane.offset", json!({"base": "YZ", "offset": 20, "name": "Mid"}))["feature"].clone();
    assert!(cuts.is_u64() && ring.is_u64() && mid.is_u64(), "{cuts} {ring} {mid}");
    (s, cuts, ring, mid)
}

/// Calls that fail today give the same message in strict, each on a fresh [`design`]: what the
/// contracts check (the body, the faces, each tool's form) and what they leave to the handler (a
/// body, sketch, line, plane or face the design lacks, a sketch line that isn't straight, a tool
/// point off the tool, and no tool at all). Split Face without `faces` is left out: today it says
/// "`faces` must be a list of points or face names", and strict "`faces` is required", as for
/// every missing key.
#[test]
fn strict_keeps_todays_errors() {
    let (_, cuts, ring, _) = design(ParamCheck::Off);
    let cases = [
        ("solid.split_body", json!({})),
        ("solid.split_body", json!({"plane": "XY"})),
        ("solid.split_body", json!({"body": 5, "plane": "XY"})),
        ("solid.split_body", json!({"body": null, "plane": "XY"})),
        ("solid.split_body", json!({"body": "Nope", "plane": "XY"})),
        ("solid.split_body", json!({"body": "B"})),
        ("solid.split_body", json!({"body": "B", "plane": null, "tool": null, "sketch": null, "line": null})),
        ("solid.split_body", json!({"body": "B", "plane": true})),
        ("solid.split_body", json!({"body": "B", "plane": 7.5})),
        ("solid.split_body", json!({"body": "B", "plane": -1})),
        ("solid.split_body", json!({"body": "B", "plane": "Nowhere"})),
        ("solid.split_body", json!({"body": "B", "plane": "Cuts"})),
        ("solid.split_body", json!({"body": "B", "plane": {"origin": [0, 0, 0]}})),
        ("solid.split_body", json!({"body": "B", "plane": {"origin": "here", "normal": [1, 0, 0]}})),
        ("solid.split_body", json!({"body": "B", "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}})),
        ("solid.split_body", json!({"body": "B", "tool": "Tool"})),
        ("solid.split_body", json!({"body": "B", "tool": {"point": [20, 10, 27]}})),
        ("solid.split_body", json!({"body": "B", "tool": {"body": 5, "point": [20, 10, 27]}})),
        ("solid.split_body", json!({"body": "B", "tool": {"body": "Tool"}})),
        ("solid.split_body", json!({"body": "B", "tool": {"body": "Tool", "point": [20, 10]}})),
        ("solid.split_body", json!({"body": "B", "tool": {"body": "Nope", "point": [20, 10, 27]}})),
        ("solid.split_body", json!({"body": "B", "tool": {"body": "Tool", "point": [500, 500, 500]}})),
        ("solid.split_body", json!({"body": "B", "sketch": "Cuts", "line": "cut"})),
        ("solid.split_body", json!({"body": "B", "sketch": -1, "line": "cut"})),
        ("solid.split_body", json!({"body": "B", "sketch": 999, "line": "cut"})),
        ("solid.split_body", json!({"body": "B", "sketch": cuts})),
        ("solid.split_body", json!({"body": "B", "sketch": cuts, "line": null})),
        ("solid.split_body", json!({"body": "B", "sketch": cuts, "line": 5})),
        ("solid.split_body", json!({"body": "B", "sketch": cuts, "line": "nope"})),
        ("solid.split_body", json!({"body": "B", "sketch": ring, "line": "c"})),
        ("solid.split_body", json!({"body": "B", "line": "cut"})),
        ("solid.split_face", json!({"faces": 5, "plane": "XY"})),
        ("solid.split_face", json!({"faces": [], "plane": "XY"})),
        ("solid.split_face", json!({"faces": [[10, 15]], "plane": "XY"})),
        ("solid.split_face", json!({"faces": [[500, 500, 500]], "plane": "Mid"})),
        ("solid.split_face", json!({"faces": ["NoSuchFace"], "plane": "Mid"})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "body": "Nope", "plane": "Mid"})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]]})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "plane": null, "tool": null, "sketch": null})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "plane": true})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "plane": "Nowhere"})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"normal": [0, 0, 0]}})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "tool": "Tool"})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "tool": {"body": "Tool", "point": "here"}})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "tool": {"body": "Nope", "point": [20, 10, 27]}})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "sketch": true})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "sketch": "Nope"})),
    ];
    for (id, p) in cases {
        let error = |level: ParamCheck| design(level).0.execute(id, &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #13127: each contract takes what its input schema allows: each call's check finds no issue
/// exactly when the call matches the schema. A call that gives no tool matches it; the handler
/// refuses it.
#[test]
fn the_check_takes_what_the_schema_allows() {
    let body = input_schema_for::<SplitBodyParams>();
    let face = input_schema_for::<SplitFaceParams>();
    for (id, schema, p, takes) in [
        ("solid.split_body", &body, json!({"body": "B", "plane": "XY"}), true),
        ("solid.split_body", &body, json!({"body": "B", "plane": 3}), true),
        (
            "solid.split_body",
            &body,
            json!({"body": "B", "plane": {"origin": [0, "w", 0], "normal": [1, 0, 0]}, "name": "S", "body_names": ["L", "R"]}),
            true,
        ),
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": "T", "point": [0, 0, 0]}}), true),
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": "T", "point": {"x": 0, "y": 0, "z": 0}, "side": 1}}), true),
        ("solid.split_body", &body, json!({"body": "B", "sketch": 4, "line": "cut", "participants": ["*"]}), true),
        ("solid.split_body", &body, json!({"body": "B", "sketch": null, "line": null, "tool": null, "plane": null}), true),
        ("solid.split_body", &body, json!({"body": "B", "line": "cut"}), true),
        ("solid.split_body", &body, json!({"body": ""}), true),
        ("solid.split_body", &body, json!({}), false),
        ("solid.split_body", &body, json!({"body": 5, "plane": "XY"}), false),
        ("solid.split_body", &body, json!({"body": "B", "plane": true}), false),
        ("solid.split_body", &body, json!({"body": "B", "plane": -1}), false),
        ("solid.split_body", &body, json!({"body": "B", "plane": {"origin": [0, 0, 0]}}), false),
        ("solid.split_body", &body, json!({"body": "B", "tool": "T"}), false),
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": "T"}}), false),
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": 1, "point": [0, 0, 0]}}), false),
        ("solid.split_body", &body, json!({"body": "B", "sketch": "Cuts", "line": "cut"}), false),
        ("solid.split_body", &body, json!({"body": "B", "sketch": -2, "line": "cut"}), false),
        ("solid.split_body", &body, json!({"body": "B", "sketch": 4, "line": 7}), false),
        ("solid.split_body", &body, json!({"body": "B", "plane": "XY", "faces": [[0, 0, 0]]}), false),
        ("solid.split_body", &body, json!({"body": "B", "plane": "XY", "body_name": 3}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "plane": "XY"}), true),
        (
            "solid.split_face",
            &face,
            json!({"faces": ["F1", {"point": [1, 2, 3]}], "body": "B", "plane": {"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}}),
            true,
        ),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "tool": {"body": "T", "point": [1, 2, 3]}}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": "Cuts"}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": 7, "name": "Split", "participants": ["*"]}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "tool": null, "sketch": null, "plane": null, "body": null}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]]}), true),
        ("solid.split_face", &face, json!({"plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [], "plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2]], "plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "body": 5, "plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": true}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "tool": {"point": [1, 2, 3]}}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "plane": "XY", "line": "cut"}), false),
    ] {
        let schema = schema.as_value();
        let issues = crate::schema::issues(id, &p);
        assert_eq!(issues.is_empty(), takes, "{id} {p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{id} {p}");
    }
}

/// #13127: both commands reply with `FeatureResult`, with the keys they gave before, for each
/// tool: in Warn, without warnings, so their contracts take each call. Split Face changes `B` and
/// makes nothing; Split Body cuts `B` in two, so two bodies are made or changed between them.
#[test]
fn split_body_and_split_face_reply_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let before = BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms"]);
    let (_, cuts, _, mid) = design(ParamCheck::Warn);
    let calls = [
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}})),
        ("solid.split_face", json!({"faces": [[10, 15, 20]], "body": "B", "plane": "Mid", "name": "Halves"})),
        ("solid.split_face", json!({"faces": [[20, 5, 20]], "body": "B", "tool": {"body": "Tool", "point": [20, 10, 27]}})),
        ("solid.split_face", json!({"faces": [[20, 15, 20]], "sketch": "Cuts", "tool": null, "plane": null})),
        ("solid.split_face", json!({"faces": [[20, 15, 20]], "sketch": cuts.clone(), "participants": ["B"]})),
        ("solid.split_body", json!({"body": "B", "plane": {"origin": [20, 0, "0 mm"], "normal": [1, 0, 0]}})),
        ("solid.split_body", json!({"body": "B", "plane": mid, "name": "Halves"})),
        ("solid.split_body", json!({"body": "B", "plane": "Mid", "tool": null, "sketch": null, "line": null})),
        ("solid.split_body", json!({"body": "B", "tool": {"body": "Tool", "point": [20, 10, 27]}})),
        ("solid.split_body", json!({"body": "B", "sketch": cuts, "line": "cut"})),
    ];
    for (id, p) in calls {
        let (mut s, ..) = design(ParamCheck::Warn);
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, before, "{id} {p}: {r}");
        let count = |key: &str| r[key].as_array().map_or(0, Vec::len);
        if id == "solid.split_face" {
            assert_eq!((count("created_bodies"), r["changed_bodies"].clone()), (0, json!(["B"])), "{id} {p}: {r}");
        } else {
            assert!(count("created_bodies") >= 1 && count("created_bodies") + count("changed_bodies") == 2, "{id} {p}: {r}");
        }
    }
}
