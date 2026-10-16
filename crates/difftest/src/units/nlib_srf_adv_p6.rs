//! Runner for unit `nlib.srf_adv_p6`: sweep surfaces with scaling
//! (`N_CreateSweepScale`) and n-sided hole filling (`N_FillNSidedHole`).
//!
//! C++: `solidcore_ffi::units::nlib_srf_adv_p6` (upstream
//! `source/SMLib/src/NL_SrfAdv.cpp`). Rust: `solidcore::nurbs::srf_adv::p6`.
//!
//! A case is `{"op": "sweep", ...}` or `{"op": "fill", ...}`. Both sides produce the
//! same flat record of doubles, split into keys by [`to_output`]: `status` (`0` ok,
//! else the `NL_*_ERR` number; `-1` on the Rust side for a non-NLib error, which never
//! matches), then on success:
//!
//! - sweep: the surface (`dims = [n, m, p, q, r, s]`, `net` x y z w row-major, `u`,
//!   `v`);
//! - fill: the patches `s<i>...`, `center`, `normal`, and the boundary curves `c<i>...`
//!   and given derivatives (`d<i>has`, `d<i>...`) after the call (upstream modifies
//!   them in place); a curve is `dims = [n, p, m]`, `net`, `u`.
//!
//! Curves are inline (`{"cpts": [[x, y, z, w]...], "p", "u"}`, `w` = `NOW` for
//! non-rational, `z` = `NOZ` for 2D), control points with a uniform clamped knot
//! vector (`{"pts": [[x, y, z]...], "deg", "range"?}`), or a full rational circle
//! (`{"circle": r, "plane": "yz" | "xy", "center"?}`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_adv::p6 as adv;
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_adv_p6 as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1000;

/// `nlib.srf_adv_p6` runner.
pub struct Runner;

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
    /// Control points with a uniform clamped knot vector over `range` (default
    /// `[0, 1]`).
    Ctrl {
        pts: Vec<[f64; 3]>,
        deg: usize,
        #[serde(default)]
        range: Option<[f64; 2]>,
    },
    /// A full rational circle (9 control points, quarter arcs, knots `[0, 1]`).
    Circle {
        circle: f64,
        plane: String,
        #[serde(default)]
        center: Option<[f64; 3]>,
    },
    /// Inline data.
    Inline(InlineCrv),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SweepIn {
    traj: CrvIn,
    sect: CrvIn,
    z0: [f64; 3],
    #[serde(default)]
    cur_z: Option<CrvIn>,
    #[serde(default)]
    cur_s: Option<CrvIn>,
    #[serde(default)]
    tflg: bool,
    #[serde(default)]
    tc: f64,
    deg: usize,
    dir: i32,
    e: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FillIn {
    curves: Vec<CrvIn>,
    #[serde(default)]
    der: Option<Vec<Option<CrvIn>>>,
    eps: f64,
    /// `[CI, NI]` when passed in (`flg = NL_YES`).
    #[serde(default)]
    center: Option<[[f64; 3]; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Inputs {
    Sweep(SweepIn),
    Fill(FillIn),
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

fn uniform_knots(n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let spans = n + 1 - p;
    let mut u = vec![a; p + 1];
    for i in 1..spans {
        u.push(a + (b - a) * i as f64 / spans as f64);
    }
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn circle(r: f64, plane: &str, c: [f64; 3]) -> Result<InlineCrv, HarnessError> {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let base: [[f64; 3]; 9] = [
        [1.0, 0.0, 1.0],
        [1.0, 1.0, h],
        [0.0, 1.0, 1.0],
        [-1.0, 1.0, h],
        [-1.0, 0.0, 1.0],
        [-1.0, -1.0, h],
        [0.0, -1.0, 1.0],
        [1.0, -1.0, h],
        [1.0, 0.0, 1.0],
    ];
    let cpts = base
        .iter()
        .map(|&[a, b, w]| {
            let (x, y, z) = match plane {
                "yz" => (c[0], c[1] + r * a, c[2] + r * b),
                "xy" => (c[0] + r * a, c[1] + r * b, c[2]),
                _ => return Err(harness(format!("bad circle plane {plane}"))),
            };
            Ok([x * w, y * w, z * w, w])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let u = vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
    Ok(InlineCrv { cpts, p: 2, u })
}

fn resolve_crv(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::Ctrl { pts, deg, range } => {
            let n = pts.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
            if *deg == 0 || *deg > n {
                return Err(harness("bad degree"));
            }
            let [a, b] = range.unwrap_or([0.0, 1.0]);
            Ok(InlineCrv {
                cpts: pts.iter().map(|q| [q[0], q[1], q[2], NOW]).collect(),
                p: *deg,
                u: uniform_knots(n, *deg, a, b),
            })
        }
        CrvIn::Circle {
            circle: r,
            plane,
            center,
        } => circle(*r, plane, center.unwrap_or([0.0; 3])),
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

fn encode_crv(c: &InlineCrv, o: &mut Vec<f64>) -> Result<(), HarnessError> {
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
    o.extend([n as f64, c.p as f64]);
    o.extend(c.cpts.iter().flatten());
    o.extend_from_slice(&c.u);
    Ok(())
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

fn split_srf(flat: &[f64], at: &mut usize, pfx: &str, mut out: Output) -> Result<Output, HarnessError> {
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

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        match i {
            Inputs::Sweep(_) => out = split_srf(flat, &mut at, "", out)?,
            Inputs::Fill(f) => {
                for s in 0..f.curves.len() {
                    out = split_srf(flat, &mut at, &format!("s{s}"), out)?;
                }
                out = out.values("center", take(flat, &mut at, 3)?.to_vec());
                out = out.values("normal", take(flat, &mut at, 3)?.to_vec());
                for c in 0..f.curves.len() {
                    out = split_crv(flat, &mut at, &format!("c{c}"), out)?;
                }
                if f.der.is_some() {
                    for d in 0..f.curves.len() {
                        let has = take(flat, &mut at, 1)?.to_vec();
                        let present = has.first() == Some(&1.0);
                        out = out.values(&format!("d{d}has"), has);
                        if present {
                            out = split_crv(flat, &mut at, &format!("d{d}"), out)?;
                        }
                    }
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

// ---- C++ side -----------------------------------------------------------------------

/// Upstream's `der` array has one entry per curve; missing trailing entries are NULL.
fn der_entry(der: &[Option<CrvIn>], i: usize) -> Option<&CrvIn> {
    der.get(i).and_then(Option::as_ref)
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Sweep(s) => {
            encode_crv(&resolve_crv(&s.traj)?, &mut o)?;
            encode_crv(&resolve_crv(&s.sect)?, &mut o)?;
            o.extend_from_slice(&s.z0);
            for c in [&s.cur_z, &s.cur_s] {
                o.push(flag(c.is_some()));
                if let Some(c) = c {
                    encode_crv(&resolve_crv(c)?, &mut o)?;
                }
            }
            o.extend([flag(s.tflg), s.tc, s.deg as f64, f64::from(s.dir), s.e]);
        }
        Inputs::Fill(f) => {
            let k = f.curves.len().checked_sub(1).ok_or_else(|| harness("no curves"))?;
            o.push(k as f64);
            for c in &f.curves {
                encode_crv(&resolve_crv(c)?, &mut o)?;
            }
            o.push(flag(f.der.is_some()));
            if let Some(der) = &f.der {
                for j in 0..=k {
                    let d = der_entry(der, j);
                    o.push(flag(d.is_some()));
                    if let Some(d) = d {
                        encode_crv(&resolve_crv(d)?, &mut o)?;
                    }
                }
            }
            o.push(f.eps);
            o.push(flag(f.center.is_some()));
            let [c, n] = f.center.unwrap_or([[0.0; 3]; 2]);
            o.extend_from_slice(&c);
            o.extend_from_slice(&n);
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    let r = match i {
        Inputs::Sweep(_) => ffi::sweep(&input),
        Inputs::Fill(_) => ffi::fill(&input),
    };
    match r {
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

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::Sweep(s) => {
            let traj = build_crv(&resolve_crv(&s.traj)?)?;
            let sect = build_crv(&resolve_crv(&s.sect)?)?;
            let cz = s.cur_z.as_ref().map(|c| build_crv(&resolve_crv(c)?)).transpose()?;
            let cs = s.cur_s.as_ref().map(|c| build_crv(&resolve_crv(c)?)).transpose()?;
            let opt = adv::SweepScaleOptions {
                cur_z: cz.as_ref(),
                cur_s: cs.as_ref(),
                tflg: s.tflg,
                tc: s.tc,
                deg: s.deg,
                dir: Direction::from_flag(s.dir).ok_or_else(|| harness("bad direction flag"))?,
                e: s.e,
            };
            let r = adv::create_sweep_scale(&traj, &sect, v3(s.z0), &opt);
            o.push(status(&r));
            if let Ok(sur) = r {
                dump(&mut o, &sur);
            }
        }
        Inputs::Fill(f) => {
            let mut cur = f
                .curves
                .iter()
                .map(|c| build_crv(&resolve_crv(c)?))
                .collect::<Result<Vec<_>, _>>()?;
            let mut der: Option<Vec<Option<Curve>>> = match &f.der {
                Some(d) => Some(
                    (0..f.curves.len())
                        .map(|j| der_entry(d, j).map(|c| build_crv(&resolve_crv(c)?)).transpose())
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None => None,
            };
            let center = f.center.map(|[c, n]| (v3(c), v3(n)));
            let r = adv::fill_n_sided_hole(&mut cur, der.as_deref_mut(), f.eps, center);
            o.push(status(&r));
            if let Ok(fl) = r {
                for s in &fl.surfaces {
                    dump(&mut o, s);
                }
                o.extend([fl.center.x, fl.center.y, fl.center.z]);
                o.extend([fl.normal.x, fl.normal.y, fl.normal.z]);
                for c in &cur {
                    dump_crv(&mut o, c);
                }
                if let Some(der) = &der {
                    for d in der {
                        o.push(flag(d.is_some()));
                        if let Some(d) = d {
                            dump_crv(&mut o, d);
                        }
                    }
                }
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation ---------------------------------------------------------------------

fn domain(rng: &mut Rng) -> [f64; 2] {
    match rng.below(4) {
        0 | 1 => [0.0, 1.0],
        2 => [rng.uniform(-3.0, 0.0), rng.uniform(0.5, 4.0)],
        _ => [0.0, rng.uniform(0.2, 10.0)],
    }
}

/// A clamped knot vector for `n + 1` control points of degree `p` over `[a, b]`,
/// uniform or with random interior knots.
fn knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    if rng.chance(0.5) {
        return uniform_knots(n, p, a, b);
    }
    let interior = n - p;
    let mut mid: Vec<f64> = (0..interior)
        .map(|_| a + (b - a) * rng.uniform(0.05, 0.95))
        .collect();
    mid.sort_by(f64::total_cmp);
    // Keep the interior knots distinct.
    let mut last = f64::NEG_INFINITY;
    for x in &mut mid {
        if *x <= last + (b - a) * 1e-3 {
            *x = last + (b - a) * 1e-3;
        }
        last = *x;
    }
    if mid.last().is_some_and(|&x| x >= b) {
        return uniform_knots(n, p, a, b);
    }
    let mut u = vec![a; p + 1];
    u.extend(mid);
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn weight(rng: &mut Rng) -> f64 {
    if rng.chance(0.3) {
        1.0
    } else {
        rng.uniform(0.5, 2.0)
    }
}

fn hom(q: [f64; 3], w: Option<f64>) -> [f64; 4] {
    match w {
        Some(w) => [q[0] * w, q[1] * w, q[2] * w, w],
        None => [q[0], q[1], q[2], NOW],
    }
}

fn inline(rng: &mut Rng, pts: &[[f64; 3]], p: usize, dom: [f64; 2], rational: bool) -> InlineCrv {
    let n = pts.len() - 1;
    let u = knots(rng, n, p, dom[0], dom[1]);
    let cpts = pts
        .iter()
        .map(|&q| {
            let w = rational.then(|| weight(rng));
            hom(q, w)
        })
        .collect();
    InlineCrv { cpts, p, u }
}

/// A trajectory: mostly along x, sometimes with straight stretches.
fn gen_traj(rng: &mut Rng) -> (InlineCrv, usize) {
    let pt = match rng.below(10) {
        0 | 1 => 1,
        2..=4 => 2,
        _ => 3,
    };
    let dom = domain(rng);
    if pt == 1 {
        let dir = [1.0, rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5)];
        let len = rng.uniform(2.0, 8.0);
        let n = 1 + rng.below(3) as usize;
        let mut ts: Vec<f64> = (0..=n).map(|j| j as f64 / n as f64).collect();
        if n >= 2 && rng.chance(0.1) {
            // A bent polyline (upstream rejects it).
            let pts: Vec<[f64; 3]> = ts
                .iter()
                .enumerate()
                .map(|(j, &t)| [len * t, if j % 2 == 1 { 1.0 } else { 0.0 }, 0.0])
                .collect();
            return (inline(rng, &pts, 1, dom, false), 1);
        }
        if let Some(t) = ts.get_mut(1) {
            if n > 1 {
                *t = rng.uniform(0.2, 0.6);
            }
        }
        let pts: Vec<[f64; 3]> = ts
            .iter()
            .map(|&t| [dir[0] * len * t, dir[1] * len * t, dir[2] * len * t])
            .collect();
        return (inline(rng, &pts, 1, dom, false), 1);
    }
    let n = pt + rng.below(4) as usize;
    let mut pts: Vec<[f64; 3]> = (0..=n)
        .map(|j| {
            [
                1.5 * j as f64 + rng.uniform(-0.3, 0.3),
                rng.uniform(-1.0, 1.0),
                rng.uniform(-1.0, 1.0),
            ]
        })
        .collect();
    if rng.chance(0.3) && n > pt {
        // Collinear leading control points: a linear first span.
        for j in 1..=pt {
            if let Some(q) = pts.get_mut(j) {
                *q = [1.5 * j as f64, 0.0, 0.0];
            }
        }
        if let Some(q) = pts.first_mut() {
            *q = [0.0, 0.0, 0.0];
        }
    }
    if rng.chance(0.2) && n > pt {
        // Collinear trailing control points.
        let base = n - pt;
        let b = pts.get(base).copied().unwrap_or([0.0; 3]);
        for j in base..=n {
            if let Some(q) = pts.get_mut(j) {
                *q = [b[0] + 1.5 * (j - base) as f64, b[1], b[2]];
            }
        }
    }
    let rational = rng.chance(0.2);
    let mut c = inline(rng, &pts, pt, dom, rational);
    if rng.chance(0.1) {
        // All weights one but stored as a rational curve.
        for q in &mut c.cpts {
            let w = if q[3] == NOW { 1.0 } else { q[3] };
            *q = [q[0] / w, q[1] / w, q[2] / w, 1.0];
        }
    }
    (c, pt)
}

/// A cross section in the yz plane (the plane the sweep maps onto the normal plane).
fn gen_sect(rng: &mut Rng) -> InlineCrv {
    if rng.chance(0.15) {
        let r = rng.uniform(0.3, 1.5);
        let c = [0.0, rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5)];
        if let Ok(c) = circle(r, "yz", c) {
            return c;
        }
    }
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let pts: Vec<[f64; 3]> = (0..=n)
        .map(|j| {
            [
                if rng.chance(0.2) { rng.uniform(-0.2, 0.2) } else { 0.0 },
                -1.0 + 2.0 * j as f64 / n as f64 + rng.uniform(-0.2, 0.2),
                rng.uniform(-0.8, 0.8),
            ]
        })
        .collect();
    let dom = domain(rng);
    let rational = rng.chance(0.25);
    let mut c = inline(rng, &pts, p, dom, rational);
    if rng.chance(0.05) {
        // A 2D section (in the xy plane, made 3D upstream).
        for q in &mut c.cpts {
            *q = [q[1], q[2], NOZ, q[3]];
        }
    }
    c
}

/// Evaluates the Euclidean point of an inline curve (through the Rust port).
fn eval_pt(c: &InlineCrv, t: f64) -> Option<[f64; 3]> {
    let cc = build_crv(c).ok()?;
    let p = cc.eval(t, solidcore::nurbs::knots::Side::Left).ok()?;
    Some([p.x, p.y, p.z])
}

fn translate(c: &mut InlineCrv, d: [f64; 3]) {
    for q in &mut c.cpts {
        let w = if q[3] == NOW { 1.0 } else { q[3] };
        q[0] -= d[0] * w;
        q[1] -= d[1] * w;
        if q[2] != NOZ {
            q[2] -= d[2] * w;
        }
    }
}

fn gen_sweep(rng: &mut Rng) -> Inputs {
    let (traj, pt) = gen_traj(rng);
    let mut sect = gen_sect(rng);
    let tflg = rng.chance(0.4);
    let (us, ue) = (
        sect.u.first().copied().unwrap_or(0.0),
        sect.u.last().copied().unwrap_or(1.0),
    );
    let mut tc = 0.0;
    if tflg {
        tc = match rng.below(6) {
            0 => us,
            1 => ue,
            2 => us - 0.5,
            _ => us + (ue - us) * rng.uniform(0.15, 0.85),
        };
        // Make the section pass through the origin at tc (usually).
        if rng.chance(0.9) {
            let t = tc.clamp(us, ue);
            if let Some(p) = eval_pt(&sect, t) {
                translate(&mut sect, p);
            }
        }
    }
    let dom = [
        traj.u.first().copied().unwrap_or(0.0),
        traj.u.last().copied().unwrap_or(1.0),
    ];
    let z0 = match rng.below(5) {
        0 => [0.0, 1.0, 0.0],
        1 => [rng.uniform(-0.3, 0.3), rng.uniform(-1.0, 1.0), rng.uniform(0.5, 1.0)],
        _ => [0.0, 0.0, 1.0],
    };
    let cur_z = if rng.chance(0.2) {
        let p = 1 + rng.below(2) as usize;
        let n = p + rng.below(2) as usize;
        let pts: Vec<[f64; 3]> = (0..=n)
            .map(|_| [rng.uniform(-0.2, 0.2), rng.uniform(-0.4, 0.4), 1.0])
            .collect();
        let d = if rng.chance(0.05) { [dom[0], dom[1] + 0.5] } else { dom };
        Some(CrvIn::Inline(inline(rng, &pts, p, d, false)))
    } else {
        None
    };
    let cur_s = if rng.chance(0.3) {
        let p = 1 + rng.below(3) as usize;
        let n = p + rng.below(3) as usize;
        let lin = rng.chance(0.3);
        let pts: Vec<[f64; 3]> = (0..=n)
            .map(|j| {
                if lin {
                    let s = 1.0 + 0.5 * j as f64 / n as f64;
                    [1.0, s, s]
                } else {
                    [1.0, rng.uniform(0.5, 1.5), rng.uniform(0.5, 1.5)]
                }
            })
            .collect();
        let d = if rng.chance(0.05) { [dom[0], dom[1] * 0.5] } else { dom };
        Some(CrvIn::Inline(inline(rng, &pts, p, d, false)))
    } else {
        None
    };
    let deg = if tflg && rng.chance(0.8) {
        (pt + rng.below(2) as usize).clamp(1, 3)
    } else {
        1 + rng.below(3) as usize
    };
    Inputs::Sweep(SweepIn {
        traj: CrvIn::Inline(traj),
        sect: CrvIn::Inline(sect),
        z0,
        cur_z,
        cur_s,
        tflg,
        tc,
        deg,
        dir: 1 + rng.below(2) as i32,
        e: match rng.below(4) {
            0 => rng.uniform(0.01, 0.05),
            _ => rng.uniform(0.05, 0.3),
        },
    })
}

/// Boundary curves around a polygon of `k + 1` corners on a bumpy dome.
fn gen_fill(rng: &mut Rng) -> Inputs {
    let k = 2 + rng.below(4) as usize;
    let r = rng.uniform(2.0, 5.0);
    let flat = rng.chance(0.15);
    let corners: Vec<[f64; 3]> = (0..=k)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64 + rng.uniform(-0.15, 0.15)) / (k + 1) as f64;
            let rr = r * rng.uniform(0.85, 1.15);
            let z = if flat { 0.0 } else { rng.uniform(-0.5, 0.5) };
            [rr * a.cos(), rr * a.sin(), z]
        })
        .collect();
    let two_d = flat && rng.chance(0.4);
    let mut curves: Vec<InlineCrv> = Vec::with_capacity(k + 1);
    for i in 0..=k {
        let a = corners.get(i).copied().unwrap_or_default();
        let b = corners.get((i + 1) % (k + 1)).copied().unwrap_or_default();
        let p = 1 + rng.below(3) as usize;
        let n = p + rng.below(3) as usize;
        let bulge = if flat { 0.0 } else { rng.uniform(-0.6, 0.8) };
        let pts: Vec<[f64; 3]> = (0..=n)
            .map(|j| {
                let t = j as f64 / n as f64;
                let s = (std::f64::consts::PI * t).sin();
                let out = rng.uniform(-0.3, 0.3) * s;
                let mx = a[0] + (b[0] - a[0]) * t;
                let my = a[1] + (b[1] - a[1]) * t;
                let nrm = (mx * mx + my * my).sqrt().max(1e-9);
                [
                    mx + out * mx / nrm,
                    my + out * my / nrm,
                    a[2] + (b[2] - a[2]) * t + bulge * s,
                ]
            })
            .collect();
        let dom = domain(rng);
        let mut c = inline(rng, &pts, p, dom, false);
        if two_d {
            for q in &mut c.cpts {
                q[2] = NOZ;
            }
        }
        curves.push(c);
    }
    if rng.chance(0.03) {
        // A gap between two boundaries.
        if let Some(q) = curves.get_mut(1).and_then(|c| c.cpts.first_mut()) {
            q[0] += 0.1;
        }
    }
    if rng.chance(0.02) {
        // Rational input (upstream rejects it).
        if let Some(c) = curves.get_mut(0) {
            for q in &mut c.cpts {
                if q[2] == NOZ {
                    q[2] = 0.0;
                }
                q[3] = 1.0;
            }
        }
    }
    let der = if rng.chance(0.5) {
        let ds = curves
            .iter()
            .map(|c| {
                if rng.chance(0.3) {
                    return None;
                }
                let mut d = c.clone();
                let up = rng.uniform(0.2, 1.5);
                let inward = rng.uniform(0.2, 1.0);
                for q in &mut d.cpts {
                    let z = if q[2] == NOZ { 0.0 } else { q[2] };
                    let nrm = (q[0] * q[0] + q[1] * q[1]).sqrt().max(1e-9);
                    *q = [
                        -inward * q[0] / nrm + rng.uniform(-0.1, 0.1),
                        -inward * q[1] / nrm + rng.uniform(-0.1, 0.1),
                        up + 0.0 * z,
                        NOW,
                    ];
                }
                if rng.chance(0.25) {
                    // A different representation (made compatible upstream).
                    let n = d.cpts.len() - 1;
                    let p = d.p;
                    let a = d.u.first().copied().unwrap_or(0.0);
                    let b = d.u.last().copied().unwrap_or(1.0);
                    let (a, b) = if rng.chance(0.5) { (0.0, 1.0) } else { (a, b) };
                    d.u = knots(rng, n, p, a, b);
                }
                Some(CrvIn::Inline(d))
            })
            .collect();
        Some(ds)
    } else {
        None
    };
    let center = if rng.chance(0.3) {
        let cx = corners.iter().map(|c| c[0]).sum::<f64>() / (k + 1) as f64;
        let cy = corners.iter().map(|c| c[1]).sum::<f64>() / (k + 1) as f64;
        let cz = rng.uniform(0.0, 1.5);
        let n = [rng.uniform(-0.2, 0.2), rng.uniform(-0.2, 0.2), 1.0];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        Some([[cx, cy, cz], [n[0] / l, n[1] / l, n[2] / l]])
    } else {
        None
    };
    Inputs::Fill(FillIn {
        curves: curves.into_iter().map(CrvIn::Inline).collect(),
        der,
        eps: match rng.below(3) {
            0 => rng.uniform(0.05, 0.5),
            _ => rng.uniform(0.5, 5.0),
        },
        center,
    })
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.5) {
        gen_sweep(rng)
    } else {
        gen_fill(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_adv_p6"
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

    /// The generator reaches successes and failures of both operations, and the
    /// branches of the sweep (split section, several trajectory pieces, rational
    /// trajectory interpolation, scaling and z curves) and of the fill (given
    /// derivatives, given center) succeed.
    #[test]
    fn generated_cases_cover_outcomes() {
        // [sweep ok, sweep err, fill ok, fill err, split ok, pieces ok, rational ok,
        //  scale ok, z curve ok, fill der ok, fill center ok]
        let mut stats = [0usize; 11];
        for seed in 0..1000u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            let ok = flat.first() == Some(&0.0);
            match &i {
                Inputs::Sweep(sw) => {
                    stats[usize::from(!ok)] += 1;
                    if !ok {
                        continue;
                    }
                    let sect = resolve_crv(&sw.sect).unwrap();
                    let (us, ue) = (sect.u[0], *sect.u.last().unwrap());
                    if sw.tflg && sw.tc > us && sw.tc < ue {
                        stats[4] += 1;
                    }
                    let traj = build_crv(&resolve_crv(&sw.traj).unwrap()).unwrap();
                    if traj.degree() > 1 && traj.linear_segs(1e-7).unwrap().len() > 1 {
                        stats[5] += 1;
                    }
                    if sw.tflg && traj.is_rational() && traj.cpts().iter().any(|p| p.w != 1.0) {
                        stats[6] += 1;
                    }
                    if sw.cur_s.is_some() {
                        stats[7] += 1;
                    }
                    if sw.cur_z.is_some() {
                        stats[8] += 1;
                    }
                }
                Inputs::Fill(f) => {
                    stats[2 + usize::from(!ok)] += 1;
                    if ok && f.der.is_some() {
                        stats[9] += 1;
                    }
                    if ok && f.center.is_some() {
                        stats[10] += 1;
                    }
                }
            }
        }
        eprintln!("{stats:?}");
        assert!(stats.iter().all(|&k| k >= 5), "{stats:?}");
    }
}
