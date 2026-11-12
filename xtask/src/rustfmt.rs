//! `cargo xtask fmt`: rustfmt over the files `cargo fmt --all` formats, a few at a time.
//!
//! `cargo fmt --all` hands the root file of every target of the workspace and of its local path
//! dependencies (solidcore's crates) to one `rustfmt` process per edition. Here that is several
//! hundred paths, and Windows starts no process whose command line is longer than 32,767
//! characters: `The filename or extension is too long. (os error 206)`, before any file is read.
//! Naming packages (`cargo fmt -p`) does not help, since one package's test files alone can be
//! too many.
//!
//! So this collects the same files the way `cargo fmt --all` does (`cargo metadata --no-deps`,
//! then the manifest of each path dependency) and runs `rustfmt` on as many at a time as fit,
//! on every platform.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The most characters of file arguments one `rustfmt` run is given. Windows allows a whole
/// command line 32,767 UTF-16 units (`CreateProcessW`); the rest is room for the program and
/// its options.
const ARGS_MAX: usize = 30_000;

/// The root files to format, by edition. rustfmt follows each one's `mod` lines itself.
pub type Roots = BTreeMap<String, BTreeSet<PathBuf>>;

/// Formats the workspace and its path dependencies, or with `check` only reports what is not
/// formatted. Every run is made, so one failure does not hide the others.
pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let roots = roots(&root.join("Cargo.toml"), &mut |manifest| {
        if !manifest.exists() {
            return Ok(None);
        }
        let mut c = crate::cargo();
        c.args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"]).arg(manifest);
        let out = c.output().map_err(|e| format!("cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(format!("cargo metadata --manifest-path {}: {}", manifest.display(), String::from_utf8_lossy(&out.stderr).trim()));
        }
        serde_json::from_slice(&out.stdout).map(Some).map_err(|e| format!("cargo metadata --manifest-path {}: {e}", manifest.display()))
    })?;
    let files: Vec<(&str, Vec<PathBuf>)> = roots.iter().map(|(edition, files)| (edition.as_str(), files.iter().cloned().collect())).collect();
    let runs: Vec<(&str, &[PathBuf])> =
        files.iter().flat_map(|(edition, files)| batches(files, ARGS_MAX).into_iter().map(|b| (*edition, b))).collect();
    let what = if check { "rustfmt --check" } else { "rustfmt" };
    eprintln!("$ {what} ({} files in {} runs)", files.iter().map(|(_, f)| f.len()).sum::<usize>(), runs.len());
    // As `cargo fmt` finds it: `RUSTFMT`, else the toolchain's `rustfmt`.
    let program = std::env::var_os("RUSTFMT").filter(|p| !p.is_empty()).unwrap_or_else(|| "rustfmt".into());
    let mut failed = 0;
    for (edition, batch) in &runs {
        let mut c = Command::new(&program);
        c.current_dir(root).args(["--edition", edition]);
        if check {
            c.arg("--check");
        }
        let status = c.args(*batch).status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
        failed += usize::from(!status.success());
    }
    match (failed, check) {
        (0, _) => Ok(()),
        (_, true) => Err(format!("{what}: {failed} of {} runs found files that are not formatted (`cargo xtask fmt` formats them)", runs.len())),
        (_, false) => Err(format!("{what}: {failed} of {} runs failed", runs.len())),
    }
}

/// The root files `cargo fmt --all` formats from `manifest`: every target of every package its
/// workspace has, and the same for each dependency given by `path`, however deep. `metadata` is
/// `cargo metadata --no-deps` for a manifest, or `None` where there is no such file (a path
/// dependency that is not checked out). A registry or git dependency has no `path` and is left
/// alone, as is a `[patch]` (the vendored crates): its dependents name it without one.
fn roots(manifest: &Path, metadata: &mut dyn FnMut(&Path) -> Result<Option<serde_json::Value>, String>) -> Result<Roots, String> {
    let mut roots = Roots::new();
    let mut taken = BTreeSet::new();
    let mut todo = vec![manifest.to_path_buf()];
    while let Some(manifest) = todo.pop() {
        if taken.contains(&manifest) {
            continue;
        }
        let Some(meta) = metadata(&manifest)? else { continue };
        let odd = |what: &str| format!("cargo metadata for {}: no {what}", manifest.display());
        for package in meta["packages"].as_array().ok_or_else(|| odd("packages"))? {
            let own = PathBuf::from(package["manifest_path"].as_str().ok_or_else(|| odd("manifest_path"))?);
            if !taken.insert(own) {
                continue;
            }
            for target in package["targets"].as_array().ok_or_else(|| odd("targets"))? {
                let edition = target["edition"].as_str().ok_or_else(|| odd("target edition"))?;
                let file = target["src_path"].as_str().ok_or_else(|| odd("target src_path"))?;
                roots.entry(edition.to_string()).or_default().insert(PathBuf::from(file));
            }
            for dependency in package["dependencies"].as_array().ok_or_else(|| odd("dependencies"))? {
                if let Some(dir) = dependency["path"].as_str() {
                    todo.push(Path::new(dir).join("Cargo.toml"));
                }
            }
        }
    }
    Ok(roots)
}

/// `files` in order, cut into runs whose arguments stay within `max` characters. A file longer
/// than `max` is a run of its own, so none is dropped.
fn batches(files: &[PathBuf], max: usize) -> Vec<&[PathBuf]> {
    let mut out = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, file) in files.iter().enumerate() {
        let cost = argument_chars(file);
        if i > start && used + cost > max {
            out.push(&files[start..i]);
            (start, used) = (i, 0);
        }
        used += cost;
    }
    if start < files.len() {
        out.push(&files[start..]);
    }
    out
}

/// What `file` takes of a Windows command line: its UTF-16 units, the quotes it gets when it
/// has a space, and the space after it.
fn argument_chars(file: &Path) -> usize {
    file.to_string_lossy().encode_utf16().count() + 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn package(dir: &str, targets: &[(&str, &str)], path_dependencies: &[&str]) -> serde_json::Value {
        let targets: Vec<_> = targets.iter().map(|(file, edition)| json!({ "src_path": format!("{dir}/{file}"), "edition": edition })).collect();
        let mut dependencies = vec![json!({ "name": "serde", "path": null })];
        dependencies.extend(path_dependencies.iter().map(|d| json!({ "name": "local", "path": d })));
        json!({ "manifest_path": format!("{dir}/Cargo.toml"), "targets": targets, "dependencies": dependencies })
    }

    /// A workspace `/ws` (`app`, `lib`) whose `app` uses `/core/a` by path; `/core` is a second
    /// workspace (`a`, `b`, `b` is not used from `/ws`) and `a` points back at `/ws/lib`.
    fn two_workspaces(manifest: &Path) -> Option<serde_json::Value> {
        let ws = json!({ "packages": [
            package("/ws/app", &[("src/main.rs", "2024"), ("build.rs", "2024")], &["/core/a", "/gone"]),
            package("/ws/lib", &[("src/lib.rs", "2024")], &[]),
        ] });
        let core = json!({ "packages": [
            package("/core/a", &[("src/lib.rs", "2021"), ("tests/one.rs", "2021")], &["/ws/lib"]),
            package("/core/b", &[("src/lib.rs", "2021")], &["/core/a"]),
        ] });
        match manifest.to_str()?.replace('\\', "/").as_str() {
            "/ws/Cargo.toml" | "/ws/app/Cargo.toml" | "/ws/lib/Cargo.toml" => Some(ws),
            "/core/Cargo.toml" | "/core/a/Cargo.toml" | "/core/b/Cargo.toml" => Some(core),
            _ => None,
        }
    }

    #[test]
    fn path_dependencies_are_followed_and_each_workspace_is_asked_once() {
        let mut asked = Vec::new();
        let roots = roots(Path::new("/ws/Cargo.toml"), &mut |m| {
            asked.push(m.to_string_lossy().replace('\\', "/"));
            Ok(two_workspaces(m))
        })
        .unwrap();
        let files = |edition: &str| roots[edition].iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(files("2024"), ["/ws/app/build.rs", "/ws/app/src/main.rs", "/ws/lib/src/lib.rs"]);
        // All of the dependency's workspace, as `cargo fmt --all` takes it.
        assert_eq!(files("2021"), ["/core/a/src/lib.rs", "/core/a/tests/one.rs", "/core/b/src/lib.rs"]);
        // The registry dependency is not asked about, the missing one is skipped, and the
        // dependencies that point back are packages already taken.
        assert_eq!(asked, ["/ws/Cargo.toml", "/gone/Cargo.toml", "/core/a/Cargo.toml"]);
    }

    #[test]
    fn metadata_without_its_fields_is_an_error_not_an_empty_check() {
        let err = roots(Path::new("/ws/Cargo.toml"), &mut |_| Ok(Some(json!({ "packages": [{ "manifest_path": "/ws/Cargo.toml" }] })))).unwrap_err();
        assert!(err.contains("no targets"), "{err}");
        assert!(roots(Path::new("/ws/Cargo.toml"), &mut |_| Err("cargo metadata: failed".into())).is_err());
    }

    /// The fault: this workspace's files as one command line are past what Windows starts.
    #[test]
    fn six_hundred_files_are_cut_into_runs_windows_can_start() {
        let files: Vec<PathBuf> = (0..600)
            .map(|i| PathBuf::from(format!(r"C:\Users\someone\solidcraft\solidcore\crates\solidcore\tests\issue_{i:04}_a_long_test_name.rs")))
            .collect();
        assert!(files.iter().map(|f| argument_chars(f)).sum::<usize>() > 32_767, "one run would fit; this no longer covers the fault");
        let runs = batches(&files, ARGS_MAX);
        assert!(runs.len() > 1);
        for run in &runs {
            assert!(run.iter().map(|f| argument_chars(f)).sum::<usize>() <= ARGS_MAX);
        }
        assert_eq!(runs.concat(), files, "every file once, in order");
    }

    #[test]
    fn batches_cut_at_the_limit_and_keep_a_file_longer_than_it() {
        let files: Vec<PathBuf> = ["aa", "bb", "a_long_one", "cc"].iter().map(PathBuf::from).collect();
        // Each short file costs 5 (2 + quotes and a space).
        assert_eq!(batches(&files, 10), [&files[..2], &files[2..3], &files[3..]]);
        assert_eq!(batches(&files, 1), [&files[..1], &files[1..2], &files[2..3], &files[3..]]);
        assert_eq!(batches(&files, 1000), [&files[..]]);
        assert!(batches(&[], 10).is_empty());
    }
}
