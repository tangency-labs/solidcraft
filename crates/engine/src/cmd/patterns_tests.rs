use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// A 100 x 50 x 5 plate with a hole at the start of a straight path (10,10) → (90,10).
fn plate_with_hole() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 50, "height": 5, "corner": [0, 0, 0]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [90, 10]], "ids": ["l1"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 2, "id": "c1"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Path", "through_all": true, "operation": "cut", "name": "Hole1"}));
    s
}

const PLATE: f64 = 100.0 * 50.0 * 5.0;
const HOLE: f64 = PI * 4.0 * 5.0;

fn feature_pattern_plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":120,"width":120,"height":5,"corner":[0,0,0],"name":"Plate"}));
    run(&mut s, "solid.hole", json!({"position":[60,60,5],"diameter":2,"name":"Hole"}));
    s
}

#[test]
fn rectangular_hole_pattern_warns_for_copies_that_miss_and_clears_after_edit() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"corner":[0,0,0],"body_name":"Plate"}));
    run(&mut s, "solid.hole", json!({"position":[10,15,10],"diameter":5,"depth":10,"name":"Hole1"}));
    let before = volume(&mut s);
    let reply = run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features":["Hole1"],"dir1":[1,0,0],"count1":2,"spacing1":45,
            "dir2":[0,0,1],"count2":2,"spacing2":5,"name":"Pattern"}),
    );
    let id = reply["feature"].as_u64().unwrap();
    let check = |session: &Session| {
        let result = session.model.result(id).unwrap();
        assert!(result.error.is_none(), "{result:?}");
        assert_eq!(result.warning.as_deref(), Some("3 of 3 cut copies remove no material"));
    };
    check(&s);
    assert_eq!(reply["warning"], "3 of 3 cut copies remove no material");
    // The remaining copy lands in the existing through hole, so the plate stays intact.
    assert!(rel(volume(&mut s), before) < 1e-8);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&reopened);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Pattern","command":"solid.pattern.rectangular","params":{
            "features":["Hole1"],"dir1":[1,0,0],"count1":2,"spacing1":45,
            "dir2":[0,1,0],"count2":2,"spacing2":8}}),
    );
    assert_eq!(s.model.result(id).unwrap().warning.as_deref(), Some("2 of 3 cut copies remove no material"));
    assert!(rel(volume(&mut s), 12000.0 - 2.0 * PI * 2.5_f64.powi(2) * 10.0) < 1e-3);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Pattern","command":"solid.pattern.rectangular","params":{
            "features":["Hole1"],"dir1":[1,0,0],"count1":2,"spacing1":15,
            "dir2":[0,1,0],"count2":2,"spacing2":8}}),
    );
    assert!(s.model.result(id).unwrap().warning.is_none());
    assert!(rel(volume(&mut s), 12000.0 - 4.0 * PI * 2.5_f64.powi(2) * 10.0) < 1e-3);
}

#[test]
fn rectangular_cut_pattern_counts_only_target_contacts_and_keeps_landing_copies() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"corner":[0,0,0],"body_name":"Plate"}));
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"corner":[45,0,0],"operation":"new","body_name":"Neighbor"}));
    run(&mut s, "solid.cylinder", json!({"radius":2,"height":20,"base":[10,15,-5],"operation":"cut","targets":["Plate"],"name":"Cut"}));
    let reply = run(&mut s, "solid.pattern.rectangular", json!({"features":["Cut"],"dir1":[1,0,0],"count1":4,"spacing1":20}));
    assert_eq!(reply["warning"], "2 of 3 cut copies remove no material");
    let state = s.world_state();
    let plate = &state.body("Plate").unwrap().body;
    assert!(!plate.contains(solidcraft_geom::Vec3::new(30., 15., 5.)));
    assert!(state.body("Neighbor").unwrap().body.contains(solidcraft_geom::Vec3::new(50., 15., 5.)));
    assert!(rel(volume(&mut s), 24000.0 - 2.0 * PI * 4.0 * 10.0) < 1e-3);
}

#[test]
fn feature_rectangular_center_flip_and_oblique_direction_keep_source_and_recompute() {
    use solidcraft_geom::Vec3;
    for flipped in [false, true] {
        let mut s = feature_pattern_plate();
        let before = s.doc.to_json();
        run(
            &mut s,
            "solid.pattern.rectangular",
            json!({
                "features":["Hole"],"dir1":[1,0,0],"count1":4,"spacing1":15,
                "dir2":[0.6,0.8,0],"count2":2,"spacing2":20,
                "alignment":"center","alignment2":"center","reversed":flipped,"reversed2":flipped,"name":"Pattern"
            }),
        );
        let sign = if flipped { -1.0 } else { 1.0 };
        let st = s.world_state();
        let body = &st.bodies[0].body;
        // Analytic slots: the source, +1, -1, +2. The second direction is independent.
        for i in [0.0, 1.0, -1.0, 2.0] {
            for j in [0.0, 1.0] {
                let p = Vec3::new(60.0 + sign * (15.0 * i + 12.0 * j), 60.0 + sign * 16.0 * j, 2.5);
                assert!(!body.contains(p), "hole at {p:?}");
                assert!(body.contains(p + Vec3::new(0.0, 2.0, 0.0)));
            }
        }
        assert!(rel(volume(&mut s), 120.0 * 120.0 * 5.0 - 8.0 * PI * 5.0) < 1e-5);
        let saved = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), saved);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!(rel(volume(&mut reopened), volume(&mut s)) < 1e-8);
        // The copies replay the edited feature rather than freezing its original tool.
        run(&mut reopened, "timeline.redefine", json!({"feature":"Hole","command":"solid.hole","params":{"position":[60,60,5],"diameter":3}}));
        assert!(rel(volume(&mut reopened), 120.0 * 120.0 * 5.0 - 8.0 * PI * 2.25 * 5.0) < 1e-5);
    }
}

#[test]
fn feature_circular_center_flip_uses_partial_arc_endpoints_and_full_circle_spacing() {
    use solidcraft_geom::Vec3;
    for total in [90.0_f64, 360.0] {
        for flipped in [false, true] {
            let mut s = feature_pattern_plate();
            run(
                &mut s,
                "solid.pattern.circular",
                json!({
                    "features":["Hole"],"axis":{"origin":[40,60,0],"dir":[0,0,1]},
                    "count":6,"angle":total,"alignment":"center","reversed":flipped
                }),
            );
            let step = total / if total == 360.0 { 6.0 } else { 5.0 };
            let st = s.world_state();
            for slot in [0.0, 1.0, -1.0, 2.0, -2.0, 3.0] {
                let a = (step * slot * if flipped { -1.0 } else { 1.0 }).to_radians();
                let p = Vec3::new(40.0 + 20.0 * a.cos(), 60.0 + 20.0 * a.sin(), 2.5);
                assert!(!st.bodies[0].body.contains(p), "hole at {p:?}");
                assert!(st.bodies[0].body.contains(p + Vec3::new(2.0 * a.cos(), 2.0 * a.sin(), 0.0)));
            }
            assert!(rel(volume(&mut s), 120.0 * 120.0 * 5.0 - 6.0 * PI * 5.0) < 1e-5);
            let saved = s.doc.to_json();
            let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
            assert!(rel(volume(&mut reopened), volume(&mut s)) < 1e-8);
        }
    }
}

/// A pattern made with the timeline rolled back to the start comes before the box it copies and
/// replays the box from its definition. A change of the box's parameter reaches the copies as it
/// reaches the box: the pattern's result was kept (its copies the old length) because nothing
/// before it in the timeline had changed.
#[test]
fn a_pattern_before_the_feature_it_copies_follows_that_features_parameter() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "18 mm"}));
    run(&mut s, "solid.box", json!({"length": "w", "width": 10, "height": 10}));
    run(&mut s, "timeline.rollback", json!({"position": 0}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Box1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30}));
    let lengths = |st: &solidcraft_doc::ModelState| {
        st.bodies.iter().map(|b| (b.name.clone(), b.body.outline_bounds())).map(|(n, bb)| (n, bb.max.x - bb.min.x)).collect::<Vec<_>>()
    };
    let fresh = |s: &Session| {
        let mut m = solidcraft_doc::Model::new();
        m.no_cache = true;
        m.evaluate(&s.doc);
        lengths(&m.state())
    };
    for step in [json!({"name": "w", "expression": "21 mm"}), json!({"name": "w", "expression": "25 mm"})] {
        run(&mut s, "parameters.change", step);
        assert_eq!(lengths(&s.model.state()), fresh(&s), "rolled back");
        run(&mut s, "timeline.rollback", json!({}));
        assert_eq!(lengths(&s.model.state()), fresh(&s), "rolled forward");
        run(&mut s, "timeline.rollback", json!({"position": 1}));
    }
    run(&mut s, "timeline.rollback", json!({}));
    let got = lengths(&s.model.state());
    assert!(got.len() == 2 && got.iter().all(|(_, l)| (l - 25.0).abs() < 1e-9), "the box and its copy, 25 mm: {got:?}");
}

/// A pattern whose source was deleted copies nothing. A feature made later under the freed name
/// is found by that name, after the pattern in the timeline: the model is what a fresh evaluation
/// makes of the design, where the pattern's empty result had been kept.
#[test]
fn a_pattern_finds_a_later_feature_that_takes_its_deleted_sources_name() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 15, "axis": "Y", "base": [-16, 6, -5]}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Cylinder1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 19}));
    run(&mut s, "timeline.delete", json!({"feature": "Cylinder1"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 19, "base": [-3, -7, -1]}));
    let bodies = |st: &solidcraft_doc::ModelState| st.bodies.iter().map(|b| (b.name.clone(), b.body.outline_bounds().min.x)).collect::<Vec<_>>();
    let mut fresh = solidcraft_doc::Model::new();
    fresh.no_cache = true;
    fresh.evaluate(&s.doc);
    assert_eq!(bodies(&s.model.state()), bodies(&fresh.state()));
}

#[test]
fn feature_pattern_rejects_malformed_alignment_and_flip_atomically() {
    let mut s = feature_pattern_plate();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for (key, value) in [("alignment", json!("diagonal")), ("alignment2", json!(null)), ("reversed", json!(1)), ("reversed2", json!("true"))] {
        let mut p = json!({"features":["Hole"],"dir1":[1,0,0],"count1":4,"spacing1":10});
        p[key] = value;
        assert!(s.execute("solid.pattern.rectangular", &p).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

#[test]
fn circular_axis_origin_rejects_invalid_values_atomically() {
    let mut s = plate_with_hole();
    run(&mut s, "parameters.add", json!({"name":"cx","expression":"50 mm"}));
    run(&mut s, "parameters.add", json!({"name":"cy","expression":"25 mm"}));
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for origin in [
        json!(["cx", "cy", 0]),
        json!(["50", 25, 0]),
        json!([0, 0]),
        json!([0, 0, 0, 0]),
        json!(null),
        json!(false),
        json!({"x":0,"y":0}),
        json!([1e9, 0, 0]),
        json!([0, 1e100, 0]),
    ] {
        for (cmd, params) in [
            ("solid.pattern.circular", json!({"features":["Hole1"],"axis":{"origin":origin,"dir":[0,0,1]},"count":4})),
            ("construct.plane.at_angle", json!({"base":"XY","axis":{"origin":origin,"dir":[1,0,0]},"angle":45})),
        ] {
            let error = s.execute(cmd, &params).unwrap_err().to_string();
            assert!(error.contains("axis.origin") && error.contains("expressions are not supported"), "{error}");
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.undo.len(), undo);
        }
    }
}

#[test]
fn circular_numeric_axis_origin_preserves_hole_geometry_and_reopens() {
    for origin in [json!([50, 50, 0]), json!({"x":50,"y":50,"z":0})] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[100,100]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":10}));
        run(&mut s, "sketch.create", json!({"plane":"XY"}));
        run(&mut s, "sketch.circle.center", json!({"center":[50,20],"radius":5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance":10,"operation":"cut","name":"Hole"}));
        let before = s.doc.to_json();
        run(&mut s, "solid.pattern.circular", json!({"features":["Hole"],"axis":{"origin":origin,"dir":[0,0,1]},"count":4}));
        let expected = 100.0 * 100.0 * 10.0 - 4.0 * PI * 25.0 * 10.0;
        assert!(rel(volume(&mut s), expected) < 1e-4);
        let saved = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), saved);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!(rel(volume(&mut reopened), expected) < 1e-4);
    }
    // The documented omission of origin still means zero.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[20,0,0],"body_name":"Cube"}));
    let before = s.doc.to_json();
    run(&mut s, "solid.pattern.circular", json!({"bodies":["Cube"],"axis":{"dir":[0,0,1]},"count":4}));
    let implicit = s.doc.to_json();
    let mut explicit = Session::new(solidcraft_doc::Document::from_json(&before).unwrap());
    run(&mut explicit, "solid.pattern.circular", json!({"bodies":["Cube"],"axis":{"origin":[0,0,0],"dir":[0,0,1]},"count":4}));
    assert_eq!(implicit, explicit.doc.to_json());
}

#[test]
fn pattern_on_path_spacing_extent_and_flip() {
    let mut s = plate_with_hole();
    assert!(rel(volume(&mut s), PLATE - HOLE) < 1e-4);
    run(&mut s, "solid.pattern.path", json!({"features": ["Hole1"], "path_sketch": "Path", "path": ["l1"], "count": 5, "spacing": 20}));
    assert!(rel(volume(&mut s), PLATE - 5.0 * HOLE) < 1e-4);
    // The count is a parameter like any other input.
    let n = s.doc.features.last().and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": n, "expression": "3"}));
    assert!(rel(volume(&mut s), PLATE - 3.0 * HOLE) < 1e-4);
    // Instances past the end of the path are an error.
    run(&mut s, "parameters.change", json!({"name": n, "expression": "6"}));
    let err = s.model.results.last().and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(err.contains("past the end of the path"), "{err}");

    // Distance = first to last instance.
    let mut s = plate_with_hole();
    run(&mut s, "solid.pattern.path", json!({"features": ["Hole1"], "path_sketch": "Path", "path": ["l1"], "count": 3, "distance": 80}));
    assert!(rel(volume(&mut s), PLATE - 3.0 * HOLE) < 1e-4);

    // Flipped, the path runs from (90,10) back toward (10,10).
    let mut s = plate_with_hole();
    run(&mut s, "solid.pattern.path", json!({"features": ["Hole1"], "path_sketch": "Path", "path": ["l1"], "count": 2, "spacing": 30, "flip": true}));
    // The copy moves by path(30) − path(0) = (60,10) − (90,10) = −30: off the plate side → only 1 hole.
    assert!(rel(volume(&mut s), PLATE - HOLE) < 1e-4);
}

#[test]
fn patterns_of_bodies() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Cube"}));
    run(&mut s, "solid.pattern.circular", json!({"bodies": ["Cube"], "axis": "Z", "count": 4}));
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["body_count"], 4, "{m}");
    assert!(rel(volume(&mut s), 4000.0) < 1e-4);
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Cube"], "dir1": [0, 0, 1], "count1": 3, "spacing1": 20}));
    assert!(rel(volume(&mut s), 6000.0) < 1e-4);
    assert!(s.execute("solid.pattern.rectangular", &json!({"bodies": ["Nope"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 5})).is_err());
}

/// A construction axis picked in the dialog comes as its name (#2885): the copies turn about it,
/// and follow it when it is edited.
#[test]
fn circular_pattern_about_a_construction_axis_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [40, 0, 0], "body_name": "Cube"}));
    let axis = |x: f64| json!({"a": {"point": [x, 0, 0]}, "b": {"point": [x, 0, 10]}, "name": "Axis1"});
    run(&mut s, "construct.axis.two_points", axis(30.0));
    run(&mut s, "solid.pattern.circular", json!({"bodies": ["Cube"], "axis": "Axis1", "count": 4}));
    // The x and y extent of the four cubes.
    let span = |s: &Session| {
        let st = s.model.state();
        assert_eq!(st.bodies.len(), 4);
        let mut out = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        for b in st.bodies.iter().filter_map(|mb| solidcraft_kernel::measure(&mb.body).ok()).map(|m| m.bbox) {
            out = [out[0].min(b.min.x), out[1].max(b.max.x), out[2].min(b.min.y), out[3].max(b.max.y)];
        }
        out
    };
    // About x = 30: the cube at x 40…42 also lands at 18…20, and at y ±(10…12).
    for (got, want) in span(&s).into_iter().zip([18.0, 42.0, -12.0, 12.0]) {
        assert!((got - want).abs() < 1e-6, "{got} vs {want}");
    }
    run(&mut s, "timeline.redefine", json!({"feature": "Axis1", "command": "construct.axis.two_points", "params": axis(35.0)}));
    for (got, want) in span(&s).into_iter().zip([28.0, 42.0, -7.0, 7.0]) {
        assert!((got - want).abs() < 1e-6, "{got} vs {want}");
    }
}

#[test]
fn patterns_of_components_place_occurrences() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Peg"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = run(&mut s, "solid.pattern.rectangular", json!({"components": ["Peg"], "dir1": [0, 0, 1], "count1": 3, "spacing1": 20}));
    assert_eq!(r["occurrences"].as_array().map(Vec::len), Some(2), "{r}");
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["body_count"], 3, "{m}");
    let zs: Vec<f64> = m["bodies"].as_array().unwrap().iter().filter_map(|b| b["bbox"]["min"][2].as_f64()).collect();
    for z in [0.0, 20.0, 40.0] {
        assert!(zs.iter().any(|x| (x - z).abs() < 1e-6), "{zs:?}");
    }
    assert!(s.execute("solid.pattern.circular", &json!({"components": ["Nope"], "axis": "Z", "count": 2})).is_err());
}

/// #4763: a body of a component moved 60 mm along X, patterned from the root. The copy goes
/// where the pattern puts the body as it is shown (beside it), not where the body sits before
/// its component is placed (x 10, near the origin).
#[test]
fn patterns_of_a_body_in_a_moved_component_follow_its_placement() {
    for (kind, params, want) in [
        ("solid.pattern.path", json!({"bodies": ["Pin"], "path_sketch": "Path", "path": ["l1"], "count": 2, "spacing": 10}), [70.0, 0.0]),
        ("solid.pattern.rectangular", json!({"bodies": ["Pin"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 10}), [70.0, 0.0]),
        ("solid.pattern.circular", json!({"bodies": ["Pin"], "axis": "Z", "count": 2, "angle": "90 deg"}), [-4.0, 60.0]),
    ] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Screw"}));
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "body_name": "Pin"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        // The first part is grounded when it is made; this one is moved.
        run(&mut s, "occurrence.ground", json!({"occurrence": 2, "grounded": false}));
        run(&mut s, "occurrence.move", json!({"occurrence": 2, "translate": [60, 0, 0]}));
        run(&mut s, kind, params);
        let m = run(&mut s, "inspect.measure", json!({}));
        let bodies = m["bodies"].as_array().unwrap();
        let at = |name: &str| {
            let b = bodies.iter().find(|b| b["name"] == name).unwrap_or_else(|| panic!("{kind}: no `{name}` in {m}"));
            [b["bbox"]["min"][0].as_f64().unwrap(), b["bbox"]["min"][1].as_f64().unwrap()]
        };
        assert!((at("Pin")[0] - 60.0).abs() < 1e-6, "{kind}: the source stays at x 60");
        let copy = at("Pin (2)");
        assert!((copy[0] - want[0]).abs() < 1e-6 && (copy[1] - want[1]).abs() < 1e-6, "{kind}: the copy is at {copy:?}, want {want:?}");
    }
}

#[test]
fn path_pattern_orientation_follows_the_path() {
    let width_of_copy = |orient: &str| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 2, "width": 4, "height": 2, "corner": [19, -2, 0], "body_name": "Tab"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
        run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [20, 0], "sweep": 90, "id": "a1"}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.pattern.path",
            json!({"bodies": ["Tab"], "path_sketch": "Arc", "path": ["a1"], "count": 2, "distance": "pi * 10", "orientation": orient}),
        );
        let m = run(&mut s, "inspect.measure", json!({}));
        let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] != "Tab").unwrap().clone();
        let (lo, hi) = (b["bbox"]["min"].clone(), b["bbox"]["max"].clone());
        // The copy sits at the arc's end (0, 20).
        assert!(((lo[1].as_f64().unwrap() + hi[1].as_f64().unwrap()) / 2.0 - 20.0).abs() < 1e-6, "{b}");
        hi[0].as_f64().unwrap() - lo[0].as_f64().unwrap()
    };
    assert!((width_of_copy("identical") - 2.0).abs() < 1e-6);
    assert!((width_of_copy("path") - 4.0).abs() < 1e-6);
}

/// Patterns of a blind hole drill every copy to the full depth from the face at the copy
/// (QA s30): n copies remove n times one hole.
#[test]
fn patterns_of_a_blind_hole_drill_every_copy() {
    let one = {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 50, "width": 25, "height": 15}));
        let v0 = volume(&mut s);
        run(&mut s, "solid.hole", json!({"position": [10, 12, 15], "diameter": "5 mm", "depth": "10 mm"}));
        v0 - volume(&mut s)
    };
    let patterned = |p: (&str, Value)| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 50, "width": 25, "height": 15}));
        let v0 = volume(&mut s);
        run(&mut s, "solid.hole", json!({"position": [10, 12, 15], "diameter": "5 mm", "depth": "10 mm"}));
        run(&mut s, p.0, p.1);
        v0 - volume(&mut s)
    };
    let r = patterned(("solid.pattern.rectangular", json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": "15 mm"})));
    assert!((r - 3.0 * one).abs() < 1e-3 * one, "{r} vs 3 × {one}");
    let c = patterned((
        "solid.pattern.circular",
        json!({"features": ["Hole1"], "axis": {"origin": [25, 12, 0], "dir": [0, 0, 1]}, "count": 2, "angle": 360}),
    ));
    assert!((c - 2.0 * one).abs() < 1e-3 * one, "{c} vs 2 × {one}");
}

/// Feature patterns and mirrors reference features by id (#1134): a rename leaves the
/// references alone, and they keep pointing at the same feature (#401).
#[test]
fn renaming_a_source_feature_leaves_pattern_and_mirror_references_alone() {
    let mut s = plate_with_hole();
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": 0, "name": "Mirror plane"}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20, "name": "Copies"}));
    run(&mut s, "solid.mirror", json!({"features": ["Hole1"], "plane": "Mirror plane", "name": "Mirrored"}));
    let hole = s.doc.find_feature("Hole1").unwrap().id;
    let mirror_plane = s.doc.find_feature("Mirror plane").unwrap().id;
    let renamed = run(&mut s, "timeline.rename", json!({"feature": "Hole1", "name": "Single hole"}));
    assert_eq!(renamed["name"], "Single hole", "{renamed}");
    run(&mut s, "timeline.rename", json!({"feature": "Mirror plane", "name": "Renamed plane"}));
    for feature in ["Copies", "Mirrored"] {
        let inspected = run(&mut s, "timeline.inspect", json!({"feature": feature}));
        assert!(inspected["error"].is_null(), "{feature}: {inspected}");
        assert_eq!(inspected["definition"]["features"], json!([hole]), "{inspected}");
    }
    let inspected = run(&mut s, "timeline.inspect", json!({"feature": "Mirrored"}));
    assert_eq!(inspected["definition"]["plane"], json!({"type": "construction", "feature": mirror_plane}), "{inspected}");
}

/// timeline.edit keeps accepting seed and plane names as input: they resolve to ids (#1134).
#[test]
fn editing_a_pattern_by_seed_name_resolves_it_to_the_id() {
    let mut s = plate_with_hole();
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20, "name": "Copies"}));
    run(&mut s, "timeline.rename", json!({"feature": "Hole1", "name": "Single hole"}));
    run(&mut s, "timeline.edit", json!({"feature": "Copies", "set": {"features": ["Single hole"]}}));
    let inspected = run(&mut s, "timeline.inspect", json!({"feature": "Copies"}));
    assert!(inspected["error"].is_null(), "{inspected}");
    let hole = s.doc.find_feature("Single hole").unwrap().id;
    assert_eq!(inspected["definition"]["features"], json!([hole]), "{inspected}");
    // A name that finds no feature is refused, as the commands that take names refuse it.
    let e = s.execute("timeline.edit", &json!({"feature": "Copies", "set": {"features": ["Nope"]}})).unwrap_err().to_string();
    assert!(e.contains("Nope"), "{e}");
}

/// A plate with a `count1` × `count2` feature pattern of one through-hole cut: its volume and how
/// long the pattern took. It runs on its own thread so a hang fails the test instead of CI.
fn hole_pattern(count1: usize, count2: usize) -> (f64, std::time::Duration) {
    use std::time::{Duration, Instant};
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 200, "width": 100, "height": 1, "operation": "new_body", "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "2", "name": "H"}));
    run(&mut s, "sketch.circle.center", json!({"center": [4, 4], "diameter": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch": "H", "distance": "4", "direction": "negative", "operation": "cut", "targets": ["Plate"], "name": "Hole"}),
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let t = Instant::now();
        let r = s.execute(
            "solid.pattern.rectangular",
            &json!({"features": ["Hole"], "dir1": [1, 0, 0], "count1": count1, "spacing1": 6, "dir2": [0, 1, 0], "count2": count2, "spacing2": 6}),
        );
        let took = t.elapsed();
        let v = r.map(|_| volume(&mut s)).map_err(|e| e.to_string());
        let _ = tx.send((v, took));
    });
    let (v, took) = rx.recv_timeout(Duration::from_secs(900)).expect("the hole pattern did not finish within 900 s");
    let n = (count1 * count2) as f64;
    let v = v.expect("pattern");
    // The measure meshes the bores: each hole reads about 0.06 % small, so allow 0.1 % a hole.
    assert!((v - (200.0 * 100.0 - n * PI)).abs() < n * PI * 1e-3, "{n} holes: {v}");
    (v, took)
}

/// Whether a job four times the size took at most about twice linear: `pair` times the small job,
/// then the big one. Linear is 4×, the old quadratic costs 16× or more. The load of the other test
/// binaries can rise between the two runs (on a 16-core Mac under `cargo xtask test`, 32 circles
/// took 4.7 s and 128 took 42 s, though both together take 18 s alone), so a pair that misses is
/// measured again, up to three times. Load only ever slows a run, so each size keeps its fastest
/// time and the ratio is taken of those: a retry cannot pass a regression on a small run that a
/// spike happened to slow, once an earlier small run was fast. (A spike on the very first small
/// run can still hide one, as it could before the retries.)
fn scales_linearly(mut pair: impl FnMut() -> (std::time::Duration, std::time::Duration)) -> Result<(), String> {
    let (mut small, mut big) = (f64::INFINITY, f64::INFINITY);
    let mut runs = Vec::new();
    for _ in 0..3 {
        let (s, b) = pair();
        small = small.min(s.as_secs_f64());
        big = big.min(b.as_secs_f64());
        runs.push(format!("{s:?} then {b:?}"));
        if big < 8.0 * small + 2.0 {
            return Ok(());
        }
    }
    Err(format!("{}; fastest {small:.2} s and {big:.2} s, limit {:.2} s", runs.join(", "), 8.0 * small + 2.0))
}

#[test]
fn scales_linearly_keeps_each_sizes_fastest_run() {
    use std::time::Duration;
    let pairs = |p: Vec<(u64, u64)>| {
        let mut p = p.into_iter().map(|(s, b)| (Duration::from_secs(s), Duration::from_secs(b)));
        move || p.next().unwrap_or((Duration::from_secs(1000), Duration::from_secs(1000)))
    };
    assert!(scales_linearly(pairs(vec![(1, 4)])).is_ok());
    // A big run slowed by load, then a quiet pair.
    assert!(scales_linearly(pairs(vec![(1, 30), (1, 5)])).is_ok());
    // A quadratic cost (16×) whose later small runs a spike slowed: the fast first one still counts.
    let r = scales_linearly(pairs(vec![(1, 16), (3, 16), (3, 16)]));
    assert!(r.as_ref().is_err_and(|e| e.contains("fastest 1.00 s and 16.00 s, limit 10.00 s")), "{r:?}");
}

/// 512 identical through-holes from a feature pattern of one cut (#261). 128 took 30 s and 512 did
/// not finish in 150 s: each copy was cut into an ever more complex plate. The time must grow about
/// linearly with the count. A ratio, not a time: CI runs every test binary at once and is many
/// times slower than a machine running this alone (about 6 s for 512 holes there).
#[test]
fn pattern_of_512_hole_cuts_finishes_in_reasonable_time() {
    if let Err(missed) = scales_linearly(|| (hole_pattern(16, 8).1, hole_pattern(32, 16).1)) {
        panic!("128 holes, then 512, took {missed}");
    }
}

/// A plate with `n` Ø2 circles drawn in one sketch (16 a row, 6 mm apart) and cut by one extrude:
/// how long the cut took. It runs on its own thread so a hang fails the test instead of CI.
fn sketch_hole_cut(n: usize) -> std::time::Duration {
    use std::time::{Duration, Instant};
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 200, "width": 100, "height": 1, "operation": "new_body", "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "2", "name": "H"}));
    for i in 0..n {
        run(&mut s, "sketch.circle.center", json!({"center": [4 + (i % 16) * 6, 4 + (i / 16) * 6], "diameter": 2}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let t = Instant::now();
        let r =
            s.execute("solid.extrude", &json!({"sketch": "H", "distance": "4", "direction": "negative", "operation": "cut", "targets": ["Plate"]}));
        let took = t.elapsed();
        let v = r.map(|_| volume(&mut s)).map_err(|e| e.to_string());
        let _ = tx.send((v, took));
    });
    let (v, took) = rx.recv_timeout(Duration::from_secs(900)).expect("the cut did not finish within 900 s");
    let n = n as f64;
    let v = v.expect("cut");
    // The measure meshes the bores: each hole reads about 0.06 % small, so allow 0.1 % a hole.
    assert!((v - (200.0 * 100.0 - n * PI)).abs() < n * PI * 1e-3, "{n} holes: {v}");
    took
}

/// Many holes drawn in one sketch and cut by one extrude (#368), the usual way to draw a vent
/// field: 32 took 2 s and 128 took 60 s, though a feature pattern of the same holes is near
/// linear since #261. Linear is 4×. A ratio, not a time (see the 512-hole pattern test).
#[test]
fn one_extrude_of_many_sketch_circles_scales_linearly() {
    if let Err(missed) = scales_linearly(|| (sketch_hole_cut(32), sketch_hole_cut(128))) {
        panic!("32 circles, then 128, took {missed}");
    }
}

/// A bar with sawtooth teeth cut along one side (#400): the tooth profile drawn once, or all
/// `teeth` in one sketch. Each tooth's slope starts on the previous tooth's wall.
fn sawtooth_bar(teeth: usize, pattern: bool) -> (Session, Option<u64>) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bar"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-10, 0], "p1": [10, 200]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Bar", "distance": 4, "operation": "new", "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Tooth"}));
    if pattern {
        run(&mut s, "sketch.line", json!({"points": [[9, 4.375], [14, 2.5], [14, 4.375]], "closed": true}));
    } else {
        let mut pts = vec![json!([9, 4.375])];
        for k in 0..teeth {
            pts.push(json!([14 + 4 * k, 2.5]));
            pts.push(json!([14 + 4 * k, if k + 1 == teeth { 4.375 } else { 4.0 }]));
        }
        run(&mut s, "sketch.line", json!({"points": pts, "closed": true}));
    }
    run(&mut s, "sketch.finish", json!({}));
    // Total thickness 16 mm keeps the sawtooth cut spanning 8 mm on each side of YZ.
    run(&mut s, "solid.extrude", json!({"sketch": "Tooth", "distance": 16, "direction": "symmetric", "operation": "cut", "name": "Tooth cut"}));
    let id = pattern.then(|| {
        let r = run(&mut s, "solid.pattern.rectangular", json!({"features": ["Tooth cut"], "dir1": [0, 1, 0], "count1": teeth, "spacing1": 4}));
        r["feature"].as_u64().unwrap()
    });
    (s, id)
}

/// A feature pattern of a cut whose copies touch (#400): every copy went through the planar
/// fallback, each left split edges the next copy had to cut through (2110 kernel edges for 333),
/// and 40 teeth took 6.5 s where one sketch of all of them took 0.04 s. The pattern must build
/// what the single cut builds, with no retry and no leftover topology.
#[test]
fn pattern_of_touching_cuts_needs_no_fallback() {
    let (mut s, id) = sawtooth_bar(40, true);
    let result = s.model.result(id.unwrap()).unwrap();
    assert!(result.error.is_none() && result.warning.is_none(), "{result:?}");
    assert!(result.boolean_diagnostics.events.is_empty(), "{:?}", result.boolean_diagnostics);
    let m = run(&mut s, "inspect.measure", json!({}));
    let (mut one, _) = sawtooth_bar(40, false);
    let single = run(&mut one, "inspect.measure", json!({}));
    // Bar less 40 teeth: in the bar each is a triangle 4 long and 1.5 deep (the slope leaves the
    // top at y = 10), cut 8 mm to each side.
    let want = 20.0 * 200.0 * 4.0 - 40.0 * 0.5 * 4.0 * 1.5 * 16.0;
    for m in [&m, &single] {
        let b = &m["bodies"][0];
        assert!(rel(b["volume_mm3"].as_f64().unwrap(), want) < 1e-9, "{b}");
        assert_eq!(b["kernel"]["edges"], b["edges"], "{b}");
        assert_eq!(b["kernel"]["vertices"], b["vertices"], "{b}");
    }
    for k in ["faces", "edges", "vertices"] {
        assert_eq!(m["bodies"][0][k], single["bodies"][0][k], "{k}");
    }
}

/// A pattern of cuts beside a round, on a bar whose long top edges are rounded, takes the exact
/// planar route for every copy rather than the general boolean's retries: the teeth touch side
/// by side and never reach the rounds (#1197). Retries and volume, not timing (see
/// `scales_linearly`).
#[test]
fn pattern_of_cuts_beside_a_round_needs_no_retries() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, 0, 0], "length": 20, "width": 200, "height": 4, "name": "Bar"}));
    run(&mut s, "solid.fillet", json!({"edges": [[-10, 100, 4], [10, 100, 4]], "radius": 1}));
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "Tooth"}));
    run(&mut s, "sketch.line", json!({"points": [[20, 4], [24, 2], [24, 5], [20, 5]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    let cut =
        run(&mut s, "solid.extrude", json!({"sketch": "Tooth", "distance": 16, "direction": "symmetric", "operation": "cut", "name": "Tooth cut"}));
    let teeth =
        run(&mut s, "solid.pattern.rectangular", json!({"features": ["Tooth cut"], "dir1": [0, 1, 0], "count1": 40, "spacing1": 4, "name": "Teeth"}));
    for created in [&cut, &teeth] {
        let id = created["feature"].as_u64().unwrap();
        let result = s.model.result(id).unwrap();
        assert!(result.error.is_none() && result.warning.is_none(), "{result:?}");
        assert!(result.boolean_diagnostics.events.is_empty(), "{:?}", result.boolean_diagnostics);
    }
    // The bar, less the rounds' corners and 40 teeth (a 4 x 2 triangle below the top, 16 wide).
    let expected = 16000.0 - 400.0 * (1.0 - PI / 4.0) - 40.0 * 4.0 * 16.0;
    let v = volume(&mut s);
    assert!(rel(v, expected) < 1e-5, "{v} vs {expected}");
}

/// #2755: a cut cylinder with `targets` cuts only those bodies, and so do its pattern and mirror
/// copies: a body over the plate that the cylinder was told to leave alone keeps its volume.
#[test]
fn copies_of_a_primitive_cut_only_its_targets() {
    let body_volume = |s: &mut Session, name: &str| {
        let m = run(s, "inspect.measure", json!({"bodies": [name]}));
        m["bodies"][0]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
    };
    // The command, and the holes in the plate after it (the cylinder's and its copies).
    for (copy, holes) in [
        (json!(["solid.pattern.rectangular", {"features": ["Trou"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 30}]), 3.0),
        (json!(["solid.mirror", {"features": ["Trou"], "plane": {"origin": [50, 0, 0], "normal": [1, 0, 0]}}]), 2.0),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "body_name": "Plaque"}));
        run(&mut s, "solid.box", json!({"length": 100, "width": 60, "height": 10, "corner": [0, 0, 5], "operation": "new", "body_name": "Voisin"}));
        let cyl = json!({"radius": 3, "height": 30, "base": [10, 10, -5], "operation": "cut", "targets": ["Plaque"], "name": "Trou"});
        run(&mut s, "solid.cylinder", cyl);
        assert!(rel(body_volume(&mut s, "Voisin"), 60000.0) < 1e-9);
        let (id, p) = (copy[0].as_str().unwrap_or_default(), copy[1].clone());
        let r = run(&mut s, id, p);
        assert_eq!(r["changed_bodies"], json!(["Plaque"]), "{id}");
        assert!(rel(body_volume(&mut s, "Voisin"), 60000.0) < 1e-9, "{id} cut Voisin");
        let removed = 60000.0 - body_volume(&mut s, "Plaque");
        assert!(rel(removed, holes * PI * 9.0 * 10.0) < 1e-3, "{id}: {removed} mm³ cut from Plaque");
    }
}

/// #3378: a pattern or mirror of a join whose targets the join merged into one body copies onto
/// that body, as before #2755, instead of looking for the target the join used up.
#[test]
fn copies_of_a_join_that_merged_its_targets_join_the_merged_body() {
    for (copy, n) in [
        (json!(["solid.pattern.circular", {"features": ["spoke"], "axis": "Y", "count": 5}]), 5.0),
        (json!(["solid.mirror", {"features": ["spoke"], "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}}]), 2.0),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 66, "height": 3, "base": [0, 0, 0], "axis": "Y", "body_name": "rim"}));
        run(&mut s, "solid.cylinder", json!({"radius": 63, "height": 3, "base": [0, 0, 0], "axis": "Y", "operation": "cut", "targets": ["rim"]}));
        run(&mut s, "solid.cylinder", json!({"radius": 12, "height": 3, "base": [0, 0, 0], "axis": "Y", "body_name": "hub"}));
        let v0 = volume(&mut s);
        let spoke = json!({"length": 52, "width": 3, "height": 2.5, "corner": [12, 0, -1.25], "operation": "join", "targets": ["rim", "hub"], "name": "spoke"});
        run(&mut s, "solid.box", spoke);
        let v1 = volume(&mut s);
        let (id, p) = (copy[0].as_str().unwrap_or_default(), copy[1].clone());
        run(&mut s, id, p);
        let names = run(&mut s, "inspect.measure", json!({}))["bodies"].as_array().map(|b| b.iter().map(|x| x["name"].clone()).collect::<Vec<_>>());
        assert_eq!(names, Some(vec![json!("rim")]), "{id}: one body, the merged rim and hub");
        // Every copy adds what the seed spoke added (curved bodies measure to about 0.02 %; a
        // missing or doubled spoke is about 5 %).
        assert!(rel(volume(&mut s), v0 + n * (v1 - v0)) < 1e-3, "{id}: {} vs {}", volume(&mut s), v0 + n * (v1 - v0));
    }
}

/// #4593: a rib is the source of a pattern or mirror. Each copy is built again where it is, its
/// walls run until they meet the body there. An 80 × 80 × 5 plate with a 16 × 16 × 30 boss in the
/// middle; the rib's triangle (8, 5)-(8, 25)-(30, 5) in the XZ plane, 4 thick, is 880 mm³.
#[test]
fn patterns_and_mirrors_of_a_rib() {
    use solidcraft_geom::Vec3;
    let rib = 0.5 * 22.0 * 20.0 * 4.0;
    let ribbed = || {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 80, "width": 80, "height": 5, "corner": [-40, -40, 0]}));
        run(&mut s, "solid.box", json!({"length": 16, "width": 16, "height": 30, "corner": [-8, -8, 0], "operation": "join"}));
        run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "RibSketch"}));
        run(&mut s, "sketch.line", json!({"points": [[8, 25], [30, 5]], "ids": ["l1"]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = volume(&mut s);
        run(&mut s, "solid.rib", json!({"sketch": "RibSketch", "curves": ["l1"], "thickness": 4, "name": "Rib"}));
        assert!(rel(volume(&mut s), before + rib) < 1e-6, "the rib itself");
        (s, before)
    };
    // The issue's step: a circular pattern of the rib (here four, a quarter turn apart).
    let (mut s, before) = ribbed();
    run(&mut s, "solid.pattern.circular", json!({"features": ["Rib"], "axis": {"origin": [0, 0, -100], "dir": [0, 0, 1]}, "count": 4, "angle": 360}));
    let ribs = [Vec3::new(12.0, 0.0, 8.0), Vec3::new(0.0, 12.0, 8.0), Vec3::new(-12.0, 0.0, 8.0), Vec3::new(0.0, -12.0, 8.0)];
    let check = |s: &Session, inside: &[Vec3]| {
        let st = s.world_state();
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results.iter().map(|r| &r.error).collect::<Vec<_>>());
        assert_eq!(st.bodies.len(), 1);
        let body = &st.bodies[0].body;
        for p in inside {
            assert!(body.contains(*p), "a rib at {p:?}");
            // Beside it, across its thickness: no material (clear of the boss too).
            let side = if p.y == 0.0 { Vec3::new(0.0, 3.0, 0.0) } else { Vec3::new(3.0, 0.0, 0.0) };
            assert!(!body.contains(*p + side), "nothing beside the rib at {p:?}");
        }
    };
    check(&s, &ribs);
    assert!(rel(volume(&mut s), before + 4.0 * rib) < 1e-6, "four ribs: {} vs {}", volume(&mut s), before + 4.0 * rib);
    // A mirror of the rib across YZ.
    let (mut s, before) = ribbed();
    run(&mut s, "solid.mirror", json!({"features": ["Rib"], "plane": {"origin": [0, 0, 0], "normal": [1, 0, 0]}}));
    check(&s, &[ribs[0], ribs[2]]);
    assert!(rel(volume(&mut s), before + 2.0 * rib) < 1e-6, "two ribs: {} vs {}", volume(&mut s), before + 2.0 * rib);
}

/// #395: a pattern or mirror of features takes a pattern or mirror of features as a source: it
/// copies what that one made (its copies, not its source). The issue's design: a 40 × 30 × 20
/// box with a hole `H` at (−15, −10) and `Pat`, two more holes along x at 10 mm steps.
#[test]
fn patterns_and_mirrors_of_patterns_and_mirrors() {
    use solidcraft_geom::Vec3;
    let hole = PI * 1.5 * 1.5 * 20.0;
    let design = || {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "corner": [-20, -15, 0], "name": "Box"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Spot"}));
        run(&mut s, "sketch.circle.center", json!({"center": [-15, -10], "radius": 1.5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"sketch": "Spot", "through_all": true, "operation": "cut", "name": "H"}));
        run(&mut s, "solid.pattern.rectangular", json!({"features": ["H"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 10, "name": "Pat"}));
        s
    };
    let across_xz = json!({"origin": [0, 0, 0], "normal": [0, 1, 0]});
    let across_yz = json!({"origin": [0, 0, 0], "normal": [1, 0, 0]});
    // Each case: the steps after `design`, the holes there must be, and the spots that must
    // stay material.
    type Case = (Vec<(&'static str, Value)>, Vec<[f64; 2]>, Vec<[f64; 2]>);
    let cases: Vec<(&str, Case)> = vec![
        // The issue's step: the mirror of `Pat` is its two copies mirrored.
        (
            "mirror of a pattern",
            (vec![("solid.mirror", json!({"features": ["Pat"], "plane": across_xz}))], vec![[-5.0, 10.0], [5.0, 10.0]], vec![[-15.0, 10.0]]),
        ),
        // With its source too: the six holes the issue expects.
        (
            "mirror of a hole and its pattern",
            (
                vec![("solid.mirror", json!({"features": ["H", "Pat"], "plane": across_xz}))],
                vec![[-15.0, 10.0], [-5.0, 10.0], [5.0, 10.0]],
                vec![[15.0, 10.0]],
            ),
        ),
        (
            "pattern of a pattern",
            (
                vec![("solid.pattern.rectangular", json!({"features": ["Pat"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 20}))],
                vec![[-5.0, 10.0], [5.0, 10.0]],
                vec![[-15.0, 10.0]],
            ),
        ),
        (
            "mirror of a mirror",
            (
                vec![
                    ("solid.mirror", json!({"features": ["H"], "plane": across_yz, "name": "Mir"})),
                    ("solid.mirror", json!({"features": ["Mir"], "plane": across_xz})),
                ],
                vec![[15.0, -10.0], [15.0, 10.0]],
                vec![[-15.0, 10.0]],
            ),
        ),
        (
            "pattern of a mirror",
            (
                vec![
                    ("solid.mirror", json!({"features": ["H"], "plane": across_yz, "name": "Mir"})),
                    ("solid.pattern.rectangular", json!({"features": ["Mir"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 20})),
                ],
                vec![[15.0, -10.0], [15.0, 10.0]],
                vec![[-15.0, 10.0]],
            ),
        ),
    ];
    for (what, (steps, holes, material)) in cases {
        let mut s = design();
        let before = volume(&mut s);
        for (id, p) in steps {
            run(&mut s, id, p);
        }
        for r in &s.model.results {
            assert!(r.error.is_none(), "{what}: {}: {:?}", r.name, r.error);
        }
        let st = s.world_state();
        assert_eq!(st.bodies.len(), 1, "{what}");
        let body = &st.bodies[0].body;
        for [x, y] in &holes {
            assert!(!body.contains(Vec3::new(*x, *y, 10.0)), "{what}: a hole at ({x}, {y})");
            assert!(body.contains(Vec3::new(*x, *y + 2.5, 10.0)), "{what}: material beside ({x}, {y})");
        }
        for [x, y] in &material {
            assert!(body.contains(Vec3::new(*x, *y, 10.0)), "{what}: no hole at ({x}, {y})");
        }
        // The listed holes the steps added (in the last two cases `Mir` made the first of them).
        let added = holes.len();
        // Measured on the body's mesh, like the other tests here: to 1e-5 of the whole body.
        let after = volume(&mut s);
        let want = before - added as f64 * hole;
        assert!(rel(after, want) < 1e-5, "{what}: {after} mm³, {added} more holes than {before} = {want}");
    }
}

/// A gear's tooth extruded To Object up to the disc's top, patterned 6 times: after the disc is
/// made twice as thick, every copy reaches the new top like the tooth itself, so the whole part
/// doubles (#2121). The copies kept the 5 mm height of the face where it was picked.
#[test]
fn copies_of_a_to_object_extrude_follow_the_moved_face() {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane":"XY","name":"Disc"})),
        ("sketch.circle.center", json!({"center":[0,0],"radius":20})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.create", json!({"plane":"XY","name":"Tooth"})),
        ("sketch.rectangle.two_point", json!({"p0":[18,-2],"p1":[24,2]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"sketch":"Tooth","profiles":[0],"to":[0,0,5],"operation":"join"})),
        ("solid.pattern.circular", json!({"features":["Extrude2"],"axis":"Z","count":6,"angle":"360 deg"})),
    ] {
        run(&mut s, id, p);
    }
    let thin = volume(&mut s);
    run(&mut s, "timeline.edit", json!({"feature":"Extrude1","set":{"extent.distance":"10 mm"}}));
    let thick = volume(&mut s);
    assert!(rel(thick, 2.0 * thin) < 1e-6, "5 mm: {thin} mm³, 10 mm: {thick} mm³ (expected {})", 2.0 * thin);
}

/// A disc with an off-centre hole whose top and bottom edges are chamfered.
fn disc_with_chamfered_hole() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 30, "height": 10, "name": "Disc"}));
    run(&mut s, "solid.hole", json!({"position": [20, 0, 10], "diameter": 6, "name": "Bore"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[23, 0, 10]], "distance": 0.5, "name": "TopChamfer"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[23, 0, 0]], "distance": 0.5, "name": "BottomChamfer"}));
    s
}

#[test]
fn circular_pattern_of_a_hole_and_its_chamfers_copies_all_of_them() {
    let mut one = disc_with_chamfered_hole();
    let v1 = volume(&mut one);
    let mut s = disc_with_chamfered_hole();
    let v0 = volume(&mut s);
    assert!(rel(v0, v1) < 1e-9);
    run(&mut s, "solid.pattern.circular", json!({"features": ["Bore", "TopChamfer", "BottomChamfer"], "axis": "Z", "count": 6, "angle": 360}));
    let v = volume(&mut s);
    println!("{v1} -> {v}");
    assert!(v < v1 - 5.0 * 0.9 * (std::f64::consts::PI * 9.0 * 10.0), "pattern had no effect: {v1} -> {v}");
}

/// The features are copied in timeline order, not in the order they were picked in: a chamfer
/// picked before its hole used to find no edge ("no edge near ...").
#[test]
fn pattern_of_features_picked_in_any_order_copies_them_in_timeline_order() {
    let mut reference = disc_with_chamfered_hole();
    run(
        &mut reference,
        "solid.pattern.circular",
        json!({"features": ["Bore", "TopChamfer", "BottomChamfer"], "axis": "Z", "count": 6, "angle": 360}),
    );
    let expected = volume(&mut reference);
    for picked in [["BottomChamfer", "TopChamfer", "Bore"], ["TopChamfer", "Bore", "BottomChamfer"]] {
        let mut s = disc_with_chamfered_hole();
        run(&mut s, "solid.pattern.circular", json!({"features": picked, "axis": "Z", "count": 6, "angle": 360}));
        assert!(rel(volume(&mut s), expected) < 1e-9, "{picked:?}");
    }
}

#[test]
fn pattern_count_units_explain_how_to_use_a_parameter() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Seed"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0]], "ids": ["line"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "parameters.add", json!({"name": "columns", "expression": "3", "unit": "mm"}));
    run(&mut s, "parameters.add", json!({"name": "turns", "expression": "3", "unit": "deg"}));
    for (command, field, params) in [
        ("solid.pattern.rectangular", "count1", json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "spacing1": 10})),
        (
            "solid.pattern.rectangular",
            "count2",
            json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "spacing1": 10, "count1": 2, "dir2": [0, 1, 0], "spacing2": 10}),
        ),
        ("solid.pattern.circular", "count", json!({"bodies": ["Seed"], "axis": "Z"})),
        ("solid.pattern.path", "count", json!({"bodies": ["Seed"], "path_sketch": "Path", "path": ["line"], "spacing": 10})),
    ] {
        for (expression, message) in [
            ("columns", "`columns` is a length (mm); set its Unit to none to use it as a count"),
            ("columns * 2", "this expression is a length (mm)"),
            ("turns", "`turns` is an angle (deg); set its Unit to none to use it as a count"),
        ] {
            let mut params = params.clone();
            params[field] = json!(expression);
            let before = s.doc.to_json();
            let error = s.execute(command, &params).err().ok_or("dimensioned count was accepted")?.to_string();
            assert!(error.contains(message), "{command} {field}: {error}");
            assert_eq!(s.doc.to_json(), before);
        }
    }
    Ok(())
}

/// #9015: in an inch design, pattern spacing `baseLength - 1` subtracts one inch, not one mm.
#[test]
fn pattern_spacing_bare_offsets_follow_design_units() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "parameters.add", json!({"name": "baseLength", "expression": "4"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Seed"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "count1": 2, "spacing1": "baseLength - 1"}));
    assert_eq!(s.model.state().bodies.len(), 2);
    // Spacing is 3 in = 76.2 mm (not 3 mm): the seed is 10 in, so the copy starts at x = 76.2.
    let m = run(&mut s, "inspect.measure", json!({}));
    let bodies = m["bodies"].as_array().expect("bodies");
    let xmin = |b: &Value| b["bbox"]["min"][0].as_f64().unwrap_or(0.0);
    assert!(bodies.iter().any(|b| (xmin(b) - 76.2).abs() < 1e-6), "expected a copy spaced 3 in apart, got {m}");
}

#[test]
fn pattern_count_fractions_are_refused() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Seed"}));
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "2.5", "unit": ""}));
    for expression in [json!(2.5), json!("n"), json!("n * 2 - 2.5")] {
        let params = json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "count1": expression, "spacing1": 10});
        let error = s.execute("solid.pattern.rectangular", &params).err().ok_or("fractional count was accepted")?.to_string();
        assert!(error.contains("a count must be a whole number;"), "{error}");
        assert!(error.contains("is 2.5"), "{error}");
        if expression == json!("n") {
            assert!(error.contains("`n` is 2.5"), "{error}");
        }
    }
    Ok(())
}

#[test]
fn pattern_count_integer_parameters_survive_save_and_reject_fractional_edits() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "body_name": "Seed"}));
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "3", "unit": ""}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "count1": "n", "spacing1": 10, "name": "Copies"}));
    assert_eq!(s.model.state().bodies.len(), 3);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json())?);
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()));
    assert_eq!(reopened.model.state().bodies.len(), 3);
    let error = reopened
        .execute("timeline.edit", &json!({"feature": "Copies", "set": {"pattern": {"count1": "n / 2"}}}))
        .err()
        .ok_or("fractional edit was accepted")?
        .to_string();
    assert!(error.contains("a count must be a whole number;"), "{error}");
    let mut saved = solidcraft_doc::Document::from_json(&s.doc.to_json())?;
    for feature in &mut saved.features {
        if let solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Rectangular { count1, .. }, .. } = &mut feature.kind {
            *count1 = "n / 2".into();
        }
    }
    let invalid = Session::new(solidcraft_doc::Document::from_json(&saved.to_json())?);
    assert!(invalid.model.results.iter().any(|r| r.error.as_ref().is_some_and(|e| e.contains("a count must be a whole number;"))));
    Ok(())

/// Whether `body` has a hole through (x, y): no material there, material 2 mm beside it.
fn hole_at(s: &Session, x: f64, y: f64) -> bool {
    use solidcraft_geom::Vec3;
    let st = s.world_state();
    let body = &st.bodies[0].body;
    !body.contains(Vec3::new(x, y, 2.5)) && body.contains(Vec3::new(x, y + 2.0, 2.5))
}

/// Extent (#8523): a direction's value runs from the first copy to the last, whatever the
/// alignment; the other direction keeps its Spacing. It saves, reopens and edits back to Spacing.
#[test]
fn rectangular_extent_spans_first_to_last_copy() {
    let mut s = feature_pattern_plate();
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features":["Hole"],"dir1":[1,0,0],"count1":4,"spacing1":30,"type1":"extent","dir2":[0,1,0],"count2":3,"spacing2":15,"name":"Pattern"}),
    );
    for x in [60.0, 70.0, 80.0, 90.0] {
        for y in [60.0, 75.0, 90.0] {
            assert!(hole_at(&s, x, y), "hole at ({x}, {y})");
        }
    }
    assert!(!hole_at(&s, 100.0, 60.0) && !hole_at(&s, 60.0, 105.0));
    let saved = s.doc.to_json();
    assert!(saved.contains("\"extent1\":true") && !saved.contains("extent2"), "{saved}");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(hole_at(&reopened, 90.0, 90.0));
    // Centred, the extent is still the whole span: two copies on each side, 7.5 mm apart.
    run(&mut s, "timeline.redefine", json!({"feature":"Pattern","command":"solid.pattern.rectangular","params":{
        "features":["Hole"],"dir1":[1,0,0],"count1":5,"spacing1":30,"type1":"extent","alignment":"center"}}));
    for x in [45.0, 52.5, 60.0, 67.5, 75.0] {
        assert!(hole_at(&s, x, 60.0), "hole at x = {x}");
    }
    assert!(!hole_at(&s, 82.5, 60.0));
    // Back to Spacing: the same value now spaces neighbours.
    run(&mut s, "timeline.redefine", json!({"feature":"Pattern","command":"solid.pattern.rectangular","params":{
        "features":["Hole"],"dir1":[1,0,0],"count1":2,"spacing1":30}}));
    assert!(hole_at(&s, 90.0, 60.0));
    assert!(!s.doc.to_json().contains("extent1"));
}

/// To Object (#8523): the last copy's centre reaches the picked face, measured from the middle of
/// the copied hole, and follows the face when it moves. Each direction picks its own object.
#[test]
fn rectangular_to_object_reaches_the_face_and_follows_it() {
    let mut s = feature_pattern_plate();
    // A stop block beside the plate: its left face at x = 100, its front face at y = 130.
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":5,"corner":[100,130,0],"name":"Stop","body_name":"Stop"}));
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features":["Hole"],"dir1":[1,0,0],"count1":5,"type1":"to_object","to1":[100,135,2.5],
               "dir2":[0,1,0],"count2":4,"type2":"to_object","to2":[30,120,2.5],"name":"Pattern"}),
    );
    // 40 mm to the stop in 4 steps; 60 mm to the plate's back face in 3.
    for x in [60.0, 70.0, 80.0, 90.0, 100.0] {
        for y in [60.0, 80.0, 100.0] {
            assert!(hole_at(&s, x, y), "hole at ({x}, {y})");
        }
    }
    assert!(!hole_at(&s, 65.0, 60.0) && !hole_at(&s, 60.0, 70.0));
    // The block moves 20 mm closer: the copies close up to it.
    run(&mut s, "timeline.redefine", json!({"feature":"Stop","command":"solid.box","params":{"length":10,"width":10,"height":5,"corner":[80,130,0],"body_name":"Stop"}}));
    let err = s.model.results.iter().find_map(|r| r.error.clone());
    assert!(err.is_none(), "{err:?}");
    for x in [60.0, 65.0, 70.0, 75.0, 80.0] {
        assert!(hole_at(&s, x, 60.0) && hole_at(&s, x, 100.0), "holes at x = {x} after the move");
    }
    assert!(!hole_at(&s, 90.0, 60.0));
    // An object where the copies start is refused, and a To Object without its point.
    let e = s.execute("solid.pattern.rectangular", &json!({"features":["Hole"],"dir1":[1,0,0],"count1":3,"type1":"to_object"}));
    assert!(e.is_err_and(|e| e.to_string().contains("to1")));
    let e = s.execute("solid.pattern.rectangular", &json!({"features":["Hole"],"dir1":[1,0,0],"count1":3,"spacing1":5,"type1":"sideways"}));
    assert!(e.is_err_and(|e| e.to_string().contains("type1")));
}

/// Editing a pattern behind the history marker (double-clicking its greyed-out item and pressing
/// OK) changes nothing in the model: the reply says so, where the edit used to look as if it had
/// silently failed, and it still carries what building the copies met (#8100).
#[test]
fn editing_a_rolled_back_pattern_says_it_is_not_built() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":10,"corner":[0,0,0],"body_name":"Plate"}));
    run(&mut s, "solid.cylinder", json!({"radius":2,"height":20,"base":[10,15,-5],"operation":"cut","targets":["Plate"],"name":"Cut"}));
    let pattern = |count: u32, origin: [f64; 3]| json!({"features":["Cut"],"axis":{"origin":origin,"dir":[0,0,1]},"count":count});
    let redefine = |count, origin| json!({"feature":"Pattern","command":"solid.pattern.circular","params":pattern(count, origin)});
    let centre = [20.0, 15.0, 0.0];
    let hole = PI * 4.0 * 10.0;
    let mut first = pattern(2, centre);
    first["name"] = json!("Pattern");
    let id = run(&mut s, "solid.pattern.circular", first)["feature"].as_u64().unwrap();
    assert!(rel(volume(&mut s), 12000.0 - 2.0 * hole) < 1e-4);
    // A pattern that is built: the edit is built at once, with nothing to say.
    let reply = run(&mut s, "timeline.redefine", redefine(3, centre));
    assert_eq!(reply["rolled_back"], json!(false), "{reply}");
    assert!(reply.get("note").is_none(), "{reply}");
    assert!(rel(volume(&mut s), 12000.0 - 3.0 * hole) < 1e-4);
    // Behind the marker: the edit is kept but not built, and the reply says why nothing changed.
    run(&mut s, "timeline.roll_to", json!({"position": 2}));
    let reply = run(&mut s, "timeline.redefine", redefine(4, centre));
    assert_eq!(reply["rolled_back"], json!(true), "{reply}");
    assert_eq!(reply["note"], "Pattern is after the history marker, so it is not built: move the marker past it to build it");
    assert!(reply.get("warning").is_none(), "{reply}");
    assert!(rel(volume(&mut s), 12000.0 - hole) < 1e-4);
    assert!(s.model.result(id).is_some_and(|r| r.skipped && r.error.is_none()));
    run(&mut s, "timeline.roll_to", json!({}));
    assert!(rel(volume(&mut s), 12000.0 - 4.0 * hole) < 1e-4);
    // What building the rolled-back copies met still reaches the reply: here a copy that misses.
    run(&mut s, "timeline.roll_to", json!({"position": 2}));
    let reply = run(&mut s, "timeline.redefine", redefine(2, [0.0, 0.0, 0.0]));
    assert_eq!(reply["rolled_back"], json!(true), "{reply}");
    assert_eq!(reply["warning"], "1 of 1 cut copies remove no material", "{reply}");
}

/// To Object as an extrude's (#7780): a face running along the direction (the stop's top) is
/// reached where it begins, not at the pick on it, and To Offset moves the end, negative short.
#[test]
fn rectangular_to_object_face_along_the_direction_and_offset() {
    let mut s = feature_pattern_plate();
    run(&mut s, "solid.box", json!({"length":20,"width":10,"height":5,"corner":[100,130,0],"name":"Stop","body_name":"Stop"}));
    // Picked far along the top (x = 115): the copies still end at its near edge, x = 100.
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features":["Hole"],"dir1":[1,0,0],"count1":3,"type1":"to_object","to1":[115,135,5],"name":"Pattern"}),
    );
    for x in [60.0, 80.0, 100.0] {
        assert!(hole_at(&s, x, 60.0), "hole at x = {x}");
    }
    // 10 mm short of it: 30 mm in two steps.
    run(&mut s, "timeline.redefine", json!({"feature":"Pattern","command":"solid.pattern.rectangular","params":{
        "features":["Hole"],"dir1":[1,0,0],"count1":3,"type1":"to_object","to1":[115,135,5],"to_offset1":"-10 mm"}}));
    for x in [60.0, 75.0, 90.0] {
        assert!(hole_at(&s, x, 60.0), "hole at x = {x} with the offset");
    }
    assert!(!hole_at(&s, 100.0, 60.0));
    assert!(s.doc.to_json().contains("\"to_offset1\""));
}

/// To Object a reference, as an extrude's (#8527): a construction plane and a sketch point the
/// copies run to, followed when a parameter moves them, also once saved and opened again.
#[test]
fn rectangular_to_object_references_follow_them() {
    let mut s = feature_pattern_plate();
    run(&mut s, "parameters.add", json!({"name": "stop", "expression": "100 mm"}));
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": "stop", "name": "Stop"}));
    let marks = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.point", json!({"point": [30, "stop"], "id": "tip"}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features":["Hole"],"dir1":[1,0,0],"count1":3,"to1":{"plane":"Stop"},
               "dir2":[0,1,0],"count2":3,"to2":{"sketch":marks,"point":"tip"},"name":"Pattern"}),
    );
    let doc = s.doc.to_json();
    assert!(doc.contains("\"to1_ref\"") && doc.contains("\"to2_ref\""), "{doc}");
    for (x, y) in [(80.0, 80.0), (100.0, 100.0), (60.0, 100.0)] {
        assert!(hole_at(&s, x, y), "hole at ({x}, {y})");
    }
    run(&mut s, "parameters.change", json!({"name": "stop", "expression": "80 mm"}));
    for (x, y) in [(70.0, 70.0), (80.0, 80.0), (60.0, 80.0)] {
        assert!(hole_at(&s, x, y), "hole at ({x}, {y}) after the move");
    }
    assert!(!hole_at(&s, 100.0, 100.0));
    let restored = solidcraft_doc::format::read(&s.doc.to_json()).unwrap();
    let mut cold = Session { doc: restored.into(), ..Default::default() };
    cold.refresh();
    assert!(hole_at(&cold, 80.0, 80.0) && !hole_at(&cold, 100.0, 100.0));
}
