//! The Sheet Metal Rules manager's standard library and rule packs (#18304). Picking a material
//! and a gauge fills the open manager with the gauge's thickness and the material, and keeps the
//! rule's own working radius (the tooling's). The K-factor is the bend model's estimate for the
//! bend method picked at that R/T, labelled as an estimate, unless a pack gives a measured one,
//! and a radius under the material's published minimum is pointed out. As with a personal rule,
//! nothing changes in the design until the manager's OK.
//! A rule pack (a JSON or CSV file of rules, `docs/formats/sheet-rule-packs.md`) is imported
//! from a local file into the personal rules, and the personal rules export as one.

use solidcraft_engine::doc::Document;
use solidcraft_engine::doc::expr;
use solidcraft_engine::doc::sheet_library::{self as library, BendMethod, PackRule, RulePack};
use solidcraft_ui_text::tr;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::dialogs_sheet::{RULE_FIELDS, Sm, design_rules};
use crate::ui_kit::{self, Text};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Request {
    Import,
    Export,
}

#[derive(Clone, Default)]
struct PanelState {
    /// The material picked: its name, or empty for none.
    material: String,
    /// Which sizes the thickness menu lists: gauges (0) or metric thicknesses (1); until picked,
    /// the design's units decide.
    series: Option<usize>,
    /// The gauge picked of it (its library rule's name), or empty.
    picked: String,
    /// The bend method the K-factor estimate is for (index into `BendMethod::ALL`).
    method: usize,
    /// What the last pick estimated or found: the K-factor estimate, a radius under the minimum.
    notes: Vec<(String, bool)>,
    request: Option<Request>,
}

const METHOD_KEYS: [&str; 3] = ["sheet.library.method_air", "sheet.library.method_bottoming", "sheet.library.method_coining"];

fn panel_id() -> egui::Id {
    egui::Id::unique("sheet_rule_library_panel")
}

/// A gauge as its row in the menu reads: the gauge, whose chart it is, and its thickness.
fn gauge_label(r: &PackRule) -> String {
    let label = r.gauge_label();
    if label == r.rule.thickness { label } else { format!("{label}  {}", r.rule.thickness) }
}

const SERIES_KEYS: [&str; 2] = ["sheet.library.series_gauge", "sheet.library.series_metric"];

/// A row of the metric series (`1.5 mm`), not of a gauge chart.
fn is_metric(r: &PackRule) -> bool {
    r.gauge.ends_with(" mm")
}

fn slot<'a>(values: &'a mut [String], key: &str) -> Option<&'a mut String> {
    RULE_FIELDS.iter().position(|(k, _)| *k == key).and_then(|index| values.get_mut(index))
}

/// The manager's bend radius (mm) for a sheet `thickness` mm thick, worked out as the rule
/// would: `Thickness` in it is that thickness.
fn radius_mm(values: &[String], doc: &Document, thickness: f64) -> Option<f64> {
    let text = RULE_FIELDS.iter().position(|(k, _)| *k == "bend_radius").and_then(|index| values.get(index))?;
    let (vals, _) = doc.param_values();
    let unit = expr::length_unit_or_mm(&doc.units);
    let text = if expr::is_bare_number(text.trim()) { format!("{} {unit}", text.trim()) } else { text.clone() };
    let look = |n: &str| {
        if n == "Thickness" {
            Ok(expr::Value::length(thickness))
        } else {
            vals.get(n).copied().ok_or_else(|| solidcraft_engine::doc::DocError::Expr(format!("unknown parameter `{n}`")))
        }
    };
    expr::eval_with(&text, &look).and_then(|v| v.to_kind(expr::Kind::Length)).ok()
}

/// Fill the manager with a library pick: the gauge's thickness and the material, under the
/// pick's name (`Aluminium 5052-H32, 14 ga`): the design's rule of that name when it has one,
/// else a new rule, as a personal rule is recalled. The working radius, reliefs, gaps and angle
/// display stay as the manager shows them; a pack's own radius and K-factor are taken. Returns
/// the notes the section shows.
fn fill(sm: &mut Sm, entry: &PackRule, doc: &Document, method: BendMethod) -> Vec<(String, bool)> {
    let Sm::Rules { pick, name, values, material, active, loaded, .. } = sm else { return Vec::new() };
    if let Some(thickness) = slot(values, "thickness") {
        *thickness = crate::length_input::stored(doc, &entry.rule.thickness);
    }
    if entry.given.bend_radius
        && let Some(radius) = slot(values, "bend_radius")
    {
        *radius = crate::length_input::stored(doc, &entry.rule.bend_radius);
    }
    let notes = estimate(values, entry, doc, method);
    material.clone_from(&entry.rule.material);
    let rules = design_rules(doc);
    *pick = rules.iter().position(|r| r.name == entry.rule.name).unwrap_or(rules.len());
    name.clone_from(&entry.rule.name);
    *active = doc.sheet_rule(None).name == entry.rule.name || *pick >= rules.len();
    // The manager loads a rule's values only when its Rule box changes: these are now its own.
    *loaded = Some(*pick);
    notes
}

/// The K-factor for a pick: the pack's own, else the bend model's estimate for `method` at the
/// manager's R/T, set in the manager. Returns what to say about it, and about a radius under the
/// material's published minimum.
fn estimate(values: &mut [String], entry: &PackRule, doc: &Document, method: BendMethod) -> Vec<(String, bool)> {
    let mut notes = Vec::new();
    let no_parameters = std::collections::BTreeMap::new();
    let Ok(thickness) = doc.rule_values(&no_parameters, &entry.rule).map(|v| v.thickness) else { return notes };
    let radius = radius_mm(values, doc, thickness);
    if entry.given.k_factor {
        if let Some(k) = slot(values, "k_factor") {
            k.clone_from(&entry.rule.k_factor);
        }
    } else if let (Some(hardness), Some(r)) = (entry.hardness, radius) {
        let k = library::k_factor_estimate(method, hardness, r / thickness);
        if let Some(slot) = slot(values, "k_factor") {
            *slot = format!("{k:.2}");
        }
        notes.push((
            tr("sheet.library.k_estimate")
                .replace("{k}", &format!("{k:.2}"))
                .replace("{method}", tr(METHOD_KEYS.get(method_index(method)).copied().unwrap_or("")))
                .replace("{ratio}", &format!("{:.2}", r / thickness)),
            false,
        ));
    }
    if let (Some(min), Some(r)) = (entry.min_bend_radius_mm(thickness), radius)
        && r < min - 1e-9
    {
        let min = format!("{min:.2} mm");
        notes.push((tr("sheet.library.under_minimum").replace("{material}", &entry.rule.material).replace("{min}", &min), true));
    }
    notes
}

fn method_index(method: BendMethod) -> usize {
    BendMethod::ALL.iter().position(|m| *m == method).unwrap_or(0)
}

fn picker(ui: &mut egui::Ui, id: &str, key: &str, display: &str, options: &[String], selected: &mut Option<usize>) {
    crate::dialogs::form_row(ui, key, |ui| {
        let response = ui_kit::dropdown_menu(ui, id, display, |ui| {
            for (index, option) in options.iter().enumerate() {
                let row = ui_kit::list_picker(ui, &format!("{id}_{index}"), option, false);
                if row.clicked() {
                    *selected = Some(index);
                }
                crate::scenario::publish_handle(&format!("dialog:{key}:{option}"), row.rect.center());
            }
        });
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, ui.is_enabled(), tr(key));
            info.current_text_value = Some(display.to_string());
            info
        });
    });
}

/// One section above the Sheet Metal Rules manager's fields.
pub(crate) fn rows(ui: &mut egui::Ui, d: &mut Dialog, doc: &Document, can_export: bool, reset: bool) {
    let Kind::Sheet(sm @ Sm::Rules { .. }) = &mut d.kind else { return };
    let id = panel_id();
    let mut state = if reset { PanelState::default() } else { ui.ctx().data_mut(|data| data.get_temp::<PanelState>(id)).unwrap_or_default() };
    ui_kit::section(ui, "sheet_rule_library", "sheet.library.title", false, |ui| {
        ui_kit::text(ui, tr("sheet.library.scope"), Text::Report("sheet_library_scope", false));
        let materials: Vec<String> = library::standard_materials().into_iter().map(str::to_string).collect();
        let display = if state.material.is_empty() { tr("sheet.library.select_material").to_string() } else { state.material.clone() };
        let mut chosen = None;
        picker(ui, "sheet_library_material", "sheet.library.material", &display, &materials, &mut chosen);
        if let Some(material) = chosen.and_then(|index| materials.get(index)) {
            // A new material clears the pick and never keeps a gauge with another thickness:
            // only picking a thickness sets one.
            if *material != state.material {
                state.picked.clear();
                state.notes.clear();
            }
            state.material.clone_from(material);
        }
        if !state.material.is_empty() {
            // Gauges or metric thicknesses: an inch design lists gauges first, any other metric.
            let inches = matches!(doc.units.trim(), "in" | "inch" | "ft");
            let series = state.series.unwrap_or(if inches { 0 } else { 1 });
            let names: Vec<String> = SERIES_KEYS.iter().map(|k| tr(k).to_string()).collect();
            let mut chosen_series = None;
            picker(ui, "sheet_library_series", "sheet.library.series", names.get(series).map_or("", String::as_str), &names, &mut chosen_series);
            if let Some(index) = chosen_series {
                state.series = Some(index);
            }
            let series = state.series.unwrap_or(series);
            let gauges: Vec<&PackRule> = library::standard_gauges(&state.material).into_iter().filter(|r| is_metric(r) == (series == 1)).collect();
            let labels: Vec<String> = gauges.iter().map(|r| gauge_label(r)).collect();
            // The box shows the gauge alone; its menu gives each one's thickness too.
            let display =
                gauges.iter().find(|r| r.rule.name == state.picked).map_or_else(|| tr("sheet.library.select_gauge").to_string(), |r| r.gauge.clone());
            let mut chosen = None;
            picker(ui, "sheet_library_gauge", "sheet.library.gauge", &display, &labels, &mut chosen);
            let method = BendMethod::ALL.get(state.method).copied().unwrap_or_default();
            if let Some(entry) = chosen.and_then(|index| gauges.get(index)) {
                state.picked.clone_from(&entry.rule.name);
                state.notes = fill(sm, entry, doc, method);
            }
            let methods: Vec<String> = METHOD_KEYS.iter().map(|k| tr(k).to_string()).collect();
            let shown = methods.get(state.method).cloned().unwrap_or_default();
            let mut chosen = None;
            picker(ui, "sheet_library_method", "sheet.library.method", &shown, &methods, &mut chosen);
            if let Some(index) = chosen {
                state.method = index;
                // A method picked after the thickness estimates the K-factor again.
                if let (Some(entry), Sm::Rules { values, .. }) = (gauges.iter().find(|r| r.rule.name == state.picked), &mut *sm) {
                    state.notes = estimate(values, entry, doc, BendMethod::ALL.get(index).copied().unwrap_or_default());
                }
            }
            for (index, (note, warning)) in state.notes.iter().enumerate() {
                ui_kit::text(ui, note, Text::Report(&format!("sheet_library_note_{index}"), *warning));
            }
        }
        ui.horizontal_wrapped(|ui| {
            let mut button = |ui: &mut egui::Ui, action: &str, label: &str, enabled: bool, request: Request| {
                let response = ui_kit::button(ui, label, ui_kit::Button::Enabled(enabled));
                crate::scenario::publish_handle(&format!("dialog:sheet.library:{action}"), response.rect.center());
                if response.clicked() {
                    state.request = Some(request);
                }
            };
            button(ui, "import", tr("sheet.library.import"), true, Request::Import);
            button(ui, "export", tr("sheet.library.export"), can_export, Request::Export);
        });
        ui_kit::text(ui, tr("sheet.library.pack_scope"), Text::Report("sheet_library_pack_scope", false));
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}

/// The import or export a button of [`rows`] asked for, run once the dialog has drawn: it asks
/// for a local file and reports in the status line.
pub(crate) fn run_request(ctx: &egui::Context, app: &mut SolveApp) {
    let id = panel_id();
    let Some(request) = ctx.data_mut(|data| data.get_temp_mut_or_default::<PanelState>(id).request.take()) else { return };
    match request {
        Request::Import => app.pick_file(tr("sheet.library.pack_kind"), &["json", "csv"], |app, path| {
            if let Some(path) = path {
                let (message, error) = import(app, &path);
                app.set_status(message, error);
            }
        }),
        Request::Export => app.pick_save("sheet-rules.json", &["json", "csv"], |app, path| {
            if let Some(path) = path {
                let (message, error) = export(app, &path);
                app.set_status(message, error);
            }
        }),
    }
}

fn import(app: &mut SolveApp, path: &str) -> (String, bool) {
    let bytes = match solidcraft_engine::io::vfs::read(path, library::MAX_PACK_BYTES) {
        Ok(bytes) => bytes,
        Err(e) => return (format!("{}: {e}", tr("sheet.library.import_failed")), true),
    };
    let read = match library::read(&bytes) {
        Ok(read) => read,
        Err(e) => return (format!("{}: {e}", tr("sheet.library.import_failed")), true),
    };
    let rules: Vec<_> = read.pack.rules.iter().map(|r| r.rule.clone()).collect();
    let (added, left_out) = app.preferences.personal_sheet_rules.import(&rules);
    let mut message = tr("sheet.library.imported").replace("{count}", &added.to_string());
    if !left_out.is_empty() {
        message.push(' ');
        message.push_str(&tr("sheet.library.left_out").replace("{names}", &left_out.join(", ")));
    }
    if !read.skipped.is_empty() {
        message.push(' ');
        message.push_str(&tr("sheet.library.skipped").replace("{fields}", &read.skipped.join(", ")));
    }
    (message, false)
}

fn export(app: &mut SolveApp, path: &str) -> (String, bool) {
    let (rules, left_out) = app.preferences.personal_sheet_rules.sheet_rules();
    let count = rules.len();
    if count == 0 {
        // Every personal rule has values a newer version saved: nothing this version can write.
        return (tr("sheet.library.export_newer").replace("{names}", &left_out.join(", ")), true);
    }
    let pack = RulePack { rules: rules.into_iter().map(PackRule::of).collect(), ..RulePack::default() };
    let text = if path.to_lowercase().ends_with(".csv") { library::write_csv(&pack) } else { library::write_json(&pack) };
    match solidcraft_engine::io::vfs::write(path, text.as_bytes()) {
        Ok(()) => {
            let mut message = tr("sheet.library.exported").replace("{count}", &count.to_string());
            if !left_out.is_empty() {
                message.push(' ');
                message.push_str(&tr("sheet.library.export_newer").replace("{names}", &left_out.join(", ")));
            }
            (message, false)
        }
        Err(e) => (format!("{}: {e}", tr("sheet.library.export_failed")), true),
    }
}

#[cfg(test)]
#[path = "sheet_rule_library_tests.rs"]
mod tests;
