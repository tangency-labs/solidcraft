//! The left feature-history panel: named rows, folders, rollback and drag-and-drop.

use crate::ui_kit::{Container, Edge, Surface};
use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::timeline::{
    TimelineKind, clipboard_buttons, delete_after_marker_button, delete_feature, delete_features, icon_of, message_button, selected_feature_ids,
};
use crate::{SolveApp, icons};

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

/// Saved width; contents are selected from the actual available width at render time.
pub(crate) fn strip_width(app: &SolveApp) -> f32 {
    app.shell
        .history_drag_width
        .map(crate::ui_kit::history_clamp_width)
        .unwrap_or_else(|| crate::ui_kit::history_snap_width(app.preferences.timeline_left_width))
}

pub(crate) fn stacked(app: &SolveApp) -> bool {
    app.ui.show_timeline && app.preferences.timeline_left && app.preferences.timeline_under_browser
}

fn header_outer_height() -> f32 {
    let t = Tokens::get();
    crate::ui_kit::TREE_ROW_HEIGHT + t.space_8 + t.space_2
}

/// Small, allocation-free transition. Wall-clock progress avoids accumulating lag when
/// a frame is delayed; retargeting starts at the visible value, including mid-flight.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FoldMotion {
    from: f32,
    to: f32,
    started: f64,
}

impl FoldMotion {
    fn value(self, now: f64, duration: f32) -> f32 {
        if duration <= 0.0 {
            return self.to;
        }
        let elapsed = ((now - self.started) / f64::from(duration)).clamp(0.0, 1.0) as f32;
        egui::lerp(self.from..=self.to, egui::emath::easing::cubic_out(elapsed))
    }

    fn advance(&mut self, target: f32, now: f64, duration: f32) -> f32 {
        let value = self.value(now, duration);
        if self.to != target {
            *self = Self { from: value, to: target, started: now };
        }
        self.value(now, duration)
    }
}

/// Calculate only two scalars per frame; cards and input geometry share the result.
pub(crate) fn animate_folds(app: &mut SolveApp, ctx: &egui::Context) {
    if !stacked(app) {
        app.shell.history_folding = None;
        app.shell.history_fold_motion = None;
        return;
    }
    let duration = ctx.global_style().animation_time.clamp(0.0, 0.12);
    let now = ctx.input(|input| input.time);
    let targets = [f32::from(app.preferences.timeline_left_browser_collapsed), f32::from(app.preferences.timeline_collapsed)];
    let motions = app.shell.history_fold_motion.get_or_insert_with(|| targets.map(|to| FoldMotion { from: to, to, started: now }));
    let progress = [motions[0].advance(targets[0], now, duration), motions[1].advance(targets[1], now, duration)];
    app.shell.history_folding = Some(progress);
    if progress != targets {
        ctx.request_repaint();
    }
}

/// Shared bounds keep browser and history at exactly the same width. Their split is half
/// the available height. One folded header gives the other the remainder; two folded
/// headers sit together at the top.
pub(crate) fn stacked_rects(app: &SolveApp, available: Rect) -> (Rect, Rect) {
    let progress = app
        .shell
        .history_folding
        .unwrap_or([f32::from(app.preferences.timeline_left_browser_collapsed), f32::from(app.preferences.timeline_collapsed)]);
    column_rects_at(app, available, progress[0], progress[1])
}

fn column_rects(app: &SolveApp, available: Rect, browser_collapsed: bool, history_collapsed: bool) -> (Rect, Rect) {
    column_rects_at(app, available, f32::from(browser_collapsed), f32::from(history_collapsed))
}

fn column_rects_at(app: &SolveApp, available: Rect, browser_fold: f32, history_fold: f32) -> (Rect, Rect) {
    let mut available = available;
    available.min.x += crate::agent_chat::left_inset(app, available);
    let gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
    let column = crate::browser::panel_bounds(available, None, crate::browser::browser_width(app.preferences.browser_width), gap);
    if !app.ui.show_browser {
        let height = egui::lerp(column.height()..=header_outer_height().min(column.height()), history_fold);
        let history = Rect::from_min_size(column.min, vec2(column.width(), height));
        return (Rect::NOTHING, history);
    }
    let budget = (column.height() - gap).max(0.0);
    let header = header_outer_height().min(budget / 2.0);
    let expanded_browser = egui::lerp(budget / 2.0..=(budget - header).max(0.0), history_fold);
    let browser_height = egui::lerp(expanded_browser..=header, browser_fold);
    let browser = Rect::from_min_size(column.min, vec2(column.width(), browser_height));
    let history_top = (browser.bottom() + gap).min(column.bottom());
    let history_height = egui::lerp((column.bottom() - history_top).max(0.0)..=header, history_fold);
    let history = Rect::from_min_size(pos2(column.left(), history_top), vec2(column.width(), history_height));
    (browser, history)
}

pub(crate) fn floating_rect(app: &SolveApp, available: Rect) -> Rect {
    if stacked(app) {
        return stacked_rects(app, available).1;
    }
    let gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
    let min = pos2((available.left() + crate::agent_chat::left_inset(app, available) + gap).min(available.right()), available.top());
    Rect::from_min_max(min, pos2((min.x + strip_width(app)).min((available.right() - gap).max(min.x)), (available.bottom() - gap).max(min.y)))
}

/// Full-height left card, or a card sharing the browser column below it.
pub fn left_panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let available = ui.available_rect_before_wrap();
    app.shell.history_dock_space = Some(available);
    if Container::for_layout(app.ui.layout, Surface::Panel).floating() || stacked(app) {
        let bounds = floating_rect(app, available);
        app.shell.history = None;
        if !bounds.is_finite() || bounds.width() < crate::ui_kit::history_width(false) || bounds.height() < t.control_height {
            return;
        }
        let container = Container::new(ui.ctx(), Surface::Panel).joined(crate::ui_kit::DockEdges { left: true, top: true, ..Default::default() });
        let frame = container.frame(ui.ctx(), t.panel, crate::frame::Side::Left).inner_margin(t.space_4);
        let inner = (bounds.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
        let card = Container::new(ui.ctx(), Surface::Panel)
            .area(egui::Id::new("sc_timeline_left_card"))
            .fixed_pos(bounds.min)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                let response = container.show_frame(ui, frame, available, |ui| {
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, inner)));
                    left_timeline(app, ui);
                });
                if stacked(app) {
                    let width = crate::browser::browser_width(app.preferences.browser_width);
                    crate::browser::resize_column(app, ui, response.response.rect, width, "left-history-rail");
                }
            });
        app.shell.history = Some(card.response.rect);
        crate::scenario::publish_rect("history:vertical", card.response.rect);
        if !stacked(app) {
            seam_handle(app, ui, card.response.rect);
        }
    } else {
        let width = strip_width(app).min(ui.available_width().max(0.0));
        if width < crate::ui_kit::history_width(false) {
            return;
        }
        let response = Container::new(ui.ctx(), Surface::Panel)
            .edge("sc_timeline_left", Edge::Left)
            .exact_size(width)
            .resizable(false)
            .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(t.panel).inner_margin(t.space_4))
            .show(ui, |ui| left_timeline(app, ui));
        crate::scenario::publish_rect("history:vertical", response.response.rect);
        app.shell.history = Some(response.response.rect);
        seam_handle(app, ui, response.response.rect);
    }
}

fn names_fit(width: f32) -> bool {
    let t = Tokens::get();
    width >= crate::ui_kit::history_width(true) - (t.space_8 + t.space_2)
}

fn seam_handle(app: &mut SolveApp, ui: &mut egui::Ui, card: Rect) {
    let seam = crate::ui_kit::HistorySeam::new(card);
    let bounds = seam.edge.union(seam.pill);
    Container::new(ui.ctx(), Surface::Canvas).area(egui::Id::new("history_seam_overlay")).order(egui::Order::Foreground).fixed_pos(bounds.min).show(
        ui.ctx(),
        |ui| {
            ui.set_min_size(bounds.size());
            ui.set_clip_rect(ui.ctx().content_rect());
            let result = crate::ui_kit::history_seam(ui, card, card.width() >= crate::ui_kit::history_width(true));
            if let Some(width) = result.width {
                app.shell.history_drag_width = Some(width);
                ui.ctx().request_repaint();
                if result.released {
                    let _ = app.run("view.history_layout", json!({"width": width}));
                    app.shell.history_drag_width = None;
                }
            } else if result.toggle {
                let width = crate::ui_kit::history_width(card.width() < crate::ui_kit::history_width(true));
                let _ = app.run("view.history_layout", json!({"width":width}));
                ui.ctx().request_repaint();
            }
        },
    );
}

/// Width alone chooses icons, names, horizontal transport and the optional title.
pub fn left_timeline(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::dialogs::versions_ui::soften_while_viewing(app, ui);
    let t = Tokens::get();
    let names = names_fit(ui.available_width());
    timeline_transport(app, ui, names);
    if stacked(app)
        && ((app.preferences.timeline_collapsed && app.shell.history_folding.is_none_or(|p| p[1] >= 1.0)) || ui.available_height() <= t.space_3)
    {
        return;
    }
    t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top(), t.border);
    ui.add_space(t.space_3);
    egui::ScrollArea::vertical()
        .id_salt("sc_left_timeline_scroll")
        // The animated card owns its height, including the last few closing frames.
        .min_scrolled_height(0.0)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE })
        .show(ui, |ui| vertical_features(app, ui, names));
}

fn timeline_transport(app: &mut SolveApp, ui: &mut egui::Ui, names: bool) {
    let t = Tokens::get();
    let count = crate::ui_kit::HistoryTransport::ALL.len() as f32;
    let height = if names { crate::ui_kit::TREE_ROW_HEIGHT } else { t.control_height * count + t.space_2 * (count - 1.0) };
    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    crate::scenario::publish_rect("history:transport", header);
    crate::scenario::publish_rect("history:header", header);
    let mut controls_right = header.right();
    if names && stacked(app) {
        let fold = Rect::from_center_size(pos2(controls_right - t.space_24 / 2.0, header.center().y), vec2(t.space_24, t.space_24));
        let response = crate::ui_kit::panel_disclosure_button_at(
            ui,
            fold,
            !app.preferences.timeline_collapsed,
            crate::i18n::tr(if app.preferences.timeline_collapsed { "ui.expand" } else { "ui.collapse" }),
        );
        crate::scenario::publish_handle("history:collapse", fold.center());
        if response.clicked() {
            let _ = app.run("view.history_layout", json!({"collapsed":!app.preferences.timeline_collapsed}));
        }
        controls_right = fold.left() - t.space_2;
    }
    if names {
        let mut grip = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(header.min, vec2(t.space_10, height)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let handle = crate::ui_kit::panel_grip(&mut grip).rect;
        let controls_width = crate::ui_kit::history_transport_width();
        let title = crate::i18n::tr("ui.history_title");
        let galley = ui.painter().layout_no_wrap(title.to_owned(), crate::fonts::semibold(t.font_heading), t.text);
        let text_left = handle.right() + t.space_8;
        let text_right = controls_right - controls_width - t.space_8;
        let drag_rect = if text_right - text_left >= galley.size().x {
            let title_rect = Rect::from_min_max(pos2(text_left, header.top()), pos2(text_right, header.bottom()));
            ui.painter().galley(pos2(text_left, header.center().y - galley.size().y / 2.0), galley, t.text);
            crate::scenario::publish_rect("history:title", title_rect);
            handle.union(title_rect)
        } else {
            handle
        };
        let drag = ui.interact(drag_rect, egui::Id::new("sc_history_dock_grip"), Sense::drag());
        crate::scenario::publish_handle("history:grip", drag_rect.center());
        dock_drag(app, ui, &drag);
    }
    for (i, control) in crate::ui_kit::HistoryTransport::ALL.into_iter().enumerate() {
        let side = if names { t.space_24 } else { t.control_height };
        let rect = if names {
            Rect::from_min_size(
                pos2(controls_right - crate::ui_kit::history_transport_width() + i as f32 * (side + t.space_2), header.center().y - side / 2.0),
                vec2(side, side),
            )
        } else {
            Rect::from_min_size(pos2(header.center().x - side / 2.0, header.top() + i as f32 * (side + t.space_2)), vec2(side, side))
        };
        if control.show(ui, rect).clicked() {
            crate::timeline::run_transport(app, control);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Dock {
    Bottom,
    Left,
    UnderBrowser,
}

impl Dock {
    fn id(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::UnderBrowser => "under_browser",
        }
    }
}

fn dock_target(app: &SolveApp, available: Rect, at: egui::Pos2) -> Option<Dock> {
    let t = Tokens::get();
    if !available.contains(at) {
        return None;
    }
    if at.y >= available.bottom() - t.space_24 * 2.0 {
        return Some(Dock::Bottom);
    }
    if at.x <= available.left() + t.space_24 * 2.0 {
        return Some(Dock::Left);
    }
    let column = stacked_rects(app, available).0;
    if app.ui.show_browser && at.x <= column.right() && at.y >= column.top() + header_outer_height() { Some(Dock::UnderBrowser) } else { None }
}

pub(crate) fn dock_drag(app: &mut SolveApp, ui: &egui::Ui, drag: &egui::Response) {
    if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let Some(available) = app.shell.history_dock_space else {
        return;
    };
    let target = ui.ctx().pointer_latest_pos().and_then(|at| dock_target(app, available, at));
    if drag.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(target) = target {
            let zone = match target {
                Dock::Bottom => {
                    if Container::for_layout(app.ui.layout, Surface::Panel).floating() {
                        crate::timeline::floating_rect(available)
                    } else {
                        Rect::from_min_max(pos2(available.left(), (available.bottom() - 40.0).max(available.top())), available.max)
                    }
                }
                Dock::Left => {
                    let mut rect = available;
                    let gap = if Container::for_layout(app.ui.layout, Surface::Panel).floating() { crate::shell::SHELL_GAP } else { 0.0 };
                    rect.min.x += gap;
                    rect.max.x = (rect.left() + strip_width(app)).min(available.right() - gap);
                    rect.max.y -= gap;
                    rect
                }
                Dock::UnderBrowser => column_rects(app, available, false, false).1,
            };
            let t = Tokens::get();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("history_dock_preview")));
            painter.rect(zone, t.radius_lg, t.accent.gamma_multiply(0.18), Stroke::new(t.space_2, t.accent), egui::StrokeKind::Inside);
            crate::scenario::publish_rect("history:dock_preview", zone);
            crate::scenario::publish_rect(&format!("history:dock:{}", target.id()), zone);
            let label = crate::i18n::tr(match target {
                Dock::Bottom => "ui.timeline_bottom",
                Dock::Left => "ui.timeline_left_sidebar",
                Dock::UnderBrowser => "ui.history_below_browser",
            });
            painter.text(zone.center(), egui::Align2::CENTER_CENTER, label, crate::fonts::semibold(t.font_heading), t.text);
        }
    }
    if drag.drag_stopped()
        && let Some(target) = target
    {
        let _ = app.run("view.history_layout", json!({"dock":target.id()}));
    }
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
    crate::ui_kit::history_marker(ui.painter(), rect, true, crate::theme::TabAccent::for_tab(&t, &app.ui.tab));
    crate::scenario::publish_handle("timeline:rollback", rect.center());
    crate::scenario::publish_rect("history:marker", rect);
    if response.drag_started() {
        *drag = Some(Drag::Marker);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response.on_hover_text(crate::i18n::tr("ui.history_marker_tip"));
    rect.center().y
}

fn group_for(groups: &[solidcraft_engine::doc::TimelineGroup], feature: u64) -> Option<u64> {
    groups.iter().find(|group| group.features.contains(&feature)).map(|group| group.id)
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

fn selection_actions(app: &mut SolveApp, ui: &mut egui::Ui, groups: &[solidcraft_engine::doc::TimelineGroup]) {
    let features = selected_feature_ids(app);
    let selected_groups = app.tree.picked_timeline_groups.clone();
    let has_selection = !features.is_empty() || !selected_groups.is_empty();
    if has_selection {
        if crate::context_menu::button(ui, "Add to New Folder", "folder").clicked() {
            let result = app.run("timeline.group", json!({"features": features, "groups": selected_groups}));
            if let Ok(result) = result
                && let Some(group) = result.get("group").and_then(serde_json::Value::as_u64)
            {
                app.tree.collapsed.remove(&format!("timeline_group:{group}"));
                crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::TimelineGroup(group), pos2(0.0, 0.0));
            }
            ui.close();
        }
        let grouped = features.iter().any(|feature| group_for(groups, *feature).is_some()) || !selected_groups.is_empty();
        if grouped && crate::context_menu::button(ui, "Remove from Folder", "folder").clicked() {
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
        if crate::context_menu::button(ui, "Delete Selected Folders", "delete")
            .on_hover_text("Keep their features and nested folders in the history")
            .clicked()
        {
            let ids = app.tree.picked_timeline_groups.clone();
            let _ = app.run("timeline.ungroup", json!({"groups": ids}));
            app.tree.picked_timeline_groups.clear();
            ui.close();
        }
    }
    if !features.is_empty() {
        ui.separator();
        if crate::context_menu::button(ui, "Delete Selected Features…", "delete").clicked() {
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
        if app.tree.collapsed.contains(&format!("timeline_group:{group_id}")) {
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

fn group_ancestors_are_open(app: &SolveApp, groups: &[solidcraft_engine::doc::TimelineGroup], id: u64) -> bool {
    let mut current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
    let mut steps = 0usize;
    while let Some(group_id) = current {
        if app.tree.collapsed.contains(&format!("timeline_group:{group_id}")) {
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

fn visible_order(
    app: &SolveApp,
    features: &[(u64, String, &'static str, bool, &'static str, TimelineKind)],
    groups: &[solidcraft_engine::doc::TimelineGroup],
) -> Vec<TimelineItem> {
    let mut order = Vec::new();
    let mut shown = std::collections::BTreeSet::new();
    for (index, (id, ..)) in features.iter().enumerate() {
        let mut at_index: Vec<_> = groups.iter().filter(|group| group_first_feature(groups, features, group.id) == Some(index)).collect();
        at_index.sort_by_key(|group| (group_depth(groups, group.id), group.id));
        for group in at_index {
            if group_ancestors_are_open(app, groups, group.id) && shown.insert(group.id) {
                order.push(TimelineItem::Group(group.id));
            }
        }
        let group_id = group_for(groups, *id);
        let open = group_id.is_none_or(|group_id| group_is_open(app, groups, group_id));
        if open {
            order.push(TimelineItem::Feature(*id));
        }
    }
    let mut trailing: Vec<_> = groups.iter().filter(|group| !shown.contains(&group.id)).collect();
    trailing.sort_by_key(|group| (group_first_feature(groups, features, group.id).unwrap_or(usize::MAX), group_depth(groups, group.id), group.id));
    for group in trailing {
        if group_ancestors_are_open(app, groups, group.id) && shown.insert(group.id) {
            order.push(TimelineItem::Group(group.id));
        }
    }
    order
}

fn timeline_group_name(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    group: &solidcraft_engine::doc::TimelineGroup,
    rect: Rect,
    offset: f32,
    names: bool,
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
            egui::FontId::proportional(crate::theme::Tokens::get().font_body),
            t.text,
        );
        return false;
    }

    let Some(mut rename) = app.menu.rename.take() else { return false };
    let left = rect.left() + 38.0 + offset;
    let edit_rect = Rect::from_min_max(pos2(left, rect.top() + 1.0), pos2(rect.right() - 3.0, rect.bottom() - 1.0));
    let edit_id = ui.id().with(("timeline_group_rename", group.id));
    let response = crate::ui_kit::inline_name(ui, edit_rect, edit_id, &mut rename.text);
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
        crate::context_menu::commit_rename(app, &rename);
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
    groups: &[solidcraft_engine::doc::TimelineGroup],
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
    let key = format!("timeline_group:{}", group.id);
    let open = !app.tree.collapsed.contains(&key);
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
            if selected { crate::theme::TabAccent::current(ui.ctx()).soft } else { t.hover },
        );
    }
    let offset = if names { depth as f32 * 14.0 } else { 0.0 };
    let centre = pos2(rect.left() + 9.0 + offset, rect.center().y);
    crate::ui_kit::disclosure(ui.painter(), centre, open, t.text_dim);
    let icon = Rect::from_center_size(pos2(rect.left() + 27.5 + offset, rect.center().y), vec2(17.0, 17.0));
    icons::paint_icon(ui.painter(), icon, "folder", f32::from(t.icon.a()) / 255.0);
    let renaming = timeline_group_name(app, ui, group, rect, offset, names, &t);
    crate::scenario::publish_handle(&format!("timeline-folder:{}", group.name), rect.center());
    let fold_rect = Rect::from_min_max(pos2(rect.left() + offset, rect.top()), pos2(rect.left() + offset + 18.0, rect.bottom()));
    crate::scenario::publish_handle(&format!("timeline-folder-fold:{}", group.name), fold_rect.center());
    let fold = ui.interact(fold_rect, ui.id().with(("timeline_group_fold", group.id)), Sense::click());
    if fold.clicked() {
        if open {
            app.tree.collapsed.insert(key.clone());
        } else {
            app.tree.collapsed.remove(&key);
        }
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
        crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::TimelineGroup(group.id), rect.center());
    }
    if response.secondary_clicked() && !selected {
        select_only(app, TimelineItem::Group(group.id));
    }
    crate::ui_kit::Menu::context(&response, |ui| {
        if crate::context_menu::button(ui, "Rename Folder", "rename").clicked() {
            crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::TimelineGroup(group.id), rect.center());
            ui.close();
        }
        ui.separator();
        selection_actions(app, ui, groups);
    });
    open
}

fn vertical_features(app: &mut SolveApp, ui: &mut egui::Ui, names: bool) {
    let t = Tokens::get();
    ui.spacing_mut().item_spacing.y = if names { t.space_2 } else { t.space_4 };
    let features: Vec<_> = app
        .session
        .doc
        .features
        .iter()
        .map(|feature| {
            (feature.id, feature.name.clone(), icon_of(&feature.kind), feature.suppressed, feature.kind.type_name(), TimelineKind::of(&feature.kind))
        })
        .collect();
    let groups = app.session.doc.timeline_groups.clone();
    let order = visible_order(app, &features, &groups);
    let first_indices: std::collections::BTreeMap<u64, Option<usize>> =
        groups.iter().map(|group| (group.id, group_first_feature(&groups, &features, group.id))).collect();
    let end_indices: std::collections::BTreeMap<u64, usize> = groups
        .iter()
        .map(|group| (group.id, group_last_feature(&groups, &features, group.id).map_or(features.len(), |index| index.saturating_add(1))))
        .collect();
    let n = features.len();
    let marker = app.session.doc.marker.unwrap_or(n);
    let from_view = match &app.viewport.hover {
        Some(crate::viewport::Hit::Face { body, .. } | crate::viewport::Hit::Edge { body, .. } | crate::viewport::Hit::Vertex { body, .. }) => {
            app.session.world_state().body(body).map(|body| body.feature)
        }
        _ => None,
    };
    let drag_id = ui.id().with("tl_vertical_drag");
    let mut drag: Option<Drag> = ui.data(|data| data.get_temp(drag_id));
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let mut slots = Vec::new();
    let mut folder_drop_zones = Vec::new();
    let mut shown_groups = std::collections::BTreeSet::new();
    let mut previous_visible_group = None;
    let mut hovered = None;
    let mut edit = None;
    let mut marker_y = f32::INFINITY;

    for (index, (id, name, icon, suppressed, ty, kind)) in features.iter().enumerate() {
        let group_id = group_for(&groups, *id);
        let mut at_index: Vec<_> = groups.iter().filter(|group| first_indices.get(&group.id).copied().flatten() == Some(index)).collect();
        at_index.sort_by_key(|group| (group_depth(&groups, group.id), group.id));
        let marker_before_group = index == marker && !at_index.is_empty();
        if marker_before_group {
            let group = at_index.first().and_then(|group| group.parent);
            let indent_group = previous_visible_group.filter(|previous| Some(*previous) != group);
            slots.push(VerticalSlot { y: ui.cursor().top(), position: index, group, indent_group });
            marker_y = vertical_marker(app, ui, &mut drag);
        }
        let mut rendered_group_header = false;
        for group in at_index {
            if group_ancestors_are_open(app, &groups, group.id) && shown_groups.insert(group.id) {
                vertical_group_row(
                    app,
                    names,
                    ui,
                    group,
                    &groups,
                    &order,
                    &mut drag,
                    group_depth(&groups, group.id),
                    index,
                    end_indices.get(&group.id).copied().unwrap_or(index),
                    previous_visible_group,
                    &mut slots,
                    &mut folder_drop_zones,
                );
                rendered_group_header = true;
            }
        }
        let group_open = group_id.is_none_or(|id| group_is_open(app, &groups, id));
        let indent_group = (!rendered_group_header).then_some(previous_visible_group).flatten().filter(|previous| Some(*previous) != group_id);
        if index == marker && !marker_before_group {
            slots.push(VerticalSlot { y: ui.cursor().top(), position: index, group: group_open.then_some(group_id).flatten(), indent_group });
            marker_y = vertical_marker(app, ui, &mut drag);
        }
        if !group_open {
            continue;
        }
        slots.push(VerticalSlot { y: ui.cursor().top(), position: index, group: group_id, indent_group });
        let indent = group_id.map(|id| (group_depth(&groups, id) + 1) as f32 * 14.0).unwrap_or(0.0);
        let has_patterns = matches!(kind, TimelineKind::Sketch) && crate::sketch_pattern_tree::has_patterns(app, *id);
        let pattern_key = format!("timeline/sketch/{id}/patterns");
        let patterns_open = crate::browser::is_open(app, &pattern_key, true);
        let style = if names { crate::ui_kit::HistoryStyle::Names } else { crate::ui_kit::HistoryStyle::Icons };
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), style.height()), Sense::click_and_drag());
        let rect = if style == crate::ui_kit::HistoryStyle::Icons {
            Rect::from_center_size(row.center(), vec2(style.height(), style.height()))
        } else {
            row
        };
        crate::scenario::publish_rect(&format!("history:feature:{name}"), rect);
        let fold_rect = Rect::from_min_size(rect.left_top() + vec2(indent, 0.0), vec2(16.0, rect.height()));
        let on_fold = has_patterns && response.interact_pointer_pos().is_some_and(|p| fold_rect.contains(p));
        crate::scenario::publish_handle(&format!("timeline:{name}"), rect.center());
        let result = app.session.model.result(*id);
        let elapsed = result.map(|r| r.ms);
        let error = result.and_then(|result| result.error.clone());
        let warning = result.and_then(|result| result.warning.clone());
        let rolled = index >= marker;
        let selected =
            app.session.selection.iter().any(|selection| matches!(selection, solidcraft_engine::Sel::Feature { id: selected } if selected == id));
        // Folder and sketch-pattern disclosure targets retain their original handles.
        let inset = if names { indent + if has_patterns { 16.0 } else { 0.0 } } else { 0.0 };
        let chip = Rect::from_min_max(rect.min + vec2(inset.min(rect.width()), 0.0), rect.max);
        crate::ui_kit::HistoryChip {
            icon,
            name,
            kind: ty,
            selected,
            hovered: response.hovered() || from_view == Some(*id),
            dim: rolled,
            suppressed: *suppressed,
            error: error.is_some(),
            warning: warning.is_some(),
            // Dragged features take the colour of their drop bar.
            accent: crate::theme::TabAccent::for_tab(
                &t,
                if matches!(&drag, Some(Drag::Items(items)) if items.features.contains(id)) { "SKETCH" } else { &app.ui.tab },
            ),
        }
        .paint(ui.painter(), chip, style);
        if has_patterns {
            crate::ui_kit::disclosure(ui.painter(), fold_rect.center(), patterns_open, t.text_dim);
            crate::scenario::publish_handle(&format!("timeline-sketch-fold:{name}"), fold_rect.center());
        }
        if response.hovered() {
            hovered = Some(*id);
        }
        if response.drag_started() && !on_fold {
            if !selected {
                select_only(app, TimelineItem::Feature(*id));
            }
            let origin_x = drag_press_x(&response, rect.center().x);
            drag = Some(Drag::Items(selected_drag_items(app, origin_x, group_id)));
        }
        let mut tooltip = egui::Tooltip::for_enabled(&response);
        tooltip.popup = tooltip.popup.align(egui::RectAlign::RIGHT_START);
        tooltip.show(|ui| {
            let summary = app
                .session
                .doc
                .features
                .get(index)
                .and_then(|f| f.kind.expressions().first().map(ToString::to_string))
                .unwrap_or_else(|| elapsed.map_or_else(String::new, |ms| format!("{ms:.1} ms")));
            let kind = crate::i18n::tr_label(ty.strip_suffix("Feature").unwrap_or(ty));
            let mut tip = format!("{name} · {kind} · {summary}");
            if error.is_some() || warning.is_some() {
                tip.push_str(&format!("\n{}", crate::timeline::status_text(name, ty, error.as_deref(), warning.as_deref())));
            }
            crate::ui_kit::text(ui, &tip, crate::ui_kit::Text::Value("history", false));
        });
        if response.clicked() && on_fold {
            crate::browser::toggle(app, &pattern_key, true);
        } else if response.clicked() {
            select_item(app, ui, TimelineItem::Feature(*id), &order);
        }
        if response.double_clicked() && !on_fold {
            edit = Some(*id);
        }
        let sketch = kind.profile_sketch();
        if response.secondary_clicked() && !selected {
            select_only(app, TimelineItem::Feature(*id));
        }
        crate::ui_kit::Menu::context(&response, |ui| {
            if crate::context_menu::button(ui, "Edit Feature", "edit").clicked() {
                edit = Some(*id);
                ui.close();
            }
            if let Some(sketch) = sketch
                && crate::context_menu::button(ui, "Edit Profile Sketch", "sketch").clicked()
            {
                app.edit_sketch(sketch.0);
                ui.close();
            }
            if crate::context_menu::button(ui, "Rename Feature", "rename").clicked() {
                crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::Feature(*id), rect.center());
                ui.close();
            }
            if crate::context_menu::button(ui, if *suppressed { "Unsuppress Feature" } else { "Suppress Feature" }, "suppress").clicked() {
                let _ = app.run_heavy("timeline.suppress", json!({"feature": id}));
                ui.close();
            }
            if crate::context_menu::button(ui, "Roll Back Here", "rollback").clicked() {
                let _ = app.run_heavy("timeline.roll_to", json!({"feature": id}));
                ui.close();
            }
            ui.separator();
            clipboard_buttons(app, ui, *id);
            message_button(app, ui, *id);
            ui.separator();
            selection_actions(app, ui, &groups);
            ui.separator();
            if crate::context_menu::button(ui, "Delete", "delete").clicked() {
                delete_feature(app, *id);
                ui.close();
            }
            delete_after_marker_button(app, ui, *id, index, marker);
        });
        if names && has_patterns && patterns_open {
            let depth = group_id.map_or(1, |group| group_depth(&groups, group) + 2);
            crate::sketch_pattern_tree::rows(app, ui, *id, depth, "timeline", None, false);
        }
        previous_visible_group = group_id;
    }
    let mut trailing: Vec<_> = groups.iter().filter(|group| !shown_groups.contains(&group.id)).collect();
    trailing.sort_by_key(|group| (first_indices.get(&group.id).copied().flatten().unwrap_or(usize::MAX), group_depth(&groups, group.id), group.id));
    for group in trailing {
        if group_ancestors_are_open(app, &groups, group.id) && shown_groups.insert(group.id) {
            vertical_group_row(
                app,
                names,
                ui,
                group,
                &groups,
                &order,
                &mut drag,
                group_depth(&groups, group.id),
                n,
                n,
                previous_visible_group,
                &mut slots,
                &mut folder_drop_zones,
            );
        }
    }
    slots.push(VerticalSlot { y: ui.cursor().top(), position: n, group: None, indent_group: previous_visible_group });
    if marker >= n {
        marker_y = vertical_marker(app, ui, &mut drag);
        // Below the marker at the end: a feature dropped here is rolled back.
        slots.push(VerticalSlot { y: ui.cursor().top(), position: n, group: None, indent_group: previous_visible_group });
    }
    crate::dialogs_assembly::timeline_joints_vertical(app, ui);

    if drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // The marker dropped where it already is needs no bar: the marker shows the spot.
    if let (Some(active), Some(pointer)) = (drag.as_ref(), pointer)
        && let Some(slot) = nearest_vertical_slot(&slots, pointer.y)
        && !(matches!(active, Drag::Marker) && slot.position == marker.min(n))
    {
        let folder_target = match active {
            Drag::Items(items) => {
                let panel_left = ui.min_rect().left();
                vertical_drop_group(pointer, slot, &folder_drop_zones, &groups, panel_left, drag_x(items, pointer.x), items.source_group)
                    .filter(|target| valid_folder_target(items, &groups, *target))
            }
            Drag::Marker => None,
        };
        let hovered_folder = folder_target.and_then(|target| {
            folder_drop_zones.iter().find(|(rect, group)| *group == target && rect.shrink2(vec2(0.0, 5.0)).contains(pointer)).map(|(rect, _)| *rect)
        });
        if let Some(rect) = hovered_folder {
            ui.painter().rect_stroke(rect.shrink(1.0), t.radius_sm.saturating_sub(1), Stroke::new(2.0, t.sketch_accent), egui::StrokeKind::Inside);
        } else {
            let left = vertical_drop_bar_left(ui.min_rect().left(), folder_target, &groups);
            crate::ui_kit::history_drop_bar(
                ui.painter(),
                Rect::from_min_max(pos2(left, slot.y), pos2(ui.max_rect().right() - 5.0, slot.y)),
                true,
                if matches!(active, Drag::Marker) { crate::theme::TabAccent::current(ui.ctx()).color } else { t.sketch_accent },
            );
        }
    }
    if ui.input(|input| input.pointer.any_released())
        && let Some(active) = drag.take()
        && let Some(pointer) = pointer
        && let Some(slot) = nearest_vertical_slot(&slots, pointer.y)
    {
        match active {
            Drag::Marker => {
                let _ = app.run_heavy("timeline.roll_to", if slot.position >= n { json!({}) } else { json!({"position": slot.position}) });
            }
            Drag::Items(items) => {
                let panel_left = ui.min_rect().left();
                let folder_target =
                    vertical_drop_group(pointer, slot, &folder_drop_zones, &groups, panel_left, drag_x(&items, pointer.x), items.source_group);
                let valid_target = folder_target.is_none_or(|target| valid_folder_target(&items, &groups, target));
                if valid_target {
                    let mut params = json!({"group": folder_target, "position": slot.position});
                    if !items.features.is_empty() {
                        params["features"] = json!(items.features);
                    }
                    if !items.groups.is_empty() {
                        params["groups"] = json!(items.groups);
                    }
                    // Crossing the marker rolls the moved features back or forward, in the same step.
                    params["active"] = json!(slot.y < marker_y);
                    let _ = app.run("timeline.move_many", params);
                    if let Some(group) = folder_target {
                        app.tree.collapsed.remove(&format!("timeline_group:{group}"));
                    }
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
    app.viewport.hover_feature = hovered;
    if let Some(feature) = edit {
        app.edit_feature(feature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Harness, handle_at};

    #[test]
    fn docking_previews_match_all_three_destinations_and_stacked_collapse_shares_height() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = mode;
            h.call("ui.resize", json!({"width":1400,"height":1000}));
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_left_width = 240.0;
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"First"})).unwrap();
            h.frames(6);
            let design = h.app.session.doc.to_json();
            let projection = h.app.viewport.rect;
            for (dock, point) in [("bottom", pos2(700.0, 990.0)), ("left", pos2(10.0, 400.0)), ("under_browser", pos2(130.0, 700.0))] {
                let grip = handle_at("history:grip").expect("dock grip");
                h.app.synthetic.push(egui::Event::PointerMoved(grip));
                h.frame();
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: grip,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frame();
                h.app.synthetic.push(egui::Event::PointerMoved(point));
                h.frames(2);
                let preview = crate::scenario::rect_of("history:dock_preview").expect("dock preview");
                h.app.synthetic.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
                h.frames(6);
                assert_eq!(h.app.preferences.timeline_left, dock != "bottom");
                assert_eq!(h.app.preferences.timeline_under_browser, dock == "under_browser");
                let card = h.app.shell.history.expect("history bounds");
                assert!(
                    (preview.min - card.min).length() < 1.0 && (preview.max - card.max).length() < 1.0,
                    "{mode:?} {dock}: preview={preview:?} card={card:?}"
                );
                assert_eq!(h.app.session.doc.to_json(), design);
                if mode == crate::shell::LayoutMode::Floating {
                    assert_eq!(h.app.viewport.rect, projection);
                }
            }
            let browser = h.app.shell.browser.unwrap();
            let history = h.app.shell.history.unwrap();
            assert!((browser.width() - history.width()).abs() < 1.0);
            assert!((browser.height() - history.height()).abs() < 1.0);
            assert!((history.top() - browser.bottom() - Container::for_layout(mode, Surface::Panel).gap()).abs() < 1.0);
            let at = handle_at("history:collapse").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            h.frames(5);
            assert!(h.app.shell.history.unwrap().height() <= header_outer_height() + 1.0);
            assert!(h.app.shell.browser.unwrap().height() > browser.height() * 1.5);
            let at = handle_at("left-browser-toggle").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            h.frames(5);
            assert!(h.app.preferences.timeline_left_browser_collapsed);
            assert!(h.app.preferences.timeline_collapsed);
            let folded_browser = h.app.shell.browser.unwrap();
            let folded_history = h.app.shell.history.unwrap();
            assert!(folded_browser.height() <= header_outer_height() + 1.0);
            assert!(folded_history.height() <= header_outer_height() + 1.0);
            assert!((folded_history.top() - folded_browser.bottom() - Container::for_layout(mode, Surface::Panel).gap()).abs() < 1.0);
            let at = handle_at("history:collapse").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            h.frames(5);
            assert!(h.app.preferences.timeline_left_browser_collapsed);
            assert!(!h.app.preferences.timeline_collapsed);
            assert!(h.app.shell.history.unwrap().height() > history.height() * 1.5);
            h.app.preferences.browser_width = 310.0;
            h.frames(5);
            assert!((h.app.shell.history.unwrap().width() - 310.0).abs() < 1.0);
            assert!((h.app.shell.browser.unwrap().width() - 310.0).abs() < 1.0);
            // Browser-first collapse must also leave both independent folded headers at the top.
            let at = handle_at("history:collapse").unwrap();
            h.call("ui.click", json!({"x":at.x,"y":at.y}));
            h.frames(5);
            assert!(h.app.preferences.timeline_left_browser_collapsed && h.app.preferences.timeline_collapsed);
            assert!((h.app.shell.browser.unwrap().top() - browser.top()).abs() < 1.0);
            assert!(h.app.shell.history.unwrap().bottom() < browser.top() + header_outer_height() * 2.0 + crate::shell::SHELL_GAP + 1.0);
        }
    }

    #[test]
    fn folding_cards_follow_their_bounds_through_the_last_animation_frame() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = mode;
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_under_browser = true;
            h.app.preferences.browser_width = 220.0;
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            h.frames(6);
            // Both closing orders, expansion, and reversal before a transition completes.
            for (browser, history, frames) in
                [(true, false, 12), (true, true, 12), (false, true, 12), (false, false, 12), (true, true, 3), (false, false, 12)]
            {
                h.app.preferences.timeline_left_browser_collapsed = browser;
                h.app.preferences.timeline_collapsed = history;
                for frame in 0..frames {
                    h.frame();
                    let available = h.app.shell.history_dock_space.unwrap();
                    let expected = stacked_rects(&h.app, available);
                    for (name, actual, target) in
                        [("Browser", h.app.shell.browser.unwrap(), expected.0), ("History", h.app.shell.history.unwrap(), expected.1)]
                    {
                        assert!(
                            (actual.min - target.min).length() < 1.0 && (actual.max - target.max).length() < 1.0,
                            "{mode:?} {name}, frame {frame}, folds ({browser}, {history}): {actual:?} != {target:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn all_four_transport_buttons_move_to_their_named_positions_in_every_layout() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for (dock, width) in
                [("bottom", 130.0), ("left", 56.0), ("left", crate::ui_kit::history_width(true)), ("left", 320.0), ("under_browser", 170.0)]
            {
                let mut h = Harness::new();
                h.app.home.open = false;
                h.app.ui.layout = mode;
                h.app.preferences.browser_width = 170.0;
                h.app.run("view.history_layout", json!({"dock":dock,"width":width})).unwrap();
                for i in 0..3 {
                    h.app
                        .run(
                            "solid.box",
                            json!({"length":10,"width":10,"height":10,"corner":[i*20,0,0],"operation":"new_body","name":format!("Step {i}")}),
                        )
                        .unwrap();
                }
                h.settle();
                let features = h.app.session.doc.features.clone();
                for control in crate::ui_kit::HistoryTransport::ALL {
                    let button = crate::scenario::rect_of(&format!("history:{}", control.id())).unwrap();
                    let header = crate::scenario::rect_of(if dock == "bottom" {
                        if mode == crate::shell::LayoutMode::Floating { "history:card" } else { "history:strip" }
                    } else {
                        "history:header"
                    })
                    .unwrap();
                    assert!(header.contains_rect(button), "{mode:?} {dock} {width}: transport button {button:?} outside {header:?}");
                }
                for (control, position) in [
                    ("first", Some(0)),
                    ("back", Some(0)),
                    ("forward", Some(1)),
                    ("forward", Some(2)),
                    ("back", Some(1)),
                    ("last", None),
                    ("back", Some(2)),
                    ("back", Some(1)),
                    ("forward", Some(2)),
                    ("forward", None),
                    ("forward", None),
                    ("first", Some(0)),
                    ("last", None),
                ] {
                    h.step(&json!({"click":{"handle":format!("history:{control}")}})).unwrap();
                    h.settle();
                    assert_eq!(h.app.session.doc.marker, position, "{mode:?} {dock} {width}: {control}");
                    assert_eq!(h.app.session.model.state().bodies.len(), position.unwrap_or(3));
                    assert_eq!(h.app.session.doc.features, features, "transport only moves the marker");
                }
            }
        }
    }

    #[test]
    fn seam_snaps_only_between_thin_and_medium_and_persists_free_widths() {
        let medium = crate::ui_kit::history_width(true);
        for (width, delta, expected) in [(56.0, 40.0, medium), (medium, -40.0, 56.0), (medium, 63.0, medium + 63.0), (320.0, -41.0, 279.0)] {
            let mut h = Harness::new();
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_left_width = width;
            h.settle();
            let card = crate::scenario::rect_of("history:vertical").unwrap();
            let start = pos2(card.right() - 4.0, card.top() + 12.0);
            h.step(&json!({"drag":[[start.x,start.y],[start.x+delta,start.y]]})).unwrap();
            h.settle();
            assert!((h.app.preferences.timeline_left_width - expected).abs() < 1.0);
            assert!(h.app.shell.history_drag_width.is_none());
            assert!((crate::scenario::rect_of("history:vertical").unwrap().width() - expected).abs() < 1.0);
            assert!(h.app.dialog.is_none());
            let mut restored = Harness::new();
            restored.app.load_prefs(&h.app.prefs());
            assert_eq!(restored.app.preferences.timeline_left_width, expected);
        }
    }

    #[test]
    fn contents_follow_measured_width_without_a_names_setting() {
        for width in [56.0, crate::ui_kit::history_width(true), 168.0, 213.0, 320.0] {
            let mut h = Harness::new();
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_left_width = width;
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"First"})).unwrap();
            h.settle();
            let row = crate::scenario::rect_of("history:feature:First").unwrap();
            let back = handle_at("history:back").unwrap();
            let play = handle_at("history:last").unwrap();
            assert_eq!(row.height(), if width < crate::ui_kit::history_width(true) { 38.0 } else { crate::ui_kit::TREE_ROW_HEIGHT });
            assert_eq!(handle_at("history:grip").is_some(), width >= crate::ui_kit::history_width(true));
            assert_eq!(back.y == play.y, width >= crate::ui_kit::history_width(true));
            if width == crate::ui_kit::history_width(true) {
                assert!(crate::scenario::rect_of("history:title").is_none());
            }
            if width >= 213.0 {
                assert!(crate::scenario::rect_of("history:title").is_some());
            }
            assert!(!crate::shortcut_box::search(&h.app, "history names").iter().any(|row| row.0 == "view.history_names"));
        }
    }

    #[test]
    fn transport_to_first_item_gap_is_at_most_twelve_points() {
        let previous = crate::theme::is_dark();
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
                for names in [false, true] {
                    let mut h = Harness::new();
                    h.app.ui.dark = dark;
                    h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
                    h.app.ui.layout = mode;
                    h.app.preferences.timeline_left = true;
                    h.app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
                    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"First"})).unwrap();
                    for selected in [false, true] {
                        if selected {
                            let id = h.app.session.doc.features.first().unwrap().id;
                            h.app.run("select.set", json!({"items":[{"type":"feature","id":id}]})).unwrap();
                        }
                        h.settle();
                        assert_eq!(crate::theme::is_dark(), dark);
                        let transport = crate::scenario::rect_of("history:transport").unwrap();
                        let first = crate::scenario::rect_of("history:feature:First").unwrap();
                        let gap = first.top() - transport.bottom();
                        assert!((0.0..=12.0).contains(&gap), "gap={gap} dark={dark} mode={mode:?} names={names} selected={selected}");
                    }
                }
            }
        }
        crate::theme::set_dark(previous);
    }

    #[test]
    fn rendered_strip_content_never_exceeds_the_viewport_during_resize() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for width in [70.0, 100.0, 130.0, 200.0] {
                for drag_width in [None, Some(60.0), Some(112.0), Some(160.0)] {
                    let ctx = crate::fonts::test_context();
                    let mut app = crate::test_support::sample_app();
                    app.ui.layout = mode;
                    app.preferences.timeline_left = true;
                    app.shell.history_drag_width = drag_width;
                    let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 600.0));
                    for _ in 0..3 {
                        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| left_panel(&mut app, ui));
                        out.textures_delta.clear();
                    }
                    if let Some(card) = app.shell.history {
                        assert!(screen.contains_rect(card), "mode={mode:?} viewport={screen:?} card={card:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn history_seam_widths_stay_inside_small_viewports() {
        let mut app = crate::test_support::sample_app();
        for names in [false, true] {
            app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
            for drag in [None, Some(112.0), Some(f32::INFINITY)] {
                app.shell.history_drag_width = drag;
                for width in [0.0, 20.0, 56.0, 100.0, 200.0, 800.0] {
                    let available = Rect::from_min_size(pos2(15.0, 30.0), vec2(width, 300.0));
                    assert!(available.contains_rect(floating_rect(&app, available)));
                }
            }
        }
    }

    #[test]
    fn both_widths_keep_selection_reorder_marker_transport_and_feature_menus() {
        for names in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
            for (name, x) in [("First", 0), ("Second", 20)] {
                h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"corner":[x,0,0],"operation":"new_body","name":name})).unwrap();
            }
            h.settle();
            h.step(&json!({"click":{"handle":"timeline:First"}})).unwrap();
            h.step(&json!({"click":{"handle":"timeline:Second"},"ctrl":true})).unwrap();
            assert_eq!(selected_feature_ids(&h.app).len(), 2);
            h.step(&json!({"click":{"handle":"timeline:Second"},"button":"right"})).unwrap();
            h.step(&json!({"widget":"Copy"})).unwrap();
            assert_eq!(h.app.session.clipboard.len(), 2);
            h.app.run("select.clear", json!({})).unwrap();
            h.settle();
            let first = handle_at("timeline:First").unwrap();
            h.step(&json!({"drag":[{"handle":"timeline:Second"},[first.x,first.y-15.0]]})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.features.first().unwrap().name, "Second");
            let first = handle_at("timeline:Second").unwrap();
            h.step(&json!({"drag":[{"handle":"timeline:rollback"},[first.x,first.y-15.0]]})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(0));
            h.step(&json!({"click":{"handle":"history:forward"}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, Some(1));
            h.step(&json!({"click":{"handle":"history:last"}})).unwrap();
            h.settle();
            assert_eq!(h.app.session.doc.marker, None);
            h.frames(40);
            h.step(&json!({"click":{"handle":"timeline:First"},"double":true})).unwrap();
            h.frames(3);
            assert!(h.app.dialog.is_some(), "double-click edits the feature, names={names}");
        }
    }

    #[test]
    fn history_is_left_of_the_browser_in_both_layouts_widths_and_browser_states() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for names in [false, true] {
                for collapsed in [false, true] {
                    let mut h = Harness::new();
                    h.app.ui.layout = mode;
                    h.app.preferences.timeline_left = true;
                    h.app.preferences.timeline_left_width = crate::ui_kit::history_width(names);
                    h.app.preferences.timeline_left_browser_collapsed = collapsed;
                    h.app.run("solid.box", json!({"length":20,"width":15,"height":10,"name":"Base"})).unwrap();
                    h.app.run("solid.fillet", json!({"edges":[[20,7.5,10]],"radius":1,"name":"Round"})).unwrap();
                    h.app.run("timeline.group", json!({"name":"Housing","features":["Base","Round"]})).unwrap();
                    h.settle();
                    let history = crate::scenario::rect_of("history:vertical").unwrap();
                    let browser = crate::scenario::rect_of("browser:card").unwrap();
                    assert!(
                        (browser.left() - history.right() - Container::for_layout(mode, Surface::Panel).gap()).abs() < 1.0,
                        "{mode:?}, names={names}, collapsed={collapsed}: {history:?}, {browser:?}"
                    );
                    assert_eq!(history.width(), crate::ui_kit::history_width(names));
                    assert_eq!(handle_at("row:Untitled").is_none(), collapsed);
                    let folder = handle_at("timeline-folder:Housing").unwrap();
                    let base = handle_at("timeline:Base").unwrap();
                    let round = handle_at("timeline:Round").unwrap();
                    assert!(folder.y < base.y && base.y < round.y);
                    let row = crate::scenario::rect_of("history:feature:Base").unwrap();
                    assert_eq!(row.height(), if names { crate::ui_kit::TREE_ROW_HEIGHT } else { 38.0 });
                    if names && !collapsed {
                        let browser_row = crate::browser::drawn_rows().into_iter().find(|r| r.0 == "Untitled").unwrap().1;
                        assert_eq!(row.height(), browser_row.height());
                    }
                }
            }
        }
    }

    #[test]
    fn history_width_toggle_and_browser_rail_persist_independently() {
        let mut h = Harness::new();
        h.app.preferences.timeline_left = true;
        h.settle();
        h.step(&json!({"click":{"handle":"history:names"}})).unwrap();
        assert!(h.app.preferences.timeline_left_width < crate::ui_kit::history_width(true));
        h.step(&json!({"click":{"handle":"left-browser-toggle"}})).unwrap();
        assert!(h.app.preferences.timeline_left_browser_collapsed);
        assert!(handle_at("row:Untitled").is_none());
        let mut restored = Harness::new();
        restored.app.load_prefs(&h.app.prefs());
        assert!(restored.app.preferences.timeline_left_width < crate::ui_kit::history_width(true));
        assert!(restored.app.preferences.timeline_left_browser_collapsed);
        h.settle();
        h.step(&json!({"click":{"handle":"left-browser-toggle"}})).unwrap();
        assert!(!h.app.preferences.timeline_left_browser_collapsed);
        assert!(handle_at("row:Untitled").is_some());
        h.step(&json!({"click":{"handle":"history:names"}})).unwrap();
        assert!(h.app.preferences.timeline_left_width >= crate::ui_kit::history_width(true));
        h.step(&json!({"key":"Enter"})).unwrap();
        assert!(h.app.preferences.timeline_left_width < crate::ui_kit::history_width(true), "focused seam handles Enter in the app");
        h.step(&json!({"key":"Space"})).unwrap();
        assert!(h.app.preferences.timeline_left_width >= crate::ui_kit::history_width(true), "focused seam handles Space in the app");
    }

    #[test]
    fn browser_sidebar_drags_wider_and_narrower() {
        let mut harness = Harness::new();
        harness.frames(4);
        let before = harness.app.preferences.browser_width;
        let rail = handle_at("left-browser-rail").expect("the browser sidebar's right edge is drawn");
        harness.step(&json!({"drag": [[rail.x, rail.y], [rail.x + 90.0, rail.y]]})).unwrap();
        let wider = harness.app.preferences.browser_width;
        assert!(wider > before + 40.0, "dragging the right edge right should widen the browser: {before} -> {wider}");

        let rail = handle_at("left-browser-rail").expect("the browser sidebar's right edge is drawn");
        harness.step(&json!({"drag": [[rail.x, rail.y], [rail.x - 40.0, rail.y]]})).unwrap();
        let narrower = harness.app.preferences.browser_width;
        assert!(narrower < wider - 20.0, "dragging it left should narrow it again: {wider} -> {narrower}");
    }

    #[test]
    fn floating_browser_sits_over_the_workspace_and_docks_again() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut harness = Harness::new();
            harness.app.ui.layout = mode;
            harness.app.preferences.browser_float = true;
            harness.frames(4);
            let card = crate::scenario::rect_of("browser:float").expect("the floating browser is drawn");
            assert!(card.min.x < 40.0, "the card sits at the view's left edge: {card:?}");
            assert_eq!(harness.app.viewport.rect.unwrap().left(), 0.0, "a floating browser must not take a column");
            assert!(handle_at("left-browser-rail").is_some(), "the card retains the saved-width drag grip");
            assert!(handle_at("row:Untitled").is_some(), "the tree is still drawn and usable");

            harness.app.preferences.browser_float = false;
            harness.frames(4);
            assert!(crate::scenario::rect_of("browser:float").is_none(), "the explicit overlay is off");
            assert!(crate::scenario::rect_of("browser:card").is_some());
            assert_eq!(harness.app.viewport.rect.unwrap().left(), 0.0);
            assert!(handle_at("left-browser-rail").is_some(), "both card and sidebar resize");
        }
    }

    #[test]
    fn rollback_bar_above_last_feature_can_be_dragged_to_the_end() {
        let mut harness = Harness::new();
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
            solidcraft_engine::doc::TimelineGroup { id: 1, name: "Outer".into(), parent: None, features: vec![] },
            solidcraft_engine::doc::TimelineGroup { id: 2, name: "Middle".into(), parent: Some(1), features: vec![] },
            solidcraft_engine::doc::TimelineGroup { id: 3, name: "Inner".into(), parent: Some(2), features: vec![] },
            solidcraft_engine::doc::TimelineGroup { id: 7, name: "Target".into(), parent: None, features: vec![] },
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Fixed"})).unwrap();
        harness.frames(4);
        let before = handle_at("timeline:Fixed").unwrap();

        harness.step(&json!({"click": {"handle": "timeline:Fixed"}})).unwrap();
        let after = handle_at("timeline:Fixed").unwrap();
        assert!((after.y - before.y).abs() < 0.5, "selection count shifted the row from {} to {}", before.y, after.y);
    }

    #[test]
    fn new_folder_starts_inline_rename_with_its_name_selected() {
        let mut harness = Harness::new();
        harness.app.preferences.timeline_left = true;
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

    #[test]
    fn nested_folder_delete_keeps_parent_and_features() {
        let mut harness = Harness::new();
        harness.app.preferences.timeline_left = true;
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

    #[test]
    fn collapsed_folder_stays_visible_while_its_contents_hide() {
        let mut harness = Harness::new();
        harness.app.preferences.timeline_left = true;
        harness.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Inside"})).unwrap();
        harness.app.run("timeline.group", json!({"name": "Folder", "features": ["Inside"]})).unwrap();
        harness.frames(4);
        assert!(handle_at("timeline:Inside").is_some());

        harness.step(&json!({"click": {"handle": "timeline-folder-fold:Folder"}})).unwrap();
        assert!(handle_at("timeline-folder:Folder").is_some(), "collapsed folder row disappeared");
        assert!(handle_at("timeline:Inside").is_none(), "collapsed folder still showed its contents");
    }

    #[test]
    fn drop_below_a_collapsed_folder_appends_to_that_folder() {
        let mut harness = Harness::new();
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
}
