//! The left feature-history panel: named rows, folders, rollback and drag-and-drop.

use crate::ui_kit::BackingTooltipExt as _;
use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::history_rows::{HistoryFeature, HistoryRows};
use crate::timeline::{
    TimelineKind, clipboard_buttons, delete_after_marker_button, delete_button, delete_features, message_button, selected_feature_ids,
};
use crate::{SolveApp, icons};
use solidcraft_theme::Tokens;

mod plan;

use plan::RowPlan;

const FOLDER_INDENT: f32 = 14.0;
const DROP_LEVEL_ANCHOR: f32 = 12.0;
const DROP_BAR_INSET: f32 = 4.0;

/// What a drag in the timeline is moving.
#[derive(Clone, Debug, PartialEq)]
enum Drag {
    Marker,
    Items(DragItems),
}

#[derive(Clone, Debug, PartialEq)]
struct DragItems {
    /// Features selected directly, whose folder membership changes on an indented drop.
    features: Vec<u64>,
    /// Folders selected directly, whose parent changes on an indented drop.
    groups: Vec<u64>,
    /// Pointer position and folder level at drag start. Indentation follows horizontal movement
    /// from this level, independent of whether the row was grabbed by its icon or its text.
    origin_x: f32,
    source_group: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
struct VerticalSlot {
    y: f32,
    position: usize,
    /// Folder containing an item inserted at this boundary without extra indentation.
    group: Option<u64>,
    /// Folder being exited at this boundary; dragging right keeps the item inside it.
    indent_group: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineItem {
    Group(u64),
    Feature(u64),
}

impl TimelineItem {
    fn key(self) -> String {
        match self {
            TimelineItem::Group(id) => format!("g:{id}"),
            TimelineItem::Feature(id) => crate::timeline::anchor_key(id),
        }
    }
}

fn names_fit(width: f32) -> bool {
    let t = Tokens::get();
    width >= crate::ui_kit::history_width(true) - (t.space_8 + t.space_2)
}

/// Side History rows retain their own scroll state without legacy transport chrome.
pub(crate) fn panel_contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    let names = names_fit(ui.available_width());
    let top = ui.cursor().top();
    let output = egui::ScrollArea::vertical()
        .id_salt("sc_left_timeline_scroll")
        .min_scrolled_height(0.0)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE })
        .show(ui, |ui| vertical_features(app, ui, names));
    crate::panel_runtime::report_content_extent(ui, top, output.content_size.y, true);
}

fn nearest_vertical_slot(slots: &[VerticalSlot], y: f32) -> Option<VerticalSlot> {
    slots.iter().min_by(|a, b| (a.y - y).abs().total_cmp(&(b.y - y).abs())).copied()
}

fn valid_folder_target(items: &DragItems, groups: &[solidcraft_engine::doc::TimelineGroup], target: u64) -> bool {
    !items.groups.iter().any(|source| *source == target || group_is_descendant(groups, target, *source))
}

/// Resolve the folder implied by a vertical drop. A folder row is always a target. At a
/// folder boundary, horizontal indentation selects every available ancestor rather than only
/// the timeline root or the deepest folder.
fn vertical_drop_group(
    pointer: egui::Pos2,
    slot: VerticalSlot,
    folder_drop_zones: &[(Rect, u64)],
    groups: &[solidcraft_engine::doc::TimelineGroup],
    panel_left: f32,
    drag_x: f32,
    source_group: Option<u64>,
) -> Option<u64> {
    if let Some((_, group)) = folder_drop_zones.iter().find(|(rect, _)| rect.shrink2(vec2(0.0, 5.0)).contains(pointer)) {
        return Some(*group);
    }
    let (deepest, base, start) = match slot.indent_group {
        Some(deepest) => (deepest, slot.group, drop_level_anchor(panel_left, slot.group, groups)),
        None if slot.group == source_group => {
            let source = source_group?;
            (source, None, drop_level_anchor(panel_left, Some(source), groups))
        }
        None => return slot.group,
    };
    let mut path = Vec::new();
    let mut current = Some(deepest);
    let mut steps = 0usize;
    while current != base {
        let Some(id) = current else { return base };
        path.push(id);
        current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
        steps = steps.saturating_add(1);
        if steps > groups.len() {
            return base;
        }
    }
    path.reverse();
    let mut target = base;
    let relative_indent_x = start + drag_x;
    // A deeply nested row can be grabbed close enough to the panel edge that there is not
    // enough screen space for one full drag step per ancestor. Moving left therefore also
    // honours the pointer's absolute indentation, so the panel edge always reaches root.
    let indent_x = if drag_x < 0.0 { relative_indent_x.min(pointer.x) } else { relative_indent_x };
    for id in path {
        let threshold = drop_level_anchor(panel_left, Some(id), groups);
        if indent_x >= threshold {
            target = Some(id);
        }
    }
    target
}

fn drop_level_anchor(panel_left: f32, group: Option<u64>, groups: &[solidcraft_engine::doc::TimelineGroup]) -> f32 {
    panel_left + DROP_LEVEL_ANCHOR + group.map(|id| (group_depth(groups, id) + 1) as f32 * FOLDER_INDENT).unwrap_or(0.0)
}

fn vertical_drop_bar_left(panel_left: f32, target: Option<u64>, groups: &[solidcraft_engine::doc::TimelineGroup]) -> f32 {
    target.map(|group| panel_left + DROP_BAR_INSET + (group_depth(groups, group) + 1) as f32 * FOLDER_INDENT).unwrap_or(panel_left + DROP_BAR_INSET)
}

/// Paints the marker row and returns its centre height, which splits the slots at its position
/// into before and after the marker.
fn vertical_marker(app: &mut SolveApp, ui: &mut egui::Ui, drag: &mut Option<Drag>) -> f32 {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_12), Sense::drag());
    crate::ui_kit::history_marker(ui.painter(), rect, true, crate::workspace::tab_accent(&t, &app.ui.tab));
    crate::scenario::publish_handle("timeline:rollback", rect.center());
    crate::scenario::publish_rect("history:marker", rect);
    if response.drag_started() {
        *drag = Some(Drag::Marker);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response.backing_on_hover_text(solidcraft_ui_text::tr("ui.history_marker_tip"));
    rect.center().y
}

pub(crate) fn group_for(groups: &[solidcraft_engine::doc::TimelineGroup], feature: u64) -> Option<u64> {
    groups.iter().find(|group| group.features.contains(&feature)).map(|group| group.id)
}

/// The features a drag starting on feature `id`'s row moves: the selection, with the row
/// selected first when it isn't. Shared with the History in the browser (#9565).
pub(crate) fn drag_features(app: &mut SolveApp, id: u64, selected: bool) -> Vec<u64> {
    if !selected {
        select_only(app, TimelineItem::Feature(id));
    }
    selected_feature_ids(app)
}

/// Drops dragged History items at timeline `position`, into folder `group` (`None`: the timeline
/// root), before the history marker when `active`. Shared with the History in the browser (#9565).
pub(crate) fn drop_items(
    app: &mut SolveApp,
    plan: &HistoryRows,
    features: &[u64],
    groups: &[u64],
    group: Option<u64>,
    position: usize,
    active: bool,
) {
    let mut params = json!({"group": group, "position": position});
    // A feature and the sketches listed under it move as one block (#6489); the move's own checks
    // refuse a place where anything would fail.
    let features = plan.with_listed(features);
    if !features.is_empty() {
        params["features"] = json!(features);
    }
    if !groups.is_empty() {
        params["groups"] = json!(groups);
    }
    // Crossing the marker rolls the moved features back or forward, in the same step.
    params["active"] = json!(active);
    let _ = app.run("timeline.move_many", params);
    if let Some(group) = group {
        crate::timeline_folders::set_open(app, group, true);
    }
}

fn selected_drag_items(app: &SolveApp, origin_x: f32, source_group: Option<u64>) -> DragItems {
    DragItems { features: selected_feature_ids(app), groups: app.tree.picked_timeline_groups.clone(), origin_x, source_group }
}

fn drag_press_x(response: &egui::Response, fallback: f32) -> f32 {
    response.ctx.input(|input| input.pointer.press_origin()).map_or(fallback, |origin| origin.x)
}

fn drag_x(items: &DragItems, pointer_x: f32) -> f32 {
    pointer_x - items.origin_x
}

fn select_only(app: &mut SolveApp, item: TimelineItem) {
    app.tree.picked_joints.clear();
    match item {
        TimelineItem::Group(id) => {
            app.tree.picked_timeline_groups.clear();
            app.tree.picked_timeline_groups.push(id);
            let _ = app.run("select.clear", json!({}));
        }
        TimelineItem::Feature(id) => {
            app.tree.picked_timeline_groups.clear();
            let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": id}]}));
        }
    }
    app.tree.timeline_anchor = Some(item.key());
}

fn select_item(app: &mut SolveApp, ui: &egui::Ui, item: TimelineItem, order: &[TimelineItem]) {
    let modifiers = ui.input(|input| input.modifiers);
    if !modifiers.command && !modifiers.ctrl {
        app.tree.picked_joints.clear();
    }
    if modifiers.shift
        && let Some(anchor) = app.tree.timeline_anchor.as_deref()
        && let (Some(from), Some(to)) = (order.iter().position(|entry| entry.key() == anchor), order.iter().position(|entry| *entry == item))
    {
        let (lo, hi) = (from.min(to), from.max(to));
        let range = &order[lo..=hi];
        let mut group_ids: Vec<u64> = range
            .iter()
            .filter_map(|entry| match entry {
                TimelineItem::Group(id) => Some(*id),
                TimelineItem::Feature(_) => None,
            })
            .collect();
        if modifiers.command || modifiers.ctrl {
            group_ids.extend(app.tree.picked_timeline_groups.iter().copied());
            group_ids.sort_unstable();
            group_ids.dedup();
        }
        app.tree.picked_timeline_groups = group_ids;
        let items: Vec<_> = range
            .iter()
            .filter_map(|entry| match entry {
                TimelineItem::Feature(id) => Some(json!({"type": "feature", "id": id})),
                TimelineItem::Group(_) => None,
            })
            .collect();
        if items.is_empty() && !(modifiers.command || modifiers.ctrl) {
            let _ = app.run("select.clear", json!({}));
        } else if !items.is_empty() {
            let _ = app.run("select.set", json!({"items": items, "add": modifiers.command || modifiers.ctrl}));
        }
        return;
    }

    app.tree.timeline_anchor = Some(item.key());
    let additive = modifiers.command || modifiers.ctrl;
    match item {
        TimelineItem::Group(id) => {
            if !additive {
                app.tree.picked_timeline_groups.clear();
                let _ = app.run("select.clear", json!({}));
            }
            if app.tree.picked_timeline_groups.contains(&id) {
                app.tree.picked_timeline_groups.retain(|group| *group != id);
            } else {
                app.tree.picked_timeline_groups.push(id);
            }
        }
        TimelineItem::Feature(id) => {
            if !additive {
                app.tree.picked_timeline_groups.clear();
                let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": id}]}));
            } else {
                let selected = selected_feature_ids(app);
                if selected.contains(&id) {
                    let items: Vec<_> =
                        selected.into_iter().filter(|selected| *selected != id).map(|selected| json!({"type": "feature", "id": selected})).collect();
                    let _ = app.run("select.set", json!({"items": items}));
                } else {
                    let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": id}], "add": true}));
                }
            }
        }
    }
}

fn selection_actions(app: &mut SolveApp, ui: &mut egui::Ui, groups: &[solidcraft_engine::doc::TimelineGroup], feature_menu: bool) {
    let features = selected_feature_ids(app);
    let selected_groups = app.tree.picked_timeline_groups.clone();
    let has_selection = !features.is_empty() || !selected_groups.is_empty();
    if has_selection {
        if crate::ui_kit::menu::button(ui, "Add to New Folder", "feature_group").clicked() {
            let result = app.run("timeline.group", json!({"features": features, "groups": selected_groups}));
            if let Ok(result) = result
                && let Some(group) = result.get("group").and_then(serde_json::Value::as_u64)
            {
                crate::timeline_folders::set_open(app, group, true);
                start_folder_rename(app, group);
            }
            ui.close();
        }
        let grouped = features.iter().any(|feature| group_for(groups, *feature).is_some()) || !selected_groups.is_empty();
        if grouped && crate::ui_kit::menu::button(ui, "Remove from Folder", "feature_group").clicked() {
            let mut params = json!({"group": null});
            if !features.is_empty() {
                params["features"] = json!(features);
            }
            if !selected_groups.is_empty() {
                params["groups"] = json!(selected_groups);
            }
            let _ = app.run("timeline.move_to_group", params);
            ui.close();
        }
    }
    if !app.tree.picked_timeline_groups.is_empty() {
        if !features.is_empty() || !selected_groups.is_empty() {
            ui.separator();
        }
        if crate::ui_kit::menu::button(ui, "Delete Selected Folders", "delete")
            .backing_on_hover_text(solidcraft_ui_text::tr("timeline.delete_folders_tip"))
            .clicked()
        {
            let ids = app.tree.picked_timeline_groups.clone();
            let _ = app.run("timeline.ungroup", json!({"groups": ids}));
            app.tree.picked_timeline_groups.clear();
            ui.close();
        }
        // The folders' own features and those of nested folders, as `timeline.suppress` and
        // `timeline.delete` take them with `groups` (#7219).
        let contents = folder_features(app, &selected_groups);
        let joints = folder_joints(app, &selected_groups);
        let has_contents = !contents.is_empty() || !joints.is_empty();
        let all_suppressed = has_contents
            && contents.iter().all(|id| app.session.doc.feature(*id).is_some_and(|f| f.suppressed))
            && joints.iter().all(|id| app.session.doc.assembly.joints.iter().any(|joint| joint.id == *id && joint.suppressed));
        let label = if all_suppressed { "Unsuppress Selected Folders" } else { "Suppress Selected Folders" };
        if crate::ui_kit::menu::button_enabled(ui, label, "suppress", has_contents)
            .backing_on_hover_text(solidcraft_ui_text::tr("timeline.suppress_folders_tip"))
            .clicked()
        {
            let _ = app.run_heavy("timeline.suppress", json!({"groups": selected_groups, "suppressed": !all_suppressed}));
            ui.close();
        }
        if crate::ui_kit::menu::button(ui, "Delete Folders and Features…", "delete")
            .backing_on_hover_text(solidcraft_ui_text::tr("timeline.delete_folders_with_features_tip"))
            .clicked()
        {
            crate::timeline::delete_folders(app, selected_groups.clone(), contents);
            app.tree.picked_timeline_groups.clear();
            ui.close();
        }
    }
    // A feature's menu has its own Delete, which deletes the selection ([`delete_button`]): a
    // right-click on a feature outside the selection selects that feature first.
    if !features.is_empty() && !feature_menu {
        ui.separator();
        if crate::ui_kit::menu::button(ui, "Delete Selected Features…", "delete").clicked() {
            delete_features(app, features);
            ui.close();
        }
    }
}

/// Delete selected timeline folders while preserving their features and nested folders.
pub fn delete_selected_folders(app: &mut SolveApp) -> bool {
    if app.tree.picked_timeline_groups.is_empty() {
        return false;
    }
    let groups = app.tree.picked_timeline_groups.clone();
    if app.run("timeline.ungroup", json!({"groups": groups})).is_ok() {
        app.tree.picked_timeline_groups.clear();
    }
    true
}

/// Open the folder holding this feature and every folder above it, so its row is shown. A sketch
/// listed under the feature built from it also opens that feature's fold (#6489).
pub fn reveal_feature(app: &mut SolveApp, feature: u64) {
    if let Some(host) = drawn_rows(app, nests(app)).host(feature) {
        app.tree.expanded.insert(sketches_key(host));
    }
    let groups = &app.session.doc.timeline_groups;
    let mut current = group_for(groups, feature);
    let mut folders = Vec::new();
    while let Some(id) = current {
        folders.push(id);
        if folders.len() > groups.len() {
            break;
        }
        current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
    }
    for id in folders {
        crate::timeline_folders::set_open(app, id, true);
    }
}

/// The features of these folders and of the folders nested in them, in timeline order.
fn folder_features(app: &SolveApp, roots: &[u64]) -> Vec<u64> {
    let groups = &app.session.doc.timeline_groups;
    let inside = |feature: u64| {
        group_for(groups, feature).is_some_and(|group| roots.contains(&group) || roots.iter().any(|root| group_is_descendant(groups, group, *root)))
    };
    app.session.doc.features.iter().map(|feature| feature.id).filter(|feature| inside(*feature)).collect()
}

fn folder_joints(app: &SolveApp, roots: &[u64]) -> Vec<u64> {
    let groups = &app.session.doc.timeline_groups;
    groups
        .iter()
        .filter(|group| roots.contains(&group.id) || roots.iter().any(|root| group_is_descendant(groups, group.id, *root)))
        .flat_map(|group| group.joints.iter().copied())
        .collect()
}

fn group_is_descendant(groups: &[solidcraft_engine::doc::TimelineGroup], group: u64, ancestor: u64) -> bool {
    let mut current = groups.iter().find(|candidate| candidate.id == group).and_then(|candidate| candidate.parent);
    let mut steps = 0usize;
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        steps += 1;
        if steps > groups.len() {
            return true;
        }
        current = groups.iter().find(|candidate| candidate.id == id).and_then(|candidate| candidate.parent);
    }
    false
}

/// A parent sorts immediately before its descendants, including folders with only joints.
fn group_path(groups: &[solidcraft_engine::doc::TimelineGroup], id: u64) -> Vec<u64> {
    let mut path = Vec::new();
    let mut current = Some(id);
    for _ in 0..groups.len() {
        let Some(id) = current else { break };
        path.push(id);
        current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
    }
    path.reverse();
    path
}

fn group_depth(groups: &[solidcraft_engine::doc::TimelineGroup], id: u64) -> usize {
    let mut depth = 0;
    let mut current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
    while let Some(parent) = current {
        depth += 1;
        if depth > groups.len() {
            break;
        }
        current = groups.iter().find(|group| group.id == parent).and_then(|group| group.parent);
    }
    depth
}

fn group_is_open(app: &SolveApp, groups: &[solidcraft_engine::doc::TimelineGroup], id: u64) -> bool {
    let mut current = Some(id);
    let mut steps = 0usize;
    while let Some(group_id) = current {
        if !crate::timeline_folders::is_open(app, group_id, true) {
            return false;
        }
        steps += 1;
        if steps > groups.len() {
            return false;
        }
        current = groups.iter().find(|group| group.id == group_id).and_then(|group| group.parent);
    }
    true
}

/// The reference the cached [`RowPlan`] is tested against.
#[cfg(test)]
fn group_ancestors_are_open(app: &SolveApp, groups: &[solidcraft_engine::doc::TimelineGroup], id: u64) -> bool {
    let mut current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
    let mut steps = 0usize;
    while let Some(group_id) = current {
        if !crate::timeline_folders::is_open(app, group_id, true) {
            return false;
        }
        steps += 1;
        if steps > groups.len() {
            return false;
        }
        current = groups.iter().find(|group| group.id == group_id).and_then(|group| group.parent);
    }
    true
}

/// The reference the cached [`RowPlan`] is tested against.
#[cfg(test)]
fn group_first_feature(
    groups: &[solidcraft_engine::doc::TimelineGroup],
    features: &[(u64, String, &'static str, bool, &'static str, TimelineKind)],
    id: u64,
) -> Option<usize> {
    let group = groups.iter().find(|group| group.id == id)?;
    let direct = group.features.iter().filter_map(|feature| features.iter().position(|item| item.0 == *feature)).min();
    let children = groups.iter().filter(|child| child.parent == Some(id)).filter_map(|child| group_first_feature(groups, features, child.id)).min();
    direct.into_iter().chain(children).min()
}

/// The reference the cached [`RowPlan`] is tested against.
#[cfg(test)]
fn group_last_feature(
    groups: &[solidcraft_engine::doc::TimelineGroup],
    features: &[(u64, String, &'static str, bool, &'static str, TimelineKind)],
    id: u64,
) -> Option<usize> {
    let group = groups.iter().find(|group| group.id == id)?;
    let direct = group.features.iter().filter_map(|feature| features.iter().position(|item| item.0 == *feature)).max();
    let children = groups.iter().filter(|child| child.parent == Some(id)).filter_map(|child| group_last_feature(groups, features, child.id)).max();
    direct.into_iter().chain(children).max()
}

/// A folder with only joints shares its nearest feature-bearing ancestor's position.
#[cfg(test)]
fn group_position(
    groups: &[solidcraft_engine::doc::TimelineGroup],
    features: &[(u64, String, &'static str, bool, &'static str, TimelineKind)],
    id: u64,
) -> Option<usize> {
    let mut current = Some(id);
    for _ in 0..groups.len() {
        let id = current?;
        if let Some(first) = group_first_feature(groups, features, id) {
            return Some(first);
        }
        current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
    }
    None
}

/// Whether Sketches: Under their feature is on (#6489).
pub(crate) fn nests(app: &SolveApp) -> bool {
    app.preferences.history_sketches == crate::prefs::HistorySketches::UnderFeature
}

/// The rows the vertical History draws: with `nest`, `history_rows::rows` (each sketch listed
/// under the feature built from it, with reference rows); otherwise every feature a row of its
/// own, in creation order.
fn drawn_rows(app: &SolveApp, nest: bool) -> HistoryRows {
    let doc = &app.session.doc;
    if !nest {
        let sketch = |feature: &solidcraft_engine::doc::Feature| matches!(feature.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. });
        return HistoryRows::in_order(doc.features.iter().map(|feature| (feature.id, sketch(feature))));
    }
    let features: Vec<HistoryFeature> = doc.features.iter().map(HistoryFeature::of).collect();
    crate::history_rows::rows(&features, &doc.timeline_groups, doc.marker, true)
}

/// The fold of a feature's listed sketches; closed until opened.
pub(crate) fn sketches_key(feature: u64) -> String {
    format!("timeline/feature/{feature}/sketches")
}

/// The reference the cached [`RowPlan`] is tested against.
#[cfg(test)]
fn visible_order(
    app: &SolveApp,
    features: &[(u64, String, &'static str, bool, &'static str, TimelineKind)],
    groups: &[solidcraft_engine::doc::TimelineGroup],
    rows: &HistoryRows,
) -> Vec<TimelineItem> {
    let mut order = Vec::new();
    let mut shown = std::collections::BTreeSet::new();
    for (index, (id, ..)) in features.iter().enumerate() {
        let mut at_index: Vec<_> = groups.iter().filter(|group| group_position(groups, features, group.id) == Some(index)).collect();
        at_index.sort_by_key(|group| group_path(groups, group.id));
        for group in at_index {
            if group_ancestors_are_open(app, groups, group.id) && shown.insert(group.id) {
                order.push(TimelineItem::Group(group.id));
            }
        }
        // A listed sketch follows the feature built from it.
        let Some(row) = rows.row_at(index) else { continue };
        let group_id = group_for(groups, *id);
        let open = group_id.is_none_or(|group_id| group_is_open(app, groups, group_id));
        if open {
            order.push(TimelineItem::Feature(*id));
            if !row.listed.is_empty() && crate::browser::is_open(app, &sketches_key(*id), false) {
                order.extend(row.listed.iter().filter(|listed| !listed.reference).map(|listed| TimelineItem::Feature(listed.sketch)));
            }
        }
    }
    let mut trailing: Vec<_> = groups.iter().filter(|group| !shown.contains(&group.id)).collect();
    trailing.sort_by_key(|group| (group_position(groups, features, group.id).unwrap_or(usize::MAX), group_path(groups, group.id)));
    for group in trailing {
        if group_ancestors_are_open(app, groups, group.id) && shown.insert(group.id) {
            order.push(TimelineItem::Group(group.id));
        }
    }
    order
}

/// Start a folder rename typed in its History row: no floating box (#16217).
fn start_folder_rename(app: &mut SolveApp, group: u64) {
    crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::TimelineGroup(group), pos2(0.0, 0.0));
    if let Some(rename) = app.menu.rename.as_mut() {
        rename.inline = true;
    }
}

fn timeline_group_name(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    group: &solidcraft_engine::doc::TimelineGroup,
    rect: Rect,
    offset: f32,
    names: bool,
    faint: bool,
    t: &Tokens,
) -> bool {
    let editing =
        matches!(app.menu.rename.as_ref().map(|rename| &rename.what), Some(crate::context_menu::RenameWhat::TimelineGroup(id)) if *id == group.id);
    if !editing {
        if !names {
            return false;
        }
        ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).text(
            pos2(rect.left() + 40.0 + offset, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &group.name,
            egui::FontId::proportional(solidcraft_theme::Tokens::get().font_body),
            if faint { t.text.gamma_multiply(0.42) } else { t.text },
        );
        return false;
    }

    let Some(mut rename) = app.menu.rename.take() else { return false };
    // F2 starts the rename before it is known where it is drawn; the row owns it from here (#16217).
    if !rename.inline {
        rename.inline = true;
        rename.focused = false;
    }
    let left = rect.left() + 38.0 + offset;
    let edit_rect = Rect::from_min_max(pos2(left, rect.top() + 1.0), pos2(rect.right() - 3.0, rect.bottom() - 1.0));
    let edit_id = ui.scope_id().with(("timeline_group_rename", group.id));
    let response = crate::ui_kit::inline_name(ui, edit_rect, edit_id, &mut rename.text);
    crate::panel_runtime::register_content_focus(ui.ctx(), &response);
    crate::scenario::publish_handle(&format!("timeline-folder-rename:{}", group.name), edit_rect.center());
    if !rename.focused {
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
            let end = rename.text.chars().count();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
            state.store(ui.ctx(), edit_id);
        }
        response.request_focus();
        rename.focused = true;
    }
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let commit = ui.input(|input| input.key_pressed(egui::Key::Enter)) || response.lost_focus();
    if escape {
        // Dropping the shared state cancels the edit and leaves the old name intact.
    } else if commit {
        let _ = crate::context_menu::commit_rename(app, &rename);
    } else {
        app.menu.rename = Some(rename);
    }
    true
}

fn vertical_group_row(
    app: &mut SolveApp,
    names: bool,
    ui: &mut egui::Ui,
    group: &solidcraft_engine::doc::TimelineGroup,
    table: &RowPlan,
    contents: &[u64],
    joints: &[u64],
    order: &[TimelineItem],
    drag: &mut Option<Drag>,
    depth: usize,
    position: usize,
    end_position: usize,
    previous_visible_group: Option<u64>,
    slots: &mut Vec<VerticalSlot>,
    drop_zones: &mut Vec<(Rect, u64)>,
) -> bool {
    let t = Tokens::get();
    let open = crate::timeline_folders::is_open(app, group.id, true);
    let selected = app.tree.picked_timeline_groups.contains(&group.id);
    slots.push(VerticalSlot {
        y: ui.cursor().top(),
        position,
        group: group.parent,
        indent_group: previous_visible_group.filter(|previous| Some(*previous) != group.parent),
    });
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), crate::ui_kit::TREE_ROW_HEIGHT), Sense::click_and_drag());
    drop_zones.push((rect, group.id));
    slots.push(VerticalSlot { y: rect.bottom(), position: if open { position } else { end_position }, group: Some(group.id), indent_group: None });
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            t.radius_sm.saturating_sub(1),
            if selected { solidcraft_theme::TabAccent::current(ui.ctx()).soft } else { t.hover },
        );
    }
    let offset = if names { depth as f32 * 14.0 } else { 0.0 };
    let centre = pos2(rect.left() + 9.0 + offset, rect.center().y);
    crate::ui_kit::disclosure(ui.painter(), centre, open, t.text_dim);
    // A folder whose features are all suppressed looks like a suppressed feature.
    let suppressed = (!contents.is_empty() || !joints.is_empty())
        && contents.iter().all(|id| table.suppressed(*id))
        && joints.iter().all(|id| app.session.doc.assembly.joints.iter().any(|joint| joint.id == *id && joint.suppressed));
    let mut painter = ui.painter().clone();
    if suppressed {
        painter.multiply_opacity(0.42);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 27.5 + offset, rect.center().y), vec2(17.0, 17.0));
    let kind = crate::timeline::folder_kind_icon(app, group.id).unwrap_or("feature_group");
    icons::paint_icon(&painter, icon, kind, f32::from(t.icon.a()) / 255.0);
    if suppressed {
        painter.line_segment([icon.left_bottom(), icon.right_top()], Stroke::new(1.5, t.text_dim));
    }
    // Folded, it carries the worst mark of its features, as a feature chip marks its own (#8592).
    let (status, tip) = if open { (None, None) } else { crate::timeline::worst_status(app, contents) };
    if let Some(status) = status {
        let side = if names { t.space_12 } else { t.space_10 };
        let at =
            if names { pos2(rect.right() - t.space_3 - side, rect.center().y - side / 2.0) } else { icon.right_top() - vec2(side / 2.0, side / 2.0) };
        let mark = Rect::from_min_size(at, vec2(side, side));
        icons::paint_icon(ui.painter(), mark, status.icon(), 1.0);
        crate::scenario::publish_handle(&format!("timeline-folder-status:{}:{}", status.icon(), group.name), mark.center());
    }
    let response = match tip {
        Some(tip) => response.backing_on_hover_text(tip),
        None => response,
    };
    let renaming = timeline_group_name(app, ui, group, rect, offset, names, suppressed, &t);
    crate::scenario::publish_handle(&format!("timeline-folder:{}", group.name), rect.center());
    let fold_rect = Rect::from_min_max(pos2(rect.left() + offset, rect.top()), pos2(rect.left() + offset + 18.0, rect.bottom()));
    crate::scenario::publish_handle(&format!("timeline-folder-fold:{}", group.name), fold_rect.center());
    let fold = ui.interact(fold_rect, ui.scope_id().with(("timeline_group_fold", group.id)), Sense::click());
    if fold.clicked() {
        crate::timeline_folders::set_open(app, group.id, !open);
    } else if !renaming && response.clicked() {
        select_item(app, ui, TimelineItem::Group(group.id), order);
    }
    if !renaming && response.drag_started() {
        if !app.tree.picked_timeline_groups.contains(&group.id) {
            select_only(app, TimelineItem::Group(group.id));
        }
        let origin_x = drag_press_x(&response, rect.center().x);
        *drag = Some(Drag::Items(selected_drag_items(app, origin_x, group.parent)));
    }
    if !renaming && response.double_clicked() {
        start_folder_rename(app, group.id);
    }
    if response.secondary_clicked() && !selected {
        select_only(app, TimelineItem::Group(group.id));
    }
    crate::ui_kit::Menu::context(&response, |ui| {
        if crate::ui_kit::menu::button(ui, "Rename Folder", "rename").clicked() {
            start_folder_rename(app, group.id);
            ui.close();
        }
        crate::timeline::edit_holes_item(app, ui, group.id);
        ui.separator();
        selection_actions(app, ui, &table.groups, false);
    });
    open
}

fn folder_joint_rows(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    group: &solidcraft_engine::doc::TimelineGroup,
    groups: &[solidcraft_engine::doc::TimelineGroup],
) {
    if !group.joints.is_empty() && group_is_open(app, groups, group.id) {
        ui.scope_builder(egui::UiBuilder::new().id_salt(("folder_joints", group.id)), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                ui.add_space((group_depth(groups, group.id) + 1) as f32 * FOLDER_INDENT);
                ui.vertical(|ui| crate::dialogs_assembly::timeline_joint_rows(app, ui, Some(&group.joints)));
            });
        });
    }
}

/// The fold-arrow column of a history row. With sketches listed under their features every row
/// keeps it, so the icons of rows with and without a fold line up.
const FOLD_COLUMN: f32 = 16.0;

/// Which row of a timeline feature the vertical history draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    /// A row of its own, in timeline order.
    Own,
    /// A sketch one level in, under the feature built from it (#6489).
    Nested,
    /// A row under the feature at timeline position `parent` that points at a sketch listed under
    /// an earlier feature: it selects and lights up that sketch, and isn't dragged.
    Reference { parent: usize },
}

/// What the rows of one frame of the vertical history share.
struct Rows<'a> {
    names: bool,
    /// Sketches: Under their feature, while the History shows names (#6489).
    under_feature: bool,
    /// The design's rows, built once per change.
    table: &'a RowPlan,
    order: &'a [TimelineItem],
    marker: usize,
    /// The features picked in the design, collected once for the column.
    selected: std::collections::HashSet<u64>,
    /// Each feature's result, found once for the column.
    results: std::collections::HashMap<u64, usize>,
    /// Find in Timeline: the feature to scroll into view this frame (#5850).
    reveal: Option<u64>,
    /// The feature whose body is under the pointer in the 3D view.
    from_view: [Option<u64>; 2],
    /// A sketch that has a reference row and one of whose rows was hovered last frame: all its
    /// rows light up.
    linked_hover: Option<u64>,
    drag: Option<Drag>,
    hovered: Option<u64>,
    edit: Option<u64>,
    /// Each sketch's solve state, read once for the column (#7245).
    sketch_states: crate::sketch_state::SketchStates,
}

/// Draw the vertical history column's feature rows, folders, markers, and drag-and-drop targets,
/// updating selection, edits, and viewport feature hover.
pub(crate) fn vertical_features(app: &mut SolveApp, ui: &mut egui::Ui, names: bool) {
    let t = Tokens::get();
    // Show origin and bodies at the top (#6490): the browser's Bodies folder and Origin rows lead
    // the history when it is wide enough for names. They are not places in the history: nothing
    // drags or drops among them, and the features start below them.
    let features_top = (names && app.preferences.timeline_origin_bodies).then(|| {
        crate::browser::history_rows(app, ui);
        ui.add_space(t.space_3);
        t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top(), t.border);
        ui.add_space(t.space_3);
        ui.cursor().top()
    });
    let among_features = |y: f32| features_top.is_none_or(|top| y >= top);
    ui.spacing_mut().item_spacing.y = if names { t.space_2 } else { t.space_4 };
    // The narrow icon strip has no room for nesting: it keeps the creation order.
    let under_feature = names && nests(app);
    let table = RowPlan::cached(app, ui, under_feature);
    let folds = table.folds(app);
    let order = table.order(ui, &folds);
    let (features, groups, plan) = (&table.features, &table.groups, &table.rows);
    let n = features.len();
    let marker = app.session.doc.marker.unwrap_or(n);
    let from_view = [crate::timeline::hovered_feature(app), crate::timeline::picked_feature(app)];
    let drag_id = ui.scope_id().with("tl_vertical_drag");
    let mut rows = Rows {
        sketch_states: crate::sketch_state::SketchStates::of(app),
        names,
        under_feature,
        table: &table,
        order: &order,
        marker,
        selected: app
            .session
            .selection
            .iter()
            .filter_map(|selection| match selection {
                solidcraft_engine::Sel::Feature { id } => Some(*id),
                _ => None,
            })
            .collect(),
        results: crate::timeline::result_index(&app.session.model.results),
        reveal: app.tree.timeline_reveal,
        from_view,
        linked_hover: app.viewport.hover_feature.filter(|id| plan.referenced(*id)),
        drag: ui.data(|data| data.get_temp(drag_id)),
        hovered: None,
        edit: None,
    };
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let mut slots = Vec::new();
    let mut folder_drop_zones = Vec::new();
    let mut shown_groups = std::collections::BTreeSet::new();
    let mut previous_visible_group = None;
    let mut marker_y = f32::INFINITY;

    for index in 0..n {
        let group_id = table.group_of.get(index).copied().flatten();
        let at_index = table.starting.get(index).map_or(&[][..], Vec::as_slice);
        let marker_before_group = index == marker && !at_index.is_empty();
        if marker_before_group {
            let group = at_index.first().and_then(|group| groups.get(*group)).and_then(|group| group.parent);
            let indent_group = previous_visible_group.filter(|previous| Some(*previous) != group);
            slots.push(VerticalSlot { y: ui.cursor().top(), position: index, group, indent_group });
            marker_y = vertical_marker(app, ui, &mut rows.drag);
        }
        let mut rendered_group_header = false;
        for &at in at_index {
            let Some(group) = groups.get(at) else { continue };
            if table.ancestors_open(&folds, group.id) && shown_groups.insert(group.id) {
                vertical_group_row(
                    app,
                    names,
                    ui,
                    group,
                    &table,
                    table.contents.get(at).map_or(&[][..], Vec::as_slice),
                    table.joints.get(at).map_or(&[][..], Vec::as_slice),
                    &order,
                    &mut rows.drag,
                    table.depth.get(at).copied().unwrap_or(0),
                    index,
                    table.end.get(at).copied().unwrap_or(index),
                    previous_visible_group,
                    &mut slots,
                    &mut folder_drop_zones,
                );
                if table.first.get(at).copied().flatten().is_none() {
                    folder_joint_rows(app, ui, group, groups);
                }
                rendered_group_header = true;
            }
        }
        let group_open = group_id.is_none_or(|id| table.is_open(&folds, id));
        let indent_group = (!rendered_group_header).then_some(previous_visible_group).flatten().filter(|previous| Some(*previous) != group_id);
        if index == marker && !marker_before_group {
            slots.push(VerticalSlot { y: ui.cursor().top(), position: index, group: group_open.then_some(group_id).flatten(), indent_group });
            marker_y = vertical_marker(app, ui, &mut rows.drag);
        }
        // A sketch listed under a feature is drawn after that feature's row.
        if group_open && let Some(row) = plan.row_at(index) {
            slots.push(VerticalSlot { y: ui.cursor().top(), position: row.slot.min(index), group: group_id, indent_group });
            let indent = group_id.map(|id| (table.depth_of(id) + 1) as f32 * FOLDER_INDENT).unwrap_or(0.0);
            feature_row(app, ui, &mut rows, index, indent, RowKind::Own);
            previous_visible_group = group_id;
        }
        for group in table.ending.get(index).map_or(&[][..], Vec::as_slice).iter().filter_map(|group| groups.get(*group)) {
            folder_joint_rows(app, ui, group, groups);
        }
    }
    for &at in &table.trailing {
        let Some(group) = groups.get(at) else { continue };
        if !shown_groups.contains(&group.id) && table.ancestors_open(&folds, group.id) && shown_groups.insert(group.id) {
            vertical_group_row(
                app,
                names,
                ui,
                group,
                &table,
                table.contents.get(at).map_or(&[][..], Vec::as_slice),
                table.joints.get(at).map_or(&[][..], Vec::as_slice),
                &order,
                &mut rows.drag,
                table.depth.get(at).copied().unwrap_or(0),
                n,
                n,
                previous_visible_group,
                &mut slots,
                &mut folder_drop_zones,
            );
            folder_joint_rows(app, ui, group, groups);
        }
    }
    slots.push(VerticalSlot { y: ui.cursor().top(), position: n, group: None, indent_group: previous_visible_group });
    if marker >= n {
        marker_y = vertical_marker(app, ui, &mut rows.drag);
        // Below the marker at the end: a feature dropped here is rolled back.
        slots.push(VerticalSlot { y: ui.cursor().top(), position: n, group: None, indent_group: previous_visible_group });
    }
    crate::dialogs_assembly::timeline_joints_vertical(app, ui);

    let mut drag = rows.drag.take();
    if drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // The marker dropped where it already is needs no bar: the marker shows the spot.
    if let (Some(active), Some(pointer)) = (drag.as_ref(), pointer)
        && among_features(pointer.y)
        && let Some(slot) = nearest_vertical_slot(&slots, pointer.y)
        && !(matches!(active, Drag::Marker) && slot.position == marker.min(n))
    {
        let folder_target = match active {
            Drag::Items(items) => {
                let panel_left = ui.min_rect().left();
                vertical_drop_group(pointer, slot, &folder_drop_zones, groups, panel_left, drag_x(items, pointer.x), items.source_group)
                    .filter(|target| valid_folder_target(items, groups, *target))
            }
            Drag::Marker => None,
        };
        let hovered_folder = folder_target.and_then(|target| {
            folder_drop_zones.iter().find(|(rect, group)| *group == target && rect.shrink2(vec2(0.0, 5.0)).contains(pointer)).map(|(rect, _)| *rect)
        });
        if let Some(rect) = hovered_folder {
            ui.painter().rect_stroke(rect.shrink(1.0), t.radius_sm.saturating_sub(1), Stroke::new(2.0, t.sketch_accent), egui::StrokeKind::Inside);
        } else {
            let left = vertical_drop_bar_left(ui.min_rect().left(), folder_target, groups);
            crate::ui_kit::history_drop_bar(
                ui.painter(),
                Rect::from_min_max(pos2(left, slot.y), pos2(ui.max_rect().right() - 5.0, slot.y)),
                true,
                if matches!(active, Drag::Marker) { solidcraft_theme::TabAccent::current(ui.ctx()).color } else { t.sketch_accent },
            );
        }
    }
    if ui.input(|input| input.pointer.any_released())
        && let Some(active) = drag.take()
        && let Some(pointer) = pointer.filter(|pointer| among_features(pointer.y))
        && let Some(slot) = nearest_vertical_slot(&slots, pointer.y)
    {
        match active {
            Drag::Marker => {
                let _ = app.run_heavy("timeline.roll_to", if slot.position >= n { json!({}) } else { json!({"position": slot.position}) });
            }
            Drag::Items(items) => {
                let panel_left = ui.min_rect().left();
                let folder_target =
                    vertical_drop_group(pointer, slot, &folder_drop_zones, groups, panel_left, drag_x(&items, pointer.x), items.source_group);
                let valid_target = folder_target.is_none_or(|target| valid_folder_target(&items, groups, target));
                if valid_target {
                    drop_items(app, plan, &items.features, &items.groups, folder_target, slot.position, slot.y < marker_y);
                }
            }
        }
    }
    ui.data_mut(|data| match drag {
        Some(active) => {
            data.insert_temp(drag_id, active.clone());
        }
        None => data.remove::<Drag>(drag_id),
    });
    crate::semantic_content::set_timeline_feature_hover(app, rows.hovered);
    if let Some(feature) = rows.edit {
        app.edit_feature(feature);
    }
}

/// One feature's row at timeline position `index`, `indent` points in, and for a feature that
/// sketches are listed under, those sketches' rows when its fold is open.
fn feature_row(app: &mut SolveApp, ui: &mut egui::Ui, rows: &mut Rows, index: usize, indent: f32, row_kind: RowKind) {
    let (table, names) = (rows.table, rows.names);
    let (features, plan) = (&table.features, &table.rows);
    let Some((id, name, _, _, _, kind)) = features.get(index) else { return };
    let id = *id;
    let group_id = table.group_of.get(index).copied().flatten();
    let reference = matches!(row_kind, RowKind::Reference { .. });
    let listed =
        (row_kind == RowKind::Own).then(|| plan.row_at(index)).flatten().map(|row| row.listed.as_slice()).filter(|listed| !listed.is_empty());
    let has_patterns = !reference && matches!(kind, TimelineKind::Sketch) && table.has_patterns.get(index).copied().unwrap_or(false);
    // One fold per row: a feature's listed sketches, or a sketch's patterns.
    let fold = match listed {
        Some(_) => Some((sketches_key(id), false)),
        None => has_patterns.then(|| (format!("timeline/sketch/{id}/patterns"), true)),
    };
    let fold_open = fold.as_ref().is_some_and(|(key, default)| crate::browser::is_open(app, key, *default));
    let style = if names { crate::ui_kit::HistoryStyle::Names } else { crate::ui_kit::HistoryStyle::Icons };
    // `allocate_exact_size` in two steps, so a row outside the view takes its place and no more.
    let size = vec2(ui.available_width(), style.height());
    let (row_id, frame) = ui.allocate_space(size);
    let row = ui.layout().align_size_within_rect(size, frame);
    let rect =
        if style == crate::ui_kit::HistoryStyle::Icons { Rect::from_center_size(row.center(), vec2(style.height(), style.height())) } else { row };
    let parent = match row_kind {
        RowKind::Reference { parent } => features.get(parent),
        _ => None,
    };
    match parent {
        Some(parent) => crate::scenario::publish_handle(&format!("timeline-reference:{}:{name}", parent.1), rect.center()),
        None => {
            crate::scenario::publish_rect(&format!("history:feature:{name}"), rect);
            crate::scenario::publish_handle(&format!("timeline:{name}"), rect.center());
            if rows.reveal == Some(id) && crate::semantic_content::scroll_acknowledged(ui, rect) {
                app.tree.timeline_reveal = None;
            }
        }
    }
    let fold_rect = Rect::from_min_size(rect.left_top() + vec2(indent, 0.0), vec2(FOLD_COLUMN, rect.height()));
    if fold.is_some() {
        let handle = if listed.is_some() { "timeline-sketches-fold" } else { "timeline-sketch-fold" };
        crate::scenario::publish_handle(&format!("{handle}:{name}"), fold_rect.center());
    }
    // A row scrolled out of the History is neither drawn nor interactive: a long history drew
    // and hit-tested every row each frame. Its slot still places the marker and drops, and its
    // handles stay published. One being found, dragged or focused, or under an open menu, stays.
    let shown = ui.clip_rect().intersects(frame)
        || rows.reveal == Some(id)
        || ui.ctx().is_being_dragged(row_id)
        || ui.memory(|memory| memory.has_focus(row_id))
        || egui::Popup::is_any_open(ui.ctx())
        || draw_every_row();
    if shown {
        let sense = if reference { Sense::click() } else { Sense::click_and_drag() };
        let mut response = ui.interact(frame, row_id, sense);
        response.set_intrinsic_size(size);
        let places = RowPlaces { rect, fold_rect, indent };
        feature_row_body(app, ui, rows, index, row_kind, fold.as_ref(), fold_open, places, response);
    }
    if names && has_patterns && fold_open {
        let depth = group_id.map_or(1, |group| table.depth_of(group) + 2) + usize::from(row_kind == RowKind::Nested);
        crate::sketch_pattern_tree::rows(app, ui, id, depth, "timeline", None, false);
    }
    if let Some(listed) = listed.filter(|_| names && fold_open) {
        for child in listed {
            let child_kind = if child.reference { RowKind::Reference { parent: index } } else { RowKind::Nested };
            feature_row(app, ui, rows, child.index, indent + FOLDER_INDENT, child_kind);
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Tests compare the History drawn row by row with the one that leaves out what is out of view.
    static DRAW_EVERY_ROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// How many feature rows were drawn and made interactive.
    static ROWS_DRAWN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn draw_every_row() -> bool {
    DRAW_EVERY_ROW.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn draw_every_row() -> bool {
    false
}

/// Where a feature row in view is drawn: its chip, its fold arrow, and how far it is indented.
#[derive(Clone, Copy)]
struct RowPlaces {
    rect: Rect,
    fold_rect: Rect,
    indent: f32,
}

/// The drawing and interaction of a feature row that is in view.
fn feature_row_body(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    rows: &mut Rows,
    index: usize,
    row_kind: RowKind,
    fold: Option<&(String, bool)>,
    fold_open: bool,
    places: RowPlaces,
    response: egui::Response,
) {
    let RowPlaces { rect, fold_rect, indent } = places;
    #[cfg(test)]
    ROWS_DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
    let t = Tokens::get();
    let (table, names, order, marker) = (rows.table, rows.names, rows.order, rows.marker);
    let (features, plan, groups) = (&table.features, &table.rows, &table.groups);
    let Some((id, name, icon, suppressed, ty, kind)) = features.get(index) else { return };
    let id = *id;
    let group_id = table.group_of.get(index).copied().flatten();
    let reference = matches!(row_kind, RowKind::Reference { .. });
    let sketch_state = matches!(kind, TimelineKind::Sketch).then(|| rows.sketch_states.get(id)).flatten();
    let on_fold = fold.is_some() && response.interact_pointer_pos().is_some_and(|p| fold_rect.contains(p));
    // An index that no longer holds this id (a command run from an earlier row) is searched for.
    let result = rows
        .results
        .get(&id)
        .and_then(|at| app.session.model.results.get(*at).filter(|result| result.id == id).or_else(|| app.session.model.result(id)));
    let elapsed = result.map(|r| r.ms);
    let error = result.and_then(|result| result.error.clone());
    let warning = result.and_then(crate::timeline::warning_text);
    let status = (error.is_some() || warning.is_some()).then(|| crate::timeline::feature_status_text(name, ty, result));
    let rolled = match row_kind {
        RowKind::Reference { parent } => parent >= marker,
        RowKind::Own | RowKind::Nested => index >= marker,
    };
    let selected = rows.selected.contains(&id);
    // Folder and sketch-pattern disclosure targets retain their original handles. Listing sketches
    // under their features keeps the fold column on every row, so the icons line up.
    let column = if rows.under_feature || fold.is_some() { FOLD_COLUMN } else { 0.0 };
    let inset = if names { indent + column } else { 0.0 };
    let chip = Rect::from_min_max(rect.min + vec2(inset.min(rect.width()), 0.0), rect.max);
    let style = if names { crate::ui_kit::HistoryStyle::Names } else { crate::ui_kit::HistoryStyle::Icons };
    // Dragged features take the colour of their drop bar, with the sketches listed under them.
    let dragged = matches!(&rows.drag, Some(Drag::Items(items)) if items.features.contains(&id)
        || (row_kind == RowKind::Nested && plan.host(id).is_some_and(|host| items.features.contains(&host))));
    crate::ui_kit::HistoryChip {
        icon,
        name,
        icon_accent: crate::workspace::feature_icon_accent(ty, icon),
        selected,
        hovered: response.hovered() || rows.from_view.contains(&Some(id)) || rows.linked_hover == Some(id),
        dim: rolled,
        suppressed: *suppressed,
        error: error.is_some(),
        warning: warning.is_some(),
        reference,
        ink: sketch_state.and_then(|state| state.ink(&t)),
        accent: crate::workspace::tab_accent(&t, if dragged { "SKETCH" } else { &app.ui.tab }),
    }
    .paint(ui.painter(), chip, style);
    if fold.is_some() {
        crate::ui_kit::disclosure(ui.painter(), fold_rect.center(), fold_open, t.text_dim);
    }
    if response.hovered() {
        rows.hovered = Some(id);
    }
    if response.drag_started() && !on_fold {
        let features = drag_features(app, id, selected);
        let origin_x = drag_press_x(&response, rect.center().x);
        rows.drag = Some(Drag::Items(DragItems { features, ..selected_drag_items(app, origin_x, group_id) }));
    }
    let listed_under = plan.host(id).and_then(|host| table.name(host));
    let mut tooltip = egui::Tooltip::for_enabled(&response);
    tooltip.popup = tooltip.popup.align(egui::RectAlign::RIGHT_START);
    crate::ui_kit::Container::show_tooltip(response.id, tooltip, |ui| {
        let summary = app
            .session
            .doc
            .features
            .get(index)
            .and_then(|f| f.kind.expressions().first().map(ToString::to_string))
            .unwrap_or_else(|| elapsed.map_or_else(String::new, |ms| format!("{ms:.1} ms")));
        let kind = solidcraft_ui_text::tr_label(ty.strip_suffix("Feature").unwrap_or(ty));
        let mut tip = format!("{name} · {kind} · {summary}");
        if let Some(status) = &status {
            tip.push_str(&format!("\n{status}"));
        }
        // Greyed out says only "not shown": say why (#8100).
        if rolled {
            tip.push_str(&format!("\n{}", solidcraft_ui_text::tr("timeline.feature_rolled_back")));
        }
        if let Some(state) = sketch_state {
            tip.push_str(&format!("\n{}", state.text()));
        }
        crate::ui_kit::text(ui, &tip, crate::ui_kit::Text::Value("history", false));
        // A value row is one line: where the sketch is listed gets a row of its own.
        if let Some(host) = listed_under.filter(|_| reference) {
            let listed = solidcraft_ui_text::format("ui.history_sketch_listed_under", &[("feature", host)]);
            crate::ui_kit::text(ui, &listed, crate::ui_kit::Text::Value("history_listed_under", false));
        }
    });
    if response.clicked() && on_fold {
        if let Some((key, default)) = fold {
            crate::browser::toggle(app, key, *default);
        }
    } else if response.clicked() && !crate::timeline::pick_feature_into_dialog(app, id, ui.input(|i| i.modifiers)) {
        select_item(app, ui, TimelineItem::Feature(id), order);
    }
    if response.double_clicked() && !on_fold {
        rows.edit = Some(id);
    }
    let sketch = kind.profile_sketch();
    if response.secondary_clicked() && !selected {
        select_only(app, TimelineItem::Feature(id));
    }
    let mut edit = false;
    crate::ui_kit::Menu::context(&response, |ui| {
        if crate::ui_kit::menu::button(ui, "Edit Feature", "edit").clicked() {
            edit = true;
            ui.close();
        }
        crate::timeline::sketch_plane_button(app, ui, id);
        if let Some(sketch) = sketch
            && crate::ui_kit::menu::button(ui, "Edit Profile Sketch", "sketch").clicked()
        {
            app.edit_sketch(sketch.0);
            ui.close();
        }
        if crate::ui_kit::menu::button(ui, "Rename Feature", "rename").clicked() {
            crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::Feature(id), rect.center());
            ui.close();
        }
        if crate::ui_kit::menu::button(ui, if *suppressed { "Unsuppress Feature" } else { "Suppress Feature" }, "suppress").clicked() {
            let _ = app.run_heavy("timeline.suppress", json!({"feature": id}));
            ui.close();
        }
        if crate::ui_kit::menu::button(ui, "Roll Back Here", "rollback").clicked() {
            let _ = app.run_heavy("timeline.roll_to", json!({"feature": id}));
            ui.close();
        }
        ui.separator();
        clipboard_buttons(app, ui, id);
        message_button(app, ui, id);
        ui.separator();
        selection_actions(app, ui, groups, true);
        ui.separator();
        delete_button(app, ui, id);
        delete_after_marker_button(app, ui, id, index, marker, None);
    });
    if edit {
        rows.edit = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    #[test]
    fn history_folder_delete_tooltip_uses_the_language() {
        let mut h = Harness::new();
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.preferences.language = "ja".into();
        h.app.run("timeline.group", json!({"name": "My folder"})).unwrap();
        h.frames(4);
        h.step(&json!({"click": {"handle": "timeline-folder:My folder"}, "button": "right"})).unwrap();
        let label = solidcraft_ui_text::Locale::for_language("ja").label("Delete Selected Folders");
        let [x, y] = h.widget(label).unwrap();
        h.call("ui.move", json!({"x": x, "y": y}));
        h.frames(90);
        let output = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
        let texts: Vec<_> = crate::test_support::text_shapes(&output).map(|t| t.galley.text()).collect();
        assert!(texts.contains(&"フィーチャーと入れ子のフォルダーを履歴に残します"), "{texts:?}");
        assert_eq!(h.app.session.doc.timeline_groups.len(), 1, "hovering does not delete the folder");
    }

    #[test]
    fn find_in_timeline_opens_the_collapsed_folders_around_the_feature() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 20, "width": 15, "height": 10, "name": "Base", "body_name": "Plate"})).unwrap();
        let inner = harness.app.run("timeline.group", json!({"name": "Inner", "features": ["Base"]})).unwrap()["group"].as_u64().unwrap();
        let outer = harness.app.run("timeline.group", json!({"name": "Outer", "groups": [inner]})).unwrap()["group"].as_u64().unwrap();
        crate::timeline_folders::set_open(&mut harness.app, inner, false);
        crate::timeline_folders::set_open(&mut harness.app, outer, false);
        harness.frames(4);
        assert!(handle_at("timeline:Base").is_none(), "the feature starts hidden in its folders");

        let menu = crate::context_menu::items(&harness.app, &crate::context_menu::Target::Body { name: "Plate".into() });
        let item = menu.into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
        crate::context_menu::run_item(&mut harness.app, &item, pos2(0.0, 0.0));
        harness.frames(4);
        assert!(handle_at("timeline:Base").is_some(), "Find in Timeline must open the folders holding the feature");
        let base = harness.app.session.doc.find_feature("Base").unwrap().id;
        assert_eq!(harness.app.session.selection, vec![solidcraft_engine::Sel::Feature { id: base }]);
    }

    /// The handle `name` drawn inside the vertical history (the browser publishes the same names).
    fn in_history(name: &str) -> Option<egui::Pos2> {
        let card = crate::scenario::rect_of("history:vertical")?;
        crate::scenario::handles(name).into_iter().filter(|(n, _)| n == name).map(|(_, at)| at).find(|at| card.contains(*at))
    }

    /// Show origin and bodies at the top (#6490): off by default; on, the vertical history starts
    /// with the active component's Bodies folder and the Origin, the browser's rows with their
    /// folds, eyes, selection and menus; they take no drops; too narrow for names, it leaves them out.
    #[test]
    fn origin_and_bodies_lead_the_vertical_history_when_asked() {
        let mut h = Harness::new();
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "First", "body_name": "Block"})).unwrap();
        h.app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Second", "body_name": "Other"}))
            .unwrap();
        h.frames(4);
        assert!(in_history("row:Origin").is_none() && in_history("row:Bodies").is_none(), "off by default");
        h.call("ui.prefs", json!({"timeline_origin_bodies": true}));
        h.frames(3);
        let bodies = in_history("row:Bodies").expect("the Bodies folder");
        let origin = in_history("row:Origin").expect("the Origin");
        let first = handle_at("timeline:First").unwrap();
        assert!(bodies.y < origin.y && origin.y < first.y, "Bodies, Origin, then the features");
        assert!(in_history("row:Block").is_some() && in_history("row:Other").is_some(), "the Bodies folder starts open, as in the browser");
        assert!(in_history("row:XY").is_none(), "the Origin starts closed, as in the browser");

        // The fold is the browser's: opening it here opens it there.
        h.call("ui.click", json!({"x": origin.x, "y": origin.y}));
        h.frames(2);
        assert!(crate::browser::is_open(&h.app, "origin", false));
        let xy = in_history("row:XY").expect("the origin planes");
        let eye = in_history("eye:XY").unwrap();
        h.call("ui.click", json!({"x": eye.x, "y": eye.y}));
        h.frames(2);
        assert!(h.app.session.visibility.hidden_origin.iter().any(|p| p == "XY"), "the plane's eye hides it");
        let eye = in_history("eye:Block").unwrap();
        h.call("ui.click", json!({"x": eye.x, "y": eye.y}));
        h.frames(2);
        assert!(h.app.session.visibility.hidden_bodies.iter().any(|b| b == "Block"), "the body's eye hides it");
        h.call("ui.click", json!({"x": xy.x, "y": xy.y, "button": "right"}));
        h.frames(2);
        assert_eq!(h.app.menu.target, crate::context_menu::Target::OriginPlane { name: "XY".into() }, "the plane's own menu");
        crate::context_menu::close(&mut h.app);
        h.frames(2);
        let other = in_history("row:Other").unwrap();
        h.call("ui.click", json!({"x": other.x, "y": other.y}));
        h.frames(2);
        assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Body { name: "Other".into() }]);

        // Neither the marker nor a feature drops among these rows.
        let order = |h: &Harness| h.app.session.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
        let before = order(&h);
        h.step(&json!({"drag": [{"handle": "timeline:rollback"}, [origin.x, origin.y]]})).unwrap();
        assert_eq!(h.app.session.doc.marker, None, "the marker stays at the end");
        h.step(&json!({"drag": [{"handle": "timeline:Second"}, [bodies.x, bodies.y]]})).unwrap();
        assert_eq!(order(&h), before, "the feature keeps its place");
        let first = handle_at("timeline:First").unwrap();
        h.step(&json!({"drag": [{"handle": "timeline:Second"}, [first.x, first.y - 8.0]]})).unwrap();
        assert_eq!(order(&h), vec!["Second".to_string(), "First".to_string()], "the first feature's top is still a place");

        // The same content can stack with Browser. Bottom History keeps its compact strip.
        h.app.run("view.history_layout", json!({"dock":"under_browser"})).unwrap();
        h.frames(4);
        assert!(in_history("row:Origin").is_some());
        h.app.run("view.history_layout", json!({"dock":"bottom"})).unwrap();
        h.frames(4);
        assert_eq!(crate::scenario::handles("row:Origin").into_iter().filter(|(n, _)| n == "row:Origin").count(), 1, "only the browser's");
    }

    /// In a design with components, the Bodies folder is the active component's, in the
    /// placement being edited, as the browser lists it under that placement.
    #[test]
    fn the_history_lists_the_active_components_bodies() {
        let mut h = Harness::new();
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.preferences.timeline_origin_bodies = true;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "RootBlock"})).unwrap();
        h.app.run("component.create", json!({"name": "Part"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Inside"})).unwrap();
        h.frames(4);
        assert!(in_history("row:Inside").is_some() && in_history("row:RootBlock").is_none(), "the active component's bodies");
        assert!(in_history("row:Origin").is_some());
        h.app.run("component.activate", json!({"component": "root"})).unwrap();
        h.frames(4);
        assert!(in_history("row:RootBlock").is_some() && in_history("row:Inside").is_none(), "the root's own bodies");
    }

    #[test]
    fn rollback_bar_above_last_feature_can_be_dragged_to_the_end() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "First"})).unwrap();
        harness
            .app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Last"}))
            .unwrap();
        harness.app.run("timeline.roll_to", json!({"position": 1})).unwrap();
        harness.frames(4);

        let last = handle_at("timeline:Last").unwrap();
        harness.step(&json!({"drag": [{"handle": "timeline:rollback"}, [last.x, last.y + 15.0]]})).unwrap();
        assert_eq!(harness.app.session.doc.marker, None, "dropping below the final row must restore the fully evaluated history");
    }

    #[test]
    fn rollback_bar_before_a_folders_first_feature_is_above_its_header() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "First"})).unwrap();
        harness
            .app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Second"}))
            .unwrap();
        harness.app.run("timeline.group", json!({"name": "Folder", "features": ["First", "Second"]})).unwrap();
        harness.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        harness.frames(4);

        let marker = handle_at("timeline:rollback").unwrap();
        let folder = handle_at("timeline-folder:Folder").unwrap();
        let first = handle_at("timeline:First").unwrap();
        assert!(marker.y < folder.y && folder.y < first.y, "rollback marker must precede the folder containing the first rolled-back feature");
    }

    #[test]
    fn feature_folder_selection_can_move_as_a_block() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [("First", "FirstBody", 0), ("Second", "SecondBody", 20), ("Third", "ThirdBody", 40)] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        harness.app.run("timeline.group", json!({"name": "Pair", "features": ["First", "Second"]})).unwrap();
        harness.frames(4);

        let third = handle_at("timeline:Third").unwrap();
        harness.step(&json!({"drag": [{"handle": "timeline-folder:Pair"}, [third.x, third.y + 15.0]]})).unwrap();
        let names: Vec<_> = harness.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect();
        assert_eq!(names, vec!["Third", "First", "Second"]);
    }

    #[test]
    fn selected_folders_and_features_can_be_dragged_into_a_folder() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Grouped"})).unwrap();
        harness
            .app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Loose"}))
            .unwrap();
        let group = harness.app.run("timeline.group", json!({"name": "Folder", "features": ["Grouped"]})).unwrap()["group"].as_u64().unwrap();
        let destination = harness.app.run("timeline.group", json!({"name": "Destination"})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);

        harness.step(&json!({"click": {"handle": "timeline-folder:Folder"}})).unwrap();
        harness.step(&json!({"click": {"handle": "timeline:Loose"}, "ctrl": true})).unwrap();
        assert_eq!(harness.app.tree.picked_timeline_groups, vec![group]);
        assert!(harness
            .app
            .session
            .selection
            .iter()
            .any(|selection| matches!(selection, solidcraft_engine::Sel::Feature { id } if harness.app.session.doc.feature(*id).is_some_and(|feature| feature.name == "Loose"))));
        harness.step(&json!({"drag": [{"handle": "timeline:Loose"}, {"handle": "timeline-folder:Destination"}]})).unwrap();
        let source = harness.app.session.doc.timeline_groups.iter().find(|entry| entry.id == group).unwrap();
        let target = harness.app.session.doc.timeline_groups.iter().find(|entry| entry.id == destination).unwrap();
        let loose = harness.app.session.doc.find_feature("Loose").unwrap().id;
        assert_eq!(source.parent, Some(destination), "the selected folder should nest in the drop target");
        assert!(target.features.contains(&loose), "the selected feature should move into the drop target");
    }

    #[test]
    fn vertical_folder_drop_targets_follow_each_available_indent_level() {
        let inside = VerticalSlot { y: 40.0, position: 1, group: Some(7), indent_group: None };
        let at_end = VerticalSlot { y: 60.0, position: 2, group: None, indent_group: Some(3) };
        let folder_rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 20.0));
        let groups = vec![
            solidcraft_engine::doc::TimelineGroup { id: 1, name: "Outer".into(), parent: None, features: vec![], joints: vec![] },
            solidcraft_engine::doc::TimelineGroup { id: 2, name: "Middle".into(), parent: Some(1), features: vec![], joints: vec![] },
            solidcraft_engine::doc::TimelineGroup { id: 3, name: "Inner".into(), parent: Some(2), features: vec![], joints: vec![] },
            solidcraft_engine::doc::TimelineGroup { id: 7, name: "Target".into(), parent: None, features: vec![], joints: vec![] },
        ];

        assert_eq!(
            vertical_drop_group(pos2(80.0, 10.0), at_end, &[(folder_rect, 7)], &groups, 0.0, 80.0, None),
            Some(7),
            "dropping over a folder row"
        );
        assert_eq!(vertical_drop_group(pos2(80.0, 40.0), inside, &[], &groups, 0.0, 80.0, None), Some(7), "dropping between expanded folder items");
        assert_eq!(vertical_drop_group(pos2(20.0, 60.0), at_end, &[], &groups, 0.0, 0.0, None), None);
        assert_eq!(vertical_drop_group(pos2(30.0, 60.0), at_end, &[], &groups, 0.0, 14.0, None), Some(1));
        assert_eq!(vertical_drop_group(pos2(44.0, 60.0), at_end, &[], &groups, 0.0, 28.0, None), Some(2));
        assert_eq!(vertical_drop_group(pos2(58.0, 60.0), at_end, &[], &groups, 0.0, 42.0, None), Some(3));

        let same_inner_row = VerticalSlot { y: 80.0, position: 2, group: Some(3), indent_group: None };
        assert_eq!(
            vertical_drop_group(pos2(44.0, 80.0), same_inner_row, &[], &groups, 0.0, -14.0, Some(3)),
            Some(2),
            "dragging left on the same row should outdent without vertical movement"
        );
        assert_eq!(vertical_drop_bar_left(0.0, Some(2), &groups), 32.0, "the bar should begin before the target level's feature icon");
        let root_drag = DragItems { features: vec![9], groups: vec![], origin_x: 100.0, source_group: None };
        assert_eq!(drag_x(&root_drag, 114.0), 14.0, "every folder level should require the same 14-point drag");
    }

    #[test]
    fn dragging_out_of_a_folder_updates_membership() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Inside"})).unwrap();
        harness
            .app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Loose"}))
            .unwrap();
        let group = harness.app.run("timeline.group", json!({"name": "Folder", "features": ["Inside"]})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);

        let loose = handle_at("timeline:Loose").unwrap();
        harness.step(&json!({"drag": [{"handle": "timeline:Inside"}, [loose.x, loose.y + 15.0]]})).unwrap();

        let inside = harness.app.session.doc.find_feature("Inside").unwrap().id;
        let group = harness.app.session.doc.timeline_groups.iter().find(|candidate| candidate.id == group).unwrap();
        assert!(!group.features.contains(&inside), "reordering outside a folder must also remove folder membership");
    }

    #[test]
    fn folder_header_top_edge_is_an_insertion_slot() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Grouped"})).unwrap();
        harness
            .app
            .run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "operation": "new_body", "name": "Loose"}))
            .unwrap();
        let group = harness.app.run("timeline.group", json!({"name": "Folder", "features": ["Grouped"]})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);

        let folder = handle_at("timeline-folder:Folder").unwrap();
        harness.step(&json!({"drag": [{"handle": "timeline:Loose"}, [folder.x, folder.y - 10.0]]})).unwrap();

        let loose = harness.app.session.doc.find_feature("Loose").unwrap().id;
        let group = harness.app.session.doc.timeline_groups.iter().find(|candidate| candidate.id == group).unwrap();
        assert!(!group.features.contains(&loose), "the folder header's top edge must not capture the drop");
        assert_eq!(harness.app.session.doc.features.first().map(|feature| feature.name.as_str()), Some("Loose"));
    }

    #[test]
    fn selected_count_does_not_move_the_history_rows() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Fixed"})).unwrap();
        harness.frames(4);
        let before = handle_at("timeline:Fixed").unwrap();

        harness.step(&json!({"click": {"handle": "timeline:Fixed"}})).unwrap();
        let after = handle_at("timeline:Fixed").unwrap();
        assert!((after.y - before.y).abs() < 0.5, "selection count shifted the row from {} to {}", before.y, after.y);
    }

    /// F2 on a clicked History folder opens its name, as a double-click does (#16213).
    #[test]
    fn f2_renames_the_clicked_folder() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Feature"})).unwrap();
        harness.frames(4);
        harness.step(&json!({"click": {"handle": "timeline:Feature"}})).unwrap();
        harness.step(&json!({"click": {"handle": "timeline:Feature"}, "button": "right"})).unwrap();
        harness.step(&json!({"widget": "Add to New Folder"})).unwrap();
        harness.frames(2);
        harness.step(&json!({"key": "Enter"})).unwrap();
        let name = harness.app.session.doc.timeline_groups.first().unwrap().name.clone();

        harness.step(&json!({"click": {"handle": format!("timeline-folder:{name}")}})).unwrap();
        harness.frames(2);
        assert!(handle_at(&format!("timeline-folder-rename:{name}")).is_none(), "a click alone does not rename");
        harness.step(&json!({"key": "F2"})).unwrap();
        harness.frames(2);
        assert!(handle_at(&format!("timeline-folder-rename:{name}")).is_some(), "F2 did not open the folder's name");
        harness.step(&json!({"text": "Renamed"})).unwrap();
        harness.step(&json!({"key": "Enter"})).unwrap();
        assert_eq!(harness.app.session.doc.timeline_groups.first().map(|group| group.name.as_str()), Some("Renamed"));
    }

    #[test]
    fn new_folder_starts_inline_rename_with_its_name_selected() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Feature"})).unwrap();
        harness.frames(4);

        harness.step(&json!({"click": {"handle": "timeline:Feature"}})).unwrap();
        harness.step(&json!({"click": {"handle": "timeline:Feature"}, "button": "right"})).unwrap();
        harness.step(&json!({"widget": "Add to New Folder"})).unwrap();
        harness.frames(2);

        let original = harness.app.session.doc.timeline_groups.first().unwrap().name.clone();
        assert!(handle_at(&format!("timeline-folder-rename:{original}")).is_some(), "new folder did not show its inline name editor");
        harness.step(&json!({"text": "Renamed"})).unwrap();
        harness.step(&json!({"key": "Enter"})).unwrap();
        assert_eq!(harness.app.session.doc.timeline_groups.first().map(|group| group.name.as_str()), Some("Renamed"));

        let group = harness.app.session.doc.timeline_groups.first().unwrap().id;
        crate::context_menu::start_rename(&mut harness.app, crate::context_menu::RenameWhat::TimelineGroup(group), pos2(0.0, 0.0));
        harness.frames(2);
        assert!(handle_at("timeline-folder-rename:Renamed").is_some(), "folder rename did not stay in the folder row");
        harness.step(&json!({"text": "Again"})).unwrap();
        harness.step(&json!({"key": "Enter"})).unwrap();
        assert_eq!(harness.app.session.doc.timeline_groups.first().map(|group| group.name.as_str()), Some("Again"));
    }

    /// The upright History can be on screen with `timeline_left` off (a side zone, a tall narrow
    /// bottom section); its folder rows must still type the name in the row, never in the
    /// floating box at (0, 0) (#16217).
    #[test]
    fn new_folder_name_is_typed_in_its_row_without_the_left_dock_preference() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Feature"})).unwrap();
        harness.frames(4);
        harness.app.preferences.timeline_left = false;
        harness.frames(2);

        harness.step(&json!({"click": {"handle": "timeline:Feature"}})).unwrap();
        harness.step(&json!({"click": {"handle": "timeline:Feature"}, "button": "right"})).unwrap();
        harness.step(&json!({"widget": "Add to New Folder"})).unwrap();
        harness.frames(2);

        let original = harness.app.session.doc.timeline_groups.first().unwrap().name.clone();
        assert!(handle_at(&format!("timeline-folder-rename:{original}")).is_some(), "new folder did not show its inline name editor");
        assert!(crate::scenario::rect_of("rename:name").is_none(), "a floating rename box was drawn over the folder's row editor");
        harness.step(&json!({"text": "Renamed"})).unwrap();
        harness.step(&json!({"key": "Enter"})).unwrap();
        assert_eq!(harness.app.session.doc.timeline_groups.first().map(|group| group.name.as_str()), Some("Renamed"));
    }

    #[test]
    fn nested_folder_delete_keeps_parent_and_features() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Feature"})).unwrap();
        let root = harness.app.run("timeline.group", json!({"name": "Root"})).unwrap()["group"].as_u64().unwrap();
        let child =
            harness.app.run("timeline.group", json!({"name": "Child", "parent": root, "features": ["Feature"]})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);
        harness.step(&json!({"click": {"handle": "timeline-folder:Child"}})).unwrap();
        harness.step(&json!({"key": "Delete"})).unwrap();
        assert!(harness.app.session.doc.timeline_groups.iter().all(|group| group.id != child));
        assert!(harness.app.session.doc.timeline_groups.iter().any(|group| group.id == root));
        assert!(harness.app.session.doc.find_feature("Feature").is_some());
    }

    /// Right-click a folder and pick a menu entry.
    fn folder_menu(harness: &mut Harness, folder: &str, entry: &str) {
        harness.step(&json!({"click": {"handle": format!("timeline-folder:{folder}")}, "button": "right"})).unwrap();
        let [x, y] = harness.widget(entry).unwrap_or_else(|| panic!("`{entry}` is in the folder's menu"));
        harness.step(&json!({"click": [x, y]})).unwrap();
        harness.frames(4);
    }

    #[test]
    fn joint_folder_rows_follow_their_nested_folder_before_its_sibling() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY", "name":"Profile"})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("component.create", json!({"name":"Part"})).unwrap();
        for name in ["A", "B", "C"] {
            h.app.run("joint.as_built", json!({"type":"rigid", "a":0, "b":"Part", "name":name})).unwrap();
        }
        let outer = h.app.run("timeline.group", json!({"name":"Outer", "features":["Profile"]})).unwrap()["group"].as_u64().unwrap();
        let first = h.app.run("timeline.group", json!({"name":"First", "joints":["A"], "parent":outer})).unwrap()["group"].as_u64().unwrap();
        h.app.run("timeline.group", json!({"name":"Second", "joints":["C"], "parent":outer})).unwrap();
        h.app.run("timeline.group", json!({"name":"Nested", "joints":["B"], "parent":first})).unwrap();
        h.frames(4);
        let y = |name| handle_at(name).unwrap().y;
        assert!(y("timeline-folder:First") < y("timeline-joint:A"));
        assert!(y("timeline-joint:A") < y("timeline-folder:Nested"));
        assert!(y("timeline-folder:Nested") < y("timeline-joint:B"));
        assert!(y("timeline-joint:B") < y("timeline-folder:Second"));
        assert!(y("timeline-folder:Second") < y("timeline-joint:C"));
        crate::timeline_folders::set_open(&mut h.app, first, false);
        h.frames(4);
        assert!(handle_at("timeline-joint:A").is_none() && handle_at("timeline-joint:B").is_none());
        assert!(handle_at("timeline-joint:C").is_some());
    }

    #[test]
    fn joint_folder_menu_suppresses_and_deletes_joints_in_one_step() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.run("component.create", json!({"name":"Part"})).unwrap();
        let joint = h.app.run("joint.as_built", json!({"type":"revolute", "a":0, "b":"Part", "name":"Hinge"})).unwrap()["joint"].as_u64().unwrap();
        h.app.run("timeline.group", json!({"name":"Folder", "joints":[joint]})).unwrap();
        h.frames(4);
        folder_menu(&mut h, "Folder", "Suppress Selected Folders");
        assert!(h.app.session.doc.assembly.joints[0].suppressed);
        folder_menu(&mut h, "Folder", "Unsuppress Selected Folders");
        assert!(!h.app.session.doc.assembly.joints[0].suppressed);
        let before = h.app.session.doc.clone();
        folder_menu(&mut h, "Folder", "Delete Folders and Features…");
        assert!(h.app.session.doc.assembly.joints.is_empty());
        assert!(h.app.session.doc.timeline_groups.is_empty());
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(*h.app.session.doc, *before);
    }

    /// A folder's menu suppresses and unsuppresses every feature in it and in its nested folders,
    /// and deletes the folders with their features in one undo step (#7219).
    #[test]
    fn folder_menu_suppresses_and_deletes_the_folder_with_its_features() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, x) in [("Outside", 0), ("Inside", 20), ("Nested", 40)] {
            let params = json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name});
            harness.app.run("solid.box", params).unwrap();
        }
        let outer = harness.app.run("timeline.group", json!({"name": "Outer", "features": ["Inside"]})).unwrap()["group"].as_u64().unwrap();
        harness.app.run("timeline.group", json!({"name": "Inner", "features": ["Nested"], "parent": outer})).unwrap();
        harness.frames(4);
        let suppressed = |harness: &Harness| {
            harness.app.session.doc.features.iter().filter(|feature| feature.suppressed).map(|feature| feature.name.clone()).collect::<Vec<_>>()
        };

        folder_menu(&mut harness, "Outer", "Suppress Selected Folders");
        assert_eq!(suppressed(&harness), ["Inside", "Nested"]);
        folder_menu(&mut harness, "Outer", "Unsuppress Selected Folders");
        assert!(suppressed(&harness).is_empty(), "the menu offers to unsuppress a folder whose features all are");

        folder_menu(&mut harness, "Outer", "Delete Folders and Features…");
        let names: Vec<_> = harness.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect();
        assert_eq!(names, ["Outside"]);
        assert!(harness.app.session.doc.timeline_groups.is_empty(), "the nested folder goes too");
        assert!(harness.app.dialog.is_none(), "nothing outside the folder needed them: no question");
        harness.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(harness.app.session.doc.features.len(), 3);
        assert_eq!(harness.app.session.doc.timeline_groups.len(), 2, "one undo step");
    }

    /// When a feature outside the folder needs one inside, the delete asks first, and its Delete
    /// takes the folder as well (#7219).
    #[test]
    fn deleting_a_folder_others_depend_on_asks_and_then_takes_the_folder() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"})).unwrap();
        harness.app.run("solid.fillet", json!({"edges": [[10, 5, 10]], "radius": 1, "name": "Round"})).unwrap();
        harness.app.run("timeline.group", json!({"name": "Stock", "features": ["Base"]})).unwrap();
        harness.frames(4);

        folder_menu(&mut harness, "Stock", "Delete Folders and Features…");
        assert!(harness.app.dialog.is_some(), "Round needs Base: the delete asks first");
        assert!(harness.app.session.doc.find_feature("Base").is_some());
        let [x, y] = harness.widget("Delete").expect("the confirmation's Delete button");
        harness.step(&json!({"click": [x, y]})).unwrap();
        harness.frames(4);
        assert!(harness.app.session.doc.find_feature("Base").is_none());
        assert!(harness.app.session.doc.timeline_groups.is_empty(), "the confirmed delete takes the folder too");
    }

    #[test]
    fn collapsed_folder_stays_visible_while_its_contents_hide() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Inside"})).unwrap();
        harness.app.run("timeline.group", json!({"name": "Folder", "features": ["Inside"]})).unwrap();
        harness.frames(4);
        assert!(handle_at("timeline:Inside").is_some());

        harness.step(&json!({"click": {"handle": "timeline-folder-fold:Folder"}})).unwrap();
        assert!(handle_at("timeline-folder:Folder").is_some(), "collapsed folder row disappeared");
        assert!(handle_at("timeline:Inside").is_none(), "collapsed folder still showed its contents");
    }

    /// #8592: a folded folder carries the worst mark of its features, nested folders included,
    /// and an open one none.
    #[test]
    fn a_folded_folder_carries_the_worst_mark_of_its_features() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for name in ["Warned", "Broken"] {
            harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": name})).unwrap();
        }
        harness.app.run("timeline.group", json!({"name": "Inner", "features": ["Broken"]})).unwrap();
        harness.app.run("timeline.group", json!({"name": "Outer", "features": ["Warned"]})).unwrap();
        let groups = harness.app.session.doc.timeline_groups.clone();
        let id = |name: &str| groups.iter().find(|group| group.name == name).map(|group| group.id).unwrap();
        harness.app.run("timeline.move_to_group", json!({"groups": [id("Inner")], "group": id("Outer")})).unwrap();
        harness.frames(4);
        let mark = |harness: &mut Harness, name: &str, error: Option<&str>, warning: Option<&str>| {
            let feature = harness.app.session.doc.features.iter().find(|f| f.name == name).map(|f| f.id).unwrap();
            let result = harness.app.session.model.results.iter_mut().find(|r| r.id == feature).unwrap();
            result.error = error.map(str::to_string);
            result.warning = warning.map(str::to_string);
        };
        mark(&mut harness, "Warned", None, Some("edge 3 no longer exists; using the edge at the picked point"));
        harness.frames(2);
        let marks = || crate::scenario::handles("timeline-folder-status:").into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        assert!(marks().is_empty(), "open folders show their features' own marks");

        crate::timeline_folders::set_open(&mut harness.app, id("Outer"), false);
        harness.frames(2);
        assert_eq!(marks(), ["timeline-folder-status:warning:Outer"]);

        mark(&mut harness, "Broken", Some("the blend is larger than the neighbouring faces"), None);
        harness.frames(2);
        assert_eq!(marks(), ["timeline-folder-status:error:Outer"], "an error in a nested folder outweighs the warning");
    }

    #[test]
    fn drop_below_a_collapsed_folder_appends_to_that_folder() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [("First", "FirstBody", 0), ("Second", "SecondBody", 20), ("Loose", "LooseBody", 40)] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        let group = harness.app.run("timeline.group", json!({"name": "Folder", "features": ["First", "Second"]})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);
        harness.step(&json!({"click": {"handle": "timeline-folder-fold:Folder"}})).unwrap();
        let folder = handle_at("timeline-folder:Folder").unwrap();

        harness.step(&json!({"drag": [{"handle": "timeline:Loose"}, [folder.x, folder.y + 14.0]]})).unwrap();

        let names: Vec<_> = harness.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect();
        assert_eq!(names, vec!["First", "Second", "Loose"], "the dropped feature belongs at the collapsed folder's end");
        let folder = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == group).unwrap();
        let loose = harness.app.session.doc.find_feature("Loose").unwrap().id;
        assert_eq!(folder.features.last(), Some(&loose));
    }

    #[test]
    fn drop_at_nested_folder_bottom_can_choose_the_outer_folder() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [("Inside", "InsideBody", 0), ("Loose", "LooseBody", 20)] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        let inner = harness.app.run("timeline.group", json!({"name": "Inner", "features": ["Inside"]})).unwrap()["group"].as_u64().unwrap();
        let outer = harness.app.run("timeline.group", json!({"name": "Outer", "groups": [inner]})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);
        let loose_row = handle_at("timeline:Loose").unwrap();

        harness.step(&json!({"drag": [{"handle": "timeline:Loose"}, [loose_row.x + FOLDER_INDENT, loose_row.y - 12.0]]})).unwrap();

        let loose = harness.app.session.doc.find_feature("Loose").unwrap().id;
        let outer = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == outer).unwrap();
        let inner = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == inner).unwrap();
        assert!(outer.features.contains(&loose), "the intermediate indentation should put the feature directly in the outer folder");
        assert!(!inner.features.contains(&loose));
    }

    #[test]
    fn last_features_in_inner_folder_can_move_out_to_their_parent_without_reordering() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [("First", "FirstBody", 0), ("Penultimate", "PenultimateBody", 20), ("Last", "LastBody", 40)] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        let inner = harness.app.run("timeline.group", json!({"name": "Inner", "features": ["First", "Penultimate", "Last"]})).unwrap()["group"]
            .as_u64()
            .unwrap();
        let outer = harness.app.run("timeline.group", json!({"name": "Outer", "groups": [inner]})).unwrap()["group"].as_u64().unwrap();
        harness.frames(4);
        harness.step(&json!({"click": {"handle": "timeline:Penultimate"}})).unwrap();
        harness.step(&json!({"click": {"handle": "timeline:Last"}, "ctrl": true})).unwrap();
        let last_row = handle_at("timeline:Last").unwrap();

        let start = last_row;
        let outdented = pos2(start.x - 14.0, start.y);
        let destination = outdented;
        harness.app.synthetic.push(egui::Event::PointerMoved(start));
        harness.frame();
        harness.app.synthetic.push(egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.frame();
        // Cross the drag threshold horizontally and release without moving vertically. The
        // initial movement must select the parent folder while keeping the timeline position.
        harness.app.synthetic.push(egui::Event::PointerMoved(outdented));
        harness.frame();
        harness.app.synthetic.push(egui::Event::PointerButton {
            pos: destination,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.frames(4);

        let moved: Vec<_> = ["Penultimate", "Last"].into_iter().map(|name| harness.app.session.doc.find_feature(name).unwrap().id).collect();
        let outer = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == outer).unwrap();
        let inner = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == inner).unwrap();
        assert!(
            moved.iter().all(|feature| outer.features.contains(feature)),
            "dragging the inner folder's last features left should move them to their parent without moving them down"
        );
        assert!(moved.iter().all(|feature| !inner.features.contains(feature)));
        assert_eq!(
            harness.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect::<Vec<_>>(),
            vec!["First", "Penultimate", "Last"],
            "outdenting should preserve timeline order"
        );
    }

    #[test]
    fn last_features_in_middle_folder_can_move_to_outer_without_reordering() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [
            ("InnerFeature", "InnerBody", 0),
            ("MiddleFirst", "MiddleFirstBody", 20),
            ("MiddleLast", "MiddleLastBody", 40),
            ("OuterFeature", "OuterBody", 60),
        ] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        let inner = harness.app.run("timeline.group", json!({"name": "Inner", "features": ["InnerFeature"]})).unwrap()["group"].as_u64().unwrap();
        let middle = harness
            .app
            .run("timeline.group", json!({"name": "Middle", "features": ["MiddleFirst", "MiddleLast"], "groups": [inner]}))
            .unwrap()["group"]
            .as_u64()
            .unwrap();
        let outer = harness.app.run("timeline.group", json!({"name": "Outer", "features": ["OuterFeature"], "groups": [middle]})).unwrap()["group"]
            .as_u64()
            .unwrap();
        harness.frames(4);
        harness.step(&json!({"click": {"handle": "timeline:MiddleFirst"}})).unwrap();
        harness.step(&json!({"click": {"handle": "timeline:MiddleLast"}, "ctrl": true})).unwrap();
        let start = handle_at("timeline:MiddleLast").unwrap();
        let destination = pos2(start.x - FOLDER_INDENT, start.y);

        harness.step(&json!({"drag": [[start.x, start.y], [destination.x, destination.y]]})).unwrap();

        let moved: Vec<_> = ["MiddleFirst", "MiddleLast"].into_iter().map(|name| harness.app.session.doc.find_feature(name).unwrap().id).collect();
        let outer = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == outer).unwrap();
        let middle = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == middle).unwrap();
        assert!(moved.iter().all(|feature| outer.features.contains(feature)), "middle-folder items should move directly into the outer folder");
        assert!(moved.iter().all(|feature| !middle.features.contains(feature)));
        assert_eq!(
            harness.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect::<Vec<_>>(),
            vec!["InnerFeature", "MiddleFirst", "MiddleLast", "OuterFeature"]
        );
    }

    #[test]
    fn last_feature_in_middle_sibling_folder_can_exit_with_a_vertical_drag() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [("TopFeature", "TopBody", 0), ("MiddleFeature", "MiddleBody", 20), ("BottomFeature", "BottomBody", 40)] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        harness.app.run("timeline.group", json!({"name": "Top", "features": ["TopFeature"]})).unwrap();
        let middle = harness.app.run("timeline.group", json!({"name": "Middle", "features": ["MiddleFeature"]})).unwrap()["group"].as_u64().unwrap();
        harness.app.run("timeline.group", json!({"name": "Bottom", "features": ["BottomFeature"]})).unwrap();
        harness.frames(4);
        let start = handle_at("timeline:MiddleFeature").unwrap();
        let bottom = handle_at("timeline-folder:Bottom").unwrap();

        harness.step(&json!({"drag": [[start.x, start.y], [start.x, bottom.y - 10.0]]})).unwrap();

        let feature = harness.app.session.doc.find_feature("MiddleFeature").unwrap().id;
        let middle = harness.app.session.doc.timeline_groups.iter().find(|folder| folder.id == middle).unwrap();
        assert!(!middle.features.contains(&feature), "a straight vertical drag to the outside boundary should remove the item from its folder");
        assert!(harness.app.session.doc.timeline_groups.iter().all(|folder| !folder.features.contains(&feature)));
    }

    #[test]
    fn last_nested_feature_can_outdent_two_levels_at_the_same_position() {
        let mut harness = Harness::new();
        harness.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        for (name, body, x) in [
            ("Shell", "ShellBody", 0),
            ("Lip", "LipBody", 20),
            ("SnapFit", "SnapFitBody", 40),
            ("Boss1", "Boss1Body", 60),
            ("Boss2", "Boss2Body", 80),
            ("Rest", "RestBody", 100),
        ] {
            harness
                .app
                .run(
                    "solid.box",
                    json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
                )
                .unwrap();
        }
        let inner = harness.app.run("timeline.group", json!({"name": "Folder1", "features": ["Lip", "SnapFit", "Boss1", "Boss2"]})).unwrap()["group"]
            .as_u64()
            .unwrap();
        harness.app.run("timeline.group", json!({"name": "Folder2", "features": ["Shell"], "groups": [inner]})).unwrap();
        harness.frames(4);
        let row = handle_at("timeline:Boss2").unwrap();
        let start = pos2(20.0, row.y);

        harness.step(&json!({"drag": [[start.x, start.y], [6.0, start.y]]})).unwrap();

        let feature = harness.app.session.doc.find_feature("Boss2").unwrap().id;
        assert!(
            harness.app.session.doc.timeline_groups.iter().all(|folder| !folder.features.contains(&feature)),
            "the last nested item should reach root without moving vertically"
        );
        assert_eq!(
            harness.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect::<Vec<_>>(),
            vec!["Shell", "Lip", "SnapFit", "Boss1", "Boss2", "Rest"]
        );
    }

    /// A finished sketch called `name`: a 10 mm square on XY with its corner at `x`.
    fn square(h: &mut Harness, name: &str, x: f64) -> u64 {
        h.app.run("sketch.create", json!({"plane": "XY", "name": name})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [x, 0], "p1": [x + 10.0, 10]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        sketch
    }

    fn extrude(h: &mut Harness, sketch: &str, name: &str, operation: &str) {
        h.app.run("solid.extrude", json!({"sketch": sketch, "distance": 5, "operation": operation, "name": name, "body_name": name})).unwrap();
    }

    /// The History at the left, wide enough for names, with Sketches: Under their feature (#6489).
    fn nested_history() -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.preferences.history_sketches = crate::prefs::HistorySketches::UnderFeature;
        h
    }

    fn row_y(handle: &str) -> f32 {
        handle_at(handle).unwrap_or_else(|| panic!("{handle} is not drawn")).y
    }

    fn feature_names(h: &Harness) -> Vec<&str> {
        h.app.session.doc.features.iter().map(|feature| feature.name.as_str()).collect()
    }

    #[test]
    fn sketches_keep_their_own_rows_in_creation_order_by_default() {
        let mut h = nested_history();
        h.app.preferences.history_sketches = crate::prefs::HistorySketches::default();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        square(&mut h, "Pocket", 20.0);
        extrude(&mut h, "Pocket", "Second", "new");
        h.settle();
        assert_eq!(crate::prefs::Prefs::default().history_sketches, crate::prefs::HistorySketches::InOrder);
        let ys: Vec<f32> = ["timeline:Profile", "timeline:Block", "timeline:Pocket", "timeline:Second"].into_iter().map(row_y).collect();
        assert!(ys.windows(2).all(|pair| pair[0] < pair[1]), "creation order: {ys:?}");
        assert!(handle_at("timeline-sketches-fold:Block").is_none());
        let row = crate::scenario::rect_of("history:feature:Block").unwrap();
        assert_eq!(row.height(), crate::ui_kit::TREE_ROW_HEIGHT);
    }

    #[test]
    fn a_sketch_is_listed_folded_under_the_feature_built_from_it() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        square(&mut h, "Pocket", 20.0);
        extrude(&mut h, "Pocket", "Second", "new");
        square(&mut h, "Unused", 40.0);
        h.settle();
        let block = h.app.session.doc.find_feature("Block").unwrap().id;

        // Folded by default: only the features and the unused sketch have rows.
        for hidden in ["timeline:Profile", "timeline:Pocket"] {
            assert!(handle_at(hidden).is_none(), "{hidden} starts folded under its feature");
        }
        assert!(row_y("timeline:Block") < row_y("timeline:Second") && row_y("timeline:Second") < row_y("timeline:Unused"));
        assert!(handle_at("timeline-sketches-fold:Unused").is_none(), "a sketch no feature uses has nothing to fold");

        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Block"}})).unwrap();
        h.settle();
        assert!(row_y("timeline:Block") < row_y("timeline:Profile") && row_y("timeline:Profile") < row_y("timeline:Second"));
        assert!(handle_at("timeline:Pocket").is_none(), "only the opened feature shows its sketch");
        assert!(h.app.session.selection.is_empty(), "the fold arrow only folds");
        // The fold is kept with the browser's folds, between runs.
        let mut restored = Harness::new();
        restored.app.load_prefs(&h.app.prefs());
        assert!(restored.app.tree.expanded.contains(&sketches_key(block)));
    }

    #[test]
    fn a_sketch_several_features_use_is_listed_under_the_first_and_the_others_select_it() {
        let mut h = nested_history();
        let profile = square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        extrude(&mut h, "Profile", "Copy", "new");
        h.settle();
        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Block"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Copy"}})).unwrap();
        h.settle();
        let reference = "timeline-reference:Copy:Profile";
        assert!(row_y("timeline:Block") < row_y("timeline:Profile") && row_y("timeline:Profile") < row_y("timeline:Copy"));
        assert!(row_y("timeline:Copy") < row_y(reference));

        h.step(&json!({"click": {"handle": reference}})).unwrap();
        assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Feature { id: profile }], "the reference row selects the sketch");
        h.app.run("select.clear", json!({})).unwrap();
        // Hovering it lights up the same sketch: in the 3D view, and in its row under Block.
        h.step(&json!({"move": {"handle": reference}})).unwrap();
        h.frames(2);
        assert_eq!(h.app.viewport.hover_feature, Some(profile));
        // Dragging it moves nothing: it only points at the sketch.
        let before = h.app.session.doc.features.clone();
        let top = row_y("timeline:Block");
        h.step(&json!({"drag": [{"handle": reference}, [handle_at(reference).unwrap().x, top - 15.0]]})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.features, before);
    }

    #[test]
    fn the_marker_between_a_sketch_and_its_feature_shows_the_sketch_above_it() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        h.app.run("timeline.roll_to", json!({"position": 1})).unwrap();
        h.settle();
        assert!(row_y("timeline:Profile") < row_y("timeline:rollback") && row_y("timeline:rollback") < row_y("timeline:Block"));
        assert!(handle_at("timeline-sketches-fold:Block").is_none());

        // Both rolled back: listed again, below the marker.
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.settle();
        assert!(row_y("timeline:rollback") < row_y("timeline:Block"));
        assert!(handle_at("timeline:Profile").is_none());

        // Dropping the marker above the feature rolls back the sketch listed under it too.
        h.app.run("timeline.roll_to", json!({})).unwrap();
        h.settle();
        let block = handle_at("timeline:Block").unwrap();
        h.step(&json!({"drag": [{"handle": "timeline:rollback"}, [block.x, block.y - 15.0]]})).unwrap();
        h.settle();
        assert_eq!(h.app.session.doc.marker, Some(0));
        assert!(handle_at("timeline-sketches-fold:Block").is_some());
    }

    #[test]
    fn editing_a_listed_sketch_shows_it_above_the_marker_until_it_is_finished() {
        let mut h = nested_history();
        let profile = square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        h.settle();
        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Block"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Profile"}})).unwrap();
        assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Feature { id: profile }], "the row under the feature is the sketch");
        h.step(&json!({"click": {"handle": "timeline:Profile"}, "button": "right"})).unwrap();
        h.step(&json!({"widget": "Edit Feature"})).unwrap();
        h.settle();
        assert_eq!(h.app.session.active_sketch, Some(profile));
        assert!(row_y("timeline:Profile") < row_y("timeline:rollback") && row_y("timeline:rollback") < row_y("timeline:Block"));
        h.app.finish_sketch();
        h.settle();
        assert_eq!(h.app.session.doc.marker, None);
        assert!(row_y("timeline:Block") < row_y("timeline:Profile"), "listed under its feature again");
    }

    #[test]
    fn a_sketch_in_another_folder_than_its_feature_keeps_its_row() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        square(&mut h, "Pocket", 20.0);
        extrude(&mut h, "Pocket", "Second", "new");
        h.app.run("timeline.group", json!({"name": "Sketches", "features": ["Profile"]})).unwrap();
        h.app.run("timeline.group", json!({"name": "Work", "features": ["Pocket", "Second"]})).unwrap();
        h.settle();
        // Profile is in a folder and Block isn't, so Profile keeps its row; Pocket and Second share one.
        assert!(row_y("timeline-folder:Sketches") < row_y("timeline:Profile") && row_y("timeline:Profile") < row_y("timeline:Block"));
        assert!(row_y("timeline-folder:Work") < row_y("timeline:Second") && handle_at("timeline:Pocket").is_none());
        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Second"}})).unwrap();
        h.settle();
        assert!(row_y("timeline:Second") < row_y("timeline:Pocket"));
        // Collapsing the folder hides the feature and the sketch under it.
        h.step(&json!({"click": {"handle": "timeline-folder-fold:Work"}})).unwrap();
        h.settle();
        assert!(handle_at("timeline:Second").is_none() && handle_at("timeline:Pocket").is_none());
    }

    #[test]
    fn dragging_a_feature_moves_the_sketch_listed_under_it() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        square(&mut h, "Pocket", 20.0);
        extrude(&mut h, "Pocket", "Second", "new");
        h.settle();
        let block = handle_at("timeline:Block").unwrap();
        h.step(&json!({"drag": [{"handle": "timeline:Second"}, [block.x, block.y - 15.0]]})).unwrap();
        h.settle();
        assert_eq!(feature_names(&h), vec!["Pocket", "Second", "Profile", "Block"], "the sketch moves with its feature");
        assert!(h.app.session.model.state().bodies.len() == 2);

        // The move's own checks still apply: a cut moved above the body it cuts is refused, and
        // its sketch stays with it.
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Slot"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [4, 4]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("solid.extrude", json!({"sketch": "Slot", "distance": 5, "operation": "cut", "name": "Cut"})).unwrap();
        h.settle();
        let block = handle_at("timeline:Block").unwrap();
        h.step(&json!({"drag": [{"handle": "timeline:Cut"}, [block.x, block.y - 15.0]]})).unwrap();
        h.settle();
        assert_eq!(feature_names(&h), vec!["Profile", "Block", "Slot", "Cut"]);
        let status = h.app.status.clone().map(|(text, _, error)| (text, error));
        assert!(status.as_ref().is_some_and(|(text, error)| *error && text.contains("breaks")), "the move was refused: {status:?}");
    }

    #[test]
    fn a_dialog_taking_features_gets_clicks_on_a_feature_and_on_the_sketch_under_it() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        let block = h.app.session.doc.find_feature("Block").unwrap().id;
        h.app.run("select.clear", json!({})).unwrap();
        h.settle();
        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Block"}})).unwrap();
        h.app.start("solid.pattern.rectangular");
        h.frames(4);
        h.step(&json!({"click": {"handle": "dialog:Object Type"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Object Type:Features"}})).unwrap();
        h.step(&json!({"click": {"handle": "timeline:Block"}})).unwrap();
        let picked = h.app.dialog.as_ref().map(|dialog| dialog.inputs[0].items.clone());
        assert_eq!(picked, Some(vec![solidcraft_engine::Sel::Feature { id: block }]), "the feature row's click goes to the dialog");
        // Wait out the double-click time: a double click edits.
        h.frames(30);
        h.step(&json!({"click": {"handle": "timeline:Profile"}})).unwrap();
        assert!(h.app.dialog.is_some() && h.app.session.active_sketch.is_none(), "the sketch row's click goes to the dialog too");
        assert!(h.app.session.selection.is_empty(), "dialog picks leave the ordinary selection alone");
    }

    #[test]
    fn find_in_timeline_opens_the_feature_a_sketch_is_listed_under() {
        let mut h = nested_history();
        let profile = square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        h.settle();
        assert!(handle_at("timeline:Profile").is_none());
        let menu = crate::context_menu::items(&h.app, &crate::context_menu::Target::Sketch { id: profile });
        let item = menu.into_iter().find(|item| item.id == "ui.findTimeline").unwrap();
        crate::context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
        h.settle();
        assert!(row_y("timeline:Block") < row_y("timeline:Profile"), "Find in Timeline opens the feature's fold");
        assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Feature { id: profile }]);
    }

    #[test]
    fn a_listed_sketch_keeps_its_pattern_rows() {
        let mut h = nested_history();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Patterned"})).unwrap();
        let sketch = h.app.session.active_sketch.unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 20], [5, 20]], "ids": ["source"]})).unwrap();
        let pattern = h.app.run("sketch.pattern.rectangular", json!({"entities": ["source"], "count": 3, "spacing": 10})).unwrap();
        h.app.finish_sketch();
        extrude(&mut h, "Patterned", "Block", "new");
        h.settle();
        let pattern_row = format!("timeline:sketch-pattern:{sketch}:{}", pattern["pattern"].as_str().unwrap());
        assert!(handle_at(&pattern_row).is_none());
        h.step(&json!({"click": {"handle": "timeline-sketches-fold:Block"}})).unwrap();
        h.settle();
        assert!(row_y("timeline:Block") < row_y("timeline:Patterned") && row_y("timeline:Patterned") < row_y(&pattern_row));
        h.step(&json!({"click": {"handle": "timeline-sketch-fold:Patterned"}})).unwrap();
        h.settle();
        assert!(handle_at(&pattern_row).is_none(), "the listed sketch folds its patterns");
        assert!(handle_at("timeline:Patterned").is_some());
    }

    /// Folders that nest three deep, a folder with only joints, a stale feature id, a parent that
    /// is no folder any more, and a feature listed in two folders: the cases the plan's tables
    /// have to place as the per-frame searches did.
    fn tangled_folders(h: &mut Harness) {
        let ids: Vec<u64> = h.app.session.doc.features.iter().map(|feature| feature.id).collect();
        let group = |id: u64, name: &str, parent: Option<u64>, features: Vec<u64>, joints: Vec<u64>| solidcraft_engine::doc::TimelineGroup {
            id,
            name: name.into(),
            parent,
            features,
            joints,
        };
        let doc = h.app.session.doc_mut();
        doc.timeline_groups = vec![
            group(9001, "Outer", None, vec![ids[1], ids[2]], vec![]),
            group(9002, "Middle", Some(9001), vec![ids[3], 777_777], vec![]),
            group(9003, "Inner", Some(9002), vec![ids[4]], vec![55]),
            group(9004, "Joints only", Some(9001), vec![], vec![56, 57]),
            group(9005, "Loose", None, vec![ids[6], ids[7]], vec![]),
            group(9006, "Orphan", Some(4242), vec![ids[8]], vec![]),
            group(9007, "Twice", None, vec![ids[7], ids[9]], vec![]),
            group(9008, "Empty", Some(9005), vec![], vec![]),
        ];
        h.app.session.revision += 1;
    }

    /// The plan's tables against the per-frame searches they replaced, in every fold state.
    fn assert_plan_matches_the_searches(h: &mut Harness, under_feature: bool) {
        let table = RowPlan::of(&h.app, under_feature);
        let app = &h.app;
        let (features, groups) = (&table.features, &table.groups);
        let n = features.len();
        assert_eq!(table.rows, drawn_rows(app, under_feature));
        for (index, feature) in features.iter().enumerate() {
            assert_eq!(table.group_of[index], group_for(groups, feature.0), "{}", feature.1);
            let sketch = matches!(feature.5, TimelineKind::Sketch);
            assert_eq!(sketch && table.has_patterns[index], sketch && crate::sketch_pattern_tree::has_patterns(app, feature.0));
        }
        for (at, group) in groups.iter().enumerate() {
            assert_eq!(table.depth[at], group_depth(groups, group.id), "{}", group.name);
            assert_eq!(table.first[at], group_first_feature(groups, features, group.id), "{}", group.name);
            assert_eq!(table.position[at], group_position(groups, features, group.id), "{}", group.name);
            assert_eq!(table.end[at], group_last_feature(groups, features, group.id).map_or(n, |index| index + 1), "{}", group.name);
            assert_eq!(table.contents[at], folder_features(app, &[group.id]), "{}", group.name);
            let joints: Vec<u64> = groups
                .iter()
                .filter(|candidate| candidate.id == group.id || group_is_descendant(groups, candidate.id, group.id))
                .flat_map(|group| group.joints.iter().copied())
                .collect();
            assert_eq!(table.joints[at], joints, "{}", group.name);
        }
        let index_of = |group: &solidcraft_engine::doc::TimelineGroup| groups.iter().position(|candidate| std::ptr::eq(candidate, group)).unwrap();
        for index in 0..n {
            let mut at_index: Vec<_> = groups.iter().filter(|group| group_position(groups, features, group.id) == Some(index)).collect();
            at_index.sort_by_key(|group| group_path(groups, group.id));
            assert_eq!(table.starting[index], at_index.into_iter().map(index_of).collect::<Vec<_>>(), "headers before {index}");
            let mut ending: Vec<_> = groups.iter().filter(|group| group_last_feature(groups, features, group.id) == Some(index)).collect();
            ending.sort_by_key(|group| std::cmp::Reverse(group_depth(groups, group.id)));
            assert_eq!(table.ending[index], ending.into_iter().map(index_of).collect::<Vec<_>>(), "folders ending at {index}");
        }
        let mut trailing: Vec<_> = groups.iter().collect();
        trailing.sort_by_key(|group| (group_position(groups, features, group.id).unwrap_or(usize::MAX), group_path(groups, group.id)));
        assert_eq!(table.trailing, trailing.into_iter().map(index_of).collect::<Vec<_>>());
        // Every combination of closed folders (and a stale key for the missing parent), with
        // and without the listed sketches shown.
        let keys: Vec<String> = groups.iter().map(|group| group.id).chain([4242]).map(|id| format!("timeline_group:{id}")).collect();
        let hosts: Vec<String> = table.rows.rows.iter().filter(|row| !row.listed.is_empty()).map(|row| sketches_key(row.feature)).collect();
        let kept = (h.app.tree.history_folds.clone(), h.app.tree.expanded.clone());
        for closed in 0..(1u32 << keys.len()) {
            for open_hosts in [false, true] {
                let app = &mut h.app;
                app.tree.history_folds = Default::default();
                for (_, key) in keys.iter().enumerate().filter(|(bit, _)| closed & (1 << bit) != 0) {
                    crate::history_folds::set_open(app, key, false);
                }
                app.tree.expanded = if open_hosts { hosts.iter().cloned().collect() } else { Default::default() };
                let folds = table.folds(app);
                assert_eq!(table.visible_order(&folds), visible_order(app, features, groups, &table.rows), "closed {closed:b}, hosts {open_hosts}");
                for group in groups {
                    assert_eq!(table.is_open(&folds, group.id), group_is_open(app, groups, group.id), "{}", group.name);
                    assert_eq!(table.ancestors_open(&folds, group.id), group_ancestors_are_open(app, groups, group.id), "{}", group.name);
                }
            }
        }
        (h.app.tree.history_folds, h.app.tree.expanded) = kept;
    }

    #[test]
    fn the_row_plan_places_folders_and_rows_as_the_per_frame_searches_did() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        square(&mut h, "Pocket", 20.0);
        extrude(&mut h, "Pocket", "Second", "new");
        for (k, x) in [40.0, 60.0, 80.0].into_iter().enumerate() {
            let name = format!("Sketch {k}");
            square(&mut h, &name, x);
            extrude(&mut h, &name, &format!("Body {k}"), "new");
        }
        h.app.run("timeline.roll_to", json!({"position": 7})).unwrap();
        tangled_folders(&mut h);
        h.settle();
        for under_feature in [false, true] {
            assert_plan_matches_the_searches(&mut h, under_feature);
        }
        // Without folders, and with an empty design.
        h.app.session.doc_mut().timeline_groups.clear();
        assert_plan_matches_the_searches(&mut h, true);
        assert_plan_matches_the_searches(&mut Harness::new(), false);
    }

    #[test]
    fn the_kept_row_plan_is_the_one_built_afresh_after_every_change() {
        let mut h = nested_history();
        square(&mut h, "Profile", 0.0);
        extrude(&mut h, "Profile", "Block", "new");
        square(&mut h, "Pocket", 20.0);
        extrude(&mut h, "Pocket", "Second", "new");
        h.settle();
        let check = |h: &mut Harness, what: &str| {
            h.frames(2);
            let kept = RowPlan::kept(&h.ctx).expect("the History drew a plan");
            assert_eq!(kept.summary(), RowPlan::of(&h.app, kept.under_feature()).summary(), "{what}");
        };
        check(&mut h, "opened");
        let id = |h: &Harness, name: &str| h.app.session.doc.find_feature(name).unwrap().id;
        let (block, second) = (id(&h, "Block"), id(&h, "Second"));
        assert!(RowPlan::kept(&h.ctx).unwrap().under_feature(), "sketches are listed under their features");
        let steps: [(&str, serde_json::Value); 6] = [
            ("timeline.group", json!({"name": "Housing", "features": ["Profile", "Block"]})),
            ("timeline.rename", json!({"feature": second, "name": "Lid"})),
            ("timeline.suppress", json!({"feature": block})),
            ("timeline.roll_to", json!({"position": 2})),
            ("timeline.roll_to", json!({})),
            ("timeline.move_many", json!({"features": [block], "position": 4, "group": null, "active": true})),
        ];
        for (command, params) in steps {
            h.app.run(command, params.clone()).unwrap_or_else(|e| panic!("{command} {params}: {e}"));
            check(&mut h, command);
        }
        h.app.run("edit.undo", json!({})).unwrap();
        check(&mut h, "undo");
        // An edit in place without a revision, and the preference.
        h.app.session.doc_mut().marker = Some(1);
        check(&mut h, "marker set in place");
        h.app.preferences.history_sketches = crate::prefs::HistorySketches::InOrder;
        check(&mut h, "sketches in order");
        // Another design in the same window.
        let other = h.app.session.doc.clone();
        h.app.session = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::default());
        check(&mut h, "new design");
        h.app.session = solidcraft_engine::Session::new((*other).clone());
        check(&mut h, "the first design again");
    }

    /// A history taller than its panel, drawn with every row and with the rows out of view left
    /// out: the same places, the same text in view, and only the rows in view painted.
    #[test]
    fn rows_out_of_view_keep_their_places_and_what_is_in_view_draws_the_same() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.call("ui.resize", json!({"width": 1200, "height": 700}));
        for k in 0..60 {
            h.app
                .run(
                    "solid.box",
                    json!({"length": 5, "width": 5, "height": 5, "corner": [k * 10, 0, 0], "operation": "new_body", "name": format!("Box {k}")}),
                )
                .unwrap();
        }
        let ids: Vec<u64> = h.app.session.doc.features.iter().map(|feature| feature.id).collect();
        h.app.run("timeline.group", json!({"name": "Front", "features": ids[5..15]})).unwrap();
        h.app.run("timeline.roll_to", json!({"position": 40})).unwrap();
        h.settle();
        let draw = |h: &mut Harness, every: bool| {
            DRAW_EVERY_ROW.with(|cell| cell.set(every));
            h.frames(2);
            ROWS_DRAWN.with(|drawn| drawn.set(0));
            h.frame();
            let painted = ROWS_DRAWN.with(std::cell::Cell::get);
            DRAW_EVERY_ROW.with(|cell| cell.set(false));
            let card = h.app.shell.history.expect("history bounds");
            let places: Vec<_> = (0..60).map(|k| crate::scenario::rect_of(&format!("history:feature:Box {k}"))).collect();
            // The text that shows in the History: inside the card and inside its clip.
            let texts: Vec<String> = h
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some((clipped.clip_rect, text)),
                    _ => None,
                })
                .filter(|(clip, text)| text.visual_bounding_rect().intersects(card.intersect(*clip)))
                .map(|(_, text)| text.galley.text().to_string())
                .collect();
            (places, texts, painted)
        };
        for reveal in [None, Some(ids[30]), Some(ids[59])] {
            h.app.tree.timeline_reveal = reveal;
            // A second for the scroll to arrive.
            h.frames(60);
            let (every_places, every_texts, every_painted) = draw(&mut h, true);
            let (places, texts, painted) = draw(&mut h, false);
            assert!(places.iter().all(Option::is_some), "every row keeps its handle");
            assert_eq!(places, every_places, "{reveal:?}");
            assert_eq!(texts, every_texts, "{reveal:?}");
            assert!(texts.iter().any(|text| text.starts_with("Box ")), "{reveal:?}: rows are in view");
            assert_eq!(every_painted, 60, "{reveal:?}");
            assert!(painted < 40, "{reveal:?}: {painted} of {every_painted} rows drawn");
        }
        // A drag among the rows in view still moves a feature, with rows out of view left out.
        h.app.tree.timeline_reveal = Some(ids[0]);
        h.frames(60);
        let target = handle_at("timeline:Box 3").unwrap();
        h.step(&json!({"drag": [{"handle": "timeline:Box 1"}, [target.x, target.y + 8.0]]})).unwrap();
        let names = feature_names(&h);
        assert_eq!(names[..4], ["Box 0", "Box 2", "Box 3", "Box 1"], "{names:?}");
        assert_eq!(names.len(), 60);
    }

    /// Frame time of the vertical History alone on a large design (P5 measurement, run by hand):
    /// `SOLIDCRAFT_HISTORY_BENCH=<design> cargo test --release -p solidcraft-ui-egui --lib
    /// bench_history_panel -- --ignored --nocapture`. Prints one JSON line per case. The folders
    /// cases add 120 folders of 20 features (every fourth nested in the one before).
    #[test]
    #[ignore = "timing, run by hand with SOLIDCRAFT_HISTORY_BENCH"]
    fn bench_history_panel_frames() {
        let Some(path) = std::env::var_os("SOLIDCRAFT_HISTORY_BENCH") else { return };
        let frames: usize = std::env::var("SOLIDCRAFT_HISTORY_BENCH_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(40);
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("view.history_layout", json!({"dock":"left"})).unwrap();
        h.app.run("doc.open", json!({"path": path.to_string_lossy()})).unwrap();
        let ctx = crate::fonts::test_context();
        let size = egui::vec2(300.0, 1000.0);
        let mut time = 0.0;
        let mut run = |app: &mut SolveApp, events: Vec<egui::Event>| {
            time += 1.0 / 60.0;
            crate::scenario::clear_handles();
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(time),
                focused: true,
                events,
                ..Default::default()
            };
            let start = std::time::Instant::now();
            let mut out = ctx.run_ui(raw, |ui| panel_contents(app, ui));
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            out.textures_delta.clear();
            ms
        };
        let report = |case: &str, mut ms: Vec<f64>| {
            ms.sort_by(f64::total_cmp);
            let median = ms[ms.len() / 2];
            println!("{}", json!({"case": case, "median_ms": median, "min_ms": ms[0], "max_ms": ms[ms.len() - 1], "frames": ms.len()}));
        };
        let features = h.app.session.doc.features.len();
        println!("{}", json!({"features": features, "groups": h.app.session.doc.timeline_groups.len()}));
        let order: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
        let middle = h.app.session.doc.features[features / 2].id;
        let middle_name = h.app.session.doc.features[features / 2].name.clone();
        let moves = |y0: f32, i: usize| vec![egui::Event::PointerMoved(egui::pos2(150.0, y0 + (i % 7) as f32 * 3.0))];
        // SOLIDCRAFT_HISTORY_BENCH_CASES: "plain", "folders" or both (the default).
        let cases = std::env::var("SOLIDCRAFT_HISTORY_BENCH_CASES").unwrap_or_default();
        for folders in [false, true] {
            if !cases.is_empty() && cases != if folders { "folders" } else { "plain" } {
                continue;
            }
            if folders {
                // 120 folders of 20 features each, every fourth nested in the folder before it.
                let ids: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
                let doc = h.app.session.doc_mut();
                let mut next = doc.next_id.max(1_000_000);
                let mut previous = None;
                for (k, chunk) in ids.chunks(20).take(120).enumerate() {
                    let id = next;
                    next += 1;
                    let parent = if k % 4 == 3 { previous } else { None };
                    doc.timeline_groups.push(solidcraft_engine::doc::TimelineGroup {
                        id,
                        name: format!("Folder {k}"),
                        parent,
                        features: chunk.to_vec(),
                        joints: Vec::new(),
                    });
                    previous = Some(id);
                }
                doc.next_id = next;
                h.app.session.revision += 1;
            }
            let tag = if folders { "folders" } else { "plain" };
            for _ in 0..3 {
                run(&mut h.app, Vec::new());
            }
            report(&format!("{tag}/top idle"), (0..frames).map(|_| run(&mut h.app, Vec::new())).collect());
            report(&format!("{tag}/top hover"), (0..frames).map(|i| run(&mut h.app, moves(300.0, i))).collect());
            h.app.tree.timeline_reveal = Some(middle);
            for _ in 0..3 {
                run(&mut h.app, Vec::new());
            }
            report(&format!("{tag}/middle idle"), (0..frames).map(|_| run(&mut h.app, Vec::new())).collect());
            report(&format!("{tag}/middle hover"), (0..frames).map(|i| run(&mut h.app, moves(400.0, i))).collect());
            // Drag the middle feature: press on it, then move the pointer each frame.
            crate::scenario::clear_handles();
            run(&mut h.app, Vec::new());
            let at = handle_at(&format!("timeline:{middle_name}")).unwrap_or(egui::pos2(150.0, 500.0));
            run(&mut h.app, vec![egui::Event::PointerMoved(at)]);
            run(
                &mut h.app,
                vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE }],
            );
            let drag: Vec<f64> =
                (0..frames).map(|i| run(&mut h.app, vec![egui::Event::PointerMoved(at + egui::vec2(0.0, 12.0 + (i % 5) as f32 * 4.0))])).collect();
            report(&format!("{tag}/middle drag"), drag);
            // Let go with the pointer gone in the same frame: the drop has no place, so nothing moves.
            run(
                &mut h.app,
                vec![
                    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
                    egui::Event::PointerGone,
                ],
            );
            run(&mut h.app, Vec::new());
        }
        let after: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
        assert_eq!(after, order, "measuring moved nothing");
    }
}
