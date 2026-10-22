//! Trim by dragging: with Trim on, a press and drag draws a path, the pieces it crosses light
//! up, and letting go trims them all (`sketch.trim.path`) in one undo step. A click still trims
//! the one piece under it.

use serde_json::{Value, json};
use solidcraft_engine::geom::Vec2;
use solidcraft_engine::sketch::Shape;

use crate::scenario::Harness;

/// Two horizontal lines crossed by a vertical one, seen from the top, with Trim on. Returns the
/// vertical line's id.
fn ladder() -> (Harness, String) {
    let mut h = Harness::new();
    h.app.home.open = false;
    run(&mut h, "sketch.create", json!({"plane": "XY"}));
    run(&mut h, "sketch.line", json!({"points": [[0, 0], [20, 0]], "connect": false}));
    run(&mut h, "sketch.line", json!({"points": [[0, 10], [20, 10]], "connect": false}));
    let v = run(&mut h, "sketch.line", json!({"points": [[10, -5], [10, 15]], "connect": false}))["curves"][0].as_str().unwrap().to_string();
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 60.0;
    h.frames(3);
    h.call("ui.start", json!({"command": "sketch.trim"}));
    h.frames(2);
    (h, v)
}

#[test]
fn dragging_across_pieces_trims_them_all_in_one_undo_step() {
    let (mut h, v) = ladder();
    let before = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    // A zigzag across the vertical line's middle piece, three times, then down across the
    // lower line's right half.
    let a = screen(&mut h, 5.0, 8.0);
    let b = screen(&mut h, 15.0, 7.0);
    let c = screen(&mut h, 5.0, 5.0);
    let d = screen(&mut h, 15.0, -3.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 8));
    path.extend(steps(b, c, 8));
    path.extend(steps(c, d, 8));
    press_and_move(&mut h, &path);

    // While dragging: nothing has changed yet, and the two pieces to go light up (the middle
    // piece once).
    assert!(h.app.viewport.trim_drag);
    assert_eq!(h.app.session.doc.to_json(), before, "a drag changes nothing before it is let go");
    assert_eq!(h.app.session.undo.len(), undo);
    let lit = super::TRIM_REMOVED.with(|r| r.borrow().clone());
    assert_eq!(lit.len(), 2, "{lit:?}");

    release(&mut h, d);
    assert!(!h.app.viewport.trim_drag);
    assert!(super::TRIM_REMOVED.with(|r| r.borrow().is_empty()));
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    // The vertical line keeps its ends; the lower line its left half.
    let sk = sketch(&h);
    let rest: Vec<Shape> = (0..sk.curves.len()).filter_map(|ci| sk.shape(ci)).collect();
    assert_eq!(rest.len(), 4, "{rest:?}");
    assert!(rest.iter().all(|s| s.dist(Vec2::new(10.0, 5.0)) > 1e-3 && s.dist(Vec2::new(15.0, 0.0)) > 1e-3), "{rest:?}");
    let lower = sk.shape(sk.curve_index(&v).unwrap()).unwrap();
    assert!(matches!(lower, Shape::Line { a, b } if a.dist(Vec2::new(10.0, -5.0)) < 1e-6 && b.dist(Vec2::new(10.0, 0.0)) < 1e-6), "{lower:?}");
    // Trim stays on for the next piece.
    assert_eq!(h.app.tool.as_ref().map(|t| t.cmd), Some("sketch.trim"));

    run(&mut h, "edit.undo", json!({}));
    assert_eq!(h.app.session.doc.to_json(), before);
}

#[test]
fn a_click_still_trims_one_piece_and_an_empty_drag_nothing() {
    let (mut h, v) = ladder();
    let undo = h.app.session.undo.len();
    // A drag that crosses nothing leaves the design alone.
    let a = screen(&mut h, 1.0, 3.0);
    let b = screen(&mut h, 8.0, 7.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 6));
    press_and_move(&mut h, &path);
    release(&mut h, b);
    assert_eq!(h.app.session.undo.len(), undo);

    // A click on the vertical line's middle piece trims just that piece.
    h.step(&json!({"click": {"sketch": [10, 5]}})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let sk = sketch(&h);
    assert_eq!(sk.curves.len(), 4);
    assert!((0..sk.curves.len()).all(|ci| sk.shape(ci).is_some_and(|s| s.dist(Vec2::new(10.0, 5.0)) > 1e-3)));
    assert!(sk.curve_index(&v).is_some());
}

#[test]
fn a_click_that_moves_along_a_piece_still_trims_it() {
    // A press that moves on the way (#2448) beside the vertical line's middle piece, crossing
    // nothing: the release is the click, and trims the piece under it.
    let (mut h, _) = ladder();
    let undo = h.app.session.undo.len();
    let a = screen(&mut h, 10.0, 3.0) + egui::vec2(3.0, 0.0);
    let b = screen(&mut h, 10.0, 7.0) + egui::vec2(3.0, 0.0);
    let mut path = vec![a];
    path.extend(steps(a, b, 6));
    press_and_move(&mut h, &path);
    release(&mut h, b);
    assert_eq!(h.app.session.undo.len(), undo + 1, "one trim");
    let sk = sketch(&h);
    assert_eq!(sk.curves.len(), 4);
    assert!((0..sk.curves.len()).all(|ci| sk.shape(ci).is_some_and(|s| s.dist(Vec2::new(10.0, 5.0)) > 1e-3)));
}

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    match h.app.run(id, p.clone()) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(h: &Harness) -> solidcraft_engine::sketch::Sketch {
    let sid = h.app.session.active_sketch.expect("sketch mode");
    h.app.session.world_state().unwrap().sketch(sid).expect("the sketch").sketch.clone()
}

fn screen(h: &mut Harness, x: f64, y: f64) -> egui::Pos2 {
    let [sx, sy] = h.at(&json!({"sketch": [x, y]})).expect("on screen");
    egui::pos2(sx as f32, sy as f32)
}

fn steps(a: egui::Pos2, b: egui::Pos2, n: usize) -> Vec<egui::Pos2> {
    (1..=n).map(|i| a + (b - a) * (i as f32 / n as f32)).collect()
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

fn play(h: &mut Harness) {
    while !h.app.synthetic.is_empty() {
        h.frame();
    }
    h.frames(2);
}
