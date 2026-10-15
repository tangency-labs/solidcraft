//! Runner for unit `intersect.surface_tracer`: `SmSurfaceTracer`.
//!
//! C++: `solidcore_ffi::units::intersect_surface_tracer` (upstream
//! `source/SMLib/src/SmSurfaceTracer.cpp` through the shim's `SectionTracer` subclass).
//! Rust: `solidcore::intersect::surface_tracer` through [`SectionTracer`], the same subclass
//! (the curves where the surface meets the plane `n.(P - p0) = 0`): its four hooks
//! (`ComputePointValues`, `LocalPointSolve`, `DoesPointLieOnCurve`,
//! `FindSolutionsOnBoundaryCurve`) are test scaffolding written with the same operations in
//! the same order on both sides; everything else compared is the unit's code (trace stepping,
//! step control, span deviation, closure and boundary tests, start points, reversal, Bezier
//! flush, iso curve test, the interior trace sequence of `FindInteriorCurves`).
//!
//! A case is a surface, a trace domain, the section plane, optional tolerances and a program
//! of ops; both sides produce the record documented in the ffi module, reported as the values
//! `rec`.
//!
//! `DoTrace` runs on the surface's cache (`smsurf_GetSurfaceCache` through
//! `tracer_surface_cache`). Deferred cases: traces reaching the general `DropCurve`
//! (`intersect.surface_drop_lib_p3`) or the surface cache's unported members
//! (`surface.surface_cache_p1`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject, SolutionType};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::intersect::surface_tracer::{
    do_trace_base, flush_curve, tracer_surface_cache, BoundaryCurve, SurfaceTrace, SurfaceTracer,
    TraceOutput, TracePoint, TracePointType,
};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_tracer::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// `intersect.surface_tracer` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

/// A B-spline surface (`w` = [`NOW`] when non-rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfData {
    p: usize,
    q: usize,
    net: Vec<Vec<[f64; 4]>>,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Bspline(SurfData),
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurveData {
    p: usize,
    cpts: Vec<[f64; 4]>,
    knots: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    Segment { a: [f64; 3], b: [f64; 3] },
    Bspline(CurveData),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
struct PointIn {
    kind: u8,
    param: f64,
    uv: [f64; 2],
    uv_tan: [f64; 2],
    p: [f64; 3],
    t: [f64; 3],
    srf: [f64; 3],
    nrm: [f64; 3],
    drop: f64,
    dev: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
struct StartIn {
    kind: u8,
    uv: [f64; 2],
    t: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Step {
        old: f64,
    },
    Trace {
        uv: [f64; 2],
    },
    Flush {
        points: Vec<PointIn>,
        curve: Option<CurveIn>,
    },
    AddCurve {
        curve: CurveIn,
    },
    OnCurve {
        p: [f64; 3],
    },
    Iso {
        v_dir: bool,
        value: f64,
    },
    Start {
        points: Vec<StartIn>,
    },
    Test {
        point: PointIn,
    },
    DoTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SurfIn,
    domain: [f64; 4],
    p0: [f64; 3],
    n: [f64; 3],
    tol: Option<f64>,
    ang: Option<f64>,
    ops: Vec<OpIn>,
}

// ---- C++ side -----------------------------------------------------------------------

fn ffi_surface(s: &SurfIn) -> ffi::SurfaceIn {
    match s {
        SurfIn::Bspline(d) => ffi::SurfaceIn::BSpline(ffi::SurfaceData {
            p: d.p,
            q: d.q,
            net: d.net.clone(),
            u: d.u.clone(),
            v: d.v.clone(),
        }),
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::SurfaceIn::Plane {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
    }
}

fn ffi_curve(c: &CurveIn) -> ffi::CurveIn {
    match c {
        CurveIn::Segment { a, b } => ffi::CurveIn::Segment(*a, *b),
        CurveIn::Bspline(d) => ffi::CurveIn::BSpline(ffi::CurveData {
            p: d.p,
            cpts: d.cpts.clone(),
            knots: d.knots.clone(),
        }),
    }
}

fn ffi_point(p: &PointIn) -> ffi::PointIn {
    ffi::PointIn {
        kind: p.kind,
        param: p.param,
        uv: p.uv,
        uv_tan: p.uv_tan,
        p: p.p,
        t: p.t,
        srf: p.srf,
        nrm: p.nrm,
        drop: p.drop,
        dev: p.dev,
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    let ops = i
        .ops
        .iter()
        .map(|o| match o {
            OpIn::Step { old } => ffi::Op::Step(*old),
            OpIn::Trace { uv } => ffi::Op::Trace(*uv),
            OpIn::Flush { points, curve } => ffi::Op::Flush {
                points: points.iter().map(ffi_point).collect(),
                curve: curve.as_ref().map(ffi_curve),
            },
            OpIn::AddCurve { curve } => ffi::Op::AddCurve(ffi_curve(curve)),
            OpIn::OnCurve { p } => ffi::Op::OnCurve(*p),
            OpIn::Iso { v_dir, value } => ffi::Op::Iso {
                v_dir: *v_dir,
                value: *value,
            },
            OpIn::Start { points } => ffi::Op::Start(
                points
                    .iter()
                    .map(|s| ffi::StartIn {
                        kind: s.kind,
                        uv: s.uv,
                        t: s.t,
                    })
                    .collect(),
            ),
            OpIn::Test { point } => ffi::Op::Test(ffi_point(point)),
            OpIn::DoTrace => ffi::Op::DoTrace,
        })
        .collect();
    ffi::Program {
        surface: ffi_surface(&i.surface),
        domain: i.domain,
        p0: i.p0,
        n: i.n,
        tol: i.tol,
        ang: i.ang,
        ops,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_program(i)) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p2(p: [f64; 2]) -> Point2 {
    Vec2::new(p[0], p[1])
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn cpoint(c: &[f64; 4]) -> CPoint {
    CPoint::new(c[0], c[1], c[2], c[3])
}

fn rust_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    match s {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext(*domain),
        ))),
        SurfIn::Bspline(d) => {
            let net = d
                .net
                .iter()
                .map(|r| r.iter().map(cpoint).collect())
                .collect();
            let sur = NurbSurface::new(net, d.p, d.q, d.u.clone(), d.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
    }
}

fn rust_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Segment { a, b } => Line::create_line_segment(3, v3(*a), v3(*b), None)
            .map(Curve::from)
            .map_err(|e| harness(format!("segment: {e}"))),
        CurveIn::Bspline(d) => {
            let pw: Vec<CPoint> = d.cpts.iter().map(cpoint).collect();
            let n = pw.len().checked_sub(1).ok_or_else(|| harness("empty curve"))?;
            let m = d.knots.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            BSplineCurve::from_data(3, n, m, d.p, &pw, &d.knots)
                .map(Curve::from)
                .map_err(|e| harness(format!("B-spline: {e}")))
        }
    }
}

/// The shim's `SectionTracer`: same hooks, same operations, same order.
struct SectionTracer<'s> {
    base: SurfaceTracer<'s>,
    p0: Point3,
    n: Vec3,
}

impl SectionTracer<'_> {
    /// `[S, S_u, S_v]` (`Evaluate(uv, 1, 1, TRUE, TRUE, FALSE, D)`).
    fn eval(&self, uv: Point2) -> solidcore::Result<(Vec3, Vec3, Vec3)> {
        let d = self
            .base
            .surface
            .evaluate(uv, 1, 1, EvaluateFlags::default())?;
        let at = |i: usize, j: usize| {
            d.get(i)
                .and_then(|r| r.get(j))
                .copied()
                .unwrap_or(Vec3::ZERO)
        };
        Ok((at(0, 0), at(1, 0), at(0, 1)))
    }
}

fn fabs(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

impl<'s> SurfaceTrace<'s> for SectionTracer<'s> {
    fn tracer(&self) -> &SurfaceTracer<'s> {
        &self.base
    }

    fn tracer_mut(&mut self) -> &mut SurfaceTracer<'s> {
        &mut self.base
    }

    fn compute_point_values(
        &mut self,
        _k: &Kernel,
        uv: Point2,
        pnt: &mut TracePoint,
        _prev: Option<&TracePoint>,
        _step_size: Option<f64>,
    ) -> solidcore::Result<()> {
        let (p, su, sv) = self.eval(uv)?;
        let f = self.n.dot(p - self.p0);
        let gu = self.n.dot(su);
        let gv = self.n.dot(sv);
        let dir = self.base.curve_trace_direction;
        let mut t = Vec2::new(-gv * dir, gu * dir);
        let len = t.length();
        pnt.point_type = TracePointType::Normal;
        if len < 1.0e-9 {
            pnt.point_type = if fabs(f) < self.base.approx_tol_3d {
                TracePointType::TangentPoint
            } else {
                TracePointType::Singularity
            };
            t = Vec2::new(0.0, 0.0);
        } else {
            t = t / len;
        }
        let t3 = su * t.x + sv * t.y;
        let norm = su.cross(sv);
        let nlen = norm.length();
        let norm = if nlen > 0.0 { norm / nlen } else { norm };
        pnt.curve_uv[0] = uv;
        pnt.curve_uv[1] = t;
        pnt.curve_3d[0] = p;
        pnt.curve_3d[1] = t3;
        pnt.surface[0][0] = p;
        pnt.surface[1][0] = su;
        pnt.surface[0][1] = sv;
        pnt.surface[2][2] = norm;
        pnt.surf_drop_dist = fabs(f);
        pnt.surf_norm_line_dist = fabs(f) * 0.5;
        Ok(())
    }

    fn local_point_solve(
        &self,
        _k: &Kernel,
        _curr: &TracePoint,
        guess: Point2,
        _next: &mut TracePoint,
    ) -> solidcore::Result<Option<Point2>> {
        let mut uv = guess;
        for _ in 0..30 {
            let (p, su, sv) = self.eval(uv)?;
            let f = self.n.dot(p - self.p0);
            if fabs(f) < self.base.approx_tol_3d * 1.0e-4 {
                return Ok(Some(uv));
            }
            let gu = self.n.dot(su);
            let gv = self.n.dot(sv);
            let g2 = gu * gu + gv * gv;
            if g2 < 1.0e-24 {
                return Ok(None);
            }
            let r = f / g2;
            let step = Vec2::new(gu * r, gv * r);
            uv = self.base.uv_domain.clamp_point(uv - step);
        }
        Ok(None)
    }

    fn does_point_lie_on_curve(
        &self,
        k: &Kernel,
        uv: &mut Point2,
        refine: bool,
    ) -> solidcore::Result<bool> {
        if refine {
            let a = TracePoint::new();
            let mut bb = TracePoint::new();
            match self.local_point_solve(k, &a, *uv, &mut bb) {
                Ok(Some(r)) => *uv = r,
                Ok(None) => {}
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => return Ok(false),
            }
        }
        let (p, _, _) = match self.eval(*uv) {
            Ok(v) => v,
            Err(e) if e.is_unported() => return Err(e),
            Err(_) => return Ok(false),
        };
        Ok(fabs(self.n.dot(p - self.p0)) < self.base.approx_tol_3d)
    }

    fn find_solutions_on_boundary_curve(
        &self,
        _k: &Kernel,
        boundary: &BSplineCurve,
        interval: &Extent1d,
        _side: usize,
        _param: SurfParamType,
    ) -> solidcore::Result<SolutionArray<BoundaryCurve>> {
        const N: usize = 24;
        let mut sols = SolutionArray::new();
        let (mut t_prev, mut f_prev) = (0.0, 0.0);
        for i in 0..=N {
            let t = interval.evaluate(i as f64 / N as f64);
            let p = boundary.evaluate_point(t)?;
            let f = self.n.dot(p - self.p0);
            if i > 0 && ((f_prev <= 0.0 && f > 0.0) || (f_prev > 0.0 && f <= 0.0)) {
                let tt = t_prev + (t - t_prev) * (f_prev / (f_prev - f));
                let mut s = Solution::new();
                s.solution_type = SolutionType::SingleValue;
                s.num_variables = 1;
                s.start.parameters[0] = tt;
                sols.add(&s);
            }
            t_prev = t;
            f_prev = f;
        }
        Ok(sols)
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

/// A failed Rust operation: the whole case is deferred, or the harness failed.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(Ok(v))`, `Ok(Err(status))`, or a deferral.
fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            Ok(Err(status(&e)))
        }
    }
}

/// The status of a call upstream ignores (only deferrals matter).
fn ign<T>(r: solidcore::Result<T>) -> Result<(), Fail> {
    st(r).map(|_| ())
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    if let CurveKind::BSpline(bs) = c.kind() {
        match bs.gw_nurb() {
            None => o.push(0.0),
            Some(cur) => {
                o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
                for p in cur.cpts() {
                    o.extend([p.x, p.y, p.z, p.w]);
                }
                o.extend_from_slice(cur.knots());
            }
        }
    }
}

fn put_curves(o: &mut Vec<f64>, a: &[Curve]) {
    o.push(a.len() as f64);
    for c in a {
        put_curve(o, c);
    }
}

fn put_doubles(o: &mut Vec<f64>, a: &[f64]) {
    o.push(a.len() as f64);
    o.extend_from_slice(a);
}

fn put_start_points(o: &mut Vec<f64>, t: &SurfaceTracer<'_>) -> Result<(), Fail> {
    o.push(t.start_points.len() as f64);
    for &id in &t.start_points {
        let p = t
            .point(id)
            .map_err(|e| Fail::Harness(harness(e.to_string())))?;
        o.push(f64::from(p.point_type as i32));
        put_uv(o, p.curve_uv[0]);
    }
    Ok(())
}

fn kind_of(code: u8) -> Result<TracePointType, HarnessError> {
    TracePointType::from_code(i32::from(code)).ok_or_else(|| harness("point type"))
}

/// A new zero-filled block with the given fields (the shim's `read_point`).
fn point_of(p: &PointIn) -> Result<TracePoint, HarnessError> {
    let mut t = TracePoint::zeroed();
    t.point_type = kind_of(p.kind)?;
    t.curve_parameter = p.param;
    t.curve_uv[0] = p2(p.uv);
    t.curve_uv[1] = p2(p.uv_tan);
    t.curve_3d[0] = v3(p.p);
    t.curve_3d[1] = v3(p.t);
    t.surface[0][0] = v3(p.srf);
    t.surface[2][2] = v3(p.nrm);
    t.surf_drop_dist = p.drop;
    t.surf_norm_line_dist = p.dev;
    Ok(t)
}

/// The shim's `TraceFrom`: the loop body of `FindInteriorCurves` for one node.
fn trace_from(
    t: &mut SectionTracer<'_>,
    k: &Kernel,
    middle: Point2,
    acc: &mut TraceOutput,
) -> Result<(u8, f64), Fail> {
    let mut pnt1 = TracePoint::new();
    let mut pnt2 = TracePoint::new();
    pnt1.curve_3d[1] = Vec3::ZERO;
    let found_uv = match st(t.local_point_solve(k, &pnt1, middle, &mut pnt2))? {
        Ok(Some(uv)) => uv,
        Ok(None) => return Ok((1, 0.0)),
        Err(s) => return Ok((1, s)),
    };
    let surf_pnt = match st(t.base.surface.evaluate_point(found_uv))? {
        Ok(p) => p,
        Err(s) => return Ok((2, s)),
    };
    let on = st(t.is_point_on_curve(k, surf_pnt, &acc.curves_3d))?
        .map_err(|_| Fail::Harness(harness("IsPointOnCurve failed")))?;
    if on {
        return Ok((3, 0.0));
    }
    let mut start = TracePoint::zeroed();
    start.point_type = TracePointType::Normal;
    if let Err(s) = st(t.compute_point_values(k, found_uv, &mut start, None, None))? {
        return Ok((4, s));
    }
    let start_id = t.base.new_point(start);
    t.base.start_point = Some(start_id);
    t.base.curve_trace_direction = 1.0;
    if let Err(s) = st(t.trace_curve(k, start_id, &acc.curves_3d))? {
        return Ok((5, s));
    }
    let start_type = t
        .base
        .point(start_id)
        .map_err(|e| Fail::Harness(harness(e.to_string())))?
        .point_type;
    if !t.base.curve_is_closed && start_type != TracePointType::Singularity {
        if let Err(s) = st(t.reverse_curve_direction())? {
            ign(flush_curve(&mut t.base, acc, None))?;
            return Ok((6, s));
        }
        t.base.curve_trace_direction = -1.0;
        let back = *t
            .base
            .curve_points
            .last()
            .ok_or_else(|| harness("empty curve after reversal"))?;
        ign(t.trace_curve(k, back, &acc.curves_3d))?;
        ign(t.reverse_curve_direction())?;
        t.base.curve_trace_direction = 1.0;
    }
    ign(flush_curve(&mut t.base, acc, None))?;
    t.base.start_point = None;
    Ok((0, 0.0))
}

fn run_op(
    t: &mut SectionTracer<'_>,
    k: &Kernel,
    op: &OpIn,
    acc: &mut TraceOutput,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match op {
        OpIn::Step { old } => {
            o.push(0.0);
            o.push(t.base.compute_step_size(*old));
        }
        OpIn::Trace { uv } => {
            let (stage, s) = trace_from(t, k, p2(*uv), acc)?;
            o.extend([
                f64::from(stage),
                s,
                b(t.base.curve_is_closed),
                acc.curves_3d.len() as f64,
            ]);
        }
        OpIn::Flush { points, curve } => {
            for p in points {
                let id = t.base.new_point(point_of(p)?);
                t.base.curve_points.push(id);
            }
            let c = curve.as_ref().map(rust_curve).transpose()?;
            match st(flush_curve(&mut t.base, acc, c.as_ref()))? {
                Ok(()) => o.push(0.0),
                Err(s) => o.push(s),
            }
            o.push(acc.curves_3d.len() as f64);
        }
        OpIn::AddCurve { curve } => {
            acc.curves_3d.push(rust_curve(curve)?);
            o.push(0.0);
        }
        OpIn::OnCurve { p } => {
            let on = st(t.is_point_on_curve(k, v3(*p), &acc.curves_3d))?
                .map_err(|_| Fail::Harness(harness("IsPointOnCurve failed")))?;
            o.push(0.0);
            o.push(b(on));
        }
        OpIn::Iso { v_dir, value } => {
            let param = if *v_dir {
                SurfParamType::V
            } else {
                SurfParamType::U
            };
            match st(t.test_iso_curve(k, param, *value))? {
                Ok(f) => o.extend([0.0, b(f)]),
                Err(s) => o.extend([s, 0.0]),
            }
        }
        OpIn::Start { points } => {
            for s in points {
                let mut p = TracePoint::zeroed();
                p.point_type = kind_of(s.kind)?;
                p.curve_uv[0] = p2(s.uv);
                p.curve_3d[1] = v3(s.t);
                let id = t.base.new_point(p);
                t.base
                    .add_start_point(id)
                    .map_err(|e| Fail::Harness(harness(e.to_string())))?;
            }
            o.push(0.0);
            put_start_points(o, &t.base)?;
        }
        OpIn::Test { point } => {
            let mut p = point_of(point)?;
            let _ = t.base.new_point(p);
            match st(t.test_point(k, &mut p))? {
                Ok(done) => {
                    o.push(0.0);
                    o.extend([b(done), b(t.base.curve_is_closed)]);
                    put_uv(o, p.curve_uv[0]);
                    put_vec(o, p.curve_3d[0]);
                    put_start_points(o, &t.base)?;
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::DoTrace => {
            // smsurf_GetSurfaceCache: the cache the trace runs on
            let mut cache = st(tracer_surface_cache::<CacheObj>(k, &t.base))?
                .map_err(|_| Fail::Harness(harness("surface cache lookup failed")))?;
            // DoTrace(ctx, &tol, &ang, ..) with the current tolerances
            let (tol, ang) = (t.base.approx_tol_3d, t.base.ang_tol_rad);
            match st(do_trace_base(t, k, &mut cache, Some(tol), Some(ang)))? {
                Ok(r) => {
                    o.push(0.0);
                    put_curves(o, &r.curves_3d);
                    put_curves(o, &r.uv_curves);
                    put_doubles(o, &r.max_drop_dists);
                    put_doubles(o, &r.deviations);
                }
                Err(s) => o.push(s),
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let surface = rust_surface(&i.surface)?;
    let base = match SurfaceTracer::new(&surface, &ext(i.domain)) {
        Ok(b) => b,
        Err(e) => {
            return Output::unported(&e)
                .ok_or_else(|| harness(format!("tracer construction: {e}")));
        }
    };
    let mut t = SectionTracer {
        base,
        p0: v3(i.p0),
        n: v3(i.n),
    };
    let mut o = vec![t.base.approx_tol_3d];
    if let Some(tol) = i.tol {
        t.base.approx_tol_3d = tol;
    }
    if let Some(ang) = i.ang {
        t.base.ang_tol_rad = ang;
    }
    let mut acc = TraceOutput::default();
    for op in &i.ops {
        o.push(op_code(op));
        match run_op(&mut t, &k, op, &mut acc, &mut o) {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => return Ok(*d),
            Err(Fail::Harness(h)) => return Err(h),
        }
    }
    o.extend([
        t.base.approx_tol_3d,
        t.base.ang_tol_rad,
        t.base.curve_trace_direction,
        b(t.base.curve_is_closed),
        t.base.curve_points.len() as f64,
    ]);
    match put_start_points(&mut o, &t.base) {
        Ok(()) => {}
        Err(Fail::Deferred(d)) => return Ok(*d),
        Err(Fail::Harness(h)) => return Err(h),
    }
    put_curves(&mut o, &acc.curves_3d);
    put_curves(&mut o, &acc.uv_curves);
    put_doubles(&mut o, &acc.max_drop_dists);
    put_doubles(&mut o, &acc.deviations);
    Ok(Output::new().values("rec", o))
}

/// The solution object of the trace's surface cache (its solves keep no objects).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CacheObj;

impl SolutionObject for CacheObj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

fn op_code(op: &OpIn) -> f64 {
    match op {
        OpIn::Step { .. } => 1.0,
        OpIn::Trace { .. } => 2.0,
        OpIn::Flush { .. } => 3.0,
        OpIn::AddCurve { .. } => 4.0,
        OpIn::OnCurve { .. } => 5.0,
        OpIn::Iso { .. } => 6.0,
        OpIn::Start { .. } => 7.0,
        OpIn::Test { .. } => 8.0,
        OpIn::DoTrace => 9.0,
    }
}

// ---- generation ---------------------------------------------------------------------

fn pick<T: Copy>(rng: &mut Rng, xs: &[T], default: T) -> T {
    rng.choose(xs).copied().unwrap_or(default)
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 && l <= 1.0 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

/// A clamped knot vector on `[a, b]` for `n1` control points of degree `p`.
fn gen_knots(rng: &mut Rng, n1: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let interior = n1 - p - 1;
    let mut inner: Vec<f64> = if rng.chance(0.5) {
        (1..=interior)
            .map(|i| a + (b - a) * i as f64 / (interior + 1) as f64)
            .collect()
    } else {
        (0..interior)
            .map(|_| a + (b - a) * rng.uniform(0.1, 0.9))
            .collect()
    };
    inner.sort_by(f64::total_cmp);
    let mut k = vec![a; p + 1];
    k.extend(inner);
    k.extend(vec![b; p + 1]);
    k
}

fn gen_bspline(rng: &mut Rng, scale: f64) -> SurfData {
    let n1 = 2 + rng.below(5) as usize;
    let m1 = 2 + rng.below(5) as usize;
    let p = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
    let q = 1 + rng.below(3.min(m1 as u64 - 1)) as usize;
    let rational = rng.chance(0.2);
    let amp = pick(rng, &[0.0, 0.2, 0.5, 1.0, 2.0], 0.5);
    let (a, c) = (rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0));
    let (u0, u1) = (a, a + rng.uniform(0.5, 6.0));
    let (v0, v1) = (c, c + rng.uniform(0.5, 6.0));
    let jitter = rng.uniform(0.0, 0.2);
    let net = (0..n1)
        .map(|i| {
            (0..m1)
                .map(|j| {
                    let x = i as f64 / (n1 - 1) as f64 + rng.uniform(-jitter, jitter) * 0.2;
                    let y = j as f64 / (m1 - 1) as f64 + rng.uniform(-jitter, jitter) * 0.2;
                    let z = amp * rng.uniform(-1.0, 1.0);
                    let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
                    [x * scale, y * scale, z * scale, w]
                })
                .collect()
        })
        .collect();
    SurfData {
        p,
        q,
        net,
        u: gen_knots(rng, n1, p, u0, u1),
        v: gen_knots(rng, m1, q, v0, v1),
    }
}

fn gen_surface(rng: &mut Rng, scale: f64) -> SurfIn {
    if rng.chance(0.25) {
        let x = unit_dir(rng);
        let mut y = unit_dir(rng);
        let dot = x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
        if dot.abs() > 0.9 {
            y = [x[1], -x[0], 0.0];
            if (y[0] * y[0] + y[1] * y[1]) < 1e-6 {
                y = [0.0, x[2], -x[1]];
            }
        }
        let o = rng.point3(-scale, scale);
        let s = [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)];
        let (u0, v0) = (
            rng.uniform(-2.0, 2.0) * scale,
            rng.uniform(-2.0, 2.0) * scale,
        );
        SurfIn::Plane {
            o,
            x,
            y,
            scale: s,
            domain: [
                u0,
                v0,
                u0 + rng.uniform(0.5, 4.0) * scale,
                v0 + rng.uniform(0.5, 4.0) * scale,
            ],
        }
    } else {
        SurfIn::Bspline(gen_bspline(rng, scale))
    }
}

fn natural(s: &SurfIn) -> [f64; 4] {
    match s {
        SurfIn::Plane { domain, .. } => *domain,
        SurfIn::Bspline(d) => [
            d.u.first().copied().unwrap_or(0.0),
            d.v.first().copied().unwrap_or(0.0),
            d.u.last().copied().unwrap_or(1.0),
            d.v.last().copied().unwrap_or(1.0),
        ],
    }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    if rng.chance(0.6) {
        return nat;
    }
    let (du, dv) = (nat[2] - nat[0], nat[3] - nat[1]);
    let a = rng.uniform(0.0, 0.4);
    let bb = rng.uniform(0.6, 1.0);
    let c = rng.uniform(0.0, 0.4);
    let d = rng.uniform(0.6, 1.0);
    [
        nat[0] + a * du,
        nat[1] + c * dv,
        nat[0] + bb * du,
        nat[1] + d * dv,
    ]
}

fn uv_in(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    // sometimes exactly on the boundary
    let snap = |rng: &mut Rng, lo: f64, hi: f64| match rng.below(8) {
        0 => lo,
        1 => hi,
        _ => rng.uniform(lo, hi),
    };
    [snap(rng, d[0], d[2]), snap(rng, d[1], d[3])]
}

fn gen_curve(rng: &mut Rng, scale: f64) -> CurveIn {
    if rng.chance(0.4) {
        let a = rng.point3(-scale, scale);
        let mut bb = rng.point3(-scale, scale);
        if a == bb {
            bb[0] += scale;
        }
        CurveIn::Segment { a, b: bb }
    } else {
        let n1 = 2 + rng.below(5) as usize;
        let p = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
        let cpts = (0..n1)
            .map(|_| {
                let q = rng.point3(-scale, scale);
                [q[0], q[1], q[2], NOW]
            })
            .collect();
        let end = rng.uniform(0.5, 3.0);
        CurveIn::Bspline(CurveData {
            p,
            cpts,
            knots: gen_knots(rng, n1, p, 0.0, end),
        })
    }
}

/// A synthetic curve point list with increasing parameters.
fn gen_flush_points(rng: &mut Rng, scale: f64) -> Vec<PointIn> {
    let kinds = [1u8, 1, 1, 1, 2, 3, 0];
    if rng.chance(0.15) {
        // a single (tangent or other) point
        let mut p = PointIn {
            kind: pick(rng, &[3u8, 3, 1], 3),
            param: rng.uniform(-1.0, 1.0),
            ..PointIn::default()
        };
        p.uv = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
        p.p = rng.point3(-scale, scale);
        p.drop = rng.uniform(0.0, 0.01) * scale;
        p.dev = rng.uniform(0.0, 0.01) * scale;
        return vec![p];
    }
    let count = 2 + rng.below(6) as usize;
    let close = rng.chance(0.15);
    let mut param = rng.uniform(-1.0, 1.0);
    let base = rng.point3(-scale, scale);
    let dir = unit_dir(rng);
    (0..count)
        .map(|i| {
            let step = if close { 1e-7 } else { rng.uniform(0.05, 0.5) };
            if i > 0 {
                param += step;
            }
            let s = param * scale;
            let wobble = rng.uniform(-0.05, 0.05);
            PointIn {
                kind: pick(rng, &kinds, 1),
                param,
                uv: [param, param * 0.5 + wobble],
                uv_tan: [1.0, 0.5],
                p: [
                    base[0] + dir[0] * s,
                    base[1] + dir[1] * s + wobble * scale,
                    base[2] + dir[2] * s,
                ],
                t: [dir[0] * scale, dir[1] * scale, dir[2] * scale],
                srf: [base[0] + dir[0] * s, base[1] + dir[1] * s, base[2]],
                nrm: unit_dir(rng),
                drop: rng.uniform(0.0, 0.01) * scale,
                dev: rng.uniform(0.0, 0.01) * scale,
            }
        })
        .collect()
}

fn gen_phase1_op(rng: &mut Rng, domain: [f64; 4], scale: f64) -> OpIn {
    match rng.below(6) {
        0 => OpIn::Flush {
            points: gen_flush_points(rng, scale),
            curve: rng.chance(0.4).then(|| gen_curve(rng, scale)),
        },
        1 => OpIn::Step {
            old: pick(rng, &[0.0, 1e-13, 0.25, -0.5], 0.0),
        },
        2 => {
            let v_dir = rng.chance(0.5);
            let (lo, hi) = if v_dir {
                (domain[1], domain[3])
            } else {
                (domain[0], domain[2])
            };
            let value = match rng.below(3) {
                0 => lo,
                1 => hi,
                _ => rng.uniform(lo, hi),
            };
            OpIn::Iso { v_dir, value }
        }
        3 => {
            let count = 1 + rng.below(5) as usize;
            let mut pts: Vec<StartIn> = Vec::new();
            for _ in 0..count {
                let reuse = !pts.is_empty() && rng.chance(0.4);
                let mut s = StartIn {
                    kind: pick(rng, &[1u8, 1, 2, 3], 1),
                    uv: uv_in(rng, domain),
                    t: unit_dir(rng),
                };
                if reuse {
                    if let Some(prev) = pts.last() {
                        s.uv = prev.uv;
                        if rng.chance(0.5) {
                            s.t = prev.t;
                        }
                    }
                }
                pts.push(s);
            }
            OpIn::Start { points: pts }
        }
        4 => OpIn::AddCurve {
            curve: gen_curve(rng, scale),
        },
        _ => OpIn::Test {
            point: PointIn {
                kind: pick(rng, &[1u8, 1, 1, 2, 3], 1),
                param: rng.uniform(0.0, 1.0),
                uv: uv_in(rng, domain),
                p: rng.point3(-scale, scale),
                ..PointIn::default()
            },
        },
    }
}

fn gen_inputs(rng: &mut Rng) -> Option<Inputs> {
    let scale = pick(rng, &[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3, 1e-6, 1e5], 1.0);
    let surface = gen_surface(rng, scale);
    let nat = natural(&surface);
    let domain = gen_domain(rng, nat);
    let s = rust_surface(&surface).ok()?;
    let uv0 = uv_in(rng, domain);
    let p0 = s.evaluate_point(p2(uv0)).ok()?;
    let mut p0 = [p0.x, p0.y, p0.z];
    if rng.chance(0.1) {
        // a plane missing the surface (or touching it far away)
        p0[2] += 10.0 * scale;
    }
    let n = match rng.below(6) {
        0 => [0.0, 0.0, 1.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0, 0.0, 0.0],
        _ => unit_dir(rng),
    };
    let tol = if rng.chance(0.25) {
        Some(pick(rng, &[1e-3, 1e-5, 1e-2, 0.0], 1e-3) * scale)
    } else {
        None
    };
    let ang = if rng.chance(0.2) {
        Some(pick(rng, &[0.1, 0.5, 1e-3, 1.5], 0.1))
    } else {
        None
    };

    // phase 1: no traces yet
    let mut ops: Vec<OpIn> = (0..rng.below(4))
        .map(|_| gen_phase1_op(rng, domain, scale))
        .collect();
    // phase 2: traces
    for _ in 0..=rng.below(5) {
        ops.push(OpIn::Trace {
            uv: uv_in(rng, domain),
        });
        if rng.chance(0.3) {
            let uv = uv_in(rng, domain);
            if let Ok(p) = s.evaluate_point(p2(uv)) {
                ops.push(OpIn::OnCurve { p: [p.x, p.y, p.z] });
            }
        }
        if rng.chance(0.2) {
            ops.push(OpIn::Test {
                point: PointIn {
                    kind: 1,
                    param: rng.uniform(0.0, 1.0),
                    uv: uv_in(rng, domain),
                    p: rng.point3(-scale, scale),
                    ..PointIn::default()
                },
            });
        }
    }
    if rng.chance(0.06) {
        ops.push(OpIn::DoTrace);
    }
    Some(Inputs {
        surface,
        domain,
        p0,
        n,
        tol,
        ang,
        ops,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_tracer"
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

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..8 {
            if let Some(i) = gen_inputs(rng) {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }
}
