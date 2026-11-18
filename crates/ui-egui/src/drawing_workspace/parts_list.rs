//! Parts List: placing a parts list on a sheet (`drawing.table.add`) and the panel that edits a
//! placed one (`drawing.table.edit`). The panel keeps what is typed in a draft, which the table's
//! own rule ([`Table::validate`]) checks whenever it changes; Apply and OK send what differs in one
//! edit, so each is one undo step.
use serde_json::{Map, Value, json};
use solidcraft_engine::{
    doc::{drawing::table::Kind, ids},
    geom::table::{BALLOONED, Heading, Table},
};

use super::close_panel;
use crate::{
    SolveApp,
    ui_kit::{self, BackingTooltipExt as _, Button},
};

fn tr(key: &'static str) -> &'static str {
    solidcraft_ui_text::tr(key)
}

/// The panel's fields for one list, as typed.
pub(super) struct Draft {
    table: u64,
    /// The list as saved when the fields were filled: a change made elsewhere, such as an undo,
    /// fills them again.
    saved: Table,
    /// 0: the heading at the top; 1: at the bottom.
    heading: usize,
    row_height: String,
    /// One per column of the saved list, in its order.
    columns: Vec<ColumnDraft>,
    /// The definition the fields make, or why they make none.
    checked: Result<Table, String>,
}

struct ColumnDraft {
    shown: bool,
    width: String,
}

impl Draft {
    fn of(table: u64, saved: &Table) -> Self {
        let columns =
            saved.columns.iter().map(|c| ColumnDraft { shown: c.printed, width: c.width_mm.map_or_else(String::new, |w| w.to_string()) }).collect();
        Draft {
            table,
            saved: saved.clone(),
            heading: usize::from(saved.heading == Heading::Bottom),
            row_height: saved.row_height_mm.to_string(),
            columns,
            checked: Ok(saved.clone()),
        }
    }

    /// The saved list with these fields, when the table's rules allow it. The column only the
    /// editor reads ([`BALLOONED`]) stays as it is.
    fn definition(&self) -> Result<Table, String> {
        let mut table = self.saved.clone();
        table.heading = if self.heading == 1 { Heading::Bottom } else { Heading::Top };
        table.row_height_mm = millimetres(&self.row_height)?;
        for (column, field) in table.columns.iter_mut().zip(&self.columns).filter(|(column, _)| column.name != BALLOONED) {
            column.printed = field.shown;
            column.width_mm = if field.width.trim().is_empty() { None } else { Some(millimetres(&field.width)?) };
        }
        table.validate().map_err(|e| e.to_string())?;
        Ok(table)
    }

    /// What `drawing.table.edit` is given to make the saved list into `table`: the fields that
    /// differ.
    fn changes(&self, table: &Table) -> Map<String, Value> {
        let mut set = Map::new();
        if table.heading != self.saved.heading {
            set.insert("heading".into(), json!(table.heading));
        }
        if table.row_height_mm != self.saved.row_height_mm {
            set.insert("row_height_mm".into(), json!(table.row_height_mm));
        }
        if table.columns != self.saved.columns {
            set.insert("columns".into(), json!(table.columns));
        }
        set
    }
}

/// A typed length in millimetres ("9" or "9 mm"): a finite number only.
fn millimetres(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let number = text.strip_suffix("mm").unwrap_or(text).trim();
    number.parse::<f64>().ok().filter(|value| value.is_finite()).ok_or_else(|| tr("ui.drawing_parts_list.number").to_owned())
}

/// The panel: while placing, how to place a list; for list `selected`, its fields, ending with
/// Cancel, Apply and OK.
pub(super) fn editor(app: &mut SolveApp, ui: &mut egui::Ui, selected: Option<u64>) {
    let saved = selected.and_then(|id| {
        let drawing = app.session.drawing("drawing.inspect").ok()?;
        drawing.tables.by_id(id).filter(|table| matches!(table.kind, Kind::PartsList(_))).map(|table| (id, table.table.clone()))
    });
    let Some((id, saved)) = saved else {
        // Placing, or an undo took the list away: nothing to edit.
        let cancel = super::inset(ui, |ui| {
            if selected.is_none() {
                ui_kit::text(ui, tr("ui.drawing_parts_list.place"), ui_kit::Text::Note("drawing_parts_list_place"));
            }
            ui.separator();
            ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked()
        });
        if cancel {
            close_panel(app);
        }
        return;
    };
    let mut draft = app.drawing_workspace.parts_list.take().filter(|d| d.table == id && d.saved == saved).unwrap_or_else(|| Draft::of(id, &saved));
    let model = app.session.doc.name.clone();
    let (cancel, apply, ok) = super::inset(ui, |ui| {
        let mut changed = false;
        ui_kit::value_row(ui, tr("ui.drawing_panel.model"), &model);
        ui_kit::row(ui, tr("ui.drawing_panel.heading"), |ui| {
            let targets = ["drawing:parts_list:heading:top", "drawing:parts_list:heading:bottom"];
            let segments = ui_kit::Segments { workspace_accent: true, targets: &targets, ..Default::default() };
            changed |= ui_kit::segmented(ui, &mut draft.heading, &[tr("ui.top"), tr("ui.bottom")], segments);
        });
        let (_, height) = ui_kit::row(ui, tr("ui.drawing_panel.row_height"), |ui| super::panels::unit_field(ui, &mut draft.row_height, "", "mm"));
        crate::scenario::publish_target("drawing:parts_list:row_height", height.rect);
        changed |= height.changed();
        ui_kit::section_heading(ui, tr("ui.drawing_panel.columns"));
        for (column, field) in saved.columns.iter().zip(&mut draft.columns).filter(|(column, _)| column.name != BALLOONED) {
            ui_kit::row(ui, &column.heading, |ui| {
                ui.horizontal(|ui| {
                    let shown = ui_kit::checkbox(ui, "", &mut field.shown);
                    crate::scenario::publish_target(&format!("drawing:parts_list:column:{}:shown", column.name), shown.rect);
                    let (_, width) = super::panels::unit_field(ui, &mut field.width, tr("ui.drawing_panel.auto"), "mm");
                    crate::scenario::publish_target(&format!("drawing:parts_list:column:{}:width", column.name), width.rect);
                    changed |= shown.changed() || width.changed();
                });
            });
        }
        if changed {
            draft.checked = draft.definition();
        }
        if let Err(why) = &draft.checked {
            ui_kit::text(ui, why, ui_kit::Text::Error("drawing_parts_list_refused"));
        }
        ui_kit::section_heading(ui, tr("ui.drawing_panel.items"));
        ui_kit::button(ui, tr("ui.drawing_panel.renumber"), ui_kit::Button::Enabled(false))
            .backing_on_disabled_hover_text(tr("ui.drawing_parts_list.renumber"));
        ui.separator();
        let valid = draft.checked.is_ok();
        ui.horizontal(|ui| {
            let cancel = ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked();
            let apply = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.apply"), Button::Normal)).inner.clicked();
            let ok = ui.add_enabled_ui(valid, |ui| ui_kit::button(ui, tr("ui.heading_ok"), Button::Normal)).inner.clicked();
            (cancel, apply, ok)
        })
        .inner
    });
    if cancel {
        close_panel(app);
        return;
    }
    let set = match &draft.checked {
        Ok(table) if apply || ok => draft.changes(table),
        _ => Map::new(),
    };
    app.drawing_workspace.parts_list = Some(draft);
    if !set.is_empty()
        && let Err(error) = app.run("drawing.table.edit", json!({"table": ids::encode(id), "set": set}))
    {
        app.set_status(error, true);
    } else if ok {
        close_panel(app);
    }
}
