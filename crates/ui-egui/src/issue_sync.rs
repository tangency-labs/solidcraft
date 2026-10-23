//! Optional native issue connections and automatic refresh, separate from AI chat.

use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{SolveApp, ui_kit};

#[derive(Default)]
pub struct State {
    pub configs: Vec<Value>,
    pending: Option<Pending>,
    next: f64,
    pub status: String,
    issue_url: String,
    endpoint: String,
    publications: BTreeMap<String, String>,
    read_basis: Option<(Value, f64)>,
    external_status: Option<(Value, String)>,
    attach_to: Option<Arc<solidcraft_engine::doc::Document>>,
}

struct Pending {
    response: mpsc::Receiver<Result<Value, String>>,
    document: Arc<solidcraft_engine::doc::Document>,
    data: Value,
    kind: &'static str,
    marker: String,
}

impl State {
    fn status_for(&self, data: &Value) -> Option<&str> {
        self.external_status
            .as_ref()
            .filter(|(source, status)| *source == data["spec"]["source"] && !status.is_empty())
            .map(|(_, status)| status.as_str())
    }

    fn reserve_publication(&mut self, marker: &str) -> bool {
        if self.publications.contains_key(marker) {
            return false;
        }
        if self.publications.len() >= 128 {
            let confirmed = self.publications.iter().find(|(_, status)| status.as_str() == "published").map(|(marker, _)| marker.clone());
            if let Some(confirmed) = confirmed {
                self.publications.remove(&confirmed);
            } else {
                self.status =
                    "Automatic publication paused: report history is full of unconfirmed attempts. Check the issues before publishing again".into();
                return false;
            }
        }
        self.publications.insert(marker.into(), "attempting".into());
        true
    }

    pub fn prefs(&self) -> Value {
        json!({"connections": self.configs, "publications": self.publications})
    }

    pub fn load(&mut self, value: &Value) {
        self.configs = value["connections"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .take(32)
                    .filter(|item| {
                        item["id"].as_str().is_some_and(|id| id.len() == 32)
                            && item["url"].as_str().is_some_and(|url| url.len() <= 2048 && url.starts_with("https://"))
                            && item["workspace"].as_str().is_some_and(|workspace| workspace.len() <= 128)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.publications = value["publications"]
            .as_object()
            .map(|items| {
                items
                    .iter()
                    .take(128)
                    .filter_map(|(key, value)| {
                        let value = value.as_str()?;
                        (key.len() <= 256 && value.len() <= 32).then(|| (key.clone(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
}

fn begin(app: &mut SolveApp, config: Value, operation: Value, data: Value, kind: &'static str, marker: String) {
    let Some(service) = &app.services.issue_system else { return };
    app.issue_sync.pending = Some(Pending { response: service(config, operation), document: app.session.doc.clone(), data, kind, marker });
    app.issue_sync.status = if kind == "connect" { "Waiting for browser sign-in" } else { "Synchronizing" }.into();
}

fn source_matches(config: &Value, data: &Value) -> bool {
    data["spec"]["source"]["provider"] == "linear" && config["workspace"] == data["spec"]["source"]["workspace"]
}

fn issue_identity(url: &str) -> Option<(String, String)> {
    let path = url.strip_prefix("https://linear.app/")?;
    if url.len() > 2048 || url.contains(['?', '#', '\\']) {
        return None;
    }
    let mut pieces = path.split('/');
    let workspace = pieces.next().filter(|part| !part.is_empty())?;
    if pieces.next()? != "issue" {
        return None;
    }
    let issue = pieces.next().filter(|part| !part.is_empty())?;
    Some((workspace.to_string(), issue.to_string()))
}

/// Progress even when the requirements window is closed. No browser is opened by polling.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if app.services.issue_system.is_none() {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_secs(1));
    if let Some(pending) = app.issue_sync.pending.take() {
        match pending.response.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {
                app.issue_sync.pending = Some(pending);
                return;
            }
            result => {
                app.issue_sync.next = app.now + 60.0;
                match result.unwrap_or_else(|_| Err("Issue-system worker stopped".into())) {
                    Err(error) => {
                        if pending.kind == "comment" {
                            app.issue_sync.publications.insert(pending.marker, "uncertain".into());
                            app.issue_sync.status = "Publication unconfirmed; check the issue before publishing again".into();
                        } else {
                            app.issue_sync.status = error;
                        }
                    }
                    Ok(result) if pending.kind == "connect" => {
                        let config = result["config"].clone();
                        if config.is_object() && app.issue_sync.configs.len() < 32 {
                            app.issue_sync.configs.retain(|old| old["workspace"] != config["workspace"]);
                            app.issue_sync.configs.push(config);
                            app.issue_sync.status = "Connected; automatic refresh enabled".into();
                            app.issue_sync.next = 0.0;
                            if pending.data["spec"].is_null() && Arc::ptr_eq(&pending.document, &app.session.doc) {
                                app.issue_sync.attach_to = Some(app.session.doc.clone());
                            }
                        }
                    }
                    Ok(_) if pending.kind == "disconnect" => {
                        app.issue_sync.configs.retain(|config| config["id"] != pending.data["id"]);
                        app.issue_sync.status = "Disconnected; local requirements kept".into();
                    }
                    Ok(_) if pending.kind == "comment" => {
                        app.issue_sync.publications.insert(pending.marker, "published".into());
                        app.issue_sync.status = "Saved review published".into();
                    }
                    Ok(result) => {
                        if !Arc::ptr_eq(&pending.document, &app.session.doc) {
                            app.issue_sync.status = "Design changed during refresh; trying again later".into();
                            return;
                        }
                        match refreshed_spec(&pending.data, &result) {
                            Ok(Some(spec)) => {
                                match app.run("requirements.import", json!({"spec": spec, "expected_spec_hash": pending.data["spec_hash"]})) {
                                    Ok(_) => app.issue_sync.status = "Requirements updated; parameters unchanged".into(),
                                    Err(error) => app.issue_sync.status = error,
                                }
                            }
                            Ok(None) => app.issue_sync.status = "Up to date".into(),
                            Err(error) => app.issue_sync.status = error,
                        }
                        app.issue_sync.attach_to = None;
                        if let Ok(current) = app.run("requirements.list", json!({})) {
                            let status = result["status"].as_str().or_else(|| result["status"]["name"].as_str()).unwrap_or_default().to_string();
                            app.issue_sync.external_status = Some((current["spec"]["source"].clone(), status));
                            app.issue_sync.read_basis = Some((current["spec"].clone(), app.now));
                            app.issue_sync.next = 0.0;
                        }
                    }
                }
            }
        }
    }
    if app.now < app.issue_sync.next || app.applying.is_some() || app.deferred.is_some() {
        return;
    }
    let Ok(mut data) = app.run("requirements.list", json!({})) else { return };
    let config = if data["spec"].is_null() {
        app.issue_sync.attach_to.as_ref().filter(|document| Arc::ptr_eq(document, &app.session.doc)).and_then(|_| app.issue_sync.configs.last())
    } else {
        app.issue_sync.configs.iter().find(|config| source_matches(config, &data))
    };
    let Some(config) = config.cloned() else { return };
    let url = data["spec"]["source"]["url"].as_str().or_else(|| config["issue_url"].as_str()).unwrap_or_default().to_string();
    let Some((_, issue)) = issue_identity(&url) else { return };
    if let Some((spec, at)) = &app.issue_sync.read_basis
        && *spec == data["spec"]
        && app.now - at < 60.0
    {
        app.issue_sync.next = app.now + 5.0;
        if config["publish_reviews"] == true {
            let items = data["items"].as_array();
            let current =
                items.is_some_and(|items| items.iter().any(|item| item["review"].is_object()) && items.iter().all(|item| item["stale"] != true));
            if current
                && let Ok(report) = app.run("requirements.report", json!({}))
                && let Some(markdown) = report["markdown"].as_str()
            {
                let digest = format!("{:x}", Sha256::digest(markdown.as_bytes()));
                let marker = format!("solidcraft-review:{digest}");
                if app.issue_sync.reserve_publication(&marker) {
                    let body = format!("{markdown}\n\n<!-- {marker} -->");
                    begin(
                        app,
                        config,
                        json!({"kind":"comment", "issue":data["spec"]["source"]["id"], "issue_url":url, "body":body, "marker":marker, "spec":data["spec"]}),
                        data,
                        "comment",
                        marker,
                    );
                }
            }
        }
        return;
    }
    if config["automatic"] != false {
        app.issue_sync.next = app.now + 60.0;
        let issue = data["spec"]["source"]["id"].as_str().unwrap_or(&issue).to_string();
        data["connection"] = config.clone();
        begin(app, config, json!({"kind":"read", "issue": issue, "issue_url": url}), data, "read", String::new());
    }
}

fn refreshed_spec(data: &Value, issue: &Value) -> Result<Option<Value>, String> {
    let title = issue["title"].as_str().ok_or("Issue response has no title")?;
    let description = issue["description"].as_str().unwrap_or_default();
    let mut spec = data["spec"].clone();
    if spec.is_null() {
        let (_, id) = issue_identity(data["connection"]["issue_url"].as_str().ok_or("Missing issue URL")?).ok_or("Invalid issue URL")?;
        spec = json!({"schema":"solidcraft-requirements/1", "source":{
            "provider":"linear", "workspace":data["connection"]["workspace"], "id":id,
            "revision":issue["updatedAt"], "url":data["connection"]["issue_url"]
        }, "requirements":[{"id":"issue", "title":"", "description":""}]});
    }
    let requirements = spec["requirements"].as_array_mut().ok_or("No requirement collection")?;
    if requirements.len() != 1 || requirements.first().is_some_and(|item| item["value"].is_number()) {
        return Err("Structured requirements need an explicit import; automatic text replacement is paused".into());
    }
    let item = requirements.first_mut().ok_or("Missing requirement")?;
    if item["title"] == title && item["description"].as_str().unwrap_or_default() == description {
        return Ok(None);
    }
    item["title"] = json!(title);
    item["description"] = json!(description);
    spec["source"]["revision"] = json!(issue["updatedAt"].as_str().ok_or("Issue response has no revision")?);
    Ok(Some(spec))
}

/// Setup is optional. It does not require AI features or manually trigger synchronization.
pub fn settings(app: &mut SolveApp, ui: &mut egui::Ui, data: &Value) {
    if app.services.issue_system.is_none() {
        return;
    }
    ui_kit::section(ui, "requirements_connection", "Issue connection", false, |ui| {
        let config = app.issue_sync.configs.iter().find(|config| source_matches(config, data)).cloned();
        if let Some(mut config) = config {
            ui.label(format!("Linear / {}", config["workspace"].as_str().unwrap_or_default()));
            let automatic = config["automatic"] != false;
            if ui_kit::checkbox_row(ui, "requirements_auto_refresh", "Automatic refresh", automatic) {
                config["automatic"] = json!(!automatic);
                if let Some(saved) = app.issue_sync.configs.iter_mut().find(|saved| saved["id"] == config["id"]) {
                    *saved = config.clone();
                }
            }
            let publish = config["publish_reviews"] == true;
            if ui_kit::checkbox_row(ui, "requirements_auto_publish", "Publish saved reviews automatically", publish) {
                config["publish_reviews"] = json!(!publish);
                if let Some(saved) = app.issue_sync.configs.iter_mut().find(|saved| saved["id"] == config["id"]) {
                    *saved = config.clone();
                }
                app.issue_sync.next = 0.0;
            }
            if let Some(status) = app.issue_sync.status_for(data) {
                ui_kit::row(ui, "Issue status", |ui| {
                    ui.label(status);
                });
            }
            if ui_kit::button(ui, "Disconnect", ui_kit::Button::Enabled(app.issue_sync.pending.is_none())).clicked() {
                begin(app, config.clone(), json!({"kind":"disconnect"}), config, "disconnect", String::new());
            }
            if ui_kit::button(ui, "Sign in again", ui_kit::Button::Enabled(app.issue_sync.pending.is_none())).clicked() {
                let Some(config) = app.issue_sync.configs.iter().find(|config| source_matches(config, data)).cloned() else { return };
                begin(app, config, json!({"kind":"connect"}), data.clone(), "connect", String::new());
            }
        } else {
            if app.issue_sync.endpoint.is_empty() {
                app.issue_sync.endpoint = "https://mcp.linear.app/mcp".into();
            }
            if app.issue_sync.issue_url.is_empty() {
                app.issue_sync.issue_url = data["spec"]["source"]["url"].as_str().unwrap_or_default().to_string();
            }
            ui_kit::row(ui, "MCP server", |ui| {
                ui_kit::text_field(ui, &mut app.issue_sync.endpoint, "https://");
            });
            ui_kit::row(ui, "Linear issue URL", |ui| {
                ui_kit::text_field(ui, &mut app.issue_sync.issue_url, "https://linear.app/workspace/issue/ID");
            });
            if ui_kit::button(ui, "Connect account", ui_kit::Button::Enabled(app.issue_sync.pending.is_none())).clicked() {
                if data["spec"].is_object() && data["spec"]["source"].is_null() {
                    app.issue_sync.status = "This design has a local checklist; keep it local or explicitly remove it before linking an issue".into();
                    return;
                }
                match issue_identity(&app.issue_sync.issue_url) {
                    Some((workspace, _)) => {
                        let workspace = data["spec"]["source"]["workspace"].as_str().unwrap_or(&workspace).to_string();
                        let config = json!({"url": app.issue_sync.endpoint, "workspace": workspace, "issue_url": app.issue_sync.issue_url, "automatic":true, "publish_reviews":false});
                        begin(app, config, json!({"kind":"connect"}), data.clone(), "connect", String::new());
                    }
                    None => app.issue_sync.status = "Enter a Linear issue URL, including its workspace".into(),
                }
            }
        }
    });
    if !app.issue_sync.status.is_empty() {
        ui.label(&app.issue_sync.status);
    }
    if app.issue_sync.publications.values().any(|status| matches!(status.as_str(), "uncertain" | "attempting")) {
        ui_kit::text(
            ui,
            "A report publication is unconfirmed. Check the issue; that report will not be sent again automatically.",
            ui_kit::Text::Report("publication_uncertain", true),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn data() -> Value {
        json!({"spec":{"schema":"solidcraft-requirements/1", "source":{"provider":"linear", "workspace":"example", "id":"EX-1", "revision":"1", "url":"https://linear.app/example/issue/EX-1/title"}, "requirements":[{"id":"fit", "title":"Old", "description":"old"}]}})
    }

    #[test]
    fn refresh_preserves_ids_and_ignores_revision_only_changes() {
        let first = data();
        let issue = json!({"title":"Old", "description":"old", "updatedAt":"2"});
        assert!(refreshed_spec(&first, &issue).unwrap().is_none());
        let changed = refreshed_spec(&first, &json!({"title":"New", "description":"new", "updatedAt":"2"})).unwrap().unwrap();
        assert_eq!(changed["requirements"][0]["id"], "fit");
        assert_eq!(changed["source"]["revision"], "2");
        let mut numeric = first;
        numeric["spec"]["requirements"][0]["value"] = json!(10);
        assert!(refreshed_spec(&numeric, &issue).is_err());
    }

    #[test]
    fn automatic_refresh_works_without_ai_or_an_open_window_and_preserves_parameters() {
        let mut harness = Harness::new();
        harness.app.run("requirements.import", json!({"spec":data()["spec"]})).unwrap();
        harness.app.agent_chat.input = "Unsent question".into();
        harness.app.issue_sync.configs.push(json!({"workspace":"example", "automatic":true}));
        harness.app.services.issue_system = Some(Arc::new(|_, operation| {
            assert_eq!(operation["kind"], "read");
            let (sender, receiver) = mpsc::channel();
            sender.send(Ok(json!({"title":"Updated", "description":"new", "updatedAt":"2"}))).unwrap();
            receiver
        }));
        assert!(harness.app.requirements.is_none());
        tick(&mut harness.app, &harness.ctx);
        tick(&mut harness.app, &harness.ctx);
        assert_eq!(harness.app.run("requirements.list", json!({})).unwrap()["spec"]["requirements"][0]["title"], "Updated");
        assert!(harness.app.session.doc.params.is_empty());
        assert_eq!(harness.app.agent_chat.input, "Unsent question");
        assert!(!harness.app.agent_chat.open);
    }

    #[test]
    fn connection_preferences_keep_workspace_contexts_and_publication_ledger() {
        let mut state = State::default();
        let first = json!({"id":"0123456789abcdef0123456789abcdef", "workspace":"one", "url":"https://mcp.linear.app/mcp"});
        let mut second = first.clone();
        second["workspace"] = json!("two");
        state.configs = vec![first, second];
        state.publications.insert("solidcraft-review:1".into(), "attempting".into());
        let mut restored = State::default();
        restored.load(&state.prefs());
        assert_eq!(restored.configs.len(), 2);
        assert_eq!(restored.publications["solidcraft-review:1"], "attempting");
        assert!(issue_identity("https://linear.app/work/issue/EX-1/test").is_some());
        assert!(issue_identity("https://linear.app/work/issue/EX-1?token=secret").is_none());
    }

    #[test]
    fn connected_accounts_do_not_add_requirements_to_other_empty_designs() {
        let mut harness = Harness::new();
        harness.app.issue_sync.configs.push(json!({"workspace":"example", "issue_url":"https://linear.app/example/issue/EX-1", "automatic":true}));
        harness.app.services.issue_system = Some(Arc::new(|_, _| panic!("Unlinked designs must stay local")));
        tick(&mut harness.app, &harness.ctx);
        assert!(harness.app.run("requirements.list", json!({})).unwrap()["spec"].is_null());
    }

    #[test]
    fn publication_is_opt_in_and_ambiguous_results_are_not_retried() {
        let mut harness = Harness::new();
        harness.app.run("requirements.import", json!({"spec":data()["spec"]})).unwrap();
        harness.app.run("requirements.review", json!({"id":"fit", "status":"done", "note":"Checked"})).unwrap();
        let current = harness.app.run("requirements.list", json!({})).unwrap();
        harness.app.issue_sync.read_basis = Some((current["spec"].clone(), 0.0));
        harness.app.issue_sync.configs.push(json!({"workspace":"example", "automatic":true, "publish_reviews":false}));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        harness.app.services.issue_system = Some(Arc::new(move |_, operation| {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(operation["kind"], "comment");
            assert!(operation["body"].as_str().unwrap().contains("solidcraft-review:"));
            let (sender, receiver) = mpsc::channel();
            sender.send(Err("Network interrupted".into())).unwrap();
            receiver
        }));
        tick(&mut harness.app, &harness.ctx);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        harness.app.issue_sync.configs[0]["publish_reviews"] = json!(true);
        harness.app.now = 6.0;
        tick(&mut harness.app, &harness.ctx);
        tick(&mut harness.app, &harness.ctx);
        harness.app.issue_sync.next = 0.0;
        tick(&mut harness.app, &harness.ctx);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(harness.app.issue_sync.publications.values().next().unwrap(), "uncertain");
    }

    #[test]
    fn external_status_is_scoped_to_the_displayed_issue() {
        let mut state = State::default();
        let first = data();
        state.external_status = Some((first["spec"]["source"].clone(), "Backlog".into()));
        assert_eq!(state.status_for(&first), Some("Backlog"));
        let mut second = first.clone();
        second["spec"]["source"]["id"] = json!("EX-2");
        assert_eq!(state.status_for(&second), None);
        assert_eq!(state.status_for(&json!({"spec":null})), None);
    }

    #[test]
    fn publication_history_reclaims_only_confirmed_entries_and_reports_capacity() {
        let mut state = State::default();
        for number in 0..128 {
            state.publications.insert(format!("report-{number}"), "uncertain".into());
        }
        assert!(!state.reserve_publication("new-report"));
        assert!(state.status.contains("history is full"));
        assert_eq!(state.publications.len(), 128);
        state.publications.insert("report-0".into(), "published".into());
        assert!(state.reserve_publication("new-report"));
        assert!(!state.publications.contains_key("report-0"));
        assert_eq!(state.publications["report-1"], "uncertain");
        assert_eq!(state.publications["new-report"], "attempting");
        assert!(!state.reserve_publication("new-report"));
        assert_eq!(state.publications.len(), 128);
    }
}
