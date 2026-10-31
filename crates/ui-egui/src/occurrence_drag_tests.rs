//! A left-button drag on a free part repositions it (#2804).
use serde_json::{Value, json};
use solidcraft_engine::geom::Vec3;

use crate::scenario::Harness;

fn run(h: &mut Harness, id: &str, p: Value) -> Value {
    h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
    let [x, y] = h.at(&json!({"world": [p.x, p.y, p.z]})).expect("on screen");
    egui::pos2(x as f32, y as f32)
}

/// Where the placement `path` puts its origin.
fn origin(h: &Harness, path: &[u64]) -> Vec3 {
    let w = h.app.session.occurrence_placement(path).expect("placement").world;
    Vec3::new(w[3][0], w[3][1], w[3][2])
}

/// A grounded base (the design's first part) and a free 10 mm cube beside it, seen from the top:
/// (harness, base occurrence, free occurrence).
fn two_parts() -> (Harness, u64, u64) {
    let mut h = Harness::new();
    h.app.home.open = false;
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

/// Press at `a` (holding `modifiers`) and move to `b` in 12 frames, holding the button there.
fn press_and_move(h: &mut Harness, a: egui::Pos2, b: egui::Pos2, modifiers: egui::Modifiers) {
    h.app.synthetic.push(egui::Event::PointerMoved(a));
    h.app.synthetic.push(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers });
    h.app.synthetic.extend((1..=12).map(|k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 12.0))));
    play(h);
}

fn release(h: &mut Harness, b: egui::Pos2) {
    h.app.synthetic.push(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    play(h);
}

/// Dragged by its top face, a free part follows the pointer in the plane facing the view, in one
/// undo step that undo takes back; frames are not echoed one by one.
#[test]
fn dragging_a_free_part_moves_it_in_one_undo_step() {
    let (mut h, base, free) = two_parts();
    assert!(h.app.session.doc.occurrences.iter().any(|o| o.id == base && o.grounded), "the first part is grounded");
    let undo = h.app.session.history.len();
    let (a, b) = (screen(&mut h, Vec3::new(25.0, 5.0, 10.0)), screen(&mut h, Vec3::new(25.0, 35.0, 10.0)));
    press_and_move(&mut h, a, b, Default::default());
    assert!(h.app.viewport.occurrence_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the part, not a selection box");
    release(&mut h, b);
    assert!(h.app.viewport.occurrence_drag.is_none());
    assert!(origin(&h, &[free]).dist(Vec3::new(0.0, 30.0, 0.0)) < 0.5, "{:?}", origin(&h, &[free]));
    assert!(origin(&h, &[base]).dist(Vec3::ZERO) < 1e-9, "the grounded part stays");
    assert_eq!(h.app.session.history.len(), undo + 1, "one undo step for the drag");
    assert_eq!(h.app.session.history.last().map(|s| s.label.as_str()), Some("Drag Occurrence"));
    assert!(h.app.session.pending_moves.is_empty(), "the position is captured");
    assert!(!h.app.session.log.iter().any(|l| l.contains("Drag Occurrence")), "frames are not echoed: {:?}", h.app.session.log);
    run(&mut h, "edit.undo", json!({}));
    assert!(origin(&h, &[free]).dist(Vec3::ZERO) < 1e-9, "{:?}", origin(&h, &[free]));
}

/// A drag on the grounded part, from empty space, or with Shift, Ctrl or Alt held on a free part
/// draws a selection box and moves nothing.
#[test]
fn grounded_parts_empty_space_and_modifier_drags_draw_a_selection_box() {
    let (mut h, base, free) = two_parts();
    let cases = [
        (Vec3::new(0.0, 0.0, 10.0), egui::Modifiers::NONE),
        (Vec3::new(-40.0, 40.0, 0.0), egui::Modifiers::NONE),
        (Vec3::new(25.0, 5.0, 10.0), egui::Modifiers::SHIFT),
        (Vec3::new(25.0, 5.0, 10.0), egui::Modifiers::CTRL),
        (Vec3::new(25.0, 5.0, 10.0), egui::Modifiers { command: true, mac_cmd: true, ..Default::default() }),
        (Vec3::new(25.0, 5.0, 10.0), egui::Modifiers::ALT),
    ];
    for (at, modifiers) in cases {
        let undo = h.app.session.history.len();
        let a = screen(&mut h, at);
        let b = a + egui::vec2(60.0, 40.0);
        press_and_move(&mut h, a, b, modifiers);
        assert!(h.app.viewport.occurrence_drag.is_none() && h.app.viewport.boxsel.is_some(), "{at:?} {modifiers:?}: a selection box");
        release(&mut h, b);
        assert!(origin(&h, &[base]).dist(Vec3::ZERO) < 1e-9 && origin(&h, &[free]).dist(Vec3::ZERO) < 1e-9, "{at:?} {modifiers:?}");
        assert_eq!(h.app.session.history.len(), undo, "{at:?} {modifiers:?}");
    }
}

/// Escape during a drag puts the part back and leaves no undo step.
#[test]
fn escape_cancels_a_drag() {
    let (mut h, _, free) = two_parts();
    let undo = h.app.session.history.len();
    let (a, b) = (screen(&mut h, Vec3::new(25.0, 5.0, 10.0)), screen(&mut h, Vec3::new(25.0, 35.0, 10.0)));
    press_and_move(&mut h, a, b, Default::default());
    assert!(origin(&h, &[free]).dist(Vec3::ZERO) > 1.0, "it moved while dragged");
    for pressed in [true, false] {
        h.app.synthetic.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
    }
    play(&mut h);
    assert!(h.app.viewport.occurrence_drag.is_none());
    assert!(origin(&h, &[free]).dist(Vec3::ZERO) < 1e-9, "{:?}", origin(&h, &[free]));
    release(&mut h, b);
    assert!(origin(&h, &[free]).dist(Vec3::ZERO) < 1e-9, "the release after Escape moves nothing");
    assert_eq!(h.app.session.history.len(), undo);
}

/// With the design active, a drag on a part inside an assembly moves that assembly's placement;
/// with the assembly active, the part inside it.
#[test]
fn the_active_placement_decides_which_occurrence_moves() {
    let (mut h, _, free) = two_parts();
    let asm = h.app.session.doc.occurrences.iter().find(|o| o.id == free).map_or(0, |o| o.component);
    run(&mut h, "component.activate", json!({"component": asm}));
    let leaf = run(&mut h, "component.create", json!({"name": "Leaf"}))["occurrence"].as_u64().expect("occurrence");
    run(&mut h, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, 0, 0]}));
    run(&mut h, "component.activate", json!({"component": "root"}));
    h.frames(2);
    let (a, b) = (screen(&mut h, Vec3::new(42.0, 2.0, 4.0)), screen(&mut h, Vec3::new(42.0, 22.0, 4.0)));
    press_and_move(&mut h, a, b, Default::default());
    release(&mut h, b);
    assert!(origin(&h, &[free]).dist(Vec3::new(0.0, 20.0, 0.0)) < 0.5, "the assembly moves: {:?}", origin(&h, &[free]));
    assert!(origin(&h, &[free, leaf]).dist(Vec3::new(0.0, 20.0, 0.0)) < 0.5, "with its part");
    run(&mut h, "component.activate", json!({"component": asm, "occurrence": free, "path": [free]}));
    h.frames(2);
    let (a, b) = (screen(&mut h, Vec3::new(42.0, 22.0, 4.0)), screen(&mut h, Vec3::new(52.0, 22.0, 4.0)));
    press_and_move(&mut h, a, b, Default::default());
    release(&mut h, b);
    assert!(origin(&h, &[free]).dist(Vec3::new(0.0, 20.0, 0.0)) < 0.5, "the active assembly stays: {:?}", origin(&h, &[free]));
    assert!(origin(&h, &[free, leaf]).dist(Vec3::new(10.0, 20.0, 0.0)) < 0.5, "its part moves: {:?}", origin(&h, &[free, leaf]));
}

#[test]
fn either_member_of_a_floating_hinge_drags_the_whole_chain() {
    let (mut h, base, free) = two_parts();
    run(&mut h, "occurrence.ground", json!({"occurrence": base, "grounded": false}));
    let at = |o| json!({"occurrence": o, "point": [0, 0, 0], "z": [0, 0, 1]});
    run(&mut h, "joint.create", json!({"type": "revolute", "a": at(base), "b": at(free), "flip": true}));
    // The base's centre is under the joint glyph; grab its face away from that glyph.
    for grab in [Vec3::new(25.0, 5.0, 10.0), Vec3::new(4.0, 4.0, 10.0)] {
        h.frames(2);
        h.step(&json!({"shot": "floating-chain-before"})).unwrap();
        let before = h.app.session.doc.to_json();
        let values = h.app.session.doc.assembly.joints[0].values.clone();
        let undo = h.app.session.history.len();
        let (a, b) = (screen(&mut h, grab), screen(&mut h, grab + Vec3::new(0.0, 30.0, 0.0)));
        press_and_move(&mut h, a, b, Default::default());
        assert!(
            h.app.viewport.occurrence_drag.is_some(),
            "grab={grab:?}, base={:?}, free={:?}, group={:?}, joint={:?}, box={:?}",
            origin(&h, &[base]),
            origin(&h, &[free]),
            h.app.session.drag_group(&[free]),
            h.app.viewport.joint_drag,
            h.app.viewport.boxsel
        );
        assert!(h.app.viewport.joint_drag.is_none());
        release(&mut h, b);
        h.step(&json!({"shot": "floating-chain-after"})).unwrap();
        for o in [base, free] {
            assert!(origin(&h, &[o]).dist(Vec3::new(0.0, 30.0, 0.0)) < 0.5);
        }
        assert_eq!(h.app.session.doc.assembly.joints[0].values, values);
        assert_eq!(h.app.session.history.len(), undo + 1);
        run(&mut h, "edit.undo", json!({}));
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

fn key(h: &mut Harness, key: egui::Key, pressed: bool) {
    h.app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() });
    play(h);
}

#[test]
fn r_still_starts_rectangle_with_a_free_part_in_the_design() {
    let (mut h, _, _) = two_parts();
    key(&mut h, egui::Key::R, true);
    key(&mut h, egui::Key::R, false);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::Sketch)), "R starts Rectangle through Create Sketch");
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.then_start), Some("sketch.rectangle.two_point"));
}
