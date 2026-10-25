//! Runner for unit `nlib.srf_adv_p5`: skinned surfaces with cross-boundary
//! continuity and rails (`N_CreateSkinSrfBoundaryContinuity`).
//!
//! C++: `solidcore_ffi::units::nlib_srf_adv_p5` (upstream
//! `source/SMLib/src/NL_SrfAdv.cpp`). Rust: `solidcore::nurbs::srf_adv::p5`.
//!
//! Both sides produce the same flat record of doubles, split into keys by
//! [`to_output`]: `status` (`0` ok, else the `NL_*_ERR` number; `-1` on the Rust side
//! for a non-NLib error, which never matches), then on success the surface (`dims =
//! [n, m, p, q, r, s]`, `net` x y z w row-major, `u`, `v`), every cross section after
//! the call (`c<i>dims = [n, p, m]`, `c<i>net`, `c<i>u`), the derivative surfaces
//! (`d<i>has`, then `d<i>...`) and the rails (`r<i>has`, then `r<i>...`) after the
//! call, since upstream modifies all of them in place.
//!
//! Curves are inline or upstream `.smc` files (`{"smc": ..., "shift": [x, y, z]}`,
//! translated after reading); derivative surfaces are inline or a two-row strip built
//! from a curve (`{"strip": curve, "vec": [x, y, z], "w": weight, "bnd": flag}`, see
//! [`strip`]). Inputs must be valid clamped surfaces and curves (the FFI wrapper rejects
//! anything else). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::{Boundary, Curve};
use solidcore::nurbs::crv_fit::p1::interp_knot_vector;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_adv::p5 as adv;
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_adv_p5 as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `nlib.srf_adv_p5` runner.
pub struct Runner;

/// An inline surface (storage = indices in use).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// An upstream `.smc` file (absolute path after fixture expansion), translated.
    File {
        smc: String,
        #[serde(default)]
        shift: Option<[f64; 3]>,
    },
    /// Inline data.
    Inline(InlineCrv),
}

/// A derivative surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// A two-row strip along a curve (see [`strip`]).
    Strip {
        strip: CrvIn,
        vec: [f64; 3],
        #[serde(default)]
        w: Option<f64>,
        bnd: i32,
    },
    /// Inline data.
    Inline(InlineSrf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    curves: Vec<CrvIn>,
    #[serde(default)]
    der: Option<[Option<SrfIn>; 2]>,
    cmp: bool,
    cnt: [i32; 2],
    bnd: [i32; 2],
    #[serde(default)]
    rail: Option<[Option<CrvIn>; 2]>,
    deg: usize,
    #[serde(default)]
    es: f64,
    dir: i32,
    #[serde(default)]
    ts: Option<Vec<f64>>,
    #[serde(default)]
    kns: Option<Vec<f64>>,
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

fn read_smc(path: &str) -> Result<InlineCrv, HarnessError> {
    let c = crate::fixtures::text_curve::read(path)?;
    Ok(InlineCrv {
        cpts: c.cpts,
        p: c.p,
        u: c.u,
    })
}

/// Translates a curve's Euclidean points by `t` (a 2D curve becomes 3D).
fn shifted(mut c: InlineCrv, t: [f64; 3]) -> InlineCrv {
    for pt in &mut c.cpts {
        let w = if pt[3] == NOW { 1.0 } else { pt[3] };
        let z = if pt[2] == NOZ { 0.0 } else { pt[2] };
        pt[0] += t[0] * w;
        pt[1] += t[1] * w;
        pt[2] = z + t[2] * w;
    }
    c
}

fn resolve_crv(c: &CrvIn) -> Result<InlineCrv, HarnessError> {
    match c {
        CrvIn::File { smc, shift } => {
            let c = read_smc(smc)?;
            Ok(match shift {
                Some(t) => shifted(c, *t),
                None => c,
            })
        }
        CrvIn::Inline(c) => Ok(c.clone()),
    }
}

/// A Bezier strip of degree 1 across a curve `c` for boundary `bnd`: the curve's
/// points `C` and `C + w*vec` (homogeneous offset, `w` the point's weight, or `wt`
/// for a non-rational curve given a weight), as two rows (`NL_LEFT`: `[C, C+d]`,
/// `NL_RIGHT`: `[C-d, C]`) or two columns (`NL_BOTTOM`: `[C, C+d]`, `NL_TOP`:
/// `[C-d, C]`).
fn strip(c: &InlineCrv, d: [f64; 3], wt: Option<f64>, bnd: i32) -> InlineSrf {
    let pts: Vec<[f64; 4]> = c
        .cpts
        .iter()
        .map(|pt| {
            let z = if pt[2] == NOZ { 0.0 } else { pt[2] };
            match (pt[3] == NOW, wt) {
                (true, Some(w)) => [pt[0] * w, pt[1] * w, z * w, w],
                _ => [pt[0], pt[1], z, pt[3]],
            }
        })
        .collect();
    let off = |pt: &[f64; 4], s: f64| -> [f64; 4] {
        let w = if pt[3] == NOW { 1.0 } else { pt[3] };
        [
            pt[0] + s * d[0] * w,
            pt[1] + s * d[1] * w,
            pt[2] + s * d[2] * w,
            pt[3],
        ]
    };
    let (a, b): (Vec<[f64; 4]>, Vec<[f64; 4]>) = if bnd == 1 || bnd == 3 {
        (pts.clone(), pts.iter().map(|p| off(p, 1.0)).collect())
    } else {
        (pts.iter().map(|p| off(p, -1.0)).collect(), pts.clone())
    };
    let lin = vec![0.0, 0.0, 1.0, 1.0];
    if bnd <= 2 {
        InlineSrf {
            net: vec![a, b],
            p: 1,
            q: c.p,
            u: lin,
            v: c.u.clone(),
        }
    } else {
        InlineSrf {
            net: a.iter().zip(&b).map(|(x, y)| vec![*x, *y]).collect(),
            p: c.p,
            q: 1,
            u: c.u.clone(),
            v: lin,
        }
    }
}

fn resolve(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::Strip {
            strip: c,
            vec,
            w,
            bnd,
        } => Ok(strip(&resolve_crv(c)?, *vec, *w, *bnd)),
        SrfIn::Inline(i) => Ok(i.clone()),
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
    out = out.values(
        &format!("{pfx}net"),
        take(flat, at, 4 * (n + 1) * (m + 1))?.to_vec(),
    );
    out = out.values(&format!("{pfx}u"), take(flat, at, r + 1)?.to_vec());
    Ok(out.values(&format!("{pfx}v"), take(flat, at, s + 1)?.to_vec()))
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

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        out = split_srf(flat, &mut at, "", out)?;
        for c in 0..i.curves.len() {
            out = split_crv(flat, &mut at, &format!("c{c}"), out)?;
        }
        if i.der.is_some() {
            for d in 0..2 {
                let has = take(flat, &mut at, 1)?.to_vec();
                let present = has.first() == Some(&1.0);
                out = out.values(&format!("d{d}has"), has);
                if present {
                    out = split_srf(flat, &mut at, &format!("d{d}"), out)?;
                }
            }
        }
        if i.rail.is_some() {
            for r in 0..2 {
                let has = take(flat, &mut at, 1)?.to_vec();
                let present = has.first() == Some(&1.0);
                out = out.values(&format!("r{r}has"), has);
                if present {
                    out = split_crv(flat, &mut at, &format!("r{r}"), out)?;
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

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let k = i
        .curves
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("no curves"))?;
    o.push(k as f64);
    for c in &i.curves {
        encode_crv(&resolve_crv(c)?, &mut o)?;
    }
    o.push(flag(i.der.is_some()));
    if let Some(der) = &i.der {
        for d in der {
            o.push(flag(d.is_some()));
            if let Some(d) = d {
                encode(&resolve(d)?, &mut o)?;
            }
        }
    }
    o.push(flag(i.cmp));
    o.extend(i.cnt.map(f64::from));
    o.extend(i.bnd.map(f64::from));
    o.push(flag(i.rail.is_some()));
    if let Some(rail) = &i.rail {
        for r in rail {
            o.push(flag(r.is_some()));
            if let Some(r) = r {
                encode_crv(&resolve_crv(r)?, &mut o)?;
            }
        }
    }
    o.extend([i.deg as f64, i.es, f64::from(i.dir)]);
    o.push(flag(i.ts.is_some()));
    if let Some(ts) = &i.ts {
        o.extend_from_slice(ts);
    }
    o.push(flag(i.kns.is_some()));
    if let Some(kns) = &i.kns {
        o.push(kns.len() as f64);
        o.extend_from_slice(kns);
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    match ffi::skin(&input) {
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

fn boundary(fl: i32) -> Result<Boundary, HarnessError> {
    Boundary::from_flag(fl).ok_or_else(|| harness(format!("bad boundary flag {fl}")))
}

fn options(i: &Inputs) -> Result<adv::SkinBoundaryOptions, HarnessError> {
    let der = match &i.der {
        Some([a, b]) => {
            let mk = |d: &Option<SrfIn>| -> Result<Option<Surface>, HarnessError> {
                d.as_ref().map(|d| build(&resolve(d)?)).transpose()
            };
            Some([mk(a)?, mk(b)?])
        }
        None => None,
    };
    let rail = match &i.rail {
        Some([a, b]) => {
            let mk = |r: &Option<CrvIn>| -> Result<Option<Curve>, HarnessError> {
                r.as_ref().map(|r| build_crv(&resolve_crv(r)?)).transpose()
            };
            Some([mk(a)?, mk(b)?])
        }
        None => None,
    };
    Ok(adv::SkinBoundaryOptions {
        der,
        cmp: i.cmp,
        cnt: i.cnt.map(adv::BoundaryContinuity::from_flag),
        bnd: [boundary(i.bnd[0])?, boundary(i.bnd[1])?],
        rail,
        deg: i.deg,
        es: i.es,
        dir: Direction::from_flag(i.dir).ok_or_else(|| harness("bad direction flag"))?,
        ts: i.ts.clone(),
        kns: i.kns.clone(),
    })
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    let mut cs = i
        .curves
        .iter()
        .map(|c| build_crv(&resolve_crv(c)?))
        .collect::<Result<Vec<_>, _>>()?;
    let mut opt = options(i)?;
    let r = adv::create_skin_srf_boundary_continuity(&mut cs, &mut opt);
    o.push(status(&r));
    if let Ok(s) = r {
        dump(&mut o, &s);
        for c in &cs {
            dump_crv(&mut o, c);
        }
        if let Some(der) = &opt.der {
            for d in der {
                o.push(if d.is_some() { 1.0 } else { 0.0 });
                if let Some(d) = d {
                    dump(&mut o, d);
                }
            }
        }
        if let Some(rail) = &opt.rail {
            for r in rail {
                o.push(if r.is_some() { 1.0 } else { 0.0 });
                if let Some(r) = r {
                    dump_crv(&mut o, r);
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

/// A valid clamped knot vector for degree `p` with `cnt` knots over `[a, b]`.
fn knots(rng: &mut Rng, cnt: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = cnt.saturating_sub(2 * p + 2);
    let grid = rng.chance(0.4);
    let steps = 2 + rng.below(4);
    let mut mid: Vec<f64> = Vec::with_capacity(interior);
    let mut guard = 0;
    while mid.len() < interior && guard < 10_000 {
        guard += 1;
        let x = if grid {
            a + (b - a) * (1 + rng.below(steps)) as f64 / (steps + 1) as f64
        } else {
            rng.uniform(a + (b - a) * 1e-3, b - (b - a) * 1e-3)
        };
        let mult = mid.iter().filter(|&&y| y == x).count();
        if x > a && x < b && mult < p {
            mid.push(x);
        }
    }
    while mid.len() < interior {
        mid.push(a + (b - a) * (mid.len() + 1) as f64 / (interior + 1) as f64 * 0.999_7);
    }
    mid.sort_by(f64::total_cmp);
    let mut k = vec![a; p + 1];
    k.extend(mid);
    k.extend(std::iter::repeat_n(b, p + 1));
    k
}

fn weight(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0..=2 => 1.0,
        3 => rng.uniform(0.2, 0.5),
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

/// Homogeneous `[x*w, y*w, z*w, w]` or `[x, y, z, NOW]`.
fn hom(x: f64, y: f64, z: f64, w: Option<f64>) -> [f64; 4] {
    match w {
        Some(w) => [x * w, y * w, z * w, w],
        None => [x, y, z, NOW],
    }
}

/// A cross section: a perturbed planar polyline-like curve at height `z`.
fn section(
    rng: &mut Rng,
    p: usize,
    n: usize,
    u: &[f64],
    z: f64,
    rational: bool,
    two_d: bool,
) -> InlineCrv {
    let cpts = (0..=n)
        .map(|j| {
            let x = j as f64 + rng.uniform(-0.3, 0.3);
            let y = rng.uniform(-1.0, 1.0);
            let zz = if two_d {
                NOZ
            } else {
                z + rng.uniform(-0.3, 0.3)
            };
            let w = rational.then(|| weight(rng));
            match w {
                Some(w) if zz == NOZ => [x * w, y * w, NOZ, w],
                _ => hom(x, y, zz, w),
            }
        })
        .collect();
    InlineCrv {
        cpts,
        p,
        u: u.to_vec(),
    }
}

/// Free cross sections (no rails): compatible or not, sometimes with derivative
/// strips, parameters and a knot vector.
fn gen_free(rng: &mut Rng) -> Inputs {
    let k = 1 + rng.below(5) as usize;
    let cmp = rng.chance(0.5);
    let rational = rng.chance(0.3);
    let two_d = rng.chance(0.03);
    let p0 = 1 + rng.below(3) as usize;
    let n0 = p0 + rng.below(4) as usize;
    let (a, b) = domain(rng);
    let u0 = knots(rng, n0 + p0 + 2, p0, a, b);
    let curves: Vec<InlineCrv> = (0..=k)
        .map(|i| {
            let z = 3.0 * i as f64 + rng.uniform(-0.5, 0.5);
            if cmp || rng.chance(0.3) {
                section(rng, p0, n0, &u0, z, rational, two_d)
            } else {
                let p = 1 + rng.below(3) as usize;
                let n = p + rng.below(4) as usize;
                let (a, b) = domain(rng);
                let u = knots(rng, n + p + 2, p, a, b);
                let rat = rational && rng.chance(0.7);
                section(rng, p, n, &u, z, rat, two_d)
            }
        })
        .collect();
    let dir = if rng.chance(0.05) {
        3
    } else {
        1 + rng.below(2) as i32
    };
    let mut der = None;
    let mut bnd = [2, 1];
    if dir != 3 && rng.chance(0.6) {
        // Targets: U -> [RIGHT, LEFT], V -> [TOP, BOTTOM]; sometimes the opposite or
        // the transposed boundary (upstream then re-expresses the strip).
        let target = if dir == 1 { [2, 1] } else { [4, 3] };
        let pick = |rng: &mut Rng, t: i32| -> i32 {
            match rng.below(6) {
                0 => 1 + rng.below(4) as i32,
                _ => t,
            }
        };
        bnd = [pick(rng, target[0]), pick(rng, target[1])];
        let mk = |rng: &mut Rng, c: &InlineCrv, b: i32| -> Option<SrfIn> {
            if rng.chance(0.2) {
                return None;
            }
            let s = if rng.chance(0.15) {
                0.05
            } else {
                rng.uniform(0.3, 3.0)
            };
            let d = [rng.uniform(-0.5, 0.5) * s, rng.uniform(-0.5, 0.5) * s, s];
            let w = (rational && rng.chance(0.3)).then(|| weight(rng));
            Some(SrfIn::Strip {
                strip: CrvIn::Inline(c.clone()),
                vec: d,
                w,
                bnd: b,
            })
        };
        let first = curves.first().cloned();
        let last = curves.last().cloned();
        let d0 = first.and_then(|c| mk(rng, &c, bnd[0]));
        let d1 = last.and_then(|c| mk(rng, &c, bnd[1]));
        der = Some([d0, d1]);
    }
    let deg = 1 + rng.below(3) as usize;
    let ts = if rng.chance(0.3) {
        let mut t: Vec<f64> = (0..=k).map(|_| rng.uniform(0.0, 1.0)).collect();
        t.sort_by(f64::total_cmp);
        if let Some(x) = t.first_mut() {
            *x = 0.0;
        }
        if let Some(x) = t.last_mut() {
            *x = 1.0;
        }
        if t.windows(2).all(|w| w.first() < w.get(1)) {
            Some(t)
        } else {
            None
        }
    } else {
        None
    };
    let dfl = der
        .as_ref()
        .map_or(0, |d: &[Option<SrfIn>; 2]| d.iter().flatten().count());
    let kns = match (&ts, rng.chance(0.3)) {
        (Some(t), true) if dfl == 0 && deg <= k => interp_knot_vector(t, deg).ok(),
        _ => None,
    };
    let es = match rng.below(6) {
        0 => 1e-3,
        1 => 0.1,
        _ => 0.0,
    };
    Inputs {
        curves: curves.into_iter().map(CrvIn::Inline).collect(),
        der,
        cmp,
        cnt: [1 + rng.below(2) as i32, 1 + rng.below(2) as i32],
        bnd,
        rail: None,
        deg,
        es,
        dir,
        ts,
        kns,
    }
}

/// Cross sections, rails and derivative strips cut from one surface: the sections are
/// iso-curves at `ts`, the rails the boundary curves, the strips offsets along the
/// homogeneous derivative across the end sections (C1 with `h = 1`).
fn gen_railed(rng: &mut Rng) -> Option<Inputs> {
    let pr = 1 + rng.below(3) as usize;
    let nr = pr + rng.below(4) as usize;
    let pc = 1 + rng.below(3) as usize;
    let nc = pc + rng.below(3) as usize;
    let rational = rng.chance(0.4);
    let (a, b) = domain(rng);
    let ur = knots(rng, nr + pr + 2, pr, a, b);
    let uc = knots(rng, nc + pc + 2, pc, 0.0, 1.0);
    // Columns j: curves over ur (the skinning direction).
    let cols: Vec<Curve> = (0..=nc)
        .map(|j| {
            let pts = (0..=nr)
                .map(|i| {
                    let w = rational.then(|| weight(rng));
                    let p = hom(
                        j as f64 + rng.uniform(-0.2, 0.2),
                        rng.uniform(-0.5, 0.5),
                        2.0 * i as f64 + rng.uniform(-0.3, 0.3),
                        w,
                    );
                    CPoint::new(p[0], p[1], p[2], p[3])
                })
                .collect();
            Curve::new(pts, pr, ur.clone())
        })
        .collect::<SolidResult<Vec<_>>>()
        .ok()?;
    let k = pr + rng.below(4) as usize;
    let mut ts: Vec<f64> = (0..=k).map(|i| a + (b - a) * i as f64 / k as f64).collect();
    if let Some(t) = ts.last_mut() {
        *t = b;
    }
    for (i, t) in ts.iter_mut().enumerate() {
        if i > 0 && i < k && rng.chance(0.5) {
            *t += (b - a) / k as f64 * rng.uniform(-0.3, 0.3);
        }
    }
    let curves: Vec<InlineCrv> = ts
        .iter()
        .map(|&t| {
            let cpts = cols
                .iter()
                .map(|c| c.eval_pt(t, Side::Left).map(CPoint::to_array))
                .collect::<SolidResult<Vec<_>>>()?;
            Ok(InlineCrv {
                cpts,
                p: pc,
                u: uc.clone(),
            })
        })
        .collect::<SolidResult<Vec<_>>>()
        .ok()?;
    let rail_of = |j: usize| -> Option<InlineCrv> {
        let c = cols.get(j)?;
        Some(InlineCrv {
            cpts: c.cpts().iter().map(|p| p.to_array()).collect(),
            p: pr,
            u: ur.clone(),
        })
    };
    let (r0, r1) = match rng.below(3) {
        0 => (rail_of(0), None),
        1 => (None, rail_of(nc)),
        _ => (rail_of(0), rail_of(nc)),
    };
    let dir = 1 + rng.below(2) as i32;
    let cnt = [1 + rng.below(2) as i32, 1 + rng.below(2) as i32];
    let mut der = None;
    let mut bnd = if dir == 1 { [2, 1] } else { [4, 3] };
    if rng.chance(0.7) {
        let mk = |end: usize, rng: &mut Rng| -> Option<SrfIn> {
            if rng.chance(0.2) {
                return None;
            }
            let t = *ts.get(if end == 0 { 0 } else { k })?;
            let h = if cnt.get(end) == Some(&1) || rng.chance(0.3) {
                1.0
            } else {
                rng.uniform(0.3, 2.0)
            };
            let sec = curves.get(if end == 0 { 0 } else { k })?;
            let rows: Vec<[f64; 4]> = cols
                .iter()
                .map(|c| {
                    c.derivs_at_knot(t, Side::Left, 1)
                        .ok()
                        .and_then(|d| d.get(1).copied())
                })
                .collect::<Option<Vec<_>>>()?
                .iter()
                .map(|d| d.to_array())
                .collect();
            let lin = vec![0.0, 0.0, 1.0, 1.0];
            let shift = |s: f64| -> Vec<[f64; 4]> {
                sec.cpts
                    .iter()
                    .zip(&rows)
                    .map(|(c, d)| {
                        let w = if c[3] == NOW {
                            NOW
                        } else {
                            c[3] + s * h * d[3]
                        };
                        [
                            c[0] + s * h * d[0],
                            c[1] + s * h * d[1],
                            c[2] + s * h * d[2],
                            w,
                        ]
                    })
                    .collect()
            };
            let (mut r_a, mut r_b) = if end == 0 {
                (shift(-1.0), sec.cpts.clone())
            } else {
                (sec.cpts.clone(), shift(1.0))
            };
            if rational && rng.chance(0.4) {
                // Rescale the off-boundary row in homogeneous space: the same Euclidean
                // strip, no longer G1 with the rails in 4-D (upstream adjusts it).
                let f = rng.uniform(0.7, 1.4);
                let off = if end == 0 { &mut r_a } else { &mut r_b };
                for c in off.iter_mut() {
                    *c = [c[0] * f, c[1] * f, c[2] * f, c[3] * f];
                }
            }
            Some(SrfIn::Inline(if dir == 1 {
                InlineSrf {
                    net: vec![r_a, r_b],
                    p: 1,
                    q: pc,
                    u: lin,
                    v: uc.clone(),
                }
            } else {
                InlineSrf {
                    net: r_a.iter().zip(&r_b).map(|(x, y)| vec![*x, *y]).collect(),
                    p: pc,
                    q: 1,
                    u: uc.clone(),
                    v: lin,
                }
            }))
        };
        let d0 = mk(0, rng);
        let d1 = mk(1, rng);
        der = Some([d0, d1]);
    }
    let mut flags_ok = true;
    if rng.chance(0.05) {
        // Wrong orientation flags: upstream re-expresses the strips first.
        bnd = [1 + rng.below(4) as i32, 1 + rng.below(4) as i32];
        flags_ok = false;
    }
    let mut ts_in = Some(ts.clone());
    match rng.below(40) {
        0 => ts_in = None,
        1 => {
            if let Some(t) = ts_in.as_mut().and_then(|t| t.get_mut(1)) {
                *t += (b - a) * 1e-3;
            }
        }
        _ => {}
    }
    let kns = if rng.chance(0.15) && der.is_none() && ts_in.is_some() {
        Some(ur.clone()).filter(|u| u.len() == k + pr + 2)
    } else {
        None
    };
    let mut curves = curves;
    if rng.chance(0.2) {
        // Shuffle the knot vectors of the sections a little (make them incompatible).
        for c in curves.iter_mut().skip(1) {
            if rng.chance(0.5) {
                let s = rng.uniform(0.5, 2.0);
                for x in &mut c.u {
                    *x *= s;
                }
            }
        }
    }
    let cmp = flags_ok
        && curves
            .windows(2)
            .all(|w| w.first().map(|c| &c.u) == w.get(1).map(|c| &c.u))
        && rng.chance(0.7);
    Some(Inputs {
        curves: curves.into_iter().map(CrvIn::Inline).collect(),
        der,
        cmp,
        cnt,
        bnd,
        rail: Some([r0.map(CrvIn::Inline), r1.map(CrvIn::Inline)]),
        deg: 1 + rng.below(3) as usize,
        es: if rng.chance(0.2) { 1e-3 } else { 0.0 },
        dir,
        ts: ts_in,
        kns,
    })
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.5) {
        if let Some(i) = gen_railed(rng) {
            return i;
        }
    }
    gen_free(rng)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_adv_p5"
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

    /// The generator reaches successes and failures with and without rails and
    /// derivative data, rational and not.
    #[test]
    fn generated_cases_cover_outcomes() {
        // [free ok, free err, free+der ok, railed ok, railed err, railed+der ok,
        //  railed+der rational ok, 2 rails + der ok]
        let mut stats = [0usize; 8];
        for seed in 0..3000u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            let ok = flat.first() == Some(&0.0);
            let has_der = i
                .der
                .as_ref()
                .is_some_and(|d| d.iter().any(Option::is_some));
            match (&i.rail, ok) {
                (None, true) => stats[0] += 1,
                (None, false) => stats[1] += 1,
                (Some(_), true) => stats[3] += 1,
                (Some(_), false) => stats[4] += 1,
            }
            if ok && has_der {
                if let Some(r) = &i.rail {
                    stats[5] += 1;
                    let rat = match i.curves.first() {
                        Some(CrvIn::Inline(c)) => c.cpts[0][3] != NOW,
                        _ => false,
                    };
                    if rat {
                        stats[6] += 1;
                    }
                    if r.iter().all(Option::is_some) {
                        stats[7] += 1;
                    }
                } else {
                    stats[2] += 1;
                }
            }
        }
        eprintln!("{stats:?}");
        assert!(stats.iter().all(|&k| k >= 10), "{stats:?}");
    }
}
