//! The Play Macro prompt: the values a macro asks for, then `macro.play`.
//!
//! A Rhai macro declares `// @param name Label [kind] [options] | default [@if gate]`
//! (docs/macros.md). Playing one shows one row per parameter, in the control its kind asks
//! for — text, whole number, number, length, angle, toggle or dropdown — prefilled with its
//! default, and runs the macro with what is typed. A parameter gated with `@if name=v1,v2`
//! stays hidden until the watched parameter holds one of those values. The engine checks
//! every value again (`cmd::macros`), so a headless caller gets the same rules without any
//! of this window.
//!
//! The values you last ran with are remembered per macro (by name and source hash, so an
//! edited macro starts afresh) and survive a restart with the preferences: change one row,
//! Play, undo, Play again with the rest as you left them. "Reset to defaults" restores the
//! macro's declared defaults and forgets the remembered set.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use serde_json::{Value, json};

use crate::SolveApp;
use crate::i18n::{tr, tr_label};
use crate::ui_kit;
use solidcraft_engine::cmd::macros::{MacroParam, ParamGate, ParamKind};
use solidcraft_engine::doc::expr::Kind;

/// How many macros remember their values before the oldest give way to the newest.
const MAX_REMEMBERED: usize = 256;

/// The key remembered values are stored under: the macro's name and a hash of its source, so
/// an edited macro starts afresh instead of replaying values for a different parameter list.
pub fn macro_key(name: &str, source: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    source.hash(&mut h);
    format!("{}:{:016x}", name, h.finish())
}

/// One parameter's draft value, in the shape its kind asks for.
#[derive(Clone, Debug)]
pub enum Draft {
    Text(String),
    Int(i64),
    Number(f64),
    Length(f64),
    Angle(f64),
    Bool(bool),
    Choice { selected: usize, options: Vec<String> },
}

/// Dialog state: the typed values (the macro itself lives in the session).
#[derive(Default)]
pub struct MacroRun {
    pub name: String,
    pub description: Option<String>,
    /// Where this macro's remembered values live in [`SolveApp::macro_values`].
    pub key: String,
    /// One row per declared parameter: name, label, typed draft and `@if` gate.
    pub fields: Vec<(String, String, Draft, Option<ParamGate>)>,
    pub error: Option<String>,
}

/// Open the prompt for the loaded macro (no-op when none is loaded). Rows start from the
/// values this macro last ran with (each re-checked against its kind, so a macro that changed
/// since falls back to its defaults), and from its defaults when nothing is remembered.
pub fn open(app: &mut SolveApp) {
    let Some(m) = app.session.macros.loaded.clone() else { return };
    let key = macro_key(m.name.as_deref().unwrap_or_default(), &m.source);
    let stored = app.macro_values.get(&key).cloned();
    app.macro_run = Some(MacroRun {
        name: m.name.clone().unwrap_or_default(),
        description: m.description.clone(),
        key,
        fields: m
            .params
            .iter()
            .map(|p| {
                (p.name.clone(), p.label.clone().unwrap_or_else(|| p.name.clone()), draft(app, p, start_value(p, stored.as_ref())), p.gate.clone())
            })
            .collect(),
        error: None,
    });
}

/// The value one row starts from: the remembered value if it still fits the parameter's kind,
/// else the declared default. A remembered value that no longer fits is dropped like one the
/// macro never declared.
fn start_value<'a>(p: &'a MacroParam, stored: Option<&'a Value>) -> Option<&'a Value> {
    stored.and_then(|s| s.get(&p.name)).filter(|v| p.coerce(v).is_ok()).or(p.default.as_ref())
}

/// The starting value of one parameter, read the way its kind reads. A length or angle
/// default may be an expression ("5 mm", `width + 2`): it is evaluated once here, and what
/// the user types goes to the commands as a plain number in the design's units.
fn draft(app: &SolveApp, p: &MacroParam, from: Option<&Value>) -> Draft {
    let number = |v: &Value| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().or_else(|| app.session.doc.eval(s, Kind::Unitless).ok()),
        _ => None,
    };
    let length = |v: &Value| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => app.session.doc.eval(s, Kind::Length).ok().or_else(|| s.trim().parse::<f64>().ok()),
        _ => None,
    };
    let angle = |v: &Value| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => app.session.doc.eval(s, Kind::Angle).ok().map(|rad| rad.to_degrees()),
        _ => None,
    };
    let d = from.cloned().unwrap_or(Value::Null);
    match p.kind {
        ParamKind::Text => Draft::Text(match &d {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }),
        ParamKind::Int => Draft::Int(number(&d).map(|x| x.round() as i64).unwrap_or(0)),
        ParamKind::Number => Draft::Number(number(&d).unwrap_or(0.0)),
        ParamKind::Length => Draft::Length(length(&d).unwrap_or(0.0)),
        ParamKind::Angle => Draft::Angle(angle(&d).unwrap_or(0.0)),
        ParamKind::Bool => Draft::Bool(match &d {
            Value::Bool(b) => *b,
            Value::String(s) => matches!(s.as_str(), "true" | "yes" | "1" | "on"),
            _ => false,
        }),
        ParamKind::Choice => {
            Draft::Choice { selected: d.as_str().and_then(|s| p.options.iter().position(|o| o == s)).unwrap_or(0), options: p.options.clone() }
        }
    }
}

/// The JSON value one draft hands to `macro.play` (the engine checks it again).
fn value(draft: &Draft) -> Value {
    match draft {
        Draft::Text(s) => json!(s),
        Draft::Int(i) => json!(i),
        Draft::Number(n) => json!(n),
        Draft::Length(mm) => json!(mm),
        Draft::Angle(deg) => json!(deg),
        Draft::Bool(b) => json!(b),
        Draft::Choice { selected, options } => json!(options.get(*selected).cloned().unwrap_or_default()),
    }
}

/// The text an `@if` gate compares a draft against.
fn shown(draft: &Draft) -> String {
    match value(draft) {
        Value::String(s) => s,
        other => other.to_string(),
    }
}

/// A parameter shows when it has no gate, or the parameter its gate watches is itself
/// visible and holds one of the gate's values (so hiding `hand` also hides the hub rows
/// that watch it). A gate watching an unknown parameter shows the row — the body guards
/// itself. The depth cap breaks a cyclic gate (a watches b, b watches a): a cycle hides
/// both rows rather than looping.
fn visible(fields: &[(String, String, Draft, Option<ParamGate>)], idx: usize, depth: usize) -> bool {
    let Some(gate) = &fields[idx].3 else { return true };
    if depth > fields.len() {
        return false;
    }
    match fields.iter().position(|f| f.0 == gate.name) {
        Some(w) => gate.values.iter().any(|v| *v == shown(&fields[w].2)) && visible(fields, w, depth + 1),
        None => true,
    }
}

/// Remember every row's value (hidden gated rows included, so they are there when they
/// reappear) under this macro's key, bounding how many macros remember anything.
fn remember(app: &mut SolveApp, key: &str, fields: &[(String, String, Draft, Option<ParamGate>)]) {
    let mut all = json!({});
    for (name, _, draft, _) in fields {
        all[name] = value(draft);
    }
    if app.macro_values.len() >= MAX_REMEMBERED && !app.macro_values.contains_key(key) {
        let oldest = app.macro_values.keys().next().cloned();
        if let Some(k) = oldest {
            app.macro_values.remove(&k);
        }
    }
    app.macro_values.insert(key.to_string(), all);
}

/// Clean remembered values read back from the preferences file: plain names, plain scalars,
/// bounded counts. Anything odd is dropped, never trusted.
pub(crate) fn clean_macro_values(v: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Some(map) = v.as_object() else { return out };
    for (key, vals) in map.iter().take(MAX_REMEMBERED) {
        if key.is_empty() || key.len() > 256 {
            continue;
        }
        let Some(vals) = vals.as_object() else { continue };
        let mut clean = serde_json::Map::new();
        for (name, val) in vals.iter().take(100) {
            let ok = match val {
                Value::String(s) => s.chars().count() <= 512 && !s.chars().any(char::is_control),
                Value::Number(n) => n.as_f64().is_some_and(|x| x.is_finite()),
                Value::Bool(_) => true,
                _ => false,
            };
            if ok && !name.is_empty() && name.len() <= 64 {
                clean.insert(name.clone(), val.clone());
            }
        }
        if !clean.is_empty() {
            out.insert(key.clone(), Value::Object(clean));
        }
    }
    out
}

/// Draw the prompt when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.macro_run.take() else { return };
    let (units, decimals) = (app.session.doc.units.clone(), app.preferences.length_decimals);
    let title = if d.name.is_empty() { tr_label("Play Macro").to_string() } else { d.name.clone() };
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_macro_run"), "play", &title)
        .fields()
        .actions(tr("ui.run"), tr("ui.cancel"), Some(tr("macro.reset_defaults")))
        .show(ctx, true, |ui| {
            if let Some(text) = &d.description {
                ui_kit::text(ui, text, ui_kit::Text::Note("macro_run.description"));
            }
            for i in 0..d.fields.len() {
                if !visible(&d.fields, i, 0) {
                    continue;
                }
                let (_, label, draft, _) = &mut d.fields[i];
                match draft {
                    Draft::Text(value) => {
                        ui_kit::row(ui, label, |ui| ui_kit::field(ui, value));
                    }
                    Draft::Int(value) => {
                        ui_kit::row(ui, label, |ui| ui_kit::number_field(ui, value, i64::MIN..=i64::MAX, 1.0, ""));
                    }
                    Draft::Number(value) => {
                        ui_kit::row(ui, label, |ui| ui_kit::number_field(ui, value, -1e12..=1e12, 0.1, ""));
                    }
                    Draft::Length(value) => {
                        ui_kit::row(ui, label, |ui| ui_kit::length_field(ui, value, &units, decimals));
                    }
                    Draft::Angle(value) => {
                        ui_kit::row(ui, label, |ui| ui_kit::angle_field(ui, value));
                    }
                    Draft::Bool(on) => {
                        *on = ui_kit::checkbox_row(ui, label, label, *on);
                    }
                    Draft::Choice { selected, options } => {
                        let names: Vec<&str> = options.iter().map(String::as_str).collect();
                        ui_kit::row(ui, label, |ui| ui_kit::dropdown(ui, label, selected, &names));
                    }
                }
            }
            if let Some(e) = &d.error {
                ui_kit::text(ui, e, ui_kit::Text::Error("macro_run.error"));
            }
        });
    let mut keep = matches!(action, ui_kit::Action::None) && !closed;
    if matches!(action, ui_kit::Action::Apply) {
        // Reset to defaults: every row back to what the macro declares, and the remembered
        // values for this macro forgotten, so the next Play starts from the author's defaults.
        if let Some(m) = app.session.macros.loaded.clone() {
            d.fields = m
                .params
                .iter()
                .map(|p| (p.name.clone(), p.label.clone().unwrap_or_else(|| p.name.clone()), draft(app, p, p.default.as_ref()), p.gate.clone()))
                .collect();
        }
        app.macro_values.remove(&d.key);
        d.error = None;
        keep = true;
    }
    if matches!(action, ui_kit::Action::Ok) {
        let mut params = json!({});
        for i in 0..d.fields.len() {
            if visible(&d.fields, i, 0) {
                params[&d.fields[i].0] = value(&d.fields[i].2);
            }
        }
        remember(app, &d.key, &d.fields);
        match app.run("macro.play", json!({"params": params})) {
            Ok(r) => {
                let steps = r.get("steps").and_then(Value::as_u64).unwrap_or(0);
                app.set_status(crate::i18n::format("macro.played", &[("steps", &steps.to_string())]), false);
            }
            Err(e) => {
                d.error = Some(e);
                keep = true;
            }
        }
    }
    if keep {
        app.macro_run = Some(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_keys_follow_the_source() {
        let a = macro_key("Gear", "run(\"a\");");
        assert_eq!(a, macro_key("Gear", "run(\"a\");"));
        assert_ne!(a, macro_key("Gear", "run(\"b\");"), "an edited macro starts afresh");
        assert_ne!(a, macro_key("Plate", "run(\"a\");"));
    }

    #[test]
    fn remembered_values_survive_cleaning_and_garbage_is_dropped() {
        let stored = json!({
            "Gear:0000000000000001": { "teeth": 12, "mat": "steel", "hub": true },
            "": { "x": 1 },
            "Bad": "not an object",
            "Gear:0000000000000002": { "evil": "a\nb", "deep": { "n": 1 }, "n": 1e2 },
        });
        let clean = clean_macro_values(&stored);
        assert_eq!(clean.len(), 2, "{clean:?}");
        assert_eq!(clean["Gear:0000000000000001"]["mat"], "steel");
        assert_eq!(clean["Gear:0000000000000002"]["n"], 1e2);
        assert!(clean["Gear:0000000000000002"].get("evil").is_none(), "control characters never come back");
        assert!(clean["Gear:0000000000000002"].get("deep").is_none(), "only plain scalars");
        assert!(clean_macro_values(&json!("garbage")).is_empty());
    }
}
