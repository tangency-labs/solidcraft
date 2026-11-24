//! SolidCraft policy and transactions for the shared Craft plugin host.

use crate::{EngineError, Result, Session};
pub use craft_plugin::ui;
pub use craft_plugin::{
    API, DesignToken, Effect, Entry, EntryKind, FileGrant, HostError, HostLimits, Limits, LossItem, LossReport, LossSeverity, MAX_APPROVED_FUEL,
    MAX_COMPONENT_BYTES, MAX_JSON_BYTES, MAX_MANIFEST_BYTES, MAX_MEMORY_MIB, Manifest, RuntimeManifest, UiSlot, check_input, conditions, dialog,
    swatches, valid_id,
};
use craft_plugin::{CommandSchema, PluginHost};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

static PROCESS_HOST_LIMITS: std::sync::OnceLock<HostLimits> = std::sync::OnceLock::new();
/// The command line's and headless MCP's plugins: the plugins folder of every session of the process
/// (`None`: the per-user one) and whether its grants are kept in that folder's grant store between
/// runs. Unset: the per-user folder, with grants that last for the session.
static PROCESS_PLUGINS: std::sync::OnceLock<(Option<PathBuf>, bool)> = std::sync::OnceLock::new();
/// Make every session this process opens from now on use the plugins folder `plugins` (`None`: the
/// per-user one) and, with `keep_grants`, keep approved grants in that folder's grant store and read
/// them back there, as the desktop app does. Without it, approvals last for the session, as before
/// there was a store. The command line and headless MCP call this once, before any session.
pub fn configure_process_plugins(plugins: Option<PathBuf>, keep_grants: bool) -> Result<()> {
    if let Some(dir) = &plugins
        && !dir.is_absolute()
    {
        return Err(error("the plugins folder must be an absolute path"));
    }
    PROCESS_PLUGINS.set((plugins, keep_grants)).map_err(|_| error("the process plugins folder is already configured"))
}
/// Keeps compiled plugin components between sessions in the `plugin-compile` folder of the per-user
/// cache folder ([`crate::recovery::cache_dir`]), so a large importer compiles once per version
/// rather than at the first file of every session. Call it once at startup, before any plugin runs.
pub fn configure_compile_cache() {
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    craft_plugin_wasm::set_compile_cache_dir(crate::recovery::cache_dir().map(|dir| dir.join("plugin-compile")));
}
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

/// Approved permissions, stored by plugin id outside the design.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Grants {
    /// Exact manifest and component approved at install. Legacy records require re-approval.
    #[serde(default)]
    pub package_hash: Option<String>,
    #[serde(default)]
    pub command_groups: BTreeSet<String>,
    #[serde(default)]
    pub folders: Vec<FileGrant>,
    /// Fuel the user let this package use above Max plugin fuel: exactly its manifest request,
    /// bound to `package_hash` like memory and never above `MAX_APPROVED_FUEL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
    /// Memory, in MiB, the user let this package use above the app's ceiling for plugins: what
    /// its manifest asks for, approved at install. Bound to `package_hash` like the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<usize>,
    /// An importer may read the other files in the folder of the file it opens (its manifest's
    /// `input_folder`), approved at install.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub input_folder: bool,
}

/// Trusted host state, never loaded from a design or supplied by guest code.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppState {
    /// Listener failures are application state, displayed in Preferences, never design data.
    pub listener_failures: BTreeMap<String, String>,
    /// Diagnostics by package and listener entry. A different entry's success cannot clear them.
    pub listener_entry_failures: BTreeMap<String, BTreeMap<String, String>>,
    /// Last successful listener result; the future panel adapter may consume this data.
    pub listener_data: std::sync::Arc<BTreeMap<String, Value>>,
    /// Latest linked-file observations, outside saved designs and undo snapshots.
    pub link_changes: std::sync::Arc<BTreeMap<u64, LinkChange>>,
    /// Successful source reads performed in this evaluation; scratch sessions start empty.
    pub(crate) link_reads: std::sync::Arc<BTreeSet<u64>>,
    /// Observations at the start of this evaluation, before any source reads or disk checks.
    pub(crate) link_read_source: std::sync::Arc<BTreeMap<u64, LinkChange>>,
    /// Host-only authority marker and event payload for a listener invocation.
    pub(crate) listener_design: Option<Option<DesignToken>>,
    pub packages: BTreeMap<String, String>,
    /// Transient, host-drawn panel state. Never serialised into a design.
    pub panels: BTreeMap<String, PanelPackage>,
    /// Namespace of the component currently updating panel data.
    pub ui_caller: Option<String>,
    pub grants: BTreeMap<String, Grants>,
    /// Adapter folder grants for the active call; runtime calls use the package's approved grants.
    pub file_grants: Vec<FileGrant>,
    /// Legacy embedding state. Runtime permissions come exclusively from grants bound to package hashes.
    pub command_groups: BTreeSet<String>,
    /// Where `plugin.install` writes and `plugin.list` looks; `None`: the per-user
    /// `plugins` folder ([`crate::recovery::data_dir`]).
    pub plugins_dir: Option<PathBuf>,
    /// The plugin whose run is under way, when it keeps data in designs (`storage@0.1`): the only
    /// one that may change plugin data, and only its own. Set by the host for that run alone.
    pub storage_caller: Option<storage::StorageCaller>,
    /// Set when `grants` is kept in the plugins folder's grant store between sessions (the command
    /// line and headless MCP with `--keep-plugin-approvals`): what this session last read from or wrote to it. `None`: the
    /// grants live only in this session, or the app keeps them itself (the desktop app).
    pub stored: Option<grant_store::Synced>,
}

impl AppState {
    /// History restores authored links, not observations of a different source or version.
    pub(crate) fn prune_link_changes(&mut self, doc: &solidcraft_doc::Document) {
        let current = |component: &u64, change: &LinkChange| {
            doc.links
                .get(component)
                .is_some_and(|link| link.path == change.path && change.read == (link.file_sha256.as_ref() == Some(&change.digest)))
        };
        if self.link_changes.iter().any(|(component, change)| !current(component, change)) {
            std::sync::Arc::make_mut(&mut self.link_changes).retain(|component, change| current(component, change));
        }
        let read = |component: &u64| self.link_changes.get(component).is_some_and(|change| change.read);
        if self.link_reads.iter().any(|component| !read(component)) {
            std::sync::Arc::make_mut(&mut self.link_reads).retain(read);
        }
    }

    /// Listener caches and panel frame adoption do not revoke an in-flight command's authority.
    pub(crate) fn same_execution_context(&self, other: &Self) -> bool {
        self.packages == other.packages
            && self.panels == other.panels
            && self.ui_caller == other.ui_caller
            && self.grants == other.grants
            && self.file_grants == other.file_grants
            && self.command_groups == other.command_groups
            && self.plugins_dir == other.plugins_dir
            && self.storage_caller == other.storage_caller
            && self.stored == other.stored
    }
    /// The installed plugins' folder, if this platform has one.
    pub fn plugins_dir(&self) -> Option<PathBuf> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        self.plugins_dir.clone().or_else(|| crate::recovery::data_dir().map(|p| p.join("plugins")))
    }

    /// A new session's plugin state: the process's plugins folder ([`configure_process_plugins`]),
    /// and when the process keeps grants between runs, one that reads and writes its grant store.
    pub fn for_process() -> Self {
        match PROCESS_PLUGINS.get() {
            Some((plugins, keep)) => Self { plugins_dir: plugins.clone(), stored: keep.then(Default::default), ..Default::default() },
            None => Self::default(),
        }
    }

    /// Take in grants another program stored or revoked since this session last looked. Nothing
    /// to do unless this session keeps its grants in the store.
    ///
    /// The store is read with the app's own authority, whatever the caller's file scope: what the
    /// user approved for a plugin applies whoever calls it, so an agent confined to its folders
    /// runs a plugin with its approvals, not with none. Writes still need the caller's scope.
    pub fn refresh_stored(&mut self) {
        let Some(dir) = self.plugins_dir().filter(|_| self.stored.is_some()) else { return };
        if let Some(synced) = self.stored.as_mut() {
            let grants = &mut self.grants;
            solidcraft_io::vfs::scoped(None, || synced.refresh(&dir, grants));
        }
    }

    /// Delete plugin `id`'s record from the store, when this session keeps its grants there, even
    /// one this session never read: a removed or revoked plugin leaves no approval behind.
    pub fn forget_stored(&mut self, id: &str) -> Result<()> {
        let Some(dir) = self.plugins_dir().filter(|_| self.stored.is_some()) else { return Ok(()) };
        match self.stored.as_mut() {
            Some(synced) => synced.forget(&dir, id).map_err(|e| error(format!("could not take back the approvals for later sessions: {e}"))),
            None => Ok(()),
        }
    }

    /// Write this session's changed grants to the store, when it keeps them there.
    pub fn persist_stored(&mut self) -> Result<()> {
        let Some(dir) = self.plugins_dir().filter(|_| self.stored.is_some()) else { return Ok(()) };
        match self.stored.as_mut() {
            Some(synced) => synced.persist(&dir, &self.grants).map_err(|e| error(format!("could not save the approvals for later sessions: {e}"))),
            None => Ok(()),
        }
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
        for entry in &runtime.entries {
            let id = format!("{}.{}", manifest.id, entry.name);
            if crate::find_command(&id).is_some() {
                return Err(refuse("runtime.entries", format!("entry `{id}` conflicts with a SolidCraft command")));
            }
        }
        if let Some(group) = runtime.permissions.command_groups.iter().find(|g| !COMMAND_GROUPS.contains(&g.as_str())) {
            return Err(refuse(
                "runtime.permissions.command_groups",
                format!("unknown command group `{group}`; SolidCraft has {}", COMMAND_GROUPS.join(", ")),
            ));
        }
        if runtime.is_exporter() {
            export::refuse_unreadable(runtime)?;
        }
        let storage = manifest.capabilities.iter().any(|capability| capability.starts_with("storage@"));
        for command in &runtime.commands {
            if command.starts_with("storage.") && !storage {
                return Err(refuse("runtime.commands", format!("`{command}` needs the storage capability: add \"storage@0.1\" to capabilities")));
            }
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
/// - `plugin`: reviews, installs, removes, inspects or runs plugins.
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
    "sketch.export_svg",
    "timeline.export_script",
];

/// Only reviewed input readers can access a folder granted for read without write.
/// A new command defaults to needing both permissions until its file effects are audited.
fn input_only_command(id: &str) -> bool {
    matches!(
        id,
        "component.insert_design"
            | "file.insert_step"
            | "file.insert_mesh"
            | "file.insert_gerber"
            | "file.gerber_files"
            | "sketch.insert_dxf"
            | "sketch.insert_svg"
    )
}

/// Both the manifest and the user must approve a folder for the intended access.
/// Writers retain the existing requirement that both read and write are approved.
fn approved_file_folders(granted: &[FileGrant], requested: &[craft_plugin::FilePermission], write: bool) -> Vec<PathBuf> {
    granted
        .iter()
        .filter(|g| g.read && (!write || g.write) && requested.iter().any(|r| r.name == g.name && r.read && (!write || r.write)))
        .map(|g| g.folder.clone())
        .collect()
}

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
        solidcraft_io::vfs::read(&module_path.to_string_lossy(), runtime.max_component_bytes()).map_err(|e| error(e.to_string()))?
    } else {
        Vec::new()
    };
    Ok(Package { manifest, approval: package_hash(&bytes, &component), component })
}

/// Whether `folder` is an installed package in the app's own plugins folder (`{plugins}/<id>`).
pub fn is_installed_folder(session: &Session, folder: &str) -> bool {
    let path = Path::new(folder);
    session.plugin_state.plugins_dir().is_some_and(|root| path.parent() == Some(root.as_path()))
        && path.file_name().and_then(|n| n.to_str()).is_some_and(valid_id)
}

/// Reads an installed package with the app's authority, whatever the caller's file scope: a
/// plugin tool call names a plugin, never a path, and the plugins folder is the app's own. Any
/// other folder is read through the caller's scope, as `plugin.inspect` reads it.
pub fn with_installed<T>(session: &Session, folder: &str, read: impl FnOnce() -> T) -> T {
    if is_installed_folder(session, folder) { solidcraft_io::vfs::scoped(None, read) } else { read() }
}

/// The ceilings a package runs under: the app's, raised to the fuel and memory the user approved
/// for this exact manifest and component at install. A grant for another package hash raises nothing.
pub fn host_limits_for(session: &Session, id: &str, approval: &str) -> HostLimits {
    let mut limits = session.plugin_limits;
    if let Some(fuel) =
        session.plugin_state.grants.get(id).filter(|grants| grants.package_hash.as_deref() == Some(approval)).and_then(|grants| grants.fuel)
    {
        limits.max_fuel = limits.max_fuel.max(fuel.min(MAX_APPROVED_FUEL));
    }
    if let Some(memory) =
        session.plugin_state.grants.get(id).filter(|grants| grants.package_hash.as_deref() == Some(approval)).and_then(|grants| grants.memory_mib)
    {
        limits.max_memory_mib = limits.max_memory_mib.max(memory.min(MAX_MEMORY_MIB));
    }
    limits
}

/// Take back what was approved for plugin `id`, keeping it installed: it then runs with nothing
/// granted until it is installed again with approvals. Whether there was anything to take back.
pub fn revoke(s: &mut Session, id: &str) -> Result<bool> {
    if !valid_id(id) {
        return Err(HostError::InvalidInput { field: "id".into(), reason: "invalid plugin id".into() }.into());
    }
    s.plugin_state.refresh_stored();
    let revoked = s.plugin_state.grants.remove(id).is_some();
    s.plugin_state.persist_stored()?;
    s.plugin_state.forget_stored(id)?;
    Ok(revoked)
}

/// The most files and folders one plugin folder holds; install and removal both stop there.
pub(super) const MAX_FILES: usize = 1024;

pub(super) fn package_hash(manifest: &[u8], component: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"solidcraft-plugin-approval-v1\0");
    digest.update((manifest.len() as u64).to_le_bytes());
    digest.update(manifest);
    digest.update(component);
    format!("{:x}", digest.finalize())
}

/// The shared hook path for a dialog worker and plugin.run. Hook code receives a scratch host
/// with no file or edit grants. Complete calls propagate execution failures; a form retains its
/// declarative state and shows a quiet note instead.
pub fn update_dialog(
    session: &Session,
    package: &Package,
    entry: &str,
    values: &Value,
    protected: &BTreeSet<String>,
    complete: bool,
) -> Result<dialog::Update> {
    if session.disabled_plugins.contains(&package.manifest.id) {
        return Err(error("plugin is disabled"));
    }
    if session.plugin_state.grants.get(&package.manifest.id).is_some_and(|g| g.package_hash.as_deref() != Some(package.approval.as_str())) {
        return Err(HostError::PermissionDenied { permission: "plugin grants need re-approval".into() }.into());
    }
    let rt = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    let declared = rt.entries.iter().find(|e| e.name == entry).ok_or_else(|| error("unknown entry"))?;
    let schema = &declared.input_schema;
    let baseline = dialog::apply(schema, values, protected, None, false)?.values;
    if !declared.dialog_hook {
        return dialog::apply(schema, &baseline, protected, None, complete).map_err(Into::into);
    }
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    let result = {
        let host = SolidCraftHost {
            session: session.scratch(),
            transaction: None,
            sketch_exits: Vec::new(),
            view: None,
            groups: BTreeSet::new(),
            file_folders: Vec::new(),
            file_read_folders: Vec::new(),
            evaluation: None,
            forms: Vec::new(),
            form_source: None,
        };
        let (_, result) = craft_plugin_wasm::dialog_fields(
            host,
            &package.manifest,
            &package.component,
            entry,
            &baseline,
            host_limits_for(session, &package.manifest.id, &package.approval),
        );
        result
    };
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    let result: craft_plugin::Result<String> = Err(HostError::Trap { reason: "plugin execution requires a native build with plugins".into() });
    match result {
        Ok(reply) => dialog::apply(schema, &baseline, protected, Some(&reply), complete).map_err(Into::into),
        Err(error) if complete => Err(error.into()),
        Err(error) => {
            let mut update = dialog::apply(schema, &baseline, protected, None, false)?;
            update.warning = Some(error.to_string());
            Ok(update)
        }
    }
}

/// Runs `entry` of `package`. A failure starts with the plugin's name.
pub fn run(session: &mut Session, package: Package, entry: &str, params: &serde_json::Value) -> Result<crate::schema::plugin::RunResult> {
    run_with_ui_event(session, package, entry, params, None)
}

/// Deliver an explicitly requested panel action while preserving the entry's ordinary input.
pub fn run_with_ui_event(
    session: &mut Session,
    package: Package,
    entry: &str,
    params: &serde_json::Value,
    ui_event: Option<&ui::UiEvent>,
) -> Result<crate::schema::plugin::RunResult> {
    let name = package.manifest.name.clone();
    run_package(session, package, entry, params, ui_event).map_err(messages::named(&name, None))
}

/// Run a workspace's optional enter or exit entry against a discarded snapshot.
///
/// The window calls this on its worker, so switching never waits for guest code. Hooks have
/// at most one second, one million fuel and 32 MiB, or the package's or user's lower budget.
/// The execution copy retains the original approval hash: narrowing a call's limits never
/// changes which exact package the user approved. No computed state, camera or history is
/// adopted, even if a host command classified as read changes transient session state.
pub fn run_workspace_hook(session: &mut Session, mut package: Package, workspace: &str, entering: bool) -> Result<crate::schema::plugin::RunResult> {
    let name = package.manifest.name.clone();
    let result = (|| {
        let mut scratch = session.scratch();
        scratch.plugin_state.refresh_stored();
        if scratch.disabled_plugins.contains(&package.manifest.id) {
            return Err(error("plugin is disabled"));
        }
        let declarations = package
            .manifest
            .runtime
            .as_ref()
            .and_then(|runtime| runtime.ui.as_ref())
            .filter(|_| package.manifest.capabilities.iter().any(|capability| capability.starts_with("ui@")))
            .ok_or_else(|| error("workspace hooks require ui@0.1"))?;
        let workspace_spec = declarations.workspaces.iter().find(|spec| spec.id == workspace).ok_or_else(|| error("unknown workspace"))?;
        let hook = if entering { &workspace_spec.on_enter } else { &workspace_spec.on_exit };
        let Some(entry) = hook.clone() else {
            return Ok(crate::schema::plugin::RunResult {
                forms: vec![],
                form_validation: None,
                result: json!({}),
                sketch_exits: vec![],
                view: None,
                param_warnings: vec![],
            });
        };
        let ceilings = host_limits_for(&scratch, &package.manifest.id, &package.approval);
        let runtime = package.manifest.runtime.as_mut().ok_or_else(|| error("no executable runtime"))?;
        let declared = runtime.entries.iter().find(|declared| declared.name == entry).ok_or_else(|| error("unknown workspace hook entry"))?;
        if declared.kind != EntryKind::Command || declared.effect != Effect::Read {
            return Err(error("workspace hooks must be read-only command entries"));
        }
        runtime.limits = workspace_hook_limits(&runtime.limits, ceilings);
        // Grouped read commands also receive no write authority. The component's own
        // files interface already rejects writes from a read entry.
        for grant in scratch.plugin_state.grants.values_mut() {
            for folder in &mut grant.folders {
                folder.write = false;
            }
        }
        let mut result = run_package(&mut scratch, package, &entry, &json!({"workspace": workspace}), None)?;
        // A hook is observational: it adopts no form, sketch exit or camera move.
        result.forms.clear();
        result.form_validation = None;
        result.sketch_exits.clear();
        result.view = None;
        Ok(result)
    })();
    result.map_err(messages::named(&name, None))
}

fn workspace_hook_limits(requested: &Limits, ceilings: HostLimits) -> Limits {
    Limits {
        fuel: requested.fuel.min(1_000_000).min(ceilings.max_fuel),
        memory_mib: requested.memory_mib.min(32).min(ceilings.max_memory_mib),
        deadline_ms: requested.deadline_ms.min(ui::WORKSPACE_HOOK_DEADLINE_MS),
        stack_kib: requested.stack_kib.map(|kib| kib.min(256)),
    }
}

fn run_package(
    session: &mut Session,
    package: Package,
    entry: &str,
    params: &serde_json::Value,
    ui_event: Option<&ui::UiEvent>,
) -> Result<crate::schema::plugin::RunResult> {
    session.plugin_state.refresh_stored();
    if session.disabled_plugins.contains(&package.manifest.id) {
        return Err(error("plugin is disabled"));
    }
    if let Some(grants) = session.plugin_state.grants.get(&package.manifest.id)
        && grants.package_hash.as_deref() != Some(package.approval.as_str())
    {
        return Err(HostError::PermissionDenied {
            permission: "stored plugin grants need re-approval for this manifest and component; install and review again".into(),
        }
        .into());
    }
    let rt = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    host_limits_for(session, &package.manifest.id, &package.approval).check(&rt.limits)?;
    let declared = rt.entries.iter().find(|e| e.name == entry).ok_or_else(|| error("unknown entry"))?;
    let prepared = if declared.dialog_hook {
        let protected = params.as_object().map(|v| v.keys().cloned().collect()).unwrap_or_default();
        let update = update_dialog(session, &package, entry, params, &protected, true)?;
        if let Some(warning) = &update.warning {
            session.echo(warning);
        }
        Some(craft_plugin::prepare_input(&declared.input_schema, &update.values)?)
    } else if conditions::has_conditions(&declared.input_schema) {
        Some(craft_plugin::prepare_input(&declared.input_schema, params)?)
    } else {
        None
    };
    let params = prepared.as_ref().unwrap_or(params);
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // What this call may reach: the groups both the manifest asks for and the user granted,
        // and for `file` commands the granted folders the manifest may both read and write.
        let grants = session.plugin_state.grants.get(&package.manifest.id).cloned().unwrap_or_default();
        let groups: BTreeSet<String> = rt.permissions.command_groups.intersection(&grants.command_groups).cloned().collect();
        let mut scratch = session.scratch();
        panels::register_package(&mut scratch, &package)?;
        scratch.plugin_state.ui_caller = Some(package.manifest.id.clone());
        scratch.plugin_state.file_grants = grants.folders;
        scratch.plugin_state.storage_caller =
            rt.data_format.filter(|_| package.manifest.capabilities.iter().any(|c| c.starts_with("storage@"))).map(|data_format| {
                storage::StorageCaller {
                    id: package.manifest.id.clone(),
                    name: package.manifest.name.clone(),
                    version: package.manifest.version.clone(),
                    data_format,
                }
            });
        let host = SolidCraftHost {
            session: scratch,
            transaction: None,
            sketch_exits: Vec::new(),
            view: None,
            groups,
            file_folders: Vec::new(),
            file_read_folders: Vec::new(),
            evaluation: None,
            forms: Vec::new(),
            form_source: Some((package.manifest.id.clone(), rt.entries.clone())),
        };
        let approved = host.granted_folders();
        let file_read_folders = approved_file_folders(&approved, &rt.permissions.files, false);
        let file_folders = approved_file_folders(&approved, &rt.permissions.files, true);
        let host = SolidCraftHost { file_folders, file_read_folders, ..host };
        let event = panels::event_for(&package.manifest, entry, ui_event)?;
        let (host, result) = match event {
            Some(event) => craft_plugin_wasm::run_ui_event(host, &package.manifest, &package.component, entry, params, &event),
            None => craft_plugin_wasm::run(host, &package.manifest, &package.component, entry, params),
        };
        let value = result?;
        session.plugin_state.panels.clone_from(&host.session.plugin_state.panels);
        if !std::sync::Arc::ptr_eq(&session.doc, &host.session.doc)
            || session.active_sketch != host.session.active_sketch
            || session.sketching_view != host.session.sketching_view
            || !host.session.sketch_finish_events.is_empty()
            || host.session.edit_start_sequence != 0
            || session.pending_moves != host.session.pending_moves
        {
            session.accept_computed(host.session)?;
        } else {
            session.plugin_state.link_changes = host.session.plugin_state.link_changes;
            std::sync::Arc::make_mut(&mut session.plugin_state.link_reads).extend(host.session.plugin_state.link_reads.iter().copied());
            for message in host.session.log {
                session.echo(message);
            }
        }
        Ok(crate::schema::plugin::RunResult {
            result: value,
            forms: host.forms,
            form_validation: None,
            sketch_exits: host.sketch_exits,
            view: host.view,
            param_warnings: vec![],
        })
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = (params, ui_event);
        Err(error("plugin execution requires an x86-64 or AArch64 native build with the plugins feature"))
    }
}

pub mod check;
mod events;
mod export;
pub use events::{LinkChange, design_changed, design_notifications};
pub(crate) use events::{record_changed_link_reads, record_link_change, record_link_reads};
mod form;
pub mod grant_store;
pub use form::form_errors;
mod import;
pub mod install;
pub mod messages;
#[cfg(not(target_arch = "wasm32"))]
mod package;
mod panels;
pub use panels::{PanelPackage, PanelSnapshot, sync_panels};
pub mod storage;
#[cfg(not(target_arch = "wasm32"))]
pub use package::{remove, store_grants, validate_grants};

pub use export::{describe as describe_exporters, export_file, exporter_for, exporters};
pub use import::{FormatSlot, Importer, describe as describe_importers, format_slots, import_file, importer_for, importers, open_filters};
pub use messages::PluginFailure;
#[cfg(test)]
mod tests;
mod tools;
pub use tools::{explain_importer, explain_named, missing_grant, structured_error as structured_tool_error, tools};

/// The application adapter owns a scratch transaction. Wasmtime sees only PluginHost.
pub struct SolidCraftHost {
    pub session: Session,
    transaction: Option<Session>,
    sketch_exits: Vec<Option<u64>>,
    /// The last view command in this run. Cleared on rollback, like `sketch_exits`.
    /// `accept_computed` does not copy `session.camera`, so the window applies this beside that.
    view: Option<crate::schema::plugin::PluginView>,
    /// Granted sensitive command groups, for this call's manifest.
    groups: BTreeSet<String>,
    /// The file scope of a granted `file` command; every other command has none.
    file_folders: Vec<PathBuf>,
    /// Approved folders used only by known read-only input commands.
    file_read_folders: Vec<PathBuf>,
    /// An importer's own design, opened to check it (`evaluate@0.1`), apart from every open design.
    evaluation: Option<Box<Session>>,
    forms: Vec<crate::schema::plugin::FormRequest>,
    form_source: Option<(String, Vec<Entry>)>,
}

impl SolidCraftHost {
    /// Keep the last view command. `view.fit` and `view.home` frame in the window.
    /// `view.navigate` does not store a camera; its reply is what the navigator applies.
    /// Any other command that changes `session.camera` keeps that camera.
    fn note_view(&mut self, name: &str, reply: &Value, before: Option<solidcraft_viewport::Camera>, after: Option<solidcraft_viewport::Camera>) {
        if matches!(name, "view.fit" | "view.home") {
            self.view = Some(if name == "view.home" { crate::schema::plugin::PluginView::Home } else { crate::schema::plugin::PluginView::Fit });
        } else if name == "view.navigate" {
            self.view = Some(crate::schema::plugin::PluginView::Navigate { reply: reply.clone() });
        } else if after != before
            && let Ok(camera) = serde_json::to_value(after)
        {
            self.view = Some(crate::schema::plugin::PluginView::Camera { camera });
        }
    }
}

/// A storage call's error as the plugin gets it: the host's own (a permission, a limit) as it is,
/// a bad name or range as invalid input it can act on, anything else as the call failing.
fn storage_error(call: &str, error: crate::EngineError) -> HostError {
    if let Some(host) = error.host_error() {
        return host.clone();
    }
    match error {
        crate::EngineError::BadParams { .. } => HostError::InvalidInput { field: call.into(), reason: error.to_string() },
        error => HostError::CommandFailed { command: call.into(), reason: error.to_string() },
    }
}

/// The commands an importer may run on its own design to check it: they read the model, and
/// `timeline.roll_to` moves only that private session's marker.
pub const EVALUATE_COMMANDS: &[&str] = &["document.inspect", "inspect.measure", "model.edges", "model.faces", "timeline.roll_to"];
impl PluginHost for SolidCraftHost {
    fn open_form(&mut self, entry: &str, params: &Value) -> craft_plugin::Result<()> {
        self.request_form(entry, params)
    }
    fn design_token(&self) -> craft_plugin::Result<Option<DesignToken>> {
        match &self.session.plugin_state.listener_design {
            Some(design) => Ok(design.clone()),
            None => Ok(Some(self.transaction.as_ref().unwrap_or(&self.session).design_token())),
        }
    }
    fn evaluate_open(&mut self, design: &[u8]) -> craft_plugin::Result<()> {
        let doc = solidcraft_io::read_design(design)
            .map_err(|e| HostError::InvalidInput { field: "design".into(), reason: format!("SolidCraft can't read this design: {e}") })?;
        let validation = self.session.validation();
        // No file reaches this session: not a font, not a linked design.
        let opened = solidcraft_io::vfs::scoped(Some(Default::default()), || Session::opened_at_validating(doc, None, None, validation));
        self.evaluation = Some(Box::new(opened));
        Ok(())
    }
    fn evaluate_run(&mut self, command: &str, args: &Value) -> craft_plugin::Result<Value> {
        if !EVALUATE_COMMANDS.contains(&command) {
            return Err(HostError::PermissionDenied { permission: format!("evaluate: {command} is not one of {}", EVALUATE_COMMANDS.join(", ")) });
        }
        let s = self.evaluation.as_mut().ok_or_else(|| HostError::InvalidInput { field: "evaluate".into(), reason: "no design is open".into() })?;
        Ok(solidcraft_io::vfs::scoped(Some(Default::default()), || s.execute(command, args)).unwrap_or_else(|e| json!({"error": e.to_string()})))
    }
    fn evaluate_close(&mut self) {
        self.evaluation = None;
    }
    fn storage_put_blob(&mut self, name: &str, data: &[u8], cache: bool, append: bool) -> craft_plugin::Result<u64> {
        let s = self
            .transaction
            .as_mut()
            .ok_or_else(|| HostError::CommandFailed { command: "storage.put-blob".into(), reason: "no active transaction".into() })?;
        storage::write_blob(s, name, data, cache, append).map_err(|e| storage_error("storage.put-blob", e))
    }
    fn storage_get_blob(&mut self, plugin: Option<&str>, name: &str, offset: u64, len: u32) -> craft_plugin::Result<Vec<u8>> {
        let s = self.transaction.as_ref().unwrap_or(&self.session);
        storage::read_blob(s, plugin, name, offset, len).map(|read| read.data).map_err(|e| storage_error("storage.get-blob", e))
    }
    fn storage_get_document(&mut self, plugin: Option<&str>, offset: u64, len: u32) -> craft_plugin::Result<Vec<u8>> {
        let s = self.transaction.as_ref().unwrap_or(&self.session);
        storage::read_document(s, plugin, offset, len).map(|(data, _)| data).map_err(|e| storage_error("storage.get-document", e))
    }
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
        // A deviation sweep starts its own parallel work, outside the guest's fuel/deadline.
        // It remains a read command for explicit invocations, but never starts from a listener.
        if self.session.plugin_state.listener_design.is_some() && name == "pointcloud.deviation" {
            return Err(HostError::PermissionDenied { permission: format!("work command {name} from read entry") });
        }
        if self.session.plugin_state.listener_design.is_some()
            && (command_group(name).is_some()
                || name.starts_with("render.")
                || name.starts_with("jobs.")
                || catalog_command(name).is_none_or(|command| command.effect != Effect::Read))
        {
            return Err(HostError::PermissionDenied { permission: format!("edit command {name} from read entry") });
        }
        if catalog_command(name).is_none() {
            return Err(HostError::PermissionDenied { permission: format!("command {name} is not granted") });
        }
        let group = command_group(name);
        if let Some(group) = group
            && !self.groups.contains(group)
        {
            return Err(HostError::PermissionDenied { permission: format!("command {name} needs the `{group}` permission, which is not granted") });
        }
        // Read-only grants apply only to known input readers. File writers retain their
        // previous read-and-write scope; unknown commands default to that stricter scope.
        let scope = match group {
            Some("file" | "doc") => {
                let folders = if input_only_command(name) { &self.file_read_folders } else { &self.file_folders };
                solidcraft_io::vfs::PathScope::new(folders.iter().cloned())
            }
            _ => Default::default(),
        };
        let s = self.transaction.as_mut().ok_or_else(|| HostError::CommandFailed { command: name.into(), reason: "no active transaction".into() })?;
        let finish_sequence = s.sketch_finish_sequence;
        let camera_before = s.camera;
        let before = s.doc.clone();
        let previous: BTreeMap<_, _> = s.model.results.iter().map(|r| (r.id, r.error.clone())).collect();
        // Even implicit font and linked-design reads have no authority from a command grant.
        let ui_caller = s.plugin_state.ui_caller.clone();
        let reply = solidcraft_io::vfs::scoped(Some(scope), || s.execute(name, params));
        // Replacing the design clears ambient callers. This component still owns its
        // active transaction and may populate its panels for the newly opened design.
        s.plugin_state.ui_caller = ui_caller;
        let reply = reply.map_err(|e| HostError::CommandFailed { command: name.into(), reason: e.to_string() })?;
        if let Some(failed) = s
            .model
            .results
            .iter()
            .find(|r| r.error.is_some() && (previous.get(&r.id) != Some(&r.error) || before.feature(r.id) != s.doc.feature(r.id)))
        {
            return Err(HostError::CommandFailed { command: name.into(), reason: failed.error.clone().unwrap_or_default() });
        }
        let camera_after = s.camera;
        for event in s.sketch_finish_events.iter().filter(|event| event.sequence > finish_sequence && event.domain == crate::SketchDomain::Model) {
            self.sketch_exits.push(Some(event.sketch));
        }
        if matches!(name, "sketch.finish" | "sketch.cancel") {
            self.sketch_exits.push(reply.get("sketch").and_then(Value::as_u64));
        }
        self.note_view(name, &reply, camera_before, camera_after);
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
        self.session.plugin_state.panels.clone_from(&computed.plugin_state.panels);
        // Session::execute(plugin.run) owns the one outer undo step.
        if !std::sync::Arc::ptr_eq(&self.session.doc, &computed.doc)
            || self.session.active_sketch != computed.active_sketch
            || self.session.sketching_view != computed.sketching_view
            || !computed.sketch_finish_events.is_empty()
            || computed.edit_start_sequence != 0
            || self.session.pending_moves != computed.pending_moves
        {
            self.session
                .accept_computed(computed)
                .map_err(|error| HostError::InvalidInput { field: "transaction".into(), reason: error.to_string() })?;
        } else {
            self.session.plugin_state.link_changes = computed.plugin_state.link_changes;
            std::sync::Arc::make_mut(&mut self.session.plugin_state.link_reads).extend(computed.plugin_state.link_reads.iter().copied());
            for message in computed.log {
                self.session.echo(message);
            }
        }
        Ok(())
    }
    fn rollback_transaction(&mut self) -> craft_plugin::Result<()> {
        self.transaction = None;
        self.sketch_exits.clear();
        self.forms.clear();
        self.view = None;
        Ok(())
    }
    fn ui_set_data(&mut self, panel: &str, updates: &[ui::Datum]) -> craft_plugin::Result<()> {
        panels::set_data(self.transaction.as_mut(), panel, updates)
    }
    fn ui_clear_data(&mut self, panel: &str) -> craft_plugin::Result<()> {
        panels::clear_data(self.transaction.as_mut(), panel)
    }
    fn ui_show_panel(&mut self, panel: &str, visible: bool) -> craft_plugin::Result<()> {
        panels::show_panel(self.transaction.as_mut(), panel, visible)
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
            // A write may make folders that do not exist yet (`write-file`): its nearest existing
            // ancestor must be in scope, as with `check_new`. A read needs the whole path to exist.
            let allowed = if write { scope.check_new(&path.to_string_lossy()) } else { scope.check(&path.to_string_lossy()) };
            return allowed.map_err(|e| HostError::PermissionDenied { permission: e.to_string() });
        }
        grant_covers(&self.session.plugin_state.file_grants, path, write)
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

/// Whether `path` is covered by a folder grant, for reading or for writing.
/// A refusal is the same error [`PluginHost::authorize_file_path`] returns when no caller
/// scope is set. Choosing a folder in a dialog does not add a grant.
pub fn require_folder_grant(grants: &[FileGrant], path: &Path) -> craft_plugin::Result<()> {
    if grant_covers(grants, path, false).is_ok() || grant_covers(grants, path, true).is_ok() { Ok(()) } else { grant_covers(grants, path, false) }
}

/// The approved root and its ancestors are explicit, even in AppData or a hidden folder.
/// Inspect only names beneath the most specific grant for this operation.
fn grant_covers(grants: &[FileGrant], path: &Path, write: bool) -> craft_plugin::Result<()> {
    let relative = grants
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
    let inputs = params.get("params").cloned().unwrap_or(json!({}));
    // Preserve direct calls' input pass-through; tool callers validate their listed schema.
    // `named`: a refusal comes back in words an agent can act on (`tools::explain`).
    let mut call = json!({"path":path,"approval":approval,"entry":entry,"params":inputs,"named":true});
    if let (Some(event), Some(call)) = (params.get("ui_event"), call.as_object_mut()) {
        call.insert("ui_event".into(), event.clone());
    }
    if let Some(expected) = params.get("expected_design") {
        call["expected_design"] = expected.clone();
    }
    Ok(Some(call))
}

/// `engine.describe` of an installed, enabled plugin's entry: a command entry as the
/// `<plugin>.<entry>` command, an importer as the tool that opens its files, or an exporter as
/// the tool that writes the open design; each with the
/// schema `tools/list` gives it. Read with the app's authority, whatever the caller's folders.
pub fn describe_entry(s: &Session, id: &str) -> Result<Option<Value>> {
    tools::describe(s, id)
}
