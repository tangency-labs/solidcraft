//! Contracts and result types for sketch editing and completion in `cmd/sketch.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::{IdOrName, present};

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub sketch: IdOrName,
    /// When present, temporarily roll to this sketch and restore this position when the edit
    /// ends. Null means the end of the timeline; omission leaves the timeline in place.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<usize>")]
    pub restore_marker: Option<Option<usize>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExitParams {
    /// Restore the temporary editor marker (default true). False transfers it directly to
    /// another editor without rebuilding the intermediate timeline.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    pub restore_marker: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub sketch: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FinishResult {
    pub sketch: Option<u64>,
    pub profiles: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CancelResult {
    pub sketch: u64,
    pub removed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
