//! Copy, Cut and Paste from the keyboard (Ctrl+C / Ctrl+X / Ctrl+V, ⌘C / ⌘X / ⌘V) and the context
//! menu (#1480).
//!
//! In a sketch they copy the selected sketch geometry (`sketch.copy`) and paste it at the cursor
//! (`sketch.paste` with `at`); Cut copies the sketch geometry and deletes it (`sketch.cut`), or offset from the original when the cursor is not over the
//! viewport. Outside a sketch they copy the selected timeline features and paste them at the
//! history marker (`timeline.copy` / `timeline.paste`).
//!
//! The window system turns Ctrl+C / Ctrl+V into clipboard events, not key presses, and only sends
//! the paste event while the system clipboard holds text. So a copy also puts a short line of
//! text on the system clipboard (a menu's Copy leaves it for the next frame, which has the
//! context to write it).

use std::sync::Mutex;

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;

/// Text for the system clipboard, written on the next frame.
static PENDING_TEXT: Mutex<Option<String>> = Mutex::new(None);

fn leave_text(text: String) {
    if let Ok(mut pending) = PENDING_TEXT.lock() {
        *pending = Some(text);
    }
}

/// The selected sketch geometry of the active sketch.
fn sketch_selection(app: &SolveApp) -> Vec<String> {
    app.session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::SketchCurve { id } | Sel::SketchPoint { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Is there something Paste would paste here?
pub fn can_paste(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        app.session.sketch_clipboard.is_some()
    } else {
        app.linked_design_clipboard.is_some() || !app.session.clipboard.is_empty()
    }
}

/// Copy the saved root design. The path stays in this app, outside per-document sessions.
pub fn copy_design(app: &mut SolveApp) -> bool {
    let Some(path) = app.session.path.clone().filter(|_| !app.session.is_dirty()) else {
        app.set_status("Save this design before copying it as a linked component", true);
        return false;
    };
    app.linked_design_clipboard = Some(path);
    leave_text(format!("SolidCraft linked design: {}", app.session.doc.name));
    app.set_status("Copied design: switch to the assembly and paste to insert a linked component", false);
    true
}

/// Copy the selection: sketch geometry in a sketch, otherwise timeline features.
pub fn copy(app: &mut SolveApp) -> bool {
    if app.session.active_sketch.is_none() && app.tree.picked_components == [0] {
        return copy_design(app);
    }
    if app.session.active_sketch.is_some() {
        let entities = sketch_selection(app);
        if entities.is_empty() {
            app.set_status("Select sketch geometry to copy", true);
            return false;
        }
        let Ok(out) = app.run("sketch.copy", json!({ "entities": entities })) else { return false };
        app.linked_design_clipboard = None;
        let n = out.get("curves").and_then(Value::as_u64).unwrap_or(0) + out.get("points").and_then(Value::as_u64).unwrap_or(0);
        let k = out.get("constraints").and_then(Value::as_u64).unwrap_or(0);
        let text = format!("SolidCraft sketch geometry: {n} entities, {k} constraints and dimensions");
        app.set_status(format!("Copied {n} sketch entities with {k} constraints and dimensions"), false);
        leave_text(text);
        return true;
    }
    let features = crate::timeline::selected_feature_ids(app);
    if features.is_empty() {
        app.set_status("Select sketch geometry or timeline features to copy", true);
        return false;
    }
    let Ok(out) = app.run("timeline.copy", json!({ "features": features })) else { return false };
    app.linked_design_clipboard = None;
    let names: Vec<&str> = out.get("copied").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    app.set_status(format!("Copied {}", names.join(", ")), false);
    leave_text(format!("SolidCraft features: {}", names.join(", ")));
    true
}

/// Cut the selected sketch geometry: copy it, then delete it (one undo step).
pub fn cut(app: &mut SolveApp) -> bool {
    if app.session.active_sketch.is_none() {
        app.set_status("Cut works on sketch geometry: edit a sketch and select some", true);
        return false;
    }
    let entities = sketch_selection(app);
    if entities.is_empty() {
        app.set_status("Select sketch geometry to cut", true);
        return false;
    }
    let Ok(out) = app.run("sketch.cut", json!({ "entities": entities })) else { return false };
    app.linked_design_clipboard = None;
    let n = out.get("curves").and_then(Value::as_u64).unwrap_or(0) + out.get("points").and_then(Value::as_u64).unwrap_or(0);
    app.set_status(format!("Cut {n} sketch entities"), false);
    leave_text(format!("SolidCraft sketch geometry: {n} entities"));
    true
}

/// The point of the active sketch under `at` (screen), when `at` is over the viewport.
fn sketch_point(app: &SolveApp, at: Option<Pos2>) -> Option<[f64; 2]> {
    let at = at?;
    let rect = app.viewport.rect?;
    // Under a card or panel floating over the viewport is not over the sketch.
    if !rect.contains(at) || crate::shell::covered(app, at) {
        return None;
    }
    let proj = crate::viewport::projection(app, rect);
    let (p, _) = crate::viewport::sketch_point_at(app, &proj, at)?;
    Some([p.x, p.y])
}

/// Paste: in a sketch, the copied sketch geometry centred at `at` (or offset from the original
/// when `at` is not over the sketch); otherwise the copied features at the history marker.
pub fn paste(app: &mut SolveApp, at: Option<Pos2>) -> bool {
    if app.session.active_sketch.is_some() {
        if app.session.sketch_clipboard.is_none() {
            app.set_status("Nothing to paste: copy sketch geometry first", true);
            return false;
        }
        let params = match sketch_point(app, at) {
            Some(p) => json!({ "at": p }),
            None => json!({}),
        };
        let Ok(out) = app.run("sketch.paste", params) else { return false };
        let n = out.get("curves").and_then(Value::as_array).map_or(0, Vec::len) + out.get("points").and_then(Value::as_array).map_or(0, Vec::len);
        app.set_status(format!("Pasted {n} sketch entities (selected: drag them, or press M to move them)"), false);
        return true;
    }
    if let Some(path) = app.linked_design_clipboard.clone() {
        let (parent_path, translate) = paste_placement(app, at);
        let Ok(out) = app.run("component.insert_design", json!({"path": path, "reuse": true, "translate":translate})) else { return false };
        if let Some(id) = out.get("occurrence").and_then(Value::as_u64) {
            let mut movement = crate::browser::OccMove::new(id);
            movement.path = parent_path;
            movement.path.push(id);
            movement.baseline = app.session.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.transform);
            app.tree.occurrence_move = Some(movement);
        }
        app.set_status("Pasted linked component: use Move/Copy to position it", false);
        return true;
    }
    if app.session.clipboard.is_empty() {
        app.set_status("Nothing to paste: copy timeline features first", true);
        return false;
    }
    app.run("timeline.paste", json!({})).is_ok()
}

/// Place the copied design's origin on the pointed solid surface, otherwise its parent's
/// XY plane. With no viewport point or a parallel/behind-camera ray, use the parent origin.
fn paste_placement(app: &SolveApp, at: Option<Pos2>) -> (Vec<u64>, [f64; 3]) {
    use solidcraft_engine::{doc, geom::Vec3};
    let path = app.session.active_occurrence_path.clone().unwrap_or_else(|| app.session.doc.first_path(app.session.active_component));
    let frame = app.session.occurrence_placement(&path).map(|p| p.world).unwrap_or(doc::IDENTITY);
    let origin = doc::apply_point(&frame, Vec3::ZERO);
    let at = at.zip(app.viewport.rect).filter(|(point, rect)| rect.contains(*point));
    let point = at
        .and_then(|(at, rect)| {
            let projection = crate::viewport::projection(app, rect);
            if let Some(point) = crate::viewport::pick_all(app, &projection, at).into_iter().find_map(|hit| match hit {
                crate::viewport::Hit::Face { point, .. } => Some(point),
                _ => None,
            }) {
                return point.is_finite().then_some(point);
            }
            let (ray_origin, ray_direction) = projection.ray(at);
            let normal = doc::apply_vector(&frame, Vec3::Z);
            let denominator = ray_direction.dot(normal);
            if denominator.abs() < 1e-9 {
                return None;
            }
            let distance = (origin - ray_origin).dot(normal) / denominator;
            let point = ray_origin + ray_direction * distance;
            (distance >= 0.0 && point.is_finite()).then_some(point)
        })
        .unwrap_or(origin);
    let local = doc::mat_inverse(&frame).map(|inverse| doc::apply_point(&inverse, point)).filter(|p| p.is_finite()).unwrap_or(Vec3::ZERO);
    (path, [local.x, local.y, local.z])
}

/// The frame's Copy, Cut and Paste events (and Ctrl+C / Ctrl+X / Ctrl+V sent as key presses), plus any
/// clipboard text a menu's Copy left. Called by the global shortcuts when no text field has the
/// keyboard.
pub fn handle(app: &mut SolveApp, ctx: &egui::Context) {
    let (copies, cuts, pastes, pointer) = ctx.input(|i| {
        let mut copies = 0;
        let mut cuts = 0;
        let mut pastes = 0;
        // Firefox on Linux sends primary-selection Paste events during middle-click navigation.
        // Include release frames and clicks completed within one frame; explicit Ctrl/Cmd+V still works.
        let middle_navigation = i.pointer.button_down(egui::PointerButton::Middle)
            || i.pointer.button_pressed(egui::PointerButton::Middle)
            || i.pointer.button_released(egui::PointerButton::Middle);
        for e in &i.events {
            match e {
                egui::Event::Copy => copies += 1,
                egui::Event::Cut => cuts += 1,
                egui::Event::Paste(_) if !middle_navigation => pastes += 1,
                egui::Event::Key { key: egui::Key::C, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    copies += 1
                }
                egui::Event::Key { key: egui::Key::X, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    cuts += 1
                }
                egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift && !modifiers.alt => {
                    pastes += 1
                }
                _ => {}
            }
        }
        (copies, cuts, pastes, i.pointer.latest_pos())
    });
    if copies > 0 {
        copy(app);
    }
    if cuts > 0 {
        cut(app);
    }
    if pastes > 0 {
        paste(app, pointer);
    }
    flush(ctx);
}

/// Write clipboard text a copy left.
pub fn flush(ctx: &egui::Context) {
    if let Some(text) = PENDING_TEXT.lock().ok().and_then(|mut p| p.take()) {
        ctx.copy_text(text);
    }
}

#[cfg(test)]
#[path = "edit_clipboard_tests.rs"]
mod tests;
