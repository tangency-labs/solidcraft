//! `difftest` CLI. See `crates/difftest/README.md`.
//!
//! ```text
//! cargo run -p difftest --features cpp -- --unit <id> [--unit <id>...]
//! cargo run -p difftest --features cpp -- --green      # all status=green|partial units in plan/units/*.toml
//! cargo run -p difftest --features cpp -- --green --jobs 0   # same, one child process per unit, all cores
//! cargo run -p difftest --features cpp -- --all        # every registered runner
//! cargo run -p difftest --features cpp -- --list
//! cargo run -p difftest --features cpp -- --status-of <id>   # prints green|partial|fail
//! options: --seeds N (default 16)  --filter <substr>  --perturb  --report-dir <dir>  --no-report
//!          --baseline <dir>   # regression ratchet against earlier reports (see `difftest::baseline`)
//!          --run-parked       # only the parked cases (`cpp_defect` entries); nightly, never the gate
//!          --jobs N (-j; default 1, 0 = all cores)   # units in N child processes (`difftest::jobs`)
//! cargo run -p difftest --features cpp -- --unit <id> --fuzz N [--fuzz-out <dir>]   # FFI guard probing
//! cargo run -p difftest --features cpp -- --ssi-corpus [--tol 1e-6|default] [--jobs N] [glob]
//! cargo run -p difftest -- --ssi-corpus [glob]   # the `ssi` and `solidcore-ssi` engines; no `cpp` needed
//! cargo run -p difftest -- --solidcraft-cases [glob]   # Rust only; no `cpp` feature needed
//! cargo run -p difftest --features cpp -- --green --digest-dir <dir> [--digest-debug]   # Rust side
//!          only, one `<dir>/<unit>.digest` per unit (native vs wasm32: scripts/wasm-digest.sh)
//! ```
//! Exit status: 0 no failures (deferred cases are not failures), 1 any failure,
//! 2 usage / harness setup error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use difftest::baseline::{self, Check, Violation, Waivers};
use difftest::case::repo_root;
use difftest::{fuzz, plan, registry, report, run_unit, CaseContext, RunOptions, UnitStatus};

const USAGE: &str = "usage: difftest [--unit <id>]... [--green] [--all] [--list]
       [--status-of <id>] [--seeds N] [--filter <substr>] [--perturb] [--run-parked] [--report-dir <dir>] [--no-report] [--jobs N]
       [--baseline <dir>] [--compare-reports <dir>] [--digest-dir <dir> [--digest-debug]]
       difftest --unit <id>... --fuzz N [--fuzz-out <dir>] [--seeds N] [--filter <substr>]
       difftest --ssi-corpus [--tol 1e-6|default] [--jobs N] [--report-dir <dir>] [--no-report] [case-glob]
       difftest --solidcraft-cases [--report-dir <dir>] [--no-report] [case-glob]
       difftest --step-corpus [--jobs N] [--report-dir <dir>] [--no-report] [file-glob]";

/// Default child processes of `--ssi-corpus`.
const DEFAULT_SSI_JOBS: usize = 2;

/// Failures (and `differs` notes) printed per unit by `--fuzz`; all failures are written.
const MAX_FUZZ_LINES: usize = 10;

/// Default number of generated cases per unit.
const DEFAULT_SEEDS: usize = 16;

/// Gate violations (and waived ones) printed per unit.
const MAX_GATE_LINES: usize = 10;

// CLI flags are naturally independent booleans.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct Args {
    units: Vec<String>,
    green: bool,
    all: bool,
    list: bool,
    status_of: Option<String>,
    seeds: Option<usize>,
    filter: Option<String>,
    perturb: bool,
    run_parked: bool,
    report_dir: Option<PathBuf>,
    no_report: bool,
    baseline: Option<PathBuf>,
    compare_reports: Option<PathBuf>,
    ssi_corpus: bool,
    solidcraft_cases: bool,
    step_corpus: bool,
    tol: Option<String>,
    jobs: Option<usize>,
    glob: Option<String>,
    fuzz: Option<usize>,
    fuzz_out: Option<PathBuf>,
    digest_dir: Option<PathBuf>,
    digest_debug: bool,
    case_timeout: Option<u64>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--unit" | "-u" => a.units.push(val("--unit")?),
            "--green" => a.green = true,
            "--all" => a.all = true,
            "--list" => a.list = true,
            "--status-of" => a.status_of = Some(val("--status-of")?),
            "--seeds" => {
                let v = val("--seeds")?;
                a.seeds = Some(v.parse().map_err(|_| format!("bad --seeds `{v}`"))?);
            }
            "--filter" => a.filter = Some(val("--filter")?),
            "--perturb" => a.perturb = true,
            "--run-parked" => a.run_parked = true,
            "--report-dir" => a.report_dir = Some(PathBuf::from(val("--report-dir")?)),
            "--no-report" => a.no_report = true,
            "--baseline" => a.baseline = Some(PathBuf::from(val("--baseline")?)),
            "--compare-reports" => {
                a.compare_reports = Some(PathBuf::from(val("--compare-reports")?));
            }
            "--ssi-corpus" => a.ssi_corpus = true,
            "--solidcraft-cases" => a.solidcraft_cases = true,
            "--step-corpus" => a.step_corpus = true,
            "--tol" => a.tol = Some(val("--tol")?),
            "--jobs" | "-j" => {
                let v = val("--jobs")?;
                a.jobs = Some(v.parse().map_err(|_| format!("bad --jobs `{v}`"))?);
            }
            "--fuzz" => {
                let v = val("--fuzz")?;
                a.fuzz = Some(v.parse().map_err(|_| format!("bad --fuzz `{v}`"))?);
            }
            "--fuzz-out" => a.fuzz_out = Some(PathBuf::from(val("--fuzz-out")?)),
            "--digest-dir" => a.digest_dir = Some(PathBuf::from(val("--digest-dir")?)),
            "--digest-debug" => a.digest_debug = true,
            "--case-timeout" => {
                let v = val("--case-timeout")?;
                a.case_timeout = Some(v.parse().map_err(|_| format!("bad --case-timeout `{v}`"))?);
            }
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

fn main() -> ExitCode {
    // Child of `--ssi-corpus`: one engine on one case (see `difftest::ssi::run_child`).
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if std::env::var_os(difftest::jobs::WORKER_ENV).is_some() {
        difftest::jobs::watch_parent();
    }
    if raw.first().map(String::as_str) == Some("--ssi-case") {
        return difftest::ssi::run_child(raw.get(1..).unwrap_or(&[]));
    }
    // Child of `--step-corpus`: one kernel on one body (see `difftest::step::run_child`).
    if raw.first().map(String::as_str) == Some("--step-case") {
        return difftest::step::run_child(raw.get(1..).unwrap_or(&[]));
    }
    // Child of unit `corpus.parity`: one side on one model file
    // (see `difftest::units::corpus_parity::run_child`).
    if raw.first().map(String::as_str) == Some("--corpus-parity-child") {
        #[cfg(feature = "cpp")]
        return difftest::units::corpus_parity::run_child(raw.get(1..).unwrap_or(&[]));
        #[cfg(not(feature = "cpp"))]
        {
            eprintln!("difftest: --corpus-parity-child needs feature `cpp`");
            return ExitCode::from(2);
        }
    }
    // Child of `--fuzz`: one batch of fuzz cases (see `difftest::fuzz::run_child`).
    if raw.first().map(String::as_str) == Some("--fuzz-exec") {
        return fuzz::run_child(raw.get(1..).unwrap_or(&[]));
    }
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("difftest: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let root = repo_root();
    let reg = registry();
    let ctx = CaseContext {
        root: root.clone(),
        seeds: args.seeds.unwrap_or(DEFAULT_SEEDS),
    };

    // Rust only, so it runs without the C++ kernel.
    if args.baseline.is_some()
        && (args.perturb
            || args.list
            || args.status_of.is_some()
            || args.filter.is_some()
            || args.digest_dir.is_some()
            || args.fuzz.is_some()
            || args.ssi_corpus
            || args.solidcraft_cases
            || args.step_corpus)
    {
        eprintln!("difftest: --baseline requires an unfiltered comparison run");
        return ExitCode::from(2);
    }

    // Without the C++ kernel it builds the Rust side alone.
    if args.step_corpus {
        let opts = difftest::step::CorpusOptions {
            glob: args.glob.clone(),
            jobs: args.jobs.unwrap_or(DEFAULT_SSI_JOBS).max(1),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
        };
        return difftest::step::run_corpus(&root, &opts);
    }

    if args.solidcraft_cases {
        let opts = difftest::solidcraft::CasesOptions {
            glob: args.glob.clone(),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
        };
        return difftest::solidcraft::run_cases(&root, &opts);
    }

    // Without the C++ kernel it runs the `ssi` engine alone.
    if args.ssi_corpus {
        let opts = difftest::ssi::CorpusOptions {
            tol: args
                .tol
                .clone()
                .unwrap_or_else(|| difftest::ssi::DEFAULT_TOL.to_string()),
            glob: args.glob.clone(),
            jobs: args.jobs.unwrap_or(DEFAULT_SSI_JOBS).max(1),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
        };
        return difftest::ssi::run_corpus(&root, &opts);
    }

    // Reports only: no C++ kernel needed.
    if let Some(current) = &args.compare_reports {
        return match Waivers::load(&root) {
            Ok(waivers) => compare_reports_main(&args, current, &waivers),
            Err(e) => {
                eprintln!("difftest: {e}");
                ExitCode::from(2)
            }
        };
    }

    if !solidcore_ffi::LINKED {
        eprintln!(
            "difftest: built without the C++ kernel; run with \
             `cargo run -p difftest --features cpp -- ...`"
        );
        return ExitCode::from(2);
    }
    if args.tol.is_some() {
        eprintln!("difftest: --tol only applies to --ssi-corpus");
        return ExitCode::from(2);
    }
    if args.jobs.is_some() && (args.fuzz.is_some() || args.list || args.status_of.is_some()) {
        eprintln!("difftest: --jobs does not apply to --fuzz, --list or --status-of");
        return ExitCode::from(2);
    }

    if args.fuzz.is_some() || args.fuzz_out.is_some() {
        return fuzz_main(&args, &ctx, &root);
    }

    let plan_units = plan::load(&root).unwrap_or_default();
    let waivers = match Waivers::load(&root) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("difftest: {e}");
            return ExitCode::from(2);
        }
    };

    if args.list {
        for u in &reg {
            let status = plan_units
                .iter()
                .find(|p| p.id == u.id())
                .map_or("-", |p| p.status.as_str());
            let n = u
                .cases(&ctx)
                .map_or_else(|e| format!("error: {e}"), |c| c.len().to_string());
            println!("{}\tstatus={status}\tcases={n}", u.id());
        }
        return ExitCode::SUCCESS;
    }

    if let Some(id) = &args.status_of {
        // Runs the unit (no report) and prints only its status word.
        let status = reg
            .iter()
            .find(|u| u.id() == id.as_str())
            .map_or(UnitStatus::Fail, |unit| {
                let opts = RunOptions {
                    filter: args.filter.clone(),
                    perturb: false,
                    case_timeout: args.case_timeout.map(std::time::Duration::from_secs),
                    run_parked: false,
                };
                let res = run_unit(unit.as_ref(), &ctx, &opts);
                // The value-share floor is part of the gate (`--green` enforces it too).
                match baseline::value_share(&res) {
                    Some(v) if res.pass() && waivers.covering(&v).is_none() => {
                        eprintln!("difftest: {id}: {}", v.detail);
                        UnitStatus::Fail
                    }
                    _ => res.status(),
                }
            });
        println!("{}", status.as_str());
        return if status == UnitStatus::Fail {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        };
    }

    // `--unit` takes globs (`*`, `?`) over the registered runners.
    let mut wanted: Vec<String> = Vec::new();
    for u in &args.units {
        let hits: Vec<String> = reg
            .iter()
            .map(|r| r.id().to_string())
            .filter(|id| u.contains(['*', '?']) && difftest::ssi::glob_match(u, id))
            .collect();
        if hits.is_empty() {
            wanted.push(u.clone());
        } else {
            wanted.extend(hits);
        }
    }
    if args.green {
        match plan::load(&root) {
            Ok(p) => wanted.extend(plan::green_units(&p)),
            Err(e) => {
                eprintln!("difftest: --green: {e}");
                return ExitCode::from(2);
            }
        }
    }
    if args.all {
        wanted.extend(reg.iter().map(|u| u.id().to_string()));
    }
    let mut seen = std::collections::BTreeSet::new();
    wanted.retain(|w| seen.insert(w.clone()));
    let mut setup_error = false;
    // A unit that was green or partial in the baseline must still be run by --green.
    let mut dropped = 0usize;
    if let Some(dir) = &args.baseline {
        let preflight = if args.green || args.all {
            baseline::baseline_units(dir)
        } else {
            baseline::validate_selected_baselines(dir, &wanted).map(|()| Vec::new())
        };
        match preflight {
            Ok(units) => {
                for u in units.iter().filter(|u| args.green && !wanted.contains(u)) {
                    let v = Violation {
                        unit: u.clone(),
                        check: Check::Ratchet,
                        case: None,
                        detail: format!(
                            "unit `{u}` passed in the baseline but --green no longer runs it \
                             (its plan status changed)"
                        ),
                    };
                    println!("{u}:");
                    if print_violations(&[v], &waivers) {
                        dropped += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("difftest: {e}");
                setup_error = true;
            }
        }
    }
    if setup_error {
        return ExitCode::from(2);
    }
    if dropped > 0 {
        return ExitCode::from(1);
    }
    if wanted.is_empty() && args.green {
        println!("difftest: no units with status = \"green\" or \"partial\" yet");
        return ExitCode::SUCCESS;
    }
    if wanted.is_empty() {
        eprintln!("difftest: nothing to run");
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let opts = RunOptions {
        filter: args.filter.clone(),
        perturb: args.perturb,
        case_timeout: args.case_timeout.map(std::time::Duration::from_secs),
        run_parked: args.run_parked,
    };
    let report_dir = args
        .report_dir
        .clone()
        .unwrap_or_else(|| root.join("reports").join("difftest"));

    let jobs = difftest::jobs::resolve(args.jobs.unwrap_or(1)).min(wanted.len());
    if jobs > 1 {
        let fwd = difftest::jobs::forwarded_args(&raw);
        // Previous reports order the queue; a fresh --report-dir falls back to the default,
        // and a unit with no report anywhere to its fixture size (reports are not committed).
        let default_dir = root.join("reports").join("difftest");
        let dirs = [report_dir.as_path(), default_dir.as_path()];
        let times = (!args.no_report).then(|| report_dir.join(difftest::jobs::TIMES_FILE));
        return difftest::jobs::run_parallel(
            &wanted,
            &fwd,
            jobs,
            &dirs,
            &root.join("fixtures"),
            times.as_deref(),
        );
    }

    if let Some(dir) = &args.digest_dir {
        return digest_main(&wanted, &reg, &ctx, &opts, dir, args.digest_debug);
    }

    let mut failed = 0usize;
    let mut partial = 0usize;
    let mut skipped = 0usize;
    for id in &wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FAIL {id} (no difftest runner registered for this unit)");
            failed += 1;
            continue;
        };
        let res = run_unit(unit.as_ref(), &ctx, &opts);
        if args.perturb && res.status() != UnitStatus::SkippedPlatform {
            let (n, caught) = res.perturb_stats();
            let ok = n > 0 && n == caught;
            let unperturbed = res
                .cases
                .len()
                .saturating_sub(n + res.deferred() + res.rejected());
            let rejected = if res.rejected() == 0 {
                String::new()
            } else {
                format!(" rejected={}", res.rejected())
            };
            println!(
                "{} {id} perturbed={n} detected={caught} unperturbed={unperturbed} deferred={}{rejected} \
                 (perturb mode: every perturbed case must FAIL)",
                if ok { "SENSITIVE" } else { "INSENSITIVE" },
                res.deferred()
            );
            if !ok {
                failed += 1;
            }
            continue;
        }
        println!("{}", report::summary_line(&res));
        eprintln!("{}", res.timing.line(id, res.cases.len()));
        if let Some(dir) = std::env::var_os("DIFFTEST_RUST_DIGEST") {
            let path = Path::new(&dir).join(format!("{id}.digest"));
            let body = res.timing.rust_digests.join("\n") + "\n";
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, body))
            {
                eprintln!("difftest: {}: {e}", path.display());
            }
            let path = Path::new(&dir).join(format!("{id}.cppdigest"));
            let body = res.timing.cpp_digests.join("\n") + "\n";
            if let Err(e) = std::fs::write(&path, body) {
                eprintln!("difftest: {}: {e}", path.display());
            }
        }
        for c in res.cases.iter().filter(|c| c.fail()).take(5) {
            let first = c
                .harness_error
                .clone()
                .or_else(|| {
                    c.diff
                        .mismatches
                        .first()
                        .map(|m| format!("{}: cpp={} rust={}", m.field, m.cpp, m.rust))
                })
                .unwrap_or_default();
            println!("  case {} [{}]: {first}", c.name, c.source.describe());
        }
        for c in res.cases.iter().filter(|c| c.is_known_mismatch()) {
            let first = c
                .diff
                .mismatches
                .first()
                .map(|m| format!("{}: cpp={} rust={}", m.field, m.cpp, m.rust))
                .unwrap_or_default();
            println!(
                "  known-mismatch {} [{}]: {first}",
                c.name,
                c.source.describe()
            );
        }
        for c in res.cases.iter().filter(|c| c.is_parked()) {
            println!(
                "  parked {} ({}) [{}]",
                c.name,
                c.cpp_defect.as_deref().unwrap_or_default(),
                c.source.describe()
            );
        }
        for c in res.cases.iter().filter(|c| c.is_cpp_defect()) {
            let first = c
                .diff
                .mismatches
                .first()
                .map(|m| format!("{}: cpp={} rust={}", m.field, m.cpp, m.rust))
                .unwrap_or_default();
            println!(
                "  cpp-defect {} ({}) [{}]: {first}",
                c.name,
                c.cpp_defect.as_deref().unwrap_or_default(),
                c.source.describe()
            );
        }
        let plan_status = plan_units
            .iter()
            .find(|p| p.id == id.as_str())
            .map(|p| p.status.as_str());
        let gate_error = selection_gate(res.status(), plan_status, args.baseline.is_some());
        if let Some(reason) = gate_error {
            println!("  FAIL {id}: {reason}");
        }
        let mut unit_failed = res.status() == UnitStatus::Fail || gate_error.is_some();
        if !unit_failed && res.status() != UnitStatus::SkippedPlatform {
            let mut violations: Vec<Violation> = baseline::value_share(&res).into_iter().collect();
            if let (Some(dir), None) = (&args.baseline, &args.filter) {
                match baseline::load_baseline(dir, id) {
                    Ok(Some(b)) => violations.extend(baseline::ratchet(&b, &res)),
                    Ok(None) => println!("  baseline: no earlier report (new unit)"),
                    Err(e) => {
                        eprintln!("difftest: {e}");
                        setup_error = true;
                    }
                }
            }
            unit_failed = print_violations(&violations, &waivers);
        }
        if unit_failed {
            failed += 1;
        } else if res.status() == UnitStatus::Partial {
            partial += 1;
        } else if res.status() == UnitStatus::SkippedPlatform {
            skipped += 1;
        }
        if !args.no_report && !args.perturb {
            match report::write_unit_report(&report_dir, &res) {
                Ok(p) => println!(
                    "  report: {}",
                    p.strip_prefix(&root).unwrap_or(&p).display()
                ),
                Err(e) => {
                    eprintln!("difftest: {e}");
                    setup_error = true;
                }
            }
        }
    }
    let dropped_note = if dropped > 0 {
        format!(", {dropped} dropped from --green")
    } else {
        String::new()
    };
    println!(
        "{}{dropped_note}",
        report::run_summary(wanted.len(), partial, failed, skipped)
    );
    if setup_error {
        ExitCode::from(2)
    } else if failed > 0 || dropped > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Selection constraints are not case-level waivers. An unsupported oracle cannot
/// satisfy an exact-base run, including when that baseline has no earlier unit report.
fn selection_gate(
    status: UnitStatus,
    plan_status: Option<&str>,
    has_baseline: bool,
) -> Option<&'static str> {
    match status {
        UnitStatus::Partial if plan_status == Some("green") => {
            Some("plan status is green but cases are deferred or known mismatches")
        }
        UnitStatus::SkippedPlatform if has_baseline => {
            Some("skipped reference cannot validate an exact-base comparison")
        }
        _ => None,
    }
}

/// Prints gate violations (waived ones with their reason); true if any is not waived.
/// `--compare-reports DIR --baseline BASE`: the regression ratchet applied to the finished
/// reports of a sweep run elsewhere (CI runs the sweep while the exact-base artifact is
/// still being validated). Runs no case and needs no C++. The sweep itself already enforced
/// per-unit status and the value-share floor; this adds exactly the `--baseline` checks:
/// every green/partial baseline unit still has a report, seeds match, and no case is lost.
fn compare_reports_main(args: &Args, current: &Path, waivers: &Waivers) -> ExitCode {
    let Some(base_dir) = &args.baseline else {
        eprintln!("difftest: --compare-reports requires --baseline");
        return ExitCode::from(2);
    };
    if args.green
        || args.all
        || args.perturb
        || args.filter.is_some()
        || args.seeds.is_some()
        || !args.units.is_empty()
    {
        eprintln!("difftest: --compare-reports takes only --baseline");
        return ExitCode::from(2);
    }
    let setup = |e: difftest::HarnessError| {
        eprintln!("difftest: {e}");
        ExitCode::from(2)
    };
    let base_units = match baseline::baseline_units(base_dir) {
        Ok(u) => u,
        Err(e) => return setup(e),
    };
    let now_units = match baseline::report_units(current) {
        Ok(u) => u,
        Err(e) => return setup(e),
    };
    let mut failed = 0usize;
    for u in base_units.iter().filter(|u| !now_units.contains(u)) {
        let v = Violation {
            unit: u.clone(),
            check: Check::Ratchet,
            case: None,
            detail: format!("unit `{u}` passed in the baseline but the sweep produced no report"),
        };
        println!("{u}:");
        if print_violations(&[v], waivers) {
            failed += 1;
        }
    }
    for unit in &now_units {
        let (b, n) = match (
            baseline::load_baseline(base_dir, unit),
            baseline::load_baseline(current, unit),
        ) {
            (Ok(Some(b)), Ok(Some(n))) => (b, n),
            (Ok(None), Ok(Some(_))) => {
                println!("{unit}: no earlier report (new unit)");
                continue;
            }
            (Err(e), _) | (_, Err(e)) => return setup(e),
            (_, Ok(None)) => return setup(difftest::HarnessError::new("report disappeared")),
        };
        let violations = baseline::ratchet_reports(&b, &n);
        if violations.is_empty() {
            println!("OK {unit} ({} baseline cases retained)", b.cases.len());
        } else {
            println!("{unit}:");
            if print_violations(&violations, waivers) {
                failed += 1;
            }
        }
    }
    println!(
        "difftest: compared {} reports against the baseline: {failed} unit(s) regressed",
        now_units.len()
    );
    ExitCode::from(u8::from(failed > 0))
}

fn print_violations(violations: &[Violation], waivers: &Waivers) -> bool {
    let (waived, open): (Vec<&Violation>, Vec<&Violation>) = violations
        .iter()
        .partition(|v| waivers.covering(v).is_some());
    for v in open.iter().take(MAX_GATE_LINES) {
        println!("  GATE-FAIL ({}) {}", v.check.as_str(), v.detail);
    }
    if open.len() > MAX_GATE_LINES {
        println!("  ... {} more gate failures", open.len() - MAX_GATE_LINES);
    }
    for v in waived.iter().take(MAX_GATE_LINES) {
        let reason = waivers.covering(v).map_or("", |w| w.reason.as_str());
        println!("  waived ({}) {}: {reason}", v.check.as_str(), v.detail);
    }
    if waived.len() > MAX_GATE_LINES {
        println!("  ... {} more waived", waived.len() - MAX_GATE_LINES);
    }
    if !open.is_empty() {
        println!(
            "  (fix the regression, or add a [[waiver]] with a reason to {})",
            baseline::WAIVERS_PATH
        );
    }
    !open.is_empty()
}

/// `--digest-dir DIR`: the Rust side of each unit only, digests to `DIR/<unit>.digest`
/// (and the outputs to `DIR/<unit>.debug` with `--digest-debug`). A unit fails only if
/// its cases cannot be built or a file cannot be written.
fn digest_main(
    wanted: &[String],
    reg: &[Box<dyn difftest::UnitDiff>],
    ctx: &CaseContext,
    opts: &RunOptions,
    dir: &Path,
    debug: bool,
) -> ExitCode {
    let mut failed = 0usize;
    for id in wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FAIL {id} (no difftest runner registered for this unit)");
            failed += 1;
            continue;
        };
        let start = Instant::now();
        let res = difftest::runner::digest_unit(unit.as_ref(), ctx, opts, debug);
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
                eprintln!("difftest: {}: {e}", path.display());
                return ExitCode::from(2);
            }
        }
        println!(
            "DIGEST {id} cases={} needs_cpp={} rust={:.3}s",
            res.lines.len(),
            res.needs_cpp,
            start.elapsed().as_secs_f64()
        );
    }
    println!(
        "difftest: {} unit(s), {} passed, 0 partial, {failed} failed",
        wanted.len(),
        wanted.len() - failed
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `--fuzz N`: probes each `--unit` with N mutated cases (see `difftest::fuzz`). No report.
fn fuzz_main(args: &Args, ctx: &CaseContext, root: &Path) -> ExitCode {
    let n = match args.fuzz {
        Some(n) if n > 0 => n,
        _ => {
            eprintln!("difftest: --fuzz needs a case count N >= 1");
            return ExitCode::from(2);
        }
    };
    if args.units.is_empty()
        || args.green
        || args.all
        || args.list
        || args.perturb
        || args.status_of.is_some()
    {
        eprintln!("difftest: --fuzz takes --unit <id> (not --green, --all, --list, --perturb or --status-of)");
        return ExitCode::from(2);
    }
    let opts = fuzz::FuzzOptions {
        cases: n,
        out_dir: args
            .fuzz_out
            .clone()
            .unwrap_or_else(|| root.join("bin").join("fuzz")),
        filter: args.filter.clone(),
    };
    let reg = registry();
    let start = Instant::now();
    let mut wanted = args.units.clone();
    let mut seen = std::collections::BTreeSet::new();
    wanted.retain(|w| seen.insert(w.clone()));
    let mut failed = 0usize;
    let mut skipped = 0usize;
    for id in &wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FUZZ-FAIL {id} (no difftest runner registered for this unit)");
            failed += 1;
            continue;
        };
        let res = fuzz::run_fuzz(unit.as_ref(), ctx, &opts);
        println!("{}", fuzz::summary_line(&res));
        if res.skipped_platform.is_some() {
            skipped += 1;
            continue;
        }
        for f in res.failures.iter().take(MAX_FUZZ_LINES) {
            let file = f.file.as_deref().map_or_else(String::new, |p| {
                format!(" -> {}", p.strip_prefix(root).unwrap_or(p).display())
            });
            println!("  {} {}: {}{file}", f.verdict.as_str(), f.case, f.detail);
        }
        if res.failures.len() > MAX_FUZZ_LINES {
            let dir = opts.out_dir.join(id);
            println!(
                "  ... {} more failures in {}",
                res.failures.len() - MAX_FUZZ_LINES,
                dir.strip_prefix(root).unwrap_or(&dir).display()
            );
        }
        for n in res.notes.iter().take(MAX_FUZZ_LINES) {
            let file = n.file.as_deref().map_or_else(String::new, |p| {
                format!(" -> {}", p.strip_prefix(root).unwrap_or(p).display())
            });
            let what = n.verdict.as_str();
            println!("  note ({what}) {}: {}{file}", n.case, n.detail);
        }
        if !res.pass() {
            failed += 1;
        }
    }
    let skipped_note = if skipped > 0 {
        format!(", {skipped} skipped(platform)")
    } else {
        String::new()
    };
    println!(
        "difftest --fuzz: {} unit(s), {failed} failed, {:.1} s{skipped_note}",
        wanted.len(),
        start.elapsed().as_secs_f64()
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_skips_are_not_exact_base_comparisons() {
        for plan in [None, Some("green"), Some("partial")] {
            assert!(selection_gate(UnitStatus::SkippedPlatform, plan, false).is_none());
            assert!(selection_gate(UnitStatus::SkippedPlatform, plan, true).is_some());
            assert!(selection_gate(UnitStatus::Green, plan, true).is_none());
        }
        // A real deferral in a green unit must still fail, on every platform.
        assert!(selection_gate(UnitStatus::Partial, Some("green"), false).is_some());
        assert!(selection_gate(UnitStatus::Partial, Some("green"), true).is_some());
        assert!(selection_gate(UnitStatus::Partial, Some("partial"), true).is_none());
    }
}
