//! `cargo xtask kernel-cases --ladder [--record REASON]`: the kernel complexity ladder.
//!
//! `tests/kernel-cases/ladder/ladder.json` lists tiers in order of complexity and, per tier, its
//! cases in order (paths under `tests/kernel-cases/`, so a tier may also list rig cases from other
//! areas). Each case is a test like any rig case, graded against its own `expected.json`. On top,
//! `tests/kernel-cases/ladder/golden.json` records per case and kernel the status, the commands'
//! time and what the kernel made (counts, volume, area): the run compares with it and flags
//! slowdowns and changes; cases run one at a time unless `--jobs` says otherwise, so the times are
//! not shared with other cases. `--record REASON` re-measures the golden and re-sorts each tier
//! by the first kernel's time, fastest first; both files are committed.
//!
//! The report, `docs/kernel-ladder.md`, gives per tier and kernel the counts per status and the
//! robustness frontier: the first case, in ladder order, the kernel fails.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::kernel_cases::{Options, cell, options, read, run_all};
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
    let Some(g) = g else { return vec!["no golden".into()] };
    let mut out = Vec::new();
    if g["status"] != r["status"] {
        out.push(format!("status was {}", g["status"].as_str().unwrap_or("?")));
    }
    if let (Some(now), Some(then)) = (r["seconds"].as_f64(), g["seconds"].as_f64())
        && now > then * SLOWDOWN_FACTOR
        && now - then > SLOWDOWN_MIN_S
    {
        out.push(format!("slower: {now:.2} s (golden {then:.2} s)"));
    }
    for k in ["body_count", "faces", "edges", "vertices"] {
        let (now, then) = (&r["measured"][k], &g["measured"][k]);
        if !now.is_null() && !then.is_null() && now != then {
            out.push(format!("{k} {now} (golden {then})"));
        }
    }
    out
}

fn secs(v: &Value) -> String {
    v.as_f64().map(|s| format!("{s:.3}")).unwrap_or_else(|| "–".into())
}

fn write_report(o: &Options, tiers: &[Tier], results: &BTreeMap<(String, String), Value>, golden: &Value) -> Result<String, String> {
    let mut md = String::from(
        "# Kernel complexity ladder\n\nGenerated by `cargo xtask kernel-cases --ladder`. The ladder in [`tests/kernel-cases/ladder/`](../tests/kernel-cases/ladder/) orders kernel operations by complexity: each tier builds on the shapes of the tiers before it, so the last case carries everything. Every case is a test with its own right answer (analysis or invariants, as in the [regression rig](kernel-cases.md)); the golden (`tests/kernel-cases/ladder/golden.json`) records per kernel its time and what it made, orders each tier by time and flags slowdowns.\n\nThe **frontier** is the first case of a tier, in ladder order, that the kernel fails (fail, crash or hang).\n\n## Tiers\n\n| Tier | What | Cases |",
    );
    for k in &o.kernels {
        md += &format!(" {k} | {k} frontier |");
    }
    md += "\n|---|---|---|";
    md += &"---|---|".repeat(o.kernels.len());
    md += "\n";
    let mut rows = String::new();
    let mut totals: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for t in tiers {
        md += &format!("| {} | {} | {} |", t.tier, t.what.replace('|', "/"), t.cases.len());
        for k in &o.kernels {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut frontier = None;
            for c in &t.cases {
                let status = results.get(&(c.clone(), k.clone())).and_then(|r| r["status"].as_str()).unwrap_or("not run").to_string();
                if frontier.is_none() && matches!(status.as_str(), "fail" | "crash" | "hang") {
                    frontier = Some(c.rsplit('/').next().unwrap_or(c).to_string());
                }
                *totals.entry(k.clone()).or_default().entry(status.clone()).or_default() += 1;
                *counts.entry(status).or_default() += 1;
            }
            let counts = counts.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ");
            md += &format!(" {counts} | {} |", frontier.unwrap_or_else(|| "–".into()));
        }
        md += "\n";
        for c in &t.cases {
            let want = read(&root().join("tests/kernel-cases").join(c).join("expected.json"));
            rows += &format!("| {} | {} | {} |", c, want["what"].as_str().unwrap_or("").replace('|', "/"), want["source"].as_str().unwrap_or("?"));
            for k in &o.kernels {
                let r = results.get(&(c.clone(), k.clone()));
                let g = golden["cases"][c].get(k);
                let f = r.map(|r| flags(r, g)).unwrap_or_default();
                rows += &format!(
                    " {} | {} | {} | {} |",
                    cell(r),
                    r.map(|r| secs(&r["seconds"])).unwrap_or_default(),
                    g.map(|g| secs(&g["seconds"])).unwrap_or_else(|| "–".into()),
                    f.join("; ").replace('|', "/")
                );
            }
            rows += "\n";
        }
    }
    // Cases whose bodies SMLib's check gave no verdict on (it can't take the record yet).
    let unchecked: Vec<String> = o
        .kernels
        .iter()
        .map(|k| {
            let n = results
                .iter()
                .filter(|((_, kk), r)| {
                    kk == k && r["checks"].as_array().into_iter().flatten().any(|c| c["got"].as_str().is_some_and(|g| g.starts_with("no verdict")))
                })
                .count();
            format!("{k}: {n}")
        })
        .collect();
    md += &format!(
        "\nCases where SMLib's check gave no verdict on a body (SMLib can't take the record yet), so only the record's checks graded validity: {}.\n",
        unchecked.join(", ")
    );
    md += "\n## Cases, in ladder order\n\nTimes are the commands' own, in seconds, from the case's process; **golden** is the recorded time.\n\n| Case | What | Answer |";
    for k in &o.kernels {
        md += &format!(" {k} | s | golden s | changes |");
    }
    md += "\n|---|---|---|";
    md += &"---|---|---|---|".repeat(o.kernels.len());
    md += "\n";
    md += &rows;
    std::fs::write(root().join("docs/kernel-ladder.md"), md).map_err(|e| format!("docs/kernel-ladder.md: {e}"))?;
    let n: usize = tiers.iter().map(|t| t.cases.len()).sum();
    let summary: Vec<String> = o
        .kernels
        .iter()
        .map(|k| {
            let pass = totals.get(k).and_then(|m| m.get("pass")).copied().unwrap_or(0);
            format!("{k}: {pass}/{n} pass")
        })
        .collect();
    Ok(format!("kernel ladder: {} → docs/kernel-ladder.md", summary.join(", ")))
}

/// The golden from this run's results (keeping what it holds for kernels not run), and the
/// manifest with each tier sorted by the first kernel's time.
fn record(
    o: &Options,
    reason: &str,
    manifest: &Value,
    tiers: &[Tier],
    results: &BTreeMap<(String, String), Value>,
    golden: &Value,
) -> Result<(), String> {
    let mut cases = golden["cases"].as_object().cloned().unwrap_or_default();
    for t in tiers {
        for c in &t.cases {
            let entry = cases.entry(c.clone()).or_insert_with(|| json!({}));
            for k in &o.kernels {
                if let Some(r) = results.get(&(c.clone(), k.clone())) {
                    entry[k.as_str()] = golden_entry(r);
                }
            }
        }
    }
    let new_golden = json!({"reason": reason, "kernels": o.kernels, "cases": cases});
    let mut m = manifest.clone();
    let lead = o.kernels.first().cloned().unwrap_or_else(|| "default".into());
    if let Some(list) = m["tiers"].as_array_mut() {
        for (t, tier) in list.iter_mut().zip(tiers) {
            let mut sorted: Vec<(f64, usize, &String)> = tier
                .cases
                .iter()
                .enumerate()
                .map(|(i, c)| (results.get(&(c.clone(), lead.clone())).and_then(|r| r["seconds"].as_f64()).unwrap_or(f64::INFINITY), i, c))
                .collect();
            // Fastest first; equal times keep the manifest's order.
            sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            t["cases"] = json!(sorted.iter().map(|(_, _, c)| c.as_str()).collect::<Vec<_>>());
        }
    }
    let pretty = |v: &Value| serde_json::to_string_pretty(v).map(|s| s + "\n").map_err(|e| e.to_string());
    std::fs::write(golden_path(), pretty(&new_golden)?).map_err(|e| format!("golden.json: {e}"))?;
    std::fs::write(manifest_path(), pretty(&m)?).map_err(|e| format!("ladder.json: {e}"))?;
    println!("kernel ladder: golden recorded ({reason}); tiers re-sorted by {lead} time");
    Ok(())
}

pub fn run(rest: &[&str]) -> Result<(), String> {
    let reason = rest.iter().position(|a| *a == "--record").map(|i| rest.get(i + 1).copied().filter(|r| !r.starts_with("--")));
    let reason = match reason {
        None => None,
        Some(None) => return Err("--record needs a reason: --record \"why the golden changes\"".into()),
        Some(Some(r)) => Some(r.to_string()),
    };
    let mut o = options(rest)?;
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
    let results: BTreeMap<(String, String), Value> =
        run_all(&o, &list).into_iter().map(|r| ((rel_of(&r), r["kernel"].as_str().unwrap_or("").to_string()), r)).collect();
    let golden = read(&golden_path());
    if let Some(reason) = &reason {
        record(&o, reason, &manifest, &tiers, &results, &golden)?;
        let tiers = self::tiers(&read(&manifest_path()))?;
        println!("{}", write_report(&o, &tiers, &results, &read(&golden_path()))?);
    } else {
        println!("{}", write_report(&o, &tiers, &results, &golden)?);
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
    fn tiers_need_paths() {
        let m = json!({"tiers": [{"tier": "01-a", "what": "w", "cases": ["ladder/01-a/001-x"]}]});
        let t = tiers(&m).unwrap();
        assert_eq!((t[0].tier.as_str(), t[0].cases.len()), ("01-a", 1));
        assert!(tiers(&json!({"tiers": [{"tier": "01-a", "cases": [3]}]})).is_err());
    }
}
