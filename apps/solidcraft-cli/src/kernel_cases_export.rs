//! `kernel-cases --export=DIR`: one kernel rig case as a corpus entry for solidcore's rig. The
//! case's commands run as the rig runs them while the kernel's operations are traced
//! (`smlib_trace`); the entry holds every operation in execution order, including discarded
//! tool construction and handled refusals, each operand that came in as a record (the
//! B-rep record, solidcore-record's JSON), the fix set, the case's expected values and the rig's
//! own result. A case whose result no kernel operation alone accounts for is written too, with
//! the reasons, so nothing is dropped silently.
//!
//! An entry's operation is expressible (`operation_expressible`) when the run failed in a kernel
//! operation (the last one traced failed), or when every body the case ends with is the record a
//! traced operation's output was written as, or an operand some traced operation took, as its
//! record. The entry is wholly expressible (`expressible`) when, besides, every value it grades is
//! the kernel's: none of `continuity`, `watertight`, `files`, `results`, `construction` or
//! `flat_pattern` (SolidCraft computes them), no face, edge or vertex count that the rig's merging
//! across analytic surfaces (`merged_topology`) changes, and no refusal graded on SolidCraft's
//! wording of the kernel's error. Those are listed in `ungraded`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcore_record::brep::Brep;
use solidcraft_engine::kernel::smlib_trace::{self, Event};

use crate::read_json;

/// What a traced case leaves for its entry: the trace of its commands, and the bodies it ends
/// with by name or the engine's error.
#[derive(Default)]
pub struct Capture {
    pub events: Vec<Event>,
    pub ran: Option<Result<Vec<(String, Brep)>, String>>,
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

/// The entry of case `dir`, run with result `out`, into `root/<area>/<id>.json`, and the fix set
/// into `root/fixes.json`.
pub fn write(root: &Path, dir: &str, out: &Value, capture: Capture) -> Result<(), String> {
    let (area, id) = area_and_id(dir)?;
    let want = read_json(&format!("{dir}/expected.json"))?;
    let t = Trace::of(&capture.events);
    // Why the operation is not the kernel's alone, and what the entry cannot grade without
    // SolidCraft.
    let mut why_not: Vec<String> = Vec::new();
    let mut ungraded: Vec<String> = Vec::new();
    let has = |k: &str| want.get(k).is_some_and(|v| !v.is_null());

    if let Some(missing) = want["missing"].as_str() {
        why_not.push(format!("SolidCraft has no command for it yet: {missing}"));
    }
    for k in GRADER_ONLY.iter().filter(|k| has(k)) {
        ungraded.push(format!("{k}: SolidCraft computes it"));
    }

    let mut result: Vec<Value> = Vec::new();
    let mut ends: Vec<u64> = Vec::new();
    let mut failed_call: Option<usize> = None;
    let mut outcome = json!({"ran": true});
    match (&capture.ran, has("missing")) {
        (_, true) => {}
        (None, false) => why_not.push("the case did not run".into()),
        (Some(Err(e)), false) => {
            // The run stopped. It stopped in the kernel when the last operation that failed is
            // followed by no operation but copies (a copy or a tagged copy, which build nothing):
            // nothing was built after it, so the engine's error reports that failure in its own
            // words. An operation that ran after it means the engine went on and failed itself.
            outcome = json!({"ran": false, "engine_error": e});
            let last_failed = t.calls.iter().rposition(|c| c.3.is_err());
            let built_after = |i: usize| t.calls.iter().skip(i + 1).any(|c| !matches!(c.0, "copy" | "tagged"));
            match last_failed.filter(|&i| !built_after(i)).and_then(|i| t.calls.get(i).map(|c| (i, c))) {
                Some((i, c)) => {
                    failed_call = Some(i);
                    ends.extend(c.1.iter().copied());
                    if let Err(k) = c.3 {
                        outcome["kernel_error"] = json!(k);
                        if let Some(why) = want["refusal"].as_str().filter(|w| !k.contains(*w)) {
                            ungraded.push(format!("refusal {why:?}: SolidCraft's wording of the kernel's error"));
                        }
                    }
                }
                None if t.calls.is_empty() => why_not.push(format!("no kernel operation ran before the failure: {e}")),
                None => why_not.push(format!("the failure is SolidCraft's, not a kernel operation's: {e}")),
            }
        }
        (Some(Ok(world)), false) => {
            let operands = t.operands();
            let mut made = 0;
            for (name, record) in world {
                let from_op = t.exports.iter().rev().find(|(id, r)| *r == record && t.producer.contains_key(id)).map(|(id, _)| *id);
                let operand = t.imports.iter().filter(|(i, (r, _, _))| *r == record && operands.contains(i)).map(|(i, _)| *i).min();
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
            if world.is_empty() {
                why_not.push("the case ends with no body".into());
            } else if made == 0 && why_not.is_empty() {
                why_not.push("no kernel operation made a body the case ends with".into());
            }
            // Counts graded as SolidCraft merges them, where that differs from the records'.
            let raw = |k: &str| -> u64 {
                world
                    .iter()
                    .map(|(_, r)| match k {
                        "faces" => r.faces.len(),
                        "edges" => r.edges.len(),
                        _ => r.vertices.len(),
                    } as u64)
                    .sum()
            };
            for k in ["faces", "edges", "vertices"] {
                if let Some(merged) = out["measured"][k].as_u64().filter(|&m| has(k) && m != raw(k)) {
                    ungraded.push(format!("{k}: graded merged across analytic surfaces ({merged}), the records have {}", raw(k)));
                }
            }
            outcome["raw_counts"] = json!({"faces": raw("faces"), "edges": raw("edges"), "vertices": raw("vertices")});
        }
    }

    let mut unknown = BTreeSet::new();
    let mut available: BTreeSet<u64> = t.imports.keys().copied().collect();
    for (_, inputs, _, outputs) in &t.calls {
        unknown.extend(inputs.iter().filter(|id| !available.contains(id)).copied());
        if let Ok(made) = outputs {
            available.extend(made.iter().copied());
        }
    }
    unknown.extend(ends.iter().filter(|id| !available.contains(id)).copied());
    if !unknown.is_empty() {
        why_not.push(format!("operands made by nothing traced: {unknown:?}"));
    }
    // Operand records: every body a call takes, or the case ends with, that came in as a record.
    let mut bodies = BTreeMap::new();
    let given = |id: u64, bodies: &mut BTreeMap<String, Value>| {
        if let Some((record, trusted, splines)) = t.imports.get(&id) {
            bodies.insert(format!("b{id}"), json!({"trusted": trusted, "keeping_splines": splines, "record": record}));
        }
    };
    let mut calls: Vec<Value> = Vec::new();
    for (i, (op, inputs, params, outputs)) in t.calls.iter().enumerate() {
        for &id in inputs.iter() {
            given(id, &mut bodies);
        }
        let mut c = json!({"op": op, "inputs": inputs.iter().map(|i| format!("b{i}")).collect::<Vec<_>>(), "params": params});
        match outputs {
            Ok(made) => c["outputs"] = json!(made.iter().map(|i| format!("b{i}")).collect::<Vec<_>>()),
            Err(e) => {
                c["error"] = json!(e);
                // The application handled this refusal. Replay must continue to any later
                // calls and grade the final bodies, even when this was the last attempt.
                if failed_call != Some(i) {
                    c["continued"] = json!(true);
                }
            }
        }
        calls.push(c);
    }
    for r in &result {
        if let Some(id) = r["operand"].as_str().and_then(|b| b.strip_prefix('b')).and_then(|n| n.parse().ok()) {
            given(id, &mut bodies);
        }
    }
    let rig = json!({
        "status": out["status"], "error": out["error"], "refused_with": out["refused_with"], "measured": out["measured"],
        "known_failing": known_failing(dir, &id),
    });
    let operation_expressible = why_not.is_empty();
    let entry = json!({
        "case": id,
        "area": area,
        "sources": want["sources"],
        "what": want["what"],
        "expressible": operation_expressible && ungraded.is_empty(),
        "operation_expressible": operation_expressible,
        "why_not": why_not,
        "ungraded": ungraded,
        "expected": want,
        "rig": rig,
        "outcome": outcome,
        "calls": calls,
        "bodies": bodies,
        "result": result,
        "traced_ops": t.calls.iter().map(|c| c.0).collect::<Vec<_>>(),
    });
    let folder: PathBuf = root.join(&area);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(format!("{id}.json"));
    let text = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    // Cases run in parallel processes: each writes its own file and renames it over the shared
    // one, so no reader sees a torn file (every process writes the same fix set).
    let fixes = serde_json::to_string_pretty(&smlib_trace::fixes()).map_err(|e| e.to_string())?;
    let own = root.join(format!("fixes.json.{}", std::process::id()));
    std::fs::write(&own, fixes).map_err(|e| format!("{}: {e}", own.display()))?;
    std::fs::rename(&own, root.join("fixes.json")).map_err(|e| format!("{}: {e}", root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_record() -> Brep {
        Brep { version: 4, tolerance: 1e-6, regions: Vec::new(), shells: Vec::new(), faces: Vec::new(), edges: Vec::new(), vertices: Vec::new() }
    }

    fn call(op: &'static str, inputs: &[u64], outputs: Result<Vec<u64>, String>) -> Event {
        Event::Call { op, inputs: inputs.to_vec(), params: json!({}), outputs }
    }

    fn exported_case(name: &str, events: Vec<Event>, ran: Result<Vec<(String, Brep)>, String>) -> Value {
        let root = std::env::temp_dir().join(format!("solidcraft-export-{}-{name}", std::process::id()));
        let dir = root.join("tests/kernel-cases/traces").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("expected.json"), "{}").unwrap();
        let output = root.join("export");
        write(&output, dir.to_str().unwrap(), &json!({"status": "pass"}), Capture { events, ran: Some(ran) }).unwrap();
        let entry = output.join("traces").join(format!("{name}.json"));
        serde_json::from_str(&std::fs::read_to_string(entry).unwrap()).unwrap()
    }

    #[test]
    fn discarded_tool_construction_is_exported_in_execution_order() {
        let record = empty_record();
        let e = exported_case(
            "discarded-tool",
            vec![call("extrude", &[], Ok(vec![1])), call("extrude", &[], Ok(vec![2])), Event::Export { id: 1, record: Box::new(record.clone()) }],
            Ok(vec![("Part".into(), record)]),
        );
        assert_eq!(e["calls"].as_array().unwrap().len(), 2);
        assert_eq!(e["calls"][0]["outputs"], json!(["b1"]));
        assert_eq!(e["calls"][1]["outputs"], json!(["b2"]));
        assert_eq!(e["traced_ops"], json!(["extrude", "extrude"]));
    }

    #[test]
    fn handled_refusals_are_exported_without_becoming_terminal() {
        let record = empty_record();
        let e = exported_case(
            "handled-refusal",
            vec![
                call("extrude", &[], Ok(vec![1])),
                call("fillet", &[1], Err("too large".into())),
                call("fillet", &[1], Ok(vec![2])),
                Event::Export { id: 2, record: Box::new(record.clone()) },
            ],
            Ok(vec![("Part".into(), record)]),
        );
        assert_eq!(e["calls"].as_array().unwrap().len(), 3);
        assert_eq!(e["calls"][1]["error"], "too large");
        assert_eq!(e["calls"][1]["continued"], true);
    }

    #[test]
    fn a_handled_last_attempt_keeps_the_retained_body_as_the_result() {
        let record = empty_record();
        let e = exported_case(
            "handled-last-attempt",
            vec![
                call("extrude", &[], Ok(vec![1])),
                call("fillet", &[1], Err("too large".into())),
                Event::Export { id: 1, record: Box::new(record.clone()) },
            ],
            Ok(vec![("Part".into(), record)]),
        );
        assert_eq!(e["calls"][1]["continued"], true);
        assert_eq!(e["result"], json!([{"body": "Part", "made": "b1"}]));
        assert_eq!(e["operation_expressible"], true);
    }

    #[test]
    fn the_final_refusal_still_ends_replay() {
        let e = exported_case(
            "terminal-refusal",
            vec![
                call("extrude", &[], Ok(vec![1])),
                call("fillet", &[1], Err("too large".into())),
                call("fillet", &[1], Err("no room".into())),
                call("copy", &[1], Ok(vec![2])),
            ],
            Err("no room".into()),
        );
        assert_eq!(e["calls"].as_array().unwrap().len(), 4);
        assert_eq!(e["calls"][1]["continued"], true);
        assert!(e["calls"][2].get("continued").is_none());
        assert_eq!(e["outcome"]["kernel_error"], "no room");
    }

    #[test]
    fn discarded_operations_still_require_their_operand_records() {
        let record = empty_record();
        let e = exported_case(
            "missing-tool-operand",
            vec![call("extrude", &[], Ok(vec![1])), call("copy", &[99], Ok(vec![2])), Event::Export { id: 1, record: Box::new(record.clone()) }],
            Ok(vec![("Part".into(), record)]),
        );
        assert_eq!(e["operation_expressible"], false);
        assert!(e["why_not"].as_array().unwrap().iter().any(|why| why.as_str().unwrap().contains("99")));
    }

    #[test]
    fn discarded_operations_keep_their_imported_operands() {
        let record = empty_record();
        let e = exported_case(
            "imported-tool-operand",
            vec![
                call("extrude", &[], Ok(vec![1])),
                Event::Import { id: 9, record: Box::new(record.clone()), trusted: false, keeping_splines: true },
                call("copy", &[9], Ok(vec![2])),
                Event::Export { id: 1, record: Box::new(record.clone()) },
            ],
            Ok(vec![("Part".into(), record.clone())]),
        );
        assert_eq!(e["bodies"]["b9"], json!({"record": record, "trusted": false, "keeping_splines": true}));
        assert_eq!(e["calls"][1]["inputs"], json!(["b9"]));
        assert_eq!(e["operation_expressible"], true);
    }

    #[test]
    fn a_later_output_cannot_supply_an_earlier_discarded_operation() {
        let record = empty_record();
        let e = exported_case(
            "future-tool-operand",
            vec![call("copy", &[9], Ok(vec![2])), call("extrude", &[], Ok(vec![9])), Event::Export { id: 9, record: Box::new(record.clone()) }],
            Ok(vec![("Part".into(), record)]),
        );
        assert_eq!(e["operation_expressible"], false);
        assert!(e["why_not"].as_array().unwrap().iter().any(|why| why.as_str().unwrap().contains("9")));
    }

    #[test]
    fn the_area_is_the_path_under_kernel_cases() {
        assert_eq!(area_and_id("tests/kernel-cases/booleans/1894-x").unwrap(), ("booleans".into(), "1894-x".into()));
        assert_eq!(area_and_id("/r/tests/kernel-cases/ladder/07-fillets/003-y").unwrap(), ("ladder/07-fillets".into(), "003-y".into()));
        assert!(area_and_id("/r/other/x").is_err());
    }
}
