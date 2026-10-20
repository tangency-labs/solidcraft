//! `--jobs N` for unit runs (`--green`, `--all`, several `--unit`s): each unit runs in
//! its own child process, at most N at a time.
//!
//! The C++ kernel keeps global state, so units never share a process (no threads). The
//! parent hands out units dynamically from one queue, longest first (by the case count
//! of each unit's previous report in the report directory), and spawns the next one as
//! soon as a child exits. A child is the same binary run as `--unit <id>` with the
//! caller's other options and [`WORKER_ENV`] set; it prints and writes its report
//! exactly as a serial run does. The parent prints each child's output in the serial
//! order (as soon as every earlier unit is done) and the usual final summary, so stdout
//! and the reports are identical to a serial run's.
//!
//! A child that crashes (signal, unexpected exit, no summary line) fails only its own
//! unit, with the tail of its stderr. Children are killed when the parent returns;
//! if the parent dies (Ctrl-C, `kill`), each child sees its stdin pipe close and exits
//! (see [`watch_parent`]).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitCode, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Set (to `1`) in the environment of a `--jobs` child.
pub const WORKER_ENV: &str = "DIFFTEST_JOBS_WORKER";

/// Exit status of a child whose parent went away.
const ORPHANED_EXIT: i32 = 3;

/// Lines of a crashed child's stderr quoted under its `FAIL` line.
const CRASH_STDERR_LINES: usize = 20;

/// Command-line flags that take a value (forwarded to children with it).
const VALUE_FLAGS: &[&str] = &[
    "--seeds",
    "--filter",
    "--report-dir",
    "--status-of",
    "--tol",
    "--fuzz",
    "--fuzz-out",
    "--baseline",
];

/// In a `--jobs` child: exits the process once the parent closes our stdin (the parent
/// exited or was killed), so no child outlives it.
pub fn watch_parent() {
    std::thread::spawn(|| {
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 64];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => std::process::exit(ORPHANED_EXIT),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => std::process::exit(ORPHANED_EXIT),
            }
        }
    });
}

/// `--jobs` value to worker count: `0` means every available core.
pub fn resolve(jobs: usize) -> usize {
    if jobs == 0 {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    } else {
        jobs
    }
}

/// The caller's arguments without the unit selection (`--green`, `--all`, `--unit`) and
/// `--jobs`: what every child gets before its own `--unit <id>`.
pub fn forwarded_args(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--green" | "--all" => {}
            "--unit" | "-u" | "--jobs" | "-j" => {
                it.next();
            }
            f if VALUE_FLAGS.contains(&f) => {
                out.push(a.clone());
                if let Some(v) = it.next() {
                    out.push(v.clone());
                }
            }
            _ => out.push(a.clone()),
        }
    }
    out
}

/// Case count of the first `<dir>/<id>.json` found in `dirs` (0 when there is none):
/// the scheduling weight.
fn previous_cases(dirs: &[&Path], id: &str) -> u64 {
    dirs.iter()
        .find_map(|d| std::fs::read_to_string(d.join(format!("{id}.json"))).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("cases_total").and_then(serde_json::Value::as_u64))
        .unwrap_or(0)
}

/// Queue order: heaviest previous report (looked up in `dirs`, in order) first, ties
/// (and units without a report) in the serial order.
pub fn schedule(units: &[String], dirs: &[&Path]) -> Vec<usize> {
    let weights: Vec<u64> = units.iter().map(|u| previous_cases(dirs, u)).collect();
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(weights.get(i).copied().unwrap_or(0)), i));
    order
}

/// One finished child, as the parent reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitRun {
    /// What the parent prints on stdout for this unit (ends with a newline, or empty).
    pub stdout: String,
    /// The child's stderr, forwarded as is.
    pub stderr: String,
    /// The unit counts as failed.
    pub failed: bool,
    /// The unit counts as partial.
    pub partial: bool,
    /// The child hit a setup error (exit status 2), e.g. an unwritable report.
    pub setup_error: bool,
}

/// `killed by signal N` or `exit code N`.
fn exit_detail(status: ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return format!("killed by signal {sig}");
        }
    }
    format!("exit code {}", status.code().unwrap_or(-1))
}

/// Parses `difftest: 1 unit(s), P passed, Q partial, F failed[...]` into `(Q, F)`.
fn parse_summary(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("difftest: 1 unit(s), ")?;
    let mut parts = rest.split(", ");
    let _passed = parts.next()?.strip_suffix(" passed")?;
    let partial = parts.next()?.strip_suffix(" partial")?.parse().ok()?;
    let failed = parts.next()?.split(' ').next()?.parse().ok()?;
    Some((partial, failed))
}

/// Turns a finished child's output into what the parent prints and counts. A child that
/// did not end normally (exit 0, 1 or 2 after its summary line) fails its unit.
pub fn interpret(id: &str, status: Option<ExitStatus>, stdout: &str, stderr: &str) -> UnitRun {
    let code = status.and_then(|s| s.code());
    let body = stdout.trim_end_matches('\n');
    let (block, last) = body.rsplit_once('\n').unwrap_or(("", body));
    if let (Some(c @ 0..=2), Some((partial, failed))) = (code, parse_summary(last)) {
        let mut out = block.to_string();
        if !out.is_empty() {
            out.push('\n');
        }
        return UnitRun {
            stdout: out,
            stderr: stderr.to_string(),
            failed: failed > 0,
            partial: partial > 0,
            setup_error: c == 2,
        };
    }
    let detail = status.map_or_else(|| "could not be waited for".to_string(), exit_detail);
    let mut out = stdout.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let _ = writeln!(out, "FAIL {id} (worker process {detail})");
    let lines: Vec<&str> = stderr.lines().collect();
    for l in lines
        .iter()
        .skip(lines.len().saturating_sub(CRASH_STDERR_LINES))
    {
        let _ = writeln!(out, "  stderr: {l}");
    }
    UnitRun {
        stdout: out,
        stderr: String::new(),
        failed: true,
        partial: false,
        setup_error: false,
    }
}

/// One running child.
struct Worker {
    unit: usize,
    child: Child,
    // Held open while the child runs; dropping it tells the child to exit.
    _stdin: Option<ChildStdin>,
    out: Option<JoinHandle<Vec<u8>>>,
    err: Option<JoinHandle<Vec<u8>>>,
    start: Instant,
}

/// All running children; killed (and reaped) when dropped, e.g. on an early return.
struct Pool(Vec<Worker>);

impl Drop for Pool {
    fn drop(&mut self) {
        for w in &mut self.0 {
            let _ = w.child.kill();
            let _ = w.child.wait();
        }
    }
}

fn drain<R: Read + Send + 'static>(r: Option<R>) -> Option<JoinHandle<Vec<u8>>> {
    r.map(|mut r| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        })
    })
}

fn joined(h: Option<JoinHandle<Vec<u8>>>) -> String {
    let bytes = h.and_then(|h| h.join().ok()).unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn spawn(exe: &Path, args: &[String], id: &str, unit: usize) -> std::io::Result<Worker> {
    let mut child = Command::new(exe)
        .args(args)
        .arg("--unit")
        .arg(id)
        .env(WORKER_ENV, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    Ok(Worker {
        unit,
        _stdin: child.stdin.take(),
        out: drain(child.stdout.take()),
        err: drain(child.stderr.take()),
        child,
        start: Instant::now(),
    })
}

/// Runs `units` in up to `jobs` child processes and prints like the serial loop in
/// `main.rs`: each unit's lines in `units` order, then the summary line. `weights` are
/// the report directories whose previous reports order the queue (see [`schedule`]).
pub fn run_parallel(units: &[String], args: &[String], jobs: usize, weights: &[&Path]) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("difftest: current_exe: {e}");
            return ExitCode::from(2);
        }
    };
    let wall = Instant::now();
    let mut queue: VecDeque<usize> = schedule(units, weights).into();
    let mut results: Vec<Option<UnitRun>> = vec![None; units.len()];
    let mut times: Vec<Duration> = vec![Duration::ZERO; units.len()];
    let mut printed = 0usize;
    let mut pool = Pool(Vec::new());
    let (mut failed, mut partial, mut setup_error) = (0usize, 0usize, false);
    let stdout = std::io::stdout();
    while printed < units.len() {
        while pool.0.len() < jobs.max(1) {
            let Some(i) = queue.pop_front() else { break };
            let Some(id) = units.get(i) else { continue };
            match spawn(&exe, args, id, i) {
                Ok(w) => pool.0.push(w),
                Err(e) => {
                    // The Pool's Drop kills the children already running.
                    eprintln!("difftest: spawn {}: {e}", exe.display());
                    return ExitCode::from(2);
                }
            }
        }
        let mut progressed = false;
        let mut k = 0;
        while k < pool.0.len() {
            let status = match pool.0.get_mut(k).map(|w| w.child.try_wait()) {
                Some(Ok(Some(st))) => Some(Some(st)),
                Some(Ok(None)) | None => None,
                Some(Err(_)) => Some(None),
            };
            let Some(status) = status else {
                k += 1;
                continue;
            };
            let mut w = pool.0.swap_remove(k);
            let out = joined(w.out.take());
            let err = joined(w.err.take());
            if let Some(t) = times.get_mut(w.unit) {
                *t = w.start.elapsed();
            }
            let id = units.get(w.unit).map_or("", String::as_str);
            if let Some(slot) = results.get_mut(w.unit) {
                *slot = Some(interpret(id, status, &out, &err));
            }
            progressed = true;
        }
        while let Some(Some(r)) = results.get_mut(printed).map(Option::take) {
            let mut so = stdout.lock();
            let _ = so.write_all(r.stdout.as_bytes());
            let _ = so.flush();
            drop(so);
            let _ = std::io::stderr().write_all(r.stderr.as_bytes());
            failed += usize::from(r.failed);
            partial += usize::from(r.partial && !r.failed);
            setup_error |= r.setup_error;
            printed += 1;
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    println!(
        "difftest: {} unit(s), {} passed, {partial} partial, {failed} failed",
        units.len(),
        units.len() - failed - partial
    );
    // Timing goes to stderr so stdout stays identical to a serial run.
    let sum: Duration = times.iter().sum();
    let (slow, slowest) = times
        .iter()
        .enumerate()
        .max_by_key(|(_, t)| **t)
        .map_or(("-", Duration::ZERO), |(i, t)| {
            (units.get(i).map_or("-", String::as_str), *t)
        });
    eprintln!(
        "difftest --jobs {jobs}: wall {:.1} s, sum of unit times {:.1} s, longest unit {slow} {:.1} s",
        wall.elapsed().as_secs_f64(),
        sum.as_secs_f64(),
        slowest.as_secs_f64()
    );
    if setup_error {
        ExitCode::from(2)
    } else if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn forwards_everything_but_the_selection() {
        let raw = s(&[
            "--green",
            "-j",
            "8",
            "--unit",
            "a.b",
            "--filter",
            "--all",
            "--seeds",
            "4",
            "--no-report",
            "--jobs",
            "0",
            "-u",
            "c",
            "--all",
        ]);
        assert_eq!(
            forwarded_args(&raw),
            s(&["--filter", "--all", "--seeds", "4", "--no-report"])
        );
    }

    #[test]
    fn summary_lines() {
        assert_eq!(
            parse_summary("difftest: 1 unit(s), 0 passed, 1 partial, 0 failed"),
            Some((1, 0))
        );
        assert_eq!(
            parse_summary("difftest: 1 unit(s), 0 passed, 0 partial, 1 failed, 1 dropped"),
            Some((0, 1))
        );
        assert_eq!(parse_summary("PASS a cases=1"), None);
    }

    #[cfg(unix)]
    #[test]
    fn interprets_children() {
        use std::os::unix::process::ExitStatusExt;
        let ok = ExitStatus::from_raw(0);
        let r = interpret(
            "u",
            Some(ok),
            "PASS u cases=1\n  report: r/u.json\ndifftest: 1 unit(s), 1 passed, 0 partial, 0 failed\n",
            "",
        );
        assert_eq!(r.stdout, "PASS u cases=1\n  report: r/u.json\n");
        assert!(!r.failed && !r.partial && !r.setup_error);
        let partial = interpret(
            "u",
            Some(ok),
            "PARTIAL u\ndifftest: 1 unit(s), 0 passed, 1 partial, 0 failed\n",
            "",
        );
        assert!(partial.partial && !partial.failed);
        let fail = interpret(
            "u",
            Some(ExitStatus::from_raw(1 << 8)),
            "FAIL u\ndifftest: 1 unit(s), 0 passed, 0 partial, 1 failed\n",
            "",
        );
        assert!(fail.failed);
        // SIGSEGV
        let crash = interpret("u", Some(ExitStatus::from_raw(11)), "", "a\nboom\n");
        assert!(crash.failed);
        assert_eq!(
            crash.stdout,
            "FAIL u (worker process killed by signal 11)\n  stderr: a\n  stderr: boom\n"
        );
        // A clean exit without the summary line is a crash too.
        let cut = interpret("u", Some(ok), "PASS u\n", "");
        assert!(cut.failed);
        assert!(cut
            .stdout
            .starts_with("PASS u\nFAIL u (worker process exit code 0)"));
    }

    #[test]
    fn schedule_is_heaviest_first_then_serial_order() {
        let dir = crate::case::repo_root()
            .join("bin")
            .join(format!("difftest-jobs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.json"), r#"{"cases_total": 5}"#).unwrap();
        std::fs::write(dir.join("c.json"), r#"{"cases_total": 50}"#).unwrap();
        let order = schedule(
            &s(&["a", "b", "c", "d"]),
            &[dir.join("none").as_path(), &dir],
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(order, vec![2, 1, 0, 3]);
    }
}
