//! The one-time question for browser users with a smooth-scrolling mouse wheel.
//!
//! Browsers expose no device identity, and a smooth wheel sends the small, evenly spaced vertical
//! steps a two-finger swipe does, so the viewport pans where the user expects a zoom. The default
//! stays two-finger pan (what most browser users, on trackpads, want). When the first scrolls in the
//! viewport look like a perfectly steady vertical wheel, a small notice asks once whether to make
//! the wheel zoom (Preferences › Navigation › Scroll wheel in the browser, Always zoom), or never to
//! ask again. Nothing here changes how the view moves until the user says so.
//!
//! The browser's own hints decide first (`hint`): a wheel that reports `wheelDeltaY` in multiples
//! of 120 that are not `-3 × deltaY` is a notched wheel. A line-mode wheel and steps of at least
//! 40 points already zoom, so they are never asked about. What is left is the step-timing check.

use crate::SolveApp;
use crate::prefs::BrowserScrollWheel;

/// One bare-or-modified `wheel` event as the page saw it (`apps/solidcraft-web`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelSample {
    /// `deltaMode` is lines or pages: a notched wheel, which zooms already.
    pub line: bool,
    pub delta_x: f64,
    pub delta_y: f64,
    /// The legacy `wheelDeltaY`, when the browser has it.
    pub wheel_delta_y: Option<f64>,
    /// No Ctrl, Shift, Alt or Meta held.
    pub bare: bool,
    /// The event's own time, in seconds (not the frame's: events batch into frames).
    pub time: f64,
}

/// What the first scrolls look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A steady vertical wheel.
    Wheel,
    /// Anything else: a swipe, momentum, or too irregular to say.
    Swipe,
}

/// A step this large (in points) zooms already; the same limit as `WebWheelGestures`.
const NOTCH: f64 = 40.0;
/// Steps that make up the timing check.
const STEPS: usize = 4;
/// A gap this long between two events ends a scroll.
const IDLE: f64 = 0.1;
/// Scrolls looked at before giving up: a trackpad user is never asked.
const MAX_SCROLLS: u8 = 3;
/// How long the notice stays when nobody points at it.
pub(crate) const SHOW_SECONDS: f64 = 30.0;

/// What the browser's hints say about a small point-mode step, before any timing.
pub fn hint(delta_y: f64, wheel_delta_y: Option<f64>) -> Option<Verdict> {
    let legacy = wheel_delta_y?;
    // A trackpad reports `-3 × deltaY`; a notched wheel reports whole multiples of 120 that differ.
    let notched = legacy != 0.0 && legacy % 120.0 == 0.0 && (legacy - -3.0 * delta_y).abs() > 1e-6;
    notched.then_some(Verdict::Wheel)
}

/// The timing check over a scroll's first steps: same direction and within 10% of the first step's
/// size, spaced 4 to 40 ms apart within 20% of the first gap.
fn steady(steps: &[(f64, f64)]) -> bool {
    let Some(&(t0, first)) = steps.first() else { return false };
    let mut gap0: Option<f64> = None;
    let mut last = t0;
    for &(time, dy) in &steps[1..] {
        let gap = time - last;
        last = time;
        let ok = (0.004..=0.040).contains(&gap) && gap0.is_none_or(|g| (gap - g).abs() <= g * 0.2);
        gap0.get_or_insert(gap);
        if !ok || (dy - first).abs() > first.abs() * 0.1 {
            return false;
        }
    }
    true
}

/// Reads wheel samples and says once what the first scrolls look like.
#[derive(Clone, Debug, Default)]
pub struct Detector {
    steps: Vec<(f64, f64)>,
    last: Option<f64>,
    /// This scroll has been judged; the rest of it (momentum) is ignored.
    judged: bool,
    swipes: u8,
}

impl Detector {
    /// Whether it has seen enough swipes to stop looking.
    pub fn gave_up(&self) -> bool {
        self.swipes >= MAX_SCROLLS
    }

    fn end_scroll(&mut self) {
        if !self.steps.is_empty() && !self.judged {
            self.swipes = self.swipes.saturating_add(1);
        }
        self.steps.clear();
        self.last = None;
        self.judged = false;
    }

    /// Note one event; `Some` once per scroll, when it is judged.
    pub fn observe(&mut self, sample: WheelSample) -> Option<Verdict> {
        if !sample.time.is_finite() || self.gave_up() {
            return None;
        }
        if self.last.is_some_and(|last| sample.time < last || sample.time - last >= IDLE) {
            self.end_scroll();
            if self.gave_up() {
                return None;
            }
        }
        // Line-mode wheels, large steps, modifiers and sideways drift zoom or pan by their own
        // rules: they say nothing about a smooth wheel, and a scroll that has any is not steady.
        let small = !sample.line && sample.delta_y.is_finite() && sample.delta_y != 0.0 && sample.delta_y.abs() < NOTCH;
        if !small || !sample.bare || sample.delta_x != 0.0 {
            if !self.judged && !self.steps.is_empty() {
                self.judged = true;
                self.swipes = self.swipes.saturating_add(1);
                self.last = Some(sample.time);
                return Some(Verdict::Swipe);
            }
            self.last = self.judged.then_some(sample.time);
            return None;
        }
        self.last = Some(sample.time);
        if self.judged {
            return None;
        }
        if let Some(verdict) = hint(sample.delta_y, sample.wheel_delta_y) {
            self.judged = true;
            return Some(verdict);
        }
        // Events batched into one frame by the browser share a timestamp: one timing sample.
        if self.steps.last().is_some_and(|&(time, _)| time == sample.time) {
            return None;
        }
        self.steps.push((sample.time, sample.delta_y));
        if !steady(&self.steps) {
            self.judged = true;
            self.swipes = self.swipes.saturating_add(1);
            return Some(Verdict::Swipe);
        }
        if self.steps.len() >= STEPS {
            self.judged = true;
            return Some(Verdict::Wheel);
        }
        None
    }
}

/// The notice's state, on the app.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub(crate) detector: Detector,
    /// A steady wheel was seen: the notice is due.
    pub(crate) wanted: bool,
    /// The notice timed out this run (nothing is saved, so it can come back next start).
    pub(crate) hidden: bool,
    pub(crate) since: Option<f64>,
}

/// What the notice's buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Sets the wheel to Always zoom.
    Zoom,
    /// Never asks again: the second button and the close (x).
    Dismiss,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zoom => "ui.browser_wheel_hint_zoom",
            Self::Dismiss => "ui.browser_wheel_hint_dismiss",
        }
    }

    pub(crate) fn kind(self) -> crate::viewport::ToastKind {
        match self {
            Self::Zoom => crate::viewport::ToastKind::Primary,
            Self::Dismiss => crate::viewport::ToastKind::Dismiss,
        }
    }
}

/// Handles of the notice for scripts: its rect, its buttons in order and its close.
pub const HANDLES: &[&str] = &["toast:wheel_hint", "toast:wheel_hint:zoom", "toast:wheel_hint:dismiss"];

/// Whether the question may still be asked: automatic routing is chosen and it was not dismissed.
fn askable(app: &SolveApp) -> bool {
    app.preferences.browser_scroll_wheel == BrowserScrollWheel::Automatic && !app.preferences.browser_wheel_hint_dismissed
}

/// Feed this frame's wheel samples, the pointer being over the viewport for each.
pub fn feed(app: &mut SolveApp, ctx: &egui::Context, samples: &[WheelSample]) {
    if samples.is_empty() || !askable(app) || app.wheel_hint.wanted {
        return;
    }
    let over_viewport = match (app.viewport.rect, ctx.input(|i| i.pointer.hover_pos())) {
        (Some(rect), Some(pos)) => rect.contains(pos),
        _ => false,
    };
    for sample in samples {
        if over_viewport && app.wheel_hint.detector.observe(*sample) == Some(Verdict::Wheel) {
            app.wheel_hint.wanted = true;
            return;
        }
    }
}

/// Whether the notice is up or waiting, for the idle check of the toast layout.
pub(crate) fn pending(app: &SolveApp) -> bool {
    app.wheel_hint.wanted && !app.wheel_hint.hidden && askable(app)
}

/// The notice due this frame, as its text, for the shared toast to draw with `Action`'s buttons.
/// It goes by itself after `SHOW_SECONDS` unless the pointer is on it (`keep`), for this run only.
pub(crate) fn due(app: &mut SolveApp, ctx: &egui::Context) -> Option<String> {
    if !pending(app) || !ctx.input(|i| i.focused) {
        return None;
    }
    let now = ctx.input(|i| i.time);
    let since = *app.wheel_hint.since.get_or_insert(now);
    if !(now - since).is_finite() || now - since >= SHOW_SECONDS {
        app.wheel_hint.hidden = true;
        return None;
    }
    Some(solidcraft_ui_text::tr("ui.browser_wheel_hint").to_string())
}

/// The share of the notice's time left, 1 to 0, for the ring around its close (x).
pub(crate) fn left(app: &SolveApp, ctx: &egui::Context) -> f32 {
    let now = ctx.input(|i| i.time);
    app.wheel_hint.since.map_or(1.0, |since| crate::viewport::countdown_left(since, now, SHOW_SECONDS))
}

/// The notice was drawn at `rect`: a pointer on it keeps it up.
pub(crate) fn keep(app: &mut SolveApp, ctx: &egui::Context, rect: egui::Rect) {
    if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|pos| rect.contains(pos)) {
        app.wheel_hint.since = Some(ctx.input(|i| i.time));
    }
}

/// The user took `action`. Both are saved with the settings.
pub fn act(app: &mut SolveApp, action: Action) {
    if !pending(app) {
        return;
    }
    match action {
        Action::Zoom => app.preferences.browser_scroll_wheel = BrowserScrollWheel::AlwaysZoom,
        Action::Dismiss => app.preferences.browser_wheel_hint_dismissed = true,
    }
    app.wheel_hint = State::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(time: f64, dy: f64) -> WheelSample {
        WheelSample { line: false, delta_x: 0.0, delta_y: dy, wheel_delta_y: Some(-3.0 * dy), bare: true, time }
    }

    fn run(samples: impl IntoIterator<Item = WheelSample>) -> Vec<Verdict> {
        let mut detector = Detector::default();
        samples.into_iter().filter_map(|s| detector.observe(s)).collect()
    }

    #[test]
    fn four_steady_vertical_steps_look_like_a_wheel_once() {
        for dy in [3.0, -7.5, 20.0] {
            let verdicts = run((0..10).map(|i| step(f64::from(i) * 0.016, dy)));
            assert_eq!(verdicts, vec![Verdict::Wheel], "{dy}");
        }
        assert!(run((0..3).map(|i| step(f64::from(i) * 0.016, 3.0))).is_empty(), "three steps are not enough");
    }

    #[test]
    fn the_browsers_hints_decide_before_the_timing() {
        assert_eq!(hint(3.0, Some(-120.0)), Some(Verdict::Wheel));
        assert_eq!(hint(3.0, Some(-9.0)), None, "-3 x deltaY is what a trackpad reports");
        assert_eq!(hint(40.0, Some(-120.0)), None, "120 = -3 x -40 cannot tell");
        assert_eq!(hint(3.0, None), None);
        let sample = WheelSample { wheel_delta_y: Some(-120.0), ..step(0.0, 3.0) };
        assert_eq!(run([sample]), vec![Verdict::Wheel], "one step is enough when the hint says wheel");
    }

    #[test]
    fn trackpad_streams_are_swipes_and_never_ask() {
        let drifting = [(0.0, 8.0), (0.016, 8.0), (0.032, 6.0), (0.055, 3.0), (0.083, 1.0)];
        let uneven = [(0.0, 8.0), (0.016, 8.0), (0.045, 8.0), (0.070, 4.0)];
        for stream in [&drifting[..], &uneven[..]] {
            let verdicts = run(stream.iter().map(|&(t, dy)| step(t, dy)));
            assert_eq!(verdicts, vec![Verdict::Swipe]);
        }
        let mut sideways = step(0.016, 3.0);
        sideways.delta_x = 0.1;
        assert_eq!(run([step(0.0, 3.0), sideways]), vec![Verdict::Swipe]);
        let mut shifted = step(0.016, 3.0);
        shifted.bare = false;
        assert_eq!(run([step(0.0, 3.0), shifted]), vec![Verdict::Swipe]);
        // After three swipes it stops looking, even at a perfectly steady scroll.
        let mut detector = Detector::default();
        for scroll in 0..3 {
            let base = f64::from(scroll) * 1.0;
            assert_eq!(detector.observe(step(base, 8.0)), None);
            assert_eq!(detector.observe(step(base + 0.016, 2.0)), Some(Verdict::Swipe));
        }
        assert_eq!(detector.observe(step(5.0, 8.0)), None);
        assert!(detector.gave_up());
        assert!((0..8).all(|i| detector.observe(step(6.0 + f64::from(i) * 0.016, 3.0)).is_none()));
    }

    #[test]
    fn notches_line_steps_batched_events_and_bad_times_say_nothing() {
        let line = WheelSample { line: true, ..step(0.0, 3.0) };
        let notch = step(0.0, 100.0);
        assert!(run([line, notch]).is_empty(), "these zoom already");
        let batched = (0..8).map(|i| step(f64::from(i / 2) * 0.016, 3.0));
        assert_eq!(run(batched), vec![Verdict::Wheel], "events sharing a timestamp are one sample");
        for time in [f64::NAN, f64::INFINITY] {
            assert!(run([WheelSample { time, ..step(0.0, 3.0) }]).is_empty());
        }
        assert!(run((0..8).map(|i| step(f64::from(i) * 0.016, 0.0))).is_empty());
    }

    #[test]
    fn a_new_scroll_after_a_pause_is_judged_again() {
        let verdicts = run((0..6).map(|i| step(f64::from(i) * 0.016, 3.0)).chain((0..6).map(|i| step(1.0 + f64::from(i) * 0.016, 3.0))));
        assert_eq!(verdicts, vec![Verdict::Wheel, Verdict::Wheel]);
    }

    fn new_app() -> (SolveApp, egui::Context) {
        let ctx = egui::Context::default();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.viewport.rect = Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)));
        (app, ctx)
    }

    fn frame(ctx: &egui::Context, pointer: Option<egui::Pos2>, time: f64, run: impl FnOnce(&egui::Context)) {
        let mut raw = egui::RawInput { time: Some(time), focused: true, ..Default::default() };
        if let Some(pos) = pointer {
            raw.events.push(egui::Event::PointerMoved(pos));
        }
        let mut run = Some(run);
        let _ = ctx.run_ui(raw, |ui| {
            if let Some(run) = run.take() {
                run(ui.ctx());
            }
        });
    }

    #[test]
    fn the_notice_asks_once_then_zoom_or_dismiss_is_saved() {
        let samples: Vec<WheelSample> = (0..6).map(|i| step(f64::from(i) * 0.016, 3.0)).collect();
        // Outside the viewport nothing is judged.
        let (mut app, ctx) = new_app();
        frame(&ctx, Some(egui::pos2(900.0, 100.0)), 0.0, |ctx| feed(&mut app, ctx, &samples));
        assert!(!pending(&app));
        // Over it, a steady wheel makes the notice due, with the default still two-finger pan.
        frame(&ctx, Some(egui::pos2(100.0, 100.0)), 0.1, |ctx| feed(&mut app, ctx, &samples));
        assert!(pending(&app));
        assert_eq!(app.preferences.browser_scroll_wheel, BrowserScrollWheel::Automatic);
        let mut text = None;
        frame(&ctx, None, 0.2, |ctx| text = due(&mut app, ctx));
        assert_eq!(text.as_deref(), Some(solidcraft_ui_text::tr("ui.browser_wheel_hint")));
        // Make the wheel zoom: saved, and never asked again.
        act(&mut app, Action::Zoom);
        assert_eq!(app.preferences.browser_scroll_wheel, BrowserScrollWheel::AlwaysZoom);
        assert!(!pending(&app) && !app.preferences.browser_wheel_hint_dismissed);
        frame(&ctx, Some(egui::pos2(100.0, 100.0)), 0.5, |ctx| feed(&mut app, ctx, &samples));
        assert!(!pending(&app), "Always zoom is not asked about");
        // Dismiss: saved, the choice stays Automatic, and a steady wheel never asks again.
        let (mut app, ctx) = new_app();
        frame(&ctx, Some(egui::pos2(100.0, 100.0)), 0.1, |ctx| feed(&mut app, ctx, &samples));
        act(&mut app, Action::Dismiss);
        assert!(app.preferences.browser_wheel_hint_dismissed);
        assert_eq!(app.preferences.browser_scroll_wheel, BrowserScrollWheel::Automatic);
        frame(&ctx, Some(egui::pos2(100.0, 100.0)), 0.5, |ctx| feed(&mut app, ctx, &samples));
        assert!(!pending(&app));
        let restored: crate::prefs::Prefs = serde_json::from_value(serde_json::to_value(&app.preferences).unwrap()).unwrap();
        assert!(restored.browser_wheel_hint_dismissed, "it is saved with the settings");
        // Without an answer it times out for this run only.
        let (mut app, ctx) = new_app();
        frame(&ctx, Some(egui::pos2(100.0, 100.0)), 0.1, |ctx| feed(&mut app, ctx, &samples));
        frame(&ctx, None, 1.0, |ctx| assert!(due(&mut app, ctx).is_some()));
        frame(&ctx, None, 1.0 + SHOW_SECONDS, |ctx| assert!(due(&mut app, ctx).is_none()));
        assert!(!pending(&app) && !app.preferences.browser_wheel_hint_dismissed);
    }
}
