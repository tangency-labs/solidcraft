// Pan, Orbit and Zoom Mode: a shortcut turns a mode on with the pointer over the view, plain
// pointer motion then moves the camera, the same key turns it off, another mode's key switches,
// and Esc ends it.
use crate::scenario::Harness;
use crate::viewport::NavMode;
use egui::{Event, Key, Modifiers, Pos2, vec2};
use serde_json::json;

fn key(key: Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn release(key: Key) -> Event {
    Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }
}

fn press(h: &mut Harness, k: Key) {
    h.frame_with(vec![key(k)]);
    h.frame_with(vec![release(k)]);
}

/// A design with a block, P/O/Z bound to the modes and the pointer resting over the view.
fn setup() -> (Harness, Pos2) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 30, "width": 20, "height": 10})).unwrap();
    for (id, k) in [("view.pan_mode", "P"), ("view.orbit_mode", "O"), ("view.zoom_mode", "Z")] {
        crate::keymap::bind(&mut h.app, id, k, true).unwrap();
    }
    h.settle();
    let rect = h.app.viewport.rect.expect("the view is laid out");
    let at = rect.center() + vec2(rect.width() * 0.2, rect.height() * 0.2);
    h.frame_with(vec![Event::PointerMoved(at)]);
    (h, at)
}

#[test]
fn modes_have_no_default_key_and_list_as_shortcuts() {
    let h = Harness::new();
    for (id, _) in crate::navigation::TOGGLE_MODES {
        let spec = solidcraft_engine::find_command(id).expect("registered");
        assert!(spec.shortcut.is_none_or(str::is_empty), "{id} has no default key");
        assert!(crate::keymap::keys(&h.app, id).is_empty());
    }
}

#[test]
fn each_key_toggles_its_mode_switches_the_other_and_escape_ends_it() {
    let (mut h, _) = setup();
    for (k, mode) in [(Key::P, NavMode::Pan), (Key::O, NavMode::Orbit), (Key::Z, NavMode::Zoom)] {
        press(&mut h, k);
        assert_eq!(h.app.viewport.nav_toggle, Some(mode), "{k:?} turns its mode on");
        press(&mut h, k);
        assert_eq!(h.app.viewport.nav_toggle, None, "{k:?} again turns it off");
    }
    press(&mut h, Key::P);
    press(&mut h, Key::O);
    assert_eq!(h.app.viewport.nav_toggle, Some(NavMode::Orbit), "another mode's key switches");
    press(&mut h, Key::Z);
    assert_eq!(h.app.viewport.nav_toggle, Some(NavMode::Zoom));
    press(&mut h, Key::Escape);
    assert_eq!(h.app.viewport.nav_toggle, None, "Esc ends the mode");
}

#[test]
fn a_mode_starts_only_with_the_pointer_over_the_view() {
    let (mut h, _) = setup();
    h.frame_with(vec![Event::PointerMoved(Pos2::new(2.0, 2.0))]);
    press(&mut h, Key::P);
    assert_eq!(h.app.viewport.nav_toggle, None, "the key away from the view starts nothing");
    h.frame_with(vec![Event::PointerGone]);
    press(&mut h, Key::P);
    assert_eq!(h.app.viewport.nav_toggle, None);
    // The navigation bar at the foot of the view counts as the view.
    let bar = h.app.viewport.layout.as_ref().expect("laid out").navigation.center();
    h.frame_with(vec![Event::PointerMoved(bar)]);
    press(&mut h, Key::P);
    assert_eq!(h.app.viewport.nav_toggle, Some(NavMode::Pan), "the key over the navigation bar starts the mode");
}

#[test]
fn a_mode_keeps_moving_the_view_over_the_toolbars_and_panels() {
    // Out to the top of the window, over the toolbars, and on along them, as a held Zoom drag
    // goes on there.
    let (mut h, at) = setup();
    let top = Pos2::new(at.x, 4.0);
    assert!(crate::shell::covered(&h.app, top) || !h.app.viewport.rect.is_some_and(|r| r.contains(top)), "off the view");
    for k in [Key::P, Key::O, Key::Z] {
        h.frame_with(vec![Event::PointerMoved(at)]);
        press(&mut h, k);
        h.frame_with(vec![Event::PointerMoved(top)]);
        let before = h.app.cam;
        h.frame_with(vec![Event::PointerMoved(top + vec2(40.0, 30.0))]);
        assert_ne!(h.app.cam, before, "{k:?}: the view goes on moving over the toolbar");
        // And over the navigation bar at the bottom of the view.
        let bar = h.app.viewport.layout.as_ref().expect("laid out").navigation.center();
        h.frame_with(vec![Event::PointerMoved(bar)]);
        let before = h.app.cam;
        h.frame_with(vec![Event::PointerMoved(bar + vec2(4.0, -3.0))]);
        assert_ne!(h.app.cam, before, "{k:?}: and over the navigation bar");
        press(&mut h, k);
        assert_eq!(h.app.viewport.nav_toggle, None, "{k:?}: its key ends it there");
        let after = h.app.cam;
        h.frame_with(vec![Event::PointerMoved(top)]);
        assert_eq!(h.app.cam, after, "{k:?}: once ended, motion leaves the view");
    }
}

#[test]
fn plain_motion_pans_orbits_and_zooms_without_touching_the_design() {
    let (mut h, at) = setup();
    let doc = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();

    press(&mut h, Key::P);
    let before = h.app.cam;
    h.frame_with(vec![Event::PointerMoved(at + vec2(30.0, 20.0))]);
    assert_ne!(h.app.cam.target, before.target, "motion pans");
    assert_eq!((h.app.cam.yaw, h.app.cam.pitch, h.app.cam.distance), (before.yaw, before.pitch, before.distance));

    press(&mut h, Key::O);
    let before = h.app.cam;
    h.frame_with(vec![Event::PointerMoved(at)]);
    assert_ne!(h.app.cam.yaw, before.yaw, "motion orbits");
    assert_eq!(h.app.cam.distance, before.distance);

    press(&mut h, Key::Z);
    let before = h.app.cam;
    h.frame_with(vec![Event::PointerMoved(at - vec2(0.0, 40.0))]);
    let zoomed_in = h.app.cam;
    assert!(zoomed_in.distance < before.distance, "moving up zooms in: {} -> {}", before.distance, zoomed_in.distance);
    h.frame_with(vec![Event::PointerMoved(at + vec2(0.0, 40.0))]);
    assert!(h.app.cam.distance > zoomed_in.distance, "moving down zooms out");
    let zoomed_out = h.app.cam;
    h.frame_with(vec![Event::PointerMoved(at + vec2(60.0, 40.0))]);
    assert_eq!(h.app.cam.distance, zoomed_out.distance, "sideways motion does not zoom");

    press(&mut h, Key::Escape);
    let after = h.app.cam;
    h.frame_with(vec![Event::PointerMoved(at)]);
    assert_eq!(h.app.cam, after, "with no mode, plain motion leaves the view");
    assert_eq!(h.app.session.doc.to_json(), doc);
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn zoom_mode_follows_the_zoom_tool_drag_direction() {
    for reverse in [false, true] {
        let (mut h, at) = setup();
        h.app.preferences.zoom_reverse = reverse;
        // The Zoom tool's left drag, from the same point.
        h.app.run("view.tool", json!({"tool": "zoom"})).unwrap();
        let before = h.app.cam;
        h.frame_with(vec![Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        for step in 1..=4 {
            h.frame_with(vec![Event::PointerMoved(at - vec2(0.0, 10.0 * step as f32))]);
        }
        h.frame_with(vec![Event::PointerButton {
            pos: at - vec2(0.0, 40.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        let tool_in = h.app.cam.distance < before.distance;
        h.app.run("view.tool", json!({"tool": "none"})).unwrap();
        h.frame_with(vec![Event::PointerMoved(at)]);
        press(&mut h, Key::Z);
        let before = h.app.cam;
        h.frame_with(vec![Event::PointerMoved(at - vec2(0.0, 40.0))]);
        assert_eq!(h.app.cam.distance < before.distance, tool_in, "moving up zooms as a Zoom tool drag up (invert zoom {reverse})");
        assert_eq!(tool_in, !reverse, "by default up zooms in; invert zoom turns it round");
    }
}

#[test]
fn the_commands_toggle_and_set_the_mode() {
    let mut h = Harness::new();
    h.app.run("view.zoom_mode", json!({})).unwrap();
    assert_eq!(h.app.viewport.nav_toggle, Some(NavMode::Zoom));
    h.app.run("view.pan_mode", json!({"on": true})).unwrap();
    assert_eq!(h.app.viewport.nav_toggle, Some(NavMode::Pan));
    h.app.run("view.orbit_mode", json!({"on": false})).unwrap();
    assert_eq!(h.app.viewport.nav_toggle, Some(NavMode::Pan), "turning off another mode leaves this one");
    h.app.run("view.pan_mode", json!({})).unwrap();
    assert_eq!(h.app.viewport.nav_toggle, None);
    // The engine takes command ids in any case; so does the mode.
    for (id, mode) in [("VIEW.PAN_MODE", NavMode::Pan), ("View.Orbit_Mode", NavMode::Orbit), ("VIEW.ZOOM_MODE", NavMode::Zoom)] {
        h.app.run(id, json!({"on": true})).unwrap();
        assert_eq!(h.app.viewport.nav_toggle, Some(mode), "{id}");
        h.app.run(id, json!({"on": false})).unwrap();
        assert_eq!(h.app.viewport.nav_toggle, None, "{id}");
    }
}
