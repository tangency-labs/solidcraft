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
    /// The installed, enabled importers: the files `doc.open` and `file.import` open with them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub importers: Vec<ImporterInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An installed importer entry.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImporterInfo {
    /// `plugin_id.entry`: what `format` names.
    pub id: String,
    /// The plugin's name.
    pub name: String,
    /// The file extensions it claims, lowercase, without the dot.
    pub extensions: Vec<String>,
    /// Its options, as a JSON schema.
    pub input_schema: Value,
}

/// Open a file with an installed importer plugin, as a new, unsaved design. Never opens a dialog.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportParams {
    /// The file to import; subject to the caller's file scope.
    pub path: crate::schema::file::OpenPath,
    /// The importer, as `plugin_id.entry` (see `plugin.list`'s `importers`). Default: the one that
    /// claims the file's extension.
    #[serde(default)]
    pub format: Option<String>,
    /// The importer's declared options. Default: its defaults.
    #[serde(default = "empty_params")]
    pub options: Value,
}

/// What an importer made: the new design, and what it could not carry over.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportedDesign {
    /// The file imported.
    pub path: String,
    /// The importer that read it, as `plugin_id.entry`.
    pub format: String,
    /// That importer plugin's name.
    pub plugin: String,
    /// The new design's name.
    pub name: String,
    /// How many features the design has.
    pub features: usize,
    /// How many of them failed to evaluate.
    pub errors: usize,
    /// How many bodies it has.
    pub bodies: usize,
    /// What could not be carried over: a summary and one item per loss.
    pub loss_report: crate::plugin::LossReport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Install a plugin from a `.zip`: `plugin.json` and its component at the top or in one folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallParams {
    /// The `.zip` to install; subject to the caller's file scope.
    pub path: String,
    /// Exact manifest-and-component hash from a reviewed dry run; a mismatch refuses installation.
    #[serde(default)]
    pub approval: Option<String>,
    /// Exact `.zip` hash (`archive`) from a reviewed dry run; any changed file in the archive refuses installation.
    #[serde(default)]
    pub archive: Option<String>,
    /// Replace an installed plugin with the same id. Without it, that is refused.
    #[serde(default)]
    pub replace: bool,
    /// Check the `.zip` and describe the plugin; write nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Permissions approved by the host user. Omission grants nothing, even on replacement.
    #[serde(default)]
    pub approve: Option<InstallApproval>,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct InstallResult {
    /// SHA-256 of the exact manifest and component. Send it as `approval` after reviewing this reply.
    pub approval: String,
    /// SHA-256 of the whole `.zip`. Send it as `archive` after reviewing this reply.
    pub archive: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Host capabilities it needs, as `name@major.minor`.
    pub capabilities: Vec<String>,
    /// What it asks for: command groups, named folders, the clock.
    pub permissions: craft_plugin::Permissions,
    /// The fuel, memory and time it asks for. Memory above `host_limits` is approved at install
    /// (`approve.memory_mib`); fuel is not, and must fit `host_limits`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<craft_plugin::Limits>,
    /// The app's ceilings for plugins, before any approval.
    pub host_limits: craft_plugin::HostLimits,
    /// App commands it calls.
    pub commands: Vec<String>,
    /// Its entries, as namespaced commands (`id.entry`).
    pub entries: Vec<String>,
    /// Entry declarations, including where the host will show them.
    pub entry_points: Vec<craft_plugin::Entry>,
    /// For an importer: the file extensions it opens, lowercase, without the dot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opens: Vec<String>,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveParams {
    pub id: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct RemoveResult {
    pub id: String,
    pub removed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A subset of the manifest's requested permissions; no unrequested access is accepted.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallApproval {
    #[serde(default)]
    pub command_groups: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub folders: Vec<ApprovedFolder>,
    /// Memory in MiB to allow above the app's ceiling for plugins: exactly the manifest's
    /// `runtime.limits.memory_mib`. Omitted: the plugin runs under the app's ceiling.
    #[serde(default)]
    pub memory_mib: Option<usize>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovedFolder {
    pub name: String,
    /// An existing absolute folder selected by the host user.
    pub path: std::path::PathBuf,
    pub read: bool,
    pub write: bool,
}
impl From<InstallApproval> for crate::plugin::Grants {
    fn from(approve: InstallApproval) -> Self {
        Self {
            package_hash: None,
            command_groups: approve.command_groups,
            folders: approve
                .folders
                .into_iter()
                .map(|f| craft_plugin::FileGrant { name: f.name, folder: f.path, read: f.read, write: f.write })
                .collect(),
            memory_mib: approve.memory_mib,
        }
    }
}
