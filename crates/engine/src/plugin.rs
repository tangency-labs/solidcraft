//! Explicit, content-pinned one-shot plugins. The design keeps ordinary native features.

use std::collections::BTreeSet;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{EngineError, Result, Session};

#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod runtime;

pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_JSON_BYTES: usize = 1024 * 1024;
pub const API: &str = "solidcraft:plugin@0.1.0";
/// Memory32 can address at most 65536 pages of 64 KiB; the runtime allows one memory.
pub const MAX_MEMORY_MIB: usize = 4096;

/// User-owned ceilings, separate from a package's exact, content-approved request.
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
            return Err(error("host limits require positive integer max_fuel and max_memory_mib 1..4096 (Wasm32)"));
        }
        Ok(())
    }
    pub fn check(&self, requested: &Limits) -> Result<()> {
        self.validate()?;
        requested.validate()?;
        if requested.fuel > self.max_fuel || requested.memory_mib > self.max_memory_mib {
            return Err(error(format!(
                "requested fuel {} / memory {} MiB exceeds user host limits {} / {} MiB; explicitly change the host settings and review the package before running",
                requested.fuel, requested.memory_mib, self.max_fuel, self.max_memory_mib
            )));
        }
        Ok(())
    }
}
static PROCESS_HOST_LIMITS: std::sync::OnceLock<HostLimits> = std::sync::OnceLock::new();
/// Configure trusted host startup once, before creating CLI/headless sessions. Not a guest command.
pub fn configure_process_host_limits(limits: HostLimits) -> Result<()> {
    limits.validate()?;
    PROCESS_HOST_LIMITS.set(limits).map_err(|_| error("process plugin host limits are already configured"))
}
pub(crate) fn default_host_limits() -> HostLimits {
    PROCESS_HOST_LIMITS.get().copied().unwrap_or_default()
}

/// Metadata is inert. A runtime declaration still needs explicit approval to execute.
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

/// The initial command-plugin world: no WASI, files, network or process imports.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeManifest {
    pub api: String,
    /// A single file name within the package, never an absolute or relative directory.
    pub module: String,
    pub entries: Vec<String>,
    /// Exact built-in command ids. No patterns, aliases or plugin-to-plugin calls.
    pub commands: BTreeSet<String>,
    #[serde(default)]
    pub limits: Limits,
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
    fn validate(&self) -> Result<()> {
        if self.fuel == 0 || !(1..=MAX_MEMORY_MIB).contains(&self.memory_mib) || !(1..=30_000).contains(&self.deadline_ms) {
            return Err(error("limits require positive fuel, memory 1..4096 MiB (Wasm32) and deadline 1..30000 ms"));
        }
        Ok(())
    }
}

pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=64).contains(&b.len())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b.first() != Some(&b'-')
        && b.last() != Some(&b'-')
}
fn plain(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}
fn error(s: impl Into<String>) -> EngineError {
    EngineError::Other(format!("plugin: {}", s.into()))
}

pub fn parse_manifest(text: &[u8], folder: &str) -> Result<Manifest> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(error("manifest exceeds 16 KiB"));
    }
    let mut m: Manifest = serde_json::from_slice(text).map_err(|e| error(format!("invalid manifest: {e}")))?;
    // Keep the metadata page's existing whitespace normalization.
    m.name = m.name.trim().to_string();
    m.version = m.version.trim().to_string();
    m.description = m.description.trim().to_string();
    if !valid_id(&m.id) || m.id != folder {
        return Err(error("manifest id does not match its lowercase package folder"));
    }
    if !plain(&m.name, 80)
        || !plain(&m.version, 32)
        || !m.version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        || m.description.chars().count() > 240
        || m.description.chars().any(char::is_control)
    {
        return Err(error("invalid name, version or description"));
    }
    if let Some(runtime) = &m.runtime {
        if runtime.api != API {
            return Err(error("unsupported plugin API"));
        }
        if !valid_id(runtime.module.strip_suffix(".wasm").unwrap_or("")) {
            return Err(error("module must be one lowercase .wasm file name"));
        }
        if runtime.entries.is_empty()
            || runtime.entries.len() > 32
            || runtime.entries.iter().any(|e| !valid_id(e))
            || runtime.entries.iter().collect::<BTreeSet<_>>().len() != runtime.entries.len()
        {
            return Err(error("entries must be 1..32 distinct ids"));
        }
        if runtime.commands.len() > 128 || runtime.commands.iter().any(|id| !allowed_command(id)) {
            return Err(error("commands include an unavailable or forbidden built-in command"));
        }
        runtime.limits.validate()?;
    }
    Ok(m)
}

/// Positive host policy, intersected with the manifest. New command families require review.
pub fn allowed_command(id: &str) -> bool {
    let family = id.starts_with("solid.") || id.starts_with("sketch.");
    let explicit = matches!(
        id,
        "parameters.add"
            | "parameters.change"
            | "parameters.delete"
            | "timeline.add"
            | "timeline.inspect"
            | "timeline.redefine"
            | "timeline.delete"
            | "document.inspect"
            | "model.faces"
            | "model.edges"
            | "model.mesh"
            | "inspect.measure"
    );
    (family || explicit) && !id.starts_with("sketch.insert_") && crate::find_command(id).is_some()
}

pub struct Package {
    pub manifest: Manifest,
    pub approval: String,
    pub component: Vec<u8>,
}

/// Reads through the caller's existing file scope; no permissions are added here.
pub fn read_package(path: &str) -> Result<Package> {
    let dir = Path::new(path);
    let folder = dir.file_name().and_then(|s| s.to_str()).ok_or_else(|| error("package needs an id folder"))?;
    let manifest_path = dir.join("plugin.json");
    let bytes = solidcraft_io::vfs::read(&manifest_path.to_string_lossy(), MAX_MANIFEST_BYTES).map_err(|e| error(e.to_string()))?;
    let manifest = parse_manifest(&bytes, folder)?;
    let runtime = manifest.runtime.as_ref().ok_or_else(|| error("metadata-only package has no executable runtime"))?;
    let module_path = dir.join(&runtime.module);
    let component = solidcraft_io::vfs::read(&module_path.to_string_lossy(), MAX_COMPONENT_BYTES).map_err(|e| error(e.to_string()))?;
    let mut digest = Sha256::new();
    digest.update(b"solidcraft-plugin-approval-v1\0");
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(&bytes);
    digest.update(&component);
    Ok(Package { manifest, approval: format!("{:x}", digest.finalize()), component })
}

pub fn run(session: &mut Session, package: Package, entry: &str, params: &serde_json::Value) -> Result<serde_json::Value> {
    if session.disabled_plugins.contains(&package.manifest.id) {
        return Err(error("plugin is disabled"));
    }
    let rt = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    session.plugin_limits.check(&rt.limits)?;
    if !rt.entries.iter().any(|e| e == entry) {
        return Err(error("unknown entry"));
    }
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        runtime::run(session, &package, entry, params)
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = params;
        Err(error("plugin execution requires an x86-64 or AArch64 native build with the plugins feature"))
    }
}

#[cfg(test)]
mod tests;
