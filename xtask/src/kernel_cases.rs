//! `cargo xtask kernel-cases`: the kernel regression rig. Runs every case in
//! `tests/kernel-cases/<area>/<id>/` through `solidcraft-cli kernel-cases` on solidcore, or the
//! C++ SMLib kernel when asked for, one process per case (so a crash or a hang is that case's result, not the run's), and writes
//! its report: results per case, coverage per area and per source, and the issues and PRs looked
//! at and skipped. Each case is graded against its own answer.
//!
//! The run's output folder (`--out`, default `target/kernel-cases`) holds `results.json`, every
//! case's full result, and `dumps/<area>-<case>/`, the inputs of each SMLib call a case's run
//! failed in (`solidcraft-cli kernel-cases --dumps`), for replay.
//!
//! `--kernel solidcore|smlib|both` (default solidcore) picks the backend each case's process runs
//! on (`SOLIDCRAFT_KERNEL`). A solidcore run keeps its results, dumps and report in
//! `<out>/solidcore/`; an smlib run writes `docs/kernel-cases.md`; `both` runs every case on each,
//! keeps `docs/kernel-cases.md` as the SMLib report and writes the side by side to
//! `docs/kernel-cases-backends.md` and `<out>/backends.json`.
//!
//! `--route <op>=solidcore[,…]` scores single operation families instead: the "solidcore" side runs
//! on SMLib (`SOLIDCRAFT_KERNEL=smlib`) with only those families moved to solidcore
//! (`SOLIDCRAFT_KERNEL_ROUTES`), its label names them (`solidcore[boolean,measure]`), and its
//! results, report and `backends.json` / `backends.md` go to `<out>/solidcore-<op>[-<op>…]/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{cargo, root};

const DEFAULT_TIMEOUT_S: u64 = 180;

/// The backends `--kernel` may name, as `SOLIDCRAFT_KERNEL` takes them.
const BACKENDS: [&str; 2] = ["smlib", "solidcore"];

/// How many cases ended in each status.
type StatusCounts = BTreeMap<String, usize>;

pub(crate) struct Options {
    cli: Option<String>,
    area: Option<String>,
    timeout: Duration,
    pub(crate) jobs: usize,
    /// The run's output folder: `results.json` and the SMLib dumps (`dumps/`).
    out: PathBuf,
    /// The backends each case runs on (`--kernel`, default solidcore alone).
    pub(crate) kernels: Vec<&'static str>,
    /// `--route`: the operation families the solidcore side moves to solidcore, on SMLib for the
    /// rest; empty, solidcore answers every family.
    routes: Vec<String>,
    /// `--known-failing FILE`: run as a regression gate against that baseline (see [`gate`]).
    known_failing: Option<PathBuf>,
    /// `--validation MODE`: when SMLib's full check runs on the cases' features (each_step,
    /// each_feature, off; the CLI's default, each_feature, when not given).
    validation: Option<String>,
}

impl Options {
    /// A backend's output folder: the run's own for SMLib, `<out>/<backend>/` for any other, and
    /// `<out>/solidcore-<op>[-<op>…]/` for routed families.
    fn out_of(&self, kernel: &str) -> PathBuf {
        match kernel {
            "smlib" => self.out.clone(),
            "solidcore" if !self.routes.is_empty() => self.out.join(format!("solidcore-{}", self.routes.join("-"))),
            other => self.out.join(other),
        }
    }

    /// What a backend's side is called in the reports: `solidcore[boolean,measure]` for routed
    /// families.
    fn label(&self, kernel: &str) -> String {
        if kernel == "solidcore" && !self.routes.is_empty() { format!("solidcore[{}]", self.routes.join(",")) } else { kernel.to_string() }
    }

    /// The environment a backend's case processes get: `SOLIDCRAFT_KERNEL`, and for routed
    /// families SMLib plus `SOLIDCRAFT_KERNEL_ROUTES`.
    fn env(&self, kernel: &str) -> Vec<(&'static str, String)> {
        if kernel == "solidcore" && !self.routes.is_empty() {
            let routes = self.routes.iter().map(|op| format!("{op}=solidcore")).collect::<Vec<_>>().join(",");
            vec![("SOLIDCRAFT_KERNEL", "smlib".into()), ("SOLIDCRAFT_KERNEL_ROUTES", routes)]
        } else {
            vec![("SOLIDCRAFT_KERNEL", kernel.into())]
        }
    }
}

/// The operation families `--route` may name: `Op::ALL` in crates/smlib/src/route.rs, as
/// `Op::id` spells them (a test keeps the two together).
const ROUTE_OPS: [&str; 12] =
    ["convert", "primitive", "transform", "measure", "check", "boolean", "blend", "offset", "split", "sweep", "ssi", "file"];

/// `--route boolean=solidcore,measure=solidcore`: the families, in table order, each once.
fn parse_routes(text: &str) -> Result<Vec<String>, String> {
    let mut ops = Vec::new();
    for item in text.split(',').map(str::trim) {
        let Some((op, backend)) = item.split_once('=') else {
            return Err(format!("--route: `{item}` is not <op>=solidcore"));
        };
        if backend.trim() != "solidcore" {
            return Err(format!("--route: `{item}`: only solidcore routes (the other side is SMLib already)"));
        }
        if !ROUTE_OPS.contains(&op.trim()) {
            return Err(format!("--route: no operation family `{}` ({})", op.trim(), ROUTE_OPS.join(", ")));
        }
        ops.push(op.trim().to_string());
    }
    Ok(ROUTE_OPS.iter().filter(|op| ops.iter().any(|o| o == *op)).map(|op| op.to_string()).collect())
}

/// The run's options; `out` is its output folder unless `--out` gives one.
pub(crate) fn options(rest: &[&str], out: &str) -> Result<Options, String> {
    let arg = |name: &str| rest.iter().position(|a| *a == name).and_then(|i| rest.get(i + 1)).map(|s| s.to_string());
    let secs = arg("--timeout").map(|t| t.parse::<u64>().map_err(|e| format!("--timeout: {e}"))).transpose()?.unwrap_or(DEFAULT_TIMEOUT_S);
    let jobs = arg("--jobs").map(|j| j.parse::<usize>().map_err(|e| format!("--jobs: {e}"))).transpose()?.unwrap_or(4).clamp(1, 32);
    let out = arg("--out").map_or_else(|| root().join(out), PathBuf::from);
    let kernels = match arg("--kernel").as_deref() {
        Some("smlib") => vec![BACKENDS[0]],
        None | Some("solidcore") => vec![BACKENDS[1]],
        Some("both") => BACKENDS.to_vec(),
        Some(k) => return Err(format!("--kernel: unknown backend '{k}' (smlib, solidcore or both)")),
    };
    let routes = match arg("--route") {
        None if rest.contains(&"--route") => return Err("--route: give <op>=solidcore[,…]".into()),
        None => Vec::new(),
        Some(r) if kernels.contains(&"solidcore") => parse_routes(&r)?,
        Some(_) => return Err("--route: needs --kernel solidcore or both".into()),
    };
    let known_failing = arg("--known-failing").map(|p| {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { root().join(p) }
    });
    // A list is one kernel's: the gate grades the run's one kernel against it.
    if known_failing.is_some() && kernels.len() != 1 {
        return Err("--known-failing: a list is one kernel's, so not with --kernel both".into());
    }
    let validation = arg("--validation");
    if let Some(v) = validation.as_deref().filter(|v| !["each_step", "each_feature", "off"].contains(v)) {
        return Err(format!("--validation must be each_step, each_feature or off, not `{v}`"));
    }
    Ok(Options { cli: arg("--cli"), area: arg("--area"), timeout: Duration::from_secs(secs), jobs, out, kernels, routes, known_failing, validation })
}

/// Every case's full result as JSON, into the run's output folder.
pub(crate) fn save_results(o: &Options, results: &[Value]) -> Result<(), String> {
    save_json(&o.out, "results.json", results)
}

/// `value` as pretty JSON into `dir/name`.
fn save_json<T: serde::Serialize + ?Sized>(dir: &Path, name: &str, value: &T) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(name);
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

pub(crate) fn read(path: &Path) -> Value {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null)
}

/// Every case directory, sorted by area then id.
fn cases(dir: &Path, only: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for area in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let area = area.path();
        if !area.is_dir() || only.is_some_and(|a| area.file_name().is_none_or(|n| n != a)) {
            continue;
        }
        for case in std::fs::read_dir(&area).map_err(|e| format!("{}: {e}", area.display()))?.flatten() {
            let case = case.path();
            if case.join("recipe.json").is_file() && case.join("expected.json").is_file() {
                out.push(case);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// One case, in its own process, killed after the timeout; `path` is the case's directory under
/// `tests/kernel-cases/`.
fn run_one(o: &Options, case: &Path, kernel: &str) -> Value {
    let mut r = run_case(o, case, kernel);
    let base = root().join("tests/kernel-cases");
    r["path"] = json!(case.strip_prefix(&base).unwrap_or(case).to_string_lossy().replace('\\', "/"));
    r
}

/// Waits for `child` for at most `timeout` while reading its stdout and stderr, so a child that
/// writes more than the pipe buffer (64 KiB on Linux) is not blocked in `write` and taken for a
/// hang. `None`: the child was still running at the timeout and has been killed.
fn wait_draining(mut child: std::process::Child, timeout: Duration) -> Result<Option<std::process::Output>, String> {
    fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    }
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                // The readers are left to finish on their own: a grandchild (`cargo run`'s CLI)
                // may still hold the pipes open.
                return Ok(None);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("wait: {e}")),
        }
    };
    let join = |h: std::thread::JoinHandle<Vec<u8>>| h.join().map_err(|_| "output: a pipe reader stopped".to_string());
    Ok(Some(std::process::Output { status, stdout: join(stdout)?, stderr: join(stderr)? }))
}

fn run_case(o: &Options, case: &Path, kernel: &str) -> Value {
    let rel = case.strip_prefix(root()).unwrap_or(case).to_path_buf();
    let mut cmd = match &o.cli {
        Some(bin) => Command::new(bin),
        None => {
            let mut c = cargo();
            c.args(["run", "-q", "--release", "-p", "solidcraft-cli", "--features", "smlib", "--"]);
            c
        }
    };
    let mut dumps = std::ffi::OsString::from("--dumps=");
    dumps.push(o.out_of(kernel).join("dumps"));
    cmd.env_remove("SOLIDCRAFT_KERNEL_ROUTES").envs(o.env(kernel)).current_dir(root()).args(["kernel-cases", "--json"]).arg(dumps);
    if let Some(v) = &o.validation {
        cmd.arg(format!("--validation={v}"));
    }
    cmd.arg(&rel).stdout(Stdio::piped()).stderr(Stdio::piped());
    let base = |status: &str, error: String| {
        let area = case.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = case.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        json!({"case": id, "area": area, "status": status, "error": error})
    };
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return base("fail", format!("solidcraft-cli: {e}")),
    };
    // A case may need longer than the run's timeout (`timeout_s`, at most 30 min).
    let timeout = read(&case.join("expected.json"))["timeout_s"].as_u64().map_or(o.timeout, |s| Duration::from_secs(s.min(1800)).max(o.timeout));
    let start = Instant::now();
    let out = match wait_draining(child, timeout) {
        Ok(Some(out)) => out,
        Ok(None) => return base("hang", format!("no result after {} s", timeout.as_secs())),
        Err(e) => return base("fail", e),
    };
    match serde_json::from_slice::<Value>(&out.stdout).ok().and_then(|v| v.as_array().and_then(|a| a.first().cloned())) {
        Some(mut r) => {
            // `seconds` is the commands' own time, measured inside the case's process.
            r["process_seconds"] = json!(start.elapsed().as_secs_f64());
            r
        }
        // No JSON: the process died (a native crash, an abort).
        None => {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" / ");
            base("crash", format!("process exited ({}) without a result: {tail}", out.status))
        }
    }
}

/// Every case on the run's one backend (the ladder's: it refuses `--kernel both`).
pub(crate) fn run_all(o: &Options, list: &[PathBuf]) -> Vec<Value> {
    run_on(o, list, &o.kernels).into_iter().next().unwrap_or_default()
}

/// Every case on each backend, one job pool for all the runs; the results per backend, in order.
fn run_on(o: &Options, list: &[PathBuf], kernels: &[&str]) -> Vec<Vec<Value>> {
    let jobs = list.len() * kernels.len();
    let next = Arc::new(Mutex::new(0usize));
    let results = Arc::new(Mutex::new(vec![Value::Null; jobs]));
    std::thread::scope(|s| {
        for _ in 0..o.jobs.min(jobs.max(1)) {
            let (next, results) = (Arc::clone(&next), Arc::clone(&results));
            s.spawn(move || {
                loop {
                    let i = {
                        let Ok(mut n) = next.lock() else { return };
                        let i = *n;
                        *n += 1;
                        i
                    };
                    // Case by case, each on every backend in turn.
                    let (Some(case), Some(kernel)) = (list.get(i / kernels.len().max(1)), kernels.get(i % kernels.len().max(1))) else { return };
                    let r = run_one(o, case, kernel);
                    let tag = if kernels.len() > 1 { format!("{:<10} ", o.label(kernel)) } else { String::new() };
                    println!(
                        "{:<11} {tag}{}",
                        r["status"].as_str().unwrap_or("?").to_uppercase(),
                        case.strip_prefix(root()).unwrap_or(case).display()
                    );
                    if let Ok(mut all) = results.lock()
                        && let Some(slot) = all.get_mut(i)
                    {
                        *slot = r;
                    }
                }
            });
        }
    });
    let all = Arc::try_unwrap(results).ok().and_then(|m| m.into_inner().ok()).unwrap_or_default();
    (0..kernels.len()).map(|k| all.iter().skip(k).step_by(kernels.len().max(1)).cloned().collect()).collect()
}

/// What went wrong in a result: its failed checks, then its error.
fn notes(r: &Value) -> Vec<String> {
    let mut notes: Vec<String> = r["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| !c["ok"].as_bool().unwrap_or(false))
        .map(|c| format!("{} {} (want {})", c["check"].as_str().unwrap_or(""), short(&c["got"]), short(&c["want"])))
        .collect();
    if let Some(e) = r["error"].as_str() {
        notes.push(e.chars().take(160).collect());
    }
    notes
}

pub(crate) fn cell(r: Option<&Value>) -> String {
    let Some(r) = r else { return String::new() };
    let status = r["status"].as_str().unwrap_or("?");
    let notes = notes(r);
    let s = match status {
        "pass" => "pass".to_string(),
        "unsupported" => "not supported yet".to_string(),
        other => format!("**{}**", other.to_uppercase()),
    };
    if notes.is_empty() { s } else { format!("{s}: {}", notes.join("; ")).replace('|', "/").replace('\n', " ") }
}

fn short(v: &Value) -> String {
    match v {
        Value::Number(n) => n.as_f64().map(|x| format!("{x:.6}").trim_end_matches('0').trim_end_matches('.').to_string()).unwrap_or_default(),
        Value::String(s) => s.chars().take(100).collect(),
        Value::Array(a) => format!("[{}]", a.iter().map(short).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

fn sources_of(want: &Value) -> Vec<String> {
    want["sources"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

/// The number in `owner/repo#123`, for sorting.
fn source_number(s: &str) -> u64 {
    s.rsplit('#').next().and_then(|n| n.parse().ok()).unwrap_or(u64::MAX)
}

/// `tangency-labs/solidcraft#123` as a link.
fn link(source: &str) -> String {
    match source.split_once('#') {
        Some((repo, n)) if repo.contains('/') => format!("[{source}](https://github.com/{repo}/issues/{n})"),
        _ => source.to_string(),
    }
}

/// Coverage by kind of source (issue, merged PR, a frozen truck-geometry PR...), from
/// `sources.json`: how many were looked at, how many became cases, how many were skipped.
fn source_kinds(dir: &Path, cited: &BTreeMap<String, Vec<String>>) -> Result<String, String> {
    let kinds = read(&dir.join("sources.json"));
    let Some(kinds) = kinds.as_object() else { return Ok(String::new()) };
    let cited: std::collections::BTreeSet<u64> = cited.keys().map(|s| source_number(s)).collect();
    let mut table: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (n, kind) in kinds {
        let row = table.entry(kind.as_str().unwrap_or("?").to_string()).or_default();
        if n.parse::<u64>().is_ok_and(|n| cited.contains(&n)) {
            row.0 += 1;
        } else {
            row.1 += 1;
        }
    }
    let mut md = String::from("\n## Coverage by source\n\n| Source | Looked at | Became cases | Skipped |\n|---|---|---|---|\n");
    let (mut a, mut b) = (0, 0);
    for (kind, (cases, skipped)) in &table {
        md += &format!("| {kind} | {} | {cases} | {skipped} |\n", cases + skipped);
        a += cases;
        b += skipped;
    }
    md += &format!("| **all** | {} | {a} | {b} |\n", a + b);
    Ok(md)
}

/// The report of one backend's run into `to` (relative to the repository root, or absolute);
/// `kernel` is the backend's label, `env` what its processes were given.
fn write_report(dir: &Path, list: &[PathBuf], results: &[Value], kernel: &str, env: &str, to: &Path) -> Result<String, String> {
    let key = |r: &Value| (r["area"].as_str().unwrap_or("").to_string(), r["case"].as_str().unwrap_or("").to_string());
    let by_case: BTreeMap<(String, String), &Value> = results.iter().map(|r| (key(r), r)).collect();
    // Coverage: per area, cases and their statuses; per source, its cases.
    let mut areas: BTreeMap<String, (usize, StatusCounts)> = BTreeMap::new();
    let mut per_source: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut rows = String::new();
    for case in list {
        let want = read(&case.join("expected.json"));
        let area = case.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = case.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let entry = areas.entry(area.clone()).or_default();
        entry.0 += 1;
        let run = by_case.get(&(area.clone(), id.clone())).copied();
        let status = run.and_then(|r| r["status"].as_str()).unwrap_or("not run").to_string();
        *entry.1.entry(status).or_default() += 1;
        let sources = sources_of(&want);
        for s in &sources {
            per_source.entry(s.clone()).or_default().push(format!("{area}/{id}"));
        }
        *kinds.entry(want["source"].as_str().unwrap_or("?").split_whitespace().next().unwrap_or("?").to_string()).or_default() += 1;
        rows += &format!(
            "| {area}/{id} | {} | {} | {} | {} |\n",
            sources.iter().map(|s| link(s)).collect::<Vec<_>>().join(" "),
            want["what"].as_str().unwrap_or("").replace('|', "/"),
            want["source"].as_str().unwrap_or("?"),
            cell(run)
        );
    }
    let mut md = String::from(
        "# Kernel regression rig\n\nGenerated by `cargo xtask kernel-cases`. Every geometry case the old kernel (truck) got wrong, as an issue, a PR or a crash report recorded it, is a use case in [`tests/kernel-cases/`](../tests/kernel-cases/) with its own right answer: numbers from analysis or a hand calculation (**analytic**) where the geometry has them, and otherwise the invariants (**invariants**): the operation succeeds, or refuses cleanly where that is the right answer; each body is one closed shell that passes the record's structure and geometry checks and SMLib's own check; the expected face, edge and vertex counts. Never a kernel's output, and never a commercial kernel's.\n\nThe cases run on SMLib, SolidCraft's kernel. A failing case stays failing until the kernel is fixed; there is no workaround on SolidCraft's side. A failed SMLib call leaves its inputs for replay in the run's output folder (`target/kernel-cases/dumps/<area>-<case>/`). See [`tests/kernel-cases/README.md`](../tests/kernel-cases/README.md) for the format. Every area is also a regression gate: CI's Kernel rig workflow runs them on every pull request that touches the kernel and fails on a case that fails and its area's `KNOWN_FAILING` (such as [`fillets/KNOWN_FAILING`](../tests/kernel-cases/fillets/KNOWN_FAILING)) does not list, or a listed case that passes.\n\n",
    );
    if kernel != "smlib" {
        md += &format!("**This run: `{env}`.**\n\n");
    }
    md += "## Coverage\n\n| Area | Cases | Results |\n|---|---|---|\n";
    for (area, (n, statuses)) in &areas {
        md += &format!("| {area} | {n} | {} |\n", statuses.iter().map(|(s, c)| format!("{c} {s}")).collect::<Vec<_>>().join(", "));
    }
    md += &format!(
        "\n{} cases from {} sources; answers: {}.\n\n## Results\n\n| Case | Sources | What went wrong | Answer | Result |\n|---|---|---|---|---|\n",
        list.len(),
        per_source.len(),
        kinds.iter().map(|(k, v)| format!("{v} {k}")).collect::<Vec<_>>().join(", ")
    );
    md += &rows;
    md += "\n## Sources\n\n| Source | Cases |\n|---|---|\n";
    for (s, cs) in &per_source {
        md += &format!("| {} | {} |\n", link(s), cs.join(", "));
    }
    md += &source_kinds(dir, &per_source)?;
    // What was looked at and skipped, area by area (`<area>/skipped.json`).
    md += "\n## Looked at and skipped\n\n";
    let mut skipped = 0;
    let mut area_dirs: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    area_dirs.sort();
    for a in area_dirs {
        // skipped.json, or several (skipped-<part>.json) where people triage an area in parts.
        let mut files: Vec<PathBuf> = std::fs::read_dir(&a)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("skipped") && n.ends_with(".json")))
            .collect();
        files.sort();
        let mut items: Vec<Value> = files.iter().flat_map(|f| read(f).as_array().cloned().unwrap_or_default()).collect();
        if items.is_empty() {
            continue;
        }
        items.sort_by_key(|it| source_number(it["source"].as_str().unwrap_or("")));
        let items = &items;
        md += &format!("### {}\n\n| Source | Why skipped |\n|---|---|\n", a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        for it in items {
            skipped += 1;
            md += &format!("| {} | {} |\n", link(it["source"].as_str().unwrap_or("?")), it["why"].as_str().unwrap_or("").replace('|', "/"));
        }
        md += "\n";
    }
    if skipped == 0 {
        md += "None yet.\n";
    }
    let path = root().join(to);
    let shown = path.strip_prefix(root()).unwrap_or(&path).display().to_string();
    std::fs::write(&path, md).map_err(|e| format!("{shown}: {e}"))?;
    let pass = results.iter().filter(|r| r["status"] == "pass").count();
    let on = if kernel == "smlib" { String::new() } else { format!(" on {kernel}") };
    Ok(format!("kernel-cases: {pass}/{} pass{on} → {shown} ({skipped} skipped sources listed)", list.len()))
}

/// pass, fail, or other (crash, hang, not supported yet...), for the summary.
fn bucket(status: &str) -> &'static str {
    match status {
        "pass" => "pass",
        "fail" => "fail",
        _ => "other",
    }
}

/// A result's commands' time (or, without one, its process's).
fn seconds(r: &Value) -> Option<f64> {
    r["seconds"].as_f64().or_else(|| r["process_seconds"].as_f64())
}

/// The two backends side by side: per case (area, id) each one's status, time and what went wrong,
/// as JSON, and the Markdown report: a summary, then per area the cases that differ first.
/// `labels` name the two sides in the report (`solidcore[boolean]` for routed families); the JSON
/// keys stay `smlib` and `solidcore`.
fn side_by_side(smlib: &[Value], solidcore: &[Value], labels: [&str; 2]) -> (Value, String) {
    let key = |r: &Value| (r["area"].as_str().unwrap_or("").to_string(), r["case"].as_str().unwrap_or("").to_string());
    let mut pairs: BTreeMap<(String, String), [Option<&Value>; 2]> = BTreeMap::new();
    for (k, rs) in [smlib, solidcore].iter().enumerate() {
        for r in rs.iter().filter(|r| !r.is_null()) {
            if let Some(slot) = pairs.entry(key(r)).or_default().get_mut(k) {
                *slot = Some(r);
            }
        }
    }
    let status = |r: Option<&Value>| r.and_then(|r| r["status"].as_str()).unwrap_or("not run").to_string();
    let mut rows = Vec::new();
    let (mut counts, mut times, mut agree) = ([StatusCounts::new(), StatusCounts::new()], [0.0f64; 2], 0);
    for ((area, case), runs) in &pairs {
        let same = status(runs[0]) == status(runs[1]);
        agree += usize::from(same);
        let mut row = json!({"case": case, "area": area, "agree": same});
        for (k, name) in BACKENDS.iter().enumerate() {
            let r = runs[k];
            *counts[k].entry(bucket(&status(r)).to_string()).or_default() += 1;
            times[k] += r.and_then(seconds).unwrap_or(0.0);
            let error = r.map(|r| notes(r).join("; ")).filter(|e| !e.is_empty());
            row[*name] = json!({"status": status(r), "seconds": r.and_then(seconds), "error": error});
        }
        rows.push(row);
    }
    let differ = pairs.len() - agree;
    let mut md = String::from(
        "# Kernel regression rig: SMLib and solidcore\n\nGenerated by `cargo xtask kernel-cases --kernel both`: every case in [`tests/kernel-cases/`](../tests/kernel-cases/) run on each backend (`SOLIDCRAFT_KERNEL`), graded against its own answer. Times are the commands' own. [`kernel-cases.md`](kernel-cases.md) is the SMLib run in full.\n\n",
    );
    if labels[1] != BACKENDS[1] {
        md += &format!("**This run: `{}` is SMLib with only those operation families on solidcore (`--route`).**\n\n", labels[1]);
    }
    md += "## Summary\n\n| Backend | Pass | Fail | Other | Time (s) |\n|---|---|---|---|---|\n";
    for (k, name) in labels.iter().enumerate() {
        let n = |b: &str| counts[k].get(b).copied().unwrap_or(0);
        md += &format!("| {name} | {} | {} | {} | {:.1} |\n", n("pass"), n("fail"), n("other"), times[k]);
    }
    md += &format!("\n{} cases: {agree} agree, {differ} differ.\n", pairs.len());
    let side = |r: &Value| match r["seconds"].as_f64() {
        Some(s) => format!("{} ({s:.2} s)", r["status"].as_str().unwrap_or("?")),
        None => r["status"].as_str().unwrap_or("?").to_string(),
    };
    let mut by_area: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for row in &rows {
        by_area.entry(row["area"].as_str().unwrap_or("")).or_default().push(row);
    }
    for (area, mut list) in by_area {
        list.sort_by_key(|r| (r["agree"].as_bool().unwrap_or(false), r["case"].as_str().unwrap_or("").to_string()));
        md += &format!("\n## {area}\n\n| Case | {} | {} | |\n|---|---|---|---|\n", labels[0], labels[1]);
        for r in list {
            let mark = if r["agree"].as_bool().unwrap_or(false) { "" } else { "**differ**" };
            md += &format!("| {} | {} | {} | {mark} |\n", r["case"].as_str().unwrap_or(""), side(&r["smlib"]), side(&r["solidcore"]));
        }
    }
    let summary: Value = BACKENDS
        .iter()
        .enumerate()
        .map(|(k, name)| (name.to_string(), json!({"counts": counts[k], "seconds": times[k]})))
        .collect::<serde_json::Map<_, _>>()
        .into();
    (json!({"labels": labels, "summary": summary, "agree": agree, "differ": differ, "cases": rows}), md)
}

pub fn run(rest: &[&str]) -> Result<(), String> {
    if rest.contains(&"--ladder") {
        if options(rest, "target/kernel-ladder")?.kernels.len() != 1 {
            return Err("--kernel: the ladder runs on one backend, smlib or solidcore".into());
        }
        return crate::kernel_ladder::run(rest);
    }
    let o = options(rest, "target/kernel-cases")?;
    let dir = root().join("tests/kernel-cases");
    if rest.contains(&"--coverage") {
        println!("{}", run_coverage(&o, &dir)?);
        return Ok(());
    }
    let list = cases(&dir, o.area.as_deref())?;
    if list.is_empty() {
        return Err(format!("no cases (recipe.json + expected.json) under {}", dir.display()));
    }
    let runs = run_on(&o, &list, &o.kernels);
    for (kernel, results) in o.kernels.iter().zip(&runs) {
        save_json(&o.out_of(kernel), "results.json", results)?;
    }
    if let Some(file) = &o.known_failing {
        // A gate run grades the run's one kernel against its baseline, and writes no report; its
        // results.json is still saved above.
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let base = file.parent().unwrap_or(&dir);
        let paths: Vec<String> = list.iter().map(|c| c.strip_prefix(base).unwrap_or(c).to_string_lossy().replace('\\', "/")).collect();
        let graded = runs.first().ok_or("--known-failing: no run to grade")?;
        let statuses: Vec<&str> = graded.iter().map(|r| r["status"].as_str().unwrap_or("?")).collect();
        println!("{}", gate(&text, &paths, &statuses).map_err(|e| format!("{}: {e}", file.display()))?);
        return Ok(());
    }
    for (kernel, results) in o.kernels.iter().zip(&runs) {
        let out = o.out_of(kernel);
        let to = if *kernel == "smlib" { PathBuf::from("docs/kernel-cases.md") } else { out.join("report.md") };
        let env = o.env(kernel).iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ");
        println!("{}", write_report(&dir, &list, results, &o.label(kernel), &env, &to)?);
    }
    if let [smlib, solidcore] = runs.as_slice() {
        let labels = [o.label(BACKENDS[0]), o.label(BACKENDS[1])];
        let (data, md) = side_by_side(smlib, solidcore, [&labels[0], &labels[1]]);
        // Routed runs keep their side by side with their results, so runs don't overwrite each other.
        let into = if o.routes.is_empty() { o.out.clone() } else { o.out_of(BACKENDS[1]) };
        save_json(&into, "backends.json", &data)?;
        if !o.routes.is_empty() {
            std::fs::write(into.join("backends.md"), &md).map_err(|e| format!("{}: {e}", into.join("backends.md").display()))?;
        }
        std::fs::write(root().join("docs/kernel-cases-backends.md"), md).map_err(|e| format!("docs/kernel-cases-backends.md: {e}"))?;
        println!(
            "kernel-cases: {} agree, {} differ → docs/kernel-cases-backends.md, {}",
            data["agree"],
            data["differ"],
            into.join("backends.json").display()
        );
    }
    Ok(())
}

/// The cases a known-failing baseline lists, with their causes: one per line, `<case> <cause>`,
/// the case's directory relative to the baseline's own; `#` starts a comment line. The list is
/// sorted with no case twice, and every case has a cause.
pub(crate) fn known_failing(text: &str) -> Result<Vec<(&str, &str)>, String> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (case, cause) = line.split_once(char::is_whitespace).map_or((line, ""), |(c, w)| (c, w.trim()));
        if cause.is_empty() {
            return Err(format!("line {}: {case} gives no cause (`<case> <why it fails today>`)", n + 1));
        }
        if let Some((last, _)) = out.last()
            && *last >= case
        {
            return Err(format!(
                "line {}: {case} is {} {last}: keep the list sorted, each case once",
                n + 1,
                if *last == case { "a repeat of" } else { "out of order after" }
            ));
        }
        out.push((case, cause));
    }
    Ok(out)
}

/// The regression gate: `cases` (paths relative to the baseline's directory) ran with
/// `statuses`; every case passes except those the baseline `text` lists. A case that fails and
/// is not listed is a regression; a listed case that passes is fixed and must leave the list, so
/// the list only shrinks. Either fails the gate, as does a listed case that is not in the set.
fn gate(text: &str, cases: &[String], statuses: &[&str]) -> Result<String, String> {
    let listed = known_failing(text)?;
    let mut problems = Vec::new();
    for (case, _) in &listed {
        if !cases.iter().any(|c| c == case) {
            problems.push(format!("listed but not a case of this set: {case}"));
        }
    }
    let mut failing = 0;
    for (case, status) in cases.iter().zip(statuses) {
        match (listed.iter().find(|(c, _)| c == case), *status == "pass") {
            (None, false) => problems.push(format!("REGRESSION ({status}): {case}")),
            (Some(_), true) => problems.push(format!("FIXED, remove it from the known-failing list: {case}")),
            (Some(_), false) => failing += 1,
            (None, true) => {}
        }
    }
    if problems.is_empty() {
        Ok(format!("kernel-cases gate: {} pass, {failing} known failing, no regression", cases.len() - failing))
    } else {
        Err(format!("kernel-cases gate:\n  {}", problems.join("\n  ")))
    }
}

/// The commands of the registry (`solidcraft-cli commands`): each id, with whether it is only
/// planned (registered, not built yet).
fn registry(o: &Options) -> Result<Vec<(String, bool)>, String> {
    let mut cmd = match &o.cli {
        Some(bin) => Command::new(bin),
        None => {
            let mut c = cargo();
            c.args(["run", "-q", "--release", "-p", "solidcraft-cli", "--"]);
            c
        }
    };
    let out = cmd.current_dir(root()).arg("commands").output().map_err(|e| format!("solidcraft-cli: {e}"))?;
    if !out.status.success() {
        return Err(format!("solidcraft-cli commands failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let list: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("commands JSON: {e}"))?;
    let list = list.as_array().ok_or("commands JSON: not a list")?;
    Ok(list.iter().filter_map(|c| Some((c["id"].as_str()?.to_string(), c["planned"].as_str().is_some_and(|p| !p.is_empty())))).collect())
}

/// Every command id the command scripts (`recipe.json`) under `dir` run, at any depth.
fn used_commands(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?.flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.file_name().is_some_and(|n| n == "recipe.json") {
                let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                out.extend(recipe_commands(&text).map_err(|e| format!("{}: {e}", p.display()))?);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The command ids a recipe runs: each `commands[].command` of a command script. An oracle
/// recipe (no `commands`) names features, not commands, and counts none.
fn recipe_commands(text: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let Some(list) = v.get("commands") else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or("`commands` is not a list")?;
    list.iter()
        .enumerate()
        .map(|(i, c)| c["command"].as_str().map(str::to_string).ok_or_else(|| format!("commands[{i}] has no `command` id")))
        .collect()
}

/// One line of `tests/kernel-cases/EXEMPT`: a command with no case (its reason is for readers).
#[derive(Debug, PartialEq)]
struct Exempt<'a> {
    id: &'a str,
    /// Listed under `[to cover]`: a geometry command still waiting for its case.
    to_cover: bool,
}

/// The section header of the commands that make geometry but have no case yet.
const TO_COVER: &str = "[to cover]";

/// The commands `tests/kernel-cases/EXEMPT` exempts from the coverage check: one per line,
/// `<id>  <why it has no case>`; `#` starts a comment line. The lines before `[to cover]` have no
/// geometry to grade; those after it make geometry and wait for a case, so that section only
/// shrinks. Each section is sorted, with no id twice in the file, and every line gives a reason.
fn exempt(text: &str) -> Result<Vec<Exempt<'_>>, String> {
    let mut out: Vec<Exempt> = Vec::new();
    let mut to_cover = false;
    let mut last: Option<&str> = None;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == TO_COVER {
            if to_cover {
                return Err(format!("line {}: a second {TO_COVER} section", n + 1));
            }
            to_cover = true;
            last = None;
            continue;
        }
        let (id, why) = line.split_once(char::is_whitespace).map_or((line, ""), |(c, w)| (c, w.trim()));
        if why.is_empty() {
            return Err(format!("line {}: {id} gives no reason (`<id>  <why it has no case>`)", n + 1));
        }
        if out.iter().any(|e| e.id == id) {
            return Err(format!("line {}: {id} is listed twice", n + 1));
        }
        if let Some(prev) = last
            && prev > id
        {
            return Err(format!("line {}: {id} is out of order after {prev}: keep each section sorted", n + 1));
        }
        last = Some(id);
        out.push(Exempt { id, to_cover });
    }
    Ok(out)
}

/// The coverage check: every built command of `registry` (id, planned) is in `used` (run by a
/// case) or exempted by `exempt_text`; an exemption names a registered command that no case
/// uses, so a case that comes removes its line.
fn coverage(registry: &[(String, bool)], used: &[String], exempt_text: &str) -> Result<String, String> {
    let listed = exempt(exempt_text)?;
    let mut problems = Vec::new();
    for e in &listed {
        if !registry.iter().any(|(id, _)| id == e.id) {
            problems.push(format!("EXEMPT lists {}, which is not a registry command: remove its line", e.id));
        } else if used.iter().any(|u| u == e.id) {
            problems.push(format!("EXEMPT lists {}, which a case now uses: remove its line", e.id));
        }
    }
    let mut covered = 0;
    for (id, _) in registry.iter().filter(|(_, planned)| !planned) {
        if used.iter().any(|u| u == id) {
            covered += 1;
        } else if !listed.iter().any(|e| e.id == id) {
            problems.push(format!("NO CASE: {id} (add a case under tests/kernel-cases/, or a line to EXEMPT)"));
        }
    }
    if !problems.is_empty() {
        return Err(format!("kernel-cases coverage:\n  {}", problems.join("\n  ")));
    }
    let to_cover = listed.iter().filter(|e| e.to_cover).count();
    Ok(format!("kernel-cases coverage: {covered} commands have a case, {} have no geometry, {to_cover} still to cover", listed.len() - to_cover))
}

fn run_coverage(o: &Options, dir: &Path) -> Result<String, String> {
    let file = dir.join("EXEMPT");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    coverage(&registry(o)?, &used_commands(dir)?, &text).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child that writes more than the pipe buffer still finishes, with all its output.
    #[cfg(unix)]
    #[test]
    fn a_child_writing_more_than_the_pipe_buffer_is_not_a_hang() {
        let child = Command::new("sh")
            .args(["-c", "head -c 200000 /dev/zero; head -c 100000 /dev/zero >&2"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = wait_draining(child, Duration::from_secs(60)).unwrap().unwrap();
        assert!(out.status.success());
        assert_eq!((out.stdout.len(), out.stderr.len()), (200_000, 100_000));
        let slow = Command::new("sh").args(["-c", "sleep 30"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let t = Instant::now();
        assert!(wait_draining(slow, Duration::from_millis(200)).unwrap().is_none());
        assert!(t.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn side_by_side_counts_and_puts_differences_first() {
        let r = |area: &str, case: &str, status: &str, s: f64| json!({"area": area, "case": case, "status": status, "seconds": s});
        let smlib = [r("booleans", "a", "pass", 1.0), r("booleans", "b", "pass", 2.0), r("shell", "c", "crash", 0.5)];
        let mut hang = json!({"area": "booleans", "case": "b", "status": "hang", "error": "no result after 180 s"});
        hang["process_seconds"] = json!(180.0);
        let solidcore = [r("booleans", "a", "pass", 0.5), hang, r("shell", "c", "fail", 1.5)];
        let (data, md) = side_by_side(&smlib, &solidcore, ["smlib", "solidcore"]);
        assert_eq!((data["agree"].as_u64(), data["differ"].as_u64()), (Some(1), Some(2)));
        assert_eq!(data["summary"]["smlib"]["counts"], json!({"pass": 2, "other": 1}));
        assert_eq!(data["summary"]["solidcore"]["counts"], json!({"pass": 1, "fail": 1, "other": 1}));
        assert_eq!(data["summary"]["solidcore"]["seconds"].as_f64(), Some(182.0));
        let b = data["cases"].as_array().and_then(|c| c.iter().find(|c| c["case"] == "b")).cloned().unwrap_or_default();
        assert_eq!((b["solidcore"]["status"].as_str(), b["solidcore"]["error"].as_str()), (Some("hang"), Some("no result after 180 s")));
        // In booleans, b (differs) comes before a (agrees).
        let (pos_a, pos_b) = (md.find("| a |"), md.find("| b |"));
        assert!(pos_b.is_some() && pos_a.is_some() && pos_b < pos_a, "{md}");
        assert!(md.contains("| b | pass (2.00 s) | hang (180.00 s) | **differ** |"), "{md}");
        assert!(md.contains("3 cases: 1 agree, 2 differ."), "{md}");
        let (data, md) = side_by_side(&smlib, &[], ["smlib", "solidcore[boolean,measure]"]);
        assert_eq!(data["labels"], json!(["smlib", "solidcore[boolean,measure]"]));
        assert!(md.contains("| solidcore[boolean,measure] | 0 | 0 | 3 |") && md.contains("| Case | smlib | solidcore[boolean,measure] | |"), "{md}");
    }

    /// `--route` moves the named families on the solidcore side only: SMLib with
    /// SOLIDCRAFT_KERNEL_ROUTES, labelled and kept in its own folder.
    #[test]
    fn routes_label_the_solidcore_side_and_keep_its_own_folder() {
        let o = options(&["--kernel", "both", "--route", "measure=solidcore, boolean=solidcore", "--out", "/r"], "x").unwrap();
        assert_eq!(o.routes, ["measure", "boolean"]);
        assert_eq!((o.label("smlib"), o.label("solidcore")), ("smlib".into(), "solidcore[measure,boolean]".into()));
        assert_eq!(o.env("smlib"), [("SOLIDCRAFT_KERNEL", "smlib".to_string())]);
        assert_eq!(
            o.env("solidcore"),
            [("SOLIDCRAFT_KERNEL", "smlib".to_string()), ("SOLIDCRAFT_KERNEL_ROUTES", "measure=solidcore,boolean=solidcore".to_string())]
        );
        assert_eq!((o.out_of("smlib"), o.out_of("solidcore")), (PathBuf::from("/r"), PathBuf::from("/r/solidcore-measure-boolean")));
        let plain = options(&["--kernel", "both", "--out", "/r"], "x").unwrap();
        assert_eq!((plain.label("solidcore"), plain.out_of("solidcore")), ("solidcore".into(), PathBuf::from("/r/solidcore")));
        assert_eq!(plain.env("solidcore"), [("SOLIDCRAFT_KERNEL", "solidcore".to_string())]);
        // The default run is on solidcore, so it takes `--route` as `both` and `solidcore` do.
        assert_eq!(options(&["--route", "file=solidcore"], "x").unwrap().routes, ["file"]);
        for bad in [
            &["--kernel", "both", "--route", "truck=solidcore"][..],
            &["--kernel", "both", "--route", "boolean=smlib"],
            &["--kernel", "both", "--route", "boolean"],
            &["--kernel", "both", "--route", ""],
            &["--kernel", "both", "--route"],
            &["--kernel", "smlib", "--route", "boolean=solidcore"],
        ] {
            assert!(options(bad, "x").is_err(), "{bad:?}");
        }
    }

    /// `--route` knows the families route.rs routes, `file` aside.
    #[test]
    fn route_ops_are_the_routing_tables_families() {
        let text = std::fs::read_to_string(root().join("crates/smlib/src/route.rs")).unwrap_or_default();
        let ids: Vec<&str> = text.lines().filter_map(|l| l.trim().strip_prefix("Op::")?.split_once(" => \"")?.1.strip_suffix("\",")).collect();
        assert_eq!(ids, ROUTE_OPS, "{ids:?}");
    }

    /// `--known-failing` follows the kernel given, solidcore by default: a list is one kernel's.
    #[test]
    fn the_gate_grades_the_kernel_given() {
        for (args, kernel) in [
            (&["--known-failing", "/k"][..], "solidcore"),
            (&["--kernel", "solidcore", "--known-failing", "/k"], "solidcore"),
            (&["--kernel", "smlib", "--known-failing", "/k"], "smlib"),
        ] {
            let o = options(args, "x").unwrap();
            assert_eq!((o.known_failing, o.kernels), (Some(PathBuf::from("/k")), vec![kernel]), "{args:?}");
        }
        assert!(options(&["--kernel", "both", "--known-failing", "/k"], "x").is_err());
    }

    /// With no `--kernel`, a run is on solidcore; the C++ kernel is asked for.
    #[test]
    fn the_rig_defaults_to_solidcore() {
        assert_eq!(options(&[], "x").unwrap().kernels, ["solidcore"]);
        assert_eq!(options(&["--kernel", "smlib"], "x").unwrap().kernels, ["smlib"]);
        assert_eq!(options(&["--kernel", "both"], "x").unwrap().kernels, ["smlib", "solidcore"]);
        assert_eq!(options(&[], "x").unwrap().env("solidcore"), [("SOLIDCRAFT_KERNEL", "solidcore".to_string())]);
    }

    fn gate_on(text: &str, cases: &[(&str, &str)]) -> Result<String, String> {
        let names: Vec<String> = cases.iter().map(|(c, _)| c.to_string()).collect();
        let statuses: Vec<&str> = cases.iter().map(|(_, s)| *s).collect();
        gate(text, &names, &statuses)
    }

    #[test]
    fn the_gate_fails_on_a_regression_and_on_a_fixed_case_still_listed() {
        let list = "# today\na-case SMLib refuses the corner\nb-case G1 at the corner point\n";
        assert!(gate_on(list, &[("a-case", "fail"), ("b-case", "crash"), ("c-case", "pass")]).is_ok());
        let regression = gate_on(list, &[("a-case", "fail"), ("b-case", "fail"), ("c-case", "hang")]).unwrap_err();
        assert!(regression.contains("REGRESSION (hang): c-case"), "{regression}");
        let fixed = gate_on(list, &[("a-case", "pass"), ("b-case", "fail"), ("c-case", "pass")]).unwrap_err();
        assert!(fixed.contains("FIXED, remove it from the known-failing list: a-case"), "{fixed}");
        let stale = gate_on(list, &[("a-case", "fail"), ("c-case", "pass")]).unwrap_err();
        assert!(stale.contains("not a case of this set: b-case"), "{stale}");
    }

    #[test]
    fn the_known_failing_list_is_sorted_with_a_cause_each() {
        assert!(gate_on("b x\na y\n", &[("a", "fail"), ("b", "fail")]).unwrap_err().contains("out of order"));
        assert!(gate_on("a x\na y\n", &[("a", "fail")]).unwrap_err().contains("a repeat of"));
        assert!(gate_on("a\n", &[("a", "fail")]).unwrap_err().contains("gives no cause"));
    }

    #[test]
    fn a_recipe_names_the_commands_of_its_script() {
        let script = r#"{"base": "../a", "commands": [{"command": "solid.box", "params": {}}, {"command": "solid.fillet"}]}"#;
        assert_eq!(recipe_commands(script).unwrap(), ["solid.box", "solid.fillet"]);
        assert!(recipe_commands(r#"{"id": "01-box", "features": []}"#).unwrap().is_empty());
        assert!(recipe_commands(r#"{"commands": [{"params": {}}]}"#).unwrap_err().contains("no `command` id"));
        assert!(recipe_commands(r#"{"commands": {}}"#).unwrap_err().contains("not a list"));
    }

    #[test]
    fn the_exempt_list_has_two_sorted_sections_with_a_reason_each() {
        let text = "# header\nview.fit  camera only\nview.top  camera only\n\n[to cover]\nsolid.cone  surfaces area\n";
        assert_eq!(
            exempt(text).unwrap(),
            [Exempt { id: "view.fit", to_cover: false }, Exempt { id: "view.top", to_cover: false }, Exempt { id: "solid.cone", to_cover: true },]
        );
        assert!(exempt("view.top x\nview.fit x\n").unwrap_err().contains("out of order"));
        assert!(exempt("view.fit x\n[to cover]\nview.fit y\n").unwrap_err().contains("listed twice"));
        assert!(exempt("view.fit\n").unwrap_err().contains("gives no reason"));
        assert!(exempt("[to cover]\n[to cover]\n").unwrap_err().contains("second"));
    }

    #[test]
    fn coverage_fails_on_a_command_with_no_case_and_on_a_stale_exemption() {
        let registry: Vec<(String, bool)> =
            [("solid.box", false), ("solid.cone", false), ("view.fit", false), ("surface.fillet", true)].map(|(i, p)| (i.to_string(), p)).to_vec();
        let used = ["solid.box".to_string()];
        let list = "view.fit  camera only\n[to cover]\nsolid.cone  surfaces area\n";
        let ok = coverage(&registry, &used, list).unwrap();
        assert!(ok.contains("1 commands have a case, 1 have no geometry, 1 still to cover"), "{ok}");
        let missing = coverage(&registry, &used, "view.fit  camera only\n").unwrap_err();
        assert!(missing.contains("NO CASE: solid.cone"), "{missing}");
        assert!(!missing.contains("surface.fillet"), "a planned command needs no case: {missing}");
        let covered = coverage(&registry, &["solid.box".into(), "solid.cone".into()], list).unwrap_err();
        assert!(covered.contains("EXEMPT lists solid.cone, which a case now uses"), "{covered}");
        let gone = coverage(&registry, &used, "view.fit  camera only\nview.gone  removed\n[to cover]\nsolid.cone  x\n").unwrap_err();
        assert!(gone.contains("view.gone, which is not a registry command"), "{gone}");
    }
}
