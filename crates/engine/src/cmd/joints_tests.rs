use solidcraft_geom::prelude::*;
use std::f64::consts::FRAC_PI_2;

use serde_json::{Value, json};
use solidcore_record::Vec3;

use crate::Session;

pub(super) fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn coderabbit_review_plane_joint() -> (Session, Value) {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "ReviewA", "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "ReviewB", "grounded": false}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let p = json!({"name":"ReviewJoint", "type":"rigid", "angle":0,
        "a":{"occurrence":"ReviewA", "plane":"XY"}, "b":{"occurrence":"ReviewB", "plane":"XY"}});
    (s, p)
}

fn coderabbit_review_saved_x(s: &Session, side: &str) -> Option<solidcraft_doc::joints::XRef> {
    use solidcraft_doc::joints::Snap;
    let j = &s.doc.assembly.joints[0];
    match if side == "a" { &j.a.snap } else { &j.b.snap } {
        Snap::FaceCenter { x, .. } | Snap::CircleCenter { x, .. } | Snap::Plane { x, .. } | Snap::Axis { x, .. } => x.clone(),
        _ => None,
    }
}

#[test]
fn coderabbit_review_primary_replacement_retains_alignment_and_validates_it() {
    use solidcraft_doc::joints::XRef;
    for (side, occurrence) in [("a", "ReviewA"), ("b", "ReviewB")] {
        let (mut s, mut p) = coderabbit_review_plane_joint();
        let alignment_key = format!("{side}_alignment");
        p[&alignment_key] = json!({"reference":{"type":"origin_axis", "axis":"Y"}});
        run(&mut s, "joint.create", p);
        let saved = coderabbit_review_saved_x(&s, side);
        assert!(matches!(saved, Some(XRef::Reference { .. })));
        let primary = json!({"occurrence":occurrence, "plane":"XY"});
        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", side:primary}));
        assert_eq!(coderabbit_review_saved_x(&s, side), saved, "omitted {alignment_key} must retain its reference");

        let before = s.doc.clone();
        let error = s.execute("joint.edit", &json!({"joint":"ReviewJoint", side:{"occurrence":occurrence, "plane":"XZ"}})).unwrap_err();
        assert!(error.to_string().contains("parallel"), "{error}");
        assert_eq!(s.doc, before, "invalid replacement must be atomic");

        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", side:primary, (alignment_key.as_str()):null}));
        assert!(!matches!(coderabbit_review_saved_x(&s, side), Some(XRef::Reference { .. } | XRef::Planar { .. })));
        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", (alignment_key.as_str()):{"reference":{"type":"origin_axis", "axis":"Y"}}}));
        run(
            &mut s,
            "joint.edit",
            json!({"joint":"ReviewJoint", side:{"occurrence":occurrence,"plane":"XY", "x":{"reference":{"type":"origin_axis","axis":"X"}}}}),
        );
        assert!(
            matches!(coderabbit_review_saved_x(&s, side), Some(XRef::Reference { reference:solidcraft_doc::joints::AlignmentRef::OriginAxis { axis }, .. }) if axis == "X")
        );
        run(
            &mut s,
            "joint.origin",
            json!({"name":"ReviewNamed", "occurrence":occurrence, "plane":"XY", "x":{"reference":{"type":"origin_axis","axis":"Y"}}}),
        );
        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", side:"ReviewNamed"}));
        assert!(
            matches!(coderabbit_review_saved_x(&s, side), Some(XRef::Reference { reference:solidcraft_doc::joints::AlignmentRef::OriginAxis { axis }, .. }) if axis == "Y")
        );
    }
}

#[test]
fn coderabbit_review_lateral_offsets_refuse_secondary_alignment_atomically() {
    for side in ["a", "b"] {
        for offset in ["offset_x", "offset_y"] {
            for inline in [false, true] {
                let (mut s, mut p) = coderabbit_review_plane_joint();
                p[offset] = json!(10);
                if inline {
                    p[side]["x"] = json!({"reference":{"type":"origin_axis", "axis":"Y"}});
                } else {
                    p[format!("{side}_alignment")] = json!({"reference":{"type":"origin_axis", "axis":"Y"}});
                }
                let before = s.doc.clone();
                let error = s.execute("joint.create", &p).unwrap_err();
                assert!(error.to_string().contains("lateral"), "{side} {offset} {inline}: {error}");
                assert_eq!(s.doc, before);
            }
        }
    }
    for side in ["a", "b"] {
        let (mut s, mut p) = coderabbit_review_plane_joint();
        p["offset_x"] = json!(10);
        run(&mut s, "joint.create", p);
        let before = s.doc.clone();
        let key = format!("{side}_alignment");
        let error =
            s.execute("joint.edit", &json!({"joint":"ReviewJoint", (key.as_str()):{"reference":{"type":"origin_axis","axis":"Y"}}})).unwrap_err();
        assert!(error.to_string().contains("lateral"), "{error}");
        assert_eq!(s.doc, before);
        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", (key.as_str()):null}));
        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", "offset_x":0, (key.as_str()):{"reference":{"type":"origin_axis","axis":"Y"}}}));
        let before = s.doc.clone();
        assert!(s.execute("joint.edit", &json!({"joint":"ReviewJoint", "offset_y":10})).is_err());
        assert_eq!(s.doc, before);
        // Old documents may already contain this unsupported combination: clearing remains possible.
        s.doc_mut().assembly.joints[0].offset_xy = [10.0, 0.0];
        run(&mut s, "joint.edit", json!({"joint":"ReviewJoint", (key.as_str()):null}));
        assert_eq!(s.doc.assembly.joints[0].offset_xy, [10.0, 0.0]);
    }
}

#[test]
fn coderabbit_review_nested_inline_secondary_references_match_side_argument_restrictions() {
    let mut s = Session::default();
    let (first, second, header) = board_twice(&mut s);
    for (parent, point) in [(first, json!([-5, 25, 10])), (second, json!([95, 25, 10]))] {
        for inline in [false, true] {
            let mut p = json!({"type":"rigid", "a":{"occurrence":header,"path":[parent,header],"face":point},
                "b":{"occurrence":"Probe", "plane":"XY"}});
            let reference = json!({"reference":{"type":"origin_axis","axis":"X"}});
            if inline {
                p["a"]["x"] = reference;
            } else {
                p["a_alignment"] = reference;
            }
            let before = s.doc.clone();
            let error = s.execute("joint.create", &p).unwrap_err();
            assert!(error.to_string().contains("nested occurrence paths"), "{inline}: {error}");
            assert_eq!(s.doc, before);
        }
    }
}

#[test]
fn secondary_construction_axis_follows_edits_undo_and_uncached_reopen() {
    use solidcraft_doc::joints::{self, XRef};
    use solidcraft_doc::{Document, apply_point, apply_vector};
    let mut s = Session::default();
    let id = run(
        &mut s,
        "construct.axis.two_points",
        json!({
            "a": {"point": [0, 0, 0]}, "b": {"point": [1, 0, 0]}, "name": "Clocking",
        }),
    )["feature"]
        .as_u64()
        .unwrap();
    let x = json!({"reference": {"type": "construction", "feature": id}});
    run(&mut s, "joint.origin", json!({"occurrence": 0, "plane": "XY", "x": x, "name": "Seat"}));
    let made = run(&mut s, "component.create", json!({"name": "Peg", "grounded": false}));
    let occurrence = made["occurrence"].as_u64().unwrap();
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": 0, "plane": "XY", "x": x},
        "b": {"occurrence": occurrence, "plane": "XY"}, "flip": true, "angle": 0}),
    );
    let check = |s: &Session, expected: Vec3| {
        let j = &s.doc.assembly.joints[0];
        assert!(matches!(j.a.snap, joints::Snap::Plane { x: Some(XRef::Reference { .. }), .. }));
        let m = joints::resolve_origin(&s.doc, &s.model.state(), &j.a).unwrap();
        assert_eq!(apply_vector(&m, Vec3::X), expected);
        assert_eq!(apply_vector(&m, Vec3::Z), Vec3::Z);
        assert_eq!(apply_point(&m, Vec3::ZERO), Vec3::ZERO);
        assert_eq!(apply_vector(&joints::occurrence_world(&s.doc, occurrence), Vec3::X), expected);
    };
    check(&s, Vec3::X);
    let joint = s.doc.assembly.joints[0].id;
    run(&mut s, "joint.edit", json!({"joint": joint, "a": {"occurrence": 0, "plane": "XY", "x": x}}));
    check(&s, Vec3::X);
    run(&mut s, "timeline.rename", json!({"feature": id, "name": "Renamed"}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": id, "command": "construct.axis.two_points",
        "params": {"a": {"point": [0, 0, 0]}, "b": {"point": [0, 1, 0]}}}),
    );
    check(&s, Vec3::Y);
    run(&mut s, "edit.undo", json!({}));
    check(&s, Vec3::X);
    run(&mut s, "edit.redo", json!({}));
    check(&s, Vec3::Y);
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    reopened.refresh();
    check(&reopened, Vec3::Y);
    for dir in [[-0.6, 0.8, 0.0], [-0.8, 0.6, 0.0]] {
        run(
            &mut reopened,
            "timeline.redefine",
            json!({"feature": id, "command": "construct.axis.two_points",
            "params": {"a": {"point": [0, 0, 0]}, "b": {"point": dir}}}),
        );
        check(&reopened, Vec3::new(dir[0], dir[1], dir[2]));
    }
    let before = reopened.doc.clone();
    assert!(reopened.execute("joint.origin", &json!({"occurrence": occurrence, "plane": "XY", "x": x})).is_err());
    assert_eq!(reopened.doc, before, "cross-component X is refused transactionally");
}

#[test]
fn secondary_construction_axis_does_not_disappear_into_circle_point_fallback() {
    let mut s = Session::default();
    let before = s.doc.clone();
    for z in [json!([0, 0, 1]), Value::Null] {
        let p = json!({"occurrence": 0, "circle": [0, 0, 0], "z": z,
            "x": {"reference": {"type": "construction", "feature": 999}}, "name": "Bad"});
        assert!(s.execute("joint.origin", &p).is_err());
        assert_eq!(s.doc, before, "a missing axis must not leave a point origin");
    }
}

#[test]
fn secondary_construction_axis_is_not_ignored_by_unsupported_origins() {
    let mut s = Session::default();
    let x = json!({"reference": {"type": "construction", "feature": 999}});
    for primary in [
        json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1]}}),
        json!({"frame": {"origin": [0, 0, 0], "z": [0, 0, 1], "x": [1, 0, 0]}}),
        json!({"edge": [1, 2, 3], "z": [0, 0, 1]}),
        json!({"edge": [1, 2, 3]}),
    ] {
        let mut p = primary;
        p["occurrence"] = json!(0);
        p["x"] = x.clone();
        let before = s.doc.clone();
        assert!(s.execute("joint.origin", &p).is_err(), "unsupported primary silently discarded X: {p}");
        assert_eq!(s.doc, before);
    }
}

#[test]
fn secondary_construction_axis_cannot_hide_inside_a_point_vector() {
    let mut s = Session::default();
    let before = s.doc.clone();
    let p = json!({"occurrence": 0, "point": [0, 0, 0],
        "x": {"x": 1, "y": 0, "z": 0, "reference": {"type": "construction", "feature": 999}}});
    assert!(s.execute("joint.origin", &p).is_err());
    assert_eq!(s.doc, before);
    run(&mut s, "joint.origin", json!({"occurrence": 0, "point": [0, 0, 0], "x": {"x": 1, "y": 0, "z": 0}}));
}

pub(super) fn occ(s: &Session, component: &str) -> u64 {
    let c = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0)
}

pub(super) fn place(s: &Session, component: &str, p: Vec3) -> Vec3 {
    solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, occ(s, component)), p)
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

/// A knob built from a linked circle in a separate, grounded skeleton component.
fn knob_from_linked_sketch(foreign_plane: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"radius", "expression":"2 mm"}));
    run(&mut s, "component.create", json!({"name":"Base", "grounded":true}));
    run(&mut s, "solid.box", json!({"length":2, "width":2, "height":2, "body_name":"BaseBody"}));
    run(&mut s, "component.create", json!({"name":"Skeleton", "parent":"root", "grounded":true}));
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Source"}));
    run(&mut s, "sketch.circle.center", json!({"center":[10,0], "radius":2}));
    run(&mut s, "sketch.dimension", json!({"entities":["c1"], "type":"radius", "value":"radius"}));
    run(&mut s, "sketch.finish", json!({}));
    if foreign_plane {
        run(&mut s, "construct.plane.offset", json!({"base":"XY", "offset":0, "name":"SkeletonPlane"}));
    }
    run(&mut s, "component.create", json!({"name":"Knob", "parent":"root", "grounded":false}));
    let plane = if foreign_plane { "SkeletonPlane" } else { "XY" };
    run(&mut s, "sketch.create", json!({"plane":plane, "name":"Profile"}));
    run(&mut s, "sketch.project", json!({"refs":[{"sketch":"Source", "curve":"c1"}], "projection_link":true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Profile", "distance":4, "operation":"new", "body_name":"KnobBody"}));
    if foreign_plane {
        run(&mut s, "sketch.create", json!({"plane":plane, "name":"Peg"}));
        run(&mut s, "sketch.circle.center", json!({"center":[5,0], "radius":1}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch":"Peg", "distance":4, "operation":"new", "body_name":"PegBody"}));
    }
    run(
        &mut s,
        "joint.as_built",
        json!({"type":"revolute", "name":"Hinge", "a":"Base", "b":"Knob",
        "at":{"occurrence":"Base", "point":[0,0,0], "z":[0,0,1]}}),
    );
    s
}

fn check_linked_knob(s: &mut Session, center: [f64; 3], radius: f64) {
    let measured = run(s, "inspect.measure", json!({"bodies":["KnobBody"]}));
    let body = &measured["bodies"][0];
    for (i, expected) in center.into_iter().enumerate() {
        let actual = body["center_of_mass"][i].as_f64().unwrap();
        assert!((actual - expected).abs() < 1e-6, "center[{i}]: {actual} != {expected}");
    }
    let volume = body["volume_mm3"].as_f64().unwrap();
    assert!((volume - 4. * std::f64::consts::PI * radius * radius).abs() < 1e-6, "volume: {volume}");
}

#[test]
fn linked_sketch_projection_moves_with_its_joint_and_keeps_source_edits() {
    for foreign_plane in [false, true] {
        let mut s = knob_from_linked_sketch(foreign_plane);
        check_linked_knob(&mut s, [10., 0., 2.], 2.);
        let result = run(&mut s, "joint.drive", json!({"joint":"Hinge", "value":90}));
        assert_eq!(result["moved"], true);
        check_linked_knob(&mut s, [0., 10., 2.], 2.);
        run(&mut s, "parameters.change", json!({"name":"radius", "expression":"3 mm"}));
        check_linked_knob(&mut s, [0., 10., 2.], 3.);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        check_linked_knob(&mut reopened, [0., 10., 2.], 3.);
        if foreign_plane {
            let peg = run(&mut reopened, "inspect.measure", json!({"bodies":["PegBody"]}));
            let center = &peg["bodies"][0]["center_of_mass"];
            assert!(center[0].as_f64().unwrap().abs() < 1e-6, "{center}");
            assert!((center[1].as_f64().unwrap() - 5.).abs() < 1e-6, "{center}");
        }
        run(&mut reopened, "joint.drive", json!({"joint":"Hinge", "value":180}));
        check_linked_knob(&mut reopened, [-10., 0., 2.], 3.);
    }
}

#[test]
fn older_sketch_references_capture_the_saved_assembly_pose() {
    for foreign_plane in [false, true] {
        let mut s = knob_from_linked_sketch(foreign_plane);
        run(&mut s, "joint.drive", json!({"joint":"Hinge", "value":90}));
        let mut file: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
        as_last_release_saved(&mut file);
        for feature in file["features"].as_array_mut().unwrap() {
            if feature["type"] != "sketch" {
                continue;
            }
            if let Some(plane) = feature.get_mut("plane").and_then(Value::as_object_mut) {
                plane.remove("placement");
            }
            for link in feature.get_mut("sketch").and_then(|sketch| sketch.get_mut("links")).and_then(Value::as_array_mut).into_iter().flatten() {
                link.as_object_mut().unwrap().remove("sketch_placements");
            }
        }
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&file.to_string()).unwrap());
        // Older cold builds anchored the circle in world space. Preserve that saved shape,
        // then move it rigidly relative to the pose at which the old file was saved.
        check_linked_knob(&mut reopened, [10., 0., 2.], 2.);
        run(&mut reopened, "joint.drive", json!({"joint":"Hinge", "value":180}));
        check_linked_knob(&mut reopened, [0., 10., 2.], 2.);
    }
}

#[test]
fn pasting_a_captured_plane_and_its_sketch_applies_the_paste_transform_once() {
    let mut s = knob_from_linked_sketch(true);
    run(&mut s, "joint.drive", json!({"joint":"Hinge", "value":90}));
    run(&mut s, "timeline.copy", json!({"features":["SkeletonPlane", "Peg"]}));
    run(&mut s, "timeline.paste", json!({"component":"root", "translate":[2,3,0]}));
    let id = s.doc.features.last().unwrap().id;
    let check = |s: &Session| {
        let state = s.model.state();
        let ss = state.sketch(id).unwrap();
        let solidcraft_sketch::CurveKind::Circle { c, .. } = ss.sketch.curves[0].kind else { panic!("expected circle") };
        let center = ss.plane.to_world(ss.sketch.points[c].pos);
        assert!(center.dist(Vec3::new(2., 8., 0.)) < 1e-6, "{center:?}");
    };
    check(&s);
    check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
}

#[test]
fn joint_creation_rejects_reversed_limits_without_changing_the_design() {
    for command in ["joint.create", "joint.as_built"] {
        for (kind, limits) in [("revolute", json!([[45, -45]])), ("slider", json!([["2 cm", "10 mm"]])), ("cylindrical", json!([null, [5, -5]]))] {
            let mut s = Session::default();
            run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
            let mut params = if command == "joint.create" {
                json!({"a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Part", "point": [0, 0, 0]}, "flip": true})
            } else {
                json!({"a": 0, "b": "Part"})
            };
            params["type"] = json!(kind);
            params["limits"] = limits;
            let (before, undo) = (s.doc.to_json(), s.undo.len());
            let err = s.execute(command, &params).expect_err("reversed limits must be refused");
            assert!(
                matches!(err, crate::EngineError::BadParams { ref cmd, ref msg } if cmd == command && msg.contains("minimum")),
                "{command}: {err}"
            );
            assert_eq!(s.doc.to_json(), before, "{command} changed the design after rejecting limits");
            assert_eq!(s.undo.len(), undo, "{command} added an undo step after rejecting limits");
        }
    }
}

#[test]
fn joint_limits_rejects_reversed_bounds_without_changing_the_design() {
    for (kind, min, max) in [("revolute", json!(50), json!(-50)), ("slider", json!("2 cm"), json!("10 mm"))] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
        run(&mut s, "joint.as_built", json!({"type": kind, "a": 0, "b": "Part", "name": "Joint", "limits": [[-90, 90]]}));
        let (before, undo) = (s.doc.to_json(), s.undo.len());
        let err = s.execute("joint.limits", &json!({"joint": "Joint", "min": min, "max": max})).expect_err("reversed limits must be refused");
        assert!(matches!(err, crate::EngineError::BadParams { ref msg, .. } if msg.contains("minimum")), "{kind}: {err}");
        assert_eq!(s.doc.to_json(), before, "joint.limits changed the design after rejecting bounds");
        assert_eq!(s.undo.len(), undo, "joint.limits added an undo step after rejecting bounds");
    }
}

#[test]
fn joint_limits_accept_ordered_and_equal_bounds_after_unit_conversion() {
    for (kind, lo, hi, expected) in [
        ("slider", "10 mm", "2 cm", (10.0, 20.0)),
        ("slider", "1 cm", "10 mm", (10.0, 10.0)),
        ("revolute", "45 deg", "90 deg", (FRAC_PI_2 / 2.0, FRAC_PI_2)),
    ] {
        for command in ["joint.create", "joint.as_built"] {
            let mut s = Session::default();
            run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
            let mut params = if command == "joint.create" {
                json!({"a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Part", "point": [0, 0, 0]}, "flip": true})
            } else {
                json!({"a": 0, "b": "Part"})
            };
            params["type"] = json!(kind);
            params["name"] = json!("Joint");
            params["limits"] = json!([[lo, hi]]);
            run(&mut s, command, params);
            assert_eq!(s.doc.assembly.joints[0].limits, vec![Some(expected)]);
            run(&mut s, "joint.limits", json!({"joint": "Joint"}));
            assert_eq!(s.doc.assembly.joints[0].limits, vec![None]);
            run(&mut s, "joint.limits", json!({"joint": "Joint", "min": lo, "max": hi}));
            assert_eq!(s.doc.assembly.joints[0].limits, vec![Some(expected)]);
        }
    }
}

#[test]
fn deleted_joint_ids_are_not_reused_after_reopening() {
    for command in ["joint.create", "joint.as_built"] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Part"}));
        let params = if command == "joint.create" {
            json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Part", "point": [0, 0, 0]}})
        } else {
            json!({"type": "revolute", "a": 0, "b": "Part"})
        };
        let first = run(&mut s, command, params.clone())["joint"].as_u64().unwrap();
        run(&mut s, "joint.delete", json!({"joint": first}));
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        *s.doc_mut() = doc;
        let second = run(&mut s, command, params)["joint"].as_u64().unwrap();
        assert!(second > first, "{command}: deleted joint {first} was reused as {second}");
        assert!(s.execute("joint.drive", &json!({"joint": first, "value": 45})).is_err());
    }
}

#[test]
fn ambiguous_joint_names_are_refused_without_changing_the_session() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    let params = json!({"type": "revolute", "a": 0, "b": "Part", "name": "Shared"});
    let first = run(&mut s, "joint.as_built", params.clone())["joint"].as_u64().unwrap();
    let second = run(&mut s, "joint.as_built", params)["joint"].as_u64().unwrap();
    let mut s = Session::opened(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap(), None);
    run(&mut s, "component.create", json!({"name": "Temporary", "activate": false}));
    run(&mut s, "edit.undo", json!({}));
    assert!(!s.redo.is_empty());
    for (command, params) in [
        ("joint.delete", json!({"joint": "Shared"})),
        ("joint.edit", json!({"joint": "Shared", "name": "Changed"})),
        ("joint.drive", json!({"joint": "Shared", "value": 45})),
        ("selection.delete", json!({"joints": ["Shared"]})),
    ] {
        let (doc, undo, redo) = (s.doc.clone(), s.undo.len(), s.redo.len());
        let error = s.execute(command, &params).expect_err("an ambiguous name must not choose a joint");
        assert!(matches!(&error, crate::EngineError::BadParams { .. }), "{command}: {error}");
        assert!(
            error.to_string().contains(&format!("ambiguous joint `Shared`; matching IDs: {first}, {second}; use a numeric joint ID")),
            "{command}: {error}"
        );
        assert_eq!(s.doc, doc, "{command} changed the document");
        assert_eq!((s.undo.len(), s.redo.len()), (undo, redo), "{command} changed undo/redo");
    }
    run(&mut s, "joint.delete", json!({"joint": second}));
    run(&mut s, "joint.edit", json!({"joint": "Shared", "name": "Unique"}));
    assert_eq!(s.doc.assembly.joints.iter().find(|j| j.id == first).unwrap().name, "Unique");
}

#[test]
fn joint_text_references_refuse_name_and_id_collisions() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    let first = run(&mut s, "joint.as_built", json!({"type": "revolute", "a": 0, "b": "Part", "name": "4"}))["joint"].as_u64().unwrap();
    let second = run(&mut s, "joint.as_built", json!({"type": "revolute", "a": 0, "b": "Part", "name": "Target"}))["joint"].as_u64().unwrap();
    assert_eq!(second, 4);
    let doc = s.doc.clone();
    let error = s.execute("joint.delete", &json!({"joint": "4"})).unwrap_err();
    assert!(error.to_string().contains(&format!("matching IDs: {first}, {second}")), "{error}");
    assert_eq!(s.doc, doc);
    run(&mut s, "joint.delete", json!({"joint": second}));
    run(&mut s, "joint.delete", json!({"joint": "4"}));
    assert!(s.doc.assembly.joints.is_empty());
}

/// A hinge leaf (oracle 63's knuckle geometry): a 40 x 30 x 3 plate and an r4 knuckle along X
/// at y = 33, z = 4.
fn leaf(s: &mut Session, name: &str) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
    run(s, "solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
}

#[test]
fn circle_given_by_its_centre_takes_that_round_end() {
    // #393: `circle` at the centre of a round end, as the parameters document, was refused with
    // "no cylindrical face there" (no face lies at a centre).
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 6, "corner": [-15, -10, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Knob"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 6], "axis": [0, 0, 1], "radius": 10, "height": 10, "operation": "new"}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "cylindrical", "a": {"occurrence": "Plate", "face": [0, 0, 6]}, "b": {"occurrence": "Knob", "circle": [0, 0, 6]}, "name": "J"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // The knob's bottom circle already sits on the plate's top: its axis stays put (it may turn
    // about it, to line up the two frames' x).
    for z in [6.0, 16.0] {
        assert!(near(place(&s, "Knob", Vec3::new(0.0, 0.0, z)), Vec3::new(0.0, 0.0, z)), "{:?}", place(&s, "Knob", Vec3::new(0.0, 0.0, z)));
    }
    // The top end's centre: its axis points up, out of the knob. It is kept by the knob's wall
    // and the face its end opens onto, so it follows them.
    run(&mut s, "joint.origin", json!({"occurrence": "Knob", "circle": [0, 0, 16], "name": "Top"}));
    let top = s.doc.assembly.origins.iter().find(|o| o.name == "Top").map(|o| o.origin.clone()).unwrap();
    assert!(matches!(&top.snap, solidcraft_doc::joints::Snap::CircleCenter { face: Some(_), end: Some(_), .. }), "{top:?}");
    let m = solidcraft_doc::joints::resolve_origin(&s.doc, &s.model.state(), &top).unwrap();
    assert!(near(Vec3::new(m[3][0], m[3][1], m[3][2]), Vec3::new(0.0, 0.0, 16.0)) && near(Vec3::new(m[2][0], m[2][1], m[2][2]), Vec3::Z), "{m:?}");
    // Off the axis is still refused; a coupling size given by a centre is that round face's
    // radius (it follows the face).
    assert!(s.execute("joint.origin", &json!({"occurrence": "Knob", "circle": [3, 0, 6]})).is_err());
    run(&mut s, "joint.screw", json!({"joint": "J", "pitch": {"occurrence": "Knob", "circle": [0, 0, 6]}}));
}

#[test]
fn revolute_hinge_turns_about_the_knuckle() {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    let r = run(
        &mut s,
        "joint.create",
        json!({
        "type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // Same frames: B stays put at angle 0.
    assert!(near(place(&s, "LeafB", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 0.0)));
    let l = run(&mut s, "joint.list", json!({}));
    let dof = l["dof"].as_array().unwrap().iter().find(|d| d["occurrence"] == occ(&s, "LeafB")).unwrap()["dof"].clone();
    assert_eq!(dof, 1, "{l}");
    // Drive 90°: B turns about the knuckle axis (through (0, 33, 4), along the frame's z = −X).
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let want = solidcraft_doc::rigid(Vec3::ZERO, Vec3::new(0.0, 33.0, 4.0), Vec3::new(-1.0, 0.0, 0.0), FRAC_PI_2);
    let p = Vec3::new(20.0, 0.0, 0.0);
    assert!(near(place(&s, "LeafB", p), solidcraft_doc::apply_point(&want, p)), "{:?}", place(&s, "LeafB", p));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));
    // LeafA did not move; the knuckle axis is fixed.
    assert!(near(place(&s, "LeafA", p), p));
    assert!(near(place(&s, "LeafB", Vec3::new(10.0, 33.0, 4.0)), Vec3::new(10.0, 33.0, 4.0)));
    // Expressions drive too, and undo puts it back.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": "pi rad"}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 66.0, 8.0)));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));
}

/// A joint value too large says so, rather than that it is not a number (#9625).
#[test]
fn a_joint_value_out_of_range_says_so() {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    let hinge = json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"});
    run(&mut s, "joint.create", hinge);
    for value in [json!(1e9), json!(-5e15)] {
        let e = s.execute("joint.drive", &json!({"joint": "Hinge", "value": value})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            e.contains("a joint value must be a number between -1e9 and 1e9") && !e.contains("a joint value must be a number or"),
            "{value}: {e}"
        );
    }
    let e = s.execute("joint.drive", &json!({"joint": "Hinge", "value": true})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("a joint value must be a number or an expression") && !e.contains("between"), "{e}");
}

/// Two 10 mm cubes in components A (grounded) and B.
fn two_cubes() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let o = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    s
}

#[test]
fn unground_reports_a_joint_whose_origin_was_removed() {
    let mut s = two_cubes();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    run(&mut s, "solid.box", json!({"length": 6, "width": 6, "height": 3, "corner": [22, 2, -2], "operation": "join", "name": "Foot"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let joint = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Foot joint", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "B", "face": [25, 5, -2]}}),
    )["joint"]
        .clone();
    run(&mut s, "timeline.suppress", json!({"feature": "Foot"}));
    let before = s.doc.occurrences.iter().find(|o| o.id == b).unwrap().transform;
    let reply = run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": false}));
    assert_eq!(reply["grounded"], false);
    assert_eq!(reply["errors"][0]["joint"], joint, "{reply}");
    assert_eq!(reply["errors"][0]["name"], "Foot joint", "{reply}");
    assert!(reply["errors"][0]["why"].as_str().unwrap().contains("no longer exists"), "{reply}");
    assert_eq!(reply["moved"], json!([]), "{reply}");
    assert_eq!(reply["transform"], json!(before), "{reply}");
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b).unwrap().transform, before);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.occurrences.iter().find(|o| o.id == b).unwrap().grounded);
}

#[test]
fn unground_reports_the_solved_placement_and_moves() {
    let mut s = two_cubes();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "B", "face": [25, 5, 0]}}));
    super::solve_counts::take();
    let reply = run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": false}));
    assert_eq!(reply["moved"], json!(["B:1"]), "{reply}");
    assert_eq!(reply["errors"], json!([]), "{reply}");
    assert_eq!(reply["conflicts"], json!([]), "{reply}");
    assert_eq!(reply["transform"], json!(s.doc.occurrences.iter().find(|o| o.id == b).unwrap().transform));
    assert!(near(place(&s, "B", Vec3::new(25., 5., 0.)), Vec3::new(5., 5., 10.)));
    assert_eq!(super::solve_counts::take(), (1, 0), "unground and refresh share one placement solve");
    // An already placed part does not need to move. That is not a failed placement.
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    let settled = run(&mut s, "occurrence.ground", json!({"occurrence": b}));
    assert_eq!(settled["moved"], json!([]), "{settled}");
    assert_eq!(settled["errors"], json!([]), "{settled}");
    assert_eq!(settled["conflicts"], json!([]), "{settled}");
    assert_eq!(settled["transform"], reply["transform"]);
}

#[test]
fn slider_limits_clamp() {
    let mut s = two_cubes();
    // B's bottom face onto A's top face (faces meet), sliding along their common normal.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "limits": [[0, 10]], "name": "Rail"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // B now sits on A.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    assert_eq!((&r["values"][0]["value"], &r["values"][0]["unit"]), (&json!(10.0), &json!("mm")), "{r}");
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 20.0)));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
    // Changing the limits clamps the current value.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 8}));
    run(&mut s, "joint.limits", json!({"joint": "Rail", "min": 0, "max": 4}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 14.0)));
    // Joints follow edits: A gets taller, B rides on its new top.
    let h = s
        .doc
        .features
        .iter()
        .find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. }))
        .and_then(|f| f.param_names.get(2).cloned())
        .unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": h, "expression": "15"}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 19.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
}

/// X and Y remain separate from A's saved snap, so a reopened joint can edit either one.
#[test]
fn lateral_joint_offsets_reopen_edit_independently_and_undo() {
    let mut s = two_cubes();
    let result = run(
        &mut s,
        "joint.create",
        json!({"type":"rigid", "name":"Offset joint", "flip":true, "angle":0,
            "a":{"occurrence":"A", "point":[5,5,10], "z":[0,0,1], "x":[1,0,0]},
            "b":{"occurrence":"B", "point":[25,5,0], "z":[0,0,1], "x":[1,0,0]},
            "offset_x":"10 mm", "offset_y":"30 mm", "offset":30}),
    );
    assert_eq!(result["errors"], json!([]), "{result}");
    let original_a = s.doc.assembly.joints[0].a.clone();
    let probe = Vec3::new(25.0, 5.0, 0.0);
    assert!(near(place(&s, "B", probe), Vec3::new(15.0, 35.0, 40.0)));
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let mut reopened = Session::new(doc);
    assert_eq!(reopened.doc.assembly.joints[0].offset_xy, [10.0, 30.0]);
    let before = reopened.doc.assembly.joints[0].clone();
    run(&mut reopened, "joint.edit", json!({"joint":"Offset joint", "offset_x":"-2 mm"}));
    assert_eq!(reopened.doc.assembly.joints[0].offset_xy, [-2.0, 30.0]);
    assert_eq!(reopened.doc.assembly.joints[0].a, original_a);
    assert!(near(place(&reopened, "B", probe), Vec3::new(3.0, 35.0, 40.0)));
    for command in ["joint.list", "joint.inspect"] {
        let report = run(&mut reopened, command, json!({}));
        assert_eq!(report["joints"][0]["offset_xy"], json!([-2.0, 30.0]), "{report}");
        let frame = if command == "joint.list" { &report["joints"][0]["frame_a"] } else { &report["joints"][0]["a"]["frame"] };
        assert_eq!(frame["origin"], json!([3.0, 35.0, 10.0]), "{report}");
    }
    run(&mut reopened, "edit.undo", json!({}));
    assert_eq!(reopened.doc.assembly.joints[0], before);
    assert!(near(place(&reopened, "B", probe), Vec3::new(15.0, 35.0, 40.0)));
    run(&mut reopened, "joint.edit", json!({"joint":"Offset joint", "offset_y":0}));
    assert_eq!(reopened.doc.assembly.joints[0].offset_xy, [10.0, 0.0]);
    assert!(near(place(&reopened, "B", probe), Vec3::new(15.0, 5.0, 40.0)));
}

/// A rotated attachment uses its own X/Y axes, and turning a hinge leaves its offset pivot fixed.
#[test]
fn lateral_joint_offsets_follow_the_attachment_axes_and_keep_the_hinge_pivot() {
    let mut s = two_cubes();
    run(
        &mut s,
        "joint.create",
        json!({"type":"revolute", "name":"Hinge", "flip":true, "angle":0,
            "a":{"occurrence":"A", "point":[5,5,10], "z":[1,0,0], "x":[0,0,1]},
            "b":{"occurrence":"B", "point":[25,5,0], "z":[1,0,0], "x":[0,0,1]},
            "offset_x":2, "offset_y":3, "offset":4}),
    );
    // A's local x is world +Z, its y is world -Y, and its z is world +X.
    let pivot = Vec3::new(9.0, 2.0, 12.0);
    let point = Vec3::new(25.0, 5.0, 0.0);
    assert!(near(place(&s, "B", point), pivot), "{:?}", place(&s, "B", point));
    let before = place(&s, "B", point + Vec3::Z) - pivot;
    run(&mut s, "joint.drive", json!({"joint":"Hinge", "value":90}));
    assert!(near(place(&s, "B", point), pivot), "the lateral pivot must not orbit");
    let after = place(&s, "B", point + Vec3::Z) - pivot;
    assert!((before.len() - 1.0).abs() < 1e-9 && (after.len() - 1.0).abs() < 1e-9);
    assert!(before.dot(after).abs() < 1e-9, "the off-axis point turns 90 degrees");
}

/// Older files saved X/Y in an explicit A frame and have no recoverable separate offset values.
#[test]
fn legacy_joint_frames_keep_their_baked_lateral_position() {
    let mut s = two_cubes();
    run(
        &mut s,
        "joint.create",
        json!({"type":"rigid", "name":"Legacy", "flip":true, "angle":0,
            "a":{"occurrence":"A", "frame":{"origin":[15,35,10],"z":[0,0,1],"x":[1,0,0]}},
            "b":{"occurrence":"B", "point":[25,5,0], "z":[0,0,1], "x":[1,0,0]}, "offset":30}),
    );
    let before = s.doc.assembly.joints[0].clone();
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    saved["assembly"]["joints"][0].as_object_mut().unwrap().remove("offset_xy");
    let doc = solidcraft_doc::Document::from_json(&saved.to_string()).unwrap();
    let mut reopened = Session::new(doc);
    assert_eq!(reopened.doc.assembly.joints[0], before);
    run(&mut reopened, "joint.edit", json!({"joint":"Legacy", "offset_x":0,"offset_y":0,"offset":30}));
    assert_eq!(reopened.doc.assembly.joints[0].a, before.a);
    assert!(near(place(&reopened, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(15.0, 35.0, 40.0)));
}

/// An invalid lateral value must refuse the entire edit, including other valid changes.
#[test]
fn invalid_lateral_joint_offsets_leave_the_design_unchanged() {
    let mut s = two_cubes();
    let params = json!({"type":"rigid", "name":"Seat", "a":{"occurrence":"A","point":[0,0,0]},
        "b":{"occurrence":"B","point":[20,0,0]}, "offset_x":2,"offset_y":3});
    run(&mut s, "joint.create", params.clone());
    for key in ["offset_x", "offset_y"] {
        for value in [json!(null), json!(true), json!([]), json!({}), json!(1e9), json!("1e200 mm"), json!("30 deg"), json!("missing_parameter")] {
            let before = s.doc.to_json();
            let mut edit = json!({"joint":"Seat", "name":"Wrong", "offset":12});
            edit[key] = value.clone();
            assert!(s.execute("joint.edit", &edit).is_err(), "{edit}");
            assert_eq!(s.doc.to_json(), before, "failed edit changed the design");
            let mut create = params.clone();
            create[key] = value;
            assert!(s.execute("joint.create", &create).is_err(), "{create}");
            assert_eq!(s.doc.to_json(), before, "failed creation changed the design");
        }
    }
}

/// A part on a revolute joint, nested in the part a slider moves, moves as far as the slider
/// does (#17305). The part is made before the slider's part and then moved into it, so its
/// occurrence has the lower id. A solve stored the placements in id order: the part in the frame
/// its parent was leaving, then the parent, which moved the part a second time. A slider whose
/// offset follows a parameter shows it in one solve: changing the parameter by 10 mm moved the
/// slider's part 10 mm and the part inside it 20 mm.
#[test]
fn a_part_nested_in_a_slider_part_moves_as_far_as_the_slider() {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("C", 40), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let (a, b, c) = (occ(&s, "A"), occ(&s, "B"), occ(&s, "C"));
    assert!(c < b, "C's occurrence {c} is older than B's {b}");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "occurrence.reparent", json!({"occurrence": c, "parent": "B"}));
    // B's bottom onto A's top, sliding up and down; C rides inside B and moves with it.
    let r = run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let (corner_b, corner_c) = (Vec3::new(20.0, 0.0, 0.0), Vec3::new(40.0, 0.0, 0.0));
    assert!(near(place(&s, "C", corner_c), Vec3::new(20.0, 0.0, 10.0)), "C follows B onto A: {:?}", place(&s, "C", corner_c));
    // C's bottom onto B's top, turning about their common normal.
    let r = run(&mut s, "joint.create", json!({"type": "revolute", "a": {"face": [5, 5, 20]}, "b": {"face": [25, 5, 10]}, "name": "Turn"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "5 mm"}));
    run(&mut s, "joint.edit", json!({"joint": "Rail", "offset": "lift"}));
    let moved = |s: &Session, b0: Vec3, c0: Vec3| (place(s, "B", corner_b) - b0, place(s, "C", corner_c) - c0);
    for (lift, by) in [("15 mm", 10.0), ("1 mm", -14.0), ("31 mm", 30.0)] {
        let (b0, c0) = (place(&s, "B", corner_b), place(&s, "C", corner_c));
        run(&mut s, "parameters.change", json!({"name": "lift", "expression": lift}));
        let (db, dc) = moved(&s, b0, c0);
        assert!(near(db, Vec3::new(0.0, 0.0, by)), "lift {lift}: B moves with the slider by {by}: {db:?}");
        assert!(near(dc, db), "lift {lift}: C moves as far as B ({db:?}), not {dc:?}");
    }
    // Driving the slider moves both by the same distance too.
    for travel in [10.0, -4.0] {
        let (b0, c0) = (place(&s, "B", corner_b), place(&s, "C", corner_c));
        let now =
            solidcraft_doc::joints::joint_values(&s.doc.assembly).get(&s.doc.assembly.joints[0].id).and_then(|q| q.first().copied()).unwrap_or(0.0);
        run(&mut s, "joint.drive", json!({"joint": "Rail", "value": now + travel}));
        let (db, dc) = moved(&s, b0, c0);
        assert!(near(db, Vec3::new(0.0, 0.0, travel)) && near(dc, db), "drive by {travel}: B {db:?}, C {dc:?}");
    }
}

/// The box an occurrence is shown in (`occurrence.properties`, which reads a move not captured).
fn shown_box(s: &mut Session, component: &str) -> [Vec3; 2] {
    let id = occ(s, component);
    let r = run(s, "occurrence.properties", json!({"occurrence": id}));
    let corner =
        |k: &str| serde_json::from_value::<[f64; 3]>(r["total"]["bbox"][k].clone()).map_or(Vec3::new(f64::NAN, f64::NAN, f64::NAN), Vec3::from);
    [corner("min"), corner("max")]
}

fn shown_at(s: &mut Session, component: &str, min: [f64; 3], max: [f64; 3]) -> bool {
    let [lo, hi] = shown_box(s, component);
    near(lo, Vec3::from(min)) && near(hi, Vec3::from(max))
}

/// A joint solve that moves a part drops the part's move not captured, so the part is shown where
/// the joints put it; an exploded view's placement stays shown.
#[test]
fn a_joint_solve_drops_the_move_not_captured_of_a_part_it_moves() {
    let mut s = two_cubes();
    let b = occ(&s, "B");
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": [0, 30, 0], "capture": false}));
    assert!(shown_at(&mut s, "B", [20.0, 30.0, 0.0], [30.0, 40.0, 10.0]), "{:?}", shown_box(&mut s, "B"));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "B", "face": [25, 35, 0]}, "name": "Stack"}),
    );
    assert!(shown_at(&mut s, "B", [0.0, 0.0, 10.0], [10.0, 10.0, 20.0]), "{:?}", shown_box(&mut s, "B"));
    assert!(!s.pending_moves.contains_key(&b));
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": true}));
    run(&mut s, "explode.create", json!({"name": "Apart", "moves": [{"occurrence": "B", "translate": [0, 0, 20]}]}));
    run(&mut s, "explode.show", json!({"view": "Apart"}));
    let shown = s.pending_moves.get(&b).copied();
    assert!(shown.is_some() && shown == s.exploded.get(&b).copied());
    let mut doc = (*s.doc).clone();
    if let Some(o) = doc.occurrences.iter_mut().find(|o| o.id == b) {
        o.transform = solidcraft_doc::rigid(Vec3::new(40.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
    }
    *s.doc_mut() = doc;
    run(&mut s, "occurrence.ground", json!({"occurrence": b, "grounded": false}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)), "the solve puts B back on A");
    assert_eq!(s.pending_moves.get(&b).copied(), shown);
}

#[test]
fn capture_position_saves_joint_values_and_revert_position_puts_them_back() {
    // #14275: Capture Position also saves every joint's values; Revert Position puts them back.
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}));
    let b = Vec3::new(25.0, 5.0, 0.0);
    // Not driven yet: Revert Position leaves the joint where it is.
    let r = run(&mut s, "component.revert_position", json!({}));
    assert_eq!(r["joints"], json!(0), "{r}");
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 10.0)));
    // Driven before first capture: Revert Position puts it back to its pre-drive baseline.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 13.0)));
    let r = run(&mut s, "component.revert_position", json!({}));
    assert_eq!(r["joints"], json!(1), "{r}");
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 10.0)));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 13.0)));
    // Capture at 3 mm, drive away, and revert.
    let r = run(&mut s, "component.capture_position", json!({}));
    assert_eq!(r["joints"], json!(1), "{r}");
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![3.0]));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 9}));
    let r = run(&mut s, "component.revert_position", json!({}));
    assert_eq!(r["joints"], json!(1), "{r}");
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 13.0)));
    // One undo step puts it back where it was driven.
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 19.0)));
    // The captured values are saved with the design.
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    *s.doc_mut() = doc;
    run(&mut s, "joint.solve", json!({}));
    run(&mut s, "component.revert_position", json!({}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 13.0)));
}

#[test]
fn a_design_without_captured_joint_values_loads_unchanged() {
    // A file saved before captured joint values has no `captured`: it loads with the same values,
    // and a joint with none saves without the key.
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 4}));
    let json = s.doc.to_json();
    assert!(!json.contains("\"captured\""), "no captured key before Capture Position");
    let doc = solidcraft_doc::Document::from_json(&json).unwrap();
    assert_eq!(doc.assembly.joints[0].captured, None);
    assert_eq!(doc.assembly.joints[0].values, s.doc.assembly.joints[0].values);
}

#[test]
fn rigid_cycles_solve_and_conflicts_report() {
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    // A rigid group and a closing as-built joint: a consistent loop.
    run(&mut s, "joint.rigid_group", json!({"occurrences": ["A", "B", "C"]}));
    let r = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "B", "b": "C"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "C", Vec3::new(40.0, 0.0, 0.0)), Vec3::new(40.0, 0.0, 0.0)));
    let l = run(&mut s, "joint.list", json!({}));
    assert!(l["dof"].as_array().unwrap().iter().all(|d| d["dof"] == 0), "{l}");
    // A joint that wants C elsewhere conflicts and says so.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "A", "point": [0, 0, 0]}, "b": {"occurrence": "C", "point": [40, 0, 0]}, "name": "Wrong"}),
    );
    let c = r["conflicts"].as_array().cloned().unwrap_or_default();
    assert_eq!(c.len(), 1, "{r}");
    assert!(c[0]["why"].as_str().unwrap_or("").contains("off by"), "{r}");
    // A rigid joint has nothing to drive: the refusal names the types that move (#6981).
    let e = s.execute("joint.drive", &json!({"joint": "Wrong", "value": 1})).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("does not move") && e.contains("joint.edit") && e.contains("revolute") && e.contains("cylindrical"), "{e}");
    // Suppressing it clears the conflict.
    let r = run(&mut s, "joint.edit", json!({"joint": "Wrong", "suppressed": true}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0));
    assert!(
        s.execute(
            "joint.create",
            &json!({"type": "rigid", "a": {"occurrence": "A", "point": [0, 0, 0]}, "b": {"occurrence": "A", "point": [1, 0, 0]}})
        )
        .is_err()
    );
    assert!(s.execute("joint.create", &json!({"type": "hinge"})).is_err());
}

/// A 30 x 30 x 3 plate with a through slot whose x = 20 wall carries one rib (0.3 tall, 0.8
/// wide), and a peg in the slot with 0.15 clearance all round.
fn ribbed_slot_and_peg(s: &mut Session, peg_x1: f64) {
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 30]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 3, "body_name": "Plate"}));
    run(s, "sketch.create", json!({"plane": "XY", "offset": -1}));
    let slot = json!([[10, 5], [20, 5], [20, 14.6], [19.7, 14.6], [19.7, 15.4], [20, 15.4], [20, 25], [10, 25]]);
    run(s, "sketch.line", json!({"points": slot, "closed": true}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 5, "operation": "cut", "targets": ["Plate"]}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [10.15, 5.15], "p1": [peg_x1, 24.85]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 3, "operation": "new", "body_name": "Peg"}));
}

/// The peg overlaps only the rib: 0.15 x 0.8 x 3 = 0.36 mm³ (#442).
#[test]
fn interference_finds_a_peg_overlapping_a_small_rib() {
    let mut s = Session::default();
    ribbed_slot_and_peg(&mut s, 19.85);
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert!((i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0) - 0.36).abs() < 1e-6, "{i}");
    let (lo, hi) = (&i["interferences"][0]["bbox"]["min"], &i["interferences"][0]["bbox"]["max"]);
    assert!((lo[0].as_f64().unwrap_or(0.0) - 19.7).abs() < 1e-6 && (hi[0].as_f64().unwrap_or(0.0) - 19.85).abs() < 1e-6, "{i}");
}

/// A pair interference can't check is listed as unchecked, with its reason, instead of passing
/// as clear. Truck could not intersect an arc revolved up to a post's wall; SMLib does, exactly,
/// so the pair that can't be checked is now an imported mesh beside a solid.
#[test]
fn interference_lists_a_pair_it_could_not_check() {
    // A quarter ring revolved round z through the post, brought in as an imported mesh (an STL
    // of it): a mesh body has no exact surfaces to intersect, so its pair with the post is listed
    // as not checked, with the reason, rather than read as clear.
    let dir = std::env::temp_dir().join(format!("solidcraft-unchecked-pair-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("arc.stl");
    let mut source = Session::default();
    run(&mut source, "sketch.create", json!({"name": "Profile", "plane": "XZ"}));
    run(&mut source, "sketch.rectangle.two_point", json!({"p0": [10, 0], "p1": [15, 5]}));
    run(&mut source, "sketch.finish", json!({}));
    run(
        &mut source,
        "solid.revolve",
        json!({"sketch": "Profile", "axis": {"origin": [0, 0, 0], "dir": [0, 0, 1]}, "angle": "90 deg", "body_name": "Arc"}),
    );
    run(&mut source, "file.export", json!({"path": stl.to_string_lossy()}));
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 14, -5], "radius": 4, "height": 15, "body_name": "Post"}));
    run(&mut s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
    let _ = std::fs::remove_dir_all(&dir);
    let mesh: Vec<String> = s.world_state().unwrap().bodies.iter().filter(|b| b.body.is_mesh()).map(|b| b.name.clone()).collect();
    assert_eq!(mesh.len(), 1, "{mesh:?}");
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 0, "{i}");
    let unchecked = i["unchecked"].as_array().cloned().unwrap_or_default();
    assert_eq!(unchecked.len(), 1, "{i}");
    assert_eq!(unchecked[0]["a"].as_str(), mesh.first().map(String::as_str), "{i}");
    assert_eq!(unchecked[0]["reason"], "mesh body: not checked", "{i}");
    // Bodies that are clear of each other, or overlap plainly, leave the list empty.
    let mut s = two_cubes();
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
}

#[test]
fn motion_links_and_interference() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Screw"}));
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, 0]}));
    // No overlap while B sits on A; then push B 4 mm into A.
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 0, "{i}");
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, -4]}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert!((i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0) - 400.0).abs() < 1e-3, "{i}");
    // A motion link: a second joint's value follows.
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "D"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0]}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "D", "face": [65, 5, 0]}, "name": "Follower"}),
    );
    run(&mut s, "joint.motion_link", json!({"a": "Screw", "ia": 1, "b": "Follower", "ratio": 2}));
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, 3]}));
    assert!(near(place(&s, "D", Vec3::new(65.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "D", Vec3::new(65.0, 5.0, 0.0)));
    // Saved and reopened, joints come back.
    let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.assembly, s.doc.assembly);
    let mut t = Session::new(back);
    let l = run(&mut t, "joint.list", json!({}));
    assert_eq!(l["joints"].as_array().map(Vec::len), Some(2));
}

/// Two faces joined mate: B's bottom face lands on A's top face, B on top of A (not inside
/// it); Flip turns B over instead.
#[test]
fn faces_mate_by_default() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Stack"}));
    // B's bottom centre (25, 5, 0) at A's top centre, B above it.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 20.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    run(&mut s, "joint.edit", json!({"joint": "Stack", "flip": true}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 0.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
}

/// Edit Joint takes new sides as joint.create does: the joint moves its attachment to them, and
/// keeps its limits, its drive and the expression its offset follows.
#[test]
fn edit_moves_a_side_and_keeps_limits_and_expressions() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "2 mm"}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "name": "Rail", "offset": "gap", "limits": [[0, 10]], "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}),
    );
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 4}));
    // B's bottom centre rides 2 mm (the gap) plus 4 mm (the drive) above A's top centre.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
    // B's top centre, where B is placed now, is the new side (`face` is a world point).
    let top = place(&s, "B", Vec3::new(25.0, 5.0, 10.0));
    let r = run(&mut s, "joint.edit", json!({"joint": "Rail", "b": {"occurrence": "B", "face": [top.x, top.y, top.z]}}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    let j = s.doc.assembly.joints.first().cloned().unwrap_or_else(|| panic!("no joint"));
    assert_eq!(j.limits, vec![Some((0.0, 10.0))]);
    assert_eq!(j.exprs.offset.as_deref(), Some("gap"));
    assert_eq!(j.values, vec![4.0]);
    assert_eq!(j.name, "Rail");
}

/// Sides left out of joint.edit stay as they are; the other parameters still change.
#[test]
fn omitted_sides_stay() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    let before = s.doc.assembly.joints[0].clone();
    run(&mut s, "joint.edit", json!({"joint": "Stack", "flip": true, "name": "Flipped"}));
    let after = &s.doc.assembly.joints[0];
    assert_eq!((&after.a, &after.b), (&before.a, &before.b));
    assert!(after.flip && after.name == "Flipped");
}

/// A joint's new name counts characters, as the other renames do: a hundred `ł` (200 bytes) is
/// taken. A new name over 128 characters is refused with the reason and changes nothing, not
/// dropped while the rest of the edit goes through. joint.create still takes any name, and a
/// joint keeps its own name through an edit whatever its length; a blank name changes nothing.
#[test]
fn joint_names_count_characters_and_a_new_long_one_is_refused() {
    let mut s = two_cubes();
    let given = "x".repeat(200);
    run(&mut s, "joint.create", json!({"type": "rigid", "name": given, "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    assert_eq!(s.doc.assembly.joints[0].name, given);
    run(&mut s, "joint.edit", json!({"joint": given, "flip": true, "name": given}));
    assert!(s.doc.assembly.joints[0].flip, "the edit with the joint's own name goes through");
    run(&mut s, "joint.edit", json!({"joint": given, "name": " "}));
    assert_eq!(s.doc.assembly.joints[0].name, given, "a blank name changes nothing");
    // A stored name with spaces round it is the joint's own name too.
    let spaced = format!(" {given} ");
    s.doc_mut().assembly.joints[0].name = spaced.clone();
    let id = s.doc.assembly.joints[0].id;
    run(&mut s, "joint.edit", json!({"joint": id, "flip": false, "name": given}));
    assert_eq!(s.doc.assembly.joints[0].name, spaced, "kept as it is");
    let long = "ł".repeat(100);
    run(&mut s, "joint.edit", json!({"joint": id, "name": long}));
    assert_eq!(s.doc.assembly.joints[0].name, long);
    let doc = s.doc.clone();
    let error = s.execute("joint.edit", &json!({"joint": long, "flip": false, "name": "ł".repeat(129)})).unwrap_err().to_string();
    assert!(error.contains("joint.edit") && error.contains("at most 128 characters"), "{error}");
    assert_eq!(s.doc, doc, "a refused name changes nothing");
}

/// One undo step puts the sides back, and the parts with them.
#[test]
fn one_undo_restores_the_old_sides() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    let before = s.doc.assembly.joints[0].clone();
    let top = place(&s, "B", Vec3::new(25.0, 5.0, 10.0));
    run(&mut s, "joint.edit", json!({"joint": "Stack", "b": {"occurrence": "B", "face": [top.x, top.y, top.z]}}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 10.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.joints[0], before);
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
}

/// Moving a side re-solves the joint as joint.create would at the same pose: the turn B keeps
/// about the joint's axis follows its new origin, so B does not turn by a half turn (or any turn
/// a fresh create would not give). Both designs start from the same pose; one moves the side,
/// the other deletes the joint and creates it with the new sides.
#[test]
fn moving_a_side_places_the_part_as_create_would() {
    let a_faces = [[5, 5, 10], [5, 0, 5], [5, 10, 5], [10, 5, 5], [0, 5, 5]];
    let b_faces = [[25, 5, 0], [25, 5, 10], [25, 0, 5], [25, 10, 5], [20, 5, 5], [30, 5, 5]];
    // A point of B off the axes of every face above.
    let probe = |s: &Session| place(s, "B", Vec3::new(22.0, 3.0, 7.0));
    let mut differ = Vec::new();
    for a in a_faces {
        for b in b_faces {
            let mut edited = two_cubes();
            run(&mut edited, "joint.create", json!({"type": "rigid", "name": "J", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
            let mut created = Session::new(solidcraft_doc::Document::from_json(&edited.doc.to_json()).unwrap());
            // The new sides in world coordinates, where B is before the move.
            let (aw, bw) = (Vec3::new(a[0] as f64, a[1] as f64, a[2] as f64), place(&edited, "B", Vec3::new(b[0] as f64, b[1] as f64, b[2] as f64)));
            let side = |occurrence: &str, p: Vec3| json!({"occurrence": occurrence, "face": [p.x, p.y, p.z]});
            run(&mut edited, "joint.edit", json!({"joint": "J", "a": side("A", aw), "b": side("B", bw)}));
            run(&mut created, "joint.delete", json!({"joint": "J"}));
            run(&mut created, "joint.create", json!({"type": "rigid", "name": "J", "a": side("A", aw), "b": side("B", bw)}));
            if !near(probe(&edited), probe(&created)) {
                differ.push((a, b, probe(&edited), probe(&created)));
            }
        }
    }
    assert!(differ.is_empty(), "moved sides place B differently from create: {differ:?}");
}

/// A side that makes no joint is refused, and the joint is left as it was: a side on the other
/// side's component, a side that is not a joint origin, an unknown occurrence.
#[test]
fn an_invalid_side_is_refused_and_the_joint_is_kept() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "name": "Stack", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}}));
    let before = s.doc.assembly.joints[0].clone();
    // A is a's occurrence and B is b's: each side is tried on the other's occurrence, and with
    // the faults every side can have.
    let cases = [
        ("a", json!({"occurrence": "B", "face": [25, 5, 10]})),
        ("a", json!({"occurrence": "Nope", "face": [5, 5, 10]})),
        ("a", json!({"occurrence": "A"})),
        ("a", json!(5)),
        ("b", json!({"occurrence": "A", "face": [5, 5, 0]})),
        ("b", json!({"occurrence": "Nope", "face": [25, 5, 0]})),
        ("b", json!({"occurrence": "B"})),
        ("b", json!("NoSuchOrigin")),
    ];
    for (key, side) in cases {
        let r = s.execute("joint.edit", &json!({"joint": "Stack", key: side.clone()}));
        assert!(r.is_err(), "{key}: {side} should be refused");
        assert_eq!(s.doc.assembly.joints[0], before, "{key}: {side} changed the joint");
    }
}

/// A move of a jointed part that the joint takes back: the model is evaluated for the
/// placements the solve leaves, so a mirror of the part's body, made from the root, is where a
/// fresh evaluation of the design puts it. The model was the one evaluated for the move before
/// the solve took it back, and its mirror didn't match the part.
#[test]
fn a_feature_reading_a_jointed_parts_placement_follows_the_solve() {
    let mut s = Session::default();
    for (cmd, p) in [
        ("component.create", json!({"grounded": true, "name": "Base"})),
        ("solid.box", json!({"corner": [-11, -25, 0], "height": 9, "length": 11, "width": 14})),
        ("component.create", json!({"grounded": false, "name": "Slide"})),
        ("solid.cylinder", json!({"base": [-2, -10, 7], "height": 7, "radius": 2})),
        ("component.activate", json!({"component": "root"})),
        ("joint.as_built", json!({"a": "Base:1", "b": "Slide:1", "name": "Rail", "type": "slider"})),
        ("solid.mirror", json!({"bodies": ["Body2"], "plane": "XY"})),
        ("occurrence.move", json!({"occurrence": "Slide:1", "translate": [-10, -18, 2]})),
    ] {
        s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
    }
    let boxes = |st: &solidcraft_doc::ModelState| {
        st.bodies
            .iter()
            .map(|b| (b.name.clone(), b.body.outline_bounds()))
            .map(|(n, bb)| format!("{n} {:.6?} {:.6?}", bb.min, bb.max))
            .collect::<Vec<_>>()
    };
    let mut fresh = solidcraft_doc::Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&s.doc);
    assert_eq!(boxes(&s.model.state()), boxes(&fresh.state()));
}

/// A drawer post built To Object up to the frame's roof (#16732): a Roof component, and a Drawer
/// whose Post runs from z 0 to the roof's underside at z 30, on a slider along z.
fn post_to_the_roof() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Frame", "grounded": true}));
    run(&mut s, "solid.box", json!({"corner": [0, 0, 30], "length": 40, "width": 40, "height": 10, "body_name": "Roof"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Drawer", "grounded": false}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [10, 10, 30], "body_name": "Post"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let end = |occurrence: &str| json!({"occurrence": occurrence, "point": [10, 10, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "slider", "a": end("Frame"), "b": end("Drawer"), "flip": true, "name": "Rail"}));
    s
}

/// The post's volume and z range, as the world shows it.
fn post_extent(s: &mut Session) -> (f64, f64, f64) {
    let volume = s
        .model
        .state()
        .bodies
        .iter()
        .find(|b| b.name == "Post")
        .and_then(|b| solidcraft_kernel::measure(&b.body).ok())
        .map_or(f64::NAN, |m| m.volume);
    let m = run(s, "inspect.measure", json!({"bodies": ["Post"]}));
    let z = |end: &str| m["bodies"][0]["bbox"][end][2].as_f64().unwrap_or(f64::NAN);
    (volume, z("min"), z("max"))
}

/// #16732: sliding a part whose body was built up to another component's face moves the body as
/// it is; it does not stay anchored to the face and stretch.
#[test]
fn a_to_object_extrude_to_another_component_moves_rigidly_with_its_slider() {
    let mut s = post_to_the_roof();
    let (volume, low, high) = post_extent(&mut s);
    assert!((volume - 3000.0).abs() < 1e-6 && low.abs() < 1e-6 && (high - 30.0).abs() < 1e-6, "{volume} {low} {high}");
    for value in [5.0, 10.0, 20.0] {
        run(&mut s, "joint.drive", json!({"joint": "Rail", "value": value}));
        let (volume, low, high) = post_extent(&mut s);
        assert!((volume - 3000.0).abs() < 1e-6, "volume at {value}: {volume}");
        assert!((low - value).abs() < 1e-6 && (high - 30.0 - value).abs() < 1e-6, "z range at {value}: {low}..{high}");
    }
}

/// The design as it was saved before the format that pins a To Object extrude's placements
/// (format 57, `v57/extrude-to-object-keeps-picked-placements.rs`): that step runs as it opens,
/// whatever changes are pending after it, and no list of the changes it has says otherwise.
fn as_last_release_saved(file: &mut Value) {
    file["format"] = json!("solidcraft/56");
    if let Some(o) = file.as_object_mut() {
        o.remove("format_pending");
    }
}

/// A design saved before the pick kept the placements it was made against opens with the post
/// where it was and moving rigidly: the file's own placements are taken as the pick's.
#[test]
fn an_older_design_with_a_to_object_extrude_opens_pinned_to_its_saved_placements() {
    let mut s = post_to_the_roof();
    let mut file: Value = serde_json::from_str(&s.doc.to_json()).unwrap_or_default();
    as_last_release_saved(&mut file);
    let mut stripped = 0;
    for feature in file["features"].as_array_mut().into_iter().flatten() {
        if feature["extent"].as_object_mut().is_some_and(|e| e.remove("picked_placements").is_some()) {
            stripped += 1;
        }
    }
    assert_eq!(stripped, 1, "the design records the pin");
    let reopened = solidcraft_doc::Document::from_json(&file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    let pinned = reopened
        .features
        .iter()
        .any(|f| matches!(&f.kind, solidcraft_doc::FeatureKind::Extrude { extent, .. } if !extent.picked_placements.is_empty()));
    assert!(pinned, "the older design is pinned as it opens");
    *s.doc_mut() = reopened;
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    let (volume, low, high) = post_extent(&mut s);
    assert!((volume - 3000.0).abs() < 1e-6 && (low - 10.0).abs() < 1e-6 && (high - 40.0).abs() < 1e-6, "{volume} {low} {high}");
}

/// An older design whose components were never placed (no occurrences in the file) opens with
/// each at the identity, and the extrude pinned to that.
#[test]
fn an_older_design_with_unplaced_components_opens_with_its_to_object_extrude_pinned() {
    let s = post_to_the_roof();
    let mut file: Value = serde_json::from_str(&s.doc.to_json()).unwrap_or_default();
    as_last_release_saved(&mut file);
    if let Some(o) = file.as_object_mut() {
        o.remove("occurrences");
    }
    file["assembly"]["joints"] = json!([]);
    for feature in file["features"].as_array_mut().into_iter().flatten() {
        if let Some(e) = feature["extent"].as_object_mut() {
            e.remove("picked_placements");
        }
    }
    let reopened = solidcraft_doc::Document::from_json(&file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    let pinned = reopened
        .features
        .iter()
        .any(|f| matches!(&f.kind, solidcraft_doc::FeatureKind::Extrude { extent, .. } if !extent.picked_placements.is_empty()));
    assert!(pinned, "the extrude is pinned as the design opens");
}

/// #1239: two parts as bodies of one component (a multi-body STEP import). The refusal names the
/// way out, and after Create Components from Bodies the same origins make the joint.
#[test]
fn bodies_of_one_component_get_components_first() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    let p = json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}});
    let e = s.execute("joint.create", &p).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("two different occurrences") && e.contains("component.from_bodies"), "{e}");
    run(&mut s, "component.from_bodies", json!({"bodies": ["Body1", "Body2"]}));
    run(&mut s, "joint.create", p);
    // The first joint's A side stays; B lands on top of it.
    assert!(near(place(&s, "Body2", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
}

/// #527: an assembly with its fasteners passes 200 bodies quickly; the clash check must still
/// cover the whole design. 201 separate 1 mm cubes 2 mm apart, plus one cube pushed half-way
/// into the first: exactly that pair overlaps, by 0.5 mm³.
#[test]
fn interference_checks_designs_with_more_than_200_bodies() {
    let mut s = Session::default();
    for i in 0..201 {
        run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [2 * i, 0, 0], "operation": "new"}));
    }
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [0.5, 0, 0], "operation": "new", "body_name": "Clash"}));
    assert_eq!(s.model.state().bodies.len(), 202);
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["checked"], 202, "{i}");
    assert_eq!(i["count"], 1, "{i}");
    let hit = &i["interferences"][0];
    let mut pair = [hit["a"].as_str().unwrap_or(""), hit["b"].as_str().unwrap_or("")];
    pair.sort_unstable();
    assert_eq!(pair, ["Body1", "Clash"], "{i}");
    assert!((hit["volume_mm3"].as_f64().unwrap_or(0.0) - 0.5).abs() < 1e-6, "{i}");
}

/// The bounding-box sweep behind `inspect.interference` finds exactly the pairs a check of every
/// pair finds (touching boxes count), and reports how many it left out past its limit.
#[test]
fn interference_box_sweep_matches_every_pair_check() {
    use solidcraft_geom::Aabb3;
    let cube = |x: f64, y: f64, z: f64, e: f64| Aabb3 { min: Vec3::new(x, y, z), max: Vec3::new(x + e, y + e, z + e) };
    // A deterministic scatter: overlapping, touching, and apart in only one axis.
    let boxes: Vec<Aabb3> = (0..60)
        .map(f64::from)
        .map(|i| cube((i * 7.3) % 20.0, (i * 3.1) % 9.0, (i * 5.7) % 4.0, 1.0 + (i % 3.0)))
        .chain([cube(30.0, 0.0, 0.0, 1.0), cube(31.0, 0.0, 0.0, 1.0)])
        .collect();
    let mut every = Vec::new();
    for (i, a) in boxes.iter().enumerate() {
        for (j, b) in boxes.iter().enumerate().skip(i + 1) {
            let apart = a.max.x < b.min.x || b.max.x < a.min.x || a.max.y < b.min.y || b.max.y < a.min.y || a.max.z < b.min.z || b.max.z < a.min.z;
            if !apart {
                every.push((i, j));
            }
        }
    }
    assert!(every.contains(&(60, 61)), "touching boxes are a candidate pair");
    let (pairs, skipped) = super::overlapping_pairs(&boxes, usize::MAX);
    assert_eq!((pairs, skipped), (every.clone(), 0));
    let (pairs, skipped) = super::overlapping_pairs(&boxes, 10);
    assert_eq!((pairs.len(), skipped), (10, every.len() - 10));
}

/// A small block in a new component: a part of a linkage, where only its joints matter.
pub(super) fn part(s: &mut Session, name: &str, corner: [f64; 3]) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": corner}));
}

/// A joint between two parts as they are, at a world point with its z along `z`.
pub(super) fn pin(s: &mut Session, kind: &str, name: &str, a: &str, b: &str, p: Vec3, z: Vec3) {
    let at = |o: &str| json!({"occurrence": o, "point": [p.x, p.y, p.z], "z": [z.x, z.y, z.z]});
    let r = run(s, "joint.create", json!({"type": kind, "a": at(a), "b": at(b), "flip": true, "name": name}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Where circles about `p` (radius `r`) and `q` (radius `s`) in the XY plane meet.
pub(super) fn meet(p: Vec3, r: f64, q: Vec3, s: f64) -> [Vec3; 2] {
    let d = q - p;
    let l = d.len();
    let a = (r * r - s * s + l * l) / (2.0 * l);
    let m = p + d * (a / l);
    let n = Vec3::new(-d.y, d.x, 0.0) * ((r * r - a * a).max(0.0).sqrt() / l);
    [m + n, m - n]
}

/// A crank-rocker four-bar about +Z: ground pivots O2 = (0, 0) and O4 = (45, 0), crank 20,
/// coupler 50, rocker 40, assembled with the crank at 90°. Returns the crank pin P0 and the
/// rocker pin Q0 (the upper, open assembly).
pub(super) fn four_bar() -> (Session, Vec3, Vec3) {
    let mut s = Session::default();
    let (o2, o4, p0) = (Vec3::ZERO, Vec3::new(45.0, 0.0, 0.0), Vec3::new(0.0, 20.0, 0.0));
    let [q0, _] = meet(p0, 50.0, o4, 40.0);
    for (name, c) in [("Ground", o2), ("Crank", p0), ("Coupler", (p0 + q0) * 0.5), ("Rocker", q0)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o2, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Coupler", p0, Vec3::Z);
    pin(&mut s, "revolute", "Rocker pin", "Coupler", "Rocker", q0, Vec3::Z);
    pin(&mut s, "revolute", "Rocker", "Ground", "Rocker", o4, Vec3::Z);
    (s, p0, q0)
}

/// The four-bar's rocker pin with the crank driven to `to`°, followed a degree at a time from
/// `from`° (where it is at `q`), so it stays on its branch.
fn rocker_pin(from: f64, to: f64, mut q: Vec3) -> Vec3 {
    let n = ((to - from).abs().ceil() as usize).max(1);
    for k in 1..=n {
        let th = (90.0 + from + (to - from) * k as f64 / n as f64).to_radians();
        let [m1, m2] = meet(Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0), 50.0, Vec3::new(45.0, 0.0, 0.0), 40.0);
        q = if m1.dist(q) <= m2.dist(q) { m1 } else { m2 };
    }
    q
}

#[test]
fn four_bar_follows_its_crank() {
    let (mut s, p0, q0) = four_bar();
    let mut q = q0;
    let mut at_330 = q0;
    for k in 1..=12 {
        let v = 30.0 * f64::from(k);
        let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": v}));
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{v}°: {r}");
        assert!(r["stopped_by_loop"].is_null(), "{v}°: {r}");
        // The driven joint keeps exactly the value asked for, answered in degrees as asked.
        assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - v).abs() < 1e-9 && r["values"][0]["unit"] == "deg", "{r}");
        q = rocker_pin(v - 30.0, v, q);
        let th = (90.0 + v).to_radians();
        let p = Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0);
        for (part, local, want) in [("Crank", p0, p), ("Coupler", p0, p), ("Coupler", q0, q), ("Rocker", q0, q)] {
            assert!(near(place(&s, part, local), want), "{v}°: {part} {:?} wants {want:?}", place(&s, part, local));
        }
        if k == 11 {
            at_330 = q;
        }
    }
    // Back where it started after a full turn, and undo steps back.
    assert!(near(q, q0));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "Rocker", q0), at_330), "{:?}", place(&s, "Rocker", q0));
    // One 180° drive lands on the same (open) branch as the stepped path.
    let (mut s, _, q0) = four_bar();
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 180}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 180.0, q0)), "{:?}", place(&s, "Rocker", q0));
    // A motion study playing the crank places the rocker the same way.
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 10, "keys": [{"joint": "Crank", "points": [[0, 0], [10, 120]]}]}));
    let r = run(&mut s, "motion.play", json!({"study": "Turn", "step": 5}));
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(180.0, 60.0, rocker_pin(0.0, 180.0, q0))), "{:?}", place(&s, "Rocker", q0));
}

/// The rocker swings between its toggle positions, where crank and coupler line up (|Q| = 70
/// and |Q| = 30, about 69.3° and 139.2° from the ground line); driving it further stops where
/// the loop still closes.
#[test]
fn four_bar_rocker_stops_where_the_loop_cannot_close() {
    let (mut s, _, q0) = four_bar();
    let start = q0.y.atan2(q0.x - 45.0).to_degrees();
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": 100}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let reached = r["values"][0]["value"].as_f64().unwrap_or(f64::NAN);
    // |Q|² = 45² + 40² + 2·45·40·cos φ = 30².
    let limit = ((900.0f64 - 3625.0) / 3600.0).acos().to_degrees() - start;
    assert!(reached > limit - 5.0 && reached <= limit, "stopped at {reached}°, the limit is {limit}°");
    // Inside its swing it moves freely, and the crank follows.
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": -10}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    let q = place(&s, "Rocker", q0);
    assert!((q.y.atan2(q.x - 45.0).to_degrees() - (start - 10.0)).abs() < 1e-6, "{q:?}");
    assert!(place(&s, "Coupler", q0).dist(q) < 1e-6);
    // A full turn would end where it started, but the rocker can't get there.
    let (mut s, _, _) = four_bar();
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert!(r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) <= limit, "{r}");
}

/// A loop value a motion link sets follows through the start of its link chain: a dial outside
/// the loop turns a gear, which sets the rocker's angle, so turning the crank turns the dial.
#[test]
fn four_bar_turns_the_source_of_a_linked_loop_value() {
    let linked = || {
        let (mut s, _, q0) = four_bar();
        for (name, x) in [("Dial", 80.0), ("Gear", 100.0)] {
            part(&mut s, name, [x, 0.0, -5.0]);
            pin(&mut s, "revolute", name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
        }
        run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Gear", "ratio": 1}));
        run(&mut s, "joint.motion_link", json!({"a": "Gear", "b": "Rocker", "ratio": 0.5}));
        (s, q0)
    };
    let (mut s, q0) = linked();
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let q = rocker_pin(0.0, 60.0, q0);
    assert!(near(place(&s, "Rocker", q0), q), "{:?} wants {q:?}", place(&s, "Rocker", q0));
    let turn = q.y.atan2(q.x - 45.0) - q0.y.atan2(q0.x - 45.0);
    let dial = s.doc.assembly.joints.iter().find(|j| j.name == "Dial").and_then(|j| j.values.first().copied()).unwrap_or(f64::NAN);
    assert!((dial - 2.0 * turn).abs() < 1e-9, "dial {dial}, rocker turned {turn}");
    // Driving the dial drives the rocker through the chain: two turns of the dial would turn
    // the rocker a full turn, which it can't make.
    let (mut s, _) = linked();
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 720}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Values left out of a drive are free to follow in a loop: a plate on a planar joint whose
/// travel a diagonal slider limits. Driving only the planar joint's x moves its y along.
#[test]
fn values_left_out_of_a_drive_follow_the_loop() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    part(&mut s, "Plate", [-1.0, -1.0, 0.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    let frame = |o: &str, z: [f64; 3], x: [f64; 3]| json!({"occurrence": o, "frame": {"origin": [0, 0, 0], "z": z, "x": x}});
    let flat = |o: &str| frame(o, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let r = run(&mut s, "joint.create", json!({"type": "planar", "a": flat("Ground"), "b": flat("Plate"), "flip": true, "name": "Table"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let diagonal = |o: &str| frame(o, [1.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let r = run(&mut s, "joint.create", json!({"type": "slider", "a": diagonal("Ground"), "b": diagonal("Plate"), "flip": true, "name": "Rail"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Table", "values": [10]}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    assert!(near(place(&s, "Plate", Vec3::ZERO), Vec3::new(10.0, 10.0, 0.0)), "{:?}", place(&s, "Plate", Vec3::ZERO));
}

/// Two boards 400 x 100 x 20 for #10331: yellow at the origin, grounded, and red 300 mm along Y.
fn boards() -> Session {
    let mut s = Session::default();
    for (name, corner) in [("Yellow", [0.0, 0.0, 0.0]), ("Red", [0.0, 300.0, 0.0])] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 400, "width": 100, "height": 20, "corner": corner}));
    }
    let y = occ(&s, "Yellow");
    run(&mut s, "occurrence.ground", json!({"occurrence": y, "grounded": true}));
    s
}

/// #10331: a board on a planar joint, mated by a second planar joint at a face that points the
/// same way as the face it meets. It meets it turned a half turn on the first joint, the way the
/// first joint moves its part into place, in either pick order, and still slides along the
/// line the two planes share. Faces that can never meet stay a conflict, and nothing moves.
#[test]
fn a_second_planar_joint_turns_its_part_a_half_turn_to_meet() {
    // Each case: the yellow face, the red face (world points once red lies on yellow, at
    // x 0..400, y 0..100, z 20..40), and where red's origin ends up.
    let cases = [
        ("fronts", [200.0, 0.0, 10.0], [200.0, 0.0, 30.0], Some(Vec3::new(400.0, 300.0, 20.0))),
        ("ends", [0.0, 50.0, 10.0], [0.0, 50.0, 30.0], Some(Vec3::new(0.0, 400.0, 20.0))),
        ("top against front", [200.0, 0.0, 10.0], [200.0, 50.0, 40.0], None),
    ];
    for red_first in [false, true] {
        for (case, yellow_face, red_face, expected) in cases {
            let mut s = boards();
            let planar = |s: &mut Session, name: &str, yellow: [f64; 3], red: [f64; 3]| {
                let (yellow, red) = (json!({"occurrence": "Yellow", "face": yellow}), json!({"occurrence": "Red", "face": red}));
                let (a, b) = if red_first { (red, yellow) } else { (yellow, red) };
                run(s, "joint.create", json!({"type": "planar", "a": a, "b": b, "name": name}))
            };
            let r = planar(&mut s, "Top", [200.0, 50.0, 20.0], [200.0, 350.0, 0.0]);
            assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{case}: {r}");
            let on_top = place(&s, "Red", Vec3::ZERO);
            let r = planar(&mut s, "Side", yellow_face, red_face);
            let label = format!("{case}, red picked first: {red_first}");
            let Some(expected) = expected else {
                assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(1), "{label}: {r}");
                assert!(near(place(&s, "Red", Vec3::ZERO), on_top), "{label}: {:?}", place(&s, "Red", Vec3::ZERO));
                continue;
            };
            assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{label}: {r}");
            assert!(place(&s, "Red", Vec3::ZERO).dist(expected) < 1e-6, "{label}: {:?}", place(&s, "Red", Vec3::ZERO));
            assert_eq!(dof_of(&mut s, "Red"), 1, "{label}");
            let axes = solidcraft_doc::joints::motion_axes(&s.doc, &s.model.state(), occ(&s, "Red"));
            assert!(axes.len() == 1 && !axes.iter().any(|a| a.angular), "{label}: {axes:?}");
        }
    }
}

/// #10331, with the board turned on the table first (the report's board was turned 40 degrees):
/// a second planar joint between any side of the yellow board and any side of the red one meets,
/// whichever turn the red board needs on the first joint to get there.
#[test]
fn any_two_sides_of_turned_boards_meet_on_a_second_planar_joint() {
    // The sides' centres, the red ones where the red board was made.
    let reds = [
        ("red front", Vec3::new(200.0, 300.0, 10.0)),
        ("red back", Vec3::new(200.0, 400.0, 10.0)),
        ("red left end", Vec3::new(0.0, 350.0, 10.0)),
        ("red right end", Vec3::new(400.0, 350.0, 10.0)),
    ];
    let yellows = [
        ("yellow front", [200.0, 0.0, 10.0]),
        ("yellow back", [200.0, 100.0, 10.0]),
        ("yellow left end", [0.0, 50.0, 10.0]),
        ("yellow right end", [400.0, 50.0, 10.0]),
    ];
    for turn in [40.0f64, 0.0, 137.0] {
        for (yn, yp) in yellows {
            for (rn, rl) in reds {
                let mut s = boards();
                run(
                    &mut s,
                    "occurrence.move",
                    json!({"occurrence": "Red:1", "space": "world", "axis": [0, 0, 1], "angle": format!("{turn} deg"), "origin": [200, 350, 0]}),
                );
                let top = place(&s, "Red", Vec3::new(200.0, 350.0, 0.0));
                run(
                    &mut s,
                    "joint.create",
                    json!({"type": "planar", "a": {"occurrence": "Yellow", "face": [200, 50, 20]}, "b": {"occurrence": "Red", "face": [top.x, top.y, top.z]}, "name": "Top"}),
                );
                let rp = place(&s, "Red", rl);
                let r = run(
                    &mut s,
                    "joint.create",
                    json!({"type": "planar", "a": {"occurrence": "Yellow", "face": yp}, "b": {"occurrence": "Red", "face": [rp.x, rp.y, rp.z]}, "name": "Side"}),
                );
                assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "turned {turn} degrees, {yn} against {rn}: {r}");
            }
        }
    }
}

/// #9687: a second planar joint left as a conflict, as a design saved before joint.create turned
/// its part a half turn keeps it, meets when Edit Joint's OK places it again, and one unsuppressed
/// after its part was turned away on the first joint meets too. A rename alone moves nothing.
#[test]
fn editing_or_unsuppressing_a_conflicting_second_planar_joint_turns_its_part_to_meet() {
    let conflicts = |r: &Value| r["conflicts"].as_array().map(Vec::len);
    let values = |s: &Session, name: &str| s.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.values.clone()).unwrap_or_default();
    let pose = |s: &Session| [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z].map(|p| place(s, "Red", p));
    let mut s = boards();
    let planar = |s: &mut Session, name: &str, yellow: [f64; 3], red: [f64; 3]| {
        run(
            s,
            "joint.create",
            json!({"type": "planar", "a": {"occurrence": "Yellow", "face": yellow}, "b": {"occurrence": "Red", "face": red}, "name": name}),
        )
    };
    planar(&mut s, "Top", [200.0, 50.0, 20.0], [200.0, 350.0, 0.0]);
    let on_top = values(&s, "Top");
    // The fronts point the same way: Side meets with Red turned a half turn on Top.
    let r = planar(&mut s, "Side", [200.0, 0.0, 10.0], [200.0, 0.0, 30.0]);
    assert_eq!(conflicts(&r), Some(0), "{r}");
    let met = pose(&s);
    // Red as it met, slid along the line the two planes share (X) at most.
    let meets = |s: &Session| {
        let now = pose(s);
        let dx = Vec3::new(now[0].x - met[0].x, 0.0, 0.0);
        now.iter().zip(&met).all(|(p, q)| near(*p, *q + dx))
    };
    // As a design saved before joint.create turned Red: Top's values from before Side.
    if let Some(j) = s.doc_mut().assembly.joints.iter_mut().find(|j| j.name == "Top") {
        j.values = on_top;
    }
    let r = run(&mut s, "joint.solve", json!({}));
    assert_eq!(conflicts(&r), Some(1), "{r}");
    let saved = pose(&s);
    assert!(!meets(&s), "{saved:?}");
    let r = run(&mut s, "joint.edit", json!({"joint": "Side", "name": "Front"}));
    assert_eq!(conflicts(&r), Some(1), "a rename: {r}");
    assert!(pose(&s).iter().zip(&saved).all(|(p, q)| near(*p, *q)), "a rename moved Red: {:?}", pose(&s));
    // Edit Joint's OK sends the type, flip and offset.
    let r = run(&mut s, "joint.edit", json!({"joint": "Front", "type": "planar", "flip": false, "offset": 0}));
    assert_eq!(conflicts(&r), Some(0), "Edit Joint's OK: {r}");
    assert!(meets(&s), "Edit Joint's OK: {:?}", pose(&s));
    assert_eq!(dof_of(&mut s, "Red"), 1);
    // Suppressed, Red turned a half turn away on Top, then unsuppressed.
    run(&mut s, "joint.edit", json!({"joint": "Front", "suppressed": true}));
    let angle = values(&s, "Top").get(2).copied().unwrap_or(f64::NAN).to_degrees();
    run(&mut s, "joint.drive", json!({"joint": "Top", "index": 2, "value": angle + 180.0}));
    assert!(!meets(&s), "turned away: {:?}", pose(&s));
    let r = run(&mut s, "joint.edit", json!({"joint": "Front", "suppressed": false}));
    assert_eq!(conflicts(&r), Some(0), "unsuppressed: {r}");
    assert!(meets(&s), "unsuppressed: {:?}", pose(&s));
}

/// #9687: a frame held by many planar joints ties its loops into one group with more values than
/// the loop solver moves at once, which was then only checked: a new joint closing a loop in it
/// stayed a conflict, and a drive in it moved nothing. A table on the ground, parts each on the
/// table and on the ground (11 parts: 70 values with the peg), and a peg 10 mm from where P0's
/// pin point sits: P0 slides onto it as it does with one part, and turns about it when driven.
#[test]
fn a_loop_in_a_group_over_the_solvers_values_still_closes_and_drives() {
    let conflicts = |r: &Value| r["conflicts"].as_array().map(Vec::len);
    for parts in [1usize, 11] {
        let mut s = Session::default();
        part(&mut s, "Ground", [-1.0, -1.0, -2.0]);
        let g = occ(&s, "Ground");
        run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
        part(&mut s, "Table", [-1.0, -1.0, 0.0]);
        pin(&mut s, "planar", "Base", "Ground", "Table", Vec3::ZERO, Vec3::Z);
        for k in 0..parts {
            let (name, c) = (format!("P{k}"), Vec3::new(10.0 * k as f64, 20.0, 0.0));
            part(&mut s, &name, [c.x - 1.0, c.y - 1.0, 0.0]);
            pin(&mut s, "planar", &format!("{name} on table"), "Table", &name, c, Vec3::Z);
            pin(&mut s, "planar", &format!("{name} on ground"), "Ground", &name, c, Vec3::Z);
        }
        let at = |o: &str, p: [f64; 3]| json!({"occurrence": o, "point": p, "z": [0, 0, 1]});
        let peg = json!({"type": "revolute", "name": "Peg", "a": at("Ground", [10.0, 20.0, 0.0]), "b": at("P0", [0.0, 20.0, 0.0]), "flip": true});
        let r = run(&mut s, "joint.create", peg);
        assert_eq!(conflicts(&r), Some(0), "{parts} parts: {r}");
        let pin_point = Vec3::new(10.0, 20.0, 0.0);
        assert!(near(place(&s, "P0", Vec3::new(0.0, 20.0, 0.0)), pin_point), "{parts} parts: {:?}", place(&s, "P0", Vec3::new(0.0, 20.0, 0.0)));
        // P0's point 1 mm along X from its pin point (closing the loop may have turned P0 a little
        // about the peg), then 30 degrees further round.
        let (p, q0) = (Vec3::new(1.0, 20.0, 0.0), place(&s, "P0", Vec3::new(1.0, 20.0, 0.0)));
        let turn = s.doc.assembly.joints.iter().find(|j| j.name == "Peg").and_then(|j| j.values.first().copied()).unwrap_or(f64::NAN);
        let r = run(&mut s, "joint.drive", json!({"joint": "Peg", "value": turn.to_degrees() + 30.0}));
        assert!(r["stopped_by_loop"].is_null() && conflicts(&r) == Some(0), "{parts} parts: {r}");
        let (a, b) = (q0 - pin_point, place(&s, "P0", p) - pin_point);
        let sin = a.x * b.y - a.y * b.x;
        assert!((b.len() - 1.0).abs() < 1e-6 && (sin.abs() - 0.5).abs() < 1e-6 && b.z.abs() < 1e-6, "{parts} parts: {a:?} to {b:?}");
    }
}

/// #18880: a chain of links closed into a ring ties its loops into one group with more values
/// than the loop solver moves at once. The group cannot be narrowed (every loop of it runs
/// through the value that moves), so it stayed a conflict and nothing in it moved: the model
/// locked up as soon as the loop was closed. The ring turns now, and the links follow.
#[test]
fn a_ring_of_links_closes_and_drives() {
    let mut s = Session::default();
    let n = 66usize;
    for i in 0..n {
        run(&mut s, "component.create", json!({"name": format!("L{i}"), "grounded": i == 0}));
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    // The links on a circle, each joined to the next at the point between them.
    let radius = n as f64 * 30.0 / (2.0 * std::f64::consts::PI);
    let pin_point = |i: usize| {
        let th = 2.0 * std::f64::consts::PI * (i as f64 + 0.5) / n as f64;
        Vec3::new(radius * th.cos(), radius * th.sin(), 0.0)
    };
    for i in 0..n {
        let j = (i + 1) % n;
        let p = pin_point(i);
        let side = |o: &str| json!({"occurrence": o, "point": [p.x, p.y, p.z], "z": [0.0, 0.0, 1.0]});
        let r = run(
            &mut s,
            "joint.create",
            json!({"type": "revolute", "a": side(&format!("L{i}")), "b": side(&format!("L{j}")), "flip": true, "name": format!("H{i}")}),
        );
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "H{i}: {r}");
    }
    // The ring is one loop: the drive turns the first link and the rest follow it.
    let before = place(&s, "L1", Vec3::ZERO);
    let r = run(&mut s, "joint.drive", json!({"joint": "H0", "value": 10}));
    assert_eq!(r["moved"], true, "{r}");
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - 10.0).abs() < 1e-9, "{r}");
    let after = place(&s, "L1", Vec3::ZERO);
    assert!(after.dist(before) > 1.0, "the link did not move: {before:?} to {after:?}");
}

/// A joint of the folding cube toy: a pin (cylindrical) or the planar joint beside it.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Toy {
    Pin,
    Flat,
}

/// #18880: a folding cube toy, 8 cubes on a ring of 8 hinges, each hinge a plate on two pins, and
/// on every pin a cylindrical joint (a quarter turn of limits) with a planar joint that keeps the
/// plate against its cube (the joints of the report's design, at their places; the parts have no
/// bodies). Joint `Jn` is row n: (kind, a, b, world point, z, angle limits from where it is,
/// degrees). Every pin sits at one end of its limits; A2 is grounded.
#[rustfmt::skip]
pub(super) const CUBE_RING: [(Toy, &str, &str, [f64; 3], [f64; 3], Option<(f64, f64)>); 32] = [
        (Toy::Pin, "A1", "H1", [-14.955, 10.121, 8.442], [-1.0, 0.0, 0.0], Some((-90.0, 0.0))),
        (Toy::Pin, "H1", "B1", [-1.62, 10.121, 2.346], [1.0, 0.0, 0.0], Some((0.0, 90.0))),
        (Toy::Flat, "A1", "H1", [-14.955, 10.121, 5.394], [-1.0, 0.0, 0.0], None),
        (Toy::Flat, "H1", "B1", [-1.62, 10.121, 2.346], [-1.0, 0.0, 0.0], None),
        (Toy::Pin, "H2", "A1", [-14.701, -2.96, 21.269], [0.0, -1.0, 0.0], Some((0.0, 90.0))),
        (Toy::Flat, "A1", "H2", [-17.749, -2.96, 21.269], [0.0, -1.0, 0.0], None),
        (Toy::Pin, "B2", "H2", [-20.797, 10.375, 21.269], [0.0, 1.0, 0.0], Some((-90.0, 0.0))),
        (Toy::Flat, "B2", "H2", [-17.749, 10.375, 21.269], [0.0, 1.0, 0.0], None),
        (Toy::Flat, "H3", "B1", [-14.701, 10.375, -10.481], [0.0, -1.0, 0.0], None),
        (Toy::Pin, "H3", "B1", [-14.701, 10.375, -10.481], [0.0, 1.0, 0.0], Some((0.0, 90.0))),
        (Toy::Flat, "A2", "H3", [-17.749, 10.375, -10.481], [0.0, 1.0, 0.0], None),
        (Toy::Pin, "A2", "H3", [-20.797, -2.96, -10.481], [0.0, -1.0, 0.0], Some((-90.0, 0.0))),
        (Toy::Flat, "H4", "B2", [-33.624, -2.706, 21.523], [0.0, 0.0, -1.0], None),
        (Toy::Pin, "H4", "B2", [-33.624, -2.706, 21.523], [0.0, 0.0, 1.0], Some((-90.0, 0.0))),
        (Toy::Flat, "H4", "A3", [-39.72, -2.706, 21.523], [0.0, 0.0, -1.0], None),
        (Toy::Pin, "H4", "A3", [-39.72, -2.706, 8.188], [0.0, 0.0, -1.0], Some((-90.0, 0.0))),
        (Toy::Flat, "H5", "A2", [-33.624, -2.706, -10.735], [0.0, 0.0, 1.0], None),
        (Toy::Pin, "H5", "A2", [-33.624, -2.706, 2.6], [0.0, 0.0, 1.0], Some((-90.0, 0.0))),
        (Toy::Flat, "B3", "H5", [-36.672, -2.706, 2.6], [0.0, 0.0, 1.0], None),
        (Toy::Pin, "B3", "H5", [-39.72, -2.706, -10.735], [0.0, 0.0, -1.0], Some((0.0, 90.0))),
        (Toy::Flat, "H6", "B3", [-52.547, 10.375, -10.481], [0.0, -1.0, 0.0], None),
        (Toy::Pin, "H6", "B3", [-52.547, -2.96, -10.481], [0.0, -1.0, 0.0], Some((-90.0, 0.0))),
        (Toy::Flat, "A4", "H6", [-55.595, 10.375, -10.481], [0.0, 1.0, 0.0], None),
        (Toy::Pin, "H6", "A4", [-58.643, -2.96, -10.481], [0.0, -1.0, 0.0], Some((0.0, 90.0))),
        (Toy::Flat, "H7", "A3", [-52.547, 10.375, 21.269], [0.0, -1.0, 0.0], None),
        (Toy::Pin, "H7", "A3", [-52.547, -2.96, 21.269], [0.0, -1.0, 0.0], Some((0.0, 90.0))),
        (Toy::Flat, "B4", "H7", [-55.595, 10.375, 21.269], [0.0, 1.0, 0.0], None),
        (Toy::Pin, "B4", "H7", [-58.643, 10.375, 21.269], [0.0, 1.0, 0.0], Some((-90.0, 0.0))),
        (Toy::Flat, "H8", "B4", [-71.724, 10.121, 8.442], [1.0, 0.0, 0.0], None),
        (Toy::Pin, "H8", "B4", [-58.389, 10.121, 8.442], [1.0, 0.0, 0.0], Some((-90.0, 0.0))),
        (Toy::Flat, "H8", "A4", [-71.724, 10.121, 2.346], [1.0, 0.0, 0.0], None),
        (Toy::Pin, "H8", "A4", [-58.389, 10.121, 2.346], [1.0, 0.0, 0.0], Some((0.0, 90.0))),
];

/// The cube toy, built joint by joint, without the joints `without` (row numbers).
pub(super) fn cube_ring(without: &[usize]) -> Session {
    let mut s = Session::default();
    for part in ["A1", "A2", "A3", "A4", "B1", "B2", "B3", "B4", "H1", "H2", "H3", "H4", "H5", "H6", "H7", "H8"] {
        run(&mut s, "component.create", json!({"name": part, "grounded": part == "A2"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
    }
    for (n, (kind, a, b, p, z, limits)) in CUBE_RING.iter().enumerate().filter(|(n, _)| !without.contains(n)) {
        let at = json!({"occurrence": a, "point": p, "z": z});
        let kind = if *kind == Toy::Pin { "cylindrical" } else { "planar" };
        let mut params = json!({"type": kind, "a": a, "b": b, "at": at, "name": format!("J{n}")});
        if let Some((lo, hi)) = limits {
            params["limits"] = json!([[lo, hi], null]);
        }
        let r = run(&mut s, "joint.as_built", params);
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "J{n}: {r}");
    }
    s
}

/// #18880: the cube toy (`CUBE_RING`) has two ways to fold: hinges 1 and 8 on one line, or
/// hinges 4 and 5 on another; the other four hinges can't turn. The loop solver cut each step
/// back to the limits after solving it, so the pins that could not turn kept the loops open and
/// the drive stopped a tenth of a degree in. Each fold moves through its pin's whole quarter turn
/// now, and the hinges off its line stay where they are.
#[test]
fn a_ring_of_hinged_cubes_folds_both_ways() {
    let mut s = cube_ring(&[]);
    // The pin joints of hinge `h`.
    let pins = |h: usize| -> Vec<String> {
        let plate = format!("H{h}");
        CUBE_RING
            .iter()
            .enumerate()
            .filter(|(_, (kind, a, b, ..))| *kind == Toy::Pin && (*a == plate || *b == plate))
            .map(|(n, _)| format!("J{n}"))
            .collect()
    };
    // Each fold, from either hinge on its line: the pin driven and the other hinge on the line.
    for (pin, mirror) in [("J0", 8usize), ("J29", 1), ("J13", 5)] {
        // Hinges 2, 3, 6 and 7 are on neither line. (The hinges of the other fold may turn: a
        // folded hinge's plate is free to swing on its two pins.)
        let off_line: Vec<(String, Vec<f64>)> = [2, 3, 6, 7].into_iter().flat_map(pins).map(|j| (j.clone(), values_of(&s, &j))).collect();
        let mirrored: Vec<(String, Vec<f64>)> = pins(mirror).into_iter().map(|j| (j.clone(), values_of(&s, &j))).collect();
        // The whole quarter turn the pin's limits allow, and back.
        for to in [-45.0, -90.0, 0.0] {
            let r = run(&mut s, "joint.drive", json!({"joint": pin, "value": to}));
            assert_eq!(r["moved"], true, "{pin} to {to}: {r}");
            assert!(r["stopped_by_loop"].is_null(), "{pin} to {to}: {r}");
            assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{pin} to {to}: {r}");
            assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - to).abs() < 1e-6, "{pin} to {to}: {r}");
            if to == -90.0 {
                // The hinge on the fold's line turns with it.
                let turned = mirrored.iter().any(|(j, was)| (values_of(&s, j)[0] - was[0]).abs() > 0.1);
                assert!(turned, "{pin} to {to}: the hinge {mirror} did not fold with it");
            }
            // The hinges off both lines can't turn and don't.
            for (j, was) in &off_line {
                assert!((values_of(&s, j)[0] - was[0]).abs() < 1e-6, "{pin} to {to}: {j} turned from {was:?} to {:?}", values_of(&s, j));
            }
        }
    }
}

/// A slider-crank: crank 15 about the origin, rod 40, a piston sliding along X. Returns the crank
/// pin and the wrist pin.
fn slider_crank() -> (Session, Vec3, Vec3) {
    let mut s = Session::default();
    let (o, p0) = (Vec3::ZERO, Vec3::new(0.0, 15.0, 0.0));
    let s0 = Vec3::new((40.0f64 * 40.0 - 15.0 * 15.0).sqrt(), 0.0, 0.0);
    for (name, c) in [("Ground", o), ("Crank", p0), ("Rod", s0 * 0.5), ("Piston", s0)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Rod", p0, Vec3::Z);
    pin(&mut s, "revolute", "Wrist pin", "Rod", "Piston", s0, Vec3::Z);
    pin(&mut s, "slider", "Bore", "Ground", "Piston", s0, Vec3::X);
    (s, p0, s0)
}

/// The piston sits at a·cos θ + √(b² − (a·sin θ)²).
#[test]
fn slider_crank_moves_the_piston() {
    let (mut s, p0, s0) = slider_crank();
    for v in [40.0, 135.0, 270.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": v}));
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{v}°: {r}");
        let th = (90.0f64 + v).to_radians();
        let x = 15.0 * th.cos() + (40.0f64.powi(2) - (15.0 * th.sin()).powi(2)).sqrt();
        assert!(near(place(&s, "Piston", s0), Vec3::new(x, 0.0, 0.0)), "{v}°: {:?} wants x = {x}", place(&s, "Piston", s0));
        assert!(near(place(&s, "Rod", p0), Vec3::new(15.0 * th.cos(), 15.0 * th.sin(), 0.0)));
    }
}

/// A slotted-arm drive: a clamp turns about the origin (Yaw) and carries a pin 26 from it; an arm
/// turns about a shaft 52 out on a motor fixed to the ground, and the pin slides in the arm's slot
/// (Slot: its frame at the shaft, x along the slot towards the pin). Every joint of the loop
/// moves when it does, the slot's included. The pin's face sits `gap` mm off the arm's along
/// the axes: a gap no value of the loop takes out, within the conflict check.
pub(crate) fn slotted_arm(yaw_first: bool, gap: f64) -> Session {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Clamp", 26.0), ("Motor", 52.0), ("Arm", 40.0)] {
        part(&mut s, name, [x - 1.0, -1.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    // The joint order sets the tree: with the yaw first, the arm hangs from the clamp through the
    // slot and the arm's shaft closes the loop; with the motor first, the slot closes it.
    if yaw_first {
        pin(&mut s, "revolute", "Yaw", "Ground", "Clamp", Vec3::ZERO, Vec3::Z);
    }
    pin(&mut s, "rigid", "Mount", "Ground", "Motor", Vec3::new(52.0, 0.0, 0.0), Vec3::Z);
    if !yaw_first {
        pin(&mut s, "revolute", "Yaw", "Ground", "Clamp", Vec3::ZERO, Vec3::Z);
    }
    pin(&mut s, "revolute", "Shaft", "Arm", "Motor", Vec3::new(52.0, 0.0, 0.0), Vec3::Z);
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "pin_slot", "a": framed("Arm", [52.0, 0.0, 0.0], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]),
            "b": framed("Clamp", [26.0, 0.0, gap], [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0]), "flip": true, "values": [0, 26], "name": "Slot"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    s
}

/// A joint's stored values.
fn values_of(s: &Session, joint: &str) -> Vec<f64> {
    s.doc.assembly.joints.iter().find(|j| j.name == joint).map(|j| j.values.clone()).unwrap_or_default()
}

/// Driving either end of the slotted arm moves the other: the arm turns atan(26 sin θ / (52 −
/// 26 cos θ)) for a yaw θ, and the pin slides out from 26 to √(26² + 52² − 2·26·52 cos θ).
#[test]
fn slotted_arm_drives_both_ways() {
    for (yaw_first, gap) in [(false, 0.0), (true, 0.0), (false, 3e-8), (true, 3e-8)] {
        slotted_arm_drives(yaw_first, gap);
    }
}

fn slotted_arm_drives(yaw_first: bool, gap: f64) {
    let mut s = slotted_arm(yaw_first, gap);
    let (shaft, pin_at) = (Vec3::new(52.0, 0.0, 0.0), Vec3::new(26.0, 0.0, 0.0));
    let check = |s: &Session, what: &str| {
        let th = values_of(s, "Yaw").first().copied().unwrap_or(f64::NAN);
        let p = Vec3::new(26.0 * th.cos(), 26.0 * th.sin(), 0.0);
        assert!(near(place(s, "Clamp", pin_at), p), "{what}: pin {:?} wants {p:?}", place(s, "Clamp", pin_at));
        let arm = values_of(s, "Shaft").first().copied().unwrap_or(f64::NAN);
        let want = (26.0 * th.sin()).atan2(52.0 - 26.0 * th.cos());
        assert!((arm.abs() - want.abs()).abs() < 1e-6, "{what}: arm {arm} wants ±{want}");
        // The arm's slot points at the pin, which sits as far out as the slot says.
        let tip = place(s, "Arm", Vec3::new(40.0, 0.0, 0.0));
        let along = (p - shaft) * (1.0 / (p - shaft).len());
        assert!(near(tip, shaft + along * 12.0), "{what}: arm tip {tip:?}");
        let slide = values_of(s, "Slot").get(1).copied().unwrap_or(f64::NAN);
        assert!((slide.abs() - (p - shaft).len()).abs() < 1e-6, "{what}: slide {slide} wants {}", (p - shaft).len());
    };
    check(&s, &format!("yaw first {yaw_first}, gap {gap}, at rest"));
    for v in [10.0, 40.0, -40.0, 0.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Yaw", "value": v}));
        assert!(
            r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0),
            "yaw first {yaw_first}, gap {gap}, yaw {v}°: {r}"
        );
        assert!((values_of(&s, "Yaw").first().copied().unwrap_or(f64::NAN) - f64::to_radians(v)).abs() < 1e-9);
        check(&s, &format!("yaw first {yaw_first}, gap {gap}, yaw {v}°"));
    }
    let slide = values_of(&s, "Slot").get(1).copied().unwrap_or(f64::NAN).abs();
    assert!((slide - 26.0).abs() < 1e-6, "{slide}");
    for v in [15.0, -27.0, 27.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Shaft", "value": v}));
        assert!(
            r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0),
            "yaw first {yaw_first}, gap {gap}, arm {v}°: {r}"
        );
        assert!((values_of(&s, "Shaft").first().copied().unwrap_or(f64::NAN) - f64::to_radians(v)).abs() < 1e-9);
        check(&s, &format!("yaw first {yaw_first}, gap {gap}, arm {v}°"));
    }
    // The clamp's pin reaches a point on its circle, the arm following.
    let (th, at) = (20f64.to_radians(), place(&s, "Clamp", pin_at));
    let r = run(&mut s, "joint.reach", json!({"occurrence": "Clamp", "at": [at.x, at.y, at.z], "point": [26.0 * th.cos(), 26.0 * th.sin(), 0.0]}));
    assert!(r["reached"] == json!(true) && r["stopped_by_loop"].is_null(), "yaw first {yaw_first}, gap {gap}, reach: {r}");
    check(&s, &format!("yaw first {yaw_first}, gap {gap}, reach"));
    // A limit on the arm stops the yaw where the arm meets it.
    run(&mut s, "joint.limits", json!({"joint": "Shaft", "min": -20, "max": 20}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Yaw", "value": 40}));
    assert!(r["stopped_by_loop"].is_object() && r["conflicts"].as_array().map(Vec::len) == Some(0), "yaw first {yaw_first}, gap {gap}, limit: {r}");
    check(&s, &format!("yaw first {yaw_first}, gap {gap}, limit"));
    let arm = values_of(&s, "Shaft").first().copied().unwrap_or(f64::NAN).abs().to_degrees();
    assert!(arm > 19.9 && arm <= 20.0 + 1e-9, "yaw first {yaw_first}, gap {gap}: the arm stopped at {arm}°");
}

/// The four-bar with its rocker a rod on an axle (a cylindrical joint), and the rocker pin
/// joining the coupler to the rod 4 mm up the axle from where the rod sits, its axis turned
/// `tilt` rad from the others about Y. Returns the closing joint's answer and the rocker pin.
fn rod_on_an_axle(tilt: f64) -> (Session, Value, Vec3) {
    let mut s = Session::default();
    let (o2, o4, p0) = (Vec3::ZERO, Vec3::new(45.0, 0.0, 0.0), Vec3::new(0.0, 20.0, 0.0));
    let [q0, _] = meet(p0, 50.0, o4, 40.0);
    for (name, c) in [("Ground", o2), ("Crank", p0), ("Coupler", (p0 + q0) * 0.5), ("Rod", q0)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o2, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Coupler", p0, Vec3::Z);
    pin(&mut s, "cylindrical", "Axle", "Ground", "Rod", o4, Vec3::Z);
    let z = [tilt.sin(), 0.0, tilt.cos()];
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "Coupler", "point": [q0.x, q0.y, 0.0], "z": z},
            "b": {"occurrence": "Rod", "point": [q0.x, q0.y, 4.0], "z": z}, "flip": true, "name": "Rocker pin"}),
    );
    (s, r, q0)
}

/// Axes of holes picked on parts are seldom exactly parallel (#6211: a few hundredths of a
/// microradian apart on a lever). A closed loop through them leaves a gap of nanometres no value
/// takes out; it still closes, the rod sliding down its axle, and follows its crank. A tilt that
/// shows still locks it.
#[test]
fn a_loop_whose_axes_are_a_hair_out_of_parallel_still_moves() {
    let (mut s, r, q0) = rod_on_an_axle(1e-7);
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let up = Vec3::new(0.0, 0.0, 4.0);
    let close = |a: Vec3, b: Vec3| a.dist(b) < 1e-4;
    assert!(close(place(&s, "Rod", q0 + up), q0), "{:?}", place(&s, "Rod", q0 + up));
    for part in ["Crank", "Coupler", "Rod"] {
        assert_eq!(dof_of(&mut s, part), 1, "{part}");
    }
    for v in [30.0, 60.0, -45.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": v}));
        assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{v}°: {r}");
        let q = rocker_pin(0.0, v, q0);
        assert!(close(place(&s, "Rod", q0 + up), q), "{v}°: {:?} wants {q:?}", place(&s, "Rod", q0 + up));
    }
    // Dragged by its crank pin (the viewport's drag of a part joints hold), the rod follows too.
    let (at, th) = (place(&s, "Crank", Vec3::new(0.0, 20.0, 0.0)), 110f64.to_radians());
    let r = run(&mut s, "joint.reach", json!({"occurrence": "Crank", "at": [at.x, at.y, at.z], "point": [20.0 * th.cos(), 20.0 * th.sin(), 0.0]}));
    assert!(r["reached"] == json!(true) && r["stopped_by_loop"].is_null(), "{r}");
    let q = rocker_pin(0.0, 20.0, q0);
    assert!(close(place(&s, "Rod", q0 + up), q), "reach: {:?} wants {q:?}", place(&s, "Rod", q0 + up));
    // Tilted a milliradian, the loop is locked: no longer planar, it can't turn.
    let (mut s, r, _) = rod_on_an_axle(1e-3);
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert_eq!(dof_of(&mut s, "Rod"), 0);
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 30}));
    assert!(r["stopped_by_loop"].is_object() && r["values"][0]["value"].as_f64().is_some_and(|v| v.abs() < 0.1), "{r}");
}

/// A joint frame on an occurrence, by origin and axes.
fn framed(o: &str, origin: [f64; 3], z: [f64; 3], x: [f64; 3]) -> Value {
    json!({"occurrence": o, "frame": {"origin": origin, "z": z, "x": x}})
}

/// A loop open from the start closes from where it is: an arm on a revolute joint, and a rigid
/// joint that wants it turned 90°.
#[test]
fn an_open_loop_closes_from_where_it_is() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    part(&mut s, "Arm", [0.0, -1.0, 0.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Pivot", "Ground", "Arm", Vec3::ZERO, Vec3::Z);
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": framed("Ground", [0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            "b": framed("Arm", [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), "flip": true, "name": "Lock"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)), Vec3::new(0.0, 10.0, 0.0)), "{:?}", place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)));
}

/// A loop that can't close does not hold up another one: the four-bar still drives beside a
/// block whose rigid joint wants it where its pivot can't take it.
#[test]
fn a_broken_loop_does_not_stop_another() {
    let (mut s, _, q0) = four_bar();
    part(&mut s, "Block", [200.0, -1.0, -5.0]);
    pin(&mut s, "revolute", "Block pivot", "Ground", "Block", Vec3::new(200.0, 0.0, 0.0), Vec3::Z);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Ground", "point": [200, 0, 0]}, "b": {"occurrence": "Block", "point": [210, 0, 0]}, "flip": true, "name": "Wrong"}),
    );
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    let names: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, ["Wrong"], "{r}");
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 60.0, q0)), "{:?}", place(&s, "Rocker", q0));
}

/// A loop locked rigid can't move at all: driving its crank stops at once.
#[test]
fn a_locked_loop_does_not_move() {
    let (mut s, _, _) = four_bar();
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Rocker", "name": "Lock"}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert!(r["values"][0]["value"].as_f64().unwrap_or(f64::NAN).to_radians().abs() < 1e-3, "{r}");
}

/// A spatial loop through a ball joint made as built, its values all 0 (#14852): a crank turning
/// about Y at the hip, a rod on a ball at the crank's tip, a universal joint (two revolutes
/// through a cross) at the rod's foot end, and the foot turning about X at the heel. As the crank
/// starts to turn, the rod moves without turning, so the ball turns about the crank's y, which is
/// the ball's x: its yaw, pitch and roll can't make that turn at pitch 0.
fn ankle() -> Session {
    let mut s = Session::default();
    for (name, c) in [
        ("Shank", [-1.0, -1.0, -5.0]),
        ("Crank", [10.0, 9.0, 39.0]),
        ("Rod", [19.0, 9.0, 19.0]),
        ("Cross", [19.0, 9.0, -1.0]),
        ("Foot", [5.0, -1.0, -1.0]),
    ] {
        part(&mut s, name, c);
    }
    let g = occ(&s, "Shank");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Hip", "Shank", "Crank", Vec3::new(0.0, 10.0, 40.0), Vec3::Y);
    let tip = |o: &str| framed(o, [20.0, 10.0, 40.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let r = run(&mut s, "joint.create", json!({"type": "ball", "a": tip("Crank"), "b": tip("Rod"), "flip": true, "name": "Ball"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    pin(&mut s, "revolute", "Ankle pitch", "Rod", "Cross", Vec3::new(20.0, 10.0, 0.0), Vec3::Y);
    pin(&mut s, "revolute", "Ankle roll", "Cross", "Foot", Vec3::new(20.0, 10.0, 0.0), Vec3::X);
    pin(&mut s, "revolute", "Heel", "Shank", "Foot", Vec3::ZERO, Vec3::X);
    s
}

/// The ankle's crank drives both ways, and every joint of the loop stays met.
#[test]
fn a_loop_through_a_ball_joint_at_rest_drives() {
    let mut s = ankle();
    let (hip, tip, ankle) = (Vec3::new(0.0, 10.0, 40.0), Vec3::new(20.0, 10.0, 40.0), Vec3::new(20.0, 10.0, 0.0));
    for v in [5.0f64, 15.0, -10.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Hip", "value": v}));
        assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{v}°: {r}");
        assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - v).abs() < 1e-9, "{v}°: {r}");
        // The crank's tip turned v° about the hip's y, and the rod's ends are on the crank's tip
        // and on the foot's ankle, which turned about the heel's x.
        let at = place(&s, "Crank", tip);
        assert!(
            (at.dist(tip) - 40.0 * (v.to_radians() / 2.0).sin().abs()).abs() < 1e-6
                && (at.dist(hip) - 20.0).abs() < 1e-6
                && (at.y - 10.0).abs() < 1e-9,
            "{v}°: tip at {at:?}"
        );
        assert!(near(place(&s, "Rod", tip), at), "{v}°: {:?} vs {at:?}", place(&s, "Rod", tip));
        let foot = place(&s, "Foot", ankle);
        assert!(
            (foot.x - 20.0).abs() < 1e-9 && (foot.y.hypot(foot.z) - ankle.y.hypot(ankle.z)).abs() < 1e-6 && foot.dist(ankle) > 1e-3,
            "{v}°: ankle at {foot:?}"
        );
        assert!(near(place(&s, "Rod", ankle), foot) && near(place(&s, "Cross", ankle), foot), "{v}°: {:?} vs {foot:?}", place(&s, "Rod", ankle));
    }
}

/// A link that turns the rocker 100 times as far as its dial: a small turn of the dial is a full
/// turn of the rocker, which it can't make.
#[test]
fn a_geared_down_loop_value_is_followed_all_the_way() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Rocker", "ratio": 100}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 3.6}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// A suppressed dial still turns the rocker through its motion link, and a rocker whose value
/// the file leaves out starts from 0: both are followed all the way, so a full turn stops.
#[test]
fn a_full_turn_stops_from_a_suppressed_dial_or_a_missing_value() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Rocker", "ratio": 0.5}));
    run(&mut s, "joint.edit", json!({"joint": "Dial", "suppressed": true}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 720}));
    assert!(r["stopped_by_loop"].is_object() && r["moved"] == true && r.get("reason").is_none(), "{r}");
    // Undriven, the suppressed dial still follows the rocker when the crank turns.
    run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 0}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    let (mut s, _, _) = four_bar();
    if let Some(j) = s.doc_mut().assembly.joints.iter_mut().find(|j| j.name == "Rocker") {
        j.values.clear();
    }
    let r = run(&mut s, "joint.drive", json!({"joint": "Rocker", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
}

/// Loops that only share a rigid joint are independent: an arm whose lock can't close does not
/// stop another arm, beside it on the same rigidly mounted base, from closing its own loop.
#[test]
fn loops_sharing_only_a_rigid_joint_are_solved_apart() {
    let mut s = Session::default();
    for (name, x) in [("Ground", -1.0), ("Base", -1.0), ("Arm", 0.0), ("Other", 100.0)] {
        part(&mut s, name, [x, -1.0, 0.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Base", "name": "Mount"}));
    pin(&mut s, "revolute", "Pivot", "Base", "Arm", Vec3::ZERO, Vec3::Z);
    pin(&mut s, "revolute", "Other pivot", "Base", "Other", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Ground", "point": [100, 0, 0]}, "b": {"occurrence": "Other", "point": [110, 0, 0]}, "flip": true, "name": "Wrong"}),
    );
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": framed("Ground", [0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            "b": framed("Arm", [0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), "flip": true, "name": "Lock"}),
    );
    // The broken loop is reported (on whichever of its joints closes it), the arm's is not.
    let names: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert!(names.len() == 1 && ["Wrong", "Other pivot"].contains(&names[0]), "{r}");
    assert!(near(place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)), Vec3::new(0.0, 10.0, 0.0)), "{:?}", place(&s, "Arm", Vec3::new(10.0, 0.0, 0.0)));
}

/// Two links writing one value: whichever carries a driven dial's turn into the loop, the turn
/// is followed (here dial A turns gear B, which turns the rocker, before dial D rewrites B).
#[test]
fn a_turn_carried_in_by_any_link_is_followed() {
    let (mut s, _, _) = four_bar();
    for (name, x) in [("A", 80.0), ("B", 100.0), ("D", 120.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
        pin(&mut s, "revolute", name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "A", "b": "B", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "B", "b": "Rocker", "ratio": 0.5}));
    run(&mut s, "joint.motion_link", json!({"a": "D", "b": "B", "ratio": 1}));
    let r = run(&mut s, "joint.drive", json!({"joint": "A", "value": 720}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
}

/// Several links writing one value: every value a link chain starts from can move to close the
/// loop, whichever of them the link order lets through (here dial A's, before dial B rewrites
/// the gear).
#[test]
fn a_loop_value_moves_through_whichever_link_carries_it() {
    let (mut s, _, q0) = four_bar();
    for (name, x) in [("A", 80.0), ("G", 100.0), ("B", 120.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
        pin(&mut s, "revolute", name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "A", "b": "G", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "G", "b": "Rocker", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "B", "b": "G", "ratio": 1}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 60}));
    assert!(r["stopped_by_loop"].is_null() && r["conflicts"].as_array().map(Vec::len) == Some(0), "{r}");
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 60.0, q0)), "{:?}", place(&s, "Rocker", q0));
}

/// Two values of one joint can feed two independent loops: a puck's x drives a pair of parallel
/// slides, its y a slide whose rigid lock can't close. Driving x moves the healthy pair.
#[test]
fn values_of_one_joint_feed_independent_loops() {
    let mut s = Session::default();
    for (name, y) in [("Ground", -10.0), ("Puck", -20.0), ("Car", 50.0), ("Other", 100.0)] {
        part(&mut s, name, [-1.0, y, 0.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    let flat = |o: &str| framed(o, [0.0, -20.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    run(&mut s, "joint.create", json!({"type": "planar", "a": flat("Ground"), "b": flat("Puck"), "flip": true, "name": "Table"}));
    pin(&mut s, "slider", "Rail", "Ground", "Car", Vec3::new(0.0, 50.0, 0.0), Vec3::X);
    pin(&mut s, "slider", "Guide", "Ground", "Car", Vec3::new(10.0, 50.0, 0.0), Vec3::X);
    pin(&mut s, "slider", "Lift", "Ground", "Other", Vec3::new(0.0, 100.0, 0.0), Vec3::Y);
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Ground", "point": [0, 100, 0]}, "b": {"occurrence": "Other", "point": [10, 100, 0]}, "flip": true, "name": "Wrong"}),
    );
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 0, "b": "Rail", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 1, "b": "Lift", "ratio": 1}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Table", "values": [10]}));
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    let names: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, ["Wrong"], "{r}");
    assert!(near(place(&s, "Car", Vec3::new(0.0, 50.0, 0.0)), Vec3::new(10.0, 50.0, 0.0)), "{:?}", place(&s, "Car", Vec3::new(0.0, 50.0, 0.0)));
}

/// Two loops can move different values of one joint: each keeps what it moved. A table's x
/// feeds one pair of parallel slides and its y another; a study drives both pairs.
#[test]
fn loops_moving_different_values_of_one_joint_keep_both() {
    let mut s = Session::default();
    for (name, y) in [("Ground", -10.0), ("Puck", -20.0), ("CarA", 50.0), ("CarB", 100.0)] {
        part(&mut s, name, [-1.0, y, 0.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    let flat = |o: &str| framed(o, [0.0, -20.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    run(&mut s, "joint.create", json!({"type": "planar", "a": flat("Ground"), "b": flat("Puck"), "flip": true, "name": "Table"}));
    for (car, y) in [("CarA", 50.0), ("CarB", 100.0)] {
        pin(&mut s, "slider", &format!("{car} rail"), "Ground", car, Vec3::new(0.0, y, 0.0), Vec3::X);
        pin(&mut s, "slider", &format!("{car} guide"), "Ground", car, Vec3::new(10.0, y, 0.0), Vec3::X);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 0, "b": "CarA rail", "ratio": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "Table", "ia": 1, "b": "CarB rail", "ratio": 1}));
    let keys = json!([{"joint": "CarA guide", "points": [[0, 0], [10, 10]]}, {"joint": "CarB guide", "points": [[0, 0], [10, 20]]}]);
    run(&mut s, "motion.study", json!({"name": "Both", "steps": 10, "keys": keys}));
    let r = run(&mut s, "motion.play", json!({"study": "Both", "step": 10}));
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    let table = s.doc.assembly.joints.iter().find(|j| j.name == "Table").map(|j| j.values.clone()).unwrap_or_default();
    assert!(table.len() == 3 && (table[0] - 10.0).abs() < 1e-6 && (table[1] - 20.0).abs() < 1e-6, "{table:?}");
    // A solve with nothing driven leaves them where they are.
    run(&mut s, "joint.solve", json!({}));
    assert!(near(place(&s, "CarA", Vec3::new(0.0, 50.0, 0.0)), Vec3::new(10.0, 50.0, 0.0)), "{:?}", place(&s, "CarA", Vec3::new(0.0, 50.0, 0.0)));
    assert!(near(place(&s, "CarB", Vec3::new(0.0, 100.0, 0.0)), Vec3::new(20.0, 100.0, 0.0)), "{:?}", place(&s, "CarB", Vec3::new(0.0, 100.0, 0.0)));
}

/// A clamped link can jump: a 100:1 link to a rocker limited to a full turn reaches its limit
/// after the dial's first few degrees, back where it started. Each step is checked, so the drive
/// still stops where the rocker can't go.
#[test]
fn a_clamped_link_is_followed_step_by_step() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Dial", [80.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Dial", "Ground", "Dial", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.limits", json!({"joint": "Rocker", "min": 0, "max": 360}));
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Rocker", "ratio": 100}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 360}));
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
}

/// Each occurrence's degrees of freedom as `joint.list` reports them.
fn dof_of(s: &mut Session, component: &str) -> u64 {
    let o = occ(s, component);
    let l = run(s, "joint.list", json!({}));
    l["dof"].as_array().into_iter().flatten().find(|d| d["occurrence"] == o).and_then(|d| d["dof"].as_u64()).unwrap_or(99)
}

/// Loops take degrees of freedom away: a four-bar's moving parts have one between them (its
/// mobility), a part hinged to the coupler one more, a slider-crank one, a parallelogram one, and a
/// four-bar locked rigid none.
#[test]
fn loops_report_their_mobility() {
    let (mut s, p0, q0) = four_bar();
    for (part, want) in [("Ground", 0), ("Crank", 1), ("Coupler", 1), ("Rocker", 1)] {
        assert_eq!(dof_of(&mut s, part), want, "four-bar {part}");
    }
    // Still one after a drive moved it.
    run(&mut s, "joint.drive", json!({"joint": "Crank", "value": 70}));
    assert_eq!(dof_of(&mut s, "Coupler"), 1);
    // A flag hinged to the coupler half-way along: one more.
    let mid = (p0 + q0) * 0.5;
    part(&mut s, "Flag", [mid.x, mid.y, 5.0]);
    run(&mut s, "joint.as_built", json!({"type": "revolute", "a": "Coupler", "b": "Flag", "name": "Flag hinge"}));
    assert_eq!(dof_of(&mut s, "Flag"), 2);
    // Still two when a rotor elsewhere turns the flag's hinge through a motion link.
    part(&mut s, "Rotor", [100.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Rotor", "Ground", "Rotor", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Rotor", "b": "Flag hinge", "ratio": 1}));
    assert_eq!(dof_of(&mut s, "Flag"), 2);
    // A rotor locked by a loop of its own leaves the flag only the mechanism's one.
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Rotor", "name": "Rotor lock"}));
    assert_eq!(dof_of(&mut s, "Flag"), 1);
    // Locked: nothing moves.
    let (mut s, _, _) = four_bar();
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Rocker", "name": "Lock"}));
    for part in ["Crank", "Coupler", "Rocker"] {
        assert_eq!(dof_of(&mut s, part), 0, "locked {part}");
    }
    // A rotor whose turn a motion link carries into the locked crank can't turn either.
    part(&mut s, "Rotor", [100.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Rotor", "Ground", "Rotor", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Rotor", "b": "Crank", "ratio": 1}));
    assert_eq!(dof_of(&mut s, "Rotor"), 0);
    let (mut s, _, _) = slider_crank();
    for part in ["Crank", "Rod", "Piston"] {
        assert_eq!(dof_of(&mut s, part), 1, "slider-crank {part}");
    }
    // A parallelogram (crank and rocker 20, ground and coupler 45), away from its folded poses.
    let mut s = Session::default();
    let (o2, o4) = (Vec3::ZERO, Vec3::new(45.0, 0.0, 0.0));
    let (p, q) = (Vec3::new(12.0, 16.0, 0.0), Vec3::new(57.0, 16.0, 0.0));
    for (name, c) in [("Ground", o2), ("Crank", p), ("Coupler", (p + q) * 0.5), ("Rocker", q)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Crank", "Ground", "Crank", o2, Vec3::Z);
    pin(&mut s, "revolute", "Crank pin", "Crank", "Coupler", p, Vec3::Z);
    pin(&mut s, "revolute", "Rocker pin", "Coupler", "Rocker", q, Vec3::Z);
    pin(&mut s, "revolute", "Rocker", "Ground", "Rocker", o4, Vec3::Z);
    for part in ["Crank", "Coupler", "Rocker"] {
        assert_eq!(dof_of(&mut s, part), 1, "parallelogram {part}");
    }
}

/// An actuator in line with the path of its tip pin, as modelled: a panel hinged at the origin,
/// its tip pin at (30, 0) moving along Y, and a barrel and rod along Y from a root pin at
/// (30, -40). There the rod only extends and the barrel's turn is zero to first order, but the
/// barrel turns as soon as the panel does: it has one degree of freedom, at that pose too.
#[test]
fn a_part_at_a_dead_point_of_its_loop_keeps_its_mobility() {
    let mut s = Session::default();
    let (tip, root) = (Vec3::new(30.0, 0.0, 0.0), Vec3::new(30.0, -40.0, 0.0));
    for (name, c) in [("Ground", Vec3::new(0.0, -40.0, 0.0)), ("Panel", tip), ("Barrel", root), ("Rod", (tip + root) * 0.5)] {
        part(&mut s, name, [c.x, c.y, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Hinge", "Ground", "Panel", Vec3::ZERO, Vec3::Z);
    pin(&mut s, "revolute", "Root pin", "Ground", "Barrel", root, Vec3::Z);
    pin(&mut s, "slider", "Stroke", "Barrel", "Rod", root, Vec3::Y);
    pin(&mut s, "revolute", "Tip pin", "Rod", "Panel", tip, Vec3::Z);
    super::solve_counts::take();
    s.refresh();
    assert_eq!(super::solve_counts::take().1, 0, "refresh must not repeat the singular-pose analysis");
    for part in ["Panel", "Barrel", "Rod"] {
        assert_eq!(dof_of(&mut s, part), 1, "dead point: {part}");
    }
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 5}));
    for part in ["Panel", "Barrel", "Rod"] {
        assert_eq!(dof_of(&mut s, part), 1, "driven: {part}");
    }
}

/// A design file path in a fresh temporary folder (removed by the caller).
fn temp_design(tag: &str, ext: &str) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-joint-visibility-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("design.{ext}")).to_string_lossy().to_string();
    (dir, path)
}

/// Two hinge leaves joined by a revolute "Hinge" and a rigid "Glue" to a third part.
fn hinged() -> Session {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    part(&mut s, "Tag", [60.0, 0.0, 0.0]);
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
    pin(&mut s, "rigid", "Glue", "LeafA", "Tag", Vec3::new(61.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    s
}

fn joint(s: &Session, name: &str) -> u64 {
    s.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.id).unwrap()
}

/// #1160: hiding a joint hides only its glyph. It is saved with the design and is an undo step
/// of its own (#2219, like other visibility), the hinge still drives its leaf while hidden, and
/// the folder hides and shows every joint at once.
#[test]
fn hidden_joints_still_drive_and_are_saved_with_the_design() {
    let (dir, path) = temp_design("saved", "json");
    let mut s = hinged();
    let (hinge, glue) = (joint(&s, "Hinge"), joint(&s, "Glue"));
    run(&mut s, "file.save", json!({"path": path}));
    let undo = s.undo.len();
    let r = run(&mut s, "browser.visibility", json!({"joints": ["Hinge"], "visible": false}));
    assert_eq!(r["changed"], 1, "{r}");
    assert!(!s.joint_visible(hinge) && s.joint_visible(glue));
    assert_eq!(s.undo.len(), undo + 1, "hiding is one undo step");
    assert!(s.is_dirty(), "hiding leaves unsaved changes");
    let l = run(&mut s, "joint.list", json!({}));
    assert_eq!(l["joints"].as_array().unwrap().iter().map(|j| j["visible"].clone()).collect::<Vec<_>>(), [json!(false), json!(true)]);

    // Hidden, the hinge still constrains and moves its leaf; undoing the drive keeps it hidden.
    let p = Vec3::new(20.0, 0.0, 0.0);
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)), "{:?}", place(&s, "LeafB", p));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "LeafB", p), p));
    assert!(!s.joint_visible(hinge), "undo keeps the hiding");
    run(&mut s, "edit.redo", json!({}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));

    // Saved and opened again, the hinge stays hidden.
    run(&mut s, "file.save", json!({}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let (hinge, glue) = (joint(&s, "Hinge"), joint(&s, "Glue"));
    assert!(!s.joint_visible(hinge) && s.joint_visible(glue), "{:?}", s.visibility);
    assert!(!s.is_dirty());
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["visibility"]["hidden_joints"], json!([hinge]));

    // The folder: all hidden, then all shown (and saved without any hidden joints).
    run(&mut s, "browser.visibility", json!({"folder": "joints", "visible": false}));
    assert!(!s.joint_visible(hinge) && !s.joint_visible(glue));
    run(&mut s, "browser.visibility", json!({"folder": "joints", "visible": true}));
    assert!(s.joint_visible(hinge) && s.joint_visible(glue));
    // By name among other items, too.
    run(&mut s, "browser.visibility", json!({"items": ["Glue"], "visible": false}));
    assert!(!s.joint_visible(glue));
    run(&mut s, "browser.visibility", json!({"items": ["Glue"], "visible": true}));
    run(&mut s, "file.save", json!({}));
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(saved.get("visibility").is_none_or(|v| v.get("hidden_joints").is_none()), "{saved}");

    // Bad requests change nothing.
    let before = s.visibility.clone();
    for p in [
        json!({"joints": ["Nope"], "visible": false}),
        json!({"joints": [], "visible": false}),
        json!({"joints": "Hinge", "visible": false}),
        json!({"joints": ["Hinge"], "folder": "joints", "visible": false}),
        json!({"joints": ["Hinge"], "items": ["Glue"], "visible": false}),
    ] {
        assert!(s.execute("browser.visibility", &p).is_err(), "{p}");
    }
    assert_eq!(s.visibility, before);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #3892: a large nested STEP assembly hides thousands of joints, and the browser asks about
/// every joint of the design on every frame: the answer must not walk the hidden-joint list,
/// so the work must not grow with the number of hidden joints. The hidden joint stays hidden,
/// the shown one stays shown, and undo and redo keep the hiding right.
#[test]
fn hidden_joint_lookups_do_not_walk_the_hidden_joint_list() {
    use crate::occurrences::visibility_work;

    let mut s = hinged();
    let (hinge, glue) = (joint(&s, "Hinge"), joint(&s, "Glue"));

    // The hidden joints of an imported assembly: thousands of ids (#3892).
    let hide_many = |s: &mut Session, n: u64| {
        s.visibility.hidden_joints.clear();
        for id in 0..n {
            s.visibility.hidden_joints.push(id);
        }
    };
    // The entries one lookup walked, for a joint that is not hidden.
    let walked_by = |s: &Session, id: u64| {
        visibility_work::take();
        let visible = s.joint_visible(id);
        let walked = visibility_work::take();
        assert!(visible, "joint {id} is not hidden here");
        walked
    };

    hide_many(&mut s, 10);
    let few = walked_by(&s, 9_000_001);
    hide_many(&mut s, 10_000);
    let many = walked_by(&s, 9_000_001);
    assert!(
        many <= few + 4,
        "a joint lookup walked {many} entries of a 10,000-entry hidden-joint list and {few} of a \
         10-entry one: the work must not grow with the number of hidden joints"
    );

    // The behaviour is unchanged: the hidden joint is hidden, the shown one is shown, and undo
    // and redo keep the hiding right.
    s.visibility.hidden_joints.clear();
    run(&mut s, "browser.visibility", json!({"joints": ["Hinge"], "visible": false}));
    assert!(!s.joint_visible(hinge) && s.joint_visible(glue));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.joint_visible(hinge), "undo shows the hinge again");
    run(&mut s, "edit.redo", json!({}));
    assert!(!s.joint_visible(hinge), "redo hides it again");
}

/// The cost of a frame's worth of joint questions, with few and with many hidden joints: the
/// manual measurement behind #3892. Run it with `--ignored --nocapture`.
#[test]
#[ignore = "manual scaling measurement"]
fn joint_visibility_lookup_cost_by_hidden_joint_count() {
    use crate::occurrences::visibility_work;

    let joints = 1_000u64;
    for hidden in [0u64, 10, 100, 1_000, 10_000] {
        let mut s = Session::default();
        for id in 0..hidden {
            s.visibility.hidden_joints.push(id);
        }
        let mut times = Vec::new();
        for _ in 0..11 {
            let start = std::time::Instant::now();
            for id in 0..joints {
                let _ = s.joint_visible(id + hidden + 1);
            }
            times.push(start.elapsed().as_secs_f64() * 1e3);
        }
        times.sort_by(f64::total_cmp);
        // The index is built once per change of the view, apart from the answers themselves.
        println!(
            "hidden_joints={hidden:>6} questions={joints} median_ms={:.3} min_ms={:.3} max_ms={:.3} built_entries={}",
            times[times.len() / 2],
            times[0],
            times[times.len() - 1],
            visibility_work::built_take()
        );
    }
}

/// #1160: a design saved before joints could be hidden (format 19, no `hidden_joints`) opens
/// with every joint shown; and a new joint that takes a deleted hidden joint's id starts shown.
#[test]
fn older_designs_show_every_joint_and_new_joints_start_shown() {
    let (dir, path) = temp_design("legacy", "json");
    let mut s = hinged();
    let mut legacy: Value = serde_json::from_slice(&solidcraft_io::write_design(&s.doc).unwrap()).unwrap();
    legacy["format"] = json!("solidcraft/19");
    legacy["visibility"] =
        json!({"hidden_bodies": [], "hidden_occurrences": [], "hidden_origin": ["XY"], "hidden_sketches": [], "shown_sketches": []});
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!(s.doc.assembly.joints.len(), 2);
    assert!(s.doc.assembly.joints.iter().all(|j| s.joint_visible(j.id)));
    assert_eq!(s.visibility.hidden_origin, ["XY"]);

    let glue = joint(&s, "Glue");
    run(&mut s, "browser.visibility", json!({"joints": [glue], "visible": false}));
    run(&mut s, "joint.delete", json!({"joint": glue}));
    pin(&mut s, "rigid", "Glue2", "LeafA", "Tag", Vec3::new(61.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let again = joint(&s, "Glue2");
    assert!(s.joint_visible(again), "id {again} reused from {glue}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Dials R0…R4 on revolute joints to ground, linked R0 → R1 → … → `last` (ratio 1), the links
/// stored from the end of the chain to its start.
fn reverse_link_chain(s: &mut Session, last: &str) {
    for k in 0..5 {
        let x = 100.0 + 10.0 * f64::from(k);
        part(s, &format!("R{k}"), [x, 0.0, -5.0]);
        pin(s, "revolute", &format!("R{k}"), "Ground", &format!("R{k}"), Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    for k in (0..5).rev() {
        let b = if k == 4 { last.to_string() } else { format!("R{}", k + 1) };
        run(s, "joint.motion_link", json!({"a": format!("R{k}"), "b": b, "ratio": 1}));
    }
}

/// A motion link chain carries a value through all its links, however they are stored: five
/// links stored from the end still turn the last joint with the first.
#[test]
fn a_link_chain_stored_backwards_carries_its_value_through() {
    let (mut s, _, _) = four_bar();
    part(&mut s, "Last", [200.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Last", "Ground", "Last", Vec3::new(200.0, 0.0, 0.0), Vec3::Z);
    reverse_link_chain(&mut s, "Last");
    run(&mut s, "joint.drive", json!({"joint": "R0", "value": 10}));
    let l = run(&mut s, "joint.list", json!({}));
    let last = l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == "Last").map(|j| j["values"][0]["value"].clone());
    assert!((last.and_then(|v| v.as_f64()).unwrap_or(f64::NAN) - 10.0).abs() < 1e-9, "{l}");
}

/// Driving the start of such a chain into a four-bar's crank turns the mechanism, and never
/// reports its loop as opened.
#[test]
fn a_link_chain_stored_backwards_drives_a_loop() {
    let (mut s, _, q0) = four_bar();
    reverse_link_chain(&mut s, "Crank");
    for v in [1.0, 30.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "R0", "value": v}));
        assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{v}: {r}");
        assert!(r["stopped_by_loop"].is_null(), "{v}: {r}");
        assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, v, q0)), "{v}: {:?}", place(&s, "Rocker", q0));
    }
}

/// A turret on a pedestal joined to nothing grounded (Rover v7's turret): the solver places
/// neither, so driving the turret moves nothing. The drive, a motion study and joint.list say
/// so, naming the pedestal, instead of answering as if it worked.
#[test]
fn a_turret_on_a_floating_pedestal_says_it_moves_nothing() {
    let mut s = Session::default();
    for (name, corner) in [("Chassis", [0.0, 0.0, 0.0]), ("Arm", [10.0, 0.0, 0.0]), ("Pedestal", [50.0, 0.0, 0.0]), ("Turret", [50.0, 0.0, 2.0])] {
        part(&mut s, name, corner);
    }
    let g = occ(&s, "Chassis");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Shoulder", "Chassis", "Arm", Vec3::new(10.0, 1.0, 1.0), Vec3::Y);
    pin(&mut s, "revolute", "Azimuth", "Pedestal", "Turret", Vec3::new(51.0, 1.0, 2.0), Vec3::Z);
    let corner = Vec3::new(52.0, 0.0, 2.0);
    let start = place(&s, "Turret", corner);
    let r = run(&mut s, "joint.drive", json!({"joint": "Azimuth", "value": 30, "quiet": true}));
    assert_eq!((&r["moved"], &r["unconnected"]), (&json!(false), &json!("Pedestal:1")), "{r}");
    assert!(r["reason"].as_str().is_some_and(|w| w.starts_with("Pedestal:1 is not connected to anything grounded")), "{r}");
    assert!(r.get("dof").is_none() && r.get("fixed").is_none(), "quiet: {r}");
    // The value is set and answered in degrees, as asked; the turret stays where it was.
    assert!((r["values"][0]["value"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9 && r["values"][0]["unit"] == "deg", "{r}");
    assert!(near(place(&s, "Turret", corner), start));
    let l = run(&mut s, "joint.list", json!({}));
    assert_eq!(l["floating"], json!([{"occurrences": ["Pedestal:1", "Turret:1"], "joints": ["Azimuth"]}]), "{l}");
    // A study keying both joints moves the arm and says the turret can't move.
    let keys = json!([{"joint": "Azimuth", "points": [[0, 0], [10, 90]]}, {"joint": "Shoulder", "points": [[0, 0], [10, 45]]}]);
    run(&mut s, "motion.study", json!({"name": "Scan", "steps": 10, "keys": keys}));
    let r = run(&mut s, "motion.play", json!({"study": "Scan", "step": 10, "quiet": true}));
    assert_eq!(r["moved"], true, "{r}");
    let unmoved = r["unmoved"].as_array().cloned().unwrap_or_default();
    assert!(unmoved.len() == 1 && unmoved[0]["name"] == "Azimuth" && unmoved[0]["reason"].as_str().is_some_and(|w| w.contains("Pedestal:1")), "{r}");
    // Quiet: only the arm, which moved, has its pose in the answer.
    assert_eq!(r["positions"].as_array().map(|p| p.iter().map(|x| x["occurrence"].clone()).collect::<Vec<_>>()), Some(vec![json!("Arm:1")]), "{r}");
    // Fixed to the chassis, the pedestal is placed and the turret turns.
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Chassis", "b": "Pedestal", "name": "Mount"}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Azimuth", "value": 60}));
    assert!(r["moved"] == true && r.get("reason").is_none() && r.get("unconnected").is_none() && r["dof"].is_array(), "{r}");
    assert!(!near(place(&s, "Turret", corner), start));
    assert_eq!(run(&mut s, "joint.list", json!({}))["floating"], json!([]));
}

/// Two floating chains whose joints share a name: a filter keeps the chain of the joint it keeps,
/// told apart by id, and counts the other as left out.
#[test]
fn a_filtered_list_tells_floating_chains_apart_by_joint_not_name() {
    let mut s = Session::default();
    let parts = [
        ("Base", [-50.0, 0.0, 0.0]),
        ("Arm", [-40.0, 0.0, 0.0]),
        ("P", [0.0, 0.0, 0.0]),
        ("Q", [10.0, 0.0, 0.0]),
        ("R", [50.0, 0.0, 0.0]),
        ("T", [60.0, 0.0, 0.0]),
    ];
    for (name, corner) in parts {
        part(&mut s, name, corner);
    }
    // A grounded base with its own joint, as in the turret above: the chains below join neither.
    let g = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Shoulder", "Base", "Arm", Vec3::new(-40.0, 1.0, 1.0), Vec3::Y);
    pin(&mut s, "slider", "Glide", "P", "Q", Vec3::new(5.0, 1.0, 1.0), Vec3::X);
    pin(&mut s, "revolute", "Hinge", "R", "T", Vec3::new(55.0, 1.0, 1.0), Vec3::Y);
    run(&mut s, "joint.edit", json!({"joint": "Glide", "name": "Same"}));
    run(&mut s, "joint.edit", json!({"joint": "Hinge", "name": "Same"}));
    let all = run(&mut s, "joint.list", json!({}));
    assert_eq!(all["floating"].as_array().map(Vec::len), Some(2), "{all}");
    let l = run(&mut s, "joint.list", json!({"kinds": ["revolute"]}));
    assert_eq!(l["floating"], json!([{"occurrences": ["R:1", "T:1"], "joints": ["Same"]}]), "{l}");
    assert_eq!(l["omitted"]["floating"], 1, "{l}");
}

/// `quiet: true` leaves the per-occurrence lists out of joint answers, and `moved` says whether
/// a drive moved anything: driving a joint to where it is moves nothing, with no reason given.
#[test]
fn quiet_joint_answers_and_a_drive_to_where_it_is() {
    let mut s = two_cubes();
    let r =
        run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail", "quiet": true}));
    assert!(r.get("dof").is_none() && r.get("fixed").is_none() && r["conflicts"].is_array(), "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5, "quiet": true}));
    assert!(r.get("dof").is_none() && r["moved"] == true, "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5}));
    assert!(r["dof"].is_array() && r["moved"] == false && r.get("reason").is_none(), "{r}");
    // A suppressed joint with no motion link moves nothing, and says why.
    run(&mut s, "joint.edit", json!({"joint": "Rail", "suppressed": true}));
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 8}));
    assert!(r["moved"] == false && r["reason"] == "Rail is suppressed" && r.get("unconnected").is_none(), "{r}");
    // Nor does it store the value, as it did with an undo step: unsuppressed, the part jumped to
    // it (#9742). The reply keeps what was asked apart from what the joint holds.
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc) && s.undo.len() == undo, "a drive of a suppressed joint changed the design");
    assert_eq!((&r["requested"][0]["value"], &r["values"][0]["value"]), (&json!(8.0), &json!(5.0)), "{r}");
    assert!(r.get("held").is_none(), "{r}");
}

/// A screw's travel follows its turn, so a distance asked of it is not reached: the reply says
/// what was asked and why the joint holds another value, and a drive of the distance alone,
/// which moves nothing, says why. A limit is named the same way (#9742).
#[test]
fn a_drive_says_which_values_a_motion_link_or_a_limit_held() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Cy"}));
    run(&mut s, "joint.screw", json!({"joint": "Cy", "pitch": 2}));
    let follows = json!("distance follows its angle through a motion link (screw)");
    let r = run(&mut s, "joint.drive", json!({"joint": "Cy", "values": [90, 5]}));
    assert!(r["moved"] == true && r.get("reason").is_none(), "{r}");
    assert_eq!((&r["requested"][0]["value"], &r["requested"][1]["value"]), (&json!(90.0), &json!(5.0)), "{r}");
    let held = r["held"].as_array().cloned().unwrap_or_default();
    assert_eq!(held.len(), 1, "only the distance is held: {r}");
    assert_eq!(
        (&held[0]["name"], &held[0]["requested"], &held[0]["unit"], &held[0]["why"]),
        (&json!("distance"), &json!(5.0), &json!("mm"), &follows),
        "{r}"
    );
    assert!((held[0]["value"].as_f64().unwrap_or(f64::NAN) - 0.5).abs() < 1e-9, "a quarter turn at pitch 2 travels 0.5 mm: {r}");
    // The distance alone: nothing moves, and the reply says why.
    let r = run(&mut s, "joint.drive", json!({"joint": "Cy", "index": 1, "value": 4}));
    assert_eq!((&r["moved"], &r["reason"], &r["held"][0]["requested"]), (&json!(false), &follows, &json!(4.0)), "{r}");

    let mut s = two_cubes();
    run(&mut s, "joint.as_built", json!({"type": "slider", "a": "A", "b": "B", "name": "Rail", "limits": [[0, 5]]}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 20}));
    assert!(r["moved"] == true && r.get("reason").is_none(), "{r}");
    assert_eq!(
        r["held"],
        json!([{"name": "distance", "requested": 20.0, "value": 5.0, "unit": "mm", "why": "distance is held to its limits, 0 to 5 mm"}]),
        "{r}"
    );
    // Within the limits, nothing is held.
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    assert!(r.get("held").is_none() && r["requested"][0]["value"] == 3.0, "{r}");
}

/// An as-built joint takes a name and limits like any other joint.
#[test]
fn as_built_takes_a_name_and_limits() {
    let mut s = two_cubes();
    let r = run(&mut s, "joint.as_built", json!({"type": "slider", "a": "A", "b": "B", "name": "Rail", "limits": [[0, 5]]}));
    assert_eq!(r["name"], "Rail", "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 20}));
    assert_eq!((&r["values"][0]["value"], &r["values"][0]["limits"]), (&json!(5.0), &json!([0.0, 5.0])), "{r}");
    assert!(near(place(&s, "B", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 5.0)), "{:?}", place(&s, "B", Vec3::new(20.0, 0.0, 0.0)));
}

/// `value` drives a joint's first value only: the others keep theirs, as with a shorter `values`.
#[test]
fn a_single_value_keeps_the_joints_other_values() {
    let mut s = two_cubes();
    run(&mut s, "joint.as_built", json!({"type": "cylindrical", "a": "A", "b": "B", "name": "Spindle"}));
    run(&mut s, "joint.drive", json!({"joint": "Spindle", "values": [30, 5]}));
    let at = |r: &Value| [0, 1].map(|i| r["values"][i]["value"].as_f64().unwrap_or(f64::NAN));
    let r = run(&mut s, "joint.drive", json!({"joint": "Spindle", "value": 45}));
    assert!((at(&r)[0] - 45.0).abs() < 1e-9 && (at(&r)[1] - 5.0).abs() < 1e-9, "{r}");
    let r = run(&mut s, "joint.drive", json!({"joint": "Spindle", "values": [60]}));
    assert!((at(&r)[0] - 60.0).abs() < 1e-9 && (at(&r)[1] - 5.0).abs() < 1e-9, "{r}");
}

/// A motion link's offset is in the linked value's units, like joint values and limits: degrees
/// for an angle, mm for a distance, or an expression. `joint.list` answers it the same way, it is
/// stored in radians as before, and a saved link opens with the offset it had.
#[test]
fn a_link_offset_is_in_the_linked_values_units() {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Dial", 80.0), ("Gear", 100.0), ("Car", 120.0), ("Cart", 140.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    for (kind, name, x) in [("revolute", "Dial", 80.0), ("revolute", "Gear", 100.0), ("slider", "Car", 120.0), ("slider", "Cart", 140.0)] {
        pin(&mut s, kind, name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Gear", "ratio": 1, "offset": 10}));
    run(&mut s, "joint.motion_link", json!({"a": "Car", "b": "Cart", "ratio": 1, "offset": "5 mm"}));
    run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 20}));
    run(&mut s, "joint.drive", json!({"joint": "Car", "value": 3}));
    let value = |l: &Value, name: &str| {
        l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == name).and_then(|j| j["values"][0]["value"].as_f64()).unwrap_or(f64::NAN)
    };
    let l = run(&mut s, "joint.list", json!({}));
    assert!((value(&l, "Gear") - 30.0).abs() < 1e-9 && (value(&l, "Cart") - 8.0).abs() < 1e-9, "{l}");
    let links: Vec<(f64, &str)> =
        l["links"].as_array().into_iter().flatten().map(|k| (k["offset"].as_f64().unwrap_or(f64::NAN), k["unit"].as_str().unwrap_or(""))).collect();
    assert!(
        links.len() == 2 && (links[0].0 - 10.0).abs() < 1e-9 && links[0].1 == "deg" && (links[1].0 - 5.0).abs() < 1e-9 && links[1].1 == "mm",
        "{l}"
    );
    assert!((s.doc.assembly.links[0].offset - 10f64.to_radians()).abs() < 1e-12);
    // A design saved before offsets took degrees stored 0.1 rad, and opens turning Gear 5.73° past Dial.
    s.doc_mut().assembly.links[0].offset = 0.1;
    let (dir, path) = temp_design("link-offset", "solidcraft");
    run(&mut s, "file.save", json!({"path": path}));
    run(&mut s, "file.new", json!({}));
    run(&mut s, "doc.open", json!({"path": path}));
    let l = run(&mut s, "joint.list", json!({}));
    assert!((value(&l, "Gear") - value(&l, "Dial") - 0.1f64.to_degrees()).abs() < 1e-9, "{l}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `joint.motion_link` with `link` puts the new link in place of that one, like the coupling
/// commands do: the other links stay put and the offset is the one typed. An index past the last
/// link is refused.
#[test]
fn a_motion_link_can_replace_another_by_index() {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Dial", 80.0), ("Gear", 100.0), ("Car", 120.0), ("Cart", 140.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    for (kind, name, x) in [("revolute", "Dial", 80.0), ("revolute", "Gear", 100.0), ("slider", "Car", 120.0), ("slider", "Cart", 140.0)] {
        pin(&mut s, kind, name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Gear", "ratio": 1, "offset": 10}));
    run(&mut s, "joint.motion_link", json!({"a": "Car", "b": "Cart", "ratio": 2}));
    let r = run(&mut s, "joint.motion_link", json!({"a": "Dial", "b": "Gear", "ratio": 2, "offset": 5, "link": 0}));
    assert_eq!(r["link"], 0, "{r}");
    assert_eq!(s.doc.assembly.links.len(), 2);
    assert!(s.doc.assembly.links[0].coupling.is_none() && (s.doc.assembly.links[0].offset - 5f64.to_radians()).abs() < 1e-12);
    run(&mut s, "joint.drive", json!({"joint": "Dial", "value": 20}));
    let l = run(&mut s, "joint.list", json!({}));
    let value = |name: &str| {
        l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == name).and_then(|j| j["values"][0]["value"].as_f64()).unwrap_or(f64::NAN)
    };
    // Gear = 2·Dial + 5.
    assert!((value("Gear") - 45.0).abs() < 1e-9, "{l}");
    let bad = json!({"a": "Car", "b": "Cart", "ratio": 1, "link": 5});
    assert!(s.execute("joint.motion_link", &bad).is_err(), "{bad}");
    let bad = json!({"a": "Car", "b": "Cart", "ratio": 1, "link": -1});
    assert!(s.execute("joint.motion_link", &bad).is_err(), "{bad}");
    assert_eq!(s.doc.assembly.links.len(), 2);
}

/// A link between an angle and a distance takes its ratio in b's units per a's, like its offset:
/// a pinion moves its rack 0.5 mm per degree, and a car turns its dial 2 degrees per mm.
/// `joint.list` answers the ratios the same way; they are stored per radian, as before.
#[test]
fn a_link_ratio_between_an_angle_and_a_distance_is_per_degree() {
    let mut s = Session::default();
    for (name, x) in [("Ground", 0.0), ("Pinion", 80.0), ("Rack", 100.0), ("Car", 120.0), ("Dial", 140.0)] {
        part(&mut s, name, [x, 0.0, -5.0]);
    }
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    for (kind, name, x) in [("revolute", "Pinion", 80.0), ("slider", "Rack", 100.0), ("slider", "Car", 120.0), ("revolute", "Dial", 140.0)] {
        pin(&mut s, kind, name, "Ground", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.motion_link", json!({"a": "Pinion", "b": "Rack", "ratio": 0.5}));
    run(&mut s, "joint.motion_link", json!({"a": "Car", "b": "Dial", "ratio": 2}));
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 20}));
    run(&mut s, "joint.drive", json!({"joint": "Car", "value": 3}));
    let l = run(&mut s, "joint.list", json!({}));
    let value = |name: &str| {
        l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == name).and_then(|j| j["values"][0]["value"].as_f64()).unwrap_or(f64::NAN)
    };
    assert!((value("Rack") - 10.0).abs() < 1e-9 && (value("Dial") - 6.0).abs() < 1e-9, "{l}");
    let ratios: Vec<f64> = l["links"].as_array().into_iter().flatten().map(|k| k["ratio"].as_f64().unwrap_or(f64::NAN)).collect();
    assert!(ratios.len() == 2 && (ratios[0] - 0.5).abs() < 1e-9 && (ratios[1] - 2.0).abs() < 1e-9, "{l}");
    let stored: Vec<f64> = s.doc.assembly.links.iter().map(|k| k.ratio).collect();
    assert!((stored[0] - 0.5f64 / 1f64.to_radians()).abs() < 1e-9 && (stored[1] - 2f64.to_radians()).abs() < 1e-12, "{stored:?}");
}

/// A new joint starts at its values inside its limits. Limits that leave out 0 (or the values
/// given) start it at the nearer limit, which it keeps, and the answer says the part moved there.
#[test]
fn a_new_joint_starts_inside_its_limits_and_says_so() {
    let mut s = two_cubes();
    let r = run(&mut s, "joint.as_built", json!({"type": "slider", "a": "A", "b": "B", "name": "Rail", "limits": [[2, 5]]}));
    let w = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        w.len() == 1 && w[0].as_str().is_some_and(|w| w.starts_with("Rail distance starts at 2 mm, inside its limits [2, 5], not at 0 mm")),
        "{r}"
    );
    assert!(near(place(&s, "B", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 2.0)), "{:?}", place(&s, "B", Vec3::new(20.0, 0.0, 0.0)));
    assert_eq!(s.doc.assembly.joints[0].values, [2.0]);
    // Values given outside the limits start at the nearer one too; angles are answered in degrees.
    let mut s = two_cubes();
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": [90], "limits": [[0, 60]], "name": "Hinge"}),
    );
    let w = r["warnings"].as_array().cloned().unwrap_or_default();
    assert!(w.len() == 1 && w[0].as_str().is_some_and(|w| w.contains("starts at 60 deg") && w.contains("not at 90 deg")), "{r}");
    // Inside the limits, nothing to say.
    let mut s = two_cubes();
    let r = run(&mut s, "joint.as_built", json!({"type": "cylindrical", "a": "A", "b": "B", "name": "Spindle", "limits": [[-30, 30], [0, 5]]}));
    assert!(r.get("warnings").is_none(), "{r}");
}

/// The trace's points as world points.
fn traced(r: &Value) -> Vec<Vec3> {
    r["points"].as_array().into_iter().flatten().filter_map(crate::params::vec3).collect()
}

/// A point rigidly carried from segment (a0, b0) to segment (a, b) in the XY plane.
fn carried(p0: Vec3, a0: Vec3, b0: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let turn = (b.y - a.y).atan2(b.x - a.x) - (b0.y - a0.y).atan2(b0.x - a0.x);
    let d = p0 - a0;
    a + Vec3::new(d.x * turn.cos() - d.y * turn.sin(), d.x * turn.sin() + d.y * turn.cos(), d.z)
}

/// A point on the four-bar's coupler, 10 mm off the line between its pins, traces the coupler
/// curve: the crank pin on its circle, the rocker pin where the coupler and rocker circles meet
/// (on the open branch), and the point carried with the pins. A motion study turning the crank
/// traces the same curve, and the design is left as it was.
#[test]
fn four_bar_coupler_point_traces_its_closed_form_curve() {
    let (mut s, p0, q0) = four_bar();
    let d = (q0 - p0) * (1.0 / q0.dist(p0));
    let c0 = (p0 + q0) * 0.5 + Vec3::new(-d.y, d.x, 0.0) * 10.0;
    let before = s.doc.clone();
    let p = json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "joint": "Crank", "from": 0, "to": 360, "samples": 73});
    let r = run(&mut s, "motion.trace", p);
    assert_eq!(*s.doc, *before, "a trace leaves the design as it was");
    assert!(r["stopped_at"].is_null() && r["samples"] == 73, "{r}");
    let pts = traced(&r);
    assert_eq!(pts.len(), 73);
    let mut q = q0;
    for (k, got) in pts.iter().enumerate() {
        let v = 5.0 * k as f64;
        if k > 0 {
            q = rocker_pin(v - 5.0, v, q);
        }
        let th = (90.0 + v).to_radians();
        let want = carried(c0, p0, q0, Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0), q);
        assert!(near(*got, want), "{v}°: {got:?} wants {want:?}");
    }
    assert!(pts.first().zip(pts.last()).is_some_and(|(a, b)| near(*a, *b)), "a full turn closes the curve");
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 72, "keys": [{"joint": "Crank", "points": [[0, 0], [72, 360]]}]}));
    let r = run(&mut s, "motion.trace", json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "study": "Turn"}));
    let from_study = traced(&r);
    assert_eq!(from_study.len(), 73, "{r}");
    assert!(from_study.iter().zip(&pts).all(|(a, b)| near(*a, *b)), "{r}");
}

fn kept_trace_fit(s: &Session, id: u64) -> Vec<Vec3> {
    s.doc.sketch(id).unwrap().wires.first().unwrap().fit.clone()
}

#[test]
fn chebyshev_demo_regenerates_its_analytic_trajectory_without_resampling_playback() {
    let mut s = Session::default();
    s.run_script(&serde_json::from_str(include_str!("../../../../examples/chebyshev-motion-trace.json")).unwrap()).unwrap();
    let trace = s.doc.assembly.traces.first().unwrap().clone();
    let original = kept_trace_fit(&s, trace.sketch);
    assert_eq!(original.len(), 145);
    assert_eq!(s.doc.assembly.joints.len(), 4);
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    // Independent circle-intersection construction: P is the crank end, Q the
    // coupler/rocker pin, and the tracer extends the coupler by another PQ.
    let mut q = Vec3::new(40.0, 50.0, 0.0);
    for (i, got) in original.iter().enumerate() {
        let theta = (90.0 + 2.5 * i as f64).to_radians();
        let p = Vec3::new(20.0 * theta.cos(), 20.0 * theta.sin(), 0.0);
        let [a, b] = meet(p, 50.0, Vec3::new(40.0, 0.0, 0.0), 50.0);
        q = if a.dist(q) < b.dist(q) { a } else { b };
        let want = q * 2.0 - p + Vec3::Z * 10.0;
        assert!(near(*got, want), "sample {i}: {got:?} wants {want:?}");
    }
    let count = s.motion_traces.recomputations;
    for angle in [30, 90, 180, 270, 0] {
        let result = run(&mut s, "joint.drive", json!({"joint":"Crank drive","value":angle,"quiet":true}));
        assert!(result["stopped_by_loop"].is_null(), "{result}");
    }
    assert_eq!(s.motion_traces.recomputations, count);
    assert_eq!(kept_trace_fit(&s, trace.sketch), original);

    run(&mut s, "parameters.change", json!({"name":"coupler_length","expression":"55 mm"}));
    let updated = kept_trace_fit(&s, trace.sketch);
    let [q, _] = meet(Vec3::new(0.0, 20.0, 0.0), 55.0, Vec3::new(40.0, 0.0, 0.0), 50.0);
    assert!(near(updated[0], q * 2.0 - Vec3::new(0.0, 20.0, 0.0) + Vec3::Z * 10.0));
    assert_ne!(updated, original);
    assert_eq!(s.doc.assembly.traces[0].wire, trace.wire);
    assert_eq!(s.motion_traces.recomputations, count + 1, "a dimension edit resamples once");
    assert!(s.model.result(trace.sketch).unwrap().warning.is_none());

    run(&mut s, "parameters.change", json!({"changes":{"coupler_length":"2.5 * unit_length","unit_length":"24 mm"}}));
    let scaled = kept_trace_fit(&s, trace.sketch);
    assert!(scaled.iter().zip(&original).all(|(a, b)| near(*a, Vec3::new(b.x * 1.2, b.y * 1.2, 10.0))));
    run(&mut s, "parameters.change", json!({"name":"trace_sweep","expression":"180 deg"}));
    let half = kept_trace_fit(&s, trace.sketch);
    assert!(near(half[0], scaled[0]) && near(*half.last().unwrap(), scaled[72]));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
}

#[test]
fn associative_trace_reuses_pose_and_unrelated_edits_but_follows_drive_expressions() {
    let (mut s, p0, q0) = four_bar();
    let mid = (p0 + q0) * 0.5;
    run(&mut s, "parameters.add", json!({"name":"swing","expression":"90 deg"}));
    let r = run(
        &mut s,
        "motion.trace.keep",
        json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],
        "joint":"Crank","from":0,"to":"swing","samples":19}),
    );
    let id = r["sketch"].as_u64().unwrap();
    let original = kept_trace_fit(&s, id);
    let count = s.motion_traces.recomputations;
    for angle in [20, 45, 75, 10] {
        run(&mut s, "joint.drive", json!({"joint":"Crank","value":angle,"quiet":true}));
        assert_eq!(kept_trace_fit(&s, id), original);
    }
    run(&mut s, "parameters.add", json!({"name":"unrelated","expression":"123 mm"}));
    run(&mut s, "parameters.change", json!({"name":"unrelated","expression":"456 mm"}));
    assert_eq!(s.motion_traces.recomputations, count, "pose and unrelated edits must not resample");
    let mut preview = s.scratch();
    run(&mut preview, "joint.drive", json!({"joint":"Crank","value":80,"quiet":true}));
    assert_eq!(preview.motion_traces.recomputations, count, "scratch previews reuse the same valid inputs");
    run(&mut s, "parameters.change", json!({"name":"swing","expression":"180 deg"}));
    let updated = kept_trace_fit(&s, id);
    assert_ne!(updated, original);
    assert_eq!(s.motion_traces.recomputations, count + 1, "a parameter command may refresh twice; sample once");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(kept_trace_fit(&s, id), original);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(kept_trace_fit(&s, id), updated);
    assert_eq!(s.motion_traces.recomputations, count + 1, "undo and redo reuse the retained input results");
    run(&mut s, "parameters.rename", json!({"name":"swing","new_name":"travel"}));
    assert_eq!(s.motion_traces.recomputations, count + 1, "renaming a parameter leaves resolved inputs unchanged");
    let count = s.motion_traces.recomputations;
    let before = s.doc.clone();
    for invalid in [
        json!({"sketch":id,"samples":501}),
        json!({"sketch":id,"index":99}),
        json!({"sketch":id,"from":"missing_parameter"}),
        json!({"sketch":id,"joint":"Missing","from":0,"to":90}),
    ] {
        assert!(s.execute("motion.trace.edit", &invalid).is_err());
        assert_eq!(*s.doc, *before, "invalid trace edits must be atomic");
    }
    run(&mut s, "motion.trace.edit", json!({"sketch":id,"samples":19}));
    assert_eq!(s.motion_traces.recomputations, count, "an identical definition is reusable");
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc), "an identical edit must not refresh the model or add an undo step");
    run(&mut s, "motion.trace.edit", json!({"sketch":id,"auto_update":false}));
    run(&mut s, "parameters.change", json!({"name":"travel","expression":"270 deg"}));
    assert_eq!(kept_trace_fit(&s, id), updated, "detaching preserves the curve");
}

/// A trace whose joint has a limit expression that no longer works out says so, as the solve
/// does, instead of keeping the curve bounded by the limit's old numbers (#20242).
#[test]
fn associative_trace_warns_when_a_limit_expression_stops_working_out() {
    let (mut s, p0, q0) = four_bar();
    let mid = (p0 + q0) * 0.5;
    run(&mut s, "parameters.add", json!({"name":"reach","expression":"120 deg"}));
    run(&mut s, "joint.limits", json!({"joint":"Crank","min":0,"max":"reach"}));
    let r =
        run(&mut s, "motion.trace.keep", json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],"joint":"Crank","from":0,"to":90,"samples":19}));
    let id = r["sketch"].as_u64().unwrap();
    let warning = |s: &Session| s.model.results.iter().find(|r| r.id == id).and_then(|r| r.warning.clone()).unwrap_or_default();
    assert_eq!(warning(&s), "");
    run(&mut s, "parameters.change", json!({"name":"reach","expression":"-10 deg"}));
    assert!(warning(&s).contains("minimum comes out above the maximum"), "{}", warning(&s));
}

#[test]
fn associative_trace_follows_a_study_and_survives_serialization() {
    let (mut s, p0, q0) = four_bar();
    let mid = (p0 + q0) * 0.5;
    let study = run(
        &mut s,
        "motion.study",
        json!({"name":"Turn","steps":18,
        "keys":[{"joint":"Crank","points":[[0,0],[18,90]]}]}),
    );
    let r = run(&mut s, "motion.trace.keep", json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],"study":"Turn"}));
    let id = r["sketch"].as_u64().unwrap();
    let original = kept_trace_fit(&s, id);
    run(
        &mut s,
        "motion.study",
        json!({"study":study["study"],"name":"Turn","steps":36,
        "keys":[{"joint":"Crank","points":[[0,0],[36,180]]}]}),
    );
    assert_eq!(kept_trace_fit(&s, id).len(), 37);
    assert_ne!(kept_trace_fit(&s, id), original);
    let doc: solidcraft_doc::Document = serde_json::from_value(serde_json::to_value(&*s.doc).unwrap()).unwrap();
    let mut opened = Session::new(doc);
    run(&mut opened, "motion.trace.edit", json!({"sketch":id,"samples":25}));
    assert_eq!(kept_trace_fit(&opened, id).len(), 25);
    run(&mut opened, "motion.delete", json!({"study":"Turn"}));
    assert!(run(&mut opened, "motion.list", json!({}))["traces"][0]["error"].is_string());
    assert!(opened.model.result(id).unwrap().warning.as_ref().unwrap().contains("last successful"));
    let count = opened.motion_traces.recomputations;
    opened.refresh();
    assert_eq!(opened.motion_traces.recomputations, count, "a missing dependency must not retry on every refresh");
    run(&mut opened, "edit.undo", json!({}));
    assert!(run(&mut opened, "motion.list", json!({}))["traces"][0]["error"].is_null());
    assert!(opened.model.result(id).unwrap().warning.is_none());
}

#[test]
fn associative_trace_follows_a_resized_picked_face_and_keeps_downstream_paths_current() {
    let mut s = two_cubes();
    pin(&mut s, "revolute", "Pivot", "A", "B", Vec3::new(20.0, 0.0, 0.0), Vec3::Z);
    let r = run(
        &mut s,
        "motion.trace.keep",
        json!({"occurrence":"B","point":[25,5,10],
        "point_ref":{"occurrence":"B","face":[25,5,10]},"joint":"Pivot","from":0,"to":90,"samples":2}),
    );
    let id = r["sketch"].as_u64().unwrap();
    let original = kept_trace_fit(&s, id);
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "solid.pipe", json!({"path_sketch":id,"path":r["curves"],"diameter":1,"operation":"new","body_name":"TracePipe"}));
    let pipe_before = s.model.state().body("TracePipe").unwrap().outline_bounds();
    let comp = s.doc.find_component("B").unwrap();
    let height =
        s.doc.features.iter().find(|f| f.component == comp && matches!(f.kind, solidcraft_doc::FeatureKind::Box { .. })).unwrap().param_names[2]
            .clone();
    let count = s.motion_traces.recomputations;
    run(&mut s, "parameters.change", json!({"name":height,"expression":"15 mm"}));
    let updated = kept_trace_fit(&s, id);
    assert!(updated.iter().all(|p| (p.z - 15.0).abs() < 1e-6));
    assert_ne!(original, updated);
    assert_eq!(s.motion_traces.recomputations, count + 1);
    let model = s.model.state();
    assert_eq!(model.sketch(id).unwrap().sketch.wires.first().unwrap().fit, updated);
    let pipe_after = model.body("TracePipe").unwrap().outline_bounds();
    assert!((pipe_after.min.z - pipe_before.min.z - 5.0).abs() < 1e-6);
}

#[test]
fn associative_trace_ignores_another_mechanism_on_the_same_ground() {
    let (mut s, p0, q0) = four_bar();
    part(&mut s, "Other", [100.0, 0.0, -5.0]);
    pin(&mut s, "revolute", "Other pivot", "Ground", "Other", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    let mid = (p0 + q0) * 0.5;
    run(
        &mut s,
        "motion.trace.keep",
        json!({"occurrence":"Coupler","point":[mid.x,mid.y,mid.z],
        "joint":"Crank","from":0,"to":180,"samples":19}),
    );
    let count = s.motion_traces.recomputations;
    run(&mut s, "joint.limits", json!({"joint":"Other pivot","min":-20,"max":30}));
    assert_eq!(s.motion_traces.recomputations, count);
}

/// The middle of a slider-crank's rod: halfway between the crank pin on its circle and the wrist
/// pin at a·cos θ + √(b² − (a·sin θ)²).
#[test]
fn slider_crank_rod_traces_its_closed_form_curve() {
    let (mut s, p0, s0) = slider_crank();
    let m0 = (p0 + s0) * 0.5;
    let r =
        run(&mut s, "motion.trace", json!({"occurrence": "Rod", "point": [m0.x, m0.y, m0.z], "joint": "Crank", "from": 0, "to": 360, "samples": 37}));
    let pts = traced(&r);
    assert_eq!(pts.len(), 37, "{r}");
    for (k, got) in pts.iter().enumerate() {
        let th = (90.0 + 10.0 * k as f64).to_radians();
        let x = 15.0 * th.cos() + (40.0f64.powi(2) - (15.0 * th.sin()).powi(2)).sqrt();
        let want = (Vec3::new(15.0 * th.cos(), 15.0 * th.sin(), 0.0) + Vec3::new(x, 0.0, 0.0)) * 0.5;
        assert!(near(*got, want), "{}°: {got:?} wants {want:?}", 10 * k);
    }
}

/// Driving the rocker past its swing stops where the loop can't close: the trace ends there.
#[test]
fn a_trace_ends_where_the_loop_stops_the_motion() {
    let (mut s, _, q0) = four_bar();
    let start = q0.y.atan2(q0.x - 45.0).to_degrees();
    let r = run(
        &mut s,
        "motion.trace",
        json!({"occurrence": "Rocker", "point": [q0.x, q0.y, q0.z], "joint": "Rocker", "from": 0, "to": 100, "samples": 21}),
    );
    assert!(r["stopped_by_loop"].is_object(), "{r}");
    let k = r["stopped_at"].as_u64().unwrap_or(u64::MAX) as usize;
    let pts = traced(&r);
    assert!(k > 0 && k < 20 && pts.len() == k + 1 && r["samples"] == k + 1, "{r}");
    let angle = |p: &Vec3| p.y.atan2(p.x - 45.0).to_degrees();
    for (i, p) in pts.iter().take(k).enumerate() {
        assert!((angle(p) - (start + 5.0 * i as f64)).abs() < 1e-6, "sample {i}: {}", angle(p));
    }
    let limit = ((900.0f64 - 3625.0) / 3600.0).acos().to_degrees();
    let last = pts.last().map(angle).unwrap_or(f64::NAN);
    assert!(last > limit - 5.0 && last <= limit + 1e-9, "stopped at {last}°, the limit is {limit}°");
}

/// Keep makes a 3D sketch spline through the trace, in one undo step.
#[test]
fn a_kept_trace_is_a_3d_sketch_spline() {
    let (mut s, p0, q0) = four_bar();
    let c0 = (p0 + q0) * 0.5;
    let p = json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "joint": "Crank", "from": 0, "to": 180, "samples": 19});
    let pts = traced(&run(&mut s, "motion.trace", p.clone()));
    let features = s.doc.features.len();
    let r = run(&mut s, "motion.trace.keep", p);
    assert_eq!((r["name"].as_str(), r["points"].as_u64()), (Some("Motion Trace1"), Some(19)), "{r}");
    assert!(s.active_sketch.is_none(), "the sketch is left");
    let id = r["sketch"].as_u64().unwrap_or(0);
    let sk = s.doc.sketch(id).cloned().unwrap_or_default();
    assert!(sk.view.three_d && sk.wires.len() == 1, "{r}");
    let fit = sk.wires.first().map(|w| w.fit.clone()).unwrap_or_default();
    assert!(fit.len() == pts.len() && fit.iter().zip(&pts).all(|(a, b)| a.dist(*b) < 1e-9), "{fit:?}");
    let again = json!({"occurrence": "Coupler", "point": [0, 0, 0], "joint": "Crank", "from": 0, "to": 90});
    assert_eq!(run(&mut s, "motion.trace.keep", again)["name"], "Motion Trace2");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.features.len(), features);
    // A point on the ground does not move: there is nothing to keep.
    let e = s.execute("motion.trace.keep", &json!({"occurrence": "Ground", "point": [0, 0, 0], "joint": "Crank", "from": 0, "to": 90})).unwrap_err();
    assert!(e.to_string().contains("does not move"), "{e}");
}

/// A full turn kept closes exactly on its start, also when the sample before the end lies a hair
/// from it (a study keyed to 359.99999° one step before 360°).
#[test]
fn a_kept_full_turn_closes_on_its_start() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    part(&mut s, "Arm", [0.0, -1.0, 0.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    pin(&mut s, "revolute", "Pivot", "Ground", "Arm", Vec3::ZERO, Vec3::Z);
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 5, "keys": [{"joint": "Pivot", "points": [[0, 0], [4, 359.99999], [5, 360]]}]}));
    let r = run(&mut s, "motion.trace.keep", json!({"occurrence": "Arm", "point": [1, 0, 0], "study": "Turn"}));
    let id = r["sketch"].as_u64().unwrap_or(0);
    let fit = s.doc.sketch(id).ok().and_then(|sk| sk.wires.first().map(|w| w.fit.clone())).unwrap_or_default();
    assert_eq!(fit.len(), 5, "{fit:?}");
    assert_eq!(fit.first(), fit.last(), "{fit:?}");
}

/// Kept while a component is active and shown moved (a move not captured yet): the spline goes
/// in the root, where no joint moves it, through the traced points; the component stays active.
#[test]
fn a_trace_kept_in_a_moved_component_stays_on_the_path() {
    let (mut s, p0, q0) = four_bar();
    let crank = occ(&s, "Crank");
    let c0 = (p0 + q0) * 0.5;
    let p = json!({"occurrence": "Coupler", "point": [c0.x, c0.y, c0.z], "joint": "Crank", "from": 0, "to": 90, "samples": 4});
    let pts = traced(&run(&mut s, "motion.trace", p.clone()));
    run(&mut s, "component.activate", json!({"component": "Crank", "occurrence": crank, "path": [crank]}));
    run(&mut s, "occurrence.move", json!({"occurrence": crank, "translate": [0, 0, 100], "capture": false}));
    let active = (s.active_component, s.active_occurrence_path.clone());
    let r = run(&mut s, "motion.trace.keep", p);
    assert_eq!((s.active_component, s.active_occurrence_path.clone()), active);
    let id = r["sketch"].as_u64().unwrap_or(0);
    assert_eq!(s.doc.feature(id).map(|f| f.component), Some(0), "{r}");
    let fit = s.doc.sketch(id).ok().and_then(|sk| sk.wires.first().map(|w| w.fit.clone())).unwrap_or_default();
    assert!(fit.len() == pts.len() && fit.iter().zip(&pts).all(|(a, b)| near(*a, *b)), "{fit:?} wants {pts:?}");
}

/// A part inside a sub-assembly placed twice: the occurrence path picks the instance traced. The
/// second Arm turns about its pivot and its Tip goes round; the first Arm, and so the Tip at its
/// first placement, stays put.
#[test]
fn a_trace_follows_the_instance_its_path_names() {
    let mut s = Session::default();
    part(&mut s, "Ground", [-1.0, -1.0, -5.0]);
    let g = occ(&s, "Ground");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let arm = run(&mut s, "component.create", json!({"name": "Arm"}));
    run(&mut s, "component.create", json!({"name": "Tip"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [10, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let second = run(&mut s, "occurrence.copy", json!({"component": arm["component"], "translate": [0, 50, 0]}))["occurrence"].as_u64().unwrap_or(0);
    let at = |o: Value| json!({"occurrence": o, "point": [0, 50, 0], "z": [0, 0, 1]});
    run(&mut s, "joint.create", json!({"type": "revolute", "a": at(json!(g)), "b": at(json!(second)), "flip": true, "name": "Pivot"}));
    let tip = occ(&s, "Tip");
    let trace = |s: &mut Session, o: Value| {
        traced(&run(s, "motion.trace", json!({"occurrence": o, "point": [11, 51, 1], "joint": "Pivot", "from": 0, "to": 90, "samples": 3})))
    };
    let turned = trace(&mut s, json!([second, tip]));
    let pivot = Vec3::new(0.0, 50.0, 1.0);
    assert!(turned.iter().all(|p| (p.dist(pivot) - 122f64.sqrt()).abs() < 1e-6), "{turned:?}");
    assert!(turned.first().zip(turned.last()).is_some_and(|(a, b)| (a.dist(*b) - 244f64.sqrt()).abs() < 1e-6), "a quarter turn: {turned:?}");
    // Kept with a point_ref as the Motion Trace dialog sends a pick on the second Arm: with the
    // pick's placement path. The trace's own occurrence names that placement already.
    let reference = json!({"occurrence": tip, "path": [second, tip], "point": [11, 51, 1]});
    let p = json!({"occurrence": [second, tip], "point": [11, 51, 1], "joint": "Pivot", "from": 0, "to": 90, "samples": 3, "point_ref": reference});
    let kept = run(&mut s, "motion.trace.keep", p);
    let trace_point = s.doc.assembly.traces.last().map(|t| t.point.clone());
    assert_eq!(trace_point.map(|o| (o.occurrence, o.inside)), Some((tip, Vec::new())), "{kept}");
    assert!(traced(&kept).iter().all(|p| (p.dist(pivot) - 122f64.sqrt()).abs() < 1e-6), "{kept}");
    let first = trace(&mut s, json!(tip));
    assert!(first.iter().all(|p| p.dist(Vec3::new(11.0, 51.0, 1.0)) < 1e-9), "{first:?}");
    assert!(s.execute("motion.trace", &json!({"occurrence": [tip, second], "point": [0, 0, 0], "joint": "Pivot", "from": 0, "to": 90})).is_err());
    assert!(s.execute("motion.trace", &json!({"occurrence": vec![second; 65], "point": [0, 0, 0], "joint": "Pivot", "from": 0, "to": 90})).is_err());
}

/// Trace params come from scripts and the control channel: bad ones are refused, never a panic.
#[test]
fn hostile_trace_params_are_refused() {
    let (mut s, _, _) = four_bar();
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 4, "keys": [{"joint": "Crank", "points": [[0, 0], [4, 90]]}]}));
    let base = json!({"occurrence": "Coupler", "point": [0, 0, 0], "joint": "Crank", "from": 0, "to": 90});
    let with = |k: &str, v: Value| {
        let mut p = base.clone();
        p[k] = v;
        p
    };
    let without = |k: &str| {
        let mut p = base.clone();
        p.as_object_mut().map(|o| o.remove(k));
        p
    };
    for p in [
        with("samples", json!(0)),
        with("samples", json!(1)),
        with("samples", json!(501)),
        with("samples", json!(1e300)),
        with("samples", json!(-3)),
        with("samples", json!("many")),
        with("point", json!([f64::MAX, 0, 0])),
        with("point", json!([0, 0])),
        with("point", json!("here")),
        without("point"),
        with("occurrence", json!("Nowhere")),
        without("occurrence"),
        with("joint", json!("Nowhere")),
        with("index", json!(1)),
        with("index", json!(-1)),
        with("from", json!("nonsense +")),
        with("to", json!(1e300)),
        without("from"),
        without("to"),
        with("study", json!("Turn")),
        with("study", json!(null)),
        without("joint"),
    ] {
        for cmd in ["motion.trace", "motion.trace.keep"] {
            assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        }
    }
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Ground", "b": "Crank", "name": "Lock"}));
    assert!(s.execute("motion.trace", &with("joint", json!("Lock"))).is_err(), "a rigid joint does not move");
}

#[path = "joints_coupling_tests.rs"]
mod coupling;

#[path = "joints_cam_tests.rs"]
mod cam;

// Plots and interference of motion studies, on the four-bar and slider-crank above.
#[path = "motion_analysis_tests.rs"]
mod motion_analysis;

// Joints swept until parts touch: a door, the four-bar above and a Watt six-bar.
#[path = "motion_sweep_tests.rs"]
mod motion_sweep;

/// A pin through a hole of its own radius whose axis is a hair off the hole's (parts placed from
/// rounded coordinates): the walls are all but tangent along two lines. Each offset gets a
/// checked answer: clear on the axis and within the kernel's same-surface tolerance, and
/// otherwise the crescent the pin shares with the plate, against the textbook lens of two circles. The pin is chamfered, so it is not a plain prism.
#[test]
fn interference_of_a_pin_a_hair_off_its_hole() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    for offset in [0.0, 1e-6, 5.9e-4, 1e-2] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 5, "corner": [-10, -10, 0], "body_name": "Plate"}));
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [0, 0, -2], "operation": "cut", "targets": ["Plate"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [offset, 0, -2], "operation": "new", "body_name": "Pin"}));
        run(&mut s, "solid.chamfer", json!({"edges": [[offset + 3.0, 0, 7]], "distance": 0.5, "body": "Pin"}));
        let i = run(&mut s, "inspect.interference", json!({"bodies": ["Plate", "Pin"]}));
        assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{offset}: {i}");
        if offset < 1e-5 {
            assert_eq!(i["count"], 0, "{offset}: {i}");
            continue;
        }
        assert_eq!(i["count"], 1, "{offset}: {i}");
        let want = 5.0 * crescent(3.0, offset);
        let got = i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
        assert!((got - want).abs() < 1e-6 * want, "{offset}: {got} against {want}: {i}");
    }
}

/// The pin's head has the radius of the bush the pin sits in, on nearly its axis but beside it:
/// two walls of one radius that never meet along the axis don't stop the crescent from counting.
#[test]
fn interference_of_a_headed_pin_a_hair_off_a_bush_of_its_heads_radius() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    let offset = 5.9e-4;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": 10, "base": [0, 0, 0], "operation": "new", "body_name": "Bush"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 14, "base": [0, 0, -2], "operation": "cut", "targets": ["Bush"]}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 8, "base": [offset, 0, -3], "operation": "new", "body_name": "Pin"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": 2, "base": [offset, 0, -5], "operation": "join", "targets": ["Pin"]}));
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["Bush", "Pin"]}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
    assert_eq!(i["count"], 1, "{i}");
    let want = 5.0 * crescent(3.0, offset);
    let got = i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
    assert!((got - want).abs() < 1e-6 * want, "{got} against {want}: {i}");
}

/// A headed pin made before the plate it sits in, a hair off the plate's hole: the crescent
/// whichever body comes first.
#[test]
fn interference_of_a_headed_pin_made_before_its_plate() {
    let crescent = |r: f64, e: f64| std::f64::consts::PI * r * r - (2.0 * r * r * (e / (2.0 * r)).acos() - e / 2.0 * (4.0 * r * r - e * e).sqrt());
    let offset = 5.6e-4;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 12, "base": [offset, 0, -2.35], "operation": "new", "body_name": "Pin"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5.25, "height": 3, "base": [offset, 0, -5.35], "operation": "join", "targets": ["Pin"]}));
    run(&mut s, "solid.box", json!({"length": 60, "width": 60, "height": 2, "corner": [-30, -30, -2.35], "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 9, "base": [0, 0, -6], "operation": "cut", "targets": ["Plate"]}));
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["Pin", "Plate"]}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
    assert_eq!(i["count"], 1, "{i}");
    let want = 2.0 * crescent(3.0, offset);
    let got = i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0);
    assert!((got - want).abs() < 1e-6 * want, "{got} against {want}: {i}");
}

/// A grounded Base (a 40 x 40 x 10 box), a Pin extruded 20 mm from the sketch circle `c1` of
/// `PinSketch` (r5 at x = 100) and a Cap box; a revolute Hinge from the Base's top to the Pin's
/// top rim, its offset the parameter `gap`, and a rigid Fix between two points of Base and Cap.
fn hinge_and_fix() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "2 mm"}));
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]}));
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Pin"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PinSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cap"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 0, 0]}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "Hinge", "offset": "gap", "limits": [[-90, 90]],
               "a": {"occurrence": "Base", "face": [20, 20, 10]}, "b": {"occurrence": "Pin", "circle": [105, 0, 20]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Fix", "a": {"occurrence": "Base", "point": [0, 0, 0]}, "b": {"occurrence": "Cap", "point": [200, 0, 0]}}),
    );
    s
}

/// The reference of `role` on side `side` of an inspected joint.
fn reference<'a>(j: &'a Value, side: &str, role: &str) -> &'a Value {
    j[side]["references"].as_array().and_then(|r| r.iter().find(|r| r["role"] == role)).unwrap_or(&Value::Null)
}

/// `joint.inspect` gives each joint's definition: its kind, offset (and the expression it
/// follows), limits and values, and for each side the occurrence and component, the snap as
/// saved, and what its face names refer to: a circle's wall back to the extrude and the sketch
/// curve it was swept from.
#[test]
fn joint_inspect_gives_what_each_joint_is_attached_to() {
    let mut s = hinge_and_fix();
    let all = run(&mut s, "joint.inspect", json!({}));
    let joints = all["joints"].as_array().cloned().unwrap_or_default();
    assert_eq!(joints.iter().map(|j| j["name"].clone()).collect::<Vec<_>>(), [json!("Hinge"), json!("Fix")], "{all}");

    let hinge = run(&mut s, "joint.inspect", json!({"joint": "Hinge"}))["joints"][0].clone();
    assert_eq!((&hinge["type"], &hinge["offset"], &hinge["expressions"]["offset"]), (&json!("revolute"), &json!(2.0), &json!("gap")), "{hinge}");
    assert_eq!(hinge["values"][0]["limits"], json!([-90.0, 90.0]), "{hinge}");
    assert_eq!(hinge["dofs"], json!(["angle"]));
    assert_eq!((&hinge["a"]["occurrence_name"], &hinge["a"]["component_name"]), (&json!("Base:1"), &json!("Base")), "{hinge}");
    assert_eq!(hinge["a"]["path"], hinge["a"]["occurrence_name"]);
    assert_eq!((&hinge["a"]["snap"]["type"], &hinge["b"]["snap"]["type"]), (&json!("face_center"), &json!("circle_center")), "{hinge}");
    assert_eq!((&hinge["a"]["resolves"], &hinge["b"]["resolves"]), (&json!(true), &json!(true)), "{hinge}");
    assert_eq!(hinge["b"]["component_name"], "Pin");
    // The Pin's wall: the extrude's side swept from PinSketch's c1.
    let wall = reference(&hinge, "b", "face");
    assert_eq!(wall["feature"]["type"], "ExtrudeFeature", "{hinge}");
    assert_eq!(
        (&wall["sketch"]["name"], &wall["sketch"]["curve"], &wall["sketch"]["curve_type"]),
        (&json!("PinSketch"), &json!("c1"), &json!("circle")),
        "{hinge}"
    );
    assert_eq!(wall["name"].as_str().map(|n| n.contains(":side:c1")), Some(true), "{hinge}");
    // The rim's end face is the extrude's top.
    assert_eq!(reference(&hinge, "b", "end")["feature"], wall["feature"], "{hinge}");
    // The Base's top: a face of its box, no sketch.
    assert!(reference(&hinge, "a", "face")["feature"]["id"].is_u64() && reference(&hinge, "a", "face")["sketch"].is_null(), "{hinge}");

    let fix = &joints[1];
    assert_eq!((&fix["type"], &fix["values"], &fix["dofs"]), (&json!("rigid"), &json!([]), &json!([])), "{fix}");
    assert_eq!((&fix["b"]["snap"]["type"], &fix["b"]["snap"]["point"]), (&json!("point"), &json!([200.0, 0.0, 0.0])), "{fix}");
    assert_eq!(fix["b"]["references"], json!([]));
    assert!(fix.get("expressions").is_none(), "{fix}");

    // A joint that isn't there is an error.
    assert!(s.execute("joint.inspect", &json!({"joint": "Nope"})).is_err());
}

/// A joint whose sketch circle is replaced: its origin's face is gone, so it no longer
/// resolves; the inspection says why and that its curve is no longer in the sketch.
#[test]
fn joint_inspect_reports_a_lost_origin() {
    let mut s = hinge_and_fix();
    run(&mut s, "sketch.edit", json!({"sketch": "PinSketch"}));
    run(&mut s, "sketch.delete", json!({"entities": ["c1"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c2"}));
    run(&mut s, "sketch.finish", json!({}));
    let hinge = run(&mut s, "joint.inspect", json!({"joint": "Hinge"}))["joints"][0].clone();
    assert_eq!(hinge["b"]["resolves"], false, "{hinge}");
    let wall = reference(&hinge, "b", "face");
    let name = wall["name"].as_str().unwrap_or_default();
    assert!(hinge["b"]["error"].as_str().is_some_and(|e| e.contains(name)), "{hinge}");
    assert_eq!(wall["sketch"]["curve"], "c1", "{hinge}");
    assert!(wall["sketch"]["error"].is_string(), "{hinge}");
    assert!(!hinge["errors"].as_array().is_none_or(Vec::is_empty), "the solve says so too: {hinge}");
    // The Base side is still found.
    assert_eq!(hinge["a"]["resolves"], true);
}

/// An edge whose first face is a fillet's: the fillet face's name holds the edge it rounds
/// (`F2:blend(<face>|<face>)`), so the edge's two faces are the whole blend name and the slot's
/// wall, not the blend name cut at its own `|`.
#[test]
fn joint_inspect_splits_an_edge_of_a_blend_face_into_its_two_faces() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 20, 10]], "radius": 3}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 4, "height": 20, "corner": [32, 18, -5], "operation": "cut"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cap"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 0, 0]}));
    // On the slot's wall y = 18, halfway round the fillet (axis x = 37, z = 7, r = 3).
    let h = 3.0 * std::f64::consts::FRAC_1_SQRT_2;
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "OnBlend", "a": {"occurrence": "Base", "edge": [37.0 + h, 18, 7.0 + h]},
               "b": {"occurrence": "Cap", "face": [205, 5, 5]}}),
    );
    let j = run(&mut s, "joint.inspect", json!({"joint": "OnBlend"}))["joints"][0].clone();
    let edge = reference(&j, "a", "edge");
    let name = edge["name"].as_str().unwrap_or_default().to_string();
    let faces: Vec<String> = edge["faces"].as_array().into_iter().flatten().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
    // `F4:blend(F3:+x|F3:+z#[F3:-y,F5:-y])|F5:-y`: the blend piece beside the slot, and the slot's wall.
    assert!(name.split_once(':').is_some_and(|(_, r)| r.starts_with("blend(")), "the blend face comes first: {name}");
    assert_eq!(faces.len(), 2, "{edge}");
    assert_eq!(format!("{}|{}", faces[0], faces[1]), name, "{edge}");
    assert!(faces[0].contains(":blend(") && faces[0].contains('|'), "{edge}");
    assert!(!faces[1].contains('|'), "{edge}");
}

/// A joint on a pattern's copy of a pin: the copy's wall (`F3:side:c1@F4.1`) still names the
/// sketch curve it was swept from, `c1`.
#[test]
fn joint_inspect_finds_the_sketch_curve_of_a_copied_side() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Pins"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PinSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20, "name": "Pin"}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Pin"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cap"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [200, 0, 0]}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "name": "OnCopy", "a": {"occurrence": "Pins", "circle": [35, 0, 20]}, "b": {"occurrence": "Cap", "face": [205, 5, 5]}}),
    );
    let j = run(&mut s, "joint.inspect", json!({"joint": "OnCopy"}))["joints"][0].clone();
    let wall = reference(&j, "a", "face");
    assert!(wall["name"].as_str().is_some_and(|n| n.contains(":side:c1@F")), "a copy's wall: {j}");
    assert_eq!((&wall["sketch"]["curve"], &wall["sketch"]["name"]), (&json!("c1"), &json!("PinSketch")), "{j}");
    assert!(wall["sketch"]["error"].is_null(), "{j}");
}

/// `joint.inspect` reads only: no undo step, the design unchanged, and the id agents' gates
/// treat as reading (`*.inspect`).
#[test]
fn joint_inspect_reads_only() {
    let mut s = hinge_and_fix();
    let spec = crate::cmd::command_specs().into_iter().find(|c| c.id == "joint.inspect");
    assert!(spec.is_some_and(|c| !c.undoable && c.tab.is_empty()));
    let undo = run(&mut s, "document.inspect", json!({}))["undo"].clone();
    let before = serde_json::to_value(&s.doc).unwrap_or_default();
    run(&mut s, "joint.inspect", json!({}));
    run(&mut s, "joint.inspect", json!({"joint": "Fix"}));
    assert_eq!(run(&mut s, "document.inspect", json!({}))["undo"], undo);
    assert_eq!(serde_json::to_value(&s.doc).unwrap_or_default(), before);
}

/// Trial placements use exactly the same constraints as a full solve. Only the expensive DOF
/// report is optional: loop stops, errors and the resulting pose must not depend on it.
#[test]
fn placement_analysis_preserves_held_loop_constraints_and_diagnostics() {
    use solidcraft_doc::joints::{self, Analysis};
    use std::collections::BTreeMap;

    let (s, _, _) = four_bar();
    let rocker = s.doc.assembly.joints.iter().find(|j| j.name == "Rocker").unwrap().id;
    let held = BTreeMap::from([((rocker, 0), 0.0)]);
    for angle in [-10.0_f64, 40.0, 100.0] {
        let mut doc = (*s.doc).clone();
        doc.assembly.joints.iter_mut().find(|j| j.id == rocker).unwrap().values[0] = angle.to_radians();
        let full = joints::solve_holding(&doc, &s.model.state(), &held);
        let placement = joints::solve_holding_with_analysis(&doc, &s.model.state(), &held, Analysis::Placement);
        assert!(placement.dof.is_empty(), "trial placement must not compute DOF");
        assert_eq!(placement.world, full.world);
        assert_eq!(placement.values, full.values);
        assert_eq!(placement.fixed, full.fixed);
        assert_eq!(placement.conflicts, full.conflicts);
        assert_eq!(placement.errors, full.errors);
        let mut placed = doc.clone();
        placed.apply_joint_solution(&placement);
        let mut reference = doc;
        reference.apply_joint_solution(&full);
        assert_eq!(placed, reference);
        assert_eq!(joints::solve(&placed, &s.model.state()).dof, joints::solve(&reference, &s.model.state()).dof);
    }
    // A degenerate origin remains an error even in a placement-only trial.
    let mut broken = (*s.doc).clone();
    broken.assembly.joints[0].b.snap = solidcraft_doc::joints::Snap::Point { point: Vec3::ZERO, z: Some(Vec3::ZERO), x: None };
    let full = joints::solve(&broken, &s.model.state());
    let placement = joints::solve_holding_with_analysis(&broken, &s.model.state(), &BTreeMap::new(), Analysis::Placement);
    assert!(!full.errors.is_empty());
    assert_eq!(placement.errors, full.errors);
    assert_eq!(placement.conflicts, full.conflicts);
}

/// Ordinary commands publish one complete report; refresh only updates constraint placements.
/// Counts are deterministic work assertions, not timing thresholds tied to a test machine.
#[test]
fn joint_drive_and_refresh_do_not_repeat_dof_analysis() {
    let (mut s, _, _) = four_bar();
    for (angle, quiet) in [(17, false), (58, true), (105, false)] {
        super::solve_counts::take();
        let r = run(&mut s, "joint.drive", json!({"joint":"Crank", "value":angle, "quiet":quiet}));
        let (all, full) = super::solve_counts::take();
        assert!(all > full, "placement trials must still run");
        assert_eq!(full, usize::from(!quiet), "only a published DOF report needs full analysis");
        let expected = super::sol_json(&solidcraft_doc::joints::solve(&s.doc, &s.model.state()), &json!({"quiet":quiet}));
        for key in ["dof", "fixed", "errors", "conflicts"] {
            assert_eq!(r.get(key), expected.get(key), "{key}: {r}");
        }
        let before = s.doc.clone();
        s.refresh();
        assert_eq!(super::solve_counts::take().1, 0, "refresh discards the DOF report");
        assert_eq!(*s.doc, *before);
    }
}

/// A parameter change solves the joints once: the refresh that follows the command finds the
/// design and the model as the command's own solve left them (the sketch the parameter drives
/// has its new solution stored after that solve). A move the joints undo is still solved.
#[test]
fn a_parameter_change_solves_the_joints_once() {
    let mut s = Session::default();
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "300 mm"}));
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": "W", "width": 100, "height": 1, "corner": [0, 0, 0]}));
    run(&mut s, "solid.hole", json!({"position": ["W / 2", 50, 1], "diameter": 6}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let bottom = r["curves"][0].as_str().unwrap_or_default().to_string();
    run(&mut s, "sketch.dimension", json!({"entities": [bottom], "value": "W / 10"}));
    run(&mut s, "sketch.finish", json!({}));
    let plate = occ(&s, "Plate");
    run(&mut s, "occurrence.ground", json!({"occurrence": plate, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 2, "corner": [100, 200, 0]}));
    run(&mut s, "solid.hole", json!({"position": [110, 210, 2], "diameter": 6}));
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Plate", "circle": [153, 50, 0.8], "x": [1, 0, 0]},
               "b": {"occurrence": "Bracket", "circle": [113, 210, 0.4], "x": [1, 0, 0]}}),
    );
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    run(&mut s, "component.activate", json!({"component": "root"}));
    for w in [400.0, 360.0] {
        super::solve_counts::take();
        run(&mut s, "parameters.change", json!({"name": "W", "expression": format!("{w} mm")}));
        assert_eq!(super::solve_counts::take().0, 1, "W = {w}");
        let at = place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0));
        assert!(near(at, Vec3::new(w / 2.0, 50.0, 1.0)), "W = {w}: the bracket follows the hole, {at:?}");
    }
    // Moving the bracket changes no feature, so the model stays; the joint still takes it back.
    let bracket = occ(&s, "Bracket");
    super::solve_counts::take();
    run(&mut s, "occurrence.move", json!({"occurrence": bracket, "translate": [5, 0, 0], "capture": true}));
    assert!(super::solve_counts::take().0 > 0);
    assert!(near(place(&s, "Bracket", Vec3::new(110.0, 210.0, 0.0)), Vec3::new(180.0, 50.0, 1.0)));
}

#[test]
fn driving_one_cylindrical_coordinate_preserves_the_other_expression_and_reports_continuous() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    run(&mut s, "parameters.add", json!({"name":"turn","expression":"30 deg"}));
    let joint = run(
        &mut s,
        "joint.create",
        json!({"type":"cylindrical","a":{"occurrence":0,"point":[0,0,0]},"b":{"occurrence":"Part","point":[0,0,0]},"flip":true,"values":["turn",0]}),
    )["joint"]
        .as_u64()
        .unwrap();
    let before = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap().exprs.value(0).map(str::to_string);
    assert!(before.is_some());
    let reply = run(&mut s, "joint.drive", json!({"joint":joint,"index":1,"value":12,"continuous":true,"quiet":true}));
    assert_eq!(reply["continuous"], true);
    let j = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap();
    assert_eq!(j.exprs.value(0).map(str::to_string), before);
    assert!((j.values[0].to_degrees() - 30.).abs() < 1e-7);
    assert!((j.values[1] - 12.).abs() < 1e-7);
    run(&mut s, "parameters.change", json!({"name":"turn","expression":"40 deg"}));
    let j = s.doc.assembly.joints.iter().find(|j| j.id == joint).unwrap();
    assert!((j.values[0].to_degrees() - 40.).abs() < 1e-7, "the preserved expression still drives the angle");
    assert!((j.values[1] - 12.).abs() < 1e-7);
    for bad in [json!(-1), json!(2), json!(1000000), json!("0")] {
        assert!(s.execute("joint.drive", &json!({"joint":joint,"index":bad,"value":0})).is_err());
    }
}

/// #10747: driving a joint to 30 reports 30, not 29.999999999999996; the joint keeps
/// full precision.
#[test]
fn driving_an_angle_reports_the_number_a_person_would_read() {
    let mut s = two_cubes();
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": "A", "point": [5.0, 5.0, 10.0]}, "b": {"occurrence": "B", "point": [25.0, 5.0, 10.0]}, "name": "Turn"}),
    );
    let r = run(&mut s, "joint.drive", json!({"joint": "Turn", "value": 30}));
    assert_eq!(r["values"][0]["value"], json!(30.0), "{r}");
    assert_eq!(r["requested"][0]["value"], json!(30.0), "{r}");
    // Only what is reported is rounded: the stored value keeps full precision.
    let j = s.doc.assembly.joints.iter().find(|j| j.name == "Turn").unwrap();
    assert_ne!(j.values[0].to_degrees(), 30.0, "the stored angle keeps its bits");
}

#[test]
fn new_joints_generate_type_prefixed_names_or_keep_custom_name() {
    let mut s = two_cubes();
    let (a, b) = (occ(&s, "A"), occ(&s, "B"));

    // 1. Without name: revolute joint defaults to "Revolute Joint1"
    let r1 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "revolute",
            "a": {"occurrence": a, "face": [5, 5, 10]},
            "b": {"occurrence": b, "face": [25, 5, 0]}
        }),
    );
    assert_eq!(r1["name"], "Revolute Joint1");

    // 2. Without name: slider joint defaults to "Slider Joint1"
    let r2 = run(
        &mut s,
        "joint.as_built",
        json!({
            "type": "slider",
            "a": "A",
            "b": "B"
        }),
    );
    assert_eq!(r2["name"], "Slider Joint1");

    // 3. With custom name: preserves custom name exactly
    let r3 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "rigid",
            "name": "CustomRigid",
            "a": {"occurrence": a, "face": [5, 5, 10]},
            "b": {"occurrence": b, "face": [25, 5, 0]}
        }),
    );
    assert_eq!(r3["name"], "CustomRigid");
}

/// A sub-assembly placed twice: Board (holding Header, a 10 mm cube from the origin, turned 90
/// degrees about Z and moved 20 mm along Y in Board, so its top's centre is at (-5, 25, 10)) at
/// the origin and again at +100 mm X, both grounded, and a free 2 mm Probe cube from the origin.
/// Returns (Board:1, Board:2, Header:1) occurrence ids.
fn board_twice(s: &mut Session) -> (u64, u64, u64) {
    run(s, "component.create", json!({"name": "Board", "parent": "root"}));
    run(s, "component.create", json!({"name": "Header", "parent": "Board"}));
    run(s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Pin"}));
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "occurrence.move", json!({"occurrence": "Header:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]}));
    run(s, "occurrence.move", json!({"occurrence": "Header:1", "translate": [0, 20, 0]}));
    let second = run(s, "occurrence.copy", json!({"component": "Board", "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    run(s, "occurrence.ground", json!({"occurrence": second, "grounded": true}));
    run(s, "component.create", json!({"name": "Probe", "parent": "root", "grounded": false}));
    run(s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "ProbeBox"}));
    run(s, "component.activate", json!({"component": "root"}));
    (occ(s, "Board"), second, occ(s, "Header"))
}

/// The Probe's centre in the world.
fn probe_centre(s: &Session) -> Vec3 {
    place(s, "Probe", Vec3::new(1.0, 1.0, 1.0))
}

#[test]
fn a_joint_origin_on_a_repeated_sub_assembly_stays_on_the_copy_it_was_picked_on() {
    // #11426: Header sits in Board, which is placed twice. A pick on the second Board's header
    // was stored on Header's one occurrence and placed through the first Board, so a part mated
    // to it landed on the first copy (-5, 25, 11) instead of the one picked (95, 25, 11).
    for how in ["path", "no occurrence"] {
        let mut s = Session::default();
        let (_, second, header) = board_twice(&mut s);
        let mut a = json!({"name": "Top", "face": [95, 25, 10]});
        if how == "path" {
            a["occurrence"] = json!(header);
            a["path"] = json!([second, header]);
        }
        run(&mut s, "joint.origin", a);
        let top = s.doc.assembly.origins.iter().find(|o| o.name == "Top").map(|o| o.origin.clone()).unwrap();
        // On the second Board, reaching its header: the copy it was picked on, its face by name.
        assert_eq!((top.occurrence, top.inside.clone()), (second, vec![header]), "{how}: {top:?}");
        assert!(matches!(&top.snap, solidcraft_doc::joints::Snap::FaceCenter { face: Some(_), .. }), "{how}: {top:?}");
        run(&mut s, "joint.create", json!({"type": "rigid", "a": "Top", "b": {"occurrence": "Probe", "face": [1, 1, 0]}}));
        assert!(near(probe_centre(&s), Vec3::new(95.0, 25.0, 11.0)), "{how}: {:?}", probe_centre(&s));
        // joint.inspect lists the occurrences inside the copy; a side with none leaves the key out.
        let sides = run(&mut s, "joint.inspect", json!({}))["joints"][0].clone();
        assert_eq!((&sides["a"]["occurrence"], &sides["a"]["inside"]), (&json!(second), &json!([header])), "{how}: {sides}");
        assert!(sides["b"].get("inside").is_none(), "{how}: {sides}");
        // Filtering by the part it was picked on finds it, as filtering by the copy does.
        for named in [header, second] {
            let kept = run(&mut s, "joint.list", json!({"occurrences": [named]}));
            assert_eq!(kept["joints"].as_array().map(Vec::len), Some(1), "{how}, {named}: {kept}");
        }
        // It follows that copy when it moves, and keeps it through saving and opening again.
        run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [0, 20, 0], "move_grounded": true}));
        assert!(near(probe_centre(&s), Vec3::new(95.0, 45.0, 11.0)), "{how}: {:?}", probe_centre(&s));
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        *s.doc_mut() = doc;
        run(&mut s, "joint.list", json!({}));
        let reopened = s.doc.assembly.origins.iter().find(|o| o.name == "Top").map(|o| o.origin.clone()).unwrap();
        assert_eq!(reopened, top, "{how}");
        let world = s.doc.origin_world(&s.model.state(), &reopened).unwrap();
        assert!(near(Vec3::new(world[3][0], world[3][1], world[3][2]), Vec3::new(95.0, 45.0, 10.0)), "{how}: {world:?}");
    }
}

#[test]
fn a_joint_origin_on_the_first_copy_is_stored_as_before() {
    // Through first placements only, an origin is its part's occurrence with nothing inside, as
    // every origin was before #11426, given a path or not.
    let mut s = Session::default();
    let (first, _, header) = board_twice(&mut s);
    for (name, path) in [("ByPath", Some(json!([first, header]))), ("ByPick", None)] {
        let mut p = json!({"name": name, "occurrence": header, "face": [-5, 25, 10]});
        if let Some(path) = path {
            p["path"] = path;
        }
        run(&mut s, "joint.origin", p);
        let o = s.doc.assembly.origins.iter().find(|o| o.name == name).map(|o| o.origin.clone()).unwrap();
        assert_eq!((o.occurrence, o.inside.len()), (header, 0), "{name}: {o:?}");
        assert!(!s.doc.to_json().contains("\"inside\""), "{name}: an empty `inside` is not written");
    }
    run(&mut s, "joint.create", json!({"type": "rigid", "a": "ByPath", "b": {"occurrence": "Probe", "face": [1, 1, 0]}}));
    assert!(near(probe_centre(&s), Vec3::new(-5.0, 25.0, 11.0)), "{:?}", probe_centre(&s));
    let sides = run(&mut s, "joint.inspect", json!({}))["joints"][0].clone();
    assert!(sides["a"].get("inside").is_none(), "{sides}");
}

#[test]
fn a_joint_origin_path_is_checked() {
    let mut s = Session::default();
    let (first, second, header) = board_twice(&mut s);
    let probe = occ(&s, "Probe");
    let long: Vec<u64> = (0..65).collect();
    for path in [
        json!([]),
        json!([999]),
        json!([header, second]),
        json!([second]),
        json!([first, header, header]),
        json!(long),
        json!("6/4"),
        json!([-1]),
        json!([1.5]),
    ] {
        let p = json!({"occurrence": header, "face": [95, 25, 10], "path": path});
        assert!(s.execute("joint.origin", &p).is_err(), "{p}");
    }
    // A path that ends at another occurrence than the one named.
    assert!(s.execute("joint.origin", &json!({"occurrence": probe, "face": [95, 25, 10], "path": [second, header]})).is_err());
    assert!(s.doc.assembly.origins.is_empty());
    // `null` is left out, as for the other optional keys: the pick finds its copy.
    run(&mut s, "joint.origin", json!({"name": "Top", "face": [95, 25, 10], "path": null}));
    let top = s.doc.assembly.origins.iter().find(|o| o.name == "Top").map(|o| o.origin.clone()).unwrap();
    assert_eq!((top.occurrence, top.inside), (second, vec![header]));
}

#[test]
fn a_placement_with_no_inverse_does_not_stop_picks_on_other_parts() {
    // A hand-edited file can hold a placement that can't be undone. The search for the placement
    // a pick is on skips it, instead of failing every pick in the design.
    let mut s = Session::default();
    let (_, second, header) = board_twice(&mut s);
    let probe = occ(&s, "Probe");
    if let Some(o) = s.doc_mut().occurrences.iter_mut().find(|o| o.id == probe) {
        o.transform = [[0.0; 4]; 4];
    }
    run(&mut s, "joint.origin", json!({"name": "Top", "face": [95, 25, 10]}));
    let top = s.doc.assembly.origins.iter().find(|o| o.name == "Top").map(|o| o.origin.clone()).unwrap();
    assert_eq!((top.occurrence, top.inside), (second, vec![header]));
}

#[test]
fn removing_the_part_inside_a_joint_origin_removes_its_joints() {
    let mut s = Session::default();
    let (_, second, header) = board_twice(&mut s);
    run(&mut s, "joint.origin", json!({"name": "Top", "occurrence": header, "path": [second, header], "face": [95, 25, 10]}));
    run(&mut s, "joint.create", json!({"type": "rigid", "a": "Top", "b": {"occurrence": "Probe", "face": [1, 1, 0]}}));
    run(&mut s, "component.delete", json!({"component": "Header"}));
    assert!(s.doc.assembly.origins.is_empty(), "{:?}", s.doc.assembly.origins);
    assert!(s.doc.assembly.joints.is_empty(), "{:?}", s.doc.assembly.joints);
}

#[test]
fn paste_new_takes_a_joint_origin_inside_a_repeated_sub_assembly_to_its_copy() {
    // Top holds Board twice (+100 mm X), each Board holds Header (a 10 mm cube, turned and moved
    // in Board as `board_twice` does). An origin on the
    // second Board's header is on that Board, inside Header. Paste New of Top copies it onto the
    // copy's second Board and the copy's Header, not the originals.
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Top", "parent": "root"}));
    run(&mut s, "component.create", json!({"name": "Board", "parent": "Top"}));
    run(&mut s, "component.create", json!({"name": "Header", "parent": "Board"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Pin"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Header:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Header:1", "translate": [0, 20, 0]}));
    let second = run(&mut s, "occurrence.copy", json!({"component": "Board", "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    let (top, header) = (occ(&s, "Top"), occ(&s, "Header"));
    run(&mut s, "joint.origin", json!({"name": "Top pin", "path": [top, second, header], "face": [95, 25, 10]}));
    let origin = s.doc.assembly.origins.iter().find(|o| o.name == "Top pin").map(|o| o.origin.clone()).unwrap();
    assert_eq!((origin.occurrence, origin.inside.clone()), (second, vec![header]), "{origin:?}");
    run(&mut s, "component.paste_new", json!({"component": "Top", "translate": [0, 200, 0]}));
    let copied: Vec<_> = s.doc.assembly.origins.iter().filter(|o| o.name != "Top pin").map(|o| o.origin.clone()).collect();
    let [copy] = copied.as_slice() else { panic!("one copied origin: {:?}", s.doc.assembly.origins) };
    assert!(copy.occurrence != second && copy.inside.len() == 1 && copy.inside != origin.inside, "{copy:?}");
    let world = s.doc.origin_world(&s.model.state(), copy).unwrap();
    assert!(near(Vec3::new(world[3][0], world[3][1], world[3][2]), Vec3::new(95.0, 225.0, 10.0)), "{world:?}");
}

/// Origin plane and axis sides: the design's own XY and YZ planes and its X and Z axes (the
/// root keeps its `PlaneRef::Origin` / `AxisTarget::Origin` controls), resolved at every solve.
#[test]
fn joint_sides_take_origin_datums() {
    let mut s = Session::default();

    // 1. Planar joint to origin plane XY: bottom face at z=10 drops to z=0, retaining x=60, y=90.
    run(&mut s, "component.create", json!({"name": "PartA", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [50, 80, 10]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_a = occ(&s, "PartA");
    let r1 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "planar",
            "a": {"plane": "XY"},
            "b": {"occurrence": part_a, "face": [60, 90, 10]}
        }),
    );
    assert_eq!(r1["conflicts"], json!([]));
    assert!(near(place(&s, "PartA", Vec3::new(60.0, 90.0, 10.0)), Vec3::new(60.0, 90.0, 0.0)));

    // 2. Planar joint to origin plane YZ (normal along X): face at x=50 moves to x=0, retaining y=90, z=15.
    run(&mut s, "component.create", json!({"name": "PartB", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [50, 80, 10]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_b = occ(&s, "PartB");
    let r2 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "planar",
            "a": {"plane": "YZ"},
            "b": {"occurrence": part_b, "face": [50, 90, 15]}
        }),
    );
    assert_eq!(r2["conflicts"], json!([]));
    assert!(near(place(&s, "PartB", Vec3::new(50.0, 90.0, 15.0)), Vec3::new(0.0, 90.0, 15.0)));

    // 3. Slider joint along origin axis Z.
    run(&mut s, "component.create", json!({"name": "PartC", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [50, 80, 10]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_c = occ(&s, "PartC");
    let r3 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "slider",
            "a": {"axis": "Z"},
            "b": {"occurrence": part_c, "face": [60, 90, 10]}
        }),
    );
    assert_eq!(r3["conflicts"], json!([]));

    // 4. Cylindrical joint along origin axis X.
    run(&mut s, "component.create", json!({"name": "PartD", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [50, 80, 10]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_d = occ(&s, "PartD");
    let r4 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "cylindrical",
            "a": {"axis": "X"},
            "b": {"occurrence": part_d, "face": [50, 90, 15]}
        }),
    );
    assert_eq!(r4["conflicts"], json!([]));
}

#[test]
fn joint_with_construction_plane_and_axis() {
    let mut s = Session::default();
    let wp = run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 15, "name": "WorkPlane"}));
    run(
        &mut s,
        "construct.axis.two_points",
        json!({
            "a": {"point": [0, 0, 0]},
            "b": {"point": [0, 0, 50]},
            "name": "WorkAxis"
        }),
    );
    let plane_feat = wp["feature"].as_u64().unwrap();

    // 1. Planar joint to construction plane by name: bottom face at z=0 moves to plane offset z=15.
    run(&mut s, "component.create", json!({"name": "PartA", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_a = occ(&s, "PartA");
    let r1 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "planar",
            "a": {"plane": "WorkPlane"},
            "b": {"occurrence": part_a, "face": [25, 5, 0]}
        }),
    );
    assert_eq!(r1["conflicts"], json!([]));
    assert!(near(place(&s, "PartA", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(25.0, 5.0, 15.0)));

    // 2. Structured construction plane reference by feature id.
    run(&mut s, "component.create", json!({"name": "PartB", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_b = occ(&s, "PartB");
    let r2 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "planar",
            "a": {"plane": {"type": "construction", "feature": plane_feat}},
            "b": {"occurrence": part_b, "face": [25, 5, 0]}
        }),
    );
    assert_eq!(r2["conflicts"], json!([]));
    assert!(near(place(&s, "PartB", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(25.0, 5.0, 15.0)));

    // 3. Revolute joint around construction axis by name.
    run(&mut s, "component.create", json!({"name": "PartC", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_c = occ(&s, "PartC");
    let r3 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "revolute",
            "a": {"axis": "WorkAxis"},
            "b": {"occurrence": part_c, "face": [25, 5, 0]}
        }),
    );
    assert_eq!(r3["conflicts"], json!([]));

    // 4. Structured construction axis reference.
    run(&mut s, "component.create", json!({"name": "PartD", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_d = occ(&s, "PartD");
    let r4 = run(
        &mut s,
        "joint.create",
        json!({
            "type": "revolute",
            "a": {"axis": {"type": "construction", "name": "WorkAxis"}},
            "b": {"occurrence": part_d, "face": [25, 5, 0]}
        }),
    );
    assert_eq!(r4["conflicts"], json!([]));
}

/// A construction axis belongs to the component that owns it: a side on another component by
/// that name is refused (no cross-component fallback), a side on its own resolves.
#[test]
fn construction_datums_stay_bound_to_their_component() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "PartA"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new"}));
    run(&mut s, "construct.axis.two_points", json!({"a": {"point": [0, 0, 0]}, "b": {"point": [0, 0, 50]}, "name": "Spine"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "PartB", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let part_a = occ(&s, "PartA");
    let part_b = occ(&s, "PartB");

    // On its own component the axis resolves.
    let r_ok = run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": part_a, "axis": "Spine"}, "b": {"occurrence": part_b, "face": [5, 5, 10]}}),
    );
    assert_eq!(r_ok["conflicts"], json!([]));
    run(&mut s, "joint.delete", json!({"joint": r_ok["joint"].as_u64().unwrap()}));

    // On another component it is refused: `Spine` is PartA's axis, not a name that falls through
    // to whatever else the model has.
    let e = s
        .execute(
            "joint.create",
            &json!({"type": "revolute", "a": {"occurrence": part_b, "axis": "Spine"}, "b": {"occurrence": part_a, "face": [5, 5, 10]}}),
        )
        .unwrap_err();
    assert!(e.to_string().contains("Spine"), "{e}");
}

/// A bare `XY` (or `X`, `Y`, `Z`) side means that side's own component's datum: re-resolved at
/// every solve, so the joint follows the component it sits on, not the root. [PIN]
#[test]
fn bare_shorthand_means_the_sides_own_component() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Anchor"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let anchor = occ(&s, "Anchor");
    let part = occ(&s, "Part");
    // The anchor stands away from the root: a bare `XY` side of its joint must mean the anchor's
    // own plane there, not the root's at the world origin. The design's first part is grounded,
    // so the repositioning is a deliberate `move_grounded` move, as Align's is.
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [100, 0, 0], "move_grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": anchor, "plane": "XY"}, "b": {"occurrence": part, "plane": "XY"}, "name": "J"}),
    );
    assert!(near(place(&s, "Part", Vec3::ZERO), Vec3::new(100.0, 0.0, 0.0)), "{:?}", place(&s, "Part", Vec3::ZERO));
    // Still the side's own datum after another move: the joint follows the component, not the root.
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [30, 20, 0], "move_grounded": true}));
    assert!(near(place(&s, "Part", Vec3::ZERO), Vec3::new(130.0, 20.0, 0.0)), "{:?}", place(&s, "Part", Vec3::ZERO));
}

/// The solver frame direction: a datum side maps with the inverse of its side occurrence's world
/// frame (`occurrence_world`; the datum itself resolves through `placement_frame`), so a side on
/// a repeated component reads the placement it names, not the component's first one. [PIN]
#[test]
fn datum_sides_resolve_in_the_side_component_frame() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Anchor"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let anchor = occ(&s, "Anchor");
    let part = occ(&s, "Part");
    // The anchor stands away from the root, and its component is placed a second time further
    // along: the side names the second placement's own XY.
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [100, 0, 0], "move_grounded": true}));
    let copy = run(&mut s, "occurrence.copy", json!({"occurrence": anchor, "translate": [100, 0, 0]}))["occurrence"].as_u64().unwrap();
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": copy, "plane": format!("o:{copy}/origin/XY")}, "b": {"occurrence": part, "plane": "XY"}, "name": "J"}),
    );
    // The second placement stands at (200, 0, 0): the side is its datum, not the first
    // placement's at (100, 0, 0) (what a component-frame mapping would give).
    assert!(near(place(&s, "Part", Vec3::ZERO), Vec3::new(200.0, 0.0, 0.0)), "{:?}", place(&s, "Part", Vec3::ZERO));
}

#[test]
fn joint_edit_repicked_datum_side_matches_create() {
    // The joint.edit half of the review thread on re-picked datum sides: an edit that re-picks
    // a side onto a datum reads that side exactly as joint.create reads it - the datum's own
    // origin, no pick synthesized from where the other side happens to sit - so an edit onto a
    // datum and a create with the same two sides land in the same pose.
    let mut t = Session::default();
    run(&mut t, "component.create", json!({"name": "Anchor"}));
    run(&mut t, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut t, "component.create", json!({"name": "Part", "parent": 0}));
    run(&mut t, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let t_anchor = occ(&t, "Anchor");
    let t_part = occ(&t, "Part");
    run(&mut t, "occurrence.move", json!({"occurrence": t_anchor, "translate": [100, 0, 0], "move_grounded": true}));
    // The joint as it was: the anchor's X datum (an axis datum, through the axis form) and an
    // explicit point on the part.
    run(
        &mut t,
        "joint.create",
        json!({"type": "rigid", "a": {"axis": format!("o:{t_anchor}/origin/X")}, "b": {"occurrence": t_part, "point": [0, 0, 5], "z": [0, 0, 1]}, "name": "J1"}),
    );
    // Re-pick side b onto the part's own XY (a datum, no pick point): it reads the datum's own
    // origin, the way create reads it - nothing in the stored side depends on side a's pose.
    run(&mut t, "joint.edit", json!({"joint": "J1", "b": {"occurrence": t_part, "plane": "XY"}}));
    let edited = (place(&t, "Part", Vec3::ZERO), place(&t, "Part", Vec3::Z), place(&t, "Part", Vec3::X));
    // The same sides through joint.create: the pose agrees.
    run(&mut t, "joint.delete", json!({"joint": "J1"}));
    run(
        &mut t,
        "joint.create",
        json!({"type": "rigid", "a": {"axis": format!("o:{t_anchor}/origin/X")}, "b": {"occurrence": t_part, "plane": "XY"}, "name": "J"}),
    );
    let created = (place(&t, "Part", Vec3::ZERO), place(&t, "Part", Vec3::Z), place(&t, "Part", Vec3::X));
    assert!(near(edited.0, created.0) && near(edited.1, created.1) && near(edited.2, created.2), "edit {edited:?} != create {created:?}");
}

/// A datum joint added at a chain's depth leaves the chain on its own edges - including the
/// datum-wave seed order: with a datum snap among the joints, every fixed occurrence but the
/// root is walked to exhaustion before the root's joints.
#[test]
fn datum_joints_keep_the_chain_fifo_tiebreak() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Anchor"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Link", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Tail", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let anchor = occ(&s, "Anchor");
    let link = occ(&s, "Link");
    let tail = occ(&s, "Tail");
    // The grounded anchor stands away from the root; the chain Link -> Tail hangs off it.
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [100, 0, 0], "move_grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": anchor, "plane": "XY"}, "b": {"occurrence": link, "plane": "XY"}, "name": "J1"}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": link, "plane": "XY"}, "b": {"occurrence": tail, "plane": "XY"}, "name": "J2"}),
    );
    assert!(near(place(&s, "Link", Vec3::ZERO), Vec3::new(100.0, 0.0, 0.0)), "{:?}", place(&s, "Link", Vec3::ZERO));
    assert!(near(place(&s, "Tail", Vec3::ZERO), Vec3::new(100.0, 0.0, 0.0)), "{:?}", place(&s, "Tail", Vec3::ZERO));
    // A datum joint tying the chain's last part to the root competes with J2 for Tail: the chain
    // keeps its edges, and the datum joint is the leftover that closes the loop - reported as a
    // conflict, since the root's origin is not where the chain holds Tail.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": tail, "plane": "XY"}, "b": {"occurrence": 0, "point": [0, 0, 0]}, "name": "J3"}),
    );
    assert!(near(place(&s, "Link", Vec3::ZERO), Vec3::new(100.0, 0.0, 0.0)), "{:?}", place(&s, "Link", Vec3::ZERO));
    assert!(near(place(&s, "Tail", Vec3::ZERO), Vec3::new(100.0, 0.0, 0.0)), "{:?}", place(&s, "Tail", Vec3::ZERO));
    let conflicts: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(conflicts, ["J3"], "{r}");
}

/// Without a datum joint, the spanning tree keeps its pre-change order: where a root joint and a
/// grounded occurrence's joint both reach a part at the same depth, the root's places it (the
/// seeds were the root first) and the grounded joint is the conflict. Datum joints reorder the
/// seeds; plain point snaps must not.
#[test]
fn root_versus_grounded_joint_keeps_the_pre_change_tree() {
    let mut s = two_cubes();
    let origin = |occurrence: Value, p: [i64; 3]| json!({"occurrence": occurrence, "point": p, "z": [0, 0, 1], "x": [1, 0, 0]});
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "flip": true, "name": "RootPin", "a": origin(json!(0), [30, 0, 0]), "b": origin("B".into(), [0, 0, 0])}),
    );
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "flip": true, "name": "GroundPin", "a": origin("A".into(), [60, 0, 0]), "b": origin("B".into(), [0, 0, 0])}),
    );
    // RootPin is B's parent, as before: B sits at (30, 0, 0), and the grounded joint conflicts.
    assert!(near(place(&s, "B", Vec3::new(5.0, 5.0, 5.0)), Vec3::new(35.0, 5.0, 5.0)), "{:?}", place(&s, "B", Vec3::new(5.0, 5.0, 5.0)));
    let conflicts: Vec<&str> = r["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(conflicts, ["GroundPin"], "{r}");
}

/// A feature named like a datum key stays reachable: a side name resolves the feature first, the
/// datum key second (the resolver order every consumer shares).
#[test]
fn feature_named_like_a_datum_key_stays_reachable() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Anchor"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let anchor = occ(&s, "Anchor");
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [100, 0, 0], "move_grounded": true}));
    // A root construction plane named exactly like the anchor's datum key.
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 15, "name": format!("o:{anchor}/origin/XY")}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let part = occ(&s, "Part");
    run(
        &mut s,
        "joint.create",
        json!({"type": "planar", "a": {"plane": format!("o:{anchor}/origin/XY")}, "b": {"occurrence": part, "face": [2.5, 2.5, 0]}, "name": "J"}),
    );
    // The feature wins: the part's face sits on the construction plane at z=15, not on the
    // anchor's own XY (which the key would otherwise name) at z=0.
    assert!(near(place(&s, "Part", Vec3::new(2.5, 2.5, 0.0)), Vec3::new(2.5, 2.5, 15.0)), "{:?}", place(&s, "Part", Vec3::new(2.5, 2.5, 0.0)));
}

/// Two placements of one component, each jointed to the root through the SAME external datum
/// key: a plane or axis side's frame is mapped through its own occurrence (`in_occurrence`), so
/// the resolve cache must not share one component-keyed entry between them - the second
/// placement would be placed with the first's frame.
#[test]
fn datum_sides_stay_distinct_across_repeated_components() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Anchor"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let anchor = occ(&s, "Anchor");
    let part = occ(&s, "Part");
    run(&mut s, "occurrence.move", json!({"occurrence": anchor, "translate": [100, 0, 0], "move_grounded": true}));
    let part2 = run(&mut s, "occurrence.copy", json!({"occurrence": part, "translate": [0, 50, 0]}))["occurrence"].as_u64().unwrap();
    let key = format!("o:{anchor}/origin/XY");
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": part, "plane": key}, "b": {"occurrence": 0, "plane": "XY"}, "name": "J1"}),
    );
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": part2, "plane": key}, "b": {"occurrence": 0, "plane": "XY"}, "name": "J2"}),
    );
    let origin_of = |o: u64| solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, o), Vec3::ZERO);
    let p1 = origin_of(part);
    let p2 = origin_of(part2);
    // Each side reads the shared anchor datum through its own occurrence: the part (created at
    // the root) lands at (-100, 0, 0); its copy (created 50 along y) keeps that offset and lands
    // at (-100, 50, 0). A component-keyed cache entry would place the copy at (-100, 0, 0) too.
    assert!(near(p1, Vec3::new(-100.0, 0.0, 0.0)), "{p1:?}");
    assert!(near(p2, Vec3::new(-100.0, 50.0, 0.0)), "{p2:?}");
}

/// A joint wholly inside a copied tree whose side sits on the copy's own nested placement's
/// datum: the cloned joint names the CLONED placements (the datum path is rebuilt through the
/// copy), and a joint whose datum names a placement outside the copied tree is skipped with a
/// notice, like any reference across the boundary.
#[test]
fn cloned_joints_remap_datum_paths_into_the_copy() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Assembly", "parent": 0}));
    run(&mut s, "component.create", json!({"name": "Inner", "parent": "Assembly"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    run(&mut s, "component.create", json!({"name": "Mate", "parent": "Assembly"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "operation": "new"}));
    let asm = occ(&s, "Assembly");
    let inner = occ(&s, "Inner");
    let mate = occ(&s, "Mate");
    let base = occ(&s, "Base");
    // Wholly inside the tree, with a nested datum path into the copy.
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": inner, "plane": format!("o:{asm}/{inner}/origin/XY")}, "b": {"occurrence": mate, "point": [0, 0, 5], "z": [0, 0, 1]}, "name": "Jin"}),
    );
    // Datum naming Base, outside the copied tree: the copy refuses this joint.
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": inner, "plane": format!("o:{base}/origin/XY")}, "b": {"occurrence": mate, "point": [0, 0, 5], "z": [0, 0, 1]}, "name": "Jout"}),
    );
    let r = run(&mut s, "component.paste_new", json!({"component": "Assembly", "translate": [200, 0, 0]}));
    let new_asm = r["occurrence"].as_u64().unwrap();
    let comp_of = |id: u64| s.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.component);
    let mut occs = vec![new_asm];
    let mut comps = vec![comp_of(new_asm).unwrap()];
    loop {
        let mut grew = false;
        for o in &s.doc.occurrences {
            if comps.contains(&o.parent) && !occs.contains(&o.id) {
                occs.push(o.id);
                comps.push(o.component);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let cloned: Vec<_> = s.doc.assembly.joints.iter().filter(|j| occs.contains(&j.a.occurrence)).collect();
    assert_eq!(cloned.len(), 1, "the boundary joint is skipped, the inner one is copied: {}", r["notices"]);
    let j = cloned[0];
    let new_inner = j.a.occurrence;
    match &j.a.snap {
        solidcraft_doc::joints::Snap::Plane { plane, .. } => match plane.as_ref() {
            solidcraft_doc::PlaneRef::Datum { datum } => {
                assert_eq!(datum.path, vec![new_asm, new_inner], "the datum path names the copy's own nested placement");
            }
            other => panic!("expected a datum plane side, got {other:?}"),
        },
        other => panic!("expected a plane side, got {other:?}"),
    }
    assert!(r["notices"].as_array().is_some_and(|n| n.iter().any(|n| n.as_str().unwrap_or_default().contains("Jout"))), "{:?}", r["notices"]);
}

/// Driving a joint turns the side that is not held by a grounded part: a leaf on a grounded base
/// (#16139), the whole chain of parts rigidly joined to it, and nothing when both sides are
/// grounded, which the reply says precisely.
#[test]
fn driving_a_joint_moves_the_ungrounded_side() {
    let mut s = Session::default();
    for (name, corner) in [("Base", [0.0, 0.0, 0.0]), ("Mid", [10.0, 0.0, 0.0]), ("Tip", [20.0, 0.0, 0.0]), ("Post", [40.0, 0.0, 0.0])] {
        part(&mut s, name, corner);
    }
    for name in ["Base", "Post"] {
        let g = occ(&s, name);
        run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    }
    // A leaf: Mid turns on the base about +Z through (10, 1, 1); Tip is welded to Mid.
    pin(&mut s, "revolute", "Hinge", "Base", "Mid", Vec3::new(10.0, 1.0, 1.0), Vec3::Z);
    pin(&mut s, "rigid", "Weld", "Mid", "Tip", Vec3::new(20.0, 1.0, 1.0), Vec3::Z);
    let (mid, tip) = (Vec3::new(11.0, 1.0, 1.0), Vec3::new(21.0, 1.0, 1.0));
    let (mid0, tip0, base0) = (place(&s, "Mid", mid), place(&s, "Tip", tip), place(&s, "Base", Vec3::new(1.0, 1.0, 1.0)));
    let r = run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90, "quiet": true}));
    assert_eq!(r["moved"], true, "{r}");
    assert!(r["reason"].is_null(), "{r}");
    // Mid swings about the hinge axis; its chain (Tip) goes with it as one rigid piece.
    let (mid1, tip1) = (place(&s, "Mid", mid), place(&s, "Tip", tip));
    assert!(!near(mid1, mid0) && !near(tip1, tip0), "the leaf and its welded part move: {mid1:?} {tip1:?}");
    assert!((mid1.dist(Vec3::new(10.0, 1.0, 1.0)) - mid0.dist(Vec3::new(10.0, 1.0, 1.0))).abs() < 1e-6);
    assert!((mid1.dist(tip1) - mid0.dist(tip0)).abs() < 1e-6, "Tip keeps its place against Mid");
    assert!(near(place(&s, "Base", Vec3::new(1.0, 1.0, 1.0)), base0));
    // A joint between two grounded parts: nothing to move, and the reply names both.
    pin(&mut s, "revolute", "Stuck", "Base", "Post", Vec3::new(40.0, 1.0, 1.0), Vec3::Z);
    let r = run(&mut s, "joint.drive", json!({"joint": "Stuck", "value": 30, "quiet": true}));
    assert_eq!(r["moved"], false, "{r}");
    assert_eq!(r["reason"], "both sides are held by grounded parts: Base:1, Post:1", "{r}");
}

/// A rigid joint whose origin face is gone (its sketch circle replaced) still holds its part at
/// the pose it had, the error stays on the rigid joint, and a revolute from that part to a leaf
/// turns only the leaf: the part is not cut from the grounded base (#16139).
#[test]
fn a_rigid_joint_with_a_lost_origin_still_holds_its_part() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [0, 0, 0]}));
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Pin"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PinSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "name": "Fix", "flip": true, "angle": 30, "offset": 2, "a": {"occurrence": "Base", "face": [20, 20, 10]}, "b": {"occurrence": "Pin", "circle": [105, 0, 20]}}),
    );
    // Its flip, angle and offset have placed the part; those are its saved pose.
    // The part's whole transform, every entry: three points could miss a turn about their line.
    let pose = |s: &Session| solidcraft_doc::joints::occurrence_world(&s.doc, occ(s, "Pin"));
    let same = |a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]| a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() < 1e-6);
    let at = Vec3::new(100.0, 0.0, 0.0);
    let saved = pose(&s);
    part(&mut s, "Leaf", [300.0, 0.0, 0.0]);
    // The face the rigid joint is made from goes: its name no longer resolves.
    run(&mut s, "sketch.edit", json!({"sketch": "PinSketch"}));
    run(&mut s, "sketch.delete", json!({"entities": ["c1"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5, "id": "c2"}));
    run(&mut s, "sketch.finish", json!({}));
    let fix = run(&mut s, "joint.inspect", json!({"joint": "Fix"}))["joints"][0].clone();
    assert!(!fix["errors"].as_array().is_none_or(Vec::is_empty), "the error stays on the rigid joint: {fix}");
    let leaf_at = Vec3::new(301.0, 1.0, 1.0);
    let (pin0, leaf0) = (place(&s, "Pin", at), place(&s, "Leaf", leaf_at));
    // The lost origin does not apply the joint's flip, angle or offset a second time.
    assert!(same(&pose(&s), &saved), "exact saved pose: {:?} {saved:?}", pose(&s));
    pin(&mut s, "revolute", "Turn", "Pin", "Leaf", pin0 + Vec3::new(0.0, 0.0, 30.0), Vec3::Y);
    // The part keeps its relative pose to the base, and the revolute turns only the leaf.
    let r = run(&mut s, "joint.drive", json!({"joint": "Turn", "value": 40, "quiet": true}));
    assert_eq!(r["moved"], true, "{r}");
    assert!(r["reason"].is_null(), "{r}");
    assert!(same(&pose(&s), &saved), "the held part stays: {:?} {saved:?}", pose(&s));
    assert!(!near(place(&s, "Leaf", leaf_at), leaf0), "the leaf turns");
    let fix = run(&mut s, "joint.inspect", json!({"joint": "Fix"}))["joints"][0].clone();
    assert!(!fix["errors"].as_array().is_none_or(Vec::is_empty), "{fix}");
    assert!(same(&pose(&s), &saved), "and after the drive: {:?}", pose(&s));
}

/// A drive moves the parts it carries by their placement alone: the faces the joint's origins
/// were picked on are not searched again, however many times the joints are solved (#17074).
#[test]
fn driving_a_slider_does_not_resolve_its_face_origins_again() {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let a = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    let probe = Vec3::new(25.0, 5.0, 5.0);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5}));
    let (resolved, before) = (s.model.state().origin_resolutions(), place(&s, "B", probe));
    assert!(resolved > 0, "the first solve found the origins");
    for value in [6.0, 7.0, 8.0] {
        let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": value}));
        assert_eq!(r["moved"], true, "{r}");
    }
    assert_eq!(s.model.state().origin_resolutions(), resolved, "origins resolved again by a drive that only moved a part");
    assert!(near(place(&s, "B", probe), before + Vec3::new(3.0, 0.0, 0.0)), "B follows the slider: {:?}", place(&s, "B", probe));
}

/// Names of the joints a `joint.list` or `joint.inspect` reply carries.
fn listed_names(r: &Value) -> Vec<String> {
    r["joints"].as_array().into_iter().flatten().filter_map(|j| j["name"].as_str().map(str::to_string)).collect()
}

/// An assembly whose sub-assembly `Kit` is held together by rigid joints, as an import holds its
/// placements, beside a hinge, a rigid joint between top-level parts and a suppressed slider:
/// `joint.list`'s filters keep only what they are asked for and count what they leave out, and its
/// summary counts the rigid joints per component.
#[test]
fn joint_list_filters_and_summarises_rigid_groups() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Base"}));
    for name in ["Arm", "Kit", "Slide"] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
    }
    for name in ["P1", "P2"] {
        run(&mut s, "component.create", json!({"name": name, "parent": "Kit", "activate": false, "grounded": false}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "joint.as_built", json!({"type": "revolute", "a": "Base", "b": "Arm", "name": "Hinge"}));
    run(&mut s, "joint.rigid_group", json!({"occurrences": ["Kit", "P1", "P2"]}));
    run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "Base", "b": "Kit", "name": "Bolt"}));
    run(&mut s, "joint.as_built", json!({"type": "slider", "a": "Base", "b": "Slide", "name": "Rail"}));
    run(&mut s, "joint.edit", json!({"joint": "Rail", "suppressed": true}));

    // No filter: every joint, and nothing about filters.
    let all = run(&mut s, "joint.list", json!({}));
    assert_eq!(all["joints"].as_array().map(Vec::len), Some(5), "{all}");
    assert!(all.get("omitted").is_none() && all.get("summary").is_none(), "{all}");

    let names = |s: &mut Session, p: Value| -> (Vec<String>, Value) {
        let r = run(s, "joint.list", p);
        (listed_names(&r), r["omitted"].clone())
    };
    let (kept, omitted) = names(&mut s, json!({"exclude_rigid": true}));
    assert_eq!(kept, ["Hinge", "Rail"]);
    assert_eq!(omitted, json!({"joints": 3, "by_type": {"rigid": 3}}));
    // Only the kit's own rigid joints go; the bolt between top-level parts stays.
    let (kept, omitted) = names(&mut s, json!({"exclude_rigid_groups": true}));
    assert_eq!(kept, ["Hinge", "Bolt", "Rail"]);
    assert_eq!(omitted, json!({"joints": 2, "by_type": {"rigid": 2}}));
    // Moving only: no rigid joint, and not the suppressed slider.
    let (kept, omitted) = names(&mut s, json!({"moving_only": true}));
    assert_eq!(kept, ["Hinge"]);
    assert_eq!(omitted, json!({"joints": 4, "by_type": {"rigid": 3, "slider": 1}}));
    let (kept, _) = names(&mut s, json!({"kinds": ["slider", "Rotate"]}));
    assert_eq!(kept, ["Hinge", "Rail"], "types as joint.create takes them");
    // An occurrence keeps the joints at it and inside it.
    let (kept, _) = names(&mut s, json!({"occurrences": ["Kit"]}));
    assert_eq!(kept.len(), 3, "the kit's two and the bolt: {kept:?}");
    assert!(kept.contains(&"Bolt".to_string()), "{kept:?}");
    let (kept, _) = names(&mut s, json!({"occurrences": ["Kit"], "exclude_rigid_groups": true}));
    assert_eq!(kept, ["Bolt"], "filters add up");
    let p1 = run(&mut s, "joint.list", json!({"occurrences": ["P1"]}));
    assert_eq!(p1["joints"].as_array().map(Vec::len), Some(1), "{p1}");
    let p1_occ = occ(&s, "P1");
    assert_eq!(p1["dof"], json!([{"occurrence": p1_occ, "dof": 0}]), "dof narrowed to the occurrence: {p1}");

    // dof_free_only lists the occurrences that can still move, and counts the others.
    let fixed = all["dof"].as_array().map(|d| d.iter().filter(|x| x["dof"] == 0).count()).unwrap_or(0);
    let free = run(&mut s, "joint.list", json!({"dof_free_only": true}));
    assert!(free["dof"].as_array().is_some_and(|d| !d.is_empty() && d.iter().all(|x| x["dof"].as_u64().is_some_and(|n| n > 0))), "{free}");
    assert_eq!(free["omitted"], json!({"dof": fixed}), "{free}");
    assert_eq!(free["joints"], all["joints"], "rows stay");
    let solved = run(&mut s, "joint.solve", json!({"dof_free_only": true}));
    assert_eq!(solved["dof"], free["dof"]);
    assert_eq!(solved["omitted"], json!({"dof": fixed}));

    let summary = run(&mut s, "joint.list", json!({"summary": true, "exclude_rigid": true}))["summary"].clone();
    let kit = s.doc.find_component("Kit").unwrap_or(0);
    assert_eq!(
        summary,
        json!({"joints": 5, "by_type": {"revolute": 1, "rigid": 3, "slider": 1}, "suppressed": 1,
               "rigid_groups": [{"component": kit, "name": "Kit", "joints": 2, "members": 3}]}),
        "the summary counts every joint, whatever the filters keep"
    );
    assert_eq!(s.contents_joints(kit).len(), 2, "the summary counts what component.unlock removes");

    // joint.inspect takes the same filters.
    let inspected = run(&mut s, "joint.inspect", json!({"exclude_rigid_groups": true}));
    assert_eq!(listed_names(&inspected), ["Hinge", "Bolt", "Rail"]);
    assert_eq!(inspected["omitted"], json!({"joints": 2, "by_type": {"rigid": 2}}));
    let one = run(&mut s, "joint.inspect", json!({"joint": "Bolt"}));
    assert!(one.get("omitted").is_none(), "{one}");

    for bad in [json!({"kinds": ["hinge"]}), json!({"kinds": "revolute"}), json!({"occurrences": ["Nowhere"]})] {
        assert!(s.execute("joint.list", &bad).is_err(), "{bad}");
    }

    // Past 20 groups the summary lists the largest and counts the rest.
    for i in 0..20 {
        let (outer, inner) = (format!("Wrap{i}"), format!("Part{i}"));
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": outer, "grounded": false}));
        run(&mut s, "component.create", json!({"name": inner, "parent": outer, "activate": false, "grounded": false}));
        run(&mut s, "joint.rigid_group", json!({"occurrences": [outer, inner]}));
    }
    let summary = run(&mut s, "joint.list", json!({"summary": true, "kinds": []}))["summary"].clone();
    assert_eq!(summary["rigid_groups"].as_array().map(Vec::len), Some(20), "{summary}");
    assert_eq!(summary["rigid_groups"][0]["name"], "Kit", "the largest first: {summary}");
    assert_eq!(summary["more_rigid_groups"], json!({"groups": 1, "joints": 1}), "{summary}");
}

/// Two cubes on a cylindrical joint.
fn sleeve() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        s.execute("component.activate", &json!({"component": "root"})).ok();
        s.execute("component.create", &json!({"name": name})).ok();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})).ok();
    }
    s.execute("joint.create", &json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Sleeve"})).ok();
    s
}

#[test]
fn joint_dynamics_clears_when_nothing_is_given() {
    let mut s = sleeve();
    assert!(s.execute("joint.dynamics", &json!({"joint": "Sleeve", "damping": 0.5})).is_ok());
    assert_eq!(s.doc.assembly.joints[0].dynamics[0].and_then(|d| d.damping), Some(0.5));
    assert!(s.execute("joint.dynamics", &json!({"joint": "Sleeve"})).is_ok());
    assert!(s.doc.assembly.joints[0].dynamics.is_empty());
}

#[test]
fn a_changed_joint_type_keeps_only_its_own_slots() {
    let mut s = sleeve();
    assert!(s.execute("joint.dynamics", &json!({"joint": "Sleeve", "index": 1, "damping": 0.5})).is_ok());
    assert_eq!(s.doc.assembly.joints[0].dynamics.len(), 2);
    assert!(s.execute("joint.edit", &json!({"joint": "Sleeve", "type": "revolute"})).is_ok());
    assert_eq!(s.doc.assembly.joints[0].dynamics.len(), 1);
}

#[test]
fn joint_dynamics_are_reported_by_the_joint_list() {
    let mut s = sleeve();
    assert!(s.execute("joint.dynamics", &json!({"joint": "Sleeve", "effort": 3.0})).is_ok());
    let list = s.execute("joint.list", &json!({})).unwrap_or_default();
    assert_eq!(list["joints"][0]["values"][0]["dynamics"]["effort"], json!(3.0), "{list}");
}

#[test]
fn joint_dynamics_refuses_hostile_values() {
    let mut s = sleeve();
    for p in [
        json!({"joint": "Sleeve", "effort": 0.0}),
        json!({"joint": "Sleeve", "velocity": -1.0}),
        json!({"joint": "Sleeve", "damping": -1.0}),
        json!({"joint": "Sleeve", "friction": 1e300}),
        json!({"joint": "Sleeve", "index": 99, "damping": 1.0}),
        json!({"joint": "Sleeve", "index": -1, "damping": 1.0}),
        json!({"joint": "Sleeve", "index": 0.5, "damping": 1.0}),
        json!({"joint": "Sleeve", "index": "0", "damping": 1.0}),
        json!({"joint": "Nobody", "damping": 1.0}),
    ] {
        assert!(s.execute("joint.dynamics", &p).is_err(), "{p}");
    }
    assert!(s.doc.assembly.joints[0].dynamics.iter().all(Option::is_none));
}

#[test]
fn capture_position_timeline_joint_snapshot_suppress_rollback_and_parameter_change() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "travel", "expression": "3 mm"}));
    let jid = run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}))["joint"]
        .as_u64()
        .unwrap();
    let b = Vec3::new(25.0, 5.0, 0.0);

    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 3}));
    let cap = run(&mut s, "component.capture_position", json!({}));
    assert_eq!(cap["joints"], json!(1), "{cap}");
    let cp_id = cap["feature"].as_u64().expect("CapturePosition feature id");

    // Driving away via joint.drive is preserved across unrelated refreshes, and revert_position returns to 3.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 9}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 19.0)));
    run(&mut s, "component.revert_position", json!({}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 13.0)));

    // Redefining CapturePosition with an expression on the joint updates on parameters.change.
    run(
        &mut s,
        "timeline.redefine",
        json!({
            "feature": cp_id,
            "command": "component.capture_position",
            "params": {"joints": [{"joint": jid, "values": [3.0], "exprs": ["travel"]}]}
        }),
    );
    run(&mut s, "parameters.change", json!({"name": "travel", "expression": "7 mm"}));
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![7.0]));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 17.0)));

    // Suppress restores pre-capture baseline (0.0 -> (5.0, 5.0, 10.0)), unsuppress restores 7.0,
    // rollback restores 0.0, roll_to restores 7.0, and delete restores 0.0 and clears base_values.
    run(&mut s, "timeline.suppress", json!({"feature": cp_id, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].captured, None);
    assert_eq!(s.doc.assembly.joints[0].values, vec![0.0]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 10.0)));
    run(&mut s, "timeline.suppress", json!({"feature": cp_id, "suppressed": false}));
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![7.0]));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 17.0)));
    let cp_idx = s.doc.feature_index(cp_id).unwrap();
    run(&mut s, "timeline.rollback", json!({"position": cp_idx}));
    assert_eq!(s.doc.assembly.joints[0].captured, None);
    assert_eq!(s.doc.assembly.joints[0].values, vec![0.0]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 10.0)));
    run(&mut s, "timeline.roll_to", json!({}));
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![7.0]));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 17.0)));
    run(&mut s, "timeline.delete", json!({"feature": cp_id}));
    assert_eq!(s.doc.assembly.joints[0].captured, None);
    assert_eq!(s.doc.assembly.joints[0].values, vec![0.0]);
    assert_eq!(s.doc.assembly.joints[0].base_values, None);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 10.0)));
}

#[test]
fn capture_position_joint_baseline_multi_capture_reopen_and_uncaptured_drive() {
    // 0 -> drive(30 deg) -> cp1 -> drive(60 deg) -> cp2 -> suppress/rollback/delete + save/reopen + uncaptured drive(90 deg)
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "revolute", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Hinge"}));
    let b = Vec3::new(35.0, 5.0, 0.0);
    let p0 = place(&s, "B", b);

    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 30}));
    let p30 = place(&s, "B", b);
    let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.joints[0].base_values, Some(vec![0.0]));

    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 60}));
    let p60 = place(&s, "B", b);
    let cp2 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();

    // Suppress cp2 -> 30 deg; rollback before cp1 -> 0 deg; roll_to end -> 60 deg.
    run(&mut s, "timeline.suppress", json!({"feature": cp2, "suppressed": true}));
    assert!((s.doc.assembly.joints[0].values[0] - 30.0_f64.to_radians()).abs() < 1e-9);
    assert!(near(place(&s, "B", b), p30));
    run(&mut s, "timeline.suppress", json!({"feature": cp2, "suppressed": false}));
    assert!(near(place(&s, "B", b), p60));

    let idx1 = s.doc.feature_index(cp1).unwrap();
    let idx2 = s.doc.feature_index(cp2).unwrap();
    run(&mut s, "timeline.rollback", json!({"position": idx2}));
    assert!(near(place(&s, "B", b), p30));
    run(&mut s, "timeline.rollback", json!({"position": idx1}));
    assert!((s.doc.assembly.joints[0].values[0]).abs() < 1e-9);
    assert!(near(place(&s, "B", b), p0));
    run(&mut s, "timeline.roll_to", json!({}));
    assert!(near(place(&s, "B", b), p60));

    // Save / reopen round-trip followed immediately by suppress(cp2) -> 30 deg and suppress(cp1) -> 0 deg.
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    run(&mut reopened, "timeline.suppress", json!({"feature": cp2, "suppressed": true}));
    assert!(near(place(&reopened, "B", b), p30));
    run(&mut reopened, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    assert!((reopened.doc.assembly.joints[0].values[0]).abs() < 1e-9);
    assert!(near(place(&reopened, "B", b), p0));

    // Uncaptured joint.drive(90) after cp2 + unrelated command keeps 90, and revert_position returns to 60.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let p90 = place(&s, "B", b);
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [50, 50, 0]}));
    assert!((s.doc.assembly.joints[0].values[0] - 90.0_f64.to_radians()).abs() < 1e-9);
    assert!(near(place(&s, "B", b), p90));
    run(&mut s, "component.revert_position", json!({}));
    assert!((s.doc.assembly.joints[0].values[0] - 60.0_f64.to_radians()).abs() < 1e-9);
    assert!(near(place(&s, "B", b), p60));

    // Delete cp2 -> 30 deg; delete cp1 -> 0 deg and base_values cleared.
    run(&mut s, "timeline.delete", json!({"feature": cp2}));
    assert!(near(place(&s, "B", b), p30));
    assert!(s.doc.assembly.joints[0].base_values.is_some());
    run(&mut s, "timeline.delete", json!({"feature": cp1}));
    assert!(near(place(&s, "B", b), p0));
    assert_eq!(s.doc.assembly.joints[0].base_values, None);
    assert_eq!(s.doc.assembly.joints[0].captured, None);
}

#[test]
fn capture_position_joint_exprs_sync_redefine_revert_and_legacy_captured() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(&mut s, "parameters.add", json!({"name": "other_gap", "expression": "12 mm"}));
    run(&mut s, "parameters.add", json!({"name": "same_gap", "expression": "12 mm"}));
    let jid = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}),
    )["joint"]
        .as_u64()
        .unwrap();
    let b = Vec3::new(25.0, 5.0, 0.0);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));

    let cp_id = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.joints[0].base_values, Some(vec![5.0]));
    assert_eq!(s.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);

    // Redefine CapturePosition to a different expression ("other_gap" = 12 mm): j.values and j.exprs must sync!
    run(
        &mut s,
        "timeline.redefine",
        json!({
            "feature": cp_id,
            "command": "component.capture_position",
            "params": {"joints": [{"joint": jid, "values": [5.0], "exprs": ["other_gap"]}]}
        }),
    );
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![12.0]));
    assert_eq!(s.doc.assembly.joints[0].values, vec![12.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("other_gap"));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 22.0)));

    // Redefine to a different expression with the SAME numeric value ("same_gap" = 12 mm): j.exprs must sync!
    run(
        &mut s,
        "timeline.redefine",
        json!({
            "feature": cp_id,
            "command": "component.capture_position",
            "params": {"joints": [{"joint": jid, "values": [12.0], "exprs": ["same_gap"]}]}
        }),
    );
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("same_gap"));

    // Base expression ("gap") blocks delete and updates on rename!
    assert!(s.execute("parameters.delete", &json!({"name": "gap"})).is_err());
    run(&mut s, "parameters.rename", json!({"name": "gap", "new_name": "base_gap"}));
    assert_eq!(s.doc.assembly.joints[0].base_exprs, vec![Some("base_gap".to_string())]);

    // joint.drive clears j.exprs, and component.revert_position restores BOTH j.values and j.exprs ("same_gap").
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 20}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 30.0)));
    run(&mut s, "component.revert_position", json!({}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![12.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("same_gap"));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 22.0)));
    run(&mut s, "parameters.change", json!({"name": "same_gap", "expression": "14 mm"}));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 24.0)));

    // Redefine to a constant ([9.0], no exprs) clears j.exprs.values[0] while keeping base_exprs = ["base_gap"].
    run(
        &mut s,
        "timeline.redefine",
        json!({
            "feature": cp_id,
            "command": "component.capture_position",
            "params": {"joints": [{"joint": jid, "values": [9.0]}]}
        }),
    );
    assert_eq!(s.doc.assembly.joints[0].values, vec![9.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 19.0)));

    // Suppressing restores "base_gap" (5 mm -> 15.0); while suppressed or rolled back, timeline.edit eagerly
    // validates joint bounds/expressions; deleting clears base_values/base_exprs while keeping "base_gap".
    run(&mut s, "timeline.suppress", json!({"feature": cp_id, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("base_gap"));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));
    let doc_suppressed = s.doc.clone();
    assert!(s.execute("timeline.edit", &json!({"feature": cp_id, "set": {"joints": [{"joint": jid, "values": [1e12]}]}})).is_err());
    assert!(s.execute("timeline.edit", &json!({"feature": cp_id, "set": {"joints": [{"joint": jid, "values": [0.0], "exprs": ["   "]}]}})).is_err());
    assert!(
        s.execute("timeline.edit", &json!({"feature": cp_id, "set": {"joints": [{"joint": jid, "values": [0.0], "exprs": ["x".repeat(4097)]}]}}))
            .is_err()
    );
    assert_eq!(s.doc, doc_suppressed);
    run(&mut s, "timeline.delete", json!({"feature": cp_id}));
    assert_eq!(s.doc.assembly.joints[0].base_values, None);
    assert!(s.doc.assembly.joints[0].base_exprs.is_empty());
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("base_gap"));
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));

    // Legacy Joint::captured: Some([3.0]) without CapturePosition (and with an unrelated CapturePosition on another occurrence)
    // must keep j.captured intact.
    let mut legacy = two_cubes();
    run(&mut legacy, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}));
    run(&mut legacy, "joint.drive", json!({"joint": "Rail", "value": 3}));
    legacy.doc_mut().assembly.joints[0].captured = Some(vec![3.0]);
    legacy.pending_joint_bases.clear();
    let (_, extra_occ) = (
        run(&mut legacy, "component.create", json!({"name": "Extra", "grounded": false}))["component"].as_u64().unwrap(),
        legacy.doc.occurrences.last().unwrap().id,
    );
    run(&mut legacy, "component.activate", json!({"component": 0}));
    run(&mut legacy, "component.capture_position", json!({"moves": [{"occurrence": extra_occ, "translate": [2, 0, 0]}]}));
    assert_eq!(legacy.doc.assembly.joints[0].captured, Some(vec![3.0]));
    assert_eq!(legacy.doc.assembly.joints[0].base_values, None);
}

#[test]
fn capture_position_preserves_pre_drive_joint_exprs_across_suppress_rollback_and_delete() {
    // Test 1: Slider with "gap" (5 mm) -> joint.drive(15) -> capture_position -> suppress/rollback/delete restores "gap" and 5.0.
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}));
    let b = Vec3::new(25.0, 5.0, 0.0);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![15.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);

    let cp_id = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.joints[0].base_values, Some(vec![5.0]));
    assert_eq!(s.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 25.0)));

    run(&mut s, "timeline.suppress", json!({"feature": cp_id, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));

    run(&mut s, "timeline.suppress", json!({"feature": cp_id, "suppressed": false}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![15.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 25.0)));

    let cp_idx = s.doc.feature_index(cp_id).unwrap();
    run(&mut s, "timeline.rollback", json!({"position": cp_idx}));
    assert_eq!(s.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));

    run(&mut s, "timeline.roll_to", json!({}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![15.0]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 25.0)));

    run(&mut s, "timeline.delete", json!({"feature": cp_id}));
    assert_eq!(s.doc.assembly.joints[0].base_values, None);
    assert!(s.doc.assembly.joints[0].base_exprs.is_empty());
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));

    // Test 2: Same numeric value drive (gap = 5 mm -> joint.drive(5)) still records base_exprs and restores "gap".
    let mut s_same = two_cubes();
    run(&mut s_same, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(
        &mut s_same,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}),
    );
    run(&mut s_same, "joint.drive", json!({"joint": "Rail", "value": 5}));
    assert_eq!(s_same.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s_same.doc.assembly.joints[0].exprs.value(0), None);
    let cp_same = run(&mut s_same, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s_same.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);
    run(&mut s_same, "timeline.suppress", json!({"feature": cp_same, "suppressed": true}));
    assert_eq!(s_same.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);
    assert_eq!(s_same.doc.assembly.joints[0].exprs.value(0), Some("gap"));
    assert_eq!(s_same.doc.assembly.joints[0].values, vec![5.0]);

    // Test 3: Multi-DOF cylindrical single-index drive preserves both base expressions.
    let mut s_cyl = two_cubes();
    run(&mut s_cyl, "parameters.add", json!({"name": "ang0", "expression": "15 deg"}));
    run(&mut s_cyl, "parameters.add", json!({"name": "dist0", "expression": "5 mm"}));
    run(
        &mut s_cyl,
        "joint.create",
        json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["ang0", "dist0"], "name": "Cyl"}),
    );
    run(&mut s_cyl, "joint.drive", json!({"joint": "Cyl", "index": 0, "value": 45}));
    assert_eq!(s_cyl.doc.assembly.joints[0].exprs.value(0), None);
    assert_eq!(s_cyl.doc.assembly.joints[0].exprs.value(1), Some("dist0"));
    let cp_cyl = run(&mut s_cyl, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s_cyl.doc.assembly.joints[0].base_exprs, vec![Some("ang0".to_string()), Some("dist0".to_string())]);
    run(&mut s_cyl, "timeline.suppress", json!({"feature": cp_cyl, "suppressed": true}));
    assert_eq!(s_cyl.doc.assembly.joints[0].exprs.value(0), Some("ang0"));
    assert_eq!(s_cyl.doc.assembly.joints[0].exprs.value(1), Some("dist0"));
    assert!((s_cyl.doc.assembly.joints[0].values[0] - 15.0_f64.to_radians()).abs() < 1e-9);
    assert!((s_cyl.doc.assembly.joints[0].values[1] - 5.0).abs() < 1e-9);
}

#[test]
fn capture_position_timeline_edit_legacy_captured_syncs_joint_values() {
    let mut legacy = two_cubes();
    let jid =
        run(&mut legacy, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}))["joint"]
            .as_u64()
            .unwrap();
    run(&mut legacy, "joint.drive", json!({"joint": "Rail", "value": 3}));
    legacy.doc_mut().assembly.joints[0].captured = Some(vec![3.0]);
    legacy.pending_joint_bases.clear();

    run(&mut legacy, "component.create", json!({"name": "Extra", "grounded": false}));
    let extra_occ = legacy.doc.occurrences.last().unwrap().id;
    run(&mut legacy, "component.activate", json!({"component": 0}));
    let cp_id = run(&mut legacy, "component.capture_position", json!({"moves": [{"occurrence": extra_occ, "translate": [2, 0, 0]}]}))["feature"]
        .as_u64()
        .unwrap();

    // Uncaptured drive changes j.values away from legacy j.captured ([3.0]) to [10.0].
    run(&mut legacy, "joint.drive", json!({"joint": "Rail", "value": 10}));
    assert_eq!(legacy.doc.assembly.joints[0].values, vec![10.0]);
    assert_eq!(legacy.doc.assembly.joints[0].captured, Some(vec![3.0]));

    // timeline.edit adding the joint with cj.values == [3.0] must still sync j.values from [10.0] back to [3.0].
    run(&mut legacy, "timeline.edit", json!({"feature": cp_id, "set": {"joints": [{"joint": jid, "values": [3.0]}]}}));
    assert_eq!(legacy.doc.assembly.joints[0].values, vec![3.0]);
    assert_eq!(legacy.doc.assembly.joints[0].captured, Some(vec![3.0]));
    let b = Vec3::new(25.0, 5.0, 0.0);
    assert!(near(place(&legacy, "B", b), Vec3::new(5.0, 5.0, 13.0)));
}

#[test]
fn uncaptured_joint_moves_and_revert_do_not_corrupt_occurrence_base_transform() {
    let mut s = two_cubes();
    let b_occ = occ(&s, "B");
    run(&mut s, "parameters.add", json!({"name": "dx", "expression": "4 mm"}));
    run(&mut s, "occurrence.move", json!({"occurrence": b_occ, "translate": ["dx", 0, 0], "capture": false}));
    let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    let b_base_after_cp1 = s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform;
    assert!(b_base_after_cp1.is_some());

    // Creating a joint permanently aligns occurrences and adjusts base_transform accordingly.
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [29, 5, 0]}, "name": "Rail"}));
    let expected_base = s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform;
    assert!(expected_base.is_some());

    // Uncaptured joint.drive must NOT mutate B.base_transform.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, expected_base);

    // Uncaptured joint.reach must NOT mutate B.base_transform.
    let tip = place(&s, "B", Vec3::new(29.0, 5.0, 0.0));
    run(&mut s, "joint.reach", json!({"occurrence": "B", "at": [tip.x, tip.y, tip.z], "point": [tip.x, tip.y, tip.z + 5.0]}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, expected_base);

    // parameters.change must NOT shift B.base_transform via solve_joints.
    run(&mut s, "parameters.change", json!({"name": "dx", "expression": "7 mm"}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, expected_base);

    // Reverted occurrence.move(capture: true) must NOT leave B.base_transform shifted.
    let reverted = run(&mut s, "occurrence.move", json!({"occurrence": b_occ, "translate": [0, 20, 0], "capture": true}));
    assert_eq!(reverted["reverted"], true, "{reverted}");
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, expected_base);

    // Capturing joint pose in CP2, driving away, and calling component.revert_position must keep expected_base.
    let cp2 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 30}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, expected_base);
    run(&mut s, "component.revert_position", json!({}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, expected_base);

    // Suppressing/deleting CP1 and CP2 must restore without corrupted transforms.
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));
    run(&mut s, "timeline.delete", json!({"feature": cp2}));
    run(&mut s, "timeline.delete", json!({"feature": cp1}));
    assert_eq!(s.doc.occurrences.iter().find(|o| o.id == b_occ).unwrap().base_transform, None);
}

#[test]
fn parameters_commands_track_pending_joint_bases_before_capture() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);

    // Delete via parameters.delete or parameters.change(delete:) must refuse while referenced in pending_joint_bases.
    let err_del = s.execute("parameters.delete", &json!({"name": "gap"})).unwrap_err();
    assert!(err_del.to_string().contains("used by"), "{err_del}");
    let err_chg = s.execute("parameters.change", &json!({"delete": "gap"})).unwrap_err();
    assert!(err_chg.to_string().contains("used by"), "{err_chg}");

    // parameters.users reports the joint pending baseline.
    let users = run(&mut s, "parameters.users", json!({"name": "gap"}));
    assert!(users["users"].as_array().is_some_and(|a| a.iter().any(|u| u["type"] == "joint" && u["name"] == "Rail")), "{users}");

    // parameters.rename updates pending_joint_bases and reports references_updated.
    let ren = run(&mut s, "parameters.rename", json!({"name": "gap", "new_name": "gap2"}));
    assert_eq!(ren["references_updated"], 1, "{ren}");

    // parameters.change("gap3 = 8 mm") also updates pending_joint_bases.
    let chg_ren = run(&mut s, "parameters.change", json!({"name": "gap2", "expression": "gap3 = 8 mm"}));
    assert_eq!(chg_ren["renamed"]["references_updated"], 1, "{chg_ren}");

    let cp_id = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.joints[0].base_exprs, vec![Some("gap3".to_string())]);
    run(&mut s, "timeline.suppress", json!({"feature": cp_id, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap3"));
    assert_eq!(s.doc.assembly.joints[0].values, vec![8.0]);
}

#[test]
fn capture_position_after_cp1_same_value_drive_clears_expr_and_noop_motion_link() {
    let find_cp_joint_exprs = |s: &Session, cp_id: u64, jid: u64| -> Vec<Option<String>> {
        let f = s.doc.features.iter().find(|f| f.id == cp_id).unwrap();
        match &f.kind {
            solidcraft_doc::FeatureKind::CapturePosition { joints, .. } => {
                joints.iter().find(|cj| cj.joint == jid).map(|cj| cj.exprs.clone()).unwrap()
            }
            other => panic!("expected CapturePosition, got {other:?}"),
        }
    };

    // 1. Single-DOF same-value joint.drive after CP1 creates CP2, prevents resurrection, and restores on suppress.
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    let jid = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}),
    )["joint"]
        .as_u64()
        .unwrap();
    let _cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    let noop1 = run(&mut s, "component.capture_position", json!({}));
    assert_eq!(noop1["captured"], 0, "{noop1}");
    assert_eq!(noop1["joints"], 0, "{noop1}");
    assert!(noop1["feature"].is_null(), "{noop1}");

    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5}));
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);
    let cp2 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert!(find_cp_joint_exprs(&s, cp2, jid).is_empty());
    let noop2 = run(&mut s, "component.capture_position", json!({}));
    assert_eq!(noop2["captured"], 0, "{noop2}");
    assert_eq!(noop2["joints"], 0, "{noop2}");
    assert!(noop2["feature"].is_null(), "{noop2}");

    run(&mut s, "parameters.change", json!({"name": "gap", "expression": "20 mm"}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);

    run(&mut s, "timeline.suppress", json!({"feature": cp2, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![20.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));
    run(&mut s, "timeline.suppress", json!({"feature": cp2, "suppressed": false}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None);
    run(&mut s, "timeline.delete", json!({"feature": cp2}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![20.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));

    // 2. Multi-DOF cylindrical single-DOF same-value drive after CP1 clears only DOF 0 and preserves DOF 1.
    let mut s_cyl = two_cubes();
    run(&mut s_cyl, "parameters.add", json!({"name": "ang0", "expression": "0 deg"}));
    run(&mut s_cyl, "parameters.add", json!({"name": "dist0", "expression": "5 mm"}));
    let cyl_id = run(
        &mut s_cyl,
        "joint.create",
        json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["ang0", "dist0"], "name": "Cyl"}),
    )["joint"]
        .as_u64()
        .unwrap();
    let _cp1 = run(&mut s_cyl, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    run(&mut s_cyl, "joint.drive", json!({"joint": "Cyl", "index": 0, "value": 0}));
    let cp2 = run(&mut s_cyl, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(find_cp_joint_exprs(&s_cyl, cp2, cyl_id), vec![None, Some("dist0".to_string())]);
    run(&mut s_cyl, "parameters.change", json!({"name": "ang0", "expression": "45 deg"}));
    assert!(s_cyl.doc.assembly.joints[0].values[0].abs() < 1e-9);
    assert_eq!(s_cyl.doc.assembly.joints[0].exprs.value(0), None);
    run(&mut s_cyl, "parameters.change", json!({"name": "dist0", "expression": "12 mm"}));
    assert!((s_cyl.doc.assembly.joints[0].values[1] - 12.0).abs() < 1e-9);
    assert_eq!(s_cyl.doc.assembly.joints[0].exprs.value(1), Some("dist0"));

    // 3. MotionLink added BEFORE CP1: single-DOF and multi-DOF trailing DOF linked -> second capture_position is a no-op.
    let mut s_ml1 = two_cubes();
    run(&mut s_ml1, "component.activate", json!({"component": "root"}));
    run(&mut s_ml1, "component.create", json!({"name": "C", "grounded": false}));
    run(&mut s_ml1, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s_ml1, "parameters.add", json!({"name": "follower_gap", "expression": "0 mm"}));
    run(&mut s_ml1, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Driver"}));
    let fid1 = run(
        &mut s_ml1,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "C", "face": [45, 5, 0]}, "values": ["follower_gap"], "name": "Follower"}),
    )["joint"]
        .as_u64()
        .unwrap();
    run(&mut s_ml1, "joint.motion_link", json!({"a": "Driver", "b": "Follower", "ratio": 1}));
    let cp_ml1 = run(&mut s_ml1, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert!(find_cp_joint_exprs(&s_ml1, cp_ml1, fid1).is_empty());
    assert_eq!(s_ml1.doc.assembly.joints[1].exprs.value(0), None);
    assert_eq!(s_ml1.doc.assembly.joints[1].base_exprs, vec![Some("follower_gap".to_string())]);
    let noop_ml1 = run(&mut s_ml1, "component.capture_position", json!({}));
    assert!(noop_ml1["feature"].is_null(), "{noop_ml1}");

    let mut s_ml_cyl = two_cubes();
    run(&mut s_ml_cyl, "component.activate", json!({"component": "root"}));
    run(&mut s_ml_cyl, "component.create", json!({"name": "C", "grounded": false}));
    run(&mut s_ml_cyl, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s_ml_cyl, "parameters.add", json!({"name": "ang0", "expression": "10 deg"}));
    run(&mut s_ml_cyl, "parameters.add", json!({"name": "dist0", "expression": "0 mm"}));
    run(&mut s_ml_cyl, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Driver"}));
    let fid_cyl = run(
        &mut s_ml_cyl,
        "joint.create",
        json!({"type": "cylindrical", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "C", "face": [45, 5, 0]}, "values": ["ang0", "dist0"], "name": "Follower"}),
    )["joint"]
        .as_u64()
        .unwrap();
    run(&mut s_ml_cyl, "joint.motion_link", json!({"a": "Driver", "b": "Follower", "ib": 1, "ratio": 1}));
    let cp_ml_cyl = run(&mut s_ml_cyl, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(find_cp_joint_exprs(&s_ml_cyl, cp_ml_cyl, fid_cyl), vec![Some("ang0".to_string())]);
    let noop_ml_cyl = run(&mut s_ml_cyl, "component.capture_position", json!({}));
    assert!(noop_ml_cyl["feature"].is_null(), "{noop_ml_cyl}");

    // 4. MotionLink added AFTER CP1: un-expressed -> no-op; expressed -> CP2 clears once, third call is no-op.
    let mut s_post_unexpr = two_cubes();
    run(&mut s_post_unexpr, "component.activate", json!({"component": "root"}));
    run(&mut s_post_unexpr, "component.create", json!({"name": "C", "grounded": false}));
    run(&mut s_post_unexpr, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s_post_unexpr, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Driver"}));
    run(
        &mut s_post_unexpr,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "C", "face": [45, 5, 0]}, "name": "Follower"}),
    );
    let _cp1 = run(&mut s_post_unexpr, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    run(&mut s_post_unexpr, "joint.motion_link", json!({"a": "Driver", "b": "Follower", "ratio": 1}));
    let noop_post = run(&mut s_post_unexpr, "component.capture_position", json!({}));
    assert!(noop_post["feature"].is_null(), "{noop_post}");

    let mut s_post_expr = two_cubes();
    run(&mut s_post_expr, "component.activate", json!({"component": "root"}));
    run(&mut s_post_expr, "component.create", json!({"name": "C", "grounded": false}));
    run(&mut s_post_expr, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s_post_expr, "parameters.add", json!({"name": "follower_gap", "expression": "0 mm"}));
    run(&mut s_post_expr, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Driver"}));
    let fid_post = run(
        &mut s_post_expr,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "C", "face": [45, 5, 0]}, "values": ["follower_gap"], "name": "Follower"}),
    )["joint"]
        .as_u64()
        .unwrap();
    let _cp1 = run(&mut s_post_expr, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    run(&mut s_post_expr, "joint.motion_link", json!({"a": "Driver", "b": "Follower", "ratio": 1}));
    let cp2 = run(&mut s_post_expr, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert!(find_cp_joint_exprs(&s_post_expr, cp2, fid_post).is_empty());
    assert_eq!(s_post_expr.doc.assembly.joints[1].exprs.value(0), None);
    let noop_third = run(&mut s_post_expr, "component.capture_position", json!({}));
    assert!(noop_third["feature"].is_null(), "{noop_third}");
    run(&mut s_post_expr, "parameters.change", json!({"name": "follower_gap", "expression": "20 mm"}));
    assert_eq!(s_post_expr.doc.assembly.joints[1].exprs.value(0), None);
    assert!(s_post_expr.doc.assembly.joints[1].values[0].abs() < 1e-9);
}

#[test]
fn capture_position_legacy_captured_joint_same_value_drive_and_noop() {
    // Untouched legacy j.captured (both with and without j.exprs) must be a no-op on capture_position {}.
    for with_expr in [false, true] {
        let mut legacy = two_cubes();
        if with_expr {
            run(&mut legacy, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
        }
        let values = if with_expr { json!(["gap"]) } else { json!([5]) };
        run(
            &mut legacy,
            "joint.create",
            json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": values, "name": "Rail"}),
        );
        legacy.doc_mut().assembly.joints[0].captured = Some(vec![5.0]);
        let noop = run(&mut legacy, "component.capture_position", json!({}));
        assert_eq!(noop["captured"], 0, "{noop}");
        assert_eq!(noop["joints"], 0, "{noop}");
        assert!(noop["feature"].is_null(), "{noop}");
        assert!(!legacy.doc.has_capture_position_timeline());

        if with_expr {
            // After same-value joint.drive(5), capture_position {} creates CP1 preserving base_exprs and clearing exprs.
            run(&mut legacy, "joint.drive", json!({"joint": "Rail", "value": 5}));
            assert_eq!(legacy.doc.assembly.joints[0].exprs.value(0), None);
            let cp1 = run(&mut legacy, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
            assert_eq!(legacy.doc.assembly.joints[0].base_exprs, vec![Some("gap".to_string())]);
            let f = legacy.doc.features.iter().find(|f| f.id == cp1).unwrap();
            match &f.kind {
                solidcraft_doc::FeatureKind::CapturePosition { joints, .. } => {
                    assert!(joints[0].exprs.is_empty());
                }
                other => panic!("expected CapturePosition, got {other:?}"),
            }
            run(&mut legacy, "parameters.change", json!({"name": "gap", "expression": "20 mm"}));
            assert_eq!(legacy.doc.assembly.joints[0].values, vec![5.0]);
            assert_eq!(legacy.doc.assembly.joints[0].exprs.value(0), None);
            run(&mut legacy, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
            assert_eq!(legacy.doc.assembly.joints[0].values, vec![20.0]);
            assert_eq!(legacy.doc.assembly.joints[0].exprs.value(0), Some("gap"));
        }
    }
}

#[test]
fn capture_position_multi_cp_preserves_uncaptured_drive_and_restores_on_cp0_suppress() {
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s, "parameters.add", json!({"name": "dx", "expression": "5 mm"}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}));
    let c_occ = occ(&s, "C");
    let c_pt = Vec3::new(40.0, 0.0, 0.0);

    // CP0 captures Rail = 10.0.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    let cp0 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![10.0]));

    // CP1 moves C by dx while Rail remains 10.0.
    run(&mut s, "occurrence.move", json!({"occurrence": c_occ, "translate": ["dx", 0, 0], "capture": false}));
    let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert!(near(place(&s, "C", c_pt), Vec3::new(45.0, 0.0, 0.0)));

    // Uncaptured joint.drive(Rail, 25) stays preserved across CP1 parameter edits, suppress, rollback, and delete.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0]);
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![10.0]));

    run(&mut s, "parameters.change", json!({"name": "dx", "expression": "12 mm"}));
    assert!(near(place(&s, "C", c_pt), Vec3::new(52.0, 0.0, 0.0)));
    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0]);

    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    assert!(near(place(&s, "C", c_pt), c_pt));
    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0]);
    assert_eq!(s.doc.assembly.joints[0].captured, Some(vec![10.0]));

    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));
    assert!(near(place(&s, "C", c_pt), Vec3::new(52.0, 0.0, 0.0)));
    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0]);

    let cp1_idx = s.doc.feature_index(cp1).unwrap();
    run(&mut s, "timeline.rollback", json!({"position": cp1_idx}));
    assert!(near(place(&s, "C", c_pt), c_pt));
    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0]);

    run(&mut s, "timeline.roll_to", json!({}));
    run(&mut s, "timeline.delete", json!({"feature": cp1}));
    assert!(near(place(&s, "C", c_pt), c_pt));
    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0]);

    // component.revert_position {} restores Rail = 10.0.
    run(&mut s, "component.revert_position", json!({}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![10.0]);

    // Driving Rail = 25.0 and suppressing/deleting CP0 restores Rail = 0.0 and clears captured.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    run(&mut s, "timeline.suppress", json!({"feature": cp0, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![0.0]);
    assert_eq!(s.doc.assembly.joints[0].captured, None);

    run(&mut s, "timeline.suppress", json!({"feature": cp0, "suppressed": false}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![10.0]);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    run(&mut s, "timeline.delete", json!({"feature": cp0}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![0.0]);
    assert_eq!(s.doc.assembly.joints[0].captured, None);
}

#[test]
fn selection_delete_on_pending_joint_base_clears_parameter_users() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    assert!(!s.pending_joint_bases.is_empty());
    let users_before = run(&mut s, "parameters.users", json!({"name": "gap"}));
    assert!(!users_before["users"].as_array().unwrap().is_empty(), "{users_before}");

    run(&mut s, "selection.delete", json!({"joints": ["Rail"]}));
    assert!(s.pending_joint_bases.is_empty());
    let users_after = run(&mut s, "parameters.users", json!({"name": "gap"}));
    assert!(users_after["users"].as_array().unwrap().is_empty(), "{users_after}");
    run(&mut s, "parameters.delete", json!({"name": "gap"}));
}

#[test]
fn capture_position_joint_timeline_bugs_reopen_redefine_and_reverse_order() {
    // 1. Bug 1 & Bug 2 regression + Item 3d: parametric base_exprs & CapturedJoint::exprs across save/reopen + suppressed/rolled-back edit.
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "base_gap", "expression": "2 mm"}));
    run(&mut s, "parameters.add", json!({"name": "cap_gap", "expression": "15 mm"}));
    let jid = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["base_gap"], "name": "Rail"}),
    )["joint"]
        .as_u64()
        .unwrap();
    let b = Vec3::new(25.0, 5.0, 0.0);

    // Pre-capture feature in root reading B's top face at base_gap = 2 mm (z = 22).
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "construct.plane.offset", json!({"base": {"face": [5, 5, 22]}, "offset": 0, "name": "PreJointPlane"}));
    let plane_z = |s: &Session, name: &str| crate::view::construction_planes(s).into_iter().find(|(_, n, _)| n == name).unwrap().2.origin.z;
    assert!((plane_z(&s, "PreJointPlane") - 22.0).abs() < 1e-6);

    // Uncaptured joint.drive sets pending_joint_bases and triggers has_uncaptured_positions / uncaptured_joint_count.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    assert_eq!(s.uncaptured_joint_count(), 1);
    assert!(s.has_uncaptured_positions());

    // Pre-capture revert_position restores base_gap and clears pending_joint_bases.
    let rev = run(&mut s, "component.revert_position", json!({}));
    assert_eq!(rev["joints"], json!(1), "{rev}");
    assert_eq!(s.uncaptured_joint_count(), 0);
    assert!(!s.has_uncaptured_positions());
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 12.0)));

    // Drive again and capture, then redefine with cap_gap expr.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(s.uncaptured_joint_count(), 0);
    run(
        &mut s,
        "timeline.redefine",
        json!({
            "feature": cp1,
            "command": "component.capture_position",
            "params": {"joints": [{"joint": jid, "values": [15.0], "exprs": ["cap_gap"]}]}
        }),
    );

    // Bug 1 check: pre-capture PreJointPlane stays at 22.0 while CP1 active (B at z=25).
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 25.0)));
    assert!((plane_z(&s, "PreJointPlane") - 22.0).abs() < 1e-6, "PreJointPlane leaked to {}", plane_z(&s, "PreJointPlane"));

    // Bug 2 regression: suppressed and rolled-back timeline.edit rejects invalid joint exprs.
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    let snap = s.doc.clone();
    for bad_expr in ["missing_param", "1e12", "2 * ("] {
        assert!(
            s.execute("timeline.edit", &json!({"feature": cp1, "set": {"joints": [{"joint": jid, "values": [0.0], "exprs": [bad_expr]}]}})).is_err(),
            "{bad_expr}"
        );
    }
    assert_eq!(s.doc, snap);
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));

    let idx = s.doc.feature_index(cp1).unwrap();
    run(&mut s, "timeline.rollback", json!({"position": idx}));
    let snap2 = s.doc.clone();
    assert!(
        s.execute("timeline.edit", &json!({"feature": cp1, "set": {"joints": [{"joint": jid, "values": [0.0], "exprs": ["missing_param"]}]}}))
            .is_err()
    );
    assert_eq!(s.doc, snap2);

    // Item 3c: capturing a new CapturePosition while rolled back before CP1 + save/reopen while rolled back and while suppressed.
    let mut reopened_rb = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened_rb.doc.marker, Some(idx));
    assert_eq!(reopened_rb.doc.assembly.joints[0].values, vec![2.0]);
    assert_eq!(reopened_rb.doc.assembly.joints[0].exprs.value(0), Some("base_gap"));
    assert!(near(place(&reopened_rb, "B", b), Vec3::new(5.0, 5.0, 12.0)));
    run(&mut reopened_rb, "joint.drive", json!({"joint": "Rail", "value": 11}));
    let cp_rb = run(&mut reopened_rb, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert!(near(place(&reopened_rb, "B", b), Vec3::new(5.0, 5.0, 21.0)));
    run(&mut reopened_rb, "timeline.roll_to", json!({}));
    assert!(near(place(&reopened_rb, "B", b), Vec3::new(5.0, 5.0, 25.0)));
    run(&mut reopened_rb, "timeline.delete", json!({"feature": cp_rb}));

    run(&mut s, "timeline.roll_to", json!({}));
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    let mut reopened_sup = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(near(place(&reopened_sup, "B", b), Vec3::new(5.0, 5.0, 12.0)));
    assert_eq!(reopened_sup.doc.assembly.joints[0].exprs.value(0), Some("base_gap"));
    run(&mut reopened_sup, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));
    assert!(near(place(&reopened_sup, "B", b), Vec3::new(5.0, 5.0, 25.0)));
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));

    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    run(&mut reopened, "parameters.change", json!({"name": "cap_gap", "expression": "21 mm"}));
    assert!(near(place(&reopened, "B", b), Vec3::new(5.0, 5.0, 31.0)));
    run(&mut reopened, "parameters.rename", json!({"name": "cap_gap", "new_name": "stroke_gap"}));
    assert!(reopened.execute("parameters.delete", &json!({"name": "stroke_gap"})).is_err());
    run(&mut reopened, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    assert!(near(place(&reopened, "B", b), Vec3::new(5.0, 5.0, 12.0)));
    run(&mut reopened, "parameters.change", json!({"name": "base_gap", "expression": "6 mm"}));
    assert!(near(place(&reopened, "B", b), Vec3::new(5.0, 5.0, 16.0)));
    run(&mut reopened, "timeline.delete", json!({"feature": cp1}));
    assert_eq!(reopened.doc.assembly.joints[0].base_values, None);
    assert!(reopened.doc.assembly.joints[0].base_exprs.is_empty());

    // Item 4c: move -> align -> joint.drive -> capture -> undo -> redo with unjoined Free component.
    let mut s2 = two_cubes();
    run(&mut s2, "component.activate", json!({"component": "root"}));
    run(&mut s2, "component.create", json!({"name": "Free", "grounded": false}));
    run(&mut s2, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [50, 0, 0], "body_name": "FreeBox"}));
    run(&mut s2, "component.activate", json!({"component": "root"}));
    let free_occ = occ(&s2, "Free");
    run(&mut s2, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Rail"}));
    run(&mut s2, "occurrence.move", json!({"occurrence": free_occ, "translate": [10, 0, 0], "capture": false}));
    run(&mut s2, "occurrence.align", json!({"occurrence": free_occ, "from": [60, 0, 0], "to": [70, 0, 0], "capture": false}));
    run(&mut s2, "joint.drive", json!({"joint": "Rail", "value": 8}));
    let cap = run(&mut s2, "component.capture_position", json!({}));
    assert_eq!(cap["captured"], json!(1));
    assert_eq!(cap["joints"], json!(1));
    let cp_mix = cap["feature"].as_u64().unwrap();
    assert!(near(place(&s2, "B", b), Vec3::new(5.0, 5.0, 18.0)));
    assert!(near(place(&s2, "Free", Vec3::new(50.0, 0.0, 0.0)), Vec3::new(70.0, 0.0, 0.0)));

    // Item 2c: inspect input_keys on CapturePosition holding both moves and joints.
    let inspect = run(&mut s2, "timeline.inspect", json!({"feature": cp_mix}));
    assert!(!inspect["inputs"].as_array().unwrap().is_empty(), "{inspect}");

    run(&mut s2, "edit.undo", json!({}));
    assert!(s2.doc.feature(cp_mix).is_none());
    assert_eq!(s2.uncaptured_joint_count(), 1);
    assert!(near(place(&s2, "B", b), Vec3::new(5.0, 5.0, 18.0)));
    run(&mut s2, "edit.redo", json!({}));
    assert!(s2.doc.feature(cp_mix).is_some());
    assert_eq!(s2.uncaptured_joint_count(), 0);
    assert!(near(place(&s2, "B", b), Vec3::new(5.0, 5.0, 18.0)));
    assert!(near(place(&s2, "Free", Vec3::new(50.0, 0.0, 0.0)), Vec3::new(70.0, 0.0, 0.0)));
}

#[test]
fn capture_position_drive_and_reach_while_suppressed_or_rolled_back_revert_to_base_expr() {
    let mut s = two_cubes();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}));
    let b = Vec3::new(25.0, 5.0, 0.0);
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 15.0)));

    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
    let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert!(near(place(&s, "B", b), Vec3::new(5.0, 5.0, 25.0)));

    // 1. Suppress CP1 -> joint.drive while suppressed must record pending_joint_bases and revert_position must restore "gap".
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));

    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    assert_eq!(s.uncaptured_joint_count(), 1);
    assert!(s.has_uncaptured_positions());
    let rev = run(&mut s, "component.revert_position", json!({}));
    assert_eq!(rev["joints"], json!(1), "{rev}");
    assert_eq!(s.uncaptured_joint_count(), 0);
    assert!(!s.has_uncaptured_positions());
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));

    // Un-suppressing while a pending drive exists clears pending_joint_bases and restores CP1 cleanly.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 28}));
    assert_eq!(s.uncaptured_joint_count(), 1);
    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));
    assert_eq!(s.uncaptured_joint_count(), 0);
    assert_eq!(s.doc.assembly.joints[0].values, vec![15.0]);
    assert!(s.pending_joint_bases.is_empty());

    // 2. Roll back before CP1 -> joint.drive must record pending_joint_bases and revert_position must restore "gap".
    let idx = s.doc.feature_index(cp1).unwrap();
    run(&mut s, "timeline.rollback", json!({"position": idx}));
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));

    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 35}));
    assert_eq!(s.uncaptured_joint_count(), 1);
    assert!(s.has_uncaptured_positions());
    let rev_rb = run(&mut s, "component.revert_position", json!({}));
    assert_eq!(rev_rb["joints"], json!(1), "{rev_rb}");
    assert_eq!(s.uncaptured_joint_count(), 0);
    assert!(!s.has_uncaptured_positions());
    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0]);
    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"));

    // 3. joint.reach while CP1 is suppressed or rolled back (Cylindrical with expressed DOF 0 + free DOF 1)
    //    must record pending_joint_bases, report uncaptured_joint_count == 1, and revert_position restores base & expr.
    let mut s_reach = two_cubes();
    run(&mut s_reach, "parameters.add", json!({"name": "ang0", "expression": "15 deg"}));
    run(
        &mut s_reach,
        "joint.create",
        json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["ang0", 0], "name": "Cyl"}),
    );
    let b_reach = Vec3::new(25.0, 5.0, 0.0);
    let p0 = place(&s_reach, "B", b_reach);
    run(&mut s_reach, "joint.drive", json!({"joint": "Cyl", "values": [15, 12]}));
    let cp_reach = run(&mut s_reach, "component.capture_position", json!({}))["feature"].as_u64().unwrap();

    run(&mut s_reach, "timeline.suppress", json!({"feature": cp_reach, "suppressed": true}));
    assert!((s_reach.doc.assembly.joints[0].values[0] - 15.0_f64.to_radians()).abs() < 1e-6);
    assert!(s_reach.doc.assembly.joints[0].values[1].abs() < 1e-6);
    assert_eq!(s_reach.doc.assembly.joints[0].exprs.value(0), Some("ang0"));

    run(&mut s_reach, "joint.reach", json!({"occurrence": "B", "at": [p0.x, p0.y, p0.z], "point": [p0.x, p0.y, p0.z + 20.0]}));
    assert!((s_reach.doc.assembly.joints[0].values[1] - 20.0).abs() < 1e-6);
    assert_eq!(s_reach.uncaptured_joint_count(), 1);
    assert!(s_reach.has_uncaptured_positions());
    let rev_reach_sup = run(&mut s_reach, "component.revert_position", json!({}));
    assert_eq!(rev_reach_sup["joints"], json!(1), "{rev_reach_sup}");
    assert_eq!(s_reach.uncaptured_joint_count(), 0);
    assert!(s_reach.doc.assembly.joints[0].values[1].abs() < 1e-6);
    assert_eq!(s_reach.doc.assembly.joints[0].exprs.value(0), Some("ang0"));

    run(&mut s_reach, "timeline.suppress", json!({"feature": cp_reach, "suppressed": false}));
    let idx_reach = s_reach.doc.feature_index(cp_reach).unwrap();
    run(&mut s_reach, "timeline.rollback", json!({"position": idx_reach}));
    run(&mut s_reach, "joint.reach", json!({"occurrence": "B", "at": [p0.x, p0.y, p0.z], "point": [p0.x, p0.y, p0.z + 22.0]}));
    assert!((s_reach.doc.assembly.joints[0].values[1] - 22.0).abs() < 1e-6);
    assert_eq!(s_reach.uncaptured_joint_count(), 1);
    assert!(s_reach.has_uncaptured_positions());
    let rev_reach_rb = run(&mut s_reach, "component.revert_position", json!({}));
    assert_eq!(rev_reach_rb["joints"], json!(1), "{rev_reach_rb}");
    assert_eq!(s_reach.uncaptured_joint_count(), 0);
    assert!(s_reach.doc.assembly.joints[0].values[1].abs() < 1e-6);
    assert_eq!(s_reach.doc.assembly.joints[0].exprs.value(0), Some("ang0"));
}

#[test]
fn capture_position_legacy_captured_joint_reach_preserves_base_and_restores() {
    let mut legacy = two_cubes();
    run(&mut legacy, "parameters.add", json!({"name": "ang0", "expression": "15 deg"}));
    run(
        &mut legacy,
        "joint.create",
        json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["ang0", 0], "name": "Cyl"}),
    );
    let p0 = place(&legacy, "B", Vec3::new(25.0, 5.0, 0.0));
    let base_vals = legacy.doc.assembly.joints[0].values.clone();
    legacy.doc_mut().assembly.joints[0].captured = Some(base_vals.clone());
    assert_eq!(legacy.doc.assembly.joints[0].base_values, None);

    let noop = run(&mut legacy, "component.capture_position", json!({}));
    assert!(noop["feature"].is_null(), "{noop}");

    run(&mut legacy, "joint.reach", json!({"occurrence": "B", "at": [p0.x, p0.y, p0.z], "point": [p0.x, p0.y, p0.z + 20.0]}));
    assert!((legacy.doc.assembly.joints[0].values[1] - 20.0).abs() < 1e-6);
    assert_eq!(legacy.uncaptured_joint_count(), 1);
    assert!(legacy.has_uncaptured_positions());

    let cp1 = run(&mut legacy, "component.capture_position", json!({}))["feature"].as_u64().unwrap();
    assert_eq!(legacy.uncaptured_joint_count(), 0);
    assert!(!legacy.has_uncaptured_positions());
    assert_eq!(legacy.doc.assembly.joints[0].base_values.as_ref(), Some(&base_vals));
    assert_eq!(legacy.doc.assembly.joints[0].base_exprs, vec![Some("ang0".to_string())]);
    let captured_vals = legacy.doc.assembly.joints[0].captured.clone().unwrap();
    assert!((captured_vals[1] - 20.0).abs() < 1e-6);

    run(&mut legacy, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
    assert!((legacy.doc.assembly.joints[0].values[0] - base_vals[0]).abs() < 1e-6);
    assert!(legacy.doc.assembly.joints[0].values[1].abs() < 1e-6);
    assert_eq!(legacy.doc.assembly.joints[0].exprs.value(0), Some("ang0"));
    assert_eq!(legacy.doc.assembly.joints[0].exprs.value(1), None);

    run(&mut legacy, "timeline.suppress", json!({"feature": cp1, "suppressed": false}));
    assert!((legacy.doc.assembly.joints[0].values[1] - 20.0).abs() < 1e-6);

    let idx = legacy.doc.feature_index(cp1).unwrap();
    run(&mut legacy, "timeline.rollback", json!({"position": idx}));
    assert!((legacy.doc.assembly.joints[0].values[0] - base_vals[0]).abs() < 1e-6);
    assert!(legacy.doc.assembly.joints[0].values[1].abs() < 1e-6);
    assert_eq!(legacy.doc.assembly.joints[0].exprs.value(0), Some("ang0"));
    assert_eq!(legacy.doc.assembly.joints[0].exprs.value(1), None);

    run(&mut legacy, "timeline.roll_to", json!({}));
    assert!((legacy.doc.assembly.joints[0].values[1] - 20.0).abs() < 1e-6);
}

#[test]
fn capture_position_cleanup_clears_pending_joint_bases_on_delete_edit_redefine() {
    #[derive(Copy, Clone, Debug)]
    enum Mode {
        Suppress,
        Rollback,
    }
    #[derive(Copy, Clone, Debug)]
    enum Op {
        Delete,
        Edit,
        Redefine,
    }
    for mode in [Mode::Suppress, Mode::Rollback] {
        for op in [Op::Delete, Op::Edit, Op::Redefine] {
            let mut s = two_cubes();
            run(&mut s, "component.activate", json!({"component": "root"}));
            run(&mut s, "component.create", json!({"name": "C", "grounded": false}));
            run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0], "body_name": "CBox"}));
            run(&mut s, "component.activate", json!({"component": "root"}));
            let c_occ = occ(&s, "C");

            run(&mut s, "parameters.add", json!({"name": "gap", "expression": "5 mm"}));
            run(
                &mut s,
                "joint.create",
                json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap"], "name": "Rail"}),
            );
            run(&mut s, "occurrence.move", json!({"occurrence": c_occ, "translate": [10, 0, 0], "capture": false}));
            run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 15}));
            let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();

            match mode {
                Mode::Suppress => {
                    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
                }
                Mode::Rollback => {
                    let idx = s.doc.feature_index(cp1).unwrap();
                    run(&mut s, "timeline.rollback", json!({"position": idx}));
                }
            }
            assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?}/{op:?}");
            assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"), "{mode:?}/{op:?}");

            run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
            assert_eq!(s.doc.assembly.joints[0].values, vec![25.0], "{mode:?}/{op:?}");
            assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None, "{mode:?}/{op:?}");
            assert_eq!(s.uncaptured_joint_count(), 1, "{mode:?}/{op:?}");
            assert!(s.has_uncaptured_positions(), "{mode:?}/{op:?}");

            match op {
                Op::Delete => {
                    run(&mut s, "timeline.delete", json!({"feature": cp1}));
                }
                Op::Edit => {
                    run(&mut s, "timeline.edit", json!({"feature": cp1, "set": {"joints": []}}));
                }
                Op::Redefine => {
                    run(
                        &mut s,
                        "timeline.redefine",
                        json!({
                            "feature": cp1,
                            "command": "component.capture_position",
                            "params": {"moves": [{"occurrence": c_occ, "translate": [10, 0, 0]}], "joints": []}
                        }),
                    );
                }
            }

            assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?}/{op:?}");
            assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"), "{mode:?}/{op:?}");
            assert_eq!(s.doc.assembly.joints[0].base_values, None, "{mode:?}/{op:?}");
            assert_eq!(s.doc.assembly.joints[0].captured, None, "{mode:?}/{op:?}");
            assert_eq!(s.uncaptured_joint_count(), 0, "{mode:?}/{op:?}");
            assert!(!s.has_uncaptured_positions(), "{mode:?}/{op:?}");
            assert!(s.pending_joint_bases.is_empty(), "{mode:?}/{op:?}");

            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.doc.assembly.joints[0].values, vec![25.0], "{mode:?}/{op:?} undo");
            assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None, "{mode:?}/{op:?} undo");
            assert_eq!(s.uncaptured_joint_count(), 1, "{mode:?}/{op:?} undo");
            assert!(s.has_uncaptured_positions(), "{mode:?}/{op:?} undo");

            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?}/{op:?} redo");
            assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap"), "{mode:?}/{op:?} redo");
            assert_eq!(s.doc.assembly.joints[0].base_values, None, "{mode:?}/{op:?} redo");
            assert_eq!(s.doc.assembly.joints[0].captured, None, "{mode:?}/{op:?} redo");
            assert_eq!(s.uncaptured_joint_count(), 0, "{mode:?}/{op:?} redo");
            assert!(!s.has_uncaptured_positions(), "{mode:?}/{op:?} redo");
            assert!(s.pending_joint_bases.is_empty(), "{mode:?}/{op:?} redo");
        }
    }
}

#[test]
fn capture_position_inactive_edit_partial_joint_drop_preserves_kept_pending_base() {
    #[derive(Copy, Clone, Debug)]
    enum Mode {
        Suppress,
        Rollback,
    }
    for mode in [Mode::Suppress, Mode::Rollback] {
        let mut s = two_cubes();
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": "C", "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "parameters.add", json!({"name": "gap1", "expression": "5 mm"}));
        run(&mut s, "parameters.add", json!({"name": "gap2", "expression": "7 mm"}));
        let j1 = run(
            &mut s,
            "joint.create",
            json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap1"], "name": "Rail1"}),
        )["joint"]
            .as_u64()
            .unwrap();
        let j2 = run(
            &mut s,
            "joint.create",
            json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "C", "face": [45, 5, 0]}, "values": ["gap2"], "name": "Rail2"}),
        )["joint"]
            .as_u64()
            .unwrap();

        run(&mut s, "joint.drive", json!({"joint": "Rail1", "value": 15}));
        run(&mut s, "joint.drive", json!({"joint": "Rail2", "value": 17}));
        let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();

        match mode {
            Mode::Suppress => {
                run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
            }
            Mode::Rollback => {
                let idx = s.doc.feature_index(cp1).unwrap();
                run(&mut s, "timeline.rollback", json!({"position": idx}));
            }
        }
        assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?}");
        assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap1"), "{mode:?}");
        assert_eq!(s.doc.assembly.joints[1].values, vec![7.0], "{mode:?}");
        assert_eq!(s.doc.assembly.joints[1].exprs.value(0), Some("gap2"), "{mode:?}");

        run(&mut s, "joint.drive", json!({"joint": "Rail1", "value": 25}));
        run(&mut s, "joint.drive", json!({"joint": "Rail2", "value": 27}));
        assert_eq!(s.uncaptured_joint_count(), 2, "{mode:?}");
        assert!(s.has_uncaptured_positions(), "{mode:?}");

        run(&mut s, "timeline.edit", json!({"feature": cp1, "set": {"joints": [{"joint": j2, "values": [17.0]}]}}));
        assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?}");
        assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap1"), "{mode:?}");
        assert_eq!(s.doc.assembly.joints[0].base_values, None, "{mode:?}");
        assert_eq!(s.doc.assembly.joints[0].captured, None, "{mode:?}");
        assert!(!s.pending_joint_bases.contains_key(&j1), "{mode:?}");

        assert_eq!(s.doc.assembly.joints[1].values, vec![27.0], "{mode:?}");
        assert_eq!(s.doc.assembly.joints[1].exprs.value(0), None, "{mode:?}");
        assert_eq!(s.doc.assembly.joints[1].captured, None, "{mode:?}");
        assert!(s.pending_joint_bases.contains_key(&j2), "{mode:?}");
        assert_eq!(s.uncaptured_joint_count(), 1, "{mode:?}");
        assert!(s.has_uncaptured_positions(), "{mode:?}");

        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.assembly.joints[0].values, vec![25.0], "{mode:?} undo");
        assert_eq!(s.doc.assembly.joints[1].values, vec![27.0], "{mode:?} undo");
        assert_eq!(s.uncaptured_joint_count(), 2, "{mode:?} undo");
        assert!(s.has_uncaptured_positions(), "{mode:?} undo");

        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?} redo");
        assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap1"), "{mode:?} redo");
        assert_eq!(s.doc.assembly.joints[1].values, vec![27.0], "{mode:?} redo");
        assert_eq!(s.uncaptured_joint_count(), 1, "{mode:?} redo");
        assert!(s.has_uncaptured_positions(), "{mode:?} redo");

        let rev = run(&mut s, "component.revert_position", json!({}));
        assert_eq!(rev["joints"], json!(1), "{mode:?}: {rev}");
        assert_eq!(s.doc.assembly.joints[1].values, vec![7.0], "{mode:?}");
        assert_eq!(s.doc.assembly.joints[1].exprs.value(0), Some("gap2"), "{mode:?}");
        assert_eq!(s.uncaptured_joint_count(), 0, "{mode:?}");
        assert!(!s.has_uncaptured_positions(), "{mode:?}");
        assert!(s.pending_joint_bases.is_empty(), "{mode:?}");
    }
}

#[test]
fn capture_position_inactive_edit_moves_joints_and_add_joint_preserves_pending_bases() {
    #[derive(Copy, Clone, Debug)]
    enum Mode {
        Suppress,
        Rollback,
    }
    #[derive(Copy, Clone, Debug)]
    enum EditCase {
        MovesOnly,
        Joints,
        AddDrivenJoint,
    }
    for mode in [Mode::Suppress, Mode::Rollback] {
        for case in [EditCase::MovesOnly, EditCase::Joints, EditCase::AddDrivenJoint] {
            let mut s = two_cubes();
            run(&mut s, "component.activate", json!({"component": "root"}));
            run(&mut s, "component.create", json!({"name": "C", "grounded": false}));
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
            run(&mut s, "component.activate", json!({"component": "root"}));
            let c_occ = occ(&s, "C");
            run(&mut s, "parameters.add", json!({"name": "gap1", "expression": "5 mm"}));
            let j1 = run(
                &mut s,
                "joint.create",
                json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "values": ["gap1"], "name": "Rail1"}),
            )["joint"]
                .as_u64()
                .unwrap();

            run(&mut s, "occurrence.move", json!({"occurrence": c_occ, "translate": [10, 0, 0], "capture": false}));
            run(&mut s, "joint.drive", json!({"joint": "Rail1", "value": 15}));
            let cp1 = run(&mut s, "component.capture_position", json!({}))["feature"].as_u64().unwrap();

            let j2_opt = if matches!(case, EditCase::AddDrivenJoint) {
                run(&mut s, "component.create", json!({"name": "D", "grounded": false}));
                run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [70, 0, 0]}));
                run(&mut s, "component.activate", json!({"component": "root"}));
                run(&mut s, "parameters.add", json!({"name": "gap2", "expression": "7 mm"}));
                let j2 = run(
                    &mut s,
                    "joint.create",
                    json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "D", "face": [75, 5, 0]}, "values": ["gap2"], "name": "Rail2"}),
                )["joint"]
                    .as_u64()
                    .unwrap();
                Some(j2)
            } else {
                None
            };

            match mode {
                Mode::Suppress => {
                    run(&mut s, "timeline.suppress", json!({"feature": cp1, "suppressed": true}));
                }
                Mode::Rollback => {
                    let idx = s.doc.feature_index(cp1).unwrap();
                    run(&mut s, "timeline.rollback", json!({"position": idx}));
                }
            }

            match case {
                EditCase::MovesOnly => {
                    run(&mut s, "joint.drive", json!({"joint": "Rail1", "value": 25}));
                    run(&mut s, "timeline.edit", json!({"feature": cp1, "set": {"moves": [{"occurrence": c_occ, "translate": [20, 0, 0]}]}}));
                    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0], "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None, "{mode:?}/{case:?}");
                }
                EditCase::Joints => {
                    run(&mut s, "joint.drive", json!({"joint": "Rail1", "value": 25}));
                    run(&mut s, "timeline.edit", json!({"feature": cp1, "set": {"joints": [{"joint": j1, "values": [18.0]}]}}));
                    assert_eq!(s.doc.assembly.joints[0].values, vec![25.0], "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), None, "{mode:?}/{case:?}");
                }
                EditCase::AddDrivenJoint => {
                    let j2 = j2_opt.unwrap();
                    run(&mut s, "joint.drive", json!({"joint": "Rail2", "value": 27}));
                    run(
                        &mut s,
                        "timeline.edit",
                        json!({"feature": cp1, "set": {"joints": [{"joint": j1, "values": [15.0]}, {"joint": j2, "values": [17.0]}]}}),
                    );
                    assert_eq!(s.doc.assembly.joints[1].values, vec![27.0], "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[1].exprs.value(0), None, "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[1].base_values, Some(vec![7.0]), "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[1].base_exprs, vec![Some("gap2".to_string())], "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[1].captured, None, "{mode:?}/{case:?}");
                }
            }

            assert_eq!(s.uncaptured_joint_count(), 1, "{mode:?}/{case:?}");
            assert!(s.has_uncaptured_positions(), "{mode:?}/{case:?}");

            let rev = run(&mut s, "component.revert_position", json!({}));
            assert_eq!(rev["joints"], json!(1), "{mode:?}/{case:?}: {rev}");
            assert_eq!(s.uncaptured_joint_count(), 0, "{mode:?}/{case:?}");
            assert!(!s.has_uncaptured_positions(), "{mode:?}/{case:?}");
            assert!(s.pending_joint_bases.is_empty(), "{mode:?}/{case:?}");
            match case {
                EditCase::MovesOnly | EditCase::Joints => {
                    assert_eq!(s.doc.assembly.joints[0].values, vec![5.0], "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[0].exprs.value(0), Some("gap1"), "{mode:?}/{case:?}");
                }
                EditCase::AddDrivenJoint => {
                    assert_eq!(s.doc.assembly.joints[1].values, vec![7.0], "{mode:?}/{case:?}");
                    assert_eq!(s.doc.assembly.joints[1].exprs.value(0), Some("gap2"), "{mode:?}/{case:?}");
                }
            }
        }
    }
}

/// A value given as an expression has the bound a number has: past it, the design would save
/// and not open again.
#[test]
fn joint_values_given_as_expressions_are_bounded_like_numbers() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part"}));
    let (a, b) = (json!({"occurrence": 0, "point": [0, 0, 0]}), json!({"occurrence": "Part", "point": [0, 0, 0]}));
    for values in [json!([1e12]), json!(["1e12 mm"])] {
        let p = json!({"type": "slider", "a": a, "b": b, "values": values});
        assert!(s.execute("joint.create", &p).is_err(), "{p}");
    }
    let joint = run(&mut s, "joint.create", json!({"type": "slider", "a": a, "b": b, "values": ["5 mm"]}))["joint"].as_u64().unwrap();
    assert!(s.execute("joint.drive", &json!({"joint": joint, "value": "1e12 mm"})).is_err());
    assert!(s.execute("joint.edit", &json!({"joint": joint, "offset": "1e12 mm"})).is_err());
    solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
}
