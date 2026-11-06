use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{
    BoundingSolidParams, BoxParams, CombineParams, CombineResult, ConeParams, CylinderParams, DraftParams, ExtrudeParams, FeatureResult,
    FilletParams, FilletResult, OffsetPlaneParams, PipeParams, PlaneAtAngleParams, ScaleParams, ShellParams, SphereParams, TorusParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BoxParams>(),
        &["body_name", "body_names", "center", "corner", "height", "length", "name", "operation", "participants", "targets", "touching", "width"],
    );
    assert_keys(
        input_schema_for::<ExtrudeParams>(),
        &[
            "body",
            "body_name",
            "body_names",
            "convention",
            "curves",
            "direction",
            "distance",
            "distance2",
            "exact_regions",
            "extent",
            "face",
            "face_body",
            "faces",
            "from",
            "name",
            "operation",
            "participants",
            "profiles",
            "side",
            "sketch",
            "start_offset",
            "taper",
            "targets",
            "thickness",
            "thickness2",
            "thin",
            "through_all",
            "to",
            "to_offset",
            "touching",
        ],
    );
    assert_keys(
        input_schema_for::<CylinderParams>(),
        &["axis", "base", "body_name", "body_names", "diameter", "height", "name", "operation", "participants", "radius", "references", "targets"],
    );
    assert_keys(
        input_schema_for::<SphereParams>(),
        &["body_name", "body_names", "center", "diameter", "name", "operation", "participants", "radius", "targets"],
    );
    assert_keys(
        input_schema_for::<TorusParams>(),
        &["body_name", "body_names", "center", "major", "minor", "name", "operation", "participants", "targets"],
    );
    assert_keys(
        input_schema_for::<ConeParams>(),
        &[
            "axis",
            "base",
            "body_name",
            "body_names",
            "diameter",
            "height",
            "name",
            "operation",
            "participants",
            "radius",
            "targets",
            "top_diameter",
            "top_radius",
        ],
    );
    assert_keys(
        input_schema_for::<PipeParams>(),
        &["body_name", "body_names", "diameter", "name", "operation", "participants", "path", "path_sketch", "targets", "wall"],
    );
    assert_keys(input_schema_for::<BoundingSolidParams>(), &["bodies", "body_name", "body_names", "margin", "name", "participants"]);
    assert_keys(input_schema_for::<ScaleParams>(), &["bodies", "factor", "factors", "name", "origin", "participants"]);
    assert_keys(input_schema_for::<OffsetPlaneParams>(), &["base", "name", "offset", "participants", "through"]);
    assert_keys(input_schema_for::<PlaneAtAngleParams>(), &["angle", "axis", "base", "direction_only", "name", "participants", "pivot"]);
    assert_keys(
        input_schema_for::<FilletParams>(),
        &[
            "body",
            "body_name",
            "body_names",
            "corner",
            "edges",
            "fit",
            "knots",
            "mid",
            "name",
            "participants",
            "radii",
            "radius",
            "radius2",
            "setbacks",
            "smooth",
            "start",
            "tangent_chain",
            "turn",
            "type",
        ],
    );
}

#[test]
fn combine_schema_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<CombineParams>(),
        &[
            "body_name",
            "body_names",
            "component_name",
            "keep_tools",
            "name",
            "new_component",
            "operation",
            "participants",
            "target",
            "tools",
            "touching",
        ],
    );
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "bogus"})),
        ("solid.box", json!({"length": true, "width": 5, "height": 5})),
        ("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [1, 2]})),
        ("solid.extrude", json!({"distance": 5, "operation": "bogus"})),
        ("solid.extrude", json!({"distance": 5, "direction": "sideways"})),
        ("solid.extrude", json!({"distance": 5, "direction": "SIDEWAYS"})),
        ("solid.extrude", json!({"distance": 5, "profiles": [{"point": [1]}]})),
        ("solid.extrude", json!({"distance": 5, "profiles": [["l1"], 7]})),
        ("solid.extrude", json!({"distance": 5, "profiles": []})),
        ("solid.extrude", json!({"distance": 5, "profiles": [true]})),
        ("solid.extrude", json!({"distance": 5, "faces": []})),
        ("solid.extrude", json!({"distance": 5, "faces": null})),
        ("solid.extrude", json!({"distance": 5, "faces": [[1, 2]]})),
        ("solid.extrude", json!({"distance": 5, "profiles": {"offset_boundaries": ["a", "a"]}})),
        ("solid.extrude", json!({"distance": 5, "thin": true, "thickness": 1, "side": "middle"})),
        ("solid.extrude", json!({"distance": 5, "thin": "true"})),
        ("solid.extrude", json!({"distance": 5, "to": [1, 2]})),
        ("solid.extrude", json!({"distance": 5, "from": "x"})),
        ("solid.extrude", json!({"distance": 5, "sketch": true})),
        ("solid.extrude", json!({"distance": 5, "convention": "per_side"})),
        ("solid.extrude", json!({"distance": "  "})),
        ("solid.extrude", json!({"distance": 5, "operation": "x".repeat(1000)})),
        ("solid.cylinder", json!({"radius": 5, "height": 5, "axis": "W"})),
        ("solid.cylinder", json!({"radius": 5, "height": 5, "axis": " w "})),
        ("solid.cylinder", json!({"radius": 5, "height": 5, "axis": [0, 0, 0]})),
        ("solid.cylinder", json!({"radius": 5, "height": 5, "axis": null})),
        ("solid.cylinder", json!({"radius": 5, "height": 5, "base": [1, 2]})),
        ("solid.cylinder", json!({"radius": 5, "height": 5, "operation": "bogus"})),
        ("solid.cylinder", json!({"radius": 5, "height": true})),
        ("solid.sphere", json!({"radius": 5, "center": "x"})),
        ("solid.sphere", json!({"radius": 5, "center": null})),
        ("solid.sphere", json!({"radius": 5, "operation": "bogus"})),
        ("solid.torus", json!({"major": 5, "minor": 1, "center": {"x": 1, "y": 2}})),
        ("solid.torus", json!({"major": true, "minor": 1})),
        ("solid.torus", json!({"major": 5, "minor": 1, "operation": "bogus"})),
        ("solid.cone", json!({"radius": 5, "height": 5, "axis": "W"})),
        ("solid.cone", json!({"radius": 5, "height": 5, "axis": null})),
        ("solid.cone", json!({"radius": 5, "height": 5, "base": [1, 2]})),
        ("solid.cone", json!({"radius": 5, "height": true})),
        ("solid.cone", json!({"radius": 5, "height": 5, "operation": "bogus"})),
        // A `null` top radius alone is the handler's to refuse, with its message.
        ("solid.cone", json!({"radius": 5, "height": 5, "top_radius": null})),
        ("solid.pipe", json!({"path_sketch": true, "path": ["l1"], "diameter": 2})),
        ("solid.pipe", json!({"path_sketch": null, "path": ["l1"], "diameter": 2})),
        ("solid.scale", json!({"bodies": ["Body1"], "factors": [1, 2]})),
        ("solid.scale", json!({"bodies": ["Body1"], "factors": [1, 2, 3, 4]})),
        ("solid.scale", json!({"bodies": ["Body1"]})),
        ("construct.plane.offset", json!({"base": true, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [1, 2]}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [0, 0, 0], "fixed": "yes"}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [0, 0, 0], "body": ""}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [0, 0, 0], "body": "Body1"}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [0, 0, 0], "index": -1}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [0, 0, 0], "path": [0]}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"face": [0, 0, 0], "path": vec![1; 65]}, "offset": 5})),
        ("construct.plane.offset", json!({"base": {"normal": [0, 0, 1], "origin": "here"}, "offset": 5})),
        ("construct.plane.offset", json!({"base": "XY", "through": {"edge": "x"}})),
        ("construct.plane.offset", json!({"base": "XY", "offset": true})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": {"dir": [0, 0, 0]}, "angle": 30})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": {"origin": "o", "dir": [0, 0, 1]}, "angle": 30})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": 5, "angle": 30})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": null, "angle": 30})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": {"face": "x", "point": [0, 0, 0]}, "angle": 30})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": {"face": "x"}, "angle": 30})),
        ("construct.plane.at_angle", json!({"base": "XY", "axis": "X", "angle": true})),
        ("solid.fillet", json!({"edges": [[0, 0, 0]], "radius": 1, "turn": "round"})),
        ("solid.fillet", json!({"edges": [[0, 0, 0]], "radius": 1, "turn": "ROUND"})),
        ("solid.fillet", json!({"edges": [[0, 0, 0]], "radius": 1, "type": "bogus"})),
        ("solid.fillet", json!({"edges": [[0, 0, 0]], "radius": 1, "fit": "bogus"})),
        ("solid.fillet", json!({"edges": [[0, 0, 0]], "radius": 1, "corner": "round"})),
        ("solid.fillet", json!({"edges": [], "radius": 1})),
        ("solid.fillet", json!({"edges": 5, "radius": 1})),
        // #12917: Combine's keys of the wrong shape; bodies the design lacks stay the handler's
        // to refuse.
        ("solid.combine", json!({"target": 5, "tools": ["B"]})),
        ("solid.combine", json!({"target": null, "tools": ["B"]})),
        ("solid.combine", json!({"target": "A", "tools": []})),
        ("solid.combine", json!({"target": "A", "tools": null})),
        ("solid.combine", json!({"target": "A", "tools": 5})),
        ("solid.combine", json!({"target": "A", "tools": ["B", 1]})),
        ("solid.combine", json!({"target": "A", "tools": vec!["B"; 10_001]})),
        ("solid.combine", json!({"target": "A", "tools": ["B"], "operation": "new"})),
        ("solid.combine", json!({"target": "A", "tools": ["B"], "operation": "bogus"})),
        ("solid.combine", json!({"target": "A", "tools": ["B"]})),
    ]);
}

/// #12917: Combine's contract takes what its input schema allows: each call's check finds no
/// issue exactly when the call matches the schema. Left out: an operation spelt in capitals,
/// which the check takes as the handler does and the schema's enum doesn't.
#[test]
fn combine_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<CombineParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"target": "A", "tools": ["B"]}), true),
        (json!({"target": "A", "tools": "B", "operation": "cut", "keep_tools": true}), true),
        (
            json!({"target": "A", "tools": ["B", "C"], "operation": "intersection", "touching": "one_body", "new_component": true, "component_name": "Merged", "name": "C1", "body_name": "X"}),
            true,
        ),
        (json!({"target": "A", "tools": ["B"], "operation": null, "touching": null, "keep_tools": null, "component_name": null}), true),
        (json!({"target": 5, "tools": ["B"]}), false),
        (json!({"target": null, "tools": ["B"]}), false),
        (json!({"target": "A"}), false),
        (json!({"target": "A", "tools": []}), false),
        (json!({"target": "A", "tools": null}), false),
        (json!({"target": "A", "tools": [1]}), false),
        (json!({"target": "A", "tools": ["B"], "operation": "new"}), false),
        (json!({"target": "A", "tools": ["B"], "operation": 5}), false),
        (json!({"target": "A", "tools": ["B"], "touching": "glued"}), false),
        (json!({"target": "A", "tools": ["B"], "keep_tools": "yes"}), false),
        (json!({"target": "A", "tools": ["B"], "targets": ["A"]}), false),
    ] {
        let issues = crate::schema::issues("solid.combine", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// #12917: Combine replies with `CombineResult`, with the keys it gave before, in each of its
/// forms: a join, a cut keeping its tool, an intersect, a join into one body, and a join into a
/// new component (which adds the component's id). In Warn, without warnings, so its contract
/// takes each call.
#[test]
fn combine_replies_with_combine_result() {
    let schema = output_schema_for::<CombineResult>();
    let schema = schema.as_value();
    let before = BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms"]);
    for p in [
        json!({"target": "A", "tools": ["B"]}),
        json!({"target": "A", "tools": "B", "operation": "cut", "keep_tools": true}),
        json!({"target": "A", "tools": ["B"], "operation": "intersect", "name": "Common"}),
        json!({"target": "A", "tools": ["B"], "operation": "union", "touching": "one_body"}),
        json!({"target": "A", "tools": ["B"], "new_component": true, "component_name": "Merged"}),
    ] {
        // Two 10 mm cubes, `B` overlapping `A` by 5 mm along each axis; made again for each call.
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        for (corner, body) in [([0, 0, 0], "A"), ([5, 5, 5], "B")] {
            let box_ = json!({"length": 10, "width": 10, "height": 10, "corner": corner, "body_name": body});
            s.execute("solid.box", &box_).unwrap_or_else(|e| panic!("{e}"));
        }
        let r = s.execute("solid.combine", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        let mut expected = before.clone();
        if p.get("new_component").is_some() {
            assert!(r["component"].is_u64(), "{p}: {r}");
            expected.insert("component");
        }
        assert_eq!(keys, expected, "{p}: {r}");
        assert!(r["changed_bodies"].as_array().is_some_and(|changed| changed.contains(&json!("A"))), "{p}: {r}");
    }
}

#[test]
fn feature_result_serializes_like_the_old_reply() {
    for (warning, ms) in [(None, None), (Some("a Boolean needed a retry".to_string()), Some(12.5)), (None, Some(0.0)), (None, Some(1.0 / 3.0))] {
        let old = json!({"feature": 7, "name": "Box1", "bodies": ["A", "B"], "created_bodies": ["B"], "changed_bodies": [],
            "removed_bodies": ["C"], "warning": warning, "ms": ms});
        let new = FeatureResult {
            feature: 7,
            name: "Box1".into(),
            bodies: vec!["A".into(), "B".into()],
            created_bodies: vec!["B".into()],
            changed_bodies: vec![],
            removed_bodies: vec!["C".into()],
            warning: warning.clone(),
            ms,
            param_warnings: vec![],
            notice: None,
        };
        assert_eq!(serde_json::to_value(&new).ok(), Some(old));
    }
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    let mut s = Session::default();
    sketch_rectangle(&mut s);
    s.param_check = ParamCheck::Warn;
    let replies = [
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})),
        s.execute("solid.extrude", &json!({"distance": 5, "operation": "join"})),
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "colour": "red"})),
    ];
    for r in replies {
        let r = r.unwrap_or_else(|e| panic!("{e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    // The check itself catches a wrong type, a wrong item type and a missing key. (The output
    // schema leaves other keys open, so a new result key doesn't break clients.)
    let ok = json!({"feature": 1, "name": "Box1", "bodies": [], "created_bodies": [], "changed_bodies": [], "removed_bodies": [], "warning": null, "ms": null});
    assert!(conforms(schema, schema, &ok));
    for (k, v) in [("feature", json!("1")), ("bodies", json!([1])), ("warning", json!(5)), ("ms", json!("1"))] {
        let mut bad = ok.clone();
        bad[k] = v;
        assert!(!conforms(schema, schema, &bad), "{bad}");
    }
    let mut bad = ok.clone();
    bad.as_object_mut().map(|o| o.remove("name"));
    assert!(!conforms(schema, schema, &bad), "{bad}");
}

/// `solid.fillet` replies with `FilletResult` in each of its forms, its contract taking each: a
/// boss's base rounded with each turn and its options spelled out, a reduced round (with the radii
/// it used), chord and curvature rounds, a smooth-patch corner with setbacks, and the variable and
/// spline forms.
#[test]
fn fillet_replies_match_its_output_schema() {
    let schema = output_schema_for::<FilletResult>();
    let schema = schema.as_value();
    // A 40 x 30 x 10 plate with a 10 x 10 x 5 boss on it, whose base edges `boss` are.
    let run = |p: Value| {
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "B"})).unwrap_or_else(|e| panic!("{e}"));
        s.execute("solid.box", &json!({"corner": [15, 10, 10], "length": 10, "width": 10, "height": 5, "operation": "join"}))
            .unwrap_or_else(|e| panic!("{e}"));
        let r = s.execute("solid.fillet", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        r
    };
    let boss = json!([[20, 10, 10], [25, 15, 10], [20, 20, 10], [15, 15, 10]]);
    run(
        json!({"edges": boss, "radius": 1, "turn": "mitre", "type": "constant", "fit": "exact", "corner": "rolling", "tangent_chain": true, "body": "B", "name": "Round"}),
    );
    run(json!({"edges": boss, "radius": "1 mm", "turn": "Swept"}));
    run(json!({"edges": boss, "radius": 1, "turn": "swept", "fit": "reduce"}));
    let reduced = run(json!({"edges": [[20, 0, 10]], "radius": 15, "fit": "reduce"}));
    assert!(reduced.get("radius_applied").is_some_and(Value::is_number) && reduced.get("radii_applied").is_some_and(Value::is_array), "{reduced}");
    run(json!({"edges": [[20, 0, 10]], "radius": 1, "type": "chord"}));
    run(json!({"edges": [{"body": "B", "index": 0}], "radius": 0.5, "tangent_chain": false}));
    run(json!({"edges": [[20, 0, 10]], "radius": 1, "type": "curvature"}));
    // Variable and spline radii: solidcore fails a variable round on a box edge at this pin (status
    // 1001, as `variable_radius_on_a_box_edge`), so their forms are checked against the contract.
    for p in [
        json!({"edges": [[20, 0, 10]], "radius": 1, "type": "variable", "radius2": 2, "start": [0, 0, 10], "mid": [[0.5, "1.5 mm"]], "smooth": true}),
        json!({"edges": [[20, 0, 10]], "radius": 1, "type": "spline", "start": [0, 0, 10], "radii": [2], "knots": [0, 0, 1, 1]}),
    ] {
        assert!(crate::schema::Contract::check(&crate::schema::TypedContract::<FilletParams>::new(), &p).is_empty(), "{p}");
    }
    run(json!({"edges": [[20, 0, 10], [40, 15, 10], [40, 0, 5]], "radius": 2, "corner": "g2", "setbacks": [{"vertex": [40, 0, 10], "distance": 4}]}));
}

/// The feature commands converted after box and extrude answer with `FeatureResult` too, and
/// their contracts take every form these calls use: in Warn, a contract that refused one would
/// still run the command, so the replies are checked for warnings too.
#[test]
fn converted_primitives_and_body_commands_reply_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    let mut replies = vec![
        run(
            "solid.cylinder",
            json!({"radius": 2, "diameter": 9, "height": 5, "base": [20, "0 mm", 0], "axis": " x ", "name": "Cyl", "body_name": "Cyl"}),
        ),
        run("solid.cylinder", json!({"diameter": 2, "height": 3, "base": [0, -20, 0], "axis": [0, 1, 0], "operation": "new body"})),
        run(
            "solid.cylinder",
            json!({"radius": 1, "height": 3, "base": {"x": 0, "y": -30, "z": 0}, "axis": {"x": 0, "y": 0, "z": 1}, "body_names": ["Peg"]}),
        ),
        run("solid.sphere", json!({"diameter": 4, "center": {"x": 0, "y": 20, "z": 0}})),
        run("solid.sphere", json!({"radius": 1, "center": [22, 0, 2], "operation": "union", "targets": ["Peg"], "participants": []})),
        run("solid.sphere", json!({"radius": 1, "center": [23, 0, -2], "operation": "join", "targets": ["Peg"], "participants": ["Cyl"]})),
        run("solid.torus", json!({"major": 5, "minor": 1, "center": [0, 0, "20 mm"], "participants": ["Peg"]})),
        run("solid.bounding_solid", json!({"bodies": ["Cyl"], "margin": 1})),
        run("solid.bounding_solid", json!({})),
        run("solid.bounding_solid", json!({"bodies": "Peg", "body_name": "PegBox"})),
        run("solid.scale", json!({"bodies": ["Cyl"], "factors": [1, 2, "1.5"], "origin": [20, 0, 0]})),
        run("solid.scale", json!({"bodies": "Cyl", "factors": [1, 1, 2], "factor": 3})),
        run(
            "solid.scale",
            json!({"bodies": "Peg", "factor": "3 / 2", "origin": {"x": 0, "y": -30, "z": 0}, "name": "Grow", "participants": ["Cyl"]}),
        ),
    ];
    let sketch = run("sketch.create", json!({"plane": "XZ", "name": "Path"}));
    let line = run("sketch.line", json!({"points": [[40, 0], [40, 30]]}));
    let across = run("sketch.line", json!({"points": [[22, -8], [22, 8]]}));
    run("sketch.finish", json!({}));
    let id = sketch["sketch"].clone();
    assert!(id.is_u64(), "{sketch}");
    replies.push(run("solid.pipe", json!({"path_sketch": "Path", "path": line["curves"].clone(), "diameter": 4, "wall": 1, "body_name": "Tube"})));
    replies.push(run(
        "solid.pipe",
        json!({"path_sketch": id, "path": across["curves"][0].clone(), "diameter": 1, "operation": "join", "targets": ["Cyl"]}),
    ));
    replies.push(run("solid.cone", json!({"radius": 3, "height": 6, "base": [60, 0, 0]})));
    replies.push(run(
        "solid.cone",
        json!({"diameter": 6, "top_diameter": 2, "height": "-4 mm", "base": {"x": 70, "y": 0, "z": 0}, "axis": " x ", "name": "Frustum", "body_name": "Frustum"}),
    ));
    replies.push(run(
        "solid.cone",
        json!({"radius": 2, "diameter": 9, "top_radius": 1, "top_diameter": 7, "height": 5, "base": [80, "0 mm", 0], "axis": [0, 1, 0]}),
    ));
    replies.push(run(
        "solid.cone",
        json!({"radius": 2, "top_radius": null, "top_diameter": 2, "height": 3, "base": [90, 0, 0], "axis": {"x": 0, "y": 0, "z": 1}, "operation": "new body", "body_names": ["Can"]}),
    ));
    // The cone meets only `Frustum` but joins its target, `Can`, as the sphere above does.
    let cone_union =
        run("solid.cone", json!({"radius": 1, "height": 2, "base": [69, 0, 0], "operation": "union", "targets": ["Can"], "participants": []}));
    assert_eq!(cone_union["changed_bodies"], json!(["Can"]), "{cone_union}");
    replies.push(cone_union);
    // Warn reports what the contract refuses and runs the command anyway.
    let typo = run("solid.scale", json!({"bodies": "Cyl", "factor": 2, "colour": "red"}));
    assert_eq!(typo["param_warnings"], json!(["unknown key `colour`"]));
    // A scale makes no body, so its contract doesn't take the new body's name.
    let no_body = run("solid.scale", json!({"bodies": "Cyl", "factor": 1, "body_name": "Big"}));
    let warning = no_body["param_warnings"][0].as_str().unwrap_or_default();
    assert!(warning.starts_with("unknown key `body_name`"), "{no_body}");
    for r in replies.iter().chain([&typo, &no_body]) {
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    }
    for r in &replies {
        assert!(r.get("param_warnings").is_none(), "{r}");
    }
    // A primitive's `targets` count when `participants` lists no bodies (the sphere meets only
    // `Cyl` but joins `Peg`) and not when it does; a scale's `participants` don't change which
    // bodies it scales.
    let changed = [4, 5, 12].map(|i| replies[i]["changed_bodies"].clone());
    assert_eq!(changed, [json!(["Peg"]), json!(["Cyl"]), json!(["Peg"])]);
    // With `factors` given, `factor` has no effect but must still evaluate.
    assert!(s.execute("solid.scale", &json!({"bodies": "Cyl", "factors": [1, 1, 2], "factor": "no_such_parameter"})).is_err());
    // Without `targets`, a primitive works on every body in the component: an intersect removes
    // the bodies it doesn't meet.
    let r = s.execute("solid.sphere", &json!({"radius": 1, "center": [22, 0, 0], "operation": "intersect"})).unwrap_or_else(|e| panic!("{e}"));
    let (changed, removed) = (r["changed_bodies"].as_array().cloned(), r["removed_bodies"].as_array().cloned());
    assert!(changed.is_some_and(|c| c.contains(&json!("Bounding"))) && removed.is_some_and(|r| r.contains(&json!("Peg"))), "{r}");
}

// ---- Shell and Draft (#12752) ----

/// A 20 mm cube, `Box`: its top face is at z = 20, a side face at x = 0.
fn cube(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "body_name": "Box"})).unwrap_or_else(|e| panic!("{e}"));
    s
}

fn shell() -> Value {
    json!({"faces": [[10, 10, 20]], "thickness": 2})
}

fn draft() -> Value {
    json!({"faces": [[0, 10, 10]], "angle": 3, "neutral": "XY"})
}

/// `base` with `key` set to `value`.
fn with(base: &Value, key: &str, value: Value) -> Value {
    let mut p = base.clone();
    p[key] = value;
    p
}

#[test]
fn the_shell_and_draft_schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<ShellParams>(),
        &["body", "body_name", "body_names", "direction", "faces", "name", "participants", "tangent_chain", "thickness"],
    );
    assert_keys(input_schema_for::<DraftParams>(), &["angle", "body", "body_name", "body_names", "faces", "name", "neutral", "participants", "pull"]);
}

/// #12752: calls to Shell and Draft that fail today get the same message in strict. The faces
/// fail on the shared helper's sketch, which has no body; the rest need the cube's faces.
#[test]
fn strict_keeps_the_shell_and_draft_errors() {
    assert_strict_keeps_todays_errors(&[
        ("solid.shell", json!({"thickness": 1})),
        ("solid.shell", json!({"faces": null, "thickness": 1})),
        ("solid.shell", json!({"faces": 5, "thickness": 1})),
        ("solid.shell", json!({"faces": [true], "thickness": 1})),
        ("solid.shell", json!({"faces": vec![[0, 0, 0]; 1001], "thickness": 1})),
        ("solid.draft", json!({"angle": 3, "neutral": "XY"})),
        ("solid.draft", json!({"faces": [], "angle": 3, "neutral": "XY"})),
        ("solid.draft", json!({"faces": [[1, 2]], "angle": 3, "neutral": "XY"})),
    ]);
    for (id, p) in [
        ("solid.shell", with(&shell(), "thickness", json!(null))),
        ("solid.shell", with(&shell(), "thickness", json!("2 deg"))),
        ("solid.shell", with(&shell(), "direction", json!("sideways"))),
        ("solid.draft", with(&draft(), "angle", json!("2 mm"))),
        ("solid.draft", json!({"faces": [[0, 10, 10]], "angle": 3})),
        ("solid.draft", with(&draft(), "neutral", json!(null))),
        ("solid.draft", with(&draft(), "neutral", json!(1.5))),
        ("solid.draft", with(&draft(), "neutral", json!("Nope"))),
        ("solid.draft", with(&draft(), "neutral", json!({"origin": [0, 0, 0]}))),
    ] {
        let off = cube(ParamCheck::Off).execute(id, &p).err().map(|e| e.to_string());
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(cube(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()), off, "{id} {p}");
    }
}

/// #12752: keys of the wrong type that Shell and Draft ignore today (a `tangent_chain` that isn't
/// true or false is read as false, a `pull` that isn't a point as the plane's normal) are refused
/// by the contracts: in Warn the call still works, with a warning naming the key.
#[test]
fn the_shell_and_draft_contracts_refuse_what_the_handlers_ignore() {
    for (id, p, key) in [
        ("solid.shell", with(&shell(), "tangent_chain", json!("yes")), "tangent_chain"),
        ("solid.shell", with(&shell(), "direction", json!(5)), "direction"),
        ("solid.shell", with(&shell(), "body", json!(5)), "body"),
        ("solid.draft", with(&draft(), "pull", json!("up")), "pull"),
        ("solid.draft", with(&draft(), "body", json!(["Box"])), "body"),
    ] {
        assert!(cube(ParamCheck::Off).execute(id, &p).is_ok(), "{id} {p} works today");
        let warned = cube(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(warned["param_warnings"].to_string().contains(key), "{warned}");
        let error = cube(ParamCheck::Strict).execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains(&format!("`{key}`")), "{id} {p}: {error}");
    }
}

/// #12752: the Shell and Draft schemas take what the handlers take. Each call is checked against
/// both the schema and the handler on the cube, with the check off, so the two can't drift apart.
#[test]
fn the_shell_and_draft_schemas_take_what_the_handlers_take() {
    let shell_schema = input_schema_for::<ShellParams>();
    let draft_schema = input_schema_for::<DraftParams>();
    for (id, schema, p) in [
        ("solid.shell", &shell_schema, shell()),
        ("solid.shell", &shell_schema, json!({"faces": [], "thickness": 2})),
        ("solid.shell", &shell_schema, with(&with(&shell(), "direction", json!("outside")), "tangent_chain", json!(true))),
        ("solid.shell", &shell_schema, with(&shell(), "direction", json!(""))),
        ("solid.shell", &shell_schema, with(&shell(), "direction", json!("sideways"))),
        ("solid.shell", &shell_schema, json!({"faces": [[10, 10]], "thickness": 2})),
        ("solid.shell", &shell_schema, json!({"thickness": 2})),
        ("solid.draft", &draft_schema, draft()),
        ("solid.draft", &draft_schema, with(&with(&draft(), "neutral", json!({"origin": [0, 0, 0], "normal": [0, 0, 1]})), "pull", json!([0, 0, 1]))),
        ("solid.draft", &draft_schema, with(&draft(), "angle", json!("3 deg"))),
        ("solid.draft", &draft_schema, with(&draft(), "neutral", json!(1.5))),
        ("solid.draft", &draft_schema, json!({"faces": [[0, 10, 10]], "angle": 3})),
    ] {
        let takes = cube(ParamCheck::Off).execute(id, &p).is_ok();
        assert_eq!(conforms(schema.as_value(), schema.as_value(), &p), takes, "{id} {p}");
    }
}

/// #12752: Shell and Draft reply as feature commands do, in Warn without warnings: the cube
/// opened at the top, closed hollow, and drafted. A misspelt key is ignored today: Warn says so,
/// and strict refuses it.
#[test]
fn shell_and_draft_reply_with_their_schemas() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    for (id, p) in [("solid.shell", shell()), ("solid.shell", json!({"faces": [], "thickness": 2})), ("solid.draft", draft())] {
        let r = cube(ParamCheck::Warn).execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert_eq!(r["changed_bodies"], json!(["Box"]), "{id}: {r}");
    }
    let p = with(&shell(), "tangent_chian", json!(true));
    let r = cube(ParamCheck::Warn).execute("solid.shell", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    assert!(r["param_warnings"].to_string().contains("tangent_chian"), "{r}");
    let error = cube(ParamCheck::Strict).execute("solid.shell", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`tangent_chian`"), "{error}");
}
