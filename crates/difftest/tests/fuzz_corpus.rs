//! Replays the `--fuzz` crash, hang and panic inputs kept in `tests/fuzz_corpus/`
//! (needs `--features cpp`).
//!
//! Each `tests/fuzz_corpus/<unit>.json` holds inputs that once crashed, hung or panicked
//! a side of `<unit>` under `difftest --fuzz` (the sweep of 2026-10-20 and later runs),
//! reduced to the smallest form that still reaches the same outcome:
//!
//! ```json
//! { "unit": "curve.composite_curve_p1",
//!   "cases": [ { "name": "fuzz00767_join_knot_span",
//!                "origin": "n2000/curve.composite_curve_p1/fuzz00767 (crash: cpp SIGSEGV)",
//!                "issue": "UB-73",
//!                "cpp": "harness: invalid argument: approx: ...",
//!                "rust": "harness: invalid argument: approx: ...",
//!                "inputs": { ... } } ] }
//! ```
//!
//! `cpp` and `rust` are the outcome each side must still give: `harness: <message>`
//! (the runner's `HarnessError`, e.g. an FFI guard refusing the input), `rejected:
//! <reason>`, `error: <normalised error>`, `deferred: <unit>` or `ok`. A `rust` of
//! plain `deferred` (the Rust side defers on an unported callee today) accepts any
//! outcome but a panic, so wiring that callee needs no edit here. Instead of
//! `cpp`, a case may carry `cpp_skip: "<reason>"`: the input still crashes or hangs the
//! C++ reference, so only the Rust side runs (it must return, without a panic). When
//! both sides give `ok` or `error:`, their outputs must also agree as in the gate.
//!
//! These inputs cannot be fixtures in `fixtures/<unit>/`: a guard refusal is a harness
//! error there, which always fails the case, and a C++ crash would kill the run. A
//! panic here fails the test, so a refactor that reopens a guard or a Rust panic shows.
//! `FUZZ_CORPUS_ONLY=<unit>` limits the run to one file, `FUZZ_CORPUS_DIR=<dir>` reads
//! the files from another directory, and `FUZZ_CORPUS_RECORD=1` prints each case's
//! actual outcomes (`@outcome <unit> <name> <cpp> | <rust>`) instead of checking them,
//! to re-record a file after an intended change.
#![cfg(feature = "cpp")]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use difftest::case::repo_root;
use difftest::fixtures::expand_inputs;
use difftest::{compare, registry, Case, CaseSource, HarnessError, Output};
use serde_json::Value;

/// The one-line outcome of one side, in the files' `cpp`/`rust` notation.
fn outcome(r: &Result<Output, HarnessError>) -> String {
    one_line(&match r {
        Err(e) => format!("harness: {}", e.0),
        Ok(o) => {
            if let Some(d) = &o.deferred {
                format!("deferred: {}", d.unit)
            } else if let Some(r) = &o.rejected {
                format!("rejected: {r}")
            } else if let Some(e) = &o.error {
                format!("error: {e}")
            } else {
                "ok".into()
            }
        }
    })
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Runs one side, turning a panic into `Err(message)`.
fn call(
    f: impl FnOnce() -> Result<Output, HarnessError>,
) -> Result<Result<Output, HarnessError>, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| {
        p.downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_default()
    })
}

fn str_field<'a>(v: &'a Value, key: &str, at: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{at}: missing string `{key}`"))
}

/// Replays one case file; returns the cases whose outcomes changed.
fn replay_file(root: &Path, path: &Path, record: bool) -> Result<Vec<String>, String> {
    let at = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{at}: {e}"))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{at}: {e}"))?;
    let unit_id = str_field(&doc, "unit", &at)?;
    if path.file_stem().and_then(|s| s.to_str()) != Some(unit_id) {
        return Err(format!("{at}: file name must be the unit id `{unit_id}`"));
    }
    let reg = registry();
    let unit = reg
        .iter()
        .find(|u| u.id() == unit_id)
        .ok_or_else(|| format!("{unit_id}: no runner registered"))?;
    let cases = doc
        .get("cases")
        .and_then(Value::as_array)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| format!("{at}: no `cases`"))?;
    let mut problems = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for c in cases {
        let name = str_field(c, "name", &at)?;
        let at = format!("{unit_id}/{name}");
        if !names.insert(name) {
            return Err(format!("{at}: duplicate case name"));
        }
        str_field(c, "origin", &at)?;
        let raw = c.get("inputs").cloned().unwrap_or(Value::Null);
        let case = Case {
            unit: unit_id.to_string(),
            name: name.to_string(),
            source: CaseSource::Inline,
            inputs: expand_inputs(root, raw).map_err(|e| format!("{at}: {e}"))?,
        };
        let cpp = if c.get("cpp_skip").is_some() {
            str_field(c, "cpp_skip", &at)?;
            if c.get("cpp").is_some() {
                return Err(format!("{at}: both `cpp` and `cpp_skip`"));
            }
            None
        } else {
            let want = if record {
                ""
            } else {
                str_field(c, "cpp", &at)?
            };
            match call(|| unit.run_cpp(&case)) {
                Err(m) => {
                    problems.push(format!("{at}: the C++ side panicked: {m}"));
                    continue;
                }
                Ok(r) => {
                    let got = outcome(&r);
                    if !record && got != want {
                        problems.push(format!("{at}: cpp gave `{got}`, expected `{want}`"));
                    }
                    Some(r)
                }
            }
        };
        let want = if record {
            ""
        } else {
            str_field(c, "rust", &at)?
        };
        let r = match call(|| unit.run_rust(&case)) {
            Err(m) => {
                problems.push(format!("{at}: the Rust side panicked: {m}"));
                continue;
            }
            Ok(r) => r,
        };
        let got = outcome(&r);
        if record {
            let cpp = cpp.as_ref().map_or_else(|| "skip".to_string(), outcome);
            println!("@outcome {unit_id} {name} {cpp} | {got}");
        } else if got != want && want != "deferred" {
            problems.push(format!("{at}: rust gave `{got}`, expected `{want}`"));
        }
        if let (Some(Ok(cpp)), Ok(rust)) = (&cpp, &r) {
            if cpp.rejected.is_none() && cpp.deferred.is_none() && rust.deferred.is_none() {
                let d = compare(cpp, rust, unit.abs_floors());
                if let Some(m) = d.mismatches.first() {
                    problems.push(format!(
                        "{at}: sides differ at {}: cpp={} rust={}",
                        m.field, m.cpp, m.rust
                    ));
                }
            }
        }
    }
    Ok(problems)
}

#[test]
fn fuzz_corpus_inputs_keep_their_outcomes() {
    let root = repo_root();
    let dir = std::env::var_os("FUZZ_CORPUS_DIR").map_or_else(
        || root.join("crates/difftest/tests/fuzz_corpus"),
        std::path::PathBuf::from,
    );
    let record = std::env::var_os("FUZZ_CORPUS_RECORD").is_some();
    let only = std::env::var("FUZZ_CORPUS_ONLY").ok();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| {
            only.as_deref()
                .is_none_or(|u| p.file_stem().is_some_and(|s| s == u))
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no case files in {}", dir.display());
    let mut problems = Vec::new();
    for f in &files {
        match replay_file(&root, f, record) {
            Ok(p) => problems.extend(p),
            Err(e) => problems.push(e),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
