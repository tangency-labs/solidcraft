//! Copy, Cut and Paste from the keyboard (Ctrl+C / Ctrl+X / Ctrl+V, ⌘C / ⌘X / ⌘V) and the context
//! menu (#1480).
//!
//! In a sketch they copy the selected sketch geometry (`sketch.copy`) and paste it at the cursor
//! (`sketch.paste` with `at`); Cut copies the sketch geometry and deletes it (`sketch.cut`), or offset from the original when the cursor is not over the
//! viewport. Outside a sketch they copy the selected timeline features and paste them at the
//! history marker (`timeline.copy` / `timeline.paste`), the selected bodies and paste copies of
//! them into the active component, where the bodies are (`component.copy_bodies`), or the
//! selected components and paste new instances of them there (`occurrence.copy`), or with
//! Ctrl+Shift+V new independent copies (`component.paste_new`), where they are too. Paste pastes
//! whichever was copied last. Cut on bodies or components only marks them: Paste moves them into
//! the component (`component.move_bodies`, `occurrence.reparent`, where they are), and a copy or
//! Esc before that leaves them be. A component placed more than once (an instance) isn't cut.
//!
//! The window system turns Ctrl+C / Ctrl+V into clipboard events, not key presses, and only sends
//! the paste event while the system clipboard holds text. So a copy also puts a short line of
//! text on the system clipboard (a menu's Copy leaves it for the next frame, which has the
//! context to write it).

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;

/// Leave text for the system clipboard: this app's next frame writes it ([`flush`]).
fn leave_text(app: &mut SolveApp, text: String) {
    app.clipboard_text = Some(text);
}

/// The selected sketch geometry of the active sketch.
fn sketch_selection(app: &SolveApp) -> Vec<String> {
    app.session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::SketchCurve { id, .. } | Sel::SketchPoint { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// The selected component placements (by path).
fn placement_selection(app: &SolveApp) -> Vec<Vec<u64>> {
    app.session.selection.iter().filter_map(|s| if let Sel::Occurrence { path } = s { Some(path.clone()) } else { None }).collect()
}

/// The selected bodies.
fn body_selection(app: &SolveApp) -> Vec<String> {
    app.session.selection.iter().filter_map(|s| if let Sel::Body { name } = s { Some(name.clone()) } else { None }).collect()
}

/// Is there something Copy would copy?
pub fn something_to_copy(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        !sketch_selection(app).is_empty()
    } else {
        app.tree.picked_components == [0]
            || !crate::timeline::selected_feature_ids(app).is_empty()
            || !body_selection(app).is_empty()
            || !placement_selection(app).is_empty()
    }
}

/// Is there something Paste would paste here?
pub fn can_paste(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        app.session.sketch_clipboard.is_some()
    } else if app.linked_design_clipboard.is_some() {
        // A copied design is what pastes, and never into itself.
        !design_copied_here(app)
    } else {
        !app.session.clipboard.is_empty()
            || !app.cut_bodies.is_empty()
            || !app.cut_placements.is_empty()
            || !app.copied_bodies.is_empty()
            || !app.copied_placements.is_empty()
    }
}

/// The design copied (to paste as a linked component) is the one open: it can't go into itself.
fn design_copied_here(app: &SolveApp) -> bool {
    app.linked_design_clipboard.is_some() && app.linked_design_clipboard == app.session.path
}

/// Whether copied components are what Paste pastes now (not features, nor a design to link).
pub fn pasting_components(app: &SolveApp) -> bool {
    app.session.active_sketch.is_none()
        && app.session.clipboard.is_empty()
        && app.linked_design_clipboard.is_none()
        && !app.copied_placements.is_empty()
}

/// Whether copied features (sketches, …) are what Paste pastes now (a Sketches folder's menu
/// offers them).
pub fn pasting_features(app: &SolveApp) -> bool {
    app.session.active_sketch.is_none() && app.linked_design_clipboard.is_none() && !app.session.clipboard.is_empty()
}

/// Whether copied or cut bodies are what Paste pastes now (a Bodies folder's menu offers them).
pub fn pasting_bodies(app: &SolveApp) -> bool {
    app.session.active_sketch.is_none()
        && app.session.clipboard.is_empty()
        && app.linked_design_clipboard.is_none()
        && (!app.copied_bodies.is_empty() || !app.cut_bodies.is_empty())
}

/// [`paste_into`], then what was pasted into `group` of the component's `folder`: a group's menu
/// pastes into the group (the pasted bodies, which a body paste leaves selected, or the pasted
/// sketches).
pub fn paste_into_group(app: &mut SolveApp, at: Option<Pos2>, into: Option<(u64, Vec<u64>)>, group: Option<u64>, folder: &str) -> bool {
    let (Some(group), Some((component, path))) = (group, into.clone()) else { return paste_into(app, at, false, into) };
    if folder == "sketches" && pasting_features(app) {
        let Ok(out) = app.run("timeline.paste", json!({ "component": component, "component_path": path })) else { return false };
        let items: Vec<String> = out
            .get("pasted")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| m.get("id").and_then(Value::as_u64))
            .filter(|id| app.session.doc.feature(*id).is_some_and(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. })))
            .map(|id| id.to_string())
            .collect();
        if !items.is_empty() {
            let _ = app.run("browser.move", json!({ "component": component, "folder": "sketches", "items": items, "group": group }));
        }
        return true;
    }
    if !paste_into(app, at, false, into) {
        return false;
    }
    let items: Vec<String> =
        app.session.selection.iter().filter_map(|s| if let Sel::Body { name } = s { Some(name.clone()) } else { None }).collect();
    if !items.is_empty() {
        let _ = app.run("browser.move", json!({ "component": component, "folder": "bodies", "items": items, "group": group }));
    }
    true
}

/// Is there something Paste would paste into `component` (a component's menu)? Copied components
/// never go inside themselves or what they hold.
pub fn can_paste_into(app: &SolveApp, component: u64) -> bool {
    let placements = if pasting_components(app) {
        &app.copied_placements
    } else if app.session.clipboard.is_empty() && app.linked_design_clipboard.is_none() && !app.cut_placements.is_empty() {
        &app.cut_placements
    } else {
        return can_paste(app);
    };
    placements
        .iter()
        .filter_map(|path| app.session.occurrence_placement(path).ok())
        .all(|p| !app.session.doc.inside_component(component, p.component))
}

/// Empty every clipboard (features, sketch geometry, bodies, components, a design): a copy that
/// fails leaves nothing behind it, so Paste never pastes what was copied before.
fn clear_clipboard(app: &mut SolveApp) {
    app.session.clipboard.clear();
    app.session.sketch_clipboard = None;
    replace_clipboard(app);
}

/// A new copy or cut replaces the copied bodies, components, cut bodies and design (copied
/// features and sketch geometry are replaced by the commands that copy them).
fn replace_clipboard(app: &mut SolveApp) {
    app.copied_bodies.clear();
    app.copied_placements.clear();
    app.cut_bodies.clear();
    app.cut_placements.clear();
    app.linked_design_clipboard = None;
}

/// Leave bodies that were cut where they are (Esc, or another copy).
pub fn drop_cut(app: &mut SolveApp) {
    app.cut_bodies.clear();
    app.cut_placements.clear();
}

/// A copy (or cut) that can't be done: it says why, and the clipboard is left empty.
fn copy_failed(app: &mut SolveApp, why: Option<&str>) -> bool {
    clear_clipboard(app);
    if let Some(why) = why {
        app.set_status(why, true);
    }
    false
}

/// Copy the saved root design. The path stays in this app, outside per-document sessions.
pub fn copy_design(app: &mut SolveApp) -> bool {
    let Some(path) = app.session.path.clone().filter(|_| !app.session.is_dirty()) else {
        return copy_failed(app, Some("Save this design before copying it as a linked component"));
    };
    replace_clipboard(app);
    app.linked_design_clipboard = Some(path);
    leave_text(app, format!("SolidCraft linked design: {}", app.session.doc.name));
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
            return copy_failed(app, Some("Select sketch geometry to copy"));
        }
        // The command says why when it fails.
        let Ok(out) = app.run("sketch.copy", json!({ "entities": entities })) else { return copy_failed(app, None) };
        replace_clipboard(app);
        let n = out.get("curves").and_then(Value::as_u64).unwrap_or(0) + out.get("points").and_then(Value::as_u64).unwrap_or(0);
        let k = out.get("constraints").and_then(Value::as_u64).unwrap_or(0);
        let text = format!("SolidCraft sketch geometry: {n} entities, {k} constraints and dimensions");
        app.set_status(format!("Copied {n} sketch entities with {k} constraints and dimensions"), false);
        leave_text(app, text);
        return true;
    }
    let features = crate::timeline::selected_feature_ids(app);
    let bodies = body_selection(app);
    let placements = placement_selection(app);
    // Components picked (in the browser): instances or copies of them paste.
    if features.is_empty() && !placements.is_empty() {
        // The last copy is what Paste pastes: what was copied before gives way.
        app.session.clipboard.clear();
        replace_clipboard(app);
        let names: Vec<String> = placements.iter().filter_map(|p| app.session.occurrence_placement(p).ok()).map(|p| p.name).collect();
        app.set_status(format!("Copied {} (Ctrl+V pastes an instance, Ctrl+Shift+V a new copy)", names.join(", ")), false);
        leave_text(app, format!("SolidCraft components: {}", names.join(", ")));
        app.copied_placements = placements;
        return true;
    }
    // Bodies picked (in the browser or the view) with no features: copies of them paste.
    if features.is_empty() && !bodies.is_empty() {
        app.session.clipboard.clear();
        replace_clipboard(app);
        app.set_status(format!("Copied {}", bodies.join(", ")), false);
        leave_text(app, format!("SolidCraft bodies: {}", bodies.join(", ")));
        app.copied_bodies = bodies;
        return true;
    }
    if features.is_empty() {
        return copy_failed(app, Some("Select sketch geometry, bodies or timeline features to copy"));
    }
    let Ok(out) = app.run("timeline.copy", json!({ "features": features })) else { return copy_failed(app, None) };
    replace_clipboard(app);
    let names: Vec<&str> = out.get("copied").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    app.set_status(format!("Copied {}", names.join(", ")), false);
    leave_text(app, format!("SolidCraft features: {}", names.join(", ")));
    true
}

/// Why the selected bodies can't be cut (`None`: they can): none selected, a locked one, or one
/// that is a placed copy's (it is moved through its component).
fn body_cut_refused(app: &SolveApp, bodies: &[String]) -> Option<String> {
    if bodies.is_empty() {
        return Some("Select bodies (or, in a sketch, sketch geometry) to cut".into());
    }
    let state = app.session.model.state();
    if let Some(b) = bodies.iter().find(|b| state.body(b).is_none()) {
        return Some(format!("{b} is a placed copy's body: cut it from its component's first placement"));
    }
    bodies.iter().find(|b| app.session.visibility.locked_bodies.contains(b)).map(|b| format!("{b} is locked"))
}

/// Why the selected components can't be cut (`None`: they can): one placed more than once (an
/// instance: cutting it would take every instance along), or held by a rigid group.
fn placement_cut_refused(app: &SolveApp, placements: &[Vec<u64>]) -> Option<String> {
    for path in placements {
        let Ok(p) = app.session.occurrence_placement(path) else { return Some("that component is gone".into()) };
        if app.session.doc.occurrences.iter().filter(|o| o.component == p.component).count() > 1 {
            return Some(format!("{} is placed more than once: an instance can't be cut (copy it instead)", p.name));
        }
        if app.session.held_by_rigid_group(p.occurrence).is_some() {
            return Some(format!("{} is held by a rigid group: Unlock Contents first", p.name));
        }
    }
    None
}

/// Is there something Cut would cut: sketch geometry in a sketch, otherwise components or bodies
/// that may move.
pub fn can_cut(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        return !sketch_selection(app).is_empty();
    }
    let placements = placement_selection(app);
    if placements.is_empty() { body_cut_refused(app, &body_selection(app)).is_none() } else { placement_cut_refused(app, &placements).is_none() }
}

/// Cut: in a sketch, the selected sketch geometry (copied, then deleted: one undo step); otherwise
/// the selected bodies, which stay until Paste moves them.
pub fn cut(app: &mut SolveApp) -> bool {
    if app.session.active_sketch.is_none() {
        let placements = placement_selection(app);
        if !placements.is_empty() {
            if let Some(why) = placement_cut_refused(app, &placements) {
                return copy_failed(app, Some(&why));
            }
            clear_clipboard(app);
            let names: Vec<String> = placements.iter().filter_map(|p| app.session.occurrence_placement(p).ok()).map(|p| p.name).collect();
            app.set_status(format!("Cut {}: paste to move it into a component (Esc leaves it)", names.join(", ")), false);
            leave_text(app, format!("SolidCraft components: {}", names.join(", ")));
            app.cut_placements = placements;
            return true;
        }
        let bodies = body_selection(app);
        if let Some(why) = body_cut_refused(app, &bodies) {
            return copy_failed(app, Some(&why));
        }
        clear_clipboard(app);
        app.set_status(format!("Cut {}: paste to move it into a component (Esc leaves it)", bodies.join(", ")), false);
        leave_text(app, format!("SolidCraft bodies: {}", bodies.join(", ")));
        app.cut_bodies = bodies;
        return true;
    }
    let entities = sketch_selection(app);
    if entities.is_empty() {
        return copy_failed(app, Some("Select sketch geometry to cut"));
    }
    let Ok(out) = app.run("sketch.cut", json!({ "entities": entities })) else { return copy_failed(app, None) };
    replace_clipboard(app);
    let n = out.get("curves").and_then(Value::as_u64).unwrap_or(0) + out.get("points").and_then(Value::as_u64).unwrap_or(0);
    app.set_status(format!("Cut {n} sketch entities"), false);
    leave_text(app, format!("SolidCraft sketch geometry: {n} entities"));
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
/// when `at` is not over the sketch); otherwise the copied features at the history marker, or
/// the copied bodies or components into the active component (components as new instances, or
/// with `new_copy` as independent copies).
pub fn paste(app: &mut SolveApp, at: Option<Pos2>) -> bool {
    paste_as(app, at, false)
}

/// [`paste`]; `new_copy`: copied components paste as independent copies (Ctrl+Shift+V).
pub fn paste_as(app: &mut SolveApp, at: Option<Pos2>, new_copy: bool) -> bool {
    paste_into(app, at, new_copy, None)
}

/// [`paste_as`], copied bodies and components going into `into` (a component and the placement
/// of it to place them by: a component's menu) instead of the active component.
pub fn paste_into(app: &mut SolveApp, at: Option<Pos2>, new_copy: bool, into: Option<(u64, Vec<u64>)>) -> bool {
    let into = into.unwrap_or_else(|| {
        let active = app.session.active_component;
        (active, app.session.active_occurrence_path.clone().unwrap_or_else(|| app.session.doc.first_path(active)))
    });
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
    if design_copied_here(app) {
        app.set_status("A design can't be pasted into itself: paste it as a linked component in another design", true);
        return false;
    }
    if let Some(path) = app.linked_design_clipboard.clone() {
        let (parent_path, translate) = paste_placement(app, at);
        // The copied design is one the user opened: its server is the user's choice.
        let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(&path).into_iter().collect();
        let pasted = solidcraft_engine::io::vfs::allowing_hosts(&hosts, || {
            app.run("component.insert_design", json!({"path": path, "reuse": true, "translate":translate}))
        });
        let Ok(out) = pasted else { return false };
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
    // Features copied later replace bodies and components (copying those empties the features).
    if app.session.clipboard.is_empty() && !app.cut_bodies.is_empty() {
        return paste_cut_bodies(app, into);
    }
    if app.session.clipboard.is_empty() && !app.cut_placements.is_empty() {
        return paste_cut_placements(app, into);
    }
    if app.session.clipboard.is_empty() && !app.copied_placements.is_empty() {
        return paste_placements(app, new_copy, into);
    }
    if app.session.clipboard.is_empty() && !app.copied_bodies.is_empty() {
        let bodies = app.copied_bodies.clone();
        // Placed by that placement: a component placed more than once gets them in each.
        let (component, path) = into;
        let Ok(out) = app.run("component.copy_bodies", json!({ "bodies": bodies, "component": component, "component_path": path })) else {
            return false;
        };
        let made: Vec<&str> = out.get("bodies").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        app.set_status(format!("Pasted {}", made.join(", ")), false);
        return true;
    }
    if app.session.clipboard.is_empty() {
        app.set_status("Nothing to paste: copy bodies or timeline features first", true);
        return false;
    }
    // Into that component (that placement), each where its original is in the world.
    let (component, path) = into;
    app.run("timeline.paste", json!({ "component": component, "component_path": path })).is_ok()
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

/// Move the cut bodies into `into` (a component, placed by its placement), where they are; they
/// are selected and the clipboard empties. Bodies already in it stay as they are. A move that
/// can't be done says why and keeps the cut, to paste somewhere else.
fn paste_cut_bodies(app: &mut SolveApp, (component, path): (u64, Vec<u64>)) -> bool {
    let state = app.session.model.state();
    let moving: Vec<String> = app
        .cut_bodies
        .iter()
        .filter(|b| state.body(b).is_some_and(|body| app.session.doc.model_body_component(body) != component))
        .cloned()
        .collect();
    drop(state);
    let cut = std::mem::take(&mut app.cut_bodies);
    if moving.is_empty() {
        app.set_status("Already in that component: nothing to move", false);
        return true;
    }
    if app.run("component.move_bodies", json!({ "bodies": moving, "component": component, "component_path": path })).is_err() {
        app.cut_bodies = cut;
        return false;
    }
    let _ = app.run("select.set", json!({ "items": moving.iter().map(|b| json!({"type": "body", "name": b})).collect::<Vec<_>>() }));
    app.set_status(format!("Moved {}", moving.join(", ")), false);
    true
}

/// Move the cut components into `into` (a component, placed by its placement), where they are
/// (`occurrence.reparent`); they are selected and the clipboard empties. Ones already in it stay
/// as they are. A move that can't be done (into itself) says why and keeps the cut.
fn paste_cut_placements(app: &mut SolveApp, (component, path): (u64, Vec<u64>)) -> bool {
    let cut = std::mem::take(&mut app.cut_placements);
    let mut moved = Vec::new();
    for from in &cut {
        let Some(&occurrence) = from.last() else { continue };
        if app.session.doc.occurrences.iter().any(|o| o.id == occurrence && o.parent == component) {
            continue;
        }
        let params = json!({ "occurrence": occurrence, "path": from, "parent": component, "parent_path": path });
        let Ok(out) = app.run("occurrence.reparent", params) else {
            app.cut_placements = cut;
            return false;
        };
        let to: Vec<u64> = out.get("path").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        crate::browser::close_placement(app, &to);
        moved.push(Sel::Occurrence { path: to });
    }
    if moved.is_empty() {
        app.set_status("Already in that component: nothing to move", false);
        return true;
    }
    let _ = app.run("select.set", json!({ "items": moved }));
    app.set_status("Moved the cut components", false);
    true
}

/// New instances (or with `new_copy`, independent copies) of the copied components in `into` (a
/// component, placed by its placement), where the components are; they are selected. A component
/// never goes inside itself or what it holds: that is refused and says so.
fn paste_placements(app: &mut SolveApp, new_copy: bool, (active, base): (u64, Vec<u64>)) -> bool {
    let mut made = Vec::new();
    let mut names = Vec::new();
    for path in app.copied_placements.clone() {
        let Some(placement) = app.session.occurrence_placement(&path).ok() else { continue };
        // Placed by that placement: a component placed more than once gets it in each.
        let source = json!({ "occurrence": placement.occurrence, "path": path, "parent": active, "parent_path": base, "in_place": true });
        let out = if new_copy {
            let mut p = source;
            p["component"] = json!(placement.component);
            app.run("component.paste_new", p)
        } else {
            app.run("occurrence.copy", source)
        };
        // A paste that can't be done (into itself) has said why.
        let Ok(out) = out else { return false };
        if let Some(id) = out.get("occurrence").and_then(Value::as_u64) {
            let path: Vec<u64> = base.iter().copied().chain([id]).collect();
            // Pasted components arrive folded.
            crate::browser::close_placement(app, &path);
            made.push(Sel::Occurrence { path });
        }
        names.push(placement.name);
    }
    if made.is_empty() {
        app.set_status("Nothing to paste: the copied components are gone", true);
        return false;
    }
    let _ = app.run("select.set", json!({ "items": made }));
    let how = if new_copy { "a new copy" } else { "a new instance" };
    app.set_status(format!("Pasted {how} of {}", names.join(", ")), false);
    true
}

/// The frame's Copy, Cut and Paste events (and Ctrl+C / Ctrl+X / Ctrl+V sent as key presses), plus any
/// clipboard text a menu's Copy left. Called by the global shortcuts when no text field has the
/// keyboard.
pub fn handle(app: &mut SolveApp, ctx: &egui::Context) {
    let (copies, cuts, pastes, shift, pointer) = ctx.input(|i| {
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
                egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. } if modifiers.command && !modifiers.alt => pastes += 1,
                _ => {}
            }
        }
        // The window system's paste event carries no keys: Shift held makes it Ctrl+Shift+V.
        (copies, cuts, pastes, i.modifiers.shift, i.pointer.latest_pos())
    });
    if copies > 0 {
        copy(app);
    }
    if cuts > 0 {
        cut(app);
    }
    if pastes > 0 {
        paste_as(app, pointer, shift);
    }
    flush(app, ctx);
}

/// Write clipboard text a copy left.
pub fn flush(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(text) = app.clipboard_text.take() {
        ctx.copy_text(text);
    }
}

#[cfg(test)]
#[path = "edit_clipboard_tests.rs"]
mod tests;
