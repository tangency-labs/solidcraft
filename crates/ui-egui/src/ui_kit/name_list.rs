//! A list of named rows, such as the web app's stored files: the name takes the width the other
//! cells leave. Built on the kit's table grid; see `docs/ui-kit.md` for tokens and usage.
use super::metrics::dimension;
use super::table::{Column, Table, TableRow, table_custom};
use crate::{scenario, theme::Tokens};
use egui::{Response, Ui, vec2};

/// Columns past this are ignored: a list of named rows has a handful of cells.
const MAX_COLUMNS: usize = 16;

/// Widest a cell past the name is laid out and measured, in points.
const MEASURE_WIDTH: f32 = 4096.0;

/// Draw a list whose first column, the name, takes every point the other columns leave. A name
/// is only cut, with an ellipsis and the full name on hover, when even that width is too narrow.
/// The other columns are as wide as their widest cell, measured as the rows are drawn: a frame
/// whose measurement changed is discarded and drawn again, so a new row never overlaps.
/// `columns` names the columns for automation (`dialog:table:{id}:{key}:cell:{column}`); no
/// header is drawn. Tokens: form_label_width, font_label, form_ink, space_8, table_row_height.
pub fn name_list(ui: &mut Ui, id: &str, columns: &[&str], empty: &str, add: impl FnOnce(&mut NameList<'_, '_>)) {
    let t = Tokens::get();
    let columns = columns.get(..columns.len().min(MAX_COLUMNS)).unwrap_or_default();
    let key = ui.scope_id().with(("name_list", id));
    let measured: Vec<f32> = ui.data(|d| d.get_temp(key)).unwrap_or_default();
    let grid: Vec<Column<'_>> = columns
        .iter()
        .enumerate()
        .map(|(i, label)| {
            if i == 0 {
                Column::new(label, 1.0).flex().min_content_width(t.form_label_width)
            } else {
                Column::new(label, 0.0).min_content_width(measured.get(i).copied().unwrap_or(0.0))
            }
        })
        .collect();
    let mut widest = vec![0.0; columns.len()];
    table_custom(ui, id, &grid, empty, |ui, table| add(&mut NameList { ui, table, widest: &mut widest }));
    let settled = widest.len() == measured.len() && widest.iter().zip(&measured).all(|(a, b)| (a - b).abs() <= 0.5);
    if !settled {
        ui.data_mut(|d| d.insert_temp(key, widest));
        ui.ctx().request_discard("name list measured its cells");
    }
}

/// The rows of a [`name_list`].
pub struct NameList<'a, 'b> {
    ui: &'a mut Ui,
    table: &'a mut Table<'b>,
    widest: &'a mut Vec<f32>,
}

impl NameList<'_, '_> {
    /// One row; `key` (the name, say) identifies it for automation. Cells go left to right.
    pub fn row(&mut self, key: &str, add: impl FnOnce(&mut NameRow<'_, '_>)) -> Response {
        let widest = &mut *self.widest;
        self.table.row(self.ui, key, false, |row| add(&mut NameRow { row, widest, column: 0 }))
    }
}

/// The cells of one [`NameList`] row.
pub struct NameRow<'a, 'b> {
    row: &'a mut TableRow<'b>,
    widest: &'a mut Vec<f32>,
    column: usize,
}

impl NameRow<'_, '_> {
    /// The name, in the next cell (the first, normally).
    pub fn name(&mut self, name: &str) -> Response {
        self.cell(|ui| name_text(ui, name))
    }

    /// Any content in the next cell, such as a name editor, a size or row actions. Past the name,
    /// the content's own width sets the column's width.
    pub fn cell<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let column = self.column;
        self.column += 1;
        let widest = &mut *self.widest;
        self.row.cell(|ui| {
            if column == 0 {
                return add(ui);
            }
            // Lay the content out at its own width (no eliding, wrapping or shrinking to the
            // current column), so the measurement can grow the column; the cell still clips.
            let rect = egui::Rect::from_min_size(ui.max_rect().min, vec2(MEASURE_WIDTH, ui.max_rect().height()));
            let inner = ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let result = add(ui);
                (result, ui.min_rect().width())
            });
            let (result, used) = inner.inner;
            if let Some(width) = widest.get_mut(column) {
                *width = width.max(dimension(used).min(MEASURE_WIDTH).ceil());
            }
            result
        })
    }
}

/// One line of name text across the cell. Only a name that does not fit is cut with an ellipsis
/// and given its full text on hover. Tokens: font_label, form_ink, control_height.
fn name_text(ui: &mut Ui, name: &str) -> Response {
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_label);
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), super::row_height()), egui::Sense::hover());
    let mut job = egui::text::LayoutJob::single_section(name.to_owned(), egui::TextFormat::simple(font.clone(), t.form_ink));
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = 1;
    let cut = ui.fonts_mut(|fonts| fonts.layout_job(job)).elided;
    super::text::paint_text(ui, rect, name, font, t.form_ink, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), name));
    let id = format!("dialog:name:{name}");
    scenario::publish_rect(&id, rect);
    scenario::publish_count(&format!("{id}:cut"), f64::from(u8::from(cut)));
    if cut { response.on_hover_text(name) } else { response }
}
