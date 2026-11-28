//! Installing a plugin from a `.zip`: checked in memory, then unpacked into a hidden folder beside
//! the installed plugins and renamed into `plugins/<id>/`. A plugin already installed under that
//! id is replaced only when the caller says so.

use super::{Manifest, parse_manifest};
use craft_plugin::HostError;
use solidcraft_io::safe_zip;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// The largest `.zip` read, and the most it may unpack to: room for an importer's component (up
/// to 64 MiB) beside its manifest. A command plugin's component stays within 16 MiB (checked
/// once the manifest says which it is).
pub const MAX_ZIP_BYTES: usize = 80 << 20;
pub const LIMITS: safe_zip::Limits =
    safe_zip::Limits { max_entries: 256, max_entry_bytes: craft_plugin::MAX_IMPORTER_COMPONENT_BYTES as u64, max_total_bytes: MAX_ZIP_BYTES as u64 };

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
    #[error("the .zip has no {0}, which plugin.json names in themes")]
    MissingTheme(String),
    /// The module is missing and the .zip holds Rust sources: most likely the "Source code"
    /// archive that GitHub lists beside a release, not the plugin's own .zip (#16736).
    #[error(
        "the .zip has no {0}, which plugin.json names: it holds the plugin's source code. Install the plugin's own .zip from its release instead"
    )]
    SourceOnly(String),
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

impl Unpacked {
    /// Optional artwork never prevents installation. Uses exactly the bytes being reviewed.
    pub fn swatch_warnings(&self) -> Vec<String> {
        let Some(runtime) = &self.manifest.runtime else { return Vec::new() };
        craft_plugin::swatches::check(&runtime.entries, |path| {
            let file = self.files.iter().find(|file| file.path == path).ok_or_else(|| "not found in the package".to_string())?;
            if file.bytes.len() > craft_plugin::swatches::IMAGE_MAX_BYTES {
                return Err(format!("file is larger than {} KiB", craft_plugin::swatches::IMAGE_MAX_BYTES / 1024));
            }
            Ok(file.bytes.clone())
        })
        .warnings
    }
}

static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Held while a package is written to or deleted from the plugins folder (an install's write and its
/// stored approvals, a removal's delete), so two writes never overlap: a job the app gave up on can
/// still be writing when the next one starts. Reading and checking a package takes no part in it, so a
/// worker stuck in a read holds nothing. A holder that panicked leaves nothing locked.
pub fn write_lock() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Files a desktop zip tool adds that are not part of the plugin.
fn is_junk(path: &str) -> bool {
    path.starts_with("__MACOSX/") || path.rsplit('/').next().is_some_and(|name| name == ".DS_Store" || name == "Thumbs.db")
}

/// Checks `zip`: one plugin (`plugin.json` and its module) at the top or in one top-level folder,
/// a manifest the host accepts, safe paths and sizes.
pub fn unpack(zip: &[u8]) -> Result<Unpacked, InstallError> {
    let files = safe_zip::read(zip, LIMITS)?;
    from_files(files, format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(zip)))
}

/// Checks a plugin's files as [`unpack`] reads them from a `.zip`, with `archive` the hash of what
/// they came from. `plugin check` reads a package folder into the same checks.
pub fn from_files(files: Vec<safe_zip::File>, archive: String) -> Result<Unpacked, InstallError> {
    let files: Vec<_> = files.into_iter().filter(|f| !is_junk(&f.path)).collect();
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
    // An install may ask for approval above the user's default, never above this hard ceiling.
    if let Some(runtime) = &manifest.runtime
        && runtime.limits.fuel > craft_plugin::MAX_APPROVED_FUEL
    {
        return Err(InstallError::Manifest(HostError::OverCeiling {
            limit: "fuel".into(),
            requested: runtime.limits.fuel,
            allowed: craft_plugin::MAX_APPROVED_FUEL,
        }));
    }
    if let Some(runtime) = &manifest.runtime
        && !files.iter().any(|f| f.path == runtime.module)
    {
        let source = files.iter().any(|f| f.path == "Cargo.toml" || f.path.ends_with(".rs"));
        let module = runtime.module.clone();
        return Err(if source { InstallError::SourceOnly(module) } else { InstallError::MissingModule(module) });
    }
    // Removal walks every file and folder up to a fixed count, so an install must not create more.
    let mut folders = std::collections::BTreeSet::new();
    for file in &files {
        let mut path = file.path.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            folders.insert(parent);
            path = parent;
        }
    }
    if files.len() + folders.len() > super::MAX_FILES {
        return Err(InstallError::Manifest(HostError::LimitExceeded { limit: "a plugin folder holds at most 1024 files and folders".into() }));
    }
    check_themes(&manifest, &files)?;
    // Every file but the plugin's component stays within a command component's budget.
    let component_path = manifest.runtime.as_ref().map(|r| r.module.as_str());
    if files.iter().any(|f| Some(f.path.as_str()) != component_path && f.bytes.len() > craft_plugin::MAX_COMPONENT_BYTES) {
        return Err(InstallError::Manifest(HostError::LimitExceeded { limit: "a plugin file other than its component is over 16 MiB".into() }));
    }
    check_shaders(&manifest, &files)?;
    let component = if let Some(runtime) = &manifest.runtime {
        let module = files.iter().find(|f| f.path == runtime.module).ok_or_else(|| InstallError::MissingModule(runtime.module.clone()))?;
        if module.bytes.len() > runtime.max_component_bytes() {
            let mib = runtime.max_component_bytes() >> 20;
            return Err(InstallError::Manifest(HostError::LimitExceeded { limit: format!("component size: {} is over {mib} MiB", runtime.module) }));
        }
        module.bytes.as_slice()
    } else {
        &[]
    };
    let approval = super::package_hash(text, component);
    Ok(Unpacked { manifest, approval, archive, files })
}

/// Refuses only a package that could never draw; the app compiles the source again when it draws.
fn check_shaders(manifest: &Manifest, files: &[safe_zip::File]) -> Result<(), InstallError> {
    let file = |name: &str| files.iter().find(|f| f.path == name);
    for shader in &manifest.shaders {
        let source = file(&shader.source).ok_or_else(|| InstallError::MissingModule(shader.source.clone()))?;
        let text = std::str::from_utf8(&source.bytes).map_err(|_| {
            InstallError::Manifest(HostError::InvalidManifest {
                field: format!("shaders.{}", shader.id),
                reason: format!("{} is not UTF-8 text", shader.source),
            })
        })?;
        craft_plugin::shader::compile(shader, text).map_err(InstallError::Manifest)?;
        for texture in &shader.textures {
            // A texture the person chooses may ship no file at all.
            let Some(name) = &texture.file else { continue };
            let picture = file(name).ok_or_else(|| InstallError::MissingModule(name.clone()))?;
            if picture.bytes.len() > craft_plugin::shader::MAX_TEXTURE_BYTES {
                return Err(InstallError::Manifest(HostError::LimitExceeded { limit: format!("{name} is over 16 MiB") }));
            }
        }
    }
    Ok(())
}

/// Each theme's file is in the package and is a theme file (`themes@0.1`); the host reads it again
/// when it lists the theme.
fn check_themes(manifest: &Manifest, files: &[safe_zip::File]) -> Result<(), InstallError> {
    for theme in &manifest.themes {
        let file = files.iter().find(|f| f.path == theme.source).ok_or_else(|| InstallError::MissingTheme(theme.source.clone()))?;
        craft_plugin::theme::check_file(&theme.id, &file.bytes).map_err(InstallError::Manifest)?;
    }
    Ok(())
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

/// [`install_locked`] under the write lock.
#[cfg(not(target_arch = "wasm32"))]
pub fn install(plugin: &Unpacked, plugins: &Path, replace: bool) -> Result<PathBuf, InstallError> {
    let _write = write_lock();
    install_locked(plugin, plugins, replace)
}

/// Whether `e` is Windows refusing to rename or remove a folder because something inside it is open
/// for a moment: `ERROR_ACCESS_DENIED` (5) or `ERROR_SHARING_VIOLATION` (32). Windows refuses to rename
/// a folder while any file or folder in it has an open handle, even one that allows deleting. The
/// app's plugin-folder watcher, the plugins scan, an antivirus or the search indexer each open files
/// there briefly (#23591).
#[cfg(not(target_arch = "wasm32"))]
fn held_for_a_moment(e: &std::io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32))
}

/// Runs `op` until it succeeds, it fails for another reason than `held`, or `tries` attempts have
/// failed, waiting `pause` between attempts. Returns the last result.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn retry_while_held(
    mut op: impl FnMut() -> std::io::Result<()>,
    held: impl Fn(&std::io::Error) -> bool,
    tries: u32,
    pause: std::time::Duration,
) -> std::io::Result<()> {
    let mut attempt = 1;
    loop {
        match op() {
            Err(e) if held(&e) && attempt < tries => {
                attempt += 1;
                std::thread::sleep(pause);
            }
            done => return done,
        }
    }
}

/// How long a rename or a removal in the plugins folder waits for a file that is open for a moment:
/// 40 attempts 50 ms apart, about 2 s.
#[cfg(not(target_arch = "wasm32"))]
const HELD_TRIES: u32 = 40;
#[cfg(not(target_arch = "wasm32"))]
const HELD_PAUSE: std::time::Duration = std::time::Duration::from_millis(50);

#[cfg(not(target_arch = "wasm32"))]
fn rename_when_free(from: &Path, to: &Path) -> std::io::Result<()> {
    retry_while_held(|| std::fs::rename(from, to), held_for_a_moment, HELD_TRIES, HELD_PAUSE)
}

/// Writes `plugin` to `plugins/<id>/`. An installed plugin of that id is replaced only when
/// `replace`; the old folder is kept until the new one is in place. The caller holds [`write_lock`].
#[cfg(not(target_arch = "wasm32"))]
pub fn install_locked(plugin: &Unpacked, plugins: &Path, replace: bool) -> Result<PathBuf, InstallError> {
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
    // Each step names the folder it could not move, so a refusal says where it happened.
    let step = |what: &str, folder: &Path, e: std::io::Error| InstallError::Io(format!("{what} ({}): {e}", folder.display()));
    let old = target.exists().then(|| plugins.join(format!(".replaced-{id}-{}-{stamp}", std::process::id())));
    if let Some(old) = &old
        && let Err(e) = rename_when_free(&target, old)
    {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(step("could not move the installed plugin aside", &target, e));
    }
    if let Err(e) = rename_when_free(&staging, &target) {
        if let Some(old) = &old {
            let _ = rename_when_free(old, &target);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(step("could not move the new plugin into place", &target, e));
    }
    if let Some(old) = old {
        let _ = retry_while_held(|| std::fs::remove_dir_all(&old), held_for_a_moment, HELD_TRIES, HELD_PAUSE);
    }
    Ok(target)
}
