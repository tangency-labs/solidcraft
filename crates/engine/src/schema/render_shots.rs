//! Contract and result type for the saved render shot commands in `cmd/render/shots.rs`
//! (#11450). The handlers read this same parameter type, so the contract is exactly what they
//! take; like them, it refuses unknown keys.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Duplicate, rename or remove a saved render shot (a named render setup) in one undo step. The
/// resources it uses, the camera in the view, captured render results and the design's
/// engineering data stay as they are.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShotParams {
    /// The saved shot's exact name.
    pub name: String,
    /// The name for the copy or the renamed shot: 1 to 128 bytes, without spaces at either end or
    /// control characters, and not in use. A name in use is refused, never replaced. Duplicate
    /// and rename need it; remove refuses it. A design holds at most 32 shots.
    #[serde(default)]
    pub new_name: Option<String>,
    /// The design's revision when the caller last read it, as `document.inspect` gives it. If
    /// the design changed since, the call is refused: read it again and retry.
    pub expected_revision: u64,
}

/// What `render.shot.duplicate`, `render.shot.rename` and `render.shot.remove` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ShotChange {
    pub operation: ShotOperation,
    /// The shot's name before the change.
    pub source: String,
    /// The copy's or the renamed shot's name; `null` after a removal.
    pub name: Option<String>,
    /// Always true: the resources the shot uses are kept.
    pub resources_preserved: bool,
    /// Always true: the camera in the view doesn't move.
    pub camera_navigation_unchanged: bool,
    /// Always true: the design's engineering data doesn't change.
    pub engineering_properties_unchanged: bool,
    /// Always true: captured render results are kept.
    pub captured_jobs_unchanged: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What was done to the shot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShotOperation {
    Duplicate,
    Rename,
    Remove,
}

#[cfg(test)]
#[path = "render_shots_tests.rs"]
mod tests;
