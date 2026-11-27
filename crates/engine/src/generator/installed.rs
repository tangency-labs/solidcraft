//! Installed generator packages: folders in the user's `generators` folder, each holding a
//! `generator.toml`. This is the only place generator code looks for packages, and it only reads:
//! one folder (`generators` in the per-user data folder, or the one `SOLIDCRAFT_GENERATORS_DIR`
//! names) and the one folder a command is given.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::package::Package;

/// The most packages listed from the folder.
const MAX_LISTED: usize = 256;

/// The folder installed packages live in.
pub fn dir() -> Option<PathBuf> {
    match std::env::var_os("SOLIDCRAFT_GENERATORS_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => crate::recovery::data_dir().map(|d| d.join("generators")),
    }
}

/// One installed package.
#[derive(Clone)]
pub struct Installed {
    pub path: PathBuf,
    pub package: Arc<Package>,
}

impl std::fmt::Debug for Installed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Installed").field("path", &self.path).field("id", &self.package.id).finish()
    }
}

/// A package folder, from what a command was given: the folder, its `generator.toml`, or the id
/// of an installed package.
pub fn locate(given: &str) -> Result<PathBuf, String> {
    let named_path = given.contains('/') || given.contains('\\') || given.starts_with('.');
    if named_path {
        let p = Path::new(given);
        let folder = if p.file_name().is_some_and(|n| n == "generator.toml") { p.parent().unwrap_or(p).to_path_buf() } else { p.to_path_buf() };
        return Ok(folder);
    }
    if given.is_empty() || given.len() > 64 || !given.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!("`{given}` is neither a package folder nor an installed package id"));
    }
    let base = dir().ok_or_else(|| "there is no folder for installed generators on this system".to_string())?;
    Ok(base.join(given))
}

/// Load the package `given` names.
pub fn load(given: &str) -> Result<Package, String> {
    let folder = locate(given)?;
    Package::load(&folder).map_err(|e| e.to_string())
}

/// The packages in the folder, by name, and a line for each folder that is not a valid one.
pub fn list() -> (Vec<Installed>, Vec<String>) {
    match dir() {
        Some(base) => list_in(&base),
        None => (Vec::new(), Vec::new()),
    }
}

/// The packages in `base`, as [`list`] gives them.
pub fn list_in(base: &Path) -> (Vec<Installed>, Vec<String>) {
    let mut folders: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base) {
        folders = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("generator.toml").is_file()).take(MAX_LISTED).collect();
        folders.sort();
    }
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for path in folders {
        match Package::load(&path) {
            Ok(p) => installed.push(Installed { path, package: Arc::new(p) }),
            Err(e) => skipped.push(format!("{}: {e}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        }
    }
    (installed, skipped)
}
