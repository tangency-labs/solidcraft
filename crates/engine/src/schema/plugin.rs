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
    #[serde(default = "empty_params")]
    pub params: Value,
}
fn empty_params() -> Value {
    Value::Object(Default::default())
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunResult {
    pub result: Value,
    /// Sketches closed by accepted guest commands, for the caller's authoring-state cleanup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch_exits: Vec<Option<u64>>,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListedPlugin {
    pub manifest: crate::plugin::Manifest,
    pub commands: Vec<String>,
    pub disabled: bool,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResult {
    pub plugins: Vec<ListedPlugin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Install a plugin from a `.zip`: `plugin.json` and its component at the top or in one folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallParams {
    /// The `.zip` to install; subject to the caller's file scope.
    pub path: String,
    /// Replace an installed plugin with the same id. Without it, that is refused.
    #[serde(default)]
    pub replace: bool,
    /// Check the `.zip` and describe the plugin; write nothing.
    #[serde(default)]
    pub dry_run: bool,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct InstallResult {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Host capabilities it needs, as `name@major.minor`.
    pub capabilities: Vec<String>,
    /// What it asks for: command groups, named folders, the clock.
    pub permissions: craft_plugin::Permissions,
    /// App commands it calls.
    pub commands: Vec<String>,
    /// Its entries, as namespaced commands (`id.entry`).
    pub entries: Vec<String>,
    /// The installed plugin of the same id, `name version`, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    /// Whether it was written (false for a dry run).
    pub installed: bool,
    /// Its folder.
    pub folder: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
