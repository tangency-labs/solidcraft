//! Runner for unit `nlib.crv_geom`: NLib curve point projection / inversion
//! (`N_CrvClosestPt`), arc length (`N_CrvArcLength`) and polyline approximation
//! (`N_ApproxCrvWithPolyline`).
//!
//! C++: `solidcore_ffi::units::nlib_crv_geom` (upstream
//! `source/SMLib/src/NL_CrvGeom.cpp`). Rust: `solidcore::nurbs::crv_geom` (methods on
//! `solidcore::nurbs::crv_basic::Curve`).
//!
//! Each case runs one routine (`inputs.op`) on one curve. Both sides produce the same
//! flat record of doubles, split into keys by [`to_output`]: `status` (`0` ok, else
//! the `NL_*_ERR` number, `1000` for upstream's bare error flag of the Bezier
//! arc-length recursion limit; `-1` on the Rust side for any other error, which never
//! matches), then the outputs:
//!
//! - `closest`: `u` and `q` always (upstream returns its best values even on
//!   failure; values it never writes are NaN on both sides);
//! - `arc_length`: `len` on success;
//! - `polyline`: `count`, then `pts` unless `ret = "parameters"` and `pars`
//!   unless `ret = "points"` on success.
//!
//! A curve input is inline (`cpts` `[x, y, z, w]`, degree `p`, knots `u`) or an
//! upstream `.smc` file (`{"smc": {"$upstream_file": "curve0.smc"}}`, read here as
//! `N_CrvReadFromFilePtr` does). Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_geom::{ToleranceKind, ITER_LIMIT};
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::{SolidError, Vec3};
use solidcore_ffi::units::nlib_crv_geom::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `nlib.crv_geom` runner.
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
            Param::Knot { knot } => {
                c.u.get(knot)
                    .copied()
                    .ok_or_else(|| harness(format!("no knot {knot}")))
            }
        }
    }
}

/// What `N_ApproxCrvWithPolyline` returns (`rflg`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Ret {
    /// `NL_POINTS` (1).
    Points,
    /// `NL_PARAMETERS` (2).
    Parameters,
    /// `NL_BOTH` (3).
    #[default]
    Both,
}

impl Ret {
    fn flag(self) -> f64 {
        match self {
            Ret::Points => 1.0,
            Ret::Parameters => 2.0,
            Ret::Both => 3.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Closest {
        crv: CrvIn,
        p: [f64; 3],
        u0: Param,
        top: f64,
        toc: f64,
    },
    ArcLength {
        crv: CrvIn,
        ul: Param,
        ur: Param,
        tol: f64,
        #[serde(default)]
        relative: bool,
    },
    Polyline {
        crv: CrvIn,
        tol: f64,
        #[serde(default)]
        relative: bool,
        #[serde(default)]
        ret: Ret,
    },
}

impl Inputs {
    fn crv(&self) -> &CrvIn {
        match self {
            Inputs::Closest { crv, .. }
            | Inputs::ArcLength { crv, .. }
            | Inputs::Polyline { crv, .. } => crv,
        }
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn kind(relative: bool) -> ToleranceKind {
    if relative {
        ToleranceKind::Relative
    } else {
        ToleranceKind::Absolute
    }
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
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
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
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
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
    if x.fract() != 0.0 || !(0.0..=1e8).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

/// Splits `k` values at `at` into `key`.
fn vals(
    flat: &[f64],
    at: &mut usize,
    key: &str,
    k: usize,
    out: Output,
) -> Result<Output, HarnessError> {
    Ok(out.values(key, take(flat, at, k)?.to_vec()))
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    let a = &mut at;
    match i {
        Inputs::Closest { .. } => {
            out = vals(flat, a, "u", 1, out)?;
            out = vals(flat, a, "q", 3, out)?;
        }
        Inputs::ArcLength { .. } => {
            if ok {
                out = vals(flat, a, "len", 1, out)?;
            }
        }
        Inputs::Polyline { ret, .. } => {
            if ok {
                let k = take(flat, a, 1)?.to_vec();
                let n = count(k.first().copied().unwrap_or(f64::NAN))?;
                out = out.values("count", k);
                if *ret != Ret::Parameters {
                    out = vals(flat, a, "pts", 3 * n, out)?;
                }
                if *ret != Ret::Points {
                    out = vals(flat, a, "pars", n, out)?;
                }
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

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty curve"))?;
    let m =
        c.u.len()
            .checked_sub(1)
            .ok_or_else(|| harness("empty knots"))?;
    o.extend([n as f64, c.p as f64, m as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
}

fn tflag(relative: bool) -> f64 {
    if relative {
        2.0
    } else {
        1.0
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let mut o = Vec::new();
    encode_crv(&c, &mut o)?;
    let op = match i {
        Inputs::Closest {
            p, u0, top, toc, ..
        } => {
            o.extend_from_slice(p);
            o.extend([u0.get(&c)?, *top, *toc]);
            Op::Closest
        }
        Inputs::ArcLength {
            ul,
            ur,
            tol,
            relative,
            ..
        } => {
            o.extend([ul.get(&c)?, ur.get(&c)?, *tol, tflag(*relative)]);
            Op::ArcLength
        }
        Inputs::Polyline {
            tol, relative, ret, ..
        } => {
            o.extend([*tol, tflag(*relative), ret.flag()]);
            Op::Polyline
        }
    };
    match ffi::eval(op, &o) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// Status of a Rust error: the NLib number, `1000` for the Bezier arc-length
/// recursion limit (upstream's bare flag), `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        SolidError::NotConverged { operation, .. } if operation.starts_with("nurbs::bezier") => {
            1000.0
        }
        _ => -1.0,
    }
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn rust_body(i: &Inputs, c: &InlineCrv, cur: &Curve, o: &mut Vec<f64>) -> Result<(), SolidError> {
    match i {
        Inputs::Closest {
            p, u0, top, toc, ..
        } => {
            let u0 = u0.get(c).map_err(|_| SolidError::InvalidInput {
                operation: "difftest::nlib_crv_geom",
                reason: "bad knot index",
            })?;
            let s =
                cur.closest_pt_search(Vec3::new(p[0], p[1], p[2]), u0, *top, *toc, ITER_LIMIT)?;
            if let Some(e) = &s.error {
                if let Some(slot) = o.first_mut() {
                    *slot = status(e);
                }
            }
            let (u, q) = s
                .best
                .unwrap_or((f64::NAN, Vec3::new(f64::NAN, f64::NAN, f64::NAN)));
            o.push(u);
            push3(o, q);
        }
        Inputs::ArcLength {
            ul,
            ur,
            tol,
            relative,
            ..
        } => {
            let bad = |_| SolidError::InvalidInput {
                operation: "difftest::nlib_crv_geom",
                reason: "bad knot index",
            };
            let (ul, ur) = (ul.get(c).map_err(bad)?, ur.get(c).map_err(bad)?);
            o.push(cur.arc_length(ul, ur, *tol, kind(*relative))?);
        }
        Inputs::Polyline {
            tol, relative, ret, ..
        } => {
            let pl = cur.approx_with_polyline(*tol, kind(*relative))?;
            o.push(pl.points.len() as f64);
            if *ret != Ret::Parameters {
                for p in &pl.points {
                    push3(o, *p);
                }
            }
            if *ret != Ret::Points {
                o.extend_from_slice(&pl.params);
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = resolve(i.crv())?;
    let cur = to_curve(&c)?;
    let mut o = vec![0.0];
    match rust_body(i, &c, &cur, &mut o) {
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
    let narcs = if full {
        4
    } else {
        1 + (sweep / (0.5 * std::f64::consts::PI)) as usize
    };
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

/// A curve whose control points lie on a line, monotone along it (an exact line for
/// the arc length's type test, or nearly one).
fn gen_line(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    let a = rng.point3(-10.0, 10.0);
    let d = rng.point3(-5.0, 5.0);
    let n = c.cpts.len();
    let eps = *rng.choose(&[0.0, 0.0, 1e-12, 1e-6, 1e-3]).unwrap_or(&0.0);
    let mut ts: Vec<f64> = (0..n).map(|_| rng.unit()).collect();
    ts.sort_by(f64::total_cmp);
    if let Some(t) = ts.first_mut() {
        *t = 0.0;
    }
    if let Some(t) = ts.last_mut() {
        *t = 1.0;
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

/// A curve with a run of coincident control points (a degenerate span).
fn gen_degen(rng: &mut Rng) -> InlineCrv {
    let mut c = gen_crv(rng);
    let n = c.cpts.len();
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

fn gen_any_crv(rng: &mut Rng) -> InlineCrv {
    match rng.below(10) {
        0 | 1 => gen_circle(rng),
        2 => gen_line(rng),
        3 => gen_degen(rng),
        4 => gen_closed(rng),
        _ => gen_crv(rng),
    }
}

/// Distinct interior knots of the curve.
fn interior(c: &InlineCrv) -> Vec<f64> {
    let p = c.p;
    let mut v: Vec<f64> = c.u.get(p + 1..c.u.len() - p - 1).unwrap_or(&[]).to_vec();
    v.dedup();
    v
}

/// A parameter inside the knot range: random, an interior knot or an end.
fn gen_u_in(rng: &mut Rng, c: &InlineCrv) -> f64 {
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
        knots
            .get(rng.below(knots.len() as u64) as usize)
            .copied()
            .unwrap_or(a)
    } else {
        rng.uniform(a, b)
    }
}

/// Euclidean point of the curve's control point `k` (for points near the curve).
fn euclid(c: &InlineCrv, k: usize) -> [f64; 3] {
    let [x, y, z, w] = c.cpts.get(k).copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

fn gen_closest(rng: &mut Rng, c: InlineCrv) -> Inputs {
    let a = c.u.first().copied().unwrap_or(0.0);
    let b = c.u.last().copied().unwrap_or(1.0);
    let p = match rng.below(4) {
        0 => rng.point3(-15.0, 15.0),
        1 => {
            // Near a control point.
            let k = rng.below(c.cpts.len() as u64) as usize;
            let e = euclid(&c, k);
            let d = rng.point3(-1.0, 1.0);
            [e[0] + d[0], e[1] + d[1], e[2] + d[2]]
        }
        2 => euclid(&c, 0),
        _ => {
            let e = euclid(&c, rng.below(c.cpts.len() as u64) as usize);
            let s = rng.uniform(0.0, 0.1);
            let d = rng.point3(-1.0, 1.0);
            [e[0] + s * d[0], e[1] + s * d[1], e[2] + s * d[2]]
        }
    };
    let u0 = if rng.chance(0.03) {
        b + (b - a) * 0.01
    } else {
        gen_u_in(rng, &c)
    };
    let top = *rng
        .choose(&[1e-12, 1e-9, 1e-7, 1e-4, 1e-2])
        .unwrap_or(&1e-7);
    let toc = *rng
        .choose(&[1e-12, 1e-9, 1e-7, 1e-4, 1e-2])
        .unwrap_or(&1e-7);
    Inputs::Closest {
        crv: CrvIn::Inline(c),
        p,
        u0: Param::Value(u0),
        top,
        toc,
    }
}

fn gen_arc(rng: &mut Rng, c: InlineCrv) -> Inputs {
    let (ul, ur) = match rng.below(10) {
        0 => {
            let u = gen_u_in(rng, &c);
            (u, u)
        }
        1 => (
            c.u.first().copied().unwrap_or(0.0),
            c.u.last().copied().unwrap_or(1.0),
        ),
        2 => {
            // Reversed interval.
            let (x, y) = (gen_u_in(rng, &c), gen_u_in(rng, &c));
            (x.max(y), x.min(y))
        }
        _ => {
            let (x, y) = (gen_u_in(rng, &c), gen_u_in(rng, &c));
            (x.min(y), x.max(y))
        }
    };
    let tol = *rng
        .choose(&[1e-3, 1e-4, 1e-6, 1e-8, 1e-10, 0.1, 0.0])
        .unwrap_or(&1e-6);
    Inputs::ArcLength {
        crv: CrvIn::Inline(c),
        ul: Param::Value(ul),
        ur: Param::Value(ur),
        tol,
        relative: rng.chance(0.5),
    }
}

fn gen_polyline(rng: &mut Rng, c: InlineCrv) -> Inputs {
    let ret = match rng.below(4) {
        0 => Ret::Points,
        1 => Ret::Parameters,
        _ => Ret::Both,
    };
    // Relative tolerances only on curves whose ends are apart (see the ffi checks).
    let size = (0..c.cpts.len())
        .flat_map(|k| euclid(&c, k))
        .fold(0.0f64, |m, x| m.max(x.abs()));
    let (e0, e1) = (euclid(&c, 0), euclid(&c, c.cpts.len().saturating_sub(1)));
    let chord =
        ((e1[0] - e0[0]).powi(2) + (e1[1] - e0[1]).powi(2) + (e1[2] - e0[2]).powi(2)).sqrt();
    let relative = chord >= 1e-2 * size && rng.chance(0.4);
    let tol = if relative {
        *rng.choose(&[1e-2, 1e-3, 1e-4, 1e-5]).unwrap_or(&1e-3)
    } else {
        *rng.choose(&[1.0, 0.1, 1e-2, 1e-3, 1e-4]).unwrap_or(&1e-2)
    };
    Inputs::Polyline {
        crv: CrvIn::Inline(c),
        tol,
        relative,
        ret,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let c = gen_any_crv(rng);
    match rng.below(3) {
        0 => gen_closest(rng, c),
        1 => gen_arc(rng, c),
        _ => gen_polyline(rng, c),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_geom"
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

    /// The generator reaches success and failure of every routine.
    #[test]
    fn generated_cases_cover_statuses() {
        let mut tally: BTreeMap<(String, i64), usize> = BTreeMap::new();
        let mut rng = Rng::new(7);
        for _ in 0..600 {
            let i = gen_case(&mut rng);
            let op = serde_json::to_value(&i).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_string();
            let c = resolve(i.crv()).unwrap();
            let cur = to_curve(&c).unwrap();
            let mut o = vec![0.0];
            let st = match rust_body(&i, &c, &cur, &mut o) {
                Ok(()) => o[0],
                Err(e) => status(&e),
            };
            *tally.entry((op, st as i64)).or_default() += 1;
        }
        eprintln!("{tally:?}");
        for op in ["closest", "arc_length", "polyline"] {
            assert!(
                tally.contains_key(&(op.to_string(), 0)),
                "{op} never succeeds"
            );
        }
        assert!(tally.keys().any(|(op, s)| op == "closest" && *s == 12));
        assert!(tally.keys().any(|(op, s)| op == "arc_length" && *s != 0));
    }
}
