//! Runner for unit `nlib.srf_adv_p1`: NLib surface extension by a distance, offset
//! second-derivative bounds, unbounded evaluation, skinning along a spine and
//! cross-boundary derivative strips from a vector field.
//!
//! C++: `solidcore_ffi::units::nlib_srf_adv_p1` (upstream
//! `source/SMLib/src/NL_SrfAdv.cpp`). Rust: `solidcore::nurbs::srf_adv::p1`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number, `1000` for upstream's bare error flag; `-1` on the Rust side for any other
//! error, which never matches), then on success the results: the surface
//! (`dims = [n, m, p, q, r, s]`, `net` x y z w row-major, `u`, `v`) for
//! `extend_by_dist`, `skin_spine` (followed by every profile after the call,
//! `c<i>dims = [n, p, m]`, `c<i>net`, `c<i>u`, since upstream modifies them in place)
//! and `vector_field`; `max = [Muu, Muv, Mvv]` for `max_deriv`. `unbounded` runs a
//! sequence of evaluations sharing the extension (`esur` / `gflg` upstream): per
//! evaluation `e<i>status` and `e<i>d` (the derivatives `D[i][j]`, `i + j <= ndr`,
//! xyz) up to the first failure, then, when all succeeded, `has_ext` and, with an
//! extension, `gflg` and the extended surface (`x` prefixed keys).
//!
//! Surfaces are inline or upstream `.sms` files (`{"sms": {"$upstream_file": ...}}`),
//! curves inline or `.smc` files (`{"smc": ...}`). Inputs must be valid clamped
//! surfaces and curves (the FFI wrapper rejects anything else). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_adv::p1::{CurveEnd, BARE_FLAG_REASON};
use solidcore::nurbs::crv_basic::{Boundary, Curve};
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_adv::p1 as adv;
use solidcore::nurbs::srf_adv::p2::SrfContinuity;
use solidcore::nurbs::srf_adv::p3::ExtendContinuity;
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_adv_p1::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2500;

/// `nlib.srf_adv_p1` runner.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    ExtendByDist {
        sur: SrfIn,
        dist: f64,
        dir: i32,
        end: i32,
        cont: i32,
        #[serde(default)]
        inplace: bool,
    },
    MaxDeriv {
        sur: SrfIn,
        d: f64,
    },
    Unbounded {
        sur: SrfIn,
        cont: i32,
        ndr: usize,
        uv: Vec<[f64; 2]>,
    },
    SkinSpine {
        curves: Vec<CrvIn>,
        spine: CrvIn,
        params: Vec<f64>,
        cmp: bool,
        align: bool,
        z0: [f64; 3],
        subdiv: usize,
        derivs: i32,
        deg: usize,
        dir: i32,
    },
    VectorField {
        cur: CrvIn,
        vfield: CrvIn,
        bndy: i32,
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

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let all = numbers(path)?;
    let mut it = all.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
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

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
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

fn encode(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend([n as f64, m as f64, s.p as f64, s.q as f64]);
    o.extend(s.net.iter().flatten().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
    o.extend([n as f64, c.p as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
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

fn build_crv(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let pw = c.cpts.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect();
    Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("rust curve: {e}")))
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

fn split_srf(flat: &[f64], at: &mut usize, out: Output) -> Result<Output, HarnessError> {
    split_srf_pfx(flat, at, "", out)
}

fn split_srf_pfx(
    flat: &[f64],
    at: &mut usize,
    pfx: &str,
    mut out: Output,
) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 6)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    out = out.values(&format!("{pfx}net"), take(flat, at, 4 * (n + 1) * (m + 1))?.to_vec());
    out = out.values(&format!("{pfx}u"), take(flat, at, r + 1)?.to_vec());
    Ok(out.values(&format!("{pfx}v"), take(flat, at, s + 1)?.to_vec()))
}

fn split_crv(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m) = (dim(0)?, dim(2)?);
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    out = out.values(&format!("{pfx}net"), take(flat, at, 4 * (n + 1))?.to_vec());
    Ok(out.values(&format!("{pfx}u"), take(flat, at, m + 1)?.to_vec()))
}

fn deriv_count(ndr: usize) -> usize {
    if ndr == 0 {
        1
    } else {
        (ndr + 1) * (ndr + 2) / 2
    }
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    if let Inputs::Unbounded { uv, ndr, .. } = i {
        let mut all_ok = true;
        for e in 0..uv.len() {
            let st = take(flat, &mut at, 1)?.to_vec();
            let ok = st.first() == Some(&0.0);
            out = out.values(&format!("e{e}status"), st);
            if !ok {
                all_ok = false;
                break;
            }
            out = out.values(
                &format!("e{e}d"),
                take(flat, &mut at, 3 * deriv_count(*ndr))?.to_vec(),
            );
        }
        if all_ok {
            let has = take(flat, &mut at, 1)?.to_vec();
            let some = has.first() == Some(&1.0);
            out = out.values("has_ext", has);
            if some {
                out = out.values("gflg", take(flat, &mut at, 1)?.to_vec());
                out = split_srf_pfx(flat, &mut at, "x", out)?;
            }
        }
    } else {
        let status = take(flat, &mut at, 1)?.to_vec();
        let ok = status.first() == Some(&0.0);
        out = out.values("status", status);
        if ok {
            match i {
                Inputs::MaxDeriv { .. } => {
                    out = out.values("max", take(flat, &mut at, 3)?.to_vec());
                }
                Inputs::SkinSpine { curves, .. } => {
                    out = split_srf(flat, &mut at, out)?;
                    for c in 0..curves.len() {
                        out = split_crv(flat, &mut at, &format!("c{c}"), out)?;
                    }
                }
                _ => out = split_srf(flat, &mut at, out)?,
            }
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

// ---- C++ side -----------------------------------------------------------------------

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = match i {
        Inputs::ExtendByDist {
            sur,
            dist,
            dir,
            end,
            cont,
            inplace,
        } => {
            encode(&resolve(sur)?, &mut o)?;
            o.extend([
                *dist,
                f64::from(*dir),
                f64::from(*end),
                f64::from(*cont),
                flag(*inplace),
            ]);
            Op::ExtendByDist
        }
        Inputs::MaxDeriv { sur, d } => {
            encode(&resolve(sur)?, &mut o)?;
            o.push(*d);
            Op::MaxDeriv
        }
        Inputs::Unbounded { sur, cont, ndr, uv } => {
            encode(&resolve(sur)?, &mut o)?;
            o.extend([f64::from(*cont), *ndr as f64, uv.len() as f64]);
            o.extend(uv.iter().flatten());
            Op::Unbounded
        }
        Inputs::SkinSpine {
            curves,
            spine,
            params,
            cmp,
            align,
            z0,
            subdiv,
            derivs,
            deg,
            dir,
        } => {
            let k = curves.len().checked_sub(1).ok_or_else(|| harness("no curves"))?;
            if params.len() != k + 1 {
                return Err(harness("one spine parameter per profile expected"));
            }
            o.push(k as f64);
            for c in curves {
                encode_crv(&resolve_crv(c)?, &mut o)?;
            }
            encode_crv(&resolve_crv(spine)?, &mut o)?;
            o.extend_from_slice(params);
            o.extend([flag(*cmp), flag(*align)]);
            o.extend_from_slice(z0);
            o.extend([
                *subdiv as f64,
                f64::from(*derivs),
                *deg as f64,
                f64::from(*dir),
            ]);
            Op::SkinSpine
        }
        Inputs::VectorField { cur, vfield, bndy } => {
            encode_crv(&resolve_crv(cur)?, &mut o)?;
            encode_crv(&resolve_crv(vfield)?, &mut o)?;
            o.push(f64::from(*bndy));
            Op::VectorField
        }
    };
    Ok((op, o))
}

/// Status of the upstream bare error flag (no `NL_ERROR` number of its own).
const BARE_FLAG: f64 = 1000.0;

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(mut flat) => {
            // Past its recursion limit `ST_surxsdr` returns a bare error flag and
            // `NL_ERROR` still holds whatever an earlier, recovered error left there
            // (plan/findings.md, nlib.srf_adv_p2). When the port reports the recursion
            // limit, an upstream failure is compared as the bare flag.
            if matches!(i, Inputs::ExtendByDist { .. })
                && flat.first().is_some_and(|&st| st != 0.0)
                && rust_flat(i)?.first() == Some(&BARE_FLAG)
            {
                if let Some(st) = flat.first_mut() {
                    *st = BARE_FLAG;
                }
            }
            to_output(i, &flat)
        }
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
        Err(SolidError::InvalidInput { reason, .. }) if *reason == BARE_FLAG_REASON => BARE_FLAG,
        Err(SolidError::NotConverged { operation, .. })
            if operation.ends_with("srf_adv::p2::extend_by_dist_at_depth") =>
        {
            BARE_FLAG
        }
        Err(_) => -1.0,
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::ExtendByDist {
            sur,
            dist,
            dir,
            end,
            cont,
            ..
        } => {
            let s = build(&resolve(sur)?)?;
            let end = CurveEnd::from_flag(*end).ok_or_else(|| harness("bad end flag"))?;
            let cont = SrfContinuity::from_flag(*cont).ok_or_else(|| harness("bad cont flag"))?;
            let r = adv::extend_by_dist(&s, *dist, direction(*dir)?, end, cont);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::MaxDeriv { sur, d } => {
            let s = build(&resolve(sur)?)?;
            let r = adv::offset_get_max_deriv(&s, *d);
            o.push(status(&r));
            if let Ok(m) = r {
                o.extend([m.muu, m.muv, m.mvv]);
            }
        }
        Inputs::Unbounded { sur, cont, ndr, uv } => {
            let s = build(&resolve(sur)?)?;
            let cont =
                ExtendContinuity::from_flag(*cont).ok_or_else(|| harness("bad cont flag"))?;
            let mut ext = None;
            let mut all_ok = true;
            for &[u, v] in uv {
                let r = adv::eval_pt_derivs_unbounded(&s, u, v, *ndr, cont, &mut ext);
                o.push(status(&r));
                let Ok(d) = r else {
                    all_ok = false;
                    break;
                };
                for (a, row) in d.iter().enumerate() {
                    for (b, p) in row.iter().enumerate() {
                        if a + b <= *ndr {
                            o.extend([p.x, p.y, p.z]);
                        }
                    }
                }
            }
            if all_ok {
                match &ext {
                    Some(e) => {
                        o.push(1.0);
                        o.push(e.closed.map_or(0.0, |d| f64::from(d.flag())));
                        dump(&mut o, &e.surface);
                    }
                    None => o.push(0.0),
                }
            }
        }
        Inputs::SkinSpine {
            curves,
            spine,
            params,
            cmp,
            align,
            z0,
            subdiv,
            derivs,
            deg,
            dir,
        } => {
            let mut cs = curves
                .iter()
                .map(|c| build_crv(&resolve_crv(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            let sp = build_crv(&resolve_crv(spine)?)?;
            let opts = adv::SkinSpineOptions {
                make_compatible: *cmp,
                align: align.then_some(adv::SpineAlignment {
                    z0: Vec3::from_array(*z0),
                    subdiv: *subdiv,
                }),
                derivs: adv::SkinDerivs::from_flag(*derivs)
                    .ok_or_else(|| harness("bad derivative flag"))?,
                degree: *deg,
                dir: direction(*dir)?,
            };
            let r = adv::create_skin_spine(&mut cs, &sp, params, &opts);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
                for c in &cs {
                    dump_crv(&mut o, c);
                }
            }
        }
        Inputs::VectorField { cur, vfield, bndy } => {
            let c = build_crv(&resolve_crv(cur)?)?;
            let f = build_crv(&resolve_crv(vfield)?)?;
            let b = Boundary::from_flag(*bndy).ok_or_else(|| harness("bad boundary flag"))?;
            let r = adv::cross_boundary_derivs_vector_field(&c, &f, b);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation ---------------------------------------------------------------------

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
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend_from_slice(&pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn direction(dir: i32) -> Result<Direction, HarnessError> {
    Direction::from_flag(dir).ok_or_else(|| harness(format!("bad direction flag {dir}")))
}

fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-1e3, 1e3),
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

fn valid_knots(rng: &mut Rng, cnt: usize, p: usize) -> Vec<f64> {
    let (a, b) = domain(rng);
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

fn domain(rng: &mut Rng) -> (f64, f64) {
    match rng.below(4) {
        0 => (0.0, 1.0),
        1 => (rng.uniform(-5.0, 0.0), rng.uniform(0.5, 5.0)),
        2 => (rng.uniform(-100.0, 100.0), rng.uniform(150.0, 300.0)),
        _ => (0.0, rng.uniform(0.1, 10.0)),
    }
}

fn cpoint(rng: &mut Rng, rational: bool, two_d: bool) -> [f64; 4] {
    let (x, y) = (coord(rng), coord(rng));
    let z = if two_d { NOZ } else { coord(rng) };
    if rational {
        let w = weight(rng);
        let zw = if z == NOZ { NOZ } else { z * w };
        [x * w, y * w, zw, w]
    } else {
        [x, y, z, NOW]
    }
}

fn degree(rng: &mut Rng) -> usize {
    if rng.chance(0.05) {
        5 + rng.below(3) as usize
    } else {
        1 + rng.below(4) as usize
    }
}

/// A random valid clamped surface; sometimes smooth (a perturbed grid) and sometimes
/// with a collapsed boundary row (a pole).
fn gen_srf(rng: &mut Rng) -> InlineSrf {
    let p = degree(rng);
    let q = degree(rng);
    let n = p + rng.below(6) as usize;
    let m = q + rng.below(6) as usize;
    let rational = rng.chance(0.4);
    let two_d = rng.chance(0.03);
    let smooth = rng.chance(0.5);
    let mut net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    if smooth {
                        let w = if rational { weight(rng) } else { NOW };
                        let (x, y) = (i as f64 + rng.uniform(-0.3, 0.3), j as f64 + rng.uniform(-0.3, 0.3));
                        let z = rng.uniform(-1.0, 1.0);
                        if rational {
                            [x * w, y * w, z * w, w]
                        } else {
                            [x, y, z, NOW]
                        }
                    } else {
                        cpoint(rng, rational, two_d)
                    }
                })
                .collect()
        })
        .collect();
    if rng.chance(0.1) {
        // Collapse a boundary row or column to one point (a pole).
        let apex = net.first().and_then(|r| r.first()).copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
        match rng.below(4) {
            0 => {
                if let Some(row) = net.first_mut() {
                    for c in row.iter_mut() {
                        *c = apex;
                    }
                }
            }
            1 => {
                if let Some(row) = net.last_mut() {
                    for c in row.iter_mut() {
                        *c = apex;
                    }
                }
            }
            2 => net.iter_mut().for_each(|r| {
                if let Some(c) = r.first_mut() {
                    *c = apex;
                }
            }),
            _ => net.iter_mut().for_each(|r| {
                if let Some(c) = r.last_mut() {
                    *c = apex;
                }
            }),
        }
    }
    let u = valid_knots(rng, n + p + 2, p);
    let v = valid_knots(rng, m + q + 2, q);
    InlineSrf { net, p, q, u, v }
}

fn gen_crv_shape(rng: &mut Rng, p: usize, n: usize, rational: bool, two_d: bool, u: Vec<f64>) -> InlineCrv {
    let cpts = (0..=n).map(|_| cpoint(rng, rational, two_d)).collect();
    InlineCrv { cpts, p, u }
}

fn gen_crv(rng: &mut Rng) -> InlineCrv {
    let p = degree(rng);
    let n = p + rng.below(5) as usize;
    let rational = rng.chance(0.4);
    let two_d = rng.chance(0.05);
    let u = valid_knots(rng, n + p + 2, p);
    gen_crv_shape(rng, p, n, rational, two_d, u)
}


/// A rational quadratic cylinder (closed, G1 in the circle direction), the circle in
/// u or (swapped) in v.
fn gen_cylinder(rng: &mut Rng) -> InlineSrf {
    let r = rng.uniform(0.5, 5.0);
    let h = rng.uniform(0.5, 10.0);
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let circ: [[f64; 3]; 9] = [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, w],
        [0.0, 1.0, 1.0],
        [-1.0, 1.0, w],
        [-1.0, 0.0, 1.0],
        [-1.0, -1.0, w],
        [0.0, -1.0, 1.0],
        [1.0, -1.0, w],
        [1.0, 0.0, 1.0],
    ];
    let q = 1 + rng.below(2) as usize;
    let rows: Vec<Vec<[f64; 4]>> = circ
        .iter()
        .map(|&[x, y, wt]| {
            (0..=q)
                .map(|j| {
                    let z = h * j as f64 / q as f64;
                    [r * x * wt, r * y * wt, z * wt, wt]
                })
                .collect()
        })
        .collect();
    let (a, b) = if rng.chance(0.5) { (0.0, 1.0) } else { (0.0, 2.0 * std::f64::consts::PI) };
    let u = [a, a, a, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0]
        .iter()
        .enumerate()
        .map(|(i, &t)| if i < 3 { a } else if i > 8 { b } else { a + (b - a) * t })
        .collect();
    let v = knots(rng, q + 1 + q + 1, q, 0.0, 1.0);
    let s = InlineSrf {
        net: rows,
        p: 2,
        q,
        u,
        v,
    };
    if rng.chance(0.5) {
        // Circle in v.
        let n = s.net.len();
        let m = s.net.first().map_or(0, Vec::len);
        let net = (0..m)
            .map(|j| s.net.iter().take(n).filter_map(|row| row.get(j).copied()).collect())
            .collect();
        InlineSrf {
            net,
            p: s.q,
            q: s.p,
            u: s.v,
            v: s.u,
        }
    } else {
        s
    }
}

fn cont_flag(rng: &mut Rng) -> i32 {
    *rng.choose(&[2, 3, 4, 6]).unwrap_or(&2)
}

fn gen_extend(rng: &mut Rng) -> Inputs {
    let s = gen_srf(rng);
    let dist = match rng.below(10) {
        0 => 1e-6,
        1 => rng.uniform(3.0, 20.0),
        _ => rng.uniform(0.01, 3.0),
    };
    let dir = 1 + rng.below(2) as i32;
    let mut cont = cont_flag(rng);
    // A degree-1 extension direction makes upstream's reflection-plane fallback read
    // past its scratch row (plan/findings.md, nlib.srf_adv_p2); the port reports
    // `InvalidInput` there. Keep degree-1 extensions to C-max.
    let ext_deg = if dir == 1 { s.p } else { s.q };
    if ext_deg == 1 && cont != 4 {
        cont = 4;
    }
    Inputs::ExtendByDist {
        sur: SrfIn::Inline(s),
        dist,
        dir,
        end: 1 + rng.below(2) as i32,
        cont,
        inplace: rng.chance(0.3),
    }
}

fn unbounded_param(rng: &mut Rng, k: &[f64]) -> f64 {
    let a = k.first().copied().unwrap_or(0.0);
    let b = k.last().copied().unwrap_or(1.0);
    let len = b - a;
    match rng.below(20) {
        0..=6 => rng.uniform(a, b),
        7 => a,
        8 => b,
        9..=14 => {
            let f = rng.uniform(0.001, 1.5) * len;
            if rng.chance(0.5) {
                a - f
            } else {
                b + f
            }
        }
        15..=17 => {
            let f = rng.uniform(1.5, 6.0) * len;
            if rng.chance(0.5) {
                a - f
            } else {
                b + f
            }
        }
        18 => *rng.choose(k).unwrap_or(&a),
        _ => match rng.below(6) {
            0 => -2e12,
            1 => 3e12,
            _ => rng.uniform(a, b),
        },
    }
}

fn gen_unbounded(rng: &mut Rng) -> Inputs {
    let s = if rng.chance(0.3) { gen_cylinder(rng) } else { gen_srf(rng) };
    let cnt = 1 + rng.below(5) as usize;
    let uv = (0..cnt)
        .map(|_| {
            let u = unbounded_param(rng, &s.u);
            let v = unbounded_param(rng, &s.v);
            [u, v]
        })
        .collect();
    Inputs::Unbounded {
        sur: SrfIn::Inline(s),
        cont: cont_flag(rng),
        ndr: rng.below(4) as usize,
        uv,
    }
}

/// A spine curve along z (a line, or a smooth wiggly curve), with its domain.
fn gen_spine(rng: &mut Rng) -> InlineCrv {
    let len = rng.uniform(2.0, 20.0);
    if rng.chance(0.3) {
        let (a, b) = domain(rng);
        let k = knots(rng, 4, 1, a, b);
        let k = if knots_valid(&k, 1) { k } else { vec![0.0, 0.0, 1.0, 1.0] };
        return InlineCrv {
            cpts: vec![[0.0, 0.0, 0.0, NOW], [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), len, NOW]],
            p: 1,
            u: k,
        };
    }
    let p = 2 + rng.below(2) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|i| {
            let z = len * i as f64 / n as f64;
            let (x, y) = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0));
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, z * w, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    let u = valid_knots(rng, n + p + 2, p);
    InlineCrv { cpts, p, u }
}

fn gen_skin_spine(rng: &mut Rng) -> Inputs {
    let k = 1 + rng.below(5) as usize;
    let spine = gen_spine(rng);
    let a = spine.u.first().copied().unwrap_or(0.0);
    let b = spine.u.last().copied().unwrap_or(1.0);
    let mut params: Vec<f64> = (0..=k)
        .map(|i| {
            if rng.chance(0.3) {
                a + (b - a) * i as f64 / k as f64
            } else {
                rng.uniform(a, b)
            }
        })
        .collect();
    for t in &mut params {
        *t = t.clamp(a, b);
    }
    params.sort_by(f64::total_cmp);
    if rng.chance(0.5) {
        if let Some(x) = params.first_mut() {
            *x = a;
        }
        if let Some(x) = params.last_mut() {
            *x = b;
        }
    }
    let align = rng.chance(0.5);
    let cmp = rng.chance(0.5);
    let rational = rng.chance(0.3);
    let two_d = rng.chance(0.05);
    let place = |rng: &mut Rng, c: &mut InlineCrv, i: usize| {
        for pt in &mut c.cpts {
            let w = if pt[3] == NOW { 1.0 } else { pt[3] };
            if pt[2] != NOZ {
                pt[2] = if align { rng.uniform(-0.1, 0.1) * w } else { pt[2] * 0.1 + 3.0 * i as f64 * w };
            }
        }
    };
    let mut curves: Vec<InlineCrv> = if cmp {
        (0..=k)
            .map(|i| {
                let mut c = if rng.chance(0.5) {
                    gen_crv(rng)
                } else {
                    let p = 1 + rng.below(3) as usize;
                    let n = p + rng.below(3) as usize;
                    let u = knots(rng, n + p + 2, p, 0.0, 1.0);
                    let u = if knots_valid(&u, p) { u } else { knots(rng, n + p + 2, p, 0.0, 1.0) };
                    gen_crv_shape(rng, p, n, rational, two_d, u)
                };
                place(rng, &mut c, i);
                c
            })
            .collect()
    } else {
        let p = degree(rng).min(4);
        let n = p + rng.below(4) as usize;
        let u = valid_knots(rng, n + p + 2, p);
        (0..=k)
            .map(|i| {
                let mut c = gen_crv_shape(rng, p, n, rational, two_d, u.clone());
                place(rng, &mut c, i);
                c
            })
            .collect()
    };
    if rng.chance(0.25) {
        if let Some(first) = curves.first().cloned() {
            if let Some(last) = curves.last_mut() {
                *last = first;
            }
        }
    }
    let derivs = 1 + rng.below(3) as i32;
    let deg = if rng.chance(0.05) {
        1 + rng.below(6) as usize
    } else {
        match derivs {
            1 => 1 + rng.below(k.min(3) as u64) as usize,
            2 => 2 + rng.below((k + 1).min(4) as u64) as usize,
            _ => 2 + rng.below(2) as usize,
        }
    };
    let z0 = if rng.chance(0.03) {
        [0.0, 0.0, 0.0]
    } else {
        [1.0 + rng.uniform(-0.2, 0.2), rng.uniform(-0.5, 0.5), rng.uniform(-0.2, 0.2)]
    };
    Inputs::SkinSpine {
        curves: curves.into_iter().map(CrvIn::Inline).collect(),
        spine: CrvIn::Inline(spine),
        params,
        cmp,
        align,
        z0,
        subdiv: rng.below(5) as usize,
        derivs,
        deg,
        dir: if rng.chance(0.04) { 3 } else { 1 + rng.below(2) as i32 },
    }
}

fn gen_vector_field(rng: &mut Rng) -> Inputs {
    let cur = gen_crv(rng);
    let vfield = match rng.below(20) {
        0 => gen_crv(rng),
        1 => {
            let mut u = cur.u.clone();
            if let Some(x) = u.last().copied() {
                let p = cur.p;
                let len = u.len();
                for y in u.iter_mut().skip(len - p - 1) {
                    *y = x + 1.0;
                }
            }
            let n = cur.cpts.len() - 1;
            let rat = rng.chance(0.2);
            gen_crv_shape(rng, cur.p, n, rat, false, u)
        }
        _ => {
            let n = cur.cpts.len() - 1;
            let two_d = cur.cpts.first().is_some_and(|c| c[2] == NOZ) && rng.chance(0.8);
            let rat = rng.chance(0.2);
            gen_crv_shape(rng, cur.p, n, rat, two_d, cur.u.clone())
        }
    };
    Inputs::VectorField {
        cur: CrvIn::Inline(cur),
        vfield: CrvIn::Inline(vfield),
        bndy: 1 + rng.below(4) as i32,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0 | 1 => gen_extend(rng),
        2 => Inputs::MaxDeriv {
            sur: SrfIn::Inline(gen_srf(rng)),
            d: match rng.below(4) {
                0 => 0.0,
                _ => rng.uniform(-3.0, 3.0),
            },
        },
        3..=5 => gen_unbounded(rng),
        6..=8 => gen_skin_spine(rng),
        _ => gen_vector_field(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_adv_p1"
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

    /// The generator reaches successes and failures of every op and the special
    /// branches: wrapped (closed) and extended unbounded evaluations, rational and
    /// closed spine skins with end and all derivatives.
    #[test]
    fn generated_cases_cover_outcomes() {
        // [extend ok, extend err, maxd ok, unb ok, unb err, unb wrapped, unb ext,
        //  spine none ok, spine ends ok, spine all ok, spine err, spine rational ok,
        //  vfield ok, vfield err]
        let mut stats = [0usize; 14];
        for seed in 0..3000u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            let ok = flat.first() == Some(&0.0);
            match &i {
                Inputs::ExtendByDist { .. } => stats[usize::from(!ok)] += 1,
                Inputs::MaxDeriv { .. } => stats[2] += usize::from(ok),
                Inputs::Unbounded { .. } => {
                    let out = to_output(&i, &flat).unwrap();
                    let all_ok = out.values.contains_key("has_ext");
                    stats[3 + usize::from(!all_ok)] += 1;
                    match out.values.get("gflg").and_then(|g| g.first()) {
                        Some(&g) if g > 0.0 => stats[5] += 1,
                        Some(_) => stats[6] += 1,
                        None => {}
                    }
                }
                Inputs::SkinSpine {
                    derivs, curves, ..
                } => {
                    if ok {
                        stats[match derivs {
                            1 => 7,
                            2 => 8,
                            _ => 9,
                        }] += 1;
                        let c0 = resolve_crv(&curves[0]).unwrap();
                        if c0.cpts[0][3] != NOW && *derivs != 1 {
                            stats[11] += 1;
                        }
                    } else {
                        stats[10] += 1;
                    }
                }
                Inputs::VectorField { .. } => stats[12 + usize::from(!ok)] += 1,
            }
        }
        eprintln!("{stats:?}");
        assert!(stats.iter().all(|&k| k >= 10), "{stats:?}");
    }
}
