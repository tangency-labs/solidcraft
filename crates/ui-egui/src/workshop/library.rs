//! Installing a parts library from the Workshop: its release `.zip` (solidcraft-parts
//! docs/library-format.md, `folder` version 1) unpacks into SolidCraft's data folder, one folder
//! per library and version, and the Parts Library panel shows it.
//!
//! The zip is untrusted. It is refused whole, before anything is written, when an entry is a
//! symbolic link, has an absolute path or `..`, is a program or script, or when there are too many
//! entries or too many bytes. It unpacks into a folder of its own that is renamed into place only
//! once every file is written, so a failed install leaves nothing half there.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::index::Refusal;

/// The most entries a library may hold.
const MAX_ENTRIES: usize = 20_000;
/// The most bytes it may unpack to.
const MAX_BYTES: u64 = 4 << 30;
/// Kinds of file a library may not hold: code belongs in a plugin.
const REFUSED: &[&str] = &[
    "app", "bat", "bin", "cmd", "com", "cpl", "dll", "dylib", "elf", "exe", "jar", "js", "lnk", "mjs", "msi", "pkg", "ps1", "py", "pyc", "pyw", "rb",
    "scr", "sh", "so", "vbs", "wasm", "ws", "wsf",
];

/// Where libraries are installed: `<data>/parts-libraries`.
pub fn root() -> Option<PathBuf> {
    solidcraft_engine::recovery::data_dir().map(|d| d.join("parts-libraries"))
}

/// Unpack `zip` as library `id` at `version` under `root`, and give the folder the panel shows:
/// the zip's single top folder when it has one. An installed copy of the same version is kept.
pub fn install(zip: &Path, root: &Path, id: &str, version: &str) -> Result<PathBuf, Refusal> {
    let file = std::fs::File::open(zip).map_err(|e| Refusal::Download(e.to_string()))?;
    install_file(file, root, id, version)
}

/// [`install`] from the open file whose sha256 was checked, so nothing can stand in for it
/// between the check and the unpacking.
pub fn install_file(file: std::fs::File, root: &Path, id: &str, version: &str) -> Result<PathBuf, Refusal> {
    if !super::link::segment(id) || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(Refusal::BadItem(format!("library id {id} {version}")));
    }
    let dest = root.join(id).join(version);
    if dest.is_dir() {
        return Ok(shown_folder(&dest));
    }
    let bad = |e: String| Refusal::LibraryContents(e);
    let mut archive = zip::ZipArchive::new(file).map_err(|e| bad(e.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(bad(format!("{} entries", archive.len())));
    }
    // Check every entry before writing any.
    let mut total = 0u64;
    let mut plan = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| bad(e.to_string()))?;
        let name = entry.name().map_err(|e| bad(e.to_string()))?.into_owned();
        if entry.is_symlink() {
            return Err(bad(format!("{name} is a link")));
        }
        let relative = entry.enclosed_name().filter(|p| p.is_relative()).ok_or_else(|| bad(format!("{name} points outside the library")))?;
        if name.contains('\\') || name.starts_with('/') || name.split('/').any(|part| part == "..") {
            return Err(bad(format!("{name} points outside the library")));
        }
        if !entry.is_dir() {
            let ext = relative.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if REFUSED.contains(&ext.as_str()) {
                return Err(bad(format!("{name} is a program or script")));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_BYTES {
                return Err(bad("it unpacks to more than 4 GiB".into()));
            }
        }
        plan.push((i, relative, entry.is_dir()));
    }
    let parent = root.join(id);
    std::fs::create_dir_all(&parent).map_err(|e| Refusal::Download(e.to_string()))?;
    let staging = parent.join(format!(".{version}.partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let written = (|| -> Result<(), Refusal> {
        for (i, relative, dir) in plan {
            let out = staging.join(&relative);
            if dir {
                std::fs::create_dir_all(&out).map_err(|e| Refusal::Download(e.to_string()))?;
                continue;
            }
            if let Some(folder) = out.parent() {
                std::fs::create_dir_all(folder).map_err(|e| Refusal::Download(e.to_string()))?;
            }
            let mut entry = archive.by_index(i).map_err(|e| bad(e.to_string()))?;
            let mut target = std::fs::File::create(&out).map_err(|e| Refusal::Download(e.to_string()))?;
            // The size the entry claims bounds what is read, whatever the compressed data holds.
            let limit = entry.size();
            std::io::copy(&mut (&mut entry).take(limit), &mut target).map_err(|e| bad(e.to_string()))?;
        }
        std::fs::rename(&staging, &dest).map_err(|e| Refusal::Download(e.to_string()))
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(shown_folder(&dest))
}

/// The library's single top folder, or `dest` when the zip holds more than one thing at its top.
fn shown_folder(dest: &Path) -> PathBuf {
    let Ok(entries) = std::fs::read_dir(dest) else { return dest.to_path_buf() };
    let top: Vec<PathBuf> = entries.flatten().map(|e| e.path()).take(2).collect();
    match top.as_slice() {
        [only] if only.is_dir() => only.clone(),
        _ => dest.to_path_buf(),
    }
}
