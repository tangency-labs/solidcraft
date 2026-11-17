//! `cargo xtask fast`: the local check before a commit and a push. It checks what the change
//! touches and leaves the rest to CI, which runs the full gate on every pull request (and the
//! heavy tests in the merge queue).
//!
//! 1. The packages to test are the ones the change itself touches: the files that differ from
//!    the base (`origin/main`, else `main`), committed, uncommitted or untracked, mapped to
//!    packages by `scripts/ci/affected.py --owners` (CI's own mapping, without the packages
//!    that depend on them). `solidcore/` is a workspace of its own: its files map to its crates
//!    here.
//! 2. Format (`cargo xtask fmt`, and solidcore/ when it changed).
//! 3. The quick repository checks of `cargo xtask ci`: assets, lists, skills, layers,
//!    ui-containers, mesh-decisions; shared when a selected package is a shared crate or a
//!    manifest or the lockfile changed, licences when one of those changed, icons when an icon
//!    file changed.
//! 4. `cargo clippy --workspace --all-targets --profile ci -- -D warnings`, as `cargo xtask ci`
//!    runs it (and reusing its build), so a change that breaks a dependent crate still fails.
//! 5. The selected packages' tests, with nextest's `ci` profile (no heavy tests).
//! 6. For changed solidcore/ crates: their clippy and tests and solidcore's wasm check
//!    (solidcore/AGENTS.md §4).
//!
//! Every step runs even when one fails: all failures are listed at the end.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const USAGE: &str = "usage: cargo xtask fast [--base REF] [-p PACKAGE]... [--no-tests] [--list] [-- TEST ARGS]";

/// The parsed command line.
#[derive(Debug, Default, PartialEq)]
struct Options {
    base: Option<String>,
    packages: Vec<String>,
    no_tests: bool,
    /// Print what would be checked and tested, and stop.
    list: bool,
    test_args: Vec<String>,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--base" => o.base = Some(it.next().ok_or(USAGE)?.to_string()),
            "-p" | "--package" => o.packages.push(it.next().ok_or(USAGE)?.to_string()),
            "--no-tests" => o.no_tests = true,
            "--list" => o.list = true,
            "--" => {
                o.test_args = it.map(|s| s.to_string()).collect();
                break;
            }
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    Ok(o)
}

/// What the change touches.
#[derive(Debug, Default, PartialEq)]
struct Selection {
    /// The change can affect every workspace package (a manifest, the lockfile, scripts/, ...).
    full: bool,
    /// Workspace packages to test.
    packages: BTreeSet<String>,
    /// solidcore/ crates to check and test.
    solidcore: BTreeSet<String>,
    /// A solidcore/ file outside its crates and docs changed (its manifest, lockfile, fixtures).
    solidcore_other: bool,
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let o = parse(args)?;
    let sc_root = root.join("solidcore");
    let paths = changed_paths(root, o.base.as_deref())?;
    let sc_dirs = if sc_root.join("Cargo.toml").is_file() { package_dirs(&metadata_of(solidcore_cargo(&sc_root))?)? } else { Vec::new() };
    let mut sel = if o.packages.is_empty() {
        let (sc_paths, rest): (Vec<String>, Vec<String>) = paths.iter().cloned().partition(|p| p.starts_with("solidcore/"));
        let (full, packages) = if rest.is_empty() { (false, BTreeSet::new()) } else { owners(root, &rest)? };
        let (solidcore, solidcore_other) = solidcore_owners(&sc_dirs, &sc_paths);
        Selection { full, packages, solidcore, solidcore_other }
    } else {
        explicit(&package_dirs(&crate::metadata()?)?, &sc_dirs, &o.packages)?
    };
    if sel.solidcore_other && sel.solidcore.is_empty() {
        // solidcore/AGENTS.md §4 tests solidcore itself for any kernel change.
        sel.solidcore.insert("solidcore".into());
    }
    report(&paths, &sel);
    if o.list {
        for p in &paths {
            println!("  {p}");
        }
        return Ok(());
    }

    let mut failed: Vec<String> = Vec::new();
    let mut step = |name: &str, result: Result<(), String>| {
        if let Err(e) = result {
            eprintln!("fast: {name} failed: {e}");
            failed.push(name.to_string());
        }
    };
    let changed = |prefix: &str| paths.iter().any(|p| p.starts_with(prefix));

    step("fmt", crate::rustfmt::run(root, false));
    if !sel.solidcore.is_empty() {
        step("solidcore fmt", crate::rustfmt::run(&sc_root, false));
    }
    step("assets", crate::assets::run(root));
    step("lists", crate::lists::run(root, false));
    step("skills", crate::skills::run(root));
    step("layers", crate::cmd_layers());
    step("ui-containers", crate::ui_containers::run(root));
    step("mesh-decisions", crate::mesh_decisions::run(root));
    let manifests = paths.iter().any(|p| p == "Cargo.lock" || p == "Cargo.toml" || (p.ends_with("/Cargo.toml") && !p.starts_with("solidcore/")));
    if manifests || sel.packages.iter().any(|p| matches!(crate::layers::classify(p), Some(crate::layers::Class::Shared))) {
        step("shared", crate::shared::run(root));
    }
    if manifests {
        step("licences --check", crate::licences::run(root, true));
    }
    if changed("crates/look/icons/") || changed("tests/tools/") {
        step("icons", crate::icons::run(root));
    }

    let code_changed = sel.full || !sel.packages.is_empty() || !sel.solidcore.is_empty();
    if code_changed {
        let mut c = crate::cargo();
        c.args(["clippy", "--workspace", "--all-targets", "--profile", "ci", "--", "-D", "warnings"]);
        step("clippy", crate::run(c, "cargo clippy --workspace --all-targets --profile ci -- -D warnings"));
    } else {
        println!("fast: no code changed: no clippy or tests");
    }

    let nextest = has_nextest(crate::cargo());
    if o.no_tests {
        println!("fast: --no-tests: tests skipped");
    } else if !sel.packages.is_empty() {
        step(
            "tests",
            run_tests(crate::cargo(), "", nextest, &["--cargo-profile", "ci", "--profile", "ci"], &["--profile", "ci"], &sel.packages, &o.test_args),
        );
    }
    if sel.full && !o.no_tests {
        println!("fast: the change can also affect packages whose files did not change: CI tests them all; name some with -p");
    }

    if !sel.solidcore.is_empty() {
        let mut c = solidcore_cargo(&sc_root);
        c.args(["clippy", "--all-targets"]);
        add_packages(&mut c, &sel.solidcore);
        c.args(["--", "-D", "warnings"]);
        step("solidcore clippy", crate::run(c, &format!("(solidcore) cargo clippy {} --all-targets -- -D warnings", flags(&sel.solidcore))));
        if !o.no_tests {
            // CI's profile for solidcore's tests: optimised, with debug assertions.
            let nextest = has_nextest(solidcore_cargo(&sc_root));
            step(
                "solidcore tests",
                run_tests(
                    solidcore_cargo(&sc_root),
                    "(solidcore) ",
                    nextest,
                    &["--cargo-profile", "goldens"],
                    &["--profile", "goldens"],
                    &sel.solidcore,
                    &o.test_args,
                ),
            );
        }
        let mut c = solidcore_cargo(&sc_root);
        c.args(["check", "-p", "solidcore", "--target", "wasm32-unknown-unknown"]);
        step("solidcore wasm", crate::run(c, "(solidcore) cargo check -p solidcore --target wasm32-unknown-unknown"));
        println!(
            "fast: for a kernel change, also run the goldens of each unit you touched (solidcore/AGENTS.md §4):\n  \
             cd solidcore && cargo run --profile goldens -p goldens -- --unit <id> --goldens"
        );
    }

    if failed.is_empty() {
        println!("fast: all checks passed. CI runs the full gate; the merge queue the heavy tests.");
        Ok(())
    } else {
        Err(format!("{} failed: {}. Check whether each fails on main too before blaming the change.", failed.len(), failed.join(", ")))
    }
}

fn report(paths: &[String], sel: &Selection) {
    let list = |s: &BTreeSet<String>| if s.is_empty() { "none".to_string() } else { s.iter().cloned().collect::<Vec<_>>().join(" ") };
    println!("fast: {} changed file(s)", paths.len());
    if sel.full {
        println!("fast: workspace: the change can affect every package (a manifest, the lockfile, scripts/, ...)");
    }
    println!("fast: workspace packages to test: {}", list(&sel.packages));
    if !sel.solidcore.is_empty() {
        println!("fast: solidcore crates to check and test: {}", list(&sel.solidcore));
    }
}

/// The changed files, relative to the repository root: the commits since the merge base with
/// `base`, uncommitted changes and untracked files.
fn changed_paths(root: &Path, base: Option<&str>) -> Result<Vec<String>, String> {
    let git = |args: &[&str]| -> Result<Vec<u8>, String> {
        let out = Command::new("git").args(args).current_dir(root).output().map_err(|e| format!("git: {e}"))?;
        if out.status.success() { Ok(out.stdout) } else { Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())) }
    };
    let base = match base {
        Some(b) => b.to_string(),
        None => ["origin/main", "main"]
            .into_iter()
            .find(|b| git(&["rev-parse", "--verify", "-q", &format!("{b}^{{commit}}")]).is_ok())
            .ok_or("no origin/main or main to compare with: pass --base REF")?
            .to_string(),
    };
    let mut out = git(&["diff", "-z", "--no-renames", "--name-only", &format!("{base}...HEAD")])?;
    out.extend(git(&["diff", "-z", "--no-renames", "--name-only", "HEAD"])?);
    out.extend(git(&["ls-files", "-z", "--others", "--exclude-standard"])?);
    Ok(split_nul(&out))
}

fn split_nul(bytes: &[u8]) -> Vec<String> {
    let set: BTreeSet<String> = bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect();
    set.into_iter().collect()
}

/// The workspace packages the paths change, by CI's own mapping (`scripts/ci/affected.py`).
fn owners(root: &Path, paths: &[String]) -> Result<(bool, BTreeSet<String>), String> {
    let python = crate::icons::python()?;
    let (program, prefix) = python.split_first().ok_or("no Python command")?;
    let mut c = Command::new(program);
    c.args(prefix).args(["-I", "scripts/ci/affected.py", "--paths", "-", "--owners"]).current_dir(root);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("affected.py: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(paths.join("\0").as_bytes()).map_err(|e| format!("affected.py: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("affected.py: {e}"))?;
    if !out.status.success() {
        return Err(format!("scripts/ci/affected.py failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    parse_owners(&String::from_utf8_lossy(&out.stdout))
}

fn parse_owners(out: &str) -> Result<(bool, BTreeSet<String>), String> {
    let field = |key: &str| out.lines().find_map(|l| l.strip_prefix(key)).ok_or_else(|| format!("affected.py printed no `{key}`"));
    let full = field("full=")? == "true";
    Ok((full, field("packages=")?.split_whitespace().map(str::to_string).collect()))
}

/// solidcore/ files that no build, test or goldens run reads (solidcore/scripts/ci/changes.sh's
/// DOCS_ONLY), relative to solidcore/.
fn solidcore_docs_only(path: &str) -> bool {
    path.starts_with("plan/findings/")
        || path.starts_with("plan/upstream-issues/")
        || path.strip_prefix("plan/").is_some_and(|p| !p.contains('/') && p.ends_with(".md"))
        || ["site/", "docs/", "assets/"].iter().any(|d| path.starts_with(d))
        || (!path.contains('/') && path.ends_with(".md"))
}

/// The solidcore crates the `solidcore/...` paths change, and whether another code file
/// (a manifest, the lockfile, fixtures, harness) changed.
fn solidcore_owners(dirs: &[(String, String)], paths: &[String]) -> (BTreeSet<String>, bool) {
    let mut crates = BTreeSet::new();
    let mut other = false;
    for path in paths.iter().filter_map(|p| p.strip_prefix("solidcore/")) {
        if let Some((_, name)) = dirs.iter().filter(|(d, _)| !d.is_empty() && path.starts_with(d.as_str())).max_by_key(|(d, _)| d.len()) {
            crates.insert(name.clone());
        } else if !solidcore_docs_only(path) {
            other = true;
        }
    }
    (crates, other)
}

/// The packages named with -p, each found in the workspace or in solidcore/.
fn explicit(ws: &[(String, String)], sc: &[(String, String)], names: &[String]) -> Result<Selection, String> {
    let mut sel = Selection::default();
    for n in names {
        if ws.iter().any(|(_, w)| w == n) {
            sel.packages.insert(n.clone());
        } else if sc.iter().any(|(_, s)| s == n) {
            sel.solidcore.insert(n.clone());
        } else {
            return Err(format!("no package `{n}` in the workspace or in solidcore/"));
        }
    }
    Ok(sel)
}

/// (directory relative to the workspace root with a trailing `/`, package name) of each package.
fn package_dirs(meta: &serde_json::Value) -> Result<Vec<(String, String)>, String> {
    let root = meta["workspace_root"].as_str().ok_or("cargo metadata: no workspace_root")?.replace('\\', "/");
    let root = format!("{}/", root.trim_end_matches('/'));
    let mut dirs = Vec::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let (Some(name), Some(manifest)) = (p["name"].as_str(), p["manifest_path"].as_str()) else { continue };
        let manifest = manifest.replace('\\', "/");
        let Some(rel) = manifest.strip_prefix(&root) else { continue };
        let dir = rel.rsplit_once('/').map_or(String::new(), |(d, _)| format!("{d}/"));
        dirs.push((dir, name.to_string()));
    }
    Ok(dirs)
}

fn metadata_of(mut c: Command) -> Result<serde_json::Value, String> {
    c.args(["metadata", "--format-version", "1", "--no-deps"]);
    let out = c.output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

/// Cargo in solidcore/, with solidcore/rust-toolchain.toml choosing the toolchain: the toolchain
/// rustup chose for `cargo xtask` would otherwise win.
fn solidcore_cargo(dir: &Path) -> Command {
    let mut c = Command::new("cargo");
    c.current_dir(dir);
    for var in ["RUSTUP_TOOLCHAIN", "CARGO", "RUSTC", "CARGO_MANIFEST_DIR"] {
        c.env_remove(var);
    }
    c
}

fn has_nextest(mut c: Command) -> bool {
    c.args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null());
    c.status().is_ok_and(|s| s.success())
}

fn add_packages(c: &mut Command, packages: &BTreeSet<String>) {
    for p in packages {
        c.args(["-p", p]);
    }
}

fn flags(packages: &BTreeSet<String>) -> String {
    packages.iter().map(|p| format!("-p {p}")).collect::<Vec<_>>().join(" ")
}

/// The packages' tests (each command line printed after `label`) with nextest (its options `nextest_args`) or, without it, `cargo test`
/// (its options `test_args`; nextest's filter of heavy tests then does not apply).
fn run_tests(
    mut c: Command,
    label: &str,
    nextest: bool,
    nextest_args: &[&str],
    test_args: &[&str],
    packages: &BTreeSet<String>,
    extra: &[String],
) -> Result<(), String> {
    let what = if nextest {
        c.args(["nextest", "run", "--no-tests=pass"]).args(nextest_args);
        format!("{label}cargo nextest run {} {}", nextest_args.join(" "), flags(packages))
    } else {
        eprintln!("fast: cargo-nextest is not installed: cargo test runs the heavy tests too (https://nexte.st)");
        c.arg("test").args(test_args);
        format!("{label}cargo test {} {}", test_args.join(" "), flags(packages))
    };
    add_packages(&mut c, packages);
    if !extra.is_empty() {
        c.arg("--").args(extra);
    }
    crate::run(c, &what)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_packages_base_and_test_arguments() {
        let o = parse(&["--base", "upstream/main", "-p", "a", "--package", "b", "--no-tests", "--list", "--", "name", "--exact"]).unwrap();
        assert_eq!(o.base.as_deref(), Some("upstream/main"));
        assert_eq!(o.packages, vec!["a", "b"]);
        assert!(o.no_tests && o.list);
        assert_eq!(o.test_args, vec!["name", "--exact"]);
        assert!(parse(&["--full"]).is_err());
        assert!(parse(&["-p"]).is_err());
    }

    #[test]
    fn reads_what_affected_py_prints() {
        assert_eq!(parse_owners("full=false\npackages=a b\n").unwrap(), (false, set(&["a", "b"])));
        assert_eq!(parse_owners("full=true\npackages=\n").unwrap(), (true, set(&[])));
        assert!(parse_owners("nonsense").is_err());
    }

    #[test]
    fn solidcore_paths_map_to_their_crate_and_docs_to_nothing() {
        let dirs =
            vec![("crates/solidcore/".to_string(), "solidcore".to_string()), ("crates/solidcore-ops/".to_string(), "solidcore-ops".to_string())];
        let paths = |ps: &[&str]| ps.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        let (crates, other) = solidcore_owners(
            &dirs,
            &paths(&["solidcore/crates/solidcore-ops/src/lib.rs", "solidcore/docs/x.md", "solidcore/plan/findings/y.md", "solidcore/README.md"]),
        );
        assert_eq!(crates, set(&["solidcore-ops"]));
        assert!(!other);
        let (crates, other) = solidcore_owners(&dirs, &paths(&["solidcore/Cargo.lock", "solidcore/plan/units/z.toml"]));
        assert!(crates.is_empty());
        assert!(other);
    }

    #[test]
    fn named_packages_are_found_in_either_workspace() {
        let ws = vec![("crates/engine/".to_string(), "solidcraft-engine".to_string())];
        let sc = vec![("crates/solidcore/".to_string(), "solidcore".to_string())];
        let sel = explicit(&ws, &sc, &["solidcraft-engine".into(), "solidcore".into()]).unwrap();
        assert_eq!((sel.packages, sel.solidcore), (set(&["solidcraft-engine"]), set(&["solidcore"])));
        assert!(explicit(&ws, &sc, &["nope".into()]).is_err());
    }

    #[test]
    fn package_dirs_are_relative_to_the_workspace_root() {
        let meta = serde_json::json!({
            "workspace_root": "C:\\ws",
            "packages": [{"name": "a", "manifest_path": "C:\\ws\\crates\\a\\Cargo.toml"}, {"name": "root", "manifest_path": "C:\\ws\\Cargo.toml"}]
        });
        assert_eq!(package_dirs(&meta).unwrap(), vec![("crates/a/".to_string(), "a".to_string()), (String::new(), "root".to_string())]);
    }

    #[test]
    fn nul_separated_paths_are_split_and_deduplicated() {
        assert_eq!(split_nul(b"b\0a\0\0b\0"), vec!["a", "b"]);
    }
}
