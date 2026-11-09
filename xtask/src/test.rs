//! `cargo xtask test`: build the workspace's tests once, then run every test binary.
//! By default they all run at once, with sequential doc/wasm `side` jobs alongside.
//! SOLIDCRAFT_TEST_JOBS opts into a shared bound for test binaries and those side jobs, and
//! splits each binary's libtest threads and action-matrix workers across the binaries in flight.

use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Instant;

use crate::{cargo, root};

/// A test executable from `cargo test --no-run`.
struct TestBin {
    label: String,
    exe: String,
    dir: String,
}

/// Build the tests with `cargo_args` (e.g. `--locked`), run each binary with `test_args`, and run
/// the `side` jobs in order. A configured job bound includes the sequential side-job worker.
/// Each prints a line when it ends; a failure prints its whole output and does not cancel work.
pub fn run(cargo_args: &[&str], test_args: &[&str], side: Vec<(String, Command)>) -> Result<(), String> {
    let limit = job_limit(std::env::var_os("SOLIDCRAFT_TEST_JOBS").as_deref())?;
    let split = thread_split(limit, std::thread::available_parallelism().map_or(1, |n| n.get()));
    let bins = build(cargo_args)?;
    let start = Instant::now();
    let side_count = side.len();
    let mut batches = Vec::with_capacity(bins.len() + 1);
    if !side.is_empty() {
        batches.push(side);
    }
    for b in &bins {
        batches.push(vec![(b.label.clone(), binary_command(b, test_args, split))]);
    }
    let jobs = limit.unwrap_or(batches.len()).min(batches.len());
    let split_note = split.map_or(String::new(), |t| format!("; {t} test threads and matrix workers per binary unless overridden"));
    eprintln!("test: {} binaries, {side_count} side jobs; at most {jobs} concurrent top-level commands{split_note}", bins.len());
    let mut failed = Vec::new();
    run_bounded(
        batches,
        jobs,
        |batch, tx| {
            for (label, c) in batch {
                let t0 = Instant::now();
                let _ = tx.send((label, finish(c), t0.elapsed().as_secs_f64()));
            }
        },
        |(label, result, secs)| match result {
            Ok(summary) => println!("ok    {label:<44} {secs:6.1}s  {summary}"),
            Err(output) => {
                println!("FAIL  {label:<44} {secs:6.1}s\n{output}");
                failed.push(label);
            }
        },
    );
    println!("test: {} binaries in {:.1}s", bins.len(), start.elapsed().as_secs_f64());
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

/// An absent limit preserves the existing unbounded default; invalid input fails before the test build.
fn job_limit(value: Option<&OsStr>) -> Result<Option<usize>, String> {
    let Some(value) = value else { return Ok(None) };
    value
        .to_str()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .map(Some)
        .ok_or_else(|| "SOLIDCRAFT_TEST_JOBS must be a positive integer".into())
}

/// The per-binary worker count that keeps the whole run inside the machine's parallelism:
/// `cpus` shared between `jobs` binaries in flight, never below one. No limit means no split,
/// so each binary keeps the thread count it would have chosen on its own.
fn thread_split(limit: Option<usize>, cpus: usize) -> Option<usize> {
    limit.map(|jobs| (cpus.max(1) / jobs.max(1)).max(1))
}

/// The value to hand a binary for `name`, or `None` when the caller set it explicitly: an
/// inherited `RUST_TEST_THREADS` or `ACTION_MATRIX_JOBS` always outranks the budget.
fn budget_env(name: &str, split: Option<usize>) -> Option<String> {
    split.filter(|_| std::env::var_os(name).is_none()).map(|threads| threads.to_string())
}

/// Workers only hold the queue lock while taking a job. Reporting remains live, and neither
/// a failed job nor a failed side command removes any remaining work from the queue.
fn run_bounded<T: Send, R: Send>(tasks: Vec<T>, jobs: usize, work: impl Fn(T, &mpsc::Sender<R>) + Sync, mut report: impl FnMut(R)) {
    let workers = jobs.max(1).min(tasks.len());
    let queue = Mutex::new(tasks.into_iter());
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let queue = &queue;
            let work = &work;
            let tx = tx.clone();
            scope.spawn(move || {
                loop {
                    let task = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).next();
                    let Some(task) = task else { break };
                    work(task, &tx);
                }
            });
        }
        drop(tx);
        for result in rx {
            report(result);
        }
    });
}

/// The command for one built test binary. With a split in force each binary's own libtest
/// threads and action-matrix workers are bounded to it, unless the caller already set them.
fn binary_command(b: &TestBin, test_args: &[&str], split: Option<usize>) -> Command {
    let mut c = Command::new(&b.exe);
    c.args(test_args).current_dir(&b.dir).env("CARGO_MANIFEST_DIR", &b.dir);
    for name in ["RUST_TEST_THREADS", "ACTION_MATRIX_JOBS"] {
        if let Some(threads) = budget_env(name, split) {
            c.env(name, threads);
        }
    }
    c
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
        let label = format!("{pkg} {kind} {}", m["target"]["name"].as_str().unwrap_or(""));
        bins.push(TestBin { label, exe: exe.to_string(), dir: dir.to_string_lossy().into_owned() });
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
    use super::{TestBin, binary_command, budget_env, job_limit, run_bounded, thread_split};
    use std::ffi::OsStr;
    use std::path::Path;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn job_limit_is_opt_in_and_rejects_invalid_values() {
        assert_eq!(job_limit(None).unwrap(), None);
        for value in [1, 2, usize::MAX] {
            assert_eq!(job_limit(Some(OsStr::new(&value.to_string()))).unwrap(), Some(value));
        }
        for value in ["", "0", "-1", "two", "1.5", " 2", "2 ", "184467440737095516150"] {
            assert!(job_limit(Some(OsStr::new(value))).is_err(), "{value:?}");
        }
    }

    #[test]
    fn bounded_workers_run_every_job_after_failure_with_budgets_one_and_two() {
        for budget in [1, 2] {
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);
            let first_wave = Barrier::new(budget);
            let mut results = Vec::new();
            run_bounded(
                (0..8).collect(),
                budget,
                |id, tx| {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    if id < budget {
                        first_wave.wait();
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    let outcome = if id == 0 { Err("deliberate first-job failure") } else { Ok(()) };
                    tx.send((id, outcome)).unwrap();
                },
                |result| results.push(result),
            );
            assert_eq!(peak.load(Ordering::SeqCst), budget);
            assert_eq!(active.load(Ordering::SeqCst), 0);
            results.sort_by_key(|(id, _)| *id);
            assert_eq!(results.iter().map(|(id, _)| *id).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
            assert_eq!(results.iter().filter(|(_, result)| result.is_err()).count(), 1);
            assert_eq!(results[0].1, Err("deliberate first-job failure"));
        }
    }

    #[test]
    fn side_batch_keeps_order_and_failure_does_not_skip_docs_or_binaries() {
        for budget in [1, 2] {
            let batches = vec![
                vec![("wasm", Err("deliberate wasm failure")), ("doc tests", Ok("passed"))],
                vec![("test one", Ok("passed"))],
                vec![("test two", Ok("passed"))],
            ];
            let mut results = Vec::new();
            run_bounded(
                batches,
                budget,
                |batch, tx| {
                    for result in batch {
                        tx.send(result).unwrap();
                    }
                },
                |result| results.push(result),
            );
            assert_eq!(results.len(), 4);
            assert!(results.iter().position(|(label, _)| *label == "wasm") < results.iter().position(|(label, _)| *label == "doc tests"));
            results.sort_by_key(|(label, _)| *label);
            assert_eq!(
                results,
                vec![("doc tests", Ok("passed")), ("test one", Ok("passed")), ("test two", Ok("passed")), ("wasm", Err("deliberate wasm failure"))]
            );
        }
    }

    #[test]
    fn empty_queue_and_excess_budget_do_not_invent_jobs() {
        let mut results = Vec::new();
        run_bounded(Vec::<usize>::new(), 2, |id, tx| tx.send(id).unwrap(), |id| results.push(id));
        assert!(results.is_empty());
        run_bounded(vec![7], usize::MAX, |id, tx| tx.send(id).unwrap(), |id| results.push(id));
        assert_eq!(results, [7]);
    }

    #[test]
    fn thread_split_shares_the_cpus_between_the_binaries_in_flight() {
        assert_eq!(thread_split(None, 32), None);
        assert_eq!(thread_split(Some(1), 32), Some(32));
        assert_eq!(thread_split(Some(4), 32), Some(8));
        assert_eq!(thread_split(Some(8), 32), Some(4));
        assert_eq!(thread_split(Some(32), 32), Some(1));
        // More binaries than cores still gives each one a worker, and one core is never zero.
        assert_eq!(thread_split(Some(64), 32), Some(1));
        assert_eq!(thread_split(Some(4), 1), Some(1));
        assert_eq!(thread_split(Some(4), 0), Some(1));
        assert_eq!(thread_split(Some(0), 8), Some(8));
    }

    #[test]
    fn budget_env_leaves_an_explicit_caller_override_alone() {
        // PATH is set in every test process, so it stands in for a caller-set override.
        assert!(std::env::var_os("PATH").is_some());
        assert_eq!(budget_env("PATH", Some(4)), None);
        assert_eq!(budget_env("PATH", None), None);
        assert_eq!(budget_env("SOLIDCRAFT_BUDGET_ENV_PROBE", Some(4)).as_deref(), Some("4"));
        assert_eq!(budget_env("SOLIDCRAFT_BUDGET_ENV_PROBE", None), None);
    }

    #[test]
    fn binary_command_bounds_threads_and_matrix_workers_only_under_a_budget() {
        let bin = TestBin { label: "pkg test t".into(), exe: "true".into(), dir: ".".into() };
        let env_of = |c: &std::process::Command, name: &str| {
            c.get_envs().find(|(k, _)| *k == OsStr::new(name)).and_then(|(_, v)| v).map(|v| v.to_string_lossy().into_owned())
        };
        for name in ["RUST_TEST_THREADS", "ACTION_MATRIX_JOBS"] {
            assert_eq!(env_of(&binary_command(&bin, &["--exact", "x"], None), name), None, "{name} unset without a budget");
            if std::env::var_os(name).is_none() {
                assert_eq!(env_of(&binary_command(&bin, &["--exact", "x"], Some(6)), name).as_deref(), Some("6"), "{name} follows the budget");
            }
        }
        let c = binary_command(&bin, &["--exact", "x"], None);
        assert_eq!(c.get_current_dir(), Some(Path::new(".")));
        assert_eq!(env_of(&c, "CARGO_MANIFEST_DIR").as_deref(), Some("."));
        assert_eq!(c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["--exact", "x"]);
    }
}
