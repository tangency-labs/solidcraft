//! The files of a design read from disk, each once, and the paths that tie them together.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use crate::check::Checked;
use crate::model::{Document, Entry, lower};
use crate::parse::{Parse, parse};

pub const EXTENSION: &str = "esc";
/// Larger than any design written by hand or by the GUI; a bigger file is refused unread.
const MAX_FILE: u64 = 16 << 20;

#[derive(Debug)]
pub struct Loaded {
    pub path: PathBuf,
    pub text: String,
    pub parse: Parse,
    /// Only for a file without syntax errors.
    pub doc: Option<Document>,
}

/// Every file read once and kept, keyed by its normalized path.
#[derive(Debug, Default)]
pub struct Project {
    files: HashMap<PathBuf, Result<Rc<Loaded>, String>>,
    pub(crate) checks: HashMap<PathBuf, Rc<Checked>>,
    pub(crate) in_progress: HashSet<PathBuf>,
}

impl Project {
    pub fn load(&mut self, path: &Path) -> Result<Rc<Loaded>, String> {
        let path = normalize(path);
        self.files.entry(path.clone()).or_insert_with(|| read(&path).map(|text| Rc::new(loaded(path, text)))).clone()
    }

    /// Puts `text` in place of what the file holds on disk, as an edit about to be written.
    pub fn replace(&mut self, path: &Path, text: String) {
        let path = normalize(path);
        self.files.insert(path.clone(), Ok(Rc::new(loaded(path, text))));
        self.checks.clear();
    }
}

fn loaded(path: PathBuf, text: String) -> Loaded {
    let parse = parse(&text);
    let doc = parse.errors.is_empty().then(|| lower(&parse.syntax()));
    Loaded { path, text, parse, doc }
}

fn read(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("{} is larger than {} MiB", path.display(), MAX_FILE >> 20));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} isn't UTF-8 text", path.display()))
}

/// Resolves `..` and `.` in the path's text, without asking the filesystem.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Where a path written in `from` points. Absolute paths are refused, so a design folder moves
/// as a whole.
pub fn resolve(from: &Path, written: &str) -> Option<PathBuf> {
    let rel = Path::new(written);
    if rel.has_root() || rel.is_absolute() || rel.components().any(|c| matches!(c, Component::Prefix(_))) {
        return None;
    }
    Some(normalize(&from.parent().unwrap_or(Path::new("")).join(rel)))
}

/// The `.esc` file an `import("...")` names.
pub fn import_path(from: &Path, written: &str) -> Option<PathBuf> {
    resolve(from, &format!("{written}.{EXTENSION}"))
}

/// The `.esc` file an entry of the file at `from` imports, when it is an `import`.
pub fn imported_file(from: &Path, e: &Entry) -> Option<PathBuf> {
    if e.call_name() != Some("import") {
        return None;
    }
    import_path(from, &e.written_path()?.0)
}

/// Every `.esc` file under `dir`, sorted, skipping hidden folders and build output.
pub fn list(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if !name.starts_with('.') && name != "target" {
                    pending.push(path);
                }
            } else if kind.is_file() && path.extension().is_some_and(|e| e == EXTENSION) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}
