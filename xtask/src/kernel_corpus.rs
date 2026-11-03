//! `cargo xtask kernel-cases [--ladder] --export DIR`: the kernel rig as a corpus for solidcore's
//! own rig. Each case's process writes its entry (`solidcraft-cli kernel-cases --export`,
//! `DIR/<area>/<case>.json`: the kernel operations that made its result, their operand records,
//! the fix set, its expected values and the rig's result). This writes what that leaves out:
//! an entry for each case whose process left none (a hang or a crash, so no trace), each ladder
//! case's golden, and `DIR/manifest.json`, the counts per area and every case that is not
//! expressible as kernel input, with its reasons. An entry's operand records go to
//! `<case>.bodies.json.gz` beside it (gzip, no timestamp, so a re-export of the same records is
//! the same bytes); the entry keeps what each body is and names that file.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::kernel_cases::read;
use crate::root;

/// The kind of reason a case is not expressible, for the counts per area.
fn reason_kind(why: &str) -> &'static str {
    if why.starts_with("SolidCraft has no command") {
        "SolidCraft has no command for it yet"
    } else if why.starts_with("graded on what SolidCraft computes") {
        "graded on SolidCraft's own measures (continuity, mesh, files, command answers, construction, flat pattern)"
    } else if why.contains("graded merged across analytic surfaces") {
        "counts graded merged across analytic surfaces (SolidCraft's merged_topology)"
    } else if why.starts_with("no kernel operation ran before the failure") || why.starts_with("the failure is SolidCraft's") {
        "refused or failed in SolidCraft, not in a kernel operation"
    } else if why.contains("is no kernel operation's output nor an operand") {
        "a final body built or changed by SolidCraft (its own geometry or a change to the kernel's record)"
    } else if why.starts_with("no kernel operation made a body") || why.starts_with("the case ends with no body") {
        "no kernel operation makes the result"
    } else if why.starts_with("the rig's run") {
        "the rig's run hung or crashed (no trace)"
    } else {
        "other"
    }
}

fn write_json(path: &Path, v: &Value, pretty: bool) -> Result<(), String> {
    let text = if pretty { serde_json::to_string_pretty(v) } else { serde_json::to_string(v) }.map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Moves the records out of `entry`'s `bodies` into the gzip file `to`, `{"b<n>": record}`, and
/// names it in `records`. An entry without records (written before) is left as it is.
fn split_records(entry: &mut Value, to: &Path) -> Result<(), String> {
    use std::io::Write as _;
    let Some(bodies) = entry["bodies"].as_object_mut() else { return Ok(()) };
    let mut records = serde_json::Map::new();
    for (name, b) in bodies.iter_mut() {
        if let Some(r) = b.as_object_mut().and_then(|o| o.remove("record")) {
            records.insert(name.clone(), r);
        }
    }
    if records.is_empty() {
        return Ok(());
    }
    let text = serde_json::to_vec(&Value::Object(records)).map_err(|e| e.to_string())?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&text).map_err(|e| e.to_string())?;
    let bytes = gz.finish().map_err(|e| e.to_string())?;
    std::fs::write(to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
    entry["records"] = json!(to.file_name().map(|n| n.to_string_lossy().to_string()));
    Ok(())
}

/// What `git args` prints in this checkout.
fn git(args: &[&str]) -> String {
    std::process::Command::new("git")
        .current_dir(root())
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The solidcore rev SolidCraft is pinned to (the workspace `Cargo.toml`).
fn solidcore_pin() -> String {
    let text = std::fs::read_to_string(root().join("Cargo.toml")).unwrap_or_default();
    text.lines()
        .find(|l| l.starts_with("solidcore ="))
        .and_then(|l| l.split("rev = \"").nth(1))
        .and_then(|r| r.split('"').next())
        .unwrap_or_default()
        .to_string()
}

/// Completes the corpus in `dir` after a run with `results` (each with its `path` under
/// `tests/kernel-cases/`); `golden`, the ladder's, for a ladder run. Returns the summary.
pub(crate) fn manifest(dir: &Path, results: &[Value], golden: Option<&Value>) -> Result<String, String> {
    let base = root().join("tests/kernel-cases");
    for r in results {
        let rel = r["path"].as_str().unwrap_or("");
        let Some((area, id)) = rel.rsplit_once('/') else { continue };
        let file = dir.join(area).join(format!("{id}.json"));
        let mut entry = read(&file);
        if entry.is_null() {
            // The case's process left no entry: it hung or crashed before writing one.
            let want = read(&base.join(rel).join("expected.json"));
            entry = json!({
                "case": id, "area": area, "sources": want["sources"], "what": want["what"],
                "expressible": false,
                "operation_expressible": false,
                "ungraded": [],
                "why_not": [format!("the rig's run {}: {}", r["status"].as_str().unwrap_or("?"), r["error"].as_str().unwrap_or(""))],
                "expected": want,
                "rig": {"status": r["status"], "error": r["error"]},
                "calls": [], "bodies": {}, "result": [], "traced_ops": [], "status": r["status"],
            });
        }
        if let Some(g) = golden.and_then(|g| g["cases"].get(rel)) {
            // The golden's status and counts; never its time.
            entry["golden"] = json!({"status": g["status"], "measured": g["measured"], "unstable": g.get("unstable")});
        }
        if let Some(e) = r.get("export_error") {
            entry["export_error"] = e.clone();
        }
        split_records(&mut entry, &dir.join(area).join(format!("{id}.bodies.json.gz")))?;
        write_json(&file, &entry, true)?;
    }

    // Every entry in the corpus, this run's or an earlier one's.
    let mut areas: BTreeMap<String, Value> = BTreeMap::new();
    let mut not: Vec<Value> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "json") && p.parent() != Some(dir) {
                files.push(p);
            }
        }
    }
    files.sort();
    for f in files {
        let e = read(&f);
        let area = e["area"].as_str().unwrap_or("?").to_string();
        let a = areas.entry(area.clone()).or_insert_with(|| {
            json!({"cases": 0, "expressible": 0, "operation_expressible": 0, "not_expressible": 0, "chain": {}, "reasons": {}, "ungraded": {}})
        });
        let bump = |a: &mut Value, k: &str| a[k] = json!(a[k].as_u64().unwrap_or(0) + 1);
        bump(a, "cases");
        if e["expressible"].as_bool() == Some(true) {
            bump(a, "expressible");
        }
        for u in e["ungraded"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let k = u.split(':').next().unwrap_or(u);
            bump(&mut a["ungraded"], k);
        }
        if e["operation_expressible"].as_bool() == Some(true) {
            bump(a, "operation_expressible");
            let n = e["calls"].as_array().map_or(0, Vec::len).to_string();
            bump(&mut a["chain"], &n);
        } else {
            bump(a, "not_expressible");
            let why: Vec<&str> = e["why_not"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let mut kinds: Vec<&str> = why.iter().map(|w| reason_kind(w)).collect();
            kinds.sort();
            kinds.dedup();
            for k in kinds {
                bump(&mut a["reasons"], k);
            }
            not.push(json!({"case": format!("{area}/{}", e["case"].as_str().unwrap_or("?")), "why": why}));
        }
    }
    let m = json!({
        "solidcraft": git(&["rev-parse", "HEAD"]),
        "solidcore": solidcore_pin(),
        "fixes": "fixes.json",
        "areas": areas,
        "not_expressible": not,
    });
    write_json(&dir.join("manifest.json"), &m, true)?;
    let mut text = String::from("area                        cases  operation  wholly  not\n");
    for (area, a) in &areas {
        let n = |k: &str| a[k].as_u64().unwrap_or(0);
        text += &format!("{area:<27} {:>5}  {:>9}  {:>6}  {:>3}\n", n("cases"), n("operation_expressible"), n("expressible"), n("not_expressible"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_the_export_gives_has_its_kind() {
        for (why, kind) in [
            ("graded on what SolidCraft computes: continuity", "graded on SolidCraft's own measures"),
            ("faces graded merged across analytic surfaces (6), the records have 8", "counts graded merged"),
            ("the failure is SolidCraft's, not a kernel operation's: x", "refused or failed in SolidCraft"),
            ("body \"Body1\" is no kernel operation's output nor an operand: SolidCraft built it", "a final body built or changed"),
            ("the rig's run hang: no result", "the rig's run hung"),
        ] {
            assert!(reason_kind(why).starts_with(kind), "{why}: {}", reason_kind(why));
        }
    }
}
