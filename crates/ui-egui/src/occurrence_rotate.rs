//! Turning a free part with the right button (Preferences ▸ Navigation: Right drag turns parts,
//! off by default): with the design (or the assembly holding it)
//! active, a right-button drag that starts on a part turns its placement freely about the centre
//! of its box, like a trackball: dragging sideways turns it about the view's vertical, up and down
//! about the view's horizontal. The part to turn is found as [`crate::occurrence_drag`] finds the
//! one a left-button drag moves, and the same parts are refused (grounded, held by joints to
//! something fixed, in a rigid group). Each frame runs `occurrence.drag` with `rotate`; the whole
//! drag is one undo step, and Escape puts everything back as it was.
//!
//! The press only marks the part. While it is marked the right button neither orbits the view nor
//! opens the mouse-gesture ring; a press that ends as a click still opens the context menu, and a
//! right-button drag that starts off every part navigates as before.

use solidcraft_engine::geom::prelude::*;
use std::sync::Arc;

use egui::{Pos2, Rect};
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::doc::{Document, apply_point, mat_inverse};

use crate::SolveApp;

/// Degrees turned per point the pointer moves.
const DEGREES_PER_POINT: f64 = 0.5;

/// A right-button press on a part, and the turn it became once the pointer moved.
#[derive(Clone, Debug)]
pub struct Turn {
    /// The placement turned: its exact occurrence path.
    path: Vec<u64>,
    /// The point turned about, in the placement's own coordinates, so it moves with the part.
    centre: Vec3,
    /// Where the pointer was last frame (the press, until the turn begins).
    last: Pos2,
    /// The press has stopped being a click: the part is turning.
    turning: bool,
    /// What Escape goes back to (`edit.checkpoint`), taken when the turn begins.
    checkpoint: Option<u64>,
    /// The turn's undo step on top of the history, as [`crate::occurrence_drag`] keeps one.
    step: Option<Arc<Document>>,
}

/// Whether a right-button press holds a part: the right button belongs to it, not to the camera
/// or the gesture ring, until it is let go.
pub fn holds(app: &SolveApp) -> bool {
    app.viewport.occurrence_turn.is_some()
}

/// Whether a part is turning.
pub fn active(app: &SolveApp) -> bool {
    app.viewport.occurrence_turn.as_ref().is_some_and(|t| t.turning)
}

/// The point to turn the placement `path` about, in world coordinates: the centre of the box
/// around the bodies it places (and those of the placements inside it).
fn centre_of(app: &SolveApp, path: &[u64]) -> Option<Vec3> {
    let s = &app.session;
    let model = s.model.state();
    let world = crate::open_meshing::session_world(s);
    let mut bb = solidcraft_engine::geom::Aabb3::default();
    for (name, _, body_path, _) in s.doc.body_frames(&model) {
        if body_path.starts_with(path)
            && let Some(b) = world.body(&name)
        {
            bb = bb.union(&b.mesh().bounds());
        }
    }
    let c = bb.center();
    (!bb.is_empty() && c.is_finite()).then_some(c)
}

/// Each frame, before the camera reads the pointer: a right-button press on a free part marks it,
/// and a marked press that has moved turns it.
pub fn track(app: &mut SolveApp, ui: &egui::Ui, rect: Rect) {
    let (pressed, down, could_click, origin, pos, mods, others) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.pointer.secondary_down(),
            i.pointer.could_any_button_be_click(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.modifiers,
            i.pointer.primary_down() || i.pointer.middle_down(),
        )
    });
    if pressed && app.viewport.occurrence_turn.is_none() {
        // Modifiers keep their navigation (Shift + right pans in some mouse modes).
        if let Some(a) = origin
            && app.preferences.right_drag_turns_parts
            && !mods.any()
            && !others
            && free(app)
            && rect.contains(a)
            && !crate::shell::covered(app, a)
        {
            begin(app, rect, a);
        }
    }
    let Some(t) = app.viewport.occurrence_turn.clone() else { return };
    if !down {
        end(app);
        return;
    }
    // Something else took the pointer (a context menu held open, a tool or dialog), or undo was
    // pressed mid-turn and took its step: the turn stops where it is.
    if !free(app) || t.step.as_ref().is_some_and(|doc| !app.session.undo.last().is_some_and(|step| Arc::ptr_eq(&step.doc, doc))) {
        end(app);
        return;
    }
    let Some(p) = pos.filter(|p| p.is_finite()) else { return };
    if !t.turning {
        if could_click {
            return;
        }
        let checkpoint = app.session.execute("edit.checkpoint", &json!({})).ok().and_then(|r| r["checkpoint"].as_u64());
        if let Some(t) = app.viewport.occurrence_turn.as_mut() {
            t.turning = true;
            t.checkpoint = checkpoint;
        }
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::AllScroll);
    turn_to(app, rect, &t, p);
}

/// Nothing else has the pointer: no tool, dialog, sketch or menu, and no left-button drag.
fn free(app: &SolveApp) -> bool {
    app.tool.is_none()
        && app.dialog.is_none()
        && app.viewport.context_menu.is_none()
        && crate::sketch_view::active_sketch(app).is_none()
        && !crate::occurrence_drag::active(app)
        && !crate::joint_drag::active(app)
}

/// Mark the part under `a` when nothing holds it in place.
fn begin(app: &mut SolveApp, rect: Rect, a: Pos2) {
    let proj = crate::viewport::projection(app, rect);
    let Some((body, _)) = crate::occurrence_drag::grabbed_face(app, &proj, a) else { return };
    let Some(path) = crate::occurrence_drag::placement_of(app, &body) else { return };
    let s = &app.session;
    if s.drag_group(&path).is_err() {
        return;
    }
    let Some(world) = centre_of(app, &path) else { return };
    let Some(centre) = s.occurrence_placement(&path).ok().and_then(|x| mat_inverse(&x.world)).map(|inv| apply_point(&inv, world)) else {
        return;
    };
    app.viewport.occurrence_turn = Some(Turn { path, centre, last: a, turning: false, checkpoint: None, step: None });
}

/// Turn the part by the pointer's move since last frame.
fn turn_to(app: &mut SolveApp, rect: Rect, t: &Turn, p: Pos2) {
    let d = p - t.last;
    if d.length() < 1e-3 {
        return;
    }
    let Ok(placement) = app.session.occurrence_placement(&t.path) else {
        end(app);
        return;
    };
    let proj = crate::viewport::projection(app, rect);
    let (right, up, _) = proj.cam.basis();
    // Sideways about the view's up, so the near side follows the pointer; down (screen y grows
    // downward) about the view's right, which takes the near side down.
    let axis = up * f64::from(d.x) + right * f64::from(d.y);
    let angle = f64::from(d.length()) * DEGREES_PER_POINT.to_radians();
    let origin = apply_point(&placement.world, t.centre);
    if !origin.is_finite() || !axis.is_finite() || axis.len() < 1e-9 {
        return;
    }
    let params = json!({
        "path": t.path,
        "rotate": {"origin": [origin.x, origin.y, origin.z], "axis": [axis.x, axis.y, axis.z], "angle": angle},
        "continuous": true
    });
    let step = crate::occurrence_drag::run_frame(app, params, t.step.clone());
    if let Some(t) = app.viewport.occurrence_turn.as_mut() {
        t.step = step;
        t.last = p;
    }
}

/// Let go: the part stays as it was turned, its position captured.
pub fn end(app: &mut SolveApp) {
    if let Some(Turn { checkpoint: Some(checkpoint), .. }) = app.viewport.occurrence_turn.take() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint, "forget": true}));
    }
}

/// Escape: the part turns back, and the turn leaves no undo step. True when a turn was cancelled.
pub fn cancel(app: &mut SolveApp) -> bool {
    let Some(t) = app.viewport.occurrence_turn.take() else { return false };
    if !t.turning {
        return false;
    }
    match t.checkpoint {
        Some(checkpoint) => drop(app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint}))),
        None if t.step.is_some() => drop(app.session.execute("edit.undo", &json!({}))),
        None => {}
    }
    true
}

#[cfg(test)]
#[path = "occurrence_rotate_tests.rs"]
mod tests;
