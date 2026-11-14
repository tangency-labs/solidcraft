//! Importer plugins: a file of an extension an installed importer claims opens as a new, unsaved
//! design made by that importer, with its loss report. The importer reads only that file (and,
//! with the `input_folder` permission, the files beside it) and touches no open design.
use super::*;
use crate::schema::plugin::{ImportedDesign, ImporterInfo};

/// An installed, enabled importer entry, read from its manifest alone: no plugin code runs.
#[derive(Clone, Debug)]
pub struct Importer {
    /// `plugin_id.entry`, as `format` names it.
    pub id: String,
    pub plugin_id: String,
    pub entry: String,
    /// The plugin's name, for the Open dialog and the progress card.
    pub name: String,
    pub extensions: Vec<String>,
    pub input_schema: Value,
    /// The installed package's folder.
    pub folder: String,
}

/// Reads only `plugin.json` from a package folder, through the caller's file scope.
fn read_manifest(folder: &str) -> Result<Manifest> {
    let dir = Path::new(folder);
    let id = dir.file_name().and_then(|s| s.to_str()).ok_or_else(|| error("package needs an id folder"))?;
    let bytes = solidcraft_io::vfs::read(&dir.join("plugin.json").to_string_lossy(), MAX_MANIFEST_BYTES).map_err(|e| error(e.to_string()))?;
    parse_manifest(&bytes, id)
}

/// The installed plugins' folders, by id: the plugins folder, plus packages inspected this session.
fn package_folders(session: &Session) -> BTreeMap<String, String> {
    let mut folders = session.plugin_state.packages.clone();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(root) = session.plugin_state.plugins_dir()
        && let Ok(entries) = std::fs::read_dir(root)
    {
        for entry in entries.flatten().take(1024) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_id(&name) && entry.file_type().is_ok_and(|t| t.is_dir()) {
                folders.entry(name).or_insert_with(|| entry.path().to_string_lossy().into_owned());
            }
        }
    }
    folders
}

/// Every installed, enabled importer entry, sorted by id. Disabled plugins claim nothing.
pub fn importers(session: &Session) -> Vec<Importer> {
    let mut found = Vec::new();
    for (id, folder) in package_folders(session) {
        if session.disabled_plugins.contains(&id) {
            continue;
        }
        let Ok(manifest) = read_manifest(&folder) else { continue };
        let Some(runtime) = manifest.runtime.as_ref().filter(|r| r.is_importer()) else { continue };
        for entry in &runtime.entries {
            found.push(Importer {
                id: format!("{}.{}", manifest.id, entry.name),
                plugin_id: manifest.id.clone(),
                entry: entry.name.clone(),
                name: manifest.name.clone(),
                extensions: entry.extensions.clone(),
                input_schema: entry.input_schema.clone(),
                folder: folder.clone(),
            });
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

/// The importer for `path`: the one `format` names, or else the one that claims its extension.
/// Two plugins claiming the same extension need `format` to choose.
pub fn importer_for(session: &Session, path: &str, format: Option<&str>) -> Result<Option<Importer>> {
    let all = importers(session);
    if let Some(format) = format {
        return all
            .into_iter()
            .find(|i| i.id == format)
            .map(Some)
            .ok_or_else(|| HostError::InvalidInput { field: "format".into(), reason: format!("no installed, enabled importer `{format}`") }.into());
    }
    let extension = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
    let mut claiming: Vec<_> =
        all.into_iter().filter(|i| !extension.is_empty() && i.extensions.iter().any(|e| e.eq_ignore_ascii_case(extension))).collect();
    if claiming.len() > 1 {
        let ids = claiming.iter().map(|i| i.id.as_str()).collect::<Vec<_>>().join(", ");
        return Err(
            HostError::InvalidInput { field: "format".into(), reason: format!("{ids} all read .{extension} files; name one in `format`") }.into()
        );
    }
    Ok(claiming.pop())
}

/// What the Open dialog offers for installed importers: each plugin's name and extensions.
pub fn open_filters(session: &Session) -> Vec<(String, Vec<String>)> {
    let mut filters: Vec<(String, Vec<String>)> = Vec::new();
    for importer in importers(session) {
        match filters.iter_mut().find(|(name, _)| *name == importer.name) {
            Some((_, extensions)) => extensions.extend(importer.extensions.into_iter().filter(|e| !extensions.contains(e)).collect::<Vec<_>>()),
            None => filters.push((importer.name, importer.extensions)),
        }
    }
    filters
}

/// The importers `plugin.list` and the Open dialog describe.
pub fn describe(session: &Session) -> Vec<ImporterInfo> {
    importers(session).into_iter().map(|i| ImporterInfo { id: i.id, name: i.name, extensions: i.extensions, input_schema: i.input_schema }).collect()
}

/// Run `importer` on `path` and make its design this session's, as a new design that is not saved
/// anywhere yet. Nothing changes unless the whole import succeeds; a trap names the plugin and file.
pub fn import_file(session: &mut Session, importer: &Importer, path: &str, options: &Value) -> Result<ImportedDesign> {
    let file = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.into());
    let package = read_package(&importer.folder)?;
    if package.manifest.id != importer.plugin_id || session.disabled_plugins.contains(&importer.plugin_id) {
        return Err(error("plugin is disabled or was replaced; open the file again"));
    }
    if let Some(grants) = session.plugin_state.grants.get(&package.manifest.id)
        && grants.package_hash.as_deref() != Some(package.approval.as_str())
    {
        return Err(HostError::PermissionDenied {
            permission: "stored plugin grants need re-approval for this manifest and component; install and review again".into(),
        }
        .into());
    }
    let runtime = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    host_limits_for(session, &package.manifest.id, &package.approval).check(&runtime.limits)?;
    if !options.is_object() {
        return Err(HostError::InvalidInput { field: "options".into(), reason: "the importer's options are an object".into() }.into());
    }
    // The caller's file scope covers the file, and with input_folder, the folder it is in. The
    // folder is read only when the manifest asks for it and the user approved it at install, for
    // this exact package (the grant above).
    solidcraft_io::vfs::check(path).map_err(|e| HostError::PermissionDenied { permission: e.to_string() })?;
    let granted = session.plugin_state.grants.get(&package.manifest.id).is_some_and(|grants| grants.input_folder);
    let input_folder = runtime.permissions.input_folder
        && granted
        && Path::new(path).parent().is_some_and(|folder| solidcraft_io::vfs::check(&folder.to_string_lossy()).is_ok());
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let request = craft_plugin_wasm::ImportRequest { path: PathBuf::from(path), input_folder };
        let host = SolidCraftHost {
            session: Session::default(),
            transaction: None,
            sketch_exits: Vec::new(),
            groups: BTreeSet::new(),
            file_folders: Vec::new(),
        };
        // The guest's stack is on this thread's: run it on a thread with room for the stack the
        // importer asks for (a real decoder needs about 16 MiB) and the host's own calls.
        let stack = runtime.limits.stack_kib.map_or(512 * 1024, |kib| kib as usize * 1024).saturating_add(8 << 20);
        let progress = solidcraft_doc::progress::current();
        let result = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("plugin-import".into())
                .stack_size(stack)
                .spawn_scoped(scope, || {
                    let import = || craft_plugin_wasm::run_import(host, &package.manifest, &package.component, &importer.entry, options, &request).1;
                    // The worker's progress card and Cancel follow the import onto this thread.
                    match progress {
                        Some(progress) => solidcraft_doc::progress::watch(progress, import),
                        None => import(),
                    }
                })
                .map_err(|e| HostError::Trap { reason: format!("could not start the import: {e}") })?
                .join()
                .map_err(|_| HostError::Trap { reason: "the import stopped unexpectedly".into() })?
        });
        let imported = result.map_err(|e| match e {
            HostError::Trap { reason } => HostError::Trap { reason: format!("{} failed on {file}: {reason}", package.manifest.name) },
            e => e,
        })?;
        if solidcraft_doc::progress::cancelled() {
            return Err(HostError::Cancelled.into());
        }
        let mut doc = solidcraft_io::read_design(&imported.design).map_err(|e| HostError::InvalidInput {
            field: "design".into(),
            reason: format!("{} wrote a design SolidCraft can't read: {e}", package.manifest.name),
        })?;
        if let Some(stem) = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned())
            && doc.name.trim().is_empty()
        {
            doc.name = stem;
        }
        let mut fresh = Session::opened_at_validating(doc, None, None, session.validation());
        fresh.undo.clear();
        fresh.redo.clear();
        fresh.mark_unsaved();
        session.replace_design(fresh);
        Ok(ImportedDesign {
            path: path.into(),
            format: importer.id.clone(),
            plugin: importer.name.clone(),
            name: session.doc.name.clone(),
            features: session.doc.features.len(),
            errors: session.model.results.iter().filter(|r| r.error.is_some()).count(),
            bodies: session.model.state().bodies.len(),
            loss_report: imported.report,
            param_warnings: Vec::new(),
        })
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = (package, input_folder, file);
        Err(error("opening a file with an importer plugin needs the desktop app"))
    }
}
