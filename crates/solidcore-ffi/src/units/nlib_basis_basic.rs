//! FFI wrappers for unit `nlib.basis_basic`.
//!
//! Shim: `harness/cpp/shim/nlib_basis_basic.cpp`.
//! Upstream: `source/SMLib/src/NL_BasisBasic.cpp`.
//!
//! | Rust                   | shim                                      | upstream                          |
//! |------------------------|-------------------------------------------|-----------------------------------|
//! | [`global_max`]         | `sc_nlib_basis_basic_global_max`          | `N_BasisFindGlobalMax`            |
//! | [`span_maxima`]        | `sc_nlib_basis_basic_span_maxima`         | `N_BasisFindAllSpanMaxima`        |
//! | [`reparam`]            | `sc_nlib_basis_basic_reparam`             | `N_BasisReparam`                  |
//! | [`compatible`]         | `sc_nlib_basis_basic_compatible`          | `N_GetCompatibleKnotVectorToTol`, `N_GetCompatibleKnotArray` |
//! | [`derivs`]             | `sc_nlib_basis_basic_derivs`              | `N_BasisDerivs`                   |
//! | [`make_compatible`]    | `sc_nlib_basis_basic_make_compatible`     | `N_MakeKnotsCompatible`           |
//!
//! Errors: `FfiError::Status(code)` carries the NLib error number (`NL_ERROR.eno`)
//! when the upstream routine returned its error flag.
//!
//! Argument domain (checked here, before any call; inputs outside it would make
//! upstream read or write out of bounds, overflow an `int`, or loop forever):
//! - every knot finite;
//! - `global_max` / `span_maxima` / `derivs`: `0 <= degree <= 1000`, at least
//!   `2*degree + 2` knots (as `nlib.knots`); `global_max` / `span_maxima` also need the
//!   bracketing step `(U[i+p+1]-U[i])/(10p)` to advance the parameter all the way
//!   (else upstream loops forever); `derivs` needs every factor `p!/(p-k)!`,
//!   `k = 1..=min(p,der)`, to fit an `i32`, and a valid flag;
//! - `reparam`: at least `degree + 1` knots;
//! - `compatible`: `tol > 0` (unless the default-tolerance routine is used), every vector
//!   non-empty with at least `n_vec - 1` knots (so the new knots fit what upstream
//!   allocates);
//! - `make_compatible`: at least `degree + 1` knots, `|knot| <= 1e100`, and a positive
//!   range `U[m] - U[0] >= 1e-100` per vector (so the rescale stays finite).

use crate::{check, FfiError};

/// Largest degree accepted by the basis wrappers (mirrors the shim).
pub const MAX_DEGREE: i32 = 1000;
/// Largest derivative order accepted by [`derivs`].
pub const MAX_DER: i64 = 1000;
/// Trailing padding the shim adds to `make_compatible` knot buffers.
const MC_PAD: usize = 8;

mod sys {
    extern "C" {
        pub fn sc_nlib_basis_basic_global_max(
            knots: *const f64,
            n_knots: i64,
            index: i64,
            degree: i32,
            tol: f64,
            out: *mut f64,
        ) -> i32;
        pub fn sc_nlib_basis_basic_span_maxima(
            knots: *const f64,
            n_knots: i64,
            index: i64,
            degree: i32,
            tol: f64,
            out_min: *mut f64,
            out_max: *mut f64,
            cap: i64,
            out_u: *mut f64,
        ) -> i32;
        pub fn sc_nlib_basis_basic_reparam(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            ul: f64,
            ur: f64,
            out_knots: *mut f64,
        ) -> i32;
        pub fn sc_nlib_basis_basic_compatible(
            flat: *const f64,
            lens: *const i64,
            n_vec: i64,
            tol: f64,
            use_array: i32,
            out_knots: *mut f64,
            out_new: *mut f64,
            new_cap: i64,
            out_new_lens: *mut i64,
        ) -> i32;
        pub fn sc_nlib_basis_basic_derivs(
            knots: *const f64,
            n_knots: i64,
            degree: i32,
            u: f64,
            flag: i32,
            der: i64,
            out_nd: *mut f64,
            cap: i64,
            out_span: *mut i64,
        ) -> i32;
        pub fn sc_nlib_basis_basic_make_compatible(
            r: *const f64,
            n_r: i64,
            s: *const f64,
            n_s: i64,
            p: i32,
            q: i32,
            out_r: *mut f64,
            out_s: *mut f64,
            out_xr: *mut f64,
            out_nxr: *mut i64,
            out_xs: *mut f64,
            out_nxs: *mut i64,
            cap: i64,
        ) -> i32;
    }
}

fn bad(msg: String) -> FfiError {
    FfiError::InvalidArgument(msg)
}

fn len_i64(n: usize) -> Result<i64, FfiError> {
    i64::try_from(n).map_err(|_| bad("length overflow".into()))
}

fn finite(knots: &[f64]) -> Result<(), FfiError> {
    if knots.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(bad("non-finite knot".into()))
    }
}

/// Degree and length checks shared by the basis routines; returns `(n_knots, p)`.
fn basis_domain(knots: &[f64], degree: i32) -> Result<(i64, usize), FfiError> {
    if knots.is_empty() {
        return Err(bad("empty knot vector".into()));
    }
    finite(knots)?;
    if !(0..=MAX_DEGREE).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    let p = usize::try_from(degree).map_err(|_| bad(format!("degree {degree}")))?;
    if knots.len() < 2 * p + 2 {
        return Err(bad(format!("{} knots for degree {degree}", knots.len())));
    }
    Ok((len_i64(knots.len())?, p))
}

/// Rejects inputs on which the bracketing loop of `N_BasisFindGlobalMax` would stall
/// (step too small to change the parameter): replays that loop without the early
/// exit on a negative derivative.
fn global_max_domain(knots: &[f64], index: i64, p: usize) -> Result<(), FfiError> {
    if index < 0 {
        return Err(bad(format!("index {index}")));
    }
    let i = usize::try_from(index).map_err(|_| bad(format!("index {index}")))?;
    // Upstream returns NL_IND_ERR before the loop for i > m-p-1.
    let Some(n) = knots.len().checked_sub(p + 2) else {
        return Ok(());
    };
    if i > n || p == 1 {
        return Ok(());
    }
    let (Some(&ui), Some(&ue)) = (knots.get(i), knots.get(i + p + 1)) else {
        return Ok(());
    };
    let du = (ue - ui) / (p * 10) as f64;
    let mut ur = ui;
    let mut steps = 0usize;
    while ur < ue {
        let mut next = ur + du;
        if next > ue {
            next = ue;
        }
        if next == ur || steps > 1_000_000 {
            return Err(bad("bracketing step does not advance".into()));
        }
        ur = next;
        steps += 1;
    }
    Ok(())
}

/// Upstream `N_BasisFindGlobalMax(knots, index, degree, tol)`: `(max, u)`.
pub fn global_max(
    knots: &[f64],
    index: i64,
    degree: i32,
    tol: f64,
) -> Result<(f64, f64), FfiError> {
    let (n, p) = basis_domain(knots, degree)?;
    global_max_domain(knots, index, p)?;
    let mut out = [0.0f64; 2];
    // SAFETY: domain checked (n >= 2p+2, finite knots, loop advances); upstream
    // rejects index > m-p-1 before indexing; `out` has two writable slots.
    let st = unsafe {
        sys::sc_nlib_basis_basic_global_max(knots.as_ptr(), n, index, degree, tol, out.as_mut_ptr())
    };
    check(st)?;
    Ok((out[0], out[1]))
}

/// Upstream `N_BasisFindAllSpanMaxima(knots, index, degree, tol)`: `(min, max, u)`
/// with `degree + 1` entries each.
pub fn span_maxima(
    knots: &[f64],
    index: i64,
    degree: i32,
    tol: f64,
) -> Result<(Vec<f64>, Vec<f64>, f64), FfiError> {
    let (n, p) = basis_domain(knots, degree)?;
    global_max_domain(knots, index, p)?;
    let mut min = vec![0.0; p + 1];
    let mut max = vec![0.0; p + 1];
    let mut u = 0.0;
    // SAFETY: as `global_max`; upstream writes min/max[0..=p] (p+1 slots each).
    let st = unsafe {
        sys::sc_nlib_basis_basic_span_maxima(
            knots.as_ptr(),
            n,
            index,
            degree,
            tol,
            min.as_mut_ptr(),
            max.as_mut_ptr(),
            len_i64(p + 1)?,
            &raw mut u,
        )
    };
    check(st)?;
    Ok((min, max, u))
}

/// Upstream `N_BasisReparam(knots, degree, [ul, ur])`: the rescaled knots.
pub fn reparam(knots: &[f64], degree: i32, ul: f64, ur: f64) -> Result<Vec<f64>, FfiError> {
    if knots.is_empty() {
        return Err(bad("empty knot vector".into()));
    }
    if !(0..=i32::from(i16::MAX)).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    let p = usize::try_from(degree).map_err(|_| bad(format!("degree {degree}")))?;
    if knots.len() < p + 1 {
        return Err(bad(format!("{} knots for degree {degree}", knots.len())));
    }
    let mut out = vec![0.0; knots.len()];
    // SAFETY: knots.len() >= p+1, so upstream touches U[0..=m] only; `out` has
    // knots.len() writable slots.
    let st = unsafe {
        sys::sc_nlib_basis_basic_reparam(
            knots.as_ptr(),
            len_i64(knots.len())?,
            degree,
            ul,
            ur,
            out.as_mut_ptr(),
        )
    };
    check(st)?;
    Ok(out)
}

/// Result of [`compatible`].
#[derive(Debug, Clone, PartialEq)]
pub struct Compatible {
    /// The input knot vectors after upstream's in-place adjustment.
    pub adjusted: Vec<Vec<f64>>,
    /// The knots to insert into each vector.
    pub new: Vec<Vec<f64>>,
}

/// Upstream `N_GetCompatibleKnotVectorToTol(knots, tol)` (or, with `tol = None`,
/// `N_GetCompatibleKnotArray`): `(adjusted input knots, new knots per vector)`.
pub fn compatible(knots: &[Vec<f64>], tol: Option<f64>) -> Result<Compatible, FfiError> {
    if knots.is_empty() {
        return Err(bad("no knot vectors".into()));
    }
    let n_vec = knots.len();
    for k in knots {
        if k.is_empty() || k.len() + 1 < n_vec {
            return Err(bad(format!("{} knots with {n_vec} vectors", k.len())));
        }
        finite(k)?;
    }
    let tol_v = match tol {
        Some(t) if t > 0.0 && t.is_finite() => t,
        Some(t) => return Err(bad(format!("tol {t}"))),
        None => 0.0,
    };
    let flat: Vec<f64> = knots.iter().flatten().copied().collect();
    let lens: Vec<i64> = knots
        .iter()
        .map(|k| len_i64(k.len()))
        .collect::<Result<_, _>>()?;
    let total: usize = knots.iter().map(|k| k.len() - 1).sum();
    let new_cap = total + 1;
    let mut out_knots = vec![0.0; flat.len()];
    let mut out_new = vec![0.0; new_cap * n_vec];
    let mut out_new_lens = vec![0i64; n_vec];
    // SAFETY: `flat`/`lens` describe n_vec non-empty vectors; every vector has at least
    // n_vec-1 knots, so upstream's new knots fit its s+1 allocation; finite knots and
    // tol > 0 guarantee termination; the shim pads each vector with a leading slot for
    // upstream's U[-1] read. Output buffers have flat.len(), new_cap*n_vec and n_vec
    // writable slots.
    let st = unsafe {
        sys::sc_nlib_basis_basic_compatible(
            flat.as_ptr(),
            lens.as_ptr(),
            len_i64(n_vec)?,
            tol_v,
            i32::from(tol.is_none()),
            out_knots.as_mut_ptr(),
            out_new.as_mut_ptr(),
            len_i64(new_cap)?,
            out_new_lens.as_mut_ptr(),
        )
    };
    check(st)?;
    let mut adjusted = Vec::with_capacity(n_vec);
    let mut off = 0;
    for k in knots {
        let part = out_knots
            .get(off..off + k.len())
            .ok_or_else(|| bad("output layout".into()))?;
        adjusted.push(part.to_vec());
        off += k.len();
    }
    let mut new = Vec::with_capacity(n_vec);
    for (r, &len) in out_new_lens.iter().enumerate() {
        let len = usize::try_from(len).map_err(|_| bad(format!("new length {len}")))?;
        if len > new_cap {
            return Err(bad(format!("new length {len}")));
        }
        let part = out_new
            .get(r * new_cap..r * new_cap + len)
            .ok_or_else(|| bad("output layout".into()))?;
        new.push(part.to_vec());
    }
    Ok(Compatible { adjusted, new })
}

/// Upstream `N_BasisDerivs(knots, degree, u, flag, der)`: `(span, nd)` with
/// `nd[k][j]`, `k = 0..=der`, `j = 0..=degree`.
pub fn derivs(
    knots: &[f64],
    degree: i32,
    u: f64,
    flag: i32,
    der: i64,
) -> Result<(i64, Vec<Vec<f64>>), FfiError> {
    let (n, p) = basis_domain(knots, degree)?;
    if flag != 1 && flag != 2 {
        return Err(bad(format!("flag {flag}")));
    }
    if !(0..=MAX_DER).contains(&der) {
        return Err(bad(format!("der {der}")));
    }
    let d = usize::try_from(der).map_err(|_| bad("der".into()))?;
    // Upstream's int factor r = p, r *= (p-k) for k = 1..=min(p,der).
    let mut r: i32 = degree;
    for k in 1..=p.min(d) {
        let pk = i32::try_from(p - k).map_err(|_| bad("degree".into()))?;
        r = r
            .checked_mul(pk)
            .ok_or_else(|| bad("derivative factor overflows int".into()))?;
    }
    let w = p + 1;
    let mut nd = vec![0.0; (d + 1) * w];
    let mut span = -1i64;
    // SAFETY: domain checked (n >= 2p+2, finite knots, valid flag, no int overflow);
    // `nd` has (der+1)*(p+1) writable slots (upstream writes ND[0..=der][0..=p]).
    let st = unsafe {
        sys::sc_nlib_basis_basic_derivs(
            knots.as_ptr(),
            n,
            degree,
            u,
            flag,
            der,
            nd.as_mut_ptr(),
            len_i64(nd.len())?,
            &raw mut span,
        )
    };
    check(st)?;
    Ok((span, nd.chunks(w).map(<[f64]>::to_vec).collect()))
}

/// Result of [`make_compatible`].
#[derive(Debug, Clone, PartialEq)]
pub struct MakeCompatible {
    /// `R` after the possible rescale.
    pub r: Vec<f64>,
    /// `S` after the possible rescale.
    pub s: Vec<f64>,
    /// Knots to insert into `R`.
    pub xr: Vec<f64>,
    /// Knots to insert into `S`.
    pub xs: Vec<f64>,
}

fn make_compatible_vector(k: &[f64], degree: i32) -> Result<(), FfiError> {
    if !(0..=i32::from(i16::MAX)).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    let p = usize::try_from(degree).map_err(|_| bad(format!("degree {degree}")))?;
    if k.len() < p + 1 {
        return Err(bad(format!("{} knots for degree {degree}", k.len())));
    }
    if k.iter().any(|v| !v.is_finite() || v.abs() > 1e100) {
        return Err(bad("knot magnitude".into()));
    }
    let (Some(&a), Some(&b)) = (k.first(), k.last()) else {
        return Err(bad("empty knot vector".into()));
    };
    if b - a < 1e-100 || (b - a).is_nan() {
        return Err(bad("knot range".into()));
    }
    Ok(())
}

/// Upstream `N_MakeKnotsCompatible(r, s, p, q)`.
pub fn make_compatible(r: &[f64], s: &[f64], p: i32, q: i32) -> Result<MakeCompatible, FfiError> {
    make_compatible_vector(r, p)?;
    make_compatible_vector(s, q)?;
    let cap = r.len() + s.len() + 2 * MC_PAD;
    let mut out_r = vec![0.0; r.len()];
    let mut out_s = vec![0.0; s.len()];
    let mut xr = vec![0.0; cap];
    let mut xs = vec![0.0; cap];
    let (mut nxr, mut nxs) = (0i64, 0i64);
    // SAFETY: finite knots with bounded magnitude and positive ranges keep the rescale
    // finite, so upstream terminates; the shim pads both vectors with +inf entries and
    // gives upstream `cap` slots for each new-knot vector (more than it can write).
    // Output buffers have r.len(), s.len(), cap and cap writable slots.
    let st = unsafe {
        sys::sc_nlib_basis_basic_make_compatible(
            r.as_ptr(),
            len_i64(r.len())?,
            s.as_ptr(),
            len_i64(s.len())?,
            p,
            q,
            out_r.as_mut_ptr(),
            out_s.as_mut_ptr(),
            xr.as_mut_ptr(),
            &raw mut nxr,
            xs.as_mut_ptr(),
            &raw mut nxs,
            len_i64(cap)?,
        )
    };
    check(st)?;
    let nxr = usize::try_from(nxr).map_err(|_| bad("xr length".into()))?;
    let nxs = usize::try_from(nxs).map_err(|_| bad("xs length".into()))?;
    if nxr > cap || nxs > cap {
        return Err(bad("new-knot count exceeds capacity".into()));
    }
    xr.truncate(nxr);
    xs.truncate(nxs);
    Ok(MakeCompatible {
        r: out_r,
        s: out_s,
        xr,
        xs,
    })
}
