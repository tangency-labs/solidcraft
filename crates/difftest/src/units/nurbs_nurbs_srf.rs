//! Runner for unit `nurbs.nurbs_srf`: SMLib's local NURBS surface routines (control
//! net / surface-sample chord heights and turning angles, allocation and copying,
//! Bezier decomposition, splitting, the degenerate-surface test).
//!
//! C++: `solidcore_ffi::units::nurbs_nurbs_srf` (upstream
//! `source/SMLib/src/SmNurbsSrf.cpp`). Rust: `solidcore::nurbs::nurbs_srf`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record of
//! doubles, split into keys by [`to_output`]: `status` (`0` ok; an NLib error
//! number; `1000` for an NLib flag without a number; an `SmStatus` value; `-1` on the
//! Rust side for any other error, which never matches), then on success the results.
//! The constants ops give `values` = `[u_ch, v_ch, u_ang, v_ang, uv_ch]` (`0` for an
//! output not requested; upstream passes NULL). A surface is dumped as
//! `dims = [n, m, p, q, r, s]`, the control points (`net`, x y z w row-major) and the
//! knots (`u`, `v`).
//!
//! The C++ side evaluates `sm_ComputeSurfConstants` (and `sm_ComputeNetConstants`
//! with a surface) through an `SmBSplineSurface` borrowing the same NLib surface
//! (its `EvaluatePoint` is `N_SrfEvalPt` at `NL_LEFT`, `SM_ERR` on failure); the Rust
//! side passes the matching closure over [`Surface::eval`].
//!
//! A surface input is inline (`net` rows of `[x, y, z, w]`, degrees, knots) or an
//! upstream `.sms` file (`{"sms": {"$upstream_file": "Surface.sms"}}`). Every run has
//! at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::nurbs_srf::{self as ns, ConstantsRequest, NetConstants};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nurbs_nurbs_srf::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

#[path = "fixtures/nurbs_nurbs_srf_sms.rs"]
mod sms;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nurbs.nurbs_srf` runner.
pub struct Runner;

/// An inline surface.
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

/// `[u_ch, v_ch, u_ang, v_ang, uv_ch]` request flags.
type Flags = [bool; 5];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    NetConstants {
        sur: SrfIn,
        zone_tol: f64,
        want: Flags,
        /// `[ul, vl, uh, vh]`: evaluate this domain of the surface instead.
        #[serde(default)]
        surface_domain: Option<[f64; 4]>,
    },
    SurfConstants {
        sur: SrfIn,
        /// `[ul, vl, uh, vh]`.
        domain: [f64; 4],
        want: Flags,
    },
    Allocate {
        /// `[n, m, p, q, r, s]`.
        dims: [usize; 6],
    },
    Copy {
        sur: SrfIn,
    },
    Decompose {
        sur: SrfIn,
    },
    Split {
        sur: SrfIn,
        t: f64,
        dir: i32,
    },
    Degenerate {
        sur: SrfIn,
        tol: f64,
    },
}

impl Inputs {
    fn sur(&self) -> Option<&SrfIn> {
        match self {
            Inputs::NetConstants { sur, .. }
            | Inputs::SurfConstants { sur, .. }
            | Inputs::Copy { sur }
            | Inputs::Decompose { sur }
            | Inputs::Split { sur, .. }
            | Inputs::Degenerate { sur, .. } => Some(sur),
            Inputs::Allocate { .. } => None,
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
    let s = sms::parse_sms(&text, path)?;
    Ok(InlineSrf {
        net: s.net,
        p: s.p,
        q: s.q,
        u: s.u,
        v: s.v,
    })
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
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

fn build(s: &InlineSrf) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
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

fn split_srf(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
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
    match i {
        Inputs::NetConstants { .. } | Inputs::SurfConstants { .. } => {
            // Upstream writes its outputs (zeros first) whatever the status.
            if ok {
                out = out.values("values", take(flat, &mut at, 5)?.to_vec());
            } else {
                take(flat, &mut at, 5)?;
            }
        }
        Inputs::Allocate { .. } | Inputs::Copy { .. } => {
            out = split_srf(flat, &mut at, "", out)?;
        }
        Inputs::Decompose { .. } => {
            if ok {
                let k = take(flat, &mut at, 2)?;
                let nsu = count(k.first().copied().unwrap_or(f64::NAN))?;
                let nsv = count(k.get(1).copied().unwrap_or(f64::NAN))?;
                out = out.values("nsu_nsv", k.to_vec());
                for a in 0..nsu {
                    for b in 0..nsv {
                        out = split_srf(flat, &mut at, &format!("b{a}_{b}_"), out)?;
                    }
                }
            }
        }
        Inputs::Split { .. } => {
            if ok {
                out = split_srf(flat, &mut at, "l_", out)?;
                out = split_srf(flat, &mut at, "r_", out)?;
            }
        }
        Inputs::Degenerate { .. } => {
            out = out.values("degenerate", take(flat, &mut at, 1)?.to_vec());
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
    let op = match i {
        Inputs::NetConstants {
            zone_tol,
            want,
            surface_domain,
            ..
        } => {
            o.push(*zone_tol);
            o.extend(want.map(flag));
            o.push(flag(surface_domain.is_some()));
            let d = surface_domain.unwrap_or_else(|| {
                // Unused; any domain inside the knots passes the wrapper.
                let k = i.sur().and_then(|s| resolve(s).ok());
                k.map_or([0.0; 4], |s| {
                    let u0 = s.u.first().copied().unwrap_or(0.0);
                    let v0 = s.v.first().copied().unwrap_or(0.0);
                    [u0, v0, u0, v0]
                })
            });
            o.extend_from_slice(&d);
            Op::NetConstants
        }
        Inputs::SurfConstants { domain, want, .. } => {
            o.extend_from_slice(domain);
            o.extend(want.map(flag));
            Op::SurfConstants
        }
        Inputs::Allocate { dims } => {
            o.extend(dims.map(|k| k as f64));
            Op::Allocate
        }
        Inputs::Copy { .. } => Op::Copy,
        Inputs::Decompose { .. } => Op::Decompose,
        Inputs::Split { t, dir, .. } => {
            o.extend([*t, f64::from(*dir)]);
            Op::Split
        }
        Inputs::Degenerate { tol, .. } => {
            o.push(*tol);
            Op::Degenerate
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

fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(SolidError::Upstream { code, .. }) => f64::from(*code),
        // Upstream's NLib flag without an error number (see the module docs of
        // `solidcore::nurbs::nurbs_srf`).
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => 1000.0,
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

fn request(w: Flags) -> ConstantsRequest {
    let [u_chord_height, v_chord_height, u_angle_deg, v_angle_deg, uv_chord_height] = w;
    ConstantsRequest {
        u_chord_height,
        v_chord_height,
        u_angle_deg,
        v_angle_deg,
        uv_chord_height,
    }
}

fn push_constants(o: &mut Vec<f64>, r: &SolidResult<NetConstants>) {
    o.push(status(r));
    let c = r.as_ref().copied().unwrap_or_default();
    for v in [
        c.u_chord_height,
        c.v_chord_height,
        c.u_angle_deg,
        c.v_angle_deg,
        c.uv_chord_height,
    ] {
        o.push(v.unwrap_or(0.0));
    }
}

/// `SmBSplineSurface::EvaluatePoint`: `N_SrfEvalPt` at `NL_LEFT`, `SM_ERR` on failure.
fn evaluator(s: &Surface) -> impl FnMut(Point2) -> SolidResult<Point3> + '_ {
    |uv: Point2| {
        s.eval(uv.x, uv.y, Side::Left, Side::Left)
            .map_err(|_| SolidError::sm_err("SmBSplineSurface::EvaluatePoint"))
    }
}

fn extent(d: [f64; 4]) -> Extent2d {
    let [ul, vl, uh, vh] = d;
    Extent2d::from_bounds(ul, vl, uh, vh)
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    if let Inputs::Allocate { dims } = i {
        let [n, m, p, q, r, s] = *dims;
        let a = ns::allocate_nurb_surface(n, m, p, q, r, s)
            .map_err(|e| harness(format!("allocate: {e}")))?;
        o.push(0.0);
        dump(&mut o, &a);
        return Ok(o);
    }
    let src = i.sur().ok_or_else(|| harness("no surface"))?;
    let s = build(&resolve(src)?)?;
    match i {
        Inputs::NetConstants {
            zone_tol,
            want,
            surface_domain,
            ..
        } => {
            let zone = zone_tol3d(&Kernel::default(), *zone_tol);
            let r = match surface_domain {
                Some(d) => {
                    let dom = extent(*d);
                    let mut f = evaluator(&s);
                    ns::compute_net_constants(
                        Some((&mut f as &mut ns::PointEvaluator<'_>, &dom)),
                        &s,
                        zone,
                        request(*want),
                    )
                }
                None => ns::compute_net_constants(None, &s, zone, request(*want)),
            };
            push_constants(&mut o, &r);
        }
        Inputs::SurfConstants { domain, want, .. } => {
            let dom = extent(*domain);
            let mut f = evaluator(&s);
            let r = ns::compute_surf_constants(&mut f, &dom, request(*want));
            push_constants(&mut o, &r);
        }
        Inputs::Allocate { .. } => {}
        Inputs::Copy { .. } => {
            o.push(0.0);
            dump(&mut o, &ns::copy_nurb_surface(&s));
        }
        Inputs::Decompose { .. } => {
            let r = ns::decompose_srf(&s);
            o.push(status(&r));
            if let Ok(rows) = r {
                let nsu = rows.len();
                let nsv = rows.first().map_or(0, Vec::len);
                o.extend([nsu as f64, nsv as f64]);
                for b in rows.iter().flatten() {
                    dump(&mut o, b);
                }
            }
        }
        Inputs::Split { t, dir, .. } => {
            let d = Direction::from_flag(*dir).ok_or_else(|| harness("bad direction"))?;
            let r = ns::split_srf(&s, *t, d);
            o.push(status(&r));
            if let Ok((l, rr)) = r {
                dump(&mut o, &l);
                dump(&mut o, &rr);
            }
        }
        Inputs::Degenerate { tol, .. } => {
            let r = ns::is_nsrf_degenerate(&s, *tol);
            match r {
                Ok(b) => {
                    o.push(0.0);
                    o.push(flag(b));
                }
                Err(e) => return Err(harness(format!("rust bbox: {e}"))),
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

/// Net shapes that reach the special branches of the constants routines.
#[derive(Clone, Copy)]
enum Shape {
    /// Random coordinates.
    Random,
    /// A smooth (nearly planar) sheet.
    Smooth,
    /// Zig-zag rows / columns: large turning angles (the 10 rad limit).
    ZigZag,
    /// Rows and columns whose end points coincide (closed polygons).
    Closed,
    /// Rows or columns collapsed to a point (poles / singular segments).
    Collapsed,
    /// The whole net in one point (degenerate surface).
    Point,
    /// Duplicated neighbouring control points (zone tolerance skips).
    Repeated,
}

fn gen_shape(rng: &mut Rng) -> Shape {
    match rng.below(14) {
        0..=3 => Shape::Random,
        4..=5 => Shape::Smooth,
        6..=7 => Shape::ZigZag,
        8..=9 => Shape::Closed,
        10..=11 => Shape::Collapsed,
        12 => Shape::Point,
        _ => Shape::Repeated,
    }
}

/// A random valid clamped surface with a net of the given shape.
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
    let n = p + rng.below(7) as usize;
    let m = q + rng.below(7) as usize;
    let rational = rng.chance(0.3);
    let two_d = rng.chance(0.1);
    let shape = gen_shape(rng);
    let scale = match rng.below(5) {
        0 => 1e-4,
        1 => 1e3,
        _ => 1.0,
    };
    let base = [coord(rng), coord(rng), coord(rng)];
    let amp = rng.uniform(0.1, 3.0);
    let collapse_row = rng.chance(0.5);
    let collapse_at = rng.below(2) as usize;
    let mut xyz: Vec<Vec<[f64; 3]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (fi, fj) = (i as f64, j as f64);
                    match shape {
                        Shape::Random | Shape::Repeated | Shape::Collapsed => {
                            [coord(rng), coord(rng), coord(rng)]
                        }
                        Shape::Smooth => [fi, fj, amp * (0.3 * fi).sin() * (0.2 * fj).cos()],
                        Shape::ZigZag => [
                            fi * 0.1,
                            fj * 0.1,
                            amp * if (i + j) % 2 == 0 { 1.0 } else { -1.0 },
                        ],
                        Shape::Closed => {
                            let a = 2.0 * std::f64::consts::PI * fj / m as f64;
                            let b = 2.0 * std::f64::consts::PI * fi / n as f64;
                            [
                                (2.0 + b.cos()) * a.cos(),
                                (2.0 + b.cos()) * a.sin(),
                                b.sin(),
                            ]
                        }
                        Shape::Point => base,
                    }
                })
                .collect()
        })
        .collect();
    if let Shape::Repeated = shape {
        for row in &mut xyz {
            for j in (1..row.len()).rev() {
                if rng.chance(0.4) {
                    if let Some(prev) = row.get(j - 1).copied() {
                        if let Some(c) = row.get_mut(j) {
                            *c = prev;
                        }
                    }
                }
            }
        }
        for i in (1..xyz.len()).rev() {
            if rng.chance(0.3) {
                if let Some(prev) = xyz.get(i - 1).cloned() {
                    if let Some(r) = xyz.get_mut(i) {
                        *r = prev;
                    }
                }
            }
        }
    }
    if let Shape::Collapsed = shape {
        if collapse_row {
            let i = if collapse_at == 0 { 0 } else { n };
            if let Some(row) = xyz.get_mut(i) {
                if let Some(&pt) = row.first() {
                    row.fill(pt);
                }
            }
        } else {
            let j = if collapse_at == 0 { 0 } else { m };
            if let Some(pt) = xyz.first().and_then(|r| r.get(j)).copied() {
                for row in &mut xyz {
                    if let Some(c) = row.get_mut(j) {
                        *c = pt;
                    }
                }
            }
        }
    }
    let net: Vec<Vec<[f64; 4]>> = xyz
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|[x, y, z]| {
                    let (x, y, z) = (x * scale, y * scale, z * scale);
                    let z = if two_d { NOZ } else { z };
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

/// A parameter for knots `k`: interior knot, random interior, end, just inside an
/// end, or (when `outside`) beyond the domain.
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
        _ => rng.uniform(a, b),
    }
}

/// A sampling sub-domain `[ul, vl, uh, vh]` inside the knot ranges.
fn sub_domain(rng: &mut Rng, s: &InlineSrf) -> [f64; 4] {
    let pair = |rng: &mut Rng, k: &[f64]| -> [f64; 2] {
        let a = k.first().copied().unwrap_or(0.0);
        let b = k.last().copied().unwrap_or(1.0);
        match rng.below(6) {
            0..=1 => [a, b],
            2 => {
                let x = rng.uniform(a, b);
                [x, x]
            }
            _ => {
                let x = rng.uniform(a, b);
                let y = rng.uniform(a, b);
                if x <= y {
                    [x, y]
                } else {
                    [y, x]
                }
            }
        }
    };
    let [ul, uh] = pair(rng, &s.u);
    let [vl, vh] = pair(rng, &s.v);
    [ul, vl, uh, vh]
}

fn gen_want(rng: &mut Rng) -> Flags {
    if rng.chance(0.3) {
        [true; 5]
    } else {
        [(); 5].map(|()| rng.chance(0.5))
    }
}

fn gen_zone(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 0.0,
        1 => 1e-9,
        2 => 1e-6,
        3 => 1e-3,
        4 => 0.1,
        5 => 1.0,
        6 => 10.0,
        _ => rng.uniform(0.0, 0.5),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let s = gen_srf(rng);
    match rng.below(10) {
        0..=3 => {
            let surface_domain = rng.chance(0.2).then(|| sub_domain(rng, &s));
            Inputs::NetConstants {
                zone_tol: gen_zone(rng),
                want: gen_want(rng),
                surface_domain,
                sur: SrfIn::Inline(s),
            }
        }
        4..=5 => Inputs::SurfConstants {
            domain: sub_domain(rng, &s),
            want: gen_want(rng),
            sur: SrfIn::Inline(s),
        },
        6 => {
            if rng.chance(0.3) {
                let mut dims = [0usize; 6];
                for d in &mut dims {
                    *d = rng.below(9) as usize;
                }
                Inputs::Allocate { dims }
            } else {
                Inputs::Copy {
                    sur: SrfIn::Inline(s),
                }
            }
        }
        7 => Inputs::Decompose {
            sur: SrfIn::Inline(s),
        },
        8 => {
            let dir = if rng.chance(0.05) {
                3
            } else {
                1 + rng.below(2) as i32
            };
            let k = if dir == 2 { s.v.clone() } else { s.u.clone() };
            let t = param(rng, &k, true);
            Inputs::Split {
                sur: SrfIn::Inline(s),
                t,
                dir,
            }
        }
        _ => {
            let tol = match rng.below(5) {
                0 => 1e-12,
                1 => 1e-3,
                2 => 1.0,
                3 => 100.0,
                _ => rng.uniform(0.0, 20.0),
            };
            Inputs::Degenerate {
                sur: SrfIn::Inline(s),
                tol,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nurbs.nurbs_srf"
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
