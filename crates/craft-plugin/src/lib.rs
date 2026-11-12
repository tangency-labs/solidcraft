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
}

/// A named folder resolved by the app after user approval, never a guest-supplied root.
#[derive(Clone, Debug, PartialEq, Eq)]
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
}
impl Default for Limits {
    fn default() -> Self {
        Self { fuel: 10_000_000, memory_mib: 64, deadline_ms: 5000 }
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
        if !(1..=30_000).contains(&self.deadline_ms) {
            return Err(invalid("runtime.limits.deadline_ms", "must be 1..30000"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    pub files: Vec<FilePermission>,
    pub clock: bool,
    /// Reserved: refused in this API regardless of app grants.
    pub net: bool,
}
impl Permissions {
    pub fn validate(&self) -> Result<()> {
        if self.net {
            return Err(HostError::PermissionDenied { permission: "net".into() });
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

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(from = "EntryDeclaration")]
pub struct Entry {
    pub name: String,
    pub effect: Effect,
    pub input_schema: Value,
    pub slots: Vec<UiSlot>,
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
    effect: Effect,
    #[serde(default = "empty_input_schema")]
    input_schema: Value,
    #[serde(default)]
    slots: Vec<UiSlot>,
}
impl From<EntryDeclaration> for Entry {
    fn from(value: EntryDeclaration) -> Self {
        match value {
            EntryDeclaration::Legacy(name) => Self { name, effect: Effect::Edit, input_schema: empty_input_schema(), slots: Vec::new() },
            EntryDeclaration::Declared(value) => {
                Self { name: value.name, effect: value.effect, input_schema: value.input_schema, slots: value.slots }
            }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub api: String,
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
    pub fn validate(&self) -> Result<()> {
        if self.api != API {
            return Err(invalid("runtime.api", "unsupported plugin API"));
        }
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
        self.limits.validate()
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
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(HostError::LimitExceeded { limit: "manifest size exceeds 16 KiB".into() });
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
    if let Some(runtime) = &manifest.runtime {
        runtime.validate()?;
    }
    Ok(manifest)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
