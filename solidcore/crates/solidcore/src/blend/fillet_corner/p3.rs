//! Fillet corners, part 3: corner classification ([`FilletCorner::create`]), the corner
//! constructors, the corner patch builders (default, 3x2 mixed-convexity, Nx1 and Nx2)
//! and the Nx2 corner's topology adjustment. `AdjustTangentCorner` is the base class's
//! success at every corner: the Nx2 override (`AdjustTwoBlendTangentCorner`) went with
//! the fork's smlib#113 (34288e5), every blend corner event's is the base class's, and
//! every two-blend corner is a blend corner (#2400).
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp`, `source/SMLib/inc/SmFilletCorner.h`.
//!
//! | Rust                                               | Upstream                                              |
//! |----------------------------------------------------|-------------------------------------------------------|
//! | [`FilletCorner`], [`FilletCornerKind`], [`FilletCorner::new`] | `SmFilletCorner` members and constructor, the derived classes and their constructors (CO-PORTED from `blend.fillet_corner_p1` / `_p2`, see below) |
//! | [`FilletCorner::new`] with [`FilletCornerKind::ConcaveNByN`] | `SmFilletConcaveNxNCorner::SmFilletConcaveNxNCorner` |
//! | [`FilletCorner::new`] with [`FilletCornerKind::ConvexNByN`] | `SmFilletConvexNxNCorner::SmFilletConvexNxNCorner` |
//! | [`FilletCorner::new`] with [`FilletCornerKind::FourByThree`] | `SmFillet4x3Corner::SmFillet4x3Corner`        |
//! | [`FilletCorner::new`] with [`FilletCornerKind::Open`] | `SmFilletOpenCorner::SmFilletOpenCorner`           |
//! | [`FilletCorner::new`] with [`FilletCornerKind::Degenerate`] | `SmFilletDegenerateCorner::SmFilletDegenerateCorner` |
//! | [`FilletCorner::corner_type`], [`FilletCornerKind::name`] | `GetCornerType`, `GetName` (per derived class)   |
//! | [`FilletCorner::create`] | `SmFilletCorner::Create` (with `fixes.fillet_corner_per_vertex_fan`, the fork's knife-edge refusal of a two-edge corner, `e418750`, WR-665; not upstream, the refusal of a 3x2 mixed-convexity corner whose rails cross, WR-710) |
//! | [`FilletCorner::is_mitre`], [`FilletCorner::filleted_edgeuses`] | the fork's `SmFilletNx2Corner::IsMitre`, `GetFilletedEdgeuses` (`9ba2b12`, WR-634) |
//! | [`FilletCorner::create_with_solver_lookup`] | `Create` with borrowed executive solvers |
//! | [`FilletCorner::calc_corner_geom`]                 | `SmFilletCorner::CalcCornerGeom`                      |
//! | [`FilletCorner::create_corner_patch`]              | `CreateCornerPatch` (virtual: `SmFilletCorner`, `SmFillet3x2MixedConvexityCorner`, `SmFilletNx1Corner`, `SmFilletNx2Corner`) |
//! | [`FilletCorner::create_corner_patch_default_with`], [`CornerPatchEnv`], [`CornerSolverStore`] | `SmFilletCorner::CreateCornerPatch` on a caller's objects (the executive's members it reads) |
//! | [`FilletCorner::adjust_topology`]                  | `AdjustTopology` (virtual; `SmFilletNx2Corner::AdjustTopology` here) |
//! | [`FilletCorner::adjust_topology_nx2_with`], [`Nx2AdjustContext`] | `SmFilletNx2Corner::AdjustTopology` (against a live or legacy context) |
//! | [`FilletExecutiveData`]                            | the `SmFilletExecutive` members the corner code reads and writes (`m_vEdgeuseSolverMap`, `GetFilletSolverOfEdgeuse`; `m_vExtendedSurfacesMap` for unit `blend.fillet_corner_p1`; CO-PORTED from `blend.fillet_executive_p1`) |
//!
//! Deviations:
//! - **The fork's mitre gap patch** (WR-644, tangency-labs/smlib@2223108, opt-in
//!   `Kernel::fixes.fillet_mitre_gap_patch`): `adjust_topology_nx2` builds the concave
//!   corner's topology for a mitre whose rails meet the side edge apart
//!   ([`FilletCorner::bevel_gap_patch`]), and the Nx2 corner patch then fills only the gap.
//! - **Object model.** The class hierarchy is one [`FilletCorner`] record with a
//!   [`FilletCornerKind`]; the virtual methods dispatch on the kind. The pseudo-B-rep
//!   members (`m_vVertices`, `m_vEdges`) are ids of the executive's [`FilletBrep`];
//!   `m_cpVertex`, `m_pSolverEUs` and `m_pAllEUs` are ids of the B-rep being filleted.
//!   The executive and the fillet solvers are passed explicitly: the executive's
//!   edge-use-to-solver map as [`FilletExecutiveData`] with a solver slice, or a
//!   lookup borrowing its actual solver bases. A corner's identity (`this`, stored by
//!   new fillet vertices and edges) is [`CornerEnv::corner`]. The context is [`Kernel`].
//! - **Constructors.** `SmFilletCorner(...)` (unit `blend.fillet_corner_p1`) and the
//!   constructors of the derived classes owned by `blend.fillet_corner_p2`
//!   (`SmFilletNx1Corner`, `SmFillet3x2MixedConvexityCorner`, `SmFilletNx2Corner`, `SmFilletNxNCorner`) only
//!   initialise members: they are CO-PORTED here as [`FilletCorner::new`] so
//!   [`FilletCorner::create`] can build every kind. `m_eStatus` (never initialised
//!   upstream) starts as [`FilStatus::Unprocessed`]; `SmFilletNx2Corner`'s `m_bG1Case`
//!   (set by its `MakeCornerTopology`) starts as `false`.
//! - **Callees.** `MakeFaceBrep`, `sm_TestG1Continuity`,
//!   `SmFilletNx1Corner::CheckNx1Degeneracy`, `CreateEndPatch` and `CalcCornerVertGeom`
//!   are [`crate::blend::fillet_corner::p1`]'s. `NoteFilletError`
//!   ([`FilletExecutiveData::error`]) is CO-PORTED from `blend.fillet_executive_p1`.
//!   The corner methods take the kernel through [`CornerEnv::k`]
//!   (mutable: `MakeFaceBrep`'s `SmBrep::MakeFaceWithCurves` needs it). `MakeAnalyticCornerSurface` and
//!   the `SmFilletConvexNxNCorner` overrides are
//!   [`crate::blend::fillet_corner::p2`]'s, `SmFilletEdge::MakeFilletEdge` is
//!   `FilletBrep::make_fillet_edge` and `ReCalcFilletGeom` is
//!   `FilletBrep::re_calc_fillet_geom` on the corner's solvers.
//! - **Face sides** (not upstream; WR-613, the fork's tangency-labs/smlib@7fd9fb0): with
//!   `Kernel::fixes.corner_patch_face_side_twist`, `CreateCornerPatch` marks the
//!   ordered edges that have an original face and builds its patch with
//!   [`BSplineSurface::create_corner_blend_on_faces`]; without it the marks are `0`,
//!   upstream's `CreateCornerBlend`.
//! - **Nx2 adjustments.** The replaced corner edge of the Nx2 topology adjustment is
//!   deleted from the pseudo B-rep (`FilletBrep::delete_fillet_edge`, upstream's
//!   `delete`).
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
//!
//! The Nx1 selector reads a failed binormal status as a degenerate corner (the fork,
//! tangency-labs/smlib 66b02de, keeps it).
//!

use std::collections::BTreeMap;

use crate::blend::blend_corner::{BlendCornerEvent, BlendEnd};
use crate::blend::fillet_corner::p1;
use crate::blend::fillet_corner::p2::find_concave_edgeuses;
use crate::blend::fillet_executive::p3::ErrorInfo;
use crate::blend::fillet_geom::p1::{
    FilStatus, FilletBrep, FilletCornerId, FilletCornerType, FilletEdgeType, FilletErrorType,
    FilletGeomId, FilletSolverId, FilletVertex, FilletVertexType,
};
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverOps, FilletSurfaceGeneratorType};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignored, EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::{BlendNeighbour, OrientType};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::{p1 as eu1, p2 as eu2};
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId, VertexuseId};

const OP_CREATE: &str = "blend::fillet_corner::p3::create";
const OP_PATCH: &str = "blend::fillet_corner::p3::create_corner_patch";
const OP_3X2: &str = "blend::fillet_corner::p3::create_corner_patch_3x2";
const OP_NX2_PATCH: &str = "blend::fillet_corner::p3::create_corner_patch_nx2";
const OP_TOPO: &str = "blend::fillet_corner::p3::adjust_topology";

/// `SM_BIG_DOUBLE`.
const SM_BIG_DOUBLE: f64 = BIG_DOUBLE;

// ---- co-ported records --------------------------------------------------------------

/// The derived class of a fillet corner (`SmFilletCorner` is abstract upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilletCornerKind {
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

/// An object identity recorded in [`FilletExecutiveData::error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutiveErrorObject {
    /// A vertex of the B-rep being filleted (`m_cpVertex`).
    TargetVertex(VertexId),
    /// A fillet vertex of the pseudo B-rep (`SmFilletVertex*`).
    FilletVertex(VertexId),
    /// The fillet surface of a [`FilletGeomId`] (`pFilletGeom->GetFilletSurface()`).
    FilletGeomSurface(FilletGeomId),
    /// A target face's surface (`extended = false`) or registered extended surface
    /// (`extended = true`, `m_vExtendedSurfacesMap`).
    TargetFaceSurface {
        /// Target face id.
        face: FaceId,
        /// Whether the surface came from `extended_surfaces_map`.
        extended: bool,
    },
    /// The temporary planar end-face surface constructed by `CalcFilletIntSideFace`
    /// when `!corner.is_extended_surface_patch()`.
    TempEndPlane(EdgeId),
    /// The cap plane of an End at a smooth vertex (`GetEndCapSurface()`, the fork's
    /// 41a39ec), whose corner's end rib is this edge.
    EndCap(EdgeId),
}

/// The members of `SmFilletExecutive` read and written by the corner code (CO-PORTED from
/// `blend.fillet_executive_p1`).
#[derive(Debug, Clone, Default)]
pub struct FilletExecutiveData {
    /// `m_vEdgeuseSolverMap`: the fillet solver of each filleted edge use.
    pub edgeuse_solver_map: BTreeMap<EdgeuseId, FilletSolverId>,
    /// `m_vExtendedSurfacesMap`: the extended surface registered for a face of the
    /// original B-rep (`SmFilletCorner::AddExtendedOriginalSurface` /
    /// `GetExtendedSurface`, unit `blend.fillet_corner_p1`).
    pub extended_surfaces_map: BTreeMap<FaceId, Surface>,
    /// `m_bDoGlobalMerge` (read by `SmFilletCorner::CalcCornerEdgeGeom`).
    pub do_global_merge: bool,
    /// `m_vTopoEdges`: the fillet edges recorded for the topology insertion, in order
    /// (`None` keeps a null entry).
    pub topo_edges: Vec<Option<EdgeId>>,
    /// `m_sErrorInfo`: fillet error state recorded by `NoteFilletError` /
    /// `RecordFilletError`.
    pub error: ErrorInfo<ExecutiveErrorObject>,
    /// Corner arena indexed by [`FilletCornerId`] (`SmFilletCorner*` storage).
    pub corners: BTreeMap<FilletCornerId, FilletCorner>,
    /// `m_vFilletCorners`: ordered active corner ids in the executive.
    pub fillet_corners: Vec<FilletCornerId>,
    /// `m_vVertexCornerMap`: target B-rep vertex -> active corner id.
    pub vertex_corner_map: BTreeMap<VertexId, FilletCornerId>,
    /// `m_vFilletSolvers`: ordered active solver ids in the executive.
    pub fillet_solvers: Vec<FilletSolverId>,
    /// `m_vEarlierPassFaces` (the fork's, tangency-labs/smlib@9822462): the faces an
    /// earlier pass of the same blend made, for `EndAtSmoothVertex`.
    pub earlier_pass_faces: Vec<FaceId>,
    /// `m_pAttributeE->GetUsers()`: the fillet-B-rep edges carrying the executive's
    /// edge attribute (`SM_AI_ALL_FILLET_EDGES`), in the order they were added. The
    /// executive's constructor creates the attribute with no users; `CreateFilletBrep`
    /// adds it to every edge of the fillet B-rep.
    pub attribute_e_users: Vec<EdgeId>,
}

impl FilletExecutiveData {
    /// `SmFilletExecutive::GetFilletSolverOfEdgeuse`: the solver registered for `eu`,
    /// `None` when there is none.
    #[must_use]
    pub fn fillet_solver_of_edgeuse(&self, eu: EdgeuseId) -> Option<FilletSolverId> {
        self.edgeuse_solver_map.get(&eu).copied()
    }

    /// `SmFilletExecutive::RemoveFromTopoEdges` (CO-PORTED from
    /// `blend.fillet_executive_p1`): drops the first entry equal to `e`, if any.
    pub fn remove_from_topo_edges(&mut self, e: EdgeId) {
        if let Some(i) = self.topo_edges.iter().position(|&x| x == Some(e)) {
            self.topo_edges.remove(i);
        }
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
    /// `m_vSurfaces`: the surfaces built for the corner (`SM_FILLETSURF_TYPE` is
    /// `SmSurface`: an end patch copied with `CopyAndAddAnalytics` can be analytic).
    pub surfaces: Vec<Surface>,
    /// The fillet-B-rep faces `MakeFaceBrep` built on [`Self::surfaces`], in order
    /// (upstream the faces own the `m_vSurfaces` pointers, so the registered corner
    /// patch surfaces are those faces' surfaces).
    pub surface_faces: Vec<FaceId>,
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
    /// `m_bBevelGapPatch` (the fork's, WR-644, tangency-labs/smlib@2223108; `FALSE` from
    /// the constructor): a bevel (mitre) whose rails meet the side edge at two places, the
    /// gap between them on the side face's continuation filled by a patch
    /// (`SmFilletNx2Corner::AdjustTopology`). Set only with
    /// `fixes.fillet_mitre_gap_patch`.
    pub bevel_gap_patch: bool,
    /// `m_bExtendedSurfacePatch`.
    pub extended_surface_patch: bool,
    /// `m_dSetBackDist`.
    pub set_back_dist: f64,
    /// `m_dEaseDist` (the fork's, tangency-labs/smlib `e74bbd3`, #1376): the length along
    /// each edge before the setback over which each round's cross-section eases, by a C2
    /// law, from its own to the curvature-continuous section (the `G2` blend curve at
    /// thirds), and the corner's rail-rail curves meet the rails curvature-continuously.
    /// `0.0` (the default) is none: the pinned behaviour.
    pub ease_dist: f64,
    /// `m_dThisApproxTol3d`.
    pub approx_tol_3d: f64,
    /// `m_dTangencyTolRadians`.
    pub tangency_tol_radians: f64,
    /// `m_eStatus` (not initialised upstream).
    pub status: FilStatus,
    /// `m_bG1Case` of `SmFilletNx2Corner`.
    pub g1_case: bool,
    /// `m_bMitre` of `SmFilletNx2Corner` (the fork's, WR-634, tangency-labs/smlib@9ba2b12;
    /// `FALSE` from the constructor): the two blends are mitred at the common face's
    /// concave corner (`MakeCornerTopology`'s bevel at a corner of mixed convexity). Read
    /// by [`Self::is_mitre`].
    pub mitre: bool,
    /// The fork's blend corner (`SmBlendCorner`, smlib#79 step 4b; WR-697): its event and
    /// turn where the corner is one, `None` for the pinned corners that remain (the closed
    /// Nx1 corner's angle case, the 3x2 mixed-convexity, 4x3 and N x N corners). A blend
    /// corner is built by the Nx2 port (see [`crate::blend::blend_corner`]).
    pub blend: Option<crate::blend::blend_corner::BlendCorner>,
    /// The fork's Swept turn's own members (`m_pSideEU`, `m_pTurnSurface` and its loop;
    /// WR-697): set by its topology on a blend corner that is a Swept turn.
    pub swept: Option<crate::blend::blend_corner::swept::SweptTurn>,
    /// The fork's Ball's own members (`m_pBallSurface` and its loop; smlib#79 step 4c,
    /// WR-697): set by its topology on a blend corner that is a Ball.
    pub ball: Option<crate::blend::blend_corner::ball::BallCorner>,
    /// The blend corner's chamfer event (WR-716, #2345, `fixes.fillet_chamfer_corner`): its
    /// corner type, exact points and, until the patch takes it, the facet's face; set
    /// with [`Self::blend`] on a 3x3 chamfer corner that is the event.
    pub chamfer_corner: Option<crate::blend::blend_corner::chamfer::ChamferCorner>,
    /// The fork's Setback's own members (`m_vSetBacks`, `m_lFillOrder`, `m_bConvexSetBack`,
    /// `m_bSetBackOverflows`, `m_pFill`; smlib#104, WR-697): set at its creation on a blend
    /// corner that is a Setback.
    pub setback: Option<crate::blend::blend_corner::setback::SetbackCorner>,
    /// The fork's Mixed corner's own members (its ring and blends' solvers, and its two
    /// faces' surfaces and loops; smlib#82, WR-697): set by its topology on a blend corner
    /// that is a Mixed one.
    pub mixed: Option<crate::blend::blend_corner::mixed::MixedCorner>,
    /// The fork's `m_pEndCap` (smlib 41a39ec, WR-697): an End at a smooth vertex's plane
    /// square to its edge there (`GetEndCapSurface`), set by its topology.
    pub end_cap: Option<Surface>,
    /// Not upstream (solidcore-native): for each rail of an Nx1 end that ends inside the face
    /// it rolled onto ([`crate::blend::fillet_corner::topology::nx1_end::TurnedBack`]), the cap
    /// edge along the foot edge's continuation there (an `SM_FE_ON_EXTEND_EDGE` fillet edge).
    /// Such a cap has an edge for each piece, four or five in all
    /// ([`FilletCorner::turned_back_cap`]).
    pub(crate) turned_back_arcs: [Option<EdgeId>; 2],
}

/// What the corner methods operate on besides the corner itself.
#[derive(Debug)]
pub struct CornerEnv<'a> {
    /// `m_crContext`.
    pub k: &'a mut Kernel,
    /// The B-rep being filleted (`m_cpVertex`, the edge uses, original faces).
    pub target: &'a mut Brep,
    /// `m_pPseudoBrep`.
    pub pseudo: &'a mut FilletBrep,
    /// `m_pExecutive`'s members the corner methods read and write.
    pub exec: &'a mut FilletExecutiveData,
    /// The corner's identity (`this`, as new fillet vertices and edges store it).
    pub corner: Option<FilletCornerId>,
    /// The fillet solvers the geometries' `GetFilletSolver()` ids index (`id.0`).
    pub solvers: &'a mut [FilletSolver],
    /// `m_pExecutive->m_pFilletBrep`, the B-rep `MakeFaceBrep` builds the corner's faces
    /// in (`None`: `NULL`).
    pub fillet: Option<CornerFilletBrep<'a>>,
}

/// The solver store the corner-patch builders resolve fillet geometries' solvers
/// through (`pFilletGeom->GetFilletSolver()`, indexed by `FilletSolverId.0`).
pub trait CornerSolverStore {
    /// The solver at `index`, or `None` when it is not in the store.
    fn get(&self, index: usize) -> Option<&FilletSolver>;
    /// The number of solver slots (the identity a temporary solver takes).
    fn len(&self) -> usize;
    /// `GetSolverType` (virtual upstream) of the solver at `index`, or `None` when it is
    /// not in the store. A plain table has only the bases; a live session answers for its
    /// concrete solvers.
    ///
    /// # Errors
    /// Those of [`FilletSolverOps::solver_type`].
    fn solver_type(
        &self,
        index: usize,
    ) -> Option<Result<crate::blend::fillet_solver::p1::FilletSolverType>> {
        self.get(index).map(FilletSolverOps::solver_type)
    }
    /// Whether the store has no slots.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A plain solver table as a [`CornerSolverStore`].
#[derive(Debug, Clone, Copy)]
pub struct IndexedSolvers<'s>(pub &'s [FilletSolver]);

impl CornerSolverStore for IndexedSolvers<'_> {
    fn get(&self, index: usize) -> Option<&FilletSolver> {
        self.0.get(index)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

/// What the corner-patch builders (`CreateCornerPatch`, `MakeAnalyticCornerSurface`,
/// `MakeFaceBrep` and their helpers) operate on besides the corner itself: the
/// legacy [`CornerEnv`] ([`CornerEnv::with_patch_env`]) or a live executive session.
pub struct CornerPatchEnv<'e, 'a> {
    /// `m_crContext`.
    pub k: &'e mut Kernel,
    /// The B-rep being filleted.
    pub target: &'e Brep,
    /// `m_pPseudoBrep`.
    pub pseudo: &'e mut FilletBrep,
    /// The executive's solvers.
    pub solvers: &'e dyn CornerSolverStore,
    /// `m_pExecutive->m_pFilletBrep` (`None`: `NULL`).
    pub fillet: Option<&'e mut CornerFilletBrep<'a>>,
    /// `FILEXEC_CORNER_ERR(this, "Corner Face Creation Error")`: notes the failed
    /// corner face on the executive.
    pub note_face_error: &'e mut dyn FnMut(&FilletCorner),
}

impl std::fmt::Debug for CornerPatchEnv<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CornerPatchEnv")
            .field("target", &self.target)
            .field("pseudo", &self.pseudo)
            .field("fillet", &self.fillet)
            .finish_non_exhaustive()
    }
}

impl CornerEnv<'_> {
    /// Run `f` on this environment's corner-patch view (its executive's error notes
    /// the failed corner face).
    ///
    /// # Errors
    /// Those of `f`.
    pub fn with_patch_env<T>(
        &mut self,
        f: impl FnOnce(&mut CornerPatchEnv<'_, '_>) -> Result<T>,
    ) -> Result<T> {
        let CornerEnv {
            k,
            target,
            pseudo,
            exec,
            solvers,
            fillet,
            ..
        } = self;
        let mut note = |corner: &FilletCorner| {
            exec.error.note_corner_error(
                FilletErrorType::VertexProblem,
                corner.corner_type(),
                FilStatus::Failure,
                Some(corner.kind.name()),
                corner
                    .filleted_vertex()
                    .map(ExecutiveErrorObject::TargetVertex),
                [None, None], // For now, we don't generally have just two surfaces.
                [None, None], // For now, we don't generally have just two edgeuses.
                Some("Fillet Corner Error: Corner Face Creation Error"),
            );
        };
        let store = IndexedSolvers(solvers);
        let mut env = CornerPatchEnv {
            k,
            target,
            pseudo,
            solvers: &store,
            fillet: fillet.as_mut(),
            note_face_error: &mut note,
        };
        f(&mut env)
    }
}

/// The fillet B-rep of [`CornerEnv::fillet`] and the notification environment of its
/// entities (the face makers notify the new faces, edges and edge uses).
#[derive(Debug)]
pub struct CornerFilletBrep<'a> {
    /// The B-rep (`m_pFilletBrep`).
    pub brep: &'a mut Brep,
    /// The notification environment of `brep`'s entities.
    pub notify: edge::NotifyEnv<'a>,
}

/// What `SmFilletNx2Corner::AdjustTopology` reaches besides the corner itself.
pub trait Nx2AdjustContext {
    /// The kernel, the target B-rep, the pseudo-B-rep and the corner's identity.
    fn parts(&mut self) -> (&Kernel, &mut Brep, &mut FilletBrep, Option<FilletCornerId>);
    /// `SmFilletNx2Corner::AdjustConcaveCornerTopology` on `corner`.
    ///
    /// # Errors
    /// Those of the concave adjustment.
    fn adjust_concave_corner_topology(&mut self, corner: &mut FilletCorner) -> Result<()>;
}

impl Nx2AdjustContext for CornerEnv<'_> {
    fn parts(&mut self) -> (&Kernel, &mut Brep, &mut FilletBrep, Option<FilletCornerId>) {
        (&*self.k, &mut *self.target, &mut *self.pseudo, self.corner)
    }
    fn adjust_concave_corner_topology(&mut self, corner: &mut FilletCorner) -> Result<()> {
        corner.adjust_concave_corner_topology(self)
    }
}

fn invalid(op: &'static str, reason: &'static str) -> SolidError {
    SolidError::invalid_input(op, reason)
}

/// `SmVertex::SetPoint` on a pseudo-B-rep vertex: the notification resets the cached
/// gaps of every vertex use, then the point is stored.
pub(super) fn set_vertex_point(pseudo: &mut FilletBrep, v: VertexId, point: Point3) -> Result<()> {
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
pub(crate) fn move_vertexuse(
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

/// The primary edge use of a pseudo-B-rep edge (`GetPrimaryEdgeuse`), required.
pub(super) fn primary_eu(pseudo: &FilletBrep, e: EdgeId, op: &'static str) -> Result<EdgeuseId> {
    eu1::primary_edgeuse(pseudo.brep(), e)?
        .ok_or_else(|| SolidError::blend_missing(op, "the edge's primary edge use"))
}

/// The largest offset of `solver`'s surfaces (a round's radius, a chamfer's setback): how far
/// its ball's centre stands off the faces. `0` where it has no offset surface yet.
///
/// # Errors
///
/// Those of [`FilletSolver::surface`].
fn round_size(solver: &FilletSolver) -> Result<f64> {
    let mut size = 0.0_f64;
    for rail in 0..2 {
        if let Some(slot) = solver.surface(rail)? {
            size = size.max(slot.surface.offset_distance().abs());
        }
    }
    Ok(size)
}

impl FilletCorner {
    /// Not upstream (solidcore-native): whether this Nx1 end's cap runs along a foot edge's
    /// continuation inside the face a rail rolled onto ([`FilletCorner::turned_back_arcs`]).
    #[must_use]
    pub(crate) fn turned_back_cap(&self) -> bool {
        self.turned_back_arcs.iter().any(Option::is_some)
    }

    /// `SmFilletCorner(crContext, cpVertex, pSolverEUs, pAllEUs, pExec, dApproxTol3d,
    /// dTangencyTolRadians)` and the derived constructors (CO-PORTED, module docs):
    /// no fillet vertices, edges or surfaces, every flag off except
    /// `m_bExtendedSurfacePatch`, no setback; the degenerate corner sets
    /// `m_bDegenerate`; `m_bG1Case` starts `FALSE`. The closed Nx1 corner is not built:
    /// a closed edge meeting itself is the blend corner's Join or closed Turn (#2349).
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
            surface_faces: Vec::new(),
            bevel: false,
            blending: false,
            chamfer: false,
            // SmFilletDegenerateCorner: m_bDegenerate = TRUE
            degenerate: kind == FilletCornerKind::Degenerate,
            calc_as_const_rad: false,
            topology_adjusted: false,
            bevel_gap_patch: false,
            extended_surface_patch: true,
            set_back_dist: 0.0,
            ease_dist: 0.0,
            approx_tol_3d,
            tangency_tol_radians,
            status: FilStatus::Unprocessed,
            // SmFilletNx2Corner: m_bG1Case = FALSE until MakeCornerTopology
            g1_case: false,
            // SmFilletNx2Corner: m_bMitre = FALSE (the fork's, 9ba2b12)
            mitre: false,
            blend: None,
            swept: None,
            ball: None,
            chamfer_corner: None,
            setback: None,
            mixed: None,
            end_cap: None,
            turned_back_arcs: [None, None],
        }
    }

    /// `SmFilletNx2Corner::IsMitre` (the fork's, WR-634, tangency-labs/smlib@9ba2b12):
    /// whether the two blends are mitred at the common face's concave corner.
    #[must_use]
    pub const fn is_mitre(&self) -> bool {
        self.mitre
    }

    /// `SmFilletNx2Corner::GetFilletedEdgeuses` (the fork's, WR-634,
    /// tangency-labs/smlib@9ba2b12): the corner's two filleted edge uses, outward from its
    /// vertex.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without solver edge uses; `SM_ERR` unless there are
    /// exactly two.
    pub fn filleted_edgeuses(&self) -> Result<(EdgeuseId, EdgeuseId)> {
        const OP: &str = "blend::fillet_corner::p3::filleted_edgeuses";
        let eus = self
            .solver_eus
            .as_ref()
            .ok_or_else(|| SolidError::blend_missing(OP, "the corner's filleted edge uses"))?;
        match eus.as_slice() {
            [first, second] => Ok((*first, *second)),
            _ => Err(SolidError::blend_failed(
                OP,
                "the corner does not have exactly two filleted edge uses",
            )),
        }
    }

    /// `IsClosedJoin()` (virtual; the fork's 543d25a, WR-697): a blend corner's Join of
    /// one closed blended edge meeting itself. `FALSE` for every pinned corner.
    #[must_use]
    pub fn is_closed_join(&self) -> bool {
        self.blend
            .is_some_and(|blend| blend.is_closed_join(self.solver_eus.as_deref()))
    }

    /// Not the fork's (#2349): a blend corner's Turn of one closed blended edge meeting
    /// itself at an angle, Mitred ([`BlendCorner::is_closed_turn`]).
    ///
    /// [`BlendCorner::is_closed_turn`]: crate::blend::blend_corner::BlendCorner::is_closed_turn
    #[must_use]
    pub fn is_closed_turn(&self) -> bool {
        self.blend
            .is_some_and(|blend| blend.is_closed_turn(self.solver_eus.as_deref()))
    }

    /// One closed blended edge meeting itself: a closed Join or a closed Turn. Its blend
    /// is split in two at the corner, and the corner's edges hold both ends of it (where
    /// upstream tests `SM_FCR_N_x_1_CLOSED`, the fork its 1x1 corner's replacement,
    /// `IsClosedJoin`).
    #[must_use]
    pub fn meets_itself(&self) -> bool {
        self.is_closed_join() || self.is_closed_turn()
    }

    /// `IsEndOnEndFace()` (virtual; the fork's 53c47b8, WR-697): a blend corner's End on
    /// its end face(s). The fork asks it where pinned SMLib tests `SM_FCR_N_x_1`, so it
    /// holds for the pinned Nx1 corner too (the End on its end face is built by that
    /// corner's moved code, of its kind).
    #[must_use]
    pub fn is_end_on_end_face(&self) -> bool {
        match self.blend {
            Some(blend) => blend.is_end_on_end_face(),
            None => self.kind == FilletCornerKind::NByOne,
        }
    }

    /// `GetCornerType()`.
    #[must_use]
    pub const fn corner_type(&self) -> FilletCornerType {
        // the fork's SmBlendCorner::GetCornerType is SM_FCR_BLEND, which took over every
        // test of SM_FCR_N_x_2 one for one (74e9beb); the blend corner of any event is an
        // Nx2 corner by type here (WR-697). Not the chamfer event (WR-716, not the fork's):
        // it is closed by the convex NxN corner's topology, whose type it keeps, so the
        // executive's Nx2-only steps (the short rail squeeze, the Nx2 edge connectivity, a
        // mitred Turn's corner edges) do not take it for two blends meeting
        match self.blend {
            Some(blend) if !matches!(blend.event, BlendCornerEvent::Chamfer) => {
                FilletCornerType::NByTwo
            }
            _ => self.kind.corner_type(),
        }
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
    /// sector convexity test; [`SolidError::Unported`] (module docs). Not upstream
    /// (WR-710), [`SolidError::Unsupported`] with the reason [`MIXED_CORNER_RAILS_CROSS`] for a 3x2
    /// mixed-convexity corner whose two filleted edges leave the vertex along one
    /// tangent, so their rails cross before the corner. [`SolidError::Unsupported`] with
    /// the reason [`MIXED_CORNER_OF_FOUR_EDGES`] for a convex and a concave blend at a
    /// vertex of four edges that are not the blend corner's Mixed one (#10876).
    pub fn create(
        k: &Kernel,
        b: &mut Brep,
        vertex: VertexId,
        solver_eus: &[EdgeuseId],
        exec: &FilletExecutiveData,
        solvers: &[FilletSolver],
    ) -> Result<Option<Self>> {
        Self::create_with_solver_lookup(
            k,
            b,
            vertex,
            solver_eus,
            &exec.earlier_pass_faces,
            |edge_use| {
                exec.fillet_solver_of_edgeuse(edge_use)
                    .and_then(|id| usize::try_from(id.0).ok())
                    .and_then(|i| solvers.get(i))
                    .ok_or_else(|| invalid(OP_CREATE, "filleted edge use without fillet solver"))
            },
        )
    }

    /// `SmFilletCorner::Create` using the executive's live solver objects.
    ///
    /// `earlier_pass_faces` are the faces an earlier pass of the same blend made
    /// (`EndAtSmoothVertex`'s `cpExec`, the fork's 9822462); empty without one.
    /// `solver_of` resolves each filleted edge use to its actual borrowed solver
    /// base. Lookups occur in `solver_eus` order after the topology walk, where the
    /// native factory reads tolerances and the surface generator. No solver is copied.
    ///
    /// # Errors
    ///
    /// Those of [`Self::create`] and `solver_of`; the first lookup failure stops
    /// construction before any later solver is requested.
    pub fn create_with_solver_lookup<'solver>(
        k: &Kernel,
        b: &mut Brep,
        vertex: VertexId,
        solver_eus: &[EdgeuseId],
        earlier_pass_faces: &[FaceId],
        solver_of: impl Fn(EdgeuseId) -> Result<&'solver FilletSolver>,
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
            let solver = solver_of(edge_use)?;
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

        // the fork's SmBlendCorner(..., eEvent, SM_BT_MITRE), its End's kind set (SetEnd)
        let blend_corner = |kind, event, end| {
            let mut c = make(kind);
            c.blend = Some(crate::blend::blend_corner::BlendCorner {
                event,
                turn: crate::blend::blend_corner::BlendTurn::Mitre,
                end,
            });
            c
        };

        // switch on number of to-be-filleted edgeuses connected to vertex
        let corner = match total_solver_eus {
            0 => None,
            1 => {
                let eu0 = start_eu;
                let e = edge_of(b, eu0)?;
                if edge::is_closed(b, e)? {
                    // The fork's (WR-697, smlib#79 step 4d, 543d25a): a closed edge meeting
                    // itself G1 is the blend corner's Join (its shared section)
                    match total_eus {
                        1..=3 => {
                            if test_g1_continuity(k, b, vertex, e, e, tan_tol_deg)? {
                                Some(blend_corner(
                                    FilletCornerKind::NByTwo,
                                    BlendCornerEvent::Join,
                                    BlendEnd::OnEndFace,
                                ))
                            } else if total_eus == 1 {
                                None // MSG: Corner Case Not Implemented
                            } else {
                                // Not the fork's (#2349): at an angle it is the blend
                                // corner's Turn, Mitred: the blend meets itself on its
                                // mitre (in place of SmFilletNx1ClosedCorner)
                                Some(blend_corner(
                                    FilletCornerKind::NByTwo,
                                    BlendCornerEvent::Turn,
                                    BlendEnd::OnEndFace,
                                ))
                            }
                        }
                        _ => None, // MSG: Corner Case Not Implemented
                    }
                } else {
                    // The fork's (WR-697, 53c47b8): one blend ending at the vertex is the
                    // blend corner's End, its kind chosen here; the three kinds are the
                    // degenerate, open and Nx1 corners' constructions, moved unchanged
                    if check_nx1_degeneracy(k, b, vertex, eu0)? {
                        Some(blend_corner(
                            FilletCornerKind::Degenerate,
                            BlendCornerEvent::End,
                            BlendEnd::RunsOut,
                        ))
                    } else if is_open_end_corner {
                        Some(blend_corner(
                            FilletCornerKind::Open,
                            BlendCornerEvent::End,
                            BlendEnd::Open,
                        ))
                    } else if crate::blend::blend_corner::end::end_at_smooth_vertex(
                        k,
                        b,
                        vertex,
                        eu0,
                        &all_eus,
                        tan_tol_deg,
                        earlier_pass_faces,
                    )? {
                        // The fork's (41a39ec): a blended edge running on tangentially into
                        // an edge of its convexity not blended ends at the smooth vertex,
                        // cut square to its edge there (MakeEndAtSmoothVertex: its own
                        // construction, a blend corner of type SM_FCR_BLEND)
                        Some(blend_corner(
                            FilletCornerKind::NByTwo,
                            BlendCornerEvent::End,
                            BlendEnd::AtSmooth,
                        ))
                    } else {
                        Some(blend_corner(
                            FilletCornerKind::NByOne,
                            BlendCornerEvent::End,
                            BlendEnd::OnEndFace,
                        ))
                    }
                }
            }
            2 => {
                let eu0 = start_eu;
                let eu1_ = *solver_eus
                    .get(1)
                    .ok_or_else(|| invalid(OP_CREATE, "second filleted edge use"))?;
                // The fork's (WR-665, tangency-labs/smlib@e418750, opt-in
                // `fillet_corner_per_vertex_fan`): no two-edge corner is built where an edge
                // not blended there is a knife edge (its faces folded back on each other,
                // as where a hole's wall touches the bottom face along a line): the
                // corners take its faces to meet at an angle.
                let mut knife = false;
                if k.fixes.fillet_corner_per_vertex_fan {
                    let e0 = edge_of(b, eu0)?;
                    let e1 = edge_of(b, eu1_)?;
                    for &other in &all_eus {
                        if knife {
                            break;
                        }
                        let e = edge_of(b, other)?;
                        if e != e0
                            && e != e1
                            && crate::blend::fillet_solver::p5::is_knife_edge(
                                k,
                                b,
                                e,
                                tangency_tol_radians,
                            )?
                        {
                            knife = true;
                        }
                    }
                }
                if knife {
                    None // MSG: Corner Case Not Implemented
                } else {
                    match total_eus {
                        // two edges: we can handle this only if they continue each other
                        2 => {
                            let e0 = edge_of(b, eu0)?;
                            let e1 = edge_of(b, eu1_)?;
                            if test_g1_continuity(k, b, vertex, e0, e1, tan_tol_deg)? {
                                // The fork's (WR-697, 68e039c): two edges continuing each
                                // other alone at the vertex are the blend corner's Join
                                // (its shared section)
                                Some(blend_corner(
                                    FilletCornerKind::NByTwo,
                                    BlendCornerEvent::Join,
                                    BlendEnd::OnEndFace,
                                ))
                            } else {
                                None // MSG: Corner Case Not Implemented
                            }
                        }
                        3 | 4 => {
                            let opposite = total_eus == 3
                                && eu2::is_convex_radial_sector(k, b, eu0, 4, false, 0.0)?
                                    != eu2::is_convex_radial_sector(k, b, eu1_, 4, false, 0.0)?;
                            // The fork's (WR-697, 6fd0411; smlib#82): a convex and a
                            // concave blend continuing each other across a tangent
                            // seam are the blend corner's Mixed
                            // (SmBlendCorner::ClassifyMixed); of three edges a failed
                            // classification builds no corner. Four edges (not the
                            // fork's, #2953): the seam's line run on past the vertex as
                            // an edge; any other vertex of four goes on below.
                            let mixed = if opposite || total_eus == 4 {
                                let lookup = |eu: EdgeuseId| solver_of(eu).ok();
                                match crate::blend::blend_corner::mixed::classify_mixed(
                                    k,
                                    b,
                                    vertex,
                                    solver_eus,
                                    &all_eus,
                                    &lookup,
                                    tan_tol_deg,
                                ) {
                                    Ok(found) => Some(found),
                                    Err(e) if e.is_unported_or_cancelled() => return Err(e),
                                    Err(_) => None,
                                }
                            } else {
                                Some(false)
                            };
                            // Not upstream (#10876): of four edges, a convex and a concave
                            // blend that are not the Mixed corner have no corner. The
                            // Turn below is for blends of one convexity: on their common
                            // face a convex blend lies in the material and a concave one
                            // outside it, so the two meet only on that face, at any size,
                            // and there is no crossing to mitre them on.
                            if mixed != Some(true)
                                && total_eus == 4
                                && eu2::is_convex_radial_sector(k, b, eu0, 4, false, 0.0)?
                                    != eu2::is_convex_radial_sector(k, b, eu1_, 4, false, 0.0)?
                            {
                                return Err(SolidError::Unsupported {
                                    operation: OP_CREATE,
                                    reason: MIXED_CORNER_OF_FOUR_EDGES,
                                });
                            }
                            if mixed == Some(true) {
                                let mut c = make(FilletCornerKind::NByTwo);
                                c.blend = Some(crate::blend::blend_corner::BlendCorner {
                                    event: BlendCornerEvent::Mixed,
                                    turn: crate::blend::blend_corner::BlendTurn::Mitre,
                                    end: BlendEnd::OnEndFace,
                                });
                                return Ok(Some(c));
                            }
                            if opposite {
                                if mixed.is_none() {
                                    return Ok(None); // MSG: Corner Case Not Implemented
                                }
                                // Not upstream (WR-710, #2256):
                                // where the two edges leave the vertex along one tangent,
                                // their common face closes to a cusp there and the rails
                                // cross before the corner; the corner patch built across
                                // them leaves the faces, so the corner is refused.
                                if mixed_corner_rails_cross(
                                    k,
                                    b,
                                    vertex,
                                    edge_of(b, eu0)?,
                                    edge_of(b, eu1_)?,
                                    tan_tol_deg,
                                )? {
                                    return Err(SolidError::Unsupported {
                                        operation: OP_CREATE,
                                        reason: MIXED_CORNER_RAILS_CROSS,
                                    });
                                }
                                Some(make(FilletCornerKind::ThreeByTwoMixed))
                            } else {
                                // The fork's (WR-697, smlib#79 step 4b, 74e9beb): two blends of
                                // one convexity among three or four edges are the blend
                                // corner's (SmBlendCorner::Classify: a Turn or a Join), built
                                // by the two-blend construction moved from SmFilletNx2Corner
                                // the blends join only where their balls are one within
                                // the corner's tolerance (`blend_corner::join_tolerance_deg`)
                                let mut size = 0.0_f64;
                                for &eu in solver_eus {
                                    size = size.max(round_size(solver_of(eu)?)?);
                                }
                                let blend = crate::blend::blend_corner::classify(
                                    k,
                                    b,
                                    Some(vertex),
                                    solver_eus,
                                    &all_eus,
                                    tan_tol_deg,
                                    crate::blend::blend_corner::join_tolerance_deg(
                                        size,
                                        approx_tol_3d,
                                        tan_tol_deg,
                                    ),
                                )?;
                                let mut c = make(FilletCornerKind::NByTwo);
                                c.blend = Some(blend);
                                Some(c)
                            }
                        }
                        _ => None, // MSG: Corner Case Not Implemented
                    }
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
                        concave_eus = find_concave_edgeuses(k, b, vertex, &all_eus)?;
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
    pub fn calc_corner_geom(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        self.calc_corner_geom_with(env, |corner, env| corner.create_corner_patch(env))
    }

    /// Apply the native corner-patch predicate before lending the actual patch context.
    ///
    /// # Errors
    /// Those of the actual concrete patch operation when a patch is needed.
    pub fn calc_corner_geom_with<C>(
        &mut self,
        context: &mut C,
        create_patch: impl FnOnce(&mut Self, &mut C) -> Result<()>,
    ) -> Result<()> {
        let total_edges = self.edges.len();
        let fillet_edges = self.solver_eus.as_ref().map_or(0, Vec::len);

        // no work - Bevel corner or degenerate corner
        // (the fork's, WR-644: a bevel with a gap to fill on a side face's continuation,
        // `m_bBevelGapPatch`, builds the gap's patch)
        if (self.bevel && fillet_edges > 1 && !self.bevel_gap_patch) || total_edges < 3 {
            return Ok(());
        }

        // Otherwise, pass the call along to build a Corner->FilletSurface and store it in m_vSurfaces
        create_patch(self, context)
    }

    // ---- CreateCornerPatch -----------------------------------------------------------

    /// `CreateCornerPatch` (virtual): the 3x2 mixed-convexity, Nx1 (and open) and Nx2
    /// overrides, the default patch for every other kind.
    ///
    /// # Errors
    ///
    /// Those of the selected implementation.
    pub fn create_corner_patch(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        env.k
            .check_cancelled("blend::fillet_corner::p3::create_corner_patch")?;
        // the fork's SmBlendCorner::CreateCornerPatch: a Join has no patch of its own, and
        // every End's is CreateEndPatchOfEnd, the Nx1 corner's moved (WR-697, 543d25a,
        // 53c47b8)
        if self
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_join)
        {
            return Ok(());
        }
        // the fork's (41a39ec): an End at a smooth vertex's patch is its cap
        if self
            .blend
            .is_some_and(|blend| blend.is_end() && blend.end == BlendEnd::AtSmooth)
        {
            let built = env.with_patch_env(|env| {
                crate::blend::blend_corner::end::create_end_cap_patch(self, env)
            })?;
            if !built {
                // the fork notes the cap's failure (SM_FILERR_VERTEX_PROBLEM), as the
                // session's path does
                env.exec.error.note_corner_error(
                    FilletErrorType::VertexProblem,
                    self.corner_type(),
                    FilStatus::Failure,
                    Some(self.kind.name()),
                    self.filleted_vertex()
                        .map(ExecutiveErrorObject::TargetVertex),
                    [None, None],
                    [None, None],
                    Some(crate::blend::blend_corner::end::SMOOTH_END_CAP_FAILED),
                );
                return Err(SolidError::blend_failed(
                    "SmBlendCorner::CreateEndPatchOfEnd",
                    "the smooth end's cap patch could not be made",
                ));
            }
            return Ok(());
        }
        if self
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_end)
        {
            return self.create_corner_patch_nx1(env);
        }
        match self.kind {
            FilletCornerKind::ThreeByTwoMixed => self.create_corner_patch_3x2(env),
            FilletCornerKind::NByOne | FilletCornerKind::Open => self.create_corner_patch_nx1(env),
            FilletCornerKind::NByTwo => self.create_corner_patch_nx2(env),
            _ => self.create_corner_patch_default(env),
        }
    }

    /// `SmFilletCorner::CreateCornerPatch`: an analytic corner surface when one fits,
    /// else a three- or four-sided Coons patch over the corner's fillet edges (a
    /// three-sided one gets a degenerate point curve at the vertex selected for it),
    /// then the corner face (`MakeFaceBrep`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` after noting a vertex-problem fillet error when `MakeFaceBrep` fails;
    /// those of [`FilletCorner::make_analytic_corner_surface`]; `SM_ERR_NULL_POINTER`
    /// for a fillet edge without fillet geometry where one is needed;
    /// [`SolidError::InvalidInput`] where upstream dereferences null or indexes past the
    /// corner's arrays; those of the point curves and `CreateCornerBlend`.
    pub fn create_corner_patch_default(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        env.with_patch_env(|env| self.create_corner_patch_default_with(env))
    }

    /// [`FilletCorner::create_corner_patch_default`] against any [`CornerPatchEnv`].
    ///
    /// # Errors
    /// As [`FilletCorner::create_corner_patch_default`].
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn create_corner_patch_default_with(
        &mut self,
        env: &mut CornerPatchEnv<'_, '_>,
    ) -> Result<()> {
        // if an AnalyticSurface can approx FilletCorner->Curves to tol,
        //   Make and store that AnalyticSurface in m_vSurfaces, and return TRUE
        // else return FALSE.
        let success = self.make_analytic_corner_surface_with(env)?;

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
                            // the fork's IsEndOnEndFace() (53c47b8): the Nx1 corner's
                            // kind either way
                            FilletVertexType::OnVertex if self.kind == FilletCornerKind::NByOne => {
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

            // Not upstream (WR-613; tangency-labs/smlib@7fd9fb0): the edges along a face
            // of the body (an original face), bit k for ordered edge k: the patch meets
            // those exactly at its corners
            let mut face_sides: u32 = 0;

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
                        let mate_eu = mate_eu.ok_or_else(|| {
                            SolidError::blend_missing(OP_PATCH, "the corner edge use's mate")
                        })?;
                        ps_curve = eu1::uv_trim_curve(env.pseudo.brep(), mate_eu)?
                            .0
                            .and_then(bspline_part);
                        base_surface.clone_from(&env.target.face(orig_face)?.surface);
                        if env.k.fixes.corner_patch_face_side_twist {
                            face_sides |= u32::try_from(ii)
                                .ok()
                                .and_then(|ii| 1u32.checked_shl(ii))
                                .unwrap_or(0);
                        }
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
                            .ok_or_else(|| {
                                SolidError::blend_missing(
                                    OP_PATCH,
                                    "the corner edge use's fillet geometry",
                                )
                            })?;
                        base_surface = env.pseudo.geom(g)?.fillet_surface(None)?.cloned();
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
            // CreateCornerBlend checks `cr3DCurves.GetSize() < 4 -> SM_ERR` before it
            // touches any curve (a null one is SM_ERR_NULL_POINTER afterwards).
            if ordered_3d.len() < 4 {
                return Err(SolidError::blend_failed(
                    "nurbs::b_spline_surface_n_lib::p2::BSplineSurface::create_corner_blend",
                    "the corner patch has fewer than four boundary curves",
                ));
            }
            let mut curves_3d = Vec::with_capacity(ordered_3d.len());
            for c in &ordered_3d {
                let c = c
                    .as_ref()
                    .ok_or_else(|| SolidError::blend_missing(OP_PATCH, "a boundary curve"))?;
                curves_3d.push(
                    bspline_part(c)
                        .ok_or_else(|| invalid(OP_PATCH, "boundary curve is not a B-spline"))?,
                );
            }
            let surfs: Vec<Option<BlendNeighbour<'_>>> = ordered_surfs
                .iter()
                .map(|s| s.as_ref().map(|surface| BlendNeighbour { surface }))
                .collect();
            let blends = BSplineSurface::create_corner_blend_on_faces(
                env.k,
                &curves_3d,
                &orients,
                &ordered_uv,
                &surfs,
                self.approx_tol_3d,
                self.tangency_tol_radians,
                !self.extended_surface_patch,
                face_sides,
            )?;

            // store New FilletSurface in m_vSurfaces
            self.surfaces.extend(blends.into_iter().map(Surface::from));
        }

        // Create Face (and Edges and Verts) in m_pFilletBrep from m_vSurfaces with call m_pFilletBrep->MakeFaceWithCurves()
        match self.make_face_brep_with(env) {
            Ok(()) => {}
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(
                e @ SolidError::InvalidInput {
                    reason: p1::NULL_DEREF,
                    ..
                },
            ) => return Err(e),
            Err(_) => {
                // when MakeFaceBrep failed - note the error
                (env.note_face_error)(self);
                // FILEXEC_CORNER_ERR(this, "Corner Face Creation Error")
                return Err(SolidError::blend_failed(
                    OP_PATCH,
                    "the corner's face could not be made",
                ));
            }
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
    /// [`SolidError::Unported`] (the drop onto a surface kind whose `DropCurve` needs
    /// unported code, see [`Surface::drop_curve`]).
    pub fn create_corner_patch_3x2(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        env.with_patch_env(|env| self.create_corner_patch_3x2_with(env))
    }

    /// [`FilletCorner::create_corner_patch_3x2`] against any [`CornerPatchEnv`].
    ///
    /// # Errors
    /// As [`FilletCorner::create_corner_patch_3x2`].
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn create_corner_patch_3x2_with(&mut self, env: &mut CornerPatchEnv<'_, '_>) -> Result<()> {
        // check state - 4 edges
        if self.edges.len() != 4 {
            // SM_DBG_WARN: Failed state check: 3x2 FilletCorner with other than 4 edges
            return Err(SolidError::blend_failed(
                OP_3X2,
                "the 3x2 corner does not have exactly four edges",
            ));
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
        let fe_on_edge = fe_on_edge
            .ok_or_else(|| SolidError::blend_missing(OP_3X2, "the corner edge on the edge"))?;
        let fe_on_extended_edge = fe_on_extended_edge.ok_or_else(|| {
            SolidError::blend_missing(OP_3X2, "the corner edge on the extended edge")
        })?;

        // get onEdge->Vertices
        let corner_v = edge::vertex(env.pseudo.brep(), fe_on_edge)?
            .ok_or_else(|| invalid(OP_3X2, "on-edge fillet edge without vertex"))?;
        let surviving_fv = edge::other_vertex(env.pseudo.brep(), fe_on_edge, Some(corner_v))?;

        // check state
        if env.pseudo.fillet_vertex(corner_v)?.fillet_vertex_type() != FilletVertexType::OnVertex
            || edge::vertex(env.pseudo.brep(), fe_on_extended_edge)? == Some(corner_v)
        {
            // Internal error, check corner topology
            return Err(SolidError::blend_failed(
                OP_3X2,
                "the 3x2 corner's topology is not as expected",
            ));
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
                .ok_or_else(|| {
                    SolidError::blend_missing(OP_3X2, "the corner edge's B-spline curve")
                })
        };
        let mut curve1 = bspline_curve(env.pseudo, fe_on_extended_edge)?;
        let mut curve2 = bspline_curve(env.pseudo, fe_on_edge)?;

        // join the 2 curves (JoinWith is virtual: SmLine and SmEllipse override it)
        join_with(&*env.k, &mut curve1, true, &mut curve2, false)?;
        // upstream joins the edges' curve objects in place: the edits go back into the
        // same objects, which keep their edge owners
        for (e, edited) in [(fe_on_extended_edge, curve1.clone()), (fe_on_edge, curve2)] {
            let slot = &mut env.pseudo.brep_mut().edge_mut(e)?.curve;
            match slot {
                Some(c) => c.assign_edited(edited),
                None => *slot = Some(edited),
            }
        }

        // Combine two edges
        self.edges.remove(found_index);
        let Some(idx) = self.vertices.iter().position(|&v| v == corner_v) else {
            return Err(SolidError::blend_failed(
                OP_3X2,
                "the corner vertex is not among the corner's vertices",
            ));
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
            .ok_or_else(|| {
                SolidError::blend_missing(OP_3X2, "the extended edge's original face")
            })?;
        let surface = env
            .target
            .face(orig_face)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::blend_missing(OP_3X2, "the original face's surface"))?;
        // `pSurface->DropCurve(m_crContext, pSurface->GetNaturalUVDomain(), *pCurve1,
        // pCurve1->GetNaturalInterval(), m_dThisApproxTol3d, ..)` (virtual with the
        // defaults `bKeepAllDropCurves = TRUE`, no fail record): the `SmPlane`
        // override for a plane (with the surface's owner), the base body otherwise.
        let domain = surface.natural_uv_domain_with_nurb(env.k)?;
        let interval = curve1.natural_interval();
        // (the drop gives `SmBSplineCurve`s; for a plane a UV `SmLine` stays a line, as
        // upstream keeps the `SmLine` object behind the pointer, `SetUVCurve` stores it)
        let uv_curves: Vec<Curve> = surface
            .drop_curve::<NoObject>(
                &*env.k,
                &domain,
                &curve1,
                &interval,
                self.approx_tol_3d,
                true,
                None,
                true,
            )?
            .uv_curves;
        if uv_curves.len() != 1 {
            // Unknown problem
            return Err(SolidError::blend_failed(
                OP_3X2,
                "the extended edge does not drop onto its face as one curve",
            ));
        }
        let new_ps_curve = uv_curves.into_iter().next().ok_or_else(|| {
            SolidError::blend_failed(
                OP_3X2,
                "the extended edge does not drop onto its face as one curve",
            )
        })?;
        let prim = primary_eu(env.pseudo, fe_on_extended_edge, OP_3X2)?;
        let mate_eu = eu1::mate(env.pseudo.brep(), prim)?
            .ok_or_else(|| SolidError::blend_missing(OP_3X2, "the extended edge use's mate"))?;
        env.pseudo
            .set_edgeuse_uv_curve(mate_eu, Some(new_ps_curve), true, true)?;

        // Create a 3-sided corner patch
        self.create_corner_patch_default_with(env)
    }

    /// `SmFilletNx1Corner::CreateCornerPatch`: with three fillet edges an end patch
    /// from the extended side face (`CreateEndPatch`) or the default patch; otherwise
    /// nothing. Not upstream: a turned-back cap ([`FilletCorner::turned_back_cap`], four or
    /// five edges) takes the end patch when its end face is extended, and nothing otherwise
    /// (the default patch is three-sided).
    ///
    /// # Errors
    ///
    /// Those of the patch builders; [`SolidError::Unported`] (`CreateEndPatch`).
    pub fn create_corner_patch_nx1(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        if self.edges.len() == 3 || (self.turned_back_cap() && self.extended_surface_patch) {
            if self.extended_surface_patch {
                self.create_end_patch(env)?;
            } else {
                self.create_corner_patch_default(env)?;
            }
        }
        // else SM_DBG_WARN: Unexpected SmFilletNx1Corner endFace construction case
        Ok(())
    }

    /// [`FilletCorner::create_corner_patch_nx1`] against any [`CornerPatchEnv`];
    /// `extended_surface` resolves the executive's extended surfaces and
    /// `note_end_face_error` notes a failed end face (`CreateEndPatch`).
    ///
    /// # Errors
    /// As [`FilletCorner::create_corner_patch_nx1`].
    pub fn create_corner_patch_nx1_with(
        &mut self,
        env: &mut CornerPatchEnv<'_, '_>,
        extended_surface: impl Fn(FaceId) -> Option<Surface>,
        note_end_face_error: &mut dyn FnMut(&FilletCorner),
    ) -> Result<()> {
        if self.edges.len() == 3 || (self.turned_back_cap() && self.extended_surface_patch) {
            if self.extended_surface_patch {
                let mut end_env = CornerPatchEnv {
                    k: &mut *env.k,
                    target: env.target,
                    pseudo: &mut *env.pseudo,
                    solvers: env.solvers,
                    fillet: env.fillet.as_deref_mut(),
                    note_face_error: note_end_face_error,
                };
                self.create_end_patch_with(&mut end_env, extended_surface)?;
            } else {
                self.create_corner_patch_default_with(env)?;
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
    pub fn create_corner_patch_nx2(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        let total_edges = self.edges.len();

        // The fork's (WR-644, tangency-labs/smlib@2223108): a mitre whose rails meet the
        // side edge at two places (`AdjustTopology`, `AdjustConcaveCornerTopology`): the
        // mitre's curve, then the three edges round the gap on the side face's
        // continuation, filled by a patch on that face's extended surface. The mitre needs
        // no patch of its own.
        if self.bevel_gap_patch {
            if total_edges != 4 {
                return Err(SolidError::blend_failed(
                    OP_NX2_PATCH,
                    "the gap's patch does not have exactly four edges",
                ));
            }
            let all = self.edges.clone();
            self.edges.remove(0);
            let gap = self.create_end_patch(env);
            self.edges = all;
            return gap;
        }

        let mut first_three_edges: Vec<EdgeId> = Vec::new();
        let mut last_three_edges: Vec<EdgeId> = Vec::new();
        let mut last_two_verts: Vec<VertexId> = Vec::new();
        let mut end_patch: Option<Surface> = None;
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
        self.create_corner_patch_default(env)?;

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

    /// [`FilletCorner::create_corner_patch_nx2`] against any [`CornerPatchEnv`];
    /// `extended_surface` resolves the executive's extended surfaces and
    /// `note_end_face_error` notes a failed end face (`CreateEndPatch`).
    ///
    /// # Errors
    /// As [`FilletCorner::create_corner_patch_nx2`].
    pub fn create_corner_patch_nx2_with(
        &mut self,
        env: &mut CornerPatchEnv<'_, '_>,
        extended_surface: impl Fn(FaceId) -> Option<Surface>,
        note_end_face_error: &mut dyn FnMut(&FilletCorner),
    ) -> Result<()> {
        let total_edges = self.edges.len();

        // The fork's (WR-644): the gap's patch only, as in
        // [`FilletCorner::create_corner_patch_nx2`].
        if self.bevel_gap_patch {
            if total_edges != 4 {
                return Err(SolidError::blend_failed(
                    OP_NX2_PATCH,
                    "the gap's patch does not have exactly four edges",
                ));
            }
            let all = self.edges.clone();
            self.edges.remove(0);
            let mut end_env = CornerPatchEnv {
                k: &mut *env.k,
                target: env.target,
                pseudo: &mut *env.pseudo,
                solvers: env.solvers,
                fillet: env.fillet.as_deref_mut(),
                note_face_error: note_end_face_error,
            };
            let gap = self.create_end_patch_with(&mut end_env, extended_surface);
            self.edges = all;
            return gap;
        }

        let mut first_three_edges: Vec<EdgeId> = Vec::new();
        let mut last_three_edges: Vec<EdgeId> = Vec::new();
        let mut last_two_verts: Vec<VertexId> = Vec::new();
        let mut end_patch: Option<Surface> = None;
        if total_edges > 3 {
            first_three_edges.extend(self.edges.iter().take(3).copied());
            last_three_edges.extend(self.edges.iter().skip(3).copied());

            if total_edges == 6 {
                // Use the last 3 edges to create a small patch
                self.edges.clone_from(&last_three_edges);
                // Create a small corner face
                let mut end_env = CornerPatchEnv {
                    k: &mut *env.k,
                    target: env.target,
                    pseudo: &mut *env.pseudo,
                    solvers: env.solvers,
                    fillet: env.fillet.as_deref_mut(),
                    note_face_error: note_end_face_error,
                };
                self.create_end_patch_with(&mut end_env, extended_surface)?;

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
        self.create_corner_patch_default_with(env)?;

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

    /// `AdjustTopology` (virtual): the Nx2 override and the convex (also the concave)
    /// `NxN` one; every other kind does nothing.
    ///
    /// # Errors
    ///
    /// Those of [`FilletCorner::adjust_topology_nx2`] and
    /// [`FilletCorner::adjust_topology_convex_nxn`].
    pub fn adjust_topology(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        self.adjust_topology_with(
            env,
            Self::adjust_topology_nx2,
            Self::adjust_topology_convex_nxn,
        )
    }

    /// Dispatch topology adjustment using the caller's live derived-corner context.
    /// The inherited native base implementation needs no context and returns success.
    ///
    /// # Errors
    /// The selected concrete adjustment callback's errors.
    pub fn adjust_topology_with<C>(
        &mut self,
        context: &mut C,
        nx2: impl FnOnce(&mut Self, &mut C) -> Result<()>,
        nxn: impl FnOnce(&mut Self, &mut C) -> Result<()>,
    ) -> Result<()> {
        // the fork's SmBlendCorner::AdjustTopology: a Ball, a Setback, a Mixed corner, a Join,
        // an End or a Swept turn adjusts nothing (WR-697; 543d25a, 53c47b8); nor does the
        // chamfer event (WR-716: no patch to stretch)
        if self.blend.is_some_and(|blend| {
            blend.is_ball()
                || blend.is_setback()
                || blend.is_mixed()
                || blend.is_join()
                || blend.is_end()
                || blend.is_swept()
                || blend.is_chamfer()
        }) {
            return Ok(());
        }
        // Not the fork's (#2349): a closed Turn's one rail crosses each side edge once, so
        // there is no gap at a side edge to adjust (the closed Nx1 corner it replaces
        // adjusted nothing either)
        if self.is_closed_turn() {
            return Ok(());
        }
        match self.kind {
            FilletCornerKind::NByTwo => nx2(self, context),
            FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN => nxn(self, context),
            _ => Ok(()),
        }
    }

    /// `SmFilletNx2Corner::AdjustTopology`: where the rails of the two fillets meet the
    /// side edge at different points of a bevel corner, splits the
    /// `SM_FV_RAIL_X_EDGEUSE` vertex in two (the farther point stays on the side edge,
    /// a new `SM_FV_FILLET_X_FILLET` vertex goes on the side face) joined by new fillet
    /// edges.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for more than four edges around the corner;
    /// [`SolidError::InvalidInput`] for an unset `m_pAllEUs`, a classification object
    /// that is not an edge use, a missing rail vertex use or side edge curve (upstream
    /// dereferences them); [`SolidError::Unported`] (module docs).
    pub fn adjust_topology_nx2(&mut self, env: &mut CornerEnv<'_>) -> Result<()> {
        self.adjust_topology_nx2_with(env)
    }

    /// [`FilletCorner::adjust_topology_nx2`] against any [`Nx2AdjustContext`] (the
    /// legacy [`CornerEnv`] or a live executive session).
    ///
    /// # Errors
    ///
    /// As [`FilletCorner::adjust_topology_nx2`], plus those of the context's
    /// concave-corner operation.
    #[allow(clippy::too_many_lines)] // one upstream routine
    pub fn adjust_topology_nx2_with(&mut self, env: &mut impl Nx2AdjustContext) -> Result<()> {
        // Can't do it if more than four total edges
        let all_eus = self
            .all_eus
            .as_ref()
            .ok_or_else(|| invalid(OP_TOPO, "m_pAllEUs is NULL"))?;
        if all_eus.len() > 4 {
            return Err(SolidError::blend_failed(
                OP_TOPO,
                "the corner has more than four edge uses",
            ));
        }

        // Check for mixed-convexity case: special routine for that.
        let num_vertex_edges = self.edges.len();
        if num_vertex_edges == 3 {
            // Mixed-convexity corners
            return env.adjust_concave_corner_topology(self);
        }

        let mut ii = 0usize;
        while ii < self.vertices.len() {
            let adjust_vert = *self
                .vertices
                .get(ii)
                .ok_or_else(|| invalid(OP_TOPO, "vertex index"))?;

            let (k, target, pseudo, _) = env.parts();
            // Check whether this is the vertex that corresponds to the
            // rail hitting the side edge
            if pseudo.fillet_vertex(adjust_vert)?.fillet_vertex_type()
                != FilletVertexType::RailXEdgeuse
            {
                ii += 1;
                continue;
            }

            let geoms = pseudo.vertex_fillet_geoms(adjust_vert)?;
            let [fillet_geom1, fillet_geom2] = geoms[..] else {
                ii += 1;
                continue; // maybe already adjusted...
            };
            let Some(EntityRef::Edgeuse(side_eu1)) =
                pseudo.fillet_vertex(adjust_vert)?.point_class_object()
            else {
                return Err(invalid(OP_TOPO, "classification object is not an edge use"));
            };
            let vu1 = pseudo
                .vu_at_rail_end(adjust_vert, Some(fillet_geom1), None, None)?
                .ok_or_else(|| invalid(OP_TOPO, "no rail vertex use"))?;
            let vu2 = pseudo
                .vu_at_rail_end(adjust_vert, Some(fillet_geom2), Some(vu1), None)?
                .ok_or_else(|| invalid(OP_TOPO, "no rail vertex use"))?;
            let edge_param1 = pseudo.fillet_vertexuse(vu1)?.tsect_pnt().curve_parameter;
            let edge_param2 = pseudo.fillet_vertexuse(vu2)?.tsect_pnt().curve_parameter;
            let side_edge = eu1::edge(target, side_eu1)?
                .ok_or_else(|| invalid(OP_TOPO, "side edge use without edge"))?;
            let side_edge_curve = target
                .edge(side_edge)?
                .curve
                .as_ref()
                .ok_or_else(|| invalid(OP_TOPO, "side edge without curve"))?;

            // fork 66b02de passes these on (SER)
            let pass_on = k.fixes.fillet_statuses_pass_on;
            let pnt1 = unchecked_point(crate::error::passed_on(
                side_edge_curve.evaluate_point(k, edge_param1),
                pass_on,
            )?)?;
            let pnt2 = unchecked_point(crate::error::passed_on(
                side_edge_curve.evaluate_point(k, edge_param2),
                pass_on,
            )?)?;

            let dist = pnt1.distance(pnt2);

            // A blending corner has no bevel to adjust: upstream checked it for the
            // big-radius case here and went on. The check went with the Join that replaced
            // the blending two-blend corner (the fork's smlib#113 fe980d7, #2400): the Turn
            // here never blends but at its 3-sided face, whose three edges took the concave
            // adjustment above.
            if self.blending {
                ii += 1;
                continue;
            }

            // SM_ASSERT(m_bBevel == TRUE); Bevel cases
            let (k, target, pseudo, this_id) = env.parts();

            // If rails hit side edge at same point, no adjustment is needed.
            if dist < self.approx_tol_3d {
                ii += 1;
                continue;
            }

            // The fork's (WR-644, tangency-labs/smlib@2223108, opt-in
            // `fixes.fillet_mitre_gap_patch`): two rounds mitred at the common face's
            // concave corner whose rails meet the side edge at two places. The larger
            // round cut by the side face, and the smaller round's rail, run on past the
            // side edge onto the side face's continuation, where they meet the mitre's
            // curve: the gap is filled by a patch on the side face's extended surface, as
            // at a corner of mixed convexity whose rounds are swept, the mitre's curve
            // taking the place of the swept face (`CreateCornerPatch`).
            if k.fixes.fillet_mitre_gap_patch
                && self.all_eus.as_ref().map_or(0, Vec::len) == 3
                && p1::check_face_complimentary_corner_concavity(
                    k,
                    target,
                    self.vertex
                        .ok_or_else(|| invalid(OP_TOPO, "m_cpVertex is NULL"))?,
                    self.solver_eus
                        .as_ref()
                        .and_then(|eus| eus.first().copied())
                        .ok_or_else(|| invalid(OP_TOPO, "m_pSolverEUs[0] is NULL"))?,
                    self.solver_eus
                        .as_ref()
                        .and_then(|eus| eus.get(1).copied())
                        .ok_or_else(|| invalid(OP_TOPO, "m_pSolverEUs[1] is NULL"))?,
                    None,
                )?
            {
                env.adjust_concave_corner_topology(self)?;
                self.bevel_gap_patch = true;
                return Ok(());
            }

            self.topology_adjusted = true;

            let vert_pnt = target
                .vertex(
                    self.vertex
                        .ok_or_else(|| invalid(OP_TOPO, "m_cpVertex is NULL"))?,
                )?
                .point;
            let (side_face, vu_on_side_face, small_rad_geom, large_rad_geom, pnt_on_side_face);

            // We'll split this vertex and keep the farther point (along the edge)
            if pnt1.distance(vert_pnt) < pnt2.distance(vert_pnt) {
                set_vertex_point(pseudo, adjust_vert, pnt2)?;
                pseudo
                    .fillet_vertex_mut(adjust_vert)?
                    .set_original_t_param(edge_param2);

                side_face = eu1::face(target, side_eu1)?;
                vu_on_side_face = vu1;
                small_rad_geom = fillet_geom1;
                large_rad_geom = fillet_geom2;
                pnt_on_side_face = pnt1;
            } else {
                set_vertex_point(pseudo, adjust_vert, pnt1)?;
                pseudo
                    .fillet_vertex_mut(adjust_vert)?
                    .set_original_t_param(edge_param1);

                let radial = eu1::radial(target, side_eu1)?
                    .ok_or_else(|| invalid(OP_TOPO, "side edge use without radial partner"))?;
                side_face = eu1::face(target, radial)?;
                vu_on_side_face = vu2;
                small_rad_geom = fillet_geom2;
                large_rad_geom = fillet_geom1;
                pnt_on_side_face = pnt2;
            }

            // The vert that is farther from the filleted vertex is now done.
            pseudo
                .fillet_vertex_mut(adjust_vert)?
                .set_status(FilStatus::Processed);

            let vert_on_side_edge = adjust_vert;

            // Create new fillet vertex on the 'side' face
            let vert_on_side_face = pseudo.brep_mut().add_vertex(pnt_on_side_face)?;
            let mut fv = FilletVertex::new(k, this_id);
            fv.set_point_class(PointClassType::Face, side_face.map(EntityRef::Face));
            fv.set_fillet_vertex_type(FilletVertexType::FilletXFillet);
            pseudo.insert_fillet_vertex(vert_on_side_face, fv);

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
                pseudo,
                vu_on_side_face,
                vert_on_side_edge,
                vert_on_side_face,
            )?;

            // Recreate fillet edges
            let fillet_edge1 = if insert_after {
                pseudo.make_fillet_edge(
                    Some(vert_on_side_edge),
                    Some(vert_on_side_face),
                    this_id,
                )?
            } else {
                pseudo.make_fillet_edge(
                    Some(vert_on_side_face),
                    Some(vert_on_side_edge),
                    this_id,
                )?
            };
            pseudo
                .fillet_edge_mut(fillet_edge1)?
                .set_fillet_edge_type(FilletEdgeType::FilletXSideFace);

            let prim_eu = primary_eu(pseudo, fillet_edge1, OP_TOPO)?;
            pseudo.set_edgeuse_fillet_geom(prim_eu, Some(large_rad_geom))?;
            pseudo
                .fillet_edge_mut(fillet_edge1)?
                .set_original_face(side_face);

            let fillet_edge2 = if insert_after {
                pseudo.make_fillet_edge(Some(vert_on_side_face), Some(other_vert), this_id)?
            } else {
                pseudo.make_fillet_edge(Some(other_vert), Some(vert_on_side_face), this_id)?
            };
            pseudo
                .fillet_edge_mut(fillet_edge2)?
                .set_fillet_edge_type(FilletEdgeType::FilletXFillet);

            let prim_eu = primary_eu(pseudo, fillet_edge2, OP_TOPO)?;
            let mate_eu = eu1::mate(pseudo.brep(), prim_eu)?
                .ok_or_else(|| invalid(OP_TOPO, "new fillet edge use without mate"))?;

            pseudo.set_edgeuse_fillet_geom(prim_eu, Some(large_rad_geom))?;
            pseudo.set_edgeuse_fillet_geom(mate_eu, Some(small_rad_geom))?;

            // (lIndex >= 1: it is ii + 1, or ii >= 1 when it was decremented)
            let index = index
                .checked_sub(1)
                .ok_or_else(|| invalid(OP_TOPO, "edge index"))?;
            if index >= self.edges.len() {
                // m_vEdges[lIndex] past the array
                return Err(invalid(OP_TOPO, "edge index"));
            }
            let removed_edge = self.edges.remove(index);
            pseudo.delete_fillet_edge(removed_edge)?;

            if insert_after {
                self.edges.push(fillet_edge1);
                self.edges.push(fillet_edge2);
            } else {
                self.edges.push(fillet_edge2);
                self.edges.push(fillet_edge1);
            }
            ii += 1;
            // (the loop's own ii++)
            ii += 1;
        }

        // all done
        Ok(())
    }
}

/// An evaluation whose status upstream ignores (`EvaluatePoint` without `SER`): a
/// failure leaves the default-constructed point ([`Point3::UNDEF`]); an unported
/// evaluator still propagates.
pub(super) fn unchecked_point(r: Result<Point3>) -> Result<Point3> {
    match r {
        Ok(p) => Ok(p),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(Point3::UNDEF),
    }
}

/// `SmBSplineCurve::JoinWith` (virtual) on an edge's curve: the `SmLine` and `SmEllipse`
/// overrides for those kinds, the B-spline join on the `SmBSplineCurve` part otherwise
/// (shared with unit `blend.fillet_removal`).
pub(crate) fn join_with(
    k: &Kernel,
    this: &mut Curve,
    this_at_end: bool,
    other: &mut Curve,
    other_at_end: bool,
) -> Result<()> {
    join_with_gap(k, this, this_at_end, other, other_at_end, None)
}

/// [`join_with`] with `JoinWith`'s optional `pGapTolerance` (`None`: the curves'
/// default).
pub(crate) fn join_with_gap(
    k: &Kernel,
    this: &mut Curve,
    this_at_end: bool,
    other: &mut Curve,
    other_at_end: bool,
    gap_tolerance: Option<f64>,
) -> Result<()> {
    let owner = this.owner();
    match this.kind_mut() {
        CurveKind::Line(l) => l.join_with(
            u64::from(this_at_end),
            other,
            u64::from(other_at_end),
            gap_tolerance,
        ),
        CurveKind::Ellipse(e) => e.join_with(
            k,
            u64::from(this_at_end),
            other,
            u64::from(other_at_end),
            gap_tolerance,
            owner,
        ),
        kind => {
            let this_base = bspline_part_mut(kind).ok_or_else(|| {
                SolidError::blend_missing(OP_3X2, "the B-spline part of the curve")
            })?;
            let other_base = bspline_part_mut(other.kind_mut()).ok_or_else(|| {
                SolidError::blend_missing(OP_3X2, "the B-spline part of the other curve")
            })?;
            this_base.join_with(k, this_at_end, other_base, other_at_end, gap_tolerance)
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

/// Not upstream (#10876): the reason [`FilletCorner::create`] refuses a corner of four edges
/// where a convex and a concave blend meet, other than the Mixed corner (one continuing
/// the other across a tangent seam).
pub const MIXED_CORNER_OF_FOUR_EDGES: &str = "a convex and a concave round meeting at a \
    vertex of four edges are not supported unless one continues the other across a tangent \
    seam: they lie on opposite sides of their faces there and have no crossing to mitre on, \
    at any size (#10876)";

/// The reason of the [`SolidError::Unsupported`] that [`FilletCorner::create`] returns
/// (WR-710, #2256) for a
/// corner of two filleted edges of differing convexity whose rails cross before the
/// corner (see [`FilletCorner::create`]).
pub const MIXED_CORNER_RAILS_CROSS: &str =
    "a corner whose edges differ in convexity, where the rails cross, is not supported (#2256)";

/// Not upstream (WR-710): whether the filleted edges `edge1` and `edge2` of a 3x2
/// mixed-convexity corner leave `vertex` along one tangent, within `ang_tol_deg`
/// degrees and in the same sense. Their common face then closes to a cusp at the
/// vertex (its corner angle between them is zero), so the two rails, each set the
/// blend's size into that face, cross each other before they reach the corner: the
/// convex blend's rail runs past the concave edge and the concave blend's rail past the
/// convex edge. (Two edges of one tangent leaving the vertex in opposite senses, an
/// edge running on smoothly into the other, are not this case.) `false` for one edge
/// given twice (two filleted uses of the same edge), a closed edge, a tangent that
/// cannot be evaluated or one shorter than `SM_EFF_ZERO`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an edge without a curve.
fn mixed_corner_rails_cross(
    k: &Kernel,
    b: &Brep,
    vertex: VertexId,
    edge1: EdgeId,
    edge2: EdgeId,
    ang_tol_deg: f64,
) -> Result<bool> {
    if edge1 == edge2 {
        return Ok(false); // two uses of one edge, not two edges
    }
    // the tangent of `e` at `vertex`, pointing away from it
    let leaving = |e: EdgeId| -> Result<Option<Vec3>> {
        let start = edge::start_vertex(b, e)? == Some(vertex);
        let end = edge::end_vertex(b, e)? == Some(vertex);
        if start == end {
            return Ok(None); // closed at the vertex, or not on it
        }
        let curve =
            edge::curve(b, e)?.ok_or_else(|| invalid(OP_CREATE, "filleted edge without curve"))?;
        let interval = edge::interval(b, e)?;
        let t = if start {
            interval.min()
        } else {
            interval.max()
        };
        let pv = ignored(curve.evaluate(k, t, 1, true, true))?;
        let Some(&[_, d, ..]) = pv.as_deref() else {
            return Ok(None);
        };
        if d.length_squared() <= crate::math::EFF_ZERO * crate::math::EFF_ZERO {
            return Ok(None);
        }
        Ok(Some(if start { d } else { -d }))
    };
    let (Some(d1), Some(d2)) = (leaving(edge1)?, leaving(edge2)?) else {
        return Ok(false);
    };
    let ang = ignored(d1.angle_between(d2))?;
    Ok(ang.is_some_and(|a| rad_to_deg(a) < ang_tol_deg))
}

/// `sm_TestG1Continuity` (unit `blend.fillet_corner_p1`).
fn test_g1_continuity(
    k: &Kernel,
    b: &Brep,
    vertex: VertexId,
    edge1: EdgeId,
    edge2: EdgeId,
    ang_tol_deg: f64,
) -> Result<bool> {
    Ok(p1::test_g1_continuity(k, b, Some(vertex), Some(edge1), Some(edge2), ang_tol_deg)?.0)
}

/// `SmFilletNx1Corner::CheckNx1Degeneracy` as `Create` tests it
/// (`if (CheckNx1Degeneracy(...))`): the routine returns an `SmBoolean`, and on an
/// evaluation failure its `SER` returns the status through it, which is nonzero, so any
/// failure selects the degenerate corner, whichever Rust error variant carries the
/// native status (an `Upstream` code, or the `InvalidInput` an evaluator reports for a
/// degenerate input, as `SmOffsetCurve::Evaluate`'s zero binormal). Only an unported
/// callee stays an error (the case is deferred).
fn check_nx1_degeneracy(k: &Kernel, b: &mut Brep, vertex: VertexId, eu: EdgeuseId) -> Result<bool> {
    // the status a failed side binormal evaluation returns through the SmBoolean is
    // nonzero (true); the guards (a null dereference, a dangling id, an unported callee)
    // stay errors. The fork (tangency-labs/smlib 66b02de) keeps this.
    let status = p1::check_nx1_degeneracy_status(k, b, vertex, eu)?;
    Ok(status.unwrap_or(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filleted_edgeuses_other_than_two_are_typed_failures() {
        use crate::error::blend::BlendFailure;
        let corner = |eus: Option<Vec<EdgeuseId>>| {
            FilletCorner::new(FilletCornerKind::NByTwo, None, eus, None, 1e-3, 0.1)
        };
        let e = corner(None).filleted_edgeuses().unwrap_err();
        assert_eq!(
            e.blend_failure(),
            Some(&BlendFailure::Missing("the corner's filleted edge uses")),
            "{e:?}"
        );
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR_NULL_POINTER));
        let e = corner(Some(vec![EdgeuseId::from_index(0)]))
            .filleted_edgeuses()
            .unwrap_err();
        assert_eq!(
            e.blend_failure(),
            Some(&BlendFailure::Failed(
                "the corner does not have exactly two filleted edge uses"
            )),
            "{e:?}"
        );
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        let two = [EdgeuseId::from_index(0), EdgeuseId::from_index(1)];
        assert_eq!(
            corner(Some(two.to_vec())).filleted_edgeuses().unwrap(),
            (two[0], two[1])
        );
    }

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

    /// A vertex with a filleted edge use whose side edge uses exist (clockwise
    /// neighbour, radial's counter-clockwise neighbour) but bound no face: the Nx1
    /// degeneracy test's binormal evaluation returns a status (`SmEdgeuse::EvaluateBinormal`
    /// on a non-loop edge use), which `Create`'s `if (CheckNx1Degeneracy(...))` reads as
    /// true. The standalone helper keeps the status.
    #[test]
    fn nx1_degeneracy_status_selects_the_degenerate_corner() {
        use crate::geom::extent1d::Extent1d;
        use crate::topology::{Edgeuse, EdgeuseOwner, Vertexuse, VertexuseOwner};

        let k = Kernel::default();
        let mut b = Brep::new();
        let v = b.add_vertex(Point3::new(0.0, 0.0, 0.0)).unwrap();
        let w = b.add_vertex(Point3::new(1.0, 0.0, 0.0)).unwrap();
        let e_fil = b.add_edge(v, w).unwrap();
        let e_side = b.add_edge(v, w).unwrap();
        for e in [e_fil, e_side] {
            b.edge_mut(e).unwrap().interval = Extent1d::from_min_max(0.0, 1.0);
        }
        let add_eu = |b: &mut Brep, e: EdgeId, orientation: OrientType, at: VertexId| {
            let eu = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(e),
                    owner: EdgeuseOwner::Unknown,
                    orientation,
                    ..Edgeuse::default()
                })
                .unwrap();
            let vu = b
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(at),
                    owner: VertexuseOwner::Edgeuse(Some(eu)),
                    ..Vertexuse::default()
                })
                .unwrap();
            b.edgeuse_mut(eu).unwrap().vertexuse = Some(vu);
            b.edge_mut(e).unwrap().edgeuses.push(eu);
            b.vertex_mut(at).unwrap().vertexuses.push(vu);
            eu
        };
        let fil = add_eu(&mut b, e_fil, OrientType::Same, v);
        let fil_radial = add_eu(&mut b, e_fil, OrientType::Opposite, w);
        let side1 = add_eu(&mut b, e_side, OrientType::Same, v);
        let _side1_mate = add_eu(&mut b, e_side, OrientType::Opposite, w);
        // the clockwise neighbour of the filleted edge use and the counter-clockwise
        // neighbour of its radial coincide: the 2x1 case, the second side is its mate
        b.edgeuse_mut(fil).unwrap().cw = Some(side1);
        b.edgeuse_mut(fil_radial).unwrap().ccw = Some(side1);
        // Create's walk closes through the mate's clockwise neighbour
        b.edgeuse_mut(_side1_mate).unwrap().cw = Some(fil_radial);

        let raw = p1::check_nx1_degeneracy(&k, &mut b, v, fil);
        assert!(
            raw.as_ref().is_err_and(|e| e.upstream_status().is_some()),
            "{raw:?}"
        );
        assert!(check_nx1_degeneracy(&k, &mut b, v, fil).unwrap());

        // through Create: the status selects the degenerate corner ...
        let (exec, solvers) = one_solver(fil);
        let c = FilletCorner::create(&k, &mut b, v, &[fil], &exec, &solvers).unwrap();
        assert_eq!(c.map(|c| c.kind), Some(FilletCornerKind::Degenerate));

        // ... while a null dereference inside the test (the side edge use without
        // vertex use) is a guard, not a status, and fails the construction
        b.edgeuse_mut(side1).unwrap().vertexuse = None;
        let raw = p1::check_nx1_degeneracy(&k, &mut b, v, fil);
        assert!(
            matches!(raw, Err(SolidError::InvalidInput { .. })),
            "{raw:?}"
        );
        let r = FilletCorner::create(&k, &mut b, v, &[fil], &exec, &solvers);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
    }

    /// An executive with one base linear solver for `eu` (the tolerances `Create` reads).
    fn one_solver(eu: EdgeuseId) -> (FilletExecutiveData, Vec<FilletSolver>) {
        let mut exec = FilletExecutiveData::default();
        exec.edgeuse_solver_map.insert(eu, FilletSolverId(0));
        let mut fs = FilletSolver::new(1e-3, 1e-3, 0.01);
        fs.fsg = Some(crate::blend::fillet_solver::p1::FilletSurfaceGenerator::Linear);
        (exec, vec![fs])
    }

    /// The same vertex with side loop edge uses whose edge carries an `SmOffsetCurve`
    /// with a zero binormal (base line along `z`, offset-plane normal `z`): upstream's
    /// `SmOffsetCurve::Evaluate` returns `SM_ERR_INVALID_INPUT` from `SER(sUBin.Unitize())`
    /// before the binormal evaluation reads the face, `CheckNx1Degeneracy` forwards it and
    /// `Create` reads the nonzero value as true. The Rust evaluator reports the same
    /// failure as `InvalidInput`, which the wrapper must read as true too.
    #[test]
    fn nx1_degeneracy_evaluator_status_selects_the_degenerate_corner() {
        use crate::curve::line::Line;
        use crate::curve::offset_curve::OffsetCurve;
        use crate::geom::extent1d::Extent1d;
        use crate::topology::{Edgeuse, EdgeuseOwner, Vertexuse, VertexuseOwner};

        let k = Kernel::default();
        let mut b = Brep::new();
        let v = b.add_vertex(Point3::new(0.0, 0.0, 0.0)).unwrap();
        let w = b.add_vertex(Point3::new(0.0, 0.0, 1.0)).unwrap();
        let e_fil = b.add_edge(v, w).unwrap();
        let e_side = b.add_edge(v, w).unwrap();
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        for e in [e_fil, e_side] {
            b.edge_mut(e).unwrap().interval = ivl;
        }
        let line = Line::from_points(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0), 3);
        let offset = OffsetCurve::new(
            3,
            Curve::from(line),
            ivl,
            Vec3::new(0.0, 0.0, 1.0),
            1.0,
            false,
        )
        .unwrap();
        b.edge_mut(e_side).unwrap().curve = Some(Curve::from(offset));
        let add_eu = |b: &mut Brep,
                      e: EdgeId,
                      orientation: OrientType,
                      at: VertexId,
                      owner: EdgeuseOwner| {
            let eu = b
                .add_edgeuse_record(Edgeuse {
                    edge: Some(e),
                    owner,
                    orientation,
                    ..Edgeuse::default()
                })
                .unwrap();
            let vu = b
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(at),
                    owner: VertexuseOwner::Edgeuse(Some(eu)),
                    ..Vertexuse::default()
                })
                .unwrap();
            b.edgeuse_mut(eu).unwrap().vertexuse = Some(vu);
            b.edge_mut(e).unwrap().edgeuses.push(eu);
            b.vertex_mut(at).unwrap().vertexuses.push(vu);
            eu
        };
        let fil = add_eu(&mut b, e_fil, OrientType::Same, v, EdgeuseOwner::Unknown);
        let fil_radial = add_eu(
            &mut b,
            e_fil,
            OrientType::Opposite,
            w,
            EdgeuseOwner::Unknown,
        );
        // the side edge uses bound a loop: the evaluation reaches the curve
        let side1 = add_eu(
            &mut b,
            e_side,
            OrientType::Same,
            v,
            EdgeuseOwner::Loopuse(None),
        );
        let _side1_mate = add_eu(
            &mut b,
            e_side,
            OrientType::Opposite,
            w,
            EdgeuseOwner::Loopuse(None),
        );
        b.edgeuse_mut(fil).unwrap().cw = Some(side1);
        b.edgeuse_mut(fil_radial).unwrap().ccw = Some(side1);
        b.edgeuse_mut(_side1_mate).unwrap().cw = Some(fil_radial);

        let raw = p1::check_nx1_degeneracy(&k, &mut b, v, fil);
        assert!(
            matches!(raw, Err(SolidError::InvalidInput { .. })),
            "{raw:?}"
        );
        assert!(check_nx1_degeneracy(&k, &mut b, v, fil).unwrap());

        // through Create: the evaluator's status selects the degenerate corner
        let (exec, solvers) = one_solver(fil);
        let c = FilletCorner::create(&k, &mut b, v, &[fil], &exec, &solvers).unwrap();
        assert_eq!(c.map(|c| c.kind), Some(FilletCornerKind::Degenerate));
    }

    fn box_corner() -> Result<(Kernel, Brep, VertexId, Vec<EdgeuseId>)> {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 2.0, 3.0, 4.0)?;
        let vertex = b
            .vertices()
            .ids()
            .next()
            .ok_or_else(|| invalid(OP_CREATE, "box vertex"))?;
        let mut edges = std::collections::BTreeSet::new();
        let mut outgoing = Vec::new();
        for &vu in &b.vertex(vertex)?.vertexuses {
            if let Some(eu) = crate::topology::vertexuse::edgeuse(&b, vu)? {
                if let Some(edge) = eu1::edge(&b, eu)? {
                    if edges.insert(edge) {
                        outgoing.push(eu);
                    }
                }
            }
        }
        Ok((k, b, vertex, outgoing))
    }

    #[test]
    fn factory_borrows_live_solver_state_in_edgeuse_order() {
        use std::cell::RefCell;

        let (k, mut b, vertex, outgoing) = box_corner().unwrap();
        assert_eq!(outgoing.len(), 3);
        let mut solvers = [
            FilletSolver::new(0.03, 0.1, 0.3),
            FilletSolver::new(0.02, 0.1, 0.2),
            FilletSolver::new(0.01, 0.1, 0.1),
        ];
        for solver in &mut solvers {
            solver.fsg = Some(crate::blend::fillet_solver::p1::FilletSurfaceGenerator::Linear);
        }
        let seen = RefCell::new(Vec::new());
        let corner =
            FilletCorner::create_with_solver_lookup(&k, &mut b, vertex, &outgoing, &[], |eu| {
                seen.borrow_mut().push(eu);
                let index = outgoing
                    .iter()
                    .position(|&candidate| candidate == eu)
                    .unwrap();
                Ok(solvers.get(index).unwrap())
            })
            .unwrap()
            .unwrap();
        assert_eq!(*seen.borrow(), outgoing);
        assert_eq!(corner.kind, FilletCornerKind::ConvexNByN);
        assert_eq!(corner.approx_tol_3d, 0.01);
        assert_eq!(corner.tangency_tol_radians, 0.1);

        // The next factory call sees the same owned solver's updated state.
        solvers.get_mut(1).unwrap().approx_tol_3d = 0.001;
        let corner =
            FilletCorner::create_with_solver_lookup(&k, &mut b, vertex, &outgoing, &[], |eu| {
                let index = outgoing
                    .iter()
                    .position(|&candidate| candidate == eu)
                    .unwrap();
                Ok(solvers.get(index).unwrap())
            })
            .unwrap()
            .unwrap();
        assert_eq!(corner.approx_tol_3d, 0.001);
    }

    #[test]
    fn factory_lookup_failure_preserves_native_lazy_prefix() {
        use std::cell::RefCell;

        let (k, mut b, vertex, outgoing) = box_corner().unwrap();
        assert_eq!(outgoing.len(), 3);
        let mut solver = FilletSolver::new(0.01, 0.1, 0.1);
        solver.fsg = Some(crate::blend::fillet_solver::p1::FilletSurfaceGenerator::Linear);
        let seen = RefCell::new(Vec::new());
        let failure = invalid("factory_lookup_test", "second solver unavailable");
        let result =
            FilletCorner::create_with_solver_lookup(&k, &mut b, vertex, &outgoing, &[], |eu| {
                seen.borrow_mut().push(eu);
                if seen.borrow().len() == 2 {
                    Err(failure.clone())
                } else {
                    Ok(&solver)
                }
            });
        assert_eq!(result.unwrap_err(), failure);
        assert_eq!(*seen.borrow(), outgoing.get(..2).unwrap());

        // Invalid topology fails before the first native solver read.
        seen.borrow_mut().clear();
        b.edgeuse_mut(*outgoing.first().unwrap()).unwrap().cw = None;
        let result =
            FilletCorner::create_with_solver_lookup(&k, &mut b, vertex, &outgoing, &[], |eu| {
                seen.borrow_mut().push(eu);
                Ok(&solver)
            });
        assert!(result.is_err());
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn calc_corner_geom_early_outs() {
        let mut k = Kernel::default();
        let mut target = Brep::new();
        let mut pseudo = FilletBrep::new();
        let mut exec = FilletExecutiveData::default();
        let mut env = CornerEnv {
            k: &mut k,
            target: &mut target,
            pseudo: &mut pseudo,
            exec: &mut exec,
            corner: None,
            solvers: &mut [],
            fillet: None,
        };
        let mut c = FilletCorner::new(FilletCornerKind::FourByThree, None, None, None, 1e-3, 0.1);
        assert!(c.calc_corner_geom(&mut env).is_ok());
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
