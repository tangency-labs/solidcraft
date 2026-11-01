//! #226: a face's outward extrusion must not join an assembly neighbour it only touches.
#![allow(clippy::unwrap_used, clippy::panic)]

use crate::Session;
use serde_json::{Value, json};
use solidcraft_doc::{Document, FeatureKind};
use solidcraft_geom::Vec3;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

fn assembly(cap_first: bool) -> Session {
    let mut s = Session::default();
    let wall = json!({"length":40,"width":30,"height":20,"body_name":"Wall"});
    let cap = json!({"corner":[0,0,20],"length":40,"width":30,"height":5,"body_name":"Cap"});
    for params in if cap_first { [cap, wall] } else { [wall, cap] } {
        run(&mut s, "solid.box", params);
    }
    s
}

fn check_wall(s: &Session, count: usize) {
    assert!(s.model.results.iter().all(|result| result.error.is_none() && result.warning.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    assert_eq!(state.bodies.len(), count);
    let wall = state.body("Wall").unwrap();
    let measure = solidcraft_kernel::measure(&wall.body).unwrap();
    assert!((measure.volume - 30_000.0).abs() < 1e-6, "{measure:?}");
    assert!(measure.bbox.max.dist(Vec3::new(50.0, 30.0, 20.0)) < 1e-9);
    assert!(wall.body.contains(Vec3::new(49.0, 15.0, 10.0)));
    let cap = solidcraft_kernel::measure(&state.body("Cap").unwrap().body).unwrap();
    assert!((cap.volume - 6000.0).abs() < 1e-6);
    assert!(cap.bbox.max.dist(Vec3::new(40.0, 30.0, 25.0)) < 1e-9);
}

#[test]
fn face_join_and_press_pull_leave_the_edge_touching_cap_in_either_body_order() {
    for cap_first in [true, false] {
        for operation in ["join", "auto", "press_pull"] {
            let mut s = assembly(cap_first);
            let before = s.model.state();
            let params = json!({"face":[40,15,10],"distance":10,"operation":operation});
            let command = if operation == "press_pull" { "solid.press_pull" } else { "solid.extrude" };
            let result = run(&mut s, command, params);
            assert_eq!(result["changed_bodies"], json!(["Wall"]));
            assert_eq!(result["removed_bodies"], json!([]));
            assert!(s.model.state().body("Cap").unwrap().body.same(&before.body("Cap").unwrap().body));
            check_wall(&s, 2);
            let saved = s.doc.to_json();
            run(&mut s, "edit.undo", json!({}));
            assert!(s.model.state().body("Wall").unwrap().body.same(&before.body("Wall").unwrap().body));
            run(&mut s, "edit.redo", json!({}));
            assert_eq!(s.doc.to_json(), saved);
            check_wall(&Session::new(Document::from_json(&saved).unwrap()), 2);
        }
    }
}

#[test]
fn face_join_respects_explicit_targets_and_participants() {
    for key in ["targets", "participants"] {
        for all in [false, true] {
            let mut s = assembly(!all);
            let mut params = json!({"face":[40,15,10],"distance":10,"operation":"join"});
            // Joining the wall first permits the cap's full base face to join afterwards.
            params[key] = if all { json!(["*"]) } else { json!(["Wall", "Cap"]) };
            run(&mut s, "solid.extrude", params);
            let state = s.model.state();
            assert_eq!(state.bodies.len(), 1, "{key}");
            assert!((solidcraft_kernel::measure(&state.body("Wall").unwrap().body).unwrap().volume - 36_000.0).abs() < 1e-6);
        }
    }
}

#[test]
fn face_join_maps_the_picked_instance_body_and_point_together() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Repeated"}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Wall"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let occurrence = run(&mut s, "occurrence.copy", json!({"component":component,"translate":[100,0,0]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "component.activate", json!({"component":component,"occurrence":occurrence}));
    let world = s.world_state();
    let body = world.bodies.iter().find(|body| body.body.contains(Vec3::new(139.0, 15.0, 10.0))).unwrap().name.clone();
    assert_ne!(body, "Wall", "exercise an instance body name");
    let result = run(&mut s, "solid.extrude", json!({"face":[140,15,10],"face_body":body,"distance":10,"operation":"join"}));
    let FeatureKind::Extrude { targets, .. } = &s.doc.feature(result["feature"].as_u64().unwrap()).unwrap().kind else { panic!("extrude") };
    assert_eq!(targets, &["Wall"]);
    let world = s.world_state();
    assert_eq!(world.bodies.len(), 2);
    for point in [Vec3::new(49.0, 15.0, 10.0), Vec3::new(149.0, 15.0, 10.0)] {
        assert!(world.bodies.iter().any(|body| body.body.contains(point)), "{point:?}");
    }
}

#[test]
fn face_join_of_a_step_import_keeps_all_32_neighbours_unchanged() {
    let mut source = assembly(true);
    for i in 0..31 {
        run(&mut source, "solid.box", json!({"corner":[100+20*i,100,0],"length":5,"width":5,"height":5,"body_name":format!("Distant{i}")}));
    }
    let step = solidcraft_io::export(&source.model.state(), &[], solidcraft_io::Format::Step, "Original assembly").unwrap();
    let imported = solidcraft_io::step_import_feature(&step, "original-assembly.step").unwrap();
    let mut doc = Document::new("Imported assembly");
    let feature = doc.add_feature(imported.kind, None).unwrap();
    doc.feature_mut(feature).unwrap().body_names = imported.body_names;
    let mut s = Session::new(doc);
    assert_eq!(s.doc.features.len(), 1);
    let before = s.model.state();
    let result = run(&mut s, "solid.extrude", json!({"face":[40,15,10],"distance":"10 mm","direction":"positive","operation":"join"}));
    assert_eq!(result["changed_bodies"], json!(["Wall"]));
    check_wall(&s, 33);
    let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    check_wall(&reopened, 33);
    for body in before.bodies.iter().filter(|body| body.name != "Wall") {
        let after = s.model.state();
        assert!(body.body.same(&after.body(&body.name).unwrap().body), "{}", body.name);
        let cold = reopened.model.state();
        let original = solidcraft_kernel::measure(&body.body).unwrap();
        let restored = solidcraft_kernel::measure(&cold.body(&body.name).unwrap().body).unwrap();
        assert!((original.volume - restored.volume).abs() < 1e-9, "{}", body.name);
        assert!(original.bbox.min.dist(restored.bbox.min) < 1e-9 && original.bbox.max.dist(restored.bbox.max) < 1e-9);
    }
    let FeatureKind::Extrude { targets, .. } = &s.doc.feature(result["feature"].as_u64().unwrap()).unwrap().kind else { panic!("extrude") };
    assert_eq!(targets, &["Wall"]);
}

/// #419: a face pick is resolved against the placed state: a leaf of an assembly that its
/// joint swung away does not make the pick ambiguous, even though both leaves are built at
/// the same local place. The pick lands on the leaf that is shown there.
#[test]
fn a_face_pick_on_an_assembly_leaf_is_resolved_where_the_leaf_is_shown() {
    let mut s = Session::default();
    for leaf in ["Leaf A", "Leaf B"] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": leaf}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
        run(&mut s, "solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "Leaf A:1", "grounded": true}));
    run(
        &mut s,
        "joint.create",
        json!({
            "type": "revolute",
            "name": "Hinge",
            "a": {"occurrence": "Leaf A:1", "circle": [1, 37, 4]},
            "b": {"occurrence": "Leaf B:1", "circle": [1, 37, 4]},
            "limits": [["0 deg", "180 deg"]]
        }),
    );
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 60}));
    // Leaf B is swung 60° away, so the pick is on Leaf A's top face, where it is shown.
    let result = run(
        &mut s,
        "solid.extrude",
        json!({"face": [13.002403831300967, 9.849781594495369, 3.0], "distance": "10 mm", "direction": "positive", "operation": "join"}),
    );
    assert_eq!(result["changed_bodies"], json!(["Body1"]));
    let state = s.model.state();
    let volume = solidcraft_kernel::measure(&state.body("Body1").unwrap().body).unwrap().volume;
    assert!((volume - 15_600.0).abs() < 1e-6, "{volume}");
}

/// #1349: a profile sketched on a body's face joins that body, and leaves alone a neighbour
/// whose face it lies on too: a rib on the side of a bracket, drawn down over the motor the
/// bracket sits on, welded the motor in (a join that ran for minutes on the imported motor and
/// failed). Explicit targets still join both.
#[test]
fn join_of_a_profile_sketched_on_a_face_leaves_a_neighbour_it_lies_on() {
    for targets in [None, Some(json!(["Bracket", "Motor"]))] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Motor"}));
        run(&mut s, "solid.box", json!({"corner":[0,0,20],"length":40,"width":30,"height":5,"body_name":"Bracket"}));
        let sketch = run(&mut s, "sketch.create", json!({"plane": {"face": [40, 15, 22.5]}}))["sketch"].as_u64().unwrap();
        let plane = s.model.state().sketch(sketch).unwrap().plane;
        let at = |y: f64, z: f64| {
            let p = plane.to_local(Vec3::new(40.0, y, z));
            json!([p.x, p.y])
        };
        // On the bracket's side and down over the motor's.
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": at(5.0, 10.0), "p1": at(25.0, 25.0)}));
        run(&mut s, "sketch.finish", json!({}));
        let mut params =
            json!({"sketch": sketch, "profiles": [{"point": at(15.0, 22.5)}, {"point": at(15.0, 15.0)}], "distance": 1, "operation": "join"});
        if let Some(t) = &targets {
            params["targets"] = t.clone();
        }
        let result = run(&mut s, "solid.extrude", params);
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
        let state = s.model.state();
        let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
        if targets.is_some() {
            assert_eq!(state.bodies.len(), 1);
            assert!((volume("Bracket") - (24_000.0 + 6000.0 + 300.0)).abs() < 1e-6, "{}", volume("Bracket"));
            continue;
        }
        assert_eq!(state.bodies.len(), 2);
        assert!((volume("Motor") - 24_000.0).abs() < 1e-6, "the motor is untouched");
        assert!((volume("Bracket") - (6000.0 + 300.0)).abs() < 1e-6, "{}", volume("Bracket"));
        let FeatureKind::Extrude { targets, .. } = &s.doc.feature(result["feature"].as_u64().unwrap()).unwrap().kind else { panic!("extrude") };
        assert_eq!(targets, &["Bracket"]);
    }
}

/// #6424: an extrude that runs to another body and joins it makes one body of the tool, the
/// body it was sketched on and the body it runs to.
#[test]
fn join_extrude_to_another_body_joins_that_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Base"}));
    run(&mut s, "solid.box", json!({"corner":[0,0,40],"length":40,"width":40,"height":20,"body_name":"Target"}));
    // A profile on the base's top face, extruded up to the target's bottom face.
    let sketch = run(&mut s, "sketch.create", json!({"plane":{"face":[20,20,10]}}))["sketch"].as_u64().unwrap();
    let plane = s.model.state().sketch(sketch).unwrap().plane;
    let at = |x: f64, y: f64| {
        let p = plane.to_local(Vec3::new(x, y, 10.0));
        json!([p.x, p.y])
    };
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": at(10.0,10.0), "p1": at(30.0,30.0)}));
    run(&mut s, "sketch.finish", json!({}));
    // The join names the body the profile was sketched on; the body it runs to joins too.
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"to":[20,20,40],"operation":"join","targets":["Base"]}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
    assert_eq!(state.bodies.len(), 1, "the tool, the base and the target are one body");
    assert!((volume("Base") - 60_000.0).abs() < 1e-6, "the target took the tool: {}", volume("Base"));
    assert_eq!(state.body("Base").unwrap().body.lumps().map_or(1, |l| l.len()), 1, "one piece");
}
