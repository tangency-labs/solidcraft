use solidcraft_geom::{MeshGeom, Vec3Geom};
use std::f64::consts::PI;
use std::sync::Arc;

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

/// A body mirrored as a new body is made of what the body is: the copy takes its physical
/// material and appearance. Before, the copy came out with neither.
#[test]
fn a_mirrored_copy_keeps_the_material_and_appearance_of_its_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    run(&mut s, "material.assign", json!({"bodies": ["Block"], "material": "Steel"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Block"], "color": "#d01c1c"}));
    let r = run(&mut s, "solid.mirror", json!({"bodies": ["Block"], "plane": {"origin": [30, 0, 0], "normal": [1, 0, 0]}}));
    let copy = r["created_bodies"][0].as_str().unwrap().to_string();
    assert_ne!(copy, "Block");
    assert_eq!(s.doc.resolved_body_material(&copy), Some("Steel"));
    assert_eq!(s.doc.body_material(&copy), "Steel");
    let look = s.doc.appearances.bodies.get("Block");
    assert!(look.is_some());
    assert_eq!(s.doc.appearances.bodies.get(&copy), look);
}

/// Face paint follows the copied face identity, including another mirror and a rebuild.
#[test]
fn mirrored_bodies_keep_face_appearances_after_rebuild_and_reopen() {
    fn check(s: &Session, body: &str, point: [f64; 3], color: [u8; 3], opacity: f64) {
        let state = s.model.state();
        let body = state.body(body).unwrap();
        let face = solidcraft_doc::appearance::face_index_at(body, solidcore_record::Vec3::from(point)).unwrap();
        let colors = s.doc.authored_face_colors(body);
        let look = colors.iter().find(|(index, _)| *index == face).map(|(_, look)| look);
        assert_eq!(look.map(|l| (l.color, l.opacity)), Some((color, opacity)), "{} at {point:?}", body.name);
    }

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"height", "expression":"10 mm"}));
    for (name, x, color, opacity) in [("Red", 0, "#ff0000", 1.0), ("Blue", 20, "#0000ff", 0.6)] {
        run(&mut s, "solid.box", json!({"length":10,"width":10,"height":"height","corner":[x,10,0],"body_name":name}));
        run(&mut s, "appearance.assign", json!({"body":name,"faces":[[x+5,15,10]],"color":color,"opacity":opacity}));
    }
    run(&mut s, "appearance.assign", json!({"body":"Red","faces":[[5,10,5]],"color":"#00ff00"}));
    let before = s.doc.appearances.clone();
    let reply = run(&mut s, "solid.mirror", json!({"bodies":["Blue","Red"],"plane":"XZ"}));
    let blue = reply["created_bodies"][0].as_str().unwrap().to_string();
    let red = reply["created_bodies"][1].as_str().unwrap().to_string();
    check(&s, &blue, [25., -15., 10.], [0, 0, 255], 0.6);
    check(&s, &red, [5., -15., 10.], [255, 0, 0], 1.0);
    check(&s, &red, [5., -10., 5.], [0, 255, 0], 1.0);
    assert_eq!(s.doc.appearances.faces.len(), 6);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.appearances, before);
    run(&mut s, "edit.redo", json!({}));
    check(&s, &red, [5., -15., 10.], [255, 0, 0], 1.0);
    let nested = run(&mut s, "solid.mirror", json!({"bodies":[red],"plane":"YZ"}));
    let nested = nested["created_bodies"][0].as_str().unwrap().to_string();
    check(&s, &nested, [-5., -15., 10.], [255, 0, 0], 1.0);
    check(&s, &nested, [-5., -10., 5.], [0, 255, 0], 1.0);
    run(&mut s, "parameters.change", json!({"name":"height","expression":"20 mm"}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for session in [&s, &reopened] {
        check(session, "Red", [5., 15., 20.], [255, 0, 0], 1.0);
        check(session, "Blue", [25., 15., 20.], [0, 0, 255], 0.6);
        check(session, &red, [5., -15., 20.], [255, 0, 0], 1.0);
        check(session, &blue, [25., -15., 20.], [0, 0, 255], 0.6);
        check(session, &nested, [-5., -15., 20.], [255, 0, 0], 1.0);
        check(session, &nested, [-5., -10., 5.], [0, 255, 0], 1.0);
    }
}

#[test]
fn a_mirrored_curved_face_keeps_its_appearance() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":3,"height":10,"base":[0,10,0],"body_name":"Post"}));
    run(&mut s, "appearance.assign", json!({"body":"Post","faces":[[3,10,5]],"color":"#a030d0","opacity":0.7}));
    let reply = run(&mut s, "solid.mirror", json!({"bodies":["Post"],"plane":"XZ"}));
    let copy = reply["created_bodies"][0].as_str().unwrap();
    let state = s.model.state();
    let body = state.body(copy).unwrap();
    let face = solidcraft_doc::appearance::face_index_at(body, solidcore_record::Vec3::new(3., -10., 5.)).unwrap();
    let colors = s.doc.authored_face_colors(body);
    assert_eq!(colors.len(), 1);
    assert_eq!(colors[0].0, face);
    assert_eq!((colors[0].1.color, colors[0].1.opacity), ([160, 48, 208], 0.7));
}

#[test]
fn a_mirror_over_the_face_appearance_limit_leaves_the_design_unchanged() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[0,10,0],"body_name":"Block"}));
    run(&mut s, "appearance.assign", json!({"body":"Block","faces":[[5,15,10]],"color":"#ff0000"}));
    let mut unrelated = s.doc.appearances.faces[0].clone();
    unrelated.body = "Unrelated".into();
    s.doc_mut().appearances.faces.resize(100_000, unrelated);
    let before = s.doc.clone();
    let error = s.execute("solid.mirror", &json!({"bodies":["Block"],"plane":"XZ"})).unwrap_err();
    assert!(error.to_string().contains("too many face appearances"), "{error}");
    assert_eq!(s.doc.appearances, before.appearances);
    assert_eq!(s.doc.features.len(), before.features.len());
    assert_eq!(s.model.state().bodies.len(), 1);
}

/// Whether a mirrored copy meets its body, which the Mirror dialog sets Join or New Body from:
/// apart, mirrored about its own face (touching) and through itself (overlapping), on the exact
/// geometry. A Mirror of features has no answer, and asking makes nothing.
#[test]
fn a_mirror_meets_its_body_when_the_copy_touches_or_overlaps_it() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    let meets = |x: f64| s.mirror_meets(&json!({"bodies": ["Block"], "plane": {"origin": [x, 0, 0], "normal": [1, 0, 0]}})).unwrap();
    assert_eq!(meets(30.0), Some(false), "apart");
    assert_eq!(meets(10.0), Some(true), "about its own face");
    assert_eq!(meets(7.0), Some(true), "through itself");
    assert_eq!(meets(-30.0), Some(false), "apart, on the other side");
    // A slanted plane, square to (1, 1, 0): the box reaches x + y = 20 at its far edge.
    let slanted = |x: f64| s.mirror_meets(&json!({"bodies": ["Block"], "plane": {"origin": [x, 0, 0], "normal": [1, 1, 0]}})).unwrap();
    assert_eq!(slanted(20.0), Some(true), "slanted, through its far edge");
    assert_eq!(slanted(25.0), Some(false), "slanted, apart");
    assert_eq!(s.mirror_meets(&json!({"features": ["Box1"], "plane": "YZ"})).unwrap(), None, "a Mirror of features");
    assert_eq!(s.model.state().bodies.len(), 1, "nothing is made");
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
fn a_hole_pattern_keeps_each_copied_wall_with_its_sketch_point() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 60, "height": 8, "corner": [0, 0, 0], "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 8, "name": "HolePoints"}));
    for x in [20, 50] {
        run(&mut s, "sketch.point", json!({"point": [x, 15]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "HolePoints", "points": ["p1", "p2"], "diameter": 4, "name": "Holes"}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Holes"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 20}));
    let copy_wall = |s: &mut Session, x: f64| {
        let result = run(s, "model.faces", json!({"body": "Plate"}));
        let face = result["faces"].as_array().unwrap().iter().find(|face| {
            let axis = &face["cylinder"]["origin"];
            axis[0].as_f64().is_some_and(|at| (at - x).abs() < 1e-6) && axis[1].as_f64().is_some_and(|at| (at - 35.0).abs() < 1e-6)
        });
        face.and_then(|face| face["name"].as_str()).map(str::to_string)
    };
    let left = copy_wall(&mut s, 20.0).unwrap();
    let right = copy_wall(&mut s, 50.0).unwrap();
    assert!(left.contains(":point:p1:") && right.contains(":point:p2:"), "{left}, {right}");
    run(&mut s, "sketch.edit", json!({"sketch": "HolePoints"}));
    run(&mut s, "sketch.delete", json!({"entities": ["p1"]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(copy_wall(&mut s, 20.0), None);
    assert_eq!(copy_wall(&mut s, 50.0), Some(right));
}

#[test]
fn a_root_pattern_recuts_the_source_component_after_its_plate_grows() {
    for consume_root_body in [false, true] {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "L", "expression": "40 mm"}));
        run(&mut s, "component.create", json!({"name": "Part"}));
        run(&mut s, "solid.box", json!({"length": "L", "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Plate"}));
        run(&mut s, "solid.box", json!({"length": 6, "width": 6, "height": 20, "corner": [4, 2, -5], "operation": "cut", "name": "First"}));
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 20, "corner": [12, 12, -5], "operation": "cut", "name": "Pocket"}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        let features = if consume_root_body {
            // The first copy removes the root's only body. The next copy then acts on the plate.
            run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [56, 4, 1], "body_name": "Small"}));
            vec!["First", "Pocket"]
        } else {
            vec!["Pocket"]
        };
        let pattern = run(&mut s, "solid.pattern.rectangular", json!({"features": features, "dir1": [1, 0, 0], "count1": 2, "spacing1": 50}));
        assert_eq!(pattern["warning"], format!("1 of {} cut copies remove no material", features.len()));

        run(&mut s, "parameters.change", json!({"name": "L", "expression": "70 mm"}));
        let expected = 70.0 * 20.0 * 10.0 - 6.0 * 6.0 * 10.0 - 2.0 * 4.0 * 4.0 * 10.0;
        let measured = volume(&mut s);
        assert!((measured - expected).abs() < 1e-6, "{features:?}: resized plate measures {measured}, expected {expected}");
        let result = s.model.result(pattern["feature"].as_u64().unwrap()).unwrap();
        assert!(result.error.is_none() && result.warning.is_none(), "{result:?}");
    }
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
    let state = s.world_state().unwrap();
    let plate = &state.body("Plate").unwrap().body;
    assert_eq!(plate.classify_point(solidcore_record::Vec3::new(30., 15., 5.)).unwrap(), solidcraft_kernel::PointClass::Outside);
    assert_eq!(
        state.body("Neighbor").unwrap().body.classify_point(solidcore_record::Vec3::new(50., 15., 5.)).unwrap(),
        solidcraft_kernel::PointClass::Inside
    );
    assert!(rel(volume(&mut s), 24000.0 - 2.0 * PI * 4.0 * 10.0) < 1e-3);
}

#[test]
fn feature_rectangular_center_flip_and_oblique_direction_keep_source_and_recompute() {
    use solidcore_record::Vec3;
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
        let st = s.world_state().unwrap();
        let body = &st.bodies[0].body;
        // Analytic slots: the source, +1, -1, +2. The second direction is independent.
        for i in [0.0, 1.0, -1.0, 2.0] {
            for j in [0.0, 1.0] {
                let p = Vec3::new(60.0 + sign * (15.0 * i + 12.0 * j), 60.0 + sign * 16.0 * j, 2.5);
                assert_eq!(body.classify_point(p).unwrap(), solidcraft_kernel::PointClass::Outside, "hole at {p:?}");
                assert_eq!(body.classify_point(p + Vec3::new(0.0, 2.0, 0.0)).unwrap(), solidcraft_kernel::PointClass::Inside);
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
    use solidcore_record::Vec3;
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
            let st = s.world_state().unwrap();
            for slot in [0.0, 1.0, -1.0, 2.0, -2.0, 3.0] {
                let a = (step * slot * if flipped { -1.0 } else { 1.0 }).to_radians();
                let p = Vec3::new(40.0 + 20.0 * a.cos(), 60.0 + 20.0 * a.sin(), 2.5);
                assert_eq!(st.bodies[0].body.classify_point(p).unwrap(), solidcraft_kernel::PointClass::Outside, "hole at {p:?}");
                assert_eq!(
                    st.bodies[0].body.classify_point(p + Vec3::new(2.0 * a.cos(), 2.0 * a.sin(), 0.0)).unwrap(),
                    solidcraft_kernel::PointClass::Inside
                );
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
    let invalid = [json!([0, 0]), json!([0, 0, 0, 0]), json!(null), json!(false), json!({"x":0,"y":0}), json!([1e9, 0, 0]), json!([0, 1e100, 0])];
    // A circular pattern's origin and an angled plane's both take length expressions (#10852),
    // so both refuse the same values.
    for origin in invalid.iter().chain(&[json!(["nope", 0, 0]), json!(["2 deg", 0, 0])]) {
        let params = json!({"features":["Hole1"],"axis":{"origin":origin,"dir":[0,0,1]},"count":4});
        let error = s.execute("solid.pattern.circular", &params).unwrap_err().to_string();
        assert!(error.contains("axis.origin"), "{origin}: {error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
        let params = json!({"base":"XY","axis":{"origin":origin,"dir":[1,0,0]},"angle":45});
        let error = s.execute("construct.plane.at_angle", &params).unwrap_err().to_string();
        assert!(error.contains("axis.origin"), "{origin}: {error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

/// #10852: a circular pattern about `{origin, dir}` with length expressions in the origin turns
/// about the point they give now, so the copies follow the parameters as the seed does. The
/// expressions are saved, renamed with their parameter, and can be set by `timeline.edit`.
#[test]
fn circular_axis_origin_expressions_follow_their_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"cx","expression":"50 mm"}));
    run(&mut s, "parameters.add", json!({"name":"cy","expression":"40 mm"}));
    // A 2 mm cube 20 mm from (cx, cy), turned about the vertical line there.
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":["cx + 20 mm","cy",0],"body_name":"Cube"}));
    run(&mut s, "solid.pattern.circular", json!({"bodies":["Cube"],"axis":{"origin":["cx","cy",0],"dir":[0,0,1]},"count":4}));
    // The x and y extent of the four cubes: 22 mm either side of the axis while it is 20 mm away.
    let span = |s: &Session| {
        let st = s.model.state();
        assert_eq!(st.bodies.len(), 4);
        let mut out = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        for b in st.bodies.iter().filter_map(|mb| solidcraft_kernel::measure(&mb.body).ok()).map(|m| m.bbox) {
            out = [out[0].min(b.min.x), out[1].max(b.max.x), out[2].min(b.min.y), out[3].max(b.max.y)];
        }
        out
    };
    let about_at = |s: &Session, x: f64, y: f64, half: f64| {
        for (got, want) in span(s).into_iter().zip([x - half, x + half, y - half, y + half]) {
            assert!((got - want).abs() < 1e-6, "{:?} is not {half} mm either side of ({x}, {y})", span(s));
        }
    };
    let about = |s: &Session, x: f64, y: f64| about_at(s, x, y, 22.0);
    // The origin as the pattern saves it.
    let saved_origin = |s: &Session| match s.doc.features.last().map(|f| &f.kind) {
        Some(solidcraft_doc::FeatureKind::Pattern { pattern, .. }) => serde_json::to_value(pattern).unwrap_or_default()["origin"].clone(),
        other => panic!("not a pattern: {other:?}"),
    };
    about(&s, 50.0, 40.0);
    assert_eq!(saved_origin(&s), json!(["cx", "cy", 0.0]), "the expressions are saved");
    run(&mut s, "parameters.change", json!({"name":"cx","expression":"60 mm"}));
    about(&s, 60.0, 40.0);
    // Renaming a parameter renames it in the origin, and the pattern still follows it.
    run(&mut s, "parameters.rename", json!({"name":"cy","new_name":"py"}));
    run(&mut s, "parameters.change", json!({"name":"py","expression":"45 mm"}));
    about(&s, 60.0, 45.0);
    assert_eq!(saved_origin(&s), json!(["cx", "py", 0.0]));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap_or_else(|e| panic!("{e}")));
    about(&reopened, 60.0, 45.0);
    // timeline.edit sets the origin as expressions too: 10 mm back, so the cube is 30 mm away.
    let pattern = s.doc.features.last().map(|f| f.id).unwrap_or_default();
    run(&mut s, "timeline.edit", json!({"feature":pattern,"set":{"pattern.origin":["cx - 10 mm","py","0 mm"]}}));
    about_at(&s, 50.0, 45.0, 32.0);
    run(&mut s, "edit.undo", json!({}));
    about(&s, 60.0, 45.0);
    // Plain numbers are saved as numbers, as before.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[70,40,0],"body_name":"Cube"}));
    run(&mut s, "solid.pattern.circular", json!({"bodies":["Cube"],"axis":{"origin":[50,40,0],"dir":[0,0,1]},"count":4}));
    about(&s, 50.0, 40.0);
    assert_eq!(saved_origin(&s), json!([50.0, 40.0, 0.0]));
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

/// The bounding boxes of the bodies, `[min, max]` each, in order of their lowest x, then y.
fn body_boxes(s: &mut Session) -> Vec<[[f64; 3]; 2]> {
    let m = run(s, "inspect.measure", json!({}));
    let at = |b: &Value, k: &str| [0, 1, 2].map(|i| b["bbox"][k][i].as_f64().unwrap_or(f64::NAN));
    let mut boxes: Vec<[[f64; 3]; 2]> = m["bodies"].as_array().unwrap().iter().map(|b| [at(b, "min"), at(b, "max")]).collect();
    boxes.sort_by(|a, b| a[0][0].total_cmp(&b[0][0]).then(a[0][1].total_cmp(&b[0][1])));
    boxes
}

fn assert_box(got: &[[f64; 3]; 2], want: [[f64; 3]; 2]) {
    let off = (0..2).flat_map(|k| (0..3).map(move |i| (k, i))).map(|(k, i)| (got[k][i] - want[k][i]).abs()).fold(0.0, f64::max);
    assert!(off < 1e-6, "{got:?} vs {want:?}");
}

/// #16084: Pattern on Path follows a chain of a body's edges. A 2 mm cube on the top front edge
/// of a 100 long block, copied three times 20 apart along that edge: the path starts at the
/// edge's end nearest the first point, so a point near x = 0 runs the copies to +x, a point near
/// x = 100 runs them to -x, as Flip does.
#[test]
fn pattern_on_path_follows_a_bodys_edge_from_the_end_nearest_the_first_point() {
    let start = |s: &mut Session| {
        run(s, "solid.box", json!({"length": 100, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        run(s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [-1, -1, 10], "body_name": "Pin"}));
    };
    let pin = |x: f64| [[x - 1.0, -1.0, 10.0], [x + 1.0, 1.0, 12.0]];
    for (edge, flip, xs) in
        [([5.0, 0.0, 10.0], false, [0.0, 20.0, 40.0]), ([95.0, 0.0, 10.0], false, [-40.0, -20.0, 0.0]), ([5.0, 0.0, 10.0], true, [-40.0, -20.0, 0.0])]
    {
        let mut s = Session::default();
        start(&mut s);
        run(
            &mut s,
            "solid.pattern.path",
            json!({"bodies": ["Pin"], "path_body": "Block", "path_edges": [edge], "count": 3, "spacing": 20, "flip": flip}),
        );
        let boxes = body_boxes(&mut s);
        assert_eq!(boxes.len(), 4, "{boxes:?}");
        // The block is the box from x = 0 with y from 0; the pins are the three others.
        let pins: Vec<_> = boxes.iter().filter(|b| b[0][1] < -0.5).collect();
        assert_eq!(pins.len(), 3, "{boxes:?}");
        for (got, x) in pins.iter().zip(xs) {
            assert_box(got, pin(x));
        }
    }
}

/// #16084: a closed chain of edges starts where the first point picks it. A 2 mm cube centred
/// on the front of the top outline of a 40 x 20 x 10 block with its upright edges rounded 4,
/// picked there, is copied 10 or 20 along the outline: 10 stays on the 32 long front edge, at
/// x = 20 ± 10, and 20 runs 16 to the corner and 4 round its radius 4 arc, a turn of 1 radian:
/// at x = 20 ± (16 + 4 sin 1), y = 4 - 4 cos 1. The chain may run either way round.
#[test]
fn pattern_on_path_round_a_closed_outline_starts_at_the_first_point() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s, "solid.fillet", json!({"edges": [[0, 0, 5], [40, 0, 5], [0, 20, 5], [40, 20, 5]], "radius": 4, "tangent_chain": false}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [19, -1, 11], "body_name": "Pin"}));
    let c = 8.0 * std::f64::consts::FRAC_1_SQRT_2;
    let outline = json!([
        [20, 0, 10],
        [36.0 + c / 2.0, 4.0 - c / 2.0, 10],
        [40, 10, 10],
        [36.0 + c / 2.0, 16.0 + c / 2.0, 10],
        [20, 20, 10],
        [4.0 - c / 2.0, 16.0 + c / 2.0, 10],
        [0, 10, 10],
        [4.0 - c / 2.0, 4.0 - c / 2.0, 10]
    ]);
    run(&mut s, "solid.pattern.path", json!({"bodies": ["Pin"], "path_body": "Block", "path_edges": outline, "count": 2, "spacing": 10}));
    let pins: Vec<_> = body_boxes(&mut s).into_iter().filter(|b| b[1][2] > 12.5).collect();
    assert_eq!(pins.len(), 2, "{pins:?}");
    let centre = |b: &[[f64; 3]; 2]| [0, 1, 2].map(|i| (b[0][i] + b[1][i]) / 2.0);
    let xs: Vec<f64> = pins.iter().map(|b| centre(b)[0]).collect();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(pins.iter().all(|b| near(centre(b)[1], 0.0) && near(centre(b)[2], 12.0)), "{pins:?}");
    assert!((near(xs[0], 10.0) && near(xs[1], 20.0)) || (near(xs[0], 20.0) && near(xs[1], 30.0)), "{xs:?}");
    // One step of 20: past the end of the straight, 4 round the corner.
    let mut s2 = Session::default();
    run(&mut s2, "solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s2, "solid.fillet", json!({"edges": [[0, 0, 5], [40, 0, 5], [0, 20, 5], [40, 20, 5]], "radius": 4, "tangent_chain": false}));
    run(&mut s2, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [19, -1, 11], "body_name": "Pin"}));
    run(&mut s2, "solid.pattern.path", json!({"bodies": ["Pin"], "path_body": "Block", "path_edges": outline, "count": 2, "spacing": 20}));
    let copy = body_boxes(&mut s2).into_iter().filter(|b| b[1][2] > 12.5).map(|b| centre(&b)).find(|p| !near(p[0], 20.0)).unwrap();
    let dx = 16.0 + 4.0 * 1f64.sin();
    assert!((near(copy[0], 20.0 + dx) || near(copy[0], 20.0 - dx)) && near(copy[1], 4.0 - 4.0 * 1f64.cos()) && near(copy[2], 12.0), "{copy:?}");
}

/// #16084: copies along a body's circular edge, turning with the path, go round its centre: a
/// 4 x 2 x 2 bar lying across the top edge of a radius 20 cylinder, copied four times a quarter
/// of the circle (10 pi) apart, stands across the edge at 90, 180 and 270 degrees. A chain that
/// leaves every plane (three edges of a block's corner) cannot turn the copies, and says so.
#[test]
fn pattern_on_path_turns_copies_round_a_bodys_circular_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 10, "body_name": "Can"}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 2, "height": 2, "corner": [18, -1, 10], "body_name": "Bar"}));
    run(
        &mut s,
        "solid.pattern.path",
        json!({"bodies": ["Bar"], "path_body": "Can", "path_edges": [[20, 0, 10]], "count": 4, "spacing": format!("{} mm", 10.0 * PI), "orientation": "path"}),
    );
    let boxes = body_boxes(&mut s);
    assert_eq!(boxes.len(), 5, "{boxes:?}");
    let bars: Vec<_> = boxes.iter().filter(|b| b[1][2] > 11.0).collect();
    assert_eq!(bars.len(), 4, "{boxes:?}");
    let want = [
        [[-22.0, -1.0, 10.0], [-18.0, 1.0, 12.0]],
        [[-1.0, -22.0, 10.0], [1.0, -18.0, 12.0]],
        [[-1.0, 18.0, 10.0], [1.0, 22.0, 12.0]],
        [[18.0, -1.0, 10.0], [22.0, 1.0, 12.0]],
    ];
    for (got, want) in bars.iter().zip(want) {
        assert_box(got, want);
    }

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [-1, -1, -1], "body_name": "Pin"}));
    let corner = json!({"bodies": ["Pin"], "path_body": "Block", "path_edges": [[20, 0, 0], [40, 0, 5], [40, 10, 10]], "count": 2, "spacing": 30});
    run(&mut s, "solid.pattern.path", corner.clone());
    assert!(s.model.results.iter().all(|r| r.error.is_none()));
    let mut turned = corner;
    turned["orientation"] = json!("path");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [-1, -1, -1], "body_name": "Pin"}));
    let refused = s.execute("solid.pattern.path", &turned).map(|_| s.model.results.last().and_then(|r| r.error.clone()).unwrap_or_default());
    let message = match refused {
        Ok(error) => error,
        Err(e) => e.to_string(),
    };
    assert!(message.contains("only along a path in one plane"), "{message}");
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

/// The copies' offsets along x and y of the last rectangular pattern `p` adds to a 10 mm cube,
/// sorted, and the stored pattern.
fn rectangular_offsets(p: Value) -> (Vec<[f64; 2]>, Session) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    let mut p = p;
    p["bodies"] = json!(["Cube"]);
    run(&mut s, "solid.pattern.rectangular", p);
    let Some(solidcraft_doc::FeatureKind::Pattern { pattern, .. }) = s.doc.features.last().map(|f| &f.kind) else {
        panic!("no pattern");
    };
    let mats = solidcraft_doc::pattern_matrices(&s.doc, &s.model.state(), pattern).unwrap();
    let mut offsets: Vec<[f64; 2]> = mats.iter().map(|m| [m[3][0], m[3][1]]).collect();
    offsets.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (offsets, s)
}

fn assert_offsets(got: &[[f64; 2]], want: &[[f64; 2]]) {
    assert_eq!(got.len(), want.len(), "{got:?} != {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g[0] - w[0]).abs() < 1e-9 && (g[1] - w[1]).abs() < 1e-9, "{got:?} != {want:?}");
    }
}

/// Each direction of a Rectangular Pattern is set by count and spacing, count and extent, or
/// spacing and extent (#8523): 80 mm at 15 mm fits 6 copies, the last at 75 mm, and Copy at the
/// end adds a 7th at 80 mm; a spacing that divides the extent adds none.
#[test]
fn rectangular_pattern_distance_types_place_the_copies() {
    let x = |v: &[f64]| v.iter().map(|x| [*x, 0.0]).collect::<Vec<_>>();
    let (got, s) = rectangular_offsets(json!({"dir1": [1, 0, 0], "distance1": "count_extent", "count1": 5, "extent1": 80}));
    assert_offsets(&got, &x(&[20.0, 40.0, 60.0, 80.0]));
    // The worked-out spacing is kept, so switching back to count and spacing finds it.
    let Some(solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Rectangular { spacing1, .. }, .. }) =
        s.doc.features.last().map(|f| &f.kind)
    else {
        panic!("no rectangular pattern");
    };
    assert_eq!(spacing1.as_str(), "20 mm");
    let (got, s) = rectangular_offsets(json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 15, "extent1": 80}));
    assert_offsets(&got, &x(&[15.0, 30.0, 45.0, 60.0, 75.0]));
    let Some(solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Rectangular { count1, .. }, .. }) =
        s.doc.features.last().map(|f| &f.kind)
    else {
        panic!("no rectangular pattern");
    };
    assert_eq!(count1.as_str(), "6");
    let (got, _) =
        rectangular_offsets(json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 15, "extent1": 80, "copy_at_end1": true}));
    assert_offsets(&got, &x(&[15.0, 30.0, 45.0, 60.0, 75.0, 80.0]));
    let (got, _) =
        rectangular_offsets(json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 20, "extent1": 80, "copy_at_end1": true}));
    assert_offsets(&got, &x(&[20.0, 40.0, 60.0, 80.0]));
    // Float noise must not drop or double the copy at the end: 0.3 / 0.1 is 2.9999…
    let (got, _) =
        rectangular_offsets(json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 0.1, "extent1": 0.3, "copy_at_end1": true}));
    assert_eq!(got.len(), 3, "{got:?}");
    // Flip runs the row the other way. Centred, the copies reach half the extent each way, as
    // many on each side: 40 mm at 15 mm fits two, and Copy at the end adds one at each end.
    let (got, _) = rectangular_offsets(
        json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 15, "extent1": 80, "copy_at_end1": true, "reversed": true}),
    );
    assert_offsets(&got, &x(&[-80.0, -75.0, -60.0, -45.0, -30.0, -15.0]));
    let (got, _) = rectangular_offsets(
        json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 15, "extent1": 80, "copy_at_end1": true, "alignment": "center"}),
    );
    assert_offsets(&got, &x(&[-40.0, -30.0, -15.0, 15.0, 30.0, 40.0]));
    let (got, _) =
        rectangular_offsets(json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 15, "extent1": 80, "alignment": "center"}));
    assert_offsets(&got, &x(&[-30.0, -15.0, 15.0, 30.0]));
    // Each direction chooses on its own.
    let (got, _) = rectangular_offsets(json!({
        "dir1": [1, 0, 0], "count1": 2, "spacing1": 30,
        "dir2": [0, 1, 0], "distance2": "count_extent", "count2": 3, "extent2": 50
    }));
    assert_offsets(&got, &[[0.0, 25.0], [0.0, 50.0], [30.0, 0.0], [30.0, 25.0], [30.0, 50.0]]);
}

/// The command works a row out with the values the pattern is built with, so an expression's
/// bare number is read in the design's unit there too (#8523): in an inch design `2 + 0` is a
/// 2 in extent, not 2 mm that a 1 in spacing would not fit in. The spacings are checked the same
/// way before that: with `inset` at 1 mm, `1 - inset` is 24.4 mm there, not the 0 mm a division would
/// refuse.
#[test]
fn rectangular_pattern_extent_reads_an_expression_in_the_design_unit() {
    let pattern = |p: Value| {
        let mut s = Session::default();
        run(&mut s, "document.units", json!({"units": "in"}));
        run(&mut s, "parameters.add", json!({"name": "inset", "expression": "1 mm"}));
        run(&mut s, "solid.box", json!({"length": "10 mm", "width": "10 mm", "height": "10 mm", "body_name": "Cube"}));
        let mut p = p;
        p["bodies"] = json!(["Cube"]);
        run(&mut s, "solid.pattern.rectangular", p);
        let Some(solidcraft_doc::FeatureKind::Pattern { pattern, .. }) = s.doc.features.last().map(|f| f.kind.clone()) else {
            panic!("no pattern");
        };
        let mats = solidcraft_doc::pattern_matrices(&s.doc, &s.model.state(), &pattern).unwrap();
        let mut xs: Vec<f64> = mats.iter().map(|m| m[3][0]).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (xs, pattern)
    };
    let (xs, p) = pattern(json!({"dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": "1 in", "extent1": "2 + 0"}));
    assert_eq!(xs.len(), 2, "{xs:?}");
    assert!((xs[0] - 25.4).abs() < 1e-9 && (xs[1] - 50.8).abs() < 1e-9, "{xs:?}");
    let solidcraft_doc::PatternKind::Rectangular { count1, .. } = &p else { panic!("not rectangular") };
    assert_eq!(count1.as_str(), "3");
    let (xs, p) = pattern(json!({"dir1": [1, 0, 0], "distance1": "count_extent", "count1": 3, "extent1": "2 + 0"}));
    assert_eq!(xs.len(), 2, "{xs:?}");
    assert!((xs[1] - 50.8).abs() < 1e-9, "{xs:?}");
    let solidcraft_doc::PatternKind::Rectangular { spacing1, .. } = &p else { panic!("not rectangular") };
    assert_eq!(spacing1.as_str(), "25.4 mm");
    let gap = 10.0 / 24.4;
    for p in [
        json!({"dir1": [1, 0, 0], "count1": 2, "spacing1": "10 mm * 1 mm / (1 - inset)"}),
        json!({"dir1": [1, 0, 0], "distance1": "count_list", "count1": 2, "spacing_list1": ["10 mm * 1 mm / (1 - inset)"]}),
    ] {
        let (xs, _) = pattern(p.clone());
        assert!(xs.len() == 1 && (xs[0] - gap).abs() < 1e-9, "{p}: {xs:?}");
    }
}

/// A direction set by count and spacing list takes the list's gaps in turn and from the start
/// again: 10, 20 and 30 mm with a count of 5 put the copies at 10, 30, 60 and 70 mm. Centred, the
/// list runs out from the original on both sides; Flip runs it the other way.
#[test]
fn rectangular_pattern_spacing_list_places_the_copies() {
    let x = |v: &[f64]| v.iter().map(|x| [*x, 0.0]).collect::<Vec<_>>();
    let list = |extra: Value| {
        let mut p = json!({"dir1": [1, 0, 0], "distance1": "count_list", "count1": 5, "spacing_list1": [10, "20 mm", 30]});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        rectangular_offsets(p)
    };
    let (got, s) = list(json!({}));
    assert_offsets(&got, &x(&[10.0, 30.0, 60.0, 70.0]));
    // The list is saved as given; the spacing keeps its first gap for switching back.
    let Some(solidcraft_doc::FeatureKind::Pattern {
        pattern: solidcraft_doc::PatternKind::Rectangular { spacing1, spacing_list1, distance1, .. },
        ..
    }) = s.doc.features.last().map(|f| &f.kind)
    else {
        panic!("no rectangular pattern");
    };
    assert_eq!(*distance1, solidcraft_doc::PatternDistance::CountList);
    assert_eq!(spacing_list1.iter().map(|e| e.as_str()).collect::<Vec<_>>(), ["10", "20 mm", "30"]);
    assert_eq!(spacing1.as_str(), "10 mm");
    let (got, _) = list(json!({"count1": 7}));
    assert_offsets(&got, &x(&[10.0, 30.0, 60.0, 70.0, 90.0, 120.0]));
    let (got, _) = list(json!({"reversed": true}));
    assert_offsets(&got, &x(&[-70.0, -60.0, -30.0, -10.0]));
    let (got, _) = list(json!({"alignment": "center"}));
    assert_offsets(&got, &x(&[-30.0, -10.0, 10.0, 30.0]));
    // An even count centred: the extra copy goes ahead, or behind with Flip.
    let (got, _) = list(json!({"alignment": "center", "count1": 4}));
    assert_offsets(&got, &x(&[-10.0, 10.0, 30.0]));
    let (got, _) = list(json!({"alignment": "center", "count1": 4, "reversed": true}));
    assert_offsets(&got, &x(&[-30.0, -10.0, 10.0]));
    let (got, _) = list(json!({"alignment": "left"}));
    assert_offsets(&got, &x(&[-70.0, -60.0, -30.0, -10.0]));
    // Each direction chooses on its own.
    let (got, _) = rectangular_offsets(json!({
        "dir1": [1, 0, 0], "count1": 2, "spacing1": 30,
        "dir2": [0, 1, 0], "distance2": "count_list", "count2": 3, "spacing_list2": [5, 15]
    }));
    assert_offsets(&got, &[[0.0, 5.0], [0.0, 20.0], [30.0, 0.0], [30.0, 5.0], [30.0, 20.0]]);
    // A list on a direction set another way isn't kept.
    let (_, s) = rectangular_offsets(json!({"dir1": [1, 0, 0], "count1": 3, "spacing1": 10, "spacing_list1": [4, 5]}));
    assert!(!s.doc.to_json().contains("spacing_list1"));
}

/// The spacing list is saved, undone and reopened with the design, and its gaps follow
/// parameters.
#[test]
fn rectangular_pattern_spacing_list_saves_reopens_and_follows_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "gap", "expression": "10 mm"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let before = s.doc.to_json();
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"bodies": ["Cube"], "dir1": [1, 0, 0], "distance1": "count_list", "count1": 4, "spacing_list1": ["gap", "2 * gap"]}),
    );
    let reach = |s: &Session| {
        let st = s.model.state();
        st.bodies.iter().filter_map(|mb| solidcraft_kernel::measure(&mb.body).ok()).map(|m| m.bbox.max.x).fold(f64::NEG_INFINITY, f64::max)
    };
    // Copies at 10, 30 and 40 mm.
    assert_eq!(s.model.state().bodies.len(), 4);
    assert!((reach(&s) - 42.0).abs() < 1e-6, "{}", reach(&s));
    let saved = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert_eq!(reopened.model.state().bodies.len(), 4);
    run(&mut reopened, "parameters.change", json!({"name": "gap", "expression": "20 mm"}));
    // Now at 20, 60 and 80 mm.
    assert!((reach(&reopened) - 82.0).abs() < 1e-6, "{}", reach(&reopened));
    // In an inch design a bare number inside an expression is inches, for the copies and for the
    // spacing kept for switching back alike.
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "solid.box", json!({"length": "2 mm", "width": "2 mm", "height": "2 mm", "body_name": "Cube"}));
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"bodies": ["Cube"], "dir1": [1, 0, 0], "distance1": "count_list", "count1": 3, "spacing_list1": ["1 + 0"]}),
    );
    assert!((reach(&s) - 52.8).abs() < 1e-6, "{}", reach(&s));
    let Some(solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Rectangular { spacing1, .. }, .. }) =
        s.doc.features.last().map(|f| &f.kind)
    else {
        panic!("no rectangular pattern");
    };
    assert_eq!(spacing1.as_str(), "25.4 mm");
}

/// The distance type, the extent and Copy at the end are saved, undone and reopened with the
/// design, and the extent follows a parameter; a pattern set by count and spacing saves none of
/// them, so older designs are unchanged.
#[test]
fn rectangular_pattern_extent_saves_reopens_and_follows_parameters() {
    let (_, s) = rectangular_offsets(json!({"dir1": [1, 0, 0], "count1": 3, "spacing1": 20}));
    let plain = s.doc.to_json();
    for key in ["distance1", "extent1", "copy_at_end1"] {
        assert!(!plain.contains(key), "{key} in {plain}");
    }
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "span", "expression": "80 mm"}));
    // Small enough that the copy 5 mm past the last stays a body of its own.
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Cube"}));
    let before = s.doc.to_json();
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"bodies": ["Cube"], "dir1": [1, 0, 0], "distance1": "spacing_extent", "spacing1": 15, "extent1": "span", "copy_at_end1": true}),
    );
    assert_eq!(s.model.state().bodies.len(), 7);
    let saved = s.doc.to_json();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), saved);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert_eq!(reopened.model.state().bodies.len(), 7);
    run(&mut reopened, "parameters.change", json!({"name": "span", "expression": "45 mm"}));
    // 45 mm at 15 mm: copies at 0, 15, 30 and 45, which the spacing divides, so none more.
    assert_eq!(reopened.model.state().bodies.len(), 4);
}

/// A distance type, extent or Copy at the end the pattern can't use is refused with its own
/// message and leaves the design as it was: no extent, a zero or negative extent or spacing, a
/// spacing longer than the extent, more copies than a pattern makes.
#[test]
fn rectangular_pattern_refuses_unusable_extents_atomically() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let cases = [
        (json!({"distance1": "pitch", "count1": 3, "spacing1": 10}), "`distance1` must be count_spacing, count_extent, spacing_extent or count_list"),
        (json!({"distance1": 2, "count1": 3, "spacing1": 10}), "`distance1` must be"),
        (json!({"distance1": "spacing_extent", "spacing1": 10, "extent1": 80, "copy_at_end1": "yes"}), "`copy_at_end1` must be true or false"),
        (json!({"distance1": "count_extent", "count1": 3}), "`extent1`"),
        (json!({"distance1": "spacing_extent", "extent1": 80}), "`spacing1`"),
        (json!({"distance1": "count_extent", "count1": 3, "extent1": 0}), "extent must be greater than zero"),
        (json!({"distance1": "count_extent", "count1": 3, "extent1": -20}), "extent must be greater than zero"),
        (json!({"distance1": "count_extent", "count1": 0, "extent1": 20}), "count"),
        (json!({"distance1": "spacing_extent", "spacing1": 0, "extent1": 80}), "spacing must be greater than zero"),
        (json!({"distance1": "spacing_extent", "spacing1": -15, "extent1": 80}), "spacing must be greater than zero"),
        (json!({"distance1": "spacing_extent", "spacing1": 90, "extent1": 80}), "is longer than its extent"),
        (json!({"distance1": "spacing_extent", "spacing1": 50, "extent1": 80, "alignment": "center"}), "is longer than half its extent"),
        (json!({"distance1": "spacing_extent", "spacing1": 0.001, "extent1": 100000}), "pattern count must be"),
        (json!({"distance1": "spacing_extent", "spacing1": 1e-300, "extent1": 1e8}), "pattern count must be"),
        (json!({"distance1": "count_extent", "count1": 3, "extent1": "5 deg"}), "extent1"),
        (json!({"count1": 2, "spacing1": 10, "dir2": [0, 1, 0], "distance2": "spacing_extent", "spacing2": 30, "extent2": 20}), "direction 2"),
        (json!({"distance1": "count_list", "count1": 3}), "`spacing_list1` must list at least one spacing"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": []}), "`spacing_list1` must list at least one spacing"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": 10}), "`spacing_list1` must be a list of spacings"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": [10, true]}), "`spacing_list1` must be a list of spacings"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": [10, " "]}), "`spacing_list1` must be a list of spacings"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": [10, "5 deg"]}), "spacing_list1 spacing 2"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": [10, 0]}), "spacing 2 must be greater than zero"),
        (json!({"distance1": "count_list", "count1": 3, "spacing_list1": [-10]}), "spacing 1 must be greater than zero"),
        (json!({"distance1": "count_list", "spacing_list1": [10]}), "`count1`"),
        (json!({"distance1": "count_list", "count1": 0, "spacing_list1": [10]}), "count"),
        (
            json!({"count1": 2, "spacing1": 10, "dir2": [0, 1, 0], "distance2": "count_list", "count2": 3}),
            "`spacing_list2` must list at least one spacing",
        ),
    ];
    for (mut p, message) in cases {
        p["bodies"] = json!(["Cube"]);
        p["dir1"] = json!([1, 0, 0]);
        let error = s.execute("solid.pattern.rectangular", &p).err().map(|e| e.to_string());
        assert!(error.as_deref().is_some_and(|e| e.contains(message)), "{p}: {error:?}");
        assert_eq!(s.doc.to_json(), before, "{p}");
        assert_eq!(s.undo.len(), undo, "{p}");
    }
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
    assert!(r.get("feature").and_then(|v| v.as_u64()).is_some(), "component pattern records a Pattern feature: {r}");
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["body_count"], 3, "{m}");
    let zs: Vec<f64> = m["bodies"].as_array().unwrap().iter().filter_map(|b| b["bbox"]["min"][2].as_f64()).collect();
    for z in [0.0, 20.0, 40.0] {
        assert!(zs.iter().any(|x| (x - z).abs() < 1e-6), "{zs:?}");
    }
    assert!(s.execute("solid.pattern.circular", &json!({"components": ["Nope"], "axis": "Z", "count": 2})).is_err());
}

/// #12116: a Pattern of a component keeps its count expression, so changing the parameter
/// adds or removes placements the way a body or feature pattern does. Survivors keep their ids.
#[test]
fn component_pattern_follows_its_count_parameter() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "2", "unit": ""}));
    run(&mut s, "component.create", json!({"name": "Unit"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = run(&mut s, "solid.pattern.rectangular", json!({"components": ["Unit"], "dir1": [1, 0, 0], "count1": "n", "spacing1": 30}));
    assert_eq!(r["occurrences"].as_array().map(Vec::len), Some(1), "{r}");
    let first_copy = r["occurrences"][0].as_u64().expect("copy id");
    let names = |s: &Session| -> Vec<String> { s.doc.occurrences.iter().filter(|o| o.component != 0).map(|o| o.name.clone()).collect() };
    assert_eq!(names(&s), ["Unit:1", "Unit:2"], "{:?}", names(&s));

    let change = run(&mut s, "parameters.change", json!({"name": "n", "expression": "4"}));
    assert!(change["recomputed"].as_u64().unwrap_or(0) >= 1, "pattern must rebuild: {change}");
    assert_eq!(names(&s), ["Unit:1", "Unit:2", "Unit:3", "Unit:4"], "{:?}", names(&s));
    assert!(s.doc.occurrences.iter().any(|o| o.id == first_copy), "survivor keeps its id");
    // The box is centred on each placement: centres at 0, 30, 60, 90 (bbox min is 5 mm left).
    let xs: Vec<f64> = run(&mut s, "inspect.measure", json!({}))["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| {
            let lo = b["bbox"]["min"][0].as_f64()?;
            let hi = b["bbox"]["max"][0].as_f64()?;
            Some((lo + hi) / 2.0)
        })
        .collect();
    for x in [0.0_f64, 30.0, 60.0, 90.0] {
        assert!(xs.iter().any(|v| (v - x).abs() < 1e-6), "want centre x={x} in {xs:?}");
    }

    let shrink = run(&mut s, "parameters.change", json!({"name": "n", "expression": "2"}));
    assert!(shrink["recomputed"].as_u64().unwrap_or(0) >= 1, "{shrink}");
    assert_eq!(names(&s), ["Unit:1", "Unit:2"], "{:?}", names(&s));
    assert!(s.doc.occurrences.iter().any(|o| o.id == first_copy), "survivor keeps its id after shrink");

    run(&mut s, "parameters.change", json!({"name": "n", "expression": "4"}));
    assert_eq!(names(&s).len(), 4, "{:?}", names(&s));
    assert!(s.doc.occurrences.iter().any(|o| o.id == first_copy), "survivor keeps its id after grow again");
}

#[test]
fn inactive_component_patterns_keep_their_occurrences_and_joints() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "name": "Base"}));
    run(&mut s, "component.create", json!({"name": "Unit"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let pattern_params = json!({"components": ["Unit"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30});
    let pattern = run(&mut s, "solid.pattern.rectangular", pattern_params.clone());
    let feature = pattern["feature"].as_u64().expect("pattern feature");
    let copy = pattern["occurrences"][0].as_u64().expect("copy");
    let made_joint = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": 0, "b": copy}));
    let joint_id = made_joint["joint"].as_u64().expect("joint");
    let joints = s.doc.assembly.joints.clone();
    let marker = s.doc.feature_index(feature).expect("pattern position");
    let shown = |s: &mut Session, expected| {
        assert_eq!(s.occurrence_placement(&[copy]).is_ok(), expected);
        let children = s.occurrence_children(&[]).expect("active root children");
        assert_eq!(children.len(), if expected { 2 } else { 1 });
        assert_eq!(children.iter().any(|child| child.occurrence == copy), expected);
        let measured = run(s, "inspect.measure", json!({}));
        assert_eq!(measured["body_count"], if expected { 3 } else { 2 });
        let mass = s.mass_properties().expect("active assembly mass");
        assert_eq!(mass.occurrences.iter().any(|row| row.occurrence == copy), expected);
        let solved = run(s, "joint.solve", json!({}));
        assert_eq!(solved["errors"], json!([]));
        let dof = solved["dof"].as_array().expect("degrees of freedom");
        assert_eq!(dof.iter().any(|entry| entry["occurrence"] == copy), expected);
    };

    // A saved rollback still contains, but does not place or solve, the Pattern's copies.
    s.visibility.shown_component_origins.push(vec![copy]);
    run(&mut s, "timeline.roll_to", json!({"position": marker}));
    shown(&mut s, false);
    assert_eq!(s.doc.assembly.joints, joints);
    assert_eq!(s.visibility.shown_component_origins, vec![vec![copy]]);
    let mut s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).expect("rolled-back design"));
    shown(&mut s, false);
    run(&mut s, "timeline.roll_to", json!({"position": "end"}));
    shown(&mut s, true);
    assert_eq!(s.doc.assembly.joints, joints);

    // Edit Feature rolls the scratch design behind the later pattern while building the edit.
    run(&mut s, "timeline.redefine", json!({"feature": "Base", "command": "solid.box", "params": {"length":9, "width":8, "height":8}}));
    shown(&mut s, true);
    assert_eq!(s.doc.assembly.joints, joints);

    let mut larger = pattern_params.clone();
    larger["count1"] = json!(3);
    run(&mut s, "timeline.redefine", json!({"feature": feature, "command": "solid.pattern.rectangular", "params": larger}));
    assert!(s.occurrence_placement(&[copy]).is_ok());
    assert_eq!(s.doc.assembly.joints, joints);
    assert_eq!(run(&mut s, "inspect.measure", json!({}))["body_count"], 4);

    let extra_copy = s.doc.active_occurrences().last().expect("new copy").id;
    assert_ne!(extra_copy, copy);
    let extra = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": 0, "b": extra_copy}));
    assert!(extra["joint"].is_number());
    run(&mut s, "timeline.redefine", json!({"feature": feature, "command": "solid.pattern.rectangular", "params": pattern_params}));
    assert!(!s.doc.occurrences.iter().any(|o| o.id == extra_copy));
    assert_eq!(s.doc.assembly.joints, joints);
    shown(&mut s, true);

    run(&mut s, "timeline.suppress", json!({"feature": feature, "suppressed": true}));
    shown(&mut s, false);
    assert_eq!(s.doc.assembly.joints, joints);
    run(&mut s, "timeline.suppress", json!({"feature": feature, "suppressed": false}));
    assert!(s.occurrence_placement(&[copy]).is_ok());
    assert_eq!(s.doc.assembly.joints, joints);
    run(&mut s, "timeline.suppress", json!({"feature": feature, "suppressed": true}));

    // Deleting the inactive feature really removes its placements and their references.
    run(&mut s, "timeline.delete", json!({"feature": feature}));
    assert!(!s.doc.occurrences.iter().any(|o| o.id == copy));
    assert!(!s.doc.assembly.joints.iter().any(|joint| joint.id == joint_id));
    shown(&mut s, false);
}

#[test]
fn an_inactive_pattern_does_not_drive_an_active_joints_reported_value() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Unit", "grounded": false}));
    let seed = s.doc.occurrences.first().expect("seed occurrence").id;
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let pattern = run(&mut s, "solid.pattern.rectangular", json!({"components": ["Unit"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30}));
    let copy = pattern["occurrences"][0].as_u64().expect("copy");
    let driver = run(&mut s, "joint.as_built", json!({"type": "slider", "a": 0, "b": copy}))["joint"].as_u64().expect("driver");
    let follower = run(&mut s, "joint.as_built", json!({"type": "slider", "a": 0, "b": seed}))["joint"].as_u64().expect("follower");
    run(&mut s, "joint.motion_link", json!({"a": driver, "b": follower, "ratio": 1, "offset": 7}));
    let check = |s: &mut Session, expected: f64| {
        let listed = run(s, "joint.list", json!({}));
        let row = listed["joints"].as_array().unwrap().iter().find(|row| row["id"] == follower).expect("listed follower");
        assert_eq!(row["values"][0]["value"], expected);
        let inspected = run(s, "joint.inspect", json!({"joint": follower}));
        assert_eq!(inspected["joints"][0]["values"][0]["value"], expected);
    };
    check(&mut s, 7.0);
    run(&mut s, "timeline.suppress", json!({"feature": pattern["feature"], "suppressed": true}));
    check(&mut s, 0.0);
    let target = solidcraft_doc::joints::Reach {
        occurrence: seed,
        point: solidcore_record::Vec3::ZERO,
        to: solidcore_record::Vec3::new(0.0, 0.0, 3.0),
        axis: None,
        joints: None,
    };
    let reached = solidcraft_doc::joints::reach(&s.doc, &s.model.state(), &target).unwrap();
    assert_eq!(reached.values.keys().copied().collect::<Vec<_>>(), [follower]);
    run(&mut s, "timeline.suppress", json!({"feature": pattern["feature"], "suppressed": false}));
    check(&mut s, 7.0);

    // Replacing the dormant link preserves the active slider's solved position, even though
    // the old link would have driven it to 7 mm if its pattern were active.
    run(&mut s, "timeline.suppress", json!({"feature": pattern["feature"], "suppressed": true}));
    run(&mut s, "component.create", json!({"name": "Rotor", "activate": false, "grounded": false}));
    let spin = run(&mut s, "joint.as_built", json!({"type": "revolute", "a": 0, "b": "Rotor"}))["joint"].as_u64().expect("revolute driver");
    let before = s.occurrence_placement(&[seed]).unwrap().world;
    run(&mut s, "joint.screw", json!({"a": spin, "b": follower, "link": 0, "pitch": 2}));
    check(&mut s, 0.0);
    assert_eq!(s.occurrence_placement(&[seed]).unwrap().world, before);
}

/// Component-pattern copies inherit their seed occurrence's constraint and DOF rather than
/// reporting `dof: 6` across rectangular, circular, and path patterns (#22213).
#[test]
fn component_pattern_copies_inherit_seed_dof_and_anchor_downstream_joints() {
    let dof_map = |s: &mut Session| -> std::collections::BTreeMap<u64, u64> {
        run(s, "joint.list", json!({}))["dof"]
            .as_array()
            .expect("dof array")
            .iter()
            .map(|e| (e["occurrence"].as_u64().expect("occ"), e["dof"].as_u64().expect("dof")))
            .collect()
    };

    // 1. Grounded seed + rectangular, circular, and path patterns: all copies report dof 0.
    for kind in ["rectangular", "circular", "path"] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Pin", "activate": true}));
        run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 10}));
        run(&mut s, "component.activate", json!({"component": "root"}));
        let seed = s.doc.occurrences[0].id;
        let pattern = match kind {
            "rectangular" => run(&mut s, "solid.pattern.rectangular", json!({"components": ["Pin"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 15})),
            "circular" => run(&mut s, "solid.pattern.circular", json!({"components": ["Pin"], "axis": "Z", "count": 3})),
            _ => {
                let sk = run(&mut s, "sketch.create", json!({"plane": "XY"}))["sketch"].as_u64().expect("sketch");
                run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]]}));
                run(&mut s, "sketch.finish", json!({}));
                run(&mut s, "solid.pattern.path", json!({"components": ["Pin"], "path_sketch": sk, "path": ["l1"], "count": 3, "distance": 30}))
            }
        };
        let copies: Vec<u64> = pattern["occurrences"].as_array().expect("copies").iter().map(|v| v.as_u64().expect("copy id")).collect();
        assert_eq!(copies.len(), 2, "{kind}");
        let dofs = dof_map(&mut s);
        assert_eq!(dofs.get(&seed), Some(&0), "{kind} seed");
        for copy in &copies {
            assert_eq!(dofs.get(copy), Some(&0), "{kind} copy {copy}");
        }
        let free = run(&mut s, "joint.list", json!({"dof_free_only": true}));
        assert_eq!(free["dof"].as_array().expect("free dof").len(), 0, "{kind} dof_free_only");
    }

    // 2. Seed on a slider joint (dof 1): copies inherit dof 1, and a part joined to a copy
    //    is anchored (not a floating chain).
    let mut s = Session::default();
    let base = run(&mut s, "component.create", json!({"name": "Base", "activate": true}))["occurrence"].as_u64().expect("base");
    run(&mut s, "solid.box", json!({"length": 50, "width": 20, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let pin = run(&mut s, "component.create", json!({"name": "Pin", "activate": true}))["occurrence"].as_u64().expect("pin");
    run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "joint.as_built", json!({"a": base, "b": pin, "type": "slider", "at": {"occurrence": base, "point": [0, 0, 10], "z": [0, 0, 1]}}));
    let pattern = run(&mut s, "solid.pattern.rectangular", json!({"components": ["Pin"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 15}));
    let copy1 = pattern["occurrences"][0].as_u64().expect("copy1");
    let copy2 = pattern["occurrences"][1].as_u64().expect("copy2");
    let cap = run(&mut s, "component.create", json!({"name": "Cap", "activate": true}))["occurrence"].as_u64().expect("cap");
    run(&mut s, "solid.box", json!({"length": 6, "width": 6, "height": 4}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "joint.as_built", json!({"a": copy1, "b": cap, "type": "rigid", "at": {"occurrence": copy1, "point": [15, 0, 10], "z": [0, 0, 1]}}));
    let listed = run(&mut s, "joint.list", json!({}));
    assert_eq!(listed["floating"], json!([]), "joint on pattern copy should not float");
    assert_eq!(listed["conflicts"], json!([]));
    assert_eq!(listed["errors"], json!([]));
    let dofs = dof_map(&mut s);
    assert_eq!(dofs.get(&base), Some(&0));
    assert_eq!(dofs.get(&pin), Some(&1));
    assert_eq!(dofs.get(&copy1), Some(&1));
    assert_eq!(dofs.get(&copy2), Some(&1));
    assert_eq!(dofs.get(&cap), Some(&1));

    // 3. Unconstrained seed (grounded: false, no joints): seed and copies stay dof 6.
    let mut s = Session::default();
    let free_seed =
        run(&mut s, "component.create", json!({"name": "Loose", "activate": true, "grounded": false}))["occurrence"].as_u64().expect("loose");
    run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let pattern = run(&mut s, "solid.pattern.rectangular", json!({"components": ["Loose"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 15}));
    let free_copy = pattern["occurrences"][0].as_u64().expect("free copy");
    let dofs = dof_map(&mut s);
    assert_eq!(dofs.get(&free_seed), Some(&6));
    assert_eq!(dofs.get(&free_copy), Some(&6));

    // 4. Loop closure moving a sub-assembly refreshes its pattern copies, downstream joints,
    //    and downstream DOFs.
    let mut s = Session::default();
    let base = run(&mut s, "component.create", json!({"name": "Base", "activate": true}))["occurrence"].as_u64().expect("base");
    run(&mut s, "solid.box", json!({"length": 50, "width": 20, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let sub_res = run(&mut s, "component.create", json!({"name": "Sub", "activate": true, "grounded": false}));
    let sub_comp = sub_res["component"].as_u64().expect("sub comp");
    let sub_occ = sub_res["occurrence"].as_u64().expect("sub occ");
    run(&mut s, "solid.box", json!({"length": 30, "width": 10, "height": 10}));
    let pin_occ = run(&mut s, "component.create", json!({"name": "Pin", "parent": sub_comp, "activate": true, "grounded": false}))["occurrence"]
        .as_u64()
        .expect("pin occ");
    run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 10}));
    run(&mut s, "component.activate", json!({"component": sub_comp}));
    run(
        &mut s,
        "joint.as_built",
        json!({"a": sub_occ, "b": pin_occ, "type": "rigid", "at": {"occurrence": sub_occ, "point": [0, 0, 10], "z": [0, 0, 1]}}),
    );
    let pattern = run(&mut s, "solid.pattern.rectangular", json!({"components": ["Pin"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 15}));
    let copy_occ = pattern["occurrences"][0].as_u64().expect("copy occ");
    run(&mut s, "component.activate", json!({"component": "root"}));
    let cap_occ =
        run(&mut s, "component.create", json!({"name": "Cap", "activate": true, "grounded": false}))["occurrence"].as_u64().expect("cap occ");
    run(&mut s, "solid.box", json!({"length": 6, "width": 6, "height": 4}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(
        &mut s,
        "joint.as_built",
        json!({"a": copy_occ, "b": cap_occ, "type": "rigid", "at": {"occurrence": copy_occ, "point": [15, 0, 10], "z": [0, 0, 1]}}),
    );
    run(
        &mut s,
        "joint.as_built",
        json!({"a": base, "b": sub_occ, "type": "slider", "at": {"occurrence": base, "point": [0, 0, 10], "z": [0, 0, 1]}}),
    );
    // Closing rigid joint with offset 10 forces the slider from 0 to 10 mm and reduces DOF to 0.
    let closing = run(
        &mut s,
        "joint.as_built",
        json!({"a": base, "b": sub_occ, "type": "rigid", "at": {"occurrence": base, "point": [0, 0, 10], "z": [0, 0, 1]}}),
    )["joint"]
        .as_u64()
        .expect("closing joint");
    run(&mut s, "joint.edit", json!({"joint": closing, "offset": 10}));
    let listed = run(&mut s, "joint.list", json!({}));
    assert_eq!(listed["conflicts"], json!([]));
    assert_eq!(listed["errors"], json!([]));
    let dofs = dof_map(&mut s);
    assert_eq!(dofs.get(&sub_occ), Some(&0));
    assert_eq!(dofs.get(&pin_occ), Some(&0));
    assert_eq!(dofs.get(&copy_occ), Some(&0));
    assert_eq!(dofs.get(&cap_occ), Some(&0));
    let sub_z = s.occurrence_placement(&[sub_occ]).expect("sub placement").world[3][2];
    let copy_z = s.occurrence_placement(&[sub_occ, copy_occ]).expect("copy placement").world[3][2];
    let cap_z = s.occurrence_placement(&[cap_occ]).expect("cap placement").world[3][2];
    assert!((sub_z - 10.0).abs() < 1e-6, "loop closure should move sub: {sub_z}");
    assert!((copy_z - 10.0).abs() < 1e-6, "copy should follow loop-closed parent: {copy_z}");
    assert!((cap_z - 10.0).abs() < 1e-6, "downstream cap should follow refreshed copy: {cap_z}");

    // 5. A pattern copy contained inside a joint-dependent descendant (`cap_occ`) also refreshes
    //    after loop closure, and a second descendant with its own local parallel-slider loop keeps
    //    its independently measured DOF (1) instead of having the seed's -1 delta subtracted again.
    let cap_comp = s.doc.occurrences.iter().find(|o| o.id == cap_occ).expect("cap occ").component;
    let nested_pin =
        run(&mut s, "component.create", json!({"name": "NestedPin", "parent": cap_comp, "activate": true, "grounded": false}))["occurrence"]
            .as_u64()
            .expect("nested pin");
    run(&mut s, "solid.cylinder", json!({"radius": 1, "height": 5}));
    run(&mut s, "component.activate", json!({"component": cap_comp}));
    run(
        &mut s,
        "joint.as_built",
        json!({"a": cap_occ, "b": nested_pin, "type": "rigid", "at": {"occurrence": cap_occ, "point": [0, 0, 10], "z": [0, 0, 1]}}),
    );
    let nested_pattern =
        run(&mut s, "solid.pattern.rectangular", json!({"components": ["NestedPin"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 8}));
    let nested_copy = nested_pattern["occurrences"][0].as_u64().expect("nested copy");
    run(&mut s, "component.activate", json!({"component": "root"}));
    let slider_cap = run(&mut s, "component.create", json!({"name": "SliderCap", "activate": true, "grounded": false}))["occurrence"]
        .as_u64()
        .expect("slider cap");
    run(&mut s, "solid.box", json!({"length": 6, "width": 6, "height": 4}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(
        &mut s,
        "joint.as_built",
        json!({"a": copy_occ, "b": slider_cap, "type": "slider", "at": {"occurrence": copy_occ, "point": [15, 0, 10], "z": [0, 0, 1]}}),
    );
    run(
        &mut s,
        "joint.as_built",
        json!({"a": copy_occ, "b": slider_cap, "type": "slider", "at": {"occurrence": copy_occ, "point": [15, 3, 10], "z": [0, 0, 1]}}),
    );
    // Re-trigger loop closure with a new offset (15 mm) and verify nested_copy & slider_cap.
    run(&mut s, "joint.edit", json!({"joint": closing, "offset": 15}));
    let dofs = dof_map(&mut s);
    assert_eq!(dofs.get(&nested_pin), Some(&0));
    assert_eq!(dofs.get(&nested_copy), Some(&0));
    assert_eq!(dofs.get(&slider_cap), Some(&1), "independently measured local loop keeps 1 DOF");
    let nested_z = s.occurrence_placement(&[cap_occ, nested_copy]).expect("nested copy placement").world[3][2];
    assert!((nested_z - 15.0).abs() < 1e-6, "nested copy inside downstream cap should follow loop closure: {nested_z}");
}

/// A component pattern's copies placed again on a rebuild read the spacing as the first copies
/// were placed (#8523): in an inch design `1 + 0` is 1 in, before and after the count changes.
#[test]
fn component_pattern_copies_keep_the_design_unit_when_the_count_changes() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "2", "unit": ""}));
    run(&mut s, "component.create", json!({"name": "Unit"}));
    run(&mut s, "solid.box", json!({"length": "10 mm", "width": "10 mm", "height": "10 mm", "operation": "new", "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.pattern.rectangular", json!({"components": ["Unit"], "dir1": [1, 0, 0], "count1": "n", "spacing1": "1 + 0"}));
    let xs = |s: &Session| -> Vec<f64> {
        let mut xs: Vec<f64> = s.doc.occurrences.iter().filter(|o| o.component != 0).map(|o| o.transform[3][0]).collect();
        xs.sort_by(f64::total_cmp);
        xs
    };
    let placed = xs(&s);
    assert!(placed.len() == 2 && (placed[1] - placed[0] - 25.4).abs() < 1e-9, "{placed:?}");
    run(&mut s, "parameters.change", json!({"name": "n", "expression": "3"}));
    let placed = xs(&s);
    assert_eq!(placed.len(), 3, "{placed:?}");
    for (i, x) in placed.iter().enumerate() {
        assert!((x - placed[0] - 25.4 * i as f64).abs() < 1e-9, "{placed:?}");
    }
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

/// The centres of mass of the bodies other than `original`, in the order they were made.
fn copy_centres(s: &mut Session, original: &str) -> Vec<[f64; 3]> {
    let m = run(s, "inspect.measure", json!({}));
    let centre = |b: &Value| [0, 1, 2].map(|i| b["center_of_mass"][i].as_f64().unwrap_or(f64::NAN));
    m["bodies"].as_array().into_iter().flatten().filter(|b| b["name"] != original).map(centre).collect()
}

fn assert_centres(got: &[[f64; 3]], want: &[[f64; 3]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(g.iter().zip(w).all(|(a, b)| (a - b).abs() < 1e-3), "{what}: a copy is at {g:?}, want {w:?} (all: {got:?})");
    }
}

/// A ball on a half circle (radius 20 about the origin, from (20, 0) through (0, 20) to
/// (−20, 0); 20π mm long), `out` mm outward from the circle at 45°.
fn ball_on_half_circle(out: f64) -> Session {
    let mut s = Session::default();
    let at = 20.0 + out;
    run(
        &mut s,
        "solid.sphere",
        json!({"radius": 1, "center": [format!("{at} * cos(45 deg)"), format!("{at} * sin(45 deg)"), 0], "body_name": "Ball"}),
    );
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [20, 0], "sweep": 180, "id": "a1"}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

/// The point at `deg` on the circle of radius `r` about the origin.
fn on_circle(r: f64, deg: f64) -> [f64; 3] {
    let a = deg.to_radians();
    [r * a.cos(), r * a.sin(), 0.0]
}

/// #6054: the start point says where along the path the original sits (a distance, or a fraction
/// of the path's length), and the copies are measured on from there. The ball sits on the half
/// circle at 45° (5π mm along it); copies 5π apart go to 90° and 135°.
#[test]
fn path_pattern_start_point_measures_the_copies_from_the_original() {
    let pattern = |extra: Value| {
        let mut s = ball_on_half_circle(0.0);
        let mut p = json!({"bodies": ["Ball"], "path_sketch": "Arc", "path": ["a1"], "count": 3, "spacing": "5 * pi"});
        if let (Some(p), Value::Object(extra)) = (p.as_object_mut(), extra) {
            p.extend(extra);
        }
        run(&mut s, "solid.pattern.path", p);
        copy_centres(&mut s, "Ball")
    };
    let on_path = [on_circle(20.0, 90.0), on_circle(20.0, 135.0)];
    assert_centres(&pattern(json!({"start": "5 * pi"})), &on_path, "start as a distance");
    assert_centres(&pattern(json!({"start_fraction": 0.25})), &on_path, "start as a fraction");
    // Without a start point the copies are measured from the path's start, (20, 0): the first
    // moves by path(5π) − path(0) = (20 cos 45° − 20, 20 sin 45°), off the circle.
    let ball = on_circle(20.0, 45.0);
    let moved = |deg: f64| {
        let (a, b) = (on_circle(20.0, deg), on_circle(20.0, 0.0));
        [ball[0] + a[0] - b[0], ball[1] + a[1] - b[1], 0.0]
    };
    assert_centres(&pattern(json!({})), &[moved(45.0), moved(90.0)], "no start point");
    // Flipped, the path runs from (−20, 0): three quarters of it from there is the 45° point
    // again, and the copies go on toward (20, 0).
    let flipped = pattern(json!({"start_fraction": 0.75, "flip": true, "count": 2}));
    assert_centres(&flipped, &[on_circle(20.0, 0.0)], "flipped");

    // The start point is a parameter: the copies follow it.
    let mut s = ball_on_half_circle(0.0);
    run(
        &mut s,
        "solid.pattern.path",
        json!({"bodies": ["Ball"], "path_sketch": "Arc", "path": ["a1"], "count": 2, "spacing": "5 * pi", "start": "5 * pi"}),
    );
    // Its parameters: the count, the spacing and the start point.
    let start = s.doc.features.last().and_then(|f| f.param_names.get(2).cloned()).unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": start, "expression": "10 * pi"}));
    // The ball now stands for the 90° point, so its copy moves by path(15π) − path(10π).
    let (a, b) = (on_circle(20.0, 135.0), on_circle(20.0, 90.0));
    assert_centres(&copy_centres(&mut s, "Ball"), &[[ball[0] + a[0] - b[0], ball[1] + a[1] - b[1], 0.0]], "start edited");

    // A start point off the path, copies running past either end, and both forms at once are
    // refused.
    let refused = |extra: Value, want: &str| {
        let mut s = ball_on_half_circle(0.0);
        let mut p = json!({"bodies": ["Ball"], "path_sketch": "Arc", "path": ["a1"], "count": 3, "spacing": "5 * pi"});
        if let (Some(p), Value::Object(extra)) = (p.as_object_mut(), extra) {
            p.extend(extra);
        }
        let err = s.execute("solid.pattern.path", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err.contains(want), "{p}: {err}");
    };
    refused(json!({"start": "21 * pi"}), "start point must lie on the path");
    refused(json!({"start": -1}), "start point must lie on the path");
    refused(json!({"start_fraction": 1.5}), "0…1 of the path's length");
    refused(json!({"start": "15 * pi"}), "past the end of the path");
    refused(json!({"start": "5 * pi", "spacing": "-5 * pi"}), "past the start of the path");
    refused(json!({"start": 5, "start_fraction": 0.5}), "not both");
    // A start point that isn't a number or an expression is refused, not read as the path's start.
    refused(json!({"start": [10]}), "`start` must be a number or an expression");
    refused(json!({"start": ""}), "`start` must be a number or an expression");
    refused(json!({"start_fraction": true}), "`start_fraction` must be a number or an expression");
    refused(json!({"start_fraction": {"value": 0.5}}), "`start_fraction` must be a number or an expression");
    // Backwards from a start point, the copies may run toward the path's start.
    assert_centres(
        &pattern(json!({"start": "10 * pi", "spacing": "-5 * pi"})),
        &[
            {
                let (a, b) = (on_circle(20.0, 45.0), on_circle(20.0, 90.0));
                [ball[0] + a[0] - b[0], ball[1] + a[1] - b[1], 0.0]
            },
            {
                let (a, b) = (on_circle(20.0, 0.0), on_circle(20.0, 90.0));
                [ball[0] + a[0] - b[0], ball[1] + a[1] - b[1], 0.0]
            },
        ],
        "backwards",
    );
}

/// #6054: with Path orientation each copy keeps the original's place relative to the path.
/// A ball 5 mm outward of the half circle at 45° stays 5 mm outward at 90° and 135°; with
/// Identical orientation it keeps its offset (5 mm along 45°) instead.
#[test]
fn path_pattern_orientation_keeps_the_original_relation_to_an_arc() {
    let pattern = |orientation: &str| {
        let mut s = ball_on_half_circle(5.0);
        run(
            &mut s,
            "solid.pattern.path",
            json!({"bodies": ["Ball"], "path_sketch": "Arc", "path": ["a1"], "count": 3, "spacing": "5 * pi", "start": "5 * pi", "orientation": orientation}),
        );
        copy_centres(&mut s, "Ball")
    };
    assert_centres(&pattern("path"), &[on_circle(25.0, 90.0), on_circle(25.0, 135.0)], "path");
    let off = on_circle(5.0, 45.0);
    let shifted = |deg: f64| {
        let p = on_circle(20.0, deg);
        [p[0] + off[0], p[1] + off[1], 0.0]
    };
    assert_centres(&pattern("identical"), &[shifted(90.0), shifted(135.0)], "identical");
}

/// #6054: on a path that leaves its sketch's plane the copies turn with the path's tangent and
/// keep the original's relation to it. The path runs 10 mm along X and then 10 mm up Z (a 3D
/// sketch on XY); a ball 5 mm above its start (square to the path, in the plane of its bend)
/// sits, 5 mm up the second leg, 5 mm back from it along −X: the path turned a quarter about
/// −Y, and so did the ball. Turning only about the sketch's normal did not turn it at all: it
/// ended up 5 mm further up the second leg, on the path.
#[test]
fn path_pattern_orientation_follows_a_3d_path() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 1, "center": [0, 0, 5], "body_name": "Ball"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let legs = run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [10, 0, 0], [10, 0, 10]]}))["curves"].clone();
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.pattern.path",
        json!({"bodies": ["Ball"], "path_sketch": "Path", "path": legs, "count": 2, "spacing": 15, "orientation": "path"}),
    );
    assert_centres(&copy_centres(&mut s, "Ball"), &[[5.0, 0.0, 5.0]], "bent 3D path");

    // Up a steep helix (radius 20, rising 100 mm a turn, drawn as a 3D spline through points on
    // it), a ball 5 mm outward of its start. A rotation-minimising frame never rolls about the
    // path, so a quarter turn on the copy is still 5 mm from the path, square to it, and turned
    // from the outward direction about the path by the helix's torsion times the length run:
    // a·h / √(r² + h²) at angle a. The turn about the sketch's normal alone left it about 2 mm
    // along the path, 4.5 mm from it.
    let (mut s, _) = ball_on_helix();
    let (r, h) = HELIX;
    let n = r.hypot(h);
    let c = copy_centres(&mut s, "Ball").first().copied().unwrap_or([f64::NAN; 3]);
    // The nearest point of the helix, and the copy's offset from it.
    let at = |a: f64| [r * a.cos(), r * a.sin(), h * a];
    let gap = |a: f64| (0..3).map(|k| (c[k] - at(a)[k]).powi(2)).sum::<f64>();
    let a = (0..=100_000).map(|i| f64::from(i) * PI / 100_000.0).min_by(|x, y| gap(*x).total_cmp(&gap(*y))).unwrap_or(f64::NAN);
    let off = [c[0] - at(a)[0], c[1] - at(a)[1], c[2] - at(a)[2]];
    let (outward, tangent) = ([a.cos(), a.sin(), 0.0], [-r * a.sin() / n, r * a.cos() / n, h / n]);
    let side = [
        tangent[1] * outward[2] - tangent[2] * outward[1],
        tangent[2] * outward[0] - tangent[0] * outward[2],
        tangent[0] * outward[1] - tangent[1] * outward[0],
    ];
    let dot = |u: [f64; 3]| off[0] * u[0] + off[1] * u[1] + off[2] * u[2];
    assert!((gap(a).sqrt() - 5.0).abs() < 0.05, "the copy is {} mm from the path: {c:?}", gap(a).sqrt());
    let turned = dot(side).atan2(dot(outward));
    assert!((turned + a * h / n).abs() < 0.01, "the copy turned {turned} rad about the path at {a} rad, want {}: {c:?}", -a * h / n);

    // A design saved before the frame keeps what it built: its copy is turned about the
    // sketch's normal only, so the ball's 5 mm outward offset stays level. It sits 5 mm from the
    // path's point at a quarter turn, (0, r, h·π/2), at that point's height, where the frame
    // tilts it with the path.
    let mut doc = (*s.doc).clone();
    for f in &mut doc.features {
        if let solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Path { follow_frame, .. }, .. } = &mut f.kind {
            assert!(*follow_frame, "a pattern made now follows the frame");
            *follow_frame = false;
        }
    }
    s.doc = std::sync::Arc::new(doc);
    s.refresh();
    let old = copy_centres(&mut s, "Ball").first().copied().unwrap_or([f64::NAN; 3]);
    let level = old[0].hypot(old[1] - r);
    assert!((old[2] - h * 0.5 * PI).abs() < 0.05 && (level - 5.0).abs() < 0.05, "the older design's copy is at {old:?}");
    assert!((c[2] - h * 0.5 * PI).abs() > 0.5, "the frame tilts the offset with the path: {c:?}");
}

/// The 3D helix of [`path_pattern_orientation_follows_a_3d_path`]: radius 20, rising 100 mm a
/// turn, as (radius, rise per radian).
const HELIX: (f64, f64) = (20.0, 50.0 / PI);

/// A ball 5 mm outward of the start of [`HELIX`] (half a turn, a 3D spline through points on
/// it), patterned a quarter turn along it with Path direction; with the command's parameters.
fn ball_on_helix() -> (Session, Value) {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"radius": 1, "center": [25, 0, 0], "body_name": "Ball"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Helix"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let (r, h) = HELIX;
    let points: Vec<[f64; 3]> = (0..=12).map(|i| f64::from(i) * PI / 12.0).map(|a| [r * a.cos(), r * a.sin(), h * a]).collect();
    let helix = run(&mut s, "sketch.spline3d", json!({"points": points}))["curves"].clone();
    run(&mut s, "sketch.finish", json!({}));
    let params =
        json!({"bodies": ["Ball"], "path_sketch": "Helix", "path": helix, "count": 2, "spacing": 0.5 * PI * r.hypot(h), "orientation": "path"});
    run(&mut s, "solid.pattern.path", params.clone());
    (s, params)
}

/// The path pattern's `follow_frame`, for the one Pattern on Path in `s`.
fn follows_frame(s: &Session) -> bool {
    s.doc.features.iter().any(|f| {
        matches!(&f.kind, solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Path { follow_frame: true, .. }, .. })
    })
}

/// Editing a Pattern on Path saved before the rotation-minimising frame does not move its
/// copies: OK in its dialog sends `timeline.redefine` with the same parameters, and the copies
/// stay where the older design built them. Choosing the orientation again is a new choice, and
/// then the copies follow the frame, as in a pattern made now.
#[test]
fn editing_an_older_path_pattern_keeps_its_copies_where_they_were() {
    let (mut s, params) = ball_on_helix();
    let id = s.doc.features.last().map(|f| f.id).unwrap_or_default();
    let framed = copy_centres(&mut s, "Ball");
    // As the design was saved before the frame: no `follow_frame`.
    let mut doc = (*s.doc).clone();
    for f in &mut doc.features {
        if let solidcraft_doc::FeatureKind::Pattern { pattern: solidcraft_doc::PatternKind::Path { follow_frame, .. }, .. } = &mut f.kind {
            *follow_frame = false;
        }
    }
    s.doc = std::sync::Arc::new(doc);
    s.refresh();
    let older = copy_centres(&mut s, "Ball");
    assert!(older.iter().zip(&framed).any(|(a, b)| (0..3).any(|k| (a[k] - b[k]).abs() > 0.5)), "{older:?} vs {framed:?}");

    let edit = |s: &mut Session, params: Value| {
        run(s, "timeline.redefine", json!({"feature": id, "command": "solid.pattern.path", "params": params}));
    };
    edit(&mut s, params.clone());
    assert!(!follows_frame(&s), "the edit kept the older design's orientation");
    assert_centres(&copy_centres(&mut s, "Ball"), &older, "the older design, edited");
    // Another option changed, here a start point given at the path's start, keeps it too.
    let mut started = params.clone();
    started["start_fraction"] = json!("0");
    edit(&mut s, started);
    assert!(!follows_frame(&s), "a start point keeps the older orientation");
    assert_centres(&copy_centres(&mut s, "Ball"), &older, "the older design, given a start point");

    // Orientation picked again: Identical, then Path direction, as a pattern made now.
    let mut identical = params.clone();
    identical["orientation"] = json!("identical");
    edit(&mut s, identical);
    edit(&mut s, params);
    assert!(follows_frame(&s), "a newly chosen orientation follows the frame");
    assert_centres(&copy_centres(&mut s, "Ball"), &framed, "orientation chosen again");
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

/// #14050: a through hole at a sketch point on a plane below the box drills up into it, and so
/// does every copy of it. Judged where the source hole already left an empty bore, the copies
/// drilled away from the box and cut almost nothing (test and analysis by Ehuntabi).
#[test]
fn patterns_of_a_sketch_point_hole_below_the_box_drill_every_copy() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 90, "width": 60, "height": 10, "center": [0, 0, 5]}));
    let v0 = volume(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "VP", "offset": -1}));
    run(&mut s, "sketch.point", json!({"point": [-20, -10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "VP", "points": ["p1"], "diameter": 6, "depth": 12, "name": "H1"}));
    let one = v0 - volume(&mut s);
    assert!((one - PI * 9.0 * 10.0).abs() < 1e-3 * one, "{one}");
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features": ["H1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20, "dir2": [0, 1, 0], "count2": 2, "spacing2": 20}),
    );
    let six = v0 - volume(&mut s);
    assert!((six - 6.0 * one).abs() < 1e-3 * one, "{six} vs 6 × {one}");
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

/// Whether a job four times the size took at most about twice linear: `pair` times the small job,
/// then the big one. Linear is 4×, the old quadratic costs 16× or more. The load of the other test
/// binaries can rise between the two runs (on a 16-core Mac under `cargo xtask test`, 32 circles
/// took 4.7 s and 128 took 42 s, though both together take 18 s alone), so a pair that misses is
/// measured again, up to three times. Load only ever slows a run, so each size keeps its fastest
/// time and the ratio is taken of those: a retry cannot pass a regression on a small run that a
/// spike happened to slow, once an earlier small run was fast. (A spike on the very first small
/// run can still hide one, as it could before the retries.)
fn scales_linearly(pair: impl FnMut() -> (std::time::Duration, std::time::Duration)) -> Result<(), String> {
    scales_within(8.0, pair)
}

/// `scales_linearly` with the allowed multiple of the small time given: at most `factor` times the
/// fastest small run plus 2 s.
fn scales_within(factor: f64, mut pair: impl FnMut() -> (std::time::Duration, std::time::Duration)) -> Result<(), String> {
    let (mut small, mut big) = (f64::INFINITY, f64::INFINITY);
    let mut runs = Vec::new();
    for _ in 0..3 {
        let (s, b) = pair();
        small = small.min(s.as_secs_f64());
        big = big.min(b.as_secs_f64());
        runs.push(format!("{s:?} then {b:?}"));
        if big < factor * small + 2.0 {
            return Ok(());
        }
    }
    Err(format!("{}; fastest {small:.2} s and {big:.2} s, limit {:.2} s", runs.join(", "), factor * small + 2.0))
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
/// linear since #261. Linear is 4×. A ratio, not a time: CI runs every test binary at once and is
/// many times slower than a machine running this alone.
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
/// that body, as before #2755, instead of looking for the target the join used up. The spoke
/// starts 1 mm inside the hub, so it shares volume with both (one tangent to the hub would only
/// touch it along a line, and the join would keep the hub a body of its own).
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
        let spoke = json!({"length": 53, "width": 3, "height": 2.5, "corner": [11, 0, -1.25], "operation": "join", "targets": ["rim", "hub"], "name": "spoke"});
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
    use solidcore_record::Vec3;
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
        let st = s.world_state().unwrap();
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results.iter().map(|r| &r.error).collect::<Vec<_>>());
        assert_eq!(st.bodies.len(), 1);
        let body = &st.bodies[0].body;
        for p in inside {
            assert_eq!(body.classify_point(*p).unwrap(), solidcraft_kernel::PointClass::Inside, "a rib at {p:?}");
            // Beside it, across its thickness: no material (clear of the boss too).
            let side = if p.y == 0.0 { Vec3::new(0.0, 3.0, 0.0) } else { Vec3::new(3.0, 0.0, 0.0) };
            assert_eq!(body.classify_point(*p + side).unwrap(), solidcraft_kernel::PointClass::Outside, "nothing beside the rib at {p:?}");
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
    use solidcore_record::Vec3;
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
        let st = s.world_state().unwrap();
        assert_eq!(st.bodies.len(), 1, "{what}");
        let body = &st.bodies[0].body;
        for [x, y] in &holes {
            assert_eq!(body.classify_point(Vec3::new(*x, *y, 10.0)).unwrap(), solidcraft_kernel::PointClass::Outside, "{what}: a hole at ({x}, {y})");
            assert_eq!(
                body.classify_point(Vec3::new(*x, *y + 2.5, 10.0)).unwrap(),
                solidcraft_kernel::PointClass::Inside,
                "{what}: material beside ({x}, {y})"
            );
        }
        for [x, y] in &material {
            assert_eq!(body.classify_point(Vec3::new(*x, *y, 10.0)).unwrap(), solidcraft_kernel::PointClass::Inside, "{what}: no hole at ({x}, {y})");
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

/// The lid of #11792: a boss (`Extrude1`) joined to a half-ring, and a cut (`Extrude2`) that starts
/// on the boss's `F5:bottom` face. Mirroring both across the front plane copies the boss and the
/// cut, so the part gains what the two features added a second time.
#[test]
fn mirror_of_a_boss_and_a_cut_from_its_face_makes_both_copies() {
    let text = include_str!("../../../../examples/regressions/mirror_from_object_lid.json");
    let open = |through: i64| {
        let mut value: Value = serde_json::from_str(text).unwrap_or_default();
        if let Some(features) = value.get_mut("features").and_then(Value::as_array_mut) {
            features.retain(|f| f.get("id").and_then(Value::as_i64).is_some_and(|id| id <= through));
        }
        let doc = solidcraft_doc::Document::from_json(&value.to_string()).unwrap_or_else(|e| panic!("{e}"));
        Session::opened(doc, None)
    };
    let ring = volume(&mut open(4));
    let mut s = open(6);
    let lid = volume(&mut s);
    run(&mut s, "solid.mirror", json!({"features":["Extrude1","Extrude2"],"plane":"XZ"}));
    let both = volume(&mut s);
    let want = lid + (lid - ring);
    assert!(rel(both, want) < 1e-6, "ring {ring} mm³, lid {lid} mm³, mirrored {both} mm³ (expected {want})");
}

/// #15054: a boss (`Boss`) joined to a bar, and a cut (`Slot`) up to the boss's top that splits
/// the boss in two. Mirrored or patterned together, each copy of the cut ends at the copy of that
/// top, as the cut did: the top the cut was made up to is in two pieces once the cut has run, so
/// it was "no longer exists as it was picked" for the copies.
#[test]
fn copies_of_a_boss_and_a_cut_up_to_its_top_make_both_copies() {
    for (command, params) in [
        ("solid.mirror", json!({"features": ["Boss", "Slot"], "plane": "YZ"})),
        ("solid.pattern.rectangular", json!({"features": ["Boss", "Slot"], "dir1": [-1, 0, 0], "count1": 2, "spacing1": 40})),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 80, "width": 10, "height": 10, "corner": [-40, 0, -10], "body_name": "Bar"}));
        let bar = run(&mut s, "inspect.measure", json!({}))["bodies"][0].clone();
        for (name, sketch, p0, p1) in [("Boss", "B", [5, 0], [35, 10]), ("Slot", "C", [15, 0], [25, 10])] {
            run(&mut s, "sketch.create", json!({"plane": "XY", "name": sketch}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
            run(&mut s, "sketch.finish", json!({}));
            let extent = if name == "Boss" { json!({"distance": 5, "operation": "join"}) } else { json!({"to": [20, 5, 5], "operation": "cut"}) };
            let mut p = json!({"sketch": sketch, "profiles": [0], "name": name});
            if let (Some(p), Some(e)) = (p.as_object_mut(), extent.as_object()) {
                p.extend(e.clone());
            }
            run(&mut s, "solid.extrude", p);
        }
        let one = run(&mut s, "inspect.measure", json!({}))["bodies"][0].clone();
        let r = run(&mut s, command, params);
        assert!(r["error"].is_null(), "{command}: {r}");
        let both = run(&mut s, "inspect.measure", json!({}))["bodies"][0].clone();
        // The bar 80 × 10 × 10 and two bosses 30 × 10 × 5, each with a 10 mm slot through it.
        let v = |m: &Value| m["volume_mm3"].as_f64().unwrap_or(f64::NAN);
        let want = 80.0 * 10.0 * 10.0 + 2.0 * (30.0 - 10.0) * 10.0 * 5.0;
        assert!(rel(v(&one), 80.0 * 10.0 * 10.0 + (30.0 - 10.0) * 10.0 * 5.0) < 1e-9, "{command}: one boss {one}");
        assert!(rel(v(&both), want) < 1e-9, "{command}: {} mm³, expected {want} mm³ (bar {bar})", v(&both));
        // The copy adds the faces, edges and vertices the boss and its slot added, apart from it.
        let count = |m: &Value, what: &str| m[what].as_i64().unwrap_or(-1);
        assert_eq!(both["kernel"]["shells"].as_i64(), Some(1), "{command}: {both}");
        for what in ["faces", "edges", "vertices"] {
            let want = count(&bar, what) + 2 * (count(&one, what) - count(&bar, what));
            assert_eq!(count(&both, what), want, "{command}: {what}: bar {bar}, one boss {one}, both {both}");
        }
    }
}

/// #17692: a pocket cut up to the plate's top, then a chamfer round the pocket's mouth that cuts
/// the top away where the cut's end was picked. The pattern comes after the chamfer and copies
/// only the pocket: its copy ends at the plate's top, as the pocket does, so it is the same
/// 22 × 22 × 5 pocket. The copy used to end at the chamfer's slope left at the pick and was
/// refused ("the profile reaches past the To face").
#[test]
fn a_copied_cut_up_to_a_face_a_later_chamfer_cut_away_at_the_pick_ends_at_that_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 84, "width": 42, "height": 5, "corner": [0, 0, 0], "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cell"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [32, 32]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Cell", "profiles": [0], "to": [33, 21, 5], "operation": "cut", "name": "Pocket"}));
    run(&mut s, "solid.chamfer", json!({"edges": [[21, 10, 5], [21, 32, 5], [10, 21, 5], [32, 21, 5]], "distance": 3, "name": "Lip"}));
    let before = volume(&mut s);
    let r = run(&mut s, "solid.pattern.rectangular", json!({"features": ["Pocket"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 42}));
    assert!(r["error"].is_null() && r["warning"].is_null(), "{r}");
    let after = volume(&mut s);
    assert!(rel(before - after, 22.0 * 22.0 * 5.0) < 1e-9, "the copy removed {} mm³, expected {} mm³", before - after, 22.0 * 22.0 * 5.0);
}

/// #14305: in a component, a pattern or mirror of two cuts makes the copies of both. The first
/// cut's copies used to hand the plate back outside its component, so the second cut's copies
/// found no body there ("there is no body to cut or intersect").
#[test]
fn copies_of_two_cuts_in_a_component_cut_its_body() {
    for (command, params, copies) in [
        ("solid.pattern.circular", json!({"features": ["CutA", "CutB"], "axis": "Z", "count": 4, "angle": 360}), 3.0),
        ("solid.mirror", json!({"features": ["CutA", "CutB"], "plane": "YZ"}), 1.0),
    ] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Plate"}));
        run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 5, "corner": [-50, -50, 0]}));
        for (name, x, r) in [("A", 30, 3), ("B", 40, 2)] {
            run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
            run(&mut s, "sketch.circle.center", json!({"center": [x, 0], "radius": r}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.extrude", json!({"sketch": name, "through_all": true, "operation": "cut", "name": format!("Cut{name}")}));
        }
        run(&mut s, command, params);
        // The plate and each instance's two bores, radius 3 and 2, through its 5 mm.
        let want = 100.0 * 100.0 * 5.0 - (1.0 + copies) * PI * (9.0 + 4.0) * 5.0;
        let v = volume(&mut s);
        assert!(rel(v, want) < 1e-6, "{command}: {v} mm³, expected {want}");
    }
}

/// #14305: a copied cut that splits the plate leaves its pieces in the component, so the copies of
/// holes after it still find the piece each falls on. The slot's copy splits the left part of the
/// plate in two, and one hole's copy lands on each piece.
#[test]
fn copies_of_a_cut_that_splits_the_body_in_a_component_leave_its_pieces_there() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 5, "corner": [-50, -50, 0]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, -60], "p1": [24, 60]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Slot", "through_all": true, "operation": "cut", "name": "CutSlot"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "B"}));
    run(&mut s, "sketch.circle.center", json!({"center": [40, 0], "radius": 2}));
    run(&mut s, "sketch.circle.center", json!({"center": [10, 0], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "B", "through_all": true, "operation": "cut", "name": "CutB"}));
    run(&mut s, "solid.mirror", json!({"features": ["CutSlot", "CutB"], "plane": "YZ"}));
    // The plate less two 4 × 100 slots across it and four bores of radius 2, through its 5 mm.
    let want = 100.0 * 100.0 * 5.0 - 2.0 * 4.0 * 100.0 * 5.0 - 4.0 * PI * 4.0 * 5.0;
    let v = volume(&mut s);
    assert!(rel(v, want) < 1e-6, "{v} mm³, expected {want}");
}

/// #14305: a chamfer among the copied features hands the disc back in its component, so a hole
/// copied after it still finds the disc there, as it does at the design's root.
#[test]
fn copies_of_a_chamfer_and_a_later_hole_in_a_component_cut_its_body() {
    let pi = std::f64::consts::PI;
    // Per instance: the bore, π·3²·10, one ring of the chamfer's triangle (Pappus, as below) and
    // the pin hole, π·2²·10. The six instances do not meet.
    let instance = pi * 9.0 * 10.0 + 2.0 * pi * (3.0 + 0.5 / 3.0) * 0.125 + pi * 4.0 * 10.0;
    let want = pi * 900.0 * 10.0 - 6.0 * instance;
    for in_component in [false, true] {
        let mut s = Session::default();
        if in_component {
            run(&mut s, "component.create", json!({"name": "Part"}));
        }
        run(&mut s, "solid.cylinder", json!({"radius": 30, "height": 10, "name": "Disc"}));
        run(&mut s, "solid.hole", json!({"position": [20, 0, 10], "diameter": 6, "name": "Bore"}));
        run(&mut s, "solid.chamfer", json!({"edges": [[23, 0, 10]], "distance": 0.5, "name": "TopChamfer"}));
        run(&mut s, "solid.hole", json!({"position": [0, 10, 10], "diameter": 4, "name": "Pin"}));
        run(&mut s, "solid.pattern.circular", json!({"features": ["Bore", "TopChamfer", "Pin"], "axis": "Z", "count": 6, "angle": 360}));
        let v = volume(&mut s);
        assert!((v - want).abs() < 4e-7 * want, "component {in_component}: {v} mm³, expected {want}");
    }
}

const BOTH: [&str; 2] = ["BoxA", "BoxB"];

/// Two 40 × 40 × 10 parts: BoxA in A at x −20..20, y −20..20, z 0..10, not placed, and BoxB in B
/// at x −10..30, y −20..20, z 0..10 in its own frame, B turned `angle` degrees about Z and lifted
/// `lift`.
fn placed_parts(angle: f64, lift: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "center": [0, 0, 5], "body_name": "BoxA"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-10, -20, 0], "operation": "new", "body_name": "BoxB"}));
    for (part, body) in [("A", "BoxA"), ("B", "BoxB")] {
        run(&mut s, "component.create", json!({"name": part, "parent": "root", "activate": false}));
        run(&mut s, "component.move_bodies", json!({"bodies": [body], "component": part}));
    }
    let turn = json!({"occurrence": "B:1", "axis": [0, 0, 1], "angle": format!("{angle} deg"), "origin": [0, 0, 0], "translate": [0, 0, lift]});
    run(&mut s, "occurrence.move", turn);
    run(&mut s, "component.capture_position", json!({}));
    s
}

/// [`placed_parts`] and a root cut of x 1..7, y 5..10, sketched 5 mm above B's floor, down through
/// `targets`.
fn root_cut_into_a_placed_part(angle: f64, lift: f64, targets: &[&str]) -> Session {
    let mut s = placed_parts(angle, lift);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": lift + 5.0, "name": "CutSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [1, 5], "p1": [7, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"through_all": true, "direction": "negative", "operation": "cut", "targets": targets, "name": "Cut"}));
    s
}

/// BoxA and BoxB after a mirror of the cut across x = 5: one 8 × 5 pocket at x 1..9, y 5..10 in
/// each where it is shown, 5 mm deep in B (z lift..lift + 5) and as deep in A as the cut reaches.
fn assert_mirrored_into_both(s: &mut Session, angle: f64, lift: f64) {
    let (a, _, a_ends) = pockets(s, "BoxA");
    let (b, b_faces, b_ends) = pockets(s, "BoxB");
    let a_depth = if lift > 0.0 { 10.0 } else { 5.0 };
    let case = format!("turned {angle}, lifted {lift}: BoxA {a} {a_ends:?}, BoxB {b} {b_faces} faces {b_ends:?}");
    assert!(rel(a, 16000.0 - 40.0 * a_depth) < 1e-9, "{case}");
    assert!(rel(b, 15800.0) < 1e-9, "{case}");
    assert_eq!((b_faces, b_ends.len(), a_ends.len()), (11, 1, usize::from(lift == 0.0)), "{case}");
    let (area, at) = b_ends[0];
    assert!(rel(area, 40.0) < 1e-9 && near(at, [5.0, 7.5, lift + 5.0]), "{case}");
}

fn near(p: [f64; 3], q: [f64; 3]) -> bool {
    p.iter().zip(q).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt() < 1e-6
}

fn mirror_the_cut(s: &mut Session) {
    run(s, "construct.plane.offset", json!({"base": "YZ", "offset": 5, "name": "MirrorPlane"}));
    let r = run(s, "solid.mirror", json!({"features": ["Cut"], "plane": "MirrorPlane", "name": "Mirror"}));
    assert!(r["warning"].is_null(), "{r}");
}

/// #19451: a root mirror of a root cut into a part placed turned and lifted cuts the world mirror
/// image of the cut. It cut where the image would be in the part's own frame, turned with the part,
/// and through the whole part, from where the cut started with the part not placed.
#[test]
fn a_mirrored_cut_into_a_turned_and_lifted_part_lands_at_its_world_image() {
    for angle in [180.0, 90.0] {
        let mut s = root_cut_into_a_placed_part(angle, 20.0, &BOTH);
        mirror_the_cut(&mut s);
        assert_mirrored_into_both(&mut s, angle, 20.0);
    }
}

#[test]
fn a_mirrored_cut_into_a_turned_part_lands_at_its_world_image() {
    let mut s = root_cut_into_a_placed_part(180.0, 0.0, &BOTH);
    mirror_the_cut(&mut s);
    assert_mirrored_into_both(&mut s, 180.0, 0.0);
}

#[test]
fn a_mirrored_cut_into_a_lifted_part_starts_where_the_cut_does() {
    let mut s = root_cut_into_a_placed_part(0.0, 20.0, &BOTH);
    mirror_the_cut(&mut s);
    assert_mirrored_into_both(&mut s, 0.0, 20.0);
}

#[test]
fn a_mirrored_cut_into_parts_not_placed_lands_at_its_image() {
    let mut s = root_cut_into_a_placed_part(0.0, 0.0, &BOTH);
    mirror_the_cut(&mut s);
    assert_mirrored_into_both(&mut s, 0.0, 0.0);
}

/// Turning the part after the mirror moves the mirrored cut with the original.
#[test]
fn a_mirrored_cut_follows_its_part_turned_later() {
    let mut s = root_cut_into_a_placed_part(0.0, 20.0, &BOTH);
    mirror_the_cut(&mut s);
    run(&mut s, "occurrence.move", json!({"occurrence": "B:1", "axis": [0, 0, 1], "angle": "180 deg", "origin": [0, 0, 0]}));
    run(&mut s, "component.capture_position", json!({}));
    assert_mirrored_into_both(&mut s, 180.0, 20.0);
}

/// A copy cutting only the part placed turned and lifted is checked against it where it is shown:
/// it removes material there, so the mirror says nothing of copies that remove none.
#[test]
fn a_mirrored_cut_into_a_turned_and_lifted_part_alone_removes_material_there() {
    let mut s = root_cut_into_a_placed_part(180.0, 20.0, &["BoxB"]);
    mirror_the_cut(&mut s);
    let (b, b_faces, b_ends) = pockets(&mut s, "BoxB");
    assert!(rel(b, 15800.0) < 1e-9 && b_faces == 11 && b_ends.len() == 1 && near(b_ends[0].1, [5.0, 7.5, 25.0]), "{b} {b_faces} {b_ends:?}");
    assert!(rel(pockets(&mut s, "BoxA").0, 16000.0) < 1e-9);
}

/// #19451: a rectangular pattern of the cut 10 mm along −x copies it to x −9..−3 in the world, in
/// a part turned and lifted too: two 6 × 5 pockets, through BoxA and 5 mm deep in BoxB.
#[test]
fn a_patterned_cut_into_a_turned_and_lifted_part_lands_at_its_world_copy() {
    let mut s = root_cut_into_a_placed_part(180.0, 20.0, &BOTH);
    let r = run(&mut s, "solid.pattern.rectangular", json!({"features": ["Cut"], "dir1": [-1, 0, 0], "count1": 2, "spacing1": 10}));
    assert!(r["warning"].is_null(), "{r}");
    let (a, _, a_ends) = pockets(&mut s, "BoxA");
    let (b, b_faces, b_ends) = pockets(&mut s, "BoxB");
    let case = format!("BoxA {a} {a_ends:?}, BoxB {b} {b_faces} faces {b_ends:?}");
    assert!(rel(a, 16000.0 - 2.0 * 300.0) < 1e-9 && a_ends.is_empty(), "{case}");
    assert!(rel(b, 16000.0 - 2.0 * 150.0) < 1e-9 && b_faces == 16, "{case}");
    let want = [[-6.0, 7.5, 25.0], [4.0, 7.5, 25.0]];
    assert!(b_ends.len() == 2 && b_ends.iter().zip(want).all(|((area, at), w)| rel(*area, 30.0) < 1e-9 && near(*at, w)), "{case}");
}

/// #19451: a root mirror of a hole at a root sketch's point, 5 mm into B's top where it is shown,
/// drills the world image of the hole: bores ending at x 2 and x 8, y 7.5, z 25.
#[test]
fn a_mirrored_sketch_point_hole_into_a_turned_and_lifted_part_lands_at_its_world_image() {
    let mut s = placed_parts(180.0, 20.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30, "name": "HoleSketch"}));
    run(&mut s, "sketch.point", json!({"point": [2, 7.5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.hole", json!({"sketch": "HoleSketch", "points": ["p1"], "diameter": 2, "depth": 5, "targets": ["BoxB"], "name": "Hole"}));
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": 5, "name": "MirrorPlane"}));
    let r = run(&mut s, "solid.mirror", json!({"features": ["Hole"], "plane": "MirrorPlane"}));
    assert!(r["warning"].is_null(), "{r}");
    let (b, _, ends) = pockets(&mut s, "BoxB");
    let want = [[2.0, 7.5, 25.0], [8.0, 7.5, 25.0]];
    assert!(rel(b, 16000.0 - 10.0 * PI) < 1e-6 && ends.len() == 2 && ends.iter().zip(want).all(|((_, at), w)| near(*at, w)), "{b} {ends:?}");
}

/// #19451: a root mirror of a root rib standing on B's top where it is shown stands there too:
/// two walls 3 long, 2 thick and 5 tall, down from the line at z 35 to the top at z 30.
#[test]
fn a_mirrored_rib_on_a_turned_and_lifted_part_stands_on_it_where_it_is_shown() {
    let mut s = placed_parts(180.0, 20.0);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "name": "RibSketch"}));
    run(&mut s, "sketch.line", json!({"points": [[1, 35], [4, 35]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.rib", json!({"sketch": "RibSketch", "curves": ["l1"], "thickness": 2, "participants": ["BoxB"], "name": "Rib"}));
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": 5, "name": "MirrorPlane"}));
    run(&mut s, "solid.mirror", json!({"features": ["Rib"], "plane": "MirrorPlane"}));
    let b = pockets(&mut s, "BoxB").0;
    assert!(rel(b, 16000.0 + 2.0 * 30.0) < 1e-9, "{b}");
}

/// [`part_with_a_boss_and_a_round`] and MirrorPlane at x = −10, made in the root: where it is
/// shown, Base is its own image across it, Boss at x −26..−20, y −10..−5.
fn part_and_mirror_plane() -> Session {
    let mut s = part_with_a_boss_and_a_round();
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": -10, "name": "MirrorPlane"}));
    s
}

/// [`part_with_a_boss_and_a_round`] with Boss joined into `targets` (none: the part's bodies),
/// before Part is placed, Part active.
fn part_with_a_boss(targets: &[&str]) -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-10, -20, 0], "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "BossSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 5], "p1": [26, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5, "operation": "join", "targets": targets, "name": "Boss"}));
    run(&mut s, "solid.fillet", json!({"edges": [[30, 20, 5]], "radius": 2, "name": "Round"}));
    s
}

/// Part turned 180° about Z and lifted 20, and MirrorPlane at x = −10, both made in the root.
fn place_the_part(s: &mut Session) {
    run(s, "component.activate", json!({"component": "root"}));
    let turn = json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "180 deg", "origin": [0, 0, 0], "translate": [0, 0, 20]});
    run(s, "occurrence.move", turn);
    run(s, "component.capture_position", json!({}));
    run(s, "construct.plane.offset", json!({"base": "YZ", "offset": -10, "name": "MirrorPlane"}));
}

/// #19451: a root mirror or pattern of a join drawn in a part placed turned and lifted adds the
/// world copy of the boss: its top at x 0..6 across x = −10, at x −6..0 20 along x. The copy was
/// made as if the part had no placement, turned with it, off Base.
#[test]
fn a_copied_join_in_a_turned_and_lifted_part_lands_at_its_world_copy() {
    for (command, params, x) in [
        ("solid.mirror", json!({"features": ["Boss"], "plane": "MirrorPlane"}), 3.0),
        ("solid.pattern.rectangular", json!({"features": ["Boss"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20}), -3.0),
    ] {
        let mut s = part_and_mirror_plane();
        let r = run(&mut s, command, params);
        assert!(r["warning"].is_null(), "{command}: {r}");
        let (v, _, tops) = pockets(&mut s, "Base");
        let case = format!("{command}: {v} {tops:?}");
        assert!(rel(v, 16000.0 + 2.0 * 150.0 - ROUND) < 1e-9, "{case}");
        let want = [[-23.0, -7.5, 35.0], [x, -7.5, 35.0]];
        assert!(tops.len() == 2 && tops.iter().zip(want).all(|((area, at), w)| rel(*area, 30.0) < 1e-9 && near(*at, w)), "{case}");
    }
}

/// #19451: moving the part after the mirror moves the copied boss to the new world image: with
/// Part 4 further along x, Boss's top is at x −19 and its image across x = −10 at x −1.
#[test]
fn a_mirrored_join_follows_its_part_moved_later() {
    let mut s = part_and_mirror_plane();
    run(&mut s, "solid.mirror", json!({"features": ["Boss"], "plane": "MirrorPlane"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [4, 0, 0]}));
    run(&mut s, "component.capture_position", json!({}));
    let (_, _, tops) = pockets(&mut s, "Base");
    let want = [[-19.0, -7.5, 35.0], [-1.0, -7.5, 35.0]];
    assert!(tops.len() == 2 && tops.iter().zip(want).all(|((_, at), w)| near(*at, w)), "{tops:?}");
}

/// #19451: a root mirror of a round in a part placed turned and lifted rounds the image of its
/// edge where the part is shown, the upright edge at x 10, y −20, taking off what the round does.
/// The copy looked for the edge where the image would be in the part's own frame, off Base.
#[test]
fn a_mirrored_round_in_a_turned_and_lifted_part_rounds_the_image_of_its_edge() {
    let mut s = part_and_mirror_plane();
    let before = pockets(&mut s, "Base").0;
    assert!(rel(before, 16150.0 - ROUND) < 1e-9, "{before}");
    let r = run(&mut s, "solid.mirror", json!({"features": ["Round"], "plane": "MirrorPlane"}));
    assert!(r["warning"].is_null(), "{r}");
    let v = pockets(&mut s, "Base").0;
    let axes = round_axes(&mut s, "Base", 2.0);
    assert!(rel(before - v, ROUND) < 1e-9, "{before} -> {v}");
    assert!(axes.len() == 2 && near2(axes[0], [-28.0, -18.0]) && near2(axes[1], [8.0, -18.0]), "{axes:?}");
}

/// #19451: a root mirror across XZ of that mirror rounds the image of its copy, the edge at x 10,
/// y 20. It looked for the edge where the image would be in the part's own frame.
#[test]
fn a_mirror_of_a_mirrored_round_in_a_turned_part_rounds_the_image_of_its_copy() {
    let mut s = part_and_mirror_plane();
    run(&mut s, "solid.mirror", json!({"features": ["Round"], "plane": "MirrorPlane", "name": "Mirror"}));
    let r = run(&mut s, "solid.mirror", json!({"features": ["Mirror"], "plane": "XZ"}));
    assert!(r["warning"].is_null(), "{r}");
    let v = pockets(&mut s, "Base").0;
    let axes = round_axes(&mut s, "Base", 2.0);
    assert!(rel(v, 16150.0 - 3.0 * ROUND) < 1e-9, "{v}");
    let want = [[-28.0, -18.0], [8.0, -18.0], [8.0, 18.0]];
    assert!(axes.len() == 3 && axes.iter().zip(want).all(|(a, w)| near2(*a, w)), "{axes:?}");
}

/// #19451: a root mirror or pattern of a join that names no body, drawn in a part placed turned
/// and lifted, joins the part at the world copy of the boss, as the join did: Base takes the
/// 150 mm³ boss again and no body is made. The copy was a new body at the part's own coordinates.
#[test]
fn a_copied_untargeted_join_in_a_turned_and_lifted_part_joins_the_part() {
    for (command, params, x) in [
        ("solid.mirror", json!({"features": ["Boss"], "plane": "MirrorPlane"}), 3.0),
        ("solid.pattern.rectangular", json!({"features": ["Boss"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20}), -3.0),
    ] {
        let mut s = part_with_a_boss(&[]);
        place_the_part(&mut s);
        let before = pockets(&mut s, "Base").0;
        let r = run(&mut s, command, params);
        assert!(r["warning"].is_null(), "{command}: {r}");
        let (v, _, tops) = pockets(&mut s, "Base");
        let bodies: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
        let case = format!("{command}: {bodies:?} {before} -> {v} {tops:?}");
        assert!(bodies == ["Base"] && rel(v - before, 150.0) < 1e-9, "{case}");
        let want = [[-23.0, -7.5, 35.0], [x, -7.5, 35.0]];
        assert!(tops.len() == 2 && tops.iter().zip(want).all(|((_, at), w)| near(*at, w)), "{case}");
    }
}

/// What a 1 mm round takes off an upright 5 mm edge of the boss.
const BOSS_ROUND: f64 = (4.0 - PI) / 4.0 * 5.0;

/// [`part_and_mirror_plane`] with an untargeted Boss and BossRound, 1 mm rounds of its two
/// upright edges at x 26 (x −26 where it is shown), mirrored by Mirror across x = −10 with Boss,
/// the two picked in the order of `features`.
fn mirrored_untargeted_boss_and_its_rounds(features: [&str; 2]) -> Session {
    let mut s = part_with_a_boss(&[]);
    run(&mut s, "solid.fillet", json!({"edges": [[26, 10, 12.5], [26, 5, 12.5]], "radius": 1, "name": "BossRound"}));
    place_the_part(&mut s);
    let r = run(&mut s, "solid.mirror", json!({"features": features, "plane": "MirrorPlane", "name": "Mirror"}));
    assert!(r["warning"].is_null(), "{r}");
    s
}

/// #19451: a root mirror of an untargeted join in a part placed turned and lifted and of the
/// rounds of the edges it made rounds the copy's edges: the boss's image at x 0..6 has rounds
/// about x 5, y −9 and y −6, as the boss has about x −25.
#[test]
fn a_mirrored_untargeted_join_and_its_rounds_in_a_turned_part_land_together() {
    let mut s = mirrored_untargeted_boss_and_its_rounds(["Boss", "BossRound"]);
    let v = pockets(&mut s, "Base").0;
    let axes = round_axes(&mut s, "Base", 1.0);
    assert_eq!(s.model.state().bodies.len(), 1);
    assert!(rel(v, 16000.0 + 2.0 * 150.0 - ROUND - 4.0 * BOSS_ROUND) < 1e-9, "{v}");
    let want = [[-25.0, -9.0], [-25.0, -6.0], [5.0, -9.0], [5.0, -6.0]];
    assert!(axes.len() == 4 && axes.iter().zip(want).all(|(a, w)| near2(*a, w)), "{axes:?}");
}

/// #19451: a root mirror across XZ of that mirror makes every copy of its copies: a third boss at
/// x 0..6, y 5..10, its rounds about x 5, y 6 and y 9. It copies them in timeline order, whatever
/// order they were picked in: picked first, the round's copy found no edge of the boss's copy.
#[test]
fn a_mirror_of_a_mirrored_untargeted_join_and_its_rounds_lands_every_copy() {
    for features in [["Boss", "BossRound"], ["BossRound", "Boss"]] {
        let mut s = mirrored_untargeted_boss_and_its_rounds(features);
        let r = s.execute("solid.mirror", &json!({"features": ["Mirror"], "plane": "XZ"}));
        assert!(r.as_ref().is_ok_and(|r| r["warning"].is_null()), "{features:?}: {r:?}");
        let v = pockets(&mut s, "Base").0;
        let axes = round_axes(&mut s, "Base", 1.0);
        assert_eq!(s.model.state().bodies.len(), 1);
        assert!(rel(v, 16000.0 + 3.0 * 150.0 - ROUND - 6.0 * BOSS_ROUND) < 1e-9, "{features:?}: {v}");
        let want = [[-25.0, -9.0], [-25.0, -6.0], [5.0, -9.0], [5.0, -6.0], [5.0, 6.0], [5.0, 9.0]];
        assert!(axes.len() == 6 && axes.iter().zip(want).all(|(a, w)| near2(*a, w)), "{features:?}: {axes:?}");
    }
}

/// The same mirror made in the part, across x = −10 where it is shown (its own x = 10), copies
/// the boss and the round in the part's frame, as before: the same copies.
#[test]
fn a_mirror_in_a_turned_part_of_its_own_features_copies_them_in_its_frame() {
    let mut s = part_and_mirror_plane();
    run(&mut s, "component.activate", json!({"component": "Part"}));
    let r = run(&mut s, "solid.mirror", json!({"features": ["Boss", "Round"], "plane": {"origin": [-10, 0, 0], "normal": [1, 0, 0]}}));
    assert!(r["warning"].is_null(), "{r}");
    let (v, _, tops) = pockets(&mut s, "Base");
    let axes = round_axes(&mut s, "Base", 2.0);
    assert!(rel(v, 16000.0 + 2.0 * 150.0 - 2.0 * ROUND) < 1e-9, "{v}");
    let want = [[-23.0, -7.5, 35.0], [3.0, -7.5, 35.0]];
    assert!(tops.len() == 2 && tops.iter().zip(want).all(|((_, at), w)| near(*at, w)), "{tops:?}");
    assert!(axes.len() == 2 && near2(axes[0], [-28.0, -18.0]) && near2(axes[1], [8.0, -18.0]), "{axes:?}");
}

/// Base, a root body moved into Part by a transfer, with Pocket, an untargeted cut of x 0..6,
/// y −15..−10 5 deep into its top, Boss, an untargeted join of x 20..26, y 5..10 on its top, 5
/// high, and BossBlend, a 1 mm `blend` (`solid.fillet` or `solid.chamfer`) of the boss's upright
/// edges at x 26, naming Base; then Part placed turned 180° about Z and lifted 20. Where it is
/// shown, Base is at x −30..10, y −20..20, z 20..30, the pocket at x −6..0, y 10..15 and the
/// boss at x −26..−20, y −10..−5.
fn transferred_part_with_a_blended_boss(blend: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-10, -20, 0], "body_name": "Base"}));
    run(&mut s, "component.create", json!({"name": "Part", "parent": "root", "grounded": false, "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Base"], "component": "Part"}));
    run(&mut s, "component.activate", json!({"component": "Part"}));
    for (name, p0, p1) in [("Pocket", [0, -15], [6, -10]), ("Boss", [20, 5], [26, 10])] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"distance": 5, "operation": "join", "name": name});
        if name == "Pocket" {
            p["operation"] = json!("cut");
            p["direction"] = json!("negative");
        }
        run(&mut s, "solid.extrude", p);
    }
    let mut p = json!({"edges": [[26, 10, 12.5], [26, 5, 12.5]], "body": "Base", "name": "BossBlend"});
    p[if blend == "solid.fillet" { "radius" } else { "distance" }] = json!(1);
    run(&mut s, blend, p);
    place_the_part(&mut s);
    s
}

/// What a 1 mm `blend` takes off one upright 5 mm edge of the boss.
fn boss_blend(blend: &str) -> f64 {
    if blend == "solid.fillet" { BOSS_ROUND } else { 2.5 }
}

/// The x − y = −8 plane where it is shown, square to Z: not one the part's turn leaves in place.
fn diagonal_plane() -> Value {
    json!({"origin": [-8, 0, 0], "normal": [1, -1, 0]})
}

/// Base's volume, pocket floors, boss tops (each within 0.3 mm: the blend takes their corners
/// off) and rounds after `copies` mirrored copies of Pocket, Boss and BossBlend.
fn assert_blended_copies(s: &mut Session, blend: &str, copies: usize, floors: &[[f64; 2]], tops: &[[f64; 2]], rounds: &[[f64; 2]]) {
    let v = pockets(s, "Base").0;
    let (got_floors, got_tops) = (pocket_ends_at(s, "Base", 25.0), pocket_ends_at(s, "Base", 35.0));
    let case = format!("{blend}: {v}, floors {got_floors:?}, tops {got_tops:?}");
    assert!(rel(v, 16000.0 - 2.0 * (copies + 1) as f64 * boss_blend(blend)) < 1e-9, "{case}");
    assert!(got_floors.len() == floors.len() && got_floors.iter().zip(floors).all(|(a, w)| near2(*a, *w)), "{case}");
    let close = |a: &[f64; 2], w: &[f64; 2]| (a[0] - w[0]).hypot(a[1] - w[1]) < 0.3;
    assert!(got_tops.len() == tops.len() && got_tops.iter().zip(tops).all(|(a, w)| close(a, w)), "{case}");
    if blend == "solid.fillet" {
        let axes = round_axes(s, "Base", 1.0);
        assert!(axes.len() == rounds.len() && axes.iter().zip(rounds).all(|(a, w)| near2(*a, *w)), "{blend}: {axes:?}");
    }
}

/// #19451: a root mirror across a diagonal plane of a pocket, a boss and a blend of the boss's
/// edges in a part placed turned, its body moved there by a transfer, makes each copy at its
/// world image, the blend by its edges' names and by its picks: the pocket's image at x 2..7,
/// y 2..8, the boss's at x −18..−13, y −18..−12, its rounds about y −17. Once the pocket's copy
/// had changed Base, the copies took Base for a root body: the boss's copy was made through the
/// part's placement twice and the blend's looked for its edges at their image in the part's own
/// frame, off Base, and blended the nearest body there.
#[test]
fn a_mirrored_blend_in_a_transferred_turned_part_lands_at_its_world_image() {
    for (blend, by_name) in [("solid.fillet", true), ("solid.fillet", false), ("solid.chamfer", true)] {
        let mut s = transferred_part_with_a_blended_boss(blend);
        if !by_name {
            let f = s.doc_mut().features.iter_mut().find(|f| f.name == "BossBlend").unwrap();
            f.edge_names.clear();
            f.edge_refs.clear();
        }
        let r = s.execute("solid.mirror", &json!({"features": ["Pocket", "Boss", "BossBlend"], "plane": diagonal_plane()}));
        assert!(r.as_ref().is_ok_and(|r| r["warning"].is_null()), "{blend} by name {by_name}: {r:?}");
        let rounds = [[-25.0, -9.0], [-25.0, -6.0], [-17.0, -17.0], [-14.0, -17.0]];
        assert_blended_copies(&mut s, blend, 1, &[[-3.0, 12.5], [4.5, 5.0]], &[[-23.0, -7.5], [-15.5, -15.0]], &rounds);
    }
}

/// #19451: a root mirror across XZ of that mirror makes every copy of its copies there too: a
/// third pocket at x 2..7, y −8..−2, a third boss at x −18..−13, y 12..18 and its rounds about
/// y 17. It looked for the blend's edges in the part's own frame, and refused the smooth edge it
/// found there.
#[test]
fn a_mirror_of_a_mirrored_blend_in_a_transferred_turned_part_lands_every_copy() {
    for blend in ["solid.fillet", "solid.chamfer"] {
        let mut s = transferred_part_with_a_blended_boss(blend);
        run(&mut s, "solid.mirror", json!({"features": ["Pocket", "Boss", "BossBlend"], "plane": diagonal_plane(), "name": "Mirror"}));
        let r = s.execute("solid.mirror", &json!({"features": ["Mirror"], "plane": "XZ"}));
        assert!(r.as_ref().is_ok_and(|r| r["warning"].is_null()), "{blend}: {r:?}");
        let floors = [[-3.0, 12.5], [4.5, -5.0], [4.5, 5.0]];
        let tops = [[-23.0, -7.5], [-15.5, -15.0], [-15.5, 15.0]];
        let rounds = [[-25.0, -9.0], [-25.0, -6.0], [-17.0, -17.0], [-17.0, 17.0], [-14.0, -17.0], [-14.0, 17.0]];
        assert_blended_copies(&mut s, blend, 2, &floors, &tops, &rounds);
    }
}

/// #19451: a root mirror whose first copy makes a body (Spare's copy) joins a later untargeted
/// join's copy to the part, as the join did: the bodies the mirror makes are not the root's to
/// join. The boss's copy went into Spare's copy as a separate piece, off Base.
#[test]
fn a_copied_untargeted_join_after_a_copied_body_joins_the_part() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-10, -20, 0], "body_name": "Base"}));
    run(&mut s, "solid.move", json!({"bodies": ["Base"], "translate": [0, 0, 50], "copy": true, "name": "Spare"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "BossSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 5], "p1": [26, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5, "operation": "join", "name": "Boss"}));
    place_the_part(&mut s);
    let r = run(&mut s, "solid.mirror", json!({"features": ["Spare", "Boss"], "plane": "MirrorPlane"}));
    assert!(r["warning"].is_null(), "{r}");
    let (v, _, tops) = pockets(&mut s, "Base");
    let want = [[-23.0, -7.5, 35.0], [3.0, -7.5, 35.0]];
    assert!(rel(v, 16000.0 + 2.0 * 150.0) < 1e-9 && tops.len() == 2 && tops.iter().zip(want).all(|((_, at), w)| near(*at, w)), "{v} {tops:?}");
}

/// #19451: a root mirror of a round in a part placed turned and lifted rounds the image of its
/// edge on the body there, a root body: Twin, at x 20..60 where Base's image across x = 15 is,
/// is rounded at its upright edge at x 60, y −20.
#[test]
fn a_mirrored_round_in_a_turned_part_rounds_a_root_body_at_its_image() {
    let mut s = part_with_a_boss_and_a_round();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [20, -20, 20], "operation": "new", "body_name": "Twin"}));
    let r = run(&mut s, "solid.mirror", json!({"features": ["Round"], "plane": {"origin": [15, 0, 0], "normal": [1, 0, 0]}}));
    assert!(r["warning"].is_null(), "{r}");
    let v = pockets(&mut s, "Twin").0;
    let axes = round_axes(&mut s, "Twin", 2.0);
    assert!(rel(v, 16000.0 - ROUND) < 1e-9 && axes.len() == 1 && near2(axes[0], [58.0, -18.0]), "{v} {axes:?}");
}

/// #19451: a root mirror of a hole placed at a point picked on B's top, kept in BoxB's frame,
/// drills the world image of the hole: bores ending at x 2 and x 8, y 7.5, z 25. The copy was
/// drilled at the image in BoxB's own frame, x −12 where it is shown.
#[test]
fn a_mirrored_picked_hole_in_a_turned_and_lifted_part_lands_at_its_world_image() {
    let mut s = placed_parts(180.0, 20.0);
    run(&mut s, "solid.hole", json!({"position": [-2, -7.5, 10], "diameter": 2, "depth": 5, "targets": ["BoxB"], "name": "Hole"}));
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": 5, "name": "MirrorPlane"}));
    let r = run(&mut s, "solid.mirror", json!({"features": ["Hole"], "plane": "MirrorPlane"}));
    assert!(r["warning"].is_null(), "{r}");
    let (b, _, ends) = pockets(&mut s, "BoxB");
    let want = [[2.0, 7.5, 25.0], [8.0, 7.5, 25.0]];
    assert!(rel(b, 16000.0 - 10.0 * PI) < 1e-6 && ends.len() == 2 && ends.iter().zip(want).all(|((_, at), w)| near(*at, w)), "{b} {ends:?}");
}

/// #20208 review: a root pattern of a root Move copy of Left and Right, whose copy of Right a
/// transfer moved into Part, placed lifted 20, copies each copy where it is shown, and moving
/// Part after the pattern gives what moving it first gives. The pattern read where the first
/// copy, Left's, is kept, the root, and so kept the copy of Right's copy where Part had been.
#[test]
fn a_pattern_of_a_moved_copy_follows_the_part_its_second_copy_was_moved_into() {
    let setup = || {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Left"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new", "body_name": "Right"}));
        run(&mut s, "solid.move", json!({"bodies": ["Left", "Right"], "translate": [0, 30, 0], "copy": true, "name": "Copy"}));
        let copy = s.doc.features.iter().find(|f| f.name == "Copy").unwrap().id;
        let made: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == copy).map(|b| b.name.clone()).collect();
        assert_eq!(made.len(), 2, "{made:?}");
        run(&mut s, "component.create", json!({"name": "Part", "parent": "root", "grounded": false, "activate": false}));
        run(&mut s, "component.move_bodies", json!({"bodies": [made[1]], "component": "Part"}));
        run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 0, 20]}));
        s
    };
    let pattern = json!({"features": ["Copy"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 100, "name": "Pat"});
    let copies = |s: &Session| {
        let pat = s.doc.features.iter().find(|f| f.name == "Pat").unwrap().id;
        let st = s.model.state();
        let centroid = |b: &solidcraft_doc::ModelBody| b.measure().map(|m| [m.centroid.x, m.centroid.y, m.centroid.z]).unwrap();
        let mut at: Vec<[f64; 3]> = st.bodies.iter().filter(|b| b.feature == pat).map(centroid).collect();
        at.sort_by(|a, b| a[0].total_cmp(&b[0]));
        at
    };
    let later_move = json!({"occurrence": "Part:1", "translate": [0, 0, 40]});
    let mut s = setup();
    run(&mut s, "solid.pattern.rectangular", pattern.clone());
    let before = copies(&s);
    assert!(before.len() == 2 && near(before[0], [105.0, 35.0, 5.0]) && near(before[1], [125.0, 35.0, 25.0]), "{before:?}");
    run(&mut s, "occurrence.move", later_move.clone());
    let later = copies(&s);
    let mut first = setup();
    run(&mut first, "occurrence.move", later_move);
    run(&mut first, "solid.pattern.rectangular", pattern);
    let moved_first = copies(&first);
    assert!(near(moved_first[1], [125.0, 35.0, 65.0]), "{moved_first:?}");
    assert!(later.len() == 2 && later.iter().zip(&moved_first).all(|(a, b)| near(*a, *b)), "moved after {later:?}, before {moved_first:?}");
}

/// #20208 review: a mirror of a Thread of two holes on two plates threads each copy on its own
/// plate, though the second plate's geometry is kept in another frame. PlateA at x 0..40, y 0..30
/// and PlateB, made at x −40..0, y 50..80 and offset 40 along x, each have holes at x 10 and
/// x 30 where they are shown; the Thread is on the holes at x 10, the mirror across x = 20. The
/// second face's copy was moved by the mirror in PlateB's own frame, to x 66.6 there, off it.
#[test]
fn a_mirror_of_a_thread_on_two_bodies_threads_each_where_its_body_is_kept() {
    let mut s = Session::default();
    for (name, x, y) in [("PlateA", 0, 0), ("PlateB", -40, 50)] {
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "corner": [x, y, 0], "operation": "new", "body_name": name}));
        for hx in [x + 10, x + 30] {
            run(&mut s, "solid.hole", json!({"position": [hx, y + 15, 10], "diameter": 6.8, "depth": 10, "targets": [name]}));
        }
    }
    let faces = json!([{"body": "PlateA", "point": [13.4, 15, 5]}, {"body": "PlateB", "point": [-26.6, 65, 5]}]);
    run(&mut s, "solid.thread", json!({"faces": faces, "designation": "M8", "name": "Thread"}));
    let mut offset = solidcraft_doc::IDENTITY;
    offset[3][0] = 40.0;
    s.doc_mut().body_offsets.insert("PlateB".into(), offset);
    let r = s.execute("solid.mirror", &json!({"features": ["Thread"], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    assert!(r.as_ref().is_ok_and(|r| r["warning"].is_null()), "{r:?}");
    let st = s.model.state();
    let mut axes: Vec<(&str, f64)> = st.threads.iter().map(|t| (t.body.as_str(), t.axis_point.x)).collect();
    axes.sort_by(|a, b| a.0.cmp(b.0).then(a.1.total_cmp(&b.1)));
    let want = [("PlateA", 10.0), ("PlateA", 30.0), ("PlateB", -30.0), ("PlateB", -10.0)];
    assert!(axes.len() == 4 && axes.iter().zip(want).all(|(a, w)| a.0 == w.0 && (a.1 - w.1).abs() < 1e-6), "{axes:?}");
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
    // One chamfered hole takes its bore, π·3²·10, and two rings of the chamfer's triangle,
    // 0.5 × 0.5 / 2, about the bore's axis at its centroid's radius 3 + 0.5/3 (Pappus). The
    // copies, 20 apart on the radius 20, do not meet: the pattern takes five more.
    let pi = std::f64::consts::PI;
    let hole = pi * 9.0 * 10.0 + 2.0 * 2.0 * pi * (3.0 + 0.5 / 3.0) * 0.125;
    assert!((v1 - (pi * 900.0 * 10.0 - hole)).abs() < 4e-7 * v1, "{v1}");
    assert!((v - (v1 - 5.0 * hole)).abs() < 4e-7 * v1, "{v1} -> {v}, expected {}", v1 - 5.0 * hole);
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

/// A 100 x 60 x 30 box shelled 5 mm (floor included) with a 20 x 11 boss joined to its +Y wall:
/// 67,500 mm³ hollow, 73,500 mm³ with the boss, which adds 6,000 mm³ (#6452).
fn hollow_box_with_join_boss() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-50, -30], "p1": [50, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Base", "distance": 30, "operation": "new", "name": "Block", "body_name": "Box"}));
    run(&mut s, "solid.shell", json!({"faces": [[0, 0, 30]], "thickness": 5, "name": "Shell"}));
    assert!(rel(volume(&mut s), 67500.0) < 1e-6, "hollow box");
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Boss"}));
    run(&mut s, "sketch.line", json!({"closed": true, "points": [[20, 29], [40, 29], [40, 40], [20, 40]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Boss", "distance": 30, "operation": "join", "targets": ["Box"], "name": "Join Boss"}));
    assert!(rel(volume(&mut s), 73500.0) < 1e-6, "joined boss");
    s
}

/// The copy of a join is built where the source stands, and the source's own material used to
/// read as the target already filling the copy's bottom band: the mirrored boss lost its
/// 20 x 10 x 5 mm floor-level slice with no warning (#6452).
#[test]
fn mirror_of_a_join_on_a_hollow_part_copies_the_whole_boss() {
    let mut s = hollow_box_with_join_boss();
    run(&mut s, "solid.mirror", json!({"features": ["Join Boss"], "plane": "XZ", "name": "Mirror Boss"}));
    assert!(rel(volume(&mut s), 79500.0) < 1e-6, "mirrored copy lost material");
}

#[test]
fn mirror_of_a_join_on_a_hollow_part_copies_the_whole_boss_across_a_custom_plane() {
    let mut s = hollow_box_with_join_boss();
    run(&mut s, "solid.mirror", json!({"features": ["Join Boss"], "plane": {"origin": [0, 0, 0], "normal": [0, 1, 0]}, "name": "Mirror Boss"}));
    assert!(rel(volume(&mut s), 79500.0) < 1e-6, "mirrored copy lost material");
}

#[test]
fn a_mirrored_join_keeps_a_detached_or_edge_touching_tab_separate() {
    for left in [-10, -7, -6, -4] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10-left, "width": 8, "height": 4, "corner": [left, -4, 0], "body_name": "Plate"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tab"}));
        // The seed overlaps x=6..8. Its copy runs from x=-8 to -6, with the
        // bottom of its extrusion on the plate's top. At x=-6 it shares an
        // edge, and at x=-4 it is detached. Neither contact joins the pieces.
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [6, -1], "p1": [8, 1]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({
                "sketch": "Tab", "start_offset": 4, "distance": 3, "operation": "join",
                "targets": ["Plate"], "name": "Join Tab"
            }),
        );
        let seed_volume = f64::from(10 - left) * 8.0 * 4.0 + 12.0;
        assert!(rel(volume(&mut s), seed_volume) < 1e-9);
        let reply = run(&mut s, "solid.mirror", json!({"features": ["Join Tab"], "plane": "YZ"}));
        assert!(rel(volume(&mut s), seed_volume + 12.0) < 1e-9);
        let bodies = &s.model.state().bodies;
        match left {
            -6 => {
                assert_eq!(bodies.len(), 2);
                assert_eq!(bodies[1].name, "Plate (1)");
                assert!(rel(bodies[1].body.volume_at(1e-8).unwrap(), 12.0) < 1e-9);
                assert!(reply["notice"]["message"].as_str().is_some_and(|message| message.contains("edge")), "{reply}");
            }
            -4 => {
                assert_eq!(bodies.len(), 1);
                assert_eq!(bodies[0].body.lumps().unwrap().len(), 2);
                assert!(reply["warning"].as_str().is_some_and(|warning| warning.contains("`Plate` is now in 2 separate pieces")), "{reply}");
            }
            _ => {
                assert_eq!(bodies.len(), 1);
                assert_eq!(bodies[0].body.lumps().unwrap().len(), 1);
                assert!(reply["warning"].is_null(), "{reply}");
            }
        }
    }
}

#[test]
fn a_later_mirrored_extrusion_can_connect_an_earlier_copy() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 14, "width": 8, "height": 4, "corner": [-4, -4, 0], "body_name": "Plate"}));
    for (name, p0, p1) in [("Tab", [6, -1], [8, 1]), ("Bridge", [3, -1], [7, 1])] {
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": name}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.extrude",
            json!({"sketch": name, "start_offset": 3, "distance": 3, "operation": "join", "targets": ["Plate"], "name": format!("Join {name}")}),
        );
    }
    let reply = run(&mut s, "solid.mirror", json!({"features": ["Join Tab", "Join Bridge"], "plane": "YZ"}));
    assert!(reply["warning"].is_null(), "{reply}");
    let bodies = &s.model.state().bodies;
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].body.lumps().unwrap().len(), 1);
    // The plate is 448 mm³. At z=3..4,
    // only the reflected x=-8..-4 strip adds material. At z=4..6 both 5-by-2
    // footprints add material.
    assert!(rel(volume(&mut s), 496.0) < 1e-9);
}

/// A pure translation loses the same slice: the copy must be congruent to the original,
/// whatever filled the space where its tool was rebuilt (#6452).
#[test]
fn rectangular_pattern_of_a_join_on_a_hollow_part_copies_the_whole_boss() {
    let mut s = hollow_box_with_join_boss();
    run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"features": ["Join Boss"], "dir1": [0, -1, 0], "count1": 2, "spacing1": 69, "name": "Pattern Boss"}),
    );
    assert!(rel(volume(&mut s), 79500.0) < 1e-6, "patterned copy lost material");
}

/// A Move that copies can be the source of a feature pattern (#5931): each pattern copy is
/// another moved copy, placed round the axis, and the same as patterning the copied body.
#[test]
fn circular_pattern_of_a_moved_copy_adds_one_body_per_instance() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Block"}));
    run(&mut s, "solid.move", json!({"bodies": ["Block"], "translate": [10, 0, 0], "copy": true, "name": "Move1"}));
    let one = volume(&mut s);
    run(&mut s, "solid.pattern.circular", json!({"features": ["Move1"], "axis": "Z", "count": 4, "angle": "360 deg"}));
    assert!(rel(volume(&mut s), one * 2.5) < 1e-6 && s.world_state().unwrap().bodies.len() == 5, "the moved copy repeats three more times");
    // Moving the first copy elsewhere moves the repeats with it: they follow the feature.
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Move1", "command": "solid.move", "params": {"bodies": ["Block"], "translate": [20, 0, 0], "copy": true}}),
    );
    assert_eq!(s.world_state().unwrap().bodies.len(), 5);
    let far = s.world_state().unwrap().bodies.iter().map(|b| b.mesh().bounds().max.x).fold(f64::MIN, f64::max);
    assert!(far > 20.0, "the repeats follow the redefined Move: {far}");
}

/// A pattern of bodies makes copies that look and weigh like their sources (#12115): the same
/// part in another place. Two bodies, three along X: each source's two copies take its own
/// appearance or material, not the other source's and not the default.
#[test]
fn a_body_pattern_keeps_each_sources_appearance_and_material() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"center":[0,0,5],"operation":"new","body_name":"Base"}));
    run(&mut s, "solid.cylinder", json!({"diameter":6,"height":20,"base":[0,0,10],"operation":"new","body_name":"Post"}));
    run(&mut s, "appearance.assign", json!({"bodies":["Post"],"color":[200,30,30]}));
    run(&mut s, "material.assign", json!({"bodies":["Base"],"material":"Steel"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies":["Base","Post"],"dir1":[1,0,0],"count1":3,"spacing1":30}));
    let red = s.doc.appearances.bodies.get("Post").cloned().expect("Post is red");
    for copy in ["Base (2)", "Base (3)"] {
        assert_eq!(s.doc.resolved_body_material(copy), Some("Steel"), "{copy}");
        assert!(!s.doc.appearances.bodies.contains_key(copy), "{copy} was not painted");
    }
    for copy in ["Post (2)", "Post (3)"] {
        assert_eq!(s.doc.appearances.bodies.get(copy), Some(&red), "{copy}");
        assert_eq!(s.doc.resolved_body_material(copy), None, "{copy} has no material of its own");
    }
}

/// A pattern of bodies that makes no copy (a count of 1) is still added: the copies' look is
/// copied only where there are copies.
#[test]
fn a_body_pattern_with_no_copies_is_still_added() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"center":[0,0,5],"operation":"new","body_name":"Base"}));
    run(&mut s, "material.assign", json!({"bodies":["Base"],"material":"Steel"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies":["Base"],"dir1":[1,0,0],"count1":1,"spacing1":30}));
    assert_eq!(s.model.state().bodies.len(), 1);
}

/// Regression (#21245): editing a body pattern via `timeline.edit` copies source material and
/// appearance onto newly created copies (from `count1 = 1` or more copies), preserves an
/// existing copy's explicit override, and applies a newly chosen seed body's material/appearance.
#[test]
fn editing_a_body_pattern_keeps_source_material_and_appearance_on_copies() {
    // 1) Increasing count1 via timeline.edit copies material & appearance while keeping overrides.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Base"}));
    run(&mut s, "solid.cylinder", json!({"diameter": 6, "height": 20, "base": [0, 50, 0], "operation": "new", "body_name": "Post"}));
    run(&mut s, "material.assign", json!({"bodies": ["Base"], "material": "Steel"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Base"], "color": [40, 120, 200]}));
    run(&mut s, "material.assign", json!({"bodies": ["Post"], "material": "Brass"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Post"], "color": [200, 30, 30]}));
    let blue = s.doc.appearances.bodies.get("Base").cloned().expect("Base has appearance");
    let red = s.doc.appearances.bodies.get("Post").cloned().expect("Post is red");
    let pat = run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Base"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30}))["feature"]
        .as_u64()
        .unwrap();
    // Override Base (2) manually; it must stay Brass/green when count1 rises to 4.
    run(&mut s, "material.assign", json!({"bodies": ["Base (2)"], "material": "Brass"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Base (2)"], "color": [20, 180, 60]}));
    let green = s.doc.appearances.bodies.get("Base (2)").cloned().expect("Base (2) is green");

    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"pattern.count1": 4}}));
    assert_eq!(s.doc.materials.get("Base (2)").map(String::as_str), Some("Brass"));
    assert_eq!(s.doc.appearances.bodies.get("Base (2)"), Some(&green));
    for copy in ["Base (3)", "Base (4)"] {
        assert_eq!(s.doc.resolved_body_material(copy), Some("Steel"), "{copy}");
        assert_eq!(s.doc.appearances.bodies.get(copy), Some(&blue), "{copy}");
    }

    // 2) Starting from count1 = 1 (no initial copies), raising count1 to 3 copies onto new copies.
    let mut one = Session::default();
    run(&mut one, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Base"}));
    run(&mut one, "material.assign", json!({"bodies": ["Base"], "material": "Aluminum"}));
    let single = run(&mut one, "solid.pattern.rectangular", json!({"bodies": ["Base"], "dir1": [1, 0, 0], "count1": 1, "spacing1": 30}))["feature"]
        .as_u64()
        .unwrap();
    run(&mut one, "timeline.edit", json!({"feature": single, "set": {"pattern.count1": 3}}));
    for copy in ["Base (2)", "Base (3)"] {
        assert_eq!(one.doc.resolved_body_material(copy), Some("Aluminum"), "{copy}");
    }

    // 3) Changing the seed body via timeline.edit copies the new seed body's material and look.
    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"bodies": ["Post"], "pattern.count1": 3}}));
    for copy in ["Post (2)", "Post (3)"] {
        assert_eq!(s.doc.resolved_body_material(copy), Some("Brass"), "{copy}");
        assert_eq!(s.doc.appearances.bodies.get(copy), Some(&red), "{copy}");
    }

    // 4) Switching to an unstyled seed body while keeping explicit copy names clears stale
    // material and appearance entries from the previous seed.
    let mut clear = Session::default();
    run(&mut clear, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Styled"}));
    run(&mut clear, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 40, 5], "operation": "new", "body_name": "Plain"}));
    run(&mut clear, "material.assign", json!({"bodies": ["Styled"], "material": "Steel"}));
    run(&mut clear, "appearance.assign", json!({"bodies": ["Styled"], "color": [40, 120, 200]}));
    let named_pat =
        run(&mut clear, "solid.pattern.rectangular", json!({"bodies": ["Styled"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 30}))["feature"]
            .as_u64()
            .unwrap();
    run(&mut clear, "timeline.edit", json!({"feature": named_pat, "set": {"body_names": ["CopyA", "CopyB"]}}));
    run(&mut clear, "timeline.edit", json!({"feature": named_pat, "set": {"bodies": ["Styled"]}}));
    // Re-seed from Styled -> CopyA/CopyB carry Steel and appearance; now switch to Plain.
    run(&mut clear, "material.assign", json!({"bodies": ["CopyA", "CopyB"], "material": "Steel"}));
    run(&mut clear, "appearance.assign", json!({"bodies": ["CopyA", "CopyB"], "color": [40, 120, 200]}));
    run(&mut clear, "timeline.edit", json!({"feature": named_pat, "set": {"bodies": ["Plain"], "body_names": ["CopyA", "CopyB"]}}));
    for copy in ["CopyA", "CopyB"] {
        assert_eq!(clear.doc.materials.get(copy), None, "{copy}");
        assert_eq!(clear.doc.appearances.bodies.get(copy), None, "{copy}");
    }
}

/// #12199: a path whose curves are listed out of chain order is the same path as in chain order.
#[test]
fn pattern_on_path_orders_the_curves_into_one_chain() {
    // Four lines: (10,10) → (30,10) → (30,30) → (60,30) → (60,10); 90 mm long.
    let copies = |order: [&str; 4]| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [9, 9, 0], "body_name": "Pin"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
        for (id, a, b) in [("l1", [10, 10], [30, 10]), ("l2", [30, 10], [30, 30]), ("l3", [30, 30], [60, 30]), ("l4", [60, 30], [60, 10])] {
            run(&mut s, "sketch.line", json!({"points": [a, b], "connect": false, "ids": [id]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.pattern.path", json!({"bodies": ["Pin"], "path_sketch": "Path", "path": order, "count": 4, "spacing": 25}));
        let m = run(&mut s, "inspect.measure", json!({}));
        let mut at: Vec<(i64, i64)> = m["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| ((b["bbox"]["min"][0].as_f64().unwrap() * 1e3).round() as i64, (b["bbox"]["min"][1].as_f64().unwrap() * 1e3).round() as i64))
            .collect();
        at.sort();
        at
    };
    // The copies sit 0, 25, 50 and 75 mm along the path, as displacements from its start.
    let want = vec![(9_000, 9_000), (29_000, 14_000), (39_000, 29_000), (59_000, 24_000)];
    assert_eq!(copies(["l1", "l2", "l3", "l4"]), want);
    assert_eq!(copies(["l1", "l3", "l2", "l4"]), want);
    assert_eq!(copies(["l4", "l3", "l2", "l1"]), copies(["l4", "l2", "l3", "l1"]));
}

/// #12199: putting the listed curves in chain order does not join curves with a gap between
/// them: such a path is still refused.
#[test]
fn pattern_on_path_with_a_gap_is_still_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [9, 9, 0], "body_name": "Pin"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    // (10,10) → (30,10), then a 20 mm gap, then (30,30) → (60,30).
    for (id, a, b) in [("l1", [10, 10], [30, 10]), ("l3", [30, 30], [60, 30])] {
        run(&mut s, "sketch.line", json!({"points": [a, b], "connect": false, "ids": [id]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    for order in [["l1", "l3"], ["l3", "l1"]] {
        let p = json!({"bodies": ["Pin"], "path_sketch": "Path", "path": order, "count": 3, "spacing": 10});
        let error = s.execute("solid.pattern.path", &p).unwrap_err().to_string();
        assert!(error.contains("not connected end to end"), "{order:?}: {error}");
    }
}

/// #12585: a pattern or mirror of an Offset Face repeats it at the copied faces instead of
/// failing with "cannot be patterned or mirrored yet". A 40 x 30 x 20 box whose top is split into
/// strips at x = 10 and x = 30 has its left strip raised 5; the copy raises the right strip.
#[test]
fn pattern_and_mirror_of_an_offset_face_raise_the_copied_face() {
    for (copy, params) in [
        ("solid.mirror", json!({"features": ["Raise"], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}})),
        ("solid.pattern.rectangular", json!({"features": ["Raise"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30})),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Block"}));
        run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "plane": {"origin": [10, 0, 0], "normal": [1, 0, 0]}}));
        run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "plane": {"origin": [30, 0, 0], "normal": [1, 0, 0]}}));
        run(&mut s, "solid.offset_face", json!({"faces": [[5, 15, 20]], "distance": 5, "name": "Raise"}));
        let strip = 10.0 * 30.0 * 5.0;
        assert!(rel(volume(&mut s), 24000.0 + strip) < 1e-6, "{copy}: the source raises one strip");
        let r = run(&mut s, copy, params);
        assert!(r["error"].is_null(), "{copy}: {r}");
        assert!(rel(volume(&mut s), 24000.0 + 2.0 * strip) < 1e-6, "{copy}: {} vs {}", volume(&mut s), 24000.0 + 2.0 * strip);
    }
}

/// #18048: a pattern or mirror of a Draft drafts the copied face instead of failing with
/// "Draft1 cannot be patterned or mirrored yet". A 20 x 20 x 20 pad joined on a 100 x 40 x 10
/// plate has its side at x = 10 drafted 5 deg about its bottom edge (neutral plane z = 10): the
/// side sweeps a wedge of 0.5 * 20 * (20 tan 5 deg) * 20 = 4000 tan 5 deg. Each copy of the pad
/// is drafted by the same wedge, whether the pad is copied with the draft or was copied before.
#[test]
fn pattern_and_mirror_of_a_draft_draft_the_copied_face() {
    let wedge = 4000.0 * 5.0_f64.to_radians().tan();
    let pattern = json!({"dir1": [1, 0, 0], "count1": 2, "spacing1": 50});
    let with = |features: Value, mut p: Value| {
        p["features"] = features;
        p
    };
    let cases = [
        ("mirror", None, ("solid.mirror", json!({"features": ["Pad1", "Draft1"], "plane": {"origin": [50, 0, 0], "normal": [1, 0, 0]}}))),
        ("pattern", None, ("solid.pattern.rectangular", with(json!(["Pad1", "Draft1"]), pattern.clone()))),
        (
            "pattern of the draft alone",
            Some(with(json!(["Pad1"]), pattern.clone())),
            ("solid.pattern.rectangular", with(json!(["Draft1"]), pattern.clone())),
        ),
    ];
    for (what, before, (copy, params)) in cases {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 100, "width": 40, "height": 10, "body_name": "Plate"}));
        run(&mut s, "solid.box", json!({"corner": [10, 10, 10], "length": 20, "width": 20, "height": 20, "operation": "join", "name": "Pad1"}));
        let pad_copied_before = before.is_some();
        if let Some(p) = before {
            let r = run(&mut s, "solid.pattern.rectangular", p);
            assert!(r["error"].is_null(), "{what}: {r}");
        }
        let pads = volume(&mut s);
        run(
            &mut s,
            "solid.draft",
            json!({"faces": [[10, 20, 20]], "angle": "5 deg", "neutral": {"origin": [0, 0, 10], "normal": [0, 0, 1]}, "name": "Draft1"}),
        );
        let drafted = volume(&mut s) - pads;
        assert!(rel(drafted.abs(), wedge) < 1e-6, "{what}: the source drafts {drafted} mm³, not ±{wedge}");
        let r = run(&mut s, copy, params);
        assert!(r["error"].is_null(), "{what}: {r}");
        // Both pads undrafted: the second pad is either there already or copied now.
        let pads = if pad_copied_before { pads } else { 48000.0 + 8000.0 };
        let want = pads + 2.0 * drafted;
        assert!(rel(volume(&mut s), want) < 1e-6, "{what}: {} vs {want}", volume(&mut s));
    }
}

/// #18048: a pattern of a Draft drafts the copied face where the draft finds it now. The pad's
/// side at x = 30 is drafted 5 deg about its bottom edge and the pad and draft are patterned
/// 50 apart; making the pad 30 long moves that side to x = 40, so the copy's side is at x = 90,
/// not at x = 80 where it was picked. Each pad is 30 x 20 x 20 less the wedge 4000 tan 5 deg.
#[test]
fn pattern_of_a_draft_follows_its_face_after_an_edit() {
    let wedge = 4000.0 * 5.0_f64.to_radians().tan();
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 40, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"corner": [10, 10, 10], "length": 20, "width": 20, "height": 20, "operation": "join", "name": "Pad1"}));
    run(
        &mut s,
        "solid.draft",
        json!({"faces": [[30, 20, 20]], "angle": "5 deg", "neutral": {"origin": [0, 0, 10], "normal": [0, 0, 1]}, "name": "Draft1"}),
    );
    let r = run(&mut s, "solid.pattern.rectangular", json!({"features": ["Pad1", "Draft1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 50}));
    assert!(r["error"].is_null(), "{r}");
    assert!(rel(volume(&mut s), 40000.0 + 2.0 * (8000.0 - wedge)) < 1e-6, "20 long: {}", volume(&mut s));
    let r = run(&mut s, "timeline.edit", json!({"feature": "Pad1", "set": {"length": "30 mm"}}));
    assert!(r["error"].is_null(), "{r}");
    let want = 40000.0 + 2.0 * (12000.0 - wedge);
    assert!(rel(volume(&mut s), want) < 1e-6, "30 long: {} vs {want}", volume(&mut s));
}

/// #13278: a mirror of a Thread repeats it on the mirrored hole, whether the hole was mirrored
/// earlier or is mirrored together with the thread.
#[test]
fn mirror_of_a_thread_threads_the_mirrored_hole() {
    let plane = json!({"origin": [20, 0, 0], "normal": [1, 0, 0]});
    for (together, modeled) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 10, "body_name": "Plate"}));
        run(&mut s, "solid.hole", json!({"position": [10, 15, 10], "diameter": 6.8, "depth": 10, "name": "Hole1"}));
        if !together {
            let r = run(&mut s, "solid.mirror", json!({"features": ["Hole1"], "plane": plane}));
            assert!(r["error"].is_null(), "{r}");
        }
        run(&mut s, "solid.thread", json!({"face": [13.4, 15, 5], "designation": "M8", "modeled": modeled, "name": "Thread1"}));
        let features = if together { json!(["Hole1", "Thread1"]) } else { json!(["Thread1"]) };
        let r = run(&mut s, "solid.mirror", json!({"features": features, "plane": plane}));
        assert!(r["error"].is_null(), "together {together}: {r}");
        let state = s.world_state().unwrap();
        if modeled {
            assert_eq!(state.bodies.len(), 1, "{r}");
            let threaded = volume(&mut s);
            assert!(threaded < 40.0 * 30.0 * 10.0 - 2.0 * std::f64::consts::PI * 3.4 * 3.4 * 10.0 - 1.0, "both holes are threaded: {threaded}");
        } else {
            assert_eq!(state.threads.len(), 2, "together {together}: the source and its mirror");
        }
    }
}

/// #21031: a mirror or pattern of a tapped Hole repeats its own thread on each copy: a modelled
/// thread removes as much from the copy as from the original, and a cosmetic one is listed again.
#[test]
fn copies_of_a_tapped_hole_are_threaded() {
    let copies = [
        ("solid.mirror", json!({"features": ["Hole1"], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}})),
        ("solid.pattern.rectangular", json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20})),
    ];
    for modeled in [true, false] {
        for (copy, params) in &copies {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 12, "body_name": "Block"}));
            let block = volume(&mut s);
            run(&mut s, "solid.hole", json!({"position": [10, 15, 12], "thread": "M4", "depth": 8, "modeled": modeled, "name": "Hole1"}));
            let one = block - volume(&mut s);
            let r = run(&mut s, copy, params.clone());
            assert!(r["error"].is_null(), "{copy}: {r}");
            if modeled {
                let both = block - volume(&mut s);
                assert!(rel(both, 2.0 * one) < 1e-6, "{copy}: the original removes {one} mm³, both {both} mm³");
            } else {
                assert_eq!(s.world_state().unwrap().threads.len(), 2, "{copy}: the hole's thread and its copy's");
            }
        }
        // A copy off the part removes no material: it warns, and only the copies that cut are threaded.
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 12, "body_name": "Block"}));
        let block = volume(&mut s);
        run(&mut s, "solid.hole", json!({"position": [10, 15, 12], "thread": "M4", "depth": 8, "modeled": modeled, "name": "Hole1"}));
        let one = block - volume(&mut s);
        let params = json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20});
        let r = run(&mut s, "solid.pattern.rectangular", params);
        assert!(r["error"].is_null(), "{r}");
        assert!(r.to_string().contains("1 of 2 cut copies remove no material"), "{r}");
        if modeled {
            let all = block - volume(&mut s);
            assert!(rel(all, 2.0 * one) < 1e-6, "the original removes {one} mm³, all {all} mm³");
        } else {
            assert_eq!(s.world_state().unwrap().threads.len(), 2, "the hole's thread and its one cutting copy's");
        }
    }
    // A copy over a 3 mm thick step has a wall shorter than the thread depth: it is drilled, left
    // unthreaded with a warning, and the pattern still evaluates.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 12, "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 30, "height": 9, "corner": [20, 0, 3], "operation": "cut"}));
    let hole = json!({"position": [10, 15, 12], "thread": "M4", "depth": 10, "thread_depth": 6, "modeled": false, "name": "Hole1"});
    run(&mut s, "solid.hole", hole);
    let r = run(&mut s, "solid.pattern.rectangular", json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20}));
    assert!(r["error"].is_null(), "{r}");
    assert!(r.to_string().contains("1 hole copies are not threaded"), "{r}");
    assert_eq!(s.world_state().unwrap().threads.len(), 1, "only the original hole's thread");
}

/// #16799: two holes placed from edges, mirrored together. The second hole's edge names mean the
/// edges of the original body, which the first hole's copy has already cut into.
#[test]
fn mirror_of_two_holes_placed_from_edges_makes_both_copies() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.center", json!({"center": [0, 0], "corner": [60, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "S", "distance": 10, "operation": "new", "name": "E", "body_name": "Body1"}));
    let plain = volume(&mut s);
    for (name, x, from) in [("Hole1", -40, "20"), ("Hole2", -20, "40")] {
        let references = json!([{"edge": [-60, -30, 5], "distance": from}, {"edge": [0, -30, 0], "distance": "5"}]);
        run(&mut s, "solid.hole", json!({"position": [x, -30, 5], "diameter": 4, "references": references, "name": name}));
    }
    let holed = volume(&mut s);
    run(&mut s, "solid.mirror", json!({"features": ["Hole1", "Hole2"], "plane": "YZ"}));
    let both = volume(&mut s);
    let want = plain - 2.0 * (plain - holed);
    assert!(rel(both, want) < 1e-6, "plain {plain} mm³, two holes {holed} mm³, mirrored {both} mm³ (expected {want})");
}

/// #16912: the mirror or pattern copy of a curvature or spline fillet is the same fillet, not a
/// constant-radius round: its face is the original's kind of surface (no cylinder), so the result
/// has twice the original's blend faces of that kind.
#[test]
fn copies_of_a_curvature_or_spline_fillet_keep_its_type() {
    let face_types = |s: &mut Session| run(s, "inspect.measure", json!({}))["bodies"][0]["face_types"].clone();
    let fillets = [
        json!({"body": "Box", "edges": [[30, 0, 30]], "radius": 8, "type": "curvature", "name": "Round"}),
        json!({"body": "Box", "edges": [[30, 0, 30]], "radius": 12, "type": "spline", "start": [0, 0, 30], "radii": ["3"], "knots": [0, 0, 1, 1], "name": "Round"}),
    ];
    let copies = [
        ("solid.mirror", json!({"features": ["Round"], "plane": {"origin": [0, 20, 0], "normal": [0, 1, 0]}})),
        ("solid.pattern.rectangular", json!({"features": ["Round"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 40})),
    ];
    for fillet in &fillets {
        for (id, p) in &copies {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 30, "body_name": "Box"}));
            run(&mut s, "solid.fillet", fillet.clone());
            let one = face_types(&mut s);
            let other = one["other"].as_u64().unwrap_or(0);
            assert!(other > 0 && one.get("cylinder").is_none(), "{fillet}: {one}");
            run(&mut s, id, p.clone());
            let both = face_types(&mut s);
            assert!(both.get("cylinder").is_none(), "{id} of {fillet}: {both}");
            assert_eq!(both["other"].as_u64(), Some(2 * other), "{id} of {fillet}: {one} then {both}");
        }
    }
}

/// #16934: a circular pattern of a groove and the fillet that rounds its edges. The fillet's pick
/// on the groove's top rim sits at the vertex where the groove's seam splits the rim in two, as
/// near to one half as to the other: copies made from the pick rounded one half or the other as
/// rounding fell, so every other copy kept half its rim sharp. Every copy is the original turned.
#[test]
fn circular_pattern_of_a_groove_and_its_fillet_rounds_every_copy_alike() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "side", "expression": "10 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plan"}));
    run(
        &mut s,
        "sketch.draw.dimensioned",
        json!({"command": "sketch.polygon.inscribed", "params": {"center": [0, 0], "radius": 16.18, "sides": 10, "angle": 0},
               "dimensions": {"radius": "side / (2 * sin(18 deg))"}}),
    );
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["origin", "p1"], "mode": "horizontal"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "Plan", "distance": 10, "name": "Prism", "body_name": "Node"}));
    run(&mut s, "solid.extrude", json!({"face": [0, 0, 10], "distance": 6, "taper": "-45 deg", "operation": "join", "name": "Top"}));
    let plain = run(&mut s, "inspect.measure", json!({}))["bodies"][0].clone();
    run(&mut s, "sketch.create", json!({"plane": "YZ", "name": "GrooveSketch"}));
    let corners = json!([[0, 28.92395], [-31.36774, -2.44379], [-33.48906, -0.32247], [-2.12132, 31.04527]]);
    run(&mut s, "sketch.line", json!({"closed": true, "points": corners}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch": "GrooveSketch", "axis": "l1", "operation": "cut", "targets": ["Node"], "name": "Groove"}));
    let edges = json!([[1.6583, -12.388, 13], [-1.6583, -12.388, 13], [0, -8.682, 16], [0, -15.388, 9.294]]);
    run(&mut s, "solid.fillet", json!({"edges": edges, "radius": 1, "name": "GrooveRounds"}));
    let one = run(&mut s, "inspect.measure", json!({}))["bodies"][0].clone();
    let r = run(&mut s, "solid.pattern.circular", json!({"features": ["Groove", "GrooveRounds"], "axis": "Z", "count": 10, "name": "Grooves"}));
    assert!(r["error"].is_null(), "{r}");
    let all = run(&mut s, "inspect.measure", json!({}))["bodies"][0].clone();
    // Each copy adds the faces the groove and its rounds added, and takes the material they took.
    let count = |m: &Value, kind: &str| m["face_types"][kind].as_u64().unwrap_or(0);
    for kind in ["cylinder", "other", "plane"] {
        let want = count(&plain, kind) + 10 * (count(&one, kind) - count(&plain, kind));
        assert_eq!(count(&all, kind), want, "{kind} faces: {all}");
    }
    let v = |m: &Value| m["volume_mm3"].as_f64().unwrap_or(f64::NAN);
    let want = v(&plain) - 10.0 * (v(&plain) - v(&one));
    assert!(rel(v(&all), want) < 1e-6, "ten rounded grooves: {} mm³, expected {want} mm³", v(&all));
}

/// #20931: a ring revolved about X from a section with a 0.2 mm corner arc, a rib revolved 7°
/// across that corner and joined, a 0.75 mm round on the two edges where the rib's end meets the
/// ring, and a circular pattern of the rib and the round. Every copy is the original turned a
/// quarter, apart from the others, so from the second rib on each one splits the ring's faces as
/// the second did: four ribs have the faces of two plus twice what the second added. At count 4
/// SMLib's check refused the round of the last copy (Bad SpeedPt, Bad CurveCusp on a 2D curve):
/// the pattern joins its three rib copies in one union, and the round at the last of them fails
/// there, while the same copies joined one at a time round.
#[test]
fn circular_pattern_of_a_rib_and_its_round_on_a_ring_builds_at_four_copies() {
    let script: Value = serde_json::from_str(include_str!("../../../../examples/regressions/ring_rib_round_circular_pattern.json")).unwrap();
    let build = |count: u32| {
        let mut script = script.clone();
        for c in script["commands"].as_array_mut().unwrap() {
            if c["params"]["feature"]["type"] == "pattern" {
                c["params"]["feature"]["pattern"]["count"] = json!(count.to_string());
            }
        }
        let mut s = Session::default();
        s.run_script(&script).unwrap();
        let errors: Vec<_> = s.model.results.iter().filter_map(|r| r.error.clone()).collect();
        assert!(errors.is_empty(), "count {count}: {errors:?}");
        let st = s.model.state();
        let ring = st.body("Ring").unwrap();
        (ring.body.face_count(), ring.body.lumps().expect("the ring body reports its lumps").len())
    };
    let (one, _) = build(1);
    let (two, _) = build(2);
    let (four, pieces) = build(4);
    assert_eq!(pieces, 1, "the four ribs are joined to the ring");
    assert_eq!(four, two + 2 * (two - one), "faces: {one} with one rib, {two} with two, {four} with four");
}

/// Copies of a body made by a pattern are drawn from the body's own triangles and display plan,
/// moved, and keep doing so when they are moved into a component: a 30 by 30 pattern of a
/// filleted cylinder meshed and planned every copy, twice, and took over a minute to show.
#[test]
fn pattern_copies_share_their_bodys_meshes_through_a_move_to_a_component() {
    use solidcraft_doc::display_quality::DisplayQuality;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 3, "height": 6}));
    run(&mut s, "solid.fillet", json!({"edges": [[3, 0, 6]], "radius": 1}));
    let r = run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"bodies": ["Body1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 10, "dir2": [0, 1, 0], "count2": 2, "spacing2": 10}),
    );
    let copies: Vec<String> = r["bodies"].as_array().unwrap().iter().filter_map(Value::as_str).filter(|b| *b != "Body1").map(String::from).collect();
    assert_eq!(copies.len(), 5, "{r}");
    let q = DisplayQuality::parse("normal").unwrap();
    let check = |s: &Session, when: &str| {
        let st = s.model.state();
        let source = st.body("Body1").unwrap();
        let (own, at) = source.mesh_in_place();
        assert_eq!(at, solidcraft_doc::IDENTITY);
        let own_plan = source.plan_mesh_at(q).unwrap();
        for name in &copies {
            let copy = st.body(name).unwrap();
            let (mesh, m) = copy.mesh_in_place();
            assert!(Arc::ptr_eq(&mesh, &own), "{when}: `{name}` is drawn from its own triangles");
            assert!(Arc::ptr_eq(&copy.plan_mesh_in_place_at(q).unwrap().0, &own_plan), "{when}: `{name}` is planned again");
            // The moved triangles lie where the copy is.
            let (bounds, shown) = (copy.body.exact_bounds().unwrap(), copy.mesh().bounds());
            assert!(bounds.min.dist(shown.min) < 0.05 && bounds.max.dist(shown.max) < 0.05, "{when}: `{name}` {bounds:?} vs {shown:?}");
            assert_ne!(m, solidcraft_doc::IDENTITY);
        }
    };
    check(&s, "patterned");
    run(&mut s, "component.create", json!({"name": "Grid", "parent": "root", "activate": false}));
    run(&mut s, "component.move_bodies", json!({"bodies": copies, "component": "Grid"}));
    check(&s, "moved into a component");
}

/// A pattern of one placement of a part placed twice (`occurrences`) places that placement again,
/// moved by the pattern, beside itself; the first placement is not what it copies.
#[test]
fn a_pattern_of_a_placement_places_that_placement_again() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Bracket"}));
    let component = c["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 4, "body_name": "Base"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let second = run(&mut s, "occurrence.copy", json!({"component": component, "translate": [50, 0, 0]}))["occurrence"].as_u64().unwrap();
    let r = run(&mut s, "solid.pattern.rectangular", json!({"occurrences": [[second]], "dir1": [0, 1, 0], "count1": 3, "spacing1": 40}));
    let made: Vec<u64> = r["occurrences"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    assert_eq!(made.len(), 2, "{r}");
    for (k, id) in made.iter().enumerate() {
        let o = s.doc.occurrences.iter().find(|o| o.id == *id).unwrap();
        assert_eq!((o.component, o.parent), (component, 0));
        let at = [o.transform[3][0], o.transform[3][1], o.transform[3][2]];
        let want = [50.0, 40.0 * (k as f64 + 1.0), 0.0];
        assert!(at.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9), "{at:?}, want {want:?}");
    }
    // No body is copied.
    assert_eq!(s.model.state().bodies.len(), 1);
    // `occurrences` that is not a list is refused, not read as none (which would copy the bodies).
    for occurrences in [json!("7"), json!(7), json!({"path": [second]})] {
        let p = json!({"occurrences": occurrences, "bodies": ["Base"], "dir1": [0, 1, 0], "count1": 3, "spacing1": 40});
        assert!(s.execute("solid.pattern.rectangular", &p).is_err(), "{p}");
    }
    assert_eq!(s.model.state().bodies.len(), 1);
    // `null` names none.
    let p = json!({"occurrences": null, "components": ["Bracket"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 40});
    assert!(s.execute("solid.pattern.rectangular", &p).is_ok(), "{p}");
    // A placement shown moved and not captured yet is refused, as a body picked there is: the
    // pattern goes by where the design places it, not where it is shown.
    let mut moved = solidcraft_doc::IDENTITY;
    moved[3] = [0.0, 0.0, 25.0, 1.0];
    s.pending_moves.insert(second, moved);
    let count = s.doc.occurrences.len();
    let p = json!({"occurrences": [[second]], "dir1": [0, 1, 0], "count1": 2, "spacing1": 40});
    let error = s.execute("solid.pattern.rectangular", &p).unwrap_err().to_string();
    assert!(error.contains("not captured yet"), "{error}");
    assert_eq!(s.doc.occurrences.len(), count, "nothing placed");
}

/// The box of `body` as the world shows it.
fn world_bounds(s: &Session, body: &str) -> (solidcore_record::Vec3, solidcore_record::Vec3) {
    let b = s.world_state().unwrap().body(body).unwrap_or_else(|| panic!("no body `{body}`")).mesh().bounds();
    (b.min, b.max)
}

/// A part "Arm" holding "ArmBody", placed twice (turned and moved, then a further instance
/// above it), with the design itself active: the further instance's world name.
fn arm_placed_twice(s: &mut Session) -> String {
    let c = run(s, "component.create", json!({"name": "Arm", "grounded": false}));
    run(s, "solid.box", json!({"corner": [10, 0, 0], "length": 20, "width": 4, "height": 6, "body_name": "ArmBody"}));
    run(s, "component.activate", json!({"component": 0}));
    run(s, "occurrence.move", json!({"occurrence": c["occurrence"], "axis": [1, 0, 0], "angle": "90 deg", "translate": [30, 50, 0]}));
    run(s, "occurrence.copy", json!({"component": c["component"], "translate": [0, 0, 40]}));
    let names: Vec<String> = s.world_state().unwrap().bodies.iter().map(|b| b.name.clone()).collect();
    names.into_iter().find(|n| n.starts_with("ArmBody") && n != "ArmBody").unwrap_or_else(|| panic!("no further instance"))
}

/// A body pattern of every body an assembly shows, picked as the Browser names them: a body of a
/// part placed twice is copied where each placement shows it. Before, the world name of the
/// second placement was "no body", and the whole pattern was refused.
#[test]
fn a_pattern_copies_each_placement_of_a_part_where_it_is_shown() {
    let mut s = Session::default();
    let instance = arm_placed_twice(&mut s);
    let r = run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["ArmBody", instance], "dir1": [1, 0, 0], "count1": 2, "spacing1": 100}));
    let pattern = s.doc.features.last().unwrap().id;
    let copies: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == pattern).map(|b| b.name.clone()).collect();
    assert_eq!(copies.len(), 2, "{r}");
    let shifted = |(lo, hi): (solidcore_record::Vec3, solidcore_record::Vec3)| {
        let d = solidcore_record::Vec3::new(100.0, 0.0, 0.0);
        (lo + d, hi + d)
    };
    let close = |a: (solidcore_record::Vec3, solidcore_record::Vec3), b: (solidcore_record::Vec3, solidcore_record::Vec3)| {
        a.0.dist(b.0) < 1e-6 && a.1.dist(b.1) < 1e-6
    };
    let wanted = [shifted(world_bounds(&s, "ArmBody")), shifted(world_bounds(&s, &instance))];
    for (copy, want) in copies.iter().zip(wanted) {
        assert!(close(world_bounds(&s, copy), want), "{copy}: {:?}, want {want:?}", world_bounds(&s, copy));
    }
    // It reads back the same from a saved design.
    let t = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for (copy, want) in copies.iter().zip(wanted) {
        assert!(close(world_bounds(&t, copy), want), "{copy} after reopening");
    }
}

/// A body the pattern cannot copy any more (here: the placement it was picked at is gone) is left
/// out with a warning, and every other body is still copied. Before, one such body failed the
/// whole pattern.
#[test]
fn a_pattern_leaves_out_a_body_it_cannot_copy_and_copies_the_rest() {
    let mut s = Session::default();
    let instance = arm_placed_twice(&mut s);
    run(&mut s, "solid.box", json!({"corner": [0, -50, 0], "length": 5, "width": 5, "height": 5, "body_name": "Block"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["ArmBody", instance, "Block"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 100}));
    let pattern = s.doc.features.last().unwrap().id;
    if let Some(f) = Arc::make_mut(&mut s.doc).feature_mut(pattern)
        && let solidcraft_doc::FeatureKind::Pattern { placements, .. } = &mut f.kind
    {
        assert_eq!(placements.len(), 3, "{placements:?}");
        // The further instance's placement, as if its occurrence were deleted since.
        if let Some(path) = placements.get_mut(1) {
            *path = vec![u64::MAX];
        }
    }
    s.refresh();
    let result = s.model.result(pattern).unwrap();
    assert!(result.error.is_none(), "{:?}", result.error);
    let warning = result.warning.clone().unwrap_or_default();
    assert!(warning.contains("1 of the bodies could not be copied") && warning.contains("ArmBody"), "{warning}");
    let copies = s.model.state().bodies.iter().filter(|b| b.feature == pattern).count();
    assert_eq!(copies, 4, "two copies each of ArmBody and Block");
}

/// While a pattern leaves a body out, the names stored for that body's copies stay in their
/// places: once the body is back, every copy has the name it had before. Before, the names of
/// the copies made were stored one after another, so the body's copies took another body's
/// names when it came back, and that body's copies had new ones.
#[test]
fn a_pattern_keeps_the_names_of_a_left_out_bodys_copies() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 5, "width": 5, "height": 5, "body_name": "A"}));
    let a = s.doc.features.last().unwrap().id;
    run(&mut s, "solid.box", json!({"corner": [0, 50, 0], "length": 5, "width": 5, "height": 5, "body_name": "B"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["A", "B"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20}));
    let pattern = s.doc.features.last().unwrap().id;
    // Each copy's name with where it is (its box's lowest corner, rounded).
    let copies = |s: &Session| -> Vec<(String, (i64, i64))> {
        let mut copies: Vec<(String, (i64, i64))> = s
            .model
            .state()
            .bodies
            .iter()
            .filter(|b| b.feature == pattern)
            .map(|b| {
                let lo = b.mesh().bounds().min;
                (b.name.clone(), (lo.x.round() as i64, lo.y.round() as i64))
            })
            .collect();
        copies.sort();
        copies
    };
    let before = copies(&s);
    assert_eq!(before.len(), 4, "{before:?}");
    run(&mut s, "timeline.suppress", json!({"feature": a, "suppressed": true}));
    let result = s.model.result(pattern).unwrap();
    assert!(result.error.is_none() && result.warning.as_deref().is_some_and(|w| w.contains("A")), "{result:?}");
    let b_copies: Vec<_> = before.iter().filter(|(_, (_, y))| *y == 50).cloned().collect();
    assert_eq!(copies(&s), b_copies, "B's copies keep their names while A is left out");
    run(&mut s, "timeline.suppress", json!({"feature": a, "suppressed": false}));
    assert_eq!(copies(&s), before, "every copy has its name back");
}

/// Placing a part many times moves none of its bodies' records: the placed world keeps each
/// part's record with each placement's motion, and a placement's record is moved only when its
/// own exact geometry is needed. Every placement of a large assembly placed a hundred times was
/// moved as it was placed, tens of gigabytes.
#[test]
fn placing_a_part_many_times_moves_no_record_until_one_is_needed() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    for k in 0..3 {
        run(&mut s, "solid.cylinder", json!({"base": [12 * k, 0, 0], "radius": 5, "height": 20, "body_name": format!("B{k}")}));
    }
    run(&mut s, "component.activate", json!({"component": 0}));
    for k in 1..=10 {
        run(&mut s, "occurrence.copy", json!({"component": c["component"], "translate": [0, 40 * k, 0]}));
    }
    let world = s.world_state().unwrap();
    assert_eq!(world.bodies.len(), 3 * 11);
    // Their shapes are drawn, named and framed from the part's: meshes, face counts, outlines,
    // whether they are closed (the viewport asks every frame it builds).
    for b in &world.bodies {
        let _ = (b.body.face_count(), b.mesh_in_place(), b.outline_bounds(), b.is_closed());
    }
    assert!(world.bodies.iter().all(|b| !b.body.lazy_move_made()), "no placement's record moved to place or draw it");
    // A measure of one placement needs its own geometry: that one is moved, once.
    let placed = world.bodies.iter().find(|b| b.placed_definition().is_some()).unwrap();
    let _ = placed.body.record();
    assert!(placed.body.lazy_move_made());
}

/// Review of #18732: a per-edge fillet whose picks include a smooth edge (dropped, with a note)
/// copies each remaining edge at its own radius. A 60 x 40 x 30 block, its top front edge rounded
/// r 5 first; then a per-edge fillet of that round's lower tangent edge (smooth, dropped) at 1 and
/// the bottom front edge at 2, mirrored onto the bottom back edge. Each round of radius r on a
/// 90 deg edge of length L removes (1 - pi/4) r^2 L: 72000 - 60 (1 - pi/4) (25 + 4 + 4). Copying
/// each edge at its position among the picks gave the copy the dropped pick's radius, 1.
#[test]
fn a_mirror_of_a_per_edge_fillet_with_a_dropped_smooth_pick_keeps_each_edges_radius() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 40, "height": 30, "body_name": "Box"}));
    run(&mut s, "solid.fillet", json!({"body": "Box", "edges": [[30, 0, 30]], "radius": 5}));
    run(
        &mut s,
        "solid.fillet",
        json!({"body": "Box", "edges": [[30, 0, 25], [30, 0, 0]], "radius": 1, "edge_radii": [1, 2], "tangent_chain": false, "name": "Each"}),
    );
    run(&mut s, "solid.mirror", json!({"features": ["Each"], "plane": {"origin": [0, 20, 0], "normal": [0, 1, 0]}}));
    let v = volume(&mut s);
    assert!((v - 71575.08836352694).abs() < 1e-6 * v, "{v}");
}

// The test below and its helpers are kaievns's, from their branch for #19451 (review of #20119):
// a root mirror of a mirror made in a placed part, which main gets right and a first version of
// #20119 broke.

/// A body's volume, its face count and the ends of its pockets (planar faces square to Z under
/// 1000 mm²) as (area, centroid) where it is shown, in x order.
fn pockets(s: &mut Session, body: &str) -> (f64, usize, Vec<(f64, [f64; 3])>) {
    let v = s.model.state().body(body).map_or(f64::NAN, |b| b.measure().unwrap().volume);
    let r = run(s, "model.faces", json!({"body": body}));
    let faces = r["faces"].as_array().unwrap();
    let mut ends: Vec<(f64, [f64; 3])> = faces
        .iter()
        .filter(|f| f["plane_normal"][2].as_f64().is_some_and(|z| z.abs() > 0.999) && f["area"].as_f64().is_some_and(|a| a < 1000.0))
        .map(|f| (f["area"].as_f64().unwrap(), std::array::from_fn(|i| f["centroid"][i].as_f64().unwrap())))
        .collect();
    ends.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    (v, faces.len(), ends)
}

/// What a 2 mm round takes off an upright 10 mm edge of a box.
const ROUND: f64 = (4.0 - PI) * 10.0;

/// Part, placed turned 180° about Z and lifted 20, and in it Base, 40 × 40 × 10 at x −10..30,
/// y −20..20, z 0..10, with Boss, a join of x 20..26, y 5..10 on its top, 5 high, and Round, a 2 mm
/// round of its upright edge at x 30, y 20. Where it is shown, Base is at x −30..10, y −20..20,
/// z 20..30.
fn part_with_a_boss_and_a_round() -> Session {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10, "corner": [-10, -20, 0], "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "BossSketch"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 5], "p1": [26, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5, "operation": "join", "targets": ["Base"], "name": "Boss"}));
    run(&mut s, "solid.fillet", json!({"edges": [[30, 20, 5]], "radius": 2, "name": "Round"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let turn = json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "180 deg", "origin": [0, 0, 0], "translate": [0, 0, 20]});
    run(&mut s, "occurrence.move", turn);
    run(&mut s, "component.capture_position", json!({}));
    s
}

/// The (x, y) of the axes of `body`'s rounds of `radius` where they are shown, in x then y order
/// (to the micrometre).
fn round_axes(s: &mut Session, body: &str, radius: f64) -> Vec<[f64; 2]> {
    let r = run(s, "model.faces", json!({"body": body}));
    let mut axes: Vec<[f64; 2]> = r["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["cylinder"]["radius"].as_f64().is_some_and(|r| (r - radius).abs() < 1e-9))
        .map(|f| [f["cylinder"]["origin"][0].as_f64().unwrap(), f["cylinder"]["origin"][1].as_f64().unwrap()])
        .collect();
    axes.sort_by_key(micrometres);
    axes
}

/// (x, y) to the micrometre, to sort points that rounding leaves a hair apart.
fn micrometres(p: &[f64; 2]) -> (i64, i64) {
    ((p[0] * 1e3).round() as i64, (p[1] * 1e3).round() as i64)
}

fn near2(p: [f64; 2], q: [f64; 2]) -> bool {
    (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6
}

/// The centroids (x, y) of the ends of `body`'s pockets at height `z` ([`pockets`]), in x then
/// y order.
fn pocket_ends_at(s: &mut Session, body: &str, z: f64) -> Vec<[f64; 2]> {
    let mut at: Vec<[f64; 2]> = pockets(s, body).2.iter().filter(|(_, c)| (c[2] - z).abs() < 1e-6).map(|(_, c)| [c[0], c[1]]).collect();
    at.sort_by_key(micrometres);
    at
}

/// #19451 review: a root mirror across XZ of a mirror made in the part copies that mirror's copies
/// at their world images, as main does: the inner mirror's copies are placed through the part's
/// frame already. A third boss at x 0..6, y 5..10, its top at x 3, y 7.5, and a third round about
/// x 8, y 18.
#[test]
fn a_root_mirror_of_a_mirror_in_a_turned_part_copies_its_copies_at_their_world_images() {
    let mut s = part_with_a_boss_and_a_round();
    run(&mut s, "component.activate", json!({"component": "Part"}));
    let plane = json!({"origin": [-10, 0, 0], "normal": [1, 0, 0]});
    run(&mut s, "solid.mirror", json!({"features": ["Boss", "Round"], "plane": plane, "name": "PartMirror"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = run(&mut s, "solid.mirror", json!({"features": ["PartMirror"], "plane": "XZ"}));
    assert!(r["warning"].is_null(), "{r}");
    let v = pockets(&mut s, "Base").0;
    let tops = pocket_ends_at(&mut s, "Base", 35.0);
    let axes = round_axes(&mut s, "Base", 2.0);
    assert!(rel(v, 16000.0 + 3.0 * 150.0 - 3.0 * ROUND) < 1e-9, "{v}");
    let want = [[-23.0, -7.5], [3.0, -7.5], [3.0, 7.5]];
    assert!(tops.len() == 3 && tops.iter().zip(want).all(|(a, w)| near2(*a, w)), "{tops:?}");
    let want = [[-28.0, -18.0], [8.0, -18.0], [8.0, 18.0]];
    assert!(axes.len() == 3 && axes.iter().zip(want).all(|(a, w)| near2(*a, w)), "{axes:?}");
}

/// #14407: copies of a split pipe surface (2 faces, 6 edges) showed 1 face and 2 edges, their split
/// lines dropped by the move.
#[test]
fn a_body_pattern_of_a_split_surface_copies_its_split_faces() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Round"}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 0], "radius": 12.5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.extrude", json!({"sketch": "Round", "distance": "20 mm", "name": "Pipe", "body_name": "Pipe"}));
    run(&mut s, "solid.split_face", json!({"body": "Pipe", "faces": [[42.5, 0, 10]], "plane": {"origin": [30, 0, 0], "normal": [1, 0, 0]}}));
    run(&mut s, "solid.pattern.circular", json!({"bodies": ["Pipe"], "axis": "Z", "count": 3}));
    let st = s.model.state();
    let shown: Vec<(String, usize, usize)> = st
        .bodies
        .iter()
        .map(|b| {
            let t = solidcraft_kernel::merged_topology(&b.body).unwrap();
            (b.name.clone(), t.faces, t.edges)
        })
        .collect();
    assert_eq!(shown.len(), 3, "{shown:?}");
    assert!(shown.iter().all(|(_, faces, edges)| (*faces, *edges) == (2, 6)), "{shown:?}");
}

/// The bodies of the design, by name.
fn body_names(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

/// A gear made of one sector (#21625): with Join, a circular pattern of a body that crosses the
/// axis is one body at any count, and the count can change without a Combine to keep up. Without
/// it each copy is a body of its own, as before. Copies that touch nothing stay apart; bodies the
/// pattern doesn't copy are left alone; and the choice is saved with the design.
#[test]
fn a_patterns_join_makes_one_body_of_the_copies_that_touch() {
    let sector = |s: &mut Session| {
        run(s, "parameters.add", json!({"name": "teeth", "expression": "6", "unit": "none"}));
        run(s, "solid.box", json!({"corner": [0, -2, 0], "length": 20, "width": 4, "height": 5, "body_name": "Tooth", "name": "ToothBox"}));
        run(s, "solid.box", json!({"corner": [50, 50, 0], "length": 5, "width": 5, "height": 5, "body_name": "Other"}));
    };
    let one = {
        let mut s = Session::default();
        sector(&mut s);
        volume(&mut s)
    };
    // Off, as before: a body per copy.
    let mut s = Session::default();
    sector(&mut s);
    run(&mut s, "solid.pattern.circular", json!({"bodies": ["Tooth"], "axis": "Z", "count": "teeth"}));
    assert_eq!(body_names(&s).len(), 7, "{:?}", body_names(&s));
    // On: the six overlapping teeth are one body, under the source's name; Other is left alone.
    for (what, from) in [("bodies", json!({"bodies": ["Tooth"]})), ("features", json!({"features": ["ToothBox"]}))] {
        let mut s = Session::default();
        sector(&mut s);
        let mut p = json!({"axis": "Z", "count": "teeth", "join": true, "name": "Teeth"});
        if let (Some(p), Some(from)) = (p.as_object_mut(), from.as_object()) {
            p.extend(from.clone());
        }
        run(&mut s, "solid.pattern.circular", p);
        assert_eq!(body_names(&s), ["Tooth", "Other"], "{what}");
        let joined = volume(&mut s);
        assert!(joined > one && joined < 7.0 * one - 1.0, "{what}: the teeth overlap at the axis: {joined} vs {one} each");
        // More teeth: still one body, with no Combine to update.
        run(&mut s, "parameters.change", json!({"name": "teeth", "expression": "9"}));
        assert_eq!(body_names(&s), ["Tooth", "Other"], "{what}: nine teeth");
        // Saved and opened again.
        let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        let mut reopened = Session::new(doc);
        reopened.refresh();
        assert_eq!(body_names(&reopened), ["Tooth", "Other"], "{what}: reopened");
    }
    // Turned on for a pattern made without it.
    let mut s = Session::default();
    sector(&mut s);
    run(&mut s, "solid.pattern.circular", json!({"bodies": ["Tooth"], "axis": "Z", "count": "teeth", "name": "Teeth"}));
    run(&mut s, "timeline.edit", json!({"feature": "Teeth", "set": {"join": true}}));
    assert_eq!(body_names(&s), ["Tooth", "Other"], "joined by an edit");
    // Teeth that don't reach the axis don't touch: each stays a body of its own.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [10, -2, 0], "length": 10, "width": 4, "height": 5, "body_name": "Tooth"}));
    run(&mut s, "solid.pattern.circular", json!({"bodies": ["Tooth"], "axis": "Z", "count": 4, "join": true}));
    assert_eq!(body_names(&s).len(), 4, "{:?}", body_names(&s));
    // A rectangular pattern of overlapping slabs joins too.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 4, "height": 2, "body_name": "Slab"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Slab"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 8, "join": true}));
    assert_eq!(body_names(&s), ["Slab"]);
    // `join` must be a bool.
    let error = s.execute("solid.pattern.rectangular", &json!({"bodies": ["Slab"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 10, "join": "yes"}));
    assert!(error.is_err(), "{error:?}");
}

/// Regression (#21548): deleting sibling occurrences of a component whose body was patterned and
/// then mirrored must not break the mirror when `mirror_copy_frame` migrates the pattern copy
/// between the root and the now-single-placed component (and adding a sibling occurrence back
/// must not break it in the reverse direction either).
#[test]
fn deleting_or_adding_sibling_occurrences_keeps_mirror_of_pattern_copy_working() {
    let dir = std::env::temp_dir().join(format!("solidcraft-mirror-pattern-occ-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let plate_path = dir.join("plate.sc");

    let mut plate = Session::default();
    run(&mut plate, "solid.box", json!({"length": 10, "width": 10, "height": 2, "center": [0, 0, 1], "body_name": "P"}));
    run(&mut plate, "file.save", json!({"path": plate_path.to_string_lossy()}));

    // 1) Start with 3 occurrences of `P`, pattern body on occurrence #3, mirror both, then delete
    // the two unrelated sibling occurrences (count 3 -> 1) and verify dry_run + compute_all + geometry.
    let mut s = Session::default();
    let o1 = run(&mut s, "component.insert_design", json!({"path": plate_path.to_string_lossy()}))["occurrence"].as_u64().unwrap();
    let o2 =
        run(&mut s, "component.insert_design", json!({"path": plate_path.to_string_lossy(), "reuse": true, "translate": [20, 0, 0]}))["occurrence"]
            .as_u64()
            .unwrap();
    let o3 =
        run(&mut s, "component.insert_design", json!({"path": plate_path.to_string_lossy(), "reuse": true, "translate": [40, 0, 0]}))["occurrence"]
            .as_u64()
            .unwrap();

    let frames = s.doc.body_frames_with_source(&s.model.state());
    let third = frames.iter().find(|(_, _, _, path, _)| path == &vec![o3]).map(|(w, ..)| w.clone()).expect("third occurrence body");
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": [third], "dir1": [0, 1, 0], "count1": 2, "spacing1": 20, "name": "REP"}));
    let copy_name = s
        .model
        .state()
        .bodies
        .iter()
        .find(|b| s.doc.feature(b.feature).is_some_and(|f| f.name == "REP"))
        .map(|b| b.name.clone())
        .expect("pattern copy");
    run(&mut s, "solid.mirror", json!({"bodies": [third, copy_name], "plane": {"origin": [0, 0, 10], "normal": [0, 0, 1]}, "name": "SYM"}));

    let sym_bboxes = |s: &Session| -> Vec<([f64; 3], [f64; 3])> {
        let st = s.model.state();
        let mut out: Vec<_> = s
            .doc
            .body_frames_with_source(&st)
            .into_iter()
            .filter_map(|(_, own, _, _, m)| {
                let mb = st.body(&own)?;
                if !s.doc.feature(mb.feature).is_some_and(|f| f.name == "SYM") {
                    return None;
                }
                let placed = solidcraft_kernel::transform_matrix(&mb.body, m).ok()?;
                let bb = solidcraft_kernel::measure(&placed).ok()?.bbox;
                Some(([bb.min.x, bb.min.y, bb.min.z], [bb.max.x, bb.max.y, bb.max.z]))
            })
            .collect();
        out.sort_by(|a, b| a.0[1].total_cmp(&b.0[1]).then(a.0[0].total_cmp(&b.0[0])));
        out
    };
    let before = sym_bboxes(&s);
    assert_eq!(before.len(), 2, "{before:?}");

    // Dry run must not predict SYM failing when deleting the two unrelated occurrences.
    let dry = run(&mut s, "selection.delete", json!({"occurrences": [o1, o2], "dry_run": true}));
    let would_fail: Vec<&str> = dry["would_fail"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!would_fail.contains(&"SYM"), "dry_run must not fail SYM: {dry}");

    run(&mut s, "selection.delete", json!({"occurrences": [o1, o2]}));
    run(&mut s, "timeline.compute_all", json!({}));
    let sym_id = s.doc.features.iter().find(|f| f.name == "SYM").unwrap().id;
    assert_eq!(s.model.result(sym_id).and_then(|r| r.error.as_deref()), None, "SYM must stay healthy after deleting sibling occurrences");
    let after = sym_bboxes(&s);
    assert_eq!(after.len(), 2, "{after:?}");
    for (a, b) in before.iter().zip(&after) {
        for i in 0..3 {
            assert!((a.0[i] - b.0[i]).abs() < 1e-6 && (a.1[i] - b.1[i]).abs() < 1e-6, "before={a:?} after={b:?}");
        }
    }

    // Moving the remaining occurrence after downward migration must still invalidate SYM's pose
    // cache and move both mirrored copies across the z=10 mirror plane.
    run(&mut s, "occurrence.move", json!({"occurrence": o3, "translate": [0, 0, 2], "capture": true}));
    let moved = sym_bboxes(&s);
    assert_eq!(moved.len(), 2, "{moved:?}");
    for (a, b) in after.iter().zip(&moved) {
        assert!((b.0[0] - a.0[0]).abs() < 1e-6 && (b.0[1] - a.0[1]).abs() < 1e-6, "after={a:?} moved={b:?}");
        assert!((b.0[2] - (a.0[2] - 2.0)).abs() < 1e-6 && (b.1[2] - (a.1[2] - 2.0)).abs() < 1e-6, "after={a:?} moved={b:?}");
    }

    // 2) Reverse direction: start with 1 occurrence so REP puts the copy inside the component and
    // SYM stores `[[o1], [o1]]`, then insert a 2nd occurrence (count 1 -> 2) which migrates the
    // copy up to root; SYM must still evaluate cleanly with identical geometry.
    let mut s2 = Session::default();
    let r1 = run(&mut s2, "component.insert_design", json!({"path": plate_path.to_string_lossy(), "translate": [40, 0, 0]}))["occurrence"]
        .as_u64()
        .unwrap();
    let first = s2
        .doc
        .body_frames_with_source(&s2.model.state())
        .into_iter()
        .find(|(_, _, _, path, _)| path == &vec![r1])
        .map(|(w, ..)| w)
        .expect("single occurrence body");
    run(&mut s2, "solid.pattern.rectangular", json!({"bodies": [first], "dir1": [0, 1, 0], "count1": 2, "spacing1": 20, "name": "REP"}));
    let copy2 = s2
        .model
        .state()
        .bodies
        .iter()
        .find(|b| s2.doc.feature(b.feature).is_some_and(|f| f.name == "REP"))
        .map(|b| b.name.clone())
        .expect("pattern copy");
    run(&mut s2, "solid.mirror", json!({"bodies": [first, copy2], "plane": {"origin": [0, 0, 10], "normal": [0, 0, 1]}, "name": "SYM"}));
    let sym2_id = s2.doc.features.iter().find(|f| f.name == "SYM").unwrap().id;
    let before2 = sym_bboxes(&s2);
    assert_eq!(before2.len(), 2, "{before2:?}");
    let _r2 =
        run(&mut s2, "component.insert_design", json!({"path": plate_path.to_string_lossy(), "reuse": true, "translate": [80, 0, 0]}))["occurrence"]
            .as_u64()
            .unwrap();
    run(&mut s2, "timeline.compute_all", json!({}));
    assert_eq!(s2.model.result(sym2_id).and_then(|r| r.error.as_deref()), None, "SYM must stay healthy after adding a 2nd occurrence");
    let after2 = sym_bboxes(&s2);
    assert_eq!(after2.len(), 2, "{after2:?}");
    for (a, b) in before2.iter().zip(&after2) {
        for i in 0..3 {
            assert!((a.0[i] - b.0[i]).abs() < 1e-6 && (a.1[i] - b.1[i]).abs() < 1e-6, "before2={a:?} after2={b:?}");
        }
    }

    // 3) Deleting the referenced occurrence itself (`o3`) still fails SYM as expected.
    run(&mut s, "selection.delete", json!({"occurrences": [o3]}));
    run(&mut s, "timeline.compute_all", json!({}));
    assert!(s.model.result(sym_id).and_then(|r| r.error.as_deref()).is_some(), "deleting the referenced occurrence must still fail SYM");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pattern_and_mirror_copies_follow_seed_material_after_creation_and_respect_overrides() {
    let measured_material = |s: &mut Session, body: &str| -> String {
        let m = run(s, "inspect.measure", json!({}));
        m["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"].as_str() == Some(body))
            .and_then(|b| b["material"].as_str())
            .unwrap_or_default()
            .to_string()
    };

    // Case B: pattern created while seed is Default, then seed is assigned Steel.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Body1"}));
    let pat = run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Body1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 25}))["feature"]
        .as_u64()
        .unwrap();
    run(&mut s, "material.assign", json!({"bodies": ["Body1"], "material": "Steel"}));
    for body in ["Body1", "Body1 (2)", "Body1 (3)"] {
        assert_eq!(s.doc.body_material(body), "Steel", "{body}");
        assert!((s.doc.density(body) - 7.85).abs() < 1e-9, "{body}");
        assert_eq!(measured_material(&mut s, body), "Steel", "{body}");
    }
    assert_eq!(s.doc.body_look("Body1 (2)", pat), s.doc.material_look("Steel"));

    // Case C: timeline.edit re-sending `bodies: ["Body1"]` keeps Steel on every copy.
    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"bodies": ["Body1"]}}));
    for body in ["Body1 (2)", "Body1 (3)"] {
        assert_eq!(s.doc.body_material(body), "Steel", "{body}");
        assert_eq!(measured_material(&mut s, body), "Steel", "{body}");
    }

    // Explicit copy override takes precedence over seed material and survives seed changes.
    run(&mut s, "material.assign", json!({"bodies": ["Body1 (2)"], "material": "Brass"}));
    assert_eq!(s.doc.body_material("Body1 (2)"), "Brass");
    assert_eq!(s.doc.body_material("Body1 (3)"), "Steel");
    run(&mut s, "material.assign", json!({"bodies": ["Body1"], "material": "Aluminum"}));
    assert_eq!(s.doc.body_material("Body1"), "Aluminum");
    assert_eq!(s.doc.body_material("Body1 (2)"), "Brass");
    assert_eq!(s.doc.body_material("Body1 (3)"), "Aluminum");
    assert_eq!(measured_material(&mut s, "Body1 (2)"), "Brass");
    assert_eq!(measured_material(&mut s, "Body1 (3)"), "Aluminum");

    // Case G: editing spacing, direction, and count keeps overrides and gives new copies seed material.
    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"pattern.spacing1": 35, "pattern.dir1": [0, 1, 0], "pattern.count1": 4}}));
    assert_eq!(s.doc.body_material("Body1 (2)"), "Brass");
    for copy in ["Body1 (3)", "Body1 (4)"] {
        assert_eq!(s.doc.body_material(copy), "Aluminum", "{copy}");
        assert_eq!(measured_material(&mut s, copy), "Aluminum", "{copy}");
    }

    // Case G directly after Case B (unstyled existing copy + new copies on count/spacing edit).
    let mut g = Session::default();
    run(&mut g, "solid.box", json!({"length": 10, "width": 10, "height": 10, "center": [0, 0, 5], "operation": "new", "body_name": "Plate"}));
    let pat_g = run(&mut g, "solid.pattern.rectangular", json!({"bodies": ["Plate"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20}))["feature"]
        .as_u64()
        .unwrap();
    run(&mut g, "material.assign", json!({"bodies": ["Plate"], "material": "Copper"}));
    run(&mut g, "timeline.edit", json!({"feature": pat_g, "set": {"pattern.spacing1": 30, "pattern.count1": 4}}));
    for copy in ["Plate (2)", "Plate (3)", "Plate (4)"] {
        assert_eq!(g.doc.body_material(copy), "Copper", "{copy}");
        assert_eq!(measured_material(&mut g, copy), "Copper", "{copy}");
    }

    // Parameter-driven count increase inherits seed material without any timeline.edit.
    let mut param = Session::default();
    run(&mut param, "parameters.add", json!({"name": "n", "expression": "2", "unit": "none"}));
    run(&mut param, "solid.box", json!({"length": 8, "width": 8, "height": 8, "center": [0, 0, 4], "operation": "new", "body_name": "Block"}));
    run(&mut param, "solid.pattern.rectangular", json!({"bodies": ["Block"], "dir1": [1, 0, 0], "count1": "n", "spacing1": 20}));
    run(&mut param, "material.assign", json!({"bodies": ["Block"], "material": "Titanium"}));
    run(&mut param, "parameters.change", json!({"name": "n", "expression": "4"}));
    for copy in ["Block (2)", "Block (3)", "Block (4)"] {
        assert_eq!(param.doc.body_material(copy), "Titanium", "{copy}");
        assert_eq!(measured_material(&mut param, copy), "Titanium", "{copy}");
    }
}

#[test]
fn pattern_edit_in_assembly_keeps_seed_materials_for_added_renamed_and_circular_copies() {
    let mut s = Session::default();
    let o1 = run(&mut s, "component.create", json!({"name": "PartA", "activate": true}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "PinA"}));
    run(&mut s, "material.assign", json!({"bodies": ["PinA"], "material": "Steel"}));
    run(&mut s, "component.activate", json!({"component": "root"}));

    let o2 = run(&mut s, "component.create", json!({"name": "PartB", "activate": true}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 30, 0], "operation": "new", "body_name": "PinB"}));
    run(&mut s, "material.assign", json!({"bodies": ["PinB"], "material": "Aluminum"}));
    run(&mut s, "component.activate", json!({"component": "root"}));

    let o3 = run(&mut s, "component.create", json!({"name": "PartC", "activate": true}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 60, 0], "operation": "new", "body_name": "PinC"}));
    run(&mut s, "material.assign", json!({"bodies": ["PinC"], "material": "Brass"}));
    run(&mut s, "component.activate", json!({"component": "root"}));

    // Case E: timeline.edit adding a new seed body with aligned bodies/placements/body_names.
    let rect = run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["PinA"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 40}))["feature"]
        .as_u64()
        .unwrap();
    run(&mut s, "material.assign", json!({"bodies": ["PinA (2)"], "material": "Copper"}));
    run(
        &mut s,
        "timeline.edit",
        json!({"feature": rect, "set": {"bodies": ["PinA", "PinB"], "placements": [[o1], [o2]], "body_names": ["PinA (2)", "PinB (2)"]}}),
    );
    assert_eq!(s.doc.body_material("PinA (2)"), "Copper", "existing explicit override survives adding a new seed body");
    assert_eq!(s.doc.body_material("PinB (2)"), "Aluminum", "added seed body's copy inherits its seed's material");
    let mass = run(&mut s, "assembly.mass_properties", json!({}));
    let mass_material = |body: &str| -> Option<String> {
        mass["occurrences"]
            .as_array()?
            .iter()
            .flat_map(|o| o["bodies"].as_array().into_iter().flatten())
            .find(|b| b["name"].as_str() == Some(body))
            .and_then(|b| b["material"].as_str())
            .map(str::to_string)
    };
    assert_eq!(mass_material("PinA (2)").as_deref(), Some("Copper"));
    assert_eq!(mass_material("PinB (2)").as_deref(), Some("Aluminum"));

    // Case F (part 1): timeline.edit adding a seed body while omitting body_names.
    run(&mut s, "timeline.edit", json!({"feature": rect, "set": {"bodies": ["PinA", "PinB", "PinC"], "placements": [[o1], [o2], [o3]]}}));
    assert_eq!(s.doc.body_material("PinA (2)"), "Copper");
    assert_eq!(s.doc.body_material("PinB (2)"), "Aluminum");
    assert_eq!(s.doc.body_material("PinC (2)"), "Brass");

    // Case F (part 2): timeline.edit renaming copies via body_names.
    run(&mut s, "timeline.edit", json!({"feature": rect, "set": {"body_names": ["CopyA", "CopyB", "CopyC"]}}));
    assert_eq!(s.doc.body_material("CopyA"), "Steel");
    assert_eq!(s.doc.body_material("CopyB"), "Aluminum");
    assert_eq!(s.doc.body_material("CopyC"), "Brass");

    // Circular pattern: timeline.edit adding a second seed body with aligned placements.
    let circ = run(
        &mut s,
        "solid.pattern.circular",
        json!({"bodies": ["PinA"], "axis": {"origin": [-30, 0, 0], "dir": [0, 0, 1]}, "count": 2, "angle": "90 deg"}),
    )["feature"]
        .as_u64()
        .unwrap();
    run(&mut s, "timeline.edit", json!({"feature": circ, "set": {"bodies": ["PinA", "PinB"], "placements": [[o1], [o2]]}}));
    let circ_copies: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == circ).map(|b| b.name.clone()).collect();
    assert_eq!(circ_copies.len(), 2, "{circ_copies:?}");
    assert_eq!(s.doc.body_material(&circ_copies[0]), "Steel");
    assert_eq!(s.doc.body_material(&circ_copies[1]), "Aluminum");
}

#[test]
fn nested_pattern_and_mirror_copies_inherit_seed_material_recursively() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new", "body_name": "Seed"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Seed"], "dir1": [0, 1, 0], "count1": 3, "spacing1": 25}));
    let pat2 = run(&mut s, "solid.pattern.circular", json!({"bodies": ["Seed (2)"], "axis": "Z", "count": 2, "angle": "180 deg"}));
    let nested_copy = pat2["created_bodies"][0].as_str().unwrap().to_string();
    let mirror = run(&mut s, "solid.mirror", json!({"bodies": [nested_copy], "plane": {"origin": [0, 0, 30], "normal": [0, 0, 1]}}));
    let mirror_id = mirror["feature"].as_u64().unwrap();
    let mirrored_nested = mirror["created_bodies"][0].as_str().unwrap().to_string();

    // Assign Titanium to Seed AFTER all pattern and mirror features already exist.
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Titanium"}));
    for body in ["Seed", "Seed (2)", "Seed (3)", nested_copy.as_str(), mirrored_nested.as_str()] {
        assert_eq!(s.doc.body_material(body), "Titanium", "{body}");
    }

    // Overriding intermediate `Seed (2)` propagates to its downstream copies but not siblings/seed.
    run(&mut s, "material.assign", json!({"bodies": ["Seed (2)"], "material": "Brass"}));
    assert_eq!(s.doc.body_material("Seed"), "Titanium");
    assert_eq!(s.doc.body_material("Seed (3)"), "Titanium");
    assert_eq!(s.doc.body_material("Seed (2)"), "Brass");
    assert_eq!(s.doc.body_material(&nested_copy), "Brass");
    assert_eq!(s.doc.body_material(&mirrored_nested), "Brass");

    // Updating Seed afterwards updates Seed (3), while Seed (2) and its descendants keep Brass.
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Copper"}));
    assert_eq!(s.doc.body_material("Seed"), "Copper");
    assert_eq!(s.doc.body_material("Seed (3)"), "Copper");
    assert_eq!(s.doc.body_material("Seed (2)"), "Brass");
    assert_eq!(s.doc.body_material(&nested_copy), "Brass");
    assert_eq!(s.doc.body_material(&mirrored_nested), "Brass");

    // Editing Mirror via timeline.edit to add `Seed (3)` gives the new mirrored copy Copper.
    run(&mut s, "timeline.edit", json!({"feature": mirror_id, "set": {"bodies": [nested_copy, "Seed (3)"]}}));
    let mirrored: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == mirror_id).map(|b| b.name.clone()).collect();
    assert_eq!(mirrored.len(), 2, "{mirrored:?}");
    assert_eq!(s.doc.body_material(&mirrored[0]), "Brass");
    assert_eq!(s.doc.body_material(&mirrored[1]), "Copper");
}

#[test]
fn assigning_default_to_a_copy_overrides_seed_material_while_root_clears_entry() {
    let measured_material = |s: &mut Session, body: &str| -> String {
        let m = run(s, "inspect.measure", json!({}));
        m["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"].as_str() == Some(body))
            .and_then(|b| b["material"].as_str())
            .unwrap_or_default()
            .to_string()
    };

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "Seed"}));
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Steel"}));
    let pat = run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 25}))["feature"]
        .as_u64()
        .unwrap();
    let mirror = run(&mut s, "solid.mirror", json!({"bodies": ["Seed (2)"], "plane": {"origin": [0, 30, 0], "normal": [0, 1, 0]}}));
    let mirrored = mirror["created_bodies"][0].as_str().unwrap().to_string();

    // Explicitly assigning Default to a copy keeps it (and its downstream copy) on Default
    // rather than falling back to the Steel seed.
    run(&mut s, "material.assign", json!({"bodies": ["Seed (2)"], "material": "Default"}));
    assert_eq!(s.doc.materials.get("Seed (2)").map(String::as_str), Some(solidcraft_doc::DEFAULT_MATERIAL));
    assert_eq!(s.doc.body_material("Seed (2)"), solidcraft_doc::DEFAULT_MATERIAL);
    assert!((s.doc.density("Seed (2)") - solidcraft_doc::DEFAULT_DENSITY).abs() < 1e-9);
    assert_eq!(s.doc.body_look("Seed (2)", pat), None);
    assert_eq!(measured_material(&mut s, "Seed (2)"), solidcraft_doc::DEFAULT_MATERIAL);
    assert_eq!(s.doc.body_material(&mirrored), solidcraft_doc::DEFAULT_MATERIAL);
    assert_eq!(measured_material(&mut s, &mirrored), solidcraft_doc::DEFAULT_MATERIAL);
    assert_eq!(s.doc.body_material("Seed"), "Steel");
    assert_eq!(s.doc.body_material("Seed (3)"), "Steel");

    // Assigning Default to a non-copy root body still removes its stored entry.
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Default"}));
    assert!(!s.doc.materials.contains_key("Seed"));
    assert_eq!(s.doc.body_material("Seed"), solidcraft_doc::DEFAULT_MATERIAL);
    assert_eq!(s.doc.body_material("Seed (3)"), solidcraft_doc::DEFAULT_MATERIAL);
}

#[test]
fn explicit_copy_override_survives_when_seed_temporarily_matches_same_material() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "Seed"}));
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Steel"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Seed"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 25}));
    let mirror = run(&mut s, "solid.mirror", json!({"bodies": ["Seed"], "plane": {"origin": [0, 30, 0], "normal": [0, 1, 0]}}));
    let mirrored = mirror["created_bodies"][0].as_str().unwrap().to_string();
    let moved = run(&mut s, "solid.move", json!({"bodies": ["Seed"], "translate": [0, -30, 0], "copy": true}))["created_bodies"][0]
        .as_str()
        .unwrap()
        .to_string();

    // Give explicit Copper overrides on a pattern copy, a mirrored copy, and a moved copy.
    run(&mut s, "material.assign", json!({"bodies": ["Seed (2)", mirrored.as_str(), moved.as_str()], "material": "Copper"}));
    assert_eq!(s.doc.body_material("Seed (3)"), "Steel");

    // Seed changes Steel -> Copper -> Aluminum: inherited sibling follows, explicit overrides stay Copper.
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Copper"}));
    run(&mut s, "material.assign", json!({"bodies": ["Seed"], "material": "Aluminum"}));
    assert_eq!(s.doc.body_material("Seed"), "Aluminum");
    assert_eq!(s.doc.body_material("Seed (3)"), "Aluminum");
    assert_eq!(s.doc.body_material("Seed (2)"), "Copper");
    assert_eq!(s.doc.body_material(&mirrored), "Copper");
    assert_eq!(s.doc.body_material(&moved), "Copper");
}

#[test]
fn reseeding_a_pattern_updates_downstream_copies_automatically() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "SteelSeed"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 50, 0], "operation": "new", "body_name": "AlumSeed"}));
    run(&mut s, "material.assign", json!({"bodies": ["SteelSeed"], "material": "Steel"}));
    run(&mut s, "material.assign", json!({"bodies": ["AlumSeed"], "material": "Aluminum"}));

    let pat_p = run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["SteelSeed"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 25}))["feature"]
        .as_u64()
        .unwrap();
    run(&mut s, "timeline.edit", json!({"feature": pat_p, "set": {"body_names": ["CopyP"]}}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["CopyP"], "dir1": [0, 0, 1], "count1": 3, "spacing1": 25}));
    let mirror = run(&mut s, "solid.mirror", json!({"bodies": ["CopyP (2)"], "plane": {"origin": [-20, 0, 0], "normal": [1, 0, 0]}}));
    let mirrored_q = mirror["created_bodies"][0].as_str().unwrap().to_string();
    run(&mut s, "material.assign", json!({"bodies": ["CopyP (3)"], "material": "Brass"}));

    assert_eq!(s.doc.body_material("CopyP"), "Steel");
    assert_eq!(s.doc.body_material("CopyP (2)"), "Steel");
    assert_eq!(s.doc.body_material(&mirrored_q), "Steel");
    assert_eq!(s.doc.body_material("CopyP (3)"), "Brass");

    // Re-seeding pattern P from SteelSeed to AlumSeed updates CopyP and all unoverridden
    // downstream copies (CopyP (2) and its mirror) while preserving CopyP (3)'s explicit Brass.
    run(&mut s, "timeline.edit", json!({"feature": pat_p, "set": {"bodies": ["AlumSeed"], "body_names": ["CopyP"]}}));
    assert_eq!(s.doc.body_material("CopyP"), "Aluminum");
    assert_eq!(s.doc.body_material("CopyP (2)"), "Aluminum");
    assert_eq!(s.doc.body_material(&mirrored_q), "Aluminum");
    assert_eq!(s.doc.body_material("CopyP (3)"), "Brass");
}

#[test]
fn multi_seed_joined_pattern_separate_copies_inherit_unanimous_seed_material() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "operation": "new", "body_name": "Left"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 40, 0], "operation": "new", "body_name": "Right"}));
    run(&mut s, "material.assign", json!({"bodies": ["Left", "Right"], "material": "Steel"}));
    let pat =
        run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Left", "Right"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 30, "join": true}))
            ["feature"]
            .as_u64()
            .unwrap();

    let copies: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == pat).map(|b| b.name.clone()).collect();
    assert!(copies.len() >= 2, "{copies:?}");
    for copy in &copies {
        assert_eq!(s.doc.body_material(copy), "Steel", "{copy}");
    }
    run(&mut s, "material.assign", json!({"bodies": [copies[0].as_str()], "material": "Default"}));
    assert_eq!(s.doc.body_material(&copies[0]), solidcraft_doc::DEFAULT_MATERIAL);
    assert_eq!(s.doc.body_material(&copies[1]), "Steel");
}

/// Renaming or deleting an input body of a multi-body pattern (with no placement) must fail the
/// pattern with an unknown-body error instead of silently skipping the missing entries.
#[test]
fn pattern_reports_error_when_input_body_is_renamed_or_deleted() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [10, -5, 0], "length": 10, "width": 10, "height": 10, "body_name": "Bracket"}));
    let p1 = run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"bodies": ["Bracket"], "dir1": [0, 1, 0], "count1": 3, "spacing1": 20, "name": "GridPat"}),
    )["feature"]
        .as_u64()
        .unwrap();
    let p2 = run(
        &mut s,
        "solid.pattern.circular",
        json!({"bodies": ["Bracket", "Bracket (2)", "Bracket (3)"], "axis": "Z", "count": 2, "name": "CircPat"}),
    )["feature"]
        .as_u64()
        .unwrap();
    let p3 = run(
        &mut s,
        "solid.pattern.rectangular",
        json!({"bodies": ["Bracket", "Bracket (2)"], "dir1": [0, 0, 1], "count1": 2, "spacing1": 25, "name": "StackPat"}),
    )["feature"]
        .as_u64()
        .unwrap();

    assert!(s.model.result(p2).unwrap().error.is_none());
    assert!(s.model.result(p3).unwrap().error.is_none());

    // Rename P1's copies so Bracket (2) and Bracket (3) no longer exist.
    run(&mut s, "timeline.edit", json!({"feature": p1, "set": {"body_names": ["Renamed_1", "Renamed_2"]}}));
    let computed = run(&mut s, "timeline.compute_all", json!({}));
    assert_eq!(computed["errors"].as_u64(), Some(2), "{computed}");

    let err_p2 = s.model.result(p2).and_then(|r| r.error.as_deref()).unwrap_or("");
    assert!(
        err_p2.contains("unknown body `Bracket (2)`") && err_p2.contains("2 missing") && err_p2.contains("`Bracket (3)`"),
        "expected missing-bodies error on CircPat, got: {err_p2:?}"
    );
    let err_p3 = s.model.result(p3).and_then(|r| r.error.as_deref()).unwrap_or("");
    assert!(err_p3.contains("unknown body `Bracket (2)`"), "expected unknown-body error on StackPat, got: {err_p3:?}");

    let inspected = run(&mut s, "document.inspect", json!({"bodies": false, "sketches": false}));
    let timeline = inspected["timeline"].as_array().unwrap();
    for name in ["CircPat", "StackPat"] {
        let row = timeline.iter().find(|r| r["name"] == name).unwrap();
        assert!(row["error"].as_str().is_some_and(|e| e.contains("unknown body")), "{name}: {row}");
    }

    // Deleting one seed body of a two-body pattern also fails the pattern cleanly.
    let mut del = Session::default();
    run(&mut del, "solid.box", json!({"corner": [0, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Pin"}));
    run(&mut del, "solid.box", json!({"corner": [30, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Spacer", "name": "SpacerBox"}));
    let pat = run(
        &mut del,
        "solid.pattern.rectangular",
        json!({"bodies": ["Pin", "Spacer"], "dir1": [0, 1, 0], "count1": 2, "spacing1": 20, "name": "PairPat"}),
    )["feature"]
        .as_u64()
        .unwrap();
    run(&mut del, "timeline.delete", json!({"feature": "SpacerBox"}));
    let err_del = del.model.result(pat).and_then(|r| r.error.as_deref()).unwrap_or("");
    assert!(err_del.contains("unknown body `Spacer`"), "expected unknown-body error after delete, got: {err_del:?}");
}

/// An explicit placement on a body pattern must resolve strictly within that placement instead of
/// falling back to a same-named body in the root component (whether a plain root body or an
/// unrelated root pattern copy).
#[test]
fn pattern_explicit_placement_is_not_shadowed_by_same_named_root_body() {
    let mut s = Session::default();
    let sub = run(&mut s, "component.create", json!({"name": "SubAsm", "grounded": false}));
    let occ = sub["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"corner": [50, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Pin"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 100, 0]}));

    run(&mut s, "solid.box", json!({"corner": [0, -80, 0], "length": 10, "width": 10, "height": 10, "body_name": "Housing"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Housing"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 25, "name": "RootPat"}));

    let circ = run(&mut s, "solid.pattern.circular", json!({"bodies": ["Pin", "Housing"], "axis": "Z", "count": 2, "name": "CircPat"}))["feature"]
        .as_u64()
        .unwrap();
    assert!(s.model.result(circ).unwrap().error.is_none());

    // 1) Plain root body ("Housing") paired with occurrence placement `[occ]` must fail instead
    //    of silently falling back to the root body.
    if let Some(f) = Arc::make_mut(&mut s.doc).feature_mut(circ)
        && let solidcraft_doc::FeatureKind::Pattern { bodies, placements, .. } = &mut f.kind
    {
        *bodies = vec!["Housing".into(), "Pin".into()];
        *placements = vec![vec![occ], vec![occ]];
    }
    s.refresh();
    let err_root = s.model.result(circ).and_then(|r| r.error.as_deref()).unwrap_or("");
    assert!(err_root.contains("no longer shows it"), "plain root body must not shadow explicit placement: {err_root:?}");

    // 2) Unrelated root pattern copy ("Housing (2)") paired with `[occ]` must also fail instead
    //    of matching `migrated_copy_placement`'s root prefix `&[]`.
    if let Some(f) = Arc::make_mut(&mut s.doc).feature_mut(circ)
        && let solidcraft_doc::FeatureKind::Pattern { bodies, placements, .. } = &mut f.kind
    {
        *bodies = vec!["Housing (2)".into(), "Pin".into()];
        *placements = vec![vec![occ], vec![occ]];
    }
    s.refresh();
    let err_copy = s.model.result(circ).and_then(|r| r.error.as_deref()).unwrap_or("");
    assert!(err_copy.contains("no longer shows it"), "root pattern copy must not shadow explicit placement: {err_copy:?}");
}

#[test]
fn pattern_edit_without_body_names_preserves_copy_names_and_overrides_with_left_out_entries() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [20, 0, 0], "operation": "new", "body_name": "FastenerA"}));
    let feat_b =
        run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [20, 20, 0], "operation": "new", "body_name": "FastenerB"}))
            ["feature"]
            .as_u64()
            .unwrap();
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [20, 40, 0], "operation": "new", "body_name": "FastenerC"}));
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [20, 60, 0], "operation": "new", "body_name": "FastenerD"}));
    run(&mut s, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [20, 80, 0], "operation": "new", "body_name": "FastenerE"}));

    run(&mut s, "material.assign", json!({"bodies": ["FastenerA", "FastenerB"], "material": "Steel"}));
    run(&mut s, "material.assign", json!({"bodies": ["FastenerC"], "material": "Aluminum"}));
    run(&mut s, "material.assign", json!({"bodies": ["FastenerD"], "material": "Copper"}));
    run(&mut s, "material.assign", json!({"bodies": ["FastenerE"], "material": "Titanium"}));

    let pat = run(
        &mut s,
        "solid.pattern.circular",
        json!({"bodies": ["FastenerA", "FastenerB", "FastenerC", "FastenerD"], "axis": "Z", "count": 2, "angle": "180 deg"}),
    )["feature"]
        .as_u64()
        .unwrap();
    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"body_names": ["Copy1", "Copy2", "Copy3", "Copy4"]}}));
    run(&mut s, "material.assign", json!({"bodies": ["Copy4"], "material": "Brass"}));

    // Suppress FastenerB so the pattern leaves index 1 out while keeping 4 source-aligned slots.
    run(&mut s, "timeline.suppress", json!({"feature": feat_b, "suppressed": true}));
    let left_out_copies: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == pat).map(|b| b.name.clone()).collect();
    assert_eq!(left_out_copies, vec!["Copy1", "Copy3", "Copy4"]);

    // Adding FastenerE without body_names while FastenerB is left out must keep Copy1/Copy3/Copy4
    // aligned to their sources (preserving Copy4's explicit Brass) and give FastenerE's copy Titanium.
    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"bodies": ["FastenerA", "FastenerB", "FastenerC", "FastenerD", "FastenerE"]}}));
    let after_add: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == pat).map(|b| b.name.clone()).collect();
    assert_eq!(after_add.len(), 4, "{after_add:?}");
    assert_eq!(after_add[..3], ["Copy1", "Copy3", "Copy4"]);
    assert_eq!(s.doc.body_material("Copy1"), "Steel");
    assert_eq!(s.doc.body_material("Copy3"), "Aluminum");
    assert_eq!(s.doc.body_material("Copy4"), "Brass");
    assert_eq!(s.doc.body_material(&after_add[3]), "Titanium");

    // Purging FastenerB and FastenerC without body_names must keep Copy4 on FastenerD (not Copy3)
    // and retain both inherited materials and Copy4's explicit Brass override.
    let copy_e = after_add[3].clone();
    run(&mut s, "timeline.edit", json!({"feature": pat, "set": {"bodies": ["FastenerA", "FastenerD", "FastenerE"]}}));
    let after_purge: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == pat).map(|b| b.name.clone()).collect();
    assert_eq!(after_purge, ["Copy1", "Copy4", copy_e.as_str()]);
    assert_eq!(s.doc.body_material("Copy1"), "Steel");
    assert_eq!(s.doc.body_material("Copy4"), "Brass");
    assert_eq!(s.doc.body_material(&copy_e), "Titanium");
}

#[test]
fn mirror_combine_unmerged_copies_inherit_seed_material() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new", "body_name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 30, 0], "operation": "new", "body_name": "Spacer"}));

    // Single-seed combined mirror of a body away from the mirror plane yields an unmerged copy lump.
    let single = run(&mut s, "solid.mirror", json!({"bodies": ["Bracket"], "plane": "YZ", "combine": true}));
    let single_id = single["feature"].as_u64().unwrap();
    let single_copy =
        s.model.state().bodies.iter().find(|b| b.feature == single_id).map(|b| b.name.clone()).expect("disconnected mirror combine copy");

    run(&mut s, "material.assign", json!({"bodies": ["Bracket"], "material": "Steel"}));
    assert_eq!(s.doc.body_material(&single_copy), "Steel");
    run(&mut s, "material.assign", json!({"bodies": [single_copy.as_str()], "material": "Default"}));
    assert_eq!(s.doc.body_material(&single_copy), solidcraft_doc::DEFAULT_MATERIAL);
    assert_eq!(s.doc.body_material("Bracket"), "Steel");

    // Multi-seed combined mirror inherits unanimously when seeds share the same material.
    run(&mut s, "material.assign", json!({"bodies": ["Spacer"], "material": "Steel"}));
    let multi =
        run(&mut s, "solid.mirror", json!({"bodies": ["Bracket", "Spacer"], "plane": {"origin": [0, -20, 0], "normal": [0, 1, 0]}, "combine": true}));
    let multi_id = multi["feature"].as_u64().unwrap();
    let multi_copies: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == multi_id).map(|b| b.name.clone()).collect();
    assert_eq!(multi_copies.len(), 2, "{multi_copies:?}");
    for copy in &multi_copies {
        assert_eq!(s.doc.body_material(copy), "Steel", "{copy}");
    }
    run(&mut s, "material.assign", json!({"bodies": ["Spacer"], "material": "Aluminum"}));
    for copy in &multi_copies {
        assert_eq!(s.doc.body_material(copy), solidcraft_doc::DEFAULT_MATERIAL, "{copy}");
    }
}

/// A pattern of picked placements adds them again once and keeps no timeline feature, so a body
/// pattern redefined into one says so instead of "did not make a feature" (#22942).
#[test]
fn a_body_pattern_redefined_to_picked_placements_says_why_it_is_refused() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part"}));
    let occurrence = part["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[0,0,0],"body_name":"PartBox"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"corner":[30,0,0],"body_name":"Block"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies":["Block"],"dir1":[0,0,1],"count1":3,"spacing1":20,"name":"Pattern"}));
    let (doc, occurrences) = (s.doc.clone(), s.doc.occurrences.len());
    let err = s
        .execute(
            "timeline.redefine",
            &json!({"feature":"Pattern","command":"solid.pattern.rectangular","params":{
                "occurrences":[[occurrence]],"dir1":[0,0,1],"count1":3,"spacing1":20}}),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("not kept in the timeline"), "{err}");
    assert!(!err.contains("did not make a feature"), "{err}");
    assert_eq!(s.doc, doc, "the refused edit leaves the design as it was");
    assert_eq!(s.doc.occurrences.len(), occurrences, "no placement is added");
}

/// The pattern repeats the full cut, including the tapered bore, helical groove, and any lead-in.
fn check_modelled_tapered_hole_patterns(circular: bool, lead_in: bool) {
    use solidcore_record::Vec3;
    for designation in ["Rc 1/2", "1/2-14 NPT"] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 100, "width": 120, "height": 16, "body_name": "Plate"}));
        let stock = volume(&mut s);
        let mut hole = json!({
            "position": [15, 50, 16], "direction": [0, 0, -1], "diameter": 18, "thread_depth": 10,
            "thread": designation, "modeled": true, "name": "Tap"
        });
        if lead_in {
            hole["chamfer_distance"] = json!(1);
        }
        run(&mut s, "solid.hole", hole);
        let drilled = volume(&mut s);
        let one_cut = stock - drilled;
        assert!(one_cut > 0.0, "{designation}: the seed hole must remove material");

        for count in [2, 4, 6] {
            let (command, params) = if circular {
                (
                    "solid.pattern.circular",
                    json!({"features": ["Tap"], "axis": {"origin": [50, 50, 0], "dir": [0, 0, 1]}, "count": count, "angle": 360}),
                )
            } else {
                (
                    "solid.pattern.rectangular",
                    json!({"features": ["Tap"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 35,
                        "dir2": [0, 1, 0], "count2": count / 2, "spacing2": 28}),
                )
            };
            let case = format!("{designation}, {command}, count {count}, lead-in {lead_in}");
            let result = s.execute(command, &params).unwrap_or_else(|err| panic!("{case}: {err}"));
            assert!(result["error"].is_null() && result["warning"].is_null(), "{case}: {result}");
            // Rigid copies of disjoint cuts in the same stock remove additive volumes.
            // The command count includes the seed, which is already removed from drilled.
            let removed = drilled - volume(&mut s);
            let expected = f64::from(count - 1) * one_cut;
            assert!(rel(removed, expected) < 1e-3, "{case}: removed {removed} mm³, expected {expected} mm³");

            let state = s.world_state().unwrap();
            assert_eq!(state.bodies.len(), 1, "{case}");
            for index in 0..count {
                let (x, y) = if circular {
                    let theta = 2.0 * PI * f64::from(index) / f64::from(count);
                    (50.0 - 35.0 * theta.cos(), 50.0 - 35.0 * theta.sin())
                } else {
                    (15.0 + 35.0 * f64::from(index % 2), 50.0 + 28.0 * f64::from(index / 2))
                };
                let point = Vec3::new(x, y, 8.0);
                assert_eq!(
                    state.bodies[0].body.classify_point(point).unwrap(),
                    solidcraft_kernel::PointClass::Outside,
                    "{case}: missing hole at {point:?}"
                );
            }
            run(&mut s, "edit.undo", json!({}));
        }
    }
}

#[test]
fn rectangular_patterns_repeat_modelled_tapered_holes() {
    check_modelled_tapered_hole_patterns(false, false);
}

#[test]
fn rectangular_patterns_repeat_modelled_tapered_holes_with_lead_in() {
    check_modelled_tapered_hole_patterns(false, true);
}

#[test]
fn circular_patterns_repeat_modelled_tapered_holes() {
    check_modelled_tapered_hole_patterns(true, false);
}

#[test]
fn circular_patterns_repeat_modelled_tapered_holes_with_lead_in() {
    check_modelled_tapered_hole_patterns(true, true);
}

/// A spacing expression that places a copy past the brep coordinate ceiling is an error,
/// and the design stays as it was. `Brep::validate_built` rejects a vertex with absolute value
/// above 1e12. This shaft's radius is 4, so a spacing of 1e12 cannot fit; a spacing of 1e9 plus
/// a few millimetres of the body is still below 1e12.
#[test]
fn a_rectangular_pattern_spacing_expression_past_the_coordinate_ceiling_is_refused() {
    // `Brep::validate_built`'s coordinate ceiling.
    const CEILING: f64 = 1e12;

    fn fresh() -> Session {
        let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Default::default() };
        run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 8, "body_name": "Shaft"}));
        run(&mut s, "solid.thread", json!({"face": [4, 0, 4], "designation": "M8"}));
        s
    }

    fn feature_list(s: &Session) -> Vec<(u64, String)> {
        s.doc.features.iter().map(|f| (f.id, f.name.clone())).collect()
    }

    fn body_names(s: &Session) -> Vec<String> {
        s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
    }

    fn threads(s: &mut Session) -> Vec<Value> {
        run(s, "model.threads", json!({}))["threads"].as_array().cloned().unwrap_or_default()
    }

    fn axis_within_ceiling(thread: &Value) -> bool {
        thread["axis_point"]
            .as_array()
            .is_some_and(|point| point.len() == 3 && point.iter().all(|c| c.as_f64().is_some_and(|c| c.is_finite() && c.abs() <= CEILING)))
    }

    fn pattern(spacing: Value, count: i64) -> Value {
        json!({"bodies": ["Shaft"], "dir1": [1, 0, 0], "count1": count, "spacing1": spacing})
    }

    fn copy_name(reply: &Value) -> String {
        let created = reply["created_bodies"].as_array().unwrap_or_else(|| panic!("{reply}"));
        assert_eq!(created.len(), 1, "{reply}");
        let copy = created[0].as_str().unwrap_or_else(|| panic!("{reply}")).to_string();
        assert_ne!(copy, "Shaft");
        copy
    }

    // Spacing 20 mm: one copy. The shaft's M8 stays. A thread row for the copy, when the
    // pattern records one, stays finite and within the ceiling.
    {
        let mut s = fresh();
        let before = body_names(&s);
        let reply = run(&mut s, "solid.pattern.rectangular", pattern(json!("20"), 2));
        let copy = copy_name(&reply);
        assert_eq!(body_names(&s).len(), before.len() + 1, "{:?}", body_names(&s));
        let listed = threads(&mut s);
        assert!(listed.iter().any(|t| t["body"] == "Shaft" && t["designation"] == "M8"), "{listed:?}");
        // Every listed axis, including a copy's when the pattern records one on that body.
        assert!(listed.iter().all(axis_within_ceiling), "{listed:?}");
        assert!(listed.iter().filter(|t| t["body"] == copy).all(axis_within_ceiling), "{listed:?}");
    }

    // 1e9 mm along X plus this body's extent (radius 4, height 8) is still under 1e12.
    {
        let mut s = fresh();
        let reply = run(&mut s, "solid.pattern.rectangular", pattern(json!("1e9"), 2));
        let copy = copy_name(&reply);
        let state = s.model.state();
        let body = state.body(&copy).unwrap_or_else(|| panic!("copy {copy} was not stored: {reply}"));
        let vertices: Vec<[f64; 3]> = body.body.record().vertices.iter().map(|v| v.point).collect();
        assert!(!vertices.is_empty(), "{copy} has no vertices to read");
        // The spacing is applied: some coordinate is far past the shaft, and none crosses 1e12.
        assert!(
            vertices.iter().any(|point| point.iter().any(|coordinate| coordinate.abs() > 1e8)),
            "{copy} vertices were not placed at the 1e9 spacing: {vertices:?}"
        );
        for point in &vertices {
            for coordinate in point {
                assert!(coordinate.is_finite() && coordinate.abs() <= CEILING, "{copy} vertex {point:?}");
            }
        }
        drop(state);
        let listed = threads(&mut s);
        assert!(listed.iter().any(|t| t["body"] == "Shaft" && t["designation"] == "M8"), "{listed:?}");
        assert!(listed.iter().filter(|t| t["body"] == copy).all(axis_within_ceiling), "{listed:?}");
    }

    fn refused(spacing: Value, count: i64) {
        let mut s = fresh();
        let features = feature_list(&s);
        let bodies = body_names(&s);
        let params = pattern(spacing, count);
        let _err = s.execute("solid.pattern.rectangular", &params).unwrap_err();
        assert_eq!(feature_list(&s), features, "{params}");
        assert_eq!(body_names(&s), bodies, "{params}");
        let listed = threads(&mut s);
        assert!(listed.iter().all(axis_within_ceiling), "a thread copy is past the ceiling: {listed:?}");
        assert!(listed.iter().any(|t| t["body"] == "Shaft" && t["designation"] == "M8"), "the shaft's M8 thread is gone: {listed:?}");
        assert_eq!(listed.len(), 1, "a thread copy was stored: {listed:?}");
    }

    // A JSON number, not an expression: the length cap already refuses 1e20.
    refused(json!(1e20), 2);
    // The second step is 2 * 9e307, which is not finite.
    refused(json!("9e307"), 3);
    for spacing in ["1e20", "1e308", "-1e308", "1e12"] {
        refused(json!(spacing), 2);
    }
}
const PATTERN_COORDINATE_CEILING: f64 = 1e12;

fn shaft_for_coordinate_ceiling_patterns() -> Session {
    let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Default::default() };
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 8, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [4, 0, 4], "designation": "M8"}));
    s
}

fn shaft_pattern_row(spacing: Value, count: Value) -> Value {
    json!({"bodies": ["Shaft"], "dir1": [1, 0, 0], "count1": count, "spacing1": spacing})
}

fn reject_pattern_without_changing_design(s: &mut Session, command: &str, params: &Value) -> String {
    let doc = s.doc.to_json();
    let undo = s.undo.clone();
    let body_names = |s: &Session| s.model.state().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let bodies = body_names(s);

    let error = s.execute(command, params).unwrap_err();
    assert!(!matches!(error, crate::EngineError::Internal(..)), "{command} {params}: {error}");
    assert_eq!(s.doc.to_json(), doc);
    assert_eq!(body_names(s), bodies);
    assert_eq!(s.undo.len(), undo.len());
    assert!(s.undo.iter().zip(&undo).all(|(after, before)| after.label == before.label && Arc::ptr_eq(&after.doc, &before.doc)));
    error.to_string()
}

fn assert_pattern_coordinate_message(message: &str, adjustment: &str) {
    let text = message.to_ascii_lowercase();
    assert!(!text.contains("portable b-rep"), "{message}");
    assert!(text.contains("coordinate"), "{message}");
    assert!(text.contains("range") || text.contains("limit") || text.contains("ceiling"), "{message}");
    assert!(text.contains(adjustment), "{message}");
}

#[test]
fn pattern_coordinate_ceiling_reports_the_rectangular_threaded_shaft_refusal() {
    let mut s = shaft_for_coordinate_ceiling_patterns();
    let error = reject_pattern_without_changing_design(&mut s, "solid.pattern.rectangular", &shaft_pattern_row(json!("999999999996"), json!(2)));
    assert_pattern_coordinate_message(&error, "spacing");
}

#[test]
fn pattern_coordinate_ceiling_covers_the_rectangular_and_circular_cap_origins() {
    let mut s = shaft_for_coordinate_ceiling_patterns();
    let ceiling = PATTERN_COORDINATE_CEILING;
    let inside = ceiling - 10.0;
    let outside = ceiling - 8.0;
    let state = s.model.state();
    let shaft = state.body("Shaft").unwrap().body.record();
    // The shaft lies within radius 4 of its axis. The stored cap plane origin can lie
    // outside the cap. Compare both offsets with the source record to prove that the
    // outside case crosses the ceiling at a cap origin while the vertices still fit.
    assert!(shaft.vertices.iter().all(|v| (v.point[0] + outside).abs() <= ceiling));
    let cap_x = shaft
        .faces
        .iter()
        .filter_map(|face| match face.surface {
            solidcore_record::brep::Surface::Plane { origin, .. } | solidcore_record::brep::Surface::PlaneThrough { origin, .. } => Some(origin[0]),
            _ => None,
        })
        .max_by(f64::total_cmp)
        .unwrap();
    assert!(cap_x + inside < ceiling && cap_x + outside > ceiling, "{cap_x}");

    let row = |offset: f64| shaft_pattern_row(json!(offset.to_string()), json!(2));
    let placed = run(&mut s, "solid.pattern.rectangular", row(inside));
    let copies = placed["created_bodies"].as_array().unwrap();
    assert_eq!(copies.len(), 1);
    let copy = copies[0].as_str().unwrap();

    // A half-turn about the Z direction through x = a sends x to 2*a - x.
    // The copy at the inside offset stays in range about x = 0. Moving the
    // axis to x = -1 makes the same cap origin cross the negative ceiling.
    let ring = |axis_x: i32| json!({"bodies": [copy], "axis": {"origin": [axis_x, 0, 0], "dir": [0, 0, 1]}, "count": 2, "angle": "180 deg"});
    let rotated = run(&mut s, "solid.pattern.circular", ring(0));
    assert_eq!(rotated["created_bodies"].as_array().unwrap().len(), 1);

    let rect_error = reject_pattern_without_changing_design(&mut s, "solid.pattern.rectangular", &row(outside));
    let circ_error = reject_pattern_without_changing_design(&mut s, "solid.pattern.circular", &ring(-1));
    assert_pattern_coordinate_message(&rect_error, "spacing");
    assert_pattern_coordinate_message(&circ_error, "count");
}

#[test]
fn pattern_coordinate_ceiling_hostile_spacing_angle_and_counts_are_refused() {
    let mut s = shaft_for_coordinate_ceiling_patterns();
    for spacing in ["NaN", "inf", "1e309"] {
        reject_pattern_without_changing_design(&mut s, "solid.pattern.rectangular", &shaft_pattern_row(json!(spacing), json!(2)));
        reject_pattern_without_changing_design(
            &mut s,
            "solid.pattern.circular",
            &json!({"bodies": ["Shaft"], "axis": "Z", "count": 2, "angle": spacing}),
        );
    }
    reject_pattern_without_changing_design(&mut s, "solid.pattern.rectangular", &shaft_pattern_row(json!(10), json!(u64::MAX)));
    reject_pattern_without_changing_design(&mut s, "solid.pattern.circular", &json!({"bodies": ["Shaft"], "axis": "Z", "count": u64::MAX}));
}

// ---- no-cache model helpers
fn fresh_no_cache_model(doc: &solidcraft_doc::Document) -> solidcraft_doc::Model {
    let mut m = solidcraft_doc::Model::new();
    m.no_cache = true;
    m.evaluate(doc);
    assert!(
        m.results.iter().all(|r| r.error.is_none()),
        "fresh eval errors: {:?}",
        m.results.iter().filter_map(|r| r.error.as_deref()).collect::<Vec<_>>()
    );
    m
}

/// Hub/Profile/Sweep(cut, twist 180 deg)/Circular Pattern in C1 referencing a 2D
/// planar XZ Path sketch in C2. Moving C2 must invalidate the Sweep and Circular Pattern caches
/// so the cached Hub area matches a fresh `Model::no_cache` evaluation.
#[test]
fn moving_component_with_planar_sweep_path_recomputes_twisted_cut_pattern() {
    let mut s = Session::default();
    let c1 = run(&mut s, "component.create", json!({"name": "C1", "grounded": false}))["component"].as_u64().unwrap();
    run(&mut s, "solid.cylinder", json!({"radius": 15, "height": 40, "base": [0, 0, 0], "body_name": "Hub"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Profile"}));
    run(&mut s, "sketch.circle.center", json!({"center": [13, 0], "radius": 3, "id": "prof"}));
    run(&mut s, "sketch.finish", json!({}));

    let c2_occ = run(&mut s, "component.create", json!({"name": "C2", "grounded": false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Path"}));
    run(&mut s, "sketch.line", json!({"points": [[13, 0], [13, -40]], "ids": ["path_line"]}));
    run(&mut s, "sketch.finish", json!({}));

    run(&mut s, "component.activate", json!({"component": c1}));
    run(
        &mut s,
        "solid.sweep",
        json!({
            "sketch": "Profile",
            "path_sketch": "Path",
            "path": ["path_line"],
            "twist": "180 deg",
            "operation": "cut",
            "targets": ["Hub"],
            "name": "Groove"
        }),
    );
    run(&mut s, "solid.pattern.circular", json!({"features": ["Groove"], "axis": "Z", "count": 4, "name": "Pattern"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);

    run(&mut s, "occurrence.move", json!({"occurrence": c2_occ, "translate": [2, 0, 0], "capture": true}));
    let fresh = fresh_no_cache_model(&s.doc);
    assert!(s.model.last_recomputed > 0, "expected recomputation after moving C2, got last_recomputed={}", s.model.last_recomputed);

    let hub_area = |m: &solidcraft_doc::Model| solidcraft_kernel::measure(&m.state().body("Hub").unwrap().body).unwrap().area;
    let cached = hub_area(&s.model);
    let expected = hub_area(&fresh);
    assert!((cached - expected).abs() < 1e-3, "cached Hub area {cached} vs fresh {expected}");
}

/// Moving component C2 owning a 2D planar `Pipe` `path_sketch` invalidates the Pipe in C1 and
/// matches fresh `Model::no_cache` evaluation.
#[test]
fn moving_component_with_planar_pipe_path_recomputes_pipe() {
    let mut s = Session::default();
    let c1 = run(&mut s, "component.create", json!({"name": "C1", "grounded": false}))["component"].as_u64().unwrap();
    let c2_occ = run(&mut s, "component.create", json!({"name": "C2", "grounded": false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "PipePath"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -20]], "ids": ["p1"]}));
    run(&mut s, "sketch.finish", json!({}));

    run(&mut s, "component.activate", json!({"component": c1}));
    run(&mut s, "solid.pipe", json!({"path_sketch": "PipePath", "path": ["p1"], "diameter": 4, "operation": "new", "body_name": "Tube"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);

    run(&mut s, "occurrence.move", json!({"occurrence": c2_occ, "translate": [5, 0, 0], "capture": true}));
    let fresh = fresh_no_cache_model(&s.doc);
    assert!(s.model.last_recomputed > 0, "expected recomputation after moving C2, got last_recomputed={}", s.model.last_recomputed);

    let bounds = |m: &solidcraft_doc::Model| solidcraft_kernel::measure(&m.state().body("Tube").unwrap().body).unwrap().bbox;
    let cached = bounds(&s.model);
    let expected = bounds(&fresh);
    assert!(cached.min.dist(expected.min) < 1e-3 && cached.max.dist(expected.max) < 1e-3, "cached {cached:?} vs fresh {expected:?}");
}

/// Moving component C2 owning a 2D planar `solid.pattern.path` `path_sketch` invalidates the
/// path pattern in C1 and matches fresh `Model::no_cache` evaluation.
#[test]
fn moving_component_with_planar_pattern_path_recomputes_path_pattern() {
    let mut s = Session::default();
    let c1 = run(&mut s, "component.create", json!({"name": "C1", "grounded": false}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [0, 0, 0], "body_name": "Pin"}));

    let c2_occ = run(&mut s, "component.create", json!({"name": "C2", "grounded": false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "PatternPath"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "ids": ["l1"]}));
    run(&mut s, "sketch.finish", json!({}));

    run(&mut s, "component.activate", json!({"component": c1}));
    run(&mut s, "solid.pattern.path", json!({"bodies": ["Pin"], "path_sketch": "PatternPath", "path": ["l1"], "count": 3, "spacing": 10}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);

    run(&mut s, "occurrence.move", json!({"occurrence": c2_occ, "axis": [0, 0, 1], "angle": "90 deg", "capture": true}));
    let fresh = fresh_no_cache_model(&s.doc);
    assert!(s.model.last_recomputed > 0, "expected recomputation after moving C2, got last_recomputed={}", s.model.last_recomputed);

    let body_boxes = |m: &solidcraft_doc::Model| {
        m.state()
            .bodies
            .iter()
            .map(|b| {
                let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
                (b.name.clone(), bb.min, bb.max)
            })
            .collect::<Vec<_>>()
    };
    let cached = body_boxes(&s.model);
    let expected = body_boxes(&fresh);
    assert_eq!(cached.len(), expected.len());
    for ((cn, cmin, cmax), (en, emin, emax)) in cached.iter().zip(&expected) {
        assert_eq!(cn, en);
        assert!(cmin.dist(*emin) < 1e-3 && cmax.dist(*emax) < 1e-3, "{cn}: cached ({cmin:?}, {cmax:?}) vs fresh ({emin:?}, {emax:?})");
    }
}

/// Moving component C2 owning a 2D planar guide rail (`rails`) on `solid.sweep` invalidates the
/// Sweep in C1 and matches fresh `Model::no_cache` evaluation.
#[test]
fn moving_component_with_planar_sweep_rail_recomputes_railed_sweep() {
    let mut s = Session::default();
    let c1 = run(&mut s, "component.create", json!({"name": "C1", "grounded": false}))["component"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Profile"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10, "id": "c"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Path"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -40]], "ids": ["up"]}));
    run(&mut s, "sketch.finish", json!({}));

    let c2_occ = run(&mut s, "component.create", json!({"name": "C2", "grounded": false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "RailSketch"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 0], [5, -40]], "ids": ["in"]}));
    run(&mut s, "sketch.finish", json!({}));

    run(&mut s, "component.activate", json!({"component": c1}));
    run(
        &mut s,
        "solid.sweep",
        json!({
            "sketch": "Profile",
            "path_sketch": "Path",
            "path": ["up"],
            "rails": [{"sketch": "RailSketch", "curves": ["in"]}],
            "operation": "new",
            "body_name": "Frustum"
        }),
    );
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);

    run(&mut s, "occurrence.move", json!({"occurrence": c2_occ, "translate": [4, 0, 0], "capture": true}));
    let fresh = fresh_no_cache_model(&s.doc);
    assert!(s.model.last_recomputed > 0, "expected recomputation after moving C2, got last_recomputed={}", s.model.last_recomputed);
    let measure = |m: &solidcraft_doc::Model| solidcraft_kernel::measure(&m.state().body("Frustum").unwrap().body).unwrap();
    let cached = measure(&s.model);
    let expected = measure(&fresh);
    assert!((cached.volume - expected.volume).abs() < 1e-3, "cached vol {} vs fresh {}", cached.volume, expected.volume);
    assert!((cached.area - expected.area).abs() < 1e-3, "cached area {} vs fresh {}", cached.area, expected.area);
}

// ---- moved-component path/rail tests

/// Scene: `Tube` pipe in C1 along a planar XZ path owned by C2 (nested under `Asm` when
/// `nested`). Returns the session and the occurrence id to move (C2, or `Asm` when nested).
fn planar_pipe_scene(nested: bool) -> (Session, u64) {
    let mut s = Session::default();
    let mut mover = 0;
    let mut owner_parent = 0;
    if nested {
        let asm = run(&mut s, "component.create", json!({"name": "Asm", "grounded": false}));
        mover = asm["occurrence"].as_u64().unwrap();
        owner_parent = asm["component"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component": 0}));
    }
    let c1 = run(&mut s, "component.create", json!({"name": "C1", "grounded": false}))["component"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": owner_parent}));
    let c2 = run(&mut s, "component.create", json!({"name": "C2", "grounded": false}));
    if !nested {
        mover = c2["occurrence"].as_u64().unwrap();
    }
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "PipePath"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -20]], "ids": ["p1"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "component.activate", json!({"component": c1}));
    run(&mut s, "solid.pipe", json!({"path_sketch": "PipePath", "path": ["p1"], "diameter": 4, "operation": "new", "body_name": "Tube"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    (s, mover)
}

fn tube_box(m: &solidcraft_doc::Model) -> (solidcore_record::Vec3, solidcore_record::Vec3) {
    let b = solidcraft_kernel::measure(&m.state().body("Tube").unwrap().body).unwrap().bbox;
    (b.min, b.max)
}

fn assert_tube_fresh(s: &Session, what: &str) {
    let fresh = fresh_no_cache_model(&s.doc);
    let (cmin, cmax) = tube_box(&s.model);
    let (fmin, fmax) = tube_box(&fresh);
    assert!(cmin.dist(fmin) < 1e-3 && cmax.dist(fmax) < 1e-3, "{what}: cached ({cmin:?},{cmax:?}) vs fresh ({fmin:?},{fmax:?})");
}

/// the owner of the planar path is nested under a moved assembly; the pipe's component is not.
#[test]
fn moving_assembly_around_planar_path_owner_recomputes_pipe() {
    let (mut s, asm_occ) = planar_pipe_scene(true);
    run(&mut s, "occurrence.move", json!({"occurrence": asm_occ, "translate": [7, 0, 0], "capture": true}));
    assert_tube_fresh(&s, "after nested move");
}

/// undo and redo of the move keep the cached pipe equal to a fresh evaluation.
#[test]
fn undo_redo_of_moving_planar_path_owner_keeps_pipe_fresh() {
    let (mut s, occ) = planar_pipe_scene(false);
    let (before_min, _) = tube_box(&s.model);
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [5, 0, 0], "capture": true}));
    assert_tube_fresh(&s, "after move");
    let (moved_min, _) = tube_box(&s.model);
    assert!(moved_min.dist(before_min) > 1.0, "the move must change the pipe");
    run(&mut s, "edit.undo", json!({}));
    assert_tube_fresh(&s, "after undo");
    assert!(tube_box(&s.model).0.dist(before_min) < 1e-3, "undo restores the pipe");
    run(&mut s, "edit.redo", json!({}));
    assert_tube_fresh(&s, "after redo");
    assert!(tube_box(&s.model).0.dist(moved_min) < 1e-3, "redo restores the moved pipe");
}

/// a moved design saved and reopened evaluates to the same pipe the live session shows.
#[test]
fn save_reload_after_moving_planar_path_owner_matches_live_pipe() {
    let (mut s, occ) = planar_pipe_scene(false);
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [5, 0, 0], "capture": true}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let (lmin, lmax) = tube_box(&s.model);
    let (rmin, rmax) = tube_box(&reopened.model);
    assert!(lmin.dist(rmin) < 1e-3 && lmax.dist(rmax) < 1e-3, "live ({lmin:?},{lmax:?}) vs reopened ({rmin:?},{rmax:?})");
}

/// A plate with a hole, moved into another component, where the hole
/// is patterned afterwards. The copies are drilled from the carried hole, not from its authored
/// spot, so every copy cuts (or the command refuses): never a success that cuts nothing.
fn plate_with_hole_in_a_moved_component() -> (Session, f64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 30, "height": 8, "body_name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [10, 15, 8], "diameter": 6, "depth": 2, "name": "H"}));
    let one = PLATE_574 - volume(&mut s);
    let c = run(&mut s, "component.create", json!({"name": "Arm", "grounded": false}));
    let arm = c["component"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": "root"}));
    let occ = s.doc.occurrences.iter().find(|o| o.component == arm).map(|o| o.id).unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 80, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Plate"], "component": arm}));
    (s, one)
}

const PLATE_574: f64 = 100.0 * 30.0 * 8.0;

#[test]
fn patterns_of_a_hole_after_a_body_transfer_cut_every_copy() {
    for circular in [false, true] {
        for arm_active in [true, false] {
            let (mut s, one) = plate_with_hole_in_a_moved_component();
            let seen = volume(&mut s);
            assert!((PLATE_574 - seen - one).abs() < 1e-3 * one, "the transfer keeps the hole");
            let (cmd, mut p) = if circular {
                (
                    "solid.pattern.circular",
                    // World coordinates: the transfer keeps the plate where it is shown.
                    json!({"features": ["H"], "axis": {"origin": [20, 15, 0], "dir": [0, 0, 1]}, "count": 3, "angle": 90}),
                )
            } else {
                ("solid.pattern.rectangular", json!({"features": ["H"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20}))
            };
            if arm_active {
                run(&mut s, "component.activate", json!({"component": "Arm"}));
            } else {
                p["participants"] = json!(["Plate"]);
            }
            let tag = format!("circular {circular}, arm active {arm_active}");
            match s.execute(cmd, &p) {
                Ok(_) => {
                    let cut = PLATE_574 - volume(&mut s);
                    assert!((cut - 3.0 * one).abs() < 1e-3 * one, "{tag}: success must cut 3 holes: {cut} vs 3 x {one}");
                    run(&mut s, "edit.undo", json!({}));
                    assert!((PLATE_574 - volume(&mut s) - one).abs() < 1e-3 * one, "{tag}: undo");
                    run(&mut s, "edit.redo", json!({}));
                    assert!((PLATE_574 - volume(&mut s) - 3.0 * one).abs() < 1e-3 * one, "{tag}: redo");
                }
                Err(e) => assert!(!e.to_string().is_empty(), "{tag}"),
            }
        }
    }
}

/// The other order: the pattern first, then the transfer, keeps every copy.
#[test]
fn patterns_of_a_hole_before_a_body_transfer_keep_every_copy() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 30, "height": 8, "body_name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [10, 15, 8], "diameter": 6, "depth": 2, "name": "H"}));
    let one = PLATE_574 - volume(&mut s);
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["H"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20}));
    let c = run(&mut s, "component.create", json!({"name": "Arm", "grounded": false}));
    let arm = c["component"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component": "root"}));
    let occ = s.doc.occurrences.iter().find(|o| o.component == arm).map(|o| o.id).unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 80, 0]}));
    run(&mut s, "component.move_bodies", json!({"bodies": ["Plate"], "component": arm}));
    assert!((PLATE_574 - volume(&mut s) - 3.0 * one).abs() < 1e-3 * one);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert!((PLATE_574 - volume(&mut s) - 3.0 * one).abs() < 1e-3 * one);
}
