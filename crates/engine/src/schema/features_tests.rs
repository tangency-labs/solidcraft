use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{
    BoundingSolidParams, BoxParams, ChamferParams, CombineParams, CombineResult, ConeParams, CylinderParams, ExtrudeParams, FeatureResult,
    FilletParams, FilletResult, MoveParams, MoveResult, OffsetPlaneParams, PipeParams, PlaneAtAngleParams, PressPullParams, PressPullResult,
    RevolveParams, ScaleParams, SphereParams, TorusParams,
};
use super::{PatternCircularParams, PatternCircularResult};
use super::{SurfaceSweepParams, SweepParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<MoveParams>(),
        &[
            "angle",
            "axis",
            "bodies",
            "body_name",
            "body_names",
            "copy",
            "matrix",
            "name",
            "occurrences",
            "origin",
            "participants",
            "tolerance",
            "translate",
        ],
    );
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
            "fixed",
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
        input_schema_for::<RevolveParams>(),
        &[
            "angle",
            "angle2",
            "axis",
            "body",
            "body_name",
            "body_names",
            "convention",
            "direction",
            "exact_regions",
            "face",
            "face_body",
            "fixed",
            "name",
            "operation",
            "participants",
            "profiles",
            "sketch",
            "symmetric",
            "targets",
            "to",
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
    assert_keys(
        input_schema_for::<ChamferParams>(),
        &["angle", "body", "body_name", "body_names", "distance", "distance2", "edges", "flip", "name", "participants", "tangent_chain"],
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
        ("solid.revolve", json!({"axis": "y", "operation": "bogus"})),
        ("solid.revolve", json!({"axis": "y", "operation": "auto"})),
        ("solid.revolve", json!({"axis": "y", "angle": "no_such_parameter"})),
        ("solid.revolve", json!({"axis": "y", "angle2": "no_such_parameter"})),
        ("solid.revolve", json!({"axis": "y", "to": [1, 2]})),
        ("solid.revolve", json!({"axis": "y", "to": null})),
        ("solid.revolve", json!({"axis": "y", "convention": "per_side"})),
        ("solid.revolve", json!({"axis": "y", "convention": "SKETCH_NORMAL"})),
        ("solid.revolve", json!({"axis": "y", "convention": null})),
        ("solid.revolve", json!({"axis": "y", "sketch": true})),
        ("solid.revolve", json!({"axis": "y", "sketch": null})),
        ("solid.revolve", json!({"axis": "y", "sketch": "Missing"})),
        ("solid.revolve", json!({"axis": "y", "profiles": []})),
        ("solid.revolve", json!({"axis": "y", "profiles": [{"point": [1]}]})),
        ("solid.revolve", json!({"axis": "y", "profiles": [true]})),
        ("solid.revolve", json!({"axis": "y", "participants": ["Missing"]})),
        ("solid.revolve", json!({"axis": null})),
        ("solid.revolve", json!({"axis": 5})),
        ("solid.revolve", json!({"axis": [0, 1, 0]})),
        ("solid.revolve", json!({"axis": true})),
        ("solid.revolve", json!({"axis": {}})),
        ("solid.revolve", json!({"axis": {"origin": [0, 0, 0]}})),
        ("solid.revolve", json!({"axis": {"dir": [0, 1]}})),
        ("solid.revolve", json!({"axis": {"dir": "y"}})),
        ("solid.revolve", json!({"axis": {"origin": [0, 0], "dir": [0, 1, 0]}})),
        ("solid.revolve", json!({"axis": {"origin": "o", "dir": [0, 1, 0]}})),
        ("solid.revolve", json!({"axis": {"origin": null, "dir": [0, 1, 0]}})),
        ("solid.revolve", json!({"axis": {"origin": ["no_such_parameter", 0, 0], "dir": [0, 1, 0]}})),
        ("solid.revolve", json!({"axis": {"type": "world"}})),
        ("solid.revolve", json!({"axis": {"type": "world", "axis": "y"}})),
        ("solid.revolve", json!({"axis": {"type": "world", "axis": 5}})),
        ("solid.revolve", json!({"axis": {"edge": "x"}})),
        ("solid.revolve", json!({"axis": {"face": [1, 2]}})),
        ("solid.revolve", json!({"axis": {"type": "edge"}})),
        ("solid.revolve", json!({"axis": {"edge": [1, 2, 3]}})),
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
        // #12884: Chamfer's edges and distances; giving both `distance2` and `angle`, and a
        // distance that doesn't evaluate as a length, stay the handler's to refuse.
        ("solid.chamfer", json!({"edges": [], "distance": 1})),
        ("solid.chamfer", json!({"edges": 5, "distance": 1})),
        ("solid.chamfer", json!({"edges": [[1, 2]], "distance": 1})),
        ("solid.chamfer", json!({"edges": [[0, 0, 0]], "distance": true})),
        ("solid.chamfer", json!({"edges": [[0, 0, 0]], "distance": "  "})),
        ("solid.chamfer", json!({"edges": [[0, 0, 0]], "distance": 1e9})),
        ("solid.chamfer", json!({"edges": [[0, 0, 0]], "distance": "no_such_parameter"})),
        ("solid.chamfer", json!({"edges": [[0, 0, 0]], "distance": 1, "angle": "5 mm"})),
        ("solid.chamfer", json!({"edges": [[0, 0, 0]], "distance": 1, "distance2": 1, "angle": 30})),
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
fn sweep_schemas_and_the_key_check_agree() {
    let shape = ["orientation", "rail_fit", "rails", "scale", "twist"];
    let mut solid = vec![
        "body_name",
        "body_names",
        "exact_regions",
        "name",
        "operation",
        "participants",
        "path",
        "path_body",
        "path_edges",
        "path_sketch",
        "profiles",
        "sketch",
        "targets",
        "touching",
    ];
    solid.extend(shape);
    solid.sort_unstable();
    assert_keys(input_schema_for::<SweepParams>(), &solid);
    let mut surface = vec![
        "body_name",
        "body_names",
        "curves",
        "exact_regions",
        "name",
        "participants",
        "path",
        "path_body",
        "path_edges",
        "path_sketch",
        "profiles",
        "sketch",
    ];
    surface.extend(shape);
    surface.sort_unstable();
    assert_keys(input_schema_for::<SurfaceSweepParams>(), &surface);
}

/// #2216: a sweep's path may be a chain of a body's edges (`path_body` and `path_edges`), so
/// the contracts take those keys: a call with them is not refused, and a key that is not one
/// of the command's still is.
#[test]
fn sweep_path_on_a_body_is_accepted_and_unknown_keys_are_not() {
    let p = json!({"path_body": "Body1", "path_edges": [[0, 0, 0], [10, 0, 0]]});
    let solid = crate::schema::TypedContract::<SweepParams>::new();
    let surface = crate::schema::TypedContract::<SurfaceSweepParams>::new();
    assert!(crate::schema::Contract::check(&solid, &p).is_empty(), "{p}");
    assert!(crate::schema::Contract::check(&surface, &p).is_empty(), "{p}");
    // A key that is not one of the command's is still refused.
    let typo = json!({"path_body": "Body1", "path_edges": [[0, 0, 0]], "path_boddy": "Body1"});
    for issues in [crate::schema::Contract::check(&solid, &typo), crate::schema::Contract::check(&surface, &typo)] {
        assert_eq!(issues.len(), 1, "{typo}");
        assert_eq!(issues[0].path, "path_boddy", "{typo}");
        assert_eq!(issues[0].message, "unknown key `path_boddy`; did you mean `path_body`?", "{typo}");
    }
    // `path_body` is a body name, or `null` (which the handler reads as absent).
    for p in [json!({"path_body": null}), json!({"path_body": "Body1", "path_edges": [[0, 0, 0]]})] {
        assert!(crate::schema::Contract::check(&solid, &p).is_empty(), "{p}");
        assert!(crate::schema::Contract::check(&surface, &p).is_empty(), "{p}");
    }
    for p in [json!({"path_body": 5}), json!({"path_body": true}), json!({"path_body": []})] {
        for issues in [crate::schema::Contract::check(&solid, &p), crate::schema::Contract::check(&surface, &p)] {
            assert_eq!(issues.len(), 1, "{p}");
            assert_eq!(issues[0].path, "path_body", "{p}");
            assert_eq!(issues[0].message, "`path_body` must be a body name", "{p}");
        }
    }
    // `path_edges` is a list of 1 to 1000 points, as the handler's `path_edge_points` reads them.
    for (p, message) in [
        (json!({"path_edges": 5}), "`path_edges` must be a list of points"),
        (json!({"path_edges": []}), "the path needs 1…1000 edges"),
        (json!({"path_edges": [[0, 0, 0], [1]]}), "each `path_edges` entry must be a point [x,y,z]"),
    ] {
        for issues in [crate::schema::Contract::check(&solid, &p), crate::schema::Contract::check(&surface, &p)] {
            assert_eq!(issues.len(), 1, "{p}");
            assert_eq!(issues[0].path, "path_edges", "{p}");
            assert_eq!(issues[0].message, message, "{p}");
        }
    }
    // The schemas the commands publish list both keys.
    for schema in [input_schema_for::<SweepParams>(), input_schema_for::<SurfaceSweepParams>()] {
        let schema = schema.as_value();
        assert_eq!(schema["properties"]["path_body"]["type"], json!(["string", "null"]), "{schema}");
        // `path_edges` is published as a `$ref` to its own definition, as `path` is.
        let name = schema["properties"]["path_edges"]["$ref"].as_str().unwrap_or_default().strip_prefix("#/$defs/").unwrap_or_default().to_string();
        let edges = &schema["$defs"][name.clone()];
        assert_eq!(edges["type"], "array", "{schema}");
        assert_eq!(edges["minItems"], 1, "{schema}");
        assert_eq!(edges["maxItems"], 1000, "{schema}");
    }
}

/// The calls both sweeps refuse today, as `sweep` and `surface_sweep` read them, each run on
/// `sketch_rectangle`'s sketch (`Sketch1`, which is the path sketch too: the path's curves are
/// only looked up when the feature is evaluated).
#[test]
fn sweep_strict_keeps_todays_errors() {
    let rail = json!([{"curves": ["l1"]}]);
    let mut cases = Vec::new();
    for extra in [
        json!({"orientation": "sideways"}),
        json!({"orientation": " parallel"}),
        json!({"orientation": 5}),
        json!({"orientation": [0, 0, 0]}),
        json!({"orientation": [1, 0]}),
        json!({"orientation": {"direction": [0, 0, 0]}}),
        json!({"orientation": {"direction": "x"}}),
        json!({"orientation": {"x": 1, "y": 0}}),
        json!({"twist": true}),
        json!({"twist": "  "}),
        json!({"twist": [10]}),
        json!({"twist": 1e9}),
        json!({"twist": "no_such_parameter"}),
        json!({"twist": "1441 deg"}),
        json!({"scale": true}),
        json!({"scale": 1e9}),
        json!({"scale": "x".repeat(4097)}),
        json!({"scale": 0}),
        json!({"scale": "no_such_parameter"}),
        json!({"rail_fit": "wobble"}),
        json!({"rail_fit": 1}),
        json!({"rail_fit": "stretch "}),
        json!({"rails": "l1"}),
        json!({"rails": {"curves": ["l1"]}}),
        json!({"rails": []}),
        json!({"rails": [{"curves": ["l1"]}, {"curves": ["l2"]}, {"curves": ["l3"]}]}),
        json!({"rails": [{}]}),
        json!({"rails": [5]}),
        json!({"rails": [{"curves": []}]}),
        json!({"rails": [{"curves": null}]}),
        json!({"rails": [{"curves": 5}]}),
        json!({"rails": [{"curves": ["l1", 2]}]}),
        json!({"rails": [{"curves": vec!["l1"; 1001]}]}),
        json!({"rails": [{"curves": vec!["l1"; 10_001]}]}),
        json!({"rails": [{"sketch": true, "curves": ["l1"]}]}),
        json!({"rails": [{"sketch": [], "curves": ["l1"]}]}),
        json!({"rails": [{"curves": ["l1"]}, {"sketch": {}, "curves": ["l2"]}]}),
        json!({"rails": [{"sketch": "Missing", "curves": ["l1"]}]}),
        json!({"rails": rail, "orientation": "parallel"}),
        json!({"rails": rail, "twist": "10 deg"}),
        json!({"rails": rail, "scale": 2}),
        json!({"path_sketch": true}),
        json!({"path_sketch": null}),
        json!({"path_sketch": "Missing"}),
        json!({"path": []}),
        json!({"sketch": true}),
        json!({"sketch": null}),
        json!({"sketch": "Missing"}),
        json!({"profiles": []}),
        json!({"profiles": [true]}),
        json!({"profiles": [{"point": [1]}]}),
        json!({"profiles": null}),
        json!({"participants": ["Missing"]}),
    ] {
        for id in ["solid.sweep", "surface.sweep"] {
            let mut p = json!({"path_sketch": "Sketch1", "path": ["l1"]});
            p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
            cases.push((id, p));
        }
    }
    cases.extend([
        ("solid.sweep", json!({"path_sketch": "Sketch1", "path": ["l1"], "operation": "bogus"})),
        ("solid.sweep", json!({"path_sketch": "Sketch1", "path": ["l1"], "operation": "auto"})),
        ("surface.sweep", json!({"path_sketch": "Sketch1", "path": vec!["l1"; 1001]})),
        ("surface.sweep", json!({"path_sketch": "Sketch1", "path": ["l1"], "curves": vec!["l1"; 1001]})),
    ]);
    assert_strict_keeps_todays_errors(&cases);
}

/// A circle `C` (radius 10, curve `c`) and a line `L` across it (`l`) on XY; a path `P` on XZ, a
/// line 40 up the Z axis (`up`), with a rail beside it (`in`) from the circle's edge in to x = 5
/// at the top; a block `Block` the sweep cuts a quarter from, and a plate `Plate` inside it.
fn sweep_rig(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY", "name": "C"})),
        ("sketch.circle.center", json!({"center": [0, 0], "radius": 10, "id": "c"})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "XY", "name": "L"})),
        ("sketch.line", json!({"points": [[-10, 0], [10, 0]], "ids": ["l"]})),
        ("sketch.finish", json!({})),
        ("sketch.create", json!({"plane": "XZ", "name": "P"})),
        // The XZ sketch's y runs down world Z.
        ("sketch.line", json!({"points": [[0, 0], [0, -40]], "ids": ["up"]})),
        ("sketch.line", json!({"points": [[10, 0], [5, -40]], "ids": ["in"]})),
        ("sketch.finish", json!({})),
        ("solid.box", json!({"corner": [0, 0, 0], "length": 30, "width": 30, "height": 10, "body_name": "Block"})),
        ("solid.box", json!({"corner": [-3, -3, 20], "length": 6, "width": 6, "height": 5, "body_name": "Plate"})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s
}

/// Every form a sweep call takes, each command with its own.
fn sweep_calls() -> Vec<(&'static str, Value)> {
    let on = |p: Value| {
        let mut call = json!({"path_sketch": "P", "path": ["up"]});
        call.as_object_mut().into_iter().for_each(|o| o.extend(p.as_object().cloned().unwrap_or_default()));
        call
    };
    let solid = [
        json!({"sketch": "C", "name": "A", "body_name": "A"}),
        json!({"sketch": "C", "profiles": "all", "exact_regions": true, "path": "up", "orientation": "Parallel", "body_names": ["B"]}),
        json!({"sketch": "C", "profiles": [0], "orientation": [1, 0, 0], "twist": "90 deg", "scale": 0.5, "operation": "new"}),
        json!({"sketch": "C", "orientation": {"direction": {"x": 1, "y": 0, "z": 0}}, "twist": 45, "scale": "1.5"}),
        json!({"sketch": "C", "orientation": "perpendicular", "twist": null, "scale": null, "rails": null, "rail_fit": null, "touching": null}),
        json!({"sketch": "C", "rails": [{"curves": ["in"]}]}),
        json!({"sketch": "C", "rails": [{"sketch": "P", "curves": "in"}], "rail_fit": "Stretch"}),
        json!({"sketch": "C", "rails": [{"sketch": null, "curves": ["in"]}], "rail_fit": "scale"}),
        json!({"sketch": "C", "operation": "join", "targets": ["A"], "touching": "one_body", "participants": []}),
        json!({"sketch": "C", "operation": "cut", "targets": "Block", "name": "Cut"}),
        json!({"sketch": "C", "operation": "intersect", "participants": ["Plate"]}),
    ];
    let surface = [
        json!({"sketch": "L", "curves": ["l"], "body_name": "W"}),
        json!({"sketch": "L", "curves": "l", "orientation": [1, 0, 0], "twist": "30 deg", "scale": 2, "body_names": ["X"]}),
        json!({"sketch": "C", "profiles": [0], "exact_regions": false, "orientation": "PARALLEL"}),
        json!({"sketch": "C", "rails": [{"sketch": "P", "curves": ["in"]}], "rail_fit": "stretch", "name": "Railed"}),
        json!({"sketch": "C", "profiles": "all", "orientation": null, "rails": null, "participants": []}),
    ];
    let mut calls: Vec<(&'static str, Value)> = solid.into_iter().map(|p| ("solid.sweep", on(p))).collect();
    calls.extend(surface.into_iter().map(|p| ("surface.sweep", on(p))));
    calls
}

/// `solid.sweep` and `surface.sweep` reply with `FeatureResult` in each of their forms, their
/// contracts taking each: in Warn a contract that refused one would still run it, so the replies
/// are checked for warnings.
#[test]
fn sweep_replies_match_the_output_schema_and_forms_are_accepted() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let mut s = sweep_rig(ParamCheck::Warn);
    for (id, p) in sweep_calls() {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
    }
    // Warn reports what the contracts refuse and runs the call anyway.
    for id in ["solid.sweep", "surface.sweep"] {
        let typo = s.execute(id, &json!({"sketch": "C", "path_sketch": "P", "path": ["up"], "twistt": 10})).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(typo["param_warnings"], json!(["unknown key `twistt`; did you mean `twist`?"]), "{id}");
        assert!(conforms(schema, schema, &typo), "{typo}");
    }
}

/// The check changes nothing about the replies: each call gives the same JSON with the check off
/// and in strict (`ms`, the time it took, apart), and its keys are the ones the commands always
/// answered with.
#[test]
fn sweep_replies_keep_their_old_shape_whatever_the_check() {
    let (mut off, mut strict) = (sweep_rig(ParamCheck::Off), sweep_rig(ParamCheck::Strict));
    for (id, p) in sweep_calls() {
        let run = |s: &mut Session| {
            let mut r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            let keys: BTreeSet<String> = r.as_object().into_iter().flat_map(|o| o.keys().cloned()).collect();
            assert!(r["ms"].is_null() || r["ms"].is_number(), "{r}");
            r["ms"] = Value::Null;
            (r, keys)
        };
        let (a, a_keys) = run(&mut off);
        let (b, _) = run(&mut strict);
        assert_eq!(a, b, "{id} {p}");
        let old = ["bodies", "changed_bodies", "created_bodies", "feature", "ms", "name", "removed_bodies", "warning"];
        assert!(old.iter().all(|k| a_keys.contains(*k)), "{a}");
        assert!(a_keys.iter().all(|k| old.contains(&k.as_str()) || matches!(k.as_str(), "component" | "notice")), "{a}");
    }
}

/// What the handlers ignore or read as left out, strict refuses; off and warn run it as before.
#[test]
fn sweep_wrong_typed_keys_are_reported() {
    let mut cases: Vec<(&str, Value, Value)> = Vec::new();
    for id in ["solid.sweep", "surface.sweep"] {
        for (key, value) in
            [("exact_regions", json!(1)), ("name", json!(5)), ("body_name", json!(5)), ("colour", json!("red")), ("orient", json!("parallel"))]
        {
            cases.push((id, json!({ key: value }), json!(1)));
        }
        // A misspelt key inside a rail: the handler reads the rail as in the path's sketch.
        cases.push((id, json!({"rails": [{"curves": ["in"], "skecth": "P"}]}), json!(1)));
    }
    cases.push(("solid.sweep", json!({"operation": 5}), json!(1)));
    // The surface sweep doesn't combine bodies: its handler leaves these alone.
    cases.push(("surface.sweep", json!({"operation": "join", "targets": ["Block"]}), json!(2)));
    cases.push(("surface.sweep", json!({"touching": "one_body"}), json!(1)));
    for (id, extra, warnings) in cases {
        let mut p = json!({"sketch": "C", "path_sketch": "P", "path": ["up"]});
        p.as_object_mut().into_iter().for_each(|o| o.extend(extra.as_object().cloned().unwrap_or_default()));
        let mut off = sweep_rig(ParamCheck::Off);
        let r = off.execute(id, &p);
        assert!(r.is_ok(), "{id} {p}: {r:?}");
        let mut warn = sweep_rig(ParamCheck::Warn);
        let r = warn.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert_eq!(r["param_warnings"].as_array().map(Vec::len), warnings.as_u64().map(|n| n as usize), "{id} {p}: {r}");
        let mut strict = sweep_rig(ParamCheck::Strict);
        assert!(strict.execute(id, &p).is_err(), "{id} {p}");
    }
}

/// The sweep value types take what the handlers' readers take, and their schemas say the same,
/// apart from what a schema can't say (case, a zero direction, the rail curves' 10 000 cap).
#[test]
fn sweep_values_match_the_handler_readers() {
    use crate::schema::types::{RailFitParam, SweepOrientationParam, SweepRails, SweepTwist};
    fn accepts<T: serde::de::DeserializeOwned>(v: &Value) -> bool {
        serde_json::from_value::<T>(v.clone()).is_ok()
    }
    let orientation = input_schema_for::<SweepOrientationParam>();
    for (v, accepted, schema) in [
        (json!("perpendicular"), true, true),
        (json!("parallel"), true, true),
        (json!("Parallel"), true, false),
        (json!(" parallel"), false, false),
        (json!("sideways"), false, false),
        (json!([0, 0, 1]), true, true),
        (json!({"x": 0, "y": 1, "z": 0}), true, true),
        (json!({"direction": [1, 0, 0]}), true, true),
        (json!({"direction": {"x": 1, "y": 0, "z": 0}, "extra": 1}), true, true),
        (json!([0, 0, 0]), false, true),
        (json!({"direction": [0, 0, 0]}), false, true),
        (json!([0, 1]), false, false),
        (json!([0, 0, 1e9]), false, false),
        (json!({"direction": "x"}), false, false),
        (json!({}), false, false),
        (json!(5), false, false),
        (json!(true), false, false),
    ] {
        let reader = crate::cmd::sweep_orientation(&v).is_ok();
        assert_eq!(accepts::<SweepOrientationParam>(&v), reader, "{v}");
        assert_eq!(reader, accepted, "{v}");
        assert_eq!(conforms(orientation.as_value(), orientation.as_value(), &v), schema, "schema {v}");
    }
    let fit = input_schema_for::<RailFitParam>();
    for (v, accepted, schema) in [
        (json!("scale"), true, true),
        (json!("stretch"), true, true),
        (json!("STRETCH"), true, false),
        (json!("stretch "), false, false),
        (json!("wobble"), false, false),
        (json!(1), false, false),
    ] {
        let reader = crate::cmd::rail_fit_form(&v).is_ok();
        assert_eq!(accepts::<RailFitParam>(&v), reader, "{v}");
        assert_eq!(reader, accepted, "{v}");
        assert_eq!(conforms(fit.as_value(), fit.as_value(), &v), schema, "schema {v}");
    }
    for (v, accepted) in
        [(json!(10), true), (json!("90 deg"), true), (json!(" a / 2 "), true), (json!("  "), false), (json!(true), false), (json!(1e9), false)]
    {
        let want = crate::params::expr(&json!({ "v": v.clone() }), "v");
        assert_eq!(serde_json::from_value::<SweepTwist>(v.clone()).ok().map(|t| t.0), want, "{v}");
        assert_eq!(want.is_some(), accepted, "{v}");
    }
    let rails = input_schema_for::<SweepRails>();
    for (v, accepted) in [
        (json!([{"curves": ["a"]}]), true),
        (json!([{"curves": "a"}]), true),
        (json!([{"sketch": "R", "curves": ["a", "b"]}, {"sketch": 3, "curves": ["c"]}]), true),
        (json!([{"sketch": null, "curves": ["a"]}]), true),
        (json!([{"curves": vec!["a"; 1000]}]), true),
        (json!([{"curves": vec!["a"; 1001]}]), false),
        (json!([]), false),
        (json!([{"curves": ["a"]}, {"curves": ["b"]}, {"curves": ["c"]}]), false),
        (json!([{}]), false),
        (json!([{"curves": []}]), false),
        (json!([{"curves": [1]}]), false),
        (json!([{"sketch": true, "curves": ["a"]}]), false),
        (json!([{"curves": ["a"], "skecth": "R"}]), false),
        (json!(["a"]), false),
        (json!({"curves": ["a"]}), false),
        (json!("a"), false),
    ] {
        assert_eq!(accepts::<SweepRails>(&v), accepted, "{v}");
        assert_eq!(conforms(rails.as_value(), rails.as_value(), &v), accepted, "schema {v}");
    }
    let issue = serde_json::from_value::<SweepRails>(json!([{"curves": ["a"]}, {"curves": ["b"], "skecth": "R"}])).err().map(|e| e.to_string());
    assert!(issue.is_some_and(|e| e.ends_with("unknown key `skecth` in `rails[1]`: a rail takes `sketch` and `curves`")));
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

/// #12884: Chamfer's contract takes what its input schema allows: each call's check finds no issue
/// exactly when the call matches the schema. Both `distance2` and `angle` match it; the handler
/// refuses them together.
#[test]
fn chamfer_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<ChamferParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (json!({"edges": [[0, 0, 0]], "distance": 1}), true),
        (json!({"edges": [{"body": "B", "index": 0}], "distance": "2 mm", "distance2": 3}), true),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "angle": "30 deg", "flip": true, "tangent_chain": false, "body": "B", "name": "Bevel"}), true),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "distance2": null, "angle": null, "flip": null, "body": null, "tangent_chain": null}), true),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "distance2": 1, "angle": 30}), true),
        (json!({"edges": [[0, 0, 0]]}), false),
        (json!({"distance": 1}), false),
        (json!({"edges": [], "distance": 1}), false),
        (json!({"edges": [[0, 0, 0]], "distance": true}), false),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "distance2": true}), false),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "angle": [30]}), false),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "flip": "yes"}), false),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "tangent_chain": 1}), false),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "body": 5}), false),
        (json!({"edges": [[0, 0, 0]], "distance": 1, "colour": "red"}), false),
    ] {
        let issues = crate::schema::issues("solid.chamfer", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// #12884: Chamfer replies with `FeatureResult`, with the keys it gave before, in each of its
/// forms: one distance, two, a distance and an angle, flipped, an edge by index, and every
/// optional key `null`. In Warn, without warnings, so its contract takes each call.
#[test]
fn chamfer_replies_with_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let before = BTreeSet::from(["feature", "name", "bodies", "created_bodies", "changed_bodies", "removed_bodies", "warning", "ms"]);
    // A 40 x 30 x 10 plate with a 10 x 10 x 5 boss on it, whose top edges `boss` are.
    let run = |p: Value| {
        let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "body_name": "B"})).unwrap_or_else(|e| panic!("{e}"));
        s.execute("solid.box", &json!({"corner": [15, 10, 10], "length": 10, "width": 10, "height": 5, "operation": "join"}))
            .unwrap_or_else(|e| panic!("{e}"));
        let r = s.execute("solid.chamfer", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        let keys: BTreeSet<&str> = r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, before, "{p}: {r}");
        assert_eq!(r["changed_bodies"], json!(["B"]), "{p}: {r}");
    };
    let boss = json!([[20, 10, 15], [25, 15, 15], [20, 20, 15], [15, 15, 15]]);
    run(json!({"edges": [[20, 0, 10]], "distance": 1}));
    run(json!({"edges": boss, "distance": 1, "distance2": "0.5 mm", "flip": true, "tangent_chain": true, "body": "B", "name": "Bevel"}));
    run(json!({"edges": [[40, 15, 10]], "distance": "1 mm", "angle": "30 deg"}));
    run(json!({"edges": [{"body": "B", "index": 0}], "distance": 0.5, "tangent_chain": false}));
    run(json!({"edges": [[20, 30, 10]], "distance": 1, "distance2": null, "angle": null, "flip": null, "body": null, "tangent_chain": null}));
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

/// A 2 x 10 profile standing off the Y axis in the XY plane, a box to join to or turn a face of,
/// and a construction axis to turn about.
fn revolve_rig(level: ParamCheck) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    for (id, p) in [
        ("sketch.create", json!({"name": "Profile", "plane": "XY"})),
        ("sketch.rectangle.two_point", json!({"p0": [2, 0], "p1": [4, 10]})),
        ("sketch.finish", json!({})),
        ("solid.box", json!({"corner": [20, 0, 0], "length": 5, "width": 5, "height": 5, "body_name": "Block"})),
        ("construct.axis.two_points", json!({"a": {"point": [0, 0, 0]}, "b": {"point": [0, 1, 0]}, "name": "Spindle"})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    s
}

/// Every form a revolve call takes.
fn revolve_calls() -> Vec<Value> {
    vec![
        json!({"sketch": "Profile", "axis": "y", "angle": "90 deg", "operation": "new", "name": "A", "body_name": "A"}),
        json!({"axis": "Y", "angle": 45, "direction": "Symmetric", "body_names": ["B"]}),
        json!({"sketch": 1, "axis": {"origin": ["0 mm", 0, 0], "dir": {"x": 0, "y": 1, "z": 0}}, "symmetric": true, "convention": "legacy_axis_start"}),
        json!({"axis": {"dir": [0, 1, 0]}, "angle": "30 deg", "angle2": "15 deg", "profiles": "all", "exact_regions": true}),
        json!({"axis": {"type": "world", "axis": "Y"}, "angle": 20, "profiles": [0], "convention": "sketch_normal"}),
        json!({"axis": "Spindle", "angle": 10, "direction": null, "symmetric": null, "touching": null}),
        json!({"axis": {"type": "sketch_line", "dir": [0, 1, 0]}, "to": [0, 5, 3], "angle": 5, "angle2": null}),
        json!({"axis": "y", "angle": "60 deg", "operation": "union", "targets": ["Block"], "participants": [], "touching": "one_body"}),
        json!({"axis": "y", "angle": 15, "operation": "join", "participants": ["A"], "name": "Join"}),
        json!({"axis": "y", "angle": 15, "operation": "cut", "targets": "A"}),
        json!({"axis": {"origin": [0, 0, 5], "dir": [0, 1, 0]}, "face": [22.5, 2.5, 5], "face_body": "Block", "body": "Block", "angle": 90}),
    ]
}

/// `solid.revolve` replies with `FeatureResult` in each of its forms, its contract taking each: in
/// Warn a contract that refused one would still run it, so the replies are checked for warnings.
#[test]
fn revolve_replies_match_the_output_schema_and_forms_are_accepted() {
    let schema = output_schema_for::<FeatureResult>();
    let schema = schema.as_value();
    let mut s = revolve_rig(ParamCheck::Warn);
    for p in revolve_calls() {
        let r = s.execute("solid.revolve", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
    }
    // Warn reports what the contract refuses and runs the call anyway.
    let typo = s.execute("solid.revolve", &json!({"axis": "y", "angel": 10})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(typo["param_warnings"], json!(["unknown key `angel`"]));
    assert!(conforms(schema, schema, &typo), "{typo}");
}

/// The check changes nothing about the reply: each call gives the same JSON with the check off
/// and in strict (`ms`, the time it took, apart), and its keys are the ones the command always
/// answered with.
#[test]
fn revolve_reply_keeps_its_old_shape_whatever_the_check() {
    let (mut off, mut strict) = (revolve_rig(ParamCheck::Off), revolve_rig(ParamCheck::Strict));
    for p in revolve_calls() {
        let run = |s: &mut Session| {
            let mut r = s.execute("solid.revolve", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
            let keys: BTreeSet<String> = r.as_object().into_iter().flat_map(|o| o.keys().cloned()).collect();
            assert!(r["ms"].is_null() || r["ms"].is_number(), "{r}");
            r["ms"] = Value::Null;
            (r, keys)
        };
        let (a, a_keys) = run(&mut off);
        let (b, _) = run(&mut strict);
        assert_eq!(a, b, "{p}");
        let old = ["bodies", "changed_bodies", "created_bodies", "feature", "ms", "name", "removed_bodies", "warning"];
        assert!(old.iter().all(|k| a_keys.contains(*k)), "{a}");
        assert!(a_keys.iter().all(|k| old.contains(&k.as_str()) || matches!(k.as_str(), "component" | "notice")), "{a}");
    }
}

/// What the handler ignores or reads as left out, strict refuses; off and warn run it as before.
#[test]
fn revolve_wrong_typed_keys_are_reported() {
    for (key, value) in [
        ("direction", json!("negative")),
        ("direction", json!(true)),
        ("symmetric", json!("yes")),
        ("exact_regions", json!(1)),
        ("angle", json!(true)),
        ("angle", json!("  ")),
        ("angle2", json!([])),
        ("face", json!("x")),
        ("face", json!([1, 2])),
        ("face_body", json!(5)),
        ("body", json!(5)),
        ("name", json!(5)),
        ("body_name", json!(5)),
        ("operation", json!(5)),
        ("colour", json!("red")),
    ] {
        let p = json!({"axis": "y", "angle": 30, key: value});
        let mut off = revolve_rig(ParamCheck::Off);
        let r = off.execute("solid.revolve", &p);
        assert!(r.is_ok(), "{p}: {r:?}");
        let mut warn = revolve_rig(ParamCheck::Warn);
        let r = warn.execute("solid.revolve", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{p}: {r}");
        let mut strict = revolve_rig(ParamCheck::Strict);
        assert!(strict.execute("solid.revolve", &p).is_err(), "{p}");
    }
}

/// A box named `Post` away from the Z axis, and a component `Part` to place more of.
fn ring_rig(level: ParamCheck) -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "name": "Post"})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("component.create", &json!({"name": "Part", "activate": false})).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = level;
    s
}

fn ring() -> Value {
    json!({"features": ["Post"], "axis": "Z", "count": 4})
}

fn ring_with(key: &str, value: Value) -> Value {
    let mut p = ring();
    p[key] = value;
    p
}

#[test]
fn circular_pattern_schema_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<PatternCircularParams>(),
        &["alignment", "angle", "axis", "bodies", "body_name", "body_names", "components", "count", "features", "name", "participants", "reversed"],
    );
}

/// Calls Circular Pattern refuses today get the same message in strict; an `angle` that isn't a
/// number or text, which the handler ignores, is refused by the contract, and so is a misspelt key.
#[test]
fn circular_pattern_contract_keeps_the_handlers_errors() {
    let cases = [
        json!({"axis": "Z", "count": 4}),
        ring_with("features", json!(5)),
        ring_with("features", json!(["Nope"])),
        ring_with("bodies", json!(["Nope"])),
        ring_with("axis", json!("Nope")),
        ring_with("axis", json!(5)),
        ring_with("axis", json!({"origin": [0, 0], "dir": [0, 0, 1]})),
        ring_with("axis", json!({"origin": [0, 0, 0]})),
        ring_with("axis", json!({"origin": [0, 0, 0], "dir": [0, 0, 0]})),
        ring_with("axis", json!({"dir": [0, 0, 0]})),
        ring_with("count", json!(true)),
        ring_with("count", json!("nope")),
        ring_with("angle", json!("5 mm")),
        ring_with("alignment", json!("up")),
        ring_with("alignment", json!(null)),
        ring_with("reversed", json!("yes")),
        ring_with("reversed", json!(null)),
    ];
    for p in &cases {
        let error = |level| ring_rig(level).execute("solid.pattern.circular", p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
    assert!(!crate::schema::issues("solid.pattern.circular", &ring_with("angle", json!(true))).is_empty());
    assert!(ring_rig(ParamCheck::Strict).execute("solid.pattern.circular", &ring_with("cuont", json!(4))).is_err());
}

/// Each form the handler takes passes strict, and every reply matches the output schema.
#[test]
fn circular_pattern_replies_match_the_output_schema() {
    let schema = output_schema_for::<PatternCircularResult>();
    let schema = schema.as_value();
    let calls = [
        ring(),
        ring_with("axis", json!("z")),
        ring_with("axis", json!({"dir": [0, 0, 1]})),
        ring_with("axis", json!({"origin": ["0 mm", 0, 0], "dir": [0, 0, 1]})),
        ring_with("count", json!("2 + 2")),
        ring_with("angle", json!("180 deg")),
        ring_with("alignment", json!("center")),
        ring_with("reversed", json!(true)),
        json!({"components": ["Part"], "axis": "Z", "count": 3}),
    ];
    for p in &calls {
        let r = ring_rig(ParamCheck::Strict).execute("solid.pattern.circular", p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    let warned = json!({"components": ["Part"], "axis": "Z", "count": 3, "colour": "red"});
    let r = ring_rig(ParamCheck::Warn).execute("solid.pattern.circular", &warned).unwrap_or_else(|e| panic!("{e}"));
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()) && conforms(schema, schema, &r), "{r}");
}

/// A 40 x 30 x 20 block from the origin, `Block`, with its top front edge rounded (radius 2) when
/// `rounded` is true.
fn press_pull_block(level: ParamCheck, rounded: bool) -> Session {
    let mut s = Session { param_check: level, ..Session::default() };
    s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [0, 0, 0], "body_name": "Block"})).unwrap();
    if rounded {
        s.execute("solid.fillet", &json!({"edges": [[20, 0, 20]], "radius": 2})).unwrap();
    }
    s
}

/// Press Pull describes its keys, keeps today's errors, and each kind of reply it gives fits its
/// output schema: a new feature from a flat face and from edges, and a fillet's size edited.
#[test]
fn press_pull_contract_and_replies() {
    assert_keys(input_schema_for::<PressPullParams>(), &["body", "distance", "edges", "face"]);
    assert_strict_keeps_todays_errors(&[
        ("solid.press_pull", json!({"face": [0, 0, 0]})),
        ("solid.press_pull", json!({"distance": 1})),
        ("solid.press_pull", json!({"face": [0, 0, 0], "distance": true})),
    ]);
    let schema = output_schema_for::<PressPullResult>();
    let schema = schema.as_value();
    let root2 = 2f64.sqrt();
    let replies = [
        (false, json!({"face": [20, 15, 20], "distance": 5}), false),
        (false, json!({"edges": [[20, 0, 20]], "distance": 1, "body": "Block"}), false),
        // Half way round the 2 mm fillet on the top front edge.
        (true, json!({"face": [20, 2.0 - root2, 18.0 + root2], "distance": 1}), true),
    ];
    for (rounded, p, edited) in replies {
        let r = press_pull_block(ParamCheck::Strict, rounded).execute("solid.press_pull", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!(r.get("edited").is_some(), edited, "{r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
}

/// With its contract, Press Pull refuses an unknown key in strict.
#[test]
fn press_pull_checks_its_keys() {
    let p = json!({"face": [20, 15, 20], "distance": 5, "flp": true});
    let error = press_pull_block(ParamCheck::Strict, false).execute("solid.press_pull", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`flp`"), "{error}");
}

/// The Move contract accepts an origin turn and a matrix, replies to its output schema, refuses an
/// unknown key when strict and warns about it otherwise.
#[test]
fn move_contract_accepts_transforms_and_reports_unknown_keys() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Default::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "B"})).unwrap();
    for p in [json!({"bodies": ["B"], "origin": [5, 5, 0], "angle": "90 deg"}), json!({"bodies": ["B"], "matrix": [[1,0,0,2], [0,1,0,0], [0,0,1,0]]})]
    {
        let output = s.execute("solid.move", &p).unwrap();
        let schema = output_schema_for::<MoveResult>();
        assert!(conforms(schema.as_value(), schema.as_value(), &output), "{output}");
    }
    let saved = s.doc.to_json();
    assert!(s.execute("solid.move", &json!({"bodies": ["B"], "orgin": [5,5,0]})).is_err());
    assert_eq!(s.doc.to_json(), saved);
    s.param_check = ParamCheck::Warn;
    let output = s.execute("solid.move", &json!({"bodies": ["B"], "orgin": [5,5,0]})).unwrap();
    assert!(output["param_warnings"].as_array().is_some_and(|v| !v.is_empty()));
    // The schema states the tolerance's bounds, as `occurrence.move`'s does.
    let schema = input_schema_for::<MoveParams>().as_value().to_string();
    assert!(schema.contains(r#""exclusiveMinimum":0"#) && schema.contains(r#""maximum":0.001"#), "{schema}");
}

/// Component copies, alone or with bodies split across frames, pass the strict contract and reply
/// to the output schema.
#[test]
fn move_contract_accepts_component_copies_with_and_without_split_body_features() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Default::default() };
    let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Peg"})).unwrap();
    s.execute("occurrence.move", &json!({"occurrence": made["occurrence"], "translate": [100, 0, 0]})).unwrap();
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Top"})).unwrap();
    let output = s.execute("solid.move", &json!({"occurrences": [[made["occurrence"]]], "copy": true, "translate": [20, 0, 0]})).unwrap();
    assert_eq!(output["occurrences"].as_array().map(Vec::len), Some(1));
    let schema = output_schema_for::<MoveResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &output), "{output}");
    let output =
        s.execute("solid.move", &json!({"bodies": ["Peg", "Top"], "occurrences": [[made["occurrence"]]], "copy": true, "angle": "90 deg"})).unwrap();
    assert_eq!(output["features"].as_array().map(Vec::len), Some(2));
    assert_eq!(output["occurrences"].as_array().map(Vec::len), Some(1));
    assert!(conforms(schema.as_value(), schema.as_value(), &output), "{output}");
}
