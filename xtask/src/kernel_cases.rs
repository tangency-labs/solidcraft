//! `cargo xtask kernel-cases`: the kernel regression rig. Runs every case in
//! `tests/kernel-cases/<area>/<id>/` through `solidcraft-cli kernel-cases` on SMLib, one process
//! per case (so a crash or a hang is that case's result, not the run's), and writes
//! `docs/kernel-cases.md`: results per case, coverage per area and per source, and the issues and
//! PRs looked at and skipped. Each case is graded against its own answer.
//!
//! The run's output folder (`--out`, default `target/kernel-cases`) holds `results.json`, every
//! case's full result, and `dumps/<area>-<case>/`, the inputs of each SMLib call a case's run
//! failed in (`solidcraft-cli kernel-cases --dumps`), for replay.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{cargo, root};

const DEFAULT_TIMEOUT_S: u64 = 180;

/// How many cases ended in each status.
type StatusCounts = BTreeMap<String, usize>;

pub(crate) struct Options {
    cli: Option<String>,
    area: Option<String>,
    timeout: Duration,
    pub(crate) jobs: usize,
    /// The run's output folder: `results.json` and the SMLib dumps (`dumps/`).
    out: PathBuf,
    /// `--known-failing FILE`: run as a regression gate against that baseline (see [`gate`]).
    known_failing: Option<PathBuf>,
    /// `--validation MODE`: when SMLib's full check runs on the cases' features (each_step,
    /// each_feature, off; the CLI's default, each_feature, when not given).
    validation: Option<String>,
}

/// The run's options; `out` is its output folder unless `--out` gives one.
pub(crate) fn options(rest: &[&str], out: &str) -> Result<Options, String> {
    let arg = |name: &str| rest.iter().position(|a| *a == name).and_then(|i| rest.get(i + 1)).map(|s| s.to_string());
    let secs = arg("--timeout").map(|t| t.parse::<u64>().map_err(|e| format!("--timeout: {e}"))).transpose()?.unwrap_or(DEFAULT_TIMEOUT_S);
    let jobs = arg("--jobs").map(|j| j.parse::<usize>().map_err(|e| format!("--jobs: {e}"))).transpose()?.unwrap_or(4).clamp(1, 32);
    let out = arg("--out").map_or_else(|| root().join(out), PathBuf::from);
    let known_failing = arg("--known-failing").map(|p| {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { root().join(p) }
    });
    let validation = arg("--validation");
    if let Some(v) = validation.as_deref().filter(|v| !["each_step", "each_feature", "off"].contains(v)) {
        return Err(format!("--validation must be each_step, each_feature or off, not `{v}`"));
    }
    Ok(Options { cli: arg("--cli"), area: arg("--area"), timeout: Duration::from_secs(secs), jobs, out, known_failing, validation })
}

/// Every case's full result as JSON, into the run's output folder.
pub(crate) fn save_results(o: &Options, results: &[Value]) -> Result<(), String> {
    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let path = o.out.join("results.json");
    let text = serde_json::to_string_pretty(results).map_err(|e| e.to_string())?;
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
fn run_one(o: &Options, case: &Path) -> Value {
    let mut r = run_case(o, case);
    let base = root().join("tests/kernel-cases");
    r["path"] = json!(case.strip_prefix(&base).unwrap_or(case).to_string_lossy().replace('\\', "/"));
    r
}

fn run_case(o: &Options, case: &Path) -> Value {
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
    dumps.push(o.out.join("dumps"));
    cmd.current_dir(root()).args(["kernel-cases", "--json"]).arg(dumps);
    if let Some(v) = &o.validation {
        cmd.arg(format!("--validation={v}"));
    }
    cmd.arg(&rel).stdout(Stdio::piped()).stderr(Stdio::piped());
    let base = |status: &str, error: String| {
        let area = case.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let id = case.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        json!({"case": id, "area": area, "status": status, "error": error})
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
    let next = Arc::new(Mutex::new(0usize));
    let results = Arc::new(Mutex::new(vec![Value::Null; list.len()]));
    std::thread::scope(|s| {
        for _ in 0..o.jobs.min(list.len().max(1)) {
            let (next, results) = (Arc::clone(&next), Arc::clone(&results));
            s.spawn(move || {
                loop {
                    let i = {
                        let Ok(mut n) = next.lock() else { return };
                        let i = *n;
                        *n += 1;
                        i
                    };
                    let Some(case) = list.get(i) else { return };
                    let r = run_one(o, case);
                    println!("{:<11} {}", r["status"].as_str().unwrap_or("?").to_uppercase(), case.strip_prefix(root()).unwrap_or(case).display());
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

fn write_report(dir: &Path, list: &[PathBuf], results: &[Value]) -> Result<String, String> {
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
    std::fs::write(root().join("docs/kernel-cases.md"), md).map_err(|e| format!("docs/kernel-cases.md: {e}"))?;
    let pass = results.iter().filter(|r| r["status"] == "pass").count();
    Ok(format!("kernel-cases: {pass}/{} pass → docs/kernel-cases.md ({skipped} skipped sources listed)", list.len()))
}

pub fn run(rest: &[&str]) -> Result<(), String> {
    if rest.contains(&"--ladder") {
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
    let results = run_all(&o, &list);
    save_results(&o, &results)?;
    if let Some(file) = &o.known_failing {
        // A gate run grades one set against its baseline; the report stays the full run's.
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let base = file.parent().unwrap_or(&dir);
        let paths: Vec<String> = list.iter().map(|c| c.strip_prefix(base).unwrap_or(c).to_string_lossy().replace('\\', "/")).collect();
        let statuses: Vec<&str> = results.iter().map(|r| r["status"].as_str().unwrap_or("?")).collect();
        println!("{}", gate(&text, &paths, &statuses).map_err(|e| format!("{}: {e}", file.display()))?);
        return Ok(());
    }
    println!("{}", write_report(&dir, &list, &results)?);
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
    use super::{Exempt, coverage, exempt, gate, recipe_commands};

    fn run(text: &str, cases: &[(&str, &str)]) -> Result<String, String> {
        let names: Vec<String> = cases.iter().map(|(c, _)| c.to_string()).collect();
        let statuses: Vec<&str> = cases.iter().map(|(_, s)| *s).collect();
        gate(text, &names, &statuses)
    }

    #[test]
    fn the_gate_fails_on_a_regression_and_on_a_fixed_case_still_listed() {
        let list = "# today\na-case SMLib refuses the corner\nb-case G1 at the corner point\n";
        assert!(run(list, &[("a-case", "fail"), ("b-case", "crash"), ("c-case", "pass")]).is_ok());
        let regression = run(list, &[("a-case", "fail"), ("b-case", "fail"), ("c-case", "hang")]).unwrap_err();
        assert!(regression.contains("REGRESSION (hang): c-case"), "{regression}");
        let fixed = run(list, &[("a-case", "pass"), ("b-case", "fail"), ("c-case", "pass")]).unwrap_err();
        assert!(fixed.contains("FIXED, remove it from the known-failing list: a-case"), "{fixed}");
        let stale = run(list, &[("a-case", "fail"), ("c-case", "pass")]).unwrap_err();
        assert!(stale.contains("not a case of this set: b-case"), "{stale}");
    }

    #[test]
    fn the_known_failing_list_is_sorted_with_a_cause_each() {
        assert!(run("b x\na y\n", &[("a", "fail"), ("b", "fail")]).unwrap_err().contains("out of order"));
        assert!(run("a x\na y\n", &[("a", "fail")]).unwrap_err().contains("a repeat of"));
        assert!(run("a\n", &[("a", "fail")]).unwrap_err().contains("gives no cause"));
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
