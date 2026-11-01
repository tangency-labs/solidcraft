//! Look At on a sketch (#3198): it faces the sketch and frames its geometry, not its plane's
//! origin, so a sketch drawn far from the origin (an imported part placed there) stays in view.

use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::scenario::Harness;

/// A 20 × 30 rectangle at x 330–350, y 475–505 on the XY plane raised 8 mm, as an imported part
/// rebuilt where it was placed; the camera jumps straight to where Look At sends it.
fn far_sketch() -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane": "XY", "offset": 8, "name": "Far"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [330, 475], "p1": [350, 505]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let id = h.app.session.doc.find_feature("Far").map(|f| f.id).unwrap();
    (h, id)
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).len() < 1e-6
}

/// The camera's view half-width (mm) the rectangle needs: its half diagonal.
const HALF_DIAGONAL: f64 = 18.027_756_377_319_946;

#[test]
fn the_menus_look_at_frames_a_sketch_far_from_the_origin() {
    let (mut h, id) = far_sketch();
    h.app.cam.distance = 20.0;
    let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Sketch { id });
    let look = items.iter().find(|i| i.id == "ui.lookAt").cloned().expect("the sketch menu has Look At");
    crate::context_menu::run_item(&mut h.app, &look, egui::Pos2::ZERO);
    assert!(near(h.app.cam.target, Vec3::new(340.0, 490.0, 8.0)), "centred on the rectangle, not the plane's origin: {:?}", h.app.cam.target);
    assert!((h.app.cam.back() - Vec3::new(0.0, 0.0, 1.0)).len() < 1e-6, "facing the sketch from above");
    let need = HALF_DIAGONAL / 0.4 * 1.15;
    assert!(h.app.cam.distance >= need - 1e-6, "zoomed out to fit it: {} < {need}", h.app.cam.distance);
}

#[test]
fn look_at_on_the_active_sketch_keeps_a_wider_zoom() {
    let (mut h, id) = far_sketch();
    h.app.session.active_sketch = Some(id);
    h.app.cam.distance = 5000.0;
    crate::dialogs::look_at_sketch(&mut h.app);
    assert!(near(h.app.cam.target, Vec3::new(340.0, 490.0, 8.0)), "{:?}", h.app.cam.target);
    assert_eq!(h.app.cam.distance, 5000.0, "it never zooms in on a zoom the user chose");
}

#[test]
fn an_empty_sketch_keeps_its_planes_origin() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane": "XY", "offset": 8})).unwrap();
    h.app.cam.distance = 123.0;
    crate::dialogs::look_at_sketch(&mut h.app);
    assert!(near(h.app.cam.target, Vec3::new(0.0, 0.0, 8.0)), "{:?}", h.app.cam.target);
    assert_eq!(h.app.cam.distance, 123.0);
}

/// The viewport menu's Look At (#4105): the same turn as the navigation bar's, for the planar
/// face under the right-click and for a plane.
#[test]
fn the_viewport_menu_looks_at_a_planar_face() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 10})).unwrap();
    let body = h.app.session.model.state().bodies[0].clone();
    let face = body.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
    h.app.run("select.set", json!({"items": [{"type": "face", "body": body.name, "index": face.index, "point": face.centroid}]})).unwrap();
    let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
    let look = items.iter().find(|i| i.id == "ui.lookAt").cloned().expect("a face's menu has Look At");
    assert!(look.enabled);
    crate::context_menu::run_item(&mut h.app, &look, egui::Pos2::ZERO);
    assert!((h.app.cam.back() - Vec3::new(0.0, 0.0, 1.0)).len() < 1e-6, "facing the top face: {:?}", h.app.cam.back());
}

#[test]
fn the_viewport_menu_looks_at_a_plane_under_the_cursor() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.menu.plane = Some("XZ".to_string());
    let items = crate::context_menu::items(&h.app, &crate::context_menu::Target::Viewport);
    let look = items.iter().find(|i| i.id == "ui.lookAt").cloned().expect("a plane's menu has Look At");
    crate::context_menu::run_item(&mut h.app, &look, egui::Pos2::ZERO);
    let normal = solidcraft_engine::geom::Plane::named("XZ").unwrap().normal();
    assert!((h.app.cam.back().cross(normal)).len() < 1e-6, "facing the plane along its normal: {:?}", h.app.cam.back());
}

/// #9029: Look At on a non-horizontal sketch squares its axes to the screen (not only faces it).
#[test]
fn look_at_squares_a_vertical_sketch_to_the_screen() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.run("sketch.create", json!({"plane": "XZ", "name": "Wall"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]})).unwrap();
    crate::dialogs::look_at_sketch(&mut h.app);
    let plane = crate::open_meshing::session_world(&h.app.session).sketch(h.app.session.active_sketch.unwrap()).unwrap().plane;
    let (right, up, back) = h.app.cam.basis();
    assert!((back.cross(plane.normal())).len() < 1e-6, "facing the sketch: back={back:?} n={:?}", plane.normal());
    assert!(right.dot(plane.x).abs() > 0.999, "sketch x to the right: right={right:?} x={:?}", plane.x);
    assert!(up.dot(plane.y) > 0.999, "sketch y upright: up={up:?} y={:?}", plane.y);
}

/// #9029: editing a sketch after its component is turned faces the world plane and squares axes.
#[test]
fn editing_a_sketch_in_a_turned_component_faces_its_world_plane() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.look_at_anim = false;
    h.app.run("component.create", json!({"name": "Part"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Face"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let occ = h.app.session.doc.occurrences.iter().find(|o| h.app.session.doc.components.iter().any(|c| c.id == o.component && c.name == "Part"));
    let Some(occ) = occ.cloned() else { panic!("Part occurrence") };
    h.app.run("occurrence.ground", json!({"occurrence": occ.id, "grounded": false})).unwrap();
    // Stand the part up: 90° about X so its XY faces world +Y.
    h.app.run("occurrence.move", json!({"occurrence": occ.id, "axis": [1, 0, 0], "angle": "90 deg", "capture": true})).unwrap();
    let id = h.app.session.doc.find_feature("Face").map(|f| f.id).unwrap();
    h.app.edit_sketch(id);
    let plane = crate::open_meshing::session_world(&h.app.session).sketch(id).unwrap().plane;
    let (right, up, back) = h.app.cam.basis();
    assert!((back.cross(plane.normal())).len() < 1e-5, "facing world plane: back={back:?} n={:?}", plane.normal());
    assert!(right.dot(plane.x).abs() > 0.99, "x right: {right:?} vs {:?}", plane.x);
    assert!(up.dot(plane.y) > 0.99, "y up: {up:?} vs {:?}", plane.y);
}
