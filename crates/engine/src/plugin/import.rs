//! Importer plugins: a file of an extension an installed importer claims opens as a new, unsaved
//! design made by that importer, with its loss report. The importer reads only that file (and,
//! with the `input_folder` permission, the files beside it) and touches no open design.
use super::*;
use crate::schema::plugin::{ImportedDesign, ImporterInfo};

/// An installed, enabled plugin's file-format slot (RFC 0015 §4), read from its manifest alone:
/// no plugin code runs. Import and export slots share this one model.
#[derive(Clone, Debug)]
pub struct FormatSlot {
    /// `plugin_id.entry`, as `format` names it.
    pub id: String,
    pub plugin_id: String,
    pub entry: String,
    pub kind: EntryKind,
    /// The plugin's name, for the Open and Export lists and the progress card.
    pub name: String,
    /// The slot's own label, or empty for the plugin's name.
    pub label: String,
    pub extensions: Vec<String>,
    /// The options drawn as a form before the import or export.
    pub input_schema: Value,
    /// Export only: the export would read the design's plugin data (not passed yet).
    pub needs_plugin_data: bool,
    /// The installed package's folder.
    pub folder: String,
}
/// An import slot.
pub type Importer = FormatSlot;

/// Reads only `plugin.json` from a package folder, through the caller's file scope.
pub(super) fn read_manifest(folder: &str) -> Result<Manifest> {
    let dir = Path::new(folder);
    let id = dir.file_name().and_then(|s| s.to_str()).ok_or_else(|| error("package needs an id folder"))?;
    let bytes = solidcraft_io::vfs::read(&dir.join("plugin.json").to_string_lossy(), MAX_MANIFEST_BYTES).map_err(|e| error(e.to_string()))?;
    parse_manifest(&bytes, id)
}

/// The installed plugins' folders, by id: the plugins folder, plus packages inspected this session.
pub(super) fn package_folders(session: &Session) -> BTreeMap<String, String> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
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

/// Every installed, enabled plugin's slots of `kind`, sorted by id. Disabled plugins claim nothing.
pub fn format_slots(session: &Session, kind: EntryKind) -> Vec<FormatSlot> {
    let mut found = Vec::new();
    for (id, folder) in package_folders(session) {
        if session.disabled_plugins.contains(&id) {
            continue;
        }
        // An installed format slot is read with the app's authority: a caller names a file, not it.
        let Ok(manifest) = with_installed(session, &folder, || read_manifest(&folder)) else { continue };
        let Some(runtime) = manifest.runtime.as_ref() else { continue };
        for entry in runtime.entries.iter().filter(|e| e.kind == kind && kind.is_format()) {
            found.push(FormatSlot {
                id: format!("{}.{}", manifest.id, entry.name),
                plugin_id: manifest.id.clone(),
                entry: entry.name.clone(),
                kind,
                name: manifest.name.clone(),
                label: entry.label.clone(),
                extensions: entry.extensions.clone(),
                input_schema: entry.input_schema.clone(),
                needs_plugin_data: entry.needs_plugin_data,
                folder: folder.clone(),
            });
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

/// Every installed, enabled importer entry, sorted by id. Disabled plugins claim nothing.
pub fn importers(session: &Session) -> Vec<Importer> {
    format_slots(session, EntryKind::Importer)
}

/// Whether the importer plugin a link names can read now, and which version of it is installed.
pub struct ImporterStatus {
    /// The installed plugin's version, when its manifest can be read.
    pub installed_version: Option<String>,
    /// Why it can't read: `not installed`, `turned off`, `unreadable` or `not offering that importer`.
    pub problem: Option<&'static str>,
}

/// The state of the importer `format` of plugin `plugin`, from manifests alone: no plugin code runs.
pub fn importer_status(session: &Session, plugin: &str, format: &str) -> ImporterStatus {
    let folders = package_folders(session);
    let Some(folder) = folders.get(plugin) else { return ImporterStatus { installed_version: None, problem: Some("not installed") } };
    let installed_version = with_installed(session, folder, || read_manifest(folder)).ok().map(|m| m.version);
    let problem = if session.disabled_plugins.contains(plugin) {
        Some("turned off")
    } else if installed_version.is_none() {
        Some("unreadable")
    } else if !importers(session).iter().any(|i| i.id == format && i.plugin_id == plugin) {
        Some("not offering that importer")
    } else {
        None
    };
    ImporterStatus { installed_version, problem }
}

/// The slot of `kind` that `format` names, or else the one that claims `path`'s extension. Two
/// plugins claiming the same extension need `format` to choose.
pub(super) fn slot_for(session: &Session, kind: EntryKind, path: &str, format: Option<&str>) -> Result<Option<FormatSlot>> {
    let what = if kind == EntryKind::Exporter { "exporter" } else { "importer" };
    let all = format_slots(session, kind);
    if let Some(format) = format {
        return all
            .into_iter()
            .find(|i| i.id == format)
            .map(Some)
            .ok_or_else(|| HostError::InvalidInput { field: "format".into(), reason: format!("no installed, enabled {what} `{format}`") }.into());
    }
    let extension = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
    let mut claiming: Vec<_> =
        all.into_iter().filter(|i| !extension.is_empty() && i.extensions.iter().any(|e| e.eq_ignore_ascii_case(extension))).collect();
    if claiming.len() > 1 {
        let ids = claiming.iter().map(|i| i.id.as_str()).collect::<Vec<_>>().join(", ");
        let verb = if kind == EntryKind::Exporter { "write" } else { "read" };
        return Err(
            HostError::InvalidInput { field: "format".into(), reason: format!("{ids} all {verb} .{extension} files; name one in `format`") }.into()
        );
    }
    Ok(claiming.pop())
}

/// The package behind `slot`, checked as it is now: still enabled, the same plugin, its grants for
/// this exact package and its limits within the user's.
pub(super) fn checked_package(session: &mut Session, slot: &FormatSlot) -> Result<Package> {
    session.plugin_state.refresh_stored();
    let package = with_installed(session, &slot.folder, || read_package(&slot.folder))?;
    if package.manifest.id != slot.plugin_id || session.disabled_plugins.contains(&slot.plugin_id) {
        return Err(error("plugin is disabled or was replaced; try again"));
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
    Ok(package)
}

/// The importer for `path`: the one `format` names, or else the one that claims its extension.
/// Two plugins claiming the same extension need `format` to choose.
pub fn importer_for(session: &Session, path: &str, format: Option<&str>) -> Result<Option<Importer>> {
    slot_for(session, EntryKind::Importer, path, format)
}

/// What the Open dialog offers for installed importers: each plugin's name and extensions.
pub fn open_filters(session: &Session) -> Vec<(String, Vec<String>)> {
    let mut filters: Vec<(String, Vec<String>)> = Vec::new();
    for importer in importers(session) {
        // A slot's own label names its group; without one, the plugin's name does.
        let name = if importer.label.is_empty() { importer.name } else { importer.label };
        match filters.iter_mut().find(|(group, _)| *group == name) {
            Some((_, extensions)) => extensions.extend(importer.extensions.into_iter().filter(|e| !extensions.contains(e)).collect::<Vec<_>>()),
            None => filters.push((name, importer.extensions)),
        }
    }
    filters
}

/// The importers `plugin.list` and the Open dialog describe.
pub fn describe(session: &Session) -> Vec<ImporterInfo> {
    importers(session)
        .into_iter()
        .map(|i| ImporterInfo { id: i.id, name: i.name, label: i.label, extensions: i.extensions, input_schema: i.input_schema })
        .collect()
}

/// Run `importer` on `path` and make its design this session's, as a new design that is not saved
/// anywhere yet. Nothing changes unless the whole import succeeds; a failure names the plugin and file.
pub fn import_file(session: &mut Session, importer: &Importer, path: &str, options: &Value) -> Result<ImportedDesign> {
    let consent = session.plugin_state.user.dialog_folders.take();
    let file = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.into());
    convert(session, importer, path, options, consent.as_ref()).map_err(super::messages::named(&importer.name, Some(&file)))
}

/// All callers receive schema defaults for omitted options. User preferences belong to the
/// interactive form only; explicit values (including null) stay the caller's own, and the schema checks them.
fn with_schema_defaults(schema: &Value, options: &Value) -> Value {
    let mut defaults = options.clone();
    if let Some(values) = defaults.as_object_mut()
        && let Some(properties) = schema.get("properties").and_then(Value::as_object)
    {
        for (key, property) in properties {
            if let Some(default) = property.get("default") {
                values.entry(key.clone()).or_insert_with(|| default.clone());
            }
        }
    }
    defaults
}

// What an importer wrote: the design's bytes and, in the desktop app, what it could not carry over.
#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
use craft_plugin_wasm::Imported;
#[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
#[allow(dead_code)]
struct Imported {
    design: Vec<u8>,
}

/// Run `importer` on `path` as the caller's file scope allows, without touching the open design: what
/// the importer wrote, the version of the plugin that ran, and what `check` made of the output. The
/// plugin's staged user settings are published only after `check` accepts the output.
fn run_importer<R>(
    session: &mut Session,
    importer: &Importer,
    path: &str,
    options: &Value,
    consent: Option<&folders::DialogFolders>,
    check: impl FnOnce(&Session, &Imported) -> Result<R>,
) -> Result<(Imported, String, R)> {
    let package = checked_package(session, importer)?;
    let runtime = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    if !options.is_object() {
        return Err(HostError::InvalidInput { field: "options".into(), reason: "the importer's options are an object".into() }.into());
    }
    // Use the checked package, not a schema cached when discovery or a dialog ran.
    let declared = runtime.entries.iter().find(|e| e.name == importer.entry).ok_or_else(|| error("unknown importer entry"))?;
    let facts = conditions::uses_file_facts(&declared.input_schema).then(|| file_facts(path, &importer.id, 0)).transpose()?;
    let schema = conditions::bind_file_conditions(&declared.input_schema, facts.as_ref())?;
    let prepared = if declared.dialog_hook {
        let protected = options.as_object().map(|v| v.keys().cloned().collect()).unwrap_or_default();
        let update = update_dialog_with_schema(session, &package, &importer.entry, options, &protected, true, &schema)?;
        if let Some(warning) = update.warning {
            session.echo(warning);
        }
        craft_plugin::prepare_input(&schema, &with_schema_defaults(&schema, &update.values))?
    } else {
        craft_plugin::prepare_input(&schema, &with_schema_defaults(&schema, options))?
    };
    let options = &prepared;
    let input_folders = folders::for_entry(session, &package, &importer.entry, options, consent)?;
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
        let user_storage = package.manifest.granted_capabilities().iter().any(|c| c.starts_with("user-storage@"));
        let mut scratch = Session::default();
        scratch.plugin_state.user.user_settings_dir = session.plugin_state.user_settings_dir();
        scratch.plugin_state.user.user_storage_caller = user_storage.then(|| package.manifest.id.clone());
        scratch.plugin_state.user.defer_user_settings = session.plugin_state.user.defer_user_settings;
        scratch.plugin_state.user.pending_user_settings = session.plugin_state.user.pending_user_settings.clone();
        let host = SolidCraftHost {
            session: scratch,
            input_folders,
            transaction: None,
            sketch_exits: Vec::new(),
            view: None,
            groups: BTreeSet::new(),
            file_folders: Vec::new(),
            file_read_folders: Vec::new(),
            evaluation: None,
            forms: Vec::new(),
            form_source: None,
            messages: Vec::new(),
            user_data: None,
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
                    let import = || craft_plugin_wasm::run_import(host, &package.manifest, &package.component, &importer.entry, options, &request);
                    // The worker's progress card and Cancel follow the import onto this thread.
                    match progress {
                        Some(progress) => solidcraft_doc::progress::watch(progress, import),
                        None => import(),
                    }
                })
                .map_err(|e| HostError::Trap { reason: format!("could not start the import: {e}") })?
                .join()
                .map_err(|_| HostError::Trap { reason: "the import stopped unexpectedly".into() })
        });
        // Keep settings staged while the caller validates the output. Any early return drops the
        // private host and its pending settings without publishing them.
        let (mut host, imported) = result?;
        let imported = imported?;
        if solidcraft_doc::progress::cancelled() {
            return Err(HostError::Cancelled.into());
        }
        let checked = check(session, &imported)?;
        // Cancel pressed while the caller evaluated the output leaves everything alone.
        if solidcraft_doc::progress::cancelled() {
            return Err(HostError::Cancelled.into());
        }
        if user_storage {
            host.commit_transaction()?;
            host.publish_committed()?;
            session.plugin_state.user.pending_user_settings = host.session.plugin_state.user.pending_user_settings;
        }
        Ok((imported, package.manifest.version.clone(), checked))
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = (package, input_folder, options, input_folders, check);
        Err(error("opening a file with an importer plugin needs the desktop app"))
    }
}

/// What an importer wrote for a linked import: the design as file bytes, the version of the plugin
/// that wrote it, and what it could not carry over, one line each.
pub struct ImportedBytes {
    pub design: Vec<u8>,
    pub version: String,
    pub notes: Vec<String>,
}

/// Run `importer` on `path` and give back the design it wrote. The open design is not touched: a
/// linked import takes its bodies from the result. A failure names the plugin and file.
pub fn import_design_bytes(session: &mut Session, importer: &Importer, path: &str, options: &Value) -> Result<ImportedBytes> {
    let consent = session.plugin_state.user.dialog_folders.take();
    let file = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.into());
    let (imported, version, ()) = run_importer(session, importer, path, options, consent.as_ref(), |_, _| Ok(()))
        .map_err(super::messages::named(&importer.name, Some(&file)))?;
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    let notes = {
        let report = &imported.report;
        let items =
            report.items.iter().take(20).map(|i| if i.subject.is_empty() { i.message.clone() } else { format!("{}: {}", i.subject, i.message) });
        let lines: Vec<String> = std::iter::once(report.summary.clone()).filter(|s| !s.is_empty()).chain(items).collect();
        lines.into_iter().map(|l| format!("{}: {l}", importer.name)).collect()
    };
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    let notes = Vec::new();
    Ok(ImportedBytes { design: imported.design, version, notes })
}

fn convert(
    session: &mut Session,
    importer: &Importer,
    path: &str,
    options: &Value,
    consent: Option<&folders::DialogFolders>,
) -> Result<ImportedDesign> {
    let (imported, _, fresh) = run_importer(session, importer, path, options, consent, |session, imported| -> Result<Session> {
        #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            let mut doc = solidcraft_io::read_design(&imported.design)
                .map_err(|e| HostError::InvalidInput { field: "design".into(), reason: format!("it wrote a design SolidCraft can't read: {e}") })?;
            if let Some(stem) = Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned())
                && doc.name.trim().is_empty()
            {
                doc.name = stem;
            }
            let mut fresh = Session::opened_at_validating(doc, None, None, session.validation());
            fresh.undo.clear();
            fresh.redo.clear();
            fresh.mark_unsaved();
            Ok(fresh)
        }
        #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
        {
            let _ = (session, imported);
            Err(error("opening a file with an importer plugin needs the desktop app"))
        }
    })?;
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
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
        let _ = (imported, fresh);
        Err(error("opening a file with an importer plugin needs the desktop app"))
    }
}

/// Bounded facts about the selected file. Paths stay with the host; forms receive no parent path.
pub fn file_facts(path: &str, format: &str, generation: u64) -> Result<dialog::FileFacts> {
    solidcraft_io::vfs::check(path).map_err(|e| HostError::PermissionDenied { permission: e.to_string() })?;
    let path_ref = Path::new(path);
    let name = path_ref.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path_ref.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if name.is_empty()
        || name.len() > 255
        || name.starts_with('.')
        || name.chars().any(char::is_control)
        || extension.len() > 32
        || format.len() > 161
    {
        return Err(HostError::InvalidInput { field: "path".into(), reason: "choose a named, visible file with a bounded extension".into() }.into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !std::fs::symlink_metadata(path).map_err(|e| error(e.to_string()))?.is_file() {
        return Err(HostError::InvalidInput { field: "path".into(), reason: "choose a regular file, without a link".into() }.into());
    }
    let size = solidcraft_io::vfs::len(path).map_err(|e| error(e.to_string()))?;
    if size > craft_plugin::MAX_IMPORT_INPUT_BYTES {
        return Err(HostError::LimitExceeded { limit: "source: the file is larger than 1 GiB".into() }.into());
    }
    Ok(dialog::FileFacts { name, extension, size, format: format.into(), generation })
}

/// A dialog result belongs to one file generation; the UI checks it before adopting its fields.
#[derive(Debug)]
pub struct ImportDialog {
    pub file: dialog::FileFacts,
    pub schema: Value,
    pub hook: bool,
}

/// Read the selected file's facts and, only for an explicit interactive form, run its bounded
/// dialog hook. Direct file imports, scripts and MCP never call this function.
#[allow(clippy::too_many_arguments)]
pub fn import_dialog(
    session: &Session,
    folder: &str,
    approval: &str,
    entry: &str,
    path: &str,
    generation: u64,
    values: &Value,
) -> Result<ImportDialog> {
    let package = with_installed(session, folder, || read_package(folder))?;
    if package.approval != approval || session.disabled_plugins.contains(&package.manifest.id) {
        return Err(error("plugin is disabled or was replaced; open the file again"));
    }
    if let Some(grants) = session.plugin_state.grants.get(&package.manifest.id)
        && grants.package_hash.as_deref() != Some(approval)
    {
        return Err(HostError::PermissionDenied { permission: "stored plugin grants need re-approval for this package".into() }.into());
    }
    let runtime = package.manifest.runtime.as_ref().filter(|r| r.is_importer()).ok_or_else(|| error("not an importer"))?;
    host_limits_for(session, &package.manifest.id, approval).check(&runtime.limits)?;
    let entry = runtime.entries.iter().find(|e| e.name == entry).ok_or_else(|| error("unknown importer entry"))?;
    let file = file_facts(path, &format!("{}.{}", package.manifest.id, entry.name), generation)?;
    let schema = conditions::bind_file_conditions(&entry.input_schema, Some(&file))?;
    let hook = !entry.dialog_hook && package.manifest.granted_capabilities().iter().any(|c| c.starts_with("dialog@"));
    if !hook {
        return Ok(ImportDialog { file, schema, hook });
    }
    if !values.is_object() || values.to_string().len() > dialog::MAX_UPDATE_BYTES {
        return Err(HostError::InvalidInput { field: "dialog.values".into(), reason: "requires an object of at most 64 KiB".into() }.into());
    }
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let mut scratch = Session::default();
        scratch.plugin_state.user.user_settings_dir = session.plugin_state.user_settings_dir();
        scratch.plugin_state.user.user_storage_caller =
            package.manifest.granted_capabilities().iter().any(|c| c.starts_with("user-storage@")).then(|| package.manifest.id.clone());
        let host = SolidCraftHost {
            session: scratch,
            input_folders: Vec::new(),
            transaction: None,
            sketch_exits: Vec::new(),
            view: None,
            groups: BTreeSet::new(),
            file_folders: Vec::new(),
            file_read_folders: Vec::new(),
            evaluation: None,
            forms: Vec::new(),
            form_source: None,
            messages: Vec::new(),
            user_data: None,
        };
        let request = craft_plugin_wasm::ImportRequest { path: PathBuf::from(path), input_folder: false };
        let context = json!({"values":values,"file":file});
        let stack = runtime.limits.stack_kib.map_or(512 * 1024, |kib| kib as usize * 1024).saturating_add(8 << 20);
        let update = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("plugin-dialog".into())
                .stack_size(stack)
                .spawn_scoped(scope, || {
                    craft_plugin_wasm::update_fields(host, &package.manifest, &package.component, &entry.name, &context, &request).1
                })
                .map_err(|e| HostError::Trap { reason: format!("could not start the dialog hook: {e}") })?
                .join()
                .map_err(|_| HostError::Trap { reason: "dialog hook stopped unexpectedly".into() })?
        })?;
        Ok(ImportDialog { file, schema: dialog::apply_update(&schema, &update)?, hook })
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        Err(error("dialog hooks need the desktop app"))
    }
}
