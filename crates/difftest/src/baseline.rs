//! Regression gates over a unit run: the baseline ratchet and the value-share floor.
//!
//! * **Ratchet** (`--baseline <dir>`): compares a run with the unit's earlier report,
//!   normally `reports/difftest/<unit>.json` from the base branch. Every case that
//!   passed there must still exist and pass, and must still compare values if it did;
//!   no deferred or failing case may disappear either. A deferred case may turn into a
//!   pass (progress); a rejected fixture may go away. Generated cases are only matched
//!   when both runs used the same `--seeds`.
//! * **Value share**: at least [`MIN_VALUE_SHARE`] of a unit's passing cases, and at
//!   least one, must compare a value (a float, integer or topology entry) rather than
//!   only an error string both sides agreed on.
//!
//! A violation is waived only by an entry in [`WAIVERS_PATH`] that names the unit,
//! the check and (for the ratchet, optionally) the case, with a reason.

use std::collections::BTreeMap;
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
    /// `pass`, `fail`, `deferred` or `rejected`.
    pub outcome: String,
    /// The case came from the generator (its inputs depend on `--seeds`).
    pub generated: bool,
    /// The case compared at least one value. Reports written before `exact_compared`
    /// was recorded only know about floats, so integer-only cases read as `false`.
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
    let mut cases = Vec::new();
    for (i, c) in v
        .get("cases")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let name = c
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("case #{i}: missing `name`"))?;
        // Reports from before `outcome` existed only had `pass` and `deferred`.
        let outcome = match c.get("outcome").and_then(Value::as_str) {
            Some(o) => o.to_string(),
            None if c.get("deferred").is_some() => "deferred".into(),
            None if c.get("pass").and_then(Value::as_bool) == Some(true) => "pass".into(),
            None => "fail".into(),
        };
        let count = |k: &str| c.get(k).and_then(Value::as_u64).unwrap_or(0);
        cases.push(BaselineCase {
            name: name.to_string(),
            outcome,
            generated: c
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with("generated")),
            compared_values: count("values_compared") + count("exact_compared") > 0,
        });
    }
    Ok(Baseline {
        unit,
        status: v.get("status").and_then(Value::as_str).map(str::to_string),
        seeds: v.get("seeds").and_then(Value::as_u64),
        cases,
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
        if matches!(b.status.as_deref(), Some("green" | "partial")) {
            out.push(b.unit);
        }
    }
    out.sort();
    Ok(out)
}

/// Ratchet violations of `res` against `base` (empty: no regression).
///
/// A case that now fails is not reported here: the failure already fails the unit.
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
    for b in &base.cases {
        if (b.generated && !same_seeds) || b.outcome == "rejected" {
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
        if b.outcome != "pass" || o == CaseOutcome::Fail {
            continue;
        }
        if o != CaseOutcome::Pass {
            push(
                &b.name,
                format!(
                    "case `{}` passed in the baseline, now {}",
                    b.name,
                    o.as_str()
                ),
            );
        } else if b.compared_values && !c.compared_values() {
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
    let too_few = with_values == 0 || with_values * 4 < passed * 3;
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
            perturbed: None,
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
    fn parses_current_and_old_reports() {
        let text = r#"{"unit":"a.b","status":"partial","seeds":16,"cases":[
            {"name":"f","source":"fixture:x.json#0","pass":true,"outcome":"pass","values_compared":3},
            {"name":"g","source":"generated:seed=1","pass":true,"outcome":"pass","values_compared":0,"exact_compared":2},
            {"name":"d","source":"inline","pass":false,"deferred":{"unit":"x","symbol":"F"},"values_compared":0},
            {"name":"o","source":"inline","pass":true,"values_compared":0}]}"#;
        let b = parse_baseline(text).unwrap();
        assert_eq!(b.seeds, Some(16));
        assert_eq!(b.status.as_deref(), Some("partial"));
        let got: Vec<_> = b
            .cases
            .iter()
            .map(|c| {
                (
                    c.name.as_str(),
                    c.outcome.as_str(),
                    c.generated,
                    c.compared_values,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("f", "pass", false, true),
                ("g", "pass", true, true),
                ("d", "deferred", false, false),
                ("o", "pass", false, false),
            ]
        );
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
    fn generated_cases_are_skipped_when_seeds_differ() {
        let b = base(&[
            ("gen0000", "pass", true, true),
            ("fix", "pass", false, true),
        ]);
        let mut r = unit(vec![case("fix", "pass", 1)]);
        assert_eq!(ratchet(&b, &r).len(), 1);
        r.seeds = 4;
        assert_eq!(ratchet(&b, &r), []);
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
