//! Runner for unit `nlib.srf_tool_p1`: NLib surface knot insertion and refinement,
//! splitting, Bezier decomposition, iso-curve and sub-patch extraction.
//!
//! C++: `solidcore_ffi::units::nlib_srf_tool_p1` (upstream
//! `source/SMLib/src/NL_SrfTool.cpp`). Rust: `solidcore::nurbs::srf_tool::p1`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then on
//! success the results. A surface is dumped as `dims = [n, m, p, q, r, s]`, the
//! control points (`net`, x y z w row-major) and the knots (`u`, `v`); a curve as
//! `dims = [n, p, m]`, `pts` and `knots`. Upstream's in-place mode (`inplace`) is
//! compared against the same Rust call.
//!
//! A surface input is inline (`net` rows of `[x, y, z, w]`, degrees, knots) or an
//! upstream `.sms` file (`{"sms": {"$upstream_file": "Surface.sms"}}`). Inputs must be
//! valid clamped surfaces (the FFI wrapper rejects anything else). Every run has at
//! least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::geom::Rectangle;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_tool::p1 as tool;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_tool_p1::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.srf_tool_p1` runner.
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
    InsertKnot {
        sur: SrfIn,
        t: f64,
        mt: usize,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
    Split {
        sur: SrfIn,
        t: f64,
        dir: i32,
    },
    Decompose {
        sur: SrfIn,
    },
    IsoCurve {
        sur: SrfIn,
        t: f64,
        dir: i32,
    },
    InsertKnots {
        sur: SrfIn,
        x: Vec<f64>,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
    ExtractPatch {
        sur: SrfIn,
        /// `[ul, ur, vl, vr]`.
        rect: [f64; 4],
        #[serde(default)]
        inplace: bool,
    },
}

impl Inputs {
    fn sur(&self) -> &SrfIn {
        match self {
            Inputs::InsertKnot { sur, .. }
            | Inputs::Split { sur, .. }
            | Inputs::Decompose { sur }
            | Inputs::IsoCurve { sur, .. }
            | Inputs::InsertKnots { sur, .. }
            | Inputs::ExtractPatch { sur, .. } => sur,
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

/// Splits a curve dump into `dims`, `pts`, `knots`.
fn split_crv(flat: &[f64], at: &mut usize, mut out: Output) -> Result<Output, HarnessError> {
    let dims = take(flat, at, 3)?;
    let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
    let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
    out = out.values("dims", dims.to_vec());
    out = out.values("pts", take(flat, at, 4 * (n + 1))?.to_vec());
    Ok(out.values("knots", take(flat, at, m + 1)?.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        match i {
            Inputs::InsertKnot { .. }
            | Inputs::InsertKnots { .. }
            | Inputs::ExtractPatch { .. } => {
                out = split_srf(flat, &mut at, "", out)?;
            }
            Inputs::Split { .. } => {
                out = split_srf(flat, &mut at, "l_", out)?;
                out = split_srf(flat, &mut at, "r_", out)?;
            }
            Inputs::Decompose { .. } => {
                let kl = take(flat, &mut at, 2)?;
                let kk = count(kl.first().copied().unwrap_or(f64::NAN))?;
                let ll = count(kl.get(1).copied().unwrap_or(f64::NAN))?;
                out = out.values("kk_ll", kl.to_vec());
                for a in 0..=kk {
                    for b in 0..=ll {
                        out = split_srf(flat, &mut at, &format!("b{a}_{b}_"), out)?;
                    }
                }
            }
            Inputs::IsoCurve { .. } => {
                out = split_crv(flat, &mut at, out)?;
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

// ---- C++ side ---------------------------------------------------------------------

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    encode(&resolve(i.sur())?, &mut o)?;
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let op = match i {
        Inputs::InsertKnot {
            t,
            mt,
            dir,
            inplace,
            ..
        } => {
            o.extend([*t, *mt as f64, f64::from(*dir), flag(*inplace)]);
            Op::InsertKnot
        }
        Inputs::Split { t, dir, .. } => {
            o.extend([*t, f64::from(*dir)]);
            Op::Split
        }
        Inputs::Decompose { .. } => Op::Decompose,
        Inputs::IsoCurve { t, dir, .. } => {
            o.extend([*t, f64::from(*dir)]);
            Op::IsoCurve
        }
        Inputs::InsertKnots {
            x, dir, inplace, ..
        } => {
            o.extend([f64::from(*dir), flag(*inplace), x.len() as f64]);
            o.extend_from_slice(x);
            Op::InsertKnots
        }
        Inputs::ExtractPatch { rect, inplace, .. } => {
            o.extend_from_slice(rect);
            o.push(flag(*inplace));
            Op::ExtractPatch
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
    o.extend([c.n(), c.degree(), c.m()].map(|k| k as f64));
    for pt in c.cpts() {
        o.extend_from_slice(&pt.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn direction(dir: i32) -> Result<Direction, HarnessError> {
    Direction::from_flag(dir).ok_or_else(|| harness(format!("bad direction flag {dir}")))
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let s = build(&resolve(i.sur())?)?;
    let mut o = Vec::new();
    match i {
        Inputs::InsertKnot { t, mt, dir, .. } => {
            let r = tool::insert_knot(&s, *t, *mt, direction(*dir)?);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::Split { t, dir, .. } => {
            let r = tool::split(&s, *t, direction(*dir)?);
            o.push(status(&r));
            if let Ok((l, rr)) = r {
                dump(&mut o, &l);
                dump(&mut o, &rr);
            }
        }
        Inputs::Decompose { .. } => {
            let r = tool::decompose_to_bez(&s);
            o.push(status(&r));
            if let Ok(rows) = r {
                let kk = rows.len().saturating_sub(1);
                let ll = rows.first().map_or(0, Vec::len).saturating_sub(1);
                o.extend([kk as f64, ll as f64]);
                for b in rows.iter().flatten() {
                    dump(&mut o, b);
                }
            }
        }
        Inputs::IsoCurve { t, dir, .. } => {
            let r = tool::extract_iso_crv(&s, *t, direction(*dir)?);
            o.push(status(&r));
            if let Ok(c) = r {
                dump_crv(&mut o, &c);
            }
        }
        Inputs::InsertKnots { x, dir, .. } => {
            let r = tool::insert_knots(&s, x, direction(*dir)?);
            o.push(status(&r));
            if let Ok(q) = r {
                dump(&mut o, &q);
            }
        }
        Inputs::ExtractPatch { rect, .. } => {
            let [ul, ur, vl, vr] = *rect;
            let r = tool::extract_patch(&s, Rectangle::new(ul, ur, vl, vr));
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

fn gen_dir(rng: &mut Rng) -> i32 {
    if rng.chance(0.03) {
        3
    } else {
        1 + rng.below(2) as i32
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let s = gen_srf(rng);
    let dir = gen_dir(rng);
    let (k, deg) = if dir == 2 {
        (s.v.clone(), s.q)
    } else {
        (s.u.clone(), s.p)
    };
    let inplace = rng.chance(0.3);
    match rng.below(6) {
        0 => {
            let t = param(rng, &k, true);
            let mt = rng.below(deg as u64 + 2) as usize;
            Inputs::InsertKnot {
                sur: SrfIn::Inline(s),
                t,
                mt,
                dir,
                inplace,
            }
        }
        1 => {
            let t = param(rng, &k, true);
            Inputs::Split {
                sur: SrfIn::Inline(s),
                t,
                dir,
            }
        }
        2 => Inputs::Decompose {
            sur: SrfIn::Inline(s),
        },
        3 => {
            // Iso-curve: the parameter is in the other direction's knots.
            let (k2, _) = if dir == 1 {
                (s.v.clone(), s.q)
            } else {
                (s.u.clone(), s.p)
            };
            let t = param(rng, &k2, true);
            Inputs::IsoCurve {
                sur: SrfIn::Inline(s),
                t,
                dir,
            }
        }
        4 => {
            let cnt = if rng.chance(0.05) {
                0
            } else {
                1 + rng.below(6) as usize
            };
            let mut x: Vec<f64> = (0..cnt).map(|_| param(rng, &k, false)).collect();
            x.sort_by(f64::total_cmp);
            Inputs::InsertKnots {
                sur: SrfIn::Inline(s),
                x,
                dir,
                inplace,
            }
        }
        _ => {
            let pair = |rng: &mut Rng, k: &[f64]| -> [f64; 2] {
                let ox = rng.chance(0.1);
                let x = param(rng, k, ox);
                let oy = rng.chance(0.1);
                let y = param(rng, k, oy);
                if rng.chance(0.05) || x > y {
                    [y, x]
                } else {
                    [x, y]
                }
            };
            let [ul, ur] = pair(rng, &s.u);
            let [vl, vr] = pair(rng, &s.v);
            Inputs::ExtractPatch {
                sur: SrfIn::Inline(s),
                rect: [ul, ur, vl, vr],
                inplace,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_tool_p1"
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
