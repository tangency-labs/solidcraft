//! Runner for unit `intersect.surface_intersector_p1`: `SmTsectPnt` and
//! `SmSurfaceIntersector`, part 1.
//!
//! C++: `solidcore_ffi::units::intersect_surface_intersector_p1` (a real
//! `SmSurfaceIntersector` on `SmPlane` / `SmBSplineSurface` objects, plus the
//! file-static helpers). Rust: `solidcore::intersect::surface_intersector::p1`.
//!
//! A case is two surfaces (B may be A itself), their domains, the fillet flag, some
//! curves, a point arena and a program of operations; both sides produce the record
//! documented in the ffi module (statuses `0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce,
//! [`ffi::REJECTED`] for linking a point twice), reported as the values `rec`. A case
//! whose Rust side reaches an unported unit is deferred as a whole.
//!
//! Deferred cases: `RefinePoint` of crossing, tangent-curve and singular points
//! (`SmSurface::LocalSurfaceIntersect`, `surface.surface_p11`),
//! `StepBackFromSingularity` (`SmSurface::LocalPointSolve`, `surface.surface_p5`),
//! `sm_ComputeCurveType` with a B-spline surface (`SmSurface::EvaluateNormal`,
//! `surface.surface_p7`) or with parallel normals (`SmSurface::EvaluateGeometric`,
//! `surface.surface_p3`), `IsPointOnCurve` near (but not within tolerance of) a curve
//! (`SmSurface::DropPoint`, `surface.surface_p5`) and `AddStartPoint` pairing a
//! bounded with an exact start point (`smsurf_EvaluateNormalSection`,
//! `surface.surface_p1`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::ExtentPointType;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{
    compute_curve_type, flush_crv, is_point_on_tsect_curve, IntersectionPointType, SrfIndex,
    SurfaceIntersector, TsectId, TsectPnt,
};
use solidcore::kernel::Kernel;
use solidcore::math::{EFF_ZERO, UNDEF_DOUBLE};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_intersector_p1::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_intersector_p1` runner.
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
#[serde(rename_all = "snake_case", tag = "op")]
enum OpIn {
    State,
    Point {
        i: usize,
    },
    New,
    Copy {
        i: usize,
    },
    Assign {
        dst: usize,
        src: usize,
    },
    IsBounded {
        i: usize,
    },
    Classify {
        i: usize,
        uvs: [[f64; 2]; 2],
        surfaces: [bool; 2],
        domains: [[f64; 4]; 2],
        tol: Option<f64>,
    },
    SetExt {
        d: f64,
    },
    SetEdges {
        a: bool,
        b: bool,
    },
    Delta {
        a: usize,
        b: usize,
    },
    Singularity {
        uvs: [[f64; 2]; 2],
        i: usize,
        prev: Option<usize>,
    },
    StepSize {
        i: usize,
        old: f64,
    },
    Refine {
        uv0: [f64; 2],
        uv1: [f64; 2],
        ip_type: u8,
        normal: Option<[f64; 3]>,
        prev: Option<[[f64; 2]; 2]>,
        boundary: bool,
    },
    TestSpan {
        a: usize,
        b: usize,
    },
    AddCurve {
        i: usize,
    },
    AddStart {
        i: usize,
    },
    AddThrough {
        i: usize,
    },
    Reverse,
    OnStart {
        i: usize,
        list: u8,
        skip: Option<usize>,
    },
    OnTsectCurve {
        p: usize,
        prev: usize,
        next: usize,
        tol: f64,
    },
    OnCurve {
        p: [f64; 3],
        skip: Option<usize>,
    },
    CurveType {
        i: usize,
        j: usize,
    },
    Flush {
        dim: u32,
        poly: Vec<[f64; 3]>,
        knots: Vec<f64>,
    },
    StepBack {
        v_dir: bool,
        srf: u8,
        current: bool,
        prev: usize,
        curr: usize,
        step: f64,
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
        OpIn::New => ffi::Op::New,
        OpIn::Copy { i } => ffi::Op::Copy(*i),
        OpIn::Assign { dst, src } => ffi::Op::Assign {
            dst: *dst,
            src: *src,
        },
        OpIn::IsBounded { i } => ffi::Op::IsBounded(*i),
        OpIn::Classify {
            i,
            uvs,
            surfaces,
            domains,
            tol,
        } => ffi::Op::Classify {
            i: *i,
            uvs: *uvs,
            surfaces: *surfaces,
            domains: *domains,
            tol: *tol,
        },
        OpIn::SetExt { d } => ffi::Op::SetExt(*d),
        OpIn::SetEdges { a, b } => ffi::Op::SetEdges(*a, *b),
        OpIn::Delta { a, b } => ffi::Op::Delta(*a, *b),
        OpIn::Singularity { uvs, i, prev } => ffi::Op::Singularity {
            uvs: *uvs,
            i: *i,
            prev: *prev,
        },
        OpIn::StepSize { i, old } => ffi::Op::StepSize(*i, *old),
        OpIn::Refine {
            uv0,
            uv1,
            ip_type,
            normal,
            prev,
            boundary,
        } => ffi::Op::Refine {
            uv0: *uv0,
            uv1: *uv1,
            ip_type: *ip_type,
            normal: *normal,
            prev: *prev,
            boundary: *boundary,
        },
        OpIn::TestSpan { a, b } => ffi::Op::TestSpan(*a, *b),
        OpIn::AddCurve { i } => ffi::Op::AddCurve(*i),
        OpIn::AddStart { i } => ffi::Op::AddStart(*i),
        OpIn::AddThrough { i } => ffi::Op::AddThrough(*i),
        OpIn::Reverse => ffi::Op::Reverse,
        OpIn::OnStart { i, list, skip } => ffi::Op::OnStart {
            i: *i,
            list: *list,
            skip: *skip,
        },
        OpIn::OnTsectCurve {
            p,
            prev,
            next,
            tol,
        } => ffi::Op::OnTsectCurve(*p, *prev, *next, *tol),
        OpIn::OnCurve { p, skip } => ffi::Op::OnCurve { p: *p, skip: *skip },
        OpIn::CurveType { i, j } => ffi::Op::CurveType(*i, *j),
        OpIn::Flush { dim, poly, knots } => ffi::Op::Flush {
            dim: *dim,
            poly: poly.clone(),
            knots: knots.clone(),
        },
        OpIn::StepBack {
            v_dir,
            srf,
            current,
            prev,
            curr,
            step,
        } => ffi::Op::StepBack {
            v_dir: *v_dir,
            srf: *srf,
            current: *current,
            prev: *prev,
            curr: *curr,
            step: *step,
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// `Ok(v)`, an op status (pushed, `None`), or a deferral.
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
            let n = pw.len().checked_sub(1).ok_or_else(|| harness("empty B-spline"))?;
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

fn put_dom(o: &mut Vec<f64>, e: &Extent2d) {
    o.extend([e.min().x, e.min().y, e.max().x, e.max().y]);
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
    put_dom(o, &si.uv_domains[0]);
    put_dom(o, &si.uv_domains[1]);
    o.extend([
        b(si.closed_u[0]),
        b(si.closed_u[1]),
        b(si.closed_v[0]),
        b(si.closed_v[1]),
        b(si.use_surface_edges[0]),
        b(si.use_surface_edges[1]),
        si.this_ang_tol_rad,
        si.this_approx_tol_3d,
        si.stepping_parallel_tol_radians,
        si.start_parallel_tol_radians,
        si.curve_trace_direction,
        si.last_through_parameter,
        si.extension_distance,
        b(si.doing_extension_clipping),
        b(si.curve_is_closed),
        b(si.check_for_interior_curves),
        b(si.doing_self_intersection),
        b(si.clipped_by_through_point),
        b(si.do_boundary_point_on_curve_test),
        si.start_point.map_or(-1.0, |id| id.index() as f64),
        si.end_point.map_or(-1.0, |id| id.index() as f64),
    ]);
    put_list(o, si.curve_points());
    put_list(o, si.start_points());
    put_list(o, si.through_points());
    o.push(si.points().len() as f64);
    for p in si.points() {
        put_point(o, p);
    }
}

fn put_bspline(o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    let CurveKind::BSpline(bs) = c.kind() else {
        return Err(Fail::Harness(harness("flushed curve is not a B-spline")));
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

fn id(i: usize) -> TsectId {
    TsectId(i)
}

fn srf(s: u8) -> SrfIndex {
    if s == 0 {
        SrfIndex::First
    } else {
        SrfIndex::Second
    }
}

fn run_op(
    k: &Kernel,
    si: &mut SurfaceIntersector<'_>,
    curves: &[Curve],
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    let pt = |si: &SurfaceIntersector<'_>, i: usize| -> Result<TsectPnt, Fail> {
        si.point(id(i))
            .cloned()
            .map_err(|e| Fail::Harness(harness(format!("point: {e}"))))
    };
    match op {
        OpIn::State => {
            o.push(0.0);
            put_state(o, si);
        }
        OpIn::Point { i } => {
            let p = pt(si, *i)?;
            o.push(0.0);
            put_point(o, &p);
        }
        OpIn::New => {
            let n = si.alloc_point(TsectPnt::new());
            o.extend([0.0, n.index() as f64]);
        }
        OpIn::Copy { i } => {
            let p = pt(si, *i)?;
            let n = si.alloc_point(p);
            o.extend([0.0, n.index() as f64]);
        }
        OpIn::Assign { dst, src } => {
            pt(si, *dst)?;
            let s = pt(si, *src)?;
            if si.is_linked(id(*dst)) {
                o.push(ffi::REJECTED);
            } else {
                si.point_mut(id(*dst))
                    .map_err(|e| Fail::Harness(harness(format!("{e}"))))?
                    .assign(&s);
                o.push(0.0);
            }
        }
        OpIn::IsBounded { i } => {
            let p = pt(si, *i)?;
            o.extend([0.0, b(p.is_bounded())]);
        }
        OpIn::Classify {
            i,
            uvs,
            surfaces,
            domains,
            tol,
        } => {
            let s0 = si.surface(SrfIndex::First);
            let s1 = si.surface(SrfIndex::Second);
            let srfs = [surfaces[0].then_some(s0), surfaces[1].then_some(s1)];
            let doms = [ext(domains[0]), ext(domains[1])];
            let p = si
                .point_mut(id(*i))
                .map_err(|e| Fail::Harness(harness(format!("{e}"))))?;
            p.classify_point_params(
                [p2(uvs[0]), p2(uvs[1])],
                srfs,
                &doms,
                tol.unwrap_or(EFF_ZERO),
            );
            o.push(0.0);
            o.extend(p.uv_param_type.iter().flatten().map(|pt| f64::from(pt.code())));
        }
        OpIn::SetExt { d } => {
            si.set_extension_distance(*d);
            o.push(0.0);
        }
        OpIn::SetEdges { a, b } => {
            si.set_use_surface_edges(*a, *b);
            o.push(0.0);
        }
        OpIn::Delta { a, b } => {
            pt(si, *a)?;
            pt(si, *b)?;
            st(o, si.compute_point_values_delta())?;
        }
        OpIn::Singularity { uvs, i, prev } => {
            pt(si, *i)?;
            st(
                o,
                si.compute_singularity_point([p2(uvs[0]), p2(uvs[1])], id(*i), prev.map(id)),
            )?;
        }
        OpIn::StepSize { i, old } => {
            pt(si, *i)?;
            o.extend([0.0, si.compute_step_size(*old)]);
        }
        OpIn::Refine {
            uv0,
            uv1,
            ip_type,
            normal,
            prev,
            boundary,
        } => {
            let t = IntersectionPointType::from_code(i32::from(*ip_type))
                .ok_or_else(|| harness("point type"))?;
            let prev_uv = prev.map(|pv| [p2(pv[0]), p2(pv[1])]);
            let r = si.refine_point(
                k,
                p2(*uv0),
                p2(*uv1),
                t,
                normal.map(v3),
                prev_uv.as_ref().map(|a| &a[..]),
                *boundary,
            );
            if let Some(found) = st(o, r)? {
                match found {
                    None => o.push(0.0),
                    Some([a, c]) => {
                        o.push(1.0);
                        put_uv(o, a);
                        put_uv(o, c);
                    }
                }
            }
        }
        OpIn::TestSpan { a, b: bb } => {
            pt(si, *a)?;
            pt(si, *bb)?;
            if let Some(r) = st(o, si.test_span_accuracy(k, id(*a), id(*bb)))? {
                o.extend([
                    b(r.found_good_point),
                    r.deviation_found.unwrap_or(ffi::UNSET),
                    r.angle_found_rad,
                ]);
            }
        }
        OpIn::AddCurve { i } | OpIn::AddStart { i } | OpIn::AddThrough { i } => {
            pt(si, *i)?;
            if si.is_linked(id(*i)) {
                o.push(ffi::REJECTED);
            } else {
                let r = match op {
                    OpIn::AddCurve { .. } => si.add_point_to_curve(id(*i)),
                    OpIn::AddStart { .. } => si.add_start_point(k, id(*i)),
                    _ => si.add_through_point(id(*i)),
                };
                st(o, r)?;
            }
        }
        OpIn::Reverse => {
            st(o, si.reverse_curve_direction())?;
        }
        OpIn::OnStart { i, list, skip } => {
            let p = pt(si, *i)?;
            let l: Vec<TsectId> = match list {
                0 => si.start_points().to_vec(),
                1 => si.through_points().to_vec(),
                _ => si.curve_points().to_vec(),
            };
            if let Some(f) = st(o, si.is_point_on_start_point(&p, &l, *skip))? {
                o.push(b(f));
            }
        }
        OpIn::OnTsectCurve {
            p,
            prev,
            next,
            tol,
        } => {
            let (a, c, d) = (pt(si, *p)?, pt(si, *prev)?, pt(si, *next)?);
            if let Some(f) = st(o, is_point_on_tsect_curve(k, &a, &c, &d, *tol))? {
                o.push(b(f));
            }
        }
        OpIn::OnCurve { p, skip } => {
            if let Some(f) = st(o, si.is_point_on_curve(k, v3(*p), curves, *skip))? {
                o.push(b(f));
            }
        }
        OpIn::CurveType { i, j } => {
            let (Some(c1), Some(c2)) = (curves.get(*i), curves.get(*j)) else {
                return Err(Fail::Harness(harness("curve index")));
            };
            let r = compute_curve_type(
                k, si.surface(SrfIndex::First),
                si.surface(SrfIndex::Second),
                c1,
                c2,
            );
            if let Some(t) = st(o, r)? {
                o.push(f64::from(t as i32));
            }
        }
        OpIn::Flush { dim, poly, knots } => {
            let pts: Vec<Vec3> = poly.iter().map(|p| v3(*p)).collect();
            let mut out = Vec::new();
            if st(o, flush_crv(u64::from(*dim), &pts, knots, &mut out))?.is_some() {
                o.push(out.len() as f64);
                for c in &out {
                    put_bspline(o, c)?;
                }
            }
        }
        OpIn::StepBack {
            v_dir,
            srf: s,
            current,
            prev,
            curr,
            step,
        } => {
            pt(si, *prev)?;
            pt(si, *curr)?;
            let dir = if *v_dir {
                SurfParamType::V
            } else {
                SurfParamType::U
            };
            st(
                o,
                si.step_back_from_singularity(&Kernel::default(), dir, srf(*s), *current, id(*prev), id(*curr), *step),
            )?;
        }
    }
    Ok(())
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
    o.push(0.0);
    for p in &i.points {
        si.alloc_point(rust_point(p)?);
    }
    for op in &i.ops {
        o.push(op_code(op));
        run_op(&k, &mut si, &curves, op, &mut o)?;
    }
    put_state(&mut o, &si);
    Ok(o)
}

fn op_code(op: &OpIn) -> f64 {
    f64::from(match op {
        OpIn::State => 1,
        OpIn::Point { .. } => 2,
        OpIn::New => 3,
        OpIn::Copy { .. } => 4,
        OpIn::Assign { .. } => 5,
        OpIn::IsBounded { .. } => 6,
        OpIn::Classify { .. } => 7,
        OpIn::SetExt { .. } => 8,
        OpIn::SetEdges { .. } => 9,
        OpIn::Delta { .. } => 10,
        OpIn::Singularity { .. } => 11,
        OpIn::StepSize { .. } => 12,
        OpIn::Refine { .. } => 13,
        OpIn::TestSpan { .. } => 14,
        OpIn::AddCurve { .. } => 15,
        OpIn::AddStart { .. } => 16,
        OpIn::AddThrough { .. } => 17,
        OpIn::Reverse => 18,
        OpIn::OnStart { .. } => 19,
        OpIn::OnTsectCurve { .. } => 20,
        OpIn::OnCurve { .. } => 21,
        OpIn::CurveType { .. } => 22,
        OpIn::Flush { .. } => 23,
        OpIn::StepBack { .. } => 24,
    })
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

fn pick_u8(rng: &mut Rng, items: &[u8]) -> u8 {
    rng.choose(items).copied().unwrap_or(0)
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

fn gen_plane(rng: &mut Rng, s: f64) -> SurfaceIn {
    let o = rng.point3(-s, s);
    let x = unit_vec(rng);
    let mut y = cross(unit_vec(rng), x);
    let l = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
    if l > 1e-3 {
        y = [y[0] / l, y[1] / l, y[2] / l];
    } else {
        y = cross(x, [0.0, 0.0, 1.0]);
    }
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
    SurfaceIn::Plane {
        o,
        x,
        y,
        scale,
        domain,
    }
}

fn clamped_knots(rng: &mut Rng, n: usize, p: usize, lo: f64, hi: f64) -> Vec<f64> {
    let interior = n - p - 1;
    let mut inner: Vec<f64> = (0..interior)
        .map(|i| {
            lo + (hi - lo) * (i as f64 + 1.0 + rng.uniform(-0.3, 0.3)) / (interior as f64 + 1.0)
        })
        .collect();
    inner.sort_by(f64::total_cmp);
    let mut k = vec![lo; p + 1];
    k.extend(inner);
    k.extend(vec![hi; p + 1]);
    k
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
    let (ulo, uhi) = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-5.0, 5.0);
        (a, a + rng.uniform(0.5, 10.0))
    };
    let (vlo, vhi) = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-5.0, 5.0);
        (a, a + rng.uniform(0.5, 10.0))
    };
    let u = clamped_knots(rng, nu, p, ulo, uhi);
    let v = clamped_knots(rng, nv, q, vlo, vhi);
    SurfaceIn::BSpline { p, q, net, u, v }
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
    }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    let [a, b0, c, d] = nat;
    let (w, h) = (c - a, d - b0);
    match rng.below(10) {
        0 => [a + 0.1 * w, b0 + 0.2 * h, c - 0.3 * w, d - 0.1 * h],
        1 => [a - 0.2 * w, b0, c + 0.1 * w, d],
        2 => [c + w, d + h, c + 2.0 * w, d + 2.0 * h],
        3 => [a, b0, a, d],
        4 => [a + 1e-13 * w, b0, c, d - 1e-13 * h],
        _ => nat,
    }
}

/// Rust evaluation of a generated surface (for consistent points); `None` on failure.
fn eval_srf(k: &Kernel, s: &Surface, uv: [f64; 2]) -> Option<[[f64; 3]; 6]> {
    let m = s
        .evaluate(
            k, p2(uv),
            2,
            2,
            solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags::default(),
        )
        .ok()?;
    let g = |i: usize, j: usize| {
        let v = m.get(i)?.get(j)?;
        Some([v.x, v.y, v.z])
    };
    Some([g(0, 0)?, g(1, 0)?, g(0, 1)?, g(2, 0)?, g(1, 1)?, g(0, 2)?])
}

fn lerp_dom(d: [f64; 4], a: f64, c: f64) -> [f64; 2] {
    [d[0] + a * (d[2] - d[0]), d[1] + c * (d[3] - d[1])]
}

/// One surface's part of a generated point: `uv duv matrix[3][3]` values, the surface
/// point and the curve tangent it implies.
fn gen_srf_part(k: &Kernel, 
    rng: &mut Rng,
    srf: &Surface,
    dom: [f64; 4],
    base: [f64; 2],
    dir: [f64; 2],
    t: f64,
    v: &mut Vec<f64>,
) -> ([f64; 3], [f64; 3]) {
    let mut uv = [base[0] + t * dir[0], base[1] + t * dir[1]];
    if rng.chance(0.05) {
        uv = lerp_dom(dom, rng.uniform(-0.2, 1.2), rng.uniform(-0.2, 1.2));
    }
    v.extend(uv);
    v.extend(dir);
    let fallback = rng.point3(-1.0, 1.0);
    let [p, du, dv, duu, duv, dvv] = eval_srf(k, srf, uv).unwrap_or([fallback; 6]);
    let n = cross(du, dv);
    let tan = [
        du[0] * dir[0] + dv[0] * dir[1],
        du[1] * dir[0] + dv[1] * dir[1],
        du[2] * dir[0] + dv[2] * dir[1],
    ];
    // [0][0] pos [0][1] dv [0][2] dvv [1][0] du [1][1] duv [1][2] unused [2][0] duu
    // [2][1] unused [2][2] normal
    let unused = if rng.chance(0.5) {
        [UNDEF_DOUBLE; 3]
    } else {
        rng.point3(-1.0, 1.0)
    };
    for vec in [p, dv, dvv, du, duv, unused, duu, unused, n] {
        v.extend(vec);
    }
    (p, tan)
}

/// A point: consistent with the surfaces along a UV line, perturbed, or random.
#[allow(clippy::too_many_arguments)]
fn gen_point(k: &Kernel, 
    rng: &mut Rng,
    srfs: &[Surface; 2],
    doms: [[f64; 4]; 2],
    base: [[f64; 2]; 2],
    dir: [[f64; 2]; 2],
    t: f64,
    param: f64,
) -> Vec<f64> {
    let mut v = Vec::with_capacity(ffi::POINT_LEN);
    let ptype = if rng.chance(0.6) {
        2.0
    } else {
        rng.below(8) as f64
    };
    v.push(ptype);
    for _ in 0..4 {
        v.push(if rng.chance(0.6) {
            1.0
        } else {
            rng.below(6) as f64
        });
    }
    // the UV parts come first for both surfaces, then the matrices
    let mut uvs = Vec::new();
    let mut mats = Vec::new();
    let [s0, s1] = srfs;
    let (p0, tan0) = gen_srf_part(k, rng, s0, doms[0], base[0], dir[0], t, &mut mats);
    let (p1, _) = gen_srf_part(k, rng, s1, doms[1], base[1], dir[1], t, &mut mats);
    // gen_srf_part writes `uv duv` then the 27 matrix values: split them
    for chunk in [mats.get(..31), mats.get(31..)].into_iter().flatten() {
        uvs.extend(chunk.iter().take(4));
    }
    v.extend(&uvs);
    for chunk in [mats.get(4..31), mats.get(35..)].into_iter().flatten() {
        v.extend(chunk);
    }
    let crv_pos = if rng.chance(0.8) {
        p0
    } else {
        [(p0[0] + p1[0]) / 2.0, (p0[1] + p1[1]) / 2.0, (p0[2] + p1[2]) / 2.0]
    };
    v.extend(crv_pos);
    let crv_tan = match rng.below(10) {
        0 => [0.0; 3],
        1 => rng.point3(-1.0, 1.0),
        _ => tan0,
    };
    v.extend(crv_tan);
    v.push(param);
    v.push(match rng.below(4) {
        0 => 0.0,
        1 => rng.uniform(0.0, 1e-4),
        _ => rng.uniform(0.0, core::f64::consts::FRAC_PI_2),
    });
    v.push(match rng.below(4) {
        0 => 0.0,
        1 => 1e-15,
        _ => rng.uniform(0.0, 1e-3),
    });
    for _ in 0..4 {
        v.push(rng.uniform(-1.0, 1.0));
    }
    v
}

fn gen_curve(rng: &mut Rng, s: f64, doms: [[f64; 4]; 2]) -> CurveIn {
    match rng.below(4) {
        0 => {
            let a = rng.point3(-s, s);
            if rng.chance(0.2) {
                // a degenerate curve: every control point the same
                let c = [Some(a[0]), Some(a[1]), Some(a[2]), None];
                return CurveIn::BSpline {
                    dim: 3,
                    p: 1,
                    cpts: vec![c, c],
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                };
            }
            let d = unit_vec(rng);
            CurveIn::Segment {
                a,
                b: [a[0] + s * d[0], a[1] + s * d[1], a[2] + s * d[2]],
            }
        }
        1 => {
            // a UV line in a domain
            let d = if rng.chance(0.5) { doms[0] } else { doms[1] };
            let a = lerp_dom(d, rng.unit(), rng.unit());
            let c = lerp_dom(d, rng.unit(), rng.unit());
            CurveIn::Segment {
                a: [a[0], a[1], 0.0],
                b: [c[0], c[1] + 1e-3, 0.0],
            }
        }
        _ => {
            let dim = if rng.chance(0.5) { 2 } else { 3 };
            let p = 1 + rng.below(3) as usize;
            let n = p + 1 + rng.below(3) as usize;
            let cpts = (0..n)
                .map(|i| {
                    let q = rng.point3(-s, s);
                    [
                        Some(q[0] + i as f64 * s),
                        Some(q[1]),
                        Some(if dim == 2 { 0.0 } else { q[2] }),
                        None,
                    ]
                })
                .collect();
            let knots = clamped_knots(rng, n, p, 0.0, 1.0);
            CurveIn::BSpline {
                dim,
                p,
                cpts,
                knots,
            }
        }
    }
}

fn gen_op(
    rng: &mut Rng,
    np: &mut usize,
    nc: usize,
    planes: bool,
    doms: [[f64; 4]; 2],
    s: f64,
) -> OpIn {
    let pi = |rng: &mut Rng, np: usize| rng.below(np as u64) as usize;
    let uv = |rng: &mut Rng, d: [f64; 4]| lerp_dom(d, rng.uniform(-0.1, 1.1), rng.uniform(-0.1, 1.1));
    let n = *np;
    match rng.below(27) {
        0 => OpIn::State,
        1 => OpIn::Point { i: pi(rng, n) },
        2 => {
            *np += 1;
            OpIn::New
        }
        3 => {
            *np += 1;
            OpIn::Copy { i: pi(rng, n) }
        }
        4 => OpIn::Assign {
            dst: pi(rng, n),
            src: pi(rng, n),
        },
        5 => OpIn::IsBounded { i: pi(rng, n) },
        6 | 7 => OpIn::Classify {
            i: pi(rng, n),
            uvs: [uv(rng, doms[0]), uv(rng, doms[1])],
            surfaces: [rng.chance(0.8), rng.chance(0.8)],
            domains: doms,
            tol: match rng.below(4) {
                0 => None,
                1 => Some(EFF_ZERO),
                2 => Some(s * 1e-3),
                _ => Some(s * rng.uniform(0.0, 0.5)),
            },
        },
        8 => OpIn::SetExt {
            d: rng.uniform(0.0, 2.0),
        },
        9 => OpIn::SetEdges {
            a: rng.chance(0.5),
            b: rng.chance(0.5),
        },
        10 => OpIn::Delta {
            a: pi(rng, n),
            b: pi(rng, n),
        },
        11 => OpIn::Singularity {
            uvs: [uv(rng, doms[0]), uv(rng, doms[1])],
            i: pi(rng, n),
            prev: rng.chance(0.5).then(|| pi(rng, n)),
        },
        12 => OpIn::StepSize {
            i: pi(rng, n),
            old: match rng.below(3) {
                0 => 0.0,
                1 => 1e-14,
                _ => rng.uniform(0.0, s),
            },
        },
        13 => OpIn::Refine {
            uv0: uv(rng, doms[0]),
            uv1: uv(rng, doms[1]),
            // crossing / tangent-curve / singular refinements are deferred
            // (LocalSurfaceIntersect): keep them rare so most cases compare values
            ip_type: if rng.chance(0.85) {
                pick_u8(rng, &[0, 1, 3, 6, 7])
            } else {
                rng.below(8) as u8
            },
            normal: rng.chance(0.5).then(|| unit_vec(rng)),
            prev: rng
                .chance(0.5)
                .then(|| [uv(rng, doms[0]), uv(rng, doms[1])]),
            boundary: rng.chance(0.5),
        },
        14..=17 => OpIn::TestSpan {
            a: pi(rng, n),
            b: pi(rng, n),
        },
        18 => OpIn::AddCurve { i: pi(rng, n) },
        19 | 20 => OpIn::AddStart { i: pi(rng, n) },
        21 => OpIn::AddThrough { i: pi(rng, n) },
        22 => OpIn::Reverse,
        23 => OpIn::OnStart {
            i: pi(rng, n),
            list: rng.below(3) as u8,
            skip: rng.chance(0.3).then(|| rng.below(4) as usize),
        },
        24 => OpIn::OnTsectCurve {
            p: pi(rng, n),
            prev: pi(rng, n),
            next: pi(rng, n),
            tol: s * pick_f(rng, &[1e-3, 1e-6, 0.1]),
        },
        // deferred unless both surfaces are planes with crossing normals
        25 if nc > 0 && (planes || rng.chance(0.1)) => OpIn::CurveType {
            i: rng.below(nc as u64) as usize,
            j: rng.below(nc as u64) as usize,
        },
        25 | 26 if nc > 0 => OpIn::OnCurve {
            p: rng.point3(-s, s),
            skip: rng.chance(0.3).then(|| rng.below(4) as usize),
        },
        _ => {
            let nk = match rng.below(4) {
                0 => 0,
                _ => 2 + rng.below(3) as usize,
            };
            let mut knots: Vec<f64> = Vec::new();
            let mut k0 = rng.uniform(-1.0, 1.0);
            for _ in 0..nk {
                knots.push(k0);
                k0 += rng.uniform(1e-3, 2.0);
            }
            let np = if nk == 0 { 0 } else { 3 * (nk - 1) + 1 };
            OpIn::Flush {
                dim: 1 + rng.below(3) as u32,
                poly: (0..np).map(|_| rng.point3(-s, s)).collect(),
                knots,
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = &Kernel::default();
    let s = gen_scale(rng);
    let scenario = rng.below(10);
    let a = match scenario {
        0..=3 => gen_plane(rng, s),
        4 => gen_bspline(rng, s, true),
        _ => {
            if rng.chance(0.5) {
                gen_bspline(rng, s, false)
            } else {
                gen_plane(rng, s)
            }
        }
    };
    let b = match scenario {
        0..=2 => Some(gen_plane(rng, s)),
        3 | 4 => None,
        _ => Some(if rng.chance(0.5) {
            gen_bspline(rng, s, false)
        } else {
            gen_plane(rng, s)
        }),
    };
    let na = natural(&a);
    let nb = natural(b.as_ref().unwrap_or(&a));
    let dom1 = gen_domain(rng, na);
    let dom2 = gen_domain(rng, nb);
    let fillet = rng.chance(0.15);

    let sa = rust_surface(&a).ok()?;
    let sb = rust_surface(b.as_ref().unwrap_or(&a)).ok()?;
    let srfs = [sa, sb];
    let doms = [na, nb];

    let nc = rng.below(4) as usize;
    let curves = (0..nc).map(|_| gen_curve(rng, s, doms)).collect();

    // a traced line in both UV domains
    let base = [
        lerp_dom(na, rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)),
        lerp_dom(nb, rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)),
    ];
    let mut dir = [[0.0; 2]; 2];
    for (d, n) in dir.iter_mut().zip(doms) {
        let ang = rng.uniform(0.0, core::f64::consts::TAU);
        let mag = match rng.below(4) {
            0 => 1e-6,
            1 => 1e-3,
            2 => 0.05,
            _ => 0.3,
        };
        *d = [
            mag * (n[2] - n[0]) * ang.cos(),
            mag * (n[3] - n[1]) * ang.sin(),
        ];
    }
    if b.is_none() || rng.chance(0.3) {
        dir[1] = dir[0];
        if b.is_none() {
            let b0 = base[0];
            return Some(build(
                k,
                rng,
                a,
                b,
                dom1,
                dom2,
                fillet,
                curves,
                &srfs,
                doms,
                [b0, b0],
                dir,
                s,
            ));
        }
    }
    Some(build(
        k, rng, a, b, dom1, dom2, fillet, curves, &srfs, doms, base, dir, s,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build(
    k: &Kernel,
    rng: &mut Rng,
    a: SurfaceIn,
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    fillet: bool,
    curves: Vec<CurveIn>,
    srfs: &[Surface; 2],
    doms: [[f64; 4]; 2],
    base: [[f64; 2]; 2],
    dir: [[f64; 2]; 2],
    s: f64,
) -> Inputs {
    let n_pts = 2 + rng.below(7) as usize;
    let step = pick_f(rng, &[1.0, 0.25, 1e-15, 3.0]);
    let points = (0..n_pts)
        .map(|i| {
            let t = i as f64 * step;
            let param = t * (1.0 + rng.uniform(-0.01, 0.01));
            gen_point(k, rng, srfs, doms, base, dir, t, param)
        })
        .collect();
    let mut np = n_pts;
    let nc = curves.len();
    let is_plane = |x: &SurfaceIn| matches!(x, SurfaceIn::Plane { .. });
    let planes = is_plane(&a) && b.as_ref().is_none_or(is_plane);
    let n_ops = 4 + rng.below(16) as usize;
    let ops = (0..n_ops)
        .map(|_| gen_op(rng, &mut np, nc, planes, doms, s))
        .collect();
    Inputs {
        a,
        b,
        dom1,
        dom2,
        fillet,
        curves,
        points,
        ops,
    }
}

// ---- inline edge cases ------------------------------------------------------------------

fn plane_json(o: [f64; 3], x: [f64; 3], y: [f64; 3], d: [f64; 4]) -> Value {
    json!({"kind": "plane", "o": o, "x": x, "y": y, "scale": [1.0, 1.0], "domain": d})
}

/// A point on the line `x = t` of the planes `z = 0` (uv `(t, v0)`) and `y = 0`
/// (uv `(t, 0)`), tangent `+x`.
fn line_point(t: f64, param: f64, ptype: f64, ptypes: [f64; 4], dev: f64) -> Vec<f64> {
    let mut v = vec![ptype];
    v.extend(ptypes);
    v.extend([t, 0.5, 1.0, 0.0, t, 0.5, 1.0, 0.0]);
    let srf0 = [
        [t, 0.5, 0.0],
        [0.0, 1.0, 0.0],
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0; 3],
        [UNDEF_DOUBLE; 3],
        [0.0; 3],
        [UNDEF_DOUBLE; 3],
        [0.0, 0.0, 1.0],
    ];
    let srf1 = [
        [t, 0.0, 0.5],
        [0.0, 0.0, 1.0],
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0; 3],
        [UNDEF_DOUBLE; 3],
        [0.0; 3],
        [UNDEF_DOUBLE; 3],
        [0.0, -1.0, 0.0],
    ];
    for vec in srf0.iter().chain(srf1.iter()) {
        v.extend(vec);
    }
    v.extend([t, 0.0, 0.0, 1.0, 0.0, 0.0]);
    v.extend([param, core::f64::consts::FRAC_PI_2, dev, 0.0, 0.0, 0.0, 0.0]);
    v
}

fn edge_cases() -> Vec<(String, Value)> {
    let xy = plane_json([0.0, -0.5, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0]);
    let xz = plane_json([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let d = [0.0, 0.0, 1.0, 1.0];
    let inside = [1.0, 1.0, 1.0, 1.0];
    let mut v = Vec::new();
    let pts: Vec<Vec<f64>> = (0..5)
        .map(|i| {
            let t = 0.2 * f64::from(i);
            line_point(t, t, 2.0, inside, 0.0)
        })
        .collect();
    let base = |ops: Value, points: &Vec<Vec<f64>>| {
        json!({"a": xy, "b": xz, "dom1": d, "dom2": d, "fillet": false,
               "curves": [], "points": points, "ops": ops})
    };
    // an exact trace along the intersection line: every span passes
    v.push((
        "edge_trace_exact".into(),
        base(
            json!([{"op": "test_span", "a": 0, "b": 1}, {"op": "test_span", "a": 1, "b": 2},
                   {"op": "add_curve", "i": 0}, {"op": "add_curve", "i": 1},
                   {"op": "add_curve", "i": 2}, {"op": "add_curve", "i": 3},
                   {"op": "reverse"}, {"op": "on_tsect_curve", "p": 4, "prev": 0, "next": 1, "tol": 1e-3},
                   {"op": "on_tsect_curve", "p": 2, "prev": 3, "next": 1, "tol": 1e-3},
                   {"op": "add_curve", "i": 1}, {"op": "state"}]),
            &pts,
        ),
    ));
    // duplicate start and through points are dropped; crossing appended, others prepended
    let mut pts2 = pts.clone();
    pts2.push(line_point(0.0, 0.0, 3.0, inside, 0.0));
    pts2.push(line_point(0.4, 0.4, 2.0, [0.0, 1.0, 0.0, 1.0], 1e-3));
    v.push((
        "edge_start_points".into(),
        base(
            json!([{"op": "add_start", "i": 0}, {"op": "add_start", "i": 5},
                   {"op": "add_start", "i": 1}, {"op": "add_start", "i": 2},
                   {"op": "add_through", "i": 3}, {"op": "add_through", "i": 4},
                   {"op": "on_start", "i": 6, "list": 0, "skip": null},
                   {"op": "on_start", "i": 2, "list": 0, "skip": 2},
                   {"op": "add_start", "i": 6}, {"op": "add_start", "i": 6}, {"op": "state"}]),
            &pts2,
        ),
    ));
    // angle rejection: a turned tangent; and a singular point demoted to crossing
    let mut pts3 = pts.clone();
    if let Some(p) = pts3.get_mut(1) {
        // curve tangent (index 70..73) turned 90 degrees, type singular
        for (k, val) in [(0, 5.0), (70, 0.0), (71, 1.0)] {
            if let Some(x) = p.get_mut(k) {
                *x = val;
            }
        }
    }
    v.push((
        "edge_span_angle".into(),
        base(
            json!([{"op": "test_span", "a": 0, "b": 1}, {"op": "point", "i": 1},
                   {"op": "test_span", "a": 1, "b": 1}, {"op": "new"},
                   {"op": "test_span", "a": 5, "b": 0}, {"op": "test_span", "a": 0, "b": 5}]),
            &pts3,
        ),
    ));
    // reverse of an empty curve, step sizes, unimplemented virtuals, refine of
    // tangent points and unknown types, flush
    v.push((
        "edge_misc".into(),
        base(
            json!([{"op": "reverse"}, {"op": "step_size", "i": 0, "old": 0.0},
                   {"op": "step_size", "i": 0, "old": 1e-16}, {"op": "step_size", "i": 0, "old": 0.25},
                   {"op": "delta", "a": 0, "b": 1},
                   {"op": "singularity", "uvs": [[0.5, 0.5], [0.5, 0.5]], "i": 0, "prev": 1},
                   {"op": "refine", "uv0": [0.3, 0.4], "uv1": [2.0, -1.0], "ip_type": 3,
                    "normal": null, "prev": null, "boundary": false},
                   {"op": "refine", "uv0": [0.3, 0.4], "uv1": [0.3, 0.4], "ip_type": 6,
                    "normal": [0.0, 0.0, 1.0], "prev": null, "boundary": true},
                   {"op": "flush", "dim": 3, "poly": [], "knots": []},
                   {"op": "flush", "dim": 3, "poly": [[0,0,0],[1,0,0],[2,1,0],[3,1,0],[4,0,0],[5,0,1],[6,0,0]],
                    "knots": [0.0, 1.0, 2.5]},
                   {"op": "flush", "dim": 2, "poly": [[0,0,0],[1,0,0],[2,1,0],[3,1,0]], "knots": [-1.0, 1.0]},
                   {"op": "is_bounded", "i": 0},
                   {"op": "classify", "i": 0, "uvs": [[0.0, 0.5], [1.0 , 1.0]], "surfaces": [true, false],
                    "domains": [d, d], "tol": null},
                   {"op": "classify", "i": 1, "uvs": [[1e-13, 0.5], [0.5 , 1.5]], "surfaces": [true, true],
                    "domains": [d, [0.5, 0.5, 0.5, 0.5]], "tol": 0.01},
                   {"op": "is_bounded", "i": 1},
                   {"op": "copy", "i": 1}, {"op": "assign", "dst": 2, "src": 5},
                   {"op": "set_ext", "d": 0.7}, {"op": "set_edges", "a": false, "b": true}]),
            &pts,
        ),
    ));
    // domains: sub-domain, outside the natural domain (cut), disjoint (reset), fillet
    for (name, d1, d2, fillet) in [
        ("edge_domain_cut", [-0.5, 0.2, 0.5, 2.0], d, false),
        ("edge_domain_disjoint", [3.0, 3.0, 4.0, 4.0], d, false),
        ("edge_domain_fillet", [3.0, 3.0, 4.0, 4.0], [-1.0, -1.0, 2.0, 2.0], true),
        ("edge_domain_degenerate", [0.5, 0.0, 0.5, 1.0], d, false),
    ] {
        v.push((
            name.into(),
            json!({"a": xy, "b": xz, "dom1": d1, "dom2": d2, "fillet": fillet, "curves": [],
                   "points": pts, "ops": [{"op": "state"}, {"op": "test_span", "a": 0, "b": 2}]}),
        ));
    }
    // self-intersection and a closed (tube) B-spline surface
    let tube = {
        let mut net = Vec::new();
        for i in 0..5 {
            let a = core::f64::consts::TAU * f64::from(i % 4) / 4.0;
            let row: Vec<Value> = (0..2)
                .map(|j| json!([a.cos(), a.sin(), f64::from(j), null]))
                .collect();
            net.push(Value::Array(row));
        }
        json!({"kind": "b_spline", "p": 1, "q": 1, "net": net,
               "u": [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0], "v": [0.0, 0.0, 1.0, 1.0]})
    };
    v.push((
        "edge_tube_self".into(),
        json!({"a": tube, "b": null, "dom1": d, "dom2": d, "fillet": false, "curves": [],
               "points": pts, "ops": [{"op": "state"}, {"op": "test_span", "a": 0, "b": 1}]}),
    ));
    // curves: a degenerate segment and a line along the intersection
    v.push((
        "edge_on_curve".into(),
        json!({"a": xy, "b": xz, "dom1": d, "dom2": d, "fillet": false,
               "curves": [{"kind": "b_spline", "dim": 3, "p": 1,
                           "cpts": [[0.3, 0.0, 0.0, null], [0.3, 0.0, 0.0, null]],
                           "knots": [0.0, 0.0, 1.0, 1.0]},
                          {"kind": "segment", "a": [0.0, 0.0, 0.0], "b": [1.0, 0.0, 0.0]}],
               "points": pts,
               "ops": [{"op": "on_curve", "p": [0.3, 1e-4, 0.0], "skip": null},
                       {"op": "on_curve", "p": [0.6, 0.0, 0.0], "skip": 0},
                       {"op": "on_curve", "p": [0.6, 5.0, 0.0], "skip": null},
                       {"op": "curve_type", "i": 0, "j": 1}]}),
    ));
    v
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_intersector_p1"
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
