//! FFI wrappers for unit `nlib.basis_adv`.
//!
//! Shim: `harness/cpp/shim/nlib_basis_adv.cpp`.
//! Upstream: `source/SMLib/src/NL_BasisAdv.cpp`.
//!
//! | Rust              | shim                                | upstream                       |
//! |-------------------|-------------------------------------|--------------------------------|
//! | [`compat_mult`]   | `sc_nlib_basis_adv_compat_mult`     | `N_GetCompatibleKnotArrayMult` |
//! | [`compat_mult_reads_in_bounds`] | `sc_nlib_basis_adv_compat_mult_walk` | (guard of [`compat_mult`]) |
//! | [`insert`]        | `sc_nlib_basis_adv_insert`          | `N_BasisInsertKnots`           |
//! | [`inc_mult`]      | `sc_nlib_basis_adv_inc_mult`        | `N_BasisIncreaseKnotMult`      |
//! | [`derivs_array`]  | `sc_nlib_basis_adv_derivs_array`    | `N_BasisDerivsArray`           |
//!
//! Errors: `FfiError::Status(code)` carries the NLib error number (`NL_ERROR.eno`)
//! when the upstream routine returned its error flag.
//!
//! Argument domain (checked here, before any call; inputs outside it would make
//! upstream read or write out of bounds, overflow an `int`, or loop forever):
//! - `compat_mult`: `0 <= p <= 32767`, at least `p + 1` knots per vector, finite knots
//!   with `|knot| <= 1e100` and a positive range `U[m] - U[0] >= 1e-100` (so the
//!   rescale stays finite and the loop terminates), and no read past `R[mr]` or `S[ms]`
//!   (upstream-issues UB-13: degree 0 with ends of multiplicity > 1, unclamped or
//!   unsorted vectors). That last check is exact: [`compat_mult_reads_in_bounds`] replays
//!   upstream's rescale (with `N_BasisReparam` itself) and index walk in the shim. The
//!   shim still pads both vectors with `+inf` entries and gives the new-knot vectors room
//!   for every write.
//! - `insert`: `0 <= p <= 32767`, at least `p + 1` knots, a non-empty insert vector,
//!   every knot finite (NaN knots make upstream loop forever). With fewer than
//!   `2p + 2` knots (only an unsorted `R` passes the range check) upstream writes
//!   `2p + 2 + mx + 1` knots, past the documented `m + mx + 2` (UB-196); the shim's
//!   output buffer holds them, so the result is defined and compared.
//! - `inc_mult`: `0 <= p <= 32767`, `0 <= t <= 1000`, a non-empty knot vector.
//! - `derivs_array`: `0 <= degree <= 1000`, at least `2*degree + 2` finite knots (as
//!   `nlib.knots`), a valid flag, `0 <= der <= 1000` with every factor `p!/(p-k)!`,
//!   `k = 1..=min(p,der)`, fitting an `i32`, at least one parameter, all finite, and
//!   every inner parameter `u[1..n]` below `U[m-p]` (`NL_LEFT`) or at most `U[m-p]`
//!   (`NL_RIGHT`), so the span walk stops at a span `<= m-p-1`.

use crate::{call_with_buffer, check, FfiError};

/// Largest degree accepted by [`derivs_array`].
pub const MAX_DEGREE: i32 = 1000;
/// Largest derivative order accepted by [`derivs_array`].
pub const MAX_DER: i64 = 1000;
/// Largest multiplicity increment accepted by [`inc_mult`].
pub const MAX_T: i64 = 1000;
/// Trailing padding the shim adds to `compat_mult` knot buffers.
const PAD: usize = 8;

mod sys {
    extern "C" {
        pub fn sc_nlib_basis_adv_compat_mult(
            r: *const f64,
            n_r: i64,
            s: *const f64,
            n_s: i64,
            p: i32,
            out_r: *mut f64,
            out_s: *mut f64,
            out_xr: *mut f64,
            out_nxr: *mut i64,
            out_xs: *mut f64,
            out_nxs: *mut i64,
            cap: i64,
        ) -> i32;
        pub fn sc_nlib_basis_adv_compat_mult_walk(
            r: *const f64,
            n_r: i64,
            s: *const f64,
            n_s: i64,
            p: i32,
            out_walk: *mut i32,
        ) -> i32;
        pub fn sc_nlib_basis_adv_insert(
            r: *const f64,
            n_r: i64,
            p: i32,
            x: *const f64,
            n_x: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_basis_adv_inc_mult(
            u: *const f64,
            n_u: i64,
            p: i32,
            t: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_nlib_basis_adv_derivs_array(
            knots: *const f64,
            n_knots: i64,
            p: i32,
            u: *const f64,
            n_u: i64,
            flag: i32,
            der: i64,
            out_nd: *mut f64,
            cap: i64,
            out_spn: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: String) -> FfiError {
    FfiError::InvalidArgument(msg)
}

fn len_i64(n: usize) -> Result<i64, FfiError> {
    i64::try_from(n).map_err(|_| bad("length overflow".into()))
}

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("non-finite {what}")))
    }
}

/// `0 <= p <= 32767` and at least `p + 1` knots; returns `p`.
fn degree_and_len(k: &[f64], p: i32) -> Result<usize, FfiError> {
    if !(0..=i32::from(i16::MAX)).contains(&p) {
        return Err(bad(format!("degree {p}")));
    }
    let pu = usize::try_from(p).map_err(|_| bad(format!("degree {p}")))?;
    if k.len() < pu + 1 {
        return Err(bad(format!("{} knots for degree {p}", k.len())));
    }
    Ok(pu)
}

fn compat_vector(k: &[f64], p: i32) -> Result<(), FfiError> {
    degree_and_len(k, p)?;
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

/// Result of [`compat_mult`].
#[derive(Debug, Clone, PartialEq)]
pub struct CompatMult {
    /// `R` after the possible rescale.
    pub r: Vec<f64>,
    /// `S` after the possible rescale.
    pub s: Vec<f64>,
    /// Knots to insert into `R`.
    pub xr: Vec<f64>,
    /// Knots to insert into `S`.
    pub xs: Vec<f64>,
}

/// Whether `N_GetCompatibleKnotArrayMult(r, s, p)` reads only `r[0..=mr]` and
/// `s[0..=ms]` (`false`: it would read past an end, upstream-issues UB-13). The shim
/// replays upstream's rescale and index walk on copies, without the routine itself.
///
/// Errors: the other [`compat_mult`] domain checks, and `InvalidArgument` where the walk
/// would stop advancing (upstream loops forever; excluded by those checks).
pub fn compat_mult_reads_in_bounds(r: &[f64], s: &[f64], p: i32) -> Result<bool, FfiError> {
    compat_vector(r, p)?;
    compat_vector(s, p)?;
    let mut walk = -1i32;
    // SAFETY: both vectors have >= p+1 finite knots of bounded magnitude and positive
    // range; the shim reads them into copies and checks every index before reading it,
    // and writes one value to `walk`.
    let st = unsafe {
        sys::sc_nlib_basis_adv_compat_mult_walk(
            r.as_ptr(),
            len_i64(r.len())?,
            s.as_ptr(),
            len_i64(s.len())?,
            p,
            &raw mut walk,
        )
    };
    check(st)?;
    match walk {
        0 => Ok(true),
        1 => Ok(false),
        2 => Err(bad("upstream would loop forever".into())),
        w => Err(bad(format!("walk status {w}"))),
    }
}

/// Upstream `N_GetCompatibleKnotArrayMult(r, s, p)`.
pub fn compat_mult(r: &[f64], s: &[f64], p: i32) -> Result<CompatMult, FfiError> {
    if !compat_mult_reads_in_bounds(r, s, p)? {
        return Err(bad(
            "upstream would read past the knot vectors (UB-13)".into()
        ));
    }
    let cap = r.len() + s.len() + 2 * PAD;
    let mut out_r = vec![0.0; r.len()];
    let mut out_s = vec![0.0; s.len()];
    let mut xr = vec![0.0; cap];
    let mut xs = vec![0.0; cap];
    let (mut nxr, mut nxs) = (0i64, 0i64);
    // SAFETY: finite knots with bounded magnitude and positive ranges keep the rescale
    // finite, so upstream terminates; the walk check above keeps every knot read in
    // bounds (the shim also pads both vectors with +inf entries) and the shim gives
    // upstream `cap` slots for each new-knot vector (more than it can write).
    // Output buffers have r.len(), s.len(), cap and cap writable slots.
    let st = unsafe {
        sys::sc_nlib_basis_adv_compat_mult(
            r.as_ptr(),
            len_i64(r.len())?,
            s.as_ptr(),
            len_i64(s.len())?,
            p,
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
    Ok(CompatMult {
        r: out_r,
        s: out_s,
        xr,
        xs,
    })
}

/// Upstream `N_BasisInsertKnots(r, p, x)`: the refined knot vector.
pub fn insert(r: &[f64], p: i32, x: &[f64]) -> Result<Vec<f64>, FfiError> {
    degree_and_len(r, p)?;
    if x.is_empty() {
        return Err(bad("empty insert vector".into()));
    }
    finite(r, "knot")?;
    finite(x, "insert knot")?;
    let (n_r, n_x) = (len_i64(r.len())?, len_i64(x.len())?);
    call_with_buffer(r.len() + x.len(), |buf, len| {
        // SAFETY: r has >= p+1 finite knots and x >= 1 finite knot, so upstream reads in
        // bounds (the shim pads X for its X[mx+1] read) and terminates; the shim sizes
        // upstream's output for every knot it writes (UB-196), writes at most buf.len()
        // entries to buf and one value to len.
        unsafe {
            sys::sc_nlib_basis_adv_insert(
                r.as_ptr(),
                n_r,
                p,
                x.as_ptr(),
                n_x,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Upstream `N_BasisIncreaseKnotMult(u, p, t)`: the knots to insert.
pub fn inc_mult(u: &[f64], p: i32, t: i64) -> Result<Vec<f64>, FfiError> {
    if u.is_empty() {
        return Err(bad("empty knot vector".into()));
    }
    if !(0..=i32::from(i16::MAX)).contains(&p) {
        return Err(bad(format!("degree {p}")));
    }
    if !(0..=MAX_T).contains(&t) {
        return Err(bad(format!("t {t}")));
    }
    let n_u = len_i64(u.len())?;
    let cap = u.len() * usize::try_from(t).map_err(|_| bad("t".into()))? + 1;
    call_with_buffer(cap, |buf, len| {
        // SAFETY: u is non-empty; upstream reads U[p+1..=m-p] only and the shim sizes
        // its insert buffer for every write; the shim writes at most buf.len() entries
        // to buf and one value to len.
        unsafe {
            sys::sc_nlib_basis_adv_inc_mult(
                u.as_ptr(),
                n_u,
                p,
                t,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Result of [`derivs_array`].
#[derive(Debug, Clone, PartialEq)]
pub struct DerivsArray {
    /// Span index of each parameter.
    pub spans: Vec<i64>,
    /// `nd[k][i][j]`, `k = 0..=der`, `i = 0..=degree`, `j < u.len()`.
    pub nd: Vec<Vec<Vec<f64>>>,
}

/// Upstream `N_BasisDerivsArray(knots, degree, u, u.len()-1, flag, der)`.
pub fn derivs_array(
    knots: &[f64],
    degree: i32,
    u: &[f64],
    flag: i32,
    der: i64,
) -> Result<DerivsArray, FfiError> {
    if !(0..=MAX_DEGREE).contains(&degree) {
        return Err(bad(format!("degree {degree}")));
    }
    let p = usize::try_from(degree).map_err(|_| bad(format!("degree {degree}")))?;
    if knots.len() < 2 * p + 2 {
        return Err(bad(format!("{} knots for degree {degree}", knots.len())));
    }
    finite(knots, "knot")?;
    if u.is_empty() {
        return Err(bad("no parameters".into()));
    }
    finite(u, "parameter")?;
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
    // The span walk for u[1..n] stops at the first U[k+1] > u (left) / >= u (right);
    // U[m-p] must stop it so the span stays <= m-p-1.
    let end = knots
        .get(knots.len() - 1 - p)
        .copied()
        .ok_or_else(|| bad("knots".into()))?;
    let inner = u.get(1..u.len().saturating_sub(1)).unwrap_or(&[]);
    let walk_ok = inner
        .iter()
        .all(|&v| if flag == 1 { v < end } else { v <= end });
    if !walk_ok {
        return Err(bad("inner parameter past the domain end".into()));
    }
    let w = p + 1;
    let n = u.len();
    let total = (d + 1)
        .checked_mul(w)
        .and_then(|v| v.checked_mul(n))
        .filter(|&v| v <= 50_000_000)
        .ok_or_else(|| bad("output too large".into()))?;
    let mut nd = vec![0.0; total];
    let mut spn = vec![-1i64; n];
    // SAFETY: domain checked (>= 2p+2 finite knots, valid flag, no int overflow, the
    // span walk stays inside the knots); `nd` has (der+1)*(p+1)*n writable slots and
    // `spn` n slots, exactly what upstream writes.
    let st = unsafe {
        sys::sc_nlib_basis_adv_derivs_array(
            knots.as_ptr(),
            len_i64(knots.len())?,
            degree,
            u.as_ptr(),
            len_i64(n)?,
            flag,
            der,
            nd.as_mut_ptr(),
            len_i64(total)?,
            spn.as_mut_ptr(),
        )
    };
    check(st)?;
    let nd = nd
        .chunks(w * n)
        .map(|plane| plane.chunks(n).map(<[f64]>::to_vec).collect())
        .collect();
    Ok(DerivsArray { spans: spn, nd })
}
