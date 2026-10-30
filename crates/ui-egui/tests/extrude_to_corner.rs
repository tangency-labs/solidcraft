use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_ui_egui::scenario::Harness;

fn extruding(h: &mut Harness, sketch: u64) {
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.app.session.selection = vec![Sel::Profile { sketch, index: 0 }];
    h.step(&json!({"start":"solid.extrude"})).unwrap();
    h.frames(3);
}

fn to_object(h: &Harness) -> Option<Vec<Sel>> {
    h.app.dialog.as_ref()?.inputs.iter().find(|i| i.label == "To Object").map(|i| i.items.clone())
}

/// Extruding by a distance, a click on a body's corner extrudes to it: To Object, to that
/// corner, without choosing To Object first.
#[test]
fn a_clicked_corner_is_where_an_extrude_by_distance_goes() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Top","corner":[-20,-20,50]})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":5})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    extruding(&mut h, sketch);
    assert_eq!(to_object(&h), None, "by a distance");
    h.step(&json!({"click":{"world":[20,-20,55]}})).unwrap();
    h.frames(4);
    assert!(matches!(to_object(&h).as_deref(), Some([Sel::Vertex { body, .. }]) if body == "Top"), "{:?}", to_object(&h));
    h.step(&json!({"widget":"OK"})).unwrap();
    let st = h.app.session.world_state();
    let tallest = st.bodies.iter().map(|b| b.mesh().bounds().max.z).fold(f64::MIN, f64::max);
    assert!((tallest - 55.0).abs() < 1e-6, "{tallest}");
}

/// A corner on the profile's own plane (the border of the face a sketch is on) is no
/// destination: the click is left to the profiles.
#[test]
fn a_corner_on_the_sketch_plane_is_not_taken() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":40,"height":10})).unwrap();
    let sketch = h.app.run("sketch.create", json!({"plane":{"face":[20,20,10]}})).unwrap()["sketch"].as_u64().unwrap();
    h.app.run("sketch.circle.center", json!({"center":[20,20],"radius":5})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let profile = h.app.session.world_state().sketch(sketch).unwrap().profiles.iter().position(|p| p.outer_curves.len() == 1).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.app.session.selection = vec![Sel::Profile { sketch, index: profile }];
    h.step(&json!({"start":"solid.extrude"})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"world":[40,0,10]}})).unwrap();
    h.frames(4);
    assert_eq!(to_object(&h), None);
}
