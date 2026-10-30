//! Standalone Git credentials. Only opaque references enter preferences and commands.
use super::*;
use solidcraft_cloud::jobs::Job;
use zeroize::{Zeroize, Zeroizing};

#[derive(Default)]
pub(super) struct Editor {
    password: Zeroizing<String>,
    pending: Option<(Job, String)>,
}

impl Editor {
    pub(super) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn unchanged(&self) -> bool {
        self.password.is_empty() && !self.busy()
    }

    pub(super) fn valid(&self, connection: &Connection) -> bool {
        connection.authentication != Authentication::Https
            || (!connection.https_username.is_empty()
                && ((!connection.credential.is_empty() && self.password.is_empty())
                    || solidcraft_cloud::credentials::check_password(&self.password).is_ok()))
    }

    pub(super) fn cancel(&self) {
        if let Some((job, _)) = &self.pending {
            let _ = job.cancel();
        }
    }

    pub(super) fn clear(&mut self) {
        self.password.zeroize();
    }
}

pub(super) fn show(ui: &mut egui::Ui, wizard: &mut Wizard) {
    if wizard.connection.authentication != Authentication::Https {
        return;
    }
    field(ui, "cloud.git_username", &mut wizard.connection.https_username, false);
    ui_kit::row(ui, tr("cloud.git_password"), |ui| {
        let response = ui_kit::bounded_password_field(ui, egui::Id::new("cloud_git_password"), &mut wizard.https.password, "", 1024);
        crate::scenario::publish_target("cloud_git:password", response.rect);
    });
    ui_kit::text(ui, tr("cloud.git_password_hint"), ui_kit::Text::Hint("cloud_git_password_hint"));
}

/// Store a fresh entry rather than replacing a working connection's password.
pub(super) fn test(app: &mut SolveApp, wizard: &mut Wizard) -> Result<(), String> {
    if wizard.connection.authentication == Authentication::Https && !wizard.https.password.is_empty() {
        let secret = solidcraft_cloud::credentials::encode_password(&wizard.https.password).map_err(|e| e.to_string())?;
        let host = app.services.cloud_host.as_ref().ok_or_else(|| tr("cloud.setup_failed").to_owned())?;
        let nonce = Job::new(String::new()).map_err(|e| e.to_string())?.status().map_err(|e| e.to_string())?.operation;
        let time = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let id = format!("cloud:git-https:{time}:{nonce}");
        let job = host.store_credential(id.clone(), secret).map_err(|e| e.to_string())?;
        wizard.https.pending = Some((job, id));
        return Ok(());
    }
    super::test(app, wizard)
}

pub(super) fn poll(app: &mut SolveApp, wizard: &mut Wizard) -> Result<(), String> {
    let Some((job, id)) = wizard.https.pending.take() else { return Ok(()) };
    let status = job.status().map_err(|e| e.to_string())?;
    match status.phase {
        Phase::Ready => {
            wizard.connection.credential = id;
            wizard.https.password.zeroize();
            wizard.verified = None;
            super::test(app, wizard)
        }
        Phase::Failed | Phase::Cancelled => Err(status.message.unwrap_or_else(|| tr("cloud.setup_failed").into())),
        _ => {
            wizard.https.pending = Some((job, id));
            Ok(())
        }
    }
}
