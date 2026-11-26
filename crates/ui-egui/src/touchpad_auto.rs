//! The Touchpad preset while the touchpad is in use (#22442): with `touchpad_auto_preset` on, a
//! two-finger scroll, pinch or twist switches navigation to the Touchpad preset, and a mouse
//! wheel notch or a middle press switches back to the preset the user chose. Pointer
//! motion alone never switches, since both devices make it. The chosen preset stays the saved
//! one: [`saved_nav`] is what the settings file gets while the Touchpad preset is borrowed.

use crate::SolveApp;
use crate::prefs::TOUCHPAD_NAV;

/// Which device this frame's input came from, when only one device makes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Device {
    Touchpad,
    Mouse,
}

/// The device that made `events`, the last one that tells, or None when nothing tells.
pub(crate) fn device(events: &[egui::Event]) -> Option<Device> {
    events.iter().rev().find_map(|event| match event {
        egui::Event::Zoom(_) | egui::Event::Rotate(_) => Some(Device::Touchpad),
        egui::Event::MouseWheel { source: egui::MouseWheelSource::Wheel, .. } => Some(Device::Mouse),
        egui::Event::MouseWheel { source: egui::MouseWheelSource::Unknown, unit, .. } => match unit {
            // A precision touchpad scrolls in points, a wheel in lines (Windows, X11, Wayland).
            egui::MouseWheelUnit::Point => Some(Device::Touchpad),
            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => Some(Device::Mouse),
        },
        egui::Event::MouseWheel { .. } => Some(Device::Touchpad),
        // A touchpad's two-finger click is a right press too, so only the middle button tells.
        egui::Event::PointerButton { button: egui::PointerButton::Middle, pressed: true, .. } => Some(Device::Mouse),
        _ => None,
    })
}

/// Follows the device in use; called once a frame before navigation reads the preset. A choice
/// made in Preferences, turning the setting off, or a scripted press ends a borrowed Touchpad
/// preset. While the Preferences dialog is open nothing switches, so it shows what it saves.
pub(crate) fn follow(app: &mut SolveApp, device: Option<Device>, buttons_down: bool) {
    if app.touchpad_borrowed.is_some() && app.preferences.nav != TOUCHPAD_NAV {
        app.touchpad_borrowed = None;
    }
    if !app.preferences.touchpad_auto_preset || app.synthetic_press {
        if let Some(chosen) = app.touchpad_borrowed.take() {
            app.preferences.nav = chosen;
        }
        return;
    }
    if app.prefs_window.shell.snapshot().is_some() {
        return;
    }
    match device {
        // Not in the middle of a drag: a press that starts one comes from the mouse anyway.
        Some(Device::Touchpad) if !buttons_down && app.preferences.nav != TOUCHPAD_NAV => {
            app.touchpad_borrowed = Some(std::mem::replace(&mut app.preferences.nav, TOUCHPAD_NAV.into()));
        }
        Some(Device::Mouse) => {
            if let Some(chosen) = app.touchpad_borrowed.take() {
                app.preferences.nav = chosen;
            }
        }
        _ => {}
    }
}

/// The preset the settings file keeps: the chosen one while the Touchpad preset is borrowed.
pub(crate) fn saved_nav(app: &SolveApp) -> Option<&str> {
    app.touchpad_borrowed.as_deref().filter(|_| app.preferences.nav == TOUCHPAD_NAV)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, TouchPhase, pos2, vec2};

    fn app() -> SolveApp {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.preferences.nav = "fusion".into();
        app.preferences.touchpad_auto_preset = true;
        app
    }

    fn wheel(unit: MouseWheelUnit, source: MouseWheelSource) -> Event {
        Event::MouseWheel { unit, delta: vec2(0.0, -3.0), phase: TouchPhase::Move, source, modifiers: Modifiers::NONE }
    }

    fn press(button: PointerButton) -> Event {
        Event::PointerButton { pos: pos2(400.0, 300.0), button, pressed: true, modifiers: Modifiers::NONE }
    }

    /// One frame of `logic` with `events`, the way the window delivers them.
    fn frame(app: &mut SolveApp, ctx: &egui::Context, events: Vec<Event>) {
        let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| app.logic(ui.ctx()));
        out.textures_delta.clear();
    }

    fn saved_nav_in_file(app: &SolveApp) -> String {
        let saved: serde_json::Value = serde_json::from_str(&app.prefs()).unwrap_or_default();
        saved["preferences"]["nav"].as_str().unwrap_or_default().to_owned()
    }

    #[test]
    fn only_input_one_device_makes_tells_which_device_is_in_use() {
        use MouseWheelSource::{Momentum, Trackpad, Unknown, Wheel};
        assert_eq!(device(&[Event::Zoom(1.1)]), Some(Device::Touchpad));
        assert_eq!(device(&[Event::Rotate(0.1)]), Some(Device::Touchpad));
        assert_eq!(device(&[wheel(MouseWheelUnit::Point, Unknown)]), Some(Device::Touchpad));
        assert_eq!(device(&[wheel(MouseWheelUnit::Point, Trackpad)]), Some(Device::Touchpad));
        assert_eq!(device(&[wheel(MouseWheelUnit::Point, Momentum)]), Some(Device::Touchpad));
        assert_eq!(device(&[wheel(MouseWheelUnit::Line, Unknown)]), Some(Device::Mouse));
        assert_eq!(device(&[wheel(MouseWheelUnit::Point, Wheel)]), Some(Device::Mouse));
        assert_eq!(device(&[press(PointerButton::Middle)]), Some(Device::Mouse));
        // A touchpad clicks with the primary and secondary buttons too, and both devices move.
        assert_eq!(device(&[press(PointerButton::Primary), press(PointerButton::Secondary)]), None);
        assert_eq!(device(&[Event::PointerMoved(pos2(10.0, 10.0))]), None);
        assert_eq!(device(&[Event::Zoom(1.1), press(PointerButton::Middle)]), Some(Device::Mouse), "the last one tells");
    }

    #[test]
    fn the_touchpad_borrows_its_preset_and_the_mouse_gives_the_chosen_one_back_unsaved() {
        let mut app = app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![wheel(MouseWheelUnit::Point, MouseWheelSource::Unknown)]);
        assert_eq!(app.preferences.nav, TOUCHPAD_NAV);
        assert_eq!(saved_nav_in_file(&app), "fusion", "the borrowed preset is never written");
        frame(&mut app, &ctx, vec![Event::PointerMoved(pos2(50.0, 50.0))]);
        assert_eq!(app.preferences.nav, TOUCHPAD_NAV, "motion alone keeps it");
        frame(&mut app, &ctx, vec![wheel(MouseWheelUnit::Line, MouseWheelSource::Unknown)]);
        assert_eq!(app.preferences.nav, "fusion");
        assert_eq!(saved_nav_in_file(&app), "fusion");
    }

    #[test]
    fn off_or_mid_drag_or_a_new_choice_the_preset_stays_as_chosen() {
        let mut app = app();
        app.preferences.touchpad_auto_preset = false;
        follow(&mut app, Some(Device::Touchpad), false);
        assert_eq!(app.preferences.nav, "fusion", "off by default and when off");
        app.preferences.touchpad_auto_preset = true;
        follow(&mut app, Some(Device::Touchpad), true);
        assert_eq!(app.preferences.nav, "fusion", "never in the middle of a drag");
        follow(&mut app, Some(Device::Touchpad), false);
        assert_eq!(app.preferences.nav, TOUCHPAD_NAV);
        // A preset chosen in Preferences while the touchpad's is borrowed is the new choice.
        app.preferences.nav = "blender".into();
        follow(&mut app, Some(Device::Mouse), false);
        assert_eq!(app.preferences.nav, "blender");
        assert_eq!(saved_nav(&app), None);
        // Turning the setting off gives the chosen preset back at once.
        follow(&mut app, Some(Device::Touchpad), false);
        app.preferences.touchpad_auto_preset = false;
        follow(&mut app, None, false);
        assert_eq!(app.preferences.nav, "blender");
        // Someone who chose the Touchpad preset keeps it whatever the mouse does.
        app.preferences.touchpad_auto_preset = true;
        app.preferences.nav = TOUCHPAD_NAV.into();
        follow(&mut app, Some(Device::Mouse), false);
        assert_eq!(app.preferences.nav, TOUCHPAD_NAV);
    }
}
