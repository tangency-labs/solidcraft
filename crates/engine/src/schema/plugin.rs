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
    /// Explicit panel action notification, separate from the entry's ordinary arguments.
    #[serde(default)]
    pub ui_event: Option<crate::plugin::ui::UiEvent>,
    /// Refuse an edit unless this is still the design state the caller computed against.
    #[serde(default)]
    pub expected_design: Option<craft_plugin::DesignToken>,
    /// Called by name (`<plugin>.<entry>`, as a tool): a missing grant or a read entry's refusal
    /// comes back naming the plugin, the grant and how to approve it, or the refused import.
    #[serde(default)]
    pub named: bool,
    /// Populated forms require explicit acknowledgement of this document revision.
    #[serde(default)]
    pub form_guard: Option<FormGuard>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditParams {
    pub plugin: String,
    pub entry: String,
    #[serde(default = "empty_params")]
    pub params: Value,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormGuard {
    pub document_id: String,
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FormRequest {
    pub plugin: String,
    pub entry: String,
    pub params: Value,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormFieldError {
    InvalidValue { reason: String },
    MissingPick,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct FormValidation {
    pub revision: u64,
    pub changed: bool,
    pub errors: std::collections::BTreeMap<String, FormFieldError>,
}
fn empty_params() -> Value {
    Value::Object(Default::default())
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct RunResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forms: Vec<FormRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_validation: Option<FormValidation>,
    pub result: Value,
    /// Sketches closed by accepted guest commands, for the caller's authoring-state cleanup.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch_exits: Vec<Option<u64>>,
    /// The last view command the plugin called. The window moves the camera from this after
    /// the run is accepted. Omitted when the plugin called none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<PluginView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A view command a plugin called, for the window to apply after the run lands.
/// The last one in call order is the only one kept.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginView {
    /// `view.fit`: frame the model, keeping the view direction.
    Fit,
    /// `view.home`: frame the model from the home direction.
    Home,
    /// A command stored this camera on its session (`view.apply` and the others that do).
    /// `null` when the command cleared it.
    Camera { camera: Value },
    /// `view.navigate`: the window applies this reply the way the navigator does.
    /// The command does not store a camera on the session.
    Navigate { reply: Value },
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
    /// The installed, enabled exporters: the files `file.export` writes with them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exporters: Vec<ImporterInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// List the installed, enabled plugins' entries as tools for agents (`plugin.tools`).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolsParams {
    /// Only this tool (`<plugin>.<entry>`), with its `approval`. Without it every tool is
    /// listed, without approvals, and no component is read.
    #[serde(default)]
    pub name: Option<String>,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolsResult {
    /// One tool per entry of every installed, enabled plugin, sorted by name.
    pub tools: Vec<PluginTool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
/// What a plugin tool does when it is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginToolKind {
    /// A command entry: called as the `<plugin>.<entry>` command with the entry's inputs, one
    /// undo step.
    Command,
    /// An exporter entry: writes the open design through `file.export`.
    Exporter,
    /// An importer entry: opens a file it claims as a new design through `file.import`.
    Importer,
}
/// One installed plugin entry as a tool. Every text field except `name`, `plugin` and `entry`
/// comes from the plugin's own manifest: plugin text, not SolidCraft's.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PluginTool {
    /// `<plugin>.<entry>`: the tool's name and, for a command entry, the command it runs.
    pub name: String,
    pub kind: PluginToolKind,
    /// The plugin's id.
    pub plugin: String,
    pub entry: String,
    /// The plugin's name, version and description, from its manifest.
    pub plugin_name: String,
    pub plugin_version: String,
    pub description: String,
    pub effect: crate::plugin::Effect,
    /// The extensions a format entry opens or writes, lowercase, without the dot; empty for a command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    /// The tool's arguments: a command entry's declared inputs, strict (no undeclared field); a
    /// format entry's `{path, options}`, `options` its declared options.
    pub input_schema: Value,
    /// A command entry's package hash, which its `<plugin>.<entry>` call passes as `approval`;
    /// given when the tool is asked for by `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
}

/// An installed import or export slot.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImporterInfo {
    /// `plugin_id.entry`: what `format` names.
    pub id: String,
    /// The plugin's name.
    pub name: String,
    /// The slot's own label, when it has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// The file extensions it claims, lowercase, without the dot.
    pub extensions: Vec<String>,
    /// Its options, as a JSON schema.
    pub input_schema: Value,
}

/// What an exporter plugin wrote: `file.export` with a format an exporter plugin writes.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportedFile {
    /// The file written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The exporter that wrote it, as `plugin_id.entry`.
    pub format: String,
    /// That exporter plugin's name.
    pub plugin: String,
    /// The slot asks for the design's plugin data, which no export passes yet: a note saying so.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
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
    /// Called as the importer's tool: a missing grant comes back naming the plugin, the grant and
    /// how to approve it.
    #[serde(default)]
    pub named: bool,
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
    /// Host-drawn panels declared by the plugin, shown during permission review.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<crate::plugin::ui::PanelSpec>,
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
    /// The fuel, memory and time it asks for. Fuel and memory above `host_limits` can be approved
    /// at install (`approve.fuel`, up to `MAX_APPROVED_FUEL`, and `approve.memory_mib`).
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
    /// Read-only entries automatically called after design changes, covered by package approval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<craft_plugin::Listener>,
    /// For an importer: the file extensions it opens, lowercase, without the dot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opens: Vec<String>,
    /// For an exporter: the file extensions it writes, lowercase, without the dot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantsParams {}
/// Whether a stored approval still applies to the installed plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantStatus {
    /// It was approved for the installed manifest and component.
    Current,
    /// The installed plugin changed since: it runs only after it is installed and approved again.
    Changed,
    /// No plugin of this id is installed.
    NotInstalled,
    /// The installed plugin could not be read (the caller's file scope, a damaged package).
    Unreadable,
}
/// What the user approved for one plugin.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StoredGrant {
    pub id: String,
    pub status: GrantStatus,
    /// The exact manifest and component approved (`approval` of `plugin.install`); none for a
    /// record from before approvals were bound to a package, which needs approval again.
    pub package_hash: Option<String>,
    pub command_groups: std::collections::BTreeSet<String>,
    pub folders: Vec<crate::plugin::FileGrant>,
    /// Fuel approved above Max plugin fuel for this exact package, up to `MAX_APPROVED_FUEL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
    /// Memory in MiB approved above the app's ceiling for plugins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<usize>,
    /// Whether an importer may read the files beside the one it opens.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub input_folder: bool,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct GrantsResult {
    pub grants: Vec<StoredGrant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevokeParams {
    pub id: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct RevokeResult {
    pub id: String,
    /// Whether anything was approved for it.
    pub revoked: bool,
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
    /// Fuel to allow above Max plugin fuel: exactly the manifest's `runtime.limits.fuel`, at
    /// most 10,000,000,000,000. Omitted: the plugin runs under the app's default limit.
    #[serde(default)]
    pub fuel: Option<u64>,
    /// Memory in MiB to allow above the app's ceiling for plugins: exactly the manifest's
    /// `runtime.limits.memory_mib`. Omitted: the plugin runs under the app's ceiling.
    #[serde(default)]
    pub memory_mib: Option<usize>,
    /// Let an importer read the other files in the folder of the file it opens; only when its
    /// manifest asks for `input_folder`.
    #[serde(default)]
    pub input_folder: bool,
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
            fuel: approve.fuel,
            memory_mib: approve.memory_mib,
            input_folder: approve.input_folder,
        }
    }
}

/// Check a plugin package without running it: what Install and a call would check.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckParams {
    /// A package folder (`plugin.json` and its component; its `tests` folder is left out) or a
    /// plugin `.zip`; subject to the caller's file scope.
    pub path: String,
}

/// How bad a problem is: an error stops the plugin installing or running; a warning does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One thing wrong with a package, and how to fix it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CheckProblem {
    pub severity: Severity,
    /// The manifest field (`runtime.limits.fuel`), `plugin.json`, `package` or `component`.
    pub field: String,
    /// What is wrong, as the app says it when it refuses the package.
    pub problem: String,
    /// What to change.
    pub fix: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckResult {
    /// Whether the package has no errors: it installs, and its component links to this app.
    pub ok: bool,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `command`, `importer`, or `metadata` for a manifest with no runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// SHA-256 of the exact manifest and component, as `plugin.inspect` and `plugin.install` report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<String>,
    /// The entries it declares.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<craft_plugin::Entry>,
    /// Every problem found, errors first.
    pub problems: Vec<CheckProblem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
