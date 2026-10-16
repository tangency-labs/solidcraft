//! FFI wrappers for unit `nlib.crv_fit_p2`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_fit_p2.cpp` (`sc_nlib_crv_fit_p2_eval`).
//! Upstream: `source/SMLib/src/NL_CrvFit.cpp`.
//!
//! | wrapper                   | upstream                         |
//! |---------------------------|----------------------------------|
//! | [`func_interp`]           | `N_FitFuncInterpGivenParams`     |
//! | [`interp`]                | `N_FitCrvInterp`                 |
//! | [`approx_lst_sq`]         | `N_FitCrvApproxLstSq`            |
//! | [`crv_matrix`]            | `N_FitCrvMatrix`                 |
//! | [`deriv_matrix`]          | `N_FitCrvDerivMatrix`            |
//! | [`weighted_lst_sq`]       | `N_FitCrvWeightedLstSqKnots`     |
//! | [`knots_derivs`]          | `N_FitCalcKnotsDerivs`           |
//! | [`knots_high_end_derivs`] | `N_FitCalcKnotsHighEndDerivs`    |
//! | [`calc_matrix`]           | `N_FitCalcMatrix`                |
//!
//! Every wrapper returns the shim's flat record: the status (`0` or the `NL_*_ERR`
//! number), then on success the result (knot vectors as `count values`; curves as
//! `n p m` + control points `x y z w` + knots; the curve function as `n p m` +
//! values + knots; control point arrays as `count` + points; the matrix as
//! `n bw count` + its band storage). The two routines with a given matrix first
//! write the status of its LU decomposition (and stop when it is nonzero).
//!
//! Points are `[x, y, z, w]` with a kind flag `ptp` (`1` = `NL_EPOINT`, `w`
//! ignored; `2` = `NL_HPOINT`). Inputs are validated so upstream only reads and
//! writes memory it is given and only reads memory it initialised: counts, indices
//! and degrees in range, values finite, parameters and knots non-decreasing where
//! upstream's band bookkeeping relies on it, derivative indices strictly increasing.

use crate::{call_with_buffer, FfiError};

/// Largest point / parameter / knot count accepted.
pub const MAX_COUNT: usize = 4096;
/// Largest degree accepted where upstream evaluates basis functions (`NL_MAXDEG`).
pub const MAX_BASIS_DEGREE: usize = 17;
/// Largest degree / derivative count accepted by the knot vector routines.
pub const MAX_KNOT_DEGREE: usize = 64;

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_fit_p2_eval(
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

/// Point data: kind flag and points.
#[derive(Debug, Clone, Copy)]
pub struct Pts<'a> {
    /// `1` (`NL_EPOINT`) or `2` (`NL_HPOINT`).
    pub ptp: i32,
    /// The points, `[x, y, z, w]`.
    pub pts: &'a [[f64; 4]],
}

/// A square matrix given by its storage, LU-decomposed by the shim before use.
#[derive(Debug, Clone, Copy)]
pub struct Matrix<'a> {
    /// Highest row / column index.
    pub n: usize,
    /// `NL_MATRIXTYPE`: `0` full or `3` banded.
    pub mtp: i32,
    /// Bandwidth (odd for banded matrices).
    pub bw: usize,
    /// Row-major storage: `(n+1) x (n+1)` (full) or `(n+1) x bw` (banded, `x 2`
    /// for `bw == 1`).
    pub storage: &'a [f64],
}

/// The data of [`weighted_lst_sq`].
#[derive(Debug, Clone, Copy)]
pub struct Weighted<'a> {
    /// Points `E[0..=r]` with their kind.
    pub pts: Pts<'a>,
    /// Point weights, one per point.
    pub wp: &'a [f64],
    /// Derivatives `F` (kind of `pts`), at least `wd.len()` of them.
    pub derivs: &'a [[f64; 4]],
    /// Derivative weights `wd[0..=s]` (may be empty).
    pub wd: &'a [f64],
    /// `I[0..=s]`, strictly increasing point indices.
    pub index: &'a [usize],
}

fn check_ptp(ptp: i32) -> Result<(), FfiError> {
    if ptp == 1 || ptp == 2 {
        Ok(())
    } else {
        Err(bad(format!("ptp {ptp}")))
    }
}

fn check_count(len: usize, what: &str) -> Result<(), FfiError> {
    if len == 0 || len > MAX_COUNT {
        return Err(bad(format!("{what}: count {len}")));
    }
    Ok(())
}

fn check_finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn check_sorted(v: &[f64], what: &str) -> Result<(), FfiError> {
    check_finite(v, what)?;
    if v.windows(2).all(|w| matches!(w, [a, b] if a <= b)) {
        Ok(())
    } else {
        Err(bad(format!("{what}: not non-decreasing")))
    }
}

fn check_pts(a: &Pts<'_>, what: &str) -> Result<(), FfiError> {
    check_ptp(a.ptp)?;
    check_count(a.pts.len(), what)?;
    if a.pts.iter().flatten().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn check_basis_degree(p: usize) -> Result<(), FfiError> {
    if p == 0 || p > MAX_BASIS_DEGREE {
        return Err(bad(format!("degree {p}")));
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

/// Knots `N_FindSpan` can search for degree `p`: non-decreasing, finite, at least
/// `2p + 2` of them.
fn check_knots(knots: &[f64], p: usize) -> Result<(), FfiError> {
    check_count(knots.len(), "knots")?;
    check_sorted(knots, "knots")?;
    if knots.len() < 2 * p + 2 {
        return Err(bad("too few knots for the degree"));
    }
    Ok(())
}

fn push_pts(o: &mut Vec<f64>, a: &Pts<'_>) {
    o.push(f64::from(a.ptp));
    o.push(a.pts.len() as f64);
    for p in a.pts {
        o.extend_from_slice(p);
    }
}

fn push_reals(o: &mut Vec<f64>, v: &[f64]) {
    o.push(v.len() as f64);
    o.extend_from_slice(v);
}

fn push_xyz(o: &mut Vec<f64>, pts: &[[f64; 3]]) {
    o.push(pts.len() as f64);
    for p in pts {
        o.extend_from_slice(p);
    }
}

fn check_xyz(pts: &[[f64; 3]]) -> Result<(), FfiError> {
    check_count(pts.len(), "points")?;
    if pts.iter().flatten().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad("points: non-finite value"))
    }
}

fn check_matrix(cm: &Matrix<'_>) -> Result<(), FfiError> {
    if cm.n >= MAX_COUNT {
        return Err(bad("matrix: size"));
    }
    let cols = match cm.mtp {
        0 => cm.n + 1,
        3 => {
            if cm.bw.is_multiple_of(2) || cm.bw > 2 * cm.n + 1 {
                return Err(bad(format!("matrix: bandwidth {}", cm.bw)));
            }
            if cm.bw == 1 {
                2
            } else {
                cm.bw
            }
        }
        t => return Err(bad(format!("matrix: type {t}"))),
    };
    if cm.storage.len() != (cm.n + 1) * cols {
        return Err(bad("matrix: storage size"));
    }
    check_finite(cm.storage, "matrix")
}

fn push_matrix(o: &mut Vec<f64>, cm: &Matrix<'_>) {
    o.push(cm.n as f64);
    o.push(f64::from(cm.mtp));
    o.push(cm.bw as f64);
    push_reals(o, cm.storage);
}

/// Runs `op` on an input the caller has validated.
fn run(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly the record
        // the shim reads for `op` (built and validated by the public wrappers,
        // including every count and index the shim passes to upstream); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_crv_fit_p2_eval(
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

/// `N_FitFuncInterpGivenParams(f, n, p, u, cfn)` with `n = f.len() - 1`; `u` has at
/// least `n + 1` non-decreasing parameters; `1 <= p <= 17`.
pub fn func_interp(f: &[f64], p: usize, u: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_count(f.len(), "values")?;
    check_finite(f, "values")?;
    check_basis_degree(p)?;
    check_count(u.len(), "parameters")?;
    if u.len() < f.len() {
        return Err(bad("too few parameters"));
    }
    check_sorted(u, "parameters")?;
    let mut o = vec![p as f64];
    push_reals(&mut o, f);
    push_reals(&mut o, u);
    run(0, &o)
}

/// `N_FitCrvInterp(P, n, p, par, cur)` with `n = len - 1`; `1 <= p <= 17`, `par` in
/// `1..=3`.
pub fn interp(pts: &[[f64; 3]], p: usize, par: i32) -> Result<Vec<f64>, FfiError> {
    check_xyz(pts)?;
    check_basis_degree(p)?;
    check_par(par)?;
    let mut o = vec![p as f64, f64::from(par)];
    push_xyz(&mut o, pts);
    run(1, &o)
}

/// `N_FitCrvApproxLstSq(P, m, n, p, par, cur)` with `m = len - 1`; `1 <= p <= 17`,
/// `par` in `1..=3`.
pub fn approx_lst_sq(pts: &[[f64; 3]], n: usize, p: usize, par: i32) -> Result<Vec<f64>, FfiError> {
    check_xyz(pts)?;
    check_basis_degree(p)?;
    check_par(par)?;
    if n > MAX_COUNT {
        return Err(bad(format!("n {n}")));
    }
    let mut o = vec![n as f64, p as f64, f64::from(par)];
    push_xyz(&mut o, pts);
    run(2, &o)
}

/// `N_FitCrvMatrix(A, k, ptp, p, cm, Pw)` with `k = len - 1`, after LU-decomposing
/// `cm`; unless `p == 1`, `a` needs at least `cm.n + 1` points.
pub fn crv_matrix(a: &Pts<'_>, p: usize, cm: &Matrix<'_>) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    if p > MAX_KNOT_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    check_matrix(cm)?;
    if p != 1 && a.pts.len() <= cm.n {
        return Err(bad("matrix larger than the data"));
    }
    let mut o = vec![p as f64];
    push_pts(&mut o, a);
    push_matrix(&mut o, cm);
    run(3, &o)
}

/// `N_FitCrvDerivMatrix(A, k, ptp, p, knt, T, whr, cm, Pw)` with `k = len - 1`,
/// after LU-decomposing `cm` (at most `k + 2` rows); `whr` 1 or 2.
pub fn deriv_matrix(
    a: &Pts<'_>,
    p: usize,
    knots: &[f64],
    t: [f64; 4],
    whr: i32,
    cm: &Matrix<'_>,
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    let k = a.pts.len() - 1;
    if p > MAX_KNOT_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    if whr != 1 && whr != 2 {
        return Err(bad(format!("whr {whr}")));
    }
    check_count(knots.len(), "knots")?;
    check_finite(knots, "knots")?;
    let need = if whr == 1 { p + 2 } else { k + p + 2 };
    if knots.len() < need {
        return Err(bad("too few knots"));
    }
    check_finite(&t, "derivative")?;
    check_matrix(cm)?;
    if cm.n > k + 1 {
        return Err(bad("matrix larger than k + 2"));
    }
    let mut o = vec![p as f64, f64::from(whr)];
    push_pts(&mut o, a);
    push_reals(&mut o, knots);
    o.extend_from_slice(&t);
    push_matrix(&mut o, cm);
    run(4, &o)
}

/// `N_FitCrvWeightedLstSqKnots(E, wp, r, F, wd, I, s, ptp, u, knt, n, p, cur)` with
/// `r = len - 1`, `s = wd.len() - 1`. Requires `1 <= p <= 17`, one weight and one
/// non-decreasing parameter per point, `wd.len()` derivatives and indices, strictly
/// increasing indices up to `r` (so every datum upstream counts is used and every
/// weight it reads is written), and searchable knots.
pub fn weighted_lst_sq(
    d: &Weighted<'_>,
    u: &[f64],
    knots: &[f64],
    n: usize,
    p: usize,
) -> Result<Vec<f64>, FfiError> {
    check_pts(&d.pts, "points")?;
    let cnt = d.pts.pts.len();
    check_basis_degree(p)?;
    if n > MAX_COUNT {
        return Err(bad(format!("n {n}")));
    }
    if d.wp.len() != cnt {
        return Err(bad("one weight per point"));
    }
    check_finite(d.wp, "point weights")?;
    let s = d.wd.len();
    if s > MAX_COUNT || d.derivs.len() != s || d.index.len() != s {
        return Err(bad("derivatives, weights and indices differ in count"));
    }
    check_finite(d.wd, "derivative weights")?;
    if !d.derivs.iter().flatten().all(|x| x.is_finite()) {
        return Err(bad("derivatives: non-finite value"));
    }
    if !d.index.windows(2).all(|w| matches!(w, [a, b] if a < b))
        || d.index.last().is_some_and(|&l| l >= cnt)
    {
        return Err(bad("indices not strictly increasing below the point count"));
    }
    if u.len() != cnt {
        return Err(bad("one parameter per point"));
    }
    check_sorted(u, "parameters")?;
    check_knots(knots, p)?;
    let mut o = vec![n as f64, p as f64];
    push_pts(&mut o, &d.pts);
    push_reals(&mut o, d.wp);
    push_pts(
        &mut o,
        &Pts {
            ptp: d.pts.ptp,
            pts: d.derivs,
        },
    );
    push_reals(&mut o, d.wd);
    let idx: Vec<f64> = d.index.iter().map(|&i| i as f64).collect();
    push_reals(&mut o, &idx);
    push_reals(&mut o, u);
    push_reals(&mut o, knots);
    run(5, &o)
}

/// `N_FitCalcKnotsDerivs(u, k, p, knt)` with `k = len - 1`; `p == 2` needs two
/// parameters.
pub fn knots_derivs(u: &[f64], p: usize) -> Result<Vec<f64>, FfiError> {
    check_count(u.len(), "parameters")?;
    check_finite(u, "parameters")?;
    if p > MAX_KNOT_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    if p == 2 && u.len() < 2 {
        return Err(bad("p == 2 needs two parameters"));
    }
    let mut o = vec![p as f64];
    push_reals(&mut o, u);
    run(6, &o)
}

/// `N_FitCalcKnotsHighEndDerivs(u, n, p, k, l, knt)` with `n = len - 1`.
pub fn knots_high_end_derivs(
    u: &[f64],
    p: usize,
    k: usize,
    l: usize,
) -> Result<Vec<f64>, FfiError> {
    check_count(u.len(), "parameters")?;
    check_finite(u, "parameters")?;
    if p > MAX_KNOT_DEGREE || k > MAX_KNOT_DEGREE || l > MAX_KNOT_DEGREE {
        return Err(bad("degree or derivative count too large"));
    }
    let mut o = vec![p as f64, k as f64, l as f64];
    push_reals(&mut o, u);
    run(7, &o)
}

/// `N_FitCalcMatrix(knt, p, t, n, itp, cm)` with `n = t.len() - 1`; `itp` in `0..=3`,
/// `1 <= p <= 17`, searchable knots.
pub fn calc_matrix(knots: &[f64], p: usize, t: &[f64], itp: i32) -> Result<Vec<f64>, FfiError> {
    check_basis_degree(p)?;
    check_knots(knots, p)?;
    check_count(t.len(), "parameters")?;
    check_finite(t, "parameters")?;
    if !(0..=3).contains(&itp) {
        return Err(bad(format!("itp {itp}")));
    }
    let mut o = vec![p as f64, f64::from(itp)];
    push_reals(&mut o, t);
    push_reals(&mut o, knots);
    run(8, &o)
}
