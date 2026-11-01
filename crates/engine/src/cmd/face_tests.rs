#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind, PlaneRef};
use solidcraft_geom::Vec3;
use std::f64::consts::PI;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}
fn check(s: &mut Session, want: f64, hole: bool, height: f64) {
    s.refresh();
    assert!(s.model.results.iter().all(|result| result.error.is_none() && result.warning.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    let got: f64 = state.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum();
    // Legacy linked outlines fit tessellated curves; enforce the existing 1e-4 material envelope.
    assert!((got - want).abs() < want * 1e-4, "{got} vs {want}");
    assert!(state.bodies.iter().any(|b| b.body.contains(Vec3::new(5.0, 5.0, height - 1.0))));
    assert!(!state.bodies.iter().any(|b| b.body.contains(Vec3::new(5.0, 5.0, height + 1.0))));
    if hole {
        assert!(!state.bodies.iter().any(|b| b.body.contains(Vec3::new(20.0, 15.0, height - 1.0))));
    }
}
#[test]
fn generated_face_profiles_keep_exact_links_live_after_edit_and_reopen() {
    for hole in [false, true] {
        let mut s = Session::default();
        let base = run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"name":"Base"}))["feature"].as_u64().unwrap();
        if hole {
            run(&mut s, "sketch.create", json!({"plane":"XY"}));
            run(&mut s, "sketch.circle.center", json!({"center":[20,15],"radius":5}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.extrude", json!({"through_all":true,"operation":"cut"}));
        }
        let result = run(&mut s, "solid.extrude", json!({"face":[5,5,20],"distance":10,"operation":"new","name":"Pad"}));
        let pad = s.doc.feature(result["feature"].as_u64().unwrap()).unwrap();
        let FeatureKind::Extrude { sketch, .. } = pad.kind else { panic!("extrude") };
        let generated = s.doc.feature(sketch.0).unwrap();
        let FeatureKind::Sketch { plane, sketch: profile } = &generated.kind else { panic!("sketch") };
        let PlaneRef::Face { anchor: Some(anchor), .. } = plane else { panic!("the original Box cap remains exact after a hole cut") };
        assert_eq!(anchor.target.origin_feature, base);
        assert!(matches!(anchor.target.certificate, Some(solidcraft_doc::config::FaceCertificate::Box { axis: 2, high: true })));
        assert!(!profile.links.is_empty());
        assert!(profile.links.iter().all(|link| link.auto_face && !link.lost));
        let area = 1200.0 - if hole { 25.0 * PI } else { 0.0 };
        check(&mut s, area * 30.0, hole, 30.0);
        run(&mut s, "timeline.edit", json!({"feature":"Base","set":{"height":"25","length":"50"}}));
        let area = 1500.0 - if hole { 25.0 * PI } else { 0.0 };
        check(&mut s, area * 35.0, hole, 35.0);
        let edited = s.model.state();
        assert!(edited.bodies.iter().any(|body| body.body.contains(Vec3::new(49.0, 5.0, 34.0))));
        assert!(!edited.bodies.iter().any(|body| body.body.contains(Vec3::new(51.0, 5.0, 34.0))));
        assert!(edited.sketch(sketch.0).unwrap().sketch.links.iter().all(|link| link.auto_face && !link.lost));
        let solved = edited.sketch(sketch.0).unwrap().plane;
        assert!((solved.origin.z - 25.0).abs() < 1e-6);
        let before = s.model.state();
        let before_volume: f64 = before.bodies.iter().map(|body| solidcraft_kernel::measure(&body.body).unwrap().volume).sum();
        let before_bounds: Vec<_> = before.bodies.iter().map(|body| (body.name.clone(), body.mesh().bounds())).collect();
        let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
        check(&mut reopened, area * 35.0, hole, 35.0);
        let after = reopened.model.state();
        let after_volume: f64 = after.bodies.iter().map(|body| solidcraft_kernel::measure(&body.body).unwrap().volume).sum();
        assert!((before_volume - after_volume).abs() < 1e-6);
        for (name, bounds) in before_bounds {
            let cold = after.body(&name).unwrap().mesh().bounds();
            assert!(bounds.min.dist(cold.min) < 1e-9 && bounds.max.dist(cold.max) < 1e-9);
        }
    }
}
#[test]
fn a_component_extruded_from_another_components_face_moves_independently() {
    let mut s = Session::default();
    let reference = run(&mut s, "component.create", json!({"name":"Reference","grounded":false}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":20,"name":"Base","body_name":"Stock"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let part = run(&mut s, "component.create", json!({"name":"Part"}));
    run(&mut s, "solid.extrude", json!({"face":[10,10,20],"distance":10,"operation":"new","body_name":"Pad"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let measure = |s: &mut Session, center: [f64; 3], volume: f64| {
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
        let got = run(s, "inspect.center_of_mass", json!({"bodies":["Pad"]}));
        for (i, want) in center.iter().enumerate() {
            assert!((got["center"][i].as_f64().unwrap() - want).abs() < 1e-6, "{got}; expected {center:?}");
        }
        assert!((got["volume"].as_f64().unwrap() - volume).abs() < 1e-6, "{got}");
    };
    measure(&mut s, [10., 10., 25.], 4000.);
    run(&mut s, "occurrence.move", json!({"occurrence":part["occurrence"],"translate":[40,0,0],"capture":true}));
    measure(&mut s, [50., 10., 25.], 4000.);
    run(&mut s, "edit.undo", json!({}));
    measure(&mut s, [10., 10., 25.], 4000.);
    run(&mut s, "edit.redo", json!({}));
    measure(&mut s, [50., 10., 25.], 4000.);
    // Moving the reference is an assembly placement, not an edit to its shape.
    run(&mut s, "occurrence.move", json!({"occurrence":reference["occurrence"],"translate":[0,50,0],"capture":true}));
    measure(&mut s, [50., 10., 25.], 4000.);
    // Both the attached plane and its projected outline still follow source dimensions.
    run(&mut s, "timeline.edit", json!({"feature":"Base","set":{"length":"30","height":"25"}}));
    measure(&mut s, [55., 10., 30.], 6000.);
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    measure(&mut reopened, [55., 10., 30.], 6000.);
    run(&mut reopened, "occurrence.move", json!({"occurrence":part["occurrence"],"translate":[0,0,10],"capture":true}));
    measure(&mut reopened, [55., 10., 40.], 6000.);
    run(&mut reopened, "timeline.copy", json!({"features":["Base","Sketch1","Extrude1"]}));
    run(&mut reopened, "timeline.paste", json!({"translate":[0,0,50]}));
    let copy = run(&mut reopened, "inspect.center_of_mass", json!({"bodies":["Pad (2)"]}));
    for (i, want) in [55., 10., 90.].iter().enumerate() {
        assert!((copy["center"][i].as_f64().unwrap() - want).abs() < 1e-6, "{copy}");
    }
    assert!((copy["volume"].as_f64().unwrap() - 6000.).abs() < 1e-6);
    // Reorganizing the feature definitions must preserve the captured context too.
    run(&mut reopened, "component.move_features", json!({"features":["Base","Sketch1","Extrude1"],"component":0}));
    measure(&mut reopened, [55., 10., 40.], 6000.);
}

#[test]
fn a_cross_component_face_profile_keeps_its_rotated_creation_context() {
    let mut s = Session::default();
    let source = run(&mut s, "component.create", json!({"name":"Reference","grounded":false}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":20,"body_name":"Stock"}));
    run(&mut s, "occurrence.move", json!({"occurrence":source["occurrence"],"translate":[30,20,0],"angle":"90 deg"}));
    let part = run(&mut s, "component.create", json!({"name":"Part","parent":0}));
    run(&mut s, "occurrence.move", json!({"occurrence":part["occurrence"],"translate":[-20,10,0],"angle":"-90 deg","capture":false}));
    run(&mut s, "solid.extrude", json!({"face":[20,30,20],"distance":10,"operation":"new","body_name":"Pad"}));
    assert!(s.pending_moves.is_empty(), "face capture records the author's pending placement");
    run(&mut s, "component.activate", json!({"component":0}));
    let center = |s: &mut Session, want: [f64; 3]| {
        let got = run(s, "inspect.center_of_mass", json!({"bodies":["Pad"]}));
        for (i, want) in want.iter().enumerate() {
            assert!((got["center"][i].as_f64().unwrap() - want).abs() < 1e-6, "{got}");
        }
        assert!((got["volume"].as_f64().unwrap() - 4000.).abs() < 1e-6);
    };
    center(&mut s, [20., 30., 25.]);
    run(&mut s, "occurrence.move", json!({"occurrence":part["occurrence"],"translate":[50,0,5],"angle":"90 deg","capture":false}));
    center(&mut s, [20., 20., 30.]);
    run(&mut s, "component.capture_position", json!({}));
    center(&mut s, [20., 20., 30.]);
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    center(&mut reopened, [20., 20., 30.]);
}

#[test]
fn moving_the_original_face_derived_component_does_not_move_its_copies_backwards() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Reference"}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":20}));
    let part = run(&mut s, "component.create", json!({"name":"Part","parent":0}));
    run(&mut s, "solid.extrude", json!({"face":[10,10,20],"distance":10,"operation":"new","body_name":"Pad"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let copy = run(&mut s, "occurrence.copy", json!({"component":part["component"],"translate":[0,40,0]}));
    run(&mut s, "occurrence.copy", json!({"component":part["component"],"translate":[0,80,0]}));
    run(&mut s, "occurrence.move", json!({"occurrence":copy["occurrence"],"translate":[10,0,0],"capture":true}));
    let centers = |s: &mut Session, original: [f64; 3]| {
        for (body, want) in [("Pad", original), ("Pad (Part:2)", [20., 50., 25.]), ("Pad (Part:3)", [10., 90., 25.])] {
            let got = run(s, "inspect.center_of_mass", json!({"bodies":[body]}));
            for (i, want) in want.iter().enumerate() {
                assert!((got["center"][i].as_f64().unwrap() - want).abs() < 1e-6, "{body}: {got}");
            }
            assert!((got["volume"].as_f64().unwrap() - 4000.).abs() < 1e-6, "{got}");
        }
    };
    centers(&mut s, [10., 10., 25.]);
    run(&mut s, "occurrence.move", json!({"occurrence":part["occurrence"],"translate":[40,0,0],"capture":true}));
    centers(&mut s, [50., 10., 25.]);
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    centers(&mut reopened, [50., 10., 25.]);
    run(&mut reopened, "occurrence.move", json!({"occurrence":part["occurrence"],"angle":"90 deg","capture":true}));
    centers(&mut reopened, [-10., 50., 25.]);
}

#[test]
fn internal_capture_does_not_bypass_missing_ambiguous_or_invalid_path_picks() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"A"}));
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"B"}));
    let before = s.doc.to_json();
    assert!(s.execute("solid.extrude", &json!({"face":[5,5,20],"distance":10,"operation":"new"})).is_err());
    assert_eq!(s.doc.to_json(), before);
    for pick in [json!({"face":[5,5,20]}), json!({"face":[500,5,20]}), json!({"face":[5,5,20],"body":"A","index":0,"path":[999999]})] {
        assert!(super::super::sketch::face::capture_internal(&s, &pick, "internal").is_err(), "{pick}");
    }
}

#[test]
fn generated_profiles_and_press_pull_convert_rotated_occurrence_picks_once() {
    for hole in [false, true] {
        let mut s = Session::default();
        let component = run(&mut s, "component.create", json!({"name":"Arm","grounded":false}));
        let cid = component["component"].as_u64().unwrap();
        let occ = component["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"name":"Base","body_name":"Stock"}));
        if hole {
            run(&mut s, "sketch.create", json!({"plane":"XY"}));
            run(&mut s, "sketch.circle.center", json!({"center":[20,15],"radius":5}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.extrude", json!({"through_all":true,"operation":"cut"}));
        }
        run(&mut s, "component.activate", json!({"component":0}));
        run(&mut s, "occurrence.move", json!({"occurrence":occ,"translate":[30,20,10],"axis":[1,0,0],"angle":"90 deg","capture":true}));
        run(&mut s, "component.activate", json!({"component":cid,"occurrence":occ}));
        run(&mut s, "solid.extrude", json!({"face":[35,0,15],"distance":10,"operation":"join"}));
        let area = 1200.0 - if hole { 25.0 * PI } else { 0.0 };
        check(&mut s, area * 30.0, hole, 30.0);
        run(&mut s, "solid.press_pull", json!({"face":[35,-10,15],"distance":5}));
        check(&mut s, area * 35.0, hole, 35.0);
        let before_cut = s.doc.to_json();
        run(&mut s, "solid.press_pull", json!({"face":[35,-15,15],"distance":-5}));
        let height = 30.0;
        check(&mut s, area * height, hole, height);
        let after_cut = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before_cut);
        check(&mut s, area * 35.0, hole, 35.0);
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.to_json(), after_cut);
        check(&mut s, area * height, hole, height);
        let world = s.world_state();
        let occupied = Vec3::new(35.0, 21.0 - height, 15.0);
        let absent = Vec3::new(35.0, 19.0 - height, 15.0);
        assert!(world.bodies.iter().any(|body| body.body.contains(occupied)));
        assert!(!world.bodies.iter().any(|body| body.body.contains(absent)));
        if hole {
            assert!(!world.bodies.iter().any(|body| body.body.contains(Vec3::new(50.0, 21.0 - height, 25.0))));
        }
        let before_volume: f64 = world.bodies.iter().map(|body| solidcraft_kernel::measure(&body.body).unwrap().volume).sum();
        let before_bounds: Vec<_> = world.bodies.iter().map(|body| (body.name.clone(), body.mesh().bounds())).collect();
        let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
        let after = reopened.world_state();
        assert!(after.bodies.iter().any(|body| body.body.contains(occupied)));
        let v: f64 = after.bodies.iter().map(|body| solidcraft_kernel::measure(&body.body).unwrap().volume).sum();
        assert!((v - area * height).abs() < area * height * 1e-4);
        assert!((v - before_volume).abs() < 1e-6);
        for (name, bounds) in before_bounds {
            let cold = after.body(&name).unwrap().mesh().bounds();
            assert!(bounds.min.dist(cold.min) < 1e-9 && bounds.max.dist(cold.max) < 1e-9);
        }
    }
}

/// A 40 x 30 x 20 box whose top is split at x = 20 and its left half raised 5 by Offset Face:
/// the raised top has no construction certificate and no exact history name yet (#494).
fn raised_half(s: &mut Session, body: &str) {
    run(s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":body}));
    run(s, "solid.split_face", json!({"faces":[[10,15,20]],"plane":{"origin":[20,0,0],"normal":[1,0,0]}}));
    run(s, "solid.offset_face", json!({"faces":[[10,15,20]],"distance":5}));
}

#[test]
fn legacy_internal_capture_refuses_an_uncaptured_source_in_another_component() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Source","grounded":false}));
    let occurrence = component["occurrence"].as_u64().unwrap();
    // A face Offset Face raised has no construction certificate or exact history name yet:
    // the legacy path.
    raised_half(&mut s, "Stock");
    run(&mut s, "component.activate", json!({"component":0}));
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,0,0],"capture":false}));
    let state = s.model.state();
    // The raised half's top.
    let end = state
        .body("Stock")
        .unwrap()
        .body
        .faces(1e-4)
        .unwrap()
        .into_iter()
        .find(|face| face.plane_normal.is_some_and(|normal| normal.z > 0.999) && (face.centroid.z - 25.0).abs() < 1e-6)
        .unwrap();
    let before = s.doc.to_json();
    let error = super::super::sketch::face::capture_internal(
        &s,
        &json!({"face":[40,15,25],"body":"Stock","index":end.index,"path":[occurrence]}),
        "internal",
    )
    .unwrap_err();
    assert!(error.to_string().contains("capture the source occurrence position"), "{error}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn repeated_source_face_profile_requires_and_uses_the_exact_active_occurrence() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Repeated"}));
    let cid = component["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Stock"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let copied = run(&mut s, "occurrence.copy", json!({"component":cid,"translate":[50,0,0]}));
    let occurrence = copied["occurrence"].as_u64().unwrap();
    let before = s.doc.to_json();
    let error = s.execute("solid.extrude", &json!({"face":[55,5,10],"distance":5,"operation":"new"})).unwrap_err();
    assert!(error.to_string().contains("activate the picked occurrence"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "component.activate", json!({"component":cid,"occurrence":occurrence}));
    run(&mut s, "solid.extrude", json!({"face":[55,5,10],"distance":5,"operation":"new"}));
    let state = s.world_state();
    for x in [5.0, 55.0] {
        assert!(state.bodies.iter().any(|body| body.body.contains(Vec3::new(x, 5.0, 14.0))));
        assert!(!state.bodies.iter().any(|body| body.body.contains(Vec3::new(x, 5.0, 16.0))));
    }
}

/// #307: four pairs of holes touching at one line each (a ring pattern against a grid). Every
/// contact is the same up to rounding, so every hole is one face: 33 holes and 6 planes.
#[test]
fn tangent_hole_contacts_split_no_faces() {
    let recipe: Value = serde_json::from_str(RECIPE_307).unwrap();
    let mut s = Session::default();
    for c in recipe["commands"].as_array().unwrap() {
        s.execute(c["command"].as_str().unwrap(), &c["params"]).unwrap();
    }
    let m = s.execute("inspect.measure", &json!({})).unwrap();
    let body = &m["bodies"][0];
    assert_eq!(body["face_types"], json!({"cylinder": 33, "plane": 6}), "{m}");
    assert_eq!(body["faces"], 39, "{m}");
}

const RECIPE_307: &str = r#"{"commands": [{"command": "sketch.create", "params": {"name": "Plate_sketch", "plane": "XY"}}, {"command": "sketch.line", "params": {"construction": false, "ids": ["l1"], "points": [[-50, -50], [50, -50]]}}, {"command": "sketch.line", "params": {"construction": false, "ids": ["l2"], "points": ["l1.end", [50, 50]]}}, {"command": "sketch.line", "params": {"construction": false, "ids": ["l3"], "points": ["l2.end", [-50, 50]]}}, {"command": "sketch.line", "params": {"construction": false, "ids": ["l4"], "points": ["l3.end", "l1.start"]}}, {"command": "sketch.finish", "params": {}}, {"command": "solid.extrude", "params": {"body_names": ["Plate"], "direction": "positive", "distance": 5, "name": "Plate", "operation": "new", "profiles": [{"loops": [{"curves": ["l1", "l2", "l3", "l4"], "outer": true}], "sketch": "Plate_sketch"}], "sketch": "Plate_sketch"}}, {"command": "sketch.create", "params": {"name": "HoleA", "plane": "XY"}}, {"command": "sketch.circle.center", "params": {"center": [-30, -30], "construction": false, "id": "c1", "radius": 5}}, {"command": "sketch.finish", "params": {}}, {"command": "solid.extrude", "params": {"body_names": ["Plate"], "direction": "positive", "name": "HoleA", "operation": "cut", "profiles": [{"loops": [{"curves": ["c1"], "outer": true}], "sketch": "HoleA"}], "sketch": "HoleA", "through_all": true}}, {"command": "solid.pattern.rectangular", "params": {"count1": 5, "count2": 5, "dir1": [1, 0, 0], "dir2": [0, 1, 0], "features": ["HoleA"], "name": "GridPattern", "spacing1": 15, "spacing2": 15}}, {"command": "sketch.create", "params": {"name": "HoleB", "plane": "XY"}}, {"command": "sketch.circle.center", "params": {"center": [25, 0], "construction": false, "id": "c1", "radius": 5}}, {"command": "sketch.finish", "params": {}}, {"command": "solid.extrude", "params": {"body_names": ["Plate"], "direction": "positive", "name": "HoleB", "operation": "cut", "profiles": [{"loops": [{"curves": ["c1"], "outer": true}], "sketch": "HoleB"}], "sketch": "HoleB", "through_all": true}}, {"command": "solid.pattern.circular", "params": {"angle": 360, "axis": {"dir": [0, 0, 1], "origin": [0, 0, 0]}, "count": 8, "features": ["HoleB"], "name": "RingPattern"}}]}"#;

/// #308: a blind cut ending inside a cylinder leaves thin strips of the cylinder wall beside
/// the cut; they must count as cylinder, not as a sphere or a freeform face.
#[test]
fn blind_cut_inside_a_cylinder_keeps_only_planes_and_cylinders() {
    let mut s = Session::default();
    let side = json!({"origin":[0,0,0],"x_dir":[0,0,-1],"y_dir":[0,1,0]});
    s.execute("solid.box", &json!({"length":40,"width":30,"height":3,"body_name":"Hinge"})).unwrap();
    for (name, radius, operation, extent) in
        [("Knuckle", 4, "join", json!({"distance":40})), ("Pin", 2, "cut", json!({"distance":100,"direction":"symmetric"}))]
    {
        s.execute("sketch.create", &json!({"name":name,"plane":side})).unwrap();
        s.execute("sketch.circle.center", &json!({"center":[-4,33],"radius":radius})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        let mut p = json!({"sketch":name,"operation":operation,"body_names":["Hinge"]});
        p.as_object_mut().unwrap().extend(extent.as_object().unwrap().clone());
        s.execute("solid.extrude", &p).unwrap();
    }
    s.execute("sketch.create", &json!({"name":"Gap","plane":"XY"})).unwrap();
    s.execute("sketch.rectangle.two_point", &json!({"p0":[12,29],"p1":[28,40]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    s.execute("solid.extrude", &json!({"sketch":"Gap","operation":"cut","distance":3.3,"body_names":["Hinge"]})).unwrap();
    let m = s.execute("inspect.measure", &json!({})).unwrap();
    let body = &m["bodies"][0];
    assert_eq!(body["face_types"], json!({"cylinder": 2, "plane": 12}), "{m}");
    assert_eq!(body["faces"], 14, "{m}");
}

fn face_anchor(s: &Session, sketch: u64) -> solidcraft_doc::FaceAnchor {
    let FeatureKind::Sketch { plane: PlaneRef::Face { anchor: Some(anchor), .. }, .. } = &s.doc.feature(sketch).unwrap().kind else {
        panic!("sketch {sketch} has no face anchor")
    };
    (**anchor).clone()
}

#[test]
fn join_keeps_exact_face_attachments_from_both_operands() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-10,-10],"p1":[10,10]}));
    run(&mut s, "sketch.finish", json!({}));
    let base = run(&mut s, "solid.extrude", json!({"distance":40,"name":"Base"}))["feature"].as_u64().unwrap();
    // A cross bar joined through the base (the shape of #420) and a boss joined on its top.
    run(&mut s, "sketch.create", json!({"plane":"YZ"}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,20],"radius":5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":20,"operation":"join"}));
    run(&mut s, "sketch.create", json!({"plane":{"face":[0,0,40]}}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":4}));
    run(&mut s, "sketch.finish", json!({}));
    let boss = run(&mut s, "solid.extrude", json!({"distance":10,"operation":"join","name":"Boss"}))["feature"].as_u64().unwrap();
    assert_eq!(s.model.state().bodies.len(), 1);

    // The base's top around the boss, and the boss's own top (the Union's second operand).
    let on_base = run(&mut s, "sketch.create", json!({"plane":{"face":[8,8,40]}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let on_boss = run(&mut s, "sketch.create", json!({"plane":{"face":[0,0,50]}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(face_anchor(&s, on_base).target.origin_feature, base);
    assert_eq!(face_anchor(&s, on_boss).target.origin_feature, boss);

    // The attachments follow their faces through an upstream edit and a reopen.
    run(&mut s, "timeline.edit", json!({"feature":"Base","set":{"extent":{"distance":"50"}}}));
    s.refresh();
    assert!(s.model.results.iter().all(|result| result.error.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    assert!((state.sketch(on_base).unwrap().plane.origin.z - 50.0).abs() < 1e-6);
    assert!((state.sketch(on_boss).unwrap().plane.origin.z - 60.0).abs() < 1e-6);
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    reopened.refresh();
    assert!(reopened.model.results.iter().all(|result| result.error.is_none()), "{:?}", reopened.model.results);
    assert!((reopened.model.state().sketch(on_boss).unwrap().plane.origin.z - 60.0).abs() < 1e-6);
}

#[test]
fn join_gives_a_face_merged_from_both_operands_its_history_name() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10}));
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"corner":[20,0,0],"operation":"join"}));
    // Both boxes' tops share one support: no single construction certificate owns it, but its
    // exact history names it (#491).
    let top = run(&mut s, "sketch.create", json!({"plane":{"face":[10,10,10]}}))["sketch"].as_u64().unwrap();
    assert_eq!(face_anchor(&s, top).target.certificate, Some(solidcraft_doc::config::FaceCertificate::Lineage));
    // An unshared support keeps its box role.
    run(&mut s, "sketch.finish", json!({}));
    let side = run(&mut s, "sketch.create", json!({"plane":{"face":[0,10,5]}}))["sketch"].as_u64().unwrap();
    assert!(matches!(face_anchor(&s, side).target.certificate, Some(solidcraft_doc::config::FaceCertificate::Box { axis: 0, high: false })));
}

#[test]
fn join_covering_a_whole_attached_face_loses_the_attachment_without_moving_it() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":10,"name":"Block"}));
    let top = run(&mut s, "sketch.create", json!({"plane":{"face":[30,15,10]},"name":"Top"}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.circle.center", json!({"center":[30,15],"radius":3}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"Top","distance":5,"operation":"join","name":"Peg"}));
    // An earlier join whose roof replaces Block's whole top.
    run(&mut s, "timeline.roll_to", json!({"feature":"Block"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"B"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[-5,-5],"p1":[45,35]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch":"B","distance":20,"operation":"join","name":"Boss"}));
    run(&mut s, "timeline.roll_to", json!({}));
    let failed = |s: &Session, id: u64| s.model.results.iter().find(|r| r.id == id).unwrap().error.is_some();
    assert!(failed(&s, top), "the attachment never follows the substitute roof");
    assert!(s.model.state().sketch(top).is_none());
    run(&mut s, "timeline.suppress", json!({"feature":"Boss","suppressed":true}));
    assert!(!failed(&s, top));
    assert!((s.model.state().sketch(top).unwrap().plane.origin.z - 10.).abs() < 1e-9);
}

fn certificate_of(s: &Session, sketch: u64) -> Option<solidcraft_doc::config::FaceCertificate> {
    let FeatureKind::Sketch { plane: PlaneRef::Face { anchor: Some(anchor), .. }, .. } = &s.doc.feature(sketch).unwrap().kind else {
        return None;
    };
    anchor.target.certificate.clone()
}

#[test]
fn cylinder_primitive_caps_take_attachments_that_follow_edits_and_reopen() {
    use solidcraft_doc::config::FaceCertificate;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":40,"name":"Post"}));
    let top = run(&mut s, "sketch.create", json!({"plane":{"face":[2,1,40]}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    let bottom = run(&mut s, "sketch.create", json!({"plane":{"face":[2,1,0]}}))["sketch"].as_u64().unwrap();
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(certificate_of(&s, top), Some(FaceCertificate::CylinderCap { high: true }));
    assert_eq!(certificate_of(&s, bottom), Some(FaceCertificate::CylinderCap { high: false }));
    let heights = |s: &Session| {
        let state = s.model.state();
        (state.sketch(top).unwrap().plane.origin.z, state.sketch(bottom).unwrap().plane.origin.z)
    };
    run(&mut s, "timeline.edit", json!({"feature":"Post","set":{"height":"55","base":[0,0,5]}}));
    s.refresh();
    assert!(s.model.results.iter().all(|result| result.error.is_none()), "{:?}", s.model.results);
    let (high, low) = heights(&s);
    assert!((high - 60.).abs() < 1e-9 && (low - 5.).abs() < 1e-9, "{high} {low}");
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    reopened.refresh();
    assert!(reopened.model.results.iter().all(|result| result.error.is_none()), "{:?}", reopened.model.results);
    assert_eq!(heights(&reopened), (high, low));
}

#[test]
fn crossed_cylinder_primitives_keep_their_caps_attachable() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":40}));
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":40,"base":[-20,0,20],"axis":"X","operation":"join"}));
    assert_eq!(s.model.state().bodies.len(), 1);
    for face in [[0., 0., 40.], [20., 0., 20.]] {
        let sketch = run(&mut s, "sketch.create", json!({"plane":{"face":face}}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.finish", json!({}));
        assert!(certificate_of(&s, sketch).is_some(), "{face:?}");
    }
}

#[test]
fn shelled_cylinder_floor_keeps_its_cap_identity() {
    use solidcraft_doc::config::FaceCertificate;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":10,"height":40}));
    run(&mut s, "solid.shell", json!({"faces":[[2,1,40]],"thickness":2}));
    let floor = run(&mut s, "sketch.create", json!({"plane":{"face":[2,1,2]}}))["sketch"].as_u64().unwrap();
    assert!(
        matches!(
            certificate_of(&s, floor),
            Some(FaceCertificate::ShellPlanar { source, inner: true, .. }) if *source == FaceCertificate::CylinderCap { high: false }
        ),
        "{:?}",
        certificate_of(&s, floor)
    );
}
