//! Dragging sketch geometry in the viewport: live frames re-solve the sketch only, letting go
//! brings the model up to date in the drag's one undo step, and the grab stays on the geometry.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec2;

use crate::scenario::Harness;
use crate::test_support::play;

#[test]
fn text_move_drag_is_one_editable_unit_and_escape_restores_the_gesture() {
    for cancel in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        run(&mut h, "sketch.create", json!({"plane":"XY"}));
        run(&mut h, "sketch.text", json!({"text":"II","at":[0,0],"height":10}));
        run(&mut h, "sketch.line", json!({"points":[[15,0],[15,10]],"connect":false}));
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 35.0;
        h.frames(3);
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        let sk = h.app.session.doc.sketch(1).unwrap();
        let line = sk.curves.last().unwrap().id.clone();
        let neighbor = point(&h, &format!("{line}.start"));
        let polyline = sk.polyline(0);
        let at = (polyline[0] + polyline[1]) * 0.5;
        let a = screen(&mut h, at.x, at.y);
        let b = screen(&mut h, at.x + 3.0, at.y + 2.0);
        let mut path = vec![a];
        path.extend(steps(a, b, 6));
        press_and_move(&mut h, &path);
        assert!(h.app.viewport.text_drag.is_some());
        assert_ne!(h.app.session.doc.to_json(), before);
        if cancel {
            h.step(&json!({"key":"Escape"})).unwrap();
        }
        release(&mut h, b);
        assert!(h.app.viewport.text_drag.is_none());
        assert_eq!(h.app.session.active_sketch, Some(1));
        assert!(point(&h, &format!("{line}.start")).dist(neighbor) < 1e-9);
        if cancel {
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!(h.app.session.undo.len(), undo);
        } else {
            assert_eq!(h.app.session.undo.len(), undo + 1);
            let sk = h.app.session.doc.sketch(1).unwrap();
            let solidcraft_engine::sketch::LinkSource::Text { at, .. } = &sk.links[0].source else { panic!() };
            assert!(at.dist(Vec2::new(3.0, 2.0)) < 0.02, "{at:?}");
            run(&mut h, "edit.undo", json!({}));
            assert_eq!(h.app.session.doc.to_json(), before);
        }
    }
}

fn run(h: &mut Harness, id: &str, p: Value) {
    if let Err(e) = h.app.run(id, p.clone()) {
        panic!("{id} {p}: {e}");
    }
}

fn screen(h: &mut Harness, x: f64, y: f64) -> egui::Pos2 {
    let [sx, sy] = h.at(&json!({"sketch": [x, y]})).expect("on screen");
    egui::pos2(sx as f32, sy as f32)
}

fn point(h: &Harness, id: &str) -> Vec2 {
    let sid = h.app.session.active_sketch.expect("sketch mode");
    let st = h.app.session.world_state();
    let sk = &st.sketch(sid).expect("the sketch").sketch;
    sk.resolve_point(id).and_then(|i| sk.point(i)).unwrap_or_else(|| panic!("no point {id}"))
}

/// Press at `path[0]` and move through the rest, one frame per event.
fn press_and_move(h: &mut Harness, path: &[egui::Pos2]) {
    let Some(&a) = path.first() else { return };
    h.app.synthetic.push(egui::Event::PointerMoved(a));
    h.app.synthetic.push(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.app.synthetic.extend(path.iter().skip(1).map(|&p| egui::Event::PointerMoved(p)));
    play(h);
}

fn release(h: &mut Harness, at: egui::Pos2) {
    h.app.synthetic.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    play(h);
}

/// Points from `a` to `b` in `n` equal moves (without `a`).
fn steps(a: egui::Pos2, b: egui::Pos2, n: usize) -> Vec<egui::Pos2> {
    (1..=n).map(|k| a + (b - a) * (k as f32 / n as f32)).collect()
}

fn four_bar() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    run(&mut h, "sketch.create", json!({"plane":"XZ"}));
    let x = -10.0 + (1600.0 - 400.0 + 52.0 * 52.0) / 104.0;
    let y = -(1600.0_f64 - (x + 10.0) * (x + 10.0)).sqrt();
    run(&mut h, "sketch.line", json!({"points":[[0,0],[-10,0],[x,y],[42,0]],"ids":["crank","coupler","rocker"],"infer":false}));
    run(&mut h, "sketch.line", json!({"points":["crank.start","rocker.end"],"ids":["ground"],"infer":false}));
    for (line, length) in [("crank", 10), ("coupler", 40), ("rocker", 20), ("ground", 42)] {
        run(&mut h, "sketch.dimension", json!({"entities":[line],"value":length}));
    }
    run(&mut h, "sketch.constraint.horizontal_vertical", json!({"line":"ground"}));
    run(&mut h, "sketch.options", json!({"snap":false,"show_dimensions":false,"show_constraints":false}));
    h.settle();
    h
}

#[test]
fn a_four_bar_edge_follows_a_stationary_pointer_and_keeps_the_grab() {
    let mut h = four_bar();
    let a = screen(&mut h, -7.0, 0.0);
    let b = screen(&mut h, -7.0, 11.2);
    let undo = h.app.session.undo.len();
    press_and_move(&mut h, &[a, b]);
    assert_eq!(h.app.viewport.point_drag.as_ref().map(|(id, _, _)| id.as_str()), Some("crank"), "grabbed the edge, not an endpoint");
    h.frames(8);
    let nearest = Vec2::new(-10.0, 16.0).normalized().unwrap() * 10.0;
    assert!(point(&h, "crank.end").dist(nearest) < 0.05, "{:?}", point(&h, "crank.end"));
    release(&mut h, b);
    assert!(point(&h, "crank.end").dist(nearest) < 0.05);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    run(&mut h, "edit.undo", json!({}));
    assert!(point(&h, "crank.end").dist(Vec2::new(-10.0, 0.0)) < 1e-7);
}

#[test]
fn a_four_bar_release_uses_the_final_pointer_position() {
    let mut h = four_bar();
    let a = screen(&mut h, -10.0, 0.0);
    let b = screen(&mut h, -10.0, 3.0);
    let end = screen(&mut h, -10.0, 16.0);
    press_and_move(&mut h, &[a, b]);
    h.frames(8);
    assert_eq!(h.app.viewport.point_drag.as_ref().map(|(id, _, _)| id.as_str()), Some("crank.end"));
    release(&mut h, end);
    let nearest = Vec2::new(-10.0, 16.0).normalized().unwrap() * 10.0;
    assert!(point(&h, "crank.end").dist(nearest) < 0.05, "release left {:?}", point(&h, "crank.end"));
    assert!(!h.app.session.model.is_stale());
}

#[test]
fn a_four_bar_edge_grab_keeps_its_pick_offset_when_rotating() {
    let mut h = four_bar();
    let a = screen(&mut h, -7.0, 0.25);
    let b = screen(&mut h, -7.0, 11.2);
    press_and_move(&mut h, &[a, b]);
    assert_eq!(h.app.viewport.point_drag.as_ref().map(|(id, _, _)| id.as_str()), Some("crank"));
    h.frames(8);
    release(&mut h, b);
    let local = Vec2::new(0.7, -0.025);
    let target = Vec2::new(-7.0, 11.2);
    let closest = target * (10.0 * local.len() / target.len());
    let end = (closest * local.x - Vec2::new(-closest.y, closest.x) * local.y) * (1.0 / local.dot(local));
    assert!(point(&h, "crank.end").dist(end) < 0.05, "{:?}, expected {end:?}", point(&h, "crank.end"));
}

#[test]
fn a_viewport_drag_solves_the_sketch_live_and_rebuilds_the_model_on_release() {
    let mut h = Harness::new();
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    run(&mut h, "sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [25, 20]}));
    run(&mut h, "sketch.dimension", json!({"entities": ["l1"], "value": 20}));
    run(&mut h, "sketch.dimension", json!({"entities": ["l2"], "value": 15}));
    run(&mut h, "sketch.finish", json!({}));
    run(&mut h, "solid.extrude", json!({"distance": 5}));
    let sketch = h.app.session.doc.features.first().map(|f| f.id).expect("the sketch");
    h.app.edit_sketch(sketch);
    h.settle();
    let undo = h.app.session.undo.len();
    let a = screen(&mut h, 5.0, 5.0);
    let b = screen(&mut h, -3.0, -6.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 8));
    press_and_move(&mut h, &path);
    assert!(h.app.viewport.point_drag.is_some(), "the drag is under way");
    assert!(h.app.session.model.is_stale(), "the frames updated the sketch only");
    assert!(point(&h, "l1.start").dist(Vec2::new(-3.0, -6.0)) < 0.1, "{:?}", point(&h, "l1.start"));
    release(&mut h, b);
    assert!(h.app.viewport.point_drag.is_none());
    assert!(!h.app.session.model.is_stale(), "letting go evaluated the model");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
    assert!(point(&h, "l1.start").dist(Vec2::new(-3.0, -6.0)) < 0.1);
    run(&mut h, "edit.undo", json!({}));
    assert!(point(&h, "l1.start").dist(Vec2::new(5.0, 5.0)) < 1e-9);
}

#[test]
fn a_dragged_line_that_stops_short_stays_under_the_cursor_when_it_can_follow_again() {
    let mut h = Harness::new();
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    // Two 10 mm links from the origin hold the start of a horizontal 10 mm line: the line can
    // slide right until its start is 20 mm from the origin (3 mm).
    let (x, y) = (15.0, 75f64.sqrt());
    run(&mut h, "sketch.line", json!({"points": [[0, 0], [10, 0], [x, y], [x + 10.0, y]]}));
    for l in ["l1", "l2", "l3"] {
        run(&mut h, "sketch.dimension", json!({"entities": [l], "value": 10}));
    }
    run(&mut h, "sketch.constraint.horizontal_vertical", json!({"line": "l3"}));
    // Dimension text uses the bundled font's actual bounds. Grab an exposed interior point
    // of the curve so this tests a curve drag rather than dragging an overlapping value label.
    let grab_offset = [5.0, 6.0, 4.0, 7.0, 3.0]
        .into_iter()
        .find(|offset| crate::dim_view::hit_text(screen(&mut h, x + offset, y)).is_none())
        .expect("an exposed interior point of the line");
    let grab = screen(&mut h, x + grab_offset, y);
    let far = screen(&mut h, x + grab_offset + 6.0, y);
    let back = screen(&mut h, x + grab_offset + 2.0, y);
    let mut path = vec![grab];
    path.extend(steps(grab, far, 8));
    path.extend(steps(far, back, 4));
    press_and_move(&mut h, &path);
    release(&mut h, back);
    // The cursor went 6 mm right, the line only 3; back at 2 mm right it is under it again.
    let start = point(&h, "l3.start");
    assert!(start.dist(Vec2::new(x + 2.0, y)) < 0.1, "{start:?}");
    assert!(!h.app.session.model.is_stale());
}

#[test]
fn associated_spline_offset_hides_fit_nodes_and_follows_a_source_drag_live() {
    let mut h = Harness::new();
    h.app.home.open = false;
    run(&mut h, "sketch.create", json!({"plane":"XY"}));
    run(
        &mut h,
        "sketch.spline.fit_point",
        json!({
            "points":[[-17,15],[0,8],[17,13],[15,0],[5,-7],[-1,-14],[-9,-1],[-21,-1]],
            "periodic":true
        }),
    );
    let output = h.app.run("sketch.offset", json!({"curves":["s1"],"distance":2,"chain":true})).unwrap()["curves"][0].as_str().unwrap().to_owned();
    h.call("ui.view", json!({"view":"top"}));
    h.app.cam.distance = 65.0;
    h.frames(3);
    let sid = h.app.session.active_sketch.unwrap();
    let original = h.app.session.world_state().sketch(sid).unwrap().sketch.clone();
    let source = original.curve_index("s1").unwrap();
    let derived = original.curve_index(&output).unwrap();
    let source_points = original.curves[source].kind.point_ids();
    let derived_points = original.curves[derived].kind.point_ids();
    assert!(derived_points.len() > 100, "exercise the crowded generated fit nodes");
    for &i in &source_points {
        assert!(solidcraft_engine::view::sketch_point_visible(&original, i));
    }
    let snaps = crate::dialogs::SnapPoints::of(&original);
    for &i in &derived_points {
        assert!(!solidcraft_engine::view::sketch_point_visible(&original, i));
        assert!(!snaps.takes(&original.points[i]));
    }
    let p = original.points[derived_points[32]].pos;
    let pos = screen(&mut h, p.x, p.y);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    let hits = crate::viewport::pick_all(&h.app, &proj, pos);
    assert!(hits.iter().any(|hit| matches!(hit, crate::viewport::Hit::SketchCurve {id,..} if id == &output)));
    assert!(
        !hits
            .iter()
            .any(|hit| matches!(hit, crate::viewport::Hit::SketchPoint {id,..} if derived_points.iter().any(|i| original.points[*i].id == *id)))
    );
    let snap = crate::drag_snap::find(&original, &[(source_points[1], p)], &source_points, 0.01);
    assert!(
        !matches!(snap.map(|s| s.target), Some(crate::drag_snap::Target::Point(id)) if derived_points.iter().any(|i| original.points[*i].id == id))
    );
    crate::drag_snap::clear();

    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    let a = screen(&mut h, 0.0, 8.0);
    let b = screen(&mut h, 1.0, 9.0);
    let c = screen(&mut h, 2.0, 10.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 4));
    press_and_move(&mut h, &path);
    for (pos, target) in [(b, Vec2::new(1.0, 9.0)), (c, Vec2::new(2.0, 10.0))] {
        h.app.synthetic.push(egui::Event::PointerMoved(pos));
        play(&mut h);
        assert!(h.app.viewport.point_drag.is_some());
        assert!(h.app.session.model.is_stale(), "the drag has not been released");
        let state = h.app.session.world_state();
        let sk = &state.sketch(sid).unwrap().sketch;
        assert!(sk.points[source_points[1]].pos.dist(target) < 0.1, "source follows the held pointer: {:?}", sk.points[source_points[1]].pos);
        assert_ne!(sk.exact_segs(derived), original.exact_segs(derived), "the offset follows before release");
        assert_eq!(sk.freeform_offsets[0].distance, 2.0);
        sk.validate_freeform_offsets().unwrap();
        let points = sk.curves[derived].kind.point_ids();
        assert!(points.iter().all(|i| !solidcraft_engine::view::sketch_point_visible(sk, *i)));
        for piece in sk.exact_segs(derived).unwrap() {
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let p = piece.point_at(t);
                let distance = sk.exact_segs(source).unwrap().iter().map(|s| s.point_at(s.closest_param(p)).dist(p)).fold(f64::INFINITY, f64::min);
                assert!((distance - 2.0).abs() < 0.002, "live offset normal distance {distance}");
            }
        }
    }
    release(&mut h, c);
    assert!(!h.app.session.model.is_stale());
    assert_eq!(h.app.session.undo.len(), undo + 1);
    run(&mut h, "edit.undo", json!({}));
    assert_eq!(h.app.session.doc.to_json(), before);
    run(&mut h, "edit.redo", json!({}));
    run(&mut h, "sketch.offset.break", json!({"curve":output}));
    let state = h.app.session.world_state();
    let sk = &state.sketch(sid).unwrap().sketch;
    assert!(sk.curves[derived].kind.point_ids().iter().all(|i| solidcraft_engine::view::sketch_point_visible(sk, *i)));
    run(&mut h, "edit.undo", json!({}));
    let state = h.app.session.world_state();
    let sk = &state.sketch(sid).unwrap().sketch;
    assert!(sk.curves[derived].kind.point_ids().iter().all(|i| !solidcraft_engine::view::sketch_point_visible(sk, *i)));
}

#[test]
fn dragging_a_selected_line_moves_the_whole_selection() {
    let mut h = Harness::new();
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    // An L with a chamfered corner and a short tail (#2621): the tail keeps only its length, so
    // dragged on its own the L would leave the tail's free end behind like a string.
    run(&mut h, "sketch.line", json!({"points": [[10, 10], [10, 50], [15, 55], [50, 55], [54, 52]]}));
    run(&mut h, "sketch.constraint.horizontal_vertical", json!({"line": "l1"}));
    run(&mut h, "sketch.constraint.horizontal_vertical", json!({"line": "l3"}));
    run(&mut h, "sketch.dimension", json!({"entities": ["l4"], "value": 5}));
    run(&mut h, "sketch.line", json!({"points": [[80, 10], [80, 30]], "connect": false}));
    let sid = h.app.session.active_sketch.expect("sketch mode");
    let before = h.app.session.world_state().sketch(sid).expect("the sketch").sketch.clone();
    let lines = ["l1", "l2", "l3", "l4"];
    let mut selected: Vec<usize> = lines.iter().flat_map(|l| before.curves[before.curve_index(l).unwrap()].kind.point_ids()).collect();
    selected.sort_unstable();
    selected.dedup();
    h.app.session.selection = lines.iter().map(|l| Sel::SketchCurve { id: (*l).into() }).collect();
    h.app.session.selection.extend(selected.iter().map(|&p| Sel::SketchPoint { id: before.points[p].id.clone() }));
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 150.0;
    h.frames(3);
    let a = screen(&mut h, 10.0, 30.0);
    let b = screen(&mut h, 20.0, 20.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 8));
    press_and_move(&mut h, &path);
    release(&mut h, b);
    let after = &h.app.session.world_state().sketch(sid).expect("the sketch").sketch.clone();
    for &p in &selected {
        let moved = after.points[p].pos - before.points[p].pos;
        assert!(moved.dist(Vec2::new(10.0, -10.0)) < 0.05, "{} moved by {moved:?}", before.points[p].id);
    }
    let other = before.curves[before.curve_index("l5").unwrap()].kind.point_ids();
    for p in other {
        assert!(after.points[p].pos.dist(before.points[p].pos) < 1e-9, "the unselected line stays");
    }
}

#[test]
fn a_window_selected_circle_still_takes_its_radius_from_a_drag_on_its_edge() {
    let mut h = Harness::new();
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    run(&mut h, "sketch.circle.center", json!({"center": [20, 20], "radius": 10, "id": "c1"}));
    // A window around the circle picks it and its centre: not a group to move.
    h.app.session.selection = vec![Sel::SketchCurve { id: "c1".into() }, Sel::SketchPoint { id: "c1.center".into() }];
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 150.0;
    h.frames(3);
    let a = screen(&mut h, 30.0, 20.0);
    let b = screen(&mut h, 35.0, 20.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 6));
    press_and_move(&mut h, &path);
    release(&mut h, b);
    assert!(point(&h, "c1.center").dist(Vec2::new(20.0, 20.0)) < 1e-9);
    let sid = h.app.session.active_sketch.expect("sketch mode");
    let st = h.app.session.world_state();
    let sk = &st.sketch(sid).expect("the sketch").sketch;
    let r = sk.radius(sk.curve_index("c1").expect("the circle")).expect("a radius");
    assert!((r - 15.0).abs() < 0.05, "{r}");
}

/// A circle drawn on the origin and freed from it (its Coincident deleted) moves by its centre:
/// the centre and the fixed origin are equally near the pointer, and the free one wins (#4841).
#[test]
fn a_freed_circle_centre_on_the_origin_drags_instead_of_the_origin() {
    let mut h = Harness::new();
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    run(&mut h, "sketch.circle.center", json!({"center": [0, 0], "radius": 10, "id": "c1"}));
    let sid = h.app.session.active_sketch.expect("sketch mode");
    let ids: Vec<String> = h.app.session.doc.sketch(sid).expect("the sketch").constraints.iter().map(|c| c.id.clone()).collect();
    assert!(!ids.is_empty(), "the centre starts coincident with the origin");
    run(&mut h, "sketch.delete", json!({"entities": ids}));
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 150.0;
    h.frames(3);
    let a = screen(&mut h, 0.0, 0.0);
    let b = screen(&mut h, 15.0, 10.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 6));
    press_and_move(&mut h, &path);
    release(&mut h, b);
    assert!(point(&h, "c1.center").dist(Vec2::new(15.0, 10.0)) < 0.05, "{:?}", point(&h, "c1.center"));
    assert!(point(&h, "origin").dist(Vec2::new(0.0, 0.0)) < 1e-9);
}
