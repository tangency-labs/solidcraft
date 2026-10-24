//! Runner for unit `nlib.srf_offset`: offset of an NLib surface (`N_SrfOffset`).
//!
//! C++: `solidcore_ffi::units::nlib_srf_offset` (upstream
//! `source/SMLib/src/NL_SrfOffset.cpp`). Rust: `solidcore::nurbs::srf_offset`
//! ([`Surface::offset`]).
//!
//! Both sides produce the same flat record of doubles, split into keys: `status` (`0`
//! ok, else the `NL_*_ERR` number; `-1` on the Rust side for a non-NLib error, which
//! never matches), then on success the offset surface (`dims = [n, m, p, q, r, s]`,
//! `net` x y z w row-major, `u`, `v`).
//!
//! Surfaces are inline or upstream `.sms` files (`{"sms": {"$upstream_file": ...}}`).
//! Generated cases cover every branch of `N_SrfOffset`: planes (bilinear and flat
//! bicubic), cylinders, cones, spheres, tori, surfaces of revolution, extrusions of
//! planar curves (along the plane normal and slanted), and free-form surfaces
//! (polynomial and rational, several patches, a collapsed boundary), with every
//! parametrisation flag, both offset signs, and tolerances that push the per-patch
//! sample count over `slm` and the total over `totalmax`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_basic::{ConicType, Curve};
use solidcore::nurbs::crv_common::{circ_arc, Arc};
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::crv_offset::OffsetParam;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::{Direction, Surface};
use solidcore::nurbs::srf_common::{extrude_crv, revolved_srf};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_srf_offset as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `nlib.srf_offset` runner.
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
struct Inputs {
    sur: SrfIn,
    d: f64,
    pp: usize,
    qq: usize,
    /// `NL_UNIFORM` 1, `NL_CHORDLENGTH` 2, `NL_CENTRIPETAL` 3, `NL_INHERITED` 4.
    par: i32,
    eps: f64,
    tol: f64,
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
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
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
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
        .collect();
    Surface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("rust surface: {e}")))
}

fn to_inline(s: &Surface) -> InlineSrf {
    let (p, q) = s.degrees();
    InlineSrf {
        net: s
            .rows()
            .map(|r| r.iter().map(|c| c.to_array()).collect())
            .collect(),
        p,
        q,
        u: s.u_knots().to_vec(),
        v: s.v_knots().to_vec(),
    }
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

fn to_output(flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status.first() == Some(&0.0);
    out = out.values("status", status);
    if ok {
        let dims = take(flat, &mut at, 6)?;
        let dim = |k: usize| count(dims.get(k).copied().unwrap_or(f64::NAN));
        let (n, m, r, s) = (dim(0)?, dim(1)?, dim(4)?, dim(5)?);
        out = out.values("dims", dims.to_vec());
        out = out.values("net", take(flat, &mut at, 4 * (n + 1) * (m + 1))?.to_vec());
        out = out.values("u", take(flat, &mut at, r + 1)?.to_vec());
        out = out.values("v", take(flat, &mut at, s + 1)?.to_vec());
    }
    if at != flat.len() {
        return Err(harness(format!(
            "{} trailing values in record",
            flat.len() - at
        )));
    }
    Ok(out)
}

// ---- C++ side -----------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let mut input = Vec::new();
    encode(&resolve(&i.sur)?, &mut input)?;
    input.extend([
        i.d,
        i.pp as f64,
        i.qq as f64,
        f64::from(i.par),
        i.eps,
        i.tol,
    ]);
    match ffi::eval(&input) {
        Ok(flat) => to_output(&flat),
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

fn offset_param(par: i32) -> Result<OffsetParam, HarnessError> {
    match par {
        1 => Ok(OffsetParam::Fit(ParamMethod::Uniform)),
        2 => Ok(OffsetParam::Fit(ParamMethod::ChordLength)),
        3 => Ok(OffsetParam::Fit(ParamMethod::Centripetal)),
        4 => Ok(OffsetParam::Inherited),
        _ => Err(harness(format!("bad par flag {par}"))),
    }
}

fn rust_result(i: &Inputs) -> Result<SolidResult<Surface>, HarnessError> {
    let sur = build(&resolve(&i.sur)?)?;
    Ok(sur.offset(i.d, i.pp, i.qq, offset_param(i.par)?, i.eps, i.tol))
}

fn flatten(r: &SolidResult<Surface>) -> Vec<f64> {
    let mut o = vec![status(r)];
    if let Ok(s) = r {
        dump(&mut o, s);
    }
    o
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let r = rust_result(i)?;
    if let Err(e) = &r {
        if let Some(out) = Output::unported(e) {
            return Ok(out);
        }
    }
    to_output(&flatten(&r))
}

// ---- generation ---------------------------------------------------------------------

/// The kinds of generated surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bilinear,
    CollapsedPlane,
    LinePatch,
    FlatGrid,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Revolution,
    Extrusion,
    SlantedExtrusion,
    FreeForm,
}

const KINDS: [Kind; 12] = [
    Kind::Bilinear,
    Kind::CollapsedPlane,
    Kind::LinePatch,
    Kind::FlatGrid,
    Kind::Cylinder,
    Kind::Cone,
    Kind::Sphere,
    Kind::Torus,
    Kind::Revolution,
    Kind::Extrusion,
    Kind::SlantedExtrusion,
    Kind::FreeForm,
];

fn pt(x: f64, y: f64, z: f64) -> CPoint {
    CPoint::new(x, y, z, NOW)
}

/// A clamped knot vector of degree `p` for `n + 1` control points on `[a, b]` with
/// uniform interior knots.
fn uniform_knots(n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let mut k = vec![a; p + 1];
    let spans = n + 1 - p;
    for i in 1..spans {
        k.push(a + (b - a) * i as f64 / spans as f64);
    }
    k.extend(std::iter::repeat_n(b, p + 1));
    k
}

/// A planar (z = 0) curve in the xz plane offset in x: `n + 1` points, degree `p`.
fn profile(rng: &mut Rng, n: usize, p: usize, x0: f64) -> Option<Curve> {
    let pw = (0..=n)
        .map(|i| {
            pt(
                x0 + rng.uniform(0.0, 1.5),
                0.0,
                i as f64 + rng.uniform(-0.2, 0.2),
            )
        })
        .collect();
    Curve::new(pw, p, uniform_knots(n, p, 0.0, 1.0)).ok()
}

fn special(rng: &mut Rng, kind: Kind) -> Option<Surface> {
    let ez = Vec3::new(0.0, 0.0, 1.0);
    let o = Vec3::new(0.0, 0.0, 0.0);
    let angle = match rng.below(3) {
        0 => 360.0,
        1 => 90.0,
        _ => rng.uniform(30.0, 300.0),
    };
    match kind {
        Kind::Bilinear => {
            let net = vec![
                vec![pt(0.0, 0.0, 0.0), pt(0.0, rng.uniform(1.0, 3.0), 0.0)],
                vec![pt(rng.uniform(1.0, 3.0), 0.0, 0.0), pt(2.0, 2.0, 0.0)],
            ];
            let (a, b) = (rng.uniform(-2.0, 0.0), rng.uniform(0.5, 3.0));
            Surface::new(net, 1, 1, vec![a, a, b, b], vec![0.0, 0.0, 1.0, 1.0]).ok()
        }
        Kind::CollapsedPlane => {
            // A triangle: one boundary edge collapsed to a point.
            let mut c = [
                pt(0.0, 0.0, 0.0),
                pt(0.0, rng.uniform(1.0, 3.0), 0.0),
                pt(rng.uniform(1.0, 3.0), 0.0, 0.0),
                pt(2.0, 2.0, rng.uniform(-0.5, 0.5)),
            ];
            let k = rng.below(4) as usize;
            let (a, b) = [(0, 1), (0, 2), (1, 3), (2, 3)]
                .get(k)
                .copied()
                .unwrap_or((0, 1));
            let first = c.get(a).copied().unwrap_or(c[0]);
            if let Some(x) = c.get_mut(b) {
                *x = first;
            }
            let [c00, c01, c10, c11] = c;
            let net = vec![vec![c00, c01], vec![c10, c11]];
            Surface::new(
                net,
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .ok()
        }
        Kind::LinePatch => {
            // Every control point on one line (no plane normal).
            let t = [0.0, rng.uniform(0.5, 1.0), rng.uniform(1.0, 2.0), 3.0];
            let dir = Vec3::new(1.0, rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0));
            let at = |x: f64| pt(dir.x * x, dir.y * x, dir.z * x);
            let net = vec![vec![at(t[0]), at(t[1])], vec![at(t[2]), at(t[3])]];
            Surface::new(
                net,
                1,
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .ok()
        }
        Kind::FlatGrid => {
            let (n, m) = (3 + rng.below(3) as usize, 3 + rng.below(3) as usize);
            let net = (0..=n)
                .map(|i| {
                    (0..=m)
                        .map(|j| {
                            let (x, y) = (
                                i as f64 + rng.uniform(-0.2, 0.2),
                                j as f64 + rng.uniform(-0.2, 0.2),
                            );
                            pt(x, y, 0.5 * x - 0.25 * y)
                        })
                        .collect()
                })
                .collect();
            Surface::new(
                net,
                3,
                3,
                uniform_knots(n, 3, 0.0, 1.0),
                uniform_knots(m, 3, 0.0, 1.0),
            )
            .ok()
        }
        Kind::Cylinder | Kind::Cone => {
            let r0 = rng.uniform(0.5, 3.0);
            let r1 = if kind == Kind::Cylinder {
                r0
            } else {
                rng.uniform(0.5, 3.0)
            };
            let h = rng.uniform(0.5, 4.0);
            let line = Curve::new(
                vec![pt(r0, 0.0, 0.0), pt(r1, 0.0, h)],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .ok()?;
            let mut line = line;
            revolved_srf(&mut line, o, ez, angle, ConicType::Quadratic).ok()
        }
        Kind::Sphere | Kind::Torus => {
            let r = rng.uniform(0.5, 2.0);
            let cx = if kind == Kind::Sphere {
                0.0
            } else {
                r + rng.uniform(0.5, 2.0)
            };
            let (s, e) = if kind == Kind::Sphere {
                (-90.0, 90.0)
            } else {
                (0.0, 360.0)
            };
            let arc = Arc {
                center: Vec3::new(cx, 0.0, 0.0),
                x: Vec3::new(1.0, 0.0, 0.0),
                y: ez,
                r,
                start: s,
                end: e,
            };
            let mut prof = circ_arc(&arc, ConicType::Quadratic).ok()?;
            revolved_srf(&mut prof, o, ez, angle, ConicType::Quadratic).ok()
        }
        Kind::Revolution => {
            let p = 2 + rng.below(2) as usize;
            let n = p + 1 + rng.below(3) as usize;
            let mut prof = profile(rng, n, p, 1.0)?;
            revolved_srf(&mut prof, o, ez, angle, ConicType::Quadratic).ok()
        }
        Kind::Extrusion | Kind::SlantedExtrusion => {
            let p = 2 + rng.below(2) as usize;
            let n = p + 1 + rng.below(3) as usize;
            let pw = (0..=n)
                .map(|i| pt(i as f64, rng.uniform(-1.0, 1.0), 0.0))
                .collect();
            let mut cur = Curve::new(pw, p, uniform_knots(n, p, 0.0, 1.0)).ok()?;
            let w = if kind == Kind::Extrusion {
                ez
            } else {
                Vec3::new(rng.uniform(0.3, 1.0), rng.uniform(-1.0, 1.0), 1.0)
            };
            let dir = if rng.chance(0.5) {
                Direction::U
            } else {
                Direction::V
            };
            extrude_crv(&mut cur, w, rng.uniform(0.5, 3.0), dir).ok()
        }
        Kind::FreeForm => None,
    }
}

fn free_form(rng: &mut Rng) -> InlineSrf {
    let p = 2 + rng.below(2) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let rational = rng.chance(0.3);
    let amp = rng.uniform(0.1, 1.0);
    // Large patches push the per-patch sample count over `slm`.
    let scale = if rng.chance(0.5) {
        rng.uniform(10.0, 300.0)
    } else {
        1.0
    };
    let mut net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (x, y) = (
                        i as f64 + rng.uniform(-0.2, 0.2),
                        j as f64 + rng.uniform(-0.2, 0.2),
                    );
                    let z = amp * rng.uniform(-1.0, 1.0);
                    let (x, y, z) = (scale * x, scale * y, scale * z);
                    if rational {
                        let w = rng.uniform(0.7, 1.5);
                        [x * w, y * w, z * w, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    if rng.chance(0.1) {
        // Collapse the first row to one point (a pole).
        if let Some(apex) = net.first().and_then(|r| r.first()).copied() {
            if let Some(row) = net.first_mut() {
                row.fill(apex);
            }
        }
    }
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-2.0, 0.0), rng.uniform(0.5, 4.0))
    };
    let u = uniform_knots(n, p, a, b);
    let v = uniform_knots(m, q, 0.0, 1.0);
    InlineSrf { net, p, q, u, v }
}

fn gen_one(rng: &mut Rng) -> Inputs {
    let kind = if rng.chance(0.3) {
        Kind::FreeForm
    } else {
        KINDS
            .get(rng.below(KINDS.len() as u64) as usize)
            .copied()
            .unwrap_or(Kind::FreeForm)
    };
    let sur = special(rng, kind)
        .map(|s| to_inline(&s))
        .unwrap_or_else(|| free_form(rng));
    let d = match rng.below(7) {
        0 => -rng.uniform(0.05, 0.5),
        6 => -rng.uniform(0.5, 3.0),
        1 => 0.0,
        _ => rng.uniform(0.05, 0.5),
    };
    let tol = match rng.below(6) {
        0 => 1e-2,
        1 | 2 => 1e-4,
        3 => 1e-1,
        _ => 1e-3,
    };
    let deg = if rng.chance(0.15) {
        1 + rng.below(5) as usize
    } else {
        3
    };
    let qq = if rng.chance(0.15) {
        1 + rng.below(5) as usize
    } else {
        deg
    };
    Inputs {
        sur: SrfIn::Inline(sur),
        d,
        pp: deg,
        qq,
        par: 1 + rng.below(4) as i32,
        eps: if rng.chance(0.15) { 1e-9 } else { 10.0 * tol },
        tol,
    }
}

/// True when upstream would read an unset generator tangent (the shim rejects such
/// inputs; `N_SrfOffset` does not check `N_CrvEvalTangent`).
fn unset_tangent(i: &Inputs) -> bool {
    let Ok(sur) = resolve(&i.sur).and_then(|s| build(&s)) else {
        return false;
    };
    let Ok(t) = sur.surface_type(i.eps) else {
        return false;
    };
    let rev = matches!(t.kind.flag(), 4..=8);
    let flt = t.cc != Vec3::new(0.0, 0.0, 0.0);
    if !((rev && flt) || t.kind.flag() == 10) {
        return false;
    }
    t.cur_a.as_ref().is_none_or(|c| {
        let (u0, _) = c.param_bounds();
        c.eval_tangent(u0, Side::Right).is_err()
    })
}

/// A generated case the shim accepts.
fn gen_case(rng: &mut Rng) -> Inputs {
    let mut i = gen_one(rng);
    for _ in 0..16 {
        if !unset_tangent(&i) {
            break;
        }
        i = gen_one(rng);
    }
    i
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.srf_offset"
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

    use solidcore::nurbs::geom::Rectangle;
    use solidcore::nurbs::srf_adv::p1::offset_get_max_deriv;
    use solidcore::nurbs::srf_offset::OffsetOptions;
    use solidcore::nurbs::srf_tool::p1::decompose_to_bez;

    /// Check interior samples of every original knot span, including the sliver and
    /// the short extensions. A global uniform grid would miss the failing region.
    fn span_parameters(knots: &[f64]) -> Vec<f64> {
        knots
            .windows(2)
            .filter(|w| w[0] < w[1])
            .flat_map(|w| (0..=32).map(move |i| w[0] + (w[1] - w[0]) * f64::from(i) / 32.0))
            .collect()
    }

    #[test]
    fn issue471_conditioned_offsets_of_the_actual_drafted_rounds() {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/nlib.srf_offset/issue471.json"
        ))
        .unwrap();
        for case in data["cases"].as_array().unwrap() {
            let i: Inputs = serde_json::from_value(case["inputs"].clone()).unwrap();
            let mut s = build(&resolve(&i.sur).unwrap()).unwrap();
            for swap in [false, true] {
                if swap {
                    s.swap_uv();
                }
                for normalized in [false, true] {
                    for d in [i.d, -i.d] {
                        let base =
                            solidcore::nurbs::b_spline_surface::BSplineSurface::from_gw_nurb(&s);
                        let offset = base.approximate_offset_surface_with(d, i.tol,
                            solidcore::nurbs::b_spline_surface_n_lib::p1::OffsetParamType::Inherited,
                            OffsetOptions { normalize_extrusion_direction: normalized, condition_sampling: true }).unwrap();
                        let q = offset.gw_nurb().unwrap();
                        assert_eq!(q.parameter_bounds(), s.parameter_bounds());
                        // The original failing net contains points more than 4 m
                        // from the origin, although this box is only 120x80x40 mm.
                        assert!(q
                            .cpts()
                            .iter()
                            .flatten()
                            .all(|p| p.to_euclid().length() < 100.0));
                        let mut worst = 0.0_f64;
                        for u in span_parameters(s.u_knots()) {
                            for v in span_parameters(s.v_knots()) {
                                let e = s
                                    .eval_pt_deriv_normal_fast(u, v, Side::Left, Side::Left)
                                    .unwrap();
                                let p = q.eval(u, v, Side::Left, Side::Left).unwrap();
                                worst = worst.max((p - (e.p + e.n * d)).length());
                            }
                        }
                        assert!(
                            worst <= i.tol,
                            "{} swap={swap} normalize={normalized} d={d} error={worst}",
                            case["name"]
                        );
                    }
                }
            }
        }
    }

    /// Some Bezier patch of `sur` gets a sample count of at least `slm` (1000).
    fn slm_reached(sur: &Surface, i: &Inputs) -> bool {
        let exp = if i.pp == 1 || i.qq == 1 { 0.5 } else { 0.34 };
        let f1 = (1.0 / i.tol).powf(exp);
        let f2 = 2.0 * i.pp.max(i.qq) as f64;
        decompose_to_bez(sur).unwrap().iter().flatten().any(|b| {
            let mut b = b.clone();
            b.reparam_to_interval(Rectangle::new(0.0, 1.0, 0.0, 1.0), Direction::UV)
                .unwrap();
            let md = offset_get_max_deriv(&b, i.d).unwrap();
            let num = 0.125 * (md.muu + md.mvv) + 0.25 * md.muv;
            (f1 * f2.max(num.sqrt())).ceil() >= 1000.0
        })
    }

    /// The generated cases reach every recognised surface kind with successes, and
    /// some failures.
    #[test]
    fn generated_cases_cover_kinds() {
        let mut stats: BTreeMap<(i32, i64), usize> = BTreeMap::new();
        let mut slm_hits = 0;
        for seed in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let sur = build(&resolve(&i.sur).unwrap()).unwrap();
            let kind = sur.surface_type(i.eps).map_or(0, |t| t.kind.flag());
            if kind == 11 && slm_reached(&sur, &i) {
                slm_hits += 1;
            }
            let flat = flatten(&rust_result(&i).unwrap());
            *stats.entry((kind, flat[0] as i64)).or_default() += 1;
        }
        eprintln!("{stats:?} slm {slm_hits}");
        assert!(slm_hits >= 5, "{slm_hits}");
        for kind in [3, 4, 5, 6, 7, 8, 10, 11] {
            assert!(
                stats.get(&(kind, 0)).copied().unwrap_or(0) >= 3,
                "{kind}: {stats:?}"
            );
        }
        assert!(stats.keys().any(|&(_, st)| st != 0), "{stats:?}");
    }
}
