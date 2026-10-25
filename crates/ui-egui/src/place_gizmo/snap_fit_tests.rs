//! The Snap Fit handles: the marker drags the arm onto a sketch
//! point (#3423), the grip points the catch and the Catch Direction combo follows.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::dialogs_plastic::{Pl, hook_dirs};
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

fn hook(h: &Harness) -> usize {
    match &h.app.dialog.as_ref().expect("a dialog is open").kind {
        Kind::Plastic(Pl::SnapFit { hook, .. }) => *hook,
        other => panic!("a Snap Fit dialog, not {other:?}"),
    }
}

#[test]
fn marker_drags_the_snap_fit_onto_a_sketch_point_and_the_grip_points_the_catch() {
    let mut h = plate();
    h.app.run("sketch.create", json!({"plane": {"face": [40, 25, 10]}})).unwrap();
    h.app.run("sketch.point", json!({"point": [50, 30]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"start": "plastic.snap_fit"})).unwrap();
    h.step(&json!({"click": {"world": [15, 12, 10]}})).unwrap();
    h.settle();
    h.step(&json!({"drag": [{"handle": "snap_fit:position"}, {"world": [50, 30, 10]}], "steps": 12})).unwrap();
    h.settle();
    assert!(position(&h).dist(Vec3::new(50.0, 30.0, 10.0)) < 1e-6, "snapped to the sketch point: {:?}", position(&h));

    // Turn the grip a quarter about the marker on screen: the catch takes the next direction.
    let before = hook(&h);
    let ball = h.at(&json!({"handle": "snap_fit:position"})).unwrap();
    let grip = h.at(&json!({"handle": "snap_fit:rotate"})).unwrap();
    let (dx, dy) = (grip[0] - ball[0], grip[1] - ball[1]);
    assert!(dx.hypot(dy) > 20.0, "the grip stands off the marker: {grip:?} {ball:?}");
    h.step(&json!({"drag": [grip, [ball[0] - dy, ball[1] + dx]], "steps": 16})).unwrap();
    h.settle();
    let after = hook(&h);
    assert_ne!(after, before, "the catch turned");
    let n = Vec3::new(0.0, 0.0, 1.0);
    let (b, a) = (hook_dirs(n)[before].1, hook_dirs(n)[after].1);
    assert!(a.dot(b).abs() < 1e-9, "a quarter turn: {b:?} -> {a:?}");

    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog");
    let f = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::SnapFit { .. })).expect("a Snap Fit was made");
    let FeatureKind::SnapFit { position, hook, .. } = &f.kind else { panic!("a Snap Fit: {:?}", f.kind) };
    assert!(position.0.dist(Vec3::new(50.0, 30.0, 10.0)) < 1e-6, "the place: {:?}", position.0);
    assert!(hook.0.dist(a) < 1e-9, "the catch direction: {:?}", hook.0);
    h.step(&json!({"expect": {"errors": 0, "bodies": 1}})).unwrap();
}
