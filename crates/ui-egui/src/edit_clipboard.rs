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
//! whichever was copied last. Cut on components, bodies or timeline features only marks them (a
//! [`CutSet`]): Paste moves them into the component in one step (`browser.reparent`, where
//! they are), and a copy or Esc before that leaves them be. A component placed more than once (an
//! instance) isn't cut.
//!
//! Copied bodies paste into another design (tab) too, as imported bodies without their history,
//! and copied components as independent copies with their history (Paste and Paste New alike),
//! where they were in the world (`component.paste_from_design`); completed sketches paste as
//! independent features with their parameters (`timeline.paste`); cut ones paste there as copies,
//! the originals staying. Sketch geometry copied or cut in a sketch pastes into a sketch of
//! another design (`sketch.paste`), at the cursor or at the copied coordinates, with the
//! constraints and dimensions among it; a dimension that named the copied design's parameters
//! keeps its value instead (#9258). The last copy or cut in any design is what pastes: a design's own
//! clipboard empties when another design copied after it.
//!
//! The window system turns Ctrl+C / Ctrl+V into clipboard events, not key presses, and only sends
//! the paste event while the system clipboard holds text. So a copy also puts a short line of
//! text on the system clipboard (a menu's Copy leaves it for the next frame, which has the
//! context to write it).

use egui::Pos2;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;

/// What Cut marked: components (by placement path), timeline features (by id, sketches included)
/// and bodies (by name). They stay where they are, shown faded, until Paste moves them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutSet {
    pub placements: Vec<Vec<u64>>,
    pub features: Vec<u64>,
    pub bodies: Vec<String>,
}

impl CutSet {
    pub fn is_empty(&self) -> bool {
        self.placements.is_empty() && self.features.is_empty() && self.bodies.is_empty()
    }
}

/// Whether Cut has marked the body: named itself, or moving with a feature that is marked.
pub fn body_is_cut(app: &SolveApp, name: &str) -> bool {
    app.cut.bodies.iter().any(|b| b == name)
}

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

/// Does Copy copy the whole design (its row picked in the browser)? Features, bodies or
/// components selected since then are what was meant, so the design gives way to them.
fn copying_design(app: &SolveApp) -> bool {
    app.session.active_sketch.is_none()
        && app.tree.picked_components == [0]
        && crate::timeline::selected_feature_ids(app).is_empty()
        && body_selection(app).is_empty()
        && placement_selection(app).is_empty()
}

/// Is there something Copy would copy?
pub fn something_to_copy(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        !sketch_selection(app).is_empty()
    } else {
        copying_design(app)
            || !crate::timeline::selected_feature_ids(app).is_empty()
            || !body_selection(app).is_empty()
            || !placement_selection(app).is_empty()
    }
}

/// What another design copied or cut last, when that is what pastes here (this design's own
/// clipboard is older).
fn foreign(app: &SolveApp) -> Option<&std::sync::Arc<solidcraft_engine::ForeignCopy>> {
    app.foreign_copy.as_ref().filter(|_| app.own_clipboard_stamp != app.clipboard_stamp)
}

/// Sketch geometry another design copied or cut last, when that is what pastes here.
fn foreign_sketch(app: &SolveApp) -> Option<&solidcraft_engine::cmd::sketch_clipboard::SketchClip> {
    app.foreign_sketch_clip.as_ref().filter(|_| app.own_clipboard_stamp != app.clipboard_stamp)
}

/// Is there sketch geometry to paste into the active sketch: this design's, or another's copied
/// last?
pub fn can_paste_sketch(app: &SolveApp) -> bool {
    app.session.sketch_clipboard.is_some() || foreign_sketch(app).is_some()
}

/// Bodies another design copied or cut last: they paste here as imported bodies.
fn foreign_bodies(app: &SolveApp) -> bool {
    foreign(app).is_some_and(|c| !c.bodies.is_empty())
}

/// This design's clipboard, when another design has copied or cut since (switching to this
/// design): it empties, so the last copy is what pastes.
pub(crate) fn drop_stale(app: &mut SolveApp) {
    if app.own_clipboard_stamp != app.clipboard_stamp {
        app.session.clipboard.clear();
        app.session.sketch_clipboard = None;
        app.copied_bodies.clear();
        app.copied_placements.clear();
        app.cut = CutSet::default();
    }
}

/// Is there something Paste would paste here?
pub fn can_paste(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        can_paste_sketch(app)
    } else if app.linked_design_clipboard.is_some() {
        // A copied design is what pastes, and never into itself.
        !design_copied_here(app)
    } else {
        foreign(app).is_some()
            || !app.session.clipboard.is_empty()
            || !app.cut.is_empty()
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
        && (!app.copied_placements.is_empty() || foreign(app).is_some_and(|c| !c.placed.is_empty()))
}

/// Whether copied features (sketches, …) are what Paste pastes now (a Sketches folder's menu
/// offers them).
pub fn pasting_features(app: &SolveApp) -> bool {
    app.session.active_sketch.is_none()
        && app.linked_design_clipboard.is_none()
        && (!app.session.clipboard.is_empty() || foreign(app).is_some_and(|c| !c.features.is_empty()))
}

/// The same engine paste serves the keyboard and the Sketches folder/group menus.
fn paste_features(app: &mut SolveApp, component: u64, path: Vec<u64>) -> Result<Value, String> {
    let copy = foreign(app).filter(|c| !c.features.is_empty()).cloned();
    let from_design = copy.is_some();
    if let Some(copy) = copy {
        app.session.foreign_copy = Some(copy);
    }
    app.run("timeline.paste", json!({ "component": component, "component_path": path, "from_design": from_design }))
}

/// Whether copied or cut bodies are what Paste pastes now (a Bodies folder's menu offers them).
pub fn pasting_bodies(app: &SolveApp) -> bool {
    app.session.active_sketch.is_none()
        && app.session.clipboard.is_empty()
        && app.linked_design_clipboard.is_none()
        && (!app.copied_bodies.is_empty() || !app.cut.bodies.is_empty() || foreign_bodies(app))
}

/// [`paste_into`], then what was pasted into `group` of the component's `folder`: a group's menu
/// pastes into the group (the pasted bodies, which a body paste leaves selected, or the pasted
/// sketches).
pub fn paste_into_group(app: &mut SolveApp, at: Option<Pos2>, into: Option<(u64, Vec<u64>)>, group: Option<u64>, folder: &str) -> bool {
    let (Some(group), Some((component, path))) = (group, into.clone()) else { return paste_into(app, at, false, into) };
    if folder == "sketches" && pasting_features(app) {
        let Ok(out) = paste_features(app, component, path) else { return false };
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
    } else if app.session.clipboard.is_empty() && app.linked_design_clipboard.is_none() && !app.cut.placements.is_empty() {
        &app.cut.placements
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

/// A new copy or cut replaces the copied bodies, components, the cut and design (copied
/// features and sketch geometry are replaced by the commands that copy them), in this design and
/// for the others: it is the last one.
fn replace_clipboard(app: &mut SolveApp) {
    app.clipboard_stamp += 1;
    app.own_clipboard_stamp = app.clipboard_stamp;
    app.foreign_copy = None;
    app.foreign_sketch_clip = None;
    app.copied_bodies.clear();
    app.copied_placements.clear();
    app.cut = CutSet::default();
    app.linked_design_clipboard = None;
}

/// Leave what was cut where it is (Esc, or another copy): another design has nothing of it to paste
/// either.
pub fn drop_cut(app: &mut SolveApp) {
    if !app.cut.is_empty() {
        app.foreign_copy = None;
    }
    app.cut = CutSet::default();
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
    if copying_design(app) {
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
        app.foreign_sketch_clip = app.session.sketch_clipboard.clone();
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
        app.foreign_copy = solidcraft_engine::ForeignCopy::of(&app.session, Vec::new(), placements.clone()).ok().map(std::sync::Arc::new);
        app.copied_placements = placements;
        return true;
    }
    // Bodies picked (in the browser or the view) with no features: copies of them paste.
    if features.is_empty() && !bodies.is_empty() {
        app.session.clipboard.clear();
        replace_clipboard(app);
        app.set_status(format!("Copied {}", bodies.join(", ")), false);
        leave_text(app, format!("SolidCraft bodies: {}", bodies.join(", ")));
        app.foreign_copy = solidcraft_engine::ForeignCopy::of(&app.session, bodies.clone(), Vec::new()).ok().map(std::sync::Arc::new);
        app.copied_bodies = bodies;
        return true;
    }
    if features.is_empty() {
        return copy_failed(app, Some("Select sketch geometry, bodies or timeline features to copy"));
    }
    let Ok(out) = app.run("timeline.copy", json!({ "features": features })) else { return copy_failed(app, None) };
    replace_clipboard(app);
    app.foreign_copy = app.session.foreign_copy.clone();
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

/// What Cut marks for the selection, or why it can't. A selected component takes what it holds
/// with it, and a selected feature takes the body it is the origin of, so those are left out of the
/// set (and so are not marked, nor moved on their own): components, features and bodies chosen
/// together are one move.
fn cut_set(app: &SolveApp) -> Result<CutSet, String> {
    let mut set = CutSet { placements: placement_selection(app), features: crate::timeline::selected_feature_ids(app), bodies: body_selection(app) };
    if set.is_empty() {
        return Err("Select components, bodies, sketches or timeline features to cut".into());
    }
    if !set.placements.is_empty()
        && let Some(why) = placement_cut_refused(app, &set.placements)
    {
        return Err(why);
    }
    if !set.bodies.is_empty()
        && let Some(why) = body_cut_refused(app, &set.bodies)
    {
        return Err(why);
    }
    let doc = &app.session.doc;
    // A placement inside another selected one moves with it.
    let all = set.placements.clone();
    set.placements.retain(|p| !all.iter().any(|q| q.len() < p.len() && p.starts_with(q)));
    let held: Vec<u64> = set.placements.iter().filter_map(|p| app.session.occurrence_placement(p).ok()).map(|p| p.component).collect();
    let inside = |component: u64| held.iter().any(|c| doc.inside_component(component, *c));
    set.features.retain(|id| doc.feature(*id).is_some_and(|f| !inside(f.component)));
    let features = set.features.clone();
    // A body belongs to the feature it depends on first, and moves with it.
    let origins = app.session.body_origins();
    let state = app.session.model.state();
    set.bodies.retain(|b| {
        state.body(b).is_some_and(|body| {
            let origin = origins.get(&body.name).copied().unwrap_or(body.feature);
            !inside(doc.model_body_component(body)) && !features.contains(&origin)
        })
    });
    Ok(set)
}

/// What Cut says it marked, and how to leave it: "Cut 1 component, 3 features, 1 body (Esc to Cancel)".
fn cut_summary(set: &CutSet) -> String {
    let count = |n: usize, one: &str, many: &str| (n > 0).then(|| format!("{n} {}", if n == 1 { one } else { many }));
    let parts: Vec<String> = [
        count(set.placements.len(), "component", "components"),
        count(set.features.len(), "feature", "features"),
        count(set.bodies.len(), "body", "bodies"),
    ]
    .into_iter()
    .flatten()
    .collect();
    format!("Cut {} (Esc to Cancel)", parts.join(", "))
}

/// The names of what `set` marks, for the system clipboard text.
fn cut_names(app: &SolveApp, set: &CutSet) -> Vec<String> {
    let placements = set.placements.iter().filter_map(|p| app.session.occurrence_placement(p).ok()).map(|p| p.name);
    let features = set.features.iter().filter_map(|id| app.session.doc.feature(*id)).map(|f| f.name.clone());
    placements.chain(features).chain(set.bodies.iter().cloned()).collect()
}

/// Is there something Cut would cut: sketch geometry in a sketch, otherwise components, bodies or
/// timeline features that may move.
pub fn can_cut(app: &SolveApp) -> bool {
    if app.session.active_sketch.is_some() {
        return !sketch_selection(app).is_empty();
    }
    cut_set(app).is_ok()
}

/// Cut: in a sketch, the selected sketch geometry (copied, then deleted: one undo step); otherwise
/// the selected components, bodies and timeline features, which stay until Paste moves them.
pub fn cut(app: &mut SolveApp) -> bool {
    if app.session.active_sketch.is_none() {
        let set = match cut_set(app) {
            Ok(set) => set,
            Err(why) => return copy_failed(app, Some(&why)),
        };
        clear_clipboard(app);
        let names = cut_names(app, &set);
        app.set_status(cut_summary(&set), false);
        leave_text(app, format!("SolidCraft cut: {}", names.join(", ")));
        // Another design pastes copies of the bodies and components cut, but features never leave their
        // design: a cut that holds any feature leaves another design nothing to paste.
        if set.features.is_empty() {
            app.foreign_copy =
                solidcraft_engine::ForeignCopy::of(&app.session, set.bodies.clone(), set.placements.clone()).ok().map(std::sync::Arc::new);
        }
        app.cut = set;
        // What is cut is the faded set; the selection that chose it would only muddy which that is.
        let _ = app.run("select.clear", json!({}));
        return true;
    }
    let entities = sketch_selection(app);
    if entities.is_empty() {
        return copy_failed(app, Some("Select sketch geometry to cut"));
    }
    let Ok(out) = app.run("sketch.cut", json!({ "entities": entities })) else { return copy_failed(app, None) };
    replace_clipboard(app);
    app.foreign_sketch_clip = app.session.sketch_clipboard.clone();
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
        // Another design copied sketch geometry last: it pastes here at the same sketch
        // coordinates, its dimensions keeping their values rather than that design's parameters.
        if let Some(clip) = foreign_sketch(app).map(|c| c.for_another_design()) {
            app.session.sketch_clipboard = Some(clip);
        }
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
    if let Some(copy) = foreign(app).cloned() {
        return paste_from_design(app, copy, into);
    }
    // Features copied later replace bodies and components (copying those empties the features).
    if app.session.clipboard.is_empty() && !app.cut.is_empty() {
        return paste_cut(app, into);
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

/// The bodies another design copied or cut, into `into` (a component, placed by its placement)
/// as imported bodies, or its components as independent copies, where they were in the world;
/// they are selected.
fn paste_from_design(app: &mut SolveApp, copy: std::sync::Arc<solidcraft_engine::ForeignCopy>, (component, path): (u64, Vec<u64>)) -> bool {
    if !copy.features.is_empty() {
        return paste_features(app, component, path).is_ok();
    }
    let design = copy.design.clone();
    app.session.foreign_copy = Some(copy);
    let Ok(out) = app.run("component.paste_from_design", json!({ "component": component, "component_path": path })) else { return false };
    if let Some(paths) = out.get("paths").and_then(Value::as_array) {
        // Pasted components arrive folded.
        for path in paths {
            let path: Vec<u64> = path.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            crate::browser::close_placement(app, &path);
        }
        app.set_status(format!("Pasted independent copies of the components copied in {design}"), false);
        return true;
    }
    let made: Vec<&str> = out.get("bodies").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    app.set_status(format!("Pasted {} as imported bodies (without their history)", made.join(", ")), false);
    true
}

/// Move what was cut into `into` (a component, placed by its placement), where it is, in one
/// step (`browser.reparent`); what moved is selected and the cut is spent. What is gone, or
/// already there, is skipped. A move that can't be done says why and keeps the cut, to paste
/// somewhere else.
fn paste_cut(app: &mut SolveApp, (component, path): (u64, Vec<u64>)) -> bool {
    let cut = std::mem::take(&mut app.cut);
    let foreign = app.foreign_copy.take();
    let (placements, features, bodies) = {
        let doc = &app.session.doc;
        let state = app.session.model.state();
        let placements: Vec<Vec<u64>> = cut
            .placements
            .iter()
            .filter(|p| {
                app.session.occurrence_placement(p).is_ok()
                    && p.last().is_some_and(|id| doc.occurrences.iter().any(|o| o.id == *id && o.parent != component))
            })
            .cloned()
            .collect();
        let features: Vec<u64> = cut.features.iter().copied().filter(|id| doc.feature(*id).is_some_and(|f| f.component != component)).collect();
        let bodies: Vec<String> =
            cut.bodies.iter().filter(|b| state.body(b).is_some_and(|body| doc.model_body_component(body) != component)).cloned().collect();
        (placements, features, bodies)
    };
    if placements.is_empty() && features.is_empty() && bodies.is_empty() {
        app.set_status("Already in that component: nothing to move", false);
        return true;
    }
    let params = json!({ "occurrences": placements, "features": features, "bodies": bodies, "component": component, "component_path": path });
    let Ok(out) = app.run("browser.reparent", params) else {
        app.cut = cut;
        app.foreign_copy = foreign;
        return false;
    };
    let ids = |key: &str| -> Vec<Value> { out.get(key).and_then(Value::as_array).cloned().unwrap_or_default() };
    let mut moved: Vec<Sel> = Vec::new();
    for id in ids("occurrences").iter().filter_map(Value::as_u64) {
        let to: Vec<u64> = path.iter().copied().chain([id]).collect();
        // Moved components arrive folded.
        crate::browser::close_placement(app, &to);
        moved.push(Sel::Occurrence { path: to });
    }
    for name in ids("features").iter().filter_map(Value::as_str) {
        if let Some(f) = app.session.doc.find_feature(name) {
            moved.push(Sel::Feature { id: f.id });
        }
    }
    for name in ids("bodies").iter().filter_map(Value::as_str) {
        moved.push(Sel::Body { name: name.to_string() });
    }
    let count = moved.len();
    let _ = app.run("select.set", json!({ "items": moved }));
    app.set_status(format!("Moved {count} {}", if count == 1 { "item" } else { "items" }), false);
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
