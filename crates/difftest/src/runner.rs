//! The [`UnitDiff`] trait and the per-unit driver.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::case::{Case, CaseContext, CaseSource};
use crate::compare::{compare, AbsFloor, CaseDiff};
use crate::output::{Deferred, Output};
use crate::rng::Rng;
use crate::HarnessError;

/// A side-by-side runner for one unit. Implement it on `pub struct Runner;`
/// in `src/units/<unit_id_with_underscores>.rs`.
pub trait UnitDiff {
    /// Unit id as in `plan/units/<id>.toml` (e.g. `nlib.knots`).
    fn id(&self) -> &'static str;

    /// Runs the upstream C++ (via `solidcore_ffi::units::*`).
    /// Return `Ok(Output::err(..))` when the *operation* fails and `Err` only
    /// for harness problems (bad inputs); `Err` always fails the case. Native fixture
    /// construction failures may use [`Output::rejected`], never operation errors.
    /// [`Output::deferred`] marks a case the C++ side could not run (e.g. a shim's crash
    /// trap caught an upstream crash): it is deferred, not compared.
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError>;

    /// Runs the solidcore Rust port on the same inputs. Return
    /// [`Output::deferred`] (e.g. via [`Output::unported`]) when the port hits a
    /// `SolidError::Unported` callee: the case is then deferred, not compared.
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError>;

    /// One generated case's inputs from a seeded RNG; `None` = no generator.
    fn generate(&self, _rng: &mut Rng) -> Option<Value> {
        None
    }

    /// Documented absolute floors for specific value keys (default: none).
    fn abs_floors(&self) -> &'static [AbsFloor] {
        &[]
    }

    /// All cases: fixtures from `fixtures/<id>/*.json`, then `ctx.seeds`
    /// generated cases. Override to add cases built in code (append to the
    /// default list via [`default_cases`]).
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        default_cases(self, ctx)
    }
}

/// Fixture cases followed by generated cases (the default [`UnitDiff::cases`]).
pub fn default_cases<U: UnitDiff + ?Sized>(
    unit: &U,
    ctx: &CaseContext,
) -> Result<Vec<Case>, HarnessError> {
    let mut v = ctx.fixture_cases(unit.id())?;
    v.extend(ctx.generated_cases(unit.id(), |rng| unit.generate(rng)));
    Ok(v)
}

/// Options for [`run_unit`].
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Only run cases whose name contains this substring.
    pub filter: Option<String>,
    /// Sensitivity check: change one item of each case's Rust output before
    /// comparing (see [`perturb_output`]); every perturbed case must fail.
    pub perturb: bool,
}

/// Relative perturbation used by [`RunOptions::perturb`] (10x the tolerance).
pub const PERTURBATION: f64 = 1e-8;

/// Result of one case.
#[derive(Debug, Clone)]
pub struct CaseResult {
    /// Case name.
    pub name: String,
    /// Provenance.
    pub source: CaseSource,
    /// Case inputs (written to the report on failure or fixture rejection).
    pub inputs: Value,
    /// Comparison result.
    pub diff: CaseDiff,
    /// Harness error on either side (fails the case).
    pub harness_error: Option<String>,
    /// Both sides reported the same operation error.
    pub both_errored: bool,
    /// Native fixture construction was rejected before running the operation.
    pub rejected: Option<String>,
    /// The Rust side hit an unported callee, or the C++ side could not run the case
    /// (case deferred, nothing compared).
    pub deferred: Option<Deferred>,
    /// In perturb mode: what was perturbed (`value`, `int`, `topology`, `error`),
    /// or `None` if the case had nothing to perturb or was deferred.
    pub perturbed: Option<&'static str>,
    /// The Rust side's [`Output::diagnostics`] (reported, never compared).
    pub diagnostics: BTreeMap<String, i64>,
    /// The case is listed in `known_mismatches.toml` (the entry's `why`).
    pub known_mismatch: Option<String>,
}

/// Classification of one case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseOutcome {
    /// Both sides ran and agree.
    Pass,
    /// The native fixture could not be constructed; no parity comparison ran.
    Rejected,
    /// Mismatch or harness error.
    Fail,
    /// The Rust side returned `Unported` (and the C++ side ran without a harness
    /// error): neither pass nor fail.
    Deferred,
    /// A mismatch listed in `known_mismatches.toml` ([`crate::known`]): not a pass,
    /// keeps the unit from being green, reported like a failure.
    KnownMismatch,
}

impl CaseOutcome {
    /// `pass`, `fail`, `deferred`, `rejected` or `known_mismatch`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Rejected => "rejected",
            Self::Fail => "fail",
            Self::Deferred => "deferred",
            Self::KnownMismatch => "known_mismatch",
        }
    }
}

impl CaseResult {
    /// Classification, excluding native fixture rejections from parity results.
    /// A harness error always fails.
    pub fn outcome(&self) -> CaseOutcome {
        if self.harness_error.is_some() {
            CaseOutcome::Fail
        } else if self.rejected.is_some() {
            CaseOutcome::Rejected
        } else if self.deferred.is_some() {
            CaseOutcome::Deferred
        } else if self.diff.pass() {
            CaseOutcome::Pass
        } else if self.known_mismatch.is_some() {
            CaseOutcome::KnownMismatch
        } else {
            CaseOutcome::Fail
        }
    }

    /// Case is a listed known mismatch that still mismatches.
    pub fn is_known_mismatch(&self) -> bool {
        self.outcome() == CaseOutcome::KnownMismatch
    }

    /// Case passes (not deferred, no harness error, no mismatch).
    pub fn pass(&self) -> bool {
        self.outcome() == CaseOutcome::Pass
    }

    /// Case fails (mismatch or harness error).
    pub fn fail(&self) -> bool {
        self.outcome() == CaseOutcome::Fail
    }

    /// Case is deferred.
    pub fn is_deferred(&self) -> bool {
        self.outcome() == CaseOutcome::Deferred
    }

    /// The comparison checked at least one float, integer or topology entry (not only
    /// an error string both sides agreed on, or empty outputs).
    pub fn compared_values(&self) -> bool {
        self.diff.values_compared + self.diff.exact_compared > 0
    }

    /// A *vacuous* pass: both sides reported success (no `error`) and the comparison
    /// checked nothing but that status. Such a pass is only as good as the runner's
    /// status reporting: a Rust `Err` swallowed into an empty output would look the
    /// same, so runners must map every Rust error into [`Output::error`]
    /// (see [`Output::from_result`]) and these cases are counted separately.
    pub fn is_vacuous(&self) -> bool {
        self.pass() && !self.compared_values() && !self.both_errored
    }
}

/// Status of a unit run (AGENTS.md section 4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitStatus {
    /// At least one parity pass, no failures or deferrals (fixture rejections excluded).
    Green,
    /// No failing case and at least one deferred case or known mismatch.
    Partial,
    /// Any failing case, a harness error, no cases, or only rejected fixtures.
    Fail,
}

impl UnitStatus {
    /// `green`, `partial` or `fail`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Partial => "partial",
            Self::Fail => "fail",
        }
    }
}

/// Result of one unit.
#[derive(Debug, Clone)]
pub struct UnitResult {
    /// Unit id.
    pub unit: String,
    /// Per-case results, in case order.
    pub cases: Vec<CaseResult>,
    /// Failure to build the case list.
    pub harness_error: Option<String>,
    /// Documented absolute floors used.
    pub abs_floors: Vec<AbsFloor>,
    /// `--seeds` value used.
    pub seeds: usize,
    /// Wall time spent per phase (not written to the report).
    pub timing: UnitTiming,
}

/// Wall time of one unit's run, split by phase (diagnostic only; printed to stderr).
#[derive(Debug, Clone, Default)]
pub struct UnitTiming {
    /// Building the case list (`UnitDiff::cases`).
    pub cases: Duration,
    /// All `run_cpp` calls.
    pub cpp: Duration,
    /// All `run_rust` calls.
    pub rust: Duration,
    /// All comparisons.
    pub compare: Duration,
    /// Slowest single `run_rust` call: case name and time.
    pub slowest_rust: Option<(String, Duration)>,
    /// With `DIFFTEST_RUST_DIGEST` set: one `<case>\t<fnv1a64 of the Rust output's Debug form>`
    /// line per case (f64 `Debug` is the shortest round-trip form, so equal digests mean
    /// bit-identical Rust outputs across builds).
    pub rust_digests: Vec<String>,
    /// With `DIFFTEST_RUST_DIGEST` set: the same line for the C++ output of every case
    /// (proves that harness changes leave the C++ side's results untouched).
    pub cpp_digests: Vec<String>,
}

fn fnv1a64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

impl UnitTiming {
    /// One `TIME` line: `TIME <unit> n=<cases> cases=<s> cpp=<s> rust=<s> compare=<s> ratio=<rust/cpp> slowest_rust=<case>:<s>`.
    pub fn line(&self, unit: &str, n_cases: usize) -> String {
        let ratio = if self.cpp.as_secs_f64() > 0.0 {
            self.rust.as_secs_f64() / self.cpp.as_secs_f64()
        } else {
            f64::NAN
        };
        let slow = self
            .slowest_rust
            .as_ref()
            .map_or_else(String::new, |(n, d)| {
                format!(" slowest_rust={n}:{:.6}", d.as_secs_f64())
            });
        format!(
            "TIME {unit} n={n_cases} cases={:.6} cpp={:.6} rust={:.6} compare={:.6} ratio={ratio:.3}{slow}",
            self.cases.as_secs_f64(),
            self.cpp.as_secs_f64(),
            self.rust.as_secs_f64(),
            self.compare.as_secs_f64(),
        )
    }
}

impl UnitResult {
    /// Green, partial or fail (see [`UnitStatus`]).
    pub fn status(&self) -> UnitStatus {
        if self.harness_error.is_some()
            || self.cases.is_empty()
            || self.failed() > 0
            || self.cases.len() == self.rejected()
        {
            UnitStatus::Fail
        } else if self.deferred() > 0 || self.known_mismatches() > 0 {
            UnitStatus::Partial
        } else {
            UnitStatus::Green
        }
    }

    /// No failure: the unit is green or partial (the exit status is 0).
    pub fn pass(&self) -> bool {
        self.status() != UnitStatus::Fail
    }

    /// Number of passing cases.
    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|c| c.pass()).count()
    }

    /// Number of passing cases that compared at least one value
    /// ([`CaseResult::compared_values`]); the other passing cases compared nothing.
    pub fn passed_with_values(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| c.pass() && c.compared_values())
            .count()
    }

    /// Number of [vacuous](CaseResult::is_vacuous) passes: both sides succeeded and only
    /// that status was compared (a subset of the passes without values).
    pub fn vacuous(&self) -> usize {
        self.cases.iter().filter(|c| c.is_vacuous()).count()
    }

    /// Number of failing cases.
    pub fn failed(&self) -> usize {
        self.cases.iter().filter(|c| c.fail()).count()
    }

    /// Number of cases rejected by the native fixture constructor.
    pub fn rejected(&self) -> usize {
        self.cases
            .iter()
            .filter(|c| c.outcome() == CaseOutcome::Rejected)
            .count()
    }

    /// Number of listed known mismatches that still mismatch.
    pub fn known_mismatches(&self) -> usize {
        self.cases.iter().filter(|c| c.is_known_mismatch()).count()
    }

    /// Number of deferred cases.
    pub fn deferred(&self) -> usize {
        self.cases.iter().filter(|c| c.is_deferred()).count()
    }

    /// Deferred case count per blocking unit id, sorted by unit id.
    pub fn deferred_by_unit(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for c in &self.cases {
            if let (CaseOutcome::Deferred, Some(d)) = (c.outcome(), &c.deferred) {
                *m.entry(d.unit.clone()).or_insert(0) += 1;
            }
        }
        m
    }

    /// Max relative error over all cases.
    pub fn max_rel_err(&self) -> f64 {
        self.cases
            .iter()
            .map(|c| c.diff.max_rel_err)
            .fold(0.0, |a: f64, b| if b > a || b.is_nan() { b } else { a })
    }

    /// Perturb mode: (perturbed cases, of which detected).
    pub fn perturb_stats(&self) -> (usize, usize) {
        let p: Vec<&CaseResult> = self
            .cases
            .iter()
            .filter(|c| c.perturbed.is_some())
            .collect();
        (p.len(), p.iter().filter(|c| c.fail()).count())
    }
}

fn guarded(f: impl FnOnce() -> Result<Output, HarnessError>, side: &str) -> Result<Output, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(o)) => Ok(o),
        Ok(Err(e)) => Err(format!("{side}: {e}")),
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            Err(format!("{side}: panic: {msg}"))
        }
    }
}

/// Changes exactly one item of `out` so that a correct comparison must fail;
/// returns what was changed, or `None` if `out` has nothing comparable.
///
/// In order of preference: the error string if the operation failed (only it is
/// compared then); the first nonzero finite float (times `1 + PERTURBATION`); the
/// first integer (`+1`); the first topology id (`+1`; an empty adjacency entry
/// gains a neighbour); else the first float of an all-zero/non-finite array
/// (`0` -> `PERTURBATION`, NaN -> `0`, `±inf` -> `∓inf`).
pub fn perturb_output(out: &mut Output) -> Option<&'static str> {
    if let Some(e) = out.error.as_mut() {
        e.push_str("~perturbed");
        return Some("error");
    }
    for v in out.values.values_mut() {
        if let Some(x) = v.iter_mut().find(|x| x.is_finite() && **x != 0.0) {
            *x *= 1.0 + PERTURBATION;
            return Some("value");
        }
    }
    for v in out.ints.values_mut() {
        if let Some(x) = v.first_mut() {
            *x = x.wrapping_add(1);
            return Some("int");
        }
    }
    if let Some(t) = out.topology.as_mut() {
        for v in t.entities.values_mut() {
            if let Some(x) = v.first_mut() {
                *x = x.wrapping_add(1);
                return Some("topology");
            }
        }
        for adj in t.adjacency.values_mut() {
            if let Some(n) = adj.first_mut() {
                match n.first_mut() {
                    Some(x) => *x = x.wrapping_add(1),
                    None => n.push(0),
                }
                return Some("topology");
            }
        }
    }
    for v in out.values.values_mut() {
        if let Some(x) = v.first_mut() {
            *x = if *x == 0.0 {
                PERTURBATION
            } else if x.is_nan() {
                0.0
            } else {
                -*x
            };
            return Some("value");
        }
    }
    None
}

/// Runs every case of `unit` through both sides and compares.
/// Cases run sequentially (the C++ kernel uses global state).
pub fn run_unit(unit: &dyn UnitDiff, ctx: &CaseContext, opts: &RunOptions) -> UnitResult {
    let mut res = UnitResult {
        unit: unit.id().to_string(),
        cases: Vec::new(),
        harness_error: None,
        abs_floors: unit.abs_floors().to_vec(),
        seeds: ctx.seeds,
        timing: UnitTiming::default(),
    };
    let digest = std::env::var_os("DIFFTEST_RUST_DIGEST").is_some();
    let t0 = Instant::now();
    let cases = unit.cases(ctx);
    res.timing.cases = t0.elapsed();
    let cases = match cases {
        Ok(c) => c,
        Err(e) => {
            res.harness_error = Some(e.0);
            return res;
        }
    };
    let known = match crate::known::for_unit(&ctx.root, unit.id()) {
        Ok(k) => k,
        Err(e) => {
            res.harness_error = Some(e.0);
            return res;
        }
    };
    if opts.filter.is_none() {
        let missing: Vec<&str> = known
            .iter()
            .filter(|k| !cases.iter().any(|c| c.name == k.case))
            .map(|k| k.case.as_str())
            .collect();
        if !missing.is_empty() {
            res.harness_error = Some(format!(
                "{}: no such case(s): {}",
                crate::known::KNOWN_MISMATCHES,
                missing.join(", ")
            ));
            return res;
        }
    }
    for case in cases {
        if let Some(f) = &opts.filter {
            if !case.name.contains(f.as_str()) {
                continue;
            }
        }
        // the Rust side reuses this case's C++ fixture exports (`export_memo`)
        let memo = crate::export_memo::CaseScope::new();
        let t0 = Instant::now();
        let cpp = guarded(|| unit.run_cpp(&case), "cpp");
        res.timing.cpp += t0.elapsed();
        if digest {
            let d = fnv1a64(&format!("{cpp:?}"));
            res.timing
                .cpp_digests
                .push(format!("{}\t{d:016x}", case.name));
        }

        let mut cr = CaseResult {
            name: case.name.clone(),
            source: case.source.clone(),
            inputs: case.inputs.clone(),
            diff: CaseDiff::default(),
            harness_error: None,
            both_errored: false,
            rejected: None,
            deferred: None,
            perturbed: None,
            diagnostics: BTreeMap::new(),
            known_mismatch: known
                .iter()
                .find(|k| k.case == case.name)
                .map(|k| k.why.clone()),
        };
        if let Ok(c) = &cpp {
            if let Some(reason) = &c.rejected {
                cr.rejected = Some(reason.clone());
                res.cases.push(cr);
                continue;
            }
        }
        crate::export_memo::rust_side();
        let t0 = Instant::now();
        let rust = guarded(|| unit.run_rust(&case), "rust");
        let dt = t0.elapsed();
        drop(memo);
        res.timing.rust += dt;
        if digest {
            let d = fnv1a64(&format!("{rust:?}"));
            res.timing
                .rust_digests
                .push(format!("{}\t{d:016x}", case.name));
        }
        if res
            .timing
            .slowest_rust
            .as_ref()
            .is_none_or(|(_, d)| dt > *d)
        {
            res.timing.slowest_rust = Some((case.name.clone(), dt));
        }
        let t0 = Instant::now();
        match (cpp, rust) {
            (_, Ok(r)) if r.rejected.is_some() => {
                cr.harness_error =
                    Some("rust: fixture rejection must originate in the native constructor".into());
            }
            // (a case the C++ side cannot run stays uncomparable once the Rust side's
            // blocker is ported: its deferral wins)
            (Ok(c), Ok(_)) if c.deferred.is_some() => {
                cr.deferred = c.deferred;
            }
            (Ok(_), Ok(r)) if r.deferred.is_some() => {
                cr.deferred = r.deferred;
            }
            (Ok(c), Ok(mut r)) => {
                cr.diagnostics = r.diagnostics.as_deref().cloned().unwrap_or_default();
                if opts.perturb {
                    cr.perturbed = perturb_output(&mut r);
                }
                cr.both_errored = c.error.is_some() && r.error.is_some();
                cr.diff = compare(&c, &r, unit.abs_floors());
            }
            (c, r) => {
                let msgs: Vec<String> = [c.err(), r.err()].into_iter().flatten().collect();
                cr.harness_error = Some(msgs.join("; "));
            }
        }
        res.timing.compare += t0.elapsed();
        if cr.known_mismatch.is_some()
            && !opts.perturb
            && cr.harness_error.is_none()
            && cr.outcome() != CaseOutcome::KnownMismatch
        {
            cr.harness_error = Some(format!(
                "listed in {} but the case is now {}: remove the entry",
                crate::known::KNOWN_MISMATCHES,
                cr.outcome().as_str()
            ));
        }
        res.cases.push(cr);
    }
    res
}

/// Result of [`digest_unit`]: the Rust side alone, one digest line per case.
#[derive(Debug, Clone, Default)]
pub struct DigestResult {
    /// Unit id.
    pub unit: String,
    /// One `<case>\t<fnv1a64>` line per case (as [`UnitTiming::rust_digests`]), or
    /// `<case>\tneeds-cpp` when the Rust side called a C++ stub (wasm32 only).
    pub lines: Vec<String>,
    /// With `debug`: one `<case>\t<Debug of the Rust output>` line per case.
    pub debug: Vec<String>,
    /// Cases whose Rust side called a C++ stub (wasm32; never natively).
    pub needs_cpp: usize,
    /// Building the case list failed, or called a C++ stub.
    pub harness_error: Option<String>,
}

/// Runs only the Rust side of every case of `unit` and digests each output like
/// `DIFFTEST_RUST_DIGEST` does (`--digest-dir`). Nothing is compared: equal digests from
/// two builds (native and wasm32) mean bit-identical Rust outputs. A case whose Rust
/// side calls the C++ kernel (fixture exports) is `needs-cpp` on wasm32, where the shims
/// are stubs ([`solidcore_ffi::unlinked_calls`]).
pub fn digest_unit(
    unit: &dyn UnitDiff,
    ctx: &CaseContext,
    opts: &RunOptions,
    debug: bool,
) -> DigestResult {
    let mut res = DigestResult {
        unit: unit.id().to_string(),
        ..DigestResult::default()
    };
    let before = solidcore_ffi::unlinked_calls();
    let cases = match unit.cases(ctx) {
        Ok(c) => c,
        Err(e) => {
            res.harness_error = Some(e.0);
            return res;
        }
    };
    if solidcore_ffi::unlinked_calls() != before {
        res.harness_error = Some("building the cases calls the C++ kernel".into());
        return res;
    }
    for case in cases {
        if let Some(f) = &opts.filter {
            if !case.name.contains(f.as_str()) {
                continue;
            }
        }
        let before = solidcore_ffi::unlinked_calls();
        let rust = guarded(|| unit.run_rust(&case), "rust");
        if solidcore_ffi::unlinked_calls() == before {
            let s = format!("{rust:?}");
            res.lines
                .push(format!("{}\t{:016x}", case.name, fnv1a64(&s)));
            if debug {
                res.debug.push(format!("{}\t{s}", case.name));
            }
        } else {
            res.needs_cpp += 1;
            res.lines.push(format!("{}\tneeds-cpp", case.name));
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::compare;
    use crate::output::Topology;

    fn caught(base: &Output) -> Option<&'static str> {
        let mut r = base.clone();
        let kind = perturb_output(&mut r);
        assert!(!compare(base, &r, &[]).pass(), "perturbation not caught");
        kind
    }

    #[test]
    fn perturbs_every_kind_of_output() {
        assert_eq!(caught(&Output::err("nlib_6")), Some("error"));
        assert_eq!(
            caught(&Output::new().values("v", vec![0.0, 2.0]).int("n", 3)),
            Some("value")
        );
        assert_eq!(caught(&Output::new().int("span", 4)), Some("int"));
        assert_eq!(
            caught(&Output::new().topology(Topology::new().count("faces", 2))),
            Some("topology")
        );
        assert_eq!(
            caught(&Output::new().topology(Topology::new().adjacency("f", vec![vec![]]))),
            Some("topology")
        );
        assert_eq!(caught(&Output::new().scalar("z", 0.0)), Some("value"));
        assert_eq!(
            caught(&Output::new().values("d", vec![f64::INFINITY, f64::NEG_INFINITY])),
            Some("value")
        );
        assert_eq!(caught(&Output::new().scalar("n", f64::NAN)), Some("value"));
        let mut empty = Output::new().values("v", Vec::<f64>::new());
        assert_eq!(perturb_output(&mut empty), None);
    }

    #[test]
    fn outcomes_and_status() {
        let case = |deferred: Option<Deferred>, pass: bool| CaseResult {
            name: "c".into(),
            source: CaseSource::Inline,
            inputs: Value::Null,
            diff: if pass {
                CaseDiff::default()
            } else {
                compare(&Output::err("a"), &Output::err("b"), &[])
            },
            harness_error: None,
            both_errored: false,
            rejected: None,
            deferred,
            perturbed: None,
            diagnostics: BTreeMap::new(),
            known_mismatch: None,
        };
        let d = || {
            Some(Deferred {
                unit: "geom.curve".into(),
                symbol: "SmCurve::Evaluate".into(),
            })
        };
        let mut r = UnitResult {
            unit: "u".into(),
            cases: vec![case(None, true), case(d(), true)],
            harness_error: None,
            abs_floors: Vec::new(),
            seeds: 0,
            timing: UnitTiming::default(),
        };
        assert_eq!(r.cases[1].outcome(), CaseOutcome::Deferred);
        assert_eq!(r.status(), UnitStatus::Partial);
        assert!(r.pass());
        assert_eq!((r.passed(), r.failed(), r.deferred()), (1, 0, 1));
        assert_eq!(r.deferred_by_unit().get("geom.curve"), Some(&1));
        // Neither passing case compared a value (`CaseDiff::default()`).
        assert_eq!(r.passed_with_values(), 0);
        assert_eq!(r.vacuous(), 1);
        r.cases[0].both_errored = true;
        assert_eq!(
            r.vacuous(),
            0,
            "an agreed error is a compared status, not vacuous"
        );
        r.cases[0].both_errored = false;
        r.cases[0].diff.exact_compared = 1;
        assert_eq!(r.passed_with_values(), 1);
        assert_eq!(r.vacuous(), 0);
        r.cases.push(case(None, false));
        assert_eq!(r.status(), UnitStatus::Fail);
        r.cases.truncate(1);
        assert_eq!(r.status(), UnitStatus::Green);
        let mut h = case(d(), true);
        h.harness_error = Some("cpp: bad".into());
        assert_eq!(h.outcome(), CaseOutcome::Fail);
        // A listed mismatch keeps the unit partial, never green, and is not a pass.
        let mut k = case(None, false);
        k.known_mismatch = Some("why".into());
        assert_eq!(k.outcome(), CaseOutcome::KnownMismatch);
        r.cases.push(k);
        assert_eq!(r.status(), UnitStatus::Partial);
        assert_eq!((r.passed(), r.failed(), r.known_mismatches()), (1, 0, 1));
        let mut listed_pass = case(None, true);
        listed_pass.known_mismatch = Some("why".into());
        assert_eq!(listed_pass.outcome(), CaseOutcome::Pass);
    }

    struct RejectedFixture {
        only_rejected: bool,
    }

    impl UnitDiff for RejectedFixture {
        fn id(&self) -> &'static str {
            "rejected.fixture"
        }
        fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
            if case.get::<u8>("i")? == 0 {
                Ok(Output::rejected("constructor failed"))
            } else {
                Ok(Output::new().scalar("value", 2.))
            }
        }
        fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
            assert_ne!(
                case.get::<u8>("i")?,
                0,
                "a rejected fixture must not run Rust"
            );
            Ok(Output::new().scalar("value", 2.))
        }
        fn cases(&self, _: &CaseContext) -> Result<Vec<Case>, HarnessError> {
            Ok((0..if self.only_rejected { 1 } else { 2 })
                .map(|i| Case {
                    unit: self.id().into(),
                    name: format!("case{i}"),
                    source: CaseSource::Inline,
                    inputs: serde_json::json!({"i": i}),
                })
                .collect())
        }
    }

    #[test]
    fn rejected_fixtures_are_recorded_without_running_rust_or_claiming_parity() {
        let ctx = CaseContext {
            root: std::path::PathBuf::new(),
            seeds: 0,
        };
        let r = run_unit(
            &RejectedFixture {
                only_rejected: false,
            },
            &ctx,
            &RunOptions::default(),
        );
        assert_eq!(
            (r.passed(), r.failed(), r.deferred(), r.rejected()),
            (1, 0, 0, 1)
        );
        let report = crate::report::unit_report(&r);
        assert_eq!(report["cases"][0]["outcome"], "rejected");
        assert_eq!(report["cases"][0]["pass"], false);
        assert_eq!(report["cases"][0]["inputs"]["i"], 0);
        assert_eq!(report["cases_rejected"], 1);
        assert!(crate::report::summary_line(&r).contains("rejected=1"));
        let only = run_unit(
            &RejectedFixture {
                only_rejected: true,
            },
            &ctx,
            &RunOptions::default(),
        );
        assert_eq!(only.status(), UnitStatus::Fail);
        let sensitivity = run_unit(
            &RejectedFixture {
                only_rejected: false,
            },
            &ctx,
            &RunOptions {
                perturb: true,
                ..Default::default()
            },
        );
        assert_eq!(sensitivity.perturb_stats(), (1, 1));
        assert_eq!(sensitivity.rejected(), 1);
    }

    /// Fake unit: case `i` returns `Unported` on the Rust side when `i` is odd.
    struct Fake;

    impl UnitDiff for Fake {
        fn id(&self) -> &'static str {
            "fake.unit"
        }
        fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
            Ok(Output::new().int("i", case.get("i")?))
        }
        fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
            let i: i64 = case.get("i")?;
            if i % 2 == 1 {
                let e = solidcore::SolidError::Unported {
                    upstream: "SmCurve::Evaluate",
                    unit: "geom.curve",
                };
                return Ok(Output::unported(&e).unwrap_or_else(|| Output::err("x")));
            }
            Ok(Output::new().int("i", i))
        }
        fn cases(&self, _ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
            Ok((0..4)
                .map(|i| Case {
                    unit: "fake.unit".into(),
                    name: format!("c{i}"),
                    source: CaseSource::Inline,
                    inputs: serde_json::json!({ "i": i }),
                })
                .collect())
        }
    }

    /// C++ finds nothing (empty output); Rust fails on case `err` and its runner
    /// reports the error, as runners must.
    struct EmptyOrErr;

    impl UnitDiff for EmptyOrErr {
        fn id(&self) -> &'static str {
            "empty.unit"
        }
        fn run_cpp(&self, _case: &Case) -> Result<Output, HarnessError> {
            Ok(Output::new().values("points", Vec::<f64>::new()))
        }
        fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
            let r: solidcore::Result<Vec<f64>> = if case.name == "err" {
                Err(solidcore::SolidError::sm_err("empty.unit"))
            } else {
                Ok(Vec::new())
            };
            Ok(Output::from_result(
                r,
                |_| "status_1".into(),
                |v| Output::new().values("points", v),
            ))
        }
        fn cases(&self, _ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
            Ok(["ok", "err"]
                .into_iter()
                .map(|n| Case {
                    unit: "empty.unit".into(),
                    name: n.into(),
                    source: CaseSource::Inline,
                    inputs: Value::Null,
                })
                .collect())
        }
    }

    fn known_root(tag: &str, list: &str) -> CaseContext {
        let root =
            std::env::temp_dir().join(format!("difftest-known-{tag}-{}", std::process::id()));
        let dir = root.join("crates/difftest");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("known_mismatches.toml"), list).unwrap();
        CaseContext { root, seeds: 0 }
    }

    #[test]
    fn status_mismatch_with_empty_outputs_fails_and_known_mismatches_are_tracked() {
        let ctx = CaseContext {
            root: std::path::PathBuf::new(),
            seeds: 0,
        };
        let r = run_unit(&EmptyOrErr, &ctx, &RunOptions::default());
        // Rust erred where C++ found nothing: a failure, not a vacuous pass.
        assert_eq!((r.passed(), r.failed(), r.vacuous()), (1, 1, 1));
        assert_eq!(r.cases[1].diff.mismatches[0].field, "error");
        assert_eq!(r.status(), UnitStatus::Fail);
        let line = crate::report::summary_line(&r);
        assert!(
            line.contains(" with_values=0 no_values=1 vacuous=1"),
            "{line}"
        );

        let entry = |case: &str| {
            format!("[[mismatch]]\nunit = \"empty.unit\"\ncase = \"{case}\"\nwhy = \"w\"\n")
        };
        let k = run_unit(
            &EmptyOrErr,
            &known_root("listed", &entry("err")),
            &RunOptions::default(),
        );
        assert_eq!(k.cases[1].outcome(), CaseOutcome::KnownMismatch);
        assert_eq!((k.passed(), k.failed(), k.known_mismatches()), (1, 0, 1));
        assert_eq!(k.status(), UnitStatus::Partial);
        assert!(crate::report::summary_line(&k).ends_with(" known_mismatch=1"));
        assert_eq!(
            crate::report::unit_report(&k)["cases"][1]["known_mismatch"],
            "w"
        );

        // A listed case that passes, or one that does not exist, fails the unit.
        let stale = run_unit(
            &EmptyOrErr,
            &known_root("stale", &entry("ok")),
            &RunOptions::default(),
        );
        assert_eq!(stale.cases[0].outcome(), CaseOutcome::Fail);
        assert_eq!(stale.status(), UnitStatus::Fail);
        let gone = run_unit(
            &EmptyOrErr,
            &known_root("gone", &entry("nope")),
            &RunOptions::default(),
        );
        assert!(gone.harness_error.is_some());
        assert_eq!(gone.status(), UnitStatus::Fail);
    }

    #[test]
    fn unported_rust_side_is_deferred() {
        let ctx = CaseContext {
            root: std::path::PathBuf::new(),
            seeds: 0,
        };
        let r = run_unit(&Fake, &ctx, &RunOptions::default());
        assert_eq!((r.passed(), r.failed(), r.deferred()), (2, 0, 2));
        assert_eq!(r.status(), UnitStatus::Partial);
        let line = crate::report::summary_line(&r);
        assert!(line.starts_with("PARTIAL fake.unit cases=4 passed=2 failed=0 deferred=2"));
        assert!(line.contains(" with_values=2 no_values=0 "), "{line}");
        assert!(line.ends_with(" blocked_by=geom.curve(2)"), "{line}");
        let rep = crate::report::unit_report(&r);
        assert_eq!(rep["status"], "partial");
        assert_eq!(rep["deferred_by_unit"]["geom.curve"], 2);
        assert_eq!(rep["cases"][1]["deferred"]["symbol"], "SmCurve::Evaluate");
        let p = run_unit(
            &Fake,
            &ctx,
            &RunOptions {
                perturb: true,
                ..RunOptions::default()
            },
        );
        assert_eq!(p.perturb_stats(), (2, 2));
    }

    /// Fake unit whose C++ side cannot run cases 1 and 2 (as a shim's crash trap reports).
    struct CppDefers;

    impl UnitDiff for CppDefers {
        fn id(&self) -> &'static str {
            "fake.cpp_defers"
        }
        fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
            let i: i64 = case.get("i")?;
            if i == 1 || i == 2 {
                return Ok(Output::deferred("cpp-crash-guard:UB-0", "SmFake::Crash"));
            }
            Ok(Output::new().int("i", i))
        }
        fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
            // (case 1 disagrees: a C++-side deferral must not become a failure; case 2 is
            // deferred on both sides: the C++ side's label wins)
            let i: i64 = case.get("i")?;
            if i == 2 {
                return Ok(Output::deferred("geom.curve", "SmCurve::Evaluate"));
            }
            Ok(Output::new().int("i", if i == 1 { -1 } else { i }))
        }
        fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
            Fake.cases(ctx)
        }
    }

    #[test]
    fn cpp_side_deferral_is_deferred() {
        let ctx = CaseContext {
            root: std::path::PathBuf::new(),
            seeds: 0,
        };
        let r = run_unit(&CppDefers, &ctx, &RunOptions::default());
        assert_eq!((r.passed(), r.failed(), r.deferred()), (2, 0, 2));
        assert_eq!(r.deferred_by_unit().get("cpp-crash-guard:UB-0"), Some(&2));
        assert_eq!(r.deferred_by_unit().get("geom.curve"), None);
        assert_eq!(
            r.cases[2].deferred.as_ref().map(|d| d.symbol.as_str()),
            Some("SmFake::Crash")
        );
    }
}
