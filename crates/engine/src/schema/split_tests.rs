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
        &[
            "body",
            "body_name",
            "body_names",
            "line",
            "name",
            "participants",
            "plane",
            "sketch",
            "tool",
            "tool_bodies",
            "tool_faces",
            "tool_plane",
            "tool_sheet",
        ],
    );
    assert_keys(
        input_schema_for::<SplitFaceParams>(),
        &["body", "body_name", "body_names", "faces", "loops", "name", "participants", "plane", "sketch", "tool"],
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
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": "T", "point": [0, 0, 0]}, "tool_plane": true}), true),
        (
            "solid.split_body",
            &body,
            json!({"body": "B", "tool": {"body": "T", "point": [0, 0, 0]}, "tool_sheet": "body", "tool_bodies": ["U", "V"]}),
            true,
        ),
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": "T", "point": [0, 0, 0]}, "tool_sheet": "body", "tool_bodies": "U"}), false),
        ("solid.split_body", &body, json!({"body": "B", "tool": {"body": "T", "point": [0, 0, 0]}, "tool_sheet": "body", "tool_bodies": [1]}), false),
        ("solid.split_body", &body, json!({"body": "B", "tool_plane": "yes"}), false),
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
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": "Cuts", "loops": true}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": "Cuts", "loops": null}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "tool": null, "sketch": null, "plane": null, "body": null}), true),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]]}), true),
        ("solid.split_face", &face, json!({"plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [], "plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2]], "plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "body": 5, "plane": "XY"}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": true}), false),
        ("solid.split_face", &face, json!({"faces": [[1, 2, 3]], "sketch": "Cuts", "loops": "yes"}), false),
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
        ("solid.split_body", json!({"body": "B", "tool": {"body": "Tool", "point": [20, 10, 27]}, "tool_plane": true})),
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

#[test]
fn a_planar_split_tool_refuses_a_curved_face_without_changing_the_design() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    s.execute("solid.box", &json!({"length":40,"width":30,"height":20,"body_name":"Target"})).unwrap();
    s.execute("solid.cylinder", &json!({"diameter":20,"height":40,"base":[20,15,-10],"operation":"new","body_name":"Tool"})).unwrap();
    let before = s.doc.to_json();
    let error = s.execute("solid.split_body", &json!({"body":"Target","tool":{"body":"Tool","point":[30,15,5]},"tool_plane":true})).unwrap_err();
    assert!(error.to_string().contains("no longer planar"), "{error}");
    assert_eq!(s.doc.to_json(), before);
}

/// A 10 mm cube from (0, 0, 5), and `Sheet`, a surface of two faces across it: a line at y = 5
/// from x = -3 to 5, then a line rising 1 in 2 to (13, 9), extruded 20 mm up from XY. The
/// first face is the plane y = 5 (the point [1, 5, 10] is on it), the second rises from x = 5
/// (the point [9, 7, 10] is on it), so they meet at a crease inside the cube.
fn bent_sheet() -> Session {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("solid.box", json!({"corner": [0, 0, 5], "length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run("sketch.create", json!({"plane": "XY", "name": "Bend"}));
    let curves = run("sketch.line", json!({"points": [[-3, 5], [5, 5], [13, 9]]}))["created"]["curves"].clone();
    run("sketch.finish", json!({}));
    run("surface.extrude", json!({"sketch": "Bend", "curves": curves, "distance": 20, "body_name": "Sheet"}));
    s
}

/// The volumes of the bodies made from `Cube`, smallest first.
fn cube_parts(s: &Session) -> Vec<f64> {
    let st = s.model.state();
    let mut v: Vec<f64> =
        st.bodies.iter().filter(|b| b.name.starts_with("Cube")).map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).collect();
    v.sort_by(f64::total_cmp);
    v
}

/// #9305: Split Body cuts with the picked faces of a surface body as they are, or with the whole
/// surface body, not only with one face extended. The first face extended is the plane y = 5,
/// which halves the cube (500 mm³ each). Both faces as they are leave under them
/// 10 · (5 · 5 + ∫ (5 + (x - 5) / 2) dx over x 5…10) = 562.5 mm³, and 437.5 mm³ above. The
/// choice is saved, and a reopened design builds the same parts.
#[test]
fn split_body_cuts_with_picked_faces_or_the_whole_surface_body() {
    let near = |v: &[f64], want: [f64; 2]| v.len() == 2 && v.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6);
    let tool = json!({"body": "Sheet", "point": [1, 5, 10]});
    let mut s = bent_sheet();
    s.execute("solid.split_body", &json!({"body": "Cube", "tool": tool})).unwrap();
    assert!(near(&cube_parts(&s), [500.0, 500.0]), "{:?}", cube_parts(&s));
    for (sheet, extra) in [("faces", json!({"tool_faces": [[9, 7, 10]]})), ("body", json!({}))] {
        let mut s = bent_sheet();
        let mut p = json!({"body": "Cube", "tool": tool, "tool_sheet": sheet});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        let id = s.execute("solid.split_body", &p).unwrap_or_else(|e| panic!("{p}: {e}"))["feature"].as_u64().unwrap();
        assert!(near(&cube_parts(&s), [437.5, 562.5]), "{p}: {:?}", cube_parts(&s));
        let saved = s.doc.to_json();
        assert!(saved.contains(&format!("\"tool_sheet\": \"{sheet}\"")), "{saved}");
        let reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!(reopened.model.result(id).unwrap().error.is_none(), "{p}");
        assert!(near(&cube_parts(&reopened), [437.5, 562.5]), "{p}: {:?}", cube_parts(&reopened));
    }
}

/// #9305 review: tool faces used as they are cut where the tool is shown. The sheet is built in a
/// component moved 2 mm along y, so it runs along y = 7 and its crease is at x = 5; the root
/// cube under it keeps 10 · (5 · 7 + ∫ (7 + (x - 5) / 2) dx over x 5…10) = 762.5 mm³ and
/// 237.5 mm³ above, not the 562.5 and 437.5 of the sheet's own frame. The tool points are where
/// the faces are shown.
#[test]
fn split_body_by_faces_meets_a_moved_tool_where_it_is_shown() {
    let near = |v: &[f64], want: [f64; 2]| v.len() == 2 && v.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6);
    for (sheet, extra) in [("faces", json!({"tool_faces": [[9, 9, 10]]})), ("body", json!({}))] {
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        run("solid.box", json!({"corner": [0, 0, 5], "length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
        run("component.create", json!({"name": "Part", "grounded": false}));
        run("sketch.create", json!({"plane": "XY", "name": "Bend"}));
        let curves = run("sketch.line", json!({"points": [[-3, 5], [5, 5], [13, 9]]}))["created"]["curves"].clone();
        run("sketch.finish", json!({}));
        run("surface.extrude", json!({"sketch": "Bend", "curves": curves, "distance": 20, "body_name": "Sheet"}));
        run("component.activate", json!({"component": "root"}));
        run("occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 2, 0]}));
        run("component.capture_position", json!({}));
        let mut p = json!({"body": "Cube", "tool": {"body": "Sheet", "point": [1, 7, 10]}, "tool_sheet": sheet});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        run("solid.split_body", p.clone());
        assert!(near(&cube_parts(&s), [237.5, 762.5]), "{p}: {:?}", cube_parts(&s));
    }
}

/// #9305: what Split Body by faces refuses, each leaving the design as it was: the first face
/// alone (it ends inside the cube), `tool_faces` without `tool_sheet` "faces", a point off the
/// tool, the whole of a solid, an infinite plane with a sheet, and a mode it does not know.
#[test]
fn split_body_by_faces_refuses_what_does_not_cut_through() {
    let mut s = bent_sheet();
    s.execute("solid.box", &json!({"corner": [30, 0, 0], "length": 5, "width": 5, "height": 5, "body_name": "Block"})).unwrap();
    let tool = json!({"body": "Sheet", "point": [1, 5, 10]});
    for (p, says) in [
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "faces"}), "do not cut the body through"),
        (json!({"body": "Cube", "tool": tool, "tool_faces": [[9, 7, 10]]}), "needs `tool_sheet`"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "faces", "tool_faces": [[500, 500, 500]]}), "not on a face"),
        (json!({"body": "Cube", "tool": {"body": "Block", "point": [30, 2, 2]}, "tool_sheet": "body"}), "not a surface body"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "body", "tool_plane": true}), "takes no `tool_sheet`"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "nope"}), "tool_sheet"),
    ] {
        let before = s.doc.to_json();
        let e = s.execute("solid.split_body", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
}

/// A 10 mm cube from (0, 0, 5), and three flat surface bodies, each a sketch line on XY extruded
/// 20 mm up, so each crosses the whole cube: `AtX3` the plane x = 3 (the point [3, 2, 10] is on
/// it), `AtX8` the plane x = 8, and `AtY6` the plane y = 6. `Away`, the plane x = 50, misses the
/// cube.
fn flat_sheets() -> Session {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("solid.box", json!({"corner": [0, 0, 5], "length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    for (name, a, b) in [("AtX3", [3, -5], [3, 15]), ("AtX8", [8, -5], [8, 15]), ("AtY6", [-5, 6], [15, 6]), ("Away", [50, -5], [50, 15])] {
        run("sketch.create", json!({"plane": "XY", "name": name}));
        let curves = run("sketch.line", json!({"points": [a, b]}))["created"]["curves"].clone();
        run("sketch.finish", json!({}));
        run("surface.extrude", json!({"sketch": name, "curves": curves, "distance": 20, "body_name": name}));
    }
    s
}

/// #13303: Split Body with the whole surface body cuts with several surface bodies at once, each
/// as it is. `AtX3` and `AtX8` cut the cube into slabs 3, 5 and 2 mm thick: 300, 500 and
/// 200 mm³. `AtX8` crosses only the piece past x = 3 and leaves the other. `AtX3` and `AtY6`
/// cross: `AtY6` cuts both pieces `AtX3` leaves, into 3 · 6 · 10 = 180, 7 · 6 · 10 = 420,
/// 3 · 4 · 10 = 120 and 7 · 4 · 10 = 280 mm³. The other bodies are saved, and a reopened design
/// builds the same parts.
#[test]
fn split_body_cuts_with_several_whole_surface_bodies() {
    let near = |v: &[f64], want: &[f64]| v.len() == want.len() && v.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6);
    for (others, want) in [(json!(["AtX8"]), vec![200.0, 300.0, 500.0]), (json!(["AtY6"]), vec![120.0, 180.0, 280.0, 420.0])] {
        let mut s = flat_sheets();
        let p = json!({"body": "Cube", "tool": {"body": "AtX3", "point": [3, 2, 10]}, "tool_sheet": "body", "tool_bodies": others});
        let id = s.execute("solid.split_body", &p).unwrap_or_else(|e| panic!("{p}: {e}"))["feature"].as_u64().unwrap();
        assert!(near(&cube_parts(&s), &want), "{p}: {:?}", cube_parts(&s));
        let saved = s.doc.to_json();
        assert!(saved.contains("\"tool_bodies\""), "{saved}");
        let reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!(reopened.model.result(id).unwrap().error.is_none(), "{p}");
        assert!(near(&cube_parts(&reopened), &want), "{p}: {:?}", cube_parts(&reopened));
    }
}

/// #13303: what Split Body with several surface bodies refuses, each leaving the design as it
/// was: other bodies without `tool_sheet` "body" or with a plane, a body the design lacks, a solid, the tool's
/// body again, one body twice, and a body that cuts no piece.
#[test]
fn split_body_by_several_surface_bodies_refuses_what_cannot_cut() {
    let mut s = flat_sheets();
    s.execute("solid.box", &json!({"corner": [30, 0, 0], "length": 5, "width": 5, "height": 5, "body_name": "Block"})).unwrap();
    let tool = json!({"body": "AtX3", "point": [3, 2, 10]});
    for (p, says) in [
        (json!({"body": "Cube", "tool": tool, "tool_bodies": ["AtX8"]}), "needs `tool_sheet` \"body\""),
        (json!({"body": "Cube", "plane": "XY", "tool_bodies": ["AtX8"]}), "needs a face `tool`"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "body", "tool_bodies": ["Nope"]}), "no body `Nope`"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "body", "tool_bodies": ["Block"]}), "not a surface body"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "body", "tool_bodies": ["AtX3"]}), "a splitting tool twice"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "body", "tool_bodies": ["AtX8", "AtX8"]}), "a splitting tool twice"),
        (json!({"body": "Cube", "tool": tool, "tool_sheet": "body", "tool_bodies": ["Away"]}), "`Away` does not cut `Cube` through"),
    ] {
        let before = s.doc.to_json();
        let e = s.execute("solid.split_body", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(says), "{p}: {e}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
}

/// #13303 review: another surface body from a further placement of a component cuts where that
/// placement puts it, not at the component's first. `Slice`, the plane x = 1, is in `Part`, and a
/// copy of `Part` 6 mm along x puts `Slice (Part:2)` at x = 7. With `AtX3` it cuts the cube into
/// slabs 3, 4 and 3 mm thick: 300, 400 and 300 mm³. The first placement would leave 100, 200
/// and 700.
#[test]
fn split_body_cuts_with_a_surface_body_where_its_placement_puts_it() {
    let near = |v: &[f64], want: &[f64]| v.len() == want.len() && v.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6);
    let mut s = flat_sheets();
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("component.create", json!({"name": "Part", "grounded": false}));
    run("sketch.create", json!({"plane": "XY", "name": "SliceSketch"}));
    let curves = run("sketch.line", json!({"points": [[1, -5], [1, 15]]}))["created"]["curves"].clone();
    run("sketch.finish", json!({}));
    run("surface.extrude", json!({"sketch": "SliceSketch", "curves": curves, "distance": 20, "body_name": "Slice"}));
    run("component.activate", json!({"component": "root"}));
    run("occurrence.copy", json!({"component": "Part", "translate": [6, 0, 0]}));
    let p = json!({"body": "Cube", "tool": {"body": "AtX3", "point": [3, 2, 10]}, "tool_sheet": "body", "tool_bodies": ["Slice (Part:2)"]});
    run("solid.split_body", p.clone());
    assert!(near(&cube_parts(&s), &[300.0, 300.0, 400.0]), "{p}: {:?}", cube_parts(&s));
}
