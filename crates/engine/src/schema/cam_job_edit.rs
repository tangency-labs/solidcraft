//! Contracts and replies of the open CAM job's commands (`cmd/cam_job_edit.rs`): `cam.job.new`,
//! `open`, `save`, `reload`, `close`, `get`, `edit`, `undo` and `redo`. docs/cam-jobs.md, "The
//! open job", describes them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Start a new job for the design, kept open in the session; nothing is written until it is
/// saved (`cam.job.save`).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobNewParams {
    /// Where the job will be saved (a `.json` file). Default: `<design name>.cam.json` beside
    /// the saved design, or when that is taken `<design name>.op2.cam.json`, `.op3`…; none for
    /// an unsaved design (then `cam.job.save {path}`).
    #[serde(default)]
    pub path: Option<String>,
    /// The program's name. Default: the design's.
    #[serde(default)]
    pub name: Option<String>,
    /// Drop an open job's unsaved changes. Default: false (refused when there are some).
    #[serde(default)]
    pub discard: Option<bool>,
}

/// Open a job file, kept open in the session for `cam.job` and the `cam.job.*` commands.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobOpenParams {
    /// The job file (`.json`).
    pub path: String,
    /// Drop an open job's unsaved changes. Default: false (refused when there are some).
    #[serde(default)]
    pub discard: Option<bool>,
}

/// Write the open job to its file, keys in the job's order and the file's own indent.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobSaveParams {
    /// Save it as this file instead (`.json`), which it then is. Default: its own file.
    #[serde(default)]
    pub path: Option<String>,
    /// Write even when the file was changed outside SolidCraft since it was read, or when
    /// another file is at `path`. Default: false (refused).
    #[serde(default)]
    pub overwrite: Option<bool>,
}

/// Read the open job's file again (an undoable step).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobReloadParams {
    /// Drop unsaved changes. Default: false (refused when there are some).
    #[serde(default)]
    pub discard: Option<bool>,
}

/// Close the open job.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobCloseParams {
    /// Drop unsaved changes. Default: false (refused when there are some).
    #[serde(default)]
    pub discard: Option<bool>,
}

/// The open job's state.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobGetParams {
    /// Include the job's JSON. Default: false.
    #[serde(default)]
    pub job: Option<bool>,
}

/// Set or remove keys of the open job (one undoable step). Every edit is read with the job's
/// schema before it is kept: one that does not parse is refused and changes nothing.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobEditParams {
    /// Keys to set: job keys, or dotted paths into objects (`tools.0.feed`).
    #[serde(default)]
    pub set: Option<Map<String, Value>>,
    /// Keys (or dotted paths) to remove, so they take their defaults again.
    #[serde(default)]
    pub unset: Option<Vec<String>>,
}

/// Undo or redo a step of the open job.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CamJobHistoryParams {}

/// An operation in a [`JobSummary`].
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct OpSummary {
    pub index: usize,
    pub name: String,
    /// Its `type`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Its tool's number, when it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<u32>,
    pub enabled: bool,
}

/// The open job in brief.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct JobSummary {
    /// Its file (none: not saved anywhere yet).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub name: String,
    /// It differs from its file.
    pub dirty: bool,
    /// Counts up with every change.
    pub revision: u64,
    /// The machine (`3axis`, `4axis_a`, a machine's name, or none).
    pub machine: String,
    /// The post (a built-in's name, a file, or none).
    pub post: String,
    /// The tools, or the library they come from.
    pub tools: String,
    /// Work zero, design coordinates.
    pub origin: [f64; 3],
    /// The part's bodies (empty: every visible body).
    pub bodies: Vec<String>,
    pub operations: Vec<OpSummary>,
}

/// What `cam.job.get` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamJobGetReply {
    /// Whether a job is open.
    pub open: bool,
    /// The file against what was last read or written: `none`, `changed` or `gone`.
    pub outside: String,
    /// The labels of the steps undo and redo would take, latest first.
    pub undo: Vec<String>,
    pub redo: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<JobSummary>,
    /// The job's JSON (with `job: true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<Value>,
}

/// What `cam.job.save` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamJobSaveReply {
    pub path: String,
    pub bytes: usize,
    /// Whether the file was written (false: it already held the job).
    pub written: bool,
}

/// What `cam.job.reload` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamJobReloadReply {
    pub summary: JobSummary,
    /// What changed, one line each.
    pub changes: Vec<String>,
}

/// What `cam.job.close` returns.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamJobCloseReply {
    pub closed: bool,
}

/// What `cam.job.undo` and `cam.job.redo` return.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct CamJobHistoryReply {
    /// The step's label (none: nothing to undo or redo).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub summary: JobSummary,
}
