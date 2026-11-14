//! The Plugins menu, and the dialog each plugin entry gets from its input schema.
//!
//! The menu sits after File in the application bar (desktop builds only; the web build runs no
//! plugins). "Install plugin…" comes first, then one row per command of each installed plugin
//! that is turned on in Preferences › Plugins, labelled by [`command_title`]. A plugin with more
//! than one command gets a submenu named after the plugin. The Model tab of the ribbon has a
//! Plugins group with one button per command ([`ribbon_buttons`]). A plugin turned off shows in
//! neither place, and nothing opens or runs until the user picks one specific command.
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

/// The start of a plugin command's ribbon button id: `plugin_entry:<plugin id>:<entry name>`.
pub(crate) const RIBBON_PREFIX: &str = "plugin_entry:";

/// The menu's state, and the open entry dialog.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) open: bool,
    /// The app bar had no room for the Plugins button this frame: Plugins is a File submenu.
    pub(crate) in_file_menu: bool,
    /// The pass the menu opened on: that click is not a click elsewhere.
    opened_pass: u64,
    /// The left edge of the Plugins button, where the menu opens.
    x: f32,
    /// Where the menu's top is: the app bar's bottom, unless the ribbon's Plugins group opened it.
    y: Option<f32>,
    /// The plugins listed when the menu last opened.
    pub(crate) listed: Vec<Listed>,
    /// `listed` has been read at least once since the plugins last changed (the ribbon reads it
    /// once rather than every frame).
    pub(crate) loaded: bool,
    pub(crate) dialog: Option<EntryDialog>,
    /// The run waiting in the control queue: its request's serial and the session it is for.
    pending: Option<(u64, u64)>,
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
        // A required key that no field can fill: the dialog could only send it empty.
        None if !required.is_empty() => return None,
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
    // Only keywords the dialog draws or checks before Run turns on; any other constraint
    // (`pattern`, `const`, `items`, `oneOf`, ...) would let an invalid value through.
    let kind = p.get("type")?.as_str()?;
    let allowed: &[&str] = match (kind, p.contains_key("enum")) {
        ("number", false) => &["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf"],
        ("integer", false) => &["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"],
        ("boolean", false) => &[],
        ("string", true) => &["enum"],
        ("string", false) => &["minLength", "maxLength", "format"],
        _ => return None,
    };
    const ANNOTATIONS: &[&str] = &["type", "title", "description", "default", "unit", "x-unit", "units", "examples", "$comment", "deprecated"];
    if p.keys().any(|k| !ANNOTATIONS.contains(&k.as_str()) && !allowed.contains(&k.as_str())) {
        return None;
    }
    // A constraint of the wrong JSON type is a schema this dialog can't check either.
    let numeric = |k: &str| p.get(k).is_none_or(|v| v.as_f64().is_some_and(f64::is_finite));
    let exclusive = |k: &str| p.get(k).is_none_or(|v| v.is_boolean() || v.as_f64().is_some_and(f64::is_finite));
    let count = |k: &str| p.get(k).is_none_or(|v| v.as_u64().is_some());
    let words = |k: &str| p.get(k).is_none_or(Value::is_string);
    if !(numeric("minimum") && numeric("maximum") && numeric("multipleOf") && exclusive("exclusiveMinimum") && exclusive("exclusiveMaximum"))
        || !(count("minLength") && count("maxLength"))
        || !(words("title") && words("description") && words("unit") && words("x-unit") && words("units") && words("format"))
    {
        return None;
    }
    let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let number = |k: &str| p.get(k).and_then(Value::as_f64).filter(|n| n.is_finite());
    let unit = text("unit").or_else(|| text("x-unit")).or_else(|| text("units"));
    let default = p.get("default");
    let (kind, input) = match kind {
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
            if p.contains_key("multipleOf") && step.is_none() {
                return None;
            }
            if let (Some(lo), Some(hi)) = (min, max)
                && (lo > hi || (lo == hi && (exclusive_min || exclusive_max)))
            {
                return None;
            }
            let input = match default.and_then(Value::as_f64).filter(|n| n.is_finite()) {
                Some(n) => Input::Number(n),
                None => Input::Typed(String::new()),
            };
            (Kind::Number { min, max, exclusive_min, exclusive_max, step, unit }, input)
        }
        "integer" if !p.contains_key("enum") => {
            let (min, max) = (integer_bound(p, true)?, integer_bound(p, false)?);
            if let (Some(lo), Some(hi)) = (min, max)
                && lo > hi
            {
                return None;
            }
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

/// An integer property's lowest (`lower`) or highest allowed value: `Some(None)` when it has
/// none, `None` when a bound lies outside the 64-bit integers this field holds. `minimum` with
/// the draft-4 boolean `exclusiveMinimum`, and the numeric `exclusiveMinimum`, both apply.
fn integer_bound(p: &Map<String, Value>, lower: bool) -> Option<Option<i64>> {
    let (inclusive, exclusive) = if lower { ("minimum", "exclusiveMinimum") } else { ("maximum", "exclusiveMaximum") };
    // Within i64 before any arithmetic: 2^63 is exactly representable, so `< 2^63` is exact.
    let whole = |v: f64| (v.is_finite() && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&v)).then_some(v as i64);
    let past = |v: f64| if lower { whole(v.floor())?.checked_add(1) } else { whole(v.ceil())?.checked_sub(1) };
    let within = |v: f64| whole(if lower { v.ceil() } else { v.floor() });
    let mut bounds = Vec::new();
    if let Some(v) = p.get(inclusive).and_then(Value::as_f64) {
        bounds.push(if p.get(exclusive) == Some(&Value::Bool(true)) { past(v)? } else { within(v)? });
    }
    if let Some(v) = p.get(exclusive).and_then(Value::as_f64) {
        bounds.push(past(v)?);
    }
    Some(if lower { bounds.into_iter().max() } else { bounds.into_iter().min() })
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

/// A plugin command's title, in the menu, on its ribbon button and on its dialog: the entry's
/// `menu` slot name when it declares one, else the plugin's name when the plugin has this one
/// command, else the entry's own name.
pub(crate) fn command_title(plugin: &Listed, entry: &solidcraft_engine::plugin::Entry) -> String {
    match slot_name(entry, "menu") {
        Some(name) => name,
        None if plugin.entries.len() == 1 => plugin.name.clone(),
        None => entry.name.clone(),
    }
}

/// The name of the entry's first slot of `kind` (`menu` or `ribbon`).
fn slot_name(entry: &solidcraft_engine::plugin::Entry, kind: &str) -> Option<String> {
    entry
        .slots
        .iter()
        .filter_map(|slot| serde_json::to_value(slot).ok())
        .find(|slot| slot["kind"] == kind)
        .and_then(|slot| slot["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned))
}

/// Whether the user has `plugin` turned on in Preferences › Plugins (read live, so turning it off
/// hides it at once).
pub(crate) fn shown(app: &SolveApp, plugin: &Listed) -> bool {
    crate::plugins::enabled(app, &plugin.id)
}

/// One plugin command's button in the ribbon's Plugins group.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RibbonButton {
    /// `plugin_entry:<plugin id>:<entry name>`.
    pub(crate) id: String,
    /// The label under the button: the entry's `ribbon` slot name, else its [`command_title`].
    pub(crate) label: String,
    /// Its accessible name and hover text: the plugin's name, and the command's when it differs.
    pub(crate) name: String,
    /// A kit icon name. Manifests have no icon of their own yet: the generic plugin icon.
    pub(crate) icon: &'static str,
}

/// The ribbon's Plugins group: one button per command of each plugin that is turned on, in the
/// menu's order. Empty on the web build, which runs no plugins.
pub(crate) fn ribbon_buttons(app: &SolveApp) -> Vec<RibbonButton> {
    if cfg!(target_arch = "wasm32") {
        return Vec::new();
    }
    let mut buttons = Vec::new();
    for plugin in app.plugin_menu.listed.iter().filter(|p| shown(app, p)) {
        for entry in &plugin.entries {
            let title = command_title(plugin, entry);
            let name = if title == plugin.name { title.clone() } else { format!("{}: {title}", plugin.name) };
            buttons.push(RibbonButton {
                id: format!("{RIBBON_PREFIX}{}:{}", plugin.id, entry.name),
                label: slot_name(entry, "ribbon").unwrap_or(title),
                name,
                icon: "prefs_plugins",
            });
        }
    }
    buttons
}

/// Read the installed plugins when they have not been read since they last changed.
pub(crate) fn ensure_listed(app: &mut SolveApp) {
    if !app.plugin_menu.loaded && !cfg!(target_arch = "wasm32") {
        refresh(app);
    }
}

/// Open the dialog of the ribbon button `id` (`plugin_entry:<plugin id>:<entry name>`). Returns
/// whether `id` was a plugin command's.
pub(crate) fn choose_ribbon(app: &mut SolveApp, id: &str) -> bool {
    let Some((plugin, entry)) = id.strip_prefix(RIBBON_PREFIX).and_then(|rest| rest.split_once(':')) else { return false };
    let found = app.plugin_menu.listed.iter().enumerate().find(|(_, p)| p.id == plugin).and_then(|(p, listed)| {
        let e = listed.entries.iter().position(|e| e.name == entry)?;
        Some((p, e))
    });
    if let Some((p, e)) = found {
        open_entry(app, p, e);
    }
    true
}

/// The command groups `plugin` asks for that the user has not granted.
pub(crate) fn missing_groups(app: &SolveApp, plugin: &Listed) -> Vec<String> {
    plugin.groups.iter().filter(|g| !app.session.plugin_state.command_groups.contains(*g)).cloned().collect()
}

/// The plugins installed in `dir` and those inspected this session, by name. Reading a package
/// runs none of its code.
pub(crate) fn list(app: &SolveApp, dir: Option<&Path>) -> Vec<Listed> {
    // A package the session inspected explicitly comes first: when an installed folder has the
    // same id, the inspected one is what `plugin.run` was pointed at, as the engine keeps it.
    let mut paths: Vec<String> = app.session.plugin_state.packages.values().cloned().collect();
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
    let mut seen = BTreeSet::new();
    let mut listed: Vec<Listed> = paths
        .iter()
        .filter_map(|path| solidcraft_engine::plugin::read_package(path).ok().map(|p| (path, p)))
        .filter_map(|(path, package)| {
            let runtime = package.manifest.runtime?;
            let id = package.manifest.id;
            seen.insert(id.clone()).then(|| Listed {
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
    } else if !app.plugin_menu.listed.iter().any(|p| shown(app, p)) {
        rows.push(item("empty".into(), app.tr("ui.plugin_menu_all_off").into(), false));
    }
    // A plugin turned off in Preferences › Plugins is left out; the others keep their index.
    for (p, plugin) in app.plugin_menu.listed.iter().enumerate().filter(|(_, p)| shown(app, p)) {
        let entries: Vec<FileRow> = plugin
            .entries
            .iter()
            .enumerate()
            .map(|(e, entry)| item(format!("entry:{p}:{e}"), format!("{}…", command_title(plugin, entry)), true))
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
    state.y = None;
    if state.open {
        // One menu at a time: File closes.
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("sc_file_menu"), false));
        refresh(app);
    }
}

/// The ribbon's Plugins group label at `rect` was clicked: open the Plugins menu below it, or
/// close it.
pub(crate) fn toggle_below(app: &mut SolveApp, ctx: &egui::Context, rect: egui::Rect) {
    toggle(app, ctx, rect);
    app.plugin_menu.y = Some(rect.bottom());
}

/// Read the installed plugins again, as the menu shows them when it opens.
pub(crate) fn refresh(app: &mut SolveApp) {
    app.plugin_menu.listed = list(app, crate::plugins::plugins_dir().as_deref());
    app.plugin_menu.loaded = true;
}

/// Where the File menu holds the Plugins menu (a narrow app bar): a submenu of the same rows,
/// their ids under `plugins:`.
pub(crate) fn file_submenu(app: &SolveApp) -> crate::app_bar::FileRow {
    use crate::app_bar::FileRow;
    fn prefixed(rows: Vec<FileRow>) -> Vec<FileRow> {
        rows.into_iter()
            .map(|row| match row {
                FileRow::Item(mut item) => {
                    item.id = format!("plugins:{}", item.id);
                    FileRow::Item(item)
                }
                FileRow::Button { mut item, tip } => {
                    item.id = format!("plugins:{}", item.id);
                    FileRow::Button { item, tip }
                }
                FileRow::Sub { label, rows } => FileRow::Sub { label, rows: prefixed(rows) },
                FileRow::Separator => FileRow::Separator,
            })
            .collect()
    }
    FileRow::Sub { label: app.tr("ui.plugins").to_owned(), rows: prefixed(rows(app)) }
}

/// Run the Plugins menu's row `id` (from the menu or the File submenu).
pub(crate) fn choose(app: &mut SolveApp, id: &str) {
    if id == INSTALL {
        install_plugin_action(app);
        return;
    }
    let mut parts = id.strip_prefix("entry:").into_iter().flat_map(|rest| rest.split(':')).map(str::parse::<usize>);
    if let (Some(Ok(p)), Some(Ok(e))) = (parts.next(), parts.next()) {
        open_entry(app, p, e);
    }
}

/// Draw the menu while it is open.
pub(crate) fn menu(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.plugin_menu.open {
        return;
    }
    let rows = rows(app);
    let at = egui::pos2(app.plugin_menu.x, app.plugin_menu.y.unwrap_or(crate::app_bar::HEIGHT));
    let just_opened = app.plugin_menu.opened_pass == ctx.cumulative_pass_nr();
    let out = crate::app_bar::file_menu(ctx, &rows, at, egui::Id::unique("sc_plugins_area"), just_opened);
    if out.close {
        app.plugin_menu.open = false;
    }
    if let Some(id) = out.chosen {
        choose(app, &id);
    }
}

/// Install plugin…: pick a plugin `.zip`, review what it asks for, then install it.
pub(crate) fn install_plugin_action(app: &mut SolveApp) {
    crate::plugin_install::start(app);
}

/// The dialog of one plugin entry.
pub(crate) struct EntryDialog {
    pub(crate) plugin: Listed,
    /// The session (design tab) the dialog was opened on: Run acts on that design only.
    pub(crate) session: u64,
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
    if !shown(app, &plugin) {
        return;
    }
    let fields = form(&entry.input_schema);
    app.plugin_menu.dialog = Some(EntryDialog {
        label: command_title(&plugin, entry),
        entry: entry.name.clone(),
        read: entry.effect == solidcraft_engine::plugin::Effect::Read,
        fields,
        plugin,
        session: app.session.uid,
        running: None,
        error: None,
        result: None,
    });
}

impl EntryDialog {
    /// The parameters to run with, or why Run is off (in words for the dialog).
    pub(crate) fn ready(&self, app: &SolveApp) -> Result<Value, String> {
        if self.session != app.session.uid {
            return Err(app.tr("ui.plugin_dialog_other_design").into());
        }
        if !shown(app, &self.plugin) {
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
    app.plugin_menu.pending = Some((req.serial, app.session.uid));
    app.control_held.push_back(req);
    if let Some(dialog) = app.plugin_menu.dialog.as_mut() {
        dialog.running = Some(Running { reply, ticket });
        dialog.error = None;
        dialog.result = None;
    }
    Ok(())
}

/// A queued run is for the design its dialog was opened on: refused, when its turn comes, if
/// another design tab is active by then.
pub(crate) fn check_dispatch(app: &mut SolveApp, req: &solidcraft_control::ControlRequest) -> Result<(), String> {
    match app.plugin_menu.pending {
        Some((serial, session)) if serial == req.serial => {
            app.plugin_menu.pending = None;
            if session == app.session.uid { Ok(()) } else { Err(app.tr("ui.plugin_dialog_other_design").into()) }
        }
        _ => Ok(()),
    }
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
    // Just the command's own title ("Gears"): no plugin or menu name after it.
    let title = dialog.label.clone();
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
    let other_design = dialog.session != app.session.uid;
    if other_design {
        ui_kit::status_line(ui, app.tr("ui.plugin_dialog_other_design"), ui_kit::StatusTone::Warning);
    } else if !shown(app, plugin) {
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
        && shown(app, plugin)
        && !other_design
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
