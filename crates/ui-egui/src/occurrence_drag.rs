//! Dragging a free part puts it somewhere else (#2804): with the design (or the assembly holding
//! it) active, a left-button drag on a part moves its placement in that assembly, the one on the
//! picked body's exact path just inside the active placement. The point grabbed follows the
//! pointer in the plane through it that faces the view. Each frame runs `occurrence.drag`; the
//! whole drag is one undo step, and Escape puts everything back as it was.
//!
//! Floating chains move together whichever member is grabbed. Parts held
//! by joints to something fixed go to `joint_drag`; grounded parts and the active component's
//! own bodies are left to the selection box.

use std::sync::Arc;

use egui::Pos2;
use serde_json::json;
use solidcraft_engine::doc::{Document, apply_point, mat_inverse};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

/// A drag under way.
#[derive(Clone, Debug)]
pub struct Drag {
    /// The placement moved: its exact occurrence path.
    path: Vec<u64>,
    /// The point grabbed, in the placement's own coordinates.
    local: Vec3,
    /// The plane the pointer moves the point in: through where it was grabbed, facing the view.
    origin: Vec3,
    normal: Vec3,
    /// What Escape goes back to (`edit.checkpoint`).
    checkpoint: Option<u64>,
    /// The drag's undo step is on top of the history (it went in with the first frame that
    /// moved); the design it holds tells it from a step that replaced it.
    step: Option<Arc<Document>>,
}

/// The placement a drag on `body` moves: the one on its path just inside the active placement.
fn placement_of(app: &SolveApp, body: &str) -> Option<Vec<u64>> {
    let s = &app.session;
    let (_, _, path, _) = s.doc.body_frames(&s.model.state()).into_iter().find(|f| f.0 == body)?;
    let active = if s.active_component == 0 {
        Vec::new()
    } else {
        s.active_occurrence_path
            .clone()
            .filter(|p| s.occurrence_placement(p).is_ok_and(|x| x.component == s.active_component))
            .unwrap_or_else(|| s.doc.first_path(s.active_component))
    };
    if path.len() <= active.len() || !path.starts_with(&active) {
        return None;
    }
    path.get(..active.len() + 1).map(<[u64]>::to_vec)
}

/// Begin a drag on the part under `p` when nothing holds it in place. True when one began.
pub fn start(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    // The face under the pointer, past the part's own edges and vertices (not past sketches or
    // construction geometry in front of it).
    let hits = crate::viewport::pick(app, proj, p);
    let Some((body, at)) =
        hits.into_iter().take_while(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })).find_map(|h| match h {
            Hit::Face { body, point, .. } => Some((body, point)),
            _ => None,
        })
    else {
        return false;
    };
    let Some(path) = placement_of(app, &body) else { return false };
    let s = &app.session;
    if s.drag_group(&path).is_err() {
        return false;
    }
    let Some(local) = s.occurrence_placement(&path).ok().and_then(|x| mat_inverse(&x.world)).map(|inv| apply_point(&inv, at)) else {
        return false;
    };
    let checkpoint = app.session.execute("edit.checkpoint", &json!({})).ok().and_then(|r| r["checkpoint"].as_u64());
    app.viewport.occurrence_drag = Some(Drag { path, local, origin: at, normal: proj.ray(p).1, checkpoint, step: None });
    true
}

pub fn active(app: &SolveApp) -> bool {
    app.viewport.occurrence_drag.is_some()
}

/// Bring the grabbed point to the pointer at `p`.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(d) = app.viewport.occurrence_drag.clone() else { return };
    // Something else took the drag's undo step (undo pressed mid-drag): the drag ends there.
    if let Some(doc) = &d.step
        && !app.session.undo.last().is_some_and(|step| Arc::ptr_eq(&step.doc, doc))
    {
        end(app);
        return;
    }
    let (o, dir) = proj.ray(p);
    let facing = dir.dot(d.normal);
    if facing.abs() < 1e-9 {
        return;
    }
    let target = o + dir * ((d.origin - o).dot(d.normal) / facing);
    let Ok(placement) = app.session.occurrence_placement(&d.path) else {
        end(app);
        return;
    };
    let at = apply_point(&placement.world, d.local);
    if !target.is_finite() || !at.is_finite() || target.dist(at) < 1e-9 {
        return;
    }
    let params = json!({"path": d.path, "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z], "continuous": true});
    // One undo step for the whole drag: the first frame's, kept on top while the others run.
    let own = if d.step.is_some() { app.session.undo.pop() } else { None };
    let before = app.session.doc.clone();
    let pushed = app.run("occurrence.drag", params).is_ok() && !Arc::ptr_eq(&before, &app.session.doc);
    match own {
        Some(step) => {
            if pushed {
                app.session.undo.pop();
            }
            app.session.undo.push(step);
        }
        None if pushed => {
            let step = app.session.undo.last().map(|step| step.doc.clone());
            if let Some(d) = app.viewport.occurrence_drag.as_mut() {
                d.step = step;
            }
        }
        None => {}
    }
}

/// Let go: the part stays where it was dragged.
pub fn end(app: &mut SolveApp) {
    if let Some(Drag { checkpoint: Some(checkpoint), .. }) = app.viewport.occurrence_drag.take() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint, "forget": true}));
    }
}

/// Escape: the part goes back, and the drag leaves no undo step. True when a drag was cancelled.
pub fn cancel(app: &mut SolveApp) -> bool {
    let Some(d) = app.viewport.occurrence_drag.take() else { return false };
    match d.checkpoint {
        Some(checkpoint) => drop(app.session.execute("edit.restore_checkpoint", &json!({"checkpoint": checkpoint}))),
        // Without a checkpoint the drag's own step takes it back.
        None if d.step.is_some() => drop(app.session.execute("edit.undo", &json!({}))),
        None => {}
    }
    true
}

#[cfg(test)]
#[path = "occurrence_drag_tests.rs"]
mod tests;
