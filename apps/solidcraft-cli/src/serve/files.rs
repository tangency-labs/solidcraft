//! The data folder over HTTP (`/api/v1/files`): list, read, save and delete design files, every
//! path confined to the folder. A path is judged as text first (no `..`, `.`, empty, hidden or
//! backslash segments, no `:`; a design file's extension), then resolved through links and checked
//! against the folder with the same `PathScope` MCP's `--allow-path` uses, so a link inside the
//! folder that leads out is refused too.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};
use solidcraft_engine::io::vfs::PathScope;

use super::http::{Body, Head, HttpError, Response};

/// The extensions the file API serves and saves (lower case): designs and the model formats
/// SolidCraft opens. Anything else is refused, so the data folder is not a general file drop.
pub const EXTENSIONS: &[&str] = &["3dm", "3mf", "dxf", "fcstd", "glb", "gltf", "obj", "sc", "solidcraft", "step", "stl", "stp", "svg"];
/// The longest relative path, in bytes.
const MAX_PATH: usize = 1024;
/// The longest name of one file or folder, in bytes.
const MAX_NAME: usize = 255;
/// How deep a path may go, and how deep a listing looks.
const MAX_DEPTH: usize = 16;
/// The most files a listing returns.
pub const MAX_LISTED: usize = 10_000;

/// Saves and deletes compare a file's version and replace it under this lock, so two saves with
/// the same `If-Match` can't both win.
static REPLACE: Mutex<()> = Mutex::new(());

/// The data folder.
pub struct Store {
    root: PathBuf,
    scope: PathScope,
}

impl Store {
    /// The store over `root`, which must be an existing folder.
    pub fn new(root: &Path) -> Result<Store, String> {
        let root = std::fs::canonicalize(root).map_err(|e| format!("--data {}: {e}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("--data {}: not a folder", root.display()));
        }
        let scope = PathScope::new([root.clone()]);
        Ok(Store { root, scope })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file `rel` names, judged as text and as resolved; `new`: it may not exist yet.
    pub fn resolve(&self, rel: &str, new: bool) -> Result<PathBuf, HttpError> {
        let path = self.root.join(check_relative(rel)?);
        let text = path.to_string_lossy();
        // Where it would lead first (existing folders resolved through links), so a path out of
        // the folder is refused the same way whether or not its file exists.
        let checked = self.scope.check_new(&text).and_then(|()| if new { Ok(()) } else { self.scope.check(&text) });
        match checked {
            Ok(()) => Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(HttpError::new(404, format!("{rel}: no such file"))),
            Err(_) => Err(HttpError::new(400, format!("{rel}: outside the data folder"))),
        }
    }

    /// `GET /api/v1/files`: every design file, sorted by path.
    pub fn list(&self) -> Response {
        let mut out = Vec::new();
        walk(&self.root, "", 0, &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        let truncated = out.len() > MAX_LISTED;
        out.truncate(MAX_LISTED);
        let files: Vec<Value> = out.into_iter().map(|(path, meta)| describe(&path, &meta)).collect();
        Response::json(200, &json!({"files": files, "truncated": truncated}))
    }

    /// The folder `rel` names, judged as text and as resolved (`""`: the data folder);
    /// `new`: it may not exist yet.
    fn resolve_folder(&self, rel: &str, new: bool) -> Result<PathBuf, HttpError> {
        let path = self.root.join(check_folder(rel)?);
        let text = path.to_string_lossy();
        let checked = self.scope.check_new(&text).and_then(|()| if new { Ok(()) } else { self.scope.check(&text) });
        match checked {
            Ok(()) => Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(HttpError::new(404, format!("{rel}: no such folder"))),
            Err(_) => Err(HttpError::new(400, format!("{rel}: outside the data folder"))),
        }
    }

    /// `GET /api/v1/folder/{rel}`: one folder's folders and design files, for the browser's
    /// folder browser. Hidden entries (`.git`, dotfiles) and links are left out.
    pub fn folder(&self, rel: &str) -> Result<Response, HttpError> {
        let dir = self.resolve_folder(rel, false)?;
        let entries = std::fs::read_dir(&dir).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => HttpError::new(404, format!("{rel}: no such folder")),
            _ => HttpError::new(409, format!("{rel}: not a folder")),
        })?;
        let join = |name: &str| if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
        let (mut folders, mut files) = (Vec::new(), Vec::new());
        for entry in entries.flatten().take(MAX_LISTED) {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if name.starts_with('.') {
                continue;
            }
            // Not followed: a link is neither a folder nor a file here.
            let Ok(meta) = entry.metadata() else { continue };
            let path = join(&name);
            if meta.is_dir() && check_folder(&path).is_ok() {
                folders.push(json!({"name": name, "path": path}));
            } else if meta.is_file() && check_relative(&path).is_ok() {
                let mut d = describe(&path, &meta);
                d["name"] = json!(name);
                files.push(d);
            }
        }
        let key = |v: &Value| v["name"].as_str().unwrap_or("").to_lowercase();
        folders.sort_by_key(key);
        files.sort_by_key(key);
        Ok(Response::json(200, &json!({"path": rel, "folders": folders, "files": files})))
    }

    /// `PUT /api/v1/folder/{rel}`: make one new folder inside an existing one.
    pub fn make_folder(&self, rel: &str) -> Result<Response, HttpError> {
        if rel.is_empty() {
            return Err(HttpError::new(400, "name the folder to make"));
        }
        let dir = self.resolve_folder(rel, true)?;
        match std::fs::create_dir(&dir) {
            Ok(()) => Ok(Response::json(201, &json!({"path": rel}))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir() => Ok(Response::json(200, &json!({"path": rel}))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(HttpError::new(409, format!("{rel}: a file has that name"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(HttpError::new(404, format!("{rel}: its folder does not exist"))),
            Err(e) => Err(HttpError::new(500, format!("{rel}: could not make it: {e}"))),
        }
    }

    /// `GET /api/v1/files/{rel}`.
    pub fn get(&self, rel: &str) -> Result<Response, HttpError> {
        let path = self.resolve(rel, false)?;
        let file = std::fs::File::open(&path).map_err(|e| not_found(rel, &e))?;
        let meta = file.metadata().map_err(|e| not_found(rel, &e))?;
        if !meta.is_file() {
            return Err(HttpError::new(409, format!("{rel}: a folder, not a file")));
        }
        Ok(Response { status: 200, headers: Vec::new(), body: Body::File(file, meta.len()) }
            .with("Content-Type", "application/octet-stream")
            .with("Cache-Control", "no-store")
            .with("ETag", etag(&meta)))
    }

    /// `PUT /api/v1/files/{rel}`: the body becomes the file, atomically.
    pub fn put(&self, rel: &str, head: &Head, body: &mut impl std::io::Read, max: u64) -> Result<Response, HttpError> {
        let path = self.resolve(rel, true)?;
        let dir = path.parent().ok_or_else(|| HttpError::new(400, "no folder"))?.to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| HttpError::new(500, format!("{rel}: could not make its folder: {e}")))?;
        // Resolved again now the folders exist: one made meanwhile through a link is caught here.
        let path = self.resolve(rel, true)?;
        if path.is_dir() {
            return Err(HttpError::new(409, format!("{rel}: a folder, not a file")));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (tmp, mut file) = temp_file(&dir, &name).map_err(|e| HttpError::new(500, format!("{rel}: could not save: {e}")))?;
        let written = (|| {
            head.copy_body(body, max, &mut file)?;
            file.sync_all().map_err(|e| HttpError::new(500, format!("{rel}: could not save: {e}")))?;
            drop(file);
            let _held = REPLACE.lock().map_err(|_| HttpError::new(500, "the save lock is broken"))?;
            precondition(head, std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file).as_ref())?;
            std::fs::rename(&tmp, &path).map_err(|e| HttpError::new(500, format!("{rel}: could not save: {e}")))
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        let meta = std::fs::metadata(&path).map_err(|e| HttpError::new(500, format!("{rel}: saved, but {e}")))?;
        Ok(Response::json(200, &describe(rel, &meta)).with("ETag", etag(&meta)))
    }

    /// `DELETE /api/v1/files/{rel}`: files only, never folders.
    pub fn delete(&self, rel: &str, head: &Head) -> Result<Response, HttpError> {
        let path = self.resolve(rel, false)?;
        let _held = REPLACE.lock().map_err(|_| HttpError::new(500, "the save lock is broken"))?;
        let meta = std::fs::symlink_metadata(&path).map_err(|e| not_found(rel, &e))?;
        if meta.is_dir() {
            return Err(HttpError::new(409, format!("{rel}: a folder; only files are deleted")));
        }
        precondition(head, std::fs::metadata(&path).ok().as_ref())?;
        std::fs::remove_file(&path).map_err(|e| HttpError::new(500, format!("{rel}: could not delete: {e}")))?;
        Ok(Response::empty(204))
    }
}

/// `rel` as a path below the data folder, or why it can't be one.
pub fn check_relative(rel: &str) -> Result<PathBuf, HttpError> {
    let path = check_segments(rel)?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(HttpError::new(415, format!("{rel}: not a design or model file (one of .{})", EXTENSIONS.join(", ."))));
    }
    Ok(path)
}

/// `rel` as a folder below the data folder (`""`: the data folder itself), or why it can't be one.
pub fn check_folder(rel: &str) -> Result<PathBuf, HttpError> {
    if rel.is_empty() { Ok(PathBuf::new()) } else { check_segments(rel) }
}

/// `rel` as `/`-separated names below the data folder, judged as text only.
fn check_segments(rel: &str) -> Result<PathBuf, HttpError> {
    let bad = |why: &str| Err(HttpError::new(400, format!("{rel}: {why}")));
    if rel.is_empty() {
        return bad("no file named");
    }
    if rel.len() > MAX_PATH {
        return bad("the path is too long");
    }
    if rel.contains(['\\', ':', '\0']) || rel.chars().any(char::is_control) {
        return bad("a path may not hold \\, : or control characters");
    }
    let segments: Vec<&str> = rel.split('/').collect();
    if segments.len() > MAX_DEPTH {
        return bad("the path is too deep");
    }
    for s in &segments {
        if s.is_empty() {
            return bad("an empty folder name (a leading, doubled or trailing /)");
        }
        if *s == "." || *s == ".." {
            return bad("`.` and `..` are not allowed");
        }
        if s.starts_with('.') {
            return bad("hidden files and folders are not served");
        }
        if s.len() > MAX_NAME {
            return bad("a name is too long");
        }
        if s.ends_with([' ', '.']) {
            return bad("a name may not end in a space or a dot");
        }
    }
    Ok(segments.iter().collect())
}

fn not_found(rel: &str, e: &std::io::Error) -> HttpError {
    if e.kind() == std::io::ErrorKind::NotFound {
        HttpError::new(404, format!("{rel}: no such file"))
    } else {
        HttpError::new(500, format!("{rel}: {e}"))
    }
}

/// A file's version as an HTTP entity tag: its size and modification time.
pub fn etag(meta: &std::fs::Metadata) -> String {
    let t = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    format!("\"{:x}-{:x}\"", meta.len(), t)
}

fn describe(rel: &str, meta: &std::fs::Metadata) -> Value {
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
    json!({"path": rel, "size": meta.len(), "modified": modified, "etag": etag(meta)})
}

/// `If-Match` and `If-None-Match: *` against the file as it is now (`None`: there is none).
fn precondition(head: &Head, current: Option<&std::fs::Metadata>) -> Result<(), HttpError> {
    if let Some(want) = head.header("if-match") {
        let ok = match current {
            Some(meta) => want == "*" || want.split(',').any(|t| t.trim() == etag(meta)),
            None => false,
        };
        if !ok {
            return Err(HttpError::new(412, "the file changed since you read it (or is gone); read it again or save under another name"));
        }
    }
    if head.header("if-none-match").is_some_and(|v| v.trim() == "*") && current.is_some() {
        return Err(HttpError::new(412, "a file of that name already exists"));
    }
    Ok(())
}

/// A new hidden file next to `name` for an upload, made with `create_new` (never someone else's).
fn temp_file(dir: &Path, name: &str) -> std::io::Result<(PathBuf, std::fs::File)> {
    let seed = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let mut last = std::io::Error::other("no temporary name");
    for i in 0..16u32 {
        let tmp = dir.join(format!(".{name}.upload-{}-{:x}", std::process::id(), seed.wrapping_add(i)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The design files under `dir` (`rel` its path in the data folder), without following links
/// or entering hidden folders.
fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<(String, std::fs::Metadata)>) {
    if depth >= MAX_DEPTH || out.len() > MAX_LISTED {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if meta.is_dir() {
            walk(&entry.path(), &path, depth + 1, out);
        } else if meta.is_file() && check_relative(&path).is_ok() {
            out.push((path, meta));
            if out.len() > MAX_LISTED {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_judged_before_the_disk_is_asked() {
        for ok in ["a.sc", "Parts/bracket v2.SC", "x/y/z.step"] {
            assert!(check_relative(ok).is_ok(), "{ok}");
        }
        for (bad, status) in [
            ("", 400),
            ("../x.sc", 400),
            ("a/../../x.sc", 400),
            ("./a.sc", 400),
            ("/etc/passwd.sc", 400),
            ("a//b.sc", 400),
            (".hidden.sc", 400),
            (".git/config.sc", 400),
            ("a\\..\\b.sc", 400),
            ("C:/x.sc", 400),
            ("a.sc.", 400),
            ("a.txt", 415),
            ("Parts", 415),
            ("passwd", 415),
        ] {
            assert_eq!(check_relative(bad).map_err(|e| e.status).err(), Some(status), "{bad}");
        }
        assert!(check_folder("").is_ok() && check_folder("Parts/Old").is_ok());
        assert!(check_folder("../x").is_err() && check_folder(".git").is_err() && check_folder("a/").is_err());
        let deep = format!("{}x.sc", "a/".repeat(MAX_DEPTH));
        assert_eq!(check_relative(&deep).map_err(|e| e.status).err(), Some(400));
        let deepest = format!("{}x.sc", "a/".repeat(MAX_DEPTH - 1));
        assert!(check_relative(&deepest).is_ok());
    }
}
