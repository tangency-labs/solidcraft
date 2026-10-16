//! FFI wrappers for unit `nlib.crv_fit_p3`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_fit_p3.cpp` (`sc_nlib_crv_fit_p3_eval`).
//! Upstream: `source/SMLib/src/NL_CrvFit.cpp`.
//!
//! | wrapper                     | upstream                          |
//! |-----------------------------|-----------------------------------|
//! | [`end_derivs`]              | `N_FitCrvDerivs`                  |
//! | [`cubic_spline`]            | `N_FitCubicSplineInterp`          |
//! | [`first_derivs`]            | `N_FitCrvFirstDeriv`              |
//! | [`knots_and_tangents`]      | `N_FitCrvKnotsAndTangents`        |
//! | [`closed_conditions`]       | `N_FitCrvApproxClosedConditions`  |
//! | [`high_derivs`]             | `N_FitCrvHighDerivs`              |
//!
//! Every wrapper returns the shim's flat record: the status (`0` or the `NL_*_ERR`
//! number), then per routine the in/out data after the call (the derivatives of
//! [`first_derivs`]; the tangents and the knot vector of [`knots_and_tangents`]) and,
//! on success, the curve as `n p m` + control points `x y z w` + knots.
//!
//! Inputs are validated so upstream only reads and writes memory it is given and
//! only reads memory it initialised: counts and degrees in range, values finite,
//! consecutive data points distinct (so the parameters increase strictly and every
//! basis span lands inside upstream's matrix storage), enough points where a missing
//! end derivative is estimated from four of them, parameters and knot vectors of
//! the shape upstream's knot searches rely on.

use crate::{call_with_buffer, FfiError};

/// Largest point / parameter / knot count accepted.
pub const MAX_COUNT: usize = 4096;
/// Largest degree / derivative count accepted (`NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_fit_p3_eval(
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

/// An existing output curve for [`end_derivs`]: highest control point index, degree
/// and knots (control points are zero).
#[derive(Debug, Clone, Copy)]
pub struct ExistingCurve<'a> {
    /// Highest control point index.
    pub n: usize,
    /// Degree.
    pub p: usize,
    /// Knots `U[0..=m]`.
    pub knots: &'a [f64],
}

fn check_finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn check_sorted(v: &[f64], what: &str, strict: bool) -> Result<(), FfiError> {
    check_finite(v, what)?;
    let ok = v
        .windows(2)
        .all(|w| matches!(w, [a, b] if if strict { a < b } else { a <= b }));
    if ok {
        Ok(())
    } else {
        Err(bad(format!("{what}: not increasing")))
    }
}

/// Data points: count in range, finite, consecutive points distinct.
fn check_pts(pts: &[[f64; 3]]) -> Result<(), FfiError> {
    if pts.is_empty() || pts.len() > MAX_COUNT {
        return Err(bad(format!("points: count {}", pts.len())));
    }
    if !pts.iter().flatten().all(|x| x.is_finite()) {
        return Err(bad("points: non-finite value"));
    }
    if pts.windows(2).any(|w| matches!(w, [a, b] if a == b)) {
        return Err(bad("points: repeated consecutive point"));
    }
    Ok(())
}

fn check_vecs(v: &[[f64; 3]], what: &str) -> Result<(), FfiError> {
    if v.len() > MAX_COUNT || !v.iter().flatten().all(|x| x.is_finite()) {
        return Err(bad(format!("{what}: bad vectors")));
    }
    Ok(())
}

fn check_par(par: i32) -> Result<(), FfiError> {
    if (1..=3).contains(&par) {
        Ok(())
    } else {
        Err(bad(format!("par {par}")))
    }
}

fn check_kind(v: i32) -> Result<(), FfiError> {
    if v == 1 || v == 2 {
        Ok(())
    } else {
        Err(bad(format!("vector kind {v}")))
    }
}

/// `p!/(p-k)!` (upstream's `int` derivative factor in `N_BasisDerivs`) fits an
/// `i32` for every derivative order up to `k`.
fn check_factor(p: usize, k: usize) -> Result<(), FfiError> {
    let mut f: i64 = 1;
    for r in 0..k.min(p) {
        f *= (p - r) as i64;
        if f > i64::from(i32::MAX) {
            return Err(bad("derivative factor overflows upstream's int"));
        }
    }
    Ok(())
}

fn is_zero(v: &[f64; 3]) -> bool {
    v.iter().all(|x| *x == 0.0)
}

fn push_pts(o: &mut Vec<f64>, pts: &[[f64; 3]]) {
    o.push(pts.len() as f64);
    for p in pts {
        o.extend_from_slice(p);
    }
}

fn push_vecs(o: &mut Vec<f64>, v: &[[f64; 3]]) {
    for p in v {
        o.extend_from_slice(p);
    }
}

fn push_reals(o: &mut Vec<f64>, v: &[f64]) {
    o.push(v.len() as f64);
    o.extend_from_slice(v);
}

/// Runs `op` on an input the caller has validated.
fn run(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly the record
        // the shim reads for `op` (built and validated by the public wrappers,
        // including every count, degree and index the shim passes to upstream); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_fit_p3_eval(
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

/// A clamped knot vector for degree `p`: `p + 1` equal knots at each end and
/// strictly increasing interior knots strictly between them.
fn check_clamped(knots: &[f64], p: usize) -> Result<(), FfiError> {
    check_sorted(knots, "knots", false)?;
    let m = knots.len().saturating_sub(1);
    if m < 2 * p + 1 {
        return Err(bad("knots: too few"));
    }
    let (Some(start), Some(end), Some(interior)) =
        (knots.get(..=p), knots.get(m - p..), knots.get(p..=m - p))
    else {
        return Err(bad("knots: too few"));
    };
    let (first, last) = (start.first(), end.last());
    let ends = start.iter().all(|k| Some(k) == first) && end.iter().all(|k| Some(k) == last);
    let ordered = matches!((first, last), (Some(a), Some(b)) if a < b);
    if !ends || !ordered || !interior.windows(2).all(|w| matches!(w, [a, b] if a < b)) {
        return Err(bad("knots: not clamped with simple interior knots"));
    }
    Ok(())
}

/// `N_FitCrvDerivs(P, kk, p, Ds, De, par, cur)` with `kk = len - 1`; `par` in `1..=4`
/// (`4` = `NL_INHERITED`, which needs `cur`). `cur` is an existing curve (else a
/// NULL one); with `NL_INHERITED` and enough storage its knots must be clamped with
/// simple interior knots and exactly `kk + p + 4` of them.
pub fn end_derivs(
    pts: &[[f64; 3]],
    p: usize,
    ds: [f64; 3],
    de: [f64; 3],
    par: i32,
    cur: Option<&ExistingCurve<'_>>,
) -> Result<Vec<f64>, FfiError> {
    check_pts(pts)?;
    let kk = pts.len() - 1;
    if p > MAX_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    check_finite(&ds, "ds")?;
    check_finite(&de, "de")?;
    if !(1..=4).contains(&par) {
        return Err(bad(format!("par {par}")));
    }
    let runs = (2..=kk + 2).contains(&p);
    if runs && (is_zero(&ds) || is_zero(&de)) && pts.len() < 4 {
        return Err(bad("estimating an end derivative needs four points"));
    }
    if par == 4 && cur.is_none() {
        return Err(bad("NL_INHERITED needs an existing curve"));
    }
    let mut o = vec![p as f64, f64::from(par)];
    push_pts(&mut o, pts);
    o.extend_from_slice(&ds);
    o.extend_from_slice(&de);
    match cur {
        None => o.push(0.0),
        Some(c) => {
            if c.knots.is_empty()
                || c.knots.len() > MAX_COUNT
                || c.n > MAX_COUNT
                || c.p > MAX_DEGREE
            {
                return Err(bad("existing curve: sizes"));
            }
            check_finite(c.knots, "existing curve knots")?;
            let n = kk + 2;
            let sized = c.n >= n && c.knots.len() > n + p + 1;
            if par == 4 && runs && sized {
                if c.knots.len() != n + p + 2 {
                    return Err(bad("existing curve: knot count"));
                }
                check_clamped(c.knots, p)?;
            }
            o.extend([1.0, c.n as f64, c.p as f64]);
            push_reals(&mut o, c.knots);
        }
    }
    run(0, &o)
}

/// `N_FitCubicSplineInterp(P, k, Ds, De, par, cur)` with `k = len - 1`.
pub fn cubic_spline(
    pts: &[[f64; 3]],
    ds: [f64; 3],
    de: [f64; 3],
    par: i32,
) -> Result<Vec<f64>, FfiError> {
    check_pts(pts)?;
    check_finite(&ds, "ds")?;
    check_finite(&de, "de")?;
    check_par(par)?;
    if pts.len() > 1 && (is_zero(&ds) || is_zero(&de)) && pts.len() < 4 {
        return Err(bad("estimating an end derivative needs four points"));
    }
    let mut o = vec![f64::from(par)];
    push_pts(&mut o, pts);
    o.extend_from_slice(&ds);
    o.extend_from_slice(&de);
    run(1, &o)
}

/// `N_FitCrvFirstDeriv(P, D, kk, p, vec, par, cur)` with `kk = len - 1 >= 1`, one
/// derivative per point; `vec` 1 (`NL_TANGENT`) or 2 (`NL_DERIVATIVE`).
pub fn first_derivs(
    pts: &[[f64; 3]],
    d: &[[f64; 3]],
    p: usize,
    vec: i32,
    par: i32,
) -> Result<Vec<f64>, FfiError> {
    check_pts(pts)?;
    if pts.len() < 2 {
        return Err(bad("needs two points"));
    }
    if d.len() != pts.len() {
        return Err(bad("one derivative per point"));
    }
    check_vecs(d, "derivatives")?;
    if p > MAX_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    check_kind(vec)?;
    check_par(par)?;
    let mut o = vec![p as f64, f64::from(vec), f64::from(par)];
    push_pts(&mut o, pts);
    push_vecs(&mut o, d);
    run(2, &o)
}

/// The inputs of [`knots_and_tangents`].
#[derive(Debug, Clone, Copy)]
pub struct Tangents<'a> {
    /// Data points.
    pub pts: &'a [[f64; 3]],
    /// Parameters, one per point, strictly increasing (or `None`).
    pub u: Option<&'a [f64]>,
    /// `0` none, `1` start, `2` end, `3` both, `4` one vector (`ts`) as both.
    pub tan: i32,
    /// Start tangent.
    pub ts: [f64; 3],
    /// End tangent.
    pub te: [f64; 3],
    /// `1` `NL_TANGENT`, `2` `NL_DERIVATIVE`.
    pub der: i32,
    /// Suggested knot vector (non-decreasing) or `None`.
    pub knots: Option<&'a [f64]>,
    /// Percentage of interval usage.
    pub per: f64,
    /// Degree.
    pub p: usize,
}

/// `N_FitCrvKnotsAndTangents(P, k, u, Ts, Te, der, &knt, per, p, cur)` with
/// `k = len - 1`.
pub fn knots_and_tangents(t: &Tangents<'_>) -> Result<Vec<f64>, FfiError> {
    check_pts(t.pts)?;
    if !(0..=4).contains(&t.tan) {
        return Err(bad(format!("tan {}", t.tan)));
    }
    check_finite(&t.ts, "ts")?;
    check_finite(&t.te, "te")?;
    check_kind(t.der)?;
    if !t.per.is_finite() {
        return Err(bad("per"));
    }
    if t.p > MAX_DEGREE {
        return Err(bad(format!("degree {}", t.p)));
    }
    if let Some(u) = t.u {
        if u.len() != t.pts.len() {
            return Err(bad("one parameter per point"));
        }
        check_sorted(u, "parameters", true)?;
    }
    if let Some(k) = t.knots {
        if k.is_empty() || k.len() > MAX_COUNT {
            return Err(bad("knots: count"));
        }
        check_sorted(k, "knots", false)?;
    }
    let mut o = vec![t.p as f64, f64::from(t.der), t.per, f64::from(t.tan)];
    push_pts(&mut o, t.pts);
    match t.u {
        Some(u) => {
            o.push(1.0);
            o.extend_from_slice(u);
        }
        None => o.push(0.0),
    }
    o.extend_from_slice(&t.ts);
    o.extend_from_slice(&t.te);
    match t.knots {
        Some(k) => {
            o.push(1.0);
            push_reals(&mut o, k);
        }
        None => o.push(0.0),
    }
    run(3, &o)
}

/// `N_FitCrvApproxClosedConditions(P, n, p, D, k, par, cur)` with `n = len - 1 >= 1`;
/// `d` holds `D[1..=k]` (or `None`); without `d` the local interpolant needs
/// `p <= nl`.
pub fn closed_conditions(
    pts: &[[f64; 3]],
    p: usize,
    d: Option<&[[f64; 3]]>,
    k: usize,
    par: i32,
) -> Result<Vec<f64>, FfiError> {
    check_pts(pts)?;
    if pts.len() < 2 {
        return Err(bad("needs two points"));
    }
    if p > MAX_DEGREE || k > MAX_DEGREE {
        return Err(bad("degree or derivative count"));
    }
    check_factor(p, k)?;
    check_par(par)?;
    let n = (pts.len() - 1) as i64;
    let pi = p as i64;
    if let Some(d) = d {
        if d.len() != k {
            return Err(bad("k derivatives"));
        }
        check_vecs(d, "derivatives")?;
    } else {
        let nm = n / 2;
        let is = nm.max(n - pi - 1);
        let ie = nm.min(pi + 1);
        let nl = n - is + ie;
        if p == 0 || pi > nl {
            return Err(bad("local interpolant degree"));
        }
    }
    let mut o = vec![
        p as f64,
        k as f64,
        f64::from(par),
        f64::from(u8::from(d.is_some())),
    ];
    push_pts(&mut o, pts);
    if let Some(d) = d {
        push_vecs(&mut o, d);
    }
    run(4, &o)
}

/// `N_FitCrvHighDerivs(P, nn, p, Ds, kk, De, ll, par, cur)` with `nn = len - 1`,
/// `kk = ds.len()`, `ll = de.len()` (`ds[i-1]` is `Ds[i]`).
pub fn high_derivs(
    pts: &[[f64; 3]],
    p: usize,
    ds: &[[f64; 3]],
    de: &[[f64; 3]],
    par: i32,
) -> Result<Vec<f64>, FfiError> {
    check_pts(pts)?;
    if p > MAX_DEGREE || ds.len() > MAX_DEGREE || de.len() > MAX_DEGREE {
        return Err(bad("degree or derivative count"));
    }
    check_factor(p, ds.len().max(de.len()))?;
    check_vecs(ds, "start derivatives")?;
    check_vecs(de, "end derivatives")?;
    check_par(par)?;
    let mut o = vec![p as f64, f64::from(par)];
    push_pts(&mut o, pts);
    o.push(ds.len() as f64);
    push_vecs(&mut o, ds);
    o.push(de.len() as f64);
    push_vecs(&mut o, de);
    run(5, &o)
}
