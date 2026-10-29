//! The selection mode (#4255): what a click on the model picks while no dialog or tool takes the
//! pick, and so what the pointer lights. One setting, `UiState::pick_mode`, set through
//! `select.mode`; the selection filter narrows it.

use serde_json::json;
use solidcraft_engine::{PickMode, Sel};

use crate::SolveApp;
use crate::ui_kit;
use crate::viewport::Hit;

/// The mode's name in the navigation bar's selection-mode menu (its icon is
/// `navigation_bar::selection_mode_icon`).
pub(crate) fn label(mode: PickMode) -> &'static str {
    match mode {
        PickMode::Faces => "Faces & edges",
        PickMode::Bodies => "Bodies",
        PickMode::Features => "Features",
        PickMode::Parts => "Parts",
        PickMode::Components => "Components",
        PickMode::Sketch => "Sketch geometry",
    }
}

/// The mode's id, as `select.mode` and `ui.set {pickMode}` take it.
pub(crate) fn id(mode: PickMode) -> String {
    serde_json::to_value(mode).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Switch to `mode` through `select.mode`, as scripts and the control channel do.
pub(crate) fn set(app: &mut SolveApp, mode: PickMode) {
    let _ = app.run("select.mode", json!({"mode": mode}));
}

/// A mode's check box, as "Select bodies" in the selection filter and in Preferences: ticked exactly
/// in that mode; ticking sets it, unticking sets Faces & edges, whichever mode was on.
pub(crate) fn checkbox(app: &mut SolveApp, ui: &mut egui::Ui, mode: PickMode, label: &str) {
    let mut on = app.ui.pick_mode == mode;
    if ui_kit::checkbox(ui, label, &mut on).changed() {
        set(app, if on { mode } else { PickMode::Faces });
    }
}

/// Render's pick filter (#6426) is this mode seen three ways: `face`, `body` or `component` (a
/// placement; Render's own Component picks the innermost one, the Parts mode).
pub(crate) fn render_filter(mode: PickMode) -> &'static str {
    match mode {
        PickMode::Faces | PickMode::Sketch => "face",
        PickMode::Bodies | PickMode::Features => "body",
        PickMode::Parts | PickMode::Components => "component",
    }
}

/// The mode Render's pick filter sets through `select.filter`.
pub(crate) fn from_render_filter(filter: Option<&str>) -> PickMode {
    match filter {
        Some("body") => PickMode::Bodies,
        Some("component") => PickMode::Parts,
        _ => PickMode::Faces,
    }
}

/// What the mode and the selection filter let a click pick with no dialog.
pub(crate) fn allowed(app: &SolveApp, h: &Hit) -> bool {
    let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
    let mode = app.ui.pick_mode;
    match h {
        Hit::SketchCurve { .. } => !off("sketch") && !off("sketch_curves"),
        Hit::SketchPoint { .. } | Hit::SketchMidpoint { .. } => !off("sketch") && !off("sketch_points"),
        Hit::Profile { .. } => !off("sketch"),
        Hit::Vertex { .. } => mode == PickMode::Faces && !off("vertices"),
        Hit::Edge { .. } => mode == PickMode::Faces && !off("edges"),
        // "Faces" in the filter is about faces themselves: a whole-object mode picks through them.
        Hit::Face { .. } => mode.whole() || (mode == PickMode::Faces && !off("faces")),
        Hit::Plane { .. } | Hit::Axis { .. } | Hit::ConstructionPoint { .. } => true,
    }
}

/// What a click on face `index` of `body` picks in a whole-object mode; None in the others.
pub(crate) fn whole_pick(app: &SolveApp, body: &str, index: usize) -> Option<Sel> {
    let whole_body = || Some(Sel::Body { name: body.to_string() });
    match app.ui.pick_mode {
        PickMode::Bodies => whole_body(),
        PickMode::Features => feature_of(app, body, index).map(|id| Sel::Feature { id }),
        PickMode::Parts => Some(part_of(app, body)),
        // A body of the top-level design is its own component too.
        PickMode::Components => crate::browser::body_placement(app, body)
            .and_then(|path| path.first().map(|top| Sel::Occurrence { path: vec![*top] }))
            .or_else(whole_body),
        PickMode::Faces | PickMode::Sketch => None,
    }
}

/// The innermost placement holding `body`, the part a click picks in the Parts mode and Render's
/// Ctrl-drop paints; a body of the top-level design is its own part. Needs the walk [`prepare`]
/// (or [`crate::browser::prepare_body_paths`]) runs.
pub(crate) fn part_of(app: &SolveApp, body: &str) -> Sel {
    crate::browser::body_placement(app, body).map_or_else(|| Sel::Body { name: body.to_string() }, |path| Sel::Occurrence { path })
}

/// The feature that made face `index` of `body`; the body's own where none can be traced (an
/// import's). Kept for the face under the pointer while the design holds still: the hover asks
/// every frame, and naming a body's faces walks all of them.
fn feature_of(app: &SolveApp, body: &str, index: usize) -> Option<u64> {
    let key = (body.to_string(), index, app.session.revision);
    if let Ok(kept) = app.viewport.face_feature.lock()
        && let Some((k, id)) = kept.as_ref()
        && *k == key
    {
        return *id;
    }
    let id = crate::context_menu::face_feature(app, body, index).or_else(|| app.session.world_state().body(body).map(|b| b.feature));
    if let Ok(mut kept) = app.viewport.face_feature.lock() {
        *kept = Some((key, id));
    }
    id
}

/// What the pointer lights in a whole-object mode: what a click on the hovered face would pick, as
/// the selection shows it. Nothing while a dialog or tool is open (they take the pick).
pub(crate) fn hovered(app: &SolveApp, hover: Option<&Hit>) -> Vec<Sel> {
    match hover {
        Some(Hit::Face { body, index, .. }) if lights_whole(app) => {
            whole_pick(app, body, *index).map(|sel| crate::viewport::displayed_selection(app, &sel)).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// A hovered face lights a whole object, not the face.
pub(crate) fn lights_whole(app: &SolveApp) -> bool {
    app.ui.pick_mode.whole() && app.dialog.is_none() && app.tool.is_none()
}

/// The placement paths [`whole_pick`] reads in the Parts and Components modes, built before the
/// viewport picks (once per model).
pub(crate) fn prepare(app: &mut SolveApp) {
    if matches!(app.ui.pick_mode, PickMode::Parts | PickMode::Components) {
        crate::browser::prepare_body_paths(app);
    }
}

/// A preference saved as `pick_mode`, or an older "select bodies" (`pick_bodies`) as Bodies.
pub(crate) fn from_prefs(v: &serde_json::Value) -> Option<PickMode> {
    match v.get("pick_mode") {
        Some(m) => serde_json::from_value(m.clone()).ok(),
        None => v.get("pick_bodies").and_then(serde_json::Value::as_bool).map(|b| if b { PickMode::Bodies } else { PickMode::Faces }),
    }
}

#[cfg(test)]
#[path = "selection_mode_tests.rs"]
mod tests;
