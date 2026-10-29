//! The "Stand this part up?" offer can be answered by handle (#1905): its buttons are published
//! as `stand_up:stand`, `stand_up:leave` and `stand_up:never`, it stays up through a camera move
//! and a command until answered, and `ui.prefs {"stand_up": false}` leaves the file as it is.
//! A file from a program not known to save Y up gets a small note instead by default (#7717),
//! with `stand_up:stand`, `stand_up:never` and `stand_up:dismiss`; Preferences picks the dialog or nothing.
use serde_json::json;
use solidcraft_ui_egui::prefs::StandUpUnknown;
use solidcraft_ui_egui::scenario::Harness;

/// A harness with a 10 × 40 × 5 mm box (Y up) just opened from STEP, from a program not known to
/// save Y up, with Preferences set to ask in the dialog: the dialog is up.
fn opened() -> Harness {
    let h = opened_from("'Example CAD','Example CAD'", |h| h.app.preferences.stand_up_unknown = StandUpUnknown::Ask);
    assert_eq!(h.app.stand_up.as_ref().map(Vec::len), Some(1), "the file's body is offered");
    assert!(!h.app.stand_up_note, "in the dialog");
    h
}

/// The same box, its FILE_NAME naming `programs` (preprocessor, originating system), opened
/// after `prepare` has set the app up.
fn opened_from(programs: &str, prepare: impl FnOnce(&mut Harness)) -> Harness {
    opened_boxes([10.0, 40.0, 5.0], &[[0.0, 0.0, 0.0]], programs, prepare)
}

/// A 40 × 40 × 10 mm plate resting on its broad face: the right way up (Z up).
const UPRIGHT: [f64; 3] = [40.0, 40.0, 10.0];
/// The same plate as a Y-up program saves it: resting on its broad face across Y, so here it
/// stands on its edge, 40 mm tall (lying on its back); stood up it is 10 mm tall.
const ON_ITS_BACK: [f64; 3] = [40.0, 10.0, 40.0];

/// One box of `size` (X, Y, Z), exported with FILE_NAME naming `programs`, opened after
/// `prepare`.
fn opened_box(size: [f64; 3], programs: &str, prepare: impl FnOnce(&mut Harness)) -> Harness {
    opened_boxes(size, &[[0.0, 0.0, 0.0]], programs, prepare)
}

/// Boxes of `size` (X, Y, Z) with their corners at `corners`, each its own body, exported to STEP
/// with FILE_NAME naming `programs`, opened after `prepare` has set the app up.
fn opened_boxes(size: [f64; 3], corners: &[[f64; 3]], programs: &str, prepare: impl FnOnce(&mut Harness)) -> Harness {
    let mut src = Harness::new();
    let path = src.dir.join("tall_in_y.step").to_string_lossy().into_owned();
    for corner in corners {
        let [length, width, height] = size;
        src.app.run("solid.box", json!({"length": length, "width": width, "height": height, "corner": corner, "operation": "new_body"})).unwrap();
    }
    src.app.run("file.export", json!({"path": path})).unwrap();
    // Another program's file: SolidCraft's own exports already have Z up (#5150).
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("'SolidCraft','SolidCraft'", programs)).unwrap();
    let mut h = Harness::new();
    prepare(&mut h);
    h.app.open_path(&path);
    h.frames(3);
    h
}

fn height(h: &Harness) -> f64 {
    let b = solidcraft_engine::view::bounds(&h.app.session);
    b.max.z - b.min.z
}

#[test]
fn the_offer_buttons_are_handles() {
    let mut h = opened();
    for name in ["stand_up:stand", "stand_up:leave", "stand_up:never"] {
        assert!(h.at(&json!({"handle": name})).is_ok(), "{name} is published");
    }
    // Unanswered, it stays up through a camera move and a command.
    h.run(&json!([{"camera": {"yaw": 0.6, "pitch": 0.4}}, {"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1}}}]))
        .unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_some(), "the offer waits for an answer");
    h.run(&json!([{"click": {"handle": "stand_up:stand"}}])).unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_none());
    assert!((height(&h) - 40.0).abs() < 1e-6, "stood up: {}", height(&h));
}

#[test]
fn leave_it_keeps_the_file_and_the_preference() {
    let mut h = opened();
    h.run(&json!([{"click": {"handle": "stand_up:leave"}}])).unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_none());
    assert!(h.app.preferences.offer_stand_up);
    assert!((height(&h) - 5.0).abs() < 1e-6);
    assert!(h.at(&json!({"handle": "stand_up:leave"})).is_err(), "the offer is gone");
}

#[test]
fn dont_ask_again_turns_the_offer_off() {
    let mut h = opened();
    h.run(&json!([{"click": {"handle": "stand_up:never"}}])).unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_none());
    assert!(!h.app.preferences.offer_stand_up);
    assert!((height(&h) - 5.0).abs() < 1e-6);
}

#[test]
fn prefs_stand_up_false_leaves_the_offer() {
    let mut h = opened();
    let r = h.call("ui.prefs", json!({"stand_up": false}));
    assert_eq!(r["result"]["stand_up_offer"], json!(null), "{r}");
    assert_eq!(r["result"]["offer_stand_up"], json!(true), "{r}");
    assert!((height(&h) - 5.0).abs() < 1e-6);
}

#[test]
fn a_solidworks_header_shows_the_offer_and_does_not_turn_the_file() {
    let h = opened_box(ON_ITS_BACK, "'SolidWorks 2021','SolidWorks 2021'", |_| {});
    assert_eq!(h.app.stand_up.as_ref().map(Vec::len), Some(1), "offered");
    assert!(h.app.stand_up_reason, "with the reason line");
    assert!(!h.app.stand_up_note, "in the dialog");
    assert!(!h.app.status_undo, "nothing was turned");
    assert!((height(&h) - 40.0).abs() < 1e-6, "not turned: {}", height(&h));
    // An unknown exporter gets the offer without the reason.
    assert!(!opened().app.stand_up_reason);
}

#[test]
fn stand_it_up_from_a_solidworks_offer_turns_once_and_undo_restores() {
    let mut h = opened_box(ON_ITS_BACK, "'SolidWorks 2021','SolidWorks 2021'", |_| {});
    h.run(&json!([{"click": {"handle": "stand_up:stand"}}])).unwrap();
    h.frames(3);
    assert!((height(&h) - 10.0).abs() < 1e-6, "stood up: {}", height(&h));
    assert!(!h.app.stand_up_reason);
    h.app.run("edit.undo", json!({})).unwrap();
    assert!((height(&h) - 40.0).abs() < 1e-6, "undone: {}", height(&h));
}

#[test]
fn a_solidcraft_header_is_left_alone() {
    let h = opened_from("'SolidCraft','SolidCraft'", |_| {});
    assert!(h.app.stand_up.is_none(), "no offer");
    assert!(!h.app.status_undo);
    assert!((height(&h) - 5.0).abs() < 1e-6, "not turned: {}", height(&h));
}

#[test]
fn the_offer_preference_off_stops_every_offer() {
    let off = |h: &mut Harness| h.app.preferences.offer_stand_up = false;
    assert!(opened_from("'Example CAD','Example CAD'", off).app.stand_up.is_none());
    let h = opened_from("'SolidWorks 2021','SolidWorks 2021'", off);
    assert!(h.app.stand_up.is_none());
    assert!((height(&h) - 5.0).abs() < 1e-6, "not turned: {}", height(&h));
}

#[test]
fn turn_up_axis_in_the_component_menu_turns_and_undo_restores() {
    // Leave the offer, then fix the orientation from the right-click menu.
    let mut h = opened();
    h.run(&json!([{"click": {"handle": "stand_up:leave"}}])).unwrap();
    h.frames(3);
    let occurrence = h.app.session.doc.occurrences.iter().map(|o| o.id).max().expect("the import's occurrence");
    h.app.run("occurrence.ground", json!({"occurrence": occurrence, "grounded": false})).unwrap();
    let items = solidcraft_ui_egui::context_menu::items(&h.app, &solidcraft_ui_egui::context_menu::Target::Occurrence { path: vec![occurrence] });
    let turn = items.iter().find(|i| i.id == "ui.turnUpAxis").expect("Turn Up Axis is in the menu");
    let labels: Vec<_> = turn.children.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Y up \u{2192} Z up", "Z up \u{2192} Y up"]);
    let at = egui::pos2(10.0, 10.0);
    solidcraft_ui_egui::context_menu::run_item(&mut h.app, &turn.children[0], at);
    assert!((height(&h) - 40.0).abs() < 1e-6, "Y up to Z up: {}", height(&h));
    h.app.run("edit.undo", json!({})).unwrap();
    assert!((height(&h) - 5.0).abs() < 1e-6, "undone: {}", height(&h));
    solidcraft_ui_egui::context_menu::run_item(&mut h.app, &turn.children[1], at);
    assert!((height(&h) - 40.0).abs() < 1e-6, "Z up to Y up: {}", height(&h));
}

#[test]
fn old_preferences_files_still_load() {
    // A file saved while "Stand up automatically" existed: the retired key is ignored.
    let old: solidcraft_ui_egui::prefs::Prefs = serde_json::from_str(r#"{"offer_stand_up": false, "auto_stand_up": true, "up_axis": "z"}"#).unwrap();
    assert!(!old.offer_stand_up, "the saved choice is kept");
    let empty: solidcraft_ui_egui::prefs::Prefs = serde_json::from_str("{}").unwrap();
    assert!(empty.offer_stand_up);
}

type V3 = solidcraft_engine::geom::Vec3;

/// A stood-up SolidWorks file (its component still the active one), a sketch started by
/// `start`, a rectangle far from the part at x 200..220, y 0..10 on it, extruded 15 mm: the
/// sketch's world normal, the direction the view turned to look from, and the new body's bounds.
fn sketched_and_extruded(start: impl FnOnce(&mut Harness)) -> (V3, V3, solidcraft_engine::geom::Aabb3) {
    let mut h = opened_from("'SolidWorks 2021','SolidWorks 2021'", |h| h.app.preferences.look_at_anim = false);
    // Stand it up through the offer when one is waiting (a SolidWorks file is offered, not turned silently).
    if h.app.stand_up.is_some() {
        solidcraft_ui_egui::stand_up::stand_up(&mut h.app).unwrap();
        h.frames(2);
    }
    assert!((height(&h) - 40.0).abs() < 1e-6, "stood up: {}", height(&h));
    assert_ne!(h.app.session.active_component, 0, "the import's component is the one worked in");
    let before: Vec<String> = h.app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    start(&mut h);
    h.frames(2);
    let sketch = h.app.session.active_sketch.expect("a sketch is open");
    let normal = h.app.session.world_state().sketch(sketch).unwrap().plane.normal();
    let back = h.app.cam.back();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [200, 0], "p1": [220, 10]})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 15})).unwrap();
    let world = h.app.session.world_state();
    let made: Vec<_> = world.bodies.iter().filter(|b| !before.contains(&b.name)).collect();
    assert_eq!(made.len(), 1);
    (normal, back, made[0].mesh().bounds())
}

fn near(a: V3, b: [f64; 3]) -> bool {
    a.dist(V3::new(b[0], b[1], b[2])) < 1e-6
}

#[test]
fn a_sketch_on_the_xy_plane_shown_after_standing_up_extrudes_up() {
    // The origin planes the viewport and browser show are the design's: the XY picked there
    // stays flat, the view looks down on it and the extrude goes up, though the stood-up
    // component's own XY is now upright.
    let (normal, back, b) = sketched_and_extruded(|h| {
        solidcraft_ui_egui::dialogs::start_sketch(&mut h.app, &solidcraft_engine::Sel::Plane { name: "XY".into() }).unwrap();
    });
    assert!(near(normal, [0.0, 0.0, 1.0]), "faces up: {normal:?}");
    assert!((back.z - 1.0).abs() < 1e-3, "looks down on it: {back:?}");
    assert!(near(b.min, [200.0, 0.0, 0.0]) && near(b.max, [220.0, 10.0, 15.0]), "{b:?}");
}

#[test]
fn a_sketch_on_the_stood_up_components_own_xy_is_turned_once() {
    // By name, a command's XY is the active component's own: upright now, facing -Y. The
    // sketch, the view and the extrude all follow it, the stand-up applied once.
    let (normal, back, b) = sketched_and_extruded(|h| {
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        solidcraft_ui_egui::dialogs::look_at_sketch(&mut h.app);
    });
    assert!(near(normal, [0.0, -1.0, 0.0]), "faces -Y: {normal:?}");
    assert!((back.y + 1.0).abs() < 1e-3, "looks at it from -Y: {back:?}");
    assert!(near(b.min, [200.0, -15.0, 0.0]) && near(b.max, [220.0, 0.0, 10.0]), "{b:?}");
}

/// Whether `text` was painted in the last frame (the note is painted, not a widget).
fn painted(h: &Harness, text: &str) -> bool {
    fn has(shape: &egui::Shape, text: &str) -> bool {
        match shape {
            egui::Shape::Text(t) => t.galley.job.text == text,
            egui::Shape::Vec(shapes) => shapes.iter().any(|s| has(s, text)),
            _ => false,
        }
    }
    h.shapes.iter().any(|c| has(&c.shape, text))
}

const UNKNOWN: &str = "'Example CAD','Example CAD'";
const SOLIDWORKS: &str = "'SolidWorks 2021','SolidWorks 2021'";

#[test]
fn an_unknown_exporter_lying_on_its_back_gets_the_note_not_the_dialog() {
    let mut h = opened_box(ON_ITS_BACK, UNKNOWN, |_| {});
    assert_eq!(h.app.stand_up.as_ref().map(Vec::len), Some(1), "offered");
    assert!(h.app.stand_up_note, "as the note");
    assert!(painted(&h, "Did this part open the wrong way up?"));
    assert!(h.widget("Stand this part up?").is_none(), "no dialog");
    for name in ["stand_up:stand", "stand_up:never", "stand_up:dismiss"] {
        assert!(h.at(&json!({"handle": name})).is_ok(), "{name} is published");
    }
    assert!(h.at(&json!({"handle": "stand_up:leave"})).is_err(), "the dialog's buttons are not");
    // It doesn't hold anything up: the view and commands work while it shows.
    h.run(&json!([{"camera": {"yaw": 0.6, "pitch": 0.4}}, {"call": "engine.execute", "params": {"command": "solid.box", "params": {"length": 1, "width": 1, "height": 1, "corner": [100, 0, 0]}}}]))
        .unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_some(), "still offered");
    assert!((height(&h) - 40.0).abs() < 1e-6, "not turned: {}", height(&h));
    let r = h.call("ui.prefs", json!({}));
    assert_eq!(r["result"]["stand_up_note"], json!(true), "{r}");
    assert_eq!(r["result"]["stand_up_unknown"], json!("note"), "{r}");
}

#[test]
fn an_upright_file_from_an_unknown_exporter_gets_nothing() {
    for (size, what) in [(UPRIGHT, "a plate on its face"), ([10.0, 40.0, 5.0], "a slab on its face")] {
        let h = opened_box(size, UNKNOWN, |_| {});
        assert!(h.app.stand_up.is_none(), "{what}: no offer");
        assert!(!painted(&h, "Did this part open the wrong way up?"), "{what}");
    }
    // Nor an upright assembly of them.
    let h = opened_boxes(UPRIGHT, &[[0.0, 0.0, 0.0], [60.0, 0.0, 0.0]], UNKNOWN, |_| {});
    assert!(h.app.stand_up.is_none(), "upright assembly: no offer");
}

#[test]
fn an_unclear_file_from_an_unknown_exporter_still_gets_the_note() {
    // A cube says nothing about which way is up; a Y-up file is never left without an offer.
    let h = opened_box([40.0, 40.0, 40.0], UNKNOWN, |_| {});
    assert!(h.app.stand_up.is_some() && h.app.stand_up_note, "a cube gets the note");
}

#[test]
fn which_way_is_up_weighs_flat_area_then_the_box() {
    use solidcraft_ui_egui::stand_up::{Upright, upright};
    assert_eq!(upright(1600.0, 400.0, [40.0, 10.0, 40.0]), Upright::Z, "a broad base facing down Z");
    assert_eq!(upright(400.0, 1600.0, [40.0, 40.0, 10.0]), Upright::Y, "broad faces across Y");
    // Close areas: the box decides; the thin axis is up.
    assert_eq!(upright(100.0, 120.0, [40.0, 5.0, 40.0]), Upright::Y);
    assert_eq!(upright(120.0, 100.0, [40.0, 40.0, 5.0]), Upright::Z);
    assert_eq!(upright(100.0, 120.0, [40.0, 40.0, 40.0]), Upright::Unclear, "a cube says nothing");
    assert_eq!(upright(0.0, 0.0, [f64::NAN, 1.0, 1.0]), Upright::Unclear);
    assert_eq!(upright(f64::INFINITY, 1.0, [1.0, 1.0, 1.0]), Upright::Unclear);
}

#[test]
fn the_notes_stand_it_up_turns_once_and_undo_restores() {
    let mut h = opened_box(ON_ITS_BACK, UNKNOWN, |_| {});
    h.run(&json!([{"click": {"handle": "stand_up:stand"}}])).unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_none() && !h.app.stand_up_note, "the note is gone");
    assert!(!painted(&h, "Did this part open the wrong way up?"));
    assert!((height(&h) - 10.0).abs() < 1e-6, "stood up: {}", height(&h));
    h.app.run("edit.undo", json!({})).unwrap();
    assert!((height(&h) - 40.0).abs() < 1e-6, "one undo step: {}", height(&h));
}

#[test]
fn the_notes_dont_ask_again_turns_the_offer_off() {
    let mut h = opened_box(ON_ITS_BACK, UNKNOWN, |_| {});
    h.run(&json!([{"click": {"handle": "stand_up:never"}}])).unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_none(), "closed");
    assert!(!h.app.preferences.offer_stand_up, "the offer is off, as the dialog's Don't ask again");
    assert!((height(&h) - 40.0).abs() < 1e-6, "not turned: {}", height(&h));
}

#[test]
fn the_note_closes_on_dismiss_and_by_itself() {
    let mut h = opened_box(ON_ITS_BACK, UNKNOWN, |_| {});
    h.run(&json!([{"click": {"handle": "stand_up:dismiss"}}])).unwrap();
    h.frames(3);
    assert!(h.app.stand_up.is_none(), "dismissed");
    assert!(h.app.preferences.offer_stand_up, "the preference stays");
    assert!((height(&h) - 40.0).abs() < 1e-6, "not turned: {}", height(&h));
    assert!(h.at(&json!({"handle": "stand_up:stand"})).is_err(), "the note is gone");

    let mut h = opened_box(ON_ITS_BACK, UNKNOWN, |_| {});
    let since = h.app.stand_up_note_since.expect("the note's clock started when it was drawn");
    h.frames(30);
    assert!(h.app.stand_up.is_some(), "half a second on, still up");
    // As if it had been up for the whole time without the pointer on it.
    h.app.stand_up_note_since = Some(since - solidcraft_ui_egui::stand_up::NOTE_SECONDS);
    h.frames(2);
    assert!(h.app.stand_up.is_none(), "gone by itself");
    assert!(!painted(&h, "Did this part open the wrong way up?"));
    assert!((height(&h) - 40.0).abs() < 1e-6, "not turned: {}", height(&h));
}

#[test]
fn the_unknown_programs_preference_picks_the_dialog_or_nothing() {
    // Ask: the dialog, as before, whichever way the file rests.
    let mut h = opened();
    assert!(h.widget("Stand this part up?").is_some(), "the dialog");
    assert!(!painted(&h, "Did this part open the wrong way up?"), "no note");
    assert!(h.at(&json!({"handle": "stand_up:dismiss"})).is_err());
    let ask = |h: &mut Harness| h.app.preferences.stand_up_unknown = StandUpUnknown::Ask;
    assert!(opened_box(UPRIGHT, UNKNOWN, ask).app.stand_up.is_some(), "asked about an upright file too");
    // Don't offer: nothing, and a SolidWorks file still gets the dialog.
    let off = |h: &mut Harness| h.app.preferences.stand_up_unknown = StandUpUnknown::Off;
    assert!(opened_box(ON_ITS_BACK, UNKNOWN, off).app.stand_up.is_none());
    let h = opened_box(ON_ITS_BACK, SOLIDWORKS, off);
    assert!(h.app.stand_up.is_some() && !h.app.stand_up_note && h.app.stand_up_reason);
    // Set through the control channel; a choice it doesn't know is refused.
    let mut h = Harness::new();
    let r = h.call("ui.prefs", json!({"stand_up_unknown": "ask"}));
    assert_eq!(r["result"]["stand_up_unknown"], json!("ask"), "{r}");
    assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Ask);
    let r = h.call("ui.prefs", json!({"stand_up_unknown": "sometimes"}));
    assert_eq!(r["ok"], json!(false), "refused: {r}");
    assert_eq!(h.app.preferences.stand_up_unknown, StandUpUnknown::Ask);
    // The default is the note, and preferences saved before the choice existed read as it.
    let old: solidcraft_ui_egui::prefs::Prefs = serde_json::from_str(r#"{"offer_stand_up": true}"#).unwrap();
    assert_eq!(old.stand_up_unknown, StandUpUnknown::Note);
}

#[test]
fn a_solidworks_file_gets_the_dialog_or_the_note_when_upright() {
    // Lying on its back: the dialog.
    let mut h = opened_box(ON_ITS_BACK, SOLIDWORKS, |_| {});
    assert!(!h.app.stand_up_note, "not the note");
    assert!(h.widget("Stand this part up?").is_some(), "the dialog");
    assert!(!painted(&h, "Did this part open the wrong way up?"));
    for name in ["stand_up:stand", "stand_up:leave", "stand_up:never"] {
        assert!(h.at(&json!({"handle": name})).is_ok(), "{name} is published");
    }
    // Already upright (saved Z up): only the note.
    let h = opened_box(UPRIGHT, SOLIDWORKS, |_| {});
    assert!(h.app.stand_up.is_some() && h.app.stand_up_note, "the note");
    assert!(h.widget("Stand this part up?").is_none(), "no dialog");
    assert!(painted(&h, "Did this part open the wrong way up?"));
    // Ask in a dialog keeps the behaviour from before: the dialog, without looking.
    let h = opened_box(UPRIGHT, SOLIDWORKS, |h| h.app.preferences.stand_up_unknown = StandUpUnknown::Ask);
    assert!(h.app.stand_up.is_some() && !h.app.stand_up_note, "the dialog");
}

#[test]
fn a_file_with_several_bodies_is_an_assembly() {
    let two = [[0.0, 0.0, 0.0], [60.0, 0.0, 0.0]];
    let h = opened_boxes(ON_ITS_BACK, &two, UNKNOWN, |_| {});
    assert_eq!(h.app.stand_up.as_ref().map(Vec::len), Some(2));
    assert!(h.app.stand_up_assembly);
    assert!(painted(&h, "Did this assembly open the wrong way up?"), "the note says assembly");
    let h = opened_boxes(ON_ITS_BACK, &two, SOLIDWORKS, |_| {});
    assert!(h.widget("Stand this assembly up?").is_some(), "the dialog says assembly");
    assert!(h.widget("Stand this part up?").is_none());
    // One body is a part.
    assert!(!opened_box(ON_ITS_BACK, UNKNOWN, |_| {}).app.stand_up_assembly);
}
