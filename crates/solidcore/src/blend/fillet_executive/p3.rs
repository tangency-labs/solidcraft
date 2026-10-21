//! Fillet construction, insertion, trimming and merge orchestration.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`,
//! `source/SMLib/inc/SmFilletExecutive.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletExecutive::do_fillet`] | `SmFilletExecutive::DoFillet` |
//! | [`FilletExecutive::create_fillet_brep`] | `SmFilletExecutive::CreateFilletBrep` |
//! | [`FilletExecutive::do_filleting`] | `SmFilletExecutive::DoFilleting` |
//! | [`FilletExecutive`], [`ErrorInfo`] | executive/error-info member data (co-ported from p1) |
//! | [`ExecutiveAccess`] | borrowed access to dependency-owned objects |
//!
//! Deviations: debug drawing/output is omitted. Handles follow the existing
//! topology-intersector adapter model: an adapter resolves topology to typed ids
//! in `Brep` arenas; it owns solver/corner objects, never this module. Views copy
//! only small metadata and handle lists, never B-reps. Error-info data is co-ported;
//! its assignment deliberately leaves the same fields unchanged as C++'s operator=.
//! Uninitialized C++ fillet-vertex pointers are represented by `None`. Constructor
//! and parameter setup belong to p1; callers supply the state. Missing dependency
//! calls return `Unported` without swallowing it in ignored-status branches. There
//! is no input-dependent recursion. The two temporary flags and temporary attribute
//! ownership are restored explicitly after fallible bodies, including on deferral.
//! The upstream `cbi_MAP_TOPO_EDGES=0` experiments remain disabled. Parent-linked
//! stage hooks borrow this live executive, never an adapter-owned duplicate.
//! Not upstream: `do_fillet` ends with a closed-manifold check of the target
//! (`check_closed_manifold`, tangency-labs/solidcore#422). A target with an edge
//! that is not `SmEdge::IsManifold` is refused with error code
//! `SM_FILERR_BAD_MERGE`, the edge in the error info and in the error, where
//! upstream returns success. `do_filleting` alone is unchanged.

use crate::blend::fillet_geom::p1::FilletCornerType;
use crate::blend::fillet_solver::p1::FilletSurfaceGenerator;
use crate::boolean::topology_intersector::p1::TopologyIntersector;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::object::NotifyOperation;
use crate::geom::solution_array::SolutionObject;
use crate::geom::tol::zone_tol3d;
use crate::intersect::point_class::ClassObject;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::topology::proto_topology::p1::CopyBrepMap;

const OP: &str = "blend::fillet_executive::p3";

fn failure() -> SolidError {
    SolidError::Upstream {
        operation: OP,
        code: 1001,
        entity: None,
    }
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

fn ignore<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_unported() => Err(error),
        Err(_) => Ok(None),
    }
}

/// The initialized error-info members p3 reads or writes. Corner kinds use the
/// shared upstream enum; the other numeric enum values are `SmFilletErrorType`,
/// `SmFilStatus` and `SmFilletEdgeType`.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorInfo<O> {
    /// `m_eErrorCode`.
    pub code: i32,
    /// `m_eFilletStatus`.
    pub status: i32,
    /// `m_eCornerType`.
    pub corner_type: FilletCornerType,
    /// `m_eEdgeType`.
    pub edge_type: i32,
    /// `m_pFilExec` (identity supplied by the executive owner).
    pub executive: Option<O>,
    /// `m_pFilletedVertex`.
    pub vertex: Option<O>,
    /// `m_pFilletedEdge`.
    pub edge: Option<O>,
    /// `m_pSideFace` (not copied by operator=).
    pub side_face: Option<O>,
    /// `m_pEndFace`.
    pub end_face: Option<O>,
    /// `m_pFilletVertex1/2` (not copied by operator=).
    pub fillet_vertices: [Option<O>; 2],
    /// `m_pFilletSurfaces`, including null entries.
    pub surfaces: Vec<Option<O>>,
    /// `m_pFilletEdges`, including null entries.
    pub edges: Vec<Option<O>>,
    /// `m_pFilletCurves` (not copied by operator=).
    pub curves: Vec<O>,
    /// `m_vLocation` (not copied by operator=).
    pub location: Option<Point3>,
    /// `m_cComment` (not copied by operator=).
    pub comment: String,
    /// `m_cEntityName` (not copied by operator=).
    pub entity_name: String,
}

impl<O> Default for ErrorInfo<O> {
    fn default() -> Self {
        Self {
            code: 0,
            status: 0,
            corner_type: FilletCornerType::Unknown,
            edge_type: 0,
            executive: None,
            vertex: None,
            edge: None,
            side_face: None,
            end_face: None,
            fillet_vertices: [None, None],
            surfaces: Vec::new(),
            edges: Vec::new(),
            curves: Vec::new(),
            location: None,
            comment: "<no comment>\n".into(),
            entity_name: "\n".into(),
        }
    }
}

impl<O: Copy> ErrorInfo<O> {
    pub(crate) fn assign(&mut self, source: &Self) {
        self.code = source.code;
        self.status = source.status;
        self.corner_type = source.corner_type;
        self.edge_type = source.edge_type;
        self.executive = source.executive;
        self.end_face = source.end_face;
        self.edge = source.edge;
        self.vertex = source.vertex;
        self.surfaces.clone_from(&source.surfaces);
        self.edges.clone_from(&source.edges);
    }
}

/// The executive's independent control switches; p1 setup determines their values.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // five independent upstream controls
pub struct ExecutionOptions {
    /// `m_bDoPiecewiseMerge`.
    pub piecewise_merge: bool,
    /// `m_bDoTopologyInsertion`.
    pub topology_insertion: bool,
    /// `m_bDoGlobalMerge`.
    pub global_merge: bool,
    /// `m_bDoTrimming`.
    pub trimming: bool,
    /// `m_bDoClassification`.
    pub classification: bool,
}

impl Default for ExecutionOptions {
    /// The five switch defaults of the upstream executive constructor.
    fn default() -> Self {
        Self {
            piecewise_merge: false,
            topology_insertion: true,
            global_merge: false,
            trimming: true,
            classification: true,
        }
    }
}

/// Executive member state consumed by p3, co-ported from p1. This is not the
/// constructor: p1 must supply its constructor/setup defaults and intersector.
#[derive(Debug, Clone)]
pub struct FilletExecutive<O> {
    /// `m_pTargetBrep`.
    pub target: Option<O>,
    /// `m_pFilletBrep`.
    pub fillet: Option<O>,
    /// `m_vFilletCorners` in upstream order.
    pub corners: Vec<O>,
    /// `m_vFilletSolvers` in upstream order.
    pub solvers: Vec<O>,
    /// `m_vTopoEdges` (fillet edges until replaced by actual B-rep edges).
    pub topo_edges: Vec<Option<O>>,
    /// `m_pAttributeE`.
    pub edge_attribute: Option<O>,
    /// `m_bIsInitialized`.
    pub initialized: bool,
    /// The five operation switches.
    pub options: ExecutionOptions,
    /// `m_sErrorInfo`.
    pub error: ErrorInfo<O>,
    /// The inherited `m_vTI`; this is the existing intersector type.
    pub intersector: TopologyIntersector<O>,
    /// Inherited `m_sIntersectionEdges` (null casts retained).
    pub intersection_edges: Vec<Option<O>>,
    /// Inherited `m_sIntersectionVertices` (null casts retained).
    pub intersection_vertices: Vec<Option<O>>,
    /// `m_sLawSequenceEdgeList`: the edge sequences `CreateSequenceOfLaws` set up, each
    /// in order (added by `blend.fillet_executive_p2`).
    pub law_sequence_edges: Vec<Vec<O>>,
}

/// Error-only corner metadata, requested only after a corner operation fails.
#[derive(Debug, Clone)]
pub struct CornerView<O> {
    /// `GetFilletedVertex()`; failure reporting accepts null.
    pub vertex: Option<O>,
    /// `GetCornerType()`.
    pub corner_type: FilletCornerType,
    /// `GetName(sBuff, SM_TBLOCK_SIZE)`.
    pub name: String,
}

/// Metadata of a recorded fillet edge, owned by fillet-geometry p2.
#[derive(Debug, Clone, Copy)]
pub struct TopoEdgeView<O> {
    /// `GetFilletEdgeType() == SM_FE_ON_EDGE`.
    pub on_edge: bool,
    /// Whether `GetCurve()` is non-null.
    pub has_curve: bool,
    /// `m_pFilletBrepEdge1`.
    pub brep_edge: Option<O>,
}

/// Stitch outputs read by this executive. Ordinary errors must still preserve the
/// counts already written by C++; `Unported` remains a `Result` error.
#[derive(Debug, Clone)]
pub struct StitchAttempt {
    /// `eStat` converted to a Rust error, if any.
    pub status: Option<SolidError>,
    /// `lNumStitched`.
    pub stitched: u64,
    /// `lNumLamina`.
    pub lamina: u64,
    /// `dMaxVGap`.
    pub max_vertex_gap: f64,
    /// `dMaxEGap`.
    pub max_edge_gap: f64,
}

/// Dependency access. Handles must preserve object identity across B-reps and
/// resolve topology through typed `Brep` arenas. Required methods read dependency
/// records or adapt existing APIs; default methods are precisely the missing calls, with complete arguments.
/// Parent-linked corner/solver/geometry operations receive the live executive,
/// including its options, lists, error info and intersector. Adapters must not
/// retain a second executive or intersector; writes must be visible to the next phase.
/// No claimed solver/corner implementation or record is reproduced here.
pub trait ExecutiveAccess<O: ClassObject + SolutionObject + Ord> {
    /// `GetName`, `GetCornerType`, `GetFilletedVertex` for error reporting only.
    fn corner_error_info(&self, corner: O, executive: &FilletExecutive<O>)
        -> Result<CornerView<O>>;
    /// `SmFilletCorner::IsBlendingCorner`.
    fn is_blending_corner(&self, corner: O) -> Result<bool>;
    /// `SmFilletCorner::IsTangentCorner`; called only for blending corners.
    fn is_tangent_corner(&self, corner: O) -> Result<bool>;
    /// `SmFilletCorner::GetFilletSolvers`; called only for tangent blending corners.
    /// This is owned logic, not a field accessor. Upstream repeats the lookup of
    /// `m_pSolverEUs[0]` for every entry, through the current executive's solver list
    /// (`SmFilletCorner.cpp:4067-4075`); adapters must preserve that quirk.
    fn corner_solvers(&self, _corner: O, _executive: &FilletExecutive<O>) -> Result<Vec<O>> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::GetFilletSolvers
        Err(SolidError::unported(
            "SmFilletCorner::GetFilletSolvers",
            "blend.fillet_corner_p1",
        ))
    }
    /// `SmFilletCorner::GetFilletedVertex`.
    fn corner_vertex(&self, corner: O) -> Result<Option<O>>;
    /// `SmFilletCorner::GetThisApproxTol3d`.
    fn corner_approx_tolerance(&self, corner: O) -> Result<f64>;
    /// `SmFilletSolver::m_vFilletGeoms`, copied only where the caller needs them.
    fn solver_geoms(&self, solver: O) -> Result<Vec<O>>;
    /// Live indexed access to `m_vFilletGeoms`, without copying its full array.
    fn solver_geom(&self, solver: O, index: usize) -> Result<Option<O>>;
    /// `SmFilletSolver::GetEdgeuse(0)->GetEdge()`.
    fn solver_edge(&self, solver: O) -> Result<O>;
    /// `SmFilletSolver::GetThisApproxTol3d`.
    fn solver_approx_tolerance(&self, solver: O) -> Result<f64>;
    /// `SmFilletGeom::m_pFilletSurface->GetOwner()`, cast to face.
    fn geom_face(&self, geom: O) -> Result<Option<O>>;
    /// Metadata of `m_vTopoEdges` entries.
    fn topo_edge(&self, edge: O) -> Result<TopoEdgeView<O>>;
    /// `SmVertex::GetPoint`.
    fn vertex_point(&self, vertex: O) -> Result<Point3>;
    /// `SmEdge::GetCurve()->GetEnds`.
    fn edge_curve_ends(&self, k: &Kernel, edge: O) -> Result<(Point3, Point3)>;
    /// `SetCalcAsConstRad`.
    fn set_calc_as_const_rad(&mut self, corner: O, value: bool) -> Result<()>;
    /// Allocate `SmBrep()` in the executive context.
    fn new_brep(&mut self, k: &Kernel) -> Result<O>;
    /// Delete a B-rep after its intersector references have been cleared.
    fn delete_brep(&mut self, brep: O) -> Result<()>;
    /// `GetTolerance` / `SmTol::GetApproxTol3d` with `approx=true`.
    fn tolerance(&self, k: &Kernel, brep: O, approx: bool) -> Result<f64>;
    /// `m_bEditingEnabled`.
    fn editing_enabled(&self, brep: O) -> Result<bool>;
    /// Store `m_bEditingEnabled`.
    fn set_editing_enabled(&mut self, brep: O, enabled: bool) -> Result<()>;
    /// `SmBrep::GetEdges`, preserving list order.
    fn edges(&self, brep: O) -> Result<Vec<O>>;
    /// `SmEdge::IsManifold` (`topology::edge::is_manifold`): four edge uses, the
    /// first a loop edge use. Used only by the closed-manifold result check.
    fn edge_is_manifold(&self, edge: O) -> Result<bool>;
    /// The typed id of `edge` for error context, when the handle has one.
    fn edge_entity(&self, _edge: O) -> Option<EntityRef> {
        None
    }
    /// `SmBrep::GetVertices`, preserving list order.
    fn vertices(&self, brep: O) -> Result<Vec<O>>;
    /// `SmBrep::GetFaces`, preserving list order.
    fn faces(&self, brep: O) -> Result<Vec<O>>;
    /// `SmAObject::AddAttribute` (null is passed through).
    fn add_attribute(&mut self, edge: O, attribute: Option<O>) -> Result<()>;
    /// `new SmAttribute(SM_AI_FILLET_EDGES, SM_AB_STANDALONE_REFERENCE)`.
    fn new_keep_attribute(&mut self, k: &Kernel) -> Result<O>;
    /// `SmAttribute::GetUsers`, preserving order.
    fn attribute_users(&self, attribute: O) -> Result<Vec<O>>;
    /// Destroy temporary attributes and unlink their users, including on errors.
    fn delete_attribute(&mut self, attribute: O) -> Result<()>;
    /// Mark common edges and vertices using one lock per distinct context, then
    /// invoke the body while those locks are held. Both mark types and context
    /// identities must be preserved (fillet context first). Each pair lists
    /// objects in mark-write order: other/target for vertices, target/other for
    /// edges; resolve each object's context, never infer it from tuple position.
    fn with_marks<T>(
        &mut self,
        k: &mut Kernel,
        target: O,
        fillet: O,
        mates: &[(O, O)],
        body: impl FnOnce(&mut Self, &mut Kernel, &[ContextMark]) -> Result<T>,
    ) -> Result<T>;
    /// `SmFace::ShrinkGeometry()` with its header defaults; status is ignored.
    fn shrink_face(&mut self, k: &mut Kernel, face: O) -> Result<()>;
    /// `SmFace::Notify`, adapting to `topology::face::p3::notify` with the
    /// caller's live notification records, attributes and surface trim cache.
    /// All notification arguments are retained, including null.
    fn notify_face(
        &mut self,
        k: &mut Kernel,
        face: O,
        operation: NotifyOperation,
        data1: Option<O>,
        data2: Option<O>,
        data3: Option<O>,
    ) -> Result<()>;
    /// `SmMerge::BooleanPostProcess`, using its actual intersector and recorded
    /// intersection lists (the implementation is already ported in merge p1).
    fn boolean_post_process(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<O>,
        check_all_edges: bool,
    ) -> Result<()>;

    /// `SmFilletExecutive::CreateFilletCorners`; waiting for `blend.fillet_executive_p1`.
    fn create_corners(
        &mut self,
        _k: &mut Kernel,
        _executive: &mut FilletExecutive<O>,
        _error: Option<&mut ErrorInfo<O>>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::CreateFilletCorners
        Err(SolidError::unported(
            "SmFilletExecutive::CreateFilletCorners",
            "blend.fillet_executive_p1",
        ))
    }

    /// `SmFilletExecutive::ProcessSmoothEdgeSequences` (`blend.fillet_executive_p2`).
    fn process_smooth_edge_sequences(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        executive.process_smooth_edge_sequences(k, self)
    }

    /// Pure virtual `SmFilletCorner::MakeCornerTopology` dispatch. Concrete
    /// overrides are not assigned to units.json; adapters must use the corner
    /// owner's implementation rather than inventing an UNPORTED owner here.
    fn make_corner_topology(
        &mut self,
        k: &mut Kernel,
        corner: O,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// `SmFilletCorner::CalcCornerVertGeom`; waiting for `blend.fillet_corner_p1`.
    fn calc_corner_vertices(
        &mut self,
        _k: &mut Kernel,
        _corner: O,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::CalcCornerVertGeom
        Err(SolidError::unported(
            "SmFilletCorner::CalcCornerVertGeom",
            "blend.fillet_corner_p1",
        ))
    }

    /// `SmFilletCorner::CheckAndFixDegeneracy`; waiting for `blend.fillet_corner_p1`.
    fn check_degeneracy(
        &mut self,
        _k: &mut Kernel,
        _corner: O,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<bool> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::CheckAndFixDegeneracy
        Err(SolidError::unported(
            "SmFilletCorner::CheckAndFixDegeneracy",
            "blend.fillet_corner_p1",
        ))
    }

    /// Virtual `AdjustTopology` dispatch. The base, 4x3 and `NxN` implementations
    /// belong to `blend.fillet_corner_p1`, and `ConvexNxN` to `blend.fillet_corner_p2`.
    /// Nx2 (and `ConvexNxN`) go to the already ported
    /// `blend::fillet_corner::p3::FilletCorner::adjust_topology` (fillet corner p3),
    /// which runs `SmFilletNx2Corner::AdjustTopology` and returns
    /// `blend.fillet_corner_p2`'s `Unported` for `ConvexNxN`. Adapters must dispatch
    /// by concrete corner type and return that owner's `Unported` while it is
    /// missing, never substitute the base success default.
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::AdjustTopology / SmFillet4x3Corner::AdjustTopology / SmFilletNxNCorner::AdjustTopology
    // UNPORTED(blend.fillet_corner_p2): SmFilletConvexNxNCorner::AdjustTopology
    fn adjust_topology(
        &mut self,
        k: &mut Kernel,
        corner: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// Virtual `AdjustTangentCorner` dispatch. The base belongs to
    /// `blend.fillet_corner_p1`, and `Nx1Closed` to `blend.fillet_corner_p2`. Nx2 (and
    /// `Nx1Closed`) go to the already ported
    /// `blend::fillet_corner::p3::FilletCorner::adjust_tangent_corner` (fillet corner
    /// p3), which runs `SmFilletNx2Corner::AdjustTangentCorner` and returns
    /// `blend.fillet_corner_p2`'s `Unported` for `Nx1Closed`. Adapters must dispatch by
    /// concrete corner type and return that owner's `Unported` while it is missing,
    /// never substitute the base success default.
    // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::AdjustTangentCorner
    // UNPORTED(blend.fillet_corner_p2): SmFilletNx1ClosedCorner::AdjustTangentCorner
    fn adjust_tangent_corner(
        &mut self,
        k: &mut Kernel,
        corner: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// `SmFilletCorner::CalcCornerEdgeGeom`; waiting for `blend.fillet_corner_p1`.
    fn calc_corner_edges(
        &mut self,
        _k: &mut Kernel,
        _corner: O,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::CalcCornerEdgeGeom
        Err(SolidError::unported(
            "SmFilletCorner::CalcCornerEdgeGeom",
            "blend.fillet_corner_p1",
        ))
    }

    /// `SmFilletCorner::ReInitializeCornerEdges`; waiting for `blend.fillet_corner_p1`.
    fn reinitialize_corner_edges(
        &mut self,
        _k: &mut Kernel,
        _corner: O,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::ReInitializeCornerEdges
        Err(SolidError::unported(
            "SmFilletCorner::ReInitializeCornerEdges",
            "blend.fillet_corner_p1",
        ))
    }

    /// `SmFilletCorner::CalcCornerGeom()` (`SmFilletExecutive.cpp:2448`): delegate to
    /// the already ported `blend::fillet_corner::p3::FilletCorner::calc_corner_geom`
    /// (fillet corner p3) on the corner `corner` resolves to, with its `CornerEnv`
    /// (the target and fillet B-reps and the executive's solvers and edge-use map).
    /// Its status is returned unchanged; the caller handles it as upstream does.
    fn calc_corner_geom(
        &mut self,
        k: &mut Kernel,
        corner: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// Virtual `CalcFilletGeom` dispatch. The base belongs to
    /// `blend.fillet_solver_p4`; the `SurfaceSurfaceFS` override belongs to
    /// `blend.fillet_standard_solver_p2`. Adapters must dispatch by concrete
    /// solver type and return that owner's `Unported` while it is missing.
    // UNPORTED(blend.fillet_solver_p4): SmFilletSolver::CalcFilletGeom
    // UNPORTED(blend.fillet_standard_solver_p2): SmSurfaceSurfaceFS::CalcFilletGeom -- justified: plan/findings.md blend.fillet_standard_solver_p2 (executive adapter stubs)
    fn calc_fillet_geom(
        &mut self,
        k: &mut Kernel,
        solver: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// `SmFilletSolver::TrimRails()` (`SmFilletExecutive.cpp:2350`): delegate to the
    /// already ported `blend::fillet_solver::p1::FilletSolver::trim_rails` (fillet
    /// solver p1) on the solver `solver` resolves to. Its status is returned
    /// unchanged; the caller handles it as upstream does.
    fn trim_rails(
        &mut self,
        k: &mut Kernel,
        solver: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// `SmFilletSolver::TestRollOver`; waiting for `blend.fillet_solver_p5`.
    fn test_rollover(
        &mut self,
        _k: &mut Kernel,
        _solver: O,
        _adjusted_corners: &mut Vec<O>,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_solver_p5): SmFilletSolver::TestRollOver
        Err(SolidError::unported(
            "SmFilletSolver::TestRollOver",
            "blend.fillet_solver_p5",
        ))
    }

    /// `SmBrep::SetTolerance(sZoneTol3d, bUpdateOnlyIfLarger, bCascadeToBndries)`:
    /// delegate to the already ported `topology::brep::p2::set_tolerance` (brep p2) on
    /// the B-rep `brep` resolves to, passing every argument. Upstream's `SetTolerance`
    /// is `void`, so the callers ignore any non-`Unported` error
    /// (`SmFilletExecutive.cpp:2439`, `:2509`).
    fn set_brep_tolerance(
        &mut self,
        k: &mut Kernel,
        brep: O,
        tolerance: f64,
        update_only_if_larger: bool,
        cascade_to_boundaries: bool,
    ) -> Result<()>;

    /// `SmFilletGeom::MakeFaceBrep(m_pFilletBrep)`: delegate to the already ported
    /// `blend::fillet_geom::p4::FilletBrep::make_face_brep` (fillet geom p4) on the
    /// fillet geometry `geom` resolves to, with the executive's solver table, the
    /// fillet B-rep `brep` resolves to (the face is built there) and that B-rep's notify
    /// environment. Its status is returned unchanged; the caller ignores a failure as
    /// upstream does (`SmFilletExecutive.cpp:2519`).
    fn make_face_brep(
        &mut self,
        k: &mut Kernel,
        geom: O,
        brep: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// `SmBrep::StitchFaces`; waiting for `topology.brep_p4`.
    fn stitch_faces(
        &mut self,
        _k: &mut Kernel,
        _brep: O,
        _tolerance: f64,
        _faces: Option<&[O]>,
        _allow_spine_edges: bool,
    ) -> Result<StitchAttempt> {
        // UNPORTED(topology.brep_p4): SmBrep::StitchFaces
        Err(SolidError::unported(
            "SmBrep::StitchFaces",
            "topology.brep_p4",
        ))
    }

    /// `SmFilletExecutive::DoPiecewiseMerge`; waiting for `blend.fillet_executive_p1`.
    fn piecewise_merge(
        &mut self,
        _k: &mut Kernel,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::DoPiecewiseMerge
        Err(SolidError::unported(
            "SmFilletExecutive::DoPiecewiseMerge",
            "blend.fillet_executive_p1",
        ))
    }

    /// `SmFilletExecutive::SetFilletBrepEdgeMap` (`blend.fillet_executive_p2`).
    fn set_fillet_brep_edge_map(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        executive.set_fillet_brep_edge_map(k, self)
    }

    /// `SmFilletGeom::InsertIntersectionTopology()`: delegate to the already ported
    /// `blend::fillet_geom::p4::FilletBrep::insert_intersection_topology` (fillet geom
    /// p4) on the fillet geometry `geom` resolves to, with the executive's solver table,
    /// the target B-rep (`executive.target`) and the fillet B-rep
    /// (`executive.fillet`). Its status propagates (`SER`, `SmFilletExecutive.cpp:3113`).
    fn insert_geom_topology(
        &mut self,
        k: &mut Kernel,
        geom: O,
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// `SmFilletCorner::InsertIntersectionTopology`; waiting for `blend.fillet_corner_p1`.
    fn insert_corner_topology(
        &mut self,
        _k: &mut Kernel,
        _corner: O,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::InsertIntersectionTopology
        Err(SolidError::unported(
            "SmFilletCorner::InsertIntersectionTopology",
            "blend.fillet_corner_p1",
        ))
    }

    /// Delegate to the live intersector's already ported
    /// `TopologyIntersector::intersect_insert_relate` (boolean intersector p2),
    /// using this adapter's `IntersectorBrep` view and stable intersector handle.
    /// Preserve its mutations on error; p3 does not consume its deleted list.
    fn intersect_insert_relate(
        &mut self,
        k: &mut Kernel,
        intersector: &mut TopologyIntersector<O>,
    ) -> Result<()>;

    /// `SmFilletGeom::TrimOriginalSurfaces(rKept, rDeleted, rMarkLocks)`: delegate to
    /// the already ported `blend::fillet_geom::p6::FilletBrep::trim_original_surfaces`
    /// (fillet geom p6) on the fillet geometry `geom` resolves to, with the fillet
    /// B-rep (`executive.fillet`), the target B-rep (`executive.target`), the
    /// executive's topology-intersector mates and the solver table; the faces it
    /// collects resolve back to handles in `kept_faces` / `deleted_faces`.
    fn trim_original_surfaces(
        &mut self,
        k: &mut Kernel,
        geom: O,
        kept_faces: &mut Vec<O>,
        deleted_faces: &mut Vec<O>,
        marks: &[ContextMark],
        executive: &mut FilletExecutive<O>,
    ) -> Result<()>;

    /// Delegate to the live intersector's already ported
    /// `TopologyIntersector::delete_faces` (boolean intersector p2), using this
    /// adapter's `IntersectorBrep` view and preserving every argument.
    fn delete_faces(
        &mut self,
        k: &mut Kernel,
        intersector: &mut TopologyIntersector<O>,
        faces: &[O],
        keep_as_wires: Option<&[O]>,
    ) -> Result<()>;

    /// `target->MergeBrep(other, pOptCopyBrepMap, pOptReverseMap, bAddAnalytics)`: the
    /// owner of the B-reps implements it with `topology::brep::p3::merge_brep` (a
    /// stateless owner through `topology::brep::p3::with_kernel_callbacks`, `other`
    /// enrolled in the same identity domain), as `IntersectorBrep::brep_delete_face`.
    ///
    /// # Errors
    ///
    /// Those of the merge; an owner with no B-rep behind a handle returns its own error.
    fn merge_brep(
        &mut self,
        k: &mut Kernel,
        target: O,
        other: O,
        copy_map: Option<&mut CopyBrepMap<O>>,
        reverse_map: Option<&mut CopyBrepMap<O>>,
        add_analytics: bool,
    ) -> Result<()>;

    /// `SmStitch::DoStitching` (`SmFilletExecutive.cpp:3559-3576`). Delegate to the
    /// already ported `heal::stitch::p2::Stitch::do_stitching` (heal stitch p2) on the
    /// B-rep `brep` resolves to:
    /// - build the `Stitch` with `DefaultStitchCallback` (upstream's base
    ///   `SmStitchCallback`) and `tolerance` as its stitch tolerance, and copy every
    ///   [`StitchSettings`] field onto the `Stitch` field of the same name;
    /// - pass the candidate lists through (`DoFilleting` passes `None` for both);
    /// - pass a `MinGapRequest` with `vertex` and `edge` set by whether
    ///   `min_vertex_gap` and `min_edge_gap` are `Some` (both `false` from
    ///   `DoFilleting`, whose call omits the min-gap out-params), writing the
    ///   reported gaps back when requested;
    /// - map `Stitching.{stitched_edges, lamina_edges, max_vertex_gap, max_edge_gap}`
    ///   to [`StitchAttempt`]`.{stitched, lamina, max_vertex_gap, max_edge_gap}` with
    ///   `status: None`; an error from `do_stitching` becomes `status: Some(error)`
    ///   with zero counts and gaps (the outputs `DoStitching` initialises), so the
    ///   retry loop sees it as upstream's `eStat`. Return `Err` only when the handle
    ///   cannot be resolved.
    ///
    /// `Stitch` keeps no state between calls besides its options and tolerance, so
    /// one `Stitch` per call matches upstream's single `sStitch` whose tolerance
    /// `SetStitchTol3d` doubles between tries.
    ///
    /// Once this is wired to a live `Brep`, UB-197 (`DoStitching` keeps using an edge
    /// that a failed glue deleted) applies: `DoFilleting` sets
    /// `m_bIgnoreProblems = TRUE`, so a failed glue does not stop the stitch.
    fn do_stitching(
        &mut self,
        k: &mut Kernel,
        brep: O,
        tolerance: f64,
        candidate_vertices: Option<&[O]>,
        candidate_edges: Option<&[O]>,
        settings: StitchSettings,
        min_vertex_gap: Option<&mut f64>,
        min_edge_gap: Option<&mut f64>,
    ) -> Result<StitchAttempt>;

    /// `SmFilletExecutive::RemoveTopoEdges`; waiting for `blend.fillet_executive_p1`.
    fn remove_topo_edges(
        &mut self,
        _k: &mut Kernel,
        _executive: &mut FilletExecutive<O>,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_executive_p1): SmFilletExecutive::RemoveTopoEdges
        Err(SolidError::unported(
            "SmFilletExecutive::RemoveTopoEdges",
            "blend.fillet_executive_p1",
        ))
    }

    /// `brep->OrientTrimmedSurfaces(bNormalsOutward, rbMaybeNotSolid, bJustUnifyNormals,
    /// pOptFacesToSwapUV)`: the owner of the B-reps implements it with
    /// `topology::brep::p3::orient_trimmed_surfaces`, writing its
    /// `maybe_not_closed_solid` to `maybe_not_solid`.
    ///
    /// # Errors
    ///
    /// Those of the orientation; an owner with no B-rep behind a handle returns its own
    /// error.
    fn orient_trimmed_surfaces(
        &mut self,
        k: &mut Kernel,
        brep: O,
        outward: bool,
        maybe_not_solid: &mut bool,
        just_unify: bool,
        faces_to_swap_uv: Option<&mut Vec<O>>,
    ) -> Result<()>;

    // ---- added by blend.fillet_executive_p2 (PR #267) --------------------------------

    /// `SmFilletGeom::GetFilletSurface() != NULL`.
    fn geom_has_surface(&self, geom: O) -> Result<bool>;
    /// The rails of a fillet geometry in `SetFilletBrepEdgeMap`'s order:
    /// `m_vRails[0]`, `m_vRails[1]`, then `m_vOtherRails1` and `m_vOtherRails2`
    /// (`None` for a null entry).
    fn geom_rails(&self, geom: O) -> Result<Vec<Option<O>>>;
    /// `SmFilletGeom::m_vSideEUs`, in order.
    fn geom_side_edgeuses(&self, geom: O) -> Result<Vec<O>>;
    /// `SmEdgeuse::GetEdge()` / the edge use's owning edge (`None` for null).
    fn edgeuse_edge(&self, edgeuse: O) -> Result<Option<O>>;
    /// `SmFilletEdge::GetOriginalFace()`.
    fn fillet_edge_original_face(&self, fillet_edge: O) -> Result<Option<O>>;
    /// `SmFilletEdge::GetOriginalEdge()`.
    fn fillet_edge_original_edge(&self, fillet_edge: O) -> Result<Option<O>>;
    /// `SmFilletEdge::FindFilletBrepEdge(crEdges, bFindMultipleEdges)` (the
    /// `blend.fillet_geom_p1` port, `FilletBrep::find_fillet_brep_edge`).
    fn find_fillet_brep_edge(
        &mut self,
        k: &mut Kernel,
        fillet_edge: O,
        edges: &[O],
        find_multiple_edges: bool,
    ) -> Result<()>;
    /// `SmFilletCorner::m_vEdges` (`None` for a null entry).
    fn corner_edges(&self, corner: O) -> Result<Vec<Option<O>>>;
    /// `SmEdge::GetBlendEdgeuse` (`crate::topology::edge::blend_edgeuse`).
    fn blend_edgeuse(&mut self, k: &mut Kernel, edge: O) -> Result<Option<O>>;
    /// `SmEdge::GetTolerance`.
    fn edge_tolerance(&self, edge: O) -> Result<f64>;
    /// `SmEdge::GetStartVertex()` and `GetOtherVertex(start)`.
    fn edge_vertices(&self, edge: O) -> Result<(Option<O>, Option<O>)>;
    /// `SM_CAST_PTR(SmVariableRadiusFS, solver)`: the solver's variable-radius data,
    /// `None` for any other solver class.
    fn variable_radius_solver(
        &self,
        solver: O,
    ) -> Result<Option<&crate::blend::fillet_standard_solver::p1::VariableRadiusFs>>;
    /// The same cast, for writing the law (`SmTemporaryRadiusChange`).
    fn variable_radius_solver_mut(
        &mut self,
        solver: O,
    ) -> Result<Option<&mut crate::blend::fillet_standard_solver::p1::VariableRadiusFs>>;

    /// `SmFilletSolver::GetEdgeuse(lIndex)`: delegate to the solver's
    /// `FilletSolver::edgeuse` (`blend.fillet_solver_p1`).
    fn solver_edgeuse(&self, solver: O, index: usize) -> Result<Option<O>>;

    /// `SmFilletSolver::GetFilletCorner(lIndex)`; waiting for `blend.fillet_solver_p2`.
    /// A vertex without a corner (`SmFilletCorner::Create` returned `NULL`) yields
    /// `None`; upstream returns its raw `m_vVertexCornerMap` edge-use array cast to a
    /// corner (UB-209).
    fn solver_corner(&self, _solver: O, _index: usize) -> Result<Option<O>> {
        // UNPORTED(blend.fillet_solver_p2): SmFilletSolver::GetFilletCorner
        Err(SolidError::unported(
            "SmFilletSolver::GetFilletCorner",
            "blend.fillet_solver_p2",
        ))
    }

    /// `SmFilletSolver::GetOtherFilletCorner(pCorner)`; waiting for
    /// `blend.fillet_solver_p3`.
    fn solver_other_corner(&self, _solver: O, _corner: O) -> Result<Option<O>> {
        // UNPORTED(blend.fillet_solver_p3): SmFilletSolver::GetOtherFilletCorner
        Err(SolidError::unported(
            "SmFilletSolver::GetOtherFilletCorner",
            "blend.fillet_solver_p3",
        ))
    }

    /// `SmFilletCorner::GetTangentFilletSolver(pFS)` (virtual); waiting for
    /// `blend.fillet_corner_p1` (the base, `SmFillet1x1Corner`, `SmFillet2x2Corner`) and
    /// `blend.fillet_corner_p2` (`SmFilletNx1ClosedCorner`, `SmFilletNx2Corner`).
    fn corner_tangent_solver(&self, _corner: O, _solver: O) -> Result<Option<O>> {
        // UNPORTED(blend.fillet_corner_p1): SmFilletCorner::GetTangentFilletSolver
        Err(SolidError::unported(
            "SmFilletCorner::GetTangentFilletSolver",
            "blend.fillet_corner_p1",
        ))
    }

    /// `SmFilletCorner::GetSetBackDist()`: the inline read of `m_dSetBackDist`
    /// (`FilletCorner::set_back_dist`).
    fn corner_setback_distance(&self, corner: O) -> Result<f64>;

    /// `new SmLinearCrossSectionFSG` / `SmCircularCrossSectionFSG(bApproximateArcs,
    /// dTolPercentRadius, bMirror, bComplement)` / `SmBlendCurveCrossSectionFSG(
    /// dBlendScale, lContinuity)` in the executive context: store `generator` (the
    /// `blend.fillet_solver_p1` record) and return its handle.
    fn new_surface_generator(
        &mut self,
        k: &mut Kernel,
        generator: FilletSurfaceGenerator,
    ) -> Result<O>;

    /// `new SmConstantRadiusFS(ctx, dApproxTol, dAngTolRad, dTanTolRad, dRadius, pEU)`:
    /// delegate to `fillet_standard_solver::p1::constant_radius_from_edgeuse` on the
    /// B-rep of `edgeuse` (no second radius) and return the new solver's handle.
    fn new_constant_radius_solver(
        &mut self,
        k: &mut Kernel,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
        radius: f64,
        edgeuse: O,
    ) -> Result<O>;

    /// The fillet solver constructors `SetFilletParameters` and `SurfaceSurfaceFillet`
    /// call, dispatched on the spec: `SmConstantRadiusFS` through
    /// [`ExecutiveAccess::new_constant_radius_solver`]; `SmConstantDistanceFS`,
    /// `SmVariableRadiusFS` and `SmSurfaceSurfaceFS` are ported
    /// (`blend.fillet_standard_solver_p2`) but need a solver arena in the adapter
    /// that no adapter has yet (`plan/findings.md`, executive adapter stubs).
    fn new_fillet_solver(
        &mut self,
        k: &mut Kernel,
        spec: &super::p2::SolverSpec<'_, O>,
    ) -> Result<O> {
        match *spec {
            super::p2::SolverSpec::ConstantRadius {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            } => self.new_constant_radius_solver(
                k,
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            ),
            // UNPORTED(blend.fillet_standard_solver_p2): SmConstantDistanceFS::SmConstantDistanceFS -- justified: plan/findings.md blend.fillet_standard_solver_p2 (executive adapter stubs)
            super::p2::SolverSpec::ConstantDistance { .. } => Err(SolidError::unported(
                "SmConstantDistanceFS::SmConstantDistanceFS",
                "blend.fillet_standard_solver_p2",
            )),
            // UNPORTED(blend.fillet_standard_solver_p2): SmVariableRadiusFS::SmVariableRadiusFS -- justified: plan/findings.md blend.fillet_standard_solver_p2 (executive adapter stubs)
            super::p2::SolverSpec::VariableRadius { .. } => Err(SolidError::unported(
                "SmVariableRadiusFS::SmVariableRadiusFS",
                "blend.fillet_standard_solver_p2",
            )),
            // UNPORTED(blend.fillet_standard_solver_p2): SmSurfaceSurfaceFS::SmSurfaceSurfaceFS -- justified: plan/findings.md blend.fillet_standard_solver_p2 (executive adapter stubs)
            super::p2::SolverSpec::SurfaceSurface { .. } => Err(SolidError::unported(
                "SmSurfaceSurfaceFS::SmSurfaceSurfaceFS",
                "blend.fillet_standard_solver_p2",
            )),
        }
    }

    /// `SmFilletSolver::SetFilletSurfaceGenerator(pFSG)`: delegate to
    /// `FilletSolver::set_fillet_surface_generator` (`blend.fillet_solver_p1`) with
    /// the generator's record.
    fn set_surface_generator(&mut self, solver: O, generator: O) -> Result<()>;
}

/// A mark lock's context identity and mark type; the adapter keeps its real
/// `MarkLock` alive around the callback passed to `with_marks`.
#[derive(Debug, Clone, Copy)]
pub struct ContextMark {
    /// Adapter context identity (not an address).
    pub context: usize,
    /// Locked mark type, to select the corresponding current mark.
    pub mark_type: crate::geom::context::MarkType,
}

/// The `SmStitch` switches passed to [`ExecutiveAccess::do_stitching`]. `Default` is
/// the configuration `DoFilleting` builds.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // upstream independent switches
pub struct StitchSettings {
    /// `m_bDoRegionNesting`.
    pub do_region_nesting: bool,
    /// `m_bValidateResult`.
    pub validate_result: bool,
    /// `m_bIgnoreProblems`.
    pub ignore_problems: bool,
    /// `m_bFastEdgeCompare`.
    pub fast_edge_compare: bool,
    /// `m_bSqueezeSmallEdges`.
    pub squeeze_small_edges: bool,
    /// `m_bSplitEdgesWithVertices`.
    pub split_edges_with_vertices: bool,
    /// `m_bMakingManifoldSolid`.
    pub making_manifold_solid: bool,
    /// `m_bRemoveLaminarSlivers`.
    pub remove_laminar_slivers: bool,
}

impl Default for StitchSettings {
    fn default() -> Self {
        Self {
            do_region_nesting: true,
            validate_result: true,
            ignore_problems: true,
            fast_edge_compare: false,
            squeeze_small_edges: false,
            split_edges_with_vertices: true,
            making_manifold_solid: true,
            remove_laminar_slivers: false,
        }
    }
}

impl<O: ClassObject + SolutionObject + Ord> FilletExecutive<O> {
    /// Execute edge filleting after parameter setup, with the upstream initialized
    /// check and default null error-info argument to `CreateFilletCorners`.
    pub fn do_fillet(
        &mut self,
        k: &mut Kernel,
        access: &mut impl ExecutiveAccess<O>,
    ) -> Result<()> {
        if !self.initialized {
            // The generic NoteError overload changes exactly these two members.
            self.error.code = 1; // SM_FILERR_EXEC_UNINITIALIZED
            self.error.comment = "ERROR: fillet parameters not set up.".into();
            return Err(invalid("fillet parameters not set up"));
        }
        access.create_corners(k, self, None)?;
        self.do_filleting(k, access, None)?;
        self.check_closed_manifold(access)
    }

    /// Refuse a filleted target that is not a closed manifold solid (not upstream;
    /// tangency-labs/solidcore#422). Upstream ignores the `bMaybeNotSolid` that
    /// `OrientTrimmedSurfaces` reports and returns success, so a round whose
    /// trimming breaks the shell came back as a valid body. The first edge of the
    /// target, in `GetEdges` order, that is not `SmEdge::IsManifold` is recorded in
    /// the error info and named in the error. The target keeps the merged
    /// topology, as on upstream's other merge failures.
    fn check_closed_manifold(&mut self, access: &impl ExecutiveAccess<O>) -> Result<()> {
        let target = self.target.ok_or(invalid("missing target B-rep"))?;
        for edge in access.edges(target)? {
            if access.edge_is_manifold(edge)? {
                continue;
            }
            self.error.code = 7; // SM_FILERR_BAD_MERGE
            self.error.status = 2; // SM_FIL_FAILURE
            self.error.edge = Some(edge);
            self.error.comment =
                "Fillet Merge Failure: Result Is Not A Closed Manifold Solid".into();
            return Err(SolidError::Upstream {
                operation: OP,
                code: 1001,
                entity: access.edge_entity(edge),
            });
        }
        Ok(())
    }

    fn corner_failure(
        &mut self,
        access: &impl ExecutiveAccess<O>,
        corner: O,
        comment: &str,
    ) -> Result<()> {
        let view = access.corner_error_info(corner, self)?;
        self.error.code = 3; // SM_FILERR_VERTEX_PROBLEM
        self.error.status = 2; // SM_FIL_FAILURE
        self.error.corner_type = view.corner_type;
        self.error.vertex = view.vertex;
        if !self.error.surfaces.contains(&None) {
            self.error.surfaces.push(None);
        }
        self.error.fillet_vertices = [None, None];
        self.error.comment = comment.into();
        self.error.entity_name = view.name;
        Err(failure())
    }

    fn solver_failure(
        &mut self,
        access: &impl ExecutiveAccess<O>,
        solver: O,
        status: i32,
        comment: &str,
    ) -> Result<()> {
        self.error.edge = Some(access.solver_edge(solver)?);
        self.error.code = 4; // SM_FILERR_EDGE_PROBLEM
        self.error.status = status;
        self.error.comment = comment.into();
        Err(failure())
    }

    /// Construct and stitch the fillet shell, preserving every stage after each
    /// missing dependency call. `error_out` is assigned only on success.
    pub fn create_fillet_brep(
        &mut self,
        k: &mut Kernel,
        access: &mut impl ExecutiveAccess<O>,
        error_out: Option<&mut ErrorInfo<O>>,
    ) -> Result<()> {
        // NER: upstream returns SM_ERR_NULL_POINTER, not invalid input.
        let target = self.target.ok_or(SolidError::Upstream {
            operation: OP,
            code: 1006,
            entity: None,
        })?;
        ignore(access.process_smooth_edge_sequences(k, self))?;

        let mut index = 0;
        while let Some(corner) = self.corners.get(index).copied() {
            if ignore(access.make_corner_topology(k, corner, self))?.is_none() {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Unable to Make Topology",
                );
            }
            let mut as_constant = access.is_blending_corner(corner)?;
            if as_constant && access.is_tangent_corner(corner)? {
                let solvers = access.corner_solvers(corner, self)?;
                if solvers.len() == 2 {
                    let solver = solvers
                        .first()
                        .copied()
                        .ok_or(invalid("missing first solver"))?;
                    let edge = access.solver_edge(solver)?;
                    let (start, end) = access.edge_curve_ends(k, edge)?;
                    let vertex = access
                        .corner_vertex(corner)?
                        .ok_or(invalid("null tangent corner vertex"))?;
                    let point = access.vertex_point(vertex)?;
                    let distance1 = start.distance(point);
                    let distance2 = end.distance(point);
                    let tolerance = 2.0 * access.corner_approx_tolerance(corner)?;
                    if distance1 > tolerance && distance2 > tolerance {
                        as_constant = false;
                    }
                }
            }
            access.set_calc_as_const_rad(corner, as_constant)?;
            if ignore(access.calc_corner_vertices(k, corner, self))?.is_none() {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Unable to Compute Geometry",
                );
            }
            index += 1;
        }
        let mut index = 0;
        while let Some(corner) = self.corners.get(index).copied() {
            if !access.check_degeneracy(k, corner, self)?
                && ignore(access.adjust_topology(k, corner, self))?.is_none()
            {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Topology Adjustment Failure",
                );
            }
            index += 1;
        }
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            if ignore(access.calc_fillet_geom(k, solver, self))?.is_none() {
                return self.solver_failure(
                    access,
                    solver,
                    10,
                    "Fillet Surface Computation Failure",
                );
            }
            index += 1;
        }
        let mut index = 0;
        while let Some(corner) = self.corners.get(index).copied() {
            if ignore(access.adjust_tangent_corner(k, corner, self))?.is_none() {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Topology Adjustment Failure",
                );
            }
            if ignore(access.calc_corner_vertices(k, corner, self))?.is_none() {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Vertex Computation Failure",
                );
            }
            if ignore(access.calc_corner_edges(k, corner, self))?.is_none() {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Corner Edge Computation Failure",
                );
            }
            index += 1;
        }
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            if ignore(access.trim_rails(k, solver, self))?.is_none() {
                return self.solver_failure(
                    access,
                    solver,
                    7,
                    "Fillet Surface Computation Failure: Unable to Trim Rail Curve",
                );
            }
            index += 1;
        }
        if self.options.classification {
            let mut adjusted = Vec::new();
            let mut index = 0;
            while let Some(solver) = self.solvers.get(index).copied() {
                if ignore(access.test_rollover(k, solver, &mut adjusted, self))?.is_none() {
                    return self.solver_failure(
                        access,
                        solver,
                        7,
                        "Fillet Solver Error: Unable to Process Rollover",
                    );
                }
                index += 1;
            }
            for corner in adjusted {
                ignore(access.reinitialize_corner_edges(k, corner, self))?;
                if ignore(access.calc_corner_edges(k, corner, self))?.is_none() {
                    return self.corner_failure(
                        access,
                        corner,
                        "Fillet Corner Error: Edge Computation Failure After Rollover Adjustment",
                    );
                }
            }
        }
        let fillet = access.new_brep(k)?;
        self.fillet = Some(fillet);
        let tolerance = access.tolerance(k, target, false)?;
        ignore(access.set_brep_tolerance(k, fillet, tolerance, true, true))?;
        access.set_editing_enabled(fillet, true)?;
        let mut index = 0;
        while let Some(corner) = self.corners.get(index).copied() {
            if ignore(access.calc_corner_geom(k, corner, self))?.is_none() {
                return self.corner_failure(
                    access,
                    corner,
                    "Fillet Corner Error: Corner Geometry Computation Failure",
                );
            }
            index += 1;
        }
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            let tolerance = access.solver_approx_tolerance(solver)?;
            if access.tolerance(k, fillet, false)? < tolerance
                && access.tolerance(k, fillet, false)? < tolerance
            {
                // SM_USE_OLDTOL: repeated comparison and full SetTolerance defaults.
                // SmBrep.h:700 implicitly constructs SmZoneTol3d at this call.
                let tolerance = zone_tol3d(k, tolerance);
                ignore(access.set_brep_tolerance(k, fillet, tolerance, true, true))?;
            }
            let mut geom_index = 0;
            while let Some(geom) = access.solver_geom(solver, geom_index)? {
                if ignore(access.make_face_brep(k, geom, fillet, self))?.is_none() {
                    return self.solver_failure(
                        access,
                        solver,
                        2,
                        "Fillet Surface Computation Failure: Fillet Face Creation Error",
                    );
                }
                geom_index += 1;
            }
            index += 1;
        }
        for edge in access.edges(fillet)? {
            ignore(access.add_attribute(edge, self.edge_attribute))?;
        }
        let tolerance = access.tolerance(k, fillet, false)?;
        let attempt = access.stitch_faces(k, fillet, tolerance, None, false)?;
        if let Some(error) = attempt.status {
            if error.is_unported() {
                return Err(error);
            }
            self.error.code = 7; // SM_FILERR_BAD_MERGE
            self.error.comment =
                "Fillet Stitching Failure: Error Stitching Fillets Together".into();
            return Err(failure());
        }
        if let Some(output) = error_out {
            output.assign(&self.error);
        }
        Ok(())
    }

    /// Build the fillet B-rep and merge it using the selected strategy. The context
    /// Boolean flag and target editing flag are restored on every return.
    pub fn do_filleting(
        &mut self,
        k: &mut Kernel,
        access: &mut impl ExecutiveAccess<O>,
        mut error_out: Option<&mut ErrorInfo<O>>,
    ) -> Result<()> {
        let doing_boolean = k.context.doing_boolean();
        k.context.set_doing_boolean(true);
        self.intersector.doing_fillets = true;
        let result = (|| {
            if ignore(self.create_fillet_brep(k, access, error_out.as_deref_mut()))?.is_none() {
                return Err(failure());
            }
            let fillet = self
                .fillet
                .ok_or(invalid("missing constructed fillet B-rep"))?;
            if access.faces(fillet)?.is_empty() {
                return Err(failure());
            }
            let mut index = 0;
            while let Some(solver) = self.solvers.get(index).copied() {
                if let Some(geom) = access.solver_geom(solver, 0)? {
                    if let Some(face) = access.geom_face(geom)? {
                        ignore(access.shrink_face(k, face))?;
                    }
                }
                index += 1;
            }
            let target = self.target.ok_or(invalid("missing target B-rep"))?;
            let editing = access.editing_enabled(target)?;
            access.set_editing_enabled(target, true)?;
            let result = self.merge_fillet(k, access, target, fillet, error_out);
            let restore = access.set_editing_enabled(target, editing);
            result.and(restore)
        })();
        k.context.set_doing_boolean(doing_boolean);
        result
    }

    fn merge_fillet(
        &mut self,
        k: &mut Kernel,
        access: &mut impl ExecutiveAccess<O>,
        target: O,
        fillet: O,
        error_out: Option<&mut ErrorInfo<O>>,
    ) -> Result<()> {
        self.intersector.set_primary_brep(Some(target));
        self.intersector.set_other_brep(Some(fillet));
        if self.options.piecewise_merge {
            access.piecewise_merge(k, self)?;
            if let Some(output) = error_out {
                output.assign(&self.error);
            }
            return Ok(());
        }
        access.set_fillet_brep_edge_map(k, self)?;
        if self.options.topology_insertion {
            let mut index = 0;
            while let Some(solver) = self.solvers.get(index).copied() {
                // GetFilletGeoms copies the array before its inner loop.
                for geom in access.solver_geoms(solver)? {
                    access.insert_geom_topology(k, geom, self)?;
                }
                index += 1;
            }
            let mut index = 0;
            while let Some(corner) = self.corners.get(index).copied() {
                access.insert_corner_topology(k, corner, self)?;
                index += 1;
            }
        }
        // Upstream cbi_MAP_TOPO_EDGES=0: do not collect mapped intersections.
        if self.options.global_merge {
            self.intersector
                .set_this_approx_tol_3d(access.tolerance(k, target, true)?);
            access.intersect_insert_relate(k, &mut self.intersector)?;
        }
        if self.options.trimming {
            let mut mates = Vec::new();
            for vertex in access.vertices(target)? {
                if let Some(other) = self.intersector.other_mate(Some(vertex)) {
                    mates.push((other, vertex)); // fillet mark first, target second
                }
            }
            for edge in access.edges(target)? {
                if let Some(other) = self.intersector.other_mate(Some(edge)) {
                    mates.push((edge, other)); // original first for edges (C++:3311)
                }
            }
            access.with_marks(k, target, fillet, &mates, |access, k, marks| {
                self.trim_and_merge(k, access, target, fillet, marks)
            })?;
        }
        // The final spine-edge scan only emits SM_DBG_WARN and cannot change state
        // or status in this release build, so there is no geometry dependency here.
        if let Some(output) = error_out {
            output.assign(&self.error);
        }
        Ok(())
    }

    fn trim_and_merge(
        &mut self,
        k: &mut Kernel,
        access: &mut impl ExecutiveAccess<O>,
        target: O,
        fillet: O,
        marks: &[ContextMark],
    ) -> Result<()> {
        let mut deleted = Vec::new();
        let mut kept = Vec::new();
        let mut index = 0;
        while let Some(solver) = self.solvers.get(index).copied() {
            for geom in access.solver_geoms(solver)? {
                access.trim_original_surfaces(k, geom, &mut kept, &mut deleted, marks, self)?;
            }
            index += 1;
        }
        for face in access.faces(fillet)? {
            if !kept.contains(&face) && !deleted.contains(&face) {
                deleted.push(face);
            }
        }
        access.delete_faces(k, &mut self.intersector, &deleted, None)?;
        let mut attributes = Vec::new();
        let result = (|| {
            let edges = access.edges(fillet)?; // re-walk after DeleteFaces
            for edge in self.topo_edges.iter().flatten().copied() {
                let view = access.topo_edge(edge)?;
                if !view.on_edge || !view.has_curve {
                    continue;
                }
                if let Some(edge) = view.brep_edge.filter(|edge| edges.contains(edge)) {
                    let attribute = access.new_keep_attribute(k)?;
                    attributes.push(attribute);
                    ignore(access.add_attribute(edge, Some(attribute)))?;
                }
            }
            // Upstream cbi_MAP_TOPO_EDGES=0: no attributes on extra mated edges.
            access.merge_brep(k, target, fillet, None, None, true)?;
            if self.intersector.other_brep() == Some(fillet) {
                self.intersector.set_other_brep(None);
            }
            if self.intersector.primary_brep() == Some(fillet) {
                self.intersector.set_primary_brep(None);
            }
            access.delete_brep(fillet)?;
            self.fillet = None;
            let mut tolerance = access.tolerance(k, target, false)?;
            for attempt in 0..5 {
                let stitch = access.do_stitching(
                    k,
                    target,
                    tolerance,
                    None,
                    None,
                    StitchSettings::default(),
                    None,
                    None,
                )?;
                if stitch.status.is_none() && stitch.lamina == 0 {
                    break;
                }
                if let Some(error) = &stitch.status {
                    if error.is_unported() {
                        return Err(error.clone());
                    }
                    if attempt == 4 {
                        self.error.code = 7; // SM_FILERR_BAD_MERGE
                        self.error.comment =
                            "Fillet Stitching Failure: Error Stitching Fillets To Original Brep"
                                .into();
                        return Err(failure());
                    }
                }
                tolerance *= 2.0;
            }
            self.intersector.set_primary_brep(Some(target));
            self.topo_edges.clear();
            for attribute in &attributes {
                if let Some(user) = access.attribute_users(*attribute)?.first().copied() {
                    self.topo_edges.push(Some(user));
                }
            }
            // Upstream cbi_MAP_TOPO_EDGES=0: do not collect mapped intersections.
            access.remove_topo_edges(k, self)?;
            access.boolean_post_process(k, self, true)?;
            for face in access.faces(target)? {
                // SmTypes.h:659: SM_NO_GET_BREP(face) expands to a null object.
                ignore(access.notify_face(
                    k,
                    face,
                    NotifyOperation::PreEdit,
                    Some(face),
                    None,
                    None,
                ))?;
            }
            let mut maybe_not_solid = false;
            ignore(access.orient_trimmed_surfaces(
                k,
                target,
                true,
                &mut maybe_not_solid,
                false,
                None,
            ))?;
            Ok(())
        })();
        // SmObjsDelete lives through orientation; also clean up on errors/deferrals.
        let mut cleanup = Ok(());
        for attribute in attributes {
            let deleted = access.delete_attribute(attribute);
            if cleanup.is_ok() {
                cleanup = deleted;
            }
        }
        result.and(cleanup)
    }
}

#[cfg(test)]
#[path = "p3_tests.rs"]
mod tests;
