//! SolidCraft's half of Open and Save against cloud storage.
//!
//! No real backend is wired into this build. The generic `CloudBackend` every Craft
//! app will share is landing upstream in `craft-ui`/`craft-cloud` (storytold's
//! craft-libs#160), along with the pages and the Share and Presence UI it carries.
//! SolidCraft's CollabCraft wiring is paused until then. It depended on a personal
//! fork of those crates. That dependency is gone now.
//!
//! This module keeps the shape of that contract alive on its own. It is trimmed to
//! what the test below needs to prove the pattern works. Every cloud menu item, the
//! PlacesPanel and the Share split button stay hidden here: [`available`] always
//! answers false.

use crate::SolveApp;

/// A unit of work a real `CloudBackend` would run off the UI thread.
///
/// Trimmed to one variant, enough to exercise the pattern in `tests`. The real `Job`
/// landing with craft-libs#160 also covers read, write, create, sign in and out, link
/// and conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// List the entries at a path.
    List(String),
}

/// The answer a submitted [`Job`] carries back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The entries found at the path a [`Job::List`] named.
    Listed(String, Vec<String>),
}

/// The contract a real cloud backend implements.
///
/// Mirrors the shape of craft-ui's own `CloudBackend`: submit a job, get its answer.
/// No type implements this outside `tests`. It exists so the pattern craft-libs#160
/// will formalise upstream can already be exercised here.
pub trait CloudBackend: Send + Sync {
    /// Runs `job` and returns its answer.
    ///
    /// A real backend answers through a channel, so a call never blocks the frame
    /// that made it. This one answers directly, because nothing here crosses a
    /// thread yet.
    fn submit(&self, job: Job) -> Event;
}

/// Cloud Open/Save state.
///
/// Always empty. Nothing in this build connects a [`CloudBackend`], so the account
/// strip, the PlacesPanel and the cloud menu items never draw ([`available`]).
#[derive(Default)]
pub struct CloudFiles;

impl CloudFiles {
    pub fn new() -> Self {
        Self
    }
}

/// A Save a Copy format the Share split button could offer.
///
/// No format exists in this build; [`copies`] always answers an empty list.
pub struct CopyFormat;

/// Whether cloud Open, Save and Share are available.
///
/// Always false until a [`CloudBackend`] is wired in (craft-libs#160). Every call
/// site that draws cloud UI is already gated on this, so none of it appears.
pub fn available(_app: &SolveApp) -> bool {
    false
}

/// Whether `file.save` has a cloud destination to save back to.
///
/// Always false, for the reason [`available`] is.
pub fn has_target(_app: &SolveApp) -> bool {
    false
}

/// Whether the signed-in account can share.
///
/// Always false, for the reason [`available`] is.
pub fn shares(_app: &SolveApp) -> bool {
    false
}

/// The account strip in the title bar.
///
/// Draws nothing. The title bar only asks once [`available`] answers true.
pub fn who(_app: &SolveApp, _ui: &mut egui::Ui) {}

/// The Share split button.
///
/// Never reached: [`shares`] always answers false.
pub fn share_button(_app: &mut SolveApp, _ui: &mut egui::Ui, _save_copy_as: &[CopyFormat]) -> bool {
    false
}

/// The Save a Copy formats the Share split button would offer.
///
/// Empty, and never read: [`shares`] always answers false.
pub fn copies() -> Vec<CopyFormat> {
    Vec::new()
}

/// The cloud file list on the start page (the PlacesPanel).
///
/// Draws nothing. There is no signed-in account to list files for.
pub fn open_page(_app: &mut SolveApp, _ui: &mut egui::Ui) {}

/// Drains answers a [`CloudBackend`] posted.
///
/// Nothing to drain: none is connected.
pub fn pump(_app: &mut SolveApp, _ctx: &egui::Context) {}

/// The save conflict modal.
///
/// Never shown: a save never reaches a cloud destination to conflict with.
pub fn conflict_modal(_app: &mut SolveApp, _ctx: &egui::Context) {}

/// The floating Save to Cloud window.
///
/// Never shown: [`available`] always answers false, so nothing sets
/// `cloud_save_window`.
pub fn save_window(_app: &mut SolveApp, _ctx: &egui::Context) {}

/// The cloud commands over the control channel.
///
/// None are handled in this build. Every `cloud.*` id reaches the control channel as
/// unrecognised.
pub fn command(_app: &mut SolveApp, _id: &str, _params: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers a list job from memory, proving the trait's shape carries a job to an
    /// answer and back.
    struct Mock;

    impl CloudBackend for Mock {
        fn submit(&self, job: Job) -> Event {
            match job {
                Job::List(path) => Event::Listed(path, vec!["part.solidcraft".to_owned()]),
            }
        }
    }

    #[test]
    fn a_backend_answers_the_job_it_was_given() {
        let Event::Listed(path, entries) = Mock.submit(Job::List("/designs".to_owned()));
        assert_eq!(path, "/designs");
        assert_eq!(entries, vec!["part.solidcraft".to_owned()]);
    }
}
