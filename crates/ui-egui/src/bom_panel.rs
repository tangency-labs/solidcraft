//! The Bill of Materials panel: `assembly.bom`'s rows (flat or top level, grouped by part
//! number or by definition, a part's sub-parts in its row or not), with Export CSV… and Export
//! JSON…. Picking a row edits its part
//! number, description and revision (`metadata::BOM_KEYS`) through one `component.metadata.set`
//! per definition. The toolbar's Bill of Materials (`ui.start` of `assembly.bom`) opens it.

use std::rc::Rc;

use egui::RichText;
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::metadata::{self, BOM_KEYS};

use crate::SolveApp;
use solidcraft_theme::Tokens;

/// The panel's state; the rows come from the engine.
#[derive(Default)]
pub struct BomPanel {
    document: String,
    focus_pending: bool,
    /// Top level instead of flat.
    pub top: bool,
    /// One row per definition instead of per part number.
    pub by_definition: bool,
    /// A part with a part number keeps its sub-parts (a bolt's washers and nut) in its row.
    pub group_children: bool,
    /// The rows for (design revision, top, by_definition, group_children).
    cache: Option<(u64, [bool; 3], Rc<Value>)>,
    /// The picked row's index, and its fields as edited (one for each of [`BOM_KEYS`]).
    pub picked: Option<usize>,
    pub fields: Vec<String>,
    /// The selected definitions, whose row index can change when the table refreshes.
    target: Option<(Value, Value)>,
    /// The last refusal.
    pub error: Option<String>,
    /// The design revision, the picked row and its fields when Apply was refused: `error` stays
    /// only while they still are.
    refused: Option<(u64, Option<usize>, Vec<String>)>,
}

impl BomPanel {
    fn clear_pick(&mut self) {
        self.picked = None;
        self.target = None;
        self.fields.clear();
    }

    fn reconcile_pick(&mut self, rows: &[Value]) {
        let matches_target = |row: &Value| self.target.as_ref().is_some_and(|(ids, names)| &row["components"] == ids && &row["definition"] == names);
        if let Some(index) = self.picked
            && !rows.get(index).is_some_and(matches_target)
        {
            self.picked = rows.iter().position(matches_target);
            if self.picked.is_none() {
                self.clear_pick();
            }
        }
    }

    /// Show why Apply was refused, for as long as the design, the row and its fields stay as they are.
    fn refuse(&mut self, revision: u64, error: String) {
        self.refused = Some((revision, self.picked, self.fields.clone()));
        self.error = Some(error);
    }

    fn params(&self) -> Value {
        json!({"levels": if self.top { "top" } else { "flat" }, "group_by": [if self.by_definition { "definition" } else { "part_number" }],
            "group_children": self.group_children})
    }

    /// The bill of materials for the design as it is now (computed again only after a change).
    fn rows(&mut self, app: &mut SolveApp) -> Rc<Value> {
        let revision = app.session.revision;
        let key = [self.top, self.by_definition, self.group_children];
        if let Some((r, k, v)) = &self.cache
            && (*r, *k) == (revision, key)
        {
            return v.clone();
        }
        let v = match app.session.execute("assembly.bom", &self.params()) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.to_string());
                json!({"rows": [], "total_occurrences": 0})
            }
        };
        let v = Rc::new(v);
        self.cache = Some((revision, key, v.clone()));
        v
    }
}

pub(crate) const TITLE_KEY: &str = "cmd.assembly_bom";

/// Open the report for the current document.
pub fn open(app: &mut SolveApp) {
    app.bom = Some(BomPanel { document: app.session.document_id.clone(), focus_pending: true, ..BomPanel::default() });
}

pub(crate) fn available(app: &SolveApp) -> bool {
    app.bom.as_ref().is_some_and(|d| d.document == app.session.document_id)
}

pub(crate) fn focus_pending(app: &SolveApp) -> bool {
    app.bom.as_ref().is_some_and(|d| d.document == app.session.document_id && d.focus_pending)
}

pub(crate) fn set_focus_pending(app: &mut SolveApp, pending: bool) {
    if let Some(d) = &mut app.bom
        && d.document == app.session.document_id
    {
        d.focus_pending = pending;
    }
}

fn text(row: &Value, k: &str) -> String {
    match &row[k] {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// The grid's headers and the row key each column shows.
const HEADERS: [&str; 6] = ["Item", "Part number", "Description", "Rev", "Qty", "Definition"];
const COLUMN_KEYS: [&str; 6] = ["item", "part_number", "description", "revision", "count", "definition"];
/// The column that takes the space the others leave, truncating its text.
const DESCRIPTION: usize = 2;
const COLUMN_SPACING: f32 = 14.0;

/// Each column's width: every column fits its widest cell, Description takes what is left.
fn column_widths(ui: &egui::Ui, rows: &[Value], scrollable: bool) -> [f32; 6] {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x.ceil();
    // The item cell is a picker button, padded on both sides.
    let padding = 2.0 * ui.spacing().button_padding.x;
    let mut widths = HEADERS.map(width);
    for row in rows.iter().flat_map(|r| std::iter::once(r).chain(r["children"].as_array().into_iter().flatten())) {
        for (i, key) in COLUMN_KEYS.iter().enumerate() {
            if i == DESCRIPTION && !scrollable {
                continue;
            }
            let w = width(&text(row, key)) + if i == 0 { padding } else { 0.0 };
            widths[i] = widths[i].max(w);
        }
    }
    let others: f32 = widths.iter().enumerate().filter(|(i, _)| *i != DESCRIPTION).map(|(_, w)| w).sum();
    let left = ui.available_width() - others - COLUMN_SPACING * (HEADERS.len() - 1) as f32;
    widths[DESCRIPTION] = left.max(widths[DESCRIPTION]);
    widths
}

/// One grid cell exactly `width` wide; text wider than that truncates.
fn cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(egui::vec2(width, crate::ui_kit::row_height()), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(width);
        add(ui)
    })
    .inner
}

/// The label of one of [`BOM_KEYS`].
fn field_label(key: &str) -> &str {
    metadata::key(key).map_or(key, |k| solidcraft_ui_text::tr(k.label))
}

/// Apply the picked row's fields to each of its definitions (one undo step each).
fn apply(app: &mut SolveApp, d: &mut BomPanel, row: &Value) {
    let mut params: Map<String, Value> = BOM_KEYS.iter().zip(&d.fields).map(|(k, v)| (k.to_string(), json!(v))).collect();
    for c in row["components"].as_array().into_iter().flatten() {
        params.insert("component".into(), c.clone());
        let r = app.run("component.metadata.set", Value::Object(params.clone()));
        if let Err(e) = r {
            d.refuse(app.session.revision, e);
            return;
        }
    }
    d.error = None;
    d.refused = None;
}

fn export(app: &mut SolveApp, d: &BomPanel, ext: &str) {
    let mut params = d.params();
    params["format"] = json!(ext);
    let document = d.document.clone();
    app.pick_save(&format!("bill-of-materials.{ext}"), &[ext], move |app, path| {
        if let Some(path) = path
            && app.session.document_id == document
        {
            params["path"] = json!(path);
            let _ = app.run("assembly.bom", params);
        }
    });
}

/// Draw the Classic report window when it is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = take_current(app) else { return };
    let closed = crate::ui_kit::Window::new(egui::Id::unique("sc_bom"), "bom", solidcraft_ui_text::tr(TITLE_KEY))
        .cards(3.0)
        .show_body(ctx, |ui| body(app, ui, &mut d, false));
    if !closed {
        app.bom = Some(d);
    }
}

/// Draw the same report inside the shared panel's body.
pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut d) = take_current(app) else { return };
    body(app, ui, &mut d, true);
    app.bom = Some(d);
}

fn take_current(app: &mut SolveApp) -> Option<BomPanel> {
    app.bom.take().filter(|d| d.document == app.session.document_id)
}

const REPORT_MAX_HEIGHT: f32 = 320.0;
const EDITOR_MAX_HEIGHT: f32 = 200.0;

fn body(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut BomPanel, shared: bool) {
    // A refused Apply belongs to the design, row and fields it ran with (#641).
    if d.refused.as_ref().is_some_and(|(rev, row, fields)| (*rev, *row, fields) != (app.session.revision, d.picked, &d.fields)) {
        d.error = None;
        d.refused = None;
    }
    let top = ui.cursor().top();
    let (footer_action, footer_height) = if shared {
        let selected = d.picked.is_some();
        let rows = if selected && ui.available_width() < 290.0 { 2 } else { 1 };
        let footer = crate::ui_kit::Panel::pinned_footer(ui, "bom-actions", rows, |ui| actions(ui, selected, true));
        (footer.inner, footer.response.rect.height())
    } else {
        (None, 0.0)
    };
    let available = ui.available_height().max(1.0);
    let (row, report_extent) = if shared {
        let max = if d.picked.is_some() || d.error.is_some() { available * 0.55 } else { available };
        let scroll = egui::ScrollArea::vertical()
            .id_salt("report")
            .min_scrolled_height(0.0)
            .max_height(max.min(REPORT_MAX_HEIGHT))
            .show(ui, |ui| report(app, ui, d, true));
        let (row, table_extra) = scroll.inner;
        (row, (scroll.content_size.y + table_extra).min(REPORT_MAX_HEIGHT))
    } else {
        report(app, ui, d, false)
    };

    let mut editor_extent = 0.0;
    if row.is_some() || d.error.is_some() {
        ui.separator();
        if shared {
            let max = ui.available_height().clamp(1.0, EDITOR_MAX_HEIGHT);
            let scroll = egui::ScrollArea::vertical()
                .id_salt("editor")
                .min_scrolled_height(0.0)
                .max_height(max)
                .show(ui, |ui| details(ui, d, row.is_some(), true));
            editor_extent = scroll.content_size.y.min(EDITOR_MAX_HEIGHT) + 2.0 * ui.spacing().item_spacing.y;
        } else {
            details(ui, d, row.is_some(), false);
        }
    }

    let action = if shared {
        footer_action
    } else {
        ui.separator();
        actions(ui, row.is_some(), false)
    };
    match action {
        Some(Action::Apply) => {
            if let Some(row) = &row {
                apply(app, d, row);
            }
        }
        Some(Action::Export(ext)) => export(app, d, ext),
        None => {}
    }
    if shared {
        let natural = report_extent + editor_extent + footer_height + ui.spacing().item_spacing.y;
        crate::panel_runtime::report_content_extent(ui, top, natural, false);
    }
}

enum Action {
    Apply,
    Export(&'static str),
}

fn actions(ui: &mut egui::Ui, selected: bool, shared: bool) -> Option<Action> {
    let buttons = |ui: &mut egui::Ui| {
        let mut action = None;
        if selected {
            let response = crate::ui_kit::button(ui, solidcraft_ui_text::tr_label("Apply"), crate::ui_kit::Button::Normal);
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
            if response.clicked() {
                action = Some(Action::Apply);
            }
        }
        for (label, ext) in [("Export CSV…", "csv"), ("Export JSON…", "json")] {
            let response = crate::ui_kit::button(ui, solidcraft_ui_text::tr_label(label), crate::ui_kit::Button::Normal);
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
            if response.clicked() {
                action = Some(Action::Export(ext));
            }
        }
        action
    };
    if shared { ui.horizontal_wrapped(buttons).inner } else { ui.horizontal(buttons).inner }
}

fn options(ui: &mut egui::Ui, d: &mut BomPanel, shared: bool) {
    let was = (d.top, d.by_definition, d.group_children);
    let available = ui.available_width();
    ui.horizontal_wrapped(|ui| {
        for (option, labels) in [(&mut d.top, ["Parts only", "Top level"]), (&mut d.by_definition, ["By part number", "By definition"])] {
            let width = labels.iter().map(|label| crate::ui_kit::ContentWidth::button(ui, label)).fold(0.0, f32::max) * 2.0;
            let width = if shared { width.min(available) } else { width };
            let mut selected = usize::from(*option);
            ui.allocate_ui(egui::vec2(width, crate::ui_kit::row_height()), |ui| {
                crate::ui_kit::segmented(ui, &mut selected, &labels, Default::default());
            });
            *option = selected == 1;
            ui.separator();
        }
        if !shared {
            crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("bom.group_children"), &mut d.group_children);
        }
    });
    if shared {
        ui.scope(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            let response = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("bom.group_children"), &mut d.group_children);
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
        });
    }
    if was != (d.top, d.by_definition, d.group_children) {
        d.clear_pick();
    }
}

fn report(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut BomPanel, shared: bool) -> (Option<Value>, f32) {
    options(ui, d, shared);
    let bom = d.rows(app);
    let rows = bom["rows"].as_array().map_or(&[][..], Vec::as_slice);
    d.reconcile_pick(rows);
    ui.separator();
    let widths = column_widths(ui, rows, shared);
    let mut grid = |ui: &mut egui::Ui| table(ui, d, rows, widths);
    let t = Tokens::get();
    let table_extra = if shared {
        let summary = crate::ui_kit::row_height() + ui.spacing().item_spacing.y;
        let scroll = egui::ScrollArea::both().id_salt("table").min_scrolled_height(0.0).max_height((ui.available_height() - summary).max(0.0)).show(
            ui,
            |ui| {
                grid(ui);
                ui.add_space(t.space_8);
            },
        );
        (scroll.content_size.y.min(REPORT_MAX_HEIGHT) - scroll.inner_rect.height()).max(0.0)
    } else {
        egui::ScrollArea::vertical().id_salt("table").max_height(REPORT_MAX_HEIGHT).show(ui, grid);
        0.0
    };
    ui.label(RichText::new(format!("{} rows, {} placed", rows.len(), text(&bom, "total_occurrences"))).small().color(t.text_dim));
    (d.picked.and_then(|i| rows.get(i)).cloned(), table_extra)
}

fn table(ui: &mut egui::Ui, d: &mut BomPanel, rows: &[Value], widths: [f32; 6]) {
    let t = Tokens::get();
    // The item cell (a picker for a row, dim text for a sub-part) is drawn by the caller.
    let rest = |ui: &mut egui::Ui, row: &Value| {
        for (i, key) in COLUMN_KEYS.iter().enumerate().skip(1) {
            let dim = *key == "definition";
            cell(ui, widths[i], |ui| ui.label(if dim { RichText::new(text(row, key)).color(t.text_dim) } else { RichText::new(text(row, key)) }));
        }
        ui.end_row();
    };
    // No minimum column width: padding a narrow column would push the grid wider.
    egui::Grid::new("sc_bom_grid").num_columns(6).striped(true).min_col_width(0.0).spacing([COLUMN_SPACING, 4.0]).show(ui, |ui| {
        for (h, w) in HEADERS.iter().zip(widths) {
            cell(ui, w, |ui| ui.label(RichText::new(*h).strong().color(t.text_dim)));
        }
        ui.end_row();
        for (i, row) in rows.iter().enumerate() {
            let picked = d.picked == Some(i);
            let response = cell(ui, widths[0], |ui| crate::ui_kit::list_picker(ui, &i.to_string(), &text(row, "item"), picked));
            if picked || response.has_focus() {
                crate::panel_runtime::register_content_focus(ui.ctx(), &response);
            }
            if response.clicked() {
                if picked {
                    d.clear_pick();
                } else {
                    d.picked = Some(i);
                    d.target = Some((row["components"].clone(), row["definition"].clone()));
                    d.fields = BOM_KEYS.iter().map(|k| text(row, k)).collect();
                }
            }
            rest(ui, row);
            for c in row["children"].as_array().into_iter().flatten() {
                cell(ui, widths[0], |ui| ui.label(RichText::new(text(c, "item")).color(t.text_dim)));
                rest(ui, c);
            }
        }
    });
}

fn details(ui: &mut egui::Ui, d: &mut BomPanel, selected: bool, shared: bool) {
    if selected {
        let mut editor = |ui: &mut egui::Ui| {
            for (key, field) in BOM_KEYS.iter().zip(d.fields.iter_mut()) {
                let label = field_label(key);
                let name = ui.label(label);
                if shared {
                    let response = crate::ui_kit::cell_editor(ui, ui.make_persistent_id(key), field, label, false).labelled_by(name.id);
                    crate::panel_runtime::register_content_focus(ui.ctx(), &response);
                } else {
                    ui.allocate_ui(egui::vec2(280.0, crate::ui_kit::row_height()), |ui| {
                        crate::ui_kit::cell_editor(ui, ui.make_persistent_id(key), field, label, false).labelled_by(name.id);
                    });
                    ui.end_row();
                }
            }
        };
        if shared {
            editor(ui);
        } else {
            egui::Grid::new("sc_bom_edit").num_columns(2).spacing([12.0, 6.0]).show(ui, editor);
        }
    }
    if let Some(e) = &d.error {
        ui.label(RichText::new(e).color(Tokens::get().error));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

    #[test]
    fn the_window_lists_counts_and_edits_a_part_number() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]})).unwrap();
        h.step(&json!({"start": "assembly.bom"})).unwrap();
        assert!(h.app.bom.is_some(), "ui.start opens the window");
        h.frames(2);
        assert!(h.widget("Export CSV…").is_some());
        assert!(h.widget("1 rows, 2 placed").is_some());
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget": "1"})).unwrap();
        let d = h.app.bom.as_mut().unwrap();
        assert_eq!(d.picked, Some(0));
        d.fields[0] = "PL-100".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        let m = h.app.session.execute("component.metadata", &json!({"component": "Plate"})).unwrap();
        assert_eq!(m["part_number"], "PL-100");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
        h.frames(2);
        assert!(h.widget("PL-100").is_some(), "the list follows the change");
    }

    #[test]
    fn a_refused_apply_keeps_its_message_until_the_row_or_its_fields_change() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.create", json!({"name": "Bracket"})).unwrap();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        // Plate placed three times and Bracket once, so no Qty cell reads "1" before row 1's item
        // number or "2" before row 2's.
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]})).unwrap();
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [100, 0, 0]})).unwrap();
        h.step(&json!({"start": "assembly.bom"})).unwrap();
        h.frames(2);
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget": "1"})).unwrap();
        h.app.bom.as_mut().unwrap().fields[0] = "PL\u{7}".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        let refused = h.app.bom.as_ref().unwrap().error.clone();
        assert!(refused.as_deref().is_some_and(|e| e.contains("control character")), "{refused:?}");
        assert_eq!(h.app.session.undo.len(), undo, "nothing applied");
        // Correcting the field takes the refusal away before Apply is clicked again.
        h.app.bom.as_mut().unwrap().fields[0] = "PL-100".into();
        h.frames(2);
        assert_eq!(h.app.bom.as_ref().unwrap().error, None, "the message was about the control character, which is gone");
        // The message is about the row and fields Apply ran with (#641): a change to another
        // field takes it away too, and the next Apply refuses again.
        h.app.bom.as_mut().unwrap().fields[0] = "PL\u{7}".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        assert_eq!(h.app.bom.as_ref().unwrap().error, refused);
        h.app.bom.as_mut().unwrap().fields[1] = "Plate, 3 mm".into();
        h.frames(2);
        assert_eq!(h.app.bom.as_ref().unwrap().error, None, "the fields differ from the refused ones");
        h.step(&json!({"widget": "Apply"})).unwrap();
        assert_eq!(h.app.bom.as_ref().unwrap().error, refused, "the same reason again");
        assert_eq!(h.app.session.undo.len(), undo, "nothing applied");
        // Picking another row takes it away: the message was about the Plate row.
        h.step(&json!({"widget": "2"})).unwrap();
        assert_eq!(h.app.bom.as_mut().unwrap().picked, Some(1));
        assert_eq!(h.app.bom.as_ref().unwrap().error, None, "another row is picked");
        h.step(&json!({"widget": "1"})).unwrap();
        h.app.bom.as_mut().unwrap().fields[0] = "PL\u{7}".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        assert_eq!(h.app.bom.as_ref().unwrap().error, refused);
        // So does a change to the design, whose rows the picked index counts: the message was
        // about the design Apply ran on.
        h.app.run("edit.undo", json!({})).unwrap();
        h.frames(2);
        assert_eq!(h.app.bom.as_ref().unwrap().error, None, "the design changed under the row");
    }

    #[test]
    fn a_picked_row_edits_each_bill_of_materials_key_under_its_label() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        // Two placed, so the Qty cell ("2") isn't mistaken for the row's item number ("1").
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]})).unwrap();
        h.app.run("component.metadata.set", json!({"component": "Plate", "part_number": "PL-100", "revision": "A"})).unwrap();
        h.step(&json!({"start": "assembly.bom"})).unwrap();
        h.frames(2);
        // The grid's own column says "Rev"; "Revision" is the edit field's label, from the table.
        assert!(h.widget("Revision").is_none());
        h.step(&json!({"widget": "1"})).unwrap();
        h.frames(2);
        assert!(h.widget("Revision").is_some());
        let d = h.app.bom.as_mut().unwrap();
        assert_eq!(d.fields, ["PL-100", "", "A"], "the row's fields, in the bill of materials' order");
        d.fields[1] = "Plate, 3 mm".into();
        d.fields[2] = "B".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        let m = h.app.session.execute("component.metadata", &json!({"component": "Plate"})).unwrap();
        assert_eq!((&m["part_number"], &m["description"], &m["revision"]), (&json!("PL-100"), &json!("Plate, 3 mm"), &json!("B")), "{m}");
    }

    /// `column_widths` for `rows` in a grid `width` wide, and the measured width of each text.
    fn widths_in(width: f32, rows: &[serde_json::Value], texts: &[&str]) -> ([f32; 6], Vec<f32>) {
        let ctx = egui::Context::default();
        let mut out = ([0.0; 6], Vec::new());
        // The first frame loads the fonts.
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.allocate_ui(egui::vec2(width, 100.0), |ui| {
                ui.set_max_width(width);
                let font = egui::TextStyle::Body.resolve(ui.style());
                let measure = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x;
                out = (super::column_widths(ui, rows, false), texts.iter().map(|s| measure(s)).collect());
            });
        })
        .textures_delta
        .clear();
        out
    }

    #[test]
    fn columns_fit_their_contents_and_the_description_takes_the_rest() {
        let short =
            [json!({"item": 1, "part_number": "ISO 4762 M5x16", "description": "Bolt", "revision": "A", "count": 12, "definition": "SHCS M5x16"})];
        let (w, text) = widths_in(800.0, &short, &["ISO 4762 M5x16", "SHCS M5x16"]);
        assert!(w[1] >= text[0] && w[5] >= text[1], "part number and definition are not cut: {w:?} {text:?}");
        let used = w.iter().sum::<f32>() + super::COLUMN_SPACING * 5.0;
        assert!((used - 800.0).abs() < 0.5, "the description fills the grid: {w:?} uses {used}");

        let long = [json!({"item": 1, "part_number": "P", "description": "Socket head cap screw ".repeat(20), "count": 1})];
        let (w, _) = widths_in(800.0, &long, &[]);
        assert!(w[2] < 800.0, "a long description truncates instead of widening the grid: {w:?}");

        let (w, header) = widths_in(10.0, &short, &["Description"]);
        assert!(w.iter().all(|x| x.is_finite() && *x >= 0.0), "{w:?}");
        assert!(w[2] >= header[0], "the description keeps its header in a narrow grid: {w:?}");
    }

    #[test]
    fn the_description_column_takes_the_window_width() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("component.metadata.set", json!({"component": "Plate", "part_number": "PL-100", "description": "Plate"})).unwrap();
        h.step(&json!({"start": "assembly.bom"})).unwrap();
        h.frames(2);
        let (Some(desc), Some(rev)) = (h.widget("Description"), h.widget("Rev")) else { panic!("the grid's headers are drawn") };
        // Before, every column but the last was as narrow as a button and the grid used a third of the window.
        let card = f64::from(solidcraft_theme::Tokens::get().form_card_width);
        assert!(rev[0] - desc[0] > card, "Description spreads out: it starts at {}, Rev at {}", desc[0], rev[0]);
    }

    #[test]
    fn sub_parts_show_under_their_part() {
        let mut h = Harness::new();
        let bolt = h.app.run("parts.insert", json!({"family": "hex_bolt", "size": "M6", "length": 30, "point": [0, 0, 0]})).unwrap();
        h.app.run("component.activate", json!({"component": bolt["component"]})).unwrap();
        h.app.run("parts.insert", json!({"family": "hex_nut", "size": "M6", "point": [0, 0, -4]})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.step(&json!({"start": "assembly.bom"})).unwrap();
        h.frames(2);
        assert!(h.widget("2 rows, 2 placed").is_some());
        h.step(&json!({"widget": "Sub-parts in their part's row"})).unwrap();
        h.frames(2);
        assert!(h.widget("1 rows, 2 placed").is_some());
        assert!(h.widget("1.1").is_some(), "the nut under its bolt");
    }
}
