//! Constraint tools accept a selected curve pair as well as tool-first clicks.

mod support;

use serde_json::json;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::{Sel, sketch::ConstraintKind};
use solidcraft_ui_egui::scenario::Harness;

fn lines() -> Harness {
    support::sketch_lines(&[[[10, 10], [40, 10]], [[10, 30], [40, 35]]], None)
}

fn select_lines(h: &mut Harness) {
    // These tests select whole edges; the exact centre is now a selectable MID.
    h.run(&json!([
        {"click":{"sketch":[17.5,10]}},
        {"click":{"sketch":[17.5,31.25]},"shift":true}
    ]))
    .unwrap();
    assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: "l1".into(), sketch: None }, Sel::SketchCurve { id: "l2".into(), sketch: None }]);
}

fn assert_line_geometry(h: &Harness, command: &str) {
    let sk = &h.app.session.model.state().sketches[0].sketch;
    let point = |name| sk.point(sk.resolve_point(name).unwrap()).unwrap();
    let a = point("l1.start");
    let b = point("l2.start");
    let u = point("l1.end") - a;
    let v = point("l2.end") - b;
    // Normalization also rejects a collapsed line; zero-length vectors must not
    // accidentally satisfy the parallel/perpendicular checks.
    let du = u.normalized().unwrap();
    let dv = v.normalized().unwrap();
    match command {
        "parallel" => assert!(du.cross(dv).abs() < 1e-6),
        "perpendicular" => assert!(du.dot(dv).abs() < 1e-6),
        "equal" => assert!((u.len() - v.len()).abs() < 1e-6),
        "collinear" => {
            assert!(du.cross(dv).abs() < 1e-6);
            assert!((b - a).cross(du).abs() < 1e-6);
        }
        _ => panic!("unexpected line constraint: {command}"),
    }
}

#[test]
fn selected_lines_apply_constraints_with_one_undo_step() {
    for command in ["parallel", "perpendicular", "equal", "collinear"] {
        let mut h = lines();
        select_lines(&mut h);
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":format!("sketch.constraint.{command}")}])).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}");
        let sk = &h.app.session.model.state().sketches[0].sketch;
        assert!(
            sk.constraints.iter().any(|c| match command {
                "parallel" => matches!(c.kind, ConstraintKind::Parallel { .. }),
                "perpendicular" => matches!(c.kind, ConstraintKind::Perpendicular { .. }),
                "equal" => matches!(c.kind, ConstraintKind::Equal { .. }),
                _ => matches!(c.kind, ConstraintKind::Collinear { .. }),
            }),
            "{command}"
        );
        assert_line_geometry(&h, command);
        let after = h.app.session.doc.clone();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(h.app.session.selection.is_empty());
        assert!(h.app.tool.is_none());
        h.app.session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), serde_json::to_value(before).unwrap());
        h.app.session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), serde_json::to_value(after).unwrap());
        assert_line_geometry(&h, command);
    }
}

#[test]
fn selected_circles_apply_constraints() {
    for command in ["tangent", "concentric", "equal"] {
        let mut h = Harness::new();
        h.run(&json!([
            {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
            {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[10,10],"radius":5}}},
            {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[25,12],"radius":7}}},
            {"call":"ui.view","params":{"view":"top"}},
            {"click":{"sketch":[10,15]}},
            {"click":{"sketch":[25,19]},"shift":true}
        ]))
        .unwrap();
        assert_eq!(h.app.session.selection.len(), 2);
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":format!("sketch.constraint.{command}")}])).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "{command}");
        let sk = &h.app.session.model.state().sketches[0].sketch;
        assert!(sk.constraints.iter().any(|c| match command {
            "tangent" => matches!(c.kind, ConstraintKind::Tangent { .. }),
            "concentric" => matches!(c.kind, ConstraintKind::Concentric { .. }),
            _ => matches!(c.kind, ConstraintKind::Equal { .. }),
        }));
        assert!(h.app.session.selection.is_empty());
    }
}

/// #4985: Equal with three or more curves selected makes them all equal, in one undo step. With
/// one circle dimensioned, the others take its size.
#[test]
fn selected_circles_all_become_equal() {
    let mut h = Harness::new();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
        {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[0,0],"radius":5}}},
        {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[20,0],"radius":3}}},
        {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[40,0],"radius":7}}},
        {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[60,0],"radius":4}}},
        {"call":"engine.execute","params":{"command":"sketch.dimension","params":{"entities":["c1"]}}}
    ]))
    .unwrap();
    let radius = |h: &Harness, id: &str| {
        let sk = &h.app.session.model.state().sketches[0].sketch;
        sk.radius(sk.curve_index(id).unwrap()).unwrap()
    };
    let r1 = radius(&h, "c1");
    h.app.session.selection = ["c4", "c3", "c2", "c1"].iter().map(|id| Sel::SketchCurve { id: (*id).into(), sketch: None }).collect();
    let undo = h.app.session.undo.len();
    h.run(&json!([{"start":"sketch.constraint.equal"}])).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert!(h.app.tool.is_none(), "applied at once, no pick tool");
    assert!(h.app.session.selection.is_empty());
    let sk = &h.app.session.model.state().sketches[0].sketch;
    assert_eq!(sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Equal { .. })).count(), 3);
    for id in ["c1", "c2", "c3", "c4"] {
        assert!((radius(&h, id) - r1).abs() < 1e-6, "{id}: {} vs the dimensioned {r1}", radius(&h, id));
    }
}

#[test]
fn ambiguous_selection_preserves_tool_first_workflow() {
    let mut h = lines();
    let undo = h.app.session.undo.len();
    h.run(&json!([{"start":"sketch.constraint.parallel"}])).unwrap();
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.tool.as_ref().unwrap().picks.is_empty());
    h.run(&json!([{"click":{"sketch":[25,10]}}, {"click":{"sketch":[25,32.5]}}])).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));

    // A single compatible curve seeds the pick list so only one remaining click is needed,
    // and completing the constraint unselects the operator.
    let mut h = lines();
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    let undo = h.app.session.undo.len();
    h.run(&json!([{"start":"sketch.constraint.parallel"}])).unwrap();
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.tool.as_ref().unwrap().picks, vec!["l1"]);
    assert!(h.app.session.selection.is_empty());
    // Clicking the already-seeded curve ignores duplicates, then clicking the second applies Parallel
    // and exits the preselected constraint operator.
    h.run(&json!([{"click":{"sketch":[25,10]}}, {"click":{"sketch":[25,32.5]}}])).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert!(h.app.tool.is_none());

    // Over-limit / incompatible preselections are automatically unselected and start tool-first
    // picking (leaving the operator selected afterwards).
    let mut h = lines();
    h.app.session.selection = vec![
        Sel::SketchCurve { id: "l1".into(), sketch: None },
        Sel::SketchCurve { id: "l2".into(), sketch: None },
        Sel::SketchPoint { id: "origin".into() },
    ];
    let undo = h.app.session.undo.len();
    h.run(&json!([{"start":"sketch.constraint.parallel"}])).unwrap();
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(h.app.session.selection.is_empty());
    assert!(h.app.tool.as_ref().unwrap().picks.is_empty());
    h.run(&json!([{"click":{"sketch":[25,10]}}, {"click":{"sketch":[25,32.5]}}])).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.constraint.parallel" && t.picks.is_empty()));
}

/// #12092: a tool that constrains two curves takes the line when the click lands on its end
/// point. It used to pass the point on, and the command refused it ("unknown sketch curve
/// `l1.end`"). Tangent keeps the end: the line's end then moves onto the circle, tangent there.
#[test]
fn a_click_on_a_line_end_picks_the_line_for_two_curve_constraints() {
    for command in ["parallel", "perpendicular", "equal", "collinear"] {
        let mut h = lines();
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":format!("sketch.constraint.{command}")}, {"click":{"sketch":[40,10]}}, {"click":{"sketch":[40,35]}}])).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}: {:?}", h.app.status);
        assert_eq!(h.app.session.undo.len(), undo + 1, "{command}");
        assert_line_geometry(&h, command);
    }
    let mut h = Harness::new();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
        {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,20],[30,20]],"infer":false}}},
        {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[50,0],"radius":10}}},
        {"call":"ui.view","params":{"view":"top"}},
        {"start":"sketch.constraint.tangent"},
        {"click":{"sketch":[30,20]}},
        {"click":{"sketch":[60,0]}}
    ]))
    .unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "tangent: {:?}", h.app.status);
    let sk = &h.app.session.model.state().sketches[0].sketch;
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Tangent { .. })), "tangent");
    // The line now touches the circle: its distance from the centre is the radius.
    let a = sk.point(sk.resolve_point("l1.start").unwrap()).unwrap();
    let b = sk.point(sk.resolve_point("l1.end").unwrap()).unwrap();
    let circle = sk.curves.iter().position(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Circle { .. })).unwrap();
    let centre = sk.point(sk.resolve_point(&format!("{}.center", sk.curves[circle].id)).unwrap()).unwrap();
    let radius = sk.radius(circle).unwrap();
    let distance = (b - a).normalized().unwrap().cross(centre - a).abs();
    assert!((distance - radius).abs() < 1e-6, "line {a:?}..{b:?} is {distance} from the centre, radius {radius}");
    // The clicked end lies on the circle, where the line touches it.
    assert!((b.dist(centre) - radius).abs() < 1e-6, "the line's end {b:?} is {} from the centre, radius {radius}", b.dist(centre));
    // Only that end moved: the line turned about its other end, and the circle stayed.
    assert!(a.x.abs() < 1e-9 && (a.y - 20.0).abs() < 1e-9, "the line's other end moved to {a:?}");
    assert!((centre.x - 50.0).abs() < 1e-9 && centre.y.abs() < 1e-9 && (radius - 10.0).abs() < 1e-9, "the circle moved: {centre:?}, radius {radius}");
}

#[test]
fn rejected_preselection_preserves_document_and_error() {
    let mut h = lines();
    h.app.session.execute("sketch.constraint.fix", &json!({"entities":["l1","l2"]})).unwrap();
    select_lines(&mut h);
    let before = serde_json::to_value(&h.app.session.doc).unwrap();
    let undo = h.app.session.undo.len();
    let selection = h.app.session.selection.clone();
    h.run(&json!([{"start":"sketch.constraint.parallel"}])).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(serde_json::to_value(&h.app.session.doc).unwrap(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(h.app.session.selection, selection);
    // Correct the conflict and retry without reselecting; the old error must disappear.
    h.app.session.execute("sketch.constraint.fix", &json!({"entities":["l1","l2"]})).unwrap();
    h.run(&json!([{"start":"sketch.constraint.parallel"}])).unwrap();
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert!(h.app.session.selection.is_empty());
}

#[test]
fn curvature_polygon_and_fix_consume_clicked_operands_atomically() {
    for command in ["fix", "curvature", "polygon"] {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let picks = match command {
            "curvature" => {
                h.app.run("sketch.line", json!({"points":[[10,10],[30,10]],"infer":false})).unwrap();
                h.app.run("sketch.spline.control_point", json!({"points":["l1.end",[40,12],[50,16],[60,25]]})).unwrap();
                vec![[15., 10.], [45., 14.875]]
            }
            "polygon" => {
                h.app.run("sketch.line", json!({"points":[[10,10],[40,10],[40,30],[10,30]],"closed":true,"infer":false})).unwrap();
                vec![[17.5, 10.], [40., 15.], [32.5, 30.], [10., 25.]]
            }
            _ => {
                h.app.run("sketch.line", json!({"points":[[10,10],[40,10]],"infer":false})).unwrap();
                h.app.run("sketch.point", json!({"point":[20,25],"id":"p"})).unwrap();
                vec![[17.5, 10.], [20., 25.]]
            }
        };
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.0;
        h.app.cam.target = solidcore_record::Vec3::new(30.0, 15.0, 0.0);
        h.settle();
        for (i, p) in picks.iter().enumerate() {
            h.step(&json!({"click":{"sketch":p},"shift":i>0})).unwrap();
        }
        assert_eq!(h.app.session.selection.len(), picks.len(), "{command}: {:?}", h.app.session.selection);
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.step(&json!({"start":format!("sketch.constraint.{command}")})).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}: {:?}", h.app.status);
        assert!(h.app.tool.is_none());
        assert!(h.app.session.selection.is_empty());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let st = h.app.session.world_state().unwrap();
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.constraints.iter().any(|c| match command {
            "fix" => matches!(c.kind, ConstraintKind::Fix { .. }),
            "curvature" => matches!(c.kind, ConstraintKind::Smooth { .. }),
            _ => matches!(c.kind, ConstraintKind::Equal { .. }),
        }));
        match command {
            "fix" => {
                assert!(sk.curve_locked(sk.curve_index("l1").unwrap()));
                assert!(sk.point_locked(sk.resolve_point("p").unwrap()));
            }
            "curvature" => {
                let spline = sk.curves.iter().find(|c| c.id != "l1").unwrap();
                let pts: Vec<_> = spline.kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect();
                let line = sk.polyline(sk.curve_index("l1").unwrap());
                let direction = (line[1] - line[0]).normalized().unwrap();
                assert!((pts[1] - pts[0]).cross(direction).abs() < 1e-5);
                assert!((pts[2] - pts[1]).cross(pts[1] - pts[0]).abs() < 1e-3);
            }
            _ => {
                let lengths: Vec<_> = ["l1", "l2", "l3", "l4"]
                    .iter()
                    .map(|id| {
                        let pts = sk.polyline(sk.curve_index(id).unwrap());
                        pts[0].dist(pts[1])
                    })
                    .collect();
                assert!(lengths[0] > 1.0);
                assert!(lengths.iter().all(|l| (l - lengths[0]).abs() < 1e-6));
            }
        }
        assert!(st.sketch(h.app.session.active_sketch.unwrap()).unwrap().report.ok());
        let after = h.app.session.doc.to_json();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), after);
    }
}

#[test]
fn remaining_constraints_clear_invalid_selections_and_preserve_rejected_geometry() {
    for command in ["fix", "curvature", "polygon"] {
        let mut h = lines();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let mixed = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }, Sel::Body { name: "missing".into() }];
        h.app.session.selection = mixed;
        h.step(&json!({"start":format!("sketch.constraint.{command}")})).unwrap();
        assert!(h.app.tool.is_some());
        assert!(h.app.tool.as_ref().unwrap().picks.is_empty());
        assert!(h.app.session.selection.is_empty());
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
    }
    let mut h = lines();
    let selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }, Sel::SketchCurve { id: "missing".into(), sketch: None }];
    h.app.session.selection = selection.clone();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.constraint.curvature"})).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn constraint_pick_tools_suppress_snap_hint_and_keep_picks_until_finished() {
    use solidcraft_ui_egui::scenario::rect_of;
    let mut h = lines();
    h.app.home.open = false;
    h.call("ui.view", json!({"view": "top"}));
    h.settle();

    // Pick tool (Parallel): suppresses snap_hint over midpoints/edges and retains first pick until second click.
    h.step(&json!({"start": "sketch.constraint.parallel"})).unwrap();
    h.step(&json!({"move": {"sketch": [25.0, 10.0]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:snap_hint").is_none(), "Parallel should suppress snap_hint");
    h.step(&json!({"click": {"sketch": [25.0, 10.0]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.tool.as_ref().unwrap().picks, vec!["l1"]);
    h.step(&json!({"move": {"sketch": [25.0, 32.5]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:snap_hint").is_none(), "Parallel keeps snap_hint hidden before second pick");
    assert_eq!(h.app.tool.as_ref().unwrap().picks, vec!["l1"]);
    h.step(&json!({"click": {"sketch": [25.0, 32.5]}})).unwrap();
    h.frames(2);
    // Constraint pick tools clear their picks once a constraint commits so more pairs can follow.
    assert!(h.app.tool.as_ref().is_some_and(|t| t.picks.is_empty()));
    let st = h.app.session.world_state().unwrap();
    let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Parallel { .. })));
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(h.app.tool.is_none());

    // Horizontal/Vertical: suppresses snap_hint normally, keeps Shift-Midpoint snap_hint working.
    h.step(&json!({"start": "sketch.constraint.horizontal_vertical"})).unwrap();
    h.step(&json!({"move": {"sketch": [25.0, 10.0]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:snap_hint").is_none(), "H/V should suppress snap_hint without Shift");
    assert!(rect_of("viewport:readout").is_none(), "H/V should suppress coordinate readout");
    assert!(h.app.tool.as_ref().unwrap().hover.is_none(), "H/V should suppress auto-snap hover without Shift");
    h.step(&json!({"move": {"sketch": [25.0, 10.0]}, "shift": true})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:snap_hint").is_none(), "H/V with Shift should still suppress snap_hint badge");
    assert_eq!(
        h.app.tool.as_ref().unwrap().hover.as_ref().and_then(|(_, s)| s.as_deref()),
        Some("Midpoint"),
        "H/V with Shift should still resolve Midpoint hover"
    );
    h.step(&json!({"key": "Escape"})).unwrap();

    // Entities-mode constraint tool (Curvature): suppresses snap_hint too.
    h.step(&json!({"start": "sketch.constraint.curvature"})).unwrap();
    h.step(&json!({"move": {"sketch": [25.0, 10.0]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:snap_hint").is_none(), "Curvature constraint tool should suppress snap_hint");
    h.step(&json!({"key": "Escape"})).unwrap();

    // Dimension tool: suppresses snap_hint like other constraint tools, while keeping plane hover and readout before typing.
    h.step(&json!({"start": "sketch.dimension"})).unwrap();
    h.step(&json!({"move": {"sketch": [25.0, 10.0]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:snap_hint").is_none(), "Dimension should suppress snap_hint");
    assert!(rect_of("viewport:readout").is_some(), "Dimension should keep coordinate readout while hovering");
    assert!(h.app.tool.as_ref().unwrap().hover.is_some(), "Dimension should keep raw plane hover for live preview");
    h.step(&json!({"key": "Escape"})).unwrap();
}

#[test]
fn incompatible_curve_or_preselected_dimension_clears_or_exits() {
    // Circles cannot participate in line-only constraints: auto-unselect and enter tool empty.
    for command in ["parallel", "perpendicular", "collinear", "horizontal_vertical"] {
        let mut h = Harness::new();
        h.run(&json!([
            {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
            {"call":"engine.execute","params":{"command":"sketch.circle.center","params":{"center":[20,20],"radius":8}}},
            {"call":"ui.view","params":{"view":"top"}}
        ]))
        .unwrap();
        h.app.session.selection = vec![Sel::SketchCurve { id: "c1".into(), sketch: None }];
        h.step(&json!({"start": format!("sketch.constraint.{command}")})).unwrap();
        assert!(h.app.session.selection.is_empty(), "{command} should unselect incompatible circle");
        assert!(h.app.tool.as_ref().is_some_and(|t| t.picks.is_empty() && t.point_picks.is_empty()), "{command}");
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}: {:?}", h.app.status);
    }

    // Invalid selection before Dimension auto-unselects and enters sticky Dimension empty.
    let mut h = lines();
    h.app.session.selection = vec![Sel::Body { name: "Body1".into() }];
    h.step(&json!({"start": "sketch.dimension"})).unwrap();
    assert!(h.app.session.selection.is_empty());
    assert!(h.app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.dimension" && t.picks.is_empty() && !t.preselected));

    // Preselected line in Dimension places once and exits the Dimension operator.
    let mut h = lines();
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    h.step(&json!({"start": "sketch.dimension"})).unwrap();
    assert!(h.app.session.selection.is_empty());
    assert!(h.app.tool.as_ref().is_some_and(|t| t.picks == vec!["l1"] && t.preselected));
    h.step(&json!({"click": {"sketch": [25.0, 2.0]}})).unwrap();
    h.frames(1);
    h.step(&json!({"key": "Return"})).unwrap();
    h.frames(1);
    assert!(h.app.tool.is_none(), "preselected dimension should exit tool after committing");
}

#[test]
fn constraint_pick_tools_show_constraint_cursor_icon_and_avoid_duplication() {
    use solidcraft_ui_egui::scenario::rect_of;
    let mut h = lines();
    h.app.home.open = false;
    h.app.ui.tool_cursor = true;
    h.call("ui.view", json!({"view": "top"}));
    h.settle();

    // Every constraint/dimension pick tool publishes `viewport:constraint_cursor`, suppresses
    // `viewport:snap_hint`, and never draws a duplicate `viewport:tool_cursor` badge.
    for cmd in [
        "sketch.constraint.parallel",
        "sketch.constraint.horizontal_vertical",
        "sketch.constraint.curvature",
        "sketch.constraint.polygon",
        "sketch.constrainer",
        "sketch.dimension",
    ] {
        h.step(&json!({"start": cmd})).unwrap();
        h.step(&json!({"move": {"sketch": [25.0, 20.0]}})).unwrap();
        h.frames(2);
        let badge = rect_of("viewport:constraint_cursor").unwrap_or_else(|| panic!("{cmd} should show cursor icon"));
        assert!((badge.width() - 15.0).abs() < 0.1, "{cmd} default cursor icon width should be 15 px, got {}", badge.width());
        assert!((badge.height() - 15.0).abs() < 0.1, "{cmd} default cursor icon height should be 15 px, got {}", badge.height());
        assert!(rect_of("viewport:tool_cursor").is_none(), "{cmd} must not duplicate icon via tool_cursor");
        assert!(rect_of("viewport:snap_hint").is_none(), "{cmd} should suppress snap_hint");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(1);
        assert!(rect_of("viewport:constraint_cursor").is_none(), "{cmd} should clear cursor icon on Escape");
    }

    // Badge size follows `constraint_glyph_scale`.
    h.app.preferences.constraint_glyph_scale = 2.0;
    h.step(&json!({"start": "sketch.constraint.equal"})).unwrap();
    h.step(&json!({"move": {"sketch": [25.0, 20.0]}})).unwrap();
    h.frames(2);
    let scaled = rect_of("viewport:constraint_cursor").expect("scaled cursor icon");
    assert!((scaled.width() - 30.0).abs() < 0.1 && (scaled.height() - 30.0).abs() < 0.1, "expected 30 px badge, got {scaled:?}");
    h.app.preferences.constraint_glyph_scale = 1.0;
    h.step(&json!({"key": "Escape"})).unwrap();

    // Dimension hides the cursor badge while the floating dimension editor box is open.
    h.app.session.selection = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }];
    h.step(&json!({"start": "sketch.dimension"})).unwrap();
    h.step(&json!({"move": {"sketch": [25.0, 2.0]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:constraint_cursor").is_some(), "Dimension should show cursor icon before editor opens");
    h.step(&json!({"click": {"sketch": [25.0, 2.0]}})).unwrap();
    h.frames(2);
    assert!(rect_of("viewport:constraint_cursor").is_none(), "Dimension should hide cursor icon while editor box owns the cursor");
    h.step(&json!({"key": "Escape"})).unwrap();
}
