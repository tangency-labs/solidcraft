//! Insert from an ordering code: pick an installed generator, type a manufacturer's ordering
//! code, see at once what it means (its fields, the table rows it used with their provenance, or
//! the package's own reason for refusing it), and insert the part. The dialog only reads the
//! package; `generator.build` does the work (docs/formats/generator-package.md, RFC 0017).

use serde_json::{Value, json};
use solidcraft_engine::generator::{Generator, GeneratorError, Plan, RefusalKind, RowUsed, installed};

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
    reading: Option<(usize, String, Result<Plan, GeneratorError>)>,
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "generator.build").then(|| (Kind::Generator(Gen::default()), Vec::new()))
}

/// The package's source for a row, as one line: `Catalog page 12, approximate`.
fn row_line(g: &Generator, r: &RowUsed) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(id) = &r.source {
        let title = g.meta.sources.iter().find(|s| s.0 == *id).map_or(id.as_str(), |s| s.1.as_str());
        match &r.page {
            Some(page) => parts.push(format!("{title}, {page}")),
            None => parts.push(title.to_string()),
        }
    }
    let shown = |status: &str| solidcraft_ui_text::tr(&format!("ui.generator.status.{status}")).to_string();
    if let Some(status) = &r.status {
        parts.push(shown(status));
    }
    // A column with a weaker status than the row's says so: the person should know about it.
    let weak: Vec<&str> = r.column_status.iter().filter(|(_, s)| s.as_str() == "approximate").map(|(c, _)| c.as_str()).collect();
    if !weak.is_empty() {
        parts.push(format!("{}: {}", shown("approximate"), weak.join(", ")));
    }
    if let Some(note) = &r.note {
        parts.push(note.clone());
    }
    parts.join("; ")
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, k: &mut Gen) -> bool {
    let packages = k.packages.get_or_insert_with(|| installed::list().0);
    let Some(chosen) = packages.get(k.package.min(packages.len().saturating_sub(1))).cloned() else {
        let folder = installed::dir().map(|d| d.display().to_string()).unwrap_or_default();
        text(ui, &app.tr("ui.generator.none").replace("{folder}", &folder), Text::Hint("generator_none"));
        return false;
    };
    k.package = k.package.min(packages.len().saturating_sub(1));
    let names: Vec<&str> = packages.iter().map(|p| p.generator.meta.name.as_str()).collect();
    choice_row(ui, app.tr("ui.generator.generator"), "generator_package", &mut k.package, &names);
    let g = &chosen.generator;
    if !g.meta.description.is_empty() {
        text(ui, &g.meta.description, Text::Note("generator_description"));
    }
    if let Some(level) = &g.meta.guarantee {
        let shown = format!("{} ({})", app.tr(&format!("ui.generator.guarantee.{level}")), app.tr("ui.generator.declared"));
        value_row(ui, app.tr("ui.generator.guarantee"), &shown);
    }
    let (enter, _) = text_row(ui, app.tr("ui.generator.code"), &mut k.code, app.tr("ui.generator.code_hint"));
    let stale = !matches!(&k.reading, Some((i, c, _)) if *i == k.package && *c == k.code);
    if stale {
        k.reading = Some((k.package, k.code.clone(), g.plan(&k.code)));
    }
    if k.code.trim().is_empty() {
        text(ui, app.tr("ui.generator.type_a_code"), Text::Hint("generator_type_a_code"));
    } else {
        match k.reading.as_ref().map(|r| &r.2) {
            Some(Ok(plan)) => {
                for (name, value) in &plan.parsed.fields {
                    value_row(ui, name, &value.display());
                }
                if let Some(component) = &plan.component {
                    value_row(ui, app.tr("ui.generator.makes"), component);
                }
                for r in &plan.parsed.rows {
                    let line = row_line(g, r);
                    if !line.is_empty() {
                        value_row(ui, app.tr("ui.generator.source"), &line);
                    }
                }
                if !g.meta.provenance.is_empty() {
                    text(ui, &g.meta.provenance, Text::Note("generator_provenance"));
                }
            }
            Some(Err(GeneratorError::Refused { kind, message })) => {
                let why = if *kind == RefusalKind::NotModelled { app.tr("ui.generator.not_modelled") } else { app.tr("ui.generator.refused") };
                text(ui, &format!("{why} {message}"), Text::Error("generator_refused"));
            }
            Some(Err(e)) => {
                text(ui, &e.to_string(), Text::Error("generator_error"));
            }
            None => {}
        }
    }
    enter
}

pub fn commands(app: &SolveApp, k: &Gen) -> Result<Vec<(String, Value)>, String> {
    let Some(chosen) = k.packages.as_ref().and_then(|l| l.get(k.package)) else { return Err(app.tr("ui.generator.pick").into()) };
    let code = k.code.trim();
    if code.is_empty() {
        return Err(app.tr("ui.generator.type_a_code").into());
    }
    chosen.generator.parse(code).map_err(|e| e.to_string())?;
    Ok(vec![("generator.build".into(), json!({"package": chosen.path.to_string_lossy(), "code": code}))])
}

#[cfg(test)]
#[path = "dialogs_generator_tests.rs"]
mod tests;
