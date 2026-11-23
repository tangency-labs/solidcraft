//! Views and panels opened in their own windows: each window is its own native viewport, with its
//! own keyboard and pointer input, drawn while the main window draws its frame.
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
    /// The windows whose last frame drew the context menu.
    static MENU_IN: RefCell<Vec<egui::ViewportId>> = RefCell::default();
}

fn context_menu_area(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.area_rect(egui::Id::unique("sc_context_menu"))).is_some()
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
        let mut out = ctx.run_ui(raw, |ui| {
            cb(ui);
            let drawn = context_menu_area(ui.ctx());
            MENU_IN.with(|m| {
                let mut m = m.borrow_mut();
                m.retain(|w| *w != id);
                if drawn {
                    m.push(id);
                }
            });
        });
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

/// Move a panel into its own window.
fn pop_out(h: &mut Harness, panel: &str) {
    let window = json!({"type": "window", "host": format!("{panel}-window"), "origin": {"x": 40, "y": 40},
        "return_to": {"zone": "left", "index": 0, "size": {"width": 280, "height": 600}, "collapsed": false}});
    h.app
        .run(
            "panel.popout",
            json!({"target": {"type": "instance", "id": panel},
                "destination": {"type": "new_section", "section": format!("{panel}-window"), "index": 0, "location": window,
                    "size": {"width": 360, "height": 520}}}),
        )
        .unwrap();
    for _ in 0..6 {
        frame(h, Vec::new());
    }
}

/// A 20 mm box with the Browser popped out into its own window, which holds the keyboard.
fn popped_out_browser() -> (Harness, egui::ViewportId) {
    SEEN.with(|s| s.borrow_mut().clear());
    PENDING.with(|p| p.borrow_mut().clear());
    MENU_IN.with(|m| m.borrow_mut().clear());
    FOCUSED.with(|f| *f.borrow_mut() = None);
    let mut h = Harness::new();
    h.ctx.set_embed_viewports(false);
    install_renderer();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 20})).unwrap();
    for _ in 0..3 {
        frame(&mut h, Vec::new());
    }
    pop_out(&mut h, "browser");
    let windows = SEEN.with(|s| s.borrow().clone());
    assert_eq!(windows.len(), 1, "the Browser has its own window");
    FOCUSED.with(|f| *f.borrow_mut() = Some(windows[0]));
    (h, windows[0])
}

fn click(at: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    let modifiers = egui::Modifiers::NONE;
    vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button, pressed: true, modifiers }]
}

fn release(at: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    vec![egui::Event::PointerButton { pos: at, button, pressed: false, modifiers: egui::Modifiers::NONE }]
}

/// Right-click the window's rows from the top until one opens a menu that `wanted` takes; Esc
/// closes the others. False when none does.
fn right_click_rows(h: &mut Harness, window: egui::ViewportId, wanted: impl Fn(&Harness) -> bool) -> bool {
    for row in 0..30 {
        let at = egui::pos2(80.0, 14.0 + 14.0 * row as f32);
        send(window, click(at, egui::PointerButton::Secondary));
        frame(h, Vec::new());
        send(window, release(at, egui::PointerButton::Secondary));
        frame(h, Vec::new());
        frame(h, Vec::new());
        if h.app.viewport.context_menu.is_some() {
            if wanted(h) {
                return true;
            }
            send(window, type_key(egui::Key::Escape, ""));
            frame(h, Vec::new());
        }
    }
    false
}

/// A right-click on a row of the Browser popped out into its own window opens the row's menu, and
/// that window draws it, not the main window (#19364).
#[test]
fn a_right_click_in_the_popped_out_browser_opens_its_menu_in_that_window() {
    let (mut h, browser) = popped_out_browser();
    assert!(right_click_rows(&mut h, browser, |_| true), "a right-click on a row opens its menu");
    assert_eq!(MENU_IN.with(|m| m.borrow().clone()), vec![browser], "the Browser's window draws its menu");
    assert!(!context_menu_area(&h.ctx), "the main window does not draw it");
    // A click in the main window closes it.
    frame(&mut h, click(egui::pos2(800.0, 500.0), egui::PointerButton::Primary));
    frame(&mut h, Vec::new());
    assert!(h.app.viewport.context_menu.is_none(), "a click elsewhere closes the menu");
}

/// The popped-out Browser's menu closes on a click in another popped-out panel's window, as it
/// does on a click in the main window.
#[test]
fn a_click_in_another_popped_out_panel_closes_the_browsers_menu() {
    let (mut h, browser) = popped_out_browser();
    pop_out(&mut h, "history");
    let history = SEEN.with(|s| s.borrow().iter().copied().find(|w| *w != browser)).expect("History has its own window");
    assert!(right_click_rows(&mut h, browser, |_| true), "a right-click on a row opens its menu");
    let at = egui::pos2(200.0, 200.0);
    send(history, click(at, egui::PointerButton::Primary));
    frame(&mut h, Vec::new());
    send(history, release(at, egui::PointerButton::Primary));
    frame(&mut h, Vec::new());
    assert!(h.app.viewport.context_menu.is_none(), "a click in the History window closes the menu");
}

/// Find in Timeline picked in the popped-out Browser's menu brings the History panel to the front
/// of its tabs, as it does from the docked Browser: the pick runs once the panel layout is back with
/// the app.
#[test]
fn find_in_timeline_from_the_popped_out_browser_shows_the_history_tab() {
    let (mut h, browser) = popped_out_browser();
    h.app.preferences.history_in_browser = false;
    // History behind the render shots tab of the bottom dock.
    h.app
        .run("panel.move", json!({"target": {"type": "instance", "id": "history"}, "destination": {"type": "join_tab", "section": "panel-render-shots", "index": 1}}))
        .unwrap();
    frame(&mut h, Vec::new());
    let history_shown = |h: &Harness| {
        let layout = h.app.panel_runtime().unwrap().layout().layout();
        layout.sections.iter().any(|section| section.active.0 == "history" && !section.collapsed)
    };
    assert!(!history_shown(&h));
    let body_menu = |h: &Harness| matches!(h.app.menu.target, crate::context_menu::Target::Body { .. });
    assert!(right_click_rows(&mut h, browser, body_menu), "a body row opens its menu");
    let target = h.app.menu.target.clone();
    let row = crate::context_menu::items(&h.app, &target).iter().position(|item| item.id == "ui.findTimeline");
    h.app.menu.key_row = Some(row.expect("the body menu offers Find in Timeline"));
    send(browser, type_key(egui::Key::Enter, ""));
    frame(&mut h, Vec::new());
    frame(&mut h, Vec::new());
    assert!(h.app.viewport.context_menu.is_none(), "the pick closes the menu");
    assert!(history_shown(&h), "Find in Timeline brings the History tab to the front");
}
