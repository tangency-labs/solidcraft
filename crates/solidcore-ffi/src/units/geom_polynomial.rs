//! FFI wrappers for unit `geom.polynomial`.
//!
//! Shim: `harness/cpp/shim/geom_polynomial.cpp`.
//! Upstream: `source/SMLib/src/SmPolynomial.cpp`.
//!
//! | Rust      | shim                        | upstream                                         |
//! |-----------|-----------------------------|--------------------------------------------------|
//! | [`solve`] | `sc_geom_polynomial_solve`  | `SmPolynomial::Solve{Quadratic,Cubic,Quartic}Eqn` |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_polynomial_solve(
            degree: i32,
            c: *const f64,
            tol: f64,
            out: *mut f64,
            out_n: *mut i64,
            out_status: *mut i32,
        ) -> i32;
    }
}

/// Result of [`solve`].
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// Upstream `SmStatus` (`SM_SUCCESS` = 1000).
    pub status: i32,
    /// The real roots, in upstream order.
    pub roots: Vec<f64>,
}

/// Calls upstream `SmPolynomial::SolveQuadraticEqn` (3 coefficients),
/// `SolveCubicEqn` (4) or `SolveQuarticEqn` (5) on `coefficients` (ascending
/// powers) with zero tolerance `tol`.
///
/// Quartics with `tol > 1` are rejected unless `|coefficients[4]| < tol`, when
/// upstream falls back to the lower-degree solver. Otherwise the rejected inputs
/// may reach an uninitialised resolvent-cubic root read (undefined behaviour):
/// upstream also has further defined fallbacks on the resolvent input (e.g.
/// `fabs(r) < tol`), so the guard stays conservative. Remaining over-rejection
/// predates this guard; equality and NaN leading coefficients do not take the
/// fallback and remain rejected.
pub fn solve(coefficients: &[f64], tol: f64) -> Result<Solution, FfiError> {
    let degree = match coefficients.len() {
        3 => 2,
        4 => 3,
        5 => 4,
        n => return Err(FfiError::InvalidArgument(format!("{n} coefficients"))),
    };
    // Resolvent reachable ⟺ NOT (|c4| < tol): exactly `|c4| >= tol || |c4| is
    // NaN` (a negated `!(a < b)` would trip `neg_cmp_op_on_partial_ord` while
    // changing nothing; NaN reaches the resolvent upstream too, so it stays
    // rejected). `get` avoids the denied direct indexing.
    if degree == 4
        && tol > 1.0
        && coefficients
            .get(4)
            .copied()
            .is_some_and(|a| a.abs() >= tol || a.abs().is_nan())
    {
        return Err(FfiError::InvalidArgument(format!("quartic tol {tol}")));
    }
    let mut out = [0.0f64; 4];
    let mut n = 0i64;
    let mut status = 0i32;
    // SAFETY: `coefficients` holds degree+1 readable f64 (checked above); `out`
    // has the 4 writable slots the shim requires; `n` and `status` are valid.
    // Nothing is retained after the call.
    let st = unsafe {
        sys::sc_geom_polynomial_solve(
            degree,
            coefficients.as_ptr(),
            tol,
            out.as_mut_ptr(),
            &raw mut n,
            &raw mut status,
        )
    };
    check(st)?;
    let len = usize::try_from(n).map_err(|_| FfiError::Status(st))?;
    let roots = out.get(..len).ok_or(FfiError::Status(st))?.to_vec();
    Ok(Solution { status, roots })
}
