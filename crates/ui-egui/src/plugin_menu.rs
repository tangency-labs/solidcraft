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
//! field, a checkbox, a dropdown, a text field or a selection field per property. A selection
//! field is the pick control Extrude's Profiles and Chamfer's Edges have: the viewport picks into
//! the active one (through a [`crate::dialogs::Kind::PluginPick`] dialog, so picking is the
//! native tools' own), and Run sends the picks as explicit references. Parts of a schema this
//! dialog does not draw (nested objects, other lists) leave Run off with a note to run the entry
//! from the agent or the CLI, where the full schema is available.
//!
//! Run goes through the same `plugin.run` request the control channel sends, with the package's
//! content hash read when the dialog opened: the computation runs on its worker and comes back as
//! one undo step. Optional field hooks use the same engine validation on a read-only command
//! worker. Programmatic calls never open this dialog.

use std::path::Path;
use std::sync::Arc;

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::plugin::conditions::{FieldConditions, InputConditions, validate_conditions, visible_values};

use crate::selection::{Accept, SelInput};

use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;

/// The menu row that installs a plugin.
const INSTALL: &str = "install";
/// Last import report…: the report of the last file an importer plugin opened, again.
const LAST_REPORT: &str = "last_import_report";

/// The start of a plugin command's ribbon button id: `plugin_entry:<plugin id>:<entry name>`.
pub(crate) const RIBBON_PREFIX: &str = "plugin_entry:";

/// The menu's state, and the open entry dialog.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) open: bool,
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
    /// The open entry dialogs, oldest first: each plugin entry has its own, with its own window,
    /// title and field values, so several can be open at once.
    pub(crate) dialogs: Vec<EntryDialog>,
    /// The next dialog's id.
    next_dialog: u64,
    /// The runs waiting in the control queue: each one's request serial and the session it is for.
    pending: Vec<(u64, u64)>,
    panel_pick: Option<PanelPick>,
    panel_runs: Vec<std::sync::mpsc::Receiver<Value>>,
}

#[cfg(test)]
impl State {
    /// The most recently opened entry dialog.
    pub(crate) fn dialog(&self) -> Option<&EntryDialog> {
        self.dialogs.last()
    }
}

pub(crate) use crate::plugins::Plugin as Listed;

struct PanelPick {
    id: u64,
    session: u64,
    document: Arc<solidcraft_engine::doc::Document>,
    plugin: String,
    approval: String,
    entry: String,
    event: solidcraft_engine::plugin::ui::UiEvent,
    previous: Option<Sel>,
}

/// Panels use the same single-target selection control and viewport picker as entry forms.
pub(crate) fn start_panel_picking(
    app: &mut SolveApp,
    plugin: String,
    approval: String,
    entry: String,
    event: solidcraft_engine::plugin::ui::UiEvent,
) {
    if app.tool.is_some() || app.dialog.as_ref().is_some_and(|dialog| !matches!(dialog.kind, crate::dialogs::Kind::PluginPick { .. })) {
        return;
    }
    if let Some(dialog) = app.dialog.take() {
        keep_picks(app, &dialog);
    }
    let id = app.plugin_menu.next_dialog;
    app.plugin_menu.next_dialog = id.saturating_add(1);
    let input = SelInput::new(
        "ui.plugin_panel_selection",
        crate::selection::PROFILES
            | crate::selection::EDGES
            | crate::selection::FACES
            | crate::selection::BODIES
            | crate::selection::PLANES
            | crate::selection::AXES
            | crate::selection::VERTICES
            | crate::selection::SKETCH_LINES
            | crate::selection::EXTERNAL_POINTS,
        false,
    );
    let mut picker = crate::dialogs::Dialog::new(crate::dialogs::Kind::PluginPick { dialog: id }, vec![input]);
    picker.awaiting_pick = true;
    app.plugin_menu.panel_pick =
        Some(PanelPick { id, session: app.session.uid, document: app.session.doc.clone(), plugin, approval, entry, event, previous: None });
    app.dialog = Some(picker);
}

/// One property of an entry's input schema, as a field of the dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) tip: Option<String>,
    pub(crate) required: bool,
    pub(crate) kind: Kind,
    /// A choice's pictures, one package image path (`x-solidcraft-icon`) per option in order;
    /// empty unless every option names one.
    pub(crate) images: Vec<String>,
    /// Optional package raster per option, including partially illustrated lists.
    pub(crate) swatches: Vec<Option<String>>,
    conditions: FieldConditions,
    order: usize,
    pub(crate) visible: bool,
    pub(crate) enabled: bool,
    available: Vec<usize>,
    pub(crate) edited: bool,
    prefilled: bool,
    controls: Option<Map<String, Value>>,
    hook: Option<solidcraft_engine::plugin::dialog::FieldPatch>,
    hook_baseline: Option<Input>,
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
    /// A string from a fixed list: the values sent, and the titles shown for them (a value's own
    /// text when the schema gives it no title).
    Choice {
        values: Vec<String>,
        titles: Vec<String>,
    },
    Text {
        min_length: Option<usize>,
        max_length: Option<usize>,
        /// `format` is `directory`: when the host has a folder picker, a Browse button fills
        /// this field. Without one, including the browser build, the field stays a text box.
        directory: bool,
    },
    /// Geometry picked in the viewport: what it takes, how many, and whether it sends body names
    /// (a `bodies` or `body_ids` list) rather than selection references.
    Selection {
        accept: Accept,
        min: usize,
        max: Option<usize>,
        names: bool,
    },
}

/// The annotation that makes an array property a selection field.
pub(crate) const SELECTION_KEY: &str = "x-solidcraft-selection";

/// What a selection field may take (`"accept"` in [`SELECTION_KEY`]), and the pick kind each is.
/// Sketch points and curves are picked with their sketch: ids repeat from one sketch to the next.
pub(crate) const CATEGORIES: &[(&str, Accept)] = &[
    ("profile", crate::selection::PROFILES),
    ("edge", crate::selection::EDGES),
    ("face", crate::selection::FACES),
    ("body", crate::selection::BODIES),
    ("sketch_point", crate::selection::EXTERNAL_POINTS),
    ("sketch_curve", crate::selection::SKETCH_LINES),
];

/// A field's current value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Input {
    /// A stored value this widget cannot represent: keep it visible until corrected.
    Raw(String),
    Number(f64),
    Integer(i64),
    /// A number or whole number with no default: typed, and empty until filled.
    Typed(String),
    Boolean(bool),
    /// The chosen option; `None` until one is chosen, when the schema gives no default (the
    /// dropdown then leads with "Choose…").
    Choice(Option<usize>),
    Text(String),
    /// The picks, in the order they were picked.
    Selection(Vec<Sel>),
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
    validate_conditions(schema).ok()?;
    let conditions = InputConditions::parse(schema).ok()?;
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
    for (field, _) in &mut fields {
        field.conditions = conditions.fields.get(&field.key).cloned().unwrap_or_default();
        field.order = conditions.order.iter().position(|name| name == &field.key).unwrap_or(usize::MAX);
    }
    refresh_conditions(&mut fields).then_some(fields)
}

fn field(key: &str, property: &Value, required: bool) -> Option<(Field, Input)> {
    let p = property.as_object()?;
    // Only keywords the dialog draws or checks before Run turns on; any other constraint
    // (`pattern`, `const`, `items`, `oneOf`, ...) would let an invalid value through.
    let kind = p.get("type")?.as_str()?;
    if kind == "array" {
        return selection_field(key, p, required);
    }
    let choice = p.contains_key("enum") || p.contains_key("oneOf");
    let allowed: &[&str] = match (kind, choice) {
        ("number", false) => &["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf"],
        ("integer", false) => &["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"],
        ("boolean", false) => &[],
        // A list of values, or of `{"const": value, "title": shown}` options; not both.
        ("string", true) if p.contains_key("enum") != p.contains_key("oneOf") => &["enum", "oneOf"],
        ("string", false) => &["minLength", "maxLength", "format"],
        _ => return None,
    };
    const ANNOTATIONS: &[&str] = &[
        "type",
        "title",
        "description",
        "default",
        "unit",
        "x-unit",
        "units",
        "examples",
        "$comment",
        "deprecated",
        "x-solidcraft-visible-when",
        "x-solidcraft-enabled-when",
        "x-solidcraft-defaults",
    ];
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
    let mut images = Vec::new();
    let mut swatches = Vec::new();
    let (kind, input) = match kind {
        "number" if !choice => {
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
        "integer" if !choice => {
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
        "string" if choice => {
            let (values, titles, pictures) = choices(p)?;
            images = pictures;
            swatches = p
                .get("oneOf")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|option| option.get(solidcraft_engine::plugin::swatches::KEY).and_then(Value::as_str).map(str::to_owned))
                .collect();
            let chosen = default.and_then(Value::as_str).and_then(|d| values.iter().position(|o| o == d));
            (Kind::Choice { values, titles }, Input::Choice(chosen))
        }
        "string" => {
            let length = |k: &str| p.get(k).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
            let input = Input::Text(default.and_then(Value::as_str).unwrap_or_default().to_owned());
            let directory = p.get("format").and_then(Value::as_str).is_some_and(|format| format.trim() == "directory");
            (Kind::Text { min_length: length("minLength"), max_length: length("maxLength"), directory }, input)
        }
        _ => return None,
    };
    let label = text("title").unwrap_or_else(|| key.to_owned());
    Some((
        Field {
            key: key.to_owned(),
            label,
            tip: text("description"),
            required,
            kind,
            images,
            swatches,
            conditions: Default::default(),
            order: 0,
            visible: true,
            enabled: true,
            available: Vec::new(),
            edited: false,
            prefilled: false,
            controls: None,
            hook: None,
            hook_baseline: None,
        },
        input,
    ))
}

/// An array property as a selection field: one annotated with [`SELECTION_KEY`]
/// (`{"version": 1, "accept": [...]}`, sending selection references), or one of the list inputs
/// the manifest reserves without it: `bodies` and `body_ids` take bodies and send their names;
/// `selection`, `target` and `targets` take edges, faces and bodies. `minItems` and `maxItems` are
/// the counts Run checks; `maxItems: 1` makes a pick replace the last one. `None` for any other
/// array, an annotation this host does not read, or an `items` that is not the type sent.
fn selection_field(key: &str, p: &Map<String, Value>, required: bool) -> Option<(Field, Input)> {
    const KEYWORDS: &[&str] = &[
        "type",
        "title",
        "description",
        "minItems",
        "maxItems",
        "items",
        "$comment",
        "deprecated",
        SELECTION_KEY,
        "x-solidcraft-visible-when",
        "x-solidcraft-enabled-when",
    ];
    if p.keys().any(|k| !KEYWORDS.contains(&k.as_str())) {
        return None;
    }
    let (accept, names) = match p.get(SELECTION_KEY) {
        Some(annotation) => (annotated_accept(annotation)?, false),
        None => match key {
            "bodies" | "body_ids" => (crate::selection::BODIES, true),
            "selection" | "target" | "targets" => (crate::selection::EDGES | crate::selection::FACES | crate::selection::BODIES, false),
            _ => return None,
        },
    };
    // Each item is a body name or a selection reference: `items` may say so, and nothing more.
    if let Some(items) = p.get("items") {
        let items = items.as_object()?;
        if items.len() != 1 || items.get("type")?.as_str()? != if names { "string" } else { "object" } {
            return None;
        }
    }
    let count = |k: &str| match p.get(k) {
        None => Some(None),
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).map(Some),
    };
    let (min, max) = (count("minItems")?, count("maxItems")?);
    if max == Some(0) || min.zip(max).is_some_and(|(lo, hi)| lo > hi) {
        return None;
    }
    let text = |k: &str| match p.get(k) {
        None => Some(None),
        Some(v) => Some(Some(v.as_str()?.trim()).filter(|s| !s.is_empty()).map(str::to_owned)),
    };
    let (title, tip) = (text("title")?, text("description")?);
    let kind = Kind::Selection { accept, min: min.unwrap_or(0), max, names };
    Some((
        Field {
            key: key.to_owned(),
            label: title.unwrap_or_else(|| key.to_owned()),
            tip,
            required,
            kind,
            images: Vec::new(),
            swatches: Vec::new(),
            conditions: Default::default(),
            order: 0,
            visible: true,
            enabled: true,
            available: Vec::new(),
            edited: false,
            prefilled: false,
            controls: None,
            hook: None,
            hook_baseline: None,
        },
        Input::Selection(Vec::new()),
    ))
}

/// The pick kinds a [`SELECTION_KEY`] annotation takes: version 1, and a non-empty `accept` list
/// of [`CATEGORIES`] names. `None` for anything else, which this host cannot honour.
fn annotated_accept(annotation: &Value) -> Option<Accept> {
    let annotation = annotation.as_object()?;
    if annotation.keys().any(|k| !matches!(k.as_str(), "version" | "accept")) || annotation.get("version")?.as_u64()? != 1 {
        return None;
    }
    let list = annotation.get("accept")?.as_array()?;
    if list.is_empty() {
        return None;
    }
    list.iter().try_fold(0, |accept, category| {
        let name = category.as_str()?;
        CATEGORIES.iter().find(|(n, _)| *n == name).map(|(_, bit)| accept | bit)
    })
}

/// A choice's values and the titles shown for them, from `enum` (each value shown as it is) or
/// `oneOf` (each option exactly `{"const": value}` plus optional `title`, `description`,
/// `x-solidcraft-when` and package artwork), and the icons when every option names one.
/// `None` for an empty list, a value that is not a string, a repeated value, an image path that is
/// not text, or an option with any other keyword, which this dialog could not check.
fn choices(p: &Map<String, Value>) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
    let images: Vec<Option<String>> = match p.get("oneOf").and_then(Value::as_array) {
        Some(options) => options
            .iter()
            .map(|option| match option.get(crate::plugin_images::KEY) {
                None => Some(None),
                Some(path) => path.as_str().map(|p| Some(p.to_owned())),
            })
            .collect::<Option<_>>()?,
        None => Vec::new(),
    };
    let images: Vec<String> = images.into_iter().collect::<Option<_>>().unwrap_or_default();
    let (values, titles): (Vec<String>, Vec<String>) = match (p.get("enum"), p.get("oneOf")) {
        (Some(values), None) => values.as_array()?.iter().map(|v| v.as_str().map(|v| (v.to_owned(), v.to_owned()))).collect::<Option<Vec<_>>>()?,
        (None, Some(options)) => options
            .as_array()?
            .iter()
            .map(|option| {
                let option = option.as_object()?;
                if option.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "const"
                            | "title"
                            | "description"
                            | "x-solidcraft-when"
                            | crate::plugin_images::KEY
                            | solidcraft_engine::plugin::swatches::KEY
                    )
                }) {
                    return None;
                }
                let value = option.get("const")?.as_str()?.to_owned();
                let title = match option.get("title") {
                    None => value.clone(),
                    Some(title) => Some(title.as_str()?.trim()).filter(|t| !t.is_empty()).map_or_else(|| value.clone(), str::to_owned),
                };
                Some((value, title))
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    }
    .into_iter()
    .unzip();
    let distinct: std::collections::BTreeSet<&String> = values.iter().collect();
    (!values.is_empty() && distinct.len() == values.len()).then_some((values, titles, images))
}

/// A scalar's current value, including partially filled forms, for sibling conditions.
fn scalar_value(field: &Field, input: &Input) -> Option<Value> {
    match input {
        Input::Number(n) => Some(json!(n)),
        Input::Integer(n) => Some(json!(n)),
        Input::Boolean(b) => Some(json!(b)),
        Input::Text(s) if !s.is_empty() || field.prefilled => Some(json!(s)),
        Input::Typed(s) => match field.kind {
            Kind::Integer { .. } => s.trim().parse::<i64>().ok().map(|n| json!(n)),
            _ => ui_kit::parse_number(s).filter(|n| n.is_finite()).map(|n| json!(n)),
        },
        Input::Choice(Some(i)) => {
            if let Kind::Choice { values, .. } = &field.kind {
                values.get(*i).map(|v| json!(v))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn default_input(field: &Field, value: &Value) -> Option<Input> {
    match &field.kind {
        Kind::Number { .. } => value.as_f64().map(Input::Number),
        Kind::Integer { .. } => value.as_i64().map(Input::Integer),
        Kind::Boolean => value.as_bool().map(Input::Boolean),
        Kind::Text { .. } => value.as_str().map(|s| Input::Text(s.to_owned())),
        Kind::Choice { values, .. } => value.as_str().and_then(|v| values.iter().position(|s| s == v)).map(|i| Input::Choice(Some(i))),
        Kind::Selection { accept, names, .. } => value
            .as_array()?
            .iter()
            .map(|value| {
                let pick = if *names { Sel::Body { name: value.as_str()?.to_owned() } } else { serde_json::from_value::<Sel>(value.clone()).ok()? };
                crate::dialogs::fits(*accept, &pick).then_some(pick)
            })
            .collect::<Option<Vec<_>>>()
            .map(Input::Selection),
    }
}

/// Controllers are evaluated first, including chains of defaults and hidden controllers.
fn refresh_conditions(fields: &mut [(Field, Input)]) -> bool {
    let mut order: Vec<(usize, usize)> = fields.iter().enumerate().map(|(i, (f, _))| (f.order, i)).collect();
    order.sort_unstable();
    let raw: Map<String, Value> = fields.iter().filter_map(|(f, i)| scalar_value(f, i).map(|v| (f.key.clone(), v))).collect();
    let Ok(mut values) = visible_values(&raw, fields.iter().map(|(f, _)| (f.key.as_str(), &f.conditions))) else { return false };
    for (_, index) in order {
        let Some((field, input)) = fields.get_mut(index) else { continue };
        let controls: Map<String, Value> =
            field.conditions.dependencies().iter().filter_map(|k| values.get(k).map(|v| (k.clone(), v.clone()))).collect();
        let changed = field.controls.as_ref() != Some(&controls);
        field.visible = field.conditions.visible(&values);
        field.enabled = field.conditions.enabled(&values);
        if !field.edited
            && changed
            && field.hook.as_ref().is_none_or(|patch| !patch.contains_key("value") && !patch.contains_key("default"))
            && let Some(default) = field.conditions.default_value(&values).and_then(|v| default_input(field, v))
        {
            *input = default;
        }
        if let Kind::Choice { values: choices, .. } = &field.kind {
            let was_unavailable = field.controls.is_some() && field.available.is_empty();
            field.available = (0..choices.len()).filter(|i| field.conditions.option_available(*i, &values)).collect();
            if let Input::Choice(chosen) = input
                && !field.prefilled
                && (chosen.is_some_and(|i| !field.available.contains(&i)) || (chosen.is_none() && was_unavailable && !field.available.is_empty()))
            {
                let default = field.conditions.default_value(&values).and_then(Value::as_str).and_then(|v| choices.iter().position(|s| s == v));
                *chosen = default.filter(|i| field.available.contains(i)).or_else(|| field.available.first().copied());
            }
        }
        let declared_visible = field.visible;
        if let Some(patch) = &field.hook {
            field.visible &= patch.get("visible") != Some(&Value::Bool(false));
            field.enabled &= patch.get("enabled") != Some(&Value::Bool(false));
            if let Some(choices) = patch.get("choices").and_then(Value::as_array)
                && let Kind::Choice { values, .. } = &field.kind
            {
                field.available.retain(|i| values.get(*i).is_some_and(|v| choices.iter().any(|c| c.as_str() == Some(v))));
            }
        }
        field.controls = Some(controls);
        if declared_visible {
            if let Some(value) = scalar_value(field, input) {
                values.insert(field.key.clone(), value);
            } else {
                values.remove(&field.key);
            }
        } else {
            values.remove(&field.key);
        }
    }
    true
}

/// The parameters the fields make, or why Run stays off. An optional field left empty is left out,
/// so the plugin's own default stands.
pub(crate) fn params(fields: &[(Field, Input)]) -> Result<Value, Problem> {
    let mut out = Map::new();
    for (field, input) in fields {
        if !sends_field(field) {
            continue;
        }
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
            (Kind::Choice { .. }, Input::Choice(None)) => None,
            (Kind::Choice { values, .. }, Input::Choice(Some(index))) => {
                if !field.available.contains(index) {
                    return Err(invalid());
                }
                Some(json!(values.get(*index).ok_or_else(invalid)?))
            }
            (Kind::Text { .. }, Input::Text(text)) if text.is_empty() && !field.prefilled => None,
            (Kind::Text { .. }, Input::Text(text)) => Some(json!(text)),
            // An optional field with nothing picked is left out; a required one is sent, empty
            // only when `minItems` allows it.
            (Kind::Selection { min, .. }, Input::Selection(picks)) if picks.is_empty() => {
                if (field.required || field.prefilled) && *min > 0 {
                    return Err(missing());
                }
                (field.required || field.prefilled).then(|| json!([]))
            }
            (Kind::Selection { min, max, names, .. }, Input::Selection(picks)) => {
                if picks.len() < *min || max.is_some_and(|m| picks.len() > m) {
                    return Err(invalid());
                }
                let items = if *names {
                    picks
                        .iter()
                        .map(|p| if let Sel::Body { name } = p { Some(json!(name)) } else { None })
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(invalid)?
                } else {
                    picks.iter().map(|p| serde_json::to_value(p).map_err(|_| invalid())).collect::<Result<Vec<_>, _>>()?
                };
                Some(Value::Array(items))
            }
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

/// A hook hides presentation only; declarative visibility decides whether a value is sent.
fn sends_field(field: &Field) -> bool {
    field.visible
        || (field.hook.as_ref().is_some_and(|p| p.get("visible") == Some(&Value::Bool(false)))
            && field.controls.as_ref().is_some_and(|c| field.conditions.visible(c)))
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
        Kind::Text { min_length, max_length, .. } => value.as_str().is_some_and(|s| {
            let n = s.chars().count();
            min_length.is_none_or(|m| n >= m) && max_length.is_none_or(|m| n <= m)
        }),
        Kind::Boolean | Kind::Choice { .. } | Kind::Selection { .. } => true,
    }
}

/// A plugin command's icon, on its ribbon button, its menu rows and its dialog's header: the image
/// its schema names with `x-solidcraft-icon` when the package has it and it passed the icon check
/// (`plugin_images`), else the generic plugin icon.
pub(crate) fn command_icon<'a>(plugin: &'a Listed, entry: &solidcraft_engine::plugin::Entry) -> &'a str {
    entry
        .input_schema
        .get(crate::plugin_images::KEY)
        .and_then(Value::as_str)
        .and_then(|rel| plugin.images.get(rel))
        .map_or(crate::plugin_images::GENERIC, String::as_str)
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
    /// The command's icon ([`command_icon`]).
    pub(crate) icon: String,
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
                icon: command_icon(plugin, entry).to_owned(),
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
    let granted = app.session.plugin_state.grants.get(&plugin.id);
    plugin.groups.iter().filter(|g| granted.is_none_or(|grants| !grants.command_groups.contains(*g))).cloned().collect()
}

/// The plugins installed in `dir` and those inspected this session, by name. Reading a package
/// runs none of its code.
pub(crate) fn list(app: &SolveApp, dir: Option<&Path>) -> Vec<Listed> {
    let (mut listed, _) = crate::plugins::inventory(dir, app.session.plugin_state.packages.values());
    for plugin in &mut listed {
        plugin.disabled = !crate::plugins::enabled(app, &plugin.id);
    }
    listed
}

/// The menu's rows: Install plugin…, Last import report…, a separator, then each plugin's entries.
pub(crate) fn rows(app: &SolveApp) -> Vec<crate::app_bar::FileRow> {
    use crate::app_bar::{FileItem, FileRow};
    let item = |id: String, label: String, enabled: bool| FileRow::Item(FileItem { id, label, key: String::new(), icon: String::new(), enabled });
    let mut rows = vec![
        item(INSTALL.into(), app.tr("ui.plugin_menu_install").into(), true),
        item(LAST_REPORT.into(), app.tr("ui.plugin_menu_last_report").into(), app.import_report.is_some()),
        FileRow::Separator,
    ];
    if app.plugin_menu.listed.iter().all(|plugin| plugin.entries.is_empty()) {
        rows.push(item("empty".into(), app.tr("ui.plugin_menu_empty").into(), false));
    } else if !app.plugin_menu.listed.iter().any(|p| shown(app, p)) {
        rows.push(item("empty".into(), app.tr("ui.plugin_menu_all_off").into(), false));
    }
    // A plugin turned off in Preferences › Plugins is left out; the others keep their index.
    for (p, plugin) in app.plugin_menu.listed.iter().enumerate().filter(|(_, p)| shown(app, p) && !p.entries.is_empty()) {
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
    solidcraft_engine::plugin::sync_panels(&mut app.session);
    app.plugin_menu.listed = list(app, app.session.plugin_state.plugins_dir().as_deref());
}

/// Run the Plugins menu's row `id` (from the menu or the File submenu).
pub(crate) fn choose(app: &mut SolveApp, id: &str) {
    if id == INSTALL {
        install_plugin_action(app);
        return;
    }
    if id == LAST_REPORT {
        crate::import_report::reopen(app);
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
    /// Names this dialog's window and its runs; no other open dialog has it.
    pub(crate) id: u64,
    pub(crate) plugin: Listed,
    /// The session (design tab) the dialog was opened on: Run acts on that design only.
    pub(crate) session: u64,
    pub(crate) entry: String,
    pub(crate) label: String,
    /// The command's icon ([`command_icon`]), in the window's header.
    pub(crate) icon: String,
    pub(crate) read: bool,
    /// The fields, or `None` when the schema needs the agent or the CLI.
    pub(crate) fields: Option<Vec<(Field, Input)>>,
    pub(crate) running: Option<Running>,
    hook: Option<HookState>,
    pub(crate) error: Option<String>,
    /// A read entry's result, as indented JSON.
    pub(crate) result: Option<String>,
    /// An importer's extensions: its button chooses the file to open, with these options.
    pub(crate) opens: Option<Vec<String>>,
    /// An export slot's extensions: its button chooses where to save, then exports the design
    /// with these options.
    pub(crate) writes: Option<Vec<String>>,
    /// The design the selection fields' picks were made on. A change to it (an edit, an undo, an
    /// update) clears them: a reference into an older model never stands for another item.
    pub(crate) picked_on: Option<Arc<solidcraft_engine::doc::Document>>,
    /// The picks were cleared because the design changed: the dialog says so until the next pick.
    pub(crate) picks_cleared: bool,
    /// The dialog's Preview box: the host draws what Run would change while the fields are filled.
    pub(crate) preview: bool,
    pub(crate) form_guard: Option<solidcraft_engine::schema::plugin::FormGuard>,
    pub(crate) changed: bool,
    /// A stale Apply finished revalidation; only then ask the person to Apply again.
    revalidated: bool,
    /// Field keys in the current viewport picker, independent of changes to raw inputs.
    picker_fields: Vec<String>,
    prefill_extra: Option<Value>,
    validation: std::sync::Mutex<Validation>,
    #[cfg(test)]
    validation_gate: Option<Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>>,
    /// Its first result has been shown: later previews leave the user's camera alone.
    preview_drawn: bool,
}

/// One bounded worker per dialog; generations invalidate replies before any field is touched.
type HookReply = Result<solidcraft_engine::plugin::dialog::Update, String>;
type PendingHook = (u64, std::sync::mpsc::Receiver<HookReply>);

struct HookState {
    schema: Value,
    generation: u64,
    observed: Vec<Input>,
    document: Arc<solidcraft_engine::doc::Document>,
    changed_at: f64,
    submitted: Option<u64>,
    pending: Option<PendingHook>,
    note: bool,
}

/// Untouched fields send their value from before the hook, so the hook sees the same inputs while
/// its last reply stays on screen.
fn hook_values(fields: &[(Field, Input)]) -> Value {
    let mut values = Map::new();
    for (field, input) in fields {
        let input = field.hook_baseline.as_ref().filter(|_| !field.edited).unwrap_or(input);
        if let Some(value) = scalar_value(field, input) {
            values.insert(field.key.clone(), value);
        } else if matches!(input, Input::Selection(_)) {
            let mut field = field.clone();
            field.required = false;
            // The shown visibility may come from a hook value; the engine drops what the sent
            // controllers hide.
            field.visible = true;
            if let Ok(value) = params(&[(field, input.clone())])
                && let Some(object) = value.as_object()
            {
                values.extend(object.clone());
            }
        }
    }
    Value::Object(values)
}

fn apply_hook_reply(dialog: &mut EntryDialog, generation: u64, update: solidcraft_engine::plugin::dialog::Update) -> bool {
    let Some(hook) = &mut dialog.hook else { return false };
    if generation != hook.generation {
        return false;
    }
    let Some(fields) = &mut dialog.fields else { return false };
    hook.note = update.warning.is_some();
    hook.submitted = Some(generation);
    for (field, input) in fields.iter_mut() {
        field.hook = update.fields.get(&field.key).cloned();
        field.tip = field
            .hook
            .as_ref()
            .and_then(|f| f.get("hint"))
            .and_then(Value::as_str)
            .or_else(|| hook.schema.get("properties")?.get(&field.key)?.get("description")?.as_str())
            .map(str::to_owned);
        if field.edited {
            continue;
        }
        // An equal value leaves the field as it is; only a value the reply drops goes back.
        match field
            .hook
            .as_ref()
            .filter(|f| f.contains_key("value") || f.contains_key("default"))
            .and_then(|_| update.values.get(&field.key))
            .and_then(|v| default_input(field, v))
        {
            Some(value) => {
                field.hook_baseline.get_or_insert_with(|| input.clone());
                *input = value;
            }
            None => {
                if let Some(baseline) = field.hook_baseline.take() {
                    *input = baseline;
                    // Let refresh_conditions apply a conditional default that the hook value held back.
                    field.controls = None;
                }
            }
        }
    }
    refresh_conditions(fields);
    hook.observed = fields.iter().map(|(_, i)| i.clone()).collect();
    true
}

/// Polling never waits. The command worker owns package I/O, Wasm compilation and execution.
fn poll_hooks(app: &mut SolveApp, ctx: &egui::Context) {
    let now = crate::now_ms();
    for dialog in &mut app.plugin_menu.dialogs {
        let Some(hook) = &mut dialog.hook else { continue };
        let Some(fields) = &mut dialog.fields else { continue };
        let values: Vec<Input> = fields.iter().map(|(_, i)| i.clone()).collect();
        let document_changed = dialog.session == app.session.uid && !Arc::ptr_eq(&hook.document, &app.session.doc);
        if values != hook.observed || document_changed {
            hook.document = app.session.doc.clone();
            hook.generation = hook.generation.wrapping_add(1);
            hook.changed_at = now;
            hook.observed = values;
            hook.note = false;
            // A value change keeps the last reply on screen until the next one replaces it.
            for (field, input) in fields.iter_mut().filter(|_| document_changed) {
                field.hook = None;
                field.controls = None;
                if let Some(baseline) = field.hook_baseline.take()
                    && !field.edited
                    && !(document_changed && matches!(&baseline, Input::Selection(_)))
                {
                    *input = baseline;
                }
                field.tip = hook
                    .schema
                    .get("properties")
                    .and_then(|p| p.get(&field.key))
                    .and_then(|p| p.get("description"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            refresh_conditions(fields);
            hook.observed = fields.iter().map(|(_, i)| i.clone()).collect();
        }
        let completed = hook.pending.as_ref().and_then(|(generation, reply)| match reply.try_recv() {
            Ok(reply) => Some((*generation, reply)),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((*generation, Err(String::new()))),
        });
        if let Some((generation, reply)) = completed {
            hook.pending = None;
            match reply {
                Ok(update) => {
                    if apply_hook_reply(dialog, generation, update) {
                        dialog.picked_on = dialog.hook.as_ref().map(|hook| hook.document.clone());
                        if let Some(picker) = app
                            .dialog
                            .as_mut()
                            .filter(|picker| matches!(picker.kind, crate::dialogs::Kind::PluginPick { dialog:id } if id == dialog.id))
                        {
                            picker.inputs = dialog.selection_inputs();
                        }
                    }
                }
                Err(_) => {
                    if let Some(hook) = &mut dialog.hook
                        && generation == hook.generation
                    {
                        hook.note = true;
                    }
                }
            }
        }
        let Some(hook) = &mut dialog.hook else { continue };
        if hook.pending.is_some() || hook.submitted == Some(hook.generation) {
            continue;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(25));
        if now - hook.changed_at < 120.0 || dialog.session != app.session.uid {
            continue;
        }
        let Some(fields) = &dialog.fields else { continue };
        let protected = fields.iter().filter(|(f, _)| f.edited).map(|(f, _)| f.key.clone()).collect();
        let values = hook_values(fields);
        let session = app.session.scratch();
        let plugin = dialog.plugin.clone();
        let entry = dialog.entry.clone();
        let generation = hook.generation;
        hook.submitted = Some(generation);
        hook.pending = crate::control_job::spawn(None, Default::default(), move || {
            let result = (|| {
                let package =
                    solidcraft_engine::plugin::with_installed(&session, &plugin.path, || solidcraft_engine::plugin::read_package(&plugin.path))?;
                if package.approval != plugin.approval {
                    return Err(solidcraft_engine::EngineError::Other("plugin approval changed".into()));
                }
                solidcraft_engine::plugin::update_dialog(&session, &package, &entry, &values, &protected, false)
            })();
            result.map_err(|e| e.to_string())
        })
        .map(|reply| (generation, reply));
        if hook.pending.is_none() {
            hook.note = true;
        }
    }
}

/// A run waiting for its reply.
pub(crate) struct Running {
    reply: std::sync::mpsc::Receiver<Value>,
    ticket: std::sync::Arc<solidcraft_control::Ticket>,
    /// A successful reply waits here while another design tab is active.
    received: Option<Value>,
}

/// Open the dialog of entry `e` of listed plugin `p`.
pub(crate) fn open_entry(app: &mut SolveApp, p: usize, e: usize) {
    let Some(plugin) = app.plugin_menu.listed.get(p).cloned() else { return };
    let Some(entry) = plugin.entries.get(e) else { return };
    if !shown(app, &plugin) {
        return;
    }
    // The entry's dialog is already open on this design: it keeps its values, and stays where it
    // is. Another design's dialog for the same entry belongs to that design.
    let session = app.session.uid;
    if app.plugin_menu.dialogs.iter().any(|d| d.plugin.id == plugin.id && d.entry == entry.name && d.session == session) {
        return;
    }
    let mut fields = form(&entry.input_schema);
    let opens = (entry.kind == solidcraft_engine::plugin::EntryKind::Importer).then(|| entry.extensions.clone());
    let writes = (entry.kind == solidcraft_engine::plugin::EntryKind::Exporter).then(|| entry.extensions.clone());
    let id = app.plugin_menu.next_dialog;
    app.plugin_menu.next_dialog += 1;
    // What is selected fills the first selection field, as far as that field takes it; the
    // other fields start empty, so one selection never fills two fields.
    let mut prefilled = false;
    if let Some((Field { kind: Kind::Selection { accept, max, .. }, .. }, Input::Selection(picks))) =
        fields.as_mut().and_then(|f| f.iter_mut().find(|(field, _)| field.visible && field.enabled && matches!(field.kind, Kind::Selection { .. })))
    {
        picks.extend(app.session.selection.iter().filter(|s| crate::dialogs::fits(*accept, s)).take(max.unwrap_or(usize::MAX)).cloned());
        prefilled = !picks.is_empty();
    }
    let hook = entry.dialog_hook.then(|| HookState {
        schema: entry.input_schema.clone(),
        generation: 0,
        observed: fields.iter().flatten().map(|(_, i)| i.clone()).collect(),
        document: app.session.doc.clone(),
        changed_at: crate::now_ms() - 120.0,
        submitted: None,
        pending: None,
        note: false,
    });
    app.plugin_menu.dialogs.push(EntryDialog {
        id,
        label: command_title(&plugin, entry),
        icon: command_icon(&plugin, entry).to_owned(),
        entry: entry.name.clone(),
        read: entry.effect == solidcraft_engine::plugin::Effect::Read,
        fields,
        plugin,
        session: app.session.uid,
        running: None,
        hook,
        error: None,
        result: None,
        opens,
        writes,
        picked_on: prefilled.then(|| app.session.doc.clone()),
        picks_cleared: false,
        preview: true,
        form_guard: None,
        changed: false,
        revalidated: false,
        picker_fields: Vec::new(),
        prefill_extra: None,
        validation: Default::default(),
        #[cfg(test)]
        validation_gate: None,
        preview_drawn: false,
    });
    // The new dialog's first selection field takes the viewport's picks, unless another tool is
    // picking: that tool keeps them until a field here is clicked.
    if let Some(dialog) = app.plugin_menu.dialogs.last()
        && let Some(field) = dialog
            .fields
            .iter()
            .flatten()
            .filter(|(_, input)| matches!(input, Input::Selection(_)))
            .position(|(field, _)| field.visible && field.enabled)
        && app.tool.is_none()
        && app.dialog.as_ref().is_none_or(|d| matches!(d.kind, crate::dialogs::Kind::PluginPick { .. }))
    {
        start_picking(app, id, field);
    }
}

/// Open an entry's normal form with stored parameters. Shared by ui.start, deferred guest
/// requests and future panel row actions (#19516); no plugin code runs here.
pub(crate) fn open_form(app: &mut SolveApp, plugin_id: &str, entry_name: &str, values: &Value) -> Result<(), String> {
    ensure_listed(app);
    let (p, e) = app
        .plugin_menu
        .listed
        .iter()
        .enumerate()
        .find_map(|(p, plugin)| {
            (plugin.id == plugin_id && shown(app, plugin))
                .then(|| {
                    plugin
                        .entries
                        .iter()
                        .position(|e| e.name == entry_name && e.kind == solidcraft_engine::plugin::EntryKind::Command)
                        .map(|e| (p, e))
                })
                .flatten()
        })
        .ok_or_else(|| app.tr("ui.plugin_edit_unknown").to_owned())?;
    if serde_json::to_vec(values).map_err(|e| e.to_string())?.len() > solidcraft_engine::plugin::MAX_JSON_BYTES {
        return Err(app.tr("ui.plugin_edit_unknown").into());
    }
    // An explicit edit request must never overwrite somebody's unsubmitted form.
    if app.plugin_menu.dialogs.iter().any(|d| d.plugin.id == plugin_id && d.entry == entry_name && d.session == app.session.uid) {
        return Err(app.tr("ui.plugin_edit_already_open").into());
    }
    open_entry(app, p, e);
    let guard =
        solidcraft_engine::schema::plugin::FormGuard { document_id: app.session.document_id.clone(), revision: app.session.design_revision() };
    let Some(dialog) = app.plugin_menu.dialogs.last_mut() else { return Err(app.tr("ui.plugin_edit_unknown").into()) };
    dialog.form_guard = Some(guard);
    // Opening a populated form only validates inputs. Preview is an explicit request.
    dialog.preview = false;
    dialog.prefill_extra = match values.as_object() {
        None => Some(values.clone()),
        Some(values) => {
            let extra: Map<String, Value> = values
                .iter()
                .filter(|(key, _)| dialog.fields.as_ref().is_none_or(|fields| !fields.iter().any(|(field, _)| &field.key == *key)))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (!extra.is_empty()).then_some(Value::Object(extra))
        }
    };
    if let Some(fields) = &mut dialog.fields {
        for (field, input) in fields.iter_mut() {
            // Do not prefill from current viewport selection for an explicit request.
            if let Input::Selection(picks) = input {
                picks.clear();
            }
            if let Some(value) = values.get(&field.key) {
                *input = stored_input(field, value).unwrap_or_else(|| Input::Raw(value.to_string()));
                field.edited = true;
                field.prefilled = true;
            }
        }
        refresh_conditions(fields);
    }
    dialog.picked_on = Some(app.session.doc.clone());
    // Replace the normal form's initial picker with the stored picks as well.
    if let Some(picker) = app.dialog.as_mut()
        && matches!(picker.kind, crate::dialogs::Kind::PluginPick { dialog: owner } if owner == dialog.id)
    {
        picker.inputs = dialog.selection_inputs();
        picker.active = dialog
            .fields
            .iter()
            .flatten()
            .filter(|(_, input)| matches!(input, Input::Selection(_)))
            .position(|(field, _)| field.visible && field.enabled)
            .unwrap_or(usize::MAX);
    }
    Ok(())
}

fn stored_input(field: &Field, value: &Value) -> Option<Input> {
    if let Kind::Selection { names, .. } = field.kind {
        let items = value
            .as_array()?
            .iter()
            .map(|v| if names { Some(Sel::Body { name: v.as_str()?.into() }) } else { serde_json::from_value(v.clone()).ok() })
            .collect::<Option<Vec<_>>>()?;
        return Some(Input::Selection(items));
    }
    if matches!(field.kind, Kind::Integer { .. }) && value.is_f64() {
        let number = value.as_f64()?;
        let integer = number as i64;
        if !number.is_finite() || number.fract() != 0.0 || number as i128 != i128::from(integer) {
            return None;
        }
        return Some(if within(&field.kind, &json!(integer)) { Input::Integer(integer) } else { Input::Typed(value.to_string()) });
    }
    if matches!(field.kind, Kind::Number { .. }) {
        let integer = value.as_i64().map(i128::from).or_else(|| value.as_u64().map(i128::from));
        if let Some(integer) = integer
            && value.as_f64().is_none_or(|number| number as i128 != integer)
        {
            return None;
        }
    }
    if matches!(field.kind, Kind::Number { .. } | Kind::Integer { .. }) && !within(&field.kind, value) {
        return Some(Input::Typed(value.to_string()));
    }
    default_input(field, value)
}

fn input_value(field: &Field, input: &Input) -> Option<Value> {
    match input {
        Input::Raw(text) => Some(serde_json::from_str(text).unwrap_or_else(|_| json!(text))),
        Input::Selection(picks) => {
            if picks.is_empty() && !field.required && !field.prefilled {
                return None;
            }
            let names = matches!(field.kind, Kind::Selection { names: true, .. });
            Some(Value::Array(
                picks
                    .iter()
                    .map(|p| match p {
                        Sel::Body { name } if names => json!(name),
                        _ => serde_json::to_value(p).unwrap_or(Value::Null),
                    })
                    .collect(),
            ))
        }
        _ => scalar_value(field, input),
    }
}

type FieldErrors = std::collections::BTreeMap<String, solidcraft_engine::schema::plugin::FormFieldError>;

#[derive(Clone)]
struct ValidationKey {
    doc: Arc<solidcraft_engine::doc::Document>,
    state: Arc<solidcraft_engine::doc::ModelState>,
    moves: std::collections::BTreeMap<u64, solidcraft_engine::doc::Mat>,
    values: Value,
}
impl ValidationKey {
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.doc, &other.doc) && Arc::ptr_eq(&self.state, &other.state) && self.moves == other.moves && self.values == other.values
    }
}
struct CachedValidation {
    key: ValidationKey,
    errors: FieldErrors,
}
#[derive(Default)]
struct Validation {
    cached: Option<CachedValidation>,
    running: Option<Checking>,
}
struct Checking {
    key: ValidationKey,
    progress: Arc<solidcraft_engine::doc::progress::Progress>,
    reply: std::sync::mpsc::Receiver<FieldErrors>,
}
impl Drop for Checking {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}
impl Validation {
    fn errors(
        &mut self,
        app: &SolveApp,
        schema: &Value,
        values: Value,
        #[cfg(test)] gate: Option<Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>>,
    ) -> Option<FieldErrors> {
        let key = ValidationKey { doc: app.session.doc.clone(), state: app.session.model.state(), moves: app.session.pending_moves.clone(), values };
        if let Some(running) = &self.running {
            if !running.key.matches(&key) {
                running.progress.cancel();
            }
            match running.reply.try_recv() {
                Ok(errors) => {
                    if !running.progress.is_cancelled() {
                        self.cached = Some(CachedValidation { key: running.key.clone(), errors });
                    }
                    self.running = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let failed = !running.progress.is_cancelled();
                    self.running = None;
                    if failed {
                        // Infrastructure failures are transient; the next check retries.
                        return Some(validation_failed());
                    }
                }
            }
        }
        if let Some(cached) = &self.cached
            && cached.key.matches(&key)
        {
            return Some(cached.errors.clone());
        }
        let scratch = app.session.scratch();
        let schema = schema.clone();
        let values = key.values.clone();
        let progress = Arc::new(solidcraft_engine::doc::progress::Progress::default());
        let reply = crate::control_job::spawn(Some(Default::default()), progress.clone(), move || {
            #[cfg(test)]
            if let Some(gate) = gate {
                let _ = gate.lock().ok().and_then(|rx| rx.recv().ok());
            }
            solidcraft_engine::plugin::form_errors(&scratch, &schema, &values)
        });
        match reply {
            Some(reply) => {
                self.running = Some(Checking { key, progress, reply });
                None
            }
            // A worker that could not start must not poison the field-error cache.
            None => Some(validation_failed()),
        }
    }
}
fn validation_failed() -> FieldErrors {
    [("params".into(), solidcraft_engine::schema::plugin::FormFieldError::InvalidValue { reason: "validation worker stopped".into() })].into()
}

impl EntryDialog {
    fn field_errors(&self, app: &SolveApp) -> std::collections::BTreeMap<String, String> {
        if self.session != app.session.uid {
            return Default::default();
        }
        let Some(fields) = &self.fields else { return Default::default() };
        let schema = self.plugin.entries.iter().find(|e| e.name == self.entry).map(|e| &e.input_schema).unwrap_or(&Value::Null);
        let mut values =
            Value::Object(fields.iter().filter(|(f, _)| sends_field(f)).filter_map(|(f, i)| input_value(f, i).map(|v| (f.key.clone(), v))).collect());
        if let Some(extra) = &self.prefill_extra {
            match (values.as_object_mut(), extra.as_object()) {
                (Some(values), Some(extra)) => values.extend(extra.clone()),
                _ => values = extra.clone(),
            }
        }
        let checked = self.validation.lock().ok().and_then(|mut validation| {
            validation.errors(
                app,
                schema,
                values,
                #[cfg(test)]
                self.validation_gate.clone(),
            )
        });
        let Some(checked) = checked else {
            return [("params".into(), app.tr("ui.plugin_edit_checking").to_owned())].into();
        };
        let mut errors: std::collections::BTreeMap<String, String> = checked
            .into_iter()
            .map(|(key, error)| {
                let text = match error {
                    solidcraft_engine::schema::plugin::FormFieldError::MissingPick => app.tr("ui.plugin_edit_missing_pick").to_owned(),
                    solidcraft_engine::schema::plugin::FormFieldError::InvalidValue { reason } if reason == "validation worker stopped" => {
                        app.tr("ui.plugin_start_failed").to_owned()
                    }
                    solidcraft_engine::schema::plugin::FormFieldError::InvalidValue { .. } => {
                        let label = fields.iter().find(|(field, _)| field.key == key).map(|(field, _)| field.label.as_str()).unwrap_or(&key);
                        solidcraft_ui_text::format("ui.plugin_dialog_invalid", &[("field", label)])
                    }
                };
                (key, text)
            })
            .collect();
        for (field, input) in fields.iter().filter(|(f, _)| sends_field(f)) {
            if params(&[(field.clone(), input.clone())]).is_err() {
                errors.entry(field.key.clone()).or_insert_with(|| solidcraft_ui_text::format("ui.plugin_dialog_invalid", &[("field", &field.label)]));
            }
        }
        errors
    }
}

/// Only requests created by this form can enable the additive guest form import.
pub(crate) fn take_interactive_run(app: &mut SolveApp, serial: u64) -> bool {
    let Some(at) = app.plugin_menu.pending.iter().position(|(id, _)| *id == serial) else { return false };
    app.plugin_menu.pending.remove(at);
    true
}

/// A selection field's label as the `'static` text a [`SelInput`] carries. Each distinct label is
/// kept once for the life of the process, so dialogs opened again keep no more.
fn static_label(label: &str) -> &'static str {
    static LABELS: std::sync::Mutex<std::collections::BTreeSet<&'static str>> = std::sync::Mutex::new(std::collections::BTreeSet::new());
    let mut labels = LABELS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(known) = labels.get(label) {
        return known;
    }
    let kept: &'static str = Box::leak(label.to_owned().into_boxed_str());
    labels.insert(kept);
    kept
}

/// A selection field as the input of a pick dialog, holding `picks`. Its label is marked when
/// required, as the other fields' are; its description is its tooltip.
fn selection_input(field: &Field, picks: &[Sel]) -> SelInput {
    let Kind::Selection { accept, max, .. } = field.kind else { return SelInput::new("", 0, false) };
    let label = if field.required { format!("{} *", field.label) } else { field.label.clone() };
    let mut input = SelInput::new(static_label(&label), accept, max != Some(1));
    input.tip = field.tip.as_deref().map(static_label);
    input.items = picks.to_vec();
    input
}

/// Copy `picker`'s picks into `dialog`'s selection fields. When they changed, they date from the
/// design `doc`.
fn take_picks(dialog: &mut EntryDialog, picker: &crate::dialogs::Dialog, doc: &Arc<solidcraft_engine::doc::Document>) {
    let mut changed = false;
    for (field, value) in dialog.fields.iter_mut().flatten() {
        if let Input::Selection(picks) = value
            && let Some(index) = dialog.picker_fields.iter().position(|key| key == &field.key)
            && let Some(input) = picker.inputs.get(index)
            && *picks != input.items
        {
            picks.clone_from(&input.items);
            field.edited = true;
            changed = true;
        }
    }
    if changed {
        dialog.picked_on = Some(doc.clone());
        dialog.picks_cleared = false;
    }
}

/// The open dialog whose selection fields the viewport picks into, if one does.
pub(crate) fn picking(app: &SolveApp) -> Option<u64> {
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(crate::dialogs::Kind::PluginPick { dialog }) => Some(*dialog),
        _ => None,
    }
}

/// Make dialog `id`'s selection field `field` (counted among its selection fields) the one the
/// viewport picks into. Another plugin dialog's picking stops (its picks stay with it); a native
/// tool that is open keeps the viewport, and this returns false.
pub(crate) fn start_picking(app: &mut SolveApp, id: u64, field: usize) -> bool {
    start_picking_listed(app, id, field, false)
}

/// [`start_picking`], with the field's list of picks open when `listed`.
fn start_picking_listed(app: &mut SolveApp, id: u64, field: usize, listed: bool) -> bool {
    // A native tool (a sketch tool) or dialog that is open keeps the viewport's input.
    if app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| !matches!(d.kind, crate::dialogs::Kind::PluginPick { .. })) {
        return false;
    }
    if let Some(d) = app.dialog.take() {
        keep_picks(app, &d);
    }
    let Some(dialog) = app.plugin_menu.dialogs.iter_mut().find(|d| d.id == id) else { return false };
    if dialog.session != app.session.uid || dialog.running.is_some() {
        return false;
    }
    let mut picker = crate::dialogs::Dialog::new(crate::dialogs::Kind::PluginPick { dialog: id }, dialog.selection_inputs());
    picker.active = field.min(picker.inputs.len().saturating_sub(1));
    if !dialog.pickable(picker.active) {
        return false;
    }
    picker.awaiting_pick = true;
    picker.listed = listed.then_some(picker.active);
    app.dialog = Some(picker);
    true
}

/// Stop picking into a plugin dialog (`d`, taken from `app.dialog`): its picks stay in its fields.
pub(crate) fn stop_picking(app: &mut SolveApp, d: crate::dialogs::Dialog) {
    keep_picks(app, &d);
}

/// Copy the picks of `picker` back into the fields of its dialog, when that dialog is still open
/// on this design. A field whose picks changed dates them to the design as it is now.
fn keep_picks(app: &mut SolveApp, picker: &crate::dialogs::Dialog) {
    let crate::dialogs::Kind::PluginPick { dialog: id } = picker.kind else { return };
    if let Some(panel) = app
        .plugin_menu
        .panel_pick
        .as_mut()
        .filter(|panel| panel.id == id && panel.session == app.session.uid && Arc::ptr_eq(&panel.document, &app.session.doc))
    {
        let selected = picker.inputs.first().and_then(|input| input.items.first()).cloned();
        if selected != panel.previous {
            panel.previous = selected.clone();
            let value = selected
                .and_then(|selected| serde_json::to_string(&selected).ok())
                .map(solidcraft_engine::plugin::ui::UiValue::GeometryRef)
                .unwrap_or(solidcraft_engine::plugin::ui::UiValue::None);
            let mut event = panel.event.clone();
            event.value = Some(value);
            let plugin = panel.plugin.clone();
            let entry = panel.entry.clone();
            let approval = panel.approval.clone();
            crate::panel_runtime::queue_selection(app, plugin, approval, entry, event);
        }
        return;
    }
    let (session, doc) = (app.session.uid, app.session.doc.clone());
    if let Some(dialog) = app.plugin_menu.dialogs.iter_mut().find(|d| d.id == id && d.session == session) {
        take_picks(dialog, picker, &doc);
    }
}

/// Keep the picking and the picks true to the design: take the viewport's picks into their
/// dialog, stop picking for a dialog that closed, ran or belongs to another design, and clear
/// picks made on a design that has changed since.
fn sync_picks(app: &mut SolveApp) {
    if let Some(id) = picking(app) {
        let owner = app.plugin_menu.dialogs.iter().find(|d| d.id == id);
        let panel_owner =
            app.plugin_menu.panel_pick.as_ref().is_some_and(|panel| {
                panel.id == id
                    && panel.session == app.session.uid
                    && Arc::ptr_eq(&panel.document, &app.session.doc)
                    && app.session.plugin_panels().iter().any(|snapshot| {
                        snapshot.plugin == panel.plugin && snapshot.panel.id == panel.event.panel && snapshot.approval == panel.approval
                    })
            });
        if !panel_owner
            && owner.is_none_or(|d| {
                d.session != app.session.uid || d.running.is_some() || app.dialog.as_ref().is_some_and(|picker| !d.pickable(picker.active))
            })
        {
            if let Some(d) = app.dialog.take() {
                keep_picks(app, &d);
            }
        } else if let Some(d) = app.dialog.take() {
            keep_picks(app, &d);
            app.dialog = Some(d);
        }
    }
    let (session, doc) = (app.session.uid, app.session.doc.clone());
    let picker = picking(app);
    for dialog in &mut app.plugin_menu.dialogs {
        if dialog.session != session || dialog.picked_on.as_ref().is_none_or(|on| Arc::ptr_eq(on, &doc)) {
            continue;
        }
        if dialog.form_guard.is_some() {
            // Keep stored references visible; validation reports missing picks beside their field.
            continue;
        }
        dialog.picked_on = None;
        let mut cleared = false;
        for picks in dialog.selection_picks_mut() {
            cleared |= !picks.is_empty();
            picks.clear();
        }
        dialog.picks_cleared |= cleared;
        if picker == Some(dialog.id)
            && let Some(d) = app.dialog.as_mut()
        {
            for input in &mut d.inputs {
                input.items.clear();
            }
            d.list_hover = None;
        }
    }
}

impl EntryDialog {
    fn pickable(&self, index: usize) -> bool {
        self.fields
            .iter()
            .flatten()
            .filter(|(_, input)| matches!(input, Input::Selection(_)))
            .nth(index)
            .is_some_and(|(field, _)| field.visible && field.enabled)
    }

    /// The selection fields as the inputs of a pick dialog, in field order, with their picks.
    pub(crate) fn selection_inputs(&mut self) -> Vec<SelInput> {
        self.picker_fields = self.selection_keys();
        let Some(fields) = &self.fields else { return Vec::new() };
        fields
            .iter()
            .filter_map(|(field, input)| match (&field.kind, input) {
                (Kind::Selection { .. }, Input::Selection(picks)) => Some(selection_input(field, picks)),
                _ => None,
            })
            .collect()
    }

    fn selection_keys(&self) -> Vec<String> {
        self.fields.iter().flatten().filter(|(_, input)| matches!(input, Input::Selection(_))).map(|(field, _)| field.key.clone()).collect()
    }

    /// The selection fields' picks, in field order.
    fn selection_picks_mut(&mut self) -> impl Iterator<Item = &mut Vec<Sel>> {
        self.fields.iter_mut().flatten().filter_map(|(_, input)| match input {
            Input::Selection(picks) => Some(picks),
            _ => None,
        })
    }

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
        if self.form_guard.is_some()
            && let Some(error) = self.field_errors(app).values().next()
        {
            return Err(error.clone());
        }
        let mut values = params(fields).map_err(|problem| match problem {
            Problem::Missing(label) => solidcraft_ui_text::format("ui.plugin_dialog_missing", &[("field", &label)]),
            Problem::Invalid(label) => solidcraft_ui_text::format("ui.plugin_dialog_invalid", &[("field", &label)]),
        })?;
        if let (Some(values), Some(extra)) = (values.as_object_mut(), self.prefill_extra.as_ref().and_then(Value::as_object)) {
            values.extend(extra.clone());
        }
        // A picked or pasted folder does not add a grant. Run uses the install grant's check.
        directories_allowed(app, &self.plugin.id, fields, &values)?;
        Ok(values)
    }
}

/// A directory field's path must already lie in a folder approved at install.
fn directories_allowed(app: &SolveApp, plugin_id: &str, fields: &[(Field, Input)], value: &Value) -> Result<(), String> {
    let grants = app.session.plugin_state.grants.get(plugin_id).map(|grants| grants.folders.as_slice()).unwrap_or(&[]);
    let Some(object) = value.as_object() else { return Ok(()) };
    for (field, _) in fields {
        let Kind::Text { directory: true, .. } = field.kind else { continue };
        let Some(path) = object.get(&field.key).and_then(Value::as_str) else { continue };
        if let Err(error) = solidcraft_engine::plugin::require_folder_grant(grants, Path::new(path)) {
            return Err(error.to_string());
        }
    }
    Ok(())
}

/// Whether a plugin command that edits this design has an open dialog.
pub(crate) fn editing_dialog_open(app: &SolveApp) -> bool {
    app.plugin_menu.dialogs.iter().any(|dialog| dialog.session == app.session.uid && !dialog.read && dialog.opens.is_none())
}

/// The dialog the viewport previews, and the `plugin.run` it previews: the dialog picking in the
/// viewport, else the most recently opened one, when it is a command that edits the design on
/// this design, has Preview on, is not running and its fields make parameters. `None` while a
/// native tool is open: its own preview is the one shown.
pub(crate) fn preview_commands(app: &SolveApp) -> Option<(u64, String, Vec<(String, Value)>)> {
    if app.dialog.as_ref().is_some_and(|d| !matches!(d.kind, crate::dialogs::Kind::PluginPick { .. })) {
        return None;
    }
    let dialog = match picking(app) {
        Some(id) => app.plugin_menu.dialogs.iter().find(|d| d.id == id),
        None => app.plugin_menu.dialogs.iter().rev().find(|d| d.session == app.session.uid),
    }?;
    if dialog.read || dialog.opens.is_some() || !dialog.preview || dialog.running.is_some() {
        return None;
    }
    let params = dialog.ready(app).ok()?;
    let run = json!({"path": dialog.plugin.path, "approval": dialog.plugin.approval, "entry": dialog.entry, "params": params});
    Some((dialog.id, dialog.plugin.id.clone(), vec![("plugin.run".to_owned(), run)]))
}

/// The first result drawn by this dialog follows its run's last Fit/Home command, as OK does.
/// Even a run with no view command consumes the first preview: later edits never move the view.
pub(crate) fn preview_drawn(app: &mut SolveApp, id: u64, view: Option<Value>) {
    let Some(dialog) = app.plugin_menu.dialogs.iter_mut().find(|dialog| dialog.id == id) else { return };
    if dialog.preview_drawn {
        return;
    }
    dialog.preview_drawn = true;
    if let Some(view) = view.filter(|view| matches!(view.get("kind").and_then(Value::as_str), Some("fit" | "home"))) {
        // Home also works for an empty result; without preview geometry use the normal view path.
        let bounds = (!app.preview.bounds.is_empty()).then_some(app.preview.bounds);
        app.apply_plugin_view(&json!({"view": view}), bounds);
    }
}

/// Make `scratch` a dry run of plugin `id`: none of its folder or command-group grants, so a
/// previewed run reads and writes no file and reaches nothing past the design. Its changes stay
/// in the scratch session, which is thrown away: nothing is committed or published.
pub(crate) fn dry_run(scratch: &mut solidcraft_engine::Session, id: &str) {
    // Requests stay in the preview reply, which is discarded with the scratch design.
    // No preview reply goes through poll_dialog, the only place that opens requested forms.
    scratch.plugin_forms_interactive = true;
    if let Some(grants) = scratch.plugin_state.grants.get_mut(id) {
        grants.folders.clear();
        grants.command_groups.clear();
        grants.input_folder = false;
    }
    scratch.plugin_state.file_grants.clear();
}

/// Run the most recently opened dialog's entry.
#[cfg(test)]
pub(crate) fn run(app: &mut SolveApp) -> Result<(), String> {
    let Some(id) = app.plugin_menu.dialog().map(|d| d.id) else { return Ok(()) };
    run_dialog(app, id)
}

/// Run the entry of dialog `id`: one `plugin.run` request with the package's content hash, queued
/// as the control channel's are, so it computes on its worker and lands as one undo step.
pub(crate) fn run_dialog(app: &mut SolveApp, id: u64) -> Result<(), String> {
    let Some(dialog) = app.plugin_menu.dialogs.iter().find(|d| d.id == id) else { return Ok(()) };
    if dialog.running.is_some() {
        return Ok(());
    }
    let params = dialog.ready(app)?;
    // The picks are sent as they are now: picking stops while the entry runs.
    if picking(app) == Some(id)
        && let Some(d) = app.dialog.take()
    {
        keep_picks(app, &d);
    }
    let Some(dialog) = app.plugin_menu.dialogs.iter().find(|d| d.id == id) else { return Ok(()) };
    // An importer: choose the file, then open it as a new design on the file worker, as File ›
    // Open does, with the options from this dialog.
    if let Some(extensions) = dialog.opens.clone() {
        let format = std::format!("{}.{}", dialog.plugin.id, dialog.entry);
        app.plugin_menu.dialogs.retain(|d| d.id != id);
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        app.pick_open_for(&extensions, move |app, path| {
            let Some(path) = path else { return };
            let mut request = crate::file_load::Request::new(&path, crate::file_load::Kind::Open);
            request.plugin_import = true;
            request.options.insert("format".into(), json!(format));
            request.options.insert("options".into(), params);
            if let Err(error) = crate::file_load::begin_request(app, request) {
                app.set_status(error, true);
            }
        });
        return Ok(());
    }
    // An export slot: choose where to save, then export the design on the worker, as a run is,
    // with the options from this dialog. The dialog stays open with any error until it is done.
    if let Some(extensions) = dialog.writes.clone() {
        let format = std::format!("{}.{}", dialog.plugin.id, dialog.entry);
        let extension = extensions.first().cloned().unwrap_or_default();
        let name = std::format!("{}.{extension}", crate::export_name::offered(app, None, &app.session.doc.name));
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        app.pick_save(&name, &extensions, move |app, path| {
            let Some(path) = path else { return };
            let request = json!({"command":"file.export","params":{"path": path, "format": format, "options": params}});
            queue(app, id, request);
        });
        return Ok(());
    }
    let request = json!({"command":"plugin.run","params":{
        "path": dialog.plugin.path, "approval": dialog.plugin.approval, "entry": dialog.entry, "params": params, "form_guard": dialog.form_guard}});
    queue(app, id, request);
    Ok(())
}

/// Queue dialog `id`'s request on the control channel's worker, for the design it was opened on.
fn queue(app: &mut SolveApp, id: u64, request: Value) {
    let Some(session) = app.plugin_menu.dialogs.iter().find(|d| d.id == id).map(|d| d.session) else { return };
    let (req, reply) = solidcraft_control::ControlRequest::new("engine.execute", request);
    let ticket = req.ticket.clone();
    app.plugin_menu.pending.push((req.serial, session));
    app.control_held.push_back(req);
    if let Some(dialog) = app.plugin_menu.dialogs.iter_mut().find(|d| d.id == id) {
        dialog.running = Some(Running { reply, ticket, received: None });
        dialog.revalidated = false;
        dialog.error = None;
        dialog.result = None;
    }
}

/// Panel actions enter the same namespaced command queue as the control channel.
pub(crate) fn run_panel(app: &mut SolveApp, command: String, params: Value) -> Result<(), String> {
    if app.plugin_menu.panel_runs.len() >= 16 {
        return Err(app.tr("prefs.plugin_busy").into());
    }
    let (request, reply) = solidcraft_control::ControlRequest::new("engine.execute", json!({"command":command,"params":params}));
    app.plugin_menu.pending.push((request.serial, app.session.uid));
    app.plugin_menu.panel_runs.push(reply);
    app.control_held.push_back(request);
    Ok(())
}

/// A queued run is for the design its dialog was opened on: refused, when its turn comes, if
/// another design tab is active by then.
pub(crate) fn check_dispatch(app: &mut SolveApp, req: &solidcraft_control::ControlRequest) -> Result<(), String> {
    let Some(at) = app.plugin_menu.pending.iter().position(|(serial, _)| *serial == req.serial) else { return Ok(()) };
    let (_, session) = app.plugin_menu.pending[at];
    if session == app.session.uid {
        // Resolve the lazy design revision before the worker copies this session, so
        // validation and the unchanged live design share the same revision token.
        app.session.design_revision();
        Ok(())
    } else {
        app.plugin_menu.pending.remove(at);
        Err(app.tr("ui.plugin_dialog_other_design").into())
    }
}

/// Take the finished runs' replies: an edit closes its dialog, a read shows its result, an error
/// stays in its dialog.
pub(crate) fn poll(app: &mut SolveApp) {
    for reply in std::mem::take(&mut app.plugin_menu.panel_runs) {
        match reply.try_recv() {
            Ok(value) => {
                if value.get("ok") != Some(&Value::Bool(true)) {
                    let error = value.get("error").and_then(Value::as_str).unwrap_or_else(|| solidcraft_ui_text::tr("ui.plugin_no_result"));
                    app.set_status(solidcraft_ui_text::format("ui.plugin_dialog_failed", &[("error", error)]), true);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => app.plugin_menu.panel_runs.push(reply),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => app.set_status(app.tr("ui.plugin_no_result").to_owned(), true),
        }
    }
    let ids: Vec<u64> = app.plugin_menu.dialogs.iter().map(|d| d.id).collect();
    for id in ids {
        poll_dialog(app, id);
    }
}

fn poll_dialog(app: &mut SolveApp, id: u64) {
    let Some(at) = app.plugin_menu.dialogs.iter().position(|d| d.id == id) else { return };
    let dialog = &mut app.plugin_menu.dialogs[at];
    let Some(running) = &mut dialog.running else { return };
    let reply = match running.received.take() {
        Some(reply) => reply,
        None => match running.reply.try_recv() {
            Ok(reply) => reply,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => json!({"ok":false,"error":solidcraft_ui_text::tr("ui.plugin_no_result")}),
        },
    };
    if reply["ok"] != true {
        dialog.running = None;
        let error = reply["error"].as_str().unwrap_or_default();
        let error = error.strip_prefix("plugin: ").unwrap_or(error);
        dialog.error = Some(solidcraft_ui_text::format("ui.plugin_dialog_failed", &[("error", error)]));
        return;
    }
    // A successful entry can request forms containing picks from its own design. Hold
    // both those requests and validation replies until that same session is active.
    if dialog.session != app.session.uid {
        running.received = Some(reply);
        return;
    }
    dialog.running = None;
    if let Some(validation) = reply.pointer("/result/form_validation") {
        if let Some(revision) = validation.get("revision").and_then(Value::as_u64)
            && let Some(guard) = &mut dialog.form_guard
        {
            guard.revision = revision;
        }
        dialog.revalidated = validation.get("changed") == Some(&Value::Bool(true));
        dialog.changed |= dialog.revalidated;
        return;
    }
    if dialog.writes.is_some() {
        let path = reply.pointer("/result/path").and_then(Value::as_str).unwrap_or_default().to_owned();
        let notes: Vec<String> =
            reply.pointer("/result/warnings").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        app.plugin_menu.dialogs.remove(at);
        let mut done = solidcraft_ui_text::format("plugin_export.done", &[("path", &path)]);
        for note in notes {
            done = std::format!("{done} · {note}");
        }
        app.set_status(done, false);
        return;
    }
    if dialog.read {
        let result = reply.pointer("/result/result").cloned().unwrap_or(Value::Null);
        dialog.result = Some(serde_json::to_string_pretty(&result).unwrap_or_default());
        let reopen = reply.pointer("/result/forms").and_then(Value::as_array).is_some_and(|forms| {
            forms.iter().any(|f| {
                f.get("plugin").and_then(Value::as_str) == Some(dialog.plugin.id.as_str())
                    && f.get("entry").and_then(Value::as_str) == Some(dialog.entry.as_str())
            })
        });
        if reopen {
            app.plugin_menu.dialogs.remove(at);
        }
        open_requested_forms(app, &reply);
        return;
    }
    let done = solidcraft_ui_text::format("ui.plugin_dialog_done", &[("entry", &dialog.label)]);
    app.plugin_menu.dialogs.remove(at);
    app.set_status(done, false);
    open_requested_forms(app, &reply);
}

fn open_requested_forms(app: &mut SolveApp, reply: &Value) {
    if let Some(forms) = reply.pointer("/result/forms").and_then(Value::as_array) {
        for form in forms {
            if let (Some(plugin), Some(entry), Some(params)) =
                (form.get("plugin").and_then(Value::as_str), form.get("entry").and_then(Value::as_str), form.get("params"))
                && let Err(error) = open_form(app, plugin, entry, params)
            {
                app.set_status(error, true);
            }
        }
    }
}

/// Draw every open entry dialog, each in its own window.
pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    poll(app);
    sync_picks(app);
    poll_hooks(app, ctx);
    for mut dialog in std::mem::take(&mut app.plugin_menu.dialogs) {
        let id = dialog.id;
        let (stays, run_it, claim) = show_dialog(app, ctx, &mut dialog);
        if !stays {
            continue;
        }
        app.plugin_menu.dialogs.push(dialog);
        // A selection field clicked while the viewport picks for something else.
        if let Some((field, listed)) = claim
            && !start_picking_listed(app, id, field, listed)
        {
            app.set_status(app.tr("ui.plugin_dialog_finish_tool").to_owned(), true);
        }
        if run_it
            && let Err(error) = run_dialog(app, id)
            && let Some(dialog) = app.plugin_menu.dialogs.iter_mut().find(|d| d.id == id)
        {
            dialog.error = Some(error);
        }
    }
    sync_picks(app);
}

/// A selection field clicked while another one has the viewport: its index among the dialog's
/// selection fields, and whether its list of picks was opened.
type Claim = Option<(usize, bool)>;

/// An entry keeps its window across reopenings on the same design. A transient dialog serial
/// changes every time, which used to discard the moved position (#18684). The design identity
/// also prevents two documents' dialogs for the same plugin entry from sharing one window.
fn dialog_window_id(dialog: &EntryDialog, remember_position: bool) -> egui::Id {
    let id = egui::Id::unique("sc_plugin_entry");
    if remember_position { id.with((dialog.plugin.id.as_str(), dialog.entry.as_str(), dialog.session)) } else { id.with(dialog.id) }
}

/// Draw one dialog. Returns whether it stays open, whether Run was pressed, and a selection field
/// that asks for the viewport's picks.
fn show_dialog(app: &mut SolveApp, ctx: &egui::Context, dialog: &mut EntryDialog) -> (bool, bool, Claim) {
    if dialog.session != app.session.uid
        && let Ok(mut validation) = dialog.validation.lock()
    {
        validation.running = None;
    }
    if let Some(guard) = &dialog.form_guard
        && dialog.session == app.session.uid
    {
        let changed = guard.document_id != app.session.document_id || guard.revision != app.session.design_revision();
        dialog.changed |= changed;
        if changed {
            dialog.revalidated = false;
        }
    }
    if let Some(fields) = &mut dialog.fields {
        refresh_conditions(fields);
    }
    let ready = dialog.ready(app);
    let running = dialog.running.is_some();
    if dialog.running.as_ref().is_some_and(|running| running.received.is_none())
        || dialog.validation.lock().ok().is_some_and(|v| v.running.is_some())
        || dialog.hook.as_ref().is_some_and(|h| h.pending.is_some())
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    // Just the command's own title ("Gears"): no plugin or menu name after it.
    let title = dialog.label.clone();
    let run_key = if dialog.opens.is_some() {
        "ui.plugin_dialog_choose_file"
    } else if dialog.writes.is_some() {
        "plugin_export.button"
    } else if dialog.form_guard.is_some() {
        "ui.plugin_edit_apply"
    } else {
        "ui.plugin_dialog_run"
    };
    let (run_label, close_label) = (app.tr(run_key).to_owned(), app.tr(if dialog.read { "ui.close" } else { "ui.cancel" }).to_owned());
    let mut browse = None;
    // This dialog's picker, when the viewport picks into its fields: drawn here, then put back.
    let mut picker = if picking(app) == Some(dialog.id) { app.dialog.take() } else { None };
    let mut claim = None;
    let icon = dialog.icon.clone();
    let (closed, action) = ui_kit::Window::new(dialog_window_id(dialog, app.preferences.plugin_remember_dialog_position), &icon, &title)
        .fields()
        .actions(&run_label, &close_label, None)
        .show(ctx, ready.is_ok() && !running, |ui| {
            browse = body(app, ui, dialog, ready.as_ref().err(), &mut picker, &mut claim);
        });
    if let Some(mut picker) = picker {
        take_picks(dialog, &picker, &app.session.doc.clone());
        if dialog.picker_fields != dialog.selection_keys() {
            let active = dialog.picker_fields.get(picker.active).cloned();
            let listed = picker.listed.and_then(|i| dialog.picker_fields.get(i)).cloned();
            picker.inputs = dialog.selection_inputs();
            picker.active = active.and_then(|key| dialog.picker_fields.iter().position(|k| k == &key)).unwrap_or(usize::MAX);
            picker.listed = listed.and_then(|key| dialog.picker_fields.iter().position(|k| k == &key));
            picker.list_hover = None;
        }
        app.dialog = Some(picker);
    }
    for (handle, key) in [("plugin_dialog:run", "dialog:ok"), ("plugin_dialog:cancel", "dialog:cancel")] {
        if let Some(at) = crate::scenario::handle_at(key) {
            crate::scenario::publish_handle(handle, at);
        }
    }
    if closed || action == ui_kit::Action::Cancel {
        // A run still computing is stopped: nothing of it is applied.
        if let Some(running) = &dialog.running {
            running.ticket.stop();
        }
        return (false, false, None);
    }
    if let Some(key) = browse {
        let id = dialog.id;
        app.pick_folder("", move |app, path| {
            if let Some(path) = path {
                apply_browsed_folder(app, id, &key, path);
            }
        });
    }
    (true, action == ui_kit::Action::Ok, claim)
}

/// Write a picked folder into that dialog's directory field. A missing picker answers nothing.
fn apply_browsed_folder(app: &mut SolveApp, id: u64, key: &str, path: String) {
    let Some(dialog) = app.plugin_menu.dialogs.iter_mut().find(|dialog| dialog.id == id) else { return };
    let Some(fields) = dialog.fields.as_mut() else { return };
    let Some((field, input)) = fields.iter_mut().find(|(field, _)| field.key == key) else { return };
    if matches!(field.kind, Kind::Text { directory: true, .. })
        && let Input::Text(text) = input
    {
        *text = path;
    }
}

/// Draws the dialog. Returns the key of a directory field whose Browse button was pressed.
fn body(
    app: &SolveApp,
    ui: &mut egui::Ui,
    dialog: &mut EntryDialog,
    why_not: Option<&String>,
    picker: &mut Option<crate::dialogs::Dialog>,
    claim: &mut Claim,
) -> Option<String> {
    let field_errors = dialog.form_guard.as_ref().map(|_| dialog.field_errors(app)).unwrap_or_default();
    let id = dialog.id;
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
    let mut browse = None;
    match dialog.fields.as_mut() {
        None => {
            ui_kit::status_line(ui, app.tr("ui.plugin_dialog_unsupported"), ui_kit::StatusTone::Warning);
        }
        Some(fields) if fields.is_empty() => {
            ui_kit::text(ui, app.tr("ui.plugin_dialog_no_inputs"), ui_kit::Text::Note("plugin_no_inputs"));
        }
        Some(fields) => {
            ui.add_enabled_ui(editable, |ui| {
                let mut picks = 0;
                for (field, input) in fields.iter_mut() {
                    let pick_index = picks;
                    if !field.visible {
                        if matches!(input, Input::Selection(_)) {
                            picks += 1;
                        }
                        continue;
                    }
                    let before = input.clone();
                    ui.add_enabled_ui(field.enabled, |ui| {
                        let mut row = |ui: &mut egui::Ui| {
                            if let Input::Selection(items) = input {
                                let slot = picker.as_ref().and_then(|_| dialog.picker_fields.iter().position(|key| key == &field.key));
                                let mut idle = None;
                                let (index, target) = if let Some(index) = slot { (index, &mut *picker) } else { (pick_index, &mut idle) };
                                selection_row(app, ui, id, index, field, items, target, claim);
                            } else if field_row(app, ui, plugin, field, input) {
                                browse = Some(field.key.clone());
                            }
                        };
                        if let Some(tip) = field.tip.as_deref() {
                            ui_kit::with_row_label_tooltip(ui, tip, row);
                        } else {
                            row(ui);
                        }
                    });
                    field.edited |= *input != before;
                    if matches!(input, Input::Selection(_)) {
                        picks += 1;
                    }
                    if let Some(error) = field_errors.get(&field.key) {
                        ui_kit::status_line(ui, error, ui_kit::StatusTone::Error);
                    }
                }
            });
            refresh_conditions(fields);
        }
    }
    // Preview: a command that edits the design, as the native tools have it.
    if !dialog.read && dialog.opens.is_none() && dialog.fields.is_some() {
        ui.add_enabled_ui(editable, |ui| ui_kit::checkbox(ui, app.tr_label("Preview"), &mut dialog.preview));
        if dialog.preview
            && editable
            && app.preview.error.is_some()
            && preview_commands(app).is_some_and(|(id, _, _)| id == dialog.id)
            && let Some(error) = &app.preview.error
        {
            ui_kit::status_line(ui, &solidcraft_ui_text::format("ui.plugin_dialog_preview_failed", &[("error", error)]), ui_kit::StatusTone::Dim);
        }
    }
    if dialog.changed {
        let key = if dialog.revalidated { "ui.plugin_edit_revalidated" } else { "ui.plugin_edit_changed" };
        ui_kit::status_line(ui, app.tr(key), ui_kit::StatusTone::Warning);
    }
    if let Some(hook) = &dialog.hook {
        if hook.note {
            ui_kit::status_line(ui, app.tr("ui.plugin_hook_unavailable"), ui_kit::StatusTone::Dim);
        } else if hook.pending.is_some() && crate::now_ms() - hook.changed_at >= 500.0 {
            // A fast hook shows no status line, so the rows under the fields do not move.
            ui_kit::status_line(ui, app.tr("ui.plugin_hook_waiting"), ui_kit::StatusTone::Dim);
        }
    }
    if dialog.picks_cleared {
        ui_kit::status_line(ui, app.tr("ui.plugin_dialog_picks_cleared"), ui_kit::StatusTone::Warning);
    }
    if dialog.running.as_ref().is_some_and(|running| running.received.is_none()) {
        ui_kit::status_line(ui, app.tr("ui.plugin_dialog_running"), ui_kit::StatusTone::Dim);
    } else if let Some(error) = &dialog.error {
        ui_kit::status_line(ui, error, ui_kit::StatusTone::Error);
    } else if let Some(why) = why_not
        && dialog.fields.is_some()
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
    browse
}

/// Selection field `index` (among the dialog's selection fields), drawn by the native dialogs'
/// own pick control: through `picker` while the viewport picks into this dialog, else from its
/// picks, where clearing or removing one still works and a click asks for the viewport.
#[allow(clippy::too_many_arguments)]
fn selection_row(
    app: &SolveApp,
    ui: &mut egui::Ui,
    id: u64,
    index: usize,
    field: &Field,
    picks: &mut Vec<Sel>,
    picker: &mut Option<crate::dialogs::Dialog>,
    claim: &mut Claim,
) {
    let names = app.preferences.origin_plane_names;
    // The chip, for scenarios and the agent: the right end of the field's first line.
    let (top, row) = (ui.cursor().top(), solidcraft_theme::Tokens::get().control_height);
    let width = ui.available_width();
    let left = ui.cursor().left();
    crate::scenario::publish_handle(&format!("plugin_dialog:field:{}", field.key), egui::pos2(left + width * 0.8, top + row / 2.0));
    if let Some(picker) = picker {
        crate::dialogs::input_rows_range(picker, ui, &app.session, names, index..index + 1);
        return;
    }
    let mut idle = crate::dialogs::Dialog::new(crate::dialogs::Kind::PluginPick { dialog: id }, vec![selection_input(field, picks)]);
    idle.active = usize::MAX;
    crate::dialogs::input_rows_range(&mut idle, ui, &app.session, names, 0..1);
    if let Some(input) = idle.inputs.first() {
        picks.clone_from(&input.items);
    }
    if idle.active != usize::MAX || idle.listed.is_some() {
        *claim = Some((index, idle.listed.is_some()));
    }
}

/// Draws one field. Returns whether its Browse button was pressed. `plugin` gives the package's
/// pictures and swatches, used by a choice whose options name them.
fn field_row(app: &SolveApp, ui: &mut egui::Ui, plugin: &Listed, field: &Field, input: &mut Input) -> bool {
    let label = if field.required { format!("{} *", field.label) } else { field.label.clone() };
    let suffix = |unit: &Option<String>| unit.as_ref().map(|u| format!(" {u}")).unwrap_or_default();
    if let Input::Raw(text) = input {
        let response = ui_kit::row(ui, &label, |ui| ui_kit::text_field_reserving(ui, text, "", 0.0).1);
        crate::scenario::publish_handle(&format!("plugin_dialog:field:{}", field.key), response.rect.center());
        if response.changed()
            && let Ok(value) = serde_json::from_str(text)
            && let Some(value) = stored_input(field, &value)
        {
            *input = value;
        }
        return false;
    }
    let mut browse = false;
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
        (Kind::Choice { titles, .. }, Input::Choice(chosen)) => {
            // Until something is chosen the dropdown leads with "Choose…", which is not a value.
            let invalid = chosen.is_some_and(|i| !field.available.contains(&i));
            let choose = if invalid {
                chosen.and_then(|i| titles.get(i)).cloned().unwrap_or_else(|| app.tr("ui.plugin_dialog_choose").to_owned())
            } else {
                app.tr("ui.plugin_dialog_choose").to_owned()
            };
            let lead = chosen.is_none() || invalid;
            let shown: Vec<&str> = lead
                .then_some(choose.as_str())
                .into_iter()
                .chain(field.available.iter().filter_map(|i| titles.get(*i).map(String::as_str)))
                .collect();
            let mut index = chosen.and_then(|i| field.available.iter().position(|v| *v == i)).unwrap_or(0);
            let id = format!("plugin_field_{}", field.key);
            // Every option's picture from the package, when each has one that passed the check.
            let pictures: Option<Vec<&str>> = (!field.images.is_empty())
                .then(|| {
                    field
                        .available
                        .iter()
                        .map(|i| field.images.get(*i).and_then(|rel| plugin.images.get(rel)).map(String::as_str))
                        .collect::<Option<Vec<&str>>>()
                })
                .flatten();
            let swatches = plugin.swatches.get();
            if swatches.is_none() && field.swatches.iter().any(Option::is_some) {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
            // The picker uses visible indices; the form and plugin always use schema indices.
            let tiles: Vec<_> = field
                .available
                .iter()
                .map(|i| field.swatches.get(*i).and_then(Option::as_ref).and_then(|rel| swatches?.get(rel)).map(|image| image.texture(ui.ctx())))
                .collect();
            if tiles.iter().any(Option::is_some) {
                let titles: Vec<String> = field.available.iter().filter_map(|i| titles.get(*i).cloned()).collect();
                let mut selected = chosen.and_then(|i| field.available.iter().position(|v| *v == i));
                let before_selected = selected;
                let response = ui_kit::row(ui, &label, |ui| {
                    ui_kit::swatch_picker(
                        ui,
                        &id,
                        &mut selected,
                        ui_kit::SwatchPicker { titles: &titles, textures: &tiles, choose: &choose, search_hint: app.tr("ui.plugin_swatch_search") },
                    )
                });
                if selected != before_selected {
                    *chosen = selected.and_then(|i| field.available.get(i).copied());
                }
                response
            } else {
                let before_index = index;
                let response = match pictures {
                    Some(pictures) => {
                        // "Choose…" has the generic plugin icon.
                        let lead_icon = lead.then_some(crate::plugin_images::GENERIC);
                        let options: Vec<(&str, &str)> = shown.iter().copied().zip(lead_icon.into_iter().chain(pictures)).collect();
                        ui_kit::row(ui, &label, |ui| ui_kit::dropdown_icons(ui, &id, &mut index, &options))
                    }
                    None => ui_kit::row(ui, &label, |ui| ui_kit::dropdown(ui, &id, &mut index, &shown)),
                };
                if index != before_index {
                    *chosen = (if lead { index.checked_sub(1) } else { Some(index) }).and_then(|i| field.available.get(i).copied());
                }
                response
            }
        }
        (Kind::Text { directory: true, .. }, Input::Text(text)) => {
            let hint = if field.required { app.tr("ui.plugin_dialog_required_hint") } else { "" };
            // The browser build has no folder picker, so Browse is not offered there.
            let can_browse = !cfg!(target_arch = "wasm32") && app.services.pick_folder.is_some();
            ui_kit::row(ui, &label, |ui| {
                if can_browse {
                    let browse_label = app.tr("ui.plugin_dialog_browse");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let button = ui_kit::button(ui, browse_label, ui_kit::Button::Normal);
                        browse = button.clicked();
                        let field = ui_kit::text_field_reserving(ui, text, hint, 0.0).1;
                        field.union(button)
                    })
                    .inner
                } else {
                    ui_kit::text_field_reserving(ui, text, hint, 0.0).1
                }
            })
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
    browse
}

#[cfg(test)]
#[path = "plugin_menu_tests.rs"]
mod tests;
