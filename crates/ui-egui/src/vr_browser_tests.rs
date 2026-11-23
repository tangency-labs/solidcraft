//! The headset's Browser (`vr_browser`): drawn into a context of its own, its eyes and rows
//! answer a pointer as the window's do.

use crate::scenario::Harness;
use serde_json::json;

/// One pass of the headset's Browser in `ctx`, 300 × 480 points, given `events`.
fn pass(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 480.0))),
        events,
        focused: true,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| crate::vr_browser::show(&mut h.app, ui, lit));
    // No renderer here: its pictures are let go unapplied.
    out.textures_delta.clear();
}

/// Where the headset's Browser drew `name`'s eye (the latest published wins: its own pass).
fn eye(name: &str) -> egui::Pos2 {
    solidcraft_ui_registry::handle_at(&format!("eye:{name}")).unwrap_or_else(|| panic!("no eye for {name}"))
}

/// A press and release on the eye of `name`, the release `jitter` points off the press.
fn click_eye(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, name: &str, jitter: f32) {
    pass(h, ctx, lit, Vec::new());
    let at = eye(name);
    click_at(h, ctx, lit, at, jitter);
}

/// A press at `at` and a release `jitter` points off it, the pointer moved there first.
fn click_at(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, at: egui::Pos2, jitter: f32) {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    pass(h, ctx, lit, vec![egui::Event::PointerMoved(at)]);
    pass(h, ctx, lit, vec![button(at, true)]);
    let off = at + egui::vec2(jitter, jitter * 0.5);
    pass(h, ctx, lit, vec![egui::Event::PointerMoved(off)]);
    pass(h, ctx, lit, vec![button(off, false)]);
    pass(h, ctx, lit, Vec::new());
}

/// Two bodies, the window drawn once.
fn two_bodies() -> Harness {
    let mut h = Harness::new();
    for (name, x) in [("One", 0), ("Two", 20)] {
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
    }
    h.settle();
    h
}

/// A click on a body's eye in the headset's Browser hides it, and another shows it again.
#[test]
fn an_eye_in_the_headsets_browser_hides_and_shows() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    click_eye(&mut h, &ctx, &mut lit, "Two", 0.0);
    assert!(h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Two"), "hidden: {:?}", h.app.session.visibility.hidden_bodies);
    click_eye(&mut h, &ctx, &mut lit, "Two", 0.0);
    assert!(!h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Two"));
}

/// A ray shakes as the trigger is pulled: an eye pressed and let go a few points off still
/// toggles once, and only its own row.
#[test]
fn an_eye_pressed_with_a_shaking_ray_still_toggles() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    click_eye(&mut h, &ctx, &mut lit, "Two", 4.0);
    assert_eq!(h.app.session.visibility.hidden_bodies, vec!["Two".to_string()]);
}

/// A placed part's eye in the headset's Browser hides the placement, as the window's does.
#[test]
fn a_placements_eye_in_the_headsets_browser_hides_it() {
    let mut h = Harness::new();
    h.app.preferences.browser_components_open = true;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Top"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Part"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"body_name":"Inner"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.settle();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    click_eye(&mut h, &ctx, &mut lit, &format!("occurrence:{part}"), 3.0);
    assert!(h.app.session.occurrence_hidden(&[part]), "hidden placements: {:?}", h.app.session.visibility.hidden_occurrences);
}

/// A press and release where the headset's Browser has no row (below its last one) hides
/// nothing, selects nothing and lights nothing.
#[test]
fn a_click_on_no_row_in_the_headsets_browser_changes_nothing() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    pass(&mut h, &ctx, &mut lit, Vec::new());
    let below = solidcraft_ui_registry::handle_at("row:Two").expect("a row for Two").y + 150.0;
    assert!(below < 470.0, "the window has room below its rows: {below}");
    let (doc, visibility) = (h.app.session.doc.clone(), h.app.session.visibility.clone());
    click_at(&mut h, &ctx, &mut lit, egui::pos2(150.0, below), 0.0);
    assert_eq!(h.app.session.visibility, visibility);
    assert_eq!(h.app.session.doc, doc);
    assert!(h.app.session.selection.is_empty(), "{:?}", h.app.session.selection);
    assert!(lit.is_empty() && h.app.viewport.hover_bodies.is_empty(), "lit: {lit:?}");
}

/// The ray on a body's row in the headset's Browser lights that body in the design, and the
/// window closing (lighting nothing) puts it out again.
#[test]
fn a_row_pointed_at_in_the_headsets_browser_lights_its_body() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    pass(&mut h, &ctx, &mut lit, Vec::new());
    let row = solidcraft_ui_registry::handle_at("row:One").expect("a row for One");
    pass(&mut h, &ctx, &mut lit, vec![egui::Event::PointerMoved(row)]);
    pass(&mut h, &ctx, &mut lit, Vec::new());
    assert_eq!(lit, vec!["One".to_string()]);
    assert!(h.app.viewport.hover_bodies.iter().any(|b| b == "One"), "{:?}", h.app.viewport.hover_bodies);
    crate::vr_pick::light(&mut h.app, &mut lit, Vec::new());
    assert!(!h.app.viewport.hover_bodies.iter().any(|b| b == "One"), "{:?}", h.app.viewport.hover_bodies);
}

/// The headset's Browser is drawn by a context of its own, but the window's stays the one the
/// Browser's drags and captures are ended through (a document switch ends them there).
#[test]
fn the_headsets_browser_leaves_the_windows_context_in_charge() {
    let mut h = two_bodies();
    let window = egui::Context::default();
    crate::semantic_content::ensure_session(&mut h.app, &window);
    let headset = egui::Context::default();
    let mut lit = Vec::new();
    pass(&mut h, &headset, &mut lit, Vec::new());
    assert!(h.app.tree.content.context() == Some(&window), "the window's context is kept");
}

/// One pass of the headset's Browser given `events`, then one frame of the window, as the app
/// runs them: the window's frame comes between the headset's.
fn pass_and_window(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, events: Vec<egui::Event>) {
    pass(h, ctx, lit, events);
    h.frame();
}

/// Where the headset's Browser draws `name`'s eye: read right after its own pass, as the window's
/// frame publishes its own eyes under the same names.
fn headset_eye(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, name: &str) -> egui::Pos2 {
    pass(h, ctx, lit, Vec::new());
    eye(name)
}

/// Select pressed at `at`, the ray moved by `moved` points, and let go there, each step a
/// headset pass with the window's frame after it.
fn press_between_window_frames(h: &mut Harness, ctx: &egui::Context, lit: &mut Vec<String>, at: egui::Pos2, moved: egui::Vec2) {
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    pass_and_window(h, ctx, lit, vec![egui::Event::PointerMoved(at)]);
    pass_and_window(h, ctx, lit, vec![button(at, true)]);
    pass_and_window(h, ctx, lit, Vec::new());
    let end = at + moved;
    pass_and_window(h, ctx, lit, vec![egui::Event::PointerMoved(end)]);
    pass_and_window(h, ctx, lit, Vec::new());
    pass_and_window(h, ctx, lit, vec![button(end, false)]);
    pass_and_window(h, ctx, lit, Vec::new());
}

/// The window's frame runs between the headset's: an eye clicked in the headset's Browser still
/// hides its body, and another click shows it again.
#[test]
fn an_eye_in_the_headsets_browser_answers_with_the_windows_frames_between() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    let at = headset_eye(&mut h, &ctx, &mut lit, "Two");
    press_between_window_frames(&mut h, &ctx, &mut lit, at, egui::Vec2::ZERO);
    assert_eq!(h.app.session.visibility.hidden_bodies, vec!["Two".to_string()]);
    let at = headset_eye(&mut h, &ctx, &mut lit, "Two");
    press_between_window_frames(&mut h, &ctx, &mut lit, at, egui::Vec2::ZERO);
    assert!(h.app.session.visibility.hidden_bodies.is_empty(), "shown again: {:?}", h.app.session.visibility.hidden_bodies);
}

/// A press on an eye that turns into a drag, with the window's frames between: the window does
/// not end the headset's drag as its own, so the drag ends when Select comes up and the next
/// click on an eye works.
#[test]
fn after_a_drag_over_the_headsets_eyes_the_next_click_still_works() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    let at = headset_eye(&mut h, &ctx, &mut lit, "Two");
    press_between_window_frames(&mut h, &ctx, &mut lit, at, egui::vec2(60.0, 0.0));
    assert!(ctx.data(|d| d.get_temp::<bool>(crate::ui_kit::eye_drag_id())).is_none(), "the drag ended in the headset's context");
    let at = headset_eye(&mut h, &ctx, &mut lit, "One");
    press_between_window_frames(&mut h, &ctx, &mut lit, at, egui::Vec2::ZERO);
    assert!(h.app.session.visibility.hidden_bodies.iter().any(|b| b == "One"), "hidden: {:?}", h.app.session.visibility.hidden_bodies);
}

/// The headset's Browser closes while Select drags over its eyes: the drag ends in the headset's
/// context, so the Browser opened again takes clicks.
#[test]
fn closing_the_headsets_browser_ends_its_eye_drag() {
    let mut h = two_bodies();
    let ctx = egui::Context::default();
    let mut lit = Vec::new();
    let at = headset_eye(&mut h, &ctx, &mut lit, "Two");
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    pass_and_window(&mut h, &ctx, &mut lit, vec![egui::Event::PointerMoved(at)]);
    pass_and_window(&mut h, &ctx, &mut lit, vec![button(at, true)]);
    pass_and_window(&mut h, &ctx, &mut lit, vec![egui::Event::PointerMoved(at + egui::vec2(60.0, 0.0))]);
    assert!(ctx.data(|d| d.get_temp::<bool>(crate::ui_kit::eye_drag_id())).is_some(), "an eye drag is under way");
    crate::vr_browser::close(&mut h.app);
    assert!(ctx.data(|d| d.get_temp::<bool>(crate::ui_kit::eye_drag_id())).is_none(), "ended through the headset's context");
    h.frame();
    assert!(ctx.data(|d| d.get_temp::<bool>(crate::ui_kit::eye_drag_id())).is_none());
}

/// The headset's Browser driven as the app drives it: each pass at the frame's clock, with the
/// window's frame after it, and places on it read right after its own pass.
struct Headset {
    ctx: egui::Context,
    lit: Vec<String>,
    /// The frame's clock (seconds).
    t: f64,
    /// Where the pointer is, while the ray is on the window.
    at: Option<egui::Pos2>,
    /// Select is held on the window.
    down: bool,
    /// What the window's closing tells egui, given with its next pass.
    queued: Vec<egui::Event>,
}

impl Headset {
    /// A headset's Browser whose clock starts at 1 s.
    fn new() -> Headset {
        Headset { ctx: egui::Context::default(), lit: Vec::new(), t: 1.0, at: None, down: false, queued: Vec::new() }
    }

    /// One pass `dt` seconds after the last, given `events`, without the window's frame after it.
    fn alone(&mut self, h: &mut Harness, events: Vec<egui::Event>, dt: f64) {
        self.t += dt;
        let events = std::mem::take(&mut self.queued).into_iter().chain(events).collect();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 480.0))),
            time: Some(self.t),
            events,
            focused: true,
            ..Default::default()
        };
        let mut out = self.ctx.run_ui(input, |ui| crate::vr_browser::show(&mut h.app, ui, &mut self.lit));
        out.textures_delta.clear();
    }

    /// One pass `dt` seconds after the last, given `events`, then the window's frame.
    fn pass(&mut self, h: &mut Harness, events: Vec<egui::Event>, dt: f64) {
        self.alone(h, events, dt);
        h.frame();
    }

    /// Where the headset's Browser draws `handle` (`eye:One`, `row:Two`), `None` when it draws
    /// none; read right after its own pass, as the window's frame publishes its own places under
    /// the same names.
    fn find(&mut self, h: &mut Harness, handle: &str) -> Option<egui::Pos2> {
        self.alone(h, Vec::new(), 0.02);
        let at = solidcraft_ui_registry::handle_at(handle);
        h.frame();
        at
    }

    /// [`Headset::find`], which must find it.
    fn place(&mut self, h: &mut Harness, handle: &str) -> egui::Pos2 {
        self.find(h, handle).unwrap_or_else(|| panic!("the headset's Browser draws no {handle}"))
    }

    /// The ray moves to `at`.
    fn point(&mut self, h: &mut Harness, at: egui::Pos2) {
        self.at = Some(at);
        self.pass(h, vec![egui::Event::PointerMoved(at)], 0.02);
    }

    /// Select pressed (`true`) or let go where the pointer is.
    /// As the app's pointer does, nothing is told while the ray is off the window.
    fn select(&mut self, h: &mut Harness, pressed: bool) {
        self.down = pressed;
        let events =
            self.at.map(|pos| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        self.pass(h, events.into_iter().collect(), 0.02);
    }

    /// A click at `at`, then `after` seconds with nothing given.
    fn click(&mut self, h: &mut Harness, at: egui::Pos2, after: f64) {
        self.point(h, at);
        self.select(h, true);
        self.select(h, false);
        self.idle(h, after);
    }

    /// `seconds` of passes every 0.1 s with nothing given.
    fn idle(&mut self, h: &mut Harness, seconds: f64) {
        let mut left = seconds;
        while left > 0.0 {
            self.pass(h, Vec::new(), left.min(0.1));
            left -= 0.1;
        }
    }

    /// Select held from `from` to `to`, a few steps, and let go there.
    fn drag(&mut self, h: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
        self.point(h, from);
        self.select(h, true);
        for k in 1..=6 {
            self.point(h, from + (to - from) * (k as f32 / 6.0));
        }
        self.select(h, false);
        self.idle(h, 0.2);
    }

    /// The window closes for `frames` of the window's frames and opens again, as the app's
    /// `Panel::close` does it: Select held lets go where the pointer is, the pointer is gone
    /// (both told with the next pass), and the Browser ends what it held.
    fn close(&mut self, h: &mut Harness, frames: usize) {
        if let (true, Some(pos)) = (self.down, self.at) {
            self.queued.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        if self.at.take().is_some() {
            self.queued.push(egui::Event::PointerGone);
        }
        self.down = false;
        for _ in 0..frames.max(1) {
            crate::vr_browser::close(&mut h.app);
            h.frame();
        }
        self.idle(h, 0.3);
    }

    /// The ray leaves the window.
    fn leave(&mut self, h: &mut Harness) {
        self.at = None;
        self.pass(h, vec![egui::Event::PointerGone], 0.02);
    }
}

/// Whether the headset's Browser still answers: a click on One's eye flips it, a click on the
/// Bodies folder's fold flips it, and a click on Two's row selects Two. Each is put back after.
fn answers(h: &mut Harness, hs: &mut Headset, after: &str) {
    let hidden = |h: &Harness| h.app.session.visibility.hidden_bodies.iter().any(|b| b == "One");
    let was = hidden(h);
    let at = hs.place(h, "eye:One");
    hs.click(h, at, 0.6);
    assert_ne!(hidden(h), was, "after {after}: One's eye answers");
    let at = hs.place(h, "eye:One");
    hs.click(h, at, 0.6);
    assert_eq!(hidden(h), was, "after {after}: One's eye answers again");
    let fold = solidcraft_ui_registry::handles("fold:").into_iter().map(|(name, _)| name).next();
    if let Some(fold) = fold {
        let at = hs.place(h, &fold);
        let rows = |h: &mut Harness, hs: &mut Headset| {
            hs.alone(h, Vec::new(), 0.02);
            let n = solidcraft_ui_registry::handles("row:").len();
            h.frame();
            n
        };
        let before = rows(h, hs);
        hs.click(h, at, 0.6);
        let after_fold = rows(h, hs);
        assert_ne!(after_fold, before, "after {after}: the fold {fold} answers");
        let at = hs.place(h, &fold);
        hs.click(h, at, 0.6);
    }
    let at = hs.place(h, "row:Two");
    hs.click(h, at, 0.6);
    assert!(
        h.app.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::Body { name } if name == "Two")),
        "after {after}: Two's row selects it: {:?}",
        h.app.session.selection
    );
    assert!(h.app.menu.rename.is_none(), "after {after}: no name editor is left open");
}

/// After every gesture the ray can make on the headset's Browser (clicks quick and slow, drags of
/// rows and over the eyes, Select let go off the window, the wheel, the window closed while Select
/// is held, other designs opened), its eyes, folds and rows still answer: input left behind by
/// one gesture must never leave the whole window dead.
#[test]
fn after_every_gesture_the_headsets_browser_still_answers() {
    type Gesture = fn(&mut Harness, &mut Headset);
    let gestures: [(&str, Gesture); 14] = [
        ("the window closed while an eye drag is under way", |h, hs| {
            let (from, to) = (hs.place(h, "eye:One"), hs.place(h, "eye:Two"));
            hs.point(h, from);
            hs.select(h, true);
            hs.point(h, to);
            hs.close(h, 3);
            h.app.session.visibility.hidden_bodies.clear();
        }),
        ("the window closed while a row is pressed", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.point(h, at);
            hs.select(h, true);
            hs.close(h, 1);
        }),
        ("a new tab with its own bodies", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.click(h, at, 0.6);
            crate::documents::new_tab(&mut h.app);
            for (name, x) in [("One", 0), ("Two", 20)] {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
            }
            h.settle();
            hs.idle(h, 0.3);
        }),
        ("a new tab while a row is pressed", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.point(h, at);
            hs.select(h, true);
            crate::documents::new_tab(&mut h.app);
            for (name, x) in [("One", 0), ("Two", 20)] {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","body_name":name})).unwrap();
            }
            h.settle();
            hs.select(h, false);
            hs.idle(h, 0.3);
        }),
        ("a tab switched away and back", |h, hs| {
            crate::documents::new_tab(&mut h.app);
            h.settle();
            hs.idle(h, 0.3);
            crate::documents::switch(&mut h.app, 0);
            h.settle();
            hs.idle(h, 0.3);
        }),
        ("nothing", |_, _| {}),
        ("a click on a row", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.click(h, at, 0.6);
        }),
        ("a slow second click on a name", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.click(h, at, 0.8);
            hs.click(h, at, 1.0);
        }),
        ("a double click on a row", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.click(h, at, 0.05);
            hs.click(h, at, 0.6);
        }),
        ("a row dragged onto another", |h, hs| {
            let (from, to) = (hs.place(h, "row:One"), hs.place(h, "row:Two"));
            hs.drag(h, from, to);
        }),
        ("a drag over the eyes", |h, hs| {
            let (from, to) = (hs.place(h, "eye:One"), hs.place(h, "eye:Two"));
            hs.drag(h, from, to);
            let (from, to) = (hs.place(h, "eye:One"), hs.place(h, "eye:Two"));
            hs.drag(h, from, to);
        }),
        ("an eye pressed, the ray off the window, let go there", |h, hs| {
            let at = hs.place(h, "eye:One");
            hs.point(h, at);
            hs.select(h, true);
            hs.leave(h);
            hs.select(h, false);
            hs.idle(h, 0.3);
            let at = hs.place(h, "eye:One");
            let hidden = h.app.session.visibility.hidden_bodies.iter().any(|b| b == "One");
            if hidden {
                hs.click(h, at, 0.6);
            }
        }),
        ("a row pressed, the ray off the window, let go there", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.point(h, at);
            hs.select(h, true);
            hs.point(h, at + egui::vec2(0.0, 40.0));
            hs.leave(h);
            hs.select(h, false);
            hs.idle(h, 0.3);
        }),
        ("the wheel", |h, hs| {
            let at = hs.place(h, "row:One");
            hs.point(h, at);
            let wheel = |d: f32| egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, d),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            };
            hs.pass(h, vec![wheel(-40.0)], 0.02);
            hs.pass(h, vec![wheel(40.0)], 0.02);
            hs.idle(h, 0.3);
        }),
    ];
    let mut failed = Vec::new();
    for (name, gesture) in gestures {
        let mut h = two_bodies();
        let mut hs = Headset::new();
        gesture(&mut h, &mut hs);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answers(&mut h, &mut hs, name)));
        if r.is_err() {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

/// A slow second click on a name in the headset's Browser, where there is no keyboard to type a
/// name with, opens no name editor: the row is picked again.
#[test]
fn a_slow_second_click_in_the_headsets_browser_opens_no_name_editor() {
    let mut h = two_bodies();
    let mut hs = Headset::new();
    let at = hs.place(&mut h, "row:Two");
    hs.click(&mut h, at, 0.8);
    hs.click(&mut h, at, 1.0);
    assert!(h.app.menu.rename.is_none(), "no name editor: {:?}", h.app.menu.rename.as_ref().map(|r| r.text.clone()));
    assert!(
        h.app.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::Body { name } if name == "Two")),
        "{:?}",
        h.app.session.selection
    );
}

/// A body and a placed part `Part` holding a body of its own, the root active and the Browser's
/// components shown; the placement's id.
fn body_and_part() -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.preferences.browser_components_open = true;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Top"})).unwrap();
    let part = h.app.run("component.create", json!({"name":"Part"})).unwrap()["occurrence"].as_u64().unwrap();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[20,0,0],"body_name":"Inner"})).unwrap();
    h.app.run("component.activate", json!({"component":0})).unwrap();
    h.settle();
    (h, part)
}

/// The headset's Browser only views the design: a double click on a body or a placed part is a
/// plain click there. It selects, and opens no name editor, dialog or edit in place, and changes
/// nothing in the design.
#[test]
fn a_double_click_in_the_headsets_browser_only_selects() {
    let (mut h, part) = body_and_part();
    let mut hs = Headset::new();
    let rows: Vec<String> = {
        hs.alone(&mut h, Vec::new(), 0.02);
        let rows = solidcraft_ui_registry::handles("row:").into_iter().map(|(name, _)| name).collect();
        h.frame();
        rows
    };
    let placement = rows.iter().find(|r| r.contains(&part.to_string()) || r.ends_with(":Part") || r.as_str() == "row:Part").cloned();
    let placement = placement.unwrap_or_else(|| panic!("a row for the placement in {rows:?}"));
    let doc = h.app.session.doc.clone();
    let active = h.app.session.active_component;
    for row in ["row:Top", placement.as_str()] {
        let at = hs.place(&mut h, row);
        hs.click(&mut h, at, 0.05);
        hs.click(&mut h, at, 0.6);
        assert!(h.app.menu.rename.is_none(), "{row}: no name editor");
        assert!(h.app.dialog.is_none(), "{row}: no dialog");
        assert!(h.app.tool.is_none(), "{row}: no tool");
        assert_eq!(h.app.session.active_component, active, "{row}: no edit in place");
        assert!(h.app.session.doc == doc, "{row}: the design is unchanged");
    }
    assert!(!h.app.session.selection.is_empty() || !h.app.tree.picked_components.is_empty(), "the double clicks selected");
}

/// No row is dragged in the headset's Browser: a body dragged onto a placed part stays where it
/// is, and the design is unchanged.
#[test]
fn a_row_dragged_in_the_headsets_browser_moves_nothing() {
    let (mut h, part) = body_and_part();
    let mut hs = Headset::new();
    let rows: Vec<String> = {
        hs.alone(&mut h, Vec::new(), 0.02);
        let rows = solidcraft_ui_registry::handles("row:").into_iter().map(|(name, _)| name).collect();
        h.frame();
        rows
    };
    let placement = rows.iter().find(|r| r.contains(&part.to_string()) || r.ends_with(":Part") || r.as_str() == "row:Part").cloned();
    let placement = placement.unwrap_or_else(|| panic!("a row for the placement in {rows:?}"));
    let doc = h.app.session.doc.clone();
    let (from, to) = (hs.place(&mut h, "row:Top"), hs.place(&mut h, &placement));
    hs.drag(&mut h, from, to);
    h.settle();
    assert!(h.app.session.doc == doc, "the design is unchanged");
}

/// With the history in the Browser, the headset's Browser still only views the design: the
/// extrusion's Edit Profile Sketch button opens no sketch, the history marker dragged above the
/// extrusion rolls nothing back, a feature row dragged above another moves nothing, and Document
/// Settings' Units, Precision, Parameters and Design Properties rows open no menu, Preferences,
/// tool or dialog.
#[test]
fn the_headsets_browser_edits_nothing_from_the_history_or_document_settings() {
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,15]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    let pad = h.app.run("solid.extrude", json!({"distance":10,"name":"Pad","body_name":"Plate"})).unwrap()["feature"].as_u64().unwrap();
    let block =
        h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"name":"Block","body_name":"Block"})).unwrap()["feature"].as_u64().unwrap();
    h.app.run("view.history_layout", json!({"dock":"browser"})).unwrap();
    h.settle();
    let doc = h.app.session.doc.clone();
    let mut hs = Headset::new();

    let button = hs.place(&mut h, "action:Pad");
    hs.click(&mut h, button, 0.6);
    assert_eq!(h.app.session.active_sketch, None, "Edit Profile Sketch opened the sketch");

    let pad_row = hs.place(&mut h, &format!("row:browser-feature:c0:{pad}"));
    let marker = hs.place(&mut h, "browser:rollback");
    hs.drag(&mut h, marker, egui::pos2(pad_row.x, pad_row.y - 8.0));
    h.settle();
    let block_row = hs.place(&mut h, &format!("row:browser-feature:c0:{block}"));
    hs.drag(&mut h, block_row, egui::pos2(pad_row.x, pad_row.y - 8.0));
    h.settle();
    assert!(h.app.session.doc == doc, "the history was rolled or reordered: marker {:?}", h.app.session.doc.marker);

    let settings = hs.place(&mut h, "fold:Document Settings");
    hs.click(&mut h, settings, 0.6);
    let rows: Vec<String> = {
        hs.alone(&mut h, Vec::new(), 0.02);
        let rows = solidcraft_ui_registry::handles("row:").into_iter().map(|(name, _)| name).collect();
        h.frame();
        rows
    };
    let wanted = ["row:Units", "row:Precision", "row:Parameters", "row:Design Properties"];
    for start in wanted {
        let row = rows.iter().find(|r| r.starts_with(start)).unwrap_or_else(|| panic!("{start} among {rows:?}")).clone();
        let at = hs.place(&mut h, &row);
        hs.click(&mut h, at, 0.6);
        assert!(h.app.viewport.context_menu.is_none(), "{row}: a menu opened");
        assert!(!h.app.prefs_window.shell.open, "{row}: Preferences opened");
        assert!(h.app.params.is_none() && h.app.tool.is_none() && h.app.dialog.is_none(), "{row}: a tool or dialog opened");
        assert!(h.app.design_properties.is_none(), "{row}: Design Properties opened");
    }
    assert!(h.app.session.doc == doc, "the design is unchanged");
}
