//! `--fuzz N`: adversarial probing of a unit's FFI guard.
//!
//! Fuzz case `i` of unit `u` starts from one of the unit's normal cases (fixtures and
//! generated cases), chosen by a [`Rng`] seeded with [`fuzz_seed`]`(u, i)`, and [`mutate`]s
//! one to three JSON leaves or arrays of its inputs: floats become `0`, `-0`, `±1`,
//! `±1e-300` or `±1e300` (or `"NaN"`/`"inf"`/`"-inf"` when the unit's inputs already carry
//! such strings), integers become `-1`, `0`, `2^31`, `2^32`, `i64::MIN`/`MAX` or the
//! length of an input array, arrays are emptied, truncated or get an element duplicated,
//! and flags are flipped. The cases are therefore deterministic.
//!
//! Cases run in batches of [`BATCH`] in a child process (`difftest --fuzz-exec`, see
//! [`run_child`]), so a C++ crash or hang ends the batch, not the run. The child reports
//! each case as it starts and finishes, so the case that died is known; it is re-run
//! alone to confirm. A crash that only happens after earlier cases of its batch is still
//! a failure, and its reproduction file keeps those cases in order. A timeout that does
//! not reproduce when the case runs alone is load-dependent: the case counts with the
//! verdict it gets alone and the timeout is only a note.
//!
//! Verdicts ([`Verdict`]): `rejected` (the C++ runner returned a harness error, the FFI
//! guard refused the input, or its fixture constructor refused it; Rust is not run), `malformed` (the mutation broke the input schema), `ran`
//! (both sides ran; `differs` when they disagree, which is only a note because fuzz
//! inputs may lie outside the generator's domain), and the failures `crash` (killed by a
//! signal or an abnormal exit), `panic` (a runner panicked on either side) and `timeout`
//! ([`CASE_TIMEOUT`] per case). Each failure's inputs are written in fixture format to
//! `<out>/<unit>/fuzzNNNNN.json`, ready to become a regression fixture.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{mpsc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::case::{Case, CaseContext, CaseSource};
use crate::compare::{compare, AbsFloor};
use crate::json::to_canonical_string;
use crate::output::Output;
use crate::rng::{fnv1a64, Rng};
use crate::runner::UnitDiff;
use crate::HarnessError;

/// Time limit for one case (both sides together).
pub const CASE_TIMEOUT: Duration = Duration::from_secs(20);

/// Cases per child process.
pub const BATCH: usize = 100;

/// Address-space limit of a child (KiB), so an input-sized allocation fails instead of
/// swapping a shared machine.
pub const MEM_LIMIT_KIB: u64 = 4 << 20;

/// Floats a float leaf is replaced with.
pub const FLOATS: [f64; 8] = [0.0, -0.0, 1.0, -1.0, 1e-300, -1e-300, 1e300, -1e300];

/// Non-finite floats, in the `"NaN"` / `"inf"` / `"-inf"` string convention of the
/// runners that accept them (used only when a unit's inputs already contain one).
pub const NON_FINITE: [&str; 3] = ["NaN", "inf", "-inf"];

/// Integers an integer leaf is replaced with (besides the length of an input array):
/// `-1`, `0`, just past `i32` and `u32`, and the `i64` extremes.
pub const INTS: [i64; 6] = [-1, 0, 1 << 31, 1 << 32, i64::MIN, i64::MAX];

/// Prefix of the child's protocol lines on stdout (anything else there is upstream output).
const MARK: &str = "@fuzz ";

/// Seed of fuzz case `index` of `unit`: `fnv1a64("fuzz:" + unit) + index`.
pub fn fuzz_seed(unit: &str, index: usize) -> u64 {
    fnv1a64(&format!("fuzz:{unit}")).wrapping_add(index as u64)
}

/// What a mutation site holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Leaf {
    /// A float (a JSON number with a fraction or exponent, or `"NaN"`/`"inf"`/`"-inf"`).
    Float,
    /// An integer.
    Int,
    /// A flag.
    Bool,
    /// A non-empty array.
    Array,
}

/// Every mutation site of `v` as `(kind, JSON pointer)`, in a stable order (object keys
/// sorted).
pub fn sites(v: &Value) -> Vec<(Leaf, String)> {
    fn walk(v: &Value, ptr: &str, out: &mut Vec<(Leaf, String)>) {
        let kind = match v {
            Value::Number(n) if n.is_f64() => Some(Leaf::Float),
            Value::Number(_) => Some(Leaf::Int),
            Value::String(s) if NON_FINITE.contains(&s.as_str()) => Some(Leaf::Float),
            Value::Bool(_) => Some(Leaf::Bool),
            Value::Array(a) if !a.is_empty() => Some(Leaf::Array),
            _ => None,
        };
        if let Some(k) = kind {
            out.push((k, ptr.to_string()));
        }
        match v {
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(x, &format!("{ptr}/{i}"), out);
                }
            }
            Value::Object(o) => {
                let mut keys: Vec<&String> = o.keys().collect();
                keys.sort();
                for k in keys {
                    if let Some(x) = o.get(k) {
                        let esc = k.replace('~', "~0").replace('/', "~1");
                        walk(x, &format!("{ptr}/{esc}"), out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, "", &mut out);
    out
}

/// True if `v` contains a `"NaN"`, `"inf"` or `"-inf"` string (the unit's inputs can
/// carry non-finite floats).
pub fn carries_non_finite(v: &Value) -> bool {
    match v {
        Value::String(s) => NON_FINITE.contains(&s.as_str()),
        Value::Array(a) => a.iter().any(carries_non_finite),
        Value::Object(o) => o.values().any(carries_non_finite),
        _ => false,
    }
}

/// Applies one mutation to `v` (a site kind is chosen uniformly among the kinds present,
/// then a site of that kind); returns its description, or `None` if `v` has no site.
pub fn mutate_once(v: &mut Value, rng: &mut Rng, non_finite: bool) -> Option<String> {
    let all = sites(v);
    let kinds: Vec<Leaf> = all
        .iter()
        .map(|s| s.0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let kind = *rng.choose(&kinds)?;
    let of_kind: Vec<&str> = all
        .iter()
        .filter(|s| s.0 == kind)
        .map(|s| s.1.as_str())
        .collect();
    let ptr = (*rng.choose(&of_kind)?).to_string();
    let lens: Vec<i64> = all
        .iter()
        .filter(|s| s.0 == Leaf::Array)
        .filter_map(|s| v.pointer(&s.1).and_then(Value::as_array))
        .map(|a| a.len() as i64)
        .collect();
    let site = v.pointer_mut(&ptr)?;
    let what = match kind {
        Leaf::Float => {
            let mut opts: Vec<Value> = FLOATS.iter().map(|&x| json!(x)).collect();
            if non_finite {
                opts.extend(NON_FINITE.iter().map(|&s| json!(s)));
            }
            // Skip the current value; bit comparison keeps `-0` distinct from `0`.
            let old_bits = site.as_f64().map(f64::to_bits);
            opts.retain(|o| match (o.as_f64(), old_bits) {
                (Some(a), Some(b)) => a.to_bits() != b,
                _ => o != &*site,
            });
            let new = rng.choose(&opts)?.clone();
            let what = format!("= {new}");
            *site = new;
            what
        }
        Leaf::Int => {
            let mut opts: Vec<i64> = INTS.to_vec();
            if let Some(&n) = rng.choose(&lens) {
                opts.push(n);
            }
            opts.retain(|&o| site.as_i64() != Some(o));
            let new = *rng.choose(&opts)?;
            *site = json!(new);
            format!("= {new}")
        }
        Leaf::Bool => {
            let new = !site.as_bool()?;
            *site = json!(new);
            format!("= {new}")
        }
        Leaf::Array => {
            let a = site.as_array_mut()?;
            let len = a.len() as u64;
            match rng.below(3) {
                0 => {
                    a.clear();
                    "emptied".to_string()
                }
                1 => {
                    let keep = rng.below(len) as usize;
                    a.truncate(keep);
                    format!("truncated to {keep}")
                }
                _ => {
                    let i = rng.below(len) as usize;
                    let x = a.get(i)?.clone();
                    a.insert(i, x);
                    format!("element {i} duplicated")
                }
            }
        }
    };
    Some(format!(
        "{} {what}",
        if ptr.is_empty() { "/" } else { ptr.as_str() }
    ))
}

/// One to three mutations of `inputs` (one in 60 % of cases, so most failures point at a
/// single change); returns the mutated inputs and the mutation descriptions.
pub fn mutate(inputs: &Value, rng: &mut Rng, non_finite: bool) -> (Value, Vec<String>) {
    let mut v = inputs.clone();
    let n = if rng.chance(0.6) { 1 } else { 2 + rng.below(2) };
    let notes = (0..n)
        .filter_map(|_| mutate_once(&mut v, rng, non_finite))
        .collect();
    (v, notes)
}

/// A mutated case and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzCase {
    /// The case (named `fuzzNNNNN`).
    pub case: Case,
    /// Name of the case it was mutated from.
    pub base: String,
    /// Seed ([`fuzz_seed`]).
    pub seed: u64,
    /// The mutations applied, e.g. `/knots truncated to 2`, `/degree = -1`.
    pub mutations: Vec<String>,
}

/// `n` fuzz cases of `unit` derived from `bases` (empty if `bases` is).
pub fn fuzz_cases(unit: &str, bases: &[Case], n: usize) -> Vec<FuzzCase> {
    let non_finite = bases.iter().any(|c| carries_non_finite(&c.inputs));
    (0..n)
        .filter_map(|i| {
            let seed = fuzz_seed(unit, i);
            let mut rng = Rng::new(seed);
            let base = rng.choose(bases)?;
            let (inputs, mutations) = mutate(&base.inputs, &mut rng, non_finite);
            Some(FuzzCase {
                case: Case {
                    unit: unit.to_string(),
                    name: format!("fuzz{i:05}"),
                    source: CaseSource::Inline,
                    inputs,
                },
                base: base.name.clone(),
                seed,
                mutations,
            })
        })
        .collect()
}

/// Classification of one fuzz case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The C++ runner returned a harness error (the FFI guard refused the input) or its
    /// fixture constructor rejected the case (Rust is not run).
    Rejected,
    /// The inputs no longer parse (the mutation broke the schema, not a guard probe).
    Malformed,
    /// Both sides ran and agree (or the Rust side is deferred).
    Ran,
    /// Both sides ran and disagree, or only the Rust side refused: a note, not a failure.
    Differs,
    /// A runner panicked (FAIL).
    Panic,
    /// The child was killed by a signal or exited abnormally (FAIL).
    Crash,
    /// The case exceeded [`CASE_TIMEOUT`] (FAIL).
    Timeout,
}

impl Verdict {
    /// Every verdict, in summary order.
    pub const ALL: [Verdict; 7] = [
        Self::Rejected,
        Self::Malformed,
        Self::Ran,
        Self::Differs,
        Self::Crash,
        Self::Panic,
        Self::Timeout,
    ];

    /// Lower-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rejected => "rejected",
            Self::Malformed => "malformed",
            Self::Ran => "ran",
            Self::Differs => "differs",
            Self::Panic => "panic",
            Self::Crash => "crash",
            Self::Timeout => "timeout",
        }
    }

    /// Inverse of [`Verdict::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Crash, panic or timeout.
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Panic | Self::Crash | Self::Timeout)
    }
}

/// True for the harness errors of [`Case::parse`] / [`Case::get`] (the inputs do not
/// match the runner's input type).
pub fn is_malformed(msg: &str) -> bool {
    msg.contains(": bad inputs: ") || msg.contains(": missing input `") || msg.contains(": input `")
}

/// What one side returned.
enum Side {
    Done(Output),
    Harness(String),
    Panic(String),
}

/// Message and location of the last panic in the child (set by its panic hook).
static LAST_PANIC: Mutex<String> = Mutex::new(String::new());

fn call(f: impl FnOnce() -> Result<Output, HarnessError>) -> Side {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(o)) => Side::Done(o),
        Ok(Err(e)) => Side::Harness(e.0),
        Err(_) => Side::Panic(
            LAST_PANIC
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        ),
    }
}

fn classify(cpp: Side, rust: Side, floors: &[AbsFloor]) -> (Verdict, String) {
    match (cpp, rust) {
        (Side::Panic(m), _) => (Verdict::Panic, format!("cpp runner: {m}")),
        // The native constructor refused the fixture: Rust must not run on it (`run_unit`).
        (Side::Done(c), _) if c.rejected.is_some() => {
            (Verdict::Rejected, c.rejected.unwrap_or_default())
        }
        (_, Side::Panic(m)) => (Verdict::Panic, format!("rust: {m}")),
        (Side::Harness(m), _) if is_malformed(&m) => (Verdict::Malformed, m),
        (Side::Harness(m), _) => (Verdict::Rejected, m),
        (Side::Done(_), Side::Harness(m)) => (Verdict::Differs, format!("rust: {m}")),
        (Side::Done(_), Side::Done(r)) if r.deferred.is_some() => (Verdict::Ran, String::new()),
        (Side::Done(c), Side::Done(r)) => {
            let d = compare(&c, &r, floors);
            match d.mismatches.first() {
                None => (Verdict::Ran, String::new()),
                Some(m) => (
                    Verdict::Differs,
                    format!("{}: cpp={} rust={}", m.field, m.cpp, m.rust),
                ),
            }
        }
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `--fuzz-exec <unit> <cases.json>`: runs each case (a JSON array of `{name, inputs}`)
/// through both sides of `unit` and prints `@fuzz start <i> cpp|rust` before each side
/// and `@fuzz done <i> <verdict> <detail>` after both. Exit `0`, or `2` on a setup error.
pub fn run_child(args: &[String]) -> ExitCode {
    let [unit_id, path] = args else {
        eprintln!("usage: difftest --fuzz-exec <unit> <cases.json>");
        return ExitCode::from(2);
    };
    let reg = crate::registry();
    let Some(unit) = reg.iter().find(|u| u.id() == unit_id.as_str()) else {
        eprintln!("difftest: no difftest runner registered for `{unit_id}`");
        return ExitCode::from(2);
    };
    let cases: Vec<Value> = match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("difftest: {path}: {e}");
            return ExitCode::from(2);
        }
    };
    std::panic::set_hook(Box::new(|info| {
        let p = info.payload();
        let msg = p
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic".into());
        let at = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        // `DIFFTEST_FUZZ_BACKTRACE=1`: also print the panic's backtrace to stderr, to
        // locate a panic inside the library (e.g. `capacity overflow` in `alloc`).
        if std::env::var_os("DIFFTEST_FUZZ_BACKTRACE").is_some() {
            eprintln!(
                "panic: {msg}{at}\n{}",
                std::backtrace::Backtrace::force_capture()
            );
        }
        *LAST_PANIC.lock().unwrap_or_else(PoisonError::into_inner) = format!("{msg}{at}");
    }));
    for (i, c) in cases.iter().enumerate() {
        let case = Case {
            unit: unit_id.clone(),
            name: c
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("fuzz")
                .to_string(),
            source: CaseSource::Inline,
            inputs: c.get("inputs").cloned().unwrap_or(Value::Null),
        };
        println!("{MARK}start {i} cpp");
        let cpp = call(|| unit.run_cpp(&case));
        // `DIFFTEST_FUZZ_TRACE=1`: report the C++ side's outcome before the Rust side runs,
        // so a Rust hang or abort can be told apart from a guard-rejected input.
        if std::env::var_os("DIFFTEST_FUZZ_TRACE").is_some() {
            let what = match &cpp {
                Side::Done(o) if o.rejected.is_some() => "rejected-fixture".to_string(),
                Side::Done(_) => "done".to_string(),
                Side::Harness(m) => format!("harness-error {}", one_line(m)),
                Side::Panic(m) => format!("panic {}", one_line(m)),
            };
            eprintln!("@cpp {i} {what}");
        }
        let rust = if matches!(&cpp, Side::Done(c) if c.rejected.is_some()) {
            // Rust never runs on a fixture the C++ constructor refused.
            Side::Harness(String::new())
        } else {
            println!("{MARK}start {i} rust");
            call(|| unit.run_rust(&case))
        };
        let (verdict, detail) = classify(cpp, rust, unit.abs_floors());
        println!("{MARK}done {i} {} {}", verdict.as_str(), one_line(&detail));
    }
    ExitCode::SUCCESS
}

/// How a child batch ended.
enum End {
    /// Every case finished.
    Normal,
    /// The child died or hung during the next unfinished case (`detail` names the side).
    Abnormal { verdict: Verdict, detail: String },
}

/// One child run: the verdicts of the cases it finished, in order, then how it ended.
struct Batch {
    done: Vec<(Verdict, String)>,
    end: End,
    stderr: String,
}

/// The child command: on Unix through `sh` to set the [`MEM_LIMIT_KIB`] address-space
/// limit first.
fn child_command(exe: &Path) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("/bin/sh");
        c.arg("-c")
            .arg(format!(
                "ulimit -v {MEM_LIMIT_KIB} 2>/dev/null; exec \"$0\" \"$@\""
            ))
            .arg(exe);
        c
    }
    #[cfg(not(unix))]
    {
        Command::new(exe)
    }
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Last `n` characters of `s` (on a char boundary).
fn tail(s: &str, n: usize) -> &str {
    let mut start = s.len().saturating_sub(n);
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s.get(start..).unwrap_or_default()
}

/// Runs `cases` in one child process.
fn run_batch(
    exe: &Path,
    unit: &str,
    cases: &[FuzzCase],
    scratch: &Path,
) -> Result<Batch, HarnessError> {
    let list: Vec<Value> = cases
        .iter()
        .map(|c| json!({"name": c.case.name, "inputs": c.case.inputs}))
        .collect();
    std::fs::write(scratch, Value::Array(list).to_string())
        .map_err(|e| HarnessError::new(format!("{}: {e}", scratch.display())))?;
    let mut child = child_command(exe)
        .arg("--fuzz-exec")
        .arg(unit)
        .arg(scratch)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HarnessError::new(format!("spawn {}: {e}", exe.display())))?;
    let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        kill(&mut child);
        return Err(HarnessError::new("child pipes missing"));
    };
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        let mut buf = Vec::new();
        while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                break;
            }
            buf.clear();
        }
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        tail(&String::from_utf8_lossy(&buf), 4000).to_string()
    });
    let mut done = Vec::new();
    let mut side = "";
    let mut deadline = Instant::now() + CASE_TIMEOUT;
    let end = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => {
                let Some(msg) = line.find(MARK).and_then(|p| line.get(p + MARK.len()..)) else {
                    continue;
                };
                let mut parts = msg.trim_end().splitn(4, ' ');
                match (parts.next(), parts.nth(1), parts.next()) {
                    (Some("start"), Some("cpp"), _) => side = "cpp",
                    (Some("start"), Some("rust"), _) => side = "rust",
                    (Some("done"), Some(v), detail) => {
                        let v = Verdict::parse(v).unwrap_or(Verdict::Crash);
                        done.push((v, detail.unwrap_or_default().to_string()));
                        deadline = Instant::now() + CASE_TIMEOUT;
                        side = "";
                    }
                    _ => {}
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                kill(&mut child);
                break End::Abnormal {
                    verdict: Verdict::Timeout,
                    detail: format!("{side}: no result after {} s", CASE_TIMEOUT.as_secs()),
                };
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child
                    .wait()
                    .map_err(|e| HarnessError::new(format!("wait: {e}")))?;
                if status.success() && done.len() == cases.len() {
                    break End::Normal;
                }
                if side.is_empty() && done.is_empty() {
                    let err = err_reader.join().unwrap_or_default();
                    return Err(HarnessError::new(format!(
                        "fuzz child failed to start ({}): {}",
                        crate::ssi::exit_detail(status),
                        one_line(&err)
                    )));
                }
                break End::Abnormal {
                    verdict: Verdict::Crash,
                    detail: format!("{side}: {}", crate::ssi::exit_detail(status)),
                };
            }
        }
    };
    let _ = reader.join();
    let stderr = err_reader.join().unwrap_or_default();
    Ok(Batch { done, end, stderr })
}

/// One failing fuzz case.
#[derive(Debug, Clone)]
pub struct Failure {
    /// Case name (`fuzzNNNNN`).
    pub case: String,
    /// Crash, panic or timeout.
    pub verdict: Verdict,
    /// Side and signal, or the panic message.
    pub detail: String,
    /// The reproduction file written, if any.
    pub file: Option<PathBuf>,
}

/// Result of fuzzing one unit.
#[derive(Debug, Clone, Default)]
pub struct FuzzResult {
    /// Unit id.
    pub unit: String,
    /// Verdict of each fuzz case, in case order.
    pub verdicts: Vec<Verdict>,
    /// The crash, panic and timeout cases.
    pub failures: Vec<Failure>,
    /// The `differs` cases (detail = first difference, `file` = reproduction), plus
    /// harness observations that are not failures (e.g. a timeout that did not reproduce).
    pub notes: Vec<Failure>,
    /// Failure to build or run the cases.
    pub harness_error: Option<String>,
    /// Reason the native reference is unsupported; no mutations or workers ran.
    pub skipped_platform: Option<String>,
}

impl FuzzResult {
    /// Number of cases with verdict `v`.
    pub fn count(&self, v: Verdict) -> usize {
        self.verdicts.iter().filter(|&&x| x == v).count()
    }

    /// At least one verdict, no failure, no harness error and no platform skip.
    pub fn pass(&self) -> bool {
        self.skipped_platform.is_none()
            && self.harness_error.is_none()
            && self.failures.is_empty()
            && !self.verdicts.is_empty()
    }
}

/// One-line summary, e.g. `FUZZ-PASS harness.selftest cases=300 rejected=81 malformed=40
/// ran=179 differs=2 crashes=0 panics=0 timeouts=0` (`ran` includes `differs`).
pub fn summary_line(r: &FuzzResult) -> String {
    if let Some(reason) = &r.skipped_platform {
        return format!("FUZZ-SKIP {} skipped(platform): {reason}", r.unit);
    }
    let mut s = format!(
        "{} {} cases={} rejected={} malformed={} ran={} differs={} crashes={} panics={} timeouts={}",
        if r.pass() { "FUZZ-PASS" } else { "FUZZ-FAIL" },
        r.unit,
        r.verdicts.len(),
        r.count(Verdict::Rejected),
        r.count(Verdict::Malformed),
        r.count(Verdict::Ran) + r.count(Verdict::Differs),
        r.count(Verdict::Differs),
        r.count(Verdict::Crash),
        r.count(Verdict::Panic),
        r.count(Verdict::Timeout),
    );
    if let Some(h) = &r.harness_error {
        let _ = write!(s, " harness_error={h:?}");
    }
    s
}

/// Options of [`run_fuzz`].
#[derive(Debug, Clone)]
pub struct FuzzOptions {
    /// Fuzz cases per unit.
    pub cases: usize,
    /// Base directory for reproduction files (`<out_dir>/<unit>/fuzzNNNNN.json`).
    pub out_dir: PathBuf,
    /// Only mutate base cases whose name contains this substring.
    pub filter: Option<String>,
}

/// Writes the reproduction of failure `f` (the cases `repro`, in order) in fixture format.
fn write_repro(
    dir: &Path,
    f: &Failure,
    repro: &[FuzzCase],
    stderr: &str,
) -> Result<PathBuf, HarnessError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let cases: Vec<Value> = repro
        .iter()
        .map(|c| {
            json!({
                "name": c.case.name,
                "inputs": c.case.inputs,
                "fuzz": {"base": c.base, "seed": c.seed, "mutations": c.mutations},
            })
        })
        .collect();
    let mut failure = json!({"case": f.case, "verdict": f.verdict.as_str(), "detail": f.detail});
    if let (Some(m), false) = (failure.as_object_mut(), stderr.trim().is_empty()) {
        m.insert("stderr_tail".into(), json!(tail(stderr, 2000)));
    }
    let doc = json!({"cases": cases, "fuzz_failure": failure});
    let path = dir.join(format!("{}.json", f.case));
    std::fs::write(&path, to_canonical_string(&doc))
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
    Ok(path)
}

/// Removes the `fuzzNNNNN.json` files (five or more digits) of an earlier run from `dir`.
fn clear_old(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let digits = name
            .strip_prefix("fuzz")
            .and_then(|n| n.strip_suffix(".json"));
        if digits.is_some_and(|d| d.len() >= 5 && d.bytes().all(|b| b.is_ascii_digit())) {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Fuzzes `unit` with `opts.cases` cases (see the module docs).
pub fn run_fuzz(unit: &dyn UnitDiff, ctx: &CaseContext, opts: &FuzzOptions) -> FuzzResult {
    let mut res = FuzzResult {
        unit: unit.id().to_string(),
        skipped_platform: unit
            .reference_platform()
            .and_then(|p| p.skip_reason(std::env::consts::ARCH))
            .map(str::to_string),
        ..FuzzResult::default()
    };
    if res.skipped_platform.is_some() {
        return res;
    }
    if let Err(e) = fuzz_unit(unit, ctx, opts, &mut res) {
        res.harness_error = Some(e.0);
    }
    res
}

fn fuzz_unit(
    unit: &dyn UnitDiff,
    ctx: &CaseContext,
    opts: &FuzzOptions,
    res: &mut FuzzResult,
) -> Result<(), HarnessError> {
    let mut bases = unit.cases(ctx)?;
    if let Some(f) = &opts.filter {
        bases.retain(|c| c.name.contains(f.as_str()));
    }
    if bases.is_empty() {
        return Err(HarnessError::new("no cases to mutate"));
    }
    let cases = fuzz_cases(unit.id(), &bases, opts.cases);
    let exe =
        std::env::current_exe().map_err(|e| HarnessError::new(format!("current_exe: {e}")))?;
    let dir = opts.out_dir.join(unit.id());
    clear_old(&dir);
    // The case list handed to each child lives in the fuzz output directory, not the
    // shared temp directory, so another local user cannot pre-create or symlink it.
    std::fs::create_dir_all(&dir)
        .map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let scratch = dir.join(format!(".cases-{}.json", std::process::id()));
    let run = run_all(unit.id(), &cases, &exe, &scratch, &dir, res);
    let _ = std::fs::remove_file(&scratch);
    run
}

/// Runs `cases` in batches, re-running the case a batch died on alone.
fn run_all(
    unit: &str,
    cases: &[FuzzCase],
    exe: &Path,
    scratch: &Path,
    dir: &Path,
    res: &mut FuzzResult,
) -> Result<(), HarnessError> {
    let mut start = 0;
    while let Some(batch) = cases.get(start..cases.len().min(start + BATCH)) {
        if batch.is_empty() {
            break;
        }
        let b = run_batch(exe, unit, batch, scratch)?;
        for ((v, detail), c) in b.done.iter().zip(batch) {
            res.verdicts.push(*v);
            if *v == Verdict::Differs {
                // A note, not a failure, but reproducible like one.
                let f = Failure {
                    case: c.case.name.clone(),
                    verdict: *v,
                    detail: detail.clone(),
                    file: None,
                };
                let mut note = f;
                note.file = Some(write_repro(dir, &note, std::slice::from_ref(c), "")?);
                res.notes.push(note);
            } else if v.is_failure() {
                let f = Failure {
                    case: c.case.name.clone(),
                    verdict: *v,
                    detail: detail.clone(),
                    file: None,
                };
                record(res, dir, f, std::slice::from_ref(c), "")?;
            }
        }
        let End::Abnormal { verdict, detail } = b.end else {
            start += batch.len();
            continue;
        };
        if b.done.len() >= batch.len() {
            // Every case reported before the child died (e.g. at process exit): no case is
            // to blame and the next batch's first case must not be.
            let last = batch
                .last()
                .map(|c| c.case.name.clone())
                .unwrap_or_default();
            res.notes.push(Failure {
                case: last,
                verdict,
                detail: format!("child ended abnormally after reporting every case: {detail}"),
                file: None,
            });
            start += batch.len();
            continue;
        }
        let k = start + b.done.len();
        let Some(culprit) = cases.get(k) else {
            break;
        };
        // Confirm the case alone; keep its batch predecessors if it only dies after them.
        // A timeout is always re-run: it may only have been machine load.
        let (solo, solo_err, alone) = if b.done.is_empty() && verdict != Verdict::Timeout {
            (Some((verdict, detail.clone())), b.stderr.clone(), None)
        } else {
            let s = run_batch(exe, unit, std::slice::from_ref(culprit), scratch)?;
            match s.end {
                End::Abnormal { verdict, detail } => (Some((verdict, detail)), s.stderr, None),
                End::Normal => (None, String::new(), s.done.first().cloned()),
            }
        };
        if let (Verdict::Timeout, None, Some((v, d))) = (verdict, &solo, &alone) {
            // Did not reproduce alone: load-dependent, so not a failure. The case counts
            // with the verdict it got alone.
            let only = std::slice::from_ref(culprit);
            let mut f = Failure {
                case: culprit.case.name.clone(),
                verdict: *v,
                detail: d.clone(),
                file: None,
            };
            res.verdicts.push(*v);
            if v.is_failure() {
                record(res, dir, f, only, "")?;
            } else {
                f.verdict = Verdict::Timeout;
                f.detail = format!(
                    "{detail}; finished alone ({}), so load-dependent, not counted",
                    v.as_str()
                );
                f.file = Some(write_repro(dir, &f, only, "")?);
                res.notes.push(f);
            }
            start = k + 1;
            continue;
        }
        let f = |verdict, detail: String, stderr: &str| Failure {
            case: culprit.case.name.clone(),
            verdict,
            detail: if verdict == Verdict::Crash {
                with_cause(&detail, stderr)
            } else {
                detail
            },
            file: None,
        };
        res.verdicts.push(match &solo {
            Some((v, _)) => *v,
            None => verdict,
        });
        if let Some((v, d)) = solo {
            let only = std::slice::from_ref(culprit);
            record(res, dir, f(v, d, &solo_err), only, &solo_err)?;
        } else {
            let d = format!("{detail} (only after the {} preceding cases)", b.done.len());
            let repro = cases.get(start..=k).unwrap_or_default();
            record(res, dir, f(verdict, d, &b.stderr), repro, &b.stderr)?;
        }
        start = k + 1;
    }
    Ok(())
}

/// `detail (<last line of stderr>)`, e.g. `cpp: killed by signal 6 (memory allocation of 8 bytes
/// failed)`; the line is cut to 160 characters.
fn with_cause(detail: &str, stderr: &str) -> String {
    let cause = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("note: "));
    match cause {
        Some(c) => format!("{detail} ({})", c.chars().take(160).collect::<String>()),
        None => detail.to_string(),
    }
}

fn record(
    res: &mut FuzzResult,
    dir: &Path,
    mut f: Failure,
    repro: &[FuzzCase],
    stderr: &str,
) -> Result<(), HarnessError> {
    f.file = Some(write_repro(dir, &f, repro, stderr)?);
    res.failures.push(f);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        json!({"op": "basis_eval", "knots": [0.0, 0.0, 0.5, 1.0, 1.0], "degree": 2,
               "u": 0.25, "closed": false, "pts": [[1.5, 2.5, 3.5]]})
    }

    fn case(name: &str, inputs: Value) -> Case {
        Case {
            unit: "fake.unit".into(),
            name: name.into(),
            source: CaseSource::Inline,
            inputs,
        }
    }

    #[test]
    fn sites_cover_every_leaf_kind_in_stable_order() {
        let s = sites(&base());
        let ptrs: Vec<&str> = s.iter().map(|x| x.1.as_str()).collect();
        assert_eq!(
            ptrs,
            [
                "/closed", "/degree", "/knots", "/knots/0", "/knots/1", "/knots/2", "/knots/3",
                "/knots/4", "/pts", "/pts/0", "/pts/0/0", "/pts/0/1", "/pts/0/2", "/u"
            ]
        );
        assert_eq!(s[0].0, Leaf::Bool);
        assert_eq!(s[1].0, Leaf::Int);
        assert_eq!(s[2].0, Leaf::Array);
        assert_eq!(s[3].0, Leaf::Float);
        // Strings are never mutated, except the non-finite float convention.
        assert_eq!(
            sites(&json!({"op": "x", "f": "NaN"})),
            [(Leaf::Float, "/f".to_string())]
        );
        assert!(sites(&json!({"a/b": [], "c": null})).is_empty());
    }

    #[test]
    fn mutations_are_deterministic() {
        let bases = vec![case("a", base()), case("b", json!({"x": 1.0, "n": [1, 2]}))];
        let a = fuzz_cases("fake.unit", &bases, 50);
        let b = fuzz_cases("fake.unit", &bases, 50);
        assert_eq!(a, b);
        assert_eq!(a.len(), 50);
        assert_eq!(a[7].case.name, "fuzz00007");
        assert_eq!(a[7].seed, fuzz_seed("fake.unit", 7));
        assert_ne!(fuzz_cases("other.unit", &bases, 50), a);
        assert!(a.iter().all(|c| !c.mutations.is_empty()));
        assert!(a.iter().any(|c| c.base == "a") && a.iter().any(|c| c.base == "b"));
        // A prefix of a longer run is the shorter run.
        assert_eq!(fuzz_cases("fake.unit", &bases, 20), a[..20]);
        assert!(fuzz_cases("fake.unit", &[], 5).is_empty());
    }

    #[test]
    fn mutations_reach_every_kind_and_value() {
        let bases = [case("a", base())];
        let cases = fuzz_cases("fake.unit", &bases, 2000);
        let all: Vec<&String> = cases.iter().flat_map(|c| &c.mutations).collect();
        let has = |s: &str| all.iter().any(|m| m.contains(s));
        let floats = FLOATS.map(|x| format!("/u = {}", json!(x)));
        let ints = INTS.map(|x| format!("/degree = {x}"));
        for want in floats.iter().chain(&ints).map(String::as_str).chain([
            "/closed = true",
            "/knots emptied",
            "/knots truncated to",
            "/knots element",
            "/pts/0 emptied",
        ]) {
            assert!(has(want), "no mutation `{want}` in 2000 cases");
        }
        // The length of an input array (just past its last index).
        assert!(has("/degree = 5"));
        // No non-finite strings unless the inputs carry them.
        assert!(!has("NaN") && !has("inf"));
        let nf = [case("n", json!({"x": "inf", "y": 2.5}))];
        let all: Vec<String> = fuzz_cases("fake.unit", &nf, 200)
            .into_iter()
            .flat_map(|c| c.mutations)
            .collect();
        for want in ["/y = \"NaN\"", "= \"inf\"", "/x = \"-inf\"", "/x = 0.0"] {
            assert!(all.iter().any(|m| m.contains(want)), "{want}");
        }
    }

    #[test]
    fn mutation_changes_the_named_site() {
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let mut v = base();
            let what = mutate_once(&mut v, &mut rng, true).unwrap();
            let ptr = what.split(' ').next().unwrap();
            let ptr = if ptr == "/" { "" } else { ptr };
            let text = |x: &Value| x.pointer(ptr).map(Value::to_string);
            assert_ne!(text(&v), text(&base()), "{what}");
        }
        assert_eq!(mutate_once(&mut json!({"s": "x"}), &mut rng, false), None);
    }

    #[test]
    fn classification() {
        let ok = || Side::Done(Output::new().scalar("x", 1.0));
        let v = |c, r| classify(c, r, &[]).0;
        assert_eq!(v(ok(), ok()), Verdict::Ran);
        assert_eq!(
            v(ok(), Side::Done(Output::new().scalar("x", 2.0))),
            Verdict::Differs
        );
        assert_eq!(
            v(Side::Harness("ffi rejected: n".into()), ok()),
            Verdict::Rejected
        );
        assert_eq!(
            v(
                Side::Harness("case fuzz00001: bad inputs: invalid type".into()),
                ok()
            ),
            Verdict::Malformed
        );
        // A fixture the C++ constructor refused is `rejected`, whatever Rust would have said.
        assert_eq!(
            v(
                Side::Done(Output::rejected("ctor")),
                Side::Harness(String::new())
            ),
            Verdict::Rejected
        );
        assert_eq!(
            v(
                Side::Done(Output::rejected("ctor")),
                Side::Done(Output::new())
            ),
            Verdict::Rejected
        );
        assert_eq!(
            v(Side::Harness("x".into()), Side::Panic("p".into())),
            Verdict::Panic
        );
        assert_eq!(v(Side::Panic("p".into()), ok()), Verdict::Panic);
        assert_eq!(
            v(
                ok(),
                Side::Done(Output::deferred("geom.curve", "SmCurve::Evaluate"))
            ),
            Verdict::Ran
        );
        assert_eq!(
            with_cause(
                "cpp: killed by signal 6",
                "x\nmemory allocation failed\nnote: run with\n"
            ),
            "cpp: killed by signal 6 (memory allocation failed)"
        );
        assert_eq!(
            with_cause("rust: killed by signal 11", " \n"),
            "rust: killed by signal 11"
        );
        for x in Verdict::ALL {
            assert_eq!(Verdict::parse(x.as_str()), Some(x));
        }
        let r = FuzzResult {
            unit: "u".into(),
            verdicts: vec![Verdict::Ran, Verdict::Differs, Verdict::Rejected],
            ..FuzzResult::default()
        };
        assert_eq!(
            summary_line(&r),
            "FUZZ-PASS u cases=3 rejected=1 malformed=0 ran=2 differs=1 crashes=0 panics=0 timeouts=0"
        );
    }
}
