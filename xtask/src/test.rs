//! `cargo xtask test`: build the workspace's tests once, then run every test binary.
//! The binaries run in a pool of `jobs` at a time (a quarter of the cores, or what
//! `SOLIDCRAFT_TEST_JOBS` says), and each one gets `RUST_TEST_THREADS`, so the suite keeps about
//! one running thread per core. The sequential doc/wasm `side` jobs share the same bound.

use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Instant;

use crate::{cargo, root};

/// The environment variable that says how many commands may run at the same time.
const JOBS_ENV: &str = "SOLIDCRAFT_TEST_JOBS";

/// The most jobs a caller may ask for.
const MAX_JOBS: usize = 1024;

/// A test executable from `cargo test --no-run`.
struct TestBin {
    label: String,
    exe: String,
    dir: String,
}

/// Build the tests with `cargo_args` (e.g. `--locked`), run each binary with `test_args`, and run
/// the `side` jobs in order. The job bound includes the sequential side-job worker.
/// Each prints a line when it ends; a failure prints its whole output and does not cancel work.
pub fn run(cargo_args: &[&str], test_args: &[&str], side: Vec<(String, Command)>) -> Result<(), String> {
    let nproc = nproc();
    let limit = job_limit(std::env::var_os(JOBS_ENV).as_deref(), nproc)?;
    let bins = build(cargo_args)?;
    let start = Instant::now();
    let side_count = side.len();
    let threads = test_threads(nproc, limit).to_string();
    let batches = test_batches(&bins, test_args, side, &threads);
    let jobs = limit.min(batches.len());
    eprintln!("test: {} binaries, {side_count} side jobs; at most {jobs} concurrent top-level commands", bins.len());
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

/// The batches the pool runs: the `side` jobs first, in order, then one batch per test binary.
/// Every binary gets `RUST_TEST_THREADS`, so the threads in flight add up to about one per core.
fn test_batches(bins: &[TestBin], test_args: &[&str], side: Vec<(String, Command)>, threads: &str) -> Vec<Vec<(String, Command)>> {
    let mut batches = Vec::with_capacity(bins.len() + 1);
    if !side.is_empty() {
        batches.push(side);
    }
    for b in bins {
        let mut c = Command::new(&b.exe);
        c.args(test_args).current_dir(&b.dir).env("CARGO_MANIFEST_DIR", &b.dir);
        c.env("RUST_TEST_THREADS", threads);
        batches.push(vec![(b.label.clone(), c)]);
    }
    batches
}

/// How many commands may run at the same time: what `SOLIDCRAFT_TEST_JOBS` says, else a quarter of
/// the cores, at least two. Invalid input fails before the test build.
fn job_limit(value: Option<&OsStr>, nproc: usize) -> Result<usize, String> {
    let Some(value) = value else { return Ok((nproc / 4).max(2)) };
    value
        .to_str()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .map(|n| n.min(MAX_JOBS))
        .ok_or_else(|| format!("{JOBS_ENV} must be a positive integer"))
}

/// The threads each test binary may use: the cores shared between the binaries in flight.
fn test_threads(nproc: usize, jobs: usize) -> usize {
    (nproc / jobs.max(1)).max(1)
}

/// The cores this machine has.
fn nproc() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
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
    use super::{MAX_JOBS, TestBin, finish, job_limit, run_bounded, test_batches, test_threads};
    use std::ffi::OsStr;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// The marker a fake binary looks for: without it the test binary does nothing when the test
    /// suite runs it.
    const FAKE_MARKER: &str = "fake-marker";
    /// Where the fake binaries say that they started, that they ended and how many threads they got.
    const FAKE_LOG: &str = "fake.log";
    /// How long a fake binary runs, so that several of them overlap.
    const FAKE_SLEEP: std::time::Duration = std::time::Duration::from_millis(400);

    /// The job bound: what `SOLIDCRAFT_TEST_JOBS` says, else a quarter of the cores. Invalid values
    /// fail instead of falling back.
    #[test]
    fn job_limit_follows_the_environment_variable_and_rejects_invalid_values() {
        assert_eq!(job_limit(None, 16).unwrap(), 4, "a quarter of the cores");
        assert_eq!(job_limit(None, 1).unwrap(), 2, "at least two jobs");
        assert_eq!(job_limit(None, 64).unwrap(), 16);
        assert_eq!(job_limit(Some(OsStr::new("1")), 16).unwrap(), 1);
        assert_eq!(job_limit(Some(OsStr::new("8")), 16).unwrap(), 8);
        assert_eq!(job_limit(Some(OsStr::new("99999")), 16).unwrap(), MAX_JOBS, "the count is capped");
        for value in ["", "0", "-1", "two", "1.5", " 2", "2 ", "184467440737095516150"] {
            assert!(job_limit(Some(OsStr::new(value)), 16).is_err(), "{value:?}");
        }
    }

    /// The threads each binary may use: the cores shared between the binaries in flight.
    #[test]
    fn test_threads_share_the_cores_between_the_binaries_in_flight() {
        assert_eq!(test_threads(16, 4), 4);
        assert_eq!(test_threads(16, 2), 8);
        assert_eq!(test_threads(16, 100), 1, "a binary always gets a thread");
        assert_eq!(test_threads(3, 4), 1, "a binary always gets a thread");
    }

    /// The pool keeps the number of binaries running at the same time under its limit, and every
    /// binary gets the shared thread count.
    #[test]
    fn pool_runs_at_most_jobs_binaries_at_once() {
        let jobs = 2;
        let count = 6;
        let dir = std::env::temp_dir().join(format!("xtask-pool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FAKE_MARKER), b"").unwrap();
        let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        let dir = dir.to_string_lossy().into_owned();
        let bins: Vec<TestBin> = (0..count).map(|i| TestBin { label: format!("fake {i}"), exe: exe.clone(), dir: dir.clone() }).collect();
        let threads = test_threads(16, jobs).to_string();
        let batches = test_batches(&bins, &["fake_test_binary"], vec![], &threads);
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
            |(label, result, _)| {
                if let Err(output) = result {
                    failed.push((label, output));
                }
            },
        );
        assert!(failed.is_empty(), "the fake binaries failed: {failed:?}");
        let log = std::fs::read_to_string(std::path::Path::new(&dir).join(FAKE_LOG)).unwrap();
        let mut running = 0;
        let mut peak = 0;
        for line in log.lines() {
            match line {
                "start" => {
                    running += 1;
                    peak = peak.max(running);
                }
                "end" => running -= 1,
                l if l.starts_with("threads ") => {
                    assert_eq!(l, format!("threads {threads}"), "the binary got the shared thread count");
                }
                _ => {}
            }
        }
        assert_eq!(log.lines().filter(|l| *l == "start").count(), count, "every binary ran");
        assert!(peak <= jobs, "{peak} binaries ran at once, the pool allows {jobs}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stand-in for a test binary: it says when it starts and when it ends, so that the test
    /// can count how many of them ran at the same time, and it records the `RUST_TEST_THREADS` it
    /// was given.
    #[test]
    fn fake_test_binary() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        if !std::path::Path::new(&dir).join(FAKE_MARKER).is_file() {
            return;
        }
        let log = std::path::Path::new(&dir).join(FAKE_LOG);
        let threads = std::env::var("RUST_TEST_THREADS").unwrap_or_default();
        append_line(&log, &format!("threads {threads}"));
        append_line(&log, "start");
        std::thread::sleep(FAKE_SLEEP);
        append_line(&log, "end");
    }

    /// Write one line to the log, so that several processes can share it.
    fn append_line(path: &std::path::Path, line: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        f.write_all(format!("{line}\n").as_bytes()).unwrap();
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
}
