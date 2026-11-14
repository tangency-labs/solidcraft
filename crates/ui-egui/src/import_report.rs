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

/// The worst first: what was left out, then what changed, then notes.
const ORDER: [LossSeverity; 3] = [LossSeverity::Dropped, LossSeverity::Degraded, LossSeverity::Info];

/// The report's lines, worst first: one per kind of loss the plugin titled, with how many items it
/// covers, in the order each kind first appears; an untitled item is a line of its own.
pub(crate) fn lines(report: &LossReport) -> Vec<(LossSeverity, String)> {
    let mut lines = Vec::new();
    for severity in ORDER {
        let label = tr(severity_label(severity));
        let mut titled: Vec<(&str, usize)> = Vec::new();
        let mut untitled = Vec::new();
        for item in report.items.iter().filter(|i| i.severity == severity) {
            let title = item.title.trim();
            if title.is_empty() {
                untitled.push(item);
            } else if let Some((_, count)) = titled.iter_mut().find(|(t, _)| *t == title) {
                *count += 1;
            } else {
                titled.push((title, 1));
            }
        }
        for (title, count) in titled {
            let what = if count == 1 { title.to_string() } else { format("import_report.group", &[("title", title), ("count", &count.to_string())]) };
            lines.push((severity, std::format!("{label}: {what}")));
        }
        for item in untitled {
            let line = if item.subject.is_empty() {
                std::format!("{label}: {}", item.message)
            } else {
                std::format!("{label}: {} — {}", item.subject, item.message)
            };
            lines.push((severity, line));
        }
    }
    lines
}

/// Every item as the plugin gave it, worst first, with its code: what Details shows.
pub(crate) fn detail_lines(report: &LossReport) -> Vec<String> {
    let mut lines = Vec::new();
    for severity in ORDER {
        for item in report.items.iter().filter(|i| i.severity == severity) {
            let head = [item.code.as_str(), item.subject.as_str()].into_iter().filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join(" · ");
            lines.push(if head.is_empty() { item.message.clone() } else { std::format!("{head}: {}", item.message) });
        }
    }
    lines
}

/// The window, while a report waits.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(shown) = app.import_report.as_ref() else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let title = tr("import_report.title").to_string();
    let ok = tr("import_report.ok").to_string();
    let heading = format("import_report.heading", &[("name", &shown.design), ("plugin", &shown.plugin)]);
    let counts = ORDER.map(|severity| (severity, shown.report.items.iter().filter(|i| i.severity == severity).count()));
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
                for (i, (severity, line)) in lines(&shown.report).iter().enumerate() {
                    let strong = *severity == LossSeverity::Dropped;
                    ui.push_id(i, |ui| ui_kit::text(ui, line, ui_kit::Text::Report("import_report_item", strong)));
                }
                // The plugin's own wording and codes, for a bug report, folded away.
                ui_kit::section(ui, "import_report_details", tr("import_report.show_details"), false, |ui| {
                    for (i, line) in detail_lines(&shown.report).iter().enumerate() {
                        ui.push_id(("detail", i), |ui| ui_kit::text(ui, line, ui_kit::Text::Note("import_report_detail")));
                    }
                });
            });
        });
    if closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel) {
        app.import_report = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(severity: &str, title: &str, subject: &str, message: &str, code: &str) -> serde_json::Value {
        serde_json::json!({"severity":severity,"title":title,"subject":subject,"message":message,"code":code})
    }

    #[test]
    fn import_report_shows_one_line_per_titled_kind_and_codes_only_in_details() {
        crate::i18n::install();
        let report: LossReport = serde_json::from_value(serde_json::json!({"summary":"3 of 4 features","items":[
            item("info", "Parts of the file kept as they are", "f3d/source.unindexed-records-retained", "14 records kept", "f3d/source.unindexed-records-retained"),
            item("dropped", "Features without editable history", "feature \"Hex socket\"", "not mapped", ""),
            item("info", "Parts of the file kept as they are", "", "2 records kept", "f3d/source.unindexed-records-retained"),
            item("degraded", "", "Body 2", "kept as geometry", ""),
        ]}))
        .unwrap();
        let lines: Vec<String> = lines(&report).into_iter().map(|(_, line)| line).collect();
        assert_eq!(
            lines,
            ["Left out: Features without editable history", "Changed: Body 2 — kept as geometry", "Note: Parts of the file kept as they are (2)",]
        );
        assert!(lines.iter().all(|line| !line.contains("f3d/")), "codes stay under Details");
        let details = detail_lines(&report);
        assert_eq!(details[0], "feature \"Hex socket\": not mapped");
        assert_eq!(details[2], "f3d/source.unindexed-records-retained · f3d/source.unindexed-records-retained: 14 records kept");
        assert_eq!(details[3], "f3d/source.unindexed-records-retained: 2 records kept");
    }
}
