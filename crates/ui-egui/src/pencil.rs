//! Pencil mode (Preferences › Sketch, web app): a pen selects and fingers move the view.
//!
//! With the preference on:
//! - a pen tap adds an item to the selection or takes it out, and a pen tap on empty space
//!   clears the selection (`viewport::show`, `pen_select`);
//! - a pen double-tap selects the whole body, or the whole sketch, of the item under it
//!   ([`whole`]);
//! - a finger that goes down in the open view never clicks there: the press the browser makes
//!   for it is taken out of the frame ([`filter`]), and the touch itself is left for navigation
//!   (touchscreen orbit, pan and pinch read the touch events, not the press). A finger on a
//!   panel, a window, a button over the view or the view's axes still clicks as usual;
//! - a finger double-tap on a planar face selects the face and starts a sketch on it
//!   ([`finger_double_tap`]).
//!
//! The platform says what a contact is: the web app's pointer listeners see `pointerType`
//! before eframe does ([`PlatformTaps`]). eframe runs a frame on `pointerdown`, before the
//! browser's touch events arrive, so a finger is known from `finger_down` in that frame and from
//! its touch events after it.

use egui::{Event, PointerButton, Pos2, RawInput, TouchPhase};
use serde_json::json;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};
use solidcraft_engine::Sel;

/// The longest touch that still counts as a tap, in seconds.
const TAP_TIME: f64 = 0.4;
/// The farthest a finger may move and still tap, in points.
const TAP_SLOP: f32 = 12.0;
/// The longest gap between the two taps of a double-tap, in seconds.
const DOUBLE_TAP_TIME: f64 = 0.4;
/// The farthest apart the two taps of a double-tap may be, in points.
const DOUBLE_TAP_SLOP: f32 = 24.0;
/// A finger reported down whose touch events never come stops holding presses back after this
/// long, in seconds.
const ORPHAN_TIME: f64 = 1.0;
/// The most fingers followed at once.
const MAX_CONTACTS: usize = 16;
/// The most pen touches followed at once.
const MAX_PENS: usize = 16;
/// How far a pen tip's lift may be from the release egui sees and still be the pen's, in points.
/// Safari reports an Apple Pencil's lift twice, as `pointerup` and as `touchend`, a few points
/// apart (3.2 points on an iPad), and eframe makes a release of each, often in one frame.
pub const PEN_RELEASE_SLOP: f32 = 12.0;

/// Whether the release at `at` is the pen tip's lifted at `pen`.
pub(crate) fn is_pen_release(pen: Option<Pos2>, at: Pos2) -> bool {
    pen.is_some_and(|pen| pen.is_finite() && at.is_finite() && pen.distance(at) <= PEN_RELEASE_SLOP)
}

/// What the platform knows about this frame's contacts and egui doesn't.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlatformTaps {
    /// Where a pen tip was lifted: the primary release at this point is a pen tap.
    pub pen_release: Option<Pos2>,
    /// Where a finger went down on the app's canvas since the last frame.
    pub finger_down: Option<Pos2>,
    /// A pen tip is on the screen now.
    pub pen_down: bool,
    /// Touches the platform names a stylus's since the last frame. Safari sends touch events for
    /// an Apple Pencil as well as pen pointer events, and eframe hands them on as egui touches.
    pub stylus: Vec<egui::TouchId>,
}

/// One finger on the screen.
#[derive(Clone, Copy, Debug)]
struct Contact {
    device: egui::TouchDeviceId,
    id: egui::TouchId,
    start: Pos2,
    at: Pos2,
    since: f64,
}

/// The fingers on the view while Pencil mode is on.
#[derive(Debug, Default)]
pub(crate) struct Fingers {
    /// A finger that went down in the open view is still down: primary presses are held back.
    holding: bool,
    /// When `holding` began, for a finger whose touch events never come.
    held_since: f64,
    contacts: Vec<Contact>,
    /// More than one finger was down during this touch: it is a gesture, not a tap.
    several: bool,
    /// The last finger tap in the view: where and when it ended.
    last_tap: Option<(Pos2, f64)>,
    /// A finger double-tap in the view, for the viewport to act on.
    pub(crate) double_tap: Option<Pos2>,
    /// Touches that are a pen's, not a finger's: they are left out of the frame.
    pens: Vec<egui::TouchId>,
}

/// Whether `p` is on the open view: inside it, not under a panel or a window over it, and not on
/// the view's axes.
fn in_view(app: &SolveApp, ctx: &egui::Context, p: Pos2) -> bool {
    let Some(rect) = app.viewport.rect else { return false };
    p.is_finite()
        && crate::toolbar::design_shown(app)
        && rect.contains(p)
        && !crate::shell::covered(app, p)
        && !app.viewport.axes.is_some_and(|axes| axes.contains(p))
        && app.viewport.layer.is_some_and(|layer| ctx.layer_id_at(p).is_none_or(|at| at == layer))
}

/// Before the frame: while a finger that went down in the open view is down, take the primary
/// presses and releases the browser makes for it out of the frame, and note finger double-taps.
/// A pen's touches (Safari's for an Apple Pencil) are not a finger's: they leave the frame, so
/// neither this nor touchscreen navigation takes the pen for a finger, and its presses stay.
/// Does nothing with Pencil mode off.
pub(crate) fn filter(app: &mut SolveApp, ctx: &egui::Context, raw: &mut RawInput, taps: &PlatformTaps) {
    let finger_down = taps.finger_down;
    if !app.preferences.pencil_mode {
        app.viewport.fingers = Fingers::default();
        return;
    }
    let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
    if let Some(p) = finger_down
        && in_view(app, ctx, p)
    {
        let fingers = &mut app.viewport.fingers;
        if !fingers.holding {
            fingers.held_since = now;
        }
        fingers.holding = true;
    }
    let touches: Vec<_> = raw
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Touch { device_id, id, phase, pos, .. } => Some((*device_id, *id, *phase, *pos)),
            _ => None,
        })
        .collect();
    let mut pen_touches = Vec::new();
    for (device, id, phase, pos) in touches {
        let fingers = &mut app.viewport.fingers;
        let pen = if phase == TouchPhase::Start { taps.pen_down || taps.stylus.contains(&id) } else { fingers.pens.contains(&id) };
        if pen {
            match phase {
                TouchPhase::Start if fingers.pens.len() < MAX_PENS && !fingers.pens.contains(&id) => fingers.pens.push(id),
                TouchPhase::End | TouchPhase::Cancel => fingers.pens.retain(|p| *p != id),
                _ => {}
            }
            pen_touches.push(id);
            continue;
        }
        let starts_in_view = phase == TouchPhase::Start && in_view(app, ctx, pos);
        let fingers = &mut app.viewport.fingers;
        match phase {
            TouchPhase::Start => {
                if fingers.contacts.is_empty() {
                    fingers.several = false;
                    if starts_in_view {
                        if !fingers.holding {
                            fingers.held_since = now;
                        }
                        fingers.holding = true;
                    }
                } else {
                    fingers.several = true;
                }
                if pos.is_finite() && fingers.contacts.len() < MAX_CONTACTS && !fingers.contacts.iter().any(|c| c.device == device && c.id == id) {
                    fingers.contacts.push(Contact { device, id, start: pos, at: pos, since: now });
                }
            }
            TouchPhase::Move => {
                if let Some(c) = fingers.contacts.iter_mut().find(|c| c.device == device && c.id == id)
                    && pos.is_finite()
                {
                    c.at = pos;
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                let Some(i) = fingers.contacts.iter().position(|c| c.device == device && c.id == id) else { continue };
                let c = fingers.contacts.remove(i);
                let end = if pos.is_finite() { pos } else { c.at };
                let tap = phase == TouchPhase::End
                    && fingers.holding
                    && !fingers.several
                    && fingers.contacts.is_empty()
                    && now - c.since <= TAP_TIME
                    && end.distance(c.start) <= TAP_SLOP;
                if tap {
                    match fingers.last_tap.take() {
                        Some((at, when)) if now - when <= DOUBLE_TAP_TIME && end.distance(at) <= DOUBLE_TAP_SLOP => fingers.double_tap = Some(end),
                        _ => fingers.last_tap = Some((end, now)),
                    }
                } else {
                    // A drag, a hold or a gesture between two taps: they are not a double-tap.
                    fingers.last_tap = None;
                }
            }
        }
    }
    if !pen_touches.is_empty() {
        drop_pen_touches(raw, &pen_touches);
    }
    let fingers = &mut app.viewport.fingers;
    if !fingers.holding {
        return;
    }
    // A touch's press is also where egui learns the finger is (eframe sends no move before it):
    // the press becomes a move there, so the view still knows the finger is over it and
    // touchscreen navigation can take the gesture, but nothing is clicked.
    for e in &mut raw.events {
        if let Event::PointerButton { pos, button: PointerButton::Primary, .. } = *e {
            *e = Event::PointerMoved(pos);
        }
    }
    // The last finger has lifted (its release was just taken out), or a finger reported down
    // never sent its touch events: presses are the pointer's again.
    let lifted = fingers.contacts.is_empty() && raw.events.iter().any(|e| matches!(e, Event::Touch { phase: TouchPhase::End | TouchPhase::Cancel, .. }));
    let orphaned = fingers.contacts.is_empty() && now - fingers.held_since > ORPHAN_TIME;
    if lifted || orphaned {
        fingers.holding = false;
        fingers.several = false;
    }
}

/// Take a pen's touches out of the frame, with what eframe makes of them: for `touchstart` a
/// primary press just before the touch, for `touchend` a primary release and the pointer gone.
/// The pen's own pointer events already press and release, so each pen tap is then one click,
/// not the two (a double-click) that egui would otherwise count, and the hover stays.
fn drop_pen_touches(raw: &mut RawInput, pen_touches: &[egui::TouchId]) {
    let mut kept: Vec<Event> = Vec::with_capacity(raw.events.len());
    for e in raw.events.drain(..) {
        match &e {
            Event::Touch { id, phase, .. } if pen_touches.contains(id) => {
                // eframe pushes what it makes of a touch right before the touch itself: one of
                // each, so the pen's own press or release before them stays.
                let pressed = |k: &Event| matches!(k, Event::PointerButton { button: PointerButton::Primary, pressed: true, .. });
                let released = |k: &Event| matches!(k, Event::PointerButton { button: PointerButton::Primary, pressed: false, .. });
                match phase {
                    TouchPhase::Start => {
                        if kept.last().is_some_and(pressed) {
                            kept.pop();
                        }
                    }
                    TouchPhase::End | TouchPhase::Cancel => {
                        if kept.last().is_some_and(|k| matches!(k, Event::PointerGone)) {
                            kept.pop();
                        }
                        if kept.last().is_some_and(released) {
                            kept.pop();
                        }
                    }
                    TouchPhase::Move => {}
                }
            }
            _ => kept.push(e),
        }
    }
    raw.events = kept;
}

/// A finger double-tap at `p`: on a planar face of a body, select the face and start a sketch on
/// it (the view turns to face it as New Sketch does). Only with nothing else under way: no tool,
/// window or open sketch. Returns whether a sketch started.
pub(crate) fn finger_double_tap(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    if app.tool.is_some() || app.dialog.is_some() || app.session.active_sketch.is_some() {
        return false;
    }
    let face = crate::viewport::pick(app, proj, p).into_iter().find_map(|hit| match hit {
        Hit::Face { body, index, point } => Some(Sel::Face { body, index, point }),
        _ => None,
    });
    let Some(face) = face else { return false };
    if app.run("select.set", json!({ "items": [face.clone()] })).is_err() {
        return false;
    }
    match crate::dialogs::start_sketch(app, &face) {
        Ok(()) => true,
        Err(why) => {
            app.set_status(why, true);
            false
        }
    }
}

/// What a pen double-tap on `hit` selects: the whole body of a face, edge or vertex; the whole
/// sketch of a sketch curve, point or profile (every curve of the sketch being edited, or the
/// sketch itself for another one). Nothing for anything else.
pub(crate) fn whole(app: &SolveApp, hit: &Hit) -> Option<Vec<Sel>> {
    match hit {
        Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. } => Some(vec![Sel::Body { name: body.clone() }]),
        Hit::SketchCurve { sketch, .. } | Hit::SketchPoint { sketch, .. } | Hit::SketchMidpoint { sketch, .. } | Hit::Profile { sketch, .. } => {
            if app.session.active_sketch == Some(*sketch) {
                let state = crate::open_meshing::session_world(&app.session);
                let ss = state.sketch(*sketch)?;
                let curves: Vec<Sel> = ss.sketch.curves.iter().map(|c| Sel::SketchCurve { id: c.id.clone(), sketch: None }).collect();
                (!curves.is_empty()).then_some(curves)
            } else {
                Some(vec![Sel::Feature { id: *sketch }])
            }
        }
        _ => None,
    }
}
