//! What the person did, by command id, for the app's anonymous usage statistics
//! (docs/privacy.md): once per action a human or an agent takes, never for a replay.
//!
//! - **Operations**: a command that changed the design, the one that made an undo step (an edit
//!   of an existing feature included). Rebuilding on open, a parameter change re-running later
//!   features, undo and redo, and recomputes make no undo step of their own and are not counted.
//! - **Tools**: a tool or dialog started from the UI, even when it is cancelled (the host counts
//!   these, [`note`] with [`Bucket::Tools`]).
//! - Each count is split by [`Source`]: a person in the app, an agent (MCP or the control
//!   channel), or a script (a plugin).
//! - Background commands are never counted ([`counted`]): selection, visibility, views and the
//!   camera. Plugin commands count as one id, `plugin`. Only registered command ids are kept:
//!   never parameters, names, paths or geometry.
//!
//! A session counts only when its host turns [`crate::Session::count_usage`] on (the app's own
//! design; never previews, scratch copies, the command line or tests). The counts live in this
//! process until the host takes them ([`take`]). The app's tests
//! (`crates/ui-egui/src/usage_stats_tests.rs`) cover what is counted.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// What kind of action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bucket {
    Operations,
    Tools,
}

/// Who took the action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// A person in the app: the ribbon, a shortcut, a dialog, an edit.
    Ui,
    /// MCP or the control channel, the app's own assistant included.
    Agent,
    /// A plugin (the command line counts nothing).
    Script,
}

impl Source {
    /// Its name in a report.
    pub fn name(self) -> &'static str {
        match self {
            Source::Ui => "ui",
            Source::Agent => "agent",
            Source::Script => "script",
        }
    }
}

/// One count: the action's kind, who took it and the command.
pub type Key = (Bucket, Source, &'static str);

/// Counts since the host last took them.
static COUNTS: Mutex<BTreeMap<Key, u32>> = Mutex::new(BTreeMap::new());

thread_local! {
    /// Who the commands on this thread come from now ([`from`]).
    static SOURCE: Cell<Source> = const { Cell::new(Source::Ui) };
}

/// Until the guard drops, actions on this thread come from `source`.
pub fn from(source: Source) -> SourceGuard {
    SourceGuard(SOURCE.with(|s| s.replace(source)))
}

/// Puts the earlier source back when dropped.
pub struct SourceGuard(Source);

impl Drop for SourceGuard {
    fn drop(&mut self) {
        SOURCE.with(|s| s.set(self.0));
    }
}

/// Whether `id` is counted at all: not selection, visibility, views or the camera, which run in
/// the background of whatever the person does.
pub fn counted(id: &str) -> bool {
    const BACKGROUND: [&str; 6] = ["select.", "view.", "camera.", "hover.", "navigation.", "ui."];
    !BACKGROUND.iter().any(|prefix| id.starts_with(prefix)) && id != "browser.visibility"
}

/// One more `bucket` action on `id` (a registered command's id) from the current source. A plugin's
/// command is `plugin`, from a script.
pub fn note(bucket: Bucket, id: &'static str) {
    if !counted(id) {
        return;
    }
    let (source, id) = if id == "plugin.run" { (Source::Script, "plugin") } else { (SOURCE.with(Cell::get), id) };
    let mut counts = COUNTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let count = counts.entry((bucket, source, id)).or_default();
    *count = count.saturating_add(1);
}

/// The counts since the last call, which start again from nothing.
pub fn take() -> BTreeMap<Key, u32> {
    std::mem::take(&mut *COUNTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}
