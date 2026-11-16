//! Shared plugin contracts. App commands are schemas and JSON, never app-specific types.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// The WIT package declaration is the sole source of the API identifier.
include!(concat!(env!("OUT_DIR"), "/api.rs"));
pub const WIT: &str = include_str!("../wit/plugin.wit");
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
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
/// A command entry finishes within 30 s; an importer converting a large assembly may take minutes.
pub const MAX_COMMAND_DEADLINE_MS: u64 = 30_000;
pub const MAX_IMPORTER_DEADLINE_MS: u64 = 600_000;
/// The most extensions one importer entry claims.
pub const MAX_EXTENSIONS: usize = 32;
/// Memory32 has at most 65536 pages of 64 KiB.
pub const MAX_MEMORY_MIB: usize = 4096;

pub type Result<T> = std::result::Result<T, HostError>;

/// Failures are actionable data at the app boundary, including failures before instantiation.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostError {
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
    #[error("plugin trap: {reason}")]
    Trap { reason: String },
    #[error("plugin cancelled")]
    Cancelled,
    /// The plugin needs a capability, or a version of one, that this host does not serve.
    #[error("{reason}")]
    UnsupportedCapability { capability: String, reason: String },
}

/// The capabilities this host serves, as name and `major.minor`. Each is a set of WIT interfaces
/// a plugin names in its manifest's `capabilities` list. While a major version is 0, a minor
/// version may break the one before it, so 0.x matches only the same minor.
pub const CAPABILITIES: &[(&str, (u32, u32))] = &[("commands", (0, 1)), ("evaluate", (0, 1)), ("formats", (0, 1))];

fn capability_version(text: &str) -> Option<(u32, u32)> {
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
    if requested.len() > 16 {
        return Err(invalid("capabilities", "at most 16 capabilities"));
    }
    let mut names = BTreeSet::new();
    for capability in requested {
        let parsed = capability.split_once('@').and_then(|(name, version)| Some((name, capability_version(version)?)));
        let Some((name, (major, minor))) = parsed.filter(|(name, _)| valid_id(name)) else {
            return Err(invalid("capabilities", format!("`{capability}` is not name@major.minor, e.g. \"commands@0.1\"")));
        };
        if !names.insert(name) {
            return Err(invalid("capabilities", format!("`{name}` is listed twice")));
        }
        let unsupported = |reason: String| HostError::UnsupportedCapability { capability: capability.clone(), reason };
        let Some((_, (have_major, have_minor))) = CAPABILITIES.iter().find(|(known, _)| *known == name) else {
            return Err(unsupported(format!("needs {name} {major}.{minor}, which this version of {app} does not have; update {app} or the plugin")));
        };
        let (have_major, have_minor) = (*have_major, *have_minor);
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

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> HostError {
    HostError::InvalidManifest { field: field.into(), reason: reason.into() }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Read,
    #[default]
    Edit,
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

/// A named folder resolved by the app after user approval, never a guest-supplied root.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileGrant {
    pub name: String,
    pub folder: PathBuf,
    pub read: bool,
    pub write: bool,
}

/// A runtime brackets a whole plugin call in one transaction. Guest failures roll it back.
/// The app must retain its original state until commit, including its undo history.
/// Granted file writes are staged until the guest succeeds and the app commits; rollback
/// removes staged files without replacing their targets. Publication errors after app
/// commit can leave a subset of files applied and must be reported to the caller.
pub trait PluginHost {
    fn command_catalog(&self) -> Vec<CommandSchema>;
    fn run_command(&mut self, name: &str, args: &Value) -> Result<Value>;
    fn begin_transaction(&mut self) -> Result<()>;
    fn commit_transaction(&mut self) -> Result<()>;
    fn rollback_transaction(&mut self) -> Result<()>;
    fn granted_folders(&self) -> Vec<FileGrant>;
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
    fn progress(&mut self, fraction: f32, message: &str);
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
}

/// User-owned ceilings, separate from the package's exact content-approved request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HostLimits {
    pub max_fuel: u64,
    pub max_memory_mib: usize,
}
impl Default for HostLimits {
    fn default() -> Self {
        Self { max_fuel: 100_000_000_000, max_memory_mib: 128 }
    }
}
impl HostLimits {
    pub fn validate(&self) -> Result<()> {
        if self.max_fuel == 0 || !(1..=MAX_MEMORY_MIB).contains(&self.max_memory_mib) {
            return Err(invalid("host_limits", "requires positive max_fuel and max_memory_mib 1..4096 (Wasm32)"));
        }
        Ok(())
    }
    pub fn check(&self, requested: &Limits) -> Result<()> {
        self.validate()?;
        requested.validate()?;
        if requested.fuel > self.max_fuel {
            return Err(HostError::LimitExceeded { limit: "fuel exceeds user host limits".into() });
        }
        if requested.memory_mib > self.max_memory_mib {
            return Err(HostError::LimitExceeded { limit: "memory exceeds user host limits".into() });
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
    /// The guest stack in KiB, when the default is too small (a real decoder needed about 16 MiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_kib: Option<u32>,
}
impl Default for Limits {
    fn default() -> Self {
        Self { fuel: 10_000_000, memory_mib: 64, deadline_ms: 5000, stack_kib: None }
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
        if !(1..=MAX_IMPORTER_DEADLINE_MS).contains(&self.deadline_ms) {
            return Err(invalid("runtime.limits.deadline_ms", "must be 1..600000"));
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
    Menu { name: String },
    Ribbon { name: String },
}

fn empty_input_schema() -> Value {
    serde_json::json!({"type":"object","properties":{}})
}

/// What an entry is: a command the plugin runs on the open design, or an importer that turns a
/// file of a claimed extension into a new design.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    Command,
    Importer,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(from = "EntryDeclaration")]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    /// The file extensions an importer claims: lowercase, without the dot. Empty for a command.
    pub extensions: Vec<String>,
    pub effect: Effect,
    pub input_schema: Value,
    pub slots: Vec<UiSlot>,
}

impl Entry {
    /// Whether this importer claims `path`'s extension (ignoring case).
    pub fn claims(&self, path: &Path) -> bool {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default();
        self.kind == EntryKind::Importer && !extension.is_empty() && self.extensions.iter().any(|e| e.eq_ignore_ascii_case(extension))
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
    effect: Effect,
    #[serde(default = "empty_input_schema")]
    input_schema: Value,
    #[serde(default)]
    slots: Vec<UiSlot>,
}
impl From<EntryDeclaration> for Entry {
    fn from(value: EntryDeclaration) -> Self {
        match value {
            EntryDeclaration::Legacy(name) => Self {
                name,
                kind: EntryKind::Command,
                extensions: Vec::new(),
                effect: Effect::Edit,
                input_schema: empty_input_schema(),
                slots: Vec::new(),
            },
            EntryDeclaration::Declared(value) => Self {
                name: value.name,
                kind: value.kind,
                extensions: value.extensions,
                effect: value.effect,
                input_schema: value.input_schema,
                slots: value.slots,
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
    #[serde(default)]
    pub description: String,
    /// The host capabilities the plugin needs, as `name@major.minor` (`commands@0.1`). A runtime
    /// needs `commands`. The host refuses a capability it does not serve, before any code runs.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    /// A single file name within the package, never a directory or absolute path.
    pub module: String,
    pub entries: Vec<Entry>,
    /// Exact app command ids, intersected with the app catalog by the runtime.
    pub commands: BTreeSet<String>,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub permissions: Permissions,
}
impl RuntimeManifest {
    /// Whether this runtime is an importer (world `importer-plugin`). A runtime's entries are all
    /// commands or all importers.
    pub fn is_importer(&self) -> bool {
        self.entries.first().is_some_and(|e| e.kind == EntryKind::Importer)
    }
    /// The largest component this runtime may ship.
    pub fn max_component_bytes(&self) -> usize {
        if self.is_importer() { MAX_IMPORTER_COMPONENT_BYTES } else { MAX_COMPONENT_BYTES }
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
            return Err(invalid("runtime.entries", "requires 1..32 distinct ids"));
        }
        let mut names = BTreeSet::new();
        for entry in &self.entries {
            if !valid_id(&entry.name) || !names.insert(&entry.name) {
                return Err(invalid("runtime.entries", "requires 1..32 distinct ids"));
            }
            validate_kind(self, entry)?;
            let field = format!("runtime.entries.{}.input_schema", entry.name);
            if !entry.input_schema.is_object() || entry.input_schema.get("type").and_then(Value::as_str) != Some("object") {
                return Err(invalid(&field, "entry inputs must be an object schema"));
            }
            validate_list_inputs(&entry.input_schema, &field)?;
            if entry.slots.len() > 16
                || entry.slots.iter().any(|slot| match slot {
                    UiSlot::Menu { name } | UiSlot::Ribbon { name } => !plain(name, 80),
                })
            {
                return Err(invalid("runtime.entries.slots", "requires at most 16 named menu or ribbon slots"));
            }
        }
        if self.commands.len() > 128 || self.commands.iter().any(|id| !valid_command_name(id)) {
            return Err(invalid("runtime.commands", "requires at most 128 exact command names"));
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
        } else {
            if self.permissions.input_folder {
                return Err(invalid("runtime.permissions.input_folder", "only an importer reads the folder of the file it imports"));
            }
            if self.limits.deadline_ms > MAX_COMMAND_DEADLINE_MS {
                return Err(invalid("runtime.limits.deadline_ms", "must be 1..30000"));
            }
        }
        Ok(())
    }
}

/// An importer entry claims 1..32 distinct lowercase extensions and edits (it makes a design);
/// a command claims none. One runtime does not mix the two: they are different worlds.
fn validate_kind(runtime: &RuntimeManifest, entry: &Entry) -> Result<()> {
    let field = format!("runtime.entries.{}", entry.name);
    if runtime.entries.iter().any(|other| other.kind != entry.kind) {
        return Err(invalid("runtime.entries", "entries are all commands or all importers; ship an importer as a plugin of its own"));
    }
    match entry.kind {
        EntryKind::Command if !entry.extensions.is_empty() => Err(invalid(format!("{field}.extensions"), "only an importer claims file extensions")),
        EntryKind::Command => Ok(()),
        EntryKind::Importer => {
            let mut seen = BTreeSet::new();
            let valid = |e: &String| (1..=16).contains(&e.len()) && e.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            if entry.extensions.is_empty() || entry.extensions.len() > MAX_EXTENSIONS || !entry.extensions.iter().all(|e| valid(e) && seen.insert(e))
            {
                return Err(invalid(
                    format!("{field}.extensions"),
                    "an importer claims 1..32 distinct lowercase extensions without the dot, such as \"f3d\"",
                ));
            }
            if entry.effect != Effect::Edit {
                return Err(invalid(format!("{field}.effect"), "an importer makes a design: its effect is edit"));
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

pub fn parse_manifest(text: &[u8], folder: &str) -> Result<Manifest> {
    parse_manifest_for(text, folder, "the app")
}

/// [`parse_manifest`], naming the host `app` in a capability refusal.
pub fn parse_manifest_for(text: &[u8], folder: &str, app: &str) -> Result<Manifest> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(HostError::LimitExceeded { limit: "manifest size exceeds 16 KiB".into() });
    }
    // A plugin from before capabilities names its API in `runtime.api`: say so plainly.
    if let Ok(value) = serde_json::from_slice::<Value>(text)
        && let Some(api) = value.get("runtime").and_then(|runtime| runtime.get("api"))
    {
        let api = api.as_str().map(|api| api.chars().take(64).collect::<String>()).unwrap_or_else(|| "an older plugin API".into());
        return Err(HostError::UnsupportedCapability {
            capability: api.clone(),
            reason: format!("was built for {api}; this version of {app} runs {API} plugins (capabilities [\"commands@0.1\"]); update the plugin"),
        });
    }
    let mut manifest: Manifest = serde_json::from_slice(text).map_err(|error| invalid("manifest", error.to_string()))?;
    manifest.name = manifest.name.trim().to_string();
    manifest.version = manifest.version.trim().to_string();
    manifest.description = manifest.description.trim().to_string();
    if !valid_id(&manifest.id) {
        return Err(invalid("id", "must match its lowercase package folder"));
    }
    if manifest.id != folder {
        return Err(invalid("id", format!("package id `{}` does not match the folder `{folder}`", manifest.id)));
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
    check_capabilities(&manifest.capabilities, app)?;
    if let Some(runtime) = &manifest.runtime {
        if runtime.is_importer() {
            if !manifest.capabilities.iter().any(|capability| capability.starts_with("formats@")) {
                return Err(invalid("capabilities", "an importer needs the formats capability: \"capabilities\": [\"formats@0.1\"]"));
            }
        } else if manifest.capabilities.iter().any(|capability| capability.starts_with("evaluate@")) {
            return Err(invalid("capabilities", "evaluate checks an importer's result: only an importer (formats) asks for it"));
        } else if !manifest.capabilities.iter().any(|capability| capability.starts_with("commands@")) {
            return Err(invalid("capabilities", "a plugin with a runtime needs the commands capability: \"capabilities\": [\"commands@0.1\"]"));
        }
        runtime.validate()?;
    }
    Ok(manifest)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
