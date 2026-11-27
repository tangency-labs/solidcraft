//! Insert from an ordering code: pick an installed generator, type a manufacturer's ordering
//! code, see at once what it means (its fields, the configuration row and parameter values it
//! gives, where the row's numbers come from, or the generator's own reason for refusing it), change
//! a choice field to see the code it makes, and insert the part. The dialog only reads the
//! package; `generator.insert` does the work (docs/formats/generator.md, RFC 0017).

use serde_json::{Value as Json, json};
use solidcraft_engine::generator::code_template::FieldKind;
use solidcraft_engine::generator::{Package, Refusal, RefusalKind, Resolved, installed};

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::selection::SelInput;
use crate::ui_kit::{Text, choice_row, text, text_row, value_row};

/// What the dialog holds.
#[derive(Clone, Debug, Default)]
pub struct Gen {
    /// The installed packages, read once when the dialog first draws.
    pub packages: Option<Vec<installed::Installed>>,
    /// Which of them is chosen.
    pub package: usize,
    /// The ordering code as typed.
    pub code: String,
    /// What the code meant, for the (package, code) it was read for.
    reading: Option<(usize, String, Result<Resolved, Refusal>)>,
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "generator.insert").then(|| (Kind::Generator(Gen::default()), Vec::new()))
}

/// A field's value as the person reads it.
fn shown(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        Json::Object(o) => {
            o.iter().map(|(k, x)| if x == &Json::Bool(true) { k.clone() } else { format!("{k} {}", shown(x)) }).collect::<Vec<_>>().join(", ")
        }
        other => other.to_string(),
    }
}

/// Where a row's numbers come from, as one line: `Catalog, page 12; Verified`.
fn row_line(r: &Resolved) -> Option<String> {
    let row = r.row.as_ref()?;
    let mut parts: Vec<String> = Vec::new();
    if !row.source.is_empty() {
        match &row.page {
            Some(page) => parts.push(format!("{}, {page}", row.source)),
            None => parts.push(row.source.clone()),
        }
    }
    parts.push(solidcraft_ui_text::tr(&format!("ui.generator.status.{}", row.status)).to_string());
    if !row.note.is_empty() {
        parts.push(row.note.clone());
    }
    Some(parts.join("; "))
}

/// The values a choice or letter-map field offers.
fn options(kind: &FieldKind) -> Option<Vec<String>> {
    match kind {
        FieldKind::Choice(values) => Some(values.clone()),
        FieldKind::Map(pairs) => Some(pairs.iter().map(|(_, v)| v.clone()).collect()),
        _ => None,
    }
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, k: &mut Gen) -> bool {
    let packages = k.packages.get_or_insert_with(|| installed::list().0);
    let Some(chosen) = packages.get(k.package.min(packages.len().saturating_sub(1))).cloned() else {
        let folder = installed::dir().map(|d| d.display().to_string()).unwrap_or_default();
        text(ui, &app.tr("ui.generator.none").replace("{folder}", &folder), Text::Hint("generator_none"));
        return false;
    };
    k.package = k.package.min(packages.len().saturating_sub(1));
    let names: Vec<&str> = packages.iter().map(|p| p.package.name.as_str()).collect();
    choice_row(ui, app.tr("ui.generator.generator"), "generator_package", &mut k.package, &names);
    let g: &Package = &chosen.package;
    if !g.description.is_empty() {
        text(ui, &g.description, Text::Note("generator_description"));
    }
    let (enter, _) = text_row(ui, app.tr("ui.generator.code"), &mut k.code, app.tr("ui.generator.code_hint"));
    let stale = !matches!(&k.reading, Some((i, c, _)) if *i == k.package && *c == k.code);
    if stale {
        k.reading = Some((k.package, k.code.clone(), g.parse(&k.code)));
    }
    if k.code.trim().is_empty() {
        text(ui, app.tr("ui.generator.type_a_code"), Text::Hint("generator_type_a_code"));
        return enter;
    }
    match k.reading.as_ref().map(|r| &r.2) {
        Some(Ok(r)) => {
            if r.code != k.code.trim() {
                value_row(ui, app.tr("ui.generator.written_as"), &r.code);
            }
            if let Some(t) = g.templates.get(r.template) {
                value_row(ui, app.tr("ui.generator.template"), t.template.source());
            }
            // A choice field can be changed here: the code is written again from the fields.
            let mut edited: Option<(String, String)> = None;
            for (name, value) in &r.fields {
                match g.declared.get(name).and_then(|d| options(&d.kind)) {
                    Some(list) => {
                        let current = shown(value);
                        let mut at = list.iter().position(|o| *o == current).unwrap_or(0);
                        let before = at;
                        let labels: Vec<&str> = list.iter().map(String::as_str).collect();
                        choice_row(ui, name, &format!("generator_field_{name}"), &mut at, &labels);
                        if at != before
                            && let Some(new) = list.get(at)
                        {
                            edited = Some((name.clone(), new.clone()));
                        }
                    }
                    None => {
                        value_row(ui, name, &shown(value));
                    }
                }
            }
            if let Some((name, new)) = edited {
                let mut fields = r.fields.clone();
                fields.insert(name, Json::String(new));
                if let Ok((_, printed)) = g.code_for(&fields, None) {
                    k.code = printed;
                }
            }
            if let Some(row) = &r.configuration {
                value_row(ui, app.tr("ui.generator.configuration"), row);
            }
            if !r.parameters.is_empty() {
                let line = r.parameters.iter().map(|(n, v)| format!("{n} = {v}")).collect::<Vec<_>>().join(", ");
                value_row(ui, app.tr("ui.generator.parameters"), &line);
            }
            if let Some(d) = &r.description {
                value_row(ui, app.tr("ui.generator.makes"), d);
            }
            if let Some(line) = row_line(r) {
                value_row(ui, app.tr("ui.generator.source"), &line);
            }
            for w in &r.warnings {
                text(ui, w, Text::Note("generator_warning"));
            }
        }
        Some(Err(Refusal { kind, message })) => {
            let why = if *kind == RefusalKind::NotModelled { app.tr("ui.generator.not_modelled") } else { app.tr("ui.generator.refused") };
            text(ui, &format!("{why} {message}"), Text::Error("generator_refused"));
        }
        None => {}
    }
    enter
}

pub fn commands(app: &SolveApp, k: &Gen) -> Result<Vec<(String, Json)>, String> {
    let Some(chosen) = k.packages.as_ref().and_then(|l| l.get(k.package)) else { return Err(app.tr("ui.generator.pick").into()) };
    let code = k.code.trim();
    if code.is_empty() {
        return Err(app.tr("ui.generator.type_a_code").into());
    }
    chosen.package.parse(code).map_err(|r| r.message)?;
    Ok(vec![("generator.insert".into(), json!({"generator": chosen.path.to_string_lossy(), "code": code}))])
}

#[cfg(test)]
#[path = "dialogs_generator_tests.rs"]
mod tests;
