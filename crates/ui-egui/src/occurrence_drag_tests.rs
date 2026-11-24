//! A left-button drag on a free part repositions it (#2804).
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

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
    let undo = h.app.session.undo.len();
    let (a, b) = (screen(&mut h, Vec3::new(25.0, 5.0, 10.0)), screen(&mut h, Vec3::new(25.0, 35.0, 10.0)));
    press_and_move(&mut h, a, b, Default::default());
    assert!(h.app.viewport.occurrence_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the part, not a selection box");
    release(&mut h, b);
    assert!(h.app.viewport.occurrence_drag.is_none());
    assert!(origin(&h, &[free]).dist(Vec3::new(0.0, 30.0, 0.0)) < 0.5, "{:?}", origin(&h, &[free]));
    assert!(origin(&h, &[base]).dist(Vec3::ZERO) < 1e-9, "the grounded part stays");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
    assert_eq!(h.app.session.undo.last().map(|s| s.label.as_str()), Some("Drag Occurrence"));
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
        let undo = h.app.session.undo.len();
        let a = screen(&mut h, at);
        let b = a + egui::vec2(60.0, 40.0);
        press_and_move(&mut h, a, b, modifiers);
        assert!(h.app.viewport.occurrence_drag.is_none() && h.app.viewport.boxsel.is_some(), "{at:?} {modifiers:?}: a selection box");
        release(&mut h, b);
        assert!(origin(&h, &[base]).dist(Vec3::ZERO) < 1e-9 && origin(&h, &[free]).dist(Vec3::ZERO) < 1e-9, "{at:?} {modifiers:?}");
        assert_eq!(h.app.session.undo.len(), undo, "{at:?} {modifiers:?}");
    }
}

/// Escape during a drag puts the part back and leaves no undo step.
#[test]
fn escape_cancels_a_drag() {
    let (mut h, _, free) = two_parts();
    let undo = h.app.session.undo.len();
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
    assert_eq!(h.app.session.undo.len(), undo);
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
        let undo = h.app.session.undo.len();
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
        assert_eq!(h.app.session.undo.len(), undo + 1);
        run(&mut h, "edit.undo", json!({}));
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

/// Where the drags on the two copies grab their top face: off the origin, whose axes a press on
/// the centre picks first.
const GRAB: Vec3 = Vec3::new(3.0, -3.0, 10.0);

/// One part placed twice on the same spot, as inserting it twice does, and a free 10 mm cube
/// beside them, seen from the top, with nothing selected. The copy whose face a press at
/// [`GRAB`] finds first is grounded and the copy behind it is free: (harness, front copy, copy
/// behind, cube).
fn two_copies() -> (Harness, u64, u64, u64) {
    let (mut h, block, cube) = two_parts();
    let copy = run(&mut h, "occurrence.copy", json!({"occurrence": block, "translate": [0, 0, 0]}))["occurrence"].as_u64().expect("occurrence");
    h.frames(2);
    let at = screen(&mut h, GRAB);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.expect("a viewport"));
    let hits = crate::viewport::pick(&h.app, &proj, at);
    let Some(crate::viewport::Hit::Face { body, .. }) = hits.first() else { panic!("the first pick is not a face: {hits:?}") };
    let holds = |o: u64| h.app.session.occurrence_bodies(&[o], true).is_ok_and(|b| b.contains(body));
    let (front, behind) = if holds(copy) { (copy, block) } else { (block, copy) };
    assert!(holds(front) && !holds(behind), "the face in front is one copy's");
    run(&mut h, "occurrence.ground", json!({"occurrence": front, "grounded": true}));
    run(&mut h, "occurrence.ground", json!({"occurrence": behind, "grounded": false}));
    run(&mut h, "select.set", json!({"items": []}));
    h.frames(2);
    (h, front, behind, cube)
}

/// #15648: with the copy behind selected in the Browser (its row, or its body), a drag over the
/// two copies moves that copy, though the grounded copy's face is in front, in one undo step.
#[test]
fn a_drag_moves_the_selected_copy_where_a_grounded_copy_is_in_front() {
    let (mut h, front, behind, _) = two_copies();
    let body = h.app.session.occurrence_bodies(&[behind], true).expect("bodies").first().cloned().expect("a body");
    for selected in [Sel::Occurrence { path: vec![behind] }, Sel::Body { name: body }] {
        run(&mut h, "select.set", json!({"items": [selected.clone()]}));
        let undo = h.app.session.undo.len();
        let (a, b) = (screen(&mut h, GRAB), screen(&mut h, GRAB + Vec3::new(0.0, 30.0, 0.0)));
        press_and_move(&mut h, a, b, Default::default());
        assert!(h.app.viewport.occurrence_drag.is_some() && h.app.viewport.boxsel.is_none(), "{selected:?}: the drag moves the selected copy");
        release(&mut h, b);
        assert!(origin(&h, &[behind]).dist(Vec3::new(0.0, 30.0, 0.0)) < 0.5, "{selected:?}: {:?}", origin(&h, &[behind]));
        assert!(origin(&h, &[front]).dist(Vec3::ZERO) < 1e-9, "{selected:?}: the grounded copy stays");
        assert_eq!(h.app.session.undo.len(), undo + 1, "{selected:?}: one undo step for the drag");
        run(&mut h, "edit.undo", json!({}));
        assert!(origin(&h, &[behind]).dist(Vec3::ZERO) < 1e-9, "{selected:?}: {:?}", origin(&h, &[behind]));
    }
}

/// With nothing selected, or with the grounded copy in front selected, a drag over the two copies
/// draws a selection box and moves nothing.
#[test]
fn with_nothing_or_the_grounded_copy_selected_a_drag_on_the_copies_draws_a_selection_box() {
    let (mut h, front, behind, _) = two_copies();
    for selection in [vec![], vec![Sel::Occurrence { path: vec![front] }]] {
        run(&mut h, "select.set", json!({"items": selection.clone()}));
        let undo = h.app.session.undo.len();
        let a = screen(&mut h, GRAB);
        let b = a + egui::vec2(60.0, 40.0);
        press_and_move(&mut h, a, b, Default::default());
        assert!(h.app.viewport.occurrence_drag.is_none() && h.app.viewport.boxsel.is_some(), "{selection:?}: a selection box");
        release(&mut h, b);
        assert!(origin(&h, &[front]).dist(Vec3::ZERO) < 1e-9 && origin(&h, &[behind]).dist(Vec3::ZERO) < 1e-9, "{selection:?}");
        assert_eq!(h.app.session.undo.len(), undo, "{selection:?}");
    }
}

/// With the copy behind selected, a drag that starts on another free part moves that part: the
/// selection decides only where the pointer is over a selected part.
#[test]
fn with_a_copy_selected_a_drag_on_another_free_part_moves_that_part() {
    let (mut h, _, behind, cube) = two_copies();
    run(&mut h, "select.set", json!({"items": [Sel::Occurrence { path: vec![behind] }]}));
    let (a, b) = (screen(&mut h, Vec3::new(25.0, 5.0, 10.0)), screen(&mut h, Vec3::new(25.0, 35.0, 10.0)));
    press_and_move(&mut h, a, b, Default::default());
    release(&mut h, b);
    assert!(origin(&h, &[cube]).dist(Vec3::new(0.0, 30.0, 0.0)) < 0.5, "{:?}", origin(&h, &[cube]));
    assert!(origin(&h, &[behind]).dist(Vec3::ZERO) < 1e-9, "the selected copy stays: {:?}", origin(&h, &[behind]));
}

/// With the free cube selected under a grounded lid, a drag over the lid moves the cube; with a
/// construction plane between the lid's top and the cube, the plane stops the drag reaching the
/// cube, as a plane in front of a part stops a drag on it.
#[test]
fn a_selected_part_is_not_grabbed_through_a_plane_in_front_of_it() {
    let (mut h, _, cube) = two_parts();
    let lid = run(&mut h, "component.create", json!({"name": "Lid"}))["occurrence"].as_u64().expect("occurrence");
    // From z = 12 to 30: the plane at z = 15 crosses it, so it is drawn over the lid.
    run(&mut h, "solid.box", json!({"length": 10, "width": 10, "height": 18, "corner": [20, 0, 12]}));
    run(&mut h, "component.activate", json!({"component": "root"}));
    run(&mut h, "occurrence.ground", json!({"occurrence": lid, "grounded": true}));
    run(&mut h, "select.set", json!({"items": [Sel::Occurrence { path: vec![cube] }]}));
    h.frames(2);
    let (a, b) = (screen(&mut h, Vec3::new(25.0, 5.0, 10.0)), screen(&mut h, Vec3::new(25.0, 35.0, 10.0)));
    press_and_move(&mut h, a, b, Default::default());
    assert!(h.app.viewport.occurrence_drag.is_some(), "the selected cube under the lid is dragged");
    release(&mut h, b);
    assert!(origin(&h, &[cube]).dist(Vec3::new(0.0, 30.0, 0.0)) < 0.5, "{:?}", origin(&h, &[cube]));
    run(&mut h, "edit.undo", json!({}));
    run(&mut h, "construct.plane.offset", json!({"base": "XY", "offset": 15}));
    run(&mut h, "select.set", json!({"items": [Sel::Occurrence { path: vec![cube] }]}));
    h.frames(2);
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.expect("a viewport"));
    let hits = crate::viewport::pick_all(&h.app, &proj, a);
    let cube_bodies = h.app.session.occurrence_bodies(&[cube], true).expect("bodies");
    let plane = hits.iter().position(|hit| matches!(hit, crate::viewport::Hit::Plane { .. }));
    let face = hits.iter().position(|hit| matches!(hit, crate::viewport::Hit::Face { body, .. } if cube_bodies.contains(body)));
    assert!(plane.is_some_and(|p| face.is_some_and(|f| p < f)), "the plane lies in front of the cube: {hits:?}");
    press_and_move(&mut h, a, b, Default::default());
    assert!(h.app.viewport.occurrence_drag.is_none() && h.app.viewport.boxsel.is_some(), "the plane in front stops the drag");
    release(&mut h, b);
    assert!(origin(&h, &[cube]).dist(Vec3::ZERO) < 1e-9, "{:?}", origin(&h, &[cube]));
}

/// A selected subassembly holds the parts inside it: with the Free assembly selected, a drag over
/// a grounded lid on its part Leaf moves the whole assembly, as a drag on Leaf in the open does.
#[test]
fn a_selected_assembly_is_dragged_by_its_part_behind_another_part() {
    let (mut h, _, free) = two_parts();
    let asm = h.app.session.doc.occurrences.iter().find(|o| o.id == free).map_or(0, |o| o.component);
    run(&mut h, "component.activate", json!({"component": asm}));
    let leaf = run(&mut h, "component.create", json!({"name": "Leaf"}))["occurrence"].as_u64().expect("occurrence");
    run(&mut h, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, 0, 0]}));
    run(&mut h, "component.activate", json!({"component": "root"}));
    let lid = run(&mut h, "component.create", json!({"name": "Lid"}))["occurrence"].as_u64().expect("occurrence");
    run(&mut h, "solid.box", json!({"length": 8, "width": 8, "height": 8, "corner": [38, -2, 0]}));
    run(&mut h, "component.activate", json!({"component": "root"}));
    run(&mut h, "occurrence.ground", json!({"occurrence": lid, "grounded": true}));
    run(&mut h, "select.set", json!({"items": [Sel::Occurrence { path: vec![free] }]}));
    h.frames(2);
    let (a, b) = (screen(&mut h, Vec3::new(42.0, 2.0, 4.0)), screen(&mut h, Vec3::new(42.0, 22.0, 4.0)));
    press_and_move(&mut h, a, b, Default::default());
    assert!(h.app.viewport.occurrence_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the selected assembly");
    release(&mut h, b);
    assert!(origin(&h, &[free]).dist(Vec3::new(0.0, 20.0, 0.0)) < 0.5, "the assembly moves: {:?}", origin(&h, &[free]));
    assert!(origin(&h, &[free, leaf]).dist(Vec3::new(0.0, 20.0, 0.0)) < 0.5, "with its part: {:?}", origin(&h, &[free, leaf]));
    assert!(origin(&h, &[lid]).dist(Vec3::ZERO) < 1e-9, "the grounded lid stays");
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
