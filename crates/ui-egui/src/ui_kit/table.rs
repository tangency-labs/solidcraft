//! Dense editable tables. Geometry and typography share the form label-column tokens.
use super::metrics::*;
use crate::ui_kit::{Container, Surface};
use crate::{scenario, theme::Tokens};
use egui::{Response, Ui, vec2};

/// A column's minimum width is a multiple of the measured form label column.
pub(crate) struct Column<'a> {
    label: &'a str,
    width: f32,
    min_content_width: f32,
    icon_only: bool,
    flexible: bool,
}

impl<'a> Column<'a> {
    pub(crate) fn new(label: &'a str, width: f32) -> Self {
        Self { label, width, min_content_width: 0.0, icon_only: false, flexible: false }
    }

    /// Share the remaining visible table width using this column's width as its weight.
    /// Explicit minimums keep editors usable and allow horizontal scrolling on small cards.
    pub(crate) fn flex(mut self) -> Self {
        self.flexible = true;
        self
    }

    /// Retain the column identity for automation without a visible action heading.
    pub(crate) fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// Keep enough room for the measured cell controls, even in a narrow scrolling card.
    pub(crate) fn min_content_width(mut self, width: f32) -> Self {
        self.min_content_width = dimension(width);
        self
    }
}

pub(crate) struct Table<'a> {
    columns: &'a [Column<'a>],
    widths: Vec<f32>,
    id: String,
    rows: usize,
    empty: String,
    empty_shown: bool,
    reveal: Option<egui::Rect>,
}

/// Header, horizontally scrolling body and empty state. Regular rows use table_row_height.
/// Tokens: form_label_width, font_label, dialog_label, space_4/8, dialog_rule, text/dim, selected_bg.
pub(crate) fn table<R>(ui: &mut Ui, id: &str, columns: &[Column<'_>], empty: &str, add: impl FnOnce(&mut Ui, &mut Table<'_>) -> R) -> R {
    table_custom(ui, id, columns, empty, |ui, table| {
        table.headers(ui, |_, _| false);
        add(ui, table)
    })
}

/// The same table grid with caller-supplied header actions through Table::headers.
/// Custom cells own their actions; all body rows share the same widths and metrics.
pub(crate) fn table_custom<R>(ui: &mut Ui, id: &str, columns: &[Column<'_>], empty: &str, add: impl FnOnce(&mut Ui, &mut Table<'_>) -> R) -> R {
    let mut reveal = None;
    let output = egui::ScrollArea::horizontal().id_salt(("table", id)).content_margin(egui::Margin::ZERO).auto_shrink([false, true]).show_viewport(
        ui,
        |ui, viewport| {
            // ScrollArea snaps its content rect. Measure here, after that rounding and any
            // enclosing list margins, and never allocate more than the actual viewport.
            let ppp = ui.ctx().pixels_per_point().max(0.5);
            let available = (dimension(ui.available_width()).min(viewport.width()) * ppp).floor() / ppp;
            let mut table = Table {
                columns,
                widths: column_widths(ui, columns, available),
                id: id.to_owned(),
                rows: 0,
                empty: empty.to_owned(),
                empty_shown: false,
                reveal: None,
            };
            ui.spacing_mut().item_spacing.y = 0.0;
            let result = add(ui, &mut table);
            if table.rows == 0 && !table.empty_shown {
                super::text(ui, empty, super::Text::Note(id));
            }
            reveal = table.reveal;
            result
        },
    );
    scenario::publish_rect(&format!("dialog:table:{id}:viewport"), output.inner_rect);
    scenario::publish_count(&format!("dialog:table:{id}:content_width"), f64::from(output.content_size.x));
    // The nested horizontal ScrollArea consumes vertical scroll targets too. Request the
    // reveal here so it reaches the card's vertical scroller, only after an explicit action.
    if let Some(rect) = reveal {
        ui.scroll_to_rect(rect, None);
    }
    output.inner
}

fn column_widths(ui: &Ui, columns: &[Column<'_>], available: f32) -> Vec<f32> {
    let t = Tokens::get();
    let mut widths: Vec<_> = columns
        .iter()
        .map(|c| {
            if c.flexible || c.icon_only {
                c.min_content_width.max(t.control_height)
            } else {
                (header_column(ui, c.label) * dimension(c.width)).max(label_width(ui, c.label)).max(c.min_content_width)
            }
        })
        .collect();
    let fixed: f32 = columns.iter().zip(&widths).filter(|(c, _)| !c.flexible).map(|(_, w)| *w).sum();
    let mut remaining = (available - fixed - t.space_8 * (columns.len().saturating_sub(1) + 2) as f32).max(0.0);
    let mut sharing: Vec<_> = columns.iter().map(|c| c.flexible && dimension(c.width) > 0.0).collect();
    // Freeze columns at their control minimum before sharing the remaining width. The bounded
    // passes also keep a hostile design with many columns from creating an unbounded loop.
    for _ in 0..columns.len().min(64) {
        let weight: f32 = columns.iter().zip(&sharing).filter(|(_, share)| **share).map(|(c, _)| dimension(c.width)).sum();
        if weight <= 0.0 {
            break;
        }
        let available = remaining;
        let mut frozen = false;
        for ((column, width), share) in columns.iter().zip(&mut widths).zip(&mut sharing) {
            if *share && *width > available * dimension(column.width) / weight {
                *share = false;
                remaining = (remaining - *width).max(0.0);
                frozen = true;
            }
        }
        if !frozen {
            for ((column, width), share) in columns.iter().zip(&mut widths).zip(&sharing) {
                if *share {
                    *width = remaining * dimension(column.width) / weight;
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

impl Table<'_> {
    /// Reveal a newly inserted row through the enclosing card's vertical scroller.
    pub(crate) fn reveal_row(&mut self, rect: egui::Rect) {
        self.reveal = Some(rect);
    }

    pub(crate) fn headers(&self, ui: &mut Ui, mut header: impl FnMut(&mut Ui, usize) -> bool) {
        let t = Tokens::get();
        let width = self.width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, row_height()), egui::Sense::hover());
        let ppp = ui.ctx().pixels_per_point().max(0.5);
        let mut left = (rect.left() * ppp).round() / ppp + t.space_8;
        for (index, (column, width)) in self.columns.iter().zip(&self.widths).enumerate() {
            let cell = egui::Rect::from_min_size(egui::pos2(left, rect.top()), vec2(*width, rect.height()));
            ui.scope_builder(
                egui::UiBuilder::new().id_salt((&self.id, "header", index)).max_rect(cell).layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(cell));
                    if !header(ui, index) && !column.icon_only {
                        header_label(ui, column.label);
                    }
                },
            );
            scenario::publish_rect(&format!("dialog:column:{}:{}", self.id, column.label), cell);
            left = cell.right() + t.space_8;
        }
        ui.painter().hline(rect.x_range(), rect.bottom(), t.tertiary_border(t.dialog_rule));
        ui.advance_cursor_after_rect(rect);
    }

    fn width(&self) -> f32 {
        self.widths.iter().sum::<f32>() + Tokens::get().space_8 * (self.widths.len().saturating_sub(1) + 2) as f32
    }

    /// A selectable row with independently interactive cells and actions.
    pub(crate) fn row(&mut self, ui: &mut Ui, key: &str, selected: bool, add: impl FnOnce(&mut TableRow<'_>)) -> Response {
        self.row_visible(ui, key, selected, true, add)
    }

    /// Keep off-screen rows' space without building their cells. Force active drafts to stay live.
    pub(crate) fn row_visible(&mut self, ui: &mut Ui, key: &str, selected: bool, force: bool, add: impl FnOnce(&mut TableRow<'_>)) -> Response {
        let response = self.row_content(ui, key, selected, force, add);
        self.rows += 1;
        response
    }

    /// The last, slim dashed line. Clicking asks the owner to insert a regular draft row.
    pub(crate) fn add_prompt(&mut self, ui: &mut Ui, label: &str, tooltip: &str) -> Response {
        let t = Tokens::get();
        if self.rows == 0 && !self.empty_shown {
            super::text(ui, &self.empty, super::Text::Note(&self.id));
            self.empty_shown = true;
        }
        let (rect, response) = ui.allocate_exact_size(vec2(self.width(), row_height() - t.space_4), egui::Sense::click());
        let stroke = t.control_border_stroke(t.control_border);
        let mut outline = Vec::new();
        egui::epaint::tessellator::path::rounded_rectangle(&mut outline, rect.shrink(stroke.width * 0.5), f32::from(t.control_radius).into());
        if let Some(first) = outline.first().copied() {
            outline.push(first);
        }
        ui.painter().add(egui::Shape::dashed_line(&outline, stroke, t.space_4, t.space_3));
        let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_16, rect.center().y), vec2(t.space_14, t.space_14));
        crate::icons::paint_ink(ui.painter(), icon, "plus", t.form_dim);
        super::text::paint_text(
            ui,
            egui::Rect::from_min_max(egui::pos2(icon.right() + t.space_6, rect.top()), rect.max),
            crate::i18n::tr_label(label),
            egui::FontId::proportional(t.font_label),
            t.form_dim,
            false,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), crate::i18n::tr_label(label)));
        scenario::publish_handle(&format!("dialog:table:{}:add", self.id), rect.center());
        scenario::publish_rect(&format!("dialog:table:{}:add", self.id), rect);
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(crate::i18n::tr_label(tooltip))
    }

    fn row_content(&self, ui: &mut Ui, key: &str, selected: bool, force: bool, add: impl FnOnce(&mut TableRow<'_>)) -> Response {
        let t = Tokens::get();
        let (rect, response) = ui.allocate_exact_size(vec2(self.width(), super::table_row_height()), egui::Sense::click());
        if !force && !ui.is_rect_visible(rect) {
            return response;
        }
        if selected || (ui.is_enabled() && ui.rect_contains_pointer(rect)) {
            ui.painter().rect_filled(rect, t.control_radius, if selected { t.selected_bg } else { t.hover });
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, key));
        scenario::publish_rect(&format!("dialog:table:{}:{key}", self.id), rect);
        let ppp = ui.ctx().pixels_per_point().max(0.5);
        let left = (rect.left() * ppp).round() / ppp + t.space_8;
        ui.push_id((&self.id, key), |ui| {
            add(&mut TableRow {
                ui,
                widths: &self.widths,
                columns: self.columns,
                rect,
                column: 0,
                left,
                id: format!("dialog:table:{}:{key}", self.id),
            });
        });
        ui.advance_cursor_after_rect(rect);
        response
    }
}

pub(crate) struct TableRow<'a> {
    ui: &'a mut Ui,
    widths: &'a [f32],
    columns: &'a [Column<'a>],
    rect: egui::Rect,
    column: usize,
    left: f32,
    id: String,
}

impl TableRow<'_> {
    /// Place an inline field, text, choice or row actions in the next clipped cell.
    pub(crate) fn cell<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let width = self.widths.get(self.column).copied().unwrap_or(0.0);
        let label = self.columns.get(self.column).map_or("", |c| c.label);
        let rect = egui::Rect::from_min_size(egui::pos2(self.left, self.rect.top()), vec2(width, self.rect.height()));
        scenario::publish_rect(&format!("{}:cell:{label}", self.id), rect);
        self.column += 1;
        self.left += width + Tokens::get().space_8;
        self.ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt(self.column)
                    .max_rect(rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .ui_stack_info(egui::UiStackInfo::default().with_tag_value(super::layout::LABEL, label.to_owned())),
                |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(rect));
                    ui.spacing_mut().text_edit_width = width;
                    add(ui)
                },
            )
            .inner
    }
}

/// Clipped row name beside compact actions. Tokens: control_height and scope font/spacing.
pub(crate) fn label_actions(ui: &mut Ui, text: &str, add: impl FnOnce(&mut Ui)) {
    let width = dimension(ui.available_width());
    ui.allocate_ui_with_layout(vec2(width * LABEL_ACTIONS_SHARE, row_height()), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        super::text(ui, text, super::Text::Cell);
    });
    add(ui);
}

/// Table heading text. Tokens: font_small, form_dim.
fn header_label(ui: &mut Ui, label: &str) {
    let t = Tokens::get();
    ui.add(egui::Label::new(egui::RichText::new(crate::i18n::tr_label(label)).size(t.font_small).strong().color(t.form_dim)).truncate())
        .on_hover_text(crate::i18n::tr_label(label));
}

/// How a table orders its rows by one column. `Listed` keeps the order the owner gave them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Sort {
    #[default]
    Listed,
    Ascending,
    Descending,
}

impl Sort {
    /// A click on the header steps ascending, descending, then back to the listed order.
    pub(crate) fn next(self) -> Self {
        match self {
            Sort::Listed => Sort::Ascending,
            Sort::Ascending => Sort::Descending,
            Sort::Descending => Sort::Listed,
        }
    }

    /// Order `items` by `key` in natural order (stable: equal names keep their listed order).
    pub(crate) fn apply<T>(self, items: &mut [T], key: impl Fn(&T) -> &str) {
        match self {
            Sort::Listed => {}
            Sort::Ascending => items.sort_by(|a, b| natural_cmp(key(a), key(b))),
            Sort::Descending => items.sort_by(|a, b| natural_cmp(key(b), key(a))),
        }
    }
}

/// Natural order for names: runs of digits compare by value ("d2" before "d10"), letters
/// ignore case ("Alpha" between "a" and "b"); exact ties fall back to the plain comparison.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
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
pub(crate) fn sort_header(ui: &mut Ui, label: &str, sort: &mut Sort) -> Response {
    let t = Tokens::get();
    let rect = ui.max_rect();
    let response = ui.allocate_rect(rect, egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    let text = crate::i18n::tr_label(label);
    let font = crate::fonts::semibold(t.font_small);
    let width = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), t.form_dim).size().x.ceil();
    let text_rect = egui::Rect::from_min_max(rect.min, egui::pos2((rect.left() + width).min(rect.right() - t.space_14 - t.space_4), rect.bottom()));
    super::text::paint_text(ui, text_rect, text, font, t.form_dim, false);
    let icon =
        egui::Rect::from_center_size(egui::pos2(text_rect.right() + t.space_4 + t.space_14 * 0.5, rect.center().y), vec2(t.space_14, t.space_14));
    match *sort {
        Sort::Ascending => crate::icons::paint_ink_flipped(ui.painter(), icon, "chevron", t.form_primary),
        Sort::Descending => crate::icons::paint_ink(ui.painter(), icon, "chevron", t.form_primary),
        Sort::Listed if response.hovered() => crate::icons::paint_ink(ui.painter(), icon, "chevron", t.form_dim.gamma_multiply(0.5)),
        Sort::Listed => {}
    }
    if response.clicked() {
        *sort = sort.next();
    }
    let tip = match *sort {
        Sort::Listed => "Sort A to Z",
        Sort::Ascending => "Sort Z to A",
        Sort::Descending => "Back to the original order",
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(crate::i18n::tr_label(tip))
}

/// Column removal appears inside the hovered header only. Keeps the former × button handle.
pub(crate) fn removable_header(ui: &mut Ui, label: &str) -> bool {
    let t = Tokens::get();
    let rect = ui.max_rect();
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect, t.control_radius, t.dialog_footer);
    }
    let button = egui::Rect::from_center_size(egui::pos2(rect.right() - t.space_10, rect.center().y), vec2(t.space_10 * 2.0, t.space_10 * 2.0));
    let text_rect = egui::Rect::from_min_max(rect.min, egui::pos2(if hovered { button.left() } else { rect.right() }, rect.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| header_label(ui, label));
    if !hovered {
        return false;
    }
    let response = super::actions::icon_button_colored(ui, button, "close", "Remove the column", Some(t.form_error));
    scenario::publish_handle(&format!("dialog:button:{label}:×"), button.center());
    scenario::publish_rect(&format!("dialog:button:{label}:×"), button);
    response.clicked()
}

/// Icon-only row delete. The old Actions handles remain aliases for the bin target.
pub(crate) fn bin_button(ui: &mut Ui, tooltip: &str) -> Response {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(t.control_height, t.control_height), egui::Sense::hover());
    let response = super::actions::icon_button_colored(ui, rect, "delete", tooltip, Some(t.form_error));
    for (row, label) in
        [("Actions", "Delete"), ("Actions", "×"), (super::layout::label(ui), crate::i18n::tr_label("Delete")), (super::layout::label(ui), "×")]
    {
        let key = format!("dialog:button:{row}:{label}");
        scenario::publish_handle(&key, rect.center());
        scenario::publish_rect(&key, rect);
    }
    response
}

/// Last header cell opens the unchanged column menu. Hover brackets indicate insertion. The
/// menu holds a filter field, so a click inside it (into the field) keeps it open: entries
/// close it themselves when picked.
pub(crate) fn plus_header<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let t = Tokens::get();
    let rect = egui::Rect::from_center_size(ui.max_rect().center(), vec2(t.form_close_size - t.space_4, t.form_close_size - t.space_4));
    let response = super::actions::icon_button_colored(ui, rect, "plus", "Add Column", Some(t.form_dim));
    if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.selected_bg);
        crate::icons::paint_ink(ui.painter(), rect.shrink(t.form_icon_inset), "plus", t.form_primary);
        for x in [rect.left() - t.form_gap_y, rect.right() + t.form_gap_y] {
            ui.painter().vline(x, rect.y_range(), egui::Stroke::new(t.space_2, t.form_primary));
        }
    }
    scenario::publish_handle("dialog:menu:Add Column", rect.center());
    scenario::publish_rect("dialog:menu:Add Column", rect);
    let popup = Container::new(ui.ctx(), Surface::Popup).menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let open = popup.is_open();
    let result = popup.show(|ui| super::scope(ui, add)).map(|r| r.inner);
    if open {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
    result
}

/// Active configuration status, on the same line as the name and optional Apply action.
pub(crate) fn active_pill(ui: &mut Ui) {
    let t = Tokens::get();
    let label = crate::i18n::tr_label("Active");
    let width = label_width(ui, label) + t.space_16;
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.font_small + t.space_6), egui::Sense::hover());
    ui.painter().rect(rect, t.radius_xl, t.control_fill, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    super::text::paint_text(ui, rect.shrink2(vec2(t.space_8, 0.0)), label, crate::fonts::semibold(t.font_small), t.form_primary, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    scenario::publish_handle(&format!("dialog:inline:{label}"), rect.center());
    scenario::publish_rect(&format!("dialog:inline:{label}"), rect);
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
