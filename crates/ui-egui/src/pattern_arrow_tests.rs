//! A rectangular pattern's second direction has its own drag arrow for Spacing 2 (#2462), as
//! the first direction's arrow sets Spacing.

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at};
use crate::sketch_modify_ui::Mod;
use serde_json::json;

fn spacings(h: &Harness) -> (String, String) {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::PatternRect { spacing, spacing2, .. }) | Some(Kind::SketchModify(Mod::PatternRect { spacing, spacing2, .. })) => {
            (spacing.clone(), spacing2.clone())
        }
        other => panic!("no rectangular pattern window: {other:?}"),
    }
}

#[test]
fn a_sketch_pattern_drags_its_second_spacing_square_to_the_first() {
    let mut h = Harness::new();
    h.run(&json!([
        {"start": "sketch.create"},
        {"click": {"plane": "XY"}},
        {"start": "sketch.circle.center"},
        {"click": {"sketch": [0, 0]}},
        {"click": {"sketch": [5, 0]}},
        {"key": "Escape"},
        {"start": "sketch.pattern.rectangular"},
        {"click": {"sketch": [5, 0]}}
    ]))
    .unwrap();
    // Keep both arrows in the exposed viewport beside the left dialog. The full-bleed
    // projection otherwise puts Spacing 2 under the dialog's top edge.
    h.call("ui.camera", json!({"target":[-30,0,0]}));
    h.settle();
    let (Some(a1), Some(a2)) = (handle_at("arrow"), handle_at("arrow2")) else {
        panic!("both arrows: {:?} {:?}", handle_at("arrow"), handle_at("arrow2"));
    };
    // Looking down on XY, the first direction (sketch x) points right and the second (square to
    // it) up the screen: the arrows share their base, so each lies along its own axis.
    assert!(a1.x > a2.x + 10.0 && (a2.y - a1.y).abs() > 10.0, "arrow {a1:?}, arrow2 {a2:?}");
    assert!(!crate::shell::covered(&h.app, a1));
    assert!(!crate::shell::covered(&h.app, a2));
    let (spacing, spacing2) = spacings(&h);
    // Dragging the second arrow further out sets Spacing 2 only.
    h.call("ui.drag", json!({"x0": a2.x, "y0": a2.y, "x1": a2.x, "y1": a2.y - 60.0, "steps": 6}));
    h.settle();
    let (after, after2) = spacings(&h);
    assert_eq!(after, spacing);
    assert_ne!(after2, spacing2);
    let grown = h.app.session.doc.eval(&after2, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    let was = h.app.session.doc.eval(&spacing2, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!(grown > was, "Spacing 2 {spacing2} -> {after2}");
}

#[test]
fn a_solid_pattern_shows_the_second_arrow_once_it_has_a_second_direction() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}},
        {"call": "ui.view", "params": {"view": "iso"}},
        {"start": "solid.pattern.rectangular"},
        {"click": {"world": [5, 5, 10]}},
        {"call": "ui.dialogInput", "params": {"index": 1}},
        {"click": {"axis": "X"}}
    ]))
    .unwrap();
    h.settle();
    assert!(handle_at("arrow").is_some());
    assert!(handle_at("arrow2").is_none(), "no second direction yet, so no second arrow");
    h.run(&json!([
        {"call": "ui.dialogInput", "params": {"index": 2}},
        {"click": {"axis": "Y"}}
    ]))
    .unwrap();
    h.settle();
    let Some(a2) = handle_at("arrow2") else { panic!("no second arrow with a second direction") };
    let (spacing, spacing2) = spacings(&h);
    let toward = h.call("ui.worldToScreen", json!({"points": [[5, 60, 5]]}))["result"][0].clone();
    let (tx, ty) = (toward[0].as_f64().unwrap() as f32, toward[1].as_f64().unwrap() as f32);
    // Drag it along +Y (towards a point far out on Y): Spacing 2 changes, Spacing doesn't.
    let (dx, dy) = (tx - a2.x, ty - a2.y);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    h.call("ui.drag", json!({"x0": a2.x, "y0": a2.y, "x1": a2.x + dx / len * 60.0, "y1": a2.y + dy / len * 60.0, "steps": 6}));
    h.settle();
    let (after, after2) = spacings(&h);
    assert_eq!(after, spacing);
    assert_ne!(after2, spacing2);
}

/// Dragging a solid pattern's Spacing arrow back past its start gives a negative spacing, as
/// typing one does (#5497): the arrow was floored at one snap step.
#[test]
fn a_solid_pattern_spacing_arrow_dragged_past_zero_goes_negative() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}},
        {"call": "ui.view", "params": {"view": "iso"}},
        {"start": "solid.pattern.rectangular"},
        {"click": {"world": [5, 5, 10]}},
        {"call": "ui.dialogInput", "params": {"index": 1}},
        {"click": {"axis": "X"}}
    ]))
    .unwrap();
    // Keep the arrow's tip clear of its value box, which the iso view pushes onto it at the
    // viewport's edge.
    h.call("ui.camera", json!({"target": [15, 5, 5]}));
    h.settle();
    let (at, dir) = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).expect("an anchor");
    let to = at - dir.expect("a direction") * 10.0;
    h.step(&json!({"drag": [{"handle": "arrow"}, {"world": [to.x, to.y, to.z]}], "ctrl": true})).unwrap();
    let (spacing, _) = spacings(&h);
    let v = h.app.session.doc.eval(&spacing, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((v + 10.0).abs() < 0.5, "dragging 10 mm back from the start gave Spacing {spacing}");
    // Applied, the copies sit on the negative side, as a typed negative spacing puts them.
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.status);
    let st = h.app.session.world_state();
    for (copy, k) in [("Body1 (2)", 1.0), ("Body1 (3)", 2.0)] {
        let x = st.body(copy).unwrap_or_else(|| panic!("no {copy}: {:?}", h.app.status)).mesh().bounds().center().x;
        assert!((x - (5.0 + k * v)).abs() < 1e-6, "{copy} centred at x = {x} for Spacing {spacing}");
    }
}
