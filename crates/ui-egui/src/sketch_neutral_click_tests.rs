use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::geom::prelude::*;

/// #16994: the point a click places lands where the pointer is. The horizontal or vertical
/// inference turns the line to the axis, and the end of the line moves with it: on a long line
/// at a small angle the end landed millimetres away from the pointer, so the geometry had to be
/// nudged into place afterwards. The inference turns the line only while its end stays within
/// the pointer's reach, the reach every other snap uses.
#[test]
fn a_line_end_that_turns_to_an_axis_stays_within_the_pointer_reach() {
    use solidcore_record::Vec2;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.settle();
    h.app.cam.distance = 120.0;
    h.settle();
    h.step(&json!({"start": "sketch.line"})).unwrap();
    h.step(&json!({"click": {"sketch": [0, 0]}})).unwrap();
    h.frames(2);
    // The end of the line: 45 mm along and two degrees off the horizontal, near enough for the
    // inference, which turns the line and moves its end about 1.6 mm from the pointer.
    let end = [44.972, 1.570];
    h.step(&json!({"move": {"sketch": end}})).unwrap();
    h.frames(2);
    h.step(&json!({"click": {"sketch": end}})).unwrap();
    h.frames(2);
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let placed = sk.points.iter().map(|p| p.pos).find(|p| p.dist(Vec2::ZERO) > 1.0).expect("the line's end is in the sketch");
    let off = placed.dist(Vec2::new(end[0], end[1]));
    assert!(off < 0.5, "the end landed {off} mm from the pointer: {placed:?}");
}

/// A line the pointer holds within the reach of the axis still turns to it, and still gets its
/// Horizontal constraint: the inference is bounded, not gone (#16994).
#[test]
fn a_line_end_within_the_pointer_reach_still_turns_to_the_axis() {
    use solidcore_record::Vec2;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.settle();
    h.app.cam.distance = 120.0;
    h.settle();
    h.step(&json!({"start": "sketch.line"})).unwrap();
    h.step(&json!({"click": {"sketch": [0, 0]}})).unwrap();
    h.frames(2);
    // 45 mm along and 0.2 mm up: the pointer is within the reach of the horizontal through the
    // start, so the line turns to it.
    let end = [45.0, 0.2];
    h.step(&json!({"move": {"sketch": end}})).unwrap();
    h.frames(2);
    h.step(&json!({"click": {"sketch": end}})).unwrap();
    h.frames(2);
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.model.state().sketch(sid).unwrap().sketch.clone();
    let placed = sk.points.iter().map(|p| p.pos).find(|p| p.dist(Vec2::ZERO) > 1.0).expect("the line's end is in the sketch");
    assert!(placed.y.abs() < 1e-9, "the line is horizontal: {placed:?}");
    assert!(
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Horizontal { .. })),
        "the line is constrained: {:?}",
        sk.constraints
    );
}

/// A picture of the view with the line being drawn: where the pointer is (the ring) and where
/// the line's end goes (the band). The evidence for the pull request.
#[test]
fn evidence_the_line_end_lands_away_from_the_pointer() {
    let Ok(dir) = std::env::var("SOLIDCRAFT_16994_EVIDENCE") else { return };
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.call("ui.view", json!({"view": "top"}));
    h.settle();
    h.app.cam.distance = 120.0;
    h.settle();
    h.step(&json!({"start": "sketch.line"})).unwrap();
    h.step(&json!({"click": {"sketch": [0, 0]}})).unwrap();
    h.frames(2);
    h.step(&json!({"move": {"sketch": [44.972, 1.570]}})).unwrap();
    h.frames(2);
    let rect = h.app.viewport.rect.expect("the viewport");
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    let mut last = None;
    for _ in 0..4 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
            focused: true,
            ..Default::default()
        };
        let app = &mut h.app;
        let output = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
        last = Some(output);
    }
    let output = last.expect("a frame");
    std::fs::create_dir_all(&dir).unwrap();
    crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, rect, &textures)
        .save(std::path::Path::new(&dir).join("line-end.png"))
        .unwrap();
}

#[test]
fn repeated_first_point_is_neutral_and_drawing_can_continue() {
    for (command, rest) in [
        ("sketch.rectangle.two_point", vec![[10, 5, 0]]),
        ("sketch.rectangle.center", vec![[10, 5, 0]]),
        ("sketch.rectangle.three_point", vec![[10, 0, 0], [10, 5, 0]]),
        ("sketch.circle.center", vec![[5, 0, 0]]),
        ("sketch.circle.three_point", vec![[10, 0, 0], [5, 5, 0]]),
        ("sketch.line", vec![[10, 5, 0]]),
        ("sketch.polygon.inscribed", vec![[10, 5, 0]]),
        ("sketch.slot.center_to_center", vec![[10, 0, 0], [10, 3, 0]]),
    ] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.cam.distance = 50.0;
        h.step(&json!({"start":command})).unwrap();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let p = h.call("ui.at", json!({"sketch":[0,0]}))["result"].clone();
        h.call("ui.click", json!({"x":p[0],"y":p[1],"double":true}));
        assert!(!h.app.status.as_ref().is_some_and(|s| s.2), "{command}: {:?}", h.app.status);
        assert_eq!(h.app.tool.as_ref().unwrap().pts.len(), 1, "{command}");
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
        for point in rest {
            h.step(&json!({"move":{"world":point}})).unwrap();
            if command == "sketch.slot.center_to_center" {
                // The live width editor is placed at the cursor; Enter accepts it.
                h.step(&json!({"key":"Enter"})).unwrap();
            } else {
                h.step(&json!({"click":{"world":point}})).unwrap();
            }
        }
        assert!(!h.app.status.as_ref().is_some_and(|s| s.2), "{command}: {:?}", h.app.status);
        let state = h.app.session.world_state().unwrap();
        assert!(!state.sketches[0].sketch.curves.is_empty(), "{command}: {:?}; {:?}", h.app.tool, h.app.status);
        assert_eq!(h.app.session.undo.len(), undo + 1, "{command}");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before, "{command}");
    }
}

/// The square chord on a rectangle's second corner: a corner the square moves loses its snap, a
/// corner that is already square to the first point keeps the place and the snap it was clicked
/// with.
#[test]
fn a_squared_corner_keeps_its_snap_only_where_it_was_clicked() {
    use solidcore_record::Vec2;
    for command in ["sketch.rectangle.two_point", "sketch.rectangle.center"] {
        let mut tool = super::Tool::for_command(command).unwrap();
        tool.pts.push((Vec2::new(20.0, 0.0), None));
        let snap = Some("on:side:50,30".to_string());
        crate::tool_modifiers::set_square_rectangle(true);
        let kept = super::squared(&tool, (Vec2::new(50.0, 30.0), snap.clone()));
        let moved = super::squared(&tool, (Vec2::new(50.0, 25.0), Some("on:side:50,25".to_string())));
        crate::tool_modifiers::set_square_rectangle(false);
        let free = super::squared(&tool, (Vec2::new(50.0, 25.0), snap.clone()));
        assert_eq!(kept, (Vec2::new(50.0, 30.0), snap.clone()), "{command}: already square");
        assert_eq!(moved, (Vec2::new(50.0, 30.0), None), "{command}: moved to the square");
        assert_eq!(free, (Vec2::new(50.0, 25.0), snap), "{command}: without the chord");
    }
}
