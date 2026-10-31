use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};

/// `ui.at {"plane": …}` gives a point that a click picks that plane at, even where another
/// origin plane hides the middle of its square (seen from the front right and a little lower
/// than iso, the XZ square covers the middle of the XY one).
#[test]
fn clicking_an_origin_plane_target_sketches_on_that_plane() {
    for (yaw, pitch) in [(-0.785, 0.615), (-1.1, 0.5), (-0.45, 0.5), (2.4, 0.6), (-2.4, 0.4)] {
        for (plane, normal) in [("XY", [0.0, 0.0, 1.0]), ("XZ", [0.0, 1.0, 0.0]), ("YZ", [1.0, 0.0, 0.0])] {
            let mut h = Harness::new();
            h.app.cam.yaw = yaw;
            h.app.cam.pitch = pitch;
            h.step(&json!({"start":"sketch.create"})).unwrap();
            h.step(&json!({"click":{"plane":plane}})).unwrap();
            let state = h.app.session.world_state();
            let n = state.sketches.first().map(|s| s.plane.normal()).unwrap_or_default();
            let along = (n.x * normal[0] + n.y * normal[1] + n.z * normal[2]).abs();
            assert!(along > 0.999, "yaw {yaw} pitch {pitch} {plane}: sketched on a plane with normal {n:?}");
        }
    }
}

/// A box at the origin: the three origin plane squares lie on or behind its faces from every
/// view (#5431).
fn box_at_origin() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":50,"width":50,"height":50})).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    h
}

/// The plane the sketch being edited was created on (a plane flipped to face the view, as the
/// plane it was flipped from).
fn sketch_plane(h: &Harness) -> PlaneRef {
    let sketch = h.app.session.active_sketch.expect("the click creates a sketch");
    match &h.app.session.doc.feature(sketch).unwrap().kind {
        FeatureKind::Sketch { plane: PlaneRef::Flipped { base, .. }, .. } => (**base).clone(),
        FeatureKind::Sketch { plane, .. } => plane.clone(),
        other => panic!("feature {sketch} is not a sketch: {other:?}"),
    }
}

/// While Create Sketch waits for a plane, the origin planes are drawn over the bodies, so a
/// click on one the box hides picks it through the box (#5431). From this view the box hides the
/// XY and YZ squares; the XZ square lies on its front face, and a face still wins that tie (#31).
/// Each click goes where the scenarios click a plane: on its square, clear of the other squares.
#[test]
fn create_sketch_picks_an_origin_plane_through_a_body() {
    use crate::viewport::Hit;
    for (plane, hidden) in [("XY", true), ("YZ", true), ("XZ", false)] {
        let mut h = box_at_origin();
        h.step(&json!({"start":"sketch.create"})).unwrap();
        let at = h.at(&json!({ "plane": plane })).unwrap();
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        let pos = egui::pos2(at[0] as f32, at[1] as f32);
        let hits = crate::viewport::pick(&h.app, &proj, pos);
        let face = hits.iter().find_map(|x| if let Hit::Face { point, .. } = x { Some(*point) } else { None });
        let on_plane = hits.iter().find_map(|x| match x {
            Hit::Plane { name, point } if name == plane => Some(*point),
            _ => None,
        });
        let (Some(face), Some(on_plane)) = (face, on_plane) else { panic!("{plane}: the box and the plane are under the pointer: {hits:?}") };
        let (o, _) = proj.ray(pos);
        if hidden {
            assert!(on_plane.dist(o) > face.dist(o) + 1.0, "{plane}: the box hides the plane where it is clicked");
        }
        h.step(&json!({"click": at})).unwrap();
        h.settle();
        let picked = sketch_plane(&h);
        if hidden {
            assert_eq!(picked, PlaneRef::Origin { name: plane.into() }, "{plane}: sketched on the origin plane");
        } else {
            assert!(matches!(picked, PlaneRef::Face { .. }), "{plane}: sketched on the face the plane lies on, not {picked:?}");
        }
    }
}

/// Away from the origin plane squares, Create Sketch still takes the body's face.
#[test]
fn create_sketch_still_picks_a_face_beside_the_origin_planes() {
    let mut h = box_at_origin();
    h.step(&json!({"start":"sketch.create"})).unwrap();
    let at = h.at(&json!({"world": [45, 45, 50]})).unwrap();
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let hits = crate::viewport::pick(&h.app, &proj, egui::pos2(at[0] as f32, at[1] as f32));
    assert!(!hits.iter().any(|x| matches!(x, crate::viewport::Hit::Plane { .. })), "no origin plane under the pointer: {hits:?}");
    h.step(&json!({"click": at})).unwrap();
    h.settle();
    let plane = sketch_plane(&h);
    assert!(matches!(plane, PlaneRef::Face { .. }), "sketched on the top face, not {plane:?}");
}
