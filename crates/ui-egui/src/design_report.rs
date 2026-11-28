//! The Inspect Design window (#20662): what `document.inspect` says about the design, for people.
//! Its name, units and file, whether it has unsaved changes, and how many features, bodies,
//! sketches and parameters it has, with the bodies by name. Copy JSON puts the whole result on the
//! clipboard. `ui.start` of `document.inspect` (its ribbon button) opens it; `engine.execute` still
//! only returns the JSON. The window follows edits and saves while it is open, at most every
//! [`REFRESH`]: inspecting a large design is slow.

use std::time::Duration;

use web_time::Instant;

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;

/// Body rows shown before the list scrolls.
const LIST_ROWS: f32 = 8.0;

/// The shortest time between two reads of the design.
const REFRESH: Duration = Duration::from_millis(250);

/// The open window: the last `document.inspect` result, or why there is none.
pub struct DesignReport {
    /// What the result was read at: the session revision, its file and whether it had unsaved
    /// changes (saving changes the last two, not the revision).
    pub read: (u64, Option<String>, bool),
    pub read_at: Instant,
    pub result: Result<Value, String>,
}

fn state(app: &SolveApp) -> (u64, Option<String>, bool) {
    (app.session.revision, app.session.path.clone(), app.session.is_dirty())
}

fn read(app: &mut SolveApp) -> Result<Value, String> {
    app.session.execute("document.inspect", &json!({"timeline": "summary", "bodies": "names"})).map_err(|e| e.to_string())
}

/// Open the window on the design.
pub fn open(app: &mut SolveApp) {
    let result = read(app);
    app.design_report = Some(DesignReport { read: state(app), read_at: Instant::now(), result });
}

fn count(v: &Value, list: &str) -> usize {
    v[list].as_array().map_or(0, Vec::len)
}

/// The features in the result that the timeline marker has not rolled back.
fn features(v: &Value) -> usize {
    v["timeline"].as_array().map_or(0, |a| a.iter().filter(|f| f["rolled_back"].as_bool() != Some(true)).count())
}

/// The body names in the result.
pub fn bodies(v: &Value) -> Vec<String> {
    v["bodies"].as_array().map(|a| a.iter().filter_map(|b| b.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Draw the window when it is open. Called once per frame.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut r) = app.design_report.take() else { return };
    let now = state(app);
    if now != r.read {
        let since = r.read_at.elapsed();
        if since >= REFRESH {
            r.read = now;
            r.read_at = Instant::now();
            r.result = read(app);
        } else {
            ctx.request_repaint_after(REFRESH - since);
        }
    }
    let title = solidcraft_ui_text::tr_label("Inspect Design");
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_design_report"), "inspect", title)
        .fields()
        .actions("", solidcraft_ui_text::tr("ui.close"), None)
        .show(ctx, true, |ui| match &r.result {
            Err(e) => {
                ui_kit::text(ui, e, ui_kit::Text::Error("error"));
            }
            Ok(v) => {
                let cell = |ui: &mut egui::Ui, label: &str, value: &str| {
                    ui_kit::row(ui, solidcraft_ui_text::tr_label(label), |ui| ui_kit::text(ui, value, ui_kit::Text::Cell));
                };
                cell(ui, "Name", v["name"].as_str().unwrap_or_default());
                cell(ui, "Units", v["units"].as_str().unwrap_or_default());
                let file = v["path"].as_str().map_or_else(|| solidcraft_ui_text::tr_label("Not saved yet").to_string(), str::to_string);
                cell(ui, "File", &file);
                let dirty = solidcraft_ui_text::tr_label(if v["dirty"].as_bool() == Some(true) { "Yes" } else { "No" });
                cell(ui, "Unsaved changes", dirty);
                let names = bodies(v);
                cell(ui, "Features", &features(v).to_string());
                cell(ui, "Bodies", &names.len().to_string());
                cell(ui, "Sketches", &count(v, "sketches").to_string());
                cell(ui, "Parameters", &count(v, "params").to_string());
                if !names.is_empty() {
                    ui_kit::section(ui, "sc_design_report_bodies", solidcraft_ui_text::tr_label("Bodies"), true, |ui| {
                        egui::ScrollArea::vertical().max_height(ui_kit::row_height() * LIST_ROWS).show(ui, |ui| {
                            for name in &names {
                                ui_kit::text(ui, name, ui_kit::Text::Cell);
                            }
                        });
                    });
                }
                if ui_kit::icon_label_button(ui, solidcraft_ui_text::tr_label("Copy JSON"), "copy", true, false).clicked() {
                    ctx.copy_text(serde_json::to_string_pretty(v).unwrap_or_default());
                }
            }
        });
    if closed || action != ui_kit::Action::None {
        return;
    }
    app.design_report = Some(r);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::scenario::Harness;

    #[test]
    fn the_ribbon_button_opens_a_window_that_follows_the_design() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.ui.tab = "INSPECT".into();
        h.frames(3);
        let at = crate::scenario::handle_at("toolbar:document.inspect").expect("Inspect is on the ribbon");
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
        let r = h.app.design_report.as_ref().expect("clicking Inspect opens a window");
        let v = r.result.as_ref().expect("the design is inspected");
        assert_eq!(super::bodies(v), vec!["Body1".to_string()]);
        assert_eq!(v["timeline"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["path"], serde_json::Value::Null);
        assert_eq!(v["dirty"], true);
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "operation": "new"})).unwrap();
        std::thread::sleep(super::REFRESH);
        h.frames(2);
        let v = h.app.design_report.as_ref().unwrap().result.as_ref().unwrap();
        assert_eq!(super::bodies(v).len(), 2, "the window follows edits");
        // Saving changes neither the design nor the revision; the window still shows it.
        struct Saved(std::path::PathBuf);
        impl Drop for Saved {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let file = Saved(std::env::temp_dir().join(format!("solidcraft-design-report-{}-{stamp}.solidcraft", std::process::id())));
        h.app.run("file.save", json!({"path": file.0.to_string_lossy()})).unwrap();
        std::thread::sleep(super::REFRESH);
        h.frames(2);
        let v = h.app.design_report.as_ref().unwrap().result.as_ref().unwrap();
        assert_eq!(v["dirty"], false, "the window follows a save");
        assert!(v["path"].as_str().is_some_and(|p| p.ends_with(".solidcraft")), "{}", v["path"]);
        // Closing it changes nothing in the design.
        let undo = h.app.session.undo.len();
        h.frames(4);
        assert!(crate::scenario::handle_at("dialog:cancel").is_some(), "the window is drawn");
        h.step(&json!({"click": {"handle": "dialog:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.design_report.is_none());
        assert_eq!(h.app.session.undo.len(), undo);
    }
}
