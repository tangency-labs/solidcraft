//! Runner for unit `intersect.curve_class_p3`: the part-3 members of `SmCurveClassification`
//! (`FindInterval`, `FindPointClass`, `IsCoincident`, `InsertPointClass`, `RemovePointClass`,
//! `MergePointClass`, `CleanupAndValidate`) and the static `sm_ChangeEdgePointClassToNearVertex`
//! and `sm_FixProblemsBySqueezing` of `SmCurveClass.cpp`.
//!
//! C++: `solidcore_ffi::units::intersect_curve_class_p3` (upstream
//! `source/SMLib/src/SmCurveClass.cpp`). Rust: `solidcore::intersect::curve_class::p3`.
//!
//! A case is a script: a pool of `objects` (curves as in unit `curve.curve_p6` plus real
//! `SmEllipse` / `SmCircle`, planes, B-spline surfaces, points, vertex, edge and face objects,
//! loop and "other" stand-ins) and `ops` on two default classifications `cc[0]`, `cc[1]`: the
//! setters of unit `intersect.curve_class_p1` build them (intervals, boundary classifications,
//! the classification curve and tolerances), then the part-3 ops run. Both sides produce the
//! flat record documented in the ffi module (per op: code, status, outputs, state of the
//! classification(s)), reported as the `out` values. A Rust error maps to its `SmStatus`
//! (`InvalidInput` 1007, `-1` for one upstream cannot return); an `Unported` error anywhere
//! defers the case.
//!
//! Deferred cases: every path that reaches `InsertAt`, `EditIntervalMin/Max` or
//! `ConnectIntervals` (`intersect.curve_class_p2`), `GetPointClassByIndex`,
//! `GetPointClassMateByIndex` or `AreCoincidentMaybe` (`intersect.curve_class_p7`: the whole of
//! `MergePointClass`, `sm_ChangeEdgePointClassToNearVertex` and `CleanupAndValidate` of a
//! non-empty classification), the zone tolerance of an edge, vertex or face classification
//! (`topology.edge`, `topology.vertex`, `topology.face_p2`: an opaque handle cannot resolve to a
//! B-rep) and the 2D tolerance mapping that needs a surface point solve. Generated scripts are
//! filtered by the shim's run-time checks (scripts it rejects as undefined upstream are
//! dropped).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::{
    self as cc_mod, change_edge_point_class_to_near_vertex, fix_problems_by_squeezing,
    CurveClassification, CurveInterval, IntervalPosition,
};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_curve_class_p3::{self as ffi, kind, op};

use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `intersect.curve_class_p3` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SM_USE_DEFAULT`.
const USE_DEFAULT: f64 = 9876543.21;
/// `SM_BIG_DOUBLE`.
const BIG: f64 = 1.0e20;
/// `SC_ERR_ARG`.
const SC_ERR_ARG: i32 = -1002;

// ---- inputs -----------------------------------------------------------------------------

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
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Line(LineIn),
    Conic(ConicIn),
}

/// A B-spline surface: control net `net[i][j]` (`x y z w`, `w = NOW` non-rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfIn {
    #[serde(default)]
    oob: bool,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ObjIn {
    Curve(CurveIn),
    Plane { o: [f64; 3], n: [f64; 3] },
    Bsurf(SurfIn),
    Point([f64; 3]),
    Vertex,
    Edge(Option<usize>),
    Face(Option<usize>),
    Loop,
    Other,
}

/// A point classification: `class obj t gap presnap src_tol [sss]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PcIn {
    class: i32,
    obj: Option<usize>,
    t: f64,
    gap: f64,
    presnap: f64,
    tol: f64,
    sss: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    SetSize { t: usize, n: usize },
    RemoveAt { t: usize, j: usize },
    IvlSet { t: usize, j: usize, a: f64, b: f64 },
    IvlSetMin { t: usize, j: usize, v: f64 },
    IvlSetMax { t: usize, j: usize, v: f64 },
    IvlReset { t: usize, j: usize },
    IvlAssign { t: usize, j: usize, k: usize },
    IvlNew { t: usize, j: usize, with: bool, owner: bool, a: f64, b: f64 },
    PcSetClass { t: usize, j: usize, w: usize, class: i32, obj: Option<usize> },
    PcSetT { t: usize, j: usize, w: usize, v: f64 },
    PcSetUv { t: usize, j: usize, w: usize, u: f64, v: f64 },
    PcSetGap { t: usize, j: usize, w: usize, g: f64 },
    PcSetSss { t: usize, j: usize, w: usize, p: [f64; 3] },
    PcSetPresnap { t: usize, j: usize, w: usize, v: f64 },
    PcSetSrcTol { t: usize, j: usize, w: usize, tol: f64 },
    SetCurve { t: usize, obj: Option<usize>, owned: bool },
    SetSurface1 { t: usize, obj: Option<usize> },
    SetSurface2 { t: usize, obj: Option<usize> },
    SetFace { t: usize, obj: Option<usize> },
    SetIntersectData {
        t: usize,
        s1: Option<usize>,
        s2: Option<usize>,
        uv2: Option<usize>,
        current: bool,
        ti: Option<usize>,
    },
    SetIsCurveCurrent { t: usize, b: bool },
    SetIgnoreTangency { t: usize, b: bool },
    SetIsCurveOwned { t: usize, b: bool },
    SetOkToSetTol { t: usize, b: bool },
    SetFilletData { t: usize, obj: Option<usize>, b: bool },
    SetTgtLoopsOnly { t: usize, b: bool },
    AddTgtLoop { t: usize, obj: Option<usize> },
    SetSrcTol { t: usize, tol: f64 },
    SetUvCurve1 { t: usize, obj: Option<usize> },
    SetUvCurve2 { t: usize, obj: Option<usize> },
    FindInterval { t: usize, prm: f64, last: f64, tol: f64 },
    FindPc { t: usize, prm: f64, tol: f64 },
    IsCoincident { t: usize, a: f64, b: f64, tol: f64 },
    Insert { t: usize, pc: PcIn, prm: f64, last: f64, force: bool },
    RemovePc { t: usize, j: usize, pos: i32 },
    Merge { t: usize, index: usize },
    Cleanup { t: usize },
    NearVertex { t: usize, index: usize, p: [f64; 3] },
    Squeeze { t: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Script {
    objects: Vec<ObjIn>,
    ops: Vec<OpIn>,
}

// ---- objects ----------------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn opt_id(o: Option<usize>) -> f64 {
    o.map_or(-1.0, |i| i as f64)
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
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
    }
}

fn surface_block(v: &mut Vec<f64>, s: &SurfIn) {
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    v.push(b(s.oob));
    v.extend([
        n as f64,
        m as f64,
        s.p as f64,
        s.q as f64,
        s.u.len().saturating_sub(1) as f64,
        s.v.len().saturating_sub(1) as f64,
    ]);
    for row in &s.net {
        for c in row {
            v.extend_from_slice(c);
        }
    }
    v.extend_from_slice(&s.u);
    v.extend_from_slice(&s.v);
}

fn build_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
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
        CurveIn::Line(l) => {
            let opts = BoundedLineOptions {
                scale: l.scale,
                inside_out: l.inside_out,
                ..BoundedLineOptions::default()
            };
            Ok(Curve::from(Line::bounded(
                v3(l.p),
                v3(l.v),
                &Extent1d::from_min_max(l.domain[0], l.domain[1]),
                &opts,
            )))
        }
        CurveIn::Conic(c) => {
            let k = Kernel::default();
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: c.inside_out,
            };
            let domain = Extent1d::from_min_max(c.domain[0], c.domain[1]);
            let e = match c.ry {
                Some(ry) => Ellipse::new(&k, v3(c.center), v3(c.x), v3(c.y), domain, c.rx, ry, &opts),
                None => Ellipse::new_circle(&k, v3(c.center), v3(c.x), v3(c.y), domain, c.rx, &opts),
            }
            .map_err(|e| harness(format!("conic: {e}")))?;
            Ok(Curve::from(e))
        }
    }
}

fn build_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| row.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
        .collect();
    let sur = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface: {e}")))?;
    let mut bs = BSplineSurface::from_nurb(sur, false);
    *bs.out_of_bounds_enabled_mut() = s.oob;
    Ok(Surface::from(bs))
}

// ---- C++ side ---------------------------------------------------------------------------

fn pc_block(v: &mut Vec<f64>, pc: &PcIn) {
    v.extend([
        f64::from(pc.class),
        opt_id(pc.obj),
        pc.t,
        pc.gap,
        pc.presnap,
        pc.tol,
        b(pc.sss.is_some()),
    ]);
    v.extend_from_slice(&pc.sss.unwrap_or([0.0; 3]));
}

fn cpp_input(s: &Script) -> Vec<f64> {
    let mut v = vec![s.objects.len() as f64];
    for o in &s.objects {
        match o {
            ObjIn::Curve(c) => {
                v.push(kind::CURVE as f64);
                curve_block(&mut v, c);
            }
            ObjIn::Plane { o, n } => {
                v.push(kind::PLANE as f64);
                v.extend_from_slice(o);
                v.extend_from_slice(n);
            }
            ObjIn::Bsurf(sf) => {
                v.push(kind::BSURF as f64);
                surface_block(&mut v, sf);
            }
            ObjIn::Point(p) => {
                v.push(kind::POINT as f64);
                v.extend_from_slice(p);
            }
            ObjIn::Vertex => v.push(kind::VERTEX as f64),
            ObjIn::Edge(c) => v.extend([kind::EDGE as f64, opt_id(*c)]),
            ObjIn::Face(sf) => v.extend([kind::FACE as f64, opt_id(*sf)]),
            ObjIn::Loop => v.push(kind::LOOP as f64),
            ObjIn::Other => v.push(kind::OTHER as f64),
        }
    }
    v.push(s.ops.len() as f64);
    for o in &s.ops {
        let (code, t) = op_code(o);
        v.extend([code as f64, t as f64]);
        match o {
            OpIn::SetSize { n, .. } => v.push(*n as f64),
            OpIn::RemoveAt { j, .. } | OpIn::IvlReset { j, .. } => v.push(*j as f64),
            OpIn::IvlSet { j, a, b: bb, .. } => v.extend([*j as f64, *a, *bb]),
            OpIn::IvlSetMin { j, v: x, .. } | OpIn::IvlSetMax { j, v: x, .. } => {
                v.extend([*j as f64, *x]);
            }
            OpIn::IvlAssign { j, k, .. } => v.extend([*j as f64, *k as f64]),
            OpIn::IvlNew {
                j, with, owner, a, b: bb, ..
            } => v.extend([*j as f64, b(*with), b(*owner), *a, *bb]),
            OpIn::PcSetClass { j, w, class, obj, .. } => {
                v.extend([*j as f64, *w as f64, f64::from(*class), opt_id(*obj)]);
            }
            OpIn::PcSetT { j, w, v: x, .. } | OpIn::PcSetPresnap { j, w, v: x, .. } => {
                v.extend([*j as f64, *w as f64, *x]);
            }
            OpIn::PcSetUv { j, w, u, v: x, .. } => v.extend([*j as f64, *w as f64, *u, *x]),
            OpIn::PcSetGap { j, w, g, .. } => v.extend([*j as f64, *w as f64, *g]),
            OpIn::PcSetSrcTol { j, w, tol, .. } => v.extend([*j as f64, *w as f64, *tol]),
            OpIn::PcSetSss { j, w, p, .. } => {
                v.extend([*j as f64, *w as f64]);
                v.extend_from_slice(p);
            }
            OpIn::SetCurve { obj, owned, .. } => v.extend([opt_id(*obj), b(*owned)]),
            OpIn::SetFilletData { obj, b: x, .. } => v.extend([opt_id(*obj), b(*x)]),
            OpIn::SetSurface1 { obj, .. }
            | OpIn::SetSurface2 { obj, .. }
            | OpIn::SetFace { obj, .. }
            | OpIn::AddTgtLoop { obj, .. }
            | OpIn::SetUvCurve1 { obj, .. }
            | OpIn::SetUvCurve2 { obj, .. } => v.push(opt_id(*obj)),
            OpIn::SetIntersectData {
                s1, s2, uv2, current, ti, ..
            } => v.extend([opt_id(*s1), opt_id(*s2), opt_id(*uv2), b(*current), opt_id(*ti)]),
            OpIn::SetIsCurveCurrent { b: x, .. }
            | OpIn::SetIgnoreTangency { b: x, .. }
            | OpIn::SetIsCurveOwned { b: x, .. }
            | OpIn::SetOkToSetTol { b: x, .. }
            | OpIn::SetTgtLoopsOnly { b: x, .. } => v.push(b(*x)),
            OpIn::SetSrcTol { tol, .. } => v.push(*tol),
            OpIn::FindInterval { prm, last, tol, .. } => v.extend([*prm, *last, *tol]),
            OpIn::FindPc { prm, tol, .. } => v.extend([*prm, *tol]),
            OpIn::IsCoincident { a, b: bb, tol, .. } => v.extend([*a, *bb, *tol]),
            OpIn::Insert {
                pc, prm, last, force, ..
            } => {
                pc_block(&mut v, pc);
                v.extend([*prm, *last, b(*force)]);
            }
            OpIn::RemovePc { j, pos, .. } => v.extend([*j as f64, f64::from(*pos)]),
            OpIn::Merge { index, .. } => v.push(*index as f64),
            OpIn::NearVertex { index, p, .. } => {
                v.push(*index as f64);
                v.extend_from_slice(p);
            }
            OpIn::Cleanup { .. } | OpIn::Squeeze { .. } => {}
        }
    }
    v
}

fn op_code(o: &OpIn) -> (usize, usize) {
    match *o {
        OpIn::SetSize { t, .. } => (op::SET_SIZE, t),
        OpIn::RemoveAt { t, .. } => (op::REMOVE_AT, t),
        OpIn::IvlSet { t, .. } => (op::IVL_SET, t),
        OpIn::IvlSetMin { t, .. } => (op::IVL_SET_MIN, t),
        OpIn::IvlSetMax { t, .. } => (op::IVL_SET_MAX, t),
        OpIn::IvlReset { t, .. } => (op::IVL_RESET, t),
        OpIn::IvlAssign { t, .. } => (op::IVL_ASSIGN, t),
        OpIn::IvlNew { t, .. } => (op::IVL_NEW, t),
        OpIn::PcSetClass { t, .. } => (op::PC_SET_CLASS, t),
        OpIn::PcSetT { t, .. } => (op::PC_SET_T, t),
        OpIn::PcSetUv { t, .. } => (op::PC_SET_UV, t),
        OpIn::PcSetGap { t, .. } => (op::PC_SET_GAP, t),
        OpIn::PcSetSss { t, .. } => (op::PC_SET_SSS, t),
        OpIn::PcSetPresnap { t, .. } => (op::PC_SET_PRESNAP, t),
        OpIn::PcSetSrcTol { t, .. } => (op::PC_SET_SRC_TOL, t),
        OpIn::SetCurve { t, .. } => (op::SET_CURVE, t),
        OpIn::SetSurface1 { t, .. } => (op::SET_SURFACE1, t),
        OpIn::SetSurface2 { t, .. } => (op::SET_SURFACE2, t),
        OpIn::SetFace { t, .. } => (op::SET_FACE, t),
        OpIn::SetIntersectData { t, .. } => (op::SET_INTERSECT_DATA, t),
        OpIn::SetIsCurveCurrent { t, .. } => (op::SET_IS_CURVE_CURRENT, t),
        OpIn::SetIgnoreTangency { t, .. } => (op::SET_IGNORE_TANGENCY, t),
        OpIn::SetIsCurveOwned { t, .. } => (op::SET_IS_CURVE_OWNED, t),
        OpIn::SetOkToSetTol { t, .. } => (op::SET_OK_TO_SET_TOL, t),
        OpIn::SetFilletData { t, .. } => (op::SET_FILLET_DATA, t),
        OpIn::SetTgtLoopsOnly { t, .. } => (op::SET_TGT_LOOPS_ONLY, t),
        OpIn::AddTgtLoop { t, .. } => (op::ADD_TGT_LOOP, t),
        OpIn::SetSrcTol { t, .. } => (op::SET_SRC_TOL, t),
        OpIn::SetUvCurve1 { t, .. } => (op::SET_UV_CURVE1, t),
        OpIn::SetUvCurve2 { t, .. } => (op::SET_UV_CURVE2, t),
        OpIn::FindInterval { t, .. } => (op::FIND_INTERVAL, t),
        OpIn::FindPc { t, .. } => (op::FIND_PC, t),
        OpIn::IsCoincident { t, .. } => (op::IS_COINCIDENT, t),
        OpIn::Insert { t, .. } => (op::INSERT, t),
        OpIn::RemovePc { t, .. } => (op::REMOVE_PC, t),
        OpIn::Merge { t, .. } => (op::MERGE, t),
        OpIn::Cleanup { t } => (op::CLEANUP, t),
        OpIn::NearVertex { t, .. } => (op::NEAR_VERTEX, t),
        OpIn::Squeeze { t } => (op::SQUEEZE, t),
    }
}

fn cpp(s: &Script) -> Result<Output, HarnessError> {
    let input = cpp_input(s);
    let out = ffi::run(&input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side --------------------------------------------------------------------------

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
}

fn class_kind_of(k: usize) -> ClassObjectKind {
    match k {
        kind::CURVE => ClassObjectKind::Curve,
        kind::PLANE | kind::BSURF => ClassObjectKind::Surface,
        kind::POINT => ClassObjectKind::Point,
        kind::VERTEX => ClassObjectKind::Vertex,
        kind::EDGE => ClassObjectKind::Edge,
        kind::FACE => ClassObjectKind::Face,
        kind::LOOP => ClassObjectKind::Loop,
        _ => ClassObjectKind::Other,
    }
}

fn obj_kind(o: &ObjIn) -> usize {
    match o {
        ObjIn::Curve(_) => kind::CURVE,
        ObjIn::Plane { .. } => kind::PLANE,
        ObjIn::Bsurf(_) => kind::BSURF,
        ObjIn::Point(_) => kind::POINT,
        ObjIn::Vertex => kind::VERTEX,
        ObjIn::Edge(_) => kind::EDGE,
        ObjIn::Face(_) => kind::FACE,
        ObjIn::Loop => kind::LOOP,
        ObjIn::Other => kind::OTHER,
    }
}

fn build_pool(objects: &[ObjIn]) -> Result<Pool, HarnessError> {
    let mut pool = Pool {
        objs: Vec::new(),
        handles: Vec::new(),
    };
    for (i, o) in objects.iter().enumerate() {
        let obj = match o {
            ObjIn::Curve(c) => PoolObj::Curve(build_curve(c)?),
            ObjIn::Plane { o, n } => {
                PoolObj::Surface(Surface::from(Plane::from_normal(v3(*o), v3(*n))))
            }
            ObjIn::Bsurf(s) => PoolObj::Surface(build_surface(s)?),
            ObjIn::Point(p) => PoolObj::Point(v3(*p)),
            _ => PoolObj::Bare,
        };
        pool.objs.push(obj);
        pool.handles.push(H(i, class_kind_of(obj_kind(o))));
    }
    Ok(pool)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.0,
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

/// The value, or a harness error for a status (an operation upstream cannot fail).
fn must<T>(r: solidcore::Result<T>) -> Result<T, Fail> {
    st(r)?.map_err(|s| Fail::Harness(harness(format!("unexpected status {s}"))))
}

fn id(h: Option<H>) -> f64 {
    h.map_or(-1.0, |h| h.0 as f64)
}

fn put_pc(o: &mut Vec<f64>, pc: &PointClassification<H>) {
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
    o.push(pc.src_zone_tol_3d());
}

fn put_cc(o: &mut Vec<f64>, k: &Kernel, cc: &CurveClassification<H>) {
    o.push(cc.size() as f64);
    let ivl = cc.interval();
    o.extend([ivl.min(), ivl.max()]);
    o.push(cc_mod::src_zone_tol_3d_of(k, Some(cc)));
    o.extend([
        b(cc.is_curve_owned()),
        b(cc.is_curve_current()),
        b(cc.ignore_tangency()),
        b(cc.doing_cross_hatch()),
        b(cc.doing_fillets()),
        b(cc.ok_to_set_tolerance()),
        b(cc.tgt_loops_only()),
    ]);
    o.extend([
        id(cc.curve()),
        id(cc.uv_curve1()),
        id(cc.uv_curve2()),
        id(cc.any_uv_curve1()),
        id(cc.any_uv_curve2()),
        id(cc.surface1()),
        id(cc.surface2()),
        id(cc.face()),
        id(cc.topology_intersector()),
        id(cc.other_classification()),
    ]);
    o.push(cc.tgt_loops().len() as f64);
    o.extend(cc.tgt_loops().iter().map(|h| h.0 as f64));
    for ivl in cc.intervals() {
        o.extend([ivl.min(), ivl.max()]);
        put_pc(o, &ivl.start);
        put_pc(o, &ivl.mid);
        put_pc(o, &ivl.end);
    }
}

fn pc_mut(ivl: &mut CurveInterval<H>, w: usize) -> Result<&mut PointClassification<H>, HarnessError> {
    match w {
        0 => Ok(&mut ivl.start),
        1 => Ok(&mut ivl.mid),
        2 => Ok(&mut ivl.end),
        _ => Err(harness("bad point classification")),
    }
}

/// `SmIntervalPosition` of an upstream value.
fn position(code: i32) -> Result<IntervalPosition, HarnessError> {
    match code {
        0 => Ok(IntervalPosition::Uninit),
        1 => Ok(IntervalPosition::Start),
        2 => Ok(IntervalPosition::Inside),
        3 => Ok(IntervalPosition::End),
        4 => Ok(IntervalPosition::Outside),
        _ => Err(harness("bad interval position")),
    }
}

/// The point classification of a [`PcIn`].
fn build_pc(k: &Kernel, pool: &Pool, p: &PcIn) -> Result<PointClassification<H>, HarnessError> {
    let mut pc = PointClassification::new(0.0);
    let c = PointClassType::from_code(p.class).ok_or_else(|| harness("bad class"))?;
    pc.set_class_object(c, pool.handle(p.obj)?);
    pc.set_t_param(p.t, false);
    pc.set_gap_3d(p.gap);
    pc.set_pre_snap_param(p.presnap);
    pc.set_src_zone_tol_3d(zone_tol3d(k, p.tol));
    if let Some(s) = p.sss {
        pc.set_sss_point(v3(s));
    }
    Ok(pc)
}

#[allow(clippy::too_many_lines)]
fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let pool = build_pool(&s.objects)?;
    let mut ccs: [CurveClassification<H>; 2] =
        [CurveClassification::new(&k), CurveClassification::new(&k)];
    let mut o: Vec<f64> = Vec::new();
    let mut deferred: Option<Box<Output>> = None;
    for (n_op, x) in s.ops.iter().enumerate() {
        let mut step = || -> Result<(), Fail> {
            let (code, t) = op_code(x);
            if t > 1 {
                return Err(harness("bad target").into());
            }
            let other = ccs.get(1 - t).cloned().ok_or_else(|| harness("bad target"))?;
            o.push(code as f64);
            {
                let cc = ccs.get_mut(t).ok_or_else(|| harness("bad target"))?;
                match x {
                    OpIn::SetSize { n, .. } => {
                        must(cc.set_size(&k, *n))?;
                        o.push(0.0);
                    }
                    OpIn::RemoveAt { j, .. } => {
                        must(cc.remove_at(*j))?;
                        o.push(0.0);
                    }
                    OpIn::IvlSet { j, a, b: bb, .. } => {
                        must(cc.at_mut(*j))?.set_interval(&Extent1d::from_min_max(*a, *bb));
                        o.push(0.0);
                    }
                    OpIn::IvlSetMin { j, v, .. } => {
                        let r = must(cc.at_mut(*j))?.set_interval_min(*v);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::IvlSetMax { j, v, .. } => {
                        let r = must(cc.at_mut(*j))?.set_interval_max(*v);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::IvlReset { j, .. } => {
                        must(cc.at_mut(*j))?.reset(&k);
                        o.push(0.0);
                    }
                    OpIn::IvlAssign { j, k: kk, .. } => {
                        let src = *must(other.at(*kk))?;
                        *must(cc.at_mut(*j))? = src;
                        o.push(0.0);
                    }
                    OpIn::IvlNew {
                        j, with, owner, a, b: bb, ..
                    } => {
                        let owner_cc = owner.then_some(&*cc);
                        let ivl = if *with {
                            CurveInterval::with_interval(&k, &Extent1d::from_min_max(*a, *bb), owner_cc)
                        } else {
                            CurveInterval::new(&k, owner_cc)
                        };
                        *must(cc.at_mut(*j))? = ivl;
                        o.push(0.0);
                    }
                    OpIn::PcSetClass { j, w, class, obj, .. } => {
                        let c = PointClassType::from_code(*class).ok_or_else(|| harness("bad class"))?;
                        let h = pool.handle(*obj)?;
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_class_object(c, h);
                        o.push(0.0);
                    }
                    OpIn::PcSetT { j, w, v, .. } => {
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_t_param(*v, false);
                        o.push(0.0);
                    }
                    OpIn::PcSetUv { j, w, u, v, .. } => {
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_uv_param(solidcore::math::Point2::new(*u, *v));
                        o.push(0.0);
                    }
                    OpIn::PcSetGap { j, w, g, .. } => {
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_gap_3d(*g);
                        o.push(0.0);
                    }
                    OpIn::PcSetSss { j, w, p, .. } => {
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_sss_point(v3(*p));
                        o.push(0.0);
                    }
                    OpIn::PcSetPresnap { j, w, v, .. } => {
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_pre_snap_param(*v);
                        o.push(0.0);
                    }
                    OpIn::PcSetSrcTol { j, w, tol, .. } => {
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_src_zone_tol_3d(zone_tol3d(&k, *tol));
                        o.push(0.0);
                    }
                    OpIn::SetCurve { obj, owned, .. } => {
                        cc.set_curve(pool.handle(*obj)?, *owned);
                        o.push(0.0);
                    }
                    OpIn::SetSurface1 { obj, .. } => {
                        cc.set_surface1(pool.handle(*obj)?);
                        o.push(0.0);
                    }
                    OpIn::SetSurface2 { obj, .. } => {
                        cc.set_surface2(pool.handle(*obj)?);
                        o.push(0.0);
                    }
                    OpIn::SetFace { obj, .. } => {
                        cc.set_face(pool.handle(*obj)?);
                        o.push(0.0);
                    }
                    OpIn::SetIntersectData {
                        s1, s2, uv2, current, ti, ..
                    } => {
                        cc.set_intersect_data(
                            pool.handle(*s1)?,
                            pool.handle(*s2)?,
                            pool.handle(*uv2)?,
                            *current,
                            pool.handle(*ti)?,
                        );
                        o.push(0.0);
                    }
                    OpIn::SetIsCurveCurrent { b: x, .. } => {
                        cc.set_is_curve_current(*x);
                        o.push(0.0);
                    }
                    OpIn::SetIgnoreTangency { b: x, .. } => {
                        cc.set_ignore_tangency(*x);
                        o.push(0.0);
                    }
                    OpIn::SetIsCurveOwned { b: x, .. } => {
                        cc.set_is_curve_owned(*x);
                        o.push(0.0);
                    }
                    OpIn::SetOkToSetTol { b: x, .. } => {
                        cc.set_ok_to_set_tolerance(*x);
                        o.push(0.0);
                    }
                    OpIn::SetFilletData { obj, b: x, .. } => {
                        cc.set_fillet_data(pool.handle(*obj)?, *x);
                        o.push(0.0);
                    }
                    OpIn::SetTgtLoopsOnly { b: x, .. } => {
                        cc.set_tgt_loops_only(*x);
                        o.push(0.0);
                    }
                    OpIn::AddTgtLoop { obj, .. } => {
                        let h = pool.handle(*obj)?.ok_or_else(|| harness("no loop"))?;
                        cc.add_tgt_loop(h);
                        o.push(0.0);
                    }
                    OpIn::SetSrcTol { tol, .. } => {
                        cc.set_src_zone_tol_3d(zone_tol3d(&k, *tol));
                        o.push(0.0);
                    }
                    OpIn::SetUvCurve1 { obj, .. } => {
                        cc.set_uv_curve1(pool.handle(*obj)?);
                        o.push(0.0);
                    }
                    OpIn::SetUvCurve2 { obj, .. } => {
                        cc.set_uv_curve2(pool.handle(*obj)?);
                        o.push(0.0);
                    }
                    OpIn::FindInterval { prm, last, tol, .. } => {
                        let r = cc.find_interval(&k, &pool, *prm, *last, zone_tol3d(&k, *tol));
                        match st(r)? {
                            Ok(f) => o.extend([0.0, f.index as f64, f.end_dist_3d, f64::from(f.position.code())]),
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::FindPc { prm, tol, .. } => {
                        let r = cc.find_point_class(&k, &pool, *prm, zone_tol3d(&k, *tol));
                        match st(r)? {
                            Ok(pc) => {
                                o.push(0.0);
                                put_pc(&mut o, &pc);
                            }
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::IsCoincident { a, b: bb, tol, .. } => {
                        let r = cc.is_coincident(&k, &pool, &Extent1d::from_min_max(*a, *bb), zone_tol3d(&k, *tol));
                        match st(r)? {
                            Ok(flag) => o.extend([0.0, b(flag)]),
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::Insert { pc, prm, last, force, .. } => {
                        let pc = build_pc(&k, &pool, pc)?;
                        let r = cc.insert_point_class(&k, &pool, &pc, *prm, *last, *force);
                        match st(r)? {
                            Ok(made) => o.extend([0.0, b(made)]),
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::RemovePc { j, pos, .. } => {
                        let r = cc.remove_point_class(*j, position(*pos)?);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::Merge { index, .. } => {
                        let r = cc.merge_point_class(&k, &pool, *index, None, None);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::NearVertex { index, p, .. } => {
                        let r = change_edge_point_class_to_near_vertex(&k, &pool, *index, cc, v3(*p));
                        match st(r)? {
                            Ok(changed) => o.extend([0.0, b(changed)]),
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::Cleanup { .. } | OpIn::Squeeze { .. } => {}
                }
            }
            // the ops on both classifications (they edit `other` too)
            if matches!(x, OpIn::Cleanup { .. } | OpIn::Squeeze { .. }) {
                let mut a = ccs.get(t).cloned().ok_or_else(|| harness("bad target"))?;
                let mut c = other;
                let r = if matches!(x, OpIn::Cleanup { .. }) {
                    a.cleanup_and_validate(&k, &pool, &mut c)
                } else {
                    fix_problems_by_squeezing(&k, &pool, &mut a, &mut c)
                };
                o.push(st(r)?.err().unwrap_or(0.0));
                put_cc(&mut o, &k, &c);
                if let Some(slot) = ccs.get_mut(t) {
                    *slot = a;
                }
                if let Some(slot) = ccs.get_mut(1 - t) {
                    *slot = c;
                }
            }
            let cc = ccs.get(t).cloned().ok_or_else(|| harness("bad target"))?;
            put_cc(&mut o, &k, &cc);
            Ok(())
        };
        match step() {
            Ok(()) => {}
            Err(Fail::Deferred(d)) => {
                // the classification is not changed by the unported callee: later ops would
                // run on another state than the C++ ones
                deferred = Some(d);
                break;
            }
            Err(Fail::Harness(e)) => {
                return Err(Fail::Harness(harness(format!("op {n_op} ({x:?}): {e}"))))
            }
        }
    }
    match deferred {
        Some(d) => Err(Fail::Deferred(d)),
        None => Ok(o),
    }
}

fn rust(s: &Script) -> Result<Output, HarnessError> {
    match rust_run(s) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -------------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational or 2D).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.1);
    let a = if rng.chance(0.5) { 0.0 } else { rng.uniform(-5.0, 5.0) };
    let len = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.1, 10.0) };
    let mut u = vec![a; p + 1];
    let mut ks: Vec<f64> = (0..n - p).map(|_| a + len * rng.uniform(0.05, 0.95)).collect();
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
            let d0 = rng.uniform(-5.0, 5.0);
            CurveIn::Line(LineIn {
                p: rp(rng, 10.0),
                v: rp(rng, 3.0),
                domain: [d0, d0 + rng.uniform(0.5, 10.0)],
                scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
                inside_out: rng.chance(0.3),
            })
        }
        2..=5 => CurveIn::Bspline(gen_bspline(rng)),
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

/// A B-spline surface patch over `[u0, u0 + ul] x [v0, v0 + vl]` of degrees up to 3,
/// near the plane through `o` spanned by `x`, `y` (size `sz`), bumped by up to `bump`.
fn gen_surface(rng: &mut Rng, o: [f64; 3], x: [f64; 3], y: [f64; 3], sz: f64, bump: f64) -> SurfIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(2) as usize;
    let m = q + rng.below(2) as usize;
    let knots = |rng: &mut Rng, deg: usize, cnt: usize| {
        let a = if rng.chance(0.6) { 0.0 } else { rng.uniform(-2.0, 2.0) };
        let l = if rng.chance(0.6) { 1.0 } else { rng.uniform(0.5, 5.0) };
        let mut k = vec![a; deg + 1];
        let mut inner: Vec<f64> = (0..cnt - deg).map(|_| a + l * rng.uniform(0.1, 0.9)).collect();
        inner.sort_by(f64::total_cmp);
        k.extend(inner);
        k.extend(vec![a + l; deg + 1]);
        k
    };
    let u = knots(rng, p, n);
    let v = knots(rng, q, m);
    let rational = rng.chance(0.2);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let s = sz * (i as f64 / n as f64 - 0.5);
                    let t = sz * (j as f64 / m as f64 - 0.5);
                    let e = rp(rng, bump);
                    let pt = [
                        o[0] + s * x[0] + t * y[0] + e[0],
                        o[1] + s * x[1] + t * y[1] + e[1],
                        o[2] + s * x[2] + t * y[2] + e[2],
                    ];
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [pt[0] * w, pt[1] * w, pt[2] * w, w]
                    } else {
                        [pt[0], pt[1], pt[2], NOW]
                    }
                })
                .collect()
        })
        .collect();
    SurfIn {
        oob: rng.chance(0.2),
        net,
        p,
        q,
        u,
        v,
    }
}

/// Pool indices of objects of kind `k`.
fn of_kind(objs: &[ObjIn], k: usize) -> Vec<usize> {
    objs.iter()
        .enumerate()
        .filter(|(_, o)| obj_kind(o) == k)
        .map(|(i, _)| i)
        .collect()
}

fn pick(rng: &mut Rng, v: &[usize]) -> Option<usize> {
    rng.choose(v).copied()
}

/// A unit-ish random direction.
fn dir(rng: &mut Rng) -> [f64; 3] {
    let mut d = rp(rng, 1.0);
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l < 0.1 {
        d = [1.0, 0.0, 0.0];
    } else {
        d = [d[0] / l, d[1] / l, d[2] / l];
    }
    d
}

fn cross(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

/// The base pool: curves, planes and B-spline surfaces, points, topology and stand-ins.
fn gen_objects(rng: &mut Rng) -> Vec<ObjIn> {
    let mut objs = Vec::new();
    for _ in 0..=rng.below(3) {
        objs.push(ObjIn::Curve(gen_curve(rng)));
    }
    objs.push(ObjIn::Plane { o: rp(rng, 5.0), n: dir(rng) });
    let (x, y) = {
        let d = dir(rng);
        let e = norm(cross(d, dir(rng)));
        (d, e)
    };
    let o = rp(rng, 5.0);
    let sz = rng.uniform(1.0, 20.0);
    let bump = rng.uniform(0.0, 2.0);
    objs.push(ObjIn::Bsurf(gen_surface(rng, o, x, y, sz, bump)));
    objs.push(ObjIn::Point(rp(rng, 10.0)));
    objs.push(ObjIn::Vertex);
    objs.push(ObjIn::Vertex);
    objs.push(ObjIn::Edge(Some(0)));
    objs.push(ObjIn::Face(of_kind(&objs, kind::PLANE).first().copied()));
    objs.push(ObjIn::Loop);
    objs.push(ObjIn::Loop);
    objs.push(ObjIn::Other);
    objs
}

fn gen_tol(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => USE_DEFAULT,
        2 => 1e-3,
        3 => 1e-8,
        4 => 0.0,
        5 => -1e-4,
        6 => rng.uniform(1e-7, 1e-1),
        _ => 1e-5,
    }
}

/// The natural domain and dimension of a curve object.
fn curve_info(o: &ObjIn) -> Option<(f64, f64, u64)> {
    let ObjIn::Curve(c) = o else { return None };
    let dim = match c {
        CurveIn::Bspline(bs) => bs.dim,
        CurveIn::Hermite(h) => h.dim,
        _ => 3,
    };
    let ivl = build_curve(c).ok()?.natural_interval();
    Some((ivl.min(), ivl.max(), dim))
}

/// A parameter offset (of `len` the domain length): zero, at and around the tolerance limits.
fn offset(rng: &mut Rng, len: f64) -> f64 {
    let d = *rng
        .choose(&[
            0.0, 0.0, 1e-14, 1e-13, 1e-12, 1e-11, 1e-9, 1e-8, 1e-7, 1e-6, 3e-6, 1e-5, 3e-5, 1e-4, 1e-3,
        ])
        .unwrap_or(&0.0);
    let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
    sign * d * len
}

/// `n + 1` increasing interval bounds over `[d0, d1]` (some short or zero-length intervals,
/// sometimes ends short of the curve ends).
fn gen_bounds(rng: &mut Rng, d0: f64, d1: f64) -> Vec<f64> {
    let len = d1 - d0;
    let n = 1 + rng.below(5) as usize;
    let mut fr: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
    for f in fr.iter_mut().skip(1).take(n - 1) {
        *f += rng.uniform(-0.35, 0.35) / n as f64;
    }
    let mut bounds: Vec<f64> = fr.iter().map(|f| d0 + len * f).collect();
    if rng.chance(0.15) {
        if let Some(first) = bounds.first_mut() {
            *first += len * rng.uniform(0.0, 0.2);
        }
    }
    if rng.chance(0.15) {
        if let Some(last) = bounds.last_mut() {
            *last -= len * rng.uniform(0.0, 0.2);
        }
    }
    // short and zero-length intervals
    for i in 1..bounds.len() {
        if rng.chance(0.12) {
            let tiny = *rng.choose(&[0.0, 1e-14, 1e-12, 1e-9, 1e-7, 1e-5]).unwrap_or(&0.0);
            let prev = bounds.get(i - 1).copied().unwrap_or(d0);
            if let Some(b) = bounds.get_mut(i) {
                *b = prev + tiny * len;
            }
        }
    }
    for i in 1..bounds.len() {
        let prev = bounds.get(i - 1).copied().unwrap_or(d0);
        if let Some(b) = bounds.get_mut(i) {
            if *b < prev {
                *b = prev;
            }
        }
    }
    bounds
}

/// A parameter near (or far from) the bounds.
fn gen_prm(rng: &mut Rng, bounds: &[f64], d0: f64, d1: f64) -> f64 {
    let len = d1 - d0;
    let first = bounds.first().copied().unwrap_or(d0);
    let last = bounds.last().copied().unwrap_or(d1);
    match rng.below(12) {
        0..=4 => {
            let base = rng.choose(bounds).copied().unwrap_or(first);
            base + offset(rng, len)
        }
        5..=6 => rng.uniform(first, last),
        7 => first - offset(rng, len).abs(),
        8 => last + offset(rng, len).abs(),
        9 => {
            if rng.chance(0.5) {
                first - rng.uniform(0.0, 0.5) * len
            } else {
                last + rng.uniform(0.0, 0.5) * len
            }
        }
        10 => rng.choose(bounds).copied().unwrap_or(first),
        _ => rng.uniform(d0, d1),
    }
}

/// A class / object pair for a point classification; `None` for unknown.
fn gen_class(rng: &mut Rng, objs: &[ObjIn], topo: bool) -> (i32, Option<usize>) {
    let choices: &[(i32, usize)] = if topo {
        &[(102, kind::FACE), (103, kind::EDGE), (104, kind::VERTEX), (106, kind::CURVE)]
    } else {
        &[(106, kind::CURVE), (106, kind::CURVE), (107, kind::PLANE), (107, kind::BSURF)]
    };
    if rng.chance(0.1) {
        return (100, None);
    }
    let &(c, k) = rng.choose(choices).unwrap_or(&(106, kind::CURVE));
    (c, pick(rng, &of_kind(objs, k)))
}

fn gen_pc(rng: &mut Rng, objs: &[ObjIn], prm: f64, topo: bool) -> PcIn {
    let (class, obj) = gen_class(rng, objs, topo);
    PcIn {
        class,
        obj,
        t: prm,
        gap: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&0.0),
        presnap: if rng.chance(0.6) { BIG } else { prm + rng.uniform(-1e-3, 1e-3) },
        tol: gen_tol(rng),
        sss: rng.chance(0.15).then(|| rp(rng, 10.0)),
    }
}

/// The setup ops of classification `t` over `bounds` on pool curve `curve`.
fn setup_cc(rng: &mut Rng, objs: &[ObjIn], t: usize, curve: Option<usize>, bounds: &[f64], topo: bool, ops: &mut Vec<OpIn>) {
    let n = bounds.len().saturating_sub(1);
    ops.push(OpIn::SetSize { t, n });
    ops.push(OpIn::SetCurve { t, obj: curve, owned: false });
    if rng.chance(0.7) {
        ops.push(OpIn::SetSrcTol { t, tol: gen_tol(rng) });
    }
    for j in 0..n {
        ops.push(OpIn::IvlSet {
            t,
            j,
            a: bounds.get(j).copied().unwrap_or(0.0),
            b: bounds.get(j + 1).copied().unwrap_or(1.0),
        });
    }
    // boundary classifications (the end of interval j - 1 and the start of interval j)
    for (bi, &prm) in bounds.iter().enumerate() {
        if !rng.chance(0.6) {
            continue;
        }
        let pc = gen_pc(rng, objs, prm, topo);
        let mut targets = Vec::new();
        if bi < n {
            targets.push((bi, 0));
        }
        if bi > 0 {
            targets.push((bi - 1, 2));
        }
        for (j, w) in targets {
            ops.push(OpIn::PcSetClass { t, j, w, class: pc.class, obj: pc.obj });
            ops.push(OpIn::PcSetGap { t, j, w, g: pc.gap });
            ops.push(OpIn::PcSetPresnap { t, j, w, v: pc.presnap });
            ops.push(OpIn::PcSetT { t, j, w, v: pc.t });
        }
    }
    // interval interiors
    for j in 0..n {
        if rng.chance(0.5) {
            let (class, obj) = match rng.below(4) {
                0 | 1 => (103, pick(rng, &of_kind(objs, kind::EDGE))),
                2 => (106, pick(rng, &of_kind(objs, kind::CURVE))),
                _ => (102, pick(rng, &of_kind(objs, kind::FACE))),
            };
            ops.push(OpIn::PcSetClass { t, j, w: 1, class, obj });
        }
    }
}

fn gen_script(rng: &mut Rng) -> Script {
    let objects = gen_objects(rng);
    let pure = rng.chance(0.5);
    let topo = !pure && rng.chance(0.12);
    // the classification curve: a 3D one most of the time
    let curves: Vec<(usize, (f64, f64, u64))> = objects
        .iter()
        .enumerate()
        .filter_map(|(i, o)| curve_info(o).map(|c| (i, c)))
        .filter(|(_, (a, b, _))| b - a > 1e-3)
        .collect();
    let three_d: Vec<(usize, (f64, f64, u64))> = curves.iter().filter(|(_, c)| c.2 == 3).copied().collect();
    let pool = if rng.chance(0.9) && !three_d.is_empty() { &three_d } else { &curves };
    let Some(&(ci, (d0, d1, dim))) = rng.choose(pool) else {
        return Script { objects, ops: Vec::new() };
    };
    let mut ops = Vec::new();
    let mut set_up = [false; 2];
    let mut all_bounds: Vec<Vec<f64>> = vec![Vec::new(), Vec::new()];
    let b0 = gen_bounds(rng, d0, d1);
    // the second classification: the same curve, bounds like the first or fresh
    let b1 = if rng.chance(0.5) {
        b0.iter().map(|x| x + offset(rng, d1 - d0)).collect()
    } else {
        gen_bounds(rng, d0, d1)
    };
    for (t, bounds) in [b0, b1].into_iter().enumerate() {
        if t == 1 && !rng.chance(0.6) {
            continue;
        }
        let curve = if rng.chance(0.04) { None } else { Some(ci) };
        setup_cc(rng, &objects, t, curve, &bounds, topo, &mut ops);
        if dim == 2 && rng.chance(0.5) {
            let surfs: Vec<usize> = of_kind(&objects, kind::PLANE)
                .into_iter()
                .chain(of_kind(&objects, kind::BSURF))
                .collect();
            ops.push(OpIn::SetSurface1 { t, obj: pick(rng, &surfs) });
        }
        if rng.chance(0.1) {
            ops.push(OpIn::SetIgnoreTangency { t, b: rng.chance(0.5) });
        }
        if let Some(slot) = set_up.get_mut(t) {
            *slot = true;
        }
        if let Some(slot) = all_bounds.get_mut(t) {
            *slot = bounds;
        }
    }
    let targets: Vec<usize> = (0..2).filter(|&t| set_up.get(t).copied().unwrap_or(false)).collect();
    let n_ops = 2 + rng.below(6);
    for _ in 0..n_ops {
        let Some(&t) = rng.choose(&targets) else { break };
        let bounds = all_bounds.get(t).cloned().unwrap_or_default();
        let size = bounds.len().saturating_sub(1);
        let prm = gen_prm(rng, &bounds, d0, d1);
        let last = if rng.chance(0.7) { BIG } else { gen_prm(rng, &bounds, d0, d1) };
        let tol = gen_tol(rng);
        // half of the scripts stay on the ops that can complete without the unported callees
        let roll = if pure { rng.below(80) } else { rng.below(100) };
        ops.push(match roll {
            0..=21 => OpIn::FindInterval { t, prm, last, tol },
            22..=31 => OpIn::FindPc { t, prm, tol },
            32..=43 => OpIn::IsCoincident {
                t,
                a: prm,
                b: gen_prm(rng, &bounds, d0, d1),
                tol,
            },
            44..=69 => OpIn::Insert {
                t,
                pc: gen_pc(rng, &objects, prm, topo),
                prm,
                last,
                force: !pure && rng.chance(0.1),
            },
            // the classification at an end of the curve (no neighbour) or anywhere
            70..=79 => {
                let at_end = pure || rng.chance(0.5);
                let (j, pos) = match (at_end, rng.chance(0.5)) {
                    (true, true) => (0, 1),
                    (true, false) => (size.saturating_sub(1), 3),
                    _ => (
                        rng.below(size as u64 + 2) as usize,
                        *rng.choose(&[1, 3, 1, 3, 2, 0, 4]).unwrap_or(&1),
                    ),
                };
                OpIn::RemovePc { t, j, pos }
            }
            80..=82 => OpIn::Merge { t, index: rng.below(size as u64 + 2) as usize },
            83..=85 => OpIn::NearVertex { t, index: rng.below(size as u64 + 2) as usize, p: rp(rng, 10.0) },
            86..=89 => OpIn::Squeeze { t },
            90..=91 => OpIn::Cleanup { t },
            92..=93 => OpIn::SetSize { t, n: rng.below(4) as usize },
            94..=95 => OpIn::IvlSet { t, j: rng.below(size as u64 + 1) as usize, a: prm, b: prm + rng.uniform(0.0, 1.0) * (d1 - d0) },
            96..=97 => OpIn::PcSetSrcTol { t, j: rng.below(size.max(1) as u64) as usize, w: rng.below(3) as usize, tol: gen_tol(rng) },
            _ => OpIn::SetSrcTol { t, tol: gen_tol(rng) },
        });
    }
    Script { objects, ops }
}

/// Whether the shim accepts the script (it rejects what is undefined upstream).
fn shim_accepts(s: &Script) -> bool {
    match ffi::run(&cpp_input(s)) {
        Ok(_) => true,
        Err(solidcore_ffi::FfiError::Status(c)) if c == SC_ERR_ARG => false,
        Err(_) => false,
    }
}

// ---- fixtures built in code ---------------------------------------------------------------

/// The pool of the edge cases: a line over `[0, 1]` of the given speed (0), a plane (1), an
/// edge on the line (2), a face (3), a vertex (4), a point (5), a circle (6).
fn edge_objects(scale: f64) -> Vec<ObjIn> {
    vec![
        ObjIn::Curve(CurveIn::Line(LineIn {
            p: [0.0, 0.0, 0.0],
            v: [scale, 0.0, 0.0],
            domain: [0.0, 1.0],
            scale: 1.0,
            inside_out: false,
        })),
        ObjIn::Plane { o: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] },
        ObjIn::Edge(Some(0)),
        ObjIn::Face(Some(1)),
        ObjIn::Vertex,
        ObjIn::Point([scale, scale, 0.0]),
        ObjIn::Curve(CurveIn::Conic(ConicIn {
            center: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            domain: [0.0, 360.0],
            rx: scale,
            ry: None,
            inside_out: false,
        })),
    ]
}

/// A classification of `bounds` on pool curve `curve` with the given tolerance.
fn edge_setup(t: usize, curve: usize, bounds: &[f64], tol: f64) -> Vec<OpIn> {
    let mut ops = vec![
        OpIn::SetSize { t, n: bounds.len() - 1 },
        OpIn::SetCurve { t, obj: Some(curve), owned: false },
        OpIn::SetSrcTol { t, tol },
    ];
    for (j, w) in bounds.windows(2).enumerate() {
        if let [a, b] = *w {
            ops.push(OpIn::IvlSet { t, j, a, b });
        }
    }
    ops
}

/// Edge cases: boundary and tolerance-limit parameters against every position, snapping from
/// off the curve ends, short and closed intervals, merges against every boundary class,
/// removal at the ends, coincidence over edge-classified interiors, empty-neighbour squeezes.
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let bounds = [0.0, 0.25, 0.75, 1.0];
    for (name, scale) in [("unit", 1.0), ("huge", 1e5), ("tiny", 1e-4)] {
        let objs = edge_objects(scale);
        let mut ops = edge_setup(0, 0, &bounds, 1e-5 * scale);
        // boundary classifications: curve at the start, surface inside, edge-less end
        ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 0, class: 106, obj: Some(0) });
        ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 2, class: 107, obj: Some(1) });
        ops.push(OpIn::PcSetClass { t: 0, j: 1, w: 0, class: 107, obj: Some(1) });
        ops.push(OpIn::PcSetClass { t: 0, j: 1, w: 1, class: 103, obj: Some(2) });
        ops.push(OpIn::PcSetClass { t: 0, j: 2, w: 1, class: 103, obj: Some(2) });
        ops.push(OpIn::PcSetGap { t: 0, j: 0, w: 2, g: 1e-7 });
        for tol in [USE_DEFAULT, 1e-5, 1e-8, 0.0, 1e-2] {
            for prm in [
                0.0, -1e-13, 1e-13, -1e-9, 1e-9, -1e-6, -1e-4, -0.5, 0.25, 0.25 - 1e-9, 0.25 + 1e-9,
                0.25 - 1e-6, 0.25 + 1e-6, 0.25 + 1e-4, 0.5, 0.75, 0.75 - 1e-7, 0.75 + 1e-7, 1.0 - 1e-13,
                1.0, 1.0 + 1e-13, 1.0 + 1e-9, 1.0 + 1e-6, 1.0 + 1e-4, 1.5, 100.0,
            ] {
                ops.push(OpIn::FindInterval { t: 0, prm, last: BIG, tol });
            }
        }
        // the previous-parameter rule: the last parameter closer to the start than this one
        for (prm, last) in [(1e-6, 0.0), (1e-6, 1e-7), (1e-6, 1e-5), (0.25 + 1e-6, 0.25), (0.25 + 1e-6, 0.25 + 1e-7)] {
            ops.push(OpIn::FindInterval { t: 0, prm, last, tol: 1e-5 });
        }
        for prm in [0.0, 0.3, 0.9, 1.0, 1.2] {
            ops.push(OpIn::FindPc { t: 0, prm, tol: USE_DEFAULT });
        }
        for (a, b) in [(0.0, 1.0), (0.3, 0.7), (0.25, 0.75), (0.1, 0.9), (0.75, 1.0), (0.0, 0.25), (0.5, 0.5)] {
            ops.push(OpIn::IsCoincident { t: 0, a, b, tol: 1e-5 });
        }
        out.push((format!("edge_find_{name}"), json!(Script { objects: objs, ops })));
    }
    // short, zero-length and closed intervals
    let objs = edge_objects(1.0);
    for (name, curve, b) in [
        ("short", 0, vec![0.0, 0.5, 0.5 + 1e-7, 1.0]),
        ("zero", 0, vec![0.0, 0.5, 0.5, 1.0]),
        ("closed", 6, vec![0.0, 180.0, 360.0]),
        ("closed_short", 6, vec![0.0, 1e-4, 360.0 - 1e-4, 360.0]),
    ] {
        let mut ops = edge_setup(0, curve, &b, 1e-5);
        let ps: Vec<f64> = b.iter().flat_map(|&x| [x, x + 1e-9, x - 1e-9, x + 1e-5, x - 1e-5, x + 1e-3]).collect();
        for prm in ps {
            ops.push(OpIn::FindInterval { t: 0, prm, last: BIG, tol: USE_DEFAULT });
            ops.push(OpIn::FindInterval { t: 0, prm, last: b.first().copied().unwrap_or(0.0), tol: 1e-3 });
        }
        out.push((format!("edge_find_{name}"), json!(Script { objects: objs.clone(), ops })));
    }
    // merges: every boundary class against every new class, at and next to the boundary (one
    // script per insertion: an insertion that needs InsertAt defers its script)
    let objs = edge_objects(1.0);
    let classes = [(106, Some(0)), (106, Some(6)), (107, Some(1)), (100, None)];
    for (name, old) in [("unknown", (100, None)), ("curve", (106, Some(0))), ("surface", (107, Some(1))), ("other_curve", (106, Some(6)))] {
        for (ci, &(class, obj)) in classes.iter().enumerate() {
            for (pi, prm) in [0.0, 0.25, 0.25 + 1e-7, 0.25 - 1e-7, 0.75, 1.0, 1.0 + 1e-6, 0.5].into_iter().enumerate() {
                for (gi, (gap, presnap)) in [(0.0, BIG), (1e-3, 0.3)].into_iter().enumerate() {
                    let mut ops = edge_setup(0, 0, &bounds, 1e-5);
                    for (j, w) in [(0, 0), (0, 2), (1, 0), (1, 2), (2, 0), (2, 2)] {
                        ops.push(OpIn::PcSetClass { t: 0, j, w, class: old.0, obj: old.1 });
                    }
                    ops.push(OpIn::Insert {
                        t: 0,
                        pc: PcIn { class, obj, t: prm, gap, presnap, tol: USE_DEFAULT, sss: None },
                        prm,
                        last: BIG,
                        force: false,
                    });
                    out.push((format!("edge_insert_{name}_{ci}_{pi}_{gi}"), json!(Script { objects: objs.clone(), ops })));
                }
            }
        }
    }
    // mid conflicts and the unknown-boundary tolerance rules
    for (i, (prm, class, obj)) in [(0.5, 106, Some(0)), (0.5, 106, Some(6)), (0.5, 107, Some(1)), (0.5, 100, None), (0.2, 106, Some(6)), (1.0 + 1e-3, 106, Some(0)), (0.26, 106, Some(6))]
        .into_iter()
        .enumerate()
    {
        let mut ops = edge_setup(0, 0, &bounds, 1e-5);
        ops.push(OpIn::PcSetClass { t: 0, j: 1, w: 1, class: 106, obj: Some(0) });
        ops.push(OpIn::Insert {
            t: 0,
            pc: PcIn { class, obj, t: prm, gap: 0.0, presnap: BIG, tol: USE_DEFAULT, sss: None },
            prm,
            last: BIG,
            force: i % 2 == 1,
        });
        out.push((format!("edge_insert_mid_conflict_{i}"), json!(Script { objects: objs.clone(), ops })));
    }
    // removal: ends, interior, invalid positions and indices
    for (i, (j, pos)) in [(0, 1), (2, 3), (0, 3), (2, 1), (1, 2), (1, 0), (1, 4), (3, 1), (9, 3)].into_iter().enumerate() {
        let mut ops = edge_setup(0, 0, &bounds, 1e-5);
        ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 0, class: 106, obj: Some(0) });
        ops.push(OpIn::PcSetClass { t: 0, j: 2, w: 2, class: 107, obj: Some(1) });
        ops.push(OpIn::RemovePc { t: 0, j, pos });
        out.push((format!("edge_remove_{i}"), json!(Script { objects: objs.clone(), ops })));
    }
    // squeeze and cleanup of two classifications of one curve
    let others: [&[f64]; 6] = [
        &bounds,
        &[0.0, 0.25, 0.5, 0.75, 1.0],
        &[0.0, 1.0],
        &[0.0, 0.3, 0.6, 1.0],
        &[0.0, 0.25, 0.25, 0.75, 1.0],
        &[0.0, 0.25 + 1e-9, 0.75, 1.0],
    ];
    for (i, other) in others.into_iter().enumerate() {
        for (name, first) in [("same", &bounds[..]), ("zero", &[0.0, 0.25, 0.25, 0.75, 1.0][..])] {
            let mut ops = edge_setup(0, 0, first, 1e-5);
            ops.extend(edge_setup(1, 0, other, 1e-5));
            ops.push(OpIn::Squeeze { t: 0 });
            out.push((format!("edge_squeeze_{name}_{i}"), json!(Script { objects: objs.clone(), ops })));
        }
    }
    let mut ops = edge_setup(0, 0, &bounds, 1e-5);
    ops.push(OpIn::SetSize { t: 1, n: 0 });
    ops.push(OpIn::SetSize { t: 0, n: 0 });
    ops.push(OpIn::Cleanup { t: 0 });
    out.push(("edge_cleanup_empty".into(), json!(Script { objects: objs, ops })));
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let s = gen_script(rng);
            if ffi::validate(&cpp_input(&s)).is_err() || !shim_accepts(&s) {
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
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
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

    /// Generated scripts are accepted by the ffi checks, and the share of comparable ones (not
    /// deferred) is reported.
    #[test]
    fn generated_scripts_are_mostly_comparable() {
        let mut rng = Rng::new(11);
        let (mut total, mut deferred) = (0, 0);
        for _ in 0..300 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
        }
        assert!(total > 250, "{total}");
        assert!(deferred * 5 < total * 4, "{deferred} of {total} deferred");
    }

    /// The edge cases are accepted by the shim.
    #[test]
    fn edge_cases_are_accepted() {
        for (name, v) in edge_cases() {
            let s: Script = serde_json::from_value(v).unwrap();
            assert!(ffi::validate(&cpp_input(&s)).is_ok(), "{name}");
            assert!(shim_accepts(&s), "{name}");
        }
    }
}
