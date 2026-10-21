//! `difftest` CLI. See `crates/difftest/README.md`.
//!
//! ```text
//! cargo run -p difftest --features cpp -- --unit <id> [--unit <id>...]
//! cargo run -p difftest --features cpp -- --green      # all status=green|partial units in plan/modules.toml
//! cargo run -p difftest --features cpp -- --green --jobs 0   # same, one child process per unit, all cores
//! cargo run -p difftest --features cpp -- --all        # every registered runner
//! cargo run -p difftest --features cpp -- --list
//! cargo run -p difftest --features cpp -- --status-of <id>   # prints green|partial|fail
//! options: --seeds N (default 16)  --filter <substr>  --perturb  --report-dir <dir>  --no-report
//!          --jobs N (-j; default 1, 0 = all cores)   # units in N child processes (`difftest::jobs`)
//! cargo run -p difftest --features cpp -- --unit <id> --fuzz N [--fuzz-out <dir>]   # FFI guard probing
//! cargo run -p difftest --features cpp -- --ssi-corpus [--tol 1e-6|default] [--jobs N] [glob]
//! cargo run -p difftest -- --solidcraft-cases [glob]   # Rust only; no `cpp` feature needed
//! ```
//! Exit status: 0 no failures (deferred cases are not failures), 1 any failure,
//! 2 usage / harness setup error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use difftest::case::repo_root;
use difftest::{fuzz, plan, registry, report, run_unit, CaseContext, RunOptions, UnitStatus};

const USAGE: &str = "usage: difftest [--unit <id>]... [--green] [--all] [--list] \
[--status-of <id>] [--seeds N] [--filter <substr>] [--perturb] [--report-dir <dir>] [--no-report] [--jobs N]
       difftest --unit <id>... --fuzz N [--fuzz-out <dir>] [--seeds N] [--filter <substr>]
       difftest --ssi-corpus [--tol 1e-6|default] [--jobs N] [--report-dir <dir>] [--no-report] [case-glob]
       difftest --solidcraft-cases [--report-dir <dir>] [--no-report] [case-glob]";

/// Default child processes of `--ssi-corpus`.
const DEFAULT_SSI_JOBS: usize = 2;

/// Failures (and `differs` notes) printed per unit by `--fuzz`; all failures are written.
const MAX_FUZZ_LINES: usize = 10;

/// Default number of generated cases per unit.
const DEFAULT_SEEDS: usize = 16;

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
    report_dir: Option<PathBuf>,
    no_report: bool,
    ssi_corpus: bool,
    solidcraft_cases: bool,
    tol: Option<String>,
    jobs: Option<usize>,
    glob: Option<String>,
    fuzz: Option<usize>,
    fuzz_out: Option<PathBuf>,
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
            "--report-dir" => a.report_dir = Some(PathBuf::from(val("--report-dir")?)),
            "--no-report" => a.no_report = true,
            "--ssi-corpus" => a.ssi_corpus = true,
            "--solidcraft-cases" => a.solidcraft_cases = true,
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
            "-h" | "--help" => return Err(String::new()),
            other
                if (a.ssi_corpus || a.solidcraft_cases)
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
        #[cfg(feature = "cpp")]
        return difftest::ssi::run_child(raw.get(1..).unwrap_or(&[]));
        #[cfg(not(feature = "cpp"))]
        {
            eprintln!("difftest: --ssi-case needs feature `cpp`");
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
    if args.solidcraft_cases {
        let opts = difftest::solidcraft::CasesOptions {
            glob: args.glob.clone(),
            report_dir: args.report_dir.clone(),
            no_report: args.no_report,
        };
        return difftest::solidcraft::run_cases(&root, &opts);
    }

    if !solidcore_ffi::LINKED {
        eprintln!(
            "difftest: built without the C++ kernel; run with \
             `cargo run -p difftest --features cpp -- ...`"
        );
        return ExitCode::from(2);
    }

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
                };
                run_unit(unit.as_ref(), &ctx, &opts).status()
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
    };
    let report_dir = args
        .report_dir
        .clone()
        .unwrap_or_else(|| root.join("reports").join("difftest"));

    let jobs = difftest::jobs::resolve(args.jobs.unwrap_or(1)).min(wanted.len());
    if jobs > 1 {
        let fwd = difftest::jobs::forwarded_args(&raw);
        // Previous reports order the queue; a fresh --report-dir falls back to the default.
        let default_dir = root.join("reports").join("difftest");
        let dirs = [report_dir.as_path(), default_dir.as_path()];
        return difftest::jobs::run_parallel(&wanted, &fwd, jobs, &dirs);
    }

    let mut failed = 0usize;
    let mut partial = 0usize;
    let mut setup_error = false;
    for id in &wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FAIL {id} (no difftest runner registered for this unit)");
            failed += 1;
            continue;
        };
        let res = run_unit(unit.as_ref(), &ctx, &opts);
        if args.perturb {
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
        let plan_status = plan_units
            .iter()
            .find(|p| p.id == id.as_str())
            .map(|p| p.status.as_str());
        match res.status() {
            UnitStatus::Fail => failed += 1,
            UnitStatus::Partial if plan_status == Some("green") => {
                // A green unit must have zero deferred cases (AGENTS.md 4a).
                println!("  FAIL {id}: plan status is green but cases are deferred");
                failed += 1;
            }
            UnitStatus::Partial => partial += 1,
            UnitStatus::Green => {}
        }
        if !args.no_report {
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
    println!(
        "difftest: {} unit(s), {} passed, {partial} partial, {failed} failed",
        wanted.len(),
        wanted.len() - failed - partial
    );
    if setup_error {
        ExitCode::from(2)
    } else if failed > 0 {
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
    for id in &wanted {
        let Some(unit) = reg.iter().find(|u| u.id() == id.as_str()) else {
            println!("FUZZ-FAIL {id} (no difftest runner registered for this unit)");
            failed += 1;
            continue;
        };
        let res = fuzz::run_fuzz(unit.as_ref(), ctx, &opts);
        println!("{}", fuzz::summary_line(&res));
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
    println!(
        "difftest --fuzz: {} unit(s), {failed} failed, {:.1} s",
        wanted.len(),
        start.elapsed().as_secs_f64()
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
