//! The import report: after an importer plugin opens a file, what it could not carry over, in the
//! app's own window. The plugin returns the report as data; the app draws it (RFC 0002: plugins
//! ship no UI code). Programs get the same report in `doc.open`'s and `file.import`'s reply and
//! never see this window.
//!
//! The importer's own `details` (for example how far it checked the history it rebuilt, and each
//! feature it mapped or could not) are shown as a folding outline under the lines. The report can
//! be copied or saved, and the last one stays open from Plugins › Last import report… after OK.

use serde_json::Value;
use solidcraft_engine::plugin::{LossReport, LossSeverity};
use solidcraft_ui_text::{format, tr};

use crate::{SolveApp, ui_kit};

/// The last import's report: shown while `open`, kept after OK so it can be read again.
pub(crate) struct Shown {
    /// The design's name and the plugin that made it.
    pub(crate) design: String,
    pub(crate) plugin: String,
    pub(crate) report: LossReport,
    /// The importer's `details` as the window folds them, made once when the report arrives.
    pub(crate) outline: Vec<Node>,
    pub(crate) open: bool,
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
    let outline = outline(&report.details, SHOWN);
    app.import_report = Some(Shown { design: text("name"), plugin, report, outline, open: true });
}

/// Plugins › Last import report…: show the last import's report again.
pub(crate) fn reopen(app: &mut SolveApp) {
    if let Some(shown) = app.import_report.as_mut() {
        shown.open = true;
    }
}

/// One line of the importer's details: a value (`label: value`), or a folding group of lines.
#[derive(Debug, PartialEq)]
pub(crate) struct Node {
    pub(crate) label: String,
    pub(crate) value: String,
    pub(crate) children: Vec<Node>,
    /// A group starts unfolded when it is near the top or holds only values.
    pub(crate) open: bool,
}

impl Node {
    fn leaf(label: String, value: String) -> Self {
        Node { label, value, children: Vec::new(), open: false }
    }
}

/// How much of the details an outline takes: the window's is bounded so drawing stays quick, a
/// copied or saved report's takes everything (the details are at most 1 MiB of JSON).
#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    /// Elements of one list shown before "and N more".
    list: usize,
    /// Lines in the whole outline.
    total: usize,
    /// Plain values of a list joined on one line before "…".
    joined: usize,
    /// Characters of one value shown; the rest is cut with "…".
    text: usize,
}
const SHOWN: Bounds = Bounds { list: 200, total: 20_000, joined: MAX_JOINED, text: MAX_TEXT };
const ALL: Bounds = Bounds { list: usize::MAX, total: usize::MAX, joined: usize::MAX, text: usize::MAX };
/// Nesting deeper than this is shown as the JSON it is, on one line.
const MAX_DEPTH: usize = 12;
/// Characters of one value or label shown; the rest is cut with "…".
const MAX_TEXT: usize = 2000;
/// Values of a list of plain values joined on one line before "…".
const MAX_JOINED: usize = 50;
/// The fields an element of a list is called by, in order: its name, else its title, …
const NAMES: [&str; 8] = ["name", "title", "label", "subject", "stage", "id", "kind", "code"];

/// The importer's `details` as folding lines: each field a line, each object or list of objects a
/// group. Empty values (null, "", [] and {}) are left out.
pub(crate) fn outline(details: &Value, bounds: Bounds) -> Vec<Node> {
    let mut budget = bounds.total;
    match details {
        Value::Object(map) => fields(map, 0, bounds, &mut budget),
        Value::Array(list) => elements(list, 0, bounds, &mut budget),
        other => node(String::new(), other, 0, bounds, &mut budget).into_iter().collect(),
    }
}

fn fields(map: &serde_json::Map<String, Value>, depth: usize, bounds: Bounds, budget: &mut usize) -> Vec<Node> {
    map.iter().filter_map(|(key, value)| node(humanize(key), value, depth, bounds, budget)).collect()
}

fn elements(list: &[Value], depth: usize, bounds: Bounds, budget: &mut usize) -> Vec<Node> {
    // A list's elements start folded: the list shows their names and statuses at a glance.
    let folded = |node: Node| Node { open: false, ..node };
    let mut out: Vec<Node> = list
        .iter()
        .take(bounds.list)
        .enumerate()
        .filter_map(|(i, value)| node(element_label(value, i), value, depth, bounds, budget).map(folded))
        .collect();
    if list.len() > bounds.list {
        out.push(Node::leaf(format("import_report.more", &[("count", &(list.len() - bounds.list).to_string())]), String::new()));
    }
    out
}

fn node(label: String, value: &Value, depth: usize, bounds: Bounds, budget: &mut usize) -> Option<Node> {
    if empty(value) || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if depth >= MAX_DEPTH {
        return Some(Node::leaf(label, clip_to(&value.to_string(), bounds.text)));
    }
    Some(match value {
        Value::Array(list) if list.iter().all(|v| !v.is_array() && !v.is_object()) => {
            let mut joined = list.iter().take(bounds.joined).map(scalar).collect::<Vec<_>>().join(", ");
            if list.len() > bounds.joined {
                joined.push_str(", …");
            }
            Node::leaf(label, clip_to(&joined, bounds.text))
        }
        Value::Array(list) => {
            let label = format("import_report.group", &[("title", &label), ("count", &list.len().to_string())]);
            Node { label, value: String::new(), children: elements(list, depth + 1, bounds, budget), open: false }
        }
        Value::Object(map) => {
            let open = depth == 0 || map.values().all(|v| !v.is_array() && !v.is_object());
            Node { label, value: String::new(), children: fields(map, depth + 1, bounds, budget), open }
        }
        other => Node::leaf(label, clip_to(&scalar(other), bounds.text)),
    })
}

fn empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(list) => list.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

/// A plain value as text: a string as it is, a number or yes/no as JSON writes it.
fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// An element of a list is called by its name (or title, label, …), with its status after it
/// when it has one ("Extrude 1 · blocked"); otherwise by its place in the list.
fn element_label(value: &Value, index: usize) -> String {
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let name = NAMES.iter().find_map(|key| field(key)).map_or_else(|| (index + 1).to_string(), clip);
    match field("status") {
        Some(status) => std::format!("{name} · {}", clip(status)),
        None => name,
    }
}

/// A field's key as a label: `final_body` reads "Final body".
fn humanize(key: &str) -> String {
    let words = clip(&key.replace(['_', '-'], " "));
    let mut chars = words.trim().chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Printable text, at most [`MAX_TEXT`] characters.
fn clip(text: &str) -> String {
    clip_to(text, MAX_TEXT)
}

/// Printable text, at most `limit` characters.
fn clip_to(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).map(|c| if c.is_control() { ' ' } else { c }).collect();
    if text.chars().nth(limit).is_some() {
        out.push('…');
    }
    out
}

fn outline_text(nodes: &[Node], depth: usize, out: &mut Vec<String>) {
    for node in nodes {
        let indent = "  ".repeat(depth);
        out.push(match (node.label.is_empty(), node.value.is_empty()) {
            (true, _) => std::format!("{indent}{}", node.value),
            (false, true) => std::format!("{indent}{}", node.label),
            (false, false) => std::format!("{indent}{}: {}", node.label, node.value),
        });
        outline_text(&node.children, depth + 1, out);
    }
}

/// The whole report as plain text, for the clipboard or a `.txt` file: the heading, the summary,
/// the lines, every item with its code, then all of the importer's details.
pub(crate) fn report_text(shown: &Shown) -> String {
    let mut out = vec![tr("import_report.title").to_string(), format("import_report.heading", &[("name", &shown.design), ("plugin", &shown.plugin)])];
    if !shown.report.summary.is_empty() {
        out.push(shown.report.summary.clone());
    }
    out.push(String::new());
    if shown.report.items.is_empty() {
        out.push(tr("import_report.nothing_lost").to_string());
    } else {
        out.extend(lines(&shown.report).into_iter().map(|(_, line)| line));
        out.push(String::new());
        out.push(tr("import_report.details").to_string());
        out.extend(detail_lines(&shown.report).into_iter().map(|line| std::format!("  {line}")));
    }
    let details = outline(&shown.report.details, ALL);
    if !details.is_empty() {
        out.push(String::new());
        out.push(tr("import_report.importer_details").to_string());
        outline_text(&details, 1, &mut out);
    }
    out.push(String::new());
    out.join("\n")
}

/// The report as `doc.open` and `file.import` reply with it, for a `.json` file.
fn report_json(shown: &Shown) -> Result<String, String> {
    let reply = serde_json::json!({"name": shown.design, "plugin": shown.plugin, "loss_report": shown.report});
    serde_json::to_string_pretty(&reply).map_err(|e| e.to_string())
}

/// Save report…: a `.txt` file gets the text, a `.json` file the report as programs get it.
fn save(app: &mut SolveApp) {
    let Some(shown) = app.import_report.as_ref() else { return };
    let (text, json) = (report_text(shown), report_json(shown));
    let stem: String = shown.design.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    let name = format("import_report.file_name", &[("name", stem.trim())]);
    app.pick_save(&std::format!("{name}.txt"), &["txt", "json"], move |app, path| {
        let Some(path) = path else { return };
        let as_json = std::path::Path::new(&path).extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
        // Through `vfs`, like every other save: the web app hands it out as a download.
        let written = if as_json { json } else { Ok(text) }
            .and_then(|body| solidcraft_engine::io::vfs::write(&path, body.as_bytes()).map_err(|e| e.to_string()));
        match written {
            Ok(()) => app.set_status(format("import_report.saved", &[("path", &path)]), false),
            Err(error) => app.set_status(format("import_report.save_failed", &[("error", &error)]), true),
        }
    });
}

fn draw_outline(ui: &mut egui::Ui, nodes: &[Node]) {
    for (i, node) in nodes.iter().enumerate() {
        ui.push_id(i, |ui| {
            if node.children.is_empty() {
                let line = if node.label.is_empty() || node.value.is_empty() {
                    std::format!("{}{}", node.label, node.value)
                } else {
                    std::format!("{}: {}", node.label, node.value)
                };
                ui_kit::text(ui, &line, ui_kit::Text::Report("import_report_field", false));
            } else {
                ui_kit::section(ui, "import_report_group", &node.label, node.open, |ui| {
                    ui.indent("import_report_group_lines", |ui| draw_outline(ui, &node.children));
                });
            }
        });
    }
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

/// The window, while a report is open.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(shown) = app.import_report.as_ref().filter(|shown| shown.open) else { return };
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let title = tr("import_report.title").to_string();
    let ok = tr("import_report.ok").to_string();
    let heading = format("import_report.heading", &[("name", &shown.design), ("plugin", &shown.plugin)]);
    let counts = ORDER.map(|severity| (severity, shown.report.items.iter().filter(|i| i.severity == severity).count()));
    let (mut copy, mut save_it) = (false, false);
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
            } else {
                let tally: Vec<String> =
                    counts.iter().filter(|(_, n)| *n > 0).map(|(severity, n)| std::format!("{} {n}", tr(severity_label(*severity)))).collect();
                ui_kit::text(ui, &tally.join(" · "), ui_kit::Text::Hint("import_report_tally"));
            }
            if !shown.report.items.is_empty() || !shown.outline.is_empty() {
                egui::ScrollArea::vertical().id_salt("import_report_items").max_height(320.0).show(ui, |ui| {
                    for (i, (severity, line)) in lines(&shown.report).iter().enumerate() {
                        let strong = *severity == LossSeverity::Dropped;
                        ui.push_id(i, |ui| ui_kit::text(ui, line, ui_kit::Text::Report("import_report_item", strong)));
                    }
                    // The importer's own data: how it checked what it made, feature by feature.
                    if !shown.outline.is_empty() {
                        ui_kit::section(ui, "import_report_importer", tr("import_report.importer_details"), true, |ui| {
                            draw_outline(ui, &shown.outline);
                        });
                    }
                    // The plugin's own wording and codes, for a bug report, folded away.
                    if !shown.report.items.is_empty() {
                        ui_kit::section(ui, "import_report_details", tr("import_report.show_details"), false, |ui| {
                            for (i, line) in detail_lines(&shown.report).iter().enumerate() {
                                ui.push_id(("detail", i), |ui| ui_kit::text(ui, line, ui_kit::Text::Note("import_report_detail")));
                            }
                        });
                    }
                });
            }
            ui.horizontal(|ui| {
                copy = ui_kit::icon_label_button(ui, tr("import_report.copy"), "copy", true, false).clicked();
                save_it = ui_kit::icon_label_button(ui, tr("import_report.save"), "export_tray", true, false).clicked();
            });
        });
    if copy {
        ctx.copy_text(report_text(shown));
        app.set_status(tr("import_report.copied").to_string(), false);
    }
    if save_it {
        save(app);
    }
    if closed || matches!(action, ui_kit::Action::Ok | ui_kit::Action::Cancel) {
        // Kept, not dropped: Plugins › Last import report… shows it again.
        if let Some(shown) = app.import_report.as_mut() {
            shown.open = false;
        }
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

    /// A real importer's details (the shape the Dutchman importer returns): how far it checked
    /// the history it rebuilt and each feature it mapped or could not, read as folding lines.
    fn details() -> serde_json::Value {
        serde_json::json!({"schema":"sample.loss/1","completeness":null,"joints":{},"parts":[],
            "outcome":{"mode":"partial_history","verification":{"status":"partial","message":"3 of 5 features rebuilt"},
                "features":[{"name":"Sketch 1","kind":"sketch","status":"mapped","message":null},
                            {"name":"Extrude 1","kind":"extrude","status":"blocked","message":"unknown profile in Sketch 2","final_body":false}]},
            "stages":[{"stage":"map","total":4},{"stage":"write","total":6}],"tags":["a","b"]})
    }

    #[test]
    fn import_report_shows_the_importers_details_as_folding_lines() {
        crate::i18n::install();
        let nodes = outline(&details(), SHOWN);
        let labels: Vec<&str> = nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Outcome", "Schema", "Stages (2)", "Tags"], "null, {{}} and [] are left out; keys read as words");
        let outcome = &nodes[0];
        assert!(outcome.open, "a top-level group starts unfolded");
        let verification = outcome.children.iter().find(|n| n.label == "Verification").expect("verification");
        assert!(verification.open, "a group of plain values starts unfolded");
        assert_eq!(verification.children[1], Node::leaf("Status".into(), "partial".into()));
        let features = outcome.children.iter().find(|n| n.label == "Features (2)").expect("features");
        assert!(!features.open, "a list starts folded");
        let named: Vec<&str> = features.children.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(named, ["Sketch 1 · mapped", "Extrude 1 · blocked"], "an element is called by its name and status");
        assert!(features.children.iter().all(|n| !n.open), "a list's elements start folded");
        assert!(features.children[1].children.contains(&Node::leaf("Final body".into(), "false".into())));
        assert_eq!(nodes[3], Node::leaf("Tags".into(), "a, b".into()), "a list of plain values is one line");

        // Copy and Save carry everything: the lines, the codes and the importer's details.
        let report: LossReport = serde_json::from_value(serde_json::json!({"summary":"3 of 5 features","items":[
            item("dropped", "Features without editable history", "Extrude 1", "not mapped", "map/blocked")],"details":details()}))
        .unwrap();
        let outline = outline(&report.details, SHOWN);
        let shown = Shown { design: "Bracket".into(), plugin: "Sample".into(), report, outline, open: true };
        let text = report_text(&shown);
        for line in [
            "Import report",
            "Bracket, opened by Sample",
            "3 of 5 features",
            "Left out: Features without editable history",
            "  map/blocked · Extrude 1: not mapped",
            "From the importer",
            "    Verification",
            "      Status: partial",
            "        Message: unknown profile in Sketch 2",
        ] {
            assert!(text.lines().any(|l| l == line), "{line:?} in\n{text}");
        }
        let json: serde_json::Value = serde_json::from_str(&report_json(&shown).unwrap()).unwrap();
        assert_eq!(json["loss_report"]["details"]["outcome"]["verification"]["status"], "partial");
    }

    #[test]
    fn import_report_details_are_bounded_and_hostile_details_never_panic() {
        crate::i18n::install();
        let many: Vec<serde_json::Value> = (0..250).map(|i| serde_json::json!({"id": i.to_string()})).collect();
        let nodes = outline(&serde_json::json!({"losses": many}), SHOWN);
        assert_eq!(nodes[0].children.len(), 201);
        assert_eq!(nodes[0].children[200].label, "and 50 more");
        assert_eq!(outline(&serde_json::json!({"losses": many}), ALL)[0].children.len(), 250, "a copied report has them all");
        let long: Vec<serde_json::Value> = (0..51).map(|i| serde_json::json!(format!("v{i}"))).collect();
        let text = "x".repeat(MAX_TEXT + 10);
        let full = outline(&serde_json::json!({"list": long, "long": text}), ALL);
        assert!(full[0].value.ends_with("v50"), "a copied report has every list element");
        assert_eq!(full[1].value.chars().count(), MAX_TEXT + 10, "and every value in full");
        let cut = outline(&serde_json::json!({"list": long, "long": text}), SHOWN);
        assert!(cut[0].value.ends_with('…') && cut[1].value.ends_with('…'));
        let mut deep = serde_json::json!("bottom");
        for _ in 0..100 {
            deep = serde_json::json!({"x\u{7}y_z": [deep]});
        }
        let nodes = outline(&deep, SHOWN);
        assert_eq!(nodes[0].label, "X y z (1)", "control characters never reach the window");
        let text = report_text(&Shown {
            design: "a/b".into(),
            plugin: String::new(),
            report: LossReport { details: deep, ..Default::default() },
            outline: nodes,
            open: true,
        });
        assert!(text.lines().count() < 40, "nesting past the limit is one line of JSON");
        for odd in [serde_json::json!(null), serde_json::json!("just text"), serde_json::json!([1, [2], {"a": "x".repeat(5000)}])] {
            let nodes = outline(&odd, SHOWN);
            assert!(nodes.iter().all(|n| n.label.chars().count() <= MAX_TEXT + 1 && n.value.chars().count() <= MAX_TEXT + 1));
        }
    }

    #[test]
    fn import_report_stays_after_ok_and_the_plugins_menu_shows_it_again() {
        crate::i18n::install();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let reply = serde_json::json!({"name":"Bracket","plugin":"Sample","loss_report":{"summary":"done","items":[],"details":details()}});
        offer(&mut app, &reply);
        let shown = app.import_report.as_mut().expect("a report");
        assert!(shown.open && !shown.outline.is_empty());
        shown.open = false;
        crate::plugin_menu::choose(&mut app, "last_import_report");
        assert!(app.import_report.as_ref().is_some_and(|s| s.open), "Plugins › Last import report… opens it again");
    }
}
