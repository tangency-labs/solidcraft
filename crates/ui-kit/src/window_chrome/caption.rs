//! A window's title bar as the system's hit test needs it, and what the system's side reports
//! back.
//!
//! On Windows the system asks a window what is under the pointer at any time (`WM_NCHITTEST`).
//! Every window that draws the app's title bar publishes the bar it laid out ([`CaptionLayout`])
//! under its native window handle, and one window procedure in the desktop app ([`Hook`])
//! answers the system from it: the bare bar is the system's caption (it drags the window,
//! maximizes on a double-click and snaps), the maximize button is the system's (Windows 11's snap
//! layouts) and the bar's controls stay the UI's.
//!
//! The system then keeps the pointer over those parts for itself, so the procedure reports what
//! the UI no longer sees: where the pointer is ([`pointer`]), a press on the caption or the
//! maximize button ([`pressed`]: to the UI the press on its bar that it is where the bar is the
//! UI's own, so whatever closes on a press elsewhere closes), a right-click on the caption
//! ([`secondary_click`]: the UI's own menu for the bar) and the maximize button held down
//! ([`set_maximize_pressed`]).
//!
//! Nothing here calls the system: it builds for every target. Coordinates are client-area
//! physical pixels (the window's top-left is 0,0). The windows live in one list behind a lock that
//! is never held across a call out of this module.

use egui::{Context, ViewportId};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// A rectangle in client-area physical pixels, `min` inclusive, `max` exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PxRect {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl PxRect {
    pub const fn new(min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> Self {
        Self { min_x, min_y, max_x, max_y }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.min_x && x < self.max_x && y >= self.min_y && y < self.max_y
    }
}

/// What a window's title bar has at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionHit {
    /// Bare title bar: the system drags the window from here (its own move loop, a double-click
    /// to maximize, snapping).
    Drag,
    /// A control the UI handles itself (tabs, menus, search, minimize, close), or something lying
    /// over the bar (a menu, a popup).
    Client,
    /// The maximize button: the system shows its snap layouts here (Windows 11).
    MaximizeButton,
    /// Not part of the title bar.
    Outside,
}

/// A window's title bar, as its UI laid it out in one pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptionLayout {
    /// The whole bar. What neither `interactive` nor `maximize` covers drags the window.
    pub bar: PxRect,
    /// The bar's controls and whatever lies over the bar: the UI's.
    pub interactive: Vec<PxRect>,
    /// The maximize button, when the bar has one.
    pub maximize: Option<PxRect>,
}

impl CaptionLayout {
    /// What the bar has at client pixel (`x`, `y`). A control wins over the maximize button (a
    /// menu may lie over it), and the button over the bare bar.
    pub fn hit(&self, x: i32, y: i32) -> CaptionHit {
        if !self.bar.contains(x, y) {
            CaptionHit::Outside
        } else if self.interactive.iter().any(|r| r.contains(x, y)) {
            CaptionHit::Client
        } else if self.maximize.is_some_and(|r| r.contains(x, y)) {
            CaptionHit::MaximizeButton
        } else {
            CaptionHit::Drag
        }
    }
}

/// The platform's side, where the process has one: answer the system for native window `window`
/// from the bar it publishes, from now on.
pub type Hook = fn(window: u64);

static HOOK: OnceLock<Hook> = OnceLock::new();

/// Let the windows of this process hand their title bars to the platform (the desktop app on
/// Windows, once; a second call changes nothing). `hook` is called the first time a window has a
/// bar, on the thread that draws it, and calls [`hooked`] once it answers the system for the
/// window. Without a hook every title bar stays the UI's own.
pub fn set_platform(hook: Hook) {
    let _ = HOOK.set(hook);
}

pub(super) fn platform() -> Option<Hook> {
    HOOK.get().copied()
}

/// Most right-clicks kept for a window whose UI has not asked for them yet.
const MAX_PENDING_CLICKS: usize = 4;

/// One native window that draws the app's title bar, or that the platform hooked.
struct Window {
    id: u64,
    /// Who draws it: where a repaint is asked for when the platform reports something.
    ui: Option<(Context, ViewportId)>,
    /// The bar its last pass laid out; `None` when that pass had no bar (full screen).
    layout: Option<CaptionLayout>,
    /// Of its last pass: the platform's pixels become the UI's points by it.
    pixels_per_point: f32,
    /// The platform was asked to hook it.
    asked: bool,
    /// The platform answers the system for it.
    hooked: bool,
    maximize_pressed: bool,
    /// Where the platform last saw the pointer over the caption or the top resize band.
    pointer: Option<(i32, i32)>,
    /// What the UI was last told of `pointer`.
    told: Option<(i32, i32)>,
    presses: Vec<(i32, i32)>,
    secondary_clicks: Vec<(i32, i32)>,
}

impl Window {
    fn new(id: u64) -> Self {
        Self {
            id,
            ui: None,
            layout: None,
            pixels_per_point: 1.0,
            asked: false,
            hooked: false,
            maximize_pressed: false,
            pointer: None,
            told: None,
            presses: Vec::new(),
            secondary_clicks: Vec::new(),
        }
    }
}

static WINDOWS: Mutex<Vec<Window>> = Mutex::new(Vec::new());

/// The list of windows. A panic while it was held leaves plain data, so a poisoned lock is used
/// as it is.
fn windows() -> MutexGuard<'static, Vec<Window>> {
    WINDOWS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Change the entry of `window`, if it has one, and ask its UI for a repaint when `change` says
/// something changed (after the lock is released).
fn report(window: u64, change: impl FnOnce(&mut Window) -> bool) {
    let ui = {
        let mut windows = windows();
        let Some(entry) = windows.iter_mut().find(|w| w.id == window) else { return };
        if !change(entry) {
            return;
        }
        entry.ui.clone()
    };
    if let Some((ctx, viewport)) = ui {
        ctx.request_repaint_of(viewport);
    }
}

// ---------------------------------------------------------------------------------------------
// The UI's side

/// The bar `window` laid out this pass (`None`: it laid out none), drawn by `viewport` of `ctx`.
/// `true` when the platform is to be asked to hook the window: once, the first time it has a bar.
pub(super) fn publish(window: u64, ctx: &Context, viewport: ViewportId, pixels_per_point: f32, layout: Option<CaptionLayout>) -> bool {
    let mut windows = windows();
    let index = match windows.iter().position(|w| w.id == window) {
        Some(index) => index,
        // A window that never draws the bar is none of this module's.
        None if layout.is_none() => return false,
        None => {
            windows.push(Window::new(window));
            windows.len() - 1
        }
    };
    let Some(entry) = windows.get_mut(index) else { return false };
    if entry.ui.as_ref().is_none_or(|(_, drawn_by)| *drawn_by != viewport) {
        entry.ui = Some((ctx.clone(), viewport));
    }
    if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        entry.pixels_per_point = pixels_per_point;
    }
    entry.layout = layout;
    let ask = entry.layout.is_some() && !entry.asked && !entry.hooked;
    entry.asked |= ask;
    ask
}

/// The platform answers the system for `window`: its title bar and frame are the system's.
pub fn is_hooked(window: u64) -> bool {
    windows().iter().any(|w| w.id == window && w.hooked)
}

/// The maximize button of `window` is held down over the system's caption.
pub(super) fn maximize_pressed(window: u64) -> bool {
    windows().iter().any(|w| w.id == window && w.maximize_pressed)
}

/// What the pointer over a window's caption did since the UI last asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PointerChange {
    Same,
    /// It is at this client pixel now.
    Moved((i32, i32)),
    /// It left the caption: for the client area, where the UI sees it again, or for elsewhere.
    Left,
}

/// What the platform reported for a window since its UI last asked.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Reported {
    pub pointer: PointerChange,
    /// Where the pointer is over the caption now, if it is there.
    pub pointer_at: Option<(i32, i32)>,
    /// Presses the system took on the caption or the maximize button, at their client pixels.
    pub presses: Vec<(i32, i32)>,
    /// Right-clicks on the caption, at their client pixels.
    pub secondary_clicks: Vec<(i32, i32)>,
    pub pixels_per_point: f32,
}

/// Take what the platform reported for `window` since the last call; `None` when there is
/// nothing new and the pointer is not over the caption.
pub(super) fn take_reported(window: u64) -> Option<Reported> {
    let mut windows = windows();
    let entry = windows.iter_mut().find(|w| w.id == window)?;
    let pointer = match (entry.pointer, entry.told) {
        (now, told) if now == told => PointerChange::Same,
        (Some(at), _) => PointerChange::Moved(at),
        (None, _) => PointerChange::Left,
    };
    entry.told = entry.pointer;
    let reported = Reported {
        pointer,
        pointer_at: entry.pointer,
        presses: std::mem::take(&mut entry.presses),
        secondary_clicks: std::mem::take(&mut entry.secondary_clicks),
        pixels_per_point: entry.pixels_per_point,
    };
    let news = reported.pointer != PointerChange::Same || !reported.presses.is_empty() || !reported.secondary_clicks.is_empty();
    (news || reported.pointer_at.is_some()).then_some(reported)
}

// ---------------------------------------------------------------------------------------------
// The platform's side

/// The platform's procedure answers the system for `window` from now on.
pub fn hooked(window: u64) {
    let mut windows = windows();
    match windows.iter_mut().find(|w| w.id == window) {
        Some(entry) => entry.hooked = true,
        None => {
            let mut entry = Window::new(window);
            entry.hooked = true;
            windows.push(entry);
        }
    }
}

/// What the title bar of `window` has at client pixel (`x`, `y`); [`CaptionHit::Outside`] for a
/// window with no bar.
pub fn hit(window: u64, x: i32, y: i32) -> CaptionHit {
    windows().iter().find(|w| w.id == window).and_then(|w| w.layout.as_ref()).map_or(CaptionHit::Outside, |layout| layout.hit(x, y))
}

/// The bar of `window` and its maximize button, for the system's question about the title bar's
/// geometry (`WM_GETTITLEBARINFOEX` on Windows).
pub fn title_bar(window: u64) -> Option<(PxRect, Option<PxRect>)> {
    windows().iter().find(|w| w.id == window).and_then(|w| w.layout.as_ref()).map(|layout| (layout.bar, layout.maximize))
}

/// Where the pointer is over the caption or the top resize band of `window`, in client pixels;
/// `None` once it is anywhere else. The UI sees it there as it does anywhere in the window.
pub fn pointer(window: u64, at: Option<(i32, i32)>) {
    report(window, |w| {
        let changed = w.ui.is_some() && w.pointer != at;
        if changed {
            w.pointer = at;
        }
        changed
    });
}

/// The maximize button of `window` is held down (`true`) or let go; what it was before.
pub fn set_maximize_pressed(window: u64, pressed: bool) -> bool {
    let mut was = false;
    report(window, |w| {
        was = w.maximize_pressed;
        w.maximize_pressed = pressed;
        was != pressed
    });
    was
}

/// The system took a press on the caption or the maximize button of `window`, at client pixel
/// (`x`, `y`).
pub fn pressed(window: u64, x: i32, y: i32) {
    report(window, |w| {
        let kept = w.ui.is_some() && w.presses.len() < MAX_PENDING_CLICKS;
        if kept {
            w.presses.push((x, y));
        }
        kept
    });
}

/// The right button was clicked on the caption of `window`, at client pixel (`x`, `y`).
pub fn secondary_click(window: u64, x: i32, y: i32) {
    report(window, |w| {
        let kept = w.ui.is_some() && w.secondary_clicks.len() < MAX_PENDING_CLICKS;
        if kept {
            w.secondary_clicks.push((x, y));
        }
        kept
    });
}

/// `window` is gone.
pub fn closed(window: u64) {
    windows().retain(|w| w.id != window);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each test has its own windows: the list is one per process.
    const BASE: u64 = 0xCA97_0000;

    fn layout() -> CaptionLayout {
        CaptionLayout {
            bar: PxRect::new(0, 0, 1000, 40),
            interactive: vec![PxRect::new(10, 5, 120, 35), PxRect::new(862, 0, 908, 40), PxRect::new(954, 0, 1000, 40)],
            maximize: Some(PxRect::new(908, 0, 954, 40)),
        }
    }

    #[test]
    fn regions_resolve_in_priority_order() {
        let l = layout();
        assert_eq!(l.hit(500, 20), CaptionHit::Drag, "the bare bar drags");
        assert_eq!(l.hit(50, 20), CaptionHit::Client, "a tab is the UI's");
        assert_eq!(l.hit(930, 20), CaptionHit::MaximizeButton);
        assert_eq!(l.hit(980, 20), CaptionHit::Client, "close is the UI's");
        assert_eq!(l.hit(500, 40), CaptionHit::Outside, "max is exclusive: below the bar");
        assert_eq!(l.hit(-1, 20), CaptionHit::Outside);
        // A menu lying over the maximize button takes it.
        let covered = CaptionLayout { interactive: vec![PxRect::new(900, 0, 1000, 300)], ..layout() };
        assert_eq!(covered.hit(930, 20), CaptionHit::Client);
    }

    #[test]
    fn each_window_has_its_own_bar_and_none_until_it_draws_one() {
        let ctx = Context::default();
        let (main, pop_out) = (BASE + 1, BASE + 2);
        assert_eq!(hit(main, 500, 20), CaptionHit::Outside, "nothing published");
        assert!(!publish(main, &ctx, ViewportId::ROOT, 1.0, None), "a window without a bar is not tracked");
        assert!(publish(main, &ctx, ViewportId::ROOT, 1.0, Some(layout())), "the first bar: the platform is asked to hook it");
        assert!(!publish(main, &ctx, ViewportId::ROOT, 1.0, Some(layout())), "asked once");
        let small = CaptionLayout { bar: PxRect::new(0, 0, 480, 34), ..Default::default() };
        assert!(publish(pop_out, &ctx, ViewportId::from_hash_of("pop-out"), 1.0, Some(small.clone())));
        assert_eq!(hit(main, 930, 20), CaptionHit::MaximizeButton);
        assert_eq!(hit(pop_out, 930, 20), CaptionHit::Outside, "the pop-out's bar is its own, 480 wide");
        assert_eq!(hit(pop_out, 200, 20), CaptionHit::Drag);
        assert_eq!(title_bar(main), Some((PxRect::new(0, 0, 1000, 40), Some(PxRect::new(908, 0, 954, 40)))));
        assert_eq!(title_bar(pop_out), Some((small.bar, None)));
        // A pass without a bar (full screen): the window has none, and is not asked for again.
        assert!(!publish(main, &ctx, ViewportId::ROOT, 1.0, None));
        assert_eq!((hit(main, 500, 20), title_bar(main)), (CaptionHit::Outside, None));
        closed(pop_out);
        assert_eq!(hit(pop_out, 200, 20), CaptionHit::Outside, "a closed window is forgotten");
        closed(main);
    }

    #[test]
    fn a_window_is_the_systems_once_the_platform_says_so() {
        let ctx = Context::default();
        let (early, late) = (BASE + 10, BASE + 11);
        // The main window is hooked when it is created, before it has drawn anything.
        hooked(early);
        assert!(is_hooked(early));
        assert!(!publish(early, &ctx, ViewportId::ROOT, 1.0, Some(layout())), "hooked already: not asked for");
        // Another window is hooked when the platform answers the request its first bar made.
        assert!(!is_hooked(late));
        assert!(publish(late, &ctx, ViewportId::ROOT, 1.0, Some(layout())));
        hooked(late);
        assert!(is_hooked(late));
        closed(early);
        closed(late);
        assert!(!is_hooked(early) && !is_hooked(late));
    }

    #[test]
    fn what_the_platform_reports_reaches_the_ui_once() {
        let ctx = Context::default();
        let window = BASE + 20;
        // Before the window has drawn anything there is no UI to tell.
        hooked(window);
        pointer(window, Some((5, 5)));
        pressed(window, 5, 5);
        secondary_click(window, 5, 5);
        assert_eq!(take_reported(window), None);
        publish(window, &ctx, ViewportId::ROOT, 1.5, Some(layout()));
        assert_eq!(take_reported(window), None, "nothing new");
        pointer(window, Some((400, 20)));
        pressed(window, 400, 20);
        let reported = take_reported(window).expect("the pointer and a press");
        assert_eq!((reported.pointer, reported.presses, reported.pixels_per_point), (PointerChange::Moved((400, 20)), vec![(400, 20)], 1.5));
        // Told once. While the pointer stays on the caption the UI can still ask where it is.
        let still = |window| take_reported(window).map(|r| (r.pointer, r.pointer_at, r.presses.len()));
        assert_eq!(still(window), Some((PointerChange::Same, Some((400, 20)), 0)));
        pointer(window, Some((400, 20)));
        assert_eq!(still(window), Some((PointerChange::Same, Some((400, 20)), 0)), "the same place is no news");
        pointer(window, None);
        assert_eq!(still(window), Some((PointerChange::Left, None, 0)));
        assert_eq!(take_reported(window), None, "and nothing once it has left");
        // Right-clicks wait for the UI, a few at most.
        for x in 0..10 {
            secondary_click(window, x, 20);
        }
        let clicks = take_reported(window).map(|r| r.secondary_clicks).unwrap_or_default();
        assert_eq!(clicks, vec![(0, 20), (1, 20), (2, 20), (3, 20)]);
        // The maximize button held down, and what it was before.
        assert!(!maximize_pressed(window));
        assert!(!set_maximize_pressed(window, true));
        assert!(maximize_pressed(window));
        assert!(set_maximize_pressed(window, false));
        assert!(!maximize_pressed(window));
        closed(window);
    }
}
