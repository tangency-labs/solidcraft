//! Runner for unit `nlib.crv_fit_p1`: curve fitting part 1 (parameter values, knot
//! vectors for interpolation and approximation, global interpolation with given
//! parameters and knots, with end derivatives or first derivatives, least-squares
//! approximation with given knots, and solves with a given LU-decomposed matrix).
//!
//! C++: `solidcore_ffi::units::nlib_crv_fit_p1` (upstream
//! `source/SMLib/src/NL_CrvFit.cpp`). Rust: `solidcore::nurbs::crv_fit::p1`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record
//! (see the ffi module), split into keys: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), for
//! the matrix ops first `lu_status`, then the result: `values` (knot or parameter
//! vectors), or `dims = [n, p, m]`, `cpts` (x y z w) and `knots` (curves), or
//! `cpts` (control point arrays).
//!
//! Points are `[x, y, z]` or `[x, y, z, w]` (a missing `w` is 1); `ptp` 1 uses
//! `Vec3` data (`w` ignored), 2 homogeneous [`CPoint`]s. A fitting case may leave
//! out `u` (then chord-length parameters of the points, computed once by the
//! runner and given to both sides) and `knots` (then the knot vector the matching
//! upstream knot routine builds from `u`), as the upstream drivers
//! (`N_FitCrvInterp`, `N_FitCrvDerivs`, `N_FitCrvFirstDeriv`, ...) do. Every run
//! has at least [`MIN_GENERATED`] generated cases.

// Harness code: the input builders index their own freshly sized arrays; a panic
// would fail the case (runner panics are caught).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::{self as fit, DataDir, DerivEnd, ParamMethod};
use solidcore::nurbs::knots::KnotVector;
use solidcore::nurbs::math::{CPoint, MatrixType, RealMatrix};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_crv_fit_p1::{self as ffi, Matrix, Pts};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_fit_p1` runner.
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
    ParamValues {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        par: i32,
    },
    InterpKnots {
        u: Vec<f64>,
        p: usize,
    },
    EndDerivsKnots {
        u: Vec<f64>,
        p: usize,
    },
    DerivKnots {
        u: Vec<f64>,
        p: usize,
        whr: i32,
    },
    ApproxKnotVector {
        u: Vec<f64>,
        n: usize,
        p: usize,
    },
    Interp {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        p: usize,
    },
    Derivs {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        p: usize,
        ds: Vec<f64>,
        de: Vec<f64>,
    },
    Deriv {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        p: usize,
        t: Vec<f64>,
        whr: i32,
    },
    FirstDerivs {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        d: Vec<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        p: usize,
    },
    Approx {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        n: usize,
        p: usize,
    },
    SrfData {
        ptp: i32,
        grid: Vec<Vec<Vec<f64>>>,
        k: usize,
        dir: i32,
        matrix: MatrixIn,
    },
    DerivsMatrix {
        ptp: i32,
        pts: Vec<Vec<f64>>,
        p: usize,
        knots: Vec<f64>,
        ds: Vec<f64>,
        de: Vec<f64>,
        matrix: MatrixIn,
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
        fit::param_values(&e, ParamMethod::ChordLength)
    } else {
        let h: Vec<CPoint> = pts.iter().copied().map(homo).collect();
        fit::param_values(&h, ParamMethod::ChordLength)
    };
    r.map_err(|e| harness_err(&e))
}

/// Knot vector of `N_FitCalcKnotsDerivs` (upstream driver of
/// `N_FitCrvFirstDerivAndKnots`), re-expressed here to build inputs.
/// A degree above [`MAX_DERIVED_DEGREE`] is a harness error (the knot vector would
/// not fit in memory; upstream rejects every degree but 2 and 3 anyway).
fn first_deriv_knots(u: &[f64], p: usize) -> Result<Vec<f64>, HarnessError> {
    if p > MAX_DERIVED_DEGREE {
        return Err(harness("first_derivs: degree too large to derive knots"));
    }
    let k = u.len() - 1;
    let n = 2 * k + 1;
    let mut uk = vec![0.0; n + p + 2];
    for i in 0..=p {
        uk[i] = u[0];
        uk[n + i + 1] = u[k];
    }
    let mut j = p + 1;
    match p {
        2 => {
            uk[p + 1] = 0.5 * (u[1] + u[0]);
            for i in 1..k {
                j += 1;
                uk[j] = u[i];
                j += 1;
                uk[j] = 0.5 * (u[i] + u[i + 1]);
            }
        }
        3 => {
            if k > 1 {
                uk[p + 1] = 0.5 * (u[1] + u[0]);
                uk[n] = 0.5 * (u[k] + u[k - 1]);
            }
            let alf = 1.0 / 3.0;
            let bet = 2.0 / 3.0;
            for i in 1..k.saturating_sub(1) {
                j += 1;
                uk[j] = bet * u[i] + alf * u[i + 1];
                j += 1;
                uk[j] = alf * u[i] + bet * u[i + 1];
            }
        }
        _ => {}
    }
    Ok(monotone(uk))
}

/// Largest degree for which the runner derives a knot vector itself.
const MAX_DERIVED_DEGREE: usize = 1 << 16;

/// Chord-length parameters may overshoot `u[n]` by an ulp before the last one, and
/// knots derived from them with it; keeps derived knots non-decreasing (the ffi
/// wrapper requires it).
fn monotone(mut uk: Vec<f64>) -> Vec<f64> {
    for i in 1..uk.len() {
        uk[i] = uk[i].max(uk[i - 1]);
    }
    uk
}

/// Fills in left-out `u` and `knots`.
fn resolve(i: &Inputs) -> Result<Inputs, HarnessError> {
    let params =
        |ptp: i32, pts: &[Vec<f64>], u: &Option<Vec<f64>>| -> Result<Vec<f64>, HarnessError> {
            match u {
                Some(u) => Ok(u.clone()),
                None => chord_params(ptp, &pts4(pts)?),
            }
        };
    let out = match i.clone() {
        Inputs::Interp {
            ptp,
            pts,
            u,
            knots,
            p,
        } => {
            let u = params(ptp, &pts, &u)?;
            let knots = match knots {
                Some(k) => k,
                None if p == 1 => {
                    // N_FitCrvInterp: degree 1 uses the parameters with doubled ends.
                    let (Some(&u0), Some(&un)) = (u.first(), u.last()) else {
                        return Err(harness("interp: no parameters for knots"));
                    };
                    let mut k = vec![u0];
                    k.extend_from_slice(&u);
                    k.push(un);
                    monotone(k)
                }
                None => monotone(fit::interp_knot_vector(&u, p).map_err(|e| harness_err(&e))?),
            };
            Inputs::Interp {
                ptp,
                pts,
                u: Some(u),
                knots: Some(knots),
                p,
            }
        }
        Inputs::Derivs {
            ptp,
            pts,
            u,
            knots,
            p,
            ds,
            de,
        } => {
            let u = params(ptp, &pts, &u)?;
            let knots = match knots {
                Some(k) => k,
                None => monotone(fit::knot_vector_end_derivs(&u, p).map_err(|e| harness_err(&e))?),
            };
            Inputs::Derivs {
                ptp,
                pts,
                u: Some(u),
                knots: Some(knots),
                p,
                ds,
                de,
            }
        }
        Inputs::Deriv {
            ptp,
            pts,
            u,
            knots,
            p,
            t,
            whr,
        } => {
            let u = params(ptp, &pts, &u)?;
            let knots = if let Some(k) = knots {
                k
            } else {
                let w = if whr == 1 {
                    DerivEnd::Start
                } else {
                    DerivEnd::End
                };
                monotone(fit::knot_vector_deriv(&u, p, w).map_err(|e| harness_err(&e))?)
            };
            Inputs::Deriv {
                ptp,
                pts,
                u: Some(u),
                knots: Some(knots),
                p,
                t,
                whr,
            }
        }
        Inputs::FirstDerivs {
            ptp,
            pts,
            d,
            u,
            knots,
            p,
        } => {
            let u = params(ptp, &pts, &u)?;
            let knots = match knots {
                Some(k) => k,
                None if u.len() >= 2 => first_deriv_knots(&u, p)?,
                None => return Err(harness("first_derivs: need two points for knots")),
            };
            Inputs::FirstDerivs {
                ptp,
                pts,
                d,
                u: Some(u),
                knots: Some(knots),
                p,
            }
        }
        Inputs::Approx {
            ptp,
            pts,
            u,
            knots,
            n,
            p,
        } => {
            let u = params(ptp, &pts, &u)?;
            let knots = match knots {
                Some(k) => k,
                None => monotone(fit::knot_vector_approx(&u, n, p).map_err(|e| harness_err(&e))?),
            };
            Inputs::Approx {
                ptp,
                pts,
                u: Some(u),
                knots: Some(knots),
                n,
                p,
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
    CPoints,
}

fn shape_of(i: &Inputs) -> (bool, Shape) {
    match i {
        Inputs::ParamValues { .. }
        | Inputs::InterpKnots { .. }
        | Inputs::EndDerivsKnots { .. }
        | Inputs::DerivKnots { .. }
        | Inputs::ApproxKnotVector { .. } => (false, Shape::Values),
        Inputs::SrfData { .. } | Inputs::DerivsMatrix { .. } => (true, Shape::CPoints),
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
        Shape::Curve => {
            let d = take(flat, &mut at, 3)?;
            let (n, m) = (count(d[0])?, count(d[2])?);
            out = out.ints("dims", d.iter().map(|x| int(*x)).collect::<Vec<_>>());
            out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
            out = out.values("knots", take(flat, &mut at, m + 1)?.to_vec());
        }
        Shape::CPoints => {
            let c = count(take(flat, &mut at, 1)?[0])?;
            out = out.int("count", c as i64);
            out = out.values("cpts", take(flat, &mut at, 4 * c)?.to_vec());
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
        Inputs::ParamValues { ptp, pts, par } => {
            let p = pts4(pts)?;
            ffi::param_values(&Pts { ptp: *ptp, pts: &p }, *par)
        }
        Inputs::InterpKnots { u, p } => ffi::interp_knot_vector(u, *p),
        Inputs::EndDerivsKnots { u, p } => ffi::knot_vector_end_derivs(u, *p),
        Inputs::DerivKnots { u, p, whr } => ffi::knot_vector_deriv(u, *p, *whr),
        Inputs::ApproxKnotVector { u, n, p } => ffi::knot_vector_approx(u, *n, *p),
        Inputs::Interp {
            ptp,
            pts,
            u,
            knots,
            p,
        } => {
            let a = pts4(pts)?;
            ffi::interp_given_params(
                &Pts { ptp: *ptp, pts: &a },
                need(u.as_ref(), "u")?,
                need(knots.as_ref(), "knots")?,
                *p,
            )
        }
        Inputs::Derivs {
            ptp,
            pts,
            u,
            knots,
            p,
            ds,
            de,
        } => {
            let a = pts4(pts)?;
            ffi::knots_and_derivs(
                &Pts { ptp: *ptp, pts: &a },
                need(u.as_ref(), "u")?,
                need(knots.as_ref(), "knots")?,
                *p,
                pt4(ds)?,
                pt4(de)?,
            )
        }
        Inputs::Deriv {
            ptp,
            pts,
            u,
            knots,
            p,
            t,
            whr,
        } => {
            let a = pts4(pts)?;
            ffi::knots_and_deriv(
                &Pts { ptp: *ptp, pts: &a },
                need(u.as_ref(), "u")?,
                need(knots.as_ref(), "knots")?,
                *p,
                pt4(t)?,
                *whr,
            )
        }
        Inputs::FirstDerivs {
            ptp,
            pts,
            d,
            u,
            knots,
            p,
        } => {
            let a = pts4(pts)?;
            let dd = pts4(d)?;
            ffi::first_deriv_and_knots(
                &Pts { ptp: *ptp, pts: &a },
                &Pts {
                    ptp: *ptp,
                    pts: &dd,
                },
                need(u.as_ref(), "u")?,
                need(knots.as_ref(), "knots")?,
                *p,
            )
        }
        Inputs::Approx {
            ptp,
            pts,
            u,
            knots,
            n,
            p,
        } => {
            let a = pts4(pts)?;
            ffi::approx_knots(
                &Pts { ptp: *ptp, pts: &a },
                need(u.as_ref(), "u")?,
                need(knots.as_ref(), "knots")?,
                *n,
                *p,
            )
        }
        Inputs::SrfData {
            ptp,
            grid,
            k,
            dir,
            matrix: m,
        } => {
            let g: Vec<Vec<[f64; 4]>> = grid.iter().map(|r| pts4(r)).collect::<Result<_, _>>()?;
            ffi::cpts_from_srf_data(*ptp, &g, *k, *dir, &matrix(m))
        }
        Inputs::DerivsMatrix {
            ptp,
            pts,
            p,
            knots,
            ds,
            de,
            matrix: m,
        } => {
            let a = pts4(pts)?;
            ffi::derivs_matrix(
                &Pts { ptp: *ptp, pts: &a },
                *p,
                knots,
                pt4(ds)?,
                pt4(de)?,
                &matrix(m),
            )
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

fn push_cpts(o: &mut Vec<f64>, r: SolidResult<Vec<CPoint>>) {
    o.push(status(&r));
    if let Ok(v) = r {
        o.push(v.len() as f64);
        for pw in v {
            o.extend_from_slice(&pw.to_array());
        }
    }
}

fn par_method(par: i32) -> Result<ParamMethod, HarnessError> {
    match par {
        1 => Ok(ParamMethod::Uniform),
        2 => Ok(ParamMethod::ChordLength),
        3 => Ok(ParamMethod::Centripetal),
        _ => Err(harness(format!("par {par}"))),
    }
}

fn deriv_end(whr: i32) -> Result<DerivEnd, HarnessError> {
    match whr {
        1 => Ok(DerivEnd::Start),
        2 => Ok(DerivEnd::End),
        _ => Err(harness(format!("whr {whr}"))),
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
        .filter(|&(_, cols)| cols > 0)
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
        Inputs::ParamValues { ptp, pts, par } => {
            let par = par_method(*par)?;
            by_ptp!(*ptp, pts, |a, _c| push_values(
                &mut o,
                fit::param_values(&a, par)
            ));
        }
        Inputs::InterpKnots { u, p } => push_values(&mut o, fit::interp_knot_vector(u, *p)),
        Inputs::EndDerivsKnots { u, p } => {
            push_values(&mut o, fit::knot_vector_end_derivs(u, *p));
        }
        Inputs::DerivKnots { u, p, whr } => {
            push_values(&mut o, fit::knot_vector_deriv(u, *p, deriv_end(*whr)?));
        }
        Inputs::ApproxKnotVector { u, n, p } => {
            push_values(&mut o, fit::knot_vector_approx(u, *n, *p));
        }
        Inputs::Interp {
            ptp,
            pts,
            u,
            knots,
            p,
        } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let u = need(u.as_ref(), "u")?;
            by_ptp!(*ptp, pts, |a, _c| push_curve(
                &mut o,
                fit::interp_given_params(&a, u, &knt, *p)
            ));
        }
        Inputs::Derivs {
            ptp,
            pts,
            u,
            knots,
            p,
            ds,
            de,
        } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let u = need(u.as_ref(), "u")?;
            let (ds, de) = (pt4(ds)?, pt4(de)?);
            by_ptp!(*ptp, pts, |a, c| push_curve(
                &mut o,
                fit::interp_knots_and_derivs(&a, u, &knt, *p, c(ds), c(de))
            ));
        }
        Inputs::Deriv {
            ptp,
            pts,
            u,
            knots,
            p,
            t,
            whr,
        } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let u = need(u.as_ref(), "u")?;
            let t = pt4(t)?;
            let whr = deriv_end(*whr)?;
            by_ptp!(*ptp, pts, |a, c| push_curve(
                &mut o,
                fit::interp_knots_and_deriv(&a, u, &knt, *p, c(t), whr)
            ));
        }
        Inputs::FirstDerivs {
            ptp,
            pts,
            d,
            u,
            knots,
            p,
        } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let u = need(u.as_ref(), "u")?;
            let dd = pts4(d)?;
            by_ptp!(*ptp, pts, |a, c| {
                let dv: Vec<_> = dd.iter().copied().map(c).collect();
                push_curve(
                    &mut o,
                    fit::interp_first_derivs_and_knots(&a, &dv, u, &knt, *p),
                );
            });
        }
        Inputs::Approx {
            ptp,
            pts,
            u,
            knots,
            n,
            p,
        } => {
            let knt = knot_vector(need(knots.as_ref(), "knots")?)?;
            let u = need(u.as_ref(), "u")?;
            by_ptp!(*ptp, pts, |a, _c| push_curve(
                &mut o,
                fit::approx_knots(&a, u, &knt, *n, *p)
            ));
        }
        Inputs::SrfData {
            ptp,
            grid,
            k,
            dir,
            matrix: m,
        } => {
            ptp_ok(*ptp)?;
            let cm = match rust_matrix(m)? {
                Ok(cm) => cm,
                Err(st) => return Ok(vec![st]),
            };
            o.push(0.0);
            let dir = match dir {
                1 => DataDir::U,
                2 => DataDir::V,
                d => return Err(harness(format!("dir {d}"))),
            };
            let g: Vec<Vec<[f64; 4]>> = grid.iter().map(|r| pts4(r)).collect::<Result<_, _>>()?;
            if *ptp == 1 {
                let a: Vec<Vec<Vec3>> = g
                    .iter()
                    .map(|r| r.iter().copied().map(euclid).collect())
                    .collect();
                push_cpts(&mut o, fit::cpts_from_srf_data(&a, *k, dir, &cm));
            } else {
                let a: Vec<Vec<CPoint>> = g
                    .iter()
                    .map(|r| r.iter().copied().map(homo).collect())
                    .collect();
                push_cpts(&mut o, fit::cpts_from_srf_data(&a, *k, dir, &cm));
            }
        }
        Inputs::DerivsMatrix {
            ptp,
            pts,
            p,
            knots,
            ds,
            de,
            matrix: m,
        } => {
            ptp_ok(*ptp)?;
            let cm = match rust_matrix(m)? {
                Ok(cm) => cm,
                Err(st) => return Ok(vec![st]),
            };
            o.push(0.0);
            let knt = knot_vector(knots)?;
            let (ds, de) = (pt4(ds)?, pt4(de)?);
            by_ptp!(*ptp, pts, |a, c| push_cpts(
                &mut o,
                fit::derivs_matrix(&a, *p, &knt, c(ds), c(de), &cm)
            ));
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
        if !pts.is_empty() && rng.chance(0.05) {
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
    rng.below(n as u64) as usize
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
    if rng.chance(0.2) && cnt > 3 {
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

fn gen_case(rng: &mut Rng) -> Inputs {
    let ptp = gen_ptp(rng);
    match below(rng, 12) {
        0 => {
            let cnt = if rng.chance(0.05) {
                1
            } else {
                2 + below(rng, 14)
            };
            Inputs::ParamValues {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                par: 1 + below(rng, 3) as i32,
            }
        }
        1 => {
            let cnt = 1 + below(rng, 14);
            Inputs::InterpKnots {
                u: gen_params(rng, cnt),
                p: below(rng, 7),
            }
        }
        2 => {
            let cnt = 1 + below(rng, 14);
            let mut p = below(rng, 7);
            if p == 1 && cnt < 2 {
                p = 2;
            }
            Inputs::EndDerivsKnots {
                u: gen_params(rng, cnt),
                p,
            }
        }
        3 => {
            let cnt = 1 + below(rng, 14);
            let mut p = below(rng, cnt + 3);
            if p == 1 && cnt < 2 {
                p = 0;
            }
            Inputs::DerivKnots {
                u: gen_params(rng, cnt),
                p,
                whr: 1 + below(rng, 2) as i32,
            }
        }
        4 => {
            let cnt = 1 + below(rng, 30);
            Inputs::ApproxKnotVector {
                u: gen_params(rng, cnt),
                n: below(rng, cnt + 2),
                p: below(rng, 6),
            }
        }
        5 => {
            let cnt = 2 + below(rng, 14);
            let n = cnt - 1;
            let p = if rng.chance(0.1) {
                0
            } else {
                1 + below(rng, n.min(6))
            };
            let knots = if rng.chance(0.1) || p == 0 {
                {
                    let extra = below(rng, 2);
                    Some(gen_knots(rng, n + p + 2 + extra, p))
                }
            } else {
                None
            };
            let u = if rng.chance(0.2) {
                Some(gen_params(rng, cnt))
            } else {
                None
            };
            Inputs::Interp {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                u,
                knots,
                p,
            }
        }
        6 => {
            let cnt = 1 + below(rng, 12);
            let kk = cnt - 1;
            let p = if rng.chance(0.05) {
                1
            } else {
                2 + below(rng, (kk + 1).min(5))
            };
            let knots = if rng.chance(0.1) || p == 1 {
                Some(gen_knots(rng, kk + p + 4, p))
            } else {
                None
            };
            Inputs::Derivs {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                u: None,
                knots,
                p,
                ds: gen_vec(rng, ptp),
                de: gen_vec(rng, ptp),
            }
        }
        7 => {
            let cnt = 2 + below(rng, 12);
            let kk = cnt - 1;
            let p = if rng.chance(0.05) {
                kk + 2
            } else {
                2 + below(rng, kk.min(5))
            };
            let knots = if rng.chance(0.1) || p > kk + 1 {
                Some(gen_knots(rng, kk + p + 3, p))
            } else {
                None
            };
            Inputs::Deriv {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                u: None,
                knots,
                p,
                t: gen_vec(rng, ptp),
                whr: 1 + below(rng, 2) as i32,
            }
        }
        8 => {
            let cnt = 2 + below(rng, 10);
            let kk = cnt - 1;
            let p = if rng.chance(0.05) {
                4
            } else {
                2 + below(rng, 2)
            };
            let knots = if rng.chance(0.15) {
                Some(gen_knots(rng, 2 * kk + p + 3, p))
            } else {
                None
            };
            let d: Vec<Vec<f64>> = (0..cnt).map(|_| gen_vec(rng, ptp)).collect();
            Inputs::FirstDerivs {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                d,
                u: None,
                knots,
                p,
            }
        }
        9 => {
            let cnt = 2 + below(rng, 30);
            let m = cnt - 1;
            // n > m (an input error) needs explicit knots.
            let n = if rng.chance(0.05) {
                m + 1
            } else {
                1 + below(rng, m)
            };
            let p = 1 + below(rng, n.min(5));
            let u = if rng.chance(0.2) {
                Some(gen_params(rng, cnt))
            } else {
                None
            };
            let knots = if n > m || rng.chance(0.1) {
                Some(gen_knots(rng, n + p + 2, p))
            } else {
                None
            };
            Inputs::Approx {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                u,
                knots,
                n,
                p,
            }
        }
        10 => {
            let rows = 1 + below(rng, 8);
            let cols = 1 + below(rng, 8);
            let grid: Vec<Vec<Vec<f64>>> = (0..rows).map(|_| gen_pts(rng, ptp, cols)).collect();
            let dir = 1 + below(rng, 2) as i32;
            let (len, other) = if dir == 1 { (rows, cols) } else { (cols, rows) };
            let n = if rng.chance(0.8) {
                len - 1
            } else {
                below(rng, len)
            };
            Inputs::SrfData {
                ptp,
                grid,
                k: below(rng, other),
                dir,
                matrix: gen_matrix(rng, n),
            }
        }
        _ => {
            let cnt = 1 + below(rng, 9);
            let k = cnt - 1;
            let p = 2 + below(rng, 3);
            let u = gen_params(rng, cnt.max(2));
            let knots = if u.len() == cnt {
                fit::knot_vector_end_derivs(&u, p).unwrap_or_else(|_| gen_knots(rng, k + p + 4, p))
            } else {
                gen_knots(rng, k + p + 4, p)
            };
            let n = if rng.chance(0.8) {
                k + 2
            } else {
                below(rng, k + 3)
            };
            Inputs::DerivsMatrix {
                ptp,
                pts: gen_pts(rng, ptp, cnt),
                p,
                knots,
                ds: gen_vec(rng, ptp),
                de: gen_vec(rng, ptp),
                matrix: gen_matrix(rng, n),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_fit_p1"
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
