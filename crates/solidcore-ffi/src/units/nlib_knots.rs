//! FFI wrappers for unit `nlib.knots`.
//!
//! Shim: `harness/cpp/shim/nlib_knots.cpp`.
//! Upstream: `source/SMLib/src/NL_BasisBasic.cpp`.
//!
//! | Rust                       | shim                                | upstream                                   |
//! |----------------------------|-------------------------------------|--------------------------------------------|
//! | [`roundtrip`]              | `sc_nlib_knots_roundtrip`           | `N_KnotVectorFromRealArray`, `N_SetKnotIndex`, `N_KnotVectorGetKnots` |
//! | [`param_checks`]           | `sc_nlib_knots_param_checks`        | `N_KnotVectorIsParamOutOfBounds`, `N_KnotVectorIsEndParam` |
//! | [`is_valid`]               | `sc_nlib_knots_is_valid`            | `N_KnotVectorIsValid`                      |
//! | [`span_count`]             | `sc_nlib_knots_span_count`          | `N_BasisGetSpanCount`                      |
//! | [`find_span`]              | `sc_nlib_knots_find_span`           | `N_BasisFindSpan`                          |
//! | [`find_span_and_mult`]     | `sc_nlib_knots_find_span_and_mult`  | `N_BasisFindSpanAndMult`                   |
//! | [`basis_eval`]             | `sc_nlib_knots_basis_eval`          | `N_BasisEval`                              |
//! | [`basis_i_eval`]           | `sc_nlib_knots_basis_i_eval`        | `N_BasisIEval`                             |
//! | [`basis_i_derivs`]         | `sc_nlib_knots_basis_i_derivs`      | `N_BasisIDerivs`                           |
//!
//! Errors: `FfiError::Status(code)` carries the NLib error number (`NL_ERROR.eno`)
//! when the upstream routine returned its error flag.
//!
//! Argument domain (checked here, before any call): at least one knot; for the
//! span and basis routines `0 <= degree <= MAX_SPAN_DEGREE`, flag
//! `NL_LEFT = 1` or `NL_RIGHT = 2`; `basis_eval` also needs
//! `degree <= NL_MAXDEG + 1` (upstream's stack scratch arrays).
//! Span/full-basis routines additionally require `2*degree + 2` knots.
//! Deviations: bounded degrees/derivative orders protect allocation sizes;
//! single-basis routines accept any nonempty vector, leaving parameter errors
//! before index errors to upstream (returned unchanged as NLib statuses).

use crate::{check, FfiError};

/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: i32 = 17;
/// Largest degree accepted by the span/basis wrappers (mirrors the shim).
pub const MAX_SPAN_DEGREE: i32 = 1000;
/// Largest derivative order accepted by [`basis_i_derivs`].
pub const MAX_DER: i64 = 1000;

mod sys {
    extern "C" {
        pub fn sc_nlib_knots_roundtrip(
            buf: *const f64,
            buf_len: i64,
            m: i64,
            new_m: i64,
            out_m: *mut i64,
            out_knots: *mut f64,
            cap: i64,
        ) -> i32;
        pub fn sc_nlib_knots_param_checks(
            knots: *const f64,
            n_knots: i64,
            u: f64,
            out_flags: *mut i32,
        ) -> i32;
        pub fn sc_nlib_knots_is_valid(knots: *const f64, n_knots: i64, degree: i32) -> i32;
        pub fn sc_nlib_knots_span_count(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            out_count: *mut i64,
        ) -> i32;
        pub fn sc_nlib_knots_find_span(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            u: f64,
            flag: i32,
            out_span: *mut i64,
        ) -> i32;
        pub fn sc_nlib_knots_find_span_and_mult(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            u: f64,
            flag: i32,
            out_span: *mut i64,
            out_mult: *mut i64,
        ) -> i32;
        pub fn sc_nlib_knots_basis_eval(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            u: f64,
            flag: i32,
            out_basis: *mut f64,
            cap: i64,
            out_span: *mut i64,
        ) -> i32;
        pub fn sc_nlib_knots_basis_i_eval(
            knots: *const f64,
            n_knots: i64,
            index: i64,
            degree: i32,
            u: f64,
            flag: i32,
            out_value: *mut f64,
        ) -> i32;
        pub fn sc_nlib_knots_basis_i_derivs(
            knots: *const f64,
            n_knots: i64,
            index: i64,
            degree: i32,
            u: f64,
            flag: i32,
            der: i64,
            out_derivs: *mut f64,
            cap: i64,
        ) -> i32;
    }
}

fn bad(msg: String) -> FfiError {
    FfiError::InvalidArgument(msg)
}

/// Number of knots as `i64`, requiring at least one.
fn knot_count(knots: &[f64]) -> Result<i64, FfiError> {
    if knots.is_empty() {
        return Err(bad("empty knot vector".into()));
    }
    i64::try_from(knots.len()).map_err(|_| bad("too many knots".into()))
}

/// Checks the span/basis domain; returns `(n_knots, p)`.
fn basis_domain(knots: &[f64], degree: i32, flag: i32) -> Result<(i64, usize), FfiError> {
    let (n, p) = single_basis_domain(knots, degree, flag)?;
    if knots.len() < 2 * p + 2 {
        return Err(bad(format!("{} knots for degree {degree}", knots.len())));
    }
    Ok((n, p))
}

/// Checks single-basis well-formedness, without the span routines' length bound.
fn single_basis_domain(knots: &[f64], degree: i32, flag: i32) -> Result<(i64, usize), FfiError> {
    let n = knot_count(knots)?;
    if flag != 1 && flag != 2 {
        return Err(bad(format!("flag {flag}")));
    }
    if !(0..=MAX_SPAN_DEGREE).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    let p = usize::try_from(degree).map_err(|_| bad(format!("degree {degree}")))?;
    Ok((n, p))
}

/// Builds a knot vector over `buf` with highest index `m`, sets the highest index to
/// `new_m` and reads it back: returns `(m, U[0..=m])` as upstream reports them.
pub fn roundtrip(buf: &[f64], m: i64, new_m: i64) -> Result<(i64, Vec<f64>), FfiError> {
    let len = knot_count(buf)?;
    if !(0..len).contains(&m) || !(0..len).contains(&new_m) {
        return Err(bad(format!("m {m} / new_m {new_m} for {len} entries")));
    }
    let cap = usize::try_from(new_m + 1).map_err(|_| bad("new_m".into()))?;
    let mut out = vec![0.0; cap];
    let mut out_m = -1i64;
    // SAFETY: `buf` is valid for `len` reads and upstream reads at most U[0..=new_m]
    // with new_m < len; `out` has new_m+1 writable slots; `out_m` is a valid i64.
    let st = unsafe {
        sys::sc_nlib_knots_roundtrip(
            buf.as_ptr(),
            len,
            m,
            new_m,
            &raw mut out_m,
            out.as_mut_ptr(),
            new_m + 1,
        )
    };
    check(st)?;
    Ok((out_m, out))
}

/// Upstream `(N_KnotVectorIsParamOutOfBounds, N_KnotVectorIsEndParam)` for `u`.
pub fn param_checks(knots: &[f64], u: f64) -> Result<(bool, bool), FfiError> {
    let n = knot_count(knots)?;
    let mut flags = [0i32; 2];
    // SAFETY: `knots` valid for `n` reads (upstream reads U[0], U[m]); `flags` has
    // two writable i32 slots.
    let st = unsafe { sys::sc_nlib_knots_param_checks(knots.as_ptr(), n, u, flags.as_mut_ptr()) };
    check(st)?;
    Ok((flags[0] != 0, flags[1] != 0))
}

/// Upstream `N_KnotVectorIsValid(knots, degree)`: `Ok(())` when valid,
/// `Err(Status(4))` (`NL_KNT_ERR`) otherwise.
pub fn is_valid(knots: &[f64], degree: i32) -> Result<(), FfiError> {
    let n = knot_count(knots)?;
    if !(0..=i32::from(i16::MAX)).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    // SAFETY: `knots` valid for `n` reads; upstream only reads U[0..=m] (its loops are
    // bounded by m and by indices derived from the first loop, which stops at i <= m).
    let st = unsafe { sys::sc_nlib_knots_is_valid(knots.as_ptr(), n, degree) };
    check(st)
}

/// Upstream `N_BasisGetSpanCount(knots, degree)`.
pub fn span_count(knots: &[f64], degree: i32) -> Result<i64, FfiError> {
    let n = knot_count(knots)?;
    if !(0..=i32::from(i16::MAX)).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    let mut count = -1i64;
    // SAFETY: `knots` valid for `n` reads; upstream reads U[i], U[i+1] for
    // p <= i < m-p only; `count` is a valid i64.
    let st = unsafe { sys::sc_nlib_knots_span_count(knots.as_ptr(), n, degree, &raw mut count) };
    check(st)?;
    Ok(count)
}

/// Upstream `N_BasisFindSpan(knots, degree, u, flag)`.
pub fn find_span(knots: &[f64], degree: i32, u: f64, flag: i32) -> Result<i64, FfiError> {
    let (n, _) = basis_domain(knots, degree, flag)?;
    let mut span = -1i64;
    // SAFETY: domain checked (n >= 2p+2, valid flag), so upstream indexes within
    // U[0..=m]; `span` is a valid i64.
    let st =
        unsafe { sys::sc_nlib_knots_find_span(knots.as_ptr(), n, degree, u, flag, &raw mut span) };
    check(st)?;
    Ok(span)
}

/// Upstream `N_BasisFindSpanAndMult(knots, degree, u, flag)`: `(span, multiplicity)`.
pub fn find_span_and_mult(
    knots: &[f64],
    degree: i32,
    u: f64,
    flag: i32,
) -> Result<(i64, i64), FfiError> {
    let (n, _) = basis_domain(knots, degree, flag)?;
    let mut span = -1i64;
    let mut mult = -1i64;
    // SAFETY: as `find_span`; the multiplicity loops stop at k < 0 / k > m.
    let st = unsafe {
        sys::sc_nlib_knots_find_span_and_mult(
            knots.as_ptr(),
            n,
            degree,
            u,
            flag,
            &raw mut span,
            &raw mut mult,
        )
    };
    check(st)?;
    Ok((span, mult))
}

/// Upstream `N_BasisEval(knots, degree, u, flag)`: `(span, N[0..=p])`.
pub fn basis_eval(
    knots: &[f64],
    degree: i32,
    u: f64,
    flag: i32,
) -> Result<(i64, Vec<f64>), FfiError> {
    let (n, p) = basis_domain(knots, degree, flag)?;
    if degree > MAX_DEGREE + 1 {
        return Err(bad(format!("degree {degree} > NL_MAXDEG + 1")));
    }
    let mut basis = vec![0.0; p + 1];
    let mut span = -1i64;
    // SAFETY: domain checked (n >= 2p+2, p <= NL_MAXDEG+1 for upstream's stack
    // arrays, valid flag); `basis` has p+1 writable slots (upstream writes N[0..=p]).
    let st = unsafe {
        sys::sc_nlib_knots_basis_eval(
            knots.as_ptr(),
            n,
            degree,
            u,
            flag,
            basis.as_mut_ptr(),
            basis.len() as i64,
            &raw mut span,
        )
    };
    check(st)?;
    Ok((span, basis))
}

/// Upstream `N_BasisIEval(knots, index, degree, u, flag)`.
pub fn basis_i_eval(
    knots: &[f64],
    index: i64,
    degree: i32,
    u: f64,
    flag: i32,
) -> Result<f64, FfiError> {
    let (n, _) = single_basis_domain(knots, degree, flag)?;
    if index < 0 {
        return Err(bad(format!("index {index}")));
    }
    let mut value = 0.0;
    // SAFETY: nonempty vector, bounded degree and valid flag; upstream checks
    // the parameter first, then rejects index > m-p-1 (NL_IND_ERR) before
    // reading U[index..=index+p+1]; `value` is a valid f64.
    let st = unsafe {
        sys::sc_nlib_knots_basis_i_eval(knots.as_ptr(), n, index, degree, u, flag, &raw mut value)
    };
    check(st)?;
    Ok(value)
}

/// Upstream `N_BasisIDerivs(knots, index, degree, u, flag, der)`: `der + 1` values.
pub fn basis_i_derivs(
    knots: &[f64],
    index: i64,
    degree: i32,
    u: f64,
    flag: i32,
    der: i64,
) -> Result<Vec<f64>, FfiError> {
    let (n, _) = single_basis_domain(knots, degree, flag)?;
    if index < 0 {
        return Err(bad(format!("index {index}")));
    }
    if !(0..=MAX_DER).contains(&der) {
        return Err(bad(format!("der {der}")));
    }
    let len = usize::try_from(der + 1).map_err(|_| bad("der".into()))?;
    let mut out = vec![0.0; len];
    // SAFETY: nonempty vector, bounded degree and valid flag; upstream checks
    // the parameter first, then rejects index > m-p-1 before indexing and
    // writes ND[0..=der]; `out` has der+1 writable slots.
    let st = unsafe {
        sys::sc_nlib_knots_basis_i_derivs(
            knots.as_ptr(),
            n,
            index,
            degree,
            u,
            flag,
            der,
            out.as_mut_ptr(),
            der + 1,
        )
    };
    check(st)?;
    Ok(out)
}
