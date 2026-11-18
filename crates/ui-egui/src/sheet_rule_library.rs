//! The Sheet Metal Rules manager's standard library and rule packs (#18304). Picking a material
//! and a gauge fills the open manager with the library's thickness, K-factor and bend radius, and
//! the material; as with a personal rule, nothing changes in the design until the manager's OK.
//! A rule pack (a JSON or CSV file of rules, `docs/formats/sheet-rule-packs.md`) is imported
//! from a local file into the personal rules, and the personal rules export as one.

use solidcraft_engine::doc::Document;
use solidcraft_engine::doc::sheet_library::{self as library, PackRule, RulePack};
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
    /// The gauge picked of it (its library rule's name), or empty.
    picked: String,
    request: Option<Request>,
}

fn panel_id() -> egui::Id {
    egui::Id::unique("sheet_rule_library_panel")
}

/// A gauge as its row in the menu reads: `16 ga  0.0508 in`.
fn gauge_label(r: &PackRule) -> String {
    format!("{}  {}", r.gauge, r.rule.thickness)
}

/// Fill the manager with a library rule: its thickness, K-factor, bend radius and material,
/// under its name (`Aluminium 5052-H32, 14 ga`): the design's rule of that name when it has one,
/// else a new rule, as a personal rule is recalled. The reliefs, gaps and angle display stay as
/// the manager shows them.
fn fill(sm: &mut Sm, entry: &PackRule, doc: &Document) {
    let Sm::Rules { pick, name, values, material, active, loaded, .. } = sm else { return };
    for (index, (key, _)) in RULE_FIELDS.iter().enumerate() {
        let text = match *key {
            "thickness" => crate::length_input::stored(doc, &entry.rule.thickness),
            "k_factor" => entry.rule.k_factor.clone(),
            "bend_radius" => crate::length_input::stored(doc, &entry.rule.bend_radius),
            _ => continue,
        };
        if let Some(slot) = values.get_mut(index) {
            *slot = text;
        }
    }
    material.clone_from(&entry.rule.material);
    let rules = design_rules(doc);
    *pick = rules.iter().position(|r| r.name == entry.rule.name).unwrap_or(rules.len());
    name.clone_from(&entry.rule.name);
    *active = doc.sheet_rule(None).name == entry.rule.name || *pick >= rules.len();
    // The manager loads a rule's values only when its Rule box changes: these are now its own.
    *loaded = Some(*pick);
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
            if *material != state.material {
                state.picked.clear();
            }
            state.material.clone_from(material);
        }
        if !state.material.is_empty() {
            let gauges = library::standard_gauges(&state.material);
            let labels: Vec<String> = gauges.iter().map(|r| gauge_label(r)).collect();
            // The box shows the gauge alone; its menu gives each one's thickness too.
            let display =
                gauges.iter().find(|r| r.rule.name == state.picked).map_or_else(|| tr("sheet.library.select_gauge").to_string(), |r| r.gauge.clone());
            let mut chosen = None;
            picker(ui, "sheet_library_gauge", "sheet.library.gauge", &display, &labels, &mut chosen);
            if let Some(entry) = chosen.and_then(|index| gauges.get(index)) {
                state.picked.clone_from(&entry.rule.name);
                fill(sm, entry, doc);
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
    let rules = app.preferences.personal_sheet_rules.sheet_rules();
    let count = rules.len();
    let pack = RulePack {
        rules: rules.into_iter().map(|rule| PackRule { rule, gauge: String::new(), sources: Vec::new() }).collect(),
        ..RulePack::default()
    };
    let text = if path.to_lowercase().ends_with(".csv") { library::write_csv(&pack) } else { library::write_json(&pack) };
    match solidcraft_engine::io::vfs::write(path, text.as_bytes()) {
        Ok(()) => (tr("sheet.library.exported").replace("{count}", &count.to_string()), false),
        Err(e) => (format!("{}: {e}", tr("sheet.library.export_failed")), true),
    }
}

#[cfg(test)]
#[path = "sheet_rule_library_tests.rs"]
mod tests;
