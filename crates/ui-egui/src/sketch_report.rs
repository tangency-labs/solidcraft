//! The Sketch report window (#6262): what `sketch.inspect` says about the active sketch, for
//! people. The solver status and degrees of freedom left, how many curves, points and
//! constraints the sketch has, and which curves and points are not fully constrained yet.
//! Copy JSON puts the whole `sketch.inspect` result on the clipboard. `ui.start` of
//! `sketch.inspect` (its ribbon button) opens it; `engine.execute` still only returns the JSON.
//! The report follows edits while it is open.

use serde_json::Value;

use crate::SolveApp;
use crate::ui_kit;

/// Rows of unconstrained curves and points shown before the list scrolls.
const LIST_ROWS: f32 = 8.0;

/// The open report: the last `sketch.inspect` result, or why there is none.
pub struct SketchReport {
    /// The session revision the report was read at.
    pub revision: u64,
    /// The sketch the report is about (the active one when the window opened).
    pub sketch: Option<u64>,
    pub result: Result<Value, String>,
}

fn read(app: &mut SolveApp, sketch: Option<u64>) -> Result<Value, String> {
    let params = match sketch {
        Some(id) => serde_json::json!({ "sketch": id }),
        None => serde_json::json!({}),
    };
    app.session.execute("sketch.inspect", &params).map_err(|e| e.to_string())
}

/// Open the report on the active sketch.
pub fn open(app: &mut SolveApp) {
    let sketch = app.session.active_sketch;
    let result = read(app, sketch);
    app.sketch_report = Some(SketchReport { revision: app.session.revision, sketch, result });
}

/// The ids of the entries in `list` that are not fully constrained.
pub fn loose(v: &Value, list: &str) -> Vec<String> {
    v[list]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|e| !e["fully_constrained"].as_bool().unwrap_or(false))
                .map(|e| match e["type"].as_str() {
                    Some(kind) => format!("{} ({kind})", e["id"].as_str().map(str::to_string).unwrap_or_else(|| e["id"].to_string())),
                    None => e["id"].as_str().map(str::to_string).unwrap_or_else(|| e["id"].to_string()),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn count(v: &Value, list: &str) -> usize {
    v[list].as_array().map_or(0, Vec::len)
}

/// The solver status as a word ("well", "under", …).
fn status(v: &Value) -> String {
    match &v["status"] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut r) = app.sketch_report.take() else { return };
    if r.revision != app.session.revision {
        r.revision = app.session.revision;
        r.result = read(app, r.sketch);
    }
    let title = crate::i18n::tr_label("Sketch Report");
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_sketch_report"), "inspect", title)
        .fields()
        .actions("", crate::i18n::tr("ui.close"), None)
        .show(ctx, true, |ui| match &r.result {
            Err(e) => {
                ui_kit::text(ui, e, ui_kit::Text::Error("error"));
            }
            Ok(v) => {
                let name = v["name"].as_str().unwrap_or_default();
                ui_kit::row(ui, crate::i18n::tr_label("Sketch"), |ui| ui_kit::text(ui, name, ui_kit::Text::Cell));
                ui_kit::row(ui, crate::i18n::tr_label("Status"), |ui| ui_kit::text(ui, &status(v), ui_kit::Text::Cell));
                let dof = v["dof"].as_u64().map(|d| d.to_string()).unwrap_or_else(|| v["dof"].to_string());
                ui_kit::row(ui, crate::i18n::tr_label("Degrees of freedom"), |ui| ui_kit::text(ui, &dof, ui_kit::Text::Cell));
                let construction = v["curves"].as_array().map_or(0, |a| a.iter().filter(|c| c["construction"].as_bool() == Some(true)).count());
                let curves =
                    if construction > 0 { format!("{} ({construction} construction)", count(v, "curves")) } else { count(v, "curves").to_string() };
                ui_kit::row(ui, crate::i18n::tr_label("Curves"), |ui| ui_kit::text(ui, &curves, ui_kit::Text::Cell));
                for (label, list) in [("Points", "points"), ("Constraints", "constraints"), ("Profiles", "profiles")] {
                    ui_kit::row(ui, crate::i18n::tr_label(label), |ui| ui_kit::text(ui, &count(v, list).to_string(), ui_kit::Text::Cell));
                }
                let (curves, points) = (loose(v, "curves"), loose(v, "points"));
                if curves.is_empty() && points.is_empty() {
                    ui_kit::text(ui, crate::i18n::tr_label("Everything is fully constrained."), ui_kit::Text::Note("all_constrained"));
                } else {
                    let heading = format!("{} ({})", crate::i18n::tr_label("Not fully constrained"), curves.len() + points.len());
                    ui_kit::section(ui, "sc_sketch_report_loose", &heading, true, |ui| {
                        egui::ScrollArea::vertical().max_height(ui_kit::row_height() * LIST_ROWS).show(ui, |ui| {
                            for (label, ids) in [("Curve", &curves), ("Point", &points)] {
                                for id in ids {
                                    ui_kit::row(ui, crate::i18n::tr_label(label), |ui| ui_kit::text(ui, id, ui_kit::Text::Cell));
                                }
                            }
                        });
                    });
                }
                if ui_kit::icon_label_button(ui, crate::i18n::tr_label("Copy JSON"), "copy", true, false).clicked() {
                    ctx.copy_text(serde_json::to_string_pretty(v).unwrap_or_default());
                }
            }
        });
    if closed || action != ui_kit::Action::None {
        return;
    }
    app.sketch_report = Some(r);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

    #[test]
    fn the_ribbon_button_opens_a_report_that_follows_the_sketch() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [10, 0]]})).unwrap();
        h.step(&json!({"start": "sketch.inspect"})).unwrap();
        h.frames(2);
        let r = h.app.sketch_report.as_ref().expect("the button opens the report");
        let v = r.result.as_ref().expect("the active sketch is inspected");
        assert_eq!(v["curves"].as_array().map(Vec::len), Some(1));
        assert_eq!(super::loose(v, "curves").len(), 1, "a free line is not fully constrained");
        h.app.run("sketch.line", json!({"points": [[10, 0], [10, 10]]})).unwrap();
        h.frames(2);
        let v = h.app.sketch_report.as_ref().unwrap().result.as_ref().unwrap();
        assert_eq!(v["curves"].as_array().map(Vec::len), Some(2), "the report follows edits");
        // Closing it changes nothing in the design.
        let undo = h.app.session.undo.len();
        h.frames(4);
        assert!(crate::scenario::handle_at("dialog:cancel").is_some(), "the window is drawn");
        h.step(&json!({"click": {"handle": "dialog:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.sketch_report.is_none());
        assert_eq!(h.app.session.undo.len(), undo);
    }

    #[test]
    fn the_command_is_on_the_sketch_tab() {
        let spec = solidcraft_engine::find_command("sketch.inspect").unwrap();
        assert_eq!((spec.tab, spec.panel), ("SKETCH", "INSPECT"));
        // While a sketch is open the Sketch tab shows it, not the Inspect tab's drop-down.
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.frames(3);
        let at = crate::scenario::handle_at("toolbar:sketch.inspect").expect("the button is on the ribbon");
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
        assert!(h.app.sketch_report.is_some(), "clicking the ribbon button opens the report");
    }
}
