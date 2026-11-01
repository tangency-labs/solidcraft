//! What the model cache is keyed by. `SOLIDCRAFT_EVALUATION_SOURCE`: the digest of the committed
//! source a design's model is computed from: the `doc` crate and every workspace crate it is
//! built from, the patched dependencies, the locked versions of all others, the workspace's build
//! settings and the cache's own reader. A cache is only the result of evaluating its design when
//! that source computed it, and an update that changed none of it (only the interface, the
//! commands or the other file formats, or the version number) computes the same model.
//! `SOLIDCRAFT_SOURCE_COMMIT`: the commit, recorded with the cache. Both are "unknown" without git
//! (a source tarball), which never trusts a cache.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace crates a design's model is computed from: `doc` and every workspace crate it
/// depends on (a test checks the list against their manifests).
const EVALUATION_CRATES: [&str; 7] = ["geom", "sketch", "kernel", "render", "derive", "doc", "cam"];
/// The other files the saved model depends on, relative to the workspace root.
const EVALUATION_FILES: [&str; 3] = ["crates/io/build.rs", "crates/io/src/design/native_cache.rs", ".cargo/config.toml"];
/// What under `vendor/` does not compute models: notes, and the interface's renderer.
const VENDOR_OUTSIDE: [&str; 2] = ["README.md", "egui-wgpu"];

fn git(root: &Path, args: &[&str]) -> Option<String> {
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

/// The digest of the committed source of the saved model (see the module notes), or `None` when
/// a part of it is not in the commit.
fn evaluation_source(root: &Path) -> Option<String> {
    let mut paths: Vec<String> = EVALUATION_CRATES.iter().map(|c| format!("crates/{c}")).collect();
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
    Some(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn main() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = crate_dir.join("../..");
    let commit = git(&crate_dir, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_SOURCE_COMMIT={commit}");
    let source = evaluation_source(&root).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_SOURCE={source}");
    println!("cargo:rustc-env=SOLIDCRAFT_EVALUATION_CRATES={}", EVALUATION_CRATES.join(","));
    // A new commit in this checkout (or worktree) restamps the build.
    if let Some(dir) = git(&crate_dir, &["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={}/logs/HEAD", dir.trim());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
