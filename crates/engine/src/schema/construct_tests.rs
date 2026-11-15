use serde_json::{Value, json};

use super::{
    AxisEdgeParams, AxisNormalToFaceParams, AxisThroughCylinderParams, AxisTwoPlanesParams, AxisTwoPointsParams, MidplaneParams,
    PlaneAlongPathParams, PlaneAxisPointParams, PlanePerpendicularParams, PlaneTangentParams, PlaneThreePointsParams, PlaneTwoEdgesParams,
    PointAlongPathParams, PointCenterParams, PointEdgePlaneParams, PointThreePlanesParams, PointTwoEdgesParams, PointVertexParams,
};
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<AxisThroughCylinderParams>(), &["face", "name", "participants"]);
    assert_keys(input_schema_for::<AxisNormalToFaceParams>(), &["face", "name", "participants", "point"]);
    assert_keys(input_schema_for::<AxisTwoPlanesParams>(), &["a", "b", "name", "participants"]);
    assert_keys(input_schema_for::<AxisTwoPointsParams>(), &["a", "b", "name", "participants"]);
    assert_keys(input_schema_for::<AxisEdgeParams>(), &["edge", "name", "participants"]);
    assert_keys(input_schema_for::<PointVertexParams>(), &["name", "participants", "point"]);
    assert_keys(input_schema_for::<PointTwoEdgesParams>(), &["a", "b", "name", "participants"]);
    assert_keys(input_schema_for::<PointThreePlanesParams>(), &["a", "b", "c", "name", "participants"]);
    assert_keys(input_schema_for::<PointCenterParams>(), &["name", "of", "participants"]);
    assert_keys(input_schema_for::<PointEdgePlaneParams>(), &["edge", "name", "participants", "plane"]);
    assert_keys(input_schema_for::<PointAlongPathParams>(), &["name", "participants", "path", "t"]);
    assert_keys(input_schema_for::<MidplaneParams>(), &["a", "b", "name", "participants"]);
    assert_keys(input_schema_for::<PlaneAxisPointParams>(), &["axis", "name", "participants", "point"]);
    assert_keys(input_schema_for::<PlaneTwoEdgesParams>(), &["a", "b", "name", "participants"]);
    assert_keys(input_schema_for::<PlaneThreePointsParams>(), &["a", "b", "c", "name", "participants"]);
    assert_keys(input_schema_for::<PlaneTangentParams>(), &["angle", "at", "face", "name", "participants", "reference"]);
    assert_keys(input_schema_for::<PlaneAlongPathParams>(), &["name", "participants", "path", "t"]);
    assert_keys(input_schema_for::<PlanePerpendicularParams>(), &["line", "name", "participants", "plane"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("construct.axis.edge", json!({"edge": 5})),
        ("construct.axis.edge", json!({"edge": {"edge": "x"}})),
        ("construct.axis.edge", json!({"edge": null})),
        ("construct.axis.through_cylinder", json!({"face": {"type": "bogus"}})),
        ("construct.axis.through_cylinder", json!({"face": {"type": "face", "at": [1, 2]}})),
        ("construct.axis.two_points", json!({"a": {"sketch": true, "point": "p1"}, "b": {"point": [0, 0, 0]}})),
        ("construct.axis.two_points", json!({"a": {"sketch": 1, "point": [1, 2, 3]}, "b": {"point": [0, 0, 0]}})),
        ("construct.point.vertex", json!({"point": {"sketch": 1}})),
        ("construct.point.vertex", json!({"point": {"type": "point", "p": false}})),
        ("construct.point.vertex", json!({"point": {"type": "point"}})),
        ("construct.point.vertex", json!({"point": {"construct_point": 5}})),
        ("construct.plane.midplane", json!({"a": {"plane": 2.5}, "b": "XY"})),
        ("construct.plane.midplane", json!({"a": {"plane": -1}, "b": "XY"})),
        ("construct.plane.midplane", json!({"a": {"plane": null}, "b": "XY"})),
        ("construct.plane.midplane", json!({"a": {"plane": {"normal": [0, 0, 0]}}, "b": "XY"})),
        ("construct.plane.midplane", json!({"a": {"plane": {"x_dir": [1, 0, 0], "y_dir": [2, 0, 0], "normal": [0, 0, 1]}}, "b": "XY"})),
        ("construct.plane.midplane", json!({"a": {"plane": {"origin": [1, 2], "normal": [0, 0, 1]}}, "b": "XY"})),
        ("construct.plane.midplane", json!({"a": {"plane": {"origin": null, "normal": [0, 0, 1]}}, "b": "XY"})),
    ]);
}

/// Every construction command answers with `FeatureResult`, and its contract takes every form
/// these calls use (in Warn, a contract that refused one would still run the command, so the
/// replies are checked for warnings too).
#[test]
fn construction_features_reply_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    // A 10 mm cube at the origin, a cylinder beside it and a sketch with a line.
    run("solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run("solid.cylinder", json!({"radius": 5, "height": 10, "base": [30, 0, 0]}));
    let sketch = run("sketch.create", json!({"plane": "XY", "name": "Guide"}));
    let line = run("sketch.line", json!({"points": [[0, -20], [20, -20]]}));
    run("sketch.finish", json!({}));
    let (sketch, curve, point) = (sketch["sketch"].clone(), line["curves"][0].clone(), line["created"]["points"][0].clone());
    assert!(sketch.is_u64() && curve.is_string() && point.is_string(), "{sketch} {line}");
    let replies = vec![
        run("construct.point.vertex", json!({"point": {"vertex": [10, 10, 10]}, "name": "Corner"})),
        run("construct.point.vertex", json!({"point": {"sketch": "Guide", "point": point}})),
        run("construct.point.vertex", json!({"point": {"edge": [35, 0, 10]}})),
        run("construct.point.two_edges", json!({"a": {"axis": "X"}, "b": {"axis": "y"}})),
        run("construct.point.three_planes", json!({"a": "XY", "b": {"plane": "XZ"}, "c": {"face": [10, 5, 5]}})),
        run("construct.point.center", json!({"of": {"edge": [35, 0, 10]}})),
        run("construct.point.edge_plane", json!({"edge": {"axis": "Z"}, "plane": {"plane": {"origin": [0, 0, "3 mm"], "normal": [0, 0, 1]}}})),
        run("construct.point.along_path", json!({"path": {"sketch": sketch, "curve": curve}, "t": "1 / 4"})),
        run("construct.point.along_path", json!({"path": {"edge": [5, 0, 0]}})),
        run("construct.axis.through_cylinder", json!({"face": {"face": [35, 0, 5]}, "name": "Bore"})),
        run("construct.axis.normal_to_face", json!({"face": {"face": [5, 5, 10]}, "point": {"construct_point": "Corner"}})),
        run("construct.axis.two_planes", json!({"a": "XY", "b": {"plane": {"x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}}})),
        run("construct.axis.two_points", json!({"a": {"type": "point", "p": [0, 0, 0]}, "b": {"point": {"x": 1, "y": 2, "z": 3}}})),
        run("construct.axis.edge", json!({"edge": {"edge": [5, 0, 0]}, "participants": []})),
        run("construct.plane.midplane", json!({"a": "XY", "b": {"face": [5, 5, 10]}, "name": "Middle"})),
        run("construct.plane.axis_point", json!({"axis":{"axis":"Z"},"point":{"point":[10,20,30]}})),
        run("construct.plane.two_edges", json!({"a": {"edge": [5, 0, 0]}, "b": {"edge": [0, 5, 0]}})),
        run("construct.plane.three_points", json!({"a": {"point": [0, 0, 0]}, "b": {"vertex": [10, 0, 0]}, "c": {"sketch": sketch, "point": point}})),
        run("construct.plane.tangent", json!({"face": {"face": [35, 0, 5]}})),
        run("construct.plane.tangent", json!({"face": {"face": [30, 5, 5]}, "at": [30, 6, 5], "participants": ["Body1"]})),
        run("construct.plane.along_path", json!({"path": {"edge": [5, 0, 0]}})),
        run("construct.plane.perpendicular", json!({"plane": "XY", "line": {"edge": [5, 0, 0]}})),
        // The offset and at-angle planes' contracts are in schema/features.rs; they run here
        // because they use this design's `Top`, its faces and the `Bore` axis.
        run("construct.plane.offset", json!({"base": "Top", "offset": 5, "name": "Shelf"})),
        run("construct.plane.offset", json!({"base": "Shelf", "offset": "-2 mm"})),
        run("construct.plane.offset", json!({"base": {"face": [5, 5, 10]}, "offset": 2})),
        run("construct.plane.offset", json!({"base": "XZ", "through": {"vertex": [10, 10, 10]}, "offset": null})),
        run("construct.plane.offset", json!({"base": {"origin": ["1 mm", 0, 0], "normal": [1, 0, 0]}, "offset": 1, "through": null})),
        run("construct.plane.at_angle", json!({"base": "XY", "axis": "x", "angle": 30})),
        run("construct.plane.at_angle", json!({"base": "XZ", "axis": {"edge": [5, 0, 0]}, "angle": "45 deg"})),
        run("construct.plane.at_angle", json!({"base": {"face": [5, 5, 10]}, "axis": {"dir": [0, 1, 0]}, "angle": 10})),
        run("construct.plane.at_angle", json!({"base": "XY", "axis": "Bore", "angle": 90})),
    ];
    // Warn reports what the contract refuses and runs the command anyway: a construction feature
    // makes no body, and a construction plane may not take an origin plane's name.
    let no_body = run("construct.axis.edge", json!({"edge": {"axis": "X"}, "body_name": "Rod"}));
    let warning = no_body["param_warnings"][0].as_str().unwrap_or_default();
    assert!(warning.starts_with("unknown key `body_name`"), "{no_body}");
    for r in replies.iter().chain([&no_body]) {
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
    }
    for r in &replies {
        assert!(r.get("param_warnings").is_none(), "{r}");
        assert_eq!((&r["created_bodies"], &r["changed_bodies"], &r["removed_bodies"]), (&json!([]), &json!([]), &json!([])), "{r}");
    }
    // A construction plane's name that an origin plane has is refused with the same message,
    // but strict names the command where the handler (`add_feature`, which doesn't know it) says
    // `construct.plane`.
    let refused = |level: ParamCheck| {
        let mut s = Session { param_check: level, ..Session::default() };
        s.execute("construct.plane.midplane", &json!({"a": "XY", "b": {"plane": {"origin": [0, 0, 10], "normal": [0, 0, 1]}}, "name": " front "}))
            .err()
            .map(|e| e.to_string())
    };
    let message = "`front` is reserved for an origin plane (XY, XZ, YZ, Top, Front, Right); give the construction plane another name";
    assert_eq!(refused(ParamCheck::Off), Some(crate::params::bad("construct.plane", message).to_string()));
    assert_eq!(refused(ParamCheck::Strict), Some(crate::params::bad("construct.plane.midplane", message).to_string()));
}
