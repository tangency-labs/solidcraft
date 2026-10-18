//! FFI wrappers for unit `nlib.srf_fit`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_fit.cpp` (`sc_nlib_srf_fit_eval`).
//! Upstream: `source/SMLib/src/NL_SrfFit.cpp`.
//!
//! | wrapper                 | upstream                    |
//! |-------------------------|-----------------------------|
//! | [`param_values`]        | `N_FitCalcSrfParamValues`   |
//! | [`interp_params`]       | `N_FitSrfInterpParams`      |
//! | [`interp_given_knots`]  | `N_FitSrfToPtsKnots`        |
//! | [`lst_sq_approx`]       | `N_FitSrfLstSqApprox`       |
//! | [`func_interp`]         | `N_FitSrfFuncInterp`        |
//!
//! Every wrapper returns the shim's flat record: the status (`0` or the `NL_*_ERR`
//! number), then on success the result (parameters `u` then `v`; `count t`;
//! surfaces as `n m p q r s` + control points `x y z w` row-major + u knots + v
//! knots; surface functions likewise with values).
//!
//! Grids are rows of points `[x, y, z, w]` with a kind flag `ptp` (`1` =
//! `NL_EPOINT`, `w` ignored; `2` = `NL_HPOINT`); every row must have the same
//! length. Inputs are validated so upstream only reads and writes memory it is
//! given: counts and degrees in range (degree `p >= 1` where upstream would size a
//! matrix of negative bandwidth or divide by a zero degree), values finite, knots
//! and the interpolation parameters of [`func_interp`] non-decreasing.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest number of grid rows / columns, parameters or knots accepted.
pub const MAX_COUNT: usize = 512;
/// Largest degree accepted (`NL_MAXDEG`).
pub const MAX_DEGREE: usize = 17;

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_fit_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A rectangular grid of points with its kind flag.
#[derive(Debug, Clone, Copy)]
pub struct Grid<'a> {
    /// `1` (`NL_EPOINT`) or `2` (`NL_HPOINT`).
    pub ptp: i32,
    /// Rows of points `[x, y, z, w]`, all of the same length.
    pub rows: &'a [Vec<[f64; 4]>],
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

/// Checks the grid; returns `(rows, cols)`.
fn check_grid(g: &Grid<'_>) -> Result<(usize, usize), FfiError> {
    if g.ptp != 1 && g.ptp != 2 {
        return Err(bad(format!("ptp {}", g.ptp)));
    }
    let rows = g.rows.len();
    check_count(rows, "grid rows")?;
    let cols = g.rows.first().map_or(0, Vec::len);
    check_count(cols, "grid columns")?;
    for row in g.rows {
        if row.len() != cols {
            return Err(bad("grid rows differ in length"));
        }
        for p in row {
            check_finite(p, "grid")?;
        }
    }
    Ok((rows, cols))
}

fn push_grid(o: &mut Vec<f64>, g: &Grid<'_>) {
    o.push(g.rows.len() as f64);
    o.push(g.rows.first().map_or(0, Vec::len) as f64);
    for row in g.rows {
        for p in row {
            o.extend_from_slice(p);
        }
    }
}

fn push_reals(o: &mut Vec<f64>, v: &[f64]) {
    o.push(v.len() as f64);
    o.extend_from_slice(v);
}

fn check_par(par: i32) -> Result<(), FfiError> {
    if (1..=3).contains(&par) {
        Ok(())
    } else {
        Err(bad(format!("par {par}")))
    }
}

fn check_degree(p: usize, min: usize, what: &str) -> Result<(), FfiError> {
    if p < min || p > MAX_DEGREE {
        return Err(bad(format!("{what} {p}")));
    }
    Ok(())
}

/// Runs `op` on an input the caller has validated.
fn run(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly the record
        // the shim reads for `op` (built and validated by the public wrappers,
        // including every count, degree and index the shim passes to upstream); buf
        // is valid for buf.len() writes and the shim honours the capacity; len is
        // valid.
        unsafe {
            sys::sc_nlib_srf_fit_eval(
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

/// `N_FitCalcSrfParamValues(A, n, m, ptp, par, u, v)`; `par` in `1..=3`.
pub fn param_values(g: &Grid<'_>, par: i32) -> Result<Vec<f64>, FfiError> {
    check_grid(g)?;
    check_par(par)?;
    let mut o = vec![f64::from(g.ptp), f64::from(par)];
    push_grid(&mut o, g);
    run(0, &o)
}

/// `N_FitSrfInterpParams(A, n, m, ptp, par, dir, t)`; `par` in `1..=3`, `dir` 1
/// (`NL_UDIR`) or 2 (`NL_VDIR`).
pub fn interp_params(g: &Grid<'_>, par: i32, dir: i32) -> Result<Vec<f64>, FfiError> {
    check_grid(g)?;
    check_par(par)?;
    if dir != 1 && dir != 2 {
        return Err(bad(format!("dir {dir}")));
    }
    let mut o = vec![f64::from(g.ptp), f64::from(par), f64::from(dir)];
    push_grid(&mut o, g);
    run(1, &o)
}

/// `N_FitSrfToPtsKnots(A, n, m, ptp, u, v, knu, knv, p, q, sur)` with one parameter
/// per grid row (`u`) and column (`v`); `1 <= p`, `q <= MAX_DEGREE`; knots
/// non-decreasing.
pub fn interp_given_knots(
    g: &Grid<'_>,
    u: &[f64],
    v: &[f64],
    uk: &[f64],
    vk: &[f64],
    p: usize,
    q: usize,
) -> Result<Vec<f64>, FfiError> {
    let (rows, cols) = check_grid(g)?;
    check_degree(p, 1, "p")?;
    check_degree(q, 0, "q")?;
    if u.len() != rows || v.len() != cols {
        return Err(bad("one parameter per grid row and column"));
    }
    check_finite(u, "u")?;
    check_finite(v, "v")?;
    check_count(uk.len(), "u knots")?;
    check_count(vk.len(), "v knots")?;
    check_sorted(uk, "u knots")?;
    check_sorted(vk, "v knots")?;
    let mut o = vec![f64::from(g.ptp), p as f64, q as f64];
    push_grid(&mut o, g);
    push_reals(&mut o, u);
    push_reals(&mut o, v);
    push_reals(&mut o, uk);
    push_reals(&mut o, vk);
    run(2, &o)
}

/// `N_FitSrfLstSqApprox(P, r, s, n, m, p, q, par, sur)` on a Euclidean grid (`w`
/// ignored); `1 <= p, q <= MAX_DEGREE`, `n`, `m` at most [`MAX_COUNT`], `par` in
/// `1..=3`.
pub fn lst_sq_approx(
    pts: &[Vec<[f64; 4]>],
    n: usize,
    m: usize,
    p: usize,
    q: usize,
    par: i32,
) -> Result<Vec<f64>, FfiError> {
    let g = Grid { ptp: 1, rows: pts };
    check_grid(&g)?;
    check_par(par)?;
    check_degree(p, 1, "p")?;
    check_degree(q, 1, "q")?;
    if n > MAX_COUNT || m > MAX_COUNT {
        return Err(bad("control net size"));
    }
    let mut o = vec![n as f64, m as f64, p as f64, q as f64, f64::from(par)];
    push_grid(&mut o, &g);
    run(3, &o)
}

/// `N_FitSrfFuncInterp(F, n, m, p, q, u, v, sfn)` with `n + 1` rows and `m + 1`
/// columns of values and at least `n + 1` / `m + 1` parameters (the first `n + 1` /
/// `m + 1` non-decreasing); `1 <= p`, `q <= MAX_DEGREE`.
pub fn func_interp(
    f: &[Vec<f64>],
    p: usize,
    q: usize,
    u: &[f64],
    v: &[f64],
) -> Result<Vec<f64>, FfiError> {
    let rows = f.len();
    check_count(rows, "value rows")?;
    let cols = f.first().map_or(0, Vec::len);
    check_count(cols, "value columns")?;
    for row in f {
        if row.len() != cols {
            return Err(bad("value rows differ in length"));
        }
        check_finite(row, "values")?;
    }
    check_degree(p, 1, "p")?;
    check_degree(q, 0, "q")?;
    check_count(u.len(), "u")?;
    check_count(v.len(), "v")?;
    let (Some(ul), Some(vl)) = (u.get(..rows), v.get(..cols)) else {
        return Err(bad("too few parameters"));
    };
    check_finite(u, "u")?;
    check_finite(v, "v")?;
    check_sorted(ul, "u")?;
    check_sorted(vl, "v")?;
    let mut o = vec![p as f64, q as f64, rows as f64, cols as f64];
    for row in f {
        o.extend_from_slice(row);
    }
    push_reals(&mut o, u);
    push_reals(&mut o, v);
    run(4, &o)
}
