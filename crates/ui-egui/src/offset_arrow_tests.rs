//! Sketch offset Two Sides has its own drag arrow for Distance 2 (#4285), pointing the
//! other way from Distance. One Side has only the first arrow.

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at};
use crate::sketch_modify_ui::{Mod, TWO_SIDES};
use serde_json::json;

fn distances(h: &Harness) -> (String, String) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::SketchModify(Mod::Offset { distance, distance2, .. })) => (distance.clone(), distance2.clone()),
        other => panic!("no offset window: {other:?}"),
    }
}

fn screen(h: &mut Harness, point: [f64; 3]) -> egui::Pos2 {
    let at = h.call("ui.worldToScreen", json!({"points": [point]}))["result"][0].clone();
    egui::pos2(at[0].as_f64().unwrap() as f32, at[1].as_f64().unwrap() as f32)
}

#[test]
fn a_two_sided_sketch_offset_drags_distance_2_the_other_way() {
    let mut h = Harness::new();
    h.app.preferences.look_at_anim = false;
    h.run(&json!([
        {"start": "sketch.create"},
        {"click": {"plane": "XY"}},
        {"start": "sketch.line"},
        {"click": {"sketch": [0, 0]}},
        {"click": {"sketch": [40, 0]}},
        {"key": "Escape"},
        {"call": "ui.view", "params": {"view": "top"}},
        {"call": "ui.camera", "params": {"target": [-30, 0, 0], "distance": 180}},
        {"start": "sketch.offset"},
        {"click": {"sketch": [20, 0]}}
    ]))
    .unwrap();
    h.settle();
    let picked = h.app.dialog.as_ref().map(|d| d.inputs.first().map(|i| i.items.len()).unwrap_or(0));
    assert!(handle_at("arrow").is_some(), "Distance has a drag arrow (picked {picked:?})");
    assert!(handle_at("arrow2").is_none(), "One Side has no Distance 2 arrow");

    let Kind::SketchModify(Mod::Offset { mode, .. }) = &mut h.app.dialog.as_mut().expect("offset window").kind else {
        panic!("offset window");
    };
    *mode = TWO_SIDES;
    h.settle();

    let (Some(a1), Some(a2)) = (handle_at("arrow"), handle_at("arrow2")) else {
        panic!("both arrows: {:?} {:?}", handle_at("arrow"), handle_at("arrow2"));
    };
    let base = screen(&mut h, [20.0, 0.0, 0.0]);
    let (v1x, v1y) = (a1.x - base.x, a1.y - base.y);
    let (v2x, v2y) = (a2.x - base.x, a2.y - base.y);
    assert!(v1x * v2x + v1y * v2y < 0.0, "Distance 2 arrow {a2:?} should oppose Distance {a1:?} from {base:?}");

    let (distance, distance2) = distances(&h);
    let (dx, dy) = (v2x, v2y);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    h.call("ui.drag", json!({"x0": a2.x, "y0": a2.y, "x1": a2.x + dx / len * 70.0, "y1": a2.y + dy / len * 70.0, "steps": 6}));
    h.settle();
    let (after, after2) = distances(&h);
    assert_eq!(after, distance, "dragging Distance 2 leaves Distance alone");
    let was = h.app.session.doc.eval(&distance2, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    let grown = h.app.session.doc.eval(&after2, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!(grown > was, "Distance 2 {distance2} -> {after2}");
}
