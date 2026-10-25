//! The Rest handles: the marker drags the pad across the model and snaps to
//! a sketch point as a click would; the grip turns a rectangular pad, also from Edit Feature.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::scenario::Harness;

/// An 80 × 50 × 10 plate seen from the top, with the handles on.
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

fn drag_handle(h: &mut Harness, name: &str, to: serde_json::Value) {
    h.step(&json!({"drag": [{"handle": name}, to], "steps": 12})).unwrap();
    h.settle();
}

#[test]
fn ball_drags_the_rest_across_the_face_and_snaps_to_a_sketch_point() {
    let mut h = plate();
    // A circle on the top face: its centre is a sketch point the pad can snap to.
    h.app.run("sketch.create", json!({"plane": {"face": [40, 25, 10]}})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [60, 40], "radius": 4})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"start": "plastic.rest"})).unwrap();
    h.step(&json!({"click": {"world": [15, 12, 10]}})).unwrap();
    h.settle();
    assert!(position(&h).dist(Vec3::new(15.0, 12.0, 10.0)) < 0.5, "placed where clicked: {:?}", position(&h));
    assert!(h.at(&json!({"handle": "rest:position"})).is_ok(), "the ball is published");

    drag_handle(&mut h, "rest:position", json!({"world": [40, 20, 10]}));
    assert!(position(&h).dist(Vec3::new(40.0, 20.0, 10.0)) < 0.5, "dragged across the face: {:?}", position(&h));

    drag_handle(&mut h, "rest:position", json!({"world": [60, 40, 10]}));
    let p = position(&h);
    assert!(p.dist(Vec3::new(60.0, 40.0, 10.0)) < 1e-6, "snapped to the circle's centre: {p:?}");
    assert!(h.app.dialog.as_ref().is_some_and(|d| d.error.is_none()));

    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog");
    let rest = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Rest { .. })).expect("a Rest was made");
    let FeatureKind::Rest { position, .. } = &rest.kind else { panic!("a Rest: {:?}", rest.kind) };
    assert!(position.0.dist(Vec3::new(60.0, 40.0, 10.0)) < 1e-6, "the Rest took the dragged place: {:?}", position.0);
}

#[test]
fn grip_turns_a_rectangular_rest_from_edit_feature() {
    let mut h = plate();
    let made = h.app.run("plastic.rest", json!({"position": [40, 25, 10], "width": 6, "length": 16, "height": 2})).unwrap();
    let name = made["name"].as_str().unwrap().to_string();
    h.settle();
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": name}})).unwrap();
    h.settle();
    let ball = h.at(&json!({"handle": "rest:position"})).unwrap();
    let grip = h.at(&json!({"handle": "rest:rotate"})).unwrap();
    assert!((grip[0] - ball[0]).hypot(grip[1] - ball[1]) > 20.0, "the grip stands off the ball: {grip:?} {ball:?}");
    // A quarter turn of the grip about the ball on screen (the top view looks straight down).
    let (dx, dy) = (grip[0] - ball[0], grip[1] - ball[1]);
    let to = [ball[0] - dy, ball[1] + dx];
    h.step(&json!({"drag": [grip, to], "steps": 16})).unwrap();
    h.settle();
    let along = h.app.dialog.as_ref().and_then(|d| d.extra.get("along")).cloned().expect("the dialog keeps `along`");
    let a: Vec<f64> = along.as_array().unwrap().iter().filter_map(|v| v.as_f64()).collect();
    // Untouched, a pad on a Z face runs along Y; a quarter turn puts it along X.
    assert!(a[0].abs() > 0.99 && a[1].abs() < 0.02, "turned a quarter: {a:?}");

    h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
    h.settle();
    assert!(h.app.dialog.is_none(), "OK closed the dialog");
    let rest = h.app.session.doc.find_feature(&name).expect("the Rest is still there");
    let FeatureKind::Rest { along: Some(along), length: Some(_), .. } = &rest.kind else {
        panic!("a rectangular Rest with `along`: {:?}", rest.kind)
    };
    assert!(along.0.x.abs() > 0.99, "the feature keeps the turn: {:?}", along.0);
    h.step(&json!({"expect": {"errors": 0, "bodies": 1}})).unwrap();
}

/// Seen from the home view, the Height arrow's stem starts at the pad's centre, where the ball
/// sits. A press there belongs to the ball: the pad moves, the height stays.
#[test]
fn ball_wins_the_press_over_the_height_arrow_at_the_base() {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.preferences.placement_handles = true;
    h.app.run("solid.box", json!({"length": 80, "width": 50, "height": 10})).unwrap();
    h.settle();
    h.step(&json!({"start": "plastic.rest"})).unwrap();
    h.step(&json!({"click": {"world": [20, 15, 10]}})).unwrap();
    h.settle();
    let height_before = match &h.app.dialog.as_ref().expect("a dialog").kind {
        crate::dialogs::Kind::Plastic(crate::dialogs_plastic::Pl::Rest { height, .. }) => height.clone(),
        other => panic!("a Rest dialog, not {other:?}"),
    };
    // Up and to the right on screen, the way the arrow points in the home view.
    drag_handle(&mut h, "rest:position", json!({"world": [45, 35, 10]}));
    assert!(position(&h).dist(Vec3::new(45.0, 35.0, 10.0)) < 0.5, "the pad moved: {:?}", position(&h));
    let height_after = match &h.app.dialog.as_ref().expect("a dialog").kind {
        crate::dialogs::Kind::Plastic(crate::dialogs_plastic::Pl::Rest { height, .. }) => height.clone(),
        other => panic!("a Rest dialog, not {other:?}"),
    };
    assert_eq!(height_after, height_before, "the Height arrow did not take the press");
}

/// Grabbing the marker off its centre must not jump the pad: the drag keeps the offset from the
/// press, so the pad moves by the drag vector, not by the vector plus the off-centre grab.
#[test]
fn a_press_off_centre_does_not_jump_the_pad() {
    let mut h = plate();
    h.step(&json!({"start": "plastic.rest"})).unwrap();
    h.step(&json!({"click": {"world": [40, 25, 10]}})).unwrap();
    h.settle();
    let start = h.at(&json!({"handle": "rest:position"})).unwrap();
    // Press 14 px off centre (inside the marker's reach), then drag by a known screen vector.
    let off = 14.0;
    let (dx, dy) = (60.0, 30.0);
    let from = json!([start[0] + off, start[1]]);
    let to = json!([start[0] + off + dx, start[1] + dy]);
    h.step(&json!({"drag": [from, to], "steps": 10})).unwrap();
    h.settle();
    let end = h.at(&json!({"handle": "rest:position"})).unwrap();
    // The pad followed the drag vector. Without the grab offset it would land `off` further along.
    assert!((end[0] - (start[0] + dx)).abs() < 6.0, "x jumped: start {start:?} end {end:?}");
    assert!((end[1] - (start[1] + dy)).abs() < 6.0, "y jumped: start {start:?} end {end:?}");
}

/// A still press on the marker is someone thinking about where to drag: it never opens Select
/// Other's list, even where the list has things to offer (a sketch point on a face).
#[test]
fn holding_the_marker_still_never_opens_select_other() {
    let mut h = plate();
    h.app.run("sketch.create", json!({"plane": {"face": [40, 25, 10]}})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [40, 25], "radius": 4})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"start": "plastic.rest"})).unwrap();
    h.step(&json!({"click": {"world": [40, 25, 10]}})).unwrap();
    h.settle();
    h.step(&json!({"hold": {"handle": "rest:position"}})).unwrap();
    assert!(!crate::select_other::is_open(&h.app), "a held press on the marker opened Select Other");
    // Away from the marker the same hold still opens it.
    h.step(&json!({"hold": {"world": [40, 29, 10]}})).unwrap();
    assert!(crate::select_other::is_open(&h.app), "a held press on the circle opens Select Other");
}

/// With the preference off (the default) nothing changes: the Position input takes faces only, no
/// handle is drawn, and the pad stays where it was clicked.
#[test]
fn with_the_preference_off_a_rest_is_placed_by_its_click_alone() {
    let mut h = plate();
    h.app.preferences.placement_handles = false;
    h.step(&json!({"start": "plastic.rest"})).unwrap();
    h.step(&json!({"click": {"world": [15, 12, 10]}})).unwrap();
    h.settle();
    let accept = h.app.dialog.as_ref().expect("a dialog").inputs.first().expect("a Position input").accept;
    assert_eq!(accept & crate::selection::POINTS, 0, "the Position input takes faces only");
    assert!(h.at(&json!({"handle": "rest:position"})).is_err(), "no marker is published");
    assert!(h.at(&json!({"handle": "rest:rotate"})).is_err(), "no grip is published");
    assert!(position(&h).dist(Vec3::new(15.0, 12.0, 10.0)) < 0.5, "placed where clicked: {:?}", position(&h));
}
