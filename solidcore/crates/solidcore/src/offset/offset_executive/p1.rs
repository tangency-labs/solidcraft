//! Offset executive settings, correspondence, synchronization and topology construction prefixes.
//!
//! Upstream: `source/SMLib/inc/SmOffsetExecutive.h` (201–222),
//! `source/SMLib/src/SmOffsetExecutive.cpp` (35–77, 1941–3450, 3517–4183).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`OffsetOperation`], [`OffsetExecutive::new`] | `SmOffsetOperationType`, `SmOffsetExecutive` |
//! | [`OffsetExecutive`] accessors | `GetInset`, `GetTangencyTolDegrees`, `GetExtendConvexEdges`, `GetOffsetBrep`, `GetShellFaces`, setters |
//! | `notify_edge_and_vertices` (private) | `sm_NotifyEdgeAndVertices` |
//! | [`OffsetExecutive::replace_implicit_offsets`] | `ReplaceImplicitOffsets` |
//! | [`OffsetExecutive::remove_excess_faces`] | `RemoveExcessFaces` |
//! | `forget_seconds`, `forget_firsts`, `removed_faces_edgeuses` (private) | `sm_ForgetSeconds`, `sm_ForgetFirsts`, `RemoveExcessFaces`' `sGone` |
//! | [`OffsetExecutive::get_create_extended_surface`] | `GetCreateExtendedSurface` |
//! | [`OffsetExecutive::insert_extended_surface`] | `m_vSurfExtSurf.Insert` (protected upstream; harness seeding) |
//! | [`OffsetExecutive::piecewise_merge`] | `PiecewiseMerge` |
//! | `sort_edgeuses`, `insert_breaks`, `remove_small_stuff`, `extract_gaps` (private) | `sm_SortEdgeuses`, `sm_InsertBreaks`, `sm_RemoveSmallStuff`, `sm_ExtractGaps` |
//! | [`OffsetExecutive::offset_edgeuse`] | `GetOffsetEdgeuse` |
//! | [`OffsetExecutive::offset_faces`] | `m_vFUToF.GetSeconds` (protected upstream; harness observation) |
//! | [`OffsetExecutive::synchronize_offset_edgeuses`] | `SyncronizeOffsetEdgeuses` |
//! | [`OffsetExecutive::build_side_wall_of_edgeuse`] | `BuildSideWallOfEdgeuse` |
//! | [`OffsetExecutive::build_offset_of_edgeuses`] | `BuildOffsetOfEdgeuses` |
//! | [`OffsetExecutive::fill_gap_edgeuses`] | `FillGapEdgeuses` |
//! | [`OffsetExecutive::build_offset_of_vertex`] | `BuildOffsetOfVertex` |
//! | [`OffsetExecutive::build_offset_of_faceuse`] | `BuildOffsetOfFaceuse` |
//! | `build_contained_offset_faces` (private) | `BuildOffsetOfFaceuse` contained-domain branch |
//! | `build_boolean_offset_faces` (private) | `BuildOffsetOfFaceuse` 2D-boolean branch |
//! | [`OffsetExecutive::map_edgeuses`] | `MapEdgeuses` |
//!
//! Deviations: RAII replaces the destructor. Constructor scalars are an immutable snapshot
//! of `SmOffsetGeometryCreation` parameters, not a retained creator reference or its implementation. `Relation` uses deterministic id order
//! instead of upstream hash/pointer order; bubble sorting remains stable, including unordered
//! floating-point comparisons. Extension identity is a caller-supplied key, never an address.
//! Every unavailable dependency is reported at its actual call, including ignored statuses.
//! Continuations after unconditional missing calls are documented there, not reported as success.
//! `RemoveExcessFaces` borrows the original B-rep immutably for queries
//! and keys its cache by each edge's and face's geometry owner (arena identity when
//! unowned), so geometry owned under a caller's notification keys is found; an owner
//! shared by two records or naming another record (an indirect owner chain) is
//! [`SolidError::Unsupported`] rather than silently skipped. Its relation forgetting
//! (`sm_ForgetSeconds` / `sm_ForgetFirsts` and the `sGone` / `sLive` walks) propagates
//! a dangling-id error from the topology queries where upstream's walks cannot fail;
//! only a corrupt B-rep reaches it.
//! Result-edge enumeration uses `SmBrep::GetEdges` list order (`topology::brep::p1::edges`);
//! the result B-rep is borrowed mutably because `NormalizedEvaluate` takes the edge use's
//! B-rep mutably (its 3D evaluation, used here, edits nothing). `BuildSideWallOfEdgeuse` takes
//! the original B-rep as an argument (upstream reaches it through the edge use pointer).
//! Edge-offset preparation and gap handling are shared with p2. Preparation can split
//! result edges and notify through caller-owned stores, using either an external
//! `NotifyEnv` or the actual kernel context. Geometry creators are ported separately;
//! vertex and edge caps use the shared creator through typed cross-arena adapters
//! (`EdgeuseOffset` edits both B-reps through the caller's stores and selected
//! context, and reads edge-use marks from the stores' records, or from the calling
//! loop's set inside p2). Cap faces are built with the caller's result identities.
//! The executive's creator snapshot keeps its angular tolerance.
//! `BuildOffsetOfFaceuse` moves the offset surface into `MakeFaceWithCurves` (upstream
//! hands over the pointer); a failed constructor drops it and, as upstream, relates no
//! face. Its `SmSSIData` UV curves are cloned into the merge input; a missing entry,
//! which upstream dereferences, is [`OffsetFailure::SelfIntersectionWithoutUvCurve`].
//! `GetInfiniteRegion` is the
//! `Brep` accessor. In the 2D-boolean branch the offset surface goes to the first
//! resulting face and each later face owns an exact copy (upstream they all share the
//! one pointer); the temporary planar and domain B-reps notify through arena-only
//! stores (`ArenaNotifyState`, faces named by `loopuse::notify_ref`), as merge p3's
//! planar boolean does, and `GetSurfaces` selects the surfaces owned under those
//! names. `FillGapEdgeuses` builds its corner-blend faces through `make_cap_face`'s
//! `CreateFaceFromSurface`, notifying through the caller's result-keyed stores; a
//! null closest edge (dereferenced upstream) is [`OffsetFailure::GapEndWithoutEdge`], a
//! null same-oriented edge use or end mate a defect, a null blended face is `RelatePair`'s
//! invalid input, and any failed `CreateCornerBlend` is
//! [`OffsetFailure::GapCornerBlendNotMade`] with `SM_ERR` (`SER(SM_ERR)`) and the blend's
//! error as its cause, which decides its class (a defect is passed on), unported callees
//! aside.
//!
//! Failures (the conversion guide, `docs/failure-codes-and-reproduction.md` §6): each keeps
//! the status upstream's site returned. A link or entity of a B-rep, or of the executive's
//! own data, that is not there is an [`OffsetFailure::Defect`] (`SM_ERR_NULL_POINTER`)
//! whose `why` says which; a missing argument, a face of the input with no surface and an
//! edge of the input with no curve are the caller's input; a kind the routine does not
//! handle is named ([`OffsetFailure::OffsetBaseKindUnsupported`],
//! [`OffsetFailure::CurveKindUnsupported`]), and so are the pieces of an edge's two sides
//! that do not pair ([`OffsetFailure::EdgeOffsetsUnpaired`]). Where the executive reads a
//! step's failure as a step that did not apply (the gap fills' pipe fallback, and p2's
//! calls whose status upstream ignores), a defect is passed on ([`ignored_unless_defect`]).
//! No new input-driven recursion; existing topology walks are cycle-bounded.
//! The lamina clean-up's `DeleteFaces` runs through merge p4's arena intersector over
//! the offset B-rep alone (both upstream operands are that B-rep); it notifies under
//! `DELETE_FACES_NAMESPACE` rather than the caller's offset keys.
//! `ReplaceImplicitOffsets` replaces each implicit offset by its certified net
//! (`certified_offset`, WR-718), not by NLib's `N_SrfOffset` fit checked at samples.
//!
//! Cancellation: `replace_implicit_offsets` polls the kernel's cancel token (#2136, #2372) per face and inside each surface offset.

use std::collections::{BTreeMap, BTreeSet};

use crate::boolean::merge::p4::KernelManifoldIntersector;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::offset::OffsetFailure;
use crate::error::{
    ignore_status, EntityRef, Failure, Result, SolidError, SM_ERR, SM_ERR_NULL_POINTER,
};
use crate::geom::attribute::{ids, Attribute, Behavior};
use crate::geom::context::{Context, MarkType};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::solution_array::{Solution, SolutionArray};
use crate::geom::tol::Relation;
use crate::heal::stitch::p1::{BorrowedStitchEdits, StitchEditContext};
use crate::heal::stitch::p2 as stitch;
use crate::intersect::topology_solver::p1::TopologySolver;
use crate::intersect::topology_solver::p2::TopologySolveOptions;
use crate::kernel::Kernel;
use crate::math::{Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::offset::offset_geometry_creation::{OffsetGeometryCreation, SsiData};

use super::edgeuse::{CapNotifications, MarkSource};
use super::p2::OffsetNotifications;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1::{self as bp1, SetRegionVoidType};
use crate::topology::brep::p4::shrink_geometry;
use crate::topology::brep::p7::{self as bp7, FaceCurvesInput, SelfIntersectionCurve};
use crate::topology::brep::p9::CurveIdentity;
use crate::topology::brep_cache::{
    arena_identity, make_brep_cache, BrepCache, BrepObjects, CacheObject, CacheSource,
};
use crate::topology::edge::{self, NotificationStores, NotifyEnv, NotifyState};
use crate::topology::edgeuse::{p1 as eu, p2 as eu2, p3 as eu3};
use crate::topology::face::{p2 as face, p3 as face3};
use crate::topology::topology;
use crate::topology::topology_traverser::face_edges;
use crate::topology::{loopuse, vertex, Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, VertexId};

const OP: &str = "offset::offset_executive::p1";

enum MapKernel<'a> {
    Read(&'a Kernel),
    Mutable(&'a mut Kernel),
}

impl MapKernel<'_> {
    fn kernel(&self) -> &Kernel {
        match self {
            Self::Read(kernel) => kernel,
            Self::Mutable(kernel) => kernel,
        }
    }

    fn with_env<T>(
        &mut self,
        state: &mut NotifyState<'_>,
        run: impl FnOnce(&mut NotifyEnv<'_>) -> Result<T>,
    ) -> Result<T> {
        match self {
            Self::Read(_) => state.with_context(None, run),
            Self::Mutable(kernel) => state.with_env(kernel, run),
        }
    }
}

/// Where an offset's results are placed (`SmOffsetOperationType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum OffsetOperation {
    /// Straight volumetric offset into a new B-rep.
    SolidOffset = 0,
    /// Non-manifold offset.
    NonManifoldOffset = 1,
    /// Offset into the original B-rep.
    Shell = 2,
    /// Local operation.
    LocalOperation = 3,
    /// Machining offset.
    MachineOffset = 4,
}

/// Synchronized offset uses, with null padding only in the upstream early-return branches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SynchronizedEdgeuses {
    /// Uses mapped from the original use.
    pub edgeuses: Vec<Option<EdgeuseId>>,
    /// Uses mapped from its radial partner.
    pub other_edgeuses: Vec<Option<EdgeuseId>>,
    /// First-side uses having no matching second-side interval.
    pub gaps: Vec<EdgeuseId>,
    /// Second-side uses having no matching surviving first-side interval.
    pub other_gaps: Vec<EdgeuseId>,
}

/// Geometry returned by `BuildOffsetOfVertex` (out-parameters upstream).
#[derive(Debug, Clone, Default)]
pub struct VertexOffset {
    /// New cap surface, absent when no cap is necessary.
    pub surface: Option<Surface>,
    /// Its 3D trimming curves.
    pub trim_curves: Vec<Curve>,
    /// Orientations of the trimming curves.
    pub curve_orientations: Vec<OrientType>,
}

/// The 27-bit object namespace under which the lamina clean-up's `DeleteFaces`
/// (merge p4's arena adapter) records its notifications; distinct from the Boolean
/// kernels' namespaces, which share no attribute store with an offset executive.
const DELETE_FACES_NAMESPACE: u32 = 0x00f5_e700;

/// Topological offset state. Original ids and result ids belong to separate B-reps.
/// Geometry-creator parameters are an immutable scalar snapshot; this executive does
/// not retain a creator reference or implement its offset-geometry algorithms.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // Four independent upstream settings, not exclusive states.
pub struct OffsetExecutive {
    operation: OffsetOperation,
    offset_distance: f64,
    approximation_tol_3d: f64,
    tangency_tol_radians: f64,
    inset: bool,
    extend_convex_edges: bool,
    merge_results: bool,
    create_offset_solid: bool,
    shell_faces: Vec<FaceId>,
    offset_brep: Option<Brep>,
    closing_faces: Vec<FaceId>,
    pub(super) faceuse_to_faces: Relation<FaceuseId, FaceId>,
    pub(super) edgeuse_to_edgeuses: Relation<EdgeuseId, EdgeuseId>,
    pub(super) edge_to_faces: Relation<EdgeId, FaceId>,
    pub(super) vertex_to_faces: Relation<VertexId, FaceId>,
    pub(super) blend_to_faces: Relation<FaceId, FaceId>,
    extended_surfaces: BTreeMap<u64, Surface>,
    /// Not upstream: how far each original face's surface must reach past the face for
    /// its neighbours' offsets to meet its own ([`super::reach::extension_reach`]), by the
    /// face and by each of its face uses, recorded as the face is offset.
    extension_reaches: BTreeMap<FaceId, f64>,
    faceuse_extension_reaches: BTreeMap<FaceuseId, f64>,
    /// Not upstream: the edge bounds the local operation's reaches have found, each edge's
    /// taken again from here for its other face ([`super::reach::EdgeBounds`]); forgotten
    /// when the local surfaces are set and when the relations are cleared.
    edge_bounds: super::reach::EdgeBounds,
    /// Not upstream: the faces of a local operation that move to surfaces of their own
    /// (the others keep theirs), for their neighbours' reach
    /// ([`super::reach::local_extension_reach`]).
    moved_faces: Vec<FaceId>,
    /// Not upstream: the original edges of a local operation that keep their geometry
    /// (their faces, and every face at their vertices, keep their surfaces), and their
    /// edge uses ([`Self::keep_unmoved_edges`]).
    pub(super) kept_edges: BTreeSet<EdgeId>,
    pub(super) kept_edgeuses: BTreeSet<EdgeuseId>,
    /// Not upstream: the uses of the original edges of a local operation where the two new
    /// surfaces cross ([`super::reach::LocalReach::crossing`]), tangent in the input or not.
    crossing_edgeuses: BTreeSet<EdgeuseId>,
    /// Not upstream (WR-346): the original faces of an inset whose offset collapses (a
    /// convex round thinner than the wall), mated across instead of offset
    /// (`m_sCollapsedFaces`).
    pub(super) collapsed_faces: Vec<FaceId>,
    /// Not upstream (WR-346): for each of [`Self::collapsed_faces`], the signed distance its
    /// surface was offset by along its own NURB normal (the offset distance, negated where
    /// the offset face use is opposite its face), for [`OffsetFailure::Collapses`]'s radius.
    pub(super) collapsed_displacements: Vec<(FaceId, f64)>,
    /// Not upstream (solidcraft#16983): the offset faces with an edge whose offset is
    /// inverted (its neighbours' offsets crossed over it; `TrimmedEdge`), which
    /// `resolve_crossing_loops` resolves when they have one loop.
    pub(super) inverted_faces: BTreeSet<FaceId>,
    /// Genuine endpoints beyond a periodic chart seam, resolved before gap closure.
    pub(super) periodic_boundary_ends: Vec<super::periodic_boundary::BoundaryEnd>,
    /// Not in the fork (solidcore#3329): each wall carried on past a region of a face its
    /// loops wind round the other way within a wall, and the strip that carries it on
    /// (`resolve_crossing_loops`); the merge does not intersect the two.
    pub(super) wall_extensions: Vec<(FaceId, FaceId)>,
    /// Not upstream (WR-344 refinement, tangency-labs/solidcore#1795): the original edge
    /// uses whose edge `EdgeuseOffset` could not mate (`MateEdges` failed); the merge
    /// intersects their faces' offsets as upstream does, not taking them as mated.
    pub(super) unmated_edgeuses: BTreeSet<EdgeuseId>,
    /// Source uses whose paired edge curves were successfully installed.
    pub(super) mated_edgeuses: BTreeSet<EdgeuseId>,
    /// The faces of a local operation and the surfaces replacing theirs, in step (the
    /// part of `SmLocalOperationGeometryCreation` the executive reads: `m_vFaces`,
    /// `m_vSurfaces`).
    pub(super) local_faces: Vec<FaceId>,
    pub(super) local_surfaces: Vec<Surface>,
}

impl OffsetExecutive {
    /// Constructs a geometry-parameter snapshot with the creator's default angular
    /// tolerance, `2.0 * PI / 180.0` radians. Other executive settings use upstream defaults.
    pub fn new(
        operation: OffsetOperation,
        offset_distance: f64,
        approximation_tol_3d: f64,
    ) -> Self {
        Self::with_geometry_parameters(
            operation,
            offset_distance,
            approximation_tol_3d,
            2.0 * PI / 180.0,
        )
    }

    /// Constructs an immutable snapshot of the geometry creator's scalar parameters,
    /// including the angular tolerance that an upstream creator subclass may override.
    /// No creator reference is retained, and geometry creation remains a separate unit.
    pub fn with_geometry_parameters(
        operation: OffsetOperation,
        offset_distance: f64,
        approximation_tol_3d: f64,
        tangency_tol_radians: f64,
    ) -> Self {
        Self {
            operation,
            offset_distance,
            approximation_tol_3d,
            tangency_tol_radians,
            inset: false,
            extend_convex_edges: false,
            merge_results: false,
            create_offset_solid: false,
            shell_faces: Vec::new(),
            offset_brep: None,
            closing_faces: Vec::new(),
            faceuse_to_faces: Relation::default(),
            edgeuse_to_edgeuses: Relation::default(),
            edge_to_faces: Relation::default(),
            vertex_to_faces: Relation::default(),
            blend_to_faces: Relation::default(),
            extended_surfaces: BTreeMap::new(),
            extension_reaches: BTreeMap::new(),
            faceuse_extension_reaches: BTreeMap::new(),
            edge_bounds: super::reach::EdgeBounds::default(),
            moved_faces: Vec::new(),
            kept_edges: BTreeSet::new(),
            kept_edgeuses: BTreeSet::new(),
            crossing_edgeuses: BTreeSet::new(),
            collapsed_faces: Vec::new(),
            collapsed_displacements: Vec::new(),
            inverted_faces: BTreeSet::new(),
            periodic_boundary_ends: Vec::new(),
            wall_extensions: Vec::new(),
            unmated_edgeuses: BTreeSet::new(),
            mated_edgeuses: BTreeSet::new(),
            local_faces: Vec::new(),
            local_surfaces: Vec::new(),
        }
    }

    /// The requested operation (`GetOffsetOperation`).
    pub const fn operation(&self) -> OffsetOperation {
        self.operation
    }

    /// Whether the operation is an inset (`GetInset`).
    pub const fn inset(&self) -> bool {
        self.inset
    }

    /// Whether convex edges are extended rather than filleted (`GetExtendConvexEdges`).
    pub const fn extend_convex_edges(&self) -> bool {
        self.extend_convex_edges
    }

    /// Whether results are merged rather than stitched.
    pub const fn merge_results(&self) -> bool {
        self.merge_results
    }

    /// Whether lamina edges receive closing side walls.
    pub const fn create_offset_solid(&self) -> bool {
        self.create_offset_solid
    }

    /// The snapshotted geometry-creator angular tolerance, converted in upstream operation order.
    pub fn tangency_tol_degrees(&self) -> f64 {
        self.tangency_tol_radians * 180.0 / PI
    }

    /// Takes the geometry creator's constructor tolerance
    /// ([`OffsetGeometryCreation::default_tangency_tol_radians`]): half a degree (WR-666,
    /// the fork's tangency-labs/smlib@2e1bb8c), not [`Self::new`]'s two degrees. Called
    /// right after construction by the executives that
    /// `SmApiOffsetBrepFull`, `SmApiShellBrepFull` and `SmBrep::LocalOperation` build.
    ///
    /// On the default path since #2981: WR-666's split is the only path, so the creator
    /// tolerance is always the fork's.
    pub fn use_creator_tangency_tol(&mut self, _kernel: &Kernel) {
        self.tangency_tol_radians = OffsetGeometryCreation::default_tangency_tol_radians();
    }

    /// Offset distance supplied by the geometry creator.
    pub const fn offset_distance(&self) -> f64 {
        self.offset_distance
    }

    /// Approximation tolerance supplied by the geometry creator.
    pub const fn approximation_tol_3d(&self) -> f64 {
        self.approximation_tol_3d
    }

    /// Faces copied rather than offset, preserving order and duplicates (`GetShellFaces`).
    pub fn shell_faces(&self) -> &[FaceId] {
        &self.shell_faces
    }

    /// The executive's temporary B-rep (`GetOffsetBrep`).
    pub const fn offset_brep(&self) -> Option<&Brep> {
        self.offset_brep.as_ref()
    }

    /// Faces created to close lamina boundaries.
    pub fn closing_faces(&self) -> &[FaceId] {
        &self.closing_faces
    }

    /// Notes a face made to close a gap in the offset exactly, which the face extensions
    /// leave as it is.
    pub(super) fn push_closing_face(&mut self, face: FaceId) {
        self.closing_faces.push(face);
    }

    /// Sets `m_bInset`.
    pub fn set_inset(&mut self, value: bool) {
        self.inset = value;
    }

    /// Sets `m_bExtendConvexEdges`.
    pub fn set_extend_convex_edges(&mut self, value: bool) {
        self.extend_convex_edges = value;
    }

    /// Sets `m_bMergeResults`.
    pub fn set_merge_results(&mut self, value: bool) {
        self.merge_results = value;
    }

    /// Sets `m_bCreateOffsetSolid`.
    pub fn set_create_offset_solid(&mut self, value: bool) {
        self.create_offset_solid = value;
    }

    /// Replaces the shell-face list with a copy, without deduplicating (`SetShellFaces`).
    pub fn set_shell_faces(&mut self, faces: &[FaceId]) {
        self.shell_faces.clear();
        self.shell_faces.extend_from_slice(faces);
    }

    /// The geometry of a local operation (`SmLocalOperationGeometryCreation`'s face and
    /// surface lists): `surfaces[i]` replaces the surface of `faces[i]`. The `moved` faces
    /// take surfaces of their own, the others copies of theirs (not upstream: each face's
    /// surface is extended as far as its neighbours' new surfaces need,
    /// [`super::reach::local_extension_reach`]).
    pub fn set_local_surfaces(
        &mut self,
        faces: &[FaceId],
        surfaces: Vec<Surface>,
        moved: &[FaceId],
    ) {
        self.local_faces = faces.to_vec();
        self.local_surfaces = surfaces;
        self.moved_faces = moved.to_vec();
        self.edge_bounds.clear();
    }

    /// Not upstream: in a local operation, records the edges of `original` that keep their
    /// geometry: each whose faces, and every face at its vertices, keep their surfaces
    /// (each is given a copy of its own by [`Self::set_local_surfaces`], none is among
    /// its moved faces). A face given no surface there is offset by the executive's
    /// distance (the offset geometry is chosen by whether surfaces were given, not by the
    /// operation), so it moves, and no edge of it is kept. Such an edge is
    /// where its surfaces met before and its ends stay put, so its offset edges keep the
    /// face's own curves, not mated again (`EdgeuseOffset`, [`Self::keeps_edgeuse`]), and
    /// the merge does not intersect its faces' offsets again. An edge between two faces
    /// that stay but with a vertex on a moved face changes its length: it is intersected
    /// again, as the edges of moved faces are.
    ///
    /// # Errors
    ///
    /// [`SolidError::NotFound`] for a dangling id.
    pub(super) fn keep_unmoved_edges(&mut self, original: &Brep) -> Result<()> {
        self.kept_edges.clear();
        self.kept_edgeuses.clear();
        if self.operation != OffsetOperation::LocalOperation {
            return Ok(());
        }
        for e in bp1::edges(original, None)? {
            let mut faces = edge::faces(original, e, None)?;
            let (start, end) = vertex::edge_vertex_pair(original, e)?;
            for v in [start, end].into_iter().flatten() {
                faces.extend(vertex::faces(original, v)?);
            }
            let keeps_surface =
                |f: &FaceId| self.local_faces.contains(f) && !self.moved_faces.contains(f);
            if faces.is_empty() || !faces.iter().all(keeps_surface) {
                continue;
            }
            self.kept_edges.insert(e);
            self.kept_edgeuses
                .extend(edge::edgeuses(original, e, None, None)?);
        }
        Ok(())
    }

    /// Not upstream: whether the offsets of original edge use `edgeuse` keep their face's
    /// own curves ([`Self::keep_unmoved_edges`]).
    pub(super) fn keeps_edgeuse(&self, edgeuse: EdgeuseId) -> bool {
        self.kept_edgeuses.contains(&edgeuse)
    }

    /// Not upstream: whether the new surfaces of a local operation cross along the edge of
    /// original edge use `edgeuse` ([`super::reach::LocalReach::crossing`]).
    pub(super) fn crosses_at_edgeuse(&self, edgeuse: EdgeuseId) -> bool {
        self.crossing_edgeuses.contains(&edgeuse)
    }

    /// Sets `m_eOffsetOperation`.
    pub fn set_operation(&mut self, operation: OffsetOperation) {
        self.operation = operation;
    }

    /// Transfers ownership of a temporary B-rep to this executive.
    pub fn set_offset_brep(&mut self, brep: Brep) {
        self.offset_brep = Some(brep);
    }

    /// Adds a many-to-many correspondence. Repeating a pair is a no-op (`SmRelation`).
    pub fn relate_edgeuses(&mut self, original: EdgeuseId, offset: EdgeuseId) {
        // ManyToMany has no cardinality error and typed ids cannot be null.
        let _ = self.edgeuse_to_edgeuses.relate_pair(original, offset);
    }

    /// Relates an original face use to a newly constructed offset face.
    pub fn relate_faceuse(&mut self, original: FaceuseId, offset: FaceId) {
        let _ = self.faceuse_to_faces.relate_pair(original, offset);
    }

    /// Relates an original edge to its offset cap face.
    pub fn relate_edge_face(&mut self, original: EdgeId, offset: FaceId) {
        let _ = self.edge_to_faces.relate_pair(original, offset);
    }

    /// Relates an original vertex to its offset cap face.
    pub fn relate_vertex_face(&mut self, original: VertexId, offset: FaceId) {
        let _ = self.vertex_to_faces.relate_pair(original, offset);
    }

    /// Relates a corner blend to one of the faces it blends.
    pub fn relate_blend_face(&mut self, blend: FaceId, face: FaceId) {
        let _ = self.blend_to_faces.relate_pair(blend, face);
    }

    /// The mapped offset use only when there is exactly one (`GetOffsetEdgeuse`).
    pub fn offset_edgeuse(&self, original: EdgeuseId) -> Option<EdgeuseId> {
        self.edgeuse_to_edgeuses.second(original)
    }

    /// The offset faces related to a face use (`m_vFUToF.GetSeconds`; protected
    /// upstream, for harness observation), in id order.
    pub fn offset_faces(&self, original: FaceuseId) -> Vec<FaceId> {
        self.faceuse_to_faces.seconds(original)
    }

    /// Not upstream (WR-346, `IsCollapsedFace`): whether original face `face` is an
    /// inset face whose offset collapsed.
    pub fn is_collapsed_face(&self, face: FaceId) -> bool {
        self.collapsed_faces.contains(&face)
    }

    /// Not upstream (WR-346, `GetEdgeuseAcrossCollapsedFace`): the original edge use
    /// beyond a collapsed face, seen from its edge use `edgeuse`. Across a rail (a
    /// tangent edge of a four-sided round) it is the radial of the opposite rail; across
    /// an end edge, seen from `from` (one of that edge's vertices), the radial of the
    /// rail at the edge's other vertex. `None` when the edge use's face did not
    /// collapse, the face across collapsed too, or the face has another shape.
    ///
    /// # Errors
    ///
    /// Those of the topology queries and the tangency test.
    pub fn edgeuse_across_collapsed_face(
        &self,
        kernel: &Kernel,
        original: &mut Brep,
        edgeuse: Option<EdgeuseId>,
        from: Option<VertexId>,
    ) -> Result<Option<EdgeuseId>> {
        let Some(edgeuse) = edgeuse else {
            return Ok(None);
        };
        match eu::face(original, edgeuse)? {
            Some(face) if self.is_collapsed_face(face) => {}
            _ => return Ok(None),
        }
        let tan_tol_deg = self.tangency_tol_degrees();
        let next = if eu2::is_tangent_sector(kernel, original, edgeuse, tan_tol_deg, false, true)? {
            let loopuse = present(eu::loopuse(original, edgeuse)?, EU_NO_LOOPUSE)?;
            if eu::loopuse_edgeuses(original, loopuse)?.len() != 4 {
                return Ok(None);
            }
            let ccw = present(eu::ccw_edgeuse(original, edgeuse)?, EU_NO_NEIGHBOUR)?;
            eu::ccw_edgeuse(original, ccw)?
        } else {
            let Some(from) = from else {
                return Ok(None);
            };
            // the end edge's vertex away from `from` (by position: `from` may be a
            // vertex of the rail beside it)
            let end_edge = edge_of(original, edgeuse)?;
            let v0 = present(edge::start_vertex(original, end_edge)?, EDGE_NO_VERTEX)?;
            let v1 = present(edge::end_vertex(original, end_edge)?, EDGE_NO_VERTEX)?;
            let from_point = original.vertex(from)?.point;
            let far = if original.vertex(v0)?.point.distance(from_point)
                > original.vertex(v1)?.point.distance(from_point)
            {
                v0
            } else {
                v1
            };
            let mut found = None;
            for side in [
                present(eu::cw_edgeuse(original, edgeuse)?, EU_NO_NEIGHBOUR)?,
                present(eu::ccw_edgeuse(original, edgeuse)?, EU_NO_NEIGHBOUR)?,
            ] {
                let side_edge = edge_of(original, side)?;
                if side != edgeuse
                    && (edge::start_vertex(original, side_edge)? == Some(far)
                        || edge::end_vertex(original, side_edge)? == Some(far))
                {
                    found = Some(side);
                    break;
                }
            }
            found
        };
        let Some(next) = next else {
            return Ok(None);
        };
        let Some(across) = eu::radial(original, next)? else {
            return Ok(None);
        };
        if across == next {
            return Ok(None);
        }
        match eu::face(original, across)? {
            Some(face) if self.is_collapsed_face(face) => Ok(None),
            _ => Ok(Some(across)),
        }
    }

    /// The offset B-rep (a defect where it is not there: [`NO_OFFSET`]).
    #[track_caller]
    fn required_offset_brep(&self) -> Result<&Brep> {
        present(self.offset_brep.as_ref(), NO_OFFSET)
    }

    /// Temporarily lends ownership to another executive part, without copying topology.
    pub(super) fn take_offset_brep(&mut self) -> Option<Brep> {
        self.offset_brep.take()
    }

    /// Original mapped uses in the relation's deterministic first-id order.
    pub(super) fn mapped_original_edgeuses(&self) -> Vec<EdgeuseId> {
        self.edgeuse_to_edgeuses.all_firsts()
    }

    fn clear_relations(&mut self) {
        self.faceuse_to_faces.remove_all();
        self.edgeuse_to_edgeuses.remove_all();
        self.edge_to_faces.remove_all();
        self.vertex_to_faces.remove_all();
        self.blend_to_faces.remove_all();
        self.extension_reaches.clear();
        self.faceuse_extension_reaches.clear();
        self.edge_bounds.clear();
        self.crossing_edgeuses.clear();
        self.periodic_boundary_ends.clear();
        self.mated_edgeuses.clear();
        self.wall_extensions.clear();
    }

    /// Records how far the surface of original face `face` must be extended for its
    /// neighbours' offsets to meet its offset along `faceuse`
    /// ([`super::reach::extension_reach`] at the executive's offset distance, the shell's
    /// faces staying where they are), and one offset distance, so that the meetings cross
    /// the extended surface's boundary rather than touch it. In a local operation, for its
    /// neighbours' new surfaces to meet its own ([`super::reach::local_extension_reach`]),
    /// and the largest move beside it, and at least the input body's tolerance (the
    /// offset body takes it): a new edge must cross each neighbouring surface's boundary,
    /// not sit on it, and the mate and the intersection identify points within that
    /// tolerance, so a shorter crossing is indistinguishable from touching the boundary.
    ///
    /// # Errors
    ///
    /// Those of [`super::reach::extension_reach`] and
    /// [`super::reach::local_extension_reach`] (in a local operation, its refusal of a
    /// face beside a move whose reach has no certified bound).
    pub(super) fn record_extension_reach(
        &mut self,
        kernel: &Kernel,
        original: &mut Brep,
        face: FaceId,
        faceuse: FaceuseId,
    ) -> Result<()> {
        // The meetings must cross the extended surface's boundary, not touch it (a face
        // that stays where it is meets its neighbours' offsets at its edge, a tangent
        // neighbour's offset meets at the edge's image): the margin is the reach and the
        // offset's own size, so each meeting lies at least one offset inside the extension
        // (a tolerance-sized margin is below what an extension can certify). A face with
        // no neighbour meets no offset and is not extended. A local operation's faces move to
        // surfaces of their own, and the move takes the offset's place: the margin is the
        // reach of the new surfaces' meetings and the largest move beside the face, and
        // at least the body's tolerance.
        let reach = if self.local_faces.is_empty() {
            super::reach::extension_reach(
                kernel,
                original,
                faceuse,
                self.offset_distance,
                &self.shell_faces,
                self.tangency_tol_radians,
            )?
            .map_or(0.0, |reach| reach + self.offset_distance.abs())
        } else {
            // the moved faces' new surfaces, and the copies the others keep
            let (mut moved, mut kept) = (Vec::new(), Vec::new());
            for (f, surface) in self.local_faces.iter().zip(&self.local_surfaces) {
                if self.moved_faces.contains(f) {
                    moved.push((*f, surface));
                } else {
                    kept.push((*f, surface));
                }
            }
            let found = super::reach::local_extension_reach(
                kernel,
                original,
                faceuse,
                &moved,
                &kept,
                self.tangency_tol_radians,
                &mut self.edge_bounds,
            )?;
            for &e in found.iter().flat_map(|r| &r.crossing) {
                self.crossing_edgeuses
                    .extend(edge::edgeuses(original, e, None, None)?);
            }
            // A new edge must cross each neighbouring surface's boundary, not sit on it:
            // the mate and the intersection identify points within the body's tolerance,
            // so a crossing shorter than that is indistinguishable from touching the
            // boundary, and the extension reaches at least that far. With nothing moved
            // beside the face (a face given the plane through its own edges), this floor
            // is the only margin (the offset branch's is the offset's size).
            found.map_or(0.0, |r| (r.reach + r.shift).max(original.tolerance()))
        };
        self.extension_reaches.insert(face, reach);
        let (primary, mate) = crate::topology::face::p2::faceuses(original, face)?;
        for faceuse in [Some(faceuse), primary, mate].into_iter().flatten() {
            self.faceuse_extension_reaches.insert(faceuse, reach);
        }
        Ok(())
    }

    /// The largest reach recorded: the margin of a surface no recorded face accounts for
    /// (an edge's or vertex's cap, a face the offset did not make from one).
    pub(super) fn body_extension_reach(&self) -> f64 {
        self.extension_reaches.values().fold(0.0, |a, &b| a.max(b))
    }

    /// The margin the surface of original face `face` is extended by: its recorded reach.
    pub(super) fn original_extension_margin(&self, face: FaceId) -> f64 {
        self.extension_reaches
            .get(&face)
            .copied()
            .unwrap_or_else(|| self.body_extension_reach())
    }

    /// The margin the surface of offset face `face` is extended by: the reach of the
    /// original face use it was made from.
    pub(super) fn offset_extension_margin(&self, face: FaceId) -> f64 {
        let reaches: Vec<f64> = self
            .faceuse_to_faces
            .firsts(face)
            .iter()
            .filter_map(|faceuse| self.faceuse_extension_reaches.get(faceuse).copied())
            .collect();
        if reaches.is_empty() {
            self.body_extension_reach()
        } else {
            reaches.into_iter().fold(0.0, f64::max)
        }
    }

    /// Synchronizes mapped edge ranges, splitting offset edges at the other side's
    /// breaks with `MakeVertexSplitEdge`. `notifications` and `curve_ref` identify the
    /// offset B-rep's entities and edge curves for the split's notifications.
    pub fn synchronize_offset_edgeuses(
        &mut self,
        kernel: &mut Kernel,
        original: Option<EdgeuseId>,
        other: Option<EdgeuseId>,
        notifications: &mut NotifyEnv<'_>,
        curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<SynchronizedEdgeuses> {
        self.synchronize_offset_edgeuses_with_state(
            kernel,
            original,
            other,
            &mut NotifyState::from_env(notifications),
            curve_ref,
        )
    }

    /// Synchronizes through the kernel's actual context and caller-owned result stores.
    /// Like the external-environment entry point, this may split and notify result edges.
    pub fn synchronize_offset_edgeuses_with_kernel_context(
        &mut self,
        kernel: &mut Kernel,
        original: Option<EdgeuseId>,
        other: Option<EdgeuseId>,
        notifications: &mut NotificationStores<'_>,
        curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<SynchronizedEdgeuses> {
        self.synchronize_offset_edgeuses_with_state(
            kernel,
            original,
            other,
            &mut NotifyState::from_kernel_stores(notifications),
            curve_ref,
        )
    }

    fn synchronize_offset_edgeuses_with_state(
        &mut self,
        kernel: &mut Kernel,
        original: Option<EdgeuseId>,
        other: Option<EdgeuseId>,
        notifications: &mut NotifyState<'_>,
        curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<SynchronizedEdgeuses> {
        // no original edge use: the caller's input
        let original = original.ok_or_else(|| failed(OffsetFailure::EdgeuseNotGiven))?;
        let mut first = self.edgeuse_to_edgeuses.seconds(original);
        // An empty mapping does not dereference the B-rep upstream.
        if !first.is_empty() {
            remove_small_stuff(kernel, self.required_offset_brep()?, &mut first)?;
        }
        let mut second = other.map_or_else(Vec::new, |id| self.edgeuse_to_edgeuses.seconds(id));
        let mut out = SynchronizedEdgeuses::default();
        if first.is_empty() {
            // Deliberately do not filter the other side in this branch.
            if !second.is_empty() {
                sort_edgeuses(self.required_offset_brep()?, &mut second)?;
            }
            out.edgeuses.resize(second.len(), None);
            out.other_edgeuses = second.into_iter().map(Some).collect();
            return Ok(out);
        }

        let brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
        remove_small_stuff(kernel, brep, &mut second)?;
        if second.is_empty() {
            sort_edgeuses(brep, &mut first)?;
            out.other_edgeuses.resize(first.len(), None);
            out.edgeuses = first.into_iter().map(Some).collect();
            return Ok(out);
        }
        insert_breaks(kernel, brep, &first, &mut second, notifications, curve_ref)?;
        insert_breaks(kernel, brep, &second, &mut first, notifications, curve_ref)?;
        let brep = &*brep;
        sort_edgeuses(brep, &mut first)?;
        sort_edgeuses(brep, &mut second)?;
        extract_gaps(brep, &mut first, &second, &mut out.gaps)?;
        // The second extraction sees the already reduced first list, as upstream.
        extract_gaps(brep, &mut second, &first, &mut out.other_gaps)?;
        out.edgeuses = first.into_iter().map(Some).collect();
        out.other_edgeuses = second.into_iter().map(Some).collect();
        Ok(out)
    }

    /// Returns a cached extension, creating it `margin` past every side of `base`'s box
    /// (none for a margin of zero: the base itself), every side exactly ruled
    /// ([`Surface::extended_sides_keeping_parameters`],
    /// [`Continuity::Ruled`](crate::surface::extend::Continuity::Ruled); the construction is
    /// [`super::reach::extended_by_margin`], which the local reach's ball check builds too): a plane,
    /// cone, sphere or torus as itself, a ball patch with its pole (a side collapsed to a
    /// point) as a ball patch on the same sphere (one without a pole takes the generic ruled
    /// extension), an offset surface as the offset of its base extended. A hit never
    /// re-extends or replaces the cached surface.
    ///
    /// A procedural blend is extended as its net, the B-spline the face's surface is (its
    /// definition is the certificate's, within its bound of the net): the margin runs past
    /// where the definition holds. Along the spine the definition is
    /// continued only while each foot stays on its support, and a round's supports end with
    /// the face; across, its bound grows by the closed-form Chebyshev factor of the span
    /// continued (`surface::extend::blend`). A margin is a ruled piece of surface for the
    /// neighbours' offsets to meet, which the net continued gives at any distance, as for
    /// every other spline face (#17373).
    ///
    /// The margin is how far the face's offset must reach past the face for its
    /// neighbours' offsets to meet it ([`Self::original_extension_margin`],
    /// [`Self::offset_extension_margin`]: [`super::reach::extension_reach`]).
    ///
    /// Not upstream: upstream's `CreateExtendedSurface(.., SM_CT_G1R, ..)` extends by five
    /// offset distances, fits a ruled extension and removes knots within a tolerance, which
    /// leaves a cylinder or a ball patch's sphere at once (WR-532, WR-728).
    ///
    /// `base_key` represents upstream pointer identity, including across Rust clones.
    /// Distinct source identities must use distinct keys. Failed extensions are not cached.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a margin that is negative or not finite; those of
    /// the extension.
    pub fn get_create_extended_surface(
        &mut self,
        kernel: &Kernel,
        base_key: u64,
        base: &Surface,
        margin: f64,
    ) -> Result<&Surface> {
        if !(margin.is_finite() && margin >= 0.0) {
            return Err(SolidError::invalid_input(
                "offset::offset_executive::p1::get_create_extended_surface",
                "the margin is negative or not finite",
            ));
        }
        match self.extended_surfaces.entry(base_key) {
            std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
            std::collections::btree_map::Entry::Vacant(entry) => {
                // The obsolete offset-specific `else` at cpp:2158 is unreachable:
                // NER(pBase) already established pOrigSurf == pBase != NULL.
                let extension = if margin == 0.0 {
                    base.clone()
                } else {
                    // a blend as its net (above)
                    let net;
                    let base = match base.kind() {
                        SurfaceKind::ProceduralBlend(pb) => {
                            net = Surface::from(pb.net.clone());
                            &net
                        }
                        _ => base,
                    };
                    super::reach::extended_by_margin(kernel, base, margin)?
                };
                Ok(entry.insert(extension))
            }
        }
    }

    /// Caches `surface` as the extension of `base_key` (`m_vSurfExtSurf.Insert`), replacing
    /// any earlier entry. Upstream's map is protected; the offset difftest shims reach it
    /// through a subclass to seed every base with itself, so that a lookup never extends,
    /// and their runners seed the port's cache through this method.
    pub fn insert_extended_surface(&mut self, base_key: u64, surface: Surface) {
        self.extended_surfaces.insert(base_key, surface);
    }

    /// Replaces implicit offset surfaces with certified nets (`ReplaceImplicitOffsets`).
    /// The unconditional initial shrink is `topology.brep_p4`'s `ShrinkGeometry`.
    ///
    /// Every face with an `SmOffsetSurface` gets a bicubic B-spline within
    /// `approximation_tol_3d` of the exact offset of its extended (else its own) base at
    /// every parameter, in the base's parametrisation
    /// ([`crate::offset::construction::sheets::certified_offset`]; not upstream, WR-718:
    /// upstream fits NLib's `N_SrfOffset` and checks it at samples). The base must be a
    /// B-spline surface. All nets are built before any face is
    /// changed; each face is then notified (`PRE_EDIT`), given the new surface's
    /// natural domain and the surface (`SmBrep::ReplaceSurface`, old surface to the
    /// deleted list, UV trim curves rebuilt) and notified again (`POST_EDIT`), in
    /// face order. `notifications` must be keyed by `brep`'s identities.
    ///
    /// # Errors
    ///
    /// [`OffsetFailure::OffsetBaseKindUnsupported`] (`SM_ERR`) for a base that is not a
    /// B-spline surface; those of `certified_offset`
    /// and of `ReplaceSurface` (`SER`). The notifications' statuses are not checked.
    pub fn replace_implicit_offsets(
        kernel: &mut Kernel,
        brep: &mut Brep,
        approximation_tol_3d: f64,
        notifications: &mut NotificationStores<'_>,
    ) -> Result<()> {
        // remove curve and surface portions not used by edges and faces (status
        // ignored upstream)
        shrink_geometry(kernel, brep)?;

        // for every face
        let mut fix = Vec::new();
        for f in bp1::faces(brep, None)? {
            kernel.check_cancelled(OP)?;
            // skip non SmOffsetSurfaces
            let Some(SurfaceKind::OffsetSurface(offset)) =
                brep.face(f)?.surface.as_ref().map(Surface::kind)
            else {
                continue;
            };
            // get SmOffsetSurface base (use extended surface if there is one)
            let base = offset
                .extended_base_surface()
                .unwrap_or_else(|| offset.base_surface());
            // make sure base surface is an SmBSplineSurface (`SM_CAST_PTR`, which every
            // subclass passes: plane, extrusion, revolution, cone, sphere, torus); its
            // B-spline part with the NURB `GetOrCreateGwNurbPointer` makes when missing
            let Some(nurb) = crate::topology::brep::p3::with_nurb(kernel, base)? else {
                // a kind with no B-spline form (an offset surface: an offset of an offset)
                return Err(failed(OffsetFailure::OffsetBaseKindUnsupported {
                    surface_type: base.type_code(),
                }));
            };
            // Not upstream (WR-718, #2350 S1): the offset certified within the tolerance
            // at every parameter, in the base's parametrisation, where upstream fits
            // NLib's `N_SrfOffset` and checks it at samples (the cancel token polled
            // inside, #2136 / #2372)
            let base_nurb = present(
                nurb.gw_nurb(),
                "the base's B-spline form has no NURB, though `with_nurb` makes it from the \
                 surface's own exact definition where it is missing",
            )?;
            let made = crate::offset::construction::sheets::certified_offset(
                kernel,
                base_nurb,
                offset.offset_distance(),
                approximation_tol_3d,
            )?;
            let approximation =
                crate::nurbs::b_spline_surface::p1::BSplineSurface::from_nurb(made.surface, false);
            // remember face/surface pair
            fix.push((f, Surface::from(approximation)));
        }

        // for every face that had an approximate surface built for an exact offset
        // surface
        let mut state = NotifyState::from_kernel_stores(notifications);
        for (f, new_surface) in fix {
            state.with_kernel_env(kernel, |kernel, env| {
                let face_ref = (env.key_of)(EntityRef::Face(f));
                // SM_NO_GET_BREP is NULL in the pinned reference
                ignore_status(face3::notify(
                    brep,
                    f,
                    env,
                    None,
                    NotifyOperation::PreEdit,
                    [Some(face_ref), None, None],
                ))?;
                let domain = new_surface.natural_uv_domain_with_nurb(kernel)?;
                crate::topology::face::p2::set_uv_domain(brep, f, &domain)?;
                // swap the approx surface for the exact offset surface
                crate::topology::brep::p2::replace_surface(
                    kernel,
                    brep,
                    env,
                    f,
                    new_surface,
                    face_ref,
                    crate::topology::brep::p2::OldSurface::AddToDeletedList,
                    true,
                )?;
                ignore_status(face3::notify(
                    brep,
                    f,
                    env,
                    None,
                    NotifyOperation::PostEdit,
                    [Some(face_ref), None, None],
                ))
            })?;
        }
        Ok(())
    }

    /// Removes result faces too close to the original (`RemoveExcessFaces`).
    ///
    /// The source is borrowed immutably while solving, corresponding to the upstream
    /// editing-disabled query; the removal uses the result's actual editing flag and
    /// the kernel's traversal marks. An empty deletion list still calls the landed
    /// `RemoveFaces` implementation.
    /// Each original edge and face is identified by its geometry's owner key
    /// ([`arena_identity`] when unowned); an owner shared by two records or naming
    /// another record returns [`SolidError::Unsupported`] rather than omitting geometry.
    /// Cache and solver errors propagate before any result-face removal. Ordinary
    /// evaluation statuses are ignored, but an unported evaluator still defers.
    /// Result edges are visited in upstream edge list order (`SmBrep::GetEdges`).
    pub fn remove_excess_faces(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        result: &mut Brep,
    ) -> Result<()> {
        let Some(delete_faces) = self.excess_faces(kernel, original, result)? else {
            return Ok(());
        };

        // The faces about to be removed and their edgeuses, which RemoveFaces deletes
        let gone_faces: BTreeSet<FaceId> = delete_faces.iter().copied().collect();
        let gone = removed_faces_edgeuses(result, &delete_faces)?;

        // Clean up those faces we have collected.
        let editing_enabled = result.editing_enabled();
        // cpp:2099–2102 ignores RemoveFaces' status; it is passed on here (WR-658,
        // tangency-labs/smlib 4803d6d).
        crate::topology::brep::p5::remove_faces(kernel, result, editing_enabled, &delete_faces)?;

        // Forget the deleted faces and edgeuses, so later lookups in the relations
        // (lamina side walls, PiecewiseMerge) cannot return them. For m_vEUToEU also
        // forget any edgeuse no result edge has.
        if !gone_faces.is_empty() {
            let mut live = BTreeSet::new();
            for e in bp1::edges(result, None)? {
                live.extend(edge::edgeuses(result, e, None, None)?);
            }
            forget_seconds(&mut self.edgeuse_to_edgeuses, &gone, Some(&live))?;
            self.forget_faces(&gone_faces)?;
        }
        Ok(())
    }

    /// `RemoveExcessFaces`' forgetting of the removed faces: `m_vFUToF`, `m_vEToF`,
    /// `m_vVToF` and `m_vBlendToFaces` forget the pairs whose second is gone, and
    /// `m_vBlendToFaces` also those whose first is gone.
    pub(super) fn forget_faces(&mut self, gone: &BTreeSet<FaceId>) -> Result<()> {
        forget_seconds(&mut self.faceuse_to_faces, gone, None)?;
        forget_seconds(&mut self.edge_to_faces, gone, None)?;
        forget_seconds(&mut self.vertex_to_faces, gone, None)?;
        forget_seconds(&mut self.blend_to_faces, gone, None)?;
        forget_firsts(&mut self.blend_to_faces, gone)
    }

    fn excess_faces(
        &self,
        kernel: &Kernel,
        original: &mut Brep,
        result: &mut Brep,
    ) -> Result<Option<Vec<FaceId>>> {
        if !self.merge_results || !self.shell_faces.is_empty() {
            return Ok(None);
        }
        let min_distance = self.offset_distance * 0.95;
        let mut delete_faces = Vec::new();
        let mut original_cache = None;
        // SmBrep::GetEdges (unit topology.brep_p1): the m_pEdgeListHead list order
        for id in crate::topology::brep::p1::edges(result, None)? {
            let primary = present(eu::primary_edgeuse(result, id)?, EDGE_NO_EDGEUSE)?;
            // NormalizedEvaluate(0.5, FALSE, sPnt): 3d. Upstream ignores the evaluation
            // status and keeps the initialized point; it is passed on here (WR-658).
            let point =
                eu3::normalized_evaluate(kernel, result, primary, 0.5, false, false, true)?.point;
            // cpp:2069–2072: the solve runs with the original's editing disabled and
            // leaves it enabled afterwards (still disabled when the solve fails)
            original.set_editing_enabled(false);
            let (cache, keys) = &*match &mut original_cache {
                Some(made) => made,
                empty => empty.insert(make_original_brep_cache(kernel, original)?),
            };
            let solutions = solve_original_brep_point(kernel, original, cache, keys, point)?;
            original.set_editing_enabled(true);
            collect_excess_faces(
                result,
                id,
                solutions.as_slice(),
                min_distance,
                &self.shell_faces,
                &mut delete_faces,
            )?;
        }
        // All candidates are gathered in first-seen order, without duplicates.
        Ok(Some(delete_faces))
    }

    /// Builds a closing ruled side wall, or succeeds without work for null/ambiguous mappings.
    /// The new face is named in an arena-only store (`topology::loopuse::notify_ref`).
    pub fn build_side_wall_of_edgeuse(
        &mut self,
        kernel: &mut Kernel,
        original_brep: &mut Brep,
        original: Option<EdgeuseId>,
    ) -> Result<()> {
        self.build_side_wall_of_edgeuse_with(kernel, original_brep, original, None)
    }

    /// [`Self::build_side_wall_of_edgeuse`] notifying the new face through
    /// `notifications`, keyed by result identities (`None`: an arena-only store).
    pub(super) fn build_side_wall_of_edgeuse_with(
        &mut self,
        kernel: &mut Kernel,
        original_brep: &mut Brep,
        original: Option<EdgeuseId>,
        notifications: Option<&mut NotifyState<'_>>,
    ) -> Result<()> {
        let Some(original) = original else {
            return Ok(());
        };
        let Some(offset) = self.offset_edgeuse(original) else {
            return Ok(());
        };
        let offset_brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
        // cpp:2812: ruled blend (0) between the original edge use and its offset edge use
        // (in the offset B-rep), curves in the same direction (1), no blend options
        let new_surface = crate::topology::brep::p1::create_edge_edge_blend(
            kernel,
            0,
            original_brep,
            Some(original),
            Some(offset_brep),
            Some(offset),
            1,
            None,
        )?;
        // cpp:2819: m_pOffsetBrep->CreateFaceFromSurface(pNewSurface, natural domain)
        let domain = new_surface.natural_uv_domain_with_nurb(kernel)?;
        let new_face = match notifications {
            Some(notifications) => {
                let key_of = notifications.key_of;
                let this_face = |face: FaceId| key_of(EntityRef::Face(face));
                notifications.with_kernel_env(kernel, |kernel, env| {
                    let mut marks = crate::topology::topology_traverser::Marks::default();
                    let mut fenv = crate::topology::brep::p8::FaceBuildEnv {
                        env,
                        marks: &mut marks,
                        this_face: &this_face,
                    };
                    crate::topology::brep::p8::create_face_from_surface(
                        kernel,
                        offset_brep,
                        Some(new_surface),
                        &domain,
                        &mut fenv,
                    )
                })?
            }
            None => crate::topology::brep::sheet::face_on(
                kernel,
                offset_brep,
                new_surface.clone(),
                &domain,
            )?,
        };
        self.closing_faces.push(new_face); // [B549]
        Ok(())
    }

    /// Builds edge cap offsets (`BuildOffsetOfEdgeuses`). Notifications use distinct
    /// original/result object keys; `offset_curve_ref` identifies offset edge curves
    /// split while synchronizing. Every synchronized pair reaches the shared
    /// `EdgeuseOffset` creator; a cap surface becomes a face of the offset B-rep that
    /// the original edge is related to. Edge-use marks are read from the stores'
    /// topology records under `mark_type`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_offset_of_edgeuses(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        edgeuse: EdgeuseId,
        other: EdgeuseId,
        mark_type: MarkType,
        notifications: &mut NotifyEnv<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        self.build_offset_of_edgeuses_with_state(
            kernel,
            original,
            edgeuse,
            other,
            mark_type,
            MarkSource::Records,
            &mut NotifyState::from_env(notifications),
            offset_key_of,
            offset_curve_ref,
        )
    }

    /// Builds the edge cap offsets with real caller stores and the kernel context.
    /// `notifications.key_of` identifies source entities; `offset_key_of` identifies
    /// result entities, including any edges created by synchronization. `mark_type`
    /// is the existing traversal mark, not a request to acquire or advance one.
    #[allow(clippy::too_many_arguments)]
    pub fn build_offset_of_edgeuses_with_kernel_context(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        edgeuse: EdgeuseId,
        other: EdgeuseId,
        mark_type: MarkType,
        notifications: &mut NotificationStores<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        self.build_offset_of_edgeuses_with_state(
            kernel,
            original,
            edgeuse,
            other,
            mark_type,
            MarkSource::Records,
            &mut NotifyState::from_kernel_stores(notifications),
            offset_key_of,
            offset_curve_ref,
        )
    }

    /// As [`Self::build_offset_of_edgeuses_with_kernel_context`], with the original
    /// edge uses the calling loop has marked as the mark source.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_offset_of_edgeuses_with_marked(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        edgeuse: EdgeuseId,
        other: EdgeuseId,
        mark_type: MarkType,
        marked: &BTreeSet<EdgeuseId>,
        notifications: &mut NotificationStores<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        self.build_offset_of_edgeuses_with_state(
            kernel,
            original,
            edgeuse,
            other,
            mark_type,
            MarkSource::Set(marked),
            &mut NotifyState::from_kernel_stores(notifications),
            offset_key_of,
            offset_curve_ref,
        )
    }

    /// Not upstream (WR-346): `m_rGeometryCreation.MateEdges(ctx, this, pEU, pAcross,
    /// pOffEU, pOffAcross, eMarkType)`, mating the offsets of the faces on either side
    /// of a collapsed face (`edgeuse` and `across` are original edge uses beside it).
    ///
    /// # Errors
    ///
    /// Those of `MateEdges`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mate_edges_across_with_marked(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        edgeuse: EdgeuseId,
        across: EdgeuseId,
        offset_edgeuse: EdgeuseId,
        offset_across: EdgeuseId,
        mark_type: MarkType,
        marked: &BTreeSet<EdgeuseId>,
        notifications: &mut NotificationStores<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
    ) -> Result<()> {
        let creator = self.geometry_creator();
        NotifyState::from_kernel_stores(notifications).with_kernel_env(kernel, |kernel, env| {
            let mut result = present(self.offset_brep.take(), NO_OFFSET)?;
            let made = super::edgeuse::mate(
                kernel,
                &creator,
                self,
                original,
                &mut result,
                edgeuse,
                across,
                offset_edgeuse,
                offset_across,
                mark_type,
                MarkSource::Set(marked),
                CapNotifications {
                    records: &mut *env.records,
                    attributes: &mut *env.attributes,
                    context: env.context.as_deref_mut(),
                    original_key_of: env.key_of,
                    offset_key_of,
                },
            );
            self.offset_brep = Some(result);
            made
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn build_offset_of_edgeuses_with_state(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        edgeuse: EdgeuseId,
        other: EdgeuseId,
        mark_type: MarkType,
        marks: MarkSource<'_>,
        notifications: &mut NotifyState<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        let Some((edge, synchronized)) = self.prepare_offset_edgeuses(
            kernel,
            original,
            edgeuse,
            other,
            &mut notifications.rekey(offset_key_of),
            offset_curve_ref,
        )?
        else {
            return Ok(());
        };
        let creator = self.geometry_creator();
        let mut new_faces = Vec::new();
        // cpp:2918–3071: every mapped offset edge-use pair may need a cap face.
        for (index, offset) in synchronized.edgeuses.iter().copied().enumerate() {
            let mut other_offset = paired_other(&synchronized, index, edge)?;
            // clear lamina other offset edges
            if eu::edge_is_lamina(original, edge)? {
                other_offset = None;
            }
            if let Some(offset) = offset {
                let result = self.required_offset_brep()?;
                let result_edge = edge_of(result, offset)?;
                notifications.with_env(kernel, |env| {
                    notify_edge_and_vertices(
                        original,
                        edge,
                        result,
                        result_edge,
                        env,
                        offset_key_of,
                    )
                })?;
            }
            // SER(m_rGeometryCreation.EdgeuseOffset(..)): the creator edits both
            // B-reps through the caller's stores and the selected context.
            let cap = notifications.with_kernel_env(kernel, |kernel, env| {
                let mut result = present(self.offset_brep.take(), NO_OFFSET)?;
                let made = super::edgeuse::build(
                    kernel,
                    &creator,
                    self,
                    original,
                    &mut result,
                    edgeuse,
                    other,
                    offset,
                    other_offset,
                    mark_type,
                    marks,
                    CapNotifications {
                        records: &mut *env.records,
                        attributes: &mut *env.attributes,
                        context: env.context.as_deref_mut(),
                        original_key_of: env.key_of,
                        offset_key_of,
                    },
                );
                self.offset_brep = Some(result);
                made
            })?;
            let Some(surface) = cap.surface else {
                continue;
            };
            // Build a face in m_pOffsetBrep for the new cap surface; its own
            // `sLoopPoints` is empty.
            let new_face = self.make_cap_face(
                kernel,
                surface,
                cap.trim_curves,
                cap.trim_orientations,
                &[],
                &mut notifications.rekey(offset_key_of),
            )?;
            // Not upstream (WR-530 addendum, fork
            // tangency-labs/smlib 5d428ae): a step wall spans the gap between two edges
            // exactly, as a lamina side wall does, and is kept out of the face extensions
            // as one (extended along its rulings it can pass through its own axis, a step
            // round a circle, and fold over itself).
            if cap.closing_wall {
                self.closing_faces.push(new_face);
            }
            // map origEdge to its offset capFaces
            new_faces.push(new_face);
            self.relate_edge_face(edge, new_face);
        }

        self.fill_offset_edgeuse_gaps(
            kernel,
            original,
            edge,
            &synchronized,
            &new_faces,
            &mut notifications.rekey(offset_key_of),
        )
    }

    /// The geometry creator this executive's scalars stand for
    /// (`m_rGeometryCreation`), with its angular tolerance.
    fn geometry_creator(&self) -> OffsetGeometryCreation {
        OffsetGeometryCreation::new(self.offset_distance, self.approximation_tol_3d)
            .with_tangency_tol_radians(self.tangency_tol_radians)
    }

    /// Shared prefix; synchronization mutates/notifies the result exactly once.
    /// `None` is the selected-shell-face no-op, not an empty synchronization.
    fn prepare_offset_edgeuses(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        edgeuse: EdgeuseId,
        other: EdgeuseId,
        notifications: &mut NotifyState<'_>,
        curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<Option<(EdgeId, SynchronizedEdgeuses)>> {
        let edge = edge_of(original, edgeuse)?;
        let first_face = eu::face(original, edgeuse)?;
        let other_face = eu::face(original, other)?;
        if first_face.is_some_and(|id| self.shell_faces.contains(&id))
            && other_face.is_some_and(|id| self.shell_faces.contains(&id))
            && !self.extend_convex_edges
        {
            return Ok(None);
        }
        let synchronized = self.synchronize_offset_edgeuses_with_state(
            kernel,
            Some(edgeuse),
            Some(other),
            notifications,
            curve_ref,
        )?;
        Ok(Some((edge, synchronized)))
    }

    /// Tail after the synchronized pair loop, shared with p2: the gap fills' blend
    /// faces notify through `notifications` (keyed by result identities).
    /// Both gap calls run before either status is inspected, as upstream.
    pub(super) fn fill_offset_edgeuse_gaps(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        edge: EdgeId,
        synchronized: &SynchronizedEdgeuses,
        new_faces: &[FaceId],
        notifications: &mut NotifyState<'_>,
    ) -> Result<()> {
        let first = self.fill_gap_edgeuses_with_state(
            kernel,
            edge,
            &synchronized.gaps,
            new_faces,
            notifications,
        );
        let second = self.fill_gap_edgeuses_with_state(
            kernel,
            edge,
            &synchronized.other_gaps,
            new_faces,
            notifications,
        );
        let mut problems = false;
        for status in [first, second] {
            match status {
                Ok(()) => {}
                // a defect (the kernel's fault) is passed on, never read as a gap left open
                // (`solidcore/docs/failure-codes-and-reproduction.md` §6, rule 5)
                Err(error) if error.is_unported_or_cancelled() || error.is_defect() => {
                    return Err(error)
                }
                Err(_) => problems = true,
            }
        }
        if problems {
            use crate::primitive::primitive_creation::model::PrimitiveModel;
            use crate::primitive::primitive_creation::p1::PrimitiveCreation;

            // cpp:3088-3094: a pipe of the offset distance along the edge's trimmed NURBS
            // curve, built into the offset B-rep (SmPrimitiveCreation on its infinite
            // region, no caps, not translational); the faces it makes are not used.
            let curve = edge::create_trimmed_nurbs_curve(kernel, original, edge)?;
            let Some(CurveKind::BSpline(path)) = curve.as_ref().map(Curve::kind) else {
                // a NULL curve handed to CreatePipeSweep upstream
                return Err(no_trimmed_curve(original, edge));
            };
            let offset = present(self.offset_brep.take(), NO_OFFSET)?;
            let region = offset.infinite_region();
            let mut model = PrimitiveModel::new(kernel, offset);
            let (mut start, mut side, mut end) = (Vec::new(), Vec::new(), Vec::new());
            let swept = PrimitiveCreation::new(region, 0, false).create_pipe_sweep(
                &mut model,
                PrimitiveModel::MAIN,
                self.offset_distance,
                path,
                self.approximation_tol_3d,
                false,
                false,
                &mut start,
                &mut side,
                &mut end,
            );
            // the offset B-rep comes back whatever the sweep's status (SER)
            self.offset_brep = Some(model.into_brep()?);
            swept?;
        }
        Ok(())
    }

    /// Fills unmatched edge gaps (`FillGapEdgeuses`). Empty gaps succeed without
    /// dereferencing any topology. For each gap use (3D midpoint first), every
    /// candidate-face vertex within the summed vertex tolerances of either end
    /// contributes its incident edge whose far vertex is closest to the midpoint; the
    /// candidate list persists across gap uses. Unless there are exactly two
    /// candidates this returns success; otherwise the gap use and the candidates'
    /// same-oriented uses get a corner blend whose surfaces become offset faces,
    /// related to `edge` and to the faces they blend. `notifications` identifies the
    /// offset B-rep's entities for the new faces' notifications.
    pub fn fill_gap_edgeuses(
        &mut self,
        kernel: &mut Kernel,
        edge: EdgeId,
        gaps: &[EdgeuseId],
        candidate_faces: &[FaceId],
        notifications: &mut NotifyEnv<'_>,
    ) -> Result<()> {
        self.fill_gap_edgeuses_with_state(
            kernel,
            edge,
            gaps,
            candidate_faces,
            &mut NotifyState::from_env(notifications),
        )
    }

    fn fill_gap_edgeuses_with_state(
        &mut self,
        kernel: &mut Kernel,
        edge: EdgeId,
        gaps: &[EdgeuseId],
        candidate_faces: &[FaceId],
        notifications: &mut NotifyState<'_>,
    ) -> Result<()> {
        if gaps.is_empty() {
            return Ok(());
        }
        let mut all_vertices = Vec::new();
        {
            let brep = self.required_offset_brep()?;
            for &face in candidate_faces {
                all_vertices.extend(face_vertices(brep, face)?);
            }
        }
        let mut candidate_edges: Vec<Option<EdgeId>> = Vec::new();
        for &gap in gaps {
            let brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
            // NormalizedEvaluate(0.5, FALSE, sMidPoint): 3d
            let mid_point =
                eu3::normalized_evaluate(kernel, brep, gap, 0.5, false, false, true)?.point;

            // Look at each end of the Gap EU
            for jj in 0..2 {
                let end_use = if jj == 1 {
                    present(eu::mate(brep, gap)?, EU_NO_MATE)?
                } else {
                    gap
                };
                let v = start_vertex(brep, end_use)?;
                let v_pnt = brep.vertex(v)?.point;
                let v_tol = vertex::tolerance(brep, v)?;

                // Look at each vertex of the candidate faces
                for &test_v in &all_vertices {
                    let tol = vertex::tolerance(brep, test_v)? + v_tol;
                    let test_pnt = brep.vertex(test_v)?.point;

                    // See if a test vertex is on current end of Gap EU
                    if test_pnt.distance(v_pnt) < tol {
                        let mut closest = None;
                        let mut closest_dist = BIG_DOUBLE;
                        for e in vertex::edges(brep, test_v, None)? {
                            let other_v = present(
                                edge::other_vertex(brep, e, Some(test_v))?,
                                EDGE_NO_VERTEX,
                            )?;
                            let dist = mid_point.distance(brep.vertex(other_v)?.point);
                            if dist < closest_dist {
                                closest_dist = dist;
                                closest = Some(e);
                            }
                        }
                        candidate_edges.push(closest);
                    }
                }
            }

            let [first, second] = candidate_edges.as_slice() else {
                // MSG(_T("Possible Problem In Offset"));
                return Ok(());
            };

            // sBlendedFaces may hold a null face (a wire edge use); RelatePair rejects it.
            let mut blended_faces: Vec<Option<FaceId>> = Vec::new();
            let mut edgeuses = vec![gap];
            for candidate in [*first, *second] {
                // upstream dereferences a null closest edge or same-oriented edge use. No
                // closest edge: the vertex near the gap's end has no edge (a vertex loop's)
                let e =
                    candidate.ok_or_else(|| failed(OffsetFailure::GapEndWithoutEdge { edge }))?;
                let same = present(edge::same_oriented_edgeuse(brep, e)?, EDGE_NO_SAME_EDGEUSE)?;
                edgeuses.push(same);
                let face = eu::face(brep, same)?;
                if !blended_faces.contains(&face) {
                    blended_faces.push(face);
                }
            }

            // We should now have enough information to make a blend.
            let blend_surfaces = match bp1::create_corner_blend(
                kernel,
                brep,
                &edgeuses,
                None,
                self.approximation_tol_3d,
                self.tangency_tol_radians,
            ) {
                Ok(surfaces) => surfaces,
                Err(error) if error.is_unported_or_cancelled() => return Err(error),
                // `SER(SM_ERR)` upstream: the blend's error is kept and decides the class (a
                // defect is passed on, never read as a blend that could not be made)
                Err(error) => {
                    return Err(
                        Failure::new(OP, OffsetFailure::GapCornerBlendNotMade { edge })
                            .caused_by(error)
                            .into_error(),
                    )
                }
            };

            for surface in blend_surfaces {
                let new_face = self.make_cap_face(
                    kernel,
                    Surface::from(surface),
                    Vec::new(),
                    Vec::new(),
                    &[],
                    notifications,
                )?;
                self.relate_edge_face(edge, new_face);
                // SER(m_vBlendToFaces.RelateToMany(pNewFace, sBlendedFaces)): pairs
                // before a null face stay related.
                for &face in &blended_faces {
                    let face = face.ok_or_else(|| {
                        SolidError::invalid_input(OP, "RelatePair: null blended face")
                    })?;
                    self.blend_to_faces.relate_pair(new_face, face)?;
                }
            }
        } // end iter every GapEdgeuse
        Ok(())
    }

    /// Builds a vertex cap from its circular/short-line offset boundary edges
    /// (`BuildOffsetOfVertex`), using the original vertex's geometry and tolerance.
    pub fn build_offset_of_vertex(
        &mut self,
        k: &Kernel,
        original: &Brep,
        vertex: Option<VertexId>,
    ) -> Result<VertexOffset> {
        let vertex = vertex.ok_or_else(|| failed(OffsetFailure::VertexNotGiven))?;
        let point = original.vertex(vertex)?.point;
        let result = self.required_offset_brep()?;
        let tol = result.tolerance();
        let radius = self.offset_distance;
        let mut edges = vertex::edges(original, vertex, None)?;
        let mut edges_to_blend = Vec::new();

        if let [only] = edges.as_slice() {
            let mut edgeuse = present(eu::primary_edgeuse(original, *only)?, EDGE_NO_EDGEUSE)?;
            if start_vertex(original, edgeuse)? != vertex {
                edgeuse = present(eu::radial(original, edgeuse)?, EU_NO_RADIAL)?;
            }
            if eu::is_strut(original, edgeuse)? {
                let faceuse = present(eu::faceuse(original, edgeuse)?, EU_NO_FACEUSE)?;
                let mut faces = self.faceuse_to_faces.seconds(faceuse);
                if faces.is_empty() {
                    // GetSeconds accepts a null key and returns an empty list upstream.
                    faces = original
                        .faceuse(faceuse)?
                        .mate
                        .map_or_else(Vec::new, |mate| self.faceuse_to_faces.seconds(mate));
                }
                for face in faces {
                    // Upstream reuses sEdges; on this strut branch it returns below.
                    edges = face_edges(result, face)?;
                    for &edge in &edges {
                        let curve = bspline_edge_curve(result, edge)?;
                        if let Some((frame, arc_radius, _, _)) = curve.is_arc(k, 10, tol)? {
                            if arc_radius < self.offset_distance
                                && frame.origin().distance(point) < self.offset_distance + tol
                            {
                                edges_to_blend.push(edge);
                            }
                        }
                    }
                }
                if edges_to_blend.len() != 1 {
                    return Ok(VertexOffset::default());
                }
                return self.vertex_offset_geometry(k, original, vertex, &edges_to_blend);
            }
        }

        let mut offset_faces = Vec::new();
        for &edge in &edges {
            let faces = self.edge_to_faces.seconds(edge);
            if faces.is_empty() {
                return Ok(VertexOffset::default());
            }
            offset_faces.extend(faces);
        }
        for face in offset_faces {
            for edge in face_edges(result, face)? {
                let curve = bspline_edge_curve(result, edge)?;
                if let Some((frame, _, _, _)) = curve.is_arc(k, 10, tol)? {
                    if frame.origin().distance(point) < tol {
                        edges_to_blend.push(edge);
                    }
                } else if let Some((line_point, line_vector)) = curve.is_line(k, 10, tol)? {
                    if (line_vector.length() - radius).abs() < tol {
                        let (distance, _) = crate::geom::geom_utility::p1::segment_point_distance(
                            line_point,
                            line_point + line_vector,
                            point,
                        );
                        if distance < tol {
                            edges_to_blend.push(edge);
                        }
                    }
                }
            }
        }
        // This call is unconditional, even when the vertex has no incident edges.
        self.vertex_offset_geometry(k, original, vertex, &edges_to_blend)
    }

    fn vertex_offset_geometry(
        &mut self,
        kernel: &Kernel,
        original: &Brep,
        vertex: VertexId,
        bounding_edges: &[EdgeId],
    ) -> Result<VertexOffset> {
        // VertexOffset uses only these creator parameters, not the angular or
        // analytic-conversion settings used by the edge/face creator methods.
        let creator = OffsetGeometryCreation::new(self.offset_distance, self.approximation_tol_3d);
        super::vertex::build(
            kernel,
            &creator,
            original,
            vertex,
            present(self.offset_brep.as_mut(), NO_OFFSET)?,
            bounding_edges,
        )
    }

    /// Builds a cap face for a new edge or vertex cap surface in the offset B-rep:
    /// `CreateFaceFromSurface` over the surface's natural domain without trimming
    /// curves, else `MakeFaceWithCurves` in the infinite region with one loop of all
    /// curves, no UV curves, `SM_OT_SAME` and the caller's loop points.
    /// `notifications` must be keyed by result identities.
    pub(super) fn make_cap_face(
        &mut self,
        kernel: &mut Kernel,
        surface: Surface,
        curves: Vec<Curve>,
        orientations: Vec<OrientType>,
        loop_points: &[Vec3],
        notifications: &mut NotifyState<'_>,
    ) -> Result<FaceId> {
        let domain = surface.natural_uv_domain_with_nurb(kernel)?;
        let key_of = notifications.key_of;
        let this_face = |face: FaceId| key_of(EntityRef::Face(face));
        let brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
        if curves.is_empty() {
            return notifications.with_kernel_env(kernel, |kernel, env| {
                let mut marks = crate::topology::topology_traverser::Marks::default();
                let mut fenv = crate::topology::brep::p8::FaceBuildEnv {
                    env,
                    marks: &mut marks,
                    this_face: &this_face,
                };
                crate::topology::brep::p8::create_face_from_surface(
                    kernel,
                    brep,
                    Some(surface),
                    &domain,
                    &mut fenv,
                )
            });
        }
        let region = brep.infinite_region();
        let input = FaceCurvesInput {
            loops: vec![curves.len()],
            curves_3d: Some(curves.into_iter().map(Some).collect()),
            uv_curves: None,
            orientations,
            loop_points: loop_points.to_vec(),
        };
        let made = bp7::make_face_with_curves_with_state(
            kernel,
            brep,
            region,
            input,
            surface,
            &domain,
            OrientType::Same,
            None,
            &this_face,
            notifications,
        )?;
        present(made.face, NO_NEW_FACE)
    }
}

impl OffsetExecutive {
    /// Not upstream (tangency-labs/solidcore#3138): marks every face of the offset with how
    /// it was made ([`ids::OFFSET_ROLE`]), from the executive's relations as they stand
    /// before the merge, once every face that replaced another (the crossing loops' rebuilt
    /// faces) has taken its place in them: a face use's offset copy, a face along an edge,
    /// a face at a vertex; a closing face (a sheet's side wall, made along a lamina edge) is
    /// a face along an edge too. Every face of `original` and of the offset first loses the
    /// role an earlier offset left on it. A related face no longer in the offset is passed
    /// over; a face related twice keeps its first role (copies, then edges, then vertices,
    /// then closing faces).
    ///
    /// # Errors
    ///
    /// Those of the attribute store; without an offset B-rep there is nothing to mark.
    fn mark_offset_roles(&mut self, original: &mut Brep) -> Result<()> {
        let mut roles: Vec<(FaceId, i64)> = Vec::new();
        for fu in self.faceuse_to_faces.all_firsts() {
            for f in self.faceuse_to_faces.seconds(fu) {
                roles.push((f, ids::OFFSET_ROLE_COPY));
            }
        }
        for e in self.edge_to_faces.all_firsts() {
            for f in self.edge_to_faces.seconds(e) {
                roles.push((f, ids::OFFSET_ROLE_EDGE));
            }
        }
        for v in self.vertex_to_faces.all_firsts() {
            for f in self.vertex_to_faces.seconds(v) {
                roles.push((f, ids::OFFSET_ROLE_VERTEX));
            }
        }
        // a sheet's side walls are not related to their edges (`build_side_wall_of_edgeuse`
        // records them as closing faces only): each stands on one lamina edge
        for &f in &self.closing_faces {
            roles.push((f, ids::OFFSET_ROLE_EDGE));
        }
        clear_offset_roles(original)?;
        let Some(offset) = self.offset_brep.as_mut() else {
            // no offset yet: no face to mark
            return Ok(());
        };
        clear_offset_roles(offset)?;
        let mut marked: BTreeSet<FaceId> = BTreeSet::new();
        let faces: BTreeSet<FaceId> = offset.face_list().iter().copied().collect();
        for (f, role) in roles {
            if faces.contains(&f) && marked.insert(f) {
                offset.add_attribute(
                    EntityRef::Face(f),
                    Attribute::long(ids::OFFSET_ROLE, role, Behavior::Copy),
                )?;
            }
        }
        Ok(())
    }

    /// Stitches or merges the assembled offset (`PiecewiseMerge`).
    ///
    /// On the non-merge path the B-rep is taken and relations are cleared *before*
    /// stitching, as upstream. A deferred stitch therefore leaves `offset_brep()` empty.
    /// The merge path builds a new merge B-rep and runs `SmMerge::PiecewiseMerge` over
    /// the offset temporary in the caller's `notifications` stores (`None` refuses it);
    /// the temporary stays in `offset_brep()`, as `m_pOffsetBrep` upstream.
    ///
    /// # Errors
    ///
    /// Those of the stitch and of the merge; [`SolidError::InvalidInput`] for a merge
    /// without `notifications` or without their merge identities.
    pub fn piecewise_merge(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        notifications: Option<&mut OffsetNotifications<'_>>,
    ) -> Result<Brep> {
        self.mark_offset_roles(original)?;
        if !self.merge_results {
            self.clear_relations();
            let mut result = present(self.offset_brep.take(), NO_OFFSET)?;
            // the stitch statuses are ignored upstream and passed on here (WR-658). Its edits notify the offset's
            // objects (its glue merges and splits carry their attributes, as upstream's
            // objects' own do) through the caller's stores under the result identities;
            // without them, on call-local records.
            let create_offset_solid = self.create_offset_solid;
            let run = |kernel: &mut Kernel,
                       result: &mut Brep,
                       edits: Option<&mut dyn StitchEditContext>|
             -> Result<()> {
                if create_offset_solid {
                    // Tol: StitchIntoShell() adds in the tols of the topology objects,
                    // so the Brep's tolerance should be plenty.
                    let stitch_tol_3d = result.tolerance();
                    stitch::stitch_into_shell_with(kernel, result, true, stitch_tol_3d, edits)?;
                } else {
                    stitch::stitch_into_solid_with(kernel, result, edits)?;
                }
                Ok(())
            };
            match notifications {
                Some(n) => {
                    let key_of = n.result_key_of;
                    let curve_ref = n.result_curve_ref;
                    let (records, attributes) = (&mut *n.records, &mut *n.attributes);
                    Context::with_notification_context(kernel, |kernel, context| {
                        let mut env = NotifyEnv {
                            records,
                            attributes,
                            context: Some(context),
                            key_of,
                        };
                        let mut edits = BorrowedStitchEdits {
                            env: &mut env,
                            curve_identity: CurveIdentity::Legacy(curve_ref),
                        };
                        run(kernel, &mut result, Some(&mut edits))
                    })??;
                }
                None => run(kernel, &mut result, None)?,
            }

            if bp1::regions(&result, None)?.len() > 1 {
                let old = result.editing_enabled();
                result.set_editing_enabled(false);
                let r = stitch::find_and_set_infinite_region(
                    kernel,
                    &mut result,
                    None,
                    SetRegionVoidType::InfiniteRegionSwap,
                    None,
                );
                result.set_editing_enabled(old);
                r?;
            }
            return Ok(result);
        }

        // Surfaces are face-owned concrete values. Face ids identify the two sides
        // of each surface pair; no geometry is copied to build this heuristic.
        let result = self.required_offset_brep()?;
        let mut no_ssi: Relation<FaceId, FaceId> = Relation::default();
        for faceuse in self.faceuse_to_faces.all_firsts() {
            let face = input_faceuse_face(original, faceuse)?;
            for offset_face in self.faceuse_to_faces.seconds(faceuse) {
                for edge_id in face_edges(original, face)? {
                    let edge_faces = self.edge_to_faces.seconds(edge_id);
                    for &edge_face in &edge_faces {
                        for blended in self.blend_to_faces.seconds(edge_face) {
                            relate_surface_pair(result, &mut no_ssi, blended, edge_face)?;
                        }
                        relate_surface_pair(result, &mut no_ssi, offset_face, edge_face)?;
                        let start = edge::vertex(original, edge_id)?;
                        let end = edge::other_vertex(original, edge_id, start)?;
                        for vertex in [start, end].into_iter().flatten() {
                            if let Some(vertex_face) = self.vertex_to_faces.second(vertex) {
                                relate_surface_pair(result, &mut no_ssi, edge_face, vertex_face)?;
                            }
                        }
                    }
                    if !edge_faces.is_empty() {
                        continue;
                    }
                    if edge::is_manifold(original, edge_id)? {
                        let primary =
                            present(eu::primary_edgeuse(original, edge_id)?, EDGE_NO_EDGEUSE)?;
                        // Not upstream (WR-344, tangency-labs/solidcore#469): see
                        // `relates_mated_offsets`.
                        let unmated = edge::edgeuses(original, edge_id, None, None)?
                            .iter()
                            .any(|eu| self.unmated_edgeuses.contains(eu));
                        // (not upstream: a kept edge's offsets keep their own curves)
                        let kept = self.kept_edges.contains(&edge_id);
                        if kept
                            || relates_mated_offsets(self.extend_convex_edges(), unmated, || {
                                eu2::is_tangent_sector(
                                    kernel,
                                    original,
                                    primary,
                                    self.tangency_tol_degrees(),
                                    false,
                                    true,
                                )
                            })?
                        {
                            for edgeuse in edge::edgeuses(original, edge_id, None, None)? {
                                if eu::faceuse(original, edgeuse)? == Some(faceuse) {
                                    let radial =
                                        present(eu::radial(original, edgeuse)?, EU_NO_RADIAL)?;
                                    let radial_faceuse =
                                        present(eu::faceuse(original, radial)?, EU_NO_FACEUSE)?;
                                    for radial_face in self.faceuse_to_faces.seconds(radial_faceuse)
                                    {
                                        relate_surface_pair(
                                            result,
                                            &mut no_ssi,
                                            offset_face,
                                            radial_face,
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                }
                for vertex in face_vertices(original, face)? {
                    if let Some(vertex_face) = self.vertex_to_faces.second(vertex) {
                        relate_surface_pair(result, &mut no_ssi, offset_face, vertex_face)?;
                    }
                }
            }
        }
        // (not in the fork, solidcore#3329) a wall and the strip carrying it on share a
        // surface
        for &(wall, strip) in &self.wall_extensions {
            relate_surface_pair(result, &mut no_ssi, wall, strip)?;
        }
        self.clear_relations();
        // Shrink the geometry - this helps some for self intersections
        shrink_geometry(kernel, present(self.offset_brep.as_mut(), NO_OFFSET)?)?;
        // Now Get things ready for piecewise merge (cpp:2453–2474)
        let notifications = notifications.ok_or_else(|| {
            SolidError::invalid_input(OP, "a merging PiecewiseMerge needs notification stores")
        })?;
        let offset = present(self.offset_brep.take(), NO_OFFSET)?;
        let (result, offset) = super::merge::merge_offset(
            kernel,
            offset,
            original.tolerance(),
            &no_ssi,
            notifications,
        );
        self.offset_brep = offset;
        result
    }

    /// Builds a face from the supplied offset surface (`BuildOffsetOfFaceuse`).
    /// A null surface is a successful no-op. When the face domain lies inside the
    /// offset surface's natural domain (or for a local operation) the offset face is
    /// made from the face's UV curve copies and takes ownership of the surface
    /// (`offset_surface` becomes `None`); `notifications` keys original-side
    /// entities and `offset_key_of` offset-side ones, and `offset_curve_ref` the curves
    /// of offset edges split by the self-intersection merge. Otherwise the face's UV
    /// domain is intersected with the offset surface's in 2D and the surface (taken
    /// from `offset_surface` once that boolean has run) trims one offset face per
    /// resulting face.
    #[allow(clippy::too_many_arguments)]
    pub fn build_offset_of_faceuse(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        faceuse: Option<FaceuseId>,
        offset_surface: &mut Option<Surface>,
        self_intersections: &SsiData<Curve>,
        notifications: &mut NotifyEnv<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        let faceuse = faceuse.ok_or_else(|| failed(OffsetFailure::FaceuseNotGiven))?;
        let Some(surface) = offset_surface.as_mut() else {
            return Ok(());
        };
        self.required_offset_brep()?;
        // SM_ENSURE_SURFACE_MPNURB: its return value is not checked.
        match surface.kind_mut() {
            SurfaceKind::BSpline(surface) => {
                ignore_status(surface.get_or_create_gw_nurb())?;
            }
            // (IsKindOf(SmBSplineSurface_TYPE): the fork's SmProceduralBlend, its net)
            SurfaceKind::ProceduralBlend(surface) => {
                ignore_status(surface.net_mut().get_or_create_gw_nurb())?;
            }
            SurfaceKind::BallPatch(surface) => {
                ignore_status(surface.net_mut().get_or_create_gw_nurb())?;
            }
            SurfaceKind::Plane(surface) if surface.base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb())?;
            }
            SurfaceKind::SurfOfRevolution(surface) if surface.base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb(kernel))?;
            }
            SurfaceKind::SurfOfExtrusion(surface) if surface.base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb(kernel))?;
            }
            // (SmCone/SmCylinder, SmSphere and SmTorus are SmBSplineSurfaces too)
            SurfaceKind::Cone(surface) if surface.rev().base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb(kernel))?;
            }
            SurfaceKind::Sphere(surface) if surface.base().base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb(kernel))?;
            }
            SurfaceKind::Torus(surface) if surface.base().base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb(kernel))?;
            }
            _ => {}
        }
        let original_faceuse = faceuse;
        let face_id = input_faceuse_face(original, faceuse)?;
        let faceuse = original.faceuse(faceuse)?;
        let orientation = faceuse.orientation;
        // Not upstream (WR-342, tangency-labs/solidcore#974): a face that does not fit the
        // offset surface's domain is clipped to it by a 2D boolean of planar faces made
        // from its UV loops, which need them counterclockwise (the SM_OT_SAME faceuse's
        // sense); upstream takes an inset's SM_OT_OPPOSITE loops, which made that face
        // inside out and the clip empty.
        let fits = original
            .face(face_id)?
            .uv_domain
            .is_contained_by(&surface.natural_uv_domain_with_nurb(kernel)?, EFF_ZERO)
            || self.operation == OffsetOperation::LocalOperation;
        let loop_orientation = if fits { orientation } else { OrientType::Same };
        // The translated-3D-curve plane case (cpp:3560–3600) is `#if 0` upstream.
        let mut curves = face::create_curves_from_face(
            kernel,
            original,
            face_id,
            loop_orientation,
            false,
            true,
        )?;
        let offset_domain = surface.natural_uv_domain_with_nurb(kernel)?;
        let face_domain = original.face(face_id)?.uv_domain;
        if face_domain.is_contained_by(&offset_domain, EFF_ZERO)
            || self.operation == OffsetOperation::LocalOperation
        {
            // (there since the null-surface return above: no failure to report)
            let Some(surface) = offset_surface.take() else {
                return Ok(());
            };
            let new_faces = self.build_contained_offset_faces(
                kernel,
                original,
                face_id,
                orientation,
                curves,
                surface,
                &offset_domain,
                self_intersections,
                notifications,
                offset_key_of,
                offset_curve_ref,
            )?;
            for new_face in new_faces {
                self.relate_faceuse(original_faceuse, new_face);
                self.map_edgeuses(
                    kernel,
                    original,
                    original_faceuse,
                    new_face,
                    notifications,
                    offset_key_of,
                )?;
            }
            return Ok(());
        }

        // cpp:3765–3928: the 2D domain boolean.
        let new_faces = self.build_boolean_offset_faces(
            kernel,
            original,
            face_id,
            orientation,
            &mut curves,
            offset_surface,
            &offset_domain,
            notifications,
            offset_key_of,
        )?;
        for new_face in new_faces {
            self.relate_faceuse(original_faceuse, new_face);
            self.map_edgeuses(
                kernel,
                original,
                original_faceuse,
                new_face,
                notifications,
                offset_key_of,
            )?;
        }
        Ok(())
    }

    /// The 2D-boolean branch of `BuildOffsetOfFaceuse` (cpp:3765–3928), when the face
    /// domain is not inside the offset surface's: the face's UV trim curves (as 3D
    /// curves at `z = 0`) bound a face on a plane, which is intersected
    /// (`SmPrimitiveCreation::Boolean2D`) with the offset surface's UV domain box;
    /// each resulting face's curves, back in 2D, trim an offset face on the surface.
    /// Returns the offset faces in the result's face order. The surface is taken from
    /// `offset_surface` once the domain boolean has run.
    #[allow(clippy::too_many_arguments)]
    fn build_boolean_offset_faces(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        face_id: FaceId,
        orientation: OrientType,
        curves: &mut face::FaceCurves,
        offset_surface: &mut Option<Surface>,
        offset_domain: &Extent2d,
        notifications: &mut NotifyEnv<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
    ) -> Result<Vec<FaceId>> {
        use crate::geom::tol::USE_DEFAULT;
        use crate::primitive::primitive_creation::p1::{Boolean2d, PrimitiveCreation};
        use crate::topology::brep::p2::ArenaNotifyState;
        use crate::topology::brep::p3::new_brep;
        use crate::topology::brep::p8::{create_planar_face_with_3d_curves, FaceBuildEnv};
        use crate::topology::topology_traverser::Marks;

        let arena_face = |face: FaceId| loopuse::notify_ref(EntityRef::Face(face));
        // The domain and planar B-reps and their 2D Boolean are made through the
        // kernel context, whose callbacks a kernel-context caller has lent to
        // `notifications.context` (upstream: the one context).
        let lent = notifications.context.as_deref_mut();
        let result =
            Context::with_lease_returned(kernel, lent, |kernel| -> Result<Option<Brep>> {
                let mut planar_brep = new_brep(kernel, USE_DEFAULT, USE_DEFAULT)?;
                planar_brep.set_editing_enabled(true);
                planar_brep.set_tolerance(1.0e-6);

                // get bounding box containing all pFU->Face->UVTrimCurves
                let mut total_bbox = Extent3d::new();
                if let Some(uv_curves) = curves.uv_curves.as_mut() {
                    for curve in uv_curves {
                        let bbox = uv_curve_bounding_box(kernel, curve)?;
                        total_bbox = total_bbox.union(&bbox);
                    }
                }

                // create a 3d line between every offsetSurface domain corner point
                let min = offset_domain.min();
                let max = offset_domain.max();
                let mut box_curves = Vec::new();
                for (start, end, ivl) in [
                    (
                        min,
                        offset_domain.evaluate(1.0, 0.0),
                        Extent1d::from_raw(min.x, max.x),
                    ),
                    (
                        offset_domain.evaluate(0.0, 1.0),
                        max,
                        Extent1d::from_raw(min.x, max.x),
                    ),
                    (
                        min,
                        offset_domain.evaluate(0.0, 1.0),
                        Extent1d::from_raw(min.y, max.y),
                    ),
                    (
                        offset_domain.evaluate(1.0, 0.0),
                        max,
                        Extent1d::from_raw(min.y, max.y),
                    ),
                ] {
                    let mut line = crate::nurbs::b_spline_curve::BSplineCurve::create_line_segment(
                        3,
                        Vec3::new(start.x, start.y, 0.0),
                        Vec3::new(end.x, end.y, 0.0),
                    )?;
                    line.edit_parameterization(&ivl)?;
                    box_curves.push(Curve::from(line));
                }

                // make a new Brep containing 1 planar face bounded by the 3d Curves just made
                let mut domain_brep = new_brep(kernel, USE_DEFAULT, USE_DEFAULT)?;
                domain_brep.set_tolerance(planar_brep.tolerance());
                domain_brep.set_editing_enabled(true);
                {
                    let mut state = ArenaNotifyState::new(&domain_brep);
                    let mut env = state.env(None);
                    let mut marks = Marks::default();
                    let mut fenv = FaceBuildEnv {
                        env: &mut env,
                        marks: &mut marks,
                        this_face: &arena_face,
                    };
                    let region = domain_brep.infinite_region();
                    let tolerance = domain_brep.tolerance();
                    create_planar_face_with_3d_curves(
                        kernel,
                        &mut domain_brep,
                        region,
                        &box_curves,
                        tolerance,
                        &mut fenv,
                    )?;
                }

                // copy the plane just made and get its natural domain
                let mut copy_plane = {
                    // GetSurfaces: the surfaces their faces own, under the faces' identities.
                    let owners = bp1::surface_owners(&domain_brep, None, &arena_face)?;
                    let first_face = present(
                        owners.first().copied(),
                        "the domain B-rep has no face, though the face bounded by the four \
                         sides of the offset surface's domain box was just made in it",
                    )?;
                    let first = domain_brep.face(first_face)?.surface.as_ref();
                    match first {
                        Some(first) if matches!(first.kind(), SurfaceKind::Plane(_)) => {
                            first.copy(kernel)?
                        }
                        // SM_CAST_PTR yields NULL, dereferenced next upstream.
                        _ => return Err(defect(DOMAIN_FACE_NOT_PLANAR, SM_ERR_NULL_POINTER)),
                    }
                };
                let mut copy_domain = copy_plane.natural_uv_domain_with_nurb(kernel)?;
                let SurfaceKind::Plane(plane) = copy_plane.kind_mut() else {
                    return Err(defect(
                        "the copy of a plane is not a plane, though a copy keeps its surface's \
                         kind",
                        SM_ERR_NULL_POINTER,
                    ));
                };

                // increase the copy domain to include the projection of every UV trim curve
                // bounding box corner onto this plane.
                for corner in [
                    total_bbox.min(),
                    total_bbox.max(),
                    total_bbox.evaluate(1.0, 0.0, 0.0),
                    total_bbox.evaluate(0.0, 1.0, 0.0),
                ] {
                    copy_domain.add_point(plane.project_point_to_uv_domain(corner));
                }
                plane.adjust_step_uv_domain(&copy_domain)?;
                let copy_plane_domain = copy_plane.natural_uv_domain_with_nurb(kernel)?;

                // make a planar face using the offset UV trim curves (as 3D curves)
                let mut loops = Vec::new();
                for &count in &curves.curve_loops {
                    loops.push(
                        usize::try_from(count).map_err(|_| {
                            SolidError::invalid_input(OP, "loop count exceeds usize")
                        })?,
                    );
                }
                let input = FaceCurvesInput {
                    loops,
                    curves_3d: curves
                        .uv_curves
                        .take()
                        .map(|list| list.into_iter().map(Some).collect()),
                    uv_curves: None,
                    orientations: std::mem::take(&mut curves.curve_orients),
                    loop_points: std::mem::take(&mut curves.loop_points),
                };
                {
                    let mut state = ArenaNotifyState::new(&planar_brep);
                    let mut env = state.env(None);
                    let region = planar_brep.infinite_region();
                    bp7::make_face_with_curves(
                        kernel,
                        &mut planar_brep,
                        region,
                        input,
                        copy_plane,
                        &copy_plane_domain,
                        OrientType::Same,
                        None,
                        &arena_face,
                        &mut env,
                    )?;
                }

                // Intersect in 2d the domain B-rep with the planar B-rep.
                let mut result = None;
                {
                    let mut primitive = PrimitiveCreation::new(None, 0, false);
                    let mut domain_marks = Marks::default();
                    let mut planar_marks = Marks::default();
                    let mut state = ArenaNotifyState::new(&domain_brep);
                    state.seed(&planar_brep);
                    primitive.boolean_2d(
                        kernel,
                        &mut domain_brep,
                        &mut planar_brep,
                        Boolean2d::Intersection,
                        &mut result,
                        (&mut domain_marks, &mut planar_marks),
                        &mut state.env(None),
                    )?;
                }
                Ok(result)
            })?;
        // Upstream dereferences the result next, even when Boolean2D left it null.
        let mut result = present(
            result,
            "the 2D boolean of the face's domain with the offset surface's returned no B-rep, \
             though it returns one whenever both B-reps have a face, and each was just given one",
        )?;

        // The result's faces represent the UV domain of the part of the original face
        // that corresponds to the offset face. Their edge curves are UV trim curves,
        // mapped into the offset B-rep with the offset surface.
        let mut new_faces = Vec::new();
        // (there since `build_offset_of_faceuse` returned on a null surface: no failure to
        // report)
        let Some(surface) = offset_surface.take() else {
            return Ok(new_faces);
        };
        let result_faces = bp1::faces(&result, None)?;
        for (index, &result_face) in result_faces.iter().enumerate() {
            // (not upstream, WR-342: the loops in the sense of the faceuse's orientation,
            // which the offset face is made with; upstream takes SM_OT_SAME)
            let made = face::create_curves_from_face(
                kernel,
                &mut result,
                result_face,
                orientation,
                true,
                false,
            )?;
            let mut uv_curves = made.curves_3d.unwrap_or_default();
            // Convert face 3dCurves into 2D curves (all their Z components are zero).
            for curve in &mut uv_curves {
                curve.convert_to_2d(kernel)?;
            }
            let mut loops = Vec::new();
            for &count in &made.curve_loops {
                loops.push(
                    usize::try_from(count)
                        .map_err(|_| SolidError::invalid_input(OP, "loop count exceeds usize"))?,
                );
            }
            let input = FaceCurvesInput {
                loops,
                curves_3d: None,
                uv_curves: Some(uv_curves),
                orientations: made.curve_orients,
                loop_points: made.loop_points,
            };
            // Upstream every face shares the one offset surface; here each face but
            // the first owns an exact copy of it.
            let face_surface = if index == 0 {
                surface.clone()
            } else {
                surface.copy(kernel)?
            };
            let this_face = |face| offset_key_of(EntityRef::Face(face));
            let brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
            let region = brep.infinite_region();
            let made = bp7::make_face_with_curves(
                kernel,
                brep,
                region,
                input,
                face_surface,
                offset_domain,
                orientation,
                None,
                &this_face,
                &mut offset_env(notifications, offset_key_of),
            )?;
            let offset_face = present(made.face, NO_NEW_FACE)?;
            new_faces.push(offset_face);
            // SM_NO_GET_BREP is NULL in the pinned reference; the status is not checked.
            ignore_status(face3::notify(
                original,
                face_id,
                notifications,
                None,
                NotifyOperation::Copy,
                [
                    Some(offset_key_of(EntityRef::Face(offset_face))),
                    None,
                    None,
                ],
            ))?;
        }
        Ok(new_faces)
    }

    /// The contained-domain branch of `BuildOffsetOfFaceuse` (cpp:3613–3762): the
    /// offset faces to relate to the face use, empty when the face constructor fails
    /// (upstream ignores that status).
    #[allow(clippy::too_many_arguments)]
    fn build_contained_offset_faces(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        face_id: FaceId,
        orientation: OrientType,
        curves: face::FaceCurves,
        surface: Surface,
        offset_domain: &Extent2d,
        self_intersections: &SsiData<Curve>,
        notifications: &mut NotifyEnv<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<Vec<FaceId>> {
        let mut loops = Vec::new();
        for &count in &curves.curve_loops {
            loops.push(
                usize::try_from(count)
                    .map_err(|_| SolidError::invalid_input(OP, "loop count exceeds usize"))?,
            );
        }
        // Not upstream: a moved face of a local operation keeps its loops' topology, one
        // edge per original edge use, each mapped to it and rebuilt where the new surfaces
        // meet. A UV gap the copy fills (a pole: two edge uses ending at one vertex at two
        // parameters) is no edge: on the face's own surface its line lifts to that vertex
        // and the face constructor drops it, but on the new surface it lifts to a curve.
        // The constructor is told so: the gap's line closes the UV loop and stands for the
        // vertex, made no edge, and its neighbours meet at one vertex there, as the
        // original's edge uses do.
        let vertex_slots = if self.operation == OffsetOperation::LocalOperation
            && self.moved_faces.contains(&face_id)
        {
            curves.filled_gaps.clone()
        } else {
            Vec::new()
        };
        // Not upstream: a face of a local operation that keeps its surface is the copy of
        // its own surface and loops, so its edge uses keep the UV curves copied with them as
        // their trim curves. Dropped again from the lifted edges, a trim curve ending at a
        // pole can end there at another parameter (every one lifts to the pole) and then
        // runs elsewhere through the face, so `map_edgeuses` finds no original edge use
        // for it and a kept edge loses its offset (#17598).
        let keeps_uv_curves = self.operation == OffsetOperation::LocalOperation
            && self.local_faces.contains(&face_id)
            && !self.moved_faces.contains(&face_id);
        let input = FaceCurvesInput {
            loops,
            curves_3d: None,
            uv_curves: curves.uv_curves,
            orientations: curves.curve_orients,
            loop_points: curves.loop_points,
        };
        let this_face = |face| offset_key_of(EntityRef::Face(face));
        let brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
        let region = brep.infinite_region();
        // Only success continues; a failed constructor leaves no face to relate (upstream
        // ignores its status), but a defect is passed on, never read as no face.
        // The surface moves into the face constructor and is dropped on failure.
        let mut owners = Vec::new();
        let made = match bp7::make_face_with_curve_owners_at_vertices(
            kernel,
            brep,
            region,
            input,
            surface,
            offset_domain,
            orientation,
            None,
            &this_face,
            &vertex_slots,
            keeps_uv_curves,
            &mut owners,
            &mut offset_env(notifications, offset_key_of),
        ) {
            Ok(made) => made,
            Err(error) if error.is_unported_or_cancelled() || error.is_defect() => {
                return Err(error)
            }
            Err(_) => return Ok(Vec::new()),
        };
        let new_face = present(made.face, NO_NEW_FACE)?;
        // SM_NO_GET_BREP is NULL in the pinned reference; the status is not checked.
        ignore_status(face3::notify(
            original,
            face_id,
            notifications,
            None,
            NotifyOperation::Copy,
            [Some(offset_key_of(EntityRef::Face(new_face))), None, None],
        ))?;

        let mut new_edges = Vec::new();
        if self.merge_results && !self_intersections.curves_3d.is_empty() {
            // Upstream dereferences each UV curve unchecked; the intersection can leave an
            // entry null, which the merge cannot cut along.
            let mut merged = Vec::new();
            for (index, curve_3d) in self_intersections.curves_3d.iter().enumerate() {
                let uv_curve = |list: &[Option<Curve>]| -> Result<Curve> {
                    list.get(index)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or_else(|| {
                            failed(OffsetFailure::SelfIntersectionWithoutUvCurve { face: face_id })
                        })
                };
                merged.push(SelfIntersectionCurve {
                    curve_3d: curve_3d.clone(),
                    uv_curve_1: uv_curve(&self_intersections.uv_curves1)?,
                    uv_curve_2: uv_curve(&self_intersections.uv_curves2)?,
                });
            }
            let tolerance = brep.tolerance();
            new_edges = bp7::merge_self_intersection_curves(
                kernel,
                brep,
                Some(new_face),
                tolerance,
                &merged,
                &mut offset_env(notifications, offset_key_of),
                offset_curve_ref,
            )?;
        }
        let mut new_faces = vec![new_face];
        for &edge in &new_edges {
            for face in edge::faces(brep, edge, None)? {
                if !new_faces.contains(&face) {
                    new_faces.push(face);
                }
            }
        }

        let mut delete_faces = Vec::new();
        for &face in &new_faces {
            if self.extend_convex_edges {
                continue;
            }
            for edge in face_edges(brep, face)? {
                if !eu::edge_is_lamina(brep, edge)? {
                    continue;
                }
                let edgeuse = present(
                    edge::same_oriented_edgeuse(brep, edge)?,
                    EDGE_NO_SAME_EDGEUSE,
                )?;
                let point =
                    eu3::normalized_evaluate(kernel, brep, edgeuse, 0.4567, false, false, true)?
                        .point;
                let offset_distance = self.offset_distance;
                let test_distance = offset_distance * 1.1;
                let original_face = original.face(face_id)?;
                let uv_domain = original_face.uv_domain;
                let original_surface = original_face
                    .surface
                    .as_ref()
                    .ok_or_else(|| failed(OffsetFailure::FaceHasNoSurface { face: face_id }))?;
                // Upstream turns the surface cache's point test off here so the solve
                // keeps all solutions without classifying them against the trim
                // boundaries (`SmOffsetExecutive.cpp:3730-3734`).
                let solutions = original_surface.global_point_solve_point_test_off(
                    kernel,
                    Some(NoObject),
                    &uv_domain,
                    SolverOperation::Minimize,
                    point,
                    face::tolerance(brep, face)?,
                    Some(test_distance),
                    SolutionRequested::Single,
                )?;
                let offset_tol = self.approximation_tol_3d;
                let Some(solution) = solutions.get(0) else {
                    continue;
                };
                if solution.start.solution_value < offset_distance - 2.0 * offset_tol {
                    if !delete_faces.contains(&face) {
                        delete_faces.push(face);
                    }
                    break;
                }
            }
        }
        if !delete_faces.is_empty() {
            // cpp:3749-3759: SmTopologyIntersector sTI(m_crContext, m_pOffsetBrep,
            // m_pOffsetBrep, SM_EFF_ZERO, SM_EFF_ZERO); sTI.DeleteFaces(sDelFaces)
            // (pOptKeepAsWires = NULL, status ignored); then FindElement / RemoveAt
            // on sNewFaces. The intersector over the one offset B-rep is merge p4's
            // arena adapter; its notifications are keyed under
            // `DELETE_FACES_NAMESPACE`, not `offset_key_of`.
            // The deletion edits the offset B-rep through the kernel context, whose
            // callbacks a kernel-context caller has lent to `notifications.context`
            // (upstream: the one context); without returning them the deletion
            // fails as a nested edit. Upstream ignores its status; it is passed on
            // here (WR-658).
            let lent = notifications.context.as_deref_mut();
            Context::with_lease_returned(kernel, lent, |kernel| {
                KernelManifoldIntersector::delete_faces_in(
                    kernel,
                    brep,
                    &delete_faces,
                    &mut *notifications.records,
                    &mut *notifications.attributes,
                    DELETE_FACES_NAMESPACE,
                )
            })?;
            new_faces.retain(|face| !delete_faces.contains(face));
        }
        Ok(new_faces)
    }

    /// Matches original and offset edge uses by their UV midpoints (`MapEdgeuses`):
    /// the offset face side whose normal agrees with the original face use is
    /// searched at tolerances scaled 1, 10, 100 and 1000, falling back to an
    /// intersecting point drop above 10. Each match is related, its edges and vertices
    /// are notified (`notifications` keys original-side entities, `offset_key_of`
    /// offset-side ones) and the original use is consumed.
    pub fn map_edgeuses(
        &mut self,
        kernel: &Kernel,
        original: &mut Brep,
        original_faceuse: FaceuseId,
        offset_face: FaceId,
        notifications: &mut NotifyEnv<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
    ) -> Result<()> {
        let mut state = NotifyState::from_env(notifications);
        self.map_edgeuses_with_state(
            MapKernel::Read(kernel),
            original,
            original_faceuse,
            offset_face,
            &mut state,
            offset_key_of,
        )
    }

    /// Map corresponding edge uses and notify through the live kernel context.
    /// Geometry is evaluated with the kernel borrowed immutably; its context is
    /// borrowed mutably only for each notification callback.
    pub fn map_edgeuses_with_kernel_context(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        original_faceuse: FaceuseId,
        offset_face: FaceId,
        notifications: &mut NotificationStores<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
    ) -> Result<()> {
        let mut state = NotifyState::from_kernel_stores(notifications);
        self.map_edgeuses_with_state(
            MapKernel::Mutable(kernel),
            original,
            original_faceuse,
            offset_face,
            &mut state,
            offset_key_of,
        )
    }

    fn map_edgeuses_with_state(
        &mut self,
        mut kernel: MapKernel<'_>,
        original: &mut Brep,
        original_faceuse: FaceuseId,
        offset_face: FaceId,
        notifications: &mut NotifyState<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
    ) -> Result<()> {
        let original_use = original.faceuse(original_faceuse)?;
        let original_orientation = original_use.orientation;
        let original_face_id = input_faceuse_face(original, original_faceuse)?;
        let original_face = original.face(original_face_id)?;
        let brep = self.required_offset_brep()?;
        let offset_face = brep.face(offset_face)?;
        // Upstream ignores Intersect's status and initializes its output on failure;
        // it is passed on here (WR-658).
        let domain = original_face.uv_domain.intersect(&offset_face.uv_domain)?;
        let mid_uv = domain.evaluate(0.5, 0.5);
        // (`EvaluateNormal(sMidUV, TRUE, TRUE, ..)`: from the left)
        let normal = original_face
            .surface
            .as_ref()
            .ok_or_else(|| {
                failed(OffsetFailure::FaceHasNoSurface {
                    face: original_face_id,
                })
            })?
            .evaluate_normal(kernel.kernel(), mid_uv, Side::Left, Side::Left)?;
        let offset_normal = present(offset_face.surface.as_ref(), OFFSET_FACE_NO_SURFACE)?
            .evaluate_normal(kernel.kernel(), mid_uv, Side::Left, Side::Left)?;
        let same_normals = normal.dot(offset_normal) > 0.0;
        let mut original_uses: Vec<Option<EdgeuseId>> =
            eu::faceuse_edgeuses(original, original_faceuse, false, None)?
                .into_iter()
                .map(Some)
                .collect();
        let upward = present(offset_face.faceuse, OFFSET_FACE_NO_FACEUSE)?;
        let offset_use = if (same_normals && original_orientation == OrientType::Same)
            || (!same_normals && original_orientation == OrientType::Opposite)
        {
            upward
        } else {
            present(brep.faceuse(upward)?.mate, FU_NO_MATE)?
        };
        for offset in eu::faceuse_edgeuses(brep, offset_use, false, None)? {
            // Only 0.5 is used: matched uses may run in opposite directions.
            let brep = present(self.offset_brep.as_mut(), NO_OFFSET)?;
            let offset_point =
                eu3::normalized_evaluate(kernel.kernel(), brep, offset, 0.5, true, false, true)?
                    .point;
            let mut found = false;
            let mut scale = 1.0;
            while !found && scale < 10000.0 {
                for slot in 0..original_uses.len() {
                    let Some(original_use) = original_uses.get(slot).copied().flatten() else {
                        continue;
                    };
                    let original_point = eu3::normalized_evaluate(
                        kernel.kernel(),
                        original,
                        original_use,
                        0.5,
                        true,
                        false,
                        true,
                    )?
                    .point;
                    let distance = original_point.distance(offset_point);
                    let uv_tol = EFF_ZERO_SQRT * (1.0 + offset_point.max_dimension());
                    found = distance < uv_tol * scale;
                    // Curves may have been split: drop onto the original UV curve.
                    if !found && scale > 10.0 {
                        let uv_curve =
                            eu::uv_trim_curve(original, original_use)?
                                .0
                                .ok_or_else(|| {
                                    failed(OffsetFailure::EdgeuseHasNoUvTrim {
                                        edge: eu::edge(original, original_use).ok().flatten(),
                                    })
                                })?;
                        found = uv_curve
                            .drop_point(
                                kernel.kernel(),
                                &uv_curve.natural_interval(),
                                offset_point,
                                None,
                                uv_tol * scale,
                                None,
                                SolverOperation::Intersect,
                            )?
                            .success;
                    }
                    if found {
                        self.relate_edgeuses(original_use, offset);
                        let result = self.required_offset_brep()?;
                        let original_edge = edge_of(original, original_use)?;
                        let offset_edge = edge_of(result, offset)?;
                        kernel.with_env(notifications, |env| {
                            notify_edge_and_vertices(
                                original,
                                original_edge,
                                result,
                                offset_edge,
                                env,
                                offset_key_of,
                            )
                        })?;
                        // Not upstream (WR-671, tangency-labs/solidcore#2111): the offset
                        // edge and its vertices, made at the B-rep's zone tolerance, keep
                        // the matched original edge's tolerance and its vertices' own.
                        keep_source_tolerances(
                            kernel.kernel(),
                            original,
                            original_use,
                            original_edge,
                            present(self.offset_brep.as_mut(), NO_OFFSET)?,
                            offset,
                            offset_edge,
                            // Ends pair at the base tolerance: `scale` only finds a
                            // piece's middle, never moves an end (#18010).
                            uv_tol,
                        )?;
                        if let Some(slot) = original_uses.get_mut(slot) {
                            *slot = None;
                        }
                        break;
                    }
                }
                scale *= 10.0;
            }
        }
        Ok(())
    }

    /// Builds a face while binding source notifications to the supplied kernel context.
    /// Source and newly-created result entities retain their separate key namespaces.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_offset_of_faceuse_with_kernel_context(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        faceuse: FaceuseId,
        offset_surface: &mut Option<Surface>,
        self_intersections: &SsiData<Curve>,
        notifications: &mut NotificationStores<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        Context::with_notification_context(kernel, |kernel, context| {
            self.build_offset_of_faceuse(
                kernel,
                original,
                Some(faceuse),
                offset_surface,
                self_intersections,
                &mut NotifyEnv {
                    records: &mut *notifications.records,
                    attributes: &mut *notifications.attributes,
                    context: Some(context),
                    key_of: notifications.key_of,
                },
                offset_key_of,
                offset_curve_ref,
            )
        })?
    }
}

/// (Not upstream, WR-671.) Raises `offset_edge` of `result` to the tolerance of the
/// original edge it was matched to, and each of its vertices to the tolerance of the
/// original vertex it stands for. `original_use` and `offset_use` are the edge uses
/// `MapEdgeuses` matched in UV (their midpoints, or the offset use's midpoint dropped onto
/// the original's UV curve where curves were split, within `uv_tol` times a scale of 1 to
/// 1000), so they may run either way, and the offset use may be a piece of the original.
/// `uv_tol` is the unscaled base, `MapEdgeuses`'s tolerance for one UV point standing for
/// another (`EFF_ZERO_SQRT * (1 + |uv|)`): the scale makes room for a piece's middle lying
/// away from the original's, and an end that stands for an original end lies on it.
///
/// The edge is raised whatever its ends do: it lies along the original's image. An end
/// of an edge use is its UV point at normalized 0 (its vertex use's vertex) or 1 (its
/// mate's). An offset face is trimmed by its original's UV curves on a surface that
/// shares the original's parameters (an offset surface; a local operation's new surface,
/// whatever its own frame, takes the face's UV curves too), so an offset end that stands
/// for an original end lies on it in UV, whichever way the uses run and however the new
/// surface carries them in space. Each offset end takes the tolerance of each original
/// end within `uv_tol` of it. An end near none is a vertex the original does not have (a
/// piece's split end) and takes nothing; pairing at the scaled tolerance gave a split end
/// within it of an original end that end's tolerance (#18010). An end
/// near both (one vertex closing the edge, or an edge shorter than `uv_tol`) takes the
/// larger. An end takes only its own vertex's tolerance otherwise: taking the larger of
/// the two ends' grew a thin vertex to its edge's other end's tolerance, one edge further
/// at each operation, until it reached past a short neighbouring edge and the merge
/// joined that edge's two ends. A NaN distance is near nothing. Tolerances only grow
/// (`SetTolerance(.., bUpdateOnlyIfLarger = TRUE)`).
///
/// # Errors
///
/// [`SolidError::PortInvariant`] for a matched edge use without a mate or vertex; those
/// of the evaluations.
fn keep_source_tolerances(
    k: &Kernel,
    original: &Brep,
    original_use: EdgeuseId,
    original_edge: EdgeId,
    result: &mut Brep,
    offset_use: EdgeuseId,
    offset_edge: EdgeId,
    uv_tol: f64,
) -> Result<()> {
    const OP: &str = "offset::offset_executive::keep_source_tolerances";
    let edge_tolerance = edge::tolerance(original, original_edge)?;
    edge::set_tolerance(result, offset_edge, edge_tolerance, true, true)?;
    let mates = |b: &Brep, u: EdgeuseId| -> Result<[VertexId; 2]> {
        let mate = eu::mate(b, u)?
            .ok_or_else(|| SolidError::port_invariant(OP, "a matched edge use without a mate"))?;
        Ok([use_vertex(b, u)?, use_vertex(b, mate)?])
    };
    let source = mates(original, original_use)?;
    let made = mates(result, offset_use)?;
    let uv_ends = |b: &Brep, u: EdgeuseId| -> Result<[Vec3; 2]> {
        let at = |t: f64| -> Result<Vec3> {
            Ok(eu3::normalized_evaluate_read_only(k, b, u, t, true, false)?.point)
        };
        Ok([at(0.0)?, at(1.0)?])
    };
    let [source_start, source_end] = source;
    let [source_start_uv, source_end_uv] = uv_ends(original, original_use)?;
    let source_ends = [
        (source_start_uv, vertex::tolerance(original, source_start)?),
        (source_end_uv, vertex::tolerance(original, source_end)?),
    ];
    for (v, at) in made.into_iter().zip(uv_ends(result, offset_use)?) {
        for (end, tolerance) in source_ends {
            if end.distance(at) < uv_tol {
                vertex::set_tolerance(result, v, tolerance, true, true)?;
            }
        }
    }
    Ok(())
}

/// The vertex an edge use starts at (`GetVertexuse()->GetVertex()`).
fn use_vertex(b: &Brep, u: EdgeuseId) -> Result<VertexId> {
    let vu = eu::vertexuse(b, u)?.ok_or_else(|| {
        SolidError::port_invariant(
            "offset::offset_executive::keep_source_tolerances",
            "an edge use without a vertex use",
        )
    })?;
    crate::topology::vertexuse::vertex(b, vu)?.ok_or_else(|| {
        SolidError::port_invariant(
            "offset::offset_executive::keep_source_tolerances",
            "a vertex use without a vertex",
        )
    })
}

/// The notification environment of offset-side entities: the caller's records,
/// attributes and context under the offset keys.
fn offset_env<'a>(
    notifications: &'a mut NotifyEnv<'_>,
    offset_key_of: &'a dyn Fn(EntityRef) -> ObjectRef,
) -> NotifyEnv<'a> {
    NotifyEnv {
        records: &mut *notifications.records,
        attributes: &mut *notifications.attributes,
        context: notifications.context.as_deref_mut(),
        key_of: offset_key_of,
    }
}

/// Whether `PiecewiseMerge` lists the offsets of the two faces at a manifold original
/// edge with no offset cap face among the pairs that already share their intersection
/// edge (`sNoSSI`), so the merge does not intersect them. `tangent_sector` is
/// `IsTangentSector` of the edge's primary edge use, evaluated only when needed.
///
/// Upstream relates a tangent sector only. Not upstream (WR-344, tangency-labs/solidcore#469;
/// the fork's tangency-labs/smlib@85bbfaf): with `extend_convex` (extended convex edges)
/// every such edge had its faces' offsets mated along it (`EdgeuseOffset`), so it is
/// related without the tangency test, as the fork's `||` does. An edge whose mate failed
/// in `EdgeuseOffset` (`unmated`; #1795: the offsets stop meeting round a closed edge) is
/// related in neither case, tangent sector or not, and its faces' offsets are intersected
/// as any other pair's (the fork's tangency-labs/smlib@9d8ace9:
/// `!m_sUnmatedEdges.IsIn(pE) && (m_bExtendConvexEdges || IsTangentSector(..))`).
fn relates_mated_offsets(
    extend_convex: bool,
    unmated: bool,
    tangent_sector: impl FnOnce() -> Result<bool>,
) -> Result<bool> {
    if unmated {
        return Ok(false);
    }
    if extend_convex {
        return Ok(true);
    }
    tangent_sector()
}

fn relate_surface_pair(
    result: &Brep,
    relation: &mut Relation<FaceId, FaceId>,
    a: FaceId,
    b: FaceId,
) -> Result<()> {
    // Upstream `SmOffsetExecutive::PiecewiseMerge` (cpp:2313–2412) does not null-check
    // `GetSurface()`, and `SmRelation::RelatePair` (SmRelation.h:203–206, unchecked) is
    // a no-op when either pointer is NULL — including faces already destroyed by
    // `RemoveExcessFaces` whose `~SmFace` cleared `m_pSurface` (UB-256).
    let has_surface = |id: FaceId| result.faces().get(id).is_some_and(|f| f.surface.is_some());
    if !has_surface(a) || !has_surface(b) {
        return Ok(());
    }
    relation.relate_pair(a, b)?;
    relation.relate_pair(b, a)
}

fn uv_curve_bounding_box(kernel: &Kernel, curve: &mut Curve) -> Result<Extent3d> {
    // CreateCurvesFromFace supplied B-spline slicing copies or lines.
    match curve.kind_mut() {
        CurveKind::BSpline(curve) => curve.convert_to_3d()?,
        CurveKind::Line(curve) => curve.convert_to_3d()?,
        // UV curves from CreateCurvesFromFace are slicing copies, never analytic
        // non-line kinds. Reject malformed values rather than silently changing them.
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "UV copy is not a B-spline or line",
            ));
        }
    }
    // cpp:3777–3780 ignores both statuses and keeps the initialized extent; they are
    // passed on here (WR-658, tangency-labs/smlib 4803d6d).
    Ok(curve
        .calculate_bounding_box(
            kernel,
            &curve.natural_interval(),
            crate::nurbs::b_spline_curve::BoundingBoxRequest {
                normal: true,
                pseudo: false,
                polar: false,
            },
            true,
        )?
        .normal
        .unwrap_or_else(Extent3d::new))
}

/// A defect of p1 ([`OffsetFailure::Defect`]): `why` says which step failed and why that
/// is the kernel's fault, with `status`, the status of the site it replaced
/// (`SM_ERR_NULL_POINTER` for an upstream `NER`, else `SM_ERR`).
#[track_caller]
fn defect(why: &'static str, status: i32) -> SolidError {
    Failure::new(OP, OffsetFailure::Defect { why })
        .with_status(status)
        .into_error()
}

/// The failure of p1 for `cause`, with the cause's status (that of the site it replaced).
#[track_caller]
fn failed(cause: OffsetFailure) -> SolidError {
    Failure::new(OP, cause).into_error()
}

/// The face of `faceuse`, a face use of the operation's input: the caller's body as
/// supplied (a caller can build a face use with no face), so a missing face is
/// [`OffsetFailure::FaceuseHasNoFace`], not a defect.
fn input_faceuse_face(original: &Brep, faceuse: FaceuseId) -> Result<FaceId> {
    original
        .faceuse(faceuse)?
        .face
        .ok_or_else(|| failed(OffsetFailure::FaceuseHasNoFace { faceuse }))
}

/// `value`, or the defect `why` with `SM_ERR_NULL_POINTER` ([`defect`]): a link or entity
/// of a B-rep, or of the executive's own data, that is not there.
#[track_caller]
fn present<T>(value: Option<T>, why: &'static str) -> Result<T> {
    match value {
        Some(v) => Ok(v),
        None => Err(defect(why, SM_ERR_NULL_POINTER)),
    }
}

/// The value of a call whose failure the executive reads as a step that did not apply
/// ([`crate::error::ignored`]: upstream ignores its status). A defect (the kernel's fault,
/// [`SolidError::is_defect`]) is passed on with its reason, never read as a step that did
/// not apply (`solidcore/docs/failure-codes-and-reproduction.md` §6, rule 5).
pub(super) fn ignored_unless_defect<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Err(e) if e.is_defect() => Err(e),
        r => crate::error::ignored(r),
    }
}

// Why each missing thing below is the kernel's fault ([`OffsetFailure::Defect`]). The
// B-reps are the operation's input and the offset B-rep the executive builds; the kernel
// makes both with every link.

/// Why the offset B-rep is not there.
const NO_OFFSET: &str = "the offset B-rep is not there, though the executive's caller sets it \
                         before any step that builds into it, and every step that takes it \
                         gives it back";
/// Why an edge use has no edge.
const EU_NO_EDGE: &str =
    "an edge use has no edge, though the kernel links every edge use to its edge";
/// Why an edge use has no vertex use.
const EU_NO_VERTEXUSE: &str =
    "an edge use has no vertex use, though the kernel starts every edge use at a vertex use";
/// Why a vertex use has no vertex.
const VU_NO_VERTEX: &str =
    "a vertex use has no vertex, though the kernel links every vertex use to its vertex";
/// Why an edge use has no loop use.
const EU_NO_LOOPUSE: &str = "an edge use of a face has no loop use, though the kernel makes \
                             every edge use of a face in a loop use";
/// Why an edge use has no neighbour in its loop.
const EU_NO_NEIGHBOUR: &str = "an edge use has no neighbour in its loop, though the kernel keeps \
                               the edge uses of a loop in a ring";
/// Why an edge use has no mate.
const EU_NO_MATE: &str =
    "an edge use has no mate, though the kernel makes edge uses in mated pairs";
/// Why an edge use has no radial.
const EU_NO_RADIAL: &str = "an edge use has no radial, though the kernel links the uses around \
                            every edge in a ring";
/// Why an edge use has no face use.
const EU_NO_FACEUSE: &str = "an edge use of a face has no face use, though the kernel makes \
                             every edge use of a face in a loop of its face use";
/// Why an edge has no edge use.
const EDGE_NO_EDGEUSE: &str =
    "an edge has no edge use, though the kernel makes every edge with its uses";
/// Why an edge has no edge use running with its curve.
const EDGE_NO_SAME_EDGEUSE: &str = "an edge has no edge use running with its curve, though the \
                                    kernel makes an edge's uses in mated pairs, one each way";
/// Why an edge has no vertex at an end.
const EDGE_NO_VERTEX: &str =
    "an edge has no vertex at an end, though the kernel bounds every edge by its vertices";
/// Why a vertex of a loop use is missing.
const LOOP_NO_VERTEX: &str = "a loop use has an edge use with no vertex, though the kernel \
                              starts every edge use of a loop at a vertex";
/// Why a face use has no mate.
const FU_NO_MATE: &str = "a face use has no mate, though the kernel makes face uses in mated pairs";
/// Why an offset face has no surface.
const OFFSET_FACE_NO_SURFACE: &str =
    "an offset face has no surface, though the offset gives every face it builds one";
/// Why an offset face has no face use.
const OFFSET_FACE_NO_FACEUSE: &str =
    "an offset face has no face use, though the kernel makes every face with its face uses";
/// Why an offset edge has no curve.
const OFFSET_EDGE_NO_CURVE: &str =
    "an offset edge has no curve, though the offset gives every edge it builds one";
/// Why the face made from the offset surface's domain box has no plane.
const DOMAIN_FACE_NOT_PLANAR: &str = "the face made from the four sides of the offset surface's \
                                      domain box has no plane, though the planar face \
                                      construction makes its face on the plane through its \
                                      curves";
/// Why a face just built is not returned.
const NO_NEW_FACE: &str = "the face construction succeeded but returned no face, though a face \
                           construction that succeeds returns the face it made";

/// Why the trimmed B-spline curve of the input's `edge` is missing
/// ([`edge::create_trimmed_nurbs_curve`] gave none, the NULL curve upstream hands to
/// `CreatePipeSweep`), with `SM_ERR_NULL_POINTER`: the edge has no curve (the caller's
/// input), its curve is of no B-spline curve kind (a kind the pipe cannot follow), or the
/// trim of its curve to its interval failed (a defect).
fn no_trimmed_curve(original: &Brep, edge: EdgeId) -> SolidError {
    let curve = match original.edge(edge) {
        Ok(record) => record.curve.as_ref(),
        Err(e) => return e,
    };
    match curve {
        None => failed(OffsetFailure::EdgeHasNoCurve { edge: Some(edge) }),
        Some(c)
            if !matches!(
                c.kind(),
                CurveKind::BSpline(_)
                    | CurveKind::Line(_)
                    | CurveKind::Ellipse(_)
                    | CurveKind::Hyperbola(_)
                    | CurveKind::Parabola(_)
            ) =>
        {
            failed(OffsetFailure::CurveKindUnsupported {
                edge: Some(edge),
                curve_type: c.type_code(),
            })
        }
        Some(_) => defect(
            "the trim of an edge's B-spline curve to the edge's interval failed, though an \
             edge's interval lies on its curve",
            SM_ERR_NULL_POINTER,
        ),
    }
}

/// `sOtherEUS[ii]`: the other side's offset edge use paired with this side's `index`th of
/// original edge `edge`, read past the list's end upstream. A split that the other side's
/// break asks for is refused near a piece's end, so this side can keep more pieces than the
/// other ([`insert_breaks`], [`extract_gaps`]): [`OffsetFailure::EdgeOffsetsUnpaired`].
fn paired_other(
    synchronized: &SynchronizedEdgeuses,
    index: usize,
    edge: EdgeId,
) -> Result<Option<EdgeuseId>> {
    synchronized
        .other_edgeuses
        .get(index)
        .copied()
        .ok_or_else(|| failed(OffsetFailure::EdgeOffsetsUnpaired { edge }))
}

/// The edge of `edgeuse` (a defect where it has none: [`EU_NO_EDGE`]).
#[track_caller]
fn edge_of(brep: &Brep, edgeuse: EdgeuseId) -> Result<EdgeId> {
    present(brep.edgeuse(edgeuse)?.edge, EU_NO_EDGE)
}

fn interval(brep: &Brep, edgeuse: EdgeuseId) -> Result<Extent1d> {
    Ok(brep.edge(edge_of(brep, edgeuse)?)?.interval)
}

fn start_vertex(brep: &Brep, edgeuse: EdgeuseId) -> Result<VertexId> {
    let vertexuse = present(brep.edgeuse(edgeuse)?.vertexuse, EU_NO_VERTEXUSE)?;
    present(brep.vertexuse(vertexuse)?.vertex, VU_NO_VERTEX)
}

/// The curve of offset edge `edge` (`SM_ERR`, as upstream's `SER`, when it has none: a
/// defect; or when it is of no B-spline curve kind, which the arc and line tests read:
/// [`OffsetFailure::CurveKindUnsupported`]).
fn bspline_edge_curve(brep: &Brep, edge: EdgeId) -> Result<&Curve> {
    let curve = brep
        .edge(edge)?
        .curve
        .as_ref()
        .ok_or_else(|| defect(OFFSET_EDGE_NO_CURVE, SM_ERR))?;
    if matches!(
        curve.kind(),
        CurveKind::BSpline(_)
            | CurveKind::Line(_)
            | CurveKind::Ellipse(_)
            | CurveKind::Hyperbola(_)
            | CurveKind::Parabola(_)
    ) {
        Ok(curve)
    } else {
        Err(Failure::new(
            OP,
            OffsetFailure::CurveKindUnsupported {
                edge: None,
                curve_type: curve.type_code(),
            },
        )
        .with_status(SM_ERR)
        .into_error())
    }
}

fn face_vertices(brep: &Brep, face: FaceId) -> Result<Vec<VertexId>> {
    let mut out = Vec::new();
    if let Some(faceuse) = brep.face(face)?.faceuse {
        for loopuse in eu::faceuse_loopuses(brep, faceuse, false, None)? {
            for vertex in loopuse::vertices(brep, loopuse, None)? {
                out.push(present(vertex, LOOP_NO_VERTEX)?);
            }
        }
    }
    Ok(out)
}

fn sort_edgeuses(brep: &Brep, edgeuses: &mut [EdgeuseId]) -> Result<()> {
    // Keep the upstream bubble sort: `>` does not exchange ties or NaNs.
    loop {
        let mut done = true;
        for i in 1..edgeuses.len() {
            // (`i - 1` and `i` are below the length: no failure to report)
            let Some(&[last, current]) = edgeuses.get(i - 1..=i) else {
                continue;
            };
            if interval(brep, last)?.min() > interval(brep, current)?.min() {
                edgeuses.swap(i - 1, i);
                done = false;
            }
        }
        if done {
            return Ok(());
        }
    }
}

fn insert_breaks(
    kernel: &mut Kernel,
    brep: &mut Brep,
    edgeuses: &[EdgeuseId],
    to_split: &mut Vec<EdgeuseId>,
    notifications: &mut NotifyState<'_>,
    curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
) -> Result<()> {
    for max_end in [false, true] {
        for &edgeuse in edgeuses {
            let source = interval(brep, edgeuse)?;
            let param = if max_end { source.max() } else { source.min() };
            // Uses appended by a split are scanned too: upstream re-reads GetSize.
            let mut index = 0;
            while let Some(&split) = to_split.get(index) {
                index += 1;
                let target = interval(brep, split)?;
                if !target.contains_value(param, 0.0)
                    || target.is_value_on_boundary(param, EFF_ZERO).0
                {
                    continue;
                }
                // The split reads the offset B-rep's own editing state (the executive's
                // constructors enable it, cpp:437, 1073). Dependency errors propagate;
                // only SM_ERR continues.
                let edge_id = edge_of(brep, split)?;
                let output = crate::topology::brep::p9::make_vertex_split_edge_with_state(
                    kernel,
                    brep,
                    edge_id,
                    param,
                    notifications,
                    curve_ref,
                )?;
                if !output.split {
                    continue; // Can't split if too close to end point
                }
                let new_edges = present(
                    output.edges,
                    "the split of an offset edge reported it split but returned no edges, though \
                     a split that succeeds returns the two edges it made",
                )?;
                let ccw = present(eu::ccw_edgeuse(brep, split)?, EU_NO_NEIGHBOUR)?;
                let cw = present(eu::cw_edgeuse(brep, split)?, EU_NO_NEIGHBOUR)?;
                if new_edges.contains(&edge_of(brep, ccw)?) {
                    to_split.push(ccw);
                }
                if new_edges.contains(&edge_of(brep, cw)?) {
                    to_split.push(cw);
                }
                break;
            }
        }
    }
    Ok(())
}

fn remove_small_stuff(k: &Kernel, brep: &Brep, edgeuses: &mut Vec<EdgeuseId>) -> Result<()> {
    let mut lengths = Vec::new();
    let mut total_length = 0.0;
    for &edgeuse in edgeuses.iter() {
        let edge = brep.edge(edge_of(brep, edgeuse)?)?;
        let curve = present(edge.curve.as_ref(), OFFSET_EDGE_NO_CURVE)?;
        let length = curve.approximate_length(k, &edge.interval, 10, false)?.0;
        lengths.push(length);
        total_length += length;
    }
    let mut kept = Vec::new();
    for (&edgeuse, length) in edgeuses.iter().zip(lengths) {
        if length > total_length / 10.0 {
            kept.push(edgeuse);
        }
    }
    *edgeuses = kept;
    Ok(())
}

fn extract_gaps(
    brep: &Brep,
    edgeuses: &mut Vec<EdgeuseId>,
    radial: &[EdgeuseId],
    gaps: &mut Vec<EdgeuseId>,
) -> Result<()> {
    let mut found = Vec::new();
    for &edgeuse in edgeuses.iter() {
        let mid = interval(brep, edgeuse)?.evaluate(0.5);
        let mut matched = false;
        for &other in radial {
            if interval(brep, other)?.contains_value(mid, 0.0) {
                found.push(edgeuse);
                matched = true;
                break;
            }
        }
        if !matched {
            gaps.push(edgeuse);
        }
    }
    *edgeuses = found;
    Ok(())
}

/// The identities `RemoveExcessFaces`' point solve keys the original B-rep by: each
/// edge (and its curve) and face (and its surface) is identified by the key its
/// geometry is owned under (upstream compares pointers, so geometry belongs to the
/// record that holds it), and by [`arena_identity`] when it has no owner; every other
/// object by [`arena_identity`].
pub(super) struct OriginalKeys {
    edges: BTreeMap<EdgeId, ObjectRef>,
    faces: BTreeMap<FaceId, ObjectRef>,
}

impl OriginalKeys {
    fn new(original: &Brep) -> Result<Self> {
        let mut edges = BTreeMap::new();
        for (id, edge) in original.edges().iter() {
            if let Some(owner) = edge.curve.as_ref().and_then(Curve::owner) {
                edges.insert(id, owner);
            }
        }
        let mut faces = BTreeMap::new();
        for (id, face) in original.faces().iter() {
            if let Some(owner) = face.surface.as_ref().and_then(Surface::owner) {
                faces.insert(id, owner);
            }
        }
        let keys = Self { edges, faces };
        // An owner shared by two records, or naming another record's identity, is an
        // indirect owner chain: reject it before cache construction can skip geometry.
        let mut seen = BTreeSet::new();
        let objects = original
            .edges()
            .ids()
            .map(CacheObject::Edge)
            .chain(original.faces().ids().map(CacheObject::Face))
            .chain(original.vertices().ids().map(CacheObject::Vertex))
            .chain([CacheObject::Brep]);
        for object in objects {
            let key = keys.key(object);
            if !seen.insert(key) {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "point-solve geometry owners must identify one edge or face each",
                });
            }
        }
        Ok(keys)
    }

    fn key(&self, object: CacheObject) -> ObjectRef {
        let owner = match object {
            CacheObject::Edge(e) | CacheObject::Curve(e) => self.edges.get(&e),
            CacheObject::Face(f) | CacheObject::Surface(f) => self.faces.get(&f),
            _ => None,
        };
        owner.copied().unwrap_or_else(|| arena_identity(object))
    }
}

pub(super) fn make_original_brep_cache(
    kernel: &Kernel,
    original: &Brep,
) -> Result<(BrepCache, OriginalKeys)> {
    let keys = OriginalKeys::new(original)?;
    let cache = make_brep_cache(kernel, CacheSource::Brep(original), &|object| {
        keys.key(object)
    })?;
    Ok((cache, keys))
}

/// `pOriginal->BrepPointSolve(..)`. The face surfaces reach their faces' caches
/// (`smsurf_GetSurfaceCache`, which may create UV trim curves on `original`): the
/// read-only solve requests each missing cache or trim, which is prepared on
/// `original` before the solve runs again from its start.
pub(super) fn solve_original_brep_point(
    kernel: &Kernel,
    original: &mut Brep,
    cache: &BrepCache,
    keys: &OriginalKeys,
    point: Vec3,
) -> Result<SolutionArray<CacheObject>> {
    let mut preparation = crate::query::brep::RayPreparation::default();
    loop {
        let objects = BrepObjects {
            preparation: Some(&preparation),
            brep: original,
            // There is exactly one B-rep in this query adapter's local namespace.
            brep_id: crate::topology::BrepId::from_index(0),
            cache: Some(cache),
            keys: &|object| keys.key(object),
            // OriginalKeys identifies every edge and face by its geometry's own owner.
            owner_of: &|_| None,
        };
        let result = TopologySolver::brep_point_solve(
            kernel,
            &objects,
            Some(CacheObject::Brep),
            point,
            TopologySolveOptions {
                operation: SolverOperation::Minimize,
                requested: SolutionRequested::Single,
                tolerance_3d: original.tolerance(),
                best_answer_so_far_sq: BIG_DOUBLE,
                opt_vectors: &[],
            },
        );
        match (result, preparation.take_request()) {
            (Err(SolidError::PreparationRequired { .. }), Some(request)) => {
                preparation.prepare(kernel, original, request)?;
            }
            (result, _) => return result,
        }
    }
}

fn collect_excess_faces(
    result: &Brep,
    edge: EdgeId,
    solutions: &[Solution<CacheObject>],
    min_distance: f64,
    shell_faces: &[FaceId],
    delete_faces: &mut Vec<FaceId>,
) -> Result<()> {
    if let [solution] = solutions {
        if solution.start.solution_value < min_distance {
            let edge_faces = edge::faces(result, edge, None)?;
            if edge_faces.iter().any(|face| shell_faces.contains(face)) {
                return Ok(());
            }
            for face in edge_faces {
                if !delete_faces.contains(&face) {
                    delete_faces.push(face);
                }
            }
        }
    }
    Ok(())
}

fn notify_edge_and_vertices(
    original: &Brep,
    old_edge: EdgeId,
    result: &Brep,
    new_edge: EdgeId,
    env: &mut NotifyEnv<'_>,
    offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<()> {
    // For Copy, the Edge and Vertex overrides directly delegate to SmTopology;
    // neither traverses uses or invalidates geometry. SM_NO_GET_BREP is NULL in
    // the pinned reference. Distinct key callbacks disambiguate the two arenas.
    let mut notify = |old, new| {
        ignore_status(topology::notify(
            env.records,
            env.attributes,
            env.context.as_deref_mut(),
            NotifyOperation::Copy,
            (env.key_of)(old),
            Some(offset_key_of(new)),
            None,
            None,
        ))
    };
    notify(EntityRef::Edge(old_edge), EntityRef::Edge(new_edge))?;
    let old_start = present(edge::start_vertex(original, old_edge)?, EDGE_NO_VERTEX)?;
    let new_start = present(edge::start_vertex(result, new_edge)?, EDGE_NO_VERTEX)?;
    notify(EntityRef::Vertex(old_start), EntityRef::Vertex(new_start))?;
    let old_end = present(
        edge::other_vertex(original, old_edge, Some(old_start))?,
        EDGE_NO_VERTEX,
    )?;
    if old_end != old_start {
        let new_end = present(
            edge::other_vertex(result, new_edge, Some(new_start))?,
            EDGE_NO_VERTEX,
        )?;
        notify(EntityRef::Vertex(old_end), EntityRef::Vertex(new_end))?;
    }
    Ok(())
}

/// `sm_ForgetSeconds`: forgets the pairs of `relation` whose second is in `gone` or,
/// with `live`, is not in `live`.
pub(super) fn forget_seconds<A: Ord + Copy, B: Ord + Copy>(
    relation: &mut Relation<A, B>,
    gone: &BTreeSet<B>,
    live: Option<&BTreeSet<B>>,
) -> Result<()> {
    for second in relation.all_seconds() {
        if !gone.contains(&second) && live.is_none_or(|live| live.contains(&second)) {
            continue;
        }
        for first in relation.firsts(second) {
            relation.remove_pair(first, second)?;
        }
    }
    Ok(())
}

/// `sm_ForgetFirsts`: forgets the pairs of `relation` whose first is in `gone`.
fn forget_firsts<A: Ord + Copy, B: Ord + Copy>(
    relation: &mut Relation<A, B>,
    gone: &BTreeSet<A>,
) -> Result<()> {
    for first in relation.all_firsts() {
        if !gone.contains(&first) {
            continue;
        }
        for second in relation.seconds(first) {
            relation.remove_pair(first, second)?;
        }
    }
    Ok(())
}

/// The edge uses `RemoveFaces` deletes with `faces` (`RemoveExcessFaces`' `sGone`,
/// collected before the removal): for each face, the uses of its edges on that face.
fn removed_faces_edgeuses(result: &Brep, faces: &[FaceId]) -> Result<BTreeSet<EdgeuseId>> {
    let mut gone = BTreeSet::new();
    for &f in faces {
        for e in face_edges(result, f)? {
            for u in edge::edgeuses(result, e, None, None)? {
                if eu::face(result, u)? == Some(f) {
                    gone.insert(u);
                }
            }
        }
    }
    Ok(gone)
}

/// Every [`ids::OFFSET_ROLE`] on the faces of `b` removed.
///
/// # Errors
///
/// Those of the attribute store.
fn clear_offset_roles(b: &mut Brep) -> Result<()> {
    for f in b.face_list().to_vec() {
        let e = EntityRef::Face(f);
        let marks: Vec<_> = b
            .entity_attributes(e)
            .into_iter()
            .filter(|(_, a)| a.attribute_id() == ids::OFFSET_ROLE)
            .map(|(h, _)| h)
            .collect();
        for h in marks {
            b.remove_attribute(e, h)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::error::ErrorKind;
    use crate::geom::attribute::{AttributeStore, ObjectKey};
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector2d::Vec2;
    use crate::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::surface::extend::Continuity;
    use crate::surface::plane::Plane;
    use crate::surface::side::BoxSide;
    use crate::topology::{Edge, Edgeuse, Face, Faceuse, ShellId, Vertex, Vertexuse};

    /// The fork's tangency-labs/smlib@9d8ace9 (tangency-labs/solidcore#1795, WR-344):
    /// an edge whose mate failed is left out of the merge's no-SSI pairs in the
    /// tangent-sector branch too, not only the extended convex one. A tangent sector whose
    /// mate failed is intersected.
    #[test]
    fn fork_9d8ace9_unmated_tangent_sector_is_intersected() {
        let tangent = || Ok(true);
        let not_tangent = || Ok(false);
        // the fork's fix: an unmated edge is related in neither branch
        for extend_convex in [false, true] {
            assert!(!relates_mated_offsets(extend_convex, true, tangent).unwrap());
            assert!(!relates_mated_offsets(extend_convex, true, not_tangent).unwrap());
        }
        // a mated edge: related with extended convex edges, else when tangent
        assert!(relates_mated_offsets(true, false, not_tangent).unwrap());
        assert!(relates_mated_offsets(false, false, tangent).unwrap());
        assert!(!relates_mated_offsets(false, false, not_tangent).unwrap());
        // the tangency test is not evaluated where the fork short-circuits it
        let unreachable = || -> Result<bool> { Err(SolidError::sm_err("test: not evaluated")) };
        assert!(!relates_mated_offsets(true, true, unreachable).unwrap());
        assert!(relates_mated_offsets(true, false, unreachable).unwrap());
    }

    /// Runs `run` with empty notification stores keyed by arena identity.
    fn with_state<R>(run: impl FnOnce(&mut NotifyState<'_>) -> R) -> R {
        let mut records = topology::NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let key = |entity| loopuse::notify_ref(entity);
        let mut env = NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key,
        };
        run(&mut NotifyState::from_env(&mut env))
    }

    fn executive() -> OffsetExecutive {
        OffsetExecutive::new(OffsetOperation::SolidOffset, 0.125, 1.0e-6)
    }

    #[test]
    fn periodic_events_keep_distinct_supports_and_roots() {
        use super::super::periodic_boundary::{record_boundary_end, BoundaryEnd};

        let first = BoundaryEnd {
            edgeuse: EdgeuseId::from_index(0),
            vertex: VertexId::from_index(0),
            seam: EdgeuseId::from_index(1),
            across: EdgeuseId::from_index(2),
            point: Vec3::new(1.0, 0.0, 0.0),
        };
        let other_support = BoundaryEnd {
            across: EdgeuseId::from_index(3),
            ..first
        };
        let other_root = BoundaryEnd {
            point: Vec3::new(2.0, 0.0, 0.0),
            ..first
        };
        // The other use of the same seam observes an otherwise identical event.
        let duplicate = BoundaryEnd {
            seam: EdgeuseId::from_index(4),
            ..first
        };
        for observations in [
            [first, other_support, other_root, duplicate],
            [duplicate, other_root, other_support, first],
        ] {
            let mut events = Vec::new();
            for event in observations {
                record_boundary_end(&mut events, event);
            }
            assert_eq!(events.len(), 3);
            for expected in [first, other_support, other_root] {
                assert!(events.iter().any(|event| {
                    event.across == expected.across && event.point == expected.point
                }));
            }
        }
    }

    fn add_use(brep: &mut Brep, min: f64, max: f64, length: f64) -> EdgeuseId {
        let mut curve = Line::from_points(Vec3::ZERO, Vec3::new(length, 0.0, 0.0), 3);
        let interval = Extent1d::from_raw(min, max);
        if length != 0.0 {
            curve.edit_parameterization(&interval).unwrap();
        }
        let edge = brep
            .add_edge_record(Edge {
                vertices: [VertexId::from_index(0); 2],
                edgeuses: Vec::new(),
                interval,
                tolerance: 0.0,
                curve: Some(Curve::from(curve)),
            })
            .unwrap();
        let edgeuse = brep
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                ..Edgeuse::default()
            })
            .unwrap();
        brep.edge_mut(edge).unwrap().edgeuses.push(edgeuse);
        edgeuse
    }

    /// An edge use on a unit-length B-spline line at height `y`.
    fn add_bspline_use(brep: &mut Brep, y: f64) -> EdgeuseId {
        let line = Line::from_points(Vec3::new(0.0, y, 0.0), Vec3::new(1.0, y, 0.0), 3);
        let curve = Curve::from(line.base_with_nurb().unwrap().into_owned());
        let edge = brep
            .add_edge_record(Edge {
                vertices: [VertexId::from_index(0); 2],
                edgeuses: Vec::new(),
                interval: Extent1d::from_raw(0.0, 1.0),
                tolerance: 0.0,
                curve: Some(curve),
            })
            .unwrap();
        let edgeuse = brep
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge),
                ..Edgeuse::default()
            })
            .unwrap();
        brep.edge_mut(edge).unwrap().edgeuses.push(edgeuse);
        edgeuse
    }

    fn plane(z: f64) -> Surface {
        Surface::from(Plane::new(
            Vec3::new(0.0, 0.0, z),
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    /// A wire edge with real vertices and a primary/mate pair, as in the p9 split tests.
    fn wire_use(brep: &mut Brep, min: f64, max: f64) -> EdgeuseId {
        let mut curve = Line::from_points(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 3);
        let interval = Extent1d::from_raw(min, max);
        curve.edit_parameterization(&interval).unwrap();
        let v0 = brep.add_vertex(Vec3::ZERO).unwrap();
        let v1 = brep.add_vertex(Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let edge = brep.add_edge(v0, v1).unwrap();
        let record = brep.edge_mut(edge).unwrap();
        record.interval = interval;
        record.curve = Some(Curve::from(curve));
        let mut primary = None;
        for (vertex, orientation) in [(v0, OrientType::Same), (v1, OrientType::Opposite)] {
            let vertexuse = brep
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(vertex),
                    ..Vertexuse::default()
                })
                .unwrap();
            let edgeuse = brep
                .add_edgeuse_record(Edgeuse {
                    edge: Some(edge),
                    owner: crate::topology::EdgeuseOwner::Shell(None),
                    vertexuse: Some(vertexuse),
                    orientation,
                    ..Edgeuse::default()
                })
                .unwrap();
            brep.vertexuse_mut(vertexuse).unwrap().owner =
                crate::topology::VertexuseOwner::Edgeuse(Some(edgeuse));
            brep.vertex_mut(vertex).unwrap().vertexuses.push(vertexuse);
            brep.edge_mut(edge).unwrap().edgeuses.push(edgeuse);
            primary.get_or_insert(edgeuse);
        }
        primary.unwrap()
    }

    fn split_entities(brep: &Brep) -> Vec<EntityRef> {
        brep.edges()
            .iter()
            .map(|(id, _)| EntityRef::Edge(id))
            .chain(brep.edgeuses().iter().map(|(id, _)| EntityRef::Edgeuse(id)))
            .chain(brep.vertices().iter().map(|(id, _)| EntityRef::Vertex(id)))
            .chain(
                brep.vertexuses()
                    .iter()
                    .map(|(id, _)| EntityRef::Vertexuse(id)),
            )
            .collect()
    }

    fn split_env<R>(
        brep_key: ObjectKey,
        entities: &[EntityRef],
        run: impl FnOnce(&mut NotifyEnv<'_>, &dyn Fn(EdgeId, i64) -> ObjectRef) -> R,
    ) -> R {
        let key = |entity| {
            let (index, type_code) = match entity {
                EntityRef::Edge(id) => (id.index(), edge::TYPE),
                EntityRef::Edgeuse(id) => (id.index(), eu::TYPE),
                EntityRef::Vertex(id) => (id.index(), vertex::TYPE),
                EntityRef::Vertexuse(id) => (id.index(), crate::topology::vertexuse::TYPE),
                _ => panic!("unexpected split fixture entity"),
            };
            ObjectRef {
                key: ObjectKey(
                    1_000_000 + u64::try_from(type_code).unwrap() * 1000 + u64::from(index),
                ),
                type_code,
            }
        };
        let mut records =
            topology::NotifyRecords::from(BTreeMap::from([(brep_key, topology::Topology::new())]));
        for &entity in entities {
            records.insert(key(entity).key, topology::Topology::new());
        }
        let mut attributes = AttributeStore::default();
        let mut env = NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key,
        };
        run(&mut env, &|edge, kind| ObjectRef {
            key: ObjectKey(1000 + u64::from(edge.index())),
            type_code: kind,
        })
    }

    fn sync(
        executive: &mut OffsetExecutive,
        original: Option<EdgeuseId>,
        other: Option<EdgeuseId>,
    ) -> Result<SynchronizedEdgeuses> {
        let mut kernel = Kernel::default();
        let entities = executive
            .offset_brep()
            .map(split_entities)
            .unwrap_or_default();
        let brep_key = executive.offset_brep().map_or(ObjectKey(0), Brep::key);
        split_env(brep_key, &entities, |env, curve_ref| {
            executive.synchronize_offset_edgeuses(&mut kernel, original, other, env, curve_ref)
        })
    }

    #[test]
    fn settings_defaults_and_scalar_view() {
        for (code, operation) in [
            OffsetOperation::SolidOffset,
            OffsetOperation::NonManifoldOffset,
            OffsetOperation::Shell,
            OffsetOperation::LocalOperation,
            OffsetOperation::MachineOffset,
        ]
        .into_iter()
        .enumerate()
        {
            let executive = OffsetExecutive::new(operation, -3.25, 1.0e-12);
            assert_eq!(operation as usize, code);
            assert_eq!(executive.operation(), operation);
            assert!(!executive.inset());
            assert!(!executive.extend_convex_edges());
            assert!(!executive.merge_results());
            assert!(!executive.create_offset_solid());
            assert!(executive.shell_faces().is_empty());
            assert!(executive.offset_brep().is_none());
            let radians = 2.0 * PI / 180.0;
            assert_eq!(
                executive.tangency_tol_degrees().to_bits(),
                (radians * 180.0 / PI).to_bits()
            );
            assert_eq!(executive.offset_distance(), -3.25);
            assert_eq!(executive.approximation_tol_3d(), 1.0e-12);
        }
    }

    #[test]
    fn geometry_parameter_snapshot_preserves_custom_tangency_conversion_order() {
        for radians in [
            0.0,
            -0.0,
            f64::MIN_POSITIVE,
            1.0e-300,
            1.0e-12,
            PI,
            PI / 7.0,
            0.31,
        ] {
            let executive = OffsetExecutive::with_geometry_parameters(
                OffsetOperation::LocalOperation,
                -4.25,
                3.0e-8,
                radians,
            );
            assert_eq!(executive.operation(), OffsetOperation::LocalOperation);
            assert_eq!(executive.offset_distance(), -4.25);
            assert_eq!(executive.approximation_tol_3d(), 3.0e-8);
            assert_eq!(
                executive.tangency_tol_degrees().to_bits(),
                (radians * 180.0 / PI).to_bits()
            );
            assert!(!executive.inset());
            assert!(!executive.extend_convex_edges());
            assert!(!executive.merge_results());
            assert!(!executive.create_offset_solid());
        }
        // This value distinguishes upstream's order from precomputing 180 / PI.
        let radians = 0.31;
        assert_ne!(
            (radians * 180.0 / PI).to_bits(),
            (radians * (180.0 / PI)).to_bits()
        );
    }

    #[test]
    fn setters_are_independent_and_copy_shell_faces_without_deduplication() {
        let mut executive = executive();
        executive.set_inset(true);
        executive.set_extend_convex_edges(true);
        executive.set_merge_results(true);
        executive.set_create_offset_solid(true);
        executive.set_operation(OffsetOperation::MachineOffset);
        let f = FaceId::from_index(9);
        let mut faces = vec![f, FaceId::from_index(2), f];
        executive.set_shell_faces(&faces);
        faces.clear();
        assert_eq!(executive.shell_faces(), &[f, FaceId::from_index(2), f]);
        assert!(executive.inset() && executive.extend_convex_edges());
        assert!(executive.merge_results() && executive.create_offset_solid());
        assert_eq!(executive.operation(), OffsetOperation::MachineOffset);
        executive.set_shell_faces(&[]);
        assert!(executive.shell_faces().is_empty());
        executive.set_offset_brep(Brep::new());
        assert!(executive.offset_brep().unwrap().faces().is_empty());
        executive.set_inset(false);
        assert!(!executive.inset());
        assert!(executive.extend_convex_edges());
    }

    #[test]
    fn relations_deduplicate_but_do_not_choose_among_multiple_results() {
        let mut executive = executive();
        let a = EdgeuseId::from_index(2);
        let b = EdgeuseId::from_index(7);
        let c = EdgeuseId::from_index(8);
        assert_eq!(executive.offset_edgeuse(a), None);
        executive.relate_edgeuses(a, b);
        executive.relate_edgeuses(a, b);
        assert_eq!(executive.offset_edgeuse(a), Some(b));
        executive.relate_edgeuses(a, c);
        assert_eq!(executive.offset_edgeuse(a), None);
        executive.relate_edgeuses(c, b);
        assert_eq!(executive.offset_edgeuse(c), Some(b));
    }

    #[test]
    fn synchronization_null_original_and_empty_maps_do_not_need_a_brep() {
        let mut executive = executive();
        let r = sync(&mut executive, None, None);
        assert_eq!(
            outcome(&r),
            (ErrorKind::InvalidInput, Some(SM_ERR_NULL_POINTER), false),
            "{r:?}"
        );
        let a = EdgeuseId::from_index(100);
        assert_eq!(
            sync(&mut executive, Some(a), None).unwrap(),
            SynchronizedEdgeuses::default()
        );
    }

    #[test]
    fn unmapped_first_side_sorts_but_does_not_filter_the_other_side() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let high = add_use(&mut brep, 5.0, 6.0, 100.0);
        let low = add_use(&mut brep, 0.0, 1.0, 0.01);
        // Early return must not even ask for curves on the other side.
        let high_edge = edge_of(&brep, high).unwrap();
        let low_edge = edge_of(&brep, low).unwrap();
        brep.edge_mut(high_edge).unwrap().curve = None;
        brep.edge_mut(low_edge).unwrap().curve = None;
        let a = EdgeuseId::from_index(50);
        let b = EdgeuseId::from_index(51);
        executive.relate_edgeuses(b, high);
        executive.relate_edgeuses(b, low);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert_eq!(out.edgeuses, vec![None, None]);
        assert_eq!(out.other_edgeuses, vec![Some(low), Some(high)]);
        assert!(out.gaps.is_empty() && out.other_gaps.is_empty());
    }

    #[test]
    fn strict_ten_percent_removal_then_null_padding() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let small = add_use(&mut brep, 0.0, 1.0, 1.0);
        let large = add_use(&mut brep, 1.0, 2.0, 9.0);
        let original = EdgeuseId::from_index(90);
        executive.relate_edgeuses(original, small);
        executive.relate_edgeuses(original, large);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(original), None).unwrap();
        assert_eq!(out.edgeuses, vec![Some(large)]);
        assert_eq!(out.other_edgeuses, vec![None]);
        // Filtering is local; correspondence cardinality is not rewritten.
        assert_eq!(executive.offset_edgeuse(original), None);
    }

    #[test]
    fn zero_length_and_ten_equal_pieces_are_all_removed() {
        let k = &Kernel::default();
        let mut brep = Brep::new();
        let zero = add_use(&mut brep, 0.0, 0.0, 0.0);
        let mut uses = vec![zero];
        remove_small_stuff(k, &brep, &mut uses).unwrap();
        assert!(uses.is_empty());
        for i in 0..10 {
            uses.push(add_use(&mut brep, f64::from(i), f64::from(i + 1), 1.0));
        }
        remove_small_stuff(k, &brep, &mut uses).unwrap();
        assert!(uses.is_empty());
    }

    #[test]
    fn synchronization_second_side_is_filtered_before_its_empty_branch() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let keep = add_use(&mut brep, 0.0, 2.0, 2.0);
        let zero = add_use(&mut brep, 0.0, 0.0, 0.0);
        let a = EdgeuseId::from_index(80);
        let b = EdgeuseId::from_index(81);
        executive.relate_edgeuses(a, keep);
        executive.relate_edgeuses(b, zero);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert_eq!(out.edgeuses, vec![Some(keep)]);
        assert_eq!(out.other_edgeuses, vec![None]);
    }

    #[test]
    fn synchronization_filtered_first_side_still_leaves_second_unfiltered() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let zero = add_use(&mut brep, 0.0, 0.0, 0.0);
        let small = add_use(&mut brep, 0.0, 1.0, 1.0);
        let large = add_use(&mut brep, 1.0, 2.0, 9.0);
        let a = EdgeuseId::from_index(80);
        let b = EdgeuseId::from_index(81);
        executive.relate_edgeuses(a, zero);
        executive.relate_edgeuses(b, small);
        executive.relate_edgeuses(b, large);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert_eq!(out.edgeuses, vec![None, None]);
        assert_eq!(out.other_edgeuses, vec![Some(small), Some(large)]);
    }

    #[test]
    fn synchronization_matches_intervals_without_requiring_splits() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let high_a = add_use(&mut brep, 1.0, 2.0, 1.0);
        let low_a = add_use(&mut brep, 0.0, 1.0, 1.0);
        let low_b = add_use(&mut brep, 0.0, 1.0, 1.0);
        let high_b = add_use(&mut brep, 1.0, 2.0, 1.0);
        let a = EdgeuseId::from_index(80);
        let b = EdgeuseId::from_index(81);
        for offset in [high_a, low_a] {
            executive.relate_edgeuses(a, offset);
        }
        for offset in [low_b, high_b] {
            executive.relate_edgeuses(b, offset);
        }
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert_eq!(out.edgeuses, vec![Some(low_a), Some(high_a)]);
        assert_eq!(out.other_edgeuses, vec![Some(low_b), Some(high_b)]);
        assert!(out.gaps.is_empty() && out.other_gaps.is_empty());
    }

    #[test]
    fn synchronization_extracts_gaps_in_sorted_order() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let high = add_use(&mut brep, 4.0, 5.0, 1.0);
        let low = add_use(&mut brep, 0.0, 1.0, 1.0);
        let other = add_use(&mut brep, 2.0, 3.0, 1.0);
        let a = EdgeuseId::from_index(80);
        let b = EdgeuseId::from_index(81);
        executive.relate_edgeuses(a, high);
        executive.relate_edgeuses(a, low);
        executive.relate_edgeuses(b, other);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert!(out.edgeuses.is_empty() && out.other_edgeuses.is_empty());
        assert_eq!(out.gaps, vec![low, high]);
        assert_eq!(out.other_gaps, vec![other]);
    }

    #[test]
    fn bubble_sort_preserves_ties_and_unordered_comparisons() {
        let mut brep = Brep::new();
        let a = add_use(&mut brep, 1.0, 2.0, 1.0);
        let b = add_use(&mut brep, 1.0, 3.0, 1.0);
        let c = add_use(&mut brep, 0.0, 1.0, 1.0);
        let mut uses = vec![b, a, c];
        sort_edgeuses(&brep, &mut uses).unwrap();
        assert_eq!(uses, vec![c, b, a]);
        let a_edge = edge_of(&brep, a).unwrap();
        brep.edge_mut(a_edge).unwrap().interval = Extent1d::from_raw(f64::NAN, 1.0);
        let mut uses = vec![b, a, c];
        sort_edgeuses(&brep, &mut uses).unwrap();
        assert_eq!(uses, vec![b, a, c]);
    }

    #[test]
    fn break_insertion_skips_boundaries_and_continues_past_refused_splits() {
        let mut kernel = Kernel::default();
        let mut brep = Brep::new();
        let whole = wire_use(&mut brep, 0.0, 1.0);
        let equal = add_use(&mut brep, 0.0, 1.0, 1.0);
        let near = add_use(&mut brep, EFF_ZERO / 2.0, 1.0, 1.0);
        let max_break = add_use(&mut brep, 0.0, 0.5, 1.0);
        let edges = brep.edges().len();
        let insert = |brep: &mut Brep, kernel: &mut Kernel, uses: &[EdgeuseId]| {
            let mut to_split = vec![whole];
            let entities = split_entities(brep);
            let result = split_env(brep.key(), &entities, |env, curve_ref| {
                insert_breaks(
                    kernel,
                    brep,
                    uses,
                    &mut to_split,
                    &mut NotifyState::from_env(env),
                    curve_ref,
                )
            });
            (result, to_split)
        };
        // Editing disabled (the B-rep constructor's default): every split is
        // refused with SM_ERR, so the scan continues and nothing is split.
        let (result, to_split) = insert(&mut brep, &mut kernel, &[equal, near, max_break]);
        result.unwrap();
        assert_eq!(to_split, vec![whole]);
        assert_eq!(brep.edges().len(), edges);
        // Interior by the boundary test but within MakeVertexSplitEdge's scaled zero:
        // refused even with editing enabled.
        brep.set_editing_enabled(true);
        let exact = add_use(&mut brep, EFF_ZERO, 1.0, 1.0);
        let edges = brep.edges().len();
        let (result, to_split) = insert(&mut brep, &mut kernel, &[equal, near, exact]);
        result.unwrap();
        assert_eq!(to_split, vec![whole]);
        assert_eq!(brep.edges().len(), edges);
        assert!(!kernel.context.doing_boolean());
        // A strict interior break splits; the scan then reads the split use's CCW edge
        // use, which this hand-built wire use leaves unlinked (upstream would dereference
        // NULL), so it stops there: a defect (a B-rep the kernel made links every edge use
        // of a loop to its neighbours), with SM_ERR_NULL_POINTER.
        let (result, _) = insert(&mut brep, &mut kernel, &[max_break]);
        assert_eq!(
            outcome(&result),
            (ErrorKind::StageFailed, Some(SM_ERR_NULL_POINTER), true),
            "{result:?}"
        );
        assert_eq!(brep.edges().len(), edges + 1);
    }

    #[test]
    fn synchronization_without_editing_refuses_splits() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let part = add_use(&mut brep, 0.25, 1.0, 1.0);
        let whole = wire_use(&mut brep, 0.0, 1.0);
        let a = EdgeuseId::from_index(80);
        let b = EdgeuseId::from_index(81);
        executive.relate_edgeuses(a, part);
        executive.relate_edgeuses(b, whole);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert_eq!(out.edgeuses, vec![Some(part)]);
        assert_eq!(out.other_edgeuses, vec![Some(whole)]);
        assert_eq!(executive.offset_brep().unwrap().edges().len(), 2);
    }

    #[test]
    fn synchronization_reports_split_dependency_without_changing_relations() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let part = add_use(&mut brep, 0.25, 1.0, 1.0);
        let whole = wire_use(&mut brep, 0.0, 1.0);
        let a = EdgeuseId::from_index(80);
        let b = EdgeuseId::from_index(81);
        executive.relate_edgeuses(a, part);
        executive.relate_edgeuses(b, whole);
        brep.set_editing_enabled(true);
        executive.set_offset_brep(brep);
        // The split completes; the unlinked CCW edge use of the hand-built wire use then
        // stops the scan (see break_insertion_skips_boundaries_and_continues_past_refused_splits).
        let r = sync(&mut executive, Some(a), Some(b));
        assert_eq!(
            outcome(&r),
            (ErrorKind::StageFailed, Some(SM_ERR_NULL_POINTER), true),
            "{r:?}"
        );
        assert_eq!(executive.offset_brep().unwrap().edges().len(), 3);
        assert_eq!(executive.offset_edgeuse(a), Some(part));
        assert_eq!(executive.offset_edgeuse(b), Some(whole));
    }

    #[test]
    fn extraction_uses_midpoint_inclusive_containment_and_preserves_existing_gaps() {
        let mut brep = Brep::new();
        let a = add_use(&mut brep, 0.0, 2.0, 1.0);
        let b = add_use(&mut brep, 5.0, 6.0, 1.0);
        let radial = add_use(&mut brep, 1.0, 3.0, 1.0);
        let mut uses = vec![b, a];
        let mut gaps = vec![radial];
        extract_gaps(&brep, &mut uses, &[radial], &mut gaps).unwrap();
        assert_eq!(uses, vec![a]);
        assert_eq!(gaps, vec![radial, b]);
    }

    #[test]
    fn invalid_mapped_ids_return_errors_instead_of_panicking() {
        let mut executive = executive();
        executive.relate_edgeuses(EdgeuseId::from_index(80), EdgeuseId::from_index(u32::MAX));
        executive.set_offset_brep(Brep::new());
        assert!(matches!(
            sync(&mut executive, Some(EdgeuseId::from_index(80)), None),
            Err(SolidError::NotFound { .. })
        ));
    }

    #[test]
    fn extension_cache_hits_by_caller_identity_and_keeps_keys_distinct() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let base = plane(0.0);
        let expected = base
            .extended_sides_keeping_parameters(
                &kernel,
                &BoxSide::ALL,
                Continuity::Ruled,
                0.125 * 5.0,
            )
            .unwrap()
            .surface;
        let first = executive
            .get_create_extended_surface(&kernel, 41, &base, 0.625)
            .unwrap();
        assert_eq!(
            first.natural_uv_domain_if_built(),
            expected.natural_uv_domain_if_built()
        );
        let first_point = first
            .evaluate_point(
                &kernel,
                first.natural_uv_domain_if_built().evaluate(0.5, 0.5),
            )
            .unwrap();
        // A cached identity does not evaluate even a different/malformed supplied base.
        let broken = Surface::from(BSplineSurface::new());
        let cached = executive
            .get_create_extended_surface(&kernel, 41, &broken, 0.625)
            .unwrap();
        assert_eq!(
            cached
                .evaluate_point(
                    &kernel,
                    cached.natural_uv_domain_if_built().evaluate(0.5, 0.5)
                )
                .unwrap(),
            first_point
        );
        let other = executive
            .get_create_extended_surface(&kernel, 42, &plane(7.0), 0.625)
            .unwrap();
        assert_eq!(
            other
                .evaluate_point(
                    &kernel,
                    other.natural_uv_domain_if_built().evaluate(0.5, 0.5)
                )
                .unwrap()
                .z,
            7.0
        );
        assert_eq!(executive.extended_surfaces.len(), 2);
    }

    /// A round's margin is its net continued, past where its definition holds (#17373).
    /// The round's spine runs x = 0 to 10 and its supports x = 0 to 20, so its definition
    /// continued 15 before x = 0 leaves both supports and is refused; the margin of 15 is
    /// the net extended ruled on every side, the net itself over the face and its straight
    /// spine continued 15 past each end.
    #[test]
    fn a_rounds_margin_is_its_net_continued_past_its_supports() {
        let (kernel, pb) = crate::surface::extend::tests::round_blend(1, true);
        let round = Surface::from(pb);
        let margin = 15.0;
        assert!(round
            .extended_sides_keeping_parameters(&kernel, &BoxSide::ALL, Continuity::Ruled, margin)
            .is_err());
        let mut executive = executive();
        let made = executive
            .get_create_extended_surface(&kernel, 7, &round, margin)
            .unwrap()
            .clone();
        assert!(
            matches!(made.kind(), SurfaceKind::BSpline(_)),
            "{:?}",
            made.kind()
        );
        let face = round.natural_uv_domain_with_nurb(&kernel).unwrap();
        let grown = made.natural_uv_domain_with_nurb(&kernel).unwrap();
        assert!(
            grown.min().x < face.min().x
                && grown.max().x > face.max().x
                && grown.min().y < face.min().y
                && grown.max().y > face.max().y,
            "{face:?} {grown:?}"
        );
        for i in 0..=4 {
            for j in 0..=4 {
                let uv = face.evaluate(f64::from(i) / 4.0, f64::from(j) / 4.0);
                let a = made.evaluate_point(&kernel, uv).unwrap();
                let b = round.evaluate_point(&kernel, uv).unwrap();
                assert!(a.distance(b) <= 1e-9, "{uv:?}: {a:?} {b:?}");
            }
        }
        let mid = 0.5 * (face.min().y + face.max().y);
        let end = |u: f64| made.evaluate_point(&kernel, Vec2::new(u, mid)).unwrap().x;
        assert!(end(grown.max().x) >= 25.0 - 1e-6, "{}", end(grown.max().x));
        assert!(end(grown.min().x) <= -15.0 + 1e-6, "{}", end(grown.min().x));
    }

    #[test]
    fn a_surface_is_extended_by_its_margin_none_by_zero_and_a_negative_one_is_refused() {
        let kernel = Kernel::default();
        let base = plane(0.0);
        let expected = base
            .extended_sides_keeping_parameters(&kernel, &BoxSide::ALL, Continuity::Ruled, 0.3)
            .unwrap()
            .surface;
        let mut executive = executive();
        let got = executive
            .get_create_extended_surface(&kernel, 1, &base, 0.3)
            .unwrap();
        assert_eq!(
            got.natural_uv_domain_if_built(),
            expected.natural_uv_domain_if_built()
        );
        let same = executive
            .get_create_extended_surface(&kernel, 2, &base, 0.0)
            .unwrap();
        assert_eq!(
            same.natural_uv_domain_if_built(),
            base.natural_uv_domain_if_built()
        );
        for refused in [-0.1, f64::NAN, f64::INFINITY] {
            assert!(executive
                .get_create_extended_surface(&kernel, 3, &base, refused)
                .is_err());
        }
        assert_eq!(executive.extended_surfaces.len(), 2);
    }

    /// A box offset by `d` either way: every face's offset is met `d` past (or inside) each
    /// edge and `d·√2` from each corner, so each face's margin is `d·√2` and one offset. An offset face takes the margin of the face use it was made from; one made
    /// from none, the body's largest.
    /// A local operation keeps an edge only where its faces, and the faces at its
    /// vertices, were given copies of their own surfaces: given no surfaces, every face is
    /// offset by the executive's distance and no edge is kept; given the box's faces with
    /// one moved, the edges away from it are kept, those touching it are not.
    #[test]
    fn a_local_operation_keeps_only_edges_whose_faces_keep_their_surfaces() {
        let mut kernel = Kernel::default();
        let brep = crate::primitive::api_primitives::create_box(
            &mut kernel,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            1.0,
            2.0,
            3.0,
        )
        .unwrap();
        let faces = bp1::faces(&brep, None).unwrap();
        let mut executive = OffsetExecutive::new(OffsetOperation::LocalOperation, 0.125, 1.0e-6);
        executive.keep_unmoved_edges(&brep).unwrap();
        assert!(executive.kept_edges.is_empty());
        assert!(executive.kept_edgeuses.is_empty());

        let surfaces = faces
            .iter()
            .map(|&f| brep.face(f).unwrap().surface.clone().unwrap())
            .collect();
        let moved = faces[0];
        executive.set_local_surfaces(&faces, surfaces, &[moved]);
        executive.keep_unmoved_edges(&brep).unwrap();
        // a box face's 4 vertices touch 8 of its 12 edges: the other 4 are kept
        let touching: BTreeSet<EdgeId> = bp1::vertices(&brep, None)
            .unwrap()
            .into_iter()
            .filter(|&v| vertex::faces(&brep, v).unwrap().contains(&moved))
            .flat_map(|v| vertex::edges(&brep, v, None).unwrap())
            .collect();
        assert_eq!(touching.len(), 8);
        let expected: BTreeSet<EdgeId> = bp1::edges(&brep, None)
            .unwrap()
            .into_iter()
            .filter(|e| !touching.contains(e))
            .collect();
        assert_eq!(expected.len(), 4);
        assert_eq!(executive.kept_edges, expected);
        // each edge has four uses: two per face, one on each of its face uses
        assert_eq!(executive.kept_edgeuses.len(), 16);
    }

    /// The reach's edge bounds ([`super::super::reach::EdgeBounds`]) live for one local
    /// operation: setting its surfaces forgets them, and so does clearing its relations.
    #[test]
    fn a_local_operations_edge_bounds_are_forgotten_with_its_surfaces() {
        let mut executive = OffsetExecutive::new(OffsetOperation::LocalOperation, 0.125, 1.0e-6);
        executive.edge_bounds.hold(EdgeId::from_index(3));
        assert_eq!(executive.edge_bounds.held(), 1);
        executive.set_local_surfaces(&[], Vec::new(), &[]);
        assert_eq!(executive.edge_bounds.held(), 0);
        executive.edge_bounds.hold(EdgeId::from_index(3));
        executive.clear_relations();
        assert_eq!(executive.edge_bounds.held(), 0);
    }

    #[test]
    fn a_box_face_margin_is_its_offset_corner_reach() {
        let mut kernel = Kernel::default();
        let mut brep = crate::primitive::api_primitives::create_box(
            &mut kernel,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            1.0,
            2.0,
            3.0,
        )
        .unwrap();
        let faces = bp1::faces(&brep, None).unwrap();
        let expected = 0.125 * 2.0_f64.sqrt() + 0.125;
        for (distance, inset) in [(0.125, false), (0.125, true)] {
            let mut executive =
                OffsetExecutive::new(OffsetOperation::SolidOffset, distance, 1.0e-6);
            executive.set_inset(inset);
            for &f in &faces {
                let faceuse =
                    super::super::p2::select_offset_faceuse(&brep, f, false, inset, "test")
                        .unwrap();
                executive
                    .record_extension_reach(&kernel, &mut brep, f, faceuse)
                    .unwrap();
                assert!((executive.original_extension_margin(f) - expected).abs() < 1e-12);
                executive.relate_faceuse(faceuse, FaceId::from_index(100));
            }
            assert!(
                (executive.offset_extension_margin(FaceId::from_index(100)) - expected).abs()
                    < 1e-12
            );
            assert!(
                (executive.offset_extension_margin(FaceId::from_index(101)) - expected).abs()
                    < 1e-12
            );
        }
    }

    #[test]
    fn failed_extensions_are_not_cached_and_can_be_retried() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let broken = Surface::from(BSplineSurface::new());
        assert!(executive
            .get_create_extended_surface(&kernel, 1, &broken, 0.625)
            .is_err());
        assert!(executive.extended_surfaces.is_empty());
        assert!(executive
            .get_create_extended_surface(&kernel, 1, &plane(0.0), 0.625)
            .is_ok());
    }

    #[test]
    fn sidewalls_handle_null_missing_ambiguous_and_single_mappings() {
        let mut kernel = Kernel::default();
        let mut executive = executive();
        // B-spline edge curves (a Line whose parameterisation was edited carries no NURB)
        let mut original_brep = Brep::new();
        let original = add_bspline_use(&mut original_brep, 0.0);
        let mut offset_brep = Brep::new();
        let offset = add_bspline_use(&mut offset_brep, 1.0);
        executive.set_offset_brep(offset_brep);
        executive
            .build_side_wall_of_edgeuse(&mut kernel, &mut original_brep, None)
            .unwrap();
        executive
            .build_side_wall_of_edgeuse(&mut kernel, &mut original_brep, Some(original))
            .unwrap();
        // the ruled blend across the two B-reps runs; the closing face is made by
        // topology.brep_p8 (CreateFaceFromSurface) on the offset B-rep, a bare
        // `Brep::new()` here (no infinite region), and fails there
        executive.relate_edgeuses(original, offset);
        let r =
            executive.build_side_wall_of_edgeuse(&mut kernel, &mut original_brep, Some(original));
        assert!(r.is_err());
        assert!(!matches!(
            r,
            Err(SolidError::Unported {
                unit: "topology.brep_p8",
                ..
            })
        ));
        executive.relate_edgeuses(original, EdgeuseId::from_index(2));
        executive
            .build_side_wall_of_edgeuse(&mut kernel, &mut original_brep, Some(original))
            .unwrap();
    }

    #[test]
    fn excess_face_early_returns_and_actual_dependencies() {
        let mut kernel = Kernel::default();
        let mut executive = executive();
        let mut original = Brep::new();
        let mut result = Brep::new();
        executive
            .remove_excess_faces(&mut kernel, &mut original, &mut result)
            .unwrap();
        executive.set_merge_results(true);
        executive.set_shell_faces(&[FaceId::from_index(1)]);
        executive
            .remove_excess_faces(&mut kernel, &mut original, &mut result)
            .unwrap();
        executive.set_shell_faces(&[]);
        // RemoveFaces' editing-disabled refusal, which upstream ignores (cpp:2099-2102)
        // and the fork passes on (WR-658, tangency-labs/smlib 4803d6d)
        let editing_refused = |r: Result<()>| {
            assert!(
                matches!(&r, Err(e) if e.sm_status() == Some(1001)
                    && e.to_string().contains("editing is not enabled")),
                "{r:?}"
            );
        };
        // No result edge: the original's editing flag is not touched; the empty
        // deletion list still reaches RemoveFaces, which refuses on the result whose
        // editing is disabled.
        editing_refused(executive.remove_excess_faces(&mut kernel, &mut original, &mut result));
        assert!(!original.editing_enabled());
        add_use(&mut result, 0.0, 1.0, 1.0);
        add_use(&mut result, 0.0, 1.0, 2.0);
        // The edges evaluate and solve against the empty original; no result face is
        // close to it, so nothing is removed, and RemoveFaces refuses as above.
        editing_refused(executive.remove_excess_faces(&mut kernel, &mut original, &mut result));
        assert_eq!(result.edges().len(), 2);
        assert_eq!(result.edgeuses().len(), 2);
        assert!(result.faces().is_empty());
        // Each edge's solve leaves the original's editing enabled (cpp:2069–2072).
        assert!(original.editing_enabled());
        // ShrinkGeometry trims the two edge curves; without offset surfaces the
        // continuation's loops are empty (SM_SUCCESS)
        let (mut records, mut attributes) =
            (topology::NotifyRecords::new(), AttributeStore::default());
        OffsetExecutive::replace_implicit_offsets(
            &mut Kernel::default(),
            &mut result,
            1.0e-6,
            &mut NotificationStores {
                records: &mut records,
                attributes: &mut attributes,
                key_of: &loopuse::notify_ref,
            },
        )
        .unwrap();
        assert_eq!(result.edges().len(), 2);
    }

    #[test]
    fn removed_offset_faces_leave_every_correspondence_in_both_directions() {
        let mut exec = executive();
        let gone = FaceId::from_index(0);
        let kept = FaceId::from_index(1);
        let blend = FaceId::from_index(2);
        let faceuses = [FaceuseId::from_index(0), FaceuseId::from_index(1)];
        let edges = [EdgeId::from_index(0), EdgeId::from_index(1)];
        let vertices = [VertexId::from_index(0), VertexId::from_index(1)];
        for face in [gone, kept] {
            for source in faceuses {
                exec.relate_faceuse(source, face);
            }
            for source in edges {
                exec.relate_edge_face(source, face);
            }
            for source in vertices {
                exec.relate_vertex_face(source, face);
            }
            exec.relate_blend_face(blend, face);
        }
        exec.relate_blend_face(gone, kept);
        exec.relate_blend_face(gone, gone);
        exec.relate_blend_face(kept, gone);

        // Duplicate candidates and repeated cleanup are harmless. Original ids can
        // have the same numeric indices as result ids, but must remain related to kept.
        exec.forget_faces(&BTreeSet::from([gone])).unwrap();
        exec.forget_faces(&BTreeSet::from([gone])).unwrap();
        for source in faceuses {
            assert_eq!(exec.offset_faces(source), vec![kept]);
        }
        for source in edges {
            assert_eq!(exec.edge_to_faces.seconds(source), vec![kept]);
        }
        for source in vertices {
            assert_eq!(exec.vertex_to_faces.seconds(source), vec![kept]);
        }
        assert_eq!(exec.blend_to_faces.seconds(blend), vec![kept]);
        assert!(exec.blend_to_faces.seconds(gone).is_empty());
        assert!(exec.blend_to_faces.seconds(kept).is_empty());
        assert!(exec.faceuse_to_faces.firsts(gone).is_empty());
        assert!(exec.edge_to_faces.firsts(gone).is_empty());
        assert!(exec.vertex_to_faces.firsts(gone).is_empty());
        assert!(exec.blend_to_faces.firsts(gone).is_empty());
        assert_eq!(exec.blend_to_faces.firsts(kept), vec![blend]);

        exec.forget_faces(&BTreeSet::from([kept])).unwrap();
        assert!(exec.faceuse_to_faces.all_firsts().is_empty());
        assert!(exec.edge_to_faces.all_firsts().is_empty());
        assert!(exec.vertex_to_faces.all_firsts().is_empty());
        assert!(exec.blend_to_faces.all_firsts().is_empty());
        assert!(exec.blend_to_faces.all_seconds().is_empty());
    }

    /// `RemoveExcessFaces`: `m_vEUToEU` forgets the pairs whose result edge use belongs
    /// to a removed face or to no result edge, and keeps the rest.
    #[test]
    fn forget_seconds_drops_removed_and_dead_edgeuses() {
        let mut k = Kernel::default();
        let result = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            1.0,
            2.0,
            3.0,
        )
        .unwrap();
        let faces = bp1::faces(&result, None).unwrap();
        let removed = faces[0];
        let mut on_removed = None;
        let mut elsewhere = None;
        let mut live = BTreeSet::new();
        for e in bp1::edges(&result, None).unwrap() {
            for u in edge::edgeuses(&result, e, None, None).unwrap() {
                live.insert(u);
                if eu::face(&result, u).unwrap() == Some(removed) {
                    on_removed.get_or_insert(u);
                } else {
                    elsewhere.get_or_insert(u);
                }
            }
        }
        let (on_removed, elsewhere) = (on_removed.unwrap(), elsewhere.unwrap());
        let dead = EdgeuseId::from_index(100_000);
        let originals = [1, 2, 3].map(EdgeuseId::from_index);
        let mut x = executive();
        for (o, r) in originals.into_iter().zip([on_removed, elsewhere, dead]) {
            x.edgeuse_to_edgeuses.relate_pair(o, r).unwrap();
        }
        let gone = removed_faces_edgeuses(&result, &[removed]).unwrap();
        assert!(gone.contains(&on_removed) && !gone.contains(&elsewhere));
        forget_seconds(&mut x.edgeuse_to_edgeuses, &gone, Some(&live)).unwrap();
        assert_eq!(x.edgeuse_to_edgeuses.all_seconds(), vec![elsewhere]);
        assert_eq!(x.edgeuse_to_edgeuses.seconds(originals[1]), vec![elsewhere]);
    }

    #[test]
    fn implicit_offsets_are_replaced_by_approximations_of_their_base() {
        use crate::surface::offset_surface::OffsetSurface;
        let patch = || {
            BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap()
        };
        let kernel = Kernel::default();
        let offset = OffsetSurface::new(&kernel, 0.25, patch().into(), true, 0).unwrap();
        let mut brep = Brep::new();
        let (plain, _) = add_face(&mut brep, plane(0.0));
        let (offset_face, _) = add_face(&mut brep, Surface::from(offset));
        let (mut records, mut attributes) =
            (topology::NotifyRecords::new(), AttributeStore::default());
        let mut kernel = kernel;
        OffsetExecutive::replace_implicit_offsets(
            &mut kernel,
            &mut brep,
            1.0e-6,
            &mut NotificationStores {
                records: &mut records,
                attributes: &mut attributes,
                key_of: &loopuse::notify_ref,
            },
        )
        .unwrap();
        // the plane is untouched; the offset face gets the approximation of its base
        // offset (the bilinear patch at z = 0.25) and that surface's natural domain
        assert!(matches!(
            brep.face(plain).unwrap().surface.as_ref().unwrap().kind(),
            SurfaceKind::Plane(_)
        ));
        let face = brep.face(offset_face).unwrap();
        let surface = face.surface.as_ref().unwrap();
        assert!(matches!(surface.kind(), SurfaceKind::BSpline(_)));
        assert_eq!(face.uv_domain, surface.natural_uv_domain_if_built());
        let p = surface
            .evaluate_point(&kernel, Vec2::new(0.5, 0.5))
            .unwrap();
        assert!((p.z - 0.25).abs() < 1.0e-6, "{p:?}");

        // a plane is an SmBSplineSurface too (issue #1476): its offset is approximated
        let offset = OffsetSurface::new(&kernel, 0.25, plane(0.0), true, 0).unwrap();
        let mut brep = Brep::new();
        let (offset_face, _) = add_face(&mut brep, Surface::from(offset));
        OffsetExecutive::replace_implicit_offsets(
            &mut kernel,
            &mut brep,
            1.0e-6,
            &mut NotificationStores {
                records: &mut records,
                attributes: &mut attributes,
                key_of: &loopuse::notify_ref,
            },
        )
        .unwrap();
        let surface = brep.face(offset_face).unwrap().surface.clone().unwrap();
        assert!(matches!(surface.kind(), SurfaceKind::BSpline(_)));
        let mid = surface.natural_uv_domain_if_built().mid();
        let p = surface.evaluate_point(&kernel, mid).unwrap();
        assert!((p.z - 0.25).abs() < 1.0e-6, "{p:?}");

        // a base that is not a B-spline surface (here an offset surface) is SM_ERR
        let inner = OffsetSurface::new(&kernel, 0.25, plane(0.0), true, 0).unwrap();
        let offset = OffsetSurface::new(&kernel, 0.25, Surface::from(inner), true, 0).unwrap();
        let mut brep = Brep::new();
        add_face(&mut brep, Surface::from(offset));
        let status = OffsetExecutive::replace_implicit_offsets(
            &mut kernel,
            &mut brep,
            1.0e-6,
            &mut NotificationStores {
                records: &mut records,
                attributes: &mut attributes,
                key_of: &loopuse::notify_ref,
            },
        );
        // a kind the routine cannot handle, named with the base's type, with the status it
        // replaced
        assert_eq!(
            outcome(&status),
            (ErrorKind::Unsupported, Some(SM_ERR), false),
            "{status:?}"
        );
        let e = status.unwrap_err();
        assert_eq!(e.operation(), OP);
        assert_eq!(
            e.offset_failure(),
            Some(&OffsetFailure::OffsetBaseKindUnsupported {
                surface_type: crate::surface::surface::p2::OFFSET_SURFACE_TYPE
            })
        );
    }

    /// The class, the status the goldens read and whether it is a defect, of `r`'s error.
    fn outcome<T: core::fmt::Debug>(r: &Result<T>) -> (ErrorKind, Option<i32>, bool) {
        let e = r.as_ref().unwrap_err();
        (e.kind(), crate::error::upstream_status(e), e.is_defect())
    }

    /// A missing link of a B-rep (the kernel's fault) is a defect; a missing argument is
    /// the caller's input. Both keep upstream's `SM_ERR_NULL_POINTER`.
    #[test]
    fn missing_links_are_defects_and_missing_arguments_input_with_their_status() {
        let null = Some(SM_ERR_NULL_POINTER);
        let mut executive = executive();
        // no edge use to synchronize: the caller's input
        let r = sync(&mut executive, None, None);
        assert_eq!(outcome(&r), (ErrorKind::InvalidInput, null, false), "{r:?}");
        let e = r.unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::EdgeuseNotGiven));
        // no vertex to offset: the caller's input
        let r = executive.build_offset_of_vertex(&Kernel::default(), &Brep::new(), None);
        assert_eq!(outcome(&r), (ErrorKind::InvalidInput, null, false), "{r:?}");
        let e = r.unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::VertexNotGiven));
        // a mapped edge use with no offset B-rep to read it in: the executive's fault
        let (a, b) = (EdgeuseId::from_index(80), EdgeuseId::from_index(81));
        executive.relate_edgeuses(a, b);
        let r = sync(&mut executive, Some(a), None);
        assert_eq!(outcome(&r), (ErrorKind::StageFailed, null, true), "{r:?}");
        assert_eq!(r.unwrap_err().operation(), OP);
    }

    /// The face-use offset: no face use is the caller's input; so is a face use of the
    /// input with no face, met when its edge uses are matched (a caller can build one), at
    /// the face use. Both keep `SM_ERR_NULL_POINTER`.
    #[test]
    fn the_face_use_offsets_missing_argument_and_links_keep_their_status() {
        let null = Some(SM_ERR_NULL_POINTER);
        let mut executive = executive();
        executive.set_offset_brep(Brep::new());
        let r = split_env(ObjectKey(0), &[], |env, curve_ref| {
            executive.build_offset_of_faceuse(
                &mut Kernel::default(),
                &mut Brep::new(),
                None,
                &mut None,
                &SsiData::default(),
                env,
                &loopuse::notify_ref,
                curve_ref,
            )
        });
        assert_eq!(outcome(&r), (ErrorKind::InvalidInput, null, false), "{r:?}");
        let e = r.unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::FaceuseNotGiven));
        let mut original = Brep::new();
        let faceuse = original.add_faceuse_record(Faceuse::default()).unwrap();
        let r = split_env(ObjectKey(0), &[], |env, _| {
            executive.map_edgeuses(
                &Kernel::default(),
                &mut original,
                faceuse,
                FaceId::from_index(0),
                env,
                &loopuse::notify_ref,
            )
        });
        assert_eq!(outcome(&r), (ErrorKind::InvalidInput, null, false), "{r:?}");
        let e = r.unwrap_err();
        assert_eq!(
            e.offset_failure(),
            Some(&OffsetFailure::FaceuseHasNoFace { faceuse })
        );
        assert_eq!(e.entity(), Some(EntityRef::Faceuse(faceuse)));
        // a face of the input with no surface, met when the normals are compared: the
        // caller's input, at the face
        let (face, faceuse) = add_face(&mut original, plane(0.0));
        original.face_mut(face).unwrap().surface = None;
        let mut offset = Brep::new();
        let (offset_face, _) = add_face(&mut offset, plane(1.0));
        executive.set_offset_brep(offset);
        let r = split_env(ObjectKey(0), &[], |env, _| {
            executive.map_edgeuses(
                &Kernel::default(),
                &mut original,
                faceuse,
                offset_face,
                env,
                &loopuse::notify_ref,
            )
        });
        assert_eq!(outcome(&r), (ErrorKind::InvalidInput, null, false), "{r:?}");
        let e = r.unwrap_err();
        assert_eq!(
            e.offset_failure(),
            Some(&OffsetFailure::FaceHasNoSurface { face })
        );
        // ... and an offset face with none: the offset's fault
        original.face_mut(face).unwrap().surface = Some(plane(0.0));
        let mut offset = executive.take_offset_brep().unwrap();
        offset.face_mut(offset_face).unwrap().surface = None;
        executive.set_offset_brep(offset);
        let r = split_env(ObjectKey(0), &[], |env, _| {
            executive.map_edgeuses(
                &Kernel::default(),
                &mut original,
                faceuse,
                offset_face,
                env,
                &loopuse::notify_ref,
            )
        });
        assert_eq!(outcome(&r), (ErrorKind::StageFailed, null, true), "{r:?}");
        assert!(r.unwrap_err().to_string().contains(OFFSET_FACE_NO_SURFACE));
    }

    /// A split the other side's break asks for can be refused (here: editing is off), so
    /// one side keeps more pieces than the other. The cap loop's read past the other side's
    /// list, a null pointer upstream, is named at the input's edge, with
    /// `SM_ERR_NULL_POINTER`.
    #[test]
    fn unpaired_pieces_after_a_refused_split_are_named_at_the_edge() {
        let mut executive = executive();
        let mut brep = Brep::new();
        let first_half = add_use(&mut brep, 0.0, 0.5, 1.0);
        let second_half = add_use(&mut brep, 0.5, 1.0, 1.0);
        let whole = wire_use(&mut brep, 0.0, 1.0);
        let (a, b) = (EdgeuseId::from_index(80), EdgeuseId::from_index(81));
        executive.relate_edgeuses(a, first_half);
        executive.relate_edgeuses(a, second_half);
        executive.relate_edgeuses(b, whole);
        executive.set_offset_brep(brep);
        let out = sync(&mut executive, Some(a), Some(b)).unwrap();
        assert_eq!(out.edgeuses, vec![Some(first_half), Some(second_half)]);
        assert_eq!(out.other_edgeuses, vec![Some(whole)]);
        let edge = EdgeId::from_index(9);
        assert_eq!(paired_other(&out, 0, edge), Ok(Some(whole)));
        let r = paired_other(&out, 1, edge);
        assert_eq!(
            outcome(&r),
            (ErrorKind::Unsupported, Some(SM_ERR_NULL_POINTER), false),
            "{r:?}"
        );
        let e = r.unwrap_err();
        assert_eq!(
            e.offset_failure(),
            Some(&OffsetFailure::EdgeOffsetsUnpaired { edge })
        );
        assert_eq!(e.entity(), Some(EntityRef::Edge(edge)));
    }

    /// p1's helpers: a defect keeps the status of the site it replaced and says why; the
    /// trimmed curve the pipe fallback follows is missing for the caller's edge with no
    /// curve, else by a defect; and a call whose status upstream ignores passes a defect on.
    #[test]
    fn p1_helpers_keep_the_status_and_class_they_replace() {
        let null = Some(SM_ERR_NULL_POINTER);
        let e = present(None::<()>, NO_OFFSET).unwrap_err();
        assert_eq!(
            (e.kind(), crate::error::upstream_status(&e)),
            (ErrorKind::StageFailed, null)
        );
        assert!(e.is_defect() && e.to_string().contains(NO_OFFSET), "{e}");
        let e = defect(OFFSET_EDGE_NO_CURVE, SM_ERR);
        assert_eq!(crate::error::upstream_status(&e), Some(SM_ERR));
        assert_eq!(e.operation(), OP);
        // an offset edge with no curve, where the arc and line tests read one: SM_ERR
        let mut brep = Brep::new();
        let offset_use = add_use(&mut brep, 0.0, 1.0, 1.0);
        let offset_edge = edge_of(&brep, offset_use).unwrap();
        brep.edge_mut(offset_edge).unwrap().curve = None;
        let r = bspline_edge_curve(&brep, offset_edge);
        assert_eq!(
            outcome(&r),
            (ErrorKind::StageFailed, Some(SM_ERR), true),
            "{r:?}"
        );
        // the pipe fallback's curve: the input's edge with none is the caller's input
        let e = no_trimmed_curve(&brep, offset_edge);
        assert_eq!(
            (e.kind(), crate::error::upstream_status(&e), e.entity()),
            (
                ErrorKind::InvalidInput,
                null,
                Some(EntityRef::Edge(offset_edge))
            )
        );
        // ... and one with a B-spline curve kind whose trim gave none, a defect
        let line_use = add_use(&mut brep, 0.0, 1.0, 1.0);
        let e = no_trimmed_curve(&brep, edge_of(&brep, line_use).unwrap());
        assert_eq!(
            (e.kind(), crate::error::upstream_status(&e), e.is_defect()),
            (ErrorKind::StageFailed, null, true),
            "{e}"
        );
        // a curve of no B-spline kind (here a Hermite curve): named at the input's edge for
        // the pipe (`SM_ERR_NULL_POINTER`), and with `SM_ERR` for the arc and line tests
        let hermite = Curve::from(crate::curve::hermite_curve::HermiteCurve::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::Y,
            3,
        ));
        let curve_type = hermite.type_code();
        brep.edge_mut(offset_edge).unwrap().curve = Some(hermite);
        let e = no_trimmed_curve(&brep, offset_edge);
        assert_eq!(
            (e.kind(), crate::error::upstream_status(&e)),
            (ErrorKind::Unsupported, null)
        );
        assert_eq!(
            e.offset_failure(),
            Some(&OffsetFailure::CurveKindUnsupported {
                edge: Some(offset_edge),
                curve_type
            })
        );
        let r = bspline_edge_curve(&brep, offset_edge);
        assert_eq!(
            outcome(&r),
            (ErrorKind::Unsupported, Some(SM_ERR), false),
            "{r:?}"
        );
        assert_eq!(
            r.unwrap_err().offset_failure(),
            Some(&OffsetFailure::CurveKindUnsupported {
                edge: None,
                curve_type
            })
        );
        // a defect is passed on; another failure is a step that did not apply
        assert!(ignored_unless_defect(Err::<(), _>(defect(NO_OFFSET, SM_ERR))).is_err());
        assert_eq!(
            ignored_unless_defect(Err::<(), _>(failed(OffsetFailure::VertexNotGiven))),
            Ok(None)
        );
        assert_eq!(ignored_unless_defect(Ok(3)), Ok(Some(3)));
    }

    #[test]
    fn kernel_context_removal_uses_actual_editing_flag_and_releases_nested_mark() {
        let mut kernel = Kernel::default();
        let mut original = Brep::new();
        let mut result = Brep::new_with_infinite_region().unwrap();
        let mut exec = executive();
        exec.set_merge_results(true);
        // Native RemoveExcessFaces ignores the editing-disabled refusal from
        // RemoveFaces; the fork passes it on (WR-658, tangency-labs/smlib 4803d6d). No
        // traversal mark is acquired by that refused inner call.
        let r = exec.remove_excess_faces(&mut kernel, &mut original, &mut result);
        assert!(
            matches!(&r, Err(e) if e.sm_status() == Some(1001)
                && e.to_string().contains("editing is not enabled")),
            "{r:?}"
        );
        assert!(!result.editing_enabled());
        assert_eq!(kernel.context.current_mark(MarkType::Mark), 1);
        result.set_editing_enabled(true);
        crate::geom::context::with_mark_lock(
            &mut kernel,
            crate::geom::context::ALL_MARKS,
            |kernel, mark| {
                assert_eq!(mark, MarkType::Mark);
                exec.remove_excess_faces(kernel, &mut original, &mut result)
                    .unwrap();
                assert!(kernel.context.is_mark_locked(mark));
                assert!(!kernel.context.is_mark_locked(MarkType::Mark2));
                assert_eq!(kernel.context.current_mark(MarkType::Mark2), 3);
            },
        )
        .unwrap();
        assert!(!kernel.context.is_mark_locked(MarkType::Mark));
        assert_eq!(kernel.context.current_mark(MarkType::Mark), 3);
    }

    fn excess_face_fixture() -> (Brep, EdgeId, FaceId, FaceId) {
        let mut result = Brep::new();
        let edgeuse = add_use(&mut result, 0.0, 1.0, 1.0);
        let edge_id = edge_of(&result, edgeuse).unwrap();
        let (first_face, first_use) = add_face(&mut result, plane(0.0));
        let (second_face, second_use) = add_face(&mut result, plane(1.0));
        let other = result
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge_id),
                ..Edgeuse::default()
            })
            .unwrap();
        result.edge_mut(edge_id).unwrap().edgeuses.push(other);
        for (edgeuse, faceuse) in [(edgeuse, first_use), (other, second_use)] {
            let loopuse = result
                .add_loopuse_record(crate::topology::Loopuse {
                    faceuse: Some(faceuse),
                    start: crate::topology::LoopuseStart::Edgeuse(Some(edgeuse)),
                    ..crate::topology::Loopuse::default()
                })
                .unwrap();
            result.faceuse_mut(faceuse).unwrap().loopuses.push(loopuse);
            let record = result.edgeuse_mut(edgeuse).unwrap();
            record.owner = crate::topology::EdgeuseOwner::Loopuse(Some(loopuse));
            record.ccw = Some(edgeuse);
            record.cw = Some(edgeuse);
        }
        (result, edge_id, first_face, second_face)
    }

    fn prepare(
        executive: &mut OffsetExecutive,
        original: &Brep,
        first: EdgeuseId,
        other: EdgeuseId,
    ) -> Result<Option<(EdgeId, SynchronizedEdgeuses)>> {
        let entities = executive
            .offset_brep()
            .map(split_entities)
            .unwrap_or_default();
        let brep_key = executive.offset_brep().map_or(ObjectKey(0), Brep::key);
        split_env(brep_key, &entities, |env, curve_ref| {
            executive.prepare_offset_edgeuses(
                &mut Kernel::default(),
                original,
                first,
                other,
                &mut NotifyState::from_env(env),
                curve_ref,
            )
        })
    }

    #[test]
    fn shared_edge_preparation_preserves_shell_skip_and_validation_order() {
        let (mut original, edge, first_face, other_face) = excess_face_fixture();
        let first = original.edge(edge).unwrap().edgeuses[0];
        let other = original.edge(edge).unwrap().edgeuses[1];
        let mut exec = executive();
        assert_eq!(
            prepare(&mut exec, &original, first, other),
            Ok(Some((edge, SynchronizedEdgeuses::default())))
        );
        exec.set_shell_faces(&[first_face, other_face]);
        let missing = EdgeuseId::from_index(99);
        exec.relate_edgeuses(first, missing);
        exec.set_offset_brep(Brep::new());
        // The selected-face no-op precedes synchronization of the stale mapping.
        assert_eq!(prepare(&mut exec, &original, first, other), Ok(None));
        exec.set_extend_convex_edges(true);
        assert!(matches!(
            prepare(&mut exec, &original, first, other),
            Err(SolidError::NotFound { entity: EntityRef::Edgeuse(id), .. }) if id == missing
        ));
        // A missing first edge is checked before even reading the other face.
        // An edge use with no edge is a defect (the kernel links every edge use to its edge).
        original.edgeuse_mut(first).unwrap().edge = None;
        let r = prepare(&mut exec, &original, first, missing);
        assert_eq!(
            outcome(&r),
            (ErrorKind::StageFailed, Some(SM_ERR_NULL_POINTER), true),
            "{r:?}"
        );
    }

    #[test]
    fn shared_gap_tail_keeps_empty_success_deferred_errors_and_pipe_fallback() {
        let mut kernel = Kernel::default();
        let mut original = Brep::new();
        let original_use = add_use(&mut original, 0.0, 1.0, 1.0);
        let edge = edge_of(&original, original_use).unwrap();
        let mut exec = executive();
        assert_eq!(
            with_state(|state| exec.fill_offset_edgeuse_gaps(
                &mut kernel,
                &original,
                edge,
                &SynchronizedEdgeuses::default(),
                &[],
                state,
            )),
            Ok(())
        );
        assert!(exec.offset_brep().is_none());
        let mut result = Brep::new();
        // evaluates, but has no vertex use and no mate (upstream dereferences them)
        let live = add_use(&mut result, 0.0, 1.0, 1.0);
        let missing = EdgeuseId::from_index(99);
        exec.set_offset_brep(result);
        let fill = |exec: &mut OffsetExecutive, kernel: &mut Kernel, first, second| {
            let synchronized = SynchronizedEdgeuses {
                gaps: vec![first],
                other_gaps: vec![second],
                ..SynchronizedEdgeuses::default()
            };
            with_state(|state| {
                exec.fill_offset_edgeuse_gaps(kernel, &original, edge, &synchronized, &[], state)
            })
        };
        // Both failing gap fills (an edge use that is not there) are problems; both run,
        // then the pipe fallback (CreatePipeSweep): this fixture's offset B-rep has no
        // region to sweep into, a failure upstream returns too, not a missing call, and not
        // p1's; the offset B-rep is handed back
        let r = fill(&mut exec, &mut kernel, missing, missing);
        assert!(
            matches!(&r, Err(e) if !e.is_unported() && !e.is_defect() && e.operation() != OP),
            "{r:?}"
        );
        assert!(exec.offset_brep().is_some());
        // An edge use with no vertex use, met by either fill, is a defect: passed on, never
        // read as a gap left open, so the pipe is not swept
        for (first, second) in [(missing, live), (live, missing)] {
            let r = fill(&mut exec, &mut kernel, first, second);
            assert_eq!(
                outcome(&r),
                (ErrorKind::StageFailed, Some(SM_ERR_NULL_POINTER), true),
                "{r:?}"
            );
            assert!(r.unwrap_err().to_string().contains(EU_NO_VERTEXUSE));
            assert!(exec.offset_brep().is_some());
        }
    }

    #[test]
    fn excess_face_collection_requires_one_solution_and_strict_distance() {
        let (result, edge_id, first_face, second_face) = excess_face_fixture();
        let min_distance = 2.0 * 0.95;
        let mut solution = Solution::<CacheObject>::new();
        solution.start.solution_value = min_distance;
        let mut delete_faces = Vec::new();
        for solutions in [vec![], vec![solution], vec![solution, solution]] {
            collect_excess_faces(
                &result,
                edge_id,
                &solutions,
                min_distance,
                &[],
                &mut delete_faces,
            )
            .unwrap();
            assert!(delete_faces.is_empty());
        }
        for distance in [min_distance + 1.0, f64::NAN] {
            solution.start.solution_value = distance;
            collect_excess_faces(
                &result,
                edge_id,
                &[solution],
                min_distance,
                &[],
                &mut delete_faces,
            )
            .unwrap();
            assert!(delete_faces.is_empty());
        }
        solution.start.solution_value = min_distance - 0.01;
        collect_excess_faces(
            &result,
            edge_id,
            &[solution],
            min_distance,
            &[],
            &mut delete_faces,
        )
        .unwrap();
        assert_eq!(delete_faces, vec![first_face, second_face]);
    }

    #[test]
    fn excess_face_collection_deduplicates_stably_and_skips_shell_incident_edges() {
        let (result, edge_id, first_face, second_face) = excess_face_fixture();
        let mut solution = Solution::<CacheObject>::new();
        solution.start.solution_value = 0.0;
        let mut delete_faces = vec![second_face];
        for _ in 0..2 {
            collect_excess_faces(&result, edge_id, &[solution], 1.0, &[], &mut delete_faces)
                .unwrap();
        }
        assert_eq!(delete_faces, vec![second_face, first_face]);
        delete_faces.clear();
        collect_excess_faces(
            &result,
            edge_id,
            &[solution],
            1.0,
            &[second_face],
            &mut delete_faces,
        )
        .unwrap();
        assert!(delete_faces.is_empty());
        // An inset's negative minimum distance must not be replaced by its magnitude.
        collect_excess_faces(&result, edge_id, &[solution], -0.95, &[], &mut delete_faces).unwrap();
        assert!(delete_faces.is_empty());
    }

    #[test]
    fn original_brep_cache_builds_through_the_brep_box() {
        let kernel = Kernel::default();
        // The real cache constructor computes the B-rep box (query.brep) first.
        assert!(make_original_brep_cache(&kernel, &Brep::new()).is_ok());
    }

    #[test]
    fn original_brep_query_calls_live_solver_and_propagates_its_status() {
        let kernel = Kernel::default();
        let mut original = Brep::new();
        let cache = BrepCache::new(crate::geom::a_object::BREP_TYPE).unwrap();
        let keys = OriginalKeys::new(&original).unwrap();
        let e = solve_original_brep_point(&kernel, &mut original, &cache, &keys, Vec3::ZERO)
            .unwrap_err();
        assert_eq!(
            e.operation(),
            "intersect::topology_solver::p1::TopologyObjects::brep_trees"
        );
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
    }

    #[test]
    fn original_brep_query_keys_records_by_their_geometry_owners() {
        let kernel = Kernel::default();
        let mut original = Brep::new();
        let first_use = add_use(&mut original, 0.0, 1.0, 1.0);
        let second_use = add_use(&mut original, 0.0, 1.0, 1.0);
        let first = edge_of(&original, first_use).unwrap();
        let second = edge_of(&original, second_use).unwrap();
        let custom = ObjectRef {
            key: ObjectKey(900),
            type_code: edge::TYPE,
        };
        let set_owner = |original: &mut Brep, edge_id: EdgeId, owner: ObjectRef| {
            original
                .edge_mut(edge_id)
                .unwrap()
                .curve
                .as_mut()
                .unwrap()
                .set_owner(Some(owner), None);
        };
        // A custom owner is the edge's identity; an unowned edge keeps its arena one.
        set_owner(&mut original, first, custom);
        let (_, keys) = make_original_brep_cache(&kernel, &original).unwrap();
        assert_eq!(keys.key(CacheObject::Edge(first)), custom);
        assert_eq!(keys.key(CacheObject::Curve(first)), custom);
        assert_eq!(
            keys.key(CacheObject::Edge(second)),
            arena_identity(CacheObject::Edge(second))
        );
        // One owner for two edges, or an owner naming another record, is rejected.
        set_owner(&mut original, second, custom);
        assert!(matches!(
            make_original_brep_cache(&kernel, &original),
            Err(SolidError::Unsupported { .. })
        ));
        set_owner(
            &mut original,
            second,
            arena_identity(CacheObject::Edge(first)),
        );
        set_owner(
            &mut original,
            first,
            arena_identity(CacheObject::Edge(second)),
        );
        assert!(make_original_brep_cache(&kernel, &original).is_ok());
        set_owner(&mut original, first, arena_identity(CacheObject::Brep));
        assert!(matches!(
            make_original_brep_cache(&kernel, &original),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn merge_clears_all_relations_and_preserves_upstream_transfer_order() {
        let mut kernel = Kernel::default();
        let mut original = Brep::new();
        for solid in [false, true] {
            let mut executive = executive();
            executive.set_offset_brep(Brep::new());
            executive.set_create_offset_solid(solid);
            executive.relate_edgeuses(EdgeuseId::from_index(1), EdgeuseId::from_index(2));
            // StitchIntoShell and StitchIntoSolid of the empty offset B-rep succeed (the
            // B-rep box is ported, query.brep); the merged B-rep is handed back
            let merged = executive
                .piecewise_merge(&mut kernel, &mut original, None)
                .unwrap();
            assert!(merged.faces().is_empty());
            assert!(executive.offset_brep().is_none());
            assert_eq!(executive.offset_edgeuse(EdgeuseId::from_index(1)), None);
        }
        let mut executive = executive();
        executive.set_merge_results(true);
        executive.set_offset_brep(Brep::new());
        executive.relate_edgeuses(EdgeuseId::from_index(1), EdgeuseId::from_index(2));
        // ShrinkGeometry of the (empty) offset B-rep succeeds; the merge itself needs
        // the caller's notification stores, and the temporary stays
        assert!(matches!(
            executive.piecewise_merge(&mut kernel, &mut original, None),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(executive.offset_brep().is_some());
        assert_eq!(executive.offset_edgeuse(EdgeuseId::from_index(1)), None);
    }

    #[test]
    fn relate_surface_pair_skips_destroyed_faces_without_surfaces() {
        let mut brep = Brep::new();
        let (live_a, _) = add_face(
            &mut brep,
            Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z)),
        );
        let (live_b, _) = add_face(
            &mut brep,
            Surface::from(Plane::from_normal(Vec3::Z, Vec3::Z)),
        );
        let (destroyed, _) = add_face(
            &mut brep,
            Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Y)),
        );
        brep.face_mut(destroyed).unwrap().surface = None;

        let mut no_ssi = Relation::default();
        relate_surface_pair(&brep, &mut no_ssi, destroyed, live_a).unwrap();
        relate_surface_pair(&brep, &mut no_ssi, live_a, FaceId::from_index(999)).unwrap();
        assert!(no_ssi.all_firsts().is_empty());

        relate_surface_pair(&brep, &mut no_ssi, live_a, live_b).unwrap();
        assert_eq!(no_ssi.seconds(live_a), vec![live_b]);
        assert_eq!(no_ssi.seconds(live_b), vec![live_a]);
    }

    fn add_face(brep: &mut Brep, surface: Surface) -> (FaceId, FaceuseId) {
        let face = brep
            .add_face_record(Face {
                surface: Some(surface),
                uv_domain: Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
                ..Face::new(ShellId::from_index(0))
            })
            .unwrap();
        let faceuse = brep
            .add_faceuse_record(Faceuse {
                face: Some(face),
                orientation: OrientType::Same,
                ..Faceuse::default()
            })
            .unwrap();
        brep.face_mut(face).unwrap().faceuse = Some(faceuse);
        (face, faceuse)
    }

    #[test]
    fn uv_boolean_bounds_return_a_failed_conversion() {
        let kernel = Kernel::default();
        // Rust-only error-path input: upstream ConvertTo3D dereferences the missing NURB.
        // The conversion's status is passed on (WR-658, tangency-labs/smlib 4803d6d), so no
        // initialized extent stands in for the box of a curve that was not converted.
        let mut curve = Curve::from(BSplineCurve::new(2));
        let r = uv_curve_bounding_box(&kernel, &mut curve);
        assert!(
            matches!(
                r,
                Err(SolidError::Upstream {
                    operation: "nurbs::b_spline_curve::p1::BSplineCurve::make_nurb",
                    code: 1001,
                    ..
                })
            ),
            "{r:?}"
        );
        // the conversion's side effects before its failure stay
        let CurveKind::BSpline(curve) = curve.kind() else {
            panic!("expected the original B-spline kind");
        };
        assert_eq!(curve.dim(), 3);
    }

    #[test]
    fn uv_boolean_bounds_compute_converted_bspline_and_line_copies() {
        let kernel = Kernel::default();
        let spline = BSplineCurve::create_line_segment(
            3,
            Vec3::new(-2.0, 1.0, 8.0),
            Vec3::new(6.0, 1.0, 9.0),
        )
        .unwrap();
        let line = Line::from_points(Vec3::new(-2.0, 1.0, 0.0), Vec3::new(6.0, 1.0, 0.0), 2);
        for mut curve in [Curve::from(spline), Curve::from(line)] {
            let bbox = uv_curve_bounding_box(&kernel, &mut curve).unwrap();
            assert_eq!(bbox.min(), Vec3::new(-2.0, 1.0, 0.0));
            assert_eq!(bbox.max(), Vec3::new(6.0, 1.0, 0.0));
        }
    }

    fn face_env<R>(
        run: impl FnOnce(&mut NotifyEnv<'_>, &dyn Fn(EntityRef) -> ObjectRef) -> R,
    ) -> R {
        let mut records = topology::NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let key = |_: EntityRef| ObjectRef {
            key: ObjectKey(1),
            type_code: edge::TYPE,
        };
        let mut env = NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key,
        };
        run(&mut env, &|_| ObjectRef {
            key: ObjectKey(2),
            type_code: edge::TYPE,
        })
    }

    fn build_face(
        executive: &mut OffsetExecutive,
        original: &mut Brep,
        faceuse: Option<FaceuseId>,
        offset: &mut Option<Surface>,
    ) -> Result<()> {
        let mut kernel = Kernel::default();
        face_env(|env, offset_key_of| {
            executive.build_offset_of_faceuse(
                &mut kernel,
                original,
                faceuse,
                offset,
                &SsiData::default(),
                env,
                offset_key_of,
                &|_, type_code| ObjectRef {
                    key: ObjectKey(3),
                    type_code,
                },
            )
        })
    }

    #[test]
    fn face_prefix_handles_null_surface_and_selects_domain_constructor() {
        let mut executive = executive();
        let mut original = Brep::new();
        let (_, faceuse) = add_face(&mut original, plane(0.0));
        // NER(pFU) precedes the null-surface no-op.
        assert!(build_face(&mut executive, &mut original, None, &mut None).is_err());
        build_face(&mut executive, &mut original, Some(faceuse), &mut None).unwrap();
        executive.set_offset_brep(Brep::new());
        // A loopless face fails MakeFaceWithCurves (invalid input), whose status
        // upstream ignores: nothing is related, and the face took the surface.
        let mut offset = Some(plane(1.0));
        build_face(&mut executive, &mut original, Some(faceuse), &mut offset).unwrap();
        assert!(offset.is_none());
        assert!(executive.faceuse_to_faces.second(faceuse).is_none());
        let smaller = Surface::from(Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.2, 0.2, 0.8, 0.8),
        ));
        let mut offset = Some(smaller.clone());
        // The 2D-boolean branch: the loopless face gives the planar face no curves,
        // and that MakeFaceWithCurves status is returned (SER) before the boolean,
        // with the offset surface still the caller's.
        match build_face(&mut executive, &mut original, Some(faceuse), &mut offset) {
            Err(error) => assert!(!error.is_unported(), "{error:?}"),
            Ok(()) => panic!("a loopless planar face must fail"),
        }
        assert!(offset.is_some());
        executive.set_operation(OffsetOperation::LocalOperation);
        let mut offset = Some(smaller);
        build_face(&mut executive, &mut original, Some(faceuse), &mut offset).unwrap();
        assert!(offset.is_none());
    }

    #[test]
    fn face_prefix_drops_uv_trim_curve_before_domain_boolean() {
        let mut executive = executive();
        let (mut original, _, face, _) = excess_face_fixture();
        let faceuse = original.face(face).unwrap().faceuse;
        original.face_mut(face).unwrap().uv_domain = Extent2d::from_bounds(-1.0, -1.0, 2.0, 2.0);
        executive.set_offset_brep(Brep::new());
        let mut offset = Some(plane(1.0));
        // Since topology.edgeuse_p3 (#117) builds the UV trim curve, the face prefix
        // runs the plane's DropCurve (wired by intersect.surface_drop_lib_p3, #136)
        // before the domain boolean; the fixture's edge use then has no vertex use to
        // set the curve on.
        match build_face(&mut executive, &mut original, faceuse, &mut offset).unwrap_err() {
            SolidError::InvalidInput { operation, reason } => {
                assert_eq!(operation, "topology::edgeuse::p1");
                assert!(reason.starts_with("SetUVTrimCurve"), "{reason}");
            }
            other => panic!("expected SetUVTrimCurve to fail, got {other:?}"),
        }
    }

    #[test]
    fn mapping_empty_faces_completes_orientation_selection() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let mut original = Brep::new();
        let (_, original_use) = add_face(&mut original, plane(0.0));
        let mut result = Brep::new();
        let (face, _) = add_face(&mut result, plane(1.0));
        executive.set_offset_brep(result);
        face_env(|env, offset_key_of| {
            executive.map_edgeuses(
                &kernel,
                &mut original,
                original_use,
                face,
                env,
                offset_key_of,
            )
        })
        .unwrap();
    }

    #[test]
    fn mapping_kernel_context_is_bound_only_at_notification_boundary() {
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::geom::context::Context;
        use crate::geom::object::NotifyOperation;
        use crate::topology::edge::notification_test_support as callbacks;
        use crate::topology::edge::NotificationStores;
        use crate::topology::topology::Topology;

        let caller = ObjectRef {
            key: ObjectKey(3101),
            type_code: 71,
        };
        let payload = ObjectRef {
            key: ObjectKey(3102),
            type_code: 72,
        };
        let result_key = ObjectRef {
            key: ObjectKey(3103),
            type_code: 73,
        };
        let mut kernel = Kernel::default();
        let kernel_events = callbacks::install(&mut kernel.context);
        let mut external_context = Context::default();
        let external_events = callbacks::install(&mut external_context);
        let mut records = topology::NotifyRecords::new();
        records.insert(caller.key, Topology::default());
        let mut attributes = AttributeStore::new();
        let key_of = |_entity| result_key;
        let notify = |env: &mut NotifyEnv<'_>| {
            if let Some(context) = env.context.as_deref_mut() {
                context.set_doing_boolean(true);
            }
            env.records
                .get_mut(&caller.key)
                .ok_or_else(|| SolidError::invalid_input(OP, "missing test topology"))?
                .set_user_index1(918);
            topology::notify(
                env.records,
                env.attributes,
                env.context.as_deref_mut(),
                NotifyOperation::PreEdit,
                caller,
                Some(payload),
                Some(result_key),
                None,
            )
        };
        // Kernel-bound state routes both callbacks to the actual kernel context.
        {
            let mut stores = NotificationStores {
                records: &mut records,
                attributes: &mut attributes,
                key_of: &key_of,
            };
            let mut state = NotifyState::from_kernel_stores(&mut stores);
            let mut mapping_kernel = MapKernel::Mutable(&mut kernel);
            mapping_kernel.with_env(&mut state, notify).unwrap();
        }
        assert_eq!(records.get(&caller.key).unwrap().user_index1(), 918);
        let event_matches = |events: &std::sync::Arc<std::sync::Mutex<Vec<callbacks::Event>>>| {
            let events = events.lock().unwrap();
            assert_eq!(events.len(), 2);
            for (name, event) in ["user", "system"].into_iter().zip(events.iter()) {
                assert_eq!(event.callback, name);
                assert_eq!(event.operation, NotifyOperation::PreEdit);
                assert_eq!(event.caller, caller);
                assert_eq!(event.data, [Some(payload), Some(result_key), None]);
            }
        };
        event_matches(&kernel_events);
        assert!(external_events.lock().unwrap().is_empty());
        assert!(kernel.context.doing_boolean());

        // Legacy None remains context-free and cannot fall through to Kernel.
        kernel.context.set_doing_boolean(false);
        records.get_mut(&caller.key).unwrap().set_user_index1(0);
        {
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &key_of,
            };
            let mut state = NotifyState::from_env(&mut env);
            let mut mapping_kernel = MapKernel::Read(&kernel);
            mapping_kernel.with_env(&mut state, notify).unwrap();
        }
        assert_eq!(records.get(&caller.key).unwrap().user_index1(), 918);
        assert!(!kernel.context.doing_boolean());
        assert_eq!(kernel_events.lock().unwrap().len(), 2);

        // Legacy Some uses its selected external context, not the supplied Kernel.
        records.get_mut(&caller.key).unwrap().set_user_index1(0);
        {
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: Some(&mut external_context),
                key_of: &key_of,
            };
            let mut state = NotifyState::from_env(&mut env);
            let mut mapping_kernel = MapKernel::Read(&kernel);
            mapping_kernel.with_env(&mut state, notify).unwrap();
        }
        assert_eq!(records.get(&caller.key).unwrap().user_index1(), 918);
        assert!(!kernel.context.doing_boolean());
        event_matches(&external_events);
    }

    fn strut_vertex_fixture() -> (Brep, VertexId, FaceuseId) {
        let mut original = Brep::new();
        let vertex = original.add_vertex_record(Vertex::default()).unwrap();
        let (_, faceuse) = add_face(&mut original, plane(0.0));
        let edgeuse = add_use(&mut original, 0.0, 1.0, 1.0);
        let vertexuse = original
            .add_vertexuse_record(Vertexuse {
                vertex: Some(vertex),
                owner: crate::topology::VertexuseOwner::Edgeuse(Some(edgeuse)),
                ..Vertexuse::default()
            })
            .unwrap();
        original
            .vertex_mut(vertex)
            .unwrap()
            .vertexuses
            .push(vertexuse);
        let loopuse = original
            .add_loopuse_record(crate::topology::Loopuse {
                faceuse: Some(faceuse),
                start: crate::topology::LoopuseStart::Edgeuse(Some(edgeuse)),
                ..crate::topology::Loopuse::default()
            })
            .unwrap();
        original
            .faceuse_mut(faceuse)
            .unwrap()
            .loopuses
            .push(loopuse);
        let record = original.edgeuse_mut(edgeuse).unwrap();
        record.owner = crate::topology::EdgeuseOwner::Loopuse(Some(loopuse));
        record.vertexuse = Some(vertexuse);
        record.ccw = Some(edgeuse);
        record.cw = Some(edgeuse);
        record.orientation = OrientType::Same;
        assert!(eu::is_strut(&original, edgeuse).unwrap());
        (original, vertex, faceuse)
    }

    #[test]
    fn vertex_strut_missing_or_unmapped_mate_returns_empty_geometry() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let (mut original, vertex, faceuse) = strut_vertex_fixture();
        let (_, mate) = add_face(&mut original, plane(0.0));
        executive.set_offset_brep(Brep::new());
        for mate in [None, Some(mate)] {
            original.faceuse_mut(faceuse).unwrap().mate = mate;
            let offset = executive
                .build_offset_of_vertex(&kernel, &original, Some(vertex))
                .unwrap();
            assert!(offset.surface.is_none());
            assert!(offset.trim_curves.is_empty());
            assert!(offset.curve_orientations.is_empty());
        }
    }

    #[test]
    fn vertex_strut_mate_mapping_builds_geometry_and_preserves_primary_precedence() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let (mut original, vertex, faceuse) = strut_vertex_fixture();
        let (_, mate) = add_face(&mut original, plane(0.0));
        original.faceuse_mut(faceuse).unwrap().mate = Some(mate);
        let (mut result, edge, cap_face, _) = excess_face_fixture();
        result.set_tolerance(1.0e-6);
        let curve = Curve::from(
            BSplineCurve::create_circle_segment(
                3,
                &Axis2Placement::default(),
                executive.offset_distance() * 0.5,
                0.0,
                90.0,
                NurbCircleParam::Quadratic,
            )
            .unwrap(),
        );
        assert!(curve
            .is_arc(&kernel, 10, result.tolerance())
            .unwrap()
            .is_some());
        result.edge_mut(edge).unwrap().curve = Some(curve);
        let (empty_face, _) = add_face(&mut result, plane(0.0));
        executive.set_offset_brep(result);
        executive.relate_faceuse(mate, cap_face);
        let cap = executive
            .build_offset_of_vertex(&kernel, &original, Some(vertex))
            .unwrap();
        assert!(matches!(
            cap.surface.unwrap().kind(),
            SurfaceKind::BSpline(_)
        ));
        assert!(cap.trim_curves.is_empty());
        assert!(cap.curve_orientations.is_empty());
        executive.relate_faceuse(faceuse, empty_face);
        let offset = executive
            .build_offset_of_vertex(&kernel, &original, Some(vertex))
            .unwrap();
        assert!(offset.surface.is_none());
        assert!(offset.trim_curves.is_empty());
        assert!(offset.curve_orientations.is_empty());
    }

    #[test]
    fn vertex_null_is_rejected_and_isolated_vertex_uses_the_source_arena() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let mut original = Brep::new();
        let point = Vec3::new(1.0, -2.0, 3.0);
        original.set_tolerance(1.0e-6);
        let vertex = original
            .add_vertex_record(Vertex {
                point,
                ..Vertex::default()
            })
            .unwrap();
        let mut result = Brep::new();
        result.set_tolerance(1.0e-3);
        let result_vertex = result
            .add_vertex_record(Vertex {
                point: Vec3::new(20.0, 30.0, 40.0),
                ..Vertex::default()
            })
            .unwrap();
        assert_eq!(vertex, result_vertex);
        executive.set_offset_brep(result);
        assert!(executive
            .build_offset_of_vertex(&kernel, &original, None)
            .is_err());
        let cap = executive
            .build_offset_of_vertex(&kernel, &original, Some(vertex))
            .unwrap();
        let surface = cap.surface.unwrap();
        assert!(matches!(surface.kind(), SurfaceKind::BSpline(_)));
        assert!(cap.trim_curves.is_empty());
        assert!(cap.curve_orientations.is_empty());
        let domain = surface.natural_uv_domain_if_built();
        for (u, v) in [(0.17, 0.29), (0.43, 0.61), (0.83, 0.79)] {
            let uv = crate::math::Point2::new(
                (1.0 - u) * domain.min().x + u * domain.max().x,
                (1.0 - v) * domain.min().y + v * domain.max().y,
            );
            let sample = surface.evaluate_point(&kernel, uv).unwrap();
            assert!((sample.distance(point) - executive.offset_distance()).abs() < 1.0e-12);
        }
        assert_eq!(original.tolerance(), 1.0e-6);
        assert_eq!(executive.offset_brep().unwrap().tolerance(), 1.0e-3);
        with_state(|state| {
            executive.fill_gap_edgeuses_with_state(
                &mut Kernel::default(),
                EdgeId::from_index(u32::MAX),
                &[],
                &[],
                state,
            )
        })
        .unwrap();
    }

    #[test]
    fn notify_copies_edge_and_distinct_vertex_records_without_cross_arena_aliases() {
        let mut original = Brep::new();
        let mut result = Brep::new();
        let old = add_use(&mut original, 0.0, 1.0, 1.0);
        let new = add_use(&mut result, 0.0, 1.0, 1.0);
        // Different primary-use orientations must not swap endpoint metadata.
        for (brep, edgeuse, orientation) in [
            (&mut original, old, OrientType::Opposite),
            (&mut result, new, OrientType::Same),
        ] {
            let v1 = brep.add_vertex_record(Vertex::default()).unwrap();
            let v2 = brep.add_vertex_record(Vertex::default()).unwrap();
            let vu1 = brep
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(v1),
                    ..Vertexuse::default()
                })
                .unwrap();
            let vu2 = brep
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(v2),
                    ..Vertexuse::default()
                })
                .unwrap();
            let edge = edge_of(brep, edgeuse).unwrap();
            brep.edgeuse_mut(edgeuse).unwrap().vertexuse = Some(vu1);
            brep.edgeuse_mut(edgeuse).unwrap().orientation = orientation;
            let mate = brep
                .add_edgeuse_record(Edgeuse {
                    edge: Some(edge),
                    vertexuse: Some(vu2),
                    orientation: if orientation == OrientType::Same {
                        OrientType::Opposite
                    } else {
                        OrientType::Same
                    },
                    ..Edgeuse::default()
                })
                .unwrap();
            brep.edge_mut(edge).unwrap().edgeuses.push(mate);
            brep.edge_mut(edge).unwrap().vertices = if orientation == OrientType::Same {
                [v1, v2]
            } else {
                [v2, v1]
            };
        }
        let key = |entity| match entity {
            EntityRef::Edge(id) => ObjectRef {
                key: ObjectKey(10 + u64::from(id.index())),
                type_code: edge::TYPE,
            },
            EntityRef::Vertex(id) => ObjectRef {
                key: ObjectKey(20 + u64::from(id.index())),
                type_code: vertex::TYPE,
            },
            _ => panic!("unexpected notified entity"),
        };
        let offset_key = |entity| {
            let mut value = key(entity);
            value.key.0 += 100;
            value
        };
        let mut records = topology::NotifyRecords::new();
        for (entity, metadata) in [
            (EntityRef::Edge(EdgeId::from_index(0)), 37),
            (EntityRef::Vertex(VertexId::from_index(0)), 11),
            (EntityRef::Vertex(VertexId::from_index(1)), 53),
        ] {
            let mut record = topology::Topology::default();
            record.set_user_index1(metadata);
            records.insert(key(entity).key, record);
            records.insert(offset_key(entity).key, topology::Topology::default());
        }
        let mut attributes = AttributeStore::default();
        let mut env = NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &key,
        };
        notify_edge_and_vertices(
            &original,
            EdgeId::from_index(0),
            &result,
            EdgeId::from_index(0),
            &mut env,
            &offset_key,
        )
        .unwrap();
        // The public helper must still use these actual records and distinct
        // arena keys before EdgeuseOffset dereferences the wire use's null face.
        for entity in [
            EntityRef::Edge(EdgeId::from_index(0)),
            EntityRef::Vertex(VertexId::from_index(0)),
            EntityRef::Vertex(VertexId::from_index(1)),
        ] {
            env.records
                .insert(offset_key(entity).key, topology::Topology::default());
        }
        let mut exec = executive();
        exec.set_offset_brep(result);
        exec.relate_edgeuses(old, new);
        let status = exec.build_offset_of_edgeuses(
            &mut Kernel::default(),
            &mut original,
            old,
            old,
            MarkType::Mark,
            &mut env,
            &offset_key,
            &|edge, kind| ObjectRef {
                key: ObjectKey(1000 + u64::from(edge.index())),
                type_code: kind,
            },
        );
        assert!(
            matches!(&status, Err(SolidError::InvalidInput { reason, .. }) if *reason == "edge use without face"),
            "{status:?}"
        );
        for (entity, expected) in [
            (EntityRef::Edge(EdgeId::from_index(0)), 37),
            (EntityRef::Vertex(VertexId::from_index(0)), 53),
            (EntityRef::Vertex(VertexId::from_index(1)), 11),
        ] {
            assert_eq!(
                records.get(&offset_key(entity).key).unwrap().user_index1(),
                expected
            );
        }
        assert_eq!(
            edge::vertex(&original, EdgeId::from_index(0)).unwrap(),
            Some(VertexId::from_index(0))
        );
        assert_eq!(
            edge::start_vertex(&original, EdgeId::from_index(0)).unwrap(),
            Some(VertexId::from_index(1))
        );
    }

    /// tangency-labs/solidcore#974: an inset (`SM_OT_OPPOSITE`) face use whose face does
    /// not fit its offset surface's domain is clipped by a 2D boolean of planar faces made
    /// from its UV loops. Taken in the face use's sense the loops run clockwise, the planar
    /// face is inside out and the clip empty: the face gets no offset (upstream). The loops
    /// are taken counterclockwise and the clipped face is made.
    #[test]
    #[ignore = "fixture does not reproduce the inside-out clip: on this create_plane sheet \
                upstream's path also clipped to one offset face; needs the shell/4567 vase's \
                inset bottom"]
    fn issue_974_inset_face_clipped_right_side_out() {
        use crate::primitive::api_primitives::create_plane;
        let offsets = || -> usize {
            let mut kernel = Kernel::default();
            let mut original = create_plane(&mut kernel, Vec3::ZERO, 10.0, 10.0).unwrap();
            let inset = original
                .faceuses()
                .iter()
                .find(|(_, fu)| fu.orientation == OrientType::Opposite)
                .map(|(id, _)| id)
                .unwrap();
            let mut executive = executive();
            executive.set_offset_brep(Brep::new_with_infinite_region().unwrap());
            // an offset plane 1 below the face, parametrised as the face's surface, whose
            // domain holds only a band of the face's UV domain
            let face = original.faceuse(inset).unwrap().face.unwrap();
            let (domain, at) = {
                let f = original.face(face).unwrap();
                let s = f.surface.as_ref().unwrap();
                let at = |u: f64, v: f64| s.evaluate_point(&kernel, Vec2::new(u, v)).unwrap();
                (f.uv_domain, [at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0)])
            };
            let [p00, p10, p01] = at;
            let (du, dv) = (p10 - p00, p01 - p00);
            let (u, v) = (domain.u_interval(), domain.v_interval());
            let mut offset = Some(Surface::from(Plane::new(
                p00 + Vec3::new(0.0, 0.0, -1.0),
                du.unitize_or_z(),
                dv.unitize_or_z(),
                Vec2::new(du.length(), dv.length()),
                &Extent2d::from_bounds(
                    u.min() + 0.2 * u.length(),
                    v.min() - 0.1 * v.length(),
                    u.min() + 0.8 * u.length(),
                    v.max() + 0.1 * v.length(),
                ),
            )));
            face_env(|env, offset_key_of| {
                executive.build_offset_of_faceuse(
                    &mut kernel,
                    &mut original,
                    Some(inset),
                    &mut offset,
                    &SsiData::default(),
                    env,
                    offset_key_of,
                    &|_, type_code| ObjectRef {
                        key: ObjectKey(3),
                        type_code,
                    },
                )
            })
            .unwrap();
            executive.offset_faces(inset).len()
        };
        assert!(offsets() >= 1);
    }

    /// `keep_source_tolerances` pairs each end of a matched edge with the original vertex
    /// it stands for, whichever way the matched uses run (tangency-labs/solidcraft#17624):
    /// a box's copy, matched once by the same edge use and once by its mate (the same edge
    /// run the other way), gives the loose start vertex's tolerance to that vertex only.
    #[test]
    fn keep_source_tolerances_pairs_ends_either_way() {
        let mut k = Kernel::default();
        let mut original = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let e = original.edges().ids().next().unwrap();
        let u = edge::edgeuses(&original, e, None, None).unwrap()[0];
        let start = use_vertex(&original, u).unwrap();
        let end = use_vertex(&original, eu::mate(&original, u).unwrap().unwrap()).unwrap();
        vertex::set_tolerance(&mut original, start, 1e-3, true, true).unwrap();
        let thin = vertex::tolerance(&original, end).unwrap();
        assert!(thin < 1e-3);
        for reversed in [false, true] {
            let mut result = original.clone_standing_for_self().unwrap();
            for v in [start, end] {
                result.vertex_mut(v).unwrap().tolerance = thin;
            }
            let matched = if reversed {
                eu::mate(&result, u).unwrap().unwrap()
            } else {
                u
            };
            keep_source_tolerances(&k, &original, u, e, &mut result, matched, e, 1e-6).unwrap();
            assert_eq!(
                vertex::tolerance(&result, start).unwrap(),
                1e-3,
                "{reversed}"
            );
            assert_eq!(vertex::tolerance(&result, end).unwrap(), thin, "{reversed}");
        }
    }

    /// `original` copied with every edge and vertex at `thin`, its edge `e` split at the
    /// middle of its interval: the copy, each piece with its use on `u`'s faceuse, and
    /// the split vertex.
    fn split_copy(
        k: &mut Kernel,
        original: &Brep,
        e: EdgeId,
        u: EdgeuseId,
        thin: f64,
    ) -> (Brep, Vec<(EdgeId, EdgeuseId)>, VertexId) {
        let mut result = original.clone_standing_for_self().unwrap();
        for v in result.vertices().ids().collect::<Vec<_>>() {
            result.vertex_mut(v).unwrap().tolerance = thin;
        }
        for x in result.edges().ids().collect::<Vec<_>>() {
            result.edge_mut(x).unwrap().tolerance = thin;
        }
        result.set_editing_enabled(true);
        let at = edge::interval(&result, e).unwrap().evaluate(0.5);
        let (a, z, split) =
            crate::heal::stitch::p1::make_vertex_split_edge(k, &mut result, e, at).unwrap();
        let on = eu::faceuse(original, u).unwrap();
        let pieces = [a.unwrap(), z.unwrap()]
            .into_iter()
            .map(|piece| {
                let uses = edge::edgeuses(&result, piece, None, None).unwrap();
                let use_on = uses
                    .into_iter()
                    .find(|&w| eu::faceuse(&result, w).unwrap() == on)
                    .unwrap();
                (piece, use_on)
            })
            .collect();
        (result, pieces, split.unwrap())
    }

    /// `MapEdgeuses`'s UV tolerance at the middle of `u` (`EFF_ZERO_SQRT * (1 + |uv|)`).
    fn uv_tol_at(k: &Kernel, b: &Brep, u: EdgeuseId) -> f64 {
        let mid = eu3::normalized_evaluate_read_only(k, b, u, 0.5, true, false)
            .unwrap()
            .point;
        EFF_ZERO_SQRT * (1.0 + mid.max_dimension())
    }

    /// A matched piece of a split original edge (`MapEdgeuses`'s drop fallback) takes
    /// the edge's tolerance, and its end on an original end that end's tolerance, run
    /// either way; the split vertex the original does not have takes no end's (only the
    /// edge's, which raising an edge gives its ends). A box's straight edge, ends loosened
    /// to 1e-3 and 2e-3 and the edge to 5e-4: pairing ends by UV tangents (one line's,
    /// parallel) gave the split vertex the far end's tolerance.
    #[test]
    fn a_split_piece_keeps_its_original_ends_tolerances_only() {
        let mut k = Kernel::default();
        let mut original = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let e = original.edges().ids().next().unwrap();
        let u = edge::edgeuses(&original, e, None, None).unwrap()[0];
        let start = use_vertex(&original, u).unwrap();
        let end = use_vertex(&original, eu::mate(&original, u).unwrap().unwrap()).unwrap();
        vertex::set_tolerance(&mut original, start, 1e-3, true, true).unwrap();
        vertex::set_tolerance(&mut original, end, 2e-3, true, true).unwrap();
        original.edge_mut(e).unwrap().tolerance = 5e-4;
        let thin = 1e-5;
        for reversed in [false, true] {
            let (mut result, pieces, split) = split_copy(&mut k, &original, e, u, thin);
            assert_eq!(pieces.len(), 2);
            for (piece, use_on) in pieces {
                let matched = if reversed {
                    eu::mate(&result, use_on).unwrap().unwrap()
                } else {
                    use_on
                };
                let tol = uv_tol_at(&k, &result, matched);
                keep_source_tolerances(&k, &original, u, e, &mut result, matched, piece, tol)
                    .unwrap();
                assert_eq!(edge::tolerance(&result, piece).unwrap(), 5e-4, "{reversed}");
            }
            assert_eq!(
                vertex::tolerance(&result, start).unwrap(),
                1e-3,
                "{reversed}"
            );
            assert_eq!(vertex::tolerance(&result, end).unwrap(), 2e-3, "{reversed}");
            // only the edge's, which an edge's tolerance gives its ends
            assert_eq!(
                vertex::tolerance(&result, split).unwrap(),
                5e-4,
                "{reversed}"
            );
        }
    }

    /// A piece of a split circle: a cylinder's rim, one closed edge on one vertex
    /// (loosened to 1e-3, the edge to 5e-4), split in half. Each half takes the edge's
    /// tolerance and its end on the rim's vertex that vertex's; the split vertex takes only
    /// the edge's. Pairing ends by UV tangents failed both ways: on the cap, where the
    /// rim's UV curve is a circle, each half's tangent at its middle is square to the whole
    /// rim's, so nothing was raised, not even the edge; on the side, where it is a line,
    /// the split vertex took the rim vertex's tolerance.
    #[test]
    fn a_split_circle_piece_keeps_the_rims_tolerances() {
        let mut k = Kernel::default();
        let mut original = crate::primitive::api_primitives::create_cylinder(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            5.0,
            10.0,
        )
        .unwrap();
        let rim = original
            .edges()
            .ids()
            .find(|&x| {
                let ends = edge::vertices(&original, x, None).unwrap();
                ends.len() == 1
            })
            .unwrap();
        let ring = edge::vertices(&original, rim, None).unwrap()[0].unwrap();
        vertex::set_tolerance(&mut original, ring, 1e-3, true, true).unwrap();
        original.edge_mut(rim).unwrap().tolerance = 5e-4;
        let thin = 1e-5;
        // the rim's use on the cylinder's side (its UV ends are the seam's two sides)
        // and on the cap (one UV point)
        for u in edge::edgeuses(&original, rim, None, None).unwrap() {
            let (mut result, pieces, split) = split_copy(&mut k, &original, rim, u, thin);
            assert_eq!(pieces.len(), 2);
            for (piece, use_on) in pieces {
                let tol = uv_tol_at(&k, &result, use_on);
                keep_source_tolerances(&k, &original, u, rim, &mut result, use_on, piece, tol)
                    .unwrap();
                assert_eq!(edge::tolerance(&result, piece).unwrap(), 5e-4, "{u:?}");
            }
            assert_eq!(vertex::tolerance(&result, ring).unwrap(), 1e-3, "{u:?}");
            assert_eq!(vertex::tolerance(&result, split).unwrap(), 5e-4, "{u:?}");
        }
    }

    /// `MapEdgeuses` pairs ends at its base UV tolerance, not at the scaled one it matched
    /// the uses with (tangency-labs/solidcraft#18010). A box's edge, start loosened to 2e-4,
    /// end to 1e-4, the edge to 5e-5, is split in the copy 40 base tolerances before its
    /// end: the long piece, met first round the face, has its middle 20 base tolerances from
    /// the edge's, so scales 1 and 10 miss it and it matches at 100, which the split end is
    /// also within. The split end, farther from the end in space than the end's tolerance,
    /// takes only the edge's tolerance; the piece's start still takes the start's.
    #[test]
    fn a_piece_matched_at_a_scaled_tolerance_pairs_ends_at_the_base_one() {
        let mut k = Kernel::default();
        let mut original = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let e = original.edges().ids().next().unwrap();
        let u = edge::edgeuses(&original, e, None, None).unwrap()[0];
        let start = use_vertex(&original, u).unwrap();
        let end = use_vertex(&original, eu::mate(&original, u).unwrap().unwrap()).unwrap();
        vertex::set_tolerance(&mut original, start, 2e-4, true, true).unwrap();
        vertex::set_tolerance(&mut original, end, 1e-4, true, true).unwrap();
        original.edge_mut(e).unwrap().tolerance = 5e-5;
        let uv = |k: &Kernel, b: &Brep, w: EdgeuseId, t: f64| {
            eu3::normalized_evaluate_read_only(k, b, w, t, true, false)
                .unwrap()
                .point
        };
        let base = uv_tol_at(&k, &original, u);
        let (uv_start, uv_end) = (uv(&k, &original, u, 0.0), uv(&k, &original, u, 1.0));
        let mut result = original.clone_standing_for_self().unwrap();
        for v in result.vertices().ids().collect::<Vec<_>>() {
            result.vertex_mut(v).unwrap().tolerance = 1e-5;
        }
        for x in result.edges().ids().collect::<Vec<_>>() {
            result.edge_mut(x).unwrap().tolerance = 1e-5;
        }
        result.set_editing_enabled(true);
        let interval = edge::interval(&result, e).unwrap();
        let frac = 40.0 * base / uv_start.distance(uv_end);
        // the edge's parameter runs with its use's (from its start) or against it
        let forward = uv(&k, &result, u, 0.0).distance(uv_start) < base;
        let at = interval.evaluate(if forward { 1.0 - frac } else { frac });
        let (a, z, split) =
            crate::heal::stitch::p1::make_vertex_split_edge(&mut k, &mut result, e, at).unwrap();
        let split = split.unwrap();
        let on = eu::faceuse(&original, u).unwrap();
        let use_on = |b: &Brep, piece: EdgeId| {
            edge::edgeuses(b, piece, None, None)
                .unwrap()
                .into_iter()
                .find(|&w| eu::faceuse(b, w).unwrap() == on)
                .unwrap()
        };
        let long = [a.unwrap(), z.unwrap()]
            .into_iter()
            .max_by(|&p, &q| {
                let len = |x| {
                    uv(&k, &result, use_on(&result, x), 0.0).distance(uv(
                        &k,
                        &result,
                        use_on(&result, x),
                        1.0,
                    ))
                };
                len(p).total_cmp(&len(q))
            })
            .unwrap();
        let long_use = use_on(&result, long);
        // the split end lies between the base and the scaled tolerance of the end in UV, and
        // farther from it in space than the end's tolerance
        let split_point = vertex::point(&result, split).unwrap();
        let split_end_uv = [
            uv(&k, &result, long_use, 0.0),
            uv(&k, &result, long_use, 1.0),
        ]
        .into_iter()
        .min_by(|p, q| p.distance(uv_end).total_cmp(&q.distance(uv_end)))
        .unwrap();
        let off = split_end_uv.distance(uv_end);
        assert!(off > base && off < 100.0 * base, "{off} {base}");
        let end_point = vertex::point(&original, end).unwrap();
        assert!(split_point.distance(end_point) > 1e-4);
        // the long piece's middle misses scales 1 and 10
        let piece_tol = uv_tol_at(&k, &result, long_use);
        let mid_off = uv(&k, &result, long_use, 0.5).distance(uv(&k, &original, u, 0.5));
        assert!(mid_off > 10.0 * piece_tol && mid_off < 100.0 * piece_tol);
        let face = eu::face(&original, u).unwrap().unwrap();
        let mut executive = executive();
        executive.set_offset_brep(result);
        face_env(|env, offset_key_of| {
            executive.map_edgeuses(&k, &mut original, on.unwrap(), face, env, offset_key_of)
        })
        .unwrap();
        let result = executive.required_offset_brep().unwrap();
        assert_eq!(edge::tolerance(result, long).unwrap(), 5e-5);
        assert_eq!(vertex::tolerance(result, start).unwrap(), 2e-4);
        // only the edge's, which raising an edge gives its ends
        assert_eq!(vertex::tolerance(result, split).unwrap(), 5e-5);
    }

    /// A matched use whose UV curve stops at its middle (a cubic with control points
    /// at 0, 1, 0, 1 of its chord: monotone, its derivative `3(1 - 2t)^2` zero at 0.5) is
    /// paired by its ends like any other; reading its UV tangent there raised a
    /// `PortInvariant` that aborted the whole offset. A NaN tolerance is near no end: the
    /// edge is raised and the vertices are not.
    #[test]
    fn a_use_stopping_at_its_middle_keeps_source_tolerances() {
        use crate::nurbs::math::CPoint;
        let mut k = Kernel::default();
        let mut original = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let e = original.edges().ids().next().unwrap();
        let u = edge::edgeuses(&original, e, None, None).unwrap()[0];
        let start = use_vertex(&original, u).unwrap();
        let end = use_vertex(&original, eu::mate(&original, u).unwrap().unwrap()).unwrap();
        vertex::set_tolerance(&mut original, start, 1e-3, true, true).unwrap();
        original.edge_mut(e).unwrap().tolerance = 5e-4;
        let fresh = |original: &Brep| {
            let mut result = original.clone_standing_for_self().unwrap();
            for v in [start, end] {
                result.vertex_mut(v).unwrap().tolerance = 1e-5;
            }
            result.edge_mut(e).unwrap().tolerance = 1e-5;
            result
        };
        let mut result = fresh(&original);
        let uv = |b: &Brep, t: f64| {
            eu3::normalized_evaluate_read_only(&k, b, u, t, true, false)
                .unwrap()
                .point
        };
        let (a, z) = (uv(&result, 0.0), uv(&result, 1.0));
        let stalling = |from: Vec3, to: Vec3| {
            let pts: Vec<CPoint> = [0.0, 1.0, 0.0, 1.0]
                .into_iter()
                .map(|s| {
                    let p = from + (to - from) * s;
                    CPoint::new(p.x, p.y, 0.0, 1.0)
                })
                .collect();
            Curve::from(
                BSplineCurve::from_data(2, 3, 7, 3, &pts, &[0., 0., 0., 0., 1., 1., 1., 1.])
                    .unwrap(),
            )
        };
        result.edgeuse_mut(u).unwrap().uv_trim_curve = Some(stalling(a, z));
        if uv(&result, 0.0).distance(a) > 1e-12 {
            result.edgeuse_mut(u).unwrap().uv_trim_curve = Some(stalling(z, a));
        }
        assert!(uv(&result, 0.0).distance(a) <= 1e-12);
        assert!(uv(&result, 1.0).distance(z) <= 1e-12);
        let stalled = eu3::normalized_evaluate_read_only(&k, &result, u, 0.5, true, true)
            .unwrap()
            .first_deriv
            .unwrap();
        assert_eq!(stalled.length(), 0.0);
        keep_source_tolerances(&k, &original, u, e, &mut result, u, e, 1e-6).unwrap();
        assert_eq!(edge::tolerance(&result, e).unwrap(), 5e-4);
        assert_eq!(vertex::tolerance(&result, start).unwrap(), 1e-3);
        assert_eq!(vertex::tolerance(&result, end).unwrap(), 5e-4);
        let mut result = fresh(&original);
        keep_source_tolerances(&k, &original, u, e, &mut result, u, e, f64::NAN).unwrap();
        assert_eq!(edge::tolerance(&result, e).unwrap(), 5e-4);
        assert_eq!(vertex::tolerance(&result, start).unwrap(), 5e-4);
    }

    /// A box whose top is given a plane moved up 1, tilted 2 degrees (so not parallel:
    /// the given frame is kept as it came) and turned `deg` degrees in its own
    /// parameters, after `loosen` has loosened the box; the local operation's result.
    fn turned_top(deg: f64, loosen: impl FnOnce(&mut Brep)) -> (Brep, Result<()>) {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        loosen(&mut b);
        let top = bp1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = face::surface(&b, f).unwrap().unwrap();
                let mid = s
                    .natural_uv_domain_with_nurb(&k)
                    .unwrap()
                    .evaluate(0.5, 0.5);
                (s.evaluate_point(&k, mid).unwrap().z - 10.0).abs() < 1e-9
            })
            .unwrap();
        let own = face::surface(&b, top).unwrap().unwrap().clone();
        let SurfaceKind::Plane(plane) = own.kind() else {
            panic!("a box's top is a plane");
        };
        let q = *plane.position();
        let (s, c) = (deg * PI / 180.0).sin_cos();
        let (x, y) = (
            q.x_axis() * c + q.y_axis() * s,
            q.y_axis() * c - q.x_axis() * s,
        );
        let (ts, tc) = (2.0 * PI / 180.0).sin_cos();
        let tilt = |v: Vec3| Vec3::new(v.x, tc * v.y - ts * v.z, ts * v.y + tc * v.z);
        let given = Surface::from(Plane::new(
            q.origin() + Vec3::Z,
            tilt(x),
            tilt(y),
            plane.uv_scale(),
            &own.natural_uv_domain_with_nurb(&k).unwrap(),
        ));
        let done = crate::offset::offset_executive::p2::local_operation_simple(
            &mut k,
            &mut b,
            &[top],
            vec![given],
        );
        (b, done)
    }

    /// The vertex of `b` at `(x, y)`, on the top (z > 5) or the bottom.
    fn vertex_at(b: &Brep, x: f64, y: f64, top: bool) -> VertexId {
        b.vertices()
            .ids()
            .find(|&v| {
                let p = vertex::point(b, v).unwrap();
                (p.x - x).abs() < 1e-6 && (p.y - y).abs() < 1e-6 && (p.z > 5.0) == top
            })
            .unwrap()
    }

    /// The edge of `b` between `v` and `w`.
    fn edge_between(b: &Brep, v: VertexId, w: VertexId) -> EdgeId {
        b.edges()
            .ids()
            .find(|&e| {
                let mut ends = edge::vertices(b, e, None).unwrap();
                ends.sort();
                let mut want = vec![Some(v), Some(w)];
                want.sort();
                ends == want
            })
            .unwrap()
    }

    /// A local operation given a top plane turned in its own parameters keeps the source
    /// tolerances on the images of their sources (WR-671). The face takes its UV curves to
    /// the given surface, so its edges are made where that surface carries them: at 30
    /// degrees the top is the face's square turned 30 degrees about the corner at the
    /// origin, and the box's top edge on y = 0 is the top edge from that corner along
    /// y = 0. Loosened one at a time (1e-3; an edge's tolerance grows its ends too):
    /// the corner's image alone takes the corner's tolerance, and the loose edge's image
    /// takes the edge's while the top edges at y = 10 stay thin.
    #[test]
    fn a_turned_top_keeps_source_tolerances_on_its_images() {
        for deg in [0.0, 30.0] {
            let (b, done) = turned_top(deg, |b| {
                let corner = vertex_at(b, 0.0, 0.0, true);
                vertex::set_tolerance(b, corner, 1e-3, true, true).unwrap();
            });
            done.unwrap_or_else(|e| panic!("{deg}: {e}"));
            let corner = vertex_at(&b, 0.0, 0.0, true);
            assert!((vertex::point(&b, corner).unwrap().z - 11.0).abs() < 1e-9);
            for v in b.vertices().ids() {
                let t = vertex::tolerance(&b, v).unwrap();
                assert_eq!(t >= 1e-3, v == corner, "{deg}: {v:?} {t}");
            }
            let (b, done) = turned_top(deg, |b| {
                let (corner, other) = (vertex_at(b, 0.0, 0.0, true), vertex_at(b, 10.0, 0.0, true));
                let e = edge_between(b, corner, other);
                edge::set_tolerance(b, e, 1e-3, true, true).unwrap();
            });
            done.unwrap_or_else(|e| panic!("{deg}: {e}"));
            let top_edges_at = |y: f64| -> Vec<EdgeId> {
                b.edges()
                    .ids()
                    .filter(|&e| {
                        let ends = edge::vertices(&b, e, None).unwrap();
                        let points: Vec<_> = ends
                            .iter()
                            .map(|v| vertex::point(&b, v.unwrap()).unwrap())
                            .collect();
                        points.iter().all(|p| p.z > 5.0)
                            && points.iter().any(|p| (p.y - y).abs() < 1e-6)
                    })
                    .collect()
            };
            let corner = vertex_at(&b, 0.0, 0.0, true);
            let image = top_edges_at(0.0)
                .into_iter()
                .find(|&e| {
                    let ends = edge::vertices(&b, e, None).unwrap();
                    ends.contains(&Some(corner))
                        && ends
                            .iter()
                            .all(|v| vertex::point(&b, v.unwrap()).unwrap().y.abs() < 1e-6)
                })
                .unwrap();
            assert!(edge::tolerance(&b, image).unwrap() >= 1e-3, "{deg}");
            let far = top_edges_at(10.0);
            assert!(!far.is_empty());
            for e in far {
                let t = edge::tolerance(&b, e).unwrap();
                assert!(t < 1e-3, "{deg}: {e:?} {t}");
            }
        }
    }

    /// The top turned a quarter turn: the face's square lands beside the box and the body
    /// built is refused as unsound, typed, with the box unchanged. Pairing the matched
    /// edges' ends by their 3D tangents, square to each other here, raised a
    /// `PortInvariant` instead.
    #[test]
    fn a_quarter_turned_top_is_refused_typed_not_a_port_invariant() {
        let (b, done) = turned_top(90.0, |_| ());
        let e = done.unwrap_err();
        assert!(!matches!(e, SolidError::PortInvariant { .. }), "{e}");
        assert!(
            matches!(
                e.failure().map(|f| &f.reason),
                Some(crate::error::Reason::Offset(OffsetFailure::Unsound { .. }))
            ),
            "{e}"
        );
        assert_eq!(
            (b.faces().len(), b.edges().len(), b.vertices().len()),
            (6, 12, 8)
        );
    }
}
