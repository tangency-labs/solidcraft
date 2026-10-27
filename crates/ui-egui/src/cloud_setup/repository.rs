//! Debounced, read-only repository checks. UI state contains credential references or
//! fingerprints; one-shot token bytes pass to the host outside recorded commands.
use super::*;

#[derive(Default)]
pub(super) struct Check {
    key: Option<(String, String)>,
    next: f64,
    started: f64,
    operation: Option<String>,
    outcome: Option<Outcome>,
    focused: bool,
    checking: bool,
}

enum Outcome {
    Available,
    Unavailable,
    Connected,
    Invalid,
    Failed(String),
}

impl Check {
    pub(super) fn cancel(&mut self, app: &mut SolveApp) {
        if let Some(operation) = self.operation.take() {
            let _ = app.run("cloud.cancel", serde_json::json!({"operation":operation}));
        }
    }

    pub(super) fn connected(&self) -> bool {
        matches!(self.outcome, Some(Outcome::Connected))
    }

    pub(super) fn status(&self) -> Option<ui_kit::InputStatus<'_>> {
        Some(match &self.outcome {
            Some(Outcome::Available) => ui_kit::InputStatus::Valid(tr("cloud.repository_available")),
            Some(Outcome::Unavailable) => ui_kit::InputStatus::Warning(tr("cloud.repository_unavailable")),
            Some(Outcome::Connected) => ui_kit::InputStatus::Warning(tr("cloud.repository_connected")),
            Some(Outcome::Invalid) => ui_kit::InputStatus::Invalid(tr("cloud.repository_invalid")),
            Some(Outcome::Failed(error)) => ui_kit::InputStatus::Warning(error),
            None if self.key.is_some() && self.checking => ui_kit::InputStatus::Checking(tr("update.checking")),
            None => return None,
        })
    }

    pub(super) fn tick(&mut self, app: &mut SolveApp, wizard: &Wizard, ctx: &egui::Context) {
        let (now, focused) = ctx.input(|i| (i.time, i.viewport().focused.unwrap_or(true)));
        let now = if now.is_finite() { now.max(0.0) } else { 0.0 };
        let returned = focused && !self.focused;
        self.focused = focused;
        self.checking = wizard.pending.is_none() && (wizard.credential().is_some() || solidcraft_cloud::setup::check_token(&wizard.token).is_ok());
        let repository = solidcraft_cloud::setup::github_repository(&wizard.repository);
        let key = repository.as_ref().ok().map(|name| {
            (name.to_ascii_lowercase(), wizard.credential().map(str::to_owned).unwrap_or_else(|| solidcraft_cloud::digest(wizard.token.as_bytes())))
        });
        if key != self.key || (returned && self.operation.is_none() && matches!(self.outcome, Some(Outcome::Unavailable | Outcome::Failed(_)))) {
            self.cancel(app);
            self.key = key;
            self.outcome = None;
            self.next = now + 0.4;
        }
        if wizard.repository.trim().is_empty() {
            self.outcome = None;
            return;
        }
        let Ok(repository) = repository else {
            self.outcome = Some(Outcome::Invalid);
            return;
        };
        if wizard.adding
            && crate::cloud_settings::connections(app).iter().any(|c| c.provider == "github" && c.repository.eq_ignore_ascii_case(&repository))
        {
            self.cancel(app);
            self.outcome = Some(Outcome::Connected);
            return;
        }
        if matches!(self.outcome, Some(Outcome::Connected)) {
            self.outcome = None;
            self.next = now + 0.4;
        }
        if wizard.pending.is_some() {
            self.cancel(app);
            return;
        }
        if wizard.credential().is_none() && solidcraft_cloud::setup::check_token(&wizard.token).is_err() {
            return;
        }
        if let Some(operation) = &self.operation {
            let statuses = app.session.cloud_operations().unwrap_or_default();
            match statuses.iter().find(|s| &s.operation == operation) {
                Some(status) if status.phase == Phase::Ready => {
                    self.outcome = Some(if status.result.as_ref().and_then(|v| v.get("exists")).and_then(serde_json::Value::as_bool) == Some(true) {
                        Outcome::Available
                    } else {
                        Outcome::Unavailable
                    });
                    self.operation = None;
                }
                Some(status) if matches!(status.phase, Phase::Failed | Phase::Cancelled) => {
                    self.outcome =
                        Some(Outcome::Failed(format!("{}\n{}", tr("cloud.repository_check_failed"), status.message.as_deref().unwrap_or_default())));
                    self.operation = None;
                }
                _ if now - self.started >= 15.0 => {
                    self.cancel(app);
                    self.outcome = Some(Outcome::Failed(tr("cloud.repository_check_failed").into()));
                }
                _ => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            }
        } else if self.outcome.is_none() {
            if now < self.next {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64((self.next - now).clamp(0.01, 0.4)));
                return;
            }
            let result = (|| -> Result<String, String> {
                let mut connection = solidcraft_cloud::setup::github_connection(&repository).map_err(|e| e.to_string())?;
                if let Some(credential) = wizard.credential() {
                    connection.credential = credential.into();
                }
                app.session
                    .set_cloud_repository_check_token(wizard.credential().is_none().then(|| wizard.token.clone()))
                    .map_err(|e| e.to_string())?;
                let result = app.run("cloud.test", serde_json::json!({"proposed":connection,"metadata_only":true}))?;
                result
                    .get("operation")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| tr("cloud.repository_check_failed").into())
            })();
            match result {
                Ok(operation) => {
                    self.operation = Some(operation);
                    self.started = now;
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(error) => self.outcome = Some(Outcome::Failed(format!("{}\n{error}", tr("cloud.repository_check_failed")))),
            }
        }
    }
}
