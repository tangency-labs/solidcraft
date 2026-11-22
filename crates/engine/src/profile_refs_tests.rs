//! Profile picks survive recomputes (#30): picked by index, they are kept by the curves around
//! them, so a face sketch whose projected outline is rebuilt still extrudes the same region.

use serde_json::{Value, json};
use solidcraft_geom::MeshGeom;

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
    s.model
        .state()
        .bodies
        .iter()
        .map(|b| (b.name.clone(), b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-2)).unwrap().measure().volume))
        .collect()
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
    let world = s.world_state().unwrap();
    let mesh = world.body("Enclosure").unwrap().mesh();
    assert!(!mesh.contains(solidcore_record::Vec3::new(49., 0., 12.)));
    assert!(mesh.contains(solidcore_record::Vec3::new(49., 0., 20.)));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.result(id).unwrap().error.is_none());
    let world = reopened.world_state().unwrap();
    let mesh = world.body("Enclosure").unwrap().mesh();
    assert!(!mesh.contains(solidcore_record::Vec3::new(49., 0., 12.)));
    assert!(mesh.contains(solidcore_record::Vec3::new(49., 0., 20.)));
}

/// A pick the sketch no longer has is an error on the feature, never a different profile.
#[test]
fn a_lost_profile_is_an_error_not_another_pick() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
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

/// A plate with two blocks on it, a sketch on its top face and a thin layer over the whole face
/// outline of that sketch; `px` places the small block.
fn skinned_plate(px: &str) -> Session {
    let mut s = Session::default();
    let script = json!([
        {"command": "parameters.add", "params": {"name": "px", "expression": px}},
        {"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 50, "width": 40, "height": 2.5}},
        {"command": "solid.box", "params": {"corner": [15, 10, 2.5], "length": 15, "width": 15, "height": 8, "operation": "join"}},
        {"command": "solid.box", "params": {"corner": ["px", 25, 2.5], "length": 6, "width": 6, "height": 0.6, "operation": "join"}},
        {"command": "sketch.create", "params": {"plane": {"face": [2, 2, 2.5]}, "name": "OnTop"}},
        {"command": "sketch.finish", "params": {}},
        {"command": "solid.extrude", "params": {"sketch": "OnTop", "distance": "0.3 mm", "operation": "join", "profiles": [{"point": [3, 3]}], "name": "Skin"}}
    ]);
    s.run_script(&script).unwrap();
    s
}

/// A face whose outline touches another at a corner is projected as a fresh sketch projects it
/// when the part changes under the sketch, not kept as it was: the layer over it is the same as
/// one built from scratch, and says nothing of a retained outline.
#[test]
fn a_face_that_touches_at_a_corner_is_projected_again_not_retained() {
    let note = "built from Sketch OnTop's last outline";
    let mut s = skinned_plate("31 mm");
    let skin = s.doc.find_feature("Skin").unwrap().id;
    assert!(s.model.result(skin).is_some_and(|r| r.warning.is_none()), "{:?}", s.model.result(skin));
    // At 30 mm the small block touches the large one at a corner.
    run(&mut s, "parameters.change", json!({"name": "px", "expression": "30 mm"}));
    let sketch = s.doc.find_feature("OnTop").unwrap().id;
    assert!(!s.model.state().sketch(sketch).is_some_and(|ss| ss.sketch.links.iter().any(|l| l.lost)));
    let warning = s.model.result(skin).and_then(|r| r.warning.clone());
    assert!(!warning.as_deref().is_some_and(|w| w.contains(note)), "{warning:?}");
    assert!(s.model.result(skin).is_some_and(|r| r.error.is_none()), "{:?}", s.model.result(skin));
    let after = bodies(&s);
    // The same as the design built from scratch with the block at 30 mm.
    let fresh = skinned_plate("30 mm");
    let built = bodies(&fresh);
    assert_eq!(after.len(), 1, "{after:?}");
    assert!((after[0].1 - built[0].1).abs() < 0.1, "{after:?} {built:?}");
    assert!((after[0].1 - 7343.3).abs() < 0.1, "{after:?}");
    let faces = |s: &Session| s.model.state().bodies.first().map(|b| b.body.face_count());
    assert_eq!(faces(&s), faces(&fresh));
    assert_eq!(faces(&s), Some(16));
}

/// Where the face cannot be projected at all (the part it lay on is gone), the sketch fails and
/// the layer built from it fails with it: no other outline is taken for it.
#[test]
fn a_layer_over_a_face_that_is_gone_does_not_build() {
    let mut s = skinned_plate("31 mm");
    let skin = s.doc.find_feature("Skin").unwrap().id;
    run(&mut s, "timeline.suppress", json!({"feature": "Box1", "suppressed": true}));
    assert!(s.model.result(skin).is_some_and(|r| r.error.is_some()), "{:?}", s.model.result(skin));
}
