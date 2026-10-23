//! The Play Macro prompt: the values a macro asks for, then `macro.play`.
//!
//! A Rhai macro declares `// @param name Label [kind] [options] | default [@if gate]`
//! (docs/macros.md). Playing one shows one row per parameter, in the control its kind asks
//! for — text, whole number, number, length, angle, toggle or dropdown — prefilled with its
//! default, and runs the macro with what is typed. A parameter gated with `@if name=v1,v2`
//! stays hidden until the watched parameter holds one of those values. The engine checks
//! every value again (`cmd::macros`), so a headless caller gets the same rules without any
//! of this window.

use serde_json::{Value, json};

use crate::SolveApp;
use crate::i18n::{tr, tr_label};
use crate::ui_kit;
use solidcraft_engine::cmd::macros::{MacroParam, ParamGate, ParamKind};
use solidcraft_engine::doc::expr::Kind;

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
    /// One row per declared parameter: name, label, typed draft and `@if` gate.
    pub fields: Vec<(String, String, Draft, Option<ParamGate>)>,
    pub error: Option<String>,
}

/// Open the prompt for the loaded macro (no-op when none is loaded).
pub fn open(app: &mut SolveApp) {
    let Some(m) = app.session.macros.loaded.clone() else { return };
    app.macro_run = Some(MacroRun {
        name: m.name.clone().unwrap_or_default(),
        description: m.description.clone(),
        fields: m.params.iter().map(|p| (p.name.clone(), p.label.clone().unwrap_or_else(|| p.name.clone()), draft(app, p), p.gate.clone())).collect(),
        error: None,
    });
}

/// The starting value of one parameter: its default, read the way its kind reads. A length
/// or angle default may be an expression ("5 mm", `width + 2`): it is evaluated once here,
/// and what the user types goes to the commands as a plain number in the design's units.
fn draft(app: &SolveApp, p: &MacroParam) -> Draft {
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
    let d = p.default.clone().unwrap_or(Value::Null);
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

/// Draw the prompt when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.macro_run.take() else { return };
    let (units, decimals) = (app.session.doc.units.clone(), app.preferences.length_decimals);
    let title = if d.name.is_empty() { tr_label("Play Macro").to_string() } else { d.name.clone() };
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_macro_run"), "play", &title)
        .fields()
        .actions(tr("ui.run"), tr("ui.cancel"), None)
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
    if matches!(action, ui_kit::Action::Ok) {
        let mut params = json!({});
        for i in 0..d.fields.len() {
            if visible(&d.fields, i, 0) {
                params[&d.fields[i].0] = value(&d.fields[i].2);
            }
        }
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
