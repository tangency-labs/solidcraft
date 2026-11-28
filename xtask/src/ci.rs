//! `cargo xtask ci`: the check before a push. It always runs the repository checks, clippy, the
//! tests and the wasm check, and adds the checks CI runs only for some paths when the branch
//! changes those paths: the files that differ from the merge-base with the base (`origin/main`,
//! else `main`), committed, uncommitted or untracked. `--all` runs every check whatever changed.
//!
//! [`PATH_GATES`] is the one place that says which paths need which check.

use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask ci [--all] [--base REF]";

/// A check that runs only when the branch changes one of `globs`.
pub struct Gate {
    /// The check: `cargo xtask <check>`, or a script path from the repository root.
    pub check: &'static str,
    /// Paths from the repository root; `**` matches any folders, `*` stays within one.
    pub globs: &'static [&'static str],
    /// What those paths are, for the report.
    pub what: &'static str,
}

/// Path-selected checks. This mirrors CI's path filters: keep it in step with them.
/// - `i18n --check`: ci.yml's `repo-checks` job runs the catalog check, and its `tests` job the
///   command-label check when solidcraft-cli, solidcraft-ui-egui or xtask changes.
/// - `kernel-cases --coverage`: kernel-rig.yml's build job, when `COVERAGE` in
///   scripts/ci/kernel_rig_plan.py matches (the command registry or the cases).
/// - `plugin-examples`: ci.yml's `wasm` job, for the plugin guide and its examples.
/// - `scripts/test-format-pending.sh`: ci.yml's `repo-checks` job (and lists.yml).
pub const PATH_GATES: &[Gate] = &[
    Gate {
        check: "i18n --check",
        globs: &["assets/locales/**", "crates/ui-egui/**", "crates/engine/src/cmd/**", "apps/solidcraft-cli/**", "xtask/src/i18n.rs"],
        what: "locale files or UI strings",
    },
    Gate {
        check: "kernel-cases --coverage",
        globs: &["crates/engine/src/cmd/**", "tests/kernel-cases/**", "xtask/src/kernel_cases.rs"],
        what: "command registrations or kernel cases",
    },
    Gate {
        check: "plugin-examples",
        globs: &["docs/book/plugin-examples/**", "docs/book/src/dev/plugins.md", "crates/craft-plugin/wit/**", "xtask/src/plugin_examples.rs"],
        what: "the plugin guide or its examples",
    },
    Gate {
        check: "scripts/test-format-pending.sh",
        globs: &[
            "crates/doc/src/format/**",
            "crates/doc/src/drawing/format/**",
            "crates/doc/build.rs",
            "scripts/fold-format-pending.sh",
            "scripts/test-format-pending.sh",
        ],
        what: "design-format change files",
    },
];

/// The checks every run makes, in order, for the report.
const ALWAYS: &str = "fmt --check, assets, icons, licences --check, lists, skills, layers, shared, ui-containers, \
mesh-decisions, app-geometry, clippy -D warnings, tests with the wasm check, web-offline-test (when Chrome is installed)";

#[derive(Debug, Default, PartialEq)]
struct Options {
    all: bool,
    base: Option<String>,
}

fn parse(args: &[&str]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--all" => o.all = true,
            "--base" => o.base = Some(it.next().ok_or(USAGE)?.to_string()),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    Ok(o)
}

/// Whether `path` matches `glob` (see [`Gate::globs`]).
fn glob_match(glob: &str, path: &str) -> bool {
    if let Some(rest) = glob.strip_prefix("**/") {
        return glob_match(rest, path) || path.split_once('/').is_some_and(|(_, tail)| glob_match(glob, tail));
    }
    if glob == "**" {
        return true;
    }
    match glob.find('*') {
        None => glob == path,
        Some(i) => {
            let (head, tail) = (&glob[..i], &glob[i..]);
            let Some(path) = path.strip_prefix(head) else { return false };
            if tail.starts_with("**") {
                return glob_match(tail, path);
            }
            let tail = &tail[1..];
            // `*`: any run of characters within one folder.
            let seg_end = path.find('/').unwrap_or(path.len());
            (0..=seg_end).any(|n| glob_match(tail, &path[n..]))
        }
    }
}

/// The first changed path a gate's globs match.
fn first_match<'a>(gate: &Gate, paths: &'a [String]) -> Option<&'a str> {
    paths.iter().find(|p| gate.globs.iter().any(|g| glob_match(g, p))).map(String::as_str)
}

/// Which path gates run, each with the reason printed for it.
fn plan(paths: Option<&[String]>, all: bool) -> Vec<(&'static Gate, bool, String)> {
    PATH_GATES
        .iter()
        .map(|g| match (all, paths) {
            (true, _) => (g, true, "--all".to_string()),
            (false, None) => (g, true, "the changed files are unknown".to_string()),
            (false, Some(paths)) => match first_match(g, paths) {
                Some(p) => (g, true, format!("{} changed ({p})", g.what)),
                None => (g, false, format!("no change to {}", g.what)),
            },
        })
        .collect()
}

/// A built solidcraft-cli in the target directory: `profile`'s, when there is one.
fn built_cli(profile: &str) -> Option<PathBuf> {
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| crate::root().join("target"));
    let p = target.join(profile).join(format!("solidcraft-cli{}", std::env::consts::EXE_SUFFIX));
    p.is_file().then_some(p)
}

/// The ci-profile CLI the tests just built from this checkout, else a release one (possibly older).
fn cli() -> Option<String> {
    built_cli("ci").or_else(|| built_cli("release")).map(|p| p.to_string_lossy().into_owned())
}

fn i18n(root: &Path) -> Result<(), String> {
    match cli() {
        Some(c) => crate::i18n::run(root, &["--check", "--cli", &c]),
        None => crate::i18n::run(root, &["--check", "--no-registry"]),
    }
}

fn kernel_coverage(_: &Path) -> Result<(), String> {
    match cli() {
        Some(c) => crate::kernel_cases::run(&["--coverage", "--cli", &c]),
        None => crate::kernel_cases::run(&["--coverage"]),
    }
}

fn plugin_examples(root: &Path) -> Result<(), String> {
    crate::plugin_examples::run(root, false)
}

fn format_pending(root: &Path) -> Result<(), String> {
    let mut c = Command::new("bash");
    c.arg("scripts/test-format-pending.sh").current_dir(root);
    crate::run(c, "bash scripts/test-format-pending.sh")
}

/// A path gate's check, run from the repository root.
type Runner = fn(&Path) -> Result<(), String>;

/// The runner of a path gate's check; `None` when no such check exists.
fn runner(check: &str) -> Option<Runner> {
    match check {
        "i18n --check" => Some(i18n),
        "kernel-cases --coverage" => Some(kernel_coverage),
        "plugin-examples" => Some(plugin_examples),
        "scripts/test-format-pending.sh" => Some(format_pending),
        _ => None,
    }
}

/// The quick gates first, so they fail fast; the wasm check (and doc tests) run while the test
/// binaries do, which leaves cargo's build lock free. The path gates that read the command
/// registry run after the tests, which build the CLI.
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let o = parse(args)?;
    let paths = if o.all {
        None
    } else {
        match crate::fast::changed_paths(root, o.base.as_deref()) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("ci: cannot list the changed files ({e}); running every check");
                None
            }
        }
    };
    let plan = plan(paths.as_deref(), o.all);
    eprintln!("ci: always: {ALWAYS}");
    for (g, on, why) in &plan {
        eprintln!("ci: {} {}: {why}", if *on { "run " } else { "skip" }, g.check);
    }
    let on = |name: &str| plan.iter().any(|(g, run, _)| *run && g.check == name);
    let gate = |name: &str| -> Result<(), String> {
        if on(name) {
            eprintln!("ci: {name}");
            runner(name).map_or_else(|| Err(format!("no check named `{name}`")), |f| f(root))
        } else {
            Ok(())
        }
    };

    crate::rustfmt::run(root, true)?;
    crate::assets::run(root)?;
    crate::icons::run(root)?;
    crate::licences::run(root, true)?;
    crate::lists::run(root, false)?;
    crate::skills::run(root)?;
    gate("scripts/test-format-pending.sh")?;
    crate::cmd_layers()?;
    crate::shared::run(root)?;
    crate::ui_containers::run(root)?;
    crate::mesh_decisions::run(root)?;
    // The same base as the path-picked checks (#22688): a fork clone's `origin/main` may be weeks old.
    let base: Vec<&str> = o.base.as_deref().map(|b| vec!["--base", b]).unwrap_or_default();
    crate::app_geometry::run(root, &base)?;
    let mut c = crate::cargo();
    c.args(["clippy", "--workspace", "--all-targets", "--profile", "ci", "--", "-D", "warnings"]);
    crate::run(c, "cargo clippy -D warnings")?;
    let (wasm, n) = crate::wasm_check()?;
    crate::test_nextest::run_selected(vec![(format!("wasm32-unknown-unknown check ({n} crates)"), wasm), crate::test::doc_tests(&[], &[])])?;
    gate("i18n --check")?;
    gate("kernel-cases --coverage")?;
    gate("plugin-examples")?;
    crate::web_offline_test::run(root, None, false)?;
    let skipped: Vec<&str> = plan.iter().filter(|(_, run, _)| !run).map(|(g, _, _)| g.check).collect();
    if skipped.is_empty() {
        eprintln!("ci: all gates passed");
    } else {
        eprintln!("ci: all gates passed; skipped as this change does not need them: {} (--all runs them)", skipped.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table names only checks that exist: each has a runner, and is an xtask command in the
    /// usage text or a script in the repository. Each glob also starts at a real path.
    #[test]
    fn path_gates_name_only_checks_that_exist() {
        let root = crate::root();
        let commands = crate::skills::xtask_commands(crate::USAGE);
        for g in PATH_GATES {
            assert!(runner(g.check).is_some(), "{}: no runner for it in ci.rs", g.check);
            let first = g.check.split_whitespace().next().unwrap_or_default();
            assert!(commands.iter().any(|c| c == first) || root.join(first).is_file(), "{}: no such xtask command or script", g.check);
            for glob in g.globs {
                let fixed = glob.split('*').next().unwrap_or_default().trim_end_matches('/');
                assert!(root.join(fixed).exists(), "{}: {glob} names no path in the repository", g.check);
            }
        }
        assert!(runner("no-such-check").is_none());
    }

    #[test]
    fn globs_match_folders_and_files() {
        assert!(glob_match("assets/locales/**", "assets/locales/de.json"));
        assert!(glob_match("assets/locales/**", "assets/locales/pending/x.json"));
        assert!(!glob_match("assets/locales/**", "assets/icons/a.svg"));
        assert!(glob_match("tests/kernel-cases/**/recipe.json", "tests/kernel-cases/a/b/recipe.json"));
        assert!(glob_match("tests/kernel-cases/**/recipe.json", "tests/kernel-cases/recipe.json"));
        assert!(glob_match("crates/*/Cargo.toml", "crates/doc/Cargo.toml"));
        assert!(!glob_match("crates/*/Cargo.toml", "crates/doc/x/Cargo.toml"));
        assert!(glob_match("xtask/src/i18n.rs", "xtask/src/i18n.rs"));
        assert!(!glob_match("xtask/src/i18n.rs", "xtask/src/i18n.rs.bak"));
    }

    #[test]
    fn the_plan_runs_a_gate_only_for_its_paths() {
        let gates = |paths: &[&str]| -> Vec<&str> {
            let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
            plan(Some(&paths), false).into_iter().filter(|(_, on, _)| *on).map(|(g, _, _)| g.check).collect()
        };
        assert!(gates(&["docs/building.md"]).is_empty());
        assert_eq!(gates(&["assets/locales/en.json"]), ["i18n --check"]);
        assert_eq!(gates(&["crates/engine/src/cmd/solid.rs"]), ["i18n --check", "kernel-cases --coverage"]);
        assert_eq!(gates(&["tests/kernel-cases/EXEMPT"]), ["kernel-cases --coverage"]);
        assert_eq!(plan(None, false).iter().filter(|(_, on, _)| *on).count(), PATH_GATES.len());
        assert!(plan(Some(&[]), true).iter().all(|(_, on, _)| *on));
    }

    #[test]
    fn parse_reads_all_and_base() {
        assert_eq!(parse(&[]).unwrap(), Options::default());
        assert_eq!(parse(&["--all", "--base", "main"]).unwrap(), Options { all: true, base: Some("main".into()) });
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["--base"]).is_err());
    }
}
