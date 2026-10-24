//! Guided setup is a small command client. Credentials go directly to platform storage;
//! the engine receives only their id. Repository setup and preference application are separate.
use crate::{SolveApp, i18n::tr, ui_kit};
use solidcraft_cloud::{
    Connection,
    jobs::{Job, Phase},
};

#[derive(Default)]
pub(crate) struct Wizard {
    staged: bool,
    repository: String,
    token: String,
    candidate: Option<Connection>,
    pending: Option<Pending>,
    error: Option<String>,
}

enum Pending {
    Credential(Job, Box<Connection>),
    Setup(String),
    Apply(String),
}

pub(crate) fn open(app: &mut SolveApp) {
    if app.cloud.setup.is_none() {
        app.cloud.setup =
            Some(Wizard { staged: app.prefs_window.open && app.prefs_window.section == crate::prefs::PAGE_CLOUD, ..Default::default() });
    }
}

pub(crate) fn prefill(app: &mut SolveApp, connection: Connection) {
    if let Some(wizard) = &mut app.cloud.setup {
        wizard.repository = connection.repository.clone();
        wizard.candidate = Some(connection);
    }
}

pub(crate) fn cancel_preferences(app: &mut SolveApp) {
    if !app.cloud.setup.as_ref().is_some_and(|w| w.staged) {
        return;
    }
    if let Some(wizard) = app.cloud.setup.take() {
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
                        return Ok(true);
                    }
                    let connection: Connection = serde_json::from_value(status.result.ok_or("setup result missing")?).map_err(|e| e.to_string())?;
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
            wizard.error = Some(error);
        }
        _ => {}
    }
    let busy = wizard.pending.is_some();
    let valid = !busy && !app.session.cloud_configuring() && solidcraft_cloud::setup::github_connection(&wizard.repository).is_ok();
    let (closed, action) = ui_kit::Window::new(egui::Id::new("cloud_setup"), "settings", tr("cmd.cloud.connect"))
        .fields()
        .cards(2.0)
        .actions(tr("cmd.cloud.connect"), "Cancel", None)
        .show(ctx, valid, |ui| {
            ui_kit::text(ui, tr("cloud.setup_hint"), ui_kit::Text::Inline);
            ui.add_enabled_ui(!busy, |ui| {
                ui.hyperlink_to(tr("cloud.create_repo"), solidcraft_cloud::github_setup().create_repository_url());
                if wizard.repository.trim().is_empty() {
                    ui_kit::text(ui, tr("cloud.repository_automatic"), ui_kit::Text::Hint("cloud_repository_automatic"));
                } else {
                    ui_kit::row(ui, tr("cloud.repository"), |ui| {
                        ui.label(&wizard.repository);
                    });
                }
                egui::CollapsingHeader::new(crate::i18n::tr_label("Advanced")).default_open(!wizard.repository.is_empty()).show(ui, |ui| {
                    ui_kit::row(ui, tr("cloud.repository"), |ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut wizard.repository).hint_text("owner/solidcraft-designs").char_limit(512));
                        crate::scenario::publish_target("cloud:repository", r.rect);
                    });
                });
                let url = solidcraft_cloud::setup::github_token_url(&wizard.repository)
                    .unwrap_or_else(|_| solidcraft_cloud::github_setup().create_token_url().into());
                let response = ui.hyperlink_to(tr("cloud.create_token"), url);
                crate::scenario::publish_target("cloud:create_token", response.rect);
                ui_kit::text(ui, tr("cloud.token_prefill"), ui_kit::Text::Hint("cloud_token_prefill"));
                ui_kit::text(ui, tr("cloud.token_scope"), ui_kit::Text::Hint("cloud_token_scope"));
                ui_kit::row(ui, tr("cloud.token"), |ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut wizard.token).password(true).char_limit(8192));
                    crate::scenario::publish_target("cloud:token", r.rect);
                });
                ui_kit::text(ui, tr("cloud.token_retry"), ui_kit::Text::Hint("cloud_token_retry"));
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
            if wizard.token.is_empty() {
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
    }
    impl Host for Provider {
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
    fn default_setup_needs_only_the_token_and_keeps_account_discovery_in_the_provider() {
        let provider = Arc::new(Provider::default());
        let mut h = Harness::new();
        h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
        h.frame();
        h.app.start("cloud.connect");
        h.frames(2);
        assert!(crate::scenario::rect_of("cloud:create_token").is_some());
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
    fn guided_setup_applies_only_verified_connections_preserves_others_and_cancels() {
        for dark in [false, true] {
            let provider = Arc::new(Provider::default());
            let mut h = Harness::new();
            h.app = SolveApp::new(Default::default(), Services { cloud_host: Some(provider.clone()), ..Default::default() });
            h.app.ui.dark = dark;
            h.frame();
            // Keep an existing independent connection and its selected default.
            let existing = solidcraft_cloud::setup::github_connection("work/existing").unwrap();
            h.app.run("cloud.configure", serde_json::json!({"settings":Settings { connections:vec![existing.clone()], default_connection:Some(existing.id.clone()), retry_pending:false, ..Default::default() }})).unwrap();
            h.frame();
            h.app.home.show(0);
            h.app.start("cloud.connect");
            h.frames(2);
            assert!(
                crate::scenario::rect_of("cloud:create_token").is_some(),
                "token creation is visible immediately, before entering any repository"
            );
            assert!(crate::scenario::rect_of("cloud:repository").is_none(), "the account and default repository are automatic");
            {
                let wizard = h.app.cloud.setup.as_mut().unwrap();
                wizard.repository = "owner/designs".into();
                wizard.token = "test-token-only-in-platform-store".into();
            }
            h.frame();
            h.step(&serde_json::json!({"click":{"handle":"dialog:ok"}})).unwrap();
            h.frames(3);
            assert!(h.app.cloud.setup.as_ref().unwrap().token.is_empty());
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
