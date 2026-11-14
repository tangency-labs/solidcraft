//! One-shot plugins share the engine's command surface, file scope and undo history.
use super::CommandSpec;
use crate::schema::plugin::{
    InspectParams, InspectResult, InstallParams, InstallResult, ListParams, ListResult, ListedPlugin, MeshParams, MeshResult, RemoveParams,
    RemoveResult, RunParams, RunResult,
};
use crate::{
    Result, Session, checked,
    params::bad,
    plugin,
    schema::{TypedContract, output_schema_for},
};
use serde_json::Value;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("model.mesh", "Read Body Mesh", checked!("model.mesh", mesh)).icon("inspect").noundo().read_only()
        .contract(&TypedContract::<MeshParams>::new()).output(output_schema_for::<MeshResult>)
        .params("body, tolerance?: finite positive chord tolerance in mm (default 0.05); evaluated component-local mesh, at most 8192 vertices and 16384 triangles; no files or model changes"),
    CommandSpec::new("plugin.inspect", "Inspect Plugin", checked!("plugin.inspect", inspect)).icon("inspect").noundo().doc_only()
        .contract(&TypedContract::<InspectParams>::new()).output(output_schema_for::<InspectResult>)
        .params("path: plugin package folder; returns inert manifest and approval hash covering manifest and component; runs no plugin code"),
    CommandSpec::new("plugin.install", "Install Plugin", checked!("plugin.install", install)).icon("import").noundo().doc_only()
        .contract(&TypedContract::<InstallParams>::new()).output(output_schema_for::<InstallResult>)
        .params("path: plugin .zip, approval?: exact hash from a reviewed dry run (mismatch refuses installation), archive?: exact .zip hash from that dry run (any changed file refuses installation), replace?: bool (required to replace an installed plugin of the same id), dry_run?: bool, approve?: {command_groups, folders: [{name, path, read, write}]}; validates requested approvals and checks the .zip (one plugin, safe paths, size caps, manifest) and unpacks it into the plugins folder; runs no plugin code"),
    CommandSpec::new("plugin.list", "List Plugins", checked!("plugin.list", list)).icon("inspect").noundo().doc_only()
        .contract(&TypedContract::<ListParams>::new()).output(output_schema_for::<ListResult>)
        .params("List inspected and installed plugin manifests, namespaced entries, declared effects and input schemas; no plugin code runs"),
    CommandSpec::new("plugin.remove", "Remove Plugin", checked!("plugin.remove", remove)).icon("delete").noundo().doc_only()
        .contract(&TypedContract::<RemoveParams>::new()).output(output_schema_for::<RemoveResult>)
        .params("id: installed plugin id; removes its folder and stored grants without executing code"),
    CommandSpec::new("plugin.run", "Run Plugin", checked!("plugin.run", run)).icon("play")
        .contract(&TypedContract::<RunParams>::new()).output(output_schema_for::<RunResult>)
        .params("path, approval: exact hash from plugin.inspect after reviewing permissions, entry, params?: JSON; explicit native Wasm component execution, one undo step; errors discard all edits"),
];
fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let p: InspectParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.inspect", e.to_string()))?;
    let package = plugin::read_package(&p.path)?;
    s.plugin_state.packages.insert(package.manifest.id.clone(), p.path.clone());
    let limits_error = package.manifest.runtime.as_ref().and_then(|r| s.plugin_limits.check(&r.limits).err()).map(|e| e.to_string());
    serde_json::to_value(InspectResult {
        manifest: package.manifest,
        approval: package.approval,
        host_limits: s.plugin_limits,
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
        commands: runtime.map(|r| r.commands.iter().cloned().collect()).unwrap_or_default(),
        entries: runtime.map(|r| r.entries.iter().map(|e| format!("{}.{}", manifest.id, e.name)).collect()).unwrap_or_default(),
        entry_points: runtime.map(|r| r.entries.clone()).unwrap_or_default(),
        replaces,
        installed,
        folder: folder.to_string_lossy().into_owned(),
        param_warnings: vec![],
    })
    .map_err(|e| bad("plugin.install", e.to_string()))
}
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let p: RunParams = serde_json::from_value(p.clone()).map_err(|e| bad("plugin.run", e.to_string()))?;
    let package = plugin::read_package(&p.path)?;
    if p.approval != package.approval {
        return Err(bad("plugin.run", "approval does not match this manifest and component; inspect and review again"));
    }
    let result = plugin::run(s, package, &p.entry, &p.params)?;
    serde_json::to_value(result).map_err(|e| bad("plugin.run", e.to_string()))
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
    // Discovery is inert, and uses the caller's file scope when reading each package.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(root) = s.plugin_state.plugins_dir()
        && let Ok(folders) = std::fs::read_dir(root)
    {
        for folder in folders.flatten().take(1024) {
            let path = folder.path().to_string_lossy().into_owned();
            if let Ok(package) = plugin::read_package(&path) {
                s.plugin_state.register_discovered(package.manifest.id, path);
            }
        }
    }
    let mut plugins = Vec::new();
    for path in s.plugin_state.packages.values() {
        if let Ok(package) = plugin::read_package(path) {
            let commands = package
                .manifest
                .runtime
                .as_ref()
                .map(|r| r.entries.iter().map(|e| format!("{}.{}", package.manifest.id, e.name)).collect())
                .unwrap_or_default();
            plugins.push(ListedPlugin { disabled: s.disabled_plugins.contains(&package.manifest.id), manifest: package.manifest, commands });
        }
    }
    serde_json::to_value(ListResult { plugins, param_warnings: vec![] }).map_err(|e| bad("plugin.list", e.to_string()))
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
