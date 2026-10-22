//! Runner for unit `curve.curve_p8`: `SmCurve::GlobalCurveIntersect`.
//!
//! C++: `solidcore_ffi::units::curve_curve_p8` (upstream `source/SMLib/src/SmCurve.cpp`,
//! called through `SmCurve*` on B-spline, Hermite and line curves). Rust:
//! `solidcore::curve::curve::p8`.
//!
//! A case is one `op` (`gci`) with its curves (`{"bspline": {dim, oob, cpts, p, u}}` with
//! control points as stored `[x, y, z, w]`, `{"hermite": {dim, p1, d1, p2, d2}}`,
//! `{"circle": ..}` (a quadratic B-spline arc) or `{"line": {p, v, domain, scale,
//! inside_out}}`, as in unit `curve.curve_p6`; `c2: null` intersects the first curve with
//! itself), the two intervals, the tolerance and `skip` (`bSkipGlobalCoincidenceCheck`).
//! Both sides produce the flat record documented in the ffi module; the runner reports
//! its first entry as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007`
//! for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest
//! as the `out` values. Solution objects are `0` (this curve), `1` (the other curve), `2`
//! (an analytic replacement made by a curve cache).
//!
//! `{"o", "x", "y", "scale"?, "repeat"?}` transforms (`gci` `xf1`, `xf2`) are applied on both sides
//! (`SmAxis2Placement::SetCanonical` + `SmCurve::Transform` / `Curve::transform`) before
//! the op; a circle with `quintic` is `SM_CO_QUINTIC`.
//!
//! Deferred cases (unit `curve.curve_p5`): the Newton curve/curve intersections of the
//! leaf pair solves and of `sm_RefineCCSolutions` (`SmCCIntersectENFO::Evaluate`), the
//! gap refinement and closed-curve recombination of the coincidence check, and the
//! analytic conversion of arcs (`SmCurve::IsArc`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::curve::p8::CurveIntersectOptions;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject, SolutionType};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::SolidError;
use solidcore_ffi::units::curve_curve_p8::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `curve.curve_p8` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermiteIn {
    dim: u64,
    p1: [f64; 3],
    d1: [f64; 3],
    p2: [f64; 3],
    d2: [f64; 3],
}

/// A quadratic circular arc built by the Rust `CreateCircleSegment` (bit-identical to
/// upstream, see unit `nurbs.b_spline_curve_p1`) and handed to both sides as inline data.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    a0: f64,
    a1: f64,
    /// `SM_CO_QUINTIC` (degree 5) instead of `SM_CO_QUADRATIC`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    quintic: bool,
}

/// A transform applied to a curve before the op, `repeat` times
/// (`SmAxis2Placement::SetCanonical(o, x, y)`, then `SmCurve::Transform` with `scale`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct XformIn {
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<[f64; 3]>,
    #[serde(default = "one_u64")]
    repeat: u64,
}

fn one_u64() -> u64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LineIn {
    p: [f64; 3],
    v: [f64; 3],
    domain: [f64; 2],
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    inside_out: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Circle(CircleIn),
    Line(LineIn),
}

// ---- C++ input --------------------------------------------------------------------

/// `c` with a circle replaced by its inline B-spline.
fn resolve(c: &CurveIn) -> Result<CurveIn, HarnessError> {
    let CurveIn::Circle(ci) = c else {
        return Ok(c.clone());
    };
    let frame = Axis2Placement::new(v3(ci.center), v3(ci.x), v3(ci.y));
    let arc = BSplineCurve::create_circle_segment(
        3,
        &frame,
        ci.radius,
        ci.a0,
        ci.a1,
        if ci.quintic {
            solidcore::nurbs::b_spline_curve::NurbCircleParam::Quintic
        } else {
            solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic
        },
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    let cur = arc.gw_nurb().ok_or_else(|| harness("circle without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    }))
}

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn push_line(v: &mut Vec<f64>, l: &LineIn) {
    v.extend_from_slice(&l.p);
    v.extend_from_slice(&l.v);
    v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, bs.dim as f64, b(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            push_line(v, l);
        }
        CurveIn::Circle(_) => return Err(harness("unresolved circle")),
    }
    Ok(())
}


/// The transforms block `nt (O X Y has_s S)*nt` (see the ffi module docs).
fn push_xf(v: &mut Vec<f64>, xf: &[XformIn]) {
    v.push(xf.iter().map(|t| t.repeat).sum::<u64>() as f64);
    for t in xf {
        for _ in 0..t.repeat {
            v.extend_from_slice(&t.o);
            v.extend_from_slice(&t.x);
            v.extend_from_slice(&t.y);
            v.push(b(t.scale.is_some()));
            v.extend_from_slice(&t.scale.unwrap_or([1.0; 3]));
        }
    }
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_line(l: &LineIn) -> Line {
    let opts = BoundedLineOptions {
        scale: l.scale,
        inside_out: l.inside_out,
        ..BoundedLineOptions::default()
    };
    Line::bounded(
        v3(l.p),
        v3(l.v),
        &Extent1d::from_min_max(l.domain[0], l.domain[1]),
        &opts,
    )
}

/// `c` transformed by `xf` in order (`Curve::transform`).
fn apply_xf(k: &Kernel, c: &mut Curve, xf: &[XformIn]) -> Result<(), HarnessError> {
    for t in xf {
        let a2p = Axis2Placement::new(v3(t.o), v3(t.x), v3(t.y));
        for _ in 0..t.repeat {
            c.transform(k, &a2p, t.scale.map(v3))
                .map_err(|e| harness(format!("transform: {e}")))?;
        }
    }
    Ok(())
}

fn build(c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m = bs.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, bs.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&bs.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&bs.u);
            let mut c = BSplineCurve::from_nurb(cur, bs.dim, false);
            c.set_out_of_bounds_enabled(bs.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
        CurveIn::Line(l) => Ok(Curve::from(build_line(l))),
        CurveIn::Circle(_) => Err(harness("unresolved circle")),
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, else the status record.
enum Fail {
    Deferred(Box<Output>),
    Status(f64),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        match Output::unported(&e) {
            Some(o) => Fail::Deferred(Box::new(o)),
            None => Fail::Status(status(&e)),
        }
    }
}

/// Evaluates `r`: `Ok(v)`, or records the status / deferral.
macro_rules! tri {
    ($o:ident, $r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => match Fail::from(e) {
                Fail::Deferred(d) => return Ok(*d),
                Fail::Status(s) => {
                    $o.push(s);
                    return Ok(to_output(&$o));
                }
            },
        }
    };
}

/// A curve handle: its id (`0` this curve, `1` the other) and natural interval.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(i64, Extent1d);

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.1)
    }
}

/// A solution record (see the ffi module docs); `id` maps the objects.
fn put_sol<O: Copy>(o: &mut Vec<f64>, s: &Solution<O>, id: impl Fn(Option<O>) -> f64) {
    let second = |e: &solidcore::geom::solution_array::SolutionEnd| {
        if s.num_variables > 1 {
            e.parameters[1]
        } else {
            0.0
        }
    };
    o.extend([
        f64::from(s.solution_type as i32),
        s.num_variables as f64,
        s.num_objects as f64,
        if s.num_objects > 0 {
            id(s.objects[0])
        } else {
            -1.0
        },
        if s.num_objects > 1 {
            id(s.objects[1])
        } else {
            -1.0
        },
        s.start.solution_value,
        s.start.parameters[0],
        second(&s.start),
    ]);
    if s.solution_type == SolutionType::RangeOfValues {
        o.extend([
            s.end.solution_value,
            s.end.parameters[0],
            second(&s.end),
        ]);
    }
}

fn ivl_of(i: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(i[0], i[1])
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational, 2D or with repeated interior knots).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.15);
    let a = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.1, 10.0)
    };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    if inner >= 2 && p >= 2 && rng.chance(0.3) {
        if let Some(&v) = ks.first() {
            if let Some(x) = ks.get_mut(1) {
                *x = v;
            }
        }
    }
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let scale = if rng.chance(0.5) { 5.0 } else { 100.0 };
    let cpts = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.2;
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            let z = if two_d { NOZ } else { z };
            if rational {
                [x * w, y * w, if two_d { z } else { z * w }, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    let dim = if two_d || rng.chance(0.1) { 2 } else { 3 };
    BSplineIn {
        dim,
        oob: rng.chance(0.1),
        cpts,
        p,
        u,
    }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 50.0 };
    let mut d1 = rp(rng, s);
    if rng.chance(0.1) {
        d1 = [0.0; 3];
    }
    let p1 = rp(rng, s);
    let mut p2 = rp(rng, s);
    if rng.chance(0.1) {
        p2 = p1;
    }
    let mut h = HermiteIn {
        dim: if rng.chance(0.2) { 2 } else { 3 },
        p1,
        d1,
        p2,
        d2: if rng.chance(0.08) { [0.0; 3] } else { rp(rng, s) },
    };
    if h.dim == 2 {
        for v in [&mut h.p1, &mut h.d1, &mut h.p2, &mut h.d2] {
            v[2] = 0.0;
        }
    }
    h
}

fn gen_line(rng: &mut Rng) -> LineIn {
    let d0 = rng.uniform(-5.0, 5.0);
    LineIn {
        p: rp(rng, 10.0),
        v: rp(rng, 3.0),
        domain: [d0, d0 + rng.uniform(0.5, 10.0)],
        scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
        inside_out: rng.chance(0.3),
    }
}

fn gen_circle(rng: &mut Rng) -> CurveIn {
    let a0 = rng.uniform(-90.0, 90.0);
    CurveIn::Circle(CircleIn {
        center: rp(rng, 5.0),
        x: [1.0, 0.0, 0.0],
        y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
        radius: rng.uniform(0.5, 10.0),
        a0,
        a1: a0 + rng.uniform(20.0, 360.0),
        quintic: false,
    })
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    let c = match rng.below(20) {
        0..=2 => gen_circle(rng),
        3..=6 => CurveIn::Line(gen_line(rng)),
        7..=14 => CurveIn::Bspline(gen_bspline(rng)),
        _ => CurveIn::Hermite(gen_hermite(rng)),
    };
    resolve(&c).unwrap_or(c)
}

/// A curve that is closed (or nearly) over its natural interval.
fn gen_closed_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(3) {
        0 => {
            let a0 = rng.uniform(-90.0, 90.0);
            let c = CurveIn::Circle(CircleIn {
                center: rp(rng, 5.0),
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                radius: rng.uniform(0.5, 10.0),
                a0,
                a1: a0 + 360.0,
                quintic: false,
            });
            resolve(&c).unwrap_or(c)
        }
        1 => {
            let mut h = gen_hermite(rng);
            h.p2 = h.p1;
            CurveIn::Hermite(h)
        }
        _ => {
            let mut bs = gen_bspline(rng);
            if let Some(f) = bs.cpts.first().copied() {
                if let Some(l) = bs.cpts.last_mut() {
                    *l = f;
                }
            }
            CurveIn::Bspline(bs)
        }
    }
}

fn range_of(c: &CurveIn) -> (f64, f64) {
    let resolved = resolve(c).ok();
    match resolved.as_ref().unwrap_or(c) {
        CurveIn::Circle(ci) => (ci.a0, ci.a1),
        CurveIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        CurveIn::Hermite(_) => (0.0, 1.0),
        CurveIn::Line(l) => {
            let nat = build_line(l).natural_interval();
            (nat.min(), nat.max())
        }
    }
}

/// An increasing interval of `c`: mostly its natural one or a sub-interval.
fn gen_ivl(rng: &mut Rng, c: &CurveIn) -> [f64; 2] {
    let (a, bb) = range_of(c);
    if rng.chance(0.5) {
        return [a, bb];
    }
    for _ in 0..16 {
        let x = a + (bb - a) * rng.uniform(-0.1, 1.1);
        let y = a + (bb - a) * rng.uniform(-0.1, 1.1);
        if (x - y).abs() > 1e-3 * (bb - a) {
            return [x.min(y), x.max(y)];
        }
    }
    [a, bb]
}

/// A unit vector-ish direction.
fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rp(rng, 1.0);
        if v.iter().map(|c| c * c).sum::<f64>() > 0.05 {
            return v;
        }
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// The point of `c` at `t` (Rust evaluation, used only to place test points).
fn point_at(k: &Kernel, c: &CurveIn, t: f64) -> [f64; 3] {
    build(c)
        .ok()
        .and_then(|cc| cc.evaluate_point(k, t).ok())
        .map_or([0.0; 3], |p| [p.x, p.y, p.z])
}

/// `c` moved by `d` (control points / line point / Hermite points).
fn translated(c: &CurveIn, d: [f64; 3]) -> CurveIn {
    match c {
        CurveIn::Bspline(bs) => {
            let mut bs = bs.clone();
            for p in &mut bs.cpts {
                let w = if p[3] == NOW { 1.0 } else { p[3] };
                p[0] += w * d[0];
                p[1] += w * d[1];
                if p[2] != NOZ {
                    p[2] += w * d[2];
                }
            }
            CurveIn::Bspline(bs)
        }
        CurveIn::Hermite(h) => {
            let mut h = h.clone();
            h.p1 = add(h.p1, d);
            h.p2 = add(h.p2, d);
            if h.dim == 2 {
                h.p1[2] = 0.0;
                h.p2[2] = 0.0;
            }
            CurveIn::Hermite(h)
        }
        CurveIn::Line(l) => {
            let mut l = l.clone();
            l.p = add(l.p, d);
            CurveIn::Line(l)
        }
        CurveIn::Circle(ci) => {
            let mut ci = ci.clone();
            ci.center = add(ci.center, d);
            CurveIn::Circle(ci)
        }
    }
}

/// A second curve near `c`: an identical copy, a slightly moved copy, the same line
/// reparameterised, or an unrelated curve.
fn gen_partner(k: &Kernel, rng: &mut Rng, c: &CurveIn) -> CurveIn {
    match rng.below(8) {
        0..=2 => c.clone(),
        3 | 4 => {
            let off = *rng.choose(&[1e-9, 1e-7, 1e-5, 1e-3, 0.1]).unwrap_or(&1e-7);
            translated(c, scale3(gen_dir(rng), off))
        }
        5 => {
            // a line on the same carrier with another parameterisation
            let (a, bb) = range_of(c);
            let p = point_at(k, c, a);
            let q = point_at(k, c, bb);
            let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            if rng.chance(0.5) {
                CurveIn::Line(LineIn {
                    p,
                    v: d,
                    domain: [0.0, 1.0],
                    scale: 1.0,
                    inside_out: rng.chance(0.5),
                })
            } else {
                CurveIn::Line(LineIn {
                    p: q,
                    v: scale3(d, -1.0),
                    domain: [rng.uniform(-1.0, 1.0), rng.uniform(1.5, 3.0)],
                    scale: 1.0,
                    inside_out: false,
                })
            }
        }
        _ => gen_curve(rng),
    }
}

/// A second interval for a partner curve.
fn gen_partner_ivl(rng: &mut Rng, c: &CurveIn, own: [f64; 2]) -> [f64; 2] {
    let (a, bb) = range_of(c);
    match rng.below(4) {
        0 => [a, bb],
        1 if own[0] >= a && own[1] <= bb && own[0] < own[1] => own,
        _ => gen_ivl(rng, c),
    }
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 0.5])
        .unwrap_or(&1e-5)
}

// ---- the op -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Gci {
        c1: CurveIn,
        /// Transforms applied to `c1` first.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        xf1: Vec<XformIn>,
        ivl1: [f64; 2],
        /// `None`: the other curve is `c1` itself (a self intersection).
        c2: Option<CurveIn>,
        /// Transforms applied to `c2` first (none for a self intersection).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        xf2: Vec<XformIn>,
        ivl2: [f64; 2],
        tol: f64,
        #[serde(default)]
        skip: bool,
    },
}

fn cpp_input(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    match op {
        OpIn::Gci {
            c1,
            xf1,
            ivl1,
            c2,
            xf2,
            ivl2,
            tol,
            skip,
        } => {
            curve_block(&mut v, c1)?;
            push_xf(&mut v, xf1);
            v.extend_from_slice(ivl1);
            v.push(b(c2.is_none()));
            if let Some(c2) = c2 {
                curve_block(&mut v, c2)?;
                push_xf(&mut v, xf2);
            }
            v.extend_from_slice(ivl2);
            v.extend([*tol, b(*skip)]);
        }
    }
    Ok((Op::Gci, v))
}

fn cpp(op: &OpIn) -> Result<Output, HarnessError> {
    let (code, input) = cpp_input(op)?;
    match ffi::eval(code, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

/// Object ids of the shim: the analytic replacements (`2`, `3`) are "another object".
fn shim_id(x: Option<Obj>) -> f64 {
    x.map_or(-1.0, |v| if v.0 >= 2 { 2.0 } else { v.0 as f64 })
}

fn rust(op: &OpIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    match op {
        OpIn::Gci {
            c1,
            xf1,
            ivl1,
            c2,
            xf2,
            ivl2,
            tol,
            skip,
        } => {
            let mut a = build(c1)?;
            apply_xf(&k, &mut a, xf1)?;
            let other = match c2 {
                Some(c) => {
                    let mut o = build(c)?;
                    apply_xf(&k, &mut o, xf2)?;
                    Some(o)
                }
                None => None,
            };
            let bb = other.as_ref().unwrap_or(&a);
            let h1 = i64::from(other.is_some());
            let opts = CurveIntersectOptions {
                skip_global_coincidence_check: *skip,
                handles: [
                    Some(Obj(0, a.natural_interval())),
                    Some(Obj(h1, bb.natural_interval())),
                ],
                analytic_handles: [
                    Some(Obj(2, a.natural_interval())),
                    Some(Obj(3, bb.natural_interval())),
                ],
            };
            let sols = tri!(
                o,
                a.global_curve_intersect(&k, &ivl_of(*ivl1), bb, &ivl_of(*ivl2), *tol, &opts)
            );
            o.extend([0.0, sols.len() as f64]);
            for s in sols.as_slice() {
                put_sol(&mut o, s, shim_id);
            }
        }
    }
    Ok(to_output(&o))
}

// ---- intersection generators ---------------------------------------------------------

/// A curve through the point of `c` at `t` (a random curve moved so that one of its
/// points lands there, optionally a little off).
fn gen_crossing(k: &Kernel, rng: &mut Rng, c: &CurveIn, t: f64) -> CurveIn {
    let other = gen_curve(rng);
    let (a, bb) = range_of(&other);
    let s = a + (bb - a) * rng.uniform(0.05, 0.95);
    let p = point_at(k, c, t);
    let q = point_at(k, &other, s);
    let off = *rng.choose(&[0.0, 0.0, 1e-9, 1e-7, 1e-5]).unwrap_or(&0.0);
    let d = add([p[0] - q[0], p[1] - q[1], p[2] - q[2]], scale3(gen_dir(rng), off));
    translated(&other, d)
}

/// A line tangent to `c` at `t` (along the chord to a nearby point), possibly offset.
fn gen_tangent_line(k: &Kernel, rng: &mut Rng, c: &CurveIn, t: f64) -> CurveIn {
    let (a, bb) = range_of(c);
    let h = 1e-4 * (bb - a);
    let p0 = point_at(k, c, t - h);
    let p1 = point_at(k, c, t + h);
    let p = point_at(k, c, t);
    let dir = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let off = *rng.choose(&[0.0, 1e-8, 1e-6, 1e-3]).unwrap_or(&0.0);
    let p = add(p, scale3(gen_dir(rng), off));
    let len = dir.iter().map(|x| x * x).sum::<f64>().sqrt();
    if len < 1e-12 {
        return CurveIn::Line(gen_line(rng));
    }
    let s = rng.uniform(0.5, 5.0) / len;
    CurveIn::Line(LineIn {
        p: add(p, scale3(dir, -s)),
        v: scale3(dir, 2.0 * s),
        domain: [0.0, 1.0],
        scale: 1.0,
        inside_out: false,
    })
}

/// A curve that crosses itself (a looping cubic Hermite or B-spline).
fn gen_looping(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.5) {
        let s = rng.uniform(1.0, 10.0);
        CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: [0.0, 0.0, 0.0],
            d1: [3.0 * s, 3.0 * s, 0.0],
            p2: [0.0, rng.uniform(-0.2, 0.2) * s, rng.uniform(-1e-6, 1e-6)],
            d2: [3.0 * s, -3.0 * s, 0.0],
        })
    } else {
        let z = rng.uniform(-1e-3, 1e-3);
        let cpts = [
            [0.0, 0.0, 0.0],
            [4.0, 3.0, z],
            [4.0, -3.0, -z],
            [0.0, 1.0, z],
            [-1.0, -2.0, 0.0],
        ];
        CurveIn::Bspline(BSplineIn {
            dim: 3,
            oob: false,
            cpts: cpts.iter().map(|p| [p[0], p[1], p[2], NOW]).collect(),
            p: 3,
            u: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
        })
    }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> OpIn {
    let c1 = match rng.below(10) {
        0 => gen_closed_curve(rng),
        1 => gen_looping(rng),
        _ => gen_curve(rng),
    };
    let ivl1 = gen_ivl(rng, &c1);
    let (a, bb) = range_of(&c1);
    let t = a + (bb - a) * rng.uniform(0.0, 1.0);
    let c2 = match rng.below(20) {
        0 | 1 => None,
        2..=6 => Some(gen_crossing(k, rng, &c1, t)),
        7 | 8 => Some(gen_tangent_line(k, rng, &c1, t)),
        9..=13 => Some(gen_partner(k, rng, &c1)),
        14 => Some(translated(&gen_curve(rng), [300.0, -200.0, 50.0])),
        15 => Some(CurveIn::Line(gen_line(rng))),
        _ => Some(gen_curve(rng)),
    };
    let ivl2 = match &c2 {
        Some(c) => gen_partner_ivl(rng, c, ivl1),
        None => {
            if rng.chance(0.7) {
                ivl1
            } else {
                gen_ivl(rng, &c1)
            }
        }
    };
    let (c1, ivl1, c2, ivl2) = match c2 {
        Some(c2) if rng.chance(0.3) => (c2, ivl2, Some(c1), ivl1),
        other => (c1, ivl1, other, ivl2),
    };
    OpIn::Gci {
        c1,
        xf1: Vec::new(),
        ivl1,
        c2,
        xf2: Vec::new(),
        ivl2,
        tol: gen_tol(rng),
        skip: rng.chance(0.15),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.curve_p8"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let op = gen_case(k, rng);
            let Ok((code, input)) = cpp_input(&op) else {
                continue;
            };
            if ffi::validate(code, &input).is_err() {
                continue;
            }
            return serde_json::to_value(op).ok();
        }
        None
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
        let op = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: its guards refuse inputs
        // upstream (and the faithful port) never finish on (fuzz Rust-side hangs).
        let (code, input) = cpp_input(&op)?;
        ffi::validate(code, &input).map_err(|e| HarnessError::new(e.to_string()))?;
        rust(&op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coincidence_splits_stop_at_the_call_bound() {
        // fuzz01456 (`tests/fuzz_corpus/curve.curve_p8.json`): the FFI refuses its
        // `1e300` control points, so the runner never hands it to the port; run
        // directly, `GlobalCoincidenceChecker` split in three about 17 levels deep and
        // ran for minutes (issue #504). It now fails past `MAX_COINCIDENCE_CALLS`.
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fuzz_corpus/curve.curve_p8.json"
        ))
        .unwrap();
        let case = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "fuzz01456_rust_hang")
            .unwrap();
        let op: OpIn = serde_json::from_value(case["inputs"].clone()).unwrap();
        let out = rust(&op).unwrap();
        assert_eq!(out.ints["status"], vec![1007], "{out:?}");
    }


    /// Generated cases mostly run (pass or defer), not harness errors.
    #[test]
    fn generated_cases_run() {
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats = [0usize; 4];
        for _ in 0..300 {
            let op = gen_case(k, &mut rng);
            stats[0] += 1;
            match rust(&op) {
                Err(_) => stats[3] += 1,
                Ok(o) if o.deferred.is_some() => stats[2] += 1,
                Ok(_) => stats[1] += 1,
            }
        }
        eprintln!(
            "total {} ran {} deferred {} harness {}",
            stats[0], stats[1], stats[2], stats[3]
        );
        assert!(stats[1] > 0);
    }
}
