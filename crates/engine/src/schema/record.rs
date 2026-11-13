//! The audit recorder, for checking a new contract against every caller the tests have: in a
//! build with the `param-audit` feature, when `SOLIDCRAFT_PARAM_AUDIT` names a directory, every
//! call to a command with a contract that breaks it is recorded, with what the command did, so a
//! test run shows which callers the contracts would warn about or refuse. Without the feature it
//! is compiled (so it is linted and tested) but never on.
//!
//! ```text
//! SOLIDCRAFT_PARAM_AUDIT=$PWD/audit cargo test --workspace --no-fail-fast --features solidcraft-engine/param-audit
//! python3 scripts/param_audit.py audit
//! ```
//!
//! A line whose outcome is `ok` is a call that works today and that strict would refuse: a form
//! the contract is missing, or a mistake the handler silently ignores.
//!
//! Records are aggregated per process by signature (command, issues, outcome, test thread) and
//! written to `<dir>/audit-<pid>.jsonl` when a signature is first seen and again each time its
//! count reaches a power of two; readers keep the highest `count` per signature in each file
//! and add the files up. A process keeps at most 4096 signatures and counts later new ones
//! under one `dropped` line. Writing never fails a command.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use super::check::{Issue, truncate};
use crate::Result;

static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static SEEN: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

/// The most signatures a process keeps; later new ones are dropped, and one line says so.
const MAX_SIGNATURES: usize = 4096;

fn dir() -> Option<&'static PathBuf> {
    DIR.get_or_init(|| std::env::var_os("SOLIDCRAFT_PARAM_AUDIT").filter(|v| !v.is_empty()).map(PathBuf::from)).as_ref()
}

/// Whether the recorder is on: built with `param-audit` and the variable set (read once per
/// process).
pub fn enabled() -> bool {
    cfg!(feature = "param-audit") && dir().is_some()
}

/// Count one more call with signature `key`. A new signature beyond `max` is counted under
/// one `dropped` entry instead. Returns whether it was dropped and the count, when the count is
/// a power of two (a line is due).
pub(crate) fn tally(seen: &mut HashMap<String, u64>, key: String, max: usize) -> Option<(bool, u64)> {
    let dropped = seen.len() >= max && !seen.contains_key(&key);
    let count = seen.entry(if dropped { "dropped".into() } else { key }).or_insert(0);
    *count += 1;
    count.is_power_of_two().then_some((dropped, *count))
}

/// One line of the audit file; `sample_params` is cut to 1 KB (empty for `null`).
pub(crate) fn line(command: &str, issues: &[impl serde::Serialize], outcome: &str, source: &str, count: u64, params: &Value) -> Value {
    let sample = if params.is_null() { String::new() } else { truncate(&params.to_string(), 1024) };
    json!({"command": command, "issues": issues, "outcome": outcome, "source": source, "count": count, "sample_params": sample})
}

/// What a call did, for its audit line: `ok`, `error: …` from the handler, or `refused` when
/// strict refused it before the handler ran (`None`).
pub(crate) fn outcome_text(outcome: Option<&Result<Value>>) -> String {
    match outcome {
        None => "refused".to_string(),
        Some(Ok(_)) => "ok".to_string(),
        Some(Err(e)) => format!("error: {}", truncate(&e.to_string(), 200)),
    }
}

/// Record one call of `command` that broke its contract with `issues`, and what it did
/// (`None`: refused before the handler ran).
pub fn note(command: &str, issues: &[Issue], outcome: Option<&Result<Value>>, params: &Value) {
    let Some(dir) = dir().filter(|_| enabled()) else { return };
    // A handler's own message names the caller's value, which may be long.
    let mut messages: Vec<String> = issues.iter().map(|i| truncate(&i.message, 300)).collect();
    messages.sort_unstable();
    messages.dedup();
    let outcome = outcome_text(outcome);
    let source = std::thread::current().name().unwrap_or("unnamed").to_string();
    let key = json!([command, messages, outcome, source]).to_string();
    let seen = SEEN.get_or_init(Default::default);
    // The lock is held while writing, so this process's lines never interleave.
    let mut seen = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let line = match tally(&mut seen, key, MAX_SIGNATURES) {
        None => return,
        Some((true, count)) => line("*", &["too many signatures; later new ones dropped"], "dropped", "*", count, &Value::Null),
        Some((false, count)) => line(command, &messages, &outcome, &source, count, params),
    };
    let path = dir.join(format!("audit-{}.jsonl", std::process::id()));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}
