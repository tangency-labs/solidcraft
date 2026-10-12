//! The [`UnitDiff`] trait and the per-unit driver.

use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::Value;

use crate::case::{Case, CaseContext, CaseSource};
use crate::compare::{compare, AbsFloor, CaseDiff};
use crate::output::Output;
use crate::rng::Rng;
use crate::HarnessError;

/// A side-by-side runner for one unit. Implement it on `pub struct Runner;`
/// in `src/units/<unit_id_with_underscores>.rs`.
pub trait UnitDiff {
    /// Unit id as in `plan/modules.toml` (e.g. `nlib.knots`).
    fn id(&self) -> &'static str;

    /// Runs the upstream C++ (via `solidcore_ffi::units::*`).
    /// Return `Ok(Output::err(..))` when the *operation* fails and `Err` only
    /// for harness problems (bad inputs); `Err` always fails the case.
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError>;

    /// Runs the solidcore Rust port on the same inputs.
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
    /// Sensitivity check: multiply the first nonzero finite Rust value of each
    /// case by `1 + PERTURBATION` before comparing; every such case must fail.
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
    /// Case inputs (written to the report only on failure).
    pub inputs: Value,
    /// Comparison result.
    pub diff: CaseDiff,
    /// Harness error on either side (fails the case).
    pub harness_error: Option<String>,
    /// Both sides reported the same operation error.
    pub both_errored: bool,
    /// In perturb mode: whether a value was perturbed.
    pub perturbed: bool,
}

impl CaseResult {
    /// Case passes: no harness error and no mismatch.
    pub fn pass(&self) -> bool {
        self.harness_error.is_none() && self.diff.pass()
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
}

impl UnitResult {
    /// Unit passes: cases built, at least one case, all pass.
    pub fn pass(&self) -> bool {
        self.harness_error.is_none()
            && !self.cases.is_empty()
            && self.cases.iter().all(CaseResult::pass)
    }

    /// Number of failing cases.
    pub fn failed(&self) -> usize {
        self.cases.iter().filter(|c| !c.pass()).count()
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
        let p: Vec<&CaseResult> = self.cases.iter().filter(|c| c.perturbed).collect();
        (p.len(), p.iter().filter(|c| !c.pass()).count())
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

fn perturb(out: &mut Output) -> bool {
    for v in out.values.values_mut() {
        if let Some(x) = v.iter_mut().find(|x| x.is_finite() && **x != 0.0) {
            *x *= 1.0 + PERTURBATION;
            return true;
        }
    }
    false
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
    };
    let cases = match unit.cases(ctx) {
        Ok(c) => c,
        Err(e) => {
            res.harness_error = Some(e.0);
            return res;
        }
    };
    for case in cases {
        if let Some(f) = &opts.filter {
            if !case.name.contains(f.as_str()) {
                continue;
            }
        }
        let cpp = guarded(|| unit.run_cpp(&case), "cpp");
        let rust = guarded(|| unit.run_rust(&case), "rust");
        let mut cr = CaseResult {
            name: case.name.clone(),
            source: case.source.clone(),
            inputs: case.inputs.clone(),
            diff: CaseDiff::default(),
            harness_error: None,
            both_errored: false,
            perturbed: false,
        };
        match (cpp, rust) {
            (Ok(c), Ok(mut r)) => {
                if opts.perturb {
                    cr.perturbed = perturb(&mut r);
                }
                cr.both_errored = c.error.is_some() && r.error.is_some();
                cr.diff = compare(&c, &r, unit.abs_floors());
            }
            (c, r) => {
                let msgs: Vec<String> = [c.err(), r.err()].into_iter().flatten().collect();
                cr.harness_error = Some(msgs.join("; "));
            }
        }
        res.cases.push(cr);
    }
    res
}
