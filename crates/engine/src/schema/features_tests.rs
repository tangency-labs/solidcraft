use serde_json::{Value, json};

use super::{
    BoundingSolidParams, BoxParams, ConeParams, CylinderParams, ExtrudeParams, FeatureResult, PipeParams, ScaleParams, SphereParams, TorusParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BoxParams>(),
        &["body_name", "body_names", "center", "corner", "height", "length", "name", "operation", "participants", "targets", "width"],
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
    ]);
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
