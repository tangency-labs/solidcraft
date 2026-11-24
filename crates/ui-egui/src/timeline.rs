//! The timeline (bottom): features in order, the rollback marker, errors and warnings.

use crate::ui_kit::{Status, StatusTip};
use egui::{Rect, pos2};
use serde_json::{Value, json};
use solidcraft_engine::doc::{
    FeatureKind, FeatureResult, PlaneRef, SketchId,
    construct::{AxisDef, PointDef},
    kernel::{ImportReport, IssueKind},
};

use solidcraft_engine::schema::edit::PlaybackAction;

use crate::SolveApp;
use crate::history_bar;
use crate::ui_kit::Item;
use solidcraft_engine::doc::Document;
use solidcraft_theme::Tokens;
use std::sync::Arc;

/// Apply transport actions from the current marker; endpoints stay bounded, including an
/// empty design. All orientations reach the same rollback command.
pub(crate) fn run_transport(app: &mut SolveApp, control: crate::ui_kit::HistoryTransport) {
    // A step moves from the marker the design has, not the one an open editor rolled to (#10688).
    if app.close_editor_for_history().is_err() {
        return;
    }
    if control == crate::ui_kit::HistoryTransport::Play {
        app.start("timeline.play");
        return;
    }
    let n = app.session.doc.features.len();
    let Some(position) = step_position(app, app.session.doc.marker.unwrap_or(n), control) else {
        // The marker stays, but the click still takes over from playback, as a step does.
        stop_playback(app);
        return;
    };
    let params = if position < n { json!({"position":position}) } else { json!({}) };
    let _ = app.run_heavy("timeline.roll_to", params);
}

/// Where a step control (not Play) takes the marker from `marker`, or `None` when it keeps it.
/// The steps are the features the History shows: in the bottom bar, only those of the component
/// it shows (`timeline_scope`, #11430). A step lands where dropping the marker in that slot would,
/// but past the last shown feature it goes to the end of the design (#15974).
fn step_position(app: &SolveApp, marker: usize, control: crate::ui_kit::HistoryTransport) -> Option<usize> {
    use crate::ui_kit::HistoryTransport;
    let n = app.session.doc.features.len();
    let view = transport_view(app);
    let len = view.len();
    if len == 0 && n > 0 {
        return None;
    }
    let slot = view.slot(marker);
    let slot = match control {
        HistoryTransport::First => 0,
        HistoryTransport::Back => slot.saturating_sub(1),
        HistoryTransport::Play => return None,
        HistoryTransport::Forward => slot.saturating_add(1).min(len),
        HistoryTransport::Last => len,
    };
    let backwards = matches!(control, HistoryTransport::First | HistoryTransport::Back);
    // Past the last feature shown, every later feature belongs to components the bar leaves out:
    // Forward and Last go to the end of the design, so none of them stays rolled back with nothing
    // in the bar saying so (#15974), unless the preference keeps the old stop.
    let to_end = !backwards && slot == len && !app.preferences.history_last_stops_at_component;
    let position = if to_end { n } else { view.index(slot) };
    // From a marker outside the shown features, the nearest shown slot can lie the other way:
    // First and Back never add features, and Forward and Last never roll any back.
    if (backwards && position > marker) || (!backwards && position < marker) { None } else { Some(position) }
}

/// Whether a History control would do anything now: a step that moves the marker, or Play (Stop
/// while History plays) when it can run. Only Play says why not: a dimmed step already says the
/// marker is at that end.
pub(crate) fn transport_effect(app: &SolveApp, control: crate::ui_kit::HistoryTransport) -> Result<(), Option<String>> {
    transport_availability(app, control).map_err(Some)?;
    if control == crate::ui_kit::HistoryTransport::Play {
        return Ok(());
    }
    // An open feature editor has rolled the marker to its feature; a step starts from the
    // design's own marker, which it gets back when the step closes the editor (#10688).
    let doc = app.editing_state.as_ref().map_or(&app.session.doc, |editing| &editing.original.doc);
    let marker = doc.marker.unwrap_or(doc.features.len());
    match step_position(app, marker, control) {
        Some(position) if position != marker => Ok(()),
        _ => Err(None),
    }
}

/// The History folder's menu, with History in the browser: the History controls (first,
/// previous, Play History when shown, next, last), each greyed out while it would do nothing,
/// then the controls' own menu.
pub(crate) fn history_folder_items(app: &SolveApp) -> Vec<Item> {
    let playing = app.timeline_playback.is_some();
    let mut items: Vec<Item> = crate::ui_kit::HistoryTransport::shown(app.preferences.history_play_button)
        .iter()
        .map(|&control| {
            let icon = match control {
                crate::ui_kit::HistoryTransport::Play if playing => "timeline_stop".to_owned(),
                _ => format!("timeline_{}", control.id()),
            };
            let effect = transport_effect(app, control);
            Item::action("ui.historyTransport", control.label(playing, play_labels(app)), &icon)
                .with(json!({ "control": control.id() }))
                .on(effect.is_ok())
                .off_because(effect.err().flatten())
        })
        .collect();
    items.push(Item::sep());
    items.extend(transport_items(app));
    items
}

/// Shared-header actions use the same command availability as History playback.
pub(crate) fn transport_availability(app: &SolveApp, control: crate::ui_kit::HistoryTransport) -> Result<(), String> {
    if control == crate::ui_kit::HistoryTransport::Play {
        if app.timeline_playback.is_some() {
            return Ok(());
        }
        return playback_ready(app);
    }
    let command = solidcraft_engine::find_command("timeline.roll_to").ok_or_else(|| tr!("cmd.timeline_roll_to").to_owned())?;
    let info = command.info(&app.session);
    if info.enabled { Ok(()) } else { Err(info.disabled_reason.unwrap_or_else(|| info.label.to_owned())) }
}

/// The features the History controls step through: those the bottom bar shows (one component's
/// and those inside it while the bar shows one, without the components kept out of the bar), or
/// every feature for the History on the left or in the browser, or when Preferences > Layout
/// turns "History steps by component" off.
fn transport_view(app: &SolveApp) -> crate::timeline_fold::View {
    let doc = &app.session.doc;
    use solidcraft_engine::panels::{SectionLocation, Zone};

    let side_history = app.panel_runtime().is_some_and(|runtime| {
        let layout = runtime.layout().layout();
        layout.sections.iter().any(|section| {
            matches!(section.location, SectionLocation::Dock { zone: Zone::Left | Zone::Right })
                && section.tabs.iter().any(|instance| instance.0 == "history")
        })
    });
    if side_history || app.preferences.history_in_browser || !app.preferences.history_steps_component {
        return crate::timeline_fold::View::of(doc, None, true);
    }
    let within = crate::timeline_scope::shown(app).map(|c| crate::timeline_scope::components_within(app, c));
    crate::timeline_fold::View::of(doc, within.as_ref(), app.tree.history_show_hidden)
}

/// The History controls' menu: shows or hides the Play button, and — when the design marks any
/// component out of the bar — shows or hides those components' entries (#10393); then Clear
/// History, greyed out while the History shows nothing to delete (#15969).
pub(crate) fn transport_items(app: &SolveApp) -> Vec<crate::ui_kit::Item> {
    let show = !app.preferences.history_play_button;
    let label = if show { tr!("ui.history_play_button_show") } else { tr!("ui.history_play_button_hide") };
    let mut items = vec![crate::ui_kit::Item::action("ui.historyPlayButton", label, "timeline_play").with(json!({ "show": show }))];
    if app.session.doc.components.iter().any(|c| c.history_hidden) {
        let show_hidden = !app.tree.history_show_hidden;
        let label = if show_hidden { tr!("ui.history_show_hidden") } else { tr!("ui.history_hide_hidden") };
        items.push(crate::ui_kit::Item::action("ui.historyShowHidden", label, "eye").with(json!({ "show": show_hidden })));
    }
    let (features, groups) = clear_targets(app);
    items.push(crate::ui_kit::Item::sep());
    items.push(crate::ui_kit::Item::action("ui.historyClear", tr!("ui.history_clear"), "delete").on(!features.is_empty() || !groups.is_empty()));
    items
}

/// What Clear History deletes: every feature the History shows, as selecting them all and
/// pressing Delete would. When the History shows the whole design, its timeline folders go too,
/// so no empty folder is left behind; a History that shows one component, or leaves out the
/// components marked out of it, keeps the folders, which can hold features it does not show
/// (#15969). The features are those the bar draws, not the stepping view ([`transport_view`]),
/// which with `history_steps_component` off also lists the components marked out of the bar.
pub(crate) fn clear_targets(app: &SolveApp) -> (Vec<u64>, Vec<u64>) {
    let doc = &app.session.doc;
    let within = crate::timeline_scope::shown(app).map(|c| crate::timeline_scope::components_within(app, c));
    let view = crate::timeline_fold::View::of(doc, within.as_ref(), app.tree.history_show_hidden);
    match view.shown() {
        None => {
            let features = doc.features.iter().map(|f| f.id).collect();
            let groups = doc.timeline_groups.iter().filter(|g| g.parent.is_none()).map(|g| g.id).collect();
            (features, groups)
        }
        Some(shown) => (shown.iter().filter_map(|&i| doc.features.get(i)).map(|f| f.id).collect(), Vec::new()),
    }
}

/// Clear History: delete [`clear_targets`] after the same dependents check as Delete, in one
/// undo step (#15969).
pub(crate) fn clear_history(app: &mut SolveApp) {
    if app.close_editor_for_history().is_err() {
        return;
    }
    let (features, groups) = clear_targets(app);
    if features.is_empty() && groups.is_empty() {
        return;
    }
    delete_folders(app, groups, features);
}

/// The feature that made a face, edge or vertex of `body` (its world name): a face's own creator,
/// as Edit Feature on a face finds it (the hole, not the plate it was cut into), else the feature
/// that made the body.
fn geometry_feature(app: &SolveApp, body: &str, face: Option<usize>) -> Option<u64> {
    // Asked at every frame for a face picked in the view (the timeline follows the pick).
    let world = crate::open_meshing::still_world(app);
    let model_body = world.body(body)?;
    let owner = app.session.doc.feature(model_body.feature);
    // An imported mesh can only highlight its import feature. Looking up a geometric face
    // name here would scan the mesh on the UI thread the first time its face is hovered.
    if model_body.body.triangle_mesh().is_some()
        && owner.is_some_and(|feature| matches!(feature.kind, FeatureKind::Import { .. } | FeatureKind::MeshImport { .. }))
    {
        return Some(model_body.feature);
    }
    face.and_then(|index| crate::context_menu::face_feature(app, model_body, index)).or_else(|| owner.map(|feature| feature.id))
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

/// The feature under the pointer (in the 3D view, Browser, or timeline) or picked in the 3D view.
pub(crate) fn from_view(app: &SolveApp) -> [Option<u64>; 2] {
    [hovered_feature(app).or(app.viewport.hover_feature), picked_feature(app)]
}

/// A new face or edge picked in the view scrolls the history shown to the feature that made it,
/// once per pick; the history stays where it is while the pick does not change.
pub(crate) fn follow_view_pick(app: &mut SolveApp) {
    crate::browser::sync_reveals(app);
    let picked = picked_feature(app);
    if picked == app.tree.view_pick {
        return;
    }
    app.tree.view_pick = picked;
    if app.ui.show_timeline
        && let Some(id) = picked
    {
        crate::browser::request_timeline_reveal(app, id);
    }
}

// Snapshot only the data used by timeline interactions. Cloning FeatureKind here would
// also copy imported mesh vertices and triangles on every frame.
#[derive(Clone, Copy, PartialEq, Eq)]
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
            // Sheet metal drawn from a sketch: the base flange's outline and the contour flange's chain.
            FeatureKind::SheetBase { sketch, .. } | FeatureKind::SheetContour { sketch, .. } => Self::Profile(*sketch),
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

/// The History menus' edit item, label and icon: a sketch says Edit Sketch, as its Browser row
/// does (#14366); both open the same editor.
pub(crate) fn edit_label(kind: TimelineKind) -> (&'static str, &'static str) {
    match kind {
        TimelineKind::Sketch => ("Edit Sketch", "sketch"),
        _ => ("Edit Feature", "edit"),
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
            PointDef::Average { .. } => "c_midpoint",
        },
        FeatureKind::Hole { .. } => "hole",
        FeatureKind::Loft { .. } => "loft",
        FeatureKind::Sweep { .. } => "sweep",
        FeatureKind::Shell { .. } => "shell",
        FeatureKind::Draft { .. } => "draft",
        FeatureKind::Split { .. } => "split",
        FeatureKind::Move { .. } => "move",
        FeatureKind::Import { .. } | FeatureKind::MeshImport { .. } | FeatureKind::MeshConvert { .. } => "import",
        FeatureKind::MeshReduce { .. } => "mesh_reduce",
        FeatureKind::BodyTransfer { .. } => "move",
        FeatureKind::Thread { .. } => "thread",
        FeatureKind::SplitFace { .. } => "split_face",
        FeatureKind::Scale { .. } => "scale",
        FeatureKind::OffsetFace { .. } => "offset_face",
        FeatureKind::DeleteFace { .. } => "delete_face",
        FeatureKind::Remove { .. } => "delete",
        FeatureKind::BoundingSolid { .. } => "bounding_solid",
        FeatureKind::Pipe { .. } => "pipe",
        FeatureKind::FrameEndCap { .. } => "end_cap",
        FeatureKind::FrameMember { .. } => "frame_member",
        FeatureKind::FrameMemberTurn { .. } => "rotate_member",
        FeatureKind::FramePlate { .. } => "frame_plate",
        FeatureKind::Emboss { .. } => "emboss",
        FeatureKind::ReplaceFace { .. } => "replace_face",
        FeatureKind::Align { .. } => "align",
        FeatureKind::Coil { .. } => "coil",
        FeatureKind::SheetPerforate { .. } => "perforate",
        FeatureKind::CellGrid { .. } => "perforate",
        FeatureKind::BoundaryFill { .. } => "boundary_fill",
        FeatureKind::Patch { .. } => "surface_patch",
        FeatureKind::Stitch { .. } => "surface_stitch",
        FeatureKind::Thicken { .. } => "surface_thicken",
        FeatureKind::SurfaceExtend { .. } => "surface_extend",
        FeatureKind::SurfaceTrim { .. } => "surface_trim",
        other => crate::workspace::feature_icon(other).unwrap_or("feature"),
    }
}

/// The icon a History folder takes from its features when Preferences says so: theirs, when every
/// feature in it (and in the folders inside it) has the same icon and it holds no joints.
pub(crate) fn folder_kind_icon(app: &SolveApp, id: u64) -> Option<&'static str> {
    if !app.preferences.history_folder_kind_icon {
        return None;
    }
    let doc = &app.session.doc;
    let mut pending = vec![id];
    let mut seen = std::collections::BTreeSet::new();
    let mut icon = None;
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let group = doc.timeline_groups.iter().find(|group| group.id == id)?;
        if !group.joints.is_empty() {
            return None;
        }
        for feature in &group.features {
            let this = icon_of(&doc.feature(*feature)?.kind);
            if icon.is_some_and(|icon| icon != this) {
                return None;
            }
            icon = Some(this);
        }
        pending.extend(doc.timeline_groups.iter().filter(|child| child.parent == Some(id)).map(|child| child.id));
    }
    icon
}

fn construction_plane_icon(plane: &PlaneRef) -> &'static str {
    match plane {
        PlaneRef::AtAngle { .. } => "plane_angle",
        PlaneRef::Midplane { .. } => "plane_midplane",
        PlaneRef::TwoEdges { .. } => "plane_two_edges",
        PlaneRef::AxisPoint { .. } => "plane_axis_point",
        PlaneRef::ThreePoints { .. } => "plane_three_points",
        PlaneRef::Tangent { .. } => "plane_tangent",
        PlaneRef::AlongPath { .. } => "plane_along_path",
        PlaneRef::Perpendicular { .. } => "plane_perpendicular",
        _ => "plane",
    }
}

/// Scrolls the History bar to `rect` when it `holds` the feature to reveal. The request is done
/// only once the rect is presented in the viewport (an offscreen or hidden bar asks again on the
/// next frame); then it returns true and the caller clears `timeline_reveal`.
fn reveal_once(ui: &egui::Ui, reveal: &mut Option<usize>, holds: bool, rect: Rect, align: egui::Align) -> bool {
    if !holds {
        return false;
    }
    // Choose one target this frame: a group's last feature must not replace its request.
    // The caller keeps the persistent reveal until this target is actually visible.
    *reveal = None;
    crate::semantic_content::scroll_acknowledged_at(ui, rect, align)
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

/// The feature a selection item stands for in the Features list and the History: a picked
/// feature itself, or the construction plane, axis or point the 3D view or the Construction
/// folder picked by name. One mapping for both directions with [`viewport_sel_of_feature`], so
/// the lists, the History and the 3D view agree on a construction feature (#17000).
pub(crate) fn feature_of_sel(doc: &Document, sel: &solidcraft_engine::Sel) -> Option<u64> {
    use solidcraft_engine::Sel;
    match sel {
        Sel::Feature { id } => Some(*id),
        Sel::Plane { .. } | Sel::Axis { .. } | Sel::ConstructionPoint { .. } => {
            doc.features.iter().find(|f| f.kind.is_construction() && solidcraft_engine::browser_find::construction_sel(f) == *sel).map(|f| f.id)
        }
        _ => None,
    }
}

/// What the 3D view lights for a selected construction feature: the plane, axis or point
/// itself, picked by name. `None` for any other feature.
pub(crate) fn viewport_sel_of_feature(doc: &Document, id: u64) -> Option<solidcraft_engine::Sel> {
    doc.feature(id).filter(|f| f.kind.is_construction()).map(solidcraft_engine::browser_find::construction_sel)
}

/// The selected timeline features, in timeline order.
pub(crate) fn selected_feature_ids(app: &SolveApp) -> Vec<u64> {
    let doc = &app.session.doc;
    let picked: std::collections::HashSet<u64> = app.session.selection.iter().filter_map(|selection| feature_of_sel(doc, selection)).collect();
    doc.features.iter().filter(|feature| picked.contains(&feature.id)).map(|feature| feature.id).collect()
}

/// What a feature's menu acts on: the selected features when the right-clicked one is among
/// them, else that feature alone. A right-click outside the selection selects that feature
/// first, so in the app these are the selected features.
pub(crate) fn menu_targets(app: &SolveApp, id: u64) -> Vec<u64> {
    let selected = selected_feature_ids(app);
    if selected.contains(&id) { selected } else { vec![id] }
}

/// How a feature's menu treats several selected features, in the History bar, the vertical
/// History and the Browser: an entry acts on all of them or cannot be used. Off in
/// Preferences › Layout gives the menus as they were, where some entries act on the
/// right-clicked feature alone.
pub(crate) struct MenuRule {
    /// The menu acts on several selected features under the rule.
    pub many: bool,
    /// Why an entry that takes one feature (Edit Feature, Rename, Roll back) is off, when it is.
    one_off: Option<String>,
}

/// The [`MenuRule`] of the menu opened on feature `id`.
pub(crate) fn menu_rule(app: &SolveApp, id: u64) -> MenuRule {
    let many = app.preferences.feature_menus_act_on_selection && menu_targets(app, id).len() > 1;
    MenuRule { many, one_off: many.then(|| tr!("timeline.select_one_feature").to_string()) }
}

impl MenuRule {
    /// An entry that takes one feature: off with the reason while the menu acts on several. It
    /// stays in the menu, so the entries below keep their places when the selection grows.
    pub(crate) fn one(&self, item: Item) -> Item {
        item.off_because(self.one_off.clone())
    }

    /// The same entry as a row of the vertical History's menu: `row` draws it, enabled or not
    /// as it is told, and the reason goes on its hover.
    pub(crate) fn one_row(&self, row: impl FnOnce(bool) -> egui::Response) -> egui::Response {
        use crate::ui_kit::BackingTooltipExt as _;
        let row = row(self.one_off.is_none());
        match &self.one_off {
            Some(why) => row.backing_on_hover_text(why.as_str()),
            None => row,
        }
    }
}

/// Delete's label in a feature's menu: how many features it deletes when that is more than one.
pub(crate) fn delete_label(count: usize) -> String {
    match count {
        1 => solidcraft_ui_text::tr_label("Delete").to_string(),
        n => tr!("timeline.delete_many", count = &n.to_string()),
    }
}

/// Delete in a feature's menu, in both timelines: the selected features when the right-clicked
/// one is among them, labelled with how many, else that feature alone (#5005).
pub(crate) fn delete_button(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) {
    let targets = menu_targets(app, id);
    if crate::ui_kit::menu::button(ui, &delete_label(targets.len()), "delete").clicked() {
        delete_features(app, targets);
        ui.close();
    }
}

/// Suppress in a feature's menu, in both timelines and the Browser, when it acts on several
/// selected features: its label, saying how many, and `timeline.suppress`'s parameters, one undo
/// step for them all. It unsuppresses them when all of them are suppressed, else suppresses them
/// all (#16245). `None` when the menu acts on one feature. Under the [`MenuRule`] it takes the
/// place of the feature's own Suppress entry; with the rule off it is an extra entry beside it
/// in both timelines.
pub(crate) fn suppress_selected(app: &SolveApp, id: u64) -> Option<(String, Value)> {
    let targets = menu_targets(app, id);
    if targets.len() < 2 {
        return None;
    }
    let all_suppressed = targets.iter().all(|id| app.session.doc.feature(*id).is_some_and(|f| f.suppressed));
    let key = if all_suppressed { "timeline.unsuppress_many" } else { "timeline.suppress_many" };
    let label = solidcraft_ui_text::format(key, &[("count", &targets.len().to_string())]);
    Some((label, json!({ "features": targets, "suppressed": !all_suppressed })))
}

/// The features from design position `from` to the end, only the `shown` positions (ascending)
/// when the timeline shows some of the design's features.
fn features_from(app: &SolveApp, from: usize, shown: Option<&[usize]>) -> Vec<u64> {
    let features = app.session.doc.features.iter().enumerate().skip(from);
    features.filter(|(i, _)| shown.is_none_or(|shown| shown.binary_search(i).is_ok())).map(|(_, f)| f.id).collect()
}

/// In the menu of a feature after the history marker (feature `id` at design position
/// `index`, `marker` a design position too): every feature after the marker, only the `shown`
/// ones when the timeline is filtered to a component, with the entry's label, for one delete
/// through [`delete_features`] so the confirmation still lists what would fail. Not offered
/// before the marker, nor when the menu acts on several selected features: an entry that acts
/// partly on the selection and partly beyond it is hard to read.
fn after_marker(app: &SolveApp, id: u64, index: usize, marker: usize, shown: Option<&[usize]>) -> Option<(String, Vec<u64>)> {
    if index < marker || menu_targets(app, id).len() > 1 {
        return None;
    }
    let ids = features_from(app, marker, shown);
    // A feature the bar's filter leaves out has nothing shown after the marker to delete.
    if ids.is_empty() {
        return None;
    }
    let key = if ids.len() == 1 { "timeline.delete_after_marker_one" } else { "timeline.delete_after_marker_other" };
    Some((solidcraft_ui_text::format(key, &[("count", &ids.len().to_string())]), ids))
}

/// The [`after_marker`] entry as a button.
pub(crate) fn delete_after_marker_button(app: &mut SolveApp, ui: &mut egui::Ui, id: u64, index: usize, marker: usize, shown: Option<&[usize]>) {
    let Some((label, ids)) = after_marker(app, id, index, marker, shown) else { return };
    if crate::ui_kit::menu::button(ui, &label, "delete").clicked() {
        delete_features(app, ids);
        ui.close();
    }
}

/// A History bar feature's menu (`Target::Timeline`): edit, rename, group, suppress, roll back,
/// find, the clipboard and delete.
pub(crate) fn feature_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let doc = &app.session.doc;
    let (Some(feature), Some(index)) = (doc.feature(id), doc.feature_index(id)) else { return Vec::new() };
    let (edit, icon) = edit_label(TimelineKind::of(&feature.kind));
    let rule = menu_rule(app, id);
    let mut v = vec![rule.one(Item::action("ui.editFeature", edit, icon).with(json!({ "feature": id })))];
    if matches!(feature.kind, FeatureKind::Sketch { .. }) {
        let redefine =
            crate::context_menu::items(app, &crate::context_menu::Target::Sketch { id }).into_iter().filter(|item| item.id == "ui.redefineSketch");
        // The sketch's own menu already has the rule on it.
        v.extend(redefine);
    }
    if let Some(sketch) = TimelineKind::of(&feature.kind).profile_sketch() {
        v.push(rule.one(Item::action("ui.editSketch", "Edit Profile Sketch", "sketch").with(json!({ "sketch": sketch.0 }))));
    }
    // Its own message, not a literal: no catalog held the literal, so the item stayed English
    // in every language (#8301).
    let suppress = if feature.suppressed { tr!("timeline.unsuppress_features") } else { tr!("timeline.suppress_features") };
    let targets = menu_targets(app, id);
    let parent = crate::timeline_folders::group_parent(&doc.timeline_groups, &targets);
    v.push(rule.one(Item::action("ui.rename", "Rename", "rename").with(json!({ "feature": id, "dialog": true }))));
    // A new folder of the selected features, or of this one when it isn't selected.
    v.push(
        Item::action("ui.groupFeatures", "Group Features", "feature_group")
            .with(json!({ "features": targets, "parent": parent }))
            .off_because(app.session.group_refusal(&targets, parent)),
    );
    // On several selected features the entry for them all takes this one's place. With the
    // rule off it is an extra entry, and this one still acts on this feature alone.
    if !rule.many {
        v.push(Item::action("timeline.suppress", solidcraft_ui_text::tr_by_id(suppress), "suppress").with(json!({ "feature": id })));
    }
    if let Some((label, params)) = suppress_selected(app, id) {
        v.push(Item::action("timeline.suppress", &label, "suppress").with(params));
    }
    v.push(rule.one(Item::action("timeline.roll_to", "Roll History Marker Here", "rollback").with(json!({ "feature": id }))));
    // What every selected feature made, under the rule.
    let find = if rule.many { json!({ "features": targets }) } else { json!({ "feature": id }) };
    v.extend([
        Item::action("ui.findBrowser", "Find in Browser", "list").with(find),
        Item::sep(),
        Item::action("timeline.copy", "Copy", "copy").with(json!({ "features": targets })),
        Item::action("timeline.paste", "Paste", "paste").with(json!({})).on_if(!app.session.clipboard.is_empty()),
    ]);
    // One feature's message: not offered for several.
    if !rule.many && feature_message(app, id).is_some() {
        let failed = app.session.model.result(id).is_some_and(|r| r.error.is_some());
        let label = if failed { "Copy Error Message" } else { "Copy Warning Message" };
        v.push(Item::action("ui.copyMessage", label, "copy").with(json!({ "feature": id })));
    }
    v.push(Item::sep());
    v.push(Item::action("ui.deleteFeatures", &delete_label(targets.len()), "delete").with(json!({ "features": targets })));
    let marker = doc.marker.unwrap_or(doc.features.len());
    // The features the bar shows (`timeline_scope`), as it draws them.
    let within = crate::timeline_scope::shown(app).map(|c| crate::timeline_scope::components_within(app, c));
    let view = crate::timeline_fold::View::of(doc, within.as_ref(), app.tree.history_show_hidden);
    if let Some((label, ids)) = after_marker(app, id, index, marker, view.shown()) {
        // Its own id, so `ui.menuPick` can tell it from Delete.
        v.push(Item::action("ui.deleteAfterMarker", &label, "delete").with(json!({ "features": ids })));
    }
    v
}

/// The timeline anchor of a feature: the last one clicked, where a Shift+click range starts.
pub(crate) fn anchor_key(id: u64) -> String {
    format!("f:{id}")
}

/// Copy and Paste in a feature's timeline menu. Paste puts the copies at the history marker.
pub(crate) fn clipboard_buttons(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) {
    if crate::ui_kit::menu::button(ui, solidcraft_ui_text::tr_label("Copy"), "copy").clicked() {
        let features = menu_targets(app, id);
        let _ = app.run("timeline.copy", json!({ "features": features }));
        ui.close();
    }
    if crate::ui_kit::menu::button_enabled(ui, solidcraft_ui_text::tr_label("Paste"), "paste", !app.session.clipboard.is_empty()).clicked() {
        let _ = app.run("timeline.paste", json!({}));
        ui.close();
    }
}

/// A feature tooltip's (and its copied message's) name, type, error and warning: an import's
/// summary as it is (the mark already says it is a warning), any other warning labelled.
pub(crate) fn feature_status_tip(name: &str, ty: &str, result: Option<&FeatureResult>) -> StatusTip {
    let error = result.and_then(|r| r.error.as_deref());
    match result.and_then(|r| r.import_report.as_deref()).filter(|r| !r.issues.is_empty()) {
        Some(report) => StatusTip { warning: Some(import_summary(report)), ..status_tip(name, ty, error, None) },
        None => status_tip(name, ty, error, result.and_then(|r| r.warning.as_deref())),
    }
}

/// Where each feature's result is in `results`, by feature id: the first result of an id, as
/// `Model::result` finds it.
pub(crate) fn result_index(results: &[solidcraft_engine::doc::FeatureResult]) -> std::collections::HashMap<u64, usize> {
    results.iter().enumerate().rev().map(|(k, r)| (r.id, k)).collect()
}

/// The common start of a feature tooltip and its copied status message: its name and type,
/// then its error and its warning, each labelled.
pub(crate) fn status_tip(name: &str, ty: &str, error: Option<&str>, warning: Option<&str>) -> StatusTip {
    let line = |key: &str, message: &str| solidcraft_ui_text::format(key, &[("message", &crate::i18n::tr_diagnostic(message))]);
    StatusTip {
        head: format!("{name}  ({ty})"),
        error: error.map(|error| line("timeline.feature_error", error)),
        warning: warning.map(|warning| line("timeline.feature_warning", warning)),
        tail: String::new(),
    }
}

/// A feature's tooltip in the bottom and the left History: its name and type, its error and
/// warning, then why a rolled-back feature isn't shown, a sketch's state, the time it took and
/// the edit hint. Both Histories show this one, so the two stay identical (#14027).
pub(crate) fn feature_tooltip(
    name: &str,
    ty: &str,
    result: Option<&FeatureResult>,
    rolled: bool,
    sketch_state: Option<crate::sketch_state::SketchState>,
) -> StatusTip {
    // The type as a user reads it, translated, without the programmatic `Feature` suffix: a
    // user sees "Fillet", not "FilletFeature".
    let kind = solidcraft_ui_text::tr_label(ty.strip_suffix("Feature").unwrap_or(ty));
    let mut lines = Vec::new();
    // Greyed out says only "not shown": say why (#8100).
    if rolled {
        lines.push(tr!("timeline.feature_rolled_back").to_string());
    }
    lines.extend(sketch_state.map(|state| state.text()));
    lines.extend(result.map(|r| format!("{:.1} ms", r.ms)));
    lines.push(tr!("timeline.feature_hint").to_string());
    StatusTip { tail: lines.join("\n"), ..feature_status_tip(name, kind, result) }
}

/// A feature's mark: an error, else a warning; none while it is suppressed or rolled back, as
/// it isn't built then.
pub(crate) fn feature_status(app: &SolveApp, id: u64) -> Option<Status> {
    let index = app.session.doc.feature_index(id)?;
    status_at(app, index, || app.session.model.result(id))
}

/// The mark of the feature at `index` in the history, given how to find its result.
pub(crate) fn status_at<'a>(app: &SolveApp, index: usize, result: impl FnOnce() -> Option<&'a FeatureResult>) -> Option<Status> {
    let feature = app.session.doc.features.get(index)?;
    if feature.suppressed || app.session.doc.marker.is_some_and(|marker| index >= marker) {
        return None;
    }
    let result = result()?;
    if result.error.is_some() { Some(Status::Error) } else { result.warning.as_ref().map(|_| Status::Warning) }
}

/// The worst mark among `ids`, with the message of the first feature that has it.
pub(crate) fn worst_status(app: &SolveApp, ids: &[u64]) -> (Option<Status>, Option<StatusTip>) {
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
        Some((status, id)) => (Some(status), feature_tip(app, id)),
        None => (None, None),
    }
}

/// What a feature's error or warning mark says, as its tooltip shows it (name and type, then
/// the error and the warning), and an import's every warning as the kernel words them. `None`
/// when it has neither.
pub(crate) fn feature_tip(app: &SolveApp, id: u64) -> Option<StatusTip> {
    let result = app.session.model.result(id)?;
    if result.error.is_none() && result.warning.is_none() {
        return None;
    }
    let feature = app.session.doc.features.iter().find(|f| f.id == id)?;
    let mut tip = feature_status_tip(&feature.name, feature.kind.type_name(), Some(result));
    if let Some(report) = result.import_report.as_ref().filter(|r| !r.issues.is_empty()) {
        // A blank line before the details.
        tip.tail.push('\n');
        tip.tail.push_str(tr!("import_report.details"));
        for issue in &report.issues {
            tip.tail.push('\n');
            tip.tail.push_str(&issue.detail);
        }
    }
    Some(tip)
}

/// [`feature_tip`] as plain text, for the clipboard.
pub(crate) fn feature_message(app: &SolveApp, id: u64) -> Option<String> {
    feature_tip(app, id).map(|tip| tip.plain())
}

/// What a feature's warning mark says: an import's warnings summed up in words, else the
/// feature's warning.
pub(crate) fn warning_text(result: &FeatureResult) -> Option<String> {
    match result.import_report.as_deref().filter(|r| !r.issues.is_empty()) {
        Some(report) => Some(import_summary(report)),
        None => result.warning.clone(),
    }
}

/// Whether a feature has a warning mark, as [`warning_text`] would say, without writing it:
/// the bar asks this of every chip every frame.
pub(crate) fn has_warning(result: &FeatureResult) -> bool {
    result.import_report.as_deref().is_some_and(|r| !r.issues.is_empty()) || result.warning.is_some()
}

/// The status line after an open or an import with warnings: their summary. The import made
/// the reply's `feature`; a STEP file opened as a design is the design's one import. `None`
/// without an import's warnings.
pub(crate) fn import_status(app: &SolveApp, reply: &Value) -> Option<String> {
    let reported = |r: &&FeatureResult| r.import_report.as_deref().is_some_and(|r| !r.issues.is_empty());
    let result = match reply.get("feature").and_then(Value::as_u64) {
        Some(id) => app.session.model.result(id).filter(reported)?,
        // Only when the open says it has warnings: a design opened with an import in it does not.
        None if reply.get("warnings").and_then(Value::as_array).is_some_and(|w| !w.is_empty()) => {
            let mut imports = app.session.model.results.iter().filter(reported);
            match (imports.next(), imports.next()) {
                (Some(only), None) => only,
                _ => return None,
            }
        }
        None => return None,
    };
    result.import_report.as_deref().map(import_summary)
}

/// The status line after an open or an import that warns: the import's summary
/// ([`import_status`]), else how many warnings the reply has and the first of them. `None` without
/// warnings. The desktop's open, the web's open and an insert all say it the same way.
pub(crate) fn import_warning_status(app: &SolveApp, reply: &Value) -> Option<String> {
    import_status(app, reply).or_else(|| {
        let w = reply.get("warnings").and_then(Value::as_array).filter(|w| !w.is_empty())?;
        Some(format!("imported with {} warning(s): {}", w.len(), w.first().and_then(Value::as_str).unwrap_or("")))
    })
}

/// SMLib's check of an import's parts runs on after the design shows (#17697). When it ends,
/// what it found joins the import's result, so its History chip warns, and a part to repair is
/// said on the status line, once per import, in history order.
///
/// A running check asks for no frames: it wakes the window once when it ends, for the frame its
/// result needs. (Asking every frame it runs kept an untouched window redrawing at 4 Hz for the
/// minutes a large import's parts take to check.)
pub(crate) fn tick_import_checks(app: &mut SolveApp, ctx: &egui::Context) {
    // Asked before the checks are settled: a check that ends between the two calls is woken for
    // (or wakes at once), never left for the next frame nobody asks for.
    if app.session.model.checking_imports() {
        let ctx = ctx.clone();
        app.session.model.on_import_checks_end(std::sync::Arc::new(move || ctx.request_repaint()));
    }
    // Parts the checks healed: the import is its healed bodies from now on.
    app.session.settle_import_checks();
    for id in app.session.model.take_import_notices() {
        if let Some(summary) = app.session.model.result(id).and_then(|r| r.import_report.as_deref()).map(import_summary) {
            app.set_status(summary, false);
        }
    }
}

/// An import's warnings for a person who uses CAD: how many of the file's parts came in as
/// solids, then what happened to the others, by kind, counted by part (faces for the faces left
/// out or not drawn), and what may help. Entity numbers and the kernel's wording stay in the
/// details ([`feature_message`]).
pub(crate) fn import_summary(report: &ImportReport) -> String {
    let mut parts: std::collections::BTreeMap<IssueKind, std::collections::BTreeSet<Option<u64>>> = Default::default();
    let mut faces: std::collections::BTreeMap<IssueKind, usize> = Default::default();
    let mut file = 0usize;
    for issue in &report.issues {
        if issue.kind == IssueKind::File {
            file += 1;
            continue;
        }
        parts.entry(issue.kind).or_default().insert(issue.item);
        *faces.entry(issue.kind).or_default() += issue.faces;
    }
    let parts_of = |kind| parts.get(&kind).map_or(0, std::collections::BTreeSet::len);
    let faces_of = |kind| faces.get(&kind).copied().unwrap_or(0);
    let counted =
        |key: &str, n: usize| solidcraft_ui_text::format(&format!("{key}_{}", if n == 1 { "one" } else { "other" }), &[("count", &n.to_string())]);
    let mut out = Vec::new();
    if report.parts > 1 && report.solids < report.parts {
        out.push(tr!("import_report.solids", solids = &report.solids.to_string(), parts = &report.parts.to_string()));
    }
    let sentences = [
        ("import_report.not_imported", parts_of(IssueKind::NotImported)),
        ("import_report.refused", parts_of(IssueKind::Refused)),
        ("import_report.surfaces", parts_of(IssueKind::ImportedAsSurfaces)),
        ("import_report.left_out", faces_of(IssueKind::FaceLeftOut)),
        ("import_report.open", parts_of(IssueKind::OpenBody)),
        ("import_report.not_shown", faces_of(IssueKind::FacesNotShown)),
        ("import_report.needs_repair", parts_of(IssueKind::NeedsRepair)),
        ("import_report.repaired", parts_of(IssueKind::Repaired)),
        ("import_report.file", file),
    ];
    out.extend(sentences.iter().filter(|(_, n)| *n > 0).map(|(key, n)| counted(key, *n)));
    let lost = [IssueKind::NotImported, IssueKind::Refused, IssueKind::ImportedAsSurfaces, IssueKind::FaceLeftOut, IssueKind::OpenBody];
    if lost.iter().any(|k| parts.contains_key(k)) {
        out.push(tr!("import_report.hint").to_string());
    }
    out.join(" ")
}

/// Copy Error Message (or Copy Warning Message) in the menu of a feature that has one. It is
/// one feature's message, so the [`MenuRule`] leaves it out for several selected features.
pub(crate) fn message_button(app: &SolveApp, ui: &mut egui::Ui, id: u64) {
    if menu_rule(app, id).many {
        return;
    }
    let Some(text) = feature_message(app, id) else { return };
    let failed = app.session.model.result(id).is_some_and(|r| r.error.is_some());
    let label = if failed { "Copy Error Message" } else { "Copy Warning Message" };
    if crate::ui_kit::menu::button(ui, solidcraft_ui_text::tr_label(label), "copy").clicked() {
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
    use crate::ui_kit::BackingTooltipExt as _;
    // The item says why it is off: it takes one sketch, so it is off for several selected features.
    let response = crate::ui_kit::menu::button_enabled(ui, &item.label, &item.icon, item.enabled);
    let response = match &item.tip {
        Some(tip) => response.backing_on_hover_text(tip.as_str()),
        None => response,
    };
    if response.clicked() {
        crate::context_menu::run_item(app, &item, response.rect.center());
        ui.close();
    }
}

/// Shift+click on a feature: select the features from the anchor (the last feature clicked, or
/// the last selected one) to this one. `order` is the timeline as drawn this frame, taken from
/// the list it was drawn from so the range never mixes two orders. Ctrl as well adds the range to
/// the selection.
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

/// Give a History or Browser feature click to a compatible active dialog input in any layout.
/// Plain clicks keep an existing pick; Ctrl/Command clicks toggle it off.
pub(crate) fn pick_feature_into_dialog(app: &mut SolveApp, id: u64, modifiers: egui::Modifiers) -> bool {
    // A pattern or Mirror says why it does not take a sketch (#10057).
    if app.dialog.as_mut().is_some_and(|d| d.refuse_sketch(&app.session, id)) {
        return true;
    }
    // The chat's picker takes the step into Features whichever list is active (#11318).
    if let Some(d) = app.dialog.as_mut() {
        d.activate_input_taking(crate::selection::FEATURES);
    }
    let Some(d) = app.dialog.as_mut().filter(|d| d.active_input().is_some_and(|i| i.accept & crate::selection::FEATURES != 0)) else {
        return false;
    };
    let feature = solidcraft_engine::Sel::Feature { id };
    // The chat's picker takes a picked step out on a plain click too, as the model view does.
    let toggle = modifiers.command || modifiers.ctrl || matches!(d.kind, crate::dialogs::Kind::ChatContext);
    if toggle || !d.active_input().is_some_and(|i| i.items.contains(&feature)) {
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
        || app.file_load_holds()
        || app.file_picker.is_some()
        || app.saving.is_some()
        || app.applying.is_some()
        || app.delete_check.is_some()
        || !app.deferred_queue.is_empty()
}

fn playback_ready(app: &SolveApp) -> Result<(), String> {
    if app.computing_since().is_some() || playback_blocked(app) {
        return Err(tr!("ui.history_play_finish_action").into());
    }
    let n = app.session.doc.features.len();
    if app.session.doc.marker.unwrap_or(n) >= n && !play_starts_over(app) {
        return Err(tr!("ui.history_play_roll_back").into());
    }
    Ok(())
}

/// Whether Play History starts over: the marker is at the end of a History that has features
/// (#10798).
fn play_starts_over(app: &SolveApp) -> bool {
    let n = app.session.doc.features.len();
    n > 0 && app.session.doc.marker.unwrap_or(n) >= n
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
    if play_starts_over(app) {
        // Back to the start first, shown like any step; the next step shows the first feature
        // (#10798).
        let revision = app.session.revision;
        app.run_heavy("timeline.roll_to", json!({"position": 0}))?;
        app.timeline_playback = Some(Playback { revision, position: 0, waiting: true, next_at: 0.0 });
        return Ok(());
    }
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

/// A folder's Edit Holes item, when it holds holes only: one Hole dialog for all of them.
pub(crate) fn edit_holes_item(app: &mut SolveApp, ui: &mut egui::Ui, folder: u64) {
    if crate::hole_folder::holes(app, folder).is_none() {
        return;
    }
    if crate::ui_kit::menu::button(ui, "Edit Holes", "hole").clicked() {
        if let Err(why) = crate::hole_folder::open(app, folder) {
            app.set_status(why, true);
        }
        ui.close();
    }
}

/// Where the bottom History strip publishes its marker and feature chips.
const BAR_HANDLES: history_bar::Handles<'static> = history_bar::Handles { marker: "history:marker", step: "timeline:" };

/// A feature as the bar draws it: id, name, icon, suppressed, type name, kind.
type BarFeature = (u64, String, &'static str, bool, &'static str, TimelineKind);

/// What the bar reads from one feature: id, name, icon, suppressed, type name and kind. The rows
/// are built from it and checked against it, so the check compares exactly what the build reads.
fn bar_feature(f: &solidcraft_engine::doc::Feature) -> (u64, &str, &'static str, bool, &'static str, TimelineKind) {
    (f.id, f.name.as_str(), icon_of(&f.kind), f.suppressed, f.kind.type_name(), TimelineKind::of(&f.kind))
}

/// What tells the bar that the document may have changed since its rows were last checked: the
/// same kind of key as the vertical History's row plan (`PlanKey`), without the marker and the
/// group count, which these rows do not depend on. The design changes through commands, which
/// count the session's revision up; the document's address and the feature count guard edits made
/// in place without one. The address is a plain number, never a pointer that is held: a weak
/// pointer here would make `Session::doc_mut` (`Arc::make_mut`) move the document to a new address
/// instead of editing it in place, which is why the engine drops its own weak caches before an
/// edit (#7502).
#[derive(Clone, Copy, PartialEq, Eq)]
struct BarSeen {
    /// Another design in the same window.
    session: u64,
    revision: u64,
    document: usize,
    features: usize,
}

impl BarSeen {
    fn of(app: &SolveApp) -> Self {
        let session = &app.session;
        Self { session: session.uid, revision: session.revision, document: Arc::as_ptr(&session.doc) as usize, features: session.doc.features.len() }
    }
}

/// The bar's features as the design alone gives them, kept in the egui context with the key they
/// were last checked under ([`BarSeen`]): a long history copied every feature's name, and wrote
/// every step's handle, on every frame. A frame with the same key takes them as they are. When the
/// key changes they are checked against the document's features ([`BarRows::describes`]) and built
/// again only if a feature changed: a counted-up revision (a selection) or a moved document (every
/// frame of a sketch drag) does not mean one did. Nothing here depends on the theme, the results,
/// the selection, the marker or the folds.
struct BarRows {
    features: Vec<BarFeature>,
    /// Each feature's handle: `BAR_HANDLES.step` followed by its name.
    handles: Vec<Arc<str>>,
    /// Each feature's icon accent, which its type name and icon give
    /// (`workspace::feature_icon_accent`).
    accents: Vec<Option<&'static str>>,
    /// No feature id repeats, so results that line up with the features need no table
    /// ([`BarResults`]).
    unique_ids: bool,
}

#[cfg(test)]
thread_local! {
    /// How many times the bar's rows were built.
    static BAR_ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many times the kept rows were checked against the document, the key having changed.
    static BAR_ROWS_CHECKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl BarRows {
    /// The rows for the design as it is: the kept ones under the same key; under another, the
    /// kept ones again (now under the new key) while they still describe its features.
    fn cached(app: &SolveApp, ui: &egui::Ui) -> Arc<Self> {
        let id = egui::Id::unique("sc_history_bar_rows");
        let seen = BarSeen::of(app);
        let kept = ui.data(|data| data.get_temp::<(BarSeen, Arc<Self>)>(id));
        if let Some((was, rows)) = &kept
            && *was == seen
        {
            return rows.clone();
        }
        // The key changed: the kept rows still serve if no feature did.
        let held = kept.map(|(_, rows)| rows).filter(|rows| {
            #[cfg(test)]
            BAR_ROWS_CHECKED.with(|checked| checked.set(checked.get() + 1));
            rows.describes(&app.session.doc)
        });
        let rows = held.unwrap_or_else(|| Arc::new(Self::of(app)));
        ui.data_mut(|data| data.insert_temp(id, (seen, rows.clone())));
        rows
    }

    /// Build the rows from the design.
    fn of(app: &SolveApp) -> Self {
        #[cfg(test)]
        BAR_ROWS_BUILT.with(|built| built.set(built.get() + 1));
        let features: Vec<BarFeature> = app
            .session
            .doc
            .features
            .iter()
            .map(|f| {
                let (id, name, icon, suppressed, ty, kind) = bar_feature(f);
                (id, name.to_string(), icon, suppressed, ty, kind)
            })
            .collect();
        let handles = features.iter().map(|f| Arc::from(format!("{}{}", BAR_HANDLES.step, f.1))).collect();
        let accents = features.iter().map(|f| crate::workspace::feature_icon_accent(f.4, f.2)).collect();
        let mut seen = std::collections::HashSet::with_capacity(features.len());
        let unique_ids = features.iter().all(|f| seen.insert(f.0));
        Self { features, handles, accents, unique_ids }
    }

    /// Whether these rows are the ones [`BarRows::of`] would build from `doc` now, for a frame whose
    /// key changed. Nothing is allocated. The handles, accents and `unique_ids` follow from the six values compared
    /// (`feature_icon_accent` reads only its two arguments), so they need no check of their own.
    fn describes(&self, doc: &Document) -> bool {
        self.features.len() == doc.features.len()
            && self.features.iter().zip(&doc.features).all(|(row, f)| (row.0, row.1.as_str(), row.2, row.3, row.4, row.5) == bar_feature(f))
    }
}

/// How the bar finds each feature's result this frame: the first result with its id, as
/// [`result_index`] and `Model::result` find it.
enum BarResults {
    /// One result per feature, with its id, at its place, and no id twice: a feature's result is
    /// the one at its own place, with no table to build.
    InPlace,
    /// Otherwise a table built once for the strip: a search per chip made a long timeline cost
    /// the square of its length every frame.
    Indexed(std::collections::HashMap<u64, usize>),
}

impl BarResults {
    fn of(features: &[BarFeature], unique_ids: bool, results: &[FeatureResult]) -> Self {
        if unique_ids && results.len() == features.len() && features.iter().zip(results).all(|(f, r)| f.0 == r.id) {
            Self::InPlace
        } else {
            Self::Indexed(result_index(results))
        }
    }

    /// The result of the feature `id` at `at` in the design. A feature without a result has
    /// none; a place that no longer holds this id (a click earlier in the frame ran a command) is
    /// searched for.
    fn get<'m>(&self, model: &'m solidcraft_engine::doc::Model, at: usize, id: u64) -> Option<&'m FeatureResult> {
        match self {
            Self::InPlace => model.results.get(at).filter(|r| r.id == id).or_else(|| model.result(id)),
            Self::Indexed(index) => index.get(&id).and_then(|k| model.results.get(*k).filter(|r| r.id == id).or_else(|| model.result(id))),
        }
    }
}

/// The History Play control's names: SolidCraft's commands that play and stop History, and
/// "Play from the start" while Play starts over (#10798).
pub(crate) fn play_labels(app: &SolveApp) -> crate::ui_kit::PlayLabels<'static> {
    let play = if play_starts_over(app) { tr!("ui.history_play_from_start") } else { tr!("cmd.timeline_play") };
    crate::ui_kit::PlayLabels { play, stop: tr!("cmd.timeline_stop") }
}

/// The bar as SolidCraft describes it; the strip sets the marker's slot and the step count.
fn bar(app: &SolveApp, floating: bool) -> history_bar::Bar<'static> {
    let t = Tokens::get();
    history_bar::Bar {
        marker_tip: tr!("ui.history_marker_tip"),
        accent: crate::workspace::tab_accent(&t, &app.ui.tab),
        drag_accent: crate::workspace::tab_accent(&t, "SKETCH"),
        marker: 0,
        steps: 0,
        floating,
        reorder: true,
        handles: BAR_HANDLES,
    }
}

/// The shared host already supplies the title and declared playback controls.
pub(crate) fn panel_inline(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::browser::sync_reveals(app);
    timeline_strip(app, ui, true, ui.max_rect().left());
}

/// Render the horizontal history strip and its interactive feature tiles, handling selection,
/// edits, drags, and hover updates.
fn timeline_strip(app: &mut SolveApp, ui: &mut egui::Ui, floating: bool, x: f32) {
    let t = Tokens::get();
    let accent = crate::workspace::tab_accent(&t, &app.ui.tab);
    let r = ui.max_rect();
    let p = ui.painter().clone();
    let n = app.session.doc.features.len();
    let marker = app.session.doc.marker.unwrap_or(n);
    let mut bar = bar(app, floating);
    // The feature under the cursor (viewport, browser, or timeline) or picked in the viewport.
    let from_view = from_view(app);
    // Long runs of one component's features show as one chip (`timeline_fold`). The bar shows the
    // edited or picked component's features, after the path to it (`timeline_scope`).
    let strip = crate::timeline_fold::Strip { marker, from_view, floating, accent };
    crate::timeline_scope::follow(app);
    let history = crate::timeline_fold::history(app, &strip);
    // The edited component's last feature, scrolled into view once (`timeline_scope`): the
    // folded chip holding it, its component's own chip, or the feature itself. Read before the
    // path, whose click asks for it in what the next frame shows.
    let reveal_at = app.tree.timeline_reveal.and_then(|id| app.session.doc.features.iter().position(|f| f.id == id));
    let reveal = reveal_at.and_then(|i| history.view.find(i));
    let x = crate::timeline_scope::header(app, ui, &p, x, r).min(r.right() - t.space_24);
    // A path click asks the next view to reveal the same feature. The old view must not
    // acknowledge that new request while finishing this frame.
    let reveal = reveal.filter(|_| history.within == crate::timeline_scope::shown(app));
    // Below, slots and features count those the bar shows; the marker's slot among them.
    let shown = history.view.len();
    let marker_view = history.view.slot(marker);
    bar.marker = marker_view;
    bar.steps = shown;
    // The features scroll horizontally in the space right of the playback controls.
    let strip_rect = Rect::from_min_max(pos2(x, r.top()), r.right_bottom());
    let rows = BarRows::cached(app, ui);
    let out = {
        let mut steps = BarSteps { app: &mut *app, history: &history, strip: &strip, rows: &rows, reveal, marker, marker_view };
        history_bar::strip(ui, &bar, r, strip_rect, ("history_strip", history.within), &mut steps)
    };
    crate::semantic_content::set_timeline_feature_hover(app, out.hovered);
    // The design's position of a slot; at the marker's, on the side of the marker the pointer is
    // on (features not shown may sit between it and the next one shown).
    let position = |slot: usize, before: bool| {
        let at = history.view.index(slot);
        match (slot == marker_view, before) {
            (true, true) => at.min(marker),
            (true, false) => at.max(marker),
            (false, _) => at,
        }
    };
    // The bar's features in the order it draws them, folded ones included: a Shift+click range
    // runs over these. Taken once, when a click needs it, from the list this frame drew (never
    // from the design as the click's command leaves it), so a range never mixes two orders.
    let mut order: Option<Vec<u64>> = None;
    for action in out.actions {
        match action {
            history_bar::Action::Select { step, modifiers } => {
                let order =
                    order.get_or_insert_with(|| history.view.indices(0, shown).iter().filter_map(|i| rows.features.get(*i).map(|f| f.0)).collect());
                click_feature(app, order, step, modifiers);
            }
            history_bar::Action::Edit(id) => app.edit_feature(id),
            // The same menu as the browser's, so the control channel reports it (#10745). A
            // right-click outside the selection selects that feature first, as the vertical
            // History does, so the menu acts on what is highlighted.
            history_bar::Action::Menu { step, at } => {
                if !selected_feature_ids(app).contains(&step) {
                    select_one(app, step);
                }
                crate::context_menu::open_for(app, at, crate::context_menu::Target::Timeline { feature: step });
            }
            history_bar::Action::Seek { slot, before } => {
                let slot = position(slot, before);
                let _ = app.run_heavy("timeline.roll_to", if slot >= n { json!({}) } else { json!({ "position": slot }) });
            }
            // Crossing the marker rolls the feature back or forward, in the same step.
            history_bar::Action::Reorder { step, slot, before } => {
                crate::timeline_folders::drop_feature(app, &history.folders, step, position(slot, before), before, marker.min(n));
            }
        }
    }
}

/// A click on a feature chip: into the open dialog's input when it takes features, else a
/// selection by the modifiers held.
fn click_feature(app: &mut SolveApp, order: &[u64], id: u64, modifiers: egui::Modifiers) {
    if pick_feature_into_dialog(app, id, modifiers) {
        return;
    }
    if modifiers.shift {
        select_range(app, order, id, modifiers.command || modifiers.ctrl);
    } else if modifiers.command || modifiers.ctrl {
        toggle_feature(app, id);
    } else if !modifiers.any() && crate::browser::pick_sketch_into_dialog(app, id) {
        // Extrude, Revolve or Emboss took every profile of the sketch (#6106).
    } else {
        select_one(app, id);
    }
}

/// SolidCraft's entries of the bar: its features, and its folded runs, folders, joints and trays.
struct BarSteps<'a> {
    app: &'a mut SolveApp,
    history: &'a crate::timeline_fold::History,
    strip: &'a crate::timeline_fold::Strip,
    rows: &'a BarRows,
    /// The slot to scroll into view, until it is.
    reveal: Option<usize>,
    /// The marker's design position, and its slot among the features shown.
    marker: usize,
    marker_view: usize,
}

impl history_bar::Contents for BarSteps<'_> {
    /// Draws the bar's chips: each feature, and the folded runs, folders, joints and trays around it.
    fn entries(&mut self, s: &mut history_bar::Strip<'_, '_>) {
        let t = Tokens::get();
        let app = &mut *self.app;
        let (history, strip) = (self.history, self.strip);
        // Each sketch's solve state, read once for the strip (#7245).
        let sketch_states = crate::sketch_state::SketchStates::of(app);
        // Each feature's result, found once for the strip.
        let results = BarResults::of(&self.rows.features, self.rows.unique_ids, &app.session.model.results);
        // The features picked in the design or in the open dialog, collected once for the
        // strip: the dialog built its item list again for every chip.
        let doc = &app.session.doc;
        let feature_of = |selection: &solidcraft_engine::Sel| feature_of_sel(doc, selection);
        let picked: std::collections::HashSet<u64> = app
            .session
            .selection
            .iter()
            .filter_map(feature_of)
            .chain(app.dialog.as_ref().map(|dialog| dialog.items()).unwrap_or_default().iter().filter_map(feature_of))
            .collect();
        let editing = crate::timeline_scope::focus(app).unwrap_or(0);
        for entry in history.entries.iter().copied() {
            let i = match entry {
                crate::timeline_fold::Entry::Feature(i) => i,
                crate::timeline_fold::Entry::Group(group) => {
                    let rect = s.chip(group.start, group.end, group.open, group.depth, |ui, p, at| {
                        crate::timeline_fold::group_chip(app, ui, p, at, &group, history, strip)
                    });
                    let holds = self.reveal.is_some_and(|k| group.start <= k && k < group.end && (!group.open || group.component == editing));
                    if reveal_once(s.ui(), &mut self.reveal, holds, rect, egui::Align::Min) {
                        app.tree.timeline_reveal = None;
                    }
                    continue;
                }
                crate::timeline_fold::Entry::Folder(folder) => {
                    let rect = s.chip(folder.start, folder.end, folder.open, folder.depth, |ui, p, at| {
                        crate::timeline_folders::folder_chip(app, ui, p, at, &folder, history, strip)
                    });
                    let holds = self.reveal.is_some_and(|k| folder.start <= k && k < folder.end && !folder.open);
                    if reveal_once(s.ui(), &mut self.reveal, holds, rect, egui::Align::Min) {
                        app.tree.timeline_reveal = None;
                    }
                    continue;
                }
                crate::timeline_fold::Entry::Joints(folder) => {
                    let joints = history.folders.joints.get(&folder).cloned().unwrap_or_default();
                    let run = crate::dialogs_assembly::JointRun::Folder { id: folder, name: history.folders.name(folder) };
                    s.inline(|ui, p, x, r| crate::dialogs_assembly::joint_chips(app, ui, p, x, r, &joints, run));
                    continue;
                }
                crate::timeline_fold::Entry::End(_) => {
                    s.end_tray();
                    continue;
                }
            };
            // The feature's index in the design.
            let at = history.view.index(i);
            let Some((id, name, icon, suppressed, ty, kind)) = self.rows.features.get(at) else {
                continue;
            };
            let sketch_state = matches!(kind, TimelineKind::Sketch).then(|| sketch_states.get(*id)).flatten();
            let res = results.get(&app.session.model, at, *id);
            let rolled = i >= self.marker_view;
            let tip = || feature_tooltip(name, ty, res, rolled, sketch_state);
            let rect = s.step(&history_bar::Step {
                id: *id,
                slot: i,
                label: name,
                icon,
                icon_accent: self.rows.accents.get(at).copied().flatten(),
                state: history_bar::StepState {
                    selected: picked.contains(id),
                    highlighted: strip.from_view.contains(&Some(*id)),
                    rolled_back: rolled,
                    suppressed: *suppressed,
                    last_done: at + 1 == self.marker,
                },
                error: res.is_some_and(|r| r.error.is_some()),
                warning: res.is_some_and(has_warning),
                ink: sketch_state.and_then(|state| state.ink(&t)),
                tip: &tip,
                handle: self.rows.handles.get(at),
            });
            // Only the step to reveal takes the strip's ui: taking it makes the steps out of view
            // after it read egui's memory again.
            let holds = self.reveal == Some(i);
            if holds && reveal_once(s.ui(), &mut self.reveal, holds, rect, egui::Align::Center) {
                app.tree.timeline_reveal = None;
            }
        }
    }

    /// The joints at the end belong to the whole design: a bar with its groups that shows one
    /// component leaves them out. Without the groups they stay, as they always have.
    fn trailing(&mut self, ui: &mut egui::Ui, p: &egui::Painter, end: f32, r: Rect) -> f32 {
        if self.history.within.is_none() || !self.app.preferences.history_bar_groups {
            crate::dialogs_assembly::timeline_joints(self.app, ui, p, end + 10.0, r, &self.history.folders.held)
        } else {
            end
        }
    }
}

/// Select what the features `ids` made, so the browser shows it: each one's bodies, or the
/// feature itself when it made none. While the browser's rows start closed
/// (Preferences › Layout), the first body is also found there, as Find in Browser on a body
/// does: the rows down to it open.
pub(crate) fn find_in_browser(app: &mut SolveApp, ids: &[u64]) {
    let st = crate::open_meshing::session_world(&app.session);
    let mut bodies: Vec<String> = Vec::new();
    let mut items: Vec<serde_json::Value> = Vec::new();
    // An id that names no feature of this design is left out.
    for id in ids.iter().filter(|id| app.session.doc.feature(**id).is_some()) {
        let made: Vec<String> = st.bodies.iter().filter(|b| b.feature == *id).map(|b| b.name.clone()).collect();
        if made.is_empty() {
            items.push(json!({"type": "feature", "id": id}));
        }
        items.extend(made.iter().map(|name| json!({"type": "body", "name": name})));
        bodies.extend(made);
    }
    // Nothing to find: the selection stays.
    if items.is_empty() {
        return;
    }
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
    fn a_folder_of_one_kind_of_feature_takes_its_icon_when_preferences_say_so() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "name": "Block"})).unwrap();
        for x in [8, 16, 24, 32] {
            h.app.run("solid.hole", json!({"position": [x, 10, 20], "diameter": 4, "name": format!("H{x}")})).unwrap();
        }
        let group = |h: &mut Harness, p: serde_json::Value| h.app.run("timeline.group", p).unwrap()["group"].as_u64().unwrap();
        let inner = group(&mut h, json!({"name": "Inner", "features": ["H8", "H16"]}));
        // Holes, and a folder of holes inside: all one kind.
        let holes = group(&mut h, json!({"name": "Holes", "features": ["H24"], "groups": [inner]}));
        let mixed = group(&mut h, json!({"name": "Mixed", "features": ["Block"]}));
        let hole = super::icon_of(&h.app.session.doc.find_feature("H8").unwrap().kind);
        // Off (the default): every folder keeps the folder icon.
        assert!(!h.app.preferences.history_folder_kind_icon);
        assert_eq!(super::folder_kind_icon(&h.app, holes), None);
        crate::prefs::set(&mut h.app, &json!({"history_folder_kind_icon": true})).unwrap();
        assert_eq!(super::folder_kind_icon(&h.app, holes), Some(hole));
        assert_eq!(super::folder_kind_icon(&h.app, inner), Some(hole));
        assert_eq!(super::folder_kind_icon(&h.app, mixed), Some(super::icon_of(&h.app.session.doc.find_feature("Block").unwrap().kind)));
        // A box among the holes: back to the folder icon.
        h.app.run("timeline.group", json!({"name": "Both", "groups": [holes, mixed]})).unwrap();
        let both = h.app.session.doc.timeline_groups.iter().find(|g| g.name == "Both").unwrap().id;
        assert_eq!(super::folder_kind_icon(&h.app, both), None);
        // Both Histories draw it without trouble.
        for left in [false, true] {
            h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
            h.frames(3);
        }
    }

    #[test]
    fn joint_folder_hides_fifty_joint_tiles_in_both_history_layouts() {
        use crate::scenario::handle_at;
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
            h.app.preferences.history_group_joints = true;
            h.app.preferences.history_bar_groups = true;
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
            crate::timeline_folders::set_open(&mut h.app, folder, false);
            h.frames(4);
            // The History bar shows a folder as a chip; the History beside the Browser as a row.
            let (folders, fold) =
                if left { ("timeline-folder:", "timeline-folder-fold:Joints") } else { ("timeline-folder-chip:", "timeline-folder-chip:Joints") };
            assert!(handle_at(&format!("{folders}Joints")).is_some());
            assert_eq!(crate::scenario::handles(folders).len(), 1);
            assert!(crate::scenario::handles("timeline-joint:").is_empty());
            for index in 0..50 {
                assert!(handle_at(&format!("timeline-joint:Hinge{index}")).is_none());
            }
            h.step(&json!({"click":{"handle":fold}})).unwrap();
            h.frames(4);
            if !left {
                // In the History bar an open folder's joints are one button from the second on.
                assert!(crate::scenario::handles("timeline-joint:").is_empty());
                h.step(&json!({"click":{"handle":"timeline-folder-joints:Joints"}})).unwrap();
                h.frames(4);
            }
            for index in 0..50 {
                assert!(handle_at(&format!("timeline-joint:Hinge{index}")).is_some(), "joint {index} missing, left={left}");
            }
            assert_eq!(crate::scenario::handles("timeline-joint:").len(), 50, "no trailing duplicates");
            h.step(&json!({"click":{"handle":"timeline-joint:Hinge0"}})).unwrap();
            assert_eq!(h.app.tree.picked_joints, [joints[0]]);
            h.step(&json!({"click":{"handle":fold}})).unwrap();
            h.frames(4);
            assert!(handle_at("timeline-joint:Hinge0").is_none());
        }
    }

    #[test]
    fn history_hiding_a_component_leaves_its_features_out_of_the_bar() {
        use crate::context_menu::{self, Target};
        use crate::scenario::handle_at;
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("component.create", json!({ "name": "Copy" })).unwrap();
        h.app.run("solid.box", json!({ "length": 10, "width": 10, "height": 10, "name": "CopyBox" })).unwrap();
        h.app.run("component.activate", json!({ "component": "root" })).unwrap();
        h.app.run("component.create", json!({ "name": "Part" })).unwrap();
        h.app.run("solid.box", json!({ "length": 5, "width": 5, "height": 5, "name": "PartBox" })).unwrap();
        h.app.run("component.activate", json!({ "component": "root" })).unwrap();
        h.frames(4);
        assert!(handle_at("timeline:CopyBox").is_some() && handle_at("timeline:PartBox").is_some(), "both parts' features start in the bar");
        // The Browser menu offers to hide the copy's History, and running it leaves its entries out.
        let copy = h.app.session.doc.components.iter().find(|c| c.name == "Copy").unwrap().id;
        let menu = context_menu::entries(&h.app, &Target::Component { id: copy });
        let hide = menu.iter().find(|i| i.id == "component.hide_history").expect("the component menu offers Hide its History");
        context_menu::run_item(&mut h.app, hide, egui::Pos2::ZERO);
        h.frames(4);
        assert!(handle_at("timeline:CopyBox").is_none(), "the hidden copy leaves the bar");
        assert!(handle_at("timeline:PartBox").is_some(), "the rest of the design stays");
        // The History controls' menu shows them anyway, and hides them again (#10393).
        fn toggle(h: &Harness) -> crate::ui_kit::Item {
            let items = context_menu::items(&h.app, &Target::HistoryControls);
            items.into_iter().find(|i| i.id == "ui.historyShowHidden").expect("the bar offers to show hidden entries").clone()
        }
        let item = toggle(&h);
        context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
        h.frames(4);
        assert!(handle_at("timeline:CopyBox").is_some(), "the toggle brings the hidden entries back");
        let item = toggle(&h);
        context_menu::run_item(&mut h.app, &item, egui::Pos2::ZERO);
        h.frames(4);
        assert!(handle_at("timeline:CopyBox").is_none(), "and hides them again");
        // The component menu now offers to show its History back.
        let menu = context_menu::entries(&h.app, &Target::Component { id: copy });
        assert!(menu.iter().any(|i| i.id == "component.show_history"), "the menu flips to Show its History");
    }

    /// Clear History in the History controls' menu deletes every step the History shows, its
    /// folders too, in one undo step; with nothing left it is greyed out (#15969).
    #[test]
    fn clear_history_deletes_every_shown_step_and_undo_brings_them_back() {
        use crate::context_menu::{self, Target};
        fn clear(h: &Harness) -> crate::ui_kit::Item {
            let items = context_menu::items(&h.app, &Target::HistoryControls);
            items.into_iter().find(|i| i.id == "ui.historyClear").expect("the History menu offers Clear History")
        }
        fn run_and_wait(h: &mut Harness, item: &crate::ui_kit::Item) {
            context_menu::run_item(&mut h.app, item, egui::Pos2::ZERO);
            for _ in 0..500 {
                if h.app.deferred.is_none() && h.app.deferred_queue.is_empty() {
                    break;
                }
                h.frame();
            }
            h.frames(4);
        }
        let mut h = two_boxes(false);
        h.app.run("timeline.group", json!({"name": "Boxes", "features": ["First"]})).unwrap();
        assert_eq!(clear(&h).label, "Clear History");
        let item = clear(&h);
        assert!(item.enabled, "a design with steps can be cleared");
        run_and_wait(&mut h, &item);
        assert!(h.app.session.doc.features.is_empty(), "every step is gone");
        assert!(h.app.session.doc.timeline_groups.is_empty(), "no empty folder is left behind");
        assert!(h.app.dialog.is_none(), "steps that only depend on each other need no confirmation");
        assert!(!clear(&h).enabled, "an empty History has nothing to clear");
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(4);
        let names: Vec<&str> = h.app.session.doc.features.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["First", "Second"], "one Undo brings every step back");
        assert_eq!(h.app.session.doc.timeline_groups.len(), 1, "and the folder");

        // A component whose History is hidden stays out of the History, so Clear History keeps it,
        // whether or not the History steps follow one component (the stepping view lists hidden
        // components when they don't).
        for steps_component in [true, false] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.preferences.history_steps_component = steps_component;
            h.app.run("component.create", json!({ "name": "Copy" })).unwrap();
            h.app.run("solid.box", json!({ "length": 10, "width": 10, "height": 10, "name": "CopyBox" })).unwrap();
            h.app.run("component.activate", json!({ "component": "root" })).unwrap();
            h.app
                .run("solid.box", json!({ "length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "operation": "new_body", "name": "RootBox" }))
                .unwrap();
            h.app.run("component.hide_history", json!({ "component": "Copy" })).unwrap();
            h.frames(4);
            let item = clear(&h);
            run_and_wait(&mut h, &item);
            let names: Vec<&str> = h.app.session.doc.features.iter().map(|f| f.name.as_str()).collect();
            assert!(names.contains(&"CopyBox") && !names.contains(&"RootBox"), "only the shown steps go ({steps_component}): {names:?}");
        }
    }

    #[test]
    fn the_bar_leaves_out_hidden_components_and_their_children() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({ "name": "Copy" })).unwrap();
        h.app.run("solid.box", json!({ "length": 1, "width": 1, "height": 1, "name": "CopyBox" })).unwrap();
        h.app.run("component.create", json!({ "name": "Inner" })).unwrap();
        h.app.run("solid.box", json!({ "length": 1, "width": 1, "height": 1, "name": "InnerBox" })).unwrap();
        h.app.run("component.activate", json!({ "component": "root" })).unwrap();
        h.app.run("component.create", json!({ "name": "Part" })).unwrap();
        h.app.run("solid.box", json!({ "length": 1, "width": 1, "height": 1, "name": "PartBox" })).unwrap();
        h.app.run("component.activate", json!({ "component": "root" })).unwrap();
        h.app.run("component.hide_history", json!({ "component": "Copy" })).unwrap();
        use crate::timeline_fold::View;
        let doc = &h.app.session.doc;
        let names = |view: View| view.indices(0, view.len()).iter().map(|i| doc.features[*i].name.clone()).collect::<Vec<_>>();
        assert_eq!(names(View::of(doc, None, false)), ["PartBox"], "a hidden component takes its own and its children's rows out");
        assert_eq!(names(View::of(doc, None, true)), ["CopyBox", "InnerBox", "PartBox"], "show-hidden lists everything again");
        // The same filter applies when the bar shows one component.
        let copy = doc.components.iter().find(|c| c.name == "Copy").unwrap().id;
        let within = crate::timeline_scope::components_within(&h.app, copy);
        assert!(names(View::of(doc, Some(&within), false)).is_empty(), "the component filter shows nothing of a hidden component");
        assert_eq!(names(View::of(doc, Some(&within), true)), ["CopyBox", "InnerBox"], "Copy owns CopyBox and InnerBox");
    }

    #[test]
    fn history_status_translates_prefixes_and_both_diagnostics() {
        crate::i18n::install();
        let diagnostic = "there is no body to cut or intersect in component 'Mount'; the tool may overlap 'Plate' in component 'Base' — activate that component or pick the body as a target";
        let _language = solidcraft_ui_text::scope("ja");
        let translated = crate::i18n::tr_diagnostic(diagnostic);
        assert_ne!(translated, diagnostic);
        assert_eq!(
            super::status_tip("My feature", "ExtrudeFeature", Some(diagnostic), Some(diagnostic)).plain(),
            format!("My feature  (ExtrudeFeature)\nエラー: {translated}\n警告: {translated}")
        );
        assert_eq!(super::status_tip("My feature", "ExtrudeFeature", None, None).plain(), "My feature  (ExtrudeFeature)");
        // Unknown diagnostics remain intact, including names and measured values.
        assert_eq!(
            super::status_tip("My feature", "ExtrudeFeature", None, Some("Custom part: 1.25 mm")).plain(),
            "My feature  (ExtrudeFeature)\n警告: Custom part: 1.25 mm"
        );
        let _english = solidcraft_ui_text::scope("en");
        assert_eq!(
            super::status_tip("Box", "BoxFeature", Some("error"), Some("warning")).plain(),
            "Box  (BoxFeature)\nError: error\nWarning: warning"
        );
    }

    /// An import's warnings read as a summary for a CAD user: parts counted by what became of
    /// them, faces counted, no entity numbers or kernel wording (those stay in the details).
    #[test]
    fn an_import_summary_counts_parts_and_faces_in_plain_words() {
        crate::i18n::install();
        use solidcraft_engine::doc::kernel::{ImportIssue, ImportReport, IssueKind};
        let _english = solidcraft_ui_text::scope("en");
        let issue = |kind, item: Option<u64>, faces, detail: &str| ImportIssue {
            kind,
            item,
            part: String::new(),
            paths: Vec::new(),
            faces,
            times: 1,
            detail: detail.into(),
        };
        let report = ImportReport {
            parts: 43,
            solids: 40,
            issues: vec![
                issue(
                    IssueKind::Refused,
                    Some(7),
                    0,
                    "solid #7: not imported: pcurves failed on 1 of 122 faces; first: face #9: SMLib pcurves: an edge lies 13.73370504071075 mm off the face's surface",
                ),
                issue(
                    IssueKind::FaceLeftOut,
                    Some(7),
                    1,
                    "solid #7: not shown: face #9: SMLib pcurves: SMLib rejected the input (SM_ERR_INVALID_INPUT)",
                ),
                issue(IssueKind::ImportedAsSurfaces, Some(8), 0, "solid #8: imported as surfaces: …"),
                issue(IssueKind::FaceLeftOut, Some(8), 1, "solid #8: left out: face #11: …"),
                issue(IssueKind::FaceLeftOut, Some(8), 1, "solid #8: left out: face #12: …"),
                issue(IssueKind::NotImported, Some(5), 0, "solid #5: not imported: …"),
                issue(IssueKind::Repaired, Some(3), 0, "solid #3: 2 seam edges added to faces that wind around their surface"),
                issue(IssueKind::Repaired, Some(4), 0, "solid #4: 1 faces turned over: their loops disagree with their same_sense"),
                issue(IssueKind::File, None, 0, "component placement #77: … (placed at the origin)"),
            ],
            single_root: true,
            ..Default::default()
        };
        let summary = super::import_summary(&report);
        assert_eq!(
            summary,
            "Imported 40 of 43 parts as solids. 1 part couldn't be imported. 1 part couldn't be made into a solid and was not imported. \
             1 part couldn't be made into a solid and came in as surfaces. 3 faces couldn't be repaired and are missing. \
             Small flaws were repaired in 2 parts. There was 1 other problem with the file. \
             Exporting these parts again from the program that made them may fix this. Copy Warning Message has the details."
        );
        assert!(!summary.contains('#') && !summary.contains("SMLib"), "{summary}");
        // Only repairs: no advice, and a part alone has no "of" count.
        let repaired = ImportReport {
            parts: 1,
            solids: 1,
            issues: vec![issue(IssueKind::Repaired, Some(3), 0, "solid #3: …")],
            single_root: true,
            ..Default::default()
        };
        assert_eq!(super::import_summary(&repaired), "Small flaws were repaired in 1 part.");
    }

    #[test]
    fn history_marker_tooltip_uses_the_language_in_both_layouts() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.language = "ja".into();
            h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
            h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            h.frames(4);
            let marker = crate::scenario::rect_of("history:marker").unwrap().center();
            h.call("ui.move", json!({"x": marker.x, "y": marker.y}));
            h.frames(90);
            let expected = solidcraft_ui_text::Locale::for_language("ja").text("ui.history_marker_tip");
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
        let ja = solidcraft_ui_text::Locale::for_language("ja").text("timeline.suppress_features").to_string();
        let ja_unsuppress = solidcraft_ui_text::Locale::for_language("ja").text("timeline.unsuppress_features").to_string();
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
        assert_eq!(solidcraft_ui_text::Locale::for_language("en").text("timeline.suppress_features"), "Suppress Features");
        assert_eq!(solidcraft_ui_text::Locale::for_language("en").text("timeline.unsuppress_features"), "Unsuppress Features");
    }

    #[test]
    fn construction_history_matches_the_tool_that_created_it() {
        use solidcore_record::Vec3;
        use solidcraft_engine::doc::{
            FeatureKind, PlaneRef,
            construct::{AxisDef, GeoRef, PointDef},
        };
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
            ("average", PointDef::Average { points: vec![point(), point()] }),
        ] {
            cases.push((format!("construct.point.{id}"), FeatureKind::ConstructionPoint { def }));
        }
        for (id, plane) in [
            ("midplane", PlaneRef::Midplane { a: reference(), b: reference(), old_frame: false }),
            ("axis_point", PlaneRef::AxisPoint { axis: reference(), point: reference() }),
            ("two_edges", PlaneRef::TwoEdges { a: reference(), b: reference() }),
            ("three_points", PlaneRef::ThreePoints { a: reference(), b: reference(), c: reference() }),
            ("tangent", PlaneRef::Tangent { face: reference(), at: Vec3::ZERO, reference: None, angle: None }),
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
            placements: Vec::new(),
            components: Vec::new(),
            occurrences: Vec::new(),
            pattern: PatternKind::Path {
                path_sketch: solidcraft_engine::doc::SketchId(0),
                path: Vec::new(),
                count: "2".into(),
                spacing: "10".into(),
                extent: false,
                flip: false,
                orient: false,
                start: None,
                start_fraction: None,
                follow_frame: true,
            },
        };
        assert_eq!(super::icon_of(&feature), solidcraft_engine::find_command("solid.pattern.path").unwrap().icon);
    }

    fn two_boxes(left: bool) -> Harness {
        let mut h = Harness::new();
        h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
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

    /// With History in the browser its bar is hidden, so a right-click on the History folder
    /// offers the bar's controls, each greyed out while it would leave the marker where it is
    /// (#14521).
    #[test]
    fn the_browser_history_folder_menu_steps_and_greys_out_what_would_not_move() {
        let mut h = two_boxes(false);
        let folder = crate::context_menu::Target::HistoryFolder;
        assert!(crate::context_menu::check_target(&h.app, &folder).is_err(), "only the browser lists History as a folder");
        h.app.run("view.history_layout", json!({"dock": "browser"})).unwrap();
        h.frames(4);
        assert!(crate::context_menu::check_target(&h.app, &folder).is_ok());
        let enabled = |h: &Harness| -> Vec<(String, bool)> {
            crate::context_menu::items(&h.app, &crate::context_menu::Target::HistoryFolder)
                .into_iter()
                .filter(|i| i.id == "ui.historyTransport")
                .map(|i| (i.params.as_ref().and_then(|p| p["control"].as_str()).unwrap_or_default().to_owned(), i.enabled))
                .collect()
        };
        let state = |pairs: [(&str, bool); 5]| pairs.map(|(c, on)| (c.to_owned(), on)).to_vec();
        // At the end: only the steps back are on, and Play plays from the start (#10798).
        assert_eq!(enabled(&h), state([("first", true), ("back", true), ("play", true), ("forward", false), ("last", false)]));
        let play = |h: &Harness| {
            crate::context_menu::items(&h.app, &folder).into_iter().find(|i| i.id == "ui.historyTransport" && i.icon == "timeline_play")
        };
        assert_eq!(play(&h).map(|i| i.label).as_deref(), Some(solidcraft_ui_text::tr_by_id("ui.history_play_from_start")));
        // The bar's controls menu follows a separator.
        let items = crate::context_menu::items(&h.app, &folder);
        assert!(items.iter().any(|i| i.is_sep()) && items.iter().any(|i| i.id == "ui.historyPlayButton"));
        // A right-click on the folder opens the menu; Go to beginning rolls to the start.
        h.step(&json!({"click": {"browser": solidcraft_ui_text::tr_by_id("ui.history_title")}, "button": "right"})).unwrap();
        assert_eq!(h.app.menu.target, folder);
        h.call("ui.menuPick", json!({"item": solidcraft_ui_text::tr_by_id("ui.history_first")}));
        h.frames(4);
        assert_eq!(h.app.session.doc.marker, Some(0));
        assert_eq!(enabled(&h), state([("first", false), ("back", false), ("play", true), ("forward", true), ("last", true)]));
        // One step forward leaves every control something to do.
        let forward = crate::context_menu::items(&h.app, &folder).into_iter().find(|i| i.label == solidcraft_ui_text::tr_by_id("ui.history_forward"));
        crate::context_menu::run_item(&mut h.app, &forward.unwrap(), egui::Pos2::ZERO);
        h.frames(4);
        assert_eq!(h.app.session.doc.marker, Some(1));
        assert!(enabled(&h).iter().all(|(_, on)| *on), "{:?}", enabled(&h));
        // With the Play button hidden, the menu leaves Play out, as the bar does.
        h.app.preferences.history_play_button = false;
        assert!(enabled(&h).iter().all(|(c, _)| c != "play"));
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
        for layout in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
        h.ctx
            .run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                super::tick_playback(&mut h.app, ui.ctx());
            })
            .textures_delta
            .clear();
        assert_eq!(h.app.session.doc.marker, Some(1));
        assert_eq!(h.app.session.model.state().bodies.len(), 1);
    }

    /// Play with the marker at the end goes back to the start, shows the design before its first
    /// feature, then plays every feature at the usual pace (#10798).
    #[test]
    fn playback_at_the_end_starts_over_from_the_first_feature() {
        let mut h = two_boxes(false);
        assert_eq!(h.app.session.doc.marker, None);
        assert_eq!(super::play_labels(&h.app).play, solidcraft_ui_text::tr_by_id("ui.history_play_from_start"));
        h.app.start("timeline.play");
        h.frames(2);
        assert_eq!(h.app.session.doc.marker, Some(0), "back to the start first");
        assert_eq!(h.app.session.model.state().bodies.len(), 0);
        assert!(h.app.timeline_playback.is_some());
        assert_eq!(
            super::play_labels(&h.app).play,
            solidcraft_ui_text::tr_by_id("cmd.timeline_play"),
            "away from the end, Play is Play History again"
        );
        h.frames(10);
        assert_eq!(h.app.session.doc.marker, Some(0), "leave time to see the start");
        h.frames(40);
        assert_eq!(h.app.session.doc.marker, Some(1));
        assert_eq!(h.app.session.model.state().bodies.len(), 1);
        h.frames(60);
        assert_eq!(h.app.session.doc.marker, None, "played to the end");
        assert_eq!(h.app.session.model.state().bodies.len(), 2);
        assert!(h.app.timeline_playback.is_none());

        // Escape stops a playback that started over where it is.
        h.app.start("timeline.play");
        h.frames(2);
        assert_eq!(h.app.session.doc.marker, Some(0));
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(80);
        assert!(h.app.timeline_playback.is_none());
        assert_eq!(h.app.session.doc.marker, Some(0), "stopped at the start");
    }

    #[test]
    fn playback_in_an_empty_design_leaves_history_unchanged() {
        let mut h = Harness::new();
        h.frames(2);
        let revision = h.app.session.revision;
        h.app.start("timeline.play");
        h.frames(2);
        h.frames(60);
        assert_eq!(h.app.session.revision, revision);
        assert!(h.app.timeline_playback.is_none());
    }

    #[test]
    fn playback_refuses_to_start_while_other_history_work_waits() {
        let mut h = two_boxes(false);
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.app.deferred_queue.push(("timeline.roll_to".into(), json!({"position": 1}), 0));
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
            let along_strip = |position: egui::Pos2| {
                if dock == "left" && width == crate::ui_kit::history_width(false) { position.y } else { position.x }
            };
            assert!(
                along_strip(back) < along_strip(play) && along_strip(play) < along_strip(forward),
                "Play sits between Previous and Next; {dock} {width}"
            );

            h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
            h.frames(2);
            h.step(&json!({"click": {"handle": "history:play"}})).unwrap();
            assert!(h.app.timeline_playback.is_some(), "the button plays; {dock} {width}");
            h.step(&json!({"click": {"handle": "history:play"}})).unwrap();
            assert!(h.app.timeline_playback.is_none(), "and stops; {dock} {width}");

            right_click(&mut h, "history:back");
            assert!(h.app.viewport.context_menu.is_some() && h.app.menu.target == Target::HistoryControls, "{dock} {width}");
            let items = crate::context_menu::items(&h.app, &Target::HistoryControls);
            assert_eq!(items[0].label, solidcraft_ui_text::tr_by_id("ui.history_play_button_hide"));
            crate::context_menu::run_item(&mut h.app, &items[0], egui::Pos2::ZERO);
            assert!(!h.app.preferences.history_play_button);
            h.frames(2);
            assert!(crate::scenario::handle_at("history:play").is_none(), "the four controls again; {dock} {width}");

            let items = crate::context_menu::items(&h.app, &Target::HistoryControls);
            assert_eq!(items[0].label, solidcraft_ui_text::tr_by_id("ui.history_play_button_show"));
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

    /// A long history folds a component's features into one chip with its name and count. A run
    /// that holds the history marker shows each feature until its chip is clicked; folded by
    /// hand, the marker waits after the chip. Clicking the chip again shows each feature.
    #[test]
    fn a_long_history_folds_a_components_features_into_one_chip() {
        use crate::scenario::{handle_at, rect_of};
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Root sketch"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("component.create", json!({"name": "Rig"})).unwrap();
        for i in 0..crate::timeline_fold::FOLD_FROM {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline:Root sketch").is_some(), "a root feature stays on its own");
        assert!(handle_at("timeline:S0").is_none(), "the component's features are folded");
        assert!(handle_at("timeline-group:Rig").is_some(), "the folded run's chip");

        h.app.run("timeline.roll_to", json!({"position": 20})).unwrap();
        h.frames(2);
        assert!(handle_at("timeline:S10").is_some(), "the run holding the marker shows each feature");
        let chip = handle_at("timeline-group:Rig").unwrap();
        h.call("ui.click", json!({"x": chip.x, "y": chip.y}));
        h.frames(2);
        assert!(handle_at("timeline:S10").is_none(), "clicking the chip folds them anyway");
        let chip = handle_at("timeline-group:Rig").unwrap();
        let marker = rect_of("history:marker").expect("the marker stays in view");
        assert!(marker.left() > chip.x, "the marker waits after the folded chip: {marker:?} {chip:?}");

        h.app.run("timeline.roll_to", json!({})).unwrap();
        h.call("ui.click", json!({"x": chip.x, "y": chip.y}));
        h.frames(2);
        assert!(handle_at("timeline:S0").is_some() && handle_at("timeline:S49").is_some(), "clicking again shows each feature");
    }

    /// #10393 with the bar's groups on: a component marked out of the History bar leaves neither
    /// its features nor a chip for them, and its features don't count towards folding. Showing
    /// hidden components brings its long run back, folded like any other.
    #[test]
    fn a_component_hidden_from_the_bar_leaves_no_chip_and_comes_back_folded() {
        use crate::scenario::handle_at;
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Root sketch"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("component.create", json!({"name": "Copy"})).unwrap();
        for i in 0..crate::timeline_fold::FOLD_FROM {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("C{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("component.hide_history", json!({"component": "Copy"})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline:Root sketch").is_some(), "the rest of the design shows");
        assert!(handle_at("timeline:C0").is_none() && handle_at("timeline-group:Copy").is_none(), "no feature or chip of the hidden component");
        h.app.tree.history_show_hidden = true;
        h.frames(4);
        assert!(handle_at("timeline-group:Copy").is_some(), "shown again, its long run folds into a chip");
        assert!(handle_at("timeline:C0").is_none(), "folded, not each feature");
    }

    /// A run that holds subassemblies folding in turn shows only its component's features when
    /// clicked, after a path back to the whole design. A subassembly there opens in place, and the
    /// marker dragged between its features rolls the whole history there. "All components" goes
    /// back to the whole design, where such a run stays folded with the marker inside it.
    #[test]
    fn a_deep_run_shows_only_its_component_and_the_path_steps_back_out() {
        use crate::scenario::{handle_at, rect_of};
        let (mut h, _) = rig_with_arm_and_leg(30);
        h.call("ui.resize", json!({"width":2200,"height":1000}));
        let rig_chip = handle_at("timeline-group:Rig").expect("the rig folds");
        assert!(handle_at("timeline-group:Arm").is_none() && handle_at("timeline-path:Rig").is_none());
        assert!(handle_at("timeline-path:All components").is_some(), "the path starts at the whole design");
        h.call("ui.click", json!({"x": rig_chip.x, "y": rig_chip.y}));
        h.frames(3);
        assert!(handle_at("timeline-path:Rig").is_some(), "the path ends at the rig");
        assert!(handle_at("timeline:Root sketch").is_none(), "the rest of the history is left out");
        let arm = handle_at("timeline-group:Arm").expect("its subassemblies show folded");
        assert!(handle_at("timeline-group:Leg").is_some());
        h.call("ui.click", json!({"x": arm.x, "y": arm.y}));
        h.frames(3);
        assert!(handle_at("timeline:Arm0").is_some() && handle_at("timeline:Arm29").is_some(), "a subassembly opens in place");

        let marker = rect_of("history:marker").expect("the marker sits at the end of the history");
        let arm5 = handle_at("timeline:Arm5").unwrap();
        h.step(&json!({"drag": [[marker.center().x, marker.center().y], [arm5.x - 14.0, arm5.y]]})).unwrap();
        h.settle();
        let index = h.app.session.doc.features.iter().position(|f| f.name == "Arm5").unwrap();
        assert_eq!(h.app.session.doc.marker, Some(index), "the marker goes before Arm5 in the whole history");

        let all = handle_at("timeline-path:All components").unwrap();
        h.call("ui.click", json!({"x": all.x, "y": all.y}));
        h.frames(3);
        assert!(handle_at("timeline:Root sketch").is_some() && handle_at("timeline-path:Rig").is_none(), "back to the whole design");
        assert!(handle_at("timeline-group:Rig").is_some() && handle_at("timeline-group:Arm").is_none(), "the rig stays folded around the marker");
    }

    /// A root sketch, then a component Rig holding Arm (`arm` sketches) and Leg (25): Rig's run is
    /// deep. The bar's groups are on. Returns the harness, editing the root again, and the
    /// components' ids by name.
    fn rig_with_arm_and_leg(arm: usize) -> (Harness, std::collections::HashMap<&'static str, u64>) {
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Root sketch"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let rig = h.app.run("component.create", json!({"name": "Rig"})).unwrap()["component"].as_u64().unwrap();
        let mut ids = std::collections::HashMap::from([("Rig", rig)]);
        for (part, count) in [("Arm", arm), ("Leg", 25)] {
            h.app.run("component.activate", json!({"component": rig})).unwrap();
            let id = h.app.run("component.create", json!({"name": part})).unwrap()["component"].as_u64().unwrap();
            ids.insert(part, id);
            for i in 0..count {
                h.app.run("sketch.create", json!({"plane": "XY", "name": format!("{part}{i}")})).unwrap();
                h.app.run("sketch.finish", json!({})).unwrap();
            }
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(4);
        (h, ids)
    }

    /// Editing a component shows only its features, after the path to it. A click on a step
    /// around it shows that component's features, with the way to the edited one open and
    /// scrolled into view past an open subassembly wider than the window; the edited one's step
    /// goes back in. Editing another component or picking one in the Browser follows it; editing
    /// the root shows the whole design.
    #[test]
    fn editing_a_component_shows_only_its_features_and_the_path_steps_out() {
        use crate::scenario::{handle_at, rect_of};
        let (mut h, ids) = rig_with_arm_and_leg(60);
        let rig = handle_at("timeline-group:Rig").unwrap();
        h.call("ui.click", json!({"x": rig.x, "y": rig.y}));
        h.frames(3);
        let arm = handle_at("timeline-group:Arm").unwrap();
        h.call("ui.click", json!({"x": arm.x, "y": arm.y}));
        h.frames(3);
        let all = handle_at("timeline-path:All components").unwrap();
        h.call("ui.click", json!({"x": all.x, "y": all.y}));
        h.frames(3);
        assert!(handle_at("timeline-group:Rig").is_some() && handle_at("timeline:Leg0").is_none());

        h.app.run("component.activate", json!({"component": ids["Leg"]})).unwrap();
        h.frames(3);
        assert!(handle_at("timeline:Leg0").is_some() && handle_at("timeline:Leg24").is_some(), "the leg's features show");
        assert!(handle_at("timeline:Arm0").is_none() && handle_at("timeline:Root sketch").is_none(), "nothing else does");
        assert!(handle_at("timeline-path:Rig").is_some() && handle_at("timeline-path:Leg").is_some(), "the path to the leg");

        let rig = handle_at("timeline-path:Rig").unwrap();
        h.call("ui.click", json!({"x": rig.x, "y": rig.y}));
        h.frames(30);
        assert!(handle_at("timeline:Arm0").is_some() && handle_at("timeline:Leg0").is_some(), "the rig's features: the open arm's and the leg's");
        let strip = rect_of("history:strip").or_else(|| rect_of("history:card")).unwrap();
        let leg = handle_at("timeline-group:Leg").unwrap();
        assert!(strip.x_range().contains(leg.x), "the leg's chip scrolled into view: {leg:?} in {strip:?}");
        let back_in = handle_at("timeline-path:Leg").expect("the leg's step stays on the path");
        h.call("ui.click", json!({"x": back_in.x, "y": back_in.y}));
        h.frames(3);
        assert!(handle_at("timeline:Leg0").is_some() && handle_at("timeline:Arm0").is_none(), "back in to the leg");

        h.app.run("component.activate", json!({"component": ids["Arm"]})).unwrap();
        h.frames(3);
        assert!(handle_at("timeline:Arm0").is_some() && handle_at("timeline:Leg0").is_none(), "switching to the arm follows it");
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(3);
        assert!(handle_at("timeline:Root sketch").is_some() && handle_at("timeline-path:Arm").is_none(), "the root shows the whole design");

        h.app.tree.picked_components = vec![ids["Leg"]];
        h.frames(3);
        assert!(handle_at("timeline:Leg0").is_some() && handle_at("timeline:Root sketch").is_none(), "a component picked in the Browser");
        h.app.tree.picked_components.clear();
        h.frames(3);
        assert!(handle_at("timeline:Root sketch").is_some(), "nothing picked shows the whole design again");
    }

    /// While a component is edited, its features keep their place in the design: the marker
    /// dragged before one rolls back there, past another component's features in between, and a
    /// feature dropped before another goes there in the design.
    #[test]
    fn a_components_history_rolls_and_reorders_in_the_whole_design() {
        use crate::scenario::{handle_at, rect_of};
        let mut h = Harness::new();
        // The path to "All components" comes with the bar's groups (`timeline_scope`).
        h.app.preferences.history_bar_groups = true;
        let a = h.app.run("component.create", json!({"name": "A"})).unwrap()["component"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "A1"})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        let b = h.app.run("component.create", json!({"name": "B"})).unwrap()["component"].as_u64().unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "name": "B1"})).unwrap();
        h.app.run("component.activate", json!({"component": a})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 20, 0], "name": "A2"})).unwrap();
        h.frames(4);
        let names = |h: &Harness| h.app.session.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&h), ["A1", "B1", "A2"]);
        assert!(handle_at("timeline:A1").is_some() && handle_at("timeline:A2").is_some() && handle_at("timeline:B1").is_none());

        let marker = rect_of("history:marker").unwrap();
        let a2 = handle_at("timeline:A2").unwrap();
        h.step(&json!({"drag": [[marker.center().x, marker.center().y], [a2.x - 14.0, a2.y]]})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, Some(2), "B1 stays built, A2 is rolled back");

        h.app.run("timeline.roll_to", json!({})).unwrap();
        h.frames(3);
        let (a1, a2) = (handle_at("timeline:A1").unwrap(), handle_at("timeline:A2").unwrap());
        h.step(&json!({"drag": [[a2.x, a2.y], [a1.x - 14.0, a1.y]]})).unwrap();
        h.settle();
        assert_eq!(names(&h), ["A2", "A1", "B1"], "A2 goes before A1 in the design");

        h.app.run("component.activate", json!({"component": b})).unwrap();
        h.frames(3);
        assert!(handle_at("timeline:B1").is_some() && handle_at("timeline:A1").is_none());
        let all = handle_at("timeline-path:All components").unwrap();
        h.call("ui.click", json!({"x": all.x, "y": all.y}));
        h.frames(3);
        assert!(handle_at("timeline:A1").is_some() && handle_at("timeline:B1").is_some(), "All components shows both");
        assert!(handle_at("timeline-path:B").is_some(), "the way back in to the edited component");
    }

    fn shown(h: &mut Harness) -> Vec<String> {
        h.frames(4);
        crate::scenario::handles("timeline:").into_iter().map(|(n, _)| n).filter(|n| n != "timeline:components").collect()
    }

    /// Two components with a box each: while one is active, the timeline shows only its
    /// feature; the toggle ahead of the features shows the whole design again, and so does the
    /// root (#2880). Without the bar's groups (the default) there is no path, and the whole
    /// design stays shown while another component is edited or picked, until the toggle again.
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
        assert!(crate::scenario::handle_at("timeline-path:All components").is_none(), "no path without the bar's groups");

        let id = |h: &Harness, name: &str| h.app.session.doc.components.iter().find(|c| c.name == name).map(|c| c.id).unwrap();
        let (left, right) = (id(&h, "Left"), id(&h, "Right"));
        h.app.run("component.activate", json!({"component": right})).unwrap();
        shown(&mut h);
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        h.app.run("component.activate", json!({"component": left})).unwrap();
        let names = shown(&mut h);
        assert!(names.contains(&"timeline:RightBox".to_string()), "all still shown while another is edited: {names:?}");
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.tree.picked_components = vec![right];
        let names = shown(&mut h);
        assert!(names.contains(&"timeline:LeftBox".to_string()), "and while one is picked: {names:?}");
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        let names = shown(&mut h);
        assert!(
            names.contains(&"timeline:RightBox".to_string()) && !names.contains(&"timeline:LeftBox".to_string()),
            "the picked one only: {names:?}"
        );
    }

    /// A design opened or made afresh in the tab shows the edited or picked component's
    /// features again, not the whole design chosen for the one before.
    #[test]
    fn a_new_design_in_the_tab_starts_on_the_edited_component() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Part"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "PartBox"})).unwrap();
        shown(&mut h);
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        shown(&mut h);
        assert_eq!(h.app.tree.timeline_scope, crate::timeline_scope::Scope::All);
        h.app.run("file.new", json!({})).unwrap();
        assert_eq!(h.app.tree.timeline_scope, crate::timeline_scope::Scope::Follow, "file.new");
        assert_eq!(h.app.tree.timeline_followed, None, "it follows the new design's own component");
    }

    /// While a component is active, the History controls step through the features the timeline
    /// shows (that component's), not the whole design's (#11430).
    #[test]
    fn the_history_controls_step_through_the_active_components_features() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock": "bottom"})).unwrap();
        h.app.run("component.create", json!({"name": "Left"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new_body", "name": "LeftBox"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "Right"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "RightBox"}))
            .unwrap();
        h.app.run("component.activate", json!({"component": "Left"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 0, 10], "operation": "new_body", "name": "LeftTop"}))
            .unwrap();
        let names: Vec<String> = h.app.session.doc.features.iter().map(|f| f.name.clone()).collect();
        assert_eq!(names, ["LeftBox", "RightBox", "LeftTop"]);
        let features = h.app.session.doc.features.clone();
        h.settle();
        // Positions are the design's: 0 before LeftBox, 2 before LeftTop, None the end.
        for (control, position) in [
            ("back", Some(2)),
            ("back", Some(0)),
            ("back", Some(0)),
            ("forward", Some(2)),
            ("forward", None),
            ("forward", None),
            ("first", Some(0)),
            ("last", None),
        ] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, position, "{control}");
            assert_eq!(h.app.session.doc.features, features, "transport only moves the marker");
        }
        // Show All steps through every feature again.
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        h.settle();
        for (control, position) in [("back", Some(2)), ("back", Some(1)), ("back", Some(0)), ("forward", Some(1))] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, position, "show all: {control}");
        }
        // Back to Left's features with the marker between them (after LeftBox, before RightBox):
        // Forward adds LeftTop, the next feature the bar shows, not only the hidden RightBox.
        h.step(&json!({"click": {"handle": "timeline:components"}})).unwrap();
        h.settle();
        h.step(&json!({"click": {"handle": "history:forward"}})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, None, "forward from between the shown features");
    }

    /// Last, and Forward onto the last slot, with a component active go to the end of the design:
    /// the features after the component's last one belong to other components, and none of them
    /// stays rolled back behind a bar that shows its marker at the end (#15974).
    #[test]
    fn last_with_a_component_active_brings_back_the_later_features_of_other_components() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock": "bottom"})).unwrap();
        h.app.run("component.create", json!({"name": "A"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new_body", "name": "BoxA"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "B"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "operation": "new_body", "name": "BoxB"}))
            .unwrap();
        h.app.run("construct.plane.offset", json!({"base": "XY", "offset": 20, "name": "PlaneB"})).unwrap();
        h.app.run("component.activate", json!({"component": "A"})).unwrap();
        h.settle();
        for (control, position) in [("back", Some(0)), ("last", None), ("back", Some(0)), ("forward", None)] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, position, "{control}");
        }
        // The preference keeps the old stop, just after A's last feature.
        h.app.preferences.history_last_stops_at_component = true;
        for (control, position) in [("back", Some(0)), ("last", Some(1)), ("back", Some(0)), ("forward", Some(1))] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, position, "old stop: {control}");
        }
    }

    /// A component with no features of its own lists nothing, so the controls leave the marker
    /// where it is rather than stepping through other components' features (#11430).
    #[test]
    fn the_history_controls_of_a_component_with_no_features_do_nothing() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock": "bottom"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"})).unwrap();
        h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "name": "Side"})).unwrap();
        h.app.run("component.create", json!({"name": "Empty"})).unwrap();
        h.settle();
        h.app.run("timeline.roll_to", json!({"position": 1})).unwrap();
        h.settle();
        for control in ["first", "back", "forward", "last"] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(1), "{control}");
        }
        // A control that leaves the marker still takes over from playback, as any other step does.
        for control in ["first", "back", "forward", "last"] {
            h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
            h.settle();
            h.app.start("timeline.play");
            h.frames(2);
            assert!(h.app.timeline_playback.is_some(), "playing before {control}");
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.frames(2);
            assert!(h.app.timeline_playback.is_none(), "{control} stops playback");
        }
    }

    /// With the marker outside the shown features (after them, or before them), a control never
    /// moves it the other way: Forward and Last don't roll back over hidden features, and First
    /// and Back don't add them (#11430).
    #[test]
    fn the_history_controls_never_step_against_their_direction() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock": "bottom"})).unwrap();
        h.app.run("component.create", json!({"name": "Left"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new_body", "name": "LeftBox"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "Right"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "RightBox"}))
            .unwrap();
        // Left shows only LeftBox, and the marker is at the end, after RightBox.
        h.app.run("component.activate", json!({"component": "Left"})).unwrap();
        h.settle();
        for control in ["forward", "last"] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, None, "{control} past the shown features");
        }
        // Right shows only RightBox, and the marker is at the start, before LeftBox.
        h.app.run("component.activate", json!({"component": "Right"})).unwrap();
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.settle();
        for control in ["back", "first"] {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(0), "{control} before the shown features");
        }
    }

    /// With "History steps by component" off, the controls step through
    /// the whole design as before, even while a component is active (#11430).
    #[test]
    fn the_history_controls_step_through_the_whole_design_when_the_preference_is_off() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.history_steps_component = false;
        h.app.run("view.history_layout", json!({"dock": "bottom"})).unwrap();
        h.app.run("component.create", json!({"name": "Left"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "operation": "new_body", "name": "LeftBox"})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "Right"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "RightBox"}))
            .unwrap();
        h.app.run("component.activate", json!({"component": "Left"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [0, 0, 10], "operation": "new_body", "name": "LeftTop"}))
            .unwrap();
        h.settle();
        for (control, position) in [("back", Some(2)), ("back", Some(1)), ("back", Some(0)), ("forward", Some(1)), ("last", None), ("first", Some(0))]
        {
            h.step(&json!({"click": {"handle": format!("history:{control}")}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, position, "{control}");
        }
    }

    /// Preferences > Layout shows the preference that keeps Last's old stop (#15974), off by
    /// default; a click turns it on and off.
    #[test]
    fn the_layout_page_has_a_checkbox_for_the_old_stop_of_last() {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"language": "en", "open": "Layout"}));
        let value = |h: &mut Harness| h.call("ui.prefs", json!({}))["result"]["history_last_stops_at_component"].clone();
        assert_eq!(value(&mut h), false, "off by default");
        crate::prefs::reveal_preference(&mut h, "prefs:history_last_stops_at_component");
        h.step(&json!({"click": {"handle": "prefs:history_last_stops_at_component"}})).unwrap();
        assert_eq!(value(&mut h), true, "a click turns it on");
        h.step(&json!({"click": {"handle": "prefs:history_last_stops_at_component"}})).unwrap();
        assert_eq!(value(&mut h), false, "a second click turns it off again");
    }

    /// Preferences > Layout shows "History steps by component" as a
    /// checkbox with its note, and clicking it turns the preference off and on (#11430).
    #[test]
    fn the_layout_page_has_a_checkbox_for_the_history_controls_preference() {
        let mut h = Harness::new();
        h.call("ui.prefs", json!({"language": "en", "open": "Layout"}));
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["history_steps_component"], true, "on by default");
        crate::prefs::reveal_preference(&mut h, "prefs:history_steps_component");
        h.step(&json!({"click": {"handle": "prefs:history_steps_component"}})).unwrap();
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["history_steps_component"], false, "a click turns it off");
        h.step(&json!({"click": {"handle": "prefs:history_steps_component"}})).unwrap();
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["history_steps_component"], true, "a second click turns it on again");
        for locale in ["ar", "de", "he", "ja", "ko", "vi", "zh"] {
            for id in ["prefs.history_steps_component", "prefs.history_steps_component_note"] {
                let text = solidcraft_ui_text::Locale::for_language(locale).text(id);
                assert_ne!(text, solidcraft_ui_text::Locale::for_language("en").text(id), "{locale} translates {id}");
            }
        }
    }

    /// Find in Timeline on the last body of a history longer than the timeline scrolls its
    /// feature into view, in the bottom strip and in the history on the left (#5850).
    #[test]
    fn find_in_timeline_scrolls_both_timelines_to_the_found_feature() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
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
            h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
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

            let state = crate::open_meshing::session_world(&h.app.session);
            let body = state.bodies.first().unwrap();
            let names = solidcraft_engine::doc::naming::face_names(body);
            let index = names.iter().position(|name| solidcraft_engine::doc::persistent_name::made_by(name) == Some(hole)).unwrap();
            let plate = body.name.clone();
            h.app.viewport.hover = Some(crate::viewport::Hit::Face { body: plate.clone(), index, point: solidcore_record::Vec3::ZERO });
            assert_eq!(crate::timeline::hovered_feature(&h.app), Some(hole), "left={left}: hovering the hole's face finds the hole");
            h.app.viewport.hover = Some(crate::viewport::Hit::Vertex { body: plate.clone(), point: solidcore_record::Vec3::ZERO });
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
    fn an_imported_mesh_lights_its_import_feature_in_history() {
        use solidcraft_engine::doc::{Document, MeshData};

        let mut doc = Document::default();
        let mesh = MeshData {
            name: "Scan".into(),
            positions: [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0].into(),
            triangles: [0, 1, 2].into(),
            color: None,
            corner_normals: Default::default(),
        };
        let imported = doc.add_feature(FeatureKind::MeshImport { file: "scan.stl".into(), meshes: vec![mesh] }, Some("Import Scan")).unwrap();
        let mut app = crate::SolveApp::new(solidcraft_engine::Session::new(doc), crate::Services::default());
        let world = crate::open_meshing::session_world(&app.session);
        let body = world.bodies.first().unwrap().name.clone();
        let point = solidcore_record::Vec3::new(0.25, 0.25, 0.0);
        app.viewport.hover = Some(crate::viewport::Hit::Face { body: body.clone(), index: 0, point });
        assert_eq!(super::hovered_feature(&app), Some(imported));
        app.session.selection = vec![solidcraft_engine::Sel::Face { body, index: 0, point }];
        assert_eq!(super::picked_feature(&app), Some(imported));
    }

    #[test]
    fn the_feature_menu_edits_its_profile_sketch_in_both_timelines() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.run("view.history_layout", json!({"dock":if left { "left" } else { "bottom" }})).unwrap();
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
        // Too large for the box: R6 on its four vertical edges is reduced to the full round (5),
        // and the feature warns that it was.
        let edges = json!([[0, 0, 5], [10, 0, 5], [10, 10, 5], [0, 10, 5]]);
        h.app.run("solid.fillet", json!({"edges": edges, "radius": 6, "fit": "reduce", "body": "One"})).unwrap();
        h.frames(2);
        let model = &h.app.session.model;
        let index = super::result_index(&model.results);
        let ids: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
        assert!(ids.len() >= 3 && model.results.iter().any(|r| r.warning.is_some()), "a warned feature among others");
        let warning = ids.last().and_then(|id| model.result(*id)).and_then(|r| r.warning.clone()).unwrap_or_default();
        assert!(warning.contains("reduced"), "the fillet is the warned feature: {warning}");
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
            (worst.map(|w| w.0), worst.and_then(|w| super::feature_tip(&h.app, w.1)))
        };
        // Every folder short enough to be searched, then the same repeated past the length that
        // builds the tables.
        let check = |h: &Harness, folder: &[u64], status: Option<Status>, of: Option<u64>, why: &str| {
            for ids in [folder.to_vec(), folder.repeat(16)] {
                let found = super::worst_status(&h.app, &ids);
                assert_eq!(found, (status, of.and_then(|id| super::feature_tip(&h.app, id))), "{why}, {} ids", ids.len());
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
            // Too large for the box: R6 on its four vertical edges would cross on the 10 mm faces,
            // so it is reduced to what fits (a full round, 5), which the feature's warning says.
            h.app
                .run("solid.fillet", json!({"edges": [[0, 0, 5], [10, 0, 5], [10, 10, 5], [0, 10, 5]], "radius": 6, "fit": "reduce", "body": "One"}))
                .unwrap();
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

    /// Shift+click selects every feature from the last one clicked to this one, folded chips in
    /// between included, and keeps measuring from that feature; Ctrl+Shift+click adds the range.
    #[test]
    fn shift_click_selects_the_features_in_between() {
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        for (i, name) in ["A", "B", "C", "D", "E", "F"].into_iter().enumerate() {
            let corner = [i as f64 * 15.0, 0.0, 0.0];
            h.app
                .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner, "operation": "new_body", "name": name}))
                .unwrap();
        }
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(4);
        let ids =
            |h: &Harness, names: &[&str]| -> Vec<u64> { names.iter().map(|n| h.app.session.doc.find_feature(n).map(|f| f.id).unwrap()).collect() };
        h.step(&json!({"click": {"handle": "timeline:B"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:D"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["B", "C", "D"]));
        h.step(&json!({"click": {"handle": "timeline:A"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["A", "B"]), "the range still starts at B");
        h.step(&json!({"click": {"handle": "timeline:E"}, "shift": true, "ctrl": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["A", "B", "C", "D", "E"]), "Ctrl+Shift adds B to E");

        // C and D folded into a folder chip: a range across it takes them too.
        h.app.run("timeline.group", json!({"name": "Pair", "features": ["C", "D"]})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("timeline:C").is_none(), "C and D are folded");
        h.step(&json!({"click": {"handle": "timeline:B"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:F"}, "shift": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["B", "C", "D", "E", "F"]));
    }

    /// Copy on a feature outside the selection copies that feature alone: the right-click
    /// selects it first.
    #[test]
    fn copy_on_an_unselected_feature_copies_only_it() {
        let mut h = two_boxes(false);
        h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Copy"})).unwrap();
        assert_eq!(copied(&h), ["Second"]);
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["Second"]));
        // Ctrl+click again takes a feature back out of the selection.
        h.step(&json!({"click": {"handle": "timeline:First"}, "ctrl": true})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:First"}, "ctrl": true})).unwrap();
        assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["Second"]));
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

    /// A right-click on a feature outside the selection selects that feature alone, so its
    /// menu acts on what is highlighted; a right-click inside the selection keeps it. The same
    /// in both timelines.
    #[test]
    fn a_right_click_outside_the_selection_selects_that_feature() {
        for left in [false, true] {
            let mut h = three_boxes(left);
            h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
            assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["First", "Second"]), "inside keeps it, left: {left}");
            assert!(h.widget("Delete 2 Selected Features").is_some(), "left: {left}");
            h.step(&json!({"key": "Escape"})).unwrap();

            h.step(&json!({"click": {"handle": "timeline:Third"}, "button": "right"})).unwrap();
            assert_eq!(super::selected_feature_ids(&h.app), ids(&h, &["Third"]), "outside selects it alone, left: {left}");
            assert!(h.widget("Delete 2 Selected Features").is_none(), "left: {left}");
            h.step(&json!({"widget": if left { "Suppress Feature" } else { "Suppress Features" }})).unwrap();
            h.settle();
            let suppressed: Vec<&str> = h.app.session.doc.features.iter().filter(|f| f.suppressed).map(|f| f.name.as_str()).collect();
            assert_eq!(suppressed, ["Third"], "left: {left}");
        }
    }

    /// The menu of several selected features has one Suppress entry, which suppresses them all in
    /// one undo step and says how many; when every selected feature is already suppressed it
    /// offers to unsuppress them instead, and a mixed selection is suppressed. The same in both
    /// timelines (#16245). With Preferences › Layout › Feature menus act on the whole selection
    /// off, the single-feature entry stays beside it and still acts on the right-clicked feature
    /// alone, as before.
    #[test]
    fn the_feature_menu_suppresses_the_selected_features() {
        fn suppressed(h: &Harness) -> Vec<String> {
            h.app.session.doc.features.iter().filter(|f| f.suppressed).map(|f| f.name.clone()).collect()
        }
        fn menu_on_first_and_second(h: &mut Harness) {
            h.app.run("select.clear", json!({})).unwrap();
            h.frames(2);
            h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
            h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
        }
        for (left, rule) in [(false, true), (true, true), (false, false), (true, false)] {
            let mut h = three_boxes(left);
            let single = if left { "Suppress Feature" } else { "Suppress Features" };
            if rule {
                // One feature's menu keeps the single entry.
                h.step(&json!({"click": {"handle": "timeline:Third"}, "button": "right"})).unwrap();
                assert!(h.widget(single).is_some(), "left: {left}");
                h.step(&json!({"key": "Escape"})).unwrap();
            } else {
                crate::prefs::set(&mut h.app, &json!({"feature_menus_act_on_selection": false})).unwrap();
                // The single-feature entry stays as it was beside the other one, and still acts
                // on the right-clicked feature alone.
                menu_on_first_and_second(&mut h);
                h.step(&json!({"widget": single})).unwrap();
                h.settle();
                assert_eq!(suppressed(&h), ["Second"], "left: {left}");
                h.app.run("edit.undo", json!({})).unwrap();
                h.settle();
                assert!(suppressed(&h).is_empty(), "left: {left}");
            }

            menu_on_first_and_second(&mut h);
            assert_eq!(h.widget(single).is_some(), !rule, "the single entry gives way to the one for the selection, left: {left}");
            assert!(h.widget("Unsuppress 2 Selected Features").is_none(), "left: {left}, rule: {rule}");
            h.step(&json!({"widget": "Suppress 2 Selected Features"})).unwrap();
            h.settle();
            assert_eq!(suppressed(&h), ["First", "Second"], "left: {left}, rule: {rule}");
            h.app.run("edit.undo", json!({})).unwrap();
            h.settle();
            assert!(suppressed(&h).is_empty(), "one undo step restores both, left: {left}, rule: {rule}");

            h.app.run("timeline.suppress", json!({"features": ["First", "Second"], "suppressed": true})).unwrap();
            h.settle();
            menu_on_first_and_second(&mut h);
            assert!(h.widget("Suppress 2 Selected Features").is_none(), "left: {left}, rule: {rule}");
            h.step(&json!({"widget": "Unsuppress 2 Selected Features"})).unwrap();
            h.settle();
            assert!(suppressed(&h).is_empty(), "left: {left}, rule: {rule}");

            h.app.run("timeline.suppress", json!({"feature": "First", "suppressed": true})).unwrap();
            h.settle();
            menu_on_first_and_second(&mut h);
            h.step(&json!({"widget": "Suppress 2 Selected Features"})).unwrap();
            h.settle();
            assert_eq!(suppressed(&h), ["First", "Second"], "a mixed selection is suppressed, left: {left}, rule: {rule}");
        }
    }

    /// The reason a one-feature entry gives while several features are selected.
    const SELECT_ONE: &str = "Select a single feature to use this.";

    /// Two boxes, a fillet with a warning on the first one and a sketch, in the bottom History.
    fn boxes_fillet_and_sketch() -> (Harness, u64, u64, u64) {
        let mut h = two_boxes(false);
        // Too large for the box, so it is reduced to what fits, which the feature's warning says.
        h.app
            .run("solid.fillet", json!({"edges": [[0, 0, 5], [10, 0, 5], [10, 10, 5], [0, 10, 5]], "radius": 6, "fit": "reduce", "body": "One"}))
            .unwrap();
        let fillet = h.app.session.doc.features.last().map(|f| f.id).unwrap();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Outline"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(4);
        let ids = ids(&h, &["Second", "Outline"]);
        (h, ids[0], fillet, ids[1])
    }

    fn select_features(h: &mut Harness, features: &[u64]) {
        let items: Vec<serde_json::Value> = features.iter().map(|id| json!({"type": "feature", "id": id})).collect();
        h.app.run("select.set", json!({ "items": items })).unwrap();
        h.frames(2);
    }

    /// Which of `features` a menu entry's parameters name: the ids under `feature`, `features`,
    /// `sketch` and `items`, where the menus put what an entry acts on.
    fn named(params: Option<&serde_json::Value>, features: &[u64]) -> Vec<u64> {
        fn walk(value: &serde_json::Value, features: &[u64], found: &mut Vec<u64>) {
            match value {
                serde_json::Value::Number(n) => found.extend(n.as_u64().filter(|n| features.contains(n))),
                serde_json::Value::Array(list) => list.iter().for_each(|v| walk(v, features, found)),
                serde_json::Value::Object(map) => map.values().for_each(|v| walk(v, features, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for key in ["feature", "features", "sketch", "items"] {
            if let Some(value) = params.and_then(|params| params.get(key)) {
                walk(value, features, &mut found);
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    /// With several features selected, every entry that can be used in the History bar's and
    /// the Browser's feature menu acts on all of them (its parameters name each one) or on none
    /// by name (Paste, Cut); the entries that take one feature are off and say why; one Suppress
    /// entry takes them all; one feature's message is not offered.
    #[test]
    fn a_feature_menu_on_several_features_acts_on_all_of_them_or_says_why() {
        use crate::context_menu::{Target, items};
        let (mut h, second, fillet, sketch) = boxes_fillet_and_sketch();
        let message = |h: &Harness| items(&h.app, &Target::Timeline { feature: fillet }).iter().any(|item| item.id == "ui.copyMessage");
        select_features(&mut h, &[fillet]);
        assert!(message(&h), "the fillet's own menu offers its warning");

        let mut selected = vec![second, fillet];
        selected.sort_unstable();
        select_features(&mut h, &selected);
        for (menu, target) in [("History bar", Target::Timeline { feature: fillet }), ("Browser", Target::Feature { id: fillet })] {
            let entries: Vec<_> = items(&h.app, &target).into_iter().filter(|item| !item.is_sep()).collect();
            // Zoom to Selection names the clicked row, and frames the whole selection when the
            // row is among it (`ui.zoomTo` in context_menu.rs), so its parameters say nothing here.
            for item in entries.iter().filter(|item| item.enabled && item.id != "ui.zoomTo") {
                let acts_on = named(item.params.as_ref(), &selected);
                assert!(
                    acts_on.is_empty() || acts_on == selected,
                    "{menu}: `{}` ({}) acts on {acts_on:?} of the selected {selected:?}",
                    item.label,
                    item.id
                );
            }
            for id in ["ui.editFeature", "ui.rename", "timeline.roll_to"] {
                let item = entries.iter().find(|item| item.id == id).unwrap_or_else(|| panic!("{menu}: {id} stays in the menu"));
                assert!(!item.enabled, "{menu}: {id} takes one feature");
                assert_eq!(item.tip.as_deref(), Some(SELECT_ONE), "{menu}: {id} says why it is off");
            }
            // The fillet has no profile sketch: where the entry is listed it is off as it always
            // was, without the reason, which would promise that one feature turns it on.
            for item in entries.iter().filter(|item| item.id == "ui.editSketch") {
                assert!(!item.enabled && item.tip.is_none(), "{menu}: Edit Profile Sketch");
            }
            let suppress: Vec<_> = entries.iter().filter(|item| item.id == "timeline.suppress").collect();
            assert_eq!(suppress.len(), 1, "{menu}: one Suppress entry");
            assert_eq!(suppress[0].label, "Suppress 2 Selected Features", "{menu}");
            assert_eq!(named(suppress[0].params.as_ref(), &selected), selected, "{menu}");
            let find = entries.iter().find(|item| item.id.starts_with("ui.find")).unwrap();
            assert_eq!(named(find.params.as_ref(), &selected), selected, "{menu}: `{}` finds them all", find.label);
        }
        assert!(!message(&h), "one feature's message is not offered for two");

        // A sketch's Redefine Sketch Plane takes one sketch too.
        select_features(&mut h, &[second, sketch]);
        let redefine = items(&h.app, &Target::Timeline { feature: sketch }).into_iter().find(|item| item.id == "ui.redefineSketch").unwrap();
        assert!(!redefine.enabled);
        assert_eq!(redefine.tip.as_deref(), Some(SELECT_ONE));

        // The sketch's own row in the Browser follows the rule for the entries the feature
        // menus have too: the same sketch behaves the same wherever it is right-clicked.
        let mut both = vec![second, sketch];
        both.sort_unstable();
        let entries = items(&h.app, &Target::Sketch { id: sketch });
        for id in ["ui.editSketch", "ui.redefineSketch", "ui.rename"] {
            let item = entries.iter().find(|item| item.id == id).unwrap();
            assert!(!item.enabled, "sketch row: {id} takes one sketch");
            assert_eq!(item.tip.as_deref(), Some(SELECT_ONE), "sketch row: {id}");
        }
        for id in ["ui.delete", "ui.findTimeline"] {
            let item = entries.iter().find(|item| item.id == id).unwrap();
            assert_eq!(named(item.params.as_ref(), &both), both, "sketch row: {id} takes both");
        }
        // One sketch selected: its menu is as it was.
        select_features(&mut h, &[sketch]);
        let entries = items(&h.app, &Target::Sketch { id: sketch });
        for id in ["ui.editSketch", "ui.redefineSketch", "ui.rename"] {
            let item = entries.iter().find(|item| item.id == id).unwrap();
            assert!(item.enabled && item.tip.is_none(), "one sketch: {id}");
        }
        // With the preference off the sketch row keeps its one-sketch entries on two features.
        crate::prefs::set(&mut h.app, &json!({"feature_menus_act_on_selection": false})).unwrap();
        select_features(&mut h, &[second, sketch]);
        let entries = items(&h.app, &Target::Sketch { id: sketch });
        for id in ["ui.editSketch", "ui.redefineSketch", "ui.rename"] {
            let item = entries.iter().find(|item| item.id == id).unwrap();
            assert!(item.enabled && item.tip.is_none(), "preference off: {id}");
        }
    }

    /// Preferences › Layout › Feature menus act on the whole selection, off: the menus are as they
    /// were. Edit, Rename and Roll back stay usable on the right-clicked feature, its own
    /// Suppress entry stays (beside the one for the selection in the History bar; the Browser
    /// has no such entry), Find takes that feature and its message is offered.
    #[test]
    fn feature_menus_keep_their_one_feature_entries_when_preferences_say_so() {
        use crate::context_menu::{Target, items};
        let (mut h, second, fillet, _) = boxes_fillet_and_sketch();
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["feature_menus_act_on_selection"], true, "on by default");
        crate::prefs::set(&mut h.app, &json!({"feature_menus_act_on_selection": false})).unwrap();
        select_features(&mut h, &[second, fillet]);
        for (menu, target, suppress_entries) in
            [("History bar", Target::Timeline { feature: fillet }, 2), ("Browser", Target::Feature { id: fillet }, 1)]
        {
            let entries = items(&h.app, &target);
            for id in ["ui.editFeature", "ui.rename", "timeline.roll_to"] {
                let item = entries.iter().find(|item| item.id == id).unwrap();
                assert!(item.enabled && item.tip.is_none(), "{menu}: {id}");
                assert_eq!(named(item.params.as_ref(), &[second, fillet]), [fillet], "{menu}: {id} takes the right-clicked feature");
            }
            let suppress: Vec<_> = entries.iter().filter(|item| item.id == "timeline.suppress").collect();
            assert_eq!(suppress.len(), suppress_entries, "{menu}");
            assert_eq!(named(suppress[0].params.as_ref(), &[second, fillet]), [fillet], "{menu}: its own Suppress entry");
            let find = entries.iter().find(|item| item.id.starts_with("ui.find")).unwrap();
            assert_eq!(named(find.params.as_ref(), &[second, fillet]), [fillet], "{menu}");
        }
        assert!(items(&h.app, &Target::Timeline { feature: fillet }).iter().any(|item| item.id == "ui.copyMessage"));

        // The box on the Layout page turns the rule on again.
        h.call("ui.prefs", json!({"language": "en", "open": "Layout"}));
        crate::prefs::reveal_preference(&mut h, "prefs:feature_menus_act_on_selection");
        h.step(&json!({"click": {"handle": "prefs:feature_menus_act_on_selection"}})).unwrap();
        assert_eq!(h.call("ui.prefs", json!({}))["result"]["feature_menus_act_on_selection"], true, "a click turns it on");
    }

    /// Find in Browser on several selected features selects what each of them made, and a
    /// feature that made no body stays selected; Find in Timeline from the Browser keeps them
    /// all selected.
    #[test]
    fn find_from_a_feature_menu_takes_every_selected_feature() {
        use crate::context_menu::{Target, items, run_item};
        use solidcraft_engine::Sel;
        let (mut h, second, _, sketch) = boxes_fillet_and_sketch();
        let first = ids(&h, &["First"])[0];
        select_features(&mut h, &[first, second, sketch]);
        let find = items(&h.app, &Target::Timeline { feature: second }).into_iter().find(|item| item.id == "ui.findBrowser").unwrap();
        run_item(&mut h.app, &find, egui::pos2(0.0, 0.0));
        h.frames(2);
        assert_eq!(
            h.app.session.selection,
            [Sel::Body { name: "One".into() }, Sel::Body { name: "Two".into() }, Sel::Feature { id: sketch }],
            "the bodies of both boxes, and the sketch itself"
        );

        select_features(&mut h, &[first, second]);
        let find = items(&h.app, &Target::Feature { id: second }).into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
        run_item(&mut h.app, &find, egui::pos2(0.0, 0.0));
        h.frames(2);
        assert_eq!(h.app.session.selection, [Sel::Feature { id: first }, Sel::Feature { id: second }]);

        // With the History in the Browser, the Browser's one reveal goes to the row the menu was
        // opened on, whichever place it has in the selection.
        h.app.run("view.history_layout", json!({"dock": "browser"})).unwrap();
        h.frames(3);
        for opened_on in [first, second] {
            select_features(&mut h, &[first, second, sketch]);
            let find = items(&h.app, &Target::Feature { id: opened_on }).into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
            run_item(&mut h.app, &find, egui::pos2(0.0, 0.0));
            assert_eq!(h.app.tree.reveal_feature.as_ref().map(|reveal| reveal.id), Some(opened_on));
            assert_eq!(h.app.tree.timeline_reveal, Some(opened_on));
            h.frames(3);
        }
    }

    /// Find from a feature menu is handed a list of features: ids that name no feature, a list
    /// without the feature itself and values that are no ids are left out, never a crash, and
    /// the features that exist are still found.
    #[test]
    fn find_from_a_feature_menu_survives_ids_that_name_no_feature() {
        use crate::context_menu::run_item;
        use crate::ui_kit::Item;
        use solidcraft_engine::Sel;
        let (mut h, second, _, sketch) = boxes_fillet_and_sketch();
        let first = ids(&h, &["First"])[0];
        for history_in_browser in [false, true] {
            h.app.run("view.history_layout", json!({"dock": if history_in_browser { "browser" } else { "bottom" }})).unwrap();
            h.frames(3);
            for features in [json!([u64::MAX, first, 0, 987_654]), json!([u64::MAX]), json!(["First", null, -1, 1.5, {}]), json!([]), json!("First")]
            {
                let find = Item::action("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": second, "features": features }));
                run_item(&mut h.app, &find, egui::pos2(0.0, 0.0));
                h.frames(2);
                let selected = &h.app.session.selection;
                assert!(selected.contains(&Sel::Feature { id: second }), "{features}: {selected:?}");
                assert!(selected.iter().all(|s| matches!(s, Sel::Feature { id } if [first, second].contains(id))), "{features}: {selected:?}");

                let find = Item::action("ui.findBrowser", "Find in Browser", "list").with(json!({ "features": features }));
                select_features(&mut h, &[sketch]);
                run_item(&mut h.app, &find, egui::pos2(0.0, 0.0));
                h.frames(2);
                let selected = &h.app.session.selection;
                let found = features.as_array().is_some_and(|list| list.contains(&json!(first)));
                let expected = if found { vec![Sel::Body { name: "One".into() }] } else { vec![Sel::Feature { id: sketch }] };
                assert_eq!(selected, &expected, "{features}: an unknown id selects nothing more");
            }
        }
        // A feature id of the list that is missing in the design, alone: nothing to find.
        let find = Item::action("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": u64::MAX, "features": [u64::MAX] }));
        run_item(&mut h.app, &find, egui::pos2(0.0, 0.0));
        h.frames(2);
    }

    /// The vertical History's menu on several selected features: the entries that take one
    /// feature are greyed out with the reason, a click on one does nothing, and the single
    /// Suppress entry gives way to the one for the selection.
    #[test]
    fn the_vertical_history_menu_on_several_features_greys_out_the_one_feature_entries() {
        /// Whether the menu row `label` can be used and what it says on hover; `None` when there is no such row.
        fn row(h: &Harness, label: &str) -> Option<(bool, Option<String>)> {
            let nodes = &h.output.accesskit_update.as_ref()?.nodes;
            let found = nodes.iter().find(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label() == Some(label));
            found.map(|(_, n)| (!n.is_disabled(), n.description().map(str::to_string)))
        }
        let mut h = three_boxes(true);
        h.step(&json!({"click": {"handle": "timeline:First"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "button": "right"})).unwrap();
        for label in ["Edit Feature", "Rename Feature", "Roll Back Here"] {
            assert_eq!(row(&h, label), Some((false, Some(SELECT_ONE.to_string()))), "{label}");
        }
        assert_eq!(row(&h, "Suppress Feature"), None);
        for label in ["Suppress 2 Selected Features", "Copy", "Add to New Folder", "Delete 2 Selected Features"] {
            assert_eq!(row(&h, label).map(|(enabled, _)| enabled), Some(true), "{label}");
        }
        h.step(&json!({"widget": "Roll Back Here"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, None, "a greyed-out entry does nothing");
        assert!(h.app.menu.rename.is_none() && h.app.dialog.is_none());
        h.step(&json!({"key": "Escape"})).unwrap();

        // One feature's menu has them all in use.
        h.step(&json!({"click": {"handle": "timeline:Third"}, "button": "right"})).unwrap();
        for label in ["Edit Feature", "Rename Feature", "Suppress Feature", "Roll Back Here"] {
            assert_eq!(row(&h, label).map(|(enabled, _)| enabled), Some(true), "{label}");
        }
        h.step(&json!({"key": "Escape"})).unwrap();

        // One feature's message is not offered for two.
        h.app
            .run("solid.fillet", json!({"edges": [[0, 0, 5], [10, 0, 5], [10, 10, 5], [0, 10, 5]], "radius": 6, "fit": "reduce", "body": "One"}))
            .unwrap();
        h.frames(4);
        let fillet = format!("timeline:{}", h.app.session.doc.features.last().map(|f| f.name.clone()).unwrap());
        h.step(&json!({"click": {"handle": fillet}, "button": "right"})).unwrap();
        assert_eq!(row(&h, "Copy Warning Message").map(|(enabled, _)| enabled), Some(true));
        h.step(&json!({"key": "Escape"})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Second"}, "ctrl": true})).unwrap();
        h.step(&json!({"click": {"handle": fillet}, "button": "right"})).unwrap();
        assert_eq!(row(&h, "Suppress 2 Selected Features").map(|(enabled, _)| enabled), Some(true));
        assert_eq!(row(&h, "Copy Warning Message"), None);
        h.step(&json!({"key": "Escape"})).unwrap();

        // With the preference off the menu is as it was: every entry in use, on the
        // right-clicked feature, and its message offered.
        crate::prefs::set(&mut h.app, &json!({"feature_menus_act_on_selection": false})).unwrap();
        h.step(&json!({"click": {"handle": fillet}, "button": "right"})).unwrap();
        for label in ["Edit Feature", "Rename Feature", "Suppress Feature", "Suppress 2 Selected Features", "Roll Back Here", "Copy Warning Message"]
        {
            assert_eq!(row(&h, label), Some((true, None)), "{label}");
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
        for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
            let mut h = two_boxes(false);
            h.app.ui.layout = mode;
            h.settle();
            let viewport = h.app.viewport.rect.unwrap();
            if mode == crate::ui_kit::LayoutMode::Floating {
                assert_eq!(rect_of("history:card").unwrap().bottom(), viewport.bottom() - crate::ui_kit::SHELL_GAP);
            } else {
                let strip = rect_of("history:strip").unwrap();
                assert!(viewport.contains_rect(strip), "the strip overlays the viewport");
                assert_eq!(strip.bottom(), viewport.bottom());
                assert_eq!(strip.left(), viewport.left());
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
    fn history_strip_body_accents_the_last_done_chip_and_marker() {
        let mut h = two_boxes(false);
        h.app.ui.tab = "SURFACE".into();
        let t = solidcraft_theme::Tokens::get();
        let accent = crate::workspace::tab_accent(&t, "SURFACE");
        for marker_position in [None, Some(1), Some(0)] {
            h.app.session.doc_mut().marker = marker_position;
            let marker_index = marker_position.unwrap_or(2);
            for selected in [None, Some(0), Some(1)] {
                h.app.session.selection =
                    selected.map(|i| solidcraft_engine::Sel::Feature { id: h.app.session.doc.features[i].id }).into_iter().collect();
                let ctx = crate::fonts::test_context();
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| super::panel_inline(&mut h.app, ui));
                out.textures_delta.clear();
                let texts: Vec<_> = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(text) => Some(text.galley.text()),
                        _ => None,
                    })
                    .collect();
                assert!(texts.is_empty(), "the shared host owns the title");
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

    /// Boxes named `names`, in that order, nothing selected.
    fn boxes(names: &[&str]) -> Harness {
        let mut h = Harness::new();
        for (i, name) in names.iter().enumerate() {
            let corner = [20.0 * i as f64, 0.0, 0.0];
            h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": corner, "operation": "new_body", "name": name})).unwrap();
        }
        h.app.run("select.clear", json!({})).unwrap();
        h.frames(4);
        h
    }

    /// Each folder's name and its features' names.
    fn folders(h: &Harness) -> Vec<(String, Vec<String>)> {
        let doc = &h.app.session.doc;
        let name = |id: &u64| doc.feature(*id).map(|f| f.name.clone()).unwrap_or_default();
        doc.timeline_groups.iter().map(|g| (g.name.clone(), g.features.iter().map(name).collect())).collect()
    }

    fn folder_names(h: &Harness, list: &[(&str, &[&str])]) -> bool {
        folders(h) == list.iter().map(|(n, f)| (n.to_string(), f.iter().map(|x| x.to_string()).collect())).collect::<Vec<_>>()
    }

    /// Whether the menu item `label` on screen is shown off.
    fn disabled(h: &Harness, label: &str) -> Option<bool> {
        let tree = h.output.accesskit_update.as_ref()?;
        tree.nodes
            .iter()
            .rev()
            .find(|(_, n)| n.label().is_some_and(|l| l == label || l.starts_with(&format!("{label} "))))
            .map(|(_, n)| n.is_disabled())
    }

    /// Group Features in a feature's menu folds the selected features, or the one right-clicked
    /// when it isn't selected, into one folded chip whose name is typed next, in one undo step.
    /// Features apart in the history can't be grouped: the item is shown off.
    #[test]
    fn group_features_in_the_feature_menu_makes_one_folded_chip() {
        use crate::scenario::handle_at;
        let mut h = boxes(&["A", "B", "C", "D", "E"]);
        h.step(&json!({"click": {"handle": "timeline:A"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:C"}, "ctrl": true})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:C"}, "button": "right"})).unwrap();
        assert_eq!(disabled(&h, "Group Features"), Some(true), "A and C are apart");
        // The control channel's report of the menu says why too.
        let group_item = |h: &mut Harness| {
            let menu = h.call("ui.menu", json!({}))["result"].clone();
            menu["items"].as_array().into_iter().flatten().find(|i| i["id"] == "ui.groupFeatures").cloned().unwrap_or_default()
        };
        let item = group_item(&mut h);
        assert_eq!(item["enabled"], false, "{item}");
        assert!(item["tip"].as_str().is_some_and(|t| !t.is_empty()), "{item}");
        h.step(&json!({"widget": "Group Features"})).unwrap();
        assert!(h.app.session.doc.timeline_groups.is_empty());
        h.step(&json!({"key": "Escape"})).unwrap();

        // One feature: the one right-clicked, not the selection.
        h.step(&json!({"click": {"handle": "timeline:E"}, "button": "right"})).unwrap();
        assert_eq!(disabled(&h, "Group Features"), Some(false));
        assert!(group_item(&mut h)["tip"].is_null(), "on, it gives no reason");
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget": "Group Features"})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
        assert!(
            matches!(h.app.menu.rename.as_ref().map(|r| &r.what), Some(crate::context_menu::RenameWhat::TimelineGroup(_))),
            "its name is typed next"
        );
        h.step(&json!({"text": "Solo"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        assert!(folder_names(&h, &[("Solo", &["E"])]), "{:?}", folders(&h));
        assert!(handle_at("timeline:E").is_none() && handle_at("timeline-folder-chip:Solo").is_some(), "folded into its chip");

        // Several selected features.
        h.step(&json!({"click": {"handle": "timeline:C"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:D"}, "ctrl": true})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:D"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Group Features"})).unwrap();
        h.step(&json!({"text": "Pair"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        assert!(folder_names(&h, &[("Solo", &["E"]), ("Pair", &["C", "D"])]), "{:?}", folders(&h));
        assert!(handle_at("timeline:C").is_none() && handle_at("timeline:D").is_none() && handle_at("timeline:B").is_some());
    }

    /// A folder's chip opens and folds with a click or its menu, which also renames it, selects
    /// its features and ungroups it. Opened or folded in either History, both show it so.
    #[test]
    fn a_folder_chip_opens_renames_ungroups_and_both_histories_agree() {
        use crate::scenario::handle_at;
        let mut h = boxes(&["A", "B", "C"]);
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("timeline.group", json!({"name": "Pair", "features": ["A", "B"]})).unwrap();
        h.frames(2);
        assert!(handle_at("timeline:A").is_none(), "a folder starts folded");
        h.step(&json!({"click": {"handle": "timeline-folder-chip:Pair"}})).unwrap();
        assert!(handle_at("timeline:A").is_some() && handle_at("timeline:B").is_some(), "a click shows each feature");

        // Folded in the History beside the browser, it is folded here too, and the other way.
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline:A").is_some(), "open there too");
        h.step(&json!({"click": {"handle": "timeline-folder-fold:Pair"}})).unwrap();
        assert!(handle_at("timeline:A").is_none());
        h.app.run("view.history_layout", json!({"dock":"bottom"})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline:A").is_none() && handle_at("timeline-folder-chip:Pair").is_some(), "folded here too");
        h.step(&json!({"click": {"handle": "timeline-folder-chip:Pair"}})).unwrap();
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline:A").is_some(), "opened here, open there");
        h.app.run("view.history_layout", json!({"dock":"bottom"})).unwrap();
        h.frames(4);

        let menu = |h: &mut Harness, name: &str, item: &str| {
            h.step(&json!({"click": {"handle": format!("timeline-folder-chip:{name}")}, "button": "right"})).unwrap();
            h.step(&json!({"widget": item})).unwrap();
        };
        menu(&mut h, "Pair", "Fold Features");
        assert!(handle_at("timeline:A").is_none());
        menu(&mut h, "Pair", "Show Each Feature");
        assert!(handle_at("timeline:A").is_some());
        menu(&mut h, "Pair", "Rename");
        h.step(&json!({"text": "Renamed"})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        assert!(folder_names(&h, &[("Renamed", &["A", "B"])]), "{:?}", folders(&h));
        menu(&mut h, "Renamed", "Select Features");
        let ids: Vec<u64> = ["A", "B"].iter().map(|n| h.app.session.doc.find_feature(n).unwrap().id).collect();
        assert_eq!(super::selected_feature_ids(&h.app), ids);
        menu(&mut h, "Renamed", "Ungroup");
        assert!(h.app.session.doc.timeline_groups.is_empty());
        assert!(handle_at("timeline:A").is_some() && handle_at("timeline-folder-chip:Renamed").is_none());
    }

    /// The marker rolled into a folded folder waits after its chip, and dragged onto the chip it
    /// goes before it; a feature dropped on the folded chip goes before it too, and dropped
    /// between an open folder's features it joins the folder.
    #[test]
    fn the_marker_and_a_dropped_feature_go_around_a_folded_folder() {
        use crate::scenario::{handle_at, rect_of};
        let mut h = boxes(&["A", "B", "C", "D"]);
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("timeline.group", json!({"name": "Mid", "features": ["B", "C"]})).unwrap();
        h.app.run("timeline.roll_to", json!({"position": 2})).unwrap();
        h.frames(2);
        let chip = rect_of("timeline-folder-chip:Mid").unwrap();
        let marker = rect_of("history:marker").unwrap();
        assert!(
            (marker.center().x - chip.right() - 2.0).abs() < 0.01,
            "the marker line sits in the existing gap after the folded chip: {marker:?} {chip:?}"
        );
        let y = chip.center().y;
        h.step(&json!({"drag": [[marker.center().x, marker.center().y], [chip.left() + 2.0, y]]})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, Some(1), "dropped on the chip, the marker goes before the folder");

        h.app.run("timeline.roll_to", json!({})).unwrap();
        h.frames(2);
        let d = handle_at("timeline:D").unwrap();
        let chip = rect_of("timeline-folder-chip:Mid").unwrap();
        h.step(&json!({"drag": [[d.x, d.y], [chip.left() + 2.0, y]]})).unwrap();
        h.settle();
        let order: Vec<&str> = h.app.session.doc.features.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(order, ["A", "D", "B", "C"]);
        assert!(folder_names(&h, &[("Mid", &["B", "C"])]), "{:?}", folders(&h));

        h.step(&json!({"click": {"handle": "timeline-folder-chip:Mid"}})).unwrap();
        let (a, b, c) = (handle_at("timeline:A").unwrap(), handle_at("timeline:B").unwrap(), handle_at("timeline:C").unwrap());
        h.step(&json!({"drag": [[a.x, a.y], [(b.x + c.x) / 2.0, y]]})).unwrap();
        h.settle();
        let order: Vec<&str> = h.app.session.doc.features.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(order, ["D", "B", "A", "C"]);
        assert!(folder_names(&h, &[("Mid", &["B", "A", "C"])]), "dropped inside the open folder, it joins it: {:?}", folders(&h));
    }

    /// An assembly's STEP file (here one cylinder placed twice) shows as one folded chip named
    /// Group #1, holding its Import feature; one rigid group keeps its parts in place, so there
    /// are no joints to show. A file with one object shows its Import feature as before.
    #[test]
    fn a_multi_object_step_insert_shows_as_one_chip() {
        use crate::scenario::{handle_at, handles};
        let corpus = |name: &str| format!("{}/../../tests/step-conformance/{name}.step", env!("CARGO_MANIFEST_DIR"));
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_group_joints": true}))["ok"], true);
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        let one = h.app.run("file.insert_step", json!({"path": corpus("cylinder")})).unwrap();
        let two = h.app.run("file.insert_step", json!({"path": corpus("cylinder_twice"), "folder": true})).unwrap();
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(4);
        let feature = |r: &serde_json::Value| r["name"].as_str().unwrap().to_string();
        assert!(handle_at(&format!("timeline:{}", feature(&one))).is_some(), "one object: its Import feature as before");
        assert_eq!(handles("timeline-folder-chip:").len(), 1, "only the second insert is a folder");
        assert!(handle_at("timeline-folder-chip:Group #1").is_some(), "{two}");
        assert!(handle_at(&format!("timeline:{}", feature(&two))).is_none(), "the Import feature is folded into the chip");
        // One rigid group holds its parts: no joints, so nothing to show after the feature.
        assert!(h.app.session.doc.assembly.joints.is_empty() && h.app.session.doc.assembly.rigid_groups.len() == 1);
        assert!(handles("timeline-joint:").is_empty() && handle_at("timeline:joints").is_none());

        h.step(&json!({"click": {"handle": "timeline-folder-chip:Group #1"}})).unwrap();
        assert!(handle_at(&format!("timeline:{}", feature(&two))).is_some(), "open, the Import feature shows");
        assert!(handle_at("timeline-folder-joints:Group #1").is_none(), "and no joints button");
    }

    /// A folder's joints, shown one by one, stay shown in their own design only: another
    /// design's `Group #1`, numbered the same, keeps its joints grouped, and the first design
    /// shows them again when its tab comes back.
    #[test]
    fn a_step_folders_shown_joints_stay_with_their_design() {
        use crate::scenario::{handle_at, handles};
        let corpus = |name: &str| format!("{}/../../tests/step-conformance/{name}.step", env!("CARGO_MANIFEST_DIR"));
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_group_joints": true}))["ok"], true);
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        // An assembly's import, unlocked and held instead by two rigid joints of a folder with its
        // Import feature.
        let insert = |h: &mut Harness| {
            let reply = h.app.run("file.insert_step", json!({"path": corpus("cylinder_twice")})).unwrap();
            let component = reply["component"].as_u64().unwrap();
            h.app.run("component.unlock", json!({"component": component})).unwrap();
            let doc = &h.app.session.doc;
            let occurrences: Vec<u64> =
                doc.occurrences.iter().filter(|o| o.component == component || doc.component_within(o.parent, component)).map(|o| o.id).collect();
            let joints = h.app.run("joint.rigid_group", json!({"occurrences": occurrences})).unwrap()["joints"].clone();
            let group = h.app.run("timeline.group", json!({"name": "Group #1", "features": [reply["feature"]], "joints": joints})).unwrap();
            h.app.run("component.activate", json!({"component": "root"})).unwrap();
            h.frames(4);
            h.step(&json!({"click": {"handle": "timeline-folder-chip:Group #1"}})).unwrap();
            group["group"].clone()
        };
        let first = insert(&mut h);
        h.step(&json!({"click": {"handle": "timeline-folder-joints:Group #1"}})).unwrap();
        assert_eq!(handles("timeline-joint:").len(), 2, "shown one by one in the first design");

        crate::documents::new_design(&mut h.app);
        assert_eq!(h.app.docs.active, 1, "the second design has a tab of its own");
        let second = insert(&mut h);
        assert_eq!(first, second, "both designs number their folder the same");
        assert!(handle_at("timeline-folder-joints:Group #1").is_some(), "the second design's folder is open");
        assert!(handles("timeline-joint:").is_empty(), "and its joints stay grouped");

        crate::documents::switch(&mut h.app, 0);
        h.frames(4);
        assert_eq!(handles("timeline-joint:").len(), 2, "back in the first design, they show one by one again");
    }

    /// With "Fold the History bar into groups" off, as it is by default, the bar shows each feature
    /// as it always has: a long run of one component's features doesn't fold, a feature folder
    /// starts open (a click still folds it), and a STEP assembly the app inserts gets no `Group #N`
    /// folder. Turned on, the run folds and the app's insert makes one; turned off again, each
    /// feature shows.
    #[test]
    fn with_its_groups_off_the_bar_shows_each_feature_and_a_step_insert_makes_no_folder() {
        use crate::scenario::handle_at;
        let corpus = |name: &str| format!("{}/../../tests/step-conformance/{name}.step", env!("CARGO_MANIFEST_DIR"));
        let mut h = Harness::new();
        assert!(!h.app.preferences.history_bar_groups, "the bar's groups are off by default");
        h.app.run("component.create", json!({"name": "Rig"})).unwrap();
        for i in 0..crate::timeline_fold::FOLD_FROM {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.app.run("timeline.group", json!({"name": "Pair", "features": ["S0", "S1"]})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline-group:Rig").is_none(), "no run folds");
        assert!(handle_at("timeline-folder-chip:Pair").is_some(), "the folder's chip");
        assert!(handle_at("timeline:S0").is_some() && handle_at("timeline:S49").is_some(), "each feature shows, the folder's too");
        h.step(&json!({"click": {"handle": "timeline-folder-chip:Pair"}})).unwrap();
        assert!(handle_at("timeline:S0").is_none() && handle_at("timeline:S2").is_some(), "a click still folds the folder");

        h.app.insert_path(&corpus("cylinder_twice"));
        let (v, _) = h.call_queued("engine.execute", json!({"command": "file.insert_step", "params": {"path": corpus("cylinder_twice")}}));
        assert_eq!(v["ok"], true, "{v}");
        let names: Vec<&str> = h.app.session.doc.timeline_groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Pair"], "neither insert made a folder");
        assert_eq!(h.app.session.doc.assembly.rigid_groups.len(), 2, "their rigid groups are there all the same");

        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(4);
        assert!(handle_at("timeline-group:Rig").is_some(), "turned on, the run folds");
        h.app.insert_path(&corpus("cylinder_twice"));
        let names: Vec<&str> = h.app.session.doc.timeline_groups.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"Group #1"), "turned on, the app's insert asks for a folder: {names:?}");
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": false}))["ok"], true);
        h.frames(4);
        assert!(handle_at("timeline-group:Rig").is_none() && handle_at("timeline:S49").is_some(), "off again, each feature shows");
    }

    /// The bar's rows as the History bar kept them, from the egui context.
    fn kept_rows(h: &Harness) -> std::sync::Arc<super::BarRows> {
        h.ctx
            .data(|data| data.get_temp::<(super::BarSeen, std::sync::Arc<super::BarRows>)>(egui::Id::unique("sc_history_bar_rows")))
            .expect("the History bar kept its rows")
            .1
    }

    /// Everything the bar's rows hold, to compare kept ones with ones built afresh.
    fn rows_summary(rows: &super::BarRows) -> String {
        use super::TimelineKind;
        let kind = |kind: &TimelineKind| match kind {
            TimelineKind::Sketch => "sketch".to_string(),
            TimelineKind::Profile(sketch) => format!("profile of {sketch:?}"),
            TimelineKind::Other => "other".to_string(),
        };
        let features: Vec<_> = rows.features.iter().map(|f| (f.0, &f.1, f.2, f.3, f.4, kind(&f.5))).collect();
        format!("{features:?}\n{:?}\n{:?}\n{}", rows.handles, rows.accents, rows.unique_ids)
    }

    fn rows_built() -> usize {
        super::BAR_ROWS_BUILT.with(std::cell::Cell::get)
    }

    fn rows_checked() -> usize {
        super::BAR_ROWS_CHECKED.with(std::cell::Cell::get)
    }

    /// A frame where nothing changed takes the kept rows without checking them. A selection
    /// counts the revision up: one check, nothing built, and the frames after it check nothing
    /// again. A rename builds the rows once.
    #[test]
    fn a_frame_where_nothing_changed_checks_nothing() {
        let mut h = two_boxes(false);
        h.settle();
        h.frames(2);
        let (checked, built) = (rows_checked(), rows_built());
        h.frames(5);
        assert_eq!((rows_checked(), rows_built()), (checked, built), "nothing changed: no check, nothing built");
        let id = h.app.session.doc.find_feature("First").unwrap().id;
        h.app.run("select.set", json!({"items": [{"type": "feature", "id": id}]})).unwrap();
        h.frames(2);
        assert!(rows_checked() > checked, "a selection counts the revision up: the rows are checked");
        assert_eq!(rows_built(), built, "and not built");
        let checked = rows_checked();
        h.frames(5);
        assert_eq!(rows_checked(), checked, "the key the check held under is kept: no check after it");
        let second = h.app.session.doc.find_feature("Second").unwrap().id;
        h.app.run("timeline.rename", json!({"feature": second, "name": "Lid"})).unwrap();
        h.frames(3);
        assert_eq!(rows_built(), built + 1, "a rename builds the rows once");
    }

    #[test]
    fn the_kept_bar_rows_are_the_ones_built_afresh_after_every_change() {
        let mut h = two_boxes(false);
        let check = |h: &mut Harness, what: &str| {
            h.frames(2);
            let kept = kept_rows(h);
            assert!(kept.describes(&h.app.session.doc), "{what}: kept for the design as it is");
            assert_eq!(rows_summary(&kept), rows_summary(&super::BarRows::of(&h.app)), "{what}");
        };
        check(&mut h, "opened");
        let id = |h: &Harness, name: &str| h.app.session.doc.find_feature(name).unwrap().id;
        let (first, second) = (id(&h, "First"), id(&h, "Second"));
        let third = json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "operation": "new_body", "name": "Third"});
        let steps: [(&str, serde_json::Value); 7] = [
            ("timeline.rename", json!({"feature": second, "name": "Lid"})),
            ("timeline.suppress", json!({"feature": first})),
            ("solid.box", third),
            ("timeline.move_many", json!({"features": [first], "position": 3, "group": null, "active": true})),
            ("timeline.roll_to", json!({"position": 1})),
            ("timeline.roll_to", json!({})),
            ("timeline.delete", json!({"feature": second})),
        ];
        for (command, params) in steps {
            h.app.run(command, params.clone()).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
            check(&mut h, command);
        }
        h.app.run("edit.undo", json!({})).unwrap();
        check(&mut h, "undo");
        assert!(has_chip("Lid"), "the undone delete brings Lid's chip back");
    }

    /// Whether the bar drew a chip for the feature `name` last frame.
    fn has_chip(name: &str) -> bool {
        crate::scenario::handle_at(&format!("timeline:{name}")).is_some()
    }

    /// The bar's rows follow the document alone: the pointer moving, a hover over a chip and a
    /// new selection (which counts the session's revision up, `Session::refresh_display`) leave
    /// them as they are, and a rename builds them once.
    #[test]
    fn the_bar_rows_are_built_again_only_when_the_document_changes() {
        let mut h = two_boxes(false);
        h.frames(2);
        let built = rows_built();
        for k in 0..5 {
            h.frame_with(vec![egui::Event::PointerMoved(egui::pos2(300.0 + 40.0 * k as f32, 300.0))]);
        }
        let chip = crate::scenario::handle_at("timeline:Second").unwrap();
        h.frame_with(vec![egui::Event::PointerMoved(chip)]);
        h.frames(2);
        assert_eq!(rows_built(), built, "a moving pointer and a hover build nothing");
        let id = h.app.session.doc.find_feature("First").unwrap().id;
        h.app.run("select.set", json!({"items": [{"type": "feature", "id": id}]})).unwrap();
        h.frames(2);
        assert_eq!(rows_built(), built, "a selection by command builds nothing");
        h.step(&json!({"click": {"handle": "timeline:Second"}})).unwrap();
        h.frames(2);
        assert_eq!(rows_built(), built, "a selection by a click on a chip builds nothing");
        let second = h.app.session.doc.find_feature("Second").unwrap().id;
        h.app.run("timeline.rename", json!({"feature": second, "name": "Lid"})).unwrap();
        h.frames(3);
        assert_eq!(rows_built(), built + 1, "a rename builds them once");
        assert!(has_chip("Lid") && !has_chip("Second"));
    }

    /// A document replaced by an equal copy at another address (as a sketch drag gives the session
    /// on every frame) keeps the rows: nothing is built and the kept rows are the same ones.
    #[test]
    fn an_equal_document_at_a_new_address_keeps_the_bar_rows() {
        let mut h = two_boxes(false);
        h.frames(2);
        let (kept, built, checked) = (kept_rows(&h), rows_built(), rows_checked());
        let before = std::sync::Arc::as_ptr(&h.app.session.doc);
        h.app.session.doc = std::sync::Arc::new((*h.app.session.doc).clone());
        assert!(!std::ptr::eq(before, std::sync::Arc::as_ptr(&h.app.session.doc)), "the copy is at another address");
        h.frames(2);
        assert!(rows_checked() > checked, "the address changed the key: the rows are checked");
        assert_eq!(rows_built(), built, "nothing is built");
        assert!(std::sync::Arc::ptr_eq(&kept, &kept_rows(&h)), "the kept rows are the same ones");
    }

    /// An edit made in place on the document, at its own address, shows on the next frame,
    /// whichever of the values the bar reads it changes, once the revision is counted up as a
    /// command counts it. A feature added or removed shows even without that: the feature count
    /// is part of the key, as for the vertical History.
    #[test]
    fn an_edit_in_place_reaches_the_bar_on_the_next_frame() {
        let mut h = boxes(&["Alpha", "Beta", "Gamma"]);
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Plan"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [100, 0], "p1": [110, 10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch": "Plan", "distance": 5, "operation": "new", "name": "Pad"})).unwrap();
        h.frames(2);
        type Edit = fn(&mut Vec<solidcraft_engine::doc::Feature>);
        // Each edit, and whether the revision is counted up after it as a command does.
        let edits: [(&str, Edit, bool); 10] = [
            ("a rename", |f| f[1].name = "Renamed".to_string(), true),
            ("a suppress", |f| f[0].suppressed = !f[0].suppressed, true),
            ("a changed id", |f| f[2].id += 1000, true),
            // The same icon and type name: only the profile's sketch changes.
            (
                "a profile's sketch changed",
                |f| {
                    let sketch = f.iter_mut().find_map(|feature| match &mut feature.kind {
                        FeatureKind::Extrude { sketch, .. } => Some(sketch),
                        _ => None,
                    });
                    let Some(sketch) = sketch else { panic!("the design has an extrude") };
                    sketch.0 += 1000;
                },
                true,
            ),
            (
                "a changed kind",
                |f| {
                    let kind = f[3].kind.clone();
                    f[3].kind = std::mem::replace(&mut f[0].kind, kind);
                },
                true,
            ),
            (
                "a feature added",
                |f| {
                    let mut added = f[1].clone();
                    added.id += 5000;
                    added.name = "Added".to_string();
                    f.push(added);
                },
                true,
            ),
            (
                "a feature removed",
                |f| {
                    f.pop();
                },
                true,
            ),
            ("two features swapped", |f| f.swap(0, 1), true),
            (
                "a feature added, the revision as it was",
                |f| {
                    let mut added = f[0].clone();
                    added.id += 7000;
                    added.name = "Quiet".to_string();
                    f.push(added);
                },
                false,
            ),
            (
                "a feature removed, the revision as it was",
                |f| {
                    f.pop();
                },
                false,
            ),
        ];
        for (what, edit, command) in edits {
            // A copy no one else holds can be changed where it is: an edit through
            // `Session::doc_mut` would move a document that others share.
            h.app.session.doc = std::sync::Arc::new((*h.app.session.doc).clone());
            let kept = kept_rows(&h);
            assert!(kept.describes(&h.app.session.doc), "{what}: the kept rows describe the copy");
            // As though a frame had checked them against the copy, so that only the edit can
            // change the key: a frame here could hold the copy elsewhere (the Browser keeps a weak
            // pointer to the document), and it could no longer be changed where it is.
            let rows_id = egui::Id::unique("sc_history_bar_rows");
            let seen = super::BarSeen::of(&h.app);
            h.ctx.data_mut(|data| data.insert_temp(rows_id, (seen, kept.clone())));
            let at = std::sync::Arc::as_ptr(&h.app.session.doc);
            let before = h.app.session.doc.features.clone();
            let Some(doc) = std::sync::Arc::get_mut(&mut h.app.session.doc) else { panic!("{what}: the copy is held elsewhere") };
            edit(&mut doc.features);
            if command {
                h.app.session.revision += 1;
            }
            assert!(std::ptr::eq(at, std::sync::Arc::as_ptr(&h.app.session.doc)), "{what}: made in place");
            assert_ne!(h.app.session.doc.features, before, "{what}: the edit changed the features");
            assert!(!kept.describes(&h.app.session.doc), "{what}: the kept rows no longer describe the document");
            let built = rows_built();
            h.frame();
            assert_eq!(rows_built(), built + 1, "{what}: the next frame builds the rows once");
            assert_eq!(rows_summary(&kept_rows(&h)), rows_summary(&super::BarRows::of(&h.app)), "{what}");
        }
    }

    /// Every feature's chip is published under the name the kept rows hold for it: after a
    /// rename, under the new name only.
    #[test]
    fn the_bar_publishes_each_features_name_from_the_kept_rows() {
        let mut h = boxes(&["Alpha", "Beta", "Gamma"]);
        h.frames(2);
        let names = |h: &Harness| h.app.session.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
        for name in names(&h) {
            assert!(has_chip(&name), "{name} has its chip");
        }
        let beta = h.app.session.doc.find_feature("Beta").unwrap().id;
        h.app.run("timeline.rename", json!({"feature": beta, "name": "Lid"})).unwrap();
        h.frames(2);
        assert_eq!(names(&h), ["Alpha", "Lid", "Gamma"]);
        for name in names(&h) {
            assert!(has_chip(&name), "{name} has its chip after the rename");
        }
        assert!(!has_chip("Beta"), "the old name is gone");
        let kept: Vec<String> = kept_rows(&h).handles.iter().map(|handle| handle.to_string()).collect();
        let published: Vec<String> = crate::scenario::handles("timeline:").into_iter().map(|(name, _)| name).collect();
        assert_eq!(published, kept, "the kept names, in order");
    }

    /// A frame drawn with the rows kept publishes the same chip handles and paints the same bar as
    /// the first frame of the same design, which builds them. `every_chip`: each feature has its
    /// own chip (no run folds), in the design's order.
    fn assert_a_warm_frame_matches_the_first(h: &mut Harness, every_chip: bool, what: &str) {
        h.frames(2);
        let bar = |h: &Harness| {
            let card = h.app.shell.history.expect("the History bar is shown");
            let shapes: Vec<egui::epaint::ClippedShape> =
                h.shapes.iter().filter(|s| card.intersects(s.shape.visual_bounding_rect())).cloned().collect();
            (crate::scenario::handles("timeline"), shapes)
        };
        h.ctx.data_mut(|data| data.remove::<(super::BarSeen, std::sync::Arc<super::BarRows>)>(egui::Id::unique("sc_history_bar_rows")));
        let built = rows_built();
        h.frame();
        assert_eq!(rows_built(), built + 1, "{what}: the first frame builds the rows");
        let first = bar(h);
        h.frame();
        assert_eq!(rows_built(), built + 1, "{what}: the next one keeps them");
        let warm = bar(h);
        assert!(!first.1.is_empty(), "{what}: the bar paints");
        assert_eq!(warm, first, "{what}: the same handles, at the same points, and the same shapes");
        if every_chip {
            let names: Vec<String> = h.app.session.doc.features.iter().map(|f| format!("timeline:{}", f.name)).collect();
            assert_eq!(warm.0.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(), names, "{what}: each feature's chip, in order");
        }
    }

    #[test]
    fn a_warm_frame_publishes_and_paints_the_bar_as_the_first_one_does() {
        assert_a_warm_frame_matches_the_first(&mut boxes(&["Alpha", "Beta", "Gamma", "Delta"]), true, "four boxes");

        // Most steps out of view.
        let mut h = Harness::new();
        for i in 0..120 {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.frames(4);
        let width = h.app.shell.history.unwrap().right();
        assert!(crate::scenario::handle_at("timeline:S119").unwrap().x > width, "the last steps are out of view");
        assert_a_warm_frame_matches_the_first(&mut h, true, "a long history");

        // The bar's groups on: a component's long run folds into one chip (as in
        // `a_long_history_folds_a_components_features_into_one_chip`).
        let mut h = Harness::new();
        assert_eq!(h.call("ui.prefs", json!({"history_bar_groups": true}))["ok"], true);
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Root sketch"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("component.create", json!({"name": "Rig"})).unwrap();
        for i in 0..crate::timeline_fold::FOLD_FROM {
            h.app.run("sketch.create", json!({"plane": "XY", "name": format!("S{i}")})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
        }
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(4);
        assert!(crate::scenario::handle_at("timeline-group:Rig").is_some(), "the run folds");
        assert_a_warm_frame_matches_the_first(&mut h, false, "the bar's groups on");
    }

    /// The bar finds each feature's result where `result_index` and `Model::result` do: in place
    /// when the results line up with the features, else through the table.
    #[test]
    fn the_bar_finds_each_result_as_the_table_does() {
        use super::{BarResults, result_index};
        let mut h = two_boxes(false);
        let edges = json!([[0, 0, 5], [10, 0, 5], [10, 10, 5], [0, 10, 5]]);
        h.app.run("solid.fillet", json!({"edges": edges, "radius": 6, "fit": "reduce", "body": "One"})).unwrap();
        h.frames(2);
        let features = super::BarRows::of(&h.app).features;
        assert!(features.len() >= 3);
        let unique = |features: &[super::BarFeature]| {
            let mut seen = std::collections::HashSet::new();
            features.iter().all(|f| seen.insert(f.0))
        };
        // Each feature's result as the strip found it before: the first of its id in a table.
        let table = |model: &solidcraft_engine::doc::Model, id: u64| {
            let index = result_index(&model.results);
            index.get(&id).and_then(|k| model.results.get(*k).filter(|r| r.id == id).or_else(|| model.result(id))).map(|r| r as *const _)
        };
        let compare = |model: &solidcraft_engine::doc::Model, features: &[super::BarFeature], in_place: bool, what: &str| {
            let found = BarResults::of(features, unique(features), &model.results);
            assert_eq!(matches!(found, BarResults::InPlace), in_place, "{what}");
            for (at, feature) in features.iter().enumerate() {
                let got = found.get(model, at, feature.0).map(|r| r as *const _);
                assert_eq!(got, table(model, feature.0), "{what}: feature {at}");
            }
        };
        let model = h.app.session.model.clone();
        compare(&model, &features, true, "lined up");
        let mut fewer = model.clone();
        fewer.results.truncate(features.len() - 1);
        compare(&fewer, &features, false, "fewer results than features");
        let mut reversed = model.clone();
        reversed.results.reverse();
        compare(&reversed, &features, false, "results in another order");
        // The first feature listed again at the end, its result too: lined up, but the second
        // listing's result is the first one's.
        let mut twice = features.clone();
        twice.push(features[0].clone());
        let mut again = model.clone();
        again.results.push(model.results[0].clone());
        compare(&again, &twice, false, "a feature id twice");
        let mut repeated = model.clone();
        repeated.results[1] = model.results[0].clone();
        compare(&repeated, &features, false, "a result id twice");
        // Results that change after the strip found them lined up (a click on a chip earlier in
        // the frame ran a command): each feature still gets what the table would give it now.
        let lined_up = BarResults::of(&features, true, &model.results);
        assert!(matches!(lined_up, BarResults::InPlace));
        for (changed, what) in [(&fewer, "a result gone"), (&reversed, "results moved"), (&repeated, "a result replaced")] {
            for (at, feature) in features.iter().enumerate() {
                let got = lined_up.get(changed, at, feature.0).map(|r| r as *const _);
                assert_eq!(got, table(changed, feature.0), "{what} after lining up: feature {at}");
            }
        }
    }

    /// A feature id listed twice: the rows say their ids are not unique, so results that line up
    /// with the features by position are still looked up as the table does, and the second
    /// listing gets the first result with its id, not the one at its own place. The bar still
    /// draws such a design.
    #[test]
    fn a_repeated_feature_id_makes_the_bar_look_results_up_by_id() {
        use super::{BarResults, BarRows, result_index};
        let mut h = two_boxes(false);
        h.frames(2);
        assert!(BarRows::of(&h.app).unique_ids, "the plain design's ids are unique");
        // A copy no one else holds, changed where it is: the first feature listed again at the
        // end under another name.
        h.app.session.doc = std::sync::Arc::new((*h.app.session.doc).clone());
        let Some(doc) = std::sync::Arc::get_mut(&mut h.app.session.doc) else { panic!("the copy is held elsewhere") };
        let mut twice = doc.features[0].clone();
        twice.name = "Twice".to_string();
        doc.features.push(twice);
        let rows = BarRows::of(&h.app);
        assert!(!rows.unique_ids, "an id listed twice");
        // One result per listing, lined up by position, the last one a copy of the first.
        let mut model = h.app.session.model.clone();
        model.results.truncate(rows.features.len() - 1);
        let Some(first) = model.results.first().cloned() else { panic!("the design has results") };
        model.results.push(first);
        assert!(rows.features.iter().zip(&model.results).all(|(f, r)| f.0 == r.id), "the results line up by position");
        let found = BarResults::of(&rows.features, rows.unique_ids, &model.results);
        assert!(matches!(found, BarResults::Indexed(_)), "looked up by id");
        let last = rows.features.len() - 1;
        let id = rows.features[last].0;
        let got = found.get(&model, last, id).map(|r| r as *const _);
        let index = result_index(&model.results);
        let table = index.get(&id).and_then(|k| model.results.get(*k)).map(|r| r as *const _);
        assert_eq!(got, table, "what the table gives");
        assert_eq!(got, model.results.first().map(|r| r as *const _), "the first result with the id");
        assert_ne!(got, model.results.get(last).map(|r| r as *const _), "not the one at its own place");
        h.frames(2);
        assert!(has_chip("Twice") && has_chip("First"), "the bar draws both listings");
    }

    #[test]
    fn has_warning_says_what_warning_text_would() {
        use solidcraft_engine::doc::kernel::{ImportIssue, ImportReport, IssueKind};
        let h = two_boxes(false);
        let plain = h.app.session.model.results[0].clone();
        let issue = ImportIssue {
            kind: IssueKind::Repaired,
            item: Some(3),
            part: String::new(),
            paths: Vec::new(),
            faces: 0,
            times: 1,
            detail: "solid #3: …".into(),
        };
        let report = |issues: Vec<ImportIssue>| {
            Some(std::sync::Arc::new(ImportReport { parts: 1, solids: 1, issues, single_root: true, ..Default::default() }))
        };
        let with = |warning: Option<&str>, import_report| {
            let mut result = plain.clone();
            result.warning = warning.map(str::to_string);
            result.import_report = import_report;
            result
        };
        let cases = [
            (with(None, None), false, "neither"),
            (with(Some("reduced"), None), true, "a warning only"),
            (with(None, report(vec![issue.clone()])), true, "a report with issues"),
            (with(None, report(Vec::new())), false, "a report without issues"),
            (with(Some("reduced"), report(vec![issue])), true, "both"),
        ];
        for (result, warned, what) in cases {
            assert_eq!(super::has_warning(&result), warned, "{what}");
            assert_eq!(super::has_warning(&result), super::warning_text(&result).is_some(), "{what}");
        }
    }

    #[test]
    fn browser_row_hover_highlights_feature_and_sketch_in_timeline() {
        for left in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.show_browser = true;
            h.app.ui.show_timeline = true;
            h.app.run("view.history_layout", json!({"dock": if left { "left" } else { "bottom" }})).unwrap();
            h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
            let round = h.app.run("solid.fillet", json!({"edges":[[0,0,10]],"radius":2,"name":"Round"})).unwrap()["feature"].as_u64().unwrap();
            h.app.run("sketch.create", json!({"plane":"XY","name":"MySketch"})).unwrap();
            h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
            h.app.run("sketch.finish", json!({})).unwrap();
            let sketch = h.app.session.doc.find_feature("MySketch").unwrap().id;
            h.app.tree.expanded.insert("c0/features".into());
            h.app.tree.expanded.insert("c0/sketches".into());
            h.frames(2);

            // Hovering a feature row in the browser sets hover_feature and highlights it in the timeline.
            let row_feature = crate::scenario::handle_at(&format!("row:browser-feature:c0:{round}")).unwrap();
            h.step(&json!({"move":[row_feature.x, row_feature.y]})).unwrap();
            assert_eq!(h.app.viewport.hover_feature, Some(round), "left={left}");
            assert_eq!(super::from_view(&h.app), [Some(round), None], "left={left}");

            // Hovering a sketch row in the browser sets hover_feature and highlights it in the timeline.
            let row_sketch = crate::scenario::handle_at("row:MySketch").unwrap();
            h.step(&json!({"move":[row_sketch.x, row_sketch.y]})).unwrap();
            assert_eq!(h.app.viewport.hover_feature, Some(sketch), "left={left}");
            assert_eq!(super::from_view(&h.app), [Some(sketch), None], "left={left}");

            // Moving the cursor away clears the timeline highlight.
            h.step(&json!({"move":[5, 5]})).unwrap();
            assert_eq!(h.app.viewport.hover_feature, None, "left={left}");
            assert_eq!(super::from_view(&h.app), [None, None], "left={left}");
        }
    }
}
