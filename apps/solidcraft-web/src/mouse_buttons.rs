//! Mouse buttons as the browser holds them, not as eframe's per-button events report them.
//!
//! eframe 0.37's web runner turns each `pointerdown` and `pointerup` into one egui button event
//! from `event.button`. Pointer events don't work that way for a mouse: a second button pressed
//! while one is held (a chord) fires `pointermove`, not `pointerdown`, and so does releasing one
//! of two held buttons; only the last release fires `pointerup`. Press right, then left, let go
//! of right, then left: eframe reports "right down", then "left up", and egui keeps the right
//! button held for good. A release the page never hears of (outside the window, an alt-tab, a
//! cancelled pointer) leaves a button held the same way.
//!
//! So every mouse or pen pointer event's `buttons` bitmask (what the browser holds right now)
//! is noted here, and before each frame [`corrections`] adds the button events egui is missing
//! for its state to match it. The canvas also captures the pointer on `pointerdown`, so the
//! release still arrives when it happens outside the canvas or the window; `pointercancel`,
//! `lostpointercapture`, the window losing focus and the page being hidden release everything.
//! The focus moving inside the page releases nothing: eframe moves it between the canvas and its
//! hidden text input whenever a text field takes or gives up the keyboard, as a value box does
//! while its manipulator is dragged.
//! (A shim here, not a patch to eframe: its listeners stay as they are, this only adds what
//! they miss.)

use egui::{Event, Modifiers, NUM_POINTER_BUTTONS, PointerButton, Pos2};
use solidcraft_ui_egui::context_menu::BrowserMenu;

/// egui's buttons by their bit in the browser's `buttons` bitmask: bit 0 primary, 1 secondary,
/// 2 auxiliary (middle), 3 back, 4 forward. egui numbers its buttons in the same order. The
/// bitmask has no bit for a further button (6 and up), so eframe's own press and release events
/// are all egui hears of one.
const BUTTONS: [PointerButton; 5] =
    [PointerButton::Primary, PointerButton::Secondary, PointerButton::Middle, PointerButton::Extra1, PointerButton::Extra2];

/// The browser's `buttons` bitmask bit for the secondary (right) button.
const SECONDARY: u16 = 1 << (PointerButton::Secondary as usize);

/// The browser's `buttons` bitmask bit for the primary (left) button.
const PRIMARY: u16 = 1 << (PointerButton::Primary as usize);

/// What the browser last said about the mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    /// The `buttons` bitmask: the buttons held now.
    pub buttons: u16,
    /// The buttons went down on the canvas, so a press egui missed may be added. A drag that
    /// started elsewhere on the page and came over the canvas must not become a press in the app.
    pub on_canvas: bool,
    /// The pointer was cancelled or the window lost the focus since the last frame: every button
    /// egui holds is let go, the further ones (6 and up) too, which the bitmask has no bit for.
    pub lost: bool,
}

/// What happened to the pointer, as the page's listeners see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Happened {
    /// `pointerdown`, `pointermove` or `pointerup` from a mouse or pen, with its `buttons`;
    /// `down_on_canvas` when it is a `pointerdown` aimed at the canvas.
    Pointer { buttons: u16, down_on_canvas: bool },
    /// `pointercancel`, `lostpointercapture`, the window losing focus or the page being hidden:
    /// whatever was held won't be heard released.
    Lost,
    /// `blur` as the window's capture listener hears it: `of_window` when the window itself lost
    /// the focus (the same as [`Happened::Lost`]). Otherwise an element inside the page gave the
    /// focus to another, heard on its way down to that element, and the buttons are held as
    /// they were (#16209).
    Blur { of_window: bool },
}

/// What the browser did with the last right-click (#20011, #21229).
///
/// On macOS and Linux, `contextmenu` arrives during the press before `pointerup`. On Windows,
/// browsers emit `pointerdown` -> `pointerup` -> `mouseup` -> `contextmenu`, while eframe runs
/// `logic()` synchronously inside `pointerup`. Assuming `Taken` on `pointerdown` would therefore
/// block every Windows right-click (and every press-and-hold radial menu). Instead, a right
/// release without an earlier `contextmenu` enters [`RightMenu::AwaitingContextMenu`] and confirms
/// after the release turn whether the browser swallowed `contextmenu`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RightMenu {
    #[default]
    Idle,
    Pressed {
        id: u64,
    },
    Offered {
        id: u64,
    },
    AwaitingContextMenu {
        id: u64,
    },
    Taken,
}

/// The listeners' state: the latest observation, waiting for the next frame.
#[derive(Debug, Default)]
pub struct Tracker {
    /// The buttons held now went down on the canvas.
    on_canvas: bool,
    pending: Option<Observed>,
    /// The active pen tip began a primary press on the canvas.
    pen_contact: Option<i32>,
    /// Position of a completed pen press, for matching eframe's primary release.
    pen_release: Option<Pos2>,
    /// Position of a pen press awaiting the next frame.
    pen_press: Option<Pos2>,
    /// Where a finger last went down on the canvas, for the next frame. eframe runs a frame on
    /// `pointerdown` before the touch events arrive, so the app learns of a finger from here.
    finger_down: Option<Pos2>,
    /// Touches the browser names a stylus's (`touchType: "stylus"`, an Apple Pencil in Safari),
    /// noted from `touchstart` before eframe turns the touch into egui's, for the next frame.
    stylus: Vec<egui::TouchId>,
    /// Whether the secondary (right) mouse button is currently held according to pointer events.
    secondary_down: bool,
    /// Monotonic identifier for each secondary press, to ignore stale post-release timers.
    right_click_id: u64,
    /// State of the current or most recent right-click against `contextmenu` (#20011, #21229).
    right_menu: RightMenu,
    /// Pending confirmation timer id created when a secondary press ended without `contextmenu`.
    pending_menu_check: Option<u64>,
    /// Shift was held during the current or most recent right-click. Only then may a missing
    /// `contextmenu` mean the browser took the click (Firefox's Shift+right-click, #20011): a
    /// plain right-click whose `contextmenu` comes late is still the page's (#21302).
    right_shift: bool,
}

/// The most stylus touches waiting for a frame.
const MAX_STYLUS: usize = 32;

impl Tracker {
    /// Track the pen tip independently of mouse chords and other pointer contacts.
    fn note_pen(&mut self, id: i32, pressed: bool, on_canvas: bool, pos: Pos2) {
        if pressed && on_canvas {
            self.pen_contact = Some(id);
            self.pen_press = Some(pos);
        } else if !pressed && self.pen_contact == Some(id) {
            self.pen_contact = None;
            self.pen_release = Some(pos);
        }
    }

    /// A finger went down on the canvas.
    fn note_finger(&mut self, on_canvas: bool, pos: Pos2) {
        if on_canvas {
            self.finger_down = Some(pos);
        }
    }

    /// A touch the browser names a stylus's.
    fn note_stylus(&mut self, id: egui::TouchId) {
        if self.stylus.len() < MAX_STYLUS && !self.stylus.contains(&id) {
            self.stylus.push(id);
        }
    }

    /// Where a finger went down since the last frame, once.
    fn take_finger_down(&mut self) -> Option<Pos2> {
        self.finger_down.take()
    }

    /// A lost or cancelled contact must never be interpreted as a pen tap.
    fn cancel_pen(&mut self, id: Option<i32>) {
        if id.is_none() || self.pen_contact == id {
            self.pen_contact = None;
            self.pen_release = None;
            self.pen_press = None;
        }
    }

    /// Match the actual release before adding any button corrections or synthetic input: the
    /// frame's last primary release is the pen's when it is near the lift. Safari reports an Apple
    /// Pencil's lift twice (`pointerup` and `touchend`), a few points apart.
    fn take_pen_release(&mut self, events: &[Event]) -> Option<Pos2> {
        let at = self.pen_release.take()?;
        events
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. } => Some(*pos),
                _ => None,
            })
            .filter(|pos| pos.distance(at) <= solidcraft_ui_egui::pencil::PEN_RELEASE_SLOP)
    }

    /// Finish an active secondary press when the button is released or the pointer is lost.
    fn finish_secondary_press(&mut self) {
        match self.right_menu {
            RightMenu::Pressed { id } => {
                self.right_menu = RightMenu::AwaitingContextMenu { id };
                self.pending_menu_check = Some(id);
            }
            RightMenu::Offered { .. } => {
                self.right_menu = RightMenu::Idle;
            }
            RightMenu::Idle | RightMenu::AwaitingContextMenu { .. } | RightMenu::Taken => {}
        }
    }

    /// Note what happened. Returns true when the held buttons changed, so a frame should run.
    pub fn note(&mut self, happened: Happened) -> bool {
        let before = self.pending.map(|o| o.buttons);
        let buttons = match happened {
            Happened::Pointer { buttons, down_on_canvas } => {
                self.on_canvas |= down_on_canvas;
                let secondary = buttons & SECONDARY != 0;
                let primary = buttons & PRIMARY != 0;
                if down_on_canvas && primary && !secondary {
                    // A left press starts over: any earlier browser menu is gone by then, and
                    // an unconfirmed right-click check is no longer relevant (#20011, #21229).
                    self.right_menu = RightMenu::Idle;
                    self.pending_menu_check = None;
                }
                if secondary && !self.secondary_down && self.on_canvas {
                    // A right-click starts on the canvas: keep menus allowed until a post-release
                    // check confirms the browser swallowed `contextmenu` (#21229).
                    self.right_click_id = self.right_click_id.wrapping_add(1);
                    self.right_menu = RightMenu::Pressed { id: self.right_click_id };
                    self.pending_menu_check = None;
                    self.right_shift = false;
                } else if !secondary && self.secondary_down {
                    self.finish_secondary_press();
                }
                self.secondary_down = secondary;
                buttons
            }
            Happened::Lost | Happened::Blur { of_window: true } => {
                // `lostpointercapture` arrives right after `pointerup` while awaiting Windows's
                // `contextmenu`: only finish a press if the secondary button was still down.
                if self.secondary_down {
                    self.secondary_down = false;
                    self.finish_secondary_press();
                }
                0
            }
            Happened::Blur { of_window: false } => return false,
        };
        let lost = matches!(happened, Happened::Lost | Happened::Blur { of_window: true }) || self.pending.is_some_and(|o| o.lost);
        let observed = Observed { buttons, on_canvas: self.on_canvas, lost };
        if buttons == 0 {
            self.on_canvas = false;
        }
        self.pending = Some(observed);
        before != Some(buttons)
    }

    /// The observation for this frame, once.
    pub fn take(&mut self) -> Option<Observed> {
        self.pending.take()
    }

    /// What the browser did with the last right-click (#20011, #21229): `Taken` when confirmation
    /// elapsed without `contextmenu`.
    pub fn menu(&self) -> BrowserMenu {
        if self.right_menu == RightMenu::Taken { BrowserMenu::Taken } else { BrowserMenu::Offered }
    }

    /// Read and consume a confirmed `Taken` state so it is delivered to the UI once (#21229).
    pub fn take_menu(&mut self) -> BrowserMenu {
        let menu = self.menu();
        if self.right_menu == RightMenu::Taken {
            self.right_menu = RightMenu::Idle;
        }
        menu
    }

    /// The browser offered the page its context menu event: the click is the page's, and the app
    /// may keep or open its own menu (#20011, #21229).
    pub fn note_menu(&mut self) {
        self.pending_menu_check = None;
        self.right_menu = match self.right_menu {
            RightMenu::Pressed { id } | RightMenu::Offered { id } => RightMenu::Offered { id },
            RightMenu::Idle | RightMenu::AwaitingContextMenu { .. } | RightMenu::Taken => RightMenu::Idle,
        };
    }

    /// A right-click release that still needs post-release `contextmenu` confirmation, once.
    pub fn take_pending_menu_check(&mut self) -> Option<u64> {
        self.pending_menu_check.take()
    }

    /// Whether the right-click identified by `id` is still awaiting `contextmenu`.
    pub fn awaiting_menu(&self, id: u64) -> bool {
        self.right_menu == RightMenu::AwaitingContextMenu { id }
    }

    /// Note whether Shift is held, from a pointer event of the current right-click (#21302).
    pub fn note_shift(&mut self, shift: bool) {
        if matches!(self.right_menu, RightMenu::Pressed { .. } | RightMenu::AwaitingContextMenu { .. }) {
            self.right_shift |= shift;
        }
    }

    /// Mark right-click `id` as taken by the browser if it is still awaiting `contextmenu` and
    /// was made with Shift held, the click Firefox takes for itself (#20011). Without Shift the
    /// browser takes no click, so a `contextmenu` that is only late (a slow frame on a large
    /// design) leaves the click the page's and its menu open (#21302).
    pub fn confirm_menu_missing(&mut self, id: u64) -> bool {
        if !self.awaiting_menu(id) {
            return false;
        }
        if self.right_shift {
            self.right_menu = RightMenu::Taken;
            true
        } else {
            self.right_menu = RightMenu::Idle;
            false
        }
    }
}

/// The button events to add after `events` (this frame's input so far) so that egui, whose
/// buttons were `down` at the end of the last frame, holds what the browser holds. Releases are
/// always added; presses only for buttons that went down on the canvas.
pub fn corrections(down: [bool; NUM_POINTER_BUTTONS], events: &[Event], observed: Observed, pos: Pos2, modifiers: Modifiers) -> Vec<Event> {
    let mut down = down;
    let mut pos = pos;
    for event in events {
        match event {
            Event::PointerButton { button, pressed, pos: at, .. } => {
                down[*button as usize] = *pressed;
                pos = *at;
            }
            Event::PointerMoved(at) => pos = *at,
            _ => {}
        }
    }
    let mut out = Vec::new();
    if observed.lost {
        // A further button egui holds is let go too: no release of it will come.
        for button in PointerButton::ALL.into_iter().skip(BUTTONS.len()) {
            if down[button as usize] {
                out.push(Event::PointerButton { pos, button, pressed: false, modifiers });
            }
        }
    }
    for (bit, button) in BUTTONS.into_iter().enumerate() {
        let held = observed.buttons & (1 << bit) != 0;
        let pressed = match (down[bit], held) {
            (true, false) => false,
            (false, true) if observed.on_canvas => true,
            _ => continue,
        };
        out.push(Event::PointerButton { pos, button, pressed, modifiers });
    }
    out
}

#[cfg(target_arch = "wasm32")]
/// egui's buttons held at the end of the last frame.
pub fn egui_down(ctx: &egui::Context) -> [bool; NUM_POINTER_BUTTONS] {
    ctx.input(|i| PointerButton::ALL.map(|b| i.pointer.button_down(b)))
}

#[cfg(target_arch = "wasm32")]
/// Add missing button events and return what the platform says about this frame's taps: where a
/// pen tip was lifted and where a finger went down.
pub fn reconcile(tracker: &std::cell::RefCell<Tracker>, ctx: &egui::Context, raw: &mut egui::RawInput) -> solidcraft_ui_egui::pencil::PlatformTaps {
    let mut tracker = tracker.borrow_mut();
    let taps = solidcraft_ui_egui::pencil::PlatformTaps {
        pen_press: tracker.pen_press.take(),
        pen_release: tracker.take_pen_release(&raw.events),
        finger_down: tracker.take_finger_down(),
        pen_down: tracker.pen_contact.is_some(),
        stylus: std::mem::take(&mut tracker.stylus),
    };
    let Some(observed) = tracker.take() else { return taps };
    let pos = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos())).unwrap_or_default();
    let modifiers = ctx.input(|i| i.modifiers);
    let fixes = corrections(egui_down(ctx), &raw.events, observed, pos, modifiers);
    if !fixes.is_empty() {
        log::debug!("mouse buttons: {fixes:?} to match the browser's {:#07b}", observed.buttons);
    }
    raw.events.extend(fixes);
    taps
}

#[cfg(target_arch = "wasm32")]
pub use web::install;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use super::{Happened, Tracker};

    /// Wait past `pointerup`'s synchronous frame and the immediate `contextmenu` turn, then wait
    /// `CONTEXTMENU_CONFIRM_MS` before concluding the browser swallowed `contextmenu` (#21229).
    const CONTEXTMENU_CONFIRM_MS: i32 = 50;

    fn schedule_menu_confirm(window: &web_sys::Window, tracker: Rc<RefCell<Tracker>>, ctx: egui::Context, id: u64) {
        let confirm_window = window.clone();
        let on_tick = Closure::once_into_js(move || {
            let awaiting = tracker.borrow().awaiting_menu(id);
            if !awaiting {
                return;
            }
            let check = Closure::once_into_js(move || {
                let missing = tracker.borrow_mut().confirm_menu_missing(id);
                if missing {
                    ctx.request_repaint();
                }
            });
            let _ = confirm_window.set_timeout_with_callback_and_timeout_and_arguments_0(check.unchecked_ref(), CONTEXTMENU_CONFIRM_MS);
        });
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(on_tick.unchecked_ref(), 0);
    }

    /// Listen on the window in the capture phase, so each observation is noted before eframe's
    /// own listeners (on the canvas and the document) run a frame from the same event.
    pub fn install(window: &web_sys::Window, canvas: &web_sys::HtmlCanvasElement, ctx: &egui::Context) -> Rc<RefCell<Tracker>> {
        let tracker = Rc::new(RefCell::new(Tracker::default()));
        let listen = |name: &str, on: Box<dyn FnMut(web_sys::Event)>| {
            let on = Closure::<dyn FnMut(web_sys::Event)>::new(on);
            let _ = window.add_event_listener_with_callback_and_bool(name, on.as_ref().unchecked_ref(), true);
            // The listeners live as long as the page.
            on.forget();
        };
        for name in ["pointerdown", "pointermove", "pointerup"] {
            let (tracker, canvas, ctx, win) = (tracker.clone(), canvas.clone(), ctx.clone(), window.clone());
            listen(
                name,
                Box::new(move |e: web_sys::Event| {
                    let Some(e) = e.dyn_ref::<web_sys::PointerEvent>() else { return };
                    // The canvas fills the page without padding. Match eframe's points at the
                    // current UI zoom, with room for the browser's coordinate rounding.
                    let at = || {
                        let rect = canvas.get_bounding_client_rect();
                        let zoom = ctx.zoom_factor();
                        egui::pos2((e.client_x() as f32 - rect.left() as f32) / zoom, (e.client_y() as f32 - rect.top() as f32) / zoom)
                    };
                    // Touches reach egui through touch events; their buttons aren't tracked here.
                    // A finger going down on the canvas is noted for Pencil mode.
                    let source = e.pointer_type();
                    if source == "touch" {
                        if name == "pointerdown" {
                            let on_canvas = e.target().is_some_and(|t| canvas.is_same_node(t.dyn_ref::<web_sys::Node>()));
                            tracker.borrow_mut().note_finger(on_canvas, at());
                        }
                        return;
                    }
                    let mut down_on_canvas = false;
                    if name == "pointerdown" && e.target().is_some_and(|t| canvas.is_same_node(t.dyn_ref::<web_sys::Node>())) {
                        down_on_canvas = true;
                        // The release then comes here wherever it happens, even outside the window.
                        let _ = canvas.set_pointer_capture(e.pointer_id());
                    }
                    let (changed, confirm) = {
                        let mut tracker = tracker.borrow_mut();
                        if source == "pen" && web_sys::MouseEvent::button(e) == 0 && name != "pointermove" {
                            tracker.note_pen(e.pointer_id(), name == "pointerdown", down_on_canvas, at());
                        }
                        let changed = tracker.note(Happened::Pointer { buttons: e.buttons(), down_on_canvas });
                        tracker.note_shift(e.shift_key());
                        (changed, tracker.take_pending_menu_check())
                    };
                    if changed {
                        ctx.request_repaint();
                    }
                    if let Some(id) = confirm {
                        schedule_menu_confirm(&win, tracker.clone(), ctx.clone(), id);
                    }
                }),
            );
        }
        // The browser's context menu event: the browser offers the page its menu, and the page may
        // stop it. Firefox takes a Shift+right-click for itself and never offers it (#20011), so
        // the app knows the browser's menu is the one that opened.
        {
            let tracker = tracker.clone();
            listen(
                "contextmenu",
                Box::new(move |_e: web_sys::Event| {
                    tracker.borrow_mut().note_menu();
                }),
            );
        }
        // Safari also sends touch events for an Apple Pencil, and eframe hands them on as egui
        // touches: name the ones from a stylus, so Pencil mode does not take them for fingers.
        {
            let tracker = tracker.clone();
            listen(
                "touchstart",
                Box::new(move |e: web_sys::Event| {
                    let Some(list) = e.dyn_ref::<web_sys::TouchEvent>().map(web_sys::TouchEvent::changed_touches) else { return };
                    for i in 0..list.length() {
                        let Some(touch) = list.get(i) else { continue };
                        let kind = js_sys::Reflect::get(&touch, &"touchType".into()).ok().and_then(|v| v.as_string());
                        if kind.as_deref() == Some("stylus") {
                            tracker.borrow_mut().note_stylus(egui::TouchId::from(touch.identifier()));
                        }
                    }
                }),
            );
        }
        let lost = {
            let (tracker, ctx, win) = (tracker.clone(), ctx.clone(), window.clone());
            move |id| {
                let (changed, confirm) = {
                    let mut tracker = tracker.borrow_mut();
                    tracker.cancel_pen(id);
                    (tracker.note(Happened::Lost), tracker.take_pending_menu_check())
                };
                if changed {
                    ctx.request_repaint();
                }
                if let Some(id) = confirm {
                    schedule_menu_confirm(&win, tracker.clone(), ctx.clone(), id);
                }
            }
        };
        for name in ["pointercancel", "lostpointercapture"] {
            let lost = lost.clone();
            listen(name, Box::new(move |e| lost(e.dyn_ref::<web_sys::PointerEvent>().map(web_sys::PointerEvent::pointer_id))));
        }
        // `blur` doesn't bubble, but a capture listener on the window hears every element's on
        // its way down, not only the window's own.
        {
            let (tracker, ctx, win) = (tracker.clone(), ctx.clone(), window.clone());
            listen(
                "blur",
                Box::new(move |e: web_sys::Event| {
                    let of_window = e.target().is_some_and(|t| t.dyn_ref::<web_sys::Window>().is_some());
                    let (changed, confirm) = {
                        let mut tracker = tracker.borrow_mut();
                        // A pen tip held while the window loses the focus is never heard lifted.
                        if of_window {
                            tracker.cancel_pen(None);
                        }
                        (tracker.note(Happened::Blur { of_window }), tracker.take_pending_menu_check())
                    };
                    if changed {
                        ctx.request_repaint();
                    }
                    if let Some(id) = confirm {
                        schedule_menu_confirm(&win, tracker.clone(), ctx.clone(), id);
                    }
                }),
            );
        }
        let document = window.document();
        listen(
            "visibilitychange",
            Box::new(move |_| {
                if document.as_ref().is_some_and(|d| d.visibility_state() == web_sys::VisibilityState::Hidden) {
                    lost(None);
                }
            }),
        );
        tracker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [bool; NUM_POINTER_BUTTONS] = [false; NUM_POINTER_BUTTONS];
    const P: Pos2 = Pos2::new(10.0, 20.0);

    fn press(button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos: P, button, pressed, modifiers: Modifiers::NONE }
    }

    fn held(buttons: &[PointerButton]) -> [bool; NUM_POINTER_BUTTONS] {
        let mut down = NONE;
        for b in buttons {
            down[*b as usize] = true;
        }
        down
    }

    /// Run one browser event through the tracker, then a frame through `corrections`, the way
    /// the page does; returns egui's buttons after the frame and the events added.
    fn frame(
        tracker: &mut Tracker,
        down: [bool; NUM_POINTER_BUTTONS],
        eframe: &[Event],
        happened: Happened,
    ) -> ([bool; NUM_POINTER_BUTTONS], Vec<Event>) {
        tracker.note(happened);
        let mut events = eframe.to_vec();
        if let Some(observed) = tracker.take() {
            let fixes = corrections(down, &events, observed, P, Modifiers::NONE);
            events.extend(fixes.clone());
            let mut after = down;
            for e in &events {
                if let Event::PointerButton { button, pressed, .. } = e {
                    after[*button as usize] = *pressed;
                }
            }
            return (after, fixes);
        }
        (down, Vec::new())
    }

    fn pointer(buttons: u16, down_on_canvas: bool) -> Happened {
        Happened::Pointer { buttons, down_on_canvas }
    }

    /// A finger down on the canvas is reported to the next frame once; one down elsewhere on the
    /// page is not reported at all.
    #[test]
    fn finger_down_is_reported_once_and_only_on_the_canvas() {
        let mut tracker = Tracker::default();
        tracker.note_finger(false, P);
        assert_eq!(tracker.take_finger_down(), None);
        tracker.note_finger(true, P);
        assert_eq!(tracker.take_finger_down(), Some(P));
        assert_eq!(tracker.take_finger_down(), None);
    }

    /// Stylus touches are reported to the next frame once, each once, and at most `MAX_STYLUS`.
    #[test]
    fn stylus_touches_are_reported_once() {
        let mut tracker = Tracker::default();
        tracker.note_stylus(egui::TouchId(3));
        tracker.note_stylus(egui::TouchId(3));
        assert_eq!(std::mem::take(&mut tracker.stylus), vec![egui::TouchId(3)]);
        for i in 0..100 {
            tracker.note_stylus(egui::TouchId(i));
        }
        assert_eq!(tracker.stylus.len(), MAX_STYLUS);
    }

    /// An Apple Pencil tap on an iPad, as Safari reported it: `pointerup` at (197, 272) and
    /// `touchend` at (199.5, 274), and eframe's two releases in one frame. It is a pen tap.
    #[test]
    fn safari_reports_a_pencil_lift_twice_a_few_points_apart() {
        let mut tracker = Tracker::default();
        let (up, end) = (egui::pos2(197.0, 272.0), egui::pos2(199.5, 274.0));
        let release = |pos| Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE };
        tracker.note_pen(-652_795_160, true, true, egui::pos2(198.5, 277.0));
        tracker.note_pen(-652_795_160, false, false, up);
        assert_eq!(tracker.take_pen_release(&[release(up), Event::PointerGone, release(end)]), Some(end));
    }

    /// A pen tap is matched once, and only after the same canvas contact releases.
    #[test]
    fn pen_release_matches_only_the_canvas_contact() {
        let mut tracker = Tracker::default();
        let release = [press(PointerButton::Primary, false)];
        tracker.note_pen(7, true, false, P);
        tracker.note_pen(7, false, false, P);
        assert_eq!(tracker.take_pen_release(&release), None);

        tracker.note_pen(7, true, true, P);
        tracker.note_pen(8, false, true, P);
        assert_eq!(tracker.take_pen_release(&release), None);
        tracker.note_pen(7, false, true, P);
        assert_eq!(tracker.take_pen_release(&release), Some(P));
        assert_eq!(tracker.take_pen_release(&release), None);
    }

    /// Lost, unmatched and cancelled contacts cannot mark a later mouse click as a pen tap.
    #[test]
    fn cancelled_or_unmatched_pen_release_is_discarded() {
        let mut tracker = Tracker::default();
        let release = [press(PointerButton::Primary, false)];
        tracker.note_pen(7, true, true, P);
        tracker.cancel_pen(Some(8));
        tracker.note_pen(7, false, false, P);
        assert_eq!(tracker.take_pen_release(&release), Some(P));

        tracker.note_pen(7, true, true, P);
        tracker.cancel_pen(Some(7));
        tracker.note_pen(7, false, false, P);
        assert_eq!(tracker.take_pen_release(&release), None);

        tracker.note_pen(7, true, true, P);
        tracker.note_pen(7, false, false, P + egui::vec2(40.0, 0.0));
        assert_eq!(tracker.take_pen_release(&release), None);
        assert_eq!(tracker.take_pen_release(&release), None);

        tracker.note_pen(7, true, true, P);
        tracker.note_pen(7, false, false, P);
        let mut later = press(PointerButton::Primary, false);
        if let Event::PointerButton { pos, .. } = &mut later {
            *pos += egui::vec2(40.0, 0.0);
        }
        assert_eq!(tracker.take_pen_release(&[release[0].clone(), later]), None);
    }

    /// Right, then left, held together; right let go, then left. The browser fires pointerdown
    /// (right), pointermove (buttons 3), pointermove (buttons 1), pointerup (left), so eframe
    /// says only "right down" and "left up": egui kept right held for good.
    #[test]
    fn a_chord_ends_with_no_button_held() {
        let mut t = Tracker::default();
        let (down, fixes) = frame(&mut t, NONE, &[press(PointerButton::Secondary, true)], pointer(0b10, true));
        assert!(fixes.is_empty(), "eframe's own press matches the browser: {fixes:?}");
        let (down, fixes) = frame(&mut t, down, &[], pointer(0b11, false));
        assert_eq!(fixes, vec![press(PointerButton::Primary, true)], "the chord's second button is pressed");
        assert_eq!(down, held(&[PointerButton::Primary, PointerButton::Secondary]));
        let (down, fixes) = frame(&mut t, down, &[], pointer(0b01, false));
        assert_eq!(fixes, vec![press(PointerButton::Secondary, false)], "right is let go while left stays");
        let (down, fixes) = frame(&mut t, down, &[press(PointerButton::Primary, false)], pointer(0, false));
        assert!(fixes.is_empty(), "eframe's own release matches: {fixes:?}");
        assert_eq!(down, NONE, "nothing stays held");
    }

    /// Both pressed at once and both let go at once: one pointerup with buttons 0 that names
    /// only one button. The other is released too.
    #[test]
    fn one_pointerup_for_two_buttons_releases_both() {
        let mut t = Tracker::default();
        let (down, _) = frame(&mut t, NONE, &[press(PointerButton::Primary, true)], pointer(0b01, true));
        let (down, _) = frame(&mut t, down, &[], pointer(0b11, false));
        assert_eq!(down, held(&[PointerButton::Primary, PointerButton::Secondary]));
        let (down, fixes) = frame(&mut t, down, &[press(PointerButton::Primary, false)], pointer(0, false));
        assert_eq!(fixes, vec![press(PointerButton::Secondary, false)]);
        assert_eq!(down, NONE);
    }

    /// A drag let go outside the window: no pointerup was heard, and the next pointer event
    /// (back over the page) has no buttons held.
    #[test]
    fn a_release_outside_is_caught_by_the_next_event() {
        let mut t = Tracker::default();
        let (down, _) = frame(&mut t, NONE, &[press(PointerButton::Secondary, true)], pointer(0b10, true));
        let (down, fixes) = frame(&mut t, down, &[Event::PointerMoved(Pos2::new(5.0, 6.0))], pointer(0, false));
        assert_eq!(
            fixes,
            vec![Event::PointerButton { pos: Pos2::new(5.0, 6.0), button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]
        );
        assert_eq!(down, NONE);
    }

    /// The window loses focus (alt-tab) or the pointer is cancelled with buttons held.
    #[test]
    fn blur_and_cancel_release_every_button() {
        let mut t = Tracker::default();
        let (down, _) = frame(&mut t, NONE, &[press(PointerButton::Middle, true)], pointer(0b100, true));
        let (down, _) = frame(&mut t, down, &[], pointer(0b110, false));
        let (down, fixes) = frame(&mut t, down, &[], Happened::Lost);
        assert_eq!(fixes, vec![press(PointerButton::Secondary, false), press(PointerButton::Middle, false)]);
        assert_eq!(down, NONE);
        // Held from before this tracker heard anything (a stale state) is released too.
        let (down, _) = frame(&mut t, held(&[PointerButton::Extra1]), &[], Happened::Lost);
        assert_eq!(down, NONE);
    }

    /// #16209: a manipulator's value box takes the keyboard while the manipulator is dragged, so
    /// eframe moves the focus from the canvas to its hidden text input, and a press on the canvas
    /// takes it back. Either way an element inside the page blurs and the window hears it. The
    /// button is still held: letting it go there ended every such drag as it began.
    #[test]
    fn focus_moving_inside_the_page_keeps_the_buttons_held() {
        let mut t = Tracker::default();
        let (down, _) = frame(&mut t, NONE, &[press(PointerButton::Primary, true)], pointer(0b01, true));
        let (down, fixes) = frame(&mut t, down, &[], Happened::Blur { of_window: false });
        assert!(fixes.is_empty(), "the press is not let go: {fixes:?}");
        assert_eq!(down, held(&[PointerButton::Primary]));
        assert!(!t.note(Happened::Blur { of_window: false }), "and no frame is asked for");
        assert_eq!(t.take(), None);
        // The drag goes on, and a second button still joins it: the press was on the canvas.
        let (down, fixes) = frame(&mut t, down, &[Event::PointerMoved(Pos2::new(30.0, 40.0))], pointer(0b01, false));
        assert!(fixes.is_empty() && down == held(&[PointerButton::Primary]), "{fixes:?}");
        let (down, _) = frame(&mut t, down, &[], Happened::Blur { of_window: false });
        let (down, fixes) = frame(&mut t, down, &[], pointer(0b11, false));
        assert_eq!(fixes.len(), 1, "{fixes:?}");
        assert_eq!(down, held(&[PointerButton::Primary, PointerButton::Secondary]));
        // The window itself losing the focus still lets go of everything.
        let (down, fixes) = frame(&mut t, down, &[], Happened::Blur { of_window: true });
        assert_eq!(fixes, vec![press(PointerButton::Primary, false), press(PointerButton::Secondary, false)]);
        assert_eq!(down, NONE);
    }

    /// A further button (6 and up) has no bit in the browser's bitmask: moves that report none
    /// held leave it held, and a cancelled pointer lets go of it with the rest.
    #[test]
    fn a_further_button_is_held_until_released_or_lost() {
        let mut t = Tracker::default();
        let (down, fixes) = frame(&mut t, NONE, &[press(PointerButton::Extra3, true)], pointer(0, true));
        assert!(fixes.is_empty(), "{fixes:?}");
        assert_eq!(down, held(&[PointerButton::Extra3]));
        let (down, fixes) = frame(&mut t, down, &[Event::PointerMoved(Pos2::new(30.0, 40.0))], pointer(0, false));
        assert!(fixes.is_empty(), "a move with no bit for it keeps it held: {fixes:?}");
        let (down, _) = frame(&mut t, down, &[press(PointerButton::Primary, true)], pointer(0b01, true));
        let (down, fixes) = frame(&mut t, down, &[], Happened::Lost);
        assert_eq!(fixes, vec![press(PointerButton::Extra3, false), press(PointerButton::Primary, false)]);
        assert_eq!(down, NONE);
    }

    /// A drag that started elsewhere on the page and came over the canvas isn't a press in the app.
    #[test]
    fn a_drag_from_outside_the_canvas_presses_nothing() {
        let mut t = Tracker::default();
        let (down, fixes) = frame(&mut t, NONE, &[], pointer(0b01, false));
        assert!(fixes.is_empty() && down == NONE, "{fixes:?}");
        let (down, fixes) = frame(&mut t, down, &[], pointer(0, false));
        assert!(fixes.is_empty() && down == NONE);
        // The next press on the canvas counts again.
        let (down, _) = frame(&mut t, down, &[press(PointerButton::Primary, true)], pointer(0b01, true));
        let (down, fixes) = frame(&mut t, down, &[], pointer(0b11, false));
        assert_eq!(fixes, vec![press(PointerButton::Secondary, true)]);
        assert_eq!(down, held(&[PointerButton::Primary, PointerButton::Secondary]));
    }

    /// A quick click inside one frame (down and up both from eframe) needs nothing, and no frame
    /// runs corrections without a new observation (touch input is left alone).
    #[test]
    fn matching_input_is_left_alone() {
        let mut t = Tracker::default();
        let (down, fixes) = frame(&mut t, NONE, &[press(PointerButton::Primary, true), press(PointerButton::Primary, false)], pointer(0, true));
        assert!(fixes.is_empty() && down == NONE, "{fixes:?}");
        assert_eq!(t.take(), None, "an observation is used once");
        let touch_held = held(&[PointerButton::Primary]);
        assert!(corrections(touch_held, &[], Observed { buttons: 1, on_canvas: false, lost: false }, P, Modifiers::NONE).is_empty());
    }

    /// The tracker asks for a frame only when the held buttons change.
    #[test]
    fn repaint_only_on_change() {
        let mut t = Tracker::default();
        assert!(t.note(pointer(0, false)), "the first observation");
        assert!(!t.note(pointer(0, false)), "hovering changes nothing");
        assert!(t.note(pointer(0b10, true)));
        assert!(!t.note(pointer(0b10, false)));
        assert!(t.note(Happened::Lost));
    }

    /// #20011, #21229: On Windows, `contextmenu` arrives after `pointerup`, so a right press
    /// keeps menus allowed until a post-release check confirms the browser swallowed `contextmenu`.
    #[test]
    fn a_right_click_the_browser_took_is_reported_as_taken() {
        let mut t = Tracker::default();
        assert_eq!(t.menu(), BrowserMenu::Offered, "no click yet: the page may open its menu");
        t.note(pointer(0b10, true));
        t.note_shift(true);
        assert_eq!(t.menu(), BrowserMenu::Offered, "menus stay allowed during press for radial holds and Windows release");
        t.note(pointer(0, false));
        let id = t.take_pending_menu_check().expect("release without contextmenu schedules a post-release check");
        assert_eq!(t.menu(), BrowserMenu::Offered, "still offered at pointerup frame");
        assert!(t.awaiting_menu(id));
        assert!(t.confirm_menu_missing(id));
        assert_eq!(t.take_menu(), BrowserMenu::Taken);
        assert_eq!(t.take_menu(), BrowserMenu::Offered, "Taken is consumed once");
    }

    /// A held right-drag stays Offered while moving, and a subsequent right-click starts over.
    #[test]
    fn a_move_keeps_the_browser_having_taken_the_click() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        t.note_shift(true);
        assert_eq!(t.menu(), BrowserMenu::Offered);
        t.note(pointer(0b10, false));
        assert_eq!(t.menu(), BrowserMenu::Offered, "moving while held keeps menus allowed");
        assert_eq!(t.take_pending_menu_check(), None);
        t.note(pointer(0, false));
        let id = t.take_pending_menu_check().expect("release without contextmenu schedules confirmation");
        assert!(t.confirm_menu_missing(id));
        assert_eq!(t.take_menu(), BrowserMenu::Taken);
        // The next right-click starts fresh.
        t.note(pointer(0b10, true));
        assert_eq!(t.menu(), BrowserMenu::Offered);
    }

    /// #20011, #21229: a left press on the canvas cancels any pending check and clears `Taken`.
    #[test]
    fn a_left_click_starts_over() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        t.note(pointer(0, false));
        let id = t.take_pending_menu_check().unwrap();
        assert!(t.awaiting_menu(id));
        t.note(pointer(0b01, true));
        assert!(!t.awaiting_menu(id), "a left press cancels pending right-click confirmation");
        assert!(!t.confirm_menu_missing(id));
        assert_eq!(t.menu(), BrowserMenu::Offered);

        // Even once `Taken` is latched, a left press clears it.
        t.note(pointer(0, false));
        t.note(pointer(0b10, true));
        t.note_shift(true);
        t.note(pointer(0, false));
        let id2 = t.take_pending_menu_check().unwrap();
        assert!(t.confirm_menu_missing(id2));
        assert_eq!(t.menu(), BrowserMenu::Taken);
        t.note(pointer(0b01, true));
        assert_eq!(t.menu(), BrowserMenu::Offered, "the browser's menu is gone by then");
    }

    /// #21229: Windows fires `pointerdown` -> `pointerup` -> `lostpointercapture` -> `contextmenu`.
    /// SolidCraft must keep menus allowed at `pointerup`, survive `lostpointercapture`, and cancel
    /// the pending check once `contextmenu` arrives.
    #[test]
    fn windows_right_click_order_keeps_solidcraft_menu_open() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        assert_eq!(t.menu(), BrowserMenu::Offered);
        t.note(pointer(0, false));
        assert_eq!(t.menu(), BrowserMenu::Offered);
        let id = t.take_pending_menu_check().expect("release schedules confirmation");
        t.note(Happened::Lost);
        assert!(t.awaiting_menu(id), "lostpointercapture after release must not drop pending check");
        t.note_menu();
        assert!(!t.awaiting_menu(id));
        assert!(!t.confirm_menu_missing(id));
        assert_eq!(t.menu(), BrowserMenu::Offered);
    }

    /// macOS/Linux fire `contextmenu` during the press before `pointerup`: no timer is scheduled.
    #[test]
    fn macos_linux_right_click_order_skips_timer_on_release() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        t.note_menu();
        assert_eq!(t.menu(), BrowserMenu::Offered);
        t.note(pointer(0, false));
        assert_eq!(t.take_pending_menu_check(), None);
        assert_eq!(t.menu(), BrowserMenu::Offered);
    }

    /// When Windows Firefox suppresses `contextmenu` on Shift+right-click, `lostpointercapture`
    /// still leaves the pending check active until confirmation marks `Taken` once.
    #[test]
    fn missing_contextmenu_after_release_marks_taken_once() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        t.note(pointer(0, false));
        t.note_shift(true);
        let id = t.take_pending_menu_check().unwrap();
        t.note(Happened::Lost);
        assert!(t.confirm_menu_missing(id));
        assert_eq!(t.take_menu(), BrowserMenu::Taken);
        assert_eq!(t.take_menu(), BrowserMenu::Offered);
    }

    /// #21302: a right-click without Shift is never one the browser took. On a large design a
    /// slow frame can hold `contextmenu` past the confirmation, and the Browser panel's menu
    /// then said the browser took the click; the click stays the page's, its menu open.
    #[test]
    fn a_late_contextmenu_without_shift_leaves_the_click_the_pages() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        t.note_shift(false);
        t.note(pointer(0, false));
        t.note_shift(false);
        let id = t.take_pending_menu_check().expect("release schedules confirmation");
        assert!(!t.confirm_menu_missing(id), "no Shift: the browser took nothing");
        assert_eq!(t.take_menu(), BrowserMenu::Offered);
        // Shift from an earlier right-click does not carry over to the next one.
        t.note(pointer(0b10, true));
        t.note_shift(true);
        t.note(pointer(0, false));
        let id = t.take_pending_menu_check().unwrap();
        assert!(t.confirm_menu_missing(id));
        assert_eq!(t.take_menu(), BrowserMenu::Taken);
        t.note(pointer(0b10, true));
        t.note(pointer(0, false));
        let id = t.take_pending_menu_check().unwrap();
        assert!(!t.confirm_menu_missing(id), "this click had no Shift");
        assert_eq!(t.menu(), BrowserMenu::Offered);
    }

    /// A stale confirmation id from an earlier right-click cannot mark a newer click as taken.
    #[test]
    fn repeated_right_clicks_and_stale_confirmation_id() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        t.note(pointer(0, false));
        let id1 = t.take_pending_menu_check().unwrap();

        t.note(pointer(0b10, true));
        assert!(!t.confirm_menu_missing(id1), "stale timer id must not affect click 2");
        assert_eq!(t.menu(), BrowserMenu::Offered);

        t.note(pointer(0, false));
        let id2 = t.take_pending_menu_check().unwrap();
        assert_ne!(id1, id2);
        t.note_menu();
        assert!(!t.confirm_menu_missing(id2));
        assert_eq!(t.menu(), BrowserMenu::Offered);
    }

    /// Holding the right button for radial press-and-hold stays Offered while moving.
    #[test]
    fn hold_without_release_keeps_menu_offered_while_moving() {
        let mut t = Tracker::default();
        t.note(pointer(0b10, true));
        assert_eq!(t.menu(), BrowserMenu::Offered);
        assert_eq!(t.take_pending_menu_check(), None);
        t.note(pointer(0b10, false));
        assert_eq!(t.menu(), BrowserMenu::Offered);
        assert_eq!(t.take_pending_menu_check(), None);
    }
}
