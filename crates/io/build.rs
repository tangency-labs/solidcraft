//! What the model cache is keyed by. `SOLIDCRAFT_EVALUATION_SOURCE`: the digest of the committed
//! source a design's model is computed from: the `doc` crate and every workspace crate it is
//! built from, the patched dependencies, the locked versions of all others, the workspace's build
//! settings and the cache's own reader. A cache is only the result of evaluating its design when
//! that source computed it, and an update that changed none of it (only the interface, the
//! commands or the other file formats, or the version number) computes the same model.
//! `SOLIDCRAFT_IMPORT_SOURCE`: the digest of the committed source a STEP import is read with (the
//! import cache's key, #9456): the `kernel` crate and every crate of the workspace or `vendor/`
//! it is built from (taken from `Cargo.lock`), every manifest (they choose the dependencies'
//! features), the locked versions of all other dependencies, the workspace's build settings and
//! the import cache's own reader. A STEP text read by that source gives the same import whatever
//! else changed, so the interface, the commands, sketches and the timeline's other features may.
//! `SOLIDCRAFT_SOURCE_COMMIT`: the commit, recorded with the cache. All are "unknown" without git
//! (a source tarball), which never trusts a cache.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace crates a design's model is computed from: `doc` and every workspace crate it
/// depends on (a test checks the list against their manifests).
const EVALUATION_CRATES: [&str; 6] = ["geom", "sketch", "kernel", "render", "derive", "doc"];
/// The other files the saved model depends on, relative to the workspace root.
const EVALUATION_FILES: [&str; 3] = ["crates/io/build.rs", "crates/io/src/design/native_cache.rs", ".cargo/config.toml"];
/// The other files a stored import depends on, relative to the workspace root.
const IMPORT_FILES: [&str; 3] = ["crates/io/build.rs", "crates/io/src/design/import_cache.rs", ".cargo/config.toml"];
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

/// The packages `root` is built from, by `Cargo.lock` (every kind of dependency, so more rather
/// than fewer): each as `(name, version, whether it comes from a registry)`. `None` when the lock
/// does not name `root` or a dependency it names.
fn lock_closure(lock: &str, root: &str) -> Option<Vec<(String, String, bool)>> {
    let quoted = |line: &str| line.split('"').nth(1).map(str::to_string);
    let mut packages = Vec::new();
    for block in lock.split("\n[[package]]").skip(1) {
        let field = |key: &str| block.lines().find(|l| l.starts_with(&format!("{key} = "))).and_then(quoted);
        let deps = block
            .split_once("dependencies = [")
            .map_or(Vec::new(), |(_, rest)| rest.split(']').next().unwrap_or_default().lines().filter_map(quoted).collect::<Vec<_>>());
        packages.push((field("name")?, field("version")?, field("source").is_some(), deps));
    }
    // A dependency is "name" when one version is locked, else "name version".
    let find = |dep: &str| {
        let mut words = dep.split(' ');
        let (name, version) = (words.next()?, words.next());
        let mut found = packages.iter().enumerate().filter(|(_, p)| p.0 == name && version.is_none_or(|v| p.1 == v));
        let first = found.next()?.0;
        found.next().is_none().then_some(first)
    };
    let mut todo = vec![find(root)?];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(i) = todo.pop() {
        if seen.insert(i) {
            for dep in &packages.get(i)?.3 {
                todo.push(find(dep)?);
            }
        }
    }
    seen.into_iter().map(|i| packages.get(i).map(|p| (p.0.clone(), p.1.clone(), p.2))).collect()
}

/// The digest of the committed source of a stored STEP import (see the module notes), or `None`
/// when a part of it is not in the commit or a crate it is built from has no known folder.
fn import_source(root: &Path) -> Option<String> {
    let lock = git(root, &["show", "HEAD:./Cargo.lock"])?;
    let manifest = git(root, &["show", "HEAD:./Cargo.toml"])?;
    // `name = { path = "folder" ...`: the folder of a workspace crate or of a patched one.
    let folder = |name: &str| {
        let prefix = format!("{name} = {{ path = \"");
        manifest.lines().find_map(|l| l.strip_prefix(prefix.as_str())?.split('"').next().map(str::to_string))
    };
    let mut paths = Vec::new();
    for (name, _, registry) in lock_closure(&lock, "solidcraft-kernel")? {
        if !registry {
            paths.push(folder(&name)?);
        }
    }
    paths.extend(IMPORT_FILES.iter().map(|f| f.to_string()));
    paths.extend(
        git(root, &["ls-tree", "-r", "--name-only", "--full-tree", "HEAD"])?.lines().filter(|f| f.ends_with("/Cargo.toml")).map(str::to_string),
    );
    paths.sort();
    paths.dedup();
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
    digest.update(locked_dependencies(&lock));
    digest.update(workspace_settings(&manifest));
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
    let import = import_source(&root).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=SOLIDCRAFT_IMPORT_SOURCE={import}");
    // A new commit in this checkout (or worktree) restamps the build.
    if let Some(dir) = git(&crate_dir, &["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={}/logs/HEAD", dir.trim());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
