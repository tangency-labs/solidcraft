//! Deterministic per-unit JSON report (`reports/difftest/<unit>.json`).
//!
//! No timestamps, no paths outside the repo, sorted keys, stable floats.
//! Passing cases are listed compactly; failing cases include their inputs
//! and the first recorded mismatches so they can be reproduced.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::compare::REL_TOL;
use crate::json::{num, to_canonical_string};
use crate::runner::{CaseResult, UnitResult, UnitStatus};
use crate::HarnessError;

fn case_json(c: &CaseResult) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), json!(c.name));
    m.insert("source".into(), json!(c.source.describe()));
    m.insert("pass".into(), json!(c.pass()));
    m.insert("outcome".into(), json!(c.outcome().as_str()));
    if let Some(reason) = &c.rejected {
        m.insert("rejected".into(), json!(reason));
        m.insert("inputs".into(), c.inputs.clone());
    }
    if let Some(d) = &c.deferred {
        m.insert(
            "deferred".into(),
            json!({"unit": d.unit, "symbol": d.symbol}),
        );
    }
    m.insert("max_rel_err".into(), num(c.diff.max_rel_err));
    m.insert("values_compared".into(), json!(c.diff.values_compared));
    if !c.diagnostics.is_empty() {
        m.insert("diagnostics".into(), json!(c.diagnostics));
    }
    if c.both_errored {
        m.insert("both_errored".into(), json!(true));
    }
    if c.is_vacuous() {
        m.insert("vacuous".into(), json!(true));
    } else if c.pass() && !c.both_errored && c.diff.nonzero_compared == 0 {
        m.insert("only_zeros".into(), json!(true));
    }
    if let Some(why) = &c.known_mismatch {
        m.insert("known_mismatch".into(), json!(why));
    }
    if c.fail() || c.is_known_mismatch() {
        m.insert("inputs".into(), c.inputs.clone());
        m.insert("mismatch_count".into(), json!(c.diff.mismatch_count));
        let mm: Vec<Value> = c
            .diff
            .mismatches
            .iter()
            .map(|x| {
                let mut o = Map::new();
                o.insert("field".into(), json!(x.field));
                o.insert("cpp".into(), json!(x.cpp));
                o.insert("rust".into(), json!(x.rust));
                if let Some(r) = x.rel_err {
                    o.insert("rel_err".into(), num(r));
                }
                Value::Object(o)
            })
            .collect();
        m.insert("mismatches".into(), Value::Array(mm));
        if let Some(h) = &c.harness_error {
            m.insert("harness_error".into(), json!(h));
        }
    }
    Value::Object(m)
}

/// Builds the report value for a unit.
pub fn unit_report(r: &UnitResult) -> Value {
    let floors: Vec<Value> = r
        .abs_floors
        .iter()
        .map(|f| json!({"key": f.key, "abs": num(f.abs), "why": f.why}))
        .collect();
    let errored = r.cases.iter().filter(|c| c.both_errored).count();
    let mut m = Map::new();
    m.insert("unit".into(), json!(r.unit));
    m.insert("pass".into(), json!(r.pass()));
    m.insert("status".into(), json!(r.status().as_str()));
    m.insert("rel_tol".into(), num(REL_TOL));
    m.insert("abs_floors".into(), Value::Array(floors));
    m.insert("seeds".into(), json!(r.seeds));
    m.insert("cases_total".into(), json!(r.cases.len()));
    m.insert("cases_passed".into(), json!(r.passed()));
    m.insert("cases_failed".into(), json!(r.failed()));
    m.insert("cases_deferred".into(), json!(r.deferred()));
    if r.rejected() > 0 {
        m.insert("cases_rejected".into(), json!(r.rejected()));
    }
    let by_unit: Map<String, Value> = r
        .deferred_by_unit()
        .into_iter()
        .map(|(u, n)| (u, json!(n)))
        .collect();
    m.insert("deferred_by_unit".into(), Value::Object(by_unit));
    m.insert("cases_both_errored".into(), json!(errored));
    m.insert("cases_vacuous".into(), json!(r.vacuous()));
    if r.known_mismatches() > 0 {
        m.insert("cases_known_mismatch".into(), json!(r.known_mismatches()));
    }
    m.insert("max_rel_err".into(), num(r.max_rel_err()));
    m.insert(
        "cases".into(),
        Value::Array(r.cases.iter().map(case_json).collect()),
    );
    if let Some(h) = &r.harness_error {
        m.insert("harness_error".into(), json!(h));
    }
    Value::Object(m)
}

/// Writes `<dir>/<unit>.json`; returns the path.
pub fn write_unit_report(dir: &Path, r: &UnitResult) -> Result<PathBuf, HarnessError> {
    fs::create_dir_all(dir).map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let path = dir.join(format!("{}.json", r.unit));
    fs::write(&path, to_canonical_string(&unit_report(r)))
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
    Ok(path)
}

/// One-line summary, e.g.
/// `PASS harness.selftest cases=40 passed=40 failed=0 deferred=0 max_rel_err=1.1e-16 with_values=36 no_values=4 vacuous=1`.
/// The status word is `PASS` (green), `PARTIAL` or `FAIL`; `with_values` / `no_values`
/// split the passing cases into those that compared at least one value and those that
/// compared nothing (the same error on both sides, or empty outputs); `vacuous` counts
/// the `no_values` cases where both sides succeeded, so only the status agreed; with deferred
/// cases the line ends with ` blocked_by=<unit>(<cases>),...`.
pub fn summary_line(r: &UnitResult) -> String {
    let with_values = r.passed_with_values();
    let mut s = format!(
        "{} {} cases={} passed={} failed={} deferred={} max_rel_err={:e} with_values={with_values} no_values={} vacuous={}",
        match r.status() {
            UnitStatus::Green => "PASS",
            UnitStatus::Partial => "PARTIAL",
            UnitStatus::Fail => "FAIL",
        },
        r.unit,
        r.cases.len(),
        r.passed(),
        r.failed(),
        r.deferred(),
        r.max_rel_err(),
        r.passed() - with_values,
        r.vacuous()
    );
    if r.rejected() > 0 {
        let _ = write!(s, " rejected={}", r.rejected());
    }
    if r.known_mismatches() > 0 {
        let _ = write!(s, " known_mismatch={}", r.known_mismatches());
    }
    let blocked: Vec<String> = r
        .deferred_by_unit()
        .iter()
        .map(|(u, n)| format!("{u}({n})"))
        .collect();
    if !blocked.is_empty() {
        let _ = write!(s, " blocked_by={}", blocked.join(","));
    }
    if let Some(h) = &r.harness_error {
        let _ = write!(s, " harness_error={h:?}");
    } else if r.cases.is_empty() {
        s.push_str(" (no cases)");
    } else if r.cases.len() == r.rejected() {
        s.push_str(" (no comparable cases)");
    }
    s
}
