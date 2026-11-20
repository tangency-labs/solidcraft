//! A click submits one particular choice on one design. A stale attempt can only return
//! to the queue after the dialog has resolved the same choice against the current design.

use super::{EntryDialog, Kind};
use crate::SolveApp;
use serde_json::{Value, json};
use solidcraft_control::{ControlRequest, Ticket};
use std::sync::{Arc, mpsc};

pub(super) struct Choice {
    value: String,
    label: String,
    /// The entry opted in and had only document permissions when Run was clicked.
    retry: bool,
}

pub(super) struct Attempt {
    serial: u64,
    revision: u64,
    reply: mpsc::Receiver<Value>,
    pub(super) ticket: Arc<Ticket>,
}

pub(super) enum Running {
    Attempt { work: Attempt, choice: Option<Choice> },
    Refresh(Choice),
}

impl Running {
    pub(super) fn stop(&self) {
        if let Self::Attempt { work, .. } = self {
            work.ticket.stop();
        }
    }

    pub(super) fn ticket(&self) -> Option<&Arc<Ticket>> {
        match self {
            Self::Attempt { work, .. } => Some(&work.ticket),
            Self::Refresh(_) => None,
        }
    }

    pub(super) fn rechecking(&self, revision: u64) -> Option<&str> {
        match self {
            Self::Refresh(choice) => Some(&choice.label),
            Self::Attempt { work, choice: Some(choice) } if work.revision != revision && choice.retry => Some(&choice.label),
            _ => None,
        }
    }
}

impl EntryDialog {
    fn document_only(&self, app: &SolveApp) -> bool {
        let safe = |grant: Option<&solidcraft_engine::plugin::Grants>| {
            grant.is_none_or(|grant| grant.command_groups.is_empty() && grant.folders.is_empty() && !grant.input_folder)
        };
        !self.read
            && self.opens.is_none()
            && self.plugin.groups.is_empty()
            && !self.plugin.requests_folders
            && safe(app.preferences.plugin_grants.get(&self.plugin.id))
            && safe(app.session.plugin_state.grants.get(&self.plugin.id))
    }

    pub(super) fn submitted_choice(&self, app: &SolveApp, params: &Value) -> Option<Choice> {
        let (field, _) = self.options_field()?;
        let value = params[&field.key].as_str()?;
        let request = self.options_request(app, params.clone())?;
        let label = self.choices.selected_label(&request)?;
        Some(Choice {
            value: value.to_owned(),
            label: label.to_owned(),
            retry: matches!(field.kind, Kind::Options { retry: true, .. }) && self.document_only(app),
        })
    }

    pub(super) fn enqueue(&mut self, params: Value, choice: Option<Choice>, revision: u64, held: &mut std::collections::VecDeque<ControlRequest>) {
        let (req, reply) = ControlRequest::new("engine.execute", json!({"command":"plugin.run","params":self.request(params)}));
        let work = Attempt { serial: req.serial, revision, reply, ticket: req.ticket.clone() };
        self.running = Some(Running::Attempt { work, choice });
        self.error = None;
        self.result = None;
        held.push_back(req);
    }

    /// Called after syncing picks and choices. Fields and the picker stay locked from the
    /// original click; only verified targets may be rebound. No fallback choice can submit.
    pub(super) fn resume_if_ready(&mut self, app: &mut SolveApp) {
        let Some(Running::Refresh(choice)) = &self.running else { return };
        let error = if self.session != app.session.uid {
            Some(app.tr("ui.plugin_dialog_other_design").to_owned())
        } else if self.picks_cleared {
            Some(app.tr("ui.plugin_dialog_check_targets").to_owned())
        } else if !self.document_only(app) {
            Some(app.tr("ui.plugin_dialog_review_changed").to_owned())
        } else {
            match (self.base_params(app), self.choices.view()) {
                (Err(reason), _) => Some(reason),
                (_, super::choices::View::Failed(reason)) => Some(reason.to_owned()),
                (_, super::choices::View::Waiting | super::choices::View::Inactive) => return,
                (Ok(_), super::choices::View::Ready { .. }) => {
                    if self.choices.select_value(&choice.value) {
                        match self.ready(app) {
                            Ok(params) => {
                                let Some(Running::Refresh(choice)) = self.running.take() else { return };
                                self.enqueue(params, Some(choice), app.session.revision, &mut app.control_held);
                                return;
                            }
                            Err(reason) => Some(reason),
                        }
                    } else {
                        Some(solidcraft_ui_text::format("ui.plugin_dialog_choice_unavailable", &[("choice", &choice.label)]))
                    }
                }
            }
        };
        self.running = None;
        self.error = error;
    }

    pub(super) fn attempt_reply(&mut self) -> Option<Value> {
        let Some(Running::Attempt { work, .. }) = &self.running else { return None };
        let reply = match work.reply.try_recv() {
            Ok(reply) => reply,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => json!({"ok":false,"error":solidcraft_ui_text::tr("ui.plugin_no_result")}),
        };
        let attempt = self.running.take();
        if reply.get("dialog_choice_stale") == Some(&Value::Bool(true))
            && let Some(Running::Attempt { work, choice: Some(choice) }) = attempt
            && choice.retry
            && !work.ticket.is_cancelled()
        {
            self.running = Some(Running::Refresh(choice));
            return None;
        }
        Some(reply)
    }
}

/// Request cooperative cancellation without cancelling the person's Run. Wait for the old
/// worker to finish before retrying, so repeated edits never accumulate background builds.
pub(crate) fn superseded(app: &SolveApp, req: &ControlRequest) -> bool {
    app.plugin_menu.dialogs.iter().any(|dialog| {
        matches!(&dialog.running, Some(Running::Attempt { work, choice: Some(choice) })
            if work.serial == req.serial && choice.retry && dialog.session == app.session.uid && work.revision != app.session.revision)
    })
}

/// Only a dialog-created request has a matching attempt. The same guard protects dispatch
/// after queue waits and acceptance after the worker finishes. Closed dialogs cancel their ticket.
pub(crate) fn check_attempt(app: &SolveApp, req: &ControlRequest) -> Result<(), Value> {
    for dialog in &app.plugin_menu.dialogs {
        let Some(Running::Attempt { work, choice }) = &dialog.running else { continue };
        if work.serial != req.serial {
            continue;
        }
        if dialog.session != app.session.uid {
            return Err(json!({"ok":false, "error":app.tr("ui.plugin_dialog_other_design")}));
        }
        if let Some(choice) = choice {
            if choice.retry && !dialog.document_only(app) {
                return Err(json!({"ok":false, "error":app.tr("ui.plugin_dialog_review_changed")}));
            }
            if work.revision != app.session.revision {
                return Err(json!({"ok":false, "dialog_choice_stale":true, "error":app.tr("ui.plugin_dialog_review_changed")}));
            }
        }
        break;
    }
    Ok(())
}
