//! The hole markers (#6818): one per placed hole, each the shared marker with its press area and
//! its grab offset; a click on one takes the hole out; the depth arrow stands clear of them.

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::scenario::{Harness, handle_at};

/// A 40 × 30 × 20 block and a second one beside it, seen from the top, with holes moving (and
/// not snapping, so a place is exactly where it is let go).
fn blocks() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    crate::prefs::set(&mut h.app, &json!({"hole_moves": true})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 20, "corner": [50, 0, 0], "body_name": "Other"})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

/// Where a world point is on screen.
fn screen(h: &mut Harness, p: Vec3) -> [f64; 2] {
    h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap()
}

/// The face places the open dialog holds.
fn places(h: &Harness) -> Vec<Vec3> {
    let d = h.app.dialog.as_ref().expect("a dialog is open");
    d.inputs[0]
        .items
        .iter()
        .map(|x| match x {
            Sel::Face { point, .. } => *point,
            other => panic!("a face place, not {other:?}"),
        })
        .collect()
}

#[test]
fn a_press_off_the_centre_keeps_its_gap_and_the_hole_lands_short_of_the_pointer() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let start = Vec3::new(10.0, 10.0, 20.0);
    let at = screen(&mut h, start);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let c = handle_at("hole:0").expect("the placed hole has a marker");
    // Grabbed 6 pt right and 4 pt below the centre (inside the shared marker's reach, outside the
    // old handle's 9 pt), and let go with the pointer the same gap from `to`: the hole lands on
    // `to`, not under the pointer.
    let to = Vec3::new(30.0, 20.0, 20.0);
    let ts = screen(&mut h, to);
    h.step(&json!({"drag": [[f64::from(c.x) + 6.0, f64::from(c.y) + 4.0], [ts[0] + 6.0, ts[1] + 4.0]], "steps": 12})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [p] if p.dist(to) < 0.3), "kept the gap: {p:?}");
}

#[test]
fn each_placed_hole_has_its_own_marker_and_a_drag_moves_only_its_hole() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let first = Vec3::new(10.0, 10.0, 20.0);
    let second = Vec3::new(60.0, 15.0, 20.0);
    let at = screen(&mut h, first);
    h.step(&json!({"click": at})).unwrap();
    let at = screen(&mut h, second);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert_eq!(places(&h).len(), 2, "{:?}", places(&h));
    let (Some(a), Some(b)) = (handle_at("hole:0"), handle_at("hole:1")) else { panic!("two markers") };
    assert!(a.distance(b) > 50.0, "one marker per hole: {a:?} {b:?}");
    assert!(handle_at("hole:2").is_none());
    // The second hole dragged across its block: the first stays.
    let to = Vec3::new(55.0, 25.0, 20.0);
    let ts = screen(&mut h, to);
    h.step(&json!({"drag": [{"handle": "hole:1"}, ts], "steps": 12})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [a, b] if a.dist(first) < 1e-4 && b.dist(to) < 0.3), "{p:?}");
}

#[test]
fn a_click_within_the_markers_reach_takes_the_hole_out() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let c = handle_at("hole:0").expect("a marker");
    // 14 pt from the centre: on the marker's arm, past the old handle's 9 pt.
    h.step(&json!({"click": [f64::from(c.x) + 14.0, f64::from(c.y)]})).unwrap();
    h.frames(2);
    assert!(places(&h).is_empty(), "{:?}", places(&h));
    assert!(handle_at("hole:0").is_none(), "no hole, no marker");
}

#[test]
fn the_depth_arrow_stands_clear_of_the_marker_only_while_holes_move() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let d = h.app.dialog.clone().expect("a dialog");
    assert!(crate::place_gizmo::arrow_clearance(&h.app, &d) > 0.0, "the arrow starts past the marker");
    crate::prefs::set(&mut h.app, &json!({"hole_moves": false})).unwrap();
    assert_eq!(crate::place_gizmo::arrow_clearance(&h.app, &d), 0.0, "no marker, no clearance");
}

#[test]
fn the_markers_follow_the_holes_own_setting_not_the_shared_one() {
    let mut h = blocks();
    crate::prefs::set(&mut h.app, &json!({"hole_moves": false, "placement_handles": true})).unwrap();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert_eq!(places(&h).len(), 1);
    assert!(handle_at("hole:0").is_none(), "Placement handles alone show no hole marker");
    crate::prefs::set(&mut h.app, &json!({"hole_moves": true, "placement_handles": false})).unwrap();
    h.frames(2);
    assert!(handle_at("hole:0").is_some(), "Move holes by clicking or dragging alone shows it");
}

#[test]
fn a_hole_whose_face_is_gone_keeps_its_marker_through_the_restored_direction() {
    // Edit Feature restores a hole whose face no longer resolves as a bare place with the
    // `direction` it had (dialogs.rs); the marker stands there all the same.
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert!(handle_at("hole:0").is_some());
    {
        let d = h.app.dialog.as_mut().expect("a dialog");
        d.inputs[0].items = vec![Sel::Face { body: String::new(), index: 0, point: Vec3::new(10.0, 10.0, 20.0) }];
        d.extra.insert("direction".into(), crate::place_gizmo::pt(Vec3::new(0.0, 0.0, 1.0)));
    }
    h.frames(2);
    assert!(handle_at("hole:0").is_some(), "the marker stays with the place");
    let d = h.app.dialog.as_mut().expect("a dialog");
    d.extra.remove("direction");
    h.frames(2);
    assert!(handle_at("hole:0").is_none(), "no face and no direction: nowhere to stand");
}
