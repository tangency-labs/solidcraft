//! Fillet by dragging, in Creative Mode: press on a body edge and pull the pointer the way the
//! fillet grows (into the body on an outside edge, into the open corner on an inside one, as the
//! Fillet window's arrow points: [`crate::canvas::blend_grow_dir`]). Once the pointer has moved
//! far enough that way on screen, Fillet starts on the edge and its radius follows the pointer in
//! steps that suit the zoom (hold Alt or Ctrl for a free value); the release applies it. A press
//! that goes any other way stays a box selection.

use egui::Pos2;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::viewport::{Hit, Proj};

/// Screen distance the pointer must travel before a press counts as a pull.
const START_PX: f32 = 6.0;
/// How closely (cosine) a drag must follow the grow direction on screen to start a fillet.
const ALIGNED: f32 = 0.7;

/// A press on an edge that may turn into a fillet.
#[derive(Clone, Debug)]
pub struct Gesture {
    press: Pos2,
    sel: Sel,
    /// The edge's point under the press, and the unit direction the fillet grows in.
    base: Vec3,
    dir: Vec3,
    /// Where along `dir` the press was: the radius is measured from here.
    grab: f64,
    /// Fillet has started (the window is open and follows the pointer).
    started: bool,
}

#[derive(Debug, Default)]
pub struct State {
    gesture: Option<Gesture>,
}

/// A fillet pull has started: the box selection stays out of it.
pub fn started(app: &SolveApp) -> bool {
    app.viewport.drag_fillet.gesture.as_ref().is_some_and(|g| g.started)
}

/// Note a press at `at` when Creative Mode is on and it is on a body edge.
pub fn press(app: &mut SolveApp, proj: &Proj, at: Pos2) {
    app.viewport.drag_fillet.gesture = None;
    if !crate::creative_mode::on(app) {
        return;
    }
    let hits = crate::viewport::pick(app, proj, at);
    let Some((Hit::Edge { body, index, point, .. }, sel @ Sel::Edge { .. })) = crate::viewport::candidate(app, &hits) else { return };
    let Some(dir) = app.session.world_state().ok().and_then(|st| st.body(&body).and_then(|b| crate::canvas::blend_grow_dir(&b.mesh(), index, point)))
    else {
        return;
    };
    let Some(grab) = crate::canvas::along(proj, at, point, dir) else { return };
    app.viewport.drag_fillet.gesture = Some(Gesture { press: at, sel, base: point, dir, grab, started: false });
}

/// Whether the move from the press to `p` follows the grow direction on screen.
fn pulls(proj: &Proj, g: &Gesture, p: Pos2) -> bool {
    let delta = p - g.press;
    if delta.length() < START_PX {
        return false;
    }
    proj.to_screen(g.base)
        .zip(proj.to_screen(g.base + g.dir))
        .map(|(a, b)| b - a)
        .filter(|n| n.length() > 1e-3)
        .is_some_and(|n| (n / n.length()).dot(delta / delta.length()) > ALIGNED)
}

/// The pointer is at `p` with the button down.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(mut g) = app.viewport.drag_fillet.gesture.clone() else { return };
    if !g.started {
        if app.dialog.is_some() || crate::drag_extrude::started(app) || !pulls(proj, &g, p) {
            return;
        }
        crate::viewport::select(app, Some(g.sel.clone()), false, false);
        app.start("solid.fillet");
        if !matches!(app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Fillet { chamfer: false, .. })) {
            app.viewport.drag_fillet.gesture = None;
            return;
        }
        g.started = true;
    }
    let Some(v) = crate::canvas::along(proj, p, g.base, g.dir) else {
        app.viewport.drag_fillet.gesture = Some(g);
        return;
    };
    let step = crate::canvas::snap_step(app.cam.half_height());
    let free = crate::tool_modifiers::manipulator_free();
    let travelled = (v - g.grab).max(0.0);
    let radius = if free { travelled } else { (travelled / step).round() * step };
    // A fillet needs some radius: the smallest step keeps the window valid until the pointer moves on.
    let radius = if radius > 0.0 { radius } else { step };
    app.viewport.drag_fillet.gesture = Some(g);
    let text = if free {
        crate::canvas::format_len(&app.session.doc, radius)
    } else {
        crate::canvas::format_len_drag(&app.session.doc, radius, step, app.preferences.length_decimals)
    };
    if let Some(Kind::Fillet { radius, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) {
        *radius = text;
    }
}

/// The button is up: the release applies the fillet as OK would.
pub fn end(app: &mut SolveApp) {
    let pulled = app.viewport.drag_fillet.gesture.take().is_some_and(|g| g.started);
    if pulled && let Some(d) = app.dialog.as_mut().filter(|d| matches!(d.kind, Kind::Fillet { .. })) {
        d.confirm = true;
    }
}
