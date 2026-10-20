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
//! | [`OffsetExecutive::map_edgeuses`] | `MapEdgeuses` |
//!
//! Deviations: RAII replaces the destructor. Constructor scalars are an immutable snapshot
//! of `SmOffsetGeometryCreation` parameters, not a retained creator reference or its implementation. `Relation` uses deterministic id order
//! instead of upstream hash/pointer order; bubble sorting remains stable, including unordered
//! floating-point comparisons. Extension identity is a caller-supplied key, never an address.
//! Every unavailable dependency is reported at its actual call, including ignored statuses.
//! Continuations after unconditional missing calls are documented there, not reported as success.
//! `RemoveExcessFaces` borrows the original B-rep immutably; its legacy metadata/cache
//! adapter remains partial
//! and uses the existing canonical arena-identity cache adapter; custom geometry owner keys
//! or indirect owner chains are explicitly unsupported rather than silently skipped.
//! Result-edge enumeration uses `SmBrep::GetEdges` list order (`topology::brep::p1::edges`);
//! nonempty queries still defer at `NormalizedEvaluate`. `BuildSideWallOfEdgeuse` takes
//! the original B-rep as an argument (upstream reaches it through the edge use pointer).
//! Vertex caps use the shared geometry creator through a cross-arena adapter;
//! edge/face construction and SSI adapters remain partial.
//! `BuildOffsetOfFaceuse` moves the offset surface into `MakeFaceWithCurves` (upstream
//! hands over the pointer); a failed constructor drops it and, as upstream, relates no
//! face. Its `SmSSIData` UV curves are cloned into the merge input; a missing entry,
//! which upstream dereferences, is a null-pointer error. `GetInfiniteRegion` is the
//! `Brep` accessor. Of the shared `NormalizedEvaluate` placeholder, only `MapEdgeuses`
//! calls `topology::edgeuse::p3`; `RemoveExcessFaces` and `FillGapEdgeuses` keep it in
//! front of their own unported continuations.
//! No new input-driven recursion; existing topology walks are cycle-bounded.
//! The lamina clean-up's `DeleteFaces` runs through merge p4's arena intersector over
//! the offset B-rep alone (both upstream operands are that B-rep); it notifies under
//! `DELETE_FACES_NAMESPACE` rather than the caller's offset keys.

use std::collections::BTreeMap;

use crate::boolean::merge::p4::KernelManifoldIntersector;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::context::MarkType;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::solution_array::{Solution, SolutionArray};
use crate::geom::tol::Relation;
use crate::heal::stitch::p2 as stitch;
use crate::intersect::topology_solver::p1::TopologySolver;
use crate::intersect::topology_solver::p2::TopologySolveOptions;
use crate::kernel::Kernel;
use crate::math::{Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::offset::offset_geometry_creation::{OffsetGeometryCreation, SsiData};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1::{self as bp1, SetRegionVoidType};
use crate::topology::brep::p7::{self as bp7, FaceCurvesInput, SelfIntersectionCurve};
use crate::topology::brep_cache::{
    arena_identity, make_brep_cache, BrepCache, BrepObjects, CacheObject, CacheSource,
};
use crate::topology::edge::{self, NotifyEnv};
use crate::topology::edgeuse::{p1 as eu, p2 as eu2, p3 as eu3};
use crate::topology::face::{p2 as face, p3 as face3};
use crate::topology::topology;
use crate::topology::topology_traverser::face_edges;
use crate::topology::{loopuse, vertex, Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, VertexId};

const OP: &str = "offset::offset_executive::p1";

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
    faceuse_to_faces: Relation<FaceuseId, FaceId>,
    edgeuse_to_edgeuses: Relation<EdgeuseId, EdgeuseId>,
    edge_to_faces: Relation<EdgeId, FaceId>,
    vertex_to_faces: Relation<VertexId, FaceId>,
    blend_to_faces: Relation<FaceId, FaceId>,
    extended_surfaces: BTreeMap<u64, Surface>,
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

    fn required_offset_brep(&self) -> Result<&Brep> {
        required(self.offset_brep.as_ref())
    }

    fn clear_relations(&mut self) {
        self.faceuse_to_faces.remove_all();
        self.edgeuse_to_edgeuses.remove_all();
        self.edge_to_faces.remove_all();
        self.vertex_to_faces.remove_all();
        self.blend_to_faces.remove_all();
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
        let original = required(original)?;
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

        let brep = required(self.offset_brep.as_mut())?;
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

    /// Returns a cached extension, creating it with distance `offset_distance * 5.0`
    /// and G1-ruled continuity. A hit never re-extends or replaces the cached surface.
    ///
    /// `base_key` represents upstream pointer identity, including across Rust clones.
    /// Distinct source identities must use distinct keys. Failed extensions are not cached.
    pub fn get_create_extended_surface(
        &mut self,
        kernel: &Kernel,
        base_key: u64,
        base: &Surface,
    ) -> Result<&Surface> {
        match self.extended_surfaces.entry(base_key) {
            std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
            std::collections::btree_map::Entry::Vacant(entry) => {
                // The obsolete offset-specific `else` at cpp:2158 is unreachable:
                // NER(pBase) already established pOrigSurf == pBase != NULL.
                let extension = base.create_extended_surface(
                    kernel,
                    self.offset_distance * 5.0,
                    ContinuityType::G1R,
                )?;
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

    /// Replaces implicit offset surfaces with approximations (`ReplaceImplicitOffsets`).
    /// The unconditional initial shrink is owned by `topology.brep_p4`.
    pub fn replace_implicit_offsets(
        _kernel: &Kernel,
        _brep: &mut Brep,
        _approximation_tol_3d: f64,
    ) -> Result<()> {
        // Continuation cpp:1978–2035: enumerate offset surfaces, approximate their
        // extended-or-original B-spline bases, then notify/replace each surface.
        // Neither shrink nor offset surfaces (#62) may be bypassed or co-ported here.
        // UNPORTED(topology.brep_p4): SmBrep::ShrinkGeometry
        Err(SolidError::unported(
            "SmBrep::ShrinkGeometry",
            "topology.brep_p4",
        ))
    }

    /// Removes result faces too close to the original (`RemoveExcessFaces`).
    ///
    /// The source is borrowed immutably while solving, corresponding to the upstream
    /// editing-disabled query. `Brep` has no editing flag, so upstream's success-only
    /// reset to `TRUE` (or leaving it `FALSE` on a solver error) cannot be represented.
    /// Geometry owners must use [`arena_identity`] directly; custom keys or indirect
    /// owner chains return [`SolidError::Unsupported`] rather than omitting geometry.
    /// Cache and solver errors propagate before any result-face removal. Ordinary
    /// evaluation statuses are ignored, but an unported evaluator still defers.
    /// Result edges are visited in upstream edge list order (`SmBrep::GetEdges`).
    pub fn remove_excess_faces(
        &self,
        kernel: &Kernel,
        original: &Brep,
        result: &mut Brep,
    ) -> Result<()> {
        if !self.merge_results || !self.shell_faces.is_empty() {
            return Ok(());
        }
        let min_distance = self.offset_distance * 0.95;
        let mut delete_faces = Vec::new();
        let mut original_cache = None;
        // SmBrep::GetEdges (unit topology.brep_p1): the m_pEdgeListHead list order
        for id in crate::topology::brep::p1::edges(result, None)? {
            let primary = required(eu::primary_edgeuse(result, id)?)?;
            // Upstream ignores the evaluation status and keeps the initialized point.
            // An unported evaluator must still propagate instead of supplying a fake point.
            let point =
                ignored(normalized_evaluate(result, primary, 0.5, false))?.unwrap_or(Vec3::UNDEF);
            if original_cache.is_none() {
                original_cache = Some(make_original_brep_cache(kernel, original)?);
            }
            let cache = required(original_cache.as_ref())?;
            let solutions = solve_original_brep_point(kernel, original, cache, point)?;
            collect_excess_faces(
                result,
                id,
                solutions.as_slice(),
                min_distance,
                &self.shell_faces,
                &mut delete_faces,
            )?;
        }
        // All candidate faces have been gathered in first-seen order, without duplicates.
        // Even an empty deletion list reaches RemoveFaces upstream (mark increment).
        // Part 5's remove_faces is available, but needs the actual editing flag
        // and mutable caller Kernel for native traversal-mark updates. When the
        // owning metadata lands, forward those and delete_faces to that body;
        // do not assume editing is enabled or clone a substitute context.
        // UNPORTED(topology.brep_p1): SmBrep::m_bEditingEnabled
        Err(SolidError::unported(
            "SmBrep::m_bEditingEnabled",
            "topology.brep_p1",
        ))
    }

    /// Builds a closing ruled side wall, or succeeds without work for null/ambiguous mappings.
    pub fn build_side_wall_of_edgeuse(
        &mut self,
        kernel: &Kernel,
        original_brep: &mut Brep,
        original: Option<EdgeuseId>,
    ) -> Result<()> {
        let Some(original) = original else {
            return Ok(());
        };
        let Some(offset) = self.offset_edgeuse(original) else {
            return Ok(());
        };
        let offset_brep = required(self.offset_brep.as_mut())?;
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
        let domain = new_surface.natural_uv_domain();
        let new_face = create_face_from_surface(offset_brep, &new_surface, &domain)?;
        self.closing_faces.push(new_face); // [B549]
        Ok(())
    }

    /// Builds edge cap offsets. Notifications use distinct original/result object keys;
    /// `offset_curve_ref` identifies offset edge curves split while synchronizing. The
    /// geometry creation itself remains owned by `offset.offset_geometry_creation`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_offset_of_edgeuses(
        &mut self,
        kernel: &mut Kernel,
        original: &Brep,
        edgeuse: EdgeuseId,
        other: EdgeuseId,
        _mark_type: MarkType,
        notifications: &mut NotifyEnv<'_>,
        offset_key_of: &dyn Fn(EntityRef) -> ObjectRef,
        offset_curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
    ) -> Result<()> {
        let edge = edge_of(original, edgeuse)?;
        let first_face = eu::face(original, edgeuse)?;
        let other_face = eu::face(original, other)?;
        if first_face.is_some_and(|id| self.shell_faces.contains(&id))
            && other_face.is_some_and(|id| self.shell_faces.contains(&id))
            && !self.extend_convex_edges
        {
            return Ok(());
        }
        // Splits notify offset B-rep entities, keyed like the result side below.
        let mut offset_notifications = NotifyEnv {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            context: notifications.context.as_deref_mut(),
            key_of: offset_key_of,
        };
        let synchronized = self.synchronize_offset_edgeuses(
            kernel,
            Some(edgeuse),
            Some(other),
            &mut offset_notifications,
            offset_curve_ref,
        )?;
        // Geometry creation is unconditional on the first pair, so no later
        // iteration is reachable until EdgeuseOffset is available.
        if let Some(offset) = synchronized.edgeuses.first() {
            let other_offset = required(synchronized.other_edgeuses.first())?;
            let _other_offset = if eu::edge_is_lamina(original, edge)? {
                None
            } else {
                *other_offset
            };
            if let Some(offset) = offset {
                let result = self.required_offset_brep()?;
                notify_edge_and_vertices(
                    original,
                    edge,
                    result,
                    edge_of(result, *offset)?,
                    notifications,
                    offset_key_of,
                )?;
            }
            // Continuation cpp:2956–3071: skip absent cap, otherwise build trimmed
            // or natural-domain face and record edge->face. No reachable geometry
            // creation branch can be substituted for #156's EdgeuseOffset.
            // UNPORTED(offset.offset_geometry_creation): SmOffsetGeometryCreation::EdgeuseOffset
            return Err(SolidError::unported(
                "SmOffsetGeometryCreation::EdgeuseOffset",
                "offset.offset_geometry_creation",
            ));
        }

        let first = self.fill_gap_edgeuses(kernel, edge, &synchronized.gaps, &[]);
        let second = self.fill_gap_edgeuses(kernel, edge, &synchronized.other_gaps, &[]);
        let mut problems = false;
        for status in [first, second] {
            match status {
                Ok(()) => {}
                Err(error) if error.is_unported() => return Err(error),
                Err(_) => problems = true,
            }
        }
        if problems {
            let _curve = edge::create_trimmed_nurbs_curve(kernel, original, edge)?;
            self.required_offset_brep()?;
            // cpp:3092: offset distance, approximation tolerance, FALSE, FALSE.
            // UNPORTED(primitive.primitive_creation_p2): SmPrimitiveCreation::CreatePipeSweep
            return Err(SolidError::unported(
                "SmPrimitiveCreation::CreatePipeSweep",
                "primitive.primitive_creation_p2",
            ));
        }
        Ok(())
    }

    /// Fills unmatched edge gaps. Empty gaps succeed without dereferencing any topology.
    pub fn fill_gap_edgeuses(
        &mut self,
        _kernel: &Kernel,
        _edge: EdgeId,
        gaps: &[EdgeuseId],
        candidate_faces: &[FaceId],
    ) -> Result<()> {
        if gaps.is_empty() {
            return Ok(());
        }
        let brep = self.required_offset_brep()?;
        let mut all_vertices = Vec::new();
        for &face in candidate_faces {
            all_vertices.extend(face_vertices(brep, face)?);
        }
        for &gap in gaps {
            let _midpoint = normalized_evaluate(brep, gap, 0.5, false)?;
            // Continuation cpp:3149–3214: for each endpoint find coincident candidate
            // vertices using the sum of vertex tolerances (strict <); take each
            // closest incident edge. The candidate list persists across gap uses.
            // Unless exactly two candidates exist return success. Otherwise create
            // the corner blend (#145), construct its faces and record both relations.
        }
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
        let vertex = required(vertex)?;
        let point = original.vertex(vertex)?.point;
        let result = self.required_offset_brep()?;
        let tol = result.tolerance();
        let radius = self.offset_distance;
        let mut edges = vertex::edges(original, vertex, None)?;
        let mut edges_to_blend = Vec::new();

        if let [only] = edges.as_slice() {
            let mut edgeuse = required(eu::primary_edgeuse(original, *only)?)?;
            if start_vertex(original, edgeuse)? != vertex {
                edgeuse = required(eu::radial(original, edgeuse)?)?;
            }
            if eu::is_strut(original, edgeuse)? {
                let faceuse = required(eu::faceuse(original, edgeuse)?)?;
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
            required(self.offset_brep.as_mut())?,
            bounding_edges,
        )
    }
}

impl OffsetExecutive {
    /// Stitches or merges the assembled offset (`PiecewiseMerge`).
    ///
    /// On the non-merge path the B-rep is taken and relations are cleared *before*
    /// stitching, as upstream. A deferred stitch therefore leaves `offset_brep()` empty.
    pub fn piecewise_merge(&mut self, kernel: &mut Kernel, original: &mut Brep) -> Result<Brep> {
        if !self.merge_results {
            self.clear_relations();
            let mut result = required(self.offset_brep.take())?;
            // the stitch statuses are ignored upstream
            if self.create_offset_solid {
                // Tol: StitchIntoShell() adds in the tols of the topology objects,
                // so the Brep's tolerance should be plenty.
                let stitch_tol_3d = result.tolerance();
                ignore_status(stitch::stitch_into_shell(
                    kernel,
                    &mut result,
                    true,
                    stitch_tol_3d,
                ))?;
            } else {
                ignore_status(stitch::stitch_into_solid(kernel, &mut result))?;
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
            let face = required(original.faceuse(faceuse)?.face)?;
            for offset_face in self.faceuse_to_faces.seconds(faceuse) {
                required(result.face(offset_face)?.surface.as_ref())?;
                for edge_id in face_edges(original, face)? {
                    let edge_faces = self.edge_to_faces.seconds(edge_id);
                    for &edge_face in &edge_faces {
                        required(result.face(edge_face)?.surface.as_ref())?;
                        for blended in self.blend_to_faces.seconds(edge_face) {
                            required(result.face(blended)?.surface.as_ref())?;
                            relate_surface_pair(&mut no_ssi, blended, edge_face)?;
                        }
                        relate_surface_pair(&mut no_ssi, offset_face, edge_face)?;
                        let start = edge::vertex(original, edge_id)?;
                        let end = edge::other_vertex(original, edge_id, start)?;
                        for vertex in [start, end].into_iter().flatten() {
                            if let Some(vertex_face) = self.vertex_to_faces.second(vertex) {
                                required(result.face(vertex_face)?.surface.as_ref())?;
                                relate_surface_pair(&mut no_ssi, edge_face, vertex_face)?;
                            }
                        }
                    }
                    if !edge_faces.is_empty() {
                        continue;
                    }
                    if edge::is_manifold(original, edge_id)? {
                        let primary = required(eu::primary_edgeuse(original, edge_id)?)?;
                        if eu2::is_tangent_sector(
                            kernel,
                            original,
                            primary,
                            self.tangency_tol_degrees(),
                            false,
                            true,
                        )? {
                            for edgeuse in edge::edgeuses(original, edge_id, None, None)? {
                                if eu::faceuse(original, edgeuse)? == Some(faceuse) {
                                    let radial = required(eu::radial(original, edgeuse)?)?;
                                    let radial_faceuse = required(eu::faceuse(original, radial)?)?;
                                    for radial_face in self.faceuse_to_faces.seconds(radial_faceuse)
                                    {
                                        required(result.face(radial_face)?.surface.as_ref())?;
                                        relate_surface_pair(&mut no_ssi, offset_face, radial_face)?;
                                    }
                                }
                            }
                        }
                    }
                }
                for vertex in face_vertices(original, face)? {
                    if let Some(vertex_face) = self.vertex_to_faces.second(vertex) {
                        required(result.face(vertex_face)?.surface.as_ref())?;
                        relate_surface_pair(&mut no_ssi, offset_face, vertex_face)?;
                    }
                }
            }
        }
        self.clear_relations();
        // Continuation cpp:2429–2474: construct a merge B-rep with the original
        // tolerance, editing enabled; SmMerge uses 20*PI/180 angular tolerance,
        // PiecewiseMerge(&no_ssi, TRUE, TRUE). It keeps the original offset B-rep.
        // UNPORTED(topology.brep_p4): SmBrep::ShrinkGeometry
        Err(SolidError::unported(
            "SmBrep::ShrinkGeometry",
            "topology.brep_p4",
        ))
    }

    /// Builds a face from the supplied offset surface (`BuildOffsetOfFaceuse`).
    /// A null surface is a successful no-op. When the face domain lies inside the
    /// offset surface's natural domain (or for a local operation) the offset face is
    /// made from the face's UV curve copies and takes ownership of the surface
    /// (`offset_surface` becomes `None`); `notifications` keys original-side
    /// entities and `offset_key_of` offset-side ones. Otherwise the 2D domain boolean
    /// defers at the B-rep face constructor.
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
    ) -> Result<()> {
        let faceuse = required(faceuse)?;
        let Some(surface) = offset_surface.as_mut() else {
            return Ok(());
        };
        self.required_offset_brep()?;
        // SM_ENSURE_SURFACE_MPNURB: its return value is not checked.
        match surface.kind_mut() {
            SurfaceKind::BSpline(surface) => {
                ignore_status(surface.get_or_create_gw_nurb())?;
            }
            SurfaceKind::Plane(surface) if surface.base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb())?;
            }
            SurfaceKind::SurfOfRevolution(surface) if surface.base().gw_nurb().is_none() => {
                ignore_status(surface.make_nurb(kernel))?;
            }
            _ => {}
        }
        let original_faceuse = faceuse;
        let faceuse = original.faceuse(faceuse)?;
        let face_id = required(faceuse.face)?;
        let orientation = faceuse.orientation;
        // The translated-3D-curve plane case (cpp:3560–3600) is `#if 0` upstream.
        let mut curves =
            face::create_curves_from_face(kernel, original, face_id, orientation, false, true)?;
        let offset_domain = surface.natural_uv_domain();
        let face_domain = original.face(face_id)?.uv_domain;
        if face_domain.is_contained_by(&offset_domain, EFF_ZERO)
            || self.operation == OffsetOperation::LocalOperation
        {
            let surface = required(offset_surface.take())?;
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

        // cpp:3767–3825: prepare the 2D domain boolean. No later boolean or
        // topology construction can run before CreatePlanarFaceWith3DCurves.
        let mut planar_brep = Brep::new();
        planar_brep.set_tolerance(1.0e-6);
        let mut total_bbox = Extent3d::new();
        if let Some(uv_curves) = curves.uv_curves.as_mut() {
            for curve in uv_curves {
                let bbox = uv_curve_bounding_box(kernel, curve)?;
                total_bbox = total_bbox.union(&bbox);
            }
        }
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
            ignore_status(line.edit_parameterization(&ivl))?;
            box_curves.push(line);
        }
        // Continuation cpp:3827–3928: copy/expand the planar surface to include UV
        // bounds, MakeFaceWithCurves, Boolean2D(intersection), map its faces' 3D
        // curves back to UV, construct offset faces, notify and relate them.
        // UNPORTED(topology.brep_p8): SmBrep::CreatePlanarFaceWith3DCurves
        Err(SolidError::unported(
            "SmBrep::CreatePlanarFaceWith3DCurves",
            "topology.brep_p8",
        ))
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
    ) -> Result<Vec<FaceId>> {
        let mut loops = Vec::new();
        for &count in &curves.curve_loops {
            loops.push(
                usize::try_from(count)
                    .map_err(|_| SolidError::invalid_input(OP, "loop count exceeds usize"))?,
            );
        }
        let input = FaceCurvesInput {
            loops,
            curves_3d: None,
            uv_curves: curves.uv_curves,
            orientations: curves.curve_orients,
            loop_points: curves.loop_points,
        };
        let this_face = |face| offset_key_of(EntityRef::Face(face));
        let brep = required(self.offset_brep.as_mut())?;
        let region = brep.infinite_region();
        // Only success continues; a failed constructor leaves no face to relate.
        // The surface moves into the face constructor and is dropped on failure.
        let made = match bp7::make_face_with_curves(
            kernel,
            brep,
            region,
            input,
            surface,
            offset_domain,
            orientation,
            None,
            &this_face,
            &mut offset_env(notifications, offset_key_of),
        ) {
            Ok(made) => made,
            Err(error) if error.is_unported() => return Err(error),
            Err(_) => return Ok(Vec::new()),
        };
        let new_face = required(made.face)?;
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
            // Upstream dereferences each UV curve unchecked.
            let mut merged = Vec::new();
            for (index, curve_3d) in self_intersections.curves_3d.iter().enumerate() {
                let uv_curve = |list: &[Option<Curve>]| -> Result<Curve> {
                    required(list.get(index).and_then(Option::as_ref)).cloned()
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
                let edgeuse = required(edge::same_oriented_edgeuse(brep, edge)?)?;
                let point =
                    eu3::normalized_evaluate(kernel, brep, edgeuse, 0.4567, false, false, true)?
                        .point;
                let offset_distance = self.offset_distance;
                let test_distance = offset_distance * 1.1;
                let original_face = original.face(face_id)?;
                let uv_domain = original_face.uv_domain;
                let original_surface = required(original_face.surface.as_ref())?;
                // Upstream disables the surface cache's point test here; the solver
                // defers for face-owned surfaces until that switch exists.
                let solutions = original_surface.global_point_solve(
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
            ignore_status(KernelManifoldIntersector::delete_faces_in(
                kernel,
                brep,
                &delete_faces,
                &mut *notifications.records,
                &mut *notifications.attributes,
                DELETE_FACES_NAMESPACE,
            ))?;
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
        let original_use = original.faceuse(original_faceuse)?;
        let original_orientation = original_use.orientation;
        let original_face = original.face(required(original_use.face)?)?;
        let brep = self.required_offset_brep()?;
        let offset_face = brep.face(offset_face)?;
        // Intersect's status is ignored; upstream initializes its output on failure.
        let domain = ignored(original_face.uv_domain.intersect(&offset_face.uv_domain))?
            .unwrap_or_else(Extent2d::new);
        let mid_uv = domain.evaluate(0.5, 0.5);
        let normal = ignored(required(original_face.surface.as_ref())?.evaluate_normal(
            kernel,
            mid_uv,
            Side::Right,
            Side::Right,
        ))?
        .unwrap_or(Vec3::UNDEF);
        let offset_normal = ignored(required(offset_face.surface.as_ref())?.evaluate_normal(
            kernel,
            mid_uv,
            Side::Right,
            Side::Right,
        ))?
        .unwrap_or(Vec3::UNDEF);
        let same_normals = normal.dot(offset_normal) > 0.0;
        let mut original_uses: Vec<Option<EdgeuseId>> =
            eu::faceuse_edgeuses(original, original_faceuse, false, None)?
                .into_iter()
                .map(Some)
                .collect();
        let upward = required(offset_face.faceuse)?;
        let offset_use = if (same_normals && original_orientation == OrientType::Same)
            || (!same_normals && original_orientation == OrientType::Opposite)
        {
            upward
        } else {
            required(brep.faceuse(upward)?.mate)?
        };
        for offset in eu::faceuse_edgeuses(brep, offset_use, false, None)? {
            // Only 0.5 is used: matched uses may run in opposite directions.
            let brep = required(self.offset_brep.as_mut())?;
            let offset_point =
                eu3::normalized_evaluate(kernel, brep, offset, 0.5, true, false, true)?.point;
            let mut found = false;
            let mut scale = 1.0;
            while !found && scale < 10000.0 {
                for slot in 0..original_uses.len() {
                    let Some(original_use) = original_uses.get(slot).copied().flatten() else {
                        continue;
                    };
                    let original_point = eu3::normalized_evaluate(
                        kernel,
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
                        let uv_curve = required(eu::uv_trim_curve(original, original_use)?.0)?;
                        found = uv_curve
                            .drop_point(
                                kernel,
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
                        notify_edge_and_vertices(
                            original,
                            edge_of(original, original_use)?,
                            result,
                            edge_of(result, offset)?,
                            notifications,
                            offset_key_of,
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

fn relate_surface_pair(
    relation: &mut Relation<FaceId, FaceId>,
    a: FaceId,
    b: FaceId,
) -> Result<()> {
    relation.relate_pair(a, b)?;
    relation.relate_pair(b, a)
}

fn uv_curve_bounding_box(kernel: &Kernel, curve: &mut Curve) -> Result<Extent3d> {
    // CreateCurvesFromFace supplied B-spline slicing copies or lines.
    match curve.kind_mut() {
        CurveKind::BSpline(curve) => ignore_status(curve.convert_to_3d())?,
        CurveKind::Line(curve) => ignore_status(curve.convert_to_3d())?,
        // UV curves from CreateCurvesFromFace are slicing copies, never analytic
        // non-line kinds. Reject malformed values rather than silently changing them.
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "UV copy is not a B-spline or line",
            ));
        }
    }
    // cpp:3777–3780 ignores both statuses. An ordinary bounding-box failure leaves
    // the initialized extent, while a genuinely missing dependency must still defer.
    Ok(ignored(curve.calculate_bounding_box(
        kernel,
        &curve.natural_interval(),
        crate::nurbs::b_spline_curve::BoundingBoxRequest {
            normal: true,
            pseudo: false,
            polar: false,
        },
        true,
    ))?
    .and_then(|boxes| boxes.normal)
    .unwrap_or_else(Extent3d::new))
}

/// `SmBrep::CreateFaceFromSurface(pSurface, sDomain, rpNewFace)` (unit
/// `topology.brep_p8`): the new face.
fn create_face_from_surface(b: &mut Brep, surface: &Surface, domain: &Extent2d) -> Result<FaceId> {
    let _ = (b, surface, domain);
    // UNPORTED(topology.brep_p8): SmBrep::CreateFaceFromSurface
    Err(SolidError::unported(
        "SmBrep::CreateFaceFromSurface",
        "topology.brep_p8",
    ))
}

fn required<T>(value: Option<T>) -> Result<T> {
    value.ok_or_else(|| SolidError::null_pointer(OP))
}

fn edge_of(brep: &Brep, edgeuse: EdgeuseId) -> Result<EdgeId> {
    required(brep.edgeuse(edgeuse)?.edge)
}

fn interval(brep: &Brep, edgeuse: EdgeuseId) -> Result<Extent1d> {
    Ok(brep.edge(edge_of(brep, edgeuse)?)?.interval)
}

fn start_vertex(brep: &Brep, edgeuse: EdgeuseId) -> Result<VertexId> {
    let vertexuse = required(brep.edgeuse(edgeuse)?.vertexuse)?;
    required(brep.vertexuse(vertexuse)?.vertex)
}

fn bspline_edge_curve(brep: &Brep, edge: EdgeId) -> Result<&Curve> {
    let curve = brep
        .edge(edge)?
        .curve
        .as_ref()
        .ok_or_else(|| SolidError::sm_err(OP))?;
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
        Err(SolidError::sm_err(OP))
    }
}

fn face_vertices(brep: &Brep, face: FaceId) -> Result<Vec<VertexId>> {
    let mut out = Vec::new();
    if let Some(faceuse) = brep.face(face)?.faceuse {
        for loopuse in eu::faceuse_loopuses(brep, faceuse, false, None)? {
            for vertex in loopuse::vertices(brep, loopuse, None)? {
                out.push(required(vertex)?);
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
            let last = required(edgeuses.get(i - 1).copied())?;
            let current = required(edgeuses.get(i).copied())?;
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
    env: &mut NotifyEnv<'_>,
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
                let output = crate::topology::brep::p9::make_vertex_split_edge(
                    kernel, brep, edge_id, param, env, curve_ref,
                )?;
                if !output.split {
                    continue; // Can't split if too close to end point
                }
                let new_edges = required(output.edges)?;
                let ccw = required(eu::ccw_edgeuse(brep, split)?)?;
                let cw = required(eu::cw_edgeuse(brep, split)?)?;
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
        let curve = required(edge.curve.as_ref())?;
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

fn make_original_brep_cache(kernel: &Kernel, original: &Brep) -> Result<BrepCache> {
    // The legacy executive API has no caller-provided owner-key or owner-chain adapter.
    // Reject incompatible owners before cache construction can silently skip them.
    for (id, edge) in original.edges().iter() {
        if edge
            .curve
            .as_ref()
            .and_then(Curve::owner)
            .is_some_and(|owner| owner != arena_identity(CacheObject::Edge(id)))
        {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "point-solve edge owners require direct canonical arena identities",
            });
        }
    }
    for (id, face) in original.faces().iter() {
        if face
            .surface
            .as_ref()
            .and_then(Surface::owner)
            .is_some_and(|owner| owner != arena_identity(CacheObject::Face(id)))
        {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "point-solve face owners require direct canonical arena identities",
            });
        }
    }
    make_brep_cache(kernel, CacheSource::Brep(original), &arena_identity)
}

fn solve_original_brep_point(
    kernel: &Kernel,
    original: &Brep,
    cache: &BrepCache,
    point: Vec3,
) -> Result<SolutionArray<CacheObject>> {
    let objects = BrepObjects {
        brep: original,
        // There is exactly one B-rep in this query adapter's local namespace.
        brep_id: crate::topology::BrepId::from_index(0),
        cache,
        keys: &arena_identity,
        // make_original_brep_cache checked that all geometry owners are direct.
        owner_of: &|_| None,
    };
    TopologySolver::brep_point_solve(
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
    )
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

fn normalized_evaluate(brep: &Brep, edgeuse: EdgeuseId, _param: f64, _uv: bool) -> Result<Vec3> {
    brep.edgeuse(edgeuse)?;
    // UNPORTED(topology.edgeuse_p3): SmEdgeuse::NormalizedEvaluate
    Err(SolidError::unported(
        "SmEdgeuse::NormalizedEvaluate",
        "topology.edgeuse_p3",
    ))
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
    let old_start = required(edge::start_vertex(original, old_edge)?)?;
    let new_start = required(edge::start_vertex(result, new_edge)?)?;
    notify(EntityRef::Vertex(old_start), EntityRef::Vertex(new_start))?;
    let old_end = required(edge::other_vertex(original, old_edge, Some(old_start))?)?;
    if old_end != old_start {
        let new_end = required(edge::other_vertex(result, new_edge, Some(new_start))?)?;
        notify(EntityRef::Vertex(old_end), EntityRef::Vertex(new_end))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::attribute::{AttributeStore, ObjectKey};
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector2d::Vec2;
    use crate::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::surface::plane::Plane;
    use crate::topology::{Edge, Edgeuse, Face, Faceuse, ShellId, Vertex, Vertexuse};

    fn executive() -> OffsetExecutive {
        OffsetExecutive::new(OffsetOperation::SolidOffset, 0.125, 1.0e-6)
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

    fn split_env<R>(
        run: impl FnOnce(&mut NotifyEnv<'_>, &dyn Fn(EdgeId, i64) -> ObjectRef) -> R,
    ) -> R {
        let mut records = BTreeMap::new();
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
        split_env(|env, curve_ref| {
            executive.synchronize_offset_edgeuses(&mut kernel, original, other, env, curve_ref)
        })
    }

    fn assert_dependency<T: core::fmt::Debug>(result: Result<T>, symbol: &str, owner: &str) {
        match result.unwrap_err() {
            SolidError::Unported { upstream, unit } => {
                assert_eq!(upstream, symbol);
                assert_eq!(unit, owner);
            }
            other => panic!("expected dependency {symbol}, got {other:?}"),
        }
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
        assert!(matches!(
            sync(&mut executive, None, None),
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
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
            let result = split_env(|env, curve_ref| {
                insert_breaks(kernel, brep, uses, &mut to_split, env, curve_ref)
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
        // NULL), so it stops there with SM_ERR_NULL_POINTER.
        let (result, _) = insert(&mut brep, &mut kernel, &[max_break]);
        assert!(matches!(
            result,
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
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
        assert!(matches!(
            sync(&mut executive, Some(a), Some(b)),
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
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
            .create_extended_surface(&kernel, 0.125 * 5.0, ContinuityType::G1R)
            .unwrap();
        let first = executive
            .get_create_extended_surface(&kernel, 41, &base)
            .unwrap();
        assert_eq!(first.natural_uv_domain(), expected.natural_uv_domain());
        let first_point = first
            .evaluate_point(&kernel, first.natural_uv_domain().evaluate(0.5, 0.5))
            .unwrap();
        // A cached identity does not evaluate even a different/malformed supplied base.
        let broken = Surface::from(BSplineSurface::new());
        let cached = executive
            .get_create_extended_surface(&kernel, 41, &broken)
            .unwrap();
        assert_eq!(
            cached
                .evaluate_point(&kernel, cached.natural_uv_domain().evaluate(0.5, 0.5))
                .unwrap(),
            first_point
        );
        let other = executive
            .get_create_extended_surface(&kernel, 42, &plane(7.0))
            .unwrap();
        assert_eq!(
            other
                .evaluate_point(&kernel, other.natural_uv_domain().evaluate(0.5, 0.5))
                .unwrap()
                .z,
            7.0
        );
        assert_eq!(executive.extended_surfaces.len(), 2);
    }

    #[test]
    fn failed_extensions_are_not_cached_and_can_be_retried() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let broken = Surface::from(BSplineSurface::new());
        assert!(executive
            .get_create_extended_surface(&kernel, 1, &broken)
            .is_err());
        assert!(executive.extended_surfaces.is_empty());
        assert!(executive
            .get_create_extended_surface(&kernel, 1, &plane(0.0))
            .is_ok());
    }

    #[test]
    fn sidewalls_handle_null_missing_ambiguous_and_single_mappings() {
        let kernel = Kernel::default();
        let mut executive = executive();
        // B-spline edge curves (a Line whose parameterisation was edited carries no NURB)
        let mut original_brep = Brep::new();
        let original = add_bspline_use(&mut original_brep, 0.0);
        let mut offset_brep = Brep::new();
        let offset = add_bspline_use(&mut offset_brep, 1.0);
        executive.set_offset_brep(offset_brep);
        executive
            .build_side_wall_of_edgeuse(&kernel, &mut original_brep, None)
            .unwrap();
        executive
            .build_side_wall_of_edgeuse(&kernel, &mut original_brep, Some(original))
            .unwrap();
        // the ruled blend across the two B-reps runs; the closing face is next
        executive.relate_edgeuses(original, offset);
        assert_dependency(
            executive.build_side_wall_of_edgeuse(&kernel, &mut original_brep, Some(original)),
            "SmBrep::CreateFaceFromSurface",
            "topology.brep_p8",
        );
        executive.relate_edgeuses(original, EdgeuseId::from_index(2));
        executive
            .build_side_wall_of_edgeuse(&kernel, &mut original_brep, Some(original))
            .unwrap();
    }

    #[test]
    fn excess_face_early_returns_and_actual_dependencies() {
        let kernel = Kernel::default();
        let mut executive = executive();
        let original = Brep::new();
        let mut result = Brep::new();
        executive
            .remove_excess_faces(&kernel, &original, &mut result)
            .unwrap();
        executive.set_merge_results(true);
        executive.set_shell_faces(&[FaceId::from_index(1)]);
        executive
            .remove_excess_faces(&kernel, &original, &mut result)
            .unwrap();
        executive.set_shell_faces(&[]);
        assert_dependency(
            executive.remove_excess_faces(&kernel, &original, &mut result),
            "SmBrep::m_bEditingEnabled",
            "topology.brep_p1",
        );
        add_use(&mut result, 0.0, 1.0, 1.0);
        add_use(&mut result, 0.0, 1.0, 2.0);
        // Multiple arena entries do not establish linked-list order parity: this
        // prefix must still defer before collecting or removing any result faces.
        assert_dependency(
            executive.remove_excess_faces(&kernel, &original, &mut result),
            "SmEdgeuse::NormalizedEvaluate",
            "topology.edgeuse_p3",
        );
        assert_eq!(result.edges().len(), 2);
        assert_eq!(result.edgeuses().len(), 2);
        assert!(result.faces().is_empty());
        assert_dependency(
            OffsetExecutive::replace_implicit_offsets(&kernel, &mut result, 1.0e-6),
            "SmBrep::ShrinkGeometry",
            "topology.brep_p4",
        );
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
    fn original_brep_query_propagates_actual_cache_dependency() {
        let kernel = Kernel::default();
        // The real cache constructor currently defers at bounding-box calculation,
        // not at the already implemented BrepPointSolve entry point.
        assert_dependency(
            make_original_brep_cache(&kernel, &Brep::new()),
            "SmBrep::CalculateBoundingBox",
            "query.brep",
        );
    }

    #[test]
    fn original_brep_query_calls_live_solver_and_propagates_its_status() {
        let kernel = Kernel::default();
        let original = Brep::new();
        let cache = BrepCache::new(crate::geom::a_object::BREP_TYPE).unwrap();
        assert!(matches!(
            solve_original_brep_point(&kernel, &original, &cache, Vec3::ZERO),
            Err(SolidError::Upstream {
                operation: "intersect::topology_solver::p1::TopologyObjects::brep_trees",
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
    }

    #[test]
    fn original_brep_query_rejects_unmapped_geometry_owners() {
        let kernel = Kernel::default();
        let mut original = Brep::new();
        let edgeuse = add_use(&mut original, 0.0, 1.0, 1.0);
        let edge_id = edge_of(&original, edgeuse).unwrap();
        original
            .edge_mut(edge_id)
            .unwrap()
            .curve
            .as_mut()
            .unwrap()
            .set_owner(
                Some(ObjectRef {
                    key: ObjectKey(900),
                    type_code: edge::TYPE,
                }),
                None,
            );
        assert!(matches!(
            make_original_brep_cache(&kernel, &original),
            Err(SolidError::Unsupported { .. })
        ));
        original
            .edge_mut(edge_id)
            .unwrap()
            .curve
            .as_mut()
            .unwrap()
            .set_owner(Some(arena_identity(CacheObject::Edge(edge_id))), None);
        assert_dependency(
            make_original_brep_cache(&kernel, &original),
            "SmBrep::CalculateBoundingBox",
            "query.brep",
        );
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
            // StitchIntoShell (past DoStitching's first Notify) and StitchIntoSolid both
            // defer at the B-rep bounding box
            assert_dependency(
                executive.piecewise_merge(&mut kernel, &mut original),
                "SmBrep::CalculateBoundingBox",
                "query.brep",
            );
            assert!(executive.offset_brep().is_none());
            assert_eq!(executive.offset_edgeuse(EdgeuseId::from_index(1)), None);
        }
        let mut executive = executive();
        executive.set_merge_results(true);
        executive.set_offset_brep(Brep::new());
        executive.relate_edgeuses(EdgeuseId::from_index(1), EdgeuseId::from_index(2));
        assert_dependency(
            executive.piecewise_merge(&mut kernel, &mut original),
            "SmBrep::ShrinkGeometry",
            "topology.brep_p4",
        );
        assert!(executive.offset_brep().is_some());
        assert_eq!(executive.offset_edgeuse(EdgeuseId::from_index(1)), None);
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
    fn uv_boolean_bounds_ignore_ordinary_failures_and_keep_conversion_side_effects() {
        let kernel = Kernel::default();
        // Rust-only error-path input: upstream ConvertTo3D dereferences the missing NURB.
        let mut curve = Curve::from(BSplineCurve::new(2));
        let bbox = uv_curve_bounding_box(&kernel, &mut curve).unwrap();
        assert_eq!(bbox.min(), Extent3d::new().min());
        assert_eq!(bbox.max(), Extent3d::new().max());
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
        let mut records = BTreeMap::new();
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
        assert_dependency(
            build_face(&mut executive, &mut original, Some(faceuse), &mut offset),
            "SmBrep::CreatePlanarFaceWith3DCurves",
            "topology.brep_p8",
        );
        assert!(offset.is_some());
        executive.set_operation(OffsetOperation::LocalOperation);
        let mut offset = Some(smaller);
        build_face(&mut executive, &mut original, Some(faceuse), &mut offset).unwrap();
        assert!(offset.is_none());
    }

    #[test]
    fn face_prefix_preserves_unported_uv_evaluation_before_domain_boolean() {
        let mut executive = executive();
        let (mut original, _, face, _) = excess_face_fixture();
        let faceuse = original.face(face).unwrap().faceuse;
        original.face_mut(face).unwrap().uv_domain = Extent2d::from_bounds(-1.0, -1.0, 2.0, 2.0);
        executive.set_offset_brep(Brep::new());
        let mut offset = Some(plane(1.0));
        // Since topology.edgeuse_p3 (#117) builds the UV trim curve, the face prefix
        // reaches the plane's DropCurve before the domain boolean.
        assert_dependency(
            build_face(&mut executive, &mut original, faceuse, &mut offset),
            "SmSurface::DropCurve",
            "intersect.surface_drop_lib_p3",
        );
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
        let domain = surface.natural_uv_domain();
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
        executive
            .fill_gap_edgeuses(&kernel, EdgeId::from_index(u32::MAX), &[], &[])
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
        let mut records = BTreeMap::new();
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
}
