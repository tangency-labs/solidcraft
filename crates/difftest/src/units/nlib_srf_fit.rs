//! Runner for unit `nlib.srf_fit`: surface fitting to regular point grids
//! (parameter values, interpolation with given knots, least-squares approximation)
//! and interpolation of a value grid by a surface function.
//!
//! C++: `solidcore_ffi::units::nlib_srf_fit` (upstream
//! `source/SMLib/src/NL_SrfFit.cpp`). Rust: `solidcore::nurbs::srf_fit`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record
//! (see the ffi module), split into keys: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then
//! the result: `u` and `v` (`param_values`), `t` (`interp_params`), or `dims =
//! [n, m, p, q, r, s]`, `cpts` (x y z w, row-major) or `values`, `u_knots` and
//! `v_knots` (surfaces and surface functions).
//!
//! Points are `[x, y, z]` or `[x, y, z, w]` (a missing `w` is 1); `ptp` 1 uses
//! `Vec3` data (`w` ignored), 2 homogeneous [`CPoint`]s. An `interp` case may leave
//! out `u` / `v` (then the chord-length parameters of the grid, computed once by
//! the runner and given to both sides; uniform when that fails) and `u_knots` /
//! `v_knots` (then the interpolation knot vectors of those parameters), as the
//! upstream driver `N_FitSrfToPts` does. Every run has at least
//! [`MIN_GENERATED`] generated cases.

// Harness code: the input builders index their own freshly sized arrays; a panic
// would fail the case (runner panics are caught).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_fit::p1::{interp_knot_vector, DataDir, ParamMethod};
use solidcore::nurbs::funcs_basic::SurfaceFunction;
use solidcore::nurbs::knots::KnotVector;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface;
use solidcore::nurbs::srf_fit as fit;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_fit::{self as ffi, Grid};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.srf_fit` runner.
pub struct Runner;

type GridIn = Vec<Vec<Vec<f64>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    ParamValues {
        ptp: i32,
        grid: GridIn,
        par: i32,
    },
    InterpParams {
        ptp: i32,
        grid: GridIn,
        par: i32,
        dir: i32,
    },
    Interp {
        ptp: i32,
        grid: GridIn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        v: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u_knots: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        v_knots: Option<Vec<f64>>,
        p: usize,
        q: usize,
    },
    Approx {
        grid: GridIn,
        n: usize,
        m: usize,
        p: usize,
        q: usize,
        par: i32,
    },
    Func {
        values: Vec<Vec<f64>>,
        p: usize,
        q: usize,
        u: Vec<f64>,
        v: Vec<f64>,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn harness_err(e: &SolidError) -> HarnessError {
    harness(format!("input preparation failed: {e}"))
}

// ---- inputs -----------------------------------------------------------------------

fn pt4(v: &[f64]) -> Result<[f64; 4], HarnessError> {
    match *v {
        [x, y, z] => Ok([x, y, z, 1.0]),
        [x, y, z, w] => Ok([x, y, z, w]),
        _ => Err(harness(format!("point with {} coordinates", v.len()))),
    }
}

fn grid4(g: &GridIn) -> Result<Vec<Vec<[f64; 4]>>, HarnessError> {
    g.iter()
        .map(|row| row.iter().map(|p| pt4(p)).collect())
        .collect()
}

fn ptp_ok(ptp: i32) -> Result<(), HarnessError> {
    if ptp == 1 || ptp == 2 {
        Ok(())
    } else {
        Err(harness(format!("ptp {ptp}")))
    }
}

fn euclid(p: [f64; 4]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn homo(p: [f64; 4]) -> CPoint {
    CPoint::new(p[0], p[1], p[2], p[3])
}

fn egrid(g: &[Vec<[f64; 4]>]) -> Vec<Vec<Vec3>> {
    g.iter()
        .map(|r| r.iter().copied().map(euclid).collect())
        .collect()
}

fn hgrid(g: &[Vec<[f64; 4]>]) -> Vec<Vec<CPoint>> {
    g.iter()
        .map(|r| r.iter().copied().map(homo).collect())
        .collect()
}

fn par_method(par: i32) -> Result<ParamMethod, HarnessError> {
    match par {
        1 => Ok(ParamMethod::Uniform),
        2 => Ok(ParamMethod::ChordLength),
        3 => Ok(ParamMethod::Centripetal),
        _ => Err(harness(format!("par {par}"))),
    }
}

fn data_dir(dir: i32) -> Result<DataDir, HarnessError> {
    match dir {
        1 => Ok(DataDir::U),
        2 => Ok(DataDir::V),
        _ => Err(harness(format!("dir {dir}"))),
    }
}

/// Keeps derived knots non-decreasing (chord parameters may overshoot by an ulp).
fn monotone(mut uk: Vec<f64>) -> Vec<f64> {
    for i in 1..uk.len() {
        uk[i] = uk[i].max(uk[i - 1]);
    }
    uk
}

/// Chord-length parameters of the grid (uniform when they fail), as
/// `N_FitSrfToPts` computes them.
fn grid_params(ptp: i32, g: &[Vec<[f64; 4]>]) -> (Vec<f64>, Vec<f64>) {
    let r = if ptp == 1 {
        fit::param_values(&egrid(g), ParamMethod::ChordLength)
    } else {
        fit::param_values(&hgrid(g), ParamMethod::ChordLength)
    };
    r.unwrap_or_else(|_| {
        let un = |n: usize| -> Vec<f64> {
            if n == 1 {
                vec![1.0]
            } else {
                (0..n).map(|i| i as f64 / (n - 1) as f64).collect()
            }
        };
        (un(g.len()), un(g.first().map_or(0, Vec::len)))
    })
}

/// Fills in left-out parameters and knots.
fn resolve(i: &Inputs) -> Result<Inputs, HarnessError> {
    match i.clone() {
        Inputs::Interp {
            ptp,
            grid,
            u,
            v,
            u_knots,
            v_knots,
            p,
            q,
        } => {
            ptp_ok(ptp)?;
            let g = grid4(&grid)?;
            let (gu, gv) = if u.is_none() || v.is_none() {
                grid_params(ptp, &g)
            } else {
                (Vec::new(), Vec::new())
            };
            let u = u.unwrap_or(gu);
            let v = v.unwrap_or(gv);
            let u_knots = match u_knots {
                Some(k) => k,
                None => monotone(interp_knot_vector(&u, p).map_err(|e| harness_err(&e))?),
            };
            let v_knots = match v_knots {
                Some(k) => k,
                None => monotone(interp_knot_vector(&v, q).map_err(|e| harness_err(&e))?),
            };
            Ok(Inputs::Interp {
                ptp,
                grid,
                u: Some(u),
                v: Some(v),
                u_knots: Some(u_knots),
                v_knots: Some(v_knots),
                p,
                q,
            })
        }
        other => Ok(other),
    }
}

fn need<'a, T>(x: Option<&'a T>, what: &str) -> Result<&'a T, HarnessError> {
    x.ok_or_else(|| harness(format!("{what} not resolved")))
}

// ---- output record ----------------------------------------------------------------

fn count(x: f64) -> Result<usize, HarnessError> {
    if x >= 0.0 && x.fract() == 0.0 && x < 1e9 {
        Ok(x as usize)
    } else {
        Err(harness(format!("bad count {x}")))
    }
}

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let s = flat
        .get(*at..*at + k)
        .ok_or_else(|| harness("record too short"))?;
    *at += k;
    Ok(s)
}

fn int(x: f64) -> i64 {
    x as i64
}

/// Grid shape `(rows, cols)` of the case's data.
fn shape(i: &Inputs) -> (usize, usize) {
    let g = |g: &GridIn| (g.len(), g.first().map_or(0, Vec::len));
    match i {
        Inputs::ParamValues { grid, .. }
        | Inputs::InterpParams { grid, .. }
        | Inputs::Interp { grid, .. }
        | Inputs::Approx { grid, .. } => g(grid),
        Inputs::Func { values, .. } => (values.len(), values.first().map_or(0, Vec::len)),
    }
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0;
    let st = take(flat, &mut at, 1)?[0];
    let mut out = Output::new().int("status", int(st));
    if st != 0.0 {
        return finish(out, flat, at);
    }
    match i {
        Inputs::ParamValues { .. } => {
            let (rows, cols) = shape(i);
            out = out.values("u", take(flat, &mut at, rows)?.to_vec());
            out = out.values("v", take(flat, &mut at, cols)?.to_vec());
        }
        Inputs::InterpParams { .. } => {
            let c = count(take(flat, &mut at, 1)?[0])?;
            out = out.values("t", take(flat, &mut at, c)?.to_vec());
        }
        Inputs::Interp { .. } | Inputs::Approx { .. } | Inputs::Func { .. } => {
            let d = take(flat, &mut at, 6)?;
            let (n, m, r, s) = (count(d[0])?, count(d[1])?, count(d[4])?, count(d[5])?);
            out = out.ints("dims", d.iter().map(|x| int(*x)).collect::<Vec<_>>());
            if matches!(i, Inputs::Func { .. }) {
                out = out.values("values", take(flat, &mut at, (n + 1) * (m + 1))?.to_vec());
            } else {
                out = out.values("cpts", take(flat, &mut at, 4 * (n + 1) * (m + 1))?.to_vec());
            }
            out = out.values("u_knots", take(flat, &mut at, r + 1)?.to_vec());
            out = out.values("v_knots", take(flat, &mut at, s + 1)?.to_vec());
        }
    }
    finish(out, flat, at)
}

fn finish(out: Output, flat: &[f64], at: usize) -> Result<Output, HarnessError> {
    if at == flat.len() {
        Ok(out)
    } else {
        Err(harness(format!(
            "record has {} extra values",
            flat.len() - at
        )))
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn cpp_flat(i: &Inputs) -> Result<Result<Vec<f64>, FfiError>, HarnessError> {
    Ok(match i {
        Inputs::ParamValues { ptp, grid, par } => {
            let g = grid4(grid)?;
            ffi::param_values(
                &Grid {
                    ptp: *ptp,
                    rows: &g,
                },
                *par,
            )
        }
        Inputs::InterpParams {
            ptp,
            grid,
            par,
            dir,
        } => {
            let g = grid4(grid)?;
            ffi::interp_params(
                &Grid {
                    ptp: *ptp,
                    rows: &g,
                },
                *par,
                *dir,
            )
        }
        Inputs::Interp {
            ptp,
            grid,
            u,
            v,
            u_knots,
            v_knots,
            p,
            q,
        } => {
            let g = grid4(grid)?;
            ffi::interp_given_knots(
                &Grid {
                    ptp: *ptp,
                    rows: &g,
                },
                need(u.as_ref(), "u")?,
                need(v.as_ref(), "v")?,
                need(u_knots.as_ref(), "u_knots")?,
                need(v_knots.as_ref(), "v_knots")?,
                *p,
                *q,
            )
        }
        Inputs::Approx {
            grid,
            n,
            m,
            p,
            q,
            par,
        } => {
            let g = grid4(grid)?;
            ffi::lst_sq_approx(&g, *n, *m, *p, *q, *par)
        }
        Inputs::Func { values, p, q, u, v } => ffi::func_interp(values, *p, *q, u, v),
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match cpp_flat(i)? {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
        Err(e) => Err(harness(format!("shim failure: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(_) => -1.0,
    }
}

fn push_surface(o: &mut Vec<f64>, r: SolidResult<Surface>) {
    o.push(status(&r));
    if let Ok(s) = r {
        let (n, m, rr, ss) = s.array_sizes();
        let (p, q) = s.degrees();
        o.extend([n, m, p, q, rr, ss].map(|x| x as f64));
        for row in s.rows() {
            for pw in row.iter().take(m + 1) {
                o.extend_from_slice(&pw.to_array());
            }
        }
        o.extend(s.u_knots().iter().take(rr + 1));
        o.extend(s.v_knots().iter().take(ss + 1));
    }
}

fn push_sfun(o: &mut Vec<f64>, r: SolidResult<SurfaceFunction>) {
    o.push(status(&r));
    if let Ok(f) = r {
        let (n, m, rr, ss) = f.array_sizes();
        let (p, q) = f.degrees();
        o.extend([n, m, p, q, rr, ss].map(|x| x as f64));
        for row in f.rows() {
            o.extend(row.iter().take(m + 1));
        }
        o.extend(f.u_knots().iter().take(rr + 1));
        o.extend(f.v_knots().iter().take(ss + 1));
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::ParamValues { ptp, grid, par } => {
            ptp_ok(*ptp)?;
            let g = grid4(grid)?;
            let par = par_method(*par)?;
            let r = if *ptp == 1 {
                fit::param_values(&egrid(&g), par)
            } else {
                fit::param_values(&hgrid(&g), par)
            };
            o.push(status(&r));
            if let Ok((u, v)) = r {
                o.extend(u);
                o.extend(v);
            }
        }
        Inputs::InterpParams {
            ptp,
            grid,
            par,
            dir,
        } => {
            ptp_ok(*ptp)?;
            let g = grid4(grid)?;
            let (par, dir) = (par_method(*par)?, data_dir(*dir)?);
            let r = if *ptp == 1 {
                fit::interp_params(&egrid(&g), par, dir)
            } else {
                fit::interp_params(&hgrid(&g), par, dir)
            };
            o.push(status(&r));
            if let Ok(t) = r {
                o.push(t.len() as f64);
                o.extend(t);
            }
        }
        Inputs::Interp {
            ptp,
            grid,
            u,
            v,
            u_knots,
            v_knots,
            p,
            q,
        } => {
            ptp_ok(*ptp)?;
            let g = grid4(grid)?;
            let (u, v) = (need(u.as_ref(), "u")?, need(v.as_ref(), "v")?);
            let knu = KnotVector::from_slice(need(u_knots.as_ref(), "u_knots")?)
                .map_err(|e| harness_err(&e))?;
            let knv = KnotVector::from_slice(need(v_knots.as_ref(), "v_knots")?)
                .map_err(|e| harness_err(&e))?;
            let r = if *ptp == 1 {
                fit::interp_given_knots(&egrid(&g), u, v, &knu, &knv, *p, *q)
            } else {
                fit::interp_given_knots(&hgrid(&g), u, v, &knu, &knv, *p, *q)
            };
            push_surface(&mut o, r);
        }
        Inputs::Approx {
            grid,
            n,
            m,
            p,
            q,
            par,
        } => {
            let g = egrid(&grid4(grid)?);
            push_surface(
                &mut o,
                fit::lst_sq_approx(&g, *n, *m, *p, *q, par_method(*par)?),
            );
        }
        Inputs::Func { values, p, q, u, v } => {
            push_sfun(&mut o, fit::func_interp(values, *p, *q, u, v));
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

fn below(rng: &mut Rng, n: usize) -> usize {
    rng.below(n as u64) as usize
}

/// A surface-like grid: a smooth base sheet plus noise, with occasional repeated
/// rows / columns / points (zero chords) and degenerate (all equal) grids.
fn gen_grid(rng: &mut Rng, ptp: i32, rows: usize, cols: usize) -> GridIn {
    let degenerate = rng.chance(0.03);
    let flat = rng.chance(0.2);
    let origin = [
        rng.uniform(-10.0, 10.0),
        rng.uniform(-10.0, 10.0),
        rng.uniform(-10.0, 10.0),
    ];
    let du = [
        rng.uniform(0.2, 3.0),
        rng.uniform(-1.0, 1.0),
        rng.uniform(-1.0, 1.0),
    ];
    let dv = [
        rng.uniform(-1.0, 1.0),
        rng.uniform(0.2, 3.0),
        rng.uniform(-1.0, 1.0),
    ];
    let amp = rng.uniform(0.0, 2.0);
    let noise = if rng.chance(0.5) {
        rng.uniform(0.0, 0.5)
    } else {
        0.0
    };
    let rep_row = if rows > 2 && rng.chance(0.08) {
        Some(1 + below(rng, rows - 1))
    } else {
        None
    };
    let rep_col = if cols > 2 && rng.chance(0.08) {
        Some(1 + below(rng, cols - 1))
    } else {
        None
    };
    let mut g: GridIn = Vec::with_capacity(rows);
    for i in 0..rows {
        let mut row: Vec<Vec<f64>> = Vec::with_capacity(cols);
        for j in 0..cols {
            if degenerate {
                row.push(origin.to_vec());
                continue;
            }
            let (fi, fj) = (i as f64, j as f64);
            let mut p: Vec<f64> = (0..3)
                .map(|k| origin[k] + fi * du[k] + fj * dv[k] + rng.uniform(-noise, noise))
                .collect();
            p[2] += amp * (0.7 * fi).sin() * (0.5 * fj).cos();
            if flat {
                p[2] = 0.0;
            }
            if rng.chance(0.1) {
                for c in &mut p {
                    *c = c.round();
                }
            }
            row.push(p);
        }
        g.push(row);
    }
    if let Some(r) = rep_row {
        let prev = g[r - 1].clone();
        g[r] = prev;
    }
    if let Some(c) = rep_col {
        for row in &mut g {
            let prev = row[c - 1].clone();
            row[c] = prev;
        }
    }
    if rows * cols > 1 && rng.chance(0.05) {
        // One repeated point.
        let (i, j) = (below(rng, rows), below(rng, cols));
        let (i2, j2) = (below(rng, rows), below(rng, cols));
        let other = g[i2][j2].clone();
        g[i][j] = other;
    }
    if ptp == 2 {
        for row in &mut g {
            for p in row.iter_mut() {
                let w = if rng.chance(0.2) {
                    1.0
                } else {
                    rng.uniform(0.3, 3.0)
                };
                for c in p.iter_mut() {
                    *c *= w;
                }
                p.push(w);
            }
        }
    }
    g
}

fn gen_ptp(rng: &mut Rng) -> i32 {
    if rng.chance(0.6) {
        1
    } else {
        2
    }
}

/// Sorted parameters in `[0, 1]` with `u[0] = 0`, `u[last] = 1` (more than `cnt`
/// entries sometimes: upstream reads only the first `cnt`).
fn gen_params(rng: &mut Rng, cnt: usize) -> Vec<f64> {
    let mut u: Vec<f64> = (0..cnt).map(|_| rng.unit()).collect();
    u.sort_by(f64::total_cmp);
    if let Some(f) = u.first_mut() {
        *f = 0.0;
    }
    if cnt > 1 {
        if let Some(l) = u.last_mut() {
            *l = 1.0;
        }
    }
    if rng.chance(0.15) && cnt > 3 {
        let i = 1 + below(rng, cnt - 2);
        u[i] = u[i - 1];
    }
    if rng.chance(0.1) {
        u.push(rng.unit());
    }
    u
}

/// Random clamped non-decreasing knots over `[0, 1]`.
fn gen_knots(rng: &mut Rng, cnt: usize, p: usize) -> Vec<f64> {
    let mut k: Vec<f64> = (0..cnt).map(|_| rng.unit()).collect();
    k.sort_by(f64::total_cmp);
    for (i, v) in k.iter_mut().enumerate() {
        if i <= p {
            *v = 0.0;
        } else if i + p + 1 >= cnt {
            *v = 1.0;
        }
    }
    k
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let ptp = gen_ptp(rng);
    match below(rng, 10) {
        0 => {
            let rows = if rng.chance(0.05) {
                1
            } else {
                2 + below(rng, 9)
            };
            let cols = if rng.chance(0.05) {
                1
            } else {
                2 + below(rng, 9)
            };
            Inputs::ParamValues {
                ptp,
                grid: gen_grid(rng, ptp, rows, cols),
                par: 1 + below(rng, 3) as i32,
            }
        }
        1 => {
            let rows = if rng.chance(0.05) {
                1
            } else {
                2 + below(rng, 9)
            };
            let cols = if rng.chance(0.05) {
                1
            } else {
                2 + below(rng, 9)
            };
            Inputs::InterpParams {
                ptp,
                grid: gen_grid(rng, ptp, rows, cols),
                par: 1 + below(rng, 3) as i32,
                dir: 1 + below(rng, 2) as i32,
            }
        }
        2..=4 => {
            let rows = 1 + below(rng, 10);
            let cols = 1 + below(rng, 10);
            let (n, m) = (rows - 1, cols - 1);
            let mut p = 1 + below(rng, n.clamp(1, 5));
            let mut q = below(rng, m.clamp(1, 5) + 1);
            if rng.chance(0.05) {
                p = n + 1;
            }
            if rng.chance(0.03) {
                q = m + 1;
            }
            let explicit = rng.chance(0.1);
            let (u, v, u_knots, v_knots) = if explicit {
                let extra = usize::from(rng.chance(0.3));
                (
                    Some(gen_params(rng, rows)[..rows].to_vec()),
                    Some(gen_params(rng, cols)[..cols].to_vec()),
                    Some(gen_knots(rng, n + p + 2 + extra, p)),
                    Some(gen_knots(rng, m + q + 2, q)),
                )
            } else {
                (None, None, None, None)
            };
            Inputs::Interp {
                ptp,
                grid: gen_grid(rng, ptp, rows, cols),
                u,
                v,
                u_knots,
                v_knots,
                p,
                q,
            }
        }
        5..=7 => {
            let rows = 2 + below(rng, 14);
            let cols = 2 + below(rng, 14);
            let (r, s) = (rows - 1, cols - 1);
            let mut n = 1 + below(rng, r);
            let mut m = 1 + below(rng, s);
            if rng.chance(0.03) {
                n = r + 1;
            }
            if rng.chance(0.03) {
                m = 0;
            }
            let p = 1 + below(rng, n.clamp(1, 5));
            let q = 1 + below(rng, m.clamp(1, 5));
            Inputs::Approx {
                grid: gen_grid(rng, 1, rows, cols),
                n,
                m,
                p,
                q,
                par: 1 + below(rng, 3) as i32,
            }
        }
        _ => {
            let rows = 1 + below(rng, 10);
            let cols = 1 + below(rng, 10);
            let (n, m) = (rows - 1, cols - 1);
            let mut p = 1 + below(rng, n.clamp(1, 5));
            let q = below(rng, m.clamp(1, 5) + 1);
            if rng.chance(0.05) {
                p = n + 1;
            }
            let values = (0..rows)
                .map(|_| (0..cols).map(|_| rng.uniform(-10.0, 10.0)).collect())
                .collect();
            Inputs::Func {
                values,
                p,
                q,
                u: gen_params(rng, rows),
                v: gen_params(rng, cols),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_fit"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&resolve(&case.parse()?)?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&resolve(&case.parse()?)?)
    }
}
