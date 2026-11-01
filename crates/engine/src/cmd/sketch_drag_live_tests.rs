//! Live drags (`sketch.drag` with `live: true`): each frame solves the sketch once, without a
//! rank analysis, and evaluates nothing after it; the gesture's end evaluates the model once.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::solve_counts;
use crate::Session;
use crate::cmd::sketch_project::refresh_count;

#[test]
fn determined_pattern_geometry_does_not_attempt_conflicting_held_solves() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [6.5, 6.5]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 1.5}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 1.5}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    let pattern = run(
        &mut s,
        "sketch.pattern.rectangular",
        json!({"entities": ["l1", "l2", "l3", "l4"], "count": 3, "count2": 3, "spacing": 2.2, "spacing2": -2.2}),
    );
    let line = pattern["curves"][0].as_str().expect("a patterned line");
    let endpoint = format!("{line}.start");
    let before = sketch_of(&s);
    assert!(!before.curve_locked(before.curve_index(line).expect("patterned line index")));
    for entity in [line, endpoint.as_str()] {
        for live in [true, false] {
            reset(&mut s);
            let result = run(&mut s, "sketch.drag", json!({"entity": entity, "from": [5, 5], "to": [9, 12], "live": live}));
            assert_eq!(result["reached"], json!(0.0));
            assert_eq!(result["at"], json!([5.0, 5.0]));
            assert_eq!(result["sketch"]["solved"], json!(true));
            let full = usize::from(!live);
            assert_eq!(solve_counts::take(), (full, full), "no held solve for determined geometry");
            assert_eq!(sketch_of(&s), before);
        }
    }
    reset(&mut s);
    run(&mut s, "sketch.move_point", json!({"point": endpoint, "to": [9, 12]}));
    assert_eq!(solve_counts::take(), (1, 1), "move_point also skips held solves");
    assert_eq!(sketch_of(&s), before);

    // A cached determined report must not suppress an outstanding dimension edit.
    let dimension = before
        .constraints
        .iter()
        .find(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Length { l: 0, .. }))
        .and_then(|c| c.param.as_ref())
        .expect("the first line's dimension");
    s.doc_mut().params.iter_mut().find(|p| &p.name == dimension).expect("dimension parameter").expr = "3".into();
    run(&mut s, "sketch.drag", json!({"entity": line, "from": [5, 5], "to": [9, 12], "live": true}));
    assert!((pt(&s, "l1.start").dist(pt(&s, "l1.end")) - 3.0).abs() < 1e-6);
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A dimensioned 20 x 15 rectangle from (5, 5) extruded 5 mm, its sketch being edited again;
/// returns the session and the extrude's id.
fn extruded() -> (Session, u64) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [25, 20]}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 20}));
    run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 15}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    let extrude = s.doc.features.last().map(|f| f.id).expect("the extrude");
    run(&mut s, "sketch.edit", json!({"sketch": "Sketch1"}));
    (s, extrude)
}

/// Counters from here on.
fn reset(s: &mut Session) {
    s.evaluations = 0;
    solve_counts::take();
}

/// Frames moving the rectangle's corner (5, 5) (and so the whole rectangle) left by 1 mm each.
fn frames(s: &mut Session, n: usize, live: bool) -> Vec<Value> {
    (1..=n)
        .map(|k| {
            let (from, to) = ([6.0 - k as f64, 5.0], [5.0 - k as f64, 5.0]);
            run(s, "sketch.drag", json!({"entity": "l1.start", "from": from, "to": to, "live": live}))
        })
        .collect()
}

fn body_contains(s: &Session, p: Vec3) -> bool {
    s.model.state().bodies.first().expect("a body").body.contains(p)
}

fn pt(s: &Session, id: &str) -> solidcraft_geom::Vec2 {
    let sk = s.active_sketch.and_then(|i| s.doc.sketch(i).ok()).expect("sketch mode");
    sk.resolve_point(id).and_then(|i| sk.point(i)).expect("the point")
}

fn corner(s: &Session) -> solidcraft_geom::Vec2 {
    let id = s.active_sketch.expect("sketch mode");
    let st = s.world_state();
    let sk = &st.sketch(id).expect("the shown sketch").sketch;
    sk.resolve_point("l1.start").and_then(|i| sk.point(i)).expect("the corner")
}

#[test]
fn a_live_frame_solves_once_and_leaves_the_features_after_the_sketch_stale() {
    let (mut s, extrude) = extruded();
    let body = s.model.state().bodies.first().expect("a body").body.clone();
    reset(&mut s);
    let r = frames(&mut s, 1, true).remove(0);
    assert_eq!(s.evaluations, 0, "a live frame evaluates no feature");
    assert_eq!(solve_counts::take(), (1, 0), "one held solve, no rank analysis");
    assert_eq!(r["reached"], json!(1.0));
    assert_eq!(r["sketch"]["solved"], json!(true));
    assert_eq!(r["sketch"]["dof"], json!(2), "the degrees of freedom from before the drag");
    // The sketch shows its new place; the extrude keeps its last result.
    assert!(corner(&s).dist(solidcraft_geom::Vec2::new(4.0, 5.0)) < 1e-9);
    let result = s.model.result(extrude).expect("the extrude's result");
    assert!(result.stale);
    assert!(s.model.state().bodies.first().expect("a body").body.same(&body), "the body is not rebuilt");
}

/// A frame copies the design once, to edit it: its undo step holds the design as it was, and a
/// second copy for storing the edited one would double the work on a large design.
#[test]
fn a_live_frame_copies_the_design_once_and_its_undo_step_keeps_the_design_before() {
    let (mut s, _) = extruded();
    let before = s.doc.clone();
    let undo = s.history.len();
    crate::tests::doc_copies::take();
    frames(&mut s, 1, true);
    assert_eq!(crate::tests::doc_copies::take(), 0, "the edited design is stored, not copied again");
    assert!(corner(&s).dist(solidcraft_geom::Vec2::new(4.0, 5.0)) < 1e-9);
    assert_eq!(s.history.len(), undo + 1);
    assert!(s.history.last().is_some_and(|u| std::sync::Arc::ptr_eq(&u.doc, &before)), "undo goes back to the design before");
    run(&mut s, "edit.undo", json!({}));
    assert!(std::sync::Arc::ptr_eq(&s.doc, &before));
}

#[test]
fn the_drag_end_evaluates_once_to_the_scripted_drags_result() {
    let (mut live, extrude) = extruded();
    let (mut scripted, _) = extruded();
    frames(&mut live, 3, true);
    reset(&mut live);
    let end = run(&mut live, "sketch.drag_end", json!({}));
    assert_eq!(live.evaluations, 1, "one full evaluation at the end");
    assert_eq!(solve_counts::take(), (0, 0));
    assert_eq!(end["sketch"], json!({"solved": true, "dof": 2}));
    assert!(!live.model.is_stale());
    frames(&mut scripted, 3, false);
    assert_eq!(*live.doc, *scripted.doc, "the same design as dragging without `live`");
    let (a, b) = (live.model.state(), scripted.model.state());
    let (sa, sb) = (&a.sketches[0], &b.sketches[0]);
    assert_eq!(sa.sketch, sb.sketch);
    assert_eq!((sa.report.dof, &sa.report.point_determined), (sb.report.dof, &sb.report.point_determined));
    assert!(body_contains(&live, Vec3::new(2.5, 6.0, 2.5)) && !body_contains(&live, Vec3::new(22.5, 6.0, 2.5)), "the body followed");
    assert!(live.model.result(extrude).is_some_and(|r| !r.stale && r.error.is_none()));
    // One undo step for the gesture is the UI's business; each frame is still undoable.
    run(&mut live, "edit.undo", json!({}));
    assert!(corner(&live).dist(solidcraft_geom::Vec2::new(3.0, 5.0)) < 1e-9);
}

#[test]
fn an_abandoned_live_drag_is_evaluated_before_the_next_command() {
    let (mut s, extrude) = extruded();
    frames(&mut s, 2, true);
    assert!(s.model.is_stale());
    reset(&mut s);
    // A command that changes nothing still sees (and leaves) a fully evaluated model.
    run(&mut s, "sketch.solve", json!({}));
    assert_eq!(s.evaluations, 1);
    assert!(!s.model.is_stale());
    assert!(s.model.result(extrude).is_some_and(|r| !r.stale));
    assert!(body_contains(&s, Vec3::new(3.5, 6.0, 2.5)));
    // Leaving the sketch does too.
    frames(&mut s, 1, true);
    run(&mut s, "sketch.finish", json!({}));
    assert!(!s.model.is_stale());
}

#[test]
fn a_scripted_drag_still_evaluates_the_whole_model() {
    let (mut s, extrude) = extruded();
    reset(&mut s);
    let r = frames(&mut s, 1, false).remove(0);
    assert_eq!(s.evaluations, 1);
    assert_eq!(solve_counts::take(), (2, 1), "the held solve, then the sketch's own solve with its analysis");
    assert_eq!(r["sketch"], json!({"solved": true, "dof": 2}));
    assert!(!s.model.is_stale());
    assert!(s.model.result(extrude).is_some_and(|r| !r.stale));
    assert!(body_contains(&s, Vec3::new(4.5, 6.0, 2.5)));
}

#[test]
fn a_drag_reports_how_far_the_geometry_followed() {
    for live in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 10], [40, 10]]}));
        run(&mut s, "sketch.line", json!({"points": [[10, 30], [40, 30]]}));
        run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
        run(&mut s, "sketch.dimension", json!({"entities": ["l1"], "value": 30}));
        // The grabbed midpoint follows a 15 mm circle around the fixed start.
        let r = run(&mut s, "sketch.drag", json!({"entity": "l1", "from": [25, 10], "to": [25, 20], "live": live}));
        let at = crate::params::vec2(&r["at"]).expect("`at`");
        let base = solidcraft_geom::Vec2::new(10.0, 10.0);
        let expected = base + solidcraft_geom::Vec2::new(15.0, 10.0).normalized().unwrap() * 15.0;
        assert!(at.dist(expected) < 1e-4, "{r}");
        assert!(at.dist((pt(&s, "l1.start") + pt(&s, "l1.end")) * 0.5) < 1e-6);
        assert!((r["reached"].as_f64().unwrap() - (at.y - 10.0) / 10.0).abs() < 1e-6);
        assert!((pt(&s, "l1.end").dist(base) - 30.0).abs() < 1e-6);
        let r = run(&mut s, "sketch.drag", json!({"entity": "l2", "from": [25, 30], "to": [25, 40]}));
        assert_eq!((&r["reached"], &r["at"]), (&json!(1.0), &json!([25.0, 40.0])), "{r}");
        let r = run(&mut s, "sketch.drag", json!({"entity": "l1.start", "from": [10, 10], "to": [0, 0], "live": true}));
        assert_eq!(r["reached"], json!(0.0));
    }
}

#[test]
fn a_live_drag_of_a_lone_sketch_shows_it_without_evaluating() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [25, 20]}));
    reset(&mut s);
    frames(&mut s, 2, true);
    assert_eq!(s.evaluations, 0);
    assert_eq!(solve_counts::take(), (2, 0));
    // The free rectangle's corner moves alone; its profile follows.
    assert!(corner(&s).dist(solidcraft_geom::Vec2::new(3.0, 5.0)) < 1e-9);
    let st = s.model.state();
    let profile = st.sketches[0].profiles.first().expect("a profile");
    assert!((profile.area - 22.0 * 15.0).abs() < 1e-9, "{}", profile.area);
    run(&mut s, "sketch.drag_end", json!({}));
    assert_eq!(s.evaluations, 1);
}

/// Frames of a UI drag of `entity` grabbed at `grab`, the cursor moving by `moves` in turn: each
/// frame's `from` is the previous frame's `at`, as the viewport does.
fn ui_frames(s: &mut Session, entity: &str, grab: [f64; 2], moves: &[[f64; 2]]) {
    let (mut from, mut cursor) = (grab, grab);
    for m in moves {
        cursor = [cursor[0] + m[0], cursor[1] + m[1]];
        let r = run(s, "sketch.drag", json!({"entity": entity, "from": from, "to": cursor, "live": true}));
        let at = crate::params::vec2(&r["at"]).expect("`at`");
        from = [at.x, at.y];
    }
}

#[test]
fn the_grab_follows_the_points_that_move() {
    // With one end fixed, the free end moves twice as far as its grabbed midpoint.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [30, 10]]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities": ["l1.start"]}));
    ui_frames(&mut s, "l1", [20.0, 10.0], &[[0.0, 1.0]; 6]);
    assert!(pt(&s, "l1.end").dist(solidcraft_geom::Vec2::new(30.0, 22.0)) < 1e-6, "{:?}", pt(&s, "l1.end"));
    assert!(pt(&s, "l1.start").dist(solidcraft_geom::Vec2::new(10.0, 10.0)) < 1e-12);
}

#[test]
fn a_query_during_a_live_drag_does_not_evaluate_the_model() {
    let (mut s, _) = extruded();
    frames(&mut s, 1, true);
    reset(&mut s);
    for (id, p) in
        [("parameters.complete", json!({"prefix": "d"})), ("parameters.list", json!({})), ("expr.evaluate", json!({"expression": "d1 * 2"}))]
    {
        run(&mut s, id, p);
    }
    assert_eq!(s.evaluations, 0);
    assert!(s.model.is_stale(), "the drag goes on");
    frames(&mut s, 1, true);
    assert_eq!(s.evaluations, 0);
}

fn sketch_of(s: &Session) -> solidcraft_sketch::Sketch {
    s.active_sketch.and_then(|i| s.doc.sketch(i).ok()).cloned().expect("sketch mode")
}

#[test]
fn a_live_frame_solves_the_dimensions_current_values() {
    let (mut s, _) = extruded();
    // The bottom side's parameter now says 25 while the sketch still holds 20 mm.
    let sk = sketch_of(&s);
    let l1 = sk.curve_index("l1").expect("l1");
    let param = sk
        .constraints
        .iter()
        .find(|c| matches!(c.kind, solidcraft_sketch::ConstraintKind::Length { l, .. } if l == l1))
        .and_then(|c| c.param.clone())
        .expect("l1's dimension");
    if let Some(p) = s.doc_mut().params.iter_mut().find(|p| p.name == param) {
        p.expr = "25".into();
    }
    let r = frames(&mut s, 1, true).remove(0);
    assert_eq!(r["sketch"]["solved"], json!(true));
    let (a, b) = (pt(&s, "l1.start"), pt(&s, "l1.end"));
    assert!((a.dist(b) - 25.0).abs() < 1e-6, "l1 is {} mm", a.dist(b));
}

#[test]
fn a_live_frame_swaps_only_the_dragged_sketch() {
    let (mut s, _) = extruded();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    run(&mut s, "sketch.circle.center", json!({"center": [50, 50], "radius": 4}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.edit", json!({"sketch": "Sketch1"}));
    let id = s.active_sketch.expect("sketch mode");
    let other = |s: &Session| s.model.state().sketches.iter().find(|x| x.feature != id).cloned().expect("the other sketch");
    let before = other(&s);
    frames(&mut s, 1, true);
    assert!(std::sync::Arc::ptr_eq(&before, &other(&s)), "the other sketch is shared, not copied");
    // Later frames change the state they own in place.
    let state = std::sync::Arc::as_ptr(&s.model.state());
    frames(&mut s, 1, true);
    assert_eq!(state, std::sync::Arc::as_ptr(&s.model.state()));
}

#[test]
fn letting_go_on_a_snap_or_undoing_evaluates_once() {
    let (mut s, _) = extruded();
    run(&mut s, "sketch.point", json!({"point": [-10, 5], "connect": false}));
    let point = sketch_of(&s).points.last().map(|p| p.id.clone()).expect("the point");
    frames(&mut s, 3, true);
    reset(&mut s);
    let snap = json!({"command": "sketch.constraint.coincident", "params": {"a": "l1.start", "b": point}});
    let r = run(&mut s, "sketch.drag_end", json!({ "constraint": snap }));
    assert_eq!(s.evaluations, 1, "{r}");
    assert!(r["constraint"].get("error").is_none(), "{r}");
    assert!(!s.model.is_stale());
    // Undo after an abandoned drag.
    frames(&mut s, 1, true);
    reset(&mut s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.evaluations, 1);
    assert!(!s.model.is_stale());
    // A snap that over-constrains the sketch is not kept; the drag still ends.
    frames(&mut s, 1, true);
    let bad = json!({"command": "sketch.constraint.coincident", "params": {"a": "l1.start", "b": "l1.end"}});
    let r = run(&mut s, "sketch.drag_end", json!({ "constraint": bad }));
    assert!(r["constraint"]["error"].is_string(), "{r}");
    assert!(!s.model.is_stale());
}

#[test]
fn a_drag_with_the_rest_of_a_selection_moves_it_as_one() {
    // A vertical line and a 5 mm tail hanging off its top that keeps only its length (#2621).
    let start = || {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 10], [10, 50], [14, 47]]}));
        run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line": "l1"}));
        run(&mut s, "sketch.dimension", json!({"entities": ["l2"], "value": 5}));
        run(&mut s, "sketch.circle.center", json!({"center": [40, 10], "radius": 3, "id": "c1"}));
        s
    };
    let v = solidcraft_geom::Vec2::new;
    for live in [false, true] {
        // Dragged alone, the line leaves the tail's free end where it was as far as it can.
        let mut s = start();
        run(&mut s, "sketch.drag", json!({"entity": "l1", "from": [10, 30], "to": [20, 20], "live": live}));
        assert!(pt(&s, "l2.end").dist(v(24.0, 37.0)) > 1.0, "{:?}", pt(&s, "l2.end"));
        // Dragged with the tail and the circle, all of it slides by the cursor's move.
        let mut s = start();
        let r = run(&mut s, "sketch.drag", json!({"entity": "l1", "with": ["l2", "c1"], "from": [10, 30], "to": [20, 20], "live": live}));
        assert_eq!((&r["reached"], &r["at"]), (&json!(1.0), &json!([20.0, 20.0])), "{r}");
        for (p, at) in [("l1.start", v(20.0, 0.0)), ("l1.end", v(20.0, 40.0)), ("l2.end", v(24.0, 37.0)), ("c1.center", v(50.0, 0.0))] {
            assert!(pt(&s, p).dist(at) < 1e-6, "{p}: {:?}", pt(&s, p));
        }
        let sk = s.doc.sketch(s.active_sketch.expect("sketch mode")).expect("the sketch");
        assert_eq!(sk.radius(sk.curve_index("c1").expect("the circle")), Some(3.0));
    }
    let mut s = start();
    for with in [json!("l2"), json!([7]), json!(["nope"])] {
        assert!(s.execute("sketch.drag", &json!({"entity": "l1", "with": with, "from": [10, 30], "to": [20, 20]})).is_err(), "{with}");
    }
}

/// A 5 x 5 square drawn on the top face of a 30 x 20 x 10 box, the face's edges projected into
/// its sketch; returns the session and a corner of the square (its id and where it is).
fn on_a_face() -> (Session, String, solidcraft_geom::Vec2) {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 30, "width": 20, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [15, 10, 10]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [10, 10]}));
    let sk = sketch_of(&s);
    assert!(!sk.links.is_empty(), "the face's edges are projected");
    let corner = sk.curves.iter().find_map(|c| match c.kind {
        solidcraft_sketch::CurveKind::Line { a, .. } if c.link.is_none() => sk.points.get(a).map(|p| (p.id.clone(), p.pos)),
        _ => None,
    });
    let (id, at) = corner.expect("a drawn line");
    (s, id, at)
}

/// One live frame moving `point` from `at` by `dx`; returns where it went.
fn live_frame(s: &mut Session, point: &str, at: solidcraft_geom::Vec2, dx: f64) -> solidcraft_geom::Vec2 {
    let to = [at.x + dx, at.y];
    run(s, "sketch.drag", json!({"entity": point, "from": [at.x, at.y], "to": to, "live": true}));
    solidcraft_geom::Vec2::new(to[0], to[1])
}

#[test]
fn a_live_gesture_projects_a_face_sketchs_links_once() {
    // Every frame projected the face's edges again and solved the sketch once more for them,
    // which on a face with many edges made each frame take a large fraction of a second (#3222).
    let (mut s, point, mut at) = on_a_face();
    refresh_count::take();
    for _ in 0..5 {
        at = live_frame(&mut s, &point, at, 0.5);
    }
    assert_eq!(refresh_count::take(), 1, "the gesture's first frame projects the links, the rest reuse them");
    assert_eq!(pt(&s, &point), at, "the corner follows the drag");
    assert!(sketch_of(&s).links.iter().all(|l| !l.lost), "the links are kept");
    run(&mut s, "sketch.drag_end", json!({}));
    assert_eq!(pt(&s, &point), at);

    // Anything run between two frames (another command, an undo) may have changed the model the
    // links come from: the next frame projects them again.
    refresh_count::take();
    at = live_frame(&mut s, &point, at, 0.5);
    run(&mut s, "sketch.solve", json!({}));
    live_frame(&mut s, &point, at, 0.5);
    assert_eq!(refresh_count::take(), 2);
    run(&mut s, "edit.undo", json!({}));
    let at = pt(&s, &point);
    refresh_count::take();
    live_frame(&mut s, &point, at, 0.5);
    assert_eq!(refresh_count::take(), 1);
}
