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
//! - **Failures**: a command the person or an agent ran that failed, by the kind of error only
//!   ([`error_kind`]: its category, or for a kernel failure the call and the kernel's numbered
//!   kind). Never the message, the parameters or anything from the design.
//! - **Compute time**: how long each counted operation or failure took, in four buckets
//!   ([`compute_bucket`]).
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

/// Failures since the host last took them: who, which command, which kind of error.
static FAILURES: Mutex<BTreeMap<(Source, &'static str, String), u32>> = Mutex::new(BTreeMap::new());

/// Compute-time buckets since the host last took them: which command, which bucket.
static COMPUTE: Mutex<BTreeMap<(&'static str, &'static str), u32>> = Mutex::new(BTreeMap::new());

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

/// Who ran `id` and the id it counts as: a plugin's command is `plugin`, from a script.
fn who(id: &'static str) -> (Source, &'static str) {
    if id == "plugin.run" { (Source::Script, "plugin") } else { (SOURCE.with(Cell::get), id) }
}

fn bump<K: Ord>(map: &Mutex<BTreeMap<K, u32>>, key: K) {
    let mut map = map.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let count = map.entry(key).or_default();
    *count = count.saturating_add(1);
}

/// One more `bucket` action on `id` (a registered command's id) from the current source.
pub fn note(bucket: Bucket, id: &'static str) {
    if counted(id) {
        let (source, id) = who(id);
        bump(&COUNTS, (bucket, source, id));
    }
}

/// `id` failed with an error of `kind` ([`error_kind`]).
pub fn note_failure(id: &'static str, kind: String) {
    if counted(id) {
        let (source, id) = who(id);
        bump(&FAILURES, (source, id, kind));
    }
}

/// `id` took `ms` milliseconds.
pub fn note_compute(id: &'static str, ms: f64) {
    if counted(id) {
        bump(&COMPUTE, (who(id).1, compute_bucket(ms)));
    }
}

/// The bucket of a command's time: `0-100ms`, `100ms-1s`, `1s-10s` or `10s+`.
pub fn compute_bucket(ms: f64) -> &'static str {
    match ms {
        ms if ms < 100.0 => "0-100ms",
        ms if ms < 1000.0 => "100ms-1s",
        ms if ms < 10_000.0 => "1s-10s",
        _ => "10s+",
    }
}

/// The kind of a command's error, for counting: its category (`bad_params`, `refused`, ...), or
/// for a kernel failure `kernel.<call>.<kind>` from the kernel's stable code. None for a
/// cancellation, which is no failure. Never the message or anything from the design.
pub fn error_kind(error: &crate::EngineError) -> Option<String> {
    use crate::EngineError as E;
    let kind = match error {
        E::Cancelled(_) => return None,
        E::Feature(failure) => {
            let call: String =
                failure.code.call.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).take(40).collect();
            let call = if call.starts_with(|c: char| c.is_ascii_lowercase()) { call } else { format!("k{call}") };
            return Some(format!("kernel.{call}.{}", failure.code.kind));
        }
        E::UnknownCommand(_) => "unknown_command",
        E::Disabled(..) => "disabled",
        E::BadParams { .. } => "bad_params",
        E::Refused { .. } => "refused",
        E::Internal(..) => "internal",
        E::Plugin(_) | E::PluginFailed(_) => "plugin",
        E::Panel(_) => "panel",
        _ => "other",
    };
    Some(kind.to_string())
}

/// What was counted since the host last took it.
#[derive(Debug, Default)]
pub struct Taken {
    pub counts: BTreeMap<Key, u32>,
    pub failures: BTreeMap<(Source, &'static str, String), u32>,
    pub compute: BTreeMap<(&'static str, &'static str), u32>,
}

impl Taken {
    /// Nothing was counted.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty() && self.failures.is_empty() && self.compute.is_empty()
    }
}

/// Everything counted since the last call, which starts again from nothing.
pub fn take() -> Taken {
    fn drain<K: Ord>(map: &Mutex<BTreeMap<K, u32>>) -> BTreeMap<K, u32> {
        std::mem::take(&mut *map.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
    Taken { counts: drain(&COUNTS), failures: drain(&FAILURES), compute: drain(&COMPUTE) }
}
