use crate::scenario::Harness;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};
use solidcraft_engine::geom::prelude::*;

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
            let state = h.app.session.world_state().unwrap();
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

/// A construction plane through a part is drawn past the part's edges at any zoom, so the body
/// cannot hide all of it; one clear of the part keeps the small square round its origin (#13318).
#[test]
fn a_midplane_shows_round_the_part_when_zoomed_in() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":60,"width":40,"height":20,"body_name":"Box"})).unwrap();
    h.app.run("construct.plane.midplane", json!({"a":{"face":[30,0,10]},"b":{"face":[30,40,10]},"name":"Mid"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":50,"name":"Above"})).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    for distance in [h.app.cam.distance, 25.0, 5.0] {
        h.app.cam.distance = distance;
        h.settle();
        let quads = crate::viewport::construction_quads(&h.app);
        let quad = |name: &str| quads.iter().find(|q| q.0 == name).map(|q| q.2).unwrap_or_else(|| panic!("{name} is drawn: {quads:?}"));
        let mid = quad("Mid");
        let lo = mid.iter().fold(Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), |a, p| Vec3::new(a.x.min(p.x), a.y.min(p.y), a.z.min(p.z)));
        let hi = mid
            .iter()
            .fold(Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| Vec3::new(a.x.max(p.x), a.y.max(p.y), a.z.max(p.z)));
        assert!(lo.x < 0.0 && hi.x > 60.0 && lo.z < 0.0 && hi.z > 20.0, "distance {distance}: the midplane reaches past the box: {mid:?}");
        let h_side = crate::selection::origin_size(h.app.cam.half_height()) * 1.2;
        let above = quad("Above");
        let side = above[0].dist(above[1]);
        assert!((side - 2.0 * h_side).abs() < 1e-6 * h_side.max(1.0), "distance {distance}: a plane clear of the box keeps its square: {above:?}");
    }
}

/// A plane offset from a face is drawn over that face, not round its origin, the plane's point
/// nearest the world origin, which is far from this box (#4345). So is a plane offset from that
/// plane, and both follow the face when the box grows. A plane clear of the box keeps its square.
#[test]
fn a_plane_offset_from_a_face_is_drawn_over_the_face() {
    let mut h = Harness::new();
    h.app.run("parameters.add", json!({"name":"len","expression":"20 mm"})).unwrap();
    h.app.run("solid.box", json!({"length":"len","width":20,"height":10,"corner":[40,30,0],"body_name":"Box"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base":{"face":[50,40,10]},"offset":5,"name":"Above"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base":"Above","offset":5,"name":"Higher"})).unwrap();
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":50,"name":"Clear"})).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.settle();
    let bounds = |q: [Vec3; 4]| {
        let lo = q.iter().fold(Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), |a, p| Vec3::new(a.x.min(p.x), a.y.min(p.y), a.z.min(p.z)));
        let hi = q
            .iter()
            .fold(Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| Vec3::new(a.x.max(p.x), a.y.max(p.y), a.z.max(p.z)));
        (lo, hi)
    };
    let fitted = h.app.cam.distance;
    for (len, x1) in [(None, 60.0), (Some("40 mm"), 80.0)] {
        if let Some(len) = len {
            h.app.run("parameters.change", json!({"name":"len","expression":len})).unwrap();
            h.settle();
        }
        for distance in [fitted, 25.0] {
            h.app.cam.distance = distance;
            h.settle();
            let quads = crate::viewport::construction_quads(&h.app);
            let quad = |name: &str| quads.iter().find(|q| q.0 == name).map(|q| q.2).unwrap_or_else(|| panic!("{name} is drawn: {quads:?}"));
            for (name, z) in [("Above", 15.0), ("Higher", 20.0)] {
                let (lo, hi) = bounds(quad(name));
                let at = format!("{name}, box to x {x1}, distance {distance}: {lo:?} {hi:?}");
                assert!((lo.z - z).abs() < 1e-9 && (hi.z - z).abs() < 1e-9, "{at}");
                assert!(lo.x <= 40.0 && hi.x >= x1 && lo.y <= 30.0 && hi.y >= 50.0, "covers the top face: {at}");
                let (cx, cy) = (0.5 * (lo.x + hi.x), 0.5 * (lo.y + hi.y));
                assert!((cx - 0.5 * (40.0 + x1)).abs() < 1e-6 && (cy - 40.0).abs() < 1e-6, "centred on the top face: {at}");
            }
            let h_side = crate::selection::origin_size(h.app.cam.half_height()) * 1.2;
            let clear = quad("Clear");
            assert!((clear[0].dist(clear[1]) - 2.0 * h_side).abs() < 1e-6 * h_side.max(1.0), "distance {distance}: {clear:?}");
            let (lo, hi) = bounds(clear);
            assert!((0.5 * (lo.x + hi.x)).abs() < 1e-9 && (0.5 * (lo.y + hi.y)).abs() < 1e-9, "distance {distance}: round its origin: {clear:?}");
        }
    }
}
