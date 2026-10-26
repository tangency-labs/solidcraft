//! Runner for unit `geom.local_solve1d`: 1D Newton/bisection/Brent local solver.
//!
//! C++: `solidcore_ffi::units::geom_local_solve1d` (upstream `SmLocalSolve1d`,
//! `SmIterQueue1d`, `SmEvalFunctionObject`; `source/SMLib/src/SmLocalSolve1d.cpp`).
//! Rust: `solidcore::geom::local_solve1d`.
//!
//! The solver is driven by a synthetic test function implemented twice with the
//! same operation order: the shim's `ScTestFn` and [`TestFn`] here. It records every
//! evaluation (`trace_t`, `trace_flags` = `ok + 2 * signal_errors`), so the whole
//! iteration path is compared, not just the answer. Function kinds: polynomial
//! (Horner), signed square root (exact Newton oscillation), trig, atan (Newton
//! overshoot), cube root (Newton divergence), `|t|` kink, Gaussian bump, and the
//! upstream base class evaluator (always `SM_ERR`). Failures can be injected (first
//! N calls, a parameter window, optionally writing `F` or zeroing `F`/`F'`
//! before failing).
//!
//! Outputs per `SolveIt`: `status` (0 or `SmStatus`), `found`, `t` (0 on error),
//! and the solver state afterwards (`state_i`: termination reason, bound hits,
//! iteration, bracket flags; `state_f`: found accuracy, brackets, current and
//! previous iteration values). Every run uses at least [`MIN_GENERATED`] generated
//! inputs (more with `--seeds`).

// Fixed-size parameter arrays and shim output records are indexed with constant
// offsets checked against their lengths above each use.
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::local_solve1d::{
    EvalFunction, FunctionValue, IterQueue1d, IterationValue, LocalSolve1d, SolverAlgorithm,
};
use solidcore::SolidError;
use solidcore_ffi::units::geom_local_solve1d::{self as ffi, FnParams, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// Status of injected evaluator failures (`SM_ERR_INVALID_INPUT`).
const FAIL_STATUS: i32 = 1007;

/// `geom.local_solve1d` runner.
pub struct Runner;

/// Test function description (see the shim's `ScTestFn`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Func {
    /// 0 poly, 1 sqrt, 2 trig, 3 atan, 4 cbrt, 5 abs, 6 gauss, 99 base class.
    kind: u32,
    /// Up to 8 parameters (zero padded).
    #[serde(default)]
    p: Vec<f64>,
    /// `found_answer` when `|F| < found_tol`.
    #[serde(default)]
    found_tol: f64,
    /// Failure window `[fail_lo, fail_hi]` (empty when `fail_lo > fail_hi`).
    #[serde(default = "one")]
    fail_lo: f64,
    #[serde(default)]
    fail_hi: f64,
    /// The first `fail_first` calls fail.
    #[serde(default)]
    fail_first: u32,
    /// Write `F` before failing.
    #[serde(default)]
    partial: bool,
    /// Zero F and F' before failing, as the curvature evaluator does.
    #[serde(default)]
    zero_on_failure: bool,
}

fn one() -> f64 {
    1.0
}

impl Func {
    fn params(&self) -> FnParams {
        let mut a = [0.0; ffi::N_FN_PARAMS];
        a[0] = f64::from(self.kind);
        for (d, s) in a[1..9].iter_mut().zip(&self.p) {
            *d = *s;
        }
        a[9] = self.found_tol;
        a[10] = self.fail_lo;
        a[11] = self.fail_hi;
        a[12] = f64::from(self.fail_first);
        a[13] = if self.zero_on_failure {
            2.0
        } else if self.partial {
            1.0
        } else {
            0.0
        };
        a
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    /// `SolveIt` on one solver for each guess in turn.
    SolveIt {
        func: Func,
        /// Problem interval (`None`: the interval-less constructor).
        #[serde(default)]
        interval: Option<[f64; 2]>,
        #[serde(default)]
        periodic: bool,
        /// `SmLocalSolverAlgorithmType` value.
        #[serde(default)]
        algorithm: i32,
        /// `SetDesiredAccuracy` argument, if called.
        #[serde(default)]
        desired: Option<f64>,
        acceptable: f64,
        guesses: Vec<f64>,
    },
    /// `SolveByBisection`.
    Bisection {
        func: Func,
        level: u32,
        lo_t: f64,
        hi_t: f64,
        lo_f: f64,
        hi_f: f64,
    },
    /// `SolveByBrent`.
    Brent {
        func: Func,
        ax: f64,
        bx: f64,
        cx: f64,
        tol: f64,
        maximize: bool,
    },
    /// `SmIterQueue1d::Add` then `IsOscillating(step)` for each `prev_step`.
    Queue {
        size: u32,
        step: u32,
        steps: Vec<f64>,
    },
}

/// Rust twin of the shim's `ScTestFn`.
struct TestFn {
    p: FnParams,
    calls: u64,
    t: Vec<f64>,
    flags: Vec<i64>,
}

impl TestFn {
    fn new(p: FnParams) -> Self {
        Self {
            p,
            calls: 0,
            t: Vec::new(),
            flags: Vec::new(),
        }
    }
}

/// `SmEvalFunctionObject` base class behaviour (the trait default).
struct Base;
impl EvalFunction for Base {}

impl EvalFunction for TestFn {
    fn evaluate(
        &mut self,
        t: f64,
        out: &mut FunctionValue,
        signal_errors: bool,
    ) -> solidcore::Result<()> {
        self.calls += 1;
        self.t.push(t);
        let sig = if signal_errors { 2 } else { 0 };
        let kind = self.p[0] as i32;
        let p = &self.p[1..9];
        if kind == 99 {
            self.flags.push(sig);
            out.found_answer = false; // as the shim (see there)
            return Base.evaluate(t, out, signal_errors);
        }
        let (mut f, mut fp) = (0.0, 0.0);
        match kind {
            0 => {
                f = p[7];
                fp = 0.0;
                for i in (0..=6).rev() {
                    fp = fp * t + f;
                    f = f * t + p[i];
                }
            }
            1 => {
                let d = t - p[1];
                let s = d.abs().sqrt();
                f = (if d >= 0.0 { p[0] * s } else { -(p[0] * s) }) + p[2];
                fp = if s > 0.0 { p[0] * 0.5 / s } else { 0.0 };
            }
            2 => {
                let arg = p[1] * t + p[2];
                f = p[0] * arg.sin() + p[3] * t + p[4];
                fp = p[0] * p[1] * arg.cos() + p[3];
            }
            3 => {
                let u = p[1] * (t - p[2]);
                f = p[0] * u.atan() + p[3];
                fp = p[0] * p[1] / (1.0 + u * u);
            }
            4 => {
                let q = (t - p[1]).cbrt();
                f = p[0] * q + p[2];
                fp = if q == 0.0 { 0.0 } else { p[0] / (3.0 * q * q) };
            }
            5 => {
                let d = t - p[1];
                f = p[0] * d.abs() + p[2];
                fp = if d >= 0.0 { p[0] } else { -p[0] };
            }
            6 => {
                let d = t - p[2];
                let e = (-p[1] * d * d).exp();
                f = p[0] * e + p[3];
                fp = -2.0 * p[0] * p[1] * d * e;
            }
            _ => {}
        }
        let found_tol = self.p[9];
        let fail_lo = self.p[10];
        let fail_hi = self.p[11];
        let fail_first = self.p[12];
        let partial = self.p[13] != 0.0;
        let fail = (self.calls as f64) <= fail_first || (fail_lo <= t && t <= fail_hi);
        if fail {
            if self.p[13] == 2.0 {
                out.f = 0.0;
                out.f_prime = 0.0;
            } else if partial {
                out.f = f;
            }
            out.found_answer = false;
            self.flags.push(sig);
            return Err(SolidError::Upstream {
                operation: "difftest::geom_local_solve1d::TestFn",
                code: FAIL_STATUS,
                entity: None,
            });
        }
        out.f = f;
        out.f_prime = fp;
        out.found_answer = f.abs() < found_tol;
        self.flags.push(1 + sig);
        Ok(())
    }
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(harness)`
fn harness(e: impl ToString) -> HarnessError {
    HarnessError::new(e.to_string())
}

/// Status code of a Rust error (only upstream statuses are expected).
fn status_of(e: &SolidError) -> Result<i64, HarnessError> {
    match e {
        SolidError::Upstream { code, .. } => Ok(i64::from(*code)),
        other => Err(harness(format!("unexpected error {other}"))),
    }
}

/// Accumulated outputs.
#[derive(Default)]
struct Acc {
    status: Vec<i64>,
    found: Vec<i64>,
    t: Vec<f64>,
    state_i: Vec<i64>,
    state_f: Vec<f64>,
}

impl Acc {
    fn finish(self, trace_t: Vec<f64>, trace_flags: Vec<i64>) -> Output {
        Output::new()
            .ints("status", self.status)
            .ints("found", self.found)
            .values("t", self.t)
            .ints("state_i", self.state_i)
            .values("state_f", self.state_f)
            .values("trace_t", trace_t)
            .ints("trace_flags", trace_flags)
    }
}

fn int(v: f64) -> Result<i64, HarnessError> {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        Ok(v as i64)
    } else {
        Err(harness(format!("non-integer {v} from shim")))
    }
}

/// Splits the shim's trace `[n, t[n], flags[n]]` starting at `at`.
fn cpp_trace(o: &[f64], at: usize) -> Result<(Vec<f64>, Vec<i64>), HarnessError> {
    let n = int(*o.get(at).ok_or_else(|| harness("short output"))?)? as usize;
    let t = o
        .get(at + 1..at + 1 + n)
        .ok_or_else(|| harness("short trace"))?
        .to_vec();
    let flags = o
        .get(at + 1 + n..at + 1 + 2 * n)
        .ok_or_else(|| harness("short trace"))?
        .iter()
        .map(|v| int(*v))
        .collect::<Result<Vec<_>, _>>()?;
    if o.len() != at + 1 + 2 * n {
        return Err(harness("trailing output"));
    }
    Ok((t, flags))
}

const PER_SOLVE: usize = 26;

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::SolveIt {
            func,
            interval,
            periodic,
            algorithm,
            desired,
            acceptable,
            guesses,
        } => {
            let [lo, hi] = interval.unwrap_or([0.0, 0.0]);
            let mut s = vec![
                f64::from(u8::from(interval.is_some())),
                lo,
                hi,
                f64::from(u8::from(*periodic)),
                f64::from(*algorithm),
                f64::from(u8::from(desired.is_some())),
                desired.unwrap_or(0.0),
                *acceptable,
            ];
            s.extend(guesses);
            let o = ffi::run(Op::SolveIt, &func.params(), &s).map_err(harness)?;
            let mut acc = Acc::default();
            for g in 0..guesses.len() {
                let r = o
                    .get(g * PER_SOLVE..(g + 1) * PER_SOLVE)
                    .ok_or_else(|| harness("short output"))?;
                acc.status.push(int(r[0])?);
                acc.found.push(int(r[1])?);
                acc.t.push(r[2]);
                acc.state_f.push(r[3]);
                for k in [4, 5, 6, 7, 8, 11] {
                    acc.state_i.push(int(r[k])?);
                }
                acc.state_f.extend([r[9], r[10], r[12], r[13]]);
                acc.state_f.extend(&r[14..26]);
            }
            let (tt, tf) = cpp_trace(&o, guesses.len() * PER_SOLVE)?;
            Ok(acc.finish(tt, tf))
        }
        Inputs::Bisection {
            func,
            level,
            lo_t,
            hi_t,
            lo_f,
            hi_f,
        } => {
            let s = [f64::from(*level), *lo_t, *hi_t, *lo_f, *hi_f];
            let o = ffi::run(Op::Bisection, &func.params(), &s).map_err(harness)?;
            pair_cpp(&o)
        }
        Inputs::Brent {
            func,
            ax,
            bx,
            cx,
            tol,
            maximize,
        } => {
            let s = [*ax, *bx, *cx, *tol, f64::from(u8::from(*maximize))];
            let o = ffi::run(Op::Brent, &func.params(), &s).map_err(harness)?;
            pair_cpp(&o)
        }
        Inputs::Queue { size, step, steps } => {
            let mut s = vec![f64::from(*size), f64::from(*step)];
            s.extend(steps);
            let o = ffi::run(Op::Queue, &Func::default_params(), &s).map_err(harness)?;
            let v = o.iter().map(|v| int(*v)).collect::<Result<Vec<_>, _>>()?;
            Ok(Output::new().ints("oscillating", v))
        }
    }
}

impl Func {
    fn default_params() -> FnParams {
        let mut a = [0.0; ffi::N_FN_PARAMS];
        a[10] = 1.0;
        a
    }
}

fn pair_cpp(o: &[f64]) -> Result<Output, HarnessError> {
    let r = o.get(0..3).ok_or_else(|| harness("short output"))?;
    let (tt, tf) = cpp_trace(o, 3)?;
    Ok(Output::new()
        .int("status", int(r[0])?)
        .values("tf", vec![r[1], r[2]])
        .values("trace_t", tt)
        .ints("trace_flags", tf))
}

fn push_iv(v: &mut Vec<f64>, iv: &IterationValue) {
    v.extend([iv.t, iv.f, iv.f_prime, iv.prev_step]);
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::SolveIt {
            func,
            interval,
            periodic,
            algorithm,
            desired,
            acceptable,
            guesses,
        } => {
            let mut f = TestFn::new(func.params());
            let mut solver = match interval {
                Some([lo, hi]) => {
                    LocalSolve1d::with_interval(Extent1d::from_min_max(*lo, *hi), *periodic)
                }
                None => LocalSolve1d::new(),
            };
            solver.set_solver_algorithm(
                SolverAlgorithm::from_code(*algorithm).ok_or_else(|| harness("algorithm"))?,
            );
            if let Some(d) = desired {
                solver.set_desired_accuracy(*d);
            }
            let mut acc = Acc::default();
            for g in guesses {
                match solver.solve_it(&mut f, *g, *acceptable) {
                    Ok(r) => {
                        acc.status.push(0);
                        acc.found.push(i64::from(r.found));
                        acc.t.push(r.t);
                    }
                    Err(e) => {
                        if let Some(o) = Output::unported(&e) {
                            return Ok(o);
                        }
                        acc.status.push(status_of(&e)?);
                        acc.found.push(0);
                        acc.t.push(0.0);
                    }
                }
                acc.state_f.push(solver.found_accuracy());
                acc.state_i.extend([
                    i64::from(solver.termination_reason().code()),
                    solver.lower_bound_hits() as i64,
                    solver.upper_bound_hits() as i64,
                    solver.iter as i64,
                    i64::from(solver.have_lower_bracket),
                    i64::from(solver.have_upper_bracket),
                ]);
                acc.state_f.extend([
                    solver.lower_bracket_f,
                    solver.lower_bracket_t,
                    solver.upper_bracket_f,
                    solver.upper_bracket_t,
                ]);
                push_iv(&mut acc.state_f, &solver.curr);
                push_iv(&mut acc.state_f, &solver.prev1);
                push_iv(&mut acc.state_f, &solver.prev2);
            }
            Ok(acc.finish(f.t, f.flags))
        }
        Inputs::Bisection {
            func,
            level,
            lo_t,
            hi_t,
            lo_f,
            hi_f,
        } => {
            let mut f = TestFn::new(func.params());
            let r = LocalSolve1d::new().solve_by_bisection(
                &mut f,
                u64::from(*level),
                *lo_t,
                *hi_t,
                *lo_f,
                *hi_f,
            );
            pair_rust(r, f)
        }
        Inputs::Brent {
            func,
            ax,
            bx,
            cx,
            tol,
            maximize,
        } => {
            let mut f = TestFn::new(func.params());
            let r = LocalSolve1d::new().solve_by_brent(&mut f, *ax, *bx, *cx, *tol, *maximize);
            pair_rust(r, f)
        }
        Inputs::Queue { size, step, steps } => {
            let mut q = match IterQueue1d::try_new(u64::from(*size)) {
                Ok(q) => q,
                Err(e) => return Err(HarnessError::new(format!("queue size: {e}"))),
            };
            let mut v = Vec::with_capacity(steps.len());
            for s in steps {
                q.add(&IterationValue {
                    prev_step: *s,
                    ..IterationValue::default()
                });
                v.push(i64::from(q.is_oscillating(u64::from(*step))));
            }
            Ok(Output::new().ints("oscillating", v))
        }
    }
}

fn pair_rust(r: solidcore::Result<(f64, f64)>, f: TestFn) -> Result<Output, HarnessError> {
    let (status, t, fv) = match r {
        Ok((t, fv)) => (0, t, fv),
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Ok(o);
            }
            (status_of(&e)?, 0.0, 0.0)
        }
    };
    Ok(Output::new()
        .int("status", status)
        .values("tf", vec![t, fv])
        .values("trace_t", f.t)
        .ints("trace_flags", f.flags))
}

/// Random test function.
fn gen_func(rng: &mut Rng) -> Func {
    let kind = *rng
        .choose(&[0u32, 0, 0, 1, 1, 2, 2, 3, 3, 4, 5, 6, 6, 99])
        .unwrap_or(&0);
    let p: Vec<f64> = match kind {
        0 => {
            let deg = 1 + rng.below(5) as usize;
            let mut c: Vec<f64> = (0..=deg).map(|_| rng.uniform(-3.0, 3.0)).collect();
            if rng.chance(0.15) {
                // Newton cycle 0 <-> 1 of t^3 - 2t + 2, scaled
                let s = rng.uniform(0.5, 2.0);
                c = vec![2.0 * s, -2.0 * s, 0.0, s];
            }
            c.resize(8, 0.0);
            c
        }
        1 => vec![
            rng.uniform(-2.0, 2.0),
            rng.uniform(-1.0, 1.0),
            if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(-0.5, 0.5)
            },
        ],
        2 => vec![
            rng.uniform(-2.0, 2.0),
            rng.uniform(0.2, 8.0),
            rng.uniform(-3.0, 3.0),
            if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(-1.0, 1.0)
            },
            rng.uniform(-1.0, 1.0),
        ],
        3 => vec![
            rng.uniform(-2.0, 2.0),
            rng.uniform(0.5, 50.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-0.5, 0.5),
        ],
        4 | 5 => vec![
            rng.uniform(-2.0, 2.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-0.5, 0.5),
        ],
        6 => vec![
            rng.uniform(-2.0, 2.0),
            rng.uniform(0.5, 10.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-0.5, 0.5),
        ],
        _ => Vec::new(),
    };
    let (mut fail_lo, mut fail_hi) = (1.0, 0.0);
    if rng.chance(0.15) {
        let c = rng.uniform(-2.0, 2.0);
        let w = rng.uniform(0.0, 0.5);
        fail_lo = c - w;
        fail_hi = c + w;
    }
    Func {
        kind,
        p,
        found_tol: if rng.chance(0.3) {
            10f64.powf(rng.uniform(-12.0, -4.0))
        } else {
            0.0
        },
        fail_lo,
        fail_hi,
        fail_first: if rng.chance(0.1) {
            rng.below(3) as u32
        } else {
            0
        },
        partial: rng.chance(0.5),
        zero_on_failure: false,
    }
}

/// Quartic `s * (t^4 + a t^2 + c)` with `c = 7 + 3a`: Newton has the 2-cycle
/// `{-1, 1}` (repelling), with or without roots in between, so oscillation
/// handling and its bracket search are exercised from nearby guesses.
fn gen_cycle_func(rng: &mut Rng) -> Func {
    let s = if rng.chance(0.5) { 1.0 } else { -1.0 } * rng.uniform(0.5, 2.0);
    let a = rng.uniform(-2.6, -1.5);
    let c = 7.0 + 3.0 * a;
    let mut p = vec![s * c, 0.0, s * a, 0.0, s];
    if rng.chance(0.5) {
        // odd perturbation: asymmetric cycle
        p[1] = rng.uniform(-0.05, 0.05);
        p[3] = rng.uniform(-0.05, 0.05);
    }
    p.resize(8, 0.0);
    Func {
        kind: 0,
        p,
        found_tol: 0.0,
        fail_lo: 1.0,
        fail_hi: 0.0,
        fail_first: 0,
        partial: false,
        zero_on_failure: false,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.local_solve1d"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let r = rng.unit();
        let inputs = if r < 0.7 {
            let (func, wide) = if rng.chance(0.25) {
                (gen_cycle_func(rng), true)
            } else {
                (gen_func(rng), false)
            };
            let interval = if wide {
                Some([-20.0, 20.0])
            } else if rng.chance(0.9) {
                let lo = rng.uniform(-3.0, 0.0);
                let hi = if rng.chance(0.05) {
                    lo
                } else {
                    lo + rng.uniform(0.1, 6.0)
                };
                Some([lo, hi])
            } else {
                None
            };
            let n = if rng.chance(0.2) { 2 } else { 1 };
            let guesses = (0..n)
                .map(|_| match interval {
                    _ if wide => rng.uniform(-2.0, 2.0),
                    Some([lo, hi]) if rng.chance(0.9) => lo + rng.uniform(-0.05, 1.05) * (hi - lo),
                    _ => rng.uniform(-3.0, 3.0),
                })
                .collect();
            Inputs::SolveIt {
                func,
                interval,
                periodic: rng.chance(0.2),
                algorithm: rng.below(4) as i32,
                desired: if rng.chance(0.3) {
                    Some(10f64.powf(rng.uniform(-14.0, -3.0)))
                } else {
                    None
                },
                acceptable: 10f64.powf(rng.uniform(-12.0, -1.0)),
                guesses,
            }
        } else if r < 0.8 {
            let func = gen_func(rng);
            let lo_t = rng.uniform(-3.0, 3.0);
            let hi_t = rng.uniform(-3.0, 3.0);
            Inputs::Bisection {
                func,
                level: rng.below(45) as u32,
                lo_t,
                hi_t,
                lo_f: -rng.uniform(0.0, 3.0),
                hi_f: rng.uniform(0.0, 3.0),
            }
        } else if r < 0.92 {
            let func = gen_func(rng);
            let ax = rng.uniform(-3.0, 0.0);
            let cx = ax + rng.uniform(0.0, 4.0);
            let bx = ax + rng.unit() * (cx - ax);
            let (ax, cx) = if rng.chance(0.3) { (cx, ax) } else { (ax, cx) };
            Inputs::Brent {
                func,
                ax,
                bx,
                cx,
                tol: 10f64.powf(rng.uniform(-14.0, -3.0)),
                maximize: rng.chance(0.5),
            }
        } else {
            // At least one step: an empty queue yields no outputs to compare (vacuous).
            let n = 1 + rng.below(24) as usize;
            let alt = rng.chance(0.6);
            let steps = (0..n)
                .map(|k| {
                    let m = rng.uniform(0.5, 1.5);
                    if alt && k % 2 == 1 {
                        -m
                    } else {
                        m
                    }
                })
                .collect();
            Inputs::Queue {
                size: 1 + rng.below(10) as u32,
                step: *rng.choose(&[1u32, 2, 2, 2, 3]).unwrap_or(&2),
                steps,
            }
        };
        serde_json::to_value(inputs).ok()
    }
}
