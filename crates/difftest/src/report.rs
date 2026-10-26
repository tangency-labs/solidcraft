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
    m.insert("exact_compared".into(), json!(c.diff.exact_compared));
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
    if let Some(id) = &c.cpp_defect {
        m.insert("cpp_defect".into(), json!(id));
    }
    if c.fail() || c.is_known_mismatch() || c.is_cpp_defect() {
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
    m.insert("schema_version".into(), json!(1));
    m.insert("unit".into(), json!(r.unit));
    m.insert("pass".into(), json!(r.pass()));
    m.insert("status".into(), json!(r.status().as_str()));
    if let Some(reason) = &r.skipped_platform {
        m.insert("skipped_platform".into(), json!(reason));
    }
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
    if r.skipped_platform() > 0 {
        m.insert("cases_skipped_platform".into(), json!(r.skipped_platform()));
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
    if r.cpp_defects() > 0 {
        m.insert("cases_cpp_defect".into(), json!(r.cpp_defects()));
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

/// Aggregate counts recorded in a report (including vacuous and known outcomes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportCounts {
    /// `green`, `partial`, `fail` or `skipped(platform)`.
    pub status: String,
    /// All cases.
    pub total: u64,
    /// Passing cases.
    pub passed: u64,
    /// Failing cases.
    pub failed: u64,
    /// Deferred cases.
    pub deferred: u64,
    /// Rejected native fixtures.
    pub rejected: u64,
    /// Known mismatches (never counted as passing cases).
    pub known: u64,
    /// Confirmed C++ reference defects (successes, but not passing cases).
    pub cpp_defect: u64,
    /// Successful cases with no numeric/topological comparisons.
    pub vacuous: u64,
}

impl ReportCounts {
    /// Counts of a run.
    pub fn of(r: &UnitResult) -> Self {
        let n = |x: usize| u64::try_from(x).unwrap_or(u64::MAX);
        Self {
            status: r.status().as_str().to_string(),
            total: n(r.cases.len()),
            passed: n(r.passed()),
            failed: n(r.failed()),
            deferred: n(r.deferred()),
            rejected: n(r.rejected()),
            known: n(r.known_mismatches()),
            cpp_defect: n(r.cpp_defects()),
            vacuous: n(r.vacuous()),
        }
    }

    /// Counts recorded in report text; `None` if it is not a unit report.
    pub fn parse(text: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(text).ok()?;
        let n = |k: &str| v.get(k).and_then(Value::as_u64);
        Some(Self {
            status: v.get("status")?.as_str()?.to_string(),
            total: n("cases_total")?,
            passed: n("cases_passed")?,
            failed: n("cases_failed")?,
            deferred: n("cases_deferred")?,
            rejected: n("cases_rejected").unwrap_or(0),
            known: n("cases_known_mismatch").unwrap_or(0),
            cpp_defect: n("cases_cpp_defect").unwrap_or(0),
            vacuous: n("cases_vacuous").unwrap_or(0),
        })
    }

    /// `status=… cases=… passed=… failed=… deferred=… rejected=…`.
    pub fn describe(&self) -> String {
        format!(
            "status={} cases={} passed={} failed={} deferred={} rejected={} known={} cpp_defect={} vacuous={}",
            self.status,
            self.total,
            self.passed,
            self.failed,
            self.deferred,
            self.rejected,
            self.known,
            self.cpp_defect,
            self.vacuous
        )
    }
}

/// Aggregate CLI summary. Skipped units never contribute to the passed count.
/// The optional suffix preserves existing output when all references are supported.
pub fn run_summary(total: usize, partial: usize, failed: usize, skipped: usize) -> String {
    let mut s = format!(
        "difftest: {total} unit(s), {} passed, {partial} partial, {failed} failed",
        total.saturating_sub(partial + failed + skipped)
    );
    if skipped > 0 {
        let _ = write!(s, ", {skipped} skipped(platform)");
    }
    s
}

/// One-line summary, e.g.
/// `PASS harness.selftest cases=40 passed=40 failed=0 deferred=0 max_rel_err=1.1e-16 with_values=36 no_values=4 vacuous=1`.
/// The status word is `PASS` (green), `PARTIAL`, `FAIL` or `SKIP`; a skip prints
/// only the unit and its platform reason. Otherwise, `with_values` / `no_values`
/// split the passing cases into those that compared at least one value and those that
/// compared nothing (the same error on both sides, or empty outputs); `vacuous` counts
/// the `no_values` cases where both sides succeeded, so only the status agreed; with deferred
/// cases the line ends with ` blocked_by=<unit>(<cases>),...`.
pub fn summary_line(r: &UnitResult) -> String {
    if r.status() == UnitStatus::SkippedPlatform {
        return format!(
            "SKIP {} skipped(platform): {}",
            r.unit,
            r.skipped_platform.as_deref().unwrap_or_default()
        );
    }
    let with_values = r.passed_with_values();
    let mut s = format!(
        "{} {} cases={} passed={} failed={} deferred={} max_rel_err={:e} with_values={with_values} no_values={} vacuous={}",
        match r.status() {
            UnitStatus::Green => "PASS",
            UnitStatus::Partial => "PARTIAL",
            UnitStatus::Fail => "FAIL",
            UnitStatus::SkippedPlatform => "SKIP",
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
    if r.cpp_defects() > 0 {
        let _ = write!(s, " cpp_defect={}", r.cpp_defects());
    }
    if r.skipped_platform() > 0 {
        let _ = write!(s, " skipped_platform={}", r.skipped_platform());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_round_trip_through_a_report() {
        let r = UnitResult {
            unit: "a.b".into(),
            cases: Vec::new(),
            harness_error: None,
            skipped_platform: None,
            abs_floors: Vec::new(),
            seeds: 16,
            timing: crate::runner::UnitTiming::default(),
        };
        let text = to_canonical_string(&unit_report(&r));
        assert_eq!(ReportCounts::parse(&text), Some(ReportCounts::of(&r)));
        assert_eq!(
            ReportCounts::of(&r).describe(),
            "status=fail cases=0 passed=0 failed=0 deferred=0 rejected=0 known=0 cpp_defect=0 vacuous=0"
        );
    }

    #[test]
    fn old_reports_without_rejected_parse() {
        let c = ReportCounts::parse(
            r#"{"status":"partial","cases_total":3,"cases_passed":1,"cases_failed":0,"cases_deferred":2}"#,
        )
        .unwrap();
        assert_eq!((c.deferred, c.rejected), (2, 0));
        assert_eq!(ReportCounts::parse(r#"{"unit":"x"}"#), None);
    }

    use crate::case::CaseSource;
    use crate::compare::compare;
    use crate::output::Output;

    #[test]
    fn known_mismatch_reports_distinguish_integer_checks_from_length_only_failures() {
        for (cpp, rust, expected) in [
            (Output::new().int("ids", 1), Output::new().int("ids", 2), 1),
            (
                Output::new().ints("ids", vec![1, 2]),
                Output::new().ints("ids", vec![1]),
                0,
            ),
        ] {
            let case = CaseResult {
                name: "known".into(),
                source: CaseSource::Inline,
                inputs: Value::Null,
                diff: compare(&cpp, &rust, &[]),
                harness_error: None,
                both_errored: false,
                rejected: None,
                deferred: None,
                skipped_platform: None,
                perturbed: None,
                diagnostics: std::collections::BTreeMap::new(),
                known_mismatch: Some("test comparison".into()),
                cpp_defect: None,
            };
            let report = case_json(&case);
            assert_eq!(report["outcome"], "known_mismatch");
            assert_eq!(report["values_compared"], 0);
            assert_eq!(report["exact_compared"], expected);
        }
    }

    #[test]
    fn cpp_defects_are_reported_separately_and_keep_the_unit_green() {
        let case = CaseResult {
            name: "defect".into(),
            source: CaseSource::Inline,
            inputs: json!({"file": "x"}),
            diff: compare(&Output::new().int("n", 1), &Output::new().int("n", 2), &[]),
            harness_error: None,
            both_errored: false,
            rejected: None,
            deferred: None,
            skipped_platform: None,
            perturbed: None,
            diagnostics: std::collections::BTreeMap::new(),
            known_mismatch: Some("C++ uses a freed pointer".into()),
            cpp_defect: Some("UB-1".into()),
        };
        let c = case_json(&case);
        assert_eq!(c["outcome"], "cpp_defect");
        assert_eq!(c["pass"], false);
        assert_eq!(c["cpp_defect"], "UB-1");
        assert_eq!(c["mismatch_count"], 1);
        let r = UnitResult {
            unit: "a.b".into(),
            cases: vec![case],
            harness_error: None,
            skipped_platform: None,
            abs_floors: Vec::new(),
            seeds: 16,
            timing: crate::runner::UnitTiming::default(),
        };
        let v = unit_report(&r);
        assert_eq!((&v["status"], &v["pass"]), (&json!("green"), &json!(true)));
        assert_eq!(v["cases_cpp_defect"], 1);
        assert_eq!(v["cases_passed"], 0);
        assert!(v.get("cases_known_mismatch").is_none());
        let text = to_canonical_string(&v);
        assert_eq!(ReportCounts::parse(&text), Some(ReportCounts::of(&r)));
        assert!(summary_line(&r).contains(" cpp_defect=1"));
        let b = crate::baseline::parse_baseline(&text).unwrap();
        assert_eq!(b.cases[0].outcome, "cpp_defect");
    }
}
