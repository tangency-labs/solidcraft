//! Horizontal/Vertical accepts either one line or two distinct sketch points.

use serde_json::json;
use solidcraft_engine::sketch::ConstraintKind;
use solidcraft_ui_egui::scenario::Harness;

fn line_midpoint(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> solidcraft_engine::geom::Vec2 {
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
        h.run(&json!([{"click":{"sketch":[25,11.5]}}])).unwrap();
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
    for selection in [
        vec![Sel::SketchPoint { id: "origin".into() }],
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
        h.app.session.selection = selection.clone();
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":"sketch.constraint.horizontal_vertical"}])).unwrap();
        assert!(h.app.tool.as_ref().unwrap().point_picks.is_empty());
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(h.app.session.selection, selection);
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
        assert_eq!(sk.point(0).unwrap(), solidcraft_engine::geom::Vec2::ZERO);
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
