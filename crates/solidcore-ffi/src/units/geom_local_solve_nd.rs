//! FFI wrappers for unit `geom.local_solve_nd`.
//!
//! Shim: `harness/cpp/shim/geom_local_solve_nd.cpp`.
//! Upstream: `source/SMLib/inc/SmLocalSolveNd.h`, `source/SMLib/src/SmLocalSolveNd.cpp`
//! (`SmLocalSolveNd::SolveIt`, `SmEvalNFunctionsObject::ComputeJacobian`), driven by the
//! shim's test equation set `ScTestENFO`.
//!
//! | Rust    | shim                          | upstream                                  |
//! |---------|-------------------------------|-------------------------------------------|
//! | [`run`] | `sc_geom_local_solve_nd_run`  | `SolveIt` ([`Op::Solve`]), `ComputeJacobian` ([`Op::Jacobian`]) |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_local_solve_nd_run(
            op: i32,
            n: i64,
            coef: *const f64,
            cfg: *const f64,
            icfg: *const i64,
            guess: *const f64,
            ivl: *const f64,
            ivl_flags: *const i64,
            fail_calls: *const i64,
            n_fail: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
            ints: *mut i64,
            ints_cap: i64,
            ints_len: *mut i64,
            status: *mut i32,
        ) -> i32;
    }
}

/// Largest problem size the shim accepts.
pub const MAX_N: usize = 8;
/// Largest number of failing call indices.
pub const MAX_FAIL: usize = 64;
/// Largest iteration limit the shim accepts.
pub const MAX_ITER: i64 = 1000;

/// Shim `SC_ERR_BUFFER`.
const SC_ERR_BUFFER: i32 = -1001;

/// Operation of [`run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `SmLocalSolveNd::SolveIt`: values = solution, trace; ints = `[found, reason,
    /// iteration, calls, set_intervals_calls, flags...]`.
    Solve = 0,
    /// `Evaluate` then `ComputeJacobian`: values = Jacobian (row-major), x after,
    /// trace; ints = `[calls, flags...]`.
    Jacobian = 1,
}

/// Inputs of [`run`] (layouts as in the shim).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Problem<'a> {
    /// Problem size `n`.
    pub n: usize,
    /// `a` (row-major `n x n`), `b`, `c`, `d`, `e`: `n*n + 4n` values.
    pub coef: &'a [f64],
    /// `[jac_scale, fail_radius, found_tol, size_scale, scaled_zero, desired,
    /// acceptable]`.
    pub cfg: [f64; 7],
    /// `[jac_mode, has_intervals, set_intervals_ok, set_desired, max_iter,
    /// step_cutting, handler, max_hits, use_intervals, use_periodic]`.
    pub icfg: [i64; 10],
    /// Guess / evaluation point, `n` values.
    pub guess: &'a [f64],
    /// Interval `lo, hi` per parameter, `2n` values.
    pub ivl: &'a [f64],
    /// Per parameter: bit 0 unbounded, bit 1 periodic.
    pub ivl_flags: &'a [i64],
    /// `Evaluate` call indices that fail.
    pub fail_calls: &'a [i64],
}

/// Output of [`run`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunResult {
    /// Upstream `SmStatus` (`0` = `SM_SUCCESS`).
    pub status: i32,
    /// Float results.
    pub values: Vec<f64>,
    /// Integer results.
    pub ints: Vec<i64>,
}

/// Runs `op` on the shim's test equation set in upstream C++.
pub fn run(op: Op, p: &Problem<'_>) -> Result<RunResult, FfiError> {
    let n = p.n;
    if n == 0 || n > MAX_N {
        return Err(FfiError::InvalidArgument(format!("n = {n}")));
    }
    if p.coef.len() != n * n + 4 * n
        || p.guess.len() != n
        || p.ivl.len() != 2 * n
        || p.ivl_flags.len() != n
    {
        return Err(FfiError::InvalidArgument("array lengths".into()));
    }
    if p.fail_calls.len() > MAX_FAIL {
        return Err(FfiError::InvalidArgument("too many failing calls".into()));
    }
    let [jac_mode, _, _, _, max_iter, _, handler, _, _, _] = p.icfg;
    if !(0..=2).contains(&jac_mode) || max_iter > MAX_ITER || handler > 2 {
        return Err(FfiError::InvalidArgument(format!("icfg {:?}", p.icfg)));
    }
    let mut cap = 4096usize;
    let mut ints_cap = 1024usize;
    for _ in 0..2 {
        let mut out = vec![0.0f64; cap];
        let mut ints = vec![0i64; ints_cap];
        let (mut out_len, mut ints_len, mut status) = (0i64, 0i64, 0i32);
        // SAFETY: every input pointer is valid for the length checked above (coef
        // n*n+4n, cfg 7, icfg 10, guess n, ivl 2n, ivl_flags n, fail_calls n_fail);
        // `out` / `ints` are valid for their lengths, passed as capacities; the
        // scalars are valid for writes. The shim retains nothing.
        let st = unsafe {
            sys::sc_geom_local_solve_nd_run(
                op as i32,
                n as i64,
                p.coef.as_ptr(),
                p.cfg.as_ptr(),
                p.icfg.as_ptr(),
                p.guess.as_ptr(),
                p.ivl.as_ptr(),
                p.ivl_flags.as_ptr(),
                p.fail_calls.as_ptr(),
                p.fail_calls.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut out_len,
                ints.as_mut_ptr(),
                ints.len() as i64,
                &raw mut ints_len,
                &raw mut status,
            )
        };
        let n_out = usize::try_from(out_len)
            .map_err(|_| FfiError::InvalidArgument(format!("shim returned {out_len} values")))?;
        let n_ints = usize::try_from(ints_len)
            .map_err(|_| FfiError::InvalidArgument(format!("shim returned {ints_len} ints")))?;
        if st == SC_ERR_BUFFER && (n_out > cap || n_ints > ints_cap) {
            cap = cap.max(n_out);
            ints_cap = ints_cap.max(n_ints);
            continue;
        }
        check(st)?;
        if n_out > cap || n_ints > ints_cap {
            return Err(FfiError::InvalidArgument("shim overran its buffers".into()));
        }
        out.truncate(n_out);
        ints.truncate(n_ints);
        return Ok(RunResult {
            status,
            values: out,
            ints,
        });
    }
    Err(FfiError::Status(SC_ERR_BUFFER))
}
