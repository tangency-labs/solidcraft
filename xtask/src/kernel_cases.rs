//! `cargo xtask kernel-cases`: the kernel regression rig. Runs every case in
//! `tests/kernel-cases/<area>/<id>/` through `solidcraft-cli kernel-cases`, one process per case
//! (so a crash or a hang is that case's result, not the run's), with each kernel asked for, and
//! writes `docs/kernel-cases.md`: results per case and kernel, coverage per area and per source,
//! and the issues and PRs looked at and skipped. Each kernel is graded against the cases' own
//! answers, never against the other kernel.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{cargo, root};

const DEFAULT_TIMEOUT_S: u64 = 180;

/// Per kernel, how many cases ended in each status.
type StatusCounts = BTreeMap<String, BTreeMap<String, usize>>;

pub(crate) struct Options {
    pub(crate) kernels: Vec<String>,
    cli: Option<String>,
    area: Option<String>,
    timeout: Duration,
    pub(crate) jobs: usize,
    /// Where to keep every case's full result as JSON.
    results: Option<String>,
}

pub(crate) fn options(rest: &[&str]) -> Result<Options, String> {
    let arg = |name: &str| rest.iter().position(|a| *a == name).and_then(|i| rest.get(i + 1)).map(|s| s.to_string());
    let kernels = match arg("--kernel").as_deref() {
        None | Some("default") => vec!["default".to_string()],
        Some("smlib") => vec!["smlib".to_string()],
        Some("both") => vec!["default".to_string(), "smlib".to_string()],
        Some(k) => return Err(format!("--kernel {k}: default, smlib or both")),
    };
    let secs = arg("--timeout").map(|t| t.parse::<u64>().map_err(|e| format!("--timeout: {e}"))).transpose()?.unwrap_or(DEFAULT_TIMEOUT_S);
    let jobs = arg("--jobs").map(|j| j.parse::<usize>().map_err(|e| format!("--jobs: {e}"))).transpose()?.unwrap_or(4).clamp(1, 32);
    Ok(Options { kernels, cli: arg("--cli"), area: arg("--area"), timeout: Duration::from_secs(secs), jobs, results: arg("--results") })
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

/// One case on one kernel, in its own process, killed after the timeout; `path` is the case's
/// directory under `tests/kernel-cases/`.
fn run_one(o: &Options, kernel: &str, case: &Path) -> Value {
    let mut r = run_case(o, kernel, case);
    let base = root().join("tests/kernel-cases");
    r["path"] = json!(case.strip_prefix(&base).unwrap_or(case).to_string_lossy().replace('\\', "/"));
    r
}

fn run_case(o: &Options, kernel: &str, case: &Path) -> Value {
    let rel = case.strip_prefix(root()).unwrap_or(case).to_path_buf();
    let mut cmd = match &o.cli {
        Some(bin) => Command::new(bin),
        None => {
            let mut c = cargo();
            c.args(["run", "-q", "--release", "-p", "solidcraft-cli", "--"]);
            c
        }
    };
    cmd.current_dir(root()).args(["kernel-cases", "--json"]).arg(&rel).stdout(Stdio::piped()).stderr(Stdio::piped());
    if kernel == "default" {
        cmd.env_remove("SOLIDCRAFT_BOOLEANS");
    } else {
        cmd.env("SOLIDCRAFT_BOOLEANS", kernel);
    }
    let base = |status: &str, error: String| {
        let area = case.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = case.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        json!({"case": id, "area": area, "kernel": kernel, "status": status, "error": error})
    };
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return base("fail", format!("solidcraft-cli: {e}")),
    };
    // A case may need longer than the run's timeout (`timeout_s`, at most 30 min).
    let timeout = read(&case.join("expected.json"))["timeout_s"].as_u64().map_or(o.timeout, |s| Duration::from_secs(s.min(1800)).max(o.timeout));
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return base("hang", format!("no result after {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return base("fail", format!("wait: {e}")),
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return base("fail", format!("output: {e}")),
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

pub(crate) fn run_all(o: &Options, list: &[PathBuf]) -> Vec<Value> {
    let jobs: Vec<(String, PathBuf)> = o.kernels.iter().flat_map(|k| list.iter().map(move |c| (k.clone(), c.clone()))).collect();
    let next = Arc::new(Mutex::new(0usize));
    let results = Arc::new(Mutex::new(vec![Value::Null; jobs.len()]));
    std::thread::scope(|s| {
        for _ in 0..o.jobs.min(jobs.len().max(1)) {
            let (next, results, jobs) = (Arc::clone(&next), Arc::clone(&results), &jobs);
            s.spawn(move || {
                loop {
                    let i = {
                        let Ok(mut n) = next.lock() else { return };
                        let i = *n;
                        *n += 1;
                        i
                    };
                    let Some((kernel, case)) = jobs.get(i) else { return };
                    let r = run_one(o, kernel, case);
                    println!(
                        "{:<11} {} [{kernel}]",
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
    Arc::try_unwrap(results).ok().and_then(|m| m.into_inner().ok()).unwrap_or_default()
}

pub(crate) fn cell(r: Option<&Value>) -> String {
    let Some(r) = r else { return String::new() };
    let status = r["status"].as_str().unwrap_or("?");
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

fn write_report(o: &Options, dir: &Path, list: &[PathBuf], results: &[Value]) -> Result<String, String> {
    let key = |r: &Value| (r["area"].as_str().unwrap_or("").to_string(), r["case"].as_str().unwrap_or("").to_string());
    let mut by_case: BTreeMap<(String, String), BTreeMap<String, Value>> = BTreeMap::new();
    for r in results {
        by_case.entry(key(r)).or_default().insert(r["kernel"].as_str().unwrap_or("").to_string(), r.clone());
    }
    // Coverage: per area, cases and their statuses per kernel; per source, its cases.
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
        let runs = by_case.get(&(area.clone(), id.clone()));
        for k in &o.kernels {
            let status = runs.and_then(|r| r.get(k)).and_then(|r| r["status"].as_str()).unwrap_or("not run").to_string();
            *entry.1.entry(k.clone()).or_default().entry(status).or_default() += 1;
        }
        let sources = sources_of(&want);
        for s in &sources {
            per_source.entry(s.clone()).or_default().push(format!("{area}/{id}"));
        }
        *kinds.entry(want["source"].as_str().unwrap_or("?").split_whitespace().next().unwrap_or("?").to_string()).or_default() += 1;
        let cells: Vec<String> = o.kernels.iter().map(|k| cell(runs.and_then(|r| r.get(k)))).collect();
        rows += &format!(
            "| {area}/{id} | {} | {} | {} | {} |\n",
            sources.iter().map(|s| link(s)).collect::<Vec<_>>().join(" "),
            want["what"].as_str().unwrap_or("").replace('|', "/"),
            want["source"].as_str().unwrap_or("?"),
            cells.join(" | ")
        );
    }
    let mut md = String::from(
        "# Kernel regression rig\n\nGenerated by `cargo xtask kernel-cases`. Every geometry case the old kernel (truck) got wrong, as an issue, a PR or a crash report recorded it, is a use case in [`tests/kernel-cases/`](../tests/kernel-cases/) with its own right answer: numbers from analysis or a hand calculation (**analytic**) where the geometry has them, and otherwise the invariants (**invariants**): the operation succeeds, or refuses cleanly where that is the right answer; each body is one closed shell that passes the record's structure and geometry checks and, in builds with SMLib, SMLib's own check; the expected face, edge and vertex counts. Never a kernel's output, and never a commercial kernel's.\n\nEach kernel is graded against the case, never against the other kernel. **default** is the kernel SolidCraft ships today; **smlib** runs with `SOLIDCRAFT_BOOLEANS=smlib`. A failing case stays failing until its kernel is fixed; there is no workaround on SolidCraft's side. See [`tests/kernel-cases/README.md`](../tests/kernel-cases/README.md) for the format.\n\n",
    );
    md += "## Coverage\n\n| Area | Cases |";
    for k in &o.kernels {
        md += &format!(" {k} |");
    }
    md += "\n|---|---|";
    md += &"---|".repeat(o.kernels.len());
    md += "\n";
    for (area, (n, statuses)) in &areas {
        md += &format!("| {area} | {n} |");
        for k in &o.kernels {
            let s = statuses.get(k).map(|m| m.iter().map(|(s, c)| format!("{c} {s}")).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            md += &format!(" {s} |");
        }
        md += "\n";
    }
    md += &format!(
        "\n{} cases from {} sources; answers: {}.\n\n## Results\n\n| Case | Sources | What went wrong | Answer |",
        list.len(),
        per_source.len(),
        kinds.iter().map(|(k, v)| format!("{v} {k}")).collect::<Vec<_>>().join(", ")
    );
    for k in &o.kernels {
        md += &format!(" {k} |");
    }
    md += "\n|---|---|---|---|";
    md += &"---|".repeat(o.kernels.len());
    md += "\n";
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
    std::fs::write(root().join("docs/kernel-cases.md"), md).map_err(|e| format!("docs/kernel-cases.md: {e}"))?;
    let summary: Vec<String> = o
        .kernels
        .iter()
        .map(|k| {
            let n = results.iter().filter(|r| r["kernel"] == k.as_str() && r["status"] == "pass").count();
            format!("{k}: {n}/{} pass", list.len())
        })
        .collect();
    Ok(format!("kernel-cases: {} → docs/kernel-cases.md ({skipped} skipped sources listed)", summary.join(", ")))
}

pub fn run(rest: &[&str]) -> Result<(), String> {
    if rest.contains(&"--ladder") {
        return crate::kernel_ladder::run(rest);
    }
    let o = options(rest)?;
    let dir = root().join("tests/kernel-cases");
    let list = cases(&dir, o.area.as_deref())?;
    if list.is_empty() {
        return Err(format!("no cases (recipe.json + expected.json) under {}", dir.display()));
    }
    let results = run_all(&o, &list);
    if let Some(path) = &o.results {
        let text = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    println!("{}", write_report(&o, &dir, &list, &results)?);
    Ok(())
}
