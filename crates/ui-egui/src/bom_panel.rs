//! The Bill of Materials window: the rows of a bill of materials view (`solidcraft_doc::bom`:
//! flat or top level, grouped by part number or by definition, a part's sub-parts in its row or
//! not), with Export CSV… and Export JSON… (through `assembly.bom`). Picking a row edits its part
//! number, description and revision through one `component.metadata.set` per definition. The
//! toolbar's Bill of Materials (`ui.start` of `assembly.bom`) opens it.

use std::sync::Arc;

use egui::RichText;
use serde_json::{Value, json};
use solidcraft_engine::doc::bom::{self, Bom, Group, Request, Row, Tree, View};

use crate::SolveApp;
use crate::theme::Tokens;

/// The window's state; the rows come from the design.
#[derive(Default)]
pub struct BomPanel {
    /// Top level instead of flat.
    pub top: bool,
    /// One row per definition instead of per part number.
    pub by_definition: bool,
    /// A part with a part number keeps its sub-parts (a bolt's washers and nut) in its row.
    pub group_children: bool,
    /// The rows for (design revision, top, by_definition, group_children).
    cache: Option<(u64, [bool; 3], Arc<Bom>)>,
    /// The picked row's index, and its fields as edited.
    pub picked: Option<usize>,
    pub fields: [String; 3],
    /// The last refusal.
    pub error: Option<String>,
}

impl BomPanel {
    fn request(&self) -> Request {
        Request {
            view: if self.top { View::FirstLevel } else { View::PartsOnly },
            group: if self.by_definition { Group::Definition } else { Group::PartNumber },
            fold: self.group_children,
            ..Request::default()
        }
    }

    /// `assembly.bom`'s parameters for the same list (the exports write it through the command).
    fn params(&self) -> Value {
        let r = self.request();
        json!({"levels": r.view.as_str(), "group_by": [r.group.as_str()], "group_children": r.fold})
    }

    /// The bill of materials for the design as it is now (listed again only after a change).
    fn rows(&mut self, app: &SolveApp) -> Arc<Bom> {
        let revision = app.session.revision;
        let key = [self.top, self.by_definition, self.group_children];
        if let Some((r, k, list)) = &self.cache
            && (*r, *k) == (revision, key)
        {
            return list.clone();
        }
        let state = app.session.model.state();
        let list = match bom::list(&mut Tree::new(&app.session.doc, &state), &self.request()) {
            Ok(list) => Arc::new(list),
            Err(e) => {
                self.error = Some(e.to_string());
                Arc::new(Bom::default())
            }
        };
        self.cache = Some((revision, key, list.clone()));
        list
    }
}

/// Open the window.
pub fn open(app: &mut SolveApp) {
    app.bom = Some(BomPanel::default());
}

/// A row's cells after its item: part number, description, revision, quantity and definition.
fn cells(ui: &mut egui::Ui, t: &Tokens, row: &Row) {
    for text in [&row.part_number, &row.description, &row.revision] {
        ui.label(text.as_str());
    }
    ui.label(row.count.to_string());
    ui.label(RichText::new(row.names.join("; ")).color(t.text_dim));
}

/// Apply the picked row's fields to each of its definitions (one undo step each).
fn apply(app: &mut SolveApp, d: &mut BomPanel, components: &[u64]) {
    let [pn, desc, rev] = d.fields.clone();
    for c in components {
        let r = app.run("component.metadata.set", json!({"component": c, "part_number": pn, "description": desc, "revision": rev}));
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
    let list = d.rows(app);
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
                crate::ui_kit::checkbox(ui, "Sub-parts in their part's row", &mut d.group_children);
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
                    // Rows at the top can be picked; the rows under them follow, dimmed.
                    let mut top = 0;
                    for (item, row) in list.rows_deep() {
                        if item.len() == 1 {
                            let (i, picked) = (top, d.picked == Some(top));
                            top += 1;
                            if crate::ui_kit::list_picker(ui, &i.to_string(), &row.item.to_string(), picked).clicked() {
                                d.picked = if picked { None } else { Some(i) };
                                d.fields = [row.part_number.clone(), row.description.clone(), row.revision.clone()];
                            }
                        } else {
                            ui.label(RichText::new(bom::item_text(&item)).color(t.text_dim));
                        }
                        cells(ui, &t, row);
                        ui.end_row();
                    }
                });
            });
            ui.label(RichText::new(format!("{} rows, {} placed", list.rows.len(), list.total.occurrences)).small().color(t.text_dim));
            if let Some(row) = d.picked.and_then(|i| list.rows.get(i)) {
                ui.separator();
                egui::Grid::new("sc_bom_edit").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    for (label, field) in ["Part number", "Description", "Revision"].into_iter().zip(d.fields.iter_mut()) {
                        ui.label(label);
                        ui.allocate_ui(egui::vec2(280.0, crate::ui_kit::row_height()), |ui| {
                            crate::ui_kit::cell_editor(ui, ui.make_persistent_id(label), field, label, false);
                        });
                        ui.end_row();
                    }
                });
                if crate::ui_kit::button(ui, "Apply", crate::ui_kit::Button::Normal).clicked() {
                    apply_row = Some(row.components.clone());
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
    if let Some(components) = apply_row {
        apply(app, &mut d, &components);
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
