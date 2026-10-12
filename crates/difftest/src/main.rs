//! `difftest` CLI. See `crates/difftest/README.md`.
//!
//! ```text
//! cargo run -p difftest --features cpp -- --unit <id> [--unit <id>...]
//! cargo run -p difftest --features cpp -- --green      # all status=green units in plan/modules.toml
//! cargo run -p difftest --features cpp -- --all        # every registered runner
//! cargo run -p difftest --features cpp -- --list
//! options: --seeds N (default 16)  --filter <substr>  --perturb  --report-dir <dir>  --no-report
//! ```
//! Exit status: 0 all pass, 1 any failure, 2 usage / harness setup error.

use std::path::PathBuf;
use std::process::ExitCode;

use difftest::case::repo_root;
use difftest::{plan, registry, report, run_unit, CaseContext, RunOptions};

const USAGE: &str = "usage: difftest [--unit <id>]... [--green] [--all] [--list] [--seeds N] \
[--filter <substr>] [--perturb] [--report-dir <dir>] [--no-report]";

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
    seeds: Option<usize>,
    filter: Option<String>,
    perturb: bool,
    report_dir: Option<PathBuf>,
    no_report: bool,
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
            "--seeds" => {
                let v = val("--seeds")?;
                a.seeds = Some(v.parse().map_err(|_| format!("bad --seeds `{v}`"))?);
            }
            "--filter" => a.filter = Some(val("--filter")?),
            "--perturb" => a.perturb = true,
            "--report-dir" => a.report_dir = Some(PathBuf::from(val("--report-dir")?)),
            "--no-report" => a.no_report = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
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

    if !solidcore_ffi::LINKED {
        eprintln!(
            "difftest: built without the C++ kernel; run with \
             `cargo run -p difftest --features cpp -- ...`"
        );
        return ExitCode::from(2);
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

    let mut wanted: Vec<String> = args.units.clone();
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
        println!("difftest: no units with status = \"green\" yet");
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

    let mut failed = 0usize;
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
            println!(
                "{} {id} perturbed={n} detected={caught} (perturb mode: every perturbed case must FAIL)",
                if ok { "SENSITIVE" } else { "INSENSITIVE" }
            );
            if !ok {
                failed += 1;
            }
            continue;
        }
        println!("{}", report::summary_line(&res));
        for c in res.cases.iter().filter(|c| !c.pass()).take(5) {
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
        if !res.pass() {
            failed += 1;
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
        "difftest: {} unit(s), {} passed, {failed} failed",
        wanted.len(),
        wanted.len() - failed
    );
    if setup_error {
        ExitCode::from(2)
    } else if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
