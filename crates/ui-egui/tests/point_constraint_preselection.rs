use serde_json::json;
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::{Sel, sketch::ConstraintKind};
use solidcraft_ui_egui::scenario::Harness;

fn setup() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[10,10],[30,10]],"infer":false,"connect":false})).unwrap();
    h.app.run("sketch.point", json!({"point":[20,20],"id":"a"})).unwrap();
    h.app.run("sketch.point", json!({"point":[25,30],"id":"b"})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h
}

#[test]
fn coincident_tool_picks_an_origin_axis() {
    let mut h = setup();
    h.app.set_origin_shown(true);
    h.settle();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.constraint.coincident"})).unwrap();
    h.step(&json!({"click":{"world":[20,20,0]}})).unwrap();
    h.step(&json!({"click":{"world":[0,10,0]}})).unwrap();
    let state = h.app.session.world_state().unwrap();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    assert!(sk.point(sk.resolve_point("a").unwrap()).unwrap().x.abs() < 1e-6);
    assert_eq!(sk.links.len(), 1);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn equal_tool_refuses_to_pick_an_origin_plane_or_axis() {
    let mut h = setup();
    h.app.set_origin_shown(true);
    h.settle();
    h.step(&json!({"start":"sketch.constraint.equal"})).unwrap();
    h.step(&json!({"click":{"world":[0,10,0]}})).unwrap();
    assert!(h.app.tool.as_ref().unwrap().picks.is_empty(), "equal cannot pick an axis or plane");
}

#[test]
fn line_constraints_skip_end_on_datum_axes() {
    let mut h = Harness::new();
    h.app.run("construct.axis.two_points", json!({"a":{"point":[-15,6,0]},"b":{"point":[-15,6,20]},"name":"End on"})).unwrap();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.set_origin_shown(true);
    h.call("ui.view", json!({"view":"iso"}));
    h.app.cam.distance = 150.0;
    h.settle();

    for (name, at) in [("Z", [0, 0, 10]), ("End on", [-15, 6, 10])] {
        h.step(&json!({"move":{"world":at}})).unwrap();
        assert_eq!(h.call("ui.selection", json!({}))["result"]["hover"]["name"], name, "the axis is under the pointer");
        for cmd in ["parallel", "perpendicular", "collinear", "tangent"] {
            h.step(&json!({"start":format!("sketch.constraint.{cmd}")})).unwrap();
            h.step(&json!({"click":{"world":at}})).unwrap();
            assert!(h.app.tool.as_ref().unwrap().picks.is_empty(), "{cmd} needs a curve");
        }
        h.step(&json!({"start":"sketch.constraint.coincident"})).unwrap();
        h.step(&json!({"click":{"world":at}})).unwrap();
        let picks = &h.app.tool.as_ref().unwrap().picks;
        assert_eq!(picks.len(), 1);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&picks[0]).unwrap(), json!({"axis":name}));
        h.step(&json!({"key":"Escape"})).unwrap();
    }
}

#[test]
fn symmetry_uses_the_datum_picked_before_two_construction_lines() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    for points in [[[-20, 15], [-10, 15]], [[10, 15], [20, 15]]] {
        h.app.run("sketch.line", json!({"points":points,"construction":true,"infer":false,"connect":false})).unwrap();
    }
    h.app.set_origin_shown(true);
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 150.0;
    h.settle();
    h.step(&json!({"start":"sketch.constraint.symmetry"})).unwrap();
    for at in [[0, 8, 0], [-14, 15, 0], [14, 15, 0]] {
        h.step(&json!({"click":{"world":at}})).unwrap();
    }
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    let state = h.app.session.world_state().unwrap();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let axis = sk.link_curves(&sk.links[0].id)[0];
    let symmetry: Vec<_> = sk
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            ConstraintKind::Symmetric { l, .. } => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(symmetry, [axis, axis]);
}

#[test]
fn clicked_point_constraint_preselection_applies_and_undoes_once() {
    // Pick the edge away from its selectable MID; the point/edge constraint is unchanged.
    for (command, picks) in [
        ("coincident", vec![[20, 20], [25, 30]]),
        ("coincident", vec![[20, 20], [15, 10]]),
        ("coincident", vec![[15, 10], [20, 20]]),
        ("midpoint", vec![[20, 20], [15, 10]]),
        ("midpoint", vec![[15, 10], [20, 20]]),
        ("symmetry", vec![[15, 10], [20, 20], [25, 30]]),
        ("symmetry", vec![[20, 20], [15, 10], [25, 30]]),
    ] {
        let mut h = setup();
        for (index, pick) in picks.iter().enumerate() {
            h.step(&json!({"click":{"sketch":pick},"shift":index>0})).unwrap();
        }
        assert_eq!(h.app.session.selection.len(), picks.len(), "{command}");
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        h.step(&json!({"start":format!("sketch.constraint.{command}")})).unwrap();
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{command}: {:?}", h.app.status);
        assert!(h.app.tool.is_none());
        assert!(h.app.session.selection.is_empty());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        let state = h.app.session.world_state().unwrap();
        let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.constraints.iter().any(|c| match command {
            "coincident" => matches!(c.kind, ConstraintKind::Coincident { .. } | ConstraintKind::PointOnCurve { .. }),
            "midpoint" => matches!(c.kind, ConstraintKind::Midpoint { .. }),
            _ => matches!(c.kind, ConstraintKind::Symmetric { .. }),
        }));
        let point = |id| sk.point(sk.resolve_point(id).unwrap()).unwrap();
        let a = point("a");
        let b = point("b");
        let start = point("l1.start");
        let end = point("l1.end");
        let axis = (end - start).normalized().unwrap();
        match command {
            "midpoint" => assert!(a.dist((start + end) * 0.5) < 1e-6),
            "symmetry" => {
                assert!(((a + b) * 0.5 - start).cross(axis).abs() < 1e-6);
                assert!((b - a).dot(axis).abs() < 1e-6);
            }
            _ if picks.contains(&[15, 10]) => assert!((a - start).cross(axis).abs() < 1e-6),
            _ => assert!(a.dist(b) < 1e-6),
        }
        let after = h.app.session.doc.to_json();
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), after);
    }
}

#[test]
fn rejected_and_ambiguous_preselection_preserve_document_and_selection() {
    let mut h = setup();
    h.app.run("sketch.constraint.fix", json!({"entities":["a","b"]})).unwrap();
    h.app.session.selection = vec![Sel::SketchPoint { id: "a".into() }, Sel::SketchPoint { id: "b".into() }];
    let selection = h.app.session.selection.clone();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.constraint.coincident"})).unwrap();
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    assert_eq!(h.app.session.selection, selection);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.undo.len(), undo);
    for command in ["coincident", "midpoint", "symmetry"] {
        h.app.session.selection = vec![Sel::SketchPoint { id: "a".into() }];
        h.step(&json!({"start":format!("sketch.constraint.{command}")})).unwrap();
        assert!(h.app.tool.as_ref().unwrap().picks.is_empty());
        assert_eq!(h.app.session.selection.len(), 1);
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

// MidPoint centres one line on another when the tool picks two lines.
fn two_lines() -> Harness {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[60,0]],"infer":false,"connect":false})).unwrap();
    h.app.run("sketch.line", json!({"points":[[5,-10],[25,-10]],"infer":false,"connect":false})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h
}

fn line_ends(h: &Harness, id: &str) -> (Vec2, Vec2) {
    let state = h.app.session.world_state().unwrap();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    let p = |end: &str| sk.point(sk.resolve_point(&format!("{id}.{end}")).unwrap()).unwrap();
    (p("start"), p("end"))
}

fn assert_centred(h: &Harness, undo: usize) {
    assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error), "{:?}", h.app.status);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let (a, b) = line_ends(h, "l1");
    assert_eq!((a, b), (Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0)), "the first line stays");
    let (c, d) = line_ends(h, "l2");
    assert!(((a + b) * 0.5).dist((c + d) * 0.5) < 1e-6);
    assert!((c.dist(d) - 20.0).abs() < 1e-6);
}

#[test]
fn midpoint_tool_clicking_two_lines_centres_them() {
    let mut h = two_lines();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.constraint.midpoint"})).unwrap();
    h.step(&json!({"click":{"sketch":[45,0]}})).unwrap();
    h.step(&json!({"click":{"sketch":[10,-10]}})).unwrap();
    assert_centred(&h, undo);
}

#[test]
fn midpoint_tool_takes_two_preselected_lines() {
    let mut h = two_lines();
    h.step(&json!({"click":{"sketch":[45,0]}})).unwrap();
    h.step(&json!({"click":{"sketch":[10,-10]},"shift":true})).unwrap();
    assert_eq!(h.app.session.selection.len(), 2);
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"sketch.constraint.midpoint"})).unwrap();
    assert!(h.app.tool.is_none());
    assert_centred(&h, undo);
}

fn midpoint_ids(h: &Harness) -> Vec<String> {
    let state = h.app.session.world_state().unwrap();
    let sk = &state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
    sk.constraints.iter().filter(|c| matches!(c.kind, ConstraintKind::Midpoint { .. })).map(|c| c.id.clone()).collect()
}

fn point_count(h: &Harness) -> usize {
    let state = h.app.session.world_state().unwrap();
    state.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch.points.len()
}

/// Centre l2 on l1 with the MidPoint tool, then click its glyph and press Delete.
fn centre_then_delete_glyph(h: &mut Harness) {
    h.step(&json!({"start":"sketch.constraint.midpoint"})).unwrap();
    h.step(&json!({"click":{"sketch":[45,0]}})).unwrap();
    h.step(&json!({"click":{"sketch":[10,-10]}})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    let ids = midpoint_ids(h);
    assert!(ids.len() >= 2, "one point at the middle of both lines");
    let glyphs: Vec<_> = solidcraft_ui_egui::sketch_tools::glyph_positions().into_iter().filter(|(id, _)| ids.contains(id)).collect();
    assert_eq!(glyphs.len(), 1, "one MidPoint glyph for both lines: {glyphs:?}");
    let at = glyphs[0].1;
    h.step(&json!({"click":[at.x, at.y]})).unwrap();
    h.step(&json!({"key":"Delete"})).unwrap();
    for _ in 0..20 {
        if midpoint_ids(h).is_empty() {
            break;
        }
        h.frames(1);
    }
    assert!(midpoint_ids(h).is_empty(), "deleting the glyph removes both constraints: {:?}", midpoint_ids(h));
}

#[test]
fn centred_lines_show_one_midpoint_glyph_that_deletes_both_constraints_and_their_point() {
    let mut h = two_lines();
    let points = point_count(&h);
    centre_then_delete_glyph(&mut h);
    assert_eq!(point_count(&h), points, "the point that only tied the midpoints goes too");
}

#[test]
fn deleting_the_shared_midpoint_glyph_keeps_a_point_another_line_uses() {
    let mut h = two_lines();
    // l3 starts at the midpoint of l2: centring reuses that point, and deleting keeps it.
    h.app.run("sketch.line", json!({"points":["mid:l2",[15,-30]],"infer":false,"connect":false})).unwrap();
    let points = point_count(&h);
    centre_then_delete_glyph(&mut h);
    assert_eq!(point_count(&h), points);
}
