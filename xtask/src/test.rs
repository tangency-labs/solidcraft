//! `cargo xtask test`: build the workspace's tests once, then run every test binary at the same
//! time. `cargo test` runs them one after another, so one slow binary held up all the others;
//! the doc tests and any other `side` jobs run alongside on their own thread.
//!
//! The binaries run in a pool of `jobs` at a time, and each one gets `RUST_TEST_THREADS`, so the
//! suite keeps about one running thread per core. Running every binary at once oversubscribed
//! the machine, because every binary is a process that starts threads of its own.

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use crate::{cargo, root};

/// The environment variable that says how many test binaries may run at the same time.
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
/// the `side` jobs one after another meanwhile. Each prints a line when it ends; a failure prints
/// its whole output.
pub fn run(cargo_args: &[&str], test_args: &[&str], side: Vec<(String, Command)>) -> Result<(), String> {
    let bins = build(cargo_args)?;
    let start = Instant::now();
    let failed = run_bins(&bins, test_args, side, test_jobs());
    println!("test: {} binaries in {:.1}s", bins.len(), start.elapsed().as_secs_f64());
    if failed.is_empty() { Ok(()) } else { Err(format!("failed: {}", failed.join(", "))) }
}

/// Run every test binary, at most `jobs` of them at a time, and the `side` jobs one after another
/// meanwhile. Returns the labels of the binaries that failed.
fn run_bins(bins: &[TestBin], test_args: &[&str], side: Vec<(String, Command)>, jobs: usize) -> Vec<String> {
    let permits = Permits::new(jobs);
    let threads = test_threads(nproc(), jobs).to_string();
    let (tx, rx) = mpsc::channel::<(String, Result<String, String>, f64)>();
    let mut failed = Vec::new();
    std::thread::scope(|scope| {
        for b in bins {
            let tx = tx.clone();
            let permits = &permits;
            let threads = threads.as_str();
            scope.spawn(move || {
                permits.take();
                let t0 = Instant::now();
                let mut c = Command::new(&b.exe);
                c.args(test_args).current_dir(&b.dir).env("CARGO_MANIFEST_DIR", &b.dir);
                c.env("RUST_TEST_THREADS", threads);
                let result = finish(c);
                permits.give_back();
                let _ = tx.send((b.label.clone(), result, t0.elapsed().as_secs_f64()));
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
    failed
}

/// A counting semaphore with `jobs` permits: a binary takes one to start and gives it back when
/// it ends, so that at most `jobs` of them run at the same time. The permits travel through a
/// bounded channel, so no binary starts before one of its neighbours has finished.
struct Permits {
    rx: std::sync::Mutex<mpsc::Receiver<()>>,
    tx: mpsc::SyncSender<()>,
}

impl Permits {
    /// A semaphore with `jobs` permits, all of them free.
    fn new(jobs: usize) -> Self {
        let jobs = jobs.max(1);
        let (tx, rx) = mpsc::sync_channel::<()>(jobs);
        for _ in 0..jobs {
            let _ = tx.send(());
        }
        Self { rx: std::sync::Mutex::new(rx), tx }
    }

    /// Wait until a permit is free.
    fn take(&self) {
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let _ = rx.recv();
    }

    /// Give a permit back.
    fn give_back(&self) {
        let _ = self.tx.send(());
    }
}

/// How many test binaries may run at the same time: what `SOLIDCRAFT_TEST_JOBS` says, or a
/// quarter of the cores.
fn test_jobs() -> usize {
    job_count(std::env::var(JOBS_ENV).ok(), nproc())
}

/// The number of jobs: `SOLIDCRAFT_TEST_JOBS` when it holds a number, else a quarter of the
/// cores, at least two.
fn job_count(env: Option<String>, nproc: usize) -> usize {
    if let Some(v) = env
        && let Ok(n) = v.trim().parse::<usize>()
        && n > 0
    {
        return n.min(MAX_JOBS);
    }
    (nproc / 4).max(2)
}

/// The threads each test binary may use: the cores shared between the binaries in flight.
fn test_threads(nproc: usize, jobs: usize) -> usize {
    (nproc / jobs.max(1)).max(1)
}

/// The cores this machine has.
fn nproc() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
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
    use super::*;

    /// The marker a fake binary looks for: without it the test binary does nothing when the test
    /// suite runs it.
    const FAKE_MARKER: &str = "fake-marker";
    /// Where the fake binaries say that they started and that they ended.
    const FAKE_LOG: &str = "fake.log";
    /// How long a fake binary runs, so that several of them overlap.
    const FAKE_SLEEP: std::time::Duration = std::time::Duration::from_millis(400);

    /// A stand-in for a test binary: it says when it starts and when it ends, so that the test
    /// can count how many of them ran at the same time.
    #[test]
    fn fake_test_binary() {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        if !std::path::Path::new(&dir).join(FAKE_MARKER).is_file() {
            return;
        }
        let log = std::path::Path::new(&dir).join(FAKE_LOG);
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

    /// The pool keeps the number of binaries running at the same time under its limit.
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
        let failed = run_bins(&bins, &["fake_test_binary"], vec![], jobs);
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
                _ => {}
            }
        }
        assert_eq!(log.lines().filter(|l| *l == "start").count(), count, "every binary ran");
        assert!(peak <= jobs, "{peak} binaries ran at once, the pool allows {jobs}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The number of jobs: what `SOLIDCRAFT_TEST_JOBS` says, else a quarter of the cores.
    #[test]
    fn job_count_follows_the_environment_variable() {
        assert_eq!(job_count(None, 16), 4, "a quarter of the cores");
        assert_eq!(job_count(None, 1), 2, "at least two jobs");
        assert_eq!(job_count(Some("8".into()), 16), 8);
        assert_eq!(job_count(Some(" 3 ".into()), 16), 3);
        assert_eq!(job_count(Some("0".into()), 16), 4, "zero is not a number of jobs");
        assert_eq!(job_count(Some("many".into()), 16), 4, "a word is not a number of jobs");
        assert_eq!(job_count(Some("-1".into()), 16), 4, "a negative count is not a number of jobs");
        assert_eq!(job_count(Some("99999".into()), 16), MAX_JOBS, "the count is capped");
    }

    /// The threads each binary may use: the cores shared between the binaries in flight.
    #[test]
    fn test_threads_share_the_cores_between_the_binaries_in_flight() {
        assert_eq!(test_threads(16, 4), 4);
        assert_eq!(test_threads(16, 2), 8);
        assert_eq!(test_threads(16, 100), 1, "a binary always gets a thread");
        assert_eq!(test_threads(3, 4), 1, "a binary always gets a thread");
    }
}
