//! The import report: after an importer plugin opens a file, what it could not carry over, in the
//! app's own window. The plugin returns the report as data; the app draws it (RFC 0002: plugins
//! ship no UI code). Programs get the same report in `doc.open`'s and `file.import`'s reply and
//! never see this window.

use serde_json::Value;
use solidcraft_engine::plugin::{LossReport, LossSeverity};
use solidcraft_ui_text::{format, tr};

use crate::{SolveApp, ui_kit};

/// The report waiting to be read.
pub(crate) struct Shown {
    /// The design's name and the plugin that made it.
    pub(crate) design: String,
    pub(crate) plugin: String,
    pub(crate) report: LossReport,
}

/// Show `result`'s loss report, when it is the reply of an import by a plugin.
pub(crate) fn offer(app: &mut SolveApp, result: &Value) {
    let Some(report) = result.get("loss_report").and_then(|r| serde_json::from_value::<LossReport>(r.clone()).ok()) else { return };
    let text = |key: &str| result.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let plugin = text("plugin");
    app.set_status(
        if report.summary.is_empty() {
            format("import_report.opened", &[("name", &text("name")), ("plugin", &plugin)])
        } else {
            format("import_report.opened_summary", &[("name", &text("name")), ("plugin", &plugin), ("summary", &report.summary)])
        },
        false,
    );
    app.import_report = Some(Shown { design: text("name"), plugin, report });
}

fn severity_label(severity: LossSeverity) -> &'static str {
    match severity {
        LossSeverity::Dropped => "import_report.dropped",
        LossSeverity::Degraded => "import_report.degraded",
        LossSeverity::Info => "import_report.info",
    }
}

/// The window, while a report waits.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(shown) = app.import_report.as_ref() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let title = tr("import_report.title").to_string();
    let ok = tr("import_report.ok").to_string();
    let heading = format("import_report.heading", &[("name", &shown.design), ("plugin", &shown.plugin)]);
    let counts = [LossSeverity::Dropped, LossSeverity::Degraded, LossSeverity::Info]
        .map(|severity| (severity, shown.report.items.iter().filter(|i| i.severity == severity).count()));
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_import_report"), "import", &title)
        .tone(if counts[0].1 > 0 { ui_kit::Tone::Warning } else { ui_kit::Tone::Info })
        .actions(&ok, "", None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &heading, ui_kit::Text::Heading("import_report_heading"));
            if !shown.report.summary.is_empty() {
                ui_kit::text(ui, &shown.report.summary, ui_kit::Text::Report("import_report_summary", false));
            }
            if shown.report.items.is_empty() {
                ui_kit::text(ui, tr("import_report.nothing_lost"), ui_kit::Text::Hint("import_report_nothing"));
                return;
            }
            let tally: Vec<String> =
                counts.iter().filter(|(_, n)| *n > 0).map(|(severity, n)| std::format!("{} {n}", tr(severity_label(*severity)))).collect();
            ui_kit::text(ui, &tally.join(" · "), ui_kit::Text::Hint("import_report_tally"));
            egui::ScrollArea::vertical().id_salt("import_report_items").max_height(320.0).show(ui, |ui| {
                // The worst first: what was left out, then what changed, then notes.
                for (severity, _) in counts {
                    for (i, item) in shown.report.items.iter().enumerate().filter(|(_, i)| i.severity == severity) {
                        let line = if item.subject.is_empty() {
                            std::format!("{}: {}", tr(severity_label(severity)), item.message)
                        } else {
                            std::format!("{}: {} — {}", tr(severity_label(severity)), item.subject, item.message)
                        };
                        ui.push_id(i, |ui| ui_kit::text(ui, &line, ui_kit::Text::Report("import_report_item", severity == LossSeverity::Dropped)));
                    }
                }
            });
        });
    if closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel) {
        app.import_report = None;
    }
}
