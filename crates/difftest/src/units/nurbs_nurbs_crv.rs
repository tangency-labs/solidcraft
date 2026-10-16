//! Runner for unit `nurbs.nurbs_crv`: SMLib's local copies of NLib curve routines
//! (unique knots, knot spreading, Bezier extraction, split, trim, basis functions
//! and derivatives, point / derivative evaluation, knot removal, basis maxima,
//! degenerate-curve test, frame extraction).
//!
//! C++: `solidcore_ffi::units::nurbs_nurbs_crv` (upstream
//! `source/SMLib/src/SmNurbsCrv.cpp`). Rust: `solidcore::nurbs::nurbs_crv`.
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record
//! of doubles, split into keys by [`to_output`]: `status` (the routine's returned flag:
//! `0` ok, `1` error, `14` for `sm_CrvTrim`'s `NL_INP_ERR`; `-1` on the Rust side for
//! a port-only error, which never matches), then on success the outputs
//! (`remove_knots` always has its curve). A curve output is `dims = [n, p, m]`, the
//! control points in use (`cpts`, x y z w) and the knots in use (`u`).
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read as
//! `N_CrvReadFromFilePtr` does). Parameters may name a knot of the case's curve
//! (`{"knot": i}`). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::{KnotVector, Side};
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::nurbs_crv as nc;
use solidcore::{NlibCode, SolidError, Vec3};
use solidcore_ffi::units::nurbs_nurbs_crv::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nurbs.nurbs_crv` runner.
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

/// A parameter: a value, or `{"knot": i}` for the curve's knot `U[i]`.
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
    Knots {
        u: Vec<f64>,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
    },
    Copy {
        crv: CrvIn,
    },
    Fixup {
        u: Vec<f64>,
        tol: f64,
    },
    Bez {
        crv: CrvIn,
    },
    Split {
        crv: CrvIn,
        u: Param,
    },
    Trim {
        crv: CrvIn,
        ul: Param,
        ur: Param,
    },
    Epolygon {
        crv: CrvIn,
        k: usize,
        l: usize,
    },
    BasisDerivs {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
        der: usize,
    },
    RatDerivs {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
        der: usize,
    },
    CrvDerivs {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
        der: usize,
    },
    BasisEval {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    CrvEval {
        crv: CrvIn,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    RemoveKnots {
        crv: CrvIn,
        tol: f64,
    },
    SpanMaxima {
        crv: CrvIn,
        i: usize,
        tol: f64,
    },
    BasisIEval {
        crv: CrvIn,
        i: usize,
        u: Param,
        #[serde(default)]
        side: SideIn,
    },
    MaxErr {
        crv: CrvIn,
        r: usize,
        s: usize,
    },
    BasisIDerivs {
        crv: CrvIn,
        i: usize,
        u: Param,
        #[serde(default)]
        side: SideIn,
        der: usize,
    },
    GlobalMax {
        crv: CrvIn,
        i: usize,
        tol: f64,
    },
    Degenerate {
        crv: CrvIn,
        tol: f64,
    },
    Frame {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
    },
}

impl Inputs {
    fn crv(&self) -> Option<&CrvIn> {
        match self {
            Inputs::Knots { .. } | Inputs::Fixup { .. } | Inputs::Frame { .. } => None,
            Inputs::Copy { crv }
            | Inputs::Bez { crv }
            | Inputs::Split { crv, .. }
            | Inputs::Trim { crv, .. }
            | Inputs::Epolygon { crv, .. }
            | Inputs::BasisDerivs { crv, .. }
            | Inputs::RatDerivs { crv, .. }
            | Inputs::CrvDerivs { crv, .. }
            | Inputs::BasisEval { crv, .. }
            | Inputs::CrvEval { crv, .. }
            | Inputs::RemoveKnots { crv, .. }
            | Inputs::SpanMaxima { crv, .. }
            | Inputs::BasisIEval { crv, .. }
            | Inputs::MaxErr { crv, .. }
            | Inputs::BasisIDerivs { crv, .. }
            | Inputs::GlobalMax { crv, .. }
            | Inputs::Degenerate { crv, .. } => Some(crv),
        }
    }

    /// The case's parameters `(u, side)` resolved against its curve.
    fn params(&self, c: Option<&InlineCrv>) -> Result<(f64, f64, SideIn), HarnessError> {
        let need = || harness("parameter needs a curve");
        let g = |p: Param| c.ok_or_else(need).and_then(|c| p.get(c));
        Ok(match self {
            Inputs::Split { u, .. } => (g(*u)?, 0.0, SideIn::Left),
            Inputs::Trim { ul, ur, .. } => (g(*ul)?, g(*ur)?, SideIn::Left),
            Inputs::BasisDerivs { u, side, .. }
            | Inputs::RatDerivs { u, side, .. }
            | Inputs::CrvDerivs { u, side, .. }
            | Inputs::BasisEval { u, side, .. }
            | Inputs::CrvEval { u, side, .. }
            | Inputs::BasisIEval { u, side, .. }
            | Inputs::BasisIDerivs { u, side, .. } => (g(*u)?, 0.0, *side),
            _ => (0.0, 0.0, SideIn::Left),
        })
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
        .map(|t| t.parse::<f64>().map_err(|e| harness(format!("{path}: {e}"))))
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

/// Splits a flat record into keys; `p` is the input curve's degree (0 without one).
fn to_output(i: &Inputs, p: usize, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    let a = &mut at;
    if let Inputs::RemoveKnots { .. } = i {
        out = split_dump(flat, a, "", out)?;
    } else if ok {
        out = match i {
            Inputs::Knots { .. } => {
                let k = take(flat, a, 1)?.to_vec();
                let n = count(k.first().copied().unwrap_or(f64::NAN))?;
                out = out.values("count", k);
                out = vals(flat, a, "knots", n, out)?;
                vals(flat, a, "mults", n, out)?
            }
            Inputs::Fixup { u, .. } => vals(flat, a, "u", u.len(), out)?,
            Inputs::Copy { .. } | Inputs::Trim { .. } => split_dump(flat, a, "", out)?,
            Inputs::Bez { .. } => {
                let k = take(flat, a, 1)?.to_vec();
                let n = count(k.first().copied().unwrap_or(f64::NAN))?;
                out = out.values("count", k);
                for s in 0..n {
                    out = split_dump(flat, a, &format!("seg{s}_"), out)?;
                }
                out
            }
            Inputs::Split { .. } => {
                out = split_dump(flat, a, "l_", out)?;
                split_dump(flat, a, "r_", out)?
            }
            Inputs::Epolygon { .. } => {
                let k = take(flat, a, 1)?.to_vec();
                let n = count(k.first().copied().unwrap_or(f64::NAN))?;
                out = out.values("count", k);
                vals(flat, a, "pts", 3 * n, out)?
            }
            Inputs::BasisDerivs { der, .. } | Inputs::RatDerivs { der, .. } => {
                out = vals(flat, a, "span", 1, out)?;
                vals(flat, a, "nd", (der + 1) * (p + 1), out)?
            }
            Inputs::CrvDerivs { der, .. } => vals(flat, a, "cd", 3 * (der + 1), out)?,
            Inputs::BasisEval { .. } => {
                out = vals(flat, a, "span", 1, out)?;
                vals(flat, a, "n", p + 1, out)?
            }
            Inputs::CrvEval { .. } => vals(flat, a, "c", 3, out)?,
            Inputs::RemoveKnots { .. } => out,
            Inputs::SpanMaxima { .. } => {
                out = vals(flat, a, "min", p + 1, out)?;
                out = vals(flat, a, "max", p + 1, out)?;
                vals(flat, a, "u", 1, out)?
            }
            Inputs::BasisIEval { .. } => vals(flat, a, "n", 1, out)?,
            Inputs::MaxErr { .. } => vals(flat, a, "br", 1, out)?,
            Inputs::BasisIDerivs { der, .. } => vals(flat, a, "nd", der + 1, out)?,
            Inputs::GlobalMax { .. } => vals(flat, a, "max", 2, out)?,
            Inputs::Degenerate { .. } => vals(flat, a, "degenerate", 1, out)?,
            Inputs::Frame { .. } => vals(flat, a, "frame", 9, out)?,
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

fn cpp_input(i: &Inputs, c: Option<&InlineCrv>) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    if let Some(c) = c {
        encode_crv(c, &mut o)?;
    }
    let (u, v, side) = i.params(c)?;
    let s = side.flag();
    let op = match i {
        Inputs::Knots { u: k, ivl } => {
            o.push(k.len() as f64);
            o.extend_from_slice(k);
            match ivl {
                Some([a, b]) => o.extend([1.0, *a, *b]),
                None => o.extend([0.0, 0.0, 0.0]),
            }
            Op::Knots
        }
        Inputs::Copy { .. } => Op::Copy,
        Inputs::Fixup { u: k, tol } => {
            o.push(k.len() as f64);
            o.extend_from_slice(k);
            o.push(*tol);
            Op::Fixup
        }
        Inputs::Bez { .. } => Op::Bez,
        Inputs::Split { .. } => {
            o.push(u);
            Op::Split
        }
        Inputs::Trim { .. } => {
            o.extend([u, v]);
            Op::Trim
        }
        Inputs::Epolygon { k, l, .. } => {
            o.extend([*k as f64, *l as f64]);
            Op::EPolygon
        }
        Inputs::BasisDerivs { der, .. } => {
            o.extend([u, s, *der as f64]);
            Op::BasisDerivs
        }
        Inputs::RatDerivs { der, .. } => {
            o.extend([u, s, *der as f64]);
            Op::RatDerivs
        }
        Inputs::CrvDerivs { der, .. } => {
            o.extend([u, s, *der as f64]);
            Op::CrvDerivs
        }
        Inputs::BasisEval { .. } => {
            o.extend([u, s]);
            Op::BasisEval
        }
        Inputs::CrvEval { .. } => {
            o.extend([u, s]);
            Op::CrvEval
        }
        Inputs::RemoveKnots { tol, .. } => {
            o.push(*tol);
            Op::RemoveKnots
        }
        Inputs::SpanMaxima { i, tol, .. } => {
            o.extend([*i as f64, *tol]);
            Op::SpanMaxima
        }
        Inputs::BasisIEval { i, .. } => {
            o.extend([*i as f64, u, s]);
            Op::BasisIEval
        }
        Inputs::MaxErr { r, s: sm, .. } => {
            o.extend([*r as f64, *sm as f64]);
            Op::MaxErr
        }
        Inputs::BasisIDerivs { i, der, .. } => {
            o.extend([*i as f64, u, s, *der as f64]);
            Op::BasisIDerivs
        }
        Inputs::GlobalMax { i, tol, .. } => {
            o.extend([*i as f64, *tol]);
            Op::GlobalMax
        }
        Inputs::Degenerate { tol, .. } => {
            o.push(*tol);
            Op::Degenerate
        }
        Inputs::Frame { o: po, x, y } => {
            o.extend_from_slice(po);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            Op::Frame
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let c = i.crv().map(resolve).transpose()?;
    let (op, input) = cpp_input(i, c.as_ref())?;
    let p = c.as_ref().map_or(0, |c| c.p);
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, p, &flat),
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

/// The returned flag of a Rust error: `14` for `NL_INP_ERR` (only `sm_CrvTrim`
/// returns that number), `1` for any other upstream error (including `sm_BasisEval`'s
/// domain-end flag, `Unsupported`), `-1` for a port-only one.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib {
            code: NlibCode::Input,
            ..
        } => 14.0,
        SolidError::Nlib { .. }
        | SolidError::Degenerate { .. }
        | SolidError::NotConverged { .. }
        | SolidError::Unsupported { .. } => 1.0,
        _ => -1.0,
    }
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn knot_vec(u: &[f64]) -> Result<KnotVector<'_>, SolidError> {
    KnotVector::from_slice(u)
}

fn rust_body(i: &Inputs, cur: Option<Curve>, prm: (f64, f64, SideIn), o: &mut Vec<f64>) -> Result<(), SolidError> {
    let (u, v, side) = (prm.0, prm.1, prm.2.side());
    let need = || SolidError::InvalidInput {
        operation: "difftest::nurbs_nurbs_crv",
        reason: "case needs a curve",
    };
    match i {
        Inputs::Knots { u: k, ivl } => {
            let e = ivl.map(|[a, b]| Extent1d::from_min_max(a, b));
            let (knots, mults) = nc::get_knots(&knot_vec(k)?, e.as_ref());
            o.push(knots.len() as f64);
            o.extend_from_slice(&knots);
            o.extend(mults.iter().map(|&m| m as f64));
            return Ok(());
        }
        Inputs::Fixup { u: k, tol } => {
            let mut k = k.clone();
            nc::fixup_knot_vector(&mut k, *tol);
            o.extend_from_slice(&k);
            return Ok(());
        }
        Inputs::Frame { o: po, x, y } => {
            let f = Axis2Placement::new(v3(*po), v3(*x), v3(*y));
            let (a, b, c) = nc::extract_frame(&f);
            push3(o, a);
            push3(o, b);
            push3(o, c);
            return Ok(());
        }
        _ => {}
    }
    let mut cur = cur.ok_or_else(need)?;
    let p = cur.degree();
    match i {
        Inputs::Knots { .. } | Inputs::Fixup { .. } | Inputs::Frame { .. } => {}
        Inputs::Copy { .. } => dump(o, &nc::copy_nurb_curve(&nc::copy_nurb_curve(&cur))),
        Inputs::Bez { .. } => {
            let segs = nc::create_bez_segments(&cur)?;
            o.push(segs.len() as f64);
            for s in &segs {
                dump(o, s);
            }
        }
        Inputs::Split { .. } => {
            let (l, r) = nc::crv_split(&cur, u)?;
            dump(o, &l);
            dump(o, &r);
        }
        Inputs::Trim { .. } => dump(o, &nc::crv_trim(&cur, u, v)?),
        Inputs::Epolygon { k, l, .. } => {
            let pts = nc::crv_get_epolygon(&cur, *k, *l)?;
            o.push(pts.len() as f64);
            for q in pts {
                push3(o, q);
            }
        }
        Inputs::BasisDerivs { der, .. } | Inputs::RatDerivs { der, .. } => {
            let bd = if let Inputs::BasisDerivs { .. } = i {
                nc::basis_derivs(&cur.knot_vector()?, p, u, side, *der)?
            } else {
                nc::crv_rational_basis_derivs(&cur, u, side, *der)?
            };
            o.push(bd.span as f64);
            for row in &bd.values {
                o.extend_from_slice(row);
            }
        }
        Inputs::CrvDerivs { der, .. } => {
            for d in nc::crv_derivs(&cur, u, side, *der)? {
                push3(o, d);
            }
        }
        Inputs::BasisEval { .. } => {
            let b = nc::basis_eval(&cur.knot_vector()?, p, u, side)?;
            o.push(b.span as f64);
            o.extend_from_slice(&b.values);
        }
        Inputs::CrvEval { .. } => push3(o, nc::crv_eval(&cur, u, side)?),
        Inputs::RemoveKnots { tol, .. } => {
            // The flag replaces the leading status; the curve always follows.
            let flag = nc::crv_remove_knots(&mut cur, *tol)?;
            if let Some(s) = o.first_mut() {
                *s = if flag { 1.0 } else { 0.0 };
            }
            dump(o, &cur);
        }
        Inputs::SpanMaxima { i, tol, .. } => {
            let sm = nc::basis_find_all_span_maxima(&cur.knot_vector()?, *i, p, *tol)?;
            o.extend_from_slice(&sm.min);
            o.extend_from_slice(&sm.max);
            o.push(sm.u);
        }
        Inputs::BasisIEval { i, .. } => o.push(nc::basis_i_eval(&cur.knot_vector()?, *i, p, u, side)?),
        Inputs::MaxErr { r, s, .. } => o.push(nc::crv_remove_knot_max_err(&cur, *r, *s)?),
        Inputs::BasisIDerivs { i, der, .. } => {
            o.extend(nc::basis_i_derivs(&cur.knot_vector()?, *i, p, u, side, *der)?);
        }
        Inputs::GlobalMax { i, tol, .. } => {
            let (mx, at) = nc::basis_find_global_max(&cur.knot_vector()?, *i, p, *tol)?;
            o.extend([mx, at]);
        }
        Inputs::Degenerate { tol, .. } => {
            o.push(if nc::is_ncrv_degenerate(&cur, *tol)? { 1.0 } else { 0.0 });
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = i.crv().map(resolve).transpose()?;
    let cur = c.as_ref().map(to_curve).transpose()?;
    // Parameters first, so a bad knot index is a harness error on both sides.
    let prm = i.params(c.as_ref())?;
    let p = c.as_ref().map_or(0, |c| c.p);
    let mut o = vec![0.0];
    match rust_body(i, cur, prm, &mut o) {
        Ok(()) => to_output(i, p, &o),
        Err(e) => match Output::unported(&e) {
            Some(d) => Ok(d),
            None => to_output(i, p, &[status(&e)]).or_else(|_| Ok(Output::err(e.to_string()))),
        },
    }
}

// ---- generated cases --------------------------------------------------------------

/// A random clamped curve: degree 1..=5 (rarely up to 8), interior knots with
/// multiplicities up to `p`, 2D or 3D, rational or not.
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
    let n = p + inner.len();
    let mut u = vec![a; p + 1];
    u.extend(inner);
    u.extend(std::iter::repeat_n(b, p + 1));
    let three_d = rng.chance(0.8);
    let rational = rng.chance(0.3);
    let degenerate = rng.chance(0.03);
    let base = rng.point3(-10.0, 10.0);
    let cpts = (0..=n)
        .map(|_| {
            let [x, y, z] = if degenerate { base } else { rng.point3(-10.0, 10.0) };
            let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
            let z = if three_d { z * w } else { NOZ };
            [x * w, y * w, z, if rational { w } else { NOW }]
        })
        .collect();
    InlineCrv { cpts, p, u }
}

/// `v[i]`, or the default when out of range (generator inputs only).
fn at<T: Copy + Default>(v: &[T], i: usize) -> T {
    v.get(i).copied().unwrap_or_default()
}

/// A uniformly chosen entry of `v`.
fn pick<T: Copy + Default>(rng: &mut Rng, v: &[T]) -> T {
    at(v, rng.below(v.len() as u64) as usize)
}

/// The first and last knot.
fn ends(c: &InlineCrv) -> (f64, f64) {
    (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    )
}

/// `alpha * a + beta * b` on homogeneous points, keeping the 2D / non-rational markers.
fn mix(alpha: f64, a: [f64; 4], beta: f64, b: [f64; 4]) -> [f64; 4] {
    let z = if a[2] == NOZ { NOZ } else { alpha * a[2] + beta * b[2] };
    let w = if a[3] == NOW { NOW } else { alpha * a[3] + beta * b[3] };
    [alpha * a[0] + beta * b[0], alpha * a[1] + beta * b[1], z, w]
}

/// Inserts `v` once (Boehm); unchanged when `v` is outside the interior or already
/// has multiplicity `p`.
fn insert_knot(c: &InlineCrv, v: f64) -> InlineCrv {
    let p = c.p;
    let m = c.u.len() - 1;
    let (Some(&a), Some(&b)) = (c.u.get(p), c.u.get(m - p)) else {
        return c.clone();
    };
    if v <= a || v >= b {
        return c.clone();
    }
    let k = (p..m - p).rfind(|&j| at(&c.u, j) <= v).unwrap_or(p);
    let s = c.u.iter().filter(|&&x| x == v).count();
    if s >= p {
        return c.clone();
    }
    let n = c.cpts.len() - 1;
    let mut q = Vec::with_capacity(n + 2);
    for i in 0..=n + 1 {
        let pt = if i + p <= k {
            at(&c.cpts, i)
        } else if i > k - s {
            at(&c.cpts, i - 1)
        } else {
            let al = (v - at(&c.u, i)) / (at(&c.u, i + p) - at(&c.u, i));
            mix(al, at(&c.cpts, i), 1.0 - al, at(&c.cpts, i - 1))
        };
        q.push(pt);
    }
    let mut u = c.u.clone();
    u.insert(k + 1, v);
    InlineCrv { cpts: q, p, u }
}

/// A parameter in or near the domain: random interior, a knot, an end, or outside.
fn gen_u(rng: &mut Rng, c: &InlineCrv) -> Param {
    let (a, b) = ends(c);
    match rng.below(10) {
        0..=4 => Param::Value(rng.uniform(a, b)),
        5..=7 => Param::Knot {
            knot: rng.below(c.u.len() as u64) as usize,
        },
        8 => Param::Value(if rng.chance(0.5) { a } else { b }),
        _ => Param::Value(rng.uniform(a - 0.1 * (b - a), b + 0.1 * (b - a))),
    }
}

fn gen_side(rng: &mut Rng) -> SideIn {
    if rng.chance(0.5) {
        SideIn::Left
    } else {
        SideIn::Right
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let mut c = gen_crv(rng);
    let n = c.cpts.len() - 1;
    let p = c.p;
    let crv = |c: &InlineCrv| CrvIn::Inline(c.clone());
    match rng.below(20) {
        0 => {
            let mut u: Vec<f64> = (0..=rng.below(12)).map(|_| rng.uniform(-3.0, 3.0)).collect();
            u.sort_by(f64::total_cmp);
            if rng.chance(0.3) && u.len() > 1 {
                let u0 = at(&u, 0);
                if let Some(x) = u.get_mut(1) {
                    *x = u0 + rng.uniform(0.0, 2e-12);
                }
            }
            let ivl = if rng.chance(0.5) {
                Some([rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)])
            } else {
                None
            };
            Inputs::Knots { u, ivl }
        }
        1 => Inputs::Copy { crv: crv(&c) },
        2 => {
            let tol = pick(rng, &[1e-8, 1.2e-8, 1e-4, 0.05, 0.3]);
            let mut u: Vec<f64> = Vec::new();
            let mut x = rng.uniform(-2.0, 2.0);
            for _ in 0..2 + rng.below(12) {
                u.push(x);
                x += match rng.below(4) {
                    0 => 0.0,
                    1 => rng.uniform(0.0, 1e-11),
                    2 => rng.uniform(0.0, 2.0 * tol),
                    _ => rng.uniform(0.0, 1.0),
                };
            }
            Inputs::Fixup { u, tol }
        }
        3 => Inputs::Bez { crv: crv(&c) },
        4 => Inputs::Split {
            u: gen_u(rng, &c),
            crv: crv(&c),
        },
        5 => {
            let (a, b) = ends(&c);
            let (ul, ur) = if rng.chance(0.1) {
                (gen_u(rng, &c), gen_u(rng, &c))
            } else {
                let mut x = rng.uniform(a, b);
                let mut y = rng.uniform(a, b);
                if x > y {
                    std::mem::swap(&mut x, &mut y);
                }
                let pick = |rng: &mut Rng, v: f64| {
                    if rng.chance(0.25) {
                        Param::Knot {
                            knot: rng.below(c.u.len() as u64) as usize,
                        }
                    } else {
                        Param::Value(v)
                    }
                };
                (pick(rng, x), pick(rng, y))
            };
            Inputs::Trim { crv: crv(&c), ul, ur }
        }
        6 => {
            let k = rng.below(n as u64 + 2) as usize;
            let l = (k + rng.below(4) as usize).min(n + 3);
            Inputs::Epolygon { crv: crv(&c), k, l }
        }
        7 | 8 => {
            let der = rng.below(5) as usize;
            let u = gen_u(rng, &c);
            let side = gen_side(rng);
            if rng.below(2) == 0 {
                Inputs::BasisDerivs { crv: crv(&c), u, side, der }
            } else {
                Inputs::RatDerivs { crv: crv(&c), u, side, der }
            }
        }
        9 => {
            let der = pick(rng, &[0, 1, 2, 3, 4, 4, 6, 8]);
            Inputs::CrvDerivs {
                u: gen_u(rng, &c),
                side: gen_side(rng),
                crv: crv(&c),
                der,
            }
        }
        10 => Inputs::BasisEval {
            u: gen_u(rng, &c),
            side: gen_side(rng),
            crv: crv(&c),
        },
        11 => Inputs::CrvEval {
            u: gen_u(rng, &c),
            side: gen_side(rng),
            crv: crv(&c),
        },
        12 | 13 => {
            // Insert knots so some are removable.
            for _ in 0..rng.below(6) {
                let (a, b) = ends(&c);
                let v = if rng.chance(0.4) {
                    pick(rng, &c.u)
                } else {
                    rng.uniform(a, b)
                };
                c = insert_knot(&c, v);
            }
            let tol = pick(rng, &[0.0, 1e-12, 1e-9, 1e-6, 1e-3, 0.1, 1.0, 1e3]);
            Inputs::RemoveKnots { crv: crv(&c), tol }
        }
        14 => Inputs::SpanMaxima {
            i: rng.below(n as u64 + 2) as usize,
            tol: pick(rng, &[1e-12, 1e-10, 1e-5 * (ends(&c).1 - ends(&c).0)]),
            crv: crv(&c),
        },
        15 => Inputs::BasisIEval {
            i: rng.below(n as u64 + 2) as usize,
            u: gen_u(rng, &c),
            side: gen_side(rng),
            crv: crv(&c),
        },
        16 => {
            let mut cands = Vec::new();
            for r in p + 1..=n {
                if at(&c.u, r) != at(&c.u, r + 1) {
                    let s = (0..=r).rev().take_while(|&k| at(&c.u, k) == at(&c.u, r)).count();
                    cands.push((r, s));
                }
            }
            if cands.is_empty() {
                Inputs::Bez { crv: crv(&c) }
            } else {
                let (r, s) = pick(rng, &cands);
                Inputs::MaxErr { crv: crv(&c), r, s }
            }
        }
        17 => Inputs::BasisIDerivs {
            i: rng.below(n as u64 + 2) as usize,
            u: gen_u(rng, &c),
            side: gen_side(rng),
            der: rng.below(7) as usize,
            crv: crv(&c),
        },
        18 => Inputs::GlobalMax {
            i: rng.below(n as u64 + 2) as usize,
            tol: pick(rng, &[1e-12, 1e-10, 1e-5 * (ends(&c).1 - ends(&c).0)]),
            crv: crv(&c),
        },
        _ => {
            if rng.chance(0.3) {
                Inputs::Frame {
                    o: rng.point3(-10.0, 10.0),
                    x: rng.point3(-1.0, 1.0),
                    y: rng.point3(-1.0, 1.0),
                }
            } else {
                Inputs::Degenerate {
                    tol: pick(rng, &[1e-12, 1e-6, 1e-3, 20.0]),
                    crv: crv(&c),
                }
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.nurbs_crv"
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
