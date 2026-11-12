//! SolidCraft policy and transactions for the shared Craft plugin host.

use crate::{EngineError, Result, Session};
pub use craft_plugin::{
    API, Effect, Entry, FileGrant, HostLimits, Limits, MAX_COMPONENT_BYTES, MAX_JSON_BYTES, MAX_MANIFEST_BYTES, MAX_MEMORY_MIB, Manifest,
    RuntimeManifest, valid_id,
};
use craft_plugin::{CommandSchema, HostError, PluginHost};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

static PROCESS_HOST_LIMITS: std::sync::OnceLock<HostLimits> = std::sync::OnceLock::new();
pub fn configure_process_host_limits(limits: HostLimits) -> Result<()> {
    limits.validate()?;
    PROCESS_HOST_LIMITS.set(limits).map_err(|_| error("process plugin host limits are already configured"))
}
pub(crate) fn default_host_limits() -> HostLimits {
    PROCESS_HOST_LIMITS.get().copied().unwrap_or_default()
}
fn error(s: impl Into<String>) -> EngineError {
    EngineError::Other(format!("plugin: {}", s.into()))
}

/// Trusted host state, never loaded from a design or supplied by guest code.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppState {
    pub packages: BTreeMap<String, String>,
    pub file_grants: Vec<FileGrant>,
}

pub fn parse_manifest(text: &[u8], folder: &str) -> Result<Manifest> {
    let manifest = craft_plugin::parse_manifest(text, folder)?;
    if manifest.runtime.as_ref().is_some_and(|r| r.commands.iter().any(|c| !allowed_command(c))) {
        return Err(HostError::InvalidManifest {
            field: "runtime.commands".into(),
            reason: "commands include an unavailable or forbidden built-in command".into(),
        }
        .into());
    }
    Ok(manifest)
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
    if !rt.entries.iter().any(|e| e.name == entry) {
        return Err(error("unknown entry"));
    }
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let (host, result) = craft_plugin_wasm::run(
            SolidCraftHost { session: session.scratch(), transaction: None },
            &package.manifest,
            &package.component,
            entry,
            params,
        );
        let value = result?;
        if !std::sync::Arc::ptr_eq(&session.doc, &host.session.doc)
            || session.active_sketch != host.session.active_sketch
            || session.pending_moves != host.session.pending_moves
        {
            session.accept_computed(host.session);
        } else {
            for message in host.session.log {
                session.echo(message);
            }
        }
        Ok(value)
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = params;
        Err(error("plugin execution requires an x86-64 or AArch64 native build with the plugins feature"))
    }
}

#[cfg(test)]
mod tests;

/// The application adapter owns a scratch transaction. Wasmtime sees only PluginHost.
pub struct SolidCraftHost {
    pub session: Session,
    transaction: Option<Session>,
}
impl PluginHost for SolidCraftHost {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        crate::command_specs()
            .iter()
            .filter(|c| allowed_command(c.id))
            .map(|c| CommandSchema {
                name: c.id.into(),
                input_schema: c.contract.map(|c| c.input_schema().to_value()).unwrap_or_else(|| json!({"type":"object"})),
                output_schema: c.output.map(|f| f().to_value()).unwrap_or(json!({})),
                effect: if c.undoable { Effect::Edit } else { Effect::Read },
            })
            .collect()
    }
    fn run_command(&mut self, name: &str, params: &Value) -> craft_plugin::Result<Value> {
        if !allowed_command(name) {
            return Err(HostError::PermissionDenied { permission: format!("command {name} is not granted") });
        }
        let s = self.transaction.as_mut().ok_or_else(|| HostError::CommandFailed { command: name.into(), reason: "no active transaction".into() })?;
        let before = s.doc.clone();
        let previous: BTreeMap<_, _> = s.model.results.iter().map(|r| (r.id, r.error.clone())).collect();
        // Even implicit font and linked-design reads have no authority from a command grant.
        let reply = solidcraft_io::vfs::scoped(Some(Default::default()), || s.execute(name, params))
            .map_err(|e| HostError::CommandFailed { command: name.into(), reason: e.to_string() })?;
        if let Some(failed) = s
            .model
            .results
            .iter()
            .find(|r| r.error.is_some() && (previous.get(&r.id) != Some(&r.error) || before.feature(r.id) != s.doc.feature(r.id)))
        {
            return Err(HostError::CommandFailed { command: name.into(), reason: failed.error.clone().unwrap_or_default() });
        }
        Ok(reply)
    }
    fn begin_transaction(&mut self) -> craft_plugin::Result<()> {
        if self.transaction.is_some() {
            return Err(HostError::InvalidInput { field: "transaction".into(), reason: "already active".into() });
        }
        self.transaction = Some(self.session.scratch());
        Ok(())
    }
    fn commit_transaction(&mut self) -> craft_plugin::Result<()> {
        let mut computed =
            self.transaction.take().ok_or_else(|| HostError::InvalidInput { field: "transaction".into(), reason: "not active".into() })?;
        computed.undo.clear();
        computed.redo.clear();
        // Session::execute(plugin.run) owns the one outer undo step.
        if !std::sync::Arc::ptr_eq(&self.session.doc, &computed.doc)
            || self.session.active_sketch != computed.active_sketch
            || self.session.pending_moves != computed.pending_moves
        {
            self.session.accept_computed(computed);
        } else {
            for message in computed.log {
                self.session.echo(message);
            }
        }
        Ok(())
    }
    fn rollback_transaction(&mut self) -> craft_plugin::Result<()> {
        self.transaction = None;
        Ok(())
    }
    fn granted_folders(&self) -> Vec<FileGrant> {
        let scope = solidcraft_io::vfs::current_scope();
        self.session
            .plugin_state
            .file_grants
            .iter()
            .filter(|g| scope.as_ref().is_none_or(|s| s.check(&g.folder.to_string_lossy()).is_ok()))
            .cloned()
            .collect()
    }
    fn log(&mut self, message: &str) {
        if let Some(s) = self.transaction.as_mut() {
            s.echo(format!("plugin: {}", message.chars().filter(|c| !c.is_control()).collect::<String>()));
        }
    }
    fn progress(&mut self, fraction: f32, message: &str) {
        solidcraft_doc::progress::item(Some((message, (fraction.clamp(0., 1.) * 100.) as usize, 100)));
    }
    fn is_cancelled(&self) -> bool {
        solidcraft_doc::progress::cancelled()
    }
}

/// Resolve an inspected entry through the same approved plugin.run transaction.
/// Unknown packages remain unknown commands; inspecting a package never approves it.
pub fn resolve_call(s: &Session, id: &str, params: &Value) -> Result<Option<Value>> {
    let Some((package, entry)) = id.split_once('.') else { return Ok(None) };
    let Some(path) = s.plugin_state.packages.get(package) else { return Ok(None) };
    if crate::find_command(id).is_some() {
        return Ok(None);
    }
    if !valid_id(entry) {
        return Err(EngineError::UnknownCommand(id.into()));
    }
    let approval =
        params.get("approval").and_then(Value::as_str).ok_or_else(|| crate::params::bad(id, "approval from plugin.inspect is required"))?;
    Ok(Some(json!({"path":path,"approval":approval,"entry":entry,"params":params.get("params").cloned().unwrap_or(json!({}))})))
}

pub fn describe_entry(s: &Session, id: &str) -> Result<Option<Value>> {
    if crate::find_command(id).is_some() {
        return Ok(None);
    }
    let Some((package, name)) = id.split_once('.') else { return Ok(None) };
    let Some(path) = s.plugin_state.packages.get(package) else { return Ok(None) };
    let package = read_package(path)?;
    let Some(runtime) = package.manifest.runtime else { return Ok(None) };
    let Some(entry) = runtime.entries.iter().find(|e| e.name == name) else { return Ok(None) };
    Ok(Some(json!({"id":id,"label":name,"params":"approval, params: declared entry inputs",
        "input":{"type":"object","required":["approval","params"],"additionalProperties":false,"properties":{
            "approval":{"type":"string"},"params":entry.input_schema}},
        "output":{},"effect":entry.effect,"slots":entry.slots})))
}
