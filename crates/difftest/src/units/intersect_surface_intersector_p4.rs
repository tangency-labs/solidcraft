//! Runner for unit `intersect.surface_intersector_p4`: `SmSurfaceIntersector`, part 4
//! (`TraceCurve`, `FindInteriorCurves`, `DoPointIntersection`, `DoLawIntersection`).
//!
//! C++: `solidcore_ffi::units::intersect_surface_intersector_p4` (a real
//! `SmSurfaceIntersector` on `SmPlane` / `SmBSplineSurface` / `SmCurveBoundedSurface`
//! objects). Rust: `solidcore::intersect::surface_intersector::p4`.
//!
//! A case is two surfaces (B may be A itself), their domains, the fillet flag, some
//! curves, a point arena and a program of operations; both sides produce the record
//! documented in the ffi module (statuses `0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce,
//! [`ffi::REJECTED`] for an op the shim does not run), reported as the values `rec`.
//! A case whose Rust side reaches an unported unit is deferred as a whole.
//!
//! Generated programs run on scenes of two planes: crossing at any angle down to nearly
//! tangent, coincident under another parameterisation, parallel a gap apart, or the
//! same object, at scales `1e-6` to `1e5`. `DoPointIntersection` through one to five
//! points on (or just off) the intersection, in or against the order of the trace, with
//! and without start and end directions, extensions, tolerances and extension clipping;
//! `TraceCurve` from evaluated or raw points (types the basic intersector does not
//! refine: unknown, tangent, coincident, touching, ...) with and without curve points,
//! targets and curves the trace must avoid, then `FlushCurve`; `DoLawIntersection` with
//! zero to three points and `FindInteriorCurves`; a few programs on B-spline and
//! curve-bounded surfaces.
//!
//! Deferred cases: the trace step from a crossing, tangent-curve or singular point
//! (`SmSurface::LocalSurfaceIntersect`, `surface.surface_p11`), the global surface solve
//! of `FindInteriorCurves` (`surface.surface_p11`), and `ComputePointValues` on B-spline
//! or curve-bounded surfaces (`SmSurface::IsSingularity`, `surface.surface_p5`).
//! `DoLawIntersection` past its offset-surface check is unreachable: no Rust surface is
//! an `SmOffsetSurface` yet (`surface.offset_surface`), so the shim builds none.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::blend::fillet_standard_solver::p1::{FilletLaw, FilletLawKind};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::{Extent1d, ExtentPointType};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{
    IntersectionPointType, SurfaceIntersector, TsectId, TsectPnt,
};
use solidcore::intersect::surface_intersector::p2::TsectCurves;
use solidcore::intersect::surface_intersector::p4::{
    IntersectionCurve, LawIntersectionOptions, PointIntersectionOptions,
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
use solidcore::SolidError;
use solidcore_ffi::units::intersect_surface_intersector_p4::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_intersector_p4` runner.
pub struct Runner;

/// No start or end point in the state record.
const NO_POINT: f64 = -3.0;

/// An output upstream leaves unset (the shim presets it).
const UNSET: f64 = -7.0;

/// The curve type the shim presets (`(SmTsectCurveType)-7`) as the record holds it:
/// `(double)type` of an enum with an unsigned underlying type, `2^32 - 7`.
const UNSET_TYPE: f64 = 4_294_967_289.0;

// ---- inputs ---------------------------------------------------------------------------

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
    Trace {
        i: usize,
    },
    Flush,
    Fic,
    Dpi {
        points: Vec<usize>,
        before: bool,
        after: bool,
        start_dir: Option<[f64; 3]>,
        end_dir: Option<[f64; 3]>,
        tol: Option<f64>,
        ang_tol: Option<f64>,
    },
    Dli {
        points: Vec<usize>,
        orient: bool,
        curve: usize,
        interval: [f64; 2],
        orientations: [f64; 2],
        average: bool,
        tol: Option<f64>,
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
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
        OpIn::Trace { i } => ffi::Op::Trace(*i),
        OpIn::Flush => ffi::Op::Flush,
        OpIn::Fic => ffi::Op::Fic,
        OpIn::Dpi {
            points,
            before,
            after,
            start_dir,
            end_dir,
            tol,
            ang_tol,
        } => ffi::Op::Dpi(ffi::DpiOp {
            points: points.clone(),
            before: *before,
            after: *after,
            start_dir: *start_dir,
            end_dir: *end_dir,
            tol: *tol,
            ang_tol: *ang_tol,
        }),
        OpIn::Dli {
            points,
            orient,
            curve,
            interval,
            orientations,
            average,
            tol,
        } => ffi::Op::Dli(ffi::DliOp {
            points: points.clone(),
            orient: *orient,
            curve: *curve,
            interval: *interval,
            orientations: *orientations,
            average: *average,
            tol: *tol,
        }),
    }
}

/// The run's state besides the intersector: program curves and the output arrays.
struct Run {
    curves: Vec<Curve>,
    outs: TsectCurves,
    nprog: usize,
}

/// The shim's rejection of a `Dpi`/`Dli`: a point twice or a point on a list.
fn points_rejected(si: &SurfaceIntersector<'_>, points: &[usize]) -> bool {
    points
        .iter()
        .enumerate()
        .any(|(k, &i)| si.is_linked(id(i)) || points.iter().take(k).any(|&j| j == i))
}

/// `has [curve]` x 3, type, deviation.
fn put_single(o: &mut Vec<f64>, c: &IntersectionCurve) -> Result<(), Fail> {
    for crv in [&c.curve_3d, &c.uv_curves[0], &c.uv_curves[1]] {
        match crv {
            None => o.push(0.0),
            Some(crv) => {
                o.push(1.0);
                put_bspline(o, crv)?;
            }
        }
    }
    o.push(c.curve_type.map_or(UNSET_TYPE, |t| f64::from(t as i32)));
    o.push(c.deviation.unwrap_or(UNSET));
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_op(run: &mut Run, si: &mut SurfaceIntersector<'_>, op: &OpIn, o: &mut Vec<f64>) -> Result<(), Fail> {
    let nprog = run.nprog;
    let k = Kernel::default();
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
                    OpIn::AddStart { .. } => si.add_start_point(&k, id(*i)),
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
        OpIn::Trace { i } => {
            pt_of(si, *i)?;
            if si.curve_points().is_empty() && si.is_linked(id(*i)) {
                o.push(ffi::REJECTED);
            } else {
                let r = si.trace_curve(&k, id(*i), &run.outs.curves_3d);
                st(o, r)?;
            }
        }
        OpIn::Flush | OpIn::Fic => {
            let mut outs = core::mem::take(&mut run.outs);
            let r = if matches!(op, OpIn::Flush) {
                si.flush_curve(&k, &mut outs)
            } else {
                si.find_interior_curves(&k, &mut outs)
            };
            run.outs = outs;
            st(o, r)?;
            put_outs(o, &run.outs, run.curves.len())?;
        }
        OpIn::Dpi {
            points,
            before,
            after,
            start_dir,
            end_dir,
            tol,
            ang_tol,
        } => {
            if points_rejected(si, points) {
                o.push(ffi::REJECTED);
            } else {
                let ids: Vec<TsectId> = points.iter().map(|&i| id(i)).collect();
                let opts = PointIntersectionOptions {
                    extend_before_start: *before,
                    extend_after_end: *after,
                    start_direction: start_dir.map(v3),
                    end_direction: end_dir.map(v3),
                    approx_tol_3d: *tol,
                    ang_tol_rad: *ang_tol,
                };
                let r = si.do_point_intersection(&k, &ids, &opts);
                if let Some(c) = st(o, r)? {
                    put_single(o, &c)?;
                }
            }
        }
        OpIn::Dli {
            points,
            orient,
            curve,
            interval,
            orientations,
            average,
            tol,
        } => {
            let curve = run.curves.get(*curve).ok_or_else(|| harness("curve index"))?;
            if points_rejected(si, points) {
                o.push(ffi::REJECTED);
            } else {
                let ids: Vec<TsectId> = points.iter().map(|&i| id(i)).collect();
                let opts = LawIntersectionOptions {
                    law_orient: *orient,
                    point_curve: curve,
                    point_curve_interval: Extent1d::from_min_max(interval[0], interval[1]),
                    surface_orientations: *orientations,
                    use_normal_plane_averaging: *average,
                    approx_tol_3d: *tol,
                };
                // the shim's default-constructed base `SmFilletLaw`
                let mut law = FilletLaw::new(FilletLawKind::Base);
                let r = si.do_law_intersection(&k, &ids, &opts, &mut law);
                if let Some(c) = st(o, r)? {
                    put_single(o, &c)?;
                }
            }
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
        OpIn::Trace { .. } => 10,
        OpIn::Flush => 11,
        OpIn::Dpi { .. } => 12,
        OpIn::Dli { .. } => 13,
        OpIn::Fic => 14,
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
        nprog: i.points.len(),
    };
    for op in &i.ops {
        o.push(op_code(op));
        run_op(&mut run, &mut si, op, &mut o)?;
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

// ---- generation: scenes ---------------------------------------------------------------------

/// The relative position of the two surfaces of a scene.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SceneKind {
    /// Planes crossing at an angle.
    Crossing,
    /// The same plane under another parameterisation.
    Coincident,
    /// Parallel planes a gap apart.
    Parallel,
    /// B is A itself.
    Same,
}

/// Two planes and a map from A's UV points to B's (the closest point of B).
struct Scene {
    kind: SceneKind,
    s: f64,
    a: SurfaceIn,
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    /// A's origin and axes.
    fa: ([f64; 3], [[f64; 3]; 2]),
    /// B's origin and axes.
    fb: ([f64; 3], [[f64; 3]; 2]),
    /// The crossing line `v = c` on A (crossing scenes).
    line_v: f64,
}

fn dot(a: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * c[0] + a[1] * c[1] + a[2] * c[2]
}

fn sub3(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [a[0] - c[0], a[1] - c[1], a[2] - c[2]]
}

impl Scene {
    /// The 3D point of A's `uv`.
    fn pos_a(&self, uv: [f64; 2]) -> [f64; 3] {
        let (o, [x, y]) = self.fa;
        add3(add3(o, scale3(x, uv[0])), scale3(y, uv[1]))
    }

    /// B's UV point closest to A's `uv`.
    fn uv_b(&self, uv: [f64; 2]) -> [f64; 2] {
        let p = self.pos_a(uv);
        let (o, [x, y]) = self.fb;
        let d = sub3(p, o);
        [dot(d, x), dot(d, y)]
    }

    /// A's direction of increasing u, in 3D.
    fn dir_u(&self) -> [f64; 3] {
        self.fa.1[0]
    }
}

fn gen_scene(rng: &mut Rng) -> Scene {
    let s = gen_scale(rng);
    let f = frame(rng);
    let t = rng.point3(-s, s);
    let dom = [0.0, 0.0, s, s];
    let kind = match rng.below(10) {
        0..=3 => SceneKind::Crossing,
        4..=6 => SceneKind::Coincident,
        7 | 8 => SceneKind::Parallel,
        _ => SceneKind::Same,
    };
    let a = plane_in(t, f[0], f[1], dom, [1.0, 1.0]);
    let mut line_v = 0.0;
    let (ob, xb, yb, dom2) = match kind {
        SceneKind::Crossing => {
            let theta = pick_f(
                rng,
                &[core::f64::consts::FRAC_PI_2, 0.7, 0.1, 1e-3, 1e-6, 2.5],
            );
            let pick_v = |rng: &mut Rng| {
                s * match rng.below(8) {
                    0 => 0.0,
                    1 => 1.0,
                    _ => rng.uniform(0.1, 0.9),
                }
            };
            let c = pick_v(rng);
            let h = pick_v(rng);
            line_v = c;
            let w = mapv(&f, [0.0, theta.cos(), theta.sin()]);
            let ob = add3(add3(t, mapv(&f, [0.0, c, 0.0])), scale3(w, -h));
            (ob, f[0], w, dom)
        }
        SceneKind::Coincident | SceneKind::Parallel | SceneKind::Same => {
            // B's axes: A's turned by phi in the plane, maybe with y flipped
            let phi = pick_f(rng, &[0.0, 0.0, 0.5, core::f64::consts::PI, 2.0]);
            let (c, sn) = (phi.cos(), phi.sin());
            let xb = add3(scale3(f[0], c), scale3(f[1], sn));
            let mut yb = add3(scale3(f[0], -sn), scale3(f[1], c));
            if rng.chance(0.3) {
                yb = scale3(yb, -1.0);
            }
            let gap = if kind == SceneKind::Parallel {
                s * pick_f(rng, &[1e-12, 1e-8, 1e-5, 1e-3, 0.1])
            } else {
                0.0
            };
            let shift = [s * rng.uniform(-0.5, 0.5), s * rng.uniform(-0.5, 0.5), gap];
            let ob = add3(t, mapv(&f, shift));
            (ob, xb, yb, [-3.0 * s, -3.0 * s, 3.0 * s, 3.0 * s])
        }
    };
    let b = (kind != SceneKind::Same).then(|| plane_in(ob, xb, yb, dom2, [1.0, 1.0]));
    let (fb, dom2) = if kind == SceneKind::Same {
        ((t, [f[0], f[1]]), dom)
    } else {
        ((ob, [xb, yb]), dom2)
    };
    let dom1 = if rng.chance(0.85) { dom } else { gen_domain(rng, dom) };
    Scene {
        kind,
        s,
        a,
        b,
        dom1,
        dom2,
        fa: (t, [f[0], f[1]]),
        fb,
        line_v,
    }
}

/// A point of type `ty` at A's `uv0` and B's `uv1` with UV tangents `duv` (both) and
/// parameter `param`; everything else zero.
fn uv_point(ty: f64, uv0: [f64; 2], uv1: [f64; 2], duv: [[f64; 2]; 2], param: f64) -> Vec<f64> {
    let mut v = blank_point(param);
    if let Some(t) = v.get_mut(0) {
        *t = ty;
    }
    for (k, x) in [uv0, duv[0], uv1, duv[1]].concat().into_iter().enumerate() {
        if let Some(slot) = v.get_mut(5 + k) {
            *slot = x;
        }
    }
    v
}

/// UV points of A along the intersection (or a line of A for coincident scenes).
fn gen_uvs(rng: &mut Rng, sc: &Scene, n: usize) -> Vec<[f64; 2]> {
    let s = sc.s;
    let step = s * pick_f(rng, &[0.1, 0.2, 0.05, 1e-4, 0.3]);
    let mid = rng_mid(rng);
    let x0 = s * pick_f(rng, &[0.0, 0.1, 0.3, mid, 1.0]);
    let dir = if rng.chance(0.25) { -1.0 } else { 1.0 };
    let (v0, dv) = if sc.kind == SceneKind::Crossing {
        (sc.line_v, 0.0)
    } else {
        (s * rng.uniform(0.0, 1.0), s * rng.uniform(-0.1, 0.1))
    };
    (0..n)
        .map(|i| {
            let k = i as f64;
            let mut uv = [x0 + dir * k * step, v0 + k * dv];
            if rng.chance(0.08) {
                // off the intersection
                uv[1] += s * pick_f(rng, &[1e-3, 0.05, 1e-7]);
            }
            uv
        })
        .collect()
}

fn rng_mid(rng: &mut Rng) -> f64 {
    rng.uniform(0.05, 0.5)
}

/// A direction: along A's u (either way), random, or tiny.
fn gen_dir(rng: &mut Rng, sc: &Scene) -> Option<[f64; 3]> {
    match rng.below(6) {
        0 | 1 => None,
        2 => Some(sc.dir_u()),
        3 => Some(scale3(sc.dir_u(), -1.0)),
        4 => Some(unit_vec(rng)),
        _ => Some(scale3(sc.dir_u(), 1e-12)),
    }
}

fn gen_tol(rng: &mut Rng, s: f64) -> Option<f64> {
    if rng.chance(0.5) {
        None
    } else {
        Some(s * pick_f(rng, &[1e-3, 1e-9, 0.5, 1e-6, 1e-2]))
    }
}

fn gen_ext(rng: &mut Rng, s: f64) -> OpIn {
    OpIn::Ext {
        clip: rng.chance(0.3),
        last_through: pick_f(rng, &[0.0, 0.1 * s]),
        distance: pick_f(rng, &[0.0, 0.1 * s, 1e-3 * s, -1.0]),
    }
}

/// A raw point type: mostly ones the basic intersector does not refine.
fn gen_raw_type(rng: &mut Rng) -> f64 {
    pick_f(rng, &[1.0, 1.0, 3.0, 3.0, 3.0, 6.0, 7.0, 0.0, 2.0, 4.0, 5.0])
}

/// A raw point at A's `uv` (and B's matching point) with a random type, tangent and
/// parameter.
fn gen_raw_point(rng: &mut Rng, sc: &Scene, uv: [f64; 2], param: f64) -> Vec<f64> {
    let ty = gen_raw_type(rng);
    let duv = match rng.below(5) {
        0 => [[1.0, 0.0], [1.0, 0.0]],
        1 => [[-1.0, 0.0], [-1.0, 0.0]],
        2 => [[0.0, 0.0], [0.0, 0.0]],
        _ => {
            let a = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
            [a, [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)]]
        }
    };
    let mut v = uv_point(ty, uv, sc.uv_b(uv), duv, param);
    // a 3D position and tangent for the on-curve tests
    let p = sc.pos_a(uv);
    let d = if rng.chance(0.8) { sc.dir_u() } else { unit_vec(rng) };
    for (k, x) in [p, d].concat().into_iter().enumerate() {
        if let Some(slot) = v.get_mut(67 + k) {
            *slot = x;
        }
    }
    v
}

/// A segment along A's u direction through A's `uv` (a curve the traces must avoid).
fn segment_through(sc: &Scene, uv: [f64; 2]) -> CurveIn {
    let p = sc.pos_a(uv);
    let d = scale3(sc.dir_u(), sc.s);
    CurveIn::Segment {
        a: sub3(p, d),
        b: add3(p, d),
    }
}

/// `DoPointIntersection` programs.
fn gen_dpi_case(rng: &mut Rng, sc: &Scene) -> (Vec<CurveIn>, Vec<Vec<f64>>, Vec<OpIn>) {
    let s = sc.s;
    let n = 1 + rng.below(5) as usize;
    let uvs = gen_uvs(rng, sc, n);
    let points: Vec<Vec<f64>> = uvs
        .iter()
        .map(|&uv| uv_point(pick_f(rng, &[1.0, 2.0, 0.0]), uv, sc.uv_b(uv), [[0.0; 2]; 2], 7.0))
        .collect();
    let mut ops = Vec::new();
    if rng.chance(0.3) {
        ops.push(gen_members(rng, s));
    }
    if rng.chance(0.3) {
        ops.push(gen_ext(rng, s));
    }
    let k = match rng.below(10) {
        0 => 0,
        _ => 1 + rng.below(n as u64) as usize,
    };
    let mut ids: Vec<usize> = (0..k).collect();
    if rng.chance(0.15) {
        ids.reverse();
    }
    if rng.chance(0.03) {
        if let Some(&f) = ids.first() {
            ids.push(f);
        }
    }
    ops.push(OpIn::Dpi {
        points: ids,
        before: rng.chance(0.4),
        after: rng.chance(0.4),
        start_dir: gen_dir(rng, sc),
        end_dir: gen_dir(rng, sc),
        tol: gen_tol(rng, s),
        ang_tol: if rng.chance(0.7) {
            None
        } else {
            Some(pick_f(rng, &[0.35, 1e-3, 1.0]))
        },
    });
    ops.push(OpIn::State);
    let mut curves = Vec::new();
    let tail = rng.below(20);
    if tail < 3 {
        // a second run on the leftover state
        ops.push(OpIn::Flush);
        ops.push(OpIn::Dpi {
            points: (0..n).rev().collect(),
            before: false,
            after: true,
            start_dir: None,
            end_dir: None,
            tol: None,
            ang_tol: None,
        });
    } else if tail < 6 {
        curves.push(segment_through(sc, uvs.first().copied().unwrap_or([0.0; 2])));
    }
    (curves, points, ops)
}

/// `TraceCurve` programs: from evaluated or raw start points, with or without curve
/// points, targets and curves to avoid, then `FlushCurve`.
fn gen_trace_case(rng: &mut Rng, sc: &Scene) -> (Vec<CurveIn>, Vec<Vec<f64>>, Vec<OpIn>) {
    let s = sc.s;
    let n = 2 + rng.below(4) as usize;
    let uvs = gen_uvs(rng, sc, n);
    let mut points = Vec::new();
    let mut ops = Vec::new();
    if rng.chance(0.4) {
        ops.push(gen_members(rng, s));
    }
    let raw = rng.chance(0.5);
    for (i, &uv) in uvs.iter().enumerate() {
        if raw {
            points.push(gen_raw_point(rng, sc, uv, i as f64 * 0.1 * s));
        } else {
            points.push(blank_point(i as f64 * 0.1 * s));
            ops.push(OpIn::Cpv {
                uvs: [uv, sc.uv_b(uv)],
                i,
                prev: (i > 0 && rng.chance(0.5)).then(|| i - 1),
                step: None,
            });
        }
    }
    // curve points so far
    let nc = match rng.below(3) {
        0 => 0,
        1 => 1,
        _ => rng.below(n as u64) as usize,
    };
    for i in 0..nc {
        ops.push(OpIn::AddCurve { i });
    }
    // targets
    if rng.chance(0.3) {
        let t = rng.below(n as u64) as usize;
        match rng.below(3) {
            0 => ops.push(OpIn::AddThrough { i: t }),
            1 => ops.push(OpIn::AddStart { i: t }),
            _ => ops.push(OpIn::Ends {
                start: rng.chance(0.5).then_some(0),
                end: Some(t),
            }),
        }
    }
    if rng.chance(0.2) {
        ops.push(gen_ext(rng, s));
    }
    let mut curves = Vec::new();
    if rng.chance(0.25) {
        let uv = if rng.chance(0.7) {
            uvs.get(nc.min(n - 1)).copied().unwrap_or([0.0; 2])
        } else {
            [0.5 * s, 0.5 * s]
        };
        curves.push(segment_through(sc, uv));
    }
    let start = nc.min(n - 1);
    ops.push(OpIn::Trace { i: start });
    if rng.chance(0.2) {
        ops.push(OpIn::Trace {
            i: rng.below(n as u64) as usize,
        });
    }
    ops.push(OpIn::Flush);
    ops.push(OpIn::State);
    (curves, points, ops)
}

/// `DoLawIntersection` and `FindInteriorCurves` programs.
fn gen_law_or_interior_case(rng: &mut Rng, sc: &Scene) -> (Vec<CurveIn>, Vec<Vec<f64>>, Vec<OpIn>) {
    let s = sc.s;
    let uvs = gen_uvs(rng, sc, 3);
    let points: Vec<Vec<f64>> = uvs
        .iter()
        .map(|&uv| gen_raw_point(rng, sc, uv, 0.0))
        .collect();
    let curves = vec![segment_through(sc, uvs.first().copied().unwrap_or([0.0; 2]))];
    let mut ops = Vec::new();
    if rng.chance(0.3) {
        ops.push(gen_members(rng, s));
    }
    if rng.chance(0.5) {
        let k = pick_f(rng, &[2.0, 2.0, 2.0, 0.0, 1.0, 3.0]) as usize;
        ops.push(OpIn::Dli {
            points: (0..k).collect(),
            orient: rng.chance(0.5),
            curve: 0,
            interval: [0.0, pick_f(rng, &[1.0, s, 0.0])],
            orientations: [pick_f(rng, &[1.0, -1.0]), pick_f(rng, &[1.0, -1.0])],
            average: rng.chance(0.5),
            tol: gen_tol(rng, s),
        });
    } else {
        ops.push(OpIn::Fic);
    }
    ops.push(OpIn::State);
    (curves, points, ops)
}

/// Programs on B-spline and curve-bounded surfaces.
fn gen_other_surfaces_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let (a, b) = match rng.below(4) {
        0 => (gen_plane(rng, s, None), Some(gen_bspline(rng, s, false))),
        1 => {
            let tube = rng.chance(0.3);
            (gen_bspline(rng, s, false), Some(gen_bspline(rng, s, tube)))
        }
        2 => (bounded(gen_plane(rng, s, None)), Some(gen_plane(rng, s, None))),
        _ => (gen_bspline(rng, s, false), None),
    };
    let na = natural(&a);
    let nb = natural(b.as_ref().unwrap_or(&a));
    let doms = [na, nb];
    let n = 2 + rng.below(3) as usize;
    let points: Vec<Vec<f64>> = (0..n).map(|i| gen_point(rng, doms, s, i as f64)).collect();
    let mut ops = Vec::new();
    match rng.below(4) {
        0 => ops.push(OpIn::Dpi {
            points: (0..n).collect(),
            before: rng.chance(0.5),
            after: rng.chance(0.5),
            start_dir: None,
            end_dir: None,
            tol: None,
            ang_tol: None,
        }),
        1 => ops.push(OpIn::Fic),
        2 => {
            ops.push(OpIn::AddCurve { i: 0 });
            ops.push(OpIn::Trace { i: 1 });
            ops.push(OpIn::Flush);
        }
        _ => ops.push(OpIn::Trace { i: 0 }),
    }
    ops.push(OpIn::State);
    Inputs {
        a,
        b,
        dom1: gen_domain(rng, na),
        dom2: gen_domain(rng, nb),
        fillet: rng.chance(0.1),
        curves: Vec::new(),
        points,
        ops,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.below(20) == 0 {
        return gen_other_surfaces_case(rng);
    }
    let sc = gen_scene(rng);
    let (curves, points, ops) = match rng.below(20) {
        0..=8 => gen_dpi_case(rng, &sc),
        9..=16 => gen_trace_case(rng, &sc),
        _ => gen_law_or_interior_case(rng, &sc),
    };
    Inputs {
        a: sc.a,
        b: sc.b,
        dom1: sc.dom1,
        dom2: sc.dom2,
        fillet: rng.chance(0.05),
        curves,
        points,
        ops,
    }
}

// ---- inline edge cases ------------------------------------------------------------------

fn plane_json(o: [f64; 3], x: [f64; 3], y: [f64; 3], d: [f64; 4]) -> Value {
    json!({"kind": "plane", "o": o, "x": x, "y": y, "scale": [1.0, 1.0], "domain": d})
}

#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    let d = [0.0, 0.0, 1.0, 1.0];
    // z = 0 over [0, 1]^2; the same plane parameterised from (1, 1) with -x, y flipped
    let xy = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], d);
    let xy_rev = plane_json([1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], d);
    let xy_up = plane_json([0.0, 0.0, 1e-4], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], d);
    // y = 0.5: crosses z = 0 along x at v = 0.5 on both
    let xz = plane_json([0.0, 0.5, -0.5], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], d);
    // a point at A's (u, v): B = xy_rev's UV is (1 - u, 1 - v)
    let rev = |u: f64, v: f64, ty: f64| uv_point(ty, [u, v], [1.0 - u, 1.0 - v], [[0.0; 2]; 2], 7.0);
    let same = |u: f64, v: f64, ty: f64, du: [f64; 2]| uv_point(ty, [u, v], [u, v], [du, du], 0.0);
    let pts_rev: Vec<Vec<f64>> = vec![rev(0.2, 0.3, 1.0), rev(0.6, 0.3, 1.0), rev(0.9, 0.9, 2.0)];
    let prog = |a: &Value, bb: &Value, curves: Value, pts: &[Vec<f64>], ops: Value| {
        json!({"a": a, "b": bb, "dom1": d, "dom2": d, "fillet": false,
               "curves": curves, "points": pts, "ops": ops})
    };
    let dpi = |points: Value, before: bool, after: bool, sd: Value, tol: Value| {
        json!({"op": "dpi", "points": points, "before": before, "after": after,
               "start_dir": sd, "end_dir": null, "tol": tol, "ang_tol": null})
    };
    let seg = |a: [f64; 3], b: [f64; 3]| json!([{"kind": "segment", "a": a, "b": b}]);
    let mut v = vec![
        (
            "edge_dpi_coincident_single_point".into(),
            prog(&xy, &xy_rev, json!([]), &pts_rev,
                 json!([dpi(json!([0]), false, false, Value::Null, Value::Null), {"op": "state"}])),
        ),
        (
            "edge_dpi_coincident_three_points".into(),
            prog(&xy, &xy_rev, json!([]), &pts_rev,
                 json!([dpi(json!([0, 1, 2]), true, true, json!([1.0, 0.0, 0.0]), Value::Null),
                        {"op": "state"}])),
        ),
        (
            "edge_dpi_coincident_on_a_curve".into(),
            prog(&xy, &xy_rev, seg([0.0, 0.3, 0.0], [1.0, 0.3, 0.0]), &pts_rev,
                 json!([{"op": "trace", "i": 2}, {"op": "flush"},
                        dpi(json!([0]), false, false, Value::Null, Value::Null), {"op": "state"}])),
        ),
        (
            "edge_dpi_no_points".into(),
            prog(&xy, &xz, json!([]), &pts_rev,
                 json!([dpi(json!([]), false, false, Value::Null, json!(0.5)), {"op": "state"}])),
        ),
        (
            "edge_dpi_first_point_off".into(),
            prog(&xy, &xy_up, json!([]), &[same(0.5, 0.5, 1.0, [0.0, 0.0])],
                 json!([dpi(json!([0]), false, false, Value::Null, Value::Null), {"op": "state"}])),
        ),
        (
            "edge_dpi_parallel_within_tol".into(),
            prog(&xy, &xy_up, json!([]), &[same(0.5, 0.5, 1.0, [0.0, 0.0])],
                 json!([dpi(json!([0]), false, false, Value::Null, json!(1e-3)), {"op": "state"}])),
        ),
        (
            "edge_dpi_rejected".into(),
            prog(&xy, &xy_rev, json!([]), &pts_rev,
                 json!([dpi(json!([0, 0]), false, false, Value::Null, Value::Null),
                        {"op": "add_through", "i": 1},
                        dpi(json!([0, 1]), false, false, Value::Null, Value::Null),
                        {"op": "state"}])),
        ),
    ];
    // traces on z = 0 / y = 0.5 from raw points
    let raw = |ty: f64, u: f64, du: [f64; 2]| {
        let mut p = same(u, 0.5, ty, du);
        for (k, x) in [u, 0.5, 0.0, du[0], 0.0, 0.0].into_iter().enumerate() {
            if let Some(s) = p.get_mut(67 + k) {
                *s = x;
            }
        }
        p
    };
    let raws = vec![
        raw(3.0, 0.5, [1.0, 0.0]),
        raw(1.0, 0.5, [1.0, 0.0]),
        raw(1.0, 1.0, [1.0, 0.0]),
        raw(6.0, 0.2, [1.0, 0.0]),
        raw(7.0, 0.0, [-1.0, 0.0]),
    ];
    for (name, curves, ops) in [
        ("edge_trace_tangent_start", json!([]), json!([{"op": "trace", "i": 0}, {"op": "flush"}])),
        (
            "edge_trace_tangent_start_on_curve",
            seg([0.0, 0.5, 0.0], [1.0, 0.5, 0.0]),
            json!([{"op": "trace", "i": 0}, {"op": "flush"}]),
        ),
        ("edge_trace_coincidence_start", json!([]), json!([{"op": "trace", "i": 3}])),
        ("edge_trace_unknown_start", json!([]), json!([{"op": "trace", "i": 1}, {"op": "flush"}])),
        (
            "edge_trace_unknown_start_reversed",
            json!([]),
            json!([{"op": "trace", "i": 2}, {"op": "flush"}]),
        ),
        (
            "edge_trace_bounded_start_on_curve",
            seg([0.0, 0.5, 0.0], [1.0, 0.5, 0.0]),
            json!([{"op": "trace", "i": 2}, {"op": "flush"}]),
        ),
        (
            "edge_trace_touching_start_at_corner",
            json!([]),
            json!([{"op": "trace", "i": 4}, {"op": "trace", "i": 3}, {"op": "flush"}]),
        ),
        (
            "edge_trace_from_curve_points",
            json!([]),
            json!([{"op": "add_curve", "i": 1}, {"op": "add_curve", "i": 0},
                   {"op": "trace", "i": 3}, {"op": "flush"}]),
        ),
        (
            "edge_trace_rejected",
            json!([]),
            json!([{"op": "add_start", "i": 1}, {"op": "trace", "i": 1}]),
        ),
        (
            "edge_law_point_counts",
            seg([0.0, 0.5, 0.0], [1.0, 0.5, 0.0]),
            json!([{"op": "dli", "points": [0], "orient": false, "curve": 0, "interval": [0.0, 1.0],
                    "orientations": [1.0, -1.0], "average": false, "tol": null},
                   {"op": "dli", "points": [0, 1], "orient": true, "curve": 0, "interval": [0.0, 1.0],
                    "orientations": [1.0, -1.0], "average": true, "tol": 0.25},
                   {"op": "dli", "points": [1, 1], "orient": true, "curve": 0, "interval": [0.0, 1.0],
                    "orientations": [1.0, -1.0], "average": true, "tol": 0.25}]),
        ),
        ("edge_interior_planes", json!([]), json!([{"op": "fic"}])),
    ] {
        let mut ops = ops.as_array().cloned().unwrap_or_default();
        ops.push(json!({"op": "state"}));
        v.push((name.into(), prog(&xy, &xz, curves, &raws, Value::Array(ops))));
    }
    // a tangent point stepping back to the corner where z = 0 meets y = 0
    let xz_origin = plane_json([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], d);
    let tangent_prev = |x: f64, dx: f64| {
        let mat = |n: [f64; 3], sv: [f64; 3]| {
            let z = [0.0; 3];
            [[x, 0.0, 0.0], sv, z, [1.0, 0.0, 0.0], z, z, z, z, n].concat()
        };
        [
            vec![3.0, 1.0, 1.0, 1.0, 1.0],
            vec![x, 0.0, dx, 0.0, x, 0.0, dx, 0.0],
            mat([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            mat([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            vec![x, 0.0, 0.0, dx, 0.0, 0.0],
            vec![0.0; 7],
        ]
        .concat()
    };
    let tpts = vec![tangent_prev(0.5, -1.0), tangent_prev(0.05, -1.0), tangent_prev(0.5, 1.0)];
    for (name, i) in [
        ("edge_trace_tangent_curve_point_to_corner", 0),
        ("edge_trace_tangent_curve_point_near_corner", 1),
        ("edge_trace_tangent_curve_point_away", 2),
    ] {
        v.push((
            name.into(),
            json!({"a": xy, "b": xz_origin, "dom1": d, "dom2": d, "fillet": false,
                   "curves": [], "points": tpts, "ops": [
                       {"op": "add_curve", "i": i}, {"op": "trace", "i": i}, {"op": "flush"},
                       {"op": "state"}]}),
        ));
    }
    v.push((
        "edge_interior_self".into(),
        json!({"a": xy, "b": null, "dom1": d, "dom2": d, "fillet": false, "curves": [],
               "points": [], "ops": [{"op": "fic"}, {"op": "state"}]}),
    ));
    v
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_intersector_p4"
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
