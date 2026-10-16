//! Runner for unit `intersect.surface_intersector_p3`: `SmSurfaceIntersector`, part 3
//! (`FindBoundaryStartPoints`, `ComputeNextPoint`, `TestPoint` with
//! `sm_UpdateTsectPoint`, `SplitSpan`).
//!
//! C++: `solidcore_ffi::units::intersect_surface_intersector_p3` (a real
//! `SmSurfaceIntersector` on `SmPlane` / `SmBSplineSurface` / `SmCurveBoundedSurface`
//! objects). Rust: `solidcore::intersect::surface_intersector::p3`.
//!
//! A case is two surfaces (B may be A itself), their domains, the fillet flag, some
//! curves, a point arena and a program of operations; both sides produce the record
//! documented in the ffi module (statuses `0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce,
//! [`ffi::REJECTED`] for an op the shim does not run), reported as the values `rec`.
//! A case whose Rust side reaches an unported unit is deferred as a whole.
//!
//! Generated programs: traces along the line where two planes cross (any angle down to
//! nearly tangent, the line inside or on a domain boundary, any scale) with through,
//! start and end points on, off, before and beyond the traced span, then `TestPoint`
//! and `ComputeNextPoint`; random points and programs on planes, B-spline and
//! curve-bounded surfaces; `FindBoundaryStartPoints` of B-spline bumps and planes with
//! intersection boxes that miss or meet the boundaries; `SplitSpan`.
//!
//! `FindBoundaryStartPoints` of planes and B-spline surfaces runs with fresh surface
//! caches (`find_boundary_start_points_with_caches`, see `Run::caches`); with a
//! curve-bounded surface it fetches them (`smsurf_GetSurfaceCache`).
//!
//! Deferred cases: `ComputeNextPoint` refining a crossing, tangent-curve or singular
//! point (`SmSurface::LocalSurfaceIntersect`), `ComputePointValues` on a B-spline or
//! curve-bounded surface (`SmSurface::IsSingularity`) or near a tangency
//! (`SmSurface::LocalSurfaceSolve`), `FindBoundaryStartPoints` with a boundary box
//! meeting the intersection box (`SmSurface::GlobalCurveIntersect`), on a plane
//! (`SmPlane::CreateIsoParametricCurve`) or with a curve-bounded surface (the cache
//! build's `CalculateContinuities`), and `SplitSpan` with a valid plane
//! (`SmFilletLaw::Evaluate`).

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
use solidcore::intersect::surface_intersector::p2::{LawPointOptions, TsectCurves};
use solidcore::kernel::Kernel;
use solidcore::math::UNDEF_DOUBLE;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::surface::p2::surface_cache;
use solidcore::surface::surface_cache::SurfaceCache;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_intersector_p3::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_intersector_p3` runner.
pub struct Runner;

/// No start or end point in the state record.
const NO_POINT: f64 = -3.0;

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SplitIn {
    curr: usize,
    next: usize,
    delta: f64,
    normals: [[f64; 3]; 2],
    average: bool,
    orient: bool,
    curve: usize,
    interval: [f64; 2],
    orientations: [f64; 2],
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
    AddThrough {
        i: usize,
    },
    Cpv {
        uvs: [[f64; 2]; 2],
        i: usize,
        prev: Option<usize>,
        step: Option<f64>,
    },
    Members {
        trace_dir: f64,
        approx_tol: f64,
        start_tol: f64,
        step_tol: f64,
        ang_tol: f64,
    },
    Ends {
        start: Option<usize>,
        end: Option<usize>,
    },
    Ext {
        clip: bool,
        last_through: f64,
        distance: f64,
    },
    Test {
        i: usize,
        done: bool,
    },
    Next {
        prev: usize,
        step: f64,
        next: usize,
        dev: f64,
        ang: f64,
    },
    Split(SplitIn),
    Fbsp {
        bbox: [[f64; 3]; 2],
        use_edges: [bool; 2],
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
        ))),
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

fn ffi_op(o: &OpIn) -> ffi::Op {
    match o {
        OpIn::State => ffi::Op::State,
        OpIn::Point { i } => ffi::Op::Point(*i),
        OpIn::AddCurve { i } => ffi::Op::AddCurve(*i),
        OpIn::AddStart { i } => ffi::Op::AddStart(*i),
        OpIn::AddThrough { i } => ffi::Op::AddThrough(*i),
        OpIn::Cpv { uvs, i, prev, step } => ffi::Op::Cpv {
            uvs: *uvs,
            i: *i,
            prev: *prev,
            step: *step,
        },
        OpIn::Members {
            trace_dir,
            approx_tol,
            start_tol,
            step_tol,
            ang_tol,
        } => ffi::Op::Members {
            trace_dir: *trace_dir,
            approx_tol: *approx_tol,
            start_tol: *start_tol,
            step_tol: *step_tol,
            ang_tol: *ang_tol,
        },
        OpIn::Ends { start, end } => ffi::Op::Ends {
            start: *start,
            end: *end,
        },
        OpIn::Ext {
            clip,
            last_through,
            distance,
        } => ffi::Op::Ext {
            clip: *clip,
            last_through: *last_through,
            distance: *distance,
        },
        OpIn::Test { i, done } => ffi::Op::Test { i: *i, done: *done },
        OpIn::Next {
            prev,
            step,
            next,
            dev,
            ang,
        } => ffi::Op::Next {
            prev: *prev,
            step: *step,
            next: *next,
            dev: *dev,
            ang: *ang,
        },
        OpIn::Split(s) => ffi::Op::Split(ffi::SplitOp {
            curr: s.curr,
            next: s.next,
            delta: s.delta,
            normals: s.normals,
            average: s.average,
            orient: s.orient,
            curve: s.curve,
            interval: s.interval,
            orientations: s.orientations,
        }),
        OpIn::Fbsp { bbox, use_edges } => ffi::Op::Fbsp {
            bbox: *bbox,
            use_edges: *use_edges,
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


// ---- Rust side: state and ops -------------------------------------------------------------

/// A list entry: a program point's index, or `-1` and the point.
fn put_ref(o: &mut Vec<f64>, si: &SurfaceIntersector<'_>, nprog: usize, id: TsectId) -> Result<(), Fail> {
    if id.index() < nprog {
        o.push(id.index() as f64);
    } else {
        o.push(-1.0);
        let p = pt_of(si, id.index())?;
        put_point(o, &p);
    }
    Ok(())
}

fn put_list(o: &mut Vec<f64>, si: &SurfaceIntersector<'_>, nprog: usize, l: &[TsectId]) -> Result<(), Fail> {
    o.push(l.len() as f64);
    for &id in l {
        put_ref(o, si, nprog, id)?;
    }
    Ok(())
}

fn end_index(p: Option<TsectId>, nprog: usize) -> f64 {
    match p {
        None => NO_POINT,
        Some(id) if id.index() < nprog => id.index() as f64,
        Some(_) => -1.0,
    }
}

fn put_state(o: &mut Vec<f64>, si: &SurfaceIntersector<'_>, nprog: usize) -> Result<(), Fail> {
    o.extend([
        si.this_approx_tol_3d,
        si.curve_trace_direction,
        si.start_parallel_tol_radians,
        si.stepping_parallel_tol_radians,
        si.this_ang_tol_rad,
        si.last_through_parameter,
        si.extension_distance,
        b(si.doing_extension_clipping),
        b(si.curve_is_closed),
        b(si.clipped_by_through_point),
        b(si.check_for_interior_curves),
        end_index(si.start_point, nprog),
        end_index(si.end_point, nprog),
    ]);
    put_list(o, si, nprog, si.curve_points())?;
    put_list(o, si, nprog, si.start_points())?;
    put_list(o, si, nprog, si.through_points())?;
    o.push(nprog as f64);
    for i in 0..nprog {
        let p = pt_of(si, i)?;
        put_point(o, &p);
    }
    Ok(())
}

/// The shim's rejection of `TestPoint`: the next point is the last curve point, or
/// the curve list is empty while a through, start or end point needs it.
fn test_rejected(si: &SurfaceIntersector<'_>, i: usize) -> bool {
    let last = si.curve_points().last().copied();
    last == Some(id(i))
        || (si.curve_points().is_empty()
            && (!si.through_points().is_empty()
                || si.start_point.is_some()
                || si.end_point.is_some()))
}

/// The run's state besides the intersector: program curves, the surface caches and
/// the `FindBoundaryStartPoints` outputs.
struct Run<'a> {
    curves: Vec<Curve>,
    surfaces: [&'a Surface; 2],
    same: bool,
    caches: Option<Vec<SurfaceCache<()>>>,
    outs: TsectCurves,
    nprog: usize,
}

impl Run<'_> {
    /// The surface caches for `FindBoundaryStartPoints` of planes and B-spline
    /// surfaces, built as `smsurf_GetSurfaceCache` builds them on the first call and
    /// kept (as upstream keeps them on the surfaces: later calls see the iso boundary
    /// curves and the tree of the first; `GlobalCurveIntersect` solves on that tree).
    /// `None` (`FindBoundaryStartPoints` fetches the caches itself, with the same
    /// result) for a curve-bounded surface, whose trimmed-surface cache build fails
    /// upstream, or when a build fails.
    fn caches(&mut self, k: &Kernel) -> Option<&mut Vec<SurfaceCache<()>>> {
        let surfaces = self.surfaces;
        if surfaces
            .iter()
            .any(|s| matches!(s.kind(), SurfaceKind::CurveBounded(_)))
        {
            return None;
        }
        if self.caches.is_none() {
            let n = if self.same { 1 } else { 2 };
            let built: Result<Vec<_>, _> = surfaces
                .iter()
                .take(n)
                .map(|s| surface_cache::<()>(k, s, Some(())))
                .collect();
            self.caches = Some(built.ok()?);
        }
        self.caches.as_mut()
    }
}

#[allow(clippy::too_many_lines)]
fn run_op(k: &Kernel, run: &mut Run<'_>, si: &mut SurfaceIntersector<'_>, op: &OpIn, o: &mut Vec<f64>) -> Result<(), Fail> {
    let nprog = run.nprog;
    match op {
        OpIn::State => {
            o.push(0.0);
            put_state(o, si, nprog)?;
        }
        OpIn::Point { i } => {
            let p = pt_of(si, *i)?;
            o.push(0.0);
            put_point(o, &p);
        }
        OpIn::AddCurve { i } | OpIn::AddStart { i } | OpIn::AddThrough { i } => {
            pt_of(si, *i)?;
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
        OpIn::Members {
            trace_dir,
            approx_tol,
            start_tol,
            step_tol,
            ang_tol,
        } => {
            si.curve_trace_direction = *trace_dir;
            si.this_approx_tol_3d = *approx_tol;
            si.start_parallel_tol_radians = *start_tol;
            si.stepping_parallel_tol_radians = *step_tol;
            si.this_ang_tol_rad = *ang_tol;
            o.push(0.0);
        }
        OpIn::Ends { start, end } => {
            si.start_point = start.map(id);
            si.end_point = end.map(id);
            o.push(0.0);
        }
        OpIn::Ext {
            clip,
            last_through,
            distance,
        } => {
            si.doing_extension_clipping = *clip;
            si.last_through_parameter = *last_through;
            si.extension_distance = *distance;
            o.push(0.0);
        }
        OpIn::Test { i, done } => {
            pt_of(si, *i)?;
            if test_rejected(si, *i) {
                o.push(ffi::REJECTED);
            } else {
                let r = si.test_point(&Kernel::default(), id(*i));
                if let Some(d) = st(o, r)? {
                    o.push(b(d.unwrap_or(*done)));
                }
                let p = pt_of(si, *i)?;
                put_point(o, &p);
            }
        }
        OpIn::Next {
            prev,
            step,
            next,
            dev,
            ang,
        } => {
            pt_of(si, *prev)?;
            pt_of(si, *next)?;
            if prev == next {
                o.push(ffi::REJECTED);
            } else {
                let r = si.compute_next_point(&Kernel::default(), id(*prev), *step, id(*next));
                if let Some(n) = st(o, r)? {
                    o.extend([
                        b(n.found_good_point),
                        n.deviation_found.unwrap_or(*dev),
                        n.angle_found_rad.unwrap_or(*ang),
                        b(n.clipped),
                        b(n.boundary_hit),
                    ]);
                }
                let p = pt_of(si, *next)?;
                put_point(o, &p);
            }
        }
        OpIn::Split(s) => {
            let curve = run
                .curves
                .get(s.curve)
                .ok_or_else(|| harness("curve index"))?;
            let opts = LawPointOptions {
                law: &(),
                law_orient: s.orient,
                point_curve: curve,
                point_curve_interval: Extent1d::from_min_max(s.interval[0], s.interval[1]),
                normals: [v3(s.normals[0]), v3(s.normals[1])],
                average_normals: s.average,
                surface_orientations: s.orientations,
            };
            pt_of(si, s.curr)?;
            pt_of(si, s.next)?;
            let r = si.split_span(k, id(s.curr), id(s.next), s.delta, &opts);
            if let Some(new_id) = st(o, r)? {
                let p = pt_of(si, new_id.index())?;
                put_point(o, &p);
            }
        }
        OpIn::Fbsp { bbox, use_edges } => {
            si.set_use_surface_edges(use_edges[0], use_edges[1]);
            let bx = Extent3d::from_bounds(
                bbox[0][0], bbox[0][1], bbox[0][2], bbox[1][0], bbox[1][1], bbox[1][2],
            );
            let k = Kernel::default();
            let mut outs = core::mem::take(&mut run.outs);
            let r = match run.caches(&k) {
                Some(caches) => si.find_boundary_start_points_with_caches(&k, caches, &bx, &mut outs),
                None => si.find_boundary_start_points(&k, &bx, &mut outs),
            };
            run.outs = outs;
            st(o, r)?;
            put_outs(o, &run.outs, run.curves.len())?;
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
        OpIn::AddThrough { .. } => 5,
        OpIn::Cpv { .. } => 6,
        OpIn::Members { .. } => 7,
        OpIn::Ends { .. } => 8,
        OpIn::Ext { .. } => 9,
        OpIn::Test { .. } => 10,
        OpIn::Next { .. } => 11,
        OpIn::Split(_) => 12,
        OpIn::Fbsp { .. } => 13,
    })
}

fn rust_run(k: &Kernel, i: &Inputs) -> Result<Vec<f64>, Fail> {
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
    let mut si = match SurfaceIntersector::new(k, &a, &ext(i.dom1), b_ref, &ext(i.dom2), i.fillet) {
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
    let mut run = Run {
        outs: TsectCurves {
            curves_3d: curves.clone(),
            ..TsectCurves::default()
        },
        curves,
        surfaces: [&a, b_ref],
        same: i.b.is_none(),
        caches: None,
        nprog: i.points.len(),
    };
    for op in &i.ops {
        o.push(op_code(op));
        run_op(k, &mut run, &mut si, op, &mut o)?;
    }
    put_state(&mut o, &si, run.nprog)?;
    Ok(o)
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    // reject what the C++ side rejects
    ffi::encode(&ffi_program(i)).map_err(|e| harness(format!("rejected input: {e}")))?;
    match rust_run(k, i) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- generation: traces along the crossing line of two planes ------------------------------

/// An orthonormal frame.
fn frame(rng: &mut Rng) -> [[f64; 3]; 3] {
    let ex = unit_vec(rng);
    let ey = loop {
        let c = cross(unit_vec(rng), ex);
        if c[0] * c[0] + c[1] * c[1] + c[2] * c[2] > 1e-2 {
            break norm(c);
        }
    };
    [ex, ey, cross(ex, ey)]
}

/// `f * v` (a vector in the frame).
fn mapv(f: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    let [a, b, c] = *f;
    [
        a[0] * v[0] + b[0] * v[1] + c[0] * v[2],
        a[1] * v[0] + b[1] * v[1] + c[1] * v[2],
        a[2] * v[0] + b[2] * v[1] + c[2] * v[2],
    ]
}

fn add3(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
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

fn gen_members(rng: &mut Rng, s: f64) -> OpIn {
    OpIn::Members {
        trace_dir: if rng.chance(0.7) { 1.0 } else { -1.0 },
        approx_tol: s * pick_f(rng, &[1e-3, 1e-3, 1e-6, 0.05, 1e-10]),
        start_tol: pick_f(rng, &[1e-10, 1e-3, 0.06, 0.3, 0.0]),
        step_tol: pick_f(rng, &[1e-12, 1e-12, 0.1, 2.0]),
        ang_tol: pick_f(rng, &[20f64.to_radians(), 1e-3, 1.0, 0.0]),
    }
}

/// Two planes crossing along a line (in A's uv: `v = c`; in B's: `v = h`) at angle
/// `theta`, under a random rigid motion; a trace of points along the line, target
/// points on and off it, then `TestPoint` / `ComputeNextPoint` / `SplitSpan`.
#[allow(clippy::too_many_lines)]
fn gen_trace_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let theta = pick_f(
        rng,
        &[core::f64::consts::FRAC_PI_2, 0.7, 0.1, 1e-3, 1e-6, 2.5],
    );
    let line_v = |rng: &mut Rng| {
        s * match rng.below(8) {
            0 => 0.0,
            1 => 1.0,
            _ => rng.uniform(0.1, 0.9),
        }
    };
    let c = line_v(rng);
    let h = line_v(rng);
    let f = frame(rng);
    let t = rng.point3(-s, s);
    let w = mapv(&f, [0.0, theta.cos(), theta.sin()]);
    let dom = [0.0, 0.0, s, s];
    let a = plane_in(t, f[0], f[1], dom, [1.0, 1.0]);
    let ob = add3(add3(t, mapv(&f, [0.0, c, 0.0])), scale3(w, -h));
    let b_plane = plane_in(ob, f[0], w, dom, [1.0, 1.0]);
    let (a, b_srf) = match rng.below(12) {
        0 => (bounded(a), b_plane),
        1 => (a, bounded(b_plane)),
        _ => (a, b_plane),
    };
    let dom1 = if rng.chance(0.8) { dom } else { gen_domain(rng, dom) };
    let dom2 = if rng.chance(0.8) { dom } else { gen_domain(rng, dom) };

    // the trace points along the line
    let mid = rng.uniform(0.05, 0.5);
    let x0 = s * pick_f(rng, &[0.0, 0.1, 0.3, mid]);
    let step = s * pick_f(rng, &[0.1, 0.2, 0.05, 1e-4, 0.3]);
    let n_line = 3 + rng.below(4) as usize;
    let xs: Vec<f64> = (0..n_line).map(|i| x0 + i as f64 * step).collect();
    let uv_at = |x: f64, da: f64, db: f64| [[x, c + da], [x, h + db]];

    let mut points: Vec<Vec<f64>> = xs.iter().map(|&x| blank_point(x - x0)).collect();
    let mut ops = Vec::new();
    if rng.chance(0.5) {
        ops.push(gen_members(rng, s));
    }
    for (i, &x) in xs.iter().enumerate() {
        let prev = (i > 0 && rng.chance(0.8)).then(|| i - 1);
        ops.push(OpIn::Cpv {
            uvs: uv_at(x, 0.0, 0.0),
            i,
            prev,
            step: None,
        });
    }
    // the curve so far: all but the last one (or two) points
    let k = if n_line > 3 && rng.chance(0.3) {
        n_line - 2
    } else {
        n_line - 1
    };
    for i in 0..k {
        ops.push(OpIn::AddCurve { i });
    }
    let next = n_line - 1;

    // target points: along the last span, behind, beyond, off the line
    let n_targets = rng.below(4) as usize;
    let mut targets = Vec::new();
    for _ in 0..n_targets {
        let frac = pick_f(rng, &[0.3, 0.5, 0.9, 1.5, -0.5, 0.0, 1.0]);
        let xa = xs.get(k - 1).copied().unwrap_or(x0);
        let xb = xs.get(next).copied().unwrap_or(x0);
        let x = xa + frac * (xb - xa);
        let off = if rng.chance(0.75) {
            0.0
        } else {
            s * pick_f(rng, &[1e-3, 0.05, 1e-7])
        };
        let ti = points.len();
        let param = if rng.chance(0.4) { -1.0 } else { x - x0 };
        points.push(blank_point(param));
        ops.push(OpIn::Cpv {
            uvs: uv_at(x, off, 0.0),
            i: ti,
            prev: None,
            step: None,
        });
        targets.push(ti);
    }
    let mut start_point = None;
    let mut end_point = None;
    for &ti in &targets {
        match rng.below(5) {
            0 | 1 => ops.push(OpIn::AddThrough { i: ti }),
            2 => ops.push(OpIn::AddStart { i: ti }),
            3 => end_point = Some(ti),
            _ => start_point = Some(ti),
        }
    }
    if rng.chance(0.3) {
        start_point = Some(0);
    }
    if start_point.is_some() || end_point.is_some() || rng.chance(0.1) {
        ops.push(OpIn::Ends {
            start: start_point,
            end: end_point,
        });
    }
    if rng.chance(0.25) {
        ops.push(OpIn::Ext {
            clip: rng.chance(0.7),
            last_through: pick_f(rng, &[0.0, step, 0.5 * step]),
            distance: pick_f(rng, &[0.0, step, 1e-3 * s]),
        });
    }

    // spare points for ComputeNextPoint / SplitSpan
    let spare = points.len();
    points.push(blank_point(0.0));
    points.push(blank_point(0.0));

    // the tests
    ops.push(OpIn::Test {
        i: next,
        done: rng.chance(0.5),
    });
    if rng.chance(0.3) {
        ops.push(OpIn::Test {
            i: rng.below(points.len() as u64) as usize,
            done: rng.chance(0.5),
        });
    }
    // (a crossing previous point is refined by LocalSurfaceIntersect: deferred)
    let n_next = if rng.chance(0.45) {
        1 + rng.below(3) as usize
    } else {
        0
    };
    for _ in 0..n_next {
        let prev = match rng.below(5) {
            0 | 1 => spare + 1,
            2 => rng.below(points.len() as u64) as usize,
            _ => k - 1,
        };
        ops.push(OpIn::Next {
            prev,
            step: s * pick_f(rng, &[0.1, 1e-3, 2.0, 0.0, -0.1]),
            next: spare,
            dev: -7.0,
            ang: -8.0,
        });
    }
    let mut curves = Vec::new();
    if rng.chance(0.15) {
        curves.push(CurveIn::Segment {
            a: t,
            b: add3(t, scale3(f[0], s)),
        });
        let zero = rng.chance(0.85);
        ops.push(OpIn::Split(SplitIn {
            curr: 0,
            next: 1,
            delta: pick_f(rng, &[1e-3, 1e-6, 0.1]),
            normals: if zero {
                [[0.0; 3], [0.0; 3]]
            } else {
                [unit_vec(rng), unit_vec(rng)]
            },
            average: zero || rng.chance(0.5),
            orient: rng.chance(0.5),
            curve: 0,
            interval: [0.0, 1.0],
            orientations: [1.0, -1.0],
        }));
    }
    ops.push(OpIn::State);
    Inputs {
        a,
        b: Some(b_srf),
        dom1,
        dom2,
        fillet: rng.chance(0.05),
        curves,
        points,
        ops,
    }
}

// ---- generation: random programs ----------------------------------------------------------

fn gen_points(rng: &mut Rng, doms: [[f64; 4]; 2], s: f64) -> Vec<Vec<f64>> {
    let n = 3 + rng.below(6) as usize;
    let step = pick_f(rng, &[1.0, 0.25, 3.0, 1e-9]);
    (0..n)
        .map(|i| {
            let t = i as f64 * step;
            let param = if rng.chance(0.1) {
                -1.0
            } else {
                t * (1.0 + rng.uniform(-0.01, 0.01))
            };
            gen_point(rng, doms, s, param)
        })
        .collect()
}

fn gen_random_ops(rng: &mut Rng, np: usize, nc: usize, s: f64, doms: [[f64; 4]; 2]) -> Vec<OpIn> {
    let mut ops = Vec::new();
    if rng.chance(0.4) {
        ops.push(gen_members(rng, s));
    }
    let pick = |rng: &mut Rng| rng.below(np as u64) as usize;
    let n_ops = 6 + rng.below(16) as usize;
    while ops.len() < n_ops {
        let op = match rng.below(20) {
            0..=2 => OpIn::AddCurve { i: pick(rng) },
            3 => OpIn::AddStart { i: pick(rng) },
            4 => OpIn::AddThrough { i: pick(rng) },
            5 => OpIn::Ends {
                start: rng.chance(0.5).then(|| pick(rng)),
                end: rng.chance(0.5).then(|| pick(rng)),
            },
            6 => OpIn::Ext {
                clip: rng.chance(0.5),
                last_through: pick_f(rng, &[0.0, 1.0, 0.5]),
                distance: pick_f(rng, &[0.0, 0.5]),
            },
            7..=10 => OpIn::Test {
                i: pick(rng),
                done: rng.chance(0.5),
            },
            11..=14 => OpIn::Next {
                prev: pick(rng),
                step: s * pick_f(rng, &[0.1, 1e-3, 1.0, 0.0]),
                next: pick(rng),
                dev: -7.0,
                ang: -8.0,
            },
            15 | 16 => OpIn::Cpv {
                uvs: [gen_uv(rng, doms[0]), gen_uv(rng, doms[1])],
                i: pick(rng),
                prev: None,
                step: None,
            },
            17 if nc > 0 => OpIn::Split(SplitIn {
                curr: pick(rng),
                next: pick(rng),
                delta: 1e-3,
                normals: [[0.0; 3], [0.0; 3]],
                average: true,
                orient: rng.chance(0.5),
                curve: 0,
                interval: [0.0, 1.0],
                orientations: [1.0, 1.0],
            }),
            18 => OpIn::Point { i: pick(rng) },
            _ => OpIn::State,
        };
        ops.push(op);
    }
    ops
}

fn gen_random_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let (a, b) = match rng.below(10) {
        0..=4 => (gen_plane(rng, s, None), Some(gen_plane(rng, s, None))),
        5 => (gen_plane(rng, s, None), None),
        6 | 7 => (gen_plane(rng, s, None), Some(gen_bspline(rng, s, false))),
        _ => {
            let tube = rng.chance(0.3);
            (gen_bspline(rng, s, false), Some(gen_bspline(rng, s, tube)))
        }
    };
    let (a, b) = match rng.below(10) {
        0 => (bounded(a), b),
        1 => (a, b.map(bounded)),
        _ => (a, b),
    };
    let na = natural(&a);
    let nb = natural(b.as_ref().unwrap_or(&a));
    let dom1 = gen_domain(rng, na);
    let dom2 = gen_domain(rng, nb);
    let doms = [na, nb];
    let nc = usize::from(rng.chance(0.3));
    let curves: Vec<CurveIn> = (0..nc).map(|_| gen_curve(rng, s)).collect();
    let points = gen_points(rng, doms, s);
    let mut ops = gen_random_ops(rng, points.len(), nc, s, doms);
    if rng.chance(0.2) {
        ops.push(gen_fbsp(rng, s, s));
    }
    Inputs {
        a,
        b,
        dom1,
        dom2,
        fillet: rng.chance(0.1),
        curves,
        points,
        ops,
    }
}

// ---- generation: boundary start points ------------------------------------------------------

/// A `FindBoundaryStartPoints` op: the box around the surfaces (size `size`), far
/// away, tiny, or empty.
fn gen_fbsp(rng: &mut Rng, s: f64, size: f64) -> OpIn {
    let bbox = match rng.below(6) {
        0 | 1 => [[-size, -size, -size], [3.0 * size, 3.0 * size, 3.0 * size]],
        2 => {
            let c = 50.0 * s;
            [[c, c, c], [c + s, c + s, c + s]]
        }
        3 => {
            let p = rng.point3(-size, size);
            [p, add3(p, [1e-6 * s; 3])]
        }
        4 => [[-size, -size, -size], [-size, -size, -size]],
        _ => [[-size, -size, 2.0 * size], [3.0 * size, 3.0 * size, 3.0 * size]],
    };
    OpIn::Fbsp {
        bbox,
        use_edges: [rng.chance(0.6), rng.chance(0.6)],
    }
}

/// Two bump patches (as in part 2) or a bump and a plane, with boundary start point
/// searches and a few tests.
fn gen_fbsp_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let deg = 2 + rng.below(2) as usize;
    let n = deg + 1 + rng.below(3) as usize;
    let h1 = heights(rng, n, s, None);
    let a = bump_bspline(s, 0.0, deg, &h1, false, [param_range(rng), param_range(rng)]);
    let b = match rng.below(4) {
        0 => {
            let o = [0.0, 0.0, s * rng.uniform(-0.2, 0.2)];
            plane_in(o, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-s, -s, 2.0 * s, 2.0 * s], [1.0, 1.0])
        }
        1 => gen_bspline(rng, s, true),
        _ => {
            let mut h2 = heights(rng, n, s, None);
            h2.reverse();
            let gap = s * pick_f(rng, &[0.0, -0.2, 0.5, 5.0]);
            bump_bspline(s, 2.0 * s + gap, deg, &h2, true, [param_range(rng), param_range(rng)])
        }
    };
    let na = natural(&a);
    let nb = natural(&b);
    let dom1 = if rng.chance(0.8) { na } else { gen_domain(rng, na) };
    let dom2 = if rng.chance(0.8) { nb } else { gen_domain(rng, nb) };
    let doms = [na, nb];
    let points = gen_points(rng, doms, s);
    let mut ops = Vec::new();
    if rng.chance(0.3) {
        ops.push(gen_members(rng, s));
    }
    let nf = 1 + rng.below(2) as usize;
    for _ in 0..nf {
        ops.push(gen_fbsp(rng, s, s));
    }
    ops.push(OpIn::State);
    if rng.chance(0.3) {
        ops.push(OpIn::Test {
            i: rng.below(points.len() as u64) as usize,
            done: false,
        });
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

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(20) {
        0..=11 => gen_trace_case(rng),
        12..=16 => gen_random_case(rng),
        _ => gen_fbsp_case(rng),
    }
}

// ---- inline edge cases ------------------------------------------------------------------

fn plane_json(o: [f64; 3], x: [f64; 3], y: [f64; 3], d: [f64; 4]) -> Value {
    json!({"kind": "plane", "o": o, "x": x, "y": y, "scale": [1.0, 1.0], "domain": d})
}

#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    // z = 0 and y = 0.5 planes over [0, 1]^2: they cross along x at v = 0.5 on both
    let xy = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0]);
    let xz = plane_json([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    // y = 0 plane: crosses z = 0 along A's v = 0 boundary
    let xz0 = plane_json([0.0, 0.0, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let d = [0.0, 0.0, 1.0, 1.0];
    let pts: Vec<Vec<f64>> = (0..10).map(|i| blank_point(0.1 * f64::from(i))).collect();
    let prog = |a: &Value, bb: &Value, ops: Value| {
        json!({"a": a, "b": bb, "dom1": d, "dom2": d, "fillet": false,
               "curves": [{"kind": "segment", "a": [0.0, 0.0, 0.0], "b": [1.0, 0.0, 0.0]}],
               "points": pts, "ops": ops})
    };
    let cpv = |x: f64, v: f64, i: usize, prev: Option<usize>| {
        json!({"op": "cpv", "uvs": [[x, v], [x, 0.5]], "i": i, "prev": prev, "step": null})
    };
    // a trace 0.1, 0.2, 0.3 with next at 0.4; point i at x = 0.1 * (i + 1)
    let trace = |extra: Vec<Value>| {
        let mut ops = vec![
            cpv(0.1, 0.5, 0, None),
            cpv(0.2, 0.5, 1, Some(0)),
            cpv(0.3, 0.5, 2, Some(1)),
            cpv(0.4, 0.5, 3, Some(2)),
            json!({"op": "add_curve", "i": 0}),
            json!({"op": "add_curve", "i": 1}),
            json!({"op": "add_curve", "i": 2}),
        ];
        ops.extend(extra);
        ops.push(json!({"op": "state"}));
        Value::Array(ops)
    };
    let mut v = Vec::new();
    v.push((
        "edge_end_point".into(),
        prog(&xy, &xz, trace(vec![
            cpv(0.35, 0.5, 5, None),
            json!({"op": "ends", "start": 0, "end": 5}),
            json!({"op": "test", "i": 3, "done": false}),
        ])),
    ));
    v.push((
        "edge_through_points_extension".into(),
        prog(&xy, &xz, trace(vec![
            cpv(0.33, 0.5, 5, None),
            cpv(0.37, 0.5, 6, None),
            json!({"op": "add_through", "i": 5}),
            json!({"op": "add_through", "i": 6}),
            json!({"op": "ext", "clip": true, "last_through": 0.0, "distance": 0.05}),
            json!({"op": "test", "i": 3, "done": true}),
            cpv(0.45, 0.5, 7, Some(3)),
            json!({"op": "add_curve", "i": 3}),
            json!({"op": "test", "i": 7, "done": true}),
        ])),
    ));
    v.push((
        "edge_extension_clip_past_last_through".into(),
        prog(&xy, &xz, trace(vec![
            json!({"op": "ext", "clip": true, "last_through": 0.1, "distance": 0.05}),
            json!({"op": "test", "i": 3, "done": false}),
        ])),
    ));
    v.push((
        "edge_start_points".into(),
        prog(&xy, &xz, trace(vec![
            cpv(0.35, 0.5, 5, None),
            cpv(0.8, 0.5, 6, None),
            cpv(0.3, 0.5, 7, None),
            json!({"op": "add_start", "i": 6}),
            json!({"op": "add_start", "i": 5}),
            json!({"op": "add_start", "i": 7}),
            json!({"op": "test", "i": 3, "done": false}),
        ])),
    ));
    v.push((
        "edge_closed_curve".into(),
        prog(&xy, &xz, trace(vec![
            cpv(0.35, 0.5, 5, None),
            json!({"op": "ends", "start": 5, "end": null}),
            json!({"op": "test", "i": 3, "done": false}),
        ])),
    ));
    v.push((
        "edge_end_point_single_span_seam".into(),
        prog(&xy, &xz, json!([
            cpv(0.1, 0.5, 0, None), cpv(0.2, 0.5, 1, Some(0)), cpv(0.1, 0.5, 2, None),
            {"op": "add_curve", "i": 0},
            {"op": "ends", "start": null, "end": 2},
            {"op": "test", "i": 1, "done": false}, {"op": "state"}])),
    ));
    v.push((
        "edge_boundary_exit".into(),
        prog(&xy, &xz, json!([
            cpv(0.9, 0.5, 0, None), cpv(1.0, 0.5, 1, Some(0)), cpv(1.0, 0.5, 2, None),
            {"op": "add_curve", "i": 0}, {"op": "add_start", "i": 2},
            {"op": "test", "i": 1, "done": false}, {"op": "state"}])),
    ));
    // the crossing along A's v = 0 boundary: a start point there is a through point
    v.push((
        "edge_start_on_boundary_along".into(),
        prog(&xy, &xz0, json!([
            {"op": "cpv", "uvs": [[0.1, 0.0], [0.1, 0.5]], "i": 0, "prev": null, "step": null},
            {"op": "cpv", "uvs": [[0.2, 0.0], [0.2, 0.5]], "i": 1, "prev": 0, "step": null},
            {"op": "cpv", "uvs": [[0.3, 0.0], [0.3, 0.5]], "i": 2, "prev": 1, "step": null},
            {"op": "cpv", "uvs": [[0.25, 0.0], [0.25, 0.5]], "i": 3, "prev": null, "step": null},
            {"op": "add_curve", "i": 0}, {"op": "add_curve", "i": 1},
            {"op": "add_start", "i": 3},
            {"op": "test", "i": 2, "done": false}, {"op": "state"}])),
    ));
    v.push((
        "edge_rejected_tests".into(),
        prog(&xy, &xz, json!([
            cpv(0.1, 0.5, 0, None),
            {"op": "ends", "start": null, "end": 0},
            {"op": "test", "i": 1, "done": false},
            {"op": "ends", "start": null, "end": null},
            {"op": "test", "i": 1, "done": true},
            {"op": "add_curve", "i": 0},
            {"op": "test", "i": 0, "done": true},
            {"op": "next", "prev": 0, "step": 0.1, "next": 0, "dev": -7.0, "ang": -8.0}])),
    ));
    v.push((
        "edge_next_points".into(),
        prog(&xy, &xz, json!([
            {"op": "cpv", "uvs": [[1.5, 0.5], [1.5, 0.5]], "i": 0, "prev": null, "step": null},
            {"op": "next", "prev": 0, "step": 0.1, "next": 5, "dev": -7.0, "ang": -8.0},
            {"op": "next", "prev": 1, "step": 0.1, "next": 5, "dev": -7.0, "ang": -8.0},
            {"op": "next", "prev": 1, "step": 0.0, "next": 5, "dev": -7.0, "ang": -8.0},
            {"op": "next", "prev": 1, "step": -0.1, "next": 6, "dev": -7.0, "ang": -8.0},
            {"op": "state"}])),
    ));
    // a tangent-point previous point (not refined: the next point is the domain
    // corner) on the line where z = 0 meets y = 0 (both planes' corner is the origin):
    // a good step back to the corner
    let xz_origin =
        plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let tangent_prev = |x: f64, dx: f64| {
        // evaluation matrix (row major [i][j]): pos, Sv, Svv, Su, Suv, -, -, -, normal
        let mat = |n: [f64; 3], sv: [f64; 3]| {
            let z = [0.0; 3];
            [[x, 0.0, 0.0], sv, z, [1.0, 0.0, 0.0], z, z, z, z, n].concat()
        };
        [
            // SM_IP_TANGENT_POINT, SM_EP_INSIDE x 4
            vec![3.0, 1.0, 1.0, 1.0, 1.0],
            // uv pos, uv tangent on both surfaces
            vec![x, 0.0, dx, 0.0, x, 0.0, dx, 0.0],
            mat([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            mat([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            vec![x, 0.0, 0.0, dx, 0.0, 0.0],
            vec![0.0; 7],
        ]
        .concat()
    };
    let tpts: Vec<Vec<f64>> = [
        tangent_prev(0.5, -1.0),
        tangent_prev(0.05, -1.0),
        tangent_prev(0.5, 1.0),
    ]
    .into_iter()
    .chain(pts.iter().skip(3).cloned())
    .collect();
    for (name, members) in [
        ("edge_next_good_from_tangent_point", json!({"op": "state"})),
        (
            "edge_next_tangent_point_tight_angle",
            json!({"op": "members", "trace_dir": -1.0, "approx_tol": 1e-9, "start_tol": 1e-10,
                   "step_tol": 1e-12, "ang_tol": 0.0}),
        ),
    ] {
        v.push((
            name.into(),
            json!({"a": xy, "b": xz_origin, "dom1": d, "dom2": d, "fillet": false,
                   "curves": [], "points": tpts, "ops": [
                members,
                {"op": "next", "prev": 0, "step": 0.1, "next": 5, "dev": -7.0, "ang": -8.0},
                {"op": "next", "prev": 1, "step": 0.1, "next": 6, "dev": -7.0, "ang": -8.0},
                {"op": "next", "prev": 2, "step": 0.1, "next": 7, "dev": -7.0, "ang": -8.0},
                {"op": "add_curve", "i": 0},
                {"op": "test", "i": 5, "done": false},
                {"op": "state"}]}),
        ));
    }
    v.push((
        "edge_split_zero_normals".into(),
        prog(&xy, &xz, json!([
            cpv(0.1, 0.5, 0, None), cpv(0.2, 0.5, 1, Some(0)),
            {"op": "split", "curr": 0, "next": 1, "delta": 1e-3,
             "normals": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], "average": true, "orient": false,
             "curve": 0, "interval": [0.0, 1.0], "orientations": [1.0, -1.0]}])),
    ));
    // boundary start points of two bumps (part 2's patches) and of a plane
    let h = [
        [0.0, 0.4, 0.4, 0.0],
        [0.4, 0.1, 0.3, 0.4],
        [0.4, 0.2, 0.1, 0.4],
        [0.0, 0.4, 0.4, 0.0],
    ];
    let bump = |x0: f64, mirror: bool| {
        let hs: Vec<Vec<f64>> = h.iter().map(|r| r.to_vec()).collect();
        serde_json::to_value(bump_bspline(1.0, x0, 2, &hs, mirror, [(0.0, 1.0), (0.0, 1.0)]))
            .unwrap_or(Value::Null)
    };
    for (name, bbox, use_edges) in [
        ("edge_fbsp_far_box", [[50.0, 50.0, 50.0], [51.0, 51.0, 51.0]], [true, true]),
        ("edge_fbsp_no_edges", [[-1.0, -1.0, -1.0], [3.0, 2.0, 1.0]], [false, false]),
        ("edge_fbsp_meeting", [[-1.0, -1.0, -1.0], [3.0, 2.0, 1.0]], [true, true]),
    ] {
        v.push((
            name.into(),
            json!({"a": bump(0.0, false), "b": bump(2.0, true), "dom1": d, "dom2": d,
                   "fillet": false, "curves": [], "points": pts,
                   "ops": [{"op": "fbsp", "bbox": bbox, "use_edges": use_edges}, {"op": "state"}]}),
        ));
    }
    v.push((
        "edge_fbsp_plane_edges".into(),
        json!({"a": xy, "b": bump(0.0, false), "dom1": d, "dom2": d,
               "fillet": false, "curves": [], "points": pts,
               "ops": [{"op": "fbsp", "bbox": [[-1.0, -1.0, -1.0], [3.0, 2.0, 1.0]],
                        "use_edges": [true, false]}]}),
    ));
    v
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_intersector_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
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
let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}
