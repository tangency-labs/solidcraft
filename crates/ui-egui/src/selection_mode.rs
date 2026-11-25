//! The selection mode (#4255): what a click on the model picks while no dialog or tool takes the
//! pick, and so what the pointer lights. One setting, `UiState::pick_mode`, set through
//! `select.mode`; the selection filter narrows it.

use serde_json::json;
use solidcraft_engine::{PickMode, Sel};

use crate::SolveApp;
use crate::ui_kit;
use crate::viewport::Hit;

/// The message id of the mode's name in the selection-mode menus (its icon is in
/// `navigation_slots::SELECTION_MODE_ICONS`).
pub(crate) fn label_id(mode: PickMode) -> &'static str {
    match mode {
        PickMode::Faces => "navbar.selection.faces",
        PickMode::Bodies => "navbar.selection.bodies",
        PickMode::Features => "navbar.selection.features",
        PickMode::Parts => "navbar.selection.parts",
        PickMode::Components => "navbar.selection.components",
        PickMode::Sketch => "navbar.selection.sketch",
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

/// The selection filter's three ways of seeing a mode (#6426, #11553): `face`, `body` or
/// `component` (a placement; the filter's Component picks the innermost one, the Parts mode).
pub(crate) fn render_filter(mode: PickMode) -> &'static str {
    match mode {
        PickMode::Faces | PickMode::Sketch => "face",
        PickMode::Bodies | PickMode::Features => "body",
        PickMode::Parts | PickMode::Components => "component",
    }
}

/// The mode a `select.filter` choice stands for: Modelling's setting outside Render, Render's own
/// filter inside it.
pub(crate) fn from_render_filter(filter: Option<&str>) -> PickMode {
    match filter {
        Some("body") => PickMode::Bodies,
        Some("component") => PickMode::Parts,
        _ => PickMode::Faces,
    }
}

/// The mode a click picks by: the setting, or in Render's product view Render's own pick filter
/// (`UiState::render_pick`, #17512), which never changes the setting.
pub(crate) fn picks(app: &SolveApp) -> PickMode {
    if crate::render_workspace::active(app) { from_render_filter(Some(&app.ui.render_pick)) } else { app.ui.pick_mode }
}

/// The Solid pick filters of the Selection mode menu: (id, English label).
pub(crate) const SOLID_FILTERS: [(&str, &str); 3] = [("faces", "Faces"), ("edges", "Edges"), ("vertices", "Vertices")];

/// The Sketch pick filters: (id, message id, indented); curves and points sit under geometry.
pub(crate) const SKETCH_FILTERS: [(&str, &str, bool); 5] = [
    ("sketch", "navbar.filter.geometry", false),
    ("sketch_curves", "navbar.filter.curves", true),
    ("sketch_points", "navbar.filter.points", true),
    ("dimensions", "navbar.filter.dimensions", false),
    ("constraints", "navbar.filter.constraints", false),
];

/// How many pick filters are switched off; the Selection mode slot shows a dot while any is.
pub(crate) fn filters_off(app: &SolveApp) -> usize {
    let off = |id: &str| app.ui.pick_off.iter().any(|x| x == id);
    SOLID_FILTERS.iter().filter(|(id, _)| off(id)).count() + SKETCH_FILTERS.iter().filter(|(id, ..)| off(id)).count()
}

/// Switch one pick filter on or off.
pub(crate) fn toggle_filter(app: &mut SolveApp, id: &str) {
    let was_off = app.ui.pick_off.iter().any(|x| x == id);
    app.ui.pick_off.retain(|x| x != id);
    if !was_off {
        app.ui.pick_off.push(id.to_owned());
    }
}

/// Turn every pick filter back on and the drag shape back to a rectangle.
pub(crate) fn reset_filters(app: &mut SolveApp) {
    app.ui.pick_off.clear();
    app.ui.lasso_select = false;
}

/// What the mode and the selection filter let a click pick with no dialog.
pub(crate) fn allowed(app: &SolveApp, h: &Hit) -> bool {
    let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
    let mode = picks(app);
    match h {
        Hit::SketchCurve { .. } => !off("sketch") && !off("sketch_curves"),
        Hit::SketchPoint { .. } | Hit::PlacedSketchPoint { .. } | Hit::SketchPoint3d { .. } | Hit::SketchMidpoint { .. } => {
            !off("sketch") && !off("sketch_points")
        }
        Hit::Profile { .. } => !off("sketch"),
        Hit::Vertex { .. } => mode == PickMode::Faces && !off("vertices"),
        Hit::Edge { .. } => mode == PickMode::Faces && !off("edges"),
        // "Faces" in the filter is about faces themselves: a whole-object mode picks through them.
        Hit::Face { .. } => mode.whole() || (mode == PickMode::Faces && !off("faces")),
        // Sketch geometry is a sketch's curves, points and profiles only.
        Hit::Plane { .. } | Hit::Axis { .. } | Hit::ConstructionPoint { .. } => mode != PickMode::Sketch,
        Hit::PointCloudPoint { .. } => mode == PickMode::Faces && !off("vertices"),
        Hit::MemberEnd { .. } | Hit::Centreline { .. } => false,
    }
}

/// What a click on face `index` of `body` picks in a whole-object mode; None in the others.
pub(crate) fn whole_pick(app: &SolveApp, body: &str, index: usize) -> Option<Sel> {
    let whole_body = || Some(Sel::Body { name: body.to_string() });
    match picks(app) {
        PickMode::Bodies => whole_body(),
        PickMode::Features => feature_of(app, body, index).map(|id| Sel::Feature { id }),
        PickMode::Parts => Some(part_of(app, body)),
        // A body of the top-level design is its own component too.
        PickMode::Components => crate::browser::placed_body_path(app, body)
            .and_then(|path| path.first().map(|top| Sel::Occurrence { path: vec![*top] }))
            .or_else(whole_body),
        PickMode::Faces | PickMode::Sketch => None,
    }
}

/// The innermost placement holding `body`, the part a click picks in the Parts mode and Render's
/// Ctrl-drop paints; a body of the top-level design is its own part. Needs the walk [`prepare`]
/// (or [`crate::browser::prepare_body_paths`]) runs.
pub(crate) fn part_of(app: &SolveApp, body: &str) -> Sel {
    crate::browser::placed_body_path(app, body).map_or_else(|| Sel::Body { name: body.to_string() }, |path| Sel::Occurrence { path })
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
    let id = crate::open_meshing::session_world(&app.session)
        .body(body)
        .and_then(|b| crate::context_menu::face_feature(app, b, index))
        .or_else(|| crate::open_meshing::session_world(&app.session).body(body).map(|b| b.feature));
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
    picks(app).whole() && app.dialog.is_none() && app.tool.is_none()
}

/// The placement paths [`whole_pick`] reads in the Parts and Components modes, built before the
/// viewport picks (once per model).
pub(crate) fn prepare(app: &mut SolveApp) {
    if matches!(picks(app), PickMode::Parts | PickMode::Components) {
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
