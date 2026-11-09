//! Optional test-level scheduling for full local CI. Reuse the built binaries so the Wasm and
//! doc-test jobs can use Cargo alongside the native run, without rebuilding native tests.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use crate::cargo;

/// Select the full-CI runner without changing `xtask test`'s argument forwarding.
pub fn run_selected(side: Vec<(String, Command)>) -> Result<(), String> {
    match std::env::var("SOLIDCRAFT_TEST_RUNNER") {
        Err(std::env::VarError::NotPresent) => crate::test::run(&[], &[], side),
        Ok(value) if value == "libtest" => crate::test::run(&[], &[], side),
        Ok(value) if value == "nextest" => run(side),
        _ => Err("SOLIDCRAFT_TEST_RUNNER must be libtest or nextest".into()),
    }
}

pub struct Prepared {
    pub cargo_metadata: PathBuf,
    pub binaries_metadata: PathBuf,
}

impl Prepared {
    pub fn command(&self) -> Command {
        self.command_with_options(
            std::env::var_os("NEXTEST_TEST_THREADS"),
            std::env::var_os("ACTION_MATRIX_JOBS"),
            std::thread::available_parallelism().map_or(1, |n| n.get()),
        )
    }

    fn command_with_options(&self, threads: Option<OsString>, matrix_jobs: Option<OsString>, cpus: usize) -> Command {
        let mut c = cargo();
        c.args(["nextest", "run", "--cargo-metadata"]).arg(&self.cargo_metadata).arg("--binaries-metadata").arg(&self.binaries_metadata).args([
            "--profile",
            "local-ci",
            "--ignore-default-filter",
            "--no-fail-fast",
            "--retries",
            "0",
            "--no-tests",
            "fail",
            "--test-threads",
        ]);
        // One geometry test can also use Rayon. Bound the default; callers can explicitly
        // choose another nextest concurrency without changing libtest's per-binary setting.
        let threads = threads.unwrap_or_else(|| cpus.clamp(1, 4).to_string().into());
        // The matrix has its own child-process pool. A nextest slot reservation alone
        // does not bound it: without this it still spawns one worker per host CPU.
        // Leave invalid thread values to nextest's own diagnostic before tests start.
        let slots = match threads.to_str() {
            Some("num-cpus") => cpus,
            Some(value) => value.parse::<isize>().ok().map(|n| if n < 0 { cpus.saturating_sub(n.unsigned_abs()) } else { n as usize }).unwrap_or(1),
            None => 1,
        }
        .clamp(1, cpus.max(1));
        c.arg(threads).env("ACTION_MATRIX_JOBS", matrix_jobs.unwrap_or_else(|| slots.to_string().into()));
        c
    }
}

pub fn run(side: Vec<(String, Command)>) -> Result<(), String> {
    let prepared = prepare()?;
    execute(("nextest full workspace".into(), prepared.command()), side)
}

pub fn prepare() -> Result<Prepared, String> {
    let mut version = cargo();
    version.args(["nextest", "--version"]);
    capture(version, "cargo nextest --version")
        .map_err(|e| format!("{e}; install cargo-nextest first (https://nexte.st/docs/installation/pre-built-binaries/)"))?;
    let mut metadata = cargo();
    metadata.args(["metadata", "--format-version", "1", "--locked"]);
    let cargo_json = capture(metadata, "cargo metadata")?;
    let parsed: serde_json::Value = serde_json::from_slice(&cargo_json).map_err(|e| format!("Cargo metadata: {e}"))?;
    let target = parsed.get("target_directory").and_then(|v| v.as_str()).ok_or("Cargo metadata has no target_directory")?;
    let dir = PathBuf::from(target).join("xtask-nextest").join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let cargo_metadata = dir.join("cargo-metadata.json");
    std::fs::write(&cargo_metadata, cargo_json).map_err(|e| format!("write {}: {e}", cargo_metadata.display()))?;
    let mut build = cargo();
    build.args(["nextest", "list", "--workspace", "--cargo-profile", "ci", "--locked", "--list-type", "binaries-only", "--message-format", "json"]);
    let binaries_json = capture(build, "cargo nextest list (build all workspace tests)")?;
    let binaries_metadata = dir.join("binaries-metadata.json");
    // Retain both metadata files under target for diagnostics and reproducible listing.
    std::fs::write(&binaries_metadata, binaries_json).map_err(|e| format!("write {}: {e}", binaries_metadata.display()))?;
    Ok(Prepared { cargo_metadata, binaries_metadata })
}

fn capture(mut c: Command, label: &str) -> Result<Vec<u8>, String> {
    eprintln!("$ {label}");
    let out = c.stdin(Stdio::null()).stderr(Stdio::inherit()).output().map_err(|e| format!("{label}: {e}"))?;
    if out.status.success() { Ok(out.stdout) } else { Err(format!("{label}: {}", out.status)) }
}

/// Keep every gate running after failures, and stream native test names/times as they finish.
pub fn execute(native: (String, Command), side: Vec<(String, Command)>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let mut failures = Vec::new();
    std::thread::scope(|scope| {
        let tx_native = tx.clone();
        scope.spawn(move || {
            let _ = tx_native.send(run_job(native));
        });
        scope.spawn(move || {
            for job in side {
                let _ = tx.send(run_job(job));
            }
        });
        for (label, result, seconds) in rx {
            match result {
                Ok(()) => eprintln!("ok    {label:<44} {seconds:6.1}s"),
                Err(e) => {
                    eprintln!("FAIL  {label:<44} {seconds:6.1}s  {e}");
                    failures.push(label);
                }
            }
        }
    });
    if failures.is_empty() { Ok(()) } else { Err(format!("failed: {}", failures.join(", "))) }
}

fn run_job((label, mut c): (String, Command)) -> (String, Result<(), String>, f64) {
    let started = Instant::now();
    let result = c.stdin(Stdio::null()).status().map_err(|e| e.to_string()).and_then(|s| if s.success() { Ok(()) } else { Err(s.to_string()) });
    (label, result, started.elapsed().as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn full_ci_does_not_drop_heavy_tests_stop_on_failure_or_retry_failures() {
        let p = Prepared { cargo_metadata: "cargo.json".into(), binaries_metadata: "binaries.json".into() };
        let c = p.command_with_options(Some("3".into()), None, 16);
        let args: Vec<_> = c.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|s| s == ["--profile", "local-ci"]));
        assert!(args.iter().any(|s| s == "--ignore-default-filter"));
        assert!(args.iter().any(|s| s == "--no-fail-fast"));
        assert!(args.windows(2).any(|s| s == ["--retries", "0"]));
        assert!(args.windows(2).any(|s| s == ["--no-tests", "fail"]));
        assert!(args.windows(2).any(|s| s == ["--test-threads", "3"]));
        assert!(!args.iter().any(|s| s == "--workspace" || s == "--cargo-profile"));
    }

    #[test]
    fn matrix_workers_follow_the_effective_nextest_budget() {
        let p = Prepared { cargo_metadata: "cargo.json".into(), binaries_metadata: "binaries.json".into() };
        for (cpus, threads, expected) in [
            (16, None, "4"),
            (2, None, "2"),
            (1, None, "1"),
            (16, Some("1"), "1"),
            (16, Some("6"), "6"),
            (16, Some("num-cpus"), "16"),
            (16, Some("-2"), "14"),
            (2, Some("-8"), "1"),
            (2, Some("8"), "2"),
        ] {
            let c = p.command_with_options(threads.map(Into::into), None, cpus);
            assert_eq!(c.get_envs().find(|(k, _)| *k == "ACTION_MATRIX_JOBS").and_then(|(_, v)| v), Some(std::ffi::OsStr::new(expected)));
        }
        let c = p.command_with_options(Some("3".into()), Some("2".into()), 16);
        assert_eq!(c.get_envs().find(|(k, _)| *k == "ACTION_MATRIX_JOBS").and_then(|(_, v)| v), Some(std::ffi::OsStr::new("2")));
    }

    #[test]
    fn subprocess_fixture() {
        let Some(path) = std::env::var_os("SOLIDCRAFT_XTASK_TEST_MARKER") else { return };
        std::fs::write(path, b"ran").unwrap();
        assert_ne!(std::env::var("SOLIDCRAFT_XTASK_TEST_FAIL").ok().as_deref(), Some("1"), "intentional child-process failure");
    }

    fn fixture(path: &std::path::Path, fail: bool) -> Command {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args(["--exact", "test_nextest::tests::subprocess_fixture", "--test-threads=1"])
            .env("SOLIDCRAFT_XTASK_TEST_MARKER", path)
            .env("SOLIDCRAFT_XTASK_TEST_FAIL", if fail { "1" } else { "0" })
            // Keep nested libtest reports out of the parent harness transcript.
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    }

    #[test]
    fn subprocess_regressions_leave_one_unambiguous_test_report() {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "test_nextest::tests::failures_propagate_without_skipping_later_side_gates", "--test-threads=1"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout.lines().filter(|line| line.starts_with("test result:")).count(), 1, "{stdout:?}");
        assert!(!stdout.contains("test test_nextest::tests::subprocess_fixture"), "{stdout:?}");
    }

    #[test]
    fn failures_propagate_without_skipping_later_side_gates() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for (native_fails, side_fails, expected) in [(true, false, Some("native")), (false, true, Some("first")), (false, false, None)] {
            let dir = std::env::temp_dir().join(format!("solidcraft-xtask-nextest-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            let native = dir.join("native");
            let first = dir.join("first");
            let last = dir.join("last");
            let result = execute(
                ("native".into(), fixture(&native, native_fails)),
                vec![("first".into(), fixture(&first, side_fails)), ("last".into(), fixture(&last, false))],
            );
            assert_eq!(result, expected.map_or(Ok(()), |label| Err(format!("failed: {label}"))));
            assert!(native.is_file() && first.is_file() && last.is_file());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
