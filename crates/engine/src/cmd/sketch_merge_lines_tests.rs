//! `sketch.merge_lines` (#20317): lines joined end to end on one straight line become one line.

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sketch(s: &Session) -> Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn ends(sk: &Sketch, id: &str) -> (Vec2, Vec2) {
    let Some(CurveKind::Line { a, b }) = sk.curve_index(id).and_then(|i| sk.curves.get(i)).map(|c| &c.kind) else { panic!("{id} is not a line") };
    (sk.point(*a).unwrap(), sk.point(*b).unwrap())
}

fn at(p: Vec2, x: f64, y: f64) -> bool {
    p.dist(Vec2::new(x, y)) < 1e-6
}

/// A run drawn as three pieces of one chain becomes the first selected line, end to end; the
/// other two go, and with them the points between the pieces.
#[test]
fn a_run_of_three_pieces_becomes_one_line() {
    let mut s = new_sketch();
    let run_ids = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [25, 0], [40, 0]], "infer": false}))["curves"]);
    assert_eq!(run_ids.len(), 3);
    let points = sketch(&s).points.len();

    let out = run(&mut s, "sketch.merge_lines", json!({"curves": [run_ids[1], run_ids[0], run_ids[2]]}));
    assert_eq!(out["line"], json!(run_ids[1]), "{out}");
    assert_eq!(ids(&out["removed"]), vec![run_ids[0].clone(), run_ids[2].clone()], "{out}");
    let sk = sketch(&s);
    assert_eq!(sk.curves.len(), 1, "{:?}", sk.curves);
    let (a, b) = ends(&sk, &run_ids[1]);
    assert!(at(a, 0.0, 0.0) && at(b, 40.0, 0.0), "{a:?} {b:?}");
    assert_eq!(sk.points.len(), points - 2, "the two points between the pieces go");
}

/// The kept line keeps its own direction: a line drawn right to left still runs right to left.
#[test]
fn the_kept_line_keeps_its_direction() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 5]], "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[30, 5], [10, 5]], "infer": false}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": "l2.end"}));
    let out = run(&mut s, "sketch.merge_lines", json!({"curves": ["l2", "l1"]}));
    assert_eq!(out["line"], json!("l2"), "{out}");
    let sk = sketch(&s);
    let (a, b) = ends(&sk, "l2");
    assert!(at(a, 30.0, 5.0) && at(b, 0.0, 5.0), "{a:?} {b:?}");
    assert!(
        !sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Coincident { .. })),
        "the tie between the pieces goes: {:?}",
        sk.constraints
    );
}

/// A line ending where two pieces meet (a cross member's line) stays on the merged line.
#[test]
fn a_line_ending_between_the_pieces_stays_on_the_merged_line() {
    let mut s = new_sketch();
    let run_ids = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [20, 0]], "infer": false}))["curves"]);
    let sk = sketch(&s);
    let joint = sk.curves[sk.curve_index(&run_ids[0]).unwrap()].kind.ends().unwrap().1;
    let joint_id = sk.points[joint].id.clone();
    let cross = run(&mut s, "sketch.line", json!({"points": [[10, 0], [10, 10]], "infer": false}))["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.constraint.coincident", json!({"a": joint_id, "b": format!("{cross}.start")}));

    let out = run(&mut s, "sketch.merge_lines", json!({"curves": run_ids}));
    let kept = out["line"].as_str().unwrap().to_string();
    let sk = sketch(&s);
    let line = sk.curve_index(&kept).unwrap();
    let start = sk.curves[sk.curve_index(&cross).unwrap()].kind.ends().unwrap().0;
    let groups = super::super::sketch::coincident_points(&sk);
    let held = sk.constraints.iter().any(|k| match k.kind {
        ConstraintKind::PointOnCurve { p, c } => c == line && groups[p] == groups[start],
        _ => false,
    });
    assert!(held, "the cross line's end is held on the merged line: {:?}", sk.constraints);
    let (a, _) = ends(&sk, &cross);
    assert!(at(a, 10.0, 0.0), "{a:?}");

    // Dragging the merged line drags the cross line's end with it.
    run(&mut s, "sketch.move_point", json!({"point": format!("{kept}.start"), "to": [0, 5]}));
    run(&mut s, "sketch.move_point", json!({"point": format!("{kept}.end"), "to": [20, 5]}));
    let (a, _) = ends(&sketch(&s), &cross);
    assert!((a.y - 5.0).abs() < 1e-6, "{a:?}");
}

/// Relations that hold for the whole run move onto the merged line once; a length of one
/// piece measured that piece and goes.
#[test]
fn direction_relations_carry_over_and_piece_lengths_go() {
    let mut s = new_sketch();
    let run_ids = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [25, 0]], "infer": false}))["curves"]);
    for l in &run_ids {
        run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": l, "mode": "horizontal"}));
    }
    run(&mut s, "sketch.dimension", json!({"entities": [run_ids[1]], "value": "15"}));

    run(&mut s, "sketch.merge_lines", json!({"curves": run_ids}));
    let sk = sketch(&s);
    let line = sk.curve_index(&run_ids[0]).unwrap();
    let horizontal = sk.constraints.iter().filter(|k| matches!(k.kind, ConstraintKind::Horizontal { l } if l == line)).count();
    assert_eq!(horizontal, 1, "{:?}", sk.constraints);
    assert!(!sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Length { .. })), "{:?}", sk.constraints);
    let (a, b) = ends(&sk, &run_ids[0]);
    assert!(at(a, 0.0, 0.0) && at(b, 25.0, 0.0), "{a:?} {b:?}");
}

/// Lines that can't be one line are refused with the reason.
#[test]
fn lines_that_are_not_one_run_are_refused() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0], [10, 10]], "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[20, 0], [30, 0]], "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[5, 0], [15, 0]], "infer": false}));
    run(&mut s, "sketch.circle.center", json!({"center": [50, 50], "radius": 5}));
    for (p, why) in [
        (json!({"curves": ["l1"]}), "at least two"),
        (json!({"curves": ["l1", "l2"]}), "one straight line"),
        (json!({"curves": ["l1", "l3"]}), "end to end"),
        (json!({"curves": ["l1", "l4"]}), "end to end"),
        (json!({"curves": ["l1", "c1"]}), "not a line"),
        (json!({"curves": ["l1", "nope"]}), "nope"),
    ] {
        let err = s.execute("sketch.merge_lines", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
    }
}

/// #20317: a frame built on the run before the merge follows it: the merged line is one tube at
/// the run's full length, and the removed lines leave the frame's line list.
#[test]
fn a_frame_member_on_the_run_becomes_one_tube() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [300, 0], [600, 0], [600, 400]], "infer": false}));
    run(&mut s, "sketch.finish", json!({}));
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3});
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube, "lines": ["l1", "l2", "l3"], "names": {"l2": "Gone"}}));
    let bodies = |s: &mut Session| run(s, "inspect.measure", json!({}))["bodies"].as_array().unwrap().len();
    assert_eq!(bodies(&mut s), 3);

    run(&mut s, "sketch.edit", json!({"sketch": "Frame"}));
    let out = run(&mut s, "sketch.merge_lines", json!({"curves": ["l1", "l2"]}));
    assert_eq!(ids(&out["frames"]).len(), 1, "{out}");
    run(&mut s, "sketch.finish", json!({}));

    let frame = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap();
    let solidcraft_doc::FeatureKind::FrameMember { lines, names, .. } = &frame.kind else { panic!("not a Frame Member") };
    assert_eq!(lines, &["l1", "l3"]);
    assert!(!names.contains_key("l2"), "{names:?}");
    let m = run(&mut s, "inspect.measure", json!({}));
    let bs = m["bodies"].as_array().unwrap();
    assert_eq!(bs.len(), 2, "{m}");
    let long = bs.iter().any(|b| {
        (b["bbox"]["max"][0].as_f64().unwrap() - b["bbox"]["min"][0].as_f64().unwrap() - 600.0).abs() < 41.0
            && b["bbox"]["max"][1].as_f64().unwrap() < 21.0
    });
    assert!(long, "one tube runs the whole 600 mm: {m}");
}

/// An angle to a piece drawn the other way is half a turn off on the merged line: it is flipped,
/// so the line it measures to stays where it is.
#[test]
fn an_angle_to_a_reversed_piece_keeps_its_line_in_place() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 5]], "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[30, 5], [10, 5]], "infer": false}));
    run(&mut s, "sketch.line", json!({"points": [[10, 5], [10, 15]], "infer": false}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": "l2.end"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "l1.end", "b": "l3.start"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2", "l3"], "value": "90 deg"}));

    run(&mut s, "sketch.merge_lines", json!({"curves": ["l1", "l2"]}));
    let sk = sketch(&s);
    assert!(sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Angle { .. })), "{:?}", sk.constraints);
    let (a, b) = ends(&sk, "l3");
    assert!(at(a, 10.0, 5.0) && at(b, 10.0, 15.0), "the cross line stays put: {a:?} {b:?}");
}

/// A Rotate step on a line that went turns the merged member instead of failing.
#[test]
fn a_rotate_step_on_a_merged_away_line_turns_the_merged_member() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [300, 0], [600, 0]], "infer": false}));
    run(&mut s, "sketch.finish", json!({}));
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 0});
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube, "names": {"l2": "Gone"}}));
    let face = {
        let st = s.model.state();
        let b = st.body("Gone").unwrap();
        let top = |i: usize| b.body.planar_face(i).is_some_and(|pl| pl.normal().z > 0.999 && (pl.origin.z - 20.0).abs() < 1e-6);
        let i = (0..b.body.face_count()).find(|i| top(*i)).unwrap();
        json!({"body": "Gone", "index": i, "face": [450, 0, 20]})
    };
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));

    run(&mut s, "sketch.edit", json!({"sketch": "Frame"}));
    let out = run(&mut s, "sketch.merge_lines", json!({"curves": ["l1", "l2"]}));
    assert!(ids(&out["frames"]).iter().any(|f| f.starts_with("Rotate")), "{out}");
    run(&mut s, "sketch.finish", json!({}));

    let turn = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMemberTurn { .. })).unwrap();
    let solidcraft_doc::FeatureKind::FrameMemberTurn { member, .. } = &turn.kind else { panic!("not a Rotate step") };
    assert_eq!(member.line, "l1");
    let m = run(&mut s, "inspect.measure", json!({}));
    let bs = m["bodies"].as_array().unwrap();
    assert_eq!(bs.len(), 1, "{m}");
    let tall = bs[0]["bbox"]["max"][2].as_f64().unwrap() - bs[0]["bbox"]["min"][2].as_f64().unwrap();
    assert!(tall > 41.0, "the 600 mm member is turned: {m}");
}
