//! `--ssi-corpus`: scores the C++ reference, the Rust port, the `ssi` crate and the `ssi`
//! crate through `solidcore-ssi` against jyviko's exact ground-truth surface/surface
//! intersection corpus
//! (<https://github.com/jyviko/ssi-corpus>, commit `b04c8a6`, MIT OR Apache-2.0; fetched
//! into `corpus/ssi/` by `scripts/fetch-ssi-corpus.sh`, never committed).
//!
//! Every case runs through each engine in a child process (`--ssi-case`), so a C++
//! crash or hang is a `CRASH` verdict instead of the end of the run, as with the
//! corpus' own runner. Every output is graded by the port of the corpus scorer
//! ([`score`]); where the C++ and the port both answered, their outputs are also
//! compared with the difftest rule (topology identical, values within
//! [`crate::REL_TOL`]). The `ssi` engine ([`ssi_engine`]) is a method of its own, never
//! compared for parity; the `solidcore-ssi` engine ([`solidcore_ssi_engine`]) runs it
//! from solidcore's own surfaces into the port's output form, sampled as the port's is
//! ([`kernel`]). Those two are the engines a build without the `cpp` feature runs. This
//! is a correctness report: it never changes a unit's difftest status.

pub mod case;
#[cfg(feature = "cpp")]
pub mod engine;
pub mod geom;
pub mod kernel;
pub mod polyset;
pub mod raw;
pub mod score;
pub mod solidcore_ssi_engine;
pub mod ssi_engine;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use self::case::SsiCase;
use self::raw::RawOutput;
use self::score::{score_case, status_names, Score, VERDICTS};
use crate::{compare, HarnessError};

/// The corpus repository.
pub const CORPUS_REPO: &str = "https://github.com/jyviko/ssi-corpus";
/// The pinned corpus commit (`b04c8a6`; full SHA, as `scripts/fetch-ssi-corpus.sh` pins it).
pub const CORPUS_COMMIT: &str = "b04c8a6093a8b3505d865d1dd874ec3879ccb1f9";
/// Per-engine time limit for one case (the corpus scorer's `TIMEOUT_S`).
pub const TIMEOUT: Duration = Duration::from_secs(300);
/// The default approximation tolerance.
pub const DEFAULT_TOL: &str = "1e-6";

/// How many engines there are.
pub const N_ENGINES: usize = 4;

/// The engines, in report order: upstream SMLib (C++), the Rust port, the `ssi` crate,
/// and the `ssi` crate through `solidcore-ssi` on solidcore's own surfaces.
pub const ENGINES: [&str; N_ENGINES] = ["cpp", "rust", "ssi", "solidcore-ssi"];

/// Whether this build runs each of [`ENGINES`]: `cpp` and `rust` need the C++ kernel
/// (feature `cpp`), `ssi` and `solidcore-ssi` nothing.
pub const BUILT: [bool; N_ENGINES] = [cfg!(feature = "cpp"), cfg!(feature = "cpp"), true, true];

/// Whether the C++ and the port both run, so their outputs are compared for parity.
const PARITY: bool = BUILT[0] && BUILT[1];

/// The indices of the engines this build runs.
fn built() -> impl DoubleEndedIterator<Item = usize> {
    BUILT
        .into_iter()
        .enumerate()
        .filter_map(|(e, b)| b.then_some(e))
}

/// Why an engine produced no [`RawOutput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineFailure {
    /// A surface could not be built (the corpus runner's exit code 3).
    Build(String),
    /// Anything else (bad input, C++ exception): a crash for scoring.
    Other(String),
}

/// Options of a corpus run.
#[derive(Debug, Clone)]
pub struct CorpusOptions {
    /// Approximation tolerance as given (`1e-6`, or `default` for upstream's default).
    pub tol: String,
    /// Case id glob (`*` and `?`).
    pub glob: Option<String>,
    /// Child processes run at once.
    pub jobs: usize,
    /// Report directory (default `reports/`).
    pub report_dir: Option<PathBuf>,
    /// Do not write the report.
    pub no_report: bool,
}

/// The corpus checkout.
pub fn corpus_dir(root: &Path) -> PathBuf {
    root.join("corpus").join("ssi")
}

/// `fnmatch`-style match of `*` and `?`.
pub fn glob_match(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = s.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(&c) if c == '?' || Some(&c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.iter().skip(pi).all(|&c| c == '*')
}

/// Parses `--tol`: `default` or a positive number.
pub fn parse_tol(s: &str) -> Result<Option<f64>, String> {
    if s == "default" {
        return Ok(None);
    }
    match s.parse::<f64>() {
        Ok(t) if t.is_finite() && t > 0.0 => Ok(Some(t)),
        _ => Err(format!("bad --tol `{s}` (a positive number or `default`)")),
    }
}

/// The case files of the corpus matching `glob`, sorted by id.
pub fn case_files(root: &Path, glob: Option<&str>) -> Result<Vec<PathBuf>, HarnessError> {
    let dir = corpus_dir(root).join("cases").join("data");
    let rd = std::fs::read_dir(&dir)
        .map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .filter(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            glob.is_none_or(|g| glob_match(g, stem))
        })
        .collect();
    v.sort();
    Ok(v)
}

/// `--ssi-case <case.json> <tol|default> <cpp|rust|ssi|solidcore-ssi> <out.json>`: one engine on one
/// case, writing the corpus runner's JSON. Exit `0` on success (including a deferred Rust
/// run and a non-success status), `3` when a surface cannot be built, `2` otherwise.
/// `cpp` and `rust` need the `cpp` feature.
pub fn run_child(args: &[String]) -> ExitCode {
    let [case_path, tol, engine_name, out_path] = args else {
        eprintln!("usage: difftest --ssi-case <case.json> <tol|default> <cpp|rust|ssi|solidcore-ssi> <out.json>");
        return ExitCode::from(2);
    };
    let tol = match parse_tol(tol) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };
    let case = match SsiCase::load(Path::new(case_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("case parse error: {e}");
            return ExitCode::from(2);
        }
    };
    let res = match engine_name.as_str() {
        #[cfg(feature = "cpp")]
        "cpp" => engine::run_cpp(&case, tol),
        #[cfg(feature = "cpp")]
        "rust" => engine::run_rust(&case, tol),
        "ssi" => ssi_engine::run_ssi(&case, tol),
        "solidcore-ssi" => solidcore_ssi_engine::run_solidcore_ssi(&case, tol),
        other if ENGINES.contains(&other) => {
            eprintln!("difftest: engine `{other}` needs feature `cpp`");
            return ExitCode::from(2);
        }
        other => {
            eprintln!("difftest: unknown engine `{other}`");
            return ExitCode::from(2);
        }
    };
    match res {
        Ok(out) => {
            let text = serde_json::to_string(&out.to_json()).unwrap_or_default();
            match std::fs::write(out_path, text + "\n") {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("difftest: {out_path}: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Err(EngineFailure::Build(e)) => {
            eprintln!("surface build error: {e}");
            ExitCode::from(3)
        }
        Err(EngineFailure::Other(e)) => {
            eprintln!("engine error: {e}");
            ExitCode::from(2)
        }
    }
}

/// One finished engine run: its output, or the crash detail (`score.py`'s wording).
type RunResult = Result<RawOutput, String>;

struct Job {
    case: usize,
    engine: usize,
    out: PathBuf,
    child: Child,
    start: Instant,
}

/// `killed by signal N` or `runner exit code N`.
pub(crate) fn exit_detail(status: std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return format!("killed by signal {sig}");
        }
    }
    format!("runner exit code {}", status.code().unwrap_or(-1))
}

fn read_raw(path: &Path) -> RunResult {
    let text = std::fs::read_to_string(path).map_err(|e| format!("unreadable output: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("unreadable output: {e}"))?;
    RawOutput::from_json(&v).map_err(|e| format!("unreadable output: {e}"))
}

/// Runs every (case, engine this build runs) in a child process, `jobs` at a time.
fn run_children(
    files: &[PathBuf],
    tol: &str,
    rawdirs: &[PathBuf; N_ENGINES],
    jobs: usize,
) -> Result<Vec<[RunResult; N_ENGINES]>, HarnessError> {
    let exe =
        std::env::current_exe().map_err(|e| HarnessError::new(format!("current_exe: {e}")))?;
    let mut results: Vec<[RunResult; N_ENGINES]> = files
        .iter()
        .map(|_| std::array::from_fn(|_| Err("not run".to_string())))
        .collect();
    let mut queue: Vec<(usize, usize)> = (0..files.len())
        .flat_map(|c| built().map(move |e| (c, e)))
        .rev()
        .collect();
    let mut running: Vec<Job> = Vec::new();
    while !queue.is_empty() || !running.is_empty() {
        while running.len() < jobs.max(1) {
            let Some((c, e)) = queue.pop() else { break };
            let (Some(file), Some(dir), Some(engine)) =
                (files.get(c), rawdirs.get(e), ENGINES.get(e))
            else {
                continue;
            };
            let id = file.file_stem().and_then(|s| s.to_str()).unwrap_or("case");
            let out = dir.join(format!("{id}.json"));
            let log = dir.join(format!("{id}.log"));
            let _ = std::fs::remove_file(&out);
            let log_file = std::fs::File::create(&log)
                .map_err(|err| HarnessError::new(format!("{}: {err}", log.display())))?;
            let log_err = log_file
                .try_clone()
                .map_err(|err| HarnessError::new(format!("{}: {err}", log.display())))?;
            let child = Command::new(&exe)
                .arg("--ssi-case")
                .arg(file)
                .arg(tol)
                .arg(engine)
                .arg(&out)
                .stdin(Stdio::null())
                .stdout(log_file)
                .stderr(log_err)
                .spawn()
                .map_err(|err| HarnessError::new(format!("spawn {}: {err}", exe.display())))?;
            running.push(Job {
                case: c,
                engine: e,
                out,
                child,
                start: Instant::now(),
            });
        }
        let mut i = 0;
        let mut progressed = false;
        while i < running.len() {
            let done = match running.get_mut(i) {
                Some(job) => match job.child.try_wait() {
                    Ok(Some(st)) => Some(if st.success() {
                        read_raw(&job.out)
                    } else {
                        Err(exit_detail(st))
                    }),
                    Ok(None) if job.start.elapsed() > TIMEOUT => {
                        let _ = job.child.kill();
                        let _ = job.child.wait();
                        Some(Err(format!("timeout after {} s", TIMEOUT.as_secs())))
                    }
                    Ok(None) => None,
                    Err(err) => Some(Err(format!("wait failed: {err}"))),
                },
                None => None,
            };
            if let Some(r) = done {
                let job = running.swap_remove(i);
                if let Some(slot) = results
                    .get_mut(job.case)
                    .and_then(|s| s.get_mut(job.engine))
                {
                    *slot = r;
                }
                progressed = true;
            } else {
                i += 1;
            }
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(results)
}

/// The report entry of one engine's grade.
fn engine_entry(r: &RunResult, s: Option<&Score>) -> Value {
    if let Ok(RawOutput {
        deferred: Some(d), ..
    }) = r
    {
        return json!({"verdict": "DEFERRED", "deferred": {"unit": d.unit, "symbol": d.symbol}});
    }
    s.map_or_else(|| json!({"verdict": "?"}), Score::to_json)
}

fn verdict_of(r: &RunResult, s: Option<&Score>) -> String {
    match (r, s) {
        (Ok(o), _) if o.deferred.is_some() => "DEFERRED".into(),
        (_, Some(s)) => s.verdict.into(),
        _ => "?".into(),
    }
}

fn summary(engine: &str, counts: &BTreeMap<String, usize>) -> String {
    let mut parts = Vec::new();
    for v in VERDICTS.iter().copied().chain(["DEFERRED"]) {
        let n = counts.get(v).copied().unwrap_or(0);
        if n > 0 || v == "PASS" {
            parts.push(format!("{}={n}", v.replace(' ', "_")));
        }
    }
    let total: usize = counts.values().sum();
    format!("SSI-CORPUS {engine}: {} (cases={total})", parts.join(" "))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "..."
    }
}

/// Grades every engine output (`None` for a deferred one or an engine this build does
/// not run), `jobs` cases at a time; the result is in case order whatever the thread
/// timing.
fn score_all(
    cases: &[SsiCase],
    results: &[[RunResult; N_ENGINES]],
    acc_tol: Option<f64>,
    cov_floor: f64,
    names: &BTreeMap<i64, String>,
    jobs: usize,
) -> Vec<Vec<Option<Score>>> {
    let grade = |case: &SsiCase, res: &[RunResult; N_ENGINES]| -> Vec<Option<Score>> {
        res.iter()
            .zip(BUILT)
            .map(|(r, run)| match r {
                _ if !run => None,
                Ok(o) if o.deferred.is_some() => None,
                Ok(o) => Some(score_case(case, Ok(o), acc_tol, cov_floor, names)),
                Err(d) => Some(score_case(case, Err(d), acc_tol, cov_floor, names)),
            })
            .collect()
    };
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(usize, Vec<Option<Score>>)> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..jobs.max(1))
            .map(|_| {
                sc.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let (Some(case), Some(res)) = (cases.get(i), results.get(i)) else {
                            break;
                        };
                        mine.push((i, grade(case, res)));
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    out.sort_by_key(|(i, _)| *i);
    // a case whose grading thread panicked keeps no grade ("?")
    let mut graded: Vec<Vec<Option<Score>>> = cases
        .iter()
        .map(|_| ENGINES.iter().map(|_| None).collect())
        .collect();
    for (i, s) in out {
        if let Some(slot) = graded.get_mut(i) {
            *slot = s;
        }
    }
    graded
}

/// `--ssi-corpus`: runs and scores the corpus. Exit `0` (also when the corpus is missing)
/// unless the harness itself fails (`2`): the verdicts are a report, not a gate.
pub fn run_corpus(root: &Path, opts: &CorpusOptions) -> ExitCode {
    let dir = corpus_dir(root);
    if !dir.join("cases").join("data").is_dir() {
        println!(
            "SSI-CORPUS: corpus not found at {}; fetch it with scripts/fetch-ssi-corpus.sh \
             ({CORPUS_REPO} at {CORPUS_COMMIT}, MIT OR Apache-2.0)",
            dir.display()
        );
        return ExitCode::SUCCESS;
    }
    let tol = match parse_tol(&opts.tol) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };
    let files = match case_files(root, opts.glob.as_deref()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };
    if files.is_empty() {
        println!(
            "SSI-CORPUS: no case matches `{}`",
            opts.glob.as_deref().unwrap_or("*")
        );
        return ExitCode::SUCCESS;
    }
    let mut cases = Vec::with_capacity(files.len());
    for f in &files {
        match SsiCase::load(f) {
            Ok(c) => cases.push(c),
            Err(e) => {
                eprintln!("difftest: {e}");
                return ExitCode::from(2);
            }
        }
    }
    let raw_root = root.join("bin").join("ssi-corpus").join("raw");
    let rawdirs = ENGINES.map(|e| raw_root.join(format!("{e}_{}", opts.tol)));
    for (d, _) in rawdirs.iter().zip(BUILT).filter(|(_, run)| *run) {
        if let Err(e) = std::fs::create_dir_all(d) {
            eprintln!("difftest: {}: {e}", d.display());
            return ExitCode::from(2);
        }
    }
    let names_run: Vec<&str> = ENGINES
        .iter()
        .zip(BUILT)
        .filter_map(|(e, run)| run.then_some(*e))
        .collect();
    println!(
        "SSI-CORPUS: {} case(s), approx tol {}, engines {} ({} job(s)); raw output in {}",
        files.len(),
        opts.tol,
        names_run.join(", "),
        opts.jobs,
        raw_root.strip_prefix(root).unwrap_or(&raw_root).display()
    );
    let results = match run_children(&files, &opts.tol, &rawdirs, opts.jobs) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };

    let names = status_names(root);
    let acc_tol = tol.map(|t| 10.0 * t);
    let cov_floor = if tol.is_none() { 5e-3 } else { 0.0 };
    let mut counts: [BTreeMap<String, usize>; N_ENGINES] = Default::default();
    let (mut compared, mut identical, mut same_verdict) = (0usize, 0usize, 0usize);
    let mut entries = Vec::new();
    let all_scores = score_all(&cases, &results, acc_tol, cov_floor, &names, opts.jobs);
    for ((case, res), scores) in cases.iter().zip(&results).zip(&all_scores) {
        let mut verdicts = Vec::new();
        let mut entry = serde_json::Map::new();
        entry.insert("id".into(), json!(case.id));
        entry.insert("class".into(), json!(case.class));
        entry.insert(
            "surfaces".into(),
            json!([case.surfaces[0].0.kind(), case.surfaces[1].0.kind()]),
        );
        for (k, ((r, s), run)) in res.iter().zip(scores).zip(BUILT).enumerate() {
            if !run {
                verdicts.push("-".to_string());
                continue;
            }
            let v = verdict_of(r, s.as_ref());
            if let Some(c) = counts.get_mut(k) {
                *c.entry(v.clone()).or_default() += 1;
            }
            entry.insert(
                ENGINES.get(k).copied().unwrap_or("?").into(),
                engine_entry(r, s.as_ref()),
            );
            verdicts.push(v);
        }
        // Rust vs C++ parity where both answered
        let mut parity = "-".to_string();
        if let [Ok(c), Ok(r), ..] = res {
            if r.deferred.is_none() {
                compared += 1;
                let d = compare(&c.parity_output(), &r.parity_output(), &[]);
                let first = d
                    .mismatches
                    .first()
                    .map(|m| format!("{}: cpp={} rust={}", m.field, m.cpp, m.rust));
                if d.pass() {
                    identical += 1;
                    parity = "identical".into();
                } else {
                    parity = format!("MISMATCH({})", d.mismatch_count);
                }
                entry.insert(
                    "parity".into(),
                    json!({"pass": d.pass(), "mismatches": d.mismatch_count,
                           "max_rel_err": crate::json::num(d.max_rel_err), "first": first}),
                );
            }
        }
        if let [a, b, _] = verdicts.as_slice() {
            if PARITY && b != "DEFERRED" && a == b {
                same_verdict += 1;
            }
        }
        // the flags of the first engine run (the C++ when it runs)
        let flags = scores
            .iter()
            .zip(BUILT)
            .filter(|(_, run)| *run)
            .find_map(|(s, _)| s.as_ref())
            .map(|s| s.detail.clone().unwrap_or_else(|| s.flags.join("; ")))
            .unwrap_or_default();
        let mut columns = String::new();
        for ((e, v), _) in ENGINES
            .iter()
            .zip(&verdicts)
            .zip(BUILT)
            .filter(|(_, run)| *run)
        {
            let _ = write!(columns, " {e} {v:<14}");
        }
        let parity_column = if PARITY {
            format!(" parity {parity:<13}")
        } else {
            String::new()
        };
        println!(
            "{:<36}{columns}{parity_column} {}",
            case.id,
            truncate(&flags, 160)
        );
        entries.push(Value::Object(entry));
    }
    let mut summaries = serde_json::Map::new();
    for ((e, c), _) in ENGINES
        .iter()
        .zip(&counts)
        .zip(BUILT)
        .filter(|(_, run)| *run)
    {
        println!("{}", summary(e, c));
        summaries.insert((*e).into(), json!(c));
    }
    if PARITY {
        println!(
            "SSI-CORPUS parity: compared={compared} identical={identical} mismatched={} \
             same_verdict={same_verdict}",
            compared - identical
        );
        summaries.insert(
            "parity".into(),
            json!({"compared": compared, "identical": identical,
                   "mismatched": compared - identical, "same_verdict": same_verdict}),
        );
    }

    if !opts.no_report {
        let report = json!({
            "corpus": {"repo": CORPUS_REPO, "commit": CORPUS_COMMIT, "glob": opts.glob},
            "approx_tol": opts.tol,
            "acc_tol": acc_tol.map(crate::json::num),
            "samples_per_curve": raw::N_SAMPLES,
            "summary": summaries,
            "cases": entries,
        });
        let dir = opts
            .report_dir
            .clone()
            .unwrap_or_else(|| root.join("reports"));
        let mut name = "ssi-corpus".to_string();
        // a build without some engines names the ones it ran, so it never overwrites a
        // full report
        if !PARITY {
            for e in &names_run {
                name.push('_');
                name.push_str(e);
            }
        }
        if opts.tol != DEFAULT_TOL {
            name.push('_');
            name.push_str(&opts.tol);
        }
        if let Some(g) = &opts.glob {
            name.push('_');
            name.extend(g.chars().filter(|c| c.is_alphanumeric() || *c == '_'));
        }
        let path = dir.join(format!("{name}.json"));
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, crate::json::to_canonical_string(&report)));
        match written {
            Ok(()) => println!(
                "  report: {}",
                path.strip_prefix(root).unwrap_or(&path).display()
            ),
            Err(e) => {
                eprintln!("difftest: {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("*", "c1_plane"));
        assert!(glob_match("c1_*", "c1_plane"));
        assert!(!glob_match("c2_*", "c1_plane"));
        assert!(glob_match("c?_*cyl*", "c1_plane_cyl_ellipse"));
        assert!(glob_match("c1_plane", "c1_plane"));
        assert!(!glob_match("c1_plan", "c1_plane"));
    }

    #[test]
    fn tolerances() {
        assert_eq!(parse_tol("default"), Ok(None));
        assert_eq!(parse_tol("1e-6"), Ok(Some(1e-6)));
        assert!(parse_tol("-1").is_err());
    }
}
