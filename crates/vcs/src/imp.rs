//! Non-wasm implementation of the VCS operations using gix (MIT OR Apache-2.0).

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
    // Write the JSON sidecar to disk so the working tree stays consistent.
    let json_filename = format!("{stem}.model.json");
    let json_path = design_dir.join(&json_filename);
    std::fs::write(&json_path, json.as_bytes())?;

    let repo = gix::open(design_dir).map_err(|e| VcsError::Git(e.to_string()))?;

    // ── Write blob ────────────────────────────────────────────────────────────
    let blob_id = repo.write_object(gix::objs::BlobRef { data: json.as_bytes() }).map_err(|e| VcsError::Git(e.to_string()))?.detach();

    // ── Build tree (merge with HEAD tree to preserve other tracked files) ─────
    let parent_id: Option<gix::ObjectId> = repo.head_id().ok().map(|id| id.detach());
    let mut entries: Vec<gix::objs::tree::Entry> = Vec::new();

    if let Some(parent) = parent_id
        && let Some(existing) = read_existing_tree_entries(&repo, parent, &json_filename)
    {
        entries = existing;
    }

    // Add/update the model JSON entry and keep the tree sorted (git requires it).
    entries.push(gix::objs::tree::Entry { mode: gix::objs::tree::EntryKind::Blob.into(), filename: json_filename.as_bytes().into(), oid: blob_id });
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));

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

/// Return the existing tree entries from a commit, excluding `skip_filename`.
fn read_existing_tree_entries(repo: &gix::Repository, parent: gix::ObjectId, skip_filename: &str) -> Option<Vec<gix::objs::tree::Entry>> {
    let parent_obj = repo.find_object(parent).ok()?;
    let commit_obj = parent_obj.try_into_commit().ok()?;
    let decoded = commit_obj.decode().ok()?;
    let tree_id = decoded.tree();
    let tree_obj = repo.find_object(tree_id).ok()?;
    let tree_ref_obj = tree_obj.try_into_tree().ok()?;
    let tree = tree_ref_obj.decode().ok()?;
    let skip: &BStr = skip_filename.as_bytes().as_bstr();

    let entries = tree
        .entries
        .iter()
        .filter(|e| e.filename != skip)
        .map(|e| gix::objs::tree::Entry { mode: e.mode, filename: e.filename.to_owned(), oid: e.oid.to_owned() })
        .collect();

    Some(entries)
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
