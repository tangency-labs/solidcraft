//! Runner for unit `nlib.srf_tool_p2`: NLib surface conditional knot removal, the
//! one-step knot-removal error bound and degree elevation.
//!
//! C++: `solidcore_ffi::units::nlib_srf_tool_p2` (upstream
//! `source/SMLib/src/NL_SrfTool.cpp`). Rust: `solidcore::nurbs::srf_tool::p2`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches), then on
//! success the results: `rt` (removal count) and the surface for `remove_conditional`,
//! `mr` for `remove_one_knot`, the surface for `elevate`. A surface is dumped as
//! `dims = [n, m, p, q, r, s]`, the control points (`net`, x y z w row-major) and the
//! knots (`u`, `v`). Upstream's in-place mode (`inplace`) is compared against the same
//! Rust call.
//!
//! A surface input is inline (`net` rows of `[x, y, z, w]`, degrees, knots) or an
//! upstream `.sms` file (`{"sms": {"$upstream_file": "Surface.sms"}}`). Inputs must be
//! valid clamped surfaces (the FFI wrapper rejects anything else). Generated surfaces
//! are random, or random with knots inserted first (`nurbs::srf_tool::p1`, green) so
//! that removals succeed. Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_tool::p1 as tool1;
use solidcore::nurbs::srf_tool::p2 as tool;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_tool_p2::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.srf_tool_p2` runner.
pub struct Runner;

/// An inline surface (storage = indices in use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InlineSrf {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    RemoveConditional {
        sur: SrfIn,
        t: f64,
        nt: usize,
        tol: f64,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
    RemoveOneKnot {
        sur: SrfIn,
        r: usize,
        s: usize,
        f: usize,
        l: usize,
        dir: i32,
        mr: f64,
    },
    Elevate {
        sur: SrfIn,
        t: usize,
        dir: i32,
        #[serde(default)]
        inplace: bool,
    },
}

impl Inputs {
    fn sur(&self) -> &SrfIn {
        match self {
            Inputs::RemoveConditional { sur, .. }
            | Inputs::RemoveOneKnot { sur, .. }
            | Inputs::Elevate { sur, .. } => sur,
        }
    }
}

pub(crate) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- surface inputs ---------------------------------------------------------------

#[path = "fixtures/nlib_srf_tool_p2_sms.rs"]
mod sms;

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let sms::SmsSurface { net, p, q, u, v } = sms::parse_sms(&text, path)?;
    Ok(InlineSrf { net, p, q, u, v })
}

pub(crate) fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
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

pub(crate) fn encode(s: &InlineSrf, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend([n as f64, m as f64, s.p as f64, s.q as f64]);
    o.extend(s.net.iter().flatten().flatten());
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

pub(crate) fn build(s: &InlineSrf) -> Result<Surface, HarnessError> {
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

pub(crate) fn take<'a>(
    flat: &'a [f64],
    at: &mut usize,
    k: usize,
) -> Result<&'a [f64], HarnessError> {
    let part = flat
        .get(*at..*at + k)
        .ok_or_else(|| harness(format!("record too short ({} values)", flat.len())))?;
    *at += k;
    Ok(part)
}

pub(crate) fn count(x: f64) -> Result<usize, HarnessError> {
    if x.fract() != 0.0 || !(0.0..=1e7).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

/// Splits a surface dump starting at `at` into `<pfx>dims`, `<pfx>net`, `<pfx>u`,
/// `<pfx>v`.
pub(crate) fn split_srf(
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
            Inputs::RemoveConditional { .. } => {
                out = out.values("rt", take(flat, &mut at, 1)?.to_vec());
                out = split_srf(flat, &mut at, "", out)?;
            }
            Inputs::RemoveOneKnot { .. } => {
                out = out.values("mr", take(flat, &mut at, 1)?.to_vec());
            }
            Inputs::Elevate { .. } => {
                out = split_srf(flat, &mut at, "", out)?;
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
        Inputs::RemoveConditional {
            t,
            nt,
            tol,
            dir,
            inplace,
            ..
        } => {
            o.extend([*t, *nt as f64, *tol, f64::from(*dir), flag(*inplace)]);
            Op::RemoveConditional
        }
        Inputs::RemoveOneKnot {
            r,
            s,
            f,
            l,
            dir,
            mr,
            ..
        } => {
            o.extend([
                *r as f64,
                *s as f64,
                *f as f64,
                *l as f64,
                f64::from(*dir),
                *mr,
            ]);
            Op::RemoveOneKnot
        }
        Inputs::Elevate {
            t, dir, inplace, ..
        } => {
            o.extend([*t as f64, f64::from(*dir), flag(*inplace)]);
            Op::ElevateDegree
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
pub(crate) fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(_) => -1.0,
    }
}

pub(crate) fn dump(o: &mut Vec<f64>, s: &Surface) {
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

pub(crate) fn direction(dir: i32) -> Result<Direction, HarnessError> {
    Direction::from_flag(dir).ok_or_else(|| harness(format!("bad direction flag {dir}")))
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let s = build(&resolve(i.sur())?)?;
    let mut o = Vec::new();
    match i {
        Inputs::RemoveConditional {
            t, nt, tol, dir, ..
        } => {
            let r = tool::remove_knot_conditional(&s, *t, *nt, *tol, direction(*dir)?);
            o.push(status(&r));
            if let Ok(q) = r {
                o.push(q.removed as f64);
                dump(&mut o, &q.surface);
            }
        }
        Inputs::RemoveOneKnot {
            r,
            s: m,
            f,
            l,
            dir,
            mr,
            ..
        } => {
            let r = tool::remove_one_knot(&s, *r, *m, *f, *l, direction(*dir)?, *mr);
            o.push(status(&r));
            if let Ok(x) = r {
                o.push(x);
            }
        }
        Inputs::Elevate { t, dir, .. } => {
            let r = tool::elevate_degree(&s, *t, direction(*dir)?);
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

pub(crate) fn coord(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => rng.uniform(-1e4, 1e4),
        2 => rng.uniform(-1.0, 1.0) * 1e-6,
        _ => rng.uniform(-10.0, 10.0),
    }
}

/// A valid clamped knot vector for degree `p` with `cnt` knots over `[a, b]`:
/// interior knots random or on a coarse grid (giving multiplicities up to `p`).
pub(crate) fn knots(rng: &mut Rng, cnt: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
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
pub(crate) fn knots_valid(u: &[f64], p: usize) -> bool {
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

pub(crate) fn weight(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0..=2 => 1.0,
        3 => rng.uniform(0.01, 0.1),
        _ => rng.uniform(0.5, 2.0),
    }
}

pub(crate) fn domain(rng: &mut Rng) -> (f64, f64) {
    match rng.below(4) {
        0 => (0.0, 1.0),
        1 => (rng.uniform(-5.0, 0.0), rng.uniform(0.5, 5.0)),
        2 => (rng.uniform(-100.0, 100.0), rng.uniform(150.0, 300.0)),
        _ => (0.0, rng.uniform(0.1, 10.0)),
    }
}

/// A random valid clamped surface.
pub(crate) fn gen_srf(rng: &mut Rng) -> InlineSrf {
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
pub(crate) fn param(rng: &mut Rng, k: &[f64], outside: bool) -> f64 {
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

/// Back from a Rust surface to an inline input.
pub(crate) fn inline(s: &Surface) -> InlineSrf {
    let (p, q) = s.degrees();
    InlineSrf {
        net: s
            .rows()
            .map(|row| row.iter().map(|c| c.to_array()).collect())
            .collect(),
        p,
        q,
        u: s.u_knots().to_vec(),
        v: s.v_knots().to_vec(),
    }
}

/// A random surface, often with knots inserted (in either direction) so that some of
/// its knots are removable.
pub(crate) fn gen_removable(rng: &mut Rng) -> InlineSrf {
    let s = gen_srf(rng);
    if rng.chance(0.3) {
        return s;
    }
    let Ok(mut sur) = build(&s) else {
        return s;
    };
    for _ in 0..=rng.below(3) {
        let dir = if rng.chance(0.5) {
            Direction::U
        } else {
            Direction::V
        };
        let (k, deg) = if dir == Direction::U {
            (sur.u_knots().to_vec(), sur.p())
        } else {
            (sur.v_knots().to_vec(), sur.q())
        };
        let t = param(rng, &k, false);
        let mt = 1 + rng.below(deg as u64) as usize;
        if let Ok(next) = tool1::insert_knot(&sur, t, mt, dir) {
            sur = next;
        }
    }
    inline(&sur)
}

pub(crate) fn gen_tol(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => -1.0,
        2 => 1.0,
        3 => 1e3,
        4 => rng.uniform(1e-3, 1e-1),
        5 | 6 => 1e-12,
        _ => 1e-9,
    }
}

/// `(r, s)` of a random distinct interior knot of `k` (last index of its run and its
/// multiplicity), if there is one.
fn interior_knot(rng: &mut Rng, k: &[f64]) -> Option<(usize, usize)> {
    let (a, b) = (*k.first()?, *k.last()?);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (idx, &x) in k.iter().enumerate() {
        if x <= a || x >= b {
            continue;
        }
        match runs.last_mut() {
            Some((last, mult)) if k.get(*last) == Some(&x) => {
                *last = idx;
                *mult += 1;
            }
            _ => runs.push((idx, 1)),
        }
    }
    rng.choose(&runs).copied()
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let s = gen_removable(rng);
    let dir = gen_dir(rng);
    let (k, deg, hi, other) = if dir == 2 {
        (
            s.v.clone(),
            s.q,
            s.net.first().map_or(0, Vec::len) - 1,
            s.net.len() - 1,
        )
    } else {
        (
            s.u.clone(),
            s.p,
            s.net.len() - 1,
            s.net.first().map_or(0, Vec::len) - 1,
        )
    };
    let inplace = rng.chance(0.3);
    match rng.below(3) {
        0 => {
            let interior: Vec<f64> = k
                .iter()
                .copied()
                .filter(|&x| Some(&x) > k.first() && Some(&x) < k.last())
                .collect();
            let t = if rng.chance(0.85) {
                rng.choose(&interior)
                    .copied()
                    .unwrap_or_else(|| param(rng, &k, true))
            } else {
                param(rng, &k, true)
            };
            let nt = rng.below(deg as u64 + 2) as usize;
            Inputs::RemoveConditional {
                sur: SrfIn::Inline(s),
                t,
                nt,
                tol: gen_tol(rng),
                dir,
                inplace,
            }
        }
        1 => {
            // Keep the arguments inside upstream's arrays (as the FFI demands); a
            // Bezier direction has no interior knot, so elevate instead.
            let Some((r, mut m)) =
                interior_knot(rng, &k).filter(|&(r, m)| r > deg && m <= deg && r - m < hi)
            else {
                return Inputs::Elevate {
                    sur: SrfIn::Inline(s),
                    t: 1 + rng.below(2) as usize,
                    dir,
                    inplace,
                };
            };
            if rng.chance(0.1) && m > 1 {
                m -= 1;
            }
            let f = rng.below(other as u64 + 1) as usize;
            let l = if rng.chance(0.1) {
                f.saturating_sub(1)
            } else {
                f + rng.below((other - f) as u64 + 1) as usize
            };
            let mr = match rng.below(4) {
                0 => -1.0,
                1 => rng.uniform(0.0, 1.0),
                _ => f64::MAX,
            };
            Inputs::RemoveOneKnot {
                sur: SrfIn::Inline(s),
                r,
                s: m,
                f,
                l,
                dir,
                mr,
            }
        }
        _ => {
            let t = if rng.chance(0.05) {
                rng.below(20) as usize
            } else {
                rng.below(4) as usize
            };
            Inputs::Elevate {
                sur: SrfIn::Inline(s),
                t,
                dir,
                inplace,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_tool_p2"
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

    /// The generator reaches every branch that matters: successful and refused
    /// removals (including partial ones), errors and elevations.
    #[test]
    fn generated_cases_cover_outcomes() {
        let mut stats = [0usize; 8];
        for seed in 0..3000u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            let ok = flat.first() == Some(&0.0);
            match (&i, ok) {
                (Inputs::RemoveConditional { nt, .. }, true) => {
                    let rt = flat[1] as usize;
                    stats[usize::from(rt > 0)] += 1;
                    if rt > 0 && rt < *nt {
                        stats[2] += 1;
                    }
                }
                (Inputs::RemoveConditional { .. }, false) => stats[3] += 1,
                (Inputs::RemoveOneKnot { .. }, true) => stats[4] += 1,
                (Inputs::Elevate { t, .. }, true) => stats[5 + usize::from(*t > 0)] += 1,
                (_, false) => stats[7] += 1,
            }
        }
        eprintln!("{stats:?}");
        assert!(stats.iter().all(|&k| k >= 10), "{stats:?}");
    }
}
