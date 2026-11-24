//! A rectangular pattern's second direction has its own drag arrow for Spacing 2 (#2462), as
//! the first direction's arrow sets Spacing.

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at};
use crate::sketch_modify_ui::Mod;
use serde_json::json;
use solidcraft_engine::geom::prelude::*;

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
    // Keep both arrows in the exposed canvas to the left of the floating editor.
    h.call("ui.camera", json!({"target":[15,0,0]}));
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

#[test]
fn a_pattern_of_features_puts_its_arrow_on_the_patterned_feature() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 60, "width": 20, "height": 5, "name": "Plate"}}},
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"corner": [8, 6, 5], "length": 8, "width": 8, "height": 6, "operation": "join", "name": "Boss"}}},
        {"call": "engine.execute", "params": {"command": "solid.pattern.rectangular", "params": {"features": ["Boss"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 20, "name": "Pattern"}}},
        {"call": "ui.view", "params": {"view": "iso"}},
        {"call": "ui.editFeature", "params": {"feature": "Pattern"}}
    ]))
    .unwrap();
    h.settle();
    let d = h.app.dialog.as_ref().expect("the pattern dialog");
    assert!(matches!(d.inputs.first().and_then(|i| i.items.first()), Some(solidcraft_engine::Sel::Feature { .. })), "{:?}", d.inputs);
    // The arrow starts at the middle of the boss and points along X, the pattern's direction.
    let (base, dir) = crate::canvas::anchor(&h.app, d).expect("the pattern has an anchor");
    assert!(base.dist(solidcore_record::Vec3::new(12.0, 10.0, 8.0)) < 1e-6, "{base:?}");
    assert!(dir.is_some_and(|v| v.dist(solidcore_record::Vec3::new(1.0, 0.0, 0.0)) < 1e-9), "{dir:?}");
    let Some(tip) = handle_at("arrow") else { panic!("no arrow on a pattern of features") };
    let from = h.call("ui.worldToScreen", json!({"points": [[12, 10, 8], [32, 10, 8]]}))["result"].clone();
    let at = |i: usize| egui::pos2(from[i][0].as_f64().unwrap() as f32, from[i][1].as_f64().unwrap() as f32);
    let (b, x) = (at(0), at(1));
    // The tip lies off the base, on the screen line of the boss's +X direction.
    let (ux, uy) = (x.x - b.x, x.y - b.y);
    let (vx, vy) = (tip.x - b.x, tip.y - b.y);
    assert!(vx * ux + vy * uy > 0.0, "tip {tip:?} base {b:?}");
    assert!((vx * uy - vy * ux).abs() / (ux * ux + uy * uy).sqrt() < 3.0, "tip {tip:?} off the X line through {b:?}");
}

#[test]
fn inch_pattern_arrows_follow_bare_primary_and_secondary_distances() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.run(&json!([
        {"call":"ui.view","params":{"view":"iso"}},
        {"start":"solid.pattern.rectangular"},
        {"click":{"world":[5,5,10]}},
        {"call":"ui.dialogInput","params":{"index":1}}, {"click":{"axis":"X"}},
        {"call":"ui.dialogInput","params":{"index":2}}, {"click":{"axis":"Y"}}
    ]))
    .unwrap();
    if let Kind::PatternRect { spacing, spacing2, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *spacing = "0.5".into();
        *spacing2 = "0.25".into();
    }
    h.settle();
    let bare = (handle_at("arrow").unwrap(), handle_at("arrow2").unwrap());
    if let Kind::PatternRect { spacing, spacing2, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *spacing = "12.7 mm".into();
        *spacing2 = "6.35 mm".into();
    }
    h.settle();
    let explicit = (handle_at("arrow").unwrap(), handle_at("arrow2").unwrap());
    assert!(bare.0.distance(explicit.0) < 0.01, "primary: {bare:?} vs {explicit:?}");
    assert!(bare.1.distance(explicit.1) < 0.01, "secondary: {bare:?} vs {explicit:?}");
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
    let st = h.app.session.world_state().unwrap();
    for (copy, k) in [("Body1 (2)", 1.0), ("Body1 (3)", 2.0)] {
        let x = st.body(copy).unwrap_or_else(|| panic!("no {copy}: {:?}", h.app.status)).mesh().bounds().center().x;
        assert!((x - (5.0 + k * v)).abs() < 1e-6, "{copy} centred at x = {x} for Spacing {spacing}");
    }
}

/// Quantity and Extent, centred and flipped (#8523): the arrow runs the way the copies go
/// furthest and shows that side's share of the extent (Quantity 4 over 30 mm: two 10 mm steps
/// ahead, one behind); Spacing and Extent adds its own Extent arrow beside Spacing's.
#[test]
fn a_centred_extent_arrow_shows_its_own_side_and_turns_with_flip() {
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
    h.call("ui.camera", json!({"target": [5, 5, 5]}));
    {
        let d = h.app.dialog.as_mut().unwrap();
        for (key, value) in [("distance1", json!("count_extent")), ("alignment", json!("center")), ("reversed", json!(true))] {
            d.extra.insert(key.into(), value);
        }
        if let Kind::PatternRect { count, extent, .. } = &mut d.kind {
            *count = "4".into();
            *extent = "30 mm".into();
        }
    }
    h.settle();
    let (base, dir) = crate::canvas::anchor(&h.app, h.app.dialog.as_ref().unwrap()).expect("an anchor");
    assert!(dir.is_some_and(|v| v.dist(solidcore_record::Vec3::new(-1.0, 0.0, 0.0)) < 1e-9), "Flip turns the arrow round: {dir:?}");
    let Some(tip) = handle_at("arrow") else { panic!("no extent arrow") };
    let end = h.call("ui.worldToScreen", json!({"points": [[base.x - 20.0, base.y, base.z]]}))["result"][0].clone();
    let end = egui::pos2(end[0].as_f64().unwrap() as f32, end[1].as_f64().unwrap() as f32);
    assert!(tip.distance(end) < 2.0, "the arrow ends 20 mm along -X: tip {tip:?}, expected {end:?}");
    assert!(handle_at("arrow_extent").is_none(), "Quantity and Extent has one arrow");
    h.app.dialog.as_mut().unwrap().extra.insert("distance1".into(), json!("spacing_extent"));
    h.settle();
    assert!(handle_at("arrow").is_some() && handle_at("arrow_extent").is_some(), "Spacing and Extent shows both arrows");
}

/// A reopened pattern whose first direction is no origin axis keeps its arrow (#8523): the
/// window keeps that direction in `dir1` alone, as it keeps a slanted second one in `dir2`.
#[test]
fn a_reopened_pattern_along_a_slanted_first_direction_keeps_its_arrow() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"corner": [0, 0, 0], "length": 60, "width": 40, "height": 5, "name": "Plate"}}},
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"corner": [8, 6, 5], "length": 8, "width": 8, "height": 6, "operation": "join", "name": "Boss"}}},
        {"call": "engine.execute", "params": {"command": "solid.pattern.rectangular", "params": {"features": ["Boss"], "dir1": [0.6, 0.8, 0], "count1": 2, "spacing1": 20, "name": "Pattern"}}},
        {"call": "ui.view", "params": {"view": "iso"}},
        {"call": "ui.editFeature", "params": {"feature": "Pattern"}}
    ]))
    .unwrap();
    h.settle();
    let d = h.app.dialog.as_ref().expect("the pattern dialog");
    assert!(d.inputs.get(1).is_none_or(|i| i.items.is_empty()), "a slanted direction is no pick: {:?}", d.inputs);
    let (_, dir) = crate::canvas::anchor(&h.app, d).expect("an anchor");
    assert!(dir.is_some_and(|v| (v.x - 0.6).abs() < 1e-9 && (v.y - 0.8).abs() < 1e-9 && v.z.abs() < 1e-9), "{dir:?}");
    assert!(handle_at("arrow").is_some(), "the Spacing arrow along the kept direction");
}

/// Spacing 2's and Extent 2's boxes in the view stay while their text does not evaluate, so a
/// cleared box can be typed on in (#8523), as Distance 2's does for an extrude.
#[test]
fn a_solid_pattern_second_direction_boxes_stay_while_their_text_is_incomplete() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}},
        {"call": "ui.view", "params": {"view": "iso"}},
        {"start": "solid.pattern.rectangular"},
        {"click": {"world": [5, 5, 10]}},
        {"call": "ui.dialogInput", "params": {"index": 1}},
        {"click": {"axis": "X"}},
        {"call": "ui.dialogInput", "params": {"index": 2}},
        {"click": {"axis": "Y"}}
    ]))
    .unwrap();
    h.app.dialog.as_mut().unwrap().extra.insert("distance2".into(), json!("spacing_extent"));
    h.settle();
    let shown = |h: &Harness, what: &str| h.ctx.read_response(egui::Id::unique(what)).is_some();
    assert!(shown(&h, "sc_canvas_value_spacing2") && shown(&h, "sc_canvas_value_extent2"), "both boxes with valid text");
    for text in ["", "-"] {
        if let Some(Kind::PatternRect { spacing2, extent2, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *spacing2 = text.into();
            *extent2 = text.into();
        }
        h.settle();
        assert!(shown(&h, "sc_canvas_value_spacing2"), "Spacing 2's box went away at {text:?}");
        assert!(shown(&h, "sc_canvas_value_extent2"), "Extent 2's box went away at {text:?}");
    }
}

/// Each floating dimension clears all four actual arrows, even those drawn later in the frame.
#[test]
fn floating_pattern_fields_clear_the_primary_and_secondary_spacing_and_extent_grips() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    h.call("ui.prefs", json!({"language":"en","look_at_anim":false}));
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"solid.box","params":{"length":10,"width":10,"height":10}}},
        {"call":"ui.view","params":{"view":"iso"}},
        {"start":"solid.pattern.rectangular"},
        {"click":{"world":[5,5,10]}},
        {"call":"ui.dialogInput","params":{"index":1}},{"click":{"axis":"X"}},
        {"call":"ui.dialogInput","params":{"index":2}},{"click":{"axis":"Y"}}
    ])).unwrap();
    let d = h.app.dialog.as_mut().unwrap();
    d.extra.insert("distance1".into(), json!("spacing_extent"));
    d.extra.insert("distance2".into(), json!("spacing_extent"));
    if let Kind::PatternRect { count, spacing, extent, count2, spacing2, extent2, .. } = &mut d.kind {
        (*count, *spacing, *extent, *count2, *spacing2, *extent2) = ("4".into(),"10 mm".into(),"30 mm".into(),"4".into(),"8 mm".into(),"24 mm".into());
    }
    h.call("ui.camera", json!({"target":[15,15,5],"distance":100}));
    h.settle();
    let tip = handle_at("arrow2").expect("second spacing grip");
    let window = h.app.viewport.dialog_rect.unwrap();
    let title = handle_at("dialog:title").unwrap();
    let delta = egui::vec2(tip.x + 30.0 - window.left(), tip.y - 80.0 - window.top());
    h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x+delta.x,"y1":title.y+delta.y,"steps":8}));
    h.frames(24);
    let tip = handle_at("arrow2").unwrap();
    let window = h.app.viewport.dialog_rect.unwrap();
    assert!((window.left()-tip.x-30.0).abs() < 3.0 && window.y_range().contains(tip.y), "window beside the actual secondary grip: {window:?}, {tip:?}");
    let grips: Vec<_> = ["arrow", "arrow_extent", "arrow2", "arrow2_extent"].into_iter().map(|name| {
        (name, egui::Rect::from_center_size(handle_at(name).unwrap_or_else(|| panic!("no {name}")),egui::vec2(20.0,20.0)))
    }).collect();
    for id in ["sc_canvas_area", "sc_canvas_area_extent", "sc_canvas_area_spacing2", "sc_canvas_area_extent2"] {
        let field = h.ctx.memory(|m| m.area_rect(egui::Id::unique(id))).unwrap_or_else(|| panic!("no {id}"));
        for (name, grip) in &grips {
            assert!(!field.intersects(*grip), "{id} at {field:?} covers {name} at {grip:?}");
        }
    }
    // Enter text through the actual secondary response and check the other dimension is intact.
    let input = h.ctx.read_response(egui::Id::unique("sc_canvas_value_spacing2")).unwrap().rect;
    h.step(&json!({"click":[input.center().x,input.center().y]})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"9 mm"})).unwrap();
    h.frames(3);
    assert_eq!(spacings(&h), ("10 mm".into(),"9 mm".into()));
}
