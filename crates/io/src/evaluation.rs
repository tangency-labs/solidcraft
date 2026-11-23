//! What the model cache is keyed by. `SOLIDCRAFT_EVALUATION_SOURCE`: the digest of the committed
//! source a design's model is computed from: the `doc` crate and every workspace crate it is
//! built from, the vendored kernel, the patched dependencies, the locked versions of all others,
//! the workspace's build settings and the cache's own reader. A cache is only the result of evaluating its design when
//! that source computed it, and an update that changed none of it (only the interface, the
//! commands or the other file formats, or the version number) computes the same model.
//! `SOLIDCRAFT_SOURCE_COMMIT`: the commit, recorded with the cache. Both are "unknown" without git
//! (a source tarball), which never trusts a cache.
//!
//! A working tree with uncommitted changes has a digest of its own: the trees below are the
//! *committed* ones, so without the marker a build from a changed tree would carry the digest of
//! the commit it sits on, and could write a cache that a clean build of that commit trusts.
//!
//! This file is included by `build.rs` (`#[path = "src/evaluation.rs"]`), so it must build as a
//! build script module too: standard library and `sha2` only.

use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

/// The workspace crates a design's model is computed from: `doc` and every workspace crate it
/// depends on (a test checks the list against their manifests).
pub const EVALUATION_CRATES: [&str; 12] =
    ["color", "geom", "profile", "smlib", "sketch", "kernel", "render", "derive", "develop", "doc", "format", "viewport"];
/// The other files the saved model depends on, relative to the workspace root.
const EVALUATION_FILES: [&str; 3] = ["crates/io/build.rs", "crates/io/src/design/native_cache.rs", ".cargo/config.toml"];
/// The committed trees whose every file computes models, hashed as one tree id each: the vendored
/// kernel, so any change under `solidcore/` invalidates the models the old kernel computed.
const EVALUATION_TREES: [&str; 1] = ["solidcore"];
/// What under `vendor/` does not compute models: notes, and the interface's renderer. The kernel is
/// not under `vendor/`; it is `solidcore/`, in [`EVALUATION_TREES`].
const VENDOR_OUTSIDE: [&str; 2] = ["README.md", "egui-wgpu"];

/// What under `solidcore/` is not compiled into the kernel: the site, its plans and reports, its
/// fixtures and the tests and goldens that read them, and the rest of its repository's own files.
/// A change there computes the same model, so it does not invalidate a cache. Matched against the
/// path's first segment, or as a directory anywhere in it.
const SOLIDCORE_OUTSIDE: [&str; 10] = ["docs", "plan", "fixtures", "harness", "scripts", "site", "reports", "bench", ".github", "assets"];
/// Test-only directories under a crate: `tests/`, `benches/`, `examples/` and the goldens' own
/// crate. They are never built into a design's model.
const DIRECTORY_OUTSIDE: [&str; 4] = ["tests", "benches", "examples", "goldens"];

pub fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()).filter(|s| !s.trim().is_empty())
}

/// The committed `Cargo.lock` without the workspace's own packages: their version is the
/// application's, which every release changes; the source they are built from is hashed itself.
fn locked_dependencies(lock: &str) -> String {
    lock.split("\n[[package]]").filter(|block| block.lines().any(|l| l.starts_with("source = "))).collect::<Vec<_>>().join("\n[[package]]")
}

/// The committed workspace `Cargo.toml` without the application's version.
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

/// Whether `path` — after `solidcore/` — is compiled into the kernel: the tests, the goldens'
/// fixtures and the repository's own files are not.
fn solidcore_evaluated(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or(path);
    if SOLIDCORE_OUTSIDE.contains(&first) {
        return false;
    }
    let mut dirs = path.split('/');
    dirs.next();
    dirs.next_back();
    !dirs.any(|d| DIRECTORY_OUTSIDE.contains(&d))
}

/// The digest of the committed source of the saved model (see the module notes), or `None` when
/// a part of it is not in the commit.
pub fn source(root: &Path) -> Option<String> {
    let mut paths: Vec<String> = EVALUATION_CRATES.iter().map(|c| format!("crates/{c}")).collect();
    paths.extend(EVALUATION_TREES.iter().map(|t| t.to_string()));
    paths.extend(EVALUATION_FILES.iter().map(|f| f.to_string()));
    let specs: Vec<String> = paths.iter().map(|p| format!("HEAD:./{p}")).collect();
    let mut args = vec!["rev-parse"];
    args.extend(specs.iter().map(String::as_str));
    let ids = git(root, &args)?;
    if ids.lines().count() != paths.len() {
        return None;
    }
    let mut digest = Sha256::new();
    for (path, id) in paths.iter().zip(ids.lines()) {
        // The vendored kernel's tree: every file that computes models, not the repository's own.
        if EVALUATION_TREES.contains(&path.as_str()) {
            let prefix = format!("{path}/");
            let listing = git(root, &["ls-tree", "-r", "HEAD", &format!("./{prefix}")])?;
            for line in listing.lines() {
                let (meta, file) = line.split_once('\t')?;
                let id = meta.split_whitespace().nth(2)?;
                let Some(file) = file.strip_prefix(&prefix) else { continue };
                if solidcore_evaluated(file) {
                    digest.update(format!("{path} {file} {id}\n"));
                }
            }
            continue;
        }
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
    // The committed trees above are the commit's; a working tree with changes of its own computes
    // a model this digest would otherwise say is the commit's (#7168's own hazard).
    if dirty(root) {
        digest.update(b"dirty\n");
    }
    Some(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Whether the working tree has changes to tracked files. Untracked files are not asked about:
/// a build ignores them, so they cannot change the model it computes.
fn dirty(root: &Path) -> bool {
    git(root, &["status", "--porcelain", "--untracked-files=no"]).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn write(root: &Path, path: &str, text: &str) {
        let p = root.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn repo(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("solidcraft-evaluation-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for krate in EVALUATION_CRATES {
            write(&root, &format!("crates/{krate}/src/lib.rs"), "pub fn x() {}\n");
        }
        write(&root, "crates/io/build.rs", "fn main() {}\n");
        write(&root, "crates/io/src/design/native_cache.rs", "pub fn y() {}\n");
        write(&root, ".cargo/config.toml", "[build]\n");
        write(&root, "solidcore/Cargo.toml", "[workspace]\n");
        write(&root, "solidcore/Cargo.lock", "version = 4\n");
        write(&root, "solidcore/crates/solidcore/src/lib.rs", "pub fn kernel() {}\n");
        write(&root, "solidcore/crates/solidcore/tests/a.rs", "fn t() {}\n");
        write(&root, "solidcore/crates/goldens/src/lib.rs", "pub fn g() {}\n");
        write(&root, "solidcore/docs/reference.md", "# reference\n");
        write(&root, "solidcore/plan/STATUS.md", "status\n");
        write(&root, "solidcore/fixtures/a/b.step", "ISO\n");
        write(&root, "vendor/README.md", "notes\n");
        write(&root, "vendor/egui/src/lib.rs", "pub fn e() {}\n");
        write(&root, "Cargo.lock", "version = 4\n");
        write(&root, "Cargo.toml", "[workspace.package]\nversion = \"0.46.2\"\n");
        git(&root, &["init", "-q"]);
        commit(&root, "one");
        root
    }

    fn commit(root: &Path, message: &str) {
        git(root, &["add", "-A"]);
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["-c", "user.name=test", "-c", "user.email=test@example.com", "commit", "-q", "-m", message])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// A change to a file the kernel's model is not computed from keeps the digest, and a change
    /// to one it is computed from moves it; a clean tree has one digest.
    #[test]
    fn only_the_source_the_model_is_computed_from_is_keyed() {
        let root = repo("scope");
        let clean = source(&root).expect("a clean digest");
        assert_eq!(source(&root).as_deref(), Some(clean.as_str()), "a clean tree has one digest");
        for outside in [
            "solidcore/docs/reference.md",
            "solidcore/plan/STATUS.md",
            "solidcore/fixtures/a/b.step",
            "solidcore/crates/solidcore/tests/a.rs",
            "solidcore/crates/goldens/src/lib.rs",
        ] {
            write(&root, outside, "changed\n");
            commit(&root, outside);
            assert_eq!(source(&root).as_deref(), Some(clean.as_str()), "{outside} computes no model");
        }
        write(&root, "solidcore/crates/solidcore/src/lib.rs", "pub fn kernel() { }\n");
        commit(&root, "kernel");
        assert_ne!(source(&root).as_deref(), Some(clean.as_str()), "the kernel computes the model");
        let _ = fs::remove_dir_all(&root);
    }

    /// A working tree with uncommitted changes has a digest of its own, and the committed one comes
    /// back when they are reverted: a build from a changed tree cannot write a cache a clean build
    /// of the same commit trusts.
    #[test]
    fn a_changed_working_tree_has_its_own_digest() {
        let root = repo("dirty");
        let clean = source(&root).expect("a clean digest");
        write(&root, "solidcore/crates/solidcore/src/lib.rs", "pub fn kernel() { edited }\n");
        assert_ne!(source(&root).as_deref(), Some(clean.as_str()), "a changed working tree is not the commit");
        write(&root, "solidcore/crates/solidcore/src/lib.rs", "pub fn kernel() {}\n");
        assert_eq!(source(&root).as_deref(), Some(clean.as_str()), "reverting restores the commit's digest");
        // A change to a file the model is not computed from still marks the tree: it is not the
        // commit, and the marker can only invalidate more.
        write(&root, "solidcore/docs/reference.md", "# changed\n");
        assert_ne!(source(&root).as_deref(), Some(clean.as_str()), "any tracked change is not the commit");
        let _ = fs::remove_dir_all(&root);
    }
}
