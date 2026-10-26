//! FFI wrappers for unit `geom.local_solve1d`.
//!
//! Shim: `harness/cpp/shim/geom_local_solve1d.cpp`.
//! Upstream: `source/SMLib/inc/SmLocalSolve1d.h`, `source/SMLib/src/SmLocalSolve1d.cpp`
//! (`SmLocalSolve1d`, `SmIterQueue1d`, `SmEvalFunctionObject`).
//!
//! | Rust    | shim                        | upstream                                 |
//! |---------|-----------------------------|------------------------------------------|
//! | [`run`] | `sc_geom_local_solve1d_run` | `SolveIt`, `SolveByBisection`, `SolveByBrent`, `SmIterQueue1d` per [`Op`] |
//!
//! The solved function is the shim's synthetic test function described by
//! [`FnParams`]; the output layouts are documented on the shim.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_local_solve1d_run(
            op: i32,
            fn_params: *const f64,
            s: *const f64,
            n_s: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Number of test-function parameters.
pub const N_FN_PARAMS: usize = 14;

/// Test-function parameters `[kind, p0..p7, found_tol, fail_lo, fail_hi, fail_first,
/// failure_write]` (`0`: keep outputs, `1`: write F, `2`: zero F and F'; see the
/// shim's `ScTestFn`).
pub type FnParams = [f64; N_FN_PARAMS];

/// Valid function kinds.
const KINDS: [f64; 8] = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 99.0];

/// Operation run by [`run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `SolveIt` once per guess on one solver:
    /// `s = [use_interval, lo, hi, periodic, algorithm, has_desired, desired,
    /// acceptable, guess...]`.
    SolveIt = 0,
    /// `SolveByBisection`: `s = [level, lo_t, hi_t, lo_f, hi_f]`.
    Bisection = 1,
    /// `SolveByBrent`: `s = [ax, bx, cx, tol, maximize]`.
    Brent = 2,
    /// `SmIterQueue1d`: `s = [size, step, prev_step...]`.
    Queue = 3,
}

/// Maximum operands (guesses / queue entries).
const MAX_S: usize = 4096;

/// Runs `op` in upstream C++ with test function `fn_params` and operands `s`;
/// returns the shim's flat output.
pub fn run(op: Op, fn_params: &FnParams, s: &[f64]) -> Result<Vec<f64>, FfiError> {
    let bad = |m: &str| Err(FfiError::InvalidArgument(m.into()));
    // Operand `i` (NaN when missing, which fails every range check below).
    let at = |i: usize| s.get(i).copied().unwrap_or(f64::NAN);
    let whole_in = |v: f64, lo: f64, hi: f64| (lo..=hi).contains(&v) && v.fract() == 0.0;
    if !KINDS.contains(&fn_params[0]) {
        return bad("fn kind");
    }
    if !fn_params[12].is_finite() {
        return bad("fail_first");
    }
    if s.len() > MAX_S {
        return bad("too many operands");
    }
    match op {
        Op::SolveIt => {
            if s.len() < 8 {
                return bad("solve_it operands");
            }
            if !whole_in(at(4), 0.0, 3.0) {
                return bad("algorithm");
            }
        }
        Op::Bisection => {
            // (ULONG) conversion and recursion depth.
            if s.len() < 5 || !whole_in(at(0), 0.0, 10_000.0) {
                return bad("bisection operands");
            }
        }
        Op::Brent => {
            if s.len() < 5 {
                return bad("brent operands");
            }
        }
        Op::Queue => {
            if s.len() < 2 || !whole_in(at(0), 1.0, 1024.0) || !whole_in(at(1), 0.0, 1024.0) {
                return bad("queue operands");
            }
        }
    }
    call_with_buffer(8192, |buf, len| {
        // SAFETY: fn_params has N_FN_PARAMS readable values; s is valid for s.len()
        // reads; buf is valid for buf.len() writes; len is a valid i64.
        unsafe {
            sys::sc_geom_local_solve1d_run(
                op as i32,
                fn_params.as_ptr(),
                s.as_ptr(),
                s.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
