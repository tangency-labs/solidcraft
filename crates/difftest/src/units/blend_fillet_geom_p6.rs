//! Runner for unit `blend.fillet_geom_p6`. C++: `solidcore_ffi::units::blend_fillet_geom_p6`.
//! Rust: `solidcore::blend::fillet_geom::p6`.
//!
//! Every case builds a target B-rep from a B-rep block of unit `topology.edgeuse_p1` (an
//! upstream `.smb` solid, a face or a wire network; the Rust side reads or builds it itself
//! in the shim's traversal order, `crate::own_reader`, so its faces own their surfaces as
//! in the C++ B-rep and the ops' surface caches reach the face through `BrepFaceServices`),
//! fillet solvers (a base `SmFilletSolver` with edge uses of the target, offset surfaces of
//! inline bases, optionally the model's fillet executive and reverse trimming), fillet
//! corners (the filleted target vertex, a constant corner type and the corner's tolerance)
//! and a pseudo B-rep of fillet vertices, geometries and edges, then runs one op on it and
//! compares the op's status, its results and the export of the whole pseudo B-rep (vertex
//! points, types, statuses, original UVs, classes and parameters; edge curves, intervals,
//! original edges and faces, UV trim curves, edge use geometries and vertex use UVs;
//! geometry statuses, side and referenced edge uses, fillet surfaces, centre lines,
//! deviations and offset surfaces).
//!
//! Selectors of target objects are taken modulo the export's counts. Points, parameters
//! and UVs can be given relative to the target (a fraction of an edge's interval or of a
//! face's surface domain); both sides get the resolved values.
//!
//! The Rust target holds all of its geometry exactly (`ImportTypes::default()`); the
//! import path below is the C++ side's selector resolution. Analytic target geometry the
//! shared B-rep export carries exactly (cone, cylinder,
//! sphere, torus, surfaces of revolution and extrusion rebuilt from their upstream
//! database record; lossless ellipses and circles) is compared as its own kind, through
//! `ImportTypes::inexact_surface` / `inexact_curve`. An op that evaluates a face surface
//! or edge curve the import still holds only as its B-spline part is deferred under
//! `harness:analytic-geometry-export` (`plan/findings.md`).
//!
//! `SmFilletGeom::TrimOriginalSurfaces` is not compared: it needs a fillet B-rep and the
//! executive's topology intersector mates the shim does not build; its Rust port is
//! covered by kernel tests.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_corner::p3::{FilletCorner, FilletCornerKind};
use solidcore::blend::fillet_custom_solver::{OffsetSurface, SurfaceId};
use solidcore::blend::fillet_executive::p3::ErrorInfo;
use solidcore::blend::fillet_geom::p1::{
    FilStatus, FilletBrep, FilletCornerId, FilletEdgeType, FilletEdgeuse,
    FilletErrorType, FilletGeom, FilletGeomId, FilletGeomType, FilletSolverId, FilletVertex,
    FilletVertexType, FilletVertexuse,
};
use solidcore::blend::fillet_geom::p3::new_solver_fillet_geom;
use solidcore::blend::fillet_geom::p6::{
    join_two_fillet_verts_by_curves, move_tsp, GeomErrorObject,
};
use solidcore::blend::fillet_solver::p1::{
    FilletExecutiveRef, FilletSolver, OffsetSurfaceSlot,
};
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::{zone_tol3d, USE_DEFAULT};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::point_class::{PointClassType, PointClassification};
use solidcore::intersect::surface_intersector::p1::{IntersectionPointType, SrfIndex, TsectPnt};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as teu;
use solidcore::topology::{
    Brep, EdgeId, Edgeuse, EdgeuseId, FaceId, VertexId, Vertexuse, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::blend_fillet_geom_p6::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;

use super::topology_edgeuse_p1::{
    brep_block_of, bspline_of, clamped_knots, import_with_types, path_of, push_curve, push_surface,
    sel, surface_of, BrepIn, BssIn, CrvIn, ImportTypes, SrfIn,
};
use crate::{HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn h(e: impl std::fmt::Display) -> HarnessError {
    harness(e.to_string())
}

// ---- inputs -----------------------------------------------------------------------------

fn minus_one() -> i64 {
    -1
}

fn no_pair() -> [i64; 2] {
    [-1, -1]
}

fn pc_unknown() -> i64 {
    100
}

fn edge_unknown() -> i64 {
    14
}

fn ptype_unknown() -> i64 {
    1
}

fn one() -> f64 {
    1.0
}

/// An offset surface: `SmOffsetSurface(dist, base)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffIn {
    base: SrfIn,
    dist: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverIn {
    tol: f64,
    ang: f64,
    #[serde(default)]
    tang: f64,
    /// Target edge-use selectors (`-1` none).
    #[serde(default = "no_pair")]
    eus: [i64; 2],
    #[serde(default)]
    surfs: [Option<OffIn>; 2],
    /// The solver's executive is the model's.
    #[serde(default)]
    exec: bool,
    #[serde(default)]
    reverse_trim: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CornerIn {
    /// The filleted target vertex's selector (`-1` none).
    #[serde(default = "minus_one")]
    vertex: i64,
    /// `SmFilletCornerType`.
    #[serde(default)]
    ctype: i64,
    #[serde(default = "one")]
    tol: f64,
}

/// A point given relative to the target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case")]
enum PointOn {
    /// The target edge's curve at a fraction of the edge's interval.
    Edge { edge: i64, frac: f64 },
    /// The target face's surface at fractions of its natural domain.
    Face { face: i64, uv: [f64; 2] },
    /// The vertex's own classification curve (an edge, or an edge use's edge) at its
    /// parameter.
    Pc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VertexIn {
    #[serde(default)]
    point: [f64; 3],
    /// Overrides `point` (plus `noise`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at: Option<PointOn>,
    #[serde(default)]
    noise: [f64; 3],
    #[serde(default)]
    vtype: i64,
    #[serde(default)]
    status: i64,
    #[serde(default = "minus_one")]
    corner: i64,
    #[serde(default = "pc_unknown")]
    pc_class: i64,
    /// `0` none, `1` target vertex, `2` target edge use, `3` target face, `4` target edge.
    #[serde(default)]
    pc_kind: i64,
    /// The classification object's selector.
    #[serde(default)]
    pc_index: i64,
    /// A target face selector (`-1` none).
    #[serde(default = "minus_one")]
    pc_face: i64,
    #[serde(default)]
    t: f64,
    /// Overrides `t`: a fraction of the classification edge's (or edge use's edge's)
    /// interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t_frac: Option<f64>,
    #[serde(default)]
    uv: [f64; 2],
    /// With a target face selector, `uv` are fractions of its surface's domain.
    #[serde(default = "minus_one")]
    uv_face: i64,
    #[serde(default = "no_pair")]
    mates: [i64; 2],
}

/// A vertex use's intersection point (`SmTsectPnt`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TsectIn {
    #[serde(default = "ptype_unknown")]
    ptype: i64,
    #[serde(default)]
    uv: [[f64; 2]; 2],
    #[serde(default)]
    crv: [f64; 3],
    #[serde(default)]
    deriv: [f64; 3],
    #[serde(default)]
    cparam: f64,
}

impl Default for TsectIn {
    fn default() -> Self {
        Self {
            ptype: 1,
            uv: [[0.0; 2]; 2],
            crv: [0.0; 3],
            deriv: [0.0; 3],
            cparam: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EuIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uv_curve: Option<CrvIn>,
    #[serde(default = "minus_one")]
    geom: i64,
    #[serde(default)]
    vu_uv: [f64; 2],
    #[serde(default)]
    tsect: TsectIn,
}

impl Default for EuIn {
    fn default() -> Self {
        eu(-1, None)
    }
}

fn eu(geom: i64, uv_curve: Option<CrvIn>) -> EuIn {
    EuIn {
        uv_curve,
        geom,
        vu_uv: [0.0; 2],
        tsect: TsectIn::default(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeIn {
    vs: usize,
    ve: usize,
    #[serde(default = "edge_unknown")]
    etype: i64,
    #[serde(default)]
    status: i64,
    #[serde(default = "minus_one")]
    corner: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    curve: Option<CrvIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interval: Option<[f64; 2]>,
    /// Target edge selector (`-1` none).
    #[serde(default = "minus_one")]
    orig_edge: i64,
    /// Target face selector (`-1` none).
    #[serde(default = "minus_one")]
    orig_face: i64,
    eus: [EuIn; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeomIn {
    #[serde(default)]
    gtype: i64,
    #[serde(default)]
    status: i64,
    #[serde(default = "minus_one")]
    solver: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surface: Option<SrfIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    center: Option<CrvIn>,
    #[serde(default)]
    devs: [f64; 2],
    #[serde(default)]
    offs: [Option<OffIn>; 2],
    #[serde(default = "no_pair")]
    rails: [i64; 2],
    #[serde(default)]
    other1: Vec<usize>,
    #[serde(default)]
    other2: Vec<usize>,
    #[serde(default)]
    sides: Vec<[usize; 2]>,
}

impl Default for GeomIn {
    fn default() -> Self {
        Self {
            gtype: 0,
            status: 0,
            solver: -1,
            surface: None,
            center: None,
            devs: [0.0; 2],
            offs: [None, None],
            rails: [-1, -1],
            other1: vec![],
            other2: vec![],
            sides: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ModelIn {
    #[serde(default)]
    solvers: Vec<SolverIn>,
    #[serde(default)]
    corners: Vec<CornerIn>,
    #[serde(default)]
    vertices: Vec<VertexIn>,
    #[serde(default)]
    geoms: Vec<GeomIn>,
    #[serde(default)]
    edges: Vec<EdgeIn>,
}

/// The start of a `Join` list: edge `e`'s curve, the UV trim curve of its use `which`
/// (`with_uv`, else `NULL`) and an orientation (`SmOrientType`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JoinStart {
    e: usize,
    #[serde(default)]
    which: usize,
    #[serde(default)]
    with_uv: bool,
    #[serde(default = "one_i")]
    orient: i64,
}

fn one_i() -> i64 {
    1
}

/// A point classification for `SetUpRailPoint`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PcIn {
    class: i64,
    #[serde(default)]
    kind: i64,
    #[serde(default)]
    index: i64,
    #[serde(default)]
    t: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Export,
    BlendingRail {
        e: usize,
    },
    EdgeOnEdge {
        e: usize,
        #[serde(default)]
        use_corner: bool,
    },
    FilletEnd {
        e: usize,
        #[serde(default)]
        use_corner: bool,
    },
    SetBackRail {
        e: usize,
        #[serde(default)]
        use_corner: bool,
    },
    ShouldSwap {
        e: usize,
        f1: i64,
        f2: i64,
    },
    Join {
        g: usize,
        vs: i64,
        ve: i64,
        uv_tol: f64,
        start: Vec<JoinStart>,
    },
    Copy {
        g: usize,
        other: usize,
    },
    FindTangent {
        g: usize,
        rail: usize,
        end: usize,
    },
    RailIndexSideEu {
        g: usize,
        eu: i64,
    },
    RailIndexVu {
        g: usize,
        e: usize,
        which: usize,
    },
    RailVertex {
        g: usize,
        rail: usize,
        end: usize,
    },
    SetUpRailPoint {
        g: usize,
        rail: usize,
        end: usize,
        param: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pc: Option<PcIn>,
    },
    MoveTsp {
        pnts: Vec<[[f64; 3]; 2]>,
        from: usize,
        to: usize,
    },
    RailEuIntersect {
        g: usize,
        rail: usize,
        eu: i64,
        guess: f64,
    },
    RecordError {
        g: usize,
        etype: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    model: ModelIn,
    #[serde(flatten)]
    op: OpIn,
    /// A regression input the FFI or the shim must refuse (upstream crashes on it): the
    /// case is recorded as rejected, never run on the Rust side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    expected_rejection: bool,
}

// ---- the target B-rep and resolution -------------------------------------------------------

/// The case's target B-rep: the C++ export's counts, the imported Rust B-rep and the
/// upstream types of its geometry.
struct Target {
    path: String,
    block: Vec<f64>,
    counts: [usize; 10],
    brep: Brep,
    types: ImportTypes,
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE`: the
/// geometry the B-rep import holds exactly.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// The deferral label of an op that evaluates target geometry the import holds only as
/// its B-spline part (`plan/findings.md`, "harness: analytic face/edge geometry in the
/// shared B-rep export").
const HARNESS_ANALYTIC_IMPORT: &str = "harness:analytic-geometry-export";

/// The Rust side's target: the source read by the Rust reader or built by the Rust
/// kernel in the shim's traversal order ([`crate::own_reader::edgeuse_graph_brep`]),
/// never imported from the C++ export, so all of its geometry is held exactly;
/// `Err(output)` when the Rust build fails, as [`target`] reports the shim's.
fn own_target(brep: &BrepIn) -> Result<Result<Target, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let brep = match crate::own_reader::edgeuse_graph_brep(&mut Kernel::default(), brep)? {
        Ok(b) => b,
        Err(e) => {
            return Ok(Err(Output::unported(&e).unwrap_or_else(|| {
                Output::new().values("flat", vec![1.0, crate::own_builder::build_status(&e)])
            })))
        }
    };
    Ok(Ok(Target {
        path,
        block,
        counts: crate::own_builder::counts(&brep),
        brep,
        types: ImportTypes::default(),
    }))
}

/// Builds the target on the C++ side (its selectors resolve on the shim's B-rep); `Err(output)` when it cannot be built.
fn target(brep: &BrepIn) -> Result<Result<Target, Output>, HarnessError> {
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    let ex = ffi1::eval(ffi1::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(counts) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::new().values("flat", vec![1.0, st])));
    };
    let (brep, types) = import_with_types(&ex)?;
    Ok(Ok(Target {
        path,
        block,
        counts,
        brep,
        types,
    }))
}

impl Target {
    fn n_f(&self) -> usize {
        self.counts[3]
    }
    fn n_eu(&self) -> usize {
        self.counts[6]
    }
    fn n_e(&self) -> usize {
        self.counts[7]
    }
    fn n_v(&self) -> usize {
        self.counts[9]
    }

    /// Whether target face `f`'s surface is imported exactly (a plane, a B-spline surface
    /// or an analytic surface rebuilt from its upstream database record).
    fn exact_face(&self, f: Option<FaceId>) -> bool {
        f.is_none_or(|f| {
            self.types
                .inexact_surface(f.index() as usize)
                .is_none_or(|t| EXACT_SURFACES.contains(&t))
        })
    }

    /// Whether target edge `e`'s curve is imported exactly.
    fn exact_edge(&self, e: Option<EdgeId>) -> bool {
        e.and_then(|e| self.types.inexact_curve(e.index() as usize))
            .is_none_or(|t| t < 0 || EXACT_CURVES.contains(&t))
    }

    fn face(&self, s: i64) -> Option<FaceId> {
        u32::try_from(sel(s, self.n_f()))
            .ok()
            .map(FaceId::from_index)
    }

    fn edge(&self, s: i64) -> Option<EdgeId> {
        u32::try_from(sel(s, self.n_e()))
            .ok()
            .map(EdgeId::from_index)
    }

    fn edgeuse(&self, s: i64) -> Option<EdgeuseId> {
        u32::try_from(sel(s, self.n_eu()))
            .ok()
            .map(EdgeuseId::from_index)
    }

    /// `(u0, v0, u1, v1)` of the face's surface domain.
    fn face_domain(&self, f: FaceId) -> Result<Option<[f64; 4]>, HarnessError> {
        let Some(srf) = self.brep.face(f).map_err(h)?.surface.as_ref() else {
            return Ok(None);
        };
        // (runner helper without a kernel in scope)
        let d = srf.natural_uv_domain_if_built();
        let (u, v) = (d.u_interval(), d.v_interval());
        Ok(Some([u.min(), v.min(), u.max(), v.max()]))
    }

    /// A resolved classification object `(kind, export index)`.
    fn pc(&self, kind: i64, s: i64) -> (i64, i64) {
        let i = match kind {
            1 => sel(s, self.n_v()),
            2 => sel(s, self.n_eu()),
            3 => sel(s, self.n_f()),
            4 => sel(s, self.n_e()),
            _ => -1,
        };
        if i < 0 {
            (0, -1)
        } else {
            (kind, i)
        }
    }

    /// The edge of a classification object (an edge, or an edge use's edge).
    fn pc_edge(&self, kind: i64, idx: i64) -> Result<Option<EdgeId>, HarnessError> {
        let Ok(i) = u32::try_from(idx) else {
            return Ok(None);
        };
        match kind {
            4 => Ok(Some(EdgeId::from_index(i))),
            2 => teu::edge(&self.brep, EdgeuseId::from_index(i)).map_err(h),
            _ => Ok(None),
        }
    }

    /// The parameter at `frac` of the edge's interval and the curve's point there.
    fn on_edge(&self, e: EdgeId, frac: f64) -> Result<Option<(f64, [f64; 3])>, HarnessError> {
        let edge = self.brep.edge(e).map_err(h)?;
        let Some(c) = edge.curve.as_ref() else {
            return Ok(None);
        };
        let ivl = if edge.interval.is_init() || edge.interval.has_negative_length() {
            c.natural_interval()
        } else {
            edge.interval
        };
        let t = ivl.min() + (ivl.max() - ivl.min()) * frac;
        // harness-side input resolution: both sides receive the resolved point
        Ok(c.evaluate_point(&Kernel::default(), t).ok().map(|p| (t, [p.x, p.y, p.z])))
    }

    fn on_face(&self, f: FaceId, uv: [f64; 2]) -> Result<Option<[f64; 3]>, HarnessError> {
        let Some([u0, v0, u1, v1]) = self.face_domain(f)? else {
            return Ok(None);
        };
        let Some(srf) = self.brep.face(f).map_err(h)?.surface.as_ref() else {
            return Ok(None);
        };
        let p = Point2::new(u0 + (u1 - u0) * uv[0], v0 + (v1 - v0) * uv[1]);
        Ok(srf
            .evaluate_point(&Kernel::default(), p)
            .ok()
            .map(|p| [p.x, p.y, p.z]))
    }
}

/// A vertex, resolved against the target.
#[derive(Debug, Clone)]
struct Vertex {
    point: [f64; 3],
    pc_kind: i64,
    pc_index: i64,
    pc_face: i64,
    t: f64,
    uv: [f64; 2],
}

fn resolve_vertex(v: &VertexIn, tg: &Target) -> Result<Vertex, HarnessError> {
    let (pc_kind, pc_index) = tg.pc(v.pc_kind, v.pc_index);
    let pc_edge = tg.pc_edge(pc_kind, pc_index)?;
    let mut t = v.t;
    if let (Some(frac), Some(e)) = (v.t_frac, pc_edge) {
        if let Some((tt, _)) = tg.on_edge(e, frac)? {
            t = tt;
        }
    }
    let mut point = v.point;
    match &v.at {
        Some(PointOn::Edge { edge, frac }) => {
            if let Some(e) = tg.edge(*edge) {
                if let Some((_, p)) = tg.on_edge(e, *frac)? {
                    point = p;
                }
            }
        }
        Some(PointOn::Face { face, uv }) => {
            if let Some(f) = tg.face(*face) {
                if let Some(p) = tg.on_face(f, *uv)? {
                    point = p;
                }
            }
        }
        Some(PointOn::Pc) => {
            if let Some(e) = pc_edge {
                if let Some(c) = tg.brep.edge(e).map_err(h)?.curve.as_ref() {
                    if let Ok(p) = c.evaluate_point(&Kernel::default(), t) {
                        point = [p.x, p.y, p.z];
                    }
                }
            }
        }
        None => {}
    }
    for (p, n) in point.iter_mut().zip(v.noise) {
        *p += n;
    }
    let mut uv = v.uv;
    if let Some(f) = tg.face(v.uv_face) {
        if let Some([u0, v0, u1, v1]) = tg.face_domain(f)? {
            uv = [u0 + (u1 - u0) * uv[0], v0 + (v1 - v0) * uv[1]];
        }
    }
    Ok(Vertex {
        point,
        pc_kind,
        pc_index,
        pc_face: sel(v.pc_face, tg.n_f()),
        t,
        uv,
    })
}

// ---- C++ ---------------------------------------------------------------------------------

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn push_offsets(o: &mut Vec<f64>, offs: &[Option<OffIn>; 2]) -> Result<(), HarnessError> {
    for off in offs {
        match off {
            Some(off) => {
                o.push(1.0);
                push_surface(o, &off.base)?;
                o.push(off.dist);
            }
            None => o.push(0.0),
        }
    }
    Ok(())
}

fn push_opt_curve(o: &mut Vec<f64>, c: Option<&CrvIn>) -> Result<(), HarnessError> {
    if let Some(c) = c {
        o.push(1.0);
        push_curve(o, c)
    } else {
        o.push(0.0);
        Ok(())
    }
}

/// The input after the B-rep block (layout in the FFI wrapper).
#[allow(clippy::too_many_lines)]
fn model_input(i: &Inputs, tg: &Target) -> Result<Vec<f64>, HarnessError> {
    let m = &i.model;
    let mut o = tg.block.clone();

    o.push(m.solvers.len() as f64);
    for s in &m.solvers {
        o.extend([s.tol, s.ang, s.tang]);
        for &e in &s.eus {
            o.push(sel(e, tg.n_eu()) as f64);
        }
        push_offsets(&mut o, &s.surfs)?;
        o.extend([b(s.exec), b(s.reverse_trim)]);
    }

    o.push(m.corners.len() as f64);
    for c in &m.corners {
        o.extend([sel(c.vertex, tg.n_v()) as f64, c.ctype as f64, c.tol]);
    }

    o.push(m.vertices.len() as f64);
    for v in &m.vertices {
        let r = resolve_vertex(v, tg)?;
        o.extend(r.point);
        o.extend([
            v.vtype as f64,
            v.status as f64,
            v.corner as f64,
            v.pc_class as f64,
        ]);
        o.extend([r.pc_kind as f64, r.pc_index as f64, r.pc_face as f64]);
        o.extend([r.t, r.uv[0], r.uv[1]]);
    }

    o.push(m.geoms.len() as f64);
    for g in &m.geoms {
        o.extend([g.gtype as f64, g.status as f64, g.solver as f64]);
        match &g.surface {
            Some(s) => {
                o.push(1.0);
                push_surface(&mut o, s)?;
            }
            None => o.push(0.0),
        }
        push_opt_curve(&mut o, g.center.as_ref())?;
        o.extend(g.devs);
        push_offsets(&mut o, &g.offs)?;
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
        push_opt_curve(&mut o, e.curve.as_ref())?;
        match e.interval {
            Some([a, c]) => o.extend([1.0, a, c]),
            None => o.push(0.0),
        }
        o.push(sel(e.orig_edge, tg.n_e()) as f64);
        o.push(sel(e.orig_face, tg.n_f()) as f64);
        for eu in &e.eus {
            push_opt_curve(&mut o, eu.uv_curve.as_ref())?;
            o.push(eu.geom as f64);
            o.extend(eu.vu_uv);
            let t = &eu.tsect;
            o.push(t.ptype as f64);
            o.extend(t.uv[0]);
            o.extend(t.uv[1]);
            o.extend(t.crv);
            o.extend(t.deriv);
            o.push(t.cparam);
        }
    }

    for g in &m.geoms {
        o.extend([g.rails[0] as f64, g.rails[1] as f64]);
        o.push(g.other1.len() as f64);
        o.extend(g.other1.iter().map(|&x| x as f64));
        o.push(g.other2.len() as f64);
        o.extend(g.other2.iter().map(|&x| x as f64));
        o.push(g.sides.len() as f64);
        for &[e, w] in &g.sides {
            o.extend([e as f64, w as f64]);
        }
    }

    for v in &m.vertices {
        o.extend([v.mates[0] as f64, v.mates[1] as f64]);
    }

    match &i.op {
        OpIn::Export => {}
        OpIn::BlendingRail { e } => o.extend([*e as f64, 0.0]),
        OpIn::EdgeOnEdge { e, use_corner }
        | OpIn::FilletEnd { e, use_corner }
        | OpIn::SetBackRail { e, use_corner } => o.extend([*e as f64, b(*use_corner)]),
        OpIn::ShouldSwap { e, f1, f2 } => o.extend([
            *e as f64,
            sel(*f1, tg.n_f()) as f64,
            sel(*f2, tg.n_f()) as f64,
        ]),
        OpIn::Join {
            g,
            vs,
            ve,
            uv_tol,
            start,
        } => {
            o.extend([*g as f64, *vs as f64, *ve as f64, *uv_tol]);
            o.push(start.len() as f64);
            for s in start {
                o.extend([s.e as f64, s.which as f64, b(s.with_uv), s.orient as f64]);
            }
        }
        OpIn::Copy { g, other } => o.extend([*g as f64, *other as f64]),
        OpIn::FindTangent { g, rail, end } | OpIn::RailVertex { g, rail, end } => {
            o.extend([*g as f64, *rail as f64, *end as f64]);
        }
        OpIn::RailIndexSideEu { g, eu } => o.extend([*g as f64, sel(*eu, tg.n_eu()) as f64]),
        OpIn::RailIndexVu { g, e, which } => o.extend([*g as f64, *e as f64, *which as f64]),
        OpIn::SetUpRailPoint {
            g,
            rail,
            end,
            param,
            pc,
        } => {
            o.extend([
                *g as f64,
                *rail as f64,
                *end as f64,
                *param,
                b(pc.is_some()),
            ]);
            let (class, kind, index, t) = match pc {
                Some(p) => {
                    let (k, i) = tg.pc(p.kind, p.index);
                    (p.class, k, i, p.t)
                }
                None => (100, 0, -1, 0.0),
            };
            o.extend([class as f64, kind as f64, index as f64, t]);
        }
        OpIn::MoveTsp { pnts, from, to } => {
            o.push(pnts.len() as f64);
            for [c, d] in pnts {
                o.extend(c);
                o.extend(d);
            }
            o.extend([*from as f64, *to as f64]);
        }
        OpIn::RailEuIntersect { g, rail, eu, guess } => {
            o.extend([*g as f64, *rail as f64, sel(*eu, tg.n_eu()) as f64, *guess]);
        }
        OpIn::RecordError { g, etype } => o.extend([*g as f64, *etype as f64]),
    }
    Ok(o)
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Export => Op::Export,
        OpIn::BlendingRail { .. } => Op::BlendingRail,
        OpIn::EdgeOnEdge { .. } => Op::EdgeOnEdge,
        OpIn::FilletEnd { .. } => Op::FilletEnd,
        OpIn::SetBackRail { .. } => Op::SetBackRail,
        OpIn::ShouldSwap { .. } => Op::ShouldSwap,
        OpIn::Join { .. } => Op::Join,
        OpIn::Copy { .. } => Op::Copy,
        OpIn::FindTangent { .. } => Op::FindTangent,
        OpIn::RailIndexSideEu { .. } => Op::RailIndexSideEu,
        OpIn::RailIndexVu { .. } => Op::RailIndexVu,
        OpIn::RailVertex { .. } => Op::RailVertex,
        OpIn::SetUpRailPoint { .. } => Op::SetUpRailPoint,
        OpIn::MoveTsp { .. } => Op::MoveTsp,
        OpIn::RailEuIntersect { .. } => Op::RailEuIntersect,
        OpIn::RecordError { .. } => Op::RecordError,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let tg = match target(&i.brep)? {
        Ok(t) => t,
        Err(out) => return Ok(out),
    };
    let input = model_input(i, &tg)?;
    let result = ffi::eval(op_of(&i.op), &tg.path, &input);
    if i.expected_rejection {
        // Only a flagged case may be recorded as rejected.
        return match result {
            Err(solidcore_ffi::FfiError::InvalidArgument(reason)) => Ok(Output::rejected(reason)),
            Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                Ok(Output::rejected("shim_rejected_model"))
            }
            Err(e) => Err(harness(format!(
                "expected-rejection fixture failed in another way: {e}"
            ))),
            Ok(_) => Err(harness("expected-rejection fixture was not refused")),
        };
    }
    let flat = result.map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("flat", flat))
}

// ---- Rust --------------------------------------------------------------------------------

/// The Rust-side model.
struct RustModel {
    k: Kernel,
    solvers: Vec<FilletSolver>,
    corners: Vec<FilletCorner>,
    fb: FilletBrep,
    vertices: Vec<VertexId>,
    edges: Vec<EdgeId>,
    geoms: Vec<FilletGeomId>,
}

/// Two edge uses (primary `SM_OT_SAME` at `vs`, mate `SM_OT_OPPOSITE` at `ve`) on `e`, as
/// `MakeFilletEdge` links them.
fn link_uses(
    b: &mut Brep,
    e: EdgeId,
    vs: VertexId,
    ve: VertexId,
) -> Result<[EdgeuseId; 2], HarnessError> {
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

fn opt(x: i64, n: usize) -> Option<usize> {
    usize::try_from(x).ok().filter(|&i| i < n)
}

/// The derived corner class whose `GetCornerType()` is `SmFilletCornerType` `code`
/// (the abstract `SM_FCT_UNKNOWN` and `SM_FCT_NxN` have none).
fn corner_kind(code: i64) -> Result<FilletCornerKind, HarnessError> {
    Ok(match code {
        1 => FilletCornerKind::Open,
        2 => FilletCornerKind::Degenerate,
        3 => FilletCornerKind::OneByOne,
        4 => FilletCornerKind::TwoByTwo,
        5 => FilletCornerKind::ThreeByTwoMixed,
        6 => FilletCornerKind::FourByThree,
        7 => FilletCornerKind::NByOneClosed,
        8 => FilletCornerKind::NByOne,
        9 => FilletCornerKind::NByTwo,
        11 => FilletCornerKind::ConvexNByN,
        12 => FilletCornerKind::ConcaveNByN,
        _ => return Err(harness("bad corner type")),
    })
}

fn offset_of(k: &Kernel, off: &OffIn, id: u32) -> Result<OffsetSurface, HarnessError> {
    let base = surface_of(&off.base)?;
    OffsetSurface::new(k, off.dist, base, SurfaceId::Free(id), SS_UNKNOWN).map_err(h)
}

fn entity(kind: i64, idx: i64) -> Option<EntityRef> {
    let idx = u32::try_from(idx).ok()?;
    match kind {
        1 => Some(EntityRef::Vertex(VertexId::from_index(idx))),
        2 => Some(EntityRef::Edgeuse(EdgeuseId::from_index(idx))),
        3 => Some(EntityRef::Face(FaceId::from_index(idx))),
        4 => Some(EntityRef::Edge(EdgeId::from_index(idx))),
        _ => None,
    }
}

fn point_class(code: i64) -> Result<PointClassType, HarnessError> {
    i32::try_from(code)
        .ok()
        .and_then(PointClassType::from_code)
        .ok_or_else(|| harness("bad point class"))
}

impl RustModel {
    #[allow(clippy::too_many_lines)]
    fn build(i: &Inputs, tg: &Target) -> Result<Self, HarnessError> {
        let m = &i.model;
        let k = Kernel::default();

        let mut solvers = Vec::new();
        for (si, s) in m.solvers.iter().enumerate() {
            let mut fs = FilletSolver::new(s.tol, s.ang, s.tang);
            for (slot, &e) in fs.edgeuses.iter_mut().zip(&s.eus) {
                *slot = tg.edgeuse(e);
            }
            for (rail, off) in s.surfs.iter().enumerate() {
                if let Some(off) = off {
                    let surface = offset_of(&k, off, (si * 2 + rail) as u32)?;
                    fs.set_surface(
                        rail,
                        Some(OffsetSurfaceSlot {
                            key: (si * 2 + rail) as u64,
                            surface,
                        }),
                    )
                    .map_err(h)?;
                }
            }
            fs.executive = s.exec.then_some(FilletExecutiveRef);
            fs.reverse_trim = s.reverse_trim;
            solvers.push(fs);
        }

        let mut corners = Vec::new();
        for c in &m.corners {
            // the shim's corner: SmFilletCorner(ctx, v, NULL, NULL, exec, tol, 0.01)
            corners.push(FilletCorner::new(
                corner_kind(c.ctype)?,
                u32::try_from(sel(c.vertex, tg.n_v()))
                    .ok()
                    .map(VertexId::from_index),
                None,
                None,
                c.tol,
                0.01,
            ));
        }
        let corner = |c: i64| {
            opt(c, corners.len())
                .and_then(|x| u32::try_from(x).ok())
                .map(FilletCornerId)
        };

        let mut fb = FilletBrep::new();
        let mut vertices = Vec::new();
        for v in &m.vertices {
            let r = resolve_vertex(v, tg)?;
            let id = fb
                .brep_mut()
                .add_vertex(Point3::new(r.point[0], r.point[1], r.point[2]))
                .map_err(h)?;
            let mut fv = FilletVertex::new(&k, corner(v.corner));
            fv.set_fillet_vertex_type(
                FilletVertexType::from_code(v.vtype).ok_or_else(|| harness("bad vertex type"))?,
            );
            fv.set_status(FilStatus::from_code(v.status).ok_or_else(|| harness("bad status"))?);
            fv.set_point_class(point_class(v.pc_class)?, entity(r.pc_kind, r.pc_index));
            if let Some(f) = u32::try_from(r.pc_face).ok().map(FaceId::from_index) {
                fv.point_classification_mut()
                    .set_face(Some(EntityRef::Face(f)));
            }
            fv.set_original_t_param(r.t);
            fv.set_original_uv(Point2::new(r.uv[0], r.uv[1]));
            fb.insert_fillet_vertex(id, fv);
            vertices.push(id);
        }

        let mut geoms = Vec::new();
        for (gi, g) in m.geoms.iter().enumerate() {
            let gtype =
                FilletGeomType::from_code(g.gtype).ok_or_else(|| harness("bad geom type"))?;
            let mut geom = match opt(g.solver, solvers.len()) {
                Some(si) => {
                    let fs = solvers.get(si).ok_or_else(|| harness("solver"))?;
                    new_solver_fillet_geom(&k, FilletSolverId(si as u32), fs, gtype).map_err(h)?
                }
                None => FilletGeom::new(&k, None, gtype).map_err(h)?,
            };
            geom.status = FilStatus::from_code(g.status).ok_or_else(|| harness("bad status"))?;
            if let Some(s) = &g.surface {
                geom.set_fillet_surface(Some(surface_of(s)?));
            }
            if let Some(c) = &g.center {
                geom.center_line_curve = Some(Curve::from(bspline_of(c)?));
            }
            geom.deviations = g.devs;
            for (rail, off) in g.offs.iter().enumerate() {
                if let Some(off) = off {
                    let s = offset_of(&k, off, (1000 + gi * 2 + rail) as u32)?;
                    geom.set_offset_surface(rail, Some(s)).map_err(h)?;
                }
            }
            geoms.push(fb.add_geom(geom));
        }

        let mut edges = Vec::new();
        for e in &m.edges {
            let vs = *vertices.get(e.vs).ok_or_else(|| harness("edge vertex"))?;
            let ve = *vertices.get(e.ve).ok_or_else(|| harness("edge vertex"))?;
            let id = fb.new_fillet_edge(vs, ve, corner(e.corner)).map_err(h)?;
            let eus = link_uses(fb.brep_mut(), id, vs, ve)?;
            {
                let fe = fb.fillet_edge_mut(id).map_err(h)?;
                fe.set_fillet_edge_type(
                    FilletEdgeType::from_code(e.etype).ok_or_else(|| harness("bad edge type"))?,
                );
                fe.set_status(FilStatus::from_code(e.status).ok_or_else(|| harness("bad status"))?);
                fe.set_original_edge(tg.edge(e.orig_edge));
                fe.set_original_face(tg.face(e.orig_face));
            }
            {
                let edge = fb.brep_mut().edge_mut(id).map_err(h)?;
                if let Some(c) = &e.curve {
                    edge.curve = Some(Curve::from(bspline_of(c)?));
                }
                if let Some([a, c]) = e.interval {
                    edge.interval = Extent1d::from_min_max(a, c);
                }
            }
            for (eu, spec) in eus.iter().zip(&e.eus) {
                fb.insert_fillet_edgeuse(*eu, FilletEdgeuse::default());
                if let Some(c) = &spec.uv_curve {
                    fb.brep_mut().edgeuse_mut(*eu).map_err(h)?.uv_trim_curve =
                        Some(Curve::from(bspline_of(c)?));
                }
                if let Some(gi) = opt(spec.geom, geoms.len()) {
                    let g = *geoms.get(gi).ok_or_else(|| harness("edge use geom"))?;
                    fb.set_edgeuse_fillet_geom(*eu, Some(g)).map_err(h)?;
                }
                let vu = fb
                    .brep()
                    .edgeuse(*eu)
                    .map_err(h)?
                    .vertexuse
                    .ok_or_else(|| harness("edge use without vertex use"))?;
                let mut fvu = FilletVertexuse::new();
                fvu.set_uv_point(Point2::new(spec.vu_uv[0], spec.vu_uv[1]));
                let t = &spec.tsect;
                let ts = fvu.tsect_pnt_mut();
                ts.point_type = i32::try_from(t.ptype)
                    .ok()
                    .and_then(IntersectionPointType::from_code)
                    .ok_or_else(|| harness("bad point type"))?;
                *ts.uv_pos_mut(SrfIndex::First) = Point2::new(t.uv[0][0], t.uv[0][1]);
                *ts.uv_pos_mut(SrfIndex::Second) = Point2::new(t.uv[1][0], t.uv[1][1]);
                *ts.crv_pos_mut() = Point3::new(t.crv[0], t.crv[1], t.crv[2]);
                *ts.crv_deriv_mut() = Vec3::new(t.deriv[0], t.deriv[1], t.deriv[2]);
                ts.curve_parameter = t.cparam;
                fb.insert_fillet_vertexuse(vu, fvu);
            }
            edges.push(id);
        }

        for (g, spec) in geoms.iter().zip(&m.geoms) {
            let eid = |x: usize| edges.get(x).copied().ok_or_else(|| harness("geom edge"));
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
            for (r, &rail) in spec.rails.iter().enumerate() {
                geom.set_rail(r, opt(rail, edges.len()).map(eid).transpose()?);
            }
            for &x in &spec.other1 {
                geom.other_rails1.push(eid(x)?);
            }
            for &x in &spec.other2 {
                geom.other_rails2.push(eid(x)?);
            }
            for eu in sides {
                geom.add_side_fillet_edgeuse(eu);
            }
        }

        for (v, spec) in vertices.iter().zip(&m.vertices) {
            for (j, &mate) in spec.mates.iter().enumerate() {
                let mate = opt(mate, vertices.len()).and_then(|x| vertices.get(x).copied());
                fb.fillet_vertex_mut(*v).map_err(h)?.set_mate(j, mate);
            }
        }

        Ok(Self {
            k,
            solvers,
            corners,
            fb,
            vertices,
            edges,
            geoms,
        })
    }

    fn put_eu(&self, o: &mut Vec<f64>, eu: EdgeuseId) {
        for (i, &e) in self.edges.iter().enumerate() {
            if let Ok(edge) = self.fb.brep().edge(e) {
                if let Some(w) = edge.edgeuses.iter().position(|&x| x == eu) {
                    o.extend([i as f64, w as f64]);
                    return;
                }
            }
        }
        o.extend([-1.0, -1.0]);
    }

    fn edge_use(&self, e: usize, which: usize) -> Result<EdgeuseId, HarnessError> {
        let e = *self.edges.get(e).ok_or_else(|| harness("edge"))?;
        self.fb
            .brep()
            .edge(e)
            .map_err(h)?
            .edgeuses
            .get(which)
            .copied()
            .ok_or_else(|| harness("edge use"))
    }

    /// The export (mirrors the shim's).
    fn put(&self, o: &mut Vec<f64>) -> Result<(), HarnessError> {
        let b = self.fb.brep();
        o.push(self.vertices.len() as f64);
        for &v in &self.vertices {
            let fv = self.fb.fillet_vertex(v).map_err(h)?;
            let p = self.fb.vertex_point(v).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(fv.fillet_vertex_type() as i64 as f64);
            o.push(fv.status() as i64 as f64);
            let uv = fv.original_uv();
            o.extend([uv.x, uv.y]);
            o.push(f64::from(fv.point_class() as i32));
            o.push(fv.original_t_param());
        }
        o.push(self.edges.len() as f64);
        for &e in &self.edges {
            let fe = self.fb.fillet_edge(e).map_err(h)?;
            let edge = b.edge(e).map_err(h)?;
            o.push(fe.fillet_edge_type() as i64 as f64);
            o.push(fe.status() as i64 as f64);
            dump_curve(o, edge.curve.as_ref());
            o.extend([edge.interval.min(), edge.interval.max()]);
            o.push(fe.original_edge().map_or(-1.0, |x| f64::from(x.index())));
            o.push(fe.original_face().map_or(-1.0, |x| f64::from(x.index())));
            for &eu in edge.edgeuses.iter().take(2) {
                let r = b.edgeuse(eu).map_err(h)?;
                dump_curve(o, r.uv_trim_curve.as_ref());
                let g = self.fb.fillet_edgeuse(eu).map_err(h)?.fillet_geom();
                o.push(
                    g.and_then(|g| self.geoms.iter().position(|&x| x == g))
                        .map_or(-1.0, |i| i as f64),
                );
                let vu = r
                    .vertexuse
                    .ok_or_else(|| harness("edge use without vertex use"))?;
                let uv = self.fb.fillet_vertexuse(vu).map_err(h)?.uv_point();
                o.extend([uv.x, uv.y]);
            }
        }
        o.push(self.geoms.len() as f64);
        for &g in &self.geoms {
            let geom = self.fb.geom(g).map_err(h)?;
            o.push(geom.status() as i64 as f64);
            o.push(geom.side_eus.len() as f64);
            for &eu in &geom.side_eus {
                self.put_eu(o, eu);
            }
            o.push(geom.ref_eus.len() as f64);
            for &eu in &geom.ref_eus {
                self.put_eu(o, eu);
            }
            dump_surface(o, geom.fillet_surface(None).map_err(h)?)?;
            dump_bcurve(o, geom.center_line_curve());
            o.extend(geom.deviations);
            for off in &geom.off_surfs {
                match off {
                    Some(off) => {
                        o.push(1.0);
                        o.push(off.offset_distance());
                        dump_surface(o, Some(off.base_surface()))?;
                    }
                    None => o.push(0.0),
                }
            }
        }
        Ok(())
    }
}

fn dump_bcurve(o: &mut Vec<f64>, bs: Option<&BSplineCurve>) {
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

/// The shim's `put_bcurve`: the NURB of every curve that `IsKindOf(SmBSplineCurve_TYPE)`
/// (lines and conics are B-spline subclasses upstream).
fn dump_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    let bs = match c.map(Curve::kind) {
        Some(CurveKind::BSpline(bs)) => Some(bs),
        Some(CurveKind::Line(c)) => Some(c.base()),
        Some(CurveKind::Ellipse(c)) => Some(c.base()),
        Some(CurveKind::Hyperbola(c)) => Some(c.base()),
        Some(CurveKind::Parabola(c)) => Some(c.base()),
        _ => None,
    };
    dump_bcurve(o, bs);
}

/// The shim's `put_bsurface`: the NURB of a B-spline surface or a surface of revolution
/// (not a plane's).
fn dump_surface(o: &mut Vec<f64>, s: Option<&Surface>) -> Result<(), HarnessError> {
    let bs = match s.map(Surface::kind) {
        Some(SurfaceKind::BSpline(bs)) => Some(bs),
        Some(SurfaceKind::SurfOfRevolution(s)) => Some(s.base()),
        _ => None,
    };
    let Some(sur) = bs.and_then(BSplineSurface::gw_nurb) else {
        o.push(0.0);
        return Ok(());
    };
    o.push(1.0);
    let (n, m) = (sur.n(), sur.m());
    o.extend([n, m, sur.p(), sur.q(), sur.r(), sur.s()].map(|x| x as f64));
    for i in 0..=n {
        for j in 0..=m {
            let p = sur.cpt(i, j).map_err(h)?;
            o.extend([p.x, p.y, p.z, p.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
    Ok(())
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
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

/// The deferral of an op that evaluates a target face surface or edge curve the import
/// holds only as its B-spline part: the original face of a blending rail (and the end
/// points dropped onto it), the original edge of an edge on an edge, the side edges of a
/// fillet end, the faces a point is classified between, and the classification edges of
/// a rail end's tangent.
fn inexact(i: &Inputs, tg: &Target) -> Result<Option<Output>, HarnessError> {
    let m = &i.model;
    let edge_in = |e: usize| m.edges.get(e).ok_or_else(|| harness("edge"));
    let pc_edge_of = |v: &VertexIn| -> Result<Option<EdgeId>, HarnessError> {
        let (kind, idx) = tg.pc(v.pc_kind, v.pc_index);
        tg.pc_edge(kind, idx)
    };
    let exact = match &i.op {
        OpIn::BlendingRail { e } => tg.exact_face(tg.face(edge_in(*e)?.orig_face)),
        OpIn::EdgeOnEdge { e, .. } => tg.exact_edge(tg.edge(edge_in(*e)?.orig_edge)),
        OpIn::FilletEnd { e, .. } => {
            let e = edge_in(*e)?;
            let mut ok = true;
            for v in [e.vs, e.ve] {
                let v = m.vertices.get(v).ok_or_else(|| harness("vertex"))?;
                ok &= tg.exact_edge(pc_edge_of(v)?);
            }
            ok
        }
        OpIn::ShouldSwap { f1, f2, .. } => {
            tg.exact_face(tg.face(*f1)) && tg.exact_face(tg.face(*f2))
        }
        OpIn::FindTangent { .. } => {
            let mut ok = true;
            for v in &m.vertices {
                ok &= tg.exact_edge(pc_edge_of(v)?);
            }
            ok
        }
        OpIn::RailEuIntersect { eu, .. } => {
            if let Some(eu) = tg.edgeuse(*eu) {
                let e = teu::edge(&tg.brep, eu).map_err(h)?;
                let f = teu::face(&tg.brep, eu).map_err(h)?;
                tg.exact_edge(e) && tg.exact_face(f)
            } else {
                true
            }
        }
        _ => true,
    };
    Ok((!exact).then(|| {
        Output::deferred(
            HARNESS_ANALYTIC_IMPORT,
            "evaluates analytic target geometry exported as its B-spline part",
        )
    }))
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut tg = match own_target(&i.brep)? {
        Ok(t) => t,
        Err(out) => return Ok(out),
    };
    if let Some(out) = inexact(i, &tg)? {
        return Ok(out);
    }
    let mut m = RustModel::build(i, &tg)?;
    let k = std::mem::take(&mut m.k);
    let edge = |m: &RustModel, e: usize| m.edges.get(e).copied().ok_or_else(|| harness("edge"));
    let geom = |m: &RustModel, g: usize| m.geoms.get(g).copied().ok_or_else(|| harness("geometry"));
    let corner_view = |m: &RustModel,
                       e: EdgeId,
                       use_corner: bool|
     -> Result<Option<FilletCorner>, HarnessError> {
        if !use_corner {
            return Ok(None);
        }
        let c = m.fb.fillet_edge(e).map_err(h)?.fillet_corner();
        Ok(c.and_then(|c| m.corners.get(c.0 as usize).cloned()))
    };
    let mut results: Vec<f64> = Vec::new();
    let res: solidcore::Result<()> = match &i.op {
        OpIn::Export => Ok(()),
        OpIn::BlendingRail { e } => {
            let e = edge(&m, *e)?;
            m.fb.calc_blending_rail(&k, e, &mut tg.brep)
        }
        OpIn::EdgeOnEdge { e, .. } => {
            let e = edge(&m, *e)?;
            m.fb.calc_edge_on_edge(&k, e, &tg.brep)
        }
        OpIn::FilletEnd { e, use_corner } => {
            let e = edge(&m, *e)?;
            let c = corner_view(&m, e, *use_corner)?;
            m.fb.calc_fillet_end(&k, e, &tg.brep, c.as_ref(), &m.solvers)
        }
        OpIn::SetBackRail { e, use_corner } => {
            let e = edge(&m, *e)?;
            let c = corner_view(&m, e, *use_corner)?;
            m.fb.calc_set_back_rail(&k, e, &tg.brep, c.as_ref(), &m.solvers)
        }
        OpIn::ShouldSwap { e, f1, f2 } => {
            let e = edge(&m, *e)?;
            let f1 = tg.face(*f1).ok_or_else(|| harness("face"))?;
            let f2 = tg.face(*f2).ok_or_else(|| harness("face"))?;
            m.fb.should_swap_faces(&k, e, f1, f2, &mut tg.brep, &m.corners)
                .map(|swap| results.push(b(swap)))
        }
        OpIn::Join {
            g,
            vs,
            ve,
            uv_tol,
            start,
        } => {
            let g = geom(&m, *g)?;
            let v = |x: i64| opt(x, m.vertices.len()).and_then(|x| m.vertices.get(x).copied());
            let mut c3 = Vec::new();
            let mut cuv = Vec::new();
            let mut orients = Vec::new();
            for s in start {
                let e = edge(&m, s.e)?;
                c3.push(m.fb.brep().edge(e).map_err(h)?.curve.clone());
                let eu = m.edge_use(s.e, s.which)?;
                cuv.push(if s.with_uv {
                    m.fb.brep().edgeuse(eu).map_err(h)?.uv_trim_curve.clone()
                } else {
                    None
                });
                orients.push(orient_of(s.orient)?);
            }
            let sides = m.fb.geom(g).map_err(h)?.side_eus.clone();
            join_two_fillet_verts_by_curves(
                &k,
                &m.fb,
                g,
                v(*vs),
                v(*ve),
                &sides,
                *uv_tol,
                &mut c3,
                &mut cuv,
                &mut orients,
                &m.solvers,
            )
            .map(|made| {
                results.push(b(made));
                results.push(c3.len() as f64);
                for ((c, u), o) in c3.iter().zip(&cuv).zip(&orients) {
                    dump_curve(&mut results, c.as_ref());
                    dump_curve(&mut results, u.as_ref());
                    results.push(orient_code(*o));
                }
            })
        }
        OpIn::Copy { g, other } => {
            let g = geom(&m, *g)?;
            let other = geom(&m, *other)?;
            m.fb.copy_geom(&k, g, other, &m.solvers)
        }
        OpIn::FindTangent { g, rail, end } => {
            let g = geom(&m, *g)?;
            m.fb.find_edge_tangent_at_point(&k, g, *rail, *end, &tg.brep)
                .map(|t| {
                    results.push(b(t.is_some()));
                    let t = t.unwrap_or(Vec3::new(0.0, 0.0, 0.0));
                    results.extend([t.x, t.y, t.z]);
                })
        }
        OpIn::RailIndexSideEu { g, eu } => {
            let g = geom(&m, *g)?;
            let eu = tg.edgeuse(*eu).ok_or_else(|| harness("edge use"))?;
            let r = m.fb.rail_index_of_side_eu(g, eu, &tg.brep, &m.solvers);
            results.push(r.as_ref().map_or(99.0, |&x| x as f64));
            r.map(|_| ())
        }
        OpIn::RailIndexVu { g, e, which } => {
            let g = geom(&m, *g)?;
            let eu = m.edge_use(*e, *which)?;
            let vu =
                m.fb.brep()
                    .edgeuse(eu)
                    .map_err(h)?
                    .vertexuse
                    .ok_or_else(|| harness("edge use without vertex use"))?;
            let r = m.fb.rail_index_of_vertexuse(g, Some(vu));
            results.push(r.as_ref().map_or(99.0, |&x| x as f64));
            r.map(|_| ())
        }
        OpIn::RailVertex { g, rail, end } => {
            let g = geom(&m, *g)?;
            m.fb.rail_vertex(g, *rail, *end).map(|v| {
                results.push(
                    m.vertices
                        .iter()
                        .position(|&x| x == v)
                        .map_or(-1.0, |x| x as f64),
                );
            })
        }
        OpIn::SetUpRailPoint {
            g,
            rail,
            end,
            param,
            pc,
        } => {
            let g = geom(&m, *g)?;
            let pc = match pc {
                Some(p) => {
                    let (kind, idx) = tg.pc(p.kind, p.index);
                    let mut c = PointClassification::new(zone_tol3d(&k, USE_DEFAULT));
                    c.set_class_object(point_class(p.class)?, entity(kind, idx));
                    c.set_t_param(p.t, false);
                    Some(c)
                }
                None => None,
            };
            m.fb.set_up_rail_point(&k, g, *rail, *end, *param, pc.as_ref())
        }
        OpIn::MoveTsp { pnts, from, to } => {
            let tsects: Vec<TsectPnt> = pnts
                .iter()
                .map(|[c, d]| {
                    let mut t = TsectPnt::new();
                    *t.crv_pos_mut() = Point3::new(c[0], c[1], c[2]);
                    *t.crv_deriv_mut() = Vec3::new(d[0], d[1], d[2]);
                    t
                })
                .collect();
            let mut order: Vec<usize> = (0..tsects.len()).collect();
            let r = move_tsp(&mut order, *from, *to, |&x| {
                tsects
                    .get(x)
                    .cloned()
                    .ok_or_else(|| SolidError::invalid_input("difftest", "tsect index"))
            });
            results.push(order.len() as f64);
            results.extend(order.iter().map(|&x| x as f64));
            r
        }
        OpIn::RailEuIntersect { g, rail, eu, guess } => {
            let g = geom(&m, *g)?;
            let eu = tg.edgeuse(*eu).ok_or_else(|| harness("edge use"))?;
            // the geometry's solver; without one the port stops at NER(pFS) before
            // touching the solver it is given
            let mut solver = match m.fb.geom(g).map_err(h)?.fillet_solver() {
                Some(si) => m
                    .solvers
                    .get(si.0 as usize)
                    .cloned()
                    .ok_or_else(|| harness("solver"))?,
                None => FilletSolver::new(1e-6, 1e-3, 1e-6),
            };
            // the shim's SmTsectPnt t
            let mut tsect = TsectPnt::default();
            let r = m.fb.rail_edgeuse_intersect_with_geometry(
                &k,
                g,
                &mut solver,
                &mut tg.brep,
                *rail,
                eu,
                *guess,
                &mut tsect,
            );
            // on a failed status, or without an intersection, the out-params keep the
            // shim's initial values
            let (found, param) = match &r {
                Ok(Some(p)) => (true, *p),
                _ => (false, 0.0),
            };
            results.extend([b(found), param]);
            r.map(|_| ())
        }
        OpIn::RecordError { g, etype } => {
            let g = geom(&m, *g)?;
            let code = *etype;
            let etype = match code {
                0 => FilletErrorType::Ok,
                1 => FilletErrorType::ExecUninitialized,
                2 => FilletErrorType::InvalidInput,
                3 => FilletErrorType::VertexProblem,
                4 => FilletErrorType::EdgeProblem,
                5 => FilletErrorType::BadInt,
                6 => FilletErrorType::BadTrim,
                _ => FilletErrorType::BadMerge,
            };
            let filleted_edge = (tg.n_e() > 0 && (code & 1) != 0).then(|| EdgeId::from_index(0));
            let target_face = (tg.n_f() > 0 && (code & 2) != 0).then(|| FaceId::from_index(0));
            let rail = if (code & 4) != 0 {
                m.fb.geom(g).map_err(h)?.rail(0)
            } else {
                None
            };
            let fillet_surface = m
                .fb
                .geom(g)
                .map_err(h)?
                .fillet_surface(None)
                .map_err(h)?
                .map(|_| GeomErrorObject::FilletSurface(g));
            let comment = ((code & 1) != 0).then_some("SmFilletGeom::RecordFilletError");
            let mut info = ErrorInfo::default();
            let r = m.fb.record_fillet_error_in(
                g,
                etype,
                filleted_edge,
                target_face,
                rail,
                fillet_surface,
                comment,
                &m.solvers,
                Some(&mut info),
            );
            if r.is_ok() {
                results.push(f64::from(info.code));
                results.push(f64::from(info.status));
                results.push(f64::from(info.edge_type));
                results.push(match info.edge {
                    Some(GeomErrorObject::FilletedEdge(e)) => f64::from(e.index()),
                    _ => -1.0,
                });
                results.push(match info.side_face {
                    Some(GeomErrorObject::SideFace(f)) => f64::from(f.index()),
                    _ => -1.0,
                });
                results.push(info.edges.len() as f64);
                for item in &info.edges {
                    let idx = match item {
                        Some(GeomErrorObject::FilletEdge(e)) => m
                            .edges
                            .iter()
                            .position(|&x| x == *e)
                            .map_or(-1.0, |i| i as f64),
                        _ => -1.0,
                    };
                    results.push(idx);
                }
                results.push(info.surfaces.len() as f64);
                for item in &info.surfaces {
                    results.push(b(matches!(item, Some(GeomErrorObject::FilletSurface(_)))));
                }
                results.push(info.comment.len() as f64);
            }
            r
        }
    };
    if let Err(e) = &res {
        if let Some(out) = Output::unported(e) {
            return Ok(out);
        }
    }
    let mut o = vec![0.0, res.as_ref().err().map_or(0.0, status)];
    o.extend(results);
    m.put(&mut o)?;
    Ok(Output::new().values("flat", o))
}

// ---- generators ----------------------------------------------------------------------------

/// Upstream test files with faces whose edge uses lie on several faces.
const SOLID_FILES: [&str; 6] = [
    "Mouse.smb",
    "Endcap.smb",
    "box_with_large_fillet.smb",
    "cone.smb",
    "torus.smb",
    "vase_rev.smb",
];

fn solid_brep(rng: &mut Rng) -> BrepIn {
    BrepIn::File {
        file: (*rng.choose(&SOLID_FILES).unwrap_or(&"Mouse.smb")).to_string(),
        rebuild: false,
    }
}

fn scale_of(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0)
}

/// A B-spline surface net over `[0, scale]^2` at height `z`.
fn gen_bspline_surface(rng: &mut Rng, scale: f64, z: f64) -> SrfIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 * scale / n as f64,
                        j as f64 * scale / m as f64,
                        (z + rng.uniform(-0.2, 0.2)) * scale,
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

/// The `v = v_min` (`at_max`: `v_max`) boundary curve of a B-spline surface block: its
/// first (last) column of control points over the `u` knots.
fn iso_v_boundary(s: &SrfIn, at_max: bool) -> Option<CrvIn> {
    let SrfIn::Bspline(bs) = s else {
        return None;
    };
    let cpts = bs
        .net
        .iter()
        .map(|row| if at_max { row.last() } else { row.first() }.copied())
        .collect::<Option<Vec<_>>>()?;
    Some(CrvIn {
        dim: 3,
        cpts,
        p: bs.p,
        u: bs.u.clone(),
    })
}

/// A curve of dimension `dim` on `[a, b]` through roughly `[0, scale]` in x.
fn gen_curve_on(rng: &mut Rng, dim: u64, scale: f64, a: f64, b: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let cpts = (0..=n)
        .map(|i| {
            let x = i as f64 / n as f64 * scale;
            let y = rng.uniform(-0.3, 0.3) * scale;
            let z = if dim == 2 {
                NOZ
            } else {
                rng.uniform(-0.3, 0.3) * scale
            };
            [x, y, z, NOW]
        })
        .collect();
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

/// A UV line from `a` to `b` on `[t0, t1]`.
fn uv_line(a: [f64; 2], b: [f64; 2], t0: f64, t1: f64) -> CrvIn {
    CrvIn {
        dim: 2,
        cpts: vec![[a[0], a[1], NOZ, NOW], [b[0], b[1], NOZ, NOW]],
        p: 1,
        u: vec![t0, t0, t1, t1],
    }
}

fn point_on(c: &CrvIn, t: f64) -> [f64; 3] {
    bspline_of(c)
        .ok()
        .and_then(|bc| bc.evaluate_point(t).ok())
        .map_or([0.0; 3], |p| [p.x, p.y, p.z])
}

fn vertex(vtype: FilletVertexType, status: FilStatus) -> VertexIn {
    VertexIn {
        point: [0.0; 3],
        at: None,
        noise: [0.0; 3],
        vtype: vtype as i64,
        status: status as i64,
        corner: -1,
        pc_class: 100,
        pc_kind: 0,
        pc_index: 0,
        pc_face: -1,
        t: 0.0,
        t_frac: None,
        uv: [0.0; 2],
        uv_face: -1,
        mates: [-1, -1],
    }
}

fn edge(vs: usize, ve: usize, etype: FilletEdgeType, eus: [EuIn; 2]) -> EdgeIn {
    EdgeIn {
        vs,
        ve,
        etype: etype as i64,
        status: 0,
        corner: -1,
        curve: None,
        interval: None,
        orig_edge: -1,
        orig_face: -1,
        eus,
    }
}

fn noise(rng: &mut Rng, size: f64) -> [f64; 3] {
    if rng.chance(0.5) {
        [0.0; 3]
    } else {
        rng.point3(-1.0, 1.0).map(|c| c * size)
    }
}

fn solver(rng: &mut Rng, scale: f64, surfaces: bool) -> SolverIn {
    let off = |rng: &mut Rng| {
        let z = rng.uniform(-0.5, 0.5);
        OffIn {
            base: gen_bspline_surface(rng, scale, z),
            dist: rng.uniform(-0.2, 0.2) * scale,
        }
    };
    SolverIn {
        tol: *rng.choose(&[1e-7, 1e-6, 1e-4, 1e-2]).unwrap_or(&1e-6) * scale.max(1e-3),
        ang: rng.uniform(0.001, 0.1),
        tang: 0.01,
        eus: [rng.below(1000) as i64, rng.below(1000) as i64],
        surfs: if surfaces {
            [Some(off(rng)), Some(off(rng))]
        } else {
            [None, None]
        },
        exec: false,
        reverse_trim: false,
    }
}

/// `CalcEdgeOnEdge`: a fillet edge on a target edge, its end points near that edge.
fn gen_edge_on_edge(rng: &mut Rng) -> Inputs {
    let target_edge = rng.below(1000) as i64;
    let mut vs = vertex(FilletVertexType::RailXEdgeuse, FilStatus::Processed);
    let mut ve = vertex(FilletVertexType::RailXEdgeuse, FilStatus::Processed);
    let (mut fa, mut fb) = (rng.uniform(0.0, 0.45), rng.uniform(0.55, 1.0));
    if rng.chance(0.3) {
        std::mem::swap(&mut fa, &mut fb); // descending: the curve is reversed
    }
    vs.at = Some(PointOn::Edge {
        edge: target_edge,
        frac: fa,
    });
    ve.at = Some(PointOn::Edge {
        edge: target_edge,
        frac: fb,
    });
    let size = *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3, 1.0]).unwrap_or(&0.0);
    vs.noise = noise(rng, size);
    ve.noise = noise(rng, size);
    let mut e = edge(
        0,
        1,
        FilletEdgeType::OnEdge,
        [EuIn::default(), EuIn::default()],
    );
    e.orig_edge = target_edge;
    e.corner = if rng.chance(0.5) { 0 } else { -1 };
    // a stale UV trim curve the new curve clears
    if rng.chance(0.3) {
        e.eus[0].uv_curve = Some(gen_curve_on(rng, 2, 1.0, 0.0, 1.0));
    }
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            corners: vec![CornerIn {
                vertex: rng.below(1000) as i64,
                ctype: 4,
                tol: 1e-3,
            }],
            vertices: vec![vs, ve],
            edges: vec![e],
            ..ModelIn::default()
        },
        op: OpIn::EdgeOnEdge {
            e: 0,
            use_corner: rng.chance(0.5),
        },
        expected_rejection: false,
    }
}

/// `CalcFilletEnd`: the edge across a fillet surface's end between two vertices on side
/// edges of the corner.
fn gen_fillet_end(rng: &mut Rng) -> Inputs {
    let scale = 1.0;
    let z = rng.uniform(-0.3, 0.3);
    let surface = gen_bspline_surface(rng, scale, z);
    let s = solver(rng, scale, false);
    let side = |rng: &mut Rng| {
        let mut v = vertex(FilletVertexType::RailXEdgeuse, FilStatus::Processed);
        if rng.chance(0.93) {
            v.pc_class = PointClassType::Edgeuse as i64;
            v.pc_kind = 2;
            v.pc_index = rng.below(1000) as i64;
            v.t_frac = Some(rng.uniform(0.0, 1.0));
            v.at = Some(PointOn::Pc);
        } else {
            v.point = rng.point3(0.0, 1.0);
        }
        if rng.chance(0.05) {
            v.status = 0; // SM_ERR
        }
        v
    };
    let vertices = vec![side(rng), side(rng)];
    let mut e = edge(
        0,
        1,
        FilletEdgeType::FilletEnd,
        [eu(0, None), EuIn::default()],
    );
    e.corner = 0;
    if rng.chance(0.05) {
        e.eus[0].geom = -1; // NER
    }
    let geom = GeomIn {
        solver: 0,
        surface: rng.chance(0.95).then_some(surface),
        ..GeomIn::default()
    };
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            solvers: vec![s],
            corners: vec![CornerIn {
                vertex: rng.below(1000) as i64,
                ctype: 4,
                tol: 1e-3,
            }],
            vertices,
            geoms: vec![geom],
            edges: vec![e],
        },
        op: OpIn::FilletEnd {
            e: 0,
            use_corner: rng.chance(0.95),
        },
        expected_rejection: false,
    }
}

/// `CalcSetBackRail`: a set-back rail along one of a geometry's rails.
fn gen_set_back_rail(rng: &mut Rng) -> Inputs {
    let scale = scale_of(rng);
    let surface = gen_bspline_surface(rng, scale, 0.0);
    let s = solver(rng, scale, false);
    // the rails: B-spline curves, along the fillet surface's iso boundary (when they have no
    // UV curve) or arbitrary
    let at_max = rng.chance(0.5);
    let rail_curve = |rng: &mut Rng| {
        if rng.chance(0.8) {
            iso_v_boundary(&surface, at_max)
                .unwrap_or_else(|| gen_curve_on(rng, 3, scale, 0.0, 1.0))
        } else {
            gen_curve_on(rng, 3, scale, 0.0, 1.0)
        }
    };
    let c0 = rail_curve(rng);
    let c1 = rail_curve(rng);
    let rail_uv = |rng: &mut Rng| rng.chance(0.4).then(|| gen_curve_on(rng, 2, 1.0, 0.0, 1.0));
    let mut vertices = vec![
        vertex(FilletVertexType::RailEnd, FilStatus::Processed),
        vertex(FilletVertexType::RailEnd, FilStatus::Processed),
        vertex(FilletVertexType::RailEnd, FilStatus::Processed),
        vertex(FilletVertexType::RailEnd, FilStatus::Processed),
    ];
    // the set-back edge's end points on rail 0 (or rail 1)
    let on_rail = rng.chance(0.5);
    let rc = if on_rail { &c1 } else { &c0 };
    let (mut ta, mut tb) = (rng.uniform(0.0, 0.45), rng.uniform(0.55, 1.0));
    if rng.chance(0.3) {
        std::mem::swap(&mut ta, &mut tb);
    }
    let size = *rng.choose(&[0.0, 1e-9, 1e-6, 1e-3]).unwrap_or(&0.0) * scale;
    let mut va = vertex(
        *rng.choose(&[
            FilletVertexType::RailXEdgeuse,
            FilletVertexType::FilletXFillet,
            FilletVertexType::Setback,
        ])
        .unwrap_or(&FilletVertexType::RailXEdgeuse),
        FilStatus::Processed,
    );
    va.point = point_on(rc, ta);
    va.noise = noise(rng, size);
    let mut vb = vertex(
        *rng.choose(&[FilletVertexType::RailXEdgeuse, FilletVertexType::Setback])
            .unwrap_or(&FilletVertexType::RailXEdgeuse),
        FilStatus::Processed,
    );
    vb.point = point_on(rc, tb);
    vb.noise = noise(rng, size);
    if rng.chance(0.05) {
        vb.status = 0; // SM_ERR
    }
    vertices.push(va);
    vertices.push(vb);
    let mut r0 = edge(
        0,
        1,
        FilletEdgeType::Rail,
        [eu(0, rail_uv(rng)), eu(0, rail_uv(rng))],
    );
    r0.curve = Some(c0);
    let mut r1 = edge(
        2,
        3,
        FilletEdgeType::Rail,
        [eu(0, rail_uv(rng)), eu(0, rail_uv(rng))],
    );
    r1.curve = Some(c1);
    let face = rng.below(1000) as i64;
    r0.orig_face = face;
    let etype = if rng.chance(0.5) {
        FilletEdgeType::SetbackRail
    } else {
        FilletEdgeType::RailExtension
    };
    let mut sb = edge(4, 5, etype, [eu(0, None), EuIn::default()]);
    sb.orig_face = if rng.chance(0.95) { face } else { -1 };
    sb.corner = 0;
    let n_by_2 = rng.chance(0.4);
    let geom = GeomIn {
        solver: 0,
        surface: Some(surface),
        rails: [0, 1],
        ..GeomIn::default()
    };
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            solvers: vec![s],
            corners: vec![CornerIn {
                vertex: -1,
                ctype: if n_by_2 { 9 } else { 4 },
                // a large tolerance squeezes the edge
                tol: if rng.chance(0.5) { 10.0 * scale } else { 1e-6 },
            }],
            vertices,
            geoms: vec![geom],
            edges: vec![r0, r1, sb],
        },
        op: OpIn::SetBackRail {
            e: 2,
            use_corner: rng.chance(0.9),
        },
        expected_rejection: false,
    }
}

/// `CalcBlendingRail`: a blending rail between the ends of two rails on a target face.
fn gen_blending_rail(rng: &mut Rng) -> Inputs {
    let face = rng.below(1000) as i64;
    let mate_end = |rng: &mut Rng| {
        let mut v = vertex(
            if rng.chance(0.75) {
                FilletVertexType::Mate
            } else {
                FilletVertexType::RailXEdgeuse
            },
            FilStatus::Processed,
        );
        v.uv = [rng.uniform(0.05, 0.95), rng.uniform(0.05, 0.95)];
        v.uv_face = face;
        v.at = Some(PointOn::Face { face, uv: v.uv });
        v.noise = noise(rng, 1e-7);
        v
    };
    let mut vertices = vec![mate_end(rng), mate_end(rng)];
    // the rails' other ends
    vertices.push(vertex(FilletVertexType::RailEnd, FilStatus::Processed));
    vertices.push(vertex(FilletVertexType::RailEnd, FilStatus::Processed));
    let rail_curve = |rng: &mut Rng| gen_curve_on(rng, 3, 1.0, 0.0, 1.0);
    let rail_uv = |rng: &mut Rng| rng.chance(0.7).then(|| gen_curve_on(rng, 2, 1.0, 0.0, 1.0));
    // the rails run into (or out of) the blending rail's ends
    let mut r0 = if rng.chance(0.5) {
        edge(
            0,
            2,
            FilletEdgeType::Rail,
            [EuIn::default(), eu(-1, rail_uv(rng))],
        )
    } else {
        edge(
            2,
            0,
            FilletEdgeType::Rail,
            [EuIn::default(), eu(-1, rail_uv(rng))],
        )
    };
    r0.curve = Some(rail_curve(rng));
    let mut r1 = if rng.chance(0.5) {
        edge(
            1,
            3,
            FilletEdgeType::Rail,
            [EuIn::default(), eu(-1, rail_uv(rng))],
        )
    } else {
        edge(
            3,
            1,
            FilletEdgeType::Rail,
            [EuIn::default(), eu(-1, rail_uv(rng))],
        )
    };
    r1.curve = Some(rail_curve(rng));
    let mut bl = edge(
        0,
        1,
        FilletEdgeType::BlendingRail,
        [EuIn::default(), EuIn::default()],
    );
    bl.orig_face = if rng.chance(0.97) { face } else { -1 };
    let brep = if rng.chance(0.2) {
        // a bilinear face: the 3D branch (DropCurve)
        BrepIn::Face {
            surface: SrfIn::Bspline(BssIn {
                oob: false,
                form: 0,
                net: vec![
                    vec![[0.0, 0.0, 0.0, NOW], [0.0, 1.0, 0.1, NOW]],
                    vec![[1.0, 0.0, 0.2, NOW], [1.0, 1.0, 0.0, NOW]],
                ],
                p: 1,
                q: 1,
                u: vec![0.0, 0.0, 1.0, 1.0],
                v: vec![0.0, 0.0, 1.0, 1.0],
            }),
            dom: [0.0, 0.0, 1.0, 1.0],
        }
    } else {
        solid_brep(rng)
    };
    Inputs {
        brep,
        model: ModelIn {
            vertices,
            edges: vec![r0, r1, bl],
            ..ModelIn::default()
        },
        op: OpIn::BlendingRail { e: 2 },
        expected_rejection: false,
    }
}

/// `ShouldSwapFaces`: which of two target faces a fillet edge lies on.
fn gen_should_swap(rng: &mut Rng) -> Inputs {
    let face = rng.below(1000) as i64;
    let mut vs = vertex(FilletVertexType::RailEnd, FilStatus::Processed);
    vs.at = Some(PointOn::Face {
        face,
        uv: [rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)],
    });
    let mut ve = vs.clone();
    ve.at = Some(PointOn::Face {
        face,
        uv: [rng.uniform(0.1, 0.9), rng.uniform(0.1, 0.9)],
    });
    let mut e = edge(
        0,
        1,
        FilletEdgeType::Rail,
        [EuIn::default(), EuIn::default()],
    );
    match rng.below(3) {
        0 => {
            // a curve over an interval: its middle, then its ends
            let c = gen_curve_on(rng, 3, 1.0, 0.0, 1.0);
            e.curve = Some(c);
            e.interval = Some([rng.uniform(0.0, 0.3), rng.uniform(0.6, 1.0)]);
        }
        1 => {
            // a corner's vertex
            e.corner = 0;
        }
        _ => {} // nothing to evaluate: FALSE
    }
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            corners: vec![CornerIn {
                vertex: rng.below(1000) as i64,
                ctype: 4,
                tol: 1e-3,
            }],
            vertices: vec![vs, ve],
            edges: vec![e],
            ..ModelIn::default()
        },
        op: OpIn::ShouldSwap {
            e: 0,
            f1: face,
            f2: if rng.chance(0.8) {
                rng.below(1000) as i64
            } else {
                face
            },
        },
        expected_rejection: false,
    }
}

/// `JoinTwoFilletVertsByCurves`: a chain of side edges between two fillet vertices.
fn gen_join(rng: &mut Rng) -> Inputs {
    let scale = 1.0;
    let n = 1 + rng.below(4) as usize;
    // vertices 0 (start of the rail), 1 (rail end = chain start), 2.. (chain), last = chain end
    let mut vertices = vec![vertex(FilletVertexType::RailEnd, FilStatus::Processed)];
    let mut uvs = vec![[0.0, 0.0]];
    for _ in 0..=n {
        vertices.push(vertex(FilletVertexType::RailEnd, FilStatus::Processed));
        uvs.push([rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)]);
    }
    let uv_tol = *rng.choose(&[1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6);
    let gap = |rng: &mut Rng| -> [f64; 2] {
        match rng.below(6) {
            0 => [uv_tol * 5.0, 0.0],   // a slight gap: the 3D check
            1 => [uv_tol * 100.0, 0.0], // too large: skipped
            _ => [0.0, 0.0],
        }
    };
    let mut rail = edge(0, 1, FilletEdgeType::Rail, [eu(0, None), eu(0, None)]);
    rail.curve = Some(gen_curve_on(rng, 3, scale, 0.0, 1.0));
    let uv = |i: usize| uvs.get(i).copied().unwrap_or([0.0; 2]);
    rail.eus[0].uv_curve = Some(uv_line(uv(0), uv(1), 0.0, 1.0));
    let mut edges = vec![rail];
    let mut sides = Vec::new();
    for i in 1..=n {
        let g = gap(rng);
        let a = [uv(i)[0] + g[0], uv(i)[1] + g[1]];
        let (t0, t1) = (rng.uniform(-1.0, 0.0), rng.uniform(1.0, 2.0));
        // the chain's edges run forward or backward
        let forward = rng.chance(0.6);
        let (vs, ve, ua, ub) = if forward {
            (i, i + 1, a, uv(i + 1))
        } else {
            (i + 1, i, uv(i + 1), a)
        };
        let mut e = edge(
            vs,
            ve,
            FilletEdgeType::CrossSection,
            [eu(0, None), eu(-1, None)],
        );
        e.curve = rng
            .chance(0.95)
            .then(|| gen_curve_on(rng, 3, scale, t0, t1));
        if rng.chance(0.9) {
            e.eus[0].uv_curve = Some(uv_line(ua, ub, t0, t1));
        } else if rng.chance(0.5) {
            // only the mate's: GetUVTrimCurve returns it
            e.eus[1].uv_curve = Some(uv_line(ua, ub, t0, t1));
        }
        edges.push(e);
        sides.push([edges.len() - 1, 0]);
    }
    // an unconnected side edge
    if rng.chance(0.3) {
        let mut e = edge(
            0,
            0,
            FilletEdgeType::CrossSection,
            [eu(0, None), eu(-1, None)],
        );
        e.curve = Some(gen_curve_on(rng, 3, scale, 0.0, 1.0));
        e.eus[0].uv_curve = Some(gen_curve_on(rng, 2, 1.0, 0.0, 1.0));
        edges.push(e);
        sides.insert(0, [edges.len() - 1, 0]);
    }
    let rng_shuffle = rng.below(2) == 0;
    if rng_shuffle {
        sides.reverse();
    }
    let geom = GeomIn {
        solver: 0,
        surface: Some(gen_bspline_surface(rng, scale, 0.0)),
        sides,
        rails: [0, -1],
        ..GeomIn::default()
    };
    let end = if rng.chance(0.85) { n + 1 } else { 0 };
    Inputs {
        brep: BrepIn::Network,
        model: ModelIn {
            solvers: vec![solver(rng, scale, false)],
            vertices,
            geoms: vec![geom],
            edges,
            ..ModelIn::default()
        },
        op: OpIn::Join {
            g: 0,
            vs: 1,
            ve: end as i64,
            uv_tol,
            start: vec![JoinStart {
                e: 0,
                which: 0,
                with_uv: rng.chance(0.85),
                orient: if rng.chance(0.85) { 1 } else { 2 },
            }],
        },
        expected_rejection: false,
    }
}

/// Two rails of a geometry (vertices 4g .. 4g+3, edges 2g, 2g+1).
fn rails_of(
    rng: &mut Rng,
    g: i64,
    vertices: &mut Vec<VertexIn>,
    edges: &mut Vec<EdgeIn>,
    with_curves: bool,
) {
    let base = vertices.len();
    for _ in 0..4 {
        vertices.push(vertex(FilletVertexType::RailEnd, FilStatus::Processed));
    }
    for r in 0..2 {
        let mut e = edge(
            base + 2 * r,
            base + 2 * r + 1,
            FilletEdgeType::Rail,
            [eu(g, None), eu(g, None)],
        );
        if with_curves {
            let (a, b) = (rng.uniform(-1.0, 0.0), rng.uniform(1.0, 2.0));
            e.curve = Some(gen_curve_on(rng, 3, 1.0, a, b));
        }
        if rng.chance(0.7) {
            e.eus[0].uv_curve = Some(gen_curve_on(rng, 2, 1.0, 0.0, 1.0));
        }
        if rng.chance(0.5) {
            e.eus[1].uv_curve = Some(gen_curve_on(rng, 2, 1.0, 0.0, 1.0));
        }
        e.orig_face = rng.below(1000) as i64;
        e.orig_edge = rng.below(1000) as i64;
        if rng.chance(0.5) {
            // the primary use runs against the rail
            e.eus.swap(0, 1);
            let (a, b) = (e.vs, e.ve);
            e.vs = b;
            e.ve = a;
        }
        edges.push(e);
    }
}

/// `SmFilletGeom::Copy`.
fn gen_copy(rng: &mut Rng) -> Inputs {
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    rails_of(rng, 0, &mut vertices, &mut edges, true);
    rails_of(rng, 1, &mut vertices, &mut edges, false);
    let with_surfaces = rng.chance(0.6);
    let s = solver(rng, 1.0, with_surfaces);
    let off = |rng: &mut Rng| {
        if rng.chance(0.4) {
            let z = rng.uniform(-0.5, 0.5);
            Some(OffIn {
                base: gen_bspline_surface(rng, 1.0, z),
                dist: rng.uniform(-0.2, 0.2),
            })
        } else {
            None
        }
    };
    let src = GeomIn {
        gtype: rng.below(7) as i64,
        solver: 0,
        surface: rng.chance(0.93).then(|| gen_bspline_surface(rng, 1.0, 0.0)),
        center: rng
            .chance(0.93)
            .then(|| gen_curve_on(rng, 3, 1.0, 0.0, 1.0)),
        devs: [rng.uniform(0.0, 1e-3), rng.uniform(0.0, 1e-3)],
        offs: [off(rng), off(rng)],
        rails: [0, 1],
        ..GeomIn::default()
    };
    let dst = GeomIn {
        solver: if rng.chance(0.95) { 0 } else { -1 },
        rails: [2, 3],
        ..GeomIn::default()
    };
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            solvers: vec![s],
            vertices,
            geoms: vec![src, dst],
            edges,
            ..ModelIn::default()
        },
        op: OpIn::Copy { g: 1, other: 0 },
        expected_rejection: false,
    }
}

/// The rail lookups of a geometry: `FindEdgeTangentAtPoint`, `GetRailIndexOfSideEU`,
/// `GetRailIndexOfVertexuse`, `GetRailVertex`, `SetUpRailPoint`.
fn gen_rail_lookup(rng: &mut Rng) -> Inputs {
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    rails_of(rng, 0, &mut vertices, &mut edges, true);
    for v in &mut vertices {
        match rng.below(4) {
            0 => {
                v.pc_class = PointClassType::Edge as i64;
                v.pc_kind = 4;
                v.pc_index = rng.below(1000) as i64;
                v.t_frac = Some(rng.uniform(0.0, 1.0));
            }
            1 => {
                v.pc_class = PointClassType::Vertex as i64;
                v.pc_kind = 1;
                v.pc_index = rng.below(1000) as i64;
            }
            _ => {}
        }
        if rng.chance(0.4) {
            v.status = 0;
        }
    }
    let mut geom = GeomIn {
        solver: 0,
        surface: rng.chance(0.95).then(|| gen_bspline_surface(rng, 1.0, 0.0)),
        rails: [0, 1],
        ..GeomIn::default()
    };
    let rail = rng.below(2) as usize;
    let end = rng.below(2) as usize;
    let op = match rng.below(5) {
        0 => OpIn::FindTangent { g: 0, rail, end },
        1 => OpIn::RailIndexSideEu {
            g: 0,
            eu: rng.below(1000) as i64,
        },
        2 => OpIn::RailIndexVu {
            g: 0,
            e: rng.below(2) as usize,
            which: rng.below(2) as usize,
        },
        3 => OpIn::RailVertex { g: 0, rail, end },
        _ => OpIn::SetUpRailPoint {
            g: 0,
            rail,
            end,
            param: rng.uniform(-0.5, 1.5),
            pc: rng.chance(0.7).then(|| PcIn {
                class: *rng.choose(&[103, 104, 102, 100]).unwrap_or(&103),
                kind: *rng.choose(&[4, 1, 3]).unwrap_or(&4),
                index: rng.below(1000) as i64,
                t: rng.uniform(0.0, 1.0),
            }),
        },
    };
    if matches!(op, OpIn::RailIndexSideEu { .. }) && rng.chance(0.2) {
        geom.rails = [0, -1]; // the solver's FindIndexOfRailXSideEdgeuse
    }
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            solvers: vec![solver(rng, 1.0, false)],
            vertices,
            geoms: vec![geom],
            edges,
            ..ModelIn::default()
        },
        op,
        expected_rejection: false,
    }
}

/// `sm_MoveTSP` on a list of intersection points.
fn gen_move_tsp(rng: &mut Rng) -> Inputs {
    let n = 1 + rng.below(5) as usize;
    let along = rng.chance(0.5);
    let pnts = (0..n)
        .map(|i| {
            let c = if along {
                [i as f64 + rng.uniform(-0.1, 0.1), 0.0, 0.0]
            } else {
                rng.point3(-1.0, 1.0)
            };
            [c, rng.point3(-1.0, 1.0)]
        })
        .collect();
    let from = rng.below(n as u64 + 1) as usize;
    let to = rng.below(n as u64 + 1) as usize;
    Inputs {
        brep: BrepIn::Network,
        model: ModelIn::default(),
        op: OpIn::MoveTsp { pnts, from, to },
        expected_rejection: false,
    }
}

/// `RailEdgeuseIntersect` and `RecordFilletError`.
fn gen_deferred(rng: &mut Rng) -> Inputs {
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    rails_of(rng, 0, &mut vertices, &mut edges, true);
    let mut s = solver(rng, 1.0, true);
    s.exec = true;
    let mut geom = GeomIn {
        solver: 0,
        rails: [0, 1],
        ..GeomIn::default()
    };
    let op = if rng.chance(0.5) {
        if rng.chance(0.15) {
            geom.solver = -1; // NER(pFS)
        }
        OpIn::RailEuIntersect {
            g: 0,
            rail: rng.below(2) as usize,
            eu: rng.below(1000) as i64,
            guess: rng.uniform(0.0, 1.0),
        }
    } else {
        OpIn::RecordError {
            g: 0,
            etype: rng.below(8) as i64,
        }
    };
    Inputs {
        brep: solid_brep(rng),
        model: ModelIn {
            solvers: vec![s],
            vertices,
            geoms: vec![geom],
            edges,
            ..ModelIn::default()
        },
        op,
        expected_rejection: false,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(24) {
        0..=2 => gen_edge_on_edge(rng),
        3..=5 => gen_fillet_end(rng),
        6..=8 => gen_set_back_rail(rng),
        9..=11 => gen_blending_rail(rng),
        12..=13 => gen_should_swap(rng),
        14..=16 => gen_join(rng),
        17..=18 => gen_copy(rng),
        19..=21 => gen_rail_lookup(rng),
        22 => gen_move_tsp(rng),
        _ => gen_deferred(rng),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_geom_p6"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<crate::Case>, HarnessError> {
        let ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(400),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
