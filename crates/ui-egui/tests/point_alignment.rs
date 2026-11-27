//! Horizontal/Vertical accepts either one line or two distinct sketch points.

#[path = "support/raster.rs"]
mod raster;

use serde_json::json;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::ConstraintKind;
use solidcraft_ui_egui::scenario::Harness;

fn line_midpoint(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> solidcore_record::Vec2 {
    let a = sk.point(sk.resolve_point(&format!("{id}.start")).unwrap()).unwrap();
    let b = sk.point(sk.resolve_point(&format!("{id}.end")).unwrap()).unwrap();
    (a + b) * 0.5
}

#[test]
fn horizontal_vertical_uses_preselected_line_and_preserves_rejection() {
    for fixed in [false, true] {
        let mut h = Harness::new();
        h.run(&json!([
            {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
            {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[10,10],[40,13]],"infer":false}}},
            {"call":"ui.view","params":{"view":"top"}}
        ]))
        .unwrap();
        if fixed {
            h.app.session.execute("sketch.constraint.fix", &json!({"entity":"l1"})).unwrap();
        }
        h.run(&json!([{"click":{"sketch":[17.5,10.75]}}])).unwrap();
        assert_eq!(h.app.session.selection.len(), 1);
        let selection = h.app.session.selection.clone();
        let before = serde_json::to_value(&h.app.session.doc).unwrap();
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":"sketch.constraint.horizontal_vertical"}])).unwrap();
        assert_eq!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error), fixed);
        if fixed {
            assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
            assert_eq!(h.app.session.selection, selection);
            assert_eq!(h.app.session.undo.len(), undo);
        } else {
            let sk = &h.app.session.model.state().sketches[0].sketch;
            let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
            let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
            assert!((a.y - b.y).abs() < 1e-6);
            assert_eq!(h.app.session.undo.len(), undo + 1);
            assert!(h.app.session.selection.is_empty());
        }
        assert!(h.app.tool.is_none());
    }
}

#[test]
fn horizontal_vertical_leaves_ambiguous_preselection_to_pick_tool() {
    use solidcraft_engine::Sel;
    // A single preselected point seeds H/V point_picks so one more click aligns them.
    {
        let mut h = Harness::new();
        h.app.session.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        h.app.session.execute("sketch.circle.center", &json!({"center":[20,3],"radius":5})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.settle();
        h.app.session.selection = vec![Sel::SketchPoint { id: "origin".into() }];
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":"sketch.constraint.horizontal_vertical"}])).unwrap();
        assert_eq!(h.app.tool.as_ref().unwrap().point_picks, vec![json!("origin")]);
        assert!(h.app.session.selection.is_empty());
        assert_eq!(h.app.session.undo.len(), undo);
        h.run(&json!([{"click":{"sketch":[20,3]}}])).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(h.app.tool.is_none());
    }
    for selection in [
        vec![Sel::SketchPoint { id: "origin".into() }, Sel::SketchCurve { id: "c1".into(), sketch: None }],
        vec![
            Sel::SketchPoint { id: "origin".into() },
            Sel::SketchPoint { id: "c1.center".into() },
            Sel::SketchCurve { id: "c1".into(), sketch: None },
        ],
    ] {
        let mut h = Harness::new();
        h.app.session.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        h.app.session.execute("sketch.circle.center", &json!({"center":[20,3],"radius":5})).unwrap();
        h.app.session.selection = selection;
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":"sketch.constraint.horizontal_vertical"}])).unwrap();
        assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.session.selection.is_empty());
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    }
}

#[test]
fn horizontal_vertical_uses_preselected_points() {
    for center in [[20.0, 3.0], [3.0, 20.0]] {
        for reverse in [false, true] {
            let mut h = Harness::new();
            let (first, second) = if reverse { (center, [0.0, 0.0]) } else { ([0.0, 0.0], center) };
            h.run(&json!([
                {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
                {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":center,"radius":5}}},
                {"call":"ui.view","params":{"view":"top"}},
                {"click":{"sketch":first}},
                {"click":{"sketch":second},"shift":true}
            ]))
            .unwrap();
            assert_eq!(h.app.session.selection.len(), 2);
            let before = serde_json::to_value(&h.app.session.doc).unwrap();
            let undo = h.app.session.undo.len();
            h.run(&json!([{"start":"sketch.constraint.horizontal_vertical"}])).unwrap();
            assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
            let sk = &h.app.session.model.state().sketches[0].sketch;
            let p = sk.point(sk.resolve_point("c1.center").unwrap()).unwrap();
            assert!((if center[0] > center[1] { p.y } else { p.x }).abs() < 1e-6);
            assert!(h.app.tool.is_none());
            assert!(h.app.session.selection.is_empty());
            assert_eq!(h.app.session.undo.len(), undo + 1);
            let after = serde_json::to_value(&h.app.session.doc).unwrap();
            h.app.session.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
            h.app.session.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), after);
        }
    }
}

#[test]
fn horizontal_vertical_aligns_origin_and_circle_center() {
    for (center, horizontal) in [([20.0, 3.0], true), ([3.0, 20.0], false)] {
        for origin_first in [true, false] {
            let mut h = Harness::new();
            h.run(&json!([
                {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
                {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":center,"radius":5}}},
                {"call":"ui.view","params":{"view":"top"}},
                {"start":"sketch.constraint.horizontal_vertical"}
            ]))
            .unwrap();
            let (first, second) = if origin_first { ([0.0, 0.0], center) } else { (center, [0.0, 0.0]) };
            let undo = h.app.session.undo.len();
            h.run(&json!([{"click":{"sketch":first}}, {"expect":{"errors":0}}])).unwrap();
            assert_eq!(h.app.tool.as_ref().unwrap().point_picks.len(), 1);
            assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
            assert_eq!(h.app.session.undo.len(), undo);
            // Repeating the first point must not complete a self-constraint.
            h.run(&json!([{"click":{"sketch":first}}, {"click":{"sketch":second}}, {"expect":{"errors":0}}])).unwrap();
            let aligned = |h: &Harness| {
                let sk = &h.app.session.model.state().sketches[0].sketch;
                let p = sk.point(sk.resolve_point("c1.center").unwrap()).unwrap();
                if horizontal { p.y } else { p.x }
            };
            assert!(aligned(&h).abs() < 1e-6);
            let sk = &h.app.session.model.state().sketches[0].sketch;
            assert!(sk.constraints.iter().any(|c| if horizontal {
                matches!(c.kind, ConstraintKind::HorizontalPoints { .. })
            } else {
                matches!(c.kind, ConstraintKind::VerticalPoints { .. })
            }));
            assert_eq!(h.app.session.undo.len(), undo + 1);
            assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
            h.run(&json!([{"call":"engine.execute","params":{"command":"edit.undo","params":{}}}])).unwrap();
            assert!((aligned(&h) - 3.0).abs() < 1e-6);
            h.run(&json!([{"call":"engine.execute","params":{"command":"edit.redo","params":{}}}])).unwrap();
            assert!(aligned(&h).abs() < 1e-6);
        }
    }
}

#[test]
fn horizontal_vertical_still_accepts_one_line() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
        {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[10,10],[40,13]],"infer":false}}},
        {"call":"ui.view","params":{"view":"top"}},
        {"start":"sketch.constraint.horizontal_vertical"},
        {"click":{"sketch":[25,11.5]}},
        {"expect":{"errors":0}}
    ]))
    .unwrap();
    let sk = &h.app.session.model.state().sketches[0].sketch;
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Horizontal { .. })));
    let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
    let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
    assert!((a.y - b.y).abs() < 1e-6);
}

#[test]
fn horizontal_vertical_ignores_curves_and_cancels_a_pending_point() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
        {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[20,3],"radius":5}}},
        {"call":"ui.view","params":{"view":"top"}},
        {"start":"sketch.constraint.horizontal_vertical"},
        {"click":{"sketch":[25,3]}}
    ]))
    .unwrap();
    assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    let undo = h.app.session.undo.len();
    h.run(&json!([{"click":{"sketch":[0,0]}}, {"click":{"sketch":[25,3]}}])).unwrap();
    assert_eq!(h.app.tool.as_ref().unwrap().point_picks.len(), 1);
    h.run(&json!([{"key":"Escape"}])).unwrap();
    assert!(h.app.tool.is_none());
    assert_eq!(h.app.session.undo.len(), undo);
    let sk = &h.app.session.model.state().sketches[0].sketch;
    let p = sk.point(sk.resolve_point("c1.center").unwrap()).unwrap();
    assert!((p.y - 3.0).abs() < 1e-6);
}

/// #20442: the Horizontal/Vertical tool takes a line's middle as the second pick, the way it
/// takes the line's ends: the circle's centre is aligned with that middle on the closer axis,
/// instead of snapping onto the line.
#[test]
fn horizontal_vertical_takes_a_line_middle_without_shift() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[20,16],[20,44]],"infer":false,"connect":false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[34,32],"radius":7})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.app.fit_view();
    h.frames(3);
    h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
    // The circle's centre, then the middle of the vertical line, with no Shift held.
    h.step(&json!({"click":{"sketch":[34,32]}})).unwrap();
    assert_eq!(h.app.tool.as_ref().unwrap().point_picks.len(), 1);
    h.step(&json!({"click":{"sketch":[20,30]}})).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    let sk = &h.app.session.model.state().sketches[0].sketch;
    let p = sk.point(sk.resolve_point("c1.center").unwrap()).unwrap();
    let mid = line_midpoint(sk, "l1");
    // Level with the middle (the closer axis): the centre does not land on the line.
    assert!((p.y - mid.y).abs() < 1e-6, "centre: {p:?}");
    assert!((p.x - mid.x).abs() > 1.0, "centre: {p:?}");
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::HorizontalPoints { .. })));
}

/// Review pictures of #20442: the circle's centre and the middle of a vertical line, with the
/// Horizontal/Vertical tool taking that middle as the point it aligns.
#[test]
#[ignore = "writes review images; run explicitly for visual verification"]
fn horizontal_vertical_line_middle_review_images() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/point-alignment-review");
    std::fs::create_dir_all(&dir).unwrap();
    solidcraft_engine::capture::install(std::sync::Arc::new(|scene, cam, w, h| {
        let (rgba, renderer) = solidcraft_gpu::env::headless_rgba(scene, cam, w, h)?;
        Ok(solidcraft_engine::capture::Picture { rgba, renderer })
    }));
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1100,"height":800}));
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[20,16],[20,44]],"infer":false,"connect":false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[34,32],"radius":7})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.app.fit_view();
    h.frames(3);
    h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
    // The circle's centre, then the middle of the vertical line.
    h.step(&json!({"click":{"sketch":[34,32]}})).unwrap();
    h.step(&json!({"click":{"sketch":[20,30]}})).unwrap();
    h.frames(3);
    let mut textures = std::collections::HashMap::new();
    for _ in 0..8 {
        raster::paint(&mut h, &mut textures, vec![]);
    }
    let output = raster::paint(&mut h, &mut textures, vec![]);
    raster::raster(&mut h, &textures, output).save(dir.join("line-middle.png")).unwrap();
    std::fs::write(dir.join("line-middle-model.png"), solidcraft_ui_egui::control::view_png(&h.app, 1000, 700).unwrap()).unwrap();
}

#[test]
fn shift_picks_midpoint_in_either_order_and_persists_ordinary_constraints() {
    for (target, horizontal) in [([50., 18.], true), ([23., 50.], false)] {
        for midpoint_first in [true, false] {
            let mut h = Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false})).unwrap();
            h.app.run("sketch.circle.center", json!({"center":target,"radius":3})).unwrap();
            h.call("ui.view", json!({"view":"top"}));
            // Keep both picks in the clear viewport beside the floating cards.
            h.app.fit_view();
            h.frames(3);
            h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            h.step(&json!({"move":{"sketch":[20,15]},"shift":true})).unwrap();
            assert_eq!(h.app.tool.as_ref().unwrap().hover.as_ref().unwrap().1.as_deref(), Some("Midpoint"));
            let midpoint = json!({"click":{"sketch":[20,15]},"shift":true});
            let target = json!({"click":{"sketch":target}});
            let (first, second) = if midpoint_first { (&midpoint, &target) } else { (&target, &midpoint) };
            h.step(first).unwrap();
            h.step(first).unwrap();
            assert_eq!(h.app.tool.as_ref().unwrap().point_picks.len(), 1);
            assert_eq!(h.app.session.undo.len(), undo);
            h.step(second).unwrap();
            assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
            assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
            assert_eq!(h.app.session.undo.len(), undo + 1);
            let sk = &h.app.session.model.state().sketches[0].sketch;
            assert_eq!(sk.points.len(), 5);
            assert_eq!(sk.curves.len(), 2);
            let p = sk.point(sk.resolve_point("c1.center").unwrap()).unwrap();
            let mid = line_midpoint(sk, "l1");
            assert!((if horizontal { p.y - mid.y } else { p.x - mid.x }).abs() < 1e-6);
            assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })));
            let constraint = sk
                .constraints
                .iter()
                .find(|c| matches!(c.kind, ConstraintKind::HorizontalPoints { .. } | ConstraintKind::VerticalPoints { .. }))
                .unwrap()
                .id
                .clone();
            h.step(&json!({"expect":{"glyphs":{constraint.clone():1}}})).unwrap();
            let after = h.app.session.doc.to_json();
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), before);
            h.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), after);
            // The ordinary constraint glyph remains selectable and deletable.
            h.step(&json!({"key":"Escape"})).unwrap();
            let glyph = solidcraft_ui_egui::sketch_tools::glyph_positions().into_iter().find(|(id, _)| id == &constraint).unwrap().1;
            h.step(&json!({"click":[glyph.x,glyph.y]})).unwrap();
            h.step(&json!({"key":"Delete"})).unwrap();
            let sk = &h.app.session.model.state().sketches[0].sketch;
            assert!(!sk.constraints.iter().any(|c| c.id == constraint));
            assert_eq!(sk.points.len(), 5);
            let midpoint_constraint = sk.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).unwrap().id.clone();
            // The defining Midpoint relationship is editable through the same glyph UI.
            let glyph = solidcraft_ui_egui::sketch_tools::glyph_positions().into_iter().find(|(id, _)| id == &midpoint_constraint).unwrap().1;
            h.step(&json!({"click":[glyph.x,glyph.y]})).unwrap();
            h.step(&json!({"key":"Delete"})).unwrap();
            let sk = &h.app.session.model.state().sketches[0].sketch;
            assert!(!sk.constraints.iter().any(|c| c.id == midpoint_constraint));
            assert_eq!(sk.points.len(), 5);
        }
    }
}

#[test]
fn shift_midpoint_pick_cancels_and_ignores_round_curves() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[40,40],"radius":3})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
    let before = h.app.session.doc.to_json();
    h.step(&json!({"click":{"sketch":[43,40]},"shift":true})).unwrap();
    assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
    h.step(&json!({"click":{"sketch":[20,15]},"shift":true})).unwrap();
    assert_eq!(h.app.tool.as_ref().unwrap().point_picks, vec![json!({"midpoint":"l1"})]);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.tool.is_none());
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn shift_can_align_two_midpoints_and_reveal_one_from_anywhere_on_the_line() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for points in [json!([[10, 10], [30, 20]]), json!([[15, 40], [35, 50]])] {
        h.app.run("sketch.line", json!({"points":points,"infer":false,"connect":false})).unwrap();
    }
    h.call("ui.view", json!({"view":"top"}));
    // Keep both picks in the clear viewport beside the floating cards.
    h.app.fit_view();
    h.frames(3);
    h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
    h.step(&json!({"move":{"sketch":[13,11.5]},"shift":true})).unwrap();
    let hover = h.app.tool.as_ref().unwrap().hover.as_ref().unwrap();
    assert_eq!([hover.0.x, hover.0.y], [20., 15.]);
    assert_eq!(hover.1.as_deref(), Some("Midpoint"));
    h.step(&json!({"click":{"sketch":[20,15]},"shift":true})).unwrap();
    h.step(&json!({"click":{"sketch":[25,45]},"shift":true})).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    let sk = &h.app.session.model.state().sketches[0].sketch;
    assert_eq!(sk.points.len(), 7);
    assert_eq!(sk.curves.len(), 2);
    let mid = |id| line_midpoint(sk, id);
    assert!((mid("l1").x - mid("l2").x).abs() < 1e-6);
}

#[test]
fn midpoint_aligns_to_the_fixed_origin_without_straightening_the_line() {
    for midpoint_first in [true, false] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[-10,15],[16,25]],"infer":false,"connect":false})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
        let midpoint = json!({"click":{"sketch":[3,20]},"shift":true});
        let origin = json!({"click":{"sketch":[0,0]}});
        let (first, second) = if midpoint_first { (&midpoint, &origin) } else { (&origin, &midpoint) };
        h.step(first).unwrap();
        h.step(second).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
        let sk = &h.app.session.model.state().sketches[0].sketch;
        assert_eq!(sk.points.len(), 4);
        let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
        let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
        assert!(((a + b) * 0.5).x.abs() < 1e-6);
        assert!((b.x - a.x).abs() > 1. && (b.y - a.y).abs() > 1., "only the midpoint is aligned");
        assert_eq!(sk.point(0).unwrap(), solidcore_record::Vec2::ZERO);
    }
}

#[test]
fn conflicting_midpoint_pick_keeps_pending_preview_without_creating_geometry() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,20]],"infer":false,"connect":false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[24,50],"radius":3})).unwrap();
    h.app.run("sketch.constraint.fix", json!({"entities":["l1","c1"]})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    // Keep both picks in the clear viewport beside the floating cards.
    h.app.fit_view();
    h.frames(3);
    h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"sketch":[20,15]},"shift":true})).unwrap();
    h.step(&json!({"click":{"sketch":[24,50]}})).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.tool.as_ref().unwrap().point_picks, vec![json!({"midpoint":"l1"})]);
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.tool.is_none());
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn context_menu_explicitly_aligns_two_points_and_removes_one_freedom() {
    use solidcraft_engine::Sel;
    use solidcraft_ui_egui::context_menu::{self, Target};
    // Include independent points, a circle centre and line endpoints; choose the direction
    // opposite to the automatic H/V guess, in either selection order.
    for (first, second) in [("a", "b"), ("l1.start", "c1.center"), ("l1.start", "l1.end")] {
        for horizontal in [true, false] {
            for reverse in [false, true] {
                let mut h = Harness::new();
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.app.run("sketch.point", json!({"point":[10,10],"id":"a"})).unwrap();
                h.app.run("sketch.point", json!({"point":[25,30],"id":"b"})).unwrap();
                h.app.run("sketch.line", json!({"points":[[15,12],[35,40]],"infer":false,"connect":false})).unwrap();
                h.app.run("sketch.circle.center", json!({"center":[45,50],"radius":3})).unwrap();
                h.call("ui.view", json!({"view":"top"}));
                let pair = if reverse { [second, first] } else { [first, second] };
                h.app.session.selection = pair.into_iter().map(|id| Sel::SketchPoint { id: id.into() }).collect();
                let before = h.app.session.doc.to_json();
                let undo = h.app.session.undo.len();
                let dof = h.app.session.model.state().sketches[0].report.dof;
                let items = context_menu::items(&h.app, &Target::Viewport);
                let horizontal_item = items.iter().find(|i| i.id == "ui.horizontalConstraint").unwrap();
                let vertical_item = items.iter().find(|i| i.id == "ui.verticalConstraint").unwrap();
                assert_eq!(horizontal_item.label, "Horizontal Constraint");
                assert_eq!(vertical_item.label, "Vertical Constraint");
                let item = if horizontal { horizontal_item } else { vertical_item };
                assert!(item.enabled);
                context_menu::open_for(&mut h.app, egui::pos2(600.0, 400.0), Target::Viewport);
                context_menu::pick(&mut h.app, &item.id).unwrap();
                assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
                let state = h.app.session.model.state();
                let ss = &state.sketches[0];
                let a = ss.sketch.point(ss.sketch.resolve_point(first).unwrap()).unwrap();
                let b = ss.sketch.point(ss.sketch.resolve_point(second).unwrap()).unwrap();
                assert!((if horizontal { a.y - b.y } else { a.x - b.x }).abs() < 1e-6);
                assert_eq!(ss.report.dof, dof - 1);
                assert_eq!(h.app.session.undo.len(), undo + 1);
                let constraint = ss
                    .sketch
                    .constraints
                    .iter()
                    .find(|c| {
                        if horizontal {
                            matches!(c.kind, ConstraintKind::HorizontalPoints { .. })
                        } else {
                            matches!(c.kind, ConstraintKind::VerticalPoints { .. })
                        }
                    })
                    .unwrap()
                    .id
                    .clone();
                h.frames(3);
                assert!(solidcraft_ui_egui::sketch_tools::glyph_positions().iter().any(|(id, _)| id == &constraint));
                let after = h.app.session.doc.to_json();
                h.app.run("edit.undo", json!({})).unwrap();
                assert_eq!(h.app.session.doc.to_json(), before);
                h.app.run("edit.redo", json!({})).unwrap();
                assert_eq!(h.app.session.doc.to_json(), after);
                // Repeating the same relationship must not stack a redundant constraint.
                let undo = h.app.session.undo.len();
                context_menu::run_item(&mut h.app, item, egui::Pos2::ZERO);
                assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
                assert_eq!(h.app.session.doc.to_json(), after);
                assert_eq!(h.app.session.undo.len(), undo);
            }
        }
    }
}

#[test]
fn context_menu_alignment_requires_exactly_two_points_and_preserves_conflicts() {
    use solidcraft_engine::Sel;
    use solidcraft_ui_egui::context_menu::{self, Target};
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[20,30],"radius":3})).unwrap();
    let origin = Sel::SketchPoint { id: "origin".into() };
    let center = Sel::SketchPoint { id: "c1.center".into() };
    let curve = Sel::SketchCurve { id: "c1".into(), sketch: None };
    for selection in [vec![], vec![origin.clone()], vec![origin.clone(), curve.clone()], vec![origin.clone(), center.clone(), curve]] {
        h.app.session.selection = selection;
        assert!(
            !context_menu::items(&h.app, &Target::Viewport)
                .iter()
                .any(|i| matches!(i.id.as_str(), "ui.horizontalConstraint" | "ui.verticalConstraint"))
        );
    }
    h.app.run("sketch.constraint.fix", json!({"entity":"c1"})).unwrap();
    h.app.session.selection = vec![origin, center];
    for id in ["ui.horizontalConstraint", "ui.verticalConstraint"] {
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let selection = h.app.session.selection.clone();
        let item = context_menu::items(&h.app, &Target::Viewport).into_iter().find(|i| i.id == id).unwrap();
        context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
        assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.selection, selection);
    }
}

/// Reproducible review images of the actual menu and glyphs, using the app's egui meshes.
#[test]
#[ignore = "writes review images; run explicitly for visual verification"]
fn context_menu_alignment_review_images() {
    use solidcraft_engine::Sel;
    use solidcraft_ui_egui::{
        context_menu::{self, Target},
        fonts,
    };
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/point-alignment-review");
    std::fs::create_dir_all(&dir).unwrap();
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.language = "ja".into();
        h.call("ui.resize", json!({"width":1100,"height":1500}));
        h.call("ui.set", json!({"dark":dark}));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        for (id, point) in [("a", [10, 10]), ("b", [35, 20]), ("c", [55, 10]), ("d", [70, 35])] {
            h.app.run("sketch.point", json!({"point":point,"id":id})).unwrap();
        }
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.target = solidcore_record::Vec3::new(40.0, 22.0, 0.0);
        h.app.cam.distance = 180.0;
        h.app.cam_anim = None;
        h.frames(3);
        h.ctx = egui::Context::default();
        fonts::setup_fonts_for(&h.ctx, "ja", "");
        solidcraft_theme::apply(&h.ctx);
        let mut textures = std::collections::HashMap::new();
        for _ in 0..8 {
            raster::paint(&mut h, &mut textures, vec![]);
        }
        let mode = if dark { "dark" } else { "light" };
        for (pair, item, shot) in [(["a", "b"], "ui.horizontalConstraint", "horizontal"), (["c", "d"], "ui.verticalConstraint", "vertical")] {
            h.app.session.selection = pair.into_iter().map(|id| Sel::SketchPoint { id: id.into() }).collect();
            context_menu::open_for(&mut h.app, egui::pos2(800.0, 950.0), Target::Viewport);
            for _ in 0..4 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            raster::raster(&mut h, &textures, output).save(dir.join(format!("{mode}-{shot}-menu.png"))).unwrap();
            context_menu::pick(&mut h.app, item).unwrap();
            for _ in 0..4 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            raster::raster(&mut h, &textures, output).save(dir.join(format!("{mode}-{shot}.png"))).unwrap();
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn context_menu_alignment_follows_screen_axes_in_principal_sketch_views() {
    use solidcraft_engine::Sel;
    use solidcraft_ui_egui::{
        context_menu::{self, Target},
        dialogs, viewport,
    };
    for plane in ["XY", "XZ", "YZ"] {
        for horizontal in [true, false] {
            let mut h = Harness::new();
            h.app.run("sketch.create", json!({"plane":plane})).unwrap();
            h.app.run("sketch.point", json!({"point":[10,10],"id":"a"})).unwrap();
            h.app.run("sketch.point", json!({"point":[30,25],"id":"b"})).unwrap();
            dialogs::look_at_sketch(&mut h.app);
            h.settle();
            h.app.session.selection = ["a", "b"].into_iter().map(|id| Sel::SketchPoint { id: id.into() }).collect();
            let id = if horizontal { "ui.horizontalConstraint" } else { "ui.verticalConstraint" };
            let item = context_menu::items(&h.app, &Target::Viewport).into_iter().find(|i| i.id == id).unwrap();
            context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
            let state = h.app.session.model.state();
            let ss = &state.sketches[0];
            let proj = viewport::projection(&h.app, h.app.viewport.rect.unwrap());
            let a = proj.to_screen(ss.plane.to_world(ss.sketch.point(ss.sketch.resolve_point("a").unwrap()).unwrap())).unwrap();
            let b = proj.to_screen(ss.plane.to_world(ss.sketch.point(ss.sketch.resolve_point("b").unwrap()).unwrap())).unwrap();
            assert!((if horizontal { a.y - b.y } else { a.x - b.x }).abs() < 0.01, "{plane}: {a:?}, {b:?}");
        }
    }
}

#[test]
fn idle_midpoints_are_selectable_without_modifying_the_sketch() {
    use solidcraft_engine::Sel;
    use solidcraft_ui_egui::{
        context_menu::{self, Target},
        viewport::Hit,
    };
    for (plane, view) in [("XY", "top"), ("XZ", "front"), ("YZ", "right")] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":plane})).unwrap();
        h.app.run("sketch.line", json!({"points":[[10,10],[50,20]],"infer":false,"connect":false})).unwrap();
        h.app.run("sketch.line", json!({"points":[[-10,40],[70,50]],"infer":false,"connect":false})).unwrap();
        h.call("ui.view", json!({"view":view}));
        h.app.fit_view();
        h.frames(3);
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.step(&json!({"move":{"sketch":[30,15]}})).unwrap();
        assert!(matches!(h.app.viewport.hover, Some(Hit::SketchMidpoint { ref id, .. }) if id == "l1"));
        h.step(&json!({"click":{"sketch":[30,15]}})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l1".into() }]);
        assert!(!context_menu::items(&h.app, &Target::Viewport).iter().any(|i| i.id == "ui.horizontalConstraint"));
        h.step(&json!({"click":{"sketch":[30,45]},"shift":true})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l1".into() }, Sel::SketchMidpoint { id: "l2".into() }]);
        let items = context_menu::items(&h.app, &Target::Viewport);
        assert_eq!(
            items.iter().find(|i| i.id == "ui.horizontalConstraint").unwrap().params.as_ref().unwrap()["points"],
            json!([{"midpoint":"l1"},{"midpoint":"l2"}])
        );
        h.step(&json!({"click":{"sketch":[30,45]},"shift":true})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchMidpoint { id: "l1".into() }]);
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
        // Ordinary edge and endpoint picks still work away from MID.
        h.step(&json!({"click":{"sketch":[20,12.5]}})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: "l1".into(), sketch: None }]);
        h.step(&json!({"click":{"sketch":[10,10]}})).unwrap();
        assert!(matches!(h.app.session.selection.as_slice(), [Sel::SketchPoint { .. }]));
        h.app.ui.pick_off.push("sketch_points".into());
        h.step(&json!({"move":{"sketch":[30,15]}})).unwrap();
        assert!(!matches!(h.app.viewport.hover, Some(Hit::SketchMidpoint { .. })));
    }
}

#[test]
fn selected_midpoints_can_take_both_axes_and_reuse_the_same_helpers() {
    use solidcraft_engine::{Sel, Session};
    use solidcraft_ui_egui::context_menu::{self, Target};
    for reverse in [false, true] {
        for horizontal_first in [false, true] {
            let mut h = Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.line", json!({"points":[[10,10],[50,20]],"infer":false,"connect":false})).unwrap();
            h.app.run("sketch.line", json!({"points":[[25,35],[65,55]],"infer":false,"connect":false})).unwrap();
            h.app.run("sketch.constraint.fix", json!({"entity":"l1"})).unwrap();
            h.call("ui.view", json!({"view":"top"}));
            h.app.fit_view();
            h.frames(3);
            let (a, b) = if reverse { ([45, 45], [30, 15]) } else { ([30, 15], [45, 45]) };
            h.step(&json!({"click":{"sketch":a}})).unwrap();
            h.step(&json!({"click":{"sketch":b},"shift":true})).unwrap();
            let selected = h.app.session.selection.clone();
            assert_eq!(selected.len(), 2);
            assert!(selected.iter().all(|s| matches!(s, Sel::SketchMidpoint { .. })));
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            let dof = h.app.session.model.state().sketches[0].report.dof;
            let mut after_first = String::new();
            for (i, horizontal) in [horizontal_first, !horizontal_first].into_iter().enumerate() {
                context_menu::open_for(&mut h.app, egui::pos2(600.0, 400.0), Target::Viewport);
                context_menu::pick(&mut h.app, if horizontal { "ui.horizontalConstraint" } else { "ui.verticalConstraint" }).unwrap();
                assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
                assert_eq!(h.app.session.selection, selected, "both MID selections remain for the second axis");
                let state = h.app.session.model.state();
                let ss = &state.sketches[0];
                let (m, n) = (line_midpoint(&ss.sketch, "l1"), line_midpoint(&ss.sketch, "l2"));
                assert!((if horizontal { m.y - n.y } else { m.x - n.x }).abs() < 1e-6);
                assert_eq!(ss.report.dof, dof - i - 1);
                assert_eq!(ss.sketch.points.len(), 7, "two helper points are reused on the second axis");
                assert_eq!(ss.sketch.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).count(), 2);
                assert_eq!(h.app.session.undo.len(), undo + i + 1);
                if i == 0 {
                    after_first = h.app.session.doc.to_json();
                }
            }
            let after = h.app.session.doc.to_json();
            let sk = &h.app.session.model.state().sketches[0].sketch;
            assert!(line_midpoint(sk, "l1").dist(line_midpoint(sk, "l2")) < 1e-6);
            // The same references must remain pickable after materializing their helper points.
            let mid = line_midpoint(sk, "l2");
            h.step(&json!({"move":{"sketch":[mid.x,mid.y]}})).unwrap();
            assert!(matches!(h.app.viewport.hover, Some(solidcraft_ui_egui::viewport::Hit::SketchMidpoint { .. })));
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), after_first);
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), before);
            h.app.run("edit.redo", json!({})).unwrap();
            h.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.to_json(), after);
            let mut reopened = Session::new(solidcraft_engine::doc::Document::from_json(&after).unwrap());
            reopened.active_sketch = h.app.session.active_sketch;
            reopened.execute("sketch.move_point", &json!({"point":"l2.end","to":[60,25]})).unwrap();
            let sk = &reopened.model.state().sketches[0].sketch;
            assert!(line_midpoint(sk, "l1").dist(line_midpoint(sk, "l2")) < 1e-6);
            let undo = h.app.session.undo.len();
            h.app.run("select.set", json!({"items": selected})).unwrap();
            let item = context_menu::items(&h.app, &Target::Viewport).into_iter().find(|i| i.id == "ui.horizontalConstraint").unwrap();
            context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
            assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
            assert_eq!(h.app.session.doc.to_json(), after);
            assert_eq!(h.app.session.undo.len(), undo);
            assert_eq!(h.app.session.selection, selected);
        }
    }
}

#[test]
fn midpoint_menu_aligns_rectangle_sides_and_refuses_conflicts_atomically() {
    use solidcraft_ui_egui::context_menu;
    for horizontal in [true, false] {
        for fixed in [true, false] {
            let mut h = Harness::new();
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[80,70]})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[15,15],"p1":[50,55]})).unwrap();
            h.app.run("sketch.constraint.fix", json!({"entities":["l1","l2","l3","l4"]})).unwrap();
            if fixed {
                h.app.run("sketch.constraint.fix", json!({"entities":["l5","l6","l7","l8"]})).unwrap();
            }
            h.call("ui.view", json!({"view":"top"}));
            h.app.fit_view();
            h.frames(3);
            let (a, b) = if horizontal { ([40., 70.], [32.5, 55.]) } else { ([40., 0.], [32.5, 15.]) };
            h.step(&json!({"click":{"sketch":a}})).unwrap();
            h.step(&json!({"click":{"sketch":b},"shift":true})).unwrap();
            assert_eq!(h.app.session.selection.len(), 2);
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            let selected = h.app.session.selection.clone();
            let lines: Vec<_> = selected
                .iter()
                .map(|s| if let solidcraft_engine::Sel::SketchMidpoint { id } = s { id.clone() } else { panic!("expected MID: {s:?}") })
                .collect();
            let dof = h.app.session.model.state().sketches[0].report.dof;
            h.step(&json!({"click":{"sketch":if horizontal {[32.5,55.]} else {b}},"button":"right"})).unwrap();
            context_menu::pick(&mut h.app, if horizontal { "ui.horizontalConstraint" } else { "ui.verticalConstraint" }).unwrap();
            if fixed {
                assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
                assert_eq!(h.app.session.doc.to_json(), before);
                assert_eq!(h.app.session.undo.len(), undo);
            } else {
                assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
                let ss = &h.app.session.model.state().sketches[0];
                let (m, n) = (line_midpoint(&ss.sketch, &lines[0]), line_midpoint(&ss.sketch, &lines[1]));
                assert!((if horizontal { m.y - n.y } else { m.x - n.x }).abs() < 1e-6);
                assert_eq!(ss.report.dof, dof - 1);
            }
            assert_eq!(h.app.session.selection, selected);
        }
    }
}

#[test]
#[ignore = "writes MID review images; run explicitly for visual verification"]
fn midpoint_selection_review_images() {
    use solidcraft_ui_egui::{
        context_menu::{self, Target},
        fonts,
    };
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/midpoint-selection-review");
    std::fs::create_dir_all(&dir).unwrap();
    solidcraft_engine::capture::install(std::sync::Arc::new(|scene, cam, w, h| {
        let (rgba, renderer) = solidcraft_gpu::env::headless_rgba(scene, cam, w, h)?;
        Ok(solidcraft_engine::capture::Picture { rgba, renderer })
    }));
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        for horizontal in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.language = "ja".into();
            h.call("ui.resize", json!({"width":1100,"height":1500}));
            h.call("ui.set", json!({"dark":dark}));
            h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[80,70]})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[15,18],"p1":[50,58]})).unwrap();
            h.call("ui.view", json!({"view":"top"}));
            h.app.cam.target = solidcore_record::Vec3::new(40., 35., 0.);
            h.app.cam.distance = 180.;
            h.app.cam_anim = None;
            h.frames(3);
            h.ctx = egui::Context::default();
            fonts::setup_fonts_for(&h.ctx, "ja", "");
            solidcraft_theme::apply(&h.ctx);
            let mut textures = std::collections::HashMap::new();
            for _ in 0..8 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let (a, b) = if horizontal { ([0., 35.], [15., 38.]) } else { ([40., 0.], [32.5, 18.]) };
            h.step(&json!({"move":{"sketch":a}})).unwrap();
            h.step(&json!({"click":{"sketch":a}})).unwrap();
            h.step(&json!({"click":{"sketch":b},"shift":true})).unwrap();
            assert!(h.app.session.selection.iter().all(|s| matches!(s, solidcraft_engine::Sel::SketchMidpoint { .. })));
            assert_eq!(h.app.session.selection.len(), 2);
            let prefix = format!("{}-{}", if dark { "dark" } else { "light" }, if horizontal { "horizontal" } else { "vertical" });
            context_menu::open_for(&mut h.app, egui::pos2(880., 800.), Target::Viewport);
            for _ in 0..4 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            raster::raster(&mut h, &textures, output).save(dir.join(format!("{prefix}-menu.png"))).unwrap();
            std::fs::write(dir.join(format!("{prefix}-menu-model.png")), solidcraft_ui_egui::control::view_png(&h.app, 1000, 900).unwrap()).unwrap();
            context_menu::pick(&mut h.app, if horizontal { "ui.horizontalConstraint" } else { "ui.verticalConstraint" }).unwrap();
            for _ in 0..4 {
                raster::paint(&mut h, &mut textures, vec![]);
            }
            let output = raster::paint(&mut h, &mut textures, vec![]);
            raster::raster(&mut h, &textures, output).save(dir.join(format!("{prefix}-aligned.png"))).unwrap();
            std::fs::write(dir.join(format!("{prefix}-aligned-model.png")), solidcraft_ui_egui::control::view_png(&h.app, 1000, 900).unwrap())
                .unwrap();
            if !horizontal {
                context_menu::open_for(&mut h.app, egui::pos2(880., 800.), Target::Viewport);
                context_menu::pick(&mut h.app, "ui.horizontalConstraint").unwrap();
                for _ in 0..4 {
                    raster::paint(&mut h, &mut textures, vec![]);
                }
                let output = raster::paint(&mut h, &mut textures, vec![]);
                raster::raster(&mut h, &textures, output).save(dir.join(format!("{prefix}-both.png"))).unwrap();
                std::fs::write(dir.join(format!("{prefix}-both-model.png")), solidcraft_ui_egui::control::view_png(&h.app, 1000, 900).unwrap())
                    .unwrap();
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

/// Horizontal and Vertical are tools of their own: each goes its own way, even for a line drawn
/// nearer the other way (which the combined tool would pick), and shows a thin dashed guide along
/// that way while it is on. The combined tool shows none.
#[test]
fn horizontal_and_vertical_tools_go_their_own_way_and_show_a_guide() {
    use solidcraft_ui_egui::scenario;
    for (tool, horizontal) in [("sketch.constraint.horizontal", true), ("sketch.constraint.vertical", false)] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let (end, middle, beside) = if horizontal { ([13, 40], [11.5, 25.0], [16.0, 25.0]) } else { ([40, 13], [25.0, 11.5], [25.0, 16.0]) };
        h.app.run("sketch.line", json!({"points":[[10,10],end],"infer":false,"connect":false})).unwrap();
        h.app.run("sketch.point", json!({"point":[20,24],"id":"q","connect":false})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.app.fit_view();
        h.frames(3);
        // Every point clear of the cards that float over the view's edges.
        h.app.cam.distance *= 2.0;
        h.app.cam_anim = None;
        h.frames(1);
        h.step(&json!({"start": tool})).unwrap();
        h.step(&json!({"move":{"sketch":beside}})).unwrap();
        assert!(h.app.tool.as_ref().is_some_and(|t| t.hover.is_some()), "{tool}: no hover");
        h.frames(1);
        let guide = scenario::rect_of("viewport:axis_guide").unwrap_or_else(|| panic!("{tool}: no guide"));
        let (along, across) = if horizontal { (guide.width(), guide.height()) } else { (guide.height(), guide.width()) };
        assert!(along > 100.0 && across < 1.0, "{tool}: {guide:?}");
        h.step(&json!({"click":{"sketch":middle}})).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{tool}: {:?}", h.app.status);
        let sk = &h.app.session.model.state().sketches[0].sketch;
        let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
        let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
        let wanted = if horizontal { ConstraintKind::Horizontal { l: 0 } } else { ConstraintKind::Vertical { l: 0 } };
        assert!(sk.constraints.iter().any(|c| std::mem::discriminant(&c.kind) == std::mem::discriminant(&wanted)), "{tool}");
        assert!(if horizontal { (a.y - b.y).abs() < 1e-6 } else { (a.x - b.x).abs() < 1e-6 }, "{tool}: {a:?} {b:?}");
        // The tool stays on for the next pick.
        assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some(tool));
        // After the first of two points the one guide still follows the cursor; none stays
        // behind through the picked point.
        let across = |name: &str| scenario::rect_of(name).map(|r| if horizontal { r.center().y } else { r.center().x });
        let (start, q) = ([a.x, a.y], [20, 24]);
        h.step(&json!({"click":{"sketch":start}})).unwrap();
        assert_eq!(h.app.tool.as_ref().unwrap().point_picks.len(), 1, "{tool}");
        let mut last = None;
        for at in [[17, 18], [24, 20]] {
            h.step(&json!({"move":{"sketch":at}})).unwrap();
            h.frames(1);
            assert_eq!(scenario::rects("viewport:axis_guide").len(), 1, "{tool}");
            let cursor = across("viewport:axis_guide");
            assert!(cursor.is_some() && cursor != last, "{tool}: {cursor:?}");
            last = cursor;
        }
        h.step(&json!({"click":{"sketch":q}})).unwrap();
        h.step(&json!({"move":{"sketch":[20,20]}})).unwrap();
        h.frames(1);
        assert!(h.app.tool.as_ref().is_some_and(|t| t.point_picks.is_empty()), "{tool}: {:?}", h.app.status);
        assert_eq!(scenario::rects("viewport:axis_guide").len(), 1, "{tool}");
    }
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.step(&json!({"start":"sketch.constraint.horizontal_vertical"})).unwrap();
    h.step(&json!({"move":{"sketch":[30,30]}})).unwrap();
    h.frames(1);
    assert!(solidcraft_ui_egui::scenario::rect_of("viewport:axis_guide").is_none());
}
