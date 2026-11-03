//! `kernel-cases --export=DIR`: one kernel rig case as a corpus entry for solidcore's rig. The
//! case's commands run as the rig runs them while the kernel's operations are traced
//! (`smlib_trace`); the entry holds the operation (or the chain of operations) that made the
//! bodies the case ends with, each operand that came in as a record given as that record (the
//! B-rep record, solidcore-record's JSON), the fix set, the case's expected values and the rig's
//! own result. A case whose result no kernel operation alone accounts for is written too, marked
//! not expressible with the reasons, so nothing is dropped silently.
//!
//! An entry is expressible when:
//! - the run failed in a kernel operation (the engine's error carries the operation's), or every
//!   body the case ends with is the record a traced operation's output was written as, or an
//!   operand some traced operation took, as its record;
//! - its expected values are all the kernel's measures: none of `continuity`, `watertight`,
//!   `files`, `results`, `construction` or `flat_pattern`, which SolidCraft computes;
//! - the counts it grades (faces, edges, vertices) are the records' own: the rig counts faces and
//!   edges merged across one analytic surface or curve (`merged_topology`, from a mesh), so an
//!   entry where those differ is not expressible.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_engine::geom::brep::Brep;
use solidcraft_engine::kernel::smlib_trace::{self, Event};

use crate::read_json;

/// What a traced case leaves for its entry: the trace of its commands and the bodies it ends with.
#[derive(Default)]
pub struct Capture {
    pub events: Vec<Event>,
    pub world: Vec<(String, Brep)>,
}

/// The expected values SolidCraft computes, never the kernel.
const GRADER_ONLY: [&str; 6] = ["continuity", "watertight", "files", "results", "construction", "flat_pattern"];

/// The case directory's area (its path under `tests/kernel-cases/`, a ladder case's tier under
/// `ladder/`) and its id.
fn area_and_id(dir: &str) -> Result<(String, String), String> {
    let p = Path::new(dir);
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let at = parts.iter().rposition(|c| c == "kernel-cases").ok_or_else(|| format!("{dir}: not under tests/kernel-cases"))?;
    let rest = parts.get(at + 1..).unwrap_or_default();
    match rest.split_last() {
        Some((id, area)) if !area.is_empty() => Ok((area.join("/"), id.clone())),
        _ => Err(format!("{dir}: no area")),
    }
}

/// The case's line in its area's `KNOWN_FAILING`: why it fails today; `None` when not listed.
fn known_failing(dir: &str, id: &str) -> Option<String> {
    let list = Path::new(dir).parent()?.join("KNOWN_FAILING");
    let text = std::fs::read_to_string(list).ok()?;
    text.lines().filter(|l| !l.trim_start().starts_with('#')).find_map(|l| {
        let (case, why) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
        (case == id).then(|| why.trim().to_string())
    })
}

/// The trace read as operations and the records that went in and came out.
struct Trace<'a> {
    /// Each operation: its name, operands, parameters and outputs or error.
    calls: Vec<(&'static str, &'a [u64], &'a Value, &'a Result<Vec<u64>, String>)>,
    /// The record each body made from a record was made from.
    imports: HashMap<u64, (&'a Brep, bool, bool)>,
    /// The records bodies were written as, in order.
    exports: Vec<(u64, &'a Brep)>,
    /// The operation that made each body.
    producer: HashMap<u64, usize>,
}

impl<'a> Trace<'a> {
    fn of(events: &'a [Event]) -> Trace<'a> {
        let mut t = Trace { calls: Vec::new(), imports: HashMap::new(), exports: Vec::new(), producer: HashMap::new() };
        for e in events {
            match e {
                Event::Import { id, record, trusted, keeping_splines } => {
                    t.imports.insert(*id, (record, *trusted, *keeping_splines));
                }
                Event::Export { id, record } => t.exports.push((*id, record)),
                Event::Call { op, inputs, params, outputs } => {
                    if let Ok(made) = outputs {
                        for id in made {
                            t.producer.insert(*id, t.calls.len());
                        }
                    }
                    t.calls.push((*op, inputs, params, outputs));
                }
            }
        }
        t
    }

    /// Bodies some operation took as an operand.
    fn operands(&self) -> BTreeSet<u64> {
        self.calls.iter().flat_map(|c| c.1.iter().copied()).collect()
    }
}

/// The operations `ends` need, in the order they ran: each body's maker, and its operands' makers
/// in turn, back to bodies made from records. Operands made by nothing traced are listed in
/// `unknown`.
fn chain(t: &Trace<'_>, ends: &[u64], unknown: &mut BTreeSet<u64>) -> Vec<usize> {
    let mut need = BTreeSet::new();
    let mut todo: Vec<u64> = ends.to_vec();
    while let Some(id) = todo.pop() {
        if t.imports.contains_key(&id) {
            continue;
        }
        match t.producer.get(&id) {
            Some(&i) => {
                if need.insert(i)
                    && let Some(c) = t.calls.get(i)
                {
                    todo.extend(c.1.iter().copied());
                }
            }
            None => {
                unknown.insert(id);
            }
        }
    }
    need.into_iter().collect()
}

/// The entry of case `dir`, run with result `out`, into `root/<area>/<id>.json`, and the fix set
/// into `root/fixes.json`.
pub fn write(root: &Path, dir: &str, out: &Value, capture: Capture) -> Result<(), String> {
    let (area, id) = area_and_id(dir)?;
    let want = read_json(&format!("{dir}/expected.json"))?;
    let t = Trace::of(&capture.events);
    let mut why_not: Vec<String> = Vec::new();
    let status = out["status"].as_str().unwrap_or("");

    if let Some(missing) = want["missing"].as_str() {
        why_not.push(format!("SolidCraft has no command for it yet: {missing}"));
    }
    let grader: Vec<&str> = GRADER_ONLY.iter().copied().filter(|k| want.get(*k).is_some_and(|v| !v.is_null())).collect();
    if !grader.is_empty() {
        why_not.push(format!("graded on what SolidCraft computes: {}", grader.join(", ")));
    }

    // The bodies the entry ends with: what each is, by name.
    let mut result: Vec<Value> = Vec::new();
    let mut ends: Vec<u64> = Vec::new();
    let mut failed_call: Option<usize> = None;
    let error = out["refused_with"].as_str().or_else(|| out["error"].as_str()).filter(|_| out.get("checks").is_none());
    if let Some(e) = error.filter(|_| want.get("missing").is_none()) {
        // The run stopped: the kernel operation that failed with the message the engine gave.
        failed_call = t.calls.iter().rposition(|c| matches!(c.3, Err(m) if !m.is_empty() && e.contains(m.as_str())));
        match failed_call {
            Some(i) => ends.extend(t.calls.get(i).map(|c| c.1.to_vec()).unwrap_or_default()),
            None if t.calls.is_empty() => why_not.push(format!("no kernel operation ran before the failure: {e}")),
            None => why_not.push(format!("the failure is SolidCraft's, not a kernel operation's: {e}")),
        }
    } else if want.get("missing").is_none() {
        let operands = t.operands();
        let mut made = 0;
        for (name, record) in &capture.world {
            let from_op = t.exports.iter().rev().find(|(_, r)| *r == record).map(|(id, _)| *id).filter(|id| t.producer.contains_key(id));
            let operand = t.imports.iter().find(|(i, (r, _, _))| *r == record && operands.contains(i)).map(|(i, _)| *i);
            match (from_op, operand) {
                (Some(id), _) => {
                    made += 1;
                    ends.push(id);
                    result.push(json!({"body": name, "made": format!("b{id}")}));
                }
                (None, Some(id)) => {
                    ends.push(id);
                    result.push(json!({"body": name, "operand": format!("b{id}")}));
                }
                (None, None) => why_not.push(format!(
                    "body {name:?} is no kernel operation's output nor an operand: SolidCraft built it or changed the kernel's record"
                )),
            }
        }
        if capture.world.is_empty() {
            why_not.push("the case ends with no body".into());
        } else if made == 0 && why_not.is_empty() {
            why_not.push("no kernel operation made a body the case ends with".into());
        }
        // Counts graded as SolidCraft merges them, where that differs from the records'.
        let raw = |k: &str| -> u64 {
            capture
                .world
                .iter()
                .map(|(_, r)| match k {
                    "faces" => r.faces.len(),
                    "edges" => r.edges.len(),
                    _ => r.vertices.len(),
                } as u64)
                .sum()
        };
        for k in ["faces", "edges", "vertices"] {
            let graded = want.get(k).is_some_and(|v| !v.is_null());
            let merged = out["measured"][k].as_u64();
            if graded && merged.is_some_and(|m| m != raw(k)) {
                why_not.push(format!("{k} graded merged across analytic surfaces ({}), the records have {}", merged.unwrap_or(0), raw(k)));
            }
        }
    }

    let mut unknown = BTreeSet::new();
    let order = chain(&t, &ends, &mut unknown);
    if !unknown.is_empty() {
        why_not.push(format!("operands made by nothing traced: {unknown:?}"));
    }
    let order: Vec<usize> = match failed_call {
        Some(i) if !order.contains(&i) => order.into_iter().chain([i]).collect(),
        _ => order,
    };
    // Operand records: every body the chain takes that came in as a record.
    let mut bodies = BTreeMap::new();
    let calls: Vec<Value> = order
        .iter()
        .filter_map(|&i| t.calls.get(i))
        .map(|(op, inputs, params, outputs)| {
            for id in inputs.iter() {
                if let Some((record, trusted, splines)) = t.imports.get(id) {
                    bodies.insert(format!("b{id}"), json!({"trusted": trusted, "keeping_splines": splines, "record": record}));
                }
            }
            let mut c = json!({"op": op, "inputs": inputs.iter().map(|i| format!("b{i}")).collect::<Vec<_>>(), "params": params});
            match outputs {
                Ok(made) => c["outputs"] = json!(made.iter().map(|i| format!("b{i}")).collect::<Vec<_>>()),
                Err(e) => c["error"] = json!(e),
            }
            c
        })
        .collect();
    for (_, record) in &capture.world {
        // An operand the case ends with, never operated on in the chain, still comes as its record.
        if let Some(id) = t.imports.iter().find(|(_, (r, _, _))| *r == record).map(|(i, _)| *i)
            && result.iter().any(|r| r["operand"] == json!(format!("b{id}")))
            && let Some((record, trusted, splines)) = t.imports.get(&id)
        {
            bodies.insert(format!("b{id}"), json!({"trusted": trusted, "keeping_splines": splines, "record": record}));
        }
    }
    let rig = json!({
        "status": out["status"], "error": out["error"], "refused_with": out["refused_with"], "measured": out["measured"],
        "known_failing": known_failing(dir, &id),
    });
    let entry = json!({
        "case": id,
        "area": area,
        "sources": want["sources"],
        "what": want["what"],
        "expressible": why_not.is_empty(),
        "why_not": why_not,
        "expected": want,
        "rig": rig,
        "calls": calls,
        "bodies": bodies,
        "result": result,
        "traced_ops": t.calls.iter().map(|c| c.0).collect::<Vec<_>>(),
        "status": status,
    });
    let folder: PathBuf = root.join(&area);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(format!("{id}.json"));
    let text = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    let fixes = serde_json::to_string_pretty(&smlib_trace::fixes()).map_err(|e| e.to_string())?;
    std::fs::write(root.join("fixes.json"), fixes).map_err(|e| format!("{}: {e}", root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_area_is_the_path_under_kernel_cases() {
        assert_eq!(area_and_id("tests/kernel-cases/booleans/1894-x").unwrap(), ("booleans".into(), "1894-x".into()));
        assert_eq!(area_and_id("/r/tests/kernel-cases/ladder/07-fillets/003-y").unwrap(), ("ladder/07-fillets".into(), "003-y".into()));
        assert!(area_and_id("/r/other/x").is_err());
    }
}
