//! Exporter plugins: an export slot (RFC 0015 §4) writes the open design as a file of an extension
//! it claims. The plugin reads the design through the read-only commands its manifest lists, on a
//! copy of the session that is thrown away, and returns the file's bytes; the app writes them to
//! the path the user chose, in one step, only when the whole export succeeds.
use super::import::{FormatSlot, checked_package, format_slots, slot_for};
use super::*;
use crate::schema::plugin::{ExportedFile, ImporterInfo};

/// Every installed, enabled export slot, sorted by id.
pub fn exporters(session: &Session) -> Vec<FormatSlot> {
    format_slots(session, EntryKind::Exporter)
}

/// The export slot `format` names (`plugin_id.entry`), or else the one that claims `path`'s
/// extension. Two plugins claiming one extension need `format`.
pub fn exporter_for(session: &Session, path: &str, format: Option<&str>) -> Result<Option<FormatSlot>> {
    slot_for(session, EntryKind::Exporter, path, format)
}

/// The export slots `plugin.list` and the Export menu describe.
pub fn describe(session: &Session) -> Vec<ImporterInfo> {
    exporters(session)
        .into_iter()
        .map(|e| ImporterInfo { id: e.id, name: e.name, label: e.label, extensions: e.extensions, input_schema: e.input_schema })
        .collect()
}

/// The commands an exporter may read the design with: read-only, in no sensitive group. Checked
/// when its manifest is read, so a plugin that asks for more is refused before it is installed.
pub(super) fn refuse_unreadable(runtime: &RuntimeManifest) -> Result<()> {
    for command in &runtime.commands {
        // An unknown command is refused next, with the message every plugin gets.
        let Some(spec) = catalog_command(command) else { continue };
        let readable = spec.effect == Effect::Read && command_group(command).is_none();
        if !readable {
            return Err(HostError::InvalidManifest {
                field: "runtime.commands".into(),
                reason: format!("an exporter reads the design: `{command}` changes it or reaches past it"),
            }
            .into());
        }
    }
    Ok(())
}

/// Write the open design to `path` with `exporter`. Nothing is written unless the whole export
/// succeeds, and then in one step: an export that fails, traps or runs out of fuel leaves the file
/// that was there (or no file) as it was. The design is never changed.
pub fn export_file(session: &mut Session, exporter: &FormatSlot, path: &str, options: &Value) -> Result<ExportedFile> {
    let file = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.into());
    convert(session, exporter, path, options).map_err(super::messages::named_export(&exporter.name, &file))
}

fn convert(session: &mut Session, exporter: &FormatSlot, path: &str, options: &Value) -> Result<ExportedFile> {
    if !options.is_object() {
        return Err(HostError::InvalidInput { field: "options".into(), reason: "the exporter's options are an object".into() }.into());
    }
    let file = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if file.is_empty() || file.starts_with('.') {
        return Err(HostError::InvalidInput { field: "path".into(), reason: "choose a named, visible file".into() }.into());
    }
    // The caller's file scope decides where the file may go; the plugin itself reaches no file.
    solidcraft_io::vfs::check(path).map_err(|e| HostError::PermissionDenied { permission: e.to_string() })?;
    let package = checked_package(session, exporter)?;
    let runtime = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    let declared = runtime.entries.iter().find(|entry| entry.name == exporter.entry).ok_or_else(|| error("unknown entry"))?;
    let prepared = if declared.dialog_hook {
        let protected = options.as_object().map(|v| v.keys().cloned().collect()).unwrap_or_default();
        let update = update_dialog(session, &package, &exporter.entry, options, &protected, true)?;
        if let Some(warning) = update.warning {
            session.echo(warning);
        }
        craft_plugin::prepare_input(&declared.input_schema, &update.values)?
    } else {
        craft_plugin::prepare_input(&declared.input_schema, options)?
    };
    let options = &prepared;
    let mut warnings = Vec::new();
    if exporter.needs_plugin_data {
        warnings.push(format!("{} asks for this design's plugin data, which exports don't pass yet; it exported without it", package.manifest.name));
    }
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
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
        // The guest's stack is on this thread's: run it on a thread with room for the stack the
        // exporter asks for and the host's own calls, as an import does.
        let stack = runtime.limits.stack_kib.map_or(512 * 1024, |kib| kib as usize * 1024).saturating_add(8 << 20);
        let progress = solidcraft_doc::progress::current();
        let result = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("plugin-export".into())
                .stack_size(stack)
                .spawn_scoped(scope, || {
                    // No command an exporter runs reaches a file, not even a font or a linked design.
                    let export = || {
                        solidcraft_io::vfs::scoped(Some(Default::default()), || {
                            craft_plugin_wasm::run_export(host, &package.manifest, &package.component, &exporter.entry, options).1
                        })
                    };
                    match progress {
                        Some(progress) => solidcraft_doc::progress::watch(progress, export),
                        None => export(),
                    }
                })
                .map_err(|e| HostError::Trap { reason: format!("could not start the export: {e}") })?
                .join()
                .map_err(|_| HostError::Trap { reason: "the export stopped unexpectedly".into() })?
        });
        let bytes = result?;
        if solidcraft_doc::progress::cancelled() {
            return Err(HostError::Cancelled.into());
        }
        solidcraft_io::write_atomic(Path::new(path), &bytes, false).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        Ok(ExportedFile {
            path: path.into(),
            bytes: bytes.len(),
            format: exporter.id.clone(),
            plugin: package.manifest.name.clone(),
            warnings,
            param_warnings: Vec::new(),
        })
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        let _ = (runtime, warnings, file, options);
        Err(error("exporting with a plugin needs the desktop app"))
    }
}
