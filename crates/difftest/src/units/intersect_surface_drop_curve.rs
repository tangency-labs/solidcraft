//! Runner for unit `intersect.surface_drop_curve`: `SmSurfaceDropCurve`.
//!
//! C++: `solidcore_ffi::units::intersect_surface_drop_curve` (upstream
//! `source/SMLib/src/SmSurfaceDropCurve.cpp`, compiled into the shim, on a real
//! `SmBSplineSurface` or `SmPlane` and a real `SmLine` or `SmBSplineCurve`).
//! Rust: `solidcore::intersect::surface_drop_curve`.
//!
//! A case is a surface, a tracer domain, a curve and its interval, the on-surface flag, optional
//! tolerances and working state (working interval, working domain, knot vector, breaks) and a
//! program of ops: `ComputePointValues` (with and without a previous point), `ComputeStepSize`,
//! `sm_AddBreak`, `sm_TestPoint`, `sm_IsGrazingIntersection`, `LocalPointSolve`,
//! `ComputeNextPoint`, `ComputeSpanDeviation`, `TraceCurve2`, `FindInteriorCurves`,
//! `FindBoundaryStartPoints`, `DoTrace` and the trivial `TestPoint`/`IsPointOnCurve`. Both sides
//! produce the record documented in the ffi module, reported as the values `rec`.
//!
//! Deferred cases (a case is deferred as a whole, so most programs avoid the deferring ops):
//! the surface point solves (`SmSurface::LocalPointSolve`, `surface.surface_p5`;
//! `SmSurface::GlobalPointSolve`, `surface.surface_p4`), so every `LocalPointSolve`,
//! `ComputeSpanDeviation`, `ComputeNextPoint` past its early exits, `FindBoundaryStartPoints`
//! over a non-degenerate interval and `DoTrace`; `SmSurface::IsSingularity` on B-spline surfaces
//! (`surface.surface_p5`); `SmSurface::DropCurve` on B-spline surfaces
//! (`intersect.surface_drop_lib_p3`); the projected boundary intersections over a plane
//! (`curve.crv_on_surf`).
//!
//! At least [`MIN_GENERATED`] generated cases run; generation only draws inputs the ffi
//! wrapper accepts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_drop_curve::{
    add_break, is_grazing_intersection, test_point, SurfaceDropCurve,
};
use solidcore::intersect::surface_tracer::{
    tracer_surface_cache, SurfaceTrace, TraceOutput, TracePoint, TracePointType,
};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_drop_curve::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 500;

/// `intersect.surface_drop_curve` runner.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Cpv {
        param: f64,
        uv: [f64; 2],
        #[serde(default)]
        use_prev: bool,
    },
    Step {
        param: f64,
        old: f64,
    },
    AddBreak {
        values: Vec<f64>,
        new: f64,
    },
    TestPoint {
        domain: [f64; 4],
        uv: [f64; 2],
        point: [f64; 3],
        tol: f64,
    },
    Graze {
        p1: f64,
        curve: CurveIn,
        p2: f64,
    },
    Lps {
        param: f64,
        guess: [f64; 2],
    },
    Next {
        #[serde(default)]
        first: bool,
        #[serde(default = "normal_kind")]
        kind: u8,
        step: f64,
    },
    Span {
        step: f64,
    },
    Trace2,
    Interior,
    Boundary,
    DoTrace,
    Hooks {
        point: [f64; 3],
    },
}

fn normal_kind() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SurfIn,
    domain: [f64; 4],
    curve: CurveIn,
    interval: [f64; 2],
    #[serde(default)]
    on_surface: bool,
    #[serde(default)]
    tol: Option<f64>,
    #[serde(default)]
    ang: Option<f64>,
    #[serde(default)]
    working_interval: Option<[f64; 2]>,
    #[serde(default)]
    working_domain: Option<[f64; 4]>,
    #[serde(default)]
    knots: Option<Vec<f64>>,
    #[serde(default)]
    breaks: Option<Vec<f64>>,
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

fn ffi_op(o: &OpIn) -> ffi::Op {
    match o {
        OpIn::Cpv {
            param,
            uv,
            use_prev,
        } => ffi::Op::Cpv {
            param: *param,
            uv: *uv,
            use_prev: *use_prev,
        },
        OpIn::Step { param, old } => ffi::Op::Step {
            param: *param,
            old: *old,
        },
        OpIn::AddBreak { values, new } => ffi::Op::AddBreak {
            values: values.clone(),
            new: *new,
        },
        OpIn::TestPoint {
            domain,
            uv,
            point,
            tol,
        } => ffi::Op::TestPoint {
            domain: *domain,
            uv: *uv,
            point: *point,
            tol: *tol,
        },
        OpIn::Graze { p1, curve, p2 } => ffi::Op::Graze {
            p1: *p1,
            curve: ffi_curve(curve),
            p2: *p2,
        },
        OpIn::Lps { param, guess } => ffi::Op::Lps {
            param: *param,
            guess: *guess,
        },
        OpIn::Next { first, kind, step } => ffi::Op::Next {
            first: *first,
            kind: *kind,
            step: *step,
        },
        OpIn::Span { step } => ffi::Op::Span { step: *step },
        OpIn::Trace2 => ffi::Op::Trace2,
        OpIn::Interior => ffi::Op::Interior,
        OpIn::Boundary => ffi::Op::Boundary,
        OpIn::DoTrace => ffi::Op::DoTrace,
        OpIn::Hooks { point } => ffi::Op::Hooks { point: *point },
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    ffi::Program {
        surface: ffi_surface(&i.surface),
        domain: i.domain,
        curve: ffi_curve(&i.curve),
        interval: i.interval,
        on_surface: i.on_surface,
        tol: i.tol,
        ang: i.ang,
        working_interval: i.working_interval,
        working_domain: i.working_domain,
        knots: i.knots.clone(),
        breaks: i.breaks.clone(),
        ops: i.ops.iter().map(ffi_op).collect(),
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
        } => {
            let mut p = Plane::new(
                v3(*o),
                v3(*x),
                v3(*y),
                Vec2::new(scale[0], scale[1]),
                &ext(*domain),
            );
            // with its NURB, as the shim makes it (a read-only query of the Rust plane
            // never keeps one; upstream's would, changing later point solves)
            p.make_nurb()
                .map_err(|e| harness(format!("plane NURB: {e}")))?;
            Ok(Surface::from(p))
        }
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
            let n = pw
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("empty curve"))?;
            let m = d
                .knots
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no knots"))?;
            BSplineCurve::from_data(3, n, m, d.p, &pw, &d.knots)
                .map(Curve::from)
                .map_err(|e| harness(format!("B-spline: {e}")))
        }
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
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

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

/// The shim's `put_point`.
fn put_point(o: &mut Vec<f64>, p: &TracePoint) {
    o.extend([
        f64::from(p.point_type as i32),
        p.curve_parameter,
        p.surf_drop_dist,
        p.surf_norm_line_dist,
    ]);
    put_vec(o, p.curve_3d[0]);
    put_vec(o, p.curve_3d[1]);
    put_uv(o, p.curve_uv[0]);
    put_uv(o, p.curve_uv[1]);
    let s = &p.surface;
    for v in [s[0][0], s[1][0], s[0][1], s[2][2]] {
        put_vec(o, v);
    }
}

/// The shim's `put_curve`.
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

fn put_opt_doubles(o: &mut Vec<f64>, a: Option<&Vec<f64>>) {
    o.push(b(a.is_some()));
    if let Some(a) = a {
        put_doubles(o, a);
    }
}

fn put_acc(o: &mut Vec<f64>, a: &TraceOutput) {
    put_curves(o, &a.curves_3d);
    put_curves(o, &a.uv_curves);
    put_doubles(o, &a.max_drop_dists);
    put_doubles(o, &a.deviations);
}

/// The start list value when a start point is also on the curve list (the shim's
/// `SHARED_START_LIST`): upstream's intrusive start list then walks the curve ring
/// (`DoTrace`'s `TraceCurve2` appends the start node it traces to `m_vCurvePoints`,
/// `plan/upstream-issues.md` WR-182), which the port's id lists do not model.
const SHARED_START_LIST: f64 = -3.0;

fn put_start_points(o: &mut Vec<f64>, t: &SurfaceDropCurve<'_>) -> Result<(), Fail> {
    if t.base
        .start_points
        .iter()
        .any(|id| t.base.curve_points.contains(id))
    {
        o.push(SHARED_START_LIST);
        return Ok(());
    }
    o.push(t.base.start_points.len() as f64);
    for &id in &t.base.start_points {
        let p = t
            .base
            .point(id)
            .map_err(|e| Fail::Harness(harness(e.to_string())))?;
        o.extend([f64::from(p.point_type as i32), p.curve_parameter]);
        put_uv(o, p.curve_uv[0]);
        put_uv(o, p.curve_uv[1]);
    }
    Ok(())
}

fn kind_of(code: u8) -> Result<TracePointType, HarnessError> {
    TracePointType::from_code(i32::from(code)).ok_or_else(|| harness("point type"))
}

/// The solution object of the drop's surface cache (its solves keep no objects).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CacheObj;

impl SolutionObject for CacheObj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

/// The ops' shared state: the last `Cpv` point and the accumulated outputs.
struct Run {
    last: Option<TracePoint>,
    acc: TraceOutput,
}

#[allow(clippy::too_many_lines)] // one match over the ops
fn run_op(
    t: &mut SurfaceDropCurve<'_>,
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    op: &OpIn,
    r: &mut Run,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match op {
        OpIn::Cpv {
            param,
            uv,
            use_prev,
        } => {
            let mut p = TracePoint::new();
            p.curve_parameter = *param;
            let mut prev = if *use_prev { r.last } else { None };
            match st(t.compute_point_values_with_prev(k, p2(*uv), &mut p, prev.as_mut()))? {
                Ok(()) => {
                    o.push(0.0);
                    put_point(o, &p);
                    o.push(b(prev.is_some()));
                    if let Some(pp) = prev {
                        put_uv(o, pp.curve_uv[1]);
                    }
                    r.last = Some(p);
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Step { param, old } => match st(t.step_size(*param, *old))? {
            Ok(v) => o.extend([0.0, v]),
            Err(s) => o.extend([s, 0.0]),
        },
        OpIn::AddBreak { values, new } => {
            let mut a = values.clone();
            add_break(&mut a, *new);
            o.push(0.0);
            put_doubles(o, &a);
        }
        OpIn::TestPoint {
            domain,
            uv,
            point,
            tol,
        } => match st(test_point(k, surface, &ext(*domain), p2(*uv), v3(*point), *tol))? {
            Ok(tp) => o.extend([
                0.0,
                b(tp.good_point),
                tp.surf_drop_dist,
                tp.surf_norm_line_dist,
            ]),
            Err(s) => o.push(s),
        },
        OpIn::Graze { p1, curve: c2, p2 } => {
            let c2 = rust_curve(c2)?;
            match st(is_grazing_intersection(k, curve, *p1, &c2, *p2))? {
                Ok(g) => o.extend([0.0, b(g)]),
                Err(s) => o.push(s),
            }
        }
        OpIn::Lps { param, guess } => {
            let mut nb = TracePoint::new();
            nb.curve_parameter = *param;
            match st(t.drop_local_point_solve(k, p2(*guess), &mut nb))? {
                Ok(found) => {
                    o.extend([0.0, b(found.is_some())]);
                    put_point(o, &nb);
                    if let Some(f) = found {
                        put_uv(o, f);
                    }
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Next { first, kind, step } => {
            let Some(lp) = r.last else {
                o.push(-1.0);
                return Ok(());
            };
            let id = t.base.new_point(TracePoint {
                point_type: kind_of(*kind)?,
                ..lp
            });
            if *first {
                t.base.add_point_to_curve(id);
            }
            let mut nx = TracePoint::new();
            match st(t.next_point(k, id, *step, &mut nx))? {
                Ok(np) => {
                    o.extend([
                        0.0,
                        b(np.found_good_point),
                        np.deviation,
                        np.angle_rad,
                        b(np.clipped),
                        b(np.boundary_hit),
                    ]);
                    put_point(o, &nx);
                    let cur = *t
                        .base
                        .point(id)
                        .map_err(|e| Fail::Harness(harness(e.to_string())))?;
                    put_point(o, &cur);
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Span { step } => {
            let Some(lp) = r.last else {
                o.push(-1.0);
                return Ok(());
            };
            match st(t.span_deviation(k, &lp, &lp, *step))? {
                Ok((dev, ok)) => {
                    o.extend([0.0, b(ok)]);
                    if ok {
                        o.push(dev.unwrap_or(0.0));
                    }
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Trace2 => {
            let Some(lp) = r.last else {
                o.push(-1.0);
                return Ok(());
            };
            let id = t.base.new_point(lp);
            match st(t.trace_curve_2(k, id, &mut r.acc))? {
                Ok(()) => o.push(0.0),
                Err(s) => o.push(s),
            }
        }
        OpIn::Interior => match st(t.interior_curves(k, &mut r.acc))? {
            Ok(()) => o.push(0.0),
            Err(s) => o.push(s),
        },
        OpIn::Boundary => {
            // smsurf_GetSurfaceCache: the cache the drop runs on
            let Ok(mut cache) = st(tracer_surface_cache::<CacheObj>(k, &t.base))? else {
                return Err(Fail::Harness(harness("surface cache build failed")));
            };
            match st(t.boundary_start_points(k, &mut cache, &mut r.acc))? {
                Ok(()) => {
                    o.push(0.0);
                    put_start_points(o, t)?;
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::DoTrace => {
            let Ok(mut cache) = st(tracer_surface_cache::<CacheObj>(k, &t.base))? else {
                return Err(Fail::Harness(harness("surface cache build failed")));
            };
            let (tol, ang) = (t.base.approx_tol_3d, t.base.ang_tol_rad);
            match st(t.drop_trace(k, &mut cache, Some(tol), Some(ang), None))? {
                Ok(out) => {
                    o.push(0.0);
                    put_acc(o, &out);
                }
                Err(s) => o.push(s),
            }
        }
        OpIn::Hooks { point } => {
            let mut p = TracePoint::new();
            let done = match st(SurfaceTrace::test_point(t, k, &mut p))? {
                Ok(d) => d,
                Err(s) => {
                    o.push(s);
                    return Ok(());
                }
            };
            let on = match st(t.is_point_on_curve(k, v3(*point), &r.acc.curves_3d))? {
                Ok(f) => f,
                Err(s) => {
                    o.push(s);
                    return Ok(());
                }
            };
            o.extend([0.0, b(done), b(on)]);
        }
    }
    Ok(())
}

fn op_code(op: &OpIn) -> f64 {
    match op {
        OpIn::Cpv { .. } => 1.0,
        OpIn::Step { .. } => 2.0,
        OpIn::AddBreak { .. } => 3.0,
        OpIn::TestPoint { .. } => 4.0,
        OpIn::Graze { .. } => 5.0,
        OpIn::Lps { .. } => 6.0,
        OpIn::Next { .. } => 7.0,
        OpIn::Span { .. } => 8.0,
        OpIn::Trace2 => 9.0,
        OpIn::Interior => 10.0,
        OpIn::Boundary => 11.0,
        OpIn::DoTrace => 12.0,
        OpIn::Hooks { .. } => 13.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let surface = rust_surface(&i.surface)?;
    let curve = rust_curve(&i.curve)?;
    let interval = Extent1d::from_min_max(i.interval[0], i.interval[1]);
    let mut t = match SurfaceDropCurve::new(&k, &surface, &ext(i.domain), &curve, &interval) {
        Ok(t) => t,
        Err(e) => {
            return Output::unported(&e)
                .ok_or_else(|| harness(format!("drop curve construction: {e}")));
        }
    };
    let mut o = vec![t.base.approx_tol_3d, t.base.ang_tol_rad];
    t.set_curve_is_on_surface(i.on_surface);
    if let Some(tol) = i.tol {
        t.base.approx_tol_3d = tol;
    }
    if let Some(ang) = i.ang {
        t.base.ang_tol_rad = ang;
    }
    if let Some([a, bb]) = i.working_interval {
        t.working_interval = Extent1d::from_min_max(a, bb);
    }
    if let Some(wd) = i.working_domain {
        t.working_uv_domain = ext(wd);
    }
    t.curve_knot_vector.clone_from(&i.knots);
    t.curve_breaks.clone_from(&i.breaks);
    let mut r = Run {
        last: None,
        acc: TraceOutput::default(),
    };
    for op in &i.ops {
        o.push(op_code(op));
        match run_op(&mut t, &k, &surface, &curve, op, &mut r, &mut o) {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => return Ok(*d),
            Err(Fail::Harness(h)) => return Err(h),
        }
    }
    o.extend([
        t.base.approx_tol_3d,
        t.base.ang_tol_rad,
        t.max_dist_to_surface(),
        b(t.curve_is_on_surface()),
        t.working_interval.min(),
        t.working_interval.max(),
    ]);
    put_uv(&mut o, t.working_uv_domain.min());
    put_uv(&mut o, t.working_uv_domain.max());
    put_opt_doubles(&mut o, t.curve_knot_vector.as_ref());
    put_opt_doubles(&mut o, t.curve_breaks.as_ref());
    o.push(t.base.curve_points.len() as f64);
    match put_start_points(&mut o, &t) {
        Ok(()) => {}
        Err(Fail::Deferred(d)) => return Ok(*d),
        Err(Fail::Harness(h)) => return Err(h),
    }
    put_acc(&mut o, &r.acc);
    Ok(Output::new().values("rec", o))
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
    // sometimes a C0 knot of full multiplicity p
    if p > 1 && interior >= p && rng.chance(0.2) {
        if let Some(&kv) = inner.first() {
            for slot in inner.iter_mut().take(p) {
                *slot = kv;
            }
        }
    }
    let mut k = vec![a; p + 1];
    k.extend(inner);
    k.extend(vec![b; p + 1]);
    k
}

fn gen_bspline_surface(rng: &mut Rng, scale: f64) -> SurfData {
    let n1 = 2 + rng.below(4) as usize;
    let m1 = 2 + rng.below(4) as usize;
    let p = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
    let q = 1 + rng.below(3.min(m1 as u64 - 1)) as usize;
    let rational = rng.chance(0.2);
    let amp = pick(rng, &[0.0, 0.05, 0.2, 0.5], 0.2);
    let (a, c) = (rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0));
    let (u0, u1) = (a, a + rng.uniform(0.5, 6.0));
    let (v0, v1) = (c, c + rng.uniform(0.5, 6.0));
    let net = (0..n1)
        .map(|i| {
            (0..m1)
                .map(|j| {
                    let x = i as f64 / (n1 - 1) as f64;
                    let y = j as f64 / (m1 - 1) as f64;
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

fn gen_plane(rng: &mut Rng, scale: f64) -> SurfIn {
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

/// A surface point at `uv` lifted `h` along the normal (the evaluation in the port).
fn lifted(k: &Kernel, surface: &Surface, uv: [f64; 2], h: f64) -> Option<[f64; 3]> {
    let (p, du, dv) = surface
        .evaluate_1st_derivatives(k, p2(uv), true, true)
        .ok()?;
    let n = du.cross(dv);
    let l = n.length();
    let q = if l > 1e-12 { p + n * (h / l) } else { p };
    Some([q.x, q.y, q.z])
}

/// A curve near the surface: control points over the domain lifted by up to `lift`.
fn gen_curve(k: &Kernel, rng: &mut Rng, surface: &Surface, d: [f64; 4], lift: f64) -> Option<CurveIn> {
    let pt = |rng: &mut Rng| {
        let uv = [rng.uniform(d[0], d[2]), rng.uniform(d[1], d[3])];
        lifted(k, surface, uv, lift * rng.uniform(-1.0, 1.0))
    };
    let segment = |a: [f64; 3], bb: [f64; 3]| {
        let l = ((bb[0] - a[0]).powi(2) + (bb[1] - a[1]).powi(2) + (bb[2] - a[2]).powi(2)).sqrt();
        let m = a
            .iter()
            .chain(bb.iter())
            .fold(1.0_f64, |m, x| m.max(x.abs()));
        (l > 1e-8 * m).then_some(CurveIn::Segment { a, b: bb })
    };
    match rng.below(10) {
        0..=2 => segment(pt(rng)?, pt(rng)?),
        3 => {
            // along an iso line: a UV tangent with a zero component (the snap branch)
            let (u0, u1) = (rng.uniform(d[0], d[2]), rng.uniform(d[0], d[2]));
            let (v0, v1) = (rng.uniform(d[1], d[3]), rng.uniform(d[1], d[3]));
            let h = lift * rng.uniform(-1.0, 1.0);
            if rng.chance(0.5) {
                segment(lifted(k, surface, [u0, v0], h)?, lifted(k, surface, [u1, v0], h)?)
            } else {
                segment(lifted(k, surface, [u0, v0], h)?, lifted(k, surface, [u0, v1], h)?)
            }
        }
        4 => {
            // along the surface normal: the dropped tangent vanishes
            let uv = [rng.uniform(d[0], d[2]), rng.uniform(d[1], d[3])];
            let h = (d[2] - d[0]).abs().max(1e-6) * 0.1;
            segment(lifted(k, surface, uv, -h)?, lifted(k, surface, uv, h)?)
        }
        _ => {
            let n1 = 2 + rng.below(5) as usize;
            let p = 1 + rng.below(3.min(n1 as u64 - 1)) as usize;
            let mut cpts = Vec::new();
            for _ in 0..n1 {
                let q = pt(rng)?;
                cpts.push([q[0], q[1], q[2], NOW]);
            }
            // sometimes a repeated end control point: a zero tangent at the end
            if n1 > 2 && rng.chance(0.25) {
                if let Some(&c0) = cpts.first() {
                    if let Some(c1) = cpts.get_mut(1) {
                        *c1 = c0;
                    }
                }
            }
            let start = pick(rng, &[0.0, 0.0, -1.0, 2.5], 0.0);
            let end = start + rng.uniform(0.5, 3.0);
            Some(CurveIn::Bspline(CurveData {
                p,
                cpts,
                knots: gen_knots(rng, n1, p, start, end),
            }))
        }
    }
}

fn curve_interval(c: &CurveIn) -> (f64, f64) {
    match c {
        CurveIn::Segment { .. } => (0.0, 1.0),
        CurveIn::Bspline(d) => (
            d.knots.first().copied().unwrap_or(0.0),
            d.knots.last().copied().unwrap_or(1.0),
        ),
    }
}

fn curve_knots(c: &CurveIn) -> Vec<f64> {
    match c {
        CurveIn::Segment { .. } => vec![0.0, 1.0],
        CurveIn::Bspline(d) => {
            let mut k = d.knots.clone();
            k.dedup();
            k
        }
    }
}

fn param_in(rng: &mut Rng, ivl: (f64, f64)) -> f64 {
    match rng.below(6) {
        0 => ivl.0,
        1 => ivl.1,
        _ => rng.uniform(ivl.0, ivl.1),
    }
}

/// The program flavours.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavour {
    /// Ops that compare real values on both sides.
    Light,
    /// Ops whose Rust side mostly defers (the C++ side still runs).
    Heavy,
}

#[allow(clippy::too_many_lines)] // one generator
fn gen_inputs(k: &Kernel, rng: &mut Rng) -> Option<Inputs> {
    let scale = pick(rng, &[1.0, 1.0, 1.0, 1e-3, 1e3], 1.0);
    let plane = rng.chance(0.65);
    let surf_in = if plane {
        gen_plane(rng, scale)
    } else {
        SurfIn::Bspline(gen_bspline_surface(rng, scale))
    };
    let surface = rust_surface(&surf_in).ok()?;
    let nat = natural(&surf_in);
    let domain = gen_domain(rng, nat);
    let lift = scale * pick(rng, &[0.0, 1e-9, 1e-4, 0.01, 0.3], 0.0);
    let curve = gen_curve(k, rng, &surface, domain, lift)?;
    let civl = curve_interval(&curve);
    let interval = match rng.below(6) {
        0 => {
            let t = param_in(rng, civl);
            [t, t]
        }
        1 => {
            let a = rng.uniform(civl.0, civl.1);
            let bb = rng.uniform(civl.0, civl.1);
            [a, bb]
        }
        _ => [civl.0, civl.1],
    };
    let on_surface = rng.chance(0.4);
    let tol = if rng.chance(0.4) {
        Some(scale * pick(rng, &[1e-6, 1e-4, 1e-2, 0.1], 1e-4))
    } else {
        None
    };
    let ang = if rng.chance(0.2) {
        Some(pick(rng, &[1e-3, 0.1, 0.35, 1.5], 0.35))
    } else {
        None
    };
    let flavour = if rng.chance(0.82) {
        Flavour::Light
    } else {
        Flavour::Heavy
    };
    let wi_lo = rng.uniform(civl.0, civl.1);
    let working_interval = if rng.chance(0.8) {
        Some(if rng.chance(0.5) {
            [civl.0, civl.1]
        } else {
            [wi_lo, rng.uniform(wi_lo, civl.1)]
        })
    } else {
        None
    };
    let bound = if plane { None } else { Some(nat) };
    let working_domain = if rng.chance(0.8) {
        Some(gen_domain(rng, bound.unwrap_or(domain)))
    } else {
        None
    };
    let knots = if rng.chance(0.75) {
        Some(if rng.chance(0.85) {
            curve_knots(&curve)
        } else {
            let mut k: Vec<f64> = (0..=rng.below(6))
                .map(|_| rng.uniform(civl.0, civl.1))
                .collect();
            k.sort_by(f64::total_cmp);
            k
        })
    } else {
        None
    };
    let breaks = if rng.chance(0.6) {
        let (lo, hi) = (interval[0].min(interval[1]), interval[0].max(interval[1]));
        let mut br = vec![lo, hi];
        if rng.chance(0.5) {
            br.insert(1, rng.uniform(lo, hi));
        }
        br.dedup();
        Some(br)
    } else {
        None
    };
    let mut knots_ok = knots.as_ref().is_some_and(|k| !k.is_empty());
    let mut breaks_ok = breaks.is_some();
    let n_ops = 1 + rng.below(7) as usize;
    let mut ops = Vec::new();
    if rng.chance(0.7) {
        ops.push(OpIn::Cpv {
            param: param_in(rng, civl),
            uv: uv_in(rng, bound.unwrap_or(domain)),
            use_prev: false,
        });
    }
    for _ in 0..n_ops {
        let pick_op = rng.below(if flavour == Flavour::Light { 11 } else { 14 });
        let op = match pick_op {
            0..=2 => OpIn::Cpv {
                param: param_in(rng, civl),
                uv: uv_in(rng, bound.unwrap_or(domain)),
                use_prev: rng.chance(0.5),
            },
            3 => {
                if !knots_ok {
                    continue;
                }
                let param = if rng.chance(0.3) {
                    *curve_knots(&curve)
                        .get(rng.below(2) as usize)
                        .unwrap_or(&civl.0)
                } else {
                    param_in(rng, civl)
                };
                OpIn::Step {
                    param,
                    old: pick(rng, &[0.0, 1e-13, 0.01, 0.2, 1.0, 10.0], 0.1) * (civl.1 - civl.0),
                }
            }
            4 => {
                let mut values: Vec<f64> =
                    (0..rng.below(6)).map(|_| rng.uniform(-2.0, 2.0)).collect();
                values.sort_by(f64::total_cmp);
                let new = match (rng.below(4), values.first().copied()) {
                    (0, Some(v)) => v,
                    (1, Some(v)) => v + 1e-11,
                    _ => rng.uniform(-3.0, 3.0),
                };
                OpIn::AddBreak { values, new }
            }
            5 => {
                let d = gen_domain(rng, bound.unwrap_or(domain));
                let uv = if rng.chance(0.2) {
                    [d[0] - 0.5 * (d[2] - d[0]), d[3] + 0.1]
                } else {
                    uv_in(rng, d)
                };
                let pnt = lifted(k, &surface, [uv[0].clamp(d[0], d[2]), uv[1].clamp(d[1], d[3])], scale * rng.uniform(-0.5, 0.5))
                    .unwrap_or([0.0; 3]);
                let jitter = scale * pick(rng, &[0.0, 1e-7, 1e-3, 0.1], 0.0);
                OpIn::TestPoint {
                    domain: d,
                    uv,
                    point: [
                        pnt[0] + jitter * rng.uniform(-1.0, 1.0),
                        pnt[1] + jitter * rng.uniform(-1.0, 1.0),
                        pnt[2] + jitter * rng.uniform(-1.0, 1.0),
                    ],
                    tol: scale * pick(rng, &[1e-6, 1e-3, 0.05], 1e-3),
                }
            }
            6 => {
                let c2 = if rng.chance(0.3) {
                    curve.clone()
                } else {
                    gen_curve(k, rng, &surface, domain, lift)?
                };
                let i2 = curve_interval(&c2);
                OpIn::Graze {
                    p1: param_in(rng, civl),
                    curve: c2,
                    p2: param_in(rng, i2),
                }
            }
            7 | 8 => {
                if working_domain.is_none() {
                    continue;
                }
                // early exits: a singular non-first point, a zero step, a step off the boundary
                OpIn::Next {
                    first: rng.chance(0.3),
                    kind: pick(rng, &[2, 2, 1, 0], 2),
                    step: pick(rng, &[0.0, 0.0, 1e-14, 0.1], 0.0) * (civl.1 - civl.0),
                }
            }
            9 => {
                if !plane {
                    OpIn::Hooks {
                        point: rng.point3(-scale, scale),
                    }
                } else if rng.chance(0.5) {
                    OpIn::Interior
                } else {
                    if !(breaks_ok && knots_ok) {
                        continue;
                    }
                    OpIn::Trace2
                }
            }
            10 => OpIn::Hooks {
                point: rng.point3(-scale, scale),
            },
            11 => {
                if working_domain.is_none() {
                    continue;
                }
                match rng.below(3) {
                    0 => OpIn::Lps {
                        param: param_in(rng, civl),
                        guess: uv_in(rng, bound.unwrap_or(domain)),
                    },
                    1 => OpIn::Span {
                        step: 0.1 * (civl.1 - civl.0),
                    },
                    _ => OpIn::Next {
                        first: rng.chance(0.5),
                        kind: 1,
                        step: 0.1 * (civl.1 - civl.0),
                    },
                }
            }
            12 => {
                knots_ok = true;
                breaks_ok = true;
                if rng.chance(0.5) {
                    OpIn::Boundary
                } else {
                    OpIn::DoTrace
                }
            }
            _ => {
                if !(breaks_ok && knots_ok) {
                    continue;
                }
                OpIn::Trace2
            }
        };
        ops.push(op);
    }
    Some(Inputs {
        surface: surf_in,
        domain,
        curve,
        interval,
        on_surface,
        tol,
        ang,
        working_interval,
        working_domain,
        knots,
        breaks,
        ops,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_drop_curve"
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
        let k = &Kernel::default();
        for _ in 0..16 {
            if let Some(i) = gen_inputs(k, rng) {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }
}
