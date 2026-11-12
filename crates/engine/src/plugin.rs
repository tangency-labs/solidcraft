//! SolidCraft policy and transactions for the shared Craft plugin host.

use crate::{EngineError, Result, Session};
pub use craft_plugin::{
    API, Effect, Entry, FileGrant, HostLimits, Limits, MAX_COMPONENT_BYTES, MAX_JSON_BYTES, MAX_MANIFEST_BYTES, MAX_MEMORY_MIB, Manifest,
    RuntimeManifest, valid_id,
};
use craft_plugin::{CommandSchema, HostError, PluginHost};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
    /// Sensitive command groups ([`COMMAND_GROUPS`]) the user granted. A plugin calls a grouped
    /// command only when its manifest asks for the group too.
    pub command_groups: BTreeSet<String>,
    /// Where `plugin.install` writes and `plugin.list` looks; `None`: the per-user
    /// `plugins` folder ([`crate::recovery::data_dir`]).
    pub plugins_dir: Option<PathBuf>,
}

impl AppState {
    /// The installed plugins' folder, if this platform has one.
    pub fn plugins_dir(&self) -> Option<PathBuf> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        self.plugins_dir.clone().or_else(|| crate::recovery::data_dir().map(|p| p.join("plugins")))
    }

    /// Discovery must not replace a package path explicitly selected by inspection.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn register_discovered(&mut self, id: String, path: String) {
        self.packages.entry(id).or_insert(path);
    }
}

pub fn parse_manifest(text: &[u8], folder: &str) -> Result<Manifest> {
    let manifest = craft_plugin::parse_manifest_for(text, folder, "SolidCraft")?;
    let refuse = |field: &str, reason: String| -> EngineError { HostError::InvalidManifest { field: field.into(), reason }.into() };
    if let Some(runtime) = &manifest.runtime {
        if let Some(group) = runtime.permissions.command_groups.iter().find(|g| !COMMAND_GROUPS.contains(&g.as_str())) {
            return Err(refuse(
                "runtime.permissions.command_groups",
                format!("unknown command group `{group}`; SolidCraft has {}", COMMAND_GROUPS.join(", ")),
            ));
        }
        for command in &runtime.commands {
            if catalog_command(command).is_none() {
                return Err(refuse(
                    "runtime.commands",
                    format!("uses `{command}`, which this version of SolidCraft does not have; update SolidCraft or the plugin"),
                ));
            }
            if let Some(group) = command_group(command)
                && !runtime.permissions.command_groups.contains(group)
            {
                return Err(refuse("runtime.commands", format!("`{command}` needs \"{group}\" in runtime.permissions.command_groups")));
            }
        }
    }
    Ok(manifest)
}

/// The sensitive command groups. Each reaches past the open design and is refused unless the
/// manifest asks for it and the user grants it, as file folders are:
/// - `file`: reads or writes files (save, export, import, insert from a file, libraries);
/// - `doc`: opens, creates, recovers or switches designs;
/// - `app`: app settings, the window and diagnostics, outside any design;
/// - `plugin`: inspects or runs plugins.
pub const COMMAND_GROUPS: &[&str] = &["app", "doc", "file", "plugin"];

/// Commands outside the `file.` family that read or write files.
const FILE_COMMANDS: &[&str] = &[
    "canvas.insert",
    "component.insert_design",
    "motion.export",
    "motion.export_animation",
    "parameters.export",
    "parameters.import",
    "parts.insert",
    "pointcloud.insert",
    "pointcloud.reload",
    "render.export",
    "sheet.export_bend_table",
    "sheet.export_dxf",
    "sketch.export_dxf",
    "timeline.export_script",
];

/// The sensitive group of a command, or `None` when it acts on the open design alone.
pub fn command_group(id: &str) -> Option<&'static str> {
    let family = id.split_once('.').map_or(id, |(family, _)| family);
    match family {
        "plugin" => Some("plugin"),
        "doc" => Some("doc"),
        "file" | "library" => Some("file"),
        "settings" | "ui" | "diagnostics" => Some("app"),
        _ if FILE_COMMANDS.contains(&id) || id.starts_with("sketch.insert_") => Some("file"),
        _ => None,
    }
}

/// A command of the MCP catalog: every command that is built, by its exact id.
fn catalog_command(id: &str) -> Option<&'static crate::CommandSpec> {
    crate::command_specs().into_iter().find(|c| c.id == id && c.planned.is_none())
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
    let component = if let Some(runtime) = &manifest.runtime {
        let module_path = dir.join(&runtime.module);
        solidcraft_io::vfs::read(&module_path.to_string_lossy(), MAX_COMPONENT_BYTES).map_err(|e| error(e.to_string()))?
    } else {
        Vec::new()
    };
    let mut digest = Sha256::new();
    digest.update(b"solidcraft-plugin-approval-v1\0");
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(&bytes);
    digest.update(&component);
    Ok(Package { manifest, approval: format!("{:x}", digest.finalize()), component })
}

pub fn run(session: &mut Session, package: Package, entry: &str, params: &serde_json::Value) -> Result<crate::schema::plugin::RunResult> {
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
        // What this call may reach: the groups both the manifest asks for and the user granted,
        // and for `file` commands the granted folders the manifest may both read and write.
        let groups: BTreeSet<String> = rt.permissions.command_groups.intersection(&session.plugin_state.command_groups).cloned().collect();
        let host = SolidCraftHost { session: session.scratch(), transaction: None, sketch_exits: Vec::new(), groups, file_folders: Vec::new() };
        let file_folders = host
            .granted_folders()
            .into_iter()
            .filter(|g| g.read && g.write && rt.permissions.files.iter().any(|r| r.name == g.name && r.read && r.write))
            .map(|g| g.folder)
            .collect();
        let (host, result) = craft_plugin_wasm::run(SolidCraftHost { file_folders, ..host }, &package.manifest, &package.component, entry, params);
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
        Ok(crate::schema::plugin::RunResult { result: value, sketch_exits: host.sketch_exits, param_warnings: vec![] })
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = params;
        Err(error("plugin execution requires an x86-64 or AArch64 native build with the plugins feature"))
    }
}

pub mod install;
#[cfg(test)]
mod tests;

/// The application adapter owns a scratch transaction. Wasmtime sees only PluginHost.
pub struct SolidCraftHost {
    pub session: Session,
    transaction: Option<Session>,
    sketch_exits: Vec<Option<u64>>,
    /// Granted sensitive command groups, for this call's manifest.
    groups: BTreeSet<String>,
    /// The file scope of a granted `file` command; every other command has none.
    file_folders: Vec<PathBuf>,
}
impl PluginHost for SolidCraftHost {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        crate::command_specs()
            .iter()
            .filter(|c| c.planned.is_none())
            .map(|c| CommandSchema {
                name: c.id.into(),
                input_schema: c.contract.map(|c| c.input_schema().to_value()).unwrap_or_else(|| json!({"type":"object"})),
                output_schema: c.output.map(|f| f().to_value()).unwrap_or(json!({})),
                effect: c.effect,
                group: command_group(c.id).map(String::from),
            })
            .collect()
    }
    fn run_command(&mut self, name: &str, params: &Value) -> craft_plugin::Result<Value> {
        if catalog_command(name).is_none() {
            return Err(HostError::PermissionDenied { permission: format!("command {name} is not granted") });
        }
        let group = command_group(name);
        if let Some(group) = group
            && !self.groups.contains(group)
        {
            return Err(HostError::PermissionDenied { permission: format!("command {name} needs the `{group}` permission, which is not granted") });
        }
        // Only a granted `file` or `doc` command reaches files, and only the folders granted for it.
        let scope = match group {
            Some("file" | "doc") => solidcraft_io::vfs::PathScope::new(self.file_folders.iter().cloned()),
            _ => Default::default(),
        };
        let s = self.transaction.as_mut().ok_or_else(|| HostError::CommandFailed { command: name.into(), reason: "no active transaction".into() })?;
        let before = s.doc.clone();
        let previous: BTreeMap<_, _> = s.model.results.iter().map(|r| (r.id, r.error.clone())).collect();
        // Even implicit font and linked-design reads have no authority from a command grant.
        let reply = solidcraft_io::vfs::scoped(Some(scope), || s.execute(name, params))
            .map_err(|e| HostError::CommandFailed { command: name.into(), reason: e.to_string() })?;
        if let Some(failed) = s
            .model
            .results
            .iter()
            .find(|r| r.error.is_some() && (previous.get(&r.id) != Some(&r.error) || before.feature(r.id) != s.doc.feature(r.id)))
        {
            return Err(HostError::CommandFailed { command: name.into(), reason: failed.error.clone().unwrap_or_default() });
        }
        if matches!(name, "sketch.finish" | "sketch.cancel") {
            self.sketch_exits.push(reply.get("sketch").and_then(Value::as_u64));
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
        self.sketch_exits.clear();
        Ok(())
    }
    fn granted_command_groups(&self) -> BTreeSet<String> {
        self.groups.clone()
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
    fn authorize_file_path(&self, path: &Path, write: bool) -> craft_plugin::Result<()> {
        if let Some(scope) = solidcraft_io::vfs::current_scope() {
            return scope.check(&path.to_string_lossy()).map_err(|e| HostError::PermissionDenied { permission: e.to_string() });
        }
        // The approved root and its ancestors are explicit, even in AppData or a hidden
        // folder. Inspect only names beneath the most specific grant for this operation.
        let relative = self
            .session
            .plugin_state
            .file_grants
            .iter()
            .filter(|grant| if write { grant.write } else { grant.read })
            .filter_map(|grant| path.strip_prefix(&grant.folder).ok().map(|relative| (grant.folder.components().count(), relative)))
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, relative)| relative)
            .ok_or_else(|| HostError::PermissionDenied { permission: "file path has no applicable folder grant".into() })?;
        if relative.components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name.starts_with('.') || (cfg!(windows) && name.eq_ignore_ascii_case("AppData"))
        }) {
            return Err(HostError::PermissionDenied { permission: "hidden file path requires an explicit caller scope grant".into() });
        }
        Ok(())
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
