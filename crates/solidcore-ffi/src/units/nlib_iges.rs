//! FFI wrappers for unit `nlib.iges`.
//!
//! Shim: `harness/cpp/shim/nlib_iges.cpp` (`sc_nlib_iges_eval`).
//! Upstream: `source/SMLib/src/NL_Iges.cpp` (`N_Iges126Crv`, `N_IgesValidate`,
//! `N_Iges128Srf`, `N_IgesClampCrv`, `N_IgesCorrectCrvKnots`, `N_IgesClampSrf`,
//! `N_IgesCorrectSrfKnots`).
//!
//! Every wrapper returns the shim's record: the upstream status (`0` ok, else the
//! `NL_*_ERR` number, `1000` for a flag without a number), then on success the curve
//! (`n p m`, control points `x y z w`, knots) or surface (`n m p q r s`, net
//! row-major, u knots, v knots).
//!
//! The wrappers reject (with [`FfiError::InvalidArgument`]) everything that could make
//! upstream read outside its arrays: wrong array lengths, non-finite values, too
//! large counts or degrees, and for the stand-alone clamp / correct routines inputs
//! that break their documented preconditions (clamp: `m = n + p + 1`, `1 <= p <= 17`,
//! non-decreasing knots, `U[p] <= b0 < b1 <= U[m-p]`; correct: additionally clamped
//! ends `U[0] = U[p] < U[m-p] = U[m]`). Under those preconditions upstream stays in
//! bounds (the entity routines check them themselves before clamping).

use crate::{call_with_buffer, FfiError};

/// Largest control point upper index (`k`, `n`, `m`) accepted.
pub const MAX_INDEX: usize = 4096;
/// Largest number of net entries accepted.
pub const MAX_NET: usize = 1 << 18;
/// Largest degree accepted by the entity routines (which reject `> 17` themselves).
pub const MAX_DEGREE_ARG: usize = 1000;
/// Largest degree accepted by the stand-alone clamp / correct routines (`NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;

mod sys {
    extern "C" {
        pub fn sc_nlib_iges_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(name: &str, a: &[f64]) -> Result<(), FfiError> {
    if a.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{name}: non-finite value")))
    }
}

fn index(name: &str, k: usize, max: usize) -> Result<(), FfiError> {
    if k > max {
        return Err(bad(format!("{name} = {k} above {max}")));
    }
    Ok(())
}

fn call(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(256, |buf, len| {
        // SAFETY: `input` is valid for `input.len()` reads and laid out as the shim
        // expects for `op` (checked by the callers); `buf` is valid for `buf.len()`
        // writes; `len` is a valid pointer.
        unsafe {
            sys::sc_nlib_iges_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// `U[p] <= v0 < v1 <= U[m-p]` and non-decreasing (the `N_IgesValidate` test).
fn valid_knots(u: &[f64], p: usize, b: [f64; 2]) -> bool {
    let m = u.len().wrapping_sub(1);
    let (Some(&up), Some(&ump)) = (u.get(p), m.checked_sub(p).and_then(|i| u.get(i))) else {
        return false;
    };
    up <= b[0] && b[0] < b[1] && ump >= b[1] && u.windows(2).all(|w| w.first() <= w.last())
}

/// Clamped ends with a non-empty domain: `U[0] = U[p] < U[m-p] = U[m]`, non-decreasing.
fn clamped_knots(u: &[f64], p: usize) -> bool {
    let m = u.len().wrapping_sub(1);
    let (Some(&a), Some(&ap), Some(&bp), Some(&b)) = (
        u.first(),
        u.get(p),
        m.checked_sub(p).and_then(|i| u.get(i)),
        u.last(),
    ) else {
        return false;
    };
    a == ap && ap < bp && bp == b && u.windows(2).all(|w| w.first() <= w.last())
}

/// Upstream `N_Iges126Crv(k, m, t, w, P, v)`; `pts` are Euclidean `x y z`.
pub fn iges_126(
    k: usize,
    m: usize,
    t: &[f64],
    w: &[f64],
    pts: &[[f64; 3]],
    v: [f64; 2],
) -> Result<Vec<f64>, FfiError> {
    index("k", k, MAX_INDEX)?;
    index("m", m, MAX_DEGREE_ARG)?;
    if t.len() != k + m + 2 || w.len() != k + 1 || pts.len() != k + 1 {
        return Err(bad("126: array lengths do not match k and m"));
    }
    let mut input = vec![k as f64, m as f64, v[0], v[1]];
    input.extend_from_slice(t);
    input.extend_from_slice(w);
    input.extend(pts.iter().flatten());
    finite("126 input", &input)?;
    call(0, &input)
}

/// Upstream `N_IgesValidate(U, m, p, v)` over all of `u` (`m = u.len() - 1`).
pub fn validate(u: &[f64], p: usize, v: [f64; 2]) -> Result<Vec<f64>, FfiError> {
    let m = u
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad("validate: no knots"))?;
    index("m", m, MAX_INDEX)?;
    if p > m {
        return Err(bad("validate: p > m"));
    }
    let mut input = vec![m as f64, p as f64, v[0], v[1]];
    input.extend_from_slice(u);
    finite("validate input", &input)?;
    call(1, &input)
}

/// Upstream `N_Iges128Srf(k1, k2, m1, m2, s, t, w, P, u, v)`; `w` and `pts` are
/// row-major `(k1+1) x (k2+1)` (`w[i][j]`, `P[i][j]`).
pub fn iges_128(
    k: [usize; 2],
    deg: [usize; 2],
    s: &[f64],
    t: &[f64],
    w: &[f64],
    pts: &[[f64; 3]],
    u: [f64; 2],
    v: [f64; 2],
) -> Result<Vec<f64>, FfiError> {
    let [k1, k2] = k;
    let [m1, m2] = deg;
    index("k1", k1, MAX_INDEX)?;
    index("k2", k2, MAX_INDEX)?;
    index("m1", m1, MAX_DEGREE_ARG)?;
    index("m2", m2, MAX_DEGREE_ARG)?;
    let np = (k1 + 1) * (k2 + 1);
    index("net size", np, MAX_NET)?;
    if s.len() != k1 + m1 + 2 || t.len() != k2 + m2 + 2 || w.len() != np || pts.len() != np {
        return Err(bad("128: array lengths do not match k1, k2, m1, m2"));
    }
    let mut input = vec![
        k1 as f64, k2 as f64, m1 as f64, m2 as f64, u[0], u[1], v[0], v[1],
    ];
    input.extend_from_slice(s);
    input.extend_from_slice(t);
    input.extend_from_slice(w);
    input.extend(pts.iter().flatten());
    finite("128 input", &input)?;
    call(2, &input)
}

fn curve_input(
    p: usize,
    cpts: &[[f64; 4]],
    knots: &[f64],
    b: [f64; 2],
) -> Result<Vec<f64>, FfiError> {
    if p == 0 || p > MAX_DEGREE {
        return Err(bad("curve: degree outside 1..=17"));
    }
    let n = cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad("curve: no control points"))?;
    index("n", n, MAX_INDEX)?;
    if knots.len() != n + p + 2 {
        return Err(bad("curve: knot count is not n + p + 2"));
    }
    let mut input = vec![p as f64, n as f64, b[0], b[1]];
    input.extend(cpts.iter().flatten());
    input.extend_from_slice(knots);
    finite("curve input", &input)?;
    Ok(input)
}

/// Upstream `N_IgesClampCrv` on the curve `(cpts, p, knots)` with bounds `b`.
pub fn clamp_curve(
    p: usize,
    cpts: &[[f64; 4]],
    knots: &[f64],
    b: [f64; 2],
) -> Result<Vec<f64>, FfiError> {
    let input = curve_input(p, cpts, knots, b)?;
    if !valid_knots(knots, p, b) {
        return Err(bad(
            "clamp curve: knots / bounds fail the N_IgesValidate test",
        ));
    }
    call(3, &input)
}

/// Upstream `N_IgesCorrectCrvKnots` on the curve `(cpts, p, knots)`.
pub fn correct_curve(p: usize, cpts: &[[f64; 4]], knots: &[f64]) -> Result<Vec<f64>, FfiError> {
    let input = curve_input(p, cpts, knots, [0.0, 0.0])?;
    if !clamped_knots(knots, p) {
        return Err(bad(
            "correct curve: knots are not clamped and non-decreasing",
        ));
    }
    call(4, &input)
}

/// A surface for the stand-alone surface routines.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceArgs<'a> {
    /// Degrees `(p, q)`.
    pub deg: [usize; 2],
    /// Net size `(n + 1, m + 1)`.
    pub size: [usize; 2],
    /// Row-major net, `x y z w`.
    pub net: &'a [[f64; 4]],
    /// U knots (`n + p + 2`).
    pub u: &'a [f64],
    /// V knots (`m + q + 2`).
    pub v: &'a [f64],
}

fn surface_input(a: &SurfaceArgs<'_>, ub: [f64; 2], vb: [f64; 2]) -> Result<Vec<f64>, FfiError> {
    let [p, q] = a.deg;
    if p == 0 || p > MAX_DEGREE || q == 0 || q > MAX_DEGREE {
        return Err(bad("surface: degree outside 1..=17"));
    }
    let [rows, cols] = a.size;
    let n = rows.checked_sub(1).ok_or_else(|| bad("surface: no rows"))?;
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| bad("surface: no columns"))?;
    index("n", n, MAX_INDEX)?;
    index("m", m, MAX_INDEX)?;
    index("net size", rows * cols, MAX_NET)?;
    if a.net.len() != rows * cols || a.u.len() != n + p + 2 || a.v.len() != m + q + 2 {
        return Err(bad("surface: array lengths do not match"));
    }
    let mut input = vec![
        p as f64, q as f64, n as f64, m as f64, ub[0], ub[1], vb[0], vb[1],
    ];
    input.extend(a.net.iter().flatten());
    input.extend_from_slice(a.u);
    input.extend_from_slice(a.v);
    finite("surface input", &input)?;
    Ok(input)
}

/// Upstream `N_IgesClampSrf` with bounds `ub`, `vb`.
pub fn clamp_surface(
    a: &SurfaceArgs<'_>,
    ub: [f64; 2],
    vb: [f64; 2],
) -> Result<Vec<f64>, FfiError> {
    let input = surface_input(a, ub, vb)?;
    if !valid_knots(a.u, a.deg[0], ub) || !valid_knots(a.v, a.deg[1], vb) {
        return Err(bad(
            "clamp surface: knots / bounds fail the N_IgesValidate test",
        ));
    }
    call(5, &input)
}

/// Upstream `N_IgesCorrectSrfKnots`.
pub fn correct_surface(a: &SurfaceArgs<'_>) -> Result<Vec<f64>, FfiError> {
    let input = surface_input(a, [0.0, 0.0], [0.0, 0.0])?;
    if !clamped_knots(a.u, a.deg[0]) || !clamped_knots(a.v, a.deg[1]) {
        return Err(bad(
            "correct surface: knots are not clamped and non-decreasing",
        ));
    }
    call(6, &input)
}
