//! Folder consent belongs to one entry invocation, never to a parameter or a saved design.
use super::*;

/// Trusted UI consent. No command accepts this type or deserialises it from JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogFolders {
    pub plugin: String,
    pub approval: String,
    pub entry: String,
    pub folders: Vec<FileGrant>,
}

/// Check path syntax without touching the filesystem (also used by the dialog each frame).
pub fn check_directory_path(path: &Path) -> craft_plugin::Result<()> {
    let text = path.to_string_lossy();
    if !path.is_absolute() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(HostError::InvalidInput {
            field: "folder".into(),
            reason: "choose an absolute local directory path of at most 4096 bytes".into(),
        });
    }
    if cfg!(windows)
        && let Some(reason) = solidcraft_io::vfs::windows_refusal(&text)
    {
        return Err(HostError::PermissionDenied { permission: reason.into() });
    }
    Ok(())
}

/// Resolve links in the selected root. The runtime confines subsequent access beneath its
/// opened directory handle. The definitive check runs again on the entry's worker.
pub fn directory(path: &Path) -> craft_plugin::Result<PathBuf> {
    check_directory_path(path)?;
    let text = path.to_string_lossy();
    solidcraft_io::vfs::check(&text).map_err(|e| HostError::PermissionDenied { permission: e.to_string() })?;
    let folder = std::fs::canonicalize(path).map_err(|e| HostError::InvalidInput { field: "folder".into(), reason: e.to_string() })?;
    if !folder.is_dir() {
        return Err(HostError::InvalidInput { field: "folder".into(), reason: "not a directory".into() });
    }
    solidcraft_io::vfs::check(&folder.to_string_lossy()).map_err(|e| HostError::PermissionDenied { permission: e.to_string() })?;
    Ok(folder)
}

pub(super) fn for_entry(
    session: &Session,
    package: &Package,
    entry: &str,
    params: &Value,
    consent: Option<&DialogFolders>,
) -> Result<Vec<FileGrant>> {
    let runtime = package.manifest.runtime.as_ref().ok_or_else(|| error("no executable runtime"))?;
    let declared = runtime.entries.iter().find(|e| e.name == entry).ok_or_else(|| error("unknown entry"))?;
    let fields = craft_plugin::directory_read_fields(&declared.input_schema)?;
    if fields.is_empty() {
        return Ok(Vec::new());
    }
    let consent = consent.filter(|c| c.plugin == package.manifest.id && c.approval == package.approval && c.entry == entry);
    let approved: Vec<_> = session
        .plugin_state
        .grants
        .get(&package.manifest.id)
        .filter(|g| g.package_hash.as_deref() == Some(package.approval.as_str()))
        .into_iter()
        .flat_map(|g| &g.folders)
        .filter(|g| g.read && runtime.permissions.files.iter().any(|r| r.name == g.name && r.read))
        .filter_map(|g| directory(&g.folder).ok().map(|folder| FileGrant { folder, ..g.clone() }))
        .collect();
    let mut folders = Vec::new();
    for name in fields {
        let Some(path) = params.get(&name).and_then(Value::as_str).filter(|p| !p.is_empty()) else { continue };
        let folder = directory(Path::new(path))?;
        let chosen = consent.is_some_and(|c| c.folders.iter().any(|g| g.name == name && g.read && !g.write && g.folder == folder));
        if !chosen {
            grant_covers_or(&approved, &folder, false, || messages::say("plugin_error.folder_option_read_approval", &[("option", &name)]))?;
        }
        folders.push(FileGrant { name, folder, read: true, write: false });
    }
    Ok(folders)
}
