//! The [`UnitDiff`] trait and the Rust-side driver of a unit's cases.

use std::panic::{catch_unwind, AssertUnwindSafe};

use serde_json::Value;

use crate::case::{Case, CaseContext};
use crate::compare::AbsFloor;
use crate::output::Output;
use crate::rng::Rng;
use crate::HarnessError;

/// The runner of one unit. Implement it on `pub struct Runner;` in
/// `src/units/<unit_id_with_underscores>.rs`.
pub trait UnitDiff {
    /// Unit id as in `plan/units/<id>.toml` (e.g. `nlib.knots`).
    fn id(&self) -> &'static str;

    /// Runs solidcore on the case's inputs. Return `Ok(Output::err(..))` when the
    /// *operation* fails and `Err` only for harness problems (bad inputs); `Err` always
    /// breaks the case. Return [`Output::deferred`] (e.g. via [`Output::unported`]) when
    /// the port hits a `SolidError::Unported` callee.
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

/// Options of a unit run.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Only run cases whose name contains this substring.
    pub filter: Option<String>,
}

fn fnv1a64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Runs `f`, turning a harness error or a panic into `Err("rust: ...")`.
fn guarded(f: impl FnOnce() -> Result<Output, HarnessError>) -> Result<Output, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(o)) => Ok(o),
        Ok(Err(e)) => Err(format!("rust: {e}")),
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into());
            Err(format!("rust: panic: {msg}"))
        }
    }
}

/// Result of [`digest_unit`]: the Rust side alone, one digest line per case.
#[derive(Debug, Clone, Default)]
pub struct DigestResult {
    /// Unit id.
    pub unit: String,
    /// One `<case>\t<fnv1a64 of the output's Debug form>` line per case (f64 `Debug` is the
    /// shortest round-trip form, so equal digests mean bit-identical outputs).
    pub lines: Vec<String>,
    /// With `debug`: one `<case>\t<Debug of the output>` line per case.
    pub debug: Vec<String>,
    /// Building the case list failed.
    pub harness_error: Option<String>,
}

/// Runs the Rust side of every case of `unit` (filtered by `opts.filter`; in reverse order
/// with `reverse`) and hands `f` each case and its output or why it failed (a harness error
/// or a panic).
///
/// # Errors
///
/// Building the case list failed.
pub fn each_rust_output(
    unit: &dyn UnitDiff,
    ctx: &CaseContext,
    opts: &RunOptions,
    reverse: bool,
    mut f: impl FnMut(&Case, Result<Output, String>),
) -> Result<(), HarnessError> {
    let mut cases = unit.cases(ctx)?;
    if let Some(filter) = &opts.filter {
        cases.retain(|c| c.name.contains(filter.as_str()));
    }
    if reverse {
        cases.reverse();
    }
    for case in &cases {
        f(case, guarded(|| unit.run_rust(case)));
    }
    Ok(())
}

/// Runs the Rust side of every case of `unit` and digests each output (`--digest-dir`).
/// Nothing is compared: equal digests from two builds (native and wasm32) mean
/// bit-identical outputs.
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
    let walked = each_rust_output(unit, ctx, opts, false, |case, rust| {
        let s = format!("{rust:?}");
        res.lines
            .push(format!("{}\t{:016x}", case.name, fnv1a64(&s)));
        if debug {
            res.debug.push(format!("{}\t{s}", case.name));
        }
    });
    if let Err(e) = walked {
        res.harness_error = Some(e.0);
    }
    res
}
