//! FFI wrappers for unit `harness.selftest`.
//!
//! Shim: `harness/cpp/shim/harness_selftest.cpp`.
//! Upstream: `source/SMLib/src/NL_BasisBasic.cpp` (`N_BasisEval`).
//!
//! | Rust          | shim                              | upstream      |
//! |---------------|-----------------------------------|---------------|
//! | [`ping`]      | `sc_harness_selftest_ping`        | (none)        |
//! | [`basis_eval`]| `sc_harness_selftest_basis_eval`  | `N_BasisEval` |
//! | [`reverse`]   | `sc_harness_selftest_reverse`     | (none)        |

use crate::{call_with_buffer, check, FfiError};

/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: i32 = 17;

mod sys {
    extern "C" {
        pub fn sc_harness_selftest_ping(x: f64, out: *mut f64) -> i32;
        #[cfg(test)]
        pub fn sc_harness_selftest_native_guard(mode: i32, available: *mut i64) -> i32;
        pub fn sc_harness_selftest_basis_eval(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            u: f64,
            flag: i32,
            out_basis: *mut f64,
            cap: i64,
            out_span: *mut i64,
        ) -> i32;
        pub fn sc_harness_selftest_reverse(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Trivial shim round-trip: returns `2x + 1` computed in C++.
pub fn ping(x: f64) -> Result<f64, FfiError> {
    let mut out = 0.0;
    // SAFETY: `out` is a valid, writable f64 for the duration of the call.
    let st = unsafe { sys::sc_harness_selftest_ping(x, &raw mut out) };
    check(st)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::sys;

    fn guard_probe(mode: i32) -> (i32, i64) {
        let mut available = -1;
        // SAFETY: the probe only writes this live i64 and retains no pointers.
        let status = unsafe { sys::sc_harness_selftest_native_guard(mode, &raw mut available) };
        (status, available)
    }

    #[test]
    fn native_guard_excludes_other_threads_and_allows_nested_shims() {
        assert_eq!(guard_probe(0), (crate::SC_OK, 0));
        assert_eq!(guard_probe(2), (crate::SC_OK, 0));
    }

    #[test]
    fn native_guard_is_released_after_cpp_exception() {
        assert_eq!(guard_probe(1), (crate::SC_ERR_EXCEPTION, 0));
        // A different caller thread must be able to acquire it after unwinding.
        assert_eq!(
            std::thread::spawn(|| guard_probe(0)).join().unwrap(),
            (crate::SC_OK, 0)
        );
    }
}

/// Result of [`basis_eval`].
#[derive(Debug, Clone, PartialEq)]
pub struct BasisEval {
    /// Span index reported by upstream.
    pub span: i64,
    /// Non-vanishing basis functions `N[0..=p]`.
    pub basis: Vec<f64>,
}

/// Calls upstream `N_BasisEval(knots, degree, u, flag)`; `flag` is
/// `NL_LEFT = 1` or `NL_RIGHT = 2`.
pub fn basis_eval(knots: &[f64], degree: i32, u: f64, flag: i32) -> Result<BasisEval, FfiError> {
    // Upstream N_BasisEval ignores N_BasisFindSpan's error for other flags and
    // then reads an uninitialised span index (UB), so reject them here.
    if flag != 1 && flag != 2 {
        return Err(FfiError::InvalidArgument(format!("flag {flag}")));
    }
    if !(0..=MAX_DEGREE).contains(&degree) {
        return Err(FfiError::InvalidArgument(format!("degree {degree}")));
    }
    let p = degree as usize;
    if knots.len() < 2 * p + 2 {
        return Err(FfiError::InvalidArgument(format!(
            "{} knots for degree {degree}",
            knots.len()
        )));
    }
    let n_knots = i64::try_from(knots.len())
        .map_err(|_| FfiError::InvalidArgument("too many knots".into()))?;
    let mut basis = vec![0.0; p + 1];
    let mut span = -1i64;
    // SAFETY: `knots` is valid for `n_knots` reads (upstream only reads U[0..=m]
    // with m = n_knots-1 and indices bounded by m given n_knots >= 2p+2);
    // `basis` has p+1 writable slots (upstream writes N[0..=p]); `span` is a
    // valid i64. Upstream does not retain any pointer after returning.
    let st = unsafe {
        sys::sc_harness_selftest_basis_eval(
            knots.as_ptr(),
            n_knots,
            degree,
            u,
            flag,
            basis.as_mut_ptr(),
            basis.len() as i64,
            &raw mut span,
        )
    };
    check(st)?;
    Ok(BasisEval { span, basis })
}

/// Variable-length output probe: `input` reversed, computed in C++ and
/// returned through [`call_with_buffer`] (initial capacity 1 forces a retry).
pub fn reverse(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let n = i64::try_from(input.len())
        .map_err(|_| FfiError::InvalidArgument("input too long".into()))?;
    call_with_buffer(1, |buf, len| {
        // SAFETY: `input` is valid for `n` reads, `buf` for `buf.len()` writes,
        // `len` is a valid i64; nothing is retained after the call.
        unsafe {
            sys::sc_harness_selftest_reverse(
                input.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
