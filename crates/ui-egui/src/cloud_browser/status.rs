//! Short, accessible row states; only observed stale copies are blocked from opening.
use super::*;
use crate::ui_kit::{FileStatus, FileStatusTone};

pub(super) fn presentation(status: Option<&SaveStatus>, receiving: bool) -> FileStatus<'static> {
    let (label, tone) = match status {
        Some(s) if s.outdated && receiving => ("cloud.status_pulling", FileStatusTone::Pending),
        Some(s) if s.outdated => ("cloud.status_outdated", FileStatusTone::Attention),
        Some(s) if s.conflict || s.local_revisions.len() > 1 => ("cloud.status_conflict", FileStatusTone::Attention),
        Some(s) => match s.phase {
            Phase::Synced => ("cloud.synced", FileStatusTone::Current),
            Phase::SavedLocally | Phase::Syncing => ("cloud.status_pushing", FileStatusTone::Pending),
            Phase::Working => ("cloud.status_saving", FileStatusTone::Pending),
            Phase::Failed | Phase::Cancelled => ("cloud.status_error", FileStatusTone::Attention),
            Phase::Ready => ("cloud.status_available", FileStatusTone::Available),
        },
        None => ("cloud.status_available", FileStatusTone::Available),
    };
    let pulse = status.is_some_and(|s| s.outdated || matches!(s.phase, Phase::SavedLocally | Phase::Syncing));
    FileStatus { label: tr(label), tone, pulse }
}
