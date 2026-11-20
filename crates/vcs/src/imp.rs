//! Non-wasm implementation of the VCS operations using gix (MIT OR Apache-2.0).
//!
//! The git object store and HEAD reference are managed directly by these functions.
//! The git index (.git/index) is intentionally not updated: callers are expected to
//! use only the `vcs.*` commands, not mix with raw `git` invocations.

use std::path::{Path, PathBuf};

use gix::bstr::{BStr, ByteSlice as _};

use crate::error::VcsError;

pub type Result<T, E = VcsError> = std::result::Result<T, E>;

// ── Public types ─────────────────────────────────────────────────────────────

/// A single entry in the commit history.
pub struct CommitInfo {
    pub id: String,
    pub message: String,
    pub author: String,
    /// Unix timestamp (seconds since epoch).
    pub time: i64,
}

/// Current repository state relative to the working design.
pub struct StatusInfo {
    pub initialized: bool,
    pub changed: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Initialize a git repository in `design_dir`.
///
/// Returns `(repo_path, created)`: `created` is `true` when this call made the repository,
/// `false` when one already existed.
pub fn init(design_dir: &Path) -> Result<(PathBuf, bool)> {
    let already_exists = design_dir.join(".git").exists();
    if !already_exists {
        gix::init(design_dir).map_err(|e| VcsError::Git(e.to_string()))?;
    }

    // Write a .gitignore on first init; leave an existing one alone.
    let gitignore = design_dir.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(
            &gitignore,
            "# SolidCraft binary design containers — history is in *.model.json\n\
             *.solidcraft\n\
             *.sc\n",
        )?;
    }

    Ok((design_dir.to_owned(), !already_exists))
}

/// Commit the design's parametric JSON as `<stem>.model.json`.
///
/// Any other files already tracked in the tree are preserved. Returns the full commit id.
pub fn commit(design_dir: &Path, stem: &str, json: &str, message: &str, author_name: &str, author_email: &str) -> Result<String> {
    // Check the repo exists before touching the filesystem.
    if !design_dir.join(".git").exists() {
        return Err(VcsError::NotARepo);
    }
    let repo = gix::open(design_dir).map_err(|e| VcsError::Git(e.to_string()))?;

    // Refuse a detached HEAD: commit_as("HEAD", …) would create an orphaned commit.
    let head = repo.head().map_err(|e| VcsError::Git(e.to_string()))?;
    if head.referent_name().is_none() {
        return Err(VcsError::Git("HEAD is detached; checkout a branch before committing".into()));
    }

    // Write the JSON sidecar to disk so the working tree stays consistent.
    let json_filename = format!("{stem}.model.json");
    let json_path = design_dir.join(&json_filename);
    std::fs::write(&json_path, json.as_bytes())?;

    // ── Write blob ────────────────────────────────────────────────────────────
    let blob_id = repo.write_object(gix::objs::BlobRef { data: json.as_bytes() }).map_err(|e| VcsError::Git(e.to_string()))?.detach();

    // ── Build tree (merge with HEAD tree to preserve other tracked files) ─────
    // head_id() failing here means unborn HEAD (first commit) — no parent.
    let parent_id: Option<gix::ObjectId> = repo.head_id().ok().map(|id| id.detach());
    let mut entries: Vec<gix::objs::tree::Entry> = Vec::new();

    if let Some(parent) = parent_id {
        entries = read_existing_tree_entries(&repo, parent, &json_filename)?;
    }

    // Add/update the model JSON entry and sort using Git's tree order.
    entries.push(gix::objs::tree::Entry { mode: gix::objs::tree::EntryKind::Blob.into(), filename: json_filename.as_bytes().into(), oid: blob_id });
    entries.sort_by_key(git_sort_key);

    let tree_id = repo.write_object(gix::objs::Tree { entries }).map_err(|e| VcsError::Git(e.to_string()))?.detach();

    // ── Create commit via gix's high-level API ────────────────────────────────
    let time = gix::date::Time::now_local_or_utc();
    let author = gix::actor::SignatureRef { name: author_name.as_bytes().as_bstr(), email: author_email.as_bytes().as_bstr(), time };

    // `commit_as` writes the commit object and updates the HEAD reference.
    let commit_id = repo.commit_as(author, author, "HEAD", message, tree_id, parent_id).map_err(|e| VcsError::Git(e.to_string()))?;

    Ok(commit_id.to_string())
}

/// Return up to `limit` commits from HEAD, newest first.
pub fn log(design_dir: &Path, limit: usize) -> Result<Vec<CommitInfo>> {
    let repo = gix::open(design_dir).map_err(|e| VcsError::Git(e.to_string()))?;
    let head_id = match repo.head_id() {
        Ok(id) => id.detach(),
        Err(_) => return Ok(Vec::new()),
    };

    let limit = limit.clamp(1, 10_000);
    let mut result = Vec::with_capacity(limit.min(64));
    let mut current = head_id;

    for _ in 0..limit {
        let obj = repo.find_object(current).map_err(|e| VcsError::Git(e.to_string()))?;
        let commit_obj = obj.try_into_commit().map_err(|_| VcsError::Git("not a commit object".into()))?;
        let decoded = commit_obj.decode().map_err(|e| VcsError::Git(e.to_string()))?;

        result.push(CommitInfo {
            id: current.to_string(),
            message: decoded.message.to_str_lossy().into_owned(),
            author: decoded.author.name.to_str_lossy().into_owned(),
            time: decoded.author.time.seconds,
        });

        match decoded.parents().next() {
            Some(parent_id) => current = parent_id.to_owned(),
            None => break,
        }
    }

    Ok(result)
}

/// Return the current VCS status for this design.
pub fn status(design_dir: &Path, stem: &str, current_json: &str) -> Result<StatusInfo> {
    let dot_git = design_dir.join(".git");
    if !dot_git.exists() {
        return Ok(StatusInfo { initialized: false, changed: true, branch: None, head: None });
    }

    let repo = gix::open(design_dir).map_err(|e| VcsError::Git(e.to_string()))?;

    let branch = repo.head().ok().and_then(|h| h.referent_name().map(|n| bstr_to_string(n.as_bstr())));

    let head = repo.head_id().ok().map(|id| id.to_string());

    let json_filename = format!("{stem}.model.json");
    let committed = read_blob_at_head(&repo, &json_filename).unwrap_or_default();
    let changed = committed != current_json.as_bytes();

    Ok(StatusInfo { initialized: true, changed, branch, head })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bstr_to_string(b: &BStr) -> String {
    b.to_str_lossy().into_owned()
}

/// Sort key for a tree entry following Git's ordering rule:
/// tree (directory) entries sort as if their name ends with '/'.
fn git_sort_key(e: &gix::objs::tree::Entry) -> Vec<u8> {
    let mut key = e.filename.to_vec();
    if e.mode.kind() == gix::objs::tree::EntryKind::Tree {
        key.push(b'/');
    }
    key
}

/// Return the existing tree entries from a commit, excluding `skip_filename`.
///
/// Errors are propagated so a corrupt parent tree is never silently treated as
/// an empty tree (which would drop all other tracked files from the tip).
fn read_existing_tree_entries(repo: &gix::Repository, parent: gix::ObjectId, skip_filename: &str) -> Result<Vec<gix::objs::tree::Entry>> {
    let parent_obj = repo.find_object(parent).map_err(|e| VcsError::Git(e.to_string()))?;
    let commit_obj = parent_obj.try_into_commit().map_err(|_| VcsError::Git("parent is not a commit object".into()))?;
    let decoded = commit_obj.decode().map_err(|e| VcsError::Git(e.to_string()))?;
    let tree_id = decoded.tree();
    let tree_obj = repo.find_object(tree_id).map_err(|e| VcsError::Git(e.to_string()))?;
    let tree_ref_obj = tree_obj.try_into_tree().map_err(|_| VcsError::Git("parent tree is not a tree object".into()))?;
    let tree = tree_ref_obj.decode().map_err(|e| VcsError::Git(e.to_string()))?;
    let skip: &BStr = skip_filename.as_bytes().as_bstr();

    Ok(tree
        .entries
        .iter()
        .filter(|e| e.filename != skip)
        .map(|e| gix::objs::tree::Entry { mode: e.mode, filename: e.filename.to_owned(), oid: e.oid.to_owned() })
        .collect())
}

/// Read the raw bytes of a file from the HEAD commit's tree, if it exists.
fn read_blob_at_head(repo: &gix::Repository, filename: &str) -> Option<Vec<u8>> {
    let head_id = repo.head_id().ok()?.detach();
    let commit_obj = repo.find_object(head_id).ok()?.try_into_commit().ok()?;
    let decoded = commit_obj.decode().ok()?;
    let tree_id = decoded.tree();
    let tree_obj = repo.find_object(tree_id).ok()?.try_into_tree().ok()?;
    let tree = tree_obj.decode().ok()?;

    let bfilename: &BStr = filename.as_bytes().as_bstr();
    let entry = tree.entries.iter().find(|e| e.filename == bfilename)?;
    let blob = repo.find_object(entry.oid.to_owned()).ok()?;
    let blob_data = blob.try_into_blob().ok()?;
    Some(blob_data.data.to_owned())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn commit_before_init_returns_not_a_repo() {
        let dir = make_dir();
        let err = commit(dir.path(), "design", "{}", "first", "Test", "test@test.local").unwrap_err();
        assert!(matches!(err, VcsError::NotARepo), "expected NotARepo, got: {err}");
    }

    #[test]
    fn init_creates_repo_and_gitignore() {
        let dir = make_dir();
        let (repo_path, created) = init(dir.path()).unwrap();
        assert!(created);
        assert!(repo_path.join(".git").exists());
        let ignore = fs::read_to_string(repo_path.join(".gitignore")).unwrap();
        assert!(ignore.contains("*.solidcraft"));
        assert!(ignore.contains("*.sc"));

        // Second call: not created again.
        let (_, created2) = init(dir.path()).unwrap();
        assert!(!created2);
    }

    #[test]
    fn commit_creates_sidecar_and_is_readable_via_log() {
        let dir = make_dir();
        init(dir.path()).unwrap();
        let id = commit(dir.path(), "part", r#"{"v":1}"#, "initial", "Alice", "alice@test.local").unwrap();
        assert_eq!(id.len(), 40);

        let sidecar = dir.path().join("part.model.json");
        assert!(sidecar.exists());
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), r#"{"v":1}"#);

        let entries = log(dir.path(), 20).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "initial");
        assert_eq!(entries[0].author, "Alice");
    }

    #[test]
    fn second_commit_preserves_other_tracked_files() {
        let dir = make_dir();
        init(dir.path()).unwrap();
        // First commit of "part"
        commit(dir.path(), "part", r#"{"v":1}"#, "first", "A", "a@t.local").unwrap();
        // Write and commit a second design in the same folder
        commit(dir.path(), "part2", r#"{"v":2}"#, "second design", "A", "a@t.local").unwrap();
        // Now commit an update to part; part2.model.json must still be in the tip tree
        commit(dir.path(), "part", r#"{"v":2}"#, "update part", "A", "a@t.local").unwrap();

        let repo = gix::open(dir.path()).unwrap();
        let head_id = repo.head_id().unwrap().detach();
        let commit_obj = repo.find_object(head_id).unwrap().try_into_commit().unwrap();
        let decoded = commit_obj.decode().unwrap();
        let tree_id = decoded.tree();
        let tree_obj = repo.find_object(tree_id).unwrap().try_into_tree().unwrap();
        let tree = tree_obj.decode().unwrap();
        let names: Vec<String> = tree.entries.iter().map(|e| e.filename.to_str_lossy().into_owned()).collect();
        assert!(names.contains(&"part.model.json".to_string()), "part missing from tip tree");
        assert!(names.contains(&"part2.model.json".to_string()), "part2 missing from tip tree");
    }

    #[test]
    fn status_reports_changed_after_edit() {
        let dir = make_dir();
        init(dir.path()).unwrap();
        commit(dir.path(), "part", r#"{"v":1}"#, "first", "A", "a@t.local").unwrap();

        let s1 = status(dir.path(), "part", r#"{"v":1}"#).unwrap();
        assert!(s1.initialized);
        assert!(!s1.changed);

        let s2 = status(dir.path(), "part", r#"{"v":2}"#).unwrap();
        assert!(s2.changed);
    }

    #[test]
    fn log_limit_is_respected() {
        let dir = make_dir();
        init(dir.path()).unwrap();
        for i in 0..5u32 {
            commit(dir.path(), "part", &format!(r#"{{"v":{i}}}"#), &format!("commit {i}"), "A", "a@t.local").unwrap();
        }
        let entries = log(dir.path(), 3).unwrap();
        assert_eq!(entries.len(), 3);
        // newest first
        assert_eq!(entries[0].message, "commit 4");
    }
}
