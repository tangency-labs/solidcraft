//! Views opened in their own windows: each window is its own native viewport, with its own
//! keyboard input, drawn while the main window draws its frame.
use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;

use crate::dialogs::{Kind, face_sel};
use crate::scenario::Harness;

thread_local! {
    /// Events waiting for each view window's next frame.
    static PENDING: RefCell<HashMap<egui::ViewportId, Vec<egui::Event>>> = RefCell::default();
    /// The view window that holds the keyboard (none: the main window).
    static FOCUSED: RefCell<Option<egui::ViewportId>> = const { RefCell::new(None) };
    /// The view windows drawn so far.
    static SEEN: RefCell<Vec<egui::ViewportId>> = RefCell::default();
}

fn info(focused: bool) -> egui::ViewportInfo {
    egui::ViewportInfo { native_pixels_per_point: Some(1.0), focused: Some(focused), ..Default::default() }
}

/// What eframe does for an immediate viewport: a pass of the shared context with that window's
/// own input, inside the main window's frame.
fn install_renderer() {
    egui::Context::set_immediate_viewport_renderer(|ctx, viewport| {
        let id = viewport.ids.this;
        SEEN.with(|seen| {
            if !seen.borrow().contains(&id) {
                seen.borrow_mut().push(id);
            }
        });
        let focused = FOCUSED.with(|f| *f.borrow()) == Some(id);
        let mut raw = egui::RawInput {
            viewport_id: id,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))),
            time: Some(ctx.input(|i| i.time)),
            focused,
            events: PENDING.with(|p| p.borrow_mut().remove(&id).unwrap_or_default()),
            ..Default::default()
        };
        raw.viewports.insert(id, info(focused));
        let mut cb = viewport.viewport_ui_cb;
        let mut out = ctx.run_ui(raw, |ui| cb(ui));
        out.textures_delta.clear();
    });
}

/// A 20 mm box and `n` more views of it, each in its own window.
fn windows(n: usize) -> (Harness, Vec<egui::ViewportId>) {
    SEEN.with(|s| s.borrow_mut().clear());
    PENDING.with(|p| p.borrow_mut().clear());
    FOCUSED.with(|f| *f.borrow_mut() = None);
    let mut h = Harness::new();
    h.ctx.set_embed_viewports(false);
    install_renderer();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    for _ in 0..n {
        let main = h.app.docs.views.active().id;
        let view = crate::documents::view_duplicate(&mut h.app, main).unwrap();
        h.app.run("tab.view.move_to_window", json!({"view": view})).unwrap();
    }
    for _ in 0..6 {
        frame(&mut h, Vec::new());
    }
    let children = SEEN.with(|s| s.borrow().clone());
    assert_eq!(children.len(), n, "each view has its own window");
    (h, children)
}

/// One frame of the main window, which draws every view window too. `root` goes to the main
/// window; events queued with [`send`] go to their window.
fn frame(h: &mut Harness, root: Vec<egui::Event>) {
    let focused = FOCUSED.with(|f| *f.borrow());
    let time = h.ctx.input(|i| i.time) + 1.0 / 60.0;
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        time: Some(time),
        focused: focused.is_none(),
        events: root,
        ..Default::default()
    };
    raw.viewports.insert(egui::ViewportId::ROOT, info(focused.is_none()));
    for id in SEEN.with(|s| s.borrow().clone()) {
        raw.viewports.insert(id, info(focused == Some(id)));
    }
    h.app.raw_input_hook(&mut raw);
    let app = &mut h.app;
    let mut out = h.ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::view_windows::prepare(app, ui.ctx());
        app.ui(ui);
        crate::view_windows::show(app, ui.ctx(), None);
    });
    out.textures_delta.clear();
    for id in out.viewport_output.keys().filter(|id| **id != egui::ViewportId::ROOT) {
        SEEN.with(|s| {
            if !s.borrow().contains(id) {
                s.borrow_mut().push(*id);
            }
        });
    }
}

fn send(window: egui::ViewportId, events: Vec<egui::Event>) {
    PENDING.with(|p| p.borrow_mut().entry(window).or_default().extend(events));
}

/// A key pressed and released as a keyboard sends it: the press, its character, the release.
fn type_key(key: egui::Key, text: &str) -> Vec<egui::Event> {
    let press = |pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: egui::Modifiers::NONE };
    vec![press(true), egui::Event::Text(text.into()), press(false)]
}

fn extrude_distance(h: &Harness) -> String {
    let d = h.app.dialog.as_ref().expect("the Extrude dialog is open");
    let Kind::Extrude { distance, .. } = &d.kind else { panic!("the Extrude dialog") };
    distance.clone()
}

/// E pressed in a window starts Extrude there; the face picked after it gives that window's
/// value box the keyboard, its value selected. The E that started the tool is typed nowhere.
fn extrude_key_then_pick(windows_open: usize, press_in: impl Fn(&[egui::ViewportId]) -> Option<egui::ViewportId>) {
    let (mut h, children) = windows(windows_open);
    let target = press_in(&children);
    let window = target.unwrap_or(egui::ViewportId::ROOT);
    let case = format!("{windows_open} view windows, keys in {window:?}");
    FOCUSED.with(|f| *f.borrow_mut() = target);
    frame(&mut h, Vec::new());
    assert!(h.app.dialog.is_none());
    let typed = |h: &mut Harness, events: Vec<egui::Event>| match target {
        Some(window) => {
            send(window, events);
            frame(h, Vec::new());
        }
        None => frame(h, events),
    };
    typed(&mut h, type_key(egui::Key::E, "e"));
    assert!(h.app.dialog.as_ref().is_some_and(|d| matches!(d.kind, Kind::Extrude { .. })), "E starts Extrude ({case})");
    let left = h.ctx.input_for(window, |i| i.events.iter().filter(|e| matches!(e, egui::Event::Text(_))).count());
    assert_eq!(left, 0, "the shortcut's character is not left for a field ({case})");
    let initial = extrude_distance(&h);
    for _ in 0..3 {
        frame(&mut h, Vec::new());
    }
    let mut d = h.app.dialog.take().unwrap();
    let Some(face @ Sel::Face { .. }) = face_sel(&h.app.session, Vec3::new(10.0, 10.0, 20.0)) else { panic!("top face") };
    d.pick(&h.app.session, face);
    h.app.dialog = Some(d);
    for _ in 0..6 {
        frame(&mut h, Vec::new());
    }
    assert_eq!(extrude_distance(&h), initial, "nothing is typed by the pick ({case})");
    typed(&mut h, vec![egui::Event::Text("7".into())]);
    frame(&mut h, Vec::new());
    assert_eq!(extrude_distance(&h), "7", "typing after the pick replaces the distance in the window with the keyboard ({case})");
}

#[test]
fn a_shortcut_key_in_the_main_window_does_not_type_into_the_dialog_it_opens() {
    extrude_key_then_pick(0, |_| None);
}

#[test]
fn a_shortcut_key_with_one_view_window_does_not_type_into_the_dialog_it_opens() {
    extrude_key_then_pick(1, |w| Some(w[0]));
}

#[test]
fn a_shortcut_key_in_a_view_window_does_not_type_into_the_dialog_it_opens() {
    extrude_key_then_pick(2, |w| Some(w[1]));
    extrude_key_then_pick(2, |w| Some(w[0]));
}

#[test]
fn a_shortcut_key_in_the_main_window_with_view_windows_does_not_type_into_the_dialog_it_opens() {
    extrude_key_then_pick(2, |_| None);
}
