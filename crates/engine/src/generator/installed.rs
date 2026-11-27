//! Installed generator packages: the `.generator.json` files in the user's `generators` folder.
//! This is the only place generator code touches the file system, and it only reads: one folder
//! (`generators` in the per-user data folder, or the one `SOLIDCRAFT_GENERATORS_DIR` names) and
//! the one file a command is given, each at most one package's size.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{Generator, MAX_PACKAGE_BYTES};

/// The most packages listed from the folder.
const MAX_LISTED: usize = 256;

/// The folder installed packages live in.
pub fn dir() -> Option<PathBuf> {
    match std::env::var_os("SOLIDCRAFT_GENERATORS_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => crate::recovery::data_dir().map(|d| d.join("generators")),
    }
}

/// A package file's text, never more than the package limit (a longer file comes back one byte
/// over it, which [`Generator::load`] refuses).
pub fn read_text(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open the generator package `{}`: {e}", path.display()))?;
    let mut text = String::new();
    file.take(MAX_PACKAGE_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read the generator package `{}`: {e}", path.display()))?;
    Ok(text)
}

/// One installed package.
#[derive(Clone, Debug)]
pub struct Installed {
    pub path: PathBuf,
    pub generator: Arc<Generator>,
}

/// Load the package at `path`.
pub fn load_file(path: &Path) -> Result<Generator, String> {
    let text = read_text(path)?;
    Generator::load(&text, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())
}

/// The packages in the folder, by file name, and a line for each file that is not a valid one.
pub fn list() -> (Vec<Installed>, Vec<String>) {
    let mut files: Vec<PathBuf> = Vec::new();
    if let Some(dir) = dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        files = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|f| f.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".generator.json")))
            .take(MAX_LISTED)
            .collect();
        files.sort();
    }
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for path in files {
        match load_file(&path) {
            Ok(g) => installed.push(Installed { path, generator: Arc::new(g) }),
            Err(e) => skipped.push(format!("{}: {e}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
        }
    }
    (installed, skipped)
}
