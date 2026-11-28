//! A right-button drag on a free part turns it about its centre.
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::doc::apply_point;
use solidcraft_engine::geom::prelude::*;

use crate::scenario::Harness;

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
    let [x, y] = h.at(&json!({"world": [p.x, p.y, p.z]})).expect("on screen");
    egui::pos2(x as f32, y as f32)
}

/// Where the placement `path` puts the point `local` of its part.
fn world(h: &Harness, path: &[u64], local: Vec3) -> Vec3 {
    apply_point(&h.app.session.occurrence_placement(path).expect("placement").world, local)
}

/// A grounded base and a free 10 mm cube at (20..30, 0..10, 0..10), seen from the top, in the
/// default mouse mode (right drag orbits), with Right drag turns parts on: (harness, base, free).
fn two_parts() -> (Harness, u64, u64) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.right_drag_turns_parts = true;
    let mut occurrences = Vec::new();
    for (name, corner) in [("Base", [-5, -5, 0]), ("Free", [20, 0, 0])] {
        run(&mut h, "component.activate", json!({"component": "root"}));
        occurrences.push(run(&mut h, "component.create", json!({"name": name}))["occurrence"].as_u64().expect("occurrence"));
        run(&mut h, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner}));
    }
    run(&mut h, "component.activate", json!({"component": "root"}));
    h.call("ui.view", json!({"view": "top"}));
    h.app.cam.distance = 200.0;
    h.frames(3);
    (h, occurrences[0], occurrences[1])
}

fn play(h: &mut Harness) {
    while !h.app.synthetic.is_empty() {
        h.frame();
    }
    h.frames(2);
}

fn right(h: &mut Harness, pos: egui::Pos2, pressed: bool) {
    h.app.synthetic.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
}

/// Right press at `a`, move to `b` in 12 frames, still held.
fn press_and_move(h: &mut Harness, a: egui::Pos2, b: egui::Pos2) {
    h.app.synthetic.push(egui::Event::PointerMoved(a));
    right(h, a, true);
    h.app.synthetic.extend((1..=12).map(|k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 12.0))));
    play(h);
}

fn view(h: &Harness) -> (f64, f64, f64) {
    (h.app.cam.yaw, h.app.cam.pitch, h.app.cam.roll)
}

/// Dragged sideways from the top, the part turns about the view's vertical through its centre
/// (world Y here): the centre stays, the near top corner goes down, the camera stays, and the
/// whole turn is one undo step.
#[test]
fn a_right_drag_on_a_free_part_turns_it_about_its_centre() {
    let (mut h, base, free) = two_parts();
    let (cam, undo) = (view(&h), h.app.session.undo.len());
    let centre = Vec3::new(25.0, 5.0, 5.0);
    let corner = Vec3::new(30.0, 0.0, 10.0);
    let a = screen(&mut h, Vec3::new(25.0, 5.0, 10.0));
    press_and_move(&mut h, a, a + egui::vec2(90.0, 0.0));
    assert!(crate::occurrence_rotate::active(&h.app), "the part is turning");
    right(&mut h, a + egui::vec2(90.0, 0.0), false);
    play(&mut h);
    assert!(h.app.viewport.occurrence_turn.is_none());
    assert_eq!(view(&h), cam, "the camera did not orbit");
    assert!(h.app.viewport.context_menu.is_none(), "a drag opens no menu");
    // Frame by frame, from pointer moves in f32: equal to within rounding, not exactly.
    assert!(world(&h, &[free], centre).dist(centre) < 1e-4, "the centre stays: {:?}", world(&h, &[free], centre));
    let moved = world(&h, &[free], corner);
    assert!((moved.y - corner.y).abs() < 1e-4, "a turn about Y keeps Y: {moved:?}");
    assert!(moved.dist(corner) > 1.0, "it turned: {moved:?}");
    assert!(world(&h, &[base], Vec3::ZERO).dist(Vec3::ZERO) < 1e-9, "the grounded part stays");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert!(h.app.session.pending_moves.is_empty(), "the position is captured");
    run(&mut h, "edit.undo", json!({}));
    assert!(world(&h, &[free], corner).dist(corner) < 1e-9);
}

/// Off by default: a right drag on a free part orbits the view and leaves the part alone.
#[test]
fn with_the_setting_off_a_right_drag_on_a_part_orbits() {
    assert!(!crate::prefs::Prefs::default().right_drag_turns_parts, "off by default");
    let (mut h, _, free) = two_parts();
    h.app.preferences.right_drag_turns_parts = false;
    let cam = view(&h);
    let a = screen(&mut h, Vec3::new(25.0, 5.0, 10.0));
    press_and_move(&mut h, a, a + egui::vec2(90.0, 0.0));
    assert!(h.app.viewport.occurrence_turn.is_none());
    right(&mut h, a + egui::vec2(90.0, 0.0), false);
    play(&mut h);
    assert_ne!(view(&h), cam, "the view orbits");
    assert!(world(&h, &[free], Vec3::ZERO).dist(Vec3::ZERO) < 1e-9, "the part stays");
}

/// Escape mid-turn puts the part back and leaves no undo step.
#[test]
fn escape_during_a_turn_puts_the_part_back() {
    let (mut h, _, free) = two_parts();
    let undo = h.app.session.undo.len();
    let corner = Vec3::new(30.0, 0.0, 10.0);
    let a = screen(&mut h, Vec3::new(25.0, 5.0, 10.0));
    press_and_move(&mut h, a, a + egui::vec2(0.0, 60.0));
    assert!(world(&h, &[free], corner).dist(corner) > 1.0, "it turned");
    assert!(crate::toolbar::cancel_unfinished(&mut h.app));
    right(&mut h, a + egui::vec2(0.0, 60.0), false);
    play(&mut h);
    assert!(world(&h, &[free], corner).dist(corner) < 1e-9);
    assert_eq!(h.app.session.undo.len(), undo);
}

/// On the grounded part or empty space the right drag orbits as before, and a right click on a
/// free part still opens the context menu.
#[test]
fn elsewhere_the_right_button_works_as_before() {
    let (mut h, base, free) = two_parts();
    for at in [Vec3::new(0.0, 0.0, 10.0), Vec3::new(-40.0, 40.0, 0.0)] {
        let cam = view(&h);
        let a = screen(&mut h, at);
        press_and_move(&mut h, a, a + egui::vec2(80.0, 0.0));
        assert!(h.app.viewport.occurrence_turn.is_none(), "{at:?}");
        right(&mut h, a + egui::vec2(80.0, 0.0), false);
        play(&mut h);
        assert_ne!(view(&h), cam, "{at:?}: the view orbits");
        assert!(world(&h, &[base], Vec3::ZERO).dist(Vec3::ZERO) < 1e-9 && world(&h, &[free], Vec3::ZERO).dist(Vec3::ZERO) < 1e-9);
        h.call("ui.view", json!({"view": "top"}));
        h.frames(3);
    }
    let a = screen(&mut h, Vec3::new(25.0, 5.0, 10.0));
    h.app.synthetic.push(egui::Event::PointerMoved(a));
    right(&mut h, a, true);
    right(&mut h, a, false);
    play(&mut h);
    assert!(h.app.viewport.context_menu.is_some(), "a click opens the menu");
    assert!(world(&h, &[free], Vec3::ZERO).dist(Vec3::ZERO) < 1e-9, "and turns nothing");
}
