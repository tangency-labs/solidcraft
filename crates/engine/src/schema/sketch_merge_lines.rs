//! Contract and result type for Merge Lines, `cmd/sketch_merge_lines.rs` (#20317).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config::NameList;
use super::sketch_ends::SketchTarget;
use super::sketch_shapes::Created;

/// Merge lines joined end to end on one straight line into one line.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MergeLinesParams {
    /// The sketch: its feature id or name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The lines, by id: at least two, joined end to end on one straight line. The first keeps
    /// its id and runs the whole length; the others go.
    #[serde(default)]
    pub curves: Option<NameList>,
}

/// What `sketch.merge_lines` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct MergeLinesResult {
    /// The id of the merged line: the first line given.
    pub line: String,
    /// The ids of the lines merged into it, now gone.
    pub removed: Vec<String>,
    /// The Frame Members built on the run whose line lists now name the merged line instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<String>,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// Everything the call added to the sketch: `{points, curves, constraints, dimensions}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Created>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
