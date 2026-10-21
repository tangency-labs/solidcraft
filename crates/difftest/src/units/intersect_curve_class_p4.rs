//! Runner for unit `intersect.curve_class_p4`: the full `SmCurveClassification` constructor,
//! `GetLoopCrossings`, `SmCurveInterval::GetIvlBound_EdgeuseObj` and the static
//! `sm_SqueezeOutSmallestInterval` of `SmCurveClass.cpp`.
//!
//! C++: `solidcore_ffi::units::intersect_curve_class_p4` (upstream
//! `source/SMLib/src/SmCurveClass.cpp`). Rust: `solidcore::intersect::curve_class::p4`.
//!
//! A case is a script: a pool of `objects` (curves as in unit `curve.curve_p6` plus real
//! `SmEllipse` / `SmCircle`, planes, B-spline surfaces, points and "other" stand-ins), an
//! optional `face` (a one-face B-rep built with `SmBrepConstructor` as in unit
//! `heal.face_props`: a plane or `SmSurfOfRevolution::CreateCanonical` surface, cylinder or
//! sphere among them, with a vertex loop or edge loops of line edges; rebuilt record by record
//! on the Rust side by `heal_face_props::rust_brep`) whose face, loops, edges, vertices and edge
//! uses join the pool after the objects, and `ops` on two default classifications `cc[0]`,
//! `cc[1]`: the setters of unit `intersect.curve_class_p1` build them (intervals, boundary
//! classifications on the face's edges and vertices, the classification curve, face and
//! tolerances), then the part-4 ops run. The topology handles resolve to the B-rep through the
//! `ClassGeometry` hooks, so the topology members are the ported ones. Both sides produce the
//! flat record documented in the ffi module (per op: code, status, outputs, state of the
//! classification; `GetLoopCrossings` also the face props snapshot when they are passed),
//! reported as the `out` values. A Rust error maps to its `SmStatus` (`InvalidInput` 1007, `-1`
//! for one upstream cannot return); an `Unported` error anywhere defers the case.
//!
//! Compared: `GetLoopCrossings` over the face's real loops up to the touch list (the face
//! props stage 2, computed by `SetProps_Stage2` or preset by seeds at stage `2a` or later, the
//! face's surface and zone tolerance, `IsClosed` / `IsDegenerate` of the classification curve
//! and the return without crossings of a degenerate one), and with an empty loop list;
//! `GetIvlBound_EdgeuseObj` on the real face for interval ends that are not on an edge (the
//! `NULL` return after the face's upward face use and surface). Deferred: every touch list
//! (`GetPointClassifiesConnectedTo`, `intersect.curve_class_p7`, after the seam sampling of a
//! closed face), `SetProps_Stage2` of a closed face (`SmFace::HasSeamProblem`,
//! `topology.face_p4`), the edge ends of `GetIvlBound_EdgeuseObj` (`SmEdge::GetCurve` in
//! `PointClassification::curve_object`, then `GetMinUV`, `intersect.curve_class_p2`), the
//! constructor (`ReSet`, `intersect.curve_class_p2`) and every squeeze of a non-empty
//! classification (`ConnectIntervals`, `intersect.curve_class_p2`). Generated scripts are
//! filtered by the shim's run-time checks (scripts it rejects as undefined upstream are
//! dropped).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::{
    self as cc_mod, squeeze_out_smallest_interval, ClassificationInit, CurveClassification,
    CurveInterval, TouchData,
};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::heal::face_props::{FaceProps, LoopObject};
use solidcore::kernel::Kernel;
use solidcore::topology::{Brep, EdgeId, FaceId, LoopId};
use solidcore::EntityRef;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_curve_class_p4::{self as ffi, kind, op};

use crate::case::{CaseContext, CaseSource};
use crate::units::heal_face_props::{self as hfp, FaceIn, GenIn, LoopsIn, SurfIn as FaceSurfIn};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `intersect.curve_class_p4` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;
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
    Squeeze { t: usize },
    IvlBound { t: usize, j: usize, do_min: bool },
    LoopCrossings {
        t: usize,
        loops: Vec<usize>,
        with_props: bool,
        /// Face props seeds `[field, idx, value]` (as the `heal.face_props` runner).
        #[serde(default)]
        seeds: Vec<[f64; 3]>,
    },
    Construct {
        t: usize,
        curve: Option<usize>,
        a: f64,
        b: f64,
        uv: Option<usize>,
        tol: f64,
        data_size: usize,
        owned: bool,
        current: bool,
        s1: Option<usize>,
        s2: Option<usize>,
        uv2: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Script {
    /// The one-face B-rep whose topology follows the objects in the pool.
    #[serde(default)]
    face: Option<FaceIn>,
    objects: Vec<ObjIn>,
    ops: Vec<OpIn>,
}

// ---- objects ----------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

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
            OpIn::Squeeze { .. } => {}
            OpIn::IvlBound { j, do_min, .. } => v.extend([*j as f64, b(*do_min)]),
            OpIn::LoopCrossings {
                loops, with_props, seeds, ..
            } => {
                v.push(loops.len() as f64);
                v.extend(loops.iter().map(|&l| l as f64));
                v.push(b(*with_props));
                v.push(seeds.len() as f64);
                for sd in seeds {
                    v.extend_from_slice(sd);
                }
            }
            OpIn::Construct {
                curve, a, b: bb, uv, tol, data_size, owned, current, s1, s2, uv2, ..
            } => v.extend([
                opt_id(*curve),
                *a,
                *bb,
                opt_id(*uv),
                *tol,
                *data_size as f64,
                b(*owned),
                b(*current),
                opt_id(*s1),
                opt_id(*s2),
                opt_id(*uv2),
            ]),
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
        OpIn::Squeeze { t } => (op::SQUEEZE, t),
        OpIn::IvlBound { t, .. } => (op::IVL_BOUND, t),
        OpIn::LoopCrossings { t, .. } => (op::LOOP_CROSSINGS, t),
        OpIn::Construct { t, .. } => (op::CONSTRUCT, t),
    }
}

/// The ffi face of the script.
fn ffi_face(s: &Script) -> Option<solidcore_ffi::units::heal_face_props::FaceSpec> {
    s.face.as_ref().map(hfp::ffi_face)
}

fn cpp(s: &Script) -> Result<Output, HarnessError> {
    let input = cpp_input(s);
    let out = ffi::run(ffi_face(s).as_ref(), &input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side --------------------------------------------------------------------------

/// A pool handle: a script object (index and kind) or an entity of the face's B-rep.
#[derive(Debug, Clone, Copy, PartialEq)]
enum H {
    Obj(usize, ClassObjectKind),
    Topo(EntityRef),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            H::Obj(_, k) => *k,
            H::Topo(e) => match e {
                EntityRef::Face(_) => ClassObjectKind::Face,
                EntityRef::Loop(_) => ClassObjectKind::Loop,
                EntityRef::Edge(_) => ClassObjectKind::Edge,
                EntityRef::Vertex(_) => ClassObjectKind::Vertex,
                EntityRef::Faceuse(_) => ClassObjectKind::Faceuse,
                EntityRef::Loopuse(_) => ClassObjectKind::Loopuse,
                EntityRef::Edgeuse(_) => ClassObjectKind::Edgeuse,
                EntityRef::Vertexuse(_) => ClassObjectKind::Vertexuse,
                EntityRef::Region(_) => ClassObjectKind::Region,
                _ => ClassObjectKind::Other,
            },
        }
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        H::Topo(EntityRef::Loop(l))
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
    /// Every pool handle: the objects, then the B-rep's topology (as the shim numbers it).
    handles: Vec<H>,
    /// The face's B-rep and face.
    brep: Option<(Brep, FaceId)>,
}

impl ClassGeometry<H> for Pool {
    fn curve(&self, o: H) -> Option<&Curve> {
        match o {
            H::Obj(i, _) => match self.objs.get(i) {
                Some(PoolObj::Curve(c)) => Some(c),
                _ => None,
            },
            H::Topo(_) => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match o {
            H::Obj(i, _) => match self.objs.get(i) {
                Some(PoolObj::Surface(s)) => Some(s),
                _ => None,
            },
            H::Topo(_) => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, o: H) -> Option<Vec3> {
        match o {
            H::Obj(i, _) => match self.objs.get(i) {
                Some(PoolObj::Point(p)) => Some(*p),
                _ => None,
            },
            H::Topo(_) => None,
        }
    }
    fn classification_vertex_point(&self, o: H) -> Option<Vec3> {
        match (o, &self.brep) {
            (H::Topo(EntityRef::Vertex(v)), Some((b, _))) => b.vertex(v).ok().map(|r| r.point),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        match (o, &self.brep) {
            (H::Topo(EntityRef::Vertex(v)), Some((b, _))) => b.vertex(v).ok().map(|r| r.tolerance),
            (H::Topo(EntityRef::Edge(e)), Some((b, _))) => b.edge(e).ok().map(|r| r.tolerance),
            (H::Topo(EntityRef::Face(f)), Some((b, _))) => b.face(f).ok().map(|r| r.tolerance),
            _ => None,
        }
    }
    fn topology_entity(&self, o: H) -> Option<(&Brep, EntityRef)> {
        match (o, &self.brep) {
            (H::Topo(e), Some((b, _))) => Some((b, e)),
            _ => None,
        }
    }
    fn topology_handle(&self, e: EntityRef) -> Option<H> {
        self.brep.as_ref().map(|_| H::Topo(e))
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

    /// The pool id of a handle (`-1` none, `-2` an object outside the pool, as the shim).
    fn id(&self, h: Option<H>) -> f64 {
        match h {
            None => -1.0,
            Some(h) => self
                .handles
                .iter()
                .position(|x| *x == h)
                .map_or(-2.0, |i| i as f64),
        }
    }

    /// The face of the B-rep (snapshots name it; without one, an id no props refer to).
    fn face_id(&self) -> FaceId {
        self.brep
            .as_ref()
            .map_or(FaceId::from_index(u32::MAX), |(_, f)| *f)
    }
}

fn class_kind_of(k: usize) -> ClassObjectKind {
    match k {
        kind::CURVE => ClassObjectKind::Curve,
        kind::PLANE | kind::BSURF => ClassObjectKind::Surface,
        kind::POINT => ClassObjectKind::Point,
        _ => ClassObjectKind::Other,
    }
}

fn obj_kind(o: &ObjIn) -> usize {
    match o {
        ObjIn::Curve(_) => kind::CURVE,
        ObjIn::Plane { .. } => kind::PLANE,
        ObjIn::Bsurf(_) => kind::BSURF,
        ObjIn::Point(_) => kind::POINT,
        ObjIn::Other => kind::OTHER,
    }
}

/// The topology of the face's B-rep in pool order (the shim's): the face, then per loop the
/// loop and its vertex (a vertex loop) or, per edge use, its edge, vertex and the edge use.
fn topology_entities(b: &Brep, f: FaceId) -> Result<Vec<(EntityRef, usize)>, HarnessError> {
    let e = |e: SolidError| harness(e.to_string());
    let mut out = vec![(EntityRef::Face(f), kind::FACE)];
    for &l in &b.face(f).map_err(e)?.loops {
        out.push((EntityRef::Loop(l), kind::LOOP));
        let lu = b.loop_(l).map_err(e)?.loopuse.ok_or_else(|| harness("loop use"))?;
        match b.loopuse(lu).map_err(e)?.start {
            solidcore::topology::LoopuseStart::Vertexuse(Some(vu)) => {
                let v = b.vertexuse(vu).map_err(e)?.vertex.ok_or_else(|| harness("vertex"))?;
                out.push((EntityRef::Vertex(v), kind::VERTEX));
            }
            solidcore::topology::LoopuseStart::Edgeuse(Some(eu0)) => {
                let mut eu = eu0;
                loop {
                    let r = b.edgeuse(eu).map_err(e)?;
                    let edge = r.edge.ok_or_else(|| harness("edge"))?;
                    let vu = r.vertexuse.ok_or_else(|| harness("vertex use"))?;
                    let v = b.vertexuse(vu).map_err(e)?.vertex.ok_or_else(|| harness("vertex"))?;
                    out.push((EntityRef::Edge(edge), kind::EDGE));
                    out.push((EntityRef::Vertex(v), kind::VERTEX));
                    out.push((EntityRef::Edgeuse(eu), kind::EDGEUSE));
                    eu = r.ccw.ok_or_else(|| harness("ccw edge use"))?;
                    if eu == eu0 {
                        break;
                    }
                }
            }
            _ => return Err(harness("loop use start")),
        }
    }
    Ok(out)
}

fn build_pool(k: &Kernel, s: &Script) -> Result<Pool, HarnessError> {
    let mut pool = Pool {
        objs: Vec::new(),
        handles: Vec::new(),
        brep: None,
    };
    for (i, o) in s.objects.iter().enumerate() {
        let obj = match o {
            ObjIn::Curve(c) => PoolObj::Curve(build_curve(c)?),
            ObjIn::Plane { o, n } => {
                PoolObj::Surface(Surface::from(Plane::from_normal(v3(*o), v3(*n))))
            }
            ObjIn::Bsurf(s) => PoolObj::Surface(build_surface(s)?),
            ObjIn::Point(p) => PoolObj::Point(v3(*p)),
            ObjIn::Other => PoolObj::Bare,
        };
        pool.objs.push(obj);
        pool.handles.push(H::Obj(i, class_kind_of(obj_kind(o))));
    }
    if let Some(face) = &s.face {
        let (mut b, f) = hfp::rust_brep(k, face)?;
        // the edge lines with their NURB made, as the shim makes them
        for i in 0..b.edges().len() {
            let id = EdgeId::from_index(u32::try_from(i).map_err(|_| harness("edge index"))?);
            let edge = b.edge_mut(id).map_err(|e| harness(e.to_string()))?;
            if let Some(c) = edge.curve.as_mut() {
                if let CurveKind::Line(l) = c.kind_mut() {
                    l.make_nurb().map_err(|e| harness(format!("line nurb: {e}")))?;
                }
            }
        }
        for (e, _) in topology_entities(&b, f)? {
            pool.handles.push(H::Topo(e));
        }
        pool.brep = Some((b, f));
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

fn put_pc(o: &mut Vec<f64>, pool: &Pool, pc: &PointClassification<H>) {
    o.push(f64::from(pc.point_class().code()));
    o.push(pool.id(pc.object()));
    o.push(pool.id(pc.face()));
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

fn put_cc(o: &mut Vec<f64>, k: &Kernel, pool: &Pool, cc: &CurveClassification<H>) {
    let id = |h: Option<H>| pool.id(h);
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
    o.extend(cc.tgt_loops().iter().map(|h| pool.id(Some(*h))));
    for ivl in cc.intervals() {
        o.extend([ivl.min(), ivl.max()]);
        put_pc(o, pool, &ivl.start);
        put_pc(o, pool, &ivl.mid);
        put_pc(o, pool, &ivl.end);
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

#[allow(clippy::too_many_lines)]
fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let pool = build_pool(&k, s)?;
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
                    OpIn::Squeeze { .. } => {
                        let r = squeeze_out_smallest_interval(cc);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::IvlBound { j, do_min, .. } => {
                        let r = cc.ivl_bound_edgeuse_obj(&pool, *j, *do_min);
                        match st(r)? {
                            Ok(eu) => o.extend([0.0, pool.id(eu)]),
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::LoopCrossings {
                        loops, with_props, seeds, ..
                    } => {
                        let hs = loops
                            .iter()
                            .map(|&l| pool.handle(Some(l))?.ok_or_else(|| harness("no loop")))
                            .collect::<Result<Vec<H>, HarnessError>>()?;
                        let mut props = FaceProps::<H>::new();
                        if !seeds.is_empty() {
                            let face = pool.face_id();
                            for sd in seeds {
                                hfp::seed(&mut props, face, sd[0] as i64, sd[1] as i64, sd[2]);
                            }
                        }
                        let r = cc.get_loop_crossings(&k, &pool, &hs, with_props.then_some(&mut props));
                        match st(r)? {
                            Ok(lc) => {
                                o.push(if lc.sm_err { 1.0 } else { 0.0 });
                                o.push(lc.crossing_counts.len() as f64);
                                o.extend(lc.crossing_counts.iter().map(|&c| c as f64));
                                put_touches(&mut o, &pool, &lc.raw_touches);
                                put_touches(&mut o, &pool, &lc.done_touches);
                                if *with_props {
                                    o.extend(hfp::snapshot(&props, pool.face_id()));
                                }
                            }
                            Err(s) => o.push(s),
                        }
                    }
                    OpIn::Construct {
                        curve, a, b: bb, uv, tol, data_size, owned, current, s1, s2, uv2, ..
                    } => {
                        let init = ClassificationInit {
                            curve: pool.handle(*curve)?,
                            interval: Extent1d::from_min_max(*a, *bb),
                            uv_curve: pool.handle(*uv)?,
                            src_zone_tol_3d: zone_tol3d(&k, *tol),
                            opt_data_size: *data_size,
                            is_curve_owned: *owned,
                            is_curve_current: *current,
                            surface1: pool.handle(*s1)?,
                            surface2: pool.handle(*s2)?,
                            uv_curve2: pool.handle(*uv2)?,
                            ti: None,
                        };
                        let r = CurveClassification::with_curve(&k, &init);
                        match st(r)? {
                            Ok(new_cc) => {
                                *cc = new_cc;
                                o.push(0.0);
                            }
                            Err(s) => o.push(s),
                        }
                    }
                }
            }
            let cc = ccs.get(t).cloned().ok_or_else(|| harness("bad target"))?;
            put_cc(&mut o, &k, &pool, &cc);
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

/// The kinds of the topology of `face`'s B-rep in pool order (see [`topology_entities`]).
fn topology_kinds(face: &FaceIn) -> Vec<usize> {
    let mut v = vec![kind::FACE];
    match &face.loops {
        None | Some(LoopsIn::Vertex { .. }) => v.extend([kind::LOOP, kind::VERTEX]),
        Some(LoopsIn::Edges { loops }) => {
            for l in loops {
                v.push(kind::LOOP);
                for _ in l {
                    v.extend([kind::EDGE, kind::VERTEX, kind::EDGEUSE]);
                }
            }
        }
    }
    v
}

/// The kind of every pool object: the script objects, then the face's topology.
fn pool_kinds(objects: &[ObjIn], face: Option<&FaceIn>) -> Vec<usize> {
    let mut v: Vec<usize> = objects.iter().map(obj_kind).collect();
    if let Some(f) = face {
        v.extend(topology_kinds(f));
    }
    v
}

/// Pool indices of objects of kind `k`.
fn of_kind(kinds: &[usize], k: usize) -> Vec<usize> {
    kinds
        .iter()
        .enumerate()
        .filter(|(_, &x)| x == k)
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

/// A line of length `len` (`SmLine::IsDegenerate`: its scale times its domain length) over the
/// domain `[0, 0.01]` along the unit direction `d` (a degenerate classification curve when `len`
/// is below the face's intersection tolerance).
fn short_line(p: [f64; 3], d: [f64; 3], len: f64) -> CurveIn {
    CurveIn::Line(LineIn {
        p,
        v: d,
        domain: [0.0, 0.01],
        scale: len / 0.01,
        inside_out: false,
    })
}

/// The base pool: curves (sometimes a short, degenerate line), planes and B-spline surfaces,
/// points and an "other" stand-in.
fn gen_objects(rng: &mut Rng) -> Vec<ObjIn> {
    let mut objs = Vec::new();
    for _ in 0..=rng.below(3) {
        objs.push(ObjIn::Curve(gen_curve(rng)));
    }
    if rng.chance(0.6) {
        let len = *rng.choose(&[2e-8, 1e-7, 1e-6, 2e-6, 1e-5, 1e-4]).unwrap_or(&1e-6);
        let p = rp(rng, 1.0);
        let d = dir(rng);
        objs.push(ObjIn::Curve(short_line(p, d, len)));
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
    objs.push(ObjIn::Other);
    objs
}

/// A cylinder (`GenIn::Line` parallel to the z axis at radius `r`) or a sphere (a half
/// circle of radius `r` in the xz plane) about the z axis, swept by `sweep` degrees.
fn revolution(sphere: bool, r: f64, z0: f64, z1: f64, sweep: f64) -> FaceSurfIn {
    let gen = if sphere {
        GenIn::Circle {
            c: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 0.0, 1.0],
            d: [-90.0, 90.0],
            r,
        }
    } else {
        GenIn::Line {
            a: [r, 0.0, z0],
            b: [r, 0.0, z1],
        }
    };
    FaceSurfIn::Revolution {
        gen,
        point: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        swap: false,
        sweep,
    }
}

/// A point at angle `a` (degrees) about the z axis, radius `r`, height `z`.
fn polar(r: f64, a: f64, z: f64) -> [f64; 3] {
    let t = a.to_radians();
    [r * t.cos(), r * t.sin(), z]
}

/// The face of a generated script: a plane (the z = 0 plane), a cylinder or a sphere about
/// the z axis (closed or swept below 360 degrees), with a vertex loop or edge loops whose
/// corners lie on the surface; sometimes a face of the `heal.face_props` generator.
fn gen_face(rng: &mut Rng) -> FaceIn {
    let tol = *rng.choose(&[1e-5, 1e-5, 1e-4, 1e-3, 1e-6, 1e-2]).unwrap_or(&1e-5);
    let s = *rng.choose(&[1.0, 1.0, 1.0, 10.0, 0.1]).unwrap_or(&1.0);
    let mut face = FaceIn {
        tol,
        o: [0.0, 0.0, 0.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        domain: [-4.0 * s, -4.0 * s, 4.0 * s, 4.0 * s],
        surf: None,
        loops: None,
    };
    let sweep = if rng.chance(0.6) {
        360.0
    } else {
        *rng.choose(&[180.0, 270.0]).unwrap_or(&180.0)
    };
    match rng.below(8) {
        // the plane: a polygon and sometimes a hole, or a vertex loop
        0..=2 => {
            if rng.chance(0.2) {
                face.loops = Some(LoopsIn::Vertex {
                    at: [rng.uniform(-s, s), rng.uniform(-s, s), 0.0],
                });
            } else {
                let n = 3 + rng.below(4) as usize;
                let a0 = rng.uniform(0.0, 90.0);
                let outer: Vec<[f64; 3]> = (0..n)
                    .map(|i| polar(2.0 * s * rng.uniform(0.7, 1.0), a0 + 360.0 * i as f64 / n as f64, 0.0))
                    .collect();
                let mut loops = vec![outer];
                if rng.chance(0.4) {
                    loops.push((0..3).map(|i| polar(0.4 * s, -120.0 * f64::from(i), 0.0)).collect());
                }
                face.loops = Some(LoopsIn::Edges { loops });
            }
        }
        // a cylinder: a vertex loop on it or a quad of two rulings and two chords
        3..=4 => {
            let r = s * rng.uniform(0.5, 2.0);
            face.surf = Some(revolution(false, r, -2.0 * s, 2.0 * s, sweep));
            if rng.chance(0.25) {
                face.loops = Some(LoopsIn::Vertex {
                    at: polar(r, rng.uniform(0.0, sweep), 0.0),
                });
            } else {
                let a0 = rng.uniform(5.0, 40.0);
                let a1 = a0 + rng.uniform(30.0, sweep - a0 - 5.0);
                let (z0, z1) = (-s * rng.uniform(0.2, 1.5), s * rng.uniform(0.2, 1.5));
                face.loops = Some(LoopsIn::Edges {
                    loops: vec![vec![polar(r, a0, z0), polar(r, a1, z0), polar(r, a1, z1), polar(r, a0, z1)]],
                });
            }
        }
        // a sphere: a vertex loop at a pole or on it, or a triangle of corners on it
        5..=6 => {
            let r = s * rng.uniform(0.5, 2.0);
            face.surf = Some(revolution(true, r, 0.0, 0.0, sweep));
            match rng.below(3) {
                0 => {
                    let z = if rng.chance(0.5) { r } else { -r };
                    face.loops = Some(LoopsIn::Vertex { at: [0.0, 0.0, z] });
                }
                1 => {
                    let lat = rng.uniform(-60.0f64, 60.0).to_radians();
                    face.loops = Some(LoopsIn::Vertex {
                        at: polar(r * lat.cos(), rng.uniform(0.0, sweep), r * lat.sin()),
                    });
                }
                _ => {
                    let lat = rng.uniform(10.0f64, 50.0).to_radians();
                    let (rc, z) = (r * lat.cos(), r * lat.sin());
                    let a0 = rng.uniform(5.0, 40.0);
                    let a1 = a0 + rng.uniform(30.0, sweep - a0 - 5.0);
                    face.loops = Some(LoopsIn::Edges {
                        loops: vec![vec![polar(rc, a0, z), polar(rc, a1, z), [0.0, 0.0, r]]],
                    });
                }
            }
        }
        // the heal.face_props generator (cones, tori, degenerate domains, loops off the surface)
        _ => face = hfp::gen_face(rng),
    }
    face
}

/// Face props seeds of a preset at stage `2a` or later: the stage, the zone tolerance and the
/// closed and pole flags `GetLoopCrossings` reads.
fn gen_preset(rng: &mut Rng) -> Vec<[f64; 3]> {
    let mut v = vec![
        [0.0, 0.0, *rng.choose(&[3.0, 4.0, 5.0, 6.0]).unwrap_or(&3.0)],
        [1.0, 0.0, *rng.choose(&[1e-5, 1e-3, 1e-7, 1e-2]).unwrap_or(&1e-5)],
        [22.0, 0.0, rng.below(3) as f64],
        [23.0, 0.0, rng.below(3) as f64],
    ];
    if rng.chance(0.3) {
        v.push([3.0, 0.0, rng.below(16) as f64]);
    }
    v
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
fn gen_class(rng: &mut Rng, objs: &[usize], topo: bool) -> (i32, Option<usize>) {
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

fn gen_pc(rng: &mut Rng, objs: &[usize], prm: f64, topo: bool) -> PcIn {
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
fn setup_cc(rng: &mut Rng, objs: &[usize], t: usize, curve: Option<usize>, bounds: &[f64], topo: bool, ops: &mut Vec<OpIn>) {
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
            ops.push(OpIn::PcSetSrcTol { t, j, w, tol: pc.tol });
            if let Some(p) = pc.sss {
                ops.push(OpIn::PcSetSss { t, j, w, p });
            }
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
    let face = rng.chance(0.9).then(|| gen_face(rng));
    let kinds = pool_kinds(&objects, face.as_ref());
    // boundary classifications on the face's topology
    let topo = face.is_some() && rng.chance(0.35);
    // the classification curve: a 3D one most of the time, often a short (degenerate) line
    let curves: Vec<(usize, (f64, f64, u64))> = objects
        .iter()
        .enumerate()
        .filter_map(|(i, o)| curve_info(o).map(|c| (i, c)))
        .filter(|(_, (a, b, _))| b - a > 1e-3)
        .collect();
    let three_d: Vec<(usize, (f64, f64, u64))> = curves.iter().filter(|(_, c)| c.2 == 3).copied().collect();
    let short: Vec<(usize, (f64, f64, u64))> = curves
        .iter()
        .filter(|(i, _)| matches!(objects.get(*i), Some(ObjIn::Curve(CurveIn::Line(l))) if l.domain == [0.0, 0.01]))
        .copied()
        .collect();
    let pool = if !short.is_empty() && rng.chance(0.6) {
        &short
    } else if rng.chance(0.9) && !three_d.is_empty() {
        &three_d
    } else {
        &curves
    };
    let Some(&(ci, (d0, d1, dim))) = rng.choose(pool) else {
        return Script { face, objects, ops: Vec::new() };
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
        setup_cc(rng, &kinds, t, curve, &bounds, topo, &mut ops);
        if dim == 2 && rng.chance(0.5) {
            let surfs: Vec<usize> = of_kind(&kinds, kind::PLANE)
                .into_iter()
                .chain(of_kind(&kinds, kind::BSURF))
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
    let faces = of_kind(&kinds, kind::FACE);
    let loops = of_kind(&kinds, kind::LOOP);
    let curve_ids: Vec<usize> = curves.iter().map(|(i, _)| *i).collect();
    let surfs: Vec<usize> = of_kind(&kinds, kind::PLANE)
        .into_iter()
        .chain(of_kind(&kinds, kind::BSURF))
        .collect();
    // the classification face (GetIvlBound_EdgeuseObj needs one)
    for &t in &targets {
        if rng.chance(0.8) {
            ops.push(OpIn::SetFace { t, obj: pick(rng, &faces) });
        }
    }
    let n_ops = 1 + rng.below(3);
    for _ in 0..n_ops {
        let Some(&t) = rng.choose(&targets) else { break };
        let bounds = all_bounds.get(t).cloned().unwrap_or_default();
        let size = bounds.len().saturating_sub(1);
        ops.push(match rng.below(100) {
            0..=44 => {
                let with_props = rng.chance(0.6);
                OpIn::LoopCrossings {
                    t,
                    loops: if !loops.is_empty() && rng.chance(0.85) {
                        if rng.chance(0.5) {
                            loops.clone()
                        } else {
                            loops.iter().take(1).copied().collect()
                        }
                    } else {
                        Vec::new()
                    },
                    with_props,
                    seeds: if with_props && face.is_some() && rng.chance(0.6) { gen_preset(rng) } else { Vec::new() },
                }
            }
            45..=49 => OpIn::SetSrcTol { t, tol: gen_tol(rng) },
            50..=54 => OpIn::SetIgnoreTangency { t, b: rng.chance(0.5) },
            55..=79 => OpIn::IvlBound {
                t,
                j: rng.below(size as u64 + 1) as usize,
                do_min: rng.chance(0.5),
            },
            80..=89 => OpIn::Squeeze { t },
            _ => {
                let a = gen_prm(rng, &bounds, d0, d1);
                OpIn::Construct {
                    t,
                    curve: if rng.chance(0.9) { Some(ci) } else { pick(rng, &curve_ids) },
                    a,
                    b: if rng.chance(0.8) { d1 } else { a + offset(rng, d1 - d0).abs() },
                    uv: if rng.chance(0.2) { pick(rng, &curve_ids) } else { None },
                    tol: gen_tol(rng),
                    data_size: if rng.chance(0.8) { 0 } else { rng.below(4) as usize },
                    owned: rng.chance(0.2),
                    current: rng.chance(0.8),
                    s1: if dim == 2 || rng.chance(0.2) { pick(rng, &surfs) } else { None },
                    s2: if rng.chance(0.2) { pick(rng, &surfs) } else { None },
                    uv2: if rng.chance(0.1) { pick(rng, &curve_ids) } else { None },
                }
            }
        });
    }
    Script { face, objects, ops }
}

/// Whether the shim accepts the script (it rejects what is undefined upstream).
fn shim_accepts(s: &Script) -> bool {
    match ffi::run(ffi_face(s).as_ref(), &cpp_input(s)) {
        Ok(_) => true,
        Err(solidcore_ffi::FfiError::Status(c)) if c == SC_ERR_ARG => false,
        Err(_) => false,
    }
}

// ---- fixtures built in code ---------------------------------------------------------------

/// A touch list: its length, then `connected dir loop_index param pt_classify_index` per touch.
fn put_touches(o: &mut Vec<f64>, pool: &Pool, l: &[TouchData<H>]) {
    o.push(l.len() as f64);
    for t in l {
        o.extend([
            pool.id(t.connected_to),
            f64::from(t.loop_dir.code()),
            t.loop_index as f64,
            t.param,
            t.pt_classify_index as f64,
        ]);
    }
}

/// The objects of the edge cases: a line over `[0, 1]` of the given speed (0), a plane (1), a
/// point (2), a circle (3), an "other" stand-in (4), a B-spline surface (5) and a line of
/// length `2e-8` over `[0, 0.01]` (6, degenerate on the fixture faces).
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
        ObjIn::Other,
        ObjIn::Bsurf(SurfIn {
            oob: false,
            net: vec![
                vec![[0.0, 0.0, 0.0, NOW], [0.0, scale, 0.0, NOW]],
                vec![[scale, 0.0, 0.0, NOW], [scale, scale, 0.1 * scale, NOW]],
            ],
            p: 1,
            q: 1,
            u: vec![0.0, 0.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
        }),
        ObjIn::Curve(short_line([0.25 * scale, 0.25 * scale, 0.0], [1.0, 0.0, 0.0], 2e-8)),
    ]
}

/// Pool index of the face of the edge cases (after the [`edge_objects`]); its loop follows,
/// then the first edge, vertex and edge use (a vertex loop: the vertex).
const EDGE_FACE: usize = 7;
const EDGE_LOOP: usize = 8;
const EDGE_EDGE: usize = 9;
const EDGE_VERTEX: usize = 10;

/// The faces of the edge cases, of size `s`: the z = 0 plane with a square edge loop, a
/// closed and a half cylinder with a quad of two rulings and two chords, and a closed sphere
/// with a vertex loop at its north pole (and a second vertex loop face on the sphere's
/// equator). The tolerance is `1e-4 s` (the short line is degenerate on them).
fn edge_faces(s: f64) -> Vec<(&'static str, FaceIn)> {
    let base = FaceIn {
        tol: 1e-4 * s.max(1e-2),
        o: [0.0, 0.0, 0.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        domain: [-2.0 * s, -2.0 * s, 2.0 * s, 2.0 * s],
        surf: None,
        loops: None,
    };
    let quad = |r: f64| {
        Some(LoopsIn::Edges {
            loops: vec![vec![polar(r, 10.0, -0.5 * r), polar(r, 100.0, -0.5 * r), polar(r, 100.0, 0.5 * r), polar(r, 10.0, 0.5 * r)]],
        })
    };
    vec![
        (
            "plane",
            FaceIn {
                loops: Some(LoopsIn::Edges {
                    loops: vec![vec![[0.0, 0.0, 0.0], [s, 0.0, 0.0], [s, s, 0.0], [0.0, s, 0.0]]],
                }),
                ..base.clone()
            },
        ),
        ("cylinder", FaceIn { surf: Some(revolution(false, s, -s, s, 360.0)), loops: quad(s), ..base.clone() }),
        ("half_cylinder", FaceIn { surf: Some(revolution(false, s, -s, s, 180.0)), loops: quad(s), ..base.clone() }),
        (
            "sphere_pole",
            FaceIn {
                surf: Some(revolution(true, s, 0.0, 0.0, 360.0)),
                loops: Some(LoopsIn::Vertex { at: [0.0, 0.0, s] }),
                ..base.clone()
            },
        ),
        (
            "sphere_equator",
            FaceIn {
                surf: Some(revolution(true, s, 0.0, 0.0, 360.0)),
                loops: Some(LoopsIn::Vertex { at: [s, 0.0, 0.0] }),
                ..base
            },
        ),
    ]
}

/// A classification of `bounds` on pool curve `curve` with the given tolerance.
fn edge_setup(t: usize, curve: Option<usize>, bounds: &[f64], tol: f64) -> Vec<OpIn> {
    let mut ops = vec![
        OpIn::SetSize { t, n: bounds.len() - 1 },
        OpIn::SetCurve { t, obj: curve, owned: false },
        OpIn::SetSrcTol { t, tol },
    ];
    for (j, w) in bounds.windows(2).enumerate() {
        if let [a, b] = *w {
            ops.push(OpIn::IvlSet { t, j, a, b });
        }
    }
    ops
}

/// A loop crossings op over the face's loop.
fn crossings(t: usize, loops: Vec<usize>, with_props: bool, seeds: Vec<[f64; 3]>) -> OpIn {
    OpIn::LoopCrossings { t, loops, with_props, seeds }
}

/// Edge cases: the loop crossings over the real loop of every fixture face (a degenerate
/// classification curve with computed, preset and no face props; a regular curve), and of
/// empty loop lists over every classification shape (empty, without curve, closed, short
/// intervals); the squeeze over equal, zero-length and single intervals; the
/// interval-bounding edge use of every end class (edge, vertex, curve, unknown) on every
/// fixture face; and the constructor over tolerance limits and every optional argument.
#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let bounds = [0.0, 0.25, 0.75, 1.0];
    for (name, scale) in [("unit", 1.0), ("huge", 1e5), ("tiny", 1e-4)] {
        let objs = edge_objects(scale);
        let script = |face: Option<&FaceIn>, ops: Vec<OpIn>| {
            json!(Script { face: face.cloned(), objects: objs.clone(), ops })
        };
        let faces = edge_faces(scale.max(1e-2));
        for (fname, face) in &faces {
            let closed = face.surf.is_some();
            // the degenerate classification curve: no props, computed props, preset props
            let preset = vec![
                [0.0, 0.0, 3.0],
                [1.0, 0.0, face.tol],
                [22.0, 0.0, if closed { 1.0 } else { 0.0 }],
                [23.0, 0.0, 0.0],
            ];
            for (mode, with_props, seeds) in [
                ("preset", true, preset.clone()),
                ("computed", true, Vec::new()),
                ("none", false, Vec::new()),
            ] {
                let mut ops = edge_setup(0, Some(6), &[0.0, 0.005, 0.01], 1e-5 * scale);
                ops.push(crossings(0, vec![EDGE_LOOP], with_props, seeds));
                out.push((format!("edge_loop_crossings_{name}_{fname}_degenerate_{mode}"), script(Some(face), ops)));
            }
            // a regular classification curve with preset props
            let mut ops = edge_setup(0, Some(0), &bounds, 1e-5 * scale);
            ops.push(OpIn::SetFace { t: 0, obj: Some(EDGE_FACE) });
            ops.push(crossings(0, vec![EDGE_LOOP], true, preset));
            out.push((format!("edge_loop_crossings_{name}_{fname}_line"), script(Some(face), ops)));

            // the bounding edge use of each end class on the face
            let on_edges = matches!(face.loops, Some(LoopsIn::Edges { .. }));
            let vertex = if on_edges { EDGE_VERTEX } else { EDGE_EDGE };
            let mut classes = vec![(104, Some(vertex), "vertex"), (106, Some(0), "curve"), (100, None, "unknown")];
            if on_edges {
                classes.insert(0, (103, Some(EDGE_EDGE), "edge"));
            }
            for (class, obj, cname) in classes {
                for do_min in [true, false] {
                    let mut ops = edge_setup(0, Some(0), &bounds, 1e-5 * scale);
                    ops.push(OpIn::SetFace { t: 0, obj: Some(EDGE_FACE) });
                    let (j, w) = if do_min { (1, 0) } else { (1, 2) };
                    ops.push(OpIn::PcSetClass { t: 0, j, w, class, obj });
                    ops.push(OpIn::PcSetT { t: 0, j, w, v: 0.5 });
                    ops.push(OpIn::IvlBound { t: 0, j, do_min });
                    out.push((
                        format!("edge_ivl_bound_{name}_{fname}_{cname}_{}", if do_min { "min" } else { "max" }),
                        script(Some(face), ops),
                    ));
                }
            }
        }

        // loop crossings of empty loop lists: empty classification, with intervals, closed
        // curve, no curve
        let plane = faces.first().map(|(_, f)| f.clone());
        let mut ops = Vec::new();
        for with_props in [false, true] {
            ops.push(crossings(0, Vec::new(), with_props, Vec::new()));
        }
        ops.extend(edge_setup(0, Some(0), &bounds, 1e-5 * scale));
        ops.push(OpIn::SetFace { t: 0, obj: Some(EDGE_FACE) });
        ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 2, class: 103, obj: Some(EDGE_EDGE) });
        ops.push(OpIn::PcSetClass { t: 0, j: 1, w: 2, class: 104, obj: Some(EDGE_VERTEX) });
        ops.push(OpIn::AddTgtLoop { t: 0, obj: Some(EDGE_LOOP) });
        for with_props in [false, true] {
            ops.push(crossings(0, Vec::new(), with_props, Vec::new()));
        }
        ops.extend(edge_setup(1, Some(3), &[0.0, 90.0, 360.0], USE_DEFAULT));
        ops.push(crossings(1, Vec::new(), true, Vec::new()));
        ops.extend(edge_setup(1, None, &[0.0, 1.0], 0.0));
        ops.push(crossings(1, Vec::new(), false, Vec::new()));
        out.push((format!("edge_loop_crossings_{name}_empty"), script(plane.as_ref(), ops)));

        // the squeeze: equal lengths (first wins), a zero-length interval, the last shortest,
        // one interval, end classes deciding the end
        for (i, b) in [
            vec![0.0, 0.5, 1.0],
            vec![0.0, 0.25, 0.25, 1.0],
            vec![0.0, 0.5, 0.9, 1.0],
            vec![0.0, 1.0],
            vec![0.0, 0.1, 0.2, 0.3, 1.0],
        ]
        .into_iter()
        .enumerate()
        {
            let mut ops = edge_setup(0, Some(0), &b, 1e-5 * scale);
            ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 0, class: 106, obj: Some(0) });
            ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 2, class: 107, obj: Some(1) });
            ops.push(OpIn::Squeeze { t: 0 });
            out.push((format!("edge_squeeze_{name}_{i}"), script(None, ops)));
        }

        // the constructor: tolerance limits, optional curves and surfaces, data sizes, flags
        for (i, (curve, a, b, tol, data_size, owned, current, uv, s1, s2, uv2)) in [
            (Some(0), 0.0, 1.0, USE_DEFAULT, 0, false, true, None, None, None, None),
            (Some(0), 0.25, 0.75, 1e-5, 0, true, false, None, Some(1), None, None),
            (Some(3), 0.0, 360.0, 0.0, 2, false, true, Some(0), Some(5), Some(1), Some(3)),
            (None, 0.0, 1.0, -1e-4, 0, false, true, None, None, None, None),
            (Some(0), 1.0, 1.0, 1e-8, 4, false, false, None, None, Some(5), None),
        ]
        .into_iter()
        .enumerate()
        {
            let mut ops = edge_setup(0, Some(0), &bounds, 1e-5 * scale);
            ops.push(OpIn::Construct { t: 0, curve, a, b, uv, tol, data_size, owned, current, s1, s2, uv2 });
            ops.push(crossings(0, Vec::new(), false, Vec::new()));
            out.push((format!("edge_construct_{name}_{i}"), script(None, ops)));
        }
    }
    out
}

/// Scripts undefined upstream that the shim must reject: the squeeze of an empty
/// classification, the bounding edge use without face, of an interval past the intervals and
/// of an edge end without classification curve, the loop crossings over a loop without
/// classification curve (UB-124). The flag is true for a script the ffi guard itself rejects
/// (the seeded face props of UB-131: `SetProps_Stage2` after `SetProps_Gaps` returned early
/// dereferences the null `m_pFace`), false for one the shim rejects at run time.
#[cfg(test)]
fn rejected_cases() -> Vec<(String, Script, bool)> {
    let objs = edge_objects(1.0);
    let face = edge_faces(1.0).first().map(|(_, f)| f.clone());
    let bounds = [0.0, 0.5, 1.0];
    let script = |ops: Vec<OpIn>| Script { face: face.clone(), objects: objs.clone(), ops };
    let mut out: Vec<(String, Script, bool)> = Vec::new();
    out.push(("squeeze_empty".into(), script(vec![OpIn::Squeeze { t: 0 }]), false));
    let mut ops = edge_setup(0, Some(0), &bounds, 1e-5);
    ops.push(OpIn::IvlBound { t: 0, j: 0, do_min: true });
    out.push(("ivl_bound_no_face".into(), script(ops), false));
    let mut ops = edge_setup(0, Some(0), &bounds, 1e-5);
    ops.push(OpIn::SetFace { t: 0, obj: Some(EDGE_FACE) });
    ops.push(OpIn::IvlBound { t: 0, j: 2, do_min: false });
    out.push(("ivl_bound_past_end".into(), script(ops), false));
    let mut ops = edge_setup(0, None, &bounds, 1e-5);
    ops.push(OpIn::SetFace { t: 0, obj: Some(EDGE_FACE) });
    ops.push(OpIn::PcSetClass { t: 0, j: 0, w: 0, class: 103, obj: Some(EDGE_EDGE) });
    ops.push(OpIn::IvlBound { t: 0, j: 0, do_min: true });
    out.push(("ivl_bound_edge_no_curve".into(), script(ops), false));
    let mut ops = edge_setup(0, None, &bounds, 1e-5);
    ops.push(crossings(0, vec![EDGE_LOOP], false, Vec::new()));
    out.push(("loop_crossings_no_curve".into(), script(ops), false));
    // UB-131: a props seed at stage GAPSa/GAPS without the face pointer (field 24)
    for stage in [1.0, 2.0] {
        let mut ops = edge_setup(0, Some(0), &bounds, 1e-5);
        ops.push(OpIn::SetFace { t: 0, obj: Some(EDGE_FACE) });
        ops.push(crossings(0, vec![EDGE_LOOP], true, vec![[0.0, 0.0, stage]]));
        out.push((format!("loop_crossings_props_null_face_{stage}"), script(ops), true));
    }
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p4"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let s = gen_script(rng);
            if ffi::validate(ffi_face(&s).as_ref(), &cpp_input(&s)).is_err() || !shim_accepts(&s) {
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

    /// Generated scripts are accepted by the ffi checks, and a share of them is compared.
    #[test]
    fn generated_scripts_are_accepted() {
        let mut rng = Rng::new(11);
        let (mut total, mut compared) = (0, 0);
        for _ in 0..300 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            if rust_run(&s).is_ok() {
                compared += 1;
            }
        }
        assert!(total > 250, "{total}");
        assert!(compared * 5 > total, "{compared} of {total} compared");
    }

    /// The edge cases are accepted by the shim.
    #[test]
    fn edge_cases_are_accepted() {
        for (name, v) in edge_cases() {
            let s: Script = serde_json::from_value(v).unwrap();
            assert!(ffi::validate(ffi_face(&s).as_ref(), &cpp_input(&s)).is_ok(), "{name}");
            assert!(shim_accepts(&s), "{name}");
        }
    }

    /// The scripts undefined upstream are rejected by the shim (or the ffi guard), and the
    /// Rust side reports an error status (or defers) instead of reading past its data.
    #[test]
    fn undefined_scripts_are_rejected() {
        for (name, s, guard) in rejected_cases() {
            let valid = ffi::validate(ffi_face(&s).as_ref(), &cpp_input(&s)).is_ok();
            if guard {
                assert!(!valid, "{name}");
                // Rust reports the null dereference instead of carrying on: the last op
                // (the loop crossings) records SM_ERR_NULL_POINTER (InvalidInput, 1007) right
                // after the record of the ops before it
                let mut before = s.clone();
                assert!(matches!(before.ops.pop(), Some(OpIn::LoopCrossings { .. })), "{name}");
                let Ok(prefix) = rust_run(&before) else { panic!("{name}: setup failed") };
                let Ok(full) = rust_run(&s) else { panic!("{name}: no record") };
                assert_eq!(full.get(..prefix.len()), Some(prefix.as_slice()), "{name}");
                assert_eq!(
                    full.get(prefix.len()..prefix.len() + 2),
                    Some([op::LOOP_CROSSINGS as f64, 1007.0].as_slice()),
                    "{name}"
                );
                continue;
            }
            assert!(valid, "{name}");
            assert!(!shim_accepts(&s), "{name}");
            assert!(rust_run(&s).is_ok() || matches!(rust_run(&s), Err(Fail::Deferred(_))), "{name}");
        }
    }
}
