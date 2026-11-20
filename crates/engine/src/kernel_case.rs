//! A failed operation's kernel case: the failing command run again on a scratch copy of the
//! session with the kernel's operations traced ([`solidcraft_kernel::smlib_trace`]), written as
//! one entry of solidcore's kernel rig (its corpus README's format, as `solidcraft-cli
//! kernel-cases --export` writes it, the operand records in a gzip beside it). It replays on the
//! kernel alone (`KERNEL_CASES_ENTRY`) and is checked against the failing call's stable code.
//!
//! It holds only what the failing operation took: the kernel calls that made its operands from
//! the bodies they started from, and those bodies as records, not the design. Nothing here
//! leaves the computer: a report attaches it only when its box is ticked.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcraft_kernel::smlib_trace::{Event, Run};

use crate::Session;

/// A kernel case: its name, its entry (`<name>.json`) and its operand records
/// (`<name>.bodies.json.gz`), and the failing call's code.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelCase {
    pub name: String,
    pub entry: Vec<u8>,
    pub bodies: Vec<u8>,
    pub code: Option<solidcraft_kernel::ErrorCode>,
}

impl KernelCase {
    /// The entry's file name.
    pub fn entry_name(&self) -> String {
        format!("{}.json", self.name)
    }

    /// The operand records' file name.
    pub fn bodies_name(&self) -> String {
        format!("{}.bodies.json.gz", self.name)
    }
}

/// The calls of a trace and the records bodies came in as, as an entry writes them.
pub struct Traced {
    /// Each call: `op`, `inputs` (`b<n>`), `params`, and `outputs` or `error` (with the kernel
    /// call's `code` when it has one).
    pub calls: Vec<Value>,
    /// The records of the bodies the calls take that came in as records: `b<n>` to
    /// `{trusted, keeping_splines, record}`.
    pub bodies: BTreeMap<String, Value>,
}

/// A call's JSON as an entry writes it.
pub fn call_json(op: &str, inputs: &[u64], params: &Value, outputs: &Result<Vec<u64>, String>, code: Option<&solidcraft_kernel::ErrorCode>) -> Value {
    let mut c = json!({"op": op, "inputs": inputs.iter().map(|i| format!("b{i}")).collect::<Vec<_>>(), "params": params});
    match outputs {
        Ok(made) => c["outputs"] = json!(made.iter().map(|i| format!("b{i}")).collect::<Vec<_>>()),
        Err(e) => {
            c["error"] = json!(e);
            // the stable code of the kernel call it failed with: what a replay is checked against
            if let Some(code) = code {
                c["code"] = json!({"call": code.call, "kind": code.kind, "operation": code.operation});
            }
        }
    }
    c
}

/// A body that came in as a record, as an entry's `bodies` writes it.
pub fn body_json(record: &solidcore_record::brep::Brep, trusted: bool, keeping_splines: bool) -> Value {
    json!({"trusted": trusted, "keeping_splines": keeping_splines, "record": record})
}

/// The failing call of a trace that stopped in the kernel: the last that failed, followed by no
/// call but copies (which build nothing). `None` when the failure is not a kernel call's.
fn failing(events: &[Event]) -> Option<usize> {
    let calls: Vec<(usize, &Event)> = events.iter().enumerate().filter(|(_, e)| matches!(e, Event::Call { .. })).collect();
    let last = calls.iter().rposition(|(_, e)| matches!(e, Event::Call { outputs: Err(_), .. }))?;
    let built_after = calls.iter().skip(last + 1).any(|(_, e)| !matches!(e, Event::Call { op: "copy" | "tagged", .. }));
    (!built_after).then(|| calls[last].0)
}

/// The kernel case of the failure the command `command` with `params` gives on a scratch copy of
/// `session`, rebuilt with the kernel's operations traced, its result not taken from the
/// evaluation cache. `commit` names the build (SolidCraft's commit, whose `solidcore/` is the kernel).
///
/// # Errors
/// Why there is no case: the command builds now, or its failure is no kernel call's.
pub fn rebuild(session: &Session, command: &str, params: &Value, commit: &str) -> Result<KernelCase, String> {
    // The features before it are taken as they are (the operation's operands come in as their
    // records); only the operation is built again, with a cache of its own, so the failure the
    // session's cache keeps is computed anew.
    let mut scratch = session.scratch();
    scratch.model.cache = std::sync::Arc::new(std::sync::Mutex::new(solidcraft_doc::EvalCache::default()));
    let run = Run::new();
    let ran = run.within(|| scratch.execute(command, params));
    let events = run.take();
    match ran {
        Ok(_) => Err("the operation builds now".into()),
        Err(e) => of_failure(&events, command, &e.to_string(), commit),
    }
}

/// The kernel case of a traced run that failed with `error` in `command`: the failing call and the
/// calls that made its operands, with the records those started from.
///
/// # Errors
/// Why there is none: the failure is no kernel call's, or a body it needs came from nothing traced.
pub fn of_failure(events: &[Event], command: &str, error: &str, commit: &str) -> Result<KernelCase, String> {
    let at = failing(events).ok_or_else(|| format!("the failure is SolidCraft's, not a kernel operation's: {error}"))?;
    // the calls that made each body, and the records bodies came in as
    let mut maker: HashMap<u64, usize> = HashMap::new();
    let mut imported: HashMap<u64, (&solidcore_record::brep::Brep, bool, bool)> = HashMap::new();
    for (i, e) in events.iter().enumerate().take(at) {
        match e {
            Event::Import { id, record, trusted, keeping_splines } => {
                imported.insert(*id, (record, *trusted, *keeping_splines));
            }
            Event::Call { outputs: Ok(made), .. } => {
                for id in made {
                    maker.insert(*id, i);
                }
            }
            _ => {}
        }
    }
    // what the failing call needs, back to the records it starts from
    let mut keep: BTreeSet<usize> = BTreeSet::from([at]);
    let mut needed: Vec<u64> = match &events[at] {
        Event::Call { inputs, .. } => inputs.clone(),
        _ => Vec::new(),
    };
    let mut records: BTreeSet<u64> = BTreeSet::new();
    let mut seen = BTreeSet::new();
    while let Some(id) = needed.pop() {
        if !seen.insert(id) {
            continue;
        }
        if imported.contains_key(&id) {
            records.insert(id);
        } else if let Some(&i) = maker.get(&id) {
            keep.insert(i);
            if let Event::Call { inputs, .. } = &events[i] {
                needed.extend(inputs.iter().copied());
            }
        } else {
            return Err(format!("body b{id} the failing operation needs was made by nothing traced"));
        }
    }
    let mut calls = Vec::new();
    let mut ops = Vec::new();
    let mut code = None;
    for &i in &keep {
        if let Event::Call { op, inputs, params, outputs, code: c } = &events[i] {
            let c = c.map(solidcraft_kernel::ErrorCode::from);
            if i == at {
                code = c.clone();
            }
            calls.push(call_json(op, inputs, params, outputs, c.as_ref()));
            ops.push(*op);
        }
    }
    let mut bodies = serde_json::Map::new();
    let mut gz = serde_json::Map::new();
    for id in &records {
        let (record, trusted, splines) = imported[id];
        bodies.insert(format!("b{id}"), json!({"trusted": trusted, "keeping_splines": splines}));
        gz.insert(format!("b{id}"), json!(record));
    }
    // named by what it holds, nothing of the design's names
    let calls_text = serde_json::to_string(&calls).map_err(|e| e.to_string())?;
    let digest = Sha256::digest(format!("{calls_text}{}", serde_json::to_string(&gz).unwrap_or_default()).as_bytes());
    let name = format!("report-{}", digest.iter().take(6).map(|b| format!("{b:02x}")).collect::<String>());
    let kernel_error = match &events[at] {
        Event::Call { outputs: Err(e), .. } => e.clone(),
        _ => String::new(),
    };
    let entry = json!({
        "case": name,
        "area": "reports",
        "sources": [],
        "what": format!("{command} failed: {kernel_error}"),
        "expressible": false,
        "operation_expressible": true,
        "why_not": [],
        "ungraded": ["a reported failure: no expected values"],
        "records": format!("{name}.bodies.json.gz"),
        "calls": calls,
        "bodies": bodies,
        "result": [],
        "outcome": {"ran": false, "engine_error": error, "kernel_error": kernel_error},
        "traced_ops": ops,
        "fixes": solidcraft_kernel::smlib_trace::fixes(),
        "solidcraft": commit,
        "solidcore": commit,
    });
    let entry = serde_json::to_vec_pretty(&entry).map_err(|e| e.to_string())?;
    let mut w = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut w, serde_json::to_string(&gz).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    let bodies = w.finish().map_err(|e| e.to_string())?;
    Ok(KernelCase { name, entry, bodies, code })
}

#[cfg(test)]
#[path = "kernel_case_tests.rs"]
mod tests;
