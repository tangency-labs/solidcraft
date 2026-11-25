//! Dense editable tables. Geometry and typography share the form label-column tokens.
use super::metrics::*;
use crate::BackingTooltipExt as _;
use crate::{Container, Surface};
use egui::{Response, Ui, vec2};
use solidcraft_theme::Tokens;

// Match the upstream capacity of persisted column widths, including wide configuration tables.
const MAX_COLUMNS: usize = 1024;

/// Plain cell labels yield clicks to the opt-in selection layer. Explicit controls keep them.
pub(super) const CELL_SELECTION: &str = "sc_table_cell_selection";

/// Caller-owned cell coordinates. Row indices count data rows, excluding section bands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellSelection {
    pub anchor: Option<(usize, usize)>,
    pub focus: Option<(usize, usize)>,
}

impl CellSelection {
    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }

    pub fn is_empty(&self) -> bool {
        self.range().is_none()
    }

    /// The selected coordinate when the selection contains exactly one cell.
    pub fn single(&self) -> Option<(usize, usize)> {
        self.anchor.filter(|anchor| Some(*anchor) == self.focus)
    }

    pub fn range(&self) -> Option<(std::ops::RangeInclusive<usize>, std::ops::RangeInclusive<usize>)> {
        let ((ar, ac), (fr, fc)) = (self.anchor?, self.focus?);
        Some((ar.min(fr)..=ar.max(fr), ac.min(fc)..=ac.max(fc)))
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        self.range().is_some_and(|(rows, cols)| rows.contains(&row) && cols.contains(&col))
    }

    fn select(&mut self, cell: (usize, usize), extend: bool) {
        if !extend || self.is_empty() {
            self.anchor = Some(cell);
        }
        self.focus = Some(cell);
    }

    fn clamp(&mut self, rows: usize, columns: usize) {
        if rows == 0 || columns == 0 || self.is_empty() {
            self.clear();
            return;
        }
        for cell in [&mut self.anchor, &mut self.focus].into_iter().flatten() {
            cell.0 = cell.0.min(rows - 1);
            cell.1 = cell.1.min(columns - 1);
        }
    }
}

/// Cell selection is opt-in; existing tables retain their row interactions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    Cell,
    Range,
}

struct CellRecord {
    row: usize,
    column: usize,
    rect: egui::Rect,
    response: Response,
}

struct CellSelectionState<'a> {
    selection: &'a mut CellSelection,
    mode: SelectionMode,
}

struct CellPaint {
    painter: egui::Painter,
    fill: egui::layers::ShapeIdx,
    outline: egui::layers::ShapeIdx,
}

struct CellBackground {
    painter: egui::Painter,
    slot: egui::layers::ShapeIdx,
    shapes: Vec<egui::Shape>,
}

/// A column's minimum width is a multiple of the measured form label column.
pub struct Column<'a> {
    label: &'a str,
    width: f32,
    min_content_width: f32,
    icon_only: bool,
    flexible: bool,
    /// A width the user dragged the column to; it replaces the automatic width and stops sharing.
    manual: Option<f32>,
    /// Held at the left edge while the table scrolls sideways.
    frozen: bool,
}

impl<'a> Column<'a> {
    pub fn new(label: &'a str, width: f32) -> Self {
        Self { label, width, min_content_width: 0.0, icon_only: false, flexible: false, manual: None, frozen: false }
    }

    /// Hold the column at a width the user chose by dragging its header divider.
    pub fn manual_width(mut self, width: Option<f32>) -> Self {
        self.manual = width.filter(|w| w.is_finite() && *w > 0.0);
        self
    }

    /// Whether this column still takes a share of the remaining width: flexible and not dragged.
    fn shares(&self) -> bool {
        self.flexible && self.manual.is_none()
    }

    /// Share the remaining visible table width using this column's width as its weight.
    /// Explicit minimums keep editors usable and allow horizontal scrolling on small cards.
    pub fn flex(mut self) -> Self {
        self.flexible = true;
        self
    }

    /// Retain the column identity for automation without a visible action heading.
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// Hold the column at the table's left edge while the other columns scroll sideways beneath
    /// it, as a spreadsheet freezes its first column. Only leading columns freeze: a frozen
    /// column after an unfrozen one scrolls as usual.
    pub fn frozen(mut self) -> Self {
        self.frozen = true;
        self
    }

    /// Keep enough room for the measured cell controls, even in a narrow scrolling card.
    pub fn min_content_width(mut self, width: f32) -> Self {
        self.min_content_width = dimension(width);
        self
    }
}

pub struct Table<'a> {
    columns: &'a [Column<'a>],
    widths: Vec<f32>,
    min_width: f32,
    id: String,
    rows: usize,
    empty: String,
    empty_shown: bool,
    reveal: Option<egui::Rect>,
    /// How far the table is scrolled sideways: frozen columns move right by this much.
    scroll_x: f32,
    /// How wide the table's visible part is.
    view_width: f32,
    selection: Option<SelectionPaint>,
    previous_row: Option<(egui::Rect, bool)>,
    groups: usize,
    group_has_rows: bool,
    cell_selection: Option<CellSelectionState<'a>>,
    cells: Vec<CellRecord>,
    cell_paint: Option<CellPaint>,
    cell_rules: Vec<(egui::Painter, egui::layers::ShapeIdx, egui::Rect, egui::Stroke)>,
    cell_background: CellBackground,
}

/// One reserved background slot for an entire contiguous selection, below all its cells.
/// Painting the union once avoids alpha overlap and antialiasing seams at row boundaries.
struct SelectionPaint {
    rect: egui::Rect,
    slot: egui::layers::ShapeIdx,
    bar_slot: egui::layers::ShapeIdx,
    painter: egui::Painter,
    hovered: bool,
}

/// Table grid with caller-supplied header actions through Table::headers.
/// Custom cells own their actions; all body rows share the same widths and metrics.
pub fn table_custom<'a, R>(ui: &mut Ui, id: &str, columns: &'a [Column<'a>], empty: &str, add: impl FnOnce(&mut Ui, &mut Table<'a>) -> R) -> R {
    let t = Tokens::get();
    let columns = columns.get(..columns.len().min(MAX_COLUMNS)).unwrap_or_default();
    let mut reveal = None;
    let output = egui::Frame::NONE
        .outer_margin(egui::Margin::same(t.space_12 as i8))
        .corner_radius(t.radius_lg)
        .fill(t.table_fill())
        .stroke(t.container_border(t.border))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal().id_salt(("table", id)).content_margin(egui::Margin::ZERO).auto_shrink([false, true]).show_viewport(
                ui,
                |ui, viewport| {
                    // ScrollArea snaps its content rect. Measure here, after that rounding and any
                    // enclosing list margins, and never allocate more than the actual viewport.
                    let ppp = ui.ctx().pixels_per_point().max(0.5);
                    let available = (dimension(ui.available_width()).min(viewport.width()) * ppp).floor() / ppp;
                    let mut table = Table {
                        columns,
                        widths: column_widths(ui, columns, available),
                        min_width: available,
                        id: id.to_owned(),
                        rows: 0,
                        empty: empty.to_owned(),
                        empty_shown: false,
                        reveal: None,
                        scroll_x: viewport.left().max(0.0),
                        view_width: viewport.width(),
                        selection: None,
                        previous_row: None,
                        groups: 0,
                        group_has_rows: false,
                        cell_selection: None,
                        cells: Vec::new(),
                        cell_paint: None,
                        cell_rules: Vec::new(),
                        cell_background: CellBackground {
                            painter: ui.painter().clone(),
                            slot: ui.painter().add(egui::Shape::Noop),
                            shapes: Vec::new(),
                        },
                    };
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let result = add(ui, &mut table);
                    table.finish_selection();
                    table.finish_cells(ui);
                    if table.is_empty() && !table.empty_shown {
                        super::text(ui, empty, super::Text::Note(id));
                    }
                    reveal = table.reveal;
                    result
                },
            )
        })
        .inner;
    solidcraft_ui_registry::publish_rect(&format!("dialog:table:{id}:viewport"), output.inner_rect);
    solidcraft_ui_registry::publish_count(&format!("dialog:table:{id}:content_width"), f64::from(output.content_size.x));
    // The nested horizontal ScrollArea consumes vertical scroll targets too. Request the
    // reveal here so it reaches the card's vertical scroller, only after an explicit action.
    if let Some(rect) = reveal {
        ui.scroll_to_rect(rect, None);
    }
    output.inner
}

/// The narrowest a user can drag a column.
const MANUAL_MIN_WIDTH: f32 = 24.0;

/// Each column's width in points for a table `available` points wide: dragged columns keep their
/// width, fixed columns measure their heading and controls, and flexible columns share the rest by
/// weight, each frozen at its minimum before the others divide what remains.
fn column_widths(ui: &Ui, columns: &[Column<'_>], available: f32) -> Vec<f32> {
    let t = Tokens::get();
    let mut widths: Vec<_> = columns
        .iter()
        .map(|c| {
            if let Some(manual) = c.manual {
                dimension(manual).max(MANUAL_MIN_WIDTH)
            } else if c.flexible || c.icon_only {
                c.min_content_width.max(t.control_height)
            } else {
                (header_column(ui, c.label) * dimension(c.width).min(64.0)).max(label_width(ui, c.label)).max(c.min_content_width)
            }
            .min(t.form_card_width * 64.0)
        })
        .collect();
    let fixed: f32 = columns.iter().zip(&widths).filter(|(c, _)| !c.shares()).map(|(_, w)| *w).sum();
    let mut remaining = (available - fixed - t.space_8 * (columns.len().saturating_sub(1) + 2) as f32 - t.space_4 * 2.0).max(0.0);
    let mut sharing: Vec<_> = columns.iter().map(|c| c.shares() && dimension(c.width) > 0.0).collect();
    // Freeze columns at their control minimum before sharing the remaining width. The bounded
    // passes also keep a hostile design with many columns from creating an unbounded loop.
    for _ in 0..columns.len().min(64) {
        let weight: f32 = columns.iter().zip(&sharing).filter(|(_, share)| **share).map(|(c, _)| dimension(c.width).min(64.0)).sum();
        if weight <= 0.0 {
            break;
        }
        let available = remaining;
        let mut frozen = false;
        for ((column, width), share) in columns.iter().zip(&mut widths).zip(&mut sharing) {
            if *share && *width > available * dimension(column.width).min(64.0) / weight {
                *share = false;
                remaining = (remaining - *width).max(0.0);
                frozen = true;
            }
        }
        if !frozen {
            for ((column, width), share) in columns.iter().zip(&mut widths).zip(&sharing) {
                if *share {
                    *width = remaining * dimension(column.width).min(64.0) / weight;
                }
            }
            break;
        }
    }
    // Round cumulative column edges, not each width independently: independent rounding
    // can add a pixel to an otherwise exactly fitting grid and create a scrollbar.
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let mut edge = 0.0;
    let mut rounded_edge = 0.0;
    for width in &mut widths {
        edge += *width;
        let next = (edge * ppp).round() / ppp;
        *width = next - rounded_edge;
        rounded_edge = next;
    }
    widths
}

impl<'a> Table<'a> {
    /// An embedded grid keeps its former header and divider targets as aliases of this
    /// table's one shared header. Aliases do not create additional widgets.
    pub(crate) fn header_aliases(&self, id: &str) {
        for (index, column) in self.columns.iter().enumerate() {
            if let Some(rect) = solidcraft_ui_registry::rect_of(&format!("dialog:column:{}:{}", self.id, column.label)) {
                solidcraft_ui_registry::publish_rect(&format!("dialog:column:{id}:{}", column.label), rect);
            }
            if let Some(rect) = solidcraft_ui_registry::rect_of(&format!("dialog:column_divider:{}:{index}", self.id)) {
                solidcraft_ui_registry::publish_target(&format!("dialog:column_divider:{id}:{index}"), rect);
            }
        }
    }

    /// Enable before adding rows. Background cell targets are registered before their content,
    /// so buttons, editors and dropdowns win clicks. Opt in to editable tables only where this
    /// interaction is appropriate; selecting a cell does not start its editor. Plain cell text
    /// yields its pointer input to selection; explicit controls (including copy cells) keep it.
    pub fn cell_selection(&mut self, selection: &'a mut CellSelection, mode: SelectionMode) {
        self.cell_selection = Some(CellSelectionState { selection, mode });
    }

    /// Resolve input after all content has registered its targets, then fill the two slots
    /// reserved before the first row's content. Cell clips never affect this painter.
    fn finish_cells(&mut self, ui: &mut Ui) {
        let Some(state) = &mut self.cell_selection else { return };
        self.cell_background.painter.set(self.cell_background.slot, egui::Shape::Vec(std::mem::take(&mut self.cell_background.shapes)));
        let selection = &mut state.selection;
        selection.clamp(self.rows, self.columns.len());
        if state.mode == SelectionMode::Cell
            && let Some(focus) = selection.focus
        {
            selection.select(focus, false);
        }
        let modifiers = ui.input(|i| i.modifiers);
        for cell in &self.cells {
            let response = ui.ctx().read_response(cell.response.id).unwrap_or_else(|| cell.response.clone());
            if response.clicked() || response.drag_started() {
                selection.select((cell.row, cell.column), state.mode == SelectionMode::Range && modifiers.shift);
                response.request_focus();
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, escape: true, ..Default::default() },
                    )
                });
            }
        }
        if state.mode == SelectionMode::Range
            && self.cells.iter().any(|c| c.response.dragged() || c.response.drag_stopped())
            && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            && let Some(cell) = self.cells.iter().rev().find(|c| c.response.interact_rect.contains(pointer))
        {
            selection.select((cell.row, cell.column), true);
        }
        let focused = self.cells.iter().any(|c| c.response.has_focus());
        let bounds = self.cells.iter().fold(egui::Rect::NOTHING, |rect, cell| rect.union(cell.response.interact_rect));
        let pointer_inside = ui.rect_contains_pointer(bounds);
        if ui.is_enabled() && (focused || (pointer_inside && !ui.ctx().egui_wants_keyboard_input())) {
            ui.input_mut(|input| {
                if input.consume_key(modifiers, egui::Key::Escape) {
                    selection.clear();
                } else if self.rows > 0 && !self.columns.is_empty() {
                    let (mut row, mut column) = selection.focus.unwrap_or((0, 0));
                    if state.mode == SelectionMode::Range && (modifiers.ctrl || modifiers.command) && input.consume_key(modifiers, egui::Key::A) {
                        selection.anchor = Some((0, 0));
                        selection.focus = Some((self.rows - 1, self.columns.len() - 1));
                    } else {
                        let moved = if input.consume_key(modifiers, egui::Key::ArrowLeft) {
                            column = column.saturating_sub(1);
                            true
                        } else if input.consume_key(modifiers, egui::Key::ArrowRight) {
                            column = column.saturating_add(1).min(self.columns.len() - 1);
                            true
                        } else if input.consume_key(modifiers, egui::Key::ArrowUp) {
                            row = row.saturating_sub(1);
                            true
                        } else if input.consume_key(modifiers, egui::Key::ArrowDown) {
                            row = row.saturating_add(1).min(self.rows - 1);
                            true
                        } else if input.consume_key(modifiers, egui::Key::Home) {
                            column = 0;
                            true
                        } else if input.consume_key(modifiers, egui::Key::End) {
                            column = self.columns.len() - 1;
                            true
                        } else {
                            false
                        };
                        if moved {
                            selection.select((row, column), state.mode == SelectionMode::Range && modifiers.shift);
                        }
                    }
                }
            });
        }
        let mut block = egui::Rect::NOTHING;
        for cell in &self.cells {
            let selected = selection.contains(cell.row, cell.column);
            cell.response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Cell, ui.is_enabled(), selected, ""));
            ui.ctx().accesskit_node_builder(cell.response.id, |node| {
                node.clear_toggled();
                node.set_selected(selected);
            });
            if selected {
                block = block.union(cell.rect);
            }
        }
        let count =
            selection.range().map_or(0, |(rows, columns)| (rows.end() - rows.start() + 1).saturating_mul(columns.end() - columns.start() + 1));
        solidcraft_ui_registry::publish_count(&format!("dialog:table:{}:selection:cells", self.id), count as f64);
        let t = Tokens::get();
        if block.is_positive() {
            // The visual cell box includes four points around the content. The stroke stays
            // inside that box, leaving the text in place and clear of the focus ring.
            block = block.expand(t.space_4);
            let ppp = ui.ctx().pixels_per_point().max(0.5);
            block.min = (block.min.to_vec2() * ppp).round().to_pos2() / ppp;
            block.max = (block.max.to_vec2() * ppp).round().to_pos2() / ppp;
            solidcraft_ui_registry::publish_rect(&format!("dialog:table:{}:selection", self.id), block);
            if let Some(paint) = &self.cell_paint {
                let fill = if selection.single().is_some() { t.input_fill() } else { t.form_primary.gamma_multiply(0.15) };
                paint.painter.set(paint.fill, egui::Shape::rect_filled(block, t.control_radius, fill));
                paint.painter.set(
                    paint.outline,
                    egui::Shape::rect_stroke(block, t.control_radius, egui::Stroke::new(1.5, t.form_primary), egui::StrokeKind::Inside),
                );
            }
        }
        // Split the ordinary row rules outside the selected columns. The single fill covers
        // every internal boundary; neither a hairline nor cell clipping can cut it in two.
        for (painter, slot, row, stroke) in &self.cell_rules {
            let y = row.top();
            let line = |left, right| egui::Shape::line_segment([egui::pos2(left, y), egui::pos2(right, y)], *stroke);
            let shapes = if block.is_positive() && y > block.top() && y < block.bottom() {
                let mut shapes = Vec::new();
                if row.left() < block.left() {
                    shapes.push(line(row.left(), block.left().min(row.right())));
                }
                if row.right() > block.right() {
                    shapes.push(line(block.right().max(row.left()), row.right()));
                }
                shapes
            } else {
                vec![line(row.left(), row.right())]
            };
            painter.set(*slot, egui::Shape::Vec(shapes));
        }
    }

    /// How many leading columns are frozen. None while they would take more than half of the
    /// visible width: the other columns must keep room to be seen as the table scrolls.
    fn frozen_count(&self) -> usize {
        let frozen = self.columns.iter().take_while(|c| c.frozen).count();
        let width = self.widths.iter().take(frozen).sum::<f32>() + Tokens::get().space_8 * frozen as f32;
        if width > self.view_width * 0.5 { 0 } else { frozen }
    }

    /// The cell of column `index` that starts at `left` in the scrolled content: a frozen one
    /// held at the visible left edge, the others where they are, clipped to the right of the
    /// frozen columns so they pass beneath them.
    fn place(&self, index: usize, cell: egui::Rect, first_left: f32) -> (egui::Rect, Option<f32>) {
        let frozen = self.frozen_count();
        if frozen == 0 || self.scroll_x <= 0.0 {
            return (cell, None);
        }
        if index < frozen {
            return (cell.translate(vec2(self.scroll_x, 0.0)), None);
        }
        let gap = Tokens::get().space_8;
        let frozen_right = first_left + self.widths.iter().take(frozen).sum::<f32>() + gap * frozen.saturating_sub(1) as f32 + self.scroll_x;
        (cell, Some(frozen_right + gap * 0.5))
    }

    /// Reveal a newly inserted row through the enclosing card's vertical scroller.
    pub fn reveal_row(&mut self, rect: egui::Rect) {
        self.reveal = Some(rect);
    }

    pub fn headers(&self, ui: &mut Ui, header: impl FnMut(&mut Ui, usize) -> bool) {
        self.headers_impl(ui, None, header);
    }

    /// Headers with a draggable divider after each column. A drag stores that column's width
    /// under `key` (egui persisted memory, shared by every table using the key so sections stay
    /// lined up); a double-click returns the column to its automatic width.
    pub fn headers_resizable(&self, ui: &mut Ui, key: egui::Id, header: impl FnMut(&mut Ui, usize) -> bool) {
        self.headers_impl(ui, Some(key), header);
    }

    /// The heading row shared by `headers` and `headers_resizable`: one clipped cell per column,
    /// `header` drawing a custom heading or the label, and a divider between columns when `resize`
    /// names the key their dragged widths are stored under.
    fn headers_impl(&self, ui: &mut Ui, resize: Option<egui::Id>, mut header: impl FnMut(&mut Ui, usize) -> bool) {
        let t = Tokens::get();
        let width = self.width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, row_height() + t.space_6), egui::Sense::hover());
        let radius = egui::CornerRadius { nw: t.radius_lg, ne: t.radius_lg, sw: 0, se: 0 };
        ui.painter().rect_filled(rect, radius, t.text.gamma_multiply(0.08));
        let mut left = rect.left() + t.space_12;
        let first_left = left;
        let mut dividers = Vec::new();
        for (index, (column, width)) in self.columns.iter().zip(&self.widths).enumerate() {
            let at = egui::Rect::from_min_size(egui::pos2(left, rect.top()), vec2(*width, rect.height()));
            let (cell, clip_left) = self.place(index, at, first_left);
            ui.scope_builder(
                egui::UiBuilder::new().id_salt((&self.id, "header", index)).max_rect(cell).layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    let mut clip = ui.clip_rect().intersect(cell);
                    if let Some(x) = clip_left {
                        clip.min.x = clip.min.x.max(x);
                    }
                    ui.set_clip_rect(clip);
                    if !header(ui, index) && !column.icon_only {
                        header_label(ui, column.label);
                    }
                },
            );
            solidcraft_ui_registry::publish_rect(&format!("dialog:column:{}:{}", self.id, column.label), cell);
            if index + 1 < self.columns.len() && clip_left.is_none_or(|x| cell.right() > x) {
                dividers.push((index, cell.right() + t.space_8 * 0.5));
            }
            left = at.right() + t.space_8;
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), t.tertiary_border(t.dialog_rule));
        if let Some(key) = resize {
            for (index, x) in dividers {
                self.column_divider(ui, key, index, x, rect);
            }
        }
        ui.advance_cursor_after_rect(rect);
    }

    /// One header divider: drag to set the column's width, double-click for its automatic width.
    fn column_divider(&self, ui: &mut Ui, key: egui::Id, index: usize, x: f32, row: egui::Rect) {
        let t = Tokens::get();
        let grip = egui::Rect::from_x_y_ranges(x - t.space_4..=x + t.space_4, row.y_range());
        let response = ui
            .interact(grip, ui.make_persistent_id((&self.id, "column_divider", index)), egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeColumn);
        solidcraft_ui_registry::publish_target(&format!("dialog:column_divider:{}:{index}", self.id), grip);
        if response.hovered() || response.dragged() {
            let inset = row.height() * 0.25;
            ui.painter().vline(x, row.top() + inset..=row.bottom() - inset, t.control_border_stroke(t.control_border));
        }
        let current = self.widths.get(index).copied().unwrap_or(0.0);
        let chosen = if response.double_clicked() {
            Some(None)
        } else if response.dragged() {
            let delta = response.drag_delta().x;
            (delta.is_finite() && delta != 0.0).then(|| Some((current + delta).max(MANUAL_MIN_WIDTH)))
        } else {
            None
        };
        if let Some(chosen) = chosen {
            // Bounded, with room for a configuration table's most columns.
            let count = self.columns.len().min(MAX_COLUMNS);
            ui.ctx().data_mut(|m| {
                let mut stored: Vec<Option<f32>> = m.get_persisted(key).unwrap_or_default();
                stored.resize(count, None);
                if let Some(slot) = stored.get_mut(index) {
                    *slot = chosen;
                }
                m.insert_persisted(key, stored);
            });
            ui.ctx().request_repaint();
        }
    }

    fn width(&self) -> f32 {
        let t = Tokens::get();
        (self.widths.iter().sum::<f32>() + t.space_8 * (self.widths.len().saturating_sub(1) + 2) as f32 + t.space_4 * 2.0).max(self.min_width)
    }

    fn is_empty(&self) -> bool {
        self.rows == 0 && !self.group_has_rows
    }

    /// A section owns no rows: callers omit its rows when closed and supply the actual count.
    /// Pass a translated label. The stable response supports pointer, Space and Enter.
    pub fn group(&mut self, ui: &mut Ui, key: &str, label: &str, count: usize, open: &mut bool) -> Response {
        self.finish_selection();
        self.previous_row = None;
        self.group_has_rows |= count > 0;
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(self.width(), row_height() - t.space_2), egui::Sense::hover());
        let mut response = ui.interact(rect, ui.scope_id().with((&self.id, "group", key)), egui::Sense::click());
        if response.clicked() {
            *open = !*open;
            response.mark_changed();
        }
        if self.cell_selection.is_some() {
            self.cell_background.shapes.push(egui::Shape::rect_filled(rect, 0, t.text.gamma_multiply(0.05)));
        } else {
            ui.painter().rect_filled(rect, 0, t.text.gamma_multiply(0.05));
        }
        let rule = t.tertiary_border(t.dialog_rule);
        if self.groups > 0 {
            if self.cell_selection.is_some() {
                self.cell_rules.push((ui.painter().clone(), ui.painter().add(egui::Shape::Noop), rect, rule));
            } else {
                ui.painter().hline(rect.x_range(), rect.top(), rule);
            }
        }
        if self.cell_selection.is_some() {
            self.cell_rules.push((ui.painter().clone(), ui.painter().add(egui::Shape::Noop), rect.translate(vec2(0.0, rect.height())), rule));
        } else {
            ui.painter().hline(rect.x_range(), rect.bottom(), rule);
        }
        self.groups = self.groups.saturating_add(1);

        let icon = egui::Rect::from_center_size(rect.left_center() + vec2(t.space_12, 0.0), vec2(t.space_12, t.space_12));
        super::disclosure(ui.painter(), icon.center(), *open, t.form_dim);
        let mut format = egui::TextFormat::simple(crate::type_face::bold(t.font_small), t.form_dim);
        format.extra_letter_spacing = t.font_small * t.font_section_tracking;
        let text = label.to_uppercase();
        let mut job = egui::text::LayoutJob::single_section(text, format);
        let count = count.to_string();
        let count_galley = ui.painter().layout_no_wrap(count, egui::FontId::proportional(t.font_panel), t.form_dim);
        let count_width = count_galley.size().x + t.space_12;
        let left = icon.right() + t.space_6;
        job.wrap.max_width = (rect.right() - t.space_8 - count_width - t.space_8 - left).max(0.0);
        job.wrap.max_rows = 1;
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let label_width = galley.size().x;
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
        painter.galley(egui::pos2(left, rect.center().y - galley.size().y * 0.5), galley, t.form_dim);
        let pill = egui::Rect::from_min_size(egui::pos2(left + label_width + t.space_8, rect.center().y - t.space_8), vec2(count_width, t.space_16));
        painter.rect_filled(pill, (t.space_16 * 0.5).round() as u8, t.text.gamma_multiply(0.13));
        painter.galley(egui::pos2(pill.left() + t.space_6, pill.center().y - count_galley.size().y * 0.5), count_galley, t.form_dim);
        if response.has_focus() {
            painter.rect_stroke(
                rect.shrink2(vec2(t.space_4, t.space_2 * 0.5)),
                t.radius_md,
                egui::Stroke::new(1.5, t.form_primary),
                egui::StrokeKind::Inside,
            );
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::DisclosureTriangle, ui.is_enabled(), *open, label));
        ui.ctx().accesskit_node_builder(response.id, |node| node.set_expanded(*open));
        let id = format!("dialog:table:{}:group:{key}", self.id);
        solidcraft_ui_registry::publish_rect(&id, rect);
        solidcraft_ui_registry::publish_handle(&id, rect.center());
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn finish_selection(&mut self) {
        if let Some(selection) = self.selection.take() {
            let t = Tokens::get();
            let rect = selection.rect;
            let fill = if selection.hovered { t.selected_bg.lerp_to_gamma(t.form_primary, 0.08) } else { t.selected_bg };
            let shape = egui::Shape::rect_filled(rect, t.radius_md, fill);
            if self.cell_selection.is_some() {
                self.cell_background.shapes.push(shape);
            } else {
                selection.painter.set(selection.slot, shape);
            }
            // The same silhouette clipped to a strip keeps the block's rounded outer edge.
            let bar = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + t.space_3, rect.bottom()));
            selection.painter.with_clip_rect(bar).set(selection.bar_slot, egui::Shape::rect_filled(rect, t.radius_md, t.form_primary));
        }
    }

    /// How much wider the columns are than the table's visible part: 0 when they all fit. A host
    /// that can grow (a resizable panel) can grow by this much to show every column.
    pub fn overflow(&self) -> f32 {
        (self.width() - self.view_width).max(0.0)
    }

    /// A selectable row with independently interactive cells and actions.
    pub fn row(&mut self, ui: &mut Ui, key: &str, selected: bool, add: impl FnOnce(&mut TableRow<'_>)) -> Response {
        self.row_visible(ui, key, selected, true, add)
    }

    /// Keep off-screen rows' space without building their cells. Force active drafts to stay live.
    pub fn row_visible(&mut self, ui: &mut Ui, key: &str, selected: bool, force: bool, add: impl FnOnce(&mut TableRow<'_>)) -> Response {
        let response = self.row_content(ui, key, selected, force, add);
        self.rows = self.rows.saturating_add(1);
        response
    }

    /// Keep the space of `count` rows that are not built (off screen in a virtualized grid), and
    /// count them as rows, so the table does not say it is empty.
    pub fn skip_rows(&mut self, ui: &mut Ui, count: usize) {
        if count > 0 {
            self.finish_selection();
            self.previous_row = None;
            ui.allocate_space(vec2(0.0, super::table_row_height() * count as f32));
            self.rows = self.rows.saturating_add(count);
        }
    }

    /// The last, slim button line. Clicking asks the owner to insert a regular draft row. It is
    /// drawn as a normal button (solid border, body ink, hover highlight), not a placeholder.
    pub fn add_prompt(&mut self, ui: &mut Ui, label: &str, tooltip: &str) -> Response {
        self.finish_selection();
        self.previous_row = None;
        let t = Tokens::get();
        if self.is_empty() && !self.empty_shown {
            super::text(ui, &self.empty, super::Text::Note(&self.id));
            self.empty_shown = true;
        }
        let size = vec2(self.width(), row_height() - t.space_4);
        let (rect, response) = if self.frozen_count() == 0 {
            ui.allocate_exact_size(size, egui::Sense::click())
        } else {
            // With frozen columns the button stays in view as the table scrolls sideways.
            let (full, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let shown =
                egui::Rect::from_min_size(full.min + vec2(self.scroll_x, 0.0), vec2(full.width().min(self.view_width).max(0.0), full.height()));
            (shown, ui.interact(shown, ui.make_persistent_id((&self.id, "add_prompt")), egui::Sense::click()))
        };
        paint_icon_button(ui, rect, &response, solidcraft_ui_text::tr_label(label), false);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), solidcraft_ui_text::tr_label(label)));
        solidcraft_ui_registry::publish_handle(&format!("dialog:table:{}:add", self.id), rect.center());
        solidcraft_ui_registry::publish_rect(&format!("dialog:table:{}:add", self.id), rect);
        response.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(solidcraft_ui_text::tr_label(tooltip))
    }

    fn row_content(&mut self, ui: &mut Ui, key: &str, selected: bool, force: bool, add: impl FnOnce(&mut TableRow<'_>)) -> Response {
        let t = Tokens::get();
        let (allocation, _) = ui.allocate_exact_size(vec2(self.width(), super::table_row_height()), egui::Sense::hover());
        let rect = allocation.shrink2(vec2(t.space_4, 0.0));
        let response = ui.interact(rect, ui.scope_id().with((&self.id, key)), egui::Sense::click());
        let hovered = ui.is_enabled() && ui.rect_contains_pointer(rect);
        let adjacent = self.previous_row.is_some_and(|(previous, _)| (previous.bottom() - rect.top()).abs() <= egui::emath::GUI_ROUNDING);
        if !selected || !adjacent {
            self.finish_selection();
        }
        if let Some((previous, was_selected)) = self.previous_row
            && adjacent
            && !(selected && was_selected)
        {
            if self.cell_selection.is_some() {
                self.cell_rules.push((
                    ui.painter().clone(),
                    ui.painter().add(egui::Shape::Noop),
                    rect,
                    t.tertiary_border(t.dialog_rule.gamma_multiply(0.5)),
                ));
            } else {
                ui.painter().hline(rect.x_range(), previous.bottom(), t.tertiary_border(t.dialog_rule.gamma_multiply(0.5)));
            }
        }
        self.previous_row = Some((rect, selected));
        if selected {
            if let Some(selection) = &mut self.selection {
                selection.rect = selection.rect.union(rect);
                selection.hovered |= hovered;
            } else {
                self.selection = Some(SelectionPaint {
                    rect,
                    slot: ui.painter().add(egui::Shape::Noop),
                    bar_slot: ui.painter().add(egui::Shape::Noop),
                    painter: ui.painter().clone(),
                    hovered,
                });
            }
        }
        if self.cell_selection.is_none() && !force && !ui.is_rect_visible(rect) {
            return response;
        }
        // Record every column even when virtualisation omits the content closure. Placement
        // uses exactly the same frozen-column geometry as the content, never inferred widths.
        let left = rect.left() + t.space_8;
        let mut cell_left = left;
        let placed: Vec<_> = self
            .widths
            .iter()
            .enumerate()
            .map(|(column, width)| {
                let at = egui::Rect::from_min_size(egui::pos2(cell_left, rect.top()), vec2(*width, rect.height()));
                cell_left += *width + t.space_8;
                self.place(column, at, left)
            })
            .collect();
        if self.cell_selection.is_some() {
            if self.cell_paint.is_none() {
                self.cell_paint = Some(CellPaint {
                    painter: ui.painter().clone(),
                    fill: ui.painter().add(egui::Shape::Noop),
                    outline: ui.painter().add(egui::Shape::Noop),
                });
            }
            for (column, (cell, clip_left)) in placed.iter().enumerate() {
                let mut shown = *cell;
                if let Some(x) = clip_left {
                    shown.min.x = shown.min.x.max(*x);
                }
                let target = shown.intersect(ui.clip_rect());
                let response = ui.interact(target, ui.scope_id().with((&self.id, "cell", self.rows, column)), egui::Sense::click_and_drag());
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, escape: true, ..Default::default() },
                    )
                });
                self.cells.push(CellRecord { row: self.rows, column, rect: shown, response });
            }
        }
        if !force && !ui.is_rect_visible(rect) {
            return response;
        }
        if !selected && hovered {
            let shape = egui::Shape::rect_filled(rect, t.radius_md, t.text.gamma_multiply(0.06));
            if self.cell_selection.is_some() {
                self.cell_background.shapes.push(shape);
            } else {
                ui.painter().add(shape);
            }
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, key));
        solidcraft_ui_registry::publish_rect(&format!("dialog:table:{}:{key}", self.id), rect);
        ui.push_id((&self.id, key), |ui| {
            add(&mut TableRow {
                ui,
                placed: &placed,
                widths: &self.widths,
                columns: self.columns,
                rect,
                column: 0,
                left,
                id: format!("dialog:table:{}:{key}", self.id),
                selectable: self.cell_selection.is_some(),
            });
        });
        ui.advance_cursor_after_rect(allocation);
        if response.has_focus() {
            ui.painter().rect_stroke(rect, t.radius_md, egui::Stroke::new(1.5, t.form_primary), egui::StrokeKind::Inside);
        }
        response
    }
}

pub struct TableRow<'a> {
    ui: &'a mut Ui,
    placed: &'a [(egui::Rect, Option<f32>)],
    widths: &'a [f32],
    columns: &'a [Column<'a>],
    rect: egui::Rect,
    column: usize,
    left: f32,
    id: String,
    selectable: bool,
}

impl TableRow<'_> {
    /// Place an inline field, text, choice or row actions in the next clipped cell.
    pub fn cell<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let width = self.widths.get(self.column).copied().unwrap_or(0.0);
        let label = self.columns.get(self.column).map_or("", |c| c.label);
        let at = egui::Rect::from_min_size(egui::pos2(self.left, self.rect.top()), vec2(width, self.rect.height()));
        let (rect, clip_left) = self.placed.get(self.column).copied().unwrap_or((at, None));
        solidcraft_ui_registry::publish_rect(&format!("{}:cell:{label}", self.id), rect);
        self.column += 1;
        self.left += width + Tokens::get().space_8;
        let mut stack = egui::UiStackInfo::default().with_tag_value(super::layout::LABEL, label.to_owned());
        if self.selectable {
            stack = stack.with_tag(CELL_SELECTION);
        }
        self.ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt(self.column)
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .ui_stack_info(stack),
                |ui| {
                    let mut clip = ui.clip_rect().intersect(rect);
                    if let Some(x) = clip_left {
                        clip.min.x = clip.min.x.max(x);
                    }
                    ui.set_clip_rect(clip);
                    // Native dropdowns use the same inside focus border as cell editors.
                    // Keep this local to table cells; regular forms retain their styling.
                    let focus = solidcraft_theme::TabAccent::number_focus_color(ui);
                    let t = Tokens::get();
                    let visuals = ui.visuals_mut();
                    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, t.control_border.lerp_to_gamma(t.form_dim, 0.35));
                    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, focus);
                    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.5, focus);
                    for state in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open]
                    {
                        state.corner_radius = t.control_radius.into();
                        state.expansion = 0.0;
                    }
                    ui.spacing_mut().text_edit_width = width;
                    add(ui)
                },
            )
            .inner
    }
}

/// Clipped row name beside compact actions. Tokens: control_height and scope font/spacing.
pub fn label_actions(ui: &mut Ui, text: &str, add: impl FnOnce(&mut Ui)) {
    let width = dimension(ui.available_width());
    ui.allocate_ui_with_layout(vec2(width * LABEL_ACTIONS_SHARE, row_height()), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        super::text(ui, text, super::Text::Cell);
    });
    add(ui);
}

/// Table heading text. Tokens: font_small, form_dim.
fn header_label(ui: &mut Ui, label: &str) {
    let t = Tokens::get();
    ui.add(
        egui::Label::new(egui::RichText::new(solidcraft_ui_text::tr_label(label)).font(crate::type_face::semibold(t.font_small)).color(t.form_dim))
            .truncate(),
    )
    .backing_on_hover_text(solidcraft_ui_text::tr_label(label));
}

/// How a table orders its rows by one column. `Listed` keeps the order the owner gave them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Sort {
    #[default]
    Listed,
    Ascending,
    Descending,
}

impl Sort {
    /// A click on the header steps ascending, descending, then back to the listed order.
    pub fn next(self) -> Self {
        match self {
            Sort::Listed => Sort::Ascending,
            Sort::Ascending => Sort::Descending,
            Sort::Descending => Sort::Listed,
        }
    }

    /// Order `items` by `key` in natural order (stable: equal names keep their listed order).
    pub fn apply<T>(self, items: &mut [T], key: impl Fn(&T) -> &str) {
        match self {
            Sort::Listed => {}
            Sort::Ascending => items.sort_by(|a, b| natural_cmp(key(a), key(b))),
            Sort::Descending => items.sort_by(|a, b| natural_cmp(key(b), key(a))),
        }
    }
}

/// Natural order for names: runs of digits compare by value ("d2" before "d10"), letters
/// ignore case ("Alpha" between "a" and "b"); exact ties fall back to the plain comparison.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    while let (Some(&p), Some(&q)) = (x.peek(), y.peek()) {
        let order = if p.is_ascii_digit() && q.is_ascii_digit() {
            let run = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                let mut digits = String::new();
                while let Some(c) = it.next_if(char::is_ascii_digit) {
                    digits.push(c);
                }
                digits
            };
            let (m, n) = (run(&mut x), run(&mut y));
            let (m_value, n_value) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
            // Compare the values by length then digits, so no run can overflow an integer;
            // "01" sorts after "1".
            m_value.len().cmp(&n_value.len()).then_with(|| m_value.cmp(n_value)).then_with(|| m.len().cmp(&n.len()))
        } else {
            x.next();
            y.next();
            p.to_lowercase().cmp(q.to_lowercase())
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    x.next().is_some().cmp(&y.next().is_some()).then_with(|| a.cmp(b))
}

/// A header that sorts the table by its column. Clicking steps through [`Sort::next`]; the
/// chevron shows the current direction (up for ascending), and none when rows are as listed.
/// Tokens: control_radius, hover, space_4/14, font_small, form_dim/primary.
pub fn sort_header(ui: &mut Ui, label: &str, sort: &mut Sort) -> Response {
    let t = Tokens::get();
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    let text = solidcraft_ui_text::tr_label(label);
    let font = crate::type_face::semibold(t.font_small);
    let width = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), t.form_dim).size().x.ceil();
    let text_rect = egui::Rect::from_min_max(rect.min, egui::pos2((rect.left() + width).min(rect.right() - t.space_14 - t.space_4), rect.bottom()));
    super::text::paint_text(ui, text_rect, text, font, t.form_dim, false);
    let icon =
        egui::Rect::from_center_size(egui::pos2(text_rect.right() + t.space_4 + t.space_14 * 0.5, rect.center().y), vec2(t.space_14, t.space_14));
    if response.hovered() {
        match *sort {
            Sort::Ascending => crate::icons::paint_ink_flipped(ui.painter(), icon, "chevron", t.form_primary),
            Sort::Descending => crate::icons::paint_ink(ui.painter(), icon, "chevron", t.form_primary),
            Sort::Listed => crate::icons::paint_ink(ui.painter(), icon, "chevron", t.form_dim.gamma_multiply(0.5)),
        }
    }
    if response.clicked() {
        *sort = sort.next();
    }
    let tip = match *sort {
        Sort::Listed => "Sort A to Z",
        Sort::Ascending => "Sort Z to A",
        Sort::Descending => "Back to the original order",
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), text));
    response.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(solidcraft_ui_text::tr_label(tip))
}

/// Column removal appears inside the hovered header only. Keeps the former × button handle.
pub fn removable_header(ui: &mut Ui, label: &str) -> bool {
    let t = Tokens::get();
    let rect = ui.max_rect();
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    let button = egui::Rect::from_center_size(egui::pos2(rect.right() - t.space_10, rect.center().y), vec2(t.space_10 * 2.0, t.space_10 * 2.0));
    let text_rect = egui::Rect::from_min_max(rect.min, egui::pos2(if hovered { button.left() } else { rect.right() }, rect.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| header_label(ui, label));
    if !hovered {
        return false;
    }
    let response = super::actions::icon_button_colored(ui, button, "close", "Remove the column", Some(t.form_error));
    solidcraft_ui_registry::publish_handle(&format!("dialog:button:{label}:×"), button.center());
    solidcraft_ui_registry::publish_rect(&format!("dialog:button:{label}:×"), button);
    response.clicked()
}

/// Icon-only row delete. The old Actions handles remain aliases for the bin target.
pub fn bin_button(ui: &mut Ui, tooltip: &str) -> Response {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(t.control_height, t.control_height), egui::Sense::hover());
    let response = super::actions::icon_button_colored(ui, rect, "delete", tooltip, Some(t.form_error));
    for (row, label) in
        [("Actions", "Delete"), ("Actions", "×"), (super::layout::label(ui), solidcraft_ui_text::tr_label("Delete")), (super::layout::label(ui), "×")]
    {
        let key = format!("dialog:button:{row}:{label}");
        solidcraft_ui_registry::publish_handle(&key, rect.center());
        solidcraft_ui_registry::publish_rect(&key, rect);
    }
    response
}

/// A button with a leading plus: the kit's normal fill, border, ink and hover highlight.
/// Tokens: control_radius, space_6/14/16, font_label, control_fill/border, form_ink, hover.
fn paint_icon_button(ui: &Ui, rect: egui::Rect, response: &Response, label: &str, centred: bool) {
    let t = Tokens::get();
    let (fill, ink, stroke) = super::actions::normal_button_look(ui, response);
    ui.painter().rect(rect, t.control_radius, fill, stroke, egui::StrokeKind::Inside);
    t.button_bevel(ui, response, false);
    let font = egui::FontId::proportional(t.font_label);
    let text_width = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), ink).size().x.ceil();
    let content = t.space_14 + t.space_6 + text_width;
    let left = if centred { rect.center().x - content / 2.0 } else { rect.left() + t.space_16 };
    let icon = egui::Rect::from_center_size(egui::pos2(left + t.space_14 / 2.0, rect.center().y), vec2(t.space_14, t.space_14));
    crate::icons::paint_ink(ui.painter(), icon, "plus", ink);
    super::text::paint_text(ui, egui::Rect::from_min_max(egui::pos2(icon.right() + t.space_6, rect.top()), rect.max), label, font, ink, false);
}

/// The Configuration Table's "+ Add column" button, above the table. It opens the column menu
/// (a filter field and the choices), which stays open for a click inside it; entries close it.
pub fn add_column_button<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let t = Tokens::get();
    let label = solidcraft_ui_text::tr_label("Add column");
    let font = egui::FontId::proportional(t.font_label);
    let width = ui.painter().layout_no_wrap(label.to_owned(), font, t.form_ink).size().x.ceil() + t.space_16 * 2.0 + t.space_14 + t.space_6;
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.control_height), egui::Sense::click());
    paint_icon_button(ui, rect, &response, label, true);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(solidcraft_ui_text::tr_label("Add Column"));
    solidcraft_ui_registry::publish_handle("dialog:menu:Add Column", rect.center());
    solidcraft_ui_registry::publish_rect("dialog:menu:Add Column", rect);
    let popup = Container::new(ui.ctx(), Surface::Popup).menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let open = popup.is_open();
    let result = popup.show(|ui| super::scope(ui, add)).map(|r| r.inner);
    if open {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
    result
}

/// Active configuration status, on the same line as the name and optional Apply action.
pub fn active_pill(ui: &mut Ui) {
    let t = Tokens::get();
    let label = solidcraft_ui_text::tr_label("Active");
    let width = label_width(ui, label) + t.space_16;
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.font_small + t.space_6), egui::Sense::hover());
    ui.painter().rect(rect, t.radius_xl, t.control_fill, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    super::text::paint_text(ui, rect.shrink2(vec2(t.space_8, 0.0)), label, crate::type_face::semibold(t.font_small), t.form_primary, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), label));
    solidcraft_ui_registry::publish_handle(&format!("dialog:inline:{label}"), rect.center());
    solidcraft_ui_registry::publish_rect(&format!("dialog:inline:{label}"), rect);
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
