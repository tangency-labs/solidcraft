//! Runner for unit `geom.local_solve_nd`: the N-dimensional Newton-Raphson solver and
//! the finite-difference Jacobian.
//!
//! C++: `solidcore_ffi::units::geom_local_solve_nd` (upstream `SmLocalSolveNd::SolveIt`,
//! `SmEvalNFunctionsObject::ComputeJacobian`). Rust: `solidcore::geom::local_solve_nd`.
//!
//! Both sides solve the same test equation set (the shim's `ScTestENFO`, mirrored by
//! [`TestEquations`] operation for operation):
//!
//! `F_i = -b_i + sum_j a_ij x_j + c_i x_i^3 + d_i x_i x_k + e_i |x_i|`, `k = (i+1) % n`,
//!
//! with an analytic Jacobian (scaled by `jac_scale`) or the finite-difference one of
//! `ComputeJacobian` (optionally clamped to the intervals), injected evaluation
//! failures (`fail_calls`, `fail_radius`), an optional "found answer" threshold and a
//! solution size / scaled zero for `IsConverged`. Every `Evaluate` call is traced (its
//! `x` and flags: 1 = Jacobian requested, 2 = failed), so the whole iteration path is
//! compared, not only the result. Failures are normalised to `status_<SmStatus>`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::local_solve_nd::{
    compute_jacobian, BoundaryHandler, EvalNFunctions, LocalSolveNd,
};
use solidcore::geom::matrix::Matrix;
use solidcore::math::{EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT};
use solidcore::SolidError;
use solidcore_ffi::units::geom_local_solve_nd::{self as ffi, Op, Problem};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::json::num;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

use solidcore::error::SM_ERR;
/// Trace flag: a Jacobian was requested.
const TRACE_JACOBIAN: i64 = 1;
/// Trace flag: the call failed.
const TRACE_FAILED: i64 = 2;

/// `geom.local_solve_nd` runner.
pub struct Runner;

fn one() -> f64 {
    1.0
}

fn default_acceptable() -> Value {
    num(EFF_ZERO_SQRT)
}

/// One parameter interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Ivl {
    lo: Value,
    hi: Value,
    #[serde(default)]
    unbounded: bool,
    #[serde(default)]
    periodic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // independent switches of the test problem
struct Inputs {
    /// `solve` or `jacobian`.
    op: String,
    n: usize,
    a: Vec<Value>,
    #[serde(default)]
    b: Vec<Value>,
    #[serde(default)]
    c: Vec<Value>,
    #[serde(default)]
    d: Vec<Value>,
    #[serde(default)]
    e: Vec<Value>,
    guess: Vec<Value>,
    #[serde(default = "one")]
    jac_scale: f64,
    #[serde(default)]
    fail_radius: f64,
    #[serde(default)]
    found_tol: f64,
    #[serde(default)]
    size_scale: f64,
    #[serde(default = "one")]
    scaled_zero: f64,
    /// `SetDesiredAccuracy` argument, if called.
    #[serde(default)]
    desired: Option<Value>,
    #[serde(default = "default_acceptable")]
    acceptable: Value,
    /// 0 analytic, 1 `ComputeJacobian`, 2 `ComputeJacobian` with the intervals.
    #[serde(default)]
    jac_mode: i64,
    #[serde(default)]
    has_intervals: bool,
    #[serde(default)]
    set_intervals_ok: bool,
    #[serde(default)]
    max_iter: Option<i64>,
    #[serde(default)]
    step_cutting: Option<bool>,
    /// `total`, `all` or `ignore`.
    #[serde(default)]
    handler: Option<String>,
    #[serde(default)]
    max_hits: Option<i64>,
    /// Intervals (empty: all unbounded `[0, 0]`).
    #[serde(default)]
    ivl: Vec<Ivl>,
    /// Pass the intervals to the solver (`solve`) / to `ComputeJacobian` (`jacobian`).
    #[serde(default)]
    use_intervals: bool,
    #[serde(default)]
    use_periodic: bool,
    #[serde(default)]
    fail_calls: Vec<i64>,
}

/// A JSON number or `"NaN"` / `"inf"` / `"-inf"`.
fn float(v: &Value) -> Result<f64, HarnessError> {
    match v {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| HarnessError::new(format!("bad number {n}"))),
        Value::String(s) if s == "NaN" => Ok(f64::NAN),
        Value::String(s) if s == "inf" => Ok(f64::INFINITY),
        Value::String(s) if s == "-inf" => Ok(f64::NEG_INFINITY),
        other => Err(HarnessError::new(format!("bad float {other}"))),
    }
}

/// `v` as `len` floats (empty = zeros).
fn floats(v: &[Value], len: usize, what: &str) -> Result<Vec<f64>, HarnessError> {
    if v.is_empty() {
        return Ok(vec![0.0; len]);
    }
    if v.len() != len {
        return Err(HarnessError::new(format!(
            "{what}: {} values, want {len}",
            v.len()
        )));
    }
    v.iter().map(float).collect()
}

/// Parsed, validated inputs shared by both sides.
struct Parsed {
    solve: bool,
    n: usize,
    coef: Vec<f64>,
    guess: Vec<f64>,
    cfg: [f64; 7],
    icfg: [i64; 10],
    ivl: Vec<f64>,
    ivl_flags: Vec<i64>,
    fail_calls: Vec<i64>,
}

fn handler_code(h: &str) -> Result<i64, HarnessError> {
    match h {
        "total" => Ok(0),
        "all" => Ok(1),
        "ignore" => Ok(2),
        other => Err(HarnessError::new(format!("unknown handler {other}"))),
    }
}

fn parse(i: &Inputs) -> Result<Parsed, HarnessError> {
    let n = i.n;
    if n == 0 || n > ffi::MAX_N {
        return Err(HarnessError::new(format!("n = {n}")));
    }
    let solve = match i.op.as_str() {
        "solve" => true,
        "jacobian" => false,
        other => return Err(HarnessError::new(format!("unknown op {other}"))),
    };
    let mut coef = floats(&i.a, n * n, "a")?;
    for (v, what) in [(&i.b, "b"), (&i.c, "c"), (&i.d, "d"), (&i.e, "e")] {
        coef.extend(floats(v, n, what)?);
    }
    let guess = floats(&i.guess, n, "guess")?;
    let desired = i.desired.as_ref().map(float).transpose()?;
    let cfg = [
        i.jac_scale,
        i.fail_radius,
        i.found_tol,
        i.size_scale,
        i.scaled_zero,
        desired.unwrap_or(0.0),
        float(&i.acceptable)?,
    ];
    let max_iter = i.max_iter.unwrap_or(-1);
    if max_iter > ffi::MAX_ITER || !(0..=2).contains(&i.jac_mode) {
        return Err(HarnessError::new("max_iter / jac_mode out of range"));
    }
    let icfg = [
        i.jac_mode,
        i64::from(i.has_intervals),
        i64::from(i.set_intervals_ok),
        i64::from(desired.is_some()),
        max_iter,
        i.step_cutting.map_or(-1, i64::from),
        i.handler
            .as_deref()
            .map(handler_code)
            .transpose()?
            .unwrap_or(-1),
        i.max_hits.unwrap_or(-1),
        i64::from(i.use_intervals),
        i64::from(i.use_periodic),
    ];
    let (ivl, ivl_flags) = if i.ivl.is_empty() {
        (vec![0.0; 2 * n], vec![1; n])
    } else {
        if i.ivl.len() != n {
            return Err(HarnessError::new("ivl count differs from n"));
        }
        let mut v = Vec::new();
        let mut f = Vec::new();
        for x in &i.ivl {
            v.push(float(&x.lo)?);
            v.push(float(&x.hi)?);
            f.push(i64::from(x.unbounded) | (i64::from(x.periodic) << 1));
        }
        (v, f)
    };
    if i.fail_calls.len() > ffi::MAX_FAIL {
        return Err(HarnessError::new("too many fail_calls"));
    }
    Ok(Parsed {
        solve,
        n,
        coef,
        guess,
        cfg,
        icfg,
        ivl,
        ivl_flags,
        fail_calls: i.fail_calls.clone(),
    })
}

fn solve_output(x: Vec<f64>, trace: Vec<f64>, head: [i64; 5], flags: Vec<i64>) -> Output {
    Output::new()
        .values("x", x)
        .values("trace", trace)
        .int("found", head[0])
        .int("reason", head[1])
        .int("iteration", head[2])
        .int("calls", head[3])
        .int("set_intervals_calls", head[4])
        .ints("flags", flags)
}

fn jacobian_output(
    j: Vec<f64>,
    x: Vec<f64>,
    trace: Vec<f64>,
    calls: i64,
    flags: Vec<i64>,
    status: i32,
) -> Output {
    Output::new()
        .values("jacobian", j)
        .values("x", x)
        .values("trace", trace)
        .int("calls", calls)
        .ints("flags", flags)
        .int("status", i64::from(status))
}

fn cpp(p: &Parsed) -> Result<Output, HarnessError> {
    let n = p.n;
    let problem = Problem {
        n,
        coef: &p.coef,
        cfg: p.cfg,
        icfg: p.icfg,
        guess: &p.guess,
        ivl: &p.ivl,
        ivl_flags: &p.ivl_flags,
        fail_calls: &p.fail_calls,
    };
    let op = if p.solve { Op::Solve } else { Op::Jacobian };
    let r = match ffi::run(op, &problem) {
        Ok(r) => r,
        Err(e @ (FfiError::InvalidArgument(_) | FfiError::Status(_))) => {
            return Err(HarnessError::new(e.to_string()))
        }
    };
    let short = || HarnessError::new("shim returned too few items");
    if p.solve {
        if r.status != 0 {
            return Ok(Output::err(format!("status_{}", r.status)));
        }
        let x = r.values.get(..n).ok_or_else(short)?.to_vec();
        let trace = r.values.get(n..).ok_or_else(short)?.to_vec();
        let head: [i64; 5] = r
            .ints
            .get(..5)
            .and_then(|h| h.try_into().ok())
            .ok_or_else(short)?;
        let flags = r.ints.get(5..).ok_or_else(short)?.to_vec();
        Ok(solve_output(x, trace, head, flags))
    } else {
        let j = r.values.get(..n * n).ok_or_else(short)?.to_vec();
        let x = r.values.get(n * n..n * n + n).ok_or_else(short)?.to_vec();
        let trace = r.values.get(n * n + n..).ok_or_else(short)?.to_vec();
        let calls = *r.ints.first().ok_or_else(short)?;
        let flags = r.ints.get(1..).ok_or_else(short)?.to_vec();
        Ok(jacobian_output(j, x, trace, calls, flags, r.status))
    }
}

// ---- Rust side ------------------------------------------------------------------

/// `v[i]` (inputs are validated, so the fallback is never used).
fn at(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

/// `v[i] = x` (inputs are validated, so `i` is always in range).
fn put(v: &mut [f64], i: usize, x: f64) {
    if let Some(slot) = v.get_mut(i) {
        *slot = x;
    }
}

fn sm_err() -> SolidError {
    SolidError::Upstream {
        operation: "difftest::geom_local_solve_nd::TestEquations",
        code: SM_ERR,
        entity: None,
    }
}

/// The test equation set (mirror of the shim's `ScTestENFO`).
struct TestEquations<'a> {
    n: usize,
    a: &'a [f64],
    b: &'a [f64],
    c: &'a [f64],
    d: &'a [f64],
    e: &'a [f64],
    jac_scale: f64,
    fail_radius: f64,
    found_tol: f64,
    size_scale: f64,
    solution_size: f64,
    scaled_zero: f64,
    jac_mode: i64,
    has_intervals: bool,
    set_intervals_ok: bool,
    set_intervals_calls: i64,
    fail_calls: &'a [i64],
    jac_intervals: Vec<Extent1d>,
    calls: i64,
    trace: Vec<f64>,
    flags: Vec<i64>,
}

impl EvalNFunctions for TestEquations<'_> {
    fn solution_size(&self) -> f64 {
        self.solution_size
    }

    fn scaled_zero(&self) -> f64 {
        self.scaled_zero
    }

    fn has_intervals(&self) -> bool {
        self.has_intervals
    }

    fn set_intervals(
        &mut self,
        _intervals: Option<&ExtentNd>,
        _periodicities: Option<&[bool]>,
    ) -> solidcore::Result<()> {
        if !self.set_intervals_ok {
            return Err(sm_err());
        }
        self.set_intervals_calls += 1;
        Ok(())
    }

    fn evaluate(
        &mut self,
        x: &[f64],
        f: &mut [f64],
        jacobian: Option<&mut Matrix>,
    ) -> solidcore::Result<bool> {
        let n = self.n;
        let call = self.calls;
        self.calls += 1;
        self.trace.extend_from_slice(x);
        let flag = if jacobian.is_some() {
            TRACE_JACOBIAN
        } else {
            0
        };
        let mut fail = self.fail_calls.contains(&call);
        if self.fail_radius > 0.0 && x.iter().any(|xi| xi.abs() > self.fail_radius) {
            fail = true;
        }
        if fail {
            self.flags.push(flag | TRACE_FAILED);
            return Err(sm_err());
        }
        self.flags.push(flag);

        let mut sum = 0.0;
        for i in 0..n {
            let k = (i + 1) % n;
            let xi = at(x, i);
            let mut v = -at(self.b, i);
            for j in 0..n {
                v += at(self.a, i * n + j) * at(x, j);
            }
            v += at(self.c, i) * xi * xi * xi;
            v += at(self.d, i) * xi * at(x, k);
            v += at(self.e, i) * xi.abs();
            put(f, i, v);
            sum += v.abs();
        }
        self.solution_size = self.size_scale * sum;

        if let Some(jac) = jacobian {
            if self.jac_mode == 0 {
                for i in 0..n {
                    let k = (i + 1) % n;
                    let xi = at(x, i);
                    for j in 0..n {
                        let mut g = at(self.a, i * n + j);
                        if j == i {
                            g += 3.0 * at(self.c, i) * xi * xi;
                            g += at(self.d, i) * at(x, k);
                            g += at(self.e, i) * (if xi >= 0.0 { 1.0 } else { -1.0 });
                        }
                        if j == k {
                            g += at(self.d, i) * xi;
                        }
                        g *= self.jac_scale;
                        jac.set_at(i, j, g)?;
                    }
                }
            } else {
                let mut tmp_x = x.to_vec();
                let fv = f.to_vec();
                let ivls = (self.jac_mode == 2).then(|| self.jac_intervals.clone());
                compute_jacobian(self, &mut tmp_x, &fv, jac, ivls.as_deref())?;
            }
        }
        Ok(self.found_tol > 0.0 && sum < self.found_tol)
    }
}

fn handler_of(code: i64) -> BoundaryHandler {
    match code {
        0 => BoundaryHandler::TotalBoundaryHits,
        1 => BoundaryHandler::HitsAllBoundaries,
        _ => BoundaryHandler::IgnoreBoundaryHits,
    }
}

fn rust(p: &Parsed) -> Result<Output, HarnessError> {
    let n = p.n;
    let short = || HarnessError::new("coefficient array too short");
    let (a, rest) = p.coef.split_at_checked(n * n).ok_or_else(short)?;
    let (b, rest) = rest.split_at_checked(n).ok_or_else(short)?;
    let (c, rest) = rest.split_at_checked(n).ok_or_else(short)?;
    let (d, e) = rest.split_at_checked(n).ok_or_else(short)?;
    let mut intervals = ExtentNd::new(n);
    let mut jac_intervals = Vec::new();
    let mut periodic = Vec::new();
    for (i, (lohi, &flags)) in p
        .ivl
        .as_chunks::<2>()
        .0
        .iter()
        .zip(&p.ivl_flags)
        .enumerate()
    {
        let mut ext = Extent1d::from_min_max(lohi[0], lohi[1]);
        if flags & 1 != 0 {
            ext.set_unbounded();
        }
        if let Some(slot) = intervals.get_mut(i) {
            *slot = ext;
        }
        jac_intervals.push(ext);
        periodic.push(flags & 2 != 0);
    }
    let [jac_scale, fail_radius, found_tol, size_scale, scaled_zero, desired, acceptable] = p.cfg;
    let [jac_mode, has_ivl, set_ok, set_desired, max_iter, step_cutting, handler, max_hits, use_ivl, use_per] =
        p.icfg;
    let mut eq = TestEquations {
        n,
        a,
        b,
        c,
        d,
        e,
        jac_scale,
        fail_radius,
        found_tol,
        size_scale,
        solution_size: 0.0,
        scaled_zero,
        jac_mode,
        has_intervals: has_ivl != 0,
        set_intervals_ok: set_ok != 0,
        set_intervals_calls: 0,
        fail_calls: &p.fail_calls,
        jac_intervals: jac_intervals.clone(),
        calls: 0,
        trace: Vec::new(),
        flags: Vec::new(),
    };

    if !p.solve {
        let mut f = vec![0.0; n];
        let mut x = p.guess.clone();
        let mut jac = Matrix::new(n, n);
        let r = eq.evaluate(&x, &mut f, None).and_then(|_| {
            compute_jacobian(
                &mut eq,
                &mut x,
                &f,
                &mut jac,
                (use_ivl != 0).then_some(jac_intervals.as_slice()),
            )
        });
        let status = match r {
            Ok(()) => 0,
            Err(e) => {
                if let Some(o) = Output::unported(&e) {
                    return Ok(o);
                }
                match e {
                    SolidError::Upstream { code, .. } => code,
                    other => return Ok(Output::err(format!("rust_error: {other}"))),
                }
            }
        };
        return Ok(jacobian_output(
            jac.as_slice().to_vec(),
            x,
            eq.trace,
            eq.calls,
            eq.flags,
            status,
        ));
    }

    let ivl_ref = (use_ivl != 0).then_some(&intervals);
    let per_ref = (use_per != 0).then_some(periodic.as_slice());
    let (result, reason, iteration) = {
        let mut solver = LocalSolveNd::new(&mut eq, ivl_ref, per_ref);
        if set_desired != 0 {
            solver.set_desired_accuracy(desired);
        }
        if max_iter >= 0 {
            solver.set_maximum_iterations(max_iter as u64);
        }
        if step_cutting >= 0 {
            solver.set_step_cutting_enabled(step_cutting != 0);
        }
        if handler >= 0 {
            let hits = if max_hits >= 0 { max_hits as u64 } else { 3 };
            solver.set_boundary_handler(handler_of(handler), hits);
        }
        let result = solver.solve_it(&p.guess, acceptable);
        let reason = i64::from(solver.termination_reason().code());
        let iteration = solver.current_iteration() as i64;
        (result, reason, iteration)
    };
    match result {
        Ok(sol) => {
            let head = [
                i64::from(sol.found),
                reason,
                iteration,
                eq.calls,
                eq.set_intervals_calls,
            ];
            Ok(solve_output(sol.x, eq.trace, head, eq.flags))
        }
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| {
            Output::err(match e {
                SolidError::Upstream { code, .. } => format!("status_{code}"),
                SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => "status_1007".into(),
                other => format!("rust_error: {other}"),
            })
        })),
    }
}

// ---- generation ---------------------------------------------------------------

fn pick_n(rng: &mut Rng) -> usize {
    match rng.below(10) {
        0..=2 => 1,
        3..=5 => 2,
        6 => 3,
        7 => 4,
        8 => 5 + rng.below(2) as usize,
        _ => 7 + rng.below(2) as usize,
    }
}

/// A coefficient: mostly moderate, sometimes zero / tiny / huge / non-finite.
fn coef(rng: &mut Rng, scale: f64, nice: bool) -> f64 {
    if nice {
        return rng.uniform(-scale, scale);
    }
    let r = rng.unit();
    if r < 0.75 {
        rng.uniform(-scale, scale)
    } else if r < 0.88 {
        0.0
    } else if r < 0.94 {
        rng.range_i64(-3, 3) as f64
    } else if r < 0.98 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(6, 14) as i32))
    } else if r < 0.995 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(rng.range_i64(3, 9) as i32)
    } else {
        match rng.below(3) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            _ => f64::NEG_INFINITY,
        }
    }
}

fn eval_f(n: usize, a: &[f64], c: &[f64], d: &[f64], e: &[f64], x: &[f64]) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let k = (i + 1) % n;
            let xi = at(x, i);
            let mut v = 0.0;
            for j in 0..n {
                v += at(a, i * n + j) * at(x, j);
            }
            v + at(c, i) * xi * xi * xi + at(d, i) * xi * at(x, k) + at(e, i) * xi.abs()
        })
        .collect()
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let n = pick_n(rng);
    let well_posed = rng.chance(0.6);
    // "nice" problems stay bounded: only they get periodic intervals, whose upstream
    // PeriodicWrap loops once per period between a far value and the interval
    let nice = well_posed && rng.chance(0.4);
    let nl = if rng.chance(0.3) {
        0.0
    } else {
        rng.uniform(0.0, 1.0)
    };
    let mut a: Vec<f64> = (0..n * n).map(|_| coef(rng, 2.0, nice)).collect();
    if well_posed {
        // diagonally dominant
        for i in 0..n {
            let sign = if rng.chance(0.2) { -1.0 } else { 1.0 };
            let diag = (2.0 + rng.uniform(0.0, 4.0)) * (n as f64) * sign;
            put(&mut a, i * n + i, diag);
        }
    }
    match if nice { 99 } else { rng.below(14) } {
        0 => {
            // zero row: SM_ERR from the LU decomposition (bad Jacobian)
            let r = rng.below(n as u64) as usize;
            for j in 0..n {
                put(&mut a, r * n + j, 0.0);
            }
        }
        1 if n > 1 => {
            // duplicate row: near-singular pivot
            for j in 0..n {
                let x = at(&a, j);
                put(&mut a, n + j, x);
            }
        }
        _ => {}
    }
    let small = |rng: &mut Rng, s: f64| -> Vec<f64> {
        (0..n)
            .map(|_| {
                if rng.chance(0.4) {
                    0.0
                } else {
                    coef(rng, s, nice)
                }
            })
            .collect()
    };
    let c = small(rng, nl);
    let d = small(rng, nl);
    let e = if rng.chance(0.25) {
        small(rng, 1.0)
    } else {
        vec![0.0; n]
    };
    let x_star: Vec<f64> = (0..n).map(|_| rng.uniform(-2.0, 2.0)).collect();
    let b = if well_posed || rng.chance(0.5) {
        eval_f(n, &a, &c, &d, &e, &x_star)
    } else {
        (0..n).map(|_| coef(rng, 5.0, nice)).collect()
    };
    let spread = match rng.below(if nice { 5 } else { 6 }) {
        0 => 0.0,
        1 => 1e-9,
        2 => 1e-3,
        3 => 0.3,
        4 => 2.0,
        _ => 20.0,
    };
    let mut guess: Vec<f64> = x_star
        .iter()
        .map(|&x| x + rng.uniform(-spread, spread))
        .collect();
    if !nice && rng.chance(0.01) {
        put(&mut guess, 0, f64::NAN);
    }

    let solve = rng.chance(0.85);
    let jac_mode = match rng.below(10) {
        0..=5 => 0,
        6..=8 => 1,
        _ => 2,
    };
    let jac_scale = match if nice { 99 } else { rng.below(12) } {
        0 => 0.5,
        1 => 2.0,
        2 => -1.0,
        3 => 0.1,
        4 => 0.0,
        5 => 1e7,
        6 => 1e10,
        _ => 1.0,
    };
    let use_intervals = rng.chance(0.4);
    let ivl: Vec<Ivl> = if use_intervals || jac_mode == 2 || rng.chance(0.2) {
        (0..n)
            .map(|i| {
                let center = if rng.chance(0.5) {
                    at(&x_star, i)
                } else {
                    at(&guess, i)
                };
                let (lo, hi) = match rng.below(8) {
                    0 => (center, center),              // degenerate
                    1 => (center + 0.5, center + 1.0),  // solution outside
                    2 => (center - 1.0, center - 0.25), // solution outside
                    3 => (center + 1.0, center - 1.0),  // inverted (swapped)
                    _ => {
                        let w = rng.uniform(0.01, 3.0);
                        (center - w * rng.unit(), center + w * rng.unit())
                    }
                };
                Ivl {
                    lo: num(lo),
                    hi: num(hi),
                    unbounded: rng.chance(0.15),
                    periodic: nice && (hi - lo).abs() >= 0.5 && rng.chance(0.3),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let fail_calls: Vec<i64> = if rng.chance(0.2) {
        (0..=rng.below(4)).map(|_| rng.range_i64(0, 30)).collect()
    } else {
        Vec::new()
    };
    let fail_radius = if rng.chance(0.1) {
        rng.uniform(0.5, 5.0)
    } else {
        0.0
    };
    let found_tol = if rng.chance(0.15) {
        10f64.powi(-(rng.range_i64(3, 12) as i32))
    } else {
        0.0
    };
    let (size_scale, scaled_zero) = if rng.chance(0.2) {
        (
            rng.uniform(0.0, 10.0),
            10f64.powi(-(rng.range_i64(0, 14) as i32)),
        )
    } else {
        (0.0, 1.0)
    };
    let desired = match rng.below(6) {
        0 => Some(EFF_ZERO_SQ),
        1 => Some(EFF_ZERO / 10.0),
        2 => Some(10f64.powi(-(rng.range_i64(1, 14) as i32))),
        _ => None,
    };
    let acceptable = match rng.below(7) {
        0 => EFF_ZERO,
        1 => EFF_ZERO_SQRT * 100.0,
        2 => 1.0e-12,
        3 => 10f64.powi(-(rng.range_i64(1, 10) as i32)),
        4 => 1.0,
        _ => EFF_ZERO_SQRT,
    };
    let max_iter = match rng.below(8) {
        0 => Some(0),
        1 => Some(rng.range_i64(1, 5)),
        2 => Some(20),
        3 => Some(30),
        _ => None,
    };
    let handler = match rng.below(5) {
        0 => Some("total".to_string()),
        1 => Some("all".to_string()),
        2 => Some("ignore".to_string()),
        _ => None,
    };
    let max_hits = if rng.chance(0.5) {
        Some(rng.range_i64(0, 8))
    } else {
        None
    };
    let has_intervals = rng.chance(0.1);
    Inputs {
        op: if solve { "solve" } else { "jacobian" }.to_string(),
        n,
        a: a.into_iter().map(num).collect(),
        b: b.into_iter().map(num).collect(),
        c: c.into_iter().map(num).collect(),
        d: d.into_iter().map(num).collect(),
        e: e.into_iter().map(num).collect(),
        guess: guess.into_iter().map(num).collect(),
        jac_scale,
        fail_radius,
        found_tol,
        size_scale,
        scaled_zero,
        desired: desired.map(num),
        acceptable: num(acceptable),
        jac_mode,
        has_intervals,
        set_intervals_ok: has_intervals && rng.chance(0.8),
        max_iter,
        step_cutting: match rng.below(3) {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        },
        handler,
        max_hits,
        use_intervals: use_intervals && !ivl.is_empty(),
        use_periodic: use_intervals && rng.chance(0.5),
        ivl,
        fail_calls,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.local_solve_nd"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&parse(&case.parse()?)?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&parse(&case.parse()?)?)
    }
}
