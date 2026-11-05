//! The Bill of Materials window: `assembly.bom`'s rows (flat or top level, grouped by part
//! number or by definition, a part's sub-parts in its row or not), with Export CSV… and Export
//! JSON…. Picking a row edits its part
//! number, description and revision (`metadata::BOM_KEYS`) through one `component.metadata.set`
//! per definition. The toolbar's Bill of Materials (`ui.start` of `assembly.bom`) opens it.

use egui::RichText;
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::metadata::{self, BOM_KEYS};

use crate::SolveApp;
use crate::theme::Tokens;

/// The window's state; the rows come from the engine.
#[derive(Default)]
pub struct BomPanel {
    /// Top level instead of flat.
    pub top: bool,
    /// One row per definition instead of per part number.
    pub by_definition: bool,
    /// A part with a part number keeps its sub-parts (a bolt's washers and nut) in its row.
    pub group_children: bool,
    /// The rows for (design revision, top, by_definition, group_children).
    cache: Option<(u64, [bool; 3], Value)>,
    /// The picked row's index, and its fields as edited (one for each of [`BOM_KEYS`]).
    pub picked: Option<usize>,
    pub fields: Vec<String>,
    /// The last refusal.
    pub error: Option<String>,
}

impl BomPanel {
    fn params(&self) -> Value {
        json!({"levels": if self.top { "top" } else { "flat" }, "group_by": [if self.by_definition { "definition" } else { "part_number" }],
            "group_children": self.group_children})
    }

    /// The bill of materials for the design as it is now (computed again only after a change).
    fn rows(&mut self, app: &mut SolveApp) -> Value {
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
        self.cache = Some((revision, key, v.clone()));
        v
    }
}

/// Open the window.
pub fn open(app: &mut SolveApp) {
    app.bom = Some(BomPanel::default());
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
fn column_widths(ui: &egui::Ui, rows: &[Value]) -> [f32; 6] {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x.ceil();
    // The item cell is a picker button, padded on both sides.
    let padding = 2.0 * ui.spacing().button_padding.x;
    let mut widths = HEADERS.map(width);
    for row in rows.iter().flat_map(|r| std::iter::once(r).chain(r["children"].as_array().into_iter().flatten())) {
        for (i, key) in COLUMN_KEYS.iter().enumerate() {
            let w = width(&text(row, key)) + if i == 0 { padding } else { 0.0 };
            widths[i] = widths[i].max(w);
        }
    }
    let others: f32 = widths.iter().enumerate().filter(|(i, _)| *i != DESCRIPTION).map(|(_, w)| w).sum();
    let left = ui.available_width() - others - COLUMN_SPACING * (HEADERS.len() - 1) as f32;
    widths[DESCRIPTION] = left.max(width(HEADERS[DESCRIPTION]));
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
    metadata::key(key).map_or(key, |k| crate::i18n::tr(k.label))
}

/// Apply the picked row's fields to each of its definitions (one undo step each).
fn apply(app: &mut SolveApp, d: &mut BomPanel, row: &Value) {
    let mut params: Map<String, Value> = BOM_KEYS.iter().zip(&d.fields).map(|(k, v)| (k.to_string(), json!(v))).collect();
    for c in row["components"].as_array().into_iter().flatten() {
        params.insert("component".into(), c.clone());
        let r = app.run("component.metadata.set", Value::Object(params.clone()));
        if let Err(e) = r {
            d.error = Some(e);
            return;
        }
    }
    d.error = None;
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.bom.take() else { return };
    let t = Tokens::get();
    let bom = d.rows(app);
    let rows: Vec<Value> = bom["rows"].as_array().cloned().unwrap_or_default();
    let (mut export, mut apply_row) = (None, None);
    let closed =
        crate::ui_kit::Window::new(egui::Id::unique("sc_bom"), "bom", crate::i18n::tr_label("Bill of Materials")).cards(3.0).show_body(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let was = (d.top, d.by_definition, d.group_children);
                let mut levels = usize::from(d.top);
                let levels_width =
                    crate::ui_kit::ContentWidth::button(ui, "Parts only").max(crate::ui_kit::ContentWidth::button(ui, "Top level")) * 2.0;
                ui.allocate_ui(egui::vec2(levels_width, crate::ui_kit::row_height()), |ui| {
                    crate::ui_kit::segmented(ui, &mut levels, &["Parts only", "Top level"], Default::default());
                });
                d.top = levels == 1;
                ui.separator();
                let mut grouping = usize::from(d.by_definition);
                let grouping_width =
                    crate::ui_kit::ContentWidth::button(ui, "By part number").max(crate::ui_kit::ContentWidth::button(ui, "By definition")) * 2.0;
                ui.allocate_ui(egui::vec2(grouping_width, crate::ui_kit::row_height()), |ui| {
                    crate::ui_kit::segmented(ui, &mut grouping, &["By part number", "By definition"], Default::default());
                });
                d.by_definition = grouping == 1;
                ui.separator();
                crate::ui_kit::checkbox(ui, crate::i18n::tr("bom.group_children"), &mut d.group_children);
                if was != (d.top, d.by_definition, d.group_children) {
                    d.picked = None;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                let widths = column_widths(ui, &rows);
                // The item cell (a picker for a row, dim text for a sub-part) is drawn by the caller.
                let rest = |ui: &mut egui::Ui, row: &Value| {
                    for (i, key) in COLUMN_KEYS.iter().enumerate().skip(1) {
                        let dim = *key == "definition";
                        cell(ui, widths[i], |ui| {
                            ui.label(if dim { RichText::new(text(row, key)).color(t.text_dim) } else { RichText::new(text(row, key)) })
                        });
                    }
                    ui.end_row();
                };
                // No minimum column width: the widths above add up to the grid's, so padding a
                // narrow column (Rev, Qty) would push the grid out of the window.
                egui::Grid::new("sc_bom_grid").num_columns(6).striped(true).min_col_width(0.0).spacing([COLUMN_SPACING, 4.0]).show(ui, |ui| {
                    for (h, w) in HEADERS.iter().zip(widths) {
                        cell(ui, w, |ui| ui.label(RichText::new(*h).strong().color(t.text_dim)));
                    }
                    ui.end_row();
                    for (i, row) in rows.iter().enumerate() {
                        let picked = d.picked == Some(i);
                        if cell(ui, widths[0], |ui| crate::ui_kit::list_picker(ui, &i.to_string(), &text(row, "item"), picked)).clicked() {
                            d.picked = if picked { None } else { Some(i) };
                            d.fields = BOM_KEYS.iter().map(|k| text(row, k)).collect();
                        }
                        rest(ui, row);
                        for c in row["children"].as_array().into_iter().flatten() {
                            cell(ui, widths[0], |ui| ui.label(RichText::new(text(c, "item")).color(t.text_dim)));
                            rest(ui, c);
                        }
                    }
                });
            });
            ui.label(RichText::new(format!("{} rows, {} placed", rows.len(), text(&bom, "total_occurrences"))).small().color(t.text_dim));
            if let Some(row) = d.picked.and_then(|i| rows.get(i)) {
                ui.separator();
                egui::Grid::new("sc_bom_edit").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    for (key, field) in BOM_KEYS.iter().zip(d.fields.iter_mut()) {
                        let label = field_label(key);
                        ui.label(label);
                        ui.allocate_ui(egui::vec2(280.0, crate::ui_kit::row_height()), |ui| {
                            crate::ui_kit::cell_editor(ui, ui.make_persistent_id(key), field, label, false);
                        });
                        ui.end_row();
                    }
                });
                if crate::ui_kit::button(ui, "Apply", crate::ui_kit::Button::Normal).clicked() {
                    apply_row = Some(row.clone());
                }
            }
            if let Some(e) = &d.error {
                ui.label(RichText::new(e).color(t.error));
            }
            ui.separator();
            ui.horizontal(|ui| {
                for (label, ext) in [("Export CSV…", "csv"), ("Export JSON…", "json")] {
                    if crate::ui_kit::button(ui, label, crate::ui_kit::Button::Normal).clicked() {
                        export = Some(ext);
                    }
                }
            });
        });
    if let Some(row) = apply_row {
        apply(app, &mut d, &row);
    }
    if let Some(ext) = export {
        let mut params = d.params();
        params["format"] = json!(ext);
        app.pick_save_command(&format!("bill-of-materials.{ext}"), &[ext], "assembly.bom", params);
    }
    if !closed {
        app.bom = Some(d);
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
                out = (super::column_widths(ui, rows), texts.iter().map(|s| measure(s)).collect());
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
        let card = f64::from(crate::theme::Tokens::get().form_card_width);
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
