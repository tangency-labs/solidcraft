//! Runner for unit `blend.fillet_geom_p1`: the fillet vertex / edge / edge use / vertex
//! use / geometry / pseudo-B-rep records of `SmFilletGeom.cpp` and its static geometry
//! helpers.
//!
//! C++: `solidcore_ffi::units::blend_fillet_geom_p1` (upstream
//! `source/SMLib/src/SmFilletGeom.cpp`). Rust: `solidcore::blend::fillet_geom::p1`.
//!
//! A case is one geometry op (`cleanup_noise`, `check_seam_out`, `check_seam_on`,
//! `surface_closure`, `is_order_correct`, `adjust_uv_on_seam`, `interpolate`,
//! `surface_drop_point`, `two_pnts`, `evaluate_mate`, `capping`, `classify_face_point`,
//! `evaluate_mate_analytic`: `sm_EvaluateMate` on a real `SmPlane` or `SmSurfOfRevolution`)
//! with its inputs, or a pseudo-B-rep op (`fb`): a model (target wire edges and faces,
//! fillet B-rep wire edges, fillet vertices, geometries, edges with their edge and
//! vertex uses) and one query on it. Both sides produce the same flat record (layout in
//! the FFI module docs), split into keys by [`to_output`]: `status` (`0` = `SM_SUCCESS`,
//! else the `SmStatus`: `1001` `SM_ERR`, `1006` `SM_ERR_NULL_POINTER`, `1007`
//! `SM_ERR_INVALID_INPUT`, `1027` `SM_ERR_BAD_INTERSECTIONS`; `-1` on the Rust side for an
//! error upstream cannot produce), the op's values, and for the pseudo-B-rep queries that
//! change the model its export (`v_*`, `e_*`, `g_*` keys).
//!
//! Faces (the model's target faces, `classify_face_point`'s, `evaluate_mate`'s) are built
//! on both sides as the shim's `face_of` builds them (`natural_face`).
//!
//! Deferred on the Rust side (unported callees): `two_pnts` past its point drops
//! (`intersect.surface_intersector_p4`), `classify_face_point` and `fb` `split_face` of
//! a vertex classified to the split face (`SmSurface::DropCurve` of
//! `intersect.surface_drop_lib_p3`, making the faces' UV trim curves in
//! `CalculateUVDomainFromUVTrimCurves`), and `fb` `cliff`
//! (`intersect.surface_drop_lib_p1`'s own unwired `SmSurfaceDropCurve::DoTrace`, whose
//! stub prints `intersect.surface_drop_curve`). `fb` `examine` past its pre-checks is
//! refused by the shim (no `SmFilletSolver`; see the FFI module). Fixtures flagged
//! `expected_rejection` are regression inputs the FFI or the shim must refuse (upstream
//! crashes or hangs: HANG-29, UB-166, the examine path); every other refusal is a
//! harness error; HANG-20 bounds the four global-solver ops at `MAX_SOLVER_MAGNITUDE`).
//! Every run has at least [`MIN_GENERATED`] generated cases.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::util::{flag, v3};

use solidcore::blend::fillet_geom::p1::{
    self as fg, FilStatus, FilletBrep, FilletCornerId, FilletEdgeType, FilletEdgeuse, FilletGeom,
    FilletGeomId, FilletGeomType, FilletVertex, FilletVertexType, FilletVertexuse,
};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::attribute::AttributeStore;
use solidcore::geom::tol::{USE_DEFAULT, ZONE_TOL_3D};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector2d::Vec2;
use solidcore::intersect::point_class::PointClassType;
use solidcore::intersect::surface_intersector::p1::TsectPnt;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    Brep, EdgeId, Edgeuse, EdgeuseId, FaceId, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::blend_fillet_geom_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 300;

/// `blend.fillet_geom_p1` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// An inline curve: dimension, control points as stored (`[x, y, z, w]`), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Crv {
    #[serde(default = "three")]
    pub(crate) dim: u64,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

fn three() -> u64 {
    3
}

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Srf {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VertexIn {
    pub(crate) point: [f64; 3],
    #[serde(default)]
    pub(crate) vtype: i64,
    #[serde(default)]
    pub(crate) status: i64,
    #[serde(default = "minus_one")]
    pub(crate) corner: i64,
    #[serde(default = "pc_unknown")]
    pub(crate) pc_class: i64,
    /// `0` none, `1` the start/end (`pc_sub`) vertex of target wire edge `pc_index`, `2`
    /// its primary edge use, `3` target face `pc_index`, `4` the target wire edge itself
    /// (an `SmEdge`, what `SM_PC_EDGE` classifies to upstream), `5` edge use `pc_sub` of
    /// the first loop of target face `pc_index` (an edge use with a face).
    #[serde(default)]
    pub(crate) pc_kind: i64,
    #[serde(default = "minus_one")]
    pub(crate) pc_index: i64,
    #[serde(default)]
    pub(crate) pc_sub: i64,
    #[serde(default = "minus_one")]
    pub(crate) pc_face: i64,
    #[serde(default)]
    pub(crate) t_param: f64,
    #[serde(default)]
    pub(crate) uv: [f64; 2],
    #[serde(default = "no_mates")]
    pub(crate) mates: [i64; 2],
}

fn minus_one() -> i64 {
    -1
}

fn pc_unknown() -> i64 {
    100
}

fn no_mates() -> [i64; 2] {
    [-1, -1]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EuIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) uv_curve: Option<Crv>,
    #[serde(default = "minus_one")]
    pub(crate) geom: i64,
    #[serde(default)]
    pub(crate) vu_uv: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EdgeIn {
    pub(crate) vs: usize,
    pub(crate) ve: usize,
    #[serde(default = "edge_unknown")]
    pub(crate) etype: i64,
    #[serde(default)]
    pub(crate) status: i64,
    #[serde(default = "minus_one")]
    pub(crate) corner: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) curve: Option<Crv>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) interval: Option<[f64; 2]>,
    #[serde(default = "no_mates")]
    pub(crate) fbe: [i64; 2],
    pub(crate) eus: [EuIn; 2],
}

fn edge_unknown() -> i64 {
    14
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GeomIn {
    #[serde(default)]
    pub(crate) gtype: i64,
    #[serde(default)]
    pub(crate) status: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) center: Option<Crv>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) surface: Option<Srf>,
    #[serde(default = "no_mates")]
    pub(crate) rails: [i64; 2],
    #[serde(default)]
    pub(crate) other1: Vec<usize>,
    #[serde(default)]
    pub(crate) other2: Vec<usize>,
    #[serde(default)]
    pub(crate) sides: Vec<[usize; 2]>,
}

/// A fillet B-rep wire edge: its curve and the edge tolerance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FbEdgeIn {
    pub(crate) curve: Crv,
    #[serde(default = "zone_tol")]
    pub(crate) tol: f64,
}

fn zone_tol() -> f64 {
    1.0e-5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Model {
    #[serde(default)]
    pub(crate) targets: Vec<Crv>,
    #[serde(default)]
    pub(crate) faces: Vec<Srf>,
    #[serde(default)]
    pub(crate) fillet_brep: Vec<FbEdgeIn>,
    #[serde(default)]
    pub(crate) vertices: Vec<VertexIn>,
    #[serde(default)]
    pub(crate) geoms: Vec<GeomIn>,
    #[serde(default)]
    pub(crate) edges: Vec<EdgeIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
enum Query {
    Export,
    VertexGeoms {
        v: usize,
    },
    VuAtRailEnd {
        v: usize,
        #[serde(default = "minus_one")]
        geom: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclude: Option<[usize; 2]>,
        #[serde(default = "minus_one")]
        rail: i64,
    },
    RailVertexuse {
        g: usize,
        rail: usize,
        end: usize,
    },
    EdgeName {
        e: usize,
    },
    FindEdge {
        e: usize,
        multi: bool,
        cands: Vec<usize>,
    },
    ClearEdges {
        e: usize,
        edges: Vec<usize>,
    },
    BrepEdges {
        e: usize,
    },
    Trim {
        g: usize,
        interval: [f64; 2],
    },
    DeleteVertex {
        v: usize,
    },
    DeleteEdge {
        e: usize,
    },
    VertOnVert {
        v: usize,
    },
    Cliff {
        v: usize,
        surface: Srf,
        tol: f64,
    },
    SplitFace {
        v: usize,
        f1: usize,
        f2: usize,
    },
    Examine {
        g1: usize,
        at1: bool,
        g2: usize,
        at2: bool,
        sv: usize,
        ev: usize,
        tol: f64,
    },
    SetEuGeom {
        e: usize,
        eu: usize,
        #[serde(default = "minus_one")]
        geom: i64,
    },
    SetUvCurve {
        e: usize,
        eu: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curve: Option<Crv>,
        #[serde(default)]
        delete: bool,
        #[serde(default)]
        warn: bool,
    },
    OrigUvCurve {
        e: usize,
    },
    SetVuProperty {
        e: usize,
        eu: usize,
        te: usize,
        teu: usize,
    },
    RemoveEdge {
        e: usize,
        #[serde(default)]
        readd: bool,
    },
}

impl Query {
    fn op(&self) -> Op {
        match self {
            Query::Export => Op::FbExport,
            Query::VertexGeoms { .. } => Op::FbVertexGeoms,
            Query::VuAtRailEnd { .. } => Op::FbVuAtRailEnd,
            Query::RailVertexuse { .. } => Op::FbRailVertexuse,
            Query::EdgeName { .. } => Op::FbEdgeName,
            Query::FindEdge { .. } => Op::FbFindEdge,
            Query::ClearEdges { .. } => Op::FbClearEdges,
            Query::BrepEdges { .. } => Op::FbBrepEdges,
            Query::Trim { .. } => Op::FbTrim,
            Query::DeleteVertex { .. } => Op::FbDeleteVertex,
            Query::DeleteEdge { .. } => Op::FbDeleteEdge,
            Query::VertOnVert { .. } => Op::FbVertOnVert,
            Query::Cliff { .. } => Op::FbCliff,
            Query::SplitFace { .. } => Op::FbSplitFace,
            Query::Examine { .. } => Op::FbExamine,
            Query::SetEuGeom { .. } => Op::FbSetEuGeom,
            Query::SetUvCurve { .. } => Op::FbSetUvCurve,
            Query::OrigUvCurve { .. } => Op::FbOrigUvCurve,
            Query::SetVuProperty { .. } => Op::FbSetVuProperty,
            Query::RemoveEdge { .. } => Op::FbRemoveEdge,
        }
    }
}

/// A case's inputs: the program, and whether the C++ side must refuse it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CaseInputs {
    /// The FFI wrapper or the shim must refuse this program before the operation runs
    /// (a regression input for a C++ crash or hang): the case is recorded as rejected
    /// and never compared. A refusal of an unflagged case, or a flagged case that runs,
    /// is a harness error, so over-rejection stays visible.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    expected_rejection: bool,
    #[serde(flatten)]
    inputs: Inputs,
}

/// A real analytic surface for `EvaluateMateAnalytic` (`sm_EvaluateMate` branches on
/// `SmPlane` and `SmSurfOfRevolution`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AnalyticSrf {
    /// `SmPlane(O, X, Y, UVScale, UVDomain)`, then `MakeNurb` when `make_nurb`.
    Plane {
        origin: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        uv_scale: [f64; 2],
        /// `[[umin, vmin], [umax, vmax]]`.
        domain: [[f64; 2]; 2],
        make_nurb: bool,
    },
    /// `SmSurfOfRevolution::CreateCanonical` of the line `a b` (its NURB made) about the
    /// axis through `axis_point` along `axis_dir`.
    Revolution {
        a: [f64; 3],
        b: [f64; 3],
        axis_point: [f64; 3],
        axis_dir: [f64; 3],
        swap: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    CleanupNoise {
        uv: [f64; 2],
    },
    CheckSeamOut {
        surface: Srf,
        tol: f64,
        closure: i64,
        uv: [f64; 2],
    },
    CheckSeamOn {
        surface: Srf,
        tol: f64,
        closure: i64,
        uv: [f64; 2],
    },
    SurfaceClosure {
        surface: Srf,
        /// `[[umin, vmin], [umax, vmax]]`.
        domain: [[f64; 2]; 2],
    },
    IsOrderCorrect {
        /// Per point `[pos, deriv]`.
        pnts: Vec<[[f64; 3]; 2]>,
        i1: usize,
        i2: usize,
    },
    AdjustUvOnSeam {
        surface: Srf,
        ref_dir: [f64; 3],
        uv: [f64; 2],
    },
    Interpolate {
        start: [f64; 3],
        end: [f64; 3],
        svec: [f64; 3],
        evec: [f64; 3],
        dim: u64,
    },
    SurfaceDropPoint {
        surface: Srf,
        pnt: [f64; 3],
        tol: f64,
    },
    TwoPnts {
        surface1: Srf,
        surface2: Srf,
        start: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<[f64; 3]>,
        dir: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_dir: Option<[f64; 3]>,
        tol: f64,
    },
    EvaluateMate {
        surface: Srf,
        uv: [f64; 2],
    },
    Capping {
        tol: f64,
        surface: Srf,
        uv1: Crv,
        orient1: i64,
        uv2: Crv,
        orient2: i64,
    },
    ClassifyFacePoint {
        surface1: Srf,
        surface2: Srf,
        pos: [f64; 3],
    },
    EvaluateMateAnalytic {
        surface: AnalyticSrf,
        uv: [f64; 2],
    },
    Fb {
        model: Model,
        query: Query,
    },
}

impl Inputs {
    fn op(&self) -> Op {
        match self {
            Inputs::CleanupNoise { .. } => Op::CleanupNoise,
            Inputs::CheckSeamOut { .. } => Op::CheckSeamOut,
            Inputs::CheckSeamOn { .. } => Op::CheckSeamOn,
            Inputs::SurfaceClosure { .. } => Op::SurfaceClosure,
            Inputs::IsOrderCorrect { .. } => Op::IsOrderCorrect,
            Inputs::AdjustUvOnSeam { .. } => Op::AdjustUvOnSeam,
            Inputs::Interpolate { .. } => Op::Interpolate,
            Inputs::SurfaceDropPoint { .. } => Op::SurfaceDropPoint,
            Inputs::TwoPnts { .. } => Op::TwoPnts,
            Inputs::EvaluateMate { .. } => Op::EvaluateMate,
            Inputs::Capping { .. } => Op::Capping,
            Inputs::ClassifyFacePoint { .. } => Op::ClassifyFacePoint,
            Inputs::EvaluateMateAnalytic { .. } => Op::EvaluateMateAnalytic,
            Inputs::Fb { query, .. } => query.op(),
        }
    }
}

fn shape(s: &Srf) -> Result<(usize, usize), HarnessError> {
    let n = s
        .net
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

// ---- C++ side: flattening ----------------------------------------------------------------

pub(crate) fn push_srf(o: &mut Vec<f64>, s: &Srf) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend(
        [
            n,
            m,
            s.p,
            s.q,
            s.u.len().saturating_sub(1),
            s.v.len().saturating_sub(1),
        ]
        .map(|x| x as f64),
    );
    for row in &s.net {
        for p in row {
            o.extend_from_slice(p);
        }
    }
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

pub(crate) fn push_crv(o: &mut Vec<f64>, c: &Crv) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([
        c.dim as f64,
        n as f64,
        c.p as f64,
        c.u.len().saturating_sub(1) as f64,
    ]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn push_opt_crv(o: &mut Vec<f64>, c: Option<&Crv>) {
    o.push(flag(c.is_some()));
    if let Some(c) = c {
        push_crv(o, c);
    }
}

pub(crate) fn push_model(o: &mut Vec<f64>, m: &Model) -> Result<(), HarnessError> {
    o.push(m.targets.len() as f64);
    for c in &m.targets {
        push_crv(o, c);
    }
    o.push(m.faces.len() as f64);
    for s in &m.faces {
        push_srf(o, s)?;
    }
    o.push(m.fillet_brep.len() as f64);
    for e in &m.fillet_brep {
        push_crv(o, &e.curve);
        o.push(e.tol);
    }
    o.push(m.vertices.len() as f64);
    for v in &m.vertices {
        o.extend_from_slice(&v.point);
        o.extend(
            [
                v.vtype, v.status, v.corner, v.pc_class, v.pc_kind, v.pc_index, v.pc_sub, v.pc_face,
            ]
            .map(|x| x as f64),
        );
        o.push(v.t_param);
        o.extend_from_slice(&v.uv);
    }
    o.push(m.geoms.len() as f64);
    for g in &m.geoms {
        o.extend([g.gtype as f64, g.status as f64]);
        push_opt_crv(o, g.center.as_ref());
        o.push(flag(g.surface.is_some()));
        if let Some(s) = &g.surface {
            push_srf(o, s)?;
        }
    }
    o.push(m.edges.len() as f64);
    for e in &m.edges {
        o.extend([
            e.vs as f64,
            e.ve as f64,
            e.etype as f64,
            e.status as f64,
            e.corner as f64,
        ]);
        push_opt_crv(o, e.curve.as_ref());
        o.push(flag(e.interval.is_some()));
        if let Some([a, b]) = e.interval {
            o.extend([a, b]);
        }
        o.extend(e.fbe.map(|x| x as f64));
        for eu in &e.eus {
            push_opt_crv(o, eu.uv_curve.as_ref());
            o.push(eu.geom as f64);
        }
        for eu in &e.eus {
            o.extend_from_slice(&eu.vu_uv);
        }
    }
    for g in &m.geoms {
        o.extend(g.rails.map(|x| x as f64));
        o.push(g.other1.len() as f64);
        o.extend(g.other1.iter().map(|&x| x as f64));
        o.push(g.other2.len() as f64);
        o.extend(g.other2.iter().map(|&x| x as f64));
        o.push(g.sides.len() as f64);
        for [e, eu] in &g.sides {
            o.extend([*e as f64, *eu as f64]);
        }
    }
    for v in &m.vertices {
        o.extend(v.mates.map(|x| x as f64));
    }
    Ok(())
}

fn push_query(o: &mut Vec<f64>, q: &Query) -> Result<(), HarnessError> {
    match q {
        Query::Export => {}
        Query::VuAtRailEnd {
            v,
            geom,
            exclude,
            rail,
        } => {
            o.extend([*v as f64, *geom as f64]);
            match exclude {
                Some([e, w]) => o.extend([*e as f64, *w as f64]),
                None => o.extend([-1.0, -1.0]),
            }
            o.push(*rail as f64);
        }
        Query::RailVertexuse { g, rail, end } => {
            o.extend([*g as f64, *rail as f64, *end as f64]);
        }
        Query::EdgeName { e } | Query::BrepEdges { e } | Query::OrigUvCurve { e } => {
            o.push(*e as f64);
        }
        Query::FindEdge { e, multi, cands } => {
            o.extend([*e as f64, flag(*multi), cands.len() as f64]);
            o.extend(cands.iter().map(|&x| x as f64));
        }
        Query::ClearEdges { e, edges } => {
            o.extend([*e as f64, edges.len() as f64]);
            o.extend(edges.iter().map(|&x| x as f64));
        }
        Query::Trim { g, interval } => {
            o.push(*g as f64);
            o.extend_from_slice(interval);
        }
        Query::VertexGeoms { v } | Query::DeleteVertex { v } | Query::VertOnVert { v } => {
            o.push(*v as f64);
        }
        Query::DeleteEdge { e } => o.push(*e as f64),
        Query::Cliff { v, surface, tol } => {
            o.push(*v as f64);
            push_srf(o, surface)?;
            o.push(*tol);
        }
        Query::SplitFace { v, f1, f2 } => o.extend([*v as f64, *f1 as f64, *f2 as f64]),
        Query::Examine {
            g1,
            at1,
            g2,
            at2,
            sv,
            ev,
            tol,
        } => {
            o.extend([
                *g1 as f64,
                flag(*at1),
                *g2 as f64,
                flag(*at2),
                *sv as f64,
                *ev as f64,
                *tol,
            ]);
        }
        Query::SetEuGeom { e, eu, geom } => o.extend([*e as f64, *eu as f64, *geom as f64]),
        Query::SetUvCurve {
            e,
            eu,
            curve,
            delete,
            warn,
        } => {
            o.extend([*e as f64, *eu as f64]);
            push_opt_crv(o, curve.as_ref());
            o.extend([flag(*delete), flag(*warn)]);
        }
        Query::SetVuProperty { e, eu, te, teu } => {
            o.extend([*e as f64, *eu as f64, *te as f64, *teu as f64]);
        }
        Query::RemoveEdge { e, readd } => o.extend([*e as f64, flag(*readd)]),
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    match i {
        Inputs::CleanupNoise { uv } => o.extend_from_slice(uv),
        Inputs::CheckSeamOut {
            surface,
            tol,
            closure,
            uv,
        }
        | Inputs::CheckSeamOn {
            surface,
            tol,
            closure,
            uv,
        } => {
            push_srf(&mut o, surface)?;
            o.extend([*tol, *closure as f64]);
            o.extend_from_slice(uv);
        }
        Inputs::SurfaceClosure { surface, domain } => {
            push_srf(&mut o, surface)?;
            o.extend([domain[0][0], domain[0][1], domain[1][0], domain[1][1]]);
        }
        Inputs::IsOrderCorrect { pnts, i1, i2 } => {
            o.push(pnts.len() as f64);
            for [p, d] in pnts {
                o.extend_from_slice(p);
                o.extend_from_slice(d);
            }
            o.extend([*i1 as f64, *i2 as f64]);
        }
        Inputs::AdjustUvOnSeam {
            surface,
            ref_dir,
            uv,
        } => {
            push_srf(&mut o, surface)?;
            o.extend_from_slice(ref_dir);
            o.extend_from_slice(uv);
        }
        Inputs::Interpolate {
            start,
            end,
            svec,
            evec,
            dim,
        } => {
            o.extend_from_slice(start);
            o.extend_from_slice(end);
            o.extend_from_slice(svec);
            o.extend_from_slice(evec);
            o.push(*dim as f64);
        }
        Inputs::SurfaceDropPoint { surface, pnt, tol } => {
            push_srf(&mut o, surface)?;
            o.extend_from_slice(pnt);
            o.push(*tol);
        }
        Inputs::TwoPnts {
            surface1,
            surface2,
            start,
            end,
            dir,
            end_dir,
            tol,
        } => {
            push_srf(&mut o, surface1)?;
            push_srf(&mut o, surface2)?;
            o.extend_from_slice(start);
            o.push(flag(end.is_some()));
            o.extend_from_slice(&end.unwrap_or([0.0; 3]));
            o.extend_from_slice(dir);
            o.push(flag(end_dir.is_some()));
            o.extend_from_slice(&end_dir.unwrap_or([0.0; 3]));
            o.push(*tol);
        }
        Inputs::EvaluateMate { surface, uv } => {
            push_srf(&mut o, surface)?;
            o.extend_from_slice(uv);
        }
        Inputs::EvaluateMateAnalytic { surface, uv } => {
            match surface {
                AnalyticSrf::Plane {
                    origin,
                    x_axis,
                    y_axis,
                    uv_scale,
                    domain,
                    make_nurb,
                } => {
                    o.push(0.0);
                    o.extend_from_slice(origin);
                    o.extend_from_slice(x_axis);
                    o.extend_from_slice(y_axis);
                    o.extend_from_slice(uv_scale);
                    o.extend([domain[0][0], domain[0][1], domain[1][0], domain[1][1]]);
                    o.push(flag(*make_nurb));
                }
                AnalyticSrf::Revolution {
                    a,
                    b,
                    axis_point,
                    axis_dir,
                    swap,
                } => {
                    o.push(1.0);
                    o.extend_from_slice(a);
                    o.extend_from_slice(b);
                    o.extend_from_slice(axis_point);
                    o.extend_from_slice(axis_dir);
                    o.push(flag(*swap));
                }
            }
            o.extend_from_slice(uv);
        }
        Inputs::Capping {
            tol,
            surface,
            uv1,
            orient1,
            uv2,
            orient2,
        } => {
            o.push(*tol);
            push_srf(&mut o, surface)?;
            push_crv(&mut o, uv1);
            o.push(*orient1 as f64);
            push_crv(&mut o, uv2);
            o.push(*orient2 as f64);
        }
        Inputs::ClassifyFacePoint {
            surface1,
            surface2,
            pos,
        } => {
            push_srf(&mut o, surface1)?;
            push_srf(&mut o, surface2)?;
            o.extend_from_slice(pos);
        }
        Inputs::Fb { model, query } => {
            push_model(&mut o, model)?;
            push_query(&mut o, query)?;
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs, expected_rejection: bool) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    let result = ffi::eval(i.op(), &input);
    if expected_rejection {
        // A fixture that upstream cannot run (see the FFI module doc): the wrapper
        // refuses it (`InvalidArgument`) or the shim does (`SC_ERR_ARG`: the `FbExamine`
        // adjusting path without a solver, a face upstream will not build, a surface
        // whose constructed v knots crash the surface cache, UB-166). Only a
        // flagged case may be recorded as rejected.
        return match result {
            Err(solidcore_ffi::FfiError::InvalidArgument(reason)) => Ok(Output::rejected(reason)),
            Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                Ok(Output::rejected("shim_rejected_args"))
            }
            Err(e) => Err(harness(format!(
                "expected-rejection fixture failed in another way: {e}"
            ))),
            Ok(_) => Err(harness("expected-rejection fixture was not refused")),
        };
    }
    let flat = result.map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

// ---- output splitting ---------------------------------------------------------------

pub(crate) struct Split<'a> {
    pub(crate) flat: &'a [f64],
    pub(crate) at: usize,
    pub(crate) out: Output,
}

impl Split<'_> {
    pub(crate) fn take(&mut self, k: usize) -> Result<Vec<f64>, HarnessError> {
        let part = self
            .at
            .checked_add(k)
            .and_then(|end| self.flat.get(self.at..end))
            .ok_or_else(|| harness(format!("record too short ({} values)", self.flat.len())))?;
        self.at += k;
        Ok(part.to_vec())
    }

    fn one(&mut self) -> Result<f64, HarnessError> {
        Ok(self.take(1)?.first().copied().unwrap_or(f64::NAN))
    }

    fn count(&mut self) -> Result<usize, HarnessError> {
        let x = self.one()?;
        if x.fract() != 0.0 || !(0.0..=1e6).contains(&x) {
            return Err(harness(format!("bad count {x} in record")));
        }
        Ok(x as usize)
    }

    pub(crate) fn put(&mut self, key: &str, v: Vec<f64>) {
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
    }

    pub(crate) fn vals(&mut self, key: &str, k: usize) -> Result<(), HarnessError> {
        let v = self.take(k)?;
        self.put(key, v);
        Ok(())
    }

    /// A curve record `has [dim n p m cpts knots]` under `pfx`.
    pub(crate) fn curve(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
            let dims = self.take(4)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let ok = |x: f64| x.fract() == 0.0 && (0.0..=1e6).contains(&x);
            let (n, m) = (cnt(1), cnt(3));
            if !(ok(n) && ok(m)) {
                return Err(harness(format!("bad curve dims {dims:?}")));
            }
            self.put(&format!("{pfx}dims"), dims);
            self.vals(&format!("{pfx}cpts"), 4 * (n as usize + 1))?;
            self.vals(&format!("{pfx}knots"), m as usize + 1)?;
        }
        Ok(())
    }

    fn surface(&mut self, pfx: &str) -> Result<(), HarnessError> {
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
            let dims = self.take(6)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let ok = |x: f64| x.fract() == 0.0 && (0.0..=1e6).contains(&x);
            let (n, m, r, s) = (cnt(0), cnt(1), cnt(4), cnt(5));
            if !(ok(n) && ok(m) && ok(r) && ok(s)) {
                return Err(harness(format!("bad surface dims {dims:?}")));
            }
            self.put(&format!("{pfx}dims"), dims);
            self.vals(
                &format!("{pfx}cpts"),
                4 * (n as usize + 1) * (m as usize + 1),
            )?;
            self.vals(&format!("{pfx}u"), r as usize + 1)?;
            self.vals(&format!("{pfx}v"), s as usize + 1)?;
        }
        Ok(())
    }

    /// The pseudo-B-rep export.
    pub(crate) fn export(&mut self) -> Result<(), HarnessError> {
        let nv = self.count()?;
        self.put("v_count", vec![nv as f64]);
        for i in 0..nv {
            self.vals(&format!("v{i:02}_state"), 16)?;
            let nvu = self.count()?;
            self.vals(&format!("v{i:02}_uses"), 4 * nvu)?;
        }
        let ne = self.count()?;
        self.put("e_count", vec![ne as f64]);
        for i in 0..ne {
            self.vals(&format!("e{i:02}_state"), 7)?;
            self.curve(&format!("e{i:02}_curve_"))?;
            self.vals(&format!("e{i:02}_ivl"), 2)?;
            let neu = self.count()?;
            self.put(&format!("e{i:02}_eu_count"), vec![neu as f64]);
            for k in 0..neu {
                self.vals(&format!("e{i:02}_eu{k}_state"), 2)?;
                self.curve(&format!("e{i:02}_eu{k}_uv_"))?;
                self.vals(&format!("e{i:02}_eu{k}_geom"), 1)?;
            }
        }
        let ng = self.count()?;
        self.put("g_count", vec![ng as f64]);
        for i in 0..ng {
            self.vals(&format!("g{i:02}_state"), 4)?;
            let n1 = self.count()?;
            self.vals(&format!("g{i:02}_other1"), n1)?;
            let n2 = self.count()?;
            self.vals(&format!("g{i:02}_other2"), n2)?;
            self.curve(&format!("g{i:02}_center_"))?;
            self.surface(&format!("g{i:02}_surface_"))?;
            let nr = self.count()?;
            self.vals(&format!("g{i:02}_refs"), 2 * nr)?;
            let ns = self.count()?;
            self.vals(&format!("g{i:02}_sides"), 2 * ns)?;
        }
        Ok(())
    }
}

/// Splits a flat record into keys (layout in the FFI module docs).
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut s = Split {
        flat,
        at: 0,
        out: Output::new(),
    };
    let status = s.take(1)?;
    s.put("status", status);
    match i {
        Inputs::CheckSeamOut { .. } | Inputs::CheckSeamOn { .. } => {
            s.vals("ret", 1)?;
            s.vals("uv", 2)?;
        }
        Inputs::SurfaceClosure { .. } => s.vals("closure", 1)?,
        Inputs::IsOrderCorrect { .. } => s.vals("ret", 1)?,
        Inputs::CleanupNoise { .. } | Inputs::AdjustUvOnSeam { .. } => s.vals("uv", 2)?,
        Inputs::Interpolate { .. } => {
            s.vals("svec", 3)?;
            s.vals("evec", 3)?;
            s.curve("curve_")?;
        }
        Inputs::SurfaceDropPoint { .. } => {
            let n = s.count()?;
            s.put("n", vec![n as f64]);
            s.vals("solutions", 3 * n)?;
        }
        Inputs::TwoPnts { .. } => {
            s.curve("c3d_")?;
            s.curve("uv1_")?;
            s.curve("uv2_")?;
            s.vals("dir", 3)?;
        }
        Inputs::EvaluateMate { .. } => s.vals("pnt", 3)?,
        Inputs::EvaluateMateAnalytic { .. } => {
            s.vals("mate_status", 1)?;
            s.vals("pnt", 3)?;
        }
        Inputs::Capping { .. } => {
            let n = s.count()?;
            s.put("n", vec![n as f64]);
            for k in 0..n {
                s.curve(&format!("cap{k}_uv_"))?;
                s.curve(&format!("cap{k}_3d_"))?;
                s.vals(&format!("cap{k}_orient"), 1)?;
            }
        }
        Inputs::ClassifyFacePoint { .. } => s.vals("which", 1)?,
        Inputs::Fb { query, .. } => match query {
            Query::Export
            | Query::DeleteVertex { .. }
            | Query::DeleteEdge { .. }
            | Query::Examine { .. }
            | Query::SetEuGeom { .. }
            | Query::SetUvCurve { .. }
            | Query::SetVuProperty { .. }
            | Query::RemoveEdge { .. } => s.export()?,
            Query::VertexGeoms { .. } => {
                let n = s.count()?;
                s.put("n", vec![n as f64]);
                s.vals("geoms", n)?;
            }
            Query::VuAtRailEnd { .. } | Query::RailVertexuse { .. } => s.vals("vu", 3)?,
            Query::EdgeName { .. } => s.vals("code", 1)?,
            Query::FindEdge { .. } => s.vals("fbe", 2)?,
            Query::ClearEdges { .. } | Query::BrepEdges { .. } => {
                let n = s.count()?;
                s.put("n", vec![n as f64]);
                s.vals("edges", n)?;
                s.vals("fbe1", 1)?;
            }
            Query::Trim { .. } => {
                s.vals("ivl", 2)?;
                s.export()?;
            }
            Query::VertOnVert { .. } | Query::Cliff { .. } => {
                s.vals("pnt", 3)?;
                s.vals("vstatus", 1)?;
            }
            Query::SplitFace { .. } => s.vals("pc", 5)?,
            Query::OrigUvCurve { .. } => s.curve("uv_")?,
        },
    }
    if s.at != flat.len() {
        return Err(harness(format!(
            "record has {} trailing values",
            flat.len() - s.at
        )));
    }
    Ok(s.out)
}

// ---- Rust side: building geometry -------------------------------------------------------

pub(crate) fn curve_of(c: &Crv) -> Result<BSplineCurve, HarnessError> {
    // the FFI's count rule (`solidcore_ffi` `curve`), as for surfaces in `surface_of`
    let n = c.cpts.len().wrapping_sub(1);
    if c.cpts.is_empty()
        || !(1..=n).contains(&c.p)
        || n.checked_add(c.p + 1) != Some(c.u.len().wrapping_sub(1))
    {
        return Err(harness("curve input: need 1 <= p <= n and m = n + p + 1"));
    }
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur =
        NurbCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, c.dim, false))
}

pub(crate) fn surface_of(s: &Srf) -> Result<Surface, HarnessError> {
    // the FFI's count rule (`solidcore_ffi` `surface`): the C++ refuses anything else,
    // and `srf_basic::Surface::new` checks only the shapes, so an inconsistent degree
    // must not reach the library (a fuzzed `q = i64::MAX` overflowed its index loops)
    let n = s.net.len().wrapping_sub(1);
    let m = s.net.first().map_or(0, Vec::len).wrapping_sub(1);
    let counts_ok = !s.net.is_empty()
        && s.net.first().is_some_and(|r| !r.is_empty())
        && (1..=n).contains(&s.p)
        && (1..=m).contains(&s.q)
        && n.checked_add(s.p + 1) == Some(s.u.len().wrapping_sub(1))
        && m.checked_add(s.q + 1) == Some(s.v.len().wrapping_sub(1));
    if !counts_ok {
        return Err(harness(
            "surface input: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    let net = s
        .net
        .iter()
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
        .collect();
    let nurb = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    Ok(Surface::from(BSplineSurface::from_nurb(nurb, false)))
}

fn uv(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

pub(crate) fn dump_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    // the SmBSplineCurve part, as the shim's IsKindOf(SmBSplineCurve_TYPE) test takes it
    let bs = match c.map(Curve::kind) {
        Some(CurveKind::BSpline(bs)) => Some(bs),
        Some(CurveKind::Line(l)) => Some(l.base()),
        Some(CurveKind::Ellipse(e)) => Some(e.base()),
        Some(CurveKind::Hyperbola(h)) => Some(h.base()),
        Some(CurveKind::Parabola(p)) => Some(p.base()),
        _ => None,
    };
    let Some(cur) = bs.and_then(BSplineCurve::gw_nurb) else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.extend([
        bs.map_or(3, BSplineCurve::dim) as f64,
        cur.n() as f64,
        cur.degree() as f64,
        cur.m() as f64,
    ]);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(cur.knots());
}

/// The B-spline part of a curve of a kind derived from `SmBSplineCurve`.
pub(crate) const fn b_spline_part(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        _ => None,
    }
}

pub(crate) fn dump_bcurve(o: &mut Vec<f64>, c: Option<&BSplineCurve>) {
    let Some(cur) = c.and_then(BSplineCurve::gw_nurb) else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.extend([
        c.map_or(3, BSplineCurve::dim) as f64,
        cur.n() as f64,
        cur.degree() as f64,
        cur.m() as f64,
    ]);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend_from_slice(cur.knots());
}

fn dump_surface(o: &mut Vec<f64>, s: Option<&Surface>) -> Result<(), HarnessError> {
    let bs = match s.map(Surface::kind) {
        Some(SurfaceKind::BSpline(bs)) => Some(bs),
        _ => None,
    };
    dump_bspline_surface(o, bs)
}

/// The shim's `put_surface` record of a B-spline surface (`0` for none).
pub(crate) fn dump_bspline_surface(
    o: &mut Vec<f64>,
    bs: Option<&BSplineSurface>,
) -> Result<(), HarnessError> {
    let Some(sur) = bs.and_then(BSplineSurface::gw_nurb) else {
        o.push(0.0);
        return Ok(());
    };
    o.push(1.0);
    let (n, m) = (sur.n(), sur.m());
    o.extend([n, m, sur.p(), sur.q(), sur.r(), sur.s()].map(|x| x as f64));
    for i in 0..=n {
        for j in 0..=m {
            let p = sur.cpt(i, j).map_err(|e| harness(e.to_string()))?;
            o.extend([p.x, p.y, p.z, p.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
    Ok(())
}

fn closure_of(code: i64) -> Result<SurfParamType, HarnessError> {
    u64::try_from(code)
        .ok()
        .and_then(SurfParamType::from_code)
        .ok_or_else(|| harness("bad closure"))
}

fn orient_of(code: i64) -> Result<OrientType, HarnessError> {
    Ok(match code {
        0 => OrientType::Unknown,
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => return Err(harness("bad orientation")),
    })
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

/// The shim's `EvaluateMateAnalytic` surface: the plane (its NURB made when asked) or
/// the canonical surface of revolution of a line (`SmLine` with its NURB made, that
/// status ignored as the shim's `GetOrCreateGwNurbPointer`).
pub(crate) fn analytic_surface(k: &Kernel, s: &AnalyticSrf) -> solidcore::Result<Surface> {
    Ok(match s {
        AnalyticSrf::Plane {
            origin,
            x_axis,
            y_axis,
            uv_scale,
            domain,
            make_nurb,
        } => {
            let dom = Extent2d::from_bounds(domain[0][0], domain[0][1], domain[1][0], domain[1][1]);
            let mut pl = Plane::new(
                v3(*origin),
                v3(*x_axis),
                v3(*y_axis),
                Vec2::new(uv_scale[0], uv_scale[1]),
                &dom,
            );
            if *make_nurb {
                pl.make_nurb()?;
            }
            Surface::from(pl)
        }
        AnalyticSrf::Revolution {
            a,
            b,
            axis_point,
            axis_dir,
            swap,
        } => {
            let mut l = Line::from_points(v3(*a), v3(*b), 3);
            let _ = l.make_nurb();
            Surface::from(SurfOfRevolution::create_canonical(
                k,
                Curve::from(l),
                v3(*axis_point),
                v3(*axis_dir),
                *swap,
            )?)
        }
    })
}

/// `new (ctx) SmBrep()`: an empty B-rep with the kernel's default tolerances and its
/// infinite region.
pub(crate) fn new_brep(k: &mut Kernel) -> Result<Brep, HarnessError> {
    solidcore::topology::brep::p3::new_brep(k, USE_DEFAULT, USE_DEFAULT)
        .map_err(|e| harness(e.to_string()))
}

/// The shim's `face_of`: `CreateFaceFromSurface` over the surface's natural domain, a
/// face in its own shell of the infinite region bounded by edges on the natural trim
/// curves (no UV trim curves yet: upstream makes them on demand with
/// `SmSurface::DropCurve`, `intersect.surface_drop_lib_p3`), with the tolerances that
/// sets. The shim refuses the case when upstream fails, so a Rust failure is a harness
/// error.
pub(crate) fn natural_face(
    k: &mut Kernel,
    b: &mut Brep,
    surface: Surface,
) -> Result<FaceId, HarnessError> {
    let domain = surface.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?;
    let mut records = BTreeMap::new();
    let mut attributes = AttributeStore::default();
    let key_of = super::topology_brep_p8::key_of;
    let mut env = NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &key_of,
    };
    let mut marks = Marks::default();
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(surface), &domain, &mut fenv)
        .map_err(|e| harness(format!("CreateFaceFromSurface: {e}")))
}

/// A single face on `surface` in a fresh B-rep (`CreateFaceFromSurface` over the natural
/// domain).
fn face_brep(k: &mut Kernel, surface: Surface) -> Result<(Brep, FaceId), HarnessError> {
    let mut b = new_brep(k)?;
    let f = natural_face(k, &mut b, surface)?;
    Ok((b, f))
}

/// A wire edge on `curve` over its natural interval (`CreateWireEdgeFromCurve`): start and
/// end vertices at the curve ends (glued into the start vertex when closer than the
/// B-rep tolerance), the edge with two edge uses and vertex uses.
fn wire_edge(k: &Kernel, b: &mut Brep, curve: BSplineCurve, tol: Option<f64>) -> Result<EdgeId, HarnessError> {
    let c = Curve::from(curve);
    let ivl = c.natural_interval();
    let ps = c
        .evaluate_point(k, ivl.min())
        .map_err(|e| harness(e.to_string()))?;
    let pe = c
        .evaluate_point(k, ivl.max())
        .map_err(|e| harness(e.to_string()))?;
    let vs = b.add_vertex(ps).map_err(|e| harness(e.to_string()))?;
    let ve = if ps.distance(pe) < ZONE_TOL_3D {
        vs
    } else {
        b.add_vertex(pe).map_err(|e| harness(e.to_string()))?
    };
    let e = b.add_edge(vs, ve).map_err(|e| harness(e.to_string()))?;
    {
        let edge = b.edge_mut(e).map_err(|e| harness(e.to_string()))?;
        edge.curve = Some(c);
        edge.interval = ivl;
        if let Some(t) = tol {
            edge.tolerance = t;
        }
    }
    link_uses(b, e, vs, ve)?;
    Ok(e)
}

/// Two edge uses (primary `SM_OT_SAME`, mate `SM_OT_OPPOSITE`) on `e` with vertex uses on
/// `vs` and `ve`, as `MakeFilletEdge` / `CreateWireEdgeFromCurve` link them.
fn link_uses(
    b: &mut Brep,
    e: EdgeId,
    vs: VertexId,
    ve: VertexId,
) -> Result<[EdgeuseId; 2], HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
    let eu1 = b.add_edgeuse_record(Edgeuse::default()).map_err(h)?;
    let eu2 = b.add_edgeuse_record(Edgeuse::default()).map_err(h)?;
    let vu1 = b.add_vertexuse_record(Vertexuse::default()).map_err(h)?;
    let vu2 = b.add_vertexuse_record(Vertexuse::default()).map_err(h)?;
    for (eu, vu, v, o) in [
        (eu1, vu1, vs, OrientType::Same),
        (eu2, vu2, ve, OrientType::Opposite),
    ] {
        let r = b.edgeuse_mut(eu).map_err(h)?;
        r.edge = Some(e);
        r.orientation = o;
        r.vertexuse = Some(vu);
        let u = b.vertexuse_mut(vu).map_err(h)?;
        u.vertex = Some(v);
        u.owner = VertexuseOwner::Edgeuse(Some(eu));
        b.vertex_mut(v).map_err(h)?.vertexuses.push(vu);
    }
    b.edge_mut(e).map_err(h)?.edgeuses = vec![eu1, eu2];
    Ok([eu1, eu2])
}

/// The Rust-side model.
pub(crate) struct RustModel {
    pub(crate) k: Kernel,
    pub(crate) target: Brep,
    pub(crate) target_edges: Vec<EdgeId>,
    pub(crate) target_faces: Vec<FaceId>,
    pub(crate) fillet: Brep,
    pub(crate) fillet_edges: Vec<EdgeId>,
    pub(crate) fb: FilletBrep,
    /// The Model's own geometries (the first `geom_count` of the arena; the shim's
    /// `geoms` vector). Geometries the library adds later (a temporary solver's, which
    /// upstream deletes with the solver) are not the Model's: the shim neither exports
    /// nor addresses them.
    pub(crate) geom_count: usize,
}

impl RustModel {
    pub(crate) fn build(m: &Model) -> Result<Self, HarnessError> {
        let h = |e: SolidError| harness(e.to_string());
        let mut k = Kernel::default();
        let mut target = new_brep(&mut k)?;
        let mut target_edges = Vec::new();
        for c in &m.targets {
            target_edges.push(wire_edge(&k, &mut target, curve_of(c)?, None)?);
        }
        let mut target_faces = Vec::new();
        for s in &m.faces {
            target_faces.push(natural_face(&mut k, &mut target, surface_of(s)?)?);
        }
        let mut fillet = Brep::new();
        let mut fillet_edges = Vec::new();
        for e in &m.fillet_brep {
            fillet_edges.push(wire_edge(&k, &mut fillet, curve_of(&e.curve)?, Some(e.tol))?);
        }

        let mut fb = FilletBrep::new();
        let corner = |c: i64| usize::try_from(c).ok().map(|i| FilletCornerId(i as u32));
        let mut vids = Vec::new();
        for v in &m.vertices {
            let id = fb.brep_mut().add_vertex(v3(v.point)).map_err(h)?;
            let mut fv = FilletVertex::new(&k, corner(v.corner));
            fv.set_fillet_vertex_type(
                FilletVertexType::from_code(v.vtype).ok_or_else(|| harness("bad vertex type"))?,
            );
            fv.set_status(FilStatus::from_code(v.status).ok_or_else(|| harness("bad status"))?);
            let class = i32::try_from(v.pc_class)
                .ok()
                .and_then(PointClassType::from_code)
                .ok_or_else(|| harness("bad point class"))?;
            let object = match (v.pc_kind, usize::try_from(v.pc_index)) {
                (1, Ok(i)) => {
                    let e = target_edges.get(i).ok_or_else(|| harness("pc index"))?;
                    let [vs, ve] = target.edge(*e).map_err(h)?.vertices;
                    Some(EntityRef::Vertex(if v.pc_sub == 0 { vs } else { ve }))
                }
                (2, Ok(i)) => {
                    let e = target_edges.get(i).ok_or_else(|| harness("pc index"))?;
                    let eu = target
                        .edge(*e)
                        .map_err(h)?
                        .edgeuses
                        .first()
                        .copied()
                        .ok_or_else(|| harness("pc edge use"))?;
                    Some(EntityRef::Edgeuse(eu))
                }
                (3, Ok(i)) => Some(EntityRef::Face(
                    *target_faces.get(i).ok_or_else(|| harness("pc face"))?,
                )),
                (5, Ok(i)) => {
                    // edge use `pc_sub` of the first loop of target face `i`
                    let f = *target_faces.get(i).ok_or_else(|| harness("pc face"))?;
                    let loops = solidcore::topology::face::p2::loops(&target, f, false, None)
                        .map_err(h)?;
                    let l = *loops.first().ok_or_else(|| harness("pc loop"))?;
                    let eus = solidcore::topology::loopuse::loop_edgeuses(&target, l).map_err(h)?;
                    let eu = usize::try_from(v.pc_sub)
                        .ok()
                        .and_then(|s| eus.get(s).copied())
                        .ok_or_else(|| harness("pc loop edge use"))?;
                    Some(EntityRef::Edgeuse(eu))
                }
                (4, Ok(i)) => Some(EntityRef::Edge(
                    *target_edges.get(i).ok_or_else(|| harness("pc index"))?,
                )),
                _ => None,
            };
            fv.set_point_class(class, object);
            if let Ok(i) = usize::try_from(v.pc_face) {
                fv.point_classification_mut().set_face(Some(EntityRef::Face(
                    *target_faces.get(i).ok_or_else(|| harness("pc face"))?,
                )));
            }
            fv.set_original_t_param(v.t_param);
            fv.set_original_uv(uv(v.uv));
            fb.insert_fillet_vertex(id, fv);
            vids.push(id);
        }

        let mut gids = Vec::new();
        for g in &m.geoms {
            let gtype =
                FilletGeomType::from_code(g.gtype).ok_or_else(|| harness("bad geom type"))?;
            let mut geom = FilletGeom::new(&k, None, gtype).map_err(h)?;
            geom.status = FilStatus::from_code(g.status).ok_or_else(|| harness("bad status"))?;
            if let Some(c) = &g.center {
                geom.center_line_curve = Some(Curve::from(curve_of(c)?));
            }
            if let Some(s) = &g.surface {
                geom.set_fillet_surface(Some(surface_of(s)?));
            }
            gids.push(fb.add_geom(geom));
        }

        let mut eids = Vec::new();
        for e in &m.edges {
            let vs = *vids.get(e.vs).ok_or_else(|| harness("edge vertex"))?;
            let ve = *vids.get(e.ve).ok_or_else(|| harness("edge vertex"))?;
            let id = fb.new_fillet_edge(vs, ve, corner(e.corner)).map_err(h)?;
            let eus = link_uses(fb.brep_mut(), id, vs, ve)?;
            {
                let fe = fb.fillet_edge_mut(id).map_err(h)?;
                fe.set_fillet_edge_type(
                    FilletEdgeType::from_code(e.etype).ok_or_else(|| harness("bad edge type"))?,
                );
                fe.set_status(FilStatus::from_code(e.status).ok_or_else(|| harness("bad status"))?);
                let fbe = |x: i64| {
                    usize::try_from(x)
                        .ok()
                        .and_then(|i| fillet_edges.get(i).copied())
                };
                fe.fillet_brep_edge1 = fbe(e.fbe[0]);
                fe.fillet_brep_edge2 = fbe(e.fbe[1]);
            }
            {
                let edge = fb.brep_mut().edge_mut(id).map_err(h)?;
                if let Some(c) = &e.curve {
                    edge.curve = Some(Curve::from(curve_of(c)?));
                }
                if let Some([a, b]) = e.interval {
                    edge.interval = Extent1d::from_min_max(a, b);
                }
            }
            for (eu, spec) in eus.iter().zip(&e.eus) {
                fb.insert_fillet_edgeuse(*eu, FilletEdgeuse::default());
                if let Some(c) = &spec.uv_curve {
                    fb.brep_mut().edgeuse_mut(*eu).map_err(h)?.uv_trim_curve =
                        Some(Curve::from(curve_of(c)?));
                }
                if let Ok(gi) = usize::try_from(spec.geom) {
                    let g = *gids.get(gi).ok_or_else(|| harness("edge use geom"))?;
                    fb.set_edgeuse_fillet_geom(*eu, Some(g)).map_err(h)?;
                }
            }
            for (eu, spec) in eus.iter().zip(&e.eus) {
                let vu = fb
                    .brep()
                    .edgeuse(*eu)
                    .map_err(h)?
                    .vertexuse
                    .ok_or_else(|| harness("edge use without vertex use"))?;
                let mut fvu = FilletVertexuse::new();
                fvu.set_uv_point(uv(spec.vu_uv));
                fb.insert_fillet_vertexuse(vu, fvu);
            }
            eids.push(id);
        }

        for (g, spec) in gids.iter().zip(&m.geoms) {
            let eid = |x: usize| eids.get(x).copied().ok_or_else(|| harness("geom edge"));
            let geom = fb.geom_mut(*g).map_err(h)?;
            for (r, &rail) in spec.rails.iter().enumerate() {
                let rail = match usize::try_from(rail) {
                    Ok(i) => Some(eid(i)?),
                    Err(_) => None,
                };
                geom.set_rail(r, rail);
            }
            for &x in &spec.other1 {
                geom.other_rails1.push(eid(x)?);
            }
            for &x in &spec.other2 {
                geom.other_rails2.push(eid(x)?);
            }
            let mut sides = Vec::new();
            for &[e, w] in &spec.sides {
                let edge = eid(e)?;
                let eu = fb
                    .brep()
                    .edge(edge)
                    .map_err(h)?
                    .edgeuses
                    .get(w)
                    .copied()
                    .ok_or_else(|| harness("side edge use"))?;
                sides.push(eu);
            }
            let geom = fb.geom_mut(*g).map_err(h)?;
            for eu in sides {
                geom.add_side_fillet_edgeuse(eu);
            }
        }

        for (v, spec) in vids.iter().zip(&m.vertices) {
            for (i, &mate) in spec.mates.iter().enumerate() {
                let mate = match usize::try_from(mate) {
                    Ok(j) => Some(*vids.get(j).ok_or_else(|| harness("mate"))?),
                    Err(_) => None,
                };
                fb.fillet_vertex_mut(*v).map_err(h)?.set_mate(i, mate);
            }
        }

        Ok(Self {
            k,
            target,
            target_edges,
            target_faces,
            fillet,
            fillet_edges,
            fb,
            geom_count: gids.len(),
        })
    }

    /// Live vertices in creation order.
    pub(crate) fn vertices(&self) -> Vec<VertexId> {
        self.fb.fillet_vertices().map(|(id, _)| id).collect()
    }

    pub(crate) fn vertex(&self, i: usize) -> Result<VertexId, HarnessError> {
        self.vertices()
            .get(i)
            .copied()
            .ok_or_else(|| harness("vertex index"))
    }

    pub(crate) fn edge(&self, i: usize) -> Result<EdgeId, HarnessError> {
        self.fb
            .edge_list()
            .get(i)
            .copied()
            .ok_or_else(|| harness("edge index"))
    }

    pub(crate) fn edgeuse(&self, e: usize, which: usize) -> Result<EdgeuseId, HarnessError> {
        let e = self.edge(e)?;
        self.fb
            .brep()
            .edge(e)
            .map_err(|e| harness(e.to_string()))?
            .edgeuses
            .get(which)
            .copied()
            .ok_or_else(|| harness("edge use index"))
    }

    pub(crate) fn geom(&self, i: usize) -> Result<FilletGeomId, HarnessError> {
        if i < self.geom_count.min(self.fb.geoms().len()) {
            Ok(FilletGeomId(i as u32))
        } else {
            Err(harness("geometry index"))
        }
    }

    pub(crate) fn vertex_index(&self, v: Option<VertexId>) -> f64 {
        v.and_then(|v| self.vertices().iter().position(|&x| x == v))
            .map_or(-1.0, |i| i as f64)
    }

    pub(crate) fn edge_index(&self, e: Option<EdgeId>) -> f64 {
        e.and_then(|e| self.fb.edge_list().iter().position(|&x| x == e))
            .map_or(-1.0, |i| i as f64)
    }

    pub(crate) fn fillet_edge_index(&self, e: Option<EdgeId>) -> f64 {
        e.and_then(|e| self.fillet_edges.iter().position(|&x| x == e))
            .map_or(-1.0, |i| i as f64)
    }

    /// `(edge, 0/1)` of an edge use, or `(-1, -1)`.
    pub(crate) fn put_eu(&self, o: &mut Vec<f64>, eu: Option<EdgeuseId>) {
        if let Some(eu) = eu {
            for (i, &e) in self.fb.edge_list().iter().enumerate() {
                if let Ok(edge) = self.fb.brep().edge(e) {
                    if let Some(w) = edge.edgeuses.iter().position(|&x| x == eu) {
                        o.extend([i as f64, w as f64]);
                        return;
                    }
                }
            }
        }
        o.extend([-1.0, -1.0]);
    }

    pub(crate) fn put_vu(&self, o: &mut Vec<f64>, vu: Option<VertexuseId>) {
        if let Some(vu) = vu {
            for (i, &e) in self.fb.edge_list().iter().enumerate() {
                if let Ok(edge) = self.fb.brep().edge(e) {
                    for (w, &eu) in edge.edgeuses.iter().enumerate() {
                        if self.fb.brep().edgeuse(eu).ok().and_then(|r| r.vertexuse) == Some(vu) {
                            o.extend([i as f64, w as f64]);
                            return;
                        }
                    }
                }
            }
        }
        o.extend([-1.0, -1.0]);
    }

    pub(crate) fn put_pc_object(&self, o: &mut Vec<f64>, obj: Option<EntityRef>) {
        match obj {
            Some(EntityRef::Vertex(v)) => {
                for (i, &e) in self.target_edges.iter().enumerate() {
                    if let Ok(edge) = self.target.edge(e) {
                        if edge.vertices[0] == v {
                            o.extend([1.0, i as f64, 0.0]);
                            return;
                        }
                        if edge.vertices[1] == v {
                            o.extend([1.0, i as f64, 1.0]);
                            return;
                        }
                    }
                }
                o.extend([-1.0, -1.0, 0.0]);
            }
            Some(EntityRef::Edgeuse(eu)) => {
                for (i, &e) in self.target_edges.iter().enumerate() {
                    if self
                        .target
                        .edge(e)
                        .ok()
                        .and_then(|r| r.edgeuses.first().copied())
                        == Some(eu)
                    {
                        o.extend([2.0, i as f64, 0.0]);
                        return;
                    }
                }
                o.extend([-1.0, -1.0, 0.0]);
            }
            Some(EntityRef::Edge(e)) => {
                let i = self.target_edges.iter().position(|&x| x == e);
                o.extend([
                    if i.is_some() { 4.0 } else { -1.0 },
                    i.map_or(-1.0, |i| i as f64),
                    0.0,
                ]);
            }
            Some(EntityRef::Face(f)) => {
                let i = self.target_faces.iter().position(|&x| x == f);
                o.extend([
                    if i.is_some() { 3.0 } else { -1.0 },
                    i.map_or(-1.0, |i| i as f64),
                    0.0,
                ]);
            }
            Some(_) => o.extend([-1.0, -1.0, 0.0]),
            None => o.extend([0.0, -1.0, 0.0]),
        }
    }

    pub(crate) fn face_index(&self, f: Option<EntityRef>) -> f64 {
        match f {
            Some(EntityRef::Face(f)) => self
                .target_faces
                .iter()
                .position(|&x| x == f)
                .map_or(-1.0, |i| i as f64),
            _ => -1.0,
        }
    }

    pub(crate) fn corner_index(c: Option<FilletCornerId>) -> f64 {
        c.map_or(-1.0, |c| f64::from(c.0))
    }

    /// The export (mirrors the shim's).
    pub(crate) fn put(&self, o: &mut Vec<f64>) -> Result<(), HarnessError> {
        let h = |e: SolidError| harness(e.to_string());
        let b = self.fb.brep();
        let vs = self.vertices();
        o.push(vs.len() as f64);
        for &v in &vs {
            let fv = self.fb.fillet_vertex(v).map_err(h)?;
            let p = self.fb.vertex_point(v).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(fv.fillet_vertex_type() as i64 as f64);
            o.push(fv.status() as i64 as f64);
            for m in 0..2 {
                o.push(self.vertex_index(fv.mate(m)));
            }
            o.push(Self::corner_index(fv.fillet_corner()));
            o.push(fv.point_class() as i64 as f64);
            self.put_pc_object(o, fv.point_class_object());
            o.push(self.face_index(fv.point_classification().face()));
            o.push(fv.original_t_param());
            let ouv = fv.original_uv();
            o.extend([ouv.x, ouv.y]);
            let uses = solidcore::topology::vertex::vertexuses(b, v).map_err(h)?;
            o.push(uses.len() as f64);
            for vu in uses {
                self.put_vu(o, Some(vu));
                let p = self.fb.fillet_vertexuse(vu).map_err(h)?.uv_point();
                o.extend([p.x, p.y]);
            }
        }

        let es = self.fb.edge_list();
        o.push(es.len() as f64);
        for &e in es {
            let fe = self.fb.fillet_edge(e).map_err(h)?;
            let edge = b.edge(e).map_err(h)?;
            // the shim's SmEdge::GetVertices(vs, ve): through the primary edge use and its
            // mate, swapped unless the primary use is SM_OT_SAME (a new rail edge's uses are
            // SM_OT_UNKNOWN); the stored pair where the uses cannot give it
            let (vs0, ve0) = solidcore::topology::vertex::edge_vertex_pair(b, e)
                .ok()
                .and_then(|(s, t)| s.zip(t))
                .unwrap_or((edge.vertices[0], edge.vertices[1]));
            o.push(self.vertex_index(Some(vs0)));
            o.push(self.vertex_index(Some(ve0)));
            o.push(fe.fillet_edge_type() as i64 as f64);
            o.push(fe.status() as i64 as f64);
            o.push(Self::corner_index(fe.fillet_corner()));
            o.push(self.fillet_edge_index(fe.fillet_brep_edge1));
            o.push(self.fillet_edge_index(fe.fillet_brep_edge2));
            dump_curve(o, edge.curve.as_ref());
            o.extend([edge.interval.min(), edge.interval.max()]);
            o.push(edge.edgeuses.len() as f64);
            for &eu in &edge.edgeuses {
                let r = b.edgeuse(eu).map_err(h)?;
                o.push(orient_code(r.orientation));
                let v = r
                    .vertexuse
                    .and_then(|vu| b.vertexuse(vu).ok().and_then(|u| u.vertex));
                o.push(self.vertex_index(v));
                dump_curve(o, r.uv_trim_curve.as_ref());
                let g = self.fb.fillet_edgeuse(eu).map_err(h)?.fillet_geom();
                // index_of(geoms, ..): -1 for a geometry that is not the Model's
                o.push(
                    g.filter(|g| (g.0 as usize) < self.geom_count)
                        .map_or(-1.0, |g| f64::from(g.0)),
                );
            }
        }

        // the shim's `geoms` (the Model's own geometries)
        let gs = self.fb.geoms();
        let gs = gs.get(..self.geom_count).unwrap_or(gs);
        o.push(gs.len() as f64);
        for g in gs {
            o.push(g.fillet_geom_type() as i64 as f64);
            o.push(g.status() as i64 as f64);
            for r in 0..2 {
                o.push(self.edge_index(g.rail(r)));
            }
            o.push(g.other_rails1.len() as f64);
            for &e in &g.other_rails1 {
                o.push(self.edge_index(Some(e)));
            }
            o.push(g.other_rails2.len() as f64);
            for &e in &g.other_rails2 {
                o.push(self.edge_index(Some(e)));
            }
            dump_bcurve(o, g.center_line_curve());
            dump_surface(o, g.fillet_surface().map_err(h)?)?;
            o.push(g.ref_eus.len() as f64);
            for &eu in &g.ref_eus {
                self.put_eu(o, Some(eu));
            }
            o.push(g.side_eus.len() as f64);
            for &eu in &g.side_eus {
                self.put_eu(o, Some(eu));
            }
        }
        Ok(())
    }
}

/// The names `SmFilletEdge::GetName` writes, in the shim's code order.
const EDGE_NAMES: [&str; 10] = [
    "SM_FE_CROSS_SECTION",
    "SM_FE_FILLET_X_FILLET",
    "SM_FE_FILLET_X_SIDE_FACE",
    "SM_FE_RAIL_RAIL_INTERPOLATION",
    "SM_FE_ON_EDGE",
    "SM_FE_ON_EXTEND_EDGE",
    "SM_FE_FILLET_END",
    "SM_FE_SETBACK_RAIL",
    "SM_FE_RAIL_EXTENSION",
    "SM_FE_RAIL",
];

/// A Rust-side result: the flat record, or a deferral.
enum Rec {
    Flat(Vec<f64>),
    Deferred(SolidError),
}

/// `Ok(value)` or `Err(status)` of a kernel result; an unported callee returns the
/// deferral from the enclosing function.
macro_rules! try_status {
    ($e:expr) => {
        match $e {
            Ok(v) => Ok(v),
            Err(e) if e.is_unported() => return Ok(Rec::Deferred(e)),
            Err(e) => Err(status(&e)),
        }
    };
}

fn rust_geometry(i: &Inputs) -> Result<Rec, HarnessError> {
    let mut k = Kernel::default();
    let mut o = Vec::new();
    match i {
        Inputs::CleanupNoise { uv: p } => {
            let r = fg::cleanup_noise(uv(*p));
            o.extend([0.0, r.x, r.y]);
        }
        Inputs::CheckSeamOut {
            surface,
            tol,
            closure,
            uv: p,
        }
        | Inputs::CheckSeamOn {
            surface,
            tol,
            closure,
            uv: p,
        } => {
            let s = surface_of(surface)?;
            let c = closure_of(*closure)?;
            let mut q = uv(*p);
            let ret = if matches!(i, Inputs::CheckSeamOut { .. }) {
                fg::check_seam_out(&s, &mut q, *tol, c)
            } else {
                fg::check_seam_on(&s, &mut q, *tol, c)
            };
            o.extend([0.0, flag(ret), q.x, q.y]);
        }
        Inputs::SurfaceClosure { surface, domain } => {
            let s = surface_of(surface)?;
            let dom = Extent2d::from_bounds(domain[0][0], domain[0][1], domain[1][0], domain[1][1]);
            match try_status!(fg::set_surface_closure(&k, &s, &dom)) {
                Ok(c) => o.extend([0.0, c as u64 as f64]),
                Err(st) => o.push(st),
            }
        }
        Inputs::IsOrderCorrect { pnts, i1, i2 } => {
            let pts: Vec<TsectPnt> = pnts
                .iter()
                .map(|[p, d]| {
                    let mut t = TsectPnt::new();
                    *t.crv_pos_mut() = v3(*p);
                    *t.crv_deriv_mut() = Vec3::new(d[0], d[1], d[2]);
                    t
                })
                .collect();
            match try_status!(fg::is_order_correct(&pts, *i1, *i2)) {
                Ok(ret) => o.extend([0.0, flag(ret)]),
                Err(st) => o.push(st),
            }
        }
        Inputs::AdjustUvOnSeam {
            surface,
            ref_dir,
            uv: p,
        } => {
            let s = surface_of(surface)?;
            let mut q = uv(*p);
            let r =
                fg::adjust_uv_on_seam(&k, &s, Vec3::new(ref_dir[0], ref_dir[1], ref_dir[2]), &mut q);
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            o.extend([q.x, q.y]);
        }
        Inputs::Interpolate {
            start,
            end,
            svec,
            evec,
            dim,
        } => {
            let mut sv = Vec3::new(svec[0], svec[1], svec[2]);
            let mut ev = Vec3::new(evec[0], evec[1], evec[2]);
            let r =
                fg::interpolate_endpts_and_tangents(v3(*start), v3(*end), &mut sv, &mut ev, *dim);
            let curve = match try_status!(r) {
                Ok(c) => {
                    o.push(0.0);
                    Some(c)
                }
                Err(st) => {
                    o.push(st);
                    None
                }
            };
            o.extend([sv.x, sv.y, sv.z, ev.x, ev.y, ev.z]);
            dump_bcurve(&mut o, curve.as_ref());
        }
        Inputs::SurfaceDropPoint { surface, pnt, tol } => {
            let s = surface_of(surface)?;
            // the solutions name the surface, as `sm_SurfaceDropPoint`'s do
            match try_status!(fg::surface_drop_point(
                &k,
                &s,
                Some(fg::SurfaceHandle(&s)),
                v3(*pnt),
                *tol
            )) {
                Ok(sols) => {
                    o.extend([0.0, sols.len() as f64]);
                    for n in 0..sols.len() {
                        if let Some(sol) = sols.get(n) {
                            o.extend([
                                sol.start.parameters.first().copied().unwrap_or(0.0),
                                sol.start.parameters.get(1).copied().unwrap_or(0.0),
                                sol.start.solution_value,
                            ]);
                        }
                    }
                }
                Err(st) => o.extend([st, 0.0]),
            }
        }
        Inputs::TwoPnts {
            surface1,
            surface2,
            start,
            end,
            dir,
            end_dir,
            tol,
        } => {
            let s1 = surface_of(surface1)?;
            let s2 = surface_of(surface2)?;
            let mut d = Vec3::new(dir[0], dir[1], dir[2]);
            let r = fg::two_pnts_intersection(
                &k,
                v3(*start),
                end.map(v3),
                &mut d,
                end_dir.map(|e| Vec3::new(e[0], e[1], e[2])),
                &s1,
                &s2,
                *tol,
            );
            match try_status!(r) {
                Ok(x) => {
                    o.push(0.0);
                    // (the C++ dumps the SmBSplineCurve outputs' NURBs)
                    dump_bcurve(&mut o, x.curve_3d.as_ref().and_then(b_spline_part));
                    dump_bcurve(&mut o, x.surface1_uv_curve.as_ref().and_then(b_spline_part));
                    dump_bcurve(&mut o, x.surface2_uv_curve.as_ref().and_then(b_spline_part));
                }
                Err(st) => o.extend([st, 0.0, 0.0, 0.0]),
            }
            o.extend([d.x, d.y, d.z]);
        }
        Inputs::EvaluateMate { surface, uv: p } => {
            let (b, f) = face_brep(&mut k, surface_of(surface)?)?;
            match try_status!(fg::evaluate_mate(&k, &b, f, uv(*p))) {
                Ok(q) => o.extend([0.0, q.x, q.y, q.z]),
                Err(st) => o.extend([st, 0.0, 0.0, 0.0]),
            }
        }
        Inputs::EvaluateMateAnalytic { surface, uv: p } => {
            match try_status!(analytic_surface(&k, surface)) {
                Err(st) => o.extend([st, 0.0, 0.0, 0.0, 0.0]),
                Ok(srf) => {
                    let (b, f) = face_brep(&mut k, srf)?;
                    o.push(0.0);
                    match try_status!(fg::evaluate_mate(&k, &b, f, uv(*p))) {
                        Ok(q) => o.extend([0.0, q.x, q.y, q.z]),
                        Err(st) => o.extend([st, 0.0, 0.0, 0.0]),
                    }
                }
            }
        }
        Inputs::Capping {
            tol,
            surface,
            uv1,
            orient1,
            uv2,
            orient2,
        } => {
            let s = surface_of(surface)?;
            let c1 = Curve::from(curve_of(uv1)?);
            let c2 = Curve::from(curve_of(uv2)?);
            let mut c3d = Vec::new();
            let mut uvs = Vec::new();
            let mut orients = Vec::new();
            let r = fg::join_verts_by_capping(
                &k,
                *tol,
                &s,
                &c1,
                orient_of(*orient1)?,
                &c2,
                orient_of(*orient2)?,
                &mut c3d,
                &mut uvs,
                &mut orients,
            );
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            o.push(uvs.len() as f64);
            for n in 0..uvs.len() {
                dump_curve(&mut o, uvs.get(n));
                dump_curve(&mut o, c3d.get(n));
                o.push(orients.get(n).copied().map_or(-1.0, orient_code));
            }
        }
        Inputs::ClassifyFacePoint {
            surface1,
            surface2,
            pos,
        } => {
            let mut b = new_brep(&mut k)?;
            let mut faces = Vec::new();
            for s in [surface1, surface2] {
                faces.push(natural_face(&mut k, &mut b, surface_of(s)?)?);
            }
            let (f1, f2) = (
                *faces.first().ok_or_else(|| harness("face"))?,
                *faces.get(1).ok_or_else(|| harness("face"))?,
            );
            match try_status!(fg::classify_face_point(&k, &mut b, v3(*pos), f1, f2)) {
                Ok(w) => o.extend([0.0, f64::from(w)]),
                Err(st) => o.extend([st, 0.0]),
            }
        }
        Inputs::Fb { .. } => return Err(harness("not a geometry op")),
    }
    Ok(Rec::Flat(o))
}

fn rust_pseudo(model: &Model, q: &Query) -> Result<Rec, HarnessError> {
    let mut m = RustModel::build(model)?;
    let h = |e: SolidError| harness(e.to_string());
    let mut o = Vec::new();
    match q {
        Query::Export => {
            o.push(0.0);
            m.put(&mut o)?;
        }
        Query::VertexGeoms { v } => {
            let v = m.vertex(*v)?;
            let gs = m.fb.vertex_fillet_geoms(v).map_err(h)?;
            o.extend([0.0, gs.len() as f64]);
            o.extend(gs.iter().map(|g| f64::from(g.0)));
        }
        Query::VuAtRailEnd {
            v,
            geom,
            exclude,
            rail,
        } => {
            let v = m.vertex(*v)?;
            let g = match usize::try_from(*geom) {
                Ok(i) => Some(m.geom(i)?),
                Err(_) => None,
            };
            let x = match exclude {
                Some([e, w]) => {
                    let eu = m.edgeuse(*e, *w)?;
                    Some(
                        m.fb.brep()
                            .edgeuse(eu)
                            .map_err(h)?
                            .vertexuse
                            .ok_or_else(|| harness("exclude: no vertex use"))?,
                    )
                }
                None => None,
            };
            let r = match usize::try_from(*rail) {
                Ok(i) => Some(m.edge(i)?),
                Err(_) => None,
            };
            let vu = m.fb.vu_at_rail_end(v, g, x, r).map_err(h)?;
            o.extend([0.0, flag(vu.is_some())]);
            m.put_vu(&mut o, vu);
        }
        Query::RailVertexuse { g, rail, end } => {
            let g = m.geom(*g)?;
            let vu = m.fb.rail_vertexuse(g, *rail, *end).map_err(h)?;
            o.extend([0.0, flag(vu.is_some())]);
            m.put_vu(&mut o, vu);
        }
        Query::EdgeName { e } => {
            let e = m.edge(*e)?;
            let name = m.fb.fillet_edge(e).map_err(h)?.name();
            let code = name
                .and_then(|n| EDGE_NAMES.iter().position(|&x| x == n))
                .map_or(-1.0, |i| i as f64);
            o.extend([0.0, code]);
        }
        Query::FindEdge { e, multi, cands } => {
            let e = m.edge(*e)?;
            let mut cs = Vec::new();
            for &c in cands {
                cs.push(*m.fillet_edges.get(c).ok_or_else(|| harness("candidate"))?);
            }
            let r = m.fb.find_fillet_brep_edge(&m.k, e, &m.fillet, &cs, *multi);
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            let fe = m.fb.fillet_edge(e).map_err(h)?;
            o.push(m.fillet_edge_index(fe.fillet_brep_edge1));
            o.push(m.fillet_edge_index(fe.fillet_brep_edge2));
        }
        Query::ClearEdges { e, edges } => {
            let e = m.edge(*e)?;
            let mut es = Vec::new();
            for &c in edges {
                es.push(*m.fillet_edges.get(c).ok_or_else(|| harness("edge"))?);
            }
            m.fb.fillet_edge_mut(e)
                .map_err(h)?
                .clear_fillet_brep_edges(&es);
            o.push(0.0);
            let fe = m.fb.fillet_edge(e).map_err(h)?;
            let got = fe.fillet_brep_edges();
            o.push(got.len() as f64);
            for x in got {
                o.push(m.fillet_edge_index(Some(x)));
            }
            o.push(m.fillet_edge_index(fe.fillet_brep_edge()));
        }
        Query::BrepEdges { e } => {
            let e = m.edge(*e)?;
            let fe = m.fb.fillet_edge(e).map_err(h)?;
            let got = fe.fillet_brep_edges();
            o.extend([0.0, got.len() as f64]);
            for x in got {
                o.push(m.fillet_edge_index(Some(x)));
            }
            o.push(m.fillet_edge_index(fe.fillet_brep_edge()));
        }
        Query::Trim { g, interval } => {
            let g = m.geom(*g)?;
            let mut ivl = Extent1d::from_min_max(interval[0], interval[1]);
            let r = m.fb.trim_geom(&m.k, g, &mut ivl);
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            o.extend([ivl.min(), ivl.max()]);
            m.put(&mut o)?;
        }
        Query::DeleteVertex { v } => {
            let v = m.vertex(*v)?;
            m.fb.delete_fillet_vertex(v).map_err(h)?;
            o.push(0.0);
            m.put(&mut o)?;
        }
        Query::DeleteEdge { e } => {
            let e = m.edge(*e)?;
            m.fb.delete_fillet_edge(e).map_err(h)?;
            o.push(0.0);
            m.put(&mut o)?;
        }
        Query::VertOnVert { v } => {
            let v = m.vertex(*v)?;
            let r = m.fb.calc_vert_on_vert(v, &m.target);
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            let p = m.fb.vertex_point(v).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(m.fb.fillet_vertex(v).map_err(h)?.status() as i64 as f64);
        }
        Query::Cliff { v, surface, tol } => {
            let v = m.vertex(*v)?;
            let s = surface_of(surface)?;
            let r = m.fb.calc_cliff_rail_int_edge(&m.k, v, &m.target, &s, *tol);
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            let p = m.fb.vertex_point(v).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(m.fb.fillet_vertex(v).map_err(h)?.status() as i64 as f64);
        }
        Query::SplitFace { v, f1, f2 } => {
            let v = m.vertex(*v)?;
            let f1 = *m.target_faces.get(*f1).ok_or_else(|| harness("face"))?;
            let f2 = *m.target_faces.get(*f2).ok_or_else(|| harness("face"))?;
            let r = m.fb.update_split_face(&m.k, v, &mut m.target, f1, f2);
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            let fv = m.fb.fillet_vertex(v).map_err(h)?;
            o.push(fv.point_class() as i64 as f64);
            m.put_pc_object(&mut o, fv.point_class_object());
            o.push(m.face_index(fv.point_classification().face()));
        }
        Query::Examine {
            g1,
            at1,
            g2,
            at2,
            sv,
            ev,
            tol,
        } => {
            let (g1, g2) = (m.geom(*g1)?, m.geom(*g2)?);
            let (sv, ev) = (m.vertex(*sv)?, m.vertex(*ev)?);
            // the shim's geometries have no solver (NULL), so none is passed here
            let r = fg::examine_n_fix_fillet_geoms(
                &m.k,
                &mut m.fb,
                g1,
                *at1,
                g2,
                *at2,
                sv,
                ev,
                *tol,
                [None, None],
            );
            match try_status!(r) {
                Ok(()) => o.push(0.0),
                Err(st) => o.push(st),
            }
            m.put(&mut o)?;
        }
        Query::SetEuGeom { e, eu, geom } => {
            let eu = m.edgeuse(*e, *eu)?;
            let g = match usize::try_from(*geom) {
                Ok(i) => Some(m.geom(i)?),
                Err(_) => None,
            };
            m.fb.set_edgeuse_fillet_geom(eu, g).map_err(h)?;
            o.push(0.0);
            m.put(&mut o)?;
        }
        Query::SetUvCurve {
            e,
            eu,
            curve,
            delete,
            warn,
        } => {
            let eu = m.edgeuse(*e, *eu)?;
            let c = match curve {
                Some(c) => Some(Curve::from(curve_of(c)?)),
                None => None,
            };
            m.fb.set_edgeuse_uv_curve(eu, c, *delete, *warn)
                .map_err(h)?;
            o.push(0.0);
            m.put(&mut o)?;
        }
        Query::OrigUvCurve { e } => {
            let e = m.edge(*e)?;
            let c = m.fb.original_uv_curve(e).map_err(h)?;
            o.push(0.0);
            dump_curve(&mut o, c);
        }
        Query::SetVuProperty { e, eu, te, teu } => {
            let eu = m.edgeuse(*e, *eu)?;
            let teu = m.edgeuse(*te, *teu)?;
            let vu =
                m.fb.brep()
                    .edgeuse(eu)
                    .map_err(h)?
                    .vertexuse
                    .ok_or_else(|| harness("no vertex use"))?;
            m.fb.set_vertexuse_property(vu, teu).map_err(h)?;
            o.push(0.0);
            m.put(&mut o)?;
        }
        Query::RemoveEdge { e, readd } => {
            let e = m.edge(*e)?;
            m.fb.remove_edge(e);
            if *readd {
                m.fb.add_edge(e);
            }
            o.push(0.0);
            m.put(&mut o)?;
        }
    }
    Ok(Rec::Flat(o))
}

// ---- generated cases ---------------------------------------------------------------------

fn knots(n: usize, p: usize, rng: &mut Rng) -> Vec<f64> {
    // clamped, with random interior knots (sorted) over [0, L]
    let m = n + p + 1;
    let len = if rng.chance(0.3) {
        rng.uniform(0.5, 4.0)
    } else {
        1.0
    };
    let interior = m + 1 - 2 * (p + 1);
    let mut inner: Vec<f64> = (0..interior)
        .map(|_| rng.uniform(0.05, 0.95) * len)
        .collect();
    inner.sort_by(f64::total_cmp);
    let mut k = vec![0.0; p + 1];
    k.extend(inner);
    k.extend(vec![len; p + 1]);
    k
}

/// A random open surface patch: degrees 1..=3, a few control points, scale `scale`,
/// occasionally rational.
pub(crate) fn gen_surface(rng: &mut Rng, scale: f64) -> Srf {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let rational = rng.chance(0.2);
    let mut net = Vec::new();
    for i in 0..=n {
        let mut row = Vec::new();
        for j in 0..=m {
            let x = (i as f64 / n as f64 + rng.uniform(-0.1, 0.1)) * scale;
            let y = (j as f64 / m as f64 + rng.uniform(-0.1, 0.1)) * scale;
            let z = rng.uniform(-0.3, 0.3) * scale;
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    Srf {
        net,
        p,
        q,
        u: knots(n, p, rng),
        v: knots(m, q, rng),
    }
}

/// A closed-in-u (cylinder-like) quadratic B-spline surface of radius `r` and height `h`:
/// a full circle from 9 control points with weights.
pub(crate) fn gen_cylinder(rng: &mut Rng, r: f64, h: f64) -> Srf {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let circle: [([f64; 2], f64); 9] = [
        ([1.0, 0.0], 1.0),
        ([1.0, 1.0], w),
        ([0.0, 1.0], 1.0),
        ([-1.0, 1.0], w),
        ([-1.0, 0.0], 1.0),
        ([-1.0, -1.0], w),
        ([0.0, -1.0], 1.0),
        ([1.0, -1.0], w),
        ([1.0, 0.0], 1.0),
    ];
    let skew = rng.uniform(-0.2, 0.2) * r;
    let mut net = Vec::new();
    for (c, wt) in circle {
        // rational control points are stored weighted (x*w, y*w, z*w, w) in NLib
        let row = vec![
            [c[0] * r * wt, c[1] * r * wt, 0.0, wt],
            [(c[0] * r + skew) * wt, c[1] * r * wt, h * wt, wt],
        ];
        net.push(row);
    }
    Srf {
        net,
        p: 2,
        q: 1,
        u: vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

pub(crate) fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> Crv {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let rational = rng.chance(0.15);
    let cpts = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            [
                (t + rng.uniform(-0.1, 0.1)) * scale,
                rng.uniform(-0.5, 0.5) * scale,
                if dim == 3 {
                    rng.uniform(-0.5, 0.5) * scale
                } else {
                    0.0
                },
                w,
            ]
        })
        .collect();
    Crv {
        dim,
        cpts,
        p,
        u: knots(n, p, rng),
    }
}

/// A 2D line in the unit square's parameter box of a surface (scaled by the domain).
fn gen_uv_line(rng: &mut Rng, du: f64, dv: f64) -> Crv {
    let a = [rng.uniform(0.05, 0.95) * du, rng.uniform(0.05, 0.95) * dv];
    let b = [rng.uniform(0.05, 0.95) * du, rng.uniform(0.05, 0.95) * dv];
    Crv {
        dim: 2,
        cpts: vec![[a[0], a[1], 0.0, NOW], [b[0], b[1], 0.0, NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

fn surface_domain(s: &Srf) -> (f64, f64) {
    (
        s.u.last().copied().unwrap_or(1.0),
        s.v.last().copied().unwrap_or(1.0),
    )
}

pub(crate) fn scale_of(rng: &mut Rng) -> f64 {
    *rng.choose(&[1.0, 1.0, 1.0, 1e-3, 1e3, 1e-6, 1e6, 0.1, 10.0])
        .unwrap_or(&1.0)
}

/// An `EvaluateMateAnalytic` case: a plane (scaled UVs, half with a NURB) or a surface
/// of revolution of a line (cylinder-like when the line is parallel to the axis, else
/// cone-like), evaluated inside or around its domain.
fn gen_evaluate_mate_analytic(rng: &mut Rng, scale: f64) -> Inputs {
    let pt = |rng: &mut Rng| {
        [
            rng.uniform(-2.0, 2.0) * scale,
            rng.uniform(-2.0, 2.0) * scale,
            rng.uniform(-2.0, 2.0) * scale,
        ]
    };
    if rng.chance(0.5) {
        let origin = pt(rng);
        let x_axis = [
            rng.uniform(0.2, 1.0),
            rng.uniform(-0.5, 0.5),
            rng.uniform(-0.5, 0.5),
        ];
        let y_axis = [
            rng.uniform(-0.5, 0.5),
            rng.uniform(0.2, 1.0),
            rng.uniform(-0.5, 0.5),
        ];
        let uv_scale = if rng.chance(0.5) {
            [1.0, 1.0]
        } else {
            [rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)]
        };
        let (u0, v0) = (
            rng.uniform(-2.0, 1.0) * scale,
            rng.uniform(-2.0, 1.0) * scale,
        );
        let (du, dv) = (rng.uniform(0.5, 3.0) * scale, rng.uniform(0.5, 3.0) * scale);
        let p = if rng.chance(0.5) {
            [
                u0 + rng.uniform(0.0, 1.0) * du,
                v0 + rng.uniform(0.0, 1.0) * dv,
            ]
        } else {
            [
                u0 + rng.uniform(-0.5, 1.5) * du,
                v0 + rng.uniform(-0.5, 1.5) * dv,
            ]
        };
        Inputs::EvaluateMateAnalytic {
            surface: AnalyticSrf::Plane {
                origin,
                x_axis,
                y_axis,
                uv_scale,
                domain: [[u0, v0], [u0 + du, v0 + dv]],
                make_nurb: rng.chance(0.5),
            },
            uv: p,
        }
    } else {
        // the axis is the z axis moved and tilted a little; the line runs from radius r
        // at height 0 to radius r2 at height h in the xz plane of that frame
        let axis_point = pt(rng);
        let axis_dir = [rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3), 1.0];
        let r = rng.uniform(0.3, 2.0) * scale;
        let r2 = if rng.chance(0.5) {
            r
        } else {
            rng.uniform(0.3, 2.0) * scale
        };
        let h = rng.uniform(0.5, 3.0) * scale;
        let at = |x: f64, z: f64| {
            [
                axis_point[0] + x + axis_dir[0] * z,
                axis_point[1] + axis_dir[1] * z,
                axis_point[2] + axis_dir[2] * z,
            ]
        };
        let (a, b) = (at(r, 0.0), at(r2, h));
        let swap = rng.chance(0.3);
        // u turns once round (angles in radians), v runs along the line's length
        let len = h * 1.2;
        let (rot, lin) = if rng.chance(0.5) {
            (
                rng.uniform(0.0, std::f64::consts::TAU),
                rng.uniform(0.0, 1.0) * len,
            )
        } else {
            (rng.uniform(-1.0, 7.3), rng.uniform(-0.5, 1.5) * len)
        };
        let p = if swap { [lin, rot] } else { [rot, lin] };
        Inputs::EvaluateMateAnalytic {
            surface: AnalyticSrf::Revolution {
                a,
                b,
                axis_point,
                axis_dir,
                swap,
            },
            uv: p,
        }
    }
}

fn gen_geometry(rng: &mut Rng, kind: u64) -> Inputs {
    let scale = scale_of(rng);
    match kind {
        0 => {
            let base = rng.uniform(-5.0, 5.0);
            let noise = *rng
                .choose(&[0.0, 1e-13, -1e-13, 1e-11, 1e-9, 1e-7])
                .unwrap_or(&0.0);
            let digits = rng.below(5) as i32;
            let q = (base * 10f64.powi(digits)).round() / 10f64.powi(digits);
            Inputs::CleanupNoise {
                uv: [q + noise, rng.uniform(-1.0, 1.0)],
            }
        }
        1 | 2 => {
            let closed = rng.chance(0.5);
            let surface = if closed {
                let h = scale * rng.uniform(0.5, 2.0);
                gen_cylinder(rng, scale, h)
            } else {
                gen_surface(rng, scale)
            };
            let (du, dv) = surface_domain(&surface);
            let tol: f64 = *rng.choose(&[0.0, 1e-12, 1e-9, 1e-6, 1e-3]).unwrap_or(&1e-9);
            // the whole SmSurfParamType range; UMIN..OUT take the early return
            let closure = rng.below(10) as i64;
            // on, just outside, far outside, inside
            let pick = |rng: &mut Rng, d: f64| -> f64 {
                match rng.below(6) {
                    0 => 0.0,
                    1 => d,
                    2 => -rng.uniform(0.0, 2.0) * tol.max(1e-12),
                    3 => d + rng.uniform(0.0, 2.0) * tol.max(1e-12),
                    4 => rng.uniform(-1.5, 2.5) * d,
                    _ => rng.uniform(0.0, 1.0) * d,
                }
            };
            let mut p = [pick(rng, du), pick(rng, dv)];
            if rng.chance(0.05) {
                if rng.chance(0.5) {
                    p[0] = -1.0e20;
                } else {
                    p[1] = -1.0e20;
                }
            }
            if kind == 1 {
                Inputs::CheckSeamOut {
                    surface,
                    tol,
                    closure,
                    uv: p,
                }
            } else {
                Inputs::CheckSeamOn {
                    surface,
                    tol,
                    closure,
                    uv: p,
                }
            }
        }
        3 => {
            let closed = rng.chance(0.5);
            let surface = if closed {
                gen_cylinder(rng, scale, scale)
            } else {
                gen_surface(rng, scale)
            };
            let (du, dv) = surface_domain(&surface);
            let sub = rng.chance(0.3);
            let domain = if sub {
                let a = rng.uniform(0.0, 0.4);
                let b = rng.uniform(0.6, 1.0);
                [[a * du, 0.0], [b * du, dv]]
            } else {
                [[0.0, 0.0], [du, dv]]
            };
            Inputs::SurfaceClosure { surface, domain }
        }
        4 => {
            let n = 2 + rng.below(4) as usize;
            let pnts = (0..n)
                .map(|_| {
                    let d = if rng.chance(0.1) {
                        [0.0, 0.0, 0.0]
                    } else {
                        rng.point3(-1.0, 1.0)
                    };
                    [rng.point3(-scale, scale), d]
                })
                .collect();
            let i1 = rng.below(n as u64) as usize;
            let i2 = rng.below(n as u64) as usize;
            Inputs::IsOrderCorrect { pnts, i1, i2 }
        }
        5 => {
            let closed = rng.chance(0.7);
            let surface = if closed {
                gen_cylinder(rng, scale, scale)
            } else {
                gen_surface(rng, scale)
            };
            let (du, dv) = surface_domain(&surface);
            let on_seam = rng.below(4);
            let p = [
                match on_seam {
                    0 => 0.0 + rng.uniform(0.0, 0.0009) * du,
                    1 => du - rng.uniform(0.0, 0.0009) * du,
                    _ => rng.uniform(0.1, 0.9) * du,
                },
                match rng.below(3) {
                    0 => rng.uniform(0.0, 0.0009) * dv,
                    1 => dv - rng.uniform(0.0, 0.0009) * dv,
                    _ => rng.uniform(0.1, 0.9) * dv,
                },
            ];
            let ref_dir = if rng.chance(0.05) {
                [0.0, 0.0, 0.0]
            } else {
                rng.point3(-1.0, 1.0)
            };
            Inputs::AdjustUvOnSeam {
                surface,
                ref_dir,
                uv: p,
            }
        }
        6 => {
            let start = rng.point3(-scale, scale);
            let end = match rng.below(8) {
                0 => start,
                1 => [start[0] + scale, start[1], start[2]],
                _ => rng.point3(-scale, scale),
            };
            let chord = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
            let svec = match rng.below(6) {
                0 => chord,
                1 => [0.0, 0.0, 0.0],
                2 => [chord[1], -chord[0], 0.0],
                _ => rng.point3(-1.0, 1.0),
            };
            let evec = match rng.below(6) {
                0 => chord,
                1 => [-chord[1], chord[0], 0.0],
                2 => [-svec[0], -svec[1], -svec[2]],
                3 => svec,
                _ => rng.point3(-1.0, 1.0),
            };
            let dim = *rng.choose(&[2u64, 3, 3, 3, 0, 1, 4]).unwrap_or(&3);
            Inputs::Interpolate {
                start,
                end,
                svec,
                evec,
                dim,
            }
        }
        7 => {
            let closed = rng.chance(0.4);
            let surface = if closed {
                gen_cylinder(rng, scale, scale)
            } else {
                gen_surface(rng, scale)
            };
            // a point near the surface: a control point, or a random point
            let pnt = if rng.chance(0.6) {
                let i = rng.below(surface.net.len() as u64) as usize;
                let row = surface.net.get(i).cloned().unwrap_or_default();
                let j = rng.below(row.len().max(1) as u64) as usize;
                let [x, y, z, w] = row.get(j).copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
                let d = if w == NOW { 1.0 } else { w };
                let off = rng.uniform(-1e-4, 1e-4) * scale;
                [x / d + off, y / d, z / d]
            } else {
                rng.point3(-scale, scale)
            };
            // negative tolerances too: GlobalPointSolve's SmZoneTol3d takes fabs
            let tol = *rng
                .choose(&[1e-9, 1e-6, 1e-5, 1e-3, 1e-1, -1e-6, -1e-3])
                .unwrap_or(&1e-6)
                * scale;
            Inputs::SurfaceDropPoint { surface, pnt, tol }
        }
        8 => {
            let surface1 = gen_surface(rng, scale);
            let surface2 = gen_surface(rng, scale);
            let on = rng.chance(0.5);
            let start = if on {
                let [x, y, z, w] = surface1
                    .net
                    .first()
                    .and_then(|r| r.first())
                    .copied()
                    .unwrap_or([0.0, 0.0, 0.0, NOW]);
                let d = if w == NOW { 1.0 } else { w };
                [x / d, y / d, z / d]
            } else {
                rng.point3(-scale, scale)
            };
            let end = if rng.chance(0.5) {
                Some(rng.point3(-scale, scale))
            } else {
                None
            };
            let dir = if rng.chance(0.05) {
                [0.0, 0.0, 0.0]
            } else {
                rng.point3(-1.0, 1.0)
            };
            let end_dir = if rng.chance(0.5) {
                Some(rng.point3(-1.0, 1.0))
            } else {
                None
            };
            let tol = *rng
                .choose(&[1e-9, 1e-6, 1e-4, 1e-2, -1e-6, -1e-2])
                .unwrap_or(&1e-6)
                * scale;
            Inputs::TwoPnts {
                surface1,
                surface2,
                start,
                end,
                dir,
                end_dir,
                tol,
            }
        }
        9 => {
            if rng.chance(0.4) {
                return gen_evaluate_mate_analytic(rng, scale.max(1e-2));
            }
            // upstream's CreateFaceFromSurface rejects surfaces below its tolerance scale
            let surface = gen_surface(rng, scale.max(1e-2));
            let (du, dv) = surface_domain(&surface);
            let inside = rng.chance(0.5);
            let p = if inside {
                [rng.uniform(0.0, 1.0) * du, rng.uniform(0.0, 1.0) * dv]
            } else {
                [rng.uniform(-0.5, 1.5) * du, rng.uniform(-0.5, 1.5) * dv]
            };
            Inputs::EvaluateMate { surface, uv: p }
        }
        10 => {
            let surface = gen_surface(rng, scale);
            let (du, dv) = surface_domain(&surface);
            let mut uv1 = gen_uv_line(rng, du, dv);
            let uv2 = gen_uv_line(rng, du, dv);
            if rng.chance(0.3) {
                // end of curve 1 already on the boundary
                if let Some(last) = uv1.cpts.last_mut() {
                    last[0] = du;
                }
            }
            Inputs::Capping {
                // negative tolerances too: LiftCurve's SmApproxTol3d takes fabs
                tol: *rng
                    .choose(&[1e-9, 1e-6, 1e-3, -1e-6, -1e-3])
                    .unwrap_or(&1e-6)
                    * scale,
                surface,
                uv1,
                orient1: rng.below(3) as i64,
                uv2,
                orient2: rng.below(3) as i64,
            }
        }
        _ => {
            let surface1 = gen_surface(rng, scale.max(1e-2));
            let surface2 = gen_surface(rng, scale.max(1e-2));
            Inputs::ClassifyFacePoint {
                surface1,
                surface2,
                pos: rng.point3(-scale, scale),
            }
        }
    }
}

/// A random pseudo-B-rep model with 2..5 vertices, 0..3 geometries and 1..4 edges.
pub(crate) fn gen_model(rng: &mut Rng) -> Model {
    let scale = scale_of(rng);
    let nt = rng.below(3) as usize;
    let targets = (0..nt)
        .map(|_| gen_curve(rng, 3, scale))
        .collect::<Vec<_>>();
    let nf = rng.below(3) as usize;
    let faces = (0..nf)
        .map(|_| gen_surface(rng, scale.max(1e-2)))
        .collect::<Vec<_>>();
    let nfb = rng.below(4) as usize;
    let fillet_brep = (0..nfb)
        .map(|_| FbEdgeIn {
            curve: gen_curve(rng, 3, scale),
            tol: *rng.choose(&[1e-5, 1e-3, 1e-9, 1.0]).unwrap_or(&1e-5) * scale,
        })
        .collect::<Vec<_>>();
    let nv = 2 + rng.below(4) as usize;
    let ng = rng.below(3) as usize;
    let ne = 1 + rng.below(4) as usize;
    let vertices = (0..nv)
        .map(|_| {
            let (pc_class, pc_kind, pc_index, pc_sub) = match rng.below(5) {
                0 if nt > 0 => (104, 1, rng.below(nt as u64) as i64, rng.below(2) as i64),
                1 if nt > 0 => (109, 2, rng.below(nt as u64) as i64, 0),
                2 if nf > 0 => (102, 3, rng.below(nf as u64) as i64, 0),
                // the other classes carry no object in this model
                _ => (
                    *rng.choose(&[100, 101, 103, 105, 106, 107, 108, 110])
                        .unwrap_or(&100),
                    0,
                    -1,
                    0,
                ),
            };
            VertexIn {
                point: rng.point3(-scale, scale),
                vtype: rng.below(14) as i64,
                status: rng.below(13) as i64,
                corner: rng.range_i64(-1, 3),
                pc_class,
                pc_kind,
                pc_index,
                pc_sub,
                pc_face: if nf > 0 {
                    rng.range_i64(-1, nf as i64 - 1)
                } else {
                    -1
                },
                t_param: rng.uniform(-1.0, 1.0),
                uv: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
                mates: [-1, -1],
            }
        })
        .collect::<Vec<_>>();
    let geoms = (0..ng)
        .map(|_| GeomIn {
            gtype: rng.below(7) as i64,
            status: rng.below(13) as i64,
            center: if rng.chance(0.5) {
                Some(gen_curve(rng, 3, scale))
            } else {
                None
            },
            surface: if rng.chance(0.7) {
                Some(gen_surface(rng, scale))
            } else {
                None
            },
            rails: [-1, -1],
            other1: Vec::new(),
            other2: Vec::new(),
            sides: Vec::new(),
        })
        .collect::<Vec<_>>();
    let edges = (0..ne)
        .map(|_| EdgeIn {
            vs: rng.below(nv as u64) as usize,
            ve: rng.below(nv as u64) as usize,
            etype: rng.below(15) as i64,
            status: rng.below(13) as i64,
            corner: rng.range_i64(-1, 3),
            curve: if rng.chance(0.8) {
                Some(gen_curve(rng, 3, scale))
            } else {
                None
            },
            interval: if rng.chance(0.7) {
                let a = rng.uniform(0.0, 0.5);
                let b = a + rng.uniform(0.0, 0.5);
                Some(if rng.chance(0.1) { [b, a] } else { [a, b] })
            } else {
                None
            },
            fbe: [
                if nfb > 0 {
                    rng.range_i64(-1, nfb as i64 - 1)
                } else {
                    -1
                },
                if nfb > 0 {
                    rng.range_i64(-1, nfb as i64 - 1)
                } else {
                    -1
                },
            ],
            eus: [
                EuIn {
                    uv_curve: if rng.chance(0.4) {
                        Some(gen_curve(rng, 2, 1.0))
                    } else {
                        None
                    },
                    geom: if ng > 0 {
                        rng.range_i64(-1, ng as i64 - 1)
                    } else {
                        -1
                    },
                    vu_uv: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
                },
                EuIn {
                    uv_curve: if rng.chance(0.4) {
                        Some(gen_curve(rng, 2, 1.0))
                    } else {
                        None
                    },
                    geom: if ng > 0 {
                        rng.range_i64(-1, ng as i64 - 1)
                    } else {
                        -1
                    },
                    vu_uv: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
                },
            ],
        })
        .collect::<Vec<_>>();
    let mut m = Model {
        targets,
        faces,
        fillet_brep,
        vertices,
        geoms,
        edges,
    };
    for g in &mut m.geoms {
        for rail in &mut g.rails {
            *rail = if rng.chance(0.7) {
                rng.below(ne as u64) as i64
            } else {
                -1
            };
        }
        for _ in 0..rng.below(3) {
            g.other1.push(rng.below(ne as u64) as usize);
        }
        for _ in 0..rng.below(2) {
            g.other2.push(rng.below(ne as u64) as usize);
        }
        for _ in 0..rng.below(3) {
            g.sides
                .push([rng.below(ne as u64) as usize, rng.below(2) as usize]);
        }
    }
    for v in &mut m.vertices {
        for mate in &mut v.mates {
            if rng.chance(0.3) {
                *mate = rng.below(nv as u64) as i64;
            }
        }
    }
    m
}

fn gen_query(rng: &mut Rng, m: &Model) -> Query {
    let nv = m.vertices.len();
    let ne = m.edges.len();
    let ng = m.geoms.len();
    let nf = m.faces.len();
    let nfb = m.fillet_brep.len();
    let pick = |rng: &mut Rng, n: usize| rng.below(n as u64) as usize;
    let rail_ok = |g: usize| {
        m.geoms
            .get(g)
            .is_some_and(|g| g.rails.iter().all(|&r| r >= 0))
    };
    let free_vertex = (0..nv).find(|&v| !m.edges.iter().any(|e| e.vs == v || e.ve == v));
    let vertex_kind = |k: i64| m.vertices.iter().position(|v| v.pc_kind == k);
    let curved_edge = m.edges.iter().position(|e| e.curve.is_some());
    let with_surface = (0..ng)
        .filter(|&g| m.geoms.get(g).is_some_and(|g| g.surface.is_some()))
        .collect::<Vec<_>>();
    match rng.below(20) {
        0 => Query::Export,
        1 => Query::VertexGeoms { v: pick(rng, nv) },
        2 => Query::VuAtRailEnd {
            v: pick(rng, nv),
            geom: if ng > 0 {
                rng.range_i64(-1, ng as i64 - 1)
            } else {
                -1
            },
            exclude: if rng.chance(0.4) {
                Some([pick(rng, ne), pick(rng, 2)])
            } else {
                None
            },
            rail: rng.range_i64(-1, ne as i64 - 1),
        },
        3 if (0..ng).any(rail_ok) => {
            let g = (0..ng).filter(|&g| rail_ok(g)).collect::<Vec<_>>();
            Query::RailVertexuse {
                g: g.get(pick(rng, g.len())).copied().unwrap_or(0),
                rail: pick(rng, 2),
                end: pick(rng, 2),
            }
        }
        4 => Query::EdgeName { e: pick(rng, ne) },
        5 if curved_edge.is_some() => Query::FindEdge {
            e: curved_edge.unwrap_or(0),
            multi: rng.chance(0.5),
            cands: (0..rng.below(nfb as u64 + 1))
                .map(|_| pick(rng, nfb))
                .collect(),
        },
        6 => Query::ClearEdges {
            e: pick(rng, ne),
            edges: (0..rng.below(nfb as u64 + 1))
                .map(|_| pick(rng, nfb))
                .collect(),
        },
        7 => Query::BrepEdges { e: pick(rng, ne) },
        8 if (0..ng).any(rail_ok) => {
            let g = (0..ng).filter(|&g| rail_ok(g)).collect::<Vec<_>>();
            let a = rng.uniform(-0.2, 0.6);
            let b = a + rng.uniform(0.0, 0.8);
            Query::Trim {
                g: g.get(pick(rng, g.len())).copied().unwrap_or(0),
                interval: if rng.chance(0.1) { [b, a] } else { [a, b] },
            }
        }
        9 if free_vertex.is_some() => Query::DeleteVertex {
            v: free_vertex.unwrap_or(0),
        },
        10 => Query::DeleteEdge { e: pick(rng, ne) },
        11 if vertex_kind(1).is_some() => Query::VertOnVert {
            v: vertex_kind(1).unwrap_or(0),
        },
        12 if vertex_kind(2).is_some() => Query::Cliff {
            v: vertex_kind(2).unwrap_or(0),
            surface: gen_surface(rng, 1.0),
            tol: 1e-6,
        },
        13 if nf > 0 => Query::SplitFace {
            v: pick(rng, nv),
            f1: pick(rng, nf),
            f2: pick(rng, nf),
        },
        14 if with_surface.len() >= 2 => Query::Examine {
            g1: with_surface.first().copied().unwrap_or(0),
            at1: rng.chance(0.5),
            g2: with_surface.get(1).copied().unwrap_or(1),
            at2: rng.chance(0.5),
            sv: pick(rng, nv),
            ev: pick(rng, nv),
            tol: *rng.choose(&[1e-6, 1e-3, 1.0, 10.0]).unwrap_or(&1e-6),
        },
        15 => Query::SetEuGeom {
            e: pick(rng, ne),
            eu: pick(rng, 2),
            geom: if ng > 0 {
                rng.range_i64(-1, ng as i64 - 1)
            } else {
                -1
            },
        },
        16 => Query::SetUvCurve {
            e: pick(rng, ne),
            eu: pick(rng, 2),
            curve: if rng.chance(0.7) {
                Some(gen_curve(rng, 2, 1.0))
            } else {
                None
            },
            delete: rng.chance(0.5),
            warn: rng.chance(0.5),
        },
        17 => Query::OrigUvCurve { e: pick(rng, ne) },
        18 => Query::SetVuProperty {
            e: pick(rng, ne),
            eu: pick(rng, 2),
            te: pick(rng, ne),
            teu: pick(rng, 2),
        },
        _ => Query::RemoveEdge {
            e: pick(rng, ne),
            readd: rng.chance(0.5),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_geom_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let c: CaseInputs = case.parse()?;
        cpp(&c.inputs, c.expected_rejection)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse::<CaseInputs>()?.inputs;
        // an input the FFI refuses is a harness error on both sides: the Rust port loops
        // as upstream does in the guarded hang regimes (HANG-20), so it must not run them
        // (fuzz mutations of the flagged fixtures reach them unflagged)
        ffi::validate(i.op(), &cpp_input(&i)?).map_err(|e| harness(e.to_string()))?;
        let rec = match &i {
            Inputs::Fb { model, query } => rust_pseudo(model, query)?,
            _ => rust_geometry(&i)?,
        };
        match rec {
            Rec::Flat(flat) => to_output(&i, &flat),
            Rec::Deferred(e) => {
                Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())))
            }
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let inputs = if rng.chance(0.5) {
            let kind = rng.below(12);
            gen_geometry(rng, kind)
        } else {
            let model = gen_model(rng);
            let query = gen_query(rng, &model);
            Inputs::Fb { model, query }
        };
        serde_json::to_value(inputs).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutates generated and fixture programs and feeds them to the FFI: every program
    /// the guards accept must run without crashing (a crash kills the test process) or
    /// hanging (30 s per call; 180 s for `ClassifyFacePoint` and `FbSplitFace`, whose
    /// accepted inputs can take up to about 100 s under the HANG-29 bound). `cargo test
    /// -p difftest --features cpp -- --ignored fuzz_guards`; `FUZZ_SEED` and
    /// `FUZZ_CALLS` override the seed (`0xfa11ba11`) and the call count (60k). A clean
    /// sweep is evidence, not proof: the pre-review's sweeps missed UB-166 and the slow
    /// HANG-29 inputs that a different mutation mix found.
    #[test]
    #[ignore = "60k FFI calls; run on demand"]
    fn fuzz_guards() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 0,
        };
        let mut bases: Vec<(Op, Vec<f64>)> = Vec::new();
        for case in Runner.cases(&ctx).unwrap_or_default() {
            if let Ok(i) = case.parse::<Inputs>() {
                if let Ok(flat) = cpp_input(&i) {
                    bases.push((i.op(), flat));
                }
            }
        }
        for i in 0..2000 {
            let mut rng = Rng::new(0x5eed_f00d + i);
            if let Some(v) = Runner.generate(&mut rng) {
                let inputs: Inputs = serde_json::from_value(v).unwrap();
                bases.push((inputs.op(), cpp_input(&inputs).unwrap()));
            }
        }
        let specials = [
            0.0,
            1.0,
            -1.0,
            2.0,
            0.5,
            100.0,
            1e300,
            -1e300,
            1e-300,
            f64::NAN,
            f64::INFINITY,
            -f64::INFINITY,
            1e20,
            -1e20,
            1e-12,
            13.0,
            999.0,
            // the guard boundaries: the magnitude cap, tiny positive weights, counts
            ffi::MAX_MAGNITUDE,
            -ffi::MAX_MAGNITUDE,
            1e149,
            1e-150,
            1e-6,
            1e-9,
            64.0,
            63.0,
            -0.0,
            // the HANG-29 slow range and the UB-166 knot fixup (scaled zero 2e-12 on [0, 1],
            // spread tolerance 1.2e-8 and its 1.01 increment)
            1e15,
            -1e15,
            1e30,
            1e-13,
            2e-12,
            3e-12,
            1e-11,
            5e-9,
            1e-8,
            1.2e-8,
            1.2e-8 * 1.01,
            2.4e-8,
            // the HANG-20 bound of the global-solver ops and the measured hangs above it
            ffi::MAX_SOLVER_MAGNITUDE,
            -ffi::MAX_SOLVER_MAGNITUDE,
            3e307,
            -7e307,
            8e307,
        ];
        let env_u64 = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        let seed = env_u64("FUZZ_SEED", 0xfa11_ba11);
        let max_calls = env_u64("FUZZ_CALLS", 60_000);
        let mut rng = Rng::new(seed);
        let (mut accepted, mut rejected) = (0u64, 0u64);
        let mut calls = 0u64;
        while calls < max_calls {
            let (op, base) = &bases[rng.below(bases.len() as u64) as usize];
            let mut input = base.clone();
            let n_mut = rng.below(6);
            for _ in 0..n_mut {
                match rng.below(9) {
                    0 if !input.is_empty() => {
                        let at = rng.below(input.len() as u64) as usize;
                        input[at] = specials[rng.below(specials.len() as u64) as usize];
                    }
                    1 if !input.is_empty() => {
                        let at = rng.below(input.len() as u64) as usize;
                        input[at] *= rng.uniform(-3.0, 3.0);
                    }
                    2 if !input.is_empty() => {
                        let at = rng.below(input.len() as u64) as usize;
                        input.truncate(at);
                    }
                    3 => input.push(specials[rng.below(specials.len() as u64) as usize]),
                    4 if input.len() > 1 => {
                        let a = rng.below(input.len() as u64) as usize;
                        let b = rng.below(input.len() as u64) as usize;
                        input.swap(a, b);
                    }
                    5 if input.len() > 1 => {
                        // a value a hair above its predecessor: inside a knot vector this
                        // is the near-coincident pair the constructor snaps or spreads
                        // (UB-166, both stages)
                        let at = 1 + rng.below(input.len() as u64 - 1) as usize;
                        let gap = [1e-13, 1e-12, 2e-12, 1e-11, 1e-9, 5e-9, 1e-8, 1.2e-8 * 1.01]
                            [rng.below(8) as usize];
                        input[at] = input[at - 1] + gap;
                    }
                    6 if input.len() > 1 => {
                        // a run of values compressed towards its predecessor: a knot
                        // vector whose whole range drops under the spread tolerance, the
                        // regime where the constructor's second stage moves knots onto
                        // the start knot (UB-166)
                        let at = 1 + rng.below(input.len() as u64 - 1) as usize;
                        let end = (at + 1 + rng.below(8) as usize).min(input.len());
                        let factor = [1e-7, 1e-8, 1e-9][rng.below(3) as usize];
                        let base = input[at - 1];
                        for x in input.iter_mut().take(end).skip(at) {
                            *x = base + (*x - base) * factor;
                        }
                    }
                    7 if input.len() > 1 => {
                        // a run of equal values a hair above their predecessor: an end
                        // knot group the constructor's first stage snaps back before the
                        // spread (UB-166's end-snap route)
                        let at = 1 + rng.below(input.len() as u64 - 1) as usize;
                        let end = (at + 1 + rng.below(4) as usize).min(input.len());
                        let d = [3e-13, 5e-13, 9e-13, 2e-12][rng.below(4) as usize];
                        let value = input[at - 1] + d;
                        for x in input.iter_mut().take(end).skip(at) {
                            *x = value;
                        }
                    }
                    _ if !input.is_empty() => {
                        let at = rng.below(input.len() as u64) as usize;
                        input[at] = (input[at] + 1.0).round();
                    }
                    _ => {}
                }
            }
            let op = if rng.chance(0.05) {
                // the same program under a random op
                let ops = [
                    Op::CleanupNoise,
                    Op::CheckSeamOut,
                    Op::CheckSeamOn,
                    Op::SurfaceClosure,
                    Op::IsOrderCorrect,
                    Op::AdjustUvOnSeam,
                    Op::Interpolate,
                    Op::SurfaceDropPoint,
                    Op::TwoPnts,
                    Op::EvaluateMate,
                    Op::Capping,
                    Op::ClassifyFacePoint,
                    Op::EvaluateMateAnalytic,
                    Op::FbExport,
                    Op::FbVertexGeoms,
                    Op::FbVuAtRailEnd,
                    Op::FbRailVertexuse,
                    Op::FbEdgeName,
                    Op::FbFindEdge,
                    Op::FbClearEdges,
                    Op::FbTrim,
                    Op::FbDeleteVertex,
                    Op::FbDeleteEdge,
                    Op::FbVertOnVert,
                    Op::FbCliff,
                    Op::FbSplitFace,
                    Op::FbExamine,
                    Op::FbSetEuGeom,
                    Op::FbSetUvCurve,
                    Op::FbOrigUvCurve,
                    Op::FbSetVuProperty,
                    Op::FbBrepEdges,
                    Op::FbRemoveEdge,
                ];
                ops[rng.below(ops.len() as u64) as usize]
            } else {
                *op
            };
            // Each call runs on its own thread so a C++ hang fails the test with the
            // offending program printed instead of stalling the sweep forever.
            let (tx, rx) = std::sync::mpsc::channel();
            let program = input.clone();
            std::thread::spawn(move || {
                let _ = tx.send(ffi::eval(op, &program).map(|_| ()));
            });
            let limit = if matches!(op, Op::ClassifyFacePoint | Op::FbSplitFace) {
                180
            } else {
                30
            };
            match rx.recv_timeout(std::time::Duration::from_secs(limit)) {
                Ok(Ok(()) | Err(solidcore_ffi::FfiError::Status(_))) => accepted += 1,
                Ok(Err(_)) => rejected += 1,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!(
                    "fuzz_guards: call {calls} (op {}) hung for {limit} s; input = {}",
                    op as i32,
                    serde_json::to_string(&input).unwrap()
                ),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!(
                    "fuzz_guards: call {calls} (op {}) died; input = {}",
                    op as i32,
                    serde_json::to_string(&input).unwrap()
                ),
            }
            calls += 1;
            if calls.is_multiple_of(5000) {
                eprintln!("fuzz_guards: {calls} calls ({accepted} ran, {rejected} rejected)");
            }
        }
        eprintln!("fuzz_guards: {calls} calls, {accepted} ran, {rejected} rejected by the wrapper");
    }

    /// Runs chosen cases through one side, for memory checking: `cargo test -p difftest
    /// --features cpp --lib --no-run`, then the test binary under valgrind with
    /// `--ignored case_sample --nocapture`. `CASE_SIDE` is `cpp` (default) or `rust`;
    /// `CASE_SEEDS` (default 2000) sizes the case list as `--seeds` does;
    /// `CASE_INDICES=<i>,..` picks cases by their index in it, otherwise
    /// `CASE_OPS=<op>,..` picks the cases of those ops (default the pseudo-B-rep ops 20 to
    /// 39), at most `CASE_PER_OP` of each (default all), leaving out `CASE_SKIP=<i>,..`
    /// (cases too heavy for valgrind). `CASE_DRY` only prints the picked indices.
    /// `CASE_CENSUS` instead tallies, per op, the Rust outcome of every case (status `0`,
    /// a non-zero upstream status, a deferral or a harness error); with no failed case
    /// those tallies are the passes' success-path / error-status split (flagged
    /// `expected_rejection` cases are left out).
    #[test]
    #[ignore = "runs cases for valgrind; run on demand"]
    fn case_sample() {
        let env = |name: &str| std::env::var(name).ok();
        let list = |text: String| -> Vec<i64> {
            text.split(',').map(|t| t.trim().parse().unwrap()).collect()
        };
        let rust_side = env("CASE_SIDE").is_some_and(|s| s == "rust");
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: env("CASE_SEEDS").map_or(2000, |s| s.parse().unwrap()),
        };
        let cases = Runner.cases(&ctx).unwrap();
        if env("CASE_CENSUS").is_some() {
            let mut tally = BTreeMap::<(i32, &str), usize>::new();
            for case in &cases {
                // flagged regression inputs are rejected, never passes (and the HANG-20
                // ones loop on the Rust side too)
                let Ok(CaseInputs {
                    expected_rejection: false,
                    inputs,
                }) = case.parse::<CaseInputs>()
                else {
                    continue;
                };
                let kind = match Runner.run_rust(case) {
                    Ok(o) if o.deferred.is_some() => "deferred",
                    // `evaluate_mate_analytic`: the evaluation's status once the
                    // surface was built
                    Ok(o) => match o
                        .values
                        .get("status")
                        .and_then(|v| v.first())
                        .and_then(|&st| {
                            if st == 0.0 && inputs.op() == Op::EvaluateMateAnalytic {
                                o.values.get("mate_status").and_then(|v| v.first()).copied()
                            } else {
                                Some(st)
                            }
                        })
                        .map(|st| st == 0.0)
                    {
                        Some(true) => "status0",
                        Some(false) => "error_status",
                        None => "no_status",
                    },
                    Err(_) => "harness_error",
                };
                *tally.entry((inputs.op() as i32, kind)).or_default() += 1;
            }
            for ((op, kind), n) in tally {
                eprintln!("case_census: op {op} {kind} {n}");
            }
            return;
        }
        let indices: Vec<usize> = if let Some(t) = env("CASE_INDICES") {
            list(t).into_iter().map(|i| i as usize).collect()
        } else {
            let ops = env("CASE_OPS").map_or_else(|| (20..40).collect(), list);
            let per_op = env("CASE_PER_OP").map_or(usize::MAX, |s| s.parse().unwrap());
            let skip = env("CASE_SKIP").map_or_else(Vec::new, list);
            let mut taken = BTreeMap::<i64, usize>::new();
            (0..cases.len())
                .filter(|&i| {
                    let Ok(inp) = cases[i].parse::<Inputs>() else {
                        return false;
                    };
                    let op = inp.op() as i64;
                    let n = taken.entry(op).or_default();
                    if !ops.contains(&op) || *n >= per_op || skip.contains(&(i as i64)) {
                        return false;
                    }
                    *n += 1;
                    true
                })
                .collect()
        };
        if env("CASE_DRY").is_some() {
            eprintln!("case_sample: indices {indices:?}");
            return;
        }
        for i in indices {
            let case = &cases[i];
            let inputs: Inputs = case.parse().unwrap();
            let r = if rust_side {
                format!(
                    "{:?}",
                    Runner
                        .run_rust(case)
                        .map(|o| (o.error, o.deferred.map(|d| d.unit)))
                )
            } else {
                format!(
                    "{:?}",
                    cpp_input(&inputs).map(|flat| {
                        ffi::eval(inputs.op(), &flat)
                            .map(|r| r.into_iter().take(2).collect::<Vec<_>>())
                    })
                )
            };
            eprintln!(
                "case_sample: {i} {} op {} -> {r}",
                case.name,
                inputs.op() as i32
            );
        }
    }

    /// Replays one flat program against the FFI with a timing: `REPLAY_FILE` names a
    /// JSON `{"op": <Op as i32>, "input": [..]}` (the shape `fuzz_guards` prints for a
    /// hang or crash, plus the op); `REPLAY_SUBST=<old>=<new>` first replaces every
    /// value equal to `old` by `new`, `REPLAY_SET=<index>=<value>,..` sets single slots;
    /// `REPLAY_TIMEOUT` (s, default 300) bounds the call. A crash kills the test
    /// process, so a crashing program prints nothing (the runner reports the signal).
    /// `cargo test -p difftest --features cpp -- --ignored replay_program --nocapture`.
    #[test]
    #[ignore = "replays a program named by REPLAY_FILE"]
    fn replay_program() {
        let Ok(file) = std::env::var("REPLAY_FILE") else {
            return;
        };
        let text = std::fs::read_to_string(&file).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let op_code = v["op"].as_i64().unwrap();
        let op = Op::from_code(op_code).unwrap();
        let mut input: Vec<f64> = serde_json::from_value(v["input"].clone()).unwrap();
        if let Ok(subst) = std::env::var("REPLAY_SUBST") {
            let (old, new) = subst.split_once('=').unwrap();
            let (old, new): (f64, f64) = (old.parse().unwrap(), new.parse().unwrap());
            for x in &mut input {
                if *x == old {
                    *x = new;
                }
            }
        }
        if let Ok(sets) = std::env::var("REPLAY_SET") {
            for set in sets.split(',') {
                let (at, value) = set.split_once('=').unwrap();
                input[at.parse::<usize>().unwrap()] = value.parse().unwrap();
            }
        }
        let limit: u64 = std::env::var("REPLAY_TIMEOUT")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(300);
        let (tx, rx) = std::sync::mpsc::channel();
        let program = input.clone();
        let t0 = std::time::Instant::now();
        std::thread::spawn(move || {
            let _ =
                tx.send(ffi::eval(op, &program).map(|r| r.into_iter().take(4).collect::<Vec<_>>()));
        });
        let Ok(r) = rx.recv_timeout(std::time::Duration::from_secs(limit)) else {
            eprintln!("replay_program: op {op_code} did not return within {limit} s");
            std::process::exit(0);
        };
        eprintln!(
            "replay_program: op {op_code} -> {r:?} in {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn generated_cases_cover_every_op() {
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("blend.fillet_geom_p1", i));
            let v = Runner.generate(&mut rng).unwrap();
            let inputs: Inputs = serde_json::from_value(v).unwrap();
            seen.insert(inputs.op() as i32);
        }
        for op in [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 20, 21, 22, 24, 26, 27, 29, 35, 39,
        ] {
            assert!(seen.contains(&op), "op {op} never generated");
        }
    }
}
