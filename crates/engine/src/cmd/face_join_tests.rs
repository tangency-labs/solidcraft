//! #226: a face's outward extrusion must not join an assembly neighbour it only touches.
#![allow(clippy::unwrap_used, clippy::panic)]

use crate::Session;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::{Document, FeatureKind};
use solidcraft_geom::prelude::*;

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
    assert_eq!(wall.body.classify_point(Vec3::new(49.0, 15.0, 10.0)).unwrap(), solidcraft_kernel::PointClass::Inside);
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
fn press_pull_selects_either_body_at_a_shared_flat_face() {
    for (key, both) in [("body", false), ("face_body", false), ("body", true)] {
        for selected in ["Body1", "Body1 (1)"] {
            for distance in [-5, 5] {
                let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
                run(&mut s, "solid.box", json!({"length":20,"width":20,"height":20,"corner":[0,0,0]}));
                run(&mut s, "solid.split_body", json!({"body":"Body1","plane":{"origin":[0,0,10],"normal":[0,0,1]}}));
                let before = s.model.state();
                let mut params = json!({"face":[10,10,10],"distance":distance});
                params[key] = json!(selected);
                if both {
                    params["face_body"] = json!(if selected == "Body1" { "Body1 (1)" } else { "Body1" });
                }
                let result = run(&mut s, "solid.press_pull", params);
                assert_eq!(result["changed_bodies"], json!([selected]));
                let check = |s: &Session| {
                    let state = s.model.state();
                    assert_eq!(state.bodies.len(), 2);
                    for body in &state.bodies {
                        let expected = if body.name == selected { 4000.0 + 400.0 * f64::from(distance) } else { 4000.0 };
                        let measure = solidcraft_kernel::measure(&body.body).unwrap();
                        assert!((measure.volume - expected).abs() < 1e-6, "{key} {selected} {distance}: {measure:?}");
                    }
                };
                check(&s);
                let other = before.bodies.iter().find(|body| body.name != selected).unwrap();
                assert!(s.model.state().body(&other.name).unwrap().body.same(&other.body));
                let saved = s.doc.to_json();
                run(&mut s, "edit.undo", json!({}));
                assert!(s.model.state().body(selected).unwrap().body.same(&before.body(selected).unwrap().body));
                run(&mut s, "edit.redo", json!({}));
                assert_eq!(s.doc.to_json(), saved);
                check(&s);
                check(&Session::new(Document::from_json(&saved).unwrap()));
            }
        }
    }
}

#[test]
fn press_pull_reports_ambiguous_or_unknown_flat_face_bodies_without_changes() {
    let mut s = assembly(false);
    let before = s.doc.to_json();
    for selector in [None, Some("body"), Some("face_body")] {
        let mut params = json!({"face":[20,15,20],"distance":5});
        if let Some(key) = selector {
            params[key] = json!("Missing");
        }
        let error = s.execute("solid.press_pull", &params).unwrap_err();
        let crate::EngineError::BadParams { cmd, msg } = error else { panic!("{error}") };
        assert_eq!(cmd, "solid.press_pull");
        assert!(msg.contains(if selector.is_some() { "Missing" } else { "face_body" }), "{msg}");
        assert_eq!(s.doc.to_json(), before);
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
    for (command, key) in [("solid.extrude", "face_body"), ("solid.press_pull", "body"), ("solid.press_pull", "face_body")] {
        let mut s = Session::default();
        let component = run(&mut s, "component.create", json!({"name":"Repeated"}))["component"].as_u64().unwrap();
        run(&mut s, "solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Wall"}));
        run(&mut s, "component.activate", json!({"component":0}));
        let occurrence = run(&mut s, "occurrence.copy", json!({"component":component,"translate":[100,0,0]}))["occurrence"].as_u64().unwrap();
        run(&mut s, "component.activate", json!({"component":component,"occurrence":occurrence}));
        let world = s.world_state().unwrap();
        let body = world
            .bodies
            .iter()
            .find(|body| body.body.classify_point(Vec3::new(139.0, 15.0, 10.0)).unwrap() == solidcraft_kernel::PointClass::Inside)
            .unwrap()
            .name
            .clone();
        assert_ne!(body, "Wall", "exercise an instance body name");
        let mut params = json!({"face":[140,15,10],"distance":10});
        params[key] = json!(body);
        if command == "solid.extrude" {
            params["operation"] = json!("join");
        }
        let result = run(&mut s, command, params);
        let FeatureKind::Extrude { targets, .. } = &s.doc.feature(result["feature"].as_u64().unwrap()).unwrap().kind else { panic!("extrude") };
        assert_eq!(targets, &["Wall"]);
        let world = s.world_state().unwrap();
        assert_eq!(world.bodies.len(), 2);
        for point in [Vec3::new(49.0, 15.0, 10.0), Vec3::new(149.0, 15.0, 10.0)] {
            assert!(world.bodies.iter().any(|body| body.body.classify_point(point).unwrap() == solidcraft_kernel::PointClass::Inside), "{point:?}");
        }
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
/// body it was sketched on and the body it runs to. The body it runs to is the scope the app
/// picks when the call names none; a call that names its targets keeps that body apart
/// (#19671).
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
    // The join takes the body the profile was sketched on; the body it runs to joins too.
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"to":[20,20,40],"operation":"join"}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
    assert_eq!(state.bodies.len(), 1, "the tool, the base and the target are one body");
    assert!((volume("Base") - 60_000.0).abs() < 1e-6, "the target took the tool: {}", volume("Base"));
    assert_eq!(state.body("Base").unwrap().body.lumps().map_or(1, |l| l.len()), 1, "one piece");
}

/// A base with a target above it, and a profile on the base's top face: the extrude runs to the
/// target's bottom face.
fn base_and_target() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Base"}));
    run(&mut s, "solid.box", json!({"corner":[0,0,40],"length":40,"width":40,"height":20,"body_name":"Target"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":{"face":[20,20,10]}}))["sketch"].as_u64().unwrap();
    let plane = s.model.state().sketch(sketch).unwrap().plane;
    let at = |x: f64, y: f64| {
        let p = plane.to_local(Vec3::new(x, y, 10.0));
        json!([p.x, p.y])
    };
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": at(10.0,10.0), "p1": at(30.0,30.0)}));
    run(&mut s, "sketch.finish", json!({}));
    (s, sketch)
}

/// #19671: a Join To Object that names its Target Bodies joins only those: the body it runs to
/// stays its own body, and the design keeps two bodies.
#[test]
fn join_extrude_to_another_body_keeps_that_body_apart() {
    let (mut s, sketch) = base_and_target();
    // The face to run to is on the target body: the join takes the profile into the base and
    // stops there, leaving the target its own body.
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"to":[20,20,40],"operation":"join","targets":["Base"]}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let check = |s: &Session| {
        let state = s.model.state();
        let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
        assert_eq!(state.bodies.len(), 2, "the body the extrude runs to stays its own body");
        assert!((volume("Base") - 28_000.0).abs() < 1e-6, "the base took the tool: {}", volume("Base"));
        assert!((volume("Target") - 32_000.0).abs() < 1e-6, "the target is untouched: {}", volume("Target"));
    };
    check(&s);
    // The design saves the choice and opens with it again.
    check(&Session::new(solidcraft_doc::format::read(&s.doc.to_json()).unwrap()));
}

/// #19671: `to_body_apart: false` joins the body the extrusion runs to even when the call names
/// its targets, which is what every Join To Object did before (#6424).
#[test]
fn join_extrude_to_another_body_joins_that_body_on_request() {
    let (mut s, sketch) = base_and_target();
    run(&mut s, "solid.extrude", json!({"sketch":sketch,"to":[20,20,40],"operation":"join","targets":["Base"],"to_body_apart":false}));
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
    let state = s.model.state();
    let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
    assert_eq!(state.bodies.len(), 1, "the tool, the base and the target are one body");
    assert!((volume("Base") - 60_000.0).abs() < 1e-6, "the target took the tool: {}", volume("Base"));
}

/// #19671: `to_body_apart` is about a Join that runs to another body: a Join without one, or an
/// operation that never joins that body, is refused with the reason.
#[test]
fn to_body_apart_needs_a_join_to_an_object() {
    let (mut s, sketch) = base_and_target();
    // No To Object: nothing to keep apart.
    let error = s
        .execute("solid.extrude", &json!({"sketch":sketch,"distance":10,"operation":"join","targets":["Base"],"to_body_apart":true}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("to_body_apart"), "{error}");
    // A Cut never joins the body it runs to.
    let error = s
        .execute("solid.extrude", &json!({"sketch":sketch,"to":[20,20,20],"operation":"cut","to_body_apart":true}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("to_body_apart"), "{error}");
}

/// #8151: a design saved before a Join To Object also joined the body it runs to (format 39 and
/// older) keeps that body apart when it opens, so its later features still find it, and an edit
/// of the extrude keeps it apart too.
#[test]
fn join_extrude_to_another_body_saved_before_it_joined_keeps_that_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":10,"body_name":"Base"}));
    run(&mut s, "solid.box", json!({"corner":[0,0,40],"length":40,"width":40,"height":20,"body_name":"Target"}));
    let sketch = run(&mut s, "sketch.create", json!({"plane":{"face":[20,20,10]}}))["sketch"].as_u64().unwrap();
    let plane = s.model.state().sketch(sketch).unwrap().plane;
    let at = |x: f64, y: f64| {
        let p = plane.to_local(Vec3::new(x, y, 10.0));
        json!([p.x, p.y])
    };
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": at(10.0,10.0), "p1": at(30.0,30.0)}));
    run(&mut s, "sketch.finish", json!({}));
    // A feature after the extrude names the body it runs to, as the lid's sketch and moves did.
    // The join leaves no such body to name, so the move is made first and put last below.
    let moved = run(&mut s, "solid.move", json!({"bodies":["Target"],"translate":[0,0,5]}))["feature"].clone();
    // The extrude joins the body it runs to, as every Join To Object did until the call named
    // its targets (#19671); a design saved before that keeps the body apart.
    let extrude =
        run(&mut s, "solid.extrude", json!({"sketch":sketch,"to":[20,20,40],"operation":"join","targets":["Base"],"to_body_apart":false}))["feature"].clone();
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let features = saved["features"].as_array_mut().unwrap();
    let at_move = features.iter().position(|f| f["id"] == moved).unwrap();
    let feature = features.remove(at_move);
    features.push(feature);
    saved["format"] = json!("solidcraft/39");
    saved.as_object_mut().unwrap().remove("format_pending");
    let mut old = Session::new(solidcraft_doc::format::read(&saved.to_string()).unwrap());
    let check = |s: &Session| {
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "{:?}", s.model.results);
        let state = s.model.state();
        let volume = |name: &str| solidcraft_kernel::measure(&state.body(name).unwrap().body).unwrap().volume;
        assert_eq!(state.bodies.len(), 2, "the body the extrude runs to stays its own body");
        assert!((volume("Base") - 28_000.0).abs() < 1e-6, "the base took the tool: {}", volume("Base"));
        assert!((volume("Target") - 32_000.0).abs() < 1e-6, "the target is untouched: {}", volume("Target"));
    };
    check(&old);
    run(
        &mut old,
        "timeline.redefine",
        json!({"feature":extrude,"command":"solid.extrude","params":{"sketch":sketch,"to":[20,20,40],"operation":"join","targets":["Base"]}}),
    );
    check(&old);
    // The design saves the choice and opens with it again.
    check(&Session::new(solidcraft_doc::format::read(&old.doc.to_json()).unwrap()));
}
