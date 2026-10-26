//! Runner for unit `surface.plane_p1`: `SmPlane`, part 1.
//!
//! C++: `solidcore_ffi::units::surface_plane_p1` (upstream `source/SMLib/src/SmPlane.cpp`
//! on `SmPlane` objects). Rust: `solidcore::surface::plane`.
//!
//! A case is a plane construction (`make`) and a program of operations (`ops`) run on
//! that plane in order; both sides produce the record documented in the ffi module
//! (statuses `0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//! `InvalidInput`, `-1` for a Rust error upstream cannot produce), reported as the
//! values `rec`. A case whose Rust side reaches an unported unit is deferred as a
//! whole.
//!
//! Deferred cases: `CoincidenceCheck` with a surface that is not a plane
//! (`SmSurface::IsPlanar`, `surface.surface_p7`), `DropCurve` falling back to the
//! general `SmSurface::DropCurve` (ported, `intersect.surface_drop_lib_p3`) where that
//! reaches a unit not ported yet, and `GlobalLineIntersect` of a line in the plane meeting the domain in one line
//! parameter (`SmSurface::GlobalLineIntersect`, `surface.surface_p8`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::solution_array::{ObjectKind, SolutionArray, SolutionObject, SolutionType};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::p1::is_grazing_intersection;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::surface_plane_p1::{self as ffi};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `surface.plane_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum MakeIn {
    Bounded {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    Normal {
        o: [f64; 3],
        n: [f64; 3],
    },
    Canonical {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DomIn {
    Step,
    Natural,
    Explicit([f64; 4]),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineCurveIn {
    dim: u32,
    p: usize,
    /// `[x, y, z, w]`; `w` = `null` for a non-rational point.
    cpts: Vec<[Option<f64>; 4]>,
    knots: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum CurveIn {
    Segment { a: [f64; 3], b: [f64; 3] },
    BSpline(BSplineCurveIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineSurfaceIn {
    p: usize,
    q: usize,
    /// Rows (u index) of `[x, y, z, w]`; `w` = `null` for a non-rational point.
    net: Vec<Vec<[Option<f64>; 4]>>,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum SurfaceIn {
    Plane { make: MakeIn },
    BSpline(BSplineSurfaceIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum OpIn {
    State,
    Evaluate {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        #[serde(default)]
        upper: bool,
    },
    EvalPoint {
        uv: [f64; 2],
    },
    EvalFast {
        uv: [f64; 2],
    },
    Normal {
        uv: [f64; 2],
    },
    Project {
        p: [f64; 3],
    },
    Queries {
        uv: [f64; 2],
    },
    KindOf {
        t: u32,
    },
    Adjust {
        dom: DomIn,
    },
    MakeNurb,
    Reparam {
        dom: DomIn,
    },
    Toggle,
    Rebuild,
    Drop {
        dom: DomIn,
        sop: u8,
        p: [f64; 3],
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<f64>,
    },
    Line {
        dom: DomIn,
        p: [f64; 3],
        v: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ivl: Option<[f64; 2]>,
        #[serde(default)]
        ray: bool,
        tol: f64,
    },
    TrimLine {
        dom: DomIn,
        a: [f64; 3],
        b: [f64; 3],
        tol: f64,
    },
    Iso {
        #[serde(default)]
        v_dir: bool,
        iso: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        domain: Option<[f64; 4]>,
        #[serde(default)]
        want_uv: bool,
    },
    DropCurve {
        dom: DomIn,
        curve: CurveIn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ivl: Option<[f64; 2]>,
        tol: f64,
        #[serde(default)]
        keep_all: bool,
        #[serde(default)]
        creating: bool,
    },
    Coincidence {
        other: SurfaceIn,
        tol: f64,
        #[serde(default)]
        whole: bool,
    },
    Eq {
        other: MakeIn,
    },
    Copy,
    Grazing {
        curve: CurveIn,
        t: f64,
        line: [[f64; 3]; 2],
        s: f64,
    },
    Inherited {
        v: [f64; 3],
    },
    Reverse {
        #[serde(default)]
        v_dir: bool,
    },
    Swap,
    Extend {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir: Option<u8>,
        dist: f64,
        cont: u8,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    make: MakeIn,
    ops: Vec<OpIn>,
}

// ---- C++ side ---------------------------------------------------------------------

fn ffi_make(m: &MakeIn) -> ffi::Make {
    match m {
        MakeIn::Bounded {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::Make::Bounded {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
        MakeIn::Normal { o, n } => ffi::Make::Normal { o: *o, n: *n },
        MakeIn::Canonical { o, x, y } => ffi::Make::Canonical {
            o: *o,
            x: *x,
            y: *y,
        },
    }
}

fn ffi_dom(d: DomIn) -> ffi::Dom {
    match d {
        DomIn::Step => ffi::Dom::Step,
        DomIn::Natural => ffi::Dom::Natural,
        DomIn::Explicit(v) => ffi::Dom::Explicit(v),
    }
}

fn weight(w: Option<f64>) -> f64 {
    w.unwrap_or(NOW)
}

fn ffi_curve(c: &CurveIn) -> ffi::CurveIn {
    match c {
        CurveIn::Segment { a, b } => ffi::CurveIn::Segment(*a, *b),
        CurveIn::BSpline(b) => ffi::CurveIn::BSpline(ffi::CurveData {
            dim: b.dim,
            p: b.p,
            cpts: b
                .cpts
                .iter()
                .map(|c| {
                    [
                        c[0].unwrap_or(f64::NAN),
                        c[1].unwrap_or(f64::NAN),
                        c[2].unwrap_or(f64::NAN),
                        weight(c[3]),
                    ]
                })
                .collect(),
            knots: b.knots.clone(),
        }),
    }
}

fn ffi_surface(s: &SurfaceIn) -> ffi::SurfaceIn {
    match s {
        SurfaceIn::Plane { make } => ffi::SurfaceIn::Plane(Box::new(ffi_make(make))),
        SurfaceIn::BSpline(b) => ffi::SurfaceIn::BSpline(ffi::SurfaceData {
            p: b.p,
            q: b.q,
            net: b
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            [
                                c[0].unwrap_or(f64::NAN),
                                c[1].unwrap_or(f64::NAN),
                                c[2].unwrap_or(f64::NAN),
                                weight(c[3]),
                            ]
                        })
                        .collect()
                })
                .collect(),
            u: b.u.clone(),
            v: b.v.clone(),
        }),
    }
}

fn ffi_op(o: &OpIn) -> ffi::Op {
    match o {
        OpIn::State => ffi::Op::State,
        OpIn::Evaluate { uv, hu, hv, upper } => ffi::Op::Evaluate {
            uv: *uv,
            hu: *hu,
            hv: *hv,
            upper: *upper,
        },
        OpIn::EvalPoint { uv } => ffi::Op::EvalPoint(*uv),
        OpIn::EvalFast { uv } => ffi::Op::EvalFast(*uv),
        OpIn::Normal { uv } => ffi::Op::Normal(*uv),
        OpIn::Project { p } => ffi::Op::Project(*p),
        OpIn::Queries { uv } => ffi::Op::Queries(*uv),
        OpIn::KindOf { t } => ffi::Op::KindOf(*t),
        OpIn::Adjust { dom } => ffi::Op::Adjust(ffi_dom(*dom)),
        OpIn::MakeNurb => ffi::Op::MakeNurb,
        OpIn::Reparam { dom } => ffi::Op::Reparam(ffi_dom(*dom)),
        OpIn::Toggle => ffi::Op::Toggle,
        OpIn::Rebuild => ffi::Op::Rebuild,
        OpIn::Drop {
            dom,
            sop,
            p,
            tol,
            target,
        } => ffi::Op::Drop {
            dom: ffi_dom(*dom),
            op: *sop,
            p: *p,
            tol: *tol,
            target: *target,
        },
        OpIn::Line {
            dom,
            p,
            v,
            ivl,
            ray,
            tol,
        } => ffi::Op::Line {
            dom: ffi_dom(*dom),
            p: *p,
            v: *v,
            ivl: *ivl,
            ray: *ray,
            tol: *tol,
        },
        OpIn::TrimLine { dom, a, b, tol } => ffi::Op::TrimLine {
            dom: ffi_dom(*dom),
            a: *a,
            b: *b,
            tol: *tol,
        },
        OpIn::Iso {
            v_dir,
            iso,
            domain,
            want_uv,
        } => ffi::Op::Iso {
            v_dir: *v_dir,
            iso: *iso,
            domain: *domain,
            want_uv: *want_uv,
        },
        OpIn::DropCurve {
            dom,
            curve,
            ivl,
            tol,
            keep_all,
            creating,
        } => ffi::Op::DropCurve {
            dom: ffi_dom(*dom),
            curve: ffi_curve(curve),
            ivl: *ivl,
            tol: *tol,
            keep_all: *keep_all,
            creating: *creating,
        },
        OpIn::Coincidence { other, tol, whole } => ffi::Op::Coincidence {
            other: ffi_surface(other),
            tol: *tol,
            whole: *whole,
        },
        OpIn::Eq { other } => ffi::Op::Eq(Box::new(ffi_make(other))),
        OpIn::Copy => ffi::Op::Copy,
        OpIn::Grazing { curve, t, line, s } => ffi::Op::Grazing {
            curve: ffi_curve(curve),
            t: *t,
            line: *line,
            s: *s,
        },
        OpIn::Inherited { v } => ffi::Op::Inherited(*v),
        OpIn::Reverse { v_dir } => ffi::Op::Reverse(*v_dir),
        OpIn::Swap => ffi::Op::Swap,
        OpIn::Extend { dir, dist, cont } => ffi::Op::Extend {
            dir: *dir,
            dist: *dist,
            cont: *cont,
        },
    }
}

fn ffi_program(i: &Inputs) -> (ffi::Make, Vec<ffi::Op>) {
    (ffi_make(&i.make), i.ops.iter().map(ffi_op).collect())
}

fn to_output(rec: Vec<f64>) -> Output {
    Output::new().values("rec", rec)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (m, ops) = ffi_program(i);
    match ffi::run(&m, &ops) {
        Ok(rec) => Ok(to_output(rec)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// The solution handle of plane A (`0`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct H(i64);

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
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

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
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

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_uv(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

fn put_dom(o: &mut Vec<f64>, e: &Extent2d) {
    o.extend([e.min().x, e.min().y, e.max().x, e.max().y]);
}

fn put_ivl(o: &mut Vec<f64>, e: &Extent1d) {
    o.extend([e.min(), e.max()]);
}

fn put_chars(o: &mut Vec<f64>, s: &str) {
    o.extend(s.bytes().map(f64::from));
}

fn put_surface_nurb(o: &mut Vec<f64>, s: Option<&NurbSurface>) {
    let Some(sur) = s else {
        o.push(0.0);
        return;
    };
    o.extend([
        1.0,
        sur.n() as f64,
        sur.m() as f64,
        sur.p() as f64,
        sur.q() as f64,
        sur.r() as f64,
        sur.s() as f64,
    ]);
    for row in sur.rows() {
        for c in row {
            o.extend([c.x, c.y, c.z, c.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
}

fn put_state(o: &mut Vec<f64>, a: &Plane) {
    let pos = a.position();
    o.push(a.type_code() as f64);
    put_vec(o, pos.origin());
    put_vec(o, pos.x_axis());
    put_vec(o, pos.y_axis());
    put_vec(o, pos.z_axis());
    put_dom(o, &a.step_uv_domain());
    put_uv(o, a.uv_scale());
    let base = a.base();
    o.extend([
        b(base.is_out_of_bounds_enabled()),
        f64::from(base.knot_type() as i32),
        f64::from(base.form() as i32),
    ]);
    match a.base_with_nurb() {
        Ok(w) => put_surface_nurb(o, w.gw_nurb()),
        Err(_) => o.push(0.0),
    }
}

fn put_curve_nurb(o: &mut Vec<f64>, c: &BSplineCurve) {
    o.extend([f64::from(c.knot_type() as i32), f64::from(c.form() as i32)]);
    match c.gw_nurb() {
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

fn put_curve(o: &mut Vec<f64>, c: &Curve) -> Result<(), Fail> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    match c.kind() {
        CurveKind::Line(l) => {
            put_vec(o, l.line_point());
            put_vec(o, l.line_vector());
            put_ivl(o, &l.step_interval());
            o.extend([l.line_scale(), b(l.inside_out())]);
            put_curve_nurb(o, l.base());
        }
        CurveKind::BSpline(bs) => put_curve_nurb(o, bs),
        _ => return Err(Fail::Harness(harness("unexpected curve kind"))),
    }
    put_ivl(o, &c.natural_interval());
    Ok(())
}

fn put_sols(o: &mut Vec<f64>, sols: &SolutionArray<H>) {
    o.push(sols.len() as f64);
    for s in sols.as_slice() {
        let obj0 = if s.num_objects > 0 {
            s.objects.first().copied().flatten().map_or(-1.0, |h| h.0 as f64)
        } else {
            -1.0
        };
        o.extend([
            f64::from(s.solution_type as i32),
            s.num_variables as f64,
            s.num_objects as f64,
            obj0,
            s.start.solution_value,
        ]);
        for k in 0..3 {
            o.push(if k < s.num_variables {
                s.start.parameters.get(k).copied().unwrap_or(0.0)
            } else {
                0.0
            });
        }
        let range = s.solution_type == SolutionType::RangeOfValues;
        o.push(if range { s.end.solution_value } else { 0.0 });
        for k in 0..3 {
            o.push(if range && k < s.num_variables {
                s.end.parameters.get(k).copied().unwrap_or(0.0)
            } else {
                0.0
            });
        }
    }
}

/// Builds a plane; `Err(status)` when the construction fails.
fn rust_make(m: &MakeIn) -> Result<Result<Plane, f64>, Fail> {
    Ok(match m {
        MakeIn::Bounded {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext(*domain),
        )),
        MakeIn::Normal { o, n } => Ok(Plane::from_normal(v3(*o), v3(*n))),
        MakeIn::Canonical { o, x, y } => {
            let frame = Axis2Placement::new(v3(*o), v3(*x), v3(*y));
            match Plane::create_canonical(&frame) {
                Ok(p) => Ok(p),
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Err(Fail::Deferred(Box::new(d)));
                    }
                    Err(status(&e))
                }
            }
        }
    })
}

fn other_plane(m: &MakeIn) -> Result<Plane, Fail> {
    rust_make(m)?.map_err(|_| Fail::Harness(harness("other plane construction failed")))
}

fn dom_of(a: &Plane, d: DomIn) -> Extent2d {
    match d {
        DomIn::Step => a.step_uv_domain(),
        DomIn::Natural => a.natural_uv_domain(),
        DomIn::Explicit(v) => ext(v),
    }
}

fn rust_curve(c: &CurveIn) -> Result<Curve, Fail> {
    match c {
        CurveIn::Segment { a, b } => Line::create_line_segment(3, v3(*a), v3(*b), None)
            .map(Curve::from)
            .map_err(|e| Fail::Harness(harness(format!("segment: {e}")))),
        CurveIn::BSpline(d) => {
            let pw: Vec<CPoint> = d
                .cpts
                .iter()
                .map(|q| CPoint {
                    x: q[0].unwrap_or(f64::NAN),
                    y: q[1].unwrap_or(f64::NAN),
                    z: q[2].unwrap_or(f64::NAN),
                    w: weight(q[3]),
                })
                .collect();
            let n = pw.len().checked_sub(1).ok_or_else(|| harness("empty B-spline"))?;
            let m = d.knots.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            BSplineCurve::from_data(u64::from(d.dim), n, m, d.p, &pw, &d.knots)
                .map(Curve::from)
                .map_err(|e| Fail::Harness(harness(format!("B-spline: {e}"))))
        }
    }
}

fn rust_surface(s: &SurfaceIn) -> Result<Surface, Fail> {
    match s {
        SurfaceIn::Plane { make } => Ok(Surface::from(other_plane(make)?)),
        SurfaceIn::BSpline(d) => {
            let net = d
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            CPoint::new(
                                c[0].unwrap_or(f64::NAN),
                                c[1].unwrap_or(f64::NAN),
                                c[2].unwrap_or(f64::NAN),
                                weight(c[3]),
                            )
                        })
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, d.p, d.q, d.u.clone(), d.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
    }
}

fn sop(code: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(code)).ok_or_else(|| harness("solver operation"))
}

fn dir(v_dir: bool) -> SurfParamType {
    if v_dir {
        SurfParamType::V
    } else {
        SurfParamType::U
    }
}

fn rust_op(k: &Kernel, op: &OpIn, a: &mut Plane, o: &mut Vec<f64>) -> Result<(), Fail> {
    match op {
        OpIn::State => {
            o.push(0.0);
            put_state(o, a);
        }
        OpIn::Evaluate { uv, hu, hv, upper } => {
            if let Some(d) = st(o, a.evaluate(p2(*uv), *hu, *hv, *upper))? {
                for row in &d {
                    for v in row {
                        put_vec(o, *v);
                    }
                }
            }
        }
        OpIn::EvalPoint { uv } => {
            o.push(0.0);
            put_vec(o, a.evaluate_point(p2(*uv)));
        }
        OpIn::EvalFast { uv } => {
            o.push(0.0);
            put_vec(o, a.evaluate_point_fast(p2(*uv)));
        }
        OpIn::Normal { .. } => {
            o.push(0.0);
            put_vec(o, a.evaluate_normal());
        }
        OpIn::Project { p } => {
            o.push(0.0);
            put_uv(o, a.project_point_to_uv_domain(v3(*p)));
        }
        OpIn::Queries { .. } => {
            o.push(0.0);
            o.push(b(a.is_bounded()));
            let (pp, pn, dev) = a.is_planar();
            o.push(1.0);
            put_vec(o, pp);
            put_vec(o, pn);
            o.push(dev);
            let (seam, sd) = a.is_on_seam();
            o.extend([b(seam), f64::from(sd as i32), 0.0]);
            let (closed, ct) = a.is_closed();
            o.extend([b(closed), f64::from(ct as i32)]);
            let (s1, d1) = a.is_singularity();
            o.extend([b(s1), f64::from(d1 as i32)]);
            let (s2, d2) = a.is_singularity_with_poles();
            o.extend([b(s2), f64::from(d2 as i32)]);
            o.push(a.type_code() as f64);
            put_chars(o, a.type_string());
            o.push(-1.0);
            put_chars(o, a.class_string());
        }
        OpIn::KindOf { t } => {
            if let Some(x) = st(o, a.is_kind_of(i64::from(*t)))? {
                o.push(b(x));
            }
        }
        OpIn::Adjust { dom } => {
            let d = dom_of(a, *dom);
            st(o, a.adjust_step_uv_domain(&d))?;
        }
        OpIn::MakeNurb => {
            st(o, a.make_nurb())?;
        }
        OpIn::Reparam { dom } => {
            let d = dom_of(a, *dom);
            st(o, a.reparameterize(&d))?;
        }
        OpIn::Toggle => {
            a.toggle_swap_uv_bit();
            o.push(0.0);
        }
        OpIn::Rebuild => {
            st(o, a.rebuild_step_from_nurb_parameters())?;
        }
        OpIn::Drop {
            dom,
            sop: code,
            p,
            tol,
            target,
        } => {
            let d = dom_of(a, *dom);
            let r = a.drop_point_fast(&d, sop(*code)?, v3(*p), *tol, *target, Some(H(0)));
            if let Some(sols) = st(o, r)? {
                put_sols(o, &sols);
            }
        }
        OpIn::Line {
            dom,
            p,
            v,
            ivl,
            ray,
            tol,
        } => {
            let d = dom_of(a, *dom);
            let i = ivl.map(|[x, y]| Extent1d::from_min_max(x, y));
            let r = a.global_line_intersect::<H>(
                &Kernel::default(),
                H(0),
                &d,
                v3(*p),
                v3(*v),
                i.as_ref(),
                *ray,
                *tol,
            );
            if let Some(sols) = st(o, r)? {
                put_sols(o, &sols);
            }
        }
        OpIn::TrimLine { dom, a: p0, b: p1, tol } => {
            let d = dom_of(a, *dom);
            o.push(0.0);
            match a.trim_plane_line_to_plane_domain(&d, v3(*p0), v3(*p1), *tol) {
                Some((t0, t1)) => {
                    o.push(1.0);
                    put_vec(o, t0);
                    put_vec(o, t1);
                }
                None => o.push(0.0),
            }
        }
        OpIn::Iso {
            v_dir,
            iso,
            domain,
            want_uv,
        } => {
            let d = domain.map(ext);
            let r = a.create_iso_parametric_curve(dir(*v_dir), *iso, d.as_ref(), *want_uv);
            if let Some(c) = st(o, r)? {
                put_curve(o, &Curve::from(c.curve))?;
                o.push(c.max_gap_3d);
                match &c.uv_iso_curve {
                    Some(uv) => {
                        o.push(1.0);
                        put_curve(o, uv)?;
                    }
                    None => o.push(0.0),
                }
            }
        }
        OpIn::DropCurve {
            dom,
            curve,
            ivl,
            tol,
            keep_all,
            creating,
        } => {
            let d = dom_of(a, *dom);
            let c = rust_curve(curve)?;
            let i = ivl.map_or_else(|| c.natural_interval(), |[x, y]| Extent1d::from_min_max(x, y));
            let r = a.drop_curve(k, &d, &c, &i, *tol, *keep_all, *creating, None);
            if let Some(res) = st(o, r)? {
                o.extend([
                    res.max_drop_to_surf,
                    res.max_approx_dev,
                    res.uv_curves.len() as f64,
                ]);
                for c in &res.uv_curves {
                    put_curve(o, c)?;
                }
            }
        }
        OpIn::Coincidence { other, tol, whole } => {
            let s = rust_surface(other)?;
            if let Some(c) = st(o, a.coincidence_check(k, &s, *tol, *whole))? {
                o.extend([b(c.are_coincident), c.max_distance]);
            }
        }
        OpIn::Eq { other } => {
            let q = other_plane(other)?;
            o.push(0.0);
            o.push(b(a.approx_eq(&q)));
        }
        OpIn::Copy => {
            o.push(0.0);
            put_state(o, &a.copy());
        }
        OpIn::Grazing { curve, t, line, s } => {
            let c = rust_curve(curve)?;
            let l = Line::create_line_segment(3, v3(line[0]), v3(line[1]), None)
                .map_err(|e| harness(format!("segment: {e}")))?;
            if let Some(g) = st(o, is_grazing_intersection(k, &c, *t, &l, *s))? {
                o.push(b(g));
            }
        }
        OpIn::Inherited { v } => {
            let s = Surface::from(a.clone());
            o.push(0.0);
            put_dom(o, &s.natural_uv_domain());
            let max = s
                .max_analytic_domain(k)
                .map_err(|e| harness(format!("max analytic domain: {e}")))?;
            put_dom(o, &max);
            o.push(b(s.is_bilinear(k)));
            let par = s
                .is_parallel_to_vector(k, v3(*v))
                .map_err(|e| harness(format!("parallel: {e}")))?;
            o.push(b(par));
            o.extend([
                s.degree(SurfParamType::U) as f64,
                s.degree(SurfParamType::V) as f64,
                s.number_control_points(k, SurfParamType::U) as f64,
                s.number_control_points(k, SurfParamType::V) as f64,
            ]);
        }
        OpIn::Reverse { v_dir } => {
            st(o, a.reverse(dir(*v_dir)))?;
        }
        OpIn::Swap => {
            st(o, a.swap_uv())?;
        }
        OpIn::Extend { dir, dist, cont } => {
            let cont = continuity_of(*cont)?;
            let r = match dir {
                Some(d) => {
                    let d = SurfParamType::from_code(u64::from(*d))
                        .ok_or_else(|| harness(format!("bad direction {d}")))?;
                    a.create_extended_surface_dir(d, *dist, cont)
                }
                None => a.create_extended_surface(*dist, cont),
            };
            if let Some(e) = st(o, r)? {
                put_surface_nurb(o, e.gw_nurb());
            }
        }
    }
    Ok(())
}

fn continuity_of(v: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match v {
        0 => C::Undefined,
        1 => C::Discontinuous,
        2 => C::C0,
        3 => C::G1,
        4 => C::G1R,
        5 => C::G1G2,
        6 => C::G1G2G3,
        7 => C::C1,
        8 => C::C1G2,
        9 => C::C1G2G3,
        10 => C::C1C2,
        11 => C::C1C2G3,
        12 => C::C1C2C3,
        13 => C::CInfinity,
        _ => return Err(harness(format!("bad continuity {v}"))),
    })
}

fn op_code(op: &OpIn) -> Result<f64, HarnessError> {
    let prog = ffi::encode(
        &ffi::Make::Normal {
            o: [0.0; 3],
            n: [0.0, 0.0, 1.0],
        },
        &[ffi_op(op)],
    )
    .map_err(|e| harness(e.to_string()))?;
    // `1 o n` (7 values), `1` (op count), then the op code
    prog.get(8).copied().ok_or_else(|| harness("empty op"))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let made = match rust_make(&i.make) {
        Ok(m) => m,
        Err(Fail::Deferred(d)) => return Ok(*d),
        Err(Fail::Harness(e)) => return Err(e),
    };
    let mut a = match made {
        Ok(p) => {
            o.push(0.0);
            p
        }
        Err(s) => {
            o.push(s);
            return Ok(to_output(o));
        }
    };
    for op in &i.ops {
        o.push(op_code(op)?);
        match rust_op(&k, op, &mut a, &mut o) {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => return Ok(*d),
            Err(Fail::Harness(e)) => return Err(e),
        }
    }
    put_state(&mut o, &a);
    Ok(to_output(o))
}

// ---- generated cases --------------------------------------------------------------

/// One of `xs`, uniformly.
fn pick<T: Copy + Default>(rng: &mut Rng, xs: &[T]) -> T {
    let i = rng.below(xs.len() as u64) as usize;
    xs.get(i).copied().unwrap_or_default()
}

fn coord(rng: &mut Rng) -> f64 {
    // coordinates on a coarse grid sometimes, so coincidences happen
    if rng.chance(0.3) {
        rng.range_i64(-4, 4) as f64
    } else {
        rng.uniform(-10.0, 10.0)
    }
}

fn pt(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    if rng.chance(0.3) {
        // axis aligned
        let mut d = [0.0; 3];
        let i = rng.below(3) as usize;
        if let Some(c) = d.get_mut(i) {
            *c = if rng.chance(0.5) { 1.0 } else { -1.0 };
        }
        return d;
    }
    loop {
        let d = rng.point3(-1.0, 1.0);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l > 0.2 {
            return [d[0] / l, d[1] / l, d[2] / l];
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// An orthonormal pair (sometimes perturbed, sometimes not unit length).
fn frame(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    let x = unit_dir(rng);
    let mut t = unit_dir(rng);
    while cross(x, t).iter().map(|c| c * c).sum::<f64>() < 0.05 {
        t = unit_dir(rng);
    }
    let z = norm(cross(x, t));
    let y = cross(z, x);
    let mut x = x;
    let mut y = y;
    if rng.chance(0.15) {
        // not unit length
        let s = rng.uniform(0.5, 3.0);
        x = [x[0] * s, x[1] * s, x[2] * s];
    }
    if rng.chance(0.1) {
        // not perpendicular
        let e = rng.uniform(-0.3, 0.3);
        y = [y[0] + e * x[0], y[1] + e * x[1], y[2] + e * x[2]];
    }
    (x, y)
}

fn gen_domain(rng: &mut Rng) -> [f64; 4] {
    let u0 = if rng.chance(0.4) { 0.0 } else { rng.uniform(-5.0, 5.0) };
    let v0 = if rng.chance(0.4) { 0.0 } else { rng.uniform(-5.0, 5.0) };
    let du = if rng.chance(0.3) { 1.0 } else { rng.uniform(0.1, 8.0) };
    let dv = if rng.chance(0.3) { 1.0 } else { rng.uniform(0.1, 8.0) };
    [u0, v0, u0 + du, v0 + dv]
}

fn gen_make(rng: &mut Rng) -> MakeIn {
    let r = rng.below(10);
    if r < 7 {
        let (x, y) = frame(rng);
        let scale = if rng.chance(0.4) {
            [1.0, 1.0]
        } else {
            [rng.uniform(0.2, 5.0), rng.uniform(0.2, 5.0)]
        };
        MakeIn::Bounded {
            o: pt(rng),
            x,
            y,
            scale,
            domain: gen_domain(rng),
        }
    } else if r < 9 {
        let n = unit_dir(rng);
        let s = rng.uniform(0.3, 4.0);
        MakeIn::Normal {
            o: pt(rng),
            n: [n[0] * s, n[1] * s, n[2] * s],
        }
    } else {
        let (x, y) = frame(rng);
        MakeIn::Canonical { o: pt(rng), x, y }
    }
}

/// Approximate geometry of a plane block for building nearby inputs.
struct Geo {
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    s: [f64; 2],
    d: [f64; 4],
}

impl Geo {
    fn of(m: &MakeIn) -> Self {
        match m {
            MakeIn::Bounded {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                let xn = norm(*x);
                let z = norm(cross(xn, *y));
                Self {
                    o: *o,
                    x: xn,
                    y: cross(z, xn),
                    s: *scale,
                    d: *domain,
                }
            }
            MakeIn::Normal { o, n } => {
                let z = norm(*n);
                let t = if z[0].abs() < 0.6 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                let x = norm(cross(t, z));
                Self {
                    o: *o,
                    x,
                    y: cross(z, x),
                    s: [1.0, 1.0],
                    d: [-6.0, -6.0, 6.0, 6.0],
                }
            }
            MakeIn::Canonical { o, x, y } => {
                let xn = norm(*x);
                let z = norm(cross(xn, *y));
                Self {
                    o: *o,
                    x: xn,
                    y: cross(z, xn),
                    s: [1.0, 1.0],
                    d: [-6.0, -6.0, 6.0, 6.0],
                }
            }
        }
    }

    fn z(&self) -> [f64; 3] {
        norm(cross(self.x, self.y))
    }

    /// The point at relative `(a, b)` of the domain, `h` off the plane.
    fn at(&self, a: f64, b: f64, h: f64) -> [f64; 3] {
        let u = self.d[0] + a * (self.d[2] - self.d[0]);
        let v = self.d[1] + b * (self.d[3] - self.d[1]);
        let z = self.z();
        let mut p = [0.0; 3];
        for (i, c) in p.iter_mut().enumerate() {
            let g = |w: &[f64; 3]| w.get(i).copied().unwrap_or(0.0);
            *c = g(&self.o) + u * self.s[0] * g(&self.x) + v * self.s[1] * g(&self.y) + h * g(&z);
        }
        p
    }

    fn uv(&self, rng: &mut Rng) -> [f64; 2] {
        let a = rng.uniform(-0.3, 1.3);
        let b = rng.uniform(-0.3, 1.3);
        [
            self.d[0] + a * (self.d[2] - self.d[0]),
            self.d[1] + b * (self.d[3] - self.d[1]),
        ]
    }
}

fn gen_rel(rng: &mut Rng) -> f64 {
    if rng.chance(0.2) {
        pick(rng, &[0.0, 1.0, 0.5])
    } else {
        rng.uniform(-0.4, 1.4)
    }
}

fn gen_dom(rng: &mut Rng, g: &Geo) -> DomIn {
    match rng.below(4) {
        0 | 1 => DomIn::Step,
        2 => DomIn::Natural,
        _ => {
            let a = rng.uniform(-0.2, 0.6);
            let c = rng.uniform(-0.2, 0.6);
            let w = g.d[2] - g.d[0];
            let h = g.d[3] - g.d[1];
            DomIn::Explicit([
                g.d[0] + a * w,
                g.d[1] + c * h,
                g.d[0] + (a + rng.uniform(0.2, 0.8)) * w,
                g.d[1] + (c + rng.uniform(0.2, 0.8)) * h,
            ])
        }
    }
}

fn gen_bspline_curve(rng: &mut Rng, g: &Geo, height: f64) -> BSplineCurveIn {
    let p = 1 + rng.below(3) as usize;
    let n1 = p + 1 + rng.below(3) as usize;
    let rational = rng.chance(0.25);
    let cpts = (0..n1)
        .map(|_| {
            let q = g.at(rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9), rng.uniform(-height, height));
            let w = if rational { Some(rng.uniform(0.5, 2.0)) } else { None };
            [Some(q[0]), Some(q[1]), Some(q[2]), w]
        })
        .collect();
    let interior = n1 - p - 1;
    let mut knots = vec![0.0; p + 1];
    for i in 0..interior {
        knots.push((i + 1) as f64 / (interior + 1) as f64);
    }
    knots.extend(vec![1.0; p + 1]);
    BSplineCurveIn {
        dim: 3,
        p,
        cpts,
        knots,
    }
}

fn gen_curve(rng: &mut Rng, g: &Geo) -> CurveIn {
    if rng.chance(0.5) {
        let h = if rng.chance(0.5) { 0.0 } else { rng.uniform(-1.0, 1.0) };
        let a = g.at(gen_rel(rng), gen_rel(rng), h);
        let mut b = g.at(gen_rel(rng), gen_rel(rng), if rng.chance(0.5) { h } else { rng.uniform(-1.0, 1.0) });
        if a == b {
            b[0] += 1.0;
        }
        CurveIn::Segment { a, b }
    } else {
        let h = if rng.chance(0.4) { 0.0 } else { rng.uniform(0.0, 0.5) };
        CurveIn::BSpline(gen_bspline_curve(rng, g, h))
    }
}

fn gen_other(rng: &mut Rng, m: &MakeIn, g: &Geo) -> SurfaceIn {
    if rng.chance(0.15) {
        // a bilinear patch near the plane (not an SmPlane)
        let net = (0..2)
            .map(|i| {
                (0..2)
                    .map(|j| {
                        let q = g.at(f64::from(i), f64::from(j), 0.0);
                        [Some(q[0]), Some(q[1]), Some(q[2]), None]
                    })
                    .collect()
            })
            .collect();
        return SurfaceIn::BSpline(BSplineSurfaceIn {
            p: 1,
            q: 1,
            net,
            u: vec![0.0, 0.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
        });
    }
    let make = match rng.below(5) {
        0 => m.clone(),
        1 | 2 => {
            // coplanar, shifted / scaled domain
            let (o, x, y) = (g.o, g.x, g.y);
            let mut d = g.d;
            let su = (d[2] - d[0]) * rng.uniform(-1.2, 1.2);
            let sv = (d[3] - d[1]) * rng.uniform(-1.2, 1.2);
            d = [d[0] + su, d[1] + sv, d[2] + su * rng.uniform(0.5, 1.5), d[3] + sv];
            if d[2] <= d[0] {
                d[2] = d[0] + 1.0;
            }
            let (x, y) = if rng.chance(0.3) { (y, [-x[0], -x[1], -x[2]]) } else { (x, y) };
            MakeIn::Bounded {
                o,
                x,
                y,
                scale: g.s,
                domain: d,
            }
        }
        3 => {
            // offset or tilted
            let z = g.z();
            let h = if rng.chance(0.5) { 1e-8 } else { rng.uniform(-1.0, 1.0) };
            let o = [g.o[0] + h * z[0], g.o[1] + h * z[1], g.o[2] + h * z[2]];
            let tilt = if rng.chance(0.5) { 0.0 } else { rng.uniform(-0.2, 0.2) };
            let x = norm([g.x[0] + tilt * z[0], g.x[1] + tilt * z[1], g.x[2] + tilt * z[2]]);
            MakeIn::Bounded {
                o,
                x,
                y: g.y,
                scale: g.s,
                domain: g.d,
            }
        }
        _ => gen_make(rng),
    };
    SurfaceIn::Plane { make }
}

fn gen_op(rng: &mut Rng, m: &MakeIn, g: &Geo, nurb_safe: bool) -> OpIn {
    let r = rng.below(27);
    match r {
        0 => OpIn::State,
        1 | 2 => {
            let hu = rng.below(4) as usize;
            let hv = rng.below(4) as usize;
            let upper = hu <= hv && rng.chance(0.3);
            OpIn::Evaluate {
                uv: g.uv(rng),
                hu,
                hv,
                upper,
            }
        }
        3 => OpIn::EvalPoint { uv: g.uv(rng) },
        4 => OpIn::EvalFast { uv: g.uv(rng) },
        5 => OpIn::Normal { uv: g.uv(rng) },
        6 => OpIn::Project {
            p: g.at(gen_rel(rng), gen_rel(rng), rng.uniform(-3.0, 3.0)),
        },
        7 => OpIn::Queries { uv: g.uv(rng) },
        8 => OpIn::KindOf {
            t: pick(rng, &[15011, 15002, 15001, 15013, 1, 0, 3]),
        },
        9 => OpIn::Adjust { dom: gen_dom(rng, g) },
        11 => OpIn::Reparam {
            dom: DomIn::Explicit(gen_domain(rng)),
        },
        12 if nurb_safe => OpIn::Toggle,
        13 => OpIn::Rebuild,
        14 | 15 => {
            let sop = pick(rng, &[3u8, 5, 0, 4, 1]);
            let h = if rng.chance(0.3) { 0.0 } else { rng.uniform(-2.0, 2.0) };
            OpIn::Drop {
                dom: gen_dom(rng, g),
                sop,
                p: g.at(gen_rel(rng), gen_rel(rng), h),
                tol: pick(rng, &[1e-6, 1e-3, 0.5, 3.0]),
                target: rng.chance(0.3).then(|| rng.uniform(0.0, 3.0)),
            }
        }
        16 | 17 => {
            let p = g.at(gen_rel(rng), gen_rel(rng), if rng.chance(0.3) { 0.0 } else { rng.uniform(-3.0, 3.0) });
            let v = if rng.chance(0.3) {
                // in the plane
                let a = rng.uniform(-1.0, 1.0);
                let c = rng.uniform(-1.0, 1.0);
                [
                    a * g.x[0] + c * g.y[0],
                    a * g.x[1] + c * g.y[1],
                    a * g.x[2] + c * g.y[2],
                ]
            } else {
                unit_dir(rng)
            };
            let v = if v.iter().all(|c| c.abs() < 1e-3) { g.z() } else { v };
            OpIn::Line {
                dom: gen_dom(rng, g),
                p,
                v,
                ivl: rng.chance(0.4).then(|| {
                    let a = rng.uniform(-5.0, 2.0);
                    [a, a + rng.uniform(0.5, 8.0)]
                }),
                ray: rng.chance(0.25),
                tol: pick(rng, &[1e-6, 1e-3, 0.5]),
            }
        }
        18 => {
            let h = if rng.chance(0.5) { 0.0 } else { rng.uniform(-1.0, 1.0) };
            OpIn::TrimLine {
                dom: gen_dom(rng, g),
                a: g.at(gen_rel(rng), gen_rel(rng), h),
                b: g.at(gen_rel(rng), gen_rel(rng), h),
                tol: pick(rng, &[0.0, 1e-6, 0.1]),
            }
        }
        19 => {
            let v_dir = rng.chance(0.5);
            let (lo, hi) = if v_dir { (g.d[1], g.d[3]) } else { (g.d[0], g.d[2]) };
            OpIn::Iso {
                v_dir,
                iso: lo + rng.uniform(-0.2, 1.2) * (hi - lo),
                domain: rng.chance(0.3).then(|| gen_domain(rng)),
                want_uv: rng.chance(0.5),
            }
        }
        20 | 21 => {
            let curve = gen_curve(rng, g);
            OpIn::DropCurve {
                dom: gen_dom(rng, g),
                curve,
                ivl: rng.chance(0.3).then(|| {
                    let a = rng.uniform(0.0, 0.4);
                    [a, a + rng.uniform(0.2, 0.6)]
                }),
                tol: pick(rng, &[1e-6, 1e-3, 0.3, 2.0]),
                keep_all: rng.chance(0.5),
                creating: rng.chance(0.3),
            }
        }
        22 | 23 => OpIn::Coincidence {
            other: gen_other(rng, m, g),
            tol: pick(rng, &[1e-6, 1e-3, 0.1]),
            whole: rng.chance(0.4),
        },
        24 if nurb_safe => OpIn::Eq {
            other: if rng.chance(0.5) { m.clone() } else { gen_make(rng) },
        },
        10 | 12 | 24 => OpIn::MakeNurb,
        25 => {
            if rng.chance(0.5) {
                OpIn::Copy
            } else {
                OpIn::Inherited {
                    v: if rng.chance(0.5) { g.x } else { unit_dir(rng) },
                }
            }
        }
        _ => {
            if rng.chance(0.15) {
                // a quadratic tangent to the line at its start: a graze
                let p0 = g.at(gen_rel(rng), gen_rel(rng), 0.0);
                let p1 = g.at(gen_rel(rng), gen_rel(rng), 0.0);
                let p2 = g.at(gen_rel(rng), gen_rel(rng), rng.uniform(-1.0, 1.0));
                let mut l1 = [2.0 * p1[0] - p0[0], 2.0 * p1[1] - p0[1], 2.0 * p1[2] - p0[2]];
                if l1 == p0 {
                    l1[0] += 1.0;
                }
                let cp = |q: [f64; 3]| [Some(q[0]), Some(q[1]), Some(q[2]), None];
                OpIn::Grazing {
                    curve: CurveIn::BSpline(BSplineCurveIn {
                        dim: 3,
                        p: 2,
                        cpts: vec![cp(p0), cp(p1), cp(p2)],
                        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    }),
                    t: 0.0,
                    line: [p0, l1],
                    s: 0.0,
                }
            } else if rng.chance(0.4) {
                let c = gen_curve(rng, g);
                let l0 = g.at(gen_rel(rng), gen_rel(rng), 0.0);
                let mut l1 = g.at(gen_rel(rng), gen_rel(rng), 0.0);
                if l0 == l1 {
                    l1[0] += 1.0;
                }
                OpIn::Grazing {
                    curve: c,
                    t: rng.uniform(0.0, 1.0),
                    line: [l0, l1],
                    s: rng.uniform(0.0, 1.0),
                }
            } else if rng.chance(0.25) {
                OpIn::Extend {
                    dir: rng
                        .chance(0.6)
                        .then(|| pick(rng, &[0u8, 1, 2, 3, 4, 5, 6, 7, 8])),
                    dist: pick(rng, &[0.01, 0.1, 0.5, 2.0]),
                    cont: pick(rng, &[2u8, 3, 4, 5, 13]),
                }
            } else if rng.chance(0.5) {
                OpIn::Reverse {
                    v_dir: rng.chance(0.5),
                }
            } else {
                OpIn::Swap
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let make = gen_make(rng);
    let g = Geo::of(&make);
    let n = 1 + rng.below(5) as usize;
    // A NURB upstream makes lazily in a const method (or a copy) stays with the plane;
    // the port makes it on a copy (also in the general DropCurve a drop may fall back on).
    // ToggleSwapUVBit (which leaves the NURB alone) and
    // operator== (which compares NURBs) observe the difference, so they come only
    // while both sides agree: before any such method or once the NURB is made in place.
    let mut made = matches!(make, MakeIn::Canonical { .. });
    let mut lazy = false;
    let mut ops = Vec::with_capacity(n);
    for _ in 0..n {
        let op = gen_op(rng, &make, &g, made || !lazy);
        match &op {
            OpIn::MakeNurb
            | OpIn::Reparam { .. }
            | OpIn::Rebuild
            | OpIn::Reverse { .. }
            | OpIn::Swap => made = true,
            // (DropCurve: the general SmSurface::DropCurve asks GetNaturalUVDomain)
            OpIn::Coincidence { .. } | OpIn::Iso { .. } | OpIn::Copy | OpIn::DropCurve { .. } => {
                lazy = true;
            }
            OpIn::Drop { sop, .. } if *sop == 5 => lazy = true,
            _ => {}
        }
        ops.push(op);
    }
    Inputs { make, ops }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.plane_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let (m, ops) = ffi_program(&i);
            if ffi::encode(&m, &ops).is_ok() {
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
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

