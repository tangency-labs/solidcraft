//! The Plugins menu, and the dialog each plugin entry gets from its input schema.
//!
//! The menu sits after File in the application bar (desktop builds only; the web build runs no
//! plugins). "Install plugin…" comes first, then one row per entry of each installed plugin,
//! labelled by the entry's `menu` slot when it declares one, else by its name. A plugin with more
//! than one entry gets a submenu named after the plugin.
//!
//! Choosing an entry opens a dialog built from its `input_schema`: a number field, a whole number
//! field, a checkbox, a dropdown or a text field per property. Parts of a schema this dialog does
//! not draw (selections, targets, nested objects) leave Run off with a note to run the entry from
//! the agent or the CLI, where the full schema is available.
//!
//! Run goes through the same `plugin.run` request the control channel sends, with the package's
//! content hash read when the dialog opened: the computation runs on its worker and comes back as
//! one undo step. Nothing here runs plugin code in any other way, and programmatic calls never
//! open this dialog.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;

/// The menu row that installs a plugin.
const INSTALL: &str = "install";

/// The menu's state, and the open entry dialog.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) open: bool,
    /// The pass the menu opened on: that click is not a click elsewhere.
    opened_pass: u64,
    /// The left edge of the Plugins button, where the menu opens.
    x: f32,
    /// The plugins listed when the menu last opened.
    pub(crate) listed: Vec<Listed>,
    pub(crate) dialog: Option<EntryDialog>,
}

/// An installed plugin, as its package read when the menu opened.
#[derive(Clone, Debug)]
pub(crate) struct Listed {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) path: String,
    /// The package's content hash, which `plugin.run` checks.
    pub(crate) approval: String,
    pub(crate) disabled: bool,
    /// The sensitive command groups the manifest asks for.
    pub(crate) groups: BTreeSet<String>,
    pub(crate) entries: Vec<solidcraft_engine::plugin::Entry>,
}

/// One property of an entry's input schema, as a field of the dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) tip: Option<String>,
    pub(crate) required: bool,
    pub(crate) kind: Kind,
}

/// What a field holds, with its schema limits.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Number {
        min: Option<f64>,
        max: Option<f64>,
        exclusive_min: bool,
        exclusive_max: bool,
        step: Option<f64>,
        unit: Option<String>,
    },
    Integer {
        min: Option<i64>,
        max: Option<i64>,
        unit: Option<String>,
    },
    Boolean,
    /// A string from a fixed list.
    Choice(Vec<String>),
    Text {
        min_length: Option<usize>,
        max_length: Option<usize>,
    },
}

/// A field's current value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Input {
    Number(f64),
    Integer(i64),
    /// A number or whole number with no default: typed, and empty until filled.
    Typed(String),
    Boolean(bool),
    /// The chosen option; `None` until one is chosen, when the schema gives no default (the
    /// dropdown then leads with "Choose…").
    Choice(Option<usize>),
    Text(String),
}

/// Why Run is off.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Problem {
    /// A required field is empty.
    Missing(String),
    /// A field's value is outside what its schema allows.
    Invalid(String),
}

/// The fields of an input schema, each with its default, or `None` when the schema has parts this
/// dialog does not draw (the entry is then run from the agent or the CLI).
pub(crate) fn form(schema: &Value) -> Option<Vec<(Field, Input)>> {
    let schema = schema.as_object()?;
    if schema.get("type").is_some_and(|t| t != "object") || ["$ref", "oneOf", "anyOf", "allOf", "if", "not"].iter().any(|k| schema.contains_key(*k)) {
        return None;
    }
    let required: Vec<&str> = match schema.get("required") {
        None => Vec::new(),
        Some(list) => list.as_array()?.iter().map(Value::as_str).collect::<Option<_>>()?,
    };
    let properties = match schema.get("properties") {
        None => return Some(Vec::new()),
        Some(p) => p.as_object()?,
    };
    if required.iter().any(|key| !properties.contains_key(*key)) {
        return None;
    }
    let mut fields = properties.iter().map(|(key, property)| field(key, property, required.contains(&key.as_str()))).collect::<Option<Vec<_>>>()?;
    // A JSON object has no order: the required fields come first in the order `required` names
    // them, then the rest by name.
    let rank = |key: &str| required.iter().position(|r| *r == key).unwrap_or(usize::MAX);
    fields.sort_by(|(a, _), (b, _)| rank(&a.key).cmp(&rank(&b.key)).then_with(|| a.key.cmp(&b.key)));
    Some(fields)
}

fn field(key: &str, property: &Value, required: bool) -> Option<(Field, Input)> {
    let p = property.as_object()?;
    if ["$ref", "oneOf", "anyOf", "allOf", "if", "not", "const", "items", "properties"].iter().any(|k| p.contains_key(*k)) {
        return None;
    }
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let number = |k: &str| p.get(k).and_then(Value::as_f64).filter(|n| n.is_finite());
    let unit = text("unit").or_else(|| text("x-unit")).or_else(|| text("units"));
    let default = p.get("default");
    let (kind, input) = match p.get("type")?.as_str()? {
        "number" if !p.contains_key("enum") => {
            // `exclusiveMinimum` as a number (draft 6+); a boolean beside `minimum` (draft 4).
            let (min, exclusive_min) = match (number("exclusiveMinimum"), number("minimum")) {
                (Some(e), Some(m)) if m > e => (Some(m), false),
                (Some(e), _) => (Some(e), true),
                (None, m) => (m, p.get("exclusiveMinimum") == Some(&Value::Bool(true))),
            };
            let (max, exclusive_max) = match (number("exclusiveMaximum"), number("maximum")) {
                (Some(e), Some(m)) if m < e => (Some(m), false),
                (Some(e), _) => (Some(e), true),
                (None, m) => (m, p.get("exclusiveMaximum") == Some(&Value::Bool(true))),
            };
            let step = number("multipleOf").filter(|s| *s > 0.0);
            let input = match default.and_then(Value::as_f64).filter(|n| n.is_finite()) {
                Some(n) => Input::Number(n),
                None => Input::Typed(String::new()),
            };
            (Kind::Number { min, max, exclusive_min, exclusive_max, step, unit }, input)
        }
        "integer" if !p.contains_key("enum") => {
            let whole = |k: &str| p.get(k).and_then(Value::as_f64).filter(|n| n.is_finite());
            let min = match whole("exclusiveMinimum") {
                Some(e) => Some(e.floor() as i64 + 1),
                None => whole("minimum").map(|m| m.ceil() as i64),
            };
            let max = match whole("exclusiveMaximum") {
                Some(e) => Some(e.ceil() as i64 - 1),
                None => whole("maximum").map(|m| m.floor() as i64),
            };
            let input = match default.and_then(Value::as_i64) {
                Some(n) => Input::Integer(n),
                None => Input::Typed(String::new()),
            };
            (Kind::Integer { min, max, unit }, input)
        }
        "boolean" => (Kind::Boolean, Input::Boolean(default.and_then(Value::as_bool).unwrap_or(false))),
        "string" => match p.get("enum") {
            Some(options) => {
                let options: Vec<String> = options.as_array()?.iter().map(|o| o.as_str().map(str::to_owned)).collect::<Option<_>>()?;
                if options.is_empty() {
                    return None;
                }
                let chosen = default.and_then(Value::as_str).and_then(|d| options.iter().position(|o| o == d));
                (Kind::Choice(options), Input::Choice(chosen))
            }
            None => {
                let length = |k: &str| p.get(k).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
                let input = Input::Text(default.and_then(Value::as_str).unwrap_or_default().to_owned());
                (Kind::Text { min_length: length("minLength"), max_length: length("maxLength") }, input)
            }
        },
        _ => return None,
    };
    let label = text("title").unwrap_or_else(|| key.to_owned());
    Some((Field { key: key.to_owned(), label, tip: text("description"), required, kind }, input))
}

/// The parameters the fields make, or why Run stays off. An optional field left empty is left out,
/// so the plugin's own default stands.
pub(crate) fn params(fields: &[(Field, Input)]) -> Result<Value, Problem> {
    let mut out = Map::new();
    for (field, input) in fields {
        let missing = || Problem::Missing(field.label.clone());
        let invalid = || Problem::Invalid(field.label.clone());
        let value = match (&field.kind, input) {
            (Kind::Number { .. }, Input::Number(n)) => Some(json!(n)),
            (Kind::Integer { .. }, Input::Integer(n)) => Some(json!(n)),
            (Kind::Number { .. }, Input::Typed(text)) => match text.trim() {
                "" => None,
                t => Some(json!(ui_kit::parse_number(t).filter(|n| n.is_finite()).ok_or_else(invalid)?)),
            },
            (Kind::Integer { .. }, Input::Typed(text)) => match text.trim() {
                "" => None,
                t => Some(json!(t.parse::<i64>().map_err(|_| invalid())?)),
            },
            (Kind::Boolean, Input::Boolean(b)) => Some(json!(b)),
            (Kind::Choice(_), Input::Choice(None)) => None,
            (Kind::Choice(options), Input::Choice(Some(index))) => Some(json!(options.get(*index).ok_or_else(invalid)?)),
            (Kind::Text { .. }, Input::Text(text)) if text.is_empty() => None,
            (Kind::Text { .. }, Input::Text(text)) => Some(json!(text)),
            _ => return Err(invalid()),
        };
        let Some(value) = value else {
            if field.required {
                return Err(missing());
            }
            continue;
        };
        if !within(&field.kind, &value) {
            return Err(invalid());
        }
        out.insert(field.key.clone(), value);
    }
    Ok(Value::Object(out))
}

/// Whether `value` is within the limits of `kind`.
fn within(kind: &Kind, value: &Value) -> bool {
    match kind {
        Kind::Number { min, max, exclusive_min, exclusive_max, step, .. } => {
            let Some(n) = value.as_f64() else { return false };
            let above = min.is_none_or(|m| if *exclusive_min { n > m } else { n >= m });
            let below = max.is_none_or(|m| if *exclusive_max { n < m } else { n <= m });
            let stepped = step.is_none_or(|s| ((n / s) - (n / s).round()).abs() < 1e-9);
            above && below && stepped
        }
        Kind::Integer { min, max, .. } => value.as_i64().is_some_and(|n| min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m)),
        Kind::Text { min_length, max_length } => value.as_str().is_some_and(|s| {
            let n = s.chars().count();
            min_length.is_none_or(|m| n >= m) && max_length.is_none_or(|m| n <= m)
        }),
        Kind::Boolean | Kind::Choice(_) => true,
    }
}

/// An entry's label in the menu: its first `menu` slot's name, else the entry's own name.
pub(crate) fn entry_label(entry: &solidcraft_engine::plugin::Entry) -> String {
    entry
        .slots
        .iter()
        .filter_map(|slot| serde_json::to_value(slot).ok())
        .find(|slot| slot["kind"] == "menu")
        .and_then(|slot| slot["name"].as_str().map(str::to_owned))
        .unwrap_or_else(|| entry.name.clone())
}

/// The command groups `plugin` asks for that the user has not granted.
pub(crate) fn missing_groups(app: &SolveApp, plugin: &Listed) -> Vec<String> {
    plugin.groups.iter().filter(|g| !app.session.plugin_state.command_groups.contains(*g)).cloned().collect()
}

/// The plugins installed in `dir` and those inspected this session, by name. Reading a package
/// runs none of its code.
pub(crate) fn list(app: &SolveApp, dir: Option<&Path>) -> Vec<Listed> {
    let mut paths: Vec<String> = Vec::new();
    if let Some(dir) = dir
        && let Ok(folders) = std::fs::read_dir(dir)
    {
        let mut found: Vec<String> = folders
            .flatten()
            .filter(|f| f.file_type().is_ok_and(|t| t.is_dir()) && !f.file_name().to_string_lossy().starts_with('.'))
            .take(1024)
            .map(|f| f.path().to_string_lossy().into_owned())
            .collect();
        found.sort();
        paths.extend(found);
    }
    paths.extend(app.session.plugin_state.packages.values().cloned());
    let mut seen = BTreeSet::new();
    let mut listed: Vec<Listed> = paths
        .iter()
        .filter_map(|path| solidcraft_engine::plugin::read_package(path).ok().map(|p| (path, p)))
        .filter_map(|(path, package)| {
            let runtime = package.manifest.runtime?;
            let id = package.manifest.id;
            seen.insert(id.clone()).then(|| Listed {
                disabled: app.preferences.disabled_plugins.contains(&id),
                id,
                name: package.manifest.name,
                version: package.manifest.version,
                description: package.manifest.description,
                path: path.clone(),
                approval: package.approval,
                groups: runtime.permissions.command_groups,
                entries: runtime.entries,
            })
        })
        .collect();
    listed.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    listed
}

/// The menu's rows: Install plugin…, a separator, then each plugin's entries.
pub(crate) fn rows(app: &SolveApp) -> Vec<crate::app_bar::FileRow> {
    use crate::app_bar::{FileItem, FileRow};
    let item = |id: String, label: String, enabled: bool| FileRow::Item(FileItem { id, label, key: String::new(), icon: String::new(), enabled });
    let mut rows = vec![item(INSTALL.into(), app.tr("ui.plugin_menu_install").into(), true), FileRow::Separator];
    if app.plugin_menu.listed.is_empty() {
        rows.push(item("empty".into(), app.tr("ui.plugin_menu_empty").into(), false));
    }
    for (p, plugin) in app.plugin_menu.listed.iter().enumerate() {
        let entries: Vec<FileRow> = plugin
            .entries
            .iter()
            .enumerate()
            .map(|(e, entry)| item(format!("entry:{p}:{e}"), format!("{}…", entry_label(entry)), !plugin.disabled))
            .collect();
        match <[FileRow; 1]>::try_from(entries) {
            Ok([one]) => rows.push(one),
            Err(entries) => rows.push(FileRow::Sub { label: plugin.name.clone(), rows: entries }),
        }
    }
    rows
}

/// The Plugins button was clicked at `rect`: open the menu (reading the installed plugins), or
/// close it.
pub(crate) fn toggle(app: &mut SolveApp, ctx: &egui::Context, rect: egui::Rect) {
    let state = &mut app.plugin_menu;
    state.open = !state.open;
    state.opened_pass = ctx.cumulative_pass_nr();
    state.x = rect.left();
    if state.open {
        // One menu at a time: File closes.
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), false));
        app.plugin_menu.listed = list(app, crate::plugins::plugins_dir().as_deref());
    }
}

/// Draw the menu while it is open.
pub(crate) fn menu(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.plugin_menu.open {
        return;
    }
    let rows = rows(app);
    let at = egui::pos2(app.plugin_menu.x, crate::app_bar::HEIGHT);
    let just_opened = app.plugin_menu.opened_pass == ctx.cumulative_pass_nr();
    let out = crate::app_bar::file_menu(ctx, &rows, at, egui::Id::unique("sc_plugins_area"), just_opened);
    if out.close {
        app.plugin_menu.open = false;
    }
    match out.chosen.as_deref() {
        Some(INSTALL) => install_plugin_action(app),
        Some(id) => {
            let mut parts = id.strip_prefix("entry:").into_iter().flat_map(|rest| rest.split(':')).map(str::parse::<usize>);
            if let (Some(Ok(p)), Some(Ok(e))) = (parts.next(), parts.next()) {
                open_entry(app, p, e);
            }
        }
        None => {}
    }
}

/// Install plugin…: pick a plugin `.zip`, review what it asks for, then install it.
pub(crate) fn install_plugin_action(app: &mut SolveApp) {
    crate::plugin_install::start(app);
}

/// The dialog of one plugin entry.
pub(crate) struct EntryDialog {
    pub(crate) plugin: Listed,
    pub(crate) entry: String,
    pub(crate) label: String,
    pub(crate) read: bool,
    /// The fields, or `None` when the schema needs the agent or the CLI.
    pub(crate) fields: Option<Vec<(Field, Input)>>,
    pub(crate) running: Option<Running>,
    pub(crate) error: Option<String>,
    /// A read entry's result, as indented JSON.
    pub(crate) result: Option<String>,
}

/// A run waiting for its reply.
pub(crate) struct Running {
    reply: std::sync::mpsc::Receiver<Value>,
    ticket: std::sync::Arc<solidcraft_control::Ticket>,
}

/// Open the dialog of entry `e` of listed plugin `p`.
pub(crate) fn open_entry(app: &mut SolveApp, p: usize, e: usize) {
    let Some(plugin) = app.plugin_menu.listed.get(p).cloned() else { return };
    let Some(entry) = plugin.entries.get(e) else { return };
    let fields = form(&entry.input_schema);
    app.plugin_menu.dialog = Some(EntryDialog {
        label: entry_label(entry),
        entry: entry.name.clone(),
        read: entry.effect == solidcraft_engine::plugin::Effect::Read,
        fields,
        plugin,
        running: None,
        error: None,
        result: None,
    });
}

impl EntryDialog {
    /// The parameters to run with, or why Run is off (in words for the dialog).
    pub(crate) fn ready(&self, app: &SolveApp) -> Result<Value, String> {
        if self.plugin.disabled {
            return Err(app.tr("ui.plugin_dialog_disabled").into());
        }
        let missing = missing_groups(app, &self.plugin);
        if !missing.is_empty() {
            return Err(solidcraft_ui_text::format("ui.plugin_dialog_needs_groups", &[("groups", &missing.join(", "))]));
        }
        let Some(fields) = &self.fields else { return Err(app.tr("ui.plugin_dialog_unsupported").into()) };
        params(fields).map_err(|problem| match problem {
            Problem::Missing(label) => solidcraft_ui_text::format("ui.plugin_dialog_missing", &[("field", &label)]),
            Problem::Invalid(label) => solidcraft_ui_text::format("ui.plugin_dialog_invalid", &[("field", &label)]),
        })
    }
}

/// Run the open dialog's entry: one `plugin.run` request with the package's content hash, queued
/// as the control channel's are, so it computes on its worker and lands as one undo step.
pub(crate) fn run(app: &mut SolveApp) -> Result<(), String> {
    let Some(dialog) = app.plugin_menu.dialog.as_ref() else { return Ok(()) };
    if dialog.running.is_some() {
        return Ok(());
    }
    let params = dialog.ready(app)?;
    let request = json!({"command":"plugin.run","params":{
        "path": dialog.plugin.path, "approval": dialog.plugin.approval, "entry": dialog.entry, "params": params}});
    let (req, reply) = solidcraft_control::ControlRequest::new("engine.execute", request);
    let ticket = req.ticket.clone();
    app.control_held.push_back(req);
    if let Some(dialog) = app.plugin_menu.dialog.as_mut() {
        dialog.running = Some(Running { reply, ticket });
        dialog.error = None;
        dialog.result = None;
    }
    Ok(())
}

/// Take a finished run's reply: an edit closes the dialog, a read shows its result, an error
/// stays in the dialog.
pub(crate) fn poll(app: &mut SolveApp) {
    let Some(dialog) = app.plugin_menu.dialog.as_mut() else { return };
    let Some(running) = &dialog.running else { return };
    let reply = match running.reply.try_recv() {
        Ok(reply) => reply,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => json!({"ok":false,"error":solidcraft_ui_text::tr("ui.plugin_no_result")}),
    };
    dialog.running = None;
    if reply["ok"] != true {
        let error = reply["error"].as_str().unwrap_or_default();
        let error = error.strip_prefix("plugin: ").unwrap_or(error);
        dialog.error = Some(solidcraft_ui_text::format("ui.plugin_dialog_failed", &[("error", error)]));
        return;
    }
    if dialog.read {
        let result = reply.pointer("/result/result").cloned().unwrap_or(Value::Null);
        dialog.result = Some(serde_json::to_string_pretty(&result).unwrap_or_default());
        return;
    }
    let done = solidcraft_ui_text::format("ui.plugin_dialog_done", &[("entry", &dialog.label)]);
    app.plugin_menu.dialog = None;
    app.set_status(done, false);
}

/// Draw the open entry dialog.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    poll(app);
    let Some(mut dialog) = app.plugin_menu.dialog.take() else { return };
    let ready = dialog.ready(app);
    let running = dialog.running.is_some();
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    let title = format!("{} — {}", dialog.plugin.name, dialog.label);
    let (run_label, close_label) = (app.tr("ui.plugin_dialog_run").to_owned(), app.tr(if dialog.read { "ui.close" } else { "ui.cancel" }).to_owned());
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_plugin_entry"), "prefs_plugins", &title)
        .fields()
        .actions(&run_label, &close_label, None)
        .show(ctx, ready.is_ok() && !running, |ui| body(app, ui, &mut dialog, ready.as_ref().err()));
    for (handle, key) in [("plugin_dialog:run", "dialog:ok"), ("plugin_dialog:cancel", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    let close = closed || action == ui_kit::Action::Cancel;
    if close {
        // A run still computing is stopped: nothing of it is applied.
        if let Some(running) = &dialog.running {
            running.ticket.stop();
        }
        return;
    }
    app.plugin_menu.dialog = Some(dialog);
    if action == ui_kit::Action::Ok
        && let Err(error) = run(app)
        && let Some(dialog) = app.plugin_menu.dialog.as_mut()
    {
        dialog.error = Some(error);
    }
}

fn body(app: &SolveApp, ui: &mut egui::Ui, dialog: &mut EntryDialog, why_not: Option<&String>) {
    let plugin = &dialog.plugin;
    let about = if plugin.description.is_empty() {
        format!("{} {}", plugin.name, plugin.version)
    } else {
        format!("{} {} · {}", plugin.name, plugin.version, plugin.description)
    };
    ui_kit::text(ui, &about, ui_kit::Text::Note("plugin_about"));
    let missing = missing_groups(app, plugin);
    if plugin.disabled {
        ui_kit::status_line(ui, app.tr("ui.plugin_dialog_disabled"), ui_kit::StatusTone::Warning);
    } else if !missing.is_empty() {
        let asks = solidcraft_ui_text::format("ui.plugin_dialog_needs_groups", &[("groups", &missing.join(", "))]);
        ui_kit::status_line(ui, &asks, ui_kit::StatusTone::Warning);
    }
    let editable = dialog.running.is_none();
    match dialog.fields.as_mut() {
        None => {
            ui_kit::status_line(ui, app.tr("ui.plugin_dialog_unsupported"), ui_kit::StatusTone::Warning);
        }
        Some(fields) if fields.is_empty() => {
            ui_kit::text(ui, app.tr("ui.plugin_dialog_no_inputs"), ui_kit::Text::Note("plugin_no_inputs"));
        }
        Some(fields) => {
            ui.add_enabled_ui(editable, |ui| {
                for (field, input) in fields.iter_mut() {
                    field_row(app, ui, field, input);
                }
            });
        }
    }
    if dialog.running.is_some() {
        ui_kit::status_line(ui, app.tr("ui.plugin_dialog_running"), ui_kit::StatusTone::Dim);
    } else if let Some(error) = &dialog.error {
        ui_kit::status_line(ui, error, ui_kit::StatusTone::Error);
    } else if let Some(why) = why_not
        && dialog.fields.as_ref().is_some_and(|f| !f.is_empty())
        && !plugin.disabled
        && missing.is_empty()
    {
        ui_kit::status_line(ui, why, ui_kit::StatusTone::Dim);
    }
    if let Some(result) = &dialog.result {
        ui_kit::text(ui, app.tr("ui.plugin_dialog_result"), ui_kit::Text::Note("plugin_result"));
        ui_kit::read_only_text(ui, result);
    }
}

fn field_row(app: &SolveApp, ui: &mut egui::Ui, field: &Field, input: &mut Input) {
    let label = if field.required { format!("{} *", field.label) } else { field.label.clone() };
    let suffix = |unit: &Option<String>| unit.as_ref().map(|u| format!(" {u}")).unwrap_or_default();
    let response = match (&field.kind, input) {
        (Kind::Number { min, max, step, unit, .. }, Input::Number(value)) => {
            let range = min.unwrap_or(f64::NEG_INFINITY)..=max.unwrap_or(f64::INFINITY);
            let speed = step.unwrap_or_else(|| match (min, max) {
                (Some(lo), Some(hi)) if hi > lo => ((hi - lo) / 200.0).min(1.0),
                _ => 0.1,
            });
            ui_kit::row(ui, &label, |ui| ui_kit::number_field(ui, value, range, speed, &suffix(unit)))
        }
        (Kind::Integer { min, max, unit }, Input::Integer(value)) => {
            let range = min.unwrap_or(i64::MIN)..=max.unwrap_or(i64::MAX);
            ui_kit::row(ui, &label, |ui| ui_kit::whole_number_field(ui, value, range, &suffix(unit)))
        }
        (Kind::Number { unit, .. } | Kind::Integer { unit, .. }, Input::Typed(text)) => {
            let hint = match (field.required, unit) {
                (true, Some(unit)) => format!("{} ({unit})", app.tr("ui.plugin_dialog_required_hint")),
                (true, None) => app.tr("ui.plugin_dialog_required_hint").to_owned(),
                (false, Some(unit)) => unit.clone(),
                (false, None) => String::new(),
            };
            ui_kit::row(ui, &label, |ui| ui_kit::text_field_reserving(ui, text, &hint, 0.0).1)
        }
        (Kind::Boolean, Input::Boolean(value)) => ui_kit::checkbox(ui, &label, value),
        (Kind::Choice(options), Input::Choice(chosen)) => {
            // Until something is chosen the dropdown leads with "Choose…", which is not a value.
            let choose = app.tr("ui.plugin_dialog_choose").to_owned();
            let lead = chosen.is_none();
            let shown: Vec<&str> = lead.then_some(choose.as_str()).into_iter().chain(options.iter().map(String::as_str)).collect();
            let mut index = chosen.unwrap_or(0);
            let response = ui_kit::row(ui, &label, |ui| ui_kit::dropdown(ui, &format!("plugin_field_{}", field.key), &mut index, &shown));
            *chosen = if lead { index.checked_sub(1) } else { Some(index) };
            response
        }
        (Kind::Text { .. }, Input::Text(text)) => {
            let hint = if field.required { app.tr("ui.plugin_dialog_required_hint") } else { "" };
            ui_kit::row(ui, &label, |ui| ui_kit::text_field_reserving(ui, text, hint, 0.0).1)
        }
        _ => ui.response(),
    };
    crate::scenario::publish_handle(&format!("plugin_dialog:field:{}", field.key), response.rect.center());
    if let Some(tip) = &field.tip {
        response.backing_on_hover_text(tip);
    }
}

#[cfg(test)]
#[path = "plugin_menu_tests.rs"]
mod tests;
