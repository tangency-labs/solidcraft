//! Optional requirements, explicit local parameter links and manual review in the shared window system.

use std::{collections::BTreeMap, sync::Arc};

use serde_json::{Value, json};

use crate::{SolveApp, ui_kit};

/// Nonblocking native issue-system operations; credentials never cross this boundary.
pub type IssueService = Arc<dyn Fn(Value, Value) -> std::sync::mpsc::Receiver<Result<Value, String>> + Send + Sync>;

/// Draft edits stay in the window until an explicit engine command applies them.
pub struct RequirementsPanel {
    document: Arc<solidcraft_engine::doc::Document>,
    data: Value,
    selected: String,
    import_json: String,
    status: String,
    note: String,
    evidence: String,
    parameter: String,
    basis_spec: Value,
    basis_design: Value,
    error: Option<String>,
    report: Option<Value>,
    drafts: BTreeMap<String, ReviewDraft>,
}

struct ReviewDraft {
    status: String,
    note: String,
    evidence: String,
    parameter: String,
    basis_spec: Value,
    basis_design: Value,
}

impl RequirementsPanel {
    fn refresh(&mut self, app: &mut SolveApp) {
        match app.run("requirements.list", json!({})) {
            Ok(data) => {
                self.data = data;
                self.document = app.session.doc.clone();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn select(&mut self, id: String) {
        if !self.selected.is_empty() {
            self.drafts.insert(
                self.selected.clone(),
                ReviewDraft {
                    status: self.status.clone(),
                    note: self.note.clone(),
                    evidence: self.evidence.clone(),
                    parameter: self.parameter.clone(),
                    basis_spec: self.basis_spec.clone(),
                    basis_design: self.basis_design.clone(),
                },
            );
        }
        self.selected = id;
        if let Some(draft) = self.drafts.remove(&self.selected) {
            self.status = draft.status;
            self.note = draft.note;
            self.evidence = draft.evidence;
            self.parameter = draft.parameter;
            self.basis_spec = draft.basis_spec;
            self.basis_design = draft.basis_design;
            self.error = None;
            return;
        }
        self.basis_spec = self.data["spec_hash"].clone();
        self.basis_design = self.data["design_hash"].clone();
        let row = self.row();
        let status = row["review"]["status"].as_str().unwrap_or("open").to_string();
        let note = row["review"]["note"].as_str().unwrap_or_default().to_string();
        let evidence = row["review"]["evidence"]
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        let parameter = row["binding"]["name"].as_str().unwrap_or_default().to_string();
        (self.status, self.note, self.evidence, self.parameter) = (status, note, evidence, parameter);
        self.error = None;
    }

    fn row(&self) -> &Value {
        self.data["items"]
            .as_array()
            .and_then(|items| items.iter().find(|row| row["requirement"]["id"].as_str() == Some(&self.selected)))
            .unwrap_or(&Value::Null)
    }

    fn execute(&mut self, app: &mut SolveApp, command: &str, params: Value) {
        match app.run(command, params) {
            Ok(result) => {
                self.error = result
                    .get("errors")
                    .and_then(Value::as_array)
                    .filter(|errors| !errors.is_empty())
                    .map(|errors| format!("Model update failed: {}", json!(errors)));
                self.report = None;
                self.refresh(app);
                if command == "requirements.review" {
                    self.drafts.remove(&self.selected);
                    let selected = std::mem::take(&mut self.selected);
                    self.select(selected);
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn save_review(&mut self, app: &mut SolveApp) {
        let evidence: Vec<&str> = self.evidence.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
        self.execute(
            app,
            "requirements.review",
            json!({
                "id": self.selected, "status": self.status, "note": self.note, "evidence": evidence,
                "expected_spec_hash": self.basis_spec, "expected_design_hash": self.basis_design
            }),
        );
    }
}

/// Open requirements without importing data or changing any model parameter.
pub fn open(app: &mut SolveApp) {
    if app.requirements.is_some() {
        return;
    }
    let mut panel = RequirementsPanel {
        document: app.session.doc.clone(),
        data: Value::Null,
        selected: String::new(),
        import_json: String::new(),
        status: "open".into(),
        note: String::new(),
        evidence: String::new(),
        parameter: String::new(),
        basis_spec: Value::Null,
        basis_design: Value::Null,
        error: None,
        report: None,
        drafts: BTreeMap::new(),
    };
    panel.refresh(app);
    let id = panel.data["items"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|row| row["requirement"]["id"].as_str())
        .unwrap_or_default()
        .to_string();
    panel.select(id);
    app.requirements = Some(panel);
}

/// Draw the optional requirements window and route all mutations through engine commands.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    crate::issue_sync::tick(app, ctx);
    let Some(mut panel) = app.requirements.take() else { return };
    if !Arc::ptr_eq(&panel.document, &app.session.doc) {
        panel.refresh(app);
        panel.report = None;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_requirements"), "settings", "Requirements")
        .fields()
        .cards(2.0)
        .actions("", "Close", None)
        .show(ctx, true, |ui| {
            if let Some(source) = panel.data["spec"]["source"].as_object() {
                ui_kit::row(ui, "Source", |ui| {
                    ui.label(format!(
                        "{} / {}",
                        source.get("provider").and_then(Value::as_str).unwrap_or_default(),
                        source.get("id").and_then(Value::as_str).unwrap_or_default()
                    ));
                });
            }
            crate::issue_sync::settings(app, ui, &panel.data);
            ui_kit::section(ui, "requirements_import", "Import JSON", false, |ui| {
                ui_kit::multiline_text_field(ui, "JSON", egui::Id::new("requirements_json"), &mut panel.import_json);
                if ui_kit::button(ui, "Import requirements", ui_kit::Button::Enabled(!panel.import_json.trim().is_empty())).clicked() {
                    match serde_json::from_str::<Value>(&panel.import_json) {
                        Ok(spec) => {
                            let expected = panel.data["spec_hash"].clone();
                            panel.execute(app, "requirements.import", json!({"spec": spec, "expected_spec_hash": expected}));
                            if panel.error.is_none() {
                                let id = panel.data["items"]
                                    .as_array()
                                    .and_then(|items| items.first())
                                    .and_then(|row| row["requirement"]["id"].as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                panel.select(id);
                            }
                        }
                        Err(error) => panel.error = Some(error.to_string()),
                    }
                }
            });
            let rows = panel.data["items"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                ui.label("No requirements");
            } else {
                let completed = rows.iter().filter(|row| row["review"]["status"] == "done" && row["stale"] != true).count();
                ui.label(format!("{completed} / {} complete (manual)", rows.len()));
                egui::ScrollArea::vertical().id_salt("requirements_list").max_height(160.0).show(ui, |ui| {
                    for row in &rows {
                        let id = row["requirement"]["id"].as_str().unwrap_or_default();
                        let title = row["requirement"]["title"].as_str().unwrap_or_default();
                        let status = status_label(row["review"]["status"].as_str().unwrap_or("open"));
                        let stale = if row["stale"] == true { " / Out of date" } else { "" };
                        if ui_kit::list_picker(ui, id, &format!("{status}{stale}  |  {title}"), panel.selected == id).clicked()
                            && panel.selected != id
                        {
                            panel.select(id.to_string());
                        }
                    }
                });
                ui.separator();
                let row = panel.row().clone();
                if let Some(description) = row["requirement"]["description"].as_str().filter(|text| !text.is_empty()) {
                    ui.label(description);
                }
                if let Some(value) = row["requirement"]["value"].as_f64() {
                    ui_kit::row(ui, "Target", |ui| {
                        ui.label(format!("{value} {}", row["requirement"]["unit"].as_str().unwrap_or_default()));
                    });
                }
                ui.separator();
                let mut parameters = vec![String::new()];
                parameters.extend(app.session.doc.params.iter().map(|parameter| parameter.name.clone()));
                let labels: Vec<&str> = parameters.iter().map(|name| if name.is_empty() { "None" } else { name.as_str() }).collect();
                let mut parameter = parameters.iter().position(|name| name == &panel.parameter).unwrap_or(0);
                ui_kit::choice_row(ui, "Parameter", "requirement_parameter", &mut parameter, &labels);
                panel.parameter = parameters.get(parameter).cloned().unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    if ui_kit::button(ui, "Save link", ui_kit::Button::Normal).clicked() {
                        let name = if panel.parameter.is_empty() { Value::Null } else { json!(panel.parameter) };
                        panel.execute(app, "requirements.bind", json!({"id": panel.selected, "name": name}));
                    }
                    let numeric = row["requirement"]["value"].is_number() && row["requirement"]["unit"].is_string();
                    if ui_kit::button(ui, "Apply target to parameter", ui_kit::Button::Enabled(numeric && !panel.parameter.is_empty())).clicked() {
                        panel.execute(app, "requirements.bind", json!({"id": panel.selected, "name": panel.parameter, "apply": true}));
                    }
                });
                ui_kit::row(ui, "Link", |ui| {
                    ui.label(match row["binding_state"].as_str() {
                        Some("linked") => "Linked",
                        Some("parameter_changed") => "Local parameter changed",
                        Some("requirement_changed") => "Requirement changed",
                        Some("missing_parameter") => "Parameter missing",
                        _ => "Unlinked",
                    });
                });
                ui.separator();
                let changed = panel.basis_spec != panel.data["spec_hash"] || panel.basis_design != panel.data["design_hash"];
                if changed {
                    ui_kit::text(ui, "Design or requirements changed", ui_kit::Text::Report("changed", true));
                    if ui_kit::button(ui, "Review current design", ui_kit::Button::Normal).clicked() {
                        panel.basis_spec = panel.data["spec_hash"].clone();
                        panel.basis_design = panel.data["design_hash"].clone();
                    }
                }
                let statuses = ["open", "in_progress", "done", "blocked"];
                let mut status = statuses.iter().position(|status| *status == panel.status).unwrap_or(0);
                ui_kit::choice_row(ui, "Manual status", "requirement_status", &mut status, &statuses.map(status_label));
                panel.status = statuses.get(status).copied().unwrap_or("open").to_string();
                if row["stale"] == true {
                    ui_kit::text(ui, "Saved review is out of date", ui_kit::Text::Report("stale", true));
                }
                ui_kit::multiline_text_field(ui, "Work completed", egui::Id::new("requirement_note"), &mut panel.note);
                ui_kit::multiline_text_field(ui, "Evidence references", egui::Id::new("requirement_evidence"), &mut panel.evidence);
                if ui_kit::button(ui, "Export model image", ui_kit::Button::Normal).clicked() {
                    capture_image(app, &mut panel);
                }
                if ui_kit::button(ui, "Save review", ui_kit::Button::Enabled(!changed)).clicked() {
                    panel.save_review(app);
                }
                if ui_kit::button(ui, "Copy report draft", ui_kit::Button::Normal).clicked() {
                    match app.run("requirements.report", json!({})) {
                        Ok(report) => {
                            ctx.copy_text(report["markdown"].as_str().unwrap_or_default().to_string());
                            panel.report = Some(report);
                        }
                        Err(error) => panel.error = Some(error),
                    }
                }
                if let Some(report) = &panel.report {
                    ui.label(format!("Report copied: {} / {} complete (manual)", report["completed"], report["total"]));
                }
            }
            if let Some(error) = &panel.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("error", true));
            }
        });
    if !closed && action != ui_kit::Action::Cancel {
        app.requirements = Some(panel);
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "in_progress" => "In progress",
        "done" => "Done (manual)",
        "blocked" => "Blocked",
        _ => "Open",
    }
}

fn capture_image(app: &mut SolveApp, panel: &mut RequirementsPanel) {
    let scene = crate::control::view_scene(app, crate::control::window_guides(app));
    let png = match solidcraft_engine::capture::png(&scene, &app.cam, 1280, 800) {
        Ok(png) => png,
        Err(error) => {
            panel.error = Some(format!("Image export failed: {error}"));
            return;
        }
    };
    let name = "requirement.png";
    if let Some(download) = &app.services.download_design {
        match download(name, &png.bytes) {
            Ok(()) => append_evidence(panel, name),
            Err(error) => panel.error = Some(error),
        }
        return;
    }
    let origin = app.session.doc.clone();
    let selected = panel.selected.clone();
    app.pick_save(name, &["png"], move |app, path| {
        let Some(path) = path else { return };
        if !Arc::ptr_eq(&origin, &app.session.doc) {
            app.set_status("The design changed while choosing the image destination; export again", true);
            return;
        }
        if let Err(error) = std::fs::write(&path, &png.bytes) {
            app.set_status(format!("Image export failed: {error}"), true);
            return;
        }
        if let Some(panel) = app.requirements.as_mut().filter(|panel| panel.selected == selected)
            && let Some(name) = std::path::Path::new(&path).file_name().and_then(|name| name.to_str())
        {
            append_evidence(panel, name);
        }
        app.set_status("Model image exported", false);
    });
}

fn append_evidence(panel: &mut RequirementsPanel, name: &str) {
    if panel.evidence.lines().any(|line| line == name) {
        return;
    }
    if !panel.evidence.is_empty() {
        panel.evidence.push('\n');
    }
    panel.evidence.push_str(name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    #[test]
    fn requirements_window_keeps_parameter_linking_optional_and_saves_a_manual_review() {
        let mut harness = Harness::new();
        harness
            .app
            .run(
                "requirements.import",
                json!({"spec": {"schema": "solidcraft-requirements/1", "requirements": [{"id": "fit", "title": "Lid fits"}]}}),
            )
            .unwrap();
        harness.step(&json!({"start": "requirements.list"})).unwrap();
        assert!(harness.app.requirements.is_some());
        harness.frames(2);
        assert!(harness.widget("No requirements").is_none());
        let mut panel = harness.app.requirements.take().unwrap();
        panel.status = "done".into();
        panel.note = "Checked the lid".into();
        panel.evidence = "lid.png".into();
        panel.save_review(&mut harness.app);
        assert!(panel.error.is_none(), "{:?}", panel.error);
        assert_eq!(panel.data["items"][0]["review"]["status"], "done");
        assert!(harness.app.session.doc.params.is_empty());
    }

    #[test]
    fn requirements_linking_preserves_drafts_and_image_export_errors_keep_evidence_unchanged() {
        let mut harness = Harness::new();
        harness.app.run("solid.box", json!({"length": 20, "width": 15, "height": 5})).unwrap();
        harness.app.run("parameters.add", json!({"name": "width", "expression": "25 mm"})).unwrap();
        harness.app.run("requirements.import", json!({"spec": {"schema": "solidcraft-requirements/1", "requirements": [{"id": "width", "title": "Width", "value": 40, "unit": "mm"}]}})).unwrap();
        open(&mut harness.app);
        let mut panel = harness.app.requirements.take().unwrap();
        panel.note = "Draft assessment".into();
        panel.execute(&mut harness.app, "requirements.bind", json!({"id": "width", "name": "width"}));
        assert_eq!(panel.note, "Draft assessment");
        let exported = Arc::new(std::sync::Mutex::new(Vec::new()));
        let image = exported.clone();
        harness.app.services.download_design = Some(Box::new(move |name, bytes| {
            assert_eq!(name, "requirement.png");
            *image.lock().unwrap() = bytes.to_vec();
            Ok(())
        }));
        capture_image(&mut harness.app, &mut panel);
        assert!(panel.error.as_deref().is_some_and(|error| error.contains("no renderer")));
        assert!(exported.lock().unwrap().is_empty());
        assert!(panel.evidence.is_empty());
        panel.execute(&mut harness.app, "requirements.bind", json!({"id": "width", "name": "width", "apply": true}));
        assert_eq!(panel.note, "Draft assessment");
        panel.save_review(&mut harness.app);
        assert!(panel.error.is_some(), "applying a target must not silently rebase a review draft");
    }

    #[test]
    fn requirements_list_keeps_status_visible_and_preserves_drafts_between_rows() {
        let mut harness = Harness::new();
        harness
            .app
            .run(
                "requirements.import",
                json!({"spec": {"schema": "solidcraft-requirements/1", "requirements": [
                    {"id": "fit", "title": "Lid fits"}, {"id": "finish", "title": "Smooth edges"}
                ]}}),
            )
            .unwrap();
        harness.step(&json!({"start": "requirements.list"})).unwrap();
        harness.frames(2);
        assert!(harness.widget("Open  |  Lid fits").is_some());
        assert!(harness.widget("Open  |  Smooth edges").is_some());
        let mut panel = harness.app.requirements.take().unwrap();
        panel.note = "Unsaved fit assessment".into();
        panel.status = "blocked".into();
        panel.select("finish".into());
        panel.note = "Unsaved finish assessment".into();
        panel.select("fit".into());
        assert_eq!(panel.note, "Unsaved fit assessment");
        assert_eq!(panel.status, "blocked");
        panel.save_review(&mut harness.app);
        assert!(panel.error.is_none(), "{:?}", panel.error);
        assert_eq!(panel.row()["review"]["status"], "blocked");
        let command = solidcraft_engine::cmd::command_specs().into_iter().find(|command| command.id == "requirements.list").unwrap();
        assert_eq!(command.tab, "SOLID");
        assert_eq!(command.panel, "INSPECT");
    }

    #[test]
    fn requirements_drafts_belong_to_their_document_even_for_identical_copies() {
        let mut harness = Harness::new();
        harness
            .app
            .run("requirements.import", json!({"spec": {"schema":"solidcraft-requirements/1", "requirements":[{"id":"fit", "title":"Lid fits"}]}}))
            .unwrap();
        open(&mut harness.app);
        harness.app.requirements.as_mut().unwrap().note = "Only the first design was inspected".into();
        open(&mut harness.app);
        assert_eq!(harness.app.requirements.as_ref().unwrap().note, "Only the first design was inspected");
        let mut copy = solidcraft_engine::Session::default();
        *Arc::make_mut(&mut copy.doc) = (*harness.app.session.doc).clone();
        crate::documents::open_tab(&mut harness.app, copy).unwrap();
        assert!(harness.app.requirements.is_none());
        open(&mut harness.app);
        assert!(harness.app.requirements.as_ref().unwrap().note.is_empty());
        crate::documents::switch(&mut harness.app, 0);
        assert_eq!(harness.app.requirements.as_ref().unwrap().note, "Only the first design was inspected");
        assert!(harness.app.run("requirements.list", json!({})).unwrap()["items"][0]["review"].is_null());
    }
}
