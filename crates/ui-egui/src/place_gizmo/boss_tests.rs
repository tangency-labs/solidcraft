//! The Boss handle: the marker drags a new boss onto a sketch point,
//! and moves an existing one from Edit Feature.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::scenario::Harness;

/// An 80 × 50 × 10 plate seen from the top,.
fn plate() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.preferences.placement_handles = true;
    h.app.run("solid.box", json!({"length": 80, "width": 50, "height": 10})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

fn position(h: &Harness) -> Vec3 {
    let d = h.app.dialog.as_ref().expect("a dialog is open");
    match d.inputs.first().and_then(|i| i.items.first()) {
        Some(Sel::Face { point, .. }) => *point,
        other => panic!("the Position input holds a face place, not {other:?}"),
    }
}

fn boss_position(h: &Harness) -> Vec3 {
    let f = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Boss { .. })).expect("a Boss was made");
    let FeatureKind::Boss { position, .. } = &f.kind else { panic!("a Boss: {:?}", f.kind) };
    position.0
}

#[test]
fn marker_drags_a_new_boss_onto_a_sketch_point() {
    let mut h = plate();
    h.app.run("sketch.create", json!({"plane": {"face": [40, 25, 10]}})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [60, 35], "radius": 1.5})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"start": "plastic.boss"})).unwrap();
    h.step(&json!({"click": {"world": [15, 12, 10]}})).unwrap();
    h.settle();
    assert!(position(&h).dist(Vec3::new(15.0, 12.0, 10.0)) < 0.5, "placed where clicked: {:?}", position(&h));
    assert!(h.at(&json!({"handle": "boss:position"})).is_ok(), "the marker is published");
    assert!(h.at(&json!({"handle": "boss:rotate"})).is_err(), "a round boss has no knob");

    h.step(&json!({"drag": [{"handle": "boss:position"}, {"world": [60, 35, 10]}], "steps": 12})).unwrap();
    h.settle();
    assert!(position(&h).dist(Vec3::new(60.0, 35.0, 10.0)) < 1e-6, "snapped to the circle's centre: {:?}", position(&h));

    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog");
    assert!(boss_position(&h).dist(Vec3::new(60.0, 35.0, 10.0)) < 1e-6, "the Boss took the dragged place: {:?}", boss_position(&h));
}

#[test]
fn marker_moves_a_boss_from_edit_feature() {
    let mut h = plate();
    let made = h.app.run("plastic.boss", json!({"position": [20, 20, 10], "diameter": 6, "height": 8})).unwrap();
    let name = made["name"].as_str().unwrap().to_string();
    h.settle();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.settle();
    h.step(&json!({"drag": [{"handle": "boss:position"}, {"world": [55, 30, 10]}], "steps": 12})).unwrap();
    h.settle();
    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog");
    assert!(boss_position(&h).dist(Vec3::new(55.0, 30.0, 10.0)) < 0.5, "the Boss moved: {:?}", boss_position(&h));
    h.step(&json!({"expect": {"errors": 0, "bodies": 1}})).unwrap();
}
