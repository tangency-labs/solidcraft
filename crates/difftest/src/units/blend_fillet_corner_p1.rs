//! Runner for unit `blend.fillet_corner_p1`: the file-static helpers of
//! `SmFilletCorner.cpp` and the `SmFilletCorner` methods the unit owns, on main's
//! `FilletCorner` record (`blend::fillet_corner::p3`).
//!
//! C++: `solidcore_ffi::units::blend_fillet_corner_p1` (the shim compiles
//! `source/SMLib/src/SmFilletCorner.cpp`). Rust: `solidcore::blend::fillet_corner::p1`.
//!
//! Two kinds of case:
//!
//! - **Geometry**: `planar` (`sm_CreatePlanarSurface`: 2 to 5 points, collinear,
//!   tiny and huge scales), `revolution` (`sm_CreateSrfOfRevolution`: boundary loops of a
//!   real surface of revolution built from exact rational arcs and a generating curve in a
//!   plane through the axis, with 3 or 4 curves, a pole, a perturbed opposite curve, an
//!   arc of a different span, a major curve that is no arc), `fillet_radius`
//!   (`sm_GetFilletRadius`), `seam_out` and `closure` (`sm_CheckSeamOut`,
//!   `sm_SetSurfaceClosure`, on planes, open and closed B-spline surfaces). Compared: the
//!   status and the whole surface block (the plane frame, scale and domain; the B-spline
//!   net, knots and form), or the values.
//! - **Topology**: `g1`, `face_by_two`, `tangential`, `concavity` on a real B-rep (unit
//!   `topology.edgeuse_p1`'s B-rep block: the box-like fillet test files of
//!   `TestFiles/pt_TestFiles/Fillet`, which have convex and concave corners, and that unit's
//!   generated B-reps). The C++ side builds the B-rep, the Rust side imports the shim's
//!   export. A vertex is selected; edges and edge uses are selected among those at it (the
//!   edges ending at it, the edge uses starting at it), so the inputs are real corners.
//!   Compared: the flags, the angle and the face (export index).
//!
//! The shim refuses (`-2`) the inputs where upstream dereferences null; the Rust side
//! returns [`SolidError::InvalidInput`] there and both report `refused` (a both-errored
//! case). A topology case whose evaluated geometry the import holds only as its B-spline
//! part (analytic surfaces and curves) is deferred as `topology.edgeuse_p2`'s runner
//! defers it.


use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::blend::fillet_corner::p1 as corner;
use solidcore::blend::fillet_corner::p3::{
    CornerEnv, FilletCorner, FilletCornerKind, FilletExecutiveData,
};
use solidcore::blend::fillet_geom::p1::{FilStatus, FilletEdgeType, FilletSolverId};
use solidcore::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGenerator};
use solidcore::curve::curve::Curve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::kernel::Kernel;
use solidcore::boolean::topology_intersector::p1::TopologyIntersector;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::math::{Vec3, EFF_ZERO_RAD, EFF_ZERO_SQ};
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::math::NOW;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId, VertexId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_corner_p1::{self as ffi, CornerOp as FfiCornerOp, Op};

use super::blend_fillet_geom_p1::{
    curve_of as model_curve_of, gen_curve as model_gen_curve, gen_model, push_model,
    status as model_status, Crv, Model, RustModel, Srf,
};

use super::topology_edgeuse_p1::{
    brep_block_of, bspline_of, domain, flag, gen_brep, gen_curve, gen_surface, harness,
    path_of, push_curve, push_surface, sel, surface_of, BrepIn, CrvIn,
    ImportTypes, SrfIn,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Registered runner.
pub struct Runner;

/// The reason [`corner`] gives where upstream dereferences null.
pub(super) const NULL_DEREF: &str = "upstream dereferences null";

/// The box-like fillet test files (planar faces with convex and concave corners, one
/// all-NURBS box, one with a cylinder; filleted and B-spline parts whose tangential
/// edges reach the success paths of the tangency tests). Not `SmallBox.smp`: `SmBrep::ReadFromFile`
/// crashes on it (UB-1).
pub(super) const CORNER_FILES: [&str; 13] = [
    "Fillet/box1x1x2.smb",
    "Fillet/brick.smb",
    "Fillet/corner_box.smb",
    "Fillet/l_shaped_box.smb",
    "Fillet/6_sided_corner.smb",
    "Fillet/box_w_slot.smb",
    "Fillet/nurb_box.smb",
    "Fillet/box_on_box.smb",
    "Fillet/cyl_on_box.smb",
    "Fillet/filleted_box.smb",
    "Fillet/nurb_nurb_1.smb",
    "Fillet/nurb_plane_w_notch1.smb",
    "Fillet/brepForFillet1.smb",
];

/// One case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Planar {
        points: Vec<[f64; 3]>,
        tol: f64,
    },
    Revolution {
        curves: Vec<CrvIn>,
        major: usize,
        tol: f64,
    },
    FilletRadius {
        p1: [f64; 3],
        n1: [f64; 3],
        p2: [f64; 3],
        n2: [f64; 3],
        orient: f64,
    },
    SeamOut {
        surface: SrfIn,
        uv: [f64; 2],
        tol: f64,
        closure: i64,
    },
    Closure {
        surface: SrfIn,
        dom: [f64; 4],
    },
    G1 {
        brep: BrepIn,
        v: i64,
        e1: i64,
        e2: i64,
        tol: f64,
    },
    FaceByTwo {
        brep: BrepIn,
        v: i64,
        eu1: i64,
        eu2: i64,
        eu3: i64,
    },
    Tangential {
        brep: BrepIn,
        eu: i64,
        tol: f64,
    },
    Concavity {
        brep: BrepIn,
        v: i64,
        eu1: i64,
        eu2: i64,
        #[serde(rename = "ref")]
        ref_eu: i64,
        /// `eu2` is `eu1`'s corner mate at `v` (taken outward), as the edge uses around
        /// a corner are paired upstream; else `eu2` selects among those at `v`.
        #[serde(default)]
        adjacent: bool,
    },
    /// `SmFilletNx1Corner::CheckNx1Degeneracy(v, eu)`: `eu` selects among the edge uses
    /// starting at `v`.
    Nx1 {
        brep: BrepIn,
        v: i64,
        eu: i64,
    },
    /// An `SmFilletCorner` method on a corner built on `blend.fillet_geom_p1`'s
    /// pseudo-B-rep model (the corner as unit `blend.fillet_corner_p3`'s corner cases,
    /// without intersection points and original faces).
    Corner {
        method: CornerMethod,
        model: Box<Model>,
        corner: Box<CornerIn>,
        /// `peek`: the setters to call first (`-1` not called) and the type codes to
        /// test with `IsKindOf`.
        #[serde(default)]
        peek: PeekIn,
        /// `solvers`: the solvers registered for target edge uses, and the index of the
        /// one `GetTangentFilletSolver` is asked about (`len` = an unregistered one).
        #[serde(default)]
        solvers: Vec<SolverRegIn>,
        #[serde(default)]
        query: usize,
        /// `ext_surface`: `AddExtendedOriginalSurface` of a plane with natural domain
        /// `[0, s]²` per `(face, s)`, then `GetExtendedSurface` per face.
        #[serde(default)]
        adds: Vec<ExtAddIn>,
        #[serde(default)]
        queries: Vec<usize>,
        /// `squeeze`: the model edge to squeeze.
        #[serde(default)]
        squeeze: usize,
        /// `rail_rail`: the curve to test.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        curve: Option<Crv>,
        /// `fix_degeneracy`: the executive's topology edges (model edges).
        #[serde(default)]
        topo: Vec<usize>,
        /// `make_face` / `insert`: the corner's surfaces (`m_vSurfaces`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        surfaces: Vec<Srf>,
        /// `const_rad`: per model geometry its solver's surface generator.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fsgs: Vec<FsgIn>,
    },
}

/// The `SmFilletCorner` method of a `corner` case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CornerMethod {
    /// The data access methods, predicates and setters.
    Peek,
    /// `GetTangentFilletSolver` and `GetFilletSolvers`.
    Solvers,
    /// `AddExtendedOriginalSurface` and `GetExtendedSurface`.
    ExtSurface,
    /// `ReInitializeCornerEdges`.
    Reinit,
    /// `SqueezeEdge`.
    Squeeze,
    /// `SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve`.
    RailRail,
    /// `CalcCornerVertGeom`.
    VertGeom,
    /// `CalcCornerEdgeGeom`.
    EdgeGeom,
    /// `CheckAndFixDegeneracy`.
    FixDegeneracy,
    /// `CreateEndPatch`.
    EndPatch,
    /// `MakeFaceBrep` on the given surfaces.
    MakeFace,
    /// `InsertIntersectionTopology` with the given surfaces.
    Insert,
    /// `MakeConstantRadiusFillet` with a solver per model geometry.
    ConstRad,
}

impl CornerMethod {
    const fn ffi(self) -> FfiCornerOp {
        match self {
            Self::Peek => FfiCornerOp::Peek,
            Self::Solvers => FfiCornerOp::Solvers,
            Self::ExtSurface => FfiCornerOp::ExtSurface,
            Self::Reinit => FfiCornerOp::Reinit,
            Self::Squeeze => FfiCornerOp::Squeeze,
            Self::RailRail => FfiCornerOp::RailRail,
            Self::VertGeom => FfiCornerOp::VertGeom,
            Self::EdgeGeom => FfiCornerOp::EdgeGeom,
            Self::FixDegeneracy => FfiCornerOp::FixDegeneracy,
            Self::EndPatch => FfiCornerOp::EndPatch,
            Self::MakeFace => FfiCornerOp::MakeFace,
            Self::Insert => FfiCornerOp::Insert,
            Self::ConstRad => FfiCornerOp::ConstRad,
        }
    }
}

/// An edge use of the model's target wire edge `e`: `0` its primary edge use, `1` the
/// mate.
type EuSel = [usize; 2];

/// The corner of a `corner` case (as `blend.fillet_corner_p3`'s, without intersection
/// points and original faces).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // the corner's independent SmBoolean members
struct CornerIn {
    /// The derived class (order of [`FilletCornerKind`]).
    kind: usize,
    /// The filleted vertex: start (`v_sub` 0) or end vertex of target edge `v_edge`
    /// (`-1` none); for `v_edge <= -2` vertex `v_sub` of target face `-(v_edge + 2)`
    /// (`SmFace::GetVertices` order).
    v_edge: i64,
    #[serde(default)]
    v_sub: usize,
    solver: Option<Vec<EuSel>>,
    all: Option<Vec<EuSel>>,
    #[serde(default)]
    verts: Vec<usize>,
    #[serde(default)]
    edges: Vec<usize>,
    #[serde(default)]
    bevel: bool,
    #[serde(default)]
    blending: bool,
    #[serde(default)]
    chamfer: bool,
    #[serde(default)]
    degenerate: bool,
    #[serde(default = "yes")]
    ext: bool,
    #[serde(default)]
    adjusted: bool,
    #[serde(default)]
    setback: f64,
    tol: f64,
    tan: f64,
    /// `SetOriginalFace` of pseudo edges: `[edge, target face]`.
    #[serde(default)]
    orig_faces: Vec<[usize; 2]>,
}

fn yes() -> bool {
    true
}

/// The `peek` method's setters (`-1` not called, else the flag) and type codes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PeekIn {
    #[serde(default = "minus_one")]
    set_bevel: i64,
    #[serde(default = "minus_one")]
    set_const_rad: i64,
    #[serde(default = "minus_one")]
    set_chamfer: i64,
    #[serde(default = "minus_one")]
    add_vertex: i64,
    #[serde(default)]
    kinds: Vec<f64>,
    /// `m_bG1Case` of the closed Nx1 / Nx2 corner (`-1` untouched).
    #[serde(default = "minus_one")]
    set_g1: i64,
    /// `SetStatus` with an `SmFilStatus` code (`-1` not called).
    #[serde(default = "minus_one")]
    set_status: i64,
    /// `GetFilletEdges` / `GetFilletVertices` append to arrays holding the model's
    /// first edge / vertex.
    #[serde(default)]
    prefill: bool,
}

fn minus_one() -> i64 {
    -1
}

/// A solver registered in the executive for a target edge use.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SolverRegIn {
    e: usize,
    which: usize,
    approx: f64,
    tang: f64,
}

/// The fillet solver of a model geometry in a `const_rad` case: `kind` -1 none, 0
/// `SmCircularCrossSectionFSG()`, 1 `SmBlendCurveCrossSectionFSG(scale, continuity)`, 2
/// `SmLinearCrossSectionFSG`, 3 the base `SmFilletSurfaceGenerator`, 4 no generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FsgIn {
    kind: i64,
    #[serde(default = "one_and_a_half")]
    scale: f64,
    #[serde(default)]
    continuity: u32,
}

fn one_and_a_half() -> f64 {
    1.5
}

/// An `AddExtendedOriginalSurface` call: the target face and the plane's domain size.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtAddIn {
    face: usize,
    s: f64,
}

const KINDS: [FilletCornerKind; 11] = [
    FilletCornerKind::OneByOne,
    FilletCornerKind::TwoByTwo,
    FilletCornerKind::NByOneClosed,
    FilletCornerKind::NByOne,
    FilletCornerKind::Degenerate,
    FilletCornerKind::Open,
    FilletCornerKind::ThreeByTwoMixed,
    FilletCornerKind::NByTwo,
    FilletCornerKind::FourByThree,
    FilletCornerKind::ConvexNByN,
    FilletCornerKind::ConcaveNByN,
];

// ---- shared -----------------------------------------------------------------------------

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::from_array(a)
}

/// The `SmStatus` of a Rust error: the upstream code, `SM_ERR_INVALID_INPUT` (1007) for
/// [`SolidError::InvalidInput`]; `None` for a null dereference (refused) or an error
/// with no upstream status.
fn status_of(e: &SolidError) -> Option<f64> {
    match e {
        SolidError::Upstream { code, .. } => Some(f64::from(*code)),
        SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. } if *reason != NULL_DEREF => Some(1007.0),
        _ => None,
    }
}

/// A Rust error as an output: deferred for `Unported`, `refused` for a null
/// dereference, else the status.
pub(super) fn rust_err(e: &SolidError) -> Output {
    if let Some(o) = Output::unported(e) {
        return o;
    }
    if matches!(e, SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. } if *reason == NULL_DEREF) {
        return Output::err("refused");
    }
    match status_of(e) {
        Some(st) => Output::new().values("status", vec![st]),
        None => Output::err(format!("rust: {e}")),
    }
}

/// A surface block, as the shim's `put_surface` (planes and B-spline surfaces).
fn put_surface(o: &mut Vec<f64>, s: &Surface) -> Result<(), HarnessError> {
    match s.kind() {
        SurfaceKind::Plane(p) => {
            o.push(1.0);
            let pos = p.position();
            for v in [pos.origin(), pos.x_axis(), pos.y_axis()] {
                o.extend([v.x, v.y, v.z]);
            }
            let sc = p.uv_scale();
            o.extend([sc.x, sc.y]);
            let d = p.step_uv_domain();
            o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
        }
        SurfaceKind::BSpline(bs) => {
            let nurb = bs
                .gw_nurb()
                .ok_or_else(|| harness("surface without NURBS data"))?;
            let (p, q) = nurb.degrees();
            o.extend([
                0.0,
                flag(bs.is_out_of_bounds_enabled()),
                bs.form() as u64 as f64,
                nurb.n() as f64,
                nurb.m() as f64,
                p as f64,
                q as f64,
                (nurb.u_knots().len() - 1) as f64,
                (nurb.v_knots().len() - 1) as f64,
            ]);
            for row in nurb.cpts() {
                for pt in row {
                    o.extend([pt.x, pt.y, pt.z, pt.w]);
                }
            }
            o.extend_from_slice(nurb.u_knots());
            o.extend_from_slice(nurb.v_knots());
        }
        _ => o.push(-2.0),
    }
    Ok(())
}

/// A C++ `status [surface block]` record as an output.
fn cpp_surface(out: &[f64]) -> Result<Output, HarnessError> {
    let st = *out.first().ok_or_else(|| harness("empty output"))?;
    let o = Output::new().values("status", vec![st]);
    Ok(if st == 0.0 {
        o.values("surface", out.get(1..).unwrap_or_default().to_vec())
    } else {
        o
    })
}

/// A Rust surface result as an output.
fn rust_surface(r: solidcore::Result<Surface>) -> Result<Output, HarnessError> {
    match r {
        Ok(s) => {
            let mut block = Vec::new();
            put_surface(&mut block, &s)?;
            Ok(Output::new()
                .values("status", vec![0.0])
                .values("surface", block))
        }
        Err(e) => Ok(rust_err(&e)),
    }
}

fn surface_args(s: &SrfIn) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    push_surface(&mut v, s)?;
    Ok(v)
}

fn closure_of(code: i64) -> Result<SurfParamType, HarnessError> {
    Ok(match code {
        0 => SurfParamType::U,
        1 => SurfParamType::V,
        2 => SurfParamType::Unknown,
        3 => SurfParamType::Both,
        4 => SurfParamType::Neither,
        _ => return Err(harness(format!("closure code {code} outside 0..=4"))),
    })
}

// ---- topology setup ---------------------------------------------------------------------

/// The built B-rep of a topology case on both sides and the resolved graph indices of the
/// op's arguments (`-1` = NULL).
pub(super) struct Topo {
    pub(super) path: String,
    pub(super) block: Vec<f64>,
    pub(super) brep: Brep,
    pub(super) types: ImportTypes,
    pub(super) args: Vec<i64>,
}

pub(super) fn eu_vertex(b: &Brep, eu: EdgeuseId) -> Option<VertexId> {
    let vu = b.edgeuse(eu).ok()?.vertexuse?;
    b.vertexuse(vu).ok()?.vertex
}

fn idx_u32(i: i64) -> Result<u32, HarnessError> {
    u32::try_from(i).map_err(|_| harness("index"))
}

/// A selector into `ring` (export indices), falling back to all `n` when it is empty.
fn pick(s: i64, ring: &[i64], n: usize) -> i64 {
    if s < 0 {
        return -1;
    }
    let r = sel(s, ring.len());
    if r >= 0 {
        return ring.get(r as usize).copied().unwrap_or(-1);
    }
    sel(s, n)
}

/// Builds and imports the B-rep and resolves the selectors of `i`; `Err(output)` when the
/// case ends before the op (the B-rep cannot be built, a required entity is missing),
/// with the same output on both sides.
fn topo(i: &Inputs, own: bool) -> Result<Result<Topo, Output>, HarnessError> {
    let (Inputs::G1 { brep, .. }
    | Inputs::FaceByTwo { brep, .. }
    | Inputs::Tangential { brep, .. }
    | Inputs::Concavity { brep, .. }
    | Inputs::Nx1 { brep, .. }) = i
    else {
        return Err(harness("not a topology case"));
    };
    let path = path_of(Some(brep))?;
    let block = brep_block_of(Some(brep), false, false)?;
    // the Rust side's B-rep is read or built by the Rust kernel, never imported
    // (`crate::own_reader::edgeuse_case`)
    let (c, b, types) = match crate::own_reader::edgeuse_case(brep, false, false, own)? {
        Ok(x) => x,
        Err(o) => return Ok(Err(o)),
    };
    let (n_eu, n_e, n_v) = (c[6], c[7], c[9]);

    // the edge uses starting at `v`, the edges ending at `v` (export order)
    let ring_eus = |v: i64| -> Vec<i64> {
        if v < 0 {
            return Vec::new();
        }
        let vid = VertexId::from_index(v as u32);
        b.edgeuses()
            .iter()
            .map(|(id, _)| id)
            .filter(|&id| eu_vertex(&b, id) == Some(vid))
            .map(|id| i64::from(id.index()))
            .collect()
    };
    let ring_edges = |v: i64| -> Vec<i64> {
        if v < 0 {
            return Vec::new();
        }
        let vid = Some(VertexId::from_index(v as u32));
        b.edges()
            .iter()
            .map(|(id, _)| id)
            .filter(|&id| {
                edge::start_vertex(&b, id).ok().flatten() == vid
                    || edge::end_vertex(&b, id).ok().flatten() == vid
            })
            .map(|id| i64::from(id.index()))
            .collect()
    };

    let args = match i {
        Inputs::G1 { v, e1, e2, .. } => {
            let vv = sel(*v, n_v);
            let ring = ring_edges(vv);
            vec![vv, pick(*e1, &ring, n_e), pick(*e2, &ring, n_e)]
        }
        Inputs::FaceByTwo {
            v, eu1, eu2, eu3, ..
        } => {
            let ring = ring_eus(sel(*v, n_v));
            vec![
                pick(*eu1, &ring, n_eu),
                pick(*eu2, &ring, n_eu),
                sel(*eu3, n_eu),
            ]
        }
        Inputs::Tangential { eu, .. } => {
            let e = sel(*eu, n_eu);
            if e < 0 {
                return Ok(Err(Output::err("no_edge_use")));
            }
            vec![e]
        }
        Inputs::Concavity {
            v,
            eu1,
            eu2,
            ref_eu,
            adjacent,
            ..
        } => {
            let vv = sel(*v, n_v);
            let ring = ring_eus(vv);
            if vv < 0 || ring.is_empty() {
                return Ok(Err(Output::err("no_edge_use")));
            }
            let a = pick(eu1.rem_euclid(1 << 30), &ring, n_eu);
            let mut bb = pick(eu2.rem_euclid(1 << 30), &ring, n_eu);
            if *adjacent {
                // the corner mate of `a` at the vertex, outward (itself or its mate)
                let vid = VertexId::from_index(idx_u32(vv)?);
                let eu_a = EdgeuseId::from_index(idx_u32(a)?);
                if let Ok(Some(cm)) = eu1::corner_mate_edgeuse(&b, eu_a, Some(vid)) {
                    let out = if eu_vertex(&b, cm) == Some(vid) {
                        Some(cm)
                    } else {
                        eu1::mate(&b, cm).ok().flatten()
                    };
                    if let Some(o) = out {
                        bb = i64::from(o.index());
                    }
                }
            }
            vec![vv, a, bb, sel(*ref_eu, n_eu)]
        }
        Inputs::Nx1 { v, eu, .. } => {
            let vv = sel(*v, n_v);
            let ring = ring_eus(vv);
            if vv < 0 || ring.is_empty() {
                return Ok(Err(Output::err("no_edge_use")));
            }
            vec![vv, pick(*eu, &ring, n_eu)]
        }
        _ => return Err(harness("not a topology case")),
    };
    Ok(Ok(Topo {
        path,
        block,
        brep: b,
        types,
        args,
    }))
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE`,
/// `SmCrvOnSurf_TYPE` (as `topology.edgeuse_p2`'s runner).
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 3] = [14033, 14010, 14048];

/// The deferral label of cases whose geometry the shared B-rep export holds only as a
/// B-spline part (an analytic face surface or edge curve the import cannot rebuild
/// exactly): a harness limitation, not an unported unit (`topology.brep_p1`'s
/// convention).
const HARNESS_ANALYTIC_EXPORT: &str = "harness:analytic-geometry-export";

/// The deferral of a case that evaluates geometry the import holds only as its B-spline
/// part: the labels of `topology.edgeuse_p2`'s `inexact_geometry`.
pub(super) fn inexact(t: &Topo, edges: &[EdgeId], eus: &[EdgeuseId]) -> Option<Output> {
    let b = &t.brep;
    let mut all_edges = edges.to_vec();
    for &eu in eus {
        let mut both = vec![eu];
        if let Ok(Some(r)) = eu1::radial(b, eu) {
            both.push(r);
        }
        for e in both {
            let st = eu1::face(b, e)
                .ok()
                .flatten()
                .and_then(|f| t.types.inexact_surface(f.index() as usize))
                .unwrap_or(-1);
            if st >= 0 && !EXACT_SURFACES.contains(&st) {
                let (unit, symbol) = match st {
                    15012 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone dispatch)"),
                    15013 => (
                        "surface.surface_p2",
                        "SurfaceKind::Cone (SmCylinder dispatch)",
                    ),
                    15014 => (
                        "surface.surface_p2",
                        "SurfaceKind::Sphere (SmSphere dispatch)",
                    ),
                    15015 => (
                        "surface.surface_p2",
                        "SurfaceKind::Torus (SmTorus dispatch)",
                    ),
                    15030 => (
                        "surface.surf_of_extrusion_p2",
                        "SurfaceKind::SurfOfExtrusion",
                    ),
                    _ => (
                        HARNESS_ANALYTIC_EXPORT,
                        "face surface of an analytic kind exported as its B-spline part",
                    ),
                };
                return Some(Output::deferred(unit, symbol));
            }
            if let Ok(Some(ed)) = eu1::edge(b, e) {
                all_edges.push(ed);
            }
        }
    }
    for e in all_edges {
        let ct = t
            .types
            .inexact_curve(e.index() as usize)
            .unwrap_or(-1);
        if ct >= 0 && !EXACT_CURVES.contains(&ct) {
            return Some(Output::deferred(
                HARNESS_ANALYTIC_EXPORT,
                "edge curve of an analytic kind exported as its B-spline part",
            ));
        }
    }
    None
}

fn opt<T>(i: i64, f: impl Fn(u32) -> T) -> Result<Option<T>, HarnessError> {
    Ok(if i < 0 { None } else { Some(f(idx_u32(i)?)) })
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let call = |op: Op, path: &str, input: &[f64]| {
        ffi::eval(op, path, input).map_err(|e| harness(format!("c++: {e}")))
    };
    match i {
        Inputs::Planar { points, tol } => {
            let mut v = vec![points.len() as f64];
            for p in points {
                v.extend(p);
            }
            v.push(*tol);
            cpp_surface(&call(Op::Planar, "", &v)?)
        }
        Inputs::Revolution { curves, major, tol } => {
            let mut v = vec![curves.len() as f64];
            for c in curves {
                push_curve(&mut v, c)?;
            }
            v.extend([*major as f64, *tol]);
            cpp_surface(&call(Op::Revolution, "", &v)?)
        }
        Inputs::FilletRadius {
            p1,
            n1,
            p2,
            n2,
            orient,
        } => {
            let mut v = Vec::new();
            for a in [p1, n1, p2, n2] {
                v.extend(a);
            }
            v.push(*orient);
            Ok(Output::new().values("radius_span", call(Op::FilletRadius, "", &v)?))
        }
        Inputs::SeamOut {
            surface,
            uv,
            tol,
            closure,
        } => {
            let mut v = surface_args(surface)?;
            v.extend([uv[0], uv[1], *tol, *closure as f64]);
            Ok(Output::new().values("seam", call(Op::SeamOut, "", &v)?))
        }
        Inputs::Closure { surface, dom } => {
            let mut v = surface_args(surface)?;
            v.extend(dom);
            Ok(Output::new().values("closure", call(Op::Closure, "", &v)?))
        }
        Inputs::Corner { .. } => corner_cpp(i),
        _ => {
            let t = match topo(i, false)? {
                Ok(t) => t,
                Err(o) => return Ok(o),
            };
            let (op, tail): (Op, Vec<f64>) = match i {
                Inputs::G1 { tol, .. } => (Op::G1, vec![*tol]),
                Inputs::FaceByTwo { .. } => (Op::FaceByTwo, vec![]),
                Inputs::Tangential { tol, .. } => (Op::Tangential, vec![*tol]),
                Inputs::Nx1 { .. } => (Op::Nx1Degeneracy, vec![]),
                _ => (Op::Concavity, vec![]),
            };
            let mut v = t.block.clone();
            v.extend(t.args.iter().map(|&a| a as f64));
            v.extend(tail);
            let out = call(op, &t.path, &v)?;
            let Some((&head, rest)) = out.split_first() else {
                return Err(harness("empty topology output"));
            };
            if head != 0.0 {
                let st = rest.first().copied().unwrap_or(0.0);
                return Ok(Output::err(format!("brep_build_status_{st}")));
            }
            if rest.first().copied() == Some(-2.0) {
                return Ok(Output::err("refused"));
            }
            let key = match op {
                Op::G1 => "g1",
                Op::FaceByTwo => "face",
                Op::Nx1Degeneracy => "ret",
                _ => "flag",
            };
            Ok(Output::new().values(key, rest.to_vec()))
        }
    }
}

// ---- Rust side --------------------------------------------------------------------------

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    match i {
        Inputs::Planar { points, tol } => {
            let pts: Vec<Vec3> = points.iter().map(|&p| v3(p)).collect();
            rust_surface(corner::create_planar_surface(&k, &pts, *tol))
        }
        Inputs::Revolution { curves, major, tol } => {
            let cs = curves
                .iter()
                .map(|c| bspline_of(c).map(Curve::from))
                .collect::<Result<Vec<_>, _>>()?;
            rust_surface(corner::create_srf_of_revolution(&k, &cs, *major, *tol))
        }
        Inputs::FilletRadius {
            p1,
            n1,
            p2,
            n2,
            orient,
        } => match corner::fillet_radius(v3(*p1), v3(*n1), v3(*p2), v3(*n2), *orient) {
            Ok((r, s)) => Ok(Output::new().values("radius_span", vec![r, s])),
            Err(e) => Ok(rust_err(&e)),
        },
        Inputs::SeamOut {
            surface,
            uv,
            tol,
            closure,
        } => {
            let s = surface_of(surface)?;
            let mut p = Point2::new(uv[0], uv[1]);
            let ret = corner::check_seam_out(&s, &mut p, *tol, closure_of(*closure)?);
            Ok(Output::new().values("seam", vec![flag(ret), p.x, p.y]))
        }
        Inputs::Closure { surface, dom } => {
            let s = surface_of(surface)?;
            let d = Extent2d::from_points(Point2::new(dom[0], dom[1]), Point2::new(dom[2], dom[3]));
            match corner::set_surface_closure(&k, &s, &d) {
                Ok(c) => Ok(Output::new().values("closure", vec![c as u64 as f64])),
                Err(e) => Ok(rust_err(&e)),
            }
        }
        Inputs::Corner { .. } => corner_rust(i),
        _ => {
            let mut t = match topo(i, true)? {
                Ok(t) => t,
                Err(o) => return Ok(o),
            };
            let args = t.args.clone();
            let a = |n: usize| -> Result<i64, HarnessError> {
                args.get(n)
                    .copied()
                    .ok_or_else(|| harness("missing argument"))
            };
            let eu = |x: i64| opt(x, EdgeuseId::from_index);
            let r: solidcore::Result<Vec<f64>> = match i {
                Inputs::G1 { tol, .. } => {
                    let (e1, e2) = (
                        opt(a(1)?, EdgeId::from_index)?,
                        opt(a(2)?, EdgeId::from_index)?,
                    );
                    let edges: Vec<EdgeId> = [e1, e2].into_iter().flatten().collect();
                    if let Some(o) = inexact(&t, &edges, &[]) {
                        return Ok(o);
                    }
                    let v = opt(a(0)?, VertexId::from_index)?;
                    corner::test_g1_continuity(&k, &t.brep, v, e1, e2, *tol)
                        .map(|(f, ang)| vec![flag(f), flag(ang.is_some()), ang.unwrap_or(0.0)])
                }
                Inputs::FaceByTwo { .. } => {
                    corner::face_by_two_edgeuses(&t.brep, eu(a(0)?)?, eu(a(1)?)?, eu(a(2)?)?)
                        .map(|f| vec![f.map_or(-1.0, |f| f64::from(f.index()))])
                }
                Inputs::Tangential { tol, .. } => {
                    let e = EdgeuseId::from_index(idx_u32(a(0)?)?);
                    if let Some(o) = inexact(&t, &[], &[e]) {
                        return Ok(o);
                    }
                    corner::check_tangential_edgeuse(&k, &mut t.brep, e, *tol)
                        .map(|f| vec![flag(f)])
                }
                Inputs::Nx1 { .. } => {
                    let v = VertexId::from_index(idx_u32(a(0)?)?);
                    let e = EdgeuseId::from_index(idx_u32(a(1)?)?);
                    // the side edge uses the routine evaluates
                    let b = &t.brep;
                    let mut eus = vec![e];
                    if let Ok(Some(s1)) = eu1::cw_edgeuse(b, e) {
                        eus.push(s1);
                        let s2 = eu1::radial(b, e)
                            .ok()
                            .flatten()
                            .and_then(|r| eu1::ccw_edgeuse(b, r).ok().flatten());
                        match s2 {
                            Some(s2) if s2 == s1 => eus.extend(eu1::mate(b, s1).ok().flatten()),
                            Some(s2) => eus.push(s2),
                            None => {}
                        }
                    }
                    if let Some(o) = inexact(&t, &[], &eus) {
                        return Ok(o);
                    }
                    // the raw `SmBoolean` return: the flag, or the status `SER` returns
                    match corner::check_nx1_degeneracy(&k, &mut t.brep, v, e) {
                        Ok(f) => Ok(vec![flag(f)]),
                        Err(e) if e.is_unported() => Err(e),
                        Err(e) if matches!(&e, SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. } if *reason == NULL_DEREF) => {
                            Err(e)
                        }
                        Err(e) => Ok(vec![model_status(&e)]),
                    }
                }
                _ => {
                    let v = VertexId::from_index(idx_u32(a(0)?)?);
                    let (e1, e2) = (
                        EdgeuseId::from_index(idx_u32(a(1)?)?),
                        EdgeuseId::from_index(idx_u32(a(2)?)?),
                    );
                    let r = eu(a(3)?)?;
                    let eus: Vec<EdgeuseId> =
                        [Some(e1), Some(e2), r].into_iter().flatten().collect();
                    if let Some(o) = inexact(&t, &[], &eus) {
                        return Ok(o);
                    }
                    corner::check_face_complimentary_corner_concavity(&k, &mut t.brep, v, e1, e2, r)
                        .map(|f| vec![flag(f)])
                }
            };
            let key = match i {
                Inputs::G1 { .. } => "g1",
                Inputs::FaceByTwo { .. } => "face",
                Inputs::Nx1 { .. } => "ret",
                _ => "flag",
            };
            match r {
                Ok(v) => Ok(Output::new().values(key, v)),
                Err(e) => Ok(rust_err(&e)),
            }
        }
    }
}

// ---- generation -------------------------------------------------------------------------

fn scale_of(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-4, 1.0, 1.0, 1.0, 1.0, 1e4]).unwrap_or(&1.0)
}

fn gen_planar(rng: &mut Rng) -> Inputs {
    let s = scale_of(rng);
    let n = *rng.choose(&[2usize, 3, 3, 3, 4, 4, 4, 5]).unwrap_or(&3);
    let mut points: Vec<[f64; 3]> = (0..n)
        .map(|_| rng.point3(-1.0, 1.0).map(|c| c * s))
        .collect();
    let collinear = rng.chance(0.15);
    let t = rng.uniform(-2.0, 2.0);
    if let [a, b, c] = points.as_mut_slice() {
        if collinear {
            // collinear (or coincident) points: Unitize fails
            *c = (v3(*a) + (v3(*b) - v3(*a)) * t).to_array();
        }
    }
    Inputs::Planar {
        points,
        tol: rng.uniform(0.0, 1e-3) * s,
    }
}

/// The exact rational quadratic arc of `angle` (radians, at most `PI / 2`) about the axis
/// through `c` along unit `z`, starting at `p` (homogeneous control points).
fn arc(c: Vec3, z: Vec3, p: Vec3, angle: f64) -> CrvIn {
    let center = c + z * (p - c).dot(z);
    let x = p - center;
    let y = z.cross(x);
    let at = |t: f64, scale: f64| center + (x * t.cos() + y * t.sin()) * scale;
    let half = angle / 2.0;
    let w = half.cos();
    let (p0, p1, p2) = (at(0.0, 1.0), at(half, 1.0 / w), at(angle, 1.0));
    CrvIn {
        dim: 3,
        cpts: vec![
            [p0.x, p0.y, p0.z, 1.0],
            [p1.x * w, p1.y * w, p1.z * w, w],
            [p2.x, p2.y, p2.z, 1.0],
        ],
        p: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

/// `p` rotated by `angle` about the z axis through the origin.
fn rot_z(p: [f64; 3], angle: f64) -> [f64; 3] {
    let [x, y, z] = p;
    let (s, c) = angle.sin_cos();
    [c * x - s * y, s * x + c * y, z]
}

/// A polynomial curve with control points `pts` (degree `min(3, n - 1)`, clamped
/// uniform knots).
fn poly(pts: &[[f64; 3]]) -> CrvIn {
    let n = pts.len().saturating_sub(1);
    let p = n.min(3);
    let mut u = vec![0.0; p + 1];
    for i in 1..(n + 1 - p) {
        u.push(i as f64 / (n + 1 - p) as f64);
    }
    u.extend(vec![1.0; p + 1]);
    CrvIn {
        dim: 3,
        cpts: pts.iter().map(|&[x, y, z]| [x, y, z, NOW]).collect(),
        p,
        u,
    }
}

/// The reversed curve (control points and knots).
fn reversed(c: &CrvIn) -> CrvIn {
    let (a, b) = (
        c.u.first().copied().unwrap_or(0.0),
        c.u.last().copied().unwrap_or(1.0),
    );
    CrvIn {
        dim: c.dim,
        cpts: c.cpts.iter().rev().copied().collect(),
        p: c.p,
        u: c.u.iter().rev().map(|t| a + b - t).collect(),
    }
}

/// A boundary loop of a surface of revolution about the z axis: the generating curve `G`
/// (in the xz plane), the arc from its end, `G` rotated (reversed), the arc back (absent
/// when `G` starts on the axis: a pole). The loop is `[G, arc_end, rot(G)^-1,
/// arc_start^-1]`, so the curve before each arc is a generating curve.
fn gen_revolution(rng: &mut Rng) -> Inputs {
    let s = scale_of(rng);
    let angle = rng.uniform(0.2, std::f64::consts::FRAC_PI_2);
    let pole = rng.chance(0.2);
    let n = 2 + rng.below(3) as usize;
    let gen: Vec<[f64; 3]> = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let r = if pole && i == 0 {
                0.0
            } else {
                rng.uniform(0.5, 2.0)
            };
            [r * s, 0.0, (t * 2.0 + rng.uniform(-0.1, 0.1)) * s]
        })
        .collect();
    let g = poly(&gen);
    let g_end = v3(gen.last().copied().unwrap_or_default());
    let g_start = v3(gen.first().copied().unwrap_or_default());
    let (o, z) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let arc_end = arc(o, z, g_end, angle);
    let rot: Vec<[f64; 3]> = gen.iter().map(|&p| rot_z(p, angle)).collect();
    let g_rot = reversed(&poly(&rot));
    let mut curves = vec![g, arc_end, g_rot];
    if !pole {
        curves.push(reversed(&arc(o, z, g_start, angle)));
    }
    let total = curves.len();
    // the major arc: the arc at G's end (index 1) mostly
    let mut major = if rng.chance(0.8) {
        1
    } else {
        rng.below(total as u64) as usize
    };
    match rng.below(10) {
        // the opposite arc of a different span (4 curves)
        0 => {
            let shorter = reversed(&arc(o, z, g_start, angle * rng.uniform(0.5, 0.9)));
            if let Some(c) = curves.get_mut(3) {
                *c = shorter;
            }
        }
        // the curve after the major arc off the surface
        1 => {
            if let Some(c) = curves.get_mut((major + 1) % total) {
                for p in &mut c.cpts {
                    // homogeneous: shift x by 5% of the scale (weight 1 when non-rational)
                    let w = if p[3] == NOW { 1.0 } else { p[3] };
                    p[0] += 0.05 * s * w;
                }
            }
        }
        // a major curve that is no arc
        2 => {
            let other = gen_curve(rng, 3, s);
            if let Some(c) = curves.get_mut(major) {
                *c = other;
            }
        }
        // a random loop of curves
        3 => {
            let m = 1 + rng.below(5) as usize;
            curves = (0..m).map(|_| gen_curve(rng, 3, s)).collect();
            major = rng.below(m as u64) as usize;
        }
        _ => {}
    }
    Inputs::Revolution {
        curves,
        major,
        tol: *rng.choose(&[1e-9, 1e-6, 1e-4, 1e-2]).unwrap_or(&1e-6) * s,
    }
}

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.1 {
            return v.map(|c| c / l);
        }
    }
}

fn gen_fillet_radius(rng: &mut Rng) -> Inputs {
    let s = scale_of(rng);
    let n1 = unit_vec(rng);
    let n2 = match rng.below(8) {
        0 => n1,
        1 => n1.map(|c| -c),
        _ => unit_vec(rng),
    };
    let p1 = rng.point3(-1.0, 1.0).map(|c| c * s);
    let p2 = if rng.chance(0.1) {
        p1
    } else {
        rng.point3(-1.0, 1.0).map(|c| c * s)
    };
    Inputs::FilletRadius {
        p1,
        n1: n1.map(|c| c * rng.uniform(0.5, 2.0)),
        p2,
        n2,
        orient: *rng.choose(&[-1.0, 1.0, 0.0]).unwrap_or(&1.0),
    }
}

/// A B-spline surface closed in u and/or v (the first and last control rows equal).
fn gen_closed_surface(rng: &mut Rng) -> SrfIn {
    let p = 1 + rng.below(2) as usize;
    let q = 1 + rng.below(2) as usize;
    let n = p + 1 + rng.below(3) as usize;
    let m = q + 1 + rng.below(3) as usize;
    let close_u = rng.chance(0.7);
    let close_v = !close_u || rng.chance(0.4);
    let mut net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    let b = std::f64::consts::TAU * j as f64 / m as f64;
                    [
                        (2.0 + b.cos()) * a.cos(),
                        (2.0 + b.cos()) * a.sin(),
                        j as f64 + b.sin(),
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    if close_u {
        let first = net.first().cloned();
        if let (Some(first), Some(last)) = (first, net.last_mut()) {
            *last = first;
        }
    }
    if close_v {
        for row in &mut net {
            let first = row.first().copied();
            if let (Some(first), Some(last)) = (first, row.last_mut()) {
                *last = first;
            }
        }
    }
    let knots = |deg: usize, cnt: usize| {
        let mut u = vec![0.0; deg + 1];
        for i in 1..(cnt + 1 - deg) {
            u.push(i as f64 / (cnt + 1 - deg) as f64);
        }
        u.extend(vec![1.0; deg + 1]);
        u
    };
    serde_json::from_value(json!({"bspline": {
        "net": net, "p": p, "q": q, "u": knots(p, n), "v": knots(q, m)
    }}))
    .unwrap_or(SrfIn::Plane {
        o: [0.0; 3],
        n: [0.0, 0.0, 1.0],
    })
}

fn gen_seam_surface(rng: &mut Rng) -> SrfIn {
    if rng.chance(0.5) {
        gen_closed_surface(rng)
    } else {
        gen_surface(rng)
    }
}

fn gen_seam_out(rng: &mut Rng) -> Inputs {
    let surface = gen_seam_surface(rng);
    let [u0, v0, u1, v1] = domain(&surface);
    let (du, dv) = (u1 - u0, v1 - v0);
    let uv = match rng.below(5) {
        0 => [
            u0 + rng.uniform(0.0, 1.0) * du,
            v0 + rng.uniform(0.0, 1.0) * dv,
        ],
        1 => [
            u0 - rng.uniform(0.0, 0.5) * du,
            v0 + rng.uniform(0.0, 1.0) * dv,
        ],
        2 => [
            u1 + rng.uniform(0.0, 0.5) * du,
            v1 + rng.uniform(0.0, 0.5) * dv,
        ],
        3 => [-1e300, v0],
        _ => [
            u0 + rng.uniform(-1.0, 2.0) * du,
            v0 + rng.uniform(-1.0, 2.0) * dv,
        ],
    };
    Inputs::SeamOut {
        surface,
        uv,
        tol: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-2]).unwrap_or(&1e-6),
        closure: rng.below(5) as i64,
    }
}

fn gen_closure(rng: &mut Rng) -> Inputs {
    let surface = gen_seam_surface(rng);
    let [u0, v0, u1, v1] = domain(&surface);
    let dom = if rng.chance(0.7) {
        [u0, v0, u1, v1]
    } else {
        let a = rng.uniform(0.0, 0.4);
        let b = rng.uniform(0.6, 1.0);
        [
            u0 + a * (u1 - u0),
            v0 + a * (v1 - v0),
            u0 + b * (u1 - u0),
            v0 + b * (v1 - v0),
        ]
    };
    Inputs::Closure { surface, dom }
}

pub(super) fn gen_topo_brep(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.8) {
        BrepIn::File {
            file: (*rng.choose(&CORNER_FILES).unwrap_or(&"Fillet/box1x1x2.smb")).to_string(),
            rebuild: false,
        }
    } else {
        gen_brep(rng)
    }
}

/// The fillet test files with concave corners (most vertices of the others are convex,
/// where the concavity test is `false`).
const CONCAVE_FILES: [&str; 5] = [
    "Fillet/l_shaped_box.smb",
    "Fillet/box_w_slot.smb",
    "Fillet/box_on_box.smb",
    "Fillet/6_sided_corner.smb",
    "Fillet/corner_box.smb",
];

fn concave_brep(rng: &mut Rng) -> BrepIn {
    BrepIn::File {
        file: (*rng
            .choose(&CONCAVE_FILES)
            .unwrap_or(&"Fillet/l_shaped_box.smb"))
        .to_string(),
        rebuild: false,
    }
}

fn gen_topology(rng: &mut Rng) -> Inputs {
    let brep = gen_topo_brep(rng);
    let s = |rng: &mut Rng| rng.range_i64(0, 1 << 20);
    let none_or = |rng: &mut Rng, p: f64| {
        if rng.chance(p) {
            -1
        } else {
            rng.range_i64(0, 1 << 20)
        }
    };
    match rng.below(5) {
        4 => Inputs::Nx1 {
            // a single-face B-rep now and then: both side edge uses lie on that face,
            // so their face use normals are parallel and the corner is degenerate
            brep: if rng.chance(0.3) {
                gen_brep(rng)
            } else {
                gen_topo_brep(rng)
            },
            v: rng.below(1 << 20) as i64,
            eu: rng.below(1 << 20) as i64,
        },
        0 => Inputs::G1 {
            brep,
            v: none_or(rng, 0.05),
            e1: none_or(rng, 0.05),
            e2: none_or(rng, 0.05),
            tol: *rng.choose(&[0.5, 1.0, 5.0, 45.0, 90.0]).unwrap_or(&1.0),
        },
        1 => Inputs::FaceByTwo {
            brep,
            v: s(rng),
            eu1: none_or(rng, 0.05),
            eu2: none_or(rng, 0.05),
            eu3: none_or(rng, 0.6),
        },
        2 => Inputs::Tangential {
            brep,
            eu: s(rng),
            tol: *rng.choose(&[0.1, 2.0, 5.0, 45.0]).unwrap_or(&2.0),
        },
        _ => Inputs::Concavity {
            brep: if rng.chance(0.7) {
                concave_brep(rng)
            } else {
                brep
            },
            v: s(rng),
            eu1: s(rng),
            eu2: s(rng),
            ref_eu: none_or(rng, 0.8),
            adjacent: rng.chance(0.7),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_corner_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = match rng.below(20) {
            0 | 1 => gen_planar(rng),
            2..=4 => gen_revolution(rng),
            5 => gen_fillet_radius(rng),
            6 => gen_seam_out(rng),
            7 => gen_closure(rng),
            8..=11 => gen_topology(rng),
            16 | 17 => gen_face_corner(rng),
            18 | 19 => gen_const_rad_corner(rng),
            _ => gen_corner(rng),
        };
        serde_json::to_value(i).ok()
    }
}

// ---- the SmFilletCorner methods on the pseudo-B-rep model --------------------------------

/// "Upstream would dereference null" (or free an object twice): the shim refused.
const CORNER_REFUSED: f64 = -2.0;
/// The shim did not run upstream (the Rust side defers).
const CORNER_NOT_RUN: f64 = -3.0;

fn push_eus(o: &mut Vec<f64>, l: Option<&Vec<EuSel>>) {
    o.push(flag(l.is_some()));
    let l = l.map_or(&[][..], Vec::as_slice);
    o.push(l.len() as f64);
    for [e, w] in l {
        o.extend([*e as f64, *w as f64]);
    }
}

/// The corner-method record of an output: `kind`, `model`, `corner`, the method's input.
struct CornerCase<'a> {
    method: CornerMethod,
    model: &'a Model,
    corner: &'a CornerIn,
    peek: &'a PeekIn,
    solvers: &'a [SolverRegIn],
    query: usize,
    adds: &'a [ExtAddIn],
    queries: &'a [usize],
    squeeze: usize,
    curve: Option<&'a Crv>,
    topo: &'a [usize],
    surfaces: &'a [Srf],
    fsgs: &'a [FsgIn],
}

fn corner_case(i: &Inputs) -> Result<CornerCase<'_>, HarnessError> {
    let Inputs::Corner {
        method,
        model,
        corner,
        peek,
        solvers,
        query,
        adds,
        queries,
        squeeze,
        curve,
        topo,
        surfaces,
        fsgs,
    } = i
    else {
        return Err(harness("not a corner case"));
    };
    Ok(CornerCase {
        method: *method,
        model: model.as_ref(),
        corner: corner.as_ref(),
        peek,
        solvers,
        query: *query,
        adds,
        queries,
        squeeze: *squeeze,
        curve: curve.as_ref(),
        topo,
        surfaces,
        fsgs,
    })
}

fn corner_input(c: &CornerCase<'_>) -> Result<Vec<f64>, HarnessError> {
    let ci = c.corner;
    let mut o = Vec::new();
    push_model(&mut o, c.model)?;
    o.extend([ci.kind as f64, ci.v_edge as f64, ci.v_sub as f64]);
    push_eus(&mut o, ci.solver.as_ref());
    push_eus(&mut o, ci.all.as_ref());
    o.push(ci.verts.len() as f64);
    o.extend(ci.verts.iter().map(|&v| v as f64));
    o.push(ci.edges.len() as f64);
    o.extend(ci.edges.iter().map(|&e| e as f64));
    o.extend([
        flag(ci.bevel),
        flag(ci.blending),
        flag(ci.chamfer),
        flag(ci.degenerate),
        flag(ci.ext),
        flag(ci.adjusted),
    ]);
    o.extend([ci.setback, ci.tol, ci.tan]);
    o.push(ci.orig_faces.len() as f64);
    for [e, f] in &ci.orig_faces {
        o.extend([*e as f64, *f as f64]);
    }
    match c.method {
        CornerMethod::Peek => {
            let p = c.peek;
            o.extend([
                p.set_bevel as f64,
                p.set_const_rad as f64,
                p.set_chamfer as f64,
                p.add_vertex as f64,
                p.set_g1 as f64,
                p.set_status as f64,
                flag(p.prefill),
                p.kinds.len() as f64,
            ]);
            o.extend(p.kinds.iter().copied());
        }
        CornerMethod::Solvers => {
            o.push(c.solvers.len() as f64);
            for r in c.solvers {
                o.extend([r.e as f64, r.which as f64, r.approx, r.tang]);
            }
            o.push(c.query as f64);
        }
        CornerMethod::ExtSurface => {
            o.push(c.adds.len() as f64);
            for a in c.adds {
                o.extend([a.face as f64, a.s]);
            }
            o.push(c.queries.len() as f64);
            o.extend(c.queries.iter().map(|&f| f as f64));
        }
        CornerMethod::Reinit
        | CornerMethod::VertGeom
        | CornerMethod::EdgeGeom
        | CornerMethod::EndPatch => {}
        CornerMethod::Squeeze => o.push(c.squeeze as f64),
        CornerMethod::RailRail => {
            let crv = c.curve.ok_or_else(|| harness("rail_rail: no curve"))?;
            super::blend_fillet_geom_p1::push_crv(&mut o, crv);
        }
        CornerMethod::FixDegeneracy => {
            o.push(c.topo.len() as f64);
            o.extend(c.topo.iter().map(|&e| e as f64));
        }
        CornerMethod::MakeFace | CornerMethod::Insert => {
            o.push(c.surfaces.len() as f64);
            for srf in c.surfaces {
                super::blend_fillet_geom_p1::push_srf(&mut o, srf)?;
            }
        }
        CornerMethod::ConstRad => {
            o.push(c.fsgs.len() as f64);
            for f in c.fsgs {
                o.extend([f.kind as f64, f.scale, f64::from(f.continuity)]);
            }
        }
    }
    Ok(o)
}

/// Splits a degeneracy record: `ret`, the remaining topology edges, the corner members,
/// the model export.
fn degeneracy_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let get = |i: usize| {
        rec.get(i)
            .copied()
            .ok_or_else(|| harness("record too short"))
    };
    let count = |i: usize| -> Result<usize, HarnessError> {
        usize::try_from(get(i)? as i64).map_err(|_| harness("count"))
    };
    let ret = get(0)?;
    let nt = count(1)?;
    let topo = rec
        .get(2..2 + nt)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    let rest = rec
        .get(2 + nt..)
        .ok_or_else(|| harness("record too short"))?;
    Ok(squeeze_output_rest(rest)?
        .values("ret", vec![ret])
        .values("topo", topo))
}

/// The corner members and the model export of a record (after its leading values).
fn squeeze_output_rest(rec: &[f64]) -> Result<Output, HarnessError> {
    let get = |i: usize| {
        rec.get(i)
            .copied()
            .ok_or_else(|| harness("record too short"))
    };
    let count = |i: usize| -> Result<usize, HarnessError> {
        usize::try_from(get(i)? as i64).map_err(|_| harness("count"))
    };
    let mut at = 0;
    let nv = count(at)?;
    at += 1 + nv;
    let ne = count(at)?;
    at += 1 + ne;
    at += 7 + 2; // the flags, the setback and the surface count
    let corner = rec
        .get(..at)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    let model = rec
        .get(at..)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    Ok(Output::new()
        .values("corner", corner)
        .values("model", model))
}

/// Splits an insertion record: `status`, the executive topology intersector's
/// intersection table ([`put_ssi_table`]), the corner members, the model export.
fn insert_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let st = *rec.first().ok_or_else(|| harness("record too short"))?;
    let n = rec
        .get(1)
        .and_then(|&n| usize::try_from(n as i64).ok())
        .ok_or_else(|| harness("ssi table count"))?;
    let end = n
        .checked_mul(3)
        .and_then(|l| l.checked_add(2))
        .ok_or_else(|| harness("ssi table count"))?;
    let table = rec
        .get(1..end)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    let mut out = vec![st];
    out.extend(rec.get(end..).ok_or_else(|| harness("record too short"))?);
    Ok(squeeze_output(&out)?.values("ssi_table", table))
}

/// A surface handle of the standalone executive's topology intersector table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TiSurface {
    /// The surface of a target-B-rep face (`pFace->GetSurface()`).
    Target(solidcore::topology::FaceId),
    /// The surface of a fillet-B-rep face (`pFEFace->GetSurface()`).
    Fillet(solidcore::topology::FaceId),
    /// The corner's `m_vSurfaces[jj]`.
    Corner(usize),
}

impl ClassObject for TiSurface {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Surface
    }
}

impl SolutionObject for TiSurface {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

/// The executive the shim's `c1_insert` gives the corner: a bare `SmFilletExecutive`
/// (its constructor's topology intersector and edge attribute, no `CreateFilletBrep`)
/// with the Model's fillet B-rep as `m_pFilletBrep`. Its intersector is a real
/// [`TopologyIntersector`] whose table `RegisterExistingSSI` fills; the merge
/// (`MergeCurveOnSurfaces`, `UpdateTopologyChanges`) needs an edge-attribute user,
/// which the bare executive has none of (as upstream), so reaching it is a harness
/// error.
struct StandaloneCornerInsertion<'a> {
    fillet: &'a Brep,
    attribute_e_users: &'a [EdgeId],
    corner_surfaces: &'a [Surface],
    ti: TopologyIntersector<TiSurface>,
    merge_reached: bool,
}

impl StandaloneCornerInsertion<'_> {
    fn target_surface(target: &Brep, face: solidcore::topology::FaceId) -> Result<Option<TiSurface>, SolidError> {
        Ok(target.face(face)?.surface.is_some().then_some(TiSurface::Target(face)))
    }
}

impl corner::CornerInsertionContext for StandaloneCornerInsertion<'_> {
    type CurveBinding = ();
    type SurfaceBinding = TiSurface;
    type FaceMarker = ();
    fn begin_corner(&mut self) -> Result<(), SolidError> {
        Ok(())
    }
    fn capture_managed_curve(&mut self, edge: EdgeId) -> Result<Option<()>, SolidError> {
        Ok(self.fillet.edge(edge)?.curve.as_ref().map(|_| ()))
    }
    fn capture_pseudo_curve(
        &mut self,
        pseudo: &mut solidcore::blend::fillet_geom::p1::FilletBrep,
        edge: EdgeId,
    ) -> Result<Option<()>, SolidError> {
        Ok(pseudo.brep().edge(edge)?.curve.as_ref().map(|_| ()))
    }
    fn attribute_edges(&self) -> Result<Vec<EdgeId>, SolidError> {
        Ok(self.attribute_e_users.to_vec())
    }
    fn managed_edge_faces(&self, e: EdgeId) -> Result<Vec<solidcore::topology::FaceId>, SolidError> {
        edge::faces(self.fillet, e, None)
    }
    fn with_corner_face_scope<T>(
        &mut self,
        target: &mut Brep,
        _face: solidcore::topology::FaceId,
        body: impl FnOnce(&mut Self, &mut Brep, &()) -> Result<T, SolidError>,
    ) -> Result<T, SolidError> {
        self.merge_reached = true;
        body(self, target, &())
    }
    fn merge_corner_curve(
        &mut self,
        _pseudo: &mut solidcore::blend::fillet_geom::p1::FilletBrep,
        _target: &mut Brep,
        _face: solidcore::topology::FaceId,
        _curve: &Option<()>,
        _managed_edge: EdgeId,
    ) -> Result<(), SolidError> {
        self.merge_reached = true;
        Err(SolidError::invalid_input(
            "difftest::blend_fillet_corner_p1",
            "standalone executive has no merge backend",
        ))
    }
    fn update_corner_topology(
        &mut self,
        _pseudo: &mut solidcore::blend::fillet_geom::p1::FilletBrep,
        _target: &mut Brep,
        _marker: &(),
        _face: solidcore::topology::FaceId,
    ) -> Result<(), SolidError> {
        self.merge_reached = true;
        Err(SolidError::invalid_input(
            "difftest::blend_fillet_corner_p1",
            "standalone executive has no topology-change bookkeeping",
        ))
    }
    fn capture_managed_surface(
        &mut self,
        face: solidcore::topology::FaceId,
    ) -> Result<Option<TiSurface>, SolidError> {
        Ok(self.fillet.face(face)?.surface.is_some().then_some(TiSurface::Fillet(face)))
    }
    fn register_bound_intersection(
        &mut self,
        target: &mut Brep,
        face: solidcore::topology::FaceId,
        surface: &Option<TiSurface>,
    ) -> Result<(), SolidError> {
        let brep = Self::target_surface(target, face)?;
        self.ti.register_empty_ssi(brep, *surface);
        Ok(())
    }
    fn register_blending_intersection(
        &mut self,
        target: &mut Brep,
        face: solidcore::topology::FaceId,
        surface: &Surface,
        _surface_face: Option<solidcore::topology::FaceId>,
    ) -> Result<(), SolidError> {
        // m_vSurfaces[jj]: the corner surface by identity
        let jj = self
            .corner_surfaces
            .iter()
            .position(|s| std::ptr::eq(s, surface))
            .ok_or_else(|| {
                SolidError::invalid_input("difftest::blend_fillet_corner_p1", "foreign corner surface")
            })?;
        let brep = Self::target_surface(target, face)?;
        self.ti.register_empty_ssi(brep, Some(TiSurface::Corner(jj)));
        Ok(())
    }
}

/// The intersection table as the shim's `c1_put_ssi_table` writes it: `n`, then per
/// entry the brep surface (index of its face in `vertex_faces`, `-1` for `NULL`, `-2`
/// otherwise), the other surface (`jj` of a corner surface, `-1` for `NULL`, `-2`
/// otherwise) and whether the entry has curve arrays.
fn put_ssi_table(
    rec: &mut Vec<f64>,
    table: Option<&solidcore::boolean::topology_intersector::p1::IntersectionTable<TiSurface>>,
    vertex_faces: &[solidcore::topology::FaceId],
) {
    let Some(t) = table else {
        rec.push(0.0);
        return;
    };
    rec.push(t.brep_surfaces.len() as f64);
    for (ii, (b, o)) in t.brep_surfaces.iter().zip(&t.other_surfaces).enumerate() {
        rec.push(match b {
            None => -1.0,
            Some(TiSurface::Target(f)) => vertex_faces
                .iter()
                .position(|v| v == f)
                .map_or(-2.0, |i| i as f64),
            Some(_) => -2.0,
        });
        rec.push(match o {
            None => -1.0,
            Some(TiSurface::Corner(jj)) => *jj as f64,
            Some(_) => -2.0,
        });
        rec.push(match t.results.get(ii) {
            Some(r) if r.curves.is_some() => 1.0,
            _ => 0.0,
        });
    }
}

/// Splits a squeeze record: `status`, the corner members, the model export.
fn squeeze_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let get = |i: usize| {
        rec.get(i)
            .copied()
            .ok_or_else(|| harness("record too short"))
    };
    let count = |i: usize| -> Result<usize, HarnessError> {
        usize::try_from(get(i)? as i64).map_err(|_| harness("count"))
    };
    let st = get(0)?;
    let mut at = 1;
    let nv = count(at)?;
    at += 1 + nv;
    let ne = count(at)?;
    at += 1 + ne;
    at += 7 + 2; // the flags, the setback and the surface count
    let corner = rec
        .get(1..at)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    let model = rec
        .get(at..)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    Ok(Output::new()
        .values("status", vec![st])
        .values("corner", corner)
        .values("model", model))
}

/// Splits a constant-radius record: `status has [surface]`, the corner members, the
/// model export.
fn const_rad_output(rec: &[f64]) -> Result<Output, HarnessError> {
    let get = |i: usize| {
        rec.get(i)
            .copied()
            .ok_or_else(|| harness("record too short"))
    };
    let count = |i: usize| -> Result<usize, HarnessError> {
        usize::try_from(get(i)? as i64).map_err(|_| harness("count"))
    };
    let st = get(0)?;
    let mut at = 2;
    if get(1)? != 0.0 {
        // the surface: `kind has n m p q r s cpts u v`
        at += 1;
        if get(at)? == 0.0 {
            at += 1;
        } else {
            let (n, m, r, s) = (count(at + 1)?, count(at + 2)?, count(at + 5)?, count(at + 6)?);
            at += 7 + 4 * (n + 1) * (m + 1) + (r + 1) + (s + 1);
        }
    }
    let surface = rec
        .get(1..at)
        .ok_or_else(|| harness("record too short"))?
        .to_vec();
    let mut out = vec![st];
    out.extend(rec.get(at..).ok_or_else(|| harness("record too short"))?);
    Ok(squeeze_output(&out)?.values("surface", surface))
}

fn corner_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let c = corner_case(i)?;
    let input = corner_input(&c)?;
    let out = ffi::corner(c.method.ffi(), &input).map_err(|e| harness(format!("c++: {e}")))?;
    let Some((&head, rest)) = out.split_first() else {
        return Err(harness("empty corner output"));
    };
    if head == CORNER_REFUSED {
        return Ok(Output::err("refused"));
    }
    if head == CORNER_NOT_RUN {
        return Ok(Output::err("not_run_by_cpp"));
    }
    if head != 0.0 {
        return Err(harness(format!("unexpected corner record head {head}")));
    }
    Ok(match c.method {
        CornerMethod::Peek => Output::new().values("peek", rest.to_vec()),
        CornerMethod::Solvers => Output::new().values("solvers", rest.to_vec()),
        CornerMethod::ExtSurface => Output::new()
            .int("n_domains", rest.len() as i64)
            .values("domains", rest.to_vec()),
        CornerMethod::Reinit => Output::new().values("model", rest.to_vec()),
        CornerMethod::Squeeze
        | CornerMethod::VertGeom
        | CornerMethod::EdgeGeom
        | CornerMethod::EndPatch
        | CornerMethod::MakeFace => squeeze_output(rest)?,
        CornerMethod::Insert => insert_output(rest)?,
        CornerMethod::RailRail => Output::new().values("ret", rest.to_vec()),
        CornerMethod::FixDegeneracy => degeneracy_output(rest)?,
        CornerMethod::ConstRad => const_rad_output(rest)?,
    })
}

/// A Rust error of a corner method: deferred for `Unported`, `refused` for a null
/// dereference, else the raw status (what upstream returns, through `SER`).
fn corner_err(e: &SolidError) -> Result<f64, Box<Output>> {
    if let Some(o) = Output::unported(e) {
        return Err(Box::new(o));
    }
    if matches!(e, SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. } if *reason == NULL_DEREF) {
        return Err(Box::new(Output::err("refused")));
    }
    Ok(model_status(e))
}

fn corner_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let c = corner_case(i)?;
    let ci = c.corner;
    let h = |e: SolidError| harness(e.to_string());
    let mut rm = RustModel::build(c.model)?;
    let kind = *KINDS.get(ci.kind).ok_or_else(|| harness("kind"))?;
    let target_eu = |rm: &RustModel, [e, w]: EuSel| -> Result<EdgeuseId, HarnessError> {
        let edge = *rm
            .target_edges
            .get(e)
            .ok_or_else(|| harness("target edge"))?;
        let prim = eu1::primary_edgeuse(&rm.target, edge)
            .map_err(h)?
            .ok_or_else(|| harness("primary edge use"))?;
        if w == 1 {
            eu1::mate(&rm.target, prim)
                .map_err(h)?
                .ok_or_else(|| harness("mate"))
        } else {
            Ok(prim)
        }
    };
    let list =
        |rm: &RustModel, l: Option<&Vec<EuSel>>| -> Result<Option<Vec<EdgeuseId>>, HarnessError> {
            l.map(|l| l.iter().map(|&s| target_eu(rm, s)).collect())
                .transpose()
        };
    let vertex_of =
        |rm: &RustModel, v_edge: i64, v_sub: usize| -> Result<Option<VertexId>, HarnessError> {
            if v_edge <= -2 {
                let f = usize::try_from(-(v_edge + 2)).map_err(|_| harness("target face"))?;
                let face = *rm
                    .target_faces
                    .get(f)
                    .ok_or_else(|| harness("target face"))?;
                let vs = solidcore::topology::face::p4::vertices(&rm.target, face, None, false, None)
                    .map_err(h)?;
                return Ok(Some(
                    *vs.get(v_sub).ok_or_else(|| harness("target face vertex"))?,
                ));
            }
            match usize::try_from(v_edge) {
                Ok(e) => {
                    let edge = *rm
                        .target_edges
                        .get(e)
                        .ok_or_else(|| harness("target edge"))?;
                    let [vs, ve] = rm.target.edge(edge).map_err(h)?.vertices;
                    Ok(Some(if v_sub == 0 { vs } else { ve }))
                }
                Err(_) => Ok(None),
            }
        };
    let vertex = vertex_of(&rm, ci.v_edge, ci.v_sub)?;
    let solver_eus = list(&rm, ci.solver.as_ref())?;
    let all_eus = list(&rm, ci.all.as_ref())?;
    let mut fc = FilletCorner::new(kind, vertex, solver_eus, all_eus, ci.tol, ci.tan);
    for &v in &ci.verts {
        fc.vertices.push(rm.vertex(v)?);
    }
    for &e in &ci.edges {
        fc.edges.push(rm.edge(e)?);
    }
    fc.bevel = ci.bevel;
    fc.blending = ci.blending;
    fc.chamfer = ci.chamfer;
    fc.degenerate = ci.degenerate;
    fc.extended_surface_patch = ci.ext;
    fc.topology_adjusted = ci.adjusted;
    fc.set_back_dist = ci.setback;
    for &[e, f] in &ci.orig_faces {
        let eid = rm.edge(e)?;
        let face = *rm.target_faces.get(f).ok_or_else(|| harness("face"))?;
        rm.fb
            .fillet_edge_mut(eid)
            .map_err(h)?
            .set_original_face(Some(face));
    }
    // the corner record (as `blend.fillet_corner_p3`'s) after a geometry step
    let corner_rec =
        |fc: &FilletCorner, rm: &RustModel, rec: &mut Vec<f64>| -> Result<(), HarnessError> {
            rec.push(fc.vertices.len() as f64);
            rec.extend(fc.vertices.iter().map(|&v| rm.vertex_index(Some(v))));
            rec.push(fc.edges.len() as f64);
            rec.extend(fc.edges.iter().map(|&e| rm.edge_index(Some(e))));
            rec.extend([
                flag(fc.bevel),
                flag(fc.blending),
                flag(fc.chamfer),
                flag(fc.degenerate),
                flag(fc.calc_as_const_rad),
                flag(fc.topology_adjusted),
                flag(fc.extended_surface_patch),
            ]);
            rec.push(fc.set_back_dist);
            rec.push(fc.surfaces.len() as f64);
            rm.put(rec)
        };

    match c.method {
        CornerMethod::Peek => {
            let p = c.peek;
            if p.set_bevel >= 0 {
                fc.set_bevel(p.set_bevel != 0);
            }
            if p.set_const_rad >= 0 {
                fc.set_calc_as_const_rad(p.set_const_rad != 0);
            }
            if p.set_chamfer >= 0 && (ci.kind == 9 || ci.kind == 10) {
                fc.set_chamfer(p.set_chamfer != 0);
            }
            if let Ok(v) = usize::try_from(p.add_vertex) {
                fc.add_fillet_vertex(rm.vertex(v)?);
            }
            if p.set_g1 >= 0 && (ci.kind == 2 || ci.kind == 7) {
                fc.g1_case = p.set_g1 != 0;
            }
            if let Some(st) = FilStatus::from_code(p.set_status) {
                fc.status = st;
            }
            let mut rec = vec![fc.this_approx_tol_3d()];
            let mut edges = Vec::new();
            if p.prefill {
                edges.extend(rm.edge(0).ok());
            }
            fc.fillet_edges(&mut edges);
            rec.push(edges.len() as f64);
            rec.extend(edges.iter().map(|&e| rm.edge_index(Some(e))));
            let mut verts = Vec::new();
            if p.prefill {
                verts.extend(rm.vertex(0).ok());
            }
            fc.fillet_vertices(&mut verts);
            rec.push(verts.len() as f64);
            rec.extend(verts.iter().map(|&v| rm.vertex_index(Some(v))));
            // the filleted vertex as (target edge, 0 start / 1 end)
            let mut found = [-1.0, -1.0];
            if let Some(v) = fc.filleted_vertex() {
                for (i, &e) in rm.target_edges.iter().enumerate() {
                    let [vs, ve] = rm.target.edge(e).map_err(h)?.vertices;
                    if vs == v {
                        found = [i as f64, 0.0];
                        break;
                    }
                    if ve == v {
                        found = [i as f64, 1.0];
                        break;
                    }
                }
            }
            rec.extend(found);
            rec.push(fc.set_back_dist());
            rec.push(fc.status() as i64 as f64);
            rec.push(fc.type_code() as f64);
            rec.push(flag(fc.type_string() == corner::FILLET_CORNER_TYPE_STRING));
            for &t in &p.kinds {
                rec.push(flag(fc.is_kind_of(t as i64)));
            }
            rec.extend([
                flag(fc.is_bevel_corner()),
                flag(fc.is_blending_corner()),
                flag(fc.is_degenerate_corner()),
                flag(fc.is_extended_surface_patch()),
                flag(fc.is_calc_as_const_rad()),
                flag(fc.is_tangent_corner()),
            ]);
            Ok(Output::new().values("peek", rec))
        }
        CornerMethod::Solvers => {
            let mut exec = FilletExecutiveData::default();
            for (i, r) in c.solvers.iter().enumerate() {
                let eu = target_eu(&rm, [r.e, r.which])?;
                exec.edgeuse_solver_map.insert(eu, FilletSolverId(i as u32));
            }
            let fs = FilletSolverId(c.query as u32);
            let index = |s: Option<FilletSolverId>| s.map_or(-1.0, |s| f64::from(s.0));
            // the solver list first: both dereference m_pSolverEUs
            let all = match fc.fillet_solvers(&exec) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(corner_err(&e)
                        .err()
                        .map_or_else(|| Output::err("error"), |o| *o))
                }
            };
            let tangent = match fc.tangent_fillet_solver(&rm.k, &rm.target, &exec, fs) {
                Ok(t) => t,
                Err(e) => {
                    return Ok(corner_err(&e)
                        .err()
                        .map_or_else(|| Output::err("error"), |o| *o))
                }
            };
            let mut rec = vec![index(tangent), all.len() as f64];
            rec.extend(all.into_iter().map(index));
            Ok(Output::new().values("solvers", rec))
        }
        CornerMethod::ExtSurface => {
            let mut exec = FilletExecutiveData::default();
            for a in c.adds {
                let face = *rm.target_faces.get(a.face).ok_or_else(|| harness("face"))?;
                let dom = Extent2d::from_bounds(0.0, 0.0, a.s, a.s);
                let plane = Plane::new(
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec2::new(1.0, 1.0),
                    &dom,
                );
                fc.add_extended_original_surface(&mut exec, face, Surface::from(plane));
            }
            let mut rec = Vec::new();
            for &q in c.queries {
                let face = *rm.target_faces.get(q).ok_or_else(|| harness("face"))?;
                match fc.extended_surface(&exec, face) {
                    Some(s) => {
                        // (runner helper without a kernel in scope)
                        let d = s.natural_uv_domain_if_built();
                        rec.extend([d.max().x, d.max().y]);
                    }
                    None => rec.extend([-1.0, -1.0]),
                }
            }
            Ok(Output::new()
                .int("n_domains", rec.len() as i64)
                .values("domains", rec))
        }
        CornerMethod::Reinit => {
            if let Err(e) = fc.re_initialize_corner_edges(&mut rm.fb) {
                return Ok(match corner_err(&e) {
                    Err(o) => *o,
                    Ok(st) => Output::err(format!("status_{st}")),
                });
            }
            let mut rec = Vec::new();
            rm.put(&mut rec)?;
            Ok(Output::new().values("model", rec))
        }
        CornerMethod::Squeeze => {
            let e = rm.edge(c.squeeze)?;
            // the shim's refusals (UB-212), following upstream's loop in order: a vertex
            // use whose edge was freed by an earlier iteration (a closed non-rail edge), and
            // a closed squeezed edge whose "surviving" vertex is the one deleted at the end;
            // an edge or vertex missing from the corner's lists stops the loop first with
            // SM_ERR, which both sides compare
            {
                let b = rm.fb.brep();
                let surviving = edge::vertex(b, e).map_err(h)?;
                let other = edge::other_vertex(b, e, surviving).map_err(h)?;
                if let (Some(mut surviving), Some(mut del)) = (surviving, other) {
                    let status = |v: VertexId| rm.fb.fillet_vertex(v).map(|r| r.status).ok();
                    if status(del) != Some(FilStatus::ToBeDeleted) {
                        std::mem::swap(&mut surviving, &mut del);
                    }
                    if status(del) == Some(FilStatus::ToBeDeleted) {
                        let mut deletes = true;
                        let mut freed: Vec<EdgeId> = Vec::new();
                        for vu in solidcore::topology::vertex::vertexuses(b, del).map_err(h)? {
                            let fe = eu1::vertexuse_edgeuse(b, vu)
                                .map_err(h)?
                                .and_then(|eu| eu1::edge(b, eu).ok().flatten());
                            let Some(fe) = fe else {
                                return Ok(Output::err("refused"));
                            };
                            if freed.contains(&fe) {
                                return Ok(Output::err("refused"));
                            }
                            let rail = rm
                                .fb
                                .fillet_edge(fe)
                                .is_ok_and(|r| r.edge_type == FilletEdgeType::Rail);
                            if rail {
                                continue;
                            }
                            if !fc.edges.contains(&fe) {
                                deletes = false;
                                break;
                            }
                            freed.push(fe);
                        }
                        if deletes && !fc.vertices.contains(&del) {
                            deletes = false;
                        }
                        if deletes && surviving == del {
                            return Ok(Output::err("refused"));
                        }
                    }
                }
            }
            let st = match fc.squeeze_edge(&mut rm.fb, e) {
                Ok(()) => 0.0,
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => st,
                },
            };
            let mut rec = vec![st];
            corner_rec(&fc, &rm, &mut rec)?;
            squeeze_output(&rec)
        }
        CornerMethod::VertGeom | CornerMethod::EdgeGeom => {
            let mut exec = FilletExecutiveData::default();
            let r = {
                let mut env = CornerEnv {
                    k: &mut rm.k,
                    target: &mut rm.target,
                    pseudo: &mut rm.fb,
                    exec: &mut exec,
                    corner: None,
                    solvers: &mut [],
                    fillet: None,
                };
                match c.method {
                    CornerMethod::VertGeom => fc.calc_corner_vert_geom(&mut env, &mut []),
                    _ => fc.calc_corner_edge_geom(&mut env),
                }
            };
            let st = match r {
                Ok(()) => 0.0,
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => st,
                },
            };
            let mut rec = vec![st];
            corner_rec(&fc, &rm, &mut rec)?;
            squeeze_output(&rec)
        }
        CornerMethod::Insert => {
            for srf in c.surfaces {
                fc.surfaces.push(super::blend_fillet_geom_p1::surface_of(srf)?);
            }
            // pFilletExec: a bare executive (no CreateFilletBrep, m_pAttributeE without
            // users) over the Model's fillet B-rep as m_pFilletBrep
            let exec = FilletExecutiveData::default();
            let mut context = StandaloneCornerInsertion {
                fillet: &rm.fillet,
                attribute_e_users: &exec.attribute_e_users,
                corner_surfaces: &fc.surfaces,
                ti: TopologyIntersector::new(None, None, EFF_ZERO_SQ, 30.0_f64.to_radians()),
                merge_reached: false,
            };
            let r = fc.insert_intersection_topology_with_context(
                &mut rm.fb,
                &mut rm.target,
                &mut context,
            );
            if context.merge_reached {
                return Err(harness(
                    "insert: an attribute user reached the merge, which the standalone \
                     executive has no backend for",
                ));
            }
            let st = match r {
                Ok(()) => 0.0,
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => st,
                },
            };
            let vertex_faces = match fc.vertex {
                Some(v) => solidcore::topology::vertex::faces(&rm.target, v).map_err(h)?,
                None => Vec::new(),
            };
            let mut rec = vec![st];
            put_ssi_table(&mut rec, context.ti.intersection_table(), &vertex_faces);
            corner_rec(&fc, &rm, &mut rec)?;
            insert_output(&rec)
        }
        CornerMethod::EndPatch | CornerMethod::MakeFace => {
            for srf in c.surfaces {
                fc.surfaces.push(super::blend_fillet_geom_p1::surface_of(srf)?);
            }
            let mut exec = FilletExecutiveData::default();
            // m_pFilletBrep: a new SmBrep for the face makers, the Model's fillet B-rep
            // (holding the corner edges' fillet-B-rep edges) for the insertion
            let mut new_brep = Brep::new_with_infinite_region().map_err(h)?;
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attributes = solidcore::geom::attribute::AttributeStore::default();
            let r = {
                let fillet_brep = &mut new_brep;
                let mut env = CornerEnv {
                    k: &mut rm.k,
                    target: &mut rm.target,
                    pseudo: &mut rm.fb,
                    exec: &mut exec,
                    corner: None,
                    solvers: &mut [],
                    fillet: Some(super::blend_fillet_corner_p3::fillet_env(
                        fillet_brep,
                        &mut records,
                        &mut attributes,
                    )),
                };
                match c.method {
                    CornerMethod::EndPatch => fc.create_end_patch(&mut env),
                    _ => fc.make_face_brep(&mut env),
                }
            };
            let st = match r {
                Ok(()) => 0.0,
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => st,
                },
            };
            let mut rec = vec![st];
            corner_rec(&fc, &rm, &mut rec)?;
            super::blend_fillet_corner_p3::put_fillet_brep(&mut rec, &new_brep)?;
            super::blend_fillet_corner_p3::put_error_info(&mut rec, &exec.error, fc.filleted_vertex());
            squeeze_output(&rec)
        }
        CornerMethod::ConstRad => {
            let mut solvers = Vec::new();
            for (g, f) in c.fsgs.iter().enumerate() {
                let gid = rm.geom(g)?;
                if f.kind < 0 {
                    rm.fb.geom_mut(gid).map_err(h)?.solver = None;
                    continue;
                }
                let mut fs = FilletSolver::new(1e-3, EFF_ZERO_RAD, 1e-2);
                fs.set_fillet_surface_generator(match f.kind {
                    0 => Some(FilletSurfaceGenerator::circular_default()),
                    1 => Some(FilletSurfaceGenerator::blend_curve(f.scale, f.continuity)),
                    2 => Some(FilletSurfaceGenerator::Linear),
                    3 => Some(FilletSurfaceGenerator::Base),
                    _ => None,
                });
                rm.fb.geom_mut(gid).map_err(h)?.solver =
                    Some(FilletSolverId(solvers.len() as u32));
                solvers.push(fs);
            }
            let mut exec = FilletExecutiveData::default();
            let r = {
                let mut env = CornerEnv {
                    k: &mut rm.k,
                    target: &mut rm.target,
                    pseudo: &mut rm.fb,
                    exec: &mut exec,
                    corner: None,
                    solvers: &mut solvers,
                    fillet: None,
                };
                fc.make_constant_radius_fillet(&mut env)
            };
            let (st, surface) = match r {
                Ok(srf) => (0.0, Some(srf)),
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => (st, None),
                },
            };
            let mut rec = vec![st, flag(surface.is_some())];
            if let Some(srf) = &surface {
                super::blend_fillet_corner_p3::put_corner_surface(&mut rec, Some(srf))?;
            }
            corner_rec(&fc, &rm, &mut rec)?;
            const_rad_output(&rec)
        }
        CornerMethod::FixDegeneracy => {
            let mut exec = FilletExecutiveData::default();
            for &e in c.topo {
                exec.topo_edges.push(Some(rm.edge(e)?));
            }
            let r = {
                let mut env = CornerEnv {
                    k: &mut rm.k,
                    target: &mut rm.target,
                    pseudo: &mut rm.fb,
                    exec: &mut exec,
                    corner: None,
                    solvers: &mut [],
                    fillet: None,
                };
                fc.check_and_fix_degeneracy(&mut env, &[])
            };
            let ret = match r {
                Ok(f) => flag(f),
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => st,
                },
            };
            let mut rec = vec![ret, exec.topo_edges.len() as f64];
            rec.extend(exec.topo_edges.iter().map(|&e| rm.edge_index(e)));
            corner_rec(&fc, &rm, &mut rec)?;
            degeneracy_output(&rec)
        }
        CornerMethod::RailRail => {
            let crv = c.curve.ok_or_else(|| harness("rail_rail: no curve"))?;
            let curve = Curve::from(model_curve_of(crv)?);
            // the raw `SmBoolean` return: the flag, or the status `SER` returns
            let ret = match fc.test_rail_rail_interpolation_curve(&rm.k, &rm.target, &curve) {
                Ok(f) => flag(f),
                Err(e) => match corner_err(&e) {
                    Err(o) => return Ok(*o),
                    Ok(st) => st,
                },
            };
            Ok(Output::new().values("ret", vec![ret]))
        }
    }
}

/// A corner case on a generated model: the method's inputs are biased towards the
/// paths that do something (a vertex to delete for the squeeze, registered solvers for
/// the lookups, a concave `NxN` corner with a vertex for the rail-rail test).
fn gen_corner(rng: &mut Rng) -> Inputs {
    let mut model = gen_model(rng);
    let nv = model.vertices.len();
    let ne = model.edges.len();
    let nt = model.targets.len();
    let nf = model.faces.len();
    let method = *rng
        .choose(&[
            CornerMethod::Peek,
            CornerMethod::Solvers,
            CornerMethod::Solvers,
            CornerMethod::ExtSurface,
            CornerMethod::Reinit,
            CornerMethod::Squeeze,
            CornerMethod::Squeeze,
            CornerMethod::Squeeze,
            CornerMethod::RailRail,
            CornerMethod::RailRail,
            CornerMethod::VertGeom,
            CornerMethod::EdgeGeom,
            CornerMethod::FixDegeneracy,
            CornerMethod::FixDegeneracy,
            CornerMethod::FixDegeneracy,
            CornerMethod::EndPatch,
            CornerMethod::EndPatch,
            CornerMethod::MakeFace,
            CornerMethod::Insert,
            CornerMethod::Insert,
            CornerMethod::ConstRad,
        ])
        .unwrap_or(&CornerMethod::Peek);
    let kind = if method == CornerMethod::RailRail {
        10
    } else {
        rng.below(11) as usize
    };
    let v_edge = if nt > 0 && (method == CornerMethod::RailRail || rng.chance(0.8)) {
        rng.below(nt as u64) as i64
    } else {
        -1
    };
    let v_sub = if v_edge >= 0 {
        rng.below(2) as usize
    } else {
        0
    };
    let eus = |rng: &mut Rng| -> Option<Vec<EuSel>> {
        if nt == 0 || rng.chance(0.15) {
            return None;
        }
        let n = rng.below(4) as usize;
        Some(
            (0..n)
                .map(|_| [rng.below(nt as u64) as usize, rng.below(2) as usize])
                .collect(),
        )
    };
    let solver = eus(rng);
    let all = eus(rng);
    let subset = |rng: &mut Rng, n: usize| -> Vec<usize> {
        let mut v: Vec<usize> = (0..n).filter(|_| rng.chance(0.7)).collect();
        if rng.chance(0.2) {
            v.reverse();
        }
        v
    };
    let mut verts = subset(rng, nv);
    let mut edges = subset(rng, ne);
    // The geometry steps run on an empty list here. Their per-vertex / per-edge
    // calculators (blend.fillet_geom_p2..p6) read corner, solver and executive data the
    // generated models do not build, and upstream dereferences it unchecked (CalcVertOnVert,
    // CalcSetBackVert, CalcRailIntExtendedEdgeuse, CalcFilletIntFillet, CalcMateGeom and
    // the edge calculators of SmFilletEdge::CalcCornerEdgeGeom all crashed on random
    // models); the fixtures compare them on hand-built corners.
    if method == CornerMethod::VertGeom {
        verts.clear();
    }
    if method == CornerMethod::EdgeGeom {
        edges.clear();
    }
    // the degeneracy fix: processed corner vertices at the filleted vertex
    let mut topo = Vec::new();
    if method == CornerMethod::FixDegeneracy {
        let p = usize::try_from(v_edge)
            .ok()
            .and_then(|e| model.targets.get(e))
            .and_then(|e| {
                if v_sub == 0 {
                    e.cpts.first()
                } else {
                    e.cpts.last()
                }
            })
            .map(|c| [c[0], c[1], c[2]]);
        if let Some(p) = p {
            if rng.chance(0.6) {
                for &v in &verts {
                    if let Some(rec) = model.vertices.get_mut(v) {
                        rec.status = 1; // SM_FIL_PROCESSED
                        if rng.chance(0.9) {
                            rec.point = p;
                        }
                    }
                }
            }
        }
        topo = (0..ne).filter(|_| rng.chance(0.5)).collect();
        // the deletions must not leave a mate slot pointing at an earlier-deleted corner
        // vertex: ~SmFilletVertex clears one slot of each mate per backlink, so simulate
        // the deletions in order and, at the first dangling slot, add a backlink to the
        // mate (a free slot) or drop the link, until the order is clean (the FFI refuses
        // such inputs)
        'repair: loop {
            let mut mates: Vec<[i64; 2]> = model.vertices.iter().map(|r| r.mates).collect();
            let mut freed: Vec<usize> = Vec::new();
            for &v in verts.iter().skip(1) {
                for slot in 0..2 {
                    let Some(m) = mates
                        .get(v)
                        .and_then(|s| s.get(slot))
                        .and_then(|&m| usize::try_from(m).ok())
                    else {
                        continue;
                    };
                    if freed.contains(&m) {
                        let backlink = model
                            .vertices
                            .get_mut(m)
                            .and_then(|rec| rec.mates.iter_mut().find(|x| **x < 0));
                        if let Some(free) = backlink {
                            *free = v as i64;
                        } else if let Some(link) = model
                            .vertices
                            .get_mut(v)
                            .and_then(|rec| rec.mates.get_mut(slot))
                        {
                            *link = -1;
                        }
                        continue 'repair;
                    }
                    if let Some(x) = mates
                        .get_mut(m)
                        .and_then(|back| back.iter_mut().find(|x| **x == v as i64))
                    {
                        *x = -1;
                    }
                }
                freed.push(v);
            }
            break;
        }
    }
    // the end patch: a fillet-side-face edge, with or without an original face
    let mut orig_faces: Vec<[usize; 2]> = Vec::new();
    if method == CornerMethod::EndPatch && ne > 0 && rng.chance(0.7) {
        let e = rng.below(ne as u64) as usize;
        if let Some(rec) = model.edges.get_mut(e) {
            rec.etype = 5; // SM_FE_FILLET_X_SIDE_FACE
        }
        if !edges.contains(&e) && rng.chance(0.8) {
            edges.push(e);
        }
        if nf > 0 && rng.chance(0.5) {
            orig_faces.push([e, rng.below(nf as u64) as usize]);
        }
    }
    if nf > 0 && ne > 0 && rng.chance(0.3) {
        orig_faces.push([rng.below(ne as u64) as usize, rng.below(nf as u64) as usize]);
    }
    let scale = *rng.choose(&[1.0, 1e-3, 1e3, 10.0, 0.1]).unwrap_or(&1.0);
    let tol = match rng.below(4) {
        0 => 1e-3 * scale,
        1 => 1e-6 * scale,
        2 => 0.0,
        _ => scale * 10f64.powf(rng.uniform(-9.0, -2.0)),
    };
    let tan = rng.uniform(1e-4, 0.2);

    // the squeeze: a vertex of the edge to be deleted, its edges in the corner
    let squeeze = rng.below(ne.max(1) as u64) as usize;
    if let (CornerMethod::Squeeze, Some(e)) = (method, model.edges.get(squeeze)) {
        let (vs, ve) = (e.vs, e.ve);
        if rng.chance(0.8) {
            let del = if rng.chance(0.5) { vs } else { ve };
            if let Some(v) = model.vertices.get_mut(del) {
                v.status = 9; // SM_FV_TO_BE_DELETED
            }
            if rng.chance(0.1) {
                if let Some(v) = model.vertices.get_mut(if del == vs { ve } else { vs }) {
                    v.status = 9;
                }
            }
            for (i, ed) in model.edges.iter_mut().enumerate() {
                if (ed.vs == del || ed.ve == del) && rng.chance(0.5) {
                    ed.etype = 9; // SM_FE_RAIL
                }
                if (ed.vs == del || ed.ve == del) && rng.chance(0.85) && !edges.contains(&i) {
                    edges.push(i);
                }
            }
            if rng.chance(0.9) && !verts.contains(&del) {
                verts.push(del);
            }
        }
    }
    let peek = PeekIn {
        set_bevel: rng.range_i64(-1, 1),
        set_const_rad: rng.range_i64(-1, 1),
        set_chamfer: if kind >= 9 { rng.range_i64(-1, 1) } else { -1 },
        add_vertex: if nv > 0 && rng.chance(0.5) {
            rng.below(nv as u64) as i64
        } else {
            -1
        },
        kinds: (0..rng.below(4))
            .map(|_| {
                *rng.choose(&[16036.0, 11001.0, 16033.0, 15002.0, 0.0])
                    .unwrap_or(&0.0)
            })
            .collect(),
        set_g1: if kind == 2 || kind == 7 {
            rng.range_i64(-1, 1)
        } else {
            -1
        },
        set_status: if rng.chance(0.5) {
            rng.range_i64(0, 12)
        } else {
            -1
        },
        prefill: rng.chance(0.4),
    };
    let solvers: Vec<SolverRegIn> = if nt > 0 {
        (0..rng.below(4))
            .map(|_| SolverRegIn {
                e: rng.below(nt as u64) as usize,
                which: rng.below(2) as usize,
                approx: tol,
                tang: tan,
            })
            .collect()
    } else {
        Vec::new()
    };
    let query = rng.below(solvers.len() as u64 + 1) as usize;
    let adds: Vec<ExtAddIn> = if nf > 0 {
        (0..rng.below(4))
            .map(|_| ExtAddIn {
                face: rng.below(nf as u64) as usize,
                s: *rng
                    .choose(&[0.5, 1.0, 1.0, 2.0, 3.0, 1.0 + 1e-13])
                    .unwrap_or(&1.0),
            })
            .collect()
    } else {
        Vec::new()
    };
    let queries: Vec<usize> = if nf > 0 {
        let mut q: Vec<usize> = (0..rng.below(3))
            .map(|_| rng.below(nf as u64) as usize)
            .collect();
        // an ExtSurface case with no query compares an empty record on both sides;
        // query face 0 instead (no extra rng draw, so the stream is unchanged)
        if q.is_empty() {
            q.push(0);
        }
        q
    } else {
        Vec::new()
    };
    let curve = if method == CornerMethod::RailRail {
        Some(model_gen_curve(rng, 3, scale))
    } else {
        None
    };
    let surfaces: Vec<Srf> = if matches!(method, CornerMethod::MakeFace | CornerMethod::Insert) {
        (0..rng.below(4))
            .map(|_| super::blend_fillet_geom_p1::gen_surface(rng, scale))
            .collect()
    } else {
        Vec::new()
    };
    let fsgs: Vec<FsgIn> = if method == CornerMethod::ConstRad {
        model.geoms.iter().map(|_| gen_fsg(rng)).collect()
    } else {
        Vec::new()
    };
    Inputs::Corner {
        method,
        model: Box::new(model),
        corner: Box::new(CornerIn {
            kind,
            v_edge,
            v_sub,
            solver,
            all,
            verts,
            edges,
            bevel: rng.chance(0.5),
            blending: rng.chance(0.3),
            chamfer: rng.chance(0.2),
            degenerate: rng.chance(0.2),
            ext: rng.chance(0.8),
            adjusted: rng.chance(0.2),
            setback: if rng.chance(0.5) {
                0.0
            } else {
                rng.uniform(0.0, scale)
            },
            tol,
            tan,
            orig_faces,
        }),
        peek,
        solvers,
        query,
        adds,
        queries,
        squeeze,
        curve,
        topo,
        surfaces,
        fsgs,
    }
}

/// A random solver generator for a `const_rad` case (mostly circular).
fn gen_fsg(rng: &mut Rng) -> FsgIn {
    let kind = *rng
        .choose(&[0, 0, 0, 1, 1, 2, 3, 4, -1])
        .unwrap_or(&0);
    FsgIn {
        kind,
        scale: *rng.choose(&[1.5, 1.5, 1.0, 2.0]).unwrap_or(&1.5),
        continuity: rng.below(3) as u32,
    }
}

/// A corner on a hand-built model for one of the face or fillet routines.
fn corner_on(method: CornerMethod, model: Model, corner: CornerIn) -> Inputs {
    Inputs::Corner {
        method,
        model: Box::new(model),
        corner: Box::new(corner),
        peek: PeekIn::default(),
        solvers: Vec::new(),
        query: 0,
        adds: Vec::new(),
        queries: Vec::new(),
        squeeze: 0,
        curve: None,
        topo: Vec::new(),
        surfaces: Vec::new(),
        fsgs: Vec::new(),
    }
}

/// The corner record of a hand-built corner over model vertices `verts` and edges
/// `edges`.
fn plain_corner(kind: usize, verts: Vec<usize>, edges: Vec<usize>, tol: f64) -> CornerIn {
    CornerIn {
        kind,
        v_edge: -1,
        v_sub: 0,
        solver: None,
        all: None,
        verts,
        edges,
        bevel: false,
        blending: false,
        chamfer: false,
        degenerate: false,
        ext: true,
        adjusted: false,
        setback: 0.0,
        tol,
        tan: 0.01,
        orig_faces: Vec::new(),
    }
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `MakeFaceBrep` on a planar polygon corner: three to five line edges chained on a
/// plane (either way round), the plane as the one surface; sometimes a vertex moved
/// off the plane or out of the loop, sometimes several B-spline surfaces (the
/// N-sided branch) or none.
fn gen_face_corner(rng: &mut Rng) -> Inputs {
    use super::blend_fillet_corner_p3::{edge_in, line, plane_srf, vin};
    let scale = *rng.choose(&[1.0, 1.0, 1e-3, 1e3, 0.1]).unwrap_or(&1.0);
    let o = rng.point3(-scale, scale);
    let a = rng.uniform(0.0, std::f64::consts::TAU);
    let tilt = rng.uniform(-0.5, 0.5);
    let u = [a.cos(), a.sin(), 0.0];
    let v = [-a.sin() * tilt.cos(), a.cos() * tilt.cos(), tilt.sin()];
    let n = 3 + rng.below(3) as usize;
    let ccw = rng.chance(0.5);
    let mut pts: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64 + rng.uniform(-0.2, 0.2);
            let t = if ccw { t } else { -t };
            let r = scale * rng.uniform(0.5, 1.0);
            add3(o, add3(scale3(u, r * t.cos()), scale3(v, r * t.sin())))
        })
        .collect();
    if rng.chance(0.1) {
        if let Some(p) = pts.get_mut(0) {
            let w = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            *p = add3(*p, scale3(w, scale * *rng.choose(&[1e-3, 0.3]).unwrap_or(&0.3)));
        }
    }
    let mut model = super::blend_fillet_geom_p4::empty_model();
    model.vertices = pts.iter().map(|&p| vin(p, 0)).collect();
    let etype = *rng.choose(&[1, 5, 9, 11]).unwrap_or(&1);
    model.edges = (0..n)
        .map(|i| {
            let (a, b) = (i, (i + 1) % n);
            let at = |k: usize| pts.get(k).copied().unwrap_or_default();
            edge_in(a, b, etype, Some(line(at(a), at(b))), [-1, -1])
        })
        .collect();
    let mut edges: Vec<usize> = (0..n).collect();
    if rng.chance(0.1) {
        edges.swap(0, 1);
    }
    let surfaces: Vec<Srf> = match rng.below(10) {
        0 => Vec::new(),
        1 | 2 => (0..2 + rng.below(2))
            .map(|_| super::blend_fillet_geom_p1::gen_surface(rng, scale))
            .collect(),
        _ => {
            let d = 4.0 * scale;
            vec![plane_srf(
                add3(o, scale3(add3(u, v), -2.0 * scale)),
                scale3(u, d),
                scale3(v, d),
            )]
        }
    };
    let tol = scale * *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-6);
    let kind = rng.below(11) as usize;
    let mut i = corner_on(
        CornerMethod::MakeFace,
        model,
        plain_corner(kind, (0..n).collect(), edges, tol),
    );
    if let Inputs::Corner { surfaces: s, .. } = &mut i {
        *s = surfaces;
    }
    i
}

/// A rational quarter circle from `c + r a` to `c + r b` (`a`, `b` orthonormal).
fn quarter_arc(c: [f64; 3], r: f64, a: [f64; 3], b: [f64; 3]) -> Crv {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let p0 = add3(c, scale3(a, r));
    let p1 = add3(c, scale3(add3(a, b), r));
    let p2 = add3(c, scale3(b, r));
    Crv {
        dim: 3,
        cpts: vec![
            [p0[0], p0[1], p0[2], 1.0],
            [p1[0] * w, p1[1] * w, p1[2] * w, w],
            [p2[0], p2[1], p2[2], 1.0],
        ],
        p: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

/// The quarter cylinder of radius `r` about the axis through `c` along `axis`, from
/// direction `a` to `b`, `len` either side of `c`.
fn quarter_cylinder(c: [f64; 3], r: f64, a: [f64; 3], b: [f64; 3], axis: [f64; 3], len: f64) -> Srf {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ring = [(a, 1.0), (add3(a, b), w), (b, 1.0)];
    let net = ring
        .iter()
        .map(|&(d, wt)| {
            [-len, len]
                .iter()
                .map(|&t| {
                    let p = add3(add3(c, scale3(d, r)), scale3(axis, t));
                    [p[0] * wt, p[1] * wt, p[2] * wt, wt]
                })
                .collect()
        })
        .collect();
    Srf {
        net,
        p: 2,
        q: 1,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// `MakeConstantRadiusFillet` on a spherical three-sided corner: three quarter-circle
/// cross sections (`A -> B -> C -> A` about a common centre, radii perturbed now and
/// then), the second and third carrying fillet geometries whose surfaces are the quarter
/// cylinders through them; solvers of random generators; sometimes an edge that is not
/// a cross section, a line instead of an arc, or a geometry without surface.
fn gen_const_rad_corner(rng: &mut Rng) -> Inputs {
    use super::blend_fillet_corner_p3::{edge_in, line, vin};
    use super::blend_fillet_geom_p1::GeomIn;
    let scale = *rng.choose(&[1.0, 1.0, 1e-2, 1e2]).unwrap_or(&1.0);
    let c = rng.point3(-scale, scale);
    let r = scale * rng.uniform(0.2, 1.0);
    let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let rr = |rng: &mut Rng| {
        if rng.chance(0.15) {
            r * rng.uniform(0.9, 1.1)
        } else {
            r
        }
    };
    let (ra, rb, rc) = (rr(rng), rr(rng), rr(rng));
    let pa = add3(c, scale3(x, r));
    let pb = add3(c, scale3(y, r));
    let pc = add3(c, scale3(z, r));
    let mut model = super::blend_fillet_geom_p4::empty_model();
    model.vertices = vec![vin(pa, 0), vin(pb, 0), vin(pc, 0)];
    let arcs = [
        quarter_arc(c, ra, x, y),
        quarter_arc(c, rb, y, z),
        quarter_arc(c, rc, z, x),
    ];
    // the fillet surfaces of the cross sections B -> C (about x) and C -> A (about y)
    let len = 2.0 * r;
    let mut geoms = vec![
        GeomIn {
            gtype: 0,
            status: 0,
            center: None,
            surface: Some(quarter_cylinder(c, r, y, z, x, len)),
            rails: [-1, -1],
            other1: Vec::new(),
            other2: Vec::new(),
            sides: Vec::new(),
        },
        GeomIn {
            gtype: 0,
            status: 0,
            center: None,
            surface: Some(quarter_cylinder(c, r, z, x, y, len)),
            rails: [-1, -1],
            other1: Vec::new(),
            other2: Vec::new(),
            sides: Vec::new(),
        },
    ];
    if rng.chance(0.05) {
        if let Some(g) = geoms.get_mut(0) {
            g.surface = None;
        }
    }
    model.geoms = geoms;
    let etype = |rng: &mut Rng| if rng.chance(0.05) { 9 } else { 1 };
    let ends = [(pa, pb, 0, 1), (pb, pc, 1, 2), (pc, pa, 2, 0)];
    let gs = [[-1, -1], [0, -1], [1, -1]];
    model.edges = arcs
        .iter()
        .zip(ends)
        .zip(gs)
        .map(|((arc, (p, q, a, b)), g)| {
            let crv = if rng.chance(0.05) {
                line(p, q)
            } else {
                arc.clone()
            };
            edge_in(a, b, etype(rng), Some(crv), g)
        })
        .collect();
    let mut edges = vec![0, 1, 2];
    edges.rotate_left(rng.below(3) as usize);
    if rng.chance(0.05) {
        edges.pop();
    }
    let tol = scale * *rng.choose(&[1e-3, 1e-5]).unwrap_or(&1e-3);
    let mut i = corner_on(
        CornerMethod::ConstRad,
        model,
        plain_corner(rng.below(11) as usize, vec![0, 1, 2], edges, tol),
    );
    let fsg = gen_fsg(rng);
    let other = if rng.chance(0.8) { fsg.clone() } else { gen_fsg(rng) };
    if let Inputs::Corner { fsgs, .. } = &mut i {
        *fsgs = vec![fsg, other];
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Prints the face / fillet corner fixtures (`fixtures/blend.fillet_corner_p1/faces.json`).
    #[test]
    #[ignore = "writes fixture JSON to stdout"]
    fn dump_face_fixtures() {
        let mut cases = Vec::new();
        for i in 0..40u64 {
            let mut rng = Rng::new(0xface_0000 + i);
            cases.push(json!({"name": format!("make_face_{i:02}"), "upstream_ref": "source/SMLib/src/SmFilletCorner.cpp:3687 (MakeFaceBrep)", "inputs": gen_face_corner(&mut rng)}));
        }
        for i in 0..40u64 {
            let mut rng = Rng::new(0xc0de_0000 + i);
            cases.push(json!({"name": format!("const_rad_{i:02}"), "upstream_ref": "source/SMLib/src/SmFilletCorner.cpp:3034 (MakeConstantRadiusFillet)", "inputs": gen_const_rad_corner(&mut rng)}));
        }
        let mut n = 0;
        let mut seed = 0x1e5e_0000u64;
        while n < 30 {
            seed += 1;
            let mut rng = Rng::new(seed);
            let i = gen_corner(&mut rng);
            let Inputs::Corner { method, .. } = &i else { continue };
            if matches!(method, CornerMethod::Insert | CornerMethod::EndPatch | CornerMethod::MakeFace) {
                cases.push(json!({"name": format!("corner_{method:?}_{n:02}").to_lowercase(), "upstream_ref": "source/SMLib/src/SmFilletCorner.cpp (CreateEndPatch, MakeFaceBrep, InsertIntersectionTopology on random models)", "inputs": i}));
                n += 1;
            }
        }
        println!("{}", serde_json::to_string_pretty(&json!({"cases": cases})).unwrap());
    }

    /// Prints the blending insertion fixtures
    /// (`fixtures/blend.fillet_corner_p1/insert_blending.json`): blending corners at a
    /// vertex of a target face with corner surfaces, so `InsertIntersectionTopology`
    /// registers each vertex face with each surface (`RegisterExistingSSI`).
    #[test]
    #[ignore = "writes fixture JSON to stdout"]
    fn dump_insert_blending_fixtures() {
        let mut cases = Vec::new();
        let mut seed = 0xb1e0_0000u64;
        while cases.len() < 24 {
            seed += 1;
            let mut rng = Rng::new(seed);
            let mut i = gen_corner(&mut rng);
            let Inputs::Corner { method, model, corner, surfaces, .. } = &mut i else { continue };
            if *method != CornerMethod::Insert || model.faces.is_empty() {
                continue;
            }
            corner.blending = true;
            corner.v_edge = -2 - rng.below(model.faces.len() as u64) as i64;
            corner.v_sub = rng.below(2) as usize;
            while surfaces.len() < 1 + rng.below(3) as usize {
                surfaces.push(super::super::blend_fillet_geom_p1::gen_surface(&mut rng, 1.0));
            }
            let n = cases.len();
            cases.push(json!({"name": format!("insert_blending_{n:02}"), "upstream_ref": "source/SMLib/src/SmFilletCorner.cpp:2353 (InsertIntersectionTopology: RegisterExistingSSI of each vertex face with each corner surface)", "inputs": i}));
        }
        println!("{}", serde_json::to_string_pretty(&json!({"cases": cases})).unwrap());
    }

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(11);
        for _ in 0..200 {
            let v = Runner.generate(&mut rng).unwrap();
            let back: Inputs =
                serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
            assert_eq!(serde_json::to_value(back).unwrap(), v);
        }
    }

    /// The op name of a case.
    fn op_name(case: &Case) -> String {
        let op = case.inputs.get("op").and_then(Value::as_str).unwrap_or("?");
        match case.inputs.get("method").and_then(Value::as_str) {
            Some(m) => format!("{op}:{m}"),
            None => op.to_string(),
        }
    }

    /// With `CASE_CENSUS`: the outcome of every case of the unit run (fixtures plus
    /// `CASE_SEEDS` generated, default 16) by op: `deferred`, `refused` / other errors,
    /// `error_status` (a non-zero `SmStatus`) or the result (`success`, and for the
    /// flag ops `true` / `false`, for `face_by_two` `face` / `no_face`, for `seam_out`
    /// `moved` / `kept`); with `CASE_LIST=<kind>` as well, prints the name and inputs
    /// of every case of that kind (to pin generated cases as fixtures). Without it: runs
    /// the cases `CASE_INDICES` (or the first `CASE_PER_OP` of each op) on one side
    /// (`CASE_SIDE=rust`, else C++), for valgrind.
    #[test]
    #[ignore = "census and valgrind sample; run on demand"]
    fn case_sample() {
        let env = |name: &str| std::env::var(name).ok();
        let ctx = crate::case::CaseContext {
            root: crate::case::repo_root(),
            seeds: env("CASE_SEEDS").map_or(16, |s| s.parse().unwrap()),
        };
        let cases = Runner.cases(&ctx).unwrap();
        if env("CASE_CENSUS").is_some() {
            let mut tally = BTreeMap::<(String, String), usize>::new();
            for case in &cases {
                let kind = match Runner.run_rust(case) {
                    Ok(o) if o.deferred.is_some() => "deferred".to_string(),
                    Ok(o) if o.error.is_some() => {
                        let e = o.error.unwrap_or_default();
                        e.split(|c: char| c == ':' || c.is_ascii_digit())
                            .next()
                            .unwrap_or("")
                            .to_string()
                    }
                    Ok(o) => {
                        let first = |k: &str| o.values.get(k).and_then(|v| v.first()).copied();
                        if first("status").is_some_and(|st| st != 0.0) {
                            "error_status".to_string()
                        } else if let Some(f) = first("flag") {
                            (if f == 0.0 { "false" } else { "true" }).to_string()
                        } else if let Some(f) = first("g1") {
                            (if f == 0.0 { "false" } else { "true" }).to_string()
                        } else if let Some(f) = first("ret") {
                            // the raw SmBoolean return: the flag, or a status
                            (if f == 0.0 {
                                "false"
                            } else if f == 1.0 {
                                "true"
                            } else {
                                "status"
                            })
                            .to_string()
                        } else if let Some(f) = first("face") {
                            (if f >= 0.0 { "face" } else { "no_face" }).to_string()
                        } else if let Some(f) = first("seam") {
                            (if f == 0.0 { "kept" } else { "moved" }).to_string()
                        } else {
                            "success".to_string()
                        }
                    }
                    Err(_) => "harness_error".to_string(),
                };
                if env("CASE_LIST").is_some_and(|k| k == kind) {
                    // the resolved arguments and, for a vertex op, the edge uses at the
                    // vertex (export order), so a seeded case can be pinned with small
                    // selectors
                    let mut resolved = String::new();
                    if let Ok(i) = serde_json::from_value::<Inputs>(case.inputs.clone()) {
                        if let Ok(Ok(t)) = topo(&i, true) {
                            let vid = t.args.first().and_then(|&v| u32::try_from(v).ok());
                            let ring: Vec<u32> = t
                                .brep
                                .edgeuses()
                                .iter()
                                .map(|(id, _)| id)
                                .filter(|&id| eu_vertex(&t.brep, id).map(VertexId::index) == vid)
                                .map(EdgeuseId::index)
                                .collect();
                            resolved = format!(
                                "args={:?} ring={ring:?} surfaces={:?} curves={:?}",
                                t.args, t.types.face_surface_types, t.types.edge_curve_types
                            );
                        }
                    }
                    eprintln!(
                        "case_item: {} {} {} {resolved}",
                        op_name(case),
                        case.name,
                        case.inputs
                    );
                }
                *tally.entry((op_name(case), kind)).or_default() += 1;
            }
            for ((op, kind), n) in tally {
                eprintln!("case_census: {op} {kind} {n}");
            }
            return;
        }
        let rust_side = env("CASE_SIDE").is_some_and(|s| s == "rust");
        let indices: Vec<usize> = if let Some(t) = env("CASE_INDICES") {
            t.split(',').map(|x| x.trim().parse().unwrap()).collect()
        } else {
            let per_op = env("CASE_PER_OP").map_or(10, |s| s.parse().unwrap());
            let mut taken = BTreeMap::<String, usize>::new();
            (0..cases.len())
                .filter(|&i| {
                    let n = taken.entry(op_name(&cases[i])).or_default();
                    *n += 1;
                    *n <= per_op
                })
                .collect()
        };
        for i in indices {
            let case = &cases[i];
            eprintln!("case_start: {i} {} {}", case.name, case.inputs);
            let r = if rust_side {
                Runner.run_rust(case).map(|o| o.error)
            } else {
                Runner.run_cpp(case).map(|o| o.error)
            };
            eprintln!("case_sample: {i} {} {r:?}", case.name);
        }
    }

    #[test]
    fn arc_is_exact() {
        let c = arc(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(2.0, 0.0, 1.0),
            1.0,
        );
        let p = c.cpts[1];
        // the middle control point lies on the bisector at r / cos(a/2)
        let (x, y) = (p[0] / p[3], p[1] / p[3]);
        assert!(((x * x + y * y).sqrt() - 2.0 / 0.5f64.cos()).abs() < 1e-12);
    }
}
