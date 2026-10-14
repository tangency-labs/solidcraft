//! Runner for unit `intersect.curve_class_p1`: `SmCurveInterval`, the part-1 members of
//! `SmCurveClassification`, `SmTouchData`'s touch-list helpers, the classification overloads
//! of `SmTol` and the static helpers of `SmCurveClass.cpp`.
//!
//! C++: `solidcore_ffi::units::intersect_curve_class_p1` (upstream
//! `source/SMLib/src/SmCurveClass.cpp`). Rust: `solidcore::intersect::curve_class`.
//!
//! A case is a script: a pool of `objects` (curves as in unit `curve.curve_p6` plus real
//! `SmEllipse` / `SmCircle`, planes, B-spline surfaces, points, vertex, edge and face objects,
//! loop and "other" stand-ins) and `ops` on two default classifications `cc[0]`, `cc[1]`. Both
//! sides produce the flat record documented in the ffi module (per op: code, status, outputs,
//! state of the target classification), reported as the `out` values. A Rust error maps to
//! its `SmStatus` (`InvalidInput` 1007, `-1` for one upstream cannot return); an `Unported`
//! error anywhere defers the case.
//!
//! Deferred cases: ops that reach topology (`ComputePointParameters` on vertices, edges and
//! faces, `sm_SetXSectEdgeThroughPoint`, `sm_GetCCIGeometry`, `sm_ComputeFacePointClass`),
//! surface point solves (`ComputePointParameters` on surfaces, `sm_GetCCISurfaceGaps` with
//! surfaces), `SmTol::MapTo2d` for a curve of dimension other than 2 or 3 on a surface
//! (`SmSurface::DropPoint`) and an `SmCrvOnSurf` over a plane. Generated scripts are filtered by the shim's
//! run-time checks (scripts it rejects as undefined upstream are dropped).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject, SolutionType};
use solidcore::geom::tol::{zone_tol3d, ScaledZero};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::{
    self as cc_mod, ApplyPointSolutionInput, CrvDirOnFaceType, CurveClassification,
    CurveInterval, TouchData, CURVE_CLASSIFICATION_TYPE_STRING,
};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_curve_class_p1::{self as ffi, kind, op};

use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `intersect.curve_class_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

pub(crate) const NOZ: f64 = f64::MAX;
pub(crate) const NOW: f64 = -f64::MAX;
/// `SM_USE_DEFAULT`.
pub(crate) const USE_DEFAULT: f64 = 9876543.21;
/// `SM_BIG_DOUBLE`.
pub(crate) const BIG: f64 = 1.0e20;
/// `SC_ERR_ARG`.
pub(crate) const SC_ERR_ARG: i32 = -1002;

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BSplineIn {
    pub(crate) dim: u64,
    #[serde(default)]
    pub(crate) oob: bool,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HermiteIn {
    pub(crate) dim: u64,
    pub(crate) p1: [f64; 3],
    pub(crate) d1: [f64; 3],
    pub(crate) p2: [f64; 3],
    pub(crate) d2: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LineIn {
    pub(crate) p: [f64; 3],
    pub(crate) v: [f64; 3],
    pub(crate) domain: [f64; 2],
    #[serde(default = "one")]
    pub(crate) scale: f64,
    #[serde(default)]
    pub(crate) inside_out: bool,
}

fn one() -> f64 {
    1.0
}

/// An `SmEllipse(C, X, Y, [d0, d1], rx, ry, dim, ctx, NULL, io)` or, without `ry`, an
/// `SmCircle(C, X, Y, [d0, d1], rx, dim, ctx, NULL, io)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConicIn {
    pub(crate) center: [f64; 3],
    pub(crate) x: [f64; 3],
    pub(crate) y: [f64; 3],
    pub(crate) domain: [f64; 2],
    pub(crate) rx: f64,
    pub(crate) ry: Option<f64>,
    #[serde(default)]
    pub(crate) inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Line(LineIn),
    Conic(ConicIn),
}

/// A B-spline surface: control net `net[i][j]` (`x y z w`, `w = NOW` non-rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SurfIn {
    #[serde(default)]
    pub(crate) oob: bool,
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObjIn {
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

/// A touch: `connected_to loop_dir loop_index param pt_index pt_classify`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct TouchIn {
    c: Option<usize>,
    dir: i32,
    li: u64,
    prm: f64,
    pi: u64,
    pc: Option<usize>,
}

/// A solution of `sm_CullOrRefineCCIIn3d`: `type s0 s1 e0 e1 value`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SolIn {
    ty: usize,
    s: [f64; 2],
    e: [f64; 2],
    value: f64,
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
    SubInterval { t: usize, j: usize },
    ClassifyCrvLoc { t: usize, j: usize, w: usize, prm: f64 },
    SetPcParams { t: usize, mids: bool },
    SqueezeEnd { t: usize, j: usize },
    IsReferenced { t: usize, obj: Option<usize> },
    MakeConsistent { t: usize },
    Type { t: usize, types: Vec<i64> },
    SrcTol { t: usize },
    IvlTols { t: usize, j: usize },
    ObjDim { t: usize, obj: Option<usize> },
    XsectCurve {
        t: usize,
        c: Option<usize>,
        tol: f64,
        uvc: Vec<Option<usize>>,
        srf: Vec<Option<usize>>,
        uv: Vec<[f64; 2]>,
        prm: f64,
        dev: f64,
        p: [f64; 3],
    },
    Apply {
        t: usize,
        use_cc: bool,
        edge: Option<usize>,
        emate: Option<usize>,
        srf: [Option<usize>; 4],
        uv: [[f64; 2]; 4],
        sol: [f64; 3],
        sss: [f64; 3],
    },
    Angle { t: usize, c1: Option<usize>, t1: f64, c2: Option<usize>, t2: f64, dev: f64 },
    Cull {
        t: usize,
        s: Option<usize>,
        uvc: Option<usize>,
        ivl1: [f64; 2],
        c2: Option<usize>,
        ivl2: [f64; 2],
        tol: f64,
        sols: Vec<SolIn>,
    },
    TolPair { t: usize, c: Option<usize>, s: Option<usize>, p1: f64, tol1: f64, p2: f64, tol2: f64 },
    Gaps {
        t: usize,
        cp: [f64; 3],
        ep: [f64; 3],
        sol: [f64; 2],
        pairs: Vec<(Option<usize>, Option<usize>)>,
        count: usize,
    },
    TouchConnected { t: usize, c: Option<usize>, touches: Vec<TouchIn> },
    TouchWinding { t: usize, touches: Vec<TouchIn> },
    EdgeThrough { t: usize, e: Option<usize>, p: [f64; 3], srf: [Option<usize>; 2], uv: [[f64; 2]; 2], prm: f64 },
    CciGeometry { t: usize, e: Option<usize> },
    FacePointClass { t: usize, f: Option<usize>, p: [f64; 3], tol: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Script {
    objects: Vec<ObjIn>,
    ops: Vec<OpIn>,
}

// ---- objects ----------------------------------------------------------------------------

pub(crate) fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

pub(crate) fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

pub(crate) fn opt_id(o: Option<usize>) -> f64 {
    o.map_or(-1.0, |i| i as f64)
}

pub(crate) fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
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

pub(crate) fn surface_block(v: &mut Vec<f64>, s: &SurfIn) {
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

pub(crate) fn build_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
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

pub(crate) fn build_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
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

fn touches_block(v: &mut Vec<f64>, ts: &[TouchIn]) {
    v.push(ts.len() as f64);
    for t in ts {
        v.extend([
            opt_id(t.c),
            f64::from(t.dir),
            t.li as f64,
            t.prm,
            t.pi as f64,
            opt_id(t.pc),
        ]);
    }
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
            OpIn::RemoveAt { j, .. }
            | OpIn::IvlReset { j, .. }
            | OpIn::SubInterval { j, .. }
            | OpIn::SqueezeEnd { j, .. }
            | OpIn::IvlTols { j, .. } => v.push(*j as f64),
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
            | OpIn::SetUvCurve2 { obj, .. }
            | OpIn::IsReferenced { obj, .. }
            | OpIn::ObjDim { obj, .. } => v.push(opt_id(*obj)),
            OpIn::CciGeometry { e, .. } => v.push(opt_id(*e)),
            OpIn::SetIntersectData {
                s1, s2, uv2, current, ti, ..
            } => v.extend([opt_id(*s1), opt_id(*s2), opt_id(*uv2), b(*current), opt_id(*ti)]),
            OpIn::SetIsCurveCurrent { b: x, .. }
            | OpIn::SetIgnoreTangency { b: x, .. }
            | OpIn::SetIsCurveOwned { b: x, .. }
            | OpIn::SetOkToSetTol { b: x, .. }
            | OpIn::SetTgtLoopsOnly { b: x, .. }
            | OpIn::SetPcParams { mids: x, .. } => v.push(b(*x)),
            OpIn::SetSrcTol { tol, .. } => v.push(*tol),
            OpIn::ClassifyCrvLoc { j, w, prm, .. } => v.extend([*j as f64, *w as f64, *prm]),
            OpIn::Type { types, .. } => {
                v.push(types.len() as f64);
                v.extend(types.iter().map(|&x| x as f64));
            }
            OpIn::XsectCurve {
                c, tol, uvc, srf, uv, prm, dev, p, ..
            } => {
                v.extend([opt_id(*c), *tol, uvc.len() as f64]);
                v.extend(uvc.iter().map(|x| opt_id(*x)));
                v.push(srf.len() as f64);
                v.extend(srf.iter().map(|x| opt_id(*x)));
                v.push(uv.len() as f64);
                for q in uv {
                    v.extend_from_slice(q);
                }
                v.extend([*prm, *dev]);
                v.extend_from_slice(p);
            }
            OpIn::Apply {
                use_cc,
                edge,
                emate,
                srf,
                uv,
                sol,
                sss,
                ..
            } => {
                v.extend([b(*use_cc), opt_id(*edge), opt_id(*emate)]);
                for (s, q) in srf.iter().zip(uv) {
                    v.extend([opt_id(*s), q[0], q[1]]);
                }
                v.extend_from_slice(sol);
                v.extend_from_slice(sss);
            }
            OpIn::Angle { c1, t1, c2, t2, dev, .. } => {
                v.extend([opt_id(*c1), *t1, opt_id(*c2), *t2, *dev]);
            }
            OpIn::Cull {
                s, uvc, ivl1, c2, ivl2, tol, sols, ..
            } => {
                v.extend([opt_id(*s), opt_id(*uvc), ivl1[0], ivl1[1], opt_id(*c2), ivl2[0], ivl2[1], *tol]);
                v.push(sols.len() as f64);
                for x in sols {
                    v.extend([x.ty as f64, x.s[0], x.s[1], x.e[0], x.e[1], x.value]);
                }
            }
            OpIn::TolPair {
                c, s, p1, tol1, p2, tol2, ..
            } => v.extend([opt_id(*c), opt_id(*s), *p1, *tol1, *p2, *tol2]),
            OpIn::Gaps {
                cp, ep, sol, pairs, count, ..
            } => {
                v.extend_from_slice(cp);
                v.extend_from_slice(ep);
                v.extend_from_slice(sol);
                v.push(pairs.len() as f64);
                for (s, u) in pairs {
                    v.extend([opt_id(*s), opt_id(*u)]);
                }
                v.push(*count as f64);
            }
            OpIn::TouchConnected { c, touches, .. } => {
                v.push(opt_id(*c));
                touches_block(&mut v, touches);
            }
            OpIn::TouchWinding { touches, .. } => touches_block(&mut v, touches),
            OpIn::EdgeThrough { e, p, srf, uv, prm, .. } => {
                v.push(opt_id(*e));
                v.extend_from_slice(p);
                for (s, q) in srf.iter().zip(uv) {
                    v.extend([opt_id(*s), q[0], q[1]]);
                }
                v.push(*prm);
            }
            OpIn::FacePointClass { f, p, tol, .. } => {
                v.push(opt_id(*f));
                v.extend_from_slice(p);
                v.push(*tol);
            }
            OpIn::MakeConsistent { .. } | OpIn::SrcTol { .. } => {}
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
        OpIn::SubInterval { t, .. } => (op::SUB_INTERVAL, t),
        OpIn::ClassifyCrvLoc { t, .. } => (op::CLASSIFY_CRV_LOC, t),
        OpIn::SetPcParams { t, .. } => (op::SET_PC_PARAMS, t),
        OpIn::SqueezeEnd { t, .. } => (op::SQUEEZE_END, t),
        OpIn::IsReferenced { t, .. } => (op::IS_REFERENCED, t),
        OpIn::MakeConsistent { t } => (op::MAKE_CONSISTENT, t),
        OpIn::Type { t, .. } => (op::TYPE, t),
        OpIn::SrcTol { t } => (op::SRC_TOL, t),
        OpIn::IvlTols { t, .. } => (op::IVL_TOLS, t),
        OpIn::ObjDim { t, .. } => (op::OBJ_DIM, t),
        OpIn::XsectCurve { t, .. } => (op::XSECT_CURVE, t),
        OpIn::Apply { t, .. } => (op::APPLY, t),
        OpIn::Angle { t, .. } => (op::ANGLE, t),
        OpIn::Cull { t, .. } => (op::CULL, t),
        OpIn::TolPair { t, .. } => (op::TOL_PAIR, t),
        OpIn::Gaps { t, .. } => (op::GAPS, t),
        OpIn::TouchConnected { t, .. } => (op::TOUCH_CONNECTED, t),
        OpIn::TouchWinding { t, .. } => (op::TOUCH_WINDING, t),
        OpIn::EdgeThrough { t, .. } => (op::EDGE_THROUGH, t),
        OpIn::CciGeometry { t, .. } => (op::CCI_GEOMETRY, t),
        OpIn::FacePointClass { t, .. } => (op::FACE_POINT_CLASS, t),
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
pub(crate) struct H(pub(crate) usize, pub(crate) ClassObjectKind);

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        self.1
    }
}

/// A solution handle naming nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NoObj;

impl SolutionObject for NoObj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

pub(crate) enum PoolObj {
    Curve(Curve),
    Surface(Surface),
    Point(Vec3),
    Bare,
}

pub(crate) struct Pool {
    pub(crate) objs: Vec<PoolObj>,
    pub(crate) handles: Vec<H>,
    pub(crate) kinds: Vec<usize>,
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
    pub(crate) fn handle(&self, i: Option<usize>) -> Result<Option<H>, HarnessError> {
        i.map(|i| {
            self.handles
                .get(i)
                .copied()
                .ok_or_else(|| harness("object index out of range"))
        })
        .transpose()
    }

    pub(crate) fn curve_at(&self, i: Option<usize>) -> Result<Option<&Curve>, HarnessError> {
        match i {
            None => Ok(None),
            Some(i) => match self.objs.get(i) {
                Some(PoolObj::Curve(c)) => Ok(Some(c)),
                _ => Err(harness("not a curve")),
            },
        }
    }

    pub(crate) fn surface_at(&self, i: Option<usize>) -> Result<Option<&Surface>, HarnessError> {
        match i {
            None => Ok(None),
            Some(i) => match self.objs.get(i) {
                Some(PoolObj::Surface(s)) => Ok(Some(s)),
                _ => Err(harness("not a surface")),
            },
        }
    }

    /// Takes curve `i` out of the pool (to edit it); put it back with [`Pool::put_curve`].
    fn take_curve(&mut self, i: usize) -> Result<Curve, HarnessError> {
        match self.objs.get_mut(i) {
            Some(slot @ PoolObj::Curve(_)) => match core::mem::replace(slot, PoolObj::Bare) {
                PoolObj::Curve(c) => Ok(c),
                _ => Err(harness("not a curve")),
            },
            _ => Err(harness("not a curve")),
        }
    }

    fn put_curve(&mut self, i: usize, c: Curve) {
        if let Some(slot) = self.objs.get_mut(i) {
            *slot = PoolObj::Curve(c);
        }
    }
}

pub(crate) fn class_kind_of(k: usize) -> ClassObjectKind {
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

pub(crate) fn obj_kind(o: &ObjIn) -> usize {
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

pub(crate) fn build_pool(objects: &[ObjIn]) -> Result<Pool, HarnessError> {
    let mut pool = Pool {
        objs: Vec::new(),
        handles: Vec::new(),
        kinds: Vec::new(),
    };
    for (i, o) in objects.iter().enumerate() {
        let k = obj_kind(o);
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
        pool.handles.push(H(i, class_kind_of(k)));
        pool.kinds.push(k);
    }
    Ok(pool)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => 1007.0,
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, a harness error otherwise.
pub(crate) enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(v)` or the status; `Unported` defers the case.
pub(crate) fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(Err(status(&e))),
        },
    }
}

/// The value, or a harness error for a status (an operation upstream cannot fail).
pub(crate) fn must<T>(r: solidcore::Result<T>) -> Result<T, Fail> {
    st(r)?.map_err(|s| Fail::Harness(harness(format!("unexpected status {s}"))))
}

pub(crate) fn id(h: Option<H>) -> f64 {
    h.map_or(-1.0, |h| h.0 as f64)
}

pub(crate) fn put_pc(o: &mut Vec<f64>, pc: &PointClassification<H>) {
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

/// The curve record (see the ffi module).
pub(crate) fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    let base = match c.kind() {
        CurveKind::BSpline(x) => Some(x),
        CurveKind::Ellipse(x) => Some(x.base()),
        CurveKind::Hyperbola(x) => Some(x.base()),
        CurveKind::Parabola(x) => Some(x.base()),
        CurveKind::Line(x) => Some(x.base()),
        _ => None,
    };
    match base.and_then(|x| x.gw_nurb()) {
        Some(cur) => o.extend([cur.n() as f64, cur.m() as f64]),
        None => o.extend([-1.0, -1.0]),
    }
    for i in 0..=6 {
        match c.evaluate_point(ivl.evaluate(f64::from(i) / 6.0)) {
            Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
            Err(e) => o.push(status(&e)),
        }
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

fn touch_list(pool: &Pool, ts: &[TouchIn]) -> Result<Vec<TouchData<H>>, HarnessError> {
    ts.iter()
        .map(|t| {
            Ok(TouchData {
                connected_to: pool.handle(t.c)?,
                loop_dir: CrvDirOnFaceType::from_code(t.dir).ok_or_else(|| harness("bad dir"))?,
                loop_index: t.li,
                param: t.prm,
                pt_classify_index: t.pi,
                pt_classify: pool.handle(t.pc)?,
            })
        })
        .collect()
}

fn put_touch(o: &mut Vec<f64>, t: &TouchData<H>) {
    o.extend([
        id(t.connected_to),
        f64::from(t.loop_dir.code()),
        t.loop_index as f64,
        t.param,
        t.pt_classify_index as f64,
        id(t.pt_classify),
    ]);
}

fn uvp(q: [f64; 2]) -> Point2 {
    Point2::new(q[0], q[1])
}

#[allow(clippy::too_many_lines)]
fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut pool = build_pool(&s.objects)?;
    let mut ccs: [CurveClassification<H>; 2] =
        [CurveClassification::new(&k), CurveClassification::new(&k)];
    let mut o: Vec<f64> = Vec::new();
    let mut deferred: Option<Box<Output>> = None;
    for x in &s.ops {
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
                        pc_mut(must(cc.at_mut(*j))?, *w)?.set_uv_param(Point2::new(*u, *v));
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
                    OpIn::SubInterval { j, .. } => {
                        let e = must(cc.sub_interval(*j))?;
                        o.extend([0.0, e.min(), e.max()]);
                    }
                    OpIn::ClassifyCrvLoc { j, w, prm, .. } => {
                        let pc = match *w {
                            0 => must(cc.at(*j))?.start,
                            1 => must(cc.at(*j))?.mid,
                            _ => must(cc.at(*j))?.end,
                        };
                        let p = must(cc.classify_crv_loc(&pool, &pc, *prm))?;
                        o.extend([0.0, p.x, p.y, p.z]);
                    }
                    OpIn::SetPcParams { mids, .. } => {
                        let r = cc.set_point_class_parameters(&k, &pool, *mids);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::SqueezeEnd { j, .. } => {
                        let pos = must(cc.find_best_squeeze_end(*j))?;
                        o.extend([0.0, f64::from(pos.code())]);
                    }
                    OpIn::IsReferenced { obj, .. } => {
                        o.extend([0.0, b(cc.is_referenced(pool.handle(*obj)?))]);
                    }
                    OpIn::MakeConsistent { .. } => {
                        let r = cc_mod::make_consistent_classifications(cc);
                        o.push(st(r)?.err().unwrap_or(0.0));
                    }
                    OpIn::Type { types, .. } => {
                        o.extend([0.0, cc.type_code() as f64]);
                        o.extend(types.iter().map(|&ty| b(cc.is_kind_of(ty))));
                        o.push(CURVE_CLASSIFICATION_TYPE_STRING.len() as f64);
                        o.extend(CURVE_CLASSIFICATION_TYPE_STRING.bytes().map(f64::from));
                    }
                    OpIn::SrcTol { .. } => {
                        o.extend([
                            0.0,
                            cc_mod::src_zone_tol_3d_of(&k, Some(&*cc)),
                            cc_mod::src_zone_tol_3d_of::<H>(&k, None),
                        ]);
                    }
                    OpIn::IvlTols { j, .. } => {
                        let ivl = *must(cc.at(*j))?;
                        let obj_tol = must(cc_mod::interval_obj_zone_tol_3d(&k, &pool, Some(&ivl)))?;
                        let none = must(cc_mod::interval_obj_zone_tol_3d::<H>(&k, &pool, None))?;
                        o.extend([
                            0.0,
                            cc_mod::src_zone_tol_3d_of(&k, Some(&*cc)),
                            obj_tol,
                            none,
                            ivl.scaled_zero(),
                        ]);
                    }
                    OpIn::ObjDim { obj, .. } => {
                        o.extend([0.0, f64::from(cc_mod::object_dimension(pool.handle(*obj)?))]);
                    }
                    _ => {}
                }
            }
            // the free functions (they may edit pool curves)
            let cc = ccs.get(t).cloned().ok_or_else(|| harness("bad target"))?;
            match x {
                OpIn::XsectCurve {
                    c, tol, uvc, srf, uv, prm, dev, p, ..
                } => {
                    let ci = c.ok_or_else(|| harness("no curve"))?;
                    let mut curve = pool.take_curve(ci)?;
                    let mut uv_curves = Vec::new();
                    for u in uvc {
                        let ui = u.ok_or_else(|| harness("no uv curve"))?;
                        match pool.curve_at(Some(ui))?.map(Curve::kind) {
                            Some(CurveKind::BSpline(bs)) => uv_curves.push(bs.clone()),
                            _ => return Err(harness("uv curve not a B-spline").into()),
                        }
                    }
                    let surfaces: Vec<&Surface> = srf
                        .iter()
                        .map(|x| pool.surface_at(*x)?.ok_or_else(|| harness("no surface")))
                        .collect::<Result<_, _>>()?;
                    let uvs: Vec<Point2> = uv.iter().map(|q| uvp(*q)).collect();
                    let r = cc_mod::set_xsect_curve_through_point(
                        &k,
                        &mut curve,
                        zone_tol3d(&k, *tol),
                        &mut uv_curves,
                        &surfaces,
                        &uvs,
                        *prm,
                        v3(*p),
                    );
                    let r = match st(r) {
                        Ok(r) => r,
                        Err(f) => {
                            pool.put_curve(ci, curve);
                            return Err(f);
                        }
                    };
                    match r {
                        Ok(r) => {
                            o.extend([
                                if r.success { 0.0 } else { 1001.0 },
                                b(r.modified_curve),
                                r.param,
                                r.deviation.unwrap_or(*dev),
                                b(r.set_3d_point),
                                r.point_3d.x,
                                r.point_3d.y,
                                r.point_3d.z,
                            ]);
                            put_curve(&mut o, &curve);
                            for (u, bs) in uvc.iter().zip(uv_curves) {
                                let ui = u.ok_or_else(|| harness("no uv curve"))?;
                                let c = Curve::from(bs);
                                put_curve(&mut o, &c);
                                pool.put_curve(ui, c);
                            }
                            pool.put_curve(ci, curve);
                        }
                        Err(s) => {
                            pool.put_curve(ci, curve);
                            return Err(harness(format!("xsect: status {s}")).into());
                        }
                    }
                }
                OpIn::Apply {
                    use_cc,
                    edge,
                    emate,
                    srf,
                    uv,
                    sol,
                    sss,
                    ..
                } => {
                    let src = if *use_cc { cc.curve() } else { None };
                    let src_tol = cc_mod::src_zone_tol_3d_of(&k, use_cc.then_some(&cc));
                    let mut curve = match src {
                        Some(h) => Some((h.0, pool.take_curve(h.0)?)),
                        None => None,
                    };
                    let mut solution: Solution<NoObj> = Solution::new();
                    solution.solution_type = SolutionType::SingleValue;
                    solution.start.parameters[0] = sol[0];
                    solution.start.parameters[1] = sol[1];
                    solution.start.solution_value = sol[2];
                    let r = {
                        let sp = |i: usize| -> Result<Option<(&Surface, Point2)>, HarnessError> {
                            let s = srf.get(i).copied().flatten();
                            let q = uv.get(i).copied().unwrap_or([0.0, 0.0]);
                            Ok(pool.surface_at(s)?.map(|s| (s, uvp(q))))
                        };
                        let input = ApplyPointSolutionInput {
                            src_zone_tol_3d: src_tol,
                            edge: pool.handle(*edge)?,
                            emate: pool.handle(*emate)?,
                            class_curve_surface1: sp(0)?,
                            class_curve_surface2: sp(1)?,
                            edge_curve_surface1: sp(2)?,
                            edge_curve_surface2: sp(3)?,
                        };
                        cc_mod::apply_point_solution(
                            &k,
                            curve.as_mut().map(|(_, c)| c),
                            &input,
                            &mut solution,
                            v3(*sss),
                        )
                    };
                    let res = st(r);
                    let done = |pool: &mut Pool, curve: Option<(usize, Curve)>| {
                        if let Some((i, c)) = curve {
                            pool.put_curve(i, c);
                        }
                    };
                    let r = match res {
                        Ok(Ok(r)) => r,
                        Ok(Err(s)) => {
                            done(&mut pool, curve);
                            return Err(harness(format!("apply: status {s}")).into());
                        }
                        Err(f) => {
                            done(&mut pool, curve);
                            return Err(f);
                        }
                    };
                    o.extend([
                        if r.success { 0.0 } else { 1001.0 },
                        r.sss_point.x,
                        r.sss_point.y,
                        r.sss_point.z,
                        b(r.sss_point_flag),
                        b(r.modified_curve),
                        b(r.modified_edge_curve),
                        b(r.modified_mate_curve),
                        r.deviation,
                        solution.start.parameters[0],
                        solution.start.parameters[1],
                    ]);
                    if let Some((_, c)) = &curve {
                        put_curve(&mut o, c);
                    }
                    done(&mut pool, curve);
                }
                OpIn::Angle { c1, t1, c2, t2, dev, .. } => {
                    let a = must(cc_mod::get_curve_curve_angle(
                        pool.curve_at(*c1)?,
                        *t1,
                        pool.curve_at(*c2)?,
                        *t2,
                        *dev,
                    ))?;
                    o.extend([0.0, a]);
                }
                OpIn::Cull {
                    s, uvc, ivl1, c2, ivl2, tol, sols, ..
                } => {
                    let mut arr: SolutionArray<NoObj> = SolutionArray::new();
                    for x in sols {
                        let mut sol: Solution<NoObj> = Solution::new();
                        sol.solution_type = *SolutionType::ALL
                            .get(x.ty)
                            .ok_or_else(|| harness("bad solution type"))?;
                        sol.start.parameters[0] = x.s[0];
                        sol.start.parameters[1] = x.s[1];
                        sol.end.parameters[0] = x.e[0];
                        sol.end.parameters[1] = x.e[1];
                        sol.start.solution_value = x.value;
                        arr.add(&sol);
                    }
                    let r = cc_mod::cull_or_refine_cci_in_3d(
                        &k,
                        pool.surface_at(*s)?.ok_or_else(|| harness("no surface"))?,
                        pool.curve_at(*uvc)?.ok_or_else(|| harness("no uv curve"))?,
                        &Extent1d::from_min_max(ivl1[0], ivl1[1]),
                        pool.curve_at(*c2)?.ok_or_else(|| harness("no curve"))?,
                        &Extent1d::from_min_max(ivl2[0], ivl2[1]),
                        *tol,
                        &mut arr,
                    );
                    o.push(st(r)?.err().unwrap_or(0.0));
                    o.push(arr.len() as f64);
                    for x in arr.as_slice() {
                        o.extend([
                            f64::from(x.solution_type.code()),
                            x.start.parameters[0],
                            x.start.parameters[1],
                            x.end.parameters[0],
                            x.end.parameters[1],
                            x.start.solution_value,
                        ]);
                    }
                }
                OpIn::TolPair {
                    c, s, p1, tol1, p2, tol2, ..
                } => {
                    let v = must(cc_mod::xsect_tol_2d_or_3d_for_classify_pair(
                        &k,
                        pool.curve_at(*c)?.ok_or_else(|| harness("no curve"))?,
                        pool.surface_at(*s)?,
                        *p1,
                        zone_tol3d(&k, *tol1),
                        *p2,
                        zone_tol3d(&k, *tol2),
                    ))?;
                    o.extend([0.0, v]);
                }
                OpIn::Gaps {
                    cp, ep, sol, pairs, count, ..
                } => {
                    let mut solution: Solution<NoObj> = Solution::new();
                    solution.start.parameters[0] = sol[0];
                    solution.start.parameters[1] = sol[1];
                    let mut surfaces = Vec::new();
                    let mut uvcs = Vec::new();
                    for (s, u) in pairs {
                        surfaces.push(pool.surface_at(*s)?.ok_or_else(|| harness("no surface"))?);
                        uvcs.push(pool.curve_at(*u)?);
                    }
                    let r = must(cc_mod::get_cci_surface_gaps(
                        &k,
                        v3(*cp),
                        v3(*ep),
                        &solution,
                        &surfaces,
                        &uvcs,
                        *count,
                    ))?;
                    o.extend([
                        if r.success { 0.0 } else { 1001.0 },
                        r.curve_edge_gap,
                        r.curve_surface_gap,
                        r.edge_surface_gap,
                        r.uv_points.len() as f64,
                    ]);
                    for q in &r.uv_points {
                        o.extend([q.x, q.y]);
                    }
                }
                OpIn::TouchConnected { c, touches, .. } => {
                    let list = touch_list(&pool, touches)?;
                    let out = cc_mod::get_connected_touches(pool.handle(*c)?, &list);
                    o.extend([0.0, out.len() as f64, out.len() as f64]);
                    for t in &out {
                        put_touch(&mut o, t);
                    }
                }
                OpIn::TouchWinding { touches, .. } => {
                    let list = touch_list(&pool, touches)?;
                    let (w, ot) = must(cc_mod::get_winding_count(&list))?;
                    let code = match ot {
                        solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Unknown => 0.0,
                        solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same => 1.0,
                        solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Opposite => 2.0,
                    };
                    o.extend([0.0, w as f64, code]);
                }
                OpIn::EdgeThrough { e, p, srf, uv, prm, .. } => {
                    let h = pool.handle(*e)?.ok_or_else(|| harness("no edge"))?;
                    let surfaces: Vec<&Surface> = srf
                        .iter()
                        .map(|x| pool.surface_at(*x)?.ok_or_else(|| harness("no surface")))
                        .collect::<Result<_, _>>()?;
                    let uvs: Vec<Point2> = uv.iter().map(|q| uvp(*q)).collect();
                    must(cc_mod::set_xsect_edge_through_point(h, v3(*p), &surfaces, &uvs, *prm))?;
                    return Err(harness("edge through point ported").into());
                }
                OpIn::CciGeometry { e, .. } => {
                    let h = pool.handle(*e)?.ok_or_else(|| harness("no edge"))?;
                    must(cc_mod::get_cci_geometry(&cc, h))?;
                    return Err(harness("cci geometry ported").into());
                }
                OpIn::FacePointClass { f, p, tol, .. } => {
                    let h = pool.handle(*f)?.ok_or_else(|| harness("no face"))?;
                    let mut pc = PointClassification::new(zone_tol3d(&k, USE_DEFAULT));
                    must(cc_mod::compute_face_point_class(v3(*p), zone_tol3d(&k, *tol), h, &mut pc))?;
                    return Err(harness("face point class ported").into());
                }
                _ => {}
            }
            put_cc(&mut o, &k, &cc);
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

fn rust(s: &Script) -> Result<Output, HarnessError> {
    match rust_run(s) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -------------------------------------------------------------------------

pub(crate) fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational or 2D).
pub(crate) fn gen_bspline(rng: &mut Rng) -> BSplineIn {
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

/// A piecewise-C0 cubic (an intersector approximation) near the line `o + s d`, `s` in
/// `[0, len]`, offset by up to `off`, over `[a, a + plen]`.
fn gen_xsect_cubic(rng: &mut Rng, o: [f64; 3], d: [f64; 3], len: f64, off: f64) -> BSplineIn {
    let segs = 1 + rng.below(3) as usize;
    let n = 3 * segs;
    let a = if rng.chance(0.5) { 0.0 } else { rng.uniform(-3.0, 3.0) };
    let plen = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.5, 20.0) };
    let mut u = vec![a; 4];
    for s in 1..segs {
        let k = a + plen * s as f64 / segs as f64;
        u.extend([k, k, k]);
    }
    u.extend([a + plen; 4]);
    let cpts = (0..=n)
        .map(|i| {
            let s = len * i as f64 / n as f64;
            let e = rp(rng, off);
            [o[0] + s * d[0] + e[0], o[1] + s * d[1] + e[1], o[2] + s * d[2] + e[2], NOW]
        })
        .collect();
    BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p: 3,
        u,
    }
}

pub(crate) fn gen_curve(rng: &mut Rng) -> CurveIn {
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
pub(crate) fn gen_surface(rng: &mut Rng, o: [f64; 3], x: [f64; 3], y: [f64; 3], sz: f64, bump: f64) -> SurfIn {
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
pub(crate) fn of_kind(objs: &[ObjIn], k: usize) -> Vec<usize> {
    objs.iter()
        .enumerate()
        .filter(|(_, o)| obj_kind(o) == k)
        .map(|(i, _)| i)
        .collect()
}

pub(crate) fn pick(rng: &mut Rng, v: &[usize]) -> Option<usize> {
    rng.choose(v).copied()
}

pub(crate) fn maybe(rng: &mut Rng, v: &[usize], p_none: f64) -> Option<usize> {
    if rng.chance(p_none) {
        None
    } else {
        pick(rng, v)
    }
}

/// A unit-ish random direction.
pub(crate) fn dir(rng: &mut Rng) -> [f64; 3] {
    let mut d = rp(rng, 1.0);
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l < 0.1 {
        d = [1.0, 0.0, 0.0];
    } else {
        d = [d[0] / l, d[1] / l, d[2] / l];
    }
    d
}

pub(crate) fn cross(a: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

pub(crate) fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

/// The base pool: curves, planes and B-spline surfaces, points, topology and stand-ins.
pub(crate) fn gen_objects(rng: &mut Rng) -> Vec<ObjIn> {
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

/// A 2D B-spline (degree 1 or 2) roughly along a chord of the UV domain of `srf` (the
/// unit square for anything but a B-spline surface), sometimes reaching past it.
fn gen_uv_line(rng: &mut Rng, srf: Option<&ObjIn>) -> BSplineIn {
    let (u0, u1, v0, v1) = match srf {
        Some(ObjIn::Bsurf(sf)) => (
            sf.u.first().copied().unwrap_or(0.0),
            sf.u.last().copied().unwrap_or(1.0),
            sf.v.first().copied().unwrap_or(0.0),
            sf.v.last().copied().unwrap_or(1.0),
        ),
        _ => (0.0, 1.0, 0.0, 1.0),
    };
    let reach = if rng.chance(0.2) { 0.3 } else { 0.0 };
    let corner = |rng: &mut Rng| {
        [
            rng.uniform(u0 - reach * (u1 - u0), u1 + reach * (u1 - u0)),
            rng.uniform(v0 - reach * (v1 - v0), v1 + reach * (v1 - v0)),
        ]
    };
    let pa = corner(rng);
    let pb = corner(rng);
    let deg = 1 + rng.below(2) as usize;
    let cnt = deg + 1 + rng.below(2) as usize;
    let cpts = (0..cnt)
        .map(|i| {
            let f = i as f64 / (cnt - 1) as f64;
            [
                pa[0] + f * (pb[0] - pa[0]) + rng.uniform(-0.05, 0.05) * (u1 - u0),
                pa[1] + f * (pb[1] - pa[1]) + rng.uniform(-0.05, 0.05) * (v1 - v0),
                NOZ,
                NOW,
            ]
        })
        .collect();
    let mut u = vec![0.0; deg + 1];
    u.extend((1..cnt - deg).map(|i| i as f64 / (cnt - deg) as f64));
    u.extend(vec![1.0; deg + 1]);
    BSplineIn { dim: 2, oob: false, cpts, p: deg, u }
}

pub(crate) fn gen_tol(rng: &mut Rng) -> f64 {
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

/// A class / object pair for `SetClassObject`.
fn gen_class(rng: &mut Rng, objs: &[ObjIn], topo: bool) -> (i32, Option<usize>) {
    let choices: &[(i32, usize)] = if topo {
        &[(102, kind::FACE), (103, kind::EDGE), (104, kind::VERTEX), (106, kind::CURVE), (107, kind::PLANE)]
    } else {
        &[(106, kind::CURVE), (106, kind::CURVE), (107, kind::PLANE), (107, kind::BSURF), (105, kind::POINT)]
    };
    if rng.chance(0.1) {
        return (*rng.choose(&[100, 110]).unwrap_or(&100), None);
    }
    let &(c, k) = rng.choose(choices).unwrap_or(&(106, kind::CURVE));
    (c, pick(rng, &of_kind(objs, k)))
}

/// Ops on the classification containers.
fn gen_container_op(rng: &mut Rng, objs: &[ObjIn], t: usize, size: usize, topo: bool) -> OpIn {
    let j = rng.below(size.max(1) as u64) as usize;
    let w = rng.below(3) as usize;
    let curves = of_kind(objs, kind::CURVE);
    let surfs: Vec<usize> = of_kind(objs, kind::PLANE)
        .into_iter()
        .chain(of_kind(objs, kind::BSURF))
        .collect();
    let all: Vec<usize> = (0..objs.len()).collect();
    match rng.below(40) {
        0..=1 => OpIn::SetSize { t, n: rng.below(7) as usize },
        2 => OpIn::RemoveAt { t, j: rng.below(size as u64 + 2) as usize },
        3..=5 => {
            let a = rng.uniform(-5.0, 5.0);
            OpIn::IvlSet { t, j, a, b: a + rng.uniform(-1.0, 5.0) }
        }
        6 => OpIn::IvlSetMin { t, j, v: rng.uniform(-5.0, 5.0) },
        7 => OpIn::IvlSetMax { t, j, v: rng.uniform(-5.0, 5.0) },
        8 => OpIn::IvlReset { t, j },
        9 => OpIn::IvlAssign { t, j, k: rng.below(3) as usize },
        10 => {
            let a = rng.uniform(-5.0, 5.0);
            OpIn::IvlNew {
                t,
                j,
                with: rng.chance(0.5),
                owner: rng.chance(0.5),
                a,
                b: a + rng.uniform(0.0, 3.0),
            }
        }
        11..=14 => {
            let (class, obj) = gen_class(rng, objs, topo);
            OpIn::PcSetClass { t, j, w, class, obj }
        }
        15 => OpIn::PcSetT { t, j, w, v: rng.uniform(-5.0, 5.0) },
        16 => OpIn::PcSetUv { t, j, w, u: rng.uniform(-5.0, 5.0), v: rng.uniform(-5.0, 5.0) },
        17 => OpIn::PcSetGap { t, j, w, g: *rng.choose(&[0.0, 1e-6, 1e-3, 0.5]).unwrap_or(&0.0) },
        18 => OpIn::PcSetSss { t, j, w, p: rp(rng, 10.0) },
        19 => OpIn::PcSetPresnap { t, j, w, v: if rng.chance(0.3) { BIG } else { rng.uniform(-5.0, 5.0) } },
        20 => OpIn::PcSetSrcTol { t, j, w, tol: gen_tol(rng) },
        21..=22 => OpIn::SetCurve { t, obj: maybe(rng, &curves, 0.1), owned: rng.chance(0.3) },
        23 => OpIn::SetSurface1 { t, obj: maybe(rng, &surfs, 0.2) },
        24 => OpIn::SetSurface2 { t, obj: maybe(rng, &surfs, 0.2) },
        25 => OpIn::SetFace { t, obj: maybe(rng, &of_kind(objs, kind::FACE), 0.3) },
        26 => OpIn::SetIntersectData {
            t,
            s1: maybe(rng, &surfs, 0.2),
            s2: maybe(rng, &surfs, 0.2),
            uv2: maybe(rng, &curves, 0.3),
            current: rng.chance(0.7),
            ti: maybe(rng, &of_kind(objs, kind::OTHER), 0.5),
        },
        27 => OpIn::SetIsCurveCurrent { t, b: rng.chance(0.5) },
        28 => match rng.below(5) {
            0 => OpIn::SetIgnoreTangency { t, b: rng.chance(0.5) },
            1 => OpIn::SetIsCurveOwned { t, b: rng.chance(0.5) },
            2 => OpIn::SetOkToSetTol { t, b: rng.chance(0.5) },
            3 => OpIn::SetTgtLoopsOnly { t, b: rng.chance(0.5) },
            _ => OpIn::SetFilletData {
                t,
                obj: maybe(rng, &of_kind(objs, kind::OTHER), 0.3),
                b: rng.chance(0.7),
            },
        },
        29 => OpIn::AddTgtLoop { t, obj: pick(rng, &of_kind(objs, kind::LOOP)) },
        30 => OpIn::SetSrcTol { t, tol: gen_tol(rng) },
        31 => {
            if rng.chance(0.5) {
                OpIn::SetUvCurve1 { t, obj: maybe(rng, &curves, 0.3) }
            } else {
                OpIn::SetUvCurve2 { t, obj: maybe(rng, &curves, 0.3) }
            }
        }
        32 => OpIn::SubInterval { t, j },
        33..=34 => OpIn::ClassifyCrvLoc { t, j, w, prm: rng.uniform(-2.0, 6.0) },
        35 => OpIn::SetPcParams { t, mids: rng.chance(0.5) },
        36 => OpIn::SqueezeEnd { t, j },
        37 => OpIn::IsReferenced { t, obj: maybe(rng, &all, 0.1) },
        38 => match rng.below(4) {
            0 => OpIn::MakeConsistent { t },
            1 => OpIn::Type { t, types: vec![12312, 11001, 12011, 12308, 0, 16500] },
            2 => OpIn::SrcTol { t },
            _ => OpIn::ObjDim { t, obj: maybe(rng, &all, 0.1) },
        },
        _ => OpIn::IvlTols { t, j },
    }
}

/// A random UV point on surface `s` of the pool (its natural domain, sometimes outside).
fn surf_uv(rng: &mut Rng, obj: &ObjIn) -> [f64; 2] {
    match obj {
        ObjIn::Bsurf(s) => {
            let lim = |k: &[f64]| (k.first().copied().unwrap_or(0.0), k.last().copied().unwrap_or(1.0));
            let (u0, u1) = lim(&s.u);
            let (v0, v1) = lim(&s.v);
            let ext = if rng.chance(0.1) { 0.2 } else { 0.0 };
            [
                rng.uniform(u0 - ext * (u1 - u0), u1 + ext * (u1 - u0)),
                rng.uniform(v0 - ext * (v1 - v0), v1 + ext * (v1 - v0)),
            ]
        }
        _ => [rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0)],
    }
}

/// UV of point `p` on the plane `(o, n)` as `SmPlane(O, N)` parameterizes it.
fn plane_uv(o: [f64; 3], n: [f64; 3], p: [f64; 3]) -> [f64; 2] {
    let pl = Plane::from_normal(v3(o), v3(n));
    let q = pl.project_point_to_uv_domain(v3(p));
    [q.x, q.y]
}

/// A surf/surf/curve setup: two planes through a common line, a piecewise-C0 cubic near
/// that line (or another curve), the UV points of a point on the line.
fn gen_xsect_setup(rng: &mut Rng, objs: &mut Vec<ObjIn>) -> (usize, [usize; 2], [[f64; 2]; 2], f64) {
    let o = rp(rng, 5.0);
    let d = dir(rng);
    let n1 = norm(cross(d, dir(rng)));
    let n2 = if rng.chance(0.1) {
        n1 // tangent planes
    } else {
        norm(cross(d, n1))
    };
    let len = rng.uniform(1.0, 20.0);
    let off = *rng.choose(&[1e-7, 1e-5, 1e-4, 1e-3, 1e-2]).unwrap_or(&1e-3);
    let c = match rng.below(10) {
        0 => gen_curve(rng),
        _ => CurveIn::Bspline(gen_xsect_cubic(rng, o, d, len, off)),
    };
    objs.push(ObjIn::Curve(c));
    let ci = objs.len() - 1;
    objs.push(ObjIn::Plane { o, n: n1 });
    objs.push(ObjIn::Plane { o, n: n2 });
    let s = [objs.len() - 2, objs.len() - 1];
    let f = rng.uniform(-0.1, 1.1);
    let es = *rng.choose(&[0.0, 1e-6, 1e-3]).unwrap_or(&0.0);
    let e = rp(rng, es);
    let p = [o[0] + f * len * d[0] + e[0], o[1] + f * len * d[1] + e[1], o[2] + f * len * d[2] + e[2]];
    let uv = [plane_uv(o, n1, p), plane_uv(o, n2, p)];
    (ci, s, uv, f)
}

#[allow(clippy::too_many_lines)]
fn gen_script(rng: &mut Rng) -> Script {
    let mut objects = gen_objects(rng);
    let mode = rng.below(20);
    let mut ops = Vec::new();
    let mut sizes = [0usize; 2];
    let topo = rng.chance(0.15);
    // set up both classifications most of the time
    for (t, size) in sizes.iter_mut().enumerate() {
        if rng.chance(0.8) {
            let n = 1 + rng.below(4) as usize;
            ops.push(OpIn::SetSize { t, n });
            *size = n;
            let curves = of_kind(&objects, kind::CURVE);
            ops.push(OpIn::SetCurve { t, obj: pick(rng, &curves), owned: rng.chance(0.2) });
            if rng.chance(0.7) {
                ops.push(OpIn::SetSrcTol { t, tol: gen_tol(rng) });
            }
            let mut a = rng.uniform(-1.0, 1.0);
            for j in 0..n {
                let l = rng.uniform(0.1, 2.0);
                ops.push(OpIn::IvlSet { t, j, a, b: a + l });
                a += l;
                for w in 0..3 {
                    if rng.chance(0.6) {
                        let (class, obj) = gen_class(rng, &objects, topo);
                        ops.push(OpIn::PcSetClass { t, j, w, class, obj });
                    }
                }
            }
        }
    }
    match mode {
        // classification containers
        0..=9 => {
            for _ in 0..(2 + rng.below(14)) {
                let t = rng.below(2) as usize;
                let size = sizes.get(t).copied().unwrap_or(0);
                let op = gen_container_op(rng, &objects, t, size, topo);
                if let Some(slot) = sizes.get_mut(t) {
                    match &op {
                        OpIn::SetSize { n, .. } => *slot = *n,
                        OpIn::RemoveAt { .. } => *slot = slot.saturating_sub(1),
                        _ => {}
                    }
                }
                ops.push(op);
            }
        }
        // sm_SetXSectCurveThroughPoint, sm_ApplyPointSolution
        10..=14 => {
            let (ci, s, uv, f) = gen_xsect_setup(rng, &mut objects);
            let t = rng.below(2) as usize;
            let prm = rng.uniform(-1.0, 1.0) + 5.0 * f;
            if rng.chance(0.5) {
                let mut srf = vec![Some(s[0]), Some(s[1])];
                let mut uvs = vec![uv[0], uv[1]];
                match rng.below(10) {
                    0 => {
                        srf.truncate(1);
                        uvs.truncate(1);
                    }
                    1 => {
                        srf.push(Some(s[0]));
                        uvs.push(uv[0]);
                    }
                    2 => uvs.push(uv[1]),
                    3 => {
                        // the B-spline surface of the pool as a second surface
                        let bsurf = of_kind(&objects, kind::BSURF).first().copied();
                        if let Some((b, obj)) = bsurf.and_then(|b| Some((b, objects.get(b)?))) {
                            if let Some(slot) = srf.get_mut(1) {
                                *slot = Some(b);
                            }
                            let q = surf_uv(rng, obj);
                            if let Some(slot) = uvs.get_mut(1) {
                                *slot = q;
                            }
                        }
                    }
                    _ => {}
                }
                ops.push(OpIn::XsectCurve {
                    t,
                    c: Some(ci),
                    tol: gen_tol(rng),
                    uvc: Vec::new(),
                    srf,
                    uv: uvs,
                    prm,
                    dev: -7.0,
                    p: rp(rng, 10.0),
                });
            } else {
                ops.push(OpIn::SetCurve { t, obj: Some(ci), owned: false });
                ops.push(OpIn::SetSrcTol { t, tol: gen_tol(rng) });
                let use_cc = rng.chance(0.9);
                let mut srf = [Some(s[0]), Some(s[1]), None, None];
                let mut uvs = [uv[0], uv[1], [0.0, 0.0], [0.0, 0.0]];
                match rng.below(8) {
                    0 => {
                        srf = [None, None, Some(s[0]), None];
                        uvs[2] = uv[0];
                    }
                    1 => {
                        srf[2] = Some(s[1]);
                        uvs[2] = uv[1];
                    }
                    _ => {}
                }
                let emate = if rng.chance(0.2) { of_kind(&objects, kind::EDGE).first().copied() } else { None };
                ops.push(OpIn::Apply {
                    t,
                    use_cc,
                    edge: None,
                    emate,
                    srf,
                    uv: uvs,
                    sol: [prm, rng.uniform(-1.0, 1.0), rng.uniform(0.0, 1e-3)],
                    sss: rp(rng, 10.0),
                });
            }
            // the edited curve's classification
            if rng.chance(0.5) {
                ops.push(OpIn::ClassifyCrvLoc { t: 0, j: 0, w: 0, prm });
            }
        }
        // angle, tolerance pair, touches, dimensions, types
        15..=17 => {
            let curves = of_kind(&objects, kind::CURVE);
            let surfs: Vec<usize> = of_kind(&objects, kind::PLANE)
                .into_iter()
                .chain(of_kind(&objects, kind::BSURF))
                .collect();
            let all: Vec<usize> = (0..objects.len()).collect();
            for _ in 0..=rng.below(6) {
                let t = rng.below(2) as usize;
                let op = match rng.below(5) {
                    0 | 1 => {
                        let c1 = maybe(rng, &curves, 0.05);
                        let c2 = if rng.chance(0.2) { c1 } else { maybe(rng, &curves, 0.05) };
                        OpIn::Angle {
                            t,
                            c1,
                            t1: rng.uniform(-1.0, 6.0),
                            c2,
                            t2: rng.uniform(-1.0, 6.0),
                            dev: rng.uniform(0.0, 1e-3),
                        }
                    }
                    2 => {
                        // half the pairs take a fresh UV curve on a surface, so the
                        // MapTo2d case (2D curve with m_cpSurface1) is well covered
                        let (c, s) = if rng.chance(0.5) {
                            let s = pick(rng, &surfs);
                            let uvc = gen_uv_line(rng, s.and_then(|i| objects.get(i)));
                            objects.push(ObjIn::Curve(CurveIn::Bspline(uvc)));
                            (Some(objects.len() - 1), s)
                        } else {
                            (pick(rng, &curves), maybe(rng, &surfs, 0.6))
                        };
                        OpIn::TolPair {
                            t,
                            c,
                            s,
                            p1: rng.uniform(-1.0, 6.0),
                            tol1: gen_tol(rng),
                            p2: rng.uniform(-1.0, 6.0),
                            tol2: gen_tol(rng),
                        }
                    }
                    3 => {
                        let nt = 2 * rng.below(6) as usize;
                        let loops = of_kind(&objects, kind::LOOP);
                        let touches = (0..nt)
                            .map(|_| TouchIn {
                                c: maybe(rng, &loops, 0.1),
                                dir: rng.below(9) as i32,
                                li: rng.below(4),
                                prm: rng.uniform(0.0, 1.0),
                                pi: rng.below(8),
                                pc: maybe(rng, &all, 0.5),
                            })
                            .collect();
                        if rng.chance(0.5) {
                            OpIn::TouchWinding { t, touches }
                        } else {
                            OpIn::TouchConnected { t, c: maybe(rng, &loops, 0.1), touches }
                        }
                    }
                    _ => OpIn::Gaps {
                        t,
                        cp: rp(rng, 5.0),
                        ep: rp(rng, 5.0),
                        sol: [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
                        pairs: if rng.chance(0.8) {
                            Vec::new()
                        } else {
                            vec![(pick(rng, &surfs), None)]
                        },
                        count: rng.below(3) as usize,
                    },
                };
                ops.push(op);
            }
        }
        // sm_CullOrRefineCCIIn3d
        _ => {
            let x = dir(rng);
            let y = norm(cross(x, dir(rng)));
            let sz = rng.uniform(2.0, 20.0);
            let o = rp(rng, 3.0);
            let bump = rng.uniform(0.0, 0.5);
            let srf = gen_surface(rng, o, x, y, sz, bump);
            let (u0, u1) = (srf.u.first().copied().unwrap_or(0.0), srf.u.last().copied().unwrap_or(1.0));
            let (v0, v1) = (srf.v.first().copied().unwrap_or(0.0), srf.v.last().copied().unwrap_or(1.0));
            objects.push(ObjIn::Bsurf(srf));
            let si = objects.len() - 1;
            // a 2D B-spline across the domain
            let pa = [rng.uniform(u0, u1), rng.uniform(v0, v1)];
            let pb = [rng.uniform(u0, u1), rng.uniform(v0, v1)];
            let deg = 1 + rng.below(2) as usize;
            let cnt = deg + 1 + rng.below(2) as usize;
            let uvc = BSplineIn {
                dim: 2,
                oob: false,
                cpts: (0..cnt)
                    .map(|i| {
                        let f = i as f64 / (cnt - 1) as f64;
                        [
                            pa[0] + f * (pb[0] - pa[0]) + rng.uniform(-0.05, 0.05) * (u1 - u0),
                            pa[1] + f * (pb[1] - pa[1]) + rng.uniform(-0.05, 0.05) * (v1 - v0),
                            NOZ,
                            NOW,
                        ]
                    })
                    .collect(),
                p: deg,
                u: {
                    let mut k = vec![0.0; deg + 1];
                    for i in 1..(cnt - deg) {
                        k.push(i as f64 / (cnt - deg) as f64);
                    }
                    k.extend(vec![1.0; deg + 1]);
                    k
                },
            };
            objects.push(ObjIn::Curve(CurveIn::Bspline(uvc)));
            let ui = objects.len() - 1;
            let c2 = CurveIn::Line(LineIn {
                p: rp(rng, 5.0),
                v: rp(rng, 3.0),
                domain: [0.0, rng.uniform(1.0, 10.0)],
                scale: 1.0,
                inside_out: false,
            });
            objects.push(ObjIn::Curve(if rng.chance(0.5) { c2 } else { gen_curve(rng) }));
            let c2i = objects.len() - 1;
            let n = rng.below(6) as usize;
            let sols = (0..n)
                .map(|_| {
                    let ty = *rng.choose(&[1usize, 1, 1, 2, 2, 0, 4]).unwrap_or(&1);
                    SolIn {
                        ty,
                        s: [rng.uniform(0.0, 1.0), rng.uniform(0.0, 5.0)],
                        e: [rng.uniform(0.0, 1.0), rng.uniform(0.0, 5.0)],
                        value: rng.uniform(0.0, 0.5),
                    }
                })
                .collect();
            ops.push(OpIn::Cull {
                t: 0,
                s: Some(if rng.chance(0.1) { of_kind(&objects, kind::PLANE).first().copied().unwrap_or(si) } else { si }),
                uvc: Some(ui),
                ivl1: [0.0, 1.0],
                c2: Some(c2i),
                ivl2: [0.0, 10.0],
                tol: *rng.choose(&[1e-6, 1e-3, 0.1, 1.0, 5.0]).unwrap_or(&0.1),
                sols,
            });
        }
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

/// Edge cases: empty / huge-scale / tiny-scale classifications, boundary indices, every
/// winding orientation, degenerate tangents.
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let line = |p: [f64; 3], v: [f64; 3], d: [f64; 2]| {
        ObjIn::Curve(CurveIn::Line(LineIn { p, v, domain: d, scale: 1.0, inside_out: false }))
    };
    for (name, scale) in [("unit", 1.0), ("huge", 1e5), ("tiny", 1e-5)] {
        let objs = vec![
            line([0.0, 0.0, 0.0], [scale, 0.0, 0.0], [0.0, 1.0]),
            ObjIn::Plane { o: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] },
            ObjIn::Point([scale, scale, 0.0]),
            ObjIn::Loop,
            ObjIn::Other,
        ];
        let ops = vec![
            OpIn::SrcTol { t: 0 },
            OpIn::Type { t: 0, types: vec![12312, 11001, 0] },
            OpIn::SetSize { t: 0, n: 0 },
            OpIn::SetSize { t: 0, n: 3 },
            OpIn::IvlTols { t: 0, j: 2 },
            OpIn::SetCurve { t: 0, obj: Some(0), owned: true },
            OpIn::SetSrcTol { t: 0, tol: 1e-5 * scale },
            OpIn::IvlSet { t: 0, j: 0, a: 0.0, b: 0.25 },
            OpIn::IvlSet { t: 0, j: 1, a: 0.25, b: 0.75 },
            OpIn::IvlSet { t: 0, j: 2, a: 0.75, b: 1.0 },
            OpIn::IvlSetMin { t: 0, j: 1, v: 0.8 },
            OpIn::IvlSetMin { t: 0, j: 1, v: 0.3 },
            OpIn::IvlSetMax { t: 0, j: 1, v: 0.2 },
            OpIn::IvlSetMax { t: 0, j: 1, v: 0.75 },
            OpIn::PcSetClass { t: 0, j: 0, w: 0, class: 107, obj: Some(1) },
            OpIn::PcSetClass { t: 0, j: 0, w: 2, class: 106, obj: Some(0) },
            OpIn::PcSetClass { t: 0, j: 1, w: 1, class: 105, obj: Some(2) },
            OpIn::PcSetSss { t: 0, j: 2, w: 2, p: [scale, 2.0 * scale, 3.0 * scale] },
            OpIn::ClassifyCrvLoc { t: 0, j: 2, w: 2, prm: 0.5 },
            OpIn::ClassifyCrvLoc { t: 0, j: 2, w: 0, prm: 0.5 },
            OpIn::ClassifyCrvLoc { t: 0, j: 0, w: 0, prm: -3.0 },
            OpIn::SqueezeEnd { t: 0, j: 0 },
            OpIn::SqueezeEnd { t: 0, j: 2 },
            OpIn::IsReferenced { t: 0, obj: Some(0) },
            OpIn::IsReferenced { t: 0, obj: Some(4) },
            OpIn::IsReferenced { t: 0, obj: None },
            OpIn::SubInterval { t: 0, j: 2 },
            OpIn::RemoveAt { t: 0, j: 9 },
            OpIn::RemoveAt { t: 0, j: 0 },
            OpIn::SetSize { t: 0, n: 4 },
            OpIn::SetSize { t: 1, n: 1 },
            OpIn::IvlAssign { t: 1, j: 0, k: 0 },
            OpIn::IvlAssign { t: 1, j: 0, k: 1 },
            OpIn::IvlNew { t: 1, j: 0, with: true, owner: true, a: 2.0, b: 1.0 },
            OpIn::IvlNew { t: 0, j: 3, with: false, owner: true, a: 0.0, b: 0.0 },
            OpIn::IvlReset { t: 0, j: 3 },
            OpIn::SetIntersectData { t: 0, s1: Some(1), s2: Some(1), uv2: Some(0), current: false, ti: Some(4) },
            OpIn::SetUvCurve1 { t: 0, obj: Some(0) },
            OpIn::SetIsCurveCurrent { t: 0, b: true },
            OpIn::SetIsCurveCurrent { t: 0, b: false },
            OpIn::SetFilletData { t: 0, obj: Some(4), b: true },
            OpIn::AddTgtLoop { t: 0, obj: Some(3) },
            OpIn::AddTgtLoop { t: 0, obj: Some(3) },
            OpIn::SetTgtLoopsOnly { t: 0, b: true },
            OpIn::MakeConsistent { t: 0 },
            OpIn::ObjDim { t: 0, obj: Some(3) },
            OpIn::ObjDim { t: 0, obj: None },
            OpIn::SetSize { t: 0, n: 0 },
            OpIn::SetPcParams { t: 0, mids: true },
            OpIn::SetSize { t: 0, n: 2 },
            OpIn::SetPcParams { t: 0, mids: false },
        ];
        out.push((format!("edge_containers_{name}"), json!(Script { objects: objs, ops })));
    }
    // winding counts: in/out excess, out/in excess, balanced, empty
    let loop_objs = vec![ObjIn::Loop, ObjIn::Loop, ObjIn::Other];
    let tch = |c: usize, dir: i32, prm: f64| TouchIn { c: Some(c), dir, li: 0, prm, pi: 0, pc: None };
    for (name, list) in [
        ("in_out", vec![tch(0, 2, 0.1), tch(0, 4, 0.1), tch(0, 2, 0.5), tch(0, 4, 0.5)]),
        ("out_in", vec![tch(0, 4, 0.1), tch(0, 2, 0.1), tch(1, 3, 0.2), tch(1, 6, 0.2)]),
        ("balanced", vec![tch(0, 2, 0.1), tch(0, 4, 0.1), tch(1, 4, 0.5), tch(1, 2, 0.5)]),
        ("empty", vec![]),
    ] {
        let ops = vec![
            OpIn::TouchWinding { t: 0, touches: list.clone() },
            OpIn::TouchConnected { t: 0, c: Some(0), touches: list.clone() },
            OpIn::TouchConnected { t: 1, c: None, touches: list.clone() },
        ];
        out.push((format!("edge_touches_{name}"), json!(Script { objects: loop_objs.clone(), ops })));
    }
    // degenerate tangents and NULL curves for the angle
    let objs = vec![
        line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        line([0.0, 0.0, 0.0], [-1.0, 1e-9, 0.0], [0.0, 1.0]),
        ObjIn::Curve(CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: [0.0; 3],
            d1: [0.0; 3],
            p2: [0.0; 3],
            d2: [0.0; 3],
        })),
        line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0]),
    ];
    let ops = vec![
        OpIn::Angle { t: 0, c1: Some(0), t1: 0.5, c2: Some(1), t2: 0.5, dev: 0.0 },
        OpIn::Angle { t: 0, c1: Some(0), t1: 0.5, c2: Some(3), t2: 0.5, dev: 0.0 },
        OpIn::Angle { t: 0, c1: Some(2), t1: 0.5, c2: Some(0), t2: 0.5, dev: 0.0 },
        OpIn::Angle { t: 0, c1: Some(0), t1: 0.5, c2: Some(2), t2: 0.5, dev: 0.0 },
        OpIn::Angle { t: 0, c1: None, t1: 0.5, c2: Some(2), t2: 0.5, dev: 0.0 },
        OpIn::TolPair { t: 0, c: Some(0), s: None, p1: 0.0, tol1: 1e-5, p2: 1.0, tol2: 2e-5 },
        OpIn::TolPair { t: 0, c: Some(0), s: None, p1: 0.0, tol1: USE_DEFAULT, p2: 1.0, tol2: -1e-3 },
        OpIn::Gaps { t: 0, cp: [0.0; 3], ep: [3.0, 4.0, 0.0], sol: [0.0, 0.0], pairs: vec![], count: 0 },
    ];
    out.push(("edge_angles".into(), json!(Script { objects: objs, ops })));
    // the topology helpers (deferred in the port)
    let objs = vec![
        line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0]),
        ObjIn::Plane { o: [0.0, 0.0, 0.0], n: [0.0, 0.0, 1.0] },
        ObjIn::Plane { o: [0.0, 0.0, 0.0], n: [0.0, 1.0, 0.0] },
        ObjIn::Edge(Some(0)),
        ObjIn::Face(Some(1)),
    ];
    for (name, op) in [
        (
            "edge_through",
            OpIn::EdgeThrough {
                t: 0,
                e: Some(3),
                p: [0.5, 0.0, 0.0],
                srf: [Some(1), Some(2)],
                uv: [[0.5, 0.0], [0.5, 0.0]],
                prm: 0.4,
            },
        ),
        ("cci_geometry", OpIn::CciGeometry { t: 0, e: Some(3) }),
        (
            "face_point_class",
            OpIn::FacePointClass { t: 0, f: Some(4), p: [0.5, 0.5, 0.0], tol: 1e-5 },
        ),
    ] {
        let ops = vec![
            OpIn::SetIntersectData { t: 0, s1: Some(1), s2: Some(2), uv2: None, current: true, ti: None },
            op,
        ];
        out.push((format!("edge_topology_{name}"), json!(Script { objects: objs.clone(), ops })));
    }
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p1"
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

    /// Generated scripts are accepted by the ffi checks, and most of them are comparable
    /// now (not deferred).
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
        assert!(deferred * 3 < total, "{deferred} of {total} deferred");
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Prints how often the generated scripts reach the interesting branches of the static
    /// helpers (a debugging aid: `cargo test -p difftest --features cpp --lib
    /// coverage_stats -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid"]
    fn coverage_stats() {
        let k = Kernel::default();
        let mut stats: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::default();
        let mut bump = |s: String| *stats.entry(s).or_default() += 1;
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("intersect.curve_class_p1", i));
            let Some(v) = Runner.generate(&mut rng) else { continue };
            let s: Script = serde_json::from_value(v).unwrap();
            let mut pool = build_pool(&s.objects).unwrap();
            for x in &s.ops {
                match x {
                    OpIn::XsectCurve { c, tol, srf, uv, prm, p, .. } => {
                        let mut curve = pool.take_curve(c.unwrap()).unwrap();
                        let surfaces: Vec<&Surface> =
                            srf.iter().map(|x| pool.surface_at(*x).unwrap().unwrap()).collect();
                        let uvs: Vec<Point2> = uv.iter().map(|q| uvp(*q)).collect();
                        let r = cc_mod::set_xsect_curve_through_point(
                            &k, &mut curve, zone_tol3d(&k, *tol), &mut [], &surfaces, &uvs, *prm, v3(*p),
                        );
                        bump(match r {
                            Ok(r) => format!(
                                "xsect success={} modified={} dev={:?}",
                                r.success,
                                r.modified_curve,
                                r.deviation.map(|d| d == 0.0)
                            ),
                            Err(e) => format!("xsect err {e}"),
                        });
                    }
                    OpIn::Apply { .. } => bump("apply".into()),
                    OpIn::Cull { sols, .. } => bump(format!("cull n={}", sols.len().min(1))),
                    OpIn::Angle { .. } => bump("angle".into()),
                    OpIn::TolPair { .. } => bump("tol_pair".into()),
                    OpIn::TouchWinding { .. } | OpIn::TouchConnected { .. } => bump("touch".into()),
                    OpIn::SetPcParams { .. } => bump("set_pc_params".into()),
                    OpIn::SqueezeEnd { .. } => bump("squeeze".into()),
                    _ => {}
                }
            }
        }
        for (k, v) in stats {
            println!("{v:6} {k}");
        }
    }

    /// Prints generated case `CC_CASE` as JSON (a debugging aid:
    /// `CC_CASE=6 cargo test -p difftest --features cpp --lib dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs CC_CASE"]
    fn dump_case() {
        let i: usize = std::env::var("CC_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("intersect.curve_class_p1", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
    }
}
