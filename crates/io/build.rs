//! What the model cache is keyed by. `SOLIDCRAFT_EVALUATION_SOURCE`: the digest of the source a
//! design's model is computed from: the `doc` crate and every workspace crate it is built from,
//! the vendored kernel, the patched dependencies, the locked versions of all others, the
//! workspace's build settings and the cache's own reader. A cache is only the result of evaluating
//! its design when that source computed it, and an update that changed none of it (only the
//! interface, the commands or the other file formats, or the version number) computes the same
//! model. Without `.git` (Docker, Nix or a source archive), the same inputs are hashed from disk;
//! `"unknown"` only when part of that source cannot be read, which never trusts a cache.
//! `SOLIDCRAFT_SOURCE_COMMIT`: the commit, recorded with the cache (`"unknown"` without `git`).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace crates a design's model is computed from: `doc` and every workspace crate it
/// depends on (a test checks the list against their manifests).
pub(crate) const EVALUATION_CRATES: &[&str] =
    &["color", "geom", "profile", "smlib", "sketch", "kernel", "render", "derive", "doc", "format", "viewport"];
/// The other files the saved model depends on, relative to the workspace root.
pub(crate) const EVALUATION_FILES: &[&str] = &["crates/io/build.rs", "crates/io/src/design/native_cache.rs", ".cargo/config.toml"];
/// The committed trees whose every file computes models, hashed as one tree id each: the vendored
/// kernel, so any change under `solidcore/` invalidates the models the old kernel computed.
pub(crate) const EVALUATION_TREES: &[&str] = &["solidcore"];
/// What under `vendor/` does not compute models: notes, and the interface's renderer. The kernel is
/// not under `vendor/`; it is `solidcore/`, in [`EVALUATION_TREES`].
pub(crate) const VENDOR_OUTSIDE: &[&str] = &["README.md", "egui-wgpu"];

fn evaluation_paths() -> Vec<String> {
    let mut paths: Vec<String> = EVALUATION_CRATES.iter().map(|c| format!("crates/{c}")).collect();
    paths.extend(EVALUATION_TREES.iter().map(|t| (*t).to_string()));
    paths.extend(EVALUATION_FILES.iter().map(|f| (*f).to_string()));
    paths
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()).filter(|s| !s.trim().is_empty())
}

/// `Cargo.lock` without the workspace's own packages: their version is the application's, which
/// every release changes; the source they are built from is hashed itself.
fn locked_dependencies(lock: &str) -> String {
    lock.replace("\r\n", "\n")
        .split("\n[[package]]")
        .filter(|block| block.lines().any(|l| l.starts_with("source = ")))
        .collect::<Vec<_>>()
        .join("\n[[package]]")
}

/// The workspace `Cargo.toml` without the application's version.
fn workspace_settings(manifest: &str) -> String {
    let mut section = "";
    let mut kept = Vec::new();
    for line in manifest.lines() {
        if line.starts_with('[') {
            section = line.trim();
        }
        if !(section == "[workspace.package]" && line.trim_start().starts_with("version")) {
            kept.push(line);
        }
    }
    kept.join("\n")
}

fn hex_digest(digest: Sha256) -> String {
    digest.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest of the committed source of the saved model (see the module notes), or `None` when
/// `.git` is absent, `git` is unavailable, or a part of the source is not in the commit.
pub(crate) fn evaluation_source(root: &Path) -> Option<String> {
    if !root.join(".git").exists() {
        return None;
    }
    let paths = evaluation_paths();
    let specs: Vec<String> = paths.iter().map(|p| format!("HEAD:./{p}")).collect();
    let mut args = vec!["rev-parse"];
    args.extend(specs.iter().map(String::as_str));
    let ids = git(root, &args)?;
    if ids.lines().count() != paths.len() {
        return None;
    }
    let mut digest = Sha256::new();
    for (path, id) in paths.iter().zip(ids.lines()) {
        digest.update(format!("{path} {id}\n"));
    }
    // `vendor/`: one line per entry, "<mode> <type> <id>\t<name>".
    for entry in git(root, &["ls-tree", "HEAD", "./vendor/"])?.lines() {
        let name = entry.rsplit(['\t', '/']).next().unwrap_or(entry);
        if !VENDOR_OUTSIDE.contains(&name) {
            digest.update(format!("vendor {entry}\n"));
        }
    }
    digest.update(locked_dependencies(&git(root, &["show", "HEAD:./Cargo.lock"])?));
    digest.update(workspace_settings(&git(root, &["show", "HEAD:./Cargo.toml"])?));
    Some(hex_digest(digest))
}

fn skip_disk_name(name: &str) -> bool {
    name == ".git" || name == ".DS_Store" || name == "__pycache__" || name.starts_with("target") || name.ends_with('~') || name.ends_with(".swp")
}

fn sorted_dir(path: &Path) -> Option<Vec<(String, PathBuf)>> {
    let mut entries = std::fs::read_dir(path).ok()?.collect::<Result<Vec<_>, _>>().ok()?;
    entries.sort_by_key(|e| e.file_name());
    entries.into_iter().map(|e| Some((e.file_name().into_string().ok()?, e.path()))).collect()
}

fn hash_disk_path(root: &Path, path: &Path, digest: &mut Sha256, watch: bool) -> Option<()> {
    let rel = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    let meta = std::fs::symlink_metadata(path).ok()?;
    if watch {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if meta.is_dir() {
        digest.update(format!("dir {}\n{rel}\n", rel.len()));
        for (name, child) in sorted_dir(path)? {
            if !skip_disk_name(&name) {
                hash_disk_path(root, &child, digest, watch)?;
            }
        }
    } else {
        let raw = if meta.file_type().is_symlink() {
            std::fs::read_link(path).ok()?.to_str()?.replace('\\', "/").into_bytes()
        } else if meta.is_file() {
            std::fs::read(path).ok()?
        } else {
            return None;
        };
        let bytes = match std::str::from_utf8(&raw) {
            Ok(text) if text.contains("\r\n") => text.replace("\r\n", "\n").into_bytes(),
            _ => raw,
        };
        digest.update(format!("file {} {}\n{rel}\n", rel.len(), bytes.len()));
        digest.update(&bytes);
    }
    Some(())
}

/// Fallback when `.git` or `git` is unavailable: hash the same evaluation inputs from `root`.
pub(crate) fn evaluation_source_from_disk(root: &Path, watch: bool) -> Option<String> {
    let mut digest = Sha256::new();
    digest.update("disk-v1\n");
    for rel in evaluation_paths() {
        hash_disk_path(root, &root.join(rel), &mut digest, watch)?;
    }
    let vendor = root.join("vendor");
    if watch {
        println!("cargo:rerun-if-changed={}", vendor.display());
    }
    for (name, child) in sorted_dir(&vendor)? {
        if !VENDOR_OUTSIDE.contains(&name.as_str()) && !skip_disk_name(&name) {
            hash_disk_path(root, &child, &mut digest, watch)?;
        }
    }
    for (file, filter) in [("Cargo.lock", locked_dependencies as fn(&str) -> String), ("Cargo.toml", workspace_settings)] {
        let path = root.join(file);
        if watch {
            println!("cargo:rerun-if-changed={}", path.display());
        }
        let text = filter(&std::fs::read_to_string(&path).ok()?);
        digest.update(format!("meta {file} {}\n{text}", text.len()));
    }
    Some(hex_digest(digest))
}

fn main() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = crate_dir.join("../..");
    let has_git = root.join(".git").exists();
    let commit = has_git.then(|| git(&crate_dir, &["rev-parse", "HEAD"])).flatten().map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_SOURCE_COMMIT={commit}");
    let source = evaluation_source(&root).or_else(|| evaluation_source_from_disk(&root, true)).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_SOURCE={source}");
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_CRATES={}", EVALUATION_CRATES.join(","));
    // A new commit in this checkout (or worktree) restamps the build.
    if let Some(dir) = has_git.then(|| git(&crate_dir, &["rev-parse", "--git-dir"])).flatten() {
        println!("cargo:rerun-if-changed={}/logs/HEAD", dir.trim());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
