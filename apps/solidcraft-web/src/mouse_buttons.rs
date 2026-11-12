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
//! (A shim here, not a patch to eframe: its listeners stay as they are, this only adds what
//! they miss.)

use egui::{Event, Modifiers, NUM_POINTER_BUTTONS, PointerButton, Pos2};

/// egui's buttons by their bit in the browser's `buttons` bitmask: bit 0 primary, 1 secondary,
/// 2 auxiliary (middle), 3 back, 4 forward. egui numbers its buttons in the same order.
const BUTTONS: [PointerButton; NUM_POINTER_BUTTONS] =
    [PointerButton::Primary, PointerButton::Secondary, PointerButton::Middle, PointerButton::Extra1, PointerButton::Extra2];

/// What the browser last said about the mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    /// The `buttons` bitmask: the buttons held now.
    pub buttons: u16,
    /// The buttons went down on the canvas, so a press egui missed may be added. A drag that
    /// started elsewhere on the page and came over the canvas must not become a press in the app.
    pub on_canvas: bool,
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
}

/// The listeners' state: the latest observation, waiting for the next frame.
#[derive(Debug, Default)]
pub struct Tracker {
    /// The buttons held now went down on the canvas.
    on_canvas: bool,
    pending: Option<Observed>,
}

impl Tracker {
    /// Note what happened. Returns true when the held buttons changed, so a frame should run.
    pub fn note(&mut self, happened: Happened) -> bool {
        let before = self.pending.map(|o| o.buttons);
        let buttons = match happened {
            Happened::Pointer { buttons, down_on_canvas } => {
                self.on_canvas |= down_on_canvas;
                buttons
            }
            Happened::Lost => 0,
        };
        let observed = Observed { buttons, on_canvas: self.on_canvas };
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
    ctx.input(|i| BUTTONS.map(|b| i.pointer.button_down(b)))
}

#[cfg(target_arch = "wasm32")]
/// Add the button events this frame needs (see [`corrections`]).
pub fn reconcile(tracker: &std::cell::RefCell<Tracker>, ctx: &egui::Context, raw: &mut egui::RawInput) {
    let Some(observed) = tracker.borrow_mut().take() else { return };
    let pos = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos())).unwrap_or_default();
    let modifiers = ctx.input(|i| i.modifiers);
    let fixes = corrections(egui_down(ctx), &raw.events, observed, pos, modifiers);
    if !fixes.is_empty() {
        log::debug!("mouse buttons: {fixes:?} to match the browser's {:#07b}", observed.buttons);
    }
    raw.events.extend(fixes);
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
            let (tracker, canvas, ctx) = (tracker.clone(), canvas.clone(), ctx.clone());
            listen(
                name,
                Box::new(move |e: web_sys::Event| {
                    let Some(e) = e.dyn_ref::<web_sys::PointerEvent>() else { return };
                    // Touches reach egui through touch events; their buttons aren't tracked here.
                    if e.pointer_type() == "touch" {
                        return;
                    }
                    let mut down_on_canvas = false;
                    if name == "pointerdown" && e.target().is_some_and(|t| canvas.is_same_node(t.dyn_ref::<web_sys::Node>())) {
                        down_on_canvas = true;
                        // The release then comes here wherever it happens, even outside the window.
                        let _ = canvas.set_pointer_capture(e.pointer_id());
                    }
                    if tracker.borrow_mut().note(Happened::Pointer { buttons: e.buttons(), down_on_canvas }) {
                        ctx.request_repaint();
                    }
                }),
            );
        }
        let lost = {
            let (tracker, ctx) = (tracker.clone(), ctx.clone());
            move || {
                if tracker.borrow_mut().note(Happened::Lost) {
                    ctx.request_repaint();
                }
            }
        };
        for name in ["pointercancel", "lostpointercapture", "blur"] {
            let lost = lost.clone();
            listen(name, Box::new(move |_| lost()));
        }
        let document = window.document();
        listen(
            "visibilitychange",
            Box::new(move |_| {
                if document.as_ref().is_some_and(|d| d.visibility_state() == web_sys::VisibilityState::Hidden) {
                    lost();
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
        assert!(corrections(touch_held, &[], Observed { buttons: 1, on_canvas: false }, P, Modifiers::NONE).is_empty());
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
}
