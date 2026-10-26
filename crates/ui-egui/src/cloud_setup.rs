//! Guided setup is a small command client. Credentials go directly to platform storage;
//! the engine receives only their id. Repository setup and preference application are separate.
use crate::{SolveApp, i18n::tr, ui_kit};
use solidcraft_cloud::{
    Connection,
    jobs::{Job, Phase},
};
mod repository;

#[derive(Default)]
pub(crate) struct Wizard {
    staged: bool,
    adding: bool,
    repository: String,
    token: String,
    tokens: Vec<Connection>,
    token_source: Option<String>,
    candidate: Option<Connection>,
    pending: Option<Pending>,
    error: Option<String>,
    check: repository::Check,
}

enum Pending {
    Credential(Job, Box<Connection>),
    Setup(String),
    Apply(String),
}

pub(crate) fn open(app: &mut SolveApp) {
    if app.cloud.setup.is_none() {
        let connections = crate::cloud_settings::connections(app);
        let tokens: Vec<_> = connections.iter().filter(|c| c.provider == "github" && !c.credential.is_empty()).cloned().collect();
        let repository = connections
            .iter()
            .find(|c| c.provider == "github")
            .and_then(|c| c.repository.split_once('/'))
            .and_then(|(owner, _)| {
                (2..=130)
                    .map(|i| format!("{owner}/solidcraft-designs-{i}"))
                    .find(|name| !connections.iter().any(|c| c.repository.eq_ignore_ascii_case(name)))
            })
            .unwrap_or_default();
        app.cloud.setup = Some(Wizard {
            staged: app.prefs_window.open && app.prefs_window.section == crate::prefs::PAGE_CLOUD,
            adding: !connections.is_empty(),
            repository,
            token_source: tokens.first().map(|c| c.id.clone()),
            tokens,
            ..Default::default()
        });
    }
}

/// Provider setup entries share one menu; another provider adds its setup command here.
pub(crate) fn repository_choices(ui: &mut egui::Ui) -> Option<&'static str> {
    let github = ui_kit::MenuItem::new("cloud_provider_github", tr("cloud.provider_github"), "").show(ui);
    crate::scenario::publish_target("dialog:list:cloud_provider_github", github.rect);
    if github.clicked() {
        ui.close();
        return Some("cloud.connect");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let git = ui_kit::MenuItem::new("cloud_provider_git", tr("cloud.provider_git"), "").show(ui);
        crate::scenario::publish_target("dialog:list:cloud_provider_git", git.rect);
        if git.clicked() {
            ui.close();
            return Some("cloud.configure");
        }
    }
    None
}

pub(crate) fn prefill(app: &mut SolveApp, connection: Connection) {
    if let Some(wizard) = &mut app.cloud.setup {
        wizard.select_token(&connection);
        wizard.repository = connection.repository.clone();
        wizard.adding = false;
        wizard.candidate = Some(connection);
    }
}

impl Wizard {
    fn select_token(&mut self, connection: &Connection) {
        self.tokens.retain(|c| c.id != connection.id);
        self.token_source = (!connection.credential.is_empty()).then(|| connection.id.clone());
        if self.token_source.is_some() {
            self.tokens.push(connection.clone());
        }
    }

    fn credential(&self) -> Option<&str> {
        self.token_source.as_ref().and_then(|id| self.tokens.iter().find(|c| &c.id == id)).map(|c| c.credential.as_str())
    }
}

pub(crate) fn cancel_preferences(app: &mut SolveApp) {
    if !app.cloud.setup.as_ref().is_some_and(|w| w.staged) {
        return;
    }
    if let Some(mut wizard) = app.cloud.setup.take() {
        wizard.check.cancel(app);
        match wizard.pending {
            Some(Pending::Credential(job, _)) => {
                let _ = job.cancel();
            }
            Some(Pending::Setup(operation)) => {
                let _ = app.run("cloud.cancel", serde_json::json!({"operation":operation}));
            }
            _ => {}
        }
    }
}

fn poll(app: &mut SolveApp, wizard: &mut Wizard) -> Result<bool, String> {
    let Some(pending) = wizard.pending.take() else { return Ok(false) };
    let applying = matches!(&pending, Pending::Apply(_));
    match pending {
        Pending::Credential(job, connection) => {
            let status = job.status().map_err(|e| e.to_string())?;
            match status.phase {
                Phase::Ready => {
                    let result = app.run("cloud.connect", serde_json::json!({"connection":connection})).map_err(|e| e.to_string())?;
                    let id = result.get("operation").and_then(serde_json::Value::as_str).ok_or("missing setup operation")?;
                    wizard.pending = Some(Pending::Setup(id.into()));
                }
                Phase::Failed | Phase::Cancelled => return Err(status.message.unwrap_or_else(|| tr("cloud.setup_failed").into())),
                _ => wizard.pending = Some(Pending::Credential(job, connection)),
            }
        }
        Pending::Setup(id) | Pending::Apply(id) => {
            let mut statuses = app.session.cloud_operations().map_err(|e| e.to_string())?;
            for slot in app.docs.background_mut() {
                statuses.extend(slot.session.cloud_operations().map_err(|e| e.to_string())?);
            }
            let status = statuses.into_iter().find(|s| s.operation == id).ok_or("setup operation no longer available")?;
            match status.phase {
                Phase::Ready | Phase::Synced => {
                    if applying {
                        if let Some(connection) = &wizard.candidate {
                            crate::cloud_browser::select(app, &connection.id);
                        }
                        return Ok(true);
                    }
                    let connection: Connection = serde_json::from_value(status.result.ok_or("setup result missing")?).map_err(|e| e.to_string())?;
                    wizard.candidate = Some(connection.clone());
                    if wizard.staged {
                        crate::cloud_settings::stage(app, connection)?;
                        return Ok(true);
                    }
                    let mut settings = app.session.cloud_settings().clone();
                    // Merge into the latest preferences, preserving other connected repos.
                    if let Some(existing) = settings.connections.iter_mut().find(|c| c.id == connection.id) {
                        *existing = connection.clone();
                    } else {
                        settings.connections.push(connection.clone());
                    }
                    if settings.default_connection.is_none() {
                        settings.default_connection = Some(connection.id);
                    }
                    let result = app.run("cloud.configure", serde_json::json!({"settings":settings})).map_err(|e| e.to_string())?;
                    wizard.pending =
                        Some(Pending::Apply(result.get("operation").and_then(serde_json::Value::as_str).ok_or("missing settings operation")?.into()));
                }
                Phase::Failed | Phase::Cancelled => return Err(status.message.unwrap_or_else(|| tr("cloud.setup_failed").into())),
                _ => wizard.pending = Some(if applying { Pending::Apply(id) } else { Pending::Setup(id) }),
            }
        }
    }
    Ok(false)
}

pub(crate) fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut wizard) = app.cloud.setup.take() else { return };
    match poll(app, &mut wizard) {
        Ok(true) => {
            app.set_status(tr(if wizard.staged { "cloud.setup_ready" } else { "cloud.connected" }), false);
            return;
        }
        Err(error) => {
            // Keep the New form stable during setup; a saved token remains available on retry.
            if wizard.token_source.is_none()
                && wizard.token.is_empty()
                && let Some(connection) = wizard.candidate.clone()
            {
                wizard.select_token(&connection);
            }
            wizard.error = Some(error);
        }
        _ => {}
    }
    let busy = wizard.pending.is_some();
    let mut check = std::mem::take(&mut wizard.check);
    check.tick(app, &wizard, ctx);
    wizard.check = check;
    let valid = !busy
        && !wizard.check.connected()
        && !app.session.cloud_configuring()
        && solidcraft_cloud::setup::github_connection(&wizard.repository).is_ok()
        && (wizard.credential().is_some() || solidcraft_cloud::setup::check_token(&wizard.token).is_ok());
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_setup"), "settings", tr("cmd.cloud.connect"))
        .fields()
        .cards(2.0)
        .actions(tr("cmd.cloud.connect"), "Cancel", None)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, tr("cloud.setup_hint"), ui_kit::Text::Inline);
            ui.add_enabled_ui(!busy, |ui| {
                ui_kit::row(ui, tr("cloud.repository"), |ui| {
                    let r = ui_kit::checked_cell_editor(
                        ui,
                        egui::Id::new("cloud_repository"),
                        &mut wizard.repository,
                        crate::i18n::tr_label("Automatic"),
                        512,
                        wizard.check.status(),
                    );
                    let r = if wizard.repository.trim().is_empty() { r.on_hover_text(tr("cloud.repository_automatic")) } else { r };
                    crate::scenario::publish_target("cloud:repository", r.rect);
                });
                let url = solidcraft_cloud::setup::github_create_repository_url(&wizard.repository)
                    .unwrap_or_else(|_| solidcraft_cloud::github_setup().create_repository_url().into());
                ui_kit::row(ui, "", |ui| {
                    let r = ui.hyperlink_to(tr("ui.create"), url);
                    crate::scenario::publish_target("cloud:create_repo", r.rect);
                });
                if wizard.adding {
                    ui_kit::text(ui, tr("cloud.additional_repository"), ui_kit::Text::Hint("cloud_additional_repository"));
                }
                ui_kit::row(ui, tr("cloud.token"), |ui| {
                    let selected = wizard
                        .tokens
                        .iter()
                        .find(|c| Some(&c.id) == wizard.token_source.as_ref())
                        .map(Connection::repository_label)
                        .unwrap_or_else(|| tr("cloud.token_new").into());
                    let response = ui_kit::dropdown_menu(ui, "cloud_token_source", &selected, |ui| {
                        for connection in &wizard.tokens {
                            let chosen = wizard.token_source.as_ref() == Some(&connection.id);
                            let response =
                                ui_kit::list_picker(ui, &format!("cloud:token-source:{}", connection.id), &connection.repository_label(), chosen);
                            crate::scenario::publish_target(&format!("cloud:token-source:{}", connection.id), response.rect);
                            if response.clicked() {
                                wizard.token_source = Some(connection.id.clone());
                            }
                        }
                        let response = ui_kit::list_picker(ui, "cloud:token-new", tr("cloud.token_new"), wizard.token_source.is_none());
                        crate::scenario::publish_target("cloud:token-new", response.rect);
                        if response.clicked() {
                            wizard.token_source = None;
                        }
                    });
                    crate::scenario::publish_target("cloud:token-source", response.rect);
                });
                if wizard.token_source.is_none() {
                    let url = solidcraft_cloud::setup::github_token_url(&wizard.repository)
                        .unwrap_or_else(|_| solidcraft_cloud::github_setup().create_token_url().into());
                    let response = ui.hyperlink_to(tr("cloud.create_token"), url);
                    crate::scenario::publish_target("cloud:create_token", response.rect);
                    ui_kit::text(ui, tr("cloud.token_prefill"), ui_kit::Text::Hint("cloud_token_prefill"));
                    ui_kit::text(ui, tr("cloud.token_scope"), ui_kit::Text::Hint("cloud_token_scope"));
                    ui_kit::row(ui, tr("cloud.token_value"), |ui| {
                        let r = ui_kit::bounded_password_field(ui, egui::Id::new("cloud_token"), &mut wizard.token, "", 8192);
                        crate::scenario::publish_target("cloud:token", r.rect);
                    });
                    ui_kit::text(ui, tr("cloud.token_enter"), ui_kit::Text::Hint("cloud_token_enter"));
                } else {
                    ui_kit::text(ui, tr("cloud.token_reuse"), ui_kit::Text::Hint("cloud_token_reuse"));
                }
            });
            if busy {
                ui.spinner();
                ui.label(tr("cloud.connecting"));
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            if let Some(error) = &wizard.error {
                ui_kit::text(ui, error, ui_kit::Text::Report("cloud_setup_error", true));
            }
            ui.hyperlink_to(crate::i18n::tr_label("Help"), "https://tangency-labs.github.io/solidcraft/guide/cloud-saves.html");
        });
    if closed || action == ui_kit::Action::Cancel {
        wizard.check.cancel(app);
        match wizard.pending {
            Some(Pending::Credential(job, _)) => {
                let _ = job.cancel();
            }
            Some(Pending::Setup(operation)) => {
                let _ = app.run("cloud.cancel", serde_json::json!({"operation":operation}));
            }
            // An Apply already persisted cannot be rolled back by closing a dialog.
            _ => {}
        }
        return;
    }
    if action == ui_kit::Action::Ok && valid {
        wizard.check.cancel(app);
        wizard.error = None;
        let result = (|| -> Result<Pending, String> {
            let mut connection = solidcraft_cloud::setup::github_connection(&wizard.repository).map_err(|e| e.to_string())?;
            if let Some(existing) = wizard
                .candidate
                .as_ref()
                .filter(|c| c.id == connection.id)
                .or_else(|| app.session.cloud_settings().connections.iter().find(|c| c.id == connection.id))
            {
                connection = existing.clone();
            }
            if let Some(credential) = wizard.credential() {
                connection.credential = credential.into();
                wizard.candidate = Some(connection.clone());
                let result = app.run("cloud.connect", serde_json::json!({"connection":connection})).map_err(|e| e.to_string())?;
                return Ok(Pending::Setup(result.get("operation").and_then(serde_json::Value::as_str).ok_or("missing setup operation")?.into()));
            }
            // An invalid replacement must not destroy a working connection's credential.
            connection.credential = format!(
                "{}:{}",
                connection.credential.split(':').take(2).collect::<Vec<_>>().join(":"),
                solidcraft_cloud::digest(wizard.token.as_bytes())
            );
            wizard.candidate = Some(connection.clone());
            let host = app.services.cloud_host.as_ref().ok_or("cloud storage unavailable")?;
            let job = host.store_credential(connection.credential.clone(), std::mem::take(&mut wizard.token)).map_err(|e| e.to_string())?;
            Ok(Pending::Credential(job, Box::new(connection)))
        })();
        match result {
            Ok(pending) => wizard.pending = Some(pending),
            Err(error) => wizard.error = Some(error),
        }
    }
    app.cloud.setup = Some(wizard);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, scenario::Harness};
    use solidcraft_cloud::{
        Settings,
        jobs::{Host, Operation},
    };
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Provider {
        setups: Mutex<Vec<(Job, Connection)>>,
        credentials: Mutex<Vec<(String, String)>>,
        checks: Mutex<Vec<(Job, Connection, bool)>>,
    }
    impl Host for Provider {
        fn check_repository(&self, connection: Connection, input: solidcraft_cloud::jobs::RepositoryCheckInput) -> solidcraft_cloud::Result<Job> {
            let job = Job::new(connection.id.clone())?;
            self.checks.lock().unwrap().push((job.clone(), connection, input.token.is_some()));
            Ok(job)
        }
        fn submit(&self, operation: Operation, _: String) -> solidcraft_cloud::Result<Job> {
            let job = Job::new(String::new())?;
            match operation {
                Operation::LoadSettings => job.update(|s| {
                    s.result = Some(serde_json::json!(Settings::default()));
                    s.phase = Phase::Ready;
                })?,
                Operation::Configure(settings) => job.update(|s| {
                    s.result = Some(serde_json::json!(settings));
                    s.phase = Phase::Synced;
                })?,
                Operation::Setup(connection) => self.setups.lock().unwrap().push((job.clone(), connection)),
                _ => job.update(|s| s.phase = Phase::Ready)?,
            }
            Ok(job)
        }
        fn store_credential(&self, id: String, token: String) -> solidcraft_cloud::Result<Job> {
            self.credentials.lock().unwrap().push((id, token));
            let job = Job::new(String::new())?;
            job.update(|s| s.phase = Phase::Ready)?;
            Ok(job)
        }
    }

    #[test]
    fn repository_override_and_create_keep_the_shared_control_alignment() {
        for dark in [false, true] {
            let mut h = Harness::new();
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.app.start("cloud.connect");
            h.frames(8);
            let field = crate::scenario::rect_of("cloud:repository").unwrap();
            let create = crate::scenario::rect_of("cloud:create_repo").unwrap();
            assert!((field.left() - create.left()).abs() < 1.0);
            assert!(create.top() >= field.bottom());
            h.step(&serde_json::json!({"click":{"handle":"cloud:repository"}})).unwrap();
            h.step(&serde_json::json!({"text":"work/designs"})).unwrap();
            assert_eq!(h.app.cloud.setup.as_ref().unwrap().repository, "work/designs");
            h.frames(2);
            assert_eq!(crate::scenario::rect_of("cloud:repository"), Some(field));
            assert!(crate::scenario::rect_of("cloud:create_token").is_some());
            assert_eq!(h.app.cloud.setup.as_ref().unwrap().repository, "work/designs");
        }
    }

    #[test]
    fn repository_checks_debounce_cancel_stale_requests_and_detect_connected_repositories_locally() {
        for dark in [false, true] {
            let provider = Arc::new(Provider::default());
            let mut h = Harness::new();
            h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.frame();
            let connected = solidcraft_cloud::setup::github_connection("work/first").unwrap();
            h.app
                .run(
                    "cloud.configure",
                    serde_json::json!({"settings":Settings{connections:vec![connected.clone()],retry_pending:false,..Default::default()}}),
                )
                .unwrap();
            h.frames(2);
            h.app.start("cloud.connect");
            h.frames(8);
            let field = crate::scenario::rect_of("cloud:repository").unwrap();
            let create = crate::scenario::rect_of("cloud:create_repo").unwrap();
            assert!((field.left() - create.left()).abs() < 1.0);
            assert!(provider.checks.lock().unwrap().is_empty());
            h.frames(30);
            let (old, _, fresh) = provider.checks.lock().unwrap().last().unwrap().clone();
            assert!(!fresh, "saved tokens use opaque references");
            let status = crate::scenario::rect_of("dialog:input_status").unwrap();
            assert!(field.contains_rect(status), "spinner must be inside the input");
            h.app.cloud.setup.as_mut().unwrap().repository = "work/other".into();
            h.frames(2);
            assert_eq!(old.status().unwrap().phase, Phase::Cancelled);
            old.update(|s| {
                s.result = Some(serde_json::json!({"exists":true}));
                s.phase = Phase::Ready;
            })
            .unwrap();
            h.frames(30);
            let current = provider.checks.lock().unwrap().last().unwrap().0.clone();
            current
                .update(|s| {
                    s.result = Some(serde_json::json!({"exists":false}));
                    s.phase = Phase::Ready;
                })
                .unwrap();
            h.frames(2);
            assert!(matches!(h.app.cloud.setup.as_ref().unwrap().check.status(), Some(ui_kit::InputStatus::Warning(_))));
            let count = provider.checks.lock().unwrap().len();
            h.frames(40);
            assert_eq!(provider.checks.lock().unwrap().len(), count, "unchanged input is checked once");
            h.app.cloud.setup.as_mut().unwrap().repository = "https://github.com/WORK/first.git".into();
            h.frames(2);
            assert!(h.app.cloud.setup.as_ref().unwrap().check.connected());
            assert_eq!(provider.checks.lock().unwrap().len(), count, "already connected is a local check");
            assert_eq!(crate::scenario::rect_of("cloud:repository"), Some(field));
            assert_eq!(crate::scenario::rect_of("cloud:create_repo"), Some(create));
            assert!(provider.credentials.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn default_setup_needs_only_the_token_and_keeps_account_discovery_in_the_provider() {
        let provider = Arc::new(Provider::default());
        let mut h = Harness::new();
        h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
        h.frame();
        h.app.start("cloud.connect");
        h.frames(2);
        assert!(crate::scenario::rect_of("cloud:create_token").is_some());
        assert!(h.app.cloud.setup.as_ref().unwrap().token_source.is_none());
        assert!(h.app.cloud.setup.as_ref().unwrap().tokens.is_empty());
        assert!(h.app.cloud.setup.as_ref().unwrap().credential().is_none());
        h.app.cloud.setup.as_mut().unwrap().token = "test-token".into();
        h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(3);
        let (job, proposed) = provider.setups.lock().unwrap().last().unwrap().clone();
        assert!(proposed.repository.is_empty());
        let discovered =
            Connection { credential: proposed.credential, ..solidcraft_cloud::setup::github_connection("discovered/solidcraft-designs").unwrap() };
        job.update(|status| {
            status.result = Some(serde_json::json!(discovered));
            status.phase = Phase::Ready;
        })
        .unwrap();
        h.frames(4);
        assert!(h.app.cloud.setup.is_none());
        assert_eq!(h.app.session.cloud_settings().connections, vec![discovered]);
    }

    #[test]
    fn choose_an_existing_token_or_new_without_changing_other_connections() {
        for dark in [false, true] {
            let provider = Arc::new(Provider::default());
            let mut h = Harness::new();
            h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.frame();
            let first = solidcraft_cloud::setup::github_connection("work/first").unwrap();
            let second = solidcraft_cloud::setup::github_connection("work/second").unwrap();
            let settings = Settings { connections: vec![first.clone(), second.clone()], retry_pending: false, ..Default::default() };
            h.app.run("cloud.configure", serde_json::json!({"settings":settings})).unwrap();
            h.frames(2);
            h.app.start("cloud.connect");
            h.frames(2);
            assert_eq!(h.app.cloud.setup.as_ref().unwrap().credential(), Some(first.credential.as_str()));
            assert!(crate::scenario::rect_of("cloud:token").is_none());
            h.step(&serde_json::json!({"click":{"handle":"cloud:token-source"}})).unwrap();
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":format!("cloud:token-source:{}", second.id)}})).unwrap();
            h.frames(2);
            assert_eq!(h.app.cloud.setup.as_ref().unwrap().credential(), Some(second.credential.as_str()));
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(2);
            let (job, proposed) = provider.setups.lock().unwrap().last().unwrap().clone();
            assert_eq!(proposed.credential, second.credential);
            assert_ne!(proposed.id, second.id);
            assert!(provider.credentials.lock().unwrap().is_empty(), "reuse never reads or copies a secret through the UI");
            job.fail(solidcraft_cloud::Error::Storage("token does not grant access to the additional repository".into())).unwrap();
            h.frames(2);
            assert_eq!(h.app.session.cloud_settings(), &settings);
            h.step(&serde_json::json!({"click":{"handle":"cloud:token-source"}})).unwrap();
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":"cloud:token-new"}})).unwrap();
            h.frames(2);
            assert!(h.app.cloud.setup.as_ref().unwrap().credential().is_none());
            assert!(crate::scenario::rect_of("cloud:create_token").is_some());
            assert!(crate::scenario::rect_of("cloud:token").is_some());
            let count = provider.setups.lock().unwrap().len();
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(2);
            assert_eq!(provider.setups.lock().unwrap().len(), count, "New requires a token");
            h.app.cloud.setup.as_mut().unwrap().token = "separate-token".into();
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(3);
            let (job, replacement) = provider.setups.lock().unwrap().last().unwrap().clone();
            assert_ne!(replacement.credential, second.credential);
            assert_eq!(provider.credentials.lock().unwrap().len(), 1);
            assert_eq!(h.app.session.cloud_settings(), &settings);
            job.fail(solidcraft_cloud::Error::Storage("offline".into())).unwrap();
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":"dialog:cancel"}})).unwrap();
            h.app.start("cloud.connect");
            prefill(&mut h.app, second.clone());
            h.frames(2);
            assert_eq!(h.app.cloud.setup.as_ref().unwrap().credential(), Some(second.credential.as_str()), "reconnect selects its own token");
        }
    }

    #[test]
    fn token_reuse_includes_staged_connections_and_persists_only_the_credential_reference() {
        let provider = Arc::new(Provider::default());
        let mut h = Harness::new();
        h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
        h.frame();
        crate::cloud_settings::begin(&mut h.app);
        let existing = solidcraft_cloud::setup::github_connection("work/staged").unwrap();
        crate::cloud_settings::stage(&mut h.app, existing.clone()).unwrap();
        h.app.prefs_window.open = true;
        h.app.prefs_window.section = crate::prefs::PAGE_CLOUD;
        h.app.start("cloud.connect");
        h.frames(2);
        assert_eq!(h.app.cloud.setup.as_ref().unwrap().credential(), Some(existing.credential.as_str()));
        assert!(h.app.session.cloud_settings().connections.is_empty());
        h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
        h.frames(2);
        let (job, connection) = provider.setups.lock().unwrap().last().unwrap().clone();
        assert_eq!(connection.credential, existing.credential);
        assert_ne!(connection.id, existing.id);
        job.update(|s| {
            s.phase = Phase::Ready;
            s.result = Some(serde_json::json!(connection));
        })
        .unwrap();
        h.frames(4);
        assert!(h.app.session.cloud_settings().connections.is_empty(), "setup honors Apply/Cancel");
        let connections = crate::cloud_settings::connections(&h.app);
        assert_eq!(
            connections.len(),
            2,
            "staged connections: {connections:?}; setup error: {:?}",
            h.app.cloud.setup.as_ref().and_then(|w| w.error.as_ref())
        );
        assert!(connections.iter().all(|c| c.credential == existing.credential));
        assert!(!crate::cloud_settings::apply(&mut h.app, false));
        h.frames(4);
        let settings = h.app.session.cloud_settings();
        let restored: Settings = serde_json::from_str(&serde_json::to_string(settings).unwrap()).unwrap();
        assert_eq!(restored, *settings);
        assert_eq!(restored.connections.len(), 2);
        assert!(provider.credentials.lock().unwrap().is_empty());
    }

    #[test]
    fn guided_setup_applies_only_verified_connections_preserves_others_and_cancels() {
        for dark in [false, true] {
            let provider = Arc::new(Provider::default());
            let mut h = Harness::new();
            h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
            h.app.preferences.theme = if dark { "dark" } else { "light" }.into();
            h.frame();
            // Keep an existing independent connection and its selected default.
            let existing = solidcraft_cloud::setup::github_connection("work/existing").unwrap();
            h.app.run("cloud.configure", serde_json::json!({"settings":Settings { connections:vec![existing.clone()], default_connection:Some(existing.id.clone()), retry_pending:false, ..Default::default() }})).unwrap();
            h.frame();
            h.app.home.show(0);
            h.app.start("cloud.connect");
            h.frames(2);
            assert!(crate::scenario::rect_of("cloud:create_token").is_none(), "an existing token is selected without requesting a new one");
            assert!(crate::scenario::rect_of("cloud:repository").is_some(), "adding storage exposes the independent repository");
            assert_eq!(h.app.cloud.setup.as_ref().unwrap().repository, "work/solidcraft-designs-2");
            h.step(&serde_json::json!({"click":{"handle":"cloud:token-source"}})).unwrap();
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":"cloud:token-new"}})).unwrap();
            h.frames(2);
            {
                let wizard = h.app.cloud.setup.as_mut().unwrap();
                wizard.repository = "owner/designs".into();
                wizard.token = "test-token-only-in-platform-store".into();
            }
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(3);
            assert!(h.app.cloud.setup.as_ref().unwrap().token.is_empty());
            assert!(h.app.cloud.setup.as_ref().unwrap().token_source.is_none(), "the New form does not collapse while setup is busy");
            assert!(crate::scenario::rect_of("cloud:token").is_some());
            assert_eq!(h.app.session.cloud_settings().connections.len(), 1, "unverified setup cannot change settings");
            let (job, connection) = provider.setups.lock().unwrap().last().unwrap().clone();
            assert!(!serde_json::to_string(&connection).unwrap().contains("test-token"));
            assert_eq!(provider.credentials.lock().unwrap()[0].1, "test-token-only-in-platform-store");
            job.fail(solidcraft_cloud::Error::Storage("offline".into())).unwrap();
            h.frames(2);
            assert_eq!(h.app.session.cloud_settings().connections.len(), 1);
            assert!(h.app.cloud.setup.as_ref().unwrap().error.is_some());
            // The token is already in the platform store. Retry without pasting it again.
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(2);
            let (job, retried) = provider.setups.lock().unwrap().last().unwrap().clone();
            assert_eq!(retried.credential, connection.credential);
            assert_eq!(provider.credentials.lock().unwrap().len(), 1);
            h.app.session.doc_mut().name = "Setup owner tab".into();
            crate::documents::open_tab(&mut h.app, Default::default()).unwrap();
            h.frame();
            let operation = job.status().unwrap().operation;
            assert!(h.app.run("cloud.status", serde_json::json!({"operation":operation})).is_ok());
            job.update(|s| {
                s.result = Some(serde_json::json!(connection));
                s.phase = Phase::Ready;
            })
            .unwrap();
            h.frames(4);
            assert!(h.app.cloud.setup.is_none());
            assert_eq!(h.app.session.cloud_settings().connections.len(), 2);
            assert_eq!(h.app.session.cloud_settings().default_connection.as_deref(), Some(existing.id.as_str()));
            h.app.start("cloud.connect");
            h.app.cloud.setup.as_mut().unwrap().repository = "owner/other".into();
            h.frames(2);
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(2);
            let job = provider.setups.lock().unwrap().last().unwrap().0.clone();
            h.step(&serde_json::json!({"click":{"handle":"dialog:cancel"}})).unwrap();
            assert!(job.cancelled().unwrap());
            h.frames(2);
            assert!(h.app.cloud.setup.is_none());
            assert_eq!(h.app.session.cloud_settings().connections.len(), 2);
        }
    }
}
