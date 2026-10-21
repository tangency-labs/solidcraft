//! Runner for unit `intersect.point_class`: `SmPointClassification`, the
//! point-classification overloads of `SmTol` and the `SmSpaceBend` / `SmSpaceUnbend`
//! class type queries.
//!
//! C++: `solidcore_ffi::units::intersect_point_class` (upstream
//! `source/SMLib/src/SmPointClass.cpp`). Rust: `solidcore::intersect::point_class`.
//!
//! A case is a script: a pool of `objects` (curves as in unit `curve.curve_p6`, planes,
//! points, a volume, region, edgeuse and "other" stand-in, and vertex, edge and face
//! objects), the source tolerances of two classifications `pc[0]`, `pc[1]`, and `ops`
//! on them. Both sides produce the flat record documented in the ffi module (per op:
//! code, status, outputs, state of the target), reported as the `out` values. A Rust
//! error maps to its `SmStatus` (`-1` for one upstream cannot return); an `Unported`
//! error anywhere defers the case.
//!
//! Deferred cases: every op that reaches topology (`SmVertex`, `SmEdge`, `SmFace`, ...
//! members: `IsConnectedTo`, `GetBrep`, `GetTolerance`, `GetPoint` / `SetPoint`,
//! `MergeIntoObject` on a vertex), the surface and volume point solves of
//! `ComputePointParameters` and `GetGap3d` (units `surface.surface_p4`,
//! `surface.volume_p2`), and stand-in regions and edgeuses whose
//! tolerance upstream takes from the system (the port asks their topology units).
//! Scripts never call a member of a stand-in object or of an `SmPoint3d` cast to
//! `SmObject` (undefined behaviour upstream): both sides reject those ops, and the
//! generator drops such scripts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::SolutionEnd;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::point_class::{
    space_bend_type, space_unbend_type, ClassGeometry, ClassObject, ClassObjectKind,
    PointClassType, PointClassification, Precedence, SPACE_BEND_TYPE_STRING,
    SPACE_UNBEND_TYPE_STRING,
};
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_point_class::{self as ffi, kind, op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `intersect.point_class` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SM_USE_DEFAULT`.
const USE_DEFAULT: f64 = 9876543.21;
/// `SM_BIG_DOUBLE`.
const BIG: f64 = 1.0e20;

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
    /// A real `SmEllipse` / `SmCircle` (`ry = None`), angles in degrees.
    Conic(ConicIn),
}

/// An `SmEllipse(C, X, Y, [d0, d1], rx, ry, dim, ctx, NULL, io)` or, without `ry`, an
/// `SmCircle(C, X, Y, [d0, d1], rx, dim, ctx, NULL, io)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConicIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    rx: f64,
    ry: Option<f64>,
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ObjIn {
    Curve(CurveIn),
    Plane { o: [f64; 3], n: [f64; 3] },
    Volume,
    Point([f64; 3]),
    Vertex,
    Edge(Option<usize>),
    Face(Option<usize>),
    Region,
    Edgeuse,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Reset {
        t: usize,
        tol: Option<f64>,
    },
    SetClassObject {
        t: usize,
        class: i32,
        obj: Option<usize>,
    },
    SetT {
        t: usize,
        v: f64,
        snap: bool,
    },
    SetUv {
        t: usize,
        u: f64,
        v: f64,
    },
    SetUvw {
        t: usize,
        p: [f64; 3],
    },
    Unset {
        t: usize,
    },
    SetGap {
        t: usize,
        g: f64,
    },
    SetDeviation {
        t: usize,
        g: f64,
    },
    SetPresnap {
        t: usize,
        v: f64,
    },
    SetSss {
        t: usize,
        p: [f64; 3],
    },
    SetSrcTol {
        t: usize,
        tol: f64,
    },
    SetFace {
        t: usize,
        obj: Option<usize>,
    },
    Assign {
        t: usize,
    },
    CopyClassOnly {
        t: usize,
    },
    CopySss {
        t: usize,
    },
    Combine {
        t: usize,
    },
    Compute {
        t: usize,
        p: [f64; 3],
    },
    Find {
        t: usize,
    },
    Gap {
        t: usize,
        s: Option<usize>,
    },
    Load {
        t: usize,
        tol: f64,
        obj: usize,
        value: f64,
        /// Up to 10 solution parameters (missing ones are 0).
        params: Vec<f64>,
        offset: usize,
        pre: f64,
    },
    Merge {
        t: usize,
        p: [f64; 3],
    },
    Brep {
        t: usize,
    },
    Connected {
        t: usize,
        j: usize,
    },
    ConnectedAny {
        t: usize,
        js: Vec<usize>,
    },
    CurveObject {
        t: usize,
    },
    ObjTol {
        t: usize,
    },
    XsectTol {
        t: usize,
    },
    SrcTol {
        t: usize,
    },
    BendTypes {
        t: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Script {
    objects: Vec<ObjIn>,
    tol: [f64; 2],
    ops: Vec<OpIn>,
}

// ---- curves -----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

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
        solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic,
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
        CurveIn::Conic(c) => {
            v.extend([if c.ry.is_some() { 3.0 } else { 4.0 }, 3.0]);
            v.extend_from_slice(&c.center);
            v.extend_from_slice(&c.x);
            v.extend_from_slice(&c.y);
            v.extend(c.domain);
            v.push(c.rx);
            if let Some(ry) = c.ry {
                v.push(ry);
            }
            v.push(b(c.inside_out));
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            v.extend_from_slice(&l.p);
            v.extend_from_slice(&l.v);
            v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
        }
        CurveIn::Circle(_) => return Err(harness("unresolved circle")),
    }
    Ok(())
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

fn build_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m =
                bs.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("no knots"))?;
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
        CurveIn::Conic(c) => {
            let k = Kernel::default();
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: c.inside_out,
            };
            let domain = Extent1d::from_min_max(c.domain[0], c.domain[1]);
            let e = match c.ry {
                Some(ry) => {
                    Ellipse::new(&k, v3(c.center), v3(c.x), v3(c.y), domain, c.rx, ry, &opts)
                }
                None => {
                    Ellipse::new_circle(&k, v3(c.center), v3(c.x), v3(c.y), domain, c.rx, &opts)
                }
            }
            .map_err(|e| harness(format!("conic: {e}")))?;
            Ok(Curve::from(e))
        }
        CurveIn::Circle(_) => Err(harness("unresolved circle")),
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn opt_id(o: Option<usize>) -> f64 {
    o.map_or(-1.0, |i| i as f64)
}

fn cpp_input(s: &Script) -> Result<Vec<f64>, HarnessError> {
    let mut v = vec![s.objects.len() as f64];
    for o in &s.objects {
        match o {
            ObjIn::Curve(c) => {
                v.push(kind::CURVE as f64);
                curve_block(&mut v, c)?;
            }
            ObjIn::Plane { o, n } => {
                v.push(kind::PLANE as f64);
                v.extend_from_slice(o);
                v.extend_from_slice(n);
            }
            ObjIn::Volume => v.push(kind::VOLUME as f64),
            ObjIn::Point(p) => {
                v.push(kind::POINT as f64);
                v.extend_from_slice(p);
            }
            ObjIn::Vertex => v.push(kind::VERTEX as f64),
            ObjIn::Edge(c) => v.extend([kind::EDGE as f64, opt_id(*c)]),
            ObjIn::Face(s) => v.extend([kind::FACE as f64, opt_id(*s)]),
            ObjIn::Region => v.push(kind::REGION as f64),
            ObjIn::Edgeuse => v.push(kind::EDGEUSE as f64),
            ObjIn::Other => v.push(kind::OTHER as f64),
        }
    }
    v.extend(s.tol);
    v.push(s.ops.len() as f64);
    for o in &s.ops {
        let (code, t) = op_code(o);
        v.extend([code as f64, t as f64]);
        match o {
            OpIn::Reset { tol, .. } => v.extend([b(tol.is_some()), tol.unwrap_or(0.0)]),
            OpIn::SetClassObject { class, obj, .. } => v.extend([f64::from(*class), opt_id(*obj)]),
            OpIn::SetT { v: x, snap, .. } => v.extend([*x, b(*snap)]),
            OpIn::SetUv { u, v: x, .. } => v.extend([*u, *x]),
            OpIn::SetUvw { p, .. }
            | OpIn::SetSss { p, .. }
            | OpIn::Compute { p, .. }
            | OpIn::Merge { p, .. } => v.extend_from_slice(p),
            OpIn::SetGap { g, .. } | OpIn::SetDeviation { g, .. } => v.push(*g),
            OpIn::SetPresnap { v: x, .. } => v.push(*x),
            OpIn::SetSrcTol { tol, .. } => v.push(*tol),
            OpIn::SetFace { obj, .. } => v.push(opt_id(*obj)),
            OpIn::Gap { s, .. } => v.push(opt_id(*s)),
            OpIn::Load {
                tol,
                obj,
                value,
                params,
                offset,
                pre,
                ..
            } => {
                v.extend([*tol, *obj as f64, *value]);
                v.extend((0..10).map(|i| params.get(i).copied().unwrap_or(0.0)));
                v.extend([*offset as f64, *pre]);
            }
            OpIn::Connected { j, .. } => v.push(*j as f64),
            OpIn::ConnectedAny { js, .. } => {
                v.push(js.len() as f64);
                v.extend(js.iter().map(|&j| j as f64));
            }
            _ => {}
        }
    }
    Ok(v)
}

fn op_code(o: &OpIn) -> (usize, usize) {
    match *o {
        OpIn::Reset { t, .. } => (op::RESET, t),
        OpIn::SetClassObject { t, .. } => (op::SET_CLASS_OBJECT, t),
        OpIn::SetT { t, .. } => (op::SET_T, t),
        OpIn::SetUv { t, .. } => (op::SET_UV, t),
        OpIn::SetUvw { t, .. } => (op::SET_UVW, t),
        OpIn::Unset { t } => (op::UNSET, t),
        OpIn::SetGap { t, .. } => (op::SET_GAP, t),
        OpIn::SetDeviation { t, .. } => (op::SET_DEVIATION, t),
        OpIn::SetPresnap { t, .. } => (op::SET_PRESNAP, t),
        OpIn::SetSss { t, .. } => (op::SET_SSS, t),
        OpIn::SetSrcTol { t, .. } => (op::SET_SRC_TOL, t),
        OpIn::SetFace { t, .. } => (op::SET_FACE, t),
        OpIn::Assign { t } => (op::ASSIGN, t),
        OpIn::CopyClassOnly { t } => (op::COPY_CLASS_ONLY, t),
        OpIn::CopySss { t } => (op::COPY_SSS, t),
        OpIn::Combine { t } => (op::COMBINE, t),
        OpIn::Compute { t, .. } => (op::COMPUTE, t),
        OpIn::Find { t } => (op::FIND, t),
        OpIn::Gap { t, .. } => (op::GAP, t),
        OpIn::Load { t, .. } => (op::LOAD, t),
        OpIn::Merge { t, .. } => (op::MERGE, t),
        OpIn::Brep { t } => (op::BREP, t),
        OpIn::Connected { t, .. } => (op::CONNECTED, t),
        OpIn::ConnectedAny { t, .. } => (op::CONNECTED_ANY, t),
        OpIn::CurveObject { t } => (op::CURVE_OBJECT, t),
        OpIn::ObjTol { t } => (op::OBJ_TOL, t),
        OpIn::XsectTol { t } => (op::XSECT_TOL, t),
        OpIn::SrcTol { t } => (op::SRC_TOL, t),
        OpIn::BendTypes { t } => (op::BEND_TYPES, t),
    }
}

fn cpp(s: &Script) -> Result<Output, HarnessError> {
    let input = cpp_input(s)?;
    let out = ffi::run(&input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side --------------------------------------------------------------------

/// A pool handle: index and kind.
#[derive(Debug, Clone, Copy, PartialEq)]
struct H(usize, ClassObjectKind);

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        self.1
    }
}

enum PoolObj {
    Curve(Curve),
    Surface(Surface),
    Point(Vec3),
    Bare,
}

struct Pool {
    objs: Vec<PoolObj>,
    handles: Vec<H>,
    kinds: Vec<usize>,
}

impl ClassGeometry<H> for Pool {
    fn curve(&self, o: H) -> Option<&Curve> {
        match self.objs.get(o.0) {
            Some(PoolObj::Curve(c)) => Some(c),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match self.objs.get(o.0) {
            Some(PoolObj::Surface(s)) => Some(s),
            _ => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, o: H) -> Option<Vec3> {
        match self.objs.get(o.0) {
            Some(PoolObj::Point(p)) => Some(*p),
            _ => None,
        }
    }
}

impl Pool {
    fn handle(&self, i: Option<usize>) -> Result<Option<H>, HarnessError> {
        i.map(|i| {
            self.handles
                .get(i)
                .copied()
                .ok_or_else(|| harness("object index out of range"))
        })
        .transpose()
    }

    fn kind(&self, o: Option<H>) -> Option<usize> {
        o.and_then(|h| self.kinds.get(h.0).copied())
    }

    fn id_of_curve(&self, c: &Curve) -> f64 {
        self.objs
            .iter()
            .position(|o| matches!(o, PoolObj::Curve(x) if core::ptr::eq(x, c)))
            .map_or(-2.0, |i| i as f64)
    }
}

fn class_kind_of(k: usize) -> ClassObjectKind {
    match k {
        kind::CURVE => ClassObjectKind::Curve,
        kind::PLANE => ClassObjectKind::Surface,
        kind::VOLUME => ClassObjectKind::Volume,
        kind::POINT => ClassObjectKind::Point,
        kind::VERTEX => ClassObjectKind::Vertex,
        kind::EDGE => ClassObjectKind::Edge,
        kind::FACE => ClassObjectKind::Face,
        kind::REGION => ClassObjectKind::Region,
        kind::EDGEUSE => ClassObjectKind::Edgeuse,
        _ => ClassObjectKind::Other,
    }
}

fn build_pool(objects: &[ObjIn]) -> Result<Pool, HarnessError> {
    let mut pool = Pool {
        objs: Vec::new(),
        handles: Vec::new(),
        kinds: Vec::new(),
    };
    for (i, o) in objects.iter().enumerate() {
        let (k, obj) = match o {
            ObjIn::Curve(c) => (kind::CURVE, PoolObj::Curve(build_curve(c)?)),
            ObjIn::Plane { o, n } => (
                kind::PLANE,
                PoolObj::Surface(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
            ),
            ObjIn::Volume => (kind::VOLUME, PoolObj::Bare),
            ObjIn::Point(p) => (kind::POINT, PoolObj::Point(v3(*p))),
            ObjIn::Vertex => (kind::VERTEX, PoolObj::Bare),
            ObjIn::Edge(_) => (kind::EDGE, PoolObj::Bare),
            ObjIn::Face(_) => (kind::FACE, PoolObj::Bare),
            ObjIn::Region => (kind::REGION, PoolObj::Bare),
            ObjIn::Edgeuse => (kind::EDGEUSE, PoolObj::Bare),
            ObjIn::Other => (kind::OTHER, PoolObj::Bare),
        };
        pool.objs.push(obj);
        pool.handles.push(H(i, class_kind_of(k)));
        pool.kinds.push(k);
    }
    Ok(pool)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, a harness error otherwise.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(v)` or the status; `Unported` defers the case.
fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(Err(status(&e))),
        },
    }
}

fn put_state(o: &mut Vec<f64>, pc: &PointClassification<H>) {
    let id = |h: Option<H>| h.map_or(-1.0, |h| h.0 as f64);
    o.push(f64::from(pc.point_class().code()));
    o.push(id(pc.object()));
    o.push(id(pc.face()));
    o.push(pc.deviation());
    o.push(pc.t_param());
    let uv = pc.uv_param();
    o.extend([uv.x, uv.y]);
    let w = pc.uvw_param();
    o.extend([w.x, w.y, w.z]);
    o.push(b(pc.are_parameters_set()));
    o.push(pc.pre_snap_param());
    o.push(b(pc.sss_point_flag()));
    let s = pc.sss_point();
    o.extend([s.x, s.y, s.z]);
    o.push(pc.tolerance());
    o.push(id(pc.vertex_object()));
    o.push(id(pc.edge_object()));
    o.push(id(pc.face_object()));
}

fn rejected() -> Fail {
    Fail::Harness(harness("op rejected (undefined behaviour upstream)"))
}

fn is_stand_in(k: Option<usize>) -> bool {
    matches!(
        k,
        Some(kind::VOLUME | kind::REGION | kind::EDGEUSE | kind::OTHER)
    )
}

fn push_str(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let pool = build_pool(&s.objects)?;
    let mut pcs: [PointClassification<H>; 2] = [
        // SmPointClassification(SmZoneTol3d(tol)): the conversion happens once, in the
        // argument
        PointClassification::new(zone_tol3d(&k, s.tol[0])),
        PointClassification::new(zone_tol3d(&k, s.tol[1])),
    ];
    let mut o: Vec<f64> = Vec::new();
    // after a deferred op the remaining ops still run (their state may differ from
    // upstream's) so that every op sees the run-time rejections of the shim
    let mut deferred: Option<Box<Output>> = None;
    for x in &s.ops {
        let mut step = || -> Result<(), Fail> {
            let (code, t) = op_code(x);
            if t > 1 {
                return Err(harness("bad target").into());
            }
            let other = pcs
                .get(1 - t)
                .copied()
                .ok_or_else(|| harness("bad target"))?;
            let pc = pcs.get_mut(t).ok_or_else(|| harness("bad target"))?;
            let ok = pool.kind(pc.object());
            let class = pc.point_class();
            o.push(code as f64);
            match x {
                OpIn::Reset { tol, .. } => {
                    pc.reset(&k, tol.map(|v| zone_tol3d(&k, v)));
                    o.push(0.0);
                }
                OpIn::SetClassObject { class, obj, .. } => {
                    let c =
                        PointClassType::from_code(*class).ok_or_else(|| harness("bad class"))?;
                    pc.set_class_object(c, pool.handle(*obj)?);
                    o.push(0.0);
                }
                OpIn::SetT { v, snap, .. } => {
                    pc.set_t_param(*v, *snap);
                    o.push(0.0);
                }
                OpIn::SetUv { u, v, .. } => {
                    pc.set_uv_param(Point2::new(*u, *v));
                    o.push(0.0);
                }
                OpIn::SetUvw { p, .. } => {
                    pc.set_uvw_param(v3(*p));
                    o.push(0.0);
                }
                OpIn::Unset { .. } => {
                    pc.unset_parameters();
                    o.push(0.0);
                }
                OpIn::SetGap { g, .. } => {
                    pc.set_gap_3d(*g);
                    o.push(0.0);
                }
                OpIn::SetDeviation { g, .. } => {
                    pc.set_deviation(*g);
                    o.push(0.0);
                }
                OpIn::SetPresnap { v, .. } => {
                    pc.set_pre_snap_param(*v);
                    o.push(0.0);
                }
                OpIn::SetSss { p, .. } => {
                    pc.set_sss_point(v3(*p));
                    o.push(0.0);
                }
                OpIn::SetSrcTol { tol, .. } => {
                    pc.set_src_zone_tol_3d(zone_tol3d(&k, *tol));
                    o.push(0.0);
                }
                OpIn::SetFace { obj, .. } => {
                    pc.set_face(pool.handle(*obj)?);
                    o.push(0.0);
                }
                OpIn::Assign { .. } => {
                    *pc = other;
                    o.push(0.0);
                }
                OpIn::CopyClassOnly { .. } => {
                    pc.copy_classification_only(&other);
                    o.push(0.0);
                }
                OpIn::CopySss { .. } => {
                    pc.copy_sss_point(&other);
                    o.push(0.0);
                }
                OpIn::Combine { .. } => {
                    if class == PointClassType::Vertex && ok != Some(kind::VERTEX) {
                        return Err(rejected());
                    }
                    let which = st(pc.combine(&other))?.map_err(|_| harness("combine failed"))?;
                    o.push(0.0);
                    o.push(if which == Precedence::This { 0.0 } else { 1.0 });
                }
                OpIn::Compute { p, .. } => {
                    if ok == Some(kind::POINT)
                        || is_stand_in(ok)
                        || class == PointClassType::Edgeuse
                    {
                        return Err(rejected());
                    }
                    o.push(
                        st(pc.compute_point_parameters(&k, &pool, v3(*p)))?
                            .err()
                            .unwrap_or(0.0),
                    );
                }
                OpIn::Find { .. } => {
                    if matches!(class, PointClassType::Edgeuse | PointClassType::Volume) {
                        return Err(rejected());
                    }
                    match st(pc.find_obj_point_3d(&k, &pool))? {
                        Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
                        Err(c) => o.push(c),
                    }
                }
                OpIn::Gap { s, .. } => {
                    if s.is_some()
                        && (ok == Some(kind::POINT)
                            || matches!(class, PointClassType::Edgeuse | PointClassType::Volume))
                    {
                        return Err(rejected());
                    }
                    let surf = match pool.handle(*s)? {
                        Some(h) => Some(
                            pool.surface(h)
                                .ok_or_else(|| harness("gap: not a surface"))?,
                        ),
                        None => None,
                    };
                    let g = st(pc.gap_3d(&k, &pool, surf))?.map_err(|_| harness("gap failed"))?;
                    o.extend([0.0, g]);
                }
                OpIn::Load {
                    tol,
                    obj,
                    value,
                    params,
                    offset,
                    pre,
                    ..
                } => {
                    let h = pool
                        .handle(Some(*obj))?
                        .ok_or_else(|| harness("load: no object"))?;
                    if pool.kinds.get(*obj) == Some(&kind::POINT) {
                        return Err(rejected());
                    }
                    let mut end = SolutionEnd::new();
                    end.solution_value = *value;
                    for (d, s) in end.parameters.iter_mut().zip(params) {
                        *d = *s;
                    }
                    let r = pc.load_from_intersection(zone_tol3d(&k, *tol), h, &end, *offset, *pre);
                    o.push(st(r)?.err().unwrap_or(0.0));
                }
                OpIn::Merge { p, .. } => {
                    if matches!(
                        ok,
                        Some(kind::POINT | kind::REGION | kind::FACE | kind::EDGE)
                    ) {
                        return Err(rejected());
                    }
                    match st(pc.merge_into_object(&k, v3(*p)))? {
                        Ok(m) => {
                            o.extend([0.0, m.new_edges.len() as f64, m.new_vertices.len() as f64]);
                        }
                        Err(c) => o.extend([c, 0.0, 0.0]),
                    }
                }
                OpIn::Brep { .. } => {
                    if class == PointClassType::Edgeuse {
                        return Err(rejected());
                    }
                    let r = st(pc.brep(&pool, None))?.map_err(|_| harness("brep failed"))?;
                    o.extend([0.0, b(r.is_some())]);
                }
                OpIn::Connected { j, .. } => {
                    let tgt = topo_target(&pool, *j)?;
                    if matches!(ok, Some(kind::POINT | kind::EDGEUSE)) {
                        return Err(rejected());
                    }
                    let r = st(pc.is_connected_to(&pool, tgt))?.map_err(|_| harness("connected failed"))?;
                    o.extend([0.0, b(r)]);
                }
                OpIn::ConnectedAny { js, .. } => {
                    let tgts = js
                        .iter()
                        .map(|&j| topo_target(&pool, j))
                        .collect::<Result<Vec<_>, _>>()?;
                    if matches!(ok, Some(kind::POINT | kind::EDGEUSE)) {
                        return Err(rejected());
                    }
                    let r = st(pc.is_connected_to_any(&pool, &tgts))?
                        .map_err(|_| harness("connected failed"))?;
                    o.extend([0.0, b(r.is_some()), r.map_or(-1.0, |i| i as f64)]);
                }
                OpIn::CurveObject { .. } => {
                    if class == PointClassType::Edgeuse {
                        return Err(rejected());
                    }
                    let r =
                        st(pc.curve_object(&pool))?.map_err(|_| harness("curve object failed"))?;
                    o.extend([0.0, r.map_or(-1.0, |c| pool.id_of_curve(c))]);
                }
                OpIn::ObjTol { .. } | OpIn::XsectTol { .. } => {
                    if ok == Some(kind::POINT) {
                        return Err(rejected());
                    }
                    let r = if matches!(x, OpIn::ObjTol { .. }) {
                        pc.obj_zone_tol_3d(&k, &pool)
                    } else {
                        pc.xsect_tol_3d(&k, &pool)
                    };
                    let v = st(r)?.map_err(|_| harness("tolerance failed"))?;
                    o.extend([0.0, v]);
                }
                OpIn::SrcTol { .. } => o.extend([0.0, pc.src_zone_tol_3d()]),
                OpIn::BendTypes { .. } => {
                    o.push(0.0);
                    o.push(space_bend_type() as f64);
                    o.push(space_unbend_type() as f64);
                    push_str(&mut o, SPACE_BEND_TYPE_STRING);
                    push_str(&mut o, SPACE_UNBEND_TYPE_STRING);
                }
            }
            put_state(&mut o, pc);
            Ok(())
        };
        match step() {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => {
                deferred.get_or_insert(d);
            }
            Err(e) => return Err(e),
        }
    }
    match deferred {
        Some(d) => Err(Fail::Deferred(d)),
        None => Ok(o),
    }
}

/// A topology target of `IsConnectedTo`.
fn topo_target(pool: &Pool, j: usize) -> Result<H, HarnessError> {
    match pool.kinds.get(j) {
        Some(&(kind::VERTEX | kind::EDGE | kind::FACE)) => pool
            .handles
            .get(j)
            .copied()
            .ok_or_else(|| harness("bad target")),
        _ => Err(harness("IsConnectedTo: target is not topology")),
    }
}

fn rust(s: &Script) -> Result<Output, HarnessError> {
    match rust_run(s) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generator --------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational or 2D).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.1);
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
    let mut ks: Vec<f64> = (0..n - p)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
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
    BSplineIn {
        dim: if two_d { 2 } else { 3 },
        oob: rng.chance(0.1),
        cpts,
        p,
        u,
    }
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    match rng.below(10) {
        0..=1 => {
            // a0 + span stays within the [-360, 360] that CreateCircleSegment accepts
            let a0 = rng.uniform(-90.0, 0.0);
            let c = CurveIn::Circle(CircleIn {
                center: rp(rng, 5.0),
                x: [1.0, 0.0, 0.0],
                y: [0.0, rng.uniform(0.5, 1.0), rng.uniform(-0.5, 0.5)],
                radius: rng.uniform(0.5, 10.0),
                a0,
                a1: a0 + rng.uniform(20.0, 360.0),
            });
            resolve(&c).unwrap_or(c)
        }
        2..=3 => {
            let d0 = rng.uniform(-5.0, 5.0);
            CurveIn::Line(LineIn {
                p: rp(rng, 10.0),
                v: rp(rng, 3.0),
                domain: [d0, d0 + rng.uniform(0.5, 10.0)],
                scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
                inside_out: rng.chance(0.3),
            })
        }
        4..=5 => CurveIn::Bspline(gen_bspline(rng)),
        6..=7 => {
            let d0 = rng.uniform(-180.0, 180.0);
            let mut y = rp(rng, 1.0);
            if y.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
                y = [0.0, 1.0, 0.0];
            }
            CurveIn::Conic(ConicIn {
                center: rp(rng, 5.0),
                x: [1.0, 0.0, 0.0],
                y,
                domain: [d0, d0 + rng.uniform(10.0, 360.0)],
                rx: rng.uniform(0.5, 10.0),
                ry: rng.chance(0.5).then(|| rng.uniform(0.5, 10.0)),
                inside_out: rng.chance(0.2),
            })
        }
        _ => CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: rp(rng, 5.0),
            d1: rp(rng, 5.0),
            p2: rp(rng, 5.0),
            d2: rp(rng, 5.0),
        }),
    }
}

fn gen_objects(rng: &mut Rng) -> Vec<ObjIn> {
    let mut objs = Vec::new();
    for _ in 0..=rng.below(3) {
        objs.push(ObjIn::Curve(gen_curve(rng)));
    }
    let mut n = rp(rng, 1.0);
    if n.iter().map(|c| c * c).sum::<f64>() < 1e-3 {
        n = [0.0, 0.0, 1.0];
    }
    objs.push(ObjIn::Plane {
        o: rp(rng, 5.0),
        n,
    });
    objs.push(ObjIn::Point(rp(rng, 10.0)));
    objs.push(ObjIn::Point(rp(rng, 10.0)));
    objs.push(ObjIn::Vertex);
    objs.push(ObjIn::Vertex);
    objs.push(ObjIn::Edge(Some(0)));
    let plane = objs
        .iter()
        .position(|o| matches!(o, ObjIn::Plane { .. }));
    objs.push(ObjIn::Face(plane));
    objs.push(ObjIn::Volume);
    objs.push(ObjIn::Region);
    objs.push(ObjIn::Edgeuse);
    objs.push(ObjIn::Other);
    objs
}

/// Pool indices of objects of kind `k`.
fn of_kind(objs: &[ObjIn], k: usize) -> Vec<usize> {
    objs.iter()
        .enumerate()
        .filter(|(_, o)| obj_kind(o) == k)
        .map(|(i, _)| i)
        .collect()
}

fn obj_kind(o: &ObjIn) -> usize {
    match o {
        ObjIn::Curve(_) => kind::CURVE,
        ObjIn::Plane { .. } => kind::PLANE,
        ObjIn::Volume => kind::VOLUME,
        ObjIn::Point(_) => kind::POINT,
        ObjIn::Vertex => kind::VERTEX,
        ObjIn::Edge(_) => kind::EDGE,
        ObjIn::Face(_) => kind::FACE,
        ObjIn::Region => kind::REGION,
        ObjIn::Edgeuse => kind::EDGEUSE,
        ObjIn::Other => kind::OTHER,
    }
}

fn class_object_kind(class: i32) -> Option<usize> {
    match class {
        101 => Some(kind::REGION),
        102 => Some(kind::FACE),
        103 => Some(kind::EDGE),
        104 => Some(kind::VERTEX),
        105 => Some(kind::POINT),
        106 => Some(kind::CURVE),
        107 => Some(kind::PLANE),
        108 => Some(kind::VOLUME),
        109 => Some(kind::EDGEUSE),
        _ => None,
    }
}

/// A point near the pool's first curve or plane, or anywhere.
fn gen_point(k: &Kernel, rng: &mut Rng, objs: &[ObjIn]) -> [f64; 3] {
    let near = |rng: &mut Rng, p: [f64; 3], d: f64| {
        let e = rp(rng, d);
        [p[0] + e[0], p[1] + e[1], p[2] + e[2]]
    };
    if rng.chance(0.7) {
        if let Some(ObjIn::Curve(c)) = objs.first() {
            if let Ok(curve) = build_curve(c) {
                let ivl = curve.natural_interval();
                let t = ivl.min() + (ivl.max() - ivl.min()) * rng.uniform(-0.1, 1.1);
                if let Ok(p) = curve.evaluate_point(k, t) {
                    let d = *rng.choose(&[0.0, 1e-9, 1e-6, 1e-4, 0.1, 2.0]).unwrap_or(&0.0);
                    return near(rng, [p.x, p.y, p.z], d);
                }
            }
        }
    }
    rp(rng, 20.0)
}

fn gen_class_object(rng: &mut Rng, objs: &[ObjIn], geometric: bool) -> (i32, Option<usize>) {
    let classes: &[i32] = if geometric {
        &[106, 106, 106, 107, 105, 108]
    } else {
        &[100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110]
    };
    let class = *rng.choose(classes).unwrap_or(&106);
    // SetClassObject(class, NULL) is valid for any class (the class becomes UNKNOWN)
    if rng.chance(0.1) {
        return (class, None);
    }
    let obj = class_object_kind(class).and_then(|k| {
        let ids = of_kind(objs, k);
        rng.choose(&ids).copied()
    });
    if class_object_kind(class).is_some() && obj.is_none() {
        return (100, None);
    }
    (class, obj)
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng
        .choose(&[1e-5, 1e-5, 1e-3, 0.1, 2e-5, USE_DEFAULT, 0.0])
        .unwrap_or(&1e-5)
}

fn gen_op(k: &Kernel, rng: &mut Rng, objs: &[ObjIn], geometric: bool) -> OpIn {
    let t = rng.below(2) as usize;
    let topo: Vec<usize> = objs
        .iter()
        .enumerate()
        .filter(|(_, o)| matches!(o, ObjIn::Vertex | ObjIn::Edge(_) | ObjIn::Face(_)))
        .map(|(i, _)| i)
        .collect();
    let any_obj: Vec<usize> = objs
        .iter()
        .enumerate()
        .filter(|(_, o)| !matches!(o, ObjIn::Point(_)))
        .map(|(i, _)| i)
        .filter(|&i| {
            !geometric || matches!(objs.get(i), Some(ObjIn::Curve(_) | ObjIn::Plane { .. } | ObjIn::Volume))
        })
        .collect();
    loop {
        let r = rng.below(31);
        let op = match r {
            0 => OpIn::Reset {
                t,
                tol: rng.chance(0.5).then(|| gen_tol(rng)),
            },
            1..=3 => {
                let (class, obj) = gen_class_object(rng, objs, geometric);
                OpIn::SetClassObject { t, class, obj }
            }
            4 => OpIn::SetT {
                t,
                v: rng.uniform(-5.0, 10.0),
                snap: rng.chance(0.5),
            },
            5 => OpIn::SetUv {
                t,
                u: rng.uniform(-5.0, 5.0),
                v: rng.uniform(-5.0, 5.0),
            },
            6 => OpIn::SetUvw { t, p: rp(rng, 2.0) },
            7 => OpIn::Unset { t },
            8 => OpIn::SetGap {
                t,
                g: *rng.choose(&[0.0, 1e-6, 0.5, 2.0]).unwrap_or(&0.0),
            },
            9 => OpIn::SetDeviation {
                t,
                g: rng.uniform(0.0, 1.0),
            },
            10 => OpIn::SetPresnap {
                t,
                v: if rng.chance(0.3) { BIG } else { rng.uniform(-3.0, 3.0) },
            },
            11 => OpIn::SetSss { t, p: rp(rng, 5.0) },
            12 => OpIn::SetSrcTol { t, tol: gen_tol(rng) },
            13 => {
                let faces = of_kind(objs, kind::FACE);
                OpIn::SetFace {
                    t,
                    obj: if rng.chance(0.3) {
                        None
                    } else {
                        rng.choose(&faces).copied()
                    },
                }
            }
            14 => OpIn::Assign { t },
            15 => OpIn::CopyClassOnly { t },
            16 => OpIn::CopySss { t },
            17..=19 => OpIn::Combine { t },
            20..=22 => OpIn::Compute {
                t,
                p: gen_point(k, rng, objs),
            },
            23 => OpIn::Find { t },
            24 => {
                let planes = of_kind(objs, kind::PLANE);
                OpIn::Gap {
                    t,
                    s: if rng.chance(0.8) {
                        None
                    } else {
                        rng.choose(&planes).copied()
                    },
                }
            }
            25 => {
                let Some(&obj) = rng.choose(&any_obj) else {
                    continue;
                };
                let params = (0..10).map(|_| rng.uniform(-2.0, 5.0)).collect();
                OpIn::Load {
                    t,
                    tol: gen_tol(rng),
                    obj,
                    value: rng.uniform(0.0, 0.5),
                    params,
                    offset: rng.below(10) as usize,
                    pre: if rng.chance(0.5) { BIG } else { rng.uniform(0.0, 1.0) },
                }
            }
            26 => OpIn::Merge {
                t,
                p: gen_point(k, rng, objs),
            },
            27 => OpIn::Brep { t },
            28 => {
                if rng.chance(0.5) {
                    let Some(&j) = rng.choose(&topo) else {
                        continue;
                    };
                    OpIn::Connected { t, j }
                } else {
                    let n = rng.below(4) as usize;
                    let js = (0..n).filter_map(|_| rng.choose(&topo).copied()).collect();
                    OpIn::ConnectedAny { t, js }
                }
            }
            29 => OpIn::CurveObject { t },
            _ => match rng.below(4) {
                0 => OpIn::ObjTol { t },
                1 => OpIn::XsectTol { t },
                2 => OpIn::SrcTol { t },
                _ => OpIn::BendTypes { t },
            },
        };
        return op;
    }
}

fn gen_script(k: &Kernel, rng: &mut Rng) -> Script {
    let objects = gen_objects(rng);
    // most scripts stay on curves, surfaces and volumes (comparable now); some
    // exercise topology (deferred on the Rust side)
    let geometric = rng.chance(0.8);
    let n_ops = 2 + rng.below(12) as usize;
    let mut ops = Vec::new();
    // start both classifications on an object most of the time
    for t in 0..2 {
        if rng.chance(0.8) {
            let (class, obj) = gen_class_object(rng, &objects, geometric);
            ops.push(OpIn::SetClassObject { t, class, obj });
        }
    }
    for _ in 0..n_ops {
        ops.push(gen_op(k, rng, &objects, geometric));
    }
    Script {
        objects,
        tol: [gen_tol(rng), gen_tol(rng)],
        ops,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.point_class"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let s = gen_script(k, rng);
            let Ok(input) = cpp_input(&s) else {
                continue;
            };
            // the static checks of the ffi wrapper and the run-time rejections (which
            // the Rust side reproduces)
            if ffi::validate(&input).is_err() {
                continue;
            }
            if let Err(Fail::Harness(_)) = rust_run(&s) {
                continue;
            }
            return serde_json::to_value(s).ok();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated scripts are accepted by the ffi checks, and most of them are
    /// comparable now (not deferred).
    #[test]
    fn generated_scripts_are_mostly_comparable() {
        let mut rng = Rng::new(11);
        let (mut total, mut deferred) = (0, 0);
        for _ in 0..500 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
        }
        assert!(total > 450, "{total}");
        assert!(deferred * 2 < total, "{deferred} of {total} deferred");
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Prints generated case `PC_CASE` as JSON (a debugging aid:
    /// `PC_CASE=6 cargo test -p difftest --features cpp --lib dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs PC_CASE"]
    fn dump_case() {
        let i: usize = std::env::var("PC_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("intersect.point_class", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
    }
}
