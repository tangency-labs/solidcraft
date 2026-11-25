//! Validation and storage of approved grants, and native plugin removal.
use super::*;
use crate::schema::plugin::RemoveResult;

fn invalid(reason: impl Into<String>) -> EngineError {
    HostError::InvalidInput { field: "package".into(), reason: reason.into() }.into()
}
fn io(e: impl std::fmt::Display) -> EngineError {
    invalid(e.to_string())
}
fn check(path: &Path) -> Result<()> {
    solidcraft_io::vfs::check(&path.to_string_lossy()).map_err(|e| HostError::PermissionDenied { permission: e.to_string() }.into())
}
fn requested(manifest: &Manifest) -> Grants {
    manifest.runtime.as_ref().map_or_else(Grants::default, |rt| Grants {
        package_hash: None,
        command_groups: rt.permissions.command_groups.clone(),
        folders: rt
            .permissions
            .files
            .iter()
            .map(|g| FileGrant { name: g.name.clone(), folder: PathBuf::new(), read: g.read, write: g.write })
            .collect(),
        fuel: Some(rt.limits.fuel),
        memory_mib: Some(rt.limits.memory_mib),
        input_folder: rt.permissions.input_folder,
        home_files: !rt.permissions.home_files.is_empty(),
    })
}
pub fn validate_grants(manifest: &Manifest, grants: &Grants) -> Result<()> {
    let asked = requested(manifest);
    let deny = || HostError::PermissionDenied { permission: "approval contains an unrequested, duplicate, invalid or excessive grant".into() };
    let picked = |name: &str| manifest.runtime.as_ref().is_some_and(|rt| rt.permissions.files.iter().any(|r| r.name == name && r.picked));
    let chosen_at_install = grants.folders.iter().filter(|grant| !picked(&grant.name)).count();
    if grants.command_groups.len() > 4
        || chosen_at_install > 32
        || grants.folders.len() > 32 + MAX_PICKED_FOLDERS * 2
        || !grants.command_groups.is_subset(&asked.command_groups)
    {
        return Err(deny().into());
    }
    if (grants.input_folder && !asked.input_folder) || (grants.home_files && !asked.home_files) {
        return Err(deny().into());
    }
    // Fuel, like memory, approves exactly the manifest request within a fixed host ceiling.
    if grants.fuel.is_some_and(|fuel| asked.fuel != Some(fuel) || !(1..=MAX_APPROVED_FUEL).contains(&fuel)) {
        return Err(deny().into());
    }
    // Memory is approved as the manifest asks for it: no more, and never past the Wasm32 limit.
    if grants.memory_mib.is_some_and(|memory| asked.memory_mib != Some(memory) || !(1..=MAX_MEMORY_MIB).contains(&memory)) {
        return Err(deny().into());
    }
    let mut names = BTreeSet::new();
    let mut folders = BTreeSet::new();
    for grant in &grants.folders {
        // A grant whose folders the user picks has one record per folder, each folder once.
        let picked = picked(&grant.name);
        let fresh = if picked { folders.insert((&grant.name, &grant.folder)) } else { names.insert(&grant.name) };
        if !fresh
            || (picked && grants.folders.iter().filter(|other| other.name == grant.name).count() > MAX_PICKED_FOLDERS)
            || !grant.folder.is_absolute()
            || grant.folder.to_string_lossy().len() > 4096
            || grant.folder.to_string_lossy().chars().any(char::is_control)
            || (!grant.read && !grant.write)
            || !asked.folders.iter().any(|r| r.name == grant.name && (!grant.read || r.read) && (!grant.write || r.write))
        {
            return Err(deny().into());
        }
        check(&grant.folder)?;
        // A picked folder was checked when it was picked; one moved or deleted since is skipped
        // at run time rather than taking the plugin's other approvals with it.
        if !picked && !std::fs::metadata(&grant.folder).map_err(io)?.is_dir() {
            return Err(deny().into());
        }
    }
    Ok(())
}

/// Approves `folder`, which the user picked with Browse… in a form of plugin `id`, for that
/// plugin's grant `grant` (a `picked` grant of its manifest), and saves the approval for later
/// sessions. Typing a path approves nothing: only the app's folder picker calls this. Refuses a
/// folder that would hand over more than a folder of work: the whole disk, the home folder or
/// one above it, a hidden folder in it (`~/.ssh`), and the app's own data.
pub fn approve_picked_folder(s: &mut Session, id: &str, grant: &str, folder: &Path) -> Result<PathBuf> {
    let path = s.plugin_state.packages.get(id).cloned().ok_or_else(|| invalid("that plugin is not installed"))?;
    let package = read_package(&path)?;
    let asked = package
        .manifest
        .runtime
        .as_ref()
        .and_then(|rt| rt.permissions.files.iter().find(|r| r.name == grant && r.picked))
        .cloned()
        .ok_or_else(|| invalid(format!("the plugin has no folder grant “{grant}” for folders you pick")))?;
    let folder = std::fs::canonicalize(folder).map_err(io)?;
    if !std::fs::metadata(&folder).map_err(io)?.is_dir() {
        return Err(invalid("pick a folder, not a file"));
    }
    // Both sides resolved: the picked folder is, and the data folder may be reached through a link.
    let plugins = s.plugin_state.plugins_dir().map(|plugins| resolved(&plugins));
    let home = solidcraft_io::vfs::home_folder().map(|home| resolved(&home));
    if let Some(why) = picked_folder_refusal(&folder, home.as_deref(), plugins.as_deref()) {
        return Err(HostError::PermissionDenied { permission: why }.into());
    }
    let grants = s.plugin_state.grants.get_mut(id).filter(|g| g.package_hash.as_deref() == Some(package.approval.as_str()));
    let Some(grants) = grants else {
        return Err(HostError::PermissionDenied { permission: "approve the plugin in Preferences › Plugins first".into() }.into());
    };
    if grants.folders.iter().any(|g| g.name == grant && g.folder == folder) {
        return Ok(folder);
    }
    if grants.folders.iter().filter(|g| g.name == grant).count() >= MAX_PICKED_FOLDERS {
        return Err(invalid(format!("at most {MAX_PICKED_FOLDERS} folders for one grant: remove one in Preferences › Plugins")));
    }
    let mut changed = grants.clone();
    changed.folders.push(FileGrant { name: grant.to_owned(), folder: folder.clone(), read: asked.read, write: asked.write });
    validate_grants(&package.manifest, &changed)?;
    *grants = changed;
    s.plugin_state.persist_stored()?;
    Ok(folder)
}

/// Why a picked `folder` (resolved) can't be approved for a plugin, or `None` when it can.
pub fn picked_folder_refusal(folder: &Path, home: Option<&Path>, plugins: Option<&Path>) -> Option<String> {
    if folder.parent().is_none() {
        return Some("pick a folder of your work, not the whole disk".into());
    }
    if let Some(home) = home {
        if home.starts_with(folder) {
            return Some("pick a folder inside your home folder, not the home folder itself".into());
        }
        // Hidden as the app's file access counts it: a dot name, and on Windows `AppData`.
        let hidden = |name: &str| name.starts_with('.') || (cfg!(windows) && name.eq_ignore_ascii_case("AppData"));
        if let Ok(inside) = folder.strip_prefix(home)
            && inside.components().any(|part| hidden(&part.as_os_str().to_string_lossy()))
        {
            return Some("pick a visible folder: a hidden one in your home folder holds settings and keys".into());
        }
    }
    // The app's data: its plugins, their approvals and the folders it keeps for them.
    if let Some(data) = plugins.and_then(Path::parent)
        && (folder.starts_with(data) || data.starts_with(folder))
    {
        return Some("pick a folder outside SolidCraft's own data folder".into());
    }
    None
}
/// `path` with its links resolved as far as it exists; a part not made yet stays as written.
fn resolved(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return missing.iter().rev().fold(real, |done, name| done.join(name));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_owned());
                at = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}
fn root(s: &Session) -> Result<PathBuf> {
    s.plugin_state.plugins_dir().ok_or_else(|| invalid("plugin directory is unavailable"))
}
/// Prepare grants bound to a checked package. Publish this state only after installation succeeds.
/// This does not install files or grant permission to execute plugin code.
pub fn store_grants(s: &mut Session, package: &install::Unpacked, mut grants: Grants) -> Result<()> {
    let manifest = &package.manifest;
    if !valid_id(&manifest.id) {
        return Err(invalid("invalid plugin id"));
    }
    validate_grants(manifest, &grants)?;
    if !s.plugin_state.grants.contains_key(&manifest.id) && s.plugin_state.grants.len() >= 256 {
        return Err(invalid("at most 256 stored plugin grants"));
    }
    grants.folders.sort_by(|a, b| a.name.cmp(&b.name));
    grants.package_hash = Some(package.approval.clone());
    s.plugin_state.grants.insert(manifest.id.clone(), grants);
    Ok(())
}

/// Deleting a whole folder must not bypass a caller's hidden-path restrictions.
pub(super) fn check_remove_tree(root: &Path) -> Result<()> {
    if solidcraft_io::vfs::current_scope().is_none() {
        return Ok(());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0;
    while let Some(dir) = pending.pop() {
        check(&dir)?;
        for entry in std::fs::read_dir(dir).map_err(io)? {
            entries += 1;
            if entries > MAX_FILES {
                return Err(invalid("installed package has too many entries"));
            }
            let path = entry.map_err(io)?.path();
            check(&path)?;
            if std::fs::symlink_metadata(&path).map_err(io)?.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(())
}

pub fn remove(s: &mut Session, id: &str) -> Result<RemoveResult> {
    if !valid_id(id) {
        return Err(invalid("invalid plugin id"));
    }
    let path = root(s)?.join(id);
    check(&path)?;
    let meta = std::fs::symlink_metadata(&path).map_err(io)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(invalid("installed package must be a directory, not a symlink"));
    }
    check_remove_tree(&path)?;
    // The delete and the approvals' update are one write: none overlaps an install's.
    let _write = install::write_lock();
    s.plugin_state.refresh_stored();
    // The approvals go first: a store that cannot be written leaves the package installed, with
    // nothing granted in this session, and the error is reported. A record left behind could
    // otherwise approve the same package again if it were copied back.
    s.plugin_state.grants.remove(id);
    s.plugin_state.persist_stored()?;
    s.plugin_state.forget_stored(id)?;
    std::fs::remove_dir_all(path).map_err(io)?;
    s.plugin_state.packages.remove(id);
    s.disabled_plugins.remove(id);
    Ok(RemoveResult { id: id.into(), removed: true, param_warnings: vec![] })
}
