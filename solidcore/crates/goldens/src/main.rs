//! `goldens` CLI. See `crates/goldens/README.md`.
//!
//! ```text
//! cargo run -p goldens -- --all --goldens [--reverse] [--goldens-dir <dir>]   # check every
//!          unit against its golden (`goldens::golden`)
//! cargo run -p goldens -- --unit <id> [--unit <id>...] --goldens   # some units (globs allowed)
//! cargo run -p goldens -- --unit <id> --bless   # rewrite the unit's golden from this run
//! cargo run -p goldens -- --list
//! cargo run -p goldens -- --all --digest-dir <dir> [--digest-debug]   # one `<dir>/<unit>.digest`
//!          per unit (native vs wasm32: scripts/wasm-digest.sh)
//! options: --seeds N (default 16)  --filter <substr>
//!          --jobs N (-j; default 1, 0 = all cores)   # units in N child processes (`goldens::jobs`)
//!          --report-dir <dir>  --no-report   # where `--jobs` keeps its unit times
//! cargo run -p goldens -- --ssi-corpus [--tol 1e-6|default] [--jobs N] [glob]
//! cargo run -p goldens -- --solidcraft-cases [glob]
//! cargo run -p goldens -- --step-corpus [--constructor] [--jobs N] [glob]
//! ```
//! Exit status: 0 no failures, 1 any failure, 2 usage / harness setup error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use goldens::case::repo_root;
use goldens::{registry, CaseContext, RunOptions};

const USAGE: &str = "usage: goldens [--unit <id>]... [--all] (--goldens [--bless] [--reverse] [--goldens-dir <dir>] | --digest-dir <dir> [--digest-debug])
       [--seeds N] [--filter <substr>] [--jobs N] [--report-dir <dir>] [--no-report]
       goldens --list [--seeds N]
       goldens --ssi-corpus [--tol 1e-6|default] [--jobs N] [--report-dir <dir>] [--no-report] [case-glob]
       goldens --solidcraft-cases [--report-dir <dir>] [--no-report] [case-glob]
       goldens --step-corpus [--constructor] [--jobs N] [--report-dir <dir>] [--no-report] [file-glob]";

/// Default child processes of `--ssi-corpus` and `--step-corpus`.
const DEFAULT_CORPUS_JOBS: usize = 2;

/// Default number of generated cases per unit.
const DEFAULT_SEEDS: usize = 16;

// CLI flags are naturally independent booleans.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct Args {
    units: Vec<String>,
    all: bool,
    list: bool,
    seeds: Option<usize>,
    filter: Option<String>,
    report_dir: Option<PathBuf>,
    no_report: bool,
    ssi_corpus: bool,
    solidcraft_cases: bool,
    step_corpus: bool,
    constructor: bool,
    tol: Option<String>,
    jobs: Option<usize>,
    glob: Option<String>,
    digest_dir: Option<PathBuf>,
    digest_debug: bool,
    /// `--goldens`: check each unit against its golden.
    goldens: bool,
    /// `--bless`: rewrite each unit's golden from this run.
    bless: bool,
    /// `--goldens-dir`: where the goldens live (default `crates/goldens/expected`).
    goldens_dir: Option<PathBuf>,
    /// `--reverse`: run each unit's cases in reverse order.
    reverse: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--unit" | "-u" => a.units.push(val("--unit")?),
            "--all" => a.all = true,
            "--list" => a.list = true,
            "--seeds" => {
                let v = val("--seeds")?;
                a.seeds = Some(v.parse().map_err(|_| format!("bad --seeds `{v}`"))?);
            }
            "--filter" => a.filter = Some(val("--filter")?),
            "--report-dir" => a.report_dir = Some(PathBuf::from(val("--report-dir")?)),
            "--no-report" => a.no_report = true,
            "--ssi-corpus" => a.ssi_corpus = true,
            "--solidcraft-cases" => a.solidcraft_cases = true,
            "--step-corpus" => a.step_corpus = true,
            "--constructor" => a.constructor = true,
            "--tol" => a.tol = Some(val("--tol")?),
            "--jobs" | "-j" => {
                let v = val("--jobs")?;
                a.jobs = Some(v.parse().map_err(|_| format!("bad --jobs `{v}`"))?);
            }
            "--digest-dir" => a.digest_dir = Some(PathBuf::from(val("--digest-dir")?)),
            "--digest-debug" => a.digest_debug = true,
            "--goldens" => a.goldens = true,
            "--bless" => {
                a.goldens = true;
                a.bless = true;
            }
            "--goldens-dir" => a.goldens_dir = Some(PathBuf::from(val("--goldens-dir")?)),
            "--reverse" => a.reverse = true,
            "-h" | "--help" => return Err(String::new()),
            other
                if (a.ssi_corpus || a.solidcraft_cases || a.step_corpus)
                    && !other.starts_with('-')
                    && a.glob.is_none() =>
            {
                a.glob = Some(other.to_string());
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(a)
}

fn usage_error(e: &str) -> ExitCode {
    if !e.is_empty() {
        eprintln!("goldens: {e}");
    }
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // Child of `--ssi-corpus`: one engine on one case (see `goldens::ssi::run_child`).
    if raw.first().map(String::as_str) == Some("--ssi-case") {
        return goldens::ssi::run_child(raw.get(1..).unwrap_or(&[]));
    }
    // Child of `--step-corpus`: one body (see `goldens::step::run_child`).
    if raw.first().map(String::as_str) == Some("--step-case") {
        return goldens::step::run_child(raw.get(1..).unwrap_or(&[]));
    }
    // Child of unit `corpus.parity`: one side on one model file
    // (see `goldens::units::corpus_parity::run_child`).
    if raw.first().map(String::as_str) == Some(goldens::units::corpus_parity::CHILD_FLAG) {
        return goldens::units::corpus_parity::run_child(raw.get(1..).unwrap_or(&[]));
    }
    // A `--jobs` child (the child modes above inherit its environment but are not one).
    if std::env::var_os(goldens::jobs::WORKER_ENV).is_some() {
        goldens::jobs::watch_parent();
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    let root = repo_root();

    if args.step_corpus {
        let opts = goldens::step::CorpusOptions {
            glob: args.glob.clone(),
            jobs: args.jobs.unwrap_or(DEFAULT_CORPUS_JOBS).max(1),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
            constructor: args.constructor,
        };
        return goldens::step::run_corpus(&root, &opts);
    }

    if args.solidcraft_cases {
        let opts = goldens::solidcraft::CasesOptions {
            glob: args.glob.clone(),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
        };
        return goldens::solidcraft::run_cases(&root, &opts);
    }

    if args.ssi_corpus {
        let opts = goldens::ssi::CorpusOptions {
            tol: args
                .tol
                .clone()
                .unwrap_or_else(|| goldens::ssi::DEFAULT_TOL.to_string()),
            glob: args.glob.clone(),
            jobs: args.jobs.unwrap_or(DEFAULT_CORPUS_JOBS).max(1),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
        };
        return goldens::ssi::run_corpus(&root, &opts);
    }

    if args.tol.is_some() {
        return usage_error("--tol only applies to --ssi-corpus");
    }
    let reg = registry();
    let ctx = CaseContext {
        root: root.clone(),
        seeds: args.seeds.unwrap_or(DEFAULT_SEEDS),
    };

    if args.list {
        for u in &reg {
            let n = u
                .cases(&ctx)
                .map_or_else(|e| format!("error: {e}"), |c| c.len().to_string());
            println!("{}\tcases={n}", u.id());
        }
        return ExitCode::SUCCESS;
    }

    if args.goldens == args.digest_dir.is_some() {
        return usage_error("pass exactly one of --goldens (or --bless) and --digest-dir");
    }

    // `--unit` takes globs (`*`, `?`) over the registered runners.
    let mut wanted: Vec<String> = Vec::new();
    for u in &args.units {
        let hits: Vec<String> = reg
            .iter()
            .map(|r| r.id().to_string())
            .filter(|id| u.contains(['*', '?']) && goldens::ssi::glob_match(u, id))
            .collect();
        if hits.is_empty() {
            wanted.push(u.clone());
        } else {
            wanted.extend(hits);
        }
    }
    if args.all {
        wanted.extend(reg.iter().map(|u| u.id().to_string()));
    }
    let mut seen = std::collections::BTreeSet::new();
    wanted.retain(|w| seen.insert(w.clone()));
    if wanted.is_empty() {
        return usage_error("nothing to run");
    }

    let opts = RunOptions {
        filter: args.filter.clone(),
    };

    let jobs = goldens::jobs::resolve(args.jobs.unwrap_or(1)).min(wanted.len());
    if jobs > 1 {
        let fwd = goldens::jobs::forwarded_args(&raw);
        let report_dir = args
            .report_dir
            .clone()
            .unwrap_or_else(|| root.join("reports").join("goldens"));
        let times = (!args.no_report).then(|| report_dir.join(goldens::jobs::TIMES_FILE));
        if let Some(dir) = times.as_deref().and_then(Path::parent) {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("goldens: {}: {e}", dir.display());
                return ExitCode::from(2);
            }
        }
        return goldens::jobs::run_parallel(
            &wanted,
            &fwd,
            jobs,
            &[report_dir.as_path()],
            &root.join("fixtures"),
            times.as_deref(),
        );
    }

    if let Some(dir) = &args.digest_dir {
        return digest_main(&wanted, &reg, &ctx, &opts, dir, args.digest_debug);
    }
    let dir = args
        .goldens_dir
        .clone()
        .unwrap_or_else(goldens::golden::default_dir);
    goldens_main(&wanted, &reg, &ctx, &opts, &dir, args.bless, args.reverse)
}

/// `--digest-dir DIR`: the Rust side of each unit only, digests to `DIR/<unit>.digest`
/// (and the outputs to `DIR/<unit>.debug` with `--digest-debug`). A unit fails only if
/// its cases cannot be built or a file cannot be written.
fn digest_main(
    wanted: &[String],
    reg: &[Box<dyn goldens::UnitDiff>],
    ctx: &CaseContext,
    opts: &RunOptions,
    dir: &Path,
    debug: bool,
) -> ExitCode {
    let mut failed = 0usize;
    for id in wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FAIL {id} (no runner registered for this unit)");
            failed += 1;
            continue;
        };
        let start = Instant::now();
        let res = goldens::runner::digest_unit(unit.as_ref(), ctx, opts, debug);
        if let Some(e) = &res.harness_error {
            println!("FAIL {id} ({e})");
            failed += 1;
            continue;
        }
        let mut files = vec![(format!("{id}.digest"), &res.lines)];
        if debug {
            files.push((format!("{id}.debug"), &res.debug));
        }
        for (name, lines) in files {
            let path = dir.join(name);
            let body = lines.join("\n") + "\n";
            if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, body))
            {
                eprintln!("goldens: {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
        println!(
            "DIGEST {id} cases={} rust={:.3}s",
            res.lines.len(),
            start.elapsed().as_secs_f64()
        );
    }
    println!("{}", goldens::jobs::run_summary(wanted.len(), failed));
    ExitCode::from(u8::from(failed > 0))
}

/// `--goldens`: the Rust side of each unit, checked against `DIR/<unit>.jsonl`; with
/// `--bless`, the goldens are rewritten from this run instead (with `--filter`, only the cases
/// it ran; a unit with a case whose Rust side fails as a harness error or panics is not
/// blessed). Prints one line per unit and, for a unit that changed, its first changes.
fn goldens_main(
    wanted: &[String],
    reg: &[Box<dyn goldens::UnitDiff>],
    ctx: &CaseContext,
    opts: &RunOptions,
    dir: &Path,
    bless: bool,
    reverse: bool,
) -> ExitCode {
    use goldens::golden;
    let mut failed = 0usize;
    for id in wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FAIL {id} (no runner registered for this unit)");
            failed += 1;
            continue;
        };
        let start = Instant::now();
        let old = match golden::read(dir, id) {
            Ok(g) => g,
            Err(e) => {
                println!("FAIL {id} ({e})");
                failed += 1;
                continue;
            }
        };
        let known = match golden::known_failing(dir, id) {
            Ok(k) => k,
            Err(e) => {
                println!("FAIL {id} ({e})");
                failed += 1;
                continue;
            }
        };
        let res = golden::check_unit(unit.as_ref(), ctx, opts, &old, &known, reverse);
        if let Some(e) = &res.harness_error {
            println!("FAIL {id} ({e})");
            failed += 1;
            continue;
        }
        let secs = start.elapsed().as_secs_f64();
        let counts = format!(
            "cases={} changed={} added={} removed={} broken={} known_failing={} rust={secs:.3}s",
            res.cases(),
            res.changed.len(),
            res.added.len(),
            res.removed.len(),
            res.broken.len(),
            res.known.len()
        );
        for (name, why) in known.iter().filter(|(n, _)| res.known.contains(n)) {
            println!("  known failing {name}: {why}");
        }
        if bless && !res.broken.is_empty() {
            println!("FAIL {id} {counts} (not blessed: a broken case is never blessed)");
            print_golden_changes(&res);
            failed += 1;
            continue;
        }
        if bless {
            let new = golden::blessed(&old, &res, opts.filter.is_some());
            if let Err(e) = golden::write(dir, id, &new) {
                println!("FAIL {id} ({e})");
                failed += 1;
                continue;
            }
            println!("BLESSED {id} {counts}");
            print_golden_changes(&res);
            continue;
        }
        if res.passed() {
            println!("PASS {id} {counts}");
        } else {
            println!("FAIL {id} {counts}");
            print_golden_changes(&res);
            failed += 1;
        }
    }
    // The summary line `goldens::jobs` reads from each child.
    println!("{}", goldens::jobs::run_summary(wanted.len(), failed));
    ExitCode::from(u8::from(failed > 0))
}

/// Lines printed per kind of change for one unit.
const MAX_GOLDEN_LINES: usize = 10;

fn print_golden_changes(res: &goldens::golden::GoldenResult) {
    for (name, c) in res.changed.iter().take(MAX_GOLDEN_LINES) {
        println!("  changed {name} ({} differences)", c.count);
        for d in &c.details {
            println!("    {d}");
        }
    }
    for name in res.added.iter().take(MAX_GOLDEN_LINES) {
        println!("  added {name}");
    }
    for name in res.removed.iter().take(MAX_GOLDEN_LINES) {
        println!("  removed {name}");
    }
    for (name, why) in res.broken.iter().take(MAX_GOLDEN_LINES) {
        println!("  broken {name}: {why}");
    }
    for (kind, n) in [
        ("changed", res.changed.len()),
        ("added", res.added.len()),
        ("removed", res.removed.len()),
        ("broken", res.broken.len()),
    ] {
        if n > MAX_GOLDEN_LINES {
            println!("  ... {} more {kind}", n - MAX_GOLDEN_LINES);
        }
    }
}
