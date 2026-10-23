//! Sketch on an origin plane from either side: the new sketch faces the viewer and the view
//! turns to it from where it was, never round to the far side (#1586).

use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::doc::{FeatureKind, PlaneRef};
use solidcraft_engine::geom::{Plane, Vec3};

fn sketch_from(yaw: f64, pitch: f64, name: &str) -> (Harness, Vec3, Plane) {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    h.app.cam.yaw = yaw;
    h.app.cam.pitch = pitch;
    let eye = h.app.cam.eye();
    h.step(&json!({"start":"sketch.create"})).unwrap();
    h.step(&json!({"click":{"plane":name}})).unwrap();
    h.settle();
    let plane = h.app.session.world_state().sketches.first().map(|s| s.plane).expect("a sketch on the plane");
    (h, eye, plane)
}

#[test]
fn a_new_sketch_on_an_origin_plane_faces_the_side_it_was_started_from() {
    // (yaw, pitch, plane, the plane's own normal); each plane from both of its sides.
    let cases = [
        (0.4, 0.4, "XZ", Vec3::Y),
        (std::f64::consts::PI - 0.4, 0.4, "XZ", Vec3::Y),
        (-1.2, 0.4, "YZ", Vec3::X),
        (1.2, 0.4, "YZ", Vec3::X),
        (-0.6, 0.5, "XY", Vec3::Z),
        (-0.6, -0.5, "XY", Vec3::Z),
    ];
    for (yaw, pitch, name, own) in cases {
        let (h, eye, plane) = sketch_from(yaw, pitch, name);
        let n = plane.normal();
        let what = format!("{name} from yaw {yaw} pitch {pitch}");
        assert!(n.dot(own).abs() > 0.999, "{what}: sketched on {n:?}");
        assert!(eye.dot(n) > 0.0, "{what}: the sketch normal {n:?} points away from the eye {eye:?}");
        // The view looks straight at the sketch from the side it was looking from.
        let after = h.app.cam.eye();
        assert!(after.dot(own * eye.dot(own).signum()) > 0.0, "{what}: the view turned to the far side {after:?}");
        let (right, up, back) = h.app.cam.basis();
        assert!(back.dot(n) > 0.999, "{what}: not looking straight at the sketch ({back:?})");
        // Only a sketch from behind the plane saves the flip, turned so its x runs right and
        // its y up on screen (a plane's own side keeps its own axes, as before).
        let sketch = h.app.session.active_sketch.unwrap();
        let FeatureKind::Sketch { plane: saved, .. } = &h.app.session.doc.feature(sketch).unwrap().kind else { panic!() };
        let behind = eye.dot(own) < 0.0;
        if behind {
            assert!(right.dot(plane.x) > 0.999 && up.dot(plane.y) > 0.999, "{what}: sketch axes {:?} {:?} on screen", plane.x, plane.y);
        }
        assert_eq!(matches!(saved, PlaneRef::Flipped { .. }), behind, "{what}: saved {saved:?}");
    }
}

#[test]
fn a_flipped_sketch_extrudes_toward_the_viewer_and_reopens_the_same() {
    let (mut h, _, plane) = sketch_from(0.4, 0.4, "XZ");
    assert!(plane.normal().dot(Vec3::Y) < -0.999);
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,5]})).unwrap();
    let sketch = h.app.session.active_sketch.unwrap();
    h.app.finish_sketch();
    h.app.run("solid.extrude", json!({"sketch":sketch,"distance":4,"operation":"new_body","body_name":"Block"})).unwrap();
    let bounds = |h: &Harness| h.app.session.world_state().body("Block").map(|b| b.mesh().bounds()).expect("an extruded body");
    let before = bounds(&h);
    assert!(before.min.y < -3.9 && before.max.y < 1e-6, "extruded away from the viewer: {before:?}");
    let path = h.dir.join("flipped-sketch.solidcraft");
    h.app.run("file.save", json!({"path":path})).unwrap();
    h.app.run("doc.open", json!({"path":path})).unwrap();
    assert_eq!(bounds(&h), before);
}

#[test]
fn a_construction_plane_seen_from_below_gets_a_sketch_facing_down() {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    h.app.run("construct.plane.offset", json!({"base":"XY","offset":"5 mm","name":"P"})).unwrap();
    h.app.cam.yaw = -0.6;
    h.app.cam.pitch = -0.5;
    h.app.session.selection = vec![solidcraft_engine::Sel::Plane { name: "P".into() }];
    h.step(&json!({"key":"L"})).unwrap();
    h.settle();
    let sketch = h.app.session.active_sketch.expect("a sketch on P");
    let plane = h.app.session.world_state().sketch(sketch).map(|s| s.plane).unwrap();
    assert!(plane.normal().z < -0.999 && (plane.origin.z - 5.0).abs() < 1e-9, "{plane:?}");
    assert!(h.app.cam.eye().z < 5.0, "the view stays below the plane");
}
