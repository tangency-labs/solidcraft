//! One-shot plugins share the engine's command surface, file scope and undo history.
use super::CommandSpec;
use crate::schema::plugin::{
    CheckParams, EditParams, FormValidation, GrantStatus, GrantsParams, GrantsResult, ImportParams, ImportedDesign, InspectParams, InspectResult,
    InstallParams, InstallResult, ListParams, ListResult, ListedPlugin, MeshParams, MeshResult, RemoveParams, RemoveResult, RevokeParams,
    RevokeResult, RunParams, RunResult, StoredGrant, ToolsParams, ToolsResult,
};
use crate::{
    Result, Session, checked,
    params::bad,
    plugin,
    schema::{TypedContract, output_schema_for},
};
use serde_json::Value;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("file.import", "Import with Plugin", checked!("file.import", import)).icon("import").noundo()
        .contract(&TypedContract::<ImportParams>::new()).output(output_schema_for::<ImportedDesign>)
        .params("path, format?: installed importer plugin_id.entry (default: the one claiming the file's extension; see plugin.list importers), options?: the importer's declared options, named?: bool (refusals name the missing grant and how to approve it); opens the file as a new, unsaved design made by the importer plugin and returns its loss_report; never opens a dialog"),
    CommandSpec::new("model.mesh", "Read Body Mesh", checked!("model.mesh", mesh)).icon("inspect").noundo().read_only()
        .contract(&TypedContract::<MeshParams>::new()).output(output_schema_for::<MeshResult>)
        .params("body, tolerance?: finite positive chord tolerance in mm (default 0.05); evaluated component-local mesh, at most 8192 vertices and 16384 triangles; no files or model changes"),
    CommandSpec::new("plugin.check", "Check Plugin", checked!("plugin.check", check)).icon("inspect").noundo().read_only().doc_only()
        .contract(&TypedContract::<CheckParams>::new()).output(output_schema_for::<crate::schema::plugin::CheckResult>)
        .params("path: plugin package folder or .zip; checks it as Install and a call would (manifest, size limits, permissions, the component's WIT world and version) and lists each problem with how to fix it; runs no plugin code and installs nothing"),
    CommandSpec::new("plugin.edit", "Edit Plugin Operation", checked!("plugin.edit", edit)).icon("edit").noundo().read_only().doc_only()
        .contract(&TypedContract::<EditParams>::new()).output(output_schema_for::<RunResult>)
        .params("plugin, entry, params: open the normal form through ui.start; scripts and MCP receive interactive_required; call the entry with params instead"),
    CommandSpec::new("plugin.grants", "Plugin Approvals", checked!("plugin.grants", grants)).icon("inspect").noundo().doc_only()
        .contract(&TypedContract::<GrantsParams>::new()).output(output_schema_for::<GrantsResult>)
        .params("What the user approved for each plugin (command groups, folders, memory, input_folder), the package each approval is bound to and its status: current, changed (install and approve again), not_installed or unreadable; runs no plugin code"),
    CommandSpec::new("plugin.inspect", "Inspect Plugin", checked!("plugin.inspect", inspect)).icon("inspect").noundo().doc_only()
        .contract(&TypedContract::<InspectParams>::new()).output(output_schema_for::<InspectResult>)
        .params("path: plugin package folder; returns inert manifest and approval hash covering manifest and component; runs no plugin code"),
    CommandSpec::new("plugin.install", "Install Plugin", checked!("plugin.install", install)).icon("import").noundo().doc_only()
        .contract(&TypedContract::<InstallParams>::new()).output(output_schema_for::<InstallResult>)
        .params("path: plugin .zip, approval?: exact hash from a reviewed dry run (mismatch refuses installation), archive?: exact .zip hash from that dry run (any changed file refuses installation), replace?: bool (required to replace an installed plugin of the same id), dry_run?: bool, approve?: {command_groups, folders: [{name, path, read, write}], memory_mib?: exactly the manifest's limits.memory_mib, allowed above the user's ceiling for this package, input_folder?: bool, only for an importer that asks for it}; validates requested approvals and checks the .zip (one plugin, safe paths, size caps, manifest) and unpacks it into the plugins folder; runs no plugin code"),
    CommandSpec::new("plugin.list", "List Plugins", checked!("plugin.list", list)).icon("inspect").noundo().doc_only()
        .contract(&TypedContract::<ListParams>::new()).output(output_schema_for::<ListResult>)
        .params("List inspected and installed plugin manifests, namespaced entries, declared effects and input schemas; no plugin code runs"),
    CommandSpec::new("plugin.remove", "Remove Plugin", checked!("plugin.remove", remove)).icon("delete").noundo().doc_only()
        .contract(&TypedContract::<RemoveParams>::new()).output(output_schema_for::<RemoveResult>)
        .params("id: installed plugin id; removes its folder and stored grants without executing code"),
    CommandSpec::new("plugin.revoke", "Revoke Plugin Approvals", checked!("plugin.revoke", revoke)).icon("delete").noundo().doc_only()
        .contract(&TypedContract::<RevokeParams>::new()).output(output_schema_for::<RevokeResult>)
        .params("id: plugin id; takes back everything approved for it and keeps it installed: it runs with nothing granted until it is installed again with approvals"),
    CommandSpec::new("plugin.run", "Run Plugin", checked!("plugin.run", run)).icon("play")
        .contract(&TypedContract::<RunParams>::new()).output(output_schema_for::<RunResult>)
        .params("path, approval: exact hash from plugin.inspect after reviewing permissions, entry, params?: JSON, expected_design?: {document_id, revision} (refuse stale edits with both expected and actual tokens), named?: bool (refusals name the missing grant and how to approve it); explicit native Wasm component execution, one undo step; errors discard all edits"),
    CommandSpec::new("plugin.tools", "List Plugin Tools", checked!("plugin.tools", tools)).icon("inspect").noundo().read_only().doc_only()
        .contract(&TypedContract::<ToolsParams>::new()).output(output_schema_for::<ToolsResult>)
        .params("name?: one tool, <plugin>.<entry>; every installed, enabled plugin's entries as tools for agents: name, kind (command or importer), the plugin's name, version and description, effect and strict input schema, and with name a command entry's approval hash; no plugin code runs"),
];
fn check(s: &mut Session, p: &Value) -> Result<Value> {
    let p: CheckParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.check", e.to_string()))?;
    serde_json::to_value(plugin::check::check(s, &p.path)).map_err(|e| bad("plugin.check", e.to_string()))
}
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let p: InspectParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.inspect", e.to_string()))?;
    let package = plugin::read_package(&p.path)?;
    s.plugin_state.refresh_stored();
    s.plugin_state.packages.insert(package.manifest.id.clone(), p.path.clone());
    let limits = plugin::host_limits_for(s, &package.manifest.id, &package.approval);
    let limits_error = package
        .manifest
        .runtime
        .as_ref()
        .and_then(|r| limits.check(&r.limits).err())
        .map(|error| plugin::PluginFailure { plugin: package.manifest.name.clone(), file: None, error }.text());
    serde_json::to_value(InspectResult {
        manifest: package.manifest,
        approval: package.approval,
        host_limits: limits,
        within_host_limits: limits_error.is_none(),
        limits_error,
        param_warnings: vec![],
    })
    .map_err(|e| bad("plugin.inspect", e.to_string()))
}
fn install(s: &mut Session, p: &Value) -> Result<Value> {
    use plugin::install::{InstallError, MAX_ZIP_BYTES, unpack};
    // Serde also accepts sequences for structs; approvals must use the declared object shape.
    if !p.is_object() {
        return Err(bad("plugin.install", "parameters must be an object"));
    }
    if p.get("approve").is_some_and(|approve| !approve.is_null() && !approve.is_object()) {
        return Err(bad("plugin.install", "approve must be an object"));
    }
    if p.pointer("/approve/folders").and_then(Value::as_array).is_some_and(|folders| folders.iter().any(|folder| !folder.is_object())) {
        return Err(bad("plugin.install", "approved folders must be objects"));
    }
    let p: InstallParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.install", e.to_string()))?;
    let failed = |e: InstallError| match e {
        InstallError::Manifest(e) => crate::EngineError::Plugin(e),
        e => crate::EngineError::Other(e.to_string()),
    };
    let dir = s.plugin_state.plugins_dir().ok_or_else(|| failed(InstallError::NoPluginsFolder))?;
    let zip = solidcraft_io::vfs::read(&p.path, MAX_ZIP_BYTES).map_err(|e| crate::EngineError::Other(format!("could not read {}: {e}", p.path)))?;
    let plugin = unpack(&zip).map_err(failed)?;
    if p.approval.as_ref().is_some_and(|approval| approval != &plugin.approval) {
        return Err(craft_plugin::HostError::InvalidInput {
            field: "approval".into(),
            reason: "does not match this manifest and component; review the package again".into(),
        }
        .into());
    }
    if p.archive.as_ref().is_some_and(|archive| archive != &plugin.archive) {
        return Err(craft_plugin::HostError::InvalidInput {
            field: "archive".into(),
            reason: "the .zip changed since it was reviewed; review the package again".into(),
        }
        .into());
    }
    let manifest = &plugin.manifest;
    let folder = dir.join(&manifest.id);
    s.plugin_state.refresh_stored();
    // Validate a prospective state before publishing any files. Omitted approval resets grants.
    #[cfg(not(target_arch = "wasm32"))]
    let mut approved_state = {
        let mut scratch = s.scratch();
        plugin::store_grants(&mut scratch, &plugin, p.approve.map(Into::into).unwrap_or_default())?;
        scratch.plugin_state
    };
    #[cfg(not(target_arch = "wasm32"))]
    let replaces = plugin::install::installed(&dir, &manifest.id).map(|(name, version)| format!("{name} {version}").trim().to_string());
    #[cfg(target_arch = "wasm32")]
    let replaces: Option<String> = None;
    let installed = !p.dry_run;
    if installed {
        // The plugins folder is the app's own, but a scoped caller must have been given it.
        solidcraft_io::vfs::check_new(&folder.to_string_lossy()).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            plugin::install::install(&plugin, &dir, p.replace).map_err(failed)?;
            std::mem::swap(&mut s.plugin_state.grants, &mut approved_state.grants);
            // Installed: a later session finds the approvals bound to this exact package.
            s.plugin_state.persist_stored().map_err(|e| crate::EngineError::Other(format!("“{}” is installed, but {e}", manifest.name)))?;
        }
        // Its entries resolve at once, as an inspected package's do.
        s.plugin_state.packages.insert(manifest.id.clone(), folder.to_string_lossy().into_owned());
    }
    let runtime = manifest.runtime.as_ref();
    serde_json::to_value(InstallResult {
        approval: plugin.approval.clone(),
        archive: plugin.archive.clone(),
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        capabilities: manifest.capabilities.clone(),
        permissions: runtime.map(|r| r.permissions.clone()).unwrap_or_default(),
        limits: runtime.map(|r| r.limits.clone()),
        host_limits: s.plugin_limits,
        commands: runtime.map(|r| r.commands.iter().cloned().collect()).unwrap_or_default(),
        entries: runtime.map(|r| r.entries.iter().map(|e| format!("{}.{}", manifest.id, e.name)).collect()).unwrap_or_default(),
        entry_points: runtime.map(|r| r.entries.clone()).unwrap_or_default(),
        listeners: runtime.map(|r| r.listeners.clone()).unwrap_or_default(),
        opens: runtime.map(|r| r.entries.iter().flat_map(|e| e.extensions.iter().cloned()).collect()).unwrap_or_default(),
        replaces,
        installed,
        folder: folder.to_string_lossy().into_owned(),
        param_warnings: plugin.swatch_warnings(),
    })
    .map_err(|e| bad("plugin.install", e.to_string()))
}
fn edit(_s: &mut Session, p: &Value) -> Result<Value> {
    let p: EditParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.edit", e.to_string()))?;
    if !plugin::valid_id(&p.plugin) || !plugin::valid_id(&p.entry) {
        return Err(bad("plugin.edit", "invalid plugin or entry id"));
    }
    Err(plugin::HostError::InteractiveRequired { entry: format!("{}.{}", p.plugin, p.entry) }.into())
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let p: RunParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.run", e.to_string()))?;
    // Called by name, an installed plugin is read with the app's authority (a tool call names
    // the plugin, not a path); anything else through the caller's file scope.
    let package = if p.named { plugin::with_installed(s, &p.path, || plugin::read_package(&p.path))? } else { plugin::read_package(&p.path)? };
    if p.approval != package.approval {
        return Err(bad("plugin.run", "approval does not match this manifest and component; inspect and review again"));
    }
    // Session::execute owns this transaction and exclusive access through its commit.
    // The command worker also validates its source session before accepting a computed edit.
    if let Some(expected) = p.expected_design {
        let actual = s.design_token();
        if expected != actual {
            return Err(craft_plugin::HostError::StaleRevision { expected, actual }.into());
        }
    }
    if let Some(guard) = &p.form_guard {
        let entry = package
            .manifest
            .runtime
            .as_ref()
            .and_then(|rt| rt.entries.iter().find(|e| e.name == p.entry))
            .ok_or_else(|| bad("plugin.run", "unknown entry"))?;
        let revision = s.design_revision();
        let errors = plugin::form_errors(s, &entry.input_schema, &p.params);
        let changed = guard.document_id != s.document_id || guard.revision != revision;
        if changed || !errors.is_empty() {
            return serde_json::to_value(RunResult {
                result: Value::Null,
                forms: vec![],
                form_validation: Some(FormValidation { revision, changed, errors }),
                sketch_exits: vec![],
                view: None,
                param_warnings: vec![],
            })
            .map_err(|e| bad("plugin.run", e.to_string()));
        }
    }
    let manifest = p.named.then(|| package.manifest.clone());
    let result = plugin::run(s, package, &p.entry, &p.params).map_err(|e| match &manifest {
        Some(manifest) => plugin::explain_named(s, manifest, &p.entry, e),
        None => e,
    })?;
    serde_json::to_value(result).map_err(|e| bad("plugin.run", e.to_string()))
}

fn tools(s: &mut Session, p: &Value) -> Result<Value> {
    let p: ToolsParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.tools", e.to_string()))?;
    serde_json::to_value(ToolsResult { tools: plugin::tools(s, p.name.as_deref()), param_warnings: vec![] })
        .map_err(|e| bad("plugin.tools", e.to_string()))
}

fn mesh(s: &mut Session, p: &Value) -> Result<Value> {
    let p: MeshParams = serde_json::from_value(p.clone()).map_err(|e| bad("model.mesh", e.to_string()))?;
    if !p.tolerance.is_finite() || p.tolerance <= 0. {
        return Err(bad("model.mesh", "tolerance must be finite and positive"));
    }
    let state = s.model.state();
    let body = state.body(&p.body).ok_or_else(|| bad("model.mesh", "unknown body"))?;
    let mesh = body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(p.tolerance))?;
    if mesh.positions.len() > 8192 || mesh.triangles.len() > 16384 {
        return Err(bad("model.mesh", "mesh exceeds the plugin transfer budget"));
    }
    let component = body.component;
    let result = MeshResult {
        body: p.body,
        component,
        frame: "component-local-mm".into(),
        positions: mesh.positions.iter().map(|v| [v.x, v.y, v.z]).collect(),
        triangles: mesh.triangles.to_vec(),
        param_warnings: vec![],
    };
    serde_json::to_value(result).map_err(|e| bad("model.mesh", e.to_string()))
}

fn list(s: &mut Session, _p: &Value) -> Result<Value> {
    // Discovery is inert; installed packages use the app's authority.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(root) = s.plugin_state.plugins_dir()
        && let Ok(folders) = std::fs::read_dir(root)
    {
        for folder in folders.flatten().take(1024) {
            let path = folder.path().to_string_lossy().into_owned();
            if let Ok(package) = plugin::with_installed(s, &path, || plugin::read_package(&path)) {
                s.plugin_state.register_discovered(package.manifest.id, path);
            }
        }
    }
    let mut plugins = Vec::new();
    for path in s.plugin_state.packages.values() {
        // An installed plugin is listed whatever folders the caller may use: the plugins folder
        // is the app's own (an inspected package elsewhere is read through the caller's scope).
        if let Ok(package) = plugin::with_installed(s, path, || plugin::read_package(path)) {
            let commands = package
                .manifest
                .runtime
                .as_ref()
                .map(|r| {
                    r.entries.iter().filter(|e| e.kind == plugin::EntryKind::Command).map(|e| format!("{}.{}", package.manifest.id, e.name)).collect()
                })
                .unwrap_or_default();
            plugins.push(ListedPlugin { disabled: s.disabled_plugins.contains(&package.manifest.id), manifest: package.manifest, commands });
        }
    }
    serde_json::to_value(ListResult { plugins, importers: plugin::describe_importers(s), param_warnings: vec![] })
        .map_err(|e| bad("plugin.list", e.to_string()))
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let p: ImportParams = serde_json::from_value(p.clone()).map_err(|e| bad("file.import", e.to_string()))?;
    let path = p.path.0;
    let importer = plugin::importer_for(s, &path, p.format.as_deref())?
        .ok_or_else(|| bad("file.import", "no installed, enabled importer plugin reads this file"))?;
    let imported = plugin::import_file(s, &importer, &path, &p.options).map_err(|e| match p.named {
        true => plugin::explain_importer(s, &importer, e),
        false => e,
    })?;
    serde_json::to_value(imported).map_err(|e| bad("file.import", e.to_string()))
}

fn grants(s: &mut Session, _p: &Value) -> Result<Value> {
    s.plugin_state.refresh_stored();
    let installed = s.plugin_state.plugins_dir();
    let grants = s
        .plugin_state
        .grants
        .iter()
        .map(|(id, grants)| {
            let folder = installed.as_ref().map(|dir| dir.join(id));
            let status = match folder {
                Some(folder) if folder.exists() => match plugin::read_package(&folder.to_string_lossy()) {
                    Ok(package) if grants.package_hash.as_deref() == Some(package.approval.as_str()) => GrantStatus::Current,
                    Ok(_) => GrantStatus::Changed,
                    Err(_) => GrantStatus::Unreadable,
                },
                _ => GrantStatus::NotInstalled,
            };
            StoredGrant {
                id: id.clone(),
                status,
                package_hash: grants.package_hash.clone(),
                command_groups: grants.command_groups.clone(),
                folders: grants.folders.clone(),
                memory_mib: grants.memory_mib,
                input_folder: grants.input_folder,
            }
        })
        .collect();
    serde_json::to_value(GrantsResult { grants, param_warnings: vec![] }).map_err(|e| bad("plugin.grants", e.to_string()))
}

fn revoke(s: &mut Session, p: &Value) -> Result<Value> {
    let p: RevokeParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.revoke", e.to_string()))?;
    let revoked = plugin::revoke(s, &p.id)?;
    serde_json::to_value(RevokeResult { id: p.id, revoked, param_warnings: vec![] }).map_err(|e| bad("plugin.revoke", e.to_string()))
}

fn remove(_s: &mut Session, p: &Value) -> Result<Value> {
    let p: RemoveParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.remove", e.to_string()))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        serde_json::to_value(plugin::remove(_s, &p.id)?).map_err(|e| bad("plugin.remove", e.to_string()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = p;
        Err(bad("plugin.remove", "plugin management is desktop only"))
    }
}
