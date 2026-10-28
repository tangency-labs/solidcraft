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
        crate::ui_kit::Window::new(egui::Id::new("sc_bom"), "bom", crate::i18n::tr_label("Bill of Materials")).cards(3.0).show_body(ctx, |ui| {
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
                egui::Grid::new("sc_bom_grid").num_columns(6).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                    for h in ["Item", "Part number", "Description", "Rev", "Qty", "Definition"] {
                        ui.label(RichText::new(h).strong().color(t.text_dim));
                    }
                    ui.end_row();
                    for (i, row) in rows.iter().enumerate() {
                        let picked = d.picked == Some(i);
                        if crate::ui_kit::list_picker(ui, &i.to_string(), &text(row, "item"), picked).clicked() {
                            d.picked = if picked { None } else { Some(i) };
                            d.fields = BOM_KEYS.iter().map(|k| text(row, k)).collect();
                        }
                        ui.label(text(row, "part_number"));
                        ui.label(text(row, "description"));
                        ui.label(text(row, "revision"));
                        ui.label(text(row, "count"));
                        ui.label(RichText::new(text(row, "definition")).color(t.text_dim));
                        ui.end_row();
                        for c in row["children"].as_array().into_iter().flatten() {
                            ui.label(RichText::new(text(c, "item")).color(t.text_dim));
                            ui.label(text(c, "part_number"));
                            ui.label(text(c, "description"));
                            ui.label(text(c, "revision"));
                            ui.label(text(c, "count"));
                            ui.label(RichText::new(text(c, "definition")).color(t.text_dim));
                            ui.end_row();
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
