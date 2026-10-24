//! Runner for unit `nlib.srf_adv_p2`: NLib surface extension by a distance
//! (`ST_surxsdr`, the recursive core of `N_SrfExtendByDist`) and the derivatives of
//! the unit surface normal (`N_SrfEvalPtNormalDeriv`).
//!
//! C++: `solidcore_ffi::units::nlib_srf_adv_p2` (upstream
//! `source/SMLib/src/NL_SrfAdv.cpp`). Rust: `solidcore::nurbs::srf_adv::p2`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `1000` for upstream's bare error flag past the recursion limit, the Rust
//! `NotConverged` of the extension; `-1` on the Rust side for any other non-NLib
//! error, which never matches), then on success the results: the extended
//! surface for `extend` (`dims = [n, m, p, q, r, s]`, the control points `net`, x y z
//! w row-major, the knots `u`, `v`), and `nd` (`N_u, N_v, N_uu, N_uv, N_vv`, xyz each)
//! for `normal_deriv`. Upstream's in-place mode (`inplace`, the way every upstream
//! caller uses `N_SrfExtendByDist`) is compared against the same Rust call.
//!
//! Past its recursion limit upstream returns a bare error flag with a stale
//! `NL_ERROR` number (left by an earlier recovered error or cleared by a callee; see
//! `plan/findings.md`). When the Rust side reports the recursion limit, a failing C++
//! side is therefore reported as the bare flag (`1000`) instead of that number; a
//! C++ success still fails the case.
//!
//! A surface input is inline (`net` rows of `[x, y, z, w]`, degrees, knots) or an
//! upstream `.sms` file (`{"sms": {"$upstream_file": "Surface.sms"}}`). Inputs must be
//! valid clamped surfaces (the FFI wrapper rejects anything else). Generated surfaces
//! are random nets or smooth grid-like nets with noise; extension distances are drawn
//! relative to the net size (small to several times the size, which exercises the
//! recursion). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_adv::p1::CurveEnd;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_adv::p2::{self as adv, SrfContinuity};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_adv_p2::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.srf_adv_p2` runner.
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
    Extend {
        sur: SrfIn,
        dist: f64,
        dir: i32,
        end: i32,
        cont: i32,
        #[serde(default)]
        depth: usize,
        #[serde(default)]
        inplace: bool,
    },
    NormalDeriv {
        sur: SrfIn,
        u: f64,
        v: f64,
        ufl: i32,
        vfl: i32,
    },
}

impl Inputs {
    fn sur(&self) -> &SrfIn {
        match self {
            Inputs::Extend { sur, .. } | Inputs::NormalDeriv { sur, .. } => sur,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- surface inputs ---------------------------------------------------------------

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut it = text.split_whitespace();
    let mut num = || -> Result<f64, HarnessError> {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))?
            .parse::<f64>()
            .map_err(|e| harness(format!("{path}: {e}")))
    };
    let int = |v: f64| -> Result<usize, HarnessError> {
        if v.fract() != 0.0 || v < 0.0 {
            return Err(harness(format!("{path}: bad count {v}")));
        }
        Ok(v as usize)
    };
    let n = int(num()?)?;
    let m = int(num()?)?;
    let p = int(num()?)?;
    let q = int(num()?)?;
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
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
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

// ---- output layout ---------------------------------------------------------------

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

/// Splits a surface dump starting at `at` into `<pfx>dims`, `<pfx>net`, `<pfx>u`,
/// `<pfx>v`.
fn split_srf(
    flat: &[f64],
    at: &mut usize,
    pfx: &str,
    mut out: Output,
) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 6)?;
    let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
    let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
    out = out.values(&format!("{pfx}dims"), dims.to_vec());
    let net = take(flat, at, 4 * (n + 1) * (m + 1))?;
    out = out.values(&format!("{pfx}net"), net.to_vec());
    let u = take(flat, at, r + 1)?;
    out = out.values(&format!("{pfx}u"), u.to_vec());
    let v = take(flat, at, s + 1)?;
    Ok(out.values(&format!("{pfx}v"), v.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        match i {
            Inputs::Extend { .. } => out = split_srf(flat, &mut at, "", out)?,
            Inputs::NormalDeriv { .. } => out = out.values("nd", take(flat, &mut at, 15)?.to_vec()),
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
    encode(&resolve(i.sur())?, &mut o)?;
    let op = match i {
        Inputs::Extend {
            dist,
            dir,
            end,
            cont,
            depth,
            inplace,
            ..
        } => {
            o.extend([
                *dist,
                f64::from(*dir),
                f64::from(*end),
                f64::from(*cont),
                *depth as f64,
                if *inplace { 1.0 } else { 0.0 },
            ]);
            Op::ExtendByDist
        }
        Inputs::NormalDeriv { u, v, ufl, vfl, .. } => {
            o.extend([*u, *v, f64::from(*ufl), f64::from(*vfl)]);
            Op::NormalDeriv
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
            // Past the recursion limit `ST_surxsdr` returns a bare error flag and
            // `NL_ERROR` still holds whatever an earlier, recovered error (or a
            // callee's `N_ErrClear`) left there: that number is not a result of the
            // routine. When the port reports the recursion limit, an upstream failure
            // is compared as the bare flag.
            if matches!(i, Inputs::Extend { .. })
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

// ---- Rust side --------------------------------------------------------------------

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `1000` for the recursion
/// limit of the extension (upstream's bare error flag), `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(SolidError::NotConverged { operation, .. })
            if operation.ends_with("srf_adv::p2::extend_by_dist_at_depth") =>
        {
            BARE_FLAG
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

fn side(f: i32) -> Result<Side, HarnessError> {
    match f {
        1 => Ok(Side::Left),
        2 => Ok(Side::Right),
        _ => Err(harness(format!("bad side flag {f}"))),
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let s = build(&resolve(i.sur())?)?;
    let mut o = Vec::new();
    match i {
        Inputs::Extend {
            dist,
            dir,
            end,
            cont,
            depth,
            ..
        } => {
            let dir = Direction::from_flag(*dir).ok_or_else(|| harness("bad direction flag"))?;
            let end = CurveEnd::from_flag(*end).ok_or_else(|| harness("bad end flag"))?;
            let cont =
                SrfContinuity::from_flag(*cont).ok_or_else(|| harness("bad continuity flag"))?;
            let r = adv::extend_by_dist_at_depth(&s, *dist, dir, end, cont, *depth);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::NormalDeriv { u, v, ufl, vfl, .. } => {
            let r = adv::eval_pt_normal_deriv(&s, *u, *v, side(*ufl)?, side(*vfl)?);
            o.push(status(&r));
            if let Ok(nd) = r {
                for p in [nd.nu, nd.nv, nd.nuu, nd.nuv, nd.nvv] {
                    o.extend([p.x, p.y, p.z]);
                }
            }
        }
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
        1 => rng.uniform(-1e4, 1e4),
        2 => rng.uniform(-1.0, 1.0) * 1e-6,
        _ => rng.uniform(-10.0, 10.0),
    }
}

/// A valid clamped knot vector for degree `p` with `cnt` knots over `[a, b]`:
/// interior knots random or on a coarse grid (giving multiplicities up to `p`).
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
                mid.push(
                    rng.uniform(a, b)
                        .max(a + (b - a) * 1e-3)
                        .min(b - (b - a) * 1e-3),
                );
            }
        }
    }
    mid.sort_by(f64::total_cmp);
    let mut k = vec![a; p + 1];
    k.extend(mid);
    k.extend(std::iter::repeat_n(b, p + 1));
    k
}

/// `N_KnotVectorIsValid`-style check (runs of equal knots).
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
fn gen_srf(rng: &mut Rng) -> InlineSrf {
    let deg = |rng: &mut Rng| {
        if rng.chance(0.05) {
            5 + rng.below(4) as usize
        } else {
            1 + rng.below(4) as usize
        }
    };
    let p = deg(rng);
    let q = deg(rng);
    let n = p + rng.below(6) as usize;
    let m = q + rng.below(6) as usize;
    let rational = rng.chance(0.4);
    let two_d = rng.chance(0.1);
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

/// A parameter for knots `k`: an interior knot, a random interior value, an end, a
/// value just inside an end, or (when `outside`) a value beyond the domain.
fn param(rng: &mut Rng, k: &[f64], outside: bool) -> f64 {
    let a = k.first().copied().unwrap_or(0.0);
    let b = k.last().copied().unwrap_or(1.0);
    let interior: Vec<f64> = k.iter().copied().filter(|&x| x > a && x < b).collect();
    match rng.below(12) {
        0..=2 => rng.choose(&interior).copied().unwrap_or((a + b) / 2.0),
        3 => {
            if rng.chance(0.5) {
                a
            } else {
                b
            }
        }
        4 => a + (b - a) * 1e-9,
        5 if outside => {
            if rng.chance(0.5) {
                a - (b - a) * rng.uniform(0.01, 0.5)
            } else {
                b + (b - a) * rng.uniform(0.01, 0.5)
            }
        }
        6 => {
            let x = rng.choose(&interior).copied().unwrap_or((a + b) / 2.0);
            x + (b - a) * rng.uniform(-1.0, 1.0) * 1e-9
        }
        _ => rng.uniform(a, b),
    }
}

/// A smooth net: a jittered grid over `[0, sx] x [0, sy]` with a gentle height
/// field, optionally rational and/or 2D.
fn gen_smooth(rng: &mut Rng) -> InlineSrf {
    let p = 1 + rng.below(4) as usize;
    let q = 1 + rng.below(4) as usize;
    let n = p + rng.below(5) as usize;
    let m = q + rng.below(5) as usize;
    let scale = match rng.below(4) {
        0 => 1.0,
        1 => 100.0,
        2 => 0.01,
        _ => rng.uniform(0.5, 20.0),
    };
    let (sx, sy) = (rng.uniform(0.5, 2.0) * scale, rng.uniform(0.5, 2.0) * scale);
    let (a, b, c) = (
        rng.uniform(-0.5, 0.5),
        rng.uniform(-0.5, 0.5),
        rng.uniform(-0.3, 0.3),
    );
    let jitter = if rng.chance(0.5) { 0.0 } else { 0.05 };
    let rational = rng.chance(0.35);
    let two_d = rng.chance(0.08);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let s = i as f64 / n as f64;
                    let t = j as f64 / m as f64;
                    let x = sx * (s + jitter * rng.uniform(-1.0, 1.0) / n as f64);
                    let y = sy * (t + jitter * rng.uniform(-1.0, 1.0) / m as f64);
                    let z = scale * (a * s * s + b * t * t + c * s * t);
                    let z = if two_d { NOZ } else { z };
                    if rational {
                        let w = rng.uniform(0.6, 1.6);
                        let zw = if z == NOZ { NOZ } else { z * w };
                        [x * w, y * w, zw, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    let (u0, u1) = domain(rng);
    let (v0, v1) = domain(rng);
    let mut u = knots(rng, n + p + 2, p, u0, u1);
    let mut v = knots(rng, m + q + 2, q, v0, v1);
    if !knots_valid(&u, p) {
        u = knots(rng, n + p + 2, p, 0.0, 1.0);
    }
    if !knots_valid(&v, q) {
        v = knots(rng, m + q + 2, q, 0.0, 1.0);
    }
    InlineSrf { net, p, q, u, v }
}

/// Size of a net: the largest coordinate range of its Euclidean points.
fn net_size(s: &InlineSrf) -> f64 {
    let pts: Vec<[f64; 3]> = s
        .net
        .iter()
        .flatten()
        .map(|c| {
            let w = if c[3] == NOW { 1.0 } else { c[3] };
            let z = if c[2] == NOZ { 0.0 } else { c[2] / w };
            [c[0] / w, c[1] / w, z]
        })
        .collect();
    (0..3)
        .map(|k| {
            let lo = pts
                .iter()
                .filter_map(|p| p.get(k).copied())
                .fold(f64::INFINITY, f64::min);
            let hi = pts
                .iter()
                .filter_map(|p| p.get(k).copied())
                .fold(f64::NEG_INFINITY, f64::max);
            hi - lo
        })
        .fold(0.0, f64::max)
}

fn gen_cont(rng: &mut Rng) -> i32 {
    match rng.below(4) {
        0 => 2,
        1 => 3,
        2 => 4,
        _ => 6,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let s = if rng.chance(0.7) {
        gen_smooth(rng)
    } else {
        gen_srf(rng)
    };
    if rng.chance(0.25) {
        let (ufl, vfl) = (1 + rng.below(2) as i32, 1 + rng.below(2) as i32);
        let (ou, ov) = (rng.chance(0.1), rng.chance(0.1));
        let u = param(rng, &s.u, ou);
        let v = param(rng, &s.v, ov);
        return Inputs::NormalDeriv {
            sur: SrfIn::Inline(s),
            u,
            v,
            ufl,
            vfl,
        };
    }
    let size = net_size(&s).max(1e-6);
    let dist = size
        * match rng.below(20) {
            0 => 0.0,
            1 => -0.1,
            2 | 3 => rng.uniform(1.0, 4.0),
            4 => rng.uniform(1e-4, 1e-2),
            _ => rng.uniform(0.02, 0.8),
        };
    let depth = if rng.chance(0.05) {
        rng.below(12) as usize
    } else {
        0
    };
    // A zero distance makes upstream's G2 iso-curve extension (used for G1 and G2)
    // loop forever; keep it to Cmax / G1R.
    let mut cont = gen_cont(rng);
    if dist == 0.0 && cont < 4 {
        cont = 4;
    }
    Inputs::Extend {
        sur: SrfIn::Inline(s),
        dist,
        dir: 1 + rng.below(2) as i32,
        end: 1 + rng.below(2) as i32,
        cont,
        depth,
        inplace: rng.chance(0.3),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_adv_p2"
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

    /// The generator reaches successful and failing extensions of every continuity
    /// and both normal-derivative outcomes.
    ///
    /// Deep-but-bounded extension recursion (`ST_curxcdr` fails only past 50 levels,
    /// nested in `ST_surxsdr`'s 10) must fit the thread stack: generated cases reach
    /// the recursion limits, and a debug build's frames are much larger than
    /// upstream's, so the loop runs on a dedicated thread with a raised stack (same
    /// pattern as the reader fuzz `run_guarded`); the release difftest profile needs
    /// the default stack.
    #[test]
    fn generated_cases_cover_outcomes() {
        const STACK: usize = 64 * 1024 * 1024;
        let h = std::thread::Builder::new()
            .stack_size(STACK)
            .spawn(|| {
                let mut stats = [0usize; 6];
                for seed in 0..1500u64 {
                    let mut rng = Rng::new(seed);
                    let i = gen_case(&mut rng);
                    let flat = rust_flat(&i).unwrap();
                    let ok = flat.first() == Some(&0.0);
                    match (&i, ok) {
                        (Inputs::Extend { cont, .. }, true) => {
                            stats[match cont {
                                2 => 0,
                                3 => 1,
                                4 => 2,
                                _ => 3,
                            }] += 1;
                        }
                        (Inputs::Extend { .. }, false) => stats[4] += 1,
                        (Inputs::NormalDeriv { .. }, true) => stats[5] += 1,
                        (Inputs::NormalDeriv { .. }, false) => {}
                    }
                }
                stats
            })
            .expect("spawn");
        let stats = h.join().unwrap();
        eprintln!("{stats:?}");
        assert!(stats.iter().all(|&k| k >= 10), "{stats:?}");
    }
}
