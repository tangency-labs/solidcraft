use crate::Session;
use serde_json::{Value, json};
use solidcraft_geom::Vec3;

fn run(s: &mut Session, command: &str, p: Value) -> Value {
    s.execute(command, &p).unwrap()
}
fn occurrence(s: &Session, name: &str) -> u64 {
    s.doc.occurrence_of(s.doc.find_component(name).unwrap()).unwrap().id
}
fn slider() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let a = occurrence(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    s
}
fn rail_value(s: &Session) -> f64 {
    s.doc.assembly.joints.iter().find(|j| j.name == "Rail").unwrap().values[0]
}

#[test]
fn joint_pose_contact_reuses_reference_geometry_across_new_placements() {
    let mut s = slider();
    // Measure the swept-contact geometry factory, separately from other joint work.
    super::ContactGeometry::new(&s).unwrap();
    for value in [11., 12., 13., 14.] {
        let result = run(&mut s, "joint.drive", json!({"joint":"Rail","value":value}));
        let (_, work) = crate::kernel::profile::capture(|| super::ContactGeometry::new(&s).unwrap());
        assert!(result["stopped_by_contact"].is_null(), "{result}");
        assert!((rail_value(&s) - value).abs() < 1e-8);
        assert_eq!(work.tessellate_ms, 0., "unchanged contact bodies were tessellated for slider value {value}");
    }
    // Reusing geometry must still stop a genuine collision, including an endpoint beyond it.
    let result = run(&mut s, "joint.drive", json!({"joint":"Rail","value":-30}));
    assert!(result["stopped_by_contact"].is_object(), "{result}");
    assert!(rail_value(&s).abs() < 2e-3);
}

#[test]
fn slider_cannot_tunnel_to_a_collision_free_endpoint() {
    let mut s = slider();
    let result = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -30}));
    assert!(result["stopped_by_contact"].is_object(), "{result}");
    assert!(rail_value(&s).abs() < 2e-3, "{}", rail_value(&s));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    assert!((rail_value(&s) - 10.0).abs() < 1e-6, "can move away from contact");
    run(&mut s, "contact.disable_all", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -100000}));
    assert!((rail_value(&s) + 100000.0).abs() < 1e-6, "disabled contact skips sweep budget");
}

/// Each drive with contact on meshed every body in a contact set again: a body that did not
/// change keeps its contact mesh across drives (and scratch copies), a changed one is meshed anew.
#[test]
fn contact_meshes_are_made_once_per_body() {
    let mut s = slider();
    let meshes = |s: &Session| super::ContactGeometry::new(s).unwrap().bodies.iter().map(|b| b.mesh.clone()).collect::<Vec<_>>();
    let first = meshes(&s);
    assert_eq!(first.len(), 2);
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5}));
    let scratch = s.scratch();
    for again in [meshes(&s), meshes(&scratch)] {
        assert!(first.iter().zip(&again).all(|(a, b)| std::sync::Arc::ptr_eq(a, b)), "the same meshes, not tessellated again");
    }
    // A body that changed is meshed for what it is now.
    run(&mut s, "component.activate", json!({"component": "B"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [24, 4, 10], "operation": "join"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let changed = meshes(&s);
    assert!(changed.iter().zip(&first).any(|(a, b)| !std::sync::Arc::ptr_eq(a, b)));
    assert!(changed.iter().map(|m| m.bounds().max.z).fold(0.0, f64::max) > 11.0, "the joined box is in the contact mesh");
}

#[test]
fn revolute_cannot_cross_a_blocker_between_clear_endpoints() {
    let mut s = Session::default();
    for (name, corner) in [("Arm", [20, -2, -2]), ("Blocker", [-2, 20, -2])] {
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": corner}));
    }
    run(
        &mut s,
        "joint.create",
        json!({"type": "revolute", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": "Arm", "point": [0,0,0]}, "flip": true, "name": "Pivot"}),
    );
    run(&mut s, "contact.create", json!({"occurrences": ["Arm", "Blocker"]}));
    let result = run(&mut s, "joint.drive", json!({"joint": "Pivot", "value": "180 deg"}));
    assert!(result["stopped_by_contact"].is_object(), "{result}");
    let angle = s.doc.assembly.joints.iter().find(|j| j.name == "Pivot").unwrap().values[0];
    assert!(angle > 60_f64.to_radians() && angle < 90_f64.to_radians(), "angle={angle}");
    let arm = solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(&s.doc, occurrence(&s, "Arm")), Vec3::new(22.0, 0.0, 0.0));
    assert!(arm.y > 20.0 && arm.x > 2.0, "{arm:?}");
}

#[test]
fn linked_slider_uses_effective_travel_for_contact() {
    let mut s = slider();
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "component.create", json!({"name": "Input"}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": 0, "point": [0,0,0]}, "b": {"occurrence": "Input", "point": [0,0,0]}, "flip": true, "name": "Input Joint"}),
    );
    run(&mut s, "joint.drive", json!({"joint": "Input Joint", "value": 1}));
    run(&mut s, "joint.motion_link", json!({"a": "Input Joint", "b": "Rail", "ratio": 10}));
    let result = run(&mut s, "joint.drive", json!({"joint": "Input Joint", "value": -3}));
    assert!(result["stopped_by_contact"].is_object(), "{result}");
    let input = s.doc.assembly.joints.iter().find(|j| j.name == "Input Joint").unwrap().values[0];
    assert!(input.abs() < 2e-4, "{input}");
}

#[test]
fn motion_play_and_sparse_export_share_swept_contact_checks() {
    let mut s = slider();
    run(&mut s, "motion.study", json!({"name": "Crossing", "steps": 100, "keys": [{"joint": "Rail", "points": [[0,10], [100,-30]]}]}));
    let doc = s.doc.clone();
    let exported = run(&mut s, "motion.export", json!({"study": "Crossing", "samples": 2}));
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "export leaves document untouched");
    assert!(exported["samples"][1]["stopped_by_contact"].is_object(), "{exported}");
    let played = run(&mut s, "motion.play", json!({"study": "Crossing", "step": 100}));
    assert!(played["stopped_by_contact"].is_object(), "{played}");
    assert!(rail_value(&s).abs() < 2e-3);
}

#[test]
fn over_budget_sweep_errors_and_restores_document() {
    let mut s = slider();
    let doc = s.doc.clone();
    let error = s.execute("joint.drive", &json!({"joint": "Rail", "value": -100000})).unwrap_err();
    assert!(error.to_string().contains("sampling budget"), "{error}");
    assert_eq!(*s.doc, *doc);
    assert!((rail_value(&s) - 10.0).abs() < 1e-6);
}

#[test]
fn thin_tilted_plates_cannot_cross_between_axis_bound_samples() {
    let mut s = Session::default();
    let normal = solidcraft_doc::apply_vector(&solidcraft_doc::rigid(Vec3::ZERO, Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0), 55_f64.to_radians()), Vec3::Z);
    for name in ["Plate A", "Plate B"] {
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 0.05, "corner": [-5,-5,-0.025], "body_name": name}));
        run(&mut s, "solid.move", json!({"bodies": [name], "axis": [1,1,0], "angle": "55 deg"}));
    }
    let (a, b) = (occurrence(&s, "Plate A"), occurrence(&s, "Plate B"));
    run(&mut s, "occurrence.move", json!({"occurrence": b, "translate": normal * 2.0}));
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(
        &mut s,
        "joint.as_built",
        json!({"type": "slider", "a": "Plate A", "b": "Plate B", "at": {"occurrence": 0, "point": [0,0,0], "z": normal}, "name": "Slide"}),
    );
    run(&mut s, "contact.create", json!({"occurrences": ["Plate A", "Plate B"]}));
    let result = run(&mut s, "joint.drive", json!({"joint": "Slide", "value": -4}));
    assert!(result["stopped_by_contact"].is_object(), "{result}");
    let value = s.doc.assembly.joints.iter().find(|j| j.name == "Slide").unwrap().values[0];
    assert!(value > -2.0 && value < -1.9, "{value}");
}

#[test]
fn ordinary_curved_bodies_keep_a_practical_sampling_step() {
    for curved in ["sphere", "fillet"] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Blocker"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0,-5,-5]}));
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.create", json!({"name": "Curved"}));
        if curved == "sphere" {
            run(&mut s, "solid.sphere", json!({"radius": 5, "center": [20,0,0]}));
        } else {
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [15,-5,-5]}));
            run(&mut s, "solid.fillet", json!({"edges": [[15,-5,0]], "radius": 2}));
        }
        run(
            &mut s,
            "joint.as_built",
            json!({"type": "slider", "a": 0, "b": "Curved", "at": {"occurrence": 0, "point": [0,0,0], "z": [1,0,0]}, "name": "Slide"}),
        );
        run(&mut s, "contact.create", json!({"occurrences": ["Blocker", "Curved"]}));
        let geometry = super::ContactGeometry::new(&s).unwrap();
        assert!(geometry.step > 1.0, "{curved}: tessellation must not force tiny steps: {}", geometry.step);
        let result = run(&mut s, "joint.drive", json!({"joint": "Slide", "value": -30}));
        assert!(result["stopped_by_contact"].is_object(), "{curved}: {result}");
        let value = s.doc.assembly.joints.iter().find(|j| j.name == "Slide").unwrap().values[0];
        assert!(value > -5.02 && value < -4.9, "{curved}: {value}");
    }
}

#[test]
fn initially_overlapping_assembly_can_move_out_for_recovery() {
    let mut s = slider();
    run(&mut s, "contact.disable_all", json!({}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    run(&mut s, "contact.enable_sets", json!({"enabled": true}));
    assert!(!run(&mut s, "contact.check", json!({}))["collisions"].as_array().unwrap().is_empty());
    let result = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    assert!(result["stopped_by_contact"].is_null(), "recovery must not stick: {result}");
    assert!((rail_value(&s) - 10.0).abs() < 1e-6);
    assert!(run(&mut s, "contact.check", json!({}))["collisions"].as_array().unwrap().is_empty());
}
