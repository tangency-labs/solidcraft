//! Validation and storage of approved grants, and native plugin removal.
use super::*;
use crate::schema::plugin::RemoveResult;

const MAX_FILES: usize = 1024;

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
    })
}
pub fn validate_grants(manifest: &Manifest, grants: &Grants) -> Result<()> {
    let asked = requested(manifest);
    let deny = || HostError::PermissionDenied { permission: "approval contains an unrequested, duplicate, invalid or excessive grant".into() };
    if grants.command_groups.len() > 4 || grants.folders.len() > 32 || !grants.command_groups.is_subset(&asked.command_groups) {
        return Err(deny().into());
    }
    let mut names = BTreeSet::new();
    for grant in &grants.folders {
        if !names.insert(&grant.name)
            || !grant.folder.is_absolute()
            || grant.folder.to_string_lossy().len() > 4096
            || grant.folder.to_string_lossy().chars().any(char::is_control)
            || (!grant.read && !grant.write)
            || !asked.folders.iter().any(|r| r.name == grant.name && (!grant.read || r.read) && (!grant.write || r.write))
        {
            return Err(deny().into());
        }
        check(&grant.folder)?;
        if !std::fs::metadata(&grant.folder).map_err(io)?.is_dir() {
            return Err(deny().into());
        }
    }
    Ok(())
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
    std::fs::remove_dir_all(path).map_err(io)?;
    s.plugin_state.grants.remove(id);
    s.plugin_state.packages.remove(id);
    s.disabled_plugins.remove(id);
    Ok(RemoveResult { id: id.into(), removed: true, param_warnings: vec![] })
}
