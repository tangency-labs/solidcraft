//! The Bill of Materials panel: `assembly.bom`'s rows (flat or top level, grouped by part
//! number or by definition, a part's sub-parts in its row or not), with Export CSV… and Export
//! JSON…. Picking a row edits its part
//! number, description and revision (`metadata::BOM_KEYS`), a typed-in material and the finish (with a
//! list of common finishes) through one `component.metadata.set`
//! per definition. The toolbar's Bill of Materials (`ui.start` of `assembly.bom`) opens it.

use std::collections::BTreeMap;
use std::rc::Rc;

use egui::RichText;
use serde_json::{Map, Value, json};
use solidcraft_engine::doc::metadata::{self, BOM_KEYS, FINISH, MATERIAL};

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;
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
    /// The picked row's index, its fields as edited (one for each of [`edit_keys`]), and what each of
    /// its components holds as far as Apply knows: the row's fields when it was picked, then what
    /// Apply last wrote to that component (by component id).
    pub picked: Option<usize>,
    pub fields: Vec<String>,
    opened: BTreeMap<String, Vec<String>>,
    /// The selected definitions, whose row index can change when the table refreshes.
    target: Option<(Value, Value)>,
    /// The last refusal.
    pub error: Option<String>,
    /// The design revision, the picked row and its fields when Apply was refused: `error` stays
    /// only while they still are.
    refused: Option<(u64, Option<usize>, Vec<String>)>,
}

impl BomPanel {
    /// Pick row `i` (as a click on its item does): what it lists is its target, its fields what it says.
    fn pick_row(&mut self, i: usize, row: &Value) {
        self.picked = Some(i);
        self.target = Some((row["components"].clone(), row["definition"].clone()));
        self.fields = opened_fields(row);
        self.opened = row["components"].as_array().into_iter().flatten().map(|c| (c.to_string(), self.fields.clone())).collect();
    }

    fn clear_pick(&mut self) {
        self.picked = None;
        self.target = None;
        self.fields.clear();
        self.opened.clear();
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

/// The value columns after the item picker.
const COLUMN_KEYS: [&str; 7] = ["part_number", "description", "count", "revision", "definition", MATERIAL, FINISH];

/// The keys a picked row edits, in the order of [`BomPanel::fields`]: [`BOM_KEYS`], then the static
/// material and the finish.
fn edit_keys() -> impl Iterator<Item = &'static str> {
    BOM_KEYS.iter().copied().chain([MATERIAL, FINISH])
}
/// How many of [`edit_keys`] are [`BOM_KEYS`]: Apply always sends those, and the material and the
/// finish only when they were changed.
const ALWAYS: usize = BOM_KEYS.len();

/// The label of one of [`edit_keys`].
fn field_label(key: &str) -> &str {
    metadata::key(key).map_or(key, |k| solidcraft_ui_text::tr(k.label))
}

/// The fields a row opens with: its part properties, then its static material (what the row says
/// only when typed in: a material worked out from the bodies is not offered as text to apply) and
/// its finish.
fn opened_fields(row: &Value) -> Vec<String> {
    let typed = row["static"].as_array().is_some_and(|keys| keys.iter().any(|k| k == MATERIAL));
    edit_keys().map(|k| if k == MATERIAL && !typed { String::new() } else { text(row, k) }).collect()
}

/// Apply the picked row's fields to each of its definitions (one undo step each). The design is
/// the part of a design that places nothing (component 0): its properties are the design's own.
///
/// What it sends for a component is what differs from what that component holds as far as Apply
/// knows ([`BomPanel::opened`]: the part properties always, material and finish only when changed, so
/// a worked-out material is never turned into typed text). Each successful write updates that
/// component's baseline, a refused one leaves it, so a value typed back to what the row opened with
/// is still written after another was applied.
fn apply(app: &mut SolveApp, d: &mut BomPanel, row: &Value) {
    for c in row["components"].as_array().into_iter().flatten() {
        let known = d.opened.get(&c.to_string());
        let sent = |at: usize, now: &String| at < ALWAYS || known.and_then(|k| k.get(at)) != Some(now);
        let params: Map<String, Value> =
            edit_keys().zip(&d.fields).enumerate().filter(|(at, (_, v))| sent(*at, v)).map(|(_, (k, v))| (k.to_string(), json!(v))).collect();
        let r = if c.as_u64() == Some(0) {
            app.run("document.metadata.set", json!({"values": params}))
        } else {
            let mut each = params.clone();
            each.insert("component".into(), c.clone());
            app.run("component.metadata.set", Value::Object(each))
        };
        if let Err(e) = r {
            d.refuse(app.session.revision, e);
            return;
        }
        d.opened.insert(c.to_string(), d.fields.clone());
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

/// Draw the same report inside the shared panel's body.
pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut d) = take_current(app) else { return };
    body(app, ui, &mut d);
    app.bom = Some(d);
}

fn take_current(app: &mut SolveApp) -> Option<BomPanel> {
    app.bom.take().filter(|d| d.document == app.session.document_id)
}

const REPORT_MAX_HEIGHT: f32 = 320.0;
const EDITOR_MAX_HEIGHT: f32 = 200.0;

fn body(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut BomPanel) {
    // A refused Apply belongs to the design, row and fields it ran with (#641).
    if d.refused.as_ref().is_some_and(|(rev, row, fields)| (*rev, *row, fields) != (app.session.revision, d.picked, &d.fields)) {
        d.error = None;
        d.refused = None;
    }
    let top = ui.cursor().top();
    let selected = d.picked.is_some();
    let rows = if selected && ui.available_width() < 290.0 { 2 } else { 1 };
    let footer = crate::ui_kit::Panel::pinned_footer(ui, "bom-actions", rows, |ui| actions(ui, selected));
    let available = ui.available_height().max(1.0);
    let max = if selected || d.error.is_some() { available * 0.55 } else { available };
    let scroll = egui::ScrollArea::vertical()
        .id_salt("report")
        .min_scrolled_height(0.0)
        .max_height(max.min(REPORT_MAX_HEIGHT))
        .show(ui, |ui| report(app, ui, d));
    let row = scroll.inner;
    let report_extent = scroll.content_size.y.min(REPORT_MAX_HEIGHT);

    let mut editor_extent = 0.0;
    if row.is_some() || d.error.is_some() {
        ui.separator();
        let max = ui.available_height().clamp(1.0, EDITOR_MAX_HEIGHT);
        let scroll =
            egui::ScrollArea::vertical().id_salt("editor").min_scrolled_height(0.0).max_height(max).show(ui, |ui| details(ui, d, row.is_some()));
        editor_extent = scroll.content_size.y.min(EDITOR_MAX_HEIGHT) + 2.0 * ui.spacing().item_spacing.y;
    }

    match footer.inner {
        Some(Action::Apply) => {
            if let Some(row) = &row {
                apply(app, d, row);
            }
        }
        Some(Action::Export(ext)) => export(app, d, ext),
        None => {}
    }
    let natural = report_extent + editor_extent + footer.response.rect.height() + ui.spacing().item_spacing.y;
    crate::panel_runtime::report_content_extent(ui, top, natural, false);
}

enum Action {
    Apply,
    Export(&'static str),
}

fn actions(ui: &mut egui::Ui, selected: bool) -> Option<Action> {
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
            let response = ui.push_id(ext, |ui| crate::ui_kit::button(ui, solidcraft_ui_text::tr_label(label), crate::ui_kit::Button::Normal)).inner;
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
            if response.clicked() {
                action = Some(Action::Export(ext));
            }
        }
        action
    };
    ui.horizontal_wrapped(buttons).inner
}

fn options(ui: &mut egui::Ui, d: &mut BomPanel) {
    let was = (d.top, d.by_definition, d.group_children);
    let available = ui.available_width();
    ui.horizontal_wrapped(|ui| {
        for (key, option, labels) in
            [("bom:levels", &mut d.top, ["Parts only", "Top level"]), ("bom:group-by", &mut d.by_definition, ["By part number", "By definition"])]
        {
            let width = ui_kit::ContentWidth::dropdown(ui, &labels).min(available);
            let mut selected = usize::from(*option);
            let response = ui.allocate_ui(egui::vec2(width, ui_kit::row_height()), |ui| ui_kit::dropdown(ui, key, &mut selected, &labels)).inner;
            solidcraft_ui_registry::publish_target(key, response.rect);
            *option = selected == 1;
            ui.separator();
        }
    });
    ui.push_id("bom-group-children", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        let response = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("bom.group_children"), &mut d.group_children);
        crate::panel_runtime::register_content_focus(ui.ctx(), &response);
    });
    if was != (d.top, d.by_definition, d.group_children) {
        d.clear_pick();
    }
}

fn report(app: &mut SolveApp, ui: &mut egui::Ui, d: &mut BomPanel) -> Option<Value> {
    options(ui, d);
    let bom = d.rows(app);
    let rows = bom["rows"].as_array().map_or(&[][..], Vec::as_slice);
    d.reconcile_pick(rows);
    ui.separator();
    table(ui, d, rows);
    let summary = solidcraft_ui_text::format("bom.summary", &[("rows", &rows.len().to_string()), ("placed", &text(&bom, "total_occurrences"))]);
    ui_kit::text(ui, &summary, ui_kit::Text::Note("bom-summary"));
    d.picked.and_then(|i| rows.get(i)).cloned()
}

fn table(ui: &mut egui::Ui, d: &mut BomPanel, rows: &[Value]) {
    let t = Tokens::get();
    let available = ui.available_width();
    let (mut item_width, mut part_width, mut qty_width) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (row, picker) in
        rows.iter().flat_map(|row| std::iter::once((row, true)).chain(row["children"].as_array().into_iter().flatten().map(|child| (child, false))))
    {
        item_width =
            item_width.max(ui_kit::ContentWidth::monospace(ui, &text(row, "item")) + if picker { ui.spacing().button_padding.x * 2.0 } else { 0.0 });
        part_width = part_width.max(ui_kit::ContentWidth::label(ui, &text(row, "part_number")));
        qty_width = qty_width.max(ui_kit::ContentWidth::monospace(ui, &text(row, "count")));
    }
    let mut columns = vec![
        ui_kit::Column::new("Item", 0.0).min_content_width(item_width.min(t.control_height * 2.0)),
        ui_kit::Column::new("Part number", 0.0).min_content_width(part_width.min(available * 0.3)),
        ui_kit::Column::new("Description", 1.0).flex().min_content_width(ui_kit::ContentWidth::label(ui, "Description") + t.space_4),
        ui_kit::Column::new("Qty", 0.0).min_content_width(qty_width.min(t.control_height * 2.0)),
        ui_kit::Column::new("Rev", 0.0),
        ui_kit::Column::new("Definition", 0.0),
        ui_kit::Column::new("Material", 0.0),
        ui_kit::Column::new("Finish", 0.0),
    ];
    let mut keys = COLUMN_KEYS.to_vec();
    // Keep the main BOM columns visible; optional details must not displace quantity.
    while columns.len() > 4 && ui_kit::table_min_width(ui, &columns) > available {
        let _ = columns.pop();
        let _ = keys.pop();
    }
    if ui_kit::table_min_width(ui, &columns) > available
        && let Some(part) = columns.get_mut(1)
    {
        *part = ui_kit::Column::new("Part number", 0.0).manual_width(Some(part_width.min(available * 0.3).max(t.control_height)));
    }
    if ui_kit::table_min_width(ui, &columns) > available
        && let Some(description) = columns.get_mut(2)
    {
        *description = ui_kit::Column::new("Description", 1.0).flex();
    }
    if columns.len() == 4 && ui_kit::table_min_width(ui, &columns) > available {
        columns.remove(1);
        keys.remove(0);
    }
    // Inline labels retain native text selection and copying; only parent items pick a part.
    let rest = |cells: &mut ui_kit::TableRow<'_>, row: &Value| {
        for key in &keys {
            cells.cell(|ui| {
                let mut value = text(row, key);
                if *key == "description" && value.is_empty() && !keys.contains(&"definition") {
                    value = text(row, "definition");
                }
                let layout =
                    if *key == "count" { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
                ui.with_layout(layout, |ui| ui_kit::text(ui, &value, if *key == "count" { ui_kit::Text::MonoInline } else { ui_kit::Text::Inline }));
            });
        }
    };
    ui_kit::table_custom(ui, "bom", &columns, solidcraft_ui_text::tr("manage.empty"), |ui, table| {
        table.headers(ui, |_, _| false);
        for (i, row) in rows.iter().enumerate() {
            let picked = d.picked == Some(i);
            table.row(ui, &i.to_string(), picked, |cells| {
                cells.cell(|ui| {
                    let item = text(row, "item");
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    let response = ui
                        .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui_kit::button(
                                ui,
                                RichText::new(&item).font(egui::FontId::monospace(t.font_body)),
                                if picked { ui_kit::Button::SmallSelected } else { ui_kit::Button::Small },
                            )
                        })
                        .inner
                        .backing_on_hover_text(&item);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), picked, &item));
                    solidcraft_ui_registry::publish_target(&format!("dialog:list:{i}"), response.rect);
                    if picked || response.has_focus() {
                        crate::panel_runtime::register_content_focus(ui.ctx(), &response);
                    }
                    if response.clicked() {
                        if picked {
                            d.clear_pick();
                        } else {
                            d.pick_row(i, row);
                        }
                    }
                });
                rest(cells, row);
            });
            for (child, c) in row["children"].as_array().into_iter().flatten().enumerate() {
                table.row(ui, &format!("{i}:child:{child}"), false, |cells| {
                    cells.cell(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui_kit::text(ui, &text(c, "item"), ui_kit::Text::MonoInline)
                        });
                    });
                    rest(cells, c);
                });
            }
        }
    });
}

fn details(ui: &mut egui::Ui, d: &mut BomPanel, selected: bool) {
    if selected {
        for (key, field) in edit_keys().zip(d.fields.iter_mut()) {
            let label = field_label(key);
            let name = ui.label(label);
            if key == FINISH {
                // The pick list sits beside the text, which keeps what is typed as it is.
                crate::finish_field::editor(ui, "bom_panel", field);
                continue;
            }
            let response = crate::ui_kit::cell_editor(ui, ui.make_persistent_id(key), field, label, false).labelled_by(name.id);
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
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
    use crate::ui_kit::test_support;

    fn open_report(h: &mut Harness) {
        h.call("ui.resize", json!({"width":1600,"height":1000}));
        h.step(&json!({"start":"assembly.bom"})).unwrap();
        h.frames(4);
        h.app
            .run_panel_command(
                "panel.resize",
                &json!({
                    "target":{"type":"instance","id":"bom"},"size":{"width":720,"height":400},"thickness_only":true
                }),
            )
            .unwrap();
        h.frames(4);
    }

    #[test]
    fn the_panel_lists_counts_and_edits_a_part_number() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]})).unwrap();
        open_report(&mut h);
        assert!(h.app.bom.is_some(), "ui.start opens the panel");
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
        // Select item buttons by their handles because quantity cells can show the same numbers.
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]})).unwrap();
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [100, 0, 0]})).unwrap();
        open_report(&mut h);
        h.frames(2);
        let undo = h.app.session.undo.len();
        h.step(&json!({"click": {"handle": "dialog:list:0"}})).unwrap();
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
        h.step(&json!({"click": {"handle": "dialog:list:1"}})).unwrap();
        assert_eq!(h.app.bom.as_mut().unwrap().picked, Some(1));
        assert_eq!(h.app.bom.as_ref().unwrap().error, None, "another row is picked");
        h.step(&json!({"click": {"handle": "dialog:list:0"}})).unwrap();
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
        open_report(&mut h);
        h.frames(2);
        // The report column says "Rev"; "Revision" is the edit field's label.
        assert!(h.widget("Revision").is_none());
        h.step(&json!({"widget": "1"})).unwrap();
        h.frames(2);
        assert!(h.widget("Revision").is_some());
        let d = h.app.bom.as_mut().unwrap();
        assert_eq!(d.fields, ["PL-100", "", "A", "", ""], "the row's fields, in the bill of materials' order, then material and finish");
        d.fields[1] = "Plate, 3 mm".into();
        d.fields[2] = "B".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        let m = h.app.session.execute("component.metadata", &json!({"component": "Plate"})).unwrap();
        assert_eq!((&m["part_number"], &m["description"], &m["revision"]), (&json!("PL-100"), &json!("Plate, 3 mm"), &json!("B")), "{m}");
    }

    /// A picked row edits its static material and its finish beside the bill of materials' keys: a
    /// material the row only works out from its bodies is not offered as typed text, so Apply never
    /// turns it into a static one.
    #[test]
    fn a_picked_row_edits_its_material_and_finish() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        h.app.run("occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]})).unwrap();
        open_report(&mut h);
        h.frames(2);
        h.step(&json!({"widget": "1"})).unwrap();
        h.frames(2);
        assert!(h.widget("Finish").is_some() && h.widget("Material").is_some(), "both are edit fields");
        let d = h.app.bom.as_mut().unwrap();
        assert_eq!(d.fields, ["", "", "", "", ""], "the worked-out material is not a typed one");
        d.fields[3] = "304, 2 mm".into();
        d.fields[4] = "Powder coat, RAL 9005, matt".into();
        h.frames(2);
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget": "Apply"})).unwrap();
        let m = h.app.session.execute("component.metadata", &json!({"component": "Plate"})).unwrap();
        assert_eq!((&m["material"], &m["finish"]), (&json!("304, 2 mm"), &json!("Powder coat, RAL 9005, matt")), "{m}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step");
        h.frames(2);
        assert!(h.widget("Powder coat, RAL 9005, matt").is_some(), "the list follows the change");
    }

    /// A design of one part is its own row, and the design holds what a part would: Apply on it sets
    /// the design's properties, not a component's.
    #[test]
    fn the_row_of_a_single_part_design_edits_the_design() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        open_report(&mut h);
        h.frames(2);
        // The Qty cell reads "1" too, so the row is picked as a click on its item would: no widget name tells them apart.
        let row = h.app.session.execute("assembly.bom", &json!({})).unwrap()["rows"][0].clone();
        assert_eq!(row["components"], json!([0]), "the design is the part: {row}");
        let d = h.app.bom.as_mut().unwrap();
        d.pick_row(0, &row);
        d.fields[0] = "SP-1".into();
        d.fields[3] = "CRS, 16 ga".into();
        d.fields[4] = "Zinc plate, clear".into();
        h.frames(2);
        h.step(&json!({"widget": "Apply"})).unwrap();
        assert_eq!(h.app.bom.as_ref().unwrap().error, None);
        let m = &h.app.session.doc.metadata;
        assert_eq!((m.part_number(), m.text("material"), m.text("finish")), (Some("SP-1"), Some("CRS, 16 ga"), Some("Zinc plate, clear")));
    }

    /// What Apply sent is what the design holds, so the next Apply compares with that, not with what the
    /// row had when it was picked: material B applied to a row that opened with A, then A typed back,
    /// puts A back. A refused Apply changes nothing the next one compares with.
    #[test]
    fn apply_compares_with_what_it_last_wrote_not_with_what_the_row_opened_with() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("document.metadata.set", json!({"values": {"material": "A", "finish": "Mill finish"}})).unwrap();
        open_report(&mut h);
        h.frames(2);
        let row = h.app.session.execute("assembly.bom", &json!({})).unwrap()["rows"][0].clone();
        h.app.bom.as_mut().unwrap().pick_row(0, &row);
        let material = |h: &Harness| h.app.session.doc.metadata.text("material").map(str::to_string);
        let set = |h: &mut Harness, text: &str| {
            h.app.bom.as_mut().unwrap().fields[3] = text.into();
            h.frames(2);
            h.step(&json!({"widget": "Apply"})).unwrap();
        };
        assert_eq!(h.app.bom.as_ref().unwrap().fields[3], "A");
        set(&mut h, "B");
        assert_eq!((material(&h), h.app.bom.as_ref().unwrap().error.clone()), (Some("B".into()), None));
        set(&mut h, "A");
        assert_eq!((material(&h), h.app.bom.as_ref().unwrap().error.clone()), (Some("A".into()), None), "A is written back");
        // A refused write leaves the design, and what the next Apply compares with, as they were.
        set(&mut h, &"x".repeat(5000));
        assert!(h.app.bom.as_ref().unwrap().error.is_some());
        assert_eq!(material(&h), Some("A".into()));
        set(&mut h, "B");
        assert_eq!((material(&h), h.app.bom.as_ref().unwrap().error.clone()), (Some("B".into()), None));
        set(&mut h, "A");
        assert_eq!(material(&h), Some("A".into()));
    }

    #[test]
    fn the_table_keeps_quantity_visible_and_hides_optional_columns_in_narrow_panels() {
        let h = Harness::new();
        let ctx = h.ctx.clone();
        let rows = [
            json!({"item": 1234, "part_number": "PL-100", "description": "Socket head cap screw ".repeat(20), "count": 12}),
            json!({"item": 4321, "part_number": "HG-200", "description": "Bolt", "count": 34}),
        ];
        for width in [720.0, 420.0, 280.0, 250.0, 240.0] {
            let mut painted = Vec::new();
            for _ in 0..2 {
                solidcraft_ui_registry::clear_handles();
                painted = test_support::raw_frame(&ctx, egui::RawInput::default(), |ui| {
                    ui.set_width(width);
                    super::table(ui, &mut super::BomPanel::default(), &rows);
                })
                .shapes;
            }
            let viewport = solidcraft_ui_registry::rect_of("dialog:table:bom:viewport").unwrap();
            let content = solidcraft_ui_registry::count_of("dialog:table:bom:content_width").unwrap() as f32;
            assert!(viewport.width() <= width, "the card stays inside the panel: {viewport:?}");
            assert!((content - viewport.width()).abs() <= 1.0, "all visible columns fit: {width}, {content}, {viewport:?}");
            if width <= 280.0 {
                assert!(solidcraft_ui_registry::rect_of("dialog:column:bom:Rev").is_none());
                assert!(solidcraft_ui_registry::rect_of("dialog:column:bom:Definition").is_none());
            }
            let item = solidcraft_ui_registry::rect_of("dialog:table:bom:0:cell:Item").unwrap();
            let picker = solidcraft_ui_registry::rect_of("dialog:list:0").unwrap();
            assert!(item.contains_rect(picker), "the item picker fits its cell");
            assert!((item.right() - picker.right()).abs() < 1.0, "item numbers align to the right");
            let qty = solidcraft_ui_registry::rect_of("dialog:table:bom:0:cell:Qty").unwrap();
            assert!(viewport.expand(1.0).contains_rect(qty), "quantity remains on screen at {width}: {qty:?}, {viewport:?}");
            let value = solidcraft_ui_registry::rect_of("dialog:inline:12").unwrap();
            assert!((qty.right() - value.right()).abs() < 1.0, "quantity is right aligned: {qty:?}, {value:?}");
            let other = solidcraft_ui_registry::rect_of("dialog:inline:34").unwrap();
            assert!((value.right() - other.right()).abs() < 1.0 && (value.width() - other.width()).abs() < 1.0, "quantities use tabular alignment");
            let mut numbers = 0;
            let mut truncated_description = false;
            for shape in &painted {
                if let egui::Shape::Text(text) = &shape.shape {
                    if text.galley.text() == "Description" && width >= 420.0 {
                        assert!(!text.galley.elided, "the Description header keeps its full label");
                    }
                    if text.galley.text().starts_with("Socket head cap screw") {
                        assert!(text.galley.elided, "long descriptions use an ellipsis, not viewport clipping");
                        truncated_description = true;
                    }
                    if matches!(text.galley.text(), "1234" | "4321" | "12" | "34") {
                        assert!(text.galley.job.sections.iter().all(|s| s.format.font_id.family == egui::FontFamily::Monospace));
                        numbers += 1;
                    }
                }
            }
            assert_eq!(numbers, 4, "both item numbers and quantities are painted at {width}");
            assert!(truncated_description, "the description is painted at {width}");
        }
    }

    #[test]
    fn several_rows_and_option_dropdowns_have_no_widget_id_clashes() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.ctx.options_mut(|options| options.warn_on_id_clash = true);
        h.app.run("component.create", json!({"name":"Plate"})).unwrap();
        h.app.run("solid.box", json!({"length":40,"width":20,"height":3})).unwrap();
        h.app
            .run(
                "component.metadata.set",
                json!({"component":"Plate","part_number":"PL-100","description":"Plate, 3 mm","material":"304, 2 mm","finish":"Brushed"}),
            )
            .unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        for x in [50, 100] {
            h.app.run("occurrence.copy", json!({"component":"Plate","translate":[x,0,0]})).unwrap();
        }
        h.app.run("component.create", json!({"name":"Hinge"})).unwrap();
        h.app.run("solid.box", json!({"length":10,"width":8,"height":4})).unwrap();
        h.app.run("component.metadata.set", json!({"component":"Hinge","part_number":"HG-200"})).unwrap();
        h.app.run("component.create", json!({"name":"Pin"})).unwrap();
        h.app.run("solid.box", json!({"length":2,"width":2,"height":8})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.call("ui.resize", json!({"width":1280,"height":720}));
        h.step(&json!({"start":"assembly.bom"})).unwrap();
        h.app.bom.as_mut().unwrap().group_children = true;
        for width in [280, 420, 560, 720, 900] {
            if width == 900 {
                h.call("ui.resize", json!({"width":1600,"height":720}));
                h.frames(2);
            }
            h.app
                .run_panel_command(
                    "panel.resize",
                    &json!({
                        "target":{"type":"instance","id":"bom"},"size":{"width":width,"height":500},"thickness_only":true
                    }),
                )
                .unwrap();
            h.frames(4);
            h.step(&json!({"click":{"handle":"dialog:list:0"}})).unwrap();
            h.frames(2);
            let warnings: Vec<_> = h
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().contains("use of ") && text.galley.text().contains(" ID ") => {
                        Some(text.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect();
            assert!(warnings.is_empty(), "panel width {width}: {warnings:?}");
            let tree = h.output.accesskit_update.as_ref().unwrap();
            let ids: Vec<_> = ["Parts only", "By part number"]
                .iter()
                .map(|value| {
                    tree.nodes
                        .iter()
                        .find(|(_, node)| node.role() == egui::Role::ComboBox && (node.label() == Some(*value) || node.value() == Some(*value)))
                        .map(|(id, _)| *id)
                        .unwrap()
                })
                .collect();
            assert_ne!(ids[0], ids[1], "the real option ComboBoxes have different native IDs");
            let cached = &h.app.bom.as_ref().unwrap().cache.as_ref().unwrap().2;
            assert_eq!(cached["rows"].as_array().unwrap().len(), 2);
            assert_eq!(cached["total_occurrences"], 5);
            let viewport = solidcraft_ui_registry::rect_of("dialog:table:bom:viewport").unwrap();
            let content = solidcraft_ui_registry::count_of("dialog:table:bom:content_width").unwrap() as f32;
            assert!((content - viewport.width()).abs() <= 1.0, "the whole panel needs no horizontal table scrolling at {width}");
            let section = h.app.universal_panel_frame().unwrap().layout.sections.iter().find(|section| section.instance.0 == "bom").unwrap();
            assert!((section.outer.size.width - width as f32).abs() <= 1.0, "the test exercises the requested whole-panel width {width}");
            let panel = crate::panel_runtime::egui_rect(section.outer).expand(1.0);
            assert!(panel.contains_rect(viewport), "the table stays inside the whole panel at {width}");
            for key in ["bom:levels", "bom:group-by", "dialog:note:bom-summary"] {
                let rect = solidcraft_ui_registry::rect_of(key).unwrap();
                assert!(panel.contains_rect(rect), "{key} stays inside the whole panel at {width}: {rect:?}, {panel:?}");
            }
            for label in ["Sub-parts in their part's row", "Export CSV…", "Export JSON…"] {
                let [x, y] = h.widget(label).unwrap();
                assert!(panel.contains(egui::pos2(x as f32, y as f32)), "{label} stays inside the whole panel at {width}");
            }
            let quantity = solidcraft_ui_registry::rect_of("dialog:column:bom:Qty").unwrap();
            assert!(viewport.expand(1.0).contains_rect(quantity), "quantity is visible at panel width {width}");
            let part = solidcraft_ui_registry::rect_of("dialog:column:bom:Part number").unwrap();
            assert!(viewport.expand(1.0).contains_rect(part), "part numbers are visible at panel width {width}");
            if width == 900 {
                let rows = cached["rows"].as_array().unwrap();
                let index = rows.iter().position(|row| row["part_number"] == "PL-100").unwrap();
                for (column, value) in [("Material", "304, 2 mm"), ("Finish", "Brushed")] {
                    let cell = solidcraft_ui_registry::rect_of(&format!("dialog:table:bom:{index}:cell:{column}")).unwrap();
                    let caption = solidcraft_ui_registry::rect_of(&format!("dialog:inline:{value}")).unwrap();
                    assert!(viewport.expand(1.0).contains_rect(cell) && cell.contains_rect(caption), "the wide table retains main's {column} column");
                }
            }
            if width == 280 {
                let rows = cached["rows"].as_array().unwrap();
                let index = rows.iter().position(|row| row["part_number"] == "HG-200").unwrap();
                let name = super::text(&rows[index], "definition");
                let caption = solidcraft_ui_registry::rect_of(&format!("dialog:inline:{name}")).unwrap();
                let description = solidcraft_ui_registry::rect_of(&format!("dialog:table:bom:{index}:cell:Description")).unwrap();
                assert!(description.contains_rect(caption), "a part with no description retains its name when Definition is hidden");
            }
        }
    }

    #[test]
    fn report_text_can_be_selected_and_copied_without_editing_a_part() {
        let mut h = Harness::new();
        h.app.run("component.create", json!({"name": "Plate"})).unwrap();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 3})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        let description = "Plate, 3 mm";
        h.app.run("component.metadata.set", json!({"component": "Plate", "description": description})).unwrap();
        open_report(&mut h);
        let undo = h.app.session.undo.len();
        for value in [description, "1"] {
            let rect = solidcraft_ui_registry::rect_of(&format!("dialog:inline:{value}")).unwrap();
            h.frame_with(test_support::pointer(rect.left_center(), true));
            h.frame_with(vec![egui::Event::PointerMoved(rect.right_center())]);
            h.frame_with(test_support::pointer(rect.right_center(), false));
            h.frame_with(vec![egui::Event::Copy]);
            assert!(h.output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == value)), "copy {value}");
        }
        assert_eq!(h.app.bom.as_ref().unwrap().picked, None);
        assert_eq!(h.app.session.undo.len(), undo);
    }

    #[test]
    fn sub_parts_show_under_their_part() {
        let mut h = Harness::new();
        let bolt = h.app.run("parts.insert", json!({"family": "hex_bolt", "size": "M6", "length": 30, "point": [0, 0, 0]})).unwrap();
        h.app.run("component.activate", json!({"component": bolt["component"]})).unwrap();
        h.app.run("parts.insert", json!({"family": "hex_nut", "size": "M6", "point": [0, 0, -4]})).unwrap();
        h.app.run("component.activate", json!({"component": 0})).unwrap();
        open_report(&mut h);
        h.frames(2);
        assert!(h.widget("2 rows, 2 placed").is_some());
        h.step(&json!({"widget": "Sub-parts in their part's row"})).unwrap();
        h.frames(2);
        assert!(h.widget("1 rows, 2 placed").is_some());
        assert!(h.widget("1.1").is_some(), "the nut under its bolt");
    }
}
