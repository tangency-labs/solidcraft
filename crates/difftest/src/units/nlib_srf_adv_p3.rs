//! Runner for unit `nlib.srf_adv_p3`: NLib surface grid points and normals, surface
//! extension by a parameter distance, swung, skinned and translational sweep
//! surfaces.
//!
//! C++: `solidcore_ffi::units::nlib_srf_adv_p3` (upstream
//! `source/SMLib/src/NL_SrfAdv.cpp`). Rust: `solidcore::nurbs::srf_adv::p3`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then on
//! success the results: `pts` and `nrm` (xyz, row-major) for `grid`; the surface
//! (`dims = [n, m, p, q, r, s]`, `net` x y z w row-major, `u`, `v`) for the others,
//! and for `skin` also every cross section after the call (`c<i>dims = [n, p, m]`,
//! `c<i>net`, `c<i>u`), since upstream makes them compatible in place. Upstream's
//! in-place extension (`inplace`) is compared against the same Rust call.
//!
//! Surfaces are inline or upstream `.sms` files (`{"sms": {"$upstream_file": ...}}`),
//! curves inline or `.smc` files (`{"smc": ...}`). Inputs must be valid clamped
//! surfaces and curves (the FFI wrapper rejects anything else). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::CurveEnd;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_adv::p3 as adv;
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_adv_p3::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

#[path = "fixtures/nlib_srf_adv_p3_sms.rs"]
mod sms;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.srf_adv_p3` runner.
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
    Grid {
        sur: SrfIn,
        u: Vec<f64>,
        v: Vec<f64>,
        ufl: i32,
        vfl: i32,
    },
    Extend {
        sur: SrfIn,
        par: f64,
        dir: i32,
        end: i32,
        cont: i32,
        #[serde(default)]
        inplace: bool,
    },
    Swung {
        prof: CrvIn,
        traj: CrvIn,
        alf: f64,
    },
    Skin {
        curves: Vec<CrvIn>,
        cmp: bool,
        cld: bool,
        deg: usize,
        dir: i32,
        knot_tol: f64,
    },
    Sweep {
        traj: CrvIn,
        sect: CrvIn,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let s = sms::parse_sms(&text, path)?;
    Ok(InlineSrf {
        net: s.net,
        p: s.p,
        q: s.q,
        u: s.u,
        v: s.v,
    })
}

fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let c = crate::fixtures::text_curve::read(path)?;
    Ok(InlineCrv {
        cpts: c.cpts,
        p: c.p,
        u: c.u,
    })
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
    let n = s
        .net
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))?;
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
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    o.extend([n as f64, c.p as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}

fn build(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                .collect()
        })
        .collect();
    Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("rust surface: {e}")))
}

fn build_crv(c: &InlineCrv) -> Result<Curve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
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

fn split_srf(flat: &[f64], at: &mut usize, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 6)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
    out = out.values("dims", dims.to_vec());
    out = out.values("net", take(flat, at, 4 * (n + 1) * (m + 1))?.to_vec());
    out = out.values("u", take(flat, at, r + 1)?.to_vec());
    Ok(out.values("v", take(flat, at, s + 1)?.to_vec()))
}

fn split_crv(
    flat: &[f64],
    at: &mut usize,
    pfx: &str,
    mut out: Output,
) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m) = (dim(0)?, dim(2)?);
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    out = out.values(&format!("{pfx}net"), take(flat, at, 4 * (n + 1))?.to_vec());
    Ok(out.values(&format!("{pfx}u"), take(flat, at, m + 1)?.to_vec()))
}

/// `(nu, nv, curve count)` of an input (the record layout depends on them).
fn layout(i: &Inputs) -> (usize, usize, usize) {
    match i {
        Inputs::Grid { u, v, .. } => (u.len(), v.len(), 0),
        Inputs::Skin { curves, .. } => (0, 0, curves.len()),
        _ => (0, 0, 0),
    }
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        let (nu, nv, nc) = layout(i);
        if let Inputs::Grid { .. } = i {
            out = out.values("pts", take(flat, &mut at, 3 * nu * nv)?.to_vec());
            out = out.values("nrm", take(flat, &mut at, 3 * nu * nv)?.to_vec());
        } else {
            out = split_srf(flat, &mut at, out)?;
            for c in 0..nc {
                out = split_crv(flat, &mut at, &format!("c{c}"), out)?;
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
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let op = match i {
        Inputs::Grid {
            sur,
            u,
            v,
            ufl,
            vfl,
        } => {
            encode(&resolve(sur)?, &mut o)?;
            o.push(u.len() as f64);
            o.extend_from_slice(u);
            o.push(v.len() as f64);
            o.extend_from_slice(v);
            o.extend([f64::from(*ufl), f64::from(*vfl)]);
            Op::Grid
        }
        Inputs::Extend {
            sur,
            par,
            dir,
            end,
            cont,
            inplace,
        } => {
            encode(&resolve(sur)?, &mut o)?;
            o.extend([
                *par,
                f64::from(*dir),
                f64::from(*end),
                f64::from(*cont),
                flag(*inplace),
            ]);
            Op::Extend
        }
        Inputs::Swung { prof, traj, alf } => {
            encode_crv(&resolve_crv(prof)?, &mut o)?;
            encode_crv(&resolve_crv(traj)?, &mut o)?;
            o.push(*alf);
            Op::Swung
        }
        Inputs::Skin {
            curves,
            cmp,
            cld,
            deg,
            dir,
            knot_tol,
        } => {
            let k = curves
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no curves"))?;
            o.push(k as f64);
            for c in curves {
                encode_crv(&resolve_crv(c)?, &mut o)?;
            }
            o.extend([
                flag(*cmp),
                flag(*cld),
                *deg as f64,
                f64::from(*dir),
                *knot_tol,
            ]);
            Op::Skin
        }
        Inputs::Sweep { traj, sect } => {
            encode_crv(&resolve_crv(traj)?, &mut o)?;
            encode_crv(&resolve_crv(sect)?, &mut o)?;
            Op::TransSweep
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

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
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
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend_from_slice(&pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn direction(dir: i32) -> Result<Direction, HarnessError> {
    Direction::from_flag(dir).ok_or_else(|| harness(format!("bad direction flag {dir}")))
}

fn side(fl: i32) -> Result<Side, HarnessError> {
    match fl {
        1 => Ok(Side::Left),
        2 => Ok(Side::Right),
        _ => Err(harness(format!("bad side flag {fl}"))),
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Grid {
            sur,
            u,
            v,
            ufl,
            vfl,
        } => {
            let s = build(&resolve(sur)?)?;
            let r = adv::grid_pts_normals(&s, u, v, side(*ufl)?, side(*vfl)?);
            o.push(status(&r));
            if let Ok(g) = r {
                for row in &g.points {
                    for p in row {
                        o.extend([p.x, p.y, p.z]);
                    }
                }
                for row in &g.normals {
                    for p in row {
                        o.extend([p.x, p.y, p.z]);
                    }
                }
            }
        }
        Inputs::Extend {
            sur,
            par,
            dir,
            end,
            cont,
            ..
        } => {
            let s = build(&resolve(sur)?)?;
            let end = CurveEnd::from_flag(*end).ok_or_else(|| harness("bad end flag"))?;
            let cont =
                adv::ExtendContinuity::from_flag(*cont).ok_or_else(|| harness("bad cont flag"))?;
            let r = adv::extend_by_param_dist(&s, *par, direction(*dir)?, end, cont);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::Swung { prof, traj, alf } => {
            let a = build_crv(&resolve_crv(prof)?)?;
            let b = build_crv(&resolve_crv(traj)?)?;
            let r = adv::create_swung_srf(&a, &b, *alf);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::Skin {
            curves,
            cmp,
            cld,
            deg,
            dir,
            knot_tol,
        } => {
            let mut cs = curves
                .iter()
                .map(|c| build_crv(&resolve_crv(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            let r = adv::create_skin_srf(&mut cs, *cmp, *cld, *deg, direction(*dir)?, *knot_tol);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
                for c in &cs {
                    dump_crv(&mut o, c);
                }
            }
        }
        Inputs::Sweep { traj, sect } => {
            let t = build_crv(&resolve_crv(traj)?)?;
            let s = build_crv(&resolve_crv(sect)?)?;
            let r = adv::create_trans_sweep_srf(&t, &s);
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
        && runs.iter().all(|&(x, k)| {
            if x == first || x == last {
                k == p + 1
            } else {
                k <= p
            }
        })
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
                        let (x, y) = (
                            i as f64 + rng.uniform(-0.3, 0.3),
                            j as f64 + rng.uniform(-0.3, 0.3),
                        );
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
        let apex = net
            .first()
            .and_then(|r| r.first())
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, NOW]);
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

fn gen_crv_shape(
    rng: &mut Rng,
    p: usize,
    n: usize,
    rational: bool,
    two_d: bool,
    u: Vec<f64>,
) -> InlineCrv {
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

/// Grid parameters for knots `k`: sorted, from knots, random values and the ends;
/// sometimes the first or last just outside the domain.
fn gen_params(rng: &mut Rng, k: &[f64], left: bool) -> Vec<f64> {
    let a = k.first().copied().unwrap_or(0.0);
    let b = k.last().copied().unwrap_or(1.0);
    let cnt = 1 + rng.below(6) as usize;
    let mut t: Vec<f64> = (0..cnt)
        .map(|_| match rng.below(6) {
            0 => rng.choose(k).copied().unwrap_or(a),
            1 => a,
            _ => rng.uniform(a, b),
        })
        .collect();
    t.sort_by(f64::total_cmp);
    if rng.chance(0.3) {
        t.push(b);
    }
    if rng.chance(0.04) {
        if let Some(x) = t.first_mut() {
            *x = a - (b - a) * 0.01;
        }
    }
    if rng.chance(0.04) {
        if let Some(x) = t.last_mut() {
            *x = b + (b - a) * 0.01;
        }
    }
    if left {
        // Inner parameters must stay below the domain end for NL_LEFT.
        let len = t.len();
        for (idx, x) in t.iter_mut().enumerate() {
            if idx > 0 && idx + 1 < len && *x >= b {
                *x = a + (b - a) * 0.5;
            }
        }
    }
    t
}

fn gen_grid(rng: &mut Rng) -> Inputs {
    let s = gen_srf(rng);
    let ufl = 1 + rng.below(2) as i32;
    let vfl = 1 + rng.below(2) as i32;
    let u = gen_params(rng, &s.u, ufl == 1);
    let v = gen_params(rng, &s.v, vfl == 1);
    Inputs::Grid {
        sur: SrfIn::Inline(s),
        u,
        v,
        ufl,
        vfl,
    }
}

fn gen_extend(rng: &mut Rng) -> Inputs {
    let s = gen_srf(rng);
    let dir = if rng.chance(0.05) {
        3
    } else {
        1 + rng.below(2) as i32
    };
    let end = 1 + rng.below(2) as i32;
    let cont = *rng.choose(&[2, 3, 4, 6]).unwrap_or(&2);
    let k = if dir == 1 { &s.u } else { &s.v };
    let a = k.first().copied().unwrap_or(0.0);
    let b = k.last().copied().unwrap_or(1.0);
    let frac = match rng.below(10) {
        0 => rng.uniform(1.0, 3.0),
        1 => rng.uniform(0.9, 1.0),
        2 => 1e-6,
        _ => rng.uniform(0.01, 0.6),
    };
    let par = if end == 1 {
        a - frac * (b - a)
    } else {
        b + frac * (b - a)
    };
    Inputs::Extend {
        sur: SrfIn::Inline(s),
        par,
        dir,
        end,
        cont,
        inplace: rng.chance(0.3),
    }
}

fn gen_skin(rng: &mut Rng) -> Inputs {
    let k = 1 + rng.below(5) as usize;
    let cmp = rng.chance(0.6);
    let rational = rng.chance(0.3);
    let two_d = rng.chance(0.03);
    let mut curves: Vec<InlineCrv> = if cmp {
        (0..=k)
            .map(|_| {
                if rng.chance(0.5) {
                    gen_crv(rng)
                } else {
                    // Same degree, different knots: cheap but nontrivial merging.
                    let p = 1 + rng.below(3) as usize;
                    let n = p + rng.below(3) as usize;
                    let u = knots(rng, n + p + 2, p, 0.0, 1.0);
                    let u = if knots_valid(&u, p) {
                        u
                    } else {
                        knots(rng, n + p + 2, p, 0.0, 1.0)
                    };
                    gen_crv_shape(rng, p, n, rational, two_d, u)
                }
            })
            .collect()
    } else {
        let p = degree(rng).min(4);
        let n = p + rng.below(4) as usize;
        let u = valid_knots(rng, n + p + 2, p);
        (0..=k)
            .map(|i| {
                let mut c = gen_crv_shape(rng, p, n, rational, two_d, u.clone());
                // Stack the sections along z so that the skin is regular.
                for pt in &mut c.cpts {
                    if pt[2] != NOZ {
                        let w = if pt[3] == NOW { 1.0 } else { pt[3] };
                        pt[2] += 5.0 * i as f64 * w;
                    }
                }
                c
            })
            .collect()
    };
    let cld = rng.chance(0.5);
    if rng.chance(0.3) {
        if let Some(first) = curves.first().cloned() {
            if let Some(last) = curves.last_mut() {
                *last = first;
            }
        }
    }
    let deg = 1 + rng.below(k.min(3) as u64) as usize;
    let dir = if rng.chance(0.04) {
        3
    } else {
        1 + rng.below(2) as i32
    };
    let knot_tol = match rng.below(4) {
        0 => 1e-3,
        1 => 1e-9,
        _ => 0.0,
    };
    Inputs::Skin {
        curves: curves.into_iter().map(CrvIn::Inline).collect(),
        cmp,
        cld,
        deg,
        dir,
        knot_tol,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(10) {
        0..=2 => gen_grid(rng),
        3..=5 => gen_extend(rng),
        6 => Inputs::Swung {
            prof: CrvIn::Inline(gen_crv(rng)),
            traj: CrvIn::Inline(gen_crv(rng)),
            alf: match rng.below(4) {
                0 => 1.0,
                1 => 0.0,
                _ => rng.uniform(-3.0, 3.0),
            },
        },
        7 => Inputs::Sweep {
            traj: CrvIn::Inline(gen_crv(rng)),
            sect: CrvIn::Inline(gen_crv(rng)),
        },
        _ => gen_skin(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_adv_p3"
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

    /// The generator reaches successes and failures of every op, and the special
    /// branches: rational G2 and C-max extensions, ruled extensions, closed skins.
    #[test]
    fn generated_cases_cover_outcomes() {
        // [grid ok, grid err, extend ok, extend err, swung ok, sweep ok, skin ok,
        //  skin err, rational G2 ok, C-max ok, G1R ok, closed skin ok]
        let mut stats = [0usize; 12];
        for seed in 0..3000u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            let ok = flat.first() == Some(&0.0);
            let slot = match (&i, ok) {
                (Inputs::Grid { .. }, true) => 0,
                (Inputs::Grid { .. }, false) => 1,
                (Inputs::Extend { .. }, true) => 2,
                (Inputs::Extend { .. }, false) => 3,
                (Inputs::Swung { .. }, _) => 4,
                (Inputs::Sweep { .. }, _) => 5,
                (Inputs::Skin { .. }, true) => 6,
                (Inputs::Skin { .. }, false) => 7,
            };
            stats[slot] += 1;
            match (&i, ok) {
                (Inputs::Extend { sur, cont, .. }, true) => {
                    let rat = resolve(sur).unwrap().net[0][0][3] != NOW;
                    match cont {
                        3 if rat => stats[8] += 1,
                        4 => stats[9] += 1,
                        6 => stats[10] += 1,
                        _ => {}
                    }
                }
                (
                    Inputs::Skin {
                        curves, cld: true, ..
                    },
                    true,
                ) => {
                    let f = resolve_crv(&curves[0]).unwrap();
                    let l = resolve_crv(curves.last().unwrap()).unwrap();
                    if f == l {
                        stats[11] += 1;
                    }
                }
                _ => {}
            }
        }
        eprintln!("{stats:?}");
        assert!(stats.iter().all(|&k| k >= 10), "{stats:?}");
    }
}
