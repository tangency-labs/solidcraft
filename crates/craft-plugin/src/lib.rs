//! Shared plugin contracts. App commands are schemas and JSON, never app-specific types.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// The WIT package declaration is the sole source of the API identifier.
include!(concat!(env!("OUT_DIR"), "/api.rs"));
pub const WIT: &str = include_str!("../wit/plugin.wit");
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_JSON_BYTES: usize = 1024 * 1024;
/// An importer's component may be larger: a real converter measured 28 to 42 MB (RFC 0002).
pub const MAX_IMPORTER_COMPONENT_BYTES: usize = 64 * 1024 * 1024;
/// The most one `source.read` or `design.write` call moves.
pub const MAX_IMPORT_CHUNK_BYTES: usize = 1024 * 1024;
/// The most an importer reads in one call: the file being imported and the files beside it.
pub const MAX_IMPORT_INPUT_BYTES: u64 = 1024 * 1024 * 1024;
/// The largest design an importer may write.
pub const MAX_IMPORT_DESIGN_BYTES: usize = 256 * 1024 * 1024;
/// The largest file an exporter may return (RFC 0015 §4: 256 MiB output).
pub const MAX_EXPORT_FILE_BYTES: usize = 256 * 1024 * 1024;
/// Default host deadline ceilings; users may approve larger guest compute budgets.
pub const MAX_COMMAND_DEADLINE_MS: u64 = 30_000;
pub const MAX_IMPORTER_DEADLINE_MS: u64 = 600_000;
/// A long job (an entry with `"job": true`) runs for at most ten minutes. The limit is fixed: a
/// plugin that needs longer asks for it through its own limits, not an app-wide setting.
pub const JOB_DEADLINE_SECONDS: u64 = 600;
/// The most host calls a long job makes (its progress reports among them); an ordinary command
/// makes at most 256.
pub const MAX_JOB_CALLS: usize = 1 << 20;
/// The most extensions one import or export slot claims.
pub const MAX_EXTENSIONS: usize = 32;
/// Memory32 has at most 65536 pages of 64 KiB.
pub const MAX_MEMORY_MIB: usize = 4096;
/// Hard ceiling for fuel approved at install: ten trillion fuel units (10^13).
/// The user's Max plugin fuel remains the default for packages without an approval.
pub const MAX_APPROVED_FUEL: u64 = 10_000_000_000_000;

pub mod shader;
pub mod theme;

pub type Result<T> = std::result::Result<T, HostError>;

/// An open document session and its committed history state, kept outside authored document data.
/// Saving preserves a token; committed edits, undo and redo each produce a fresh revision.
/// Opening or importing a document creates a fresh identity, including reopening the same file.
/// Both strings are opaque and must only be compared for equality within their open session.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesignToken {
    #[serde(deserialize_with = "deserialize_design_token_part")]
    #[schemars(length(min = 1, max = 128))]
    pub document_id: String,
    #[serde(deserialize_with = "deserialize_design_token_part")]
    #[schemars(length(min = 1, max = 128))]
    pub revision: String,
}
fn deserialize_design_token_part<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    use serde::de::Error;

    let value = String::deserialize(deserializer)?;
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
        return Err(D::Error::custom("a design token field must contain 1 to 128 printable ASCII characters"));
    }
    Ok(value)
}

impl std::fmt::Display for DesignToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}@{}", self.document_id, self.revision)
    }
}

pub mod conditions;
pub mod dialog;
pub mod images;
mod input;
pub mod layout;
mod schema_order;
mod schema_refs;
pub mod swatches;
pub mod ui;
pub use input::FONT_KEY;
pub use input::check_input;
pub use input::check_input_field;
pub use input::font_field;
pub use input::prepare_input;
pub use input::strict_input_schema;

/// The host imports that change something outside the design. A `read` entry is refused every
/// one of them, whatever folders its plugin was granted, as it is refused edit commands.
pub const WRITING_IMPORTS: &[&str] = &["user-storage.remove", "user-storage.set", "write-file"];

/// The refusal of a writing host import to a `read` entry: it names the import.
pub fn read_entry_refusal(import: &str) -> HostError {
    HostError::PermissionDenied { permission: format!("{READ_ENTRY_IMPORT}{import}") }
}
/// How [`read_entry_refusal`]'s permission starts; the import's name follows it.
pub const READ_ENTRY_IMPORT: &str = "read entry may not use the host import ";

/// Per-plugin settings budgets: the entire compact JSON object, including keys and punctuation.
pub const MAX_USER_STORAGE_BYTES: usize = 64 * 1024;
pub const MAX_USER_STORAGE_KEYS: usize = 128;
pub const MAX_USER_STORAGE_KEY_BYTES: usize = 128;

pub fn check_user_storage_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > MAX_USER_STORAGE_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(HostError::InvalidInput { field: "user-storage.key".into(), reason: "requires 1..128 bytes without control characters".into() });
    }
    Ok(())
}

pub fn check_user_storage(data: &BTreeMap<String, Value>) -> Result<()> {
    if data.len() > MAX_USER_STORAGE_KEYS {
        return Err(HostError::LimitExceeded { limit: "user-storage keys (128)".into() });
    }
    for key in data.keys() {
        check_user_storage_key(key)?;
    }
    let bytes = serde_json::to_vec(data).map_err(|e| HostError::InvalidInput { field: "user-storage".into(), reason: e.to_string() })?;
    if bytes.len() > MAX_USER_STORAGE_BYTES {
        return Err(HostError::LimitExceeded { limit: "user-storage total bytes (65536)".into() });
    }
    Ok(())
}

/// Failures are actionable data at the app boundary, including failures before instantiation.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostError {
    /// An edit was computed against a different history state. Checked in its transaction.
    #[error("stale design revision: expected {expected}, current {actual}")]
    StaleRevision { expected: DesignToken, actual: DesignToken },
    #[error("permission denied: {permission}")]
    PermissionDenied { permission: String },
    #[error("command {command} failed: {reason}")]
    CommandFailed { command: String, reason: String },
    #[error("plugin limit exceeded: {limit}")]
    LimitExceeded { limit: String },
    #[error("invalid manifest field {field}: {reason}")]
    InvalidManifest { field: String, reason: String },
    #[error("invalid input {field}: {reason}")]
    InvalidInput { field: String, reason: String },
    /// Another run changed this plugin's user settings or is committing them. Retry the run
    /// from freshly loaded settings; the refused transaction published nothing.
    #[error("user settings for {plugin} changed or are being written; retry the plugin run")]
    UserStorageConflict { plugin: String },
    #[error("plugin trap: {reason}")]
    Trap { reason: String },
    #[error("plugin cancelled")]
    Cancelled,
    #[error("interactive_required: opening {entry} requires the app form; call the entry with params instead")]
    InteractiveRequired { entry: String },
    /// The plugin needs a capability, or a version of one, that this host does not serve.
    #[error("{reason}")]
    UnsupportedCapability { capability: String, reason: String },
    /// The input is valid, but this plugin does not handle it (WIT `plugin-error.unsupported`,
    /// from API 0.2): a file it can't convert, an option value it doesn't offer.
    #[error("not supported: {reason}")]
    Unsupported { reason: String },
    /// The plugin asks for more of a budget than the app lets plugins have, so it was not run.
    /// `limit` is the manifest field (`fuel`, `memory_mib`, `host_calls`, `deadline_ms`); `requested` and `allowed` are in its
    /// unit.
    #[error("{}", over_ceiling(limit, *requested, *allowed))]
    OverCeiling { limit: String, requested: u64, allowed: u64 },
    /// The plugin ran into one of its budgets and was stopped. `limit` is `fuel`, `memory_mib`,
    /// `host_calls`, `deadline_ms` or `stack_kib` (the manifest's own budgets) or `design_mib` (the most an
    /// importer may hand over); `allowed` is that budget, in its unit.
    #[error("{}", exhausted(limit, *allowed))]
    Exhausted { limit: String, allowed: u64 },
}

fn over_ceiling(limit: &str, requested: u64, allowed: u64) -> String {
    match limit {
        "memory_mib" => format!(
            "asks for {requested} MiB of memory, more than the {allowed} MiB plugins may use; allow its memory request when you install it, or raise the memory ceiling for plugins"
        ),
        "fuel" if requested > MAX_APPROVED_FUEL && allowed == MAX_APPROVED_FUEL => {
            format!("asks for {requested} fuel, more than the hard host ceiling of {allowed} that may be approved at install")
        }
        "fuel" => format!(
            "asks for {requested} fuel, more than the {allowed} plugins may use; allow its fuel request when you install it, or raise the fuel ceiling for plugins"
        ),
        "host_calls" => format!(
            "asks for {requested} host calls, more than the {allowed} plugins may use; raise Max plugin host calls in Preferences > Plugins (max_host_calls in --plugin-limits)"
        ),
        "deadline_ms" => format!(
            "asks for {requested} ms of guest time, more than the {allowed} ms plugins may use; raise Max plugin guest time (ms) in Preferences > Plugins (max_deadline_ms in --plugin-limits)"
        ),
        _ => format!("asks for {requested} {limit}, more than the {allowed} plugins may use"),
    }
}

fn exhausted(limit: &str, allowed: u64) -> String {
    match limit {
        "fuel" => format!("used all {allowed} fuel it asks for and was stopped"),
        "memory_mib" => format!("needed more than the {allowed} MiB of memory it asks for and was stopped"),
        "deadline_ms" if allowed == 0 => "ran longer than its time limit and was stopped".into(),
        "deadline_ms" => format!(
            "ran longer than its time limit of {allowed} ms and was stopped; raise runtime.limits.deadline_ms and, if needed, Max plugin guest time (ms) in Preferences > Plugins (max_deadline_ms in --plugin-limits)"
        ),
        "host_calls" => format!(
            "used all {allowed} host calls it asks for and was stopped; raise runtime.limits.host_calls and, if needed, Max plugin host calls in Preferences > Plugins (max_host_calls in --plugin-limits)"
        ),
        "stack_kib" => format!("ran out of its {allowed} KiB stack and was stopped"),
        "design_mib" => format!("made a design larger than {allowed} MiB, the most an importer may hand over"),
        _ => format!("went over its {limit} limit of {allowed} and was stopped"),
    }
}

/// The capabilities this host serves, as name and `major.minor`. Each is a set of WIT interfaces
/// a plugin names in its manifest's `capabilities` list. While a major version is 0, a minor
/// version may break the one before it, so 0.x matches only the same minor.
pub const CAPABILITIES: &[(&str, (u32, u32))] = &[
    ("commands", (0, 2)),
    ("design", (0, 1)),
    ("dialog", (0, 1)),
    ("evaluate", (0, 2)),
    ("folders", (0, 1)),
    ("formats", (0, 2)),
    ("forms", (0, 1)),
    ("host", (0, 1)),
    ("shaders", (0, 2)),
    ("storage", (0, 1)),
    ("themes", (0, 1)),
    ("ui", (0, 1)),
    ("user-storage", (0, 1)),
];
/// Older versions of [`CAPABILITIES`] the host still serves: plugins built against
/// `craft:plugin@0.1.0` ([`WIT_0_1`]) run unchanged.
pub const OLDER_CAPABILITIES: &[(&str, (u32, u32))] = &[("commands", (0, 1)), ("evaluate", (0, 1)), ("formats", (0, 1)), ("shaders", (0, 1))];
/// The WIT versions a host runs, newest first: [`API`]'s and the frozen 0.1 one.
pub const WIT_VERSIONS: &[&str] = &["0.2.0", "0.1.0"];
/// `craft:plugin@0.1.0`, unchanged, for plugins built before 0.2.
pub const WIT_0_1: &str = include_str!("../wit-0.1/plugin.wit");

/// The highest data format number a plugin may declare (`runtime.data_format`).
pub const MAX_DATA_FORMAT: u32 = 1_000_000;

/// Parse a capability version as major/minor, accepting an optional numeric patch.
pub fn capability_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    match (parts.next(), parts.next()) {
        (None, _) => Some((major, minor)),
        (Some(patch), None) if patch.parse::<u32>().is_ok() => Some((major, minor)),
        _ => None,
    }
}

/// Checks a manifest's `capabilities` against [`CAPABILITIES`]. `app` names the host in the
/// refusal ("update SolidCraft or the plugin").
pub fn check_capabilities(requested: &[String], app: &str) -> Result<()> {
    check_capability_declarations(requested, &[])?;
    for capability in requested {
        let (name, (major, minor)) = parse_capability(capability, "capabilities")?;
        let unsupported = |reason: String| HostError::UnsupportedCapability { capability: capability.clone(), reason };
        let Some((_, (have_major, have_minor))) = CAPABILITIES.iter().find(|(known, _)| *known == name) else {
            return Err(unsupported(format!("needs {name} {major}.{minor}, which this version of {app} does not have; update {app} or the plugin")));
        };
        let (have_major, have_minor) = (*have_major, *have_minor);
        if OLDER_CAPABILITIES.iter().any(|(known, version)| *known == name && *version == (major, minor)) {
            continue;
        }
        if (major, minor) > (have_major, have_minor) {
            return Err(unsupported(format!("needs {name} {major}.{minor}; update {app} or the plugin")));
        }
        if major != have_major || (major == 0 && minor != have_minor) {
            return Err(unsupported(format!(
                "was built for {name} {major}.{minor}, and this version of {app} runs {name} {have_major}.{have_minor}; update the plugin"
            )));
        }
    }
    Ok(())
}

fn parse_capability<'a>(capability: &'a str, field: &str) -> Result<(&'a str, (u32, u32))> {
    capability
        .split_once('@')
        .and_then(|(name, version)| Some((name, capability_version(version)?)))
        .filter(|(name, _)| valid_id(name))
        .ok_or_else(|| invalid(field, format!("`{capability}` is not name@major.minor, e.g. \"commands@0.1\"")))
}

fn check_capability_declarations(required: &[String], optional: &[String]) -> Result<()> {
    let mut names = BTreeSet::new();
    for (field, capabilities) in [("capabilities", required), ("optional_capabilities", optional)] {
        if capabilities.len() > 16 {
            return Err(invalid(field, "at most 16 capabilities"));
        }
        for capability in capabilities {
            let (name, _) = parse_capability(capability, field)?;
            if !names.insert(name) {
                return Err(invalid(field, format!("`{name}` is listed twice")));
            }
        }
    }
    Ok(())
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> HostError {
    HostError::InvalidManifest { field: field.into(), reason: reason.into() }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Reads the open design and changes nothing.
    Read,
    /// Changes the open design.
    #[default]
    Edit,
    /// Opens a file as a new design and leaves the open one as it is: every importer's effect.
    Open,
}

/// The app supplies the same contracts it exposes to its other programmatic clients.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandSchema {
    pub name: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub effect: Effect,
    /// A sensitive command's permission group (`file`, `doc`, ...): callable only when the
    /// manifest's `permissions.command_groups` names it and the app grants it. `None` for
    /// commands that act on the open design alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// The severity level of a message logged by a plugin guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A named folder resolved by the app after user approval, never a guest-supplied root.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileGrant {
    pub name: String,
    pub folder: PathBuf,
    pub read: bool,
    pub write: bool,
}

/// Folder access ordered for install-review prefill, from no access through read to write.
/// Write includes requests with both flags. This comparison never authorizes file operations:
/// runtime checks still use the grant's exact `read` and `write` flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileAccess {
    None,
    Read,
    Write,
}

impl FileGrant {
    /// The highest requested or granted access, only for comparing review starting values.
    pub fn access(&self) -> FileAccess {
        if self.write {
            FileAccess::Write
        } else if self.read {
            FileAccess::Read
        } else {
            FileAccess::None
        }
    }
}

/// A runtime brackets a whole plugin call in one transaction. Guest failures roll it back.
/// The app must retain its original state until commit, including its undo history.
/// Granted file writes are staged until the guest succeeds and the app commits; rollback
/// removes staged files without replacing their targets. Publication errors after app
/// commit can leave a subset of files applied and must be reported to the caller.
/// An importer with user storage leaves its transaction pending after conversion; the app
/// commits only when it accepts the validated design, or discards it on failure.
pub trait PluginHost {
    /// Host-side notification of importer preparation and conversion. Not a guest capability.
    fn importer_stage(&mut self, _plugin: &str, _preparing: bool) {}

    /// Request a populated form. Programs refuse; interactive hosts defer opening until success.
    fn open_form(&mut self, entry: &str, _params: &Value) -> Result<()> {
        Err(HostError::InteractiveRequired { entry: entry.into() })
    }
    /// Only the host-authenticated plugin's per-user JSON settings. Optional for existing hosts.
    fn user_storage_get(&mut self, _key: &str) -> Result<Option<Value>> {
        Err(HostError::PermissionDenied { permission: "user-storage is unavailable".into() })
    }
    fn user_storage_set(&mut self, _key: &str, _value: &Value) -> Result<()> {
        Err(HostError::PermissionDenied { permission: "user-storage is unavailable".into() })
    }
    fn user_storage_remove(&mut self, _key: &str) -> Result<bool> {
        Err(HostError::PermissionDenied { permission: "user-storage is unavailable".into() })
    }
    fn user_storage_list(&mut self) -> Result<Vec<String>> {
        Err(HostError::PermissionDenied { permission: "user-storage is unavailable".into() })
    }
    /// The active document's identity and committed revision (`design@0.1`), or none when closed.
    /// Existing hosts need not implement this additive read-only capability.
    fn design_token(&self) -> Result<Option<DesignToken>> {
        Err(HostError::PermissionDenied { permission: "design@0.1 is not available in this host".into() })
    }
    fn command_catalog(&self) -> Vec<CommandSchema>;
    fn run_command(&mut self, name: &str, args: &Value) -> Result<Value>;
    fn begin_transaction(&mut self) -> Result<()>;
    fn commit_transaction(&mut self) -> Result<()>;
    /// Called after the approved file writes are in place; anything that must not outlive a failed
    /// publication (per-user settings) is made permanent here. Nothing to do by default.
    fn publish_committed(&mut self) -> Result<()> {
        Ok(())
    }
    /// Called instead of [`PluginHost::publish_committed`] when the approved file writes could not
    /// be published after a commit: drop what was staged for publication. The transaction has
    /// already committed, so it is not rolled back. Nothing to do by default.
    fn discard_committed(&mut self) {}
    fn rollback_transaction(&mut self) -> Result<()>;
    fn granted_folders(&self) -> Vec<FileGrant>;
    /// Read-only roots for annotated entry fields, already authorised by the app for this call.
    /// Names are property keys, not install-time permission names. No grants by default.
    fn granted_input_folders(&self) -> Vec<FileGrant> {
        Vec::new()
    }
    /// Apply the caller's scope to a file beneath an input folder. Separate from the legacy
    /// files interface so an input grant cannot widen any other host operation.
    fn authorize_input_file_path(&self, path: &Path) -> Result<()> {
        self.authorize_file_path(path, false)
    }
    /// The sensitive command groups the user granted. A guest may call a grouped command only
    /// when its manifest requests the group too. None by default.
    fn granted_command_groups(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }
    /// Apply the caller's scope to every complete file path, in addition to folder grants.
    /// Approved folder roots may have hidden ancestors. Hidden child names require
    /// a more specific folder grant or a host override applying explicit scope.
    fn authorize_file_path(&self, path: &Path, write: bool) -> Result<()> {
        let grants = self.granted_folders();
        let relative = grants
            .iter()
            .filter(|grant| if write { grant.write } else { grant.read })
            .filter_map(|grant| path.strip_prefix(&grant.folder).ok())
            .min_by_key(|relative| relative.components().count())
            .unwrap_or(path);
        if relative.components().any(|component| matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with('.'))) {
            return Err(HostError::PermissionDenied { permission: format!("hidden file: {}", path.display()) });
        }
        Ok(())
    }
    fn log(&mut self, message: &str);
    /// Receive a log message with its severity level from a plugin guest.
    /// By default delegates to [`PluginHost::log`].
    fn log_level(&mut self, level: LogLevel, message: &str) {
        let _ = level;
        self.log(message);
    }
    fn progress(&mut self, fraction: f32, message: &str);
    /// Writes the running plugin's file `name` in the open design from its bytes, as they are
    /// (`storage@0.1`, the `storage` interface's `put-blob`); `append` adds them to its end. The
    /// file's size. An app that keeps no plugin data refuses, as here.
    fn storage_put_blob(&mut self, _name: &str, _data: &[u8], _cache: bool, _append: bool) -> Result<u64> {
        Err(HostError::PermissionDenied { permission: "storage: this app keeps no plugin data".into() })
    }
    /// Up to `len` bytes of a plugin's file from `offset` (`get-blob`).
    fn storage_get_blob(&mut self, _plugin: Option<&str>, _name: &str, _offset: u64, _len: u32) -> Result<Vec<u8>> {
        Err(HostError::PermissionDenied { permission: "storage: this app keeps no plugin data".into() })
    }
    /// Up to `len` bytes of a plugin's main document, its saved JSON, from `offset` (`get-document`).
    fn storage_get_document(&mut self, _plugin: Option<&str>, _offset: u64, _len: u32) -> Result<Vec<u8>> {
        Err(HostError::PermissionDenied { permission: "storage: this app keeps no plugin data".into() })
    }
    fn is_cancelled(&self) -> bool {
        false
    }
    /// Opens `design`, a design file an importer made, in a private session apart from every open
    /// design and every file, for [`PluginHost::evaluate_run`] (capability `evaluate@0.1`). An app
    /// that doesn't check designs refuses, as here.
    fn evaluate_open(&mut self, _design: &[u8]) -> Result<()> {
        Err(HostError::PermissionDenied { permission: "evaluate: this app does not check imported designs".into() })
    }
    /// Runs one of the app's read-only commands on the design [`PluginHost::evaluate_open`] opened.
    /// A command that fails is an answer (`{"error": ...}`), not an error.
    fn evaluate_run(&mut self, command: &str, _args: &Value) -> Result<Value> {
        Err(HostError::PermissionDenied { permission: format!("evaluate: {command} is not available") })
    }
    /// Discards the session [`PluginHost::evaluate_open`] opened.
    fn evaluate_close(&mut self) {}
    /// Stage data for one declared panel in the calling plugin's namespace. Rollback must
    /// discard these updates; successful updates are adopted once per rendered frame.
    fn ui_set_data(&mut self, _panel: &str, _updates: &[ui::Datum]) -> Result<()> {
        Err(HostError::Unsupported { reason: "plugin panels are unavailable".into() })
    }
    /// Clear a declared panel's data, under the same transaction as the entry.
    fn ui_clear_data(&mut self, _panel: &str) -> Result<()> {
        Err(HostError::Unsupported { reason: "plugin panels are unavailable".into() })
    }
    /// Change visibility only for a panel the calling plugin declared.
    fn ui_show_panel(&mut self, _panel: &str, _visible: bool) -> Result<()> {
        Err(HostError::Unsupported { reason: "plugin panels are unavailable".into() })
    }
}

/// User-owned ceilings, separate from the package's exact content-approved request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HostLimits {
    pub max_fuel: u64,
    pub max_memory_mib: usize,
    pub max_host_calls: u64,
    pub max_deadline_ms: u64,
}
impl Default for HostLimits {
    fn default() -> Self {
        Self { max_fuel: 100_000_000_000, max_memory_mib: 128, max_host_calls: 256, max_deadline_ms: MAX_COMMAND_DEADLINE_MS }
    }
}
impl HostLimits {
    pub fn validate(&self) -> Result<()> {
        if self.max_fuel == 0 || self.max_host_calls == 0 || self.max_deadline_ms == 0 || !(1..=MAX_MEMORY_MIB).contains(&self.max_memory_mib) {
            return Err(invalid("host_limits", "requires positive max_fuel, max_host_calls, max_deadline_ms and max_memory_mib 1..4096 (Wasm32)"));
        }
        Ok(())
    }
    pub fn check(&self, requested: &Limits) -> Result<()> {
        self.check_kind(requested, false)
    }
    /// Importers retain their existing ten-minute ceiling and separate host-import budget.
    pub fn check_importer(&self, requested: &Limits) -> Result<()> {
        self.check_kind(requested, true)
    }
    fn check_kind(&self, requested: &Limits, importer: bool) -> Result<()> {
        self.validate()?;
        requested.validate()?;
        if requested.fuel > self.max_fuel {
            return Err(HostError::OverCeiling { limit: "fuel".into(), requested: requested.fuel, allowed: self.max_fuel });
        }
        if requested.memory_mib > self.max_memory_mib {
            return Err(HostError::OverCeiling {
                limit: "memory_mib".into(),
                requested: requested.memory_mib as u64,
                allowed: self.max_memory_mib as u64,
            });
        }
        if !importer && requested.host_calls > self.max_host_calls {
            return Err(HostError::OverCeiling { limit: "host_calls".into(), requested: requested.host_calls, allowed: self.max_host_calls });
        }
        let max_deadline_ms = if importer { self.max_deadline_ms.max(MAX_IMPORTER_DEADLINE_MS) } else { self.max_deadline_ms };
        if requested.deadline_ms > max_deadline_ms {
            return Err(HostError::OverCeiling { limit: "deadline_ms".into(), requested: requested.deadline_ms, allowed: max_deadline_ms });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub fuel: u64,
    pub memory_mib: usize,
    pub deadline_ms: u64,
    #[serde(default = "default_host_calls")]
    pub host_calls: u64,
    /// The guest stack in KiB, when the default is too small (a real decoder needed about 16 MiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_kib: Option<u32>,
}
fn default_host_calls() -> u64 {
    256
}
impl Default for Limits {
    fn default() -> Self {
        Self { fuel: 10_000_000, memory_mib: 64, deadline_ms: 5000, host_calls: default_host_calls(), stack_kib: None }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.fuel == 0 {
            return Err(invalid("runtime.limits.fuel", "must be positive"));
        }
        if !(1..=MAX_MEMORY_MIB).contains(&self.memory_mib) {
            return Err(invalid("runtime.limits.memory_mib", "must be 1..4096 (Wasm32)"));
        }
        if self.deadline_ms == 0 {
            return Err(invalid("runtime.limits.deadline_ms", "must be positive"));
        }
        if self.host_calls == 0 {
            return Err(invalid("runtime.limits.host_calls", "must be positive"));
        }
        if self.stack_kib.is_some_and(|kib| !(64..=65_536).contains(&kib)) {
            return Err(invalid("runtime.limits.stack_kib", "must be 64..65536 (64 KiB to 64 MiB)"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    pub files: Vec<FilePermission>,
    pub clock: bool,
    /// Sensitive command groups the plugin asks for, such as `file` or `doc`. Each is refused
    /// unless the app grants it too; which groups exist is the app's.
    pub command_groups: BTreeSet<String>,
    /// An importer may read the other files in the folder of the file it imports (an assembly's
    /// parts), through `source`. Nothing outside that folder, and only during that one import.
    pub input_folder: bool,
    /// Reserved: refused in this API regardless of app grants.
    pub net: bool,
}
impl Permissions {
    pub fn validate(&self) -> Result<()> {
        if self.net {
            return Err(HostError::PermissionDenied { permission: "net".into() });
        }
        if self.command_groups.len() > 16 || self.command_groups.iter().any(|group| !valid_id(group)) {
            return Err(invalid("runtime.permissions.command_groups", "at most 16 lowercase group names"));
        }
        if self.files.len() > 32 {
            return Err(invalid("runtime.permissions.files", "at most 32 folder grants"));
        }
        let mut names = BTreeSet::new();
        for grant in &self.files {
            if !valid_id(&grant.name) || (!grant.read && !grant.write) || !names.insert(&grant.name) {
                return Err(invalid("runtime.permissions.files", "distinct named grants must request read or write"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilePermission {
    pub name: String,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
}

/// Apps decide which named slots exist. Plugins only declare placement preferences.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum UiSlot {
    Menu {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Ribbon {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_group: Option<bool>,
    },
}

fn empty_input_schema() -> Value {
    serde_json::json!({"type":"object","properties":{}})
}

/// What an entry is: a command the plugin runs on the open design, or a file-format slot (RFC
/// 0015 §4): an importer turns a file of a claimed extension into a new design, and an exporter
/// writes the open design as a file of a claimed extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    Command,
    Importer,
    Exporter,
}
impl EntryKind {
    /// An import or export slot: it claims file extensions.
    pub fn is_format(self) -> bool {
        matches!(self, Self::Importer | Self::Exporter)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(from = "EntryDeclaration")]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    /// The file extensions an import or export slot claims: lowercase, without the dot. Empty for
    /// a command.
    pub extensions: Vec<String>,
    /// A format slot's name in the app's Open and Export lists ("Sample mesh"). Empty: the
    /// plugin's name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// Export slots only: the export also reads the design's own data of this plugin (RFC 0015
    /// §5). Accepted now; the data is passed once per-design plugin data lands.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_plugin_data: bool,
    /// Importers only: File > Open, a drop and the importer's menu entry choose the file first and
    /// then show the options, so they can match it. Absent or false shows the options first.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file_first: bool,
    pub effect: Effect,
    /// The entry's options, as an object schema. A format slot's options are drawn as a form
    /// before the import or export.
    pub input_schema: Value,
    /// Property names in declaration order, keyed by the object schema's JSON Pointer (the
    /// empty string for the root). Host metadata keeps that order across entry round trips;
    /// ordinary JSON maps remain sorted. Missing keys use the dialog's usual field order.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input_property_order: BTreeMap<String, Vec<String>>,
    /// Opt in to the read-only dialog-provider export (dialog@0.1).
    pub dialog_hook: bool,
    pub slots: Vec<UiSlot>,
    /// A long job: it runs for up to [`JOB_DEADLINE_SECONDS`] instead of the runtime's `deadline_ms`,
    /// and shows its progress in the app's operations.
    pub job: bool,
    /// Whether this entry appears in the Model tab's default Plugins group. None follows slot placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_group: Option<bool>,
}

impl Entry {
    /// Whether this importer claims `path`'s extension (ignoring case).
    pub fn claims(&self, path: &Path) -> bool {
        self.kind == EntryKind::Importer && self.has_extension_of(path)
    }
    /// Whether this exporter writes files with `path`'s extension (ignoring case).
    pub fn writes(&self, path: &Path) -> bool {
        self.kind == EntryKind::Exporter && self.has_extension_of(path)
    }
    fn has_extension_of(&self, path: &Path) -> bool {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        !extension.is_empty() && self.extensions.iter().any(|e| e.eq_ignore_ascii_case(extension))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum EntryDeclaration {
    Legacy(String),
    Declared(DeclaredEntry),
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeclaredEntry {
    name: String,
    #[serde(default)]
    kind: EntryKind,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    label: String,
    #[serde(default)]
    needs_plugin_data: bool,
    #[serde(default)]
    file_first: bool,
    /// A command's effect is `edit` unless it says `read`; an exporter's is `read`; an importer's is
    /// `open`. An importer that says `edit` (the only effect it could state before `open` existed)
    /// still opens a new design.
    #[serde(default)]
    effect: Option<Effect>,
    #[serde(default)]
    #[schemars(with = "Value")]
    input_schema: schema_order::InputSchema,
    // Present in a host's serialized Entry; plugin authors only declare schema properties.
    #[serde(default)]
    input_property_order: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    dialog_hook: bool,
    #[serde(default)]
    slots: Vec<UiSlot>,
    #[serde(default)]
    job: bool,
    #[serde(default)]
    model_group: Option<bool>,
}
impl From<EntryDeclaration> for Entry {
    fn from(value: EntryDeclaration) -> Self {
        match value {
            EntryDeclaration::Legacy(name) => Self {
                name,
                kind: EntryKind::Command,
                extensions: Vec::new(),
                label: String::new(),
                needs_plugin_data: false,
                file_first: false,
                effect: Effect::Edit,
                input_schema: empty_input_schema(),
                input_property_order: BTreeMap::new(),
                dialog_hook: false,
                slots: Vec::new(),
                job: false,
                model_group: None,
            },
            EntryDeclaration::Declared(value) => Self {
                name: value.name,
                kind: value.kind,
                extensions: value.extensions,
                label: value.label.trim().to_string(),
                needs_plugin_data: value.needs_plugin_data,
                file_first: value.file_first,
                effect: match (value.kind, value.effect) {
                    (EntryKind::Importer, None | Some(Effect::Edit)) => Effect::Open,
                    (_, Some(effect)) => effect,
                    (EntryKind::Exporter, None) => Effect::Read,
                    (EntryKind::Command, None) => Effect::Edit,
                },
                input_schema: value.input_schema.value,
                // An object with no properties has no order to keep: an entry that declared no schema
                // serializes without one and must read back the same.
                input_property_order: value
                    .input_property_order
                    .unwrap_or(value.input_schema.property_order)
                    .into_iter()
                    .filter(|(_, names)| !names.is_empty())
                    .collect(),
                dialog_hook: value.dialog_hook,
                slots: value.slots,
                job: value.job,
                model_group: value.model_group,
            },
        }
    }
}

/// Metadata is inert; executing its runtime still requires the app's explicit approval.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The oldest supported host version, as a complete semantic version (`1.2.3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_host_version: Option<String>,
    #[serde(default)]
    pub description: String,
    /// The host capabilities the plugin needs, as `name@major.minor` (`commands@0.1`). A runtime
    /// needs `commands`. The host refuses a capability it does not serve, before any code runs.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Capabilities the host grants when supported; the plugin must provide a fallback.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optional_capabilities: Vec<String>,
    /// Inert metadata reserved for future optional fields. Unknown keys are deliberately accepted.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    #[schemars(with = "std::collections::BTreeMap<String, Value>")]
    pub extensions: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeManifest>,
    /// View backgrounds and tints (`shaders@0.2`); a plugin may ship shaders and no runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shaders: Vec<shader::Shader>,
    /// Colour themes for the theme picker (`themes@0.1`); a plugin may ship themes and no runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub themes: Vec<theme::ThemeDecl>,
    /// Where newer versions are published. SolidCraft before 0.49 refuses the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updates: Option<Updates>,
}

/// The plugin's GitHub repository, `owner/repo`: its latest release is the update.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Updates {
    pub github: String,
}

/// A GitHub `owner/repo`: an owner of 1..39 letters, digits and hyphens, and a repository of
/// 1..100 letters, digits, `-`, `_` and `.`, other than `.` and `..`.
pub fn valid_github_repo(text: &str) -> bool {
    let Some((owner, repo)) = text.split_once('/') else { return false };
    (1..=39).contains(&owner.len())
        && owner.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        && !owner.starts_with('-')
        && (1..=100).contains(&repo.len())
        && repo.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        && repo != "."
        && repo != ".."
}

impl Manifest {
    /// Required capabilities followed by the optional capabilities this host supports, retaining
    /// their declared strings and order. This describes API availability, not user permissions.
    pub fn granted_capabilities(&self) -> Vec<String> {
        self.capabilities
            .iter()
            .chain(self.optional_capabilities.iter().filter(|capability| check_capabilities(std::slice::from_ref(*capability), "the app").is_ok()))
            .cloned()
            .collect()
    }
}

/// A read-only entry called after a coalesced design or linked-file invalidation. Listeners refresh
/// their own caches or panel data; they do not edit, write files or start background work.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Listener {
    pub entry: String,
    pub on: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    /// A single file name within the package, never a directory or absolute path.
    pub module: String,
    pub entries: Vec<Entry>,
    /// Explicit ribbon menus. Entries not named here keep their individual buttons.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ribbon_groups: Vec<RibbonGroup>,
    /// Opt-in notifications, under capability `design@0.1`. Missing for existing packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<Listener>,
    /// Exact app command ids, intersected with the app catalog by the runtime.
    pub commands: BTreeSet<String>,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub permissions: Permissions,
    /// The format number of the data the plugin keeps in a design (capability `storage@0.1`),
    /// 1 or more. The app stamps it on what the plugin saves; data saved with a higher number
    /// (by a newer version of the plugin) is read-only to this version, and a lower one tells the
    /// plugin to migrate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_format: Option<u32>,
    /// Static panel declarations listed before the plugin is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ui::Declarations>,
}

/// A plugin's opt-in ribbon button and the entries its menu lists, in order.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RibbonGroup {
    pub label: String,
    /// A package-relative SVG path, checked and loaded by the host like an entry icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub entries: Vec<String>,
}

/// The package-relative SVG paths a host may read for plugin icons.
pub fn valid_icon_path(path: &str) -> bool {
    images::valid_path(path)
}

impl RuntimeManifest {
    /// Whether this runtime is an importer (world `importer-plugin`, optionally with storage).
    /// A runtime's entries are all commands, all importers or all exporters.
    pub fn is_importer(&self) -> bool {
        self.entries.first().is_some_and(|e| e.kind == EntryKind::Importer)
    }
    /// Whether this runtime is an exporter (world `exporter-plugin`).
    pub fn is_exporter(&self) -> bool {
        self.entries.first().is_some_and(|e| e.kind == EntryKind::Exporter)
    }
    /// The largest component this runtime may ship: a format converter may be larger.
    pub fn max_component_bytes(&self) -> usize {
        if self.is_importer() || self.is_exporter() { MAX_IMPORTER_COMPONENT_BYTES } else { MAX_COMPONENT_BYTES }
    }
    /// The importer entry that claims `path`'s extension, if any.
    pub fn importer_for(&self, path: &Path) -> Option<&Entry> {
        self.entries.iter().find(|e| e.claims(path))
    }
    pub fn validate(&self) -> Result<()> {
        if !valid_id(self.module.strip_suffix(".wasm").unwrap_or("")) {
            return Err(invalid("runtime.module", "must be one lowercase .wasm file name"));
        }
        if self.entries.is_empty() || self.entries.len() > 32 {
            return Err(invalid("runtime.entries", format!("declares {} entries; a plugin has 1 to 32", self.entries.len())));
        }
        let mut names = BTreeSet::new();
        for entry in &self.entries {
            // The name as written, quoted and cut short: it comes from an untrusted manifest. A cut
            // name says so and gives its length, as its first 64 characters may be valid alone.
            let length = entry.name.chars().count();
            let shown = match length {
                0..=64 => format!("{:?}", entry.name),
                _ => format!("{:?}… ({length} characters)", entry.name.chars().take(64).collect::<String>()),
            };
            if !valid_id(&entry.name) {
                return Err(invalid(
                    "runtime.entries",
                    format!(
                        "entry {shown} is not a valid name: use 1 to 64 lowercase letters, digits and dashes, not starting or ending with a dash (no underscores or capitals)"
                    ),
                ));
            }
            if !names.insert(&entry.name) {
                return Err(invalid("runtime.entries", format!("entry {shown} is declared more than once")));
            }
            validate_kind(self, entry)?;
            let field = format!("runtime.entries.{}.input_schema", entry.name);
            if !entry.input_schema.is_object() || entry.input_schema.get("type").and_then(Value::as_str) != Some("object") {
                return Err(invalid(&field, "entry inputs must be an object schema"));
            }
            if let Some(order) = entry.input_schema.get("x-solidcraft-field-order")
                && order.as_str() != Some("declared")
            {
                return Err(invalid(format!("{field}.x-solidcraft-field-order"), "must be \"declared\" when present"));
            }
            layout::validate(&entry.input_schema, &field)?;
            validate_list_inputs(&entry.input_schema, &field)?;
            directory_read_fields(&entry.input_schema)?;
            conditions::validate_conditions(&entry.input_schema)?;
            if entry.slots.len() > 16
                || entry.slots.iter().any(|slot| match slot {
                    UiSlot::Menu { name, label } | UiSlot::Ribbon { name, label, .. } => {
                        !plain(name, 80) || label.as_ref().is_some_and(|l| !plain(l, 80))
                    }
                })
            {
                return Err(invalid("runtime.entries.slots", "requires at most 16 named menu or ribbon slots"));
            }
        }
        self.validate_ribbon_groups()?;
        if self.listeners.len() > 16 {
            return Err(invalid("runtime.listeners", "at most 16 distinct listeners"));
        }
        let mut listeners = BTreeSet::new();
        for listener in &self.listeners {
            let entry = self.entries.iter().find(|entry| entry.name == listener.entry);
            let mut events = BTreeSet::new();
            let valid_events = !listener.on.is_empty()
                && listener.on.iter().all(|event| matches!(event.as_str(), "design.changed" | "design.links_changed") && events.insert(event));
            if !listeners.insert(&listener.entry)
                || !valid_events
                || entry.is_none_or(|entry| entry.kind != EntryKind::Command || entry.effect != Effect::Read)
            {
                return Err(invalid(
                    "runtime.listeners",
                    "distinct read command entries listening to a nonempty, unique selection of design.changed and design.links_changed are required",
                ));
            }
        }
        if self.commands.len() > 128 || self.commands.iter().any(|id| !valid_command_name(id)) {
            return Err(invalid("runtime.commands", "requires at most 128 exact command names"));
        }
        if let Some(ui) = &self.ui {
            if self.is_importer() {
                return Err(invalid("runtime.ui", "panels require a command plugin runtime"));
            }
            ui.validate(self)?;
        }
        self.permissions.validate()?;
        self.limits.validate()?;
        if self.is_importer() {
            if !self.commands.is_empty() {
                return Err(invalid("runtime.commands", "an importer runs no app commands; leave the list empty"));
            }
            if !self.permissions.files.is_empty() || !self.permissions.command_groups.is_empty() {
                return Err(invalid("runtime.permissions", "an importer reads only its file and, with input_folder, the files beside it"));
            }
        } else if self.is_exporter() {
            // An exporter reads the design through read-only commands and returns the file's
            // bytes; the app writes them to the path the user chose. It reaches no file itself.
            if !self.permissions.files.is_empty() || !self.permissions.command_groups.is_empty() || self.permissions.input_folder {
                return Err(invalid(
                    "runtime.permissions",
                    "an exporter reads the design through commands and writes only the file the user chooses; it asks for no files, command groups or input_folder",
                ));
            }
        } else {
            if self.permissions.input_folder {
                return Err(invalid("runtime.permissions.input_folder", "only an importer reads the folder of the file it imports"));
            }
        }
        Ok(())
    }

    fn validate_ribbon_groups(&self) -> Result<()> {
        if self.ribbon_groups.len() > 32 {
            return Err(invalid("runtime.ribbon_groups", "requires at most 32 ribbon groups"));
        }
        let mut labels = BTreeSet::new();
        let mut grouped = BTreeSet::new();
        for (index, group) in self.ribbon_groups.iter().enumerate() {
            let field = format!("runtime.ribbon_groups.{index}");
            if !plain(&group.label, 80) {
                return Err(invalid(format!("{field}.label"), "requires 1..80 printable characters"));
            }
            if !labels.insert(group.label.trim()) {
                return Err(invalid(format!("{field}.label"), format!("ribbon group `{}` is listed twice", group.label.trim())));
            }
            if group.icon.as_deref().is_some_and(|icon| !valid_icon_path(icon)) {
                return Err(invalid(format!("{field}.icon"), "must be a relative .svg path inside the package, at most 128 characters"));
            }
            if group.entries.is_empty() || group.entries.len() > 32 {
                return Err(invalid(format!("{field}.entries"), "requires 1..32 distinct entry names"));
            }
            for name in &group.entries {
                if !self.entries.iter().any(|entry| entry.name == *name) {
                    return Err(invalid(format!("{field}.entries"), format!("unknown entry `{name}`; name an entry from runtime.entries")));
                }
                if !grouped.insert(name) {
                    return Err(invalid(
                        format!("{field}.entries"),
                        format!("entry `{name}` is listed twice; an entry belongs to at most one ribbon group"),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A format slot (importer or exporter) claims 1..32 distinct lowercase extensions; an importer
/// edits (it makes a design) and an exporter reads. A command claims none. One runtime does not
/// mix kinds: they are different worlds.
fn validate_kind(runtime: &RuntimeManifest, entry: &Entry) -> Result<()> {
    let field = format!("runtime.entries.{}", entry.name);
    if runtime.entries.iter().any(|other| other.kind != entry.kind) {
        return Err(invalid(
            "runtime.entries",
            "entries are all commands, all importers or all exporters; ship an importer or exporter as a plugin of its own",
        ));
    }
    if !entry.label.is_empty() && !plain(&entry.label, 80) {
        return Err(invalid(format!("{field}.label"), "a slot's label is 1..80 printable characters"));
    }
    if entry.needs_plugin_data && entry.kind != EntryKind::Exporter {
        return Err(invalid(format!("{field}.needs_plugin_data"), "only an export slot reads the design's plugin data"));
    }
    if entry.file_first && entry.kind != EntryKind::Importer {
        return Err(invalid(format!("{field}.file_first"), "only an importer chooses its file first"));
    }
    match entry.kind {
        EntryKind::Command if !entry.extensions.is_empty() || !entry.label.is_empty() => {
            Err(invalid(format!("{field}.extensions"), "only an import or export slot claims file extensions and has a label"))
        }
        EntryKind::Command if entry.effect == Effect::Open => {
            Err(invalid(format!("{field}.effect"), "only an importer opens a file as a new design; a command's effect is read or edit"))
        }
        EntryKind::Importer if entry.job => {
            Err(invalid(format!("{field}.job"), "an import already runs as a long job; only a command is declared one"))
        }
        EntryKind::Command => Ok(()),
        EntryKind::Importer | EntryKind::Exporter => {
            let mut seen = BTreeSet::new();
            let is_alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
            let valid = |e: &String| {
                let bytes = e.as_bytes();
                (1..=16).contains(&bytes.len())
                    && bytes.first().copied().is_some_and(is_alnum)
                    && bytes.last().copied().is_some_and(is_alnum)
                    && bytes.iter().all(|&c| is_alnum(c) || c == b'_')
            };
            if entry.extensions.is_empty() || entry.extensions.len() > MAX_EXTENSIONS || !entry.extensions.iter().all(|e| valid(e) && seen.insert(e))
            {
                return Err(invalid(
                    format!("{field}.extensions"),
                    "an import or export slot claims 1..32 distinct lowercase extensions without the dot, such as \"f3d\"",
                ));
            }
            if entry.kind == EntryKind::Importer && entry.effect != Effect::Open {
                return Err(invalid(format!("{field}.effect"), "an importer opens a file as a new design: its effect is open"));
            }
            if entry.kind == EntryKind::Exporter && entry.effect != Effect::Read {
                return Err(invalid(format!("{field}.effect"), "an exporter only reads the design: its effect is read"));
            }
            Ok(())
        }
    }
}

/// How bad one loss is: kept with a note, carried over in a lesser form, or left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LossSeverity {
    Info,
    Degraded,
    Dropped,
}

/// One thing an importer could not carry over as it was.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LossItem {
    pub severity: LossSeverity,
    /// What kind of loss it is, in the user's words, the same for every item of that kind
    /// ("Parts of the file kept as they are"): the host shows one line per title with its count.
    /// May be empty; the item then stands on its own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// What it is about: a feature, body or part name. May be empty.
    #[serde(default)]
    pub subject: String,
    pub message: String,
    /// The importer's stable code for this kind of loss, for bug reports. Shown under Details.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
}

/// What an importer returns besides the design: what it could not carry over. The host shows
/// the summary and items to the user; `details` is the importer's own data, kept for programs.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LossReport {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub items: Vec<LossItem>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}
pub const MAX_LOSS_ITEMS: usize = 4096;
impl LossReport {
    pub fn validate(&self) -> Result<()> {
        let text = |value: &str, max: usize| value.chars().count() <= max && !value.chars().any(char::is_control);
        if !text(&self.summary, 240) {
            return Err(HostError::InvalidInput { field: "loss_report.summary".into(), reason: "at most 240 printable characters".into() });
        }
        if self.items.len() > MAX_LOSS_ITEMS
            || !self.items.iter().all(|i| text(&i.subject, 256) && plain(&i.message, 1024) && text(&i.title, 120) && text(&i.code, 128))
        {
            return Err(HostError::InvalidInput {
                field: "loss_report.items".into(),
                reason: "at most 4096 items, each a printable message of at most 1024 characters, a subject of at most 256, a title of at most 120 and a code of at most 128".into(),
            });
        }
        Ok(())
    }
}

/// Top-level directory properties that request read access for the entry's current run.
/// Other placements and access modes are refused rather than silently ignored.
pub fn directory_read_fields(schema: &Value) -> Result<BTreeSet<String>> {
    // Keep discovery and placement validation in step across schema drafts.
    const SCHEMA_MAPS: &[&str] = &["$defs", "definitions", "dependencies", "dependentSchemas", "patternProperties", "properties"];
    const SCHEMAS: &[&str] = &[
        "additionalItems",
        "additionalProperties",
        "allOf",
        "anyOf",
        "contains",
        "contentSchema",
        "else",
        "if",
        "items",
        "not",
        "oneOf",
        "prefixItems",
        "propertyNames",
        "then",
        "unevaluatedItems",
        "unevaluatedProperties",
    ];
    // Existing schemas with no annotation keep their previous loading limits.
    let mut pending = vec![schema];
    let mut annotated = false;
    while let Some(node) = pending.pop() {
        if node.get("x-solidcraft-grant").is_some() {
            annotated = true;
            break;
        }
        match node {
            Value::Object(object) => {
                for &key in SCHEMA_MAPS {
                    if let Some(children) = object.get(key).and_then(Value::as_object) {
                        pending.extend(children.values());
                    }
                }
                for &key in SCHEMAS {
                    pending.extend(object.get(key));
                }
            }
            Value::Array(array) => pending.extend(array),
            _ => {}
        }
    }
    if !annotated {
        return Ok(BTreeSet::new());
    }
    fn visit(schema: &Value, field: &str, allowed: bool, depth: usize, steps: &mut usize) -> Result<()> {
        *steps += 1;
        if depth > 32 || *steps > 16384 {
            return Err(invalid(field, "folder annotations are nested too deeply or too numerous"));
        }
        if let Some(grant) = schema.get("x-solidcraft-grant")
            && (!allowed
                || grant.as_str() != Some("read")
                || schema.get("type").and_then(Value::as_str) != Some("string")
                || schema.get("format").and_then(Value::as_str) != Some("directory"))
        {
            return Err(invalid(field, "x-solidcraft-grant must be read on a top-level string property with format directory"));
        }
        for &key in SCHEMA_MAPS {
            if let Some(properties) = schema.get(key).and_then(Value::as_object) {
                for (name, property) in properties {
                    visit(property, &format!("{field}.{key}.{name}"), depth == 0 && key == "properties", depth + 1, steps)?;
                }
            }
        }
        for &key in SCHEMAS {
            if let Some(child) = schema.get(key) {
                if let Some(children) = child.as_array() {
                    for child in children {
                        visit(child, &format!("{field}.{key}"), false, depth + 1, steps)?;
                    }
                } else {
                    visit(child, &format!("{field}.{key}"), false, depth + 1, steps)?;
                }
            }
        }
        Ok(())
    }
    visit(schema, "input_schema", false, 0, &mut 0)?;
    let fields: BTreeSet<_> = schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, property)| property.get("x-solidcraft-grant").is_some())
        .map(|(key, _)| key.clone())
        .collect();
    if fields.len() > 32 || fields.iter().any(|key| !plain(key, 128)) {
        return Err(invalid("input_schema", "at most 32 folder grants, with printable property names of at most 128 characters"));
    }
    Ok(fields)
}

fn validate_list_inputs(schema: &Value, field: &str) -> Result<()> {
    if let Some(properties) = schema.get("properties") {
        let properties = properties.as_object().ok_or_else(|| invalid(format!("{field}.properties"), "must be an object"))?;
        for (name, property) in properties {
            let field = format!("{field}.properties.{name}");
            if matches!(name.as_str(), "selection" | "target" | "targets" | "bodies" | "body_ids")
                && property.get("type").and_then(Value::as_str) != Some("array")
            {
                return Err(invalid(&field, "selection and target inputs must be lists"));
            }
            font_field(property).map_err(|_| invalid(format!("{field}.{FONT_KEY}"), "must be true on a string property"))?;
            validate_list_inputs(property, &field)?;
        }
    }
    Ok(())
}

pub fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
}
fn valid_command_name(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
}
fn plain(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}

/// A plugin that keeps data in designs asks for `storage@0.1` and says its data's format number;
/// only a command plugin does: an importer makes a new design and an exporter only reads it.
fn check_storage(manifest: &Manifest) -> Result<()> {
    if manifest.granted_capabilities().iter().any(|capability| capability.starts_with("user-storage@")) && manifest.runtime.is_none() {
        return Err(invalid("capabilities", "user-storage requires a command or importer plugin runtime"));
    }

    let storage = manifest.granted_capabilities().iter().any(|capability| capability.starts_with("storage@"));
    let runtime = manifest.runtime.as_ref();
    let format = runtime.and_then(|runtime| runtime.data_format);
    if !storage {
        if format.is_some() {
            return Err(invalid(
                "runtime.data_format",
                "only a plugin that keeps data in designs has a data format: add \"storage@0.1\" to capabilities",
            ));
        }
        return Ok(());
    }
    if runtime.is_none_or(|runtime| runtime.is_importer() || runtime.is_exporter()) {
        return Err(invalid(
            "capabilities",
            "storage keeps a command plugin's data in the open design; an importer, exporter or a plugin without a runtime has none",
        ));
    }
    if !format.is_some_and(|format| (1..=MAX_DATA_FORMAT).contains(&format)) {
        return Err(invalid("runtime.data_format", "a plugin with storage says its data's format number, 1 to 1000000, e.g. \"data_format\": 1"));
    }
    Ok(())
}

pub fn parse_manifest(text: &[u8], folder: &str) -> Result<Manifest> {
    parse_manifest_for(text, folder, "the app")
}

/// [`parse_manifest`], naming the host `app` in a capability refusal.
pub fn parse_manifest_for(text: &[u8], folder: &str, app: &str) -> Result<Manifest> {
    parse_manifest_for_version(text, folder, app, env!("CARGO_PKG_VERSION"))
}

/// [`parse_manifest_for`], using an explicit host semantic version. The minimum host version is
/// checked before unknown fields, legacy API declarations or capability support.
pub fn parse_manifest_for_version(text: &[u8], folder: &str, app: &str, host_version: &str) -> Result<Manifest> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(HostError::LimitExceeded { limit: "manifest size exceeds 64 KiB".into() });
    }
    // A plugin from before capabilities names its API in `runtime.api`: say so plainly.
    // Read with each schema's declared property order stored beside it: expansion below works on
    // a sorted JSON value, and a schema's order would be alphabetical by then.
    let mut parsed = serde_json::from_slice::<schema_order::MarkedManifest>(text).ok().map(|marked| marked.0);
    if let Some(value) = &parsed {
        if let Some(minimum) = value.get("min_host_version") {
            let minimum = minimum.as_str().ok_or_else(|| invalid("min_host_version", "must be a semantic version string, e.g. \"1.2.3\""))?;
            let required =
                semver::Version::parse(minimum).map_err(|_| invalid("min_host_version", "must be a complete semantic version, e.g. \"1.2.3\""))?;
            let current = semver::Version::parse(host_version)
                .map_err(|_| invalid("min_host_version", format!("{app} has an invalid host version `{host_version}`")))?;
            if current.cmp_precedence(&required).is_lt() {
                return Err(invalid(
                    "min_host_version",
                    format!("needs {app} {minimum} or newer; this version of {app} is {host_version}; update {app}"),
                ));
            }
        }
        if let Some(api) = value.get("runtime").and_then(|runtime| runtime.get("api")) {
            let api = api.as_str().map(|api| api.chars().take(64).collect::<String>()).unwrap_or_else(|| "an older plugin API".into());
            return Err(HostError::UnsupportedCapability {
                capability: api.clone(),
                reason: format!("was built for {api}; this version of {app} runs {API} plugins (capabilities [\"commands@0.2\"]); update the plugin"),
            });
        }
        // A newer capability may introduce new manifest keys. Report that capability first,
        // before strict deserialization would misdiagnose it as an unknown field.
        if let Some(capabilities) = value.get("capabilities")
            && let Ok(requested) = serde_json::from_value::<Vec<String>>(capabilities.clone())
        {
            check_capabilities(&requested, app)?;
        }
    }
    // Shared field definitions are inlined first; a manifest without them keeps its exact
    // deserialization (and error positions).
    let expanded = match parsed.as_mut().map(schema_refs::expand).transpose()? {
        Some(true) => parsed,
        _ => None,
    };
    let mut manifest: Manifest = match expanded {
        Some(mut value) => {
            schema_order::take_orders(&mut value);
            serde_json::from_value(value)
        }
        None => serde_json::from_slice(text),
    }
    .map_err(|error| invalid("manifest", error.to_string()))?;
    manifest.name = manifest.name.trim().to_string();
    manifest.version = manifest.version.trim().to_string();
    manifest.description = manifest.description.trim().to_string();
    if !valid_id(&manifest.id) {
        return Err(invalid("id", "must match its lowercase package folder"));
    }
    if manifest.id != folder {
        return Err(invalid(
            "id",
            format!(
                "package id `{}` does not match the folder `{folder}`: a package folder is named after its plugin's id, so rename the folder to `{}`",
                manifest.id, manifest.id
            ),
        ));
    }
    if !plain(&manifest.name, 80) {
        return Err(invalid("name", "requires 1..80 printable characters"));
    }
    if !plain(&manifest.version, 32) || !manifest.version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
        return Err(invalid("version", "requires 1..32 alphanumeric, dot, dash or plus characters"));
    }
    if manifest.description.chars().count() > 240 || manifest.description.chars().any(char::is_control) {
        return Err(invalid("description", "requires at most 240 printable characters"));
    }
    check_capability_declarations(&manifest.capabilities, &manifest.optional_capabilities)?;
    check_capabilities(&manifest.capabilities, app)?;
    let granted = manifest.granted_capabilities();
    if let Some(updates) = &manifest.updates
        && !valid_github_repo(&updates.github)
    {
        return Err(invalid("updates.github", "must be a GitHub repository, owner/repo"));
    }
    if let Some(runtime) = &manifest.runtime {
        if runtime.entries.iter().any(|entry| entry.dialog_hook) && !manifest.capabilities.iter().any(|c| c.starts_with("dialog@")) {
            return Err(invalid("capabilities", "a dialog hook needs the dialog capability: dialog@0.1"));
        }
        if runtime.is_importer() || runtime.is_exporter() {
            if !manifest.capabilities.iter().any(|capability| capability.starts_with("formats@")) {
                return Err(invalid("capabilities", "an importer or exporter needs the formats capability: \"capabilities\": [\"formats@0.1\"]"));
            }
            if runtime.is_exporter() && manifest.capabilities.iter().any(|capability| capability.starts_with("evaluate@")) {
                return Err(invalid("capabilities", "evaluate checks an importer's result: only an importer asks for it"));
            }
        } else if granted.iter().any(|capability| capability.starts_with("evaluate@")) {
            return Err(invalid("capabilities", "evaluate checks an importer's result: only an importer (formats) asks for it"));
        } else if !manifest.capabilities.iter().any(|capability| capability.starts_with("commands@")) {
            return Err(invalid("capabilities", "a plugin with a runtime needs the commands capability: \"capabilities\": [\"commands@0.1\"]"));
        }
        runtime.validate()?;
    }
    if manifest.runtime.as_ref().is_some_and(|runtime| !runtime.listeners.is_empty())
        && !granted.iter().any(|capability| capability.starts_with("design@"))
    {
        return Err(invalid("runtime.listeners", "design listeners require capability design@0.1"));
    }
    check_storage(&manifest)?;
    let has_ui = manifest.capabilities.iter().any(|cap| cap.starts_with("ui@"));
    if has_ui && manifest.runtime.as_ref().is_none_or(RuntimeManifest::is_importer) {
        return Err(invalid("capabilities", "ui requires a command plugin runtime"));
    }
    if has_ui && manifest.runtime.as_ref().and_then(|runtime| runtime.ui.as_ref()).is_none() {
        return Err(invalid("runtime.ui", "ui@0.1 requires static panel declarations"));
    }
    if manifest.runtime.as_ref().and_then(|runtime| runtime.ui.as_ref()).is_some() && !has_ui {
        return Err(invalid("runtime.ui", "panel declarations require ui@0.1"));
    }
    if !manifest.shaders.is_empty() {
        if !manifest.capabilities.iter().any(|capability| capability.starts_with("shaders@")) {
            return Err(invalid(
                "capabilities",
                "a plugin with shaders needs the shaders capability: \"capabilities\": [\"shaders@0.2\"] (backgrounds alone may say \"shaders@0.1\")",
            ));
        }
        // Tints, settings and chosen pictures came with 0.2: a 0.1 plugin keeps exactly what 0.1 offered.
        let newer = manifest.shaders.iter().any(|s| {
            s.kind != shader::ShaderKind::Background || !s.settings.is_empty() || s.textures.iter().any(|t| t.choose.is_some()) || s.state.is_some()
        });
        if newer && !manifest.capabilities.iter().any(|capability| capability == "shaders@0.2") {
            return Err(invalid(
                "capabilities",
                "tints, shader settings, chosen pictures and state grids need \"shaders@0.2\"; update SolidCraft or the plugin",
            ));
        }
        shader::validate(&manifest.shaders)?;
    }
    let themed = manifest.capabilities.iter().any(|capability| capability.starts_with("themes@"));
    if !manifest.themes.is_empty() {
        if !themed {
            return Err(invalid("capabilities", "a plugin with themes needs the themes capability: \"capabilities\": [\"themes@0.1\"]"));
        }
        theme::validate(&manifest.themes)?;
    } else if themed {
        return Err(invalid("themes", "the themes capability needs at least one theme"));
    }
    Ok(manifest)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
