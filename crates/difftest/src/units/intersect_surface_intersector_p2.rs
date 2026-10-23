//! Runner for unit `intersect.surface_intersector_p2`: `SmSurfaceIntersector`, part 2
//! (`ComputePointValues`, `ClassifyBoundaryStartPointIntersection`, `FlushCurve`,
//! `DoBoundaryPlaneTest`, `ComputeLawPointValues`, `sm_IsPointUnique`).
//!
//! C++: `solidcore_ffi::units::intersect_surface_intersector_p2` (a real
//! `SmSurfaceIntersector` on `SmPlane` / `SmBSplineSurface` / `SmCurveBoundedSurface`
//! objects). Rust: `solidcore::intersect::surface_intersector::p2`.
//!
//! A case is two surfaces (B may be A itself), their domains, the fillet flag, some
//! curves, a point arena and a program of operations; both sides produce the record
//! documented in the ffi module (statuses `0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce,
//! [`ffi::REJECTED`] for an op the shim does not run), reported as the values `rec`.
//! A case whose Rust side reaches an unported unit is deferred as a whole.
//!
//! Deferred cases: `ComputePointValues` reaching `SmSurface::IsSingularity` of a
//! B-spline surface or `SmSurface::LocalSurfaceSolve` (normals within 3 degrees of a
//! known previous point), `ClassifyBoundaryStartPointIntersection` sampling a tangent
//! boundary (`SmSurface::LocalPointSolve`), `DoBoundaryPlaneTest` on a plane
//! (`SmPlane::CreateIsoParametricCurve`), and `ComputeLawPointValues` with a valid
//! plane (`SmFilletLaw::Evaluate`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::{Extent1d, ExtentPointType};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{
    IntersectionPointType, SurfaceIntersector, TsectId, TsectPnt,
};
use solidcore::intersect::surface_intersector::p2::{
    is_point_unique, LawPointOptions, TsectCurves,
};
use solidcore::kernel::Kernel;
use solidcore::math::UNDEF_DOUBLE;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::PatchBoundaryBoundingPlanes;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_intersector_p2::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_intersector_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum SurfaceIn {
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    BSpline {
        p: usize,
        q: usize,
        /// Rows (u index) of `[x, y, z, w]`; `w` = `null` for a non-rational point.
        net: Vec<Vec<[Option<f64>; 4]>>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    /// An `SmCurveBoundedSurface` (implicit outer, no boundary curves) over a plane or
    /// B-spline basis.
    CurveBounded {
        basis: Box<SurfaceIn>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum CurveIn {
    Segment {
        a: [f64; 3],
        b: [f64; 3],
    },
    BSpline {
        dim: u32,
        p: usize,
        /// `[x, y, z, w]`; `w` = `null` for a non-rational point.
        cpts: Vec<[Option<f64>; 4]>,
        knots: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LawIn {
    param: f64,
    uv0: [f64; 2],
    uv1: [f64; 2],
    delta: f64,
    normals: [[f64; 3]; 2],
    average: bool,
    orient: bool,
    curve: usize,
    interval: [f64; 2],
    orientations: [f64; 2],
    skip: bool,
    i: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum OpIn {
    State,
    Point {
        i: usize,
    },
    AddCurve {
        i: usize,
    },
    AddStart {
        i: usize,
    },
    Cpv {
        uvs: [[f64; 2]; 2],
        i: usize,
        prev: Option<usize>,
        step: Option<f64>,
    },
    Classify {
        i: usize,
    },
    Flush,
    Unique {
        p: [f64; 3],
        tol: f64,
    },
    Bpt {
        tol: f64,
        bbox: [[f64; 3]; 2],
        dom1: [f64; 4],
        dom2: [f64; 4],
    },
    Law(LawIn),
    Members {
        trace_dir: f64,
        approx_tol: f64,
        start_tol: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    a: SurfaceIn,
    /// `None`: B is A itself.
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    fillet: bool,
    curves: Vec<CurveIn>,
    /// [`ffi::POINT_LEN`] values each.
    points: Vec<Vec<f64>>,
    ops: Vec<OpIn>,
}

fn weight(w: Option<f64>) -> f64 {
    w.unwrap_or(NOW)
}

fn cpt4(c: &[Option<f64>; 4]) -> [f64; 4] {
    [
        c[0].unwrap_or(f64::NAN),
        c[1].unwrap_or(f64::NAN),
        c[2].unwrap_or(f64::NAN),
        weight(c[3]),
    ]
}

fn ffi_surface(s: &SurfaceIn) -> ffi::SurfaceIn {
    match s {
        SurfaceIn::Plane {
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
        SurfaceIn::BSpline { p, q, net, u, v } => ffi::SurfaceIn::BSpline {
            p: *p,
            q: *q,
            net: net.iter().map(|r| r.iter().map(cpt4).collect()).collect(),
            u: u.clone(),
            v: v.clone(),
        },
        SurfaceIn::CurveBounded { basis } => {
            ffi::SurfaceIn::CurveBounded(Box::new(ffi_surface(basis)))
        }
    }
}

fn ffi_curve(c: &CurveIn) -> ffi::CurveIn {
    match c {
        CurveIn::Segment { a, b } => ffi::CurveIn::Segment(*a, *b),
        CurveIn::BSpline {
            dim,
            p,
            cpts,
            knots,
        } => ffi::CurveIn::BSpline {
            dim: *dim,
            p: *p,
            cpts: cpts.iter().map(cpt4).collect(),
            knots: knots.clone(),
        },
    }
}

fn ffi_op(o: &OpIn) -> ffi::Op {
    match o {
        OpIn::State => ffi::Op::State,
        OpIn::Point { i } => ffi::Op::Point(*i),
        OpIn::AddCurve { i } => ffi::Op::AddCurve(*i),
        OpIn::AddStart { i } => ffi::Op::AddStart(*i),
        OpIn::Cpv { uvs, i, prev, step } => ffi::Op::Cpv {
            uvs: *uvs,
            i: *i,
            prev: *prev,
            step: *step,
        },
        OpIn::Classify { i } => ffi::Op::Classify(*i),
        OpIn::Flush => ffi::Op::Flush,
        OpIn::Unique { p, tol } => ffi::Op::Unique { p: *p, tol: *tol },
        OpIn::Bpt {
            tol,
            bbox,
            dom1,
            dom2,
        } => ffi::Op::Bpt {
            tol: *tol,
            bbox: *bbox,
            dom1: *dom1,
            dom2: *dom2,
        },
        OpIn::Law(l) => ffi::Op::Law(ffi::LawOp {
            param: l.param,
            uv0: l.uv0,
            uv1: l.uv1,
            delta: l.delta,
            normals: l.normals,
            average: l.average,
            orient: l.orient,
            curve: l.curve,
            interval: l.interval,
            orientations: l.orientations,
            skip: l.skip,
            i: l.i,
        }),
        OpIn::Members {
            trace_dir,
            approx_tol,
            start_tol,
        } => ffi::Op::Members {
            trace_dir: *trace_dir,
            approx_tol: *approx_tol,
            start_tol: *start_tol,
        },
    }
}

fn ffi_program(i: &Inputs) -> ffi::Program {
    ffi::Program {
        a: ffi_surface(&i.a),
        b: i.b.as_ref().map(ffi_surface),
        dom1: i.dom1,
        dom2: i.dom2,
        fillet: i.fillet,
        curves: i.curves.iter().map(ffi_curve).collect(),
        points: i.points.clone(),
        ops: i.ops.iter().map(ffi_op).collect(),
    }
}

// ---- C++ side -----------------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&ffi_program(i)) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(harness(format!("C++: {e}"))),
    }
}

// ---- Rust side ----------------------------------------------------------------------------

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

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// Pushes the op status of `r`; `Ok(Some(v))` on success, a deferral for `Unported`.
fn st<T>(o: &mut Vec<f64>, r: solidcore::Result<T>) -> Result<Option<T>, Fail> {
    match r {
        Ok(v) => {
            o.push(0.0);
            Ok(Some(v))
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            o.push(status(&e));
            Ok(None)
        }
    }
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn rust_surface(s: &SurfaceIn) -> Result<Surface, Fail> {
    match s {
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            p2(*scale),
            &ext(*domain),
        ))),
        SurfaceIn::BSpline { p, q, net, u, v } => {
            let net = net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            let [x, y, z, w] = cpt4(c);
                            CPoint::new(x, y, z, w)
                        })
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
        SurfaceIn::CurveBounded { basis } => Ok(Surface::from(CurveBoundedSurface::new(
            rust_surface(basis)?,
            Vec::new(),
            Vec::new(),
            true,
        ).map_err(|e| HarnessError::new(e.to_string()))?)),
    }
}

fn rust_curve(c: &CurveIn) -> Result<Curve, Fail> {
    match c {
        CurveIn::Segment { a, b } => Line::create_line_segment(3, v3(*a), v3(*b), None)
            .map(Curve::from)
            .map_err(|e| Fail::Harness(harness(format!("segment: {e}")))),
        CurveIn::BSpline {
            dim,
            p,
            cpts,
            knots,
        } => {
            let pw: Vec<CPoint> = cpts
                .iter()
                .map(|c| {
                    let [x, y, z, w] = cpt4(c);
                    CPoint { x, y, z, w }
                })
                .collect();
            let n = pw
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("empty B-spline"))?;
            let m = knots.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            BSplineCurve::from_data(u64::from(*dim), n, m, *p, &pw, knots)
                .map(Curve::from)
                .map_err(|e| Fail::Harness(harness(format!("B-spline: {e}"))))
        }
    }
}

fn ept(code: f64) -> Result<ExtentPointType, HarnessError> {
    Ok(match code as i64 {
        0 => ExtentPointType::Start,
        1 => ExtentPointType::Inside,
        2 => ExtentPointType::End,
        3 => ExtentPointType::Both,
        4 => ExtentPointType::Outside,
        5 => ExtentPointType::Unknown,
        _ => return Err(harness("extent point type")),
    })
}

fn rust_point(v: &[f64]) -> Result<TsectPnt, HarnessError> {
    if v.len() != ffi::POINT_LEN {
        return Err(harness("point length"));
    }
    let mut it = v.iter().copied();
    let mut g = || it.next().unwrap_or(f64::NAN);
    let mut t = TsectPnt::new();
    t.point_type =
        IntersectionPointType::from_code(g() as i32).ok_or_else(|| harness("point type"))?;
    for pt in t.uv_param_type.iter_mut().flatten() {
        *pt = ept(g())?;
    }
    for uv in t.uv_curve_pv.iter_mut().flatten() {
        *uv = Vec2::new(g(), g());
    }
    for vec in t.surface_pv.iter_mut().flatten().flatten() {
        *vec = Vec3::new(g(), g(), g());
    }
    for vec in &mut t.curve_pv {
        *vec = Vec3::new(g(), g(), g());
    }
    t.curve_parameter = g();
    t.tangent_plane_angle_rad = g();
    t.deviation = g();
    for u in &mut t.user_doubles {
        *u = g();
    }
    Ok(t)
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

fn put_point(o: &mut Vec<f64>, t: &TsectPnt) {
    o.push(f64::from(t.point_type.code()));
    o.extend(t.uv_param_type.iter().flatten().map(|pt| f64::from(pt.code())));
    for uv in t.uv_curve_pv.iter().flatten() {
        put_uv(o, *uv);
    }
    for vec in t.surface_pv.iter().flatten().flatten() {
        put_vec(o, *vec);
    }
    put_vec(o, t.curve_pv[0]);
    put_vec(o, t.curve_pv[1]);
    o.extend([t.curve_parameter, t.tangent_plane_angle_rad, t.deviation]);
    o.extend(t.user_doubles);
}

fn put_list(o: &mut Vec<f64>, l: &[TsectId]) {
    o.push(l.len() as f64);
    o.extend(l.iter().map(|id| id.index() as f64));
}

fn put_state(o: &mut Vec<f64>, si: &SurfaceIntersector<'_>) {
    o.extend([
        si.this_approx_tol_3d,
        si.curve_trace_direction,
        si.start_parallel_tol_radians,
    ]);
    put_list(o, si.curve_points());
    put_list(o, si.start_points());
    o.push(si.points().len() as f64);
    for p in si.points() {
        put_point(o, p);
    }
}

fn put_bspline(o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    let CurveKind::BSpline(bs) = c.kind() else {
        return Err(Fail::Harness(harness("output curve is not a B-spline")));
    };
    o.extend([
        f64::from(bs.knot_type() as i32),
        f64::from(bs.form() as i32),
    ]);
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
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    Ok(())
}

fn put_curves(o: &mut Vec<f64>, cs: &[Curve], from: usize) -> Result<(), Fail> {
    let tail = cs.get(from..).unwrap_or(&[]);
    o.push(tail.len() as f64);
    for c in tail {
        put_bspline(o, c)?;
    }
    Ok(())
}

fn put_outs(o: &mut Vec<f64>, out: &TsectCurves, first: usize) -> Result<(), Fail> {
    put_curves(o, &out.curves_3d, first)?;
    put_curves(o, &out.uv_curves[0], 0)?;
    put_curves(o, &out.uv_curves[1], 0)?;
    o.push(out.curve_types.len() as f64);
    o.extend(out.curve_types.iter().map(|t| f64::from(*t as i32)));
    o.push(out.deviations.len() as f64);
    o.extend_from_slice(&out.deviations);
    Ok(())
}

fn id(i: usize) -> TsectId {
    TsectId(i)
}

/// The shim's `is_rejected_classify`.
fn classify_rejected(t: &TsectPnt) -> bool {
    let finite = |v: Vec2| v.x.is_finite() && v.y.is_finite();
    if t.uv_curve_pv.iter().flatten().any(|v| !finite(*v)) {
        return true;
    }
    let zero = |v: Vec2| v.x == 0.0 && v.y == 0.0;
    zero(t.uv_curve_pv[0][1]) && zero(t.uv_curve_pv[1][1])
}

fn pt_of(si: &SurfaceIntersector<'_>, i: usize) -> Result<TsectPnt, Fail> {
    si.point(id(i))
        .cloned()
        .map_err(|e| Fail::Harness(harness(format!("point: {e}"))))
}

fn set_pt(si: &mut SurfaceIntersector<'_>, i: usize, p: TsectPnt) -> Result<(), Fail> {
    *si.point_mut(id(i))
        .map_err(|e| Fail::Harness(harness(format!("point: {e}"))))? = p;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_op(
    k: &Kernel,
    si: &mut SurfaceIntersector<'_>,
    curves: &[Curve],
    outs: &mut TsectCurves,
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    match op {
        OpIn::State => {
            o.push(0.0);
            put_state(o, si);
        }
        OpIn::Point { i } => {
            let p = pt_of(si, *i)?;
            o.push(0.0);
            put_point(o, &p);
        }
        OpIn::AddCurve { i } | OpIn::AddStart { i } => {
            pt_of(si, *i)?;
            if si.is_linked(id(*i)) {
                o.push(ffi::REJECTED);
            } else {
                let r = if matches!(op, OpIn::AddCurve { .. }) {
                    si.add_point_to_curve(id(*i))
                } else {
                    si.add_start_point(k, id(*i))
                };
                st(o, r)?;
            }
        }
        OpIn::Cpv { uvs, i, prev, step } => {
            let mut p = pt_of(si, *i)?;
            let prev_p = match prev {
                Some(j) => Some(pt_of(si, *j)?),
                None => None,
            };
            let r = si.compute_point_values(&Kernel::default(), [p2(uvs[0]), p2(uvs[1])], &mut p, prev_p.as_ref(), *step);
            st(o, r)?;
            put_point(o, &p);
            set_pt(si, *i, p)?;
        }
        OpIn::Classify { i } => {
            let p = pt_of(si, *i)?;
            if classify_rejected(&p) {
                o.push(ffi::REJECTED);
            } else if let Some(d) = st(o, si.classify_boundary_start_point_intersection(k, &p))? {
                o.push(b(d));
            }
        }
        OpIn::Flush => {
            if st(o, si.flush_curve(k, outs))?.is_some() {
                put_outs(o, outs, curves.len())?;
            }
        }
        OpIn::Unique { p, tol } => {
            let starts = si
                .start_points()
                .iter()
                .map(|&j| pt_of(si, j.index()))
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<&TsectPnt> = starts.iter().collect();
            let r = is_point_unique(k, v3(*p), &refs, &outs.curves_3d, *tol);
            if let Some(f) = st(o, r)? {
                o.push(b(f));
            }
        }
        OpIn::Bpt {
            tol,
            bbox,
            dom1,
            dom2,
        } => {
            let mut bp1 = PatchBoundaryBoundingPlanes::new(ext(*dom1));
            let mut bp2 = PatchBoundaryBoundingPlanes::new(ext(*dom2));
            let bx = Extent3d::from_bounds(
                bbox[0][0], bbox[0][1], bbox[0][2], bbox[1][0], bbox[1][1], bbox[1][2],
            );
            let mut local = TsectCurves::default();
            let r = si.do_boundary_plane_test(k, &mut bp1, &mut bp2, *tol, &bx, &mut local);
            if let Some(res) = st(o, r)? {
                o.push(f64::from(res as i32));
                put_outs(o, &local, 0)?;
            }
        }
        OpIn::Law(l) => {
            let mut p = pt_of(si, l.i)?;
            let curve = curves
                .get(l.curve)
                .ok_or_else(|| harness("curve index"))?;
            let opts = LawPointOptions {
                law: &(),
                law_orient: l.orient,
                point_curve: curve,
                point_curve_interval: Extent1d::from_min_max(l.interval[0], l.interval[1]),
                normals: [v3(l.normals[0]), v3(l.normals[1])],
                average_normals: l.average,
                surface_orientations: l.orientations,
            };
            let r = si.compute_law_point_values(
                k, l.param,
                p2(l.uv0),
                p2(l.uv1),
                l.delta,
                &opts,
                l.skip,
                &mut p,
            );
            st(o, r)?;
            put_point(o, &p);
            set_pt(si, l.i, p)?;
        }
        OpIn::Members {
            trace_dir,
            approx_tol,
            start_tol,
        } => {
            si.curve_trace_direction = *trace_dir;
            si.this_approx_tol_3d = *approx_tol;
            si.start_parallel_tol_radians = *start_tol;
            o.push(0.0);
        }
    }
    Ok(())
}

fn op_code(op: &OpIn) -> f64 {
    f64::from(match op {
        OpIn::State => 1,
        OpIn::Point { .. } => 2,
        OpIn::AddCurve { .. } => 3,
        OpIn::AddStart { .. } => 4,
        OpIn::Cpv { .. } => 5,
        OpIn::Classify { .. } => 6,
        OpIn::Flush => 7,
        OpIn::Unique { .. } => 8,
        OpIn::Bpt { .. } => 9,
        OpIn::Law(_) => 10,
        OpIn::Members { .. } => 11,
    })
}

fn rust_run(i: &Inputs) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let a = rust_surface(&i.a)?;
    let b_srf = match &i.b {
        Some(s) => Some(rust_surface(s)?),
        None => None,
    };
    let b_ref = b_srf.as_ref().unwrap_or(&a);
    let curves = i
        .curves
        .iter()
        .map(rust_curve)
        .collect::<Result<Vec<_>, _>>()?;
    let mut o = Vec::new();
    let mut si = match SurfaceIntersector::new(&k, &a, &ext(i.dom1), b_ref, &ext(i.dom2), i.fillet) {
        Ok(si) => si,
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Err(Fail::Deferred(Box::new(d)));
            }
            return Err(Fail::Harness(harness(format!("constructor: {e}"))));
        }
    };
    for p in &i.points {
        si.alloc_point(rust_point(p)?);
    }
    let mut outs = TsectCurves {
        curves_3d: curves.clone(),
        ..TsectCurves::default()
    };
    for op in &i.ops {
        o.push(op_code(op));
        run_op(&k, &mut si, &curves, &mut outs, op, &mut o)?;
    }
    put_state(&mut o, &si);
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    // reject what the C++ side rejects
    ffi::encode(&ffi_program(i)).map_err(|e| harness(format!("rejected input: {e}")))?;
    match rust_run(i) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- generation -----------------------------------------------------------------------------

fn pick_f(rng: &mut Rng, items: &[f64]) -> f64 {
    rng.choose(items).copied().unwrap_or(1.0)
}

/// A length scale: mostly unit, sometimes tiny or huge.
fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 1e-3,
        1 => 1e3,
        2 => 1e-6,
        3 => 1e5,
        _ => rng.uniform(0.5, 4.0),
    }
}

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.2 {
            return [v[0] / l, v[1] / l, v[2] / l];
        }
    }
}

fn cross(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [1.0, 0.0, 0.0]
    }
}

fn plane_in(o: [f64; 3], x: [f64; 3], y: [f64; 3], domain: [f64; 4], scale: [f64; 2]) -> SurfaceIn {
    SurfaceIn::Plane {
        o,
        x,
        y,
        scale,
        domain,
    }
}

/// A random plane; `near` = a normal: the plane is tilted from it by `tilt` radians.
fn gen_plane(rng: &mut Rng, s: f64, near: Option<([f64; 3], f64)>) -> SurfaceIn {
    let o = rng.point3(-s, s);
    let (x, y) = match near {
        None => {
            let x = unit_vec(rng);
            let mut y = cross(unit_vec(rng), x);
            let l = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
            if l > 1e-3 {
                y = [y[0] / l, y[1] / l, y[2] / l];
            } else {
                y = norm(cross(x, [0.0, 0.0, 1.0]));
            }
            (x, y)
        }
        Some((n, tilt)) => {
            // a basis of the plane with normal n, then x tilted about y
            let a = if n[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let x0 = norm(cross(n, a));
            let y0 = cross(n, x0);
            let (c, sn) = (tilt.cos(), tilt.sin());
            let x = [
                x0[0] * c + n[0] * sn,
                x0[1] * c + n[1] * sn,
                x0[2] * c + n[2] * sn,
            ];
            (x, y0)
        }
    };
    let scale = if rng.chance(0.7) {
        [1.0, 1.0]
    } else {
        [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)]
    };
    let domain = if rng.chance(0.5) {
        [0.0, 0.0, s, s]
    } else {
        [-s * rng.uniform(0.2, 1.0), -s, s, s * rng.uniform(0.2, 2.0)]
    };
    plane_in(o, x, y, domain, scale)
}

fn clamped_knots(rng: &mut Rng, n: usize, p: usize, lo: f64, hi: f64) -> Vec<f64> {
    let interior = n - p - 1;
    let mut inner: Vec<f64> = (0..interior)
        .map(|i| lo + (hi - lo) * (i as f64 + 1.0 + rng.uniform(-0.3, 0.3)) / (interior as f64 + 1.0))
        .collect();
    inner.sort_by(f64::total_cmp);
    let mut k = vec![lo; p + 1];
    k.extend(inner);
    k.extend(vec![hi; p + 1]);
    k
}

fn param_range(rng: &mut Rng) -> (f64, f64) {
    if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-5.0, 5.0);
        (a, a + rng.uniform(0.5, 10.0))
    }
}

fn gen_bspline(rng: &mut Rng, s: f64, tube: bool) -> SurfaceIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let nu = p + 1 + rng.below(3) as usize;
    let nv = q + 1 + rng.below(3) as usize;
    let rational = rng.chance(0.2);
    let o = rng.point3(-s, s);
    let mut net = Vec::new();
    for i in 0..nu {
        let mut row = Vec::new();
        for j in 0..nv {
            let (x, y, z) = if tube {
                let a = core::f64::consts::TAU * (i % (nu - 1)) as f64 / (nu - 1) as f64;
                (s * a.cos(), s * a.sin(), s * j as f64 / (nv - 1) as f64)
            } else {
                (
                    s * i as f64 / (nu - 1) as f64,
                    s * j as f64 / (nv - 1) as f64,
                    s * rng.uniform(-0.3, 0.3),
                )
            };
            let w = if rational {
                Some(rng.uniform(0.5, 2.0))
            } else {
                None
            };
            let ww = w.unwrap_or(1.0);
            row.push([
                Some((o[0] + x) * ww),
                Some((o[1] + y) * ww),
                Some((o[2] + z) * ww),
                w,
            ]);
        }
        net.push(row);
    }
    let (ulo, uhi) = param_range(rng);
    let (vlo, vhi) = param_range(rng);
    let u = clamped_knots(rng, nu, p, ulo, uhi);
    let v = clamped_knots(rng, nv, q, vlo, vhi);
    SurfaceIn::BSpline { p, q, net, u, v }
}

/// A bump B-spline over `[x0, x0 + s] x [0, s]` with planar, curved boundaries (its
/// boundary rows have constant x or y and varying heights); `heights` gives the net's
/// z values, `mirror` flips x (so the patch lies at `x <= x0`).
fn bump_bspline(
    s: f64,
    x0: f64,
    deg: usize,
    heights: &[Vec<f64>],
    mirror: bool,
    range: [(f64, f64); 2],
) -> SurfaceIn {
    let nu = heights.len();
    let nv = heights.first().map_or(0, Vec::len);
    let mut net = Vec::new();
    for (i, hrow) in heights.iter().enumerate() {
        let mut row = Vec::new();
        for (j, &z) in hrow.iter().enumerate() {
            let fx = s * i as f64 / (nu - 1) as f64;
            let x = if mirror { x0 - fx } else { x0 + fx };
            let y = s * j as f64 / (nv - 1) as f64;
            row.push([Some(x), Some(y), Some(z), None]);
        }
        net.push(row);
    }
    let knots = |n: usize, (lo, hi): (f64, f64)| {
        let mut k = vec![lo; deg + 1];
        for i in 1..n - deg {
            k.push(lo + (hi - lo) * i as f64 / (n - deg) as f64);
        }
        k.extend(vec![hi; deg + 1]);
        k
    };
    let [ru, rv] = range;
    SurfaceIn::BSpline {
        p: deg,
        q: deg,
        net,
        u: knots(nu, ru),
        v: knots(nv, rv),
    }
}

fn natural(s: &SurfaceIn) -> [f64; 4] {
    match s {
        SurfaceIn::Plane { domain, .. } => *domain,
        SurfaceIn::BSpline { u, v, .. } => [
            u.first().copied().unwrap_or(0.0),
            v.first().copied().unwrap_or(0.0),
            u.last().copied().unwrap_or(1.0),
            v.last().copied().unwrap_or(1.0),
        ],
        SurfaceIn::CurveBounded { basis } => natural(basis),
    }
}

/// Wraps `s` in a curve-bounded surface.
fn bounded(s: SurfaceIn) -> SurfaceIn {
    SurfaceIn::CurveBounded { basis: Box::new(s) }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    let [a, b0, c, d] = nat;
    let (w, h) = (c - a, d - b0);
    match rng.below(10) {
        0 => [a + 0.1 * w, b0 + 0.2 * h, c - 0.3 * w, d - 0.1 * h],
        1 => [a - 0.2 * w, b0, c + 0.1 * w, d],
        2 => [a + 1e-13 * w, b0, c, d - 1e-13 * h],
        _ => nat,
    }
}

fn lerp_dom(d: [f64; 4], a: f64, c: f64) -> [f64; 2] {
    [d[0] + a * (d[2] - d[0]), d[1] + c * (d[3] - d[1])]
}

/// A UV point of `d`: inside, on a side or corner, just off it, or outside.
fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let edge = |rng: &mut Rng| pick_f(rng, &[0.0, 1.0, 1e-12, 1.0 - 1e-12, -1e-3, 1.001]);
    match rng.below(10) {
        0 => lerp_dom(d, edge(rng), rng.unit()),
        1 => lerp_dom(d, rng.unit(), edge(rng)),
        2 => lerp_dom(d, edge(rng), edge(rng)),
        3 => lerp_dom(d, rng.uniform(-0.3, 1.3), rng.uniform(-0.3, 1.3)),
        _ => lerp_dom(d, rng.uniform(0.02, 0.98), rng.uniform(0.02, 0.98)),
    }
}

/// A point: random UVs and tangents in the domains, a random type (mostly crossing),
/// matrices of plausible size, increasing parameters.
fn gen_point(rng: &mut Rng, doms: [[f64; 4]; 2], s: f64, param: f64) -> Vec<f64> {
    let mut v = Vec::with_capacity(ffi::POINT_LEN);
    v.push(if rng.chance(0.6) {
        2.0
    } else {
        rng.below(8) as f64
    });
    for _ in 0..4 {
        v.push(if rng.chance(0.6) {
            1.0
        } else {
            rng.below(6) as f64
        });
    }
    for d in doms {
        v.extend(gen_uv(rng, d));
        let t = if rng.chance(0.1) {
            [0.0, 0.0]
        } else {
            let a = rng.uniform(-1.0, 1.0);
            [a, rng.uniform(-1.0, 1.0)]
        };
        v.extend(t);
    }
    for _ in 0..2 {
        for k in 0..9 {
            let p = if k == 8 {
                unit_vec(rng)
            } else if rng.chance(0.1) {
                [UNDEF_DOUBLE; 3]
            } else {
                rng.point3(-s, s)
            };
            v.extend(p);
        }
    }
    v.extend(rng.point3(-s, s));
    v.extend(match rng.below(8) {
        0 => [0.0; 3],
        _ => unit_vec(rng),
    });
    v.push(param);
    v.push(rng.uniform(0.0, core::f64::consts::FRAC_PI_2));
    v.push(rng.uniform(0.0, 1e-3) * s);
    for _ in 0..4 {
        v.push(rng.uniform(-1.0, 1.0));
    }
    v
}

fn gen_curve(rng: &mut Rng, s: f64) -> CurveIn {
    if rng.below(3) == 0 {
        let a = rng.point3(-s, s);
        let d = unit_vec(rng);
        CurveIn::Segment {
            a,
            b: [a[0] + s * d[0], a[1] + s * d[1], a[2] + s * d[2]],
        }
    } else {
            let p = 1 + rng.below(3) as usize;
            let n = p + 1 + rng.below(3) as usize;
            let cpts = (0..n)
                .map(|i| {
                    let q = rng.point3(-s, s);
                    [Some(q[0] + i as f64 * s), Some(q[1]), Some(q[2]), None]
                })
                .collect();
            let knots = clamped_knots(rng, n, p, 0.0, 1.0);
            CurveIn::BSpline {
                dim: 3,
                p,
                cpts,
                knots,
            }
    }
}

struct GenCtx {
    doms: [[f64; 4]; 2],
    s: f64,
    np: usize,
    nc: usize,
    /// Both surfaces are planes (no unported singularity checks).
    planes: bool,
}

fn gen_cpv(rng: &mut Rng, g: &GenCtx, done: &[usize]) -> OpIn {
    let i = rng.below(g.np as u64) as usize;
    let uvs = if rng.chance(0.15) {
        // the same UV on both surfaces (self intersection, coincident planes)
        let u = gen_uv(rng, g.doms[0]);
        [u, u]
    } else {
        [gen_uv(rng, g.doms[0]), gen_uv(rng, g.doms[1])]
    };
    // a previous point: one computed before (a real trace) or any other
    let prev = if rng.chance(0.4) {
        let cand: Vec<usize> = done.iter().copied().filter(|&j| j != i).collect();
        if cand.is_empty() || rng.chance(0.2) {
            let j = rng.below(g.np as u64) as usize;
            (j != i).then_some(j)
        } else {
            rng.choose(&cand).copied()
        }
    } else {
        None
    };
    let step = rng
        .chance(if g.planes { 0.5 } else { 0.1 })
        .then(|| g.s * pick_f(rng, &[0.1, 1e-3, 1.0]));
    OpIn::Cpv { uvs, i, prev, step }
}

fn gen_members(rng: &mut Rng, s: f64) -> OpIn {
    OpIn::Members {
        trace_dir: if rng.chance(0.5) { 1.0 } else { -1.0 },
        approx_tol: s * pick_f(rng, &[1e-3, 1e-6, 0.05, 1e-10]),
        start_tol: pick_f(rng, &[1e-10, 1e-3, 0.06, 0.3, 0.0]),
    }
}

/// Ops for a trace-like program: evaluations, start/curve points, flushes and the
/// other predicates.
fn gen_ops(rng: &mut Rng, g: &GenCtx) -> Vec<OpIn> {
    let mut ops = Vec::new();
    let mut done: Vec<usize> = Vec::new();
    if rng.chance(0.4) {
        ops.push(gen_members(rng, g.s));
    }
    let n_ops = 6 + rng.below(20) as usize;
    while ops.len() < n_ops {
        let op = match rng.below(20) {
            0..=7 => {
                let op = gen_cpv(rng, g, &done);
                if let OpIn::Cpv { i, .. } = &op {
                    done.push(*i);
                }
                op
            }
            8 | 9 => OpIn::Classify {
                i: if done.is_empty() || rng.chance(0.3) {
                    rng.below(g.np as u64) as usize
                } else {
                    rng.choose(&done).copied().unwrap_or(0)
                },
            },
            10..=12 => OpIn::AddCurve {
                i: rng.below(g.np as u64) as usize,
            },
            13 => OpIn::AddStart {
                i: rng.below(g.np as u64) as usize,
            },
            14 | 15 => OpIn::Flush,
            16 => OpIn::Unique {
                p: rng.point3(-g.s, g.s),
                tol: g.s * pick_f(rng, &[1e-3, 0.5, 2.0]),
            },
            17 => OpIn::Point {
                i: rng.below(g.np as u64) as usize,
            },
            18 => OpIn::State,
            _ => gen_members(rng, g.s),
        };
        ops.push(op);
    }
    // a full trace: evaluate points in order and flush them
    if rng.chance(0.5) {
        let k = 2 + rng.below(4.min(g.np as u64 - 1)) as usize;
        let start = gen_uv(rng, g.doms[0]);
        let start1 = gen_uv(rng, g.doms[1]);
        let d0 = [rng.uniform(-0.2, 0.2), rng.uniform(-0.2, 0.2)];
        let d1 = [rng.uniform(-0.2, 0.2), rng.uniform(-0.2, 0.2)];
        let w0 = [g.doms[0][2] - g.doms[0][0], g.doms[0][3] - g.doms[0][1]];
        let w1 = [g.doms[1][2] - g.doms[1][0], g.doms[1][3] - g.doms[1][1]];
        for j in 0..k {
            let t = j as f64;
            let uvs = [
                [start[0] + t * d0[0] * w0[0], start[1] + t * d0[1] * w0[1]],
                [start1[0] + t * d1[0] * w1[0], start1[1] + t * d1[1] * w1[1]],
            ];
            let prev = (j > 0 && rng.chance(0.5)).then(|| j - 1);
            ops.push(OpIn::Cpv {
                uvs,
                i: j,
                prev,
                step: None,
            });
            ops.push(OpIn::AddCurve { i: j });
        }
        ops.push(OpIn::Flush);
        ops.push(OpIn::State);
    }
    if g.nc > 0 && rng.chance(0.1) {
        ops.push(gen_law(rng, g));
    }
    ops
}

fn gen_law(rng: &mut Rng, g: &GenCtx) -> OpIn {
    // mostly a zero plane normal (compared); else deferred at the fillet law
    let zero = rng.chance(0.85);
    let normals = if zero {
        [[0.0; 3], [0.0; 3]]
    } else {
        [unit_vec(rng), unit_vec(rng)]
    };
    OpIn::Law(LawIn {
        param: rng.uniform(0.0, 1.0),
        uv0: gen_uv(rng, g.doms[0]),
        uv1: gen_uv(rng, g.doms[1]),
        delta: pick_f(rng, &[1e-3, 1e-6, 0.1]),
        normals,
        average: zero || rng.chance(0.5),
        orient: rng.chance(0.5),
        curve: rng.below(g.nc as u64) as usize,
        interval: if rng.chance(0.8) {
            [0.0, 1.0]
        } else {
            [0.5, 0.5]
        },
        orientations: [
            if rng.chance(0.5) { 1.0 } else { -1.0 },
            if rng.chance(0.5) { 1.0 } else { -1.0 },
        ],
        skip: rng.chance(0.5),
        i: rng.below(g.np as u64) as usize,
    })
}

fn gen_points(rng: &mut Rng, doms: [[f64; 4]; 2], s: f64) -> Vec<Vec<f64>> {
    let n = 3 + rng.below(6) as usize;
    let step = pick_f(rng, &[1.0, 0.25, 3.0, 1e-9]);
    (0..n)
        .map(|i| {
            let t = i as f64 * step;
            let param = if rng.chance(0.05) {
                0.0
            } else {
                t * (1.0 + rng.uniform(-0.01, 0.01))
            };
            gen_point(rng, doms, s, param)
        })
        .collect()
}

fn heights(rng: &mut Rng, n: usize, s: f64, flat_row0: Option<&[f64]>) -> Vec<Vec<f64>> {
    (0..n)
        .map(|i| {
            if i == 0 {
                if let Some(r) = flat_row0 {
                    return r.to_vec();
                }
            }
            (0..n).map(|_| s * rng.uniform(-0.3, 0.3)).collect()
        })
        .collect()
}

/// Two bump patches meeting (or not) along `x = s`: disjoint, touching (sharing the
/// boundary row or not), overlapping, slightly tilted, or a straight-edged patch.
fn gen_bpt_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let deg = 2 + rng.below(2) as usize;
    let n = deg + 1 + rng.below(3) as usize;
    let h1 = heights(rng, n, s, None);
    let shared: Vec<f64> = h1.last().cloned().unwrap_or_default();
    let mode = rng.below(6);
    let gap = match mode {
        0 => s * pick_f(rng, &[0.5, 1e-3, 1e-7]),
        1 | 2 => 0.0,
        3 => -s * 0.2,
        _ => s * pick_f(rng, &[0.0, 1e-12]),
    };
    let h2 = heights(rng, n, s, (mode == 1 || mode >= 4).then_some(&shared[..]));
    let r1 = [param_range(rng), param_range(rng)];
    let r2 = [param_range(rng), param_range(rng)];
    let a = bump_bspline(s, 0.0, deg, &h1, false, r1);
    let b = if mode == 5 && rng.chance(0.5) {
        // a straight boundary: collinear boundary samples
        let flat: Vec<Vec<f64>> = (0..n).map(|_| vec![0.0; n]).collect();
        bump_bspline(s, 2.0 * s + gap, deg, &flat, true, r2)
    } else {
        // mirrored copy starting at x = s + gap and running to larger x: its u = 0 row is
        // at x = s + gap
        let mut hh = h2;
        hh.reverse();
        bump_bspline(s, 2.0 * s + gap, deg, &hh, true, r2)
    };
    // a curve-bounded surface is not an SmBSplineSurface: SM_BR_UNKNOWN
    let (a, b) = match rng.below(10) {
        0 => (bounded(a), b),
        1 => (a, bounded(b)),
        _ => (a, b),
    };
    let na = natural(&a);
    let nb = natural(&b);
    let dom1 = if rng.chance(0.85) { na } else { gen_domain(rng, na) };
    let dom2 = if rng.chance(0.85) { nb } else { gen_domain(rng, nb) };
    let mut ops = Vec::new();
    let nbpt = 1 + rng.below(2) as usize;
    for _ in 0..nbpt {
        let bmin = [-s, -s, -s];
        let bmax = [3.0 * s, 2.0 * s, s];
        // a zero tolerance with an empty box is 0 / 0: a NaN curve intersection
        // tolerance, on which the C++ and the Rust agree (case adv_graze_nan)
        let empty_box = rng.chance(0.1);
        let tol = s * pick_f(rng, &[1e-3, 1e-6, 0.1, 0.0]);
        ops.push(OpIn::Bpt {
            tol,
            bbox: [bmin, if empty_box { bmin } else { bmax }],
            dom1: if rng.chance(0.85) { dom1 } else { gen_domain(rng, na) },
            dom2: if rng.chance(0.85) { dom2 } else { gen_domain(rng, nb) },
        });
    }
    let doms = [na, nb];
    let points = gen_points(rng, doms, s);
    let g = GenCtx {
        doms,
        s,
        np: points.len(),
        nc: 0,
        planes: false,
    };
    if rng.chance(0.3) {
        ops.push(gen_cpv(rng, &g, &[]));
    }
    Inputs {
        a,
        b: Some(b),
        dom1,
        dom2,
        fillet: false,
        curves: Vec::new(),
        points,
        ops,
    }
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let scenario = rng.below(20);
    if scenario < 4 {
        return Some(gen_bpt_case(rng));
    }
    let s = gen_scale(rng);
    let (a, b) = match scenario {
        4..=10 => {
            let a = gen_plane(rng, s, None);
            let b = match rng.below(8) {
                // nearly parallel planes (tangent and singular paths)
                0 => {
                    let SurfaceIn::Plane { x, y, .. } = &a else {
                        return None;
                    };
                    let n = norm(cross(*x, *y));
                    let tilt = pick_f(rng, &[0.0, 1e-9, 1e-3, 0.03]);
                    Some(gen_plane(rng, s, Some((n, tilt))))
                }
                1 => None,
                _ => Some(gen_plane(rng, s, None)),
            };
            (a, b)
        }
        11..=13 => (gen_plane(rng, s, None), Some(gen_bspline(rng, s, false))),
        14..=16 => (gen_bspline(rng, s, false), Some(gen_plane(rng, s, None))),
        17 => (gen_bspline(rng, s, true), Some(gen_bspline(rng, s, false))),
        _ => (gen_bspline(rng, s, false), Some(gen_bspline(rng, s, false))),
    };
    // curve-bounded surfaces over either kind (their basis answers every query)
    let (a, b) = match rng.below(8) {
        0 => (bounded(a), b),
        1 => (a, b.map(bounded)),
        2 => (bounded(a), b.map(bounded)),
        _ => (a, b),
    };
    let na = natural(&a);
    let nb = natural(b.as_ref().unwrap_or(&a));
    let dom1 = gen_domain(rng, na);
    let dom2 = gen_domain(rng, nb);
    let doms = [na, nb];
    let nc = rng.below(3) as usize;
    let curves: Vec<CurveIn> = (0..nc).map(|_| gen_curve(rng, s)).collect();
    let points = gen_points(rng, doms, s);
    let is_plane = |x: &SurfaceIn| matches!(x, SurfaceIn::Plane { .. });
    let g = GenCtx {
        doms,
        s,
        np: points.len(),
        nc,
        planes: is_plane(&a) && b.as_ref().is_none_or(is_plane),
    };
    let ops = gen_ops(rng, &g);
    Some(Inputs {
        a,
        b,
        dom1,
        dom2,
        fillet: rng.chance(0.1),
        curves,
        points,
        ops,
    })
}

// ---- inline edge cases ------------------------------------------------------------------

fn plane_json(o: [f64; 3], x: [f64; 3], y: [f64; 3], d: [f64; 4]) -> Value {
    json!({"kind": "plane", "o": o, "x": x, "y": y, "scale": [1.0, 1.0], "domain": d})
}

/// A default-like point with parameter `param` (UVs and tangents zero, type unknown).
fn blank_point(param: f64) -> Vec<f64> {
    let mut v = vec![1.0, 5.0, 5.0, 5.0, 5.0];
    v.extend([0.0; 8]);
    v.extend([0.0; 54]);
    v.extend([0.0; 6]);
    v.extend([param, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    v
}

fn edge_cases() -> Vec<(String, Value)> {
    let xy = plane_json([0.0, -0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0]);
    let xz = plane_json([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let xy2 = plane_json([0.0, -0.5, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0]);
    let d = [0.0, 0.0, 1.0, 1.0];
    let pts: Vec<Vec<f64>> = (0..6).map(|i| blank_point(0.2 * f64::from(i))).collect();
    let prog = |a: &Value, bb: &Value, ops: Value| {
        json!({"a": a, "b": bb, "dom1": d, "dom2": d, "fillet": false,
               "curves": [{"kind": "segment", "a": [0.0, 0.0, 0.0], "b": [1.0, 0.0, 0.0]}],
               "points": pts, "ops": ops})
    };
    let cpv = |u: f64, i: usize, prev: Option<usize>| {
        json!({"op": "cpv", "uvs": [[u, 0.5], [u, 0.5]], "i": i, "prev": prev, "step": null})
    };
    let mut v = Vec::new();
    // an exact trace along x, flushed into a curve
    let mut trace = Vec::new();
    for i in 0..5 {
        trace.push(cpv(0.25 * i as f64, i, (i > 0).then(|| i - 1)));
        trace.push(json!({"op": "add_curve", "i": i}));
    }
    trace.push(json!({"op": "flush"}));
    trace.push(json!({"op": "unique", "p": [0.0, 0.0, 0.0], "tol": 1e-3}));
    trace.push(json!({"op": "unique", "p": [0.5, 0.0, 0.0], "tol": 1e-3}));
    trace.push(json!({"op": "state"}));
    v.push(("edge_trace_flush".into(), prog(&xy, &xz, Value::Array(trace))));
    // start points on the natural boundaries (direction flips), classification
    v.push((
        "edge_boundary_starts".into(),
        prog(
            &xy,
            &xz,
            json!([{"op": "cpv", "uvs": [[0.0, 0.5], [0.0, 0.5]], "i": 0, "prev": null, "step": null},
                   {"op": "cpv", "uvs": [[1.0, 0.5], [1.0, 0.5]], "i": 1, "prev": null, "step": null},
                   {"op": "members", "trace_dir": -1.0, "approx_tol": 1e-3, "start_tol": 1e-10},
                   {"op": "cpv", "uvs": [[1.0, 0.5], [1.0, 0.5]], "i": 2, "prev": null, "step": null},
                   {"op": "cpv", "uvs": [[0.5, 0.0], [0.5, 1.0]], "i": 3, "prev": null, "step": null},
                   {"op": "classify", "i": 0}, {"op": "classify", "i": 1},
                   {"op": "classify", "i": 2}, {"op": "classify", "i": 5},
                   {"op": "add_start", "i": 0}, {"op": "add_curve", "i": 1}, {"op": "flush"},
                   {"op": "add_curve", "i": 0}, {"op": "flush"}, {"op": "state"}]),
        ),
    ));
    // parallel planes: the singularity point is SM_ERR, a tangent point when the
    // start tolerance is zero
    v.push((
        "edge_parallel_planes".into(),
        prog(
            &xy,
            &xy2,
            json!([{"op": "cpv", "uvs": [[0.3, 0.5], [0.3, 0.5]], "i": 0, "prev": null, "step": null},
                   {"op": "members", "trace_dir": 1.0, "approx_tol": 1e-3, "start_tol": 0.0},
                   {"op": "cpv", "uvs": [[0.3, 0.5], [0.3, 0.5]], "i": 1, "prev": null, "step": null},
                   {"op": "classify", "i": 1}, {"op": "add_curve", "i": 1}, {"op": "flush"}]),
        ),
    ));
    // a fillet law with a zero plane normal; flush with non-increasing parameters
    v.push((
        "edge_law_and_rejected_flush".into(),
        prog(
            &xy,
            &xz,
            json!([{"op": "law", "param": 0.5, "uv0": [0.5, 0.5], "uv1": [0.5, 0.5], "delta": 1e-3,
                    "normals": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], "average": true, "orient": false,
                    "curve": 0, "interval": [0.0, 1.0], "orientations": [1.0, -1.0], "skip": true, "i": 0},
                   {"op": "cpv", "uvs": [[0.1, 0.5], [0.1, 0.5]], "i": 2, "prev": null, "step": null},
                   {"op": "cpv", "uvs": [[0.9, 0.5], [0.9, 0.5]], "i": 1, "prev": null, "step": null},
                   {"op": "add_curve", "i": 2}, {"op": "add_curve", "i": 1}, {"op": "flush"},
                   {"op": "state"}]),
        ),
    ));
    // the boundary plane test of two bump patches sharing a boundary row
    // every boundary row is an arch (planar, not straight)
    let h = [
        [0.0, 0.4, 0.4, 0.0],
        [0.4, 0.1, 0.3, 0.4],
        [0.4, 0.2, 0.1, 0.4],
        [0.0, 0.4, 0.4, 0.0],
    ];
    let bump = |x0: f64, rows: &[[f64; 4]], mirror: bool| {
        let hs: Vec<Vec<f64>> = rows.iter().map(|r| r.to_vec()).collect();
        serde_json::to_value(bump_bspline(1.0, x0, 2, &hs, mirror, [(0.0, 1.0), (0.0, 1.0)]))
            .unwrap_or(Value::Null)
    };
    let mut rev = h;
    rev.reverse();
    let bpt = json!([{"op": "bpt", "tol": 1e-3, "bbox": [[-1.0, -1.0, -1.0], [3.0, 2.0, 1.0]],
                      "dom1": d, "dom2": d}, {"op": "state"}]);
    for (name, x0) in [("edge_bpt_shared", 2.0), ("edge_bpt_disjoint", 2.5), ("edge_bpt_overlap", 1.8)] {
        v.push((
            name.into(),
            json!({"a": bump(0.0, &h, false), "b": bump(x0, &rev, true), "dom1": d, "dom2": d,
                   "fillet": false, "curves": [], "points": pts, "ops": bpt}),
        ));
    }
    v
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_intersector_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Some(i) = gen_case(rng) else { continue };
            if ffi::encode(&ffi_program(&i)).is_ok() {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            v.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
