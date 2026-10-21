//! Fillet corners, part 3: corner classification ([`FilletCorner::create`]), the corner
//! constructors, the corner patch builders (default, 3x2 mixed-convexity, Nx1 and Nx2)
//! and the Nx2 corner's topology and tangent-corner adjustments.
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp`, `source/SMLib/inc/SmFilletCorner.h`.
//!
//! | Rust                                               | Upstream                                              |
//! |----------------------------------------------------|-------------------------------------------------------|
//! | [`FilletCorner`], [`FilletCornerKind`], [`FilletCorner::new`] | `SmFilletCorner` members and constructor, the derived classes and their constructors (CO-PORTED from `blend.fillet_corner_p1` / `_p2`, see below) |
//! | [`FilletCorner::new`] with [`FilletCornerKind::TwoByTwo`] | `SmFillet2x2Corner::SmFillet2x2Corner`          |
//! | [`FilletCorner::new`] with [`FilletCornerKind::ConcaveNByN`] | `SmFilletConcaveNxNCorner::SmFilletConcaveNxNCorner` |
//! | [`FilletCorner::new`] with [`FilletCornerKind::ConvexNByN`] | `SmFilletConvexNxNCorner::SmFilletConvexNxNCorner` |
//! | [`FilletCorner::new`] with [`FilletCornerKind::FourByThree`] | `SmFillet4x3Corner::SmFillet4x3Corner`        |
//! | [`FilletCorner::new`] with [`FilletCornerKind::Open`] | `SmFilletOpenCorner::SmFilletOpenCorner`           |
//! | [`FilletCorner::new`] with [`FilletCornerKind::Degenerate`] | `SmFilletDegenerateCorner::SmFilletDegenerateCorner` |
//! | [`FilletCorner::corner_type`], [`FilletCornerKind::name`] | `GetCornerType`, `GetName` (per derived class)   |
//! | [`FilletCorner::create`]                           | `SmFilletCorner::Create`                              |
//! | [`FilletCorner::calc_corner_geom`]                 | `SmFilletCorner::CalcCornerGeom`                      |
//! | [`FilletCorner::create_corner_patch`]              | `CreateCornerPatch` (virtual: `SmFilletCorner`, `SmFillet3x2MixedConvexityCorner`, `SmFilletNx1Corner`, `SmFilletNx2Corner`) |
//! | [`FilletCorner::adjust_topology`]                  | `AdjustTopology` (virtual; `SmFilletNx2Corner::AdjustTopology` here) |
//! | [`FilletCorner::adjust_tangent_corner`]            | `AdjustTangentCorner` (virtual; `SmFilletNx2Corner::AdjustTangentCorner` here) |
//! | [`FilletExecutiveData`]                            | the `SmFilletExecutive` members read here (`m_vEdgeuseSolverMap`, `GetFilletSolverOfEdgeuse`; CO-PORTED from `blend.fillet_executive_p1`) |
//!
//! Deviations:
//! - **Object model.** The class hierarchy is one [`FilletCorner`] record with a
//!   [`FilletCornerKind`]; the virtual methods dispatch on the kind. The pseudo-B-rep
//!   members (`m_vVertices`, `m_vEdges`) are ids of the executive's [`FilletBrep`];
//!   `m_cpVertex`, `m_pSolverEUs` and `m_pAllEUs` are ids of the B-rep being filleted.
//!   The executive and the fillet solvers are passed explicitly: the executive's
//!   edge-use-to-solver map as [`FilletExecutiveData`], the solvers as a slice indexed
//!   by [`FilletSolverId`]. A corner's own identity (`this`, stored by new fillet
//!   vertices and edges) is [`CornerEnv::corner`]. `m_crContext` is the [`Kernel`].
//! - **Constructors.** `SmFilletCorner(...)` (unit `blend.fillet_corner_p1`) and the
//!   constructors of the derived classes owned by `blend.fillet_corner_p2`
//!   (`SmFillet1x1Corner`, `SmFilletNx1ClosedCorner`, `SmFilletNx1Corner`,
//!   `SmFillet3x2MixedConvexityCorner`, `SmFilletNx2Corner`, `SmFilletNxNCorner`) only
//!   initialise members: they are CO-PORTED here as [`FilletCorner::new`] so
//!   [`FilletCorner::create`] can build every kind. `m_eStatus` (never initialised
//!   upstream) starts as [`FilStatus::Unprocessed`]; `SmFilletNx2Corner`'s `m_bG1Case`
//!   (set by its `MakeCornerTopology`) starts as `false`.
//! - **Unported callees.** `sm_TestG1Continuity`, `SmFilletNx1Corner::CheckNx1Degeneracy`,
//!   `CreateEndPatch`, `MakeFaceBrep` and `CalcCornerVertGeom` (unit
//!   `blend.fillet_corner_p1`), `sm_FindConcaveEdgeuses`, `MakeAnalyticCornerSurface`,
//!   `AdjustConcaveCornerTopology` and the `SmFilletConvexNxNCorner` /
//!   `SmFilletNx1ClosedCorner` overrides (`blend.fillet_corner_p2`),
//!   `TestAndConvertBigRadFillet` and `NoteFilletError` (`blend.fillet_executive_p1`),
//!   `SmFilletEdge::MakeFilletEdge` (`blend.fillet_geom_p5`), `GetRailVertex`
//!   (`blend.fillet_geom_p6`), `ReCalcFilletGeom` (`blend.fillet_geom_p3`),
//!   `SplitFilletGeomAtSideEdge` (ported in `blend.fillet_solver_p1` on its own fillet
//!   geometry handles; its first call `GetRailIndexOfVertexuse` is
//!   `blend.fillet_geom_p6`, so the stub carries that tag) and `SmSurface::DropCurve`
//!   (`intersect.surface_drop_lib_p3`) return [`SolidError::Unported`] at the call site.
//! - **Intersection-point user pointers.** `SmTsectPnt::m_apUserPointer[0..2]` (the two
//!   surfaces of a fillet vertex use's intersection point, read by
//!   `AdjustTangentCorner`) are not members of the Rust [`TsectPnt`]: the caller passes
//!   them per vertex use in [`CornerEnv::tsect_surfaces`]. `SmTemporaryChangeValue`
//!   enabling out-of-bounds evaluation of the user surfaces (every `SmBSplineSurface`,
//!   which includes `SmPlane` and `SmSurfOfRevolution`) is applied to copies.
//! - **Topology moves.** `SmVertex::Remove` / `PostInsert` of a vertex use (the
//!   `SmOwningTopology` list operations) are applied to the pseudo B-rep's records
//!   directly; `SmVertex::SetPoint`'s notification resets the vertex uses' cached gaps.
//! - **Undefined inputs.** Where upstream dereferences `NULL` (a missing edge use
//!   neighbour, solver, surface generator, curve or vertex use; `m_pAllEUs` unset) or
//!   reads past an array (`(*pSolverEUs)[0]` of an empty list, the three vertices of a
//!   three-sided patch), the port returns [`SolidError::InvalidInput`] or
//!   `SM_ERR_NULL_POINTER`; a classification object of the wrong kind where upstream
//!   casts blindly is [`SolidError::InvalidInput`]. `SmFilletNx2Corner::CreateCornerPatch`
//!   walks `m_vVertices` down from `GetSize() - 1` with an unsigned index: an empty list
//!   wraps around upstream and is [`SolidError::InvalidInput`] here.
//! - **Iteration.** The walk around the corner vertex in [`FilletCorner::create`]
//!   (next edge use = radial of the clockwise neighbour) is a permutation of the edge
//!   uses, so it returns to its start; it is bounded by the B-rep's edge use count (an
//!   error past it, which only corrupt topology reaches). There is no recursion.

use std::collections::BTreeMap;

use crate::blend::fillet_geom::p1::{
    FilStatus, FilletBrep, FilletCornerId, FilletCornerType, FilletEdgeType, FilletGeomId,
    FilletGeomType, FilletSolverId, FilletVertex, FilletVertexType,
};
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGeneratorType};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::global_solver::SolverOperation;
use crate::intersect::point_class::PointClassType;
use crate::intersect::surface_intersector::p1::SrfIndex;
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::{p1 as eu1, p2 as eu2, p3 as eu3};
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId, VertexuseId};

const OP_CREATE: &str = "blend::fillet_corner::p3::create";
const OP_PATCH: &str = "blend::fillet_corner::p3::create_corner_patch";
const OP_3X2: &str = "blend::fillet_corner::p3::create_corner_patch_3x2";
const OP_NX2_PATCH: &str = "blend::fillet_corner::p3::create_corner_patch_nx2";
const OP_TOPO: &str = "blend::fillet_corner::p3::adjust_topology";
const OP_TANGENT: &str = "blend::fillet_corner::p3::adjust_tangent_corner";

/// `SM_BIG_DOUBLE`.
const SM_BIG_DOUBLE: f64 = BIG_DOUBLE;

// ---- co-ported records --------------------------------------------------------------

/// The derived class of a fillet corner (`SmFilletCorner` is abstract upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilletCornerKind {
    /// `SmFillet1x1Corner`.
    OneByOne,
    /// `SmFillet2x2Corner` (derives from `SmFillet1x1Corner`).
    TwoByTwo,
    /// `SmFilletNx1ClosedCorner`.
    NByOneClosed,
    /// `SmFilletNx1Corner`.
    NByOne,
    /// `SmFilletDegenerateCorner`.
    Degenerate,
    /// `SmFilletOpenCorner` (derives from `SmFilletNx1Corner`).
    Open,
    /// `SmFillet3x2MixedConvexityCorner`.
    ThreeByTwoMixed,
    /// `SmFilletNx2Corner`.
    NByTwo,
    /// `SmFillet4x3Corner`.
    FourByThree,
    /// `SmFilletConvexNxNCorner` (derives from `SmFilletNxNCorner`).
    ConvexNByN,
    /// `SmFilletConcaveNxNCorner` (derives from `SmFilletConvexNxNCorner`).
    ConcaveNByN,
}

impl FilletCornerKind {
    /// `GetCornerType()` of the derived class.
    #[must_use]
    pub const fn corner_type(self) -> FilletCornerType {
        match self {
            Self::OneByOne => FilletCornerType::OneByOne,
            Self::TwoByTwo => FilletCornerType::TwoByTwo,
            Self::NByOneClosed => FilletCornerType::NByOneClosed,
            Self::NByOne => FilletCornerType::NByOne,
            Self::Degenerate => FilletCornerType::Degenerate,
            Self::Open => FilletCornerType::Open,
            Self::ThreeByTwoMixed => FilletCornerType::ThreeByTwoMixed,
            Self::NByTwo => FilletCornerType::NByTwo,
            Self::FourByThree => FilletCornerType::FourByThree,
            Self::ConvexNByN => FilletCornerType::NByNConvex,
            Self::ConcaveNByN => FilletCornerType::NByNConcave,
        }
    }

    /// `GetName()` of the derived class.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::OneByOne => "SmFillet1x1Corner",
            Self::TwoByTwo => "SmFillet2x2Corner",
            Self::NByOneClosed => "SmFilletNx1ClosedCorner",
            Self::NByOne => "SmFilletNx1Corner",
            Self::Degenerate => "SmFilletDegenerateCorner",
            Self::Open => "SmFilletOpenCorner",
            Self::ThreeByTwoMixed => "SmFillet3x2MixedConvexityCorner",
            Self::NByTwo => "SmFilletNx2Corner",
            Self::FourByThree => "SmFillet4x3Corner",
            Self::ConvexNByN => "SmFilletConvexNxNCorner",
            Self::ConcaveNByN => "SmFilletConcaveNxNCorner",
        }
    }
}

/// The members of `SmFilletExecutive` read by the corner code (CO-PORTED from
/// `blend.fillet_executive_p1`).
#[derive(Debug, Clone, Default)]
pub struct FilletExecutiveData {
    /// `m_vEdgeuseSolverMap`: the fillet solver of each filleted edge use.
    pub edgeuse_solver_map: BTreeMap<EdgeuseId, FilletSolverId>,
}

impl FilletExecutiveData {
    /// `SmFilletExecutive::GetFilletSolverOfEdgeuse`: the solver registered for `eu`,
    /// `None` when there is none.
    #[must_use]
    pub fn fillet_solver_of_edgeuse(&self, eu: EdgeuseId) -> Option<FilletSolverId> {
        self.edgeuse_solver_map.get(&eu).copied()
    }
}

/// A fillet corner (`SmFilletCorner` and its derived classes).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // upstream's independent SmBoolean members
pub struct FilletCorner {
    /// The derived class.
    pub kind: FilletCornerKind,
    /// `m_cpVertex`: the vertex being filleted (B-rep being filleted).
    pub vertex: Option<VertexId>,
    /// `m_pSolverEUs`: the outward edge uses around the corner that are filleted.
    pub solver_eus: Option<Vec<EdgeuseId>>,
    /// `m_pAllEUs`: all outward edge uses around the corner.
    pub all_eus: Option<Vec<EdgeuseId>>,
    /// `m_vVertices`: the fillet vertices of the corner (pseudo B-rep).
    pub vertices: Vec<VertexId>,
    /// `m_vEdges`: the fillet edges around the corner (pseudo B-rep).
    pub edges: Vec<EdgeId>,
    /// `m_vSurfaces`: the surfaces built for the corner.
    pub surfaces: Vec<BSplineSurface>,
    /// `m_bBevel`.
    pub bevel: bool,
    /// `m_bBlending`.
    pub blending: bool,
    /// `m_bChamfer`.
    pub chamfer: bool,
    /// `m_bDegenerate`.
    pub degenerate: bool,
    /// `m_bCalcAsConstRad`.
    pub calc_as_const_rad: bool,
    /// `m_bTopologyAdjusted`.
    pub topology_adjusted: bool,
    /// `m_bExtendedSurfacePatch`.
    pub extended_surface_patch: bool,
    /// `m_dSetBackDist`.
    pub set_back_dist: f64,
    /// `m_dThisApproxTol3d`.
    pub approx_tol_3d: f64,
    /// `m_dTangencyTolRadians`.
    pub tangency_tol_radians: f64,
    /// `m_eStatus` (not initialised upstream).
    pub status: FilStatus,
    /// `m_bG1Case` of `SmFilletNx1ClosedCorner` / `SmFilletNx2Corner`.
    pub g1_case: bool,
}

/// What the corner methods operate on besides the corner itself.
#[derive(Debug)]
pub struct CornerEnv<'a> {
    /// `m_crContext`.
    pub k: &'a Kernel,
    /// The B-rep being filleted (`m_cpVertex`, the edge uses, original faces).
    pub target: &'a mut Brep,
    /// `m_pPseudoBrep`.
    pub pseudo: &'a mut FilletBrep,
    /// The corner's identity (`this`, as new fillet vertices and edges store it).
    pub corner: Option<FilletCornerId>,
    /// `m_apUserPointer[0..2]` of the intersection point of each fillet vertex use (see
    /// the module docs).
    pub tsect_surfaces: &'a BTreeMap<VertexuseId, [Option<Surface>; 2]>,
}

fn invalid(op: &'static str, reason: &'static str) -> SolidError {
    SolidError::invalid_input(op, reason)
}

fn unported(upstream: &'static str, unit: &'static str) -> SolidError {
    SolidError::unported(upstream, unit)
}

/// `SmVertex::SetPoint` on a pseudo-B-rep vertex: the notification resets the cached
/// gaps of every vertex use, then the point is stored.
fn set_vertex_point(pseudo: &mut FilletBrep, v: VertexId, point: Point3) -> Result<()> {
    let brep = pseudo.brep_mut();
    for vu in brep.vertex(v)?.vertexuses.clone() {
        let rec = brep.vertexuse_mut(vu)?;
        rec.vertex_edge_gap.reset();
        rec.vertex_face_gap.reset();
    }
    brep.vertex_mut(v)?.point = point;
    Ok(())
}

/// `pFrom->Remove(pVU); pTo->PostInsert(pVU)` for a vertex use of the pseudo B-rep.
fn move_vertexuse(
    pseudo: &mut FilletBrep,
    vu: VertexuseId,
    from: VertexId,
    to: VertexId,
) -> Result<()> {
    let brep = pseudo.brep_mut();
    brep.vertex_mut(from)?.vertexuses.retain(|&x| x != vu);
    brep.vertexuse_mut(vu)?.vertex = Some(to);
    brep.vertex_mut(to)?.vertexuses.push(vu);
    Ok(())
}

/// The `SmBSplineCurve` part of a curve (`SM_CAST_PTR(SmBSplineCurve, ...)`: the
/// analytic curves derive from it upstream).
fn bspline_part(curve: &Curve) -> Option<BSplineCurve> {
    match curve.kind() {
        CurveKind::BSpline(c) => Some(c.clone()),
        CurveKind::Line(c) => Some(c.base.clone()),
        CurveKind::Ellipse(c) => Some(c.base().clone()),
        CurveKind::Hyperbola(c) => Some(c.base().clone()),
        CurveKind::Parabola(c) => Some(c.base().clone()),
        _ => None,
    }
}

/// The neighbour surface `SmBSplineSurface::CreateCornerBlend` gets (see
/// `topology::brep::p1`).
fn bspline_surface(surface: &Surface) -> Result<&BSplineSurface> {
    match surface.kind() {
        SurfaceKind::BSpline(s) => Ok(s),
        SurfaceKind::Plane(p) => Ok(p.base()),
        SurfaceKind::SurfOfRevolution(s) => Ok(s.base()),
        // UNPORTED(nurbs.b_spline_surface_n_lib_p2): SmBSplineSurface::CreateCornerBlend with a non-B-spline neighbour surface
        _ => Err(unported(
            "SmBSplineSurface::CreateCornerBlend (non-B-spline neighbour SmSurface)",
            "nurbs.b_spline_surface_n_lib_p2",
        )),
    }
}

/// The primary edge use of a pseudo-B-rep edge (`GetPrimaryEdgeuse`), required.
fn primary_eu(pseudo: &FilletBrep, e: EdgeId, op: &'static str) -> Result<EdgeuseId> {
    eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(|| SolidError::null_pointer(op))
}

impl FilletCorner {
    /// `SmFilletCorner(crContext, cpVertex, pSolverEUs, pAllEUs, pExec, dApproxTol3d,
    /// dTangencyTolRadians)` and the derived constructors (CO-PORTED, module docs):
    /// no fillet vertices, edges or surfaces, every flag off except
    /// `m_bExtendedSurfacePatch`, no setback; the degenerate corner sets
    /// `m_bDegenerate`, the closed Nx1 corner clears `m_bG1Case`.
    #[must_use]
    pub fn new(
        kind: FilletCornerKind,
        vertex: Option<VertexId>,
        solver_eus: Option<Vec<EdgeuseId>>,
        all_eus: Option<Vec<EdgeuseId>>,
        approx_tol_3d: f64,
        tangency_tol_radians: f64,
    ) -> Self {
        Self {
            kind,
            vertex,
            solver_eus,
            all_eus,
            vertices: Vec::new(),
            edges: Vec::new(),
            surfaces: Vec::new(),
            bevel: false,
            blending: false,
            chamfer: false,
            // SmFilletDegenerateCorner: m_bDegenerate = TRUE
            degenerate: kind == FilletCornerKind::Degenerate,
            calc_as_const_rad: false,
            topology_adjusted: false,
            extended_surface_patch: true,
            set_back_dist: 0.0,
            approx_tol_3d,
            tangency_tol_radians,
            status: FilStatus::Unprocessed,
            // SmFilletNx1ClosedCorner: m_bG1Case = FALSE
            g1_case: false,
        }
    }

    /// `GetCornerType()`.
    #[must_use]
    pub const fn corner_type(&self) -> FilletCornerType {
        self.kind.corner_type()
    }

    // ---- SmFilletCorner::Create ------------------------------------------------------

    /// `SmFilletCorner::Create`: the corner for `vertex` whose kind follows from the
    /// number of edges around the vertex and how many of them are filleted
    /// (`solver_eus`, outward edge uses at the vertex). Its tolerances are the smallest
    /// approximation and tangency tolerances of the solvers of `solver_eus`; it is a
    /// chamfer corner when all of them generate linear cross sections. `None` for
    /// corner cases upstream does not implement.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for an empty `solver_eus`, an edge use without a
    /// clockwise neighbour, radial partner or edge, a filleted edge use without a
    /// solver (or a solver id past `solvers`) or a solver without surface generator
    /// (upstream dereferences them), and a walk that does not close; those of the
    /// sector convexity test; [`SolidError::Unported`] (module docs).
    pub fn create(
        k: &Kernel,
        b: &mut Brep,
        vertex: VertexId,
        solver_eus: &[EdgeuseId],
        exec: &FilletExecutiveData,
        solvers: &[FilletSolver],
    ) -> Result<Option<Self>> {
        // get number of edgeuses connected to this vertex to be filleted
        let total_solver_eus = solver_eus.len();

        // locals
        let mut is_open_end_corner = false;
        let mut is_chamfer_corner = true;

        // get first to-be-filleted edgeuse connected to vertex
        let start_eu = *solver_eus
            .first()
            .ok_or_else(|| invalid(OP_CREATE, "no edge use to fillet"))?;

        // Create array of all outward edgeuses connected to unique edges that surround the corner
        let mut all_eus = vec![start_eu];
        let mut eu = start_eu;
        let edge_of = |b: &Brep, eu: EdgeuseId| -> Result<EdgeId> {
            eu1::edge(b, eu)?.ok_or_else(|| invalid(OP_CREATE, "edge use without edge"))
        };
        let mut steps = 0usize;
        let max_steps = b.edgeuses().len();
        loop {
            steps += 1;
            if steps > max_steps.saturating_add(1) {
                return Err(invalid(OP_CREATE, "walk around the vertex does not close"));
            }
            // get edgeuse neighbors circuling this vertex and march to next radial neighbor
            let eu0 = eu; // edgeuse starting at vertex
            let eu1_ = eu1::cw_edgeuse(b, eu0)?
                .ok_or_else(|| invalid(OP_CREATE, "edge use without clockwise neighbour"))?; // edgeuse in same loop ending at vertex
            let eu2_ = eu1::radial(b, eu1_)?
                .ok_or_else(|| invalid(OP_CREATE, "edge use without radial partner"))?; // next edgeuse starting at vertex in same region
            eu = eu2_;

            // quit - when we get back to StartEU
            if eu == start_eu {
                break;
            }

            // skip strut edgeuses
            if eu1::edge(b, eu0)? == eu1::edge(b, eu1_)? {
                continue;
            }

            // skip edgeuses that duplicate an edge - happens on closed curves
            let e = eu1::edge(b, eu)?;
            let mut found = false;
            for &eu3 in &all_eus {
                if e == eu1::edge(b, eu3)? {
                    found = true;
                    break;
                }
            }
            if found {
                continue;
            }

            // lamina edges have OpenEndCorners
            if eu1::edge_is_lamina(b, edge_of(b, eu)?)? {
                is_open_end_corner = true;
            }

            // If we have mixed convexity, pEU may not match the edgeuse in
            // pSolverEUs, use the one in pSolverEUs instead
            let mut solver_eu = None;
            for &s in solver_eus {
                if eu != s && e == eu1::edge(b, s)? {
                    // SM_DBG_WARN: CONFUSED CLASSIFICATION OF CORNER VERTEX
                    solver_eu = Some(s);
                    break; // mixed convexity
                }
            }
            all_eus.push(solver_eu.unwrap_or(eu));
        } // end while(TRUE) adding outward edgeuses to pAllEUs list

        // get total number of outward edgeuses connected to unique edges that are connected to vertex
        let total_eus = all_eus.len();

        // 3D tolerance for corner patch creation
        let mut approx_tol_3d = SM_BIG_DOUBLE;
        // Angle tol for tangent field approximation
        let mut tangency_tol_radians = SM_BIG_DOUBLE;

        // for every to-be-filleted edgeuse connected to vertex:
        // Collect max tolerances and check for Chamfer corner.
        for &edge_use in solver_eus {
            let solver = exec
                .fillet_solver_of_edgeuse(edge_use)
                .and_then(|id| usize::try_from(id.0).ok())
                .and_then(|i| solvers.get(i))
                .ok_or_else(|| invalid(OP_CREATE, "filleted edge use without fillet solver"))?;
            let tol = solver.approx_tol_3d;
            let tan_tol = solver.tangency_tolerance;

            // save tightest tolerance value
            if tol < approx_tol_3d {
                approx_tol_3d = tol;
            }
            if tan_tol < tangency_tol_radians {
                tangency_tol_radians = tan_tol;
            }

            // when any to-be-filleted edgeuse is not linear - this it not a ChamferCorner
            if is_chamfer_corner {
                let fsg = solver
                    .fsg
                    .as_ref()
                    .ok_or_else(|| invalid(OP_CREATE, "fillet solver without surface generator"))?;
                if fsg.fillet_surface_generator_type() != FilletSurfaceGeneratorType::Linear {
                    is_chamfer_corner = false;
                }
            }
        }

        let tan_tol_deg = rad_to_deg(tangency_tol_radians);
        let make = |kind| {
            Self::new(
                kind,
                Some(vertex),
                Some(solver_eus.to_vec()),
                Some(all_eus.clone()),
                approx_tol_3d,
                tangency_tol_radians,
            )
        };

        // switch on number of to-be-filleted edgeuses connected to vertex
        let corner = match total_solver_eus {
            0 => None,
            1 => {
                let eu0 = start_eu;
                let e = edge_of(b, eu0)?;
                if edge::is_closed(b, e)? {
                    // Corner with one 'closed' edge filleted
                    match total_eus {
                        // Need to be G1-closed
                        1 => {
                            if test_g1_continuity(b, vertex, e, e, tan_tol_deg)? {
                                Some(make(FilletCornerKind::OneByOne))
                            } else {
                                None // MSG: Corner Case Not Implemented
                            }
                        }
                        2 | 3 => Some(make(FilletCornerKind::NByOneClosed)),
                        _ => None, // MSG: Corner Case Not Implemented
                    }
                } else if check_nx1_degeneracy(b, vertex, eu0)? {
                    // Degeneracy-check needs to be checked first
                    Some(make(FilletCornerKind::Degenerate))
                } else if is_open_end_corner {
                    Some(make(FilletCornerKind::Open))
                } else {
                    Some(make(FilletCornerKind::NByOne))
                }
            }
            2 => {
                let eu0 = start_eu;
                let eu1_ = *solver_eus
                    .get(1)
                    .ok_or_else(|| invalid(OP_CREATE, "second filleted edge use"))?;
                match total_eus {
                    // 2x2: we can handle this only if it's a tangent fillet
                    2 => {
                        let e0 = edge_of(b, eu0)?;
                        let e1 = edge_of(b, eu1_)?;
                        if test_g1_continuity(b, vertex, e0, e1, tan_tol_deg)? {
                            Some(make(FilletCornerKind::TwoByTwo))
                        } else {
                            None // MSG: Corner Case Not Implemented
                        }
                    }
                    3 | 4 => {
                        if total_eus == 3
                            && eu2::is_convex_radial_sector(k, b, eu0, 4, false, 0.0)?
                                != eu2::is_convex_radial_sector(k, b, eu1_, 4, false, 0.0)?
                        {
                            Some(make(FilletCornerKind::ThreeByTwoMixed))
                        } else {
                            // else continue with case 4
                            Some(make(FilletCornerKind::NByTwo))
                        }
                    }
                    _ => None, // MSG: Corner Case Not Implemented
                }
            }
            _ => {
                if total_solver_eus == 3 && total_eus == 4 {
                    Some(make(FilletCornerKind::FourByThree))
                } else if total_eus != total_solver_eus {
                    // Basically, we are handling NxN cases here
                    None // MSG: Corner Case Not Implemented
                } else {
                    let mut concave_eus: Vec<EdgeuseId> = Vec::new();
                    if !is_chamfer_corner {
                        concave_eus = find_concave_edgeuses(b, vertex, &all_eus)?;
                    }
                    if concave_eus.is_empty() {
                        let mut c = make(FilletCornerKind::ConvexNByN);
                        c.chamfer = is_chamfer_corner; // SetChamfer
                        Some(c)
                    } else {
                        Some(make(FilletCornerKind::ConcaveNByN))
                    }
                }
            }
        };
        Ok(corner)
    }

    // ---- SmFilletCorner::CalcCornerGeom ----------------------------------------------

    /// `SmFilletCorner::CalcCornerGeom`: builds the corner patch (`CreateCornerPatch`)
    /// unless the corner is a bevel between several fillets or has fewer than three
    /// fillet edges.
    ///
    /// # Errors
    ///
    /// Those of [`FilletCorner::create_corner_patch`].
    pub fn calc_corner_geom(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        let total_edges = self.edges.len();
        let fillet_edges = self.solver_eus.as_ref().map_or(0, Vec::len);

        // no work - Bevel corner or degenerate corner
        if (self.bevel && fillet_edges > 1) || total_edges < 3 {
            return Ok(());
        }

        // Otherwise, pass the call along to build a Corner->FilletSurface and store it in m_vSurfaces
        self.create_corner_patch(k, env)
    }

    // ---- CreateCornerPatch -----------------------------------------------------------

    /// `CreateCornerPatch` (virtual): the 3x2 mixed-convexity, Nx1 (and open) and Nx2
    /// overrides, the default patch for every other kind.
    ///
    /// # Errors
    ///
    /// Those of the selected implementation.
    pub fn create_corner_patch(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        match self.kind {
            FilletCornerKind::ThreeByTwoMixed => self.create_corner_patch_3x2(k, env),
            FilletCornerKind::NByOne | FilletCornerKind::Open => {
                self.create_corner_patch_nx1(k, env)
            }
            FilletCornerKind::NByTwo => self.create_corner_patch_nx2(k, env),
            _ => self.create_corner_patch_default(k, env),
        }
    }

    /// `SmFilletCorner::CreateCornerPatch`: an analytic corner surface when one fits,
    /// else a three- or four-sided Coons patch over the corner's fillet edges (a
    /// three-sided one gets a degenerate point curve at the vertex selected for it),
    /// then the corner face (`MakeFaceBrep`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] (`MakeAnalyticCornerSurface`, `MakeFaceBrep`,
    /// `NoteFilletError`); `SM_ERR_NULL_POINTER` for a fillet edge without fillet
    /// geometry where one is needed; [`SolidError::InvalidInput`] where upstream
    /// dereferences null or indexes past the corner's arrays; those of the point curves
    /// and `CreateCornerBlend`.
    pub fn create_corner_patch_default(
        &mut self,
        k: &Kernel,
        env: &mut CornerEnv<'_>,
    ) -> Result<()> {
        // if an AnalyticSurface can approx FilletCorner->Curves to tol,
        //   Make and store that AnalyticSurface in m_vSurfaces, and return TRUE
        // else return FALSE.
        let success = self.make_analytic_corner_surface(env)?;

        if !success {
            // When unable to create analytic corner - create COONS patch
            let mut init_index = 0usize;
            let mut total_edges = self.edges.len();
            let mut fv_of_degenerate_curve: Option<VertexId> = None; // used to indicate degenerate Curve of 3 sided patches

            // for corners with 3 edges - select a degenerate vertex (lInitIndex)
            if total_edges == 3 {
                // Select degenerate corner - Pay special attention to Nx2 corners
                let corner_type = self.corner_type();
                if corner_type == FilletCornerType::NByTwo
                    || corner_type == FilletCornerType::ThreeByTwoMixed
                    || corner_type == FilletCornerType::NByOne
                {
                    // for every corner vertex
                    for &fv in &self.vertices {
                        match env.pseudo.fillet_vertex(fv)?.fillet_vertex_type() {
                            FilletVertexType::RailXEdgeuse => {
                                if fv_of_degenerate_curve.is_none() {
                                    fv_of_degenerate_curve = Some(fv);
                                }
                            }
                            FilletVertexType::FilletXFillet => fv_of_degenerate_curve = Some(fv),
                            FilletVertexType::OnVertex
                                if corner_type == FilletCornerType::NByOne =>
                            {
                                fv_of_degenerate_curve = Some(fv);
                            }
                            _ => {}
                        }
                    }
                }

                // when we have yet to select the degenerate corner
                let fv_degenerate = if let Some(fv) = fv_of_degenerate_curve {
                    fv
                } else {
                    // get distance between every filletCorner->FilletVertex pair
                    let vert = |i: usize| -> Result<VertexId> {
                        self.vertices
                            .get(i)
                            .copied()
                            .ok_or_else(|| invalid(OP_PATCH, "fewer than three corner vertices"))
                    };
                    let mut pnt_pnt_dist = [0.0f64; 4];
                    for (ii, d) in pnt_pnt_dist.iter_mut().enumerate().take(3) {
                        let next_vert = (ii + 1) % 3;
                        let pnt = env.pseudo.vertex_point(vert(ii)?)?;
                        *d = pnt.distance(env.pseudo.vertex_point(vert(next_vert)?)?);
                    }
                    pnt_pnt_dist[3] = pnt_pnt_dist[0];

                    // select degenerate vertex = vertex connected to edges most equal in length
                    let mut vert_of_deg_curve = 0usize;
                    let mut min_diff = (pnt_pnt_dist[2] - pnt_pnt_dist[0]).abs();
                    for (ii, w) in pnt_pnt_dist.windows(2).enumerate().take(2) {
                        let ii = ii + 1;
                        let &[prev, cur] = w else { continue };
                        let diff = (prev - cur).abs();
                        if diff < min_diff {
                            min_diff = diff;
                            vert_of_deg_curve = ii;
                        }
                    }

                    // set the Degenerate vertex pointer
                    vert(vert_of_deg_curve)?
                };
                fv_of_degenerate_curve = Some(fv_degenerate);

                // get index of degenerate vertex
                for ii in 0..3 {
                    let e = *self
                        .edges
                        .get(ii)
                        .ok_or_else(|| invalid(OP_PATCH, "corner edge index"))?;
                    if edge::vertex(env.pseudo.brep(), e)? == Some(fv_degenerate) {
                        init_index = ii; // Left curve index
                        break;
                    }
                }
            }
            // else for corners with 4 edges branch - set lInitIndex == EdgeOfType SM_FE_RAIL_RAIL_INTERPOLATION
            else if total_edges == 4 {
                for (ii, &fe) in self.edges.iter().enumerate() {
                    if env.pseudo.fillet_edge(fe)?.fillet_edge_type()
                        == FilletEdgeType::RailRailInterpolation
                    {
                        init_index = (ii + 1) % total_edges; // Left curve index
                        break;
                    }
                }
            }

            // Next - build ordered patch boundary arrays.
            let mut ordered_uv: Vec<Option<BSplineCurve>> = Vec::new();
            let mut ordered_3d: Vec<Option<Curve>> = Vec::new();
            let mut orients: Vec<OrientType> = Vec::new();
            let mut ordered_surfs: Vec<Option<Surface>> = Vec::new();

            // for every edge - get edge shape as (neighborFace->Surface,UVTrimCurve) pair
            for ii in 0..total_edges {
                let fe = *self
                    .edges
                    .get((init_index + ii) % total_edges)
                    .ok_or_else(|| invalid(OP_PATCH, "corner edge index"))?;
                let prim_eu = primary_eu(env.pseudo, fe, OP_PATCH)?;
                let mate_eu = eu1::mate(env.pseudo.brep(), prim_eu)?;
                let mut ps_curve: Option<BSplineCurve> = None;
                let mut base_surface: Option<Surface> = None;

                // when build g1 patches - get UVTrimCurve and Surface
                if self.extended_surface_patch {
                    // when the edge has an OriginalFace
                    if let Some(orig_face) = env.pseudo.fillet_edge(fe)?.original_face() {
                        let mate_eu = mate_eu.ok_or_else(|| SolidError::null_pointer(OP_PATCH))?;
                        ps_curve = eu1::uv_trim_curve(env.pseudo.brep(), mate_eu)?
                            .0
                            .and_then(bspline_part);
                        base_surface.clone_from(&env.target.face(orig_face)?.surface);
                    } else {
                        // no Original Face branch - curve is only connected to filletSurfaces
                        // use primaryEdgeuse to get (neighborFace,UVTrimCurve) pair
                        ps_curve = eu1::uv_trim_curve(env.pseudo.brep(), prim_eu)?
                            .0
                            .and_then(bspline_part);
                        let g = env
                            .pseudo
                            .fillet_edgeuse(prim_eu)?
                            .fillet_geom()
                            .ok_or_else(|| SolidError::null_pointer(OP_PATCH))?;
                        base_surface.clone_from(&env.pseudo.geom(g)?.fillet_surface);
                    }
                }

                // add edge 3DCurve, NeighborSurface, NeighborSurface->UVTrimCurve, orients to ordered arrays
                ordered_uv.push(ps_curve);
                ordered_3d.push(env.pseudo.brep().edge(fe)?.curve.clone());
                orients.push(OrientType::Same);
                ordered_surfs.push(base_surface);
            }

            // when building a 3 sided patch - add a degenerate curve to the Ordered arrays
            if let Some(fv) = fv_of_degenerate_curve {
                // make a temporary degenerate 3DCurve
                let pnt = env.pseudo.vertex_point(fv)?;
                let point_curve = BSplineCurve::create_point_curve(pnt)?;

                // when degenerate vertex classifies to a Face
                // and building g1 patches - give it a (surface,UVTrimCurve) pair
                let mut base_surface: Option<Surface> = None;
                let mut uv_point_curve: Option<BSplineCurve> = None;
                let rec = env.pseudo.fillet_vertex(fv)?;
                if let (Some(EntityRef::Face(face)), true) =
                    (rec.point_class_object(), self.extended_surface_patch)
                {
                    base_surface.clone_from(&env.target.face(face)?.surface);
                    let uv = rec.original_uv();
                    let pt = Point3::new(uv.x, uv.y, 0.0);
                    let mut c = BSplineCurve::create_point_curve(pt)?;
                    c.convert_to_2d()?;
                    uv_point_curve = Some(c);
                }

                // add degenerate curve to ordered boundary arrays
                ordered_uv.push(uv_point_curve);
                ordered_3d.push(Some(Curve::from(point_curve)));
                orients.push(OrientType::Same);
                ordered_surfs.push(base_surface);
                total_edges += 1;
            }
            let _ = total_edges;

            // build the corner patch from the ordered array boundary information
            let mut curves_3d = Vec::with_capacity(ordered_3d.len());
            for c in &ordered_3d {
                let c = c
                    .as_ref()
                    .ok_or_else(|| SolidError::null_pointer(OP_PATCH))?;
                curves_3d.push(
                    bspline_part(c)
                        .ok_or_else(|| invalid(OP_PATCH, "boundary curve is not a B-spline"))?,
                );
            }
            let mut surfs: Vec<Option<&BSplineSurface>> = Vec::with_capacity(ordered_surfs.len());
            for s in &ordered_surfs {
                surfs.push(match s {
                    Some(s) => Some(bspline_surface(s)?),
                    None => None,
                });
            }
            let blends = BSplineSurface::create_corner_blend(
                k,
                &curves_3d,
                &orients,
                &ordered_uv,
                &surfs,
                self.approx_tol_3d,
                self.tangency_tol_radians,
                !self.extended_surface_patch,
            )?;

            // store New FilletSurface in m_vSurfaces
            self.surfaces.extend(blends);
        }

        // Create Face (and Edges and Verts) in m_pFilletBrep from m_vSurfaces with call m_pFilletBrep->MakeFaceWithCurves()
        if let Err(e) = self.make_face_brep(env) {
            if e.is_unported() {
                return Err(e);
            }
            // when MakeFaceBrep failed - note the error
            // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::NoteFilletError
            return Err(unported(
                "SmFilletExecutive::NoteFilletError",
                "blend.fillet_executive_p1",
            ));
        }

        // all done
        Ok(())
    }

    /// `SmFillet3x2MixedConvexityCorner::CreateCornerPatch`: joins the on-edge fillet
    /// edge's curve onto the on-extended-edge fillet edge's curve, merges the two edges
    /// (the on-edge edge and its corner vertex are deleted, the vertex's uses moved to
    /// the edge's other vertex), drops the joined curve onto the original face for the
    /// mate edge use's UV curve and builds the three-sided default patch.
    ///
    /// # Errors
    ///
    /// `SM_ERR` unless the corner has four fillet edges, for a corner vertex that is
    /// not `SM_FV_ON_VERTEX` or starts the on-extended-edge edge, or one not in the
    /// corner; `SM_ERR_NULL_POINTER` for a missing on-edge / on-extended-edge edge, a
    /// non-B-spline curve or a missing original face; those of `JoinWith`;
    /// [`SolidError::Unported`] (`SmSurface::DropCurve`).
    pub fn create_corner_patch_3x2(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        // check state - 4 edges
        if self.edges.len() != 4 {
            // SM_DBG_WARN: Failed state check: 3x2 FilletCorner with other than 4 edges
            return Err(SolidError::sm_err(OP_3X2));
        }

        // search for the onEdge and onExtended edges
        let mut fe_on_edge = None;
        let mut fe_on_extended_edge = None;
        let mut found_index = 0usize;
        for (ii, &fe) in self.edges.iter().enumerate() {
            match env.pseudo.fillet_edge(fe)?.fillet_edge_type() {
                FilletEdgeType::OnExtendEdge => fe_on_extended_edge = Some(fe),
                FilletEdgeType::OnEdge => {
                    // To be deleted later
                    found_index = ii;
                    fe_on_edge = Some(fe);
                }
                _ => {}
            }
        }

        // check state - must have onEdge and onExtendedEdge
        let fe_on_edge = fe_on_edge.ok_or_else(|| SolidError::null_pointer(OP_3X2))?;
        let fe_on_extended_edge =
            fe_on_extended_edge.ok_or_else(|| SolidError::null_pointer(OP_3X2))?;

        // get onEdge->Vertices
        let corner_v = edge::vertex(env.pseudo.brep(), fe_on_edge)?
            .ok_or_else(|| invalid(OP_3X2, "on-edge fillet edge without vertex"))?;
        let surviving_fv = edge::other_vertex(env.pseudo.brep(), fe_on_edge, Some(corner_v))?;

        // check state
        if env.pseudo.fillet_vertex(corner_v)?.fillet_vertex_type() != FilletVertexType::OnVertex
            || edge::vertex(env.pseudo.brep(), fe_on_extended_edge)? == Some(corner_v)
        {
            // Internal error, check corner topology
            return Err(SolidError::sm_err(OP_3X2));
        }

        // get onEdge and onExtended curves (SM_CAST_PTR(SmBSplineCurve, ..): the analytic
        // curves derived from SmBSplineCurve pass)
        let bspline_curve = |pseudo: &FilletBrep, e: EdgeId| -> Result<Curve> {
            pseudo
                .brep()
                .edge(e)?
                .curve
                .as_ref()
                .filter(|c| {
                    matches!(
                        c.kind(),
                        CurveKind::BSpline(_)
                            | CurveKind::Line(_)
                            | CurveKind::Ellipse(_)
                            | CurveKind::Hyperbola(_)
                            | CurveKind::Parabola(_)
                    )
                })
                .cloned()
                .ok_or_else(|| SolidError::null_pointer(OP_3X2))
        };
        let mut curve1 = bspline_curve(env.pseudo, fe_on_extended_edge)?;
        let mut curve2 = bspline_curve(env.pseudo, fe_on_edge)?;

        // join the 2 curves (JoinWith is virtual: SmLine and SmEllipse override it)
        join_with(env.k, &mut curve1, true, &mut curve2, false)?;
        env.pseudo.brep_mut().edge_mut(fe_on_extended_edge)?.curve = Some(curve1.clone());
        env.pseudo.brep_mut().edge_mut(fe_on_edge)?.curve = Some(curve2);

        // Combine two edges
        self.edges.remove(found_index);
        let Some(idx) = self.vertices.iter().position(|&v| v == corner_v) else {
            return Err(SolidError::sm_err(OP_3X2));
        };
        self.vertices.remove(idx);

        env.pseudo.delete_fillet_edge(fe_on_edge)?;

        let surviving_fv = surviving_fv.ok_or_else(|| invalid(OP_3X2, "no surviving vertex"))?;
        for vu in env.pseudo.brep().vertex(corner_v)?.vertexuses.clone() {
            move_vertexuse(env.pseudo, vu, corner_v, surviving_fv)?;
        }

        env.pseudo.delete_fillet_vertex(corner_v)?;

        // Project pFEOnExtendedEdge onto original face
        let orig_face: FaceId = env
            .pseudo
            .fillet_edge(fe_on_extended_edge)?
            .original_face()
            .ok_or_else(|| SolidError::null_pointer(OP_3X2))?;
        let _surface = env.target.face(orig_face)?.surface.as_ref();
        let _ = (&curve1, self.approx_tol_3d);
        // UNPORTED(intersect.surface_drop_lib_p3): SmSurface::DropCurve
        let uv_curves: Vec<BSplineCurve> = Err(unported(
            "SmSurface::DropCurve",
            "intersect.surface_drop_lib_p3",
        ))?;
        if uv_curves.len() != 1 {
            return Err(SolidError::sm_err(OP_3X2)); // Unknown problem
        }
        let new_ps_curve = uv_curves
            .into_iter()
            .next()
            .ok_or_else(|| SolidError::sm_err(OP_3X2))?;
        let prim = primary_eu(env.pseudo, fe_on_extended_edge, OP_3X2)?;
        let mate_eu =
            eu1::mate(env.pseudo.brep(), prim)?.ok_or_else(|| SolidError::null_pointer(OP_3X2))?;
        env.pseudo
            .set_edgeuse_uv_curve(mate_eu, Some(Curve::from(new_ps_curve)), true, true)?;

        // Create a 3-sided corner patch
        self.create_corner_patch_default(k, env)
    }

    /// `SmFilletNx1Corner::CreateCornerPatch`: with three fillet edges an end patch
    /// from the extended side face (`CreateEndPatch`) or the default patch; otherwise
    /// nothing.
    ///
    /// # Errors
    ///
    /// Those of the patch builders; [`SolidError::Unported`] (`CreateEndPatch`).
    pub fn create_corner_patch_nx1(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        if self.edges.len() == 3 {
            if self.extended_surface_patch {
                self.create_end_patch(env)?;
            } else {
                self.create_corner_patch_default(k, env)?;
            }
        }
        // else SM_DBG_WARN: Unexpected SmFilletNx1Corner endFace construction case
        Ok(())
    }

    /// `SmFilletNx2Corner::CreateCornerPatch`: with six fillet edges a small end patch
    /// over the last three first; then the three-sided default patch over the first
    /// three edges and vertices; the edges, vertices and end patch are put back.
    ///
    /// # Errors
    ///
    /// Those of the patch builders; [`SolidError::InvalidInput`] for more than three
    /// edges and no vertex (module docs).
    pub fn create_corner_patch_nx2(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        let total_edges = self.edges.len();

        let mut first_three_edges: Vec<EdgeId> = Vec::new();
        let mut last_three_edges: Vec<EdgeId> = Vec::new();
        let mut last_two_verts: Vec<VertexId> = Vec::new();
        let mut end_patch: Option<BSplineSurface> = None;
        if total_edges > 3 {
            first_three_edges.extend(self.edges.iter().take(3).copied());
            last_three_edges.extend(self.edges.iter().skip(3).copied());

            if total_edges == 6 {
                // Use the last 3 edges to create a small patch
                self.edges.clone_from(&last_three_edges);
                // Create a small corner face
                self.create_end_patch(env)?;

                // Temporarily remove the small patch
                if self.surfaces.is_empty() {
                    return Err(invalid(OP_NX2_PATCH, "end patch built no surface"));
                }
                end_patch = Some(self.surfaces.remove(0));
                self.surfaces.clear();
            }

            self.edges.clone_from(&first_three_edges);

            // Temporarily remove the last two vertices
            if self.vertices.is_empty() {
                return Err(invalid(OP_NX2_PATCH, "unsigned vertex index wraps around"));
            }
            while self.vertices.len() > 3 {
                if let Some(v) = self.vertices.pop() {
                    last_two_verts.push(v);
                }
            }
        }

        // Create a 3-sided corner patch
        self.create_corner_patch_default(k, env)?;

        // Put the end patch back
        if let Some(p) = end_patch {
            self.surfaces.push(p);
        }

        if !last_three_edges.is_empty() {
            self.edges.extend(last_three_edges);
            self.vertices.extend(last_two_verts);
        }

        Ok(())
    }

    // ---- AdjustTopology --------------------------------------------------------------

    /// `AdjustTopology` (virtual): the Nx2 override; the convex (and concave) `NxN`
    /// override is unported; every other kind does nothing.
    ///
    /// # Errors
    ///
    /// Those of [`FilletCorner::adjust_topology_nx2`]; [`SolidError::Unported`]
    /// (`SmFilletConvexNxNCorner::AdjustTopology`).
    pub fn adjust_topology(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        match self.kind {
            FilletCornerKind::NByTwo => self.adjust_topology_nx2(k, env),
            FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN => {
                // UNPORTED(blend.fillet_corner_p2): SmFilletConvexNxNCorner::AdjustTopology
                Err(unported(
                    "SmFilletConvexNxNCorner::AdjustTopology",
                    "blend.fillet_corner_p2",
                ))
            }
            _ => Ok(()),
        }
    }

    /// `SmFilletNx2Corner::AdjustTopology`: where the rails of the two fillets meet the
    /// side edge at different points of a bevel corner, splits the
    /// `SM_FV_RAIL_X_EDGEUSE` vertex in two (the farther point stays on the side edge,
    /// a new `SM_FV_FILLET_X_FILLET` vertex goes on the side face) joined by new fillet
    /// edges. Blending corners are checked for the big-radius case instead.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for more than four edges around the corner;
    /// [`SolidError::InvalidInput`] for an unset `m_pAllEUs`, a classification object
    /// that is not an edge use, a missing rail vertex use or side edge curve (upstream
    /// dereferences them); [`SolidError::Unported`] (module docs).
    pub fn adjust_topology_nx2(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        // Can't do it if more than four total edges
        let all_eus = self
            .all_eus
            .as_ref()
            .ok_or_else(|| invalid(OP_TOPO, "m_pAllEUs is NULL"))?;
        if all_eus.len() > 4 {
            return Err(SolidError::sm_err(OP_TOPO));
        }

        // Check for mixed-convexity case: special routine for that.
        let num_vertex_edges = self.edges.len();
        if num_vertex_edges == 3 {
            // Mixed-convexity corners
            // UNPORTED(blend.fillet_corner_p2): SmFilletNx2Corner::AdjustConcaveCornerTopology
            return Err(unported(
                "SmFilletNx2Corner::AdjustConcaveCornerTopology",
                "blend.fillet_corner_p2",
            ));
        }

        let mut ii = 0usize;
        while ii < self.vertices.len() {
            let adjust_vert = *self
                .vertices
                .get(ii)
                .ok_or_else(|| invalid(OP_TOPO, "vertex index"))?;

            // Check whether this is the vertex that corresponds to the
            // rail hitting the side edge
            if env.pseudo.fillet_vertex(adjust_vert)?.fillet_vertex_type()
                != FilletVertexType::RailXEdgeuse
            {
                ii += 1;
                continue;
            }

            let geoms = env.pseudo.vertex_fillet_geoms(adjust_vert)?;
            let [fillet_geom1, fillet_geom2] = geoms[..] else {
                ii += 1;
                continue; // maybe already adjusted...
            };
            let Some(EntityRef::Edgeuse(side_eu1)) =
                env.pseudo.fillet_vertex(adjust_vert)?.point_class_object()
            else {
                return Err(invalid(OP_TOPO, "classification object is not an edge use"));
            };
            let vu1 = env
                .pseudo
                .vu_at_rail_end(adjust_vert, Some(fillet_geom1), None, None)?
                .ok_or_else(|| invalid(OP_TOPO, "no rail vertex use"))?;
            let vu2 = env
                .pseudo
                .vu_at_rail_end(adjust_vert, Some(fillet_geom2), Some(vu1), None)?
                .ok_or_else(|| invalid(OP_TOPO, "no rail vertex use"))?;
            let edge_param1 = env
                .pseudo
                .fillet_vertexuse(vu1)?
                .tsect_pnt()
                .curve_parameter;
            let edge_param2 = env
                .pseudo
                .fillet_vertexuse(vu2)?
                .tsect_pnt()
                .curve_parameter;
            let side_edge = eu1::edge(env.target, side_eu1)?
                .ok_or_else(|| invalid(OP_TOPO, "side edge use without edge"))?;
            let side_edge_curve = env
                .target
                .edge(side_edge)?
                .curve
                .as_ref()
                .ok_or_else(|| invalid(OP_TOPO, "side edge without curve"))?;

            let pnt1 = unchecked_point(side_edge_curve.evaluate_point(k, edge_param1))?;
            let pnt2 = unchecked_point(side_edge_curve.evaluate_point(k, edge_param2))?;

            let dist = pnt1.distance(pnt2);

            // Check for the big-radius case
            if self.blending {
                // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::TestAndConvertBigRadFillet
                return Err(unported(
                    "SmFilletExecutive::TestAndConvertBigRadFillet",
                    "blend.fillet_executive_p1",
                ));
            }

            // SM_ASSERT(m_bBevel == TRUE); Bevel cases

            // If rails hit side edge at same point, no adjustment is needed.
            if dist < self.approx_tol_3d {
                ii += 1;
                continue;
            }

            self.topology_adjusted = true;

            let vert_pnt = env
                .target
                .vertex(
                    self.vertex
                        .ok_or_else(|| invalid(OP_TOPO, "m_cpVertex is NULL"))?,
                )?
                .point;
            let (side_face, vu_on_side_face, small_rad_geom, large_rad_geom, pnt_on_side_face);

            // We'll split this vertex and keep the farther point (along the edge)
            if pnt1.distance(vert_pnt) < pnt2.distance(vert_pnt) {
                set_vertex_point(env.pseudo, adjust_vert, pnt2)?;
                env.pseudo
                    .fillet_vertex_mut(adjust_vert)?
                    .set_original_t_param(edge_param2);

                side_face = eu1::face(env.target, side_eu1)?;
                vu_on_side_face = vu1;
                small_rad_geom = fillet_geom1;
                large_rad_geom = fillet_geom2;
                pnt_on_side_face = pnt1;
            } else {
                set_vertex_point(env.pseudo, adjust_vert, pnt1)?;
                env.pseudo
                    .fillet_vertex_mut(adjust_vert)?
                    .set_original_t_param(edge_param1);

                let radial = eu1::radial(env.target, side_eu1)?
                    .ok_or_else(|| invalid(OP_TOPO, "side edge use without radial partner"))?;
                side_face = eu1::face(env.target, radial)?;
                vu_on_side_face = vu2;
                small_rad_geom = fillet_geom2;
                large_rad_geom = fillet_geom1;
                pnt_on_side_face = pnt2;
            }

            // The vert that is farther from the filleted vertex is now done.
            env.pseudo
                .fillet_vertex_mut(adjust_vert)?
                .set_status(FilStatus::Processed);

            let vert_on_side_edge = adjust_vert;

            // Create new fillet vertex on the 'side' face
            let vert_on_side_face = env.pseudo.brep_mut().add_vertex(pnt_on_side_face)?;
            let mut fv = FilletVertex::new(env.k, env.corner);
            fv.set_point_class(PointClassType::Face, side_face.map(EntityRef::Face));
            fv.set_fillet_vertex_type(FilletVertexType::FilletXFillet);
            env.pseudo.insert_fillet_vertex(vert_on_side_face, fv);

            let mut insert_after = true;
            let mut index = ii + 1;
            let other_vert = if index < self.vertices.len() {
                *self
                    .vertices
                    .get(index)
                    .ok_or_else(|| invalid(OP_TOPO, "vertex index"))?
            } else {
                // Insert new vertex before i
                index -= 1;
                insert_after = false;
                *index
                    .checked_sub(1)
                    .and_then(|i| self.vertices.get(i))
                    .ok_or_else(|| invalid(OP_TOPO, "vertex index"))?
            };

            self.vertices.insert(index, vert_on_side_face);

            // Find the vertexuse of rail which is on the
            // 'side' face and reconnect it to the newly created vertex.
            move_vertexuse(
                env.pseudo,
                vu_on_side_face,
                vert_on_side_edge,
                vert_on_side_face,
            )?;

            // Recreate fillet edges
            let _ = (insert_after, other_vert, small_rad_geom, large_rad_geom);
            // UNPORTED(blend.fillet_geom_p5): SmFilletEdge::MakeFilletEdge
            return Err(unported(
                "SmFilletEdge::MakeFilletEdge",
                "blend.fillet_geom_p5",
            ));
        }

        // all done
        Ok(())
    }

    // ---- AdjustTangentCorner ---------------------------------------------------------

    /// `AdjustTangentCorner` (virtual): the Nx2 override; the closed Nx1 override is
    /// unported; every other kind does nothing.
    ///
    /// # Errors
    ///
    /// Those of [`FilletCorner::adjust_tangent_corner_nx2`];
    /// [`SolidError::Unported`] (`SmFilletNx1ClosedCorner::AdjustTangentCorner`).
    pub fn adjust_tangent_corner(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        match self.kind {
            FilletCornerKind::NByTwo => self.adjust_tangent_corner_nx2(k, env),
            FilletCornerKind::NByOneClosed => {
                // UNPORTED(blend.fillet_corner_p2): SmFilletNx1ClosedCorner::AdjustTangentCorner
                Err(unported(
                    "SmFilletNx1ClosedCorner::AdjustTangentCorner",
                    "blend.fillet_corner_p2",
                ))
            }
            _ => Ok(()),
        }
    }

    /// `SmFilletNx2Corner::AdjustTangentCorner`: for a 4x2 corner whose two filleted
    /// edges are tangent at the vertex (within 2 degrees), checks that the two fillets'
    /// surfaces meet where their rails hit the side edges; when they do not, splits the
    /// fillet geometries at the side edges so another fillet surface fills the gap.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] / `SM_ERR_NULL_POINTER` where upstream dereferences
    /// null (a missing rail, rail vertex use or intersection surface);
    /// [`SolidError::Unported`] (module docs); those of the evaluations.
    pub fn adjust_tangent_corner_nx2(&mut self, k: &Kernel, env: &mut CornerEnv<'_>) -> Result<()> {
        // First check that this is the appropriate type.
        // Must be Nx2 tangent corner, and currently N must be 4.
        let Some(&[eu_a, eu_b]) = self.solver_eus.as_deref() else {
            return Ok(());
        };
        if self.all_eus.as_ref().is_none_or(|v| v.len() != 4) {
            return Ok(());
        }

        // Check that the two filleted edges are tangent at this vertex.
        // (the status is ignored upstream: a failed evaluation leaves the derivative
        // default-constructed)
        let deriv = |env: &mut CornerEnv<'_>, eu: EdgeuseId| -> Result<Vec3> {
            match eu3::normalized_evaluate(env.k, env.target, eu, 0.0, false, true, true) {
                Ok(ev) => Ok(ev.first_deriv.unwrap_or(Vec3::UNDEF)),
                Err(e) if e.is_unported() => Err(e),
                Err(_) => Ok(Vec3::UNDEF),
            }
        };
        let first_deriv1 = deriv(env, eu_a)?;
        let first_deriv2 = deriv(env, eu_b)?;

        let ang_tol_deg = 2.0; // cbi: some places use 5 degrees; sort that all out.
        if !first_deriv1.is_parallel_to(first_deriv2, ang_tol_deg) {
            return Ok(());
        }

        // Ok, we are an Nx2-tangent corner.
        let mut close_enough = true;

        for &adjust_vert in &self.vertices {
            if env.pseudo.fillet_vertex(adjust_vert)?.fillet_vertex_type()
                != FilletVertexType::RailXEdgeuse
            {
                continue;
            }

            let geoms = env.pseudo.vertex_fillet_geoms(adjust_vert)?;
            let [fillet_geom1, fillet_geom2] = geoms[..] else {
                continue; // maybe already adjusted...
            };

            // We need the rail curves to work from.
            // If not there yet, just wait for a later call. (Upstream's `||` chain
            // short-circuits: the first missing curve returns before a later rail is
            // looked at.)
            for g in [fillet_geom1, fillet_geom2] {
                for r in 0..2 {
                    let rail = env
                        .pseudo
                        .geom(g)?
                        .rail(r)
                        .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
                    if env.pseudo.brep().edge(rail)?.curve.is_none() {
                        return Ok(());
                    }
                }
            }

            // Get the vertexuses corresponding to the two surfaces, and their TSectPnts:
            let vu1 = env
                .pseudo
                .vu_at_rail_end(adjust_vert, Some(fillet_geom1), None, None)?
                .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
            let vu2 = env
                .pseudo
                .vu_at_rail_end(adjust_vert, Some(fillet_geom2), Some(vu1), None)?
                .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
            let int_pt1 = env.pseudo.fillet_vertexuse(vu1)?.tsect_pnt().clone();
            let int_pt2 = env.pseudo.fillet_vertexuse(vu2)?.tsect_pnt().clone();

            let user_surface = |vu: VertexuseId, i: usize| -> Result<Surface> {
                let mut s = env
                    .tsect_surfaces
                    .get(&vu)
                    .and_then(|s| s.get(i))
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
                // SmTemporaryChangeValue: GetOutOfBoundsEnabled() = TRUE on every
                // SmBSplineSurface (SM_CAST_PTR also succeeds for the analytic kinds
                // derived from it: SmPlane, SmSurfOfRevolution)
                match s.kind_mut() {
                    SurfaceKind::BSpline(b) => *b.out_of_bounds_enabled_mut() = true,
                    SurfaceKind::Plane(p) => *p.out_of_bounds_enabled_mut() = true,
                    SurfaceKind::SurfOfRevolution(r) => {
                        *r.base_mut().out_of_bounds_enabled_mut() = true;
                    }
                    SurfaceKind::Cone(c) => {
                        *c.base_mut().out_of_bounds_enabled_mut() = true;
                    }
                    SurfaceKind::CurveBounded(_)
                    | SurfaceKind::Step(_)
                    | SurfaceKind::OffsetSurface(_)
                    | SurfaceKind::SrfInVolume(_) => {}
                }
                Ok(s)
            };
            let srf1a = user_surface(vu1, 0)?;
            let srf1b = user_surface(vu1, 1)?;
            let srf2a = user_surface(vu2, 0)?;
            let srf2b = user_surface(vu2, 1)?;

            let srf1_pt =
                unchecked_point(srf1a.evaluate_point(k, int_pt1.uv_pos(SrfIndex::First)))?;
            let srf2_pt =
                unchecked_point(srf2a.evaluate_point(k, int_pt2.uv_pos(SrfIndex::First)))?;

            if srf1_pt.distance(srf2_pt) > self.approx_tol_3d {
                close_enough = false;
                break;
            }

            // The mates must agree as well for 4x2
            let srf1_pt =
                unchecked_point(srf1b.evaluate_point(k, int_pt1.uv_pos(SrfIndex::Second)))?;
            let srf2_pt =
                unchecked_point(srf2b.evaluate_point(k, int_pt2.uv_pos(SrfIndex::Second)))?;

            if srf1_pt.distance(srf2_pt) > self.approx_tol_3d {
                close_enough = false;
                break;
            }
        }

        if close_enough {
            return Ok(());
        }

        // Ok, we have to make a new fillet face in between.
        // When we process a vertex, we don't want to process its mate.
        let mut fvs_to_process = self.vertices.clone();

        let mut which_fv = 0usize;
        while which_fv < fvs_to_process.len() {
            let this_fv = *fvs_to_process
                .get(which_fv)
                .ok_or_else(|| invalid(OP_TANGENT, "vertex index"))?;

            // Looking for a vertex where a rail hits a side edge:
            if env.pseudo.fillet_vertex(this_fv)?.fillet_vertex_type()
                != FilletVertexType::RailXEdgeuse
            {
                which_fv += 1;
                continue;
            }

            let geoms = env.pseudo.vertex_fillet_geoms(this_fv)?;
            let [geom0, geom1] = geoms[..] else {
                which_fv += 1;
                continue;
            };

            // This Fillet Vertex has two Fillet Vertexuses, one on each FilletGeom.
            let vu0 = env
                .pseudo
                .vu_at_rail_end(this_fv, Some(geom0), None, None)?;
            let vu1 = env.pseudo.vu_at_rail_end(this_fv, Some(geom1), vu0, None)?;
            let vus = [vu0, vu1];

            let vertex_pos = env.pseudo.vertex_point(this_fv)?;

            // This is something we'll need after the next loop:
            let mut opposite_fv: Option<VertexId> = None;

            // Look at both FilletGeoms
            for which_fillet_geom in 0..2usize {
                let orig_geom = geoms
                    .get(which_fillet_geom)
                    .copied()
                    .ok_or_else(|| invalid(OP_TANGENT, "geometry index"))?;
                let this_fv_use = vus.get(which_fillet_geom).copied().flatten();

                // Grap the correct rail
                let rail_idx = env.pseudo.rail_index_of_vertex(orig_geom, Some(this_fv))?;
                let this_rail = env
                    .pseudo
                    .geom(orig_geom)?
                    .rail(rail_idx)
                    .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;

                let rail_ivl = env.pseudo.brep().edge(this_rail)?.interval;
                let rail_curve = env
                    .pseudo
                    .brep()
                    .edge(this_rail)?
                    .curve
                    .clone()
                    .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
                let drop = rail_curve.drop_point(
                    env.k,
                    &rail_ivl,
                    vertex_pos,
                    None,
                    SM_BIG_DOUBLE,
                    None,
                    SolverOperation::Minimize,
                );
                // the status is ignored upstream (bSuccess stays unset on a failure):
                // a failed drop counts as no drop point
                let drop = match drop {
                    Ok(d) => d,
                    Err(e) if e.is_unported() => return Err(e),
                    Err(_) => continue,
                };
                if !drop.success {
                    continue;
                }
                let split_param = drop.parameter;

                if rail_ivl
                    .is_value_on_boundary(split_param, self.approx_tol_3d)
                    .0
                {
                    continue;
                }

                // Which end of the FilletGeom are we at?  0 for start, 1 for end.
                let which_end =
                    usize::from(env.pseudo.rail_vertexuse(orig_geom, rail_idx, 1)? == this_fv_use);

                if which_end == 0 {
                    env.pseudo
                        .geom_mut(orig_geom)?
                        .set_fillet_geom_type(FilletGeomType::TangentRollover);
                }

                // SmFilletSolver::SplitFilletGeomAtSideEdge is ported
                // (`FilletSolver::split_fillet_geom_at_side_edge`, blend.fillet_solver_p1),
                // but on the solver's `FilletGeomRef` stand-in, which carries no geometry
                // data, and its first call is GetRailIndexOfVertexuse
                // (blend.fillet_geom_p6). Wire it once fillet_geom_p6 lands and the solver
                // works on `FilletGeomId`.
                // UNPORTED(blend.fillet_geom_p6): SmFilletSolver::SplitFilletGeomAtSideEdge
                let new_geom: FilletGeomId = Err(unported(
                    "SmFilletSolver::SplitFilletGeomAtSideEdge",
                    "blend.fillet_geom_p6",
                ))?;

                env.pseudo
                    .fillet_vertex_mut(this_fv)?
                    .set_fillet_vertex_type(FilletVertexType::Mate);
                env.pseudo
                    .fillet_vertex_mut(this_fv)?
                    .set_status(FilStatus::Unprocessed);

                // Set it as mate of its opposite fillet vertex.
                let other_rail = 1usize
                    .checked_sub(rail_idx)
                    .ok_or_else(|| invalid(OP_TANGENT, "rail index"))?;
                let opposite_fvu = if which_end == 0 {
                    env.pseudo.rail_vertexuse(orig_geom, other_rail, 0)?
                } else {
                    env.pseudo.rail_vertexuse(new_geom, other_rail, 1)?
                };
                let opposite_fvu =
                    opposite_fvu.ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
                opposite_fv = env.pseudo.brep().vertexuse(opposite_fvu)?.vertex;

                if let Some(opp) = opposite_fv {
                    env.pseudo
                        .fillet_vertex_mut(this_fv)?
                        .set_mate(0, Some(opp));
                    env.pseudo
                        .fillet_vertex_mut(opp)?
                        .set_mate(0, Some(this_fv));
                    env.pseudo
                        .fillet_vertex_mut(opp)?
                        .set_status(FilStatus::Unprocessed);
                }

                // Also, pThisFV is now in a face.
                let new_rail = env
                    .pseudo
                    .geom(new_geom)?
                    .rail(rail_idx)
                    .ok_or_else(|| SolidError::null_pointer(OP_TANGENT))?;
                let new_base_face = env.pseudo.fillet_edge(new_rail)?.original_face();
                env.pseudo
                    .fillet_vertex_mut(this_fv)?
                    .set_point_class(PointClassType::Face, new_base_face.map(EntityRef::Face));

                // Edges are kaput too.
                for &e in &self.edges {
                    env.pseudo
                        .fillet_edge_mut(e)?
                        .set_status(FilStatus::Unprocessed);
                }

                // UNPORTED(blend.fillet_geom_p6): SmFilletGeom::GetRailVertex
                return Err(unported(
                    "SmFilletGeom::GetRailVertex",
                    "blend.fillet_geom_p6",
                ));
            }

            // Remove the opposite-rail FilletVertex from the list to be processed.
            if let Some(opp) = opposite_fv {
                if let Some(idx) = fvs_to_process.iter().position(|&v| v == opp) {
                    fvs_to_process.remove(idx);
                    if idx <= which_fv {
                        // if we removed one before this one, slide us back one.
                        // (upstream's unsigned decrement wraps from 0 and the loop
                        // increment brings it back)
                        if which_fv == 0 {
                            continue;
                        }
                        which_fv -= 1;
                    }
                }
            }
            which_fv += 1;
        }

        Ok(())
    }

    // ---- unported members of other parts ---------------------------------------------

    /// `SmFilletCorner::MakeAnalyticCornerSurface` (unit `blend.fillet_corner_p2`).
    #[allow(clippy::unused_self)]
    fn make_analytic_corner_surface(&self, _env: &CornerEnv<'_>) -> Result<bool> {
        // UNPORTED(blend.fillet_corner_p2): SmFilletCorner::MakeAnalyticCornerSurface
        Err(unported(
            "SmFilletCorner::MakeAnalyticCornerSurface",
            "blend.fillet_corner_p2",
        ))
    }

    /// `SmFilletCorner::CreateEndPatch` (unit `blend.fillet_corner_p1`).
    #[allow(clippy::unused_self)]
    fn create_end_patch(&self, _env: &CornerEnv<'_>) -> Result<()> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::CreateEndPatch
        Err(unported(
            "SmFilletCorner::CreateEndPatch",
            "blend.fillet_corner_p1",
        ))
    }

    /// `SmFilletCorner::MakeFaceBrep` (unit `blend.fillet_corner_p1`).
    #[allow(clippy::unused_self)]
    fn make_face_brep(&self, _env: &CornerEnv<'_>) -> Result<()> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::MakeFaceBrep
        Err(unported(
            "SmFilletCorner::MakeFaceBrep",
            "blend.fillet_corner_p1",
        ))
    }
}

/// An evaluation whose status upstream ignores (`EvaluatePoint` without `SER`): a
/// failure leaves the default-constructed point ([`Point3::UNDEF`]); an unported
/// evaluator still propagates.
fn unchecked_point(r: Result<Point3>) -> Result<Point3> {
    match r {
        Ok(p) => Ok(p),
        Err(e) if e.is_unported() => Err(e),
        Err(_) => Ok(Point3::UNDEF),
    }
}

/// `SmBSplineCurve::JoinWith` (virtual) on an edge's curve: the `SmLine` and `SmEllipse`
/// overrides for those kinds, the B-spline join on the `SmBSplineCurve` part otherwise.
fn join_with(
    k: &Kernel,
    this: &mut Curve,
    this_at_end: bool,
    other: &mut Curve,
    other_at_end: bool,
) -> Result<()> {
    let owner = this.owner();
    match this.kind_mut() {
        CurveKind::Line(l) => {
            l.join_with(u64::from(this_at_end), other, u64::from(other_at_end), None)
        }
        CurveKind::Ellipse(e) => e.join_with(
            k,
            u64::from(this_at_end),
            other,
            u64::from(other_at_end),
            None,
            owner,
        ),
        kind => {
            let this_base =
                bspline_part_mut(kind).ok_or_else(|| SolidError::null_pointer(OP_3X2))?;
            let other_base = bspline_part_mut(other.kind_mut())
                .ok_or_else(|| SolidError::null_pointer(OP_3X2))?;
            this_base.join_with(k, this_at_end, other_base, other_at_end, None)
        }
    }
}

/// The `SmBSplineCurve` part of a curve, mutable (mirrors [`bspline_part`]).
fn bspline_part_mut(kind: &mut CurveKind) -> Option<&mut BSplineCurve> {
    match kind {
        CurveKind::BSpline(c) => Some(c),
        CurveKind::Line(c) => Some(&mut c.base),
        CurveKind::Ellipse(c) => Some(c.base_mut()),
        CurveKind::Hyperbola(c) => Some(c.base_mut()),
        CurveKind::Parabola(c) => Some(c.base_mut()),
        _ => None,
    }
}

/// `sm_TestG1Continuity` (unit `blend.fillet_corner_p1`).
fn test_g1_continuity(
    _b: &Brep,
    _vertex: VertexId,
    _edge1: EdgeId,
    _edge2: EdgeId,
    _ang_tol_deg: f64,
) -> Result<bool> {
    // UNPORTED(blend.fillet_corner_p1): sm_TestG1Continuity
    Err(unported("sm_TestG1Continuity", "blend.fillet_corner_p1"))
}

/// `SmFilletNx1Corner::CheckNx1Degeneracy` (unit `blend.fillet_corner_p1`).
fn check_nx1_degeneracy(_b: &Brep, _vertex: VertexId, _eu: EdgeuseId) -> Result<bool> {
    // UNPORTED(blend.fillet_corner_p1): SmFilletNx1Corner::CheckNx1Degeneracy
    Err(unported(
        "SmFilletNx1Corner::CheckNx1Degeneracy",
        "blend.fillet_corner_p1",
    ))
}

/// `sm_FindConcaveEdgeuses` (unit `blend.fillet_corner_p2`).
fn find_concave_edgeuses(
    _b: &Brep,
    _vertex: VertexId,
    _all: &[EdgeuseId],
) -> Result<Vec<EdgeuseId>> {
    // UNPORTED(blend.fillet_corner_p2): sm_FindConcaveEdgeuses
    Err(unported("sm_FindConcaveEdgeuses", "blend.fillet_corner_p2"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_map_to_upstream_types_and_names() {
        assert_eq!(
            FilletCornerKind::NByTwo.corner_type(),
            FilletCornerType::NByTwo
        );
        assert_eq!(
            FilletCornerKind::ConcaveNByN.corner_type(),
            FilletCornerType::NByNConcave
        );
        assert_eq!(FilletCornerKind::Open.name(), "SmFilletOpenCorner");
    }

    #[test]
    fn constructor_defaults() {
        let c = FilletCorner::new(FilletCornerKind::Degenerate, None, None, None, 1e-3, 0.1);
        assert!(c.degenerate);
        assert!(c.extended_surface_patch);
        assert!(!c.bevel && !c.blending && !c.chamfer);
        assert_eq!(c.set_back_dist, 0.0);
        let c = FilletCorner::new(FilletCornerKind::NByOne, None, None, None, 1e-3, 0.1);
        assert!(!c.degenerate);
    }

    #[test]
    fn calc_corner_geom_early_outs() {
        let k = Kernel::default();
        let mut target = Brep::new();
        let mut pseudo = FilletBrep::new();
        let surfaces = BTreeMap::new();
        let mut env = CornerEnv {
            k: &k,
            target: &mut target,
            pseudo: &mut pseudo,
            corner: None,
            tsect_surfaces: &surfaces,
        };
        let mut c = FilletCorner::new(FilletCornerKind::FourByThree, None, None, None, 1e-3, 0.1);
        assert!(c.calc_corner_geom(&k, &mut env).is_ok());
    }

    #[test]
    fn create_rejects_empty() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let v = b.add_vertex(Point3::new(0.0, 0.0, 0.0)).unwrap();
        let r = FilletCorner::create(&k, &mut b, v, &[], &FilletExecutiveData::default(), &[]);
        assert!(r.is_err());
    }
}
