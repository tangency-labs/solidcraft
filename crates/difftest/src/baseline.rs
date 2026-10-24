//! Regression gates over a unit run: the baseline ratchet and the value-share floor.
//!
//! * **Ratchet** (`--baseline <dir>`): compares a run with the unit's earlier report,
//!   from the exact base revision's validated CI artifact. Every case that
//!   passed there must still exist and pass, and must still compare values if it did;
//!   no deferred or failing case may disappear either. A deferred case may turn into a
//!   pass (progress); a rejected fixture may go away. Different `--seeds` are a gate failure, so generated cases cannot be hidden.
//! * **Value share**: at least [`MIN_VALUE_SHARE`] of a unit's passing cases, and at
//!   least one, must compare a value (a float, integer or topology entry) rather than
//!   only an error string both sides agreed on.
//!
//! A violation is waived only by an entry in [`WAIVERS_PATH`] that names the unit,
//! the check and (for the ratchet, optionally) the case, with a reason.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::runner::{CaseOutcome, UnitResult};
use crate::HarnessError;

/// Minimum fraction of passing cases that must compare at least one value.
pub const MIN_VALUE_SHARE: f64 = 0.75;

/// Repo-relative path of the waiver file.
pub const WAIVERS_PATH: &str = "plan/difftest-waivers.toml";

/// Which gate a [`Violation`] or [`Waiver`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// A case or unit covered by the baseline got worse or disappeared.
    Ratchet,
    /// Too few passing cases compare a value.
    ValueShare,
}

impl Check {
    /// `ratchet` or `value-share` (the spelling used in the waiver file).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ratchet => "ratchet",
            Self::ValueShare => "value-share",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "ratchet" => Some(Self::Ratchet),
            "value-share" => Some(Self::ValueShare),
            _ => None,
        }
    }
}

/// One gate violation of a unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Unit id.
    pub unit: String,
    /// Gate that was violated.
    pub check: Check,
    /// Case name, if the violation is about one case.
    pub case: Option<String>,
    /// Human-readable explanation.
    pub detail: String,
}

/// One case of a baseline report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineCase {
    /// Case name (unique within the unit).
    pub name: String,
    /// `pass`, `fail`, `deferred`, `known_mismatch` or `rejected`.
    pub outcome: String,
    /// The case came from the generator (its inputs depend on `--seeds`).
    pub generated: bool,
    /// The case compared at least one value. Reports written before `exact_compared`
    /// was recorded use #550 vacuity semantics to prove integer-only coverage.
    pub compared_values: bool,
}

/// A unit's earlier report, as far as the ratchet needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// Unit id.
    pub unit: String,
    /// Plan-independent status the report recorded (`green`, `partial`, `fail`).
    pub status: Option<String>,
    /// `--seeds` used for the report.
    pub seeds: Option<u64>,
    /// Cases in report order.
    pub cases: Vec<BaselineCase>,
}

/// Parses a report written by [`crate::report::write_unit_report`].
pub fn parse_baseline(text: &str) -> Result<Baseline, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let unit = v
        .get("unit")
        .and_then(Value::as_str)
        .ok_or("missing `unit`")?
        .to_string();
    if unit.is_empty()
        || !unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
    {
        return Err("invalid unit id".into());
    }
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .ok_or("missing status")?;
    if !matches!(status, "green" | "partial" | "fail") {
        return Err("unknown report status".into());
    }
    let count = |object: &Value, key: &str, required: bool| -> Result<u64, String> {
        match object.get(key) {
            Some(n) => n
                .as_u64()
                .ok_or_else(|| format!("invalid nonnegative integer `{key}`")),
            None if required => Err(format!("missing `{key}`")),
            None => Ok(0),
        }
    };
    let seeds = count(&v, "seeds", true)?;
    let schema = v
        .get("schema_version")
        .map(|_| count(&v, "schema_version", true))
        .transpose()?;
    if schema.is_some_and(|n| n != 1) {
        return Err("unsupported report schema".into());
    }
    let raw = v
        .get("cases")
        .and_then(Value::as_array)
        .ok_or("missing cases array")?;
    if count(&v, "cases_total", true)? != raw.len() as u64 {
        return Err("cases_total disagrees with cases array".into());
    }
    let mut names = BTreeSet::new();
    let mut counts = BTreeMap::<&str, u64>::new();
    let mut cases = Vec::new();
    let mut vacuous_count = 0;
    let mut errored_count = 0;
    for c in raw {
        let name = c
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .ok_or("missing case name")?;
        if !names.insert(name) {
            return Err(format!("duplicate case `{name}`"));
        }
        let outcome = c
            .get("outcome")
            .and_then(Value::as_str)
            .ok_or("missing case outcome")?;
        if !matches!(
            outcome,
            "pass" | "fail" | "deferred" | "rejected" | "known_mismatch"
        ) {
            return Err(format!("unknown case outcome `{outcome}`"));
        }
        if c.get("pass").and_then(Value::as_bool) != Some(outcome == "pass") {
            return Err(format!("case `{name}` pass/outcome disagree"));
        }
        *counts.entry(outcome).or_default() += 1;
        let flag = |key: &str| -> Result<bool, String> {
            match c.get(key) {
                None => Ok(false),
                Some(x) => x
                    .as_bool()
                    .ok_or_else(|| format!("invalid boolean `{key}`")),
            }
        };
        let vacuous = flag("vacuous")?;
        let both_errored = flag("both_errored")?;
        flag("only_zeros")?;
        vacuous_count += u64::from(vacuous);
        errored_count += u64::from(both_errored);
        let floats = count(c, "values_compared", true)?;
        let exact = count(c, "exact_compared", schema == Some(1))?;
        let mut compared_values = floats > 0 || exact > 0;
        if (vacuous || both_errored) && compared_values {
            return Err(format!(
                "case `{name}` claims values and an empty/error result"
            ));
        }
        if outcome == "pass"
            && !compared_values
            && !vacuous
            && !both_errored
            && c.get("exact_compared").is_none()
        {
            // #550 tested BOTH float and exact counts when setting vacuous, but
            // omitted exact_compared. This proves presence, never an invented count.
            if v.get("cases_vacuous").is_none() {
                return Err("pre-550 integer coverage is ambiguous".into());
            }
            compared_values = true;
        }
        if schema == Some(1) && outcome == "pass" && !compared_values && !vacuous && !both_errored {
            return Err(format!("case `{name}` has an unmarked zero-value success"));
        }
        if outcome == "known_mismatch" && !compared_values && c.get("exact_compared").is_none() {
            if vacuous || both_errored || !legacy_known_integer_presence(c) {
                return Err("legacy known-mismatch integer coverage is ambiguous".into());
            }
            compared_values = true;
        }
        cases.push(BaselineCase {
            name: name.to_string(),
            outcome: outcome.to_string(),
            generated: c
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with("generated")),
            compared_values,
        });
    }
    for (outcome, field, required) in [
        ("pass", "cases_passed", true),
        ("fail", "cases_failed", true),
        ("deferred", "cases_deferred", true),
        ("rejected", "cases_rejected", false),
        ("known_mismatch", "cases_known_mismatch", false),
    ] {
        if count(&v, field, required)? != counts.get(outcome).copied().unwrap_or(0) {
            return Err(format!("{field} disagrees with case outcomes"));
        }
    }
    for (field, actual) in [
        ("cases_vacuous", vacuous_count),
        ("cases_both_errored", errored_count),
    ] {
        if v.get(field).is_some() && count(&v, field, true)? != actual {
            return Err(format!("{field} disagrees with cases"));
        }
    }
    let n = |outcome| counts.get(outcome).copied().unwrap_or(0);
    let expected_status = if v.get("harness_error").is_some_and(|e| !e.is_null())
        || raw.is_empty()
        || n("fail") > 0
        || n("rejected") == raw.len() as u64
    {
        "fail"
    } else if n("deferred") > 0 || n("known_mismatch") > 0 {
        "partial"
    } else {
        "green"
    };
    if status != expected_status
        || v.get("pass").and_then(Value::as_bool) != Some(expected_status != "fail")
    {
        return Err("report status/pass disagree with cases or harness error".into());
    }
    Ok(Baseline {
        unit,
        status: Some(status.to_string()),
        seeds: Some(seeds),
        cases,
    })
}

// exact_list increments its counter before serializing an indexed integer
// mismatch. Keys/length mismatches cannot establish integer value coverage.
fn legacy_known_integer_presence(case: &Value) -> bool {
    case.get("mismatches")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|mismatch| {
                let field = mismatch.get("field").and_then(Value::as_str).unwrap_or("");
                let indexed = field
                    .strip_prefix("ints.")
                    .and_then(|name| name.rsplit_once('['))
                    .is_some_and(|(name, index)| {
                        !name.is_empty()
                            && index.strip_suffix(']').is_some_and(|digits| {
                                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
                            })
                    });
                indexed
                    && ["cpp", "rust"].iter().all(|key| {
                        mismatch
                            .get(*key)
                            .and_then(Value::as_str)
                            .is_some_and(|value| {
                                let digits = value.strip_prefix('-').unwrap_or(value);
                                !digits.is_empty()
                                    && digits.bytes().all(|b| b.is_ascii_digit())
                                    && value.parse::<i64>().is_ok()
                            })
                    })
            })
        })
}

/// Reads `<dir>/<unit>.json`; `None` if there is no such report (a new unit).
pub fn load_baseline(dir: &Path, unit: &str) -> Result<Option<Baseline>, HarnessError> {
    let path = dir.join(format!("{unit}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
    parse_baseline(&text)
        .and_then(|b| {
            if b.unit == unit {
                Ok(b)
            } else {
                Err("filename/unit mismatch".into())
            }
        })
        .map(Some)
        .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))
}

/// Units whose baseline report in `dir` is `green` or `partial`, sorted.
pub fn baseline_units(dir: &Path) -> Result<Vec<String>, HarnessError> {
    let entries =
        fs::read_dir(dir).map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for e in entries {
        let path = e
            .map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?
            .path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        let b = parse_baseline(&text)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        if path.file_stem().and_then(|x| x.to_str()) != Some(b.unit.as_str()) {
            return Err(HarnessError::new("baseline filename/unit mismatch"));
        }
        if matches!(b.status.as_deref(), Some("green" | "partial")) {
            out.push(b.unit);
        }
    }
    if out.is_empty() {
        return Err(HarnessError::new(
            "baseline has no green/partial unit reports",
        ));
    }
    out.sort();
    Ok(out)
}

/// Validate only explicitly selected unit reports before their work starts.
///
/// The `--green`/`--all` parent validates the complete directory separately.
/// Repeating that full parse in every per-unit child would make setup quadratic.
/// A new unit may have no baseline report; the directory must still contain at
/// least one valid report so an empty or missing baseline cannot disable gates.
pub fn validate_selected_baselines(dir: &Path, units: &[String]) -> Result<(), HarnessError> {
    let entries =
        fs::read_dir(dir).map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?;
    let mut first = None;
    for entry in entries {
        let path = entry
            .map_err(|e| HarnessError::new(format!("{}: {e}", dir.display())))?
            .path();
        if path.extension().and_then(|x| x.to_str()) == Some("json")
            && first.as_ref().is_none_or(|earlier| &path < earlier)
        {
            first = Some(path);
        }
    }
    let first = first.ok_or_else(|| HarnessError::new("baseline has no unit reports"))?;
    let mut found = false;
    for unit in units {
        found |= load_baseline(dir, unit)?.is_some();
    }
    if !found {
        let unit = first
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| HarnessError::new("invalid baseline filename"))?;
        if load_baseline(dir, unit)?.is_none() {
            return Err(HarnessError::new(
                "baseline report disappeared during validation",
            ));
        }
    }
    Ok(())
}

/// Ratchet violations of `res` against `base` (empty: no regression).
///
/// A formerly passing case that now fails already fails the unit. Known mismatch
/// outcomes and previously exercised numerical coverage are also ratcheted.
pub fn ratchet(base: &Baseline, res: &UnitResult) -> Vec<Violation> {
    let now: BTreeMap<&str, &crate::runner::CaseResult> =
        res.cases.iter().map(|c| (c.name.as_str(), c)).collect();
    let same_seeds = base.seeds == u64::try_from(res.seeds).ok();
    let mut out = Vec::new();
    let mut push = |case: &str, detail: String| {
        out.push(Violation {
            unit: res.unit.clone(),
            check: Check::Ratchet,
            case: Some(case.to_string()),
            detail,
        });
    };
    if !same_seeds {
        push(
            "<seeds>",
            "baseline/current seeds differ; generated coverage cannot be ratcheted".into(),
        );
    }
    for b in &base.cases {
        if b.outcome == "rejected" {
            continue;
        }
        let Some(c) = now.get(b.name.as_str()) else {
            push(
                &b.name,
                format!("case `{}` ({} in the baseline) is gone", b.name, b.outcome),
            );
            continue;
        };
        let o = c.outcome();
        if b.outcome == "known_mismatch"
            && !matches!(o, CaseOutcome::KnownMismatch | CaseOutcome::Pass)
        {
            push(
                &b.name,
                format!("case `{}` was a known mismatch, now {}", b.name, o.as_str()),
            );
            continue;
        }
        if b.outcome == "pass" && o != CaseOutcome::Pass {
            if o == CaseOutcome::Fail {
                continue;
            }
            push(
                &b.name,
                format!(
                    "case `{}` passed in the baseline, now {}",
                    b.name,
                    o.as_str()
                ),
            );
            continue;
        }
        if b.compared_values && !c.compared_values() {
            push(
                &b.name,
                format!(
                    "case `{}` compared values in the baseline, now compares nothing{}",
                    b.name,
                    if c.both_errored {
                        " (both sides return the same error)"
                    } else {
                        ""
                    }
                ),
            );
        }
    }
    out
}

/// Value-share violation of `res`, if any. Only meaningful for a unit that does not
/// already fail.
pub fn value_share(res: &UnitResult) -> Option<Violation> {
    let passed = res.passed();
    let with_values = res.passed_with_values();
    // Exact integer arithmetic for `with_values / passed < MIN_VALUE_SHARE` (= 3/4).
    let too_few = with_values == 0 || (with_values as u128) * 4 < (passed as u128) * 3;
    too_few.then(|| Violation {
        unit: res.unit.clone(),
        check: Check::ValueShare,
        case: None,
        detail: format!(
            "{with_values} of {passed} passing cases compare a value (minimum {:.0}% and at \
             least one): make the generator reach the success path",
            MIN_VALUE_SHARE * 100.0
        ),
    })
}

/// One `[[waiver]]` entry of [`WAIVERS_PATH`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    /// Unit id.
    pub unit: String,
    /// Gate it waives.
    pub check: Check,
    /// Case it waives (ratchet only); `None` waives every case of the unit.
    pub case: Option<String>,
    /// Why the violation is acceptable.
    pub reason: String,
}

/// All waivers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waivers(pub Vec<Waiver>);

impl Waivers {
    /// Reads `<root>/plan/difftest-waivers.toml`; no file means no waivers.
    pub fn load(root: &Path) -> Result<Self, HarnessError> {
        let path = root.join(WAIVERS_PATH);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| HarnessError::new(format!("{}: {e}", path.display())))
    }

    /// Parses waiver-file text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        let entries = v
            .get("waiver")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for (i, w) in entries.iter().enumerate() {
            let s = |k: &str| w.get(k).and_then(toml::Value::as_str).map(str::to_string);
            let unit = s("unit").ok_or_else(|| format!("waiver #{i}: missing `unit`"))?;
            let check = s("check")
                .as_deref()
                .and_then(Check::parse)
                .ok_or_else(|| {
                    format!("waiver #{i} ({unit}): `check` must be `ratchet` or `value-share`")
                })?;
            let case = s("case");
            if case.is_some() && check != Check::Ratchet {
                return Err(format!(
                    "waiver #{i} ({unit}): `case` only applies to `ratchet`"
                ));
            }
            let reason = s("reason")
                .filter(|r| !r.trim().is_empty())
                .ok_or_else(|| format!("waiver #{i} ({unit}): missing `reason`"))?;
            out.push(Waiver {
                unit,
                check,
                case,
                reason,
            });
        }
        Ok(Self(out))
    }

    /// The waiver covering `v`, if any.
    pub fn covering(&self, v: &Violation) -> Option<&Waiver> {
        self.0.iter().find(|w| {
            w.unit == v.unit
                && w.check == v.check
                && (w.case.is_none() || w.case.as_deref() == v.case.as_deref())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::CaseSource;
    use crate::compare::CaseDiff;
    use crate::output::Deferred;
    use crate::runner::CaseResult;

    fn case(name: &str, outcome: &str, values: usize) -> CaseResult {
        let mut c = CaseResult {
            name: name.into(),
            source: CaseSource::Inline,
            inputs: Value::Null,
            diff: CaseDiff {
                values_compared: values,
                ..CaseDiff::default()
            },
            harness_error: None,
            both_errored: false,
            rejected: None,
            deferred: None,
            skipped_platform: None,
            perturbed: None,
            diagnostics: BTreeMap::new(),
            known_mismatch: None,
        };
        match outcome {
            "pass" => {}
            "deferred" => {
                c.deferred = Some(Deferred {
                    unit: "x.y".into(),
                    symbol: "F".into(),
                });
            }
            "rejected" => c.rejected = Some("bad".into()),
            "fail" => c.diff.mismatch_count = 1,
            "known_mismatch" => {
                c.diff.mismatch_count = 1;
                c.known_mismatch = Some("tracked mismatch".into());
            }
            other => panic!("{other}"),
        }
        c
    }

    fn unit(cases: Vec<CaseResult>) -> UnitResult {
        UnitResult {
            unit: "a.b".into(),
            cases,
            harness_error: None,
            abs_floors: Vec::new(),
            seeds: 16,
            timing: crate::runner::UnitTiming::default(),
        }
    }

    fn base(cases: &[(&str, &str, bool, bool)]) -> Baseline {
        Baseline {
            unit: "a.b".into(),
            status: Some("green".into()),
            seeds: Some(16),
            cases: cases
                .iter()
                .map(|&(n, o, generated, v)| BaselineCase {
                    name: n.into(),
                    outcome: o.into(),
                    generated,
                    compared_values: v,
                })
                .collect(),
        }
    }

    #[test]
    fn parses_550_boolean_bridge_and_exact_counts_without_guessing() {
        let old = serde_json::json!({"unit":"a.b","status":"green","pass":true,"seeds":16,
            "cases_total":3,"cases_passed":3,"cases_failed":0,"cases_deferred":0,
            "cases_vacuous":1,"cases_both_errored":1,
            "cases":[
              {"name":"integer","pass":true,"outcome":"pass","values_compared":0},
              {"name":"error","pass":true,"outcome":"pass","values_compared":0,"both_errored":true},
              {"name":"empty","pass":true,"outcome":"pass","values_compared":0,"vacuous":true}]});
        let b = parse_baseline(&old.to_string()).unwrap();
        assert_eq!(
            b.cases
                .iter()
                .map(|c| c.compared_values)
                .collect::<Vec<_>>(),
            [true, false, false]
        );
        let mut ambiguous = old.clone();
        ambiguous.as_object_mut().unwrap().remove("cases_vacuous");
        assert!(parse_baseline(&ambiguous.to_string()).is_err());
        let mut modern = old.clone();
        modern["schema_version"] = serde_json::json!(1);
        assert!(parse_baseline(&modern.to_string()).is_err());
        for c in modern["cases"].as_array_mut().unwrap() {
            c["exact_compared"] = serde_json::json!(0);
        }
        modern["cases"][0]["exact_compared"] = serde_json::json!(7);
        assert!(parse_baseline(&modern.to_string()).unwrap().cases[0].compared_values);
        assert_eq!(modern["cases"][0]["exact_compared"], 7);
        let mut duplicate = old.clone();
        duplicate["cases"][1]["name"] = serde_json::json!("integer");
        assert!(parse_baseline(&duplicate.to_string()).is_err());
        for field in ["cases", "cases_total", "status", "seeds"] {
            let mut missing = old.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(parse_baseline(&missing.to_string()).is_err(), "{field}");
        }
        for (field, value) in [
            ("status", serde_json::json!("fail")),
            ("pass", serde_json::json!(false)),
            ("harness_error", serde_json::json!("broken")),
        ] {
            let mut invalid = old.clone();
            invalid[field] = value;
            assert!(parse_baseline(&invalid.to_string()).is_err(), "{field}");
        }
        let mut corrupt = old;
        corrupt["cases_total"] = serde_json::json!(99);
        assert!(parse_baseline(&corrupt.to_string()).is_err());
    }

    #[test]
    fn passing_to_known_is_not_progress_and_known_cases_cannot_disappear() {
        let b = base(&[
            ("pass", "pass", false, true),
            ("known", "known_mismatch", false, true),
        ]);
        let violations = ratchet(&b, &unit(vec![case("pass", "known_mismatch", 1)]));
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn known_mismatches_keep_outcomes_and_numerical_coverage() {
        let b = base(&[("known", "known_mismatch", false, true)]);
        for outcome in ["known_mismatch", "pass"] {
            assert!(ratchet(&b, &unit(vec![case("known", outcome, 1)])).is_empty());
            let violations = ratchet(&b, &unit(vec![case("known", outcome, 0)]));
            assert_eq!(violations.len(), 1, "{outcome}");
            assert!(violations[0].detail.contains("now compares nothing"));
        }
        for outcome in ["deferred", "rejected", "fail"] {
            let violations = ratchet(&b, &unit(vec![case("known", outcome, 0)]));
            assert_eq!(violations.len(), 1, "{outcome}");
            assert!(violations[0].detail.contains("was a known mismatch"));
        }
    }

    #[test]
    fn legacy_indexed_integer_mismatch_proves_presence_without_a_count() {
        let mut old = serde_json::json!({"unit":"a.b","status":"partial","pass":true,"seeds":16,
            "cases_total":1,"cases_passed":0,"cases_failed":0,"cases_deferred":0,
            "cases_known_mismatch":1,"cases_vacuous":0,
            "cases":[{"name":"known","pass":false,"outcome":"known_mismatch","values_compared":0,
                "mismatches":[{"field":"ints.status[0]","cpp":"1006","rust":"1001"}]}]});
        assert!(parse_baseline(&old.to_string()).unwrap().cases[0].compared_values);
        assert!(old["cases"][0].get("exact_compared").is_none());
        for bad in [
            serde_json::json!([]),
            serde_json::json!([{"field":"ints.status.len","cpp":"1","rust":"2"}]),
            serde_json::json!([{"field":"ints.status[0]","cpp":"missing","rust":"1"}]),
        ] {
            old["cases"][0]["mismatches"] = bad;
            assert!(parse_baseline(&old.to_string()).is_err());
        }
    }

    #[test]
    fn unchanged_or_improved_run_has_no_violation() {
        let b = base(&[("p", "pass", false, true), ("d", "deferred", false, false)]);
        let r = unit(vec![
            case("p", "pass", 1),
            case("d", "pass", 4),
            case("new", "pass", 0),
        ]);
        assert_eq!(ratchet(&b, &r), []);
    }

    #[test]
    fn lost_downgraded_and_emptied_cases_are_caught() {
        let b = base(&[
            ("gone", "pass", false, true),
            ("gone_deferred", "deferred", false, false),
            ("now_deferred", "pass", false, true),
            ("now_rejected", "pass", false, false),
            ("now_empty", "pass", false, true),
            ("now_fails", "pass", false, true),
            ("was_rejected", "rejected", false, false),
        ]);
        let r = unit(vec![
            case("now_deferred", "deferred", 0),
            case("now_rejected", "rejected", 0),
            case("now_empty", "pass", 0),
            case("now_fails", "fail", 1),
        ]);
        let cases: Vec<_> = ratchet(&b, &r)
            .into_iter()
            .map(|v| v.case.unwrap())
            .collect();
        assert_eq!(
            cases,
            [
                "gone",
                "gone_deferred",
                "now_deferred",
                "now_rejected",
                "now_empty"
            ]
        );
    }

    #[test]
    fn changed_seeds_cannot_hide_generated_case_loss() {
        let b = base(&[
            ("gen0000", "pass", true, true),
            ("fix", "pass", false, true),
        ]);
        let mut r = unit(vec![case("fix", "pass", 1)]);
        assert_eq!(ratchet(&b, &r).len(), 1);
        r.seeds = 4;
        assert_eq!(ratchet(&b, &r).len(), 2);
    }

    #[test]
    fn value_share_needs_three_quarters_and_at_least_one() {
        let share = |with: usize, without: usize| {
            let mut v: Vec<_> = (0..with)
                .map(|i| case(&format!("v{i}"), "pass", 1))
                .collect();
            v.extend((0..without).map(|i| case(&format!("e{i}"), "pass", 0)));
            value_share(&unit(v))
        };
        assert!(share(3, 1).is_none());
        assert!(share(2, 1).is_some());
        assert!(share(1, 0).is_none());
        // Deferred cases do not count either way, but some pass must compare a value.
        assert!(value_share(&unit(vec![case("a", "deferred", 0)])).is_some());
        assert!(value_share(&unit(vec![case("a", "pass", 1), case("b", "deferred", 0)])).is_none());
    }

    #[test]
    fn waivers_match_unit_check_and_case() {
        let w = Waivers::parse(
            r#"
            [[waiver]]
            unit = "a.b"
            check = "ratchet"
            case = "gone"
            reason = "fixture was wrong (#12)"

            [[waiver]]
            unit = "c.d"
            check = "value-share"
            reason = "traversal outputs are error codes by design"
            "#,
        )
        .unwrap();
        let v = |unit: &str, check, case: Option<&str>| Violation {
            unit: unit.into(),
            check,
            case: case.map(str::to_string),
            detail: String::new(),
        };
        assert!(w
            .covering(&v("a.b", Check::Ratchet, Some("gone")))
            .is_some());
        assert!(w
            .covering(&v("a.b", Check::Ratchet, Some("other")))
            .is_none());
        assert!(w.covering(&v("a.b", Check::ValueShare, None)).is_none());
        assert!(w.covering(&v("c.d", Check::ValueShare, None)).is_some());
    }

    #[test]
    fn waivers_need_a_reason_and_a_known_check() {
        assert!(Waivers::parse("[[waiver]]\nunit='a'\ncheck='ratchet'\n").is_err());
        assert!(Waivers::parse("[[waiver]]\nunit='a'\ncheck='x'\nreason='r'\n").is_err());
        assert!(Waivers::parse(
            "[[waiver]]\nunit='a'\ncheck='value-share'\ncase='c'\nreason='r'\n"
        )
        .is_err());
        assert_eq!(Waivers::parse("").unwrap(), Waivers::default());
    }
}
