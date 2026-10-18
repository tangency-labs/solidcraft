//! FFI wrappers for unit `nlib.crv_fit_p1`.
//!
//! Shim: `harness/cpp/shim/nlib_crv_fit_p1.cpp` (`sc_nlib_crv_fit_p1_eval`).
//! Upstream: `source/SMLib/src/NL_CrvFit.cpp`.
//!
//! | wrapper                   | upstream                         |
//! |---------------------------|----------------------------------|
//! | [`param_values`]          | `N_FitCalcCrvParamValues`        |
//! | [`interp_knot_vector`]    | `N_FitCrvCalcKnotVector`         |
//! | [`knot_vector_end_derivs`]| `N_FitCalcKnotVectorEndDerivs`   |
//! | [`knot_vector_deriv`]     | `N_FitCalcKnotVectorDeriv`       |
//! | [`knot_vector_approx`]    | `N_FitCalcKnotVectorCrvApprox`   |
//! | [`interp_given_params`]   | `N_FitCrvInterpGivenParams`      |
//! | [`knots_and_derivs`]      | `N_FitCrvKnotsAndDerivs`         |
//! | [`knots_and_deriv`]       | `N_FitCrvKnotsAndDeriv`          |
//! | [`cpts_from_srf_data`]    | `N_FitCrvCPtsFromSrfData`        |
//! | [`approx_knots`]          | `N_FitCrvApproxKnots`            |
//! | [`derivs_matrix`]         | `N_FitCrvDerivsMatrix`           |
//! | [`first_deriv_and_knots`] | `N_FitCrvFirstDerivAndKnots`     |
//!
//! Every wrapper returns the shim's flat record: the status (`0` or the `NL_*_ERR`
//! number), then on success the result (knot / parameter vectors as `count
//! values`, curves as `n p m` + control points `x y z w` + knots, control point
//! arrays as `count` + points). The two matrix routines first write the status of
//! the LU decomposition of the given matrix (and stop when it is nonzero).
//!
//! Points are `[x, y, z, w]` with a kind flag `ptp` (`1` = `NL_EPOINT`, `w`
//! ignored; `2` = `NL_HPOINT`). Inputs are validated so upstream only reads and
//! writes memory it is given: counts, indices and degrees must be in range,
//! parameters and knots finite, knots non-decreasing, and the interior parameters
//! of [`approx_knots`] non-decreasing (its band bookkeeping assumes it).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest point / parameter / knot count accepted.
pub const MAX_COUNT: usize = 4096;
/// Largest degree accepted where upstream evaluates basis functions (`NL_MAXDEG`).
pub const MAX_BASIS_DEGREE: usize = 17;
/// Largest degree accepted by the knot vector routines.
pub const MAX_KNOT_DEGREE: usize = 64;

mod sys {
    extern "C" {
        pub fn sc_nlib_crv_fit_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
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
    check_count(a.pts.len(), what)
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
    Ok(())
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
            sys::sc_nlib_crv_fit_p1_eval(
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

/// `N_FitCalcCrvParamValues(A, n, ptp, par, u)` with `n = len - 1`; `par` in `1..=3`.
pub fn param_values(a: &Pts<'_>, par: i32) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    if !(1..=3).contains(&par) {
        return Err(bad(format!("par {par}")));
    }
    let mut o = vec![f64::from(a.ptp), f64::from(par)];
    o.push(a.pts.len() as f64);
    for p in a.pts {
        o.extend_from_slice(p);
    }
    run(0, &o)
}

fn knot_input(u: &[f64], p: usize) -> Result<(), FfiError> {
    check_count(u.len(), "parameters")?;
    check_finite(u, "parameters")?;
    if p > MAX_KNOT_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    Ok(())
}

/// `N_FitCrvCalcKnotVector(u, n, p, knt)` with `n = len - 1`.
pub fn interp_knot_vector(u: &[f64], p: usize) -> Result<Vec<f64>, FfiError> {
    knot_input(u, p)?;
    let mut o = vec![p as f64];
    push_reals(&mut o, u);
    run(1, &o)
}

/// `N_FitCalcKnotVectorEndDerivs(u, k, p, knt)` with `k = len - 1`.
pub fn knot_vector_end_derivs(u: &[f64], p: usize) -> Result<Vec<f64>, FfiError> {
    knot_input(u, p)?;
    if p == 1 && u.len() < 2 {
        return Err(bad("p == 1 needs two parameters"));
    }
    let mut o = vec![p as f64];
    push_reals(&mut o, u);
    run(2, &o)
}

/// `N_FitCalcKnotVectorDeriv(u, k, p, whr, knt)` with `k = len - 1`; `whr` 1 or 2.
pub fn knot_vector_deriv(u: &[f64], p: usize, whr: i32) -> Result<Vec<f64>, FfiError> {
    knot_input(u, p)?;
    if whr != 1 && whr != 2 {
        return Err(bad(format!("whr {whr}")));
    }
    if p == 1 && u.len() < 2 {
        return Err(bad("p == 1 needs two parameters"));
    }
    let mut o = vec![p as f64, f64::from(whr)];
    push_reals(&mut o, u);
    run(3, &o)
}

/// `N_FitCalcKnotVectorCrvApprox(u, m, n, p, knt)` with `m = len - 1`.
pub fn knot_vector_approx(u: &[f64], n: usize, p: usize) -> Result<Vec<f64>, FfiError> {
    knot_input(u, p)?;
    if n > MAX_COUNT {
        return Err(bad(format!("n {n}")));
    }
    let mut o = vec![n as f64, p as f64];
    push_reals(&mut o, u);
    run(4, &o)
}

/// Common checks of the interpolation routines: degree, finite parameters and
/// non-decreasing finite knots, at least `need_u` parameters.
fn fit_input(u: &[f64], knots: &[f64], p: usize, need_u: usize) -> Result<(), FfiError> {
    if p > MAX_BASIS_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    check_count(knots.len(), "knots")?;
    check_sorted(knots, "knots")?;
    check_count(u.len(), "parameters")?;
    check_finite(u, "parameters")?;
    if u.len() < need_u {
        return Err(bad("too few parameters"));
    }
    Ok(())
}

/// `N_FitCrvInterpGivenParams(A, n, ptp, u, knt, p, cur)` with `n = len - 1`.
/// Requires `n >= p` for `p >= 2` (fewer knots than `N_BasisEval` reads).
pub fn interp_given_params(
    a: &Pts<'_>,
    u: &[f64],
    knots: &[f64],
    p: usize,
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    let n = a.pts.len() - 1;
    fit_input(u, knots, p, n + 1)?;
    if p >= 2 && n < p {
        return Err(bad("n < p"));
    }
    let mut o = vec![p as f64];
    push_pts(&mut o, a);
    push_reals(&mut o, u);
    push_reals(&mut o, knots);
    run(5, &o)
}

/// `N_FitCrvKnotsAndDerivs(A, kk, ptp, u, knt, p, As, Ae, cur)` with `kk = len - 1`.
pub fn knots_and_derivs(
    a: &Pts<'_>,
    u: &[f64],
    knots: &[f64],
    p: usize,
    ds: [f64; 4],
    de: [f64; 4],
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    fit_input(u, knots, p, a.pts.len())?;
    let mut o = vec![p as f64];
    push_pts(&mut o, a);
    push_reals(&mut o, u);
    push_reals(&mut o, knots);
    o.extend_from_slice(&ds);
    o.extend_from_slice(&de);
    run(6, &o)
}

/// `N_FitCrvKnotsAndDeriv(A, kk, ptp, u, knt, p, T, whr, cur)` with `kk = len - 1`;
/// `whr` 1 or 2.
pub fn knots_and_deriv(
    a: &Pts<'_>,
    u: &[f64],
    knots: &[f64],
    p: usize,
    t: [f64; 4],
    whr: i32,
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    if whr != 1 && whr != 2 {
        return Err(bad(format!("whr {whr}")));
    }
    fit_input(u, knots, p, a.pts.len())?;
    let mut o = vec![p as f64, f64::from(whr)];
    push_pts(&mut o, a);
    push_reals(&mut o, u);
    push_reals(&mut o, knots);
    o.extend_from_slice(&t);
    run(7, &o)
}

/// `N_FitCrvCPtsFromSrfData(A, n, m, ptp, k, dir, cm, Qw)` on the grid `grid`
/// (rows `0..=n`, each with `m + 1` points) after LU-decomposing `cm`; `dir` 1
/// (`NL_UDIR`) or 2 (`NL_VDIR`). The matrix may not exceed the extracted column
/// (`n`) or row (`m`).
pub fn cpts_from_srf_data(
    ptp: i32,
    grid: &[Vec<[f64; 4]>],
    k: usize,
    dir: i32,
    cm: &Matrix<'_>,
) -> Result<Vec<f64>, FfiError> {
    check_ptp(ptp)?;
    check_count(grid.len(), "grid rows")?;
    let cols = grid.first().map_or(0, Vec::len);
    check_count(cols, "grid columns")?;
    if grid.iter().any(|r| r.len() != cols) || grid.len() * cols > MAX_COUNT {
        return Err(bad("grid shape"));
    }
    let len = match dir {
        1 => grid.len(),
        2 => cols,
        d => return Err(bad(format!("dir {d}"))),
    };
    let other = if dir == 1 { cols } else { grid.len() };
    if k >= other {
        return Err(bad("k out of range"));
    }
    check_matrix(cm)?;
    if cm.n >= len {
        return Err(bad("matrix larger than the extracted data"));
    }
    let mut o = vec![
        f64::from(ptp),
        f64::from(dir),
        k as f64,
        grid.len() as f64,
        cols as f64,
    ];
    for row in grid {
        for p in row {
            o.extend_from_slice(p);
        }
    }
    push_matrix(&mut o, cm);
    run(8, &o)
}

/// `N_FitCrvApproxKnots(A, m, ptp, u, knt, n, p, cur)` with `m = len - 1`.
/// Requires `n >= 1`, `p >= 1`, at least `n + p + 2` knots, two points for `n == 1`,
/// and non-decreasing parameters `u[1..m]` (the samples upstream evaluates).
pub fn approx_knots(
    a: &Pts<'_>,
    u: &[f64],
    knots: &[f64],
    n: usize,
    p: usize,
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    let m = a.pts.len() - 1;
    if n == 0 || p == 0 || n > MAX_COUNT {
        return Err(bad("n and p must be at least 1"));
    }
    fit_input(u, knots, p, m + 1)?;
    // The band bookkeeping needs the spans of the evaluated samples u[1..m] in order.
    check_sorted(u.get(1..m).unwrap_or(&[]), "parameters")?;
    if n >= p && knots.len() < n + p + 2 {
        return Err(bad("too few knots"));
    }
    if n == 1 && m < 1 {
        return Err(bad("n == 1 needs two points"));
    }
    let mut o = vec![n as f64, p as f64];
    push_pts(&mut o, a);
    push_reals(&mut o, u);
    push_reals(&mut o, knots);
    run(9, &o)
}

/// `N_FitCrvDerivsMatrix(A, k, ptp, p, knt, As, Ae, cm, Pw)` with `k = len - 1`,
/// after LU-decomposing `cm` (at most `k + 3` rows).
pub fn derivs_matrix(
    a: &Pts<'_>,
    p: usize,
    knots: &[f64],
    ds: [f64; 4],
    de: [f64; 4],
    cm: &Matrix<'_>,
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    let k = a.pts.len() - 1;
    if p > MAX_KNOT_DEGREE {
        return Err(bad(format!("degree {p}")));
    }
    check_count(knots.len(), "knots")?;
    check_finite(knots, "knots")?;
    if knots.len() < k + p + 3 {
        return Err(bad("too few knots"));
    }
    check_matrix(cm)?;
    if cm.n > k + 2 {
        return Err(bad("matrix larger than k + 3"));
    }
    let mut o = vec![p as f64];
    push_pts(&mut o, a);
    push_reals(&mut o, knots);
    o.extend_from_slice(&ds);
    o.extend_from_slice(&de);
    push_matrix(&mut o, cm);
    run(10, &o)
}

/// `N_FitCrvFirstDerivAndKnots(A, B, kk, ptp, u, knt, p, cur)` with `kk = len - 1`;
/// `d` has the kind of `a` and at least as many entries.
pub fn first_deriv_and_knots(
    a: &Pts<'_>,
    d: &Pts<'_>,
    u: &[f64],
    knots: &[f64],
    p: usize,
) -> Result<Vec<f64>, FfiError> {
    check_pts(a, "points")?;
    check_pts(d, "derivatives")?;
    if d.ptp != a.ptp || d.pts.len() < a.pts.len() {
        return Err(bad("derivatives do not match the points"));
    }
    fit_input(u, knots, p, a.pts.len())?;
    let mut o = vec![p as f64];
    push_pts(&mut o, a);
    push_pts(&mut o, d);
    push_reals(&mut o, u);
    push_reals(&mut o, knots);
    run(11, &o)
}
