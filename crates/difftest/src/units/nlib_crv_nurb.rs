//! Runner for unit `nlib.crv_nurb`: NLib curve evaluation and geometry (points,
//! derivatives, tangents, curvature, Frenet frames, boxes, affine and projective maps,
//! reversal, reparametrisation, Greville abscissae, linear / degenerate segments,
//! line and circle recognition, closure continuity).
//!
//! C++: `solidcore_ffi::units::nlib_crv_nurb` (upstream
//! `source/SMLib/src/NL_CrvNurb.cpp`). Rust: `solidcore::nurbs::crv_nurb` (methods on
//! `solidcore::nurbs::crv_basic::Curve`).
//!
//! Each case runs one routine (`inputs.op`) on one curve (none for `line`). Both
//! sides produce the same flat record of doubles, split into keys by [`to_output`]:
//! `status` (`0` ok, else the `NL_*_ERR` number; `-1` on the Rust side for a non-NLib
//! error, which never matches), then on success the outputs. A curve output is
//! `dims = [n, p, m]`, the control points in use (`cpts`, x y z w) and the knots in
//! use (`u`). The C++ side may run `project` / `reverse` in place (`inplace`); the
//! Rust side runs [`Curve::reverse`] for in-place and [`Curve::reversed`] otherwise.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). `degen_segs` is deferred on the Rust side whenever it
//! needs `N_CrvArcLength` (unit `nlib.crv_geom`). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_nurb::{greville_abscissa, line_from_pt_and_vector, CurveType, Projection};
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, MatrixType, RealMatrix, NOW, NOZ};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::nlib_crv_nurb::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_nurb` runner.
pub struct Runner;

/// An inline curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineCrv {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A curve input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum CrvIn {
    /// An upstream `.smc` file (absolute path after fixture expansion).
    File { smc: String },
    /// Inline data.
    Inline(InlineCrv),
}

/// A parameter: a value, or `{"knot": i}` for the curve's knot `U[i]` (lets
/// fixtures name knots of `.smc` files exactly; JSON text need not round-trip).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
enum Param {
    /// The parameter value.
    Value(f64),
    /// Knot `U[knot]` of the case's curve.
    Knot { knot: usize },
}

impl Param {
    fn get(self, c: &InlineCrv) -> Result<f64, HarnessError> {
        match self {
            Param::Value(v) => Ok(v),
            Param::Knot { knot } => c
                .u
                .get(knot)
                .copied()
                .ok_or_else(|| harness(format!("no knot {knot}"))),
        }
    }
}


/// Evaluation side (`NL_LEFT` / `NL_RIGHT`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SideIn {
    #[default]
    Left,
    Right,
}

impl SideIn {
    fn flag(self) -> f64 {
        match self {
            SideIn::Left => 1.0,
            SideIn::Right => 2.0,
        }
    }

    fn side(self) -> Side {
        match self {
            SideIn::Left => Side::Left,
            SideIn::Right => Side::Right,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Greville {
        crv: CrvIn,
        deg: usize,
        idx: usize,
    },
    Curvature {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
        #[serde(default)]
        circle: bool,
    },
    Eval {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    Bbox {
        crv: CrvIn,
    },
    Derivs {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
        der: usize,
    },
    Tangent {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    Frenet {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    Transform {
        crv: CrvIn,
        m: [f64; 16],
    },
    MinMax {
        crv: CrvIn,
    },
    Line {
        p: [f64; 3],
        v: [f64; 3],
    },
    Scale {
        crv: CrvIn,
        c: [f64; 3],
        f: [f64; 3],
    },
    ScaleCpts {
        crv: CrvIn,
        f: f64,
    },
    Translate {
        crv: CrvIn,
        t: [f64; 3],
    },
    Project {
        crv: CrvIn,
        o: [f64; 3],
        n: [f64; 3],
        e: [f64; 3],
        #[serde(default)]
        perspective: bool,
        #[serde(default)]
        inplace: bool,
    },
    Reverse {
        crv: CrvIn,
        #[serde(default)]
        inplace: bool,
    },
    BboxDiag {
        crv: CrvIn,
    },
    MaxPos {
        crv: CrvIn,
    },
    LinearSegs {
        crv: CrvIn,
        tol: f64,
    },
    DegenSegs {
        crv: CrvIn,
        tol: f64,
    },
    Reparam {
        crv: CrvIn,
        ul: f64,
        ur: f64,
    },
    GetType {
        crv: CrvIn,
        tol: f64,
    },
    CurvDeriv {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    Closed {
        crv: CrvIn,
        n: usize,
        tols: [f64; 3],
    },
}

impl Inputs {
    fn crv(&self) -> Option<&CrvIn> {
        match self {
            Inputs::Line { .. } => None,
            Inputs::Greville { crv, .. }
            | Inputs::Curvature { crv, .. }
            | Inputs::Eval { crv, .. }
            | Inputs::Bbox { crv }
            | Inputs::Derivs { crv, .. }
            | Inputs::Tangent { crv, .. }
            | Inputs::Frenet { crv, .. }
            | Inputs::Transform { crv, .. }
            | Inputs::MinMax { crv }
            | Inputs::Scale { crv, .. }
            | Inputs::ScaleCpts { crv, .. }
            | Inputs::Translate { crv, .. }
            | Inputs::Project { crv, .. }
            | Inputs::Reverse { crv, .. }
            | Inputs::BboxDiag { crv }
            | Inputs::MaxPos { crv }
            | Inputs::LinearSegs { crv, .. }
            | Inputs::DegenSegs { crv, .. }
            | Inputs::Reparam { crv, .. }
            | Inputs::GetType { crv, .. }
            | Inputs::CurvDeriv { crv, .. }
            | Inputs::Closed { crv, .. } => Some(crv),
        }
    }

    /// The case's parameter `u`, if any.
    fn param(&self) -> Option<Param> {
        match self {
            Inputs::Curvature { u, .. }
            | Inputs::Eval { u, .. }
            | Inputs::Derivs { u, .. }
            | Inputs::Tangent { u, .. }
            | Inputs::Frenet { u, .. }
            | Inputs::CurvDeriv { u, .. } => Some(*u),
            _ => None,
        }
    }

    /// The case's side, if any.
    fn side(&self) -> SideIn {
        match self {
            Inputs::Curvature { side, .. }
            | Inputs::Eval { side, .. }
            | Inputs::Derivs { side, .. }
            | Inputs::Tangent { side, .. }
            | Inputs::Frenet { side, .. }
            | Inputs::CurvDeriv { side, .. } => *side,
            _ => SideIn::Left,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// Whitespace-separated numbers of an upstream text file.
fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| harness(format!("{path}: {e}")))
        })
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.smc` text file (as `N_CrvReadFromFilePtr`, without its
/// validity check and pruning): `n`, `p`, rational flag, dimension, `n+1` points
/// (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let p = int(path, num()?)?;
    let rat = num()? != 0.0;
    let dim = int(path, num()?)?;
    let mut cpts = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let (x, y) = (num()?, num()?);
        let z = if dim == 3 { num()? } else { NOZ };
        let w = if rat { num()? } else { NOW };
        cpts.push([x, y, z, w]);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineCrv { cpts, p, u })
}

fn resolve(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn to_curve(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))
}


fn resolve_opt(c: Option<&CrvIn>) -> Result<Option<InlineCrv>, HarnessError> {
    c.map(resolve).transpose()
}

// ---- records ----------------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = at
        .checked_add(k)
        .and_then(|end| flat.get(*at..end))
        .ok_or_else(|| harness(format!("record too short ({} values)", flat.len())))?;
    *at += k;
    Ok(part)
}

fn count(x: f64) -> Result<usize, HarnessError> {
    if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

/// Splits a curve dump starting at `at` into `<pfx>dims`, `<pfx>cpts`, `<pfx>u`.
fn split_dump(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    let c = take(flat, at, 4 * (n + 1))?;
    out = out.values(&format!("{pfx}cpts"), c.to_vec());
    let u = take(flat, at, m + 1)?;
    Ok(out.values(&format!("{pfx}u"), u.to_vec()))
}


/// Splits `k` values at `at` into `key`.
fn vals(flat: &[f64], at: &mut usize, key: &str, k: usize, out: Output) -> Result<Output, HarnessError> {
    Ok(out.values(key, take(flat, at, k)?.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    if ok {
        let a = &mut at;
        out = match i {
            Inputs::Greville { .. } => vals(flat, a, "g", 1, out)?,
            Inputs::Curvature { circle, .. } => {
                out = vals(flat, a, "kappa", 1, out)?;
                out = vals(flat, a, "p", 3, out)?;
                out = vals(flat, a, "t", 3, out)?;
                if *circle {
                    out = vals(flat, a, "c", 3, out)?;
                    out = vals(flat, a, "n", 3, out)?;
                }
                out
            }
            Inputs::Eval { .. } => vals(flat, a, "p", 3, out)?,
            Inputs::Bbox { .. } => vals(flat, a, "box", 6, out)?,
            Inputs::Derivs { der, .. } => vals(flat, a, "d", 3 * (der + 1), out)?,
            Inputs::Tangent { .. } => {
                out = vals(flat, a, "p", 3, out)?;
                vals(flat, a, "t", 3, out)?
            }
            Inputs::Frenet { .. } => {
                out = vals(flat, a, "p", 3, out)?;
                out = vals(flat, a, "t", 3, out)?;
                out = vals(flat, a, "n", 3, out)?;
                vals(flat, a, "b", 3, out)?
            }
            Inputs::MinMax { .. } => vals(flat, a, "w", 4, out)?,
            Inputs::Transform { .. }
            | Inputs::Line { .. }
            | Inputs::Scale { .. }
            | Inputs::ScaleCpts { .. }
            | Inputs::Translate { .. }
            | Inputs::Project { .. }
            | Inputs::Reverse { .. }
            | Inputs::Reparam { .. } => split_dump(flat, a, "", out)?,
            Inputs::BboxDiag { .. } => vals(flat, a, "d", 1, out)?,
            Inputs::MaxPos { .. } => {
                out = vals(flat, a, "p", 3, out)?;
                vals(flat, a, "mag", 1, out)?
            }
            Inputs::LinearSegs { .. } | Inputs::DegenSegs { .. } => {
                let k = take(flat, a, 1)?.to_vec();
                let n = count(k.first().copied().unwrap_or(f64::NAN))?;
                out = out.values("count", k);
                let w = if matches!(i, Inputs::LinearSegs { .. }) { 3 } else { 2 };
                vals(flat, a, "segs", w * n, out)?
            }
            Inputs::GetType { .. } => {
                let t = take(flat, a, 1)?.to_vec();
                let typ = t.first().copied().unwrap_or(f64::NAN);
                out = out.values("type", t);
                if typ == 1.0 {
                    vals(flat, a, "geom", 6, out)?
                } else if typ == 2.0 {
                    vals(flat, a, "geom", 14, out)?
                } else {
                    out
                }
            }
            Inputs::CurvDeriv { .. } => vals(flat, a, "kpp", 1, out)?,
            Inputs::Closed { .. } => vals(flat, a, "closed", 1, out)?,
        };
    }
    if at != flat.len() {
        return Err(harness(format!(
            "record has {} values, layout used {at}",
            flat.len()
        )));
    }
    Ok(out)
}

// ---- C++ side ---------------------------------------------------------------------

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("empty knots"))?;
    o.extend([n as f64, c.p as f64, m as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}


fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let c = resolve_opt(i.crv())?;
    if let Some(c) = &c {
        encode_crv(c, &mut o)?;
    }
    let u = match (i.param(), &c) {
        (Some(p), Some(c)) => p.get(c)?,
        _ => 0.0,
    };
    let s = i.side().flag();
    let op = match i {
        Inputs::Greville { deg, idx, .. } => {
            o.extend([*deg as f64, *idx as f64]);
            Op::Greville
        }
        Inputs::Curvature { circle, .. } => {
            o.extend([u, s, flag(*circle)]);
            Op::Curvature
        }
        Inputs::Eval { .. } => {
            o.extend([u, s]);
            Op::Eval
        }
        Inputs::Bbox { .. } => Op::BBox,
        Inputs::Derivs { der, .. } => {
            o.extend([u, s, *der as f64]);
            Op::Derivs
        }
        Inputs::Tangent { .. } => {
            o.extend([u, s]);
            Op::Tangent
        }
        Inputs::Frenet { .. } => {
            o.extend([u, s]);
            Op::Frenet
        }
        Inputs::Transform { m, .. } => {
            o.extend_from_slice(m);
            Op::Transform
        }
        Inputs::MinMax { .. } => Op::MinMax,
        Inputs::Line { p, v } => {
            o.extend_from_slice(p);
            o.extend_from_slice(v);
            Op::Line
        }
        Inputs::Scale { c, f, .. } => {
            o.extend_from_slice(c);
            o.extend_from_slice(f);
            Op::Scale
        }
        Inputs::ScaleCpts { f, .. } => {
            o.push(*f);
            Op::ScaleCPts
        }
        Inputs::Translate { t, .. } => {
            o.extend_from_slice(t);
            Op::Translate
        }
        Inputs::Project {
            o: po,
            n,
            e,
            perspective,
            inplace,
            ..
        } => {
            o.extend_from_slice(po);
            o.extend_from_slice(n);
            o.extend_from_slice(e);
            o.extend([if *perspective { 2.0 } else { 1.0 }, flag(*inplace)]);
            Op::Project
        }
        Inputs::Reverse { inplace, .. } => {
            o.push(flag(*inplace));
            Op::Reverse
        }
        Inputs::BboxDiag { .. } => Op::BBoxDiag,
        Inputs::MaxPos { .. } => Op::MaxPos,
        Inputs::LinearSegs { tol, .. } => {
            o.push(*tol);
            Op::LinearSegs
        }
        Inputs::DegenSegs { tol, .. } => {
            o.push(*tol);
            Op::DegenSegs
        }
        Inputs::Reparam { ul, ur, .. } => {
            o.extend([*ul, *ur]);
            Op::Reparam
        }
        Inputs::GetType { tol, .. } => {
            o.push(*tol);
            Op::Type
        }
        Inputs::CurvDeriv { .. } => {
            o.extend([u, s]);
            Op::CurvDeriv
        }
        Inputs::Closed { n, tols, .. } => {
            o.push(*n as f64);
            o.extend_from_slice(tols);
            Op::Closed
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn dump(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend(pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

/// Status of a Rust error: the NLib number, `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        _ => -1.0,
    }
}


fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn rust_body(i: &Inputs, cur: Option<Curve>, u: f64, o: &mut Vec<f64>) -> Result<(), SolidError> {
    let side = i.side().side();
    let need = || SolidError::InvalidInput {
        operation: "difftest::nlib_crv_nurb",
        reason: "case needs a curve",
    };
    if let Inputs::Line { p, v } = i {
        dump(o, &line_from_pt_and_vector(v3(*p), v3(*v))?);
        return Ok(());
    }
    let mut cur = cur.ok_or_else(need)?;
    match i {
        Inputs::Line { .. } => {}
        Inputs::Greville { deg, idx, .. } => o.push(greville_abscissa(cur.knots(), *deg, *idx)?),
        Inputs::Curvature { circle, .. } => {
            let c = cur.eval_curvature(u, side, *circle)?;
            o.push(c.kappa);
            push3(o, c.point);
            push3(o, c.tangent);
            if let Some((cc, nn)) = c.circle {
                push3(o, cc);
                push3(o, nn);
            }
        }
        Inputs::Eval { .. } => push3(o, cur.eval(u, side)?),
        Inputs::Bbox { .. } => {
            let b = cur.bbox()?;
            o.extend([b.xl, b.xr, b.yb, b.yt, b.zn, b.zf]);
        }
        Inputs::Derivs { der, .. } => {
            for d in cur.derivs(u, side, *der)? {
                push3(o, d);
            }
        }
        Inputs::Tangent { .. } => {
            let (p, t) = cur.eval_tangent(u, side)?;
            push3(o, p);
            push3(o, t);
        }
        Inputs::Frenet { .. } => {
            let f = cur.eval_frenet_frame(u, side)?;
            push3(o, f.point);
            push3(o, f.tangent);
            push3(o, f.normal);
            push3(o, f.binormal);
        }
        Inputs::Transform { m, .. } => {
            let rows = m.chunks(4).map(<[f64]>::to_vec).collect();
            let rma = RealMatrix::from_rows(3, 3, rows, MatrixType::Full, 3)?;
            cur.transform(&rma)?;
            dump(o, &cur);
        }
        Inputs::MinMax { .. } => {
            let w = cur.min_max_weights_and_pts();
            o.extend([w.wmin, w.wmax, w.pmin, w.pmax]);
        }
        Inputs::Scale { c, f, .. } => {
            cur.scale(v3(*c), v3(*f));
            dump(o, &cur);
        }
        Inputs::ScaleCpts { f, .. } => {
            cur.scale_cpts(*f);
            dump(o, &cur);
        }
        Inputs::Translate { t, .. } => {
            cur.translate(v3(*t));
            dump(o, &cur);
        }
        Inputs::Project {
            o: po,
            n,
            e,
            perspective,
            ..
        } => {
            let prj = if *perspective {
                Projection::Perspective
            } else {
                Projection::Parallel
            };
            dump(o, &cur.project_onto_plane(v3(*po), v3(*n), v3(*e), prj)?);
        }
        Inputs::Reverse { inplace, .. } => {
            if *inplace {
                cur.reverse()?;
                dump(o, &cur);
            } else {
                dump(o, &cur.reversed()?);
            }
        }
        Inputs::BboxDiag { .. } => o.push(cur.bbox_max_diag_dist()?),
        Inputs::MaxPos { .. } => {
            let (p, mag) = cur.max_pos_vector();
            push3(o, p);
            o.push(mag);
        }
        Inputs::LinearSegs { tol, .. } => {
            let segs = cur.linear_segs(*tol)?;
            o.push(segs.len() as f64);
            for s in segs {
                o.extend([s.start, s.end, flag(s.linear)]);
            }
        }
        Inputs::DegenSegs { tol, .. } => {
            let segs = cur.degen_segs(*tol)?;
            o.push(segs.len() as f64);
            for (a, b) in segs {
                o.extend([a, b]);
            }
        }
        Inputs::Reparam { ul, ur, .. } => {
            cur.reparam(*ul, *ur)?;
            dump(o, &cur);
        }
        Inputs::GetType { tol, .. } => {
            let t = cur.get_type(*tol)?;
            o.push(f64::from(t.flag()));
            match t {
                CurveType::Line { start, dir } => {
                    push3(o, start);
                    push3(o, dir);
                }
                CurveType::Circle {
                    center,
                    normal,
                    x_axis,
                    y_axis,
                    radius,
                    angle,
                } => {
                    push3(o, center);
                    push3(o, normal);
                    push3(o, x_axis);
                    push3(o, y_axis);
                    o.extend([radius, angle]);
                }
                CurveType::FreeForm => {}
            }
        }
        Inputs::CurvDeriv { .. } => o.push(cur.curvature_deriv(u, side)?),
        Inputs::Closed { n, tols, .. } => o.push(flag(cur.is_closed_continuity(*n, tols)?)),
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve_opt(i.crv())?;
    let cur = c.as_ref().map(to_curve).transpose()?;
    // Parameters first, so a bad knot index is a harness error on both sides.
    let u = match (i.param(), &c) {
        (Some(p), Some(c)) => p.get(c)?,
        _ => 0.0,
    };
    let mut o = vec![0.0];
    match rust_body(i, cur, u, &mut o) {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(d) => Ok(d),
            None => to_output(i, &[status(&e)]),
        },
    }
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped curve: degree 1..=5 (rarely up to 8), `n - p` interior knots
/// with multiplicities up to `p`, 2D or 3D, rational or not.
fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let p = if rng.chance(0.1) {
        6 + rng.below(3) as usize
    } else {
        1 + rng.below(5) as usize
    };
    let n = p + rng.below(9) as usize;
    let (a, len) = if rng.chance(0.3) {
        (rng.below(5) as f64 - 2.0, 1.0 + rng.below(6) as f64)
    } else {
        (rng.uniform(-5.0, 5.0), rng.uniform(0.25, 10.0))
    };
    let b = a + len;
    let integral = rng.chance(0.3);
    // Distinct values strictly inside (a, b), each with multiplicity 1..=p.
    let mut inner: Vec<f64> = Vec::new();
    let mut used: Vec<f64> = Vec::new();
    let mut remaining = n - p;
    while remaining > 0 {
        let mut v = a + 0.5 * len;
        for attempt in 0..32 {
            v = if integral && attempt < 16 {
                a + len * (1.0 + rng.below(15) as f64) / 16.0
            } else {
                rng.uniform(a, b)
            };
            if v > a && v < b && !used.contains(&v) {
                break;
            }
        }
        if v <= a || v >= b || used.contains(&v) {
            break;
        }
        used.push(v);
        let mult = if rng.chance(0.25) {
            1 + rng.below(p.min(remaining) as u64) as usize
        } else {
            1
        };
        inner.extend(std::iter::repeat_n(v, mult));
        remaining -= mult;
    }
    inner.sort_by(f64::total_cmp);
    // Fewer interior knots than asked for (no distinct value found): fewer points.
    let n = p + inner.len();
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));

    let three_d = rng.chance(0.8);
    let rational = rng.chance(0.3);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

/// Distinct interior knots of `u` (degree `p`).
fn interior(c: &InlineCrv) -> Vec<f64> {
    let p = c.p;
    let mut v: Vec<f64> = c
        .u
        .get(p + 1..c.u.len() - p - 1)
        .unwrap_or(&[])
        .to_vec();
    v.dedup();
    v
}

/// A random rotation (orthonormal columns) from three uniform angles.
fn gen_frame(rng: &mut Rng) -> [[f64; 3]; 3] {
    let (a, b, c) = (
        rng.uniform(0.0, std::f64::consts::TAU),
        rng.uniform(0.0, std::f64::consts::TAU),
        rng.uniform(0.0, std::f64::consts::TAU),
    );
    let (sa, ca) = a.sin_cos();
    let (sb, cb) = b.sin_cos();
    let (sc, cc) = c.sin_cos();
    [
        [ca * cb, ca * sb * sc - sa * cc, ca * sb * cc + sa * sc],
        [sa * cb, sa * sb * sc + ca * cc, sa * sb * cc - ca * sc],
        [-sb, cb * sc, cb * cc],
    ]
}

fn apply(f: &[[f64; 3]; 3], o: [f64; 3], x: f64, y: f64) -> [f64; 3] {
    [
        o[0] + f[0][0] * x + f[0][1] * y,
        o[1] + f[1][0] * x + f[1][1] * y,
        o[2] + f[2][0] * x + f[2][1] * y,
    ]
}

/// A rational quadratic circular arc (or full circle) in a random plane: the
/// standard construction with arcs of at most 90 degrees.
fn gen_circle(rng: &mut Rng) -> InlineCrv {
    let full = rng.chance(0.3);
    let sweep = if full {
        2.0 * std::f64::consts::PI
    } else {
        rng.uniform(0.2, 2.0 * std::f64::consts::PI - 0.2)
    };
    let narcs = if full { 4 } else { 1 + (sweep / (0.5 * std::f64::consts::PI)) as usize };
    let narcs = narcs.min(4);
    let r = rng.uniform(0.1, 20.0);
    let o = rng.point3(-10.0, 10.0);
    let planar = rng.chance(0.3);
    let f = if planar {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        gen_frame(rng)
    };
    let three_d = !planar || rng.chance(0.5);
    let dth = sweep / narcs as f64;
    let w1 = (0.5 * dth).cos();
    let start = rng.uniform(0.0, 6.0);
    let mut cpts = Vec::new();
    let mut push = |pt: [f64; 3], w: f64| {
        let z = if three_d { pt[2] * w } else { NOZ };
        cpts.push([pt[0] * w, pt[1] * w, z, w]);
    };
    let at = |t: f64, s: f64| apply(&f, o, s * t.cos(), s * t.sin());
    push(at(start, r), 1.0);
    for k in 0..narcs {
        let t0 = start + k as f64 * dth;
        let t1 = t0 + dth;
        push(at(0.5 * (t0 + t1), r / w1), w1);
        push(at(t1, r), 1.0);
    }
    let mut u = vec![0.0; 3];
    for k in 1..narcs {
        let v = k as f64 / narcs as f64;
        u.extend([v, v]);
    }
    u.extend([1.0; 3]);
    InlineCrv { cpts, p: 2, u }
}

/// A curve whose control points lie on a line (monotone or with a fold), optionally
/// with a small perpendicular perturbation.
fn gen_line(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    let a = rng.point3(-10.0, 10.0);
    let d = rng.point3(-5.0, 5.0);
    let n = c.cpts.len();
    let fold = rng.chance(0.15);
    let eps = *rng.choose(&[0.0, 0.0, 1e-9, 1e-6, 1e-3]).unwrap_or(&0.0);
    let mut ts: Vec<f64> = (0..n).map(|_| rng.unit()).collect();
    if !fold {
        ts.sort_by(f64::total_cmp);
        if let Some(t) = ts.first_mut() {
            *t = 0.0;
        }
        if let Some(t) = ts.last_mut() {
            *t = 1.0;
        }
    }
    for (cp, t) in c.cpts.iter_mut().zip(ts) {
        let w = if cp[3] == NOW { 1.0 } else { cp[3] };
        let p = [
            a[0] + t * d[0] + eps * rng.uniform(-1.0, 1.0),
            a[1] + t * d[1] + eps * rng.uniform(-1.0, 1.0),
            a[2] + t * d[2] + eps * rng.uniform(-1.0, 1.0),
        ];
        cp[0] = p[0] * w;
        cp[1] = p[1] * w;
        if cp[2] != NOZ {
            cp[2] = p[2] * w;
        }
    }
    c
}

/// A curve with runs of coincident control points (degenerate spans), or entirely
/// collapsed to a point.
fn gen_degen(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    let n = c.cpts.len();
    if rng.chance(0.2) {
        let first = c.cpts.first().copied().unwrap_or([0.0; 4]);
        for cp in &mut c.cpts {
            let w = if cp[3] == NOW { 1.0 } else { cp[3] };
            let f = if first[3] == NOW { 1.0 } else { first[3] };
            cp[0] = first[0] / f * w;
            cp[1] = first[1] / f * w;
            if cp[2] != NOZ {
                cp[2] = first[2] / f * w;
            }
        }
        return c;
    }
    let start = rng.below(n as u64) as usize;
    let len = 1 + rng.below((c.p + 2) as u64) as usize;
    let base = c.cpts.get(start).copied().unwrap_or([0.0; 4]);
    for k in start..(start + len).min(n) {
        if let Some(cp) = c.cpts.get_mut(k) {
            let w = if cp[3] == NOW { 1.0 } else { cp[3] };
            let bw = if base[3] == NOW { 1.0 } else { base[3] };
            cp[0] = base[0] / bw * w;
            cp[1] = base[1] / bw * w;
            if cp[2] != NOZ {
                cp[2] = base[2] / bw * w;
            }
        }
    }
    c
}

/// A closed curve: the last control point equals the first.
fn gen_closed(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    if let (Some(&first), Some(last)) = (c.cpts.first(), c.cpts.last().copied()) {
        let w = if last[3] == NOW { 1.0 } else { last[3] };
        let f = if first[3] == NOW { 1.0 } else { first[3] };
        let n = c.cpts.len();
        if let Some(cp) = c.cpts.get_mut(n - 1) {
            cp[0] = first[0] / f * w;
            cp[1] = first[1] / f * w;
            if cp[2] != NOZ {
                cp[2] = first[2] / f * w;
            }
        }
    }
    c
}

fn gen_any_crv(rng: &mut Rng) -> InlineCrv {
    match rng.below(10) {
        0 | 1 => gen_circle(rng),
        2 => gen_line(rng),
        3 => gen_degen(rng),
        4 => gen_closed(rng),
        _ => gen_crv(rng),
    }
}

/// A parameter for `c`: random interior, an interior knot, an end, or just outside.
fn gen_u(rng: &mut Rng, c: &InlineCrv) -> f64 {
    let a = c.u.first().copied().unwrap_or(0.0);
    let b = c.u.last().copied().unwrap_or(1.0);
    let knots = interior(c);
    let roll = rng.unit();
    if roll < 0.1 {
        if rng.chance(0.5) {
            a
        } else {
            b
        }
    } else if roll < 0.3 && !knots.is_empty() {
        knots.get(rng.below(knots.len() as u64) as usize).copied().unwrap_or(a)
    } else if roll < 0.33 {
        if rng.chance(0.5) {
            a - (b - a) * 1e-3
        } else {
            b + (b - a) * 1e-3
        }
    } else {
        rng.uniform(a, b)
    }
}

fn gen_side(rng: &mut Rng) -> SideIn {
    if rng.chance(0.5) {
        SideIn::Left
    } else {
        SideIn::Right
    }
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng
        .choose(&[1e-12, 1e-9, 1e-6, 1e-4, 1e-3, 1e-2, 0.1, 1.0, 10.0])
        .unwrap_or(&1e-6)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let c = gen_any_crv(rng);
    let u = Param::Value(gen_u(rng, &c));
    let side = gen_side(rng);
    let crv = |c: InlineCrv| CrvIn::Inline(c);
    match rng.below(23) {
        0 => {
            let m = c.u.len() - 1;
            let deg = if rng.chance(0.05) { 0 } else { 1 + rng.below(c.p as u64 + 1) as usize };
            let idx = rng.below((m + 2) as u64) as usize;
            Inputs::Greville { crv: crv(c), deg, idx }
        }
        1 => Inputs::Curvature {
            crv: crv(c),
            u,
            side,
            circle: rng.chance(0.6),
        },
        2 => Inputs::Eval { crv: crv(c), u, side },
        3 => Inputs::Bbox { crv: crv(c) },
        4 => {
            let der = rng.below(6) as usize;
            Inputs::Derivs { crv: crv(c), u, side, der }
        }
        5 => Inputs::Tangent { crv: crv(c), u, side },
        6 => Inputs::Frenet { crv: crv(c), u, side },
        7 => {
            let f = gen_frame(rng);
            let t = rng.point3(-10.0, 10.0);
            let s = rng.uniform(0.2, 3.0);
            let mut m = [0.0; 16];
            for (dst, (row, ti)) in m.chunks_mut(4).zip(f.iter().zip(t)) {
                for (d, v) in dst.iter_mut().zip(row) {
                    *d = s * v;
                }
                if let Some(d) = dst.get_mut(3) {
                    *d = ti;
                }
            }
            let last = if rng.chance(0.3) {
                // Projective row.
                [
                    rng.uniform(-0.01, 0.01),
                    rng.uniform(-0.01, 0.01),
                    rng.uniform(-0.01, 0.01),
                    1.0,
                ]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            };
            if let Some(dst) = m.get_mut(12..) {
                dst.copy_from_slice(&last);
            }
            Inputs::Transform { crv: crv(c), m }
        }
        8 => Inputs::MinMax { crv: crv(c) },
        9 => Inputs::Line {
            p: rng.point3(-10.0, 10.0),
            v: rng.point3(-10.0, 10.0),
        },
        10 => Inputs::Scale {
            crv: crv(c),
            c: rng.point3(-10.0, 10.0),
            f: rng.point3(-3.0, 3.0),
        },
        11 => Inputs::ScaleCpts {
            crv: crv(c),
            f: rng.uniform(-3.0, 3.0),
        },
        12 => Inputs::Translate {
            crv: crv(c),
            t: rng.point3(-10.0, 10.0),
        },
        13 => {
            let n = rng.point3(-1.0, 1.0);
            let e = if rng.chance(0.05) {
                // Parallel to the plane.
                [n[1], -n[0], 0.0]
            } else {
                rng.point3(-20.0, 20.0)
            };
            Inputs::Project {
                crv: crv(c),
                o: rng.point3(-5.0, 5.0),
                n,
                e,
                perspective: rng.chance(0.5),
                inplace: rng.chance(0.3),
            }
        }
        14 => Inputs::Reverse {
            crv: crv(c),
            inplace: rng.chance(0.5),
        },
        15 => Inputs::BboxDiag { crv: crv(c) },
        16 => Inputs::MaxPos { crv: crv(c) },
        17 => Inputs::LinearSegs {
            crv: crv(c),
            tol: gen_tol(rng),
        },
        18 => Inputs::DegenSegs {
            crv: crv(c),
            tol: gen_tol(rng),
        },
        19 => {
            let ul = rng.uniform(-10.0, 10.0);
            let ur = if rng.chance(0.1) { ul } else { ul + rng.uniform(0.01, 20.0) };
            Inputs::Reparam { crv: crv(c), ul, ur }
        }
        20 => Inputs::GetType {
            crv: crv(c),
            tol: gen_tol(rng),
        },
        21 => Inputs::CurvDeriv { crv: crv(c), u, side },
        _ => {
            let n = if rng.chance(0.05) { 3 } else { rng.below(3) as usize };
            Inputs::Closed {
                crv: crv(c),
                n,
                tols: [gen_tol(rng), rng.uniform(0.01, 10.0), gen_tol(rng)],
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_nurb"
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
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
