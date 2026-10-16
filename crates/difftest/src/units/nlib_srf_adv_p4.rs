//! Runner for unit `nlib.srf_adv_p4`: NLib Coons surfaces, cross-boundary derivative
//! construction and approximation, derivative strip adjustment, sweep frame helpers
//! and knot vector merging.
//!
//! C++: `solidcore_ffi::units::nlib_srf_adv_p4` (upstream
//! `source/SMLib/src/NL_SrfAdv.cpp`). Rust: `solidcore::nurbs::srf_adv::p4`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number, `1000` for upstream's bare error flag; `-1` on the Rust side for any other
//! error, which never matches), then on success the results. Surfaces are split into
//! `dims = [n, m, p, q, r, s]`, `net` (x y z w row-major), `u`, `v`; curves into
//! `<pfx>dims = [n, p, m]`, `<pfx>net`, `<pfx>u`. Input curves that upstream modifies
//! in place are dumped after the call (`c<i>...`). Upstream's in-place modes
//! (`inplace`) are compared against the same Rust call.
//!
//! Curves are inline or upstream `.smc` files (`{"smc": {"$upstream_file": ...}}`).
//! Inputs must be valid clamped surfaces and curves (the FFI wrapper rejects anything
//! else). Every run has at least [`MIN_GENERATED`] generated cases.

// Harness code: the input generators index their own freshly sized arrays; a panic
// would fail the case (runner panics are caught).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::BARE_FLAG_REASON;
use solidcore::nurbs::crv_basic::{Boundary, Curve};
use solidcore::nurbs::knots::KnotVector;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_adv::p4 as adv;
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_adv_p4::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.srf_adv_p4` runner.
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

/// An inline curve.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// The z source of `rot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum ZIn {
    Curve {
        zcur: CrvIn,
    },
    Samples {
        z: [f64; 3],
        prev: Option<([f64; 3], f64)>,
        next: Option<([f64; 3], f64)>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Bvec {
        cur: CrvIn,
        b0: [f64; 3],
        t: Vec<f64>,
    },
    Coons {
        /// `U0 U1 V0 V1`.
        curves: Vec<CrvIn>,
    },
    SkinDerivs {
        cur: CrvIn,
        v: [f64; 3],
        bndy: i32,
    },
    Merge {
        p: usize,
        r: Vec<f64>,
        s: Vec<f64>,
    },
    Rot {
        traj: CrvIn,
        u: f64,
        zsrc: ZIn,
    },
    SweepTangent {
        dtraj: [f64; 3],
        scale: [f64; 3],
        dscale: [f64; 3],
        rma: [[f64; 3]; 3],
        drma: [[f64; 3]; 3],
        cur: CrvIn,
    },
    AdjustDeriv {
        sur: InlineSrf,
        dir: i32,
        cst: i32,
        usg: i32,
        #[serde(default)]
        inplace: bool,
    },
    AdjustWeight {
        pw: [f64; 4],
        qw: [f64; 4],
        rw: [f64; 4],
    },
    Approx {
        cur: CrvIn,
        ds: [f64; 3],
        de: [f64; 3],
        eps: f64,
        #[serde(default)]
        inplace: bool,
    },
    CoonsBoundary {
        /// `L dL R dR B dB T dT`.
        curves: Vec<CrvIn>,
        cpf: bool,
    },
    CrossDeriv {
        cur: CrvIn,
        ns: [f64; 3],
        ne: [f64; 3],
        bs: [f64; 3],
        be: [f64; 3],
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

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

/// Reads an upstream `.smc` text file: `n`, `p`, rational flag, dimension, `n+1`
/// points (`x y [z]`, or `wx wy [wz] w` when rational), `n+p+2` knots.
fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let all = numbers(path)?;
    let mut it = all.into_iter();
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

fn resolve_crv(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc } => read_smc(smc),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn encode_srf(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend([n as f64, m as f64, s.p as f64, s.q as f64]);
    o.extend(s.net.iter().flatten().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn encode_crv(c: &CrvIn, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let c = resolve_crv(c)?;
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
    o.extend([n as f64, c.p as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}

fn build_srf(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
        .collect();
    Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("rust surface: {e}")))
}

fn build_crv(c: &CrvIn) -> Result<Curve, HarnessError> {
    let c = resolve_crv(c)?;
    let pw = c.cpts.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect();
    Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("rust curve: {e}")))
}

fn cp(a: [f64; 4]) -> CPoint {
    CPoint::new(a[0], a[1], a[2], a[3])
}

// ---- output layout ------------------------------------------------------------------

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
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m) = (dim(0)?, dim(2)?);
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    out = out.values(&format!("{pfx}net"), take(flat, at, 4 * (n + 1))?.to_vec());
    Ok(out.values(&format!("{pfx}u"), take(flat, at, m + 1)?.to_vec()))
}

fn split_counted(flat: &[f64], at: &mut usize, key: &str, out: Output) -> Result<Output, HarnessError> {
    let c = take(flat, at, 1)?.to_vec();
    let k = count(c.first().copied().unwrap_or(f64::NAN))?;
    let out = out.values(&format!("{key}_count"), c);
    Ok(out.values(key, take(flat, at, k)?.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        let crvs = |out: Output, at: &mut usize, k: usize| -> Result<Output, HarnessError> {
            let mut out = out;
            for c in 0..k {
                out = split_crv(flat, at, &format!("c{c}"), out)?;
            }
            Ok(out)
        };
        out = match i {
            Inputs::Bvec { t, .. } => out.values("b", take(flat, &mut at, 3 * t.len())?.to_vec()),
            Inputs::Coons { .. } => crvs(split_srf(flat, &mut at, out)?, &mut at, 4)?,
            Inputs::SkinDerivs { .. } => crvs(split_srf(flat, &mut at, out)?, &mut at, 1)?,
            Inputs::Merge { .. } => {
                let out = split_counted(flat, &mut at, "xr", out)?;
                split_counted(flat, &mut at, "xs", out)?
            }
            Inputs::Rot { .. } => out.values("rm", take(flat, &mut at, 9)?.to_vec()),
            Inputs::SweepTangent { .. } | Inputs::CrossDeriv { .. } => {
                split_crv(flat, &mut at, "", out)?
            }
            Inputs::AdjustDeriv { .. } => split_srf(flat, &mut at, out)?,
            Inputs::AdjustWeight { .. } => out.values("wr_alf", take(flat, &mut at, 2)?.to_vec()),
            Inputs::Approx { inplace, .. } => {
                let out = split_crv(flat, &mut at, "", out)?;
                if *inplace {
                    out
                } else {
                    split_crv(flat, &mut at, "c0", out)?
                }
            }
            Inputs::CoonsBoundary { .. } => crvs(split_srf(flat, &mut at, out)?, &mut at, 8)?,
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

// ---- C++ side -----------------------------------------------------------------------

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let op = match i {
        Inputs::Bvec { cur, b0, t } => {
            encode_crv(cur, &mut o)?;
            o.extend(b0);
            let k = t.len().checked_sub(1).ok_or_else(|| harness("no parameters"))?;
            o.push(k as f64);
            o.extend_from_slice(t);
            Op::BVectors
        }
        Inputs::Coons { curves } => {
            if curves.len() != 4 {
                return Err(harness("coons needs 4 curves"));
            }
            for c in curves {
                encode_crv(c, &mut o)?;
            }
            Op::Coons
        }
        Inputs::SkinDerivs { cur, v, bndy } => {
            encode_crv(cur, &mut o)?;
            o.extend(v);
            o.push(f64::from(*bndy));
            Op::SkinDerivs
        }
        Inputs::Merge { p, r, s } => {
            o.push(*p as f64);
            o.push(r.len() as f64);
            o.extend_from_slice(r);
            o.push(s.len() as f64);
            o.extend_from_slice(s);
            Op::MergeKnots
        }
        Inputs::Rot { traj, u, zsrc } => {
            encode_crv(traj, &mut o)?;
            o.push(*u);
            match zsrc {
                ZIn::Curve { zcur } => {
                    o.push(0.0);
                    encode_crv(zcur, &mut o)?;
                }
                ZIn::Samples { z, prev, next } => {
                    o.push(1.0);
                    o.extend(z);
                    for nb in [prev, next] {
                        let (pt, t) = nb.unwrap_or(([0.0; 3], 0.0));
                        o.push(flag(nb.is_some()));
                        o.extend(pt);
                        o.push(t);
                    }
                }
            }
            Op::RotationRate
        }
        Inputs::SweepTangent {
            dtraj,
            scale,
            dscale,
            rma,
            drma,
            cur,
        } => {
            o.extend(dtraj);
            o.extend(scale);
            o.extend(dscale);
            o.extend(rma.iter().flatten());
            o.extend(drma.iter().flatten());
            encode_crv(cur, &mut o)?;
            Op::SweepTangent
        }
        Inputs::AdjustDeriv {
            sur,
            dir,
            cst,
            usg,
            inplace,
        } => {
            encode_srf(sur, &mut o)?;
            o.extend([f64::from(*dir), f64::from(*cst), f64::from(*usg), flag(*inplace)]);
            Op::AdjustDeriv
        }
        Inputs::AdjustWeight { pw, qw, rw } => {
            o.extend(pw);
            o.extend(qw);
            o.extend(rw);
            Op::AdjustWeight
        }
        Inputs::Approx {
            cur,
            ds,
            de,
            eps,
            inplace,
        } => {
            encode_crv(cur, &mut o)?;
            o.extend(ds);
            o.extend(de);
            o.push(*eps);
            o.push(flag(*inplace));
            Op::Approx
        }
        Inputs::CoonsBoundary { curves, cpf } => {
            if curves.len() != 8 {
                return Err(harness("coons_boundary needs 8 curves"));
            }
            for c in curves {
                encode_crv(c, &mut o)?;
            }
            o.push(flag(*cpf));
            Op::CoonsBoundary
        }
        Inputs::CrossDeriv { cur, ns, ne, bs, be } => {
            encode_crv(cur, &mut o)?;
            for v in [ns, ne, bs, be] {
                o.extend(v);
            }
            Op::CrossDeriv
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

// ---- Rust side ----------------------------------------------------------------------

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `1000` for upstream's bare
/// error flag, `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(SolidError::InvalidInput { reason, .. }) if *reason == BARE_FLAG_REASON => 1000.0,
        Err(_) => -1.0,
    }
}

fn dump_srf(o: &mut Vec<f64>, s: &Surface) {
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
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend_from_slice(&pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn boundary(fl: i32) -> Result<Boundary, HarnessError> {
    Boundary::from_flag(fl).ok_or_else(|| harness(format!("bad boundary flag {fl}")))
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Bvec { cur, b0, t } => {
            let c = build_crv(cur)?;
            let r = adv::create_b_vectors(&c, v3(*b0), t);
            o.push(status(&r));
            if let Ok(b) = r {
                for v in b {
                    o.extend([v.x, v.y, v.z]);
                }
            }
        }
        Inputs::Coons { curves } => {
            let cs = curves.iter().map(build_crv).collect::<Result<Vec<_>, _>>()?;
            let [a, b, c, d]: [Curve; 4] = cs
                .try_into()
                .map_err(|_| harness("coons needs 4 curves"))?;
            let mut cu = [a, b];
            let mut cv = [c, d];
            let r = adv::create_coons_srf(&mut cu, &mut cv);
            o.push(status(&r));
            if let Ok(s) = r {
                dump_srf(&mut o, &s);
                for c in cu.iter().chain(cv.iter()) {
                    dump_crv(&mut o, c);
                }
            }
        }
        Inputs::SkinDerivs { cur, v, bndy } => {
            let mut c = build_crv(cur)?;
            let r = adv::create_skin_srf_boundary_derivs(&mut c, v3(*v), boundary(*bndy)?);
            o.push(status(&r));
            if let Ok(s) = r {
                dump_srf(&mut o, &s);
                dump_crv(&mut o, &c);
            }
        }
        Inputs::Merge { p, r, s } => {
            let kr = KnotVector::from_slice(r).map_err(|e| harness(format!("knots: {e}")))?;
            let ks = KnotVector::from_slice(s).map_err(|e| harness(format!("knots: {e}")))?;
            let res = adv::merge_knot_vectors(&kr, &ks, *p);
            o.push(status(&res));
            if let Ok((xr, xs)) = res {
                o.push(xr.len() as f64);
                o.extend(xr);
                o.push(xs.len() as f64);
                o.extend(xs);
            }
        }
        Inputs::Rot { traj, u, zsrc } => {
            let t = build_crv(traj)?;
            let zc;
            let src = match zsrc {
                ZIn::Curve { zcur } => {
                    zc = build_crv(zcur)?;
                    adv::ZSource::Curve(&zc)
                }
                ZIn::Samples { z, prev, next } => adv::ZSource::Samples {
                    z: v3(*z),
                    prev: prev.map(|(p, t)| (v3(p), t)),
                    next: next.map(|(p, t)| (v3(p), t)),
                },
            };
            let r = adv::rotation_rate_of_change(&t, *u, src);
            o.push(status(&r));
            if let Ok(m) = r {
                o.extend(m.iter().flatten());
            }
        }
        Inputs::SweepTangent {
            dtraj,
            scale,
            dscale,
            rma,
            drma,
            cur,
        } => {
            let c = build_crv(cur)?;
            let r = adv::sweep_cross_tangent_crv(v3(*dtraj), v3(*scale), v3(*dscale), rma, drma, &c);
            o.push(status(&r));
            if let Ok(q) = r {
                dump_crv(&mut o, &q);
            }
        }
        Inputs::AdjustDeriv {
            sur, dir, cst, usg, ..
        } => {
            let s = build_srf(sur)?;
            let d = Direction::from_flag(*dir).ok_or_else(|| harness("bad direction flag"))?;
            let r = adv::adjust_deriv_srf(&s, d, boundary(*cst)?, boundary(*usg)?);
            o.push(status(&r));
            if let Ok(q) = r {
                dump_srf(&mut o, &q);
            }
        }
        Inputs::AdjustWeight { pw, qw, rw } => {
            let r = adv::adjust_weight_scale(cp(*pw), cp(*qw), cp(*rw));
            o.push(status(&r));
            if let Ok((wr, alf)) = r {
                o.extend([wr, alf]);
            }
        }
        Inputs::Approx {
            cur,
            ds,
            de,
            eps,
            inplace,
        } => {
            let mut c = build_crv(cur)?;
            let r = adv::approx_cross_boundary_derivs(&mut c, v3(*ds), v3(*de), *eps);
            o.push(status(&r));
            if let Ok(q) = r {
                dump_crv(&mut o, &q);
                if !*inplace {
                    dump_crv(&mut o, &c);
                }
            }
        }
        Inputs::CoonsBoundary { curves, cpf } => {
            let cs = curves.iter().map(build_crv).collect::<Result<Vec<_>, _>>()?;
            let [left, left_deriv, right, right_deriv, bottom, bottom_deriv, top, top_deriv]: [Curve; 8] =
                cs.try_into().map_err(|_| harness("coons_boundary needs 8 curves"))?;
            let mut b = adv::CoonsBoundary {
                left,
                left_deriv,
                right,
                right_deriv,
                bottom,
                bottom_deriv,
                top,
                top_deriv,
            };
            let r = adv::create_coons_boundary_crvs(&mut b, *cpf);
            o.push(status(&r));
            if let Ok(s) = r {
                dump_srf(&mut o, &s);
                for c in [
                    &b.left,
                    &b.left_deriv,
                    &b.right,
                    &b.right_deriv,
                    &b.bottom,
                    &b.bottom_deriv,
                    &b.top,
                    &b.top_deriv,
                ] {
                    dump_crv(&mut o, c);
                }
            }
        }
        Inputs::CrossDeriv { cur, ns, ne, bs, be } => {
            let c = build_crv(cur)?;
            let r = adv::create_cross_boundary_deriv_crv(&c, v3(*ns), v3(*ne), v3(*bs), v3(*be));
            o.push(status(&r));
            if let Ok(q) = r {
                dump_crv(&mut o, &q);
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation ---------------------------------------------------------------------

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-1e3, 1e3),
        _ => rng.uniform(-10.0, 10.0),
    }
}

fn vec3(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn unit3(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.1 {
            return [v[0] / l, v[1] / l, v[2] / l];
        }
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
                mid.push(rng.uniform(a + (b - a) * 1e-3, b - (b - a) * 1e-3));
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

fn domain(rng: &mut Rng) -> (f64, f64) {
    match rng.below(4) {
        0 => (0.0, 1.0),
        1 => (rng.uniform(-5.0, 0.0), rng.uniform(0.5, 5.0)),
        2 => (rng.uniform(-100.0, 100.0), rng.uniform(150.0, 300.0)),
        _ => (0.0, rng.uniform(0.1, 10.0)),
    }
}

fn knots_on(rng: &mut Rng, cnt: usize, p: usize, (a, b): (f64, f64)) -> Vec<f64> {
    let k = knots(rng, cnt, p, a, b);
    if knots_valid(&k, p) {
        k
    } else {
        knots(rng, cnt, p, 0.0, 1.0)
    }
}

fn weight(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0..=2 => 1.0,
        3 => rng.uniform(0.05, 0.2),
        _ => rng.uniform(0.5, 2.0),
    }
}

fn degree(rng: &mut Rng) -> usize {
    if rng.chance(0.05) {
        5 + rng.below(3) as usize
    } else {
        1 + rng.below(4) as usize
    }
}

fn to_cpt(p: [f64; 3], rational: bool, rng: &mut Rng) -> [f64; 4] {
    if rational {
        let w = weight(rng);
        let z = if p[2] == NOZ { NOZ } else { p[2] * w };
        [p[0] * w, p[1] * w, z, w]
    } else {
        [p[0], p[1], p[2], NOW]
    }
}

/// A curve of degree `p` with `n + 1` points from `pt(i)` on `dom`.
fn crv_from(
    rng: &mut Rng,
    p: usize,
    n: usize,
    rational: bool,
    dom: (f64, f64),
    mut pt: impl FnMut(&mut Rng, usize) -> [f64; 3],
) -> InlineCrv {
    let cpts = (0..=n)
        .map(|i| {
            let x = pt(rng, i);
            to_cpt(x, rational, rng)
        })
        .collect();
    let u = knots_on(rng, n + p + 2, p, dom);
    InlineCrv { cpts, p, u }
}

fn gen_crv(rng: &mut Rng, rational: bool) -> InlineCrv {
    let p = degree(rng);
    let n = p + rng.below(5) as usize;
    let two_d = rng.chance(0.05);
    let dom = domain(rng);
    crv_from(rng, p, n, rational, dom, |rng, _| {
        let mut v = vec3(rng);
        if two_d {
            v[2] = NOZ;
        }
        v
    })
}

/// A smooth curve from `a` to `b` (interior points near the chord).
fn gen_crv_between(rng: &mut Rng, a: [f64; 3], b: [f64; 3], rational: bool, dom: (f64, f64)) -> InlineCrv {
    let p = degree(rng).min(4);
    let n = p + rng.below(4) as usize;
    let mut c = crv_from(rng, p, n, false, dom, |rng, i| {
        let t = i as f64 / n as f64;
        let mut x = [0.0; 3];
        for k in 0..3 {
            x[k] = a[k] + t * (b[k] - a[k]);
            if i > 0 && i < n {
                x[k] += rng.uniform(-0.5, 0.5);
            }
        }
        x
    });
    if rational {
        let last = c.cpts.len() - 1;
        for (i, pt) in c.cpts.iter_mut().enumerate() {
            // End weights stay 1 only sometimes, so the reparametrisation is exercised.
            let w = if (i == 0 || i == last) && rng.chance(0.5) { 1.0 } else { weight(rng) };
            *pt = [pt[0] * w, pt[1] * w, pt[2] * w, w];
        }
    }
    c
}

fn inline(c: InlineCrv) -> CrvIn {
    CrvIn::Inline(c)
}

fn gen_params(rng: &mut Rng, dom: (f64, f64)) -> Vec<f64> {
    let (a, b) = dom;
    let cnt = 1 + rng.below(8) as usize;
    let mut t: Vec<f64> = (0..cnt).map(|_| rng.uniform(a, b)).collect();
    t.sort_by(f64::total_cmp);
    if rng.chance(0.5) {
        t.insert(0, a);
    }
    if rng.chance(0.5) {
        t.push(b);
    }
    if rng.chance(0.03) {
        t.push(b + (b - a) * 0.1);
    }
    t
}

fn crv_dom(c: &InlineCrv) -> (f64, f64) {
    (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    )
}

fn gen_bvec(rng: &mut Rng) -> Inputs {
    let mut c = { let rat = rng.chance(0.3); gen_crv(rng, rat) };
    if rng.chance(0.3) {
        // Closed curve.
        if let Some(&first) = c.cpts.first() {
            if let Some(last) = c.cpts.last_mut() {
                *last = first;
            }
        }
    }
    let t = gen_params(rng, crv_dom(&c));
    let b0 = if rng.chance(0.03) { [0.0; 3] } else { vec3(rng) };
    Inputs::Bvec { cur: inline(c), b0, t }
}

fn gen_coons(rng: &mut Rng) -> Inputs {
    let p00 = vec3(rng);
    let p10 = vec3(rng);
    let p01 = vec3(rng);
    let p11 = vec3(rng);
    let du = domain(rng);
    let dv = if rng.chance(0.5) { du } else { domain(rng) };
    let rat = |rng: &mut Rng| rng.chance(0.2);
    let (r0, r1, r2, r3) = (rat(rng), rat(rng), rat(rng), rat(rng));
    let u0 = gen_crv_between(rng, p00, p10, r0, du);
    let u1 = gen_crv_between(rng, p01, p11, r1, du);
    let v0 = gen_crv_between(rng, p00, p01, r2, dv);
    let v1 = gen_crv_between(rng, p10, p11, r3, dv);
    Inputs::Coons {
        curves: [u0, u1, v0, v1].into_iter().map(inline).collect(),
    }
}

fn gen_merge(rng: &mut Rng) -> Inputs {
    let p = 1 + rng.below(4) as usize;
    let (a, b) = domain(rng);
    let shared: Vec<f64> = (0..rng.below(4)).map(|_| rng.uniform(a, b)).collect();
    let one = |rng: &mut Rng| -> Vec<f64> {
        let mut mid: Vec<f64> = Vec::new();
        for &x in &shared {
            if rng.chance(0.7) {
                let mult = 1 + rng.below(p as u64) as usize;
                let y = if rng.chance(0.2) { x + (b - a) * 1e-14 } else { x };
                mid.extend(std::iter::repeat_n(y, mult));
            }
        }
        for _ in 0..rng.below(4) {
            let x = rng.uniform(a, b);
            let mult = 1 + rng.below(p as u64) as usize;
            mid.extend(std::iter::repeat_n(x, mult));
        }
        mid.sort_by(f64::total_cmp);
        let mut k = vec![a; p + 1];
        k.extend(mid);
        k.extend(std::iter::repeat_n(b, p + 1));
        k
    };
    let r = one(rng);
    let mut s = one(rng);
    if rng.chance(0.05) {
        if let Some(x) = s.last_mut() {
            *x += 1.0;
        }
    }
    let p = if rng.chance(0.05) { p + 3 } else { p };
    Inputs::Merge { p, r, s }
}

fn gen_rot(rng: &mut Rng) -> Inputs {
    let traj = { let rat = rng.chance(0.3); gen_crv(rng, rat) };
    let (a, b) = crv_dom(&traj);
    let u = match rng.below(6) {
        0 => a,
        1 => b,
        _ => rng.uniform(a, b),
    };
    let zsrc = if rng.chance(0.5) {
        let mut z = { let rat = rng.chance(0.3); gen_crv(rng, rat) };
        // Same domain as the trajectory.
        let p = z.p;
        let n = z.cpts.len() - 1;
        z.u = knots_on(rng, n + p + 2, p, (a, b));
        ZIn::Curve { zcur: inline(z) }
    } else {
        let nb = |rng: &mut Rng| {
            if rng.chance(0.6) {
                Some((unit3(rng), u + rng.uniform(-1.0, 1.0)))
            } else {
                None
            }
        };
        let prev = nb(rng);
        let next = nb(rng);
        ZIn::Samples {
            z: unit3(rng),
            prev,
            next,
        }
    };
    Inputs::Rot {
        traj: inline(traj),
        u,
        zsrc,
    }
}

fn mat(rng: &mut Rng) -> [[f64; 3]; 3] {
    [unit3(rng), unit3(rng), unit3(rng)]
}

fn gen_sweep_tangent(rng: &mut Rng) -> Inputs {
    let mut cur = { let rat = rng.chance(0.3); gen_crv(rng, rat) };
    if rng.chance(0.3) {
        // Repeated interior knots (fewer segments than knots).
        let p = cur.p;
        let n = cur.cpts.len() - 1;
        if n > p && p >= 2 {
            let (a, b) = crv_dom(&cur);
            let x = a + 0.5 * (b - a);
            let mut k = vec![a; p + 1];
            let interior = n - p;
            for i in 0..interior {
                k.push(if i < 2 { x } else { a + (b - a) * (0.6 + 0.3 * i as f64 / interior as f64) });
            }
            k.extend(std::iter::repeat_n(b, p + 1));
            if knots_valid(&k, p) {
                cur.u = k;
            }
        }
    }
    Inputs::SweepTangent {
        dtraj: vec3(rng),
        scale: [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)],
        dscale: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
        rma: mat(rng),
        drma: mat(rng),
        cur: inline(cur),
    }
}

fn gen_adjust_deriv(rng: &mut Rng) -> Inputs {
    let dir = 1 + rng.below(2) as i32;
    let bez_deg = degree(rng).min(4);
    let other_deg = degree(rng).min(4);
    let other_n = other_deg + rng.below(4) as usize;
    let bezier = !rng.chance(0.1);
    let (p, n, q, m) = if dir == 1 {
        let n = if bezier { bez_deg } else { bez_deg + 1 };
        (bez_deg, n, other_deg, other_n)
    } else {
        let m = if bezier { bez_deg } else { bez_deg + 1 };
        (other_deg, other_n, bez_deg, m)
    };
    let rational = rng.chance(0.4);
    let net = (0..=n)
        .map(|_| (0..=m).map(|_| to_cpt(vec3(rng), rational, rng)).collect())
        .collect();
    let u = { let d = domain(rng); knots_on(rng, n + p + 2, p, d) };
    let v = { let d = domain(rng); knots_on(rng, m + q + 2, q, d) };
    Inputs::AdjustDeriv {
        sur: InlineSrf { net, p, q, u, v },
        dir,
        cst: 1 + rng.below(4) as i32,
        usg: 1 + rng.below(4) as i32,
        inplace: rng.chance(0.4),
    }
}

fn gen_adjust_weight(rng: &mut Rng) -> Inputs {
    let rational = !rng.chance(0.05);
    let pt = |rng: &mut Rng| to_cpt(vec3(rng), rational, rng);
    let pw = pt(rng);
    let mut qw = pt(rng);
    let rw = pt(rng);
    if rational && rng.chance(0.3) {
        // Equal weights.
        let w = pw[3];
        qw = [qw[0] / qw[3] * w, qw[1] / qw[3] * w, qw[2] / qw[3] * w, w];
    }
    Inputs::AdjustWeight { pw, qw, rw }
}

fn gen_approx(rng: &mut Rng) -> Inputs {
    let rational = rng.chance(0.05);
    let mut c = gen_crv(rng, rational);
    if rng.chance(0.3) {
        // A single Bezier span.
        let p = c.p;
        c.cpts.truncate(p + 1);
        let (a, b) = crv_dom(&c);
        let mut k = vec![a; p + 1];
        k.extend(std::iter::repeat_n(b, p + 1));
        c.u = k;
    }
    // A zero second (or next-to-last) control point makes N_VectorsAngle fail, which
    // upstream ignores (it then reads an uninitialised angle).
    let last = c.cpts.len() - 1;
    for i in [1, last - 1] {
        if let Some(q) = c.cpts.get_mut(i) {
            if q[0] == 0.0 && q[1] == 0.0 && (q[2] == 0.0 || q[2] == NOZ) {
                q[0] = 1.0;
            }
        }
    }
    // Far from the origin with nearly exact end derivatives and a tiny tolerance:
    // large new end spans, which on a single span trigger upstream's mid knot.
    let tiny = rng.chance(0.25);
    if tiny {
        for q in &mut c.cpts {
            let w = if q[3] == NOW { 1.0 } else { q[3] };
            q[0] += 1000.0 * w;
        }
    }
    // End derivatives near the current ones (scaled control point differences),
    // rotated by random amounts so that some exceed the tolerance.
    let pts: Vec<[f64; 3]> = c
        .cpts
        .iter()
        .map(|q| {
            let w = if q[3] == NOW { 1.0 } else { q[3] };
            let z = if q[2] == NOZ { 0.0 } else { q[2] / w };
            [q[0] / w, q[1] / w, z]
        })
        .collect();
    let n = pts.len() - 1;
    let p = c.p as f64;
    let (a, b) = crv_dom(&c);
    let span = |i: usize| c.u.get(i).copied().unwrap_or(b);
    let dsf = p / (span(c.p + 1) - a);
    let def = p / (b - span(n));
    let jitter = if tiny {
        10f64.powf(rng.uniform(-12.0, -2.0))
    } else {
        rng.uniform(0.0, 1.0) * if rng.chance(0.3) { 0.0 } else { 1.0 }
    };
    let d = |i: usize, j: usize, f: f64, rng: &mut Rng| -> [f64; 3] {
        let (x, y) = (pts[i], pts[j]);
        let mut v = [0.0; 3];
        for k in 0..3 {
            v[k] = f * (y[k] - x[k]) + jitter * rng.uniform(-1.0, 1.0);
        }
        v
    };
    let ds = d(0, 1, dsf, rng);
    let de = d(n - 1, n, def, rng);
    // Never exactly 0: a zero end angle would equal it, and upstream then refines
    // with an uninitialised knot.
    let eps = if tiny {
        // Just below the smaller end angle (upstream's formula: the angle between the
        // second control point and P0 + (span/p) * Ds, as position vectors).
        let angle = |a: [f64; 3], b: [f64; 3]| {
            let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let ma = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            let mb = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
            (dot / (ma * mb)).clamp(-1.0, 1.0).acos().to_degrees()
        };
        let along = |o: [f64; 3], f: f64, v: [f64; 3]| [o[0] + f * v[0], o[1] + f * v[1], o[2] + f * v[2]];
        let als = angle(pts[1], along(pts[0], 1.0 / dsf, ds));
        let ale = angle(pts[n - 1], along(pts[n], -1.0 / def, de));
        // Never 0 (an exactly zero angle would equal it).
        (als.min(ale) / rng.uniform(1.05, 2.0)).max(1e-12)
    } else {
        *rng.choose(&[0.1, 1.0, 5.0, 30.0, 1e-9]).unwrap_or(&1.0)
    };
    Inputs::Approx {
        cur: inline(c),
        ds,
        de,
        eps,
        inplace: rng.chance(0.3),
    }
}

/// Boundary curves and cross derivatives of a random non-rational surface (so the
/// Coons compatibility conditions hold), or unrelated curves on shared domains.
fn gen_coons_boundary(rng: &mut Rng) -> Inputs {
    let du = domain(rng);
    let dv = domain(rng);
    let cpf = rng.chance(0.6);
    if rng.chance(0.25) {
        // Unrelated curves on the right domains (B-spline compatibility exercised).
        let c = |rng: &mut Rng, dom: (f64, f64)| {
            let mut k = gen_crv(rng, false);
            let p = k.p;
            let n = k.cpts.len() - 1;
            k.u = knots_on(rng, n + p + 2, p, dom);
            for pt in &mut k.cpts {
                if pt[2] == NOZ {
                    pt[2] = 0.0;
                }
            }
            inline(k)
        };
        let curves = vec![
            c(rng, dv),
            c(rng, dv),
            c(rng, dv),
            c(rng, dv),
            c(rng, du),
            c(rng, du),
            c(rng, du),
            c(rng, du),
        ];
        return Inputs::CoonsBoundary { curves, cpf };
    }
    let p = 1 + rng.below(4) as usize;
    let q = 1 + rng.below(4) as usize;
    let n = p + rng.below(4) as usize;
    let m = q + rng.below(4) as usize;
    let net: Vec<Vec<[f64; 3]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 + rng.uniform(-0.3, 0.3),
                        j as f64 + rng.uniform(-0.3, 0.3),
                        rng.uniform(-1.0, 1.0),
                    ]
                })
                .collect()
        })
        .collect();
    let u = knots_on(rng, n + p + 2, p, du);
    let v = knots_on(rng, m + q + 2, q, dv);
    let row = |i: usize| -> Vec<[f64; 4]> { net[i].iter().map(|x| [x[0], x[1], x[2], NOW]).collect() };
    let col = |j: usize| -> Vec<[f64; 4]> { net.iter().map(|r| [r[j][0], r[j][1], r[j][2], NOW]).collect() };
    let diff = |a: [f64; 3], b: [f64; 3], f: f64| [f * (b[0] - a[0]), f * (b[1] - a[1]), f * (b[2] - a[2]), NOW];
    let fu0 = p as f64 / (u[p + 1] - u[1]);
    let fu1 = p as f64 / (u[n + p] - u[n]);
    let fv0 = q as f64 / (v[q + 1] - v[1]);
    let fv1 = q as f64 / (v[m + q] - v[m]);
    let der_l: Vec<[f64; 4]> = (0..=m).map(|j| diff(net[0][j], net[1][j], fu0)).collect();
    let der_r: Vec<[f64; 4]> = (0..=m).map(|j| diff(net[n - 1][j], net[n][j], fu1)).collect();
    let der_b: Vec<[f64; 4]> = (0..=n).map(|i| diff(net[i][0], net[i][1], fv0)).collect();
    let der_t: Vec<[f64; 4]> = (0..=n).map(|i| diff(net[i][m - 1], net[i][m], fv1)).collect();
    let cv = |cpts: Vec<[f64; 4]>| inline(InlineCrv { cpts, p: q, u: v.clone() });
    let cu = |cpts: Vec<[f64; 4]>| inline(InlineCrv { cpts, p, u: u.clone() });
    let mut curves = vec![
        cv(row(0)),
        cv(der_l),
        cv(row(n)),
        cv(der_r),
        cu(col(0)),
        cu(der_b),
        cu(col(m)),
        cu(der_t),
    ];
    if rng.chance(0.15) {
        // Break a derivative (fails the checks with cpf).
        let k = *rng.choose(&[1usize, 3, 5, 7]).unwrap_or(&1);
        if let Some(CrvIn::Inline(c)) = curves.get_mut(k) {
            if let Some(pt) = c.cpts.first_mut() {
                pt[0] += 0.5;
            }
        }
    }
    if rng.chance(0.05) {
        // A rational curve or a mismatched domain.
        if let Some(CrvIn::Inline(c)) = curves.get_mut(rng.below(8) as usize) {
            if rng.chance(0.5) {
                if let Some(pt) = c.cpts.first_mut() {
                    pt[3] = 1.0;
                }
                for pt in c.cpts.iter_mut().skip(1) {
                    pt[3] = 1.0;
                }
            } else {
                // Shift the end knots (domain end + 1).
                let deg = c.p;
                for y in c.u.iter_mut().rev().take(deg + 1) {
                    *y += 1.0;
                }
            }
        }
    }
    Inputs::CoonsBoundary { curves, cpf }
}

fn gen_cross_deriv(rng: &mut Rng) -> Inputs {
    let c = { let rat = rng.chance(0.3); gen_crv(rng, rat) };
    Inputs::CrossDeriv {
        cur: inline(c),
        ns: unit3(rng),
        ne: unit3(rng),
        bs: vec3(rng),
        be: vec3(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(11) {
        0 => gen_bvec(rng),
        1 => gen_coons(rng),
        2 => {
            let v = if rng.chance(0.03) { [0.0; 3] } else { vec3(rng) };
            Inputs::SkinDerivs {
                cur: inline({ let rat = rng.chance(0.3); gen_crv(rng, rat) }),
                v,
                bndy: 1 + rng.below(4) as i32,
            }
        }
        3 => gen_merge(rng),
        4 => gen_rot(rng),
        5 => gen_sweep_tangent(rng),
        6 => gen_adjust_deriv(rng),
        7 => gen_adjust_weight(rng),
        8 => gen_approx(rng),
        9 => gen_coons_boundary(rng),
        _ => gen_cross_deriv(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_adv_p4"
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

    /// The generator reaches successes and failures of every op.
    #[test]
    fn generated_cases_cover_outcomes() {
        // [ok, err] per op, in gen_case order.
        let mut stats = [[0usize; 2]; 11];
        // [approx +1 knot, +2 knots, mid knot + 2, cpf coons ok, rational coons ok,
        //  adjusted strip, merge both ways, rot curve, rot samples, rational cross]
        let mut branches = [0usize; 10];
        for seed in 0..3000u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            let ok = flat.first() == Some(&0.0);
            let slot = match &i {
                Inputs::Bvec { .. } => 0,
                Inputs::Coons { .. } => 1,
                Inputs::SkinDerivs { .. } => 2,
                Inputs::Merge { .. } => 3,
                Inputs::Rot { .. } => 4,
                Inputs::SweepTangent { .. } => 5,
                Inputs::AdjustDeriv { .. } => 6,
                Inputs::AdjustWeight { .. } => 7,
                Inputs::Approx { .. } => 8,
                Inputs::CoonsBoundary { .. } => 9,
                Inputs::CrossDeriv { .. } => 10,
            };
            stats[slot][usize::from(!ok)] += 1;
            if !ok {
                continue;
            }
            match &i {
                Inputs::Approx { cur, .. } => {
                    let c = resolve_crv(cur).unwrap();
                    let n_out = flat[1] as usize;
                    let n_in = c.cpts.len() - 1;
                    if n_out == n_in + 1 {
                        branches[0] += 1;
                    } else if n_out == n_in + 2 {
                        branches[1] += 1;
                    }
                    // Mid knot of a single span (upstream's repeat pass).
                    let (a, b) = crv_dom(&c);
                    let (p, m_out) = (flat[2] as usize, flat[3] as usize);
                    let knots = &flat[4 + 4 * (n_out + 1)..][..=m_out];
                    if n_in == c.p && p == c.p && knots.contains(&(a + 0.5 * (b - a))) {
                        branches[2] += 1;
                    }
                }
                Inputs::CoonsBoundary { cpf: true, .. } => branches[3] += 1,
                Inputs::Coons { curves } => {
                    if curves.iter().any(|c| resolve_crv(c).unwrap().cpts[0][3] != NOW) {
                        branches[4] += 1;
                    }
                }
                Inputs::AdjustDeriv { cst, usg, .. } if cst != usg => branches[5] += 1,
                Inputs::Merge { .. } => {
                    let nxr = flat[1] as usize;
                    let nxs = flat[2 + nxr] as usize;
                    if nxr > 0 && nxs > 0 {
                        branches[6] += 1;
                    }
                }
                Inputs::Rot { zsrc: ZIn::Curve { .. }, .. } => branches[7] += 1,
                Inputs::Rot { zsrc: ZIn::Samples { .. }, .. } => branches[8] += 1,
                Inputs::CrossDeriv { cur, .. } if resolve_crv(cur).unwrap().cpts[0][3] != NOW => {
                    branches[9] += 1;
                }
                _ => {}
            }
        }
        eprintln!("{stats:?} {branches:?}");
        assert!(stats.iter().all(|s| s[0] >= 20), "{stats:?}");
        assert!(branches.iter().all(|&k| k >= 5), "{branches:?}");
    }
}
