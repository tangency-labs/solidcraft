//! The timeline (bottom): features in order, the rollback marker, errors and warnings.

use crate::ui_kit::{Container, Edge, Status, Surface};
use egui::{Rect, Sense, UiBuilder, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::doc::{
    FeatureKind, PlaneRef, SketchId,
    construct::{AxisDef, PointDef},
};

use solidcraft_engine::schema::edit::PlaybackAction;

use crate::SolveApp;
use crate::theme::Tokens;

/// Apply transport actions from the current marker; endpoints stay bounded, including an
/// empty design. All orientations reach the same rollback command.
pub(crate) fn run_transport(app: &mut SolveApp, control: crate::ui_kit::HistoryTransport) {
    let n = app.session.doc.features.len();
    let marker = app.session.doc.marker.unwrap_or(n);
    let position = match control {
        crate::ui_kit::HistoryTransport::First => 0,
        crate::ui_kit::HistoryTransport::Back => marker.saturating_sub(1),
        crate::ui_kit::HistoryTransport::Play => {
            app.start("timeline.play");
            return;
        }
        crate::ui_kit::HistoryTransport::Forward => marker.saturating_add(1).min(n),
        crate::ui_kit::HistoryTransport::Last => n,
    };
    let params = if position < n { json!({"position":position}) } else { json!({}) };
    let _ = app.run_heavy("timeline.roll_to", params);
}

/// A click runs a History control; a right-click offers to show or hide the Play button.
pub(crate) fn transport_response(app: &mut SolveApp, control: crate::ui_kit::HistoryTransport, response: &egui::Response) {
    if response.clicked() {
        run_transport(app, control);
    }
    if response.secondary_clicked() {
        let at = response.interact_pointer_pos().unwrap_or(response.rect.left_bottom());
        crate::context_menu::open_for(app, at, crate::context_menu::Target::HistoryControls);
    }
}

/// The History controls' menu: shows or hides the Play button.
pub(crate) fn transport_items(app: &SolveApp) -> Vec<crate::context_menu::Item> {
    let show = !app.preferences.history_play_button;
    let label = crate::i18n::tr(if show { "ui.history_play_button_show" } else { "ui.history_play_button_hide" });
    vec![crate::context_menu::Item::action("ui.historyPlayButton", label, "timeline_play").with(json!({ "show": show }))]
}

/// The feature that made a face, edge or vertex of `body` (its world name): a face's own creator,
/// as Edit Feature on a face finds it (the hole, not the plate it was cut into), else the feature
/// that made the body.
fn geometry_feature(app: &SolveApp, body: &str, face: Option<usize>) -> Option<u64> {
    face.and_then(|index| crate::context_menu::face_feature(app, body, index))
        .or_else(|| app.session.world_state().body(body).map(|b| b.feature))
        .filter(|id| app.session.doc.feature(*id).is_some())
}

/// The feature of the face, edge or vertex under the pointer in the view.
pub(crate) fn hovered_feature(app: &SolveApp) -> Option<u64> {
    match &app.viewport.hover {
        Some(crate::viewport::Hit::Face { body, index, .. }) => geometry_feature(app, body, Some(*index)),
        Some(crate::viewport::Hit::Edge { body, .. } | crate::viewport::Hit::Vertex { body, .. }) => geometry_feature(app, body, None),
        _ => None,
    }
}

/// The feature of the first face or edge picked in the view.
pub(crate) fn picked_feature(app: &SolveApp) -> Option<u64> {
    app.session.selection.iter().find_map(|selection| match selection {
        solidcraft_engine::Sel::Face { body, index, .. } => geometry_feature(app, body, Some(*index)),
        solidcraft_engine::Sel::Edge { body, .. } => geometry_feature(app, body, None),
        _ => None,
    })
}

/// A new face or edge picked in the view scrolls the history shown to the feature that made it,
/// once per pick; the history stays where it is while the pick does not change.
pub(crate) fn follow_view_pick(app: &mut SolveApp) {
    let picked = picked_feature(app);
    if picked == app.tree.view_pick {
        return;
    }
    app.tree.view_pick = picked;
    if picked.is_some() && app.ui.show_timeline {
        app.tree.timeline_reveal = picked;
    }
}

// Snapshot only the data used by timeline interactions. Cloning FeatureKind here would
// also copy imported mesh vertices and triangles on every frame.
#[derive(Clone, Copy)]
pub(crate) enum TimelineKind {
    Sketch,
    Profile(SketchId),
    Other,
}

impl TimelineKind {
    pub(crate) fn of(kind: &FeatureKind) -> Self {
        match kind {
            FeatureKind::Sketch { .. } => Self::Sketch,
            FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } | FeatureKind::Sweep { sketch, .. } => Self::Profile(*sketch),
            _ => Self::Other,
        }
    }

    pub(crate) fn profile_sketch(self) -> Option<SketchId> {
        match self {
            Self::Profile(sketch) => Some(sketch),
            _ => None,
        }
    }
}

pub(crate) fn icon_of(k: &FeatureKind) -> &'static str {
    match k {
        FeatureKind::Sketch { .. } => "sketch",
        FeatureKind::Extrude { .. } => "extrude",
        FeatureKind::SurfaceExtrude { .. } => "surface_extrude",
        FeatureKind::Unstitch { .. } => "surface_unstitch",
        FeatureKind::ReverseNormal { .. } => "surface_reverse_normal",
        FeatureKind::SurfaceRevolve { .. } => "surface_revolve",
        FeatureKind::SurfaceSweep { .. } => "surface_sweep",
        FeatureKind::SurfaceLoft { .. } => "surface_loft",
        FeatureKind::SurfaceOffset { .. } => "surface_offset",
        FeatureKind::SurfaceRuled { .. } => "surface_ruled",
        FeatureKind::SurfaceUntrim { .. } => "surface_untrim",
        FeatureKind::SurfaceBlend { .. } => "surface_blend",
        FeatureKind::SurfaceBoundary { .. } | FeatureKind::SurfaceNetwork { .. } => "surface_boundary",
        FeatureKind::SurfaceMatch { .. } => "continuity",
        FeatureKind::SurfaceFair { .. } => "surface_fair",
        FeatureKind::Revolve { .. } => "revolve",
        FeatureKind::Fillet { .. } => "fillet",
        FeatureKind::Chamfer { .. } => "chamfer",
        FeatureKind::Box { .. } => "box",
        FeatureKind::Cylinder { .. } => "cylinder",
        FeatureKind::Sphere { .. } => "sphere",
        FeatureKind::Torus { .. } => "torus",
        FeatureKind::Cone { .. } => "cone",
        FeatureKind::Combine { .. } => "combine",
        FeatureKind::Pattern { pattern: solidcraft_engine::doc::PatternKind::Rectangular { .. }, .. } => "pattern_rect",
        FeatureKind::Pattern { pattern: solidcraft_engine::doc::PatternKind::Path { .. }, .. } => "pattern_path",
        FeatureKind::Pattern { .. } => "pattern_circ",
        FeatureKind::Mirror { .. } => "mirror",
        FeatureKind::ConstructionPlane { plane } => construction_plane_icon(plane),
        FeatureKind::ConstructionAxis { def } => match def {
            AxisDef::ThroughFace { .. } => "axis_through_cylinder",
            AxisDef::NormalToFace { .. } => "axis_normal_to_face",
            AxisDef::TwoPlanes { .. } => "axis_two_planes",
            AxisDef::TwoPoints { .. } => "axis_two_points",
            AxisDef::Edge { .. } => "axis_edge",
        },
        FeatureKind::ConstructionPoint { def } => match def {
            PointDef::At { .. } => "point_vertex",
            PointDef::TwoEdges { .. } => "point_two_edges",
            PointDef::ThreePlanes { .. } => "point_three_planes",
            PointDef::Center { .. } => "point_center",
            PointDef::EdgeAndPlane { .. } => "point_edge_plane",
            PointDef::AlongPath { .. } => "point_along_path",
        },
        FeatureKind::Hole { .. } => "hole",
        FeatureKind::Loft { .. } => "loft",
        FeatureKind::Sweep { .. } => "sweep",
        FeatureKind::Shell { .. } => "shell",
        FeatureKind::Draft { .. } => "draft",
        FeatureKind::Split { .. } => "split",
        FeatureKind::Move { .. } => "move",
        FeatureKind::Import { .. } | FeatureKind::MeshImport { .. } | FeatureKind::MeshConvert { .. } => "import",
        FeatureKind::BodyTransfer { .. } => "move",
        FeatureKind::Thread { .. } => "thread",
        FeatureKind::SplitFace { .. } => "split_face",
        FeatureKind::Scale { .. } => "scale",
        FeatureKind::OffsetFace { .. } => "offset_face",
        FeatureKind::DeleteFace { .. } => "delete_face",
        FeatureKind::Remove { .. } => "delete",
        FeatureKind::BoundingSolid { .. } => "bounding_solid",
        FeatureKind::Pipe { .. } => "pipe",
        FeatureKind::FrameMember { .. } => "frame_member",
        FeatureKind::Emboss { .. } => "emboss",
        FeatureKind::ReplaceFace { .. } => "replace_face",
        FeatureKind::Align { .. } => "align",
        FeatureKind::Coil { .. } => "coil",
        FeatureKind::SheetPerforate { .. } => "perforate",
        FeatureKind::BoundaryFill { .. } => "boundary_fill",
        FeatureKind::Patch { .. } => "surface_patch",
        FeatureKind::Stitch { .. } => "surface_stitch",
        FeatureKind::Thicken { .. } => "surface_thicken",
        FeatureKind::SurfaceExtend { .. } => "surface_extend",
        FeatureKind::SurfaceTrim { .. } => "surface_trim",
        other => crate::workspace::feature_icon(other).unwrap_or("feature"),
    }
}

fn construction_plane_icon(plane: &PlaneRef) -> &'static str {
    match plane {
        PlaneRef::AtAngle { .. } => "plane_angle",
        PlaneRef::Midplane { .. } => "plane_midplane",
        PlaneRef::TwoEdges { .. } => "plane_two_edges",
        PlaneRef::ThreePoints { .. } => "plane_three_points",
        PlaneRef::Tangent { .. } => "plane_tangent",
        PlaneRef::AlongPath { .. } => "plane_along_path",
        PlaneRef::Perpendicular { .. } => "plane_perpendicular",
        _ => "plane",
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Marker,
    Feature(u64),
}

/// The component whose features the bottom timeline shows (#2880): the active component, else
/// the one component picked in the browser. `None`: the whole design (nothing active or picked,
/// or Show All turned on). A design with many parts lists every part's features otherwise.
pub(crate) fn timeline_component(app: &SolveApp, show_all: bool) -> Option<u64> {
    if show_all {
        return None;
    }
    if app.session.active_component != 0 {
        return Some(app.session.active_component);
    }
    match app.tree.picked_components.as_slice() {
        [one] if *one != 0 => Some(*one),
        _ => None,
    }
}

/// Indices into the design's features that the timeline shows for `component` (all of them for
/// `None`): the features of that component and of the components inside it.
pub(crate) fn shown_feature_indices(doc: &solidcraft_engine::doc::Document, component: Option<u64>) -> Vec<usize> {
    doc.features.iter().enumerate().filter(|(_, f)| component.is_none_or(|c| doc.component_within(f.component, c))).map(|(i, _)| i).collect()
}

/// The design's feature position for a slot among the shown features: before the shown feature
/// in that slot, or just after the last shown one (the end of the design when all are shown).
fn design_position(shown: &[usize], slot: usize, n: usize) -> usize {
    match shown.get(slot) {
        Some(i) => *i,
        None => shown.last().map_or(n, |i| (i + 1).min(n)),
    }
}

/// Slot index (0..=n) nearest to screen x, given the left edges of the feature buttons.
fn slot_at(xs: &[f32], end: f32, x: f32) -> usize {
    let mut best = (0usize, f32::INFINITY);
    for (i, sx) in xs.iter().chain(std::iter::once(&end)).enumerate() {
        let d = (sx - 2.0 - x).abs();
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// Check what deleting `ids` takes with it or makes fail (off the window thread when that takes a
/// while, #2678), then delete them or ask first.
pub(crate) fn delete_features(app: &mut SolveApp, ids: Vec<u64>) {
    let checks = ids.iter().map(|id| ("timeline.dependents".to_string(), json!({ "feature": id }))).collect();
    app.check_delete(checks, crate::deferred::AfterCheck::Timeline { features: ids, groups: Vec::new() });
}

/// Delete timeline folders with their features (`contents`, nested folders' included) after the
/// same check as [`delete_features`] (#7219).
pub(crate) fn delete_folders(app: &mut SolveApp, groups: Vec<u64>, contents: Vec<u64>) {
    let checks = contents.iter().map(|id| ("timeline.dependents".to_string(), json!({ "feature": id }))).collect();
    app.check_delete(checks, crate::deferred::AfterCheck::Timeline { features: contents, groups });
}

/// [`delete_features`] once each feature's `timeline.dependents` report is in; `groups` are
/// timeline folders deleted with them.
pub(crate) fn delete_checked_features(app: &mut SolveApp, ids: Vec<u64>, groups: Vec<u64>, reports: &[Value]) {
    // Each report is about one feature alone, so it names the others being deleted too: those
    // go anyway and are left out of the confirmation.
    let deleting: Vec<&str> = ids.iter().filter_map(|id| app.session.doc.feature(*id)).map(|f| f.name.as_str()).collect();
    let mut with = std::collections::BTreeSet::new();
    let mut fail = std::collections::BTreeSet::new();
    for deps in reports {
        for (key, output) in [("deleted_with_it", &mut with), ("would_fail", &mut fail)] {
            output.extend(
                deps.get(key)
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str())
                    .filter(|name| !deleting.contains(name))
                    .map(String::from),
            );
        }
    }
    if with.is_empty() && fail.is_empty() {
        let _ = app.run_heavy("timeline.delete", delete_params(&ids, &groups));
    } else {
        app.dialog = Some(crate::dialogs::Dialog::confirm_delete_many(ids, groups, with.into_iter().collect(), fail.into_iter().collect()));
    }
}

/// `timeline.delete`'s parameters for these features and timeline folders.
pub(crate) fn delete_params(features: &[u64], groups: &[u64]) -> Value {
    if groups.is_empty() { json!({ "features": features }) } else { json!({ "features": features, "groups": groups }) }
}

/// The selected timeline features, in timeline order.
pub(crate) fn selected_feature_ids(app: &SolveApp) -> Vec<u64> {
    app.session
        .doc
        .features
        .iter()
        .filter(|feature| {
            app.session.selection.iter().any(|selection| matches!(selection, solidcraft_engine::Sel::Feature { id } if *id == feature.id))
        })
        .map(|feature| feature.id)
        .collect()
}

/// What Copy and Delete in a feature's menu act on: the selected features when the
/// right-clicked one is among them, else that feature alone.
pub(crate) fn menu_targets(app: &SolveApp, id: u64) -> Vec<u64> {
    let selected = selected_feature_ids(app);
    if selected.contains(&id) { selected } else { vec![id] }
}

/// Delete in a feature's menu, in both timelines: the selected features when the right-clicked
/// one is among them, labelled with how many, else that feature alone (#5005).
pub(crate) fn delete_button(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) {
    let targets = menu_targets(app, id);
    let label = match targets.len() {
        1 => crate::i18n::tr_label("Delete").to_string(),
        n => crate::i18n::format("timeline.delete_many", &[("count", &n.to_string())]),
    };
    if crate::context_menu::button(ui, &label, "delete").clicked() {
        delete_features(app, targets);
        ui.close();
    }
}

/// The features from design position `from` to the end, only the `shown` positions (ascending)
/// when the timeline shows some of the design's features.
fn features_from(app: &SolveApp, from: usize, shown: Option<&[usize]>) -> Vec<u64> {
    let features = app.session.doc.features.iter().enumerate().skip(from);
    features.filter(|(i, _)| shown.is_none_or(|shown| shown.binary_search(i).is_ok())).map(|(_, f)| f.id).collect()
}

/// In the menu of a feature after the history marker (feature `id` at design position
/// `index`, `marker` a design position too): delete every feature after the marker, only the
/// `shown` ones when the timeline is filtered to a component, labelled with how many, in one
/// delete through [`delete_features`] so the confirmation still lists what would fail. Not
/// offered before the marker, nor when the menu acts on several selected features: an entry
/// that acts partly on the selection and partly beyond it is hard to read.
pub(crate) fn delete_after_marker_button(app: &mut SolveApp, ui: &mut egui::Ui, id: u64, index: usize, marker: usize, shown: Option<&[usize]>) {
    if index < marker || menu_targets(app, id).len() > 1 {
        return;
    }
    let ids = features_from(app, marker, shown);
    let key = if ids.len() == 1 { "timeline.delete_after_marker_one" } else { "timeline.delete_after_marker_other" };
    let label = crate::i18n::format(key, &[("count", &ids.len().to_string())]);
    if crate::context_menu::button(ui, &label, "delete").clicked() {
        delete_features(app, ids);
        ui.close();
    }
}

/// The timeline anchor of a feature: the last one clicked, where a Shift+click range starts.
pub(crate) fn anchor_key(id: u64) -> String {
    format!("f:{id}")
}

/// Copy and Paste in a feature's timeline menu. Paste puts the copies at the history marker.
pub(crate) fn clipboard_buttons(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) {
    if crate::context_menu::button(ui, crate::i18n::tr_label("Copy"), "copy").clicked() {
        let features = menu_targets(app, id);
        let _ = app.run("timeline.copy", json!({ "features": features }));
        ui.close();
    }
    if crate::context_menu::button_enabled(ui, crate::i18n::tr_label("Paste"), "paste", !app.session.clipboard.is_empty()).clicked() {
        let _ = app.run("timeline.paste", json!({}));
        ui.close();
    }
}

/// Where each feature's result is in `results`, by feature id: the first result of an id, as
/// `Model::result` finds it.
pub(crate) fn result_index(results: &[solidcraft_engine::doc::FeatureResult]) -> std::collections::HashMap<u64, usize> {
    results.iter().enumerate().rev().map(|(k, r)| (r.id, k)).collect()
}

/// The common prefix of a feature tooltip and its copied status message.
pub(crate) fn status_text(name: &str, ty: &str, error: Option<&str>, warning: Option<&str>) -> String {
    let mut text = format!("{name}  ({ty})");
    if let Some(error) = error {
        text.push('\n');
        text.push_str(&crate::i18n::format("timeline.feature_error", &[("message", &crate::i18n::tr_diagnostic(error))]));
    }
    if let Some(warning) = warning {
        text.push('\n');
        text.push_str(&crate::i18n::format("timeline.feature_warning", &[("message", &crate::i18n::tr_diagnostic(warning))]));
    }
    text
}

/// A feature's mark: an error, else a warning; none while it is suppressed or rolled back, as
/// it isn't built then.
pub(crate) fn feature_status(app: &SolveApp, id: u64) -> Option<Status> {
    let index = app.session.doc.feature_index(id)?;
    status_at(app, index, || app.session.model.result(id))
}

/// The mark of the feature at `index` in the history, given how to find its result.
fn status_at<'a>(app: &SolveApp, index: usize, result: impl FnOnce() -> Option<&'a solidcraft_engine::doc::FeatureResult>) -> Option<Status> {
    let feature = app.session.doc.features.get(index)?;
    if feature.suppressed || app.session.doc.marker.is_some_and(|marker| index >= marker) {
        return None;
    }
    let result = result()?;
    if result.error.is_some() { Some(Status::Error) } else { result.warning.as_ref().map(|_| Status::Warning) }
}

/// The worst mark among `ids`, with the message of the first feature that has it.
pub(crate) fn worst_status(app: &SolveApp, ids: &[u64]) -> (Option<Status>, Option<String>) {
    let mark = |status: Option<Status>, id: &u64| status.map(|status| (status, *id));
    // A long folder finds each feature's place and result in tables built once: a search per
    // feature made a folded long history cost the square of its length every frame. A short
    // one searches, which costs less than building the tables.
    let marks: Vec<(Status, u64)> = if ids.len() < 16 {
        ids.iter().filter_map(|id| mark(feature_status(app, *id), id)).collect()
    } else {
        let places: std::collections::HashMap<u64, usize> = app.session.doc.features.iter().enumerate().rev().map(|(i, f)| (f.id, i)).collect();
        let results = result_index(&app.session.model.results);
        let status = |id: &u64| {
            let index = *places.get(id)?;
            status_at(app, index, || results.get(id).and_then(|k| app.session.model.results.get(*k)))
        };
        ids.iter().filter_map(|id| mark(status(id), id)).collect()
    };
    let worst = marks.into_iter().reduce(|a, b| if b.0 > a.0 { b } else { a });
    match worst {
        Some((status, id)) => (Some(status), feature_message(app, id)),
        None => (None, None),
    }
}

/// What a feature's error or warning mark says, as its tooltip shows it (name and type, then
/// the error and the warning), for the clipboard. `None` when it has neither.
pub(crate) fn feature_message(app: &SolveApp, id: u64) -> Option<String> {
    let result = app.session.model.result(id)?;
    if result.error.is_none() && result.warning.is_none() {
        return None;
    }
    let feature = app.session.doc.features.iter().find(|f| f.id == id)?;
    Some(status_text(&feature.name, feature.kind.type_name(), result.error.as_deref(), result.warning.as_deref()))
}

/// Copy Error Message (or Copy Warning Message) in the menu of a feature that has one.
pub(crate) fn message_button(app: &SolveApp, ui: &mut egui::Ui, id: u64) {
    let Some(text) = feature_message(app, id) else { return };
    let failed = app.session.model.result(id).is_some_and(|r| r.error.is_some());
    let label = if failed { "Copy Error Message" } else { "Copy Warning Message" };
    if crate::context_menu::button(ui, crate::i18n::tr_label(label), "copy").clicked() {
        ui.ctx().copy_text(text);
        ui.close();
    }
}

/// Reuse the Browser action so both History layouts edit the same sketch attachment.
pub(crate) fn sketch_plane_button(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) {
    if !matches!(app.session.doc.feature(id).map(|feature| &feature.kind), Some(FeatureKind::Sketch { .. })) {
        return;
    }
    let Some(item) =
        crate::context_menu::items(app, &crate::context_menu::Target::Sketch { id }).into_iter().find(|item| item.id == "ui.redefineSketch")
    else {
        return;
    };
    let response = crate::context_menu::button_enabled(ui, &item.label, &item.icon, item.enabled);
    if response.clicked() {
        crate::context_menu::run_item(app, &item, response.rect.center());
        ui.close();
    }
}

/// Shift+click on a feature: select the features from the anchor (the last feature clicked, or
/// the last selected one) to this one. `order` is the timeline as drawn this frame, taken before
/// drawing so the range never mixes two orders. Ctrl as well adds the range to the selection.
fn select_range(app: &mut SolveApp, order: &[u64], id: u64, add: bool) {
    let anchor = app.tree.timeline_anchor.clone().or_else(|| selected_feature_ids(app).last().map(|f| anchor_key(*f)));
    let from = anchor.and_then(|key| order.iter().position(|f| anchor_key(*f) == key));
    let (Some(from), Some(to)) = (from, order.iter().position(|f| *f == id)) else {
        select_one(app, id);
        return;
    };
    let range = order.get(from.min(to)..=from.max(to)).unwrap_or_default();
    let items: Vec<_> = range.iter().map(|f| json!({"type": "feature", "id": f})).collect();
    let _ = app.run("select.set", json!({ "items": items, "add": add }));
}

/// Give a History feature click to a compatible active dialog input in any layout.
/// Plain clicks keep an existing pick; Ctrl/Command clicks toggle it off.
pub(crate) fn pick_feature_into_dialog(app: &mut SolveApp, id: u64, modifiers: egui::Modifiers) -> bool {
    // A pattern or Mirror says why it does not take a sketch (#10057).
    if app.dialog.as_mut().is_some_and(|d| d.refuse_sketch(&app.session, id)) {
        return true;
    }
    let Some(d) = app.dialog.as_mut().filter(|d| d.active_input().is_some_and(|i| i.accept & crate::selection::FEATURES != 0)) else {
        return false;
    };
    let feature = solidcraft_engine::Sel::Feature { id };
    if modifiers.command || modifiers.ctrl || !d.active_input().is_some_and(|i| i.items.contains(&feature)) {
        d.pick(&app.session, feature);
    }
    true
}

/// A plain click on a feature: select it alone.
fn select_one(app: &mut SolveApp, id: u64) {
    app.tree.picked_joints.clear();
    app.tree.picked_timeline_groups.clear();
    app.tree.timeline_anchor = Some(anchor_key(id));
    let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": id}]}));
}

/// Ctrl+click on a feature: add it to the selection, or take it out.
fn toggle_feature(app: &mut SolveApp, id: u64) {
    app.tree.timeline_anchor = Some(anchor_key(id));
    let selected = selected_feature_ids(app);
    if selected.contains(&id) {
        let rest: Vec<_> =
            app.session.selection.iter().filter(|s| !matches!(s, solidcraft_engine::Sel::Feature { id: f } if *f == id)).cloned().collect();
        let _ = app.run("select.set", json!({ "items": rest }));
    } else {
        let _ = app.run("select.set", json!({ "items": [{"type": "feature", "id": id}], "add": true }));
    }
}

/// One history step at a time. The delay starts when the computed step is displayed, so a
/// slow rebuild cannot consume the pause or make queued marker moves skip intermediate steps.
pub(crate) struct Playback {
    revision: u64,
    position: usize,
    waiting: bool,
    next_at: f64,
}

const PLAYBACK_STEP_SECONDS: f64 = 0.5;

pub(crate) fn stop_playback(app: &mut SolveApp) {
    if app.timeline_playback.take().is_some_and(|p| p.waiting) {
        // The worker only fills the evaluation cache. Dropping its reply prevents an
        // interrupted playback step from later moving the marker in an edited/new design.
        app.deferred = None;
    }
}

pub(crate) fn playback_command_info(app: &SolveApp, command: &solidcraft_engine::CommandSpec) -> solidcraft_engine::CommandInfo {
    let mut info = command.info(&app.session);
    if app.timeline_playback.is_some() {
        info.label = "Stop History";
        info.icon = "timeline_stop";
    } else if let Err(why) = playback_ready(app) {
        info.enabled = false;
        info.disabled_reason = Some(why);
    }
    info
}

/// What playback can't run alongside: starting refuses on these, and a running playback stops on them.
fn playback_blocked(app: &SolveApp) -> bool {
    app.home.open
        || crate::render_workspace::active(app)
        || app.session.active_sketch.is_some()
        || app.dialog.is_some()
        || app.tool.is_some()
        || app.file_load.busy()
        || app.file_picker.is_some()
        || app.saving.is_some()
        || app.applying.is_some()
        || app.delete_check.is_some()
        || !app.deferred_queue.is_empty()
}

fn playback_ready(app: &SolveApp) -> Result<(), String> {
    if app.computing_since().is_some() || playback_blocked(app) {
        return Err(crate::i18n::tr("ui.history_play_finish_action").into());
    }
    let n = app.session.doc.features.len();
    if app.session.doc.marker.unwrap_or(n) >= n {
        return Err(crate::i18n::tr("ui.history_play_roll_back").into());
    }
    Ok(())
}

pub(crate) fn request_playback(app: &mut SolveApp, action: PlaybackAction) -> Result<(), String> {
    if matches!(action, PlaybackAction::Stop) || (matches!(action, PlaybackAction::Toggle) && app.timeline_playback.is_some()) {
        stop_playback(app);
        return Ok(());
    }
    if app.timeline_playback.is_some() {
        return Ok(());
    }
    playback_ready(app)?;
    let n = app.session.doc.features.len();
    let position = app.session.doc.marker.unwrap_or(n);
    app.timeline_playback = Some(Playback { revision: app.session.revision, position, waiting: false, next_at: 0.0 });
    Ok(())
}

pub(crate) fn tick_playback(app: &mut SolveApp, ctx: &egui::Context) {
    if app.timeline_playback.is_none() {
        return;
    }
    if playback_blocked(app) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        stop_playback(app);
        return;
    }
    let Some(mut playback) = app.timeline_playback.take() else { return };
    let now = ctx.input(|i| i.time);
    let n = app.session.doc.features.len();
    let position = app.session.doc.marker.unwrap_or(n);
    if playback.waiting {
        if app.deferred.is_some() {
            if app.session.revision == playback.revision {
                app.timeline_playback = Some(playback);
            } else {
                app.deferred = None;
            }
            return;
        }
        // A failed step, another edit, or a different marker move stops the sequence.
        if app.session.revision != playback.revision.saturating_add(1) || position != playback.position {
            return;
        }
        playback.waiting = false;
        playback.revision = app.session.revision;
        playback.next_at = now + PLAYBACK_STEP_SECONDS;
    } else if app.session.revision != playback.revision || position != playback.position || app.deferred.is_some() {
        return;
    }
    if position >= n {
        return;
    }
    if now >= playback.next_at {
        playback.position = position.saturating_add(1).min(n);
        if app.run_heavy("timeline.roll_to", json!({"position": playback.position})).is_err() {
            return;
        }
        // Observe completion on the next frame even for a synchronous rebuild (the web).
        // This frame's input time predates that work and cannot time the visible pause.
        playback.waiting = true;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(50));
    app.timeline_playback = Some(playback);
}

pub fn timeline(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    app.shell.history_dock_space = Some(ui.available_rect_before_wrap());
    if Container::for_layout(app.ui.layout, Surface::Panel).floating() {
        let rect = floating_rect(ui.available_rect_before_wrap());
        // A window too short for the card (the ribbon already fills it) leaves the history out
        // rather than drawing into a negative-size rectangle.
        if !rect.is_finite() || rect.width() < 1.0 || rect.height() < 30.0 {
            app.shell.history = None;
            return;
        }
        app.shell.history = Some(rect);
        crate::scenario::publish_rect("history:card", rect);
        let mut child = ui.new_child(UiBuilder::new().id_salt("sc_timeline").max_rect(rect));
        Container::paint_panel_frame(
            ui.painter(),
            Container::new(ui.ctx(), Surface::Panel).frame(ui.ctx(), t.timeline, crate::frame::Side::Left),
            rect,
        );
        child.set_clip_rect(rect);
        timeline_contents(app, &mut child, true);
    } else {
        let host = Container::new(ui.ctx(), Surface::Panel);
        let response = host.show_edge(ui, host.edge("sc_timeline", Edge::Bottom).exact_size(40.0), host.edge_frame(t.timeline), |ui| {
            timeline_contents(app, ui, false);
        });
        let frame = host.edge_frame(t.timeline);
        Container::paint_inset(ui.painter(), response.response.rect - frame.outer_margin, frame);
        app.shell.history = Some(response.response.rect);
        crate::scenario::publish_rect("history:strip", response.response.rect);
    }
}

pub(crate) fn floating_rect(available: Rect) -> Rect {
    Container::floating_card(Surface::Panel).bottom_strip_rect(available, 40.0)
}

#[derive(Clone, Debug)]
enum FolderTile {
    Feature(usize),
    Folder(u64),
    Joints(Vec<u64>),
}

/// Lay out each folder as a block, keeping feature order and placing its joints at its end.
fn folder_tiles(app: &SolveApp, features: &[u64]) -> Vec<FolderTile> {
    if app.session.doc.timeline_groups.is_empty() {
        return (0..features.len()).map(FolderTile::Feature).collect();
    }
    fn entries(app: &SolveApp, features: &[u64], parent: Option<u64>) -> Vec<FolderTile> {
        let groups = &app.session.doc.timeline_groups;
        let mut entries: Vec<(usize, u64, FolderTile)> = features
            .iter()
            .enumerate()
            .filter(|(_, id)| groups.iter().find(|group| group.features.contains(id)).map(|group| group.id) == parent)
            .map(|(index, id)| (index, *id, FolderTile::Feature(index)))
            .collect();
        for group in groups.iter().filter(|group| group.parent == parent) {
            let mut pending = vec![group.id];
            let mut seen = std::collections::BTreeSet::new();
            let mut first = features.len();
            while let Some(id) = pending.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if let Some(group) = groups.iter().find(|group| group.id == id) {
                    first = first.min(features.iter().position(|id| group.features.contains(id)).unwrap_or(features.len()));
                    pending.extend(groups.iter().filter(|child| child.parent == Some(id)).map(|child| child.id));
                }
            }
            entries.push((first, group.id, FolderTile::Folder(group.id)));
        }
        entries.sort_by_key(|(index, id, _)| (*index, *id));
        entries.into_iter().map(|(_, _, tile)| tile).collect()
    }

    let mut out = Vec::new();
    let mut pending = entries(app, features, None);
    pending.reverse();
    let mut visited = std::collections::BTreeSet::new();
    while let Some(tile) = pending.pop() {
        if let FolderTile::Folder(id) = tile {
            if !visited.insert(id) {
                continue;
            }
            out.push(FolderTile::Folder(id));
            if !app.tree.collapsed.contains(&format!("timeline_group:{id}")) {
                if let Some(group) = app.session.doc.timeline_groups.iter().find(|group| group.id == id) {
                    pending.push(FolderTile::Joints(group.joints.clone()));
                }
                pending.extend(entries(app, features, Some(id)).into_iter().rev());
            }
        } else {
            out.push(tile);
        }
    }
    out
}

fn folder_tile(app: &mut SolveApp, ui: &mut egui::Ui, p: &egui::Painter, x: f32, r: Rect, id: u64) -> f32 {
    let Some(group) = app.session.doc.timeline_groups.iter().find(|group| group.id == id).cloned() else { return x };
    let t = Tokens::get();
    let br = Rect::from_min_size(pos2(x, r.top() + 6.0), vec2(28.0, 28.0));
    let key = format!("timeline_group:{id}");
    let open = !app.tree.collapsed.contains(&key);
    let resp = ui.interact(br, ui.id().with(("folder", id)), Sense::click());
    p.rect(
        br,
        t.radius_sm.saturating_sub(1),
        if resp.hovered() { t.hover } else { t.timeline_item },
        t.control_border_stroke(t.border),
        egui::StrokeKind::Inside,
    );
    crate::icons::paint_icon(p, br.shrink(4.0), "folder", f32::from(t.icon.a()) / 255.0);
    let mut pending = vec![id];
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0;
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(group) = app.session.doc.timeline_groups.iter().find(|group| group.id == id) {
            count += group.features.len() + group.joints.len();
            pending.extend(app.session.doc.timeline_groups.iter().filter(|child| child.parent == Some(id)).map(|child| child.id));
        }
    }
    p.text(br.right_bottom(), egui::Align2::RIGHT_BOTTOM, count.to_string(), egui::FontId::proportional(t.font_micro), t.text);
    crate::scenario::publish_handle(&format!("timeline-folder:{}", group.name), br.center());
    crate::scenario::publish_handle(&format!("timeline-folder-fold:{}", group.name), br.center());
    if resp.clicked() {
        if open {
            app.tree.collapsed.insert(key);
        } else {
            app.tree.collapsed.remove(&key);
        }
    }
    resp.on_hover_text(format!("{} ({count})", group.name));
    x + 32.0
}

fn timeline_contents(app: &mut SolveApp, ui: &mut egui::Ui, floating: bool) {
    let t = Tokens::get();
    let accent = crate::theme::TabAccent::for_tab(&t, &app.ui.tab);
    let r = ui.max_rect();
    let p = ui.painter().clone();

    let n = app.session.doc.features.len();
    let marker = app.session.doc.marker.unwrap_or(n);
    // Playback controls.
    let mut x = r.left() + t.space_10;
    let grip_rect = Rect::from_center_size(pos2(x + t.space_10 / 2.0, r.center().y), vec2(t.space_10, t.space_16 + t.space_2));
    let mut grip_ui = ui.new_child(UiBuilder::new().max_rect(grip_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let grip = crate::ui_kit::panel_grip(&mut grip_ui).rect;
    x += t.space_10 + t.space_8;
    let mut drag_rect = grip;
    let heading = crate::ui_kit::StripHeading::new(&p, crate::i18n::tr("ui.history_title"));
    if x + heading.width() + crate::ui_kit::history_transport_width() <= r.right() {
        let title_rect = heading.paint(&p, Rect::from_min_max(pos2(x, r.top()), r.right_bottom()));
        drag_rect = drag_rect.union(title_rect);
        x += heading.width();
    }
    let response = ui.interact(drag_rect, egui::Id::new("sc_history_dock_grip"), Sense::drag());
    crate::scenario::publish_handle("history:grip", drag_rect.center());
    crate::timeline_left::dock_drag(app, ui, &response);
    let controls = crate::ui_kit::HistoryTransport::shown(app.preferences.history_play_button);
    for &control in controls {
        let br = Rect::from_center_size(pos2(x + t.space_24 / 2.0, r.center().y), vec2(t.space_24, t.space_24));
        let response = control.show(ui, br, app.timeline_playback.is_some());
        transport_response(app, control, &response);
        x += t.space_24 + t.space_2;
    }
    x += 14.0;
    // The features scroll horizontally in the space right of the playback controls.
    let strip = Rect::from_min_max(pos2(x, r.top()), r.right_bottom());
    ui.scope_builder(UiBuilder::new().max_rect(strip), |ui| {
        // A plain mouse wheel scrolls the strip sideways too (#3356).
        ui.style_mut().always_scroll_the_only_direction = true;
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
            let p = ui.painter().clone();
            let mut x = ui.max_rect().left();
            let origin = x;
            // In an assembly, only the active (or picked) component's features (#2880).
            let show_all_id = ui.id().with("tl_show_all");
            let show_all = ui.data(|d| d.get_temp::<bool>(show_all_id)).unwrap_or(false);
            let only = timeline_component(app, show_all);
            let filtering = only.is_some() || show_all;
            let shown = shown_feature_indices(&app.session.doc, only);
            if filtering || app.session.doc.features.iter().any(|f| f.component != 0) {
                // A toggle ahead of the features: this part only, or the whole design.
                let label = match only {
                    Some(c) => app.session.doc.components.iter().find(|k| k.id == c).map_or_else(|| "Component".to_string(), |k| k.name.clone()),
                    None => "All components".to_string(),
                };
                let galley = ui.painter().layout_no_wrap(label.clone(), egui::FontId::proportional(t.font_label), t.text);
                let br = Rect::from_min_size(pos2(x, r.top() + 9.0), vec2(galley.size().x + 12.0, 22.0));
                crate::scenario::publish_handle("timeline:components", br.center());
                let resp = ui.interact(br, ui.id().with("tl_components"), Sense::click());
                p.rect(br, 3.0, if resp.hovered() { t.hover } else { t.timeline_item }, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                p.galley(br.left_center() + vec2(6.0, -galley.size().y * 0.5), galley, t.text);
                let tip = if show_all {
                    "Showing every component's features. Click to show only the active or picked component's"
                } else if only.is_some() {
                    "Showing this component's features only. Click to show the whole design's"
                } else {
                    "Activate or pick a component in the browser to show only its features"
                };
                if resp.on_hover_text(tip).clicked() && (only.is_some() || show_all) {
                    ui.data_mut(|d| d.insert_temp(show_all_id, !show_all));
                }
                x = br.right() + 8.0;
            }
            let design_marker = marker;
            let marker = shown.iter().position(|i| *i >= marker).unwrap_or(shown.len());
            let feats: Vec<_> = shown
                .iter()
                .filter_map(|i| app.session.doc.features.get(*i))
                .map(|f| (f.id, f.name.clone(), icon_of(&f.kind), f.suppressed, f.kind.type_name(), TimelineKind::of(&f.kind)))
                .collect();
            let order: Vec<u64> = feats.iter().map(|f| f.0).collect();
            // Find in Timeline: scroll the found feature into the strip on this frame (#5850).
            let reveal = app.tree.timeline_reveal.take();
            // The feature that made what is under the cursor or picked in the viewport.
            let from_view = [hovered_feature(app), picked_feature(app)];
            let drag_id = ui.id().with("tl_drag");
            let mut drag: Option<Drag> = ui.data(|d| d.get_temp(drag_id));
            let pointer = ui.input(|i| i.pointer.hover_pos());
            let mut xs: Vec<f32> = vec![x; feats.len()];
            let mut hovered: Option<u64> = None;
            let mut edit: Option<u64> = None;
            let mut marker_rect: Option<Rect> = None;
            // Each sketch's solve state, read once for the strip (#7245).
            let sketch_states = crate::sketch_state::SketchStates::of(app);
            // Each feature's result, found once for the strip: a search per chip made a long
            // timeline cost the square of its length every frame.
            let results = result_index(&app.session.model.results);
            // The features picked in the design or in the open dialog, collected once for the
            // strip: the dialog built its item list again for every chip.
            let feature_of = |selection: &solidcraft_engine::Sel| match selection {
                solidcraft_engine::Sel::Feature { id } => Some(*id),
                _ => None,
            };
            let picked: std::collections::HashSet<u64> = app
                .session
                .selection
                .iter()
                .filter_map(feature_of)
                .chain(app.dialog.as_ref().map(|dialog| dialog.items()).unwrap_or_default().iter().filter_map(feature_of))
                .collect();
            for tile in folder_tiles(app, &order) {
                let i = match tile {
                    FolderTile::Folder(id) => {
                        if marker_rect.is_none() && app.tree.collapsed.contains(&format!("timeline_group:{id}")) {
                            let mut pending = vec![id];
                            let mut seen = std::collections::BTreeSet::new();
                            let mut contains_marker = false;
                            while let Some(id) = pending.pop() {
                                if !seen.insert(id) {
                                    continue;
                                }
                                if let Some(group) = app.session.doc.timeline_groups.iter().find(|group| group.id == id) {
                                    contains_marker |= order.get(marker).is_some_and(|feature| group.features.contains(feature));
                                    pending.extend(
                                        app.session.doc.timeline_groups.iter().filter(|child| child.parent == Some(id)).map(|child| child.id),
                                    );
                                }
                            }
                            if contains_marker {
                                marker_rect = Some(Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(t.space_12, r.height() - 10.0)));
                                x += t.space_16;
                            }
                        }
                        x = folder_tile(app, ui, &p, x, r, id);
                        continue;
                    }
                    FolderTile::Joints(ids) => {
                        x = crate::dialogs_assembly::timeline_joint_tiles(app, ui, &p, x, r, Some(&ids));
                        continue;
                    }
                    FolderTile::Feature(index) => index,
                };
                let Some((id, name, icon, suppressed, ty, kind)) = feats.get(i) else { continue };
                let sketch_state = matches!(kind, TimelineKind::Sketch).then(|| sketch_states.get(*id)).flatten();
                if i == marker {
                    marker_rect = Some(Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(t.space_12, r.height() - 10.0)));
                    x += t.space_16;
                }
                if let Some(slot) = xs.get_mut(i) {
                    *slot = x;
                }
                let br = Rect::from_min_size(pos2(x, r.top() + 6.0), vec2(28.0, 28.0));
                if reveal == Some(*id) {
                    ui.scroll_to_rect(br, Some(egui::Align::Center));
                }
                crate::scenario::publish_handle(&format!("timeline:{name}"), br.center());
                // A chip scrolled out of the strip is neither drawn nor interactive: a long
                // history drew every chip each frame. Its slot above still places the marker and drops.
                // One being dragged or focused, or under an open menu, stays alive until that ends.
                let chip_id = ui.id().with(("feat", *id));
                if !ui.clip_rect().intersects(br)
                    && drag != Some(Drag::Feature(*id))
                    && !ui.memory(|m| m.has_focus(chip_id))
                    && !egui::Popup::is_any_open(ui.ctx())
                {
                    x += 32.0;
                    continue;
                }
                let resp = ui.interact(br, chip_id, Sense::click_and_drag());
                // A feature without a result has none; an index that no longer holds this id (a
                // command run from an earlier chip evaluated again) is searched for.
                let res =
                    results.get(id).and_then(|k| app.session.model.results.get(*k).filter(|r| r.id == *id).or_else(|| app.session.model.result(*id)));
                let err = res.and_then(|r| r.error.clone());
                let warn = res.and_then(|r| r.warning.clone());
                let rolled = i >= marker;
                let sel = picked.contains(id);
                if resp.hovered() {
                    hovered = Some(*id);
                }
                crate::ui_kit::HistoryChip {
                    icon,
                    name,
                    kind: ty,
                    selected: sel,
                    hovered: resp.hovered() || from_view.contains(&Some(*id)),
                    dim: rolled,
                    suppressed: *suppressed,
                    error: err.is_some(),
                    warning: warn.is_some(),
                    reference: false,
                    ink: sketch_state.and_then(|state| state.ink(&t)),
                    // A dragged feature takes the colour of its drop bar.
                    accent: if drag == Some(Drag::Feature(*id)) { crate::theme::TabAccent::for_tab(&t, "SKETCH") } else { accent },
                }
                .paint(&p, br, crate::ui_kit::HistoryStyle::Bottom);
                if floating && marker > 0 && i == marker - 1 {
                    p.circle_filled(pos2(br.center().x, br.bottom() + t.space_3), t.space_2, accent.color);
                }
                let ms = res.map(|r| r.ms);
                if resp.drag_started() {
                    drag = Some(Drag::Feature(*id));
                }
                // The tip is written only while it shows (as `on_hover_text` would lay it out).
                let resp = resp.on_hover_ui(|ui| {
                    let mut tip = status_text(name, ty, err.as_deref(), warn.as_deref());
                    // Greyed out says only "not shown": say why (#8100).
                    if rolled {
                        tip.push('\n');
                        tip.push_str(crate::i18n::tr("timeline.feature_rolled_back"));
                    }
                    if let Some(state) = sketch_state {
                        tip.push('\n');
                        tip.push_str(&state.text());
                    }
                    if let Some(ms) = ms {
                        tip += &format!("\n{ms:.1} ms");
                    }
                    tip.push('\n');
                    tip.push_str(crate::i18n::tr("timeline.feature_hint"));
                    ui.set_max_width(ui.spacing().tooltip_width);
                    ui.add(egui::Label::new(tip));
                });
                if resp.clicked() && !pick_feature_into_dialog(app, *id, ui.input(|i| i.modifiers)) {
                    let modifiers = ui.input(|i| i.modifiers);
                    if modifiers.shift {
                        select_range(app, &order, *id, modifiers.command || modifiers.ctrl);
                    } else if modifiers.command || modifiers.ctrl {
                        toggle_feature(app, *id);
                    } else if !modifiers.any() && crate::browser::pick_sketch_into_dialog(app, *id) {
                        // Extrude, Revolve or Emboss took every profile of the sketch (#6106).
                    } else {
                        select_one(app, *id);
                    }
                }
                if resp.double_clicked() {
                    edit = Some(*id);
                }
                let sketch_of = kind.profile_sketch();
                crate::ui_kit::Menu::context(&resp, |ui| {
                    if crate::context_menu::button(ui, crate::i18n::tr_label("Edit Feature"), "edit").clicked() {
                        edit = Some(*id);
                        ui.close();
                    }
                    sketch_plane_button(app, ui, *id);
                    if let Some(sk) = sketch_of
                        && crate::context_menu::button(ui, crate::i18n::tr_label("Edit Profile Sketch"), "sketch").clicked()
                    {
                        app.edit_sketch(sk.0);
                        ui.close();
                    }
                    if crate::context_menu::button(ui, crate::i18n::tr_label("Rename"), "rename").clicked() {
                        app.dialog = Some(crate::dialogs::Dialog::rename(*id, name));
                        ui.close();
                    }
                    // Its own message, not a literal: no catalog held the literal, so the item
                    // stayed English in every language (#8301).
                    let suppress = if *suppressed { "timeline.unsuppress_features" } else { "timeline.suppress_features" };
                    if crate::context_menu::button(ui, crate::i18n::tr(suppress), "suppress").clicked() {
                        let _ = app.run_heavy("timeline.suppress", json!({"feature": id}));
                        ui.close();
                    }
                    if crate::context_menu::button(ui, crate::i18n::tr_label("Roll History Marker Here"), "rollback").clicked() {
                        let _ = app.run_heavy("timeline.roll_to", json!({ "feature": id }));
                        ui.close();
                    }
                    if crate::context_menu::button(ui, crate::i18n::tr_label("Find in Browser"), "list").clicked() {
                        find_in_browser(app, *id);
                        ui.close();
                    }
                    ui.separator();
                    clipboard_buttons(app, ui, *id);
                    message_button(app, ui, *id);
                    ui.separator();
                    delete_button(app, ui, *id);
                    delete_after_marker_button(app, ui, *id, design_position(&shown, i, n), design_marker, Some(&shown));
                });
                x += 32.0;
            }
            let mut previous = origin;
            for slot in &mut xs {
                *slot = (*slot).max(previous);
                previous = *slot;
            }
            let end = x;
            if marker >= feats.len() {
                marker_rect = Some(Rect::from_min_size(pos2(x, r.top() + 5.0), vec2(t.space_12, r.height() - 10.0)));
            }
            // The history marker: drag it to roll the model back or forward.
            if let Some(mr) = marker_rect {
                crate::scenario::publish_rect("history:marker", mr);
                let resp = ui.interact(mr.expand2(vec2(3.0, 0.0)), ui.id().with("tl_marker"), Sense::drag());
                if resp.drag_started() {
                    drag = Some(Drag::Marker);
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                crate::ui_kit::history_marker(&p, mr, false, accent);
                resp.on_hover_text(crate::i18n::tr("ui.history_marker_tip"));
            }
            let content_end = crate::dialogs_assembly::timeline_joints(app, ui, &p, end + 10.0, r);
            // The scroll range follows the drawn content (the buttons don't take layout space themselves).
            ui.allocate_exact_size(vec2((content_end - origin).max(0.0), r.height()), Sense::hover());
            // Drag feedback, and the drop.
            if drag.is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            let marker_slot = marker.min(n);
            // At the marker's slot, which side of the marker the pointer is on: before it, a
            // dropped feature stays in the model; after it, the feature is rolled back.
            let before_marker = |slot: usize, x: f32| slot < marker_slot || (slot == marker_slot && marker_rect.is_none_or(|mr| x < mr.center().x));
            // The marker dropped where it already is needs no bar: the marker shows the spot.
            if let (Some(dg), Some(pp)) = (drag.as_ref(), pointer)
                && let slot = slot_at(&xs, end, pp.x)
                && !(matches!(dg, Drag::Marker) && slot == marker_slot)
            {
                // Centred in the gap between two chips, or beside the marker at its slot.
                let sx = match marker_rect {
                    Some(mr) if slot == marker_slot => {
                        if before_marker(slot, pp.x) {
                            mr.left() - 2.0
                        } else {
                            mr.right() + 2.0
                        }
                    }
                    _ => xs.get(slot).copied().unwrap_or(end) - 2.0,
                };
                // Kept inside the strip so the slot before the first feature stays visible.
                let sx = sx.max(origin + t.space_2);
                crate::ui_kit::history_drop_bar(
                    &p,
                    Rect::from_min_max(pos2(sx, r.top() + 3.0), pos2(sx, r.bottom() - 3.0)),
                    false,
                    if matches!(dg, Drag::Marker) { accent.color } else { t.sketch_accent },
                );
            }
            let released = ui.input(|i| i.pointer.any_released());
            // (Taking the drag ends it even when the pointer has left the window.)
            if released
                && let Some(dg) = drag.take()
                && let Some(pp) = pointer
            {
                let slot = design_position(&shown, slot_at(&xs, end, pp.x), n);
                match dg {
                    Drag::Marker => {
                        let _ = app.run_heavy("timeline.roll_to", if slot >= n { json!({}) } else { json!({ "position": slot }) });
                    }
                    Drag::Feature(id) => {
                        let from = app.session.doc.feature_index(id).unwrap_or(0);
                        // Dropping after itself means the slot shifts by one.
                        let to = if slot > from { slot - 1 } else { slot };
                        // Crossing the marker rolls the feature back or forward, in the same step.
                        let active = before_marker(slot, pp.x);
                        if to != from || active != (from < marker_slot) {
                            let _ = app.run_heavy("timeline.reorder", json!({ "feature": id, "position": to, "active": active }));
                        }
                    }
                }
            }
            ui.data_mut(|d| match drag {
                Some(dg) => {
                    d.insert_temp(drag_id, dg.clone());
                }
                None => d.remove::<Drag>(drag_id),
            });
            app.viewport.hover_feature = hovered;
            if let Some(id) = edit {
                app.edit_feature(id);
            }
        });
    });
}

/// Select what a feature made, so the browser shows it. While the browser's rows start closed
/// (Preferences › Layout), its first body is also found there, as Find in Browser on a body
/// does: the rows down to it open.
pub(crate) fn find_in_browser(app: &mut SolveApp, id: u64) {
    let st = app.session.world_state();
    let bodies: Vec<String> = st.bodies.iter().filter(|b| b.feature == id).map(|b| b.name.clone()).collect();
    let items: Vec<serde_json::Value> = if bodies.is_empty() {
        vec![json!({"type": "feature", "id": id})]
    } else {
        bodies.iter().map(|name| json!({"type": "body", "name": name})).collect()
    };
    let _ = app.run("select.set", json!({ "items": items }));
    if !crate::browser::rows_start_open(app)
        && let Some(first) = bodies.first()
    {
        crate::browser::reveal_body(app, first);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_engine::doc::{FeatureKind, PlaneRef};

    use crate::scenario::Harness;

    #[test]
    fn joint_folder_hides_fifty_joint_tiles_in_both_history_layouts() {
        use crate::scenario::handle_at;
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.timeline_left = left;
            h.app.run("component.create", json!({"name":"Part"})).unwrap();
            let mut joints = Vec::new();
            for index in 0..50 {
                joints.push(
                    h.app.run("joint.as_built", json!({"type":"rigid", "a":0, "b":"Part", "name":format!("Hinge{index}")})).unwrap()["joint"]
                        .as_u64()
                        .unwrap(),
                );
            }
            let folder = h.app.run("timeline.group", json!({"name":"Joints", "joints":joints})).unwrap()["group"].as_u64().unwrap();
            h.app.tree.collapsed.insert(format!("timeline_group:{folder}"));
            h.frames(4);
            assert!(handle_at("timeline-folder:Joints").is_some());
            assert_eq!(crate::scenario::handles("timeline-folder:").len(), 1);
            assert!(crate::scenario::handles("timeline-joint:").is_empty());
            for index in 0..50 {
                assert!(handle_at(&format!("timeline-joint:Hinge{index}")).is_none());
            }
            h.step(&json!({"click":{"handle":"timeline-folder-fold:Joints"}})).unwrap();
            h.frames(4);
            for index in 0..50 {
                assert!(handle_at(&format!("timeline-joint:Hinge{index}")).is_some(), "joint {index} missing, left={left}");
            }
            assert_eq!(crate::scenario::handles("timeline-joint:").len(), 50, "no trailing duplicates");
            h.step(&json!({"click":{"handle":"timeline-joint:Hinge0"}})).unwrap();
            assert_eq!(h.app.tree.picked_joints, [joints[0]]);
            h.step(&json!({"click":{"handle":"timeline-folder-fold:Joints"}})).unwrap();
            h.frames(4);
            assert!(handle_at("timeline-joint:Hinge0").is_none());
        }
    }

    #[test]
    fn history_status_translates_prefixes_and_both_diagnostics() {
        let diagnostic = "there is no body to cut or intersect in component 'Mount'; the tool may overlap 'Plate' in component 'Base' — activate that component or pick the body as a target";
        let _language = crate::i18n::scope("ja");
        let translated = crate::i18n::tr_diagnostic(diagnostic);
        assert_ne!(translated, diagnostic);
        assert_eq!(
            super::status_text("My feature", "ExtrudeFeature", Some(diagnostic), Some(diagnostic)),
            format!("My feature  (ExtrudeFeature)\nエラー: {translated}\n警告: {translated}")
        );
        assert_eq!(super::status_text("My feature", "ExtrudeFeature", None, None), "My feature  (ExtrudeFeature)");
        // Unknown diagnostics remain intact, including names and measured values.
        assert_eq!(
            super::status_text("My feature", "ExtrudeFeature", None, Some("Custom part: 1.25 mm")),
            "My feature  (ExtrudeFeature)\n警告: Custom part: 1.25 mm"
        );
        let _english = crate::i18n::scope("en");
        assert_eq!(super::status_text("Box", "BoxFeature", Some("error"), Some("warning")), "Box  (BoxFeature)\nError: error\nWarning: warning");
    }

    #[test]
    fn history_marker_tooltip_uses_the_language_in_both_layouts() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.language = "ja".into();
            h.app.preferences.timeline_left = left;
            h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            h.frames(4);
            let marker = crate::scenario::rect_of("history:marker").unwrap().center();
            h.call("ui.move", json!({"x": marker.x, "y": marker.y}));
            h.frames(90);
            let expected = crate::i18n::Locale::for_language("ja").text("ui.history_marker_tip");
            let output = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
            let texts: Vec<_> = crate::test_support::text_shapes(&output).map(|t| t.galley.text()).collect();
            assert!(texts.contains(&expected), "left={left}: {texts:?}");
        }
    }

    /// #8301: the feature menu's suppress item is drawn from the catalog, so it follows the
    /// reader's language. The literals it used were in no catalog and stayed English in every
    /// language, the Japanese UI included.
    #[test]
    fn the_feature_menu_suppress_item_follows_the_language() {
        let mut h = two_boxes(false);
        h.app.preferences.language = "ja".into();
        h.frames(2);
        let painted = |h: &Harness| -> Vec<String> {
            let output = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
            crate::test_support::text_shapes(&output).map(|t| t.galley.text().to_string()).collect()
        };
        // No English on screen: the item shows the reader's language.
        h.step(&json!({"click": {"handle": "timeline:First"}, "button": "right"})).unwrap();
        let texts = painted(&h);
        assert!(!texts.iter().any(|t| t == "Suppress Features"), "the English literal is painted in the Japanese UI: {texts:?}");
        let ja = crate::i18n::Locale::for_language("ja").text("timeline.suppress_features").to_string();
        let ja_unsuppress = crate::i18n::Locale::for_language("ja").text("timeline.unsuppress_features").to_string();
        assert!(texts.contains(&ja), "the item must read {ja}: {texts:?}");
        h.step(&json!({"key": "Escape"})).unwrap();
        // A suppressed feature offers the other way round.
        let first = h.app.session.doc.find_feature("First").map(|f| f.id).unwrap();
        h.app.run("timeline.suppress", json!({"feature": first, "suppressed": true})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "timeline:First"}, "button": "right"})).unwrap();
        let texts = painted(&h);
        assert!(!texts.iter().any(|t| t == "Unsuppress Features"), "the English literal is painted in the Japanese UI: {texts:?}");
        assert!(texts.contains(&ja_unsuppress), "the item must read {ja_unsuppress}: {texts:?}");
        h.step(&json!({"key": "Escape"})).unwrap();
        // The English wording is the one the menu always showed: the change adds messages, it
        // does not reword the item.
        assert_eq!(crate::i18n::Locale::for_language("en").text("timeline.suppress_features"), "Suppress Features");
        assert_eq!(crate::i18n::Locale::for_language("en").text("timeline.unsuppress_features"), "Unsuppress Features");
    }

    #[test]
    fn construction_history_matches_the_tool_that_created_it() {
        use solidcraft_engine::doc::{
            FeatureKind, PlaneRef,
            construct::{AxisDef, GeoRef, PointDef},
        };
        use solidcraft_engine::geom::Vec3;
        let point = || GeoRef::Point { p: Vec3::ZERO };
        let reference = || Box::new(point());
        let mut cases = Vec::new();
        for (id, def) in [
            ("through_cylinder", AxisDef::ThroughFace { face: point() }),
            ("normal_to_face", AxisDef::NormalToFace { face: point(), point: point() }),
            ("two_planes", AxisDef::TwoPlanes { a: point(), b: point() }),
            ("two_points", AxisDef::TwoPoints { a: point(), b: point() }),
            ("edge", AxisDef::Edge { edge: point() }),
        ] {
            cases.push((format!("construct.axis.{id}"), FeatureKind::ConstructionAxis { def }));
        }
        for (id, def) in [
            ("vertex", PointDef::At { point: point() }),
            ("two_edges", PointDef::TwoEdges { a: point(), b: point() }),
            ("three_planes", PointDef::ThreePlanes { a: point(), b: point(), c: point() }),
            ("center", PointDef::Center { of: point() }),
            ("edge_plane", PointDef::EdgeAndPlane { edge: point(), plane: point() }),
            ("along_path", PointDef::AlongPath { path: point(), t: "0.5".into() }),
        ] {
            cases.push((format!("construct.point.{id}"), FeatureKind::ConstructionPoint { def }));
        }
        for (id, plane) in [
            ("midplane", PlaneRef::Midplane { a: reference(), b: reference() }),
            ("two_edges", PlaneRef::TwoEdges { a: reference(), b: reference() }),
            ("three_points", PlaneRef::ThreePoints { a: reference(), b: reference(), c: reference() }),
            ("tangent", PlaneRef::Tangent { face: reference(), at: Vec3::ZERO }),
            ("along_path", PlaneRef::AlongPath { path: reference(), t: "0.5".into() }),
            ("perpendicular", PlaneRef::Perpendicular { plane: reference(), line: reference() }),
        ] {
            cases.push((format!("construct.plane.{id}"), FeatureKind::ConstructionPlane { plane }));
        }
        for (id, feature) in cases {
            let command = solidcraft_engine::find_command(&id).unwrap();
            let icon = super::icon_of(&feature);
            assert_eq!(icon, command.icon, "{id}: history and toolbar disagree");
            assert!(!crate::icon_svg::resolve(icon).placeholder, "{id}");
        }
    }

    #[test]
    fn new_surface_and_web_history_icons_match_their_commands() {
        let cases = [
            ("surface.ruled", json!({"type":"surface_ruled", "body":"A", "edges":[], "distance":"5"})),
            ("surface.untrim", json!({"type":"surface_untrim", "face":{"body":"A", "point":[0,0,0]}})),
            ("surface.unstitch", json!({"type":"unstitch", "body":"A"})),
            ("surface.reverse_normal", json!({"type":"reverse_normal", "body":"A"})),
            ("surface.blend", json!({"type":"surface_blend", "from":{"body":"A", "edges":[]}, "to":{"body":"B", "edges":[]}})),
            ("surface.boundary", json!({"type":"surface_boundary", "sketch":0, "curves":[]})),
            ("surface.boundary", json!({"type":"surface_network", "u":[]})),
            ("surface.match", json!({"type":"surface_match", "body":"A", "target":"B", "edges":[]})),
            ("surface.fair", json!({"type":"surface_fair", "body":"A", "tolerance":"0.001"})),
            ("solid.rib", json!({"type":"rib", "sketch":0, "curves":[], "thickness":"2"})),
            ("solid.web", json!({"type":"rib", "sketch":0, "curves":[], "thickness":"2", "web":true})),
        ];
        for (id, definition) in cases {
            let feature: solidcraft_engine::doc::FeatureKind = serde_json::from_value(definition).unwrap();
            let command = solidcraft_engine::find_command(id).unwrap();
            assert_eq!(super::icon_of(&feature), command.icon, "{id}: history and toolbar disagree");
        }
    }

    #[test]
    fn a_path_pattern_keeps_its_path_icon_in_history() {
        use solidcraft_engine::doc::{FeatureKind, PatternKind};
        let feature = FeatureKind::Pattern {
            features: Vec::new(),
            bodies: Vec::new(),
            pattern: PatternKind::Path {
                path_sketch: solidcraft_engine::doc::SketchId(0),
                path: Vec::new(),
                count: "2".into(),
                spacing: "10".into(),
                extent: false,
                flip: false,
                orient: false,
            },
        };
        assert_eq!(super::icon_of(&feature), solidcraft_engine::find_command("solid.pattern.path").unwrap().icon);
    }

    fn two_boxes(left: bool) -> Harness {
        let mut h = Harness::new();
        h.app.preferences.timeline_left = left;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "First", "body_name": "One"})).unwrap();
        h.app
            .run(
                "solid.box",
                json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Second", "body_name": "Two"}),
            )
            .unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(4);
        h
    }

    #[test]
    fn playback_command_runs_from_ribbon_palette_shortcut_and_json() {
        let mut h = two_boxes(false);
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.app.ui.tab = "INSPECT".into();
        h.frames(3);
        assert!(crate::scenario::handle_at("history:play").is_some(), "Play sits in the History controls by default");
        let command = solidcraft_engine::find_command("timeline.play").unwrap();
        let info = crate::render_workspace::interactive_command_info(&h.app, command);
        assert!(info.enabled);
        assert_eq!((info.label, info.icon), ("Play History", "timeline_play"));
        h.step(&json!({"click": {"handle": "toolbar:timeline.play"}})).unwrap();
        assert!(h.app.timeline_playback.is_some());
        let info = crate::render_workspace::interactive_command_info(&h.app, command);
        assert_eq!((info.label, info.icon), ("Stop History", "timeline_stop"));
        // Opening search must leave playback running so Stop is available there too.
        h.app.start("ui.palette");
        h.call("ui.palette", json!({"text": "timeline.play"}));
        h.frames(2);
        assert!(h.app.timeline_playback.is_some());
        let row = crate::shortcut_box::search(&h.app, "stop history").into_iter().find(|r| r.0 == "timeline.play").unwrap();
        assert_eq!((row.1, row.2), ("Stop History", "timeline_stop"));
        h.step(&json!({"key": "Enter"})).unwrap();
        assert!(h.app.timeline_playback.is_none());
        h.frames(3);
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        crate::keymap::bind(&mut h.app, "timeline.play", "Shift+P", true).unwrap();
        h.step(&json!({"key": "P", "shift": true})).unwrap();
        assert!(h.app.timeline_playback.is_some(), "marker {:?}, status {:?}", h.app.session.doc.marker, h.app.status);
        h.step(&json!({"key": "P", "shift": true})).unwrap();
        assert!(h.app.timeline_playback.is_none());
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.app.ui.show_timeline = false;
        let camera = serde_json::to_value(h.app.cam).unwrap();
        assert_eq!(h.call("engine.execute", json!({"command": "timeline.play", "params": {"action": "play"}}))["ok"], true);
        h.frames(2);
        assert_eq!(h.app.session.doc.marker, Some(1));
        h.app.run("timeline.play", json!({"action": "play"})).unwrap();
        assert!(h.app.timeline_playback.is_some(), "explicit Play is idempotent");
        assert_eq!(h.call("engine.execute", json!({"command": "timeline.play", "params": {"action": "stop"}}))["ok"], true);
        assert_eq!(serde_json::to_value(h.app.cam).unwrap(), camera, "playback never refits the view");
        h.app.run("timeline.play", json!({"action": "stop"})).unwrap();
        assert!(h.app.timeline_playback.is_none());
    }

    #[test]
    fn playback_shows_each_feature_in_every_history_layout() {
        for layout in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for (dock, width) in
                [("bottom", 130.0), ("left", 56.0), ("left", crate::ui_kit::history_width(true)), ("left", 320.0), ("under_browser", 170.0)]
            {
                let mut h = two_boxes(dock != "bottom");
                h.app.ui.layout = layout;
                h.app.preferences.browser_width = 170.0;
                h.app.run("view.history_layout", json!({"dock":dock,"width":width})).unwrap();
                h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
                h.frames(2);
                h.app.start("timeline.play");
                h.frames(2);
                assert_eq!(h.app.session.doc.marker, Some(1), "the first feature must be shown before the end; {layout:?} {dock} {width}");
                assert_eq!(h.app.session.model.state().bodies.len(), 1);
                h.frames(10);
                assert_eq!(h.app.session.doc.marker, Some(1), "leave time to see each step");
                h.frames(60);
                assert_eq!(h.app.session.doc.marker, None);
                assert_eq!(h.app.session.model.state().bodies.len(), 2);
            }
        }
    }

    #[test]
    fn playback_advances_in_an_embed_without_application_chrome() {
        let mut h = two_boxes(false);
        h.app.embedded = true;
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.frames(2);
        h.app.start("timeline.play");
        h.frames(2);
        assert_eq!(h.app.session.doc.marker, Some(1));
        assert_eq!(h.app.session.model.state().bodies.len(), 1);
        assert!(crate::scenario::rect_of("toolbar").is_none());
        h.frames(60);
        assert_eq!(h.app.session.doc.marker, None);
        assert_eq!(h.app.session.model.state().bodies.len(), 2);
        assert!(crate::scenario::rect_of("toolbar").is_none());
    }

    #[test]
    fn playback_can_be_stopped_or_interrupted() {
        for action in ["stop", "back", "last", "escape", "edit", "tool", "anchored_tool", "pinned_tool", "new"] {
            let mut h = two_boxes(false);
            h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
            h.frames(2);
            h.app.start("timeline.play");
            h.frames(2);
            assert_eq!(h.app.session.doc.marker, Some(1));
            match action {
                "stop" => h.app.start("timeline.play"),
                "back" => h.step(&json!({"click": {"handle": "history:back"}})).unwrap(),
                "last" => {
                    h.step(&json!({"click": {"handle": "history:last"}})).unwrap();
                    assert_eq!(h.app.session.doc.marker, None, "Go to end jumps immediately, even during playback");
                    assert_eq!(h.app.session.model.state().bodies.len(), 2);
                }
                "escape" => h.step(&json!({"key": "Escape"})).unwrap(),
                "edit" => {
                    h.app.run("timeline.rename", json!({"feature": "First", "name": "Renamed"})).unwrap();
                }
                "tool" => h.app.start("solid.box"),
                "anchored_tool" => h.app.start_at("solid.box", Some(egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(28.0, 28.0)))),
                "pinned_tool" => h.app.start_pinned("solid.box", None),
                _ => {
                    h.call("ui.documents", json!({"action": "new"}));
                }
            }
            h.frames(1);
            let marker = h.app.session.doc.marker;
            h.frames(90);
            assert!(h.app.timeline_playback.is_none(), "{action}");
            assert_eq!(h.app.session.doc.marker, marker, "no later step after {action}");
        }
    }

    #[test]
    fn playback_keeps_a_visible_pause_after_a_synchronous_rebuild() {
        let mut h = two_boxes(false);
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.app.run("timeline.play", json!({})).unwrap();
        super::tick_playback(&mut h.app, &h.ctx);
        assert_eq!(h.app.session.doc.marker, Some(1));
        // Model a rebuild that held the window thread for ten seconds. Its first visible
        // frame must still show the first feature; elapsed compute time is not playback.
        let time = h.ctx.input(|i| i.time) + 10.0;
        let _ = h.ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
            super::tick_playback(&mut h.app, ui.ctx());
        });
        assert_eq!(h.app.session.doc.marker, Some(1));
        assert_eq!(h.app.session.model.state().bodies.len(), 1);
    }

    #[test]
    fn playback_at_the_end_or_in_an_empty_design_leaves_history_unchanged() {
        for mut h in [Harness::new(), two_boxes(false)] {
            h.frames(2);
            let revision = h.app.session.revision;
            h.app.start("timeline.play");
            h.frames(2);
            h.frames(60);
            assert_eq!(h.app.session.revision, revision);
            assert!(h.app.timeline_playback.is_none());
        }
    }

    #[test]
    fn playback_refuses_to_start_while_other_history_work_waits() {
        let mut h = two_boxes(false);
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.app.deferred_queue.push(("timeline.roll_to".into(), json!({"position": 1})));
        assert!(h.app.run("timeline.play", json!({"action": "play"})).is_err(), "refused up front, not stopped a frame later");
        assert!(h.app.timeline_playback.is_none());
        h.app.deferred_queue.clear();
        h.app.run("timeline.play", json!({"action": "play"})).unwrap();
        assert!(h.app.timeline_playback.is_some());
    }

    fn right_click(h: &mut Harness, handle: &str) {
        let at = crate::scenario::handle_at(handle).unwrap();
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() };
        h.frame_with(vec![egui::Event::PointerMoved(at)]);
        h.frame_with(vec![button(true)]);
        h.frame_with(vec![button(false)]);
    }

    /// History shows Play between Previous and Next (#3688); turning it off gives back the four
    /// controls it had before.
    #[test]
    fn the_play_button_plays_and_stops_and_the_controls_menu_hides_it() {
        use crate::context_menu::Target;
        for (dock, width) in [("bottom", 130.0), ("left", 56.0), ("left", 320.0)] {
            let mut h = two_boxes(dock != "bottom");
            h.app.run("view.history_layout", json!({"dock": dock, "width": width})).unwrap();
            h.frames(2);
            assert!(h.app.preferences.history_play_button, "on by default");

            let at = |name: &str| crate::scenario::handle_at(name).unwrap();
            let (back, play, forward) = (at("history:back"), at("history:play"), at("history:forward"));
            let across = dock == "bottom" || width > 56.0;
            if across {
                assert!(back.x < play.x && play.x < forward.x, "Play sits between Previous and Next; {dock} {width}");
            } else {
                assert!(back.y < play.y && play.y < forward.y, "Play sits between Previous and Next; {dock} {width}");
            }

            h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
            h.frames(2);
            h.step(&json!({"click": {"handle": "history:play"}})).unwrap();
            assert!(h.app.timeline_playback.is_some(), "the button plays; {dock} {width}");
            h.step(&json!({"click": {"handle": "history:play"}})).unwrap();
            assert!(h.app.timeline_playback.is_none(), "and stops; {dock} {width}");

            right_click(&mut h, "history:back");
            assert!(h.app.viewport.context_menu.is_some() && h.app.menu.target == Target::HistoryControls, "{dock} {width}");
            let items = crate::context_menu::items(&h.app, &Target::HistoryControls);
            assert_eq!(items[0].label, crate::i18n::tr("ui.history_play_button_hide"));
            crate::context_menu::run_item(&mut h.app, &items[0], egui::Pos2::ZERO);
            assert!(!h.app.preferences.history_play_button);
            h.frames(2);
            assert!(crate::scenario::handle_at("history:play").is_none(), "the four controls again; {dock} {width}");

            let items = crate::context_menu::items(&h.app, &Target::HistoryControls);
            assert_eq!(items[0].label, crate::i18n::tr("ui.history_play_button_show"));
            crate::context_menu::run_item(&mut h.app, &items[0], egui::Pos2::ZERO);
            h.frames(2);
            assert!(crate::scenario::handle_at("history:play").is_some(), "and Play back; {dock} {width}");
        }
    }

    #[test]
    fn sketch_context_menu_redefines_the_plane_in_both_timelines() {
        for left in [false, true] {
            let mut h = two_boxes(left);
            let sketch = h.app.run("sketch.create", json!({"plane":"XY","name":"Profile"})).unwrap()["sketch"].as_u64().unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[1,1],"p1":[8,6]})).unwrap();
            h.app.finish_sketch();
            h.settle();
            let before = h.app.session.doc.to_json();
            let raw = serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap();
            let marker = h.app.session.doc.marker;
            let undo = h.app.session.undo.len();
            let item = crate::context_menu::items(&h.app, &crate::context_menu::Target::Sketch { id: sketch })
                .into_iter()
                .find(|item| item.id == "ui.redefineSketch")
                .unwrap();
            h.step(&json!({"click":{"handle":"timeline:First"},"button":"right"})).unwrap();
            assert!(h.widget(&item.label).is_none(), "a body feature has no sketch plane");
            h.step(&json!({"key":"Escape"})).unwrap();
            for apply in [false, true] {
                h.step(&json!({"click":{"handle":"timeline:Profile"},"button":"right"})).unwrap();
                h.step(&json!({"widget":item.label})).unwrap();
                assert_eq!(h.app.tree.redefine, Some(sketch));
                assert_eq!(h.app.session.doc.to_json(), before, "opening the menu changes no geometry");
                h.step(&json!({"widget":"Use XZ plane"})).unwrap();
                h.settle();
                h.step(&json!({"click":{"handle":if apply { "dialog:ok" } else { "dialog:cancel" }}})).unwrap();
                assert!(h.app.tree.redefine.is_none());
                assert_eq!(h.app.session.doc.marker, marker);
                assert_eq!(serde_json::to_value(h.app.session.doc.sketch(sketch).unwrap()).unwrap(), raw);
                if apply {
                    assert!(matches!(&h.app.session.doc.feature(sketch).unwrap().kind,
                        FeatureKind::Sketch { plane: PlaneRef::Origin { name }, .. } if name == "XZ"));
                    assert_eq!(h.app.session.undo.len(), undo + 1);
                    let after = h.app.session.doc.to_json();
                    h.app.run("edit.undo", json!({})).unwrap();
                    assert_eq!(h.app.session.doc.to_json(), before);
                    h.app.run("edit.redo", json!({})).unwrap();
                    assert_eq!(h.app.session.doc.to_json(), after);
                } else {
                    assert_eq!(h.app.session.doc.to_json(), before);
                    assert_eq!(h.app.session.undo.len(), undo);
                }
            }
        }
    }

    /// A timeline wider than the window scrolls with a plain mouse wheel (#3356).
    #[test]
    fn the_mouse_wheel_scrolls_a_long_timeline() {
        let mut h = Harness::new();
        for i in 0..80 {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.frames(4);
        let first = crate::scenario::handle_at("timeline:S0").unwrap();
        let last = crate::scenario::handle_at("timeline:S79").unwrap();
        assert!(last.x > 1600.0, "the timeline runs past the 1600-point window: {last:?}");
        h.call("ui.scroll", json!({"x": first.x + 200.0, "y": first.y, "delta": -400.0}));
        h.frames(30);
        let moved = crate::scenario::handle_at("timeline:S0").unwrap();
        assert!(moved.x < first.x - 100.0, "the wheel scrolled the timeline: {first:?} → {moved:?}");
        assert_eq!(moved.y, first.y);
    }

    /// A long timeline paints only the chips in view, and those past the window still keep their
    /// places: scrolled into view, the last one is drawn where its handle says.
    #[test]
    fn a_long_timeline_paints_only_the_chips_in_view() {
        let mut h = Harness::new();
        for i in 0..200 {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.frames(4);
        let width = h.app.shell.history.unwrap().right();
        // Past the window, only the history marker at the end of the strip.
        let past = |h: &Harness| {
            let marker = crate::scenario::rect_of("history:marker").unwrap().expand(2.0);
            let drawn = h.shapes.iter().map(|s| s.shape.visual_bounding_rect()).filter(|r| r.is_finite());
            drawn.filter(|r| r.left() > width && !marker.contains_rect(*r)).count()
        };
        assert!(crate::scenario::handle_at("timeline:S199").unwrap().x > width, "the last chip starts past the window");
        assert_eq!(past(&h), 0, "nothing is painted past the window");
        let found = h.app.session.doc.find_feature("S199").unwrap().id;
        h.app.tree.timeline_reveal = Some(found);
        h.frames(30);
        let last = crate::scenario::handle_at("timeline:S199").unwrap();
        assert!(h.app.shell.history.unwrap().contains(last), "the last chip scrolled into view: {last:?}");
        h.step(&json!({"click": {"handle": "timeline:S199"}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Feature { id: found }], "the drawn chip is the one clicked");

        // Scrolled to the end, the chips out of view on the left still hold their slots.
        let target = crate::scenario::handle_at("timeline:S180").unwrap();
        let marker = crate::scenario::rect_of("history:marker").unwrap().center();
        h.step(&json!({"drag": [[marker.x, marker.y], [target.x - 12.0, target.y]]})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, Some(180), "the marker dropped before S180");
        h.app.run("timeline.roll_to", json!({})).unwrap();
        h.settle();

        // A range picked across chips out of view takes every feature between its ends.
        h.app.tree.timeline_reveal = h.app.session.doc.find_feature("S0").map(|f| f.id);
        h.frames(30);
        h.step(&json!({"click": {"handle": "timeline:S0"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app).len(), 200);
    }

    /// Find in Timeline on the last body of a history longer than the timeline scrolls its
    /// feature into view, in the bottom strip and in the history on the left (#5850).
    #[test]
    fn find_in_timeline_scrolls_both_timelines_to_the_found_feature() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.timeline_left = left;
            for i in 0..80 {
                h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
            }
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Last", "body_name": "Plate"})).unwrap();
            h.app.run("select.clear", json!({})).unwrap();
            h.frames(4);
            let shown = |h: &Harness| {
                let history = h.app.shell.history.unwrap();
                let found = crate::scenario::handle_at("timeline:Last").unwrap();
                history.contains(found)
            };
            assert!(!shown(&h), "left={left}: the last feature starts outside the timeline");

            let target = crate::context_menu::Target::Body { name: "Plate".into() };
            let item = crate::context_menu::items(&h.app, &target).into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
            crate::context_menu::run_item(&mut h.app, &item, egui::pos2(0.0, 0.0));
            h.frames(30);
            let last = h.app.session.doc.find_feature("Last").unwrap().id;
            assert_eq!(h.app.session.selection, [solidcraft_engine::Sel::Feature { id: last }], "left={left}");
            assert!(shown(&h), "left={left}: Find in Timeline scrolled the found feature into the timeline");
            assert!(h.app.tree.timeline_reveal.is_none(), "left={left}: the request is done once drawn");
        }
    }

    /// A face picked in the view lights the feature that made it, the hole rather than the block
    /// it was cut into, and scrolls both timelines to it once; hovering finds the same feature.
    #[test]
    fn a_face_picked_in_the_view_scrolls_both_timelines_to_the_feature_that_made_it() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.timeline_left = left;
            let block =
                h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 10, "name": "Block", "body_name": "Plate"})).unwrap()["feature"]
                    .as_u64()
                    .unwrap();
            for i in 0..80 {
                h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
            }
            let hole = h
                .app
                .run("solid.cylinder", json!({"base": [10, 10, -1], "radius": 3, "height": 12, "operation": "cut", "name": "Hole"}))
                .unwrap()["feature"]
                .as_u64()
                .unwrap();
            h.app.run("select.clear", json!({})).unwrap();
            h.frames(4);
            let shown = |h: &Harness| h.app.shell.history.unwrap().contains(crate::scenario::handle_at("timeline:Hole").unwrap());
            assert!(!shown(&h), "left={left}: the hole starts outside the timeline");

            let state = h.app.session.world_state();
            let body = state.bodies.first().unwrap();
            let names = solidcraft_engine::doc::naming::face_names(body);
            let index = names.iter().position(|name| solidcraft_engine::doc::persistent_name::made_by(name) == Some(hole)).unwrap();
            let plate = body.name.clone();
            h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: plate.clone(), index, point: solidcraft_engine::geom::Vec3::ZERO });
            assert_eq!(crate::timeline::hovered_feature(&h.app), Some(hole), "left={left}: hovering the hole's face finds the hole");
            h.app.viewport.hover = Some(crate::viewport::Hit::Vertex { body: plate.clone(), point: solidcraft_engine::geom::Vec3::ZERO });
            assert_eq!(crate::timeline::hovered_feature(&h.app), Some(block), "left={left}: a vertex finds the feature that made its body");
            h.app.viewport.hover = None;

            h.app.run("select.set", json!({"items": [{"type": "face", "body": plate, "index": index, "point": [0, 0, 0]}]})).unwrap();
            h.frames(30);
            assert_eq!(crate::timeline::picked_feature(&h.app), Some(hole), "left={left}");
            assert_eq!(h.app.tree.view_pick, Some(hole), "left={left}");
            assert!(shown(&h), "left={left}: the pick scrolled the hole into the timeline");
            assert!(h.app.tree.timeline_reveal.is_none(), "left={left}: once, not on every frame");
        }
    }

    #[test]
    fn the_feature_menu_edits_its_profile_sketch_in_both_timelines() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.timeline_left = left;
            h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            h.app.run("solid.extrude", json!({"sketch": "Base", "distance": 10, "operation": "new", "name": "Depth"})).unwrap();
            let sketch = h.app.session.doc.find_feature("Base").map(|f| f.id).unwrap();
            h.frames(4);

            h.step(&json!({"click": {"handle": "timeline:Base"}, "button": "right"})).unwrap();
            assert!(h.widget("Edit Profile Sketch").is_none(), "left: {left}");
            h.step(&json!({"key": "Escape"})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Depth"}, "button": "right"})).unwrap();
            h.step(&json!({"widget": "Edit Profile Sketch"})).unwrap();
            assert_eq!(h.app.session.active_sketch, Some(sketch), "left: {left}");
        }
    }

    fn shown(h: &mut Harness) -> Vec<String> {
        h.frames(4);
        crate::scenario::handles("timeline:").into_iter().map(|(n, _)| n).filter(|n| n != "timeline:components").collect()
    }

    /// Two components with a box each: while one is active, the timeline shows only its
    /// feature; the toggle ahead of the features shows the whole design again, and so does the
    /// root (#2880).
    #[test]
    fn the_timeline_shows_only_the_active_components_features() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Left"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "LeftBox"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "Right"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "name": "RightBox"})).unwrap();
        let names = shown(&mut h);
        assert!(names.contains(&"timeline:RightBox".to_string()) && !names.contains(&"timeline:LeftBox".to_string()), "{names:?}");
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        let names = shown(&mut h);
        assert!(names.contains(&"timeline:LeftBox".to_string()) && names.contains(&"timeline:RightBox".to_string()), "show all: {names:?}");
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        let names = shown(&mut h);
        assert!(!names.contains(&"timeline:LeftBox".to_string()), "this part again: {names:?}");
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        let names = shown(&mut h);
        assert!(names.contains(&"timeline:LeftBox".to_string()) && names.contains(&"timeline:RightBox".to_string()), "root: {names:?}");
    }

    /// The marker and a drop map from the shown features back to the design's positions.
    #[test]
    fn a_slot_among_the_shown_features_is_a_design_position() {
        let shown = [1, 4, 5];
        assert_eq!(super::design_position(&shown, 0, 7), 1);
        assert_eq!(super::design_position(&shown, 2, 7), 5);
        assert_eq!(super::design_position(&shown, 3, 7), 6);
        assert_eq!(super::design_position(&[], 0, 7), 7);
    }

    fn copied(h: &Harness) -> Vec<String> {
        h.app.session.clipboard.iter().map(|f| f.name.clone()).collect()
    }

    /// Ctrl+click picks two features; Copy in the menu copies both; Paste puts copies of both at
    /// the marker. In the bottom timeline and in the history beside the browser.
    #[test]
    fn the_feature_menu_copies_the_selected_features_and_pastes_them() {
        for left in [false, true] {
            let mut h = two_boxes(left);
            h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
            let first = h.app.session.doc.find_feature("First").map(|f| f.id).unwrap();
            let second = h.app.session.doc.find_feature("Second").map(|f| f.id).unwrap();
            assert_eq!(super::selected_feature_ids(&h.app), [first, second], "left: {left}");

            h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
            h.step(&json!({"widget": "Copy"})).unwrap();
            assert_eq!(copied(&h), ["First", "Second"], "left: {left}");

            h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
            h.step(&json!({"widget": "Paste"})).unwrap();
            assert_eq!(h.app.session.doc.features.len(), 4, "left: {left}");
            let bodies: Vec<String> = h.app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect();
            assert_eq!(bodies, ["One", "Two", "One (2)", "Two (2)"], "left: {left}");
        }
    }

    /// A feature with a warning mark offers Copy Warning Message, which copies what its tooltip
    /// says; a feature with nothing to report doesn't offer it. In both timelines.
    /// The strip finds each chip's result in one table: the same result `Model::result` gives,
    /// the first one when an id has two, and none for a feature without one.
    #[test]
    fn each_chip_finds_its_own_result() {
        let mut h = two_boxes(false);
        h.app.run("solid.fillet", json!({"edges": [[5, 0, 10]], "radius": 20, "fit": "reduce", "body": "One"})).unwrap();
        h.frames(2);
        let model = &h.app.session.model;
        let index = super::result_index(&model.results);
        let ids: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
        assert!(ids.len() >= 3 && model.results.iter().any(|r| r.warning.is_some()), "a warned feature among others");
        for id in &ids {
            let found = index.get(id).and_then(|k| model.results.get(*k));
            assert!(found.is_some_and(|r| std::ptr::eq(r, model.result(*id).unwrap())), "feature {id}");
        }
        assert!(!index.contains_key(&u64::MAX), "a feature without a result has none");
        let mut twice = model.results.clone();
        let first = twice.iter().position(|r| r.warning.is_some()).unwrap();
        let mut copy = twice[first].clone();
        copy.warning = None;
        twice.push(copy);
        let index = super::result_index(&twice);
        assert_eq!(index.get(&twice[first].id), Some(&first), "the first result of an id wins, as in Model::result");
    }

    /// A folder's mark is the worst of its features' own, searched for a short folder and found
    /// in tables for a long one: an error over warnings, the message of the first feature with
    /// the worst, none for a feature suppressed or rolled back, and a feature listed twice in the
    /// history found at its first place.
    #[test]
    fn a_folders_mark_is_the_worst_of_its_features_own() {
        use crate::ui_kit::Status;
        let mut h = two_boxes(false);
        let third =
            json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "operation": "new_body", "name": "Third", "body_name": "Three"});
        h.app.run("solid.box", third).unwrap();
        h.frames(2);
        let ids: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
        assert_eq!(ids.len(), 3);
        let (a, b, c) = (ids[0], ids[1], ids[2]);
        for (id, error, warning) in [(a, None, Some("a")), (b, Some("b"), None), (c, None, Some("c"))] {
            let result = h.app.session.model.results.iter_mut().find(|r| r.id == id).unwrap();
            result.error = error.map(str::to_string);
            result.warning = warning.map(str::to_string);
        }
        let own = |h: &Harness, ids: &[u64]| {
            let worst = ids.iter().filter_map(|id| super::feature_status(&h.app, *id).map(|s| (s, *id))).reduce(|a, b| if b.0 > a.0 { b } else { a });
            (worst.map(|w| w.0), worst.and_then(|w| super::feature_message(&h.app, w.1)))
        };
        // Every folder short enough to be searched, then the same repeated past the length that
        // builds the tables.
        let check = |h: &Harness, folder: &[u64], status: Option<Status>, of: Option<u64>, why: &str| {
            for ids in [folder.to_vec(), folder.repeat(16)] {
                let found = super::worst_status(&h.app, &ids);
                assert_eq!(found, (status, of.and_then(|id| super::feature_message(&h.app, id))), "{why}, {} ids", ids.len());
                assert_eq!(found, own(h, &ids), "{why}, {} ids", ids.len());
            }
        };
        check(&h, &ids, Some(Status::Error), Some(b), "the error outweighs the warnings");
        check(&h, &[a, c], Some(Status::Warning), Some(a), "the first warned feature's message");
        check(&h, &[c, a], Some(Status::Warning), Some(c), "the first in the folder, not in the history");
        check(&h, &[], None, None, "an empty folder");
        check(&h, &[u64::MAX], None, None, "an id outside the history");

        let mut twice = h.app.session.doc.features[0].clone();
        twice.suppressed = true;
        std::sync::Arc::make_mut(&mut h.app.session.doc).features.push(twice);
        check(&h, &[a], Some(Status::Warning), Some(a), "a feature listed twice is found at its first place");
        std::sync::Arc::make_mut(&mut h.app.session.doc).features.pop();

        std::sync::Arc::make_mut(&mut h.app.session.doc).marker = Some(1);
        check(&h, &ids, Some(Status::Warning), Some(a), "the features after the marker have no mark");
        std::sync::Arc::make_mut(&mut h.app.session.doc).marker = None;
        std::sync::Arc::make_mut(&mut h.app.session.doc).features[1].suppressed = true;
        check(&h, &ids, Some(Status::Warning), Some(a), "a suppressed feature has none");
    }

    #[test]
    fn the_feature_menu_copies_a_warning_message() {
        for left in [false, true] {
            let mut h = two_boxes(left);
            // Too large for the box: reduced to what fits, which the feature's warning says.
            h.app.run("solid.fillet", json!({"edges": [[5, 0, 10]], "radius": 20, "fit": "reduce", "body": "One"})).unwrap();
            h.frames(4);
            let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
            let name = h.app.session.doc.features.last().map(|f| f.name.clone()).unwrap();
            let warning = h.app.session.model.result(id).and_then(|r| r.warning.clone()).unwrap();
            h.step(&json!({"click": {"handle": "timeline:First"}, "button": "right"})).unwrap();
            assert!(h.widget("Copy Warning Message").is_none(), "left: {left}");
            h.step(&json!({"key": "Escape"})).unwrap();
            h.step(&json!({"click": {"handle": format!("timeline:{name}")}, "button": "right"})).unwrap();
            let [x, y] = h.widget("Copy Warning Message").unwrap();
            let (request, _reply) = crate::control::ControlRequest::new("ui.click", json!({"x": x, "y": y}));
            crate::control::handle(&mut h.app, &h.ctx, &request);
            let mut copied = Vec::new();
            for _ in 0..8 {
                h.frame();
                copied.extend(h.output.commands.iter().filter_map(|c| match c {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                }));
            }
            assert!(copied.iter().any(|t| t.starts_with(&name) && t.contains(&format!("Warning: {warning}"))), "left: {left}: {copied:?}");
        }
    }

    /// Copy on a feature outside the selection copies that feature alone.
    #[test]
    fn copy_on_an_unselected_feature_copies_only_it() {
        let mut h = two_boxes(false);
        h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Copy"})).unwrap();
        assert_eq!(copied(&h), ["Second"]);
        // Ctrl+click again takes a feature back out of the selection.
        h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
        let first = h.app.session.doc.find_feature("First").map(|f| f.id).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), [first]);
    }
    fn three_boxes(left: bool) -> Harness {
        let mut h = two_boxes(left);
        h.app
            .run(
                "solid.box",
                json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "operation": "new_body", "name": "Third", "body_name": "Three"}),
            )
            .unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(4);
        h
    }

    fn names(h: &Harness) -> Vec<String> {
        h.app.session.doc.features.iter().map(|f| f.name.clone()).collect()
    }

    fn ids(h: &Harness, names: &[&str]) -> Vec<u64> {
        names.iter().map(|name| h.app.session.doc.find_feature(name).map(|f| f.id).unwrap()).collect()
    }

    /// Shift+click selects the range from the last feature clicked to this one, either way
    /// round; with nothing clicked yet it starts at the last selected feature (#5005).
    #[test]
    fn shift_click_selects_a_range_of_features() {
        let mut h = three_boxes(false);
        h.step(&json!({"click": {"handle": "timeline:Third"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:First"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["First", "Second", "Third"]));
        h.step(&json!({"click": {"handle": "timeline:Second"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["Second", "Third"]), "the range starts at the anchor, Third");

        h.app.tree.timeline_anchor = None;
        h.app.run("select.set", json!({"items": [{"type": "feature", "id": ids(&h, &["First"])[0]}]})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["First", "Second"]));
    }

    /// Delete in the menu of a selected feature deletes the whole selection in one undo step,
    /// and says how many; on a feature outside the selection it deletes that one (#5005). The
    /// same in both timelines, and the history beside the browser no longer adds its own Delete
    /// Selected Features… to a feature's menu (#8861).
    #[test]
    fn the_feature_menu_deletes_the_selected_features() {
        for left in [false, true] {
            let mut h = three_boxes(left);
            h.step(&json!({"click": {"handle": "timeline:Third"}, "button": "right"})).unwrap();
            assert!(h.widget("Delete").is_some(), "left: {left}");
            h.step(&json!({"key": "Escape"})).unwrap();

            h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
            assert!(h.widget("Delete Selected Features…").is_none(), "left: {left}");
            h.step(&json!({"widget": "Delete 2 Selected Features"})).unwrap();
            h.settle();
            assert!(h.app.dialog.is_none(), "nothing else depends on them");
            assert_eq!(names(&h), ["Third"], "left: {left}");
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(names(&h), ["First", "Second", "Third"], "left: {left}");
        }
    }

    /// The menu of a feature after the history marker deletes every feature after the marker,
    /// and says how many; features before the marker, and a selection of several, don't offer
    /// it. In both timelines (#5005).
    #[test]
    fn a_feature_after_the_marker_deletes_everything_after_the_marker() {
        for left in [false, true] {
            let mut h = three_boxes(left);
            h.app.run("timeline.roll_to", json!({"position": 1})).unwrap();
            h.settle();
            h.step(&json!({"click": {"handle": "timeline:First"}, "button": "right"})).unwrap();
            assert!(h.widget("Delete 2 Features After History Marker").is_none(), "left: {left}");
            h.step(&json!({"key": "Escape"})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Third"}, "ctrl": true})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Third"}, "button": "right"})).unwrap();
            assert!(h.widget("Delete 2 Selected Features").is_some(), "left: {left}");
            assert!(h.widget("Delete 2 Features After History Marker").is_none(), "left: {left}");
            h.step(&json!({"key": "Escape"})).unwrap();
            h.app.run("select.clear", json!({})).unwrap();
            h.frames(2);

            h.step(&json!({"click": {"handle": "timeline:Third"}, "button": "right"})).unwrap();
            h.step(&json!({"widget": "Delete 2 Features After History Marker"})).unwrap();
            h.settle();
            assert_eq!(names(&h), ["First"], "left: {left}");
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(names(&h), ["First", "Second", "Third"], "left: {left}");
        }
    }

    /// With the timeline showing one component's features, the entry counts and deletes that
    /// component's features after the marker, found by their positions in the design: none of
    /// another component's and none before the marker (#8860).
    #[test]
    fn after_the_marker_a_shown_component_deletes_only_its_features() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "C1"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "C2"})).unwrap();
        for (k, name) in ["B1", "B2", "B3", "B4", "B5", "B6"].into_iter().enumerate() {
            h.app.run("component.activate", json!({"component": if k % 2 == 0 { "C1" } else { "C2" }})).unwrap();
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20 * k, 0, 0], "name": name})).unwrap();
        }
        let before = names(&h);
        h.app.run("timeline.roll_to", json!({"feature": "B2"})).unwrap();
        h.app.run("component.activate", json!({"component": "C1"})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.settle();
        h.frames(4);
        h.step(&json!({"click": {"handle": "timeline:B1"}, "button": "right"})).unwrap();
        assert!(h.widget("Delete 2 Features After History Marker").is_none(), "B1 is before the marker");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:B3"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Delete 2 Features After History Marker"})).unwrap();
        h.settle();
        let kept: Vec<String> = before.iter().filter(|n| *n != "B3" && *n != "B5").cloned().collect();
        assert_eq!(names(&h), kept, "B2 is before the marker, B4 and B6 are C2's");
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(names(&h), before);
    }

    /// Deleting a sketch together with the extrude that uses it asks nothing: the extrude is
    /// one of the features deleted, not one that goes with it or fails. The Delete key, with
    /// the features picked in the timeline, does the same (#5005).
    #[test]
    fn deleting_a_feature_with_its_dependents_does_not_ask() {
        let mut h = Harness::new();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch": "Base", "distance": 10, "operation": "new", "name": "Depth"})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(4);
        h.step(&json!({"click": {"handle": "timeline:Base"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Depth"}, "shift": true})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Depth"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Delete 2 Selected Features"})).unwrap();
        h.settle();
        assert!(h.app.dialog.is_none());
        assert!(names(&h).is_empty());

        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(4);
        h.step(&json!({"click": {"handle": "timeline:Base"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Depth"}, "shift": true})).unwrap();
        h.step(&json!({"key": "Delete"})).unwrap();
        h.settle();
        assert!(names(&h).is_empty());
    }

    #[test]
    fn both_strips_keep_selection_reorder_marker_and_transport() {
        use crate::scenario::{handle_at, rect_of};
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = two_boxes(false);
            h.app.ui.layout = mode;
            h.settle();
            let viewport = h.app.viewport.rect.unwrap();
            if mode == crate::shell::LayoutMode::Floating {
                assert_eq!(rect_of("history:card").unwrap().bottom(), viewport.bottom() - crate::shell::SHELL_GAP);
            } else {
                let strip = rect_of("history:strip").unwrap();
                assert_eq!(strip.top(), viewport.bottom());
                assert_eq!(strip.bottom(), h.ctx.content_rect().bottom());
                assert_eq!(strip.left(), 0.0);
                assert_eq!(strip.height(), 40.0);
            }
            h.step(&json!({"click":{"handle":"timeline:First"}})).unwrap();
            h.step(&json!({"click":{"handle":"timeline:Second"},"ctrl":true})).unwrap();
            assert_eq!(super::selected_feature_ids(&h.app).len(), 2);
            let first = handle_at("timeline:First").unwrap();
            let second = handle_at("timeline:Second").unwrap();
            h.step(&json!({"drag":[[second.x,second.y],[first.x-12.0,first.y]]})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.features.first().unwrap().name, "Second");
            let first = handle_at("timeline:Second").unwrap();
            let marker = rect_of("history:marker").unwrap().center();
            h.step(&json!({"drag":[[marker.x,marker.y],[first.x-12.0,first.y]]})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(0));
            h.step(&json!({"click":{"handle":"history:forward"}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(1));
            h.step(&json!({"click":{"handle":"history:last"}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, None);
        }
    }
    /// A feature dropped just past the history marker is rolled back, and dropped just before it
    /// is active again, in both strips.
    #[test]
    fn dragging_a_feature_across_the_marker_rolls_it_back_and_forward() {
        use crate::scenario::{handle_at, rect_of};
        for left in [false, true] {
            let mut h = two_boxes(left);
            h.settle();
            let second = handle_at("timeline:Second").unwrap();
            let marker = rect_of("history:marker").unwrap();
            let behind = if left { [second.x, marker.bottom() + 3.0] } else { [marker.right() + 4.0, second.y] };
            let undo = h.app.session.undo.len();
            h.step(&json!({"drag":[[second.x,second.y],behind]})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(1), "left={left}: dropped behind the marker, Second is rolled back");
            assert_eq!(h.app.session.undo.len(), undo + 1, "left={left}: crossing the marker is one undo step");
            let second = handle_at("timeline:Second").unwrap();
            let marker = rect_of("history:marker").unwrap();
            let before = if left { [second.x, marker.top() - 2.0] } else { [marker.left() - 3.0, second.y] };
            h.step(&json!({"drag":[[second.x,second.y],before]})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, None, "left={left}: dropped before the marker, Second is active again");
            let names: Vec<_> = h.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect();
            assert_eq!(names, ["First", "Second"], "left={left}: crossing the marker keeps the order");
        }
    }

    #[test]
    fn floating_strip_has_only_history_text_and_accents_the_last_done_chip_and_marker() {
        let mut h = two_boxes(false);
        h.app.ui.tab = "SURFACE".into();
        let t = crate::theme::Tokens::get();
        let accent = crate::theme::TabAccent::for_tab(&t, "SURFACE");
        for marker_position in [None, Some(1), Some(0)] {
            h.app.session.doc_mut().marker = marker_position;
            let marker_index = marker_position.unwrap_or(2);
            for selected in [None, Some(0), Some(1)] {
                h.app.session.selection =
                    selected.map(|i| solidcraft_engine::Sel::Feature { id: h.app.session.doc.features[i].id }).into_iter().collect();
                let ctx = crate::fonts::test_context();
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| super::timeline(&mut h.app, ui));
                out.textures_delta.clear();
                let texts: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(text) => Some(text.galley.text()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(texts, ["History"]);
                let chips: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(r) if r.rect.size() == egui::vec2(28.0, 28.0) => Some(r),
                        _ => None,
                    })
                    .collect();
                assert_eq!(chips.len(), 2);
                for (i, chip) in chips.iter().enumerate() {
                    let opacity = if i >= marker_index { 0.42 } else { 1.0 };
                    let (fill, edge) = if selected == Some(i) { (accent.soft, accent.color) } else { (t.timeline_item, t.border) };
                    assert_eq!(chip.fill, fill.gamma_multiply(opacity), "marker={marker_position:?}, selected={selected:?}, chip={i}");
                    assert_eq!(chip.stroke.color, edge.gamma_multiply(opacity));
                    let dot_center = egui::pos2(chip.rect.center().x, chip.rect.bottom() + t.space_3);
                    let has_dot = out.shapes.iter().any(|s| {
                        matches!(&s.shape, egui::Shape::Circle(c)
                        if c.center == dot_center && c.radius == t.space_2 && c.fill == accent.color && c.stroke == egui::Stroke::NONE)
                    });
                    assert_eq!(has_dot, marker_index > 0 && i == marker_index - 1);
                }
                let marker = crate::scenario::rect_of("history:marker").unwrap();
                assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c)
                    if marker.contains(c.center) && c.fill == t.timeline && c.stroke == egui::Stroke::new(t.space_2, accent.color))));
            }
        }
    }
}
