//! Input and output schemas for explicit plugin inspection and execution.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    /// Package folder containing plugin.json and its component; subject to the caller's file scope.
    pub path: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct InspectResult {
    pub manifest: crate::plugin::Manifest,
    /// SHA-256 of the exact manifest bytes and component. Review the manifest before using it.
    pub approval: String,
    pub host_limits: crate::plugin::HostLimits,
    /// Whether the requested resources fit the current user ceilings; not an execution grant.
    pub within_host_limits: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunParams {
    pub path: String,
    /// Exact approval hash returned by plugin.inspect, after permission review.
    pub approval: String,
    pub entry: String,
    #[serde(default)]
    pub params: Value,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunResult {
    pub result: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Read a body's evaluated triangle mesh in its owning component's coordinates (millimetres).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshParams {
    pub body: String,
    /// Chord tolerance in mm. Default: 0.05; must be finite and positive.
    #[serde(default = "mesh_tolerance")]
    pub tolerance: f64,
}
fn mesh_tolerance() -> f64 {
    0.05
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct MeshResult {
    pub body: String,
    pub component: u64,
    pub frame: String,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
