//! `cargo xtask test`: build the workspace's tests once, then run every test binary at the same
//! time. `cargo test` runs them one after another, so one slow binary held up all the others;
//! the doc tests and any other `side` jobs run alongside on their own thread. Like pushes to
//! `main`, it skips the property tests that CI runs only nightly ([`NIGHTLY_ONLY`]) unless asked.

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use crate::{cargo, root};

/// Test binaries (package folder, test target) that only the nightly "Property tests" CI job runs:
/// their inputs are random on every run, so a known kernel bug would fail them at random. Pull
/// requests and pushes to `main` skip them (`binary(boolean_props)` in .config/nextest.toml and
/// .github/workflows/ci.yml); `cargo xtask test --property-tests` runs them.
const NIGHTLY_ONLY: &[(&str, &str)] = &[("kernel", "boolean_props")];

/// A test executable from `cargo test --no-run`.
struct TestBin {
    label: String,
    /// The package's folder name (`kernel`) and the test target's name (`boolean_props`).
    pkg: String,
    name: String,
    exe: String,
    dir: String,
}

/// Build the tests with `cargo_args` (e.g. `--locked`), run each binary with `test_args`, and run
/// the `side` jobs one after another meanwhile. Each prints a line when it ends; a failure prints
/// its whole output. The [`NIGHTLY_ONLY`] binaries run only with `property_tests`.
pub fn run(cargo_args: &[&str], test_args: &[&str], property_tests: bool, side: Vec<(String, Command)>) -> Result<(), String> {
    let (bins, skipped): (Vec<TestBin>, Vec<TestBin>) =
        build(cargo_args)?.into_iter().partition(|b| property_tests || !nightly_only(&b.pkg, &b.name));
    for b in &skipped {
        println!("skip  {:<44}          nightly property tests in CI; --property-tests runs them", b.label);
    }
    let start = Instant::now();
    let (tx, rx) = mpsc::channel::<(String, Result<String, String>, f64)>();
    let mut failed = Vec::new();
    std::thread::scope(|scope| {
        for b in &bins {
            let tx = tx.clone();
            scope.spawn(move || {
                let t0 = Instant::now();
                let mut c = Command::new(&b.exe);
                c.args(test_args).current_dir(&b.dir).env("CARGO_MANIFEST_DIR", &b.dir);
                let _ = tx.send((b.label.clone(), finish(c), t0.elapsed().as_secs_f64()));
            });
        }
        let tx_side = tx.clone();
        scope.spawn(move || {
            for (label, c) in side {
                let t0 = Instant::now();
                let _ = tx_side.send((label, finish(c), t0.elapsed().as_secs_f64()));
            }
        });
        drop(tx);
        for (label, result, secs) in rx {
            match result {
                Ok(summary) => println!("ok    {label:<44} {secs:6.1}s  {summary}"),
                Err(output) => {
                    println!("FAIL  {label:<44} {secs:6.1}s\n{output}");
                    failed.push(label);
                }
            }
        }
    });
    println!("test: {} binaries in {:.1}s", bins.len(), start.elapsed().as_secs_f64());
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

/// Whether CI runs this test target only in the nightly property tests.
fn nightly_only(pkg: &str, name: &str) -> bool {
    NIGHTLY_ONLY.iter().any(|(p, n)| *p == pkg && *n == name)
}

/// Run to the end: the last `test result:` line on success, all output on failure.
fn finish(mut c: Command) -> Result<String, String> {
    let out = c.stdin(Stdio::null()).output().map_err(|e| format!("failed to spawn: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if out.status.success() {
        Ok(stdout.lines().rev().find(|l| l.starts_with("test result:")).unwrap_or("").trim_start_matches("test result: ").to_string())
    } else {
        Err(format!("{stdout}{}\n({})", String::from_utf8_lossy(&out.stderr), out.status))
    }
}

/// `cargo test --workspace --profile ci --no-run`, and the executables it built.
fn build(cargo_args: &[&str]) -> Result<Vec<TestBin>, String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--profile", "ci", "--no-run", "--message-format=json-render-diagnostics"]).args(cargo_args);
    eprintln!("$ cargo test --workspace --profile ci --no-run");
    let out = c.stderr(Stdio::inherit()).output().map_err(|e| format!("cargo test --no-run: failed to spawn: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo test --no-run: exited with {}", out.status));
    }
    let mut bins: Vec<TestBin> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(m) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(exe) = m["executable"].as_str().filter(|_| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true) else {
            continue;
        };
        if bins.iter().any(|b| b.exe == exe) {
            continue;
        }
        let manifest = std::path::Path::new(m["manifest_path"].as_str().unwrap_or_default()).to_path_buf();
        let dir = manifest.parent().map(|d| d.to_path_buf()).unwrap_or_else(root);
        let pkg = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = m["target"]["kind"][0].as_str().unwrap_or("");
        let name = m["target"]["name"].as_str().unwrap_or("").to_string();
        let label = format!("{pkg} {kind} {name}");
        bins.push(TestBin { label, pkg, name, exe: exe.to_string(), dir: dir.to_string_lossy().into_owned() });
    }
    if bins.is_empty() {
        return Err("cargo test --no-run built no test executables".into());
    }
    Ok(bins)
}

/// The doc tests (`cargo test --doc`), to run as a side job.
pub fn doc_tests(cargo_args: &[&str], test_args: &[&str]) -> (String, Command) {
    let mut c = cargo();
    c.args(["test", "--workspace", "--profile", "ci", "--doc"]).args(cargo_args).arg("--").args(test_args);
    ("doc tests".into(), c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_nightly_binaries_are_skipped() {
        assert!(nightly_only("kernel", "boolean_props"));
        assert!(!nightly_only("kernel", "kernel"));
        assert!(!nightly_only("engine", "boolean_props"));
        assert!(!nightly_only("engine", "atomic_commands"));
    }

    /// CI's filters name the same binaries, so `cargo xtask test` keeps checking what a push to
    /// `main` checks.
    #[test]
    fn ci_skips_the_same_binaries() {
        let read = |p: &str| std::fs::read_to_string(root().join(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
        let nextest = read(".config/nextest.toml");
        let ci = read(".github/workflows/ci.yml");
        for (pkg, name) in NIGHTLY_ONLY {
            let filter = format!("package(solidcraft-{pkg}) & binary({name})");
            assert!(nextest.contains(&filter), ".config/nextest.toml does not skip {filter}");
            assert!(ci.contains(&format!("not ({filter})")), "ci.yml does not skip {filter} on main");
        }
    }
}
