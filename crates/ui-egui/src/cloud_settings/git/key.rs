//! Key generation UI. Only the one-shot secret API receives the passphrase.
use super::*;
use solidcraft_cloud::jobs::{SshKey, SshKeyInput};
use zeroize::Zeroizing;

#[derive(Default)]
pub(super) struct Editor {
    passphrase: Zeroizing<String>,
    remember: bool,
    pending: Option<String>,
    generated: Option<SshKey>,
}

impl Editor {
    pub(super) fn editing(&self) -> bool {
        self.generated.is_none()
    }
    pub(super) fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn cancel(&self, app: &mut SolveApp) {
        if let Some(operation) = &self.pending {
            let _ = app.run("cloud.cancel", json!({"operation":operation}));
        }
    }
}

pub(super) fn poll(app: &mut SolveApp, wizard: &mut Wizard) {
    let Some(editor) = &mut wizard.key else { return };
    let Some(id) = &editor.pending else { return };
    let result = (|| -> Result<Option<SshKey>, String> {
        let all = statuses(app);
        let status = all.iter().find(|s| &s.operation == id).ok_or_else(|| tr("cloud.setup_failed").to_owned())?;
        match status.phase {
            Phase::Ready => {
                let key: SshKey =
                    serde_json::from_value(status.result.clone().ok_or_else(|| tr("cloud.setup_failed").to_owned())?).map_err(|e| e.to_string())?;
                // Keep the same path validation as a manually chosen identity.
                let mut connection = wizard.connection.clone();
                connection.ssh_identity = SshIdentity::File(key.path.clone());
                connection.validate().map_err(|e| e.to_string())?;
                if !key.public_key.starts_with("ssh-ed25519 ") || key.public_key.len() > 4096 || key.public_key.chars().any(char::is_control) {
                    return Err(tr("cloud.setup_failed").into());
                }
                Ok(Some(key))
            }
            Phase::Failed | Phase::Cancelled => Err(status.message.clone().unwrap_or_else(|| tr("cloud.setup_failed").into())),
            _ => Ok(None),
        }
    })();
    match result {
        Ok(Some(key)) => {
            wizard.connection.ssh_identity = SshIdentity::File(key.path.clone());
            wizard.verified = None;
            wizard.error = None;
            editor.generated = Some(key);
            editor.pending = None;
        }
        Err(error) => {
            wizard.error = Some(error);
            editor.pending = None;
        }
        Ok(None) => {}
    }
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, wizard: &mut Wizard, enabled: bool) {
    if wizard.key.is_none() {
        if button(ui, "cloud_git:generate", tr("cmd.cloud.ssh.generate"), enabled) {
            wizard.key = Some(Editor::default());
        }
        return;
    }
    if wizard.key.as_ref().and_then(|e| e.generated.as_ref()).is_some_and(|key| wizard.connection.ssh_identity != SshIdentity::File(key.path.clone()))
    {
        wizard.key = None;
        return;
    }
    let Some(editor) = &mut wizard.key else { return };
    if let Some(key) = &editor.generated {
        ui_kit::text(ui, tr("cloud.ssh_created"), ui_kit::Text::Inline);
        ui_kit::row(ui, tr("cloud.ssh_public"), |ui| {
            ui.label(&key.public_key);
        });
        if button(ui, "cloud_git:copy_key", tr("cloud.ssh_copy"), true) {
            ui.ctx().copy_text(key.public_key.clone());
        }
        let remote = wizard.connection.git_remote.as_ref().map(|r| r.fetch.as_str()).unwrap_or(&wizard.connection.remote);
        if let Some(host) = solidcraft_cloud::host_setup(&wizard.connection.provider, remote) {
            ui.hyperlink_to(tr("cloud.ssh_host"), host.ssh_settings_url());
        }
        ui_kit::text(ui, tr("cloud.ssh_register"), ui_kit::Text::Hint("cloud_ssh_register"));
        return;
    }
    if button(ui, "cloud_git:cancel_key", crate::i18n::tr_label("Cancel"), true) {
        editor.cancel(app);
        wizard.key = None;
        return;
    }
    ui_kit::text(ui, tr("cloud.ssh_generate_hint"), ui_kit::Text::Hint("cloud_ssh_generate_hint"));
    ui.add_enabled_ui(enabled, |ui| {
        ui_kit::row(ui, tr("cloud.ssh_passphrase"), |ui| {
            let r = ui_kit::bounded_password_field(ui, egui::Id::new("cloud_ssh_passphrase"), &mut editor.passphrase, "", 1024);
            crate::scenario::publish_target("cloud_git:passphrase", r.rect);
        });
        let r = ui.add_enabled_ui(!editor.passphrase.is_empty(), |ui| ui_kit::checkbox(ui, tr("cloud.ssh_remember"), &mut editor.remember)).inner;
        crate::scenario::publish_target("cloud_git:remember", r.rect);
        if editor.passphrase.is_empty() {
            editor.remember = false;
        }
        ui_kit::text(ui, tr("cloud.ssh_unlock_hint"), ui_kit::Text::Hint("cloud_ssh_unlock_hint"));
        if button(ui, "cloud_git:create_key", tr("cmd.cloud.ssh.generate"), enabled) {
            let input = SshKeyInput { passphrase: std::mem::take(&mut editor.passphrase), remember: editor.remember };
            let result = app.session.set_cloud_ssh_key_input(input).map_err(|e| e.to_string()).and_then(|()| {
                app.run("cloud.ssh.generate", json!({})).and_then(|reply| {
                    editor.pending = Some(reply.get("operation").and_then(|v| v.as_str()).ok_or_else(|| tr("cloud.setup_failed").to_owned())?.into());
                    Ok(())
                })
            });
            app.session.clear_cloud_ssh_key_input();
            wizard.error = result.err();
        }
    });
}
