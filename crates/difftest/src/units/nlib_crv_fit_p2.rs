//! Runner for unit `nlib.crv_fit_p2`: curve fitting part 2 (function and curve
//! interpolation, least-squares approximation, weighted / constrained least squares
//! with given knots, knot vectors for interpolation with derivatives, the
//! interpolation matrix, and solves with a given LU-decomposed matrix).
//!
//! C++: `solidcore_ffi::units::nlib_crv_fit_p2` (upstream
//! `source/SMLib/src/NL_CrvFit.cpp`). Rust: `solidcore::nurbs::crv_fit::p2`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record
//! (see the ffi module), split into keys: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), for
//! the given-matrix ops first `lu_status`, then the result: `values` (knot vectors),
//! `dims = [n, p, m]`, `cpts` (x y z w) and `knots` (curves), `dims`, `values` and
//! `knots` (the curve function), `count` and `cpts` (control point arrays), or
//! `dims = [n, bw]` and `storage` (the matrix, band columns).
//!
//! Points are `[x, y, z]` or `[x, y, z, w]` (a missing `w` is 1); `ptp` 1 uses
//! `Vec3` data (`w` ignored), 2 homogeneous [`CPoint`]s. A case may leave out `u`
//! (then chord-length parameters of the points, computed once by the runner and
//! given to both sides), `knots` (then the knot vector the matching upstream knot
//! routine builds from `u`, as the upstream drivers do) and, for the given-matrix
//! ops, `matrix` (then the un-decomposed interpolation matrix for `u` and the knots,
//! built here exactly as `N_FitCalcMatrix` fills it). Every run has at least
//! [`MIN_GENERATED`] generated cases.

// Harness code: the input builders index their own freshly sized arrays; a panic
// would fail the case (runner panics are caught).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::{self, DerivEnd, ParamMethod};
use solidcore::nurbs::crv_fit::p2::{self as fit, MatrixEnds, WeightedData};
use solidcore::nurbs::funcs_basic::CurveFunction;
use solidcore::nurbs::knots::{KnotVector, Side};
use solidcore::nurbs::math::{CPoint, MatrixType, RealMatrix};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_crv_fit_p2::{self as ffi, Matrix, Pts, Weighted};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_fit_p2` runner.
pub struct Runner;

/// A square matrix by storage (LU-decomposed by both sides before use).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatrixIn {
    n: usize,
    mtp: i32,
    bw: usize,
    storage: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    FuncInterp {
        f: Vec<f64>,
        u: Vec<f64>,
        p: usize,
    },
    Interp {
        pts: Vec<Vec<f64>>,
        p: usize,
        par: i32,
    },
    LstSq {
        pts: Vec<Vec<f64>>,
        n: usize,
        p: usize,
        par: i32,
    },
    CrvMatrix {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        p: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        matrix: Option<MatrixIn>,
    },
    DerivMatrix {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        p: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        t: Vec<f64>,
        whr: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        matrix: Option<MatrixIn>,
    },
    Weighted {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        wp: Vec<f64>,
        #[serde(default)]
        d: Vec<Vec<f64>>,
        #[serde(default)]
        wd: Vec<f64>,
        #[serde(default)]
        idx: Vec<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        n: usize,
        p: usize,
    },
    KnotsDerivs {
        u: Vec<f64>,
        p: usize,
    },
    HighEndKnots {
        u: Vec<f64>,
        p: usize,
        k: usize,
        l: usize,
    },
    CalcMatrix {
        u: Vec<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        p: usize,
        itp: i32,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

fn pt4(v: &[f64]) -> Result<[f64; 4], HarnessError> {
    match *v {
        [x, y, z] => Ok([x, y, z, 1.0]),
        [x, y, z, w] => Ok([x, y, z, w]),
        _ => Err(harness(format!("point with {} coordinates", v.len()))),
    }
}

fn pts4(v: &[Vec<f64>]) -> Result<Vec<[f64; 4]>, HarnessError> {
    v.iter().map(|p| pt4(p)).collect()
}

fn pts3(v: &[Vec<f64>]) -> Result<Vec<[f64; 3]>, HarnessError> {
    v.iter()
        .map(|p| pt4(p).map(|q| [q[0], q[1], q[2]]))
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

fn harness_err(e: &SolidError) -> HarnessError {
    harness(format!("input preparation failed: {e}"))
}

/// Chord-length parameters of the points (Euclidean or homogeneous by `ptp`).
fn chord_params(ptp: i32, pts: &[[f64; 4]]) -> Result<Vec<f64>, HarnessError> {
    let r = if ptp == 1 {
        let e: Vec<Vec3> = pts.iter().copied().map(euclid).collect();
        p1::param_values(&e, ParamMethod::ChordLength)
    } else {
        let h: Vec<CPoint> = pts.iter().copied().map(homo).collect();
        p1::param_values(&h, ParamMethod::ChordLength)
    };
    r.map_err(|e| harness_err(&e))
}

/// Chord-length parameters may overshoot `u[n]` by an ulp before the last one, and
/// knots derived from them with it; keeps derived values non-decreasing (the ffi
/// wrapper requires it).
fn monotone(mut uk: Vec<f64>) -> Vec<f64> {
    for i in 1..uk.len() {
        uk[i] = uk[i].max(uk[i - 1]);
    }
    uk
}

fn deriv_end(whr: i32) -> Result<DerivEnd, HarnessError> {
    match whr {
        1 => Ok(DerivEnd::Start),
        2 => Ok(DerivEnd::End),
        _ => Err(harness(format!("whr {whr}"))),
    }
}

fn matrix_ends(itp: i32) -> Result<MatrixEnds, HarnessError> {
    match itp {
        0 => Ok(MatrixEnds::None),
        1 => Ok(MatrixEnds::Start),
        2 => Ok(MatrixEnds::End),
        3 => Ok(MatrixEnds::Both),
        _ => Err(harness(format!("itp {itp}"))),
    }
}

/// The knot vector the upstream drivers build for `N_FitCalcMatrix` with `itp`.
fn knots_for(u: &[f64], p: usize, itp: i32) -> Result<Vec<f64>, HarnessError> {
    let r = match itp {
        0 => p1::interp_knot_vector(u, p),
        1 => p1::knot_vector_deriv(u, p, DerivEnd::Start),
        2 => p1::knot_vector_deriv(u, p, DerivEnd::End),
        _ => p1::knot_vector_end_derivs(u, p),
    };
    r.map(monotone).map_err(|e| harness_err(&e))
}

/// The un-decomposed banded matrix `N_FitCalcMatrix` builds (row-major storage,
/// row pointers into one block, so `A[i][-1]` is the last entry of row `i - 1`);
/// `None` where upstream would stop with an error.
fn interp_storage(knots: &[f64], p: usize, t: &[f64], itp: i32) -> Option<MatrixIn> {
    let knt = KnotVector::from_slice(knots).ok()?;
    let n = t.len() as i64 - 1;
    let pi = p as i64;
    let k = match itp {
        0 => n,
        1 | 2 => n + 1,
        _ => n + 2,
    };
    let (bw, sbw) = if 2 * pi <= k {
        (2 * pi + 1, pi)
    } else {
        (2 * pi - 1, pi - 1)
    };
    if bw < 1 || k < 0 {
        return None;
    }
    let sp = sbw - pi;
    let cols = if bw == 1 { 2 } else { bw };
    let mut s = vec![0.0; ((k + 1) * cols) as usize];
    let mut set = |i: i64, j: i64, v: f64| -> Option<()> {
        let f = usize::try_from(i * cols + j).ok()?;
        *s.get_mut(f)? = v;
        Some(())
    };
    let row = |set: &mut dyn FnMut(i64, i64, f64) -> Option<()>, i: i64, ti: f64| -> Option<()> {
        let b = knt.basis_eval(p, ti, Side::Left).ok()?;
        let l = sp - i + b.span as i64;
        if l < 0 || l + pi >= bw {
            return None;
        }
        for (kk, v) in b.values.iter().enumerate() {
            set(i, l + kk as i64, *v)?;
        }
        Some(())
    };
    match itp {
        0 => {
            set(0, sbw, 1.0)?;
            set(n, sbw, 1.0)?;
            for i in 1..n {
                row(&mut set, i, t[i as usize])?;
            }
        }
        1 => {
            set(0, sbw, 1.0)?;
            set(1, sbw - 1, -1.0)?;
            set(1, sbw, 1.0)?;
            set(n + 1, sbw, 1.0)?;
            for i in 2..=n {
                row(&mut set, i, t[i as usize - 1])?;
            }
        }
        2 => {
            set(0, sbw, 1.0)?;
            set(n, sbw, -1.0)?;
            set(n, sbw + 1, 1.0)?;
            set(n + 1, sbw, 1.0)?;
            for i in 1..n {
                row(&mut set, i, t[i as usize])?;
            }
        }
        _ => {
            set(0, sbw, 1.0)?;
            set(1, sbw - 1, -1.0)?;
            set(1, sbw, 1.0)?;
            set(n + 1, sbw, -1.0)?;
            set(n + 1, sbw + 1, 1.0)?;
            set(n + 2, sbw, 1.0)?;
            for i in 2..=n {
                row(&mut set, i, t[i as usize - 1])?;
            }
        }
    }
    Some(MatrixIn {
        n: k as usize,
        mtp: 3,
        bw: bw as usize,
        storage: s,
    })
}

/// The full identity matrix with highest index `n` (stand-in where upstream would
/// refuse to build the interpolation matrix).
fn identity(n: usize) -> MatrixIn {
    let mut storage = vec![0.0; (n + 1) * (n + 1)];
    for i in 0..=n {
        storage[i * (n + 1) + i] = 1.0;
    }
    MatrixIn {
        n,
        mtp: 0,
        bw: n + 1,
        storage,
    }
}

/// Fills in left-out `u`, `knots` and matrices.
fn resolve(i: &Inputs) -> Result<Inputs, HarnessError> {
    let out = match i.clone() {
        Inputs::CrvMatrix {
            ptp,
            pts,
            p,
            matrix,
        } => {
            let matrix = if let Some(m) = matrix {
                m
            } else {
                let u = chord_params(ptp, &pts4(&pts)?)?;
                let knots = knots_for(&u, p, 0)?;
                interp_storage(&knots, p, &u, 0).unwrap_or_else(|| identity(pts.len() - 1))
            };
            Inputs::CrvMatrix {
                ptp,
                pts,
                p,
                matrix: Some(matrix),
            }
        }
        Inputs::DerivMatrix {
            ptp,
            pts,
            p,
            knots,
            t,
            whr,
            matrix,
        } => {
            let u = chord_params(ptp, &pts4(&pts)?)?;
            let knots = match knots {
                Some(k) => k,
                None => knots_for(&u, p, whr)?,
            };
            let matrix = match matrix {
                Some(m) => m,
                None => interp_storage(&knots, p, &u, whr).unwrap_or_else(|| identity(pts.len())),
            };
            Inputs::DerivMatrix {
                ptp,
                pts,
                p,
                knots: Some(knots),
                t,
                whr,
                matrix: Some(matrix),
            }
        }
        Inputs::Weighted {
            ptp,
            pts,
            wp,
            d,
            wd,
            idx,
            u,
            knots,
            n,
            p,
        } => {
            let u = match u {
                Some(u) => u,
                None => monotone(chord_params(ptp, &pts4(&pts)?)?),
            };
            let knots = match knots {
                Some(k) => k,
                None => monotone(p1::knot_vector_approx(&u, n, p).map_err(|e| harness_err(&e))?),
            };
            Inputs::Weighted {
                ptp,
                pts,
                wp,
                d,
                wd,
                idx,
                u: Some(u),
                knots: Some(knots),
                n,
                p,
            }
        }
        Inputs::CalcMatrix { u, knots, p, itp } => {
            let knots = match knots {
                Some(k) => k,
                None => knots_for(&u, p, itp)?,
            };
            Inputs::CalcMatrix {
                u,
                knots: Some(knots),
                p,
                itp,
            }
        }
        other => other,
    };
    Ok(out)
}

fn need<'a, T>(v: Option<&'a T>, what: &str) -> Result<&'a T, HarnessError> {
    v.ok_or_else(|| harness(format!("{what} not resolved")))
}

// ---- output -----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Values,
    Curve,
    Function,
    CPoints,
    Matrix,
}

fn shape_of(i: &Inputs) -> (bool, Shape) {
    match i {
        Inputs::KnotsDerivs { .. } | Inputs::HighEndKnots { .. } => (false, Shape::Values),
        Inputs::FuncInterp { .. } => (false, Shape::Function),
        Inputs::CrvMatrix { .. } | Inputs::DerivMatrix { .. } => (true, Shape::CPoints),
        Inputs::CalcMatrix { .. } => (false, Shape::Matrix),
        _ => (false, Shape::Curve),
    }
}

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

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let (has_lu, shape) = shape_of(i);
    let mut at = 0;
    let mut out = Output::new();
    if has_lu {
        let lu = take(flat, &mut at, 1)?[0];
        out = out.int("lu_status", int(lu));
        if lu != 0.0 {
            return finish(out, flat, at);
        }
    }
    let st = take(flat, &mut at, 1)?[0];
    out = out.int("status", int(st));
    if st != 0.0 {
        return finish(out, flat, at);
    }
    match shape {
        Shape::Values => {
            let c = count(take(flat, &mut at, 1)?[0])?;
            out = out.values("values", take(flat, &mut at, c)?.to_vec());
        }
        Shape::Curve | Shape::Function => {
            let d = take(flat, &mut at, 3)?;
            let (n, m) = (count(d[0])?, count(d[2])?);
            out = out.ints("dims", d.iter().map(|x| int(*x)).collect::<Vec<_>>());
            if shape == Shape::Curve {
                out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
            } else {
                out = out.values("values", take(flat, &mut at, n + 1)?.to_vec());
            }
            out = out.values("knots", take(flat, &mut at, m + 1)?.to_vec());
        }
        Shape::CPoints => {
            let c = count(take(flat, &mut at, 1)?[0])?;
            out = out.int("count", c as i64);
            out = out.values("cpts", take(flat, &mut at, 4 * c)?.to_vec());
        }
        Shape::Matrix => {
            let d = take(flat, &mut at, 3)?;
            out = out.ints("dims", vec![int(d[0]), int(d[1])]);
            let c = count(d[2])?;
            out = out.values("storage", take(flat, &mut at, c)?.to_vec());
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

fn matrix(m: &MatrixIn) -> Matrix<'_> {
    Matrix {
        n: m.n,
        mtp: m.mtp,
        bw: m.bw,
        storage: &m.storage,
    }
}

fn cpp_flat(i: &Inputs) -> Result<Result<Vec<f64>, FfiError>, HarnessError> {
    Ok(match i {
        Inputs::FuncInterp { f, u, p } => ffi::func_interp(f, *p, u),
        Inputs::Interp { pts, p, par } => ffi::interp(&pts3(pts)?, *p, *par),
        Inputs::LstSq { pts, n, p, par } => ffi::approx_lst_sq(&pts3(pts)?, *n, *p, *par),
        Inputs::CrvMatrix {
            ptp,
            pts,
            p,
            matrix: m,
        } => {
            let a = pts4(pts)?;
            ffi::crv_matrix(
                &Pts { ptp: *ptp, pts: &a },
                *p,
                &matrix(need(m.as_ref(), "matrix")?),
            )
        }
        Inputs::DerivMatrix {
            ptp,
            pts,
            p,
            knots,
            t,
            whr,
            matrix: m,
        } => {
            let a = pts4(pts)?;
            ffi::deriv_matrix(
                &Pts { ptp: *ptp, pts: &a },
                *p,
                need(knots.as_ref(), "knots")?,
                pt4(t)?,
                *whr,
                &matrix(need(m.as_ref(), "matrix")?),
            )
        }
        Inputs::Weighted {
            ptp,
            pts,
            wp,
            d,
            wd,
            idx,
            u,
            knots,
            n,
            p,
        } => {
            let a = pts4(pts)?;
            let dd = pts4(d)?;
            ffi::weighted_lst_sq(
                &Weighted {
                    pts: Pts { ptp: *ptp, pts: &a },
                    wp,
                    derivs: &dd,
                    wd,
                    index: idx,
                },
                need(u.as_ref(), "u")?,
                need(knots.as_ref(), "knots")?,
                *n,
                *p,
            )
        }
        Inputs::KnotsDerivs { u, p } => ffi::knots_derivs(u, *p),
        Inputs::HighEndKnots { u, p, k, l } => ffi::knots_high_end_derivs(u, *p, *k, *l),
        Inputs::CalcMatrix { u, knots, p, itp } => {
            ffi::calc_matrix(need(knots.as_ref(), "knots")?, *p, u, *itp)
        }
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

fn push_values(o: &mut Vec<f64>, r: SolidResult<Vec<f64>>) {
    o.push(status(&r));
    if let Ok(v) = r {
        o.push(v.len() as f64);
        o.extend(v);
    }
}

fn push_curve(o: &mut Vec<f64>, r: SolidResult<Curve>) {
    o.push(status(&r));
    if let Ok(c) = r {
        let (n, m) = c.array_sizes();
        o.extend([n as f64, c.degree() as f64, m as f64]);
        for pw in c.cpts().iter().take(n + 1) {
            o.extend_from_slice(&pw.to_array());
        }
        o.extend(c.knots().iter().take(m + 1));
    }
}

fn push_function(o: &mut Vec<f64>, r: SolidResult<CurveFunction>) {
    o.push(status(&r));
    if let Ok(c) = r {
        o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
        o.extend(c.values().iter().take(c.n() + 1));
        o.extend(c.knots().iter().take(c.m() + 1));
    }
}

fn push_cpts(o: &mut Vec<f64>, r: SolidResult<Vec<CPoint>>) {
    o.push(status(&r));
    if let Ok(v) = r {
        o.push(v.len() as f64);
        for pw in v {
            o.extend_from_slice(&pw.to_array());
        }
    }
}

fn push_matrix(o: &mut Vec<f64>, r: SolidResult<RealMatrix>) -> Result<(), HarnessError> {
    o.push(status(&r));
    if let Ok(cm) = r {
        let (rows, cols) = cm
            .storage_shape()
            .ok_or_else(|| harness("matrix storage"))?;
        // Only the band column for bandwidth 1 (see the shim).
        let cols = if cm.bandwidth() == 1 { 1 } else { cols };
        o.extend([cm.n() as f64, cm.bandwidth() as f64, (rows * cols) as f64]);
        for i in 0..rows {
            for j in 0..cols {
                o.push(cm.get(i, j).map_err(|e| harness_err(&e))?);
            }
        }
    }
    Ok(())
}

fn par_method(par: i32) -> Result<ParamMethod, HarnessError> {
    match par {
        1 => Ok(ParamMethod::Uniform),
        2 => Ok(ParamMethod::ChordLength),
        3 => Ok(ParamMethod::Centripetal),
        _ => Err(harness(format!("par {par}"))),
    }
}

fn knot_vector(k: &[f64]) -> Result<KnotVector<'_>, HarnessError> {
    KnotVector::from_slice(k).map_err(|e| harness_err(&e))
}

/// Builds and LU-decomposes the matrix; `Err(status)` when the decomposition fails.
fn rust_matrix(m: &MatrixIn) -> Result<Result<RealMatrix, f64>, HarnessError> {
    let mtp = MatrixType::from_code(m.mtp).ok_or_else(|| harness("matrix type"))?;
    let mut cm = RealMatrix::new(m.n, m.n, mtp, m.bw).map_err(|e| harness_err(&e))?;
    let (_, cols) = cm
        .storage_shape()
        .ok_or_else(|| harness("matrix storage"))?;
    for (k, v) in m.storage.iter().enumerate() {
        cm.set(k / cols, k % cols, *v)
            .map_err(|e| harness_err(&e))?;
    }
    let r = cm.lu_decompose();
    Ok(match r {
        Ok(()) => Ok(cm),
        Err(_) => Err(status(&r)),
    })
}

/// Runs a fitting routine on Euclidean or homogeneous data by `ptp`.
macro_rules! by_ptp {
    ($ptp:expr, $pts:expr, |$a:ident, $conv:ident| $body:expr) => {{
        ptp_ok($ptp)?;
        let raw = pts4($pts)?;
        if $ptp == 1 {
            let $conv = euclid;
            let $a: Vec<Vec3> = raw.iter().copied().map(euclid).collect();
            $body
        } else {
            let $conv = homo;
            let $a: Vec<CPoint> = raw.iter().copied().map(homo).collect();
            $body
        }
    }};
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::FuncInterp { f, u, p } => {
            push_function(&mut o, fit::func_interp_given_params(f, *p, u));
        }
        Inputs::Interp { pts, p, par } => {
            let a: Vec<Vec3> = pts4(pts)?.into_iter().map(euclid).collect();
            push_curve(&mut o, fit::interp(&a, *p, par_method(*par)?));
        }
        Inputs::LstSq { pts, n, p, par } => {
            let a: Vec<Vec3> = pts4(pts)?.into_iter().map(euclid).collect();
            push_curve(&mut o, fit::approx_lst_sq(&a, *n, *p, par_method(*par)?));
        }
        Inputs::CrvMatrix {
            ptp,
            pts,
            p,
            matrix: m,
        } => {
            ptp_ok(*ptp)?;
            let cm = match rust_matrix(need(m.as_ref(), "matrix")?)? {
                Ok(cm) => cm,
                Err(st) => return Ok(vec![st]),
            };
            o.push(0.0);
            by_ptp!(*ptp, pts, |a, _c| push_cpts(
                &mut o,
                fit::cpts_with_matrix(&a, *p, &cm)
            ));
        }
        Inputs::DerivMatrix {
            ptp,
            pts,
            p,
            knots,
            t,
            whr,
            matrix: m,
        } => {
            ptp_ok(*ptp)?;
            let cm = match rust_matrix(need(m.as_ref(), "matrix")?)? {
                Ok(cm) => cm,
                Err(st) => return Ok(vec![st]),
            };
            o.push(0.0);
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let t = pt4(t)?;
            let whr = deriv_end(*whr)?;
            by_ptp!(*ptp, pts, |a, c| push_cpts(
                &mut o,
                fit::deriv_matrix(&a, *p, &knt, c(t), whr, &cm)
            ));
        }
        Inputs::Weighted {
            ptp,
            pts,
            wp,
            d,
            wd,
            idx,
            u,
            knots,
            n,
            p,
        } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let u = need(u.as_ref(), "u")?;
            let dd = pts4(d)?;
            by_ptp!(*ptp, pts, |a, c| {
                let dv: Vec<_> = dd.iter().copied().map(c).collect();
                let data = WeightedData {
                    points: &a,
                    point_weights: wp,
                    derivs: &dv,
                    deriv_weights: wd,
                    deriv_index: idx,
                };
                push_curve(&mut o, fit::weighted_lst_sq_knots(&data, u, &knt, *n, *p));
            });
        }
        Inputs::KnotsDerivs { u, p } => push_values(&mut o, fit::knots_derivs(u, *p)),
        Inputs::HighEndKnots { u, p, k, l } => {
            push_values(&mut o, fit::knots_high_end_derivs(u, *p, *k, *l));
        }
        Inputs::CalcMatrix { u, knots, p, itp } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            push_matrix(&mut o, fit::calc_matrix(&knt, *p, u, matrix_ends(*itp)?))?;
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    let c = rng.uniform(-10.0, 10.0);
    if rng.chance(0.1) {
        c.round()
    } else {
        c
    }
}

fn gen_pts(rng: &mut Rng, ptp: i32, cnt: usize) -> Vec<Vec<f64>> {
    let mut pts: Vec<Vec<f64>> = Vec::with_capacity(cnt);
    // A random walk keeps the data curve-like.
    let mut cur = [coord(rng), coord(rng), coord(rng)];
    let flat = rng.chance(0.2);
    for _ in 0..cnt {
        if !pts.is_empty() && rng.chance(0.03) {
            // Repeated point (zero chord).
            let last = pts[pts.len() - 1].clone();
            pts.push(last);
            continue;
        }
        for c in &mut cur {
            *c += rng.uniform(-3.0, 3.0);
        }
        if flat {
            cur[2] = 0.0;
        }
        let mut p = cur.to_vec();
        if ptp == 2 {
            let w = if rng.chance(0.1) {
                1.0
            } else {
                rng.uniform(0.3, 3.0)
            };
            for c in &mut p {
                *c *= w;
            }
            p.push(w);
        }
        pts.push(p);
    }
    pts
}

fn gen_vec(rng: &mut Rng, ptp: i32) -> Vec<f64> {
    let mut v = vec![coord(rng), coord(rng), coord(rng)];
    if ptp == 2 {
        v.push(rng.uniform(-1.0, 1.0));
    }
    v
}

fn gen_ptp(rng: &mut Rng) -> i32 {
    if rng.chance(0.6) {
        1
    } else {
        2
    }
}

fn below(rng: &mut Rng, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        rng.below(n as u64) as usize
    }
}

/// Sorted parameters in `[0, 1]` with `u[0] = 0`, `u[last] = 1`.
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
        // A repeated interior parameter.
        let i = 1 + below(rng, cnt - 2);
        u[i] = u[i - 1];
    }
    u
}

/// A diagonally dominant matrix (full or banded) by storage.
fn gen_matrix(rng: &mut Rng, n: usize) -> MatrixIn {
    if rng.chance(0.3) {
        let mut storage = Vec::with_capacity((n + 1) * (n + 1));
        for i in 0..=n {
            for j in 0..=n {
                storage.push(if i == j {
                    rng.uniform(3.0, 6.0) * (n as f64 + 1.0)
                } else {
                    rng.uniform(-1.0, 1.0)
                });
            }
        }
        if rng.chance(0.05) {
            storage[0] = 0.0;
        }
        return MatrixIn {
            n,
            mtp: 0,
            bw: n + 1,
            storage,
        };
    }
    let sbw = below(rng, n.min(3) + 1);
    let bw = 2 * sbw + 1;
    let cols = if bw == 1 { 2 } else { bw };
    let mut storage = Vec::with_capacity((n + 1) * cols);
    for _ in 0..=n {
        for c in 0..cols {
            storage.push(if c == sbw {
                rng.uniform(3.0, 6.0) * bw as f64
            } else {
                rng.uniform(-1.0, 1.0)
            });
        }
    }
    MatrixIn {
        n,
        mtp: 3,
        bw,
        storage,
    }
}

/// Random non-decreasing knots for a degree-`p` curve with `cnt` knots, clamped
/// over `[0, 1]`.
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

fn gen_par(rng: &mut Rng) -> i32 {
    1 + below(rng, 3) as i32
}

fn gen_weighted(rng: &mut Rng) -> Inputs {
    let ptp = gen_ptp(rng);
    let cnt = 3 + below(rng, 28);
    let r = cnt - 1;
    // Derivatives at a few distinct, increasing points.
    let s = if rng.chance(0.5) {
        0
    } else {
        1 + below(rng, 3)
    };
    let mut idx: Vec<usize> = Vec::new();
    for _ in 0..s {
        let i = below(rng, cnt);
        if !idx.contains(&i) {
            idx.push(i);
        }
    }
    idx.sort_unstable();
    let s = idx.len();
    let mut wp: Vec<f64> = (0..cnt).map(|_| rng.uniform(0.2, 4.0)).collect();
    let mut wd: Vec<f64> = (0..s).map(|_| rng.uniform(0.2, 4.0)).collect();
    if rng.chance(0.5) {
        wp[0] = -1.0;
        wp[r] = -1.0;
    }
    if rng.chance(0.2) {
        wp[below(rng, cnt)] = if rng.chance(0.3) { 0.0 } else { -1.0 };
    }
    for w in &mut wd {
        if rng.chance(0.3) {
            *w = -1.0;
        }
    }
    let total = cnt + s;
    let n = 1 + below(rng, (total / 2).max(2));
    let p = 1 + below(rng, n.min(5));
    let d: Vec<Vec<f64>> = (0..s).map(|_| gen_vec(rng, ptp)).collect();
    let u = if rng.chance(0.2) {
        Some(gen_params(rng, cnt))
    } else {
        None
    };
    let knots = if rng.chance(0.1) {
        Some(gen_knots(rng, n + p + 2, p))
    } else {
        None
    };
    // Explicit knots when the approximation knot routine would refuse (n > r).
    let knots = if knots.is_none() && n > r {
        Some(gen_knots(rng, n + p + 2, p))
    } else {
        knots
    };
    Inputs::Weighted {
        ptp,
        pts: gen_pts(rng, ptp, cnt),
        wp,
        d,
        wd,
        idx,
        u,
        knots,
        n,
        p,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let ptp = gen_ptp(rng);
    match below(rng, 10) {
        0 => {
            let cnt = 1 + below(rng, 15);
            let n = cnt - 1;
            let p = if rng.chance(0.05) {
                n + 1
            } else {
                1 + below(rng, n.clamp(1, 6))
            };
            Inputs::FuncInterp {
                f: (0..cnt).map(|_| coord(rng)).collect(),
                u: gen_params(rng, cnt),
                p,
            }
        }
        1 => {
            let cnt = 1 + below(rng, 15);
            let n = cnt - 1;
            let p = if rng.chance(0.05) {
                n + 1
            } else {
                1 + below(rng, n.clamp(1, 6))
            };
            Inputs::Interp {
                pts: gen_pts(rng, 1, cnt),
                p,
                par: gen_par(rng),
            }
        }
        2 => {
            let cnt = 2 + below(rng, 30);
            let m = cnt - 1;
            let n = if rng.chance(0.05) {
                below(rng, m + 3)
            } else {
                2 + below(rng, m.saturating_sub(1).max(1))
            };
            let p = 1 + below(rng, n.clamp(1, 5));
            Inputs::LstSq {
                pts: gen_pts(rng, 1, cnt),
                n,
                p,
                par: gen_par(rng),
            }
        }
        3 => {
            let cnt = 2 + below(rng, 10);
            let n = cnt - 1;
            let p = if rng.chance(0.1) {
                1
            } else {
                2 + below(rng, n.clamp(1, 4))
            };
            let matrix = if p == 1 || p > n || rng.chance(0.3) {
                let mn = below(rng, cnt);
                Some(gen_matrix(rng, mn))
            } else {
                None
            };
            Inputs::CrvMatrix {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                p,
                matrix,
            }
        }
        4 => {
            let cnt = 2 + below(rng, 10);
            let k = cnt - 1;
            let p = 2 + below(rng, (k + 1).min(3));
            let whr = 1 + below(rng, 2) as i32;
            let (knots, matrix) = if p > k + 1 || rng.chance(0.15) {
                let mn = below(rng, k + 2);
                (
                    Some(gen_knots(rng, k + p + 3, p)),
                    Some(gen_matrix(rng, mn)),
                )
            } else {
                (None, None)
            };
            Inputs::DerivMatrix {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                p,
                knots,
                t: gen_vec(rng, ptp),
                whr,
                matrix,
            }
        }
        5 | 6 => gen_weighted(rng),
        7 => {
            let cnt = 1 + below(rng, 14);
            let mut p = below(rng, 6);
            if p == 2 && cnt < 2 {
                p = 3;
            }
            Inputs::KnotsDerivs {
                u: gen_params(rng, cnt),
                p,
            }
        }
        8 => {
            let cnt = 1 + below(rng, 14);
            let p = below(rng, 7);
            Inputs::HighEndKnots {
                u: gen_params(rng, cnt),
                p,
                k: below(rng, p + 1),
                l: below(rng, p + 1),
            }
        }
        _ => {
            let cnt = 1 + below(rng, 14);
            let n = cnt - 1;
            let itp = below(rng, 4) as i32;
            let extra = match itp {
                0 => 0,
                1 | 2 => 1,
                _ => 2,
            };
            let pmax = (n + extra).clamp(1, 5);
            let p = 1 + below(rng, pmax);
            let auto_ok = p <= n + extra && n != 0;
            let knots = if !auto_ok || rng.chance(0.1) {
                let len = (n + extra + p + 2).max(2 * p + 2);
                Some(gen_knots(rng, len, p))
            } else {
                None
            };
            Inputs::CalcMatrix {
                u: gen_params(rng, cnt),
                knots,
                p,
                itp,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_fit_p2"
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
