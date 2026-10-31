//! The left feature-history panel: named rows, folders, rollback and drag-and-drop.

use crate::ui_kit::{Container, Edge, Surface};
use egui::{Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::history_rows::{HistoryFeature, HistoryRows};
use crate::theme::Tokens;
use crate::timeline::{
    TimelineKind, clipboard_buttons, delete_after_marker_button, delete_button, delete_features, message_button, selected_feature_ids,
};
use crate::{SolveApp, icons};

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

/// Shared bounds keep browser and history at exactly the same width. Their split is the
/// remembered share of the available height (half until the seam is dragged, #7371). One folded
/// header gives the other the remainder; two folded headers sit together at the top.
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
    let expanded_browser = egui::lerp(browser_split(app.preferences.browser_share, budget, header)..=(budget - header).max(0.0), history_fold);
    let browser_height = egui::lerp(expanded_browser..=header, browser_fold);
    let browser = Rect::from_min_size(column.min, vec2(column.width(), browser_height));
    let history_top = (browser.bottom() + gap).min(column.bottom());
    let history_height = egui::lerp((column.bottom() - history_top).max(0.0)..=header, history_fold);
    let history = Rect::from_min_size(pos2(column.left(), history_top), vec2(column.width(), history_height));
    (browser, history)
}

/// The expanded browser's height in a column of `budget` (both cards, without the gap): the
/// remembered share, kept to at least a header and two rows for either card.
pub(crate) fn browser_split(share: f32, budget: f32, header: f32) -> f32 {
    let share = if share.is_finite() { share } else { crate::prefs::DEFAULT_BROWSER_SHARE };
    let least = (header + 2.0 * crate::ui_kit::TREE_ROW_HEIGHT).min(budget / 2.0).max(0.0);
    (budget * share).clamp(least, (budget - least).max(least))
}

/// How tall a grip the seam between the stacked browser and History offers, around the gap.
const SPLIT_RAIL: f32 = 6.0;

/// Dragging the seam between the expanded browser and History moves their split (#7371), as the
/// column's right edge moves its width.
fn split_handle(app: &mut SolveApp, ui: &egui::Ui, available: Rect) {
    if !app.ui.show_browser
        || app.preferences.timeline_left_browser_collapsed
        || app.preferences.timeline_collapsed
        || app.shell.history_folding.is_some_and(|p| p != [0.0, 0.0])
    {
        return;
    }
    let (browser, history) = stacked_rects(app, available);
    let budget = history.bottom() - browser.top() - (history.top() - browser.bottom());
    if !browser.is_positive() || !history.is_positive() || budget <= 0.0 {
        return;
    }
    let rail = Rect::from_x_y_ranges(browser.x_range(), browser.bottom() - SPLIT_RAIL / 2.0..=history.top() + SPLIT_RAIL / 2.0);
    let overlay = Container::new(ui.ctx(), Surface::Panel).area(egui::Id::new("sc_column_split")).fixed_pos(rail.min);
    ui.ctx().set_sublayer(ui.layer_id(), overlay.layer());
    overlay.show(ui.ctx(), |ui| {
        ui.set_min_size(rail.size());
        let response = ui.interact(rail, egui::Id::new("sc_column_split_rail"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta().y);
            if delta.is_finite() {
                let header = header_outer_height().min(budget / 2.0);
                app.preferences.browser_share = browser_split((browser.height() + delta) / budget, budget, header) / budget;
            }
        }
    });
    crate::scenario::publish_handle("left-column-split", rail.center());
}

pub(crate) fn floating_rect(app: &SolveApp, available: Rect) -> Rect {
    if stacked(app) {
        return stacked_rects(app, available).1;
    }
    let gap = Container::for_layout(app.ui.layout, Surface::Panel).gap();
    let min = pos2((available.left() + crate::agent_chat::left_inset(app, available) + gap).min(available.right()), available.top());
    Rect::from_min_max(min, pos2((min.x + strip_width(app)).min((available.right() - gap).max(min.x)), (available.bottom() - gap).max(min.y)))
}

/// Under the browser, History is one column with it and follows its translucency.
fn card_fill(app: &SolveApp, floating: bool) -> egui::Color32 {
    let panel = Tokens::get().panel;
    if floating && stacked(app) && app.ui.show_browser && app.preferences.browser_float {
        panel.gamma_multiply(crate::browser::BROWSER_FLOAT_ALPHA)
    } else {
        panel
    }
}

/// Full-height left card, or a card sharing the browser column below it.
pub fn left_panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let available = ui.available_rect_before_wrap();
    app.shell.history_dock_space = Some(available);
    // A dialog in the left column holds the History's place (`dialog_column`).
    let held = crate::dialog_column::holds(app);
    if held {
        // No row is under the pointer: the view lights no feature for the History.
        app.viewport.hover_feature = None;
    }
    if Container::for_layout(app.ui.layout, Surface::Panel).floating() || stacked(app) {
        let bounds = floating_rect(app, available);
        app.shell.history = None;
        if held || !bounds.is_finite() || bounds.width() < crate::ui_kit::history_width(false) || bounds.height() < t.control_height {
            return;
        }
        let container = Container::new(ui.ctx(), Surface::Panel).joined(crate::ui_kit::DockEdges { left: true, top: true, ..Default::default() });
        let fill = card_fill(app, container.floating());
        let frame = container.frame(ui.ctx(), fill, crate::frame::Side::Left).inner_margin(t.space_4);
        let inner = (bounds.size() - frame.total_margin().sum()).max(egui::Vec2::ZERO);
        let card = Container::new(ui.ctx(), Surface::Panel).area(egui::Id::new("sc_timeline_left_card")).fixed_pos(bounds.min).show(ui.ctx(), |ui| {
            let response = container.show_frame(ui, frame, available, |ui| {
                ui.set_min_size(inner);
                ui.set_max_size(inner);
                ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, inner)));
                left_timeline(app, ui);
            });
            if stacked(app) {
                let width = crate::browser::browser_width(app.preferences.browser_width);
                crate::browser::resize_column(app, ui, response.response.rect, width, "left-history-rail");
                split_handle(app, ui, available);
            }
        });
        app.shell.history = Some(card.response.rect);
        crate::scenario::publish_rect("history:vertical", card.response.rect);
        if !stacked(app) {
            seam_handle(app, ui, card.response.rect, card.response.layer_id);
        }
    } else {
        let width = strip_width(app).min(ui.available_width().max(0.0));
        if width < crate::ui_kit::history_width(false) {
            return;
        }
        // Held by a dialog, the docked panel stays allocated but empty, so the view keeps its size.
        let response = Container::new(ui.ctx(), Surface::Panel)
            .edge("sc_timeline_left", Edge::Left)
            .exact_size(width)
            .resizable(false)
            .frame(Container::new(ui.ctx(), Surface::Panel).edge_frame(t.panel).inner_margin(t.space_4))
            .show(ui, |ui| {
                if !held {
                    left_timeline(app, ui);
                }
            });
        app.shell.history = Some(response.response.rect);
        if !held {
            crate::scenario::publish_rect("history:vertical", response.response.rect);
            seam_handle(app, ui, response.response.rect, response.response.layer_id);
        }
    }
}

fn names_fit(width: f32) -> bool {
    let t = Tokens::get();
    width >= crate::ui_kit::history_width(true) - (t.space_8 + t.space_2)
}

fn seam_handle(app: &mut SolveApp, ui: &mut egui::Ui, card: Rect, parent: egui::LayerId) {
    let seam = crate::ui_kit::HistorySeam::new(card);
    let bounds = seam.edge.union(seam.pill);
    let overlay = Container::new(ui.ctx(), Surface::Panel).area(egui::Id::new("history_seam_overlay")).fixed_pos(bounds.min);
    // The grip follows its panel when clicked, while remaining below movable dialogs.
    ui.ctx().set_sublayer(parent, overlay.layer());
    overlay.show(ui.ctx(), |ui| {
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
    });
}

/// Width alone chooses icons, names, horizontal transport and the optional title.
pub fn left_timeline(app: &mut SolveApp, ui: &mut egui::Ui) {
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
    let mut controls = crate::ui_kit::HistoryTransport::shown(app.preferences.history_play_button);
    let count = controls.len() as f32;
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
        // A named column too narrow for five buttons beside its grip leaves Play to the ribbon.
        if controls_right - crate::ui_kit::history_controls_width(controls.len()) < handle.right() + t.space_2 {
            controls = &crate::ui_kit::HistoryTransport::ALL;
        }
        let controls_width = crate::ui_kit::history_controls_width(controls.len());
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
    for (i, &control) in controls.iter().enumerate() {
        let side = if names { t.space_24 } else { t.control_height };
        let rect = if names {
            Rect::from_min_size(
                pos2(
                    controls_right - crate::ui_kit::history_controls_width(controls.len()) + i as f32 * (side + t.space_2),
                    header.center().y - side / 2.0,
                ),
                vec2(side, side),
            )
        } else {
            Rect::from_min_size(pos2(header.center().x - side / 2.0, header.top() + i as f32 * (side + t.space_2)), vec2(side, side))
        };
        let response = control.show(ui, rect, app.timeline_playback.is_some());
        crate::timeline::transport_response(app, control, &response);
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
        app.tree.collapsed.remove(&format!("timeline_group:{group}"));
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
        if crate::context_menu::button(ui, "Add to New Folder", "feature_group").clicked() {
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
        if grouped && crate::context_menu::button(ui, "Remove from Folder", "feature_group").clicked() {
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
            .on_hover_text(crate::i18n::tr("timeline.delete_folders_tip"))
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
        if crate::context_menu::button_enabled(ui, label, "suppress", has_contents)
            .on_hover_text(crate::i18n::tr("timeline.suppress_folders_tip"))
            .clicked()
        {
            let _ = app.run_heavy("timeline.suppress", json!({"groups": selected_groups, "suppressed": !all_suppressed}));
            ui.close();
        }
        if crate::context_menu::button(ui, "Delete Folders and Features…", "delete")
            .on_hover_text(crate::i18n::tr("timeline.delete_folders_with_features_tip"))
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

/// Open the folder holding this feature and every folder above it, so its row is shown. A sketch
/// listed under the feature built from it also opens that feature's fold (#6489).
pub fn reveal_feature(app: &mut SolveApp, feature: u64) {
    if let Some(host) = drawn_rows(app, nests(app)).host(feature) {
        app.tree.expanded.insert(sketches_key(host));
    }
    let groups = &app.session.doc.timeline_groups;
    let mut current = group_for(groups, feature);
    let mut steps = 0usize;
    while let Some(id) = current {
        app.tree.collapsed.remove(&format!("timeline_group:{id}"));
        steps += 1;
        if steps > groups.len() {
            break;
        }
        current = groups.iter().find(|group| group.id == id).and_then(|group| group.parent);
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

/// The reference the cached [`RowPlan`] is tested against.
#[cfg(test)]
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
            egui::FontId::proportional(crate::theme::Tokens::get().font_body),
            if faint { t.text.gamma_multiply(0.42) } else { t.text },
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
    // A folder whose features are all suppressed looks like a suppressed feature.
    let suppressed = (!contents.is_empty() || !joints.is_empty())
        && contents.iter().all(|id| table.suppressed(*id))
        && joints.iter().all(|id| app.session.doc.assembly.joints.iter().any(|joint| joint.id == *id && joint.suppressed));
    let mut painter = ui.painter().clone();
    if suppressed {
        painter.multiply_opacity(0.42);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 27.5 + offset, rect.center().y), vec2(17.0, 17.0));
    icons::paint_icon(&painter, icon, "feature_group", f32::from(t.icon.a()) / 255.0);
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
        Some(tip) => response.on_hover_text(tip),
        None => response,
    };
    let renaming = timeline_group_name(app, ui, group, rect, offset, names, suppressed, &t);
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
    /// With a component active, the features that affect it; the others are dimmed (#5854).
    affecting: Option<std::sync::Arc<std::collections::BTreeSet<u64>>>,
}

fn vertical_features(app: &mut SolveApp, ui: &mut egui::Ui, names: bool) {
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
    let drag_id = ui.id().with("tl_vertical_drag");
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
        reveal: app.tree.timeline_reveal.take(),
        from_view,
        linked_hover: app.viewport.hover_feature.filter(|id| plan.referenced(*id)),
        drag: ui.data(|data| data.get_temp(drag_id)),
        hovered: None,
        edit: None,
        affecting: crate::timeline::affecting(app),
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
                if matches!(active, Drag::Marker) { crate::theme::TabAccent::current(ui.ctx()).color } else { t.sketch_accent },
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
    app.viewport.hover_feature = rows.hovered;
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
            if rows.reveal == Some(id) {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
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
    let warning = result.and_then(|result| result.warning.clone());
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
        kind: ty,
        selected,
        hovered: response.hovered() || rows.from_view.contains(&Some(id)) || rows.linked_hover == Some(id),
        // A selected feature stays full strength, so its highlight shows.
        dim: rolled || (!selected && rows.affecting.as_ref().is_some_and(|a| !a.contains(&id))),
        suppressed: *suppressed,
        error: error.is_some(),
        warning: warning.is_some(),
        reference,
        ink: sketch_state.and_then(|state| state.ink(&t)),
        accent: crate::theme::TabAccent::for_tab(&t, if dragged { "SKETCH" } else { &app.ui.tab }),
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
        // Greyed out says only "not shown": say why (#8100).
        if rolled {
            tip.push_str(&format!("\n{}", crate::i18n::tr("timeline.feature_rolled_back")));
        }
        if let Some(state) = sketch_state {
            tip.push_str(&format!("\n{}", state.text()));
        }
        crate::ui_kit::text(ui, &tip, crate::ui_kit::Text::Value("history", false));
        // A value row is one line: where the sketch is listed gets a row of its own.
        if let Some(host) = listed_under.filter(|_| reference) {
            let listed = crate::i18n::format("ui.history_sketch_listed_under", &[("feature", host)]);
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
        if crate::context_menu::button(ui, "Edit Feature", "edit").clicked() {
            edit = true;
            ui.close();
        }
        crate::timeline::sketch_plane_button(app, ui, id);
        if let Some(sketch) = sketch
            && crate::context_menu::button(ui, "Edit Profile Sketch", "sketch").clicked()
        {
            app.edit_sketch(sketch.0);
            ui.close();
        }
        if crate::context_menu::button(ui, "Rename Feature", "rename").clicked() {
            crate::context_menu::start_rename(app, crate::context_menu::RenameWhat::Feature(id), rect.center());
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
        h.app.preferences.timeline_left = true;
        h.app.preferences.language = "ja".into();
        h.app.run("timeline.group", json!({"name": "My folder"})).unwrap();
        h.frames(4);
        h.step(&json!({"click": {"handle": "timeline-folder:My folder"}, "button": "right"})).unwrap();
        let label = crate::i18n::Locale::for_language("ja").label("Delete Selected Folders");
        let [x, y] = h.widget(label).unwrap();
        h.call("ui.move", json!({"x": x, "y": y}));
        h.frames(90);
        let output = egui::FullOutput { shapes: h.shapes.clone(), ..Default::default() };
        let texts: Vec<_> = crate::test_support::text_shapes(&output).map(|t| t.galley.text()).collect();
        assert!(texts.contains(&"フィーチャーと入れ子のフォルダーを履歴に残します"), "{texts:?}");
        assert_eq!(h.app.session.doc.timeline_groups.len(), 1, "hovering does not delete the folder");
    }

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
                    assert!(h.app.timeline_playback.is_none(), "navigation never starts playback");
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
    fn the_seam_between_browser_and_history_drags_their_split_and_is_remembered() {
        // #7371: under the browser, History took exactly half the column; the seam now moves it.
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.layout = mode;
            h.call("ui.resize", json!({"width":1400,"height":900}));
            h.app.run("view.history_layout", json!({"dock":"under_browser"})).unwrap();
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            h.settle();
            let (browser, history) = (h.app.shell.browser.unwrap(), h.app.shell.history.unwrap());
            assert!((browser.height() - history.height()).abs() < 1.0, "{mode:?}: an equal split by default: {browser:?} {history:?}");
            let seam = handle_at("left-column-split").expect("the seam between the cards is a grip");
            h.step(&json!({"drag": [[seam.x, seam.y], [seam.x, seam.y + 120.0]]})).unwrap();
            h.settle();
            let (taller, shorter) = (h.app.shell.browser.unwrap(), h.app.shell.history.unwrap());
            assert!((taller.height() - browser.height() - 120.0).abs() < 2.0, "{mode:?}: {browser:?} -> {taller:?}");
            assert!((history.height() - shorter.height() - 120.0).abs() < 2.0, "{mode:?}: {history:?} -> {shorter:?}");
            assert!((shorter.bottom() - history.bottom()).abs() < 1.0 && (taller.top() - browser.top()).abs() < 1.0);
            let share = h.app.preferences.browser_share;
            assert!(share > 0.6 && share < 0.7, "{mode:?}: {share}");
            let mut restored = SolveApp::new(Default::default(), Default::default());
            restored.load_prefs(&h.app.prefs());
            assert_eq!(restored.preferences.browser_share, share);
            // Either card keeps its header and two rows however far the seam goes.
            let least = header_outer_height() + 2.0 * crate::ui_kit::TREE_ROW_HEIGHT - 1.0;
            for dy in [2000.0, -2000.0] {
                let seam = handle_at("left-column-split").unwrap();
                h.step(&json!({"drag": [[seam.x, seam.y], [seam.x, seam.y + dy]]})).unwrap();
                h.settle();
                let (browser, history) = (h.app.shell.browser.unwrap(), h.app.shell.history.unwrap());
                assert!(browser.height() >= least && history.height() >= least, "{mode:?}, {dy}: {browser:?} {history:?}");
            }
            // Folding History still gives the browser the rest, and unfolding brings the split back.
            let split = h.app.shell.browser.unwrap().height();
            h.app.preferences.timeline_collapsed = true;
            h.frames(12);
            assert!(h.app.shell.history.unwrap().height() <= header_outer_height() + 1.0);
            assert!(handle_at("left-column-split").is_none(), "no grip while a card is folded");
            h.app.preferences.timeline_collapsed = false;
            h.frames(12);
            assert!((h.app.shell.browser.unwrap().height() - split).abs() < 1.0);
        }
        // A hostile preference falls back to the equal split; a tiny column still fits both headers.
        assert_eq!(browser_split(f32::NAN, 600.0, 40.0), 300.0);
        assert_eq!(browser_split(f32::INFINITY, 600.0, 40.0), 300.0);
        assert_eq!(browser_split(-3.0, 600.0, 40.0), 40.0 + 2.0 * crate::ui_kit::TREE_ROW_HEIGHT);
        assert_eq!(browser_split(0.9, 60.0, 30.0), 30.0);
    }

    #[test]
    fn history_follows_the_browsers_translucency_only_under_the_browser() {
        let opaque = Tokens::get().panel;
        let translucent = opaque.gamma_multiply(crate::browser::BROWSER_FLOAT_ALPHA);
        for (floating, under_browser, browser_float, expected) in [
            (true, true, true, translucent),
            (true, true, false, opaque),
            (true, false, true, opaque),
            (false, true, true, opaque),
            (false, false, true, opaque),
        ] {
            let mut h = Harness::new();
            h.app.ui.layout = if floating { crate::shell::LayoutMode::Floating } else { crate::shell::LayoutMode::Docked };
            h.app.ui.show_timeline = true;
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_under_browser = under_browser;
            h.app.preferences.browser_float = browser_float;
            let container = Container::for_layout(h.app.ui.layout, Surface::Panel);
            assert_eq!(card_fill(&h.app, container.floating()), expected, "floating={floating} under_browser={under_browser} float={browser_float}");
        }
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
    fn find_in_timeline_opens_the_collapsed_folders_around_the_feature() {
        let mut harness = Harness::new();
        harness.app.preferences.timeline_left = true;
        harness.app.run("solid.box", json!({"length": 20, "width": 15, "height": 10, "name": "Base", "body_name": "Plate"})).unwrap();
        let inner = harness.app.run("timeline.group", json!({"name": "Inner", "features": ["Base"]})).unwrap()["group"].as_u64().unwrap();
        let outer = harness.app.run("timeline.group", json!({"name": "Outer", "groups": [inner]})).unwrap()["group"].as_u64().unwrap();
        harness.app.tree.collapsed.insert(format!("timeline_group:{inner}"));
        harness.app.tree.collapsed.insert(format!("timeline_group:{outer}"));
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
        h.app.preferences.timeline_left = true;
        h.app.preferences.timeline_left_width = 240.0;
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

        // Under the browser too; not in the icons-only width, nor in the bottom strip.
        h.app.preferences.timeline_under_browser = true;
        h.frames(4);
        assert!(in_history("row:Origin").is_some());
        h.app.preferences.timeline_under_browser = false;
        h.app.preferences.timeline_left_width = crate::ui_kit::history_width(false);
        h.frames(4);
        assert!(in_history("row:Origin").is_none(), "no room for the rows without names");
        h.app.preferences.timeline_left = false;
        h.frames(4);
        assert_eq!(crate::scenario::handles("row:Origin").into_iter().filter(|(n, _)| n == "row:Origin").count(), 1, "only the browser's");
    }

    /// In a design with components, the Bodies folder is the active component's, in the
    /// placement being edited, as the browser lists it under that placement.
    #[test]
    fn the_history_lists_the_active_components_bodies() {
        let mut h = Harness::new();
        h.app.preferences.timeline_left = true;
        h.app.preferences.timeline_left_width = 240.0;
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
        h.app.preferences.timeline_left = true;
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
        h.app.tree.collapsed.insert(format!("timeline_group:{first}"));
        h.frames(4);
        assert!(handle_at("timeline-joint:A").is_none() && handle_at("timeline-joint:B").is_none());
        assert!(handle_at("timeline-joint:C").is_some());
    }

    #[test]
    fn joint_folder_menu_suppresses_and_deletes_joints_in_one_step() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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
        harness.app.preferences.timeline_left = true;
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

        harness.app.tree.collapsed.insert(format!("timeline_group:{}", id("Outer")));
        harness.frames(2);
        assert_eq!(marks(), ["timeline-folder-status:warning:Outer"]);

        mark(&mut harness, "Broken", Some("the blend is larger than the neighbouring faces"), None);
        harness.frames(2);
        assert_eq!(marks(), ["timeline-folder-status:error:Outer"], "an error in a nested folder outweighs the warning");
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
        h.app.preferences.timeline_left = true;
        h.app.preferences.timeline_left_width = 240.0;
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

        // The narrow icon strip keeps the creation order.
        h.app.preferences.timeline_left_width = 56.0;
        h.settle();
        assert!(row_y("timeline:Profile") < row_y("timeline:Block") && row_y("timeline:Pocket") < row_y("timeline:Second"));
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
        let kept = (h.app.tree.collapsed.clone(), h.app.tree.expanded.clone());
        for closed in 0..(1u32 << keys.len()) {
            for open_hosts in [false, true] {
                let app = &mut h.app;
                app.tree.collapsed = keys.iter().enumerate().filter(|(bit, _)| closed & (1 << bit) != 0).map(|(_, key)| key.clone()).collect();
                app.tree.expanded = if open_hosts { hosts.iter().cloned().collect() } else { Default::default() };
                let folds = table.folds(app);
                assert_eq!(table.visible_order(&folds), visible_order(app, features, groups, &table.rows), "closed {closed:b}, hosts {open_hosts}");
                for group in groups {
                    assert_eq!(table.is_open(&folds, group.id), group_is_open(app, groups, group.id), "{}", group.name);
                    assert_eq!(table.ancestors_open(&folds, group.id), group_ancestors_are_open(app, groups, group.id), "{}", group.name);
                }
            }
        }
        (h.app.tree.collapsed, h.app.tree.expanded) = kept;
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
        h.app.preferences.timeline_left = true;
        h.app.preferences.timeline_left_width = 240.0;
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
        h.app.preferences.timeline_left = true;
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
            let mut out = ctx.run_ui(raw, |ui| left_timeline(app, ui));
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
