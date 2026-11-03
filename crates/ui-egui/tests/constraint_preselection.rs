//! Constraint tools accept a selected curve pair as well as tool-first clicks.

mod support;

use serde_json::json;
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
    for selection in [
        vec![],
        vec![Sel::SketchCurve { id: "l1".into(), sketch: None }],
        vec![
            Sel::SketchCurve { id: "l1".into(), sketch: None },
            Sel::SketchCurve { id: "l2".into(), sketch: None },
            Sel::SketchPoint { id: "origin".into() },
        ],
    ] {
        let mut h = lines();
        h.app.session.selection = selection;
        let undo = h.app.session.undo.len();
        h.run(&json!([{"start":"sketch.constraint.parallel"}])).unwrap();
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(h.app.tool.as_ref().unwrap().picks.is_empty());
        h.run(&json!([{"click":{"sketch":[25,10]}}, {"click":{"sketch":[25,32.5]}}])).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    }
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
        h.app.cam.target = solidcraft_engine::geom::Vec3::new(30.0, 15.0, 0.0);
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
fn remaining_constraints_preserve_mixed_selections_and_rejected_geometry() {
    for command in ["fix", "curvature", "polygon"] {
        let mut h = lines();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let mixed = vec![Sel::SketchCurve { id: "l1".into(), sketch: None }, Sel::Body { name: "missing".into() }];
        h.app.session.selection = mixed.clone();
        h.step(&json!({"start":format!("sketch.constraint.{command}")})).unwrap();
        assert!(h.app.tool.is_some());
        assert_eq!(h.app.session.selection, mixed);
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
