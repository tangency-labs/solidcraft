//! Edit Sketch rolls the timeline back to the sketch. A body made before the sketch and deleted
//! after it is then part of the model again, but it must not be shown while the sketch is edited
//! (#1167): the history is rolled back, the view of it is not. A body the sketch is built on stays.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;
use solidcraft_sketch::LinkSource;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A 10 x 10 x 5 block `name`, drawn on the XY plane at `x`.
fn block(s: &mut Session, name: &str, x: f64) {
    run(s, "sketch.create", json!({"plane": "XY", "name": format!("{name}Sketch")}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [x, 0], "p1": [x + 10.0, 10]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"sketch": format!("{name}Sketch"), "distance": 5, "body_name": name}));
}

/// Whether `inspect` lists the body, and what it says of it.
fn inspected(s: &mut Session, body: &str) -> Value {
    let r = run(s, "document.inspect", json!({"measure": false, "sketches": false}));
    r["bodies"].as_array().and_then(|bodies| bodies.iter().find(|b| b["name"] == body)).cloned().unwrap_or(Value::Null)
}

/// `Kept` and `Gone` (removed after the sketch `Edited`), then Edit Sketch on `Edited`.
fn edit_with_a_later_removal(s: &mut Session) {
    block(s, "Kept", 0.0);
    block(s, "Gone", 20.0);
    run(s, "sketch.create", json!({"plane": "XY", "offset": "30 mm", "name": "Edited"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, 5]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.remove", json!({"bodies": ["Gone"]}));
    run(s, "timeline.roll_to", json!({"feature": "Edited"}));
    run(s, "sketch.edit", json!({"sketch": "Edited"}));
}

#[test]
fn a_body_deleted_later_is_not_shown_while_a_sketch_is_edited() {
    let mut s = Session::default();
    edit_with_a_later_removal(&mut s);
    // The rolled-back history still has the body: commands and measurements see it.
    assert!(s.model.state().body("Gone").is_some());
    assert!(!s.body_visible("Gone"));
    assert!(s.body_removed_later("Gone"));
    let gone = inspected(&mut s, "Gone");
    assert_eq!(gone["visible"], json!(false), "{gone}");
    assert_eq!(gone["removed_later"], json!(true), "{gone}");
    // What is not deleted is shown as before.
    assert!(s.body_visible("Kept"));
    let kept = inspected(&mut s, "Kept");
    assert_eq!(kept["visible"], json!(true), "{kept}");
    assert!(kept.get("removed_later").is_none(), "{kept}");
}

#[test]
fn the_body_is_shown_again_when_the_sketch_is_finished() {
    let mut s = Session::default();
    edit_with_a_later_removal(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    // Rolled back and not editing: the history as it was, which has the body.
    assert!(s.body_visible("Gone"));
    assert!(inspected(&mut s, "Gone").get("removed_later").is_none());
    // Rolled forward, it is gone.
    run(&mut s, "timeline.roll_to", json!({}));
    assert!(s.model.state().body("Gone").is_none());
}

#[test]
fn rolling_back_by_hand_shows_the_history() {
    let mut s = Session::default();
    block(&mut s, "Gone", 0.0);
    run(&mut s, "solid.remove", json!({"bodies": ["Gone"]}));
    run(&mut s, "timeline.roll_to", json!({"position": 2}));
    assert!(s.body_visible("Gone"));
}

#[test]
fn a_suppressed_removal_does_not_hide_the_body() {
    let mut s = Session::default();
    edit_with_a_later_removal(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    let removal = s.doc.features.iter().find(|f| f.kind.type_name() == "RemoveFeature").map(|f| f.id).unwrap_or(0);
    run(&mut s, "timeline.suppress", json!({"feature": removal, "suppressed": true}));
    run(&mut s, "sketch.edit", json!({"sketch": "Edited"}));
    assert!(s.body_visible("Gone"));
}

#[test]
fn a_body_the_sketch_projects_stays_shown() {
    let mut s = Session::default();
    block(&mut s, "Gone", 0.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "5 mm", "name": "Edited"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [10, 5, 5], "body": "Gone"}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.remove", json!({"bodies": ["Gone"]}));
    run(&mut s, "timeline.roll_to", json!({"feature": "Edited"}));
    run(&mut s, "sketch.edit", json!({"sketch": "Edited"}));
    assert!(s.body_visible("Gone"));
}

#[test]
fn a_body_the_sketch_sits_on_stays_shown() {
    let mut s = Session::default();
    block(&mut s, "Gone", 0.0);
    let top = {
        let world = s.world_state().unwrap();
        let face = world
            .body("Gone")
            .and_then(|body| body.body.faces(0.01).ok())
            .and_then(|faces| faces.into_iter().find(|f| f.plane_normal.is_some_and(|n| n.dot(Vec3::Z) > 0.999)))
            .unwrap();
        json!({"face": [face.centroid.x, face.centroid.y, face.centroid.z], "body": "Gone", "index": face.index})
    };
    run(&mut s, "sketch.create", json!({"plane": top, "name": "Edited"}));
    let anchored = match s.doc.find_feature("Edited").map(|f| &f.kind) {
        Some(solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { anchor, .. }, .. }) => anchor.is_some(),
        other => panic!("not attached to a face: {other:?}"),
    };
    assert!(anchored, "the face attachment names its body");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.remove", json!({"bodies": ["Gone"]}));
    run(&mut s, "timeline.roll_to", json!({"feature": "Edited"}));
    run(&mut s, "sketch.edit", json!({"sketch": "Edited"}));
    assert!(s.body_visible("Gone"));
}

#[test]
fn every_placement_of_a_component_body_is_left_out() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name": "Block"}))["component"].as_u64().unwrap_or(0);
    block(&mut s, "Gone", 0.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": "30 mm", "name": "Edited"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [5, 5]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.remove", json!({"bodies": ["Gone"]}));
    run(&mut s, "occurrence.copy", json!({"component": component, "translate": [50, 0, 0]}));
    run(&mut s, "timeline.roll_to", json!({"feature": "Edited"}));
    run(&mut s, "sketch.edit", json!({"sketch": "Edited"}));
    let world = s.world_state().unwrap();
    assert_eq!(world.bodies.len(), 2, "one body, placed twice");
    for body in &world.bodies {
        assert!(!s.body_visible(&body.name), "{} is deleted later", body.name);
    }
}

#[test]
fn a_link_reads_the_bodies_of_its_sources() {
    let at = Vec3::ZERO;
    let edge = LinkSource::Edge { body: "A".into(), at };
    let face = LinkSource::Face { body: "B".into(), at };
    assert_eq!(edge.bodies(), ["A"]);
    assert_eq!(LinkSource::Intersection { a: Box::new(edge), b: Box::new(face) }.bodies(), ["A", "B"]);
    assert!(LinkSource::Origin.bodies().is_empty());
}
