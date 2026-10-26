//! `cargo xtask test`: build the workspace's tests once, then run every test binary.
//! The binaries run in a pool of `jobs` at a time (a quarter of the cores, or what
//! `SOLIDCRAFT_TEST_JOBS` says), and the pool gives each binary a share of the cores, so the
//! suite keeps about one running thread per core. A caller who sets `RUST_TEST_THREADS` keeps
//! that value: the pool never overwrites it. The sequential doc/wasm `side` jobs share the same
//! bound.

use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Instant;

use crate::{cargo, root};

/// The environment variable that says how many commands may run at the same time.
const JOBS_ENV: &str = "SOLIDCRAFT_TEST_JOBS";

/// The environment variable that says how many threads each test binary may use.
const THREADS_ENV: &str = "RUST_TEST_THREADS";

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
    let threads = thread_budget(nproc, limit, std::env::var_os(THREADS_ENV).as_deref());
    let batches = test_batches(&bins, test_args, side, threads.as_deref());
    let jobs = limit.min(batches.len());
    eprintln!("test: {} binaries, {side_count} side jobs; at most {jobs} concurrent top-level commands{}", bins.len(), threads_note(&threads));
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

/// What the pool did with the thread count, for the log line: its own split, or the caller's
/// value left alone.
fn threads_note(threads: &Option<String>) -> String {
    match threads {
        Some(count) => format!("; {count} test threads per binary"),
        None => format!("; the caller's {THREADS_ENV} is left alone"),
    }
}

/// The batches the pool runs: the `side` jobs first, in order, then one batch per test binary.
/// `threads` is the thread count the pool gives each binary, or `None` when the caller set
/// `RUST_TEST_THREADS` itself: then each binary inherits the caller's value.
fn test_batches(bins: &[TestBin], test_args: &[&str], side: Vec<(String, Command)>, threads: Option<&str>) -> Vec<Vec<(String, Command)>> {
    let mut batches = Vec::with_capacity(bins.len() + 1);
    if !side.is_empty() {
        batches.push(side);
    }
    for b in bins {
        let mut c = Command::new(&b.exe);
        c.args(test_args).current_dir(&b.dir).env("CARGO_MANIFEST_DIR", &b.dir);
        if let Some(count) = threads {
            c.env(THREADS_ENV, count);
        }
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

/// The thread count the pool gives each test binary, or `None` to leave `RUST_TEST_THREADS` to
/// the caller.
///
/// A caller that sets `RUST_TEST_THREADS` has chosen the thread count: the test binaries inherit
/// that value and the pool never overwrites it. Otherwise the pool shares the cores between the
/// binaries in flight, so that the threads in flight add up to about one per core.
fn thread_budget(nproc: usize, jobs: usize, caller: Option<&OsStr>) -> Option<String> {
    match caller {
        // The caller chose the thread count: the binaries keep it.
        Some(_) => None,
        // Nobody chose: share the cores between the binaries in flight.
        None => Some(shared_threads(nproc, jobs).to_string()),
    }
}

/// The threads one binary gets when the caller chose none: the cores shared between the binaries
/// in flight, never below one.
fn shared_threads(nproc: usize, jobs: usize) -> usize {
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
    use super::{MAX_JOBS, THREADS_ENV, TestBin, finish, job_limit, run_bounded, shared_threads, test_batches, thread_budget};
    use std::ffi::OsStr;
    use std::process::Command;
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

    /// The threads one binary gets when the caller chose none: the cores shared between the
    /// binaries in flight.
    #[test]
    fn shared_threads_divide_the_cores_between_the_binaries_in_flight() {
        assert_eq!(shared_threads(16, 4), 4);
        assert_eq!(shared_threads(16, 2), 8);
        assert_eq!(shared_threads(16, 100), 1, "a binary always gets a thread");
        assert_eq!(shared_threads(3, 4), 1, "a binary always gets a thread");
    }

    /// An explicit `RUST_TEST_THREADS` from the caller wins: the pool sets nothing, so the test
    /// binaries inherit the caller's value. Only when the caller chose nothing does the pool
    /// split the cores.
    #[test]
    fn an_explicit_caller_thread_count_wins_over_the_pool_split() {
        assert_eq!(thread_budget(16, 4, Some(OsStr::new("3"))), None, "the caller's value is never overwritten");
        assert_eq!(thread_budget(16, 4, Some(OsStr::new(""))), None, "an empty value is the caller's too");
        assert_eq!(thread_budget(16, 4, None).as_deref(), Some("4"), "the pool splits the cores");
        assert_eq!(thread_budget(16, 1, None).as_deref(), Some("16"));
        assert_eq!(thread_budget(1, 4, None).as_deref(), Some("1"), "a binary always gets a thread");
    }

    /// The pool keeps the number of binaries running at the same time under its limit, and every
    /// binary gets the shared thread count.
    #[test]
    fn pool_runs_at_most_jobs_binaries_at_once() {
        let jobs = 2;
        let count = 6;
        let dir = fake_dir("pool");
        let bins = fake_bins(count, &dir);
        let threads = shared_threads(16, jobs).to_string();
        let batches = test_batches(&bins, &["fake_test_binary"], vec![], Some(&threads));
        let results = run_batches(batches, jobs);
        assert!(results.iter().all(|(_, result, _)| result.is_ok()), "the fake binaries failed: {results:?}");
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

    /// The thread count the test binary really receives: the caller's value when the caller set
    /// one, the pool's split when nobody did.
    #[test]
    fn the_test_binary_keeps_the_callers_thread_count_and_otherwise_gets_the_pool_split() {
        // The caller set RUST_TEST_THREADS: the pool must not overwrite it.
        let caller_dir = fake_dir("caller-threads");
        run_fake_pool(&caller_dir, Some("3"));
        assert_eq!(logged_threads(&caller_dir), ["3", "3"], "the binary kept the caller's thread count");
        let _ = std::fs::remove_dir_all(&caller_dir);

        // Nobody set RUST_TEST_THREADS: the pool's split applies.
        let pool_dir = fake_dir("pool-threads");
        run_fake_pool(&pool_dir, None);
        assert_eq!(logged_threads(&pool_dir), ["8", "8"], "the binary got the pool's split");
        let _ = std::fs::remove_dir_all(&pool_dir);
    }

    /// The pool adds no `RUST_TEST_THREADS` of its own when the caller chose one, so the test
    /// binary inherits the caller's value.
    #[test]
    fn the_pool_leaves_the_callers_thread_count_out_of_the_binaries_environment() {
        let bin = TestBin { label: "pkg test t".into(), exe: "true".into(), dir: ".".into() };
        let caller = thread_budget(16, 4, Some(OsStr::new("3")));
        let batches = test_batches(std::slice::from_ref(&bin), &["--exact", "x"], vec![], caller.as_deref());
        let overrides: Vec<String> = batches[0][0].1.get_envs().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        assert!(!overrides.contains(&THREADS_ENV.to_string()), "the pool must not set {THREADS_ENV}: {overrides:?}");
        let pool = thread_budget(16, 4, None);
        let batches = test_batches(std::slice::from_ref(&bin), &["--exact", "x"], vec![], pool.as_deref());
        let set =
            batches[0][0].1.get_envs().find(|(k, _)| *k == OsStr::new(THREADS_ENV)).and_then(|(_, v)| v).map(|v| v.to_string_lossy().into_owned());
        assert_eq!(set.as_deref(), Some("4"), "the pool sets its own split when nobody chose");
    }

    /// Run the fake pool in a subprocess whose environment is the caller's: `caller` is the
    /// `RUST_TEST_THREADS` the caller set, or `None` when nobody set it.
    fn run_fake_pool(dir: &str, caller: Option<&str>) {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.arg("fake_pool_driver").env("XTASK_FAKE_POOL", "1").env("XTASK_FAKE_POOL_DIR", dir).env("XTASK_FAKE_POOL_CORES", "16");
        match caller {
            Some(value) => {
                c.env(THREADS_ENV, value);
            }
            None => {
                c.env_remove(THREADS_ENV);
            }
        }
        let out = c.output().unwrap();
        assert!(out.status.success(), "the fake pool failed: {}", String::from_utf8_lossy(&out.stdout));
    }

    /// A stand-in for the pool: it builds the test commands the way `run` does, with the
    /// caller's `RUST_TEST_THREADS` taken from its own environment, and runs the fake binaries.
    /// The test above starts it with the caller's environment, so the fake binaries inherit it.
    #[test]
    fn fake_pool_driver() {
        if std::env::var_os("XTASK_FAKE_POOL").is_none() {
            return;
        }
        let cores: usize = std::env::var("XTASK_FAKE_POOL_CORES").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
        let caller = std::env::var_os(THREADS_ENV);
        let budget = thread_budget(cores, 2, caller.as_deref());
        let dir = std::env::var("XTASK_FAKE_POOL_DIR").unwrap_or_default();
        let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        let bins: Vec<TestBin> = (0..2).map(|i| TestBin { label: format!("fake {i}"), exe: exe.clone(), dir: dir.clone() }).collect();
        let batches = test_batches(&bins, &["fake_test_binary"], vec![], budget.as_deref());
        for batch in batches {
            for (_, c) in batch {
                let _ = finish(c);
            }
        }
    }

    /// A directory with the fake-binary marker: the fake binaries look for it and log beside it.
    fn fake_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FAKE_MARKER), b"").unwrap();
        dir.to_string_lossy().into_owned()
    }

    /// The fake test binaries: the test binary itself, run as a test binary.
    fn fake_bins(count: usize, dir: &str) -> Vec<TestBin> {
        let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        (0..count).map(|i| TestBin { label: format!("fake {i}"), exe: exe.clone(), dir: dir.to_string() }).collect()
    }

    /// Run the batches in the pool and collect what each of them reported.
    fn run_batches(batches: Vec<Vec<(String, Command)>>, jobs: usize) -> Vec<(String, Result<String, String>, f64)> {
        let mut results = Vec::new();
        run_bounded(
            batches,
            jobs,
            |batch, tx| {
                for (label, c) in batch {
                    let t0 = Instant::now();
                    let _ = tx.send((label, finish(c), t0.elapsed().as_secs_f64()));
                }
            },
            |result| results.push(result),
        );
        results
    }

    /// The thread counts the fake binaries were given, one per binary.
    fn logged_threads(dir: &str) -> Vec<String> {
        let log = std::fs::read_to_string(std::path::Path::new(dir).join(FAKE_LOG)).unwrap();
        log.lines().filter_map(|l| l.strip_prefix("threads ")).map(str::to_string).collect()
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
