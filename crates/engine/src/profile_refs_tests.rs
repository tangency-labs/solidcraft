//! Profile picks survive recomputes (#30): picked by index, they are kept by the curves around
//! them, so a face sketch whose projected outline is rebuilt still extrudes the same region.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The demo enclosure (80 × 60 × 30, rounded, shelled) with a USB port cut through a side, the
/// port picked by index as the Extrude dialog does.
fn enclosure() -> Session {
    let mut s = Session::default();
    let script = json!([
        {"command": "parameters.change", "params": {"name": "width", "expression": "80 mm", "comment": "outside, X"}},
        {"command": "parameters.change", "params": {"name": "depth", "expression": "60 mm", "comment": "outside, Y"}},
        {"command": "parameters.change", "params": {"name": "height", "expression": "30 mm"}},
        {"command": "parameters.change", "params": {"name": "wall", "expression": "2 mm"}},
        {"command": "sketch.create", "params": {"plane": "XY", "name": "Outline"}},
        {"command": "sketch.rectangle.center", "params": {"center": [0, 0], "corner": [40, 30]}},
        {"command": "sketch.constraint.coincident", "params": {"a": "p5", "b": "origin"}},
        {"command": "sketch.dimension", "params": {"entities": ["l1"], "value": "width"}},
        {"command": "sketch.dimension", "params": {"entities": ["l2"], "value": "depth"}},
        {"command": "sketch.finish", "params": {}},
        {"command": "solid.extrude", "params": {"distance": "height", "body_name": "Enclosure"}},
        {"command": "solid.fillet", "params": {"edges": [[40, 30, 15], [-40, 30, 15], [-40, -30, 15], [40, -30, 15]], "radius": 6}},
        {"command": "solid.shell", "params": {"faces": [[0, 0, 30]], "thickness": "wall"}},
        {"command": "sketch.create", "params": {"plane": "YZ", "offset":"width / 2", "name": "USB port"}},
        {"command": "sketch.project", "params": {"refs":[{"face":[40,10,20],"body":"Enclosure"}]}},
        {"command": "sketch.rectangle.center", "params": {"center": [0, 12], "corner": [6, 15.5]}},
        {"command": "sketch.finish", "params": {}},
        {"command": "solid.extrude", "params": {"distance": "-wall", "operation": "cut", "profiles": [1]}}
    ]);
    s.run_script(&script).unwrap();
    s
}

fn bodies(s: &Session) -> Vec<(String, f64)> {
    s.model.state().bodies.iter().map(|b| (b.name.clone(), b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-2)).unwrap().measure().volume)).collect()
}

#[test]
fn a_port_picked_by_index_stays_a_cut_after_a_width_change() {
    let mut s = enclosure();
    let f = s.doc.features.last().unwrap();
    let solidcraft_doc::FeatureKind::Extrude { profiles, .. } = &f.kind else { panic!("{:?}", f.kind) };
    assert!(matches!(profiles, solidcraft_doc::ProfileSel::Curves { .. }), "stored by curves: {profiles:?}");
    let before = bodies(&s);
    assert_eq!(before.len(), 1, "{before:?}");
    run(&mut s, "parameters.change", json!({"name": "width", "expression": "100 mm"}));
    let after = bodies(&s);
    assert_eq!(after.len(), 1, "the port is still a cut, not a new body: {after:?}");
    // The longer box less the same port: more wall and floor, the port still taken out.
    assert!(after[0].1 > before[0].1 + 1000.0, "{before:?} {after:?}");
    let id = s.doc.features.last().unwrap().id;
    assert!(s.model.result(id).is_some_and(|r| r.error.is_none()), "{:?}", s.model.result(id));
    let port = s.doc.find_feature("USB port").unwrap().id;
    assert!((s.model.state().sketch(port).unwrap().plane.origin.x - 50.).abs() < 1e-6);
    let world = s.world_state();
    let mesh = world.body("Enclosure").unwrap().mesh();
    assert!(!mesh.contains(solidcraft_geom::Vec3::new(49., 0., 12.)));
    assert!(mesh.contains(solidcraft_geom::Vec3::new(49., 0., 20.)));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.result(id).unwrap().error.is_none());
    let world = reopened.world_state();
    let mesh = world.body("Enclosure").unwrap().mesh();
    assert!(!mesh.contains(solidcraft_geom::Vec3::new(49., 0., 12.)));
    assert!(mesh.contains(solidcraft_geom::Vec3::new(49., 0., 20.)));
}

/// A pick the sketch no longer has is an error on the feature, never a different profile.
#[test]
fn a_lost_profile_is_an_error_not_another_pick() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10], "ids": ["a", "b", "c", "d"]}));
    run(&mut s, "sketch.circle.center", json!({"center": [30, 5], "radius": 3, "id": "ring"}));
    run(&mut s, "sketch.finish", json!({}));
    let ring = s.model.state().sketches[0].profiles.iter().position(|p| p.outer_curves == ["ring"]).unwrap();
    run(&mut s, "solid.extrude", json!({"sketch": "S", "profiles": [ring], "distance": 5}));
    let n = s.model.state().bodies.len();
    assert_eq!(n, 1);
    // Delete the picked circle: the extrude fails with a message instead of taking the square.
    let sk = s.doc.find_feature("S").unwrap().id;
    let picked = match &s.doc.features.last().unwrap().kind {
        solidcraft_doc::FeatureKind::Extrude { profiles: solidcraft_doc::ProfileSel::Curves { loops }, .. } => loops[0].clone(),
        k => panic!("{k:?}"),
    };
    run(&mut s, "sketch.edit", json!({"sketch": sk}));
    run(&mut s, "sketch.delete", json!({"entities": picked}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(s.model.state().bodies.is_empty(), "no silent re-pick: {:?}", bodies(&s));
    let id = s.doc.features.last().unwrap().id;
    assert!(s.model.result(id).is_some_and(|r| r.error.is_some()), "{:?}", s.model.result(id));
}
