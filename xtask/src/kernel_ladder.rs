//! `cargo xtask kernel-cases --ladder [--record REASON | --gate]`: the kernel complexity ladder.
//!
//! `tests/kernel-cases/ladder/ladder.json` lists tiers in order of complexity and, per tier, its
//! cases in order (paths under `tests/kernel-cases/`, so a tier may also list rig cases from other
//! areas). Each case is a test like any rig case, graded against its own `expected.json`, on
//! `--kernel`'s one backend. The golden is SMLib's, so a gate or a record says `--kernel smlib`. On top, `tests/kernel-cases/ladder/golden.json` records per case the status, the
//! commands' time and what was made (counts, volume, area): the run compares with it and flags
//! slowdowns and changes; cases run one at a time unless `--jobs` says otherwise, so the times are
//! not shared with other cases. `--record REASON` re-measures the golden and re-sorts each tier
//! by time, fastest first; both files are committed. The run's output folder (`--out`, default
//! `target/kernel-ladder`) holds `results.json` and the SMLib dumps, as the rig's does.
//! `--gate` makes the run a regression gate instead of the report: it fails on any case whose
//! status or counts differ from the golden (times aside, which are the machine's).
//!
//! The report, `docs/kernel-ladder.md`, gives per tier the counts per status and the robustness
//! frontier: the first case, in ladder order, that fails.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::kernel_cases::{cell, options, read, run_all, save_results};
use crate::root;

/// A case is flagged slower when it takes more than this many times its golden time…
const SLOWDOWN_FACTOR: f64 = 2.0;
/// …and at least this many seconds more (timer noise on fast cases is not a slowdown).
const SLOWDOWN_MIN_S: f64 = 0.25;

struct Tier {
    tier: String,
    what: String,
    cases: Vec<String>,
}

fn manifest_path() -> PathBuf {
    root().join("tests/kernel-cases/ladder/ladder.json")
}

fn golden_path() -> PathBuf {
    root().join("tests/kernel-cases/ladder/golden.json")
}

fn tiers(manifest: &Value) -> Result<Vec<Tier>, String> {
    let list = manifest["tiers"].as_array().ok_or("ladder.json: `tiers` must be an array")?;
    let mut out = Vec::new();
    for t in list {
        let tier = t["tier"].as_str().ok_or("ladder.json: a tier without `tier`")?.to_string();
        let cases = t["cases"]
            .as_array()
            .ok_or_else(|| format!("ladder.json: tier {tier} without `cases`"))?
            .iter()
            .map(|c| c.as_str().map(str::to_string).ok_or_else(|| format!("ladder.json: tier {tier}: a case that is not a path")))
            .collect::<Result<Vec<_>, _>>()?;
        out.push(Tier { tier, what: t["what"].as_str().unwrap_or("").to_string(), cases });
    }
    Ok(out)
}

/// Every case directory under `ladder/` is listed once in the manifest, and every listed case
/// exists: a case left out would never run.
fn check_listed(tiers: &[Tier]) -> Result<(), String> {
    let base = root().join("tests/kernel-cases");
    let mut listed: BTreeMap<&str, usize> = BTreeMap::new();
    for t in tiers {
        for c in &t.cases {
            *listed.entry(c.as_str()).or_default() += 1;
            let d = base.join(c);
            if !(d.join("recipe.json").is_file() && d.join("expected.json").is_file()) {
                return Err(format!("ladder.json: {c} has no recipe.json and expected.json"));
            }
        }
    }
    if let Some((c, _)) = listed.iter().find(|(_, n)| **n > 1) {
        return Err(format!("ladder.json: {c} is listed more than once"));
    }
    let ladder = base.join("ladder");
    for tier in std::fs::read_dir(&ladder).map_err(|e| format!("{}: {e}", ladder.display()))?.flatten() {
        if !tier.path().is_dir() {
            continue;
        }
        for case in std::fs::read_dir(tier.path()).map_err(|e| e.to_string())?.flatten() {
            let rel = case.path().strip_prefix(&base).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if case.path().join("recipe.json").is_file() && !listed.contains_key(rel.as_str()) {
                return Err(format!("ladder.json: {rel} is not listed in any tier"));
            }
        }
    }
    Ok(())
}

fn rel_of(r: &Value) -> String {
    r["path"].as_str().unwrap_or("").to_string()
}

/// The golden entry for one result: status, the commands' time, what was made.
fn golden_entry(r: &Value) -> Value {
    let mut g = json!({"status": r["status"], "seconds": r["seconds"]});
    if let Some(m) = r.get("measured").filter(|m| m.is_object()) {
        g["measured"] = m.clone();
    }
    g
}

/// What changed against the golden: a slowdown, a status change, other counts.
fn flags(r: &Value, g: Option<&Value>) -> Vec<String> {
    let mut out = changes(r, g);
    if let Some(g) = g
        && let (Some(now), Some(then)) = (r["seconds"].as_f64(), g["seconds"].as_f64())
        && now > then * SLOWDOWN_FACTOR
        && now - then > SLOWDOWN_MIN_S
    {
        out.push(format!("slower: {now:.2} s (golden {then:.2} s)"));
    }
    out
}

/// What changed against the golden that the machine does not explain: no golden, a status
/// change, other counts. Times are left out (they depend on the machine the golden was recorded
/// on), so these are what the gate fails on.
fn changes(r: &Value, g: Option<&Value>) -> Vec<String> {
    let Some(g) = g else { return vec!["no golden".into()] };
    let mut out = Vec::new();
    if g["status"] != r["status"] {
        out.push(format!("status was {}", g["status"].as_str().unwrap_or("?")));
    }
    for k in ["body_count", "faces", "edges", "vertices"] {
        let (now, then) = (&r["measured"][k], &g["measured"][k]);
        if !now.is_null() && !then.is_null() && now != then {
            out.push(format!("{k} {now} (golden {then})"));
        }
    }
    out
}

/// The ladder's regression gate (`--gate`): every case's status and counts are the golden's. A
/// case that changed fails it, better or worse, as does a case with no golden: the change is
/// re-recorded with its reason (`--record`) in the same pull request.
fn gate(tiers: &[Tier], results: &BTreeMap<String, Value>, golden: &Value) -> Result<String, String> {
    let mut problems = Vec::new();
    let mut pass = 0;
    for c in tiers.iter().flat_map(|t| &t.cases) {
        let Some(r) = results.get(c) else {
            problems.push(format!("{c}: no result"));
            continue;
        };
        pass += usize::from(r["status"] == "pass");
        let changed = changes(r, golden["cases"].get(c));
        if !changed.is_empty() {
            problems.push(format!("CHANGED ({}): {c}: {}", r["status"].as_str().unwrap_or("?"), changed.join("; ")));
        }
    }
    let n: usize = tiers.iter().map(|t| t.cases.len()).sum();
    if problems.is_empty() {
        Ok(format!("kernel ladder gate: {pass}/{n} pass, every case as the golden records it"))
    } else {
        Err(format!(
            "kernel ladder gate: {} case(s) differ from tests/kernel-cases/ladder/golden.json (re-record with --record \"why\" if the change is right):\n  {}",
            problems.len(),
            problems.join("\n  ")
        ))
    }
}

fn secs(v: &Value) -> String {
    v.as_f64().map(|s| format!("{s:.3}")).unwrap_or_else(|| "–".into())
}

fn write_report(tiers: &[Tier], results: &BTreeMap<String, Value>, golden: &Value) -> Result<String, String> {
    let mut md = String::from(
        "# Kernel complexity ladder\n\nGenerated by `cargo xtask kernel-cases --ladder`. The ladder in [`tests/kernel-cases/ladder/`](../tests/kernel-cases/ladder/) orders kernel operations by complexity: each tier builds on the shapes of the tiers before it, so the last case carries everything. Every case is a test with its own right answer (analysis or invariants, as in the [regression rig](kernel-cases.md)); the golden (`tests/kernel-cases/ladder/golden.json`) records each case's time and what it made, orders each tier by time and flags slowdowns. The cases run on SMLib.\n\nThe **frontier** is the first case of a tier, in ladder order, that fails (fail, crash or hang).\n\n## Tiers\n\n| Tier | What | Cases | Results | Frontier |\n|---|---|---|---|---|\n",
    );
    let mut rows = String::new();
    let mut pass = 0;
    for t in tiers {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut frontier = None;
        for c in &t.cases {
            let status = results.get(c).and_then(|r| r["status"].as_str()).unwrap_or("not run").to_string();
            if frontier.is_none() && matches!(status.as_str(), "fail" | "crash" | "hang") {
                frontier = Some(c.rsplit('/').next().unwrap_or(c).to_string());
            }
            pass += usize::from(status == "pass");
            *counts.entry(status).or_default() += 1;
        }
        let counts = counts.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ");
        md += &format!("| {} | {} | {} | {counts} | {} |\n", t.tier, t.what.replace('|', "/"), t.cases.len(), frontier.unwrap_or_else(|| "–".into()));
        for c in &t.cases {
            let want = read(&root().join("tests/kernel-cases").join(c).join("expected.json"));
            let r = results.get(c);
            let g = golden["cases"].get(c);
            let f = r.map(|r| flags(r, g)).unwrap_or_default();
            rows += &format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                c,
                want["what"].as_str().unwrap_or("").replace('|', "/"),
                want["source"].as_str().unwrap_or("?"),
                cell(r),
                r.map(|r| secs(&r["seconds"])).unwrap_or_default(),
                g.map(|g| secs(&g["seconds"])).unwrap_or_else(|| "–".into()),
                f.join("; ").replace('|', "/")
            );
        }
    }
    // Cases whose bodies SMLib's check gave no verdict on (it can't take the record yet).
    let unchecked = results
        .values()
        .filter(|r| r["checks"].as_array().into_iter().flatten().any(|c| c["got"].as_str().is_some_and(|g| g.starts_with("no verdict"))))
        .count();
    md += &format!(
        "\nCases where SMLib's check gave no verdict on a body (SMLib can't take the record yet), so only the record's checks graded validity: {unchecked}.\n"
    );
    md += "\n## Cases, in ladder order\n\nTimes are the commands' own, in seconds, from the case's process; **golden** is the recorded time.\n\n| Case | What | Answer | Result | s | golden s | changes |\n|---|---|---|---|---|---|---|\n";
    md += &rows;
    std::fs::write(root().join("docs/kernel-ladder.md"), md).map_err(|e| format!("docs/kernel-ladder.md: {e}"))?;
    let n: usize = tiers.iter().map(|t| t.cases.len()).sum();
    Ok(format!("kernel ladder: {pass}/{n} pass → docs/kernel-ladder.md"))
}

/// The golden from this run's results (keeping what it holds for cases not run), and the
/// manifest with each tier sorted by time.
fn record(reason: &str, manifest: &Value, tiers: &[Tier], results: &BTreeMap<String, Value>, golden: &Value) -> Result<(), String> {
    let mut cases = golden["cases"].as_object().cloned().unwrap_or_default();
    for t in tiers {
        for c in &t.cases {
            if let Some(r) = results.get(c) {
                cases.insert(c.clone(), golden_entry(r));
            }
        }
    }
    let new_golden = json!({"reason": reason, "cases": cases});
    let mut m = manifest.clone();
    if let Some(list) = m["tiers"].as_array_mut() {
        for (t, tier) in list.iter_mut().zip(tiers) {
            let mut sorted: Vec<(f64, usize, &String)> = tier
                .cases
                .iter()
                .enumerate()
                .map(|(i, c)| (results.get(c).and_then(|r| r["seconds"].as_f64()).unwrap_or(f64::INFINITY), i, c))
                .collect();
            // Fastest first; equal times keep the manifest's order.
            sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            t["cases"] = json!(sorted.iter().map(|(_, _, c)| c.as_str()).collect::<Vec<_>>());
        }
    }
    let pretty = |v: &Value| serde_json::to_string_pretty(v).map(|s| s + "\n").map_err(|e| e.to_string());
    std::fs::write(golden_path(), pretty(&new_golden)?).map_err(|e| format!("golden.json: {e}"))?;
    std::fs::write(manifest_path(), pretty(&m)?).map_err(|e| format!("ladder.json: {e}"))?;
    println!("kernel ladder: golden recorded ({reason}); tiers re-sorted by time");
    Ok(())
}

pub fn run(rest: &[&str]) -> Result<(), String> {
    let reason = rest.iter().position(|a| *a == "--record").map(|i| rest.get(i + 1).copied().filter(|r| !r.starts_with("--")));
    let reason = match reason {
        None => None,
        Some(None) => return Err("--record needs a reason: --record \"why the golden changes\"".into()),
        Some(Some(r)) => Some(r.to_string()),
    };
    let gate_run = rest.contains(&"--gate");
    if gate_run && reason.is_some() {
        return Err("--gate and --record: a gate run compares with the golden, a record run rewrites it".into());
    }
    let mut o = options(rest, "target/kernel-ladder")?;
    // Times are compared with the golden: cases run one at a time unless --jobs says otherwise,
    // so they don't share the machine with each other.
    if !rest.contains(&"--jobs") {
        o.jobs = 1;
    }
    let manifest = read(&manifest_path());
    if manifest.is_null() {
        return Err(format!("{}: missing or not JSON", manifest_path().display()));
    }
    let tiers = tiers(&manifest)?;
    check_listed(&tiers)?;
    let base = root().join("tests/kernel-cases");
    let list: Vec<PathBuf> = tiers.iter().flat_map(|t| t.cases.iter().map(|c| base.join(c))).collect();
    let all = run_all(&o, &list);
    save_results(&o, &all)?;
    let results: BTreeMap<String, Value> = all.into_iter().map(|r| (rel_of(&r), r)).collect();
    let golden = read(&golden_path());
    if gate_run {
        // A gate run grades against the golden; the report stays the last full run's.
        println!("{}", gate(&tiers, &results, &golden)?);
    } else if let Some(reason) = &reason {
        record(reason, &manifest, &tiers, &results, &golden)?;
        let tiers = self::tiers(&read(&manifest_path()))?;
        println!("{}", write_report(&tiers, &results, &read(&golden_path()))?);
    } else {
        println!("{}", write_report(&tiers, &results, &golden)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_slowdowns_status_and_count_changes_only() {
        let golden = json!({"status": "pass", "seconds": 1.0, "measured": {"faces": 6, "edges": 12}});
        let same = json!({"status": "pass", "seconds": 1.2, "measured": {"faces": 6, "edges": 12}});
        assert!(flags(&same, Some(&golden)).is_empty());
        // Twice as slow but within timer noise of a fast case.
        let fast = json!({"status": "pass", "seconds": 0.03, "measured": {}});
        assert!(flags(&fast, Some(&json!({"status": "pass", "seconds": 0.01}))).is_empty());
        let worse = json!({"status": "fail", "seconds": 3.0, "measured": {"faces": 7, "edges": 12}});
        let f = flags(&worse, Some(&golden));
        assert_eq!(f.len(), 3, "{f:?}");
        assert_eq!(flags(&same, None), ["no golden"]);
    }

    #[test]
    fn the_gate_fails_on_any_change_but_time() {
        let t = vec![Tier { tier: "01-a".into(), what: String::new(), cases: vec!["ladder/01-a/001-x".into(), "ladder/01-a/002-y".into()] }];
        let golden = json!({"cases": {
            "ladder/01-a/001-x": {"status": "pass", "seconds": 1.0, "measured": {"faces": 6}},
            "ladder/01-a/002-y": {"status": "fail", "seconds": 1.0}
        }});
        let run =
            |x: Value, y: Value| gate(&t, &BTreeMap::from([("ladder/01-a/001-x".to_string(), x), ("ladder/01-a/002-y".to_string(), y)]), &golden);
        // Ten times slower is the machine, not a change.
        assert!(run(json!({"status": "pass", "seconds": 10.0, "measured": {"faces": 6}}), json!({"status": "fail"})).is_ok());
        let fixed = run(json!({"status": "pass", "measured": {"faces": 6}}), json!({"status": "pass"})).unwrap_err();
        assert!(fixed.contains("CHANGED (pass): ladder/01-a/002-y: status was fail"), "{fixed}");
        let counts = run(json!({"status": "pass", "measured": {"faces": 7}}), json!({"status": "fail"})).unwrap_err();
        assert!(counts.contains("faces 7 (golden 6)"), "{counts}");
        let unrecorded = gate(&t, &BTreeMap::new(), &golden).unwrap_err();
        assert!(unrecorded.contains("ladder/01-a/001-x: no result"), "{unrecorded}");
    }

    #[test]
    fn tiers_need_paths() {
        let m = json!({"tiers": [{"tier": "01-a", "what": "w", "cases": ["ladder/01-a/001-x"]}]});
        let t = tiers(&m).unwrap();
        assert_eq!((t[0].tier.as_str(), t[0].cases.len()), ("01-a", 1));
        assert!(tiers(&json!({"tiers": [{"tier": "01-a", "cases": [3]}]})).is_err());
    }
}
