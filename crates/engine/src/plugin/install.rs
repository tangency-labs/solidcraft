//! Installing a plugin from a `.zip`: checked in memory, then unpacked into a hidden folder beside
//! the installed plugins and renamed into `plugins/<id>/`. A plugin already installed under that
//! id is replaced only when the caller says so.

use super::{MAX_COMPONENT_BYTES, Manifest, parse_manifest};
use craft_plugin::HostError;
use solidcraft_io::safe_zip;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// The largest `.zip` read, and the most it may unpack to.
pub const MAX_ZIP_BYTES: usize = 64 << 20;
pub const LIMITS: safe_zip::Limits =
    safe_zip::Limits { max_entries: 256, max_entry_bytes: MAX_COMPONENT_BYTES as u64, max_total_bytes: MAX_ZIP_BYTES as u64 };

/// Why a `.zip` was not installed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Archive(#[from] safe_zip::Error),
    #[error("the .zip has no plugin.json")]
    MissingManifest,
    #[error("the .zip must hold one plugin, at its top or in one folder")]
    NotOnePlugin,
    #[error("the .zip has no {0}, which plugin.json names")]
    MissingModule(String),
    #[error("{0}")]
    Manifest(HostError),
    #[error("“{name}” {version} is already installed; confirm to replace it")]
    AlreadyInstalled { id: String, name: String, version: String },
    #[error("installing plugins needs the desktop app")]
    NoPluginsFolder,
    #[error("could not install the plugin: {0}")]
    Io(String),
}

/// A checked plugin, still in memory.
#[derive(Debug)]
pub struct Unpacked {
    pub manifest: Manifest,
    /// Hash of the exact manifest and component that will be published.
    pub approval: String,
    /// SHA-256 of the whole `.zip` as read, so an install can insist on exactly the archive that was
    /// reviewed: `approval` covers only the manifest and component, not the plugin's other files.
    pub archive: String,
    /// Paths relative to the plugin's folder.
    pub files: Vec<safe_zip::File>,
}

/// Files a desktop zip tool adds that are not part of the plugin.
fn is_junk(path: &str) -> bool {
    path.starts_with("__MACOSX/") || path.rsplit('/').next().is_some_and(|name| name == ".DS_Store" || name == "Thumbs.db")
}

/// Checks `zip`: one plugin (`plugin.json` and its module) at the top or in one top-level folder,
/// a manifest the host accepts, safe paths and sizes.
pub fn unpack(zip: &[u8]) -> Result<Unpacked, InstallError> {
    let files: Vec<_> = safe_zip::read(zip, LIMITS)?.into_iter().filter(|f| !is_junk(&f.path)).collect();
    let manifests: Vec<&str> =
        files.iter().filter(|f| f.path == "plugin.json" || f.path.ends_with("/plugin.json")).map(|f| f.path.as_str()).collect();
    let prefix = match manifests.as_slice() {
        [] => return Err(InstallError::MissingManifest),
        ["plugin.json"] => "",
        [one] => match one.strip_suffix("plugin.json") {
            Some(folder) if folder.matches('/').count() == 1 => folder,
            _ => return Err(InstallError::NotOnePlugin),
        },
        _ => return Err(InstallError::NotOnePlugin),
    }
    .to_string();
    // Everything belongs to that one plugin.
    if files.iter().any(|f| !f.path.starts_with(&prefix)) {
        return Err(InstallError::NotOnePlugin);
    }
    let files: Vec<safe_zip::File> = files.into_iter().map(|f| safe_zip::File { path: f.path[prefix.len()..].to_string(), bytes: f.bytes }).collect();
    let text = files.iter().find(|f| f.path == "plugin.json").map(|f| f.bytes.as_slice()).ok_or(InstallError::MissingManifest)?;
    // The folder it installs to is its id: the parser checks the id itself.
    let id = serde_json::from_slice::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("id").and_then(serde_json::Value::as_str).map(str::to_string))
        .unwrap_or_default();
    let manifest = parse_manifest(text, &id).map_err(|e| match e {
        crate::EngineError::Plugin(e) => InstallError::Manifest(e),
        e => InstallError::Manifest(HostError::InvalidManifest { field: "manifest".into(), reason: e.to_string() }),
    })?;
    let component = if let Some(runtime) = &manifest.runtime {
        files
            .iter()
            .find(|f| f.path == runtime.module)
            .map(|f| f.bytes.as_slice())
            .ok_or_else(|| InstallError::MissingModule(runtime.module.clone()))?
    } else {
        &[]
    };
    let approval = super::package_hash(text, component);
    let archive = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(zip));
    Ok(Unpacked { manifest, approval, archive, files })
}

/// The installed plugin of this id: its name and version, if its folder holds a readable manifest.
#[cfg(not(target_arch = "wasm32"))]
pub fn installed(plugins: &Path, id: &str) -> Option<(String, String)> {
    let folder = plugins.join(id);
    if !folder.exists() {
        return None;
    }
    let text = std::fs::read(folder.join("plugin.json")).ok().filter(|t| t.len() <= super::MAX_MANIFEST_BYTES);
    Some(match text.and_then(|t| parse_manifest(&t, id).ok()) {
        Some(m) => (m.name, m.version),
        None => (id.to_string(), String::new()),
    })
}

/// Writes `plugin` to `plugins/<id>/`. An installed plugin of that id is replaced only when
/// `replace`; the old folder is kept until the new one is in place.
#[cfg(not(target_arch = "wasm32"))]
pub fn install(plugin: &Unpacked, plugins: &Path, replace: bool) -> Result<PathBuf, InstallError> {
    let io = |e: std::io::Error| InstallError::Io(e.to_string());
    let id = &plugin.manifest.id;
    let target = plugins.join(id);
    if !replace && let Some((name, version)) = installed(plugins, id) {
        return Err(InstallError::AlreadyInstalled { id: id.clone(), name, version });
    }
    // Preflight logical destinations before the staging folder writes or a replacement deletes.
    // Checking only the plugin folder would bypass a scoped caller's hidden-file restrictions.
    for path in std::iter::once(target.clone()).chain(plugin.files.iter().map(|file| target.join(&file.path))) {
        solidcraft_io::vfs::check_new(&path.to_string_lossy()).map_err(|e| InstallError::Io(e.to_string()))?;
    }
    if target.exists() {
        super::package::check_remove_tree(&target).map_err(|e| InstallError::Io(e.to_string()))?;
    }
    std::fs::create_dir_all(plugins).map_err(io)?;
    // A hidden name: the plugins list skips it while it is written.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    let staging = plugins.join(format!(".install-{id}-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&staging).map_err(io)?;
    let written = (|| -> std::io::Result<()> {
        for file in &plugin.files {
            let path = staging.join(&file.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::OpenOptions::new().write(true).create_new(true).open(&path).and_then(|mut f| std::io::Write::write_all(&mut f, &file.bytes))?;
        }
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io(e));
    }
    let old = target.exists().then(|| plugins.join(format!(".replaced-{id}-{}-{stamp}", std::process::id())));
    if let Some(old) = &old
        && let Err(e) = std::fs::rename(&target, old)
    {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io(e));
    }
    if let Err(e) = std::fs::rename(&staging, &target) {
        if let Some(old) = &old {
            let _ = std::fs::rename(old, &target);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(io(e));
    }
    if let Some(old) = old {
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(target)
}
