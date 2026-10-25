//! Read-only document-tab presentation; the durable catalog and captures own sync state.
use super::*;
use crate::ui_kit::{FileStatus, FileStatusTone};

pub(crate) struct Indicator {
    pub status: FileStatus<'static>,
    pub tooltip: String,
}
pub(super) type PendingKey = (String, String, String, Option<String>);
pub(super) fn key(destination: &Destination, revision: Option<&str>) -> PendingKey {
    (destination.connection.clone(), destination.project.clone(), destination.file.clone(), revision.map(str::to_owned))
}

pub(crate) fn request_timeout(app: &SolveApp, ctx: &egui::Context) {
    for since in app.cloud.browser.pending_since.values() {
        if let Some(remaining) = std::time::Duration::from_secs(30).checked_sub(since.elapsed()) {
            ctx.request_repaint_after(remaining);
        }
    }
}

pub(crate) fn indicator(app: &SolveApp, session: &Session) -> Option<Indicator> {
    let binding = session.cloud_binding();
    let status = if session.cloud_save_pending() {
        FileStatus { label: tr("cloud.status_saving"), tone: FileStatusTone::Unsaved, pulse: false }
    } else {
        let binding = binding?;
        let browser = &app.cloud.browser;
        let latest = browser
            .statuses
            .iter()
            .chain(browser.catalogs.get(&binding.destination.connection).into_iter().flat_map(|c| &c.stored))
            .filter(|s| s.destination == binding.destination)
            .max_by_key(|s| s.operation);
        let mut shown = status::presentation(latest, browser.receiving.contains(&binding.destination.connection));
        let failed = browser
            .statuses
            .iter()
            .filter(|s| s.destination == binding.destination)
            .max_by_key(|s| s.operation)
            .is_some_and(|s| s.failed && (s.revision == binding.revision || session.is_dirty()));
        if failed || latest.is_some_and(|s| s.failed) {
            shown = FileStatus { label: tr("cloud.status_error"), tone: FileStatusTone::Attention, pulse: false };
        } else if latest.is_some_and(|s| !s.conflict && s.local_revisions.len() <= 1 && s.revision != binding.revision) {
            shown = FileStatus { label: tr("cloud.status_outdated"), tone: FileStatusTone::Attention, pulse: true };
        } else if session.is_dirty() {
            shown = FileStatus { label: crate::i18n::tr_label("Not saved yet"), tone: FileStatusTone::Unsaved, pulse: false };
        } else if latest.is_some_and(|s| matches!(s.phase, Phase::SavedLocally | Phase::Syncing)) {
            let slow = browser
                .pending_since
                .get(&key(&binding.destination, binding.revision.as_deref()))
                .is_some_and(|since| since.elapsed() >= std::time::Duration::from_secs(30));
            shown.tone = if slow { FileStatusTone::Pending } else { FileStatusTone::Unsaved };
            shown.pulse = true;
        }
        if matches!(shown.tone, FileStatusTone::Current | FileStatusTone::Available) {
            return None;
        }
        shown
    };
    let mut tooltip = if let Some(binding) = binding {
        let destination = &binding.destination;
        let repository = session
            .cloud_settings()
            .connection(&destination.connection)
            .map(|c| c.repository_label())
            .unwrap_or_else(|| destination.connection.clone());
        format!("{repository} / {} / {}\n{}", destination.project, destination.file, status.label)
    } else {
        status.label.to_owned()
    };
    if session.is_dirty() && status.label != crate::i18n::tr_label("Not saved yet") {
        tooltip.push('\n');
        tooltip.push_str(crate::i18n::tr_label("Not saved yet"));
    }
    Some(Indicator { status, tooltip })
}
