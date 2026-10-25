//! Runner for unit `nlib.srf_nurb`: NLib surface evaluation (basis derivatives,
//! points, derivatives, normals and pole normals), bounding boxes, affine maps,
//! reversal, reparametrisation, type recognition, smooth closure and the
//! elliptic-ended cone.
//!
//! C++: `solidcore_ffi::units::nlib_srf_nurb` (upstream
//! `source/SMLib/src/NL_SrfNurb.cpp`). Rust: `solidcore::nurbs::srf_nurb`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number, `1000` for upstream's bare error flag; `-1` on the Rust side for any
//! other error, which never matches), then on success the results. A surface is
//! dumped as `dims = [n, m, p, q, r, s]`, `net` (x y z w row-major), `u`, `v`; a
//! curve as `<pfx>dims = [n, p, m]`, `<pfx>pts`, `<pfx>knots`.
//!
//! A surface input is inline (`net` rows of `[x, y, z, w]`, degrees, knots) or an
//! upstream `.sms` file (`{"sms": {"$upstream_file": "Surface.sms"}}`). Generated
//! cases mix random clamped surfaces with planes, spheres, tori, cylinders, cones,
//! surfaces of revolution, extrusions and ruled surfaces built with
//! `solidcore::nurbs::srf_common` (green), optionally rotated and reversed, so that
//! every branch of `N_SrfType` and of the pole evaluators is reached. Every run has at
//! least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::{ConicType, Curve};
use solidcore::nurbs::crv_common::{circ_arc, Arc};
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, MatrixType, RealMatrix, NOW, NOZ};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_common::{corner_pts, cyl_cone, extrude_crv, revolved_srf, ruled_srf, CylCone};
use solidcore::nurbs::srf_nurb::{cone_elliptic_ends, EllipticCone, PtDerivNormal, SurfaceType};
use solidcore::{Result as SolidResult, SolidError, Vec3};
use solidcore_ffi::units::nlib_srf_nurb::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2500;

/// `nlib.srf_nurb` runner.
pub struct Runner;

/// An inline surface (storage = indices in use).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    BasisDerivs {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
        mfl: bool,
        udr: usize,
        vdr: usize,
    },
    EvalPt {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
    },
    Derivs {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
        mfl: bool,
        udr: usize,
        vdr: usize,
    },
    Bbox {
        sur: SrfIn,
    },
    Transform {
        sur: SrfIn,
        /// Row-major 4x4.
        m: Vec<f64>,
    },
    MinMax {
        sur: SrfIn,
    },
    NormalFast {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
    },
    Translate {
        sur: SrfIn,
        t: [f64; 3],
    },
    Rotate {
        sur: SrfIn,
        p: [f64; 3],
        v: [f64; 3],
        al: f64,
    },
    Reverse {
        sur: SrfIn,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
    Type {
        sur: SrfIn,
        tol: f64,
    },
    Reparam {
        sur: SrfIn,
        /// `[us, ue, vs, ve]`.
        r: [f64; 4],
    },
    ClosedSmooth {
        sur: SrfIn,
        n: usize,
        tols: [f64; 2],
        dflg: i32,
    },
    NormalPole {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
    },
    NormalAtPole {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
    },
    ConeElliptic {
        /// `[rf, rr, h, tyf, tyr, txf, txr]`.
        c: [f64; 7],
    },
}

impl Inputs {
    fn sur(&self) -> Option<&SrfIn> {
        match self {
            Inputs::BasisDerivs { sur, .. }
            | Inputs::EvalPt { sur, .. }
            | Inputs::Derivs { sur, .. }
            | Inputs::Bbox { sur }
            | Inputs::Transform { sur, .. }
            | Inputs::MinMax { sur }
            | Inputs::NormalFast { sur, .. }
            | Inputs::Translate { sur, .. }
            | Inputs::Rotate { sur, .. }
            | Inputs::Reverse { sur, .. }
            | Inputs::Type { sur, .. }
            | Inputs::Reparam { sur, .. }
            | Inputs::ClosedSmooth { sur, .. }
            | Inputs::NormalPole { sur, .. }
            | Inputs::NormalAtPole { sur, .. } => Some(sur),
            Inputs::ConeElliptic { .. } => None,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- surface inputs ---------------------------------------------------------------

#[path = "fixtures/nlib_srf_nurb_sms.rs"]
mod sms_reader;

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let sms_reader::ParsedSrf { net, p, q, u, v } = sms_reader::parse_sms(&text, path)?;
    Ok(InlineSrf { net, p, q, u, v })
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

/// `(n, m)` of a rectangular net.
fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn encode(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend([n as f64, m as f64, s.p as f64, s.q as f64]);
    o.extend(s.net.iter().flatten().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn build(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
        .collect();
    Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("rust surface: {e}")))
}

fn inline_of(s: &Surface) -> InlineSrf {
    InlineSrf {
        net: s.rows().map(|r| r.iter().map(|c| c.to_array()).collect()).collect(),
        p: s.p(),
        q: s.q(),
        u: s.u_knots().to_vec(),
        v: s.v_knots().to_vec(),
    }
}

// ---- output layout ----------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = flat
        .get(*at..*at + k)
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

fn split_srf(flat: &[f64], at: &mut usize, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 6)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
    out = out.values("dims", dims.to_vec());
    out = out.values("net", take(flat, at, 4 * (n + 1) * (m + 1))?.to_vec());
    out = out.values("u", take(flat, at, r + 1)?.to_vec());
    Ok(out.values("v", take(flat, at, s + 1)?.to_vec()))
}

fn split_crv(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    out = out.values(&format!("{pfx}pts"), take(flat, at, 4 * (n + 1))?.to_vec());
    Ok(out.values(&format!("{pfx}knots"), take(flat, at, m + 1)?.to_vec()))
}

fn split_pdn(flat: &[f64], at: &mut usize, mut out: Output) -> Result<Output, HarnessError> {
    for key in ["p", "su", "sv", "n"] {
        out = out.values(key, take(flat, at, 3)?.to_vec());
    }
    Ok(out)
}

/// `(p, q)` of the surface input (for the basis derivative layout).
fn degrees(i: &Inputs) -> Result<(usize, usize), HarnessError> {
    let s = resolve(i.sur().ok_or_else(|| harness("no surface"))?)?;
    Ok((s.p, s.q))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        match i {
            Inputs::BasisDerivs { udr, vdr, .. } => {
                let (p, q) = degrees(i)?;
                out = out.values("span", take(flat, &mut at, 2)?.to_vec());
                let k = (udr + 1) * (vdr + 1) * (p + 1) * (q + 1);
                out = out.values("bd", take(flat, &mut at, k)?.to_vec());
            }
            Inputs::EvalPt { .. } => out = out.values("p", take(flat, &mut at, 3)?.to_vec()),
            Inputs::Derivs { udr, vdr, .. } => {
                out = out.values("sd", take(flat, &mut at, 3 * (udr + 1) * (vdr + 1))?.to_vec());
            }
            Inputs::Bbox { .. } => out = out.values("box", take(flat, &mut at, 6)?.to_vec()),
            Inputs::MinMax { .. } => out = out.values("w_p", take(flat, &mut at, 4)?.to_vec()),
            Inputs::NormalFast { .. } | Inputs::NormalPole { .. } | Inputs::NormalAtPole { .. } => {
                out = split_pdn(flat, &mut at, out)?;
            }
            Inputs::Transform { .. }
            | Inputs::Translate { .. }
            | Inputs::Rotate { .. }
            | Inputs::Reverse { .. }
            | Inputs::Reparam { .. }
            | Inputs::ConeElliptic { .. } => out = split_srf(flat, &mut at, out)?,
            Inputs::Type { .. } => {
                out = out.values("kind_dir", take(flat, &mut at, 2)?.to_vec());
                out = out.values("pts", take(flat, &mut at, 18)?.to_vec());
                out = out.values("vals", take(flat, &mut at, 5)?.to_vec());
                let has_a = take(flat, &mut at, 1)?.to_vec();
                let a = has_a.first() == Some(&1.0);
                out = out.values("has_a", has_a);
                if a {
                    out = split_crv(flat, &mut at, "a_", out)?;
                }
                let has_b = take(flat, &mut at, 1)?.to_vec();
                let b = has_b.first() == Some(&1.0);
                out = out.values("has_b", has_b);
                if b {
                    out = split_crv(flat, &mut at, "b_", out)?;
                }
            }
            Inputs::ClosedSmooth { .. } => out = out.values("flag", take(flat, &mut at, 1)?.to_vec()),
        }
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

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    if let Some(s) = i.sur() {
        encode(&resolve(s)?, &mut o)?;
    }
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let op = match i {
        Inputs::BasisDerivs {
            u,
            v,
            ufl,
            vfl,
            mfl,
            udr,
            vdr,
            ..
        }
        | Inputs::Derivs {
            u,
            v,
            ufl,
            vfl,
            mfl,
            udr,
            vdr,
            ..
        } => {
            o.extend([
                *u,
                *v,
                f64::from(*ufl),
                f64::from(*vfl),
                flag(*mfl),
                *udr as f64,
                *vdr as f64,
            ]);
            if matches!(i, Inputs::BasisDerivs { .. }) {
                Op::BasisDerivs
            } else {
                Op::Derivs
            }
        }
        Inputs::EvalPt { u, v, ufl, vfl, .. }
        | Inputs::NormalFast { u, v, ufl, vfl, .. }
        | Inputs::NormalPole { u, v, ufl, vfl, .. }
        | Inputs::NormalAtPole { u, v, ufl, vfl, .. } => {
            o.extend([*u, *v, f64::from(*ufl), f64::from(*vfl)]);
            match i {
                Inputs::EvalPt { .. } => Op::EvalPt,
                Inputs::NormalFast { .. } => Op::NormalFast,
                Inputs::NormalPole { .. } => Op::NormalPole,
                _ => Op::NormalAtPole,
            }
        }
        Inputs::Bbox { .. } => Op::BBox,
        Inputs::Transform { m, .. } => {
            if m.len() != 16 {
                return Err(harness("matrix needs 16 entries"));
            }
            o.extend_from_slice(m);
            Op::Transform
        }
        Inputs::MinMax { .. } => Op::MinMaxWeightPos,
        Inputs::Translate { t, .. } => {
            o.extend_from_slice(t);
            Op::Translate
        }
        Inputs::Rotate { p, v, al, .. } => {
            o.extend_from_slice(p);
            o.extend_from_slice(v);
            o.push(*al);
            Op::Rotate
        }
        Inputs::Reverse { dir, inplace, .. } => {
            o.extend([f64::from(*dir), flag(*inplace)]);
            Op::Reverse
        }
        Inputs::Type { tol, .. } => {
            o.push(*tol);
            Op::Type
        }
        Inputs::Reparam { r, .. } => {
            o.extend_from_slice(r);
            Op::Reparam
        }
        Inputs::ClosedSmooth { n, tols, dflg, .. } => {
            o.push(*n as f64);
            o.extend_from_slice(tols);
            o.push(f64::from(*dflg));
            Op::ClosedSmooth
        }
        Inputs::ConeElliptic { c } => {
            o.extend_from_slice(c);
            Op::ConeElliptic
        }
    };
    Ok((op, o))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
        Err(e) => Err(harness(format!("shim failure: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `1000` for upstream's bare
/// error flag ([`SolidError::Degenerate`] of `cone_elliptic_ends`), `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(SolidError::Degenerate { operation, .. })
            if *operation == "nurbs::srf_nurb::cone_elliptic_ends" =>
        {
            1000.0
        }
        Err(_) => -1.0,
    }
}

fn dump(o: &mut Vec<f64>, s: &Surface) {
    let (n, m, r, ss) = s.array_sizes();
    let (p, q) = s.degrees();
    o.extend([n, m, p, q, r, ss].map(|k| k as f64));
    for row in s.rows() {
        for c in row {
            o.extend_from_slice(&c.to_array());
        }
    }
    o.extend_from_slice(s.u_knots());
    o.extend_from_slice(s.v_knots());
}

fn dump_crv(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n(), c.degree(), c.m()].map(|k| k as f64));
    for pt in c.cpts() {
        o.extend_from_slice(&pt.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn push_pdn(o: &mut Vec<f64>, r: &PtDerivNormal) {
    push3(o, r.p);
    push3(o, r.su);
    push3(o, r.sv);
    push3(o, r.n);
}

fn push_type(o: &mut Vec<f64>, t: &SurfaceType) {
    o.push(f64::from(t.kind.flag()));
    o.push(t.dir.map_or(12.0, |d| f64::from(d.flag())));
    for v in [t.aa, t.bb, t.cc, t.dd, t.xx, t.yy] {
        push3(o, v);
    }
    o.extend([t.r1, t.r2, t.ac, t.al, t.h]);
    for c in [&t.cur_a, &t.cur_b] {
        match c {
            Some(c) => {
                o.push(1.0);
                dump_crv(o, c);
            }
            None => o.push(0.0),
        }
    }
}

fn side(f: i32) -> Result<Side, HarnessError> {
    match f {
        1 => Ok(Side::Left),
        2 => Ok(Side::Right),
        _ => Err(harness(format!("bad side flag {f}"))),
    }
}

fn direction(dir: i32) -> Result<Direction, HarnessError> {
    Direction::from_flag(dir).ok_or_else(|| harness(format!("bad direction flag {dir}")))
}

/// Pushes the status and, on success, what `f` writes.
fn emit<T>(o: &mut Vec<f64>, r: &SolidResult<T>, f: impl FnOnce(&mut Vec<f64>, &T)) {
    o.push(status(r));
    if let Ok(x) = r {
        f(o, x);
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    if let Inputs::ConeElliptic { c } = i {
        let ec = EllipticCone {
            radius_front: c[0],
            radius_rear: c[1],
            height: c[2],
            tilt_y_front: c[3],
            tilt_y_rear: c[4],
            tilt_x_front: c[5],
            tilt_x_rear: c[6],
        };
        emit(&mut o, &cone_elliptic_ends(&ec), dump);
        return Ok(o);
    }
    let mut s = build(&resolve(i.sur().ok_or_else(|| harness("no surface"))?)?)?;
    match i {
        Inputs::BasisDerivs {
            u,
            v,
            ufl,
            vfl,
            mfl,
            udr,
            vdr,
            ..
        } => {
            let r = s.basis_derivs(*u, *v, side(*ufl)?, side(*vfl)?, *mfl, *udr, *vdr);
            emit(&mut o, &r, |o, bd| {
                o.extend([bd.usp as f64, bd.vsp as f64]);
                let (p, q) = bd.degrees();
                for k in 0..=*udr {
                    for l in 0..=*vdr {
                        for a in 0..=p {
                            for b in 0..=q {
                                o.push(bd.get(k, l, a, b).unwrap_or(f64::NAN));
                            }
                        }
                    }
                }
            });
        }
        Inputs::EvalPt { u, v, ufl, vfl, .. } => {
            let r = s.eval(*u, *v, side(*ufl)?, side(*vfl)?);
            emit(&mut o, &r, |o, p| push3(o, *p));
        }
        Inputs::Derivs {
            u,
            v,
            ufl,
            vfl,
            mfl,
            udr,
            vdr,
            ..
        } => {
            let r = s.derivs(*u, *v, side(*ufl)?, side(*vfl)?, *mfl, *udr, *vdr);
            emit(&mut o, &r, |o, sd| {
                for p in sd.iter().flatten() {
                    push3(o, *p);
                }
            });
        }
        Inputs::Bbox { .. } => {
            let r = s.bbox();
            emit(&mut o, &r, |o, b| o.extend([b.xl, b.xr, b.yb, b.yt, b.zn, b.zf]));
        }
        Inputs::Transform { m, .. } => {
            let rows = m.chunks(4).map(<[f64]>::to_vec).collect();
            let rma = RealMatrix::from_rows(3, 3, rows, MatrixType::Full, 3)
                .map_err(|e| harness(format!("matrix: {e}")))?;
            let r = s.transform(&rma).map(|()| s.clone());
            emit(&mut o, &r, dump);
        }
        Inputs::MinMax { .. } => {
            let w = s.min_max_weight_pos_vectors();
            o.extend([0.0, w.wmin, w.wmax, w.pmin, w.pmax]);
        }
        Inputs::NormalFast { u, v, ufl, vfl, .. } => {
            let r = s.eval_pt_deriv_normal_fast(*u, *v, side(*ufl)?, side(*vfl)?);
            emit(&mut o, &r, push_pdn);
        }
        Inputs::NormalPole { u, v, ufl, vfl, .. } => {
            let r = s.eval_pt_deriv_normal_pole(*u, *v, side(*ufl)?, side(*vfl)?);
            emit(&mut o, &r, push_pdn);
        }
        Inputs::NormalAtPole { u, v, ufl, vfl, .. } => {
            let r = s.eval_pt_normal_at_pole(*u, *v, side(*ufl)?, side(*vfl)?);
            emit(&mut o, &r, push_pdn);
        }
        Inputs::Translate { t, .. } => {
            s.translate(v3(*t));
            o.push(0.0);
            dump(&mut o, &s);
        }
        Inputs::Rotate { p, v, al, .. } => {
            let r = s.rotate_at_pt(v3(*p), v3(*v), *al).map(|()| s.clone());
            emit(&mut o, &r, dump);
        }
        Inputs::Reverse { dir, inplace, .. } => {
            let d = direction(*dir)?;
            let r = if *inplace {
                s.reverse(d).map(|()| s.clone())
            } else {
                s.reversed(d)
            };
            emit(&mut o, &r, dump);
        }
        Inputs::Type { tol, .. } => {
            let r = s.surface_type(*tol);
            emit(&mut o, &r, push_type);
        }
        Inputs::Reparam { r, .. } => {
            let res = s.reparam(r[0], r[1], r[2], r[3]).map(|()| s.clone());
            emit(&mut o, &res, dump);
        }
        Inputs::ClosedSmooth { n, tols, dflg, .. } => {
            let r = s.is_closed_smooth(*n, tols, direction(*dflg)?);
            emit(&mut o, &r, |o, f| o.push(f.map_or(0.0, |d| f64::from(d.flag()))));
        }
        Inputs::ConeElliptic { .. } => return Err(harness("unreachable")),
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-1e3, 1e3),
        2 => rng.uniform(-1.0, 1.0) * 1e-6,
        _ => rng.uniform(-10.0, 10.0),
    }
}

/// A valid clamped knot vector for degree `p` with `cnt` knots over `[a, b]`.
fn knots(rng: &mut Rng, cnt: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = cnt.saturating_sub(2 * p + 2);
    let grid = rng.chance(0.4);
    let steps = 2 + rng.below(4);
    let mut mid: Vec<f64> = Vec::with_capacity(interior);
    let mut tries = 0;
    while mid.len() < interior {
        let x = if grid {
            a + (b - a) * (1 + rng.below(steps)) as f64 / (steps + 1) as f64
        } else {
            rng.uniform(a, b)
        };
        tries += 1;
        let mult = mid.iter().filter(|&&y| y == x).count();
        if x > a && x < b && (mult < p || tries > 1000) {
            if mult < p {
                mid.push(x);
            } else {
                mid.push(rng.uniform(a, b).max(a + (b - a) * 1e-3).min(b - (b - a) * 1e-3));
            }
        }
    }
    mid.sort_by(f64::total_cmp);
    let mut k = vec![a; p + 1];
    k.extend(mid);
    k.extend(std::iter::repeat_n(b, p + 1));
    k
}

fn knots_valid(u: &[f64], p: usize) -> bool {
    let (Some(&first), Some(&last)) = (u.first(), u.last()) else {
        return false;
    };
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &x in u {
        match runs.last_mut() {
            Some((y, k)) if *y == x => *k += 1,
            _ => runs.push((x, 1)),
        }
    }
    first < last
        && runs
            .iter()
            .all(|&(x, k)| if x == first || x == last { k == p + 1 } else { k <= p })
}

fn weight(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0..=2 => 1.0,
        3 => rng.uniform(0.01, 0.1),
        _ => rng.uniform(0.5, 2.0),
    }
}

fn domain(rng: &mut Rng) -> (f64, f64) {
    match rng.below(4) {
        0 => (0.0, 1.0),
        1 => (rng.uniform(-5.0, 0.0), rng.uniform(0.5, 5.0)),
        2 => (rng.uniform(-100.0, 100.0), rng.uniform(150.0, 300.0)),
        _ => (0.0, rng.uniform(0.1, 10.0)),
    }
}

/// A random valid clamped surface.
fn gen_random_srf(rng: &mut Rng) -> InlineSrf {
    let deg = |rng: &mut Rng| {
        if rng.chance(0.05) {
            5 + rng.below(4) as usize
        } else {
            1 + rng.below(3) as usize
        }
    };
    let p = deg(rng);
    let q = deg(rng);
    let n = p + rng.below(5) as usize;
    let m = q + rng.below(5) as usize;
    let rational = rng.chance(0.4);
    let two_d = rng.chance(0.08);
    let net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|_| {
            (0..=m)
                .map(|_| {
                    let (x, y) = (coord(rng), coord(rng));
                    let z = if two_d { NOZ } else { coord(rng) };
                    if rational {
                        let w = weight(rng);
                        let zw = if z == NOZ { NOZ } else { z * w };
                        [x * w, y * w, zw, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    let (a, b) = domain(rng);
    let (c, d) = domain(rng);
    let mut u = knots(rng, n + p + 2, p, a, b);
    let mut v = knots(rng, m + q + 2, q, c, d);
    if !knots_valid(&u, p) {
        u = knots(rng, n + p + 2, p, 0.0, 1.0);
    }
    if !knots_valid(&v, q) {
        v = knots(rng, m + q + 2, q, 0.0, 1.0);
    }
    InlineSrf { net, p, q, u, v }
}

fn unit_vec(rng: &mut Rng) -> Vec3 {
    loop {
        let [x, y, z] = rng.point3(-1.0, 1.0);
        let v = Vec3::new(x, y, z);
        let l = v.length();
        if l > 0.1 {
            return v * (1.0 / l);
        }
    }
}

/// A random non-rational B-spline curve (3D) with `n + 1` points.
fn gen_curve(rng: &mut Rng, planar: bool) -> Option<Curve> {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let pts: Vec<CPoint> = (0..=n)
        .map(|_| {
            let [x, y, z] = rng.point3(-5.0, 5.0);
            let z = if planar { 0.0 } else { z };
            CPoint::new(x, y, z, NOW)
        })
        .collect();
    let u = knots(rng, n + p + 2, p, 0.0, 1.0);
    if !knots_valid(&u, p) {
        return None;
    }
    Curve::new(pts, p, u).ok()
}

fn gen_arc(rng: &mut Rng, r: f64, start: f64, end: f64) -> Option<Curve> {
    let center = v3(rng.point3(-3.0, 3.0));
    let x = unit_vec(rng);
    let t = unit_vec(rng);
    let y = x.cross(t);
    let l = y.length();
    if l < 0.1 {
        return None;
    }
    let ctp = if rng.chance(0.8) {
        ConicType::Quadratic
    } else {
        ConicType::Quartic
    };
    circ_arc(
        &Arc {
            center,
            x,
            y: y * (1.0 / l),
            r,
            start,
            end,
        },
        ctp,
    )
    .ok()
}

/// A special surface built with `srf_common` (plane, cylinder, cone, sphere, torus,
/// revolution, extrusion, ruled).
fn gen_special_srf(rng: &mut Rng) -> Option<Surface> {
    match rng.below(9) {
        0 => {
            // Bilinear: planar or twisted.
            let p00 = v3(rng.point3(-5.0, 5.0));
            let a = v3(rng.point3(-5.0, 5.0));
            let b = v3(rng.point3(-5.0, 5.0));
            let twist = if rng.chance(0.5) {
                v3(rng.point3(-1.0, 1.0))
            } else {
                Vec3::ZERO
            };
            corner_pts(p00, p00 + a, p00 + b, p00 + a + b + twist).ok()
        }
        1 | 2 => {
            // Cylinder / cone (patch), apex possible.
            let x = unit_vec(rng);
            let y = x.cross(unit_vec(rng));
            let l = y.length();
            if l < 0.1 {
                return None;
            }
            let rb = if rng.chance(0.15) { 0.0 } else { rng.uniform(0.5, 4.0) };
            let rt = if rng.chance(0.4) { rb } else { rng.uniform(0.0, 4.0) };
            let start = if rng.chance(0.5) { 0.0 } else { rng.uniform(-90.0, 90.0) };
            let end = if rng.chance(0.5) { start + 360.0 } else { start + rng.uniform(30.0, 300.0) };
            let cc = CylCone {
                center: v3(rng.point3(-3.0, 3.0)),
                x,
                y: y * (1.0 / l),
                rb,
                rt,
                start,
                end,
                h: rng.uniform(0.5, 6.0),
            };
            let ctp = if rng.chance(0.8) {
                ConicType::Quadratic
            } else {
                ConicType::Quartic
            };
            let dir = if rng.chance(0.5) { Direction::U } else { Direction::V };
            cyl_cone(&cc, ctp, dir).ok()
        }
        3 | 4 => {
            // Sphere (semicircle about its diameter) or torus (circle about an axis).
            let sphere = rng.below(2) == 0;
            let r = rng.uniform(0.5, 3.0);
            let s = Vec3::ZERO;
            let t = Vec3::new(0.0, 0.0, 1.0);
            let mut prof = if sphere {
                circ_arc(
                    &Arc {
                        center: Vec3::ZERO,
                        x: Vec3::new(1.0, 0.0, 0.0),
                        y: Vec3::new(0.0, 0.0, 1.0),
                        r,
                        start: -90.0,
                        end: if rng.chance(0.7) { 90.0 } else { rng.uniform(-30.0, 80.0) },
                    },
                    ConicType::Quadratic,
                )
                .ok()?
            } else {
                circ_arc(
                    &Arc {
                        center: Vec3::new(r + rng.uniform(0.5, 3.0), 0.0, 0.0),
                        x: Vec3::new(1.0, 0.0, 0.0),
                        y: Vec3::new(0.0, 0.0, 1.0),
                        r,
                        start: 0.0,
                        end: if rng.chance(0.6) { 360.0 } else { rng.uniform(40.0, 300.0) },
                    },
                    ConicType::Quadratic,
                )
                .ok()?
            };
            let al = if rng.chance(0.5) { 360.0 } else { rng.uniform(20.0, 330.0) };
            revolved_srf(&mut prof, s, t, al, ConicType::Quadratic).ok()
        }
        5 => {
            // General revolution of a (planar or not) curve.
            let planar = rng.chance(0.6);
            let mut prof = gen_curve(rng, planar)?;
            let s = v3(rng.point3(-8.0, -6.0));
            let t = unit_vec(rng);
            let al = if rng.chance(0.4) { 360.0 } else { rng.uniform(20.0, 330.0) };
            revolved_srf(&mut prof, s, t, al, ConicType::Quadratic).ok()
        }
        6 => {
            // Extrusion of a curve or arc.
            let mut cur = if rng.chance(0.3) {
                {
                let (r, e) = (rng.uniform(0.5, 3.0), rng.uniform(30.0, 360.0));
                gen_arc(rng, r, 0.0, e)?
            }
            } else {
                gen_curve(rng, false)?
            };
            let w = unit_vec(rng);
            let d = rng.uniform(0.5, 5.0);
            let dir = if rng.chance(0.5) { Direction::U } else { Direction::V };
            extrude_crv(&mut cur, w, d, dir).ok()
        }
        7 => {
            // Ruled surface between two curves.
            let mut a = gen_curve(rng, false)?;
            let mut b = gen_curve(rng, false)?;
            let dir = if rng.chance(0.5) { Direction::U } else { Direction::V };
            ruled_srf(&mut a, &mut b, dir).ok()
        }
        _ => {
            // Line-ruled: a straight curve and its parallel copy or a skew line.
            let p0 = v3(rng.point3(-5.0, 5.0));
            let d0 = v3(rng.point3(-5.0, 5.0));
            let mut a = solidcore::nurbs::crv_nurb::line_from_pt_and_vector(p0, d0).ok()?;
            let off = v3(rng.point3(-3.0, 3.0));
            let d1 = if rng.chance(0.5) { d0 } else { v3(rng.point3(-5.0, 5.0)) };
            let mut b = solidcore::nurbs::crv_nurb::line_from_pt_and_vector(p0 + off, d1).ok()?;
            let dir = if rng.chance(0.5) { Direction::U } else { Direction::V };
            ruled_srf(&mut a, &mut b, dir).ok()
        }
    }
}

fn gen_srf(rng: &mut Rng) -> InlineSrf {
    if rng.chance(0.5) {
        for _ in 0..8 {
            if let Some(mut s) = gen_special_srf(rng) {
                if rng.chance(0.3) {
                    let _ = s.rotate_at_pt(v3(rng.point3(-2.0, 2.0)), unit_vec(rng), rng.uniform(-180.0, 180.0));
                }
                if rng.chance(0.2) {
                    let d = if rng.chance(0.5) { Direction::U } else { Direction::V };
                    let _ = s.reverse(d);
                }
                let inl = inline_of(&s);
                let (Ok((n, m)), true, true) = (shape(&inl), inl.p >= 1, inl.q >= 1) else {
                    continue;
                };
                if inl.p <= n
                    && inl.q <= m
                    && knots_valid(&inl.u, inl.p)
                    && knots_valid(&inl.v, inl.q)
                    && inl.u.len() == n + inl.p + 2
                    && inl.v.len() == m + inl.q + 2
                    && inl.net.iter().flatten().flatten().all(|x| x.is_finite())
                {
                    return inl;
                }
            }
        }
    }
    gen_random_srf(rng)
}

/// A parameter for knots `k`: an interior knot, a random interior value, an end, a
/// value just inside an end, or (when `outside`) a value beyond the domain.
fn param(rng: &mut Rng, k: &[f64], outside: bool) -> f64 {
    let a = k.first().copied().unwrap_or(0.0);
    let b = k.last().copied().unwrap_or(1.0);
    let interior: Vec<f64> = k.iter().copied().filter(|&x| x > a && x < b).collect();
    match rng.below(12) {
        0..=2 => rng.choose(&interior).copied().unwrap_or((a + b) / 2.0),
        3 | 4 => {
            if rng.chance(0.5) {
                a
            } else {
                b
            }
        }
        5 if outside => {
            if rng.chance(0.5) {
                a - (b - a) * rng.uniform(0.01, 0.5)
            } else {
                b + (b - a) * rng.uniform(0.01, 0.5)
            }
        }
        6 => a + (b - a) * 1e-9,
        _ => rng.uniform(a, b),
    }
}

fn gen_side(rng: &mut Rng) -> i32 {
    1 + rng.below(2) as i32
}

fn gen_dir(rng: &mut Rng) -> i32 {
    if rng.chance(0.05) {
        3
    } else {
        1 + rng.below(2) as i32
    }
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-6, 1e-4, 1e-3, 1e-9, 1e-2]).unwrap_or(&1e-6)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.04) {
        let ang = |rng: &mut Rng| match rng.below(4) {
            0 => 0.0,
            1 => rng.uniform(-1e-9, 1e-9),
            _ => rng.uniform(-60.0, 60.0),
        };
        let rf = if rng.chance(0.1) { 0.0 } else { rng.uniform(0.2, 5.0) };
        let rr = if rng.chance(0.3) { rf } else { rng.uniform(0.0, 5.0) };
        let h = if rng.chance(0.05) { 0.0 } else { rng.uniform(0.2, 8.0) };
        return Inputs::ConeElliptic {
            c: [rf, rr, h, ang(rng), ang(rng), ang(rng), ang(rng)],
        };
    }
    let s = gen_srf(rng);
    let (u, v) = (param(rng, &s.u, true), param(rng, &s.v, true));
    let (ui, vi) = (param(rng, &s.u, false), param(rng, &s.v, false));
    let (ufl, vfl) = (gen_side(rng), gen_side(rng));
    let sur = SrfIn::Inline(s);
    match rng.below(16) {
        0 => {
            let udr = rng.below(6) as usize;
            let vdr = if rng.chance(0.5) { udr } else { rng.below(6) as usize };
            Inputs::BasisDerivs {
                sur,
                u,
                v,
                ufl,
                vfl,
                mfl: rng.chance(0.5),
                udr,
                vdr,
            }
        }
        1 => Inputs::EvalPt { sur, u, v, ufl, vfl },
        2 | 3 => {
            let udr = rng.below(5) as usize;
            let vdr = if rng.chance(0.5) { udr } else { rng.below(5) as usize };
            Inputs::Derivs {
                sur,
                u,
                v,
                ufl,
                vfl,
                mfl: rng.chance(0.5),
                udr,
                vdr,
            }
        }
        4 => {
            if rng.chance(0.5) {
                Inputs::Bbox { sur }
            } else {
                Inputs::MinMax { sur }
            }
        }
        5 => {
            let mut m: Vec<f64> = (0..16).map(|_| rng.uniform(-2.0, 2.0)).collect();
            if rng.chance(0.7) {
                m.truncate(12);
                m.extend([0.0, 0.0, 0.0, 1.0]);
            }
            Inputs::Transform { sur, m }
        }
        6 | 7 => Inputs::NormalFast {
            sur,
            u: ui,
            v: vi,
            ufl,
            vfl,
        },
        8 => {
            if rng.chance(0.5) {
                Inputs::Translate {
                    sur,
                    t: rng.point3(-10.0, 10.0),
                }
            } else {
                let vv = if rng.chance(0.05) { [0.0; 3] } else { rng.point3(-1.0, 1.0) };
                Inputs::Rotate {
                    sur,
                    p: rng.point3(-5.0, 5.0),
                    v: vv,
                    al: rng.uniform(-360.0, 360.0),
                }
            }
        }
        9 => Inputs::Reverse {
            sur,
            dir: gen_dir(rng),
            inplace: rng.chance(0.5),
        },
        10..=12 => Inputs::Type { sur, tol: gen_tol(rng) },
        13 => {
            let (a, b, c, d) = match &sur {
                SrfIn::Inline(s) => (
                    s.u.first().copied().unwrap_or(0.0),
                    s.u.last().copied().unwrap_or(1.0),
                    s.v.first().copied().unwrap_or(0.0),
                    s.v.last().copied().unwrap_or(1.0),
                ),
                SrfIn::File { .. } => (0.0, 1.0, 0.0, 1.0),
            };
            let mut r = [a, b, c, d];
            for x in &mut r {
                if rng.chance(0.5) {
                    *x += rng.uniform(-3.0, 3.0);
                }
            }
            if r[0] >= r[1] {
                r[1] = r[0] + 1.0;
            }
            if r[2] >= r[3] {
                r[3] = r[2] + 1.0;
            }
            Inputs::Reparam { sur, r }
        }
        14 => Inputs::ClosedSmooth {
            sur,
            n: if rng.chance(0.05) { 2 } else { rng.below(2) as usize },
            tols: [gen_tol(rng), *rng.choose(&[0.5, 1.0, 5.0, 1e-3]).unwrap_or(&1.0)],
            dflg: 1 + rng.below(3) as i32,
        },
        _ => {
            // Poles: mostly at the ends of the domain.
            let end = |rng: &mut Rng, k: &[f64], x: f64| {
                if rng.chance(0.6) {
                    let a = k.first().copied().unwrap_or(0.0);
                    let b = k.last().copied().unwrap_or(1.0);
                    if rng.chance(0.5) {
                        a
                    } else {
                        b
                    }
                } else {
                    x
                }
            };
            let (ku, kv) = match &sur {
                SrfIn::Inline(s) => (s.u.clone(), s.v.clone()),
                SrfIn::File { .. } => (vec![0.0, 1.0], vec![0.0, 1.0]),
            };
            let ui = end(rng, &ku, ui);
            let vi = end(rng, &kv, vi);
            if rng.chance(0.5) {
                Inputs::NormalPole {
                    sur,
                    u: ui,
                    v: vi,
                    ufl,
                    vfl,
                }
            } else {
                Inputs::NormalAtPole {
                    sur,
                    u: ui,
                    v: vi,
                    ufl,
                    vfl,
                }
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_nurb"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generator reaches every surface type, ruled outputs, poles and errors.
    #[test]
    fn generated_cases_cover_branches() {
        let mut kinds: BTreeMap<i32, usize> = BTreeMap::new();
        let mut statuses: BTreeMap<String, usize> = BTreeMap::new();
        let (mut poles, mut cur_b) = (0usize, 0usize);
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("nlib.srf_nurb", i));
            let inputs = gen_case(&mut rng);
            let flat = rust_flat(&inputs).unwrap();
            let op = serde_json::to_value(&inputs).unwrap()["op"].as_str().unwrap().to_string();
            *statuses.entry(format!("{op}:{}", flat[0])).or_default() += 1;
            if flat[0] != 0.0 {
                continue;
            }
            match &inputs {
                Inputs::Type { sur, tol } => {
                    let t = build(&resolve(sur).unwrap()).unwrap().surface_type(*tol).unwrap();
                    *kinds.entry(t.kind.flag()).or_default() += 1;
                    cur_b += usize::from(t.cur_b.is_some());
                }
                Inputs::NormalPole { .. } | Inputs::NormalAtPole { .. } | Inputs::NormalFast { .. } => {
                    let zero = |r: &[f64]| r.iter().all(|x| *x == 0.0);
                    if zero(&flat[4..7]) || zero(&flat[7..10]) {
                        poles += 1;
                    }
                }
                _ => {}
            }
        }
        eprintln!("kinds {kinds:?}\npoles {poles} cur_b {cur_b}\nstatuses {statuses:?}");
        for k in 3..=11 {
            assert!(kinds.get(&k).copied().unwrap_or(0) > 0, "surface kind {k} never generated");
        }
        assert!(poles > 0, "no pole evaluation");
        assert!(cur_b > 0, "no ruled surface output");
    }
}
