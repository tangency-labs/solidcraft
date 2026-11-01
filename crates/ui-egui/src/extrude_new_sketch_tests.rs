//! New Sketch from inside Extrude (#6383): the dialog waits while its sketch is drawn, comes
//! back with the sketch's profile picked when it has only one (none when it has more), and OK
//! makes the sketch and the extrude one undo step;
//! Cancel leaves the design and its undo history as they were. Scripts are not affected.
use std::sync::Arc;

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::Harness;
use crate::test_support::{click, key};
use crate::viewport::Hit;

fn xy() -> Hit {
    Hit::Plane { name: "XY".into(), point: Vec3::new(5.0, 5.0, 0.0) }
}

fn sketches(h: &Harness) -> usize {
    h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).count()
}

fn extrudes(h: &Harness) -> usize {
    h.app.session.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Extrude { .. })).count()
}

fn in_extrude(h: &Harness) -> bool {
    h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Extrude { .. }))
}

/// A box (so the design has something to undo to), the Extrude dialog open with nothing picked.
fn extrude_open() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [40, 0, 0]})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    assert!(in_extrude(&h));
    h
}

/// New Sketch, XY picked as its plane, two separate squares drawn: the sketch is open.
fn sketch_two_squares(h: &mut Harness) {
    click(h, "dialog:new_sketch");
    assert!(h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Sketch)), "Create Sketch asks for the plane");
    crate::viewport::dialog_pick(&mut h.app, &xy(), Sel::Plane { name: "XY".into() });
    h.frames(3);
    assert!(h.app.session.active_sketch.is_some(), "the sketch opened on XY");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [30, 10]})).unwrap();
}

fn ok(h: &mut Harness) {
    h.frames(2);
    h.step(&json!({"widget": "OK"})).unwrap();
    for _ in 0..200 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
    }
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|d| (&d.error, &d.inputs)));
}

#[test]
fn new_sketch_with_two_profiles_comes_back_with_none_picked_and_ok_is_one_undo_step() {
    let mut h = extrude_open();
    let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
    sketch_two_squares(&mut h);
    h.app.start("sketch.finish");
    h.frames(3);
    assert!(in_extrude(&h), "Finish Sketch goes back to Extrude");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty(), "two profiles: the user picks");
    let sketch = h.app.session.doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).unwrap().id;
    for index in [0, 1] {
        crate::viewport::dialog_pick(&mut h.app, &Hit::Profile { sketch, index }, Sel::Profile { sketch, index });
    }
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, vec![Sel::Profile { sketch, index: 0 }, Sel::Profile { sketch, index: 1 }]);
    ok(&mut h);
    assert_eq!((sketches(&h), extrudes(&h)), (1, 1), "the sketch and the extrude are their own timeline entries");
    assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
    assert_eq!(h.app.session.undo.last().map(|s| s.label.as_str()), Some("Extrude"));
    h.app.start("edit.undo");
    h.settle();
    assert!(Arc::ptr_eq(&h.app.session.doc, &doc), "Undo takes the sketch and the extrude away together");
    h.app.start("edit.redo");
    h.settle();
    assert_eq!((sketches(&h), extrudes(&h)), (1, 1), "Redo brings both back");
}

#[test]
fn cancel_after_the_sketch_leaves_nothing() {
    let mut h = extrude_open();
    let (doc, undo, redo) = (h.app.session.doc.clone(), h.app.session.undo.len(), h.app.session.redo.len());
    sketch_two_squares(&mut h);
    h.app.start("sketch.finish");
    h.frames(3);
    assert!(in_extrude(&h));
    h.step(&json!({"widget": "Cancel"})).unwrap();
    h.frames(2);
    assert!(h.app.dialog.is_none() && h.app.inline_sketch.is_none());
    assert!(Arc::ptr_eq(&h.app.session.doc, &doc), "no sketch is left behind");
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), (undo, redo), "and no undo step");
}

#[test]
fn escape_in_the_returned_dialog_leaves_nothing() {
    let mut h = extrude_open();
    let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
    sketch_two_squares(&mut h);
    h.app.start("sketch.finish");
    h.frames(3);
    h.frame_with(key(egui::Key::Escape));
    h.frames(2);
    assert!(h.app.dialog.is_none());
    assert!(Arc::ptr_eq(&h.app.session.doc, &doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn cancel_sketch_goes_back_to_the_dialog_without_it() {
    let mut h = extrude_open();
    let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
    sketch_two_squares(&mut h);
    h.app.start("sketch.cancel");
    h.frames(3);
    assert!(in_extrude(&h), "back in Extrude");
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty());
    assert!(Arc::ptr_eq(&h.app.session.doc, &doc), "without the sketch");
    assert_eq!(h.app.session.undo.len(), undo, "and without a step to undo it");
}

#[test]
fn a_plane_clicked_into_empty_profiles_starts_the_sketch_on_it() {
    let mut h = extrude_open();
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.candidate(&h.app.session, &xy()), Some(Sel::Plane { name: "XY".into() }), "the plane is offered while Profiles is empty");
    crate::viewport::dialog_pick(&mut h.app, &xy(), Sel::Plane { name: "XY".into() });
    h.frames(2);
    assert!(h.app.session.active_sketch.is_some(), "the sketch opened on the plane at once");
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    // Extrude's own button (ribbon, shortcut, gesture) in the sketch goes back to the dialog.
    h.app.start("solid.extrude");
    h.frames(3);
    assert!(h.app.session.active_sketch.is_none() && in_extrude(&h));
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1, "a sketch with one profile has it picked");
    ok(&mut h);
    assert_eq!((sketches(&h), extrudes(&h)), (1, 1));
}

/// On a face, the face's outline is projected into the sketch: a circle drawn inside it and the
/// ring of the face around it are two profiles, so none is picked.
#[test]
fn on_a_face_a_circle_and_the_ring_around_it_leave_the_pick_to_the_user() {
    let mut h = extrude_open();
    click(&mut h, "dialog:new_sketch");
    let top = Vec3::new(42.5, 2.5, 5.0);
    let face = crate::dialogs::face_sel(&h.app.session, top).unwrap();
    let Sel::Face { body, index, point } = face.clone() else { panic!("{face:?}") };
    crate::viewport::dialog_pick(&mut h.app, &Hit::Face { body, index, point }, face);
    h.frames(3);
    let sketch = h.app.session.active_sketch.expect("the sketch opened on the face");
    let plane = h.app.session.world_state().sketch(sketch).unwrap().plane;
    let c = plane.to_local(top);
    h.app.run("sketch.circle.center", json!({"center": [c.x, c.y], "radius": 1})).unwrap();
    h.app.start("sketch.finish");
    h.frames(3);
    let state = h.app.session.world_state();
    let profiles = &state.sketch(sketch).unwrap().profiles;
    assert!(profiles.len() >= 2, "the face's outline bounds a profile too: {}", profiles.len());
    assert!(in_extrude(&h));
    assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.is_empty(), "nothing picked for the user");
}

#[test]
fn editing_an_extrude_offers_no_new_sketch() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 5})).unwrap();
    let id = h.app.session.doc.features.last().unwrap().id;
    h.app.edit_feature(id);
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    assert!(crate::command_sketch::profile_input(d).is_none());
    assert!(crate::scenario::handle_at("dialog:new_sketch").is_none());
}

/// Scripts see no change: `solid.extrude` takes the sketch it is given, every command its own
/// undo step, and the sketch stays a feature of its own.
#[test]
fn a_script_extrude_is_unchanged() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "S"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let undo = h.app.session.undo.len();
    h.app.run("solid.extrude", json!({"sketch": "S", "profiles": [0], "distance": 5})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!((sketches(&h), extrudes(&h)), (1, 1));
    assert!(h.app.inline_sketch.is_none());
}
