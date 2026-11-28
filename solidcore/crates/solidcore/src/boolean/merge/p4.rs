//! Manifold Boolean orchestration, face classification and region bookkeeping.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp` (2510–5099),
//! `source/SMLib/inc/SmMerge.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`manifold_boolean`] | `SmMerge::ManifoldBoolean` |
//! | [`ManifoldState`] | the intersection and extracted-component members of `SmMerge` |
//! | [`ManifoldIntersector`] | calls through `SmMerge::m_vTI` and shared merge stages |
//!
//! Deviations: operand-local arena ids replace pointers; nullable owned B-reps
//! express deletion. The kernel adapter reuses `MergeState<Relations>` and the
//! ported merge stages; it maintains no duplicate maps. Both operand kernels
//! are explicit; `None` for the second kernel means a shared context. Contexts are temporarily moved
//! together only for the existing paired mark-lock API. No input-driven recursion
//! is introduced; the two retry loops retain upstream's ten-attempt limits.
//! Not upstream (#16935): `MarkCommon` goes on to the next single vertex after a loop
//! closure that deleted no topology and left the vertex with two common edges, instead of
//! starting a new pass, so the ten passes are not a limit of ten gaps.
//! Not upstream (#17118): in a union, difference or intersection, a face found coincident at a
//! common edge with a face of the other body on a surface of another analytic kind, when the
//! pair is not made (the partner already classified, or `FixFoundFaceCoincidence` pairing
//! nothing), is classified once every common edge has been visited, as a copy of that part of
//! its partner (`resolve_tangent_patches`), instead of being left to a ray. Not upstream
//! (#21931): with no copy kept, such a face stays beside a kept partner, and one of the other
//! body that stays is made in its partner's region on the side the operation keeps the other
//! body's faces (`kept_patch_region`).
//! Global cache queue limits are absent (the port has no global caches).
//! The temporary origin shift goes through `merge_p1`, before any topology
//! mutation. Its inverse token spans the entire operation,
//! including error/imprint exits, extraction and deletion of either operand.
//! The complete subsequent stages remain here.
//! Component face copies use `brep::p7::copy_faces` with the native merge defaults.
//! Not upstream: `SM_BO_SLICE` gets its own coincident-face rule in steps 7.3e and 7.B
//! for a primary face bounding material (keep the pair on the sheet's `SM_OT_SAME` side
//! only) instead of upstream's difference rule, which keeps a face lying in the sheet in
//! both parts (issue #528, WR-254). After step 8 a Union, Difference or Intersection
//! whose result is a manifold solid with a nested shell runs
//! `SetRegionIsVoidFlagsForNestedSolids`, so every region's outer shell is first (and
//! its void flags follow the nesting): upstream's union that seals a void left the
//! void's shell first and failed `SmBrep::AssertValid` test 12 (the fork's fix,
//! tangency-labs/smlib `abastran/void-shell-check`; WR-265,
//! `plan/upstream-issues/WR-265--manifold-boolean-void-shell-first.md`). Own design
//! (WR-619): a union whose step 6 deletes a face between the infinite region and a solid
//! region, and after which that solid's side (the uses of its faces that stay) no longer lies
//! in a solid region (the solid merged into the outside), is refused with `SM_ERR` (the face
//! in the error) instead of returning that body. Where none of those uses is left, the union
//! is refused when it has no solid region at all.
//! Own design, not upstream: in step 6 a region that a deleted face joins to another, neither
//! with merge data (no face of the other body was copied into them, as when a union fills a
//! void exactly), takes the class a deleted face has on both sides ([`deleted_face_sides`]);
//! upstream leaves it the flag it had, so the filled void stayed void.

use std::collections::BTreeSet;

use super::p1::{self, MergeOptions};
use crate::error::boolean::{BooleanFailure, BooleanStage};
use crate::error::{Failure, Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::geom::attribute::{
    ids, Attribute, AttributeStore, Behavior, MergeRegionData, ObjectKey,
};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::{MarkLock, MarkType};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::BrepQuery;
use crate::topology::brep::p1 as bp;
use crate::topology::brep::p9::OrientSheetNote;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::p2 as face;
use crate::topology::topology_traverser::{self as traverser, Entity, Marks};
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, FaceuseId, RegionId, VertexId};
use crate::EntityRef;

mod arena_pair;
mod kernel_adapter;
mod notification_keys;
pub(crate) use kernel_adapter::{
    adopt_namespace_owners, entity_of_notification_key, notification_key,
};
pub use kernel_adapter::{KernelManifoldIntersector, ManifoldKernelAdapter};
pub use notification_keys::ManifoldNotificationKeys;

const OP: &str = "boolean::merge::p4::manifold_boolean";

/// State written by `ManifoldBoolean`, retained across calls as upstream members.
#[derive(Debug)]
pub struct ManifoldState {
    /// Primary edges directly involved in the intersection, before later copying.
    pub intersection_edges: Vec<EdgeId>,
    /// Primary vertices directly involved in the intersection.
    pub intersection_vertices: Vec<VertexId>,
    /// Primary common vertices, paired in order with `other_vertices`.
    pub vertices: Vec<VertexId>,
    /// Other common vertices.
    pub other_vertices: Vec<VertexId>,
    /// Primary common edges, paired in order with `other_edges`.
    pub edges: Vec<EdgeId>,
    /// Other common edges.
    pub other_edges: Vec<EdgeId>,
    /// Whether every common vertex has a common-edge count other than one.
    pub intersection_loop_closed: bool,
    /// One copied B-rep per connected primary face set for `ExtractSeparate`.
    pub primary_components: Vec<Brep>,
    /// One copied B-rep per connected other face set for `ExtractSeparate`.
    pub other_components: Vec<Brep>,
    /// Traversal marks of the primary operand.
    pub primary_marks: Marks,
    /// Traversal marks of the other operand.
    pub other_marks: Marks,
}

impl Default for ManifoldState {
    fn default() -> Self {
        Self {
            intersection_edges: Vec::new(),
            intersection_vertices: Vec::new(),
            vertices: Vec::new(),
            other_vertices: Vec::new(),
            edges: Vec::new(),
            other_edges: Vec::new(),
            intersection_loop_closed: true,
            primary_components: Vec::new(),
            other_components: Vec::new(),
            primary_marks: Marks::default(),
            other_marks: Marks::default(),
        }
    }
}

/// The two kernels of a Boolean, or one shared kernel when `other` is `None`.
pub struct BooleanKernels<'a> {
    /// Primary operand context, tolerances and caches.
    pub primary: &'a mut Kernel,
    /// Independently owned other context; `None` means the primary context.
    pub other: Option<&'a mut Kernel>,
}

impl BooleanKernels<'_> {
    fn other(&self) -> &Kernel {
        self.other.as_deref().unwrap_or(self.primary)
    }

    pub(crate) fn mark_action<T>(
        &mut self,
        action: impl FnOnce(&mut [crate::geom::context::Context]) -> Result<T>,
    ) -> Result<T> {
        if let Some(other) = self.other.as_deref_mut() {
            let mut contexts = [
                std::mem::take(&mut self.primary.context),
                std::mem::take(&mut other.context),
            ];
            let result = action(&mut contexts);
            let [first, second] = contexts;
            self.primary.context = first;
            other.context = second;
            result
        } else {
            action(std::slice::from_mut(&mut self.primary.context))
        }
    }

    fn doing_boolean(&mut self, value: bool) {
        self.primary.context.set_doing_boolean(value);
        if let Some(other) = self.other.as_deref_mut() {
            other.context.set_doing_boolean(value);
        }
    }
}

/// Outputs of `SmMerge::MakeFaceuse`; new faces remain ordered.
#[derive(Debug, Default)]
pub struct MadeFaceuse {
    /// Existing primary region receiving the new face.
    pub old_region: Option<RegionId>,
    /// New region, if inserting the face split a region.
    pub new_region: Option<RegionId>,
    /// Newly made primary faces.
    pub faces: Vec<FaceId>,
}

/// Outputs of `SmTopologyIntersector::FixGaps`.
#[derive(Debug, Default)]
pub struct FixedGaps {
    /// Whether the marking loop must restart.
    pub modified_topology: bool,
    /// Deleted primary vertices to remove from saved intersection lists.
    pub deleted_vertices: Vec<VertexId>,
    /// Deleted primary edges to remove from saved intersection lists.
    pub deleted_edges: Vec<EdgeId>,
}

/// Interface for calls into the canonical intersector and other merge units.
/// [`KernelManifoldIntersector`] implements the ported stages using one
/// `p1::MergeState<Relations>`, shared notification records and caller attributes.
/// Defaults replace only calls into genuinely unported dependencies.
pub trait ManifoldIntersector {
    /// `SmBrep::Transform` for the temporary scope's surviving operands. The
    /// adapter supplies notification records and selects the operand's live
    /// kernel from `other`; attributes are the same store used during the body.
    fn transform_brep(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        brep: &mut Brep,
        other: bool,
        attributes: &mut AttributeStore,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        scale_tolerance: bool,
    ) -> Result<()>;

    /// `CopyFaces` for extracted components, with native `use_merge=TRUE`.
    fn copy_component_faces(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        source: &mut Brep,
        other: bool,
        faces: &[FaceId],
        destination: &mut Brep,
        attributes: &mut AttributeStore,
    ) -> Result<()>;

    /// `GetMappedBrepObjects(SmEdge_TYPE)` before later topology copies.
    fn mapped_edges(&self) -> Result<Vec<EdgeId>>;
    /// `GetMappedBrepObjects(SmVertex_TYPE)` before later topology copies.
    fn mapped_vertices(&self) -> Result<Vec<VertexId>>;
    /// `GetCommonEdges`, in primary B-rep list order, paired with other mates.
    fn common_edges(&self, primary: &Brep, other: &Brep) -> Result<(Vec<EdgeId>, Vec<EdgeId>)>;
    /// `GetCommonVertices`, in primary B-rep list order.
    fn common_vertices(
        &self,
        primary: &Brep,
        other: &Brep,
    ) -> Result<(Vec<VertexId>, Vec<VertexId>)>;
    /// `GetOtherMate` for a primary edge.
    fn edge_mate(&self, edge: EdgeId) -> Option<EdgeId>;
    /// `GetOtherMate` for a primary face.
    fn face_mate(&self, face: FaceId) -> Option<FaceId>;
    /// `Relate` for coincident faces (the `topology_intersector_p1` port).
    fn relate_faces(
        &mut self,
        primary: &mut Brep,
        face: FaceId,
        other: &mut Brep,
        other_face: FaceId,
    ) -> Result<()>;
    /// `FixFoundFaceCoincidence`: adapter target is `merge_p1` with the live edge maps;
    /// a mismatch runs `MergeCoincidentSurfaces` on the operands with these kernels and
    /// the attributes its edits notify.
    fn fix_face_coincidence(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
        face: FaceId,
        other_face: FaceId,
    ) -> Result<(Vec<FaceId>, Vec<FaceId>, bool)>;
    /// `ProcessWiresAndShellVertices`: adapter target is `merge_p5` using the same mates.
    /// Build `VertexCopy` with these caller attributes and the adapter's records;
    /// no separate attribute store may be retained by the adapter.
    fn process_wires(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        state: &mut ManifoldState,
        attributes: &mut AttributeStore,
        operation: BooleanOperation,
        primary_mark: MarkType,
        other_mark: MarkType,
        keep: &mut Vec<EdgeId>,
    ) -> Result<()>;
    /// `sm_FindBrepRegionOfOtherFace`: adapter target is `merge_p1` with live edge maps.
    fn find_region_of_other_face(
        &mut self,
        k: &Kernel,
        primary: &mut Brep,
        other: &mut Brep,
        faceuse: FaceuseId,
    ) -> Result<Option<RegionId>>;
    /// `PropagateOtherSrcRegionAttribs`: adapter target is `merge_p1` with source lookups
    /// and the contexts' notification handlers. Its ordinary status is ignored.
    fn propagate_regions(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &Brep,
        other: &Brep,
        attributes: &mut AttributeStore,
        operation: BooleanOperation,
    ) -> Result<()>;
    /// `BooleanPostProcess(FALSE)`: adapter target is `merge_p1` with intersection lists
    /// and live mates rather than all topology objects.
    fn post_process(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        attributes: &mut AttributeStore,
        state: &ManifoldState,
        check_all_edges: bool,
    ) -> Result<()>;
    /// A globally distinct attribute/notification identity for an arena entity.
    /// `other` distinguishes equal ids in independently owned arenas.
    fn object_key(&self, other: bool, entity: MergeEntity) -> ObjectKey;
    /// Whether a partial-merge subset exists and excludes this other face's
    /// surface. Uses surface identity, not face identity.
    fn excluded_surface(&self, other: &Brep, face: FaceId) -> Result<bool>;

    /// `IntersectInsertRelate()`; all subsequent orchestration remains after it.
    fn intersect_insert_relate(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
    ) -> Result<()>;
    /// `RemoveRelationship(pFace,pOtherFace)`.
    fn remove_relationship(&mut self, face: FaceId, other_face: FaceId) -> Result<()>;
    /// Reset `m_bTopologyDeleted` before attempting to close a single vertex.
    fn clear_topology_deleted(&mut self);
    /// Current `m_bTopologyDeleted`, including changes during a failed close.
    fn topology_deleted(&self) -> bool;
    /// `CloseIntersectionLoop(pVertex,bMadeNewTopology,sDelVerts,sDelEdges)`. The
    /// callgraph omits this live callee; it is co-ported into
    /// `boolean.topology_intersector_p2`
    /// (`TopologyIntersector::close_intersection_loop`). `made_new_topology` is
    /// written only on success. `deleted` receives the primary vertices and edges its
    /// intersections deleted, also when it fails. Topology it inserts notifies through
    /// `attributes`. The retry/cleanup tail stays at the call site.
    fn close_intersection_loop(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
        vertex: VertexId,
        made_new_topology: &mut bool,
        deleted: &mut (Vec<VertexId>, Vec<EdgeId>),
    ) -> Result<()>;
    /// `FixGaps(sSingleVertices,bModifiedTopology,sDelVerts,sDelEdges)`.
    fn fix_gaps(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
        single_vertices: &[VertexId],
    ) -> Result<FixedGaps>;
    /// `MakeFaceuse(pFU1,pOldRegion,pNewRegion,pOrigRegion,&NewFs)`
    /// (`boolean::merge::p2::make_faceuse` with the same mates).
    /// Its copy notifications use this same caller store, including when a face
    /// already has a mate. Build `TopologyCopy` with these attributes.
    fn make_faceuse(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        other: &mut Brep,
        attributes: &mut AttributeStore,
        faceuse: FaceuseId,
        original_region: Option<RegionId>,
    ) -> Result<MadeFaceuse>;
    /// `DeleteFaces(sDelFaces,&sKeepAsWires)`; the wire list is non-null even empty.
    fn delete_faces(
        &mut self,
        kernels: &mut BooleanKernels<'_>,
        primary: &mut Brep,
        attributes: &mut AttributeStore,
        faces: &[FaceId],
        keep_wires: Option<&[EdgeId]>,
    ) -> Result<()>;
}

/// Typed entities used for attribute identities; ids stay operand-local.
#[derive(Debug, Clone, Copy)]
pub enum MergeEntity {
    /// A region.
    Region(RegionId),
    /// A face.
    Face(FaceId),
    /// An edge.
    Edge(EdgeId),
    /// A vertex.
    Vertex(VertexId),
}

/// `value`, or a defect naming the link `why` the kernel failed to keep (each call names
/// its own link; no shared phrase).
#[track_caller]
fn linked<T>(value: Option<T>, stage: BooleanStage, why: &'static str) -> Result<T> {
    value.ok_or_else(|| SolidError::boolean_null_defect(OP, stage, why))
}

/// The operand in `slot`, which the caller passes (an empty slot is the caller's).
fn operand(slot: &mut Option<Brep>) -> Result<&mut Brep> {
    slot.as_mut()
        .ok_or_else(|| SolidError::boolean(OP, BooleanFailure::MergeWithoutOperands))
}

fn ignored<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_unported_or_cancelled() => Err(error),
        Err(_) => Ok(None),
    }
}

fn pairs<A>(a: &[A], b: &[A]) -> Result<()> {
    if a.len() != b.len() {
        return Err(SolidError::invalid_input(
            OP,
            "unpaired common/coincident objects",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct Faces {
    keep: Vec<FaceId>,
    other_keep: Vec<FaceId>,
    delete: Vec<FaceId>,
    other_delete: Vec<FaceId>,
    primary_sets: Vec<Vec<FaceId>>,
    other_sets: Vec<Vec<FaceId>>,
    /// Not upstream (#17118): the tangent patches found at common edges, resolved by
    /// [`resolve_tangent_patches`] once every common edge has been visited.
    tangent_patches: Vec<TangentPatch>,
    /// Not upstream (#21931): each face of the other body that [`resolve_tangent_patches`]
    /// kept, with its partner face in the primary, for [`kept_patch_region`].
    kept_patches: Vec<(FaceId, FaceId)>,
}

/// Not upstream (#17118): a face found coincident at a common edge with a face of the other
/// body on a surface of another analytic kind, which the coincidence repair did not pair with
/// it (or whose partner was classified first). The source face is `of` when `reverse`, `pf`
/// otherwise; `keep` is what [`coincident_keep`] gives the pair.
#[derive(Clone, Copy)]
struct TangentPatch {
    reverse: bool,
    pf: FaceId,
    of: FaceId,
    keep: Option<bool>,
}

// `slice_same` (`Slice` with a primary face bounding material only; WR-254): that
// material lies on the side the sheet's `SM_OT_SAME` faceuse bounds. Upstream gives
// `SM_BO_SLICE` the difference rule, which never deletes a pair against a sheet (void on
// both sides), so a face lying in the slicing sheet is kept in both parts (issue #528,
// WR-254). The fix keeps such a pair on the sheet's same side only, the rule step 7.4d
// uses for radial sectors; other primary faces keep the difference rule.
fn coincident_keep(
    operation: BooleanOperation,
    same_side: bool,
    non_solid: bool,
    slice_same: Option<bool>,
) -> Option<bool> {
    match operation {
        BooleanOperation::Union | BooleanOperation::Intersection => Some(same_side || non_solid),
        BooleanOperation::Difference => Some(!same_side || non_solid),
        BooleanOperation::Slice => Some(slice_same.unwrap_or(!same_side || non_solid)),
        BooleanOperation::ExclusiveOr => Some(false),
        BooleanOperation::Merge | BooleanOperation::PartialMerge => Some(true),
        _ => None,
    }
}

fn region_keep(
    operation: BooleanOperation,
    other: bool,
    is_void: bool,
    slice_same: Option<bool>,
) -> Option<bool> {
    match operation {
        BooleanOperation::Union => Some(is_void),
        BooleanOperation::Difference => Some(if other { !is_void } else { is_void }),
        BooleanOperation::Slice => Some(if other {
            !is_void
        } else {
            slice_same.unwrap_or(is_void)
        }),
        BooleanOperation::Intersection => Some(!is_void),
        BooleanOperation::ExclusiveOr
        | BooleanOperation::Merge
        | BooleanOperation::PartialMerge => Some(true),
        _ => None,
    }
}

/// Whether the surfaces of the two faces are of different analytic kinds
/// ([`crate::surface::surface::Surface::is_other_analytic_kind`]): such faces coincide
/// only over a patch on which they stay within the tolerance.
fn surfaces_of_other_kinds(primary: &Brep, pf: FaceId, other: &Brep, of: FaceId) -> Result<bool> {
    fn surface(b: &Brep, f: FaceId) -> Result<&crate::surface::surface::Surface> {
        b.face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::boolean(OP, BooleanFailure::FaceWithoutSurface))
    }
    Ok(surface(primary, pf)?.is_other_analytic_kind(surface(other, of)?))
}

/// Not upstream (#17118, #21931): classifies each tangent patch still unclassified once every
/// common edge has been visited. A face on a surface of another kind than the face it coincides
/// with lies on it only over a tangent patch within the tolerance, so within the tolerance it is
/// a copy of that part of its partner. The operation keeps one copy of coincident faces or none
/// ([`coincident_keep`]). With one, the copy is the partner's when the partner is kept, so the
/// patch goes, and the patch's when the partner is deleted. With none (a difference of faces
/// facing the same way, a union or intersection of faces facing opposite ways), a deleted
/// partner takes the patch with it. A kept partner still covers the patch, since the imprint
/// drew no edge round the patch in it, so the space between the two, thinner than the
/// tolerance, lies against a face of the result, and the patch is kept to close it: a wedge
/// less a part whose round grazes the wedge's plane keeps that plane (it lies outside the
/// part), and the round's sliver on it closes the leftover beside it. Which side of the partner
/// the patch lies on is below the tolerance, so it is not read. A patch whose partner is
/// classified by a later patch is visited again, so the order of the list does not matter; a
/// patch whose partner is still unclassified when a pass classifies nothing more is left to the
/// ray, as before.
fn resolve_tangent_patches(
    kernels: &BooleanKernels<'_>,
    state: &mut ManifoldState,
    faces: &mut Faces,
    marks: (MarkType, MarkType),
) {
    let mut pending = std::mem::take(&mut faces.tangent_patches);
    loop {
        let count = pending.len();
        pending.retain(|&patch| !resolve_tangent_patch(kernels, state, faces, marks, patch));
        if pending.len() == count {
            break;
        }
    }
}

/// One patch of [`resolve_tangent_patches`]: whether it is settled (classified, or never to be
/// classified here); `false` while its partner is unclassified.
fn resolve_tangent_patch(
    kernels: &BooleanKernels<'_>,
    state: &mut ManifoldState,
    faces: &mut Faces,
    marks: (MarkType, MarkType),
    patch: TangentPatch,
) -> bool {
    let TangentPatch {
        reverse,
        pf,
        of,
        keep,
    } = patch;
    let Some(keep_copy) = keep else {
        return true;
    };
    let (source_marked, partner_kept, partner_deleted) = if reverse {
        (
            state
                .other_marks
                .is_marked(kernels.other(), Entity::Face(of), marks.1),
            faces.keep.contains(&pf),
            faces.delete.contains(&pf),
        )
    } else {
        (
            state
                .primary_marks
                .is_marked(kernels.primary, Entity::Face(pf), marks.0),
            faces.other_keep.contains(&of),
            faces.other_delete.contains(&of),
        )
    };
    if source_marked {
        return true;
    }
    if !partner_kept && !partner_deleted {
        return false;
    }
    let keep_source = keep_copy != partner_kept;
    if reverse {
        state
            .other_marks
            .mark(kernels.other(), Entity::Face(of), marks.1);
        if keep_source {
            faces.kept_patches.push((of, pf));
            faces.other_keep.push(of);
        } else {
            faces.other_delete.push(of);
        }
    } else {
        state
            .primary_marks
            .mark(kernels.primary, Entity::Face(pf), marks.0);
        if keep_source {
            faces.keep.push(pf);
        } else {
            faces.delete.push(pf);
        }
    }
    true
}

/// Not upstream (#21931): the region a face `f` of the other body is made in when
/// [`resolve_tangent_patches`] kept it. The patch lies on its partner within the tolerance, so
/// a radial sector at its edges or a point on them can fall on either side of the partner, and
/// made in the region on the wrong side it splits no region with the faces it bounds the result
/// with. Its side is the one where the operation keeps the other body's faces
/// ([`region_keep`]): of the two regions the partner lies between, the one on that side. `None`
/// for a face that is no kept patch, or a partner with that side on both or neither of its
/// faces (no face between a solid and its outside), which is placed as any other face.
fn kept_patch_region(
    primary: &Brep,
    operation: BooleanOperation,
    faces: &Faces,
    f: FaceId,
) -> Result<Option<RegionId>> {
    let Some(&(_, partner)) = faces.kept_patches.iter().find(|&&(of, _)| of == f) else {
        return Ok(None);
    };
    let Some(fu) = face::faceuses(primary, partner)?.0 else {
        return Ok(None);
    };
    let (r1, r2) = region_pair(primary, fu)?;
    let kept = |r: RegionId| -> Result<bool> {
        Ok(region_keep(operation, true, void(primary, r)? != 0, None) == Some(true))
    };
    Ok(match (kept(r1)?, kept(r2)?) {
        (true, false) => Some(r1),
        (false, true) => Some(r2),
        _ => None,
    })
}

/// Not upstream (#17118): a bound on how far any point of `f` lies from the plane of
/// `partner_face`, proved rather than sampled, for a face on a cylinder or cone; `None` for any
/// other pair of kinds, or where the bound below does not apply.
///
/// Every point of the cone is `O + h A + R(h) (cos t E1 + sin t E2)` with `R(h) = r + h tan(a)`,
/// and the plane's signed distance `n . (X - P)` there is `C + D h + R(h) S(t)` with `S` a
/// sinusoid. With `R` positive, that is linear in `h` for each `t`, so over a box
/// `[t0, t1] x [h0, h1]` it is extreme at `h0` or `h1` and at `S`'s extremes over `[t0, t1]`,
/// which are at the ends or at `S`'s crest or trough. The box comes from the face's boundary:
/// its vertices, each line edge's ends and every control point of each B-spline edge's Bezier
/// segments (a B-spline with positive weights lies in the convex hull of those). `h` is linear,
/// so the hull's `h` range holds the boundary's. The angles are measured from the points' mean
/// direction about the axis and must all lie within a quarter turn of it, so the hull lies in a
/// half-space bounded by a plane through the axis and its angles lie between the points'. A face
/// with one loop that does not wind round the axis is the region inside that loop, so it lies in
/// the box of its boundary. The box is widened by the face's edge tolerances, since the edges
/// lie only within those of the cone.
fn cone_face_gap_to_plane(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    partner: &Brep,
    partner_face: FaceId,
) -> Result<Option<f64>> {
    use crate::curve::curve::CurveKind;
    use crate::surface::implicit::{AnalyticSupport, Implicit};
    let implicit = |b: &Brep, f: FaceId| -> Result<Option<Implicit>> {
        Ok(b.face(f)?
            .surface
            .as_ref()
            .and_then(AnalyticSupport::of)
            .map(|support| support.implicit()))
    };
    let (
        Some(Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        }),
        Some(Implicit::Plane {
            origin: plane_origin,
            normal,
        }),
    ) = (implicit(b, f)?, implicit(partner, partner_face)?)
    else {
        return Ok(None);
    };
    if face::loops(b, f, false, None)?.len() != 1 {
        return Ok(None);
    }
    // the boundary's hull points, and the widest edge tolerance
    let mut points = Vec::new();
    let mut widen = 0.0_f64;
    for v in crate::topology::face::p4::vertices(b, f, None, false, None)? {
        points.push(vertex::point(b, v)?);
    }
    for e in crate::topology::face::p4::edges(b, f, None, false, None)? {
        let edge = b.edge(e)?;
        widen = widen.max(edge.tolerance);
        let Some(curve) = edge.curve.as_ref() else {
            return Ok(None);
        };
        match curve.kind() {
            CurveKind::Line(_) => {
                for t in [0.0, 1.0] {
                    points.push(curve.evaluate_point(k, edge.interval.evaluate(t))?);
                }
            }
            CurveKind::BSpline(spline) => {
                let Some(nurb) = spline.nurb.as_ref() else {
                    return Ok(None);
                };
                for segment in nurb.decompose_bez()? {
                    for c in segment.cpts() {
                        let w = if c.w == crate::nurbs::math::NOW {
                            1.0
                        } else {
                            c.w
                        };
                        if w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                            return Ok(None);
                        }
                        points.push(Vec3::new(c.x / w, c.y / w, c.z / w));
                    }
                }
            }
            _ => return Ok(None),
        }
    }
    Ok(cone_box_gap(
        (origin, axis, radius, tan_angle),
        (plane_origin, normal),
        &points,
        widen,
    ))
}

/// The bound of [`cone_face_gap_to_plane`] for a face whose boundary lies in the convex hull of
/// `points`, within `widen` of the cone `(O, A, r, tan(a))`, against the plane `(P, n)`.
///
/// Rounding, with `g(m)` the bound `m u / (1 - m u)` on `m` roundings (`u` the unit roundoff)
/// and `s` the size of every coordinate, `|O| + |P| + max |X|`:
///
/// - Each point's height `(X - O) . A` and radial vector take at most 8 roundings of terms
///   below `s`, so they are within `g(8) s`. The frame `E1` (the normalised mean) and
///   `E2 = A x E1` are within `g(8)` of an orthonormal frame, which turns each point's angle
///   by at most `g(16) s / rho` with its components' error, `rho` the least radial distance,
///   plus two units in the last place of `atan2`. The box is widened by those margins, so it
///   holds the exact heights and angles.
/// - The distance `C + D h + R(h) S(t)` is computed from `C` (5 roundings), `D` (3), `R(h)` (3),
///   and `S(t)`, whose coefficients carry the frame's `g(16)` and whose sine and cosine are
///   within one unit in the last place each; with the sum's own 3 roundings, each value is
///   within `g(32)` of the sizes of its terms, at most `|O| + |P| + max |h| + max R(h)`.
///   That is added to the bound.
/// - A crest or trough is counted whenever it falls within `1e-9` of the box. Counting one
///   that falls just outside only raises the bound, since `|S|` never exceeds its amplitude.
fn cone_box_gap(
    (origin, axis, radius, tan_angle): (Vec3, Vec3, f64, f64),
    (plane_origin, normal): (Vec3, Vec3),
    points: &[Vec3],
    widen: f64,
) -> Option<f64> {
    use crate::nurbs::math::gamma;
    use std::cmp::Ordering;
    use std::f64::consts::PI;
    // each point's height and radial vector about the axis
    let about = |x: Vec3| {
        let v = x - origin;
        let h = v.dot(axis);
        (h, v - axis * h)
    };
    let mean = points
        .iter()
        .fold(Vec3::new(0.0, 0.0, 0.0), |m, &x| m + about(x).1);
    if points.is_empty() || mean.length() == 0.0 {
        return None;
    }
    let s = origin.length()
        + plane_origin.length()
        + points.iter().map(|x| x.length()).fold(0.0_f64, f64::max);
    let e1 = mean * (1.0 / mean.length());
    let e2 = axis.cross(e1);
    let (mut h0, mut h1, mut t0, mut t1) = (f64::MAX, f64::MIN, 0.0_f64, 0.0_f64);
    let mut rho = f64::MAX;
    for &x in points {
        let (h, v) = about(x);
        let t = v.dot(e2).atan2(v.dot(e1));
        if t.abs().partial_cmp(&(0.5 * PI)) != Some(Ordering::Less) {
            return None;
        }
        (h0, h1, t0, t1) = (h0.min(h), h1.max(h), t0.min(t), t1.max(t));
        rho = rho.min(v.length());
    }
    let r_at = |h: f64| radius + tan_angle * h;
    let r_min = r_at(h0).min(r_at(h1));
    if r_min.partial_cmp(&widen) != Some(Ordering::Greater)
        || rho.partial_cmp(&(gamma(16) * s)) != Some(Ordering::Greater)
    {
        return None;
    }
    // a point within `widen` of the cone where its radius is `R` turns by at most this
    let spread = (widen / (r_min - widen)).min(1.0).asin();
    let margin_h = widen + gamma(8) * s;
    let margin_t = spread + gamma(16) * s / rho + 4.0 * f64::EPSILON;
    let (h0, h1, t0, t1) = (h0 - margin_h, h1 + margin_h, t0 - margin_t, t1 + margin_t);
    let (r0, r1) = (r_at(h0), r_at(h1));
    if !(r0 > 0.0 && r1 > 0.0 && t1 - t0 < PI) {
        return None;
    }
    // the signed distance C + D h + R(h) (alpha cos t + beta sin t)
    let c = normal.dot(origin - plane_origin);
    let d = normal.dot(axis);
    let (alpha, beta) = (normal.dot(e1), normal.dot(e2));
    let sine = |t: f64| alpha * t.cos() + beta * t.sin();
    let crest = beta.atan2(alpha);
    let (mut s_min, mut s_max) = (sine(t0).min(sine(t1)), sine(t0).max(sine(t1)));
    let amplitude = alpha.hypot(beta);
    for (peak, value) in [(crest, amplitude), (crest + PI, -amplitude)] {
        // the peak's turn nearest the box
        let at = peak + 2.0 * PI * ((0.5 * (t0 + t1) - peak) / (2.0 * PI)).round();
        if t0 - 1e-9 <= at && at <= t1 + 1e-9 {
            s_min = s_min.min(value);
            s_max = s_max.max(value);
        }
    }
    let mut gap = 0.0_f64;
    for (h, r) in [(h0, r0), (h1, r1)] {
        gap = gap
            .max((c + d * h + r * s_max).abs())
            .max((c + d * h + r * s_min).abs());
    }
    let size = origin.length() + plane_origin.length() + h0.abs().max(h1.abs()) + r0.max(r1);
    Some(gap + gamma(32) * size)
}

/// Not upstream (#17118): whether every one of `points` (the source face's vertices and three
/// points along each of its edges) classifies to the partner face `f` of `b`, its inside, an
/// edge or a vertex, within `tol`, so that the source face lies within the partner face's
/// trimmed extent, not only on its surface. The classification builds the face's UV trim
/// curves, so it runs on a scratch copy of `b`. A point the classification cannot place
/// counts as outside.
fn points_in_face(k: &Kernel, b: &Brep, f: FaceId, points: &[Vec3], tol: f64) -> Result<bool> {
    use crate::intersect::point_class::PointClassification;
    use crate::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
    let mut scratch = b.clone_sharing_surface_caches();
    for &point in points {
        let mut pc = PointClassification::new(tol);
        match point_3d_classify(
            k,
            &mut scratch,
            f,
            point,
            tol,
            false,
            &mut pc,
            Point3dClassifyOptions::default(),
        ) {
            Ok(()) => {}
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        }
        if !matches!(
            pc.point_class(),
            PointClassType::Face | PointClassType::Edge | PointClassType::Vertex
        ) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The points of face `f` that [`points_in_face`] classifies: its vertices and the points a
/// quarter, half and three quarters along each edge.
fn face_boundary_points(k: &Kernel, b: &Brep, f: FaceId) -> Result<Vec<Vec3>> {
    let mut points = Vec::new();
    for v in crate::topology::face::p4::vertices(b, f, None, false, None)? {
        points.push(vertex::point(b, v)?);
    }
    for e in crate::topology::face::p4::edges(b, f, None, false, None)? {
        let edge = b.edge(e)?;
        if let Some(curve) = edge.curve.as_ref() {
            for t in [0.25, 0.5, 0.75] {
                points.push(curve.evaluate_point(k, edge.interval.evaluate(t))?);
            }
        }
    }
    Ok(points)
}

/// Not upstream (#17118): whether the source face of a tangent patch (`of` when `reverse`, `pf`
/// otherwise) is a copy of part of its partner face: it lies within the tolerance of the
/// partner's surface everywhere, which is proved only for a face on a cylinder or cone against
/// a plane ([`cone_face_gap_to_plane`]), and its boundary lies within the partner face's trimmed
/// extent ([`points_in_face`]). Any other face is left to the ray.
fn within_partner(
    kernels: &BooleanKernels<'_>,
    primary: &Brep,
    pf: FaceId,
    other: &Brep,
    of: FaceId,
    reverse: bool,
) -> Result<bool> {
    let (sk, sb, sf, pk, pb, ppf) = if reverse {
        (kernels.other(), other, of, &*kernels.primary, primary, pf)
    } else {
        (&*kernels.primary, primary, pf, kernels.other(), other, of)
    };
    let tol = face::tolerance(sb, sf)?.max(face::tolerance(pb, ppf)?);
    if !cone_face_gap_to_plane(sk, sb, sf, pb, ppf)?.is_some_and(|g| g <= tol) {
        return Ok(false);
    }
    points_in_face(pk, pb, ppf, &face_boundary_points(sk, sb, sf)?, tol)
}

fn region_pair(b: &Brep, fu: FaceuseId) -> Result<(RegionId, RegionId)> {
    Ok((
        linked(
            eu::faceuse_region(b, fu)?,
            BooleanStage::Classify,
            "a face use of a face lies in no region, though the kernel keeps every face \
             use's shell in a region",
        )?,
        linked(
            eu::faceuse_region(
                b,
                linked(
                    eu::faceuse_mate(b, fu)?,
                    BooleanStage::Classify,
                    "a face use has no mate, though the kernel makes every face with a pair \
                     of face uses",
                )?,
            )?,
            BooleanStage::Classify,
            "a face use's mate lies in no region, though the kernel keeps every face use's \
             shell in a region",
        )?,
    ))
}

/// Not upstream (WR-619): a face to be deleted with the infinite region on one side and a
/// solid region on the other, and the face uses, on faces that stay, that bound that solid.
struct SolidLeak {
    face: FaceId,
    region: RegionId,
    bounding: Vec<FaceuseId>,
}

/// Not upstream (WR-619): one [`SolidLeak`] per solid region that a face of `delete` lies
/// between it and the infinite region (the first such face names it). Deleting such a face
/// joins the solid to the outside unless the other body's solid lies just across it.
fn solid_leaks(b: &Brep, delete: &[FaceId]) -> Result<Vec<SolidLeak>> {
    let Some(infinite) = b.infinite_region() else {
        return Ok(Vec::new());
    };
    let mut leaks: Vec<SolidLeak> = Vec::new();
    for &f in delete {
        let Some(fu) = face::faceuses(b, f)?.0 else {
            continue;
        };
        let (r1, r2) = region_pair(b, fu)?;
        let inside = match (r1 == infinite, r2 == infinite) {
            (true, false) => r2,
            (false, true) => r1,
            _ => continue,
        };
        if void(b, inside)? == 0 && !leaks.iter().any(|l| l.region == inside) {
            leaks.push(SolidLeak {
                face: f,
                region: inside,
                bounding: Vec::new(),
            });
        }
    }
    if leaks.is_empty() {
        return Ok(leaks);
    }
    let deleted: BTreeSet<FaceId> = delete.iter().copied().collect();
    for f in bp::faces(b, None)? {
        if deleted.contains(&f) {
            continue;
        }
        let (fu1, fu2) = face::faceuses(b, f)?;
        for fu in [fu1, fu2].into_iter().flatten() {
            let Some(r) = eu::faceuse_region(b, fu)? else {
                continue;
            };
            for leak in leaks.iter_mut().filter(|l| l.region == r) {
                leak.bounding.push(fu);
            }
        }
    }
    Ok(leaks)
}

/// Not upstream (WR-619): whether the solid a [`SolidLeak`]'s face bounded still lies in a
/// solid region. Each face use that bounded it and is left must lie in a region that is
/// neither the infinite one nor a void: the solid's material is in one of the operands, so
/// a use whose side ends outside or in a void shows a wrong union. When no such use is left
/// the solid cannot be located; the test is then the whole-body one, `b` has a solid region
/// ([`has_solid_region`]), so that case refuses exactly what it refused before the per-leak
/// check.
fn leak_stays_solid(b: &Brep, leak: &SolidLeak) -> Result<bool> {
    let infinite = b.infinite_region();
    let mut located = false;
    for &fu in &leak.bounding {
        if b.faceuses().get(fu).is_none() {
            continue;
        }
        let Some(r) = eu::faceuse_region(b, fu)? else {
            continue;
        };
        if Some(r) == infinite || void(b, r)? != 0 {
            return Ok(false);
        }
        located = true;
    }
    if located {
        return Ok(true);
    }
    has_solid_region(b)
}

/// Not upstream (WR-619): whether `b` has a solid region other than the infinite one.
fn has_solid_region(b: &Brep) -> Result<bool> {
    let infinite = b.infinite_region();
    for r in bp::regions(b, None)? {
        if Some(r) != infinite && void(b, r)? == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn void(b: &Brep, r: RegionId) -> Result<i32> {
    Ok(b.region(r)?.is_void())
}

fn common_count(
    intersector: &impl ManifoldIntersector,
    b: &Brep,
    v: VertexId,
) -> Result<(usize, bool)> {
    let mut count = 0;
    let mut lamina = false;
    for e in vertex::edges(b, v, None)? {
        lamina |= edge::is_lamina(b, e)?;
        if intersector.edge_mate(e).is_some() {
            count += 1;
        }
        if edge::is_closed(b, e)? {
            count += 1;
        }
    }
    Ok((count, lamina))
}

fn refresh_common(
    intersector: &impl ManifoldIntersector,
    primary: &Brep,
    other: &Brep,
    state: &mut ManifoldState,
) -> Result<()> {
    (state.edges, state.other_edges) = intersector.common_edges(primary, other)?;
    (state.vertices, state.other_vertices) = intersector.common_vertices(primary, other)?;
    pairs(&state.edges, &state.other_edges)?;
    pairs(&state.vertices, &state.other_vertices)
}

fn mark_common(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Brep,
    other: &mut Brep,
    state: &mut ManifoldState,
    intersector: &mut impl ManifoldIntersector,
    lock: &MarkLock,
    manifold: bool,
    attributes: &mut AttributeStore,
) -> Result<()> {
    let mark = lock.mark_type1();
    let other_mark = lock.mark_type2();
    for _ in 0..10 {
        kernels.mark_action(|contexts| lock.new_mark(contexts))?;
        refresh_common(intersector, primary, other, state)?;
        for (&e, &oe) in state.edges.iter().zip(&state.other_edges) {
            state
                .primary_marks
                .mark(kernels.primary, Entity::Edge(e), mark);
            state
                .other_marks
                .mark(kernels.other(), Entity::Edge(oe), other_mark);
        }
        let mut single = Vec::new();
        let mut done = true;
        for (&v, &ov) in state.vertices.iter().zip(&state.other_vertices) {
            state
                .primary_marks
                .mark(kernels.primary, Entity::Vertex(v), mark);
            state
                .other_marks
                .mark(kernels.other(), Entity::Vertex(ov), other_mark);
            let (count, lamina) = common_count(intersector, primary, v)?;
            if count != 1 || lamina {
                continue;
            }
            let mut made = false;
            intersector.clear_topology_deleted();
            if manifold {
                // This could delete entities in our stored pointers:
                let mut deleted = (Vec::new(), Vec::new());
                let status = ignored(intersector.close_intersection_loop(
                    kernels,
                    primary,
                    other,
                    attributes,
                    v,
                    &mut made,
                    &mut deleted,
                ));
                state
                    .intersection_vertices
                    .retain(|x| !deleted.0.contains(x));
                state.intersection_edges.retain(|x| !deleted.1.contains(x));
                if status?.is_none() {
                    made = false;
                }
            }
            // Not upstream (#16935): a closure that deleted nothing and gave `v` a second
            // common edge (so `v` no longer ends the loop) leaves the vertex list valid and
            // closed its gap, so the pass goes on to the loop's other ends. Upstream ended
            // the pass at each closure, so its ten passes closed at most ten gaps: a cut
            // with twenty left the rest open and classified whole faces across them. A
            // closure that made topology but left `v` an end ends the pass as upstream: it
            // closed nothing, and going on would repeat such closures at the other ends
            // too (#1036's coincident skins, whose closures split edges each pass without
            // relating them).
            if made
                && !intersector.topology_deleted()
                && common_count(intersector, primary, v)?.0 != 1
            {
                done = false;
                continue;
            }
            if made || intersector.topology_deleted() {
                done = false;
                break;
            }
            single.push(v);
        }
        if done && single.len() > 1 && manifold {
            let fixed = intersector.fix_gaps(kernels, primary, other, attributes, &single)?;
            state
                .intersection_vertices
                .retain(|v| !fixed.deleted_vertices.contains(v));
            state
                .intersection_edges
                .retain(|e| !fixed.deleted_edges.contains(e));
            if fixed.modified_topology {
                done = false;
            }
        }
        if done {
            break;
        }
    }
    (state.vertices, state.other_vertices) = intersector.common_vertices(primary, other)?;
    pairs(&state.vertices, &state.other_vertices)?;
    state.intersection_loop_closed = true;
    for &v in &state.vertices {
        if common_count(intersector, primary, v)?.0 == 1 {
            state.intersection_loop_closed = false;
            break;
        }
    }
    Ok(())
}

// One direction of the common-edge face classification. Coincidence is always
// recorded primary-to-other, including when reached from an other edge use.
fn classify_edge_faces(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Brep,
    other: &mut Brep,
    attributes: &mut AttributeStore,
    state: &mut ManifoldState,
    intersector: &mut impl ManifoldIntersector,
    operation: BooleanOperation,
    marks: (MarkType, MarkType),
    edge_id: EdgeId,
    other_edge: EdgeId,
    reverse: bool,
    faces: &mut Faces,
) -> Result<bool> {
    let edgeuses = if reverse {
        edge::edgeuses(other, other_edge, None, None)?
    } else {
        edge::edgeuses(primary, edge_id, None, None)?
    };
    for &edgeuse in edgeuses.iter().step_by(2) {
        let (source, source_marks, source_k, mark) = if reverse {
            (&*other, &state.other_marks, kernels.other(), marks.1)
        } else {
            (&*primary, &state.primary_marks, &*kernels.primary, marks.0)
        };
        if eu::is_shell_edgeuse(source, edgeuse)? {
            continue;
        }
        let source_face = linked(
            eu::face(source, edgeuse)?,
            BooleanStage::Classify,
            "an edge use that is not a shell edge use has no face, though only shell edge \
             uses lie outside a face",
        )?;
        if source_marks.is_marked(source_k, Entity::Face(source_face), mark) {
            continue;
        }
        let sector = if reverse {
            ignored(edge::find_radial_sector_between(
                kernels.primary,
                primary,
                edge_id,
                Some(&mut *other),
                edgeuse,
                None,
                OrientType::Same,
                None,
            ))?
        } else {
            ignored(edge::find_radial_sector_between(
                kernels.other(),
                other,
                other_edge,
                Some(&mut *primary),
                edgeuse,
                None,
                OrientType::Same,
                None,
            ))?
        };
        let Some(sector) = sector else {
            continue;
        };
        if let Some(found) = sector.coincident_faceuse {
            let source_fu = linked(
                eu::faceuse(if reverse { other } else { primary }, edgeuse)?,
                BooleanStage::Classify,
                "an edge use of a face has no face use, though the kernel links every edge use \
                 of a face to its face use",
            )?;
            let (pf, of, pf_regions, of_regions) = if reverse {
                (
                    linked(
                        eu::faceuse_face(primary, found)?,
                        BooleanStage::Classify,
                        "the coincident face use the radial sector found has no face, though \
                         the kernel links every face use to its face",
                    )?,
                    source_face,
                    region_pair(primary, found)?,
                    region_pair(other, source_fu)?,
                )
            } else {
                (
                    source_face,
                    linked(
                        eu::faceuse_face(other, found)?,
                        BooleanStage::Classify,
                        "the coincident face use the radial sector found has no face, though \
                         the kernel links every face use to its face",
                    )?,
                    region_pair(primary, source_fu)?,
                    region_pair(other, found)?,
                )
            };
            // tangent faces only meet at the edge: coincident when their surfaces are (WR-755)
            if !sector.zero_sector && !p1::faces_coincide(kernels.primary, primary, pf, other, of)?
            {
                continue;
            }
            let first_equal = void(primary, pf_regions.0)? == void(other, of_regions.0)?;
            let second_equal = void(primary, pf_regions.1)? == void(other, of_regions.1)?;
            let same_side = first_equal && second_equal;
            let non_solid = first_equal && !second_equal;
            // the coincident faceuses point the same way (FindRadialSector): when the
            // primary face bounds material, its faceuse faces it if its region is solid
            let slice_same = if operation == BooleanOperation::Slice {
                let solid = void(primary, pf_regions.0)? == 0;
                if solid == (void(primary, pf_regions.1)? == 0) {
                    None
                } else {
                    let other_fu = if reverse { source_fu } else { found };
                    Some(solid == (eu::faceuse_orientation(other, other_fu)? == OrientType::Same))
                }
            } else {
                None
            };
            // Not upstream (#17118): a face on a surface of another kind than the face it
            // coincides with lies on it only over a tangent patch within the tolerance,
            // whose boundary the imprint need not draw in that face. Such a pair is often
            // not one `FixFoundFaceCoincidence` accepts (it pairs nothing), or the partner
            // was classified at another edge first. The patch is classified once every
            // common edge has been visited (`resolve_tangent_patches`). Left to the ray
            // instead, it was classified from a point within the tolerance of the other
            // boundary, where the answer can go either way: a sliver of a groove's round
            // lying in a wedge thinner than the tolerance was kept by a cut, and the result
            // was not closed.
            let tangent_patch = matches!(
                operation,
                BooleanOperation::Union
                    | BooleanOperation::Difference
                    | BooleanOperation::Intersection
            ) && surfaces_of_other_kinds(primary, pf, other, of)?
                && within_partner(kernels, primary, pf, other, of, reverse)?;
            let keep = coincident_keep(operation, same_side, non_solid, None);
            if if reverse {
                state
                    .primary_marks
                    .is_marked(kernels.primary, Entity::Face(pf), marks.0)
            } else {
                state
                    .other_marks
                    .is_marked(kernels.other(), Entity::Face(of), marks.1)
            } {
                if tangent_patch {
                    faces.tangent_patches.push(TangentPatch {
                        reverse,
                        pf,
                        of,
                        keep,
                    });
                }
                continue;
            }
            let (compatible, other_compatible, modified) =
                intersector.fix_face_coincidence(kernels, primary, other, attributes, pf, of)?;
            if modified {
                return Ok(false);
            }
            let source_paired = if reverse {
                other_compatible.contains(&of)
            } else {
                compatible.contains(&pf)
            };
            if tangent_patch && !source_paired {
                faces.tangent_patches.push(TangentPatch {
                    reverse,
                    pf,
                    of,
                    keep,
                });
            }
            pairs(&compatible, &other_compatible)?;
            if operation == BooleanOperation::ExtractSeparate {
                faces.primary_sets.push(compatible.clone());
            }
            for (&pf, &of) in compatible.iter().zip(&other_compatible) {
                intersector.relate_faces(primary, pf, other, of)?;
                state
                    .primary_marks
                    .mark(kernels.primary, Entity::Face(pf), marks.0);
                state
                    .other_marks
                    .mark(kernels.other(), Entity::Face(of), marks.1);
                if let Some(keep) = coincident_keep(operation, same_side, non_solid, slice_same) {
                    if keep {
                        faces.keep.push(pf);
                        faces.other_keep.push(of);
                    } else {
                        faces.delete.push(pf);
                        faces.other_delete.push(of);
                    }
                }
            }
        } else if let Some(found) = sector.radial_edgeuse {
            let mut collected = Vec::new();
            let (source, source_marks, source_k, mark) = if reverse {
                (&*other, &mut state.other_marks, kernels.other(), marks.1)
            } else {
                (
                    &*primary,
                    &mut state.primary_marks,
                    &*kernels.primary,
                    marks.0,
                )
            };
            traverser::collect_faces(
                source_k,
                source,
                source_marks,
                Some(source_face),
                mark,
                &mut collected,
                None,
                None,
            )?;
            if operation == BooleanOperation::ExtractSeparate {
                if reverse {
                    faces.other_sets.push(collected.clone());
                } else {
                    faces.primary_sets.push(collected.clone());
                }
            }
            let opposite = if reverse { &*primary } else { &*other };
            let r = linked(
                eu::shell_region(
                    opposite,
                    linked(
                        eu::shell(opposite, found)?,
                        BooleanStage::Classify,
                        "the radial edge use the sector found has no shell, though the sector \
                         walks only the edge uses of faces, each of which lies in a shell",
                    )?,
                )?,
                BooleanStage::Classify,
                "the shell of the radial edge use the sector found lies in no region, though \
                 the kernel keeps every shell in a region",
            )?;
            let slice_same = if !reverse && operation == BooleanOperation::Slice {
                Some(
                    eu::faceuse_orientation(
                        opposite,
                        linked(
                            eu::faceuse(opposite, found)?,
                            BooleanStage::Classify,
                            "the radial edge use the sector found has no face use, though the \
                             sector walks only the edge uses of faces",
                        )?,
                    )? == OrientType::Same,
                )
            } else {
                None
            };
            if let Some(keep) = region_keep(operation, reverse, void(opposite, r)? != 0, slice_same)
            {
                match (reverse, keep) {
                    (false, true) => faces.keep.extend(collected),
                    (false, false) => faces.delete.extend(collected),
                    (true, true) => faces.other_keep.extend(collected),
                    (true, false) => faces.other_delete.extend(collected),
                }
            }
        } else {
            return Err(SolidError::boolean(OP, BooleanFailure::NoRadialEdgeUse));
        }
    }
    Ok(true)
}

fn classify_remaining(
    kernels: &BooleanKernels<'_>,
    primary: &mut Brep,
    other: &mut Brep,
    state: &mut ManifoldState,
    operation: BooleanOperation,
    marks: (MarkType, MarkType),
    faces: &mut Faces,
) -> Result<()> {
    for reverse in [false, true] {
        let (source, opposite, source_marks, source_k, opposite_k, mark) = if reverse {
            (
                &*other,
                &mut *primary,
                &mut state.other_marks,
                kernels.other(),
                &*kernels.primary,
                marks.1,
            )
        } else {
            (
                &*primary,
                &mut *other,
                &mut state.primary_marks,
                &*kernels.primary,
                kernels.other(),
                marks.0,
            )
        };
        // The pass only reads `opposite`, so one query (and its cache) serves every face;
        // its geometry owners named by `opposite`'s naming scheme and proven owners.
        let query = BrepQuery::of_brep(opposite);
        for f in bp::faces(source, None)? {
            if source_marks.is_marked(source_k, Entity::Face(f), mark) {
                continue;
            }
            if !reverse
                && matches!(
                    operation,
                    BooleanOperation::ExclusiveOr
                        | BooleanOperation::Merge
                        | BooleanOperation::PartialMerge
                )
            {
                faces.keep.push(f);
                continue;
            }
            let mut collected = Vec::new();
            if !reverse {
                traverser::collect_faces(
                    source_k,
                    source,
                    source_marks,
                    Some(f),
                    mark,
                    &mut collected,
                    None,
                    None,
                )?;
                if operation == BooleanOperation::ExtractSeparate {
                    faces.primary_sets.push(collected);
                    continue;
                }
            }
            let edges = traverser::face_edges(source, f)?;
            let mut region = None;
            for &edge_id in &edges {
                // Upstream deliberately/repeatedly uses sFaceEdges[0] in the
                // primary pass; the other pass uses j and a different fraction.
                // (inside the loop over `edges`, so the list has a first edge)
                let selected = if reverse {
                    edge_id
                } else {
                    edges.first().copied().unwrap_or(edge_id)
                };
                let e = source.edge(selected)?;
                let curve = e
                    .curve
                    .as_ref()
                    .ok_or_else(|| SolidError::boolean(OP, BooleanFailure::EdgeWithoutCurve))?;
                let point = curve.evaluate_point(
                    source_k,
                    e.interval
                        .evaluate(if reverse { 0.456789 } else { 0.45678 }),
                )?;
                // Upstream ignores a failed `Point3DClassify` and tries the next edge.
                let located = ignored(p1::classify_point_editing_disabled(
                    opposite_k,
                    &query,
                    opposite,
                    point,
                    crate::math::EFF_ZERO,
                    false,
                ))?;
                let Some(classification) = located else {
                    continue;
                };
                // The other pass checks PC_REGION; the primary pass only casts
                // GetObject(). These are caller decisions, not query behaviour.
                if reverse && classification.point_class() != PointClassType::Region {
                    continue;
                }
                if let Some(EntityRef::Region(r)) = classification.object() {
                    region = Some(r);
                    break;
                }
            }
            let Some(region) = region else {
                continue;
            };
            if reverse {
                traverser::collect_faces(
                    source_k,
                    source,
                    source_marks,
                    Some(f),
                    mark,
                    &mut collected,
                    None,
                    None,
                )?;
                if operation == BooleanOperation::ExtractSeparate {
                    faces.other_sets.push(collected.clone());
                }
            }
            if let Some(keep) = region_keep(operation, reverse, void(opposite, region)? != 0, None)
            {
                match (reverse, keep) {
                    (false, true) => faces.keep.extend(collected),
                    (false, false) => faces.delete.extend(collected),
                    (true, true) => faces.other_keep.extend(collected),
                    (true, false) => faces.other_delete.extend(collected),
                }
            }
        }
    }
    Ok(())
}

fn unique<T: Copy + PartialEq>(values: &mut Vec<T>) {
    // SmTArray::RemoveDuplicates retains the LAST occurrence of each entry.
    // Reverse/retain/reverse preserves precisely that order without pointer sort.
    let mut seen = Vec::new();
    values.reverse();
    values.retain(|v| {
        if seen.contains(v) {
            false
        } else {
            seen.push(*v);
            true
        }
    });
    values.reverse();
}

fn repair_lists(
    primary: &Brep,
    other: &Brep,
    operation: BooleanOperation,
    faces: &mut Faces,
) -> Result<()> {
    for list in [
        &mut faces.keep,
        &mut faces.delete,
        &mut faces.other_keep,
        &mut faces.other_delete,
    ] {
        unique(list);
    }
    let actual = bp::faces(primary, None)?;
    if actual.len() as i128 - faces.keep.len() as i128 - faces.delete.len() as i128 != 0 {
        faces.keep.retain(|f| actual.contains(f));
        faces.delete.retain(|f| actual.contains(f));
    }
    // a patch whose partner is gone (a last coincidence repair that changed the topology) has
    // no side to be made on
    faces
        .kept_patches
        .retain(|(_, partner)| actual.contains(partner));
    let actual = bp::faces(other, None)?;
    if operation != BooleanOperation::PartialMerge
        && actual.len() as i128 - faces.other_keep.len() as i128 - faces.other_delete.len() as i128
            != 0
    {
        faces.other_keep.retain(|f| actual.contains(f));
        faces.other_delete.retain(|f| actual.contains(f));
    }
    // Confused/duplicate lists only emit SE_MSG upstream; they do not fail.
    Ok(())
}

fn label(attributes: &mut AttributeStore, keys: &[ObjectKey], attribute_id: u64) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let attribute = attributes.insert(Attribute::long(attribute_id, 1, Behavior::Reference))?;
    for &key in keys {
        if attributes.find_attribute(key, attribute_id).is_none() {
            attributes.add_attribute(key, attribute)?;
        }
    }
    Ok(())
}

fn label_classification(
    state: &ManifoldState,
    intersector: &impl ManifoldIntersector,
    faces: &Faces,
    attributes: &mut AttributeStore,
) -> Result<()> {
    let mut intersection: Vec<_> = state
        .intersection_edges
        .iter()
        .map(|&e| intersector.object_key(false, MergeEntity::Edge(e)))
        .collect();
    intersection.extend(
        state
            .intersection_vertices
            .iter()
            .map(|&v| intersector.object_key(false, MergeEntity::Vertex(v))),
    );
    label(attributes, &intersection, ids::BOOLEAN_INTERSECT)?;
    for (other, list, id) in [
        (false, &faces.delete, ids::BOOLEAN_DELETE),
        (false, &faces.keep, ids::BOOLEAN_SAVE),
        (true, &faces.other_delete, ids::BOOLEAN_DELETE),
        (true, &faces.other_keep, ids::BOOLEAN_SAVE),
    ] {
        let keys: Vec<_> = list
            .iter()
            .map(|&f| intersector.object_key(other, MergeEntity::Face(f)))
            .collect();
        label(attributes, &keys, id)?;
    }
    Ok(())
}

fn merge_data(attributes: &AttributeStore, key: ObjectKey) -> Option<&MergeRegionData> {
    attributes
        .find_attribute(key, ids::OTHER_REGION_ID)
        .and_then(|h| attributes.get(h))
        .and_then(Attribute::merge_region_data)
}

fn source_type(b: &Brep, r: RegionId, infinite: bool) -> Result<i64> {
    Ok(if infinite {
        2
    } else {
        i64::from(void(b, r)? != 0)
    })
}

fn remember_split(
    primary: &Brep,
    other: &Brep,
    intersector: &impl ManifoldIntersector,
    attributes: &mut AttributeStore,
    original: RegionId,
    new_face: FaceId,
    other_face: FaceId,
) -> Result<()> {
    let (r1, r2) = region_pair(
        primary,
        linked(
            face::upward_faceuse(primary, new_face)?,
            BooleanStage::Merge,
            "the face made from the other face has no upward face use, though the kernel \
             makes every face with its face uses",
        )?,
    )?;
    let (o1, o2) = region_pair(
        other,
        linked(
            face::upward_faceuse(other, other_face)?,
            BooleanStage::Merge,
            "the other face kept by the Boolean has no upward face use, though the kernel \
             keeps every face with its face uses",
        )?,
    )?;
    let key = |r| intersector.object_key(false, MergeEntity::Region(r));
    // Capture both handles before either AddAttribute, even when r1 == r2.
    let handle1 = attributes.find_attribute(key(r1), ids::OTHER_REGION_ID);
    let handle2 = attributes.find_attribute(key(r2), ids::OTHER_REGION_ID);
    let old_handle = if r1 == original { handle1 } else { handle2 };
    let old_infinite = old_handle
        .and_then(|h| attributes.get(h))
        .and_then(Attribute::merge_region_data)
        .is_some_and(MergeRegionData::has_this_infinite_region)
        || Some(original) == primary.infinite_region();
    let this_infinite = merge_data(attributes, key(r1))
        .is_some_and(MergeRegionData::has_this_infinite_region)
        || merge_data(attributes, key(r2)).is_some_and(MergeRegionData::has_this_infinite_region)
        || Some(r1) == primary.infinite_region()
        || Some(r2) == primary.infinite_region();
    let old_type = source_type(primary, original, old_infinite)?;
    for (r, o, handle) in [(r1, o1, handle1), (r2, o2, handle2)] {
        let other_key = intersector.object_key(true, MergeEntity::Region(o));
        let this_type = source_type(primary, r, this_infinite)?;
        let other_type = source_type(other, o, Some(o) == other.infinite_region())?;
        if let Some(handle) = handle {
            let data = linked(
                attributes
                    .get_mut(handle)
                    .and_then(Attribute::merge_region_data_mut),
                BooleanStage::Merge,
                "the region's merge-region attribute is not in the attribute table with its \
                 data, though the handle was just found by that attribute's id",
            )?;
            let first = linked(
                data.this_region(0),
                BooleanStage::Merge,
                "the region's merge-region attribute records no first region, though the \
                 Boolean makes that attribute with its first region",
            )?;
            // (fork e7ebc8d passes this status on: `SER`)
            data.set_regions(0, first, this_type, other_key, other_type)?;
        } else {
            let h = attributes.insert(Attribute::merge_region(
                key(original),
                old_type,
                other_key,
                other_type,
            ))?;
            attributes.add_attribute(key(r), h)?;
        }
    }
    Ok(())
}

fn make_other_faces(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Brep,
    other: &mut Brep,
    intersector: &mut impl ManifoldIntersector,
    attributes: &mut AttributeStore,
    operation: BooleanOperation,
    faces: &Faces,
) -> Result<()> {
    for &f in &faces.other_keep {
        let (fu1, fu2) = face::faceuses(other, f)?;
        let [fu1, fu2] = [fu1, fu2].map(|fu| {
            linked(
                fu,
                BooleanStage::Merge,
                "an other face kept by the Boolean has not both of its face uses, though the \
                 kernel keeps every face with a pair",
            )
        });
        let (fu1, fu2) = (fu1?, fu2?);
        let original = match kept_patch_region(primary, operation, faces, f)? {
            Some(r) => Some(r),
            None => intersector.find_region_of_other_face(kernels.primary, primary, other, fu1)?,
        };
        let original_void = if let Some(r) = original {
            void(primary, r)?
        } else {
            0
        };
        let made = if matches!(
            operation,
            BooleanOperation::Union
                | BooleanOperation::Intersection
                | BooleanOperation::Difference
                | BooleanOperation::PartialMerge
                | BooleanOperation::Merge
                | BooleanOperation::ExclusiveOr
                | BooleanOperation::Slice
        ) {
            intersector.make_faceuse(kernels, primary, other, attributes, fu1, original)?
        } else {
            MadeFaceuse::default()
        };
        if let (Some(new_region), Some(_)) = (made.new_region, original) {
            let old = linked(
                made.old_region,
                BooleanStage::Merge,
                "the face made from the other face split a region but names no old region, \
                 though making the face reports the region it splits",
            )?;
            if void(primary, new_region)? != original_void || void(primary, old)? != original_void {
                let changed = if void(primary, old)? == original_void {
                    new_region
                } else {
                    old
                };
                let [region1, region2] = [fu1, fu2].map(|fu| {
                    linked(
                        eu::faceuse_region(other, fu)?,
                        BooleanStage::Merge,
                        "a face use of an other face kept by the Boolean lies in no region, \
                         though the kernel keeps every face use's shell in a region",
                    )
                });
                let other_void1 = void(other, region1?)?;
                let other_void2 = void(other, region2?)?;
                if other_void1 != other_void2 || (other_void1 == 1 && other_void2 == 1) {
                    primary.region_mut(changed)?.set_is_void(original_void);
                }
            }
        }
        if made.new_region.is_some() {
            if let Some(&new_face) = made.faces.first() {
                remember_split(
                    primary,
                    other,
                    intersector,
                    attributes,
                    linked(
                        made.old_region,
                        BooleanStage::Merge,
                        "the face made from the other face split a region but names no old \
                         region, though making the face reports the region it splits",
                    )?,
                    new_face,
                    f,
                )?;
            }
        }
    }
    Ok(())
}

fn update_infinite(
    primary: &mut Brep,
    intersector: &impl ManifoldIntersector,
    attributes: &AttributeStore,
) -> Result<bool> {
    let infinite = linked(
        primary.infinite_region(),
        BooleanStage::Classify,
        "the primary B-rep has no infinite region, though the kernel keeps one in every \
         B-rep",
    )?;
    if merge_data(
        attributes,
        intersector.object_key(false, MergeEntity::Region(infinite)),
    )
    .is_none()
    {
        return Ok(true);
    }
    let mut count = 0;
    for r in bp::regions(primary, None)? {
        if merge_data(
            attributes,
            intersector.object_key(false, MergeEntity::Region(r)),
        )
        .is_some_and(|data| data.has_this_infinite_region() && data.has_other_infinite_region())
        {
            primary.set_infinite_region(Some(r));
            count += 1;
        }
    }
    Ok(count <= 1)
}

fn result_solid(operation: BooleanOperation, data: &MergeRegionData, was_solid: bool) -> bool {
    match operation {
        BooleanOperation::Union | BooleanOperation::Merge | BooleanOperation::PartialMerge => {
            data.has_any_solid(3)
        }
        BooleanOperation::Difference | BooleanOperation::Slice => {
            data.has_any_solid(1) && !data.has_any_solid(2)
        }
        BooleanOperation::Intersection => data.has_any_solid(1) && data.has_any_solid(2),
        BooleanOperation::ExclusiveOr => data.has_any_solid(1) != data.has_any_solid(2),
        _ => was_solid,
    }
}

/// What a regularized operation makes both sides of a face it deletes (§4.4 of
/// `docs/booleans.md`): solid for a union, void for an intersection or a difference. Each
/// deletes a face exactly where the result is the same on both sides: a union a face inside
/// the other body or a coincident pair bounding material on opposite sides, an intersection
/// a face outside it or such a pair, a difference a face inside it or a pair bounding
/// material on the same side. `None` for the operations whose deletions are not all one class.
fn deleted_face_sides(operation: BooleanOperation) -> Option<bool> {
    match operation {
        BooleanOperation::Union => Some(true),
        BooleanOperation::Intersection | BooleanOperation::Difference => Some(false),
        _ => None,
    }
}

/// The regions of `primary` on either side of a face in `delete` that joins two of them
/// with no merge data on either, the infinite region aside. A region gets merge data when a
/// face of the other body is copied into it ([`remember_split`]); one whose boundary the
/// other body's faces all lie on gets none, as the void a union fills exactly, so the
/// deletion joins it to its neighbour with nothing to give the joined region its flag.
fn joined_without_data(
    primary: &Brep,
    intersector: &impl ManifoldIntersector,
    attributes: &AttributeStore,
    delete: &[FaceId],
) -> Result<Vec<RegionId>> {
    let infinite = primary.infinite_region();
    let recorded = |r: RegionId| {
        merge_data(
            attributes,
            intersector.object_key(false, MergeEntity::Region(r)),
        )
        .is_some()
    };
    let mut joined = Vec::new();
    for &f in delete {
        let Some(fu) = face::faceuses(primary, f)?.0 else {
            continue;
        };
        let (r1, r2) = region_pair(primary, fu)?;
        if r1 == r2 || infinite == Some(r1) || infinite == Some(r2) || recorded(r1) || recorded(r2)
        {
            continue;
        }
        for r in [r1, r2] {
            if !joined.contains(&r) {
                joined.push(r);
            }
        }
    }
    Ok(joined)
}

/// Every region's enclosing shell first, the region's outer shell that
/// `SmBrep::AssertValid` test 12 takes it to be (the fork's fix, tangency-labs/smlib
/// `abastran/void-shell-check`; WR-265); upstream leaves step 6's order. Step 6 merges
/// regions and appends the merged region's shells after
/// the survivor's own, so when a union seals a void the void's shell can come first;
/// step 8 only swaps void flags. [`bp::set_region_is_void_flags_for_nested_solids`]
/// walks the nesting from the infinite region and makes each nested region's outer
/// shell its list head. It also sets every region's void flag from that nesting, which
/// differs from step 7's flags where those disagree with the nesting.
fn put_outer_shells_first(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Brep,
    operation: BooleanOperation,
) -> Result<()> {
    if !matches!(
        operation,
        BooleanOperation::Union | BooleanOperation::Difference | BooleanOperation::Intersection
    ) {
        return Ok(());
    }
    let regions = bp::regions(primary, None)?;
    if regions.len() > 2
        && regions.first() == primary.infinite_region().as_ref()
        && crate::topology::brep::p2::is_manifold_solid(primary)?
    {
        bp::set_region_is_void_flags_for_nested_solids(kernels.primary, primary)?;
    }
    Ok(())
}

fn find_and_set_infinite(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Brep,
    shell: Option<crate::topology::ShellId>,
    void_setting: bp::SetRegionVoidType,
    bounding_box: Option<&crate::geom::extent3d::Extent3d>,
) -> Result<()> {
    // (the query's geometry owners named by `primary`'s naming scheme and proven owners)
    let query = BrepQuery::of_brep(primary);
    crate::topology::brep::p5::find_and_set_infinite_region(
        kernels.primary,
        primary,
        &query,
        shell,
        void_setting,
        bounding_box,
    )
}

/// `m_pOther->IsSheet()` (`topology.brep_p2`), in the other B-rep's context as
/// [`orient_sheet_surfaces`].
fn is_sheet(kernels: &mut BooleanKernels<'_>, other: &Brep) -> Result<bool> {
    let k = kernels.other.as_deref_mut().unwrap_or(kernels.primary);
    crate::topology::brep::p2::is_sheet(k, other)
}

/// `m_pOther->IsOrientedSheet()` (`topology.brep_p2`), in the other B-rep's context.
fn is_oriented_sheet(kernels: &mut BooleanKernels<'_>, other: &Brep) -> Result<bool> {
    let k = kernels.other.as_deref_mut().unwrap_or(kernels.primary);
    crate::topology::brep::p2::is_oriented_sheet(k, other)
}

/// `m_pOther->OrientSheetSurfaces(face, same)` (`topology.brep_p9`), in the other
/// B-rep's context. Its UV swaps notify on records and attributes local to the call,
/// under the type code and arena index of each entity (`loopuse::notify_ref`, as the
/// slit-loop removal below), and without a context: the boolean holds no notification
/// state for the other B-rep, so its attributes and the context's callbacks see none
/// of these edits (upstream's do). Its note is returned as upstream's status `1`
/// ([`OrientSheetNote::stop_on_note`]), which the merge ignores or passes on as it does
/// any other status of the call.
fn orient_sheet_surfaces(
    kernels: &mut BooleanKernels<'_>,
    other: &mut Brep,
    face: FaceId,
    same: bool,
) -> Result<()> {
    let k = kernels.other.as_deref_mut().unwrap_or(kernels.primary);
    let mut records = crate::topology::topology::NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = edge::NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &crate::topology::loopuse::notify_ref,
    };
    OrientSheetNote::stop_on_note(crate::topology::brep::p9::orient_sheet_surfaces(
        k,
        other,
        Some(face),
        same,
        &mut env,
    ))
}

/// `new SmBrep(dSize, dRatio, pOptContextForStack)` for a component: the B-rep and its
/// infinite void region ([`Brep::new_with_infinite_region`], the topology part of
/// `topology.brep_p3`'s constructor). The kernel is held shared here, so the rest of
/// `SmBrep()` (the context's zone tolerance and the given sizes, and the
/// `SM_NO_CONSTRUCTION` notification) is not applied, the same deviation as
/// `blend::fillet_geom::p1::FilletBrep::new`'s and `primitive::api_primitives`'.
fn new_component_brep(
    _kernel: &Kernel,
    _model_size_estimate: f64,
    _large_small_size_ratio: f64,
    _stack_context: Option<&crate::geom::context::Context>,
) -> Result<Brep> {
    Brep::new_with_infinite_region()
}

/// `m_vTI.m_pBrep->DeleteStaleCaches()` (`topology.brep_p2`): the faces' trimmed-surface
/// caches are held by their Rust callers, not by the B-rep, and the merge holds none, so
/// none is stale.
fn delete_stale_caches(primary: &mut Brep) -> Result<()> {
    crate::topology::brep::p2::delete_stale_caches::<()>(
        primary,
        &mut std::collections::BTreeMap::new(),
    )
}

// true means imprint-only or imprint/classify: preserve normal topology and exit.
fn editing_stages(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Brep,
    other: &mut Brep,
    intersector: &mut impl ManifoldIntersector,
    state: &mut ManifoldState,
    attributes: &mut AttributeStore,
    options: MergeOptions,
    manifold: bool,
    operation: BooleanOperation,
    lock: &MarkLock,
    faces: &mut Faces,
) -> Result<bool> {
    let marks = (lock.mark_type1(), lock.mark_type2());
    for _ in 0..10 {
        for &f in faces.keep.iter().chain(&faces.delete) {
            if let Some(of) = intersector.face_mate(f) {
                intersector.remove_relationship(f, of)?;
            }
        }
        faces.keep.clear();
        faces.other_keep.clear();
        faces.delete.clear();
        faces.other_delete.clear();
        faces.tangent_patches.clear();
        mark_common(
            kernels,
            primary,
            other,
            state,
            intersector,
            lock,
            manifold,
            attributes,
        )?;
        if operation == BooleanOperation::Imprint || options.imprinting {
            return Ok(true);
        }
        if operation == BooleanOperation::PartialMerge {
            for f in bp::faces(other, None)? {
                if intersector.excluded_surface(other, f)? {
                    state
                        .other_marks
                        .mark(kernels.other(), Entity::Face(f), marks.1);
                }
            }
        }
        refresh_common(intersector, primary, other, state)?;
        let mut done = true;
        // A snapshot is required: coincidence repair can invalidate the current
        // common-edge lists and forces a new outer iteration before continuing.
        let edge_pairs: Vec<_> = state
            .edges
            .iter()
            .copied()
            .zip(state.other_edges.iter().copied())
            .collect();
        for (e, oe) in edge_pairs {
            if !classify_edge_faces(
                kernels,
                primary,
                other,
                attributes,
                state,
                intersector,
                operation,
                marks,
                e,
                oe,
                false,
                faces,
            )? || !classify_edge_faces(
                kernels,
                primary,
                other,
                attributes,
                state,
                intersector,
                operation,
                marks,
                e,
                oe,
                true,
                faces,
            )? {
                done = false;
                break;
            }
        }
        if done {
            break;
        }
    }
    resolve_tangent_patches(kernels, state, faces, marks);
    classify_remaining(kernels, primary, other, state, operation, marks, faces)?;
    if operation != BooleanOperation::ExtractSeparate {
        repair_lists(primary, other, operation, faces)?;
    }
    if operation == BooleanOperation::ImprintClassify || options.imprint_and_classify_faces {
        label_classification(state, intersector, faces, attributes)?;
        return Ok(true);
    }
    let mut keep_wires = Vec::new();
    if !options.cookie_cutter && operation != BooleanOperation::PartialMerge {
        intersector.process_wires(
            kernels,
            primary,
            other,
            state,
            attributes,
            operation,
            marks.0,
            marks.1,
            &mut keep_wires,
        )?;
    }
    if !options.cookie_cutter && operation != BooleanOperation::ExtractSeparate {
        make_other_faces(
            kernels,
            primary,
            other,
            intersector,
            attributes,
            operation,
            faces,
        )?;
    }
    let infinite_current = update_infinite(primary, intersector, attributes)?;
    if operation == BooleanOperation::Intersection {
        let (edges, _) = intersector.common_edges(primary, other)?;
        for e in edges {
            if p1::edge_borders_solid(primary, e)?.is_none() && !keep_wires.contains(&e) {
                keep_wires.push(e);
            }
        }
    }
    // Not upstream (WR-619): a union whose face deletion merges a solid into the outside is
    // refused after post-processing, if the solid side of any deleted face between a solid
    // and the outside no longer lies in a solid region. A leak whose solid's remaining face
    // uses all lie in solid regions is kept, as the region count kept it; no known input
    // records such a leak: coincident-wall joins delete no face between a solid and the
    // outside.
    let leaks = if operation == BooleanOperation::Union {
        solid_leaks(primary, &faces.delete)?
    } else {
        Vec::new()
    };
    let joined = joined_without_data(primary, intersector, attributes, &faces.delete)?;
    intersector.delete_faces(
        kernels,
        primary,
        attributes,
        &faces.delete,
        Some(&keep_wires),
    )?;
    for r in bp::regions(primary, None)? {
        if let Some(data) = merge_data(
            attributes,
            intersector.object_key(false, MergeEntity::Region(r)),
        ) {
            let solid = result_solid(operation, data, void(primary, r)? == 0);
            primary.region_mut(r)?.set_is_void(i32::from(!solid));
        } else if let (true, Some(solid)) = (joined.contains(&r), deleted_face_sides(operation)) {
            primary.region_mut(r)?.set_is_void(i32::from(!solid));
        }
    }
    if !infinite_current
        || void(
            primary,
            linked(
                primary.infinite_region(),
                BooleanStage::Classify,
                "the primary B-rep has no infinite region, though the kernel keeps one in \
                 every B-rep",
            )?,
        )? == 0
    {
        find_and_set_infinite(
            kernels,
            primary,
            None,
            bp::SetRegionVoidType::InfiniteRegionSwap,
            None,
        )?;
    }
    put_outer_shells_first(kernels, primary, operation)?;
    intersector.propagate_regions(kernels, primary, other, attributes, operation)?;
    if options.boolean_post_process
        && matches!(
            operation,
            BooleanOperation::Union
                | BooleanOperation::Intersection
                | BooleanOperation::Difference
                | BooleanOperation::ExclusiveOr
                | BooleanOperation::Slice
        )
    {
        intersector.post_process(kernels, primary, attributes, state, false)?;
    }
    for leak in &leaks {
        if !leak_stays_solid(primary, leak)? {
            return Err(
                Failure::new(OP, BooleanFailure::SolidLeaks { face: leak.face }).into_error(),
            );
        }
    }
    Ok(false)
}

/// Perform the manifold Boolean using the canonical intersector adapter. The
/// optional caller lock is bound to both operand contexts and remains locked on
/// return; an internally created lock is always released. `manifold` is the
/// existing `m_bManifoldBoolean` flag (this upstream function does not set it).
/// Returns whether `rpResult` was assigned the primary B-rep; `ExtractSeparate`
/// leaves it unassigned and writes the component arrays instead. A supplied lock
/// uses the same context-index convention as these kernels (primary 0, other 1
/// or 0 when shared); it may be initially unbound.
///
/// Ordinary early failures preserve upstream's doing-Boolean flags; temporary
/// editing changes and internally owned mark locks are restored on every return.
/// Imprint exits deliberately leave the other operand editing-enabled, as
/// `SmTemporaryChangeValue::Clear` does in the pinned source.
/// Operand slots are authoritative on **all** exits: a late deferred cache call
/// or inverse-transform error can follow deletion of `other`. `Err` does not
/// promise retained ownership or rollback of topology, records or attributes.
pub fn manifold_boolean(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Option<Brep>,
    other: &mut Option<Brep>,
    intersector: &mut impl ManifoldIntersector,
    state: &mut ManifoldState,
    attributes: &mut AttributeStore,
    options: MergeOptions,
    manifold: bool,
    operation: BooleanOperation,
    caller_lock: Option<&mut MarkLock>,
) -> Result<bool> {
    crate::profile::profile_scope!(kernels.primary, "boolean::manifold_boolean");
    manifold_boolean_transformed(
        kernels,
        primary,
        other,
        intersector,
        state,
        attributes,
        options,
        manifold,
        operation,
        caller_lock,
    )
}

/// The body of [`manifold_boolean`].
fn manifold_boolean_transformed(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Option<Brep>,
    other: &mut Option<Brep>,
    intersector: &mut impl ManifoldIntersector,
    state: &mut ManifoldState,
    attributes: &mut AttributeStore,
    options: MergeOptions,
    manifold: bool,
    operation: BooleanOperation,
    caller_lock: Option<&mut MarkLock>,
) -> Result<bool> {
    let a = operand(primary)?;
    let b = operand(other)?;
    kernels.primary.check_cancelled(OP)?;
    if let Some(other_kernel) = kernels.other.as_deref() {
        other_kernel.check_cancelled(OP)?;
    }
    let transform = p1::temporary_brep_transform(kernels.primary, kernels.other(), a, b)?.apply(
        a,
        b,
        |brep, is_other, xfm, scale, scale_tol| {
            intersector.transform_brep(kernels, brep, is_other, attributes, xfm, scale, scale_tol)
        },
    )?;
    // This outer scope matches sTempXfm's lifetime: every Result exit from the
    // complete operation (including imprint, extraction and deletion) first
    // leaves its mark/edit scopes, then restores only surviving operands.
    let mut environment = (kernels, intersector, attributes);
    transform.run(
        primary,
        other,
        &mut environment,
        |(kernels, intersector, attributes), primary, other| {
            manifold_boolean_translated(
                kernels,
                primary,
                other,
                *intersector,
                state,
                attributes,
                options,
                manifold,
                operation,
                caller_lock,
            )
        },
        |(kernels, intersector, attributes), brep, is_other, xfm, scale, scale_tol| {
            intersector.transform_brep(kernels, brep, is_other, attributes, xfm, scale, scale_tol)
        },
    )
}

fn manifold_boolean_translated(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Option<Brep>,
    other: &mut Option<Brep>,
    intersector: &mut impl ManifoldIntersector,
    state: &mut ManifoldState,
    attributes: &mut AttributeStore,
    options: MergeOptions,
    manifold: bool,
    operation: BooleanOperation,
    caller_lock: Option<&mut MarkLock>,
) -> Result<bool> {
    let a = linked(
        primary.as_mut(),
        BooleanStage::Merge,
        "the primary operand is gone after the temporary transform, though the transform \
         hands on the operands checked on entry",
    )?;
    let b = linked(
        other.as_mut(),
        BooleanStage::Merge,
        "the other operand is gone after the temporary transform, though the transform \
         hands on the operands checked on entry",
    )?;
    kernels.primary.check_cancelled(OP)?;
    if let Some(other_kernel) = kernels.other.as_deref() {
        other_kernel.check_cancelled(OP)?;
    }
    if operation == BooleanOperation::Slice {
        if !is_sheet(kernels, b)? {
            return Err(SolidError::boolean(OP, BooleanFailure::SliceNeedsSheet));
        }
        if !is_oriented_sheet(kernels, b)? {
            let first = linked(
                bp::faces(b, None)?.first().copied(),
                BooleanStage::Merge,
                "the sheet to slice with has no face, though a sheet is one only when its \
                 edges are manifold or lamina edges, each of which borders a face",
            )?;
            orient_sheet_surfaces(kernels, b, first, true)?;
        }
    }
    kernels.doing_boolean(true);
    let edit_a = a.editing_enabled();
    let edit_b = b.editing_enabled();
    a.set_editing_enabled(true);
    b.set_editing_enabled(true);
    let mut local_lock = MarkLock::unbound();
    let caller_owned = caller_lock.is_some();
    let lock = caller_lock.unwrap_or(&mut local_lock);
    let second = usize::from(kernels.other.is_some());
    // Intersections precede binding the classification mark lock upstream.
    let intersect = intersector
        .intersect_insert_relate(kernels, a, b, attributes)
        .and_then(|()| {
            state.intersection_edges.extend(intersector.mapped_edges()?);
            state
                .intersection_vertices
                .extend(intersector.mapped_vertices()?);
            Ok(())
        });
    if let Err(error) = intersect {
        a.set_editing_enabled(edit_a);
        b.set_editing_enabled(edit_b);
        return Err(error);
    }
    let bind = kernels.mark_action(|contexts| lock.set_context_pair(contexts, 0, second));
    if let Err(error) = bind {
        a.set_editing_enabled(edit_a);
        b.set_editing_enabled(edit_b);
        return Err(error);
    }
    let mut faces = Faces::default();
    let result = editing_stages(
        kernels,
        a,
        b,
        intersector,
        state,
        attributes,
        options,
        manifold,
        operation,
        lock,
        &mut faces,
    );
    if matches!(result, Ok(true)) {
        // Clear() disables restoring the other's editing flag. Deletion and
        // context resets happen while the primary is still editing-enabled,
        // before the native stack guards leave scope.
        if (operation == BooleanOperation::Imprint || options.imprinting)
            && !options.keep_other_brep
        {
            *other = None;
        }
        kernels.doing_boolean(false);
        let released = if caller_owned {
            Ok(())
        } else {
            kernels.mark_action(|contexts| lock.release(contexts))
        };
        linked(
            primary.as_mut(),
            BooleanStage::Merge,
            "the primary operand is gone after the Boolean, though the Boolean drops only the \
             other operand",
        )?
        .set_editing_enabled(edit_a);
        released?;
        return Ok(true);
    }
    // The local mark lock belongs to the editing scope. It is destroyed
    // before the earlier-declared editing guards; extraction follows both.
    let released = if caller_owned {
        Ok(())
    } else {
        kernels.mark_action(|contexts| lock.release(contexts))
    };
    a.set_editing_enabled(edit_a);
    b.set_editing_enabled(edit_b);
    released?;
    result?;
    if operation == BooleanOperation::ExtractSeparate {
        for (source, reverse, sets, components) in [
            (
                linked(
                    primary.as_mut(),
                    BooleanStage::Merge,
                    "the primary operand is gone after the extraction, though the Boolean \
                     drops only the other operand",
                )?,
                false,
                &faces.primary_sets,
                &mut state.primary_components,
            ),
            (
                linked(
                    other.as_mut(),
                    BooleanStage::Merge,
                    "the other operand is gone after the extraction, though the Boolean drops \
                     it only on the path that returned before",
                )?,
                true,
                &faces.other_sets,
                &mut state.other_components,
            ),
        ] {
            for set in sets {
                let mut component = new_component_brep(
                    if reverse {
                        kernels.other()
                    } else {
                        kernels.primary
                    },
                    crate::geom::tol::USE_DEFAULT,
                    crate::geom::tol::USE_DEFAULT,
                    None,
                )?;
                // Native defaults: new_faces=NULL, use_merge=TRUE, merge_object=NULL.
                intersector.copy_component_faces(
                    kernels,
                    source,
                    reverse,
                    set,
                    &mut component,
                    attributes,
                )?;
                components.push(component);
            }
        }
        if !options.keep_other_brep {
            *primary = None;
        }
    }
    // Not upstream (tangency-labs/solidcore#746, WR-360): a volumetric Boolean's result
    // loses the zero-area inner loops a contact curve inside a face left, and the shells
    // joined only through them are split (the fork's `RemoveSlitLoopsWhereSolidsTouch`
    // before `rpResult = m_vTI.m_pBrep`). The destroyed edge uses notify on call-local
    // records under their type code and arena index.
    if matches!(
        operation,
        BooleanOperation::Union | BooleanOperation::Intersection | BooleanOperation::Difference
    ) {
        if let Some(a) = primary.as_mut() {
            let editing = a.editing_enabled();
            a.set_editing_enabled(true);
            let mut records = crate::topology::topology::NotifyRecords::new();
            let mut env = edge::NotifyEnv {
                records: &mut records,
                attributes: &mut *attributes,
                context: None,
                key_of: &crate::topology::loopuse::notify_ref,
            };
            let removed = crate::topology::brep::p3::remove_slit_loops_where_solids_touch(
                &mut *kernels.primary,
                a,
                &mut env,
            );
            let removed = removed.and_then(|_| {
                // Not upstream (#16640): the faces of no area between edges that coincide
                // to within tolerance go too.
                crate::topology::brep::p3::collapse_zero_area_two_edge_faces(
                    &mut *kernels.primary,
                    a,
                    &mut env,
                )
            });
            a.set_editing_enabled(editing);
            removed?;
        }
    }
    finish_boolean(
        kernels,
        primary,
        other,
        options,
        operation,
        delete_stale_caches,
    )
}

// SmMerge.cpp:5059–5097 deletes other BEFORE the ignored cache status. An
// Unported cache call (or later inverse-transform failure) can therefore follow
// consumption. Keep that boundary explicit: callers must inspect the operand
// slots on every exit, not infer ownership from Result::is_ok().
fn finish_boolean(
    kernels: &mut BooleanKernels<'_>,
    primary: &mut Option<Brep>,
    other: &mut Option<Brep>,
    options: MergeOptions,
    operation: BooleanOperation,
    delete_caches: impl FnOnce(&mut Brep) -> Result<()>,
) -> Result<bool> {
    if operation != BooleanOperation::PartialMerge && !options.keep_other_brep {
        *other = None;
    }
    if let Some(a) = primary.as_mut() {
        delete_caches(a)?;
    }
    kernels.doing_boolean(false);
    Ok(operation != BooleanOperation::ExtractSeparate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_mark_contexts_survive_a_failing_action() {
        {
            let mut first = Kernel::default();
            let mut second = Kernel::default();
            first.context.set_doing_boolean(true);
            let mut kernels = BooleanKernels {
                primary: &mut first,
                other: Some(&mut second),
            };
            let mut lock = kernels
                .mark_action(|contexts| MarkLock::new_pair(contexts, 0, 1))
                .unwrap();
            let marks = (lock.mark_type1(), lock.mark_type2());
            let before = (
                kernels.primary.context.current_mark(marks.0),
                kernels.other().context.current_mark(marks.1),
            );
            let result: Result<()> = kernels.mark_action(|contexts| {
                lock.new_mark(contexts)?;
                Err(SolidError::sm_err("test"))
            });
            assert!(result.is_err());
            assert!(kernels.primary.context.doing_boolean());
            assert!(!kernels.other().context.doing_boolean());
            assert_ne!(kernels.primary.context.current_mark(marks.0), before.0);
            assert_ne!(kernels.other().context.current_mark(marks.1), before.1);
            kernels
                .mark_action(|contexts| lock.release(contexts))
                .unwrap();
            assert!(!kernels.primary.context.is_mark_locked(marks.0));
            assert!(!kernels.other().context.is_mark_locked(marks.1));
        }
    }

    #[test]
    fn completion_reports_actual_consumption_before_late_cache_failure() {
        for operation in [BooleanOperation::Difference, BooleanOperation::PartialMerge] {
            for keep in [false, true] {
                for late in [0, 1, 2] {
                    let mut kernel = Kernel::default();
                    kernel.context.set_doing_boolean(true);
                    let mut kernels = BooleanKernels {
                        primary: &mut kernel,
                        other: None,
                    };
                    let mut primary = Some(Brep::new());
                    let mut other = Some(Brep::new());
                    let options = MergeOptions {
                        keep_other_brep: keep,
                        ..MergeOptions::default()
                    };
                    let mut called = false;
                    let result = finish_boolean(
                        &mut kernels,
                        &mut primary,
                        &mut other,
                        options,
                        operation,
                        |_| {
                            called = true;
                            match late {
                                0 => Ok(()),
                                1 => Err(SolidError::sm_err("test::cache")),
                                _ => Err(SolidError::unported(
                                    "topology::brep::p2::delete_stale_caches",
                                    "topology.brep_p2",
                                )),
                            }
                        },
                    );
                    assert!(called);
                    assert!(primary.is_some());
                    assert_eq!(
                        other.is_none(),
                        !keep && operation != BooleanOperation::PartialMerge
                    );
                    // DeleteStaleCaches' ordinary failure is passed on too (WR-658,
                    // tangency-labs/smlib e7ebc8d), before the boolean flag is cleared
                    assert_eq!(result.is_err(), late != 0);
                    assert_eq!(kernels.primary.context.doing_boolean(), late != 0);
                }
            }
        }
    }

    /// #17118: the bound on how far a cylinder or cone face strays from a plane covers the
    /// whole face, not only its boundary points. `on` gives points of the cone over a box of
    /// angles (radians from the x axis) and heights; `exact` is the largest distance from the
    /// plane over that box.
    #[test]
    fn a_cone_face_bound_covers_the_face_between_its_boundary_points() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let on = |r: f64, tan: f64, ts: &[f64], hs: &[f64]| -> Vec<Vec3> {
            let mut out = Vec::new();
            for &t in ts {
                for &h in hs {
                    let rh = r + tan * h;
                    out.push(Vec3::new(rh * t.cos(), rh * t.sin(), h));
                }
            }
            out
        };
        let x = Vec3::new(1.0, 0.0, 0.0);
        // a sliver of a cylinder of radius 5 beside the plane x = 5, tangent along y = 0
        let sliver = on(5.0, 0.0, &[0.0, 1e-3], &[0.0, 10.0]);
        let cylinder = (Vec3::new(0.0, 0.0, 0.0), z, 5.0, 0.0);
        let tangent = (Vec3::new(5.0, 0.0, 0.0), x);
        let exact = 5.0 * (1.0 - 1e-3_f64.cos());
        let bound = cone_box_gap(cylinder, tangent, &sliver, 0.0).unwrap();
        assert!(bound >= exact && bound < exact + 1e-12, "{bound} {exact}");
        // the plane x = 4.99 cuts into the cylinder: the face's corners lie on it, but the
        // face's middle, at y = 0, lies 0.01 beyond it
        let t = (4.99_f64 / 5.0).acos();
        let chord = on(5.0, 0.0, &[-t, t], &[0.0, 10.0]);
        let cutting = (Vec3::new(4.99, 0.0, 0.0), x);
        assert!(chord
            .iter()
            .all(|&p| (p - cutting.0).dot(cutting.1).abs() < 1e-12));
        let bound = cone_box_gap(cylinder, cutting, &chord, 0.0).unwrap();
        assert!(bound >= 0.01 - 1e-12, "{bound}");
        // the edges' tolerance widens the box
        let widened = cone_box_gap(cylinder, tangent, &sliver, 1e-5).unwrap();
        assert!(widened > exact, "{widened}");
        // a cone of radius 5 + h / 2 beside its tangent plane along y = 0
        let cone = (Vec3::new(0.0, 0.0, 0.0), z, 5.0, 0.5);
        let normal = Vec3::new(1.0, 0.0, -0.5) * (1.0 / 1.25_f64.sqrt());
        let sliver = on(5.0, 0.5, &[0.0, 1e-3], &[0.0, 4.0]);
        let exact = 7.0 * (1.0 - 1e-3_f64.cos()) / 1.25_f64.sqrt();
        let bound = cone_box_gap(cone, (Vec3::new(5.0, 0.0, 0.0), normal), &sliver, 0.0).unwrap();
        assert!(bound >= exact && bound < exact + 1e-12, "{bound} {exact}");
        // a boundary that reaches a quarter turn from its mean direction is not bounded
        let wide = on(5.0, 0.0, &[-1.6, 0.0, 1.6], &[0.0, 10.0]);
        assert_eq!(cone_box_gap(cylinder, tangent, &wide, 0.0), None);
    }

    /// #17118: a face counts as a copy of part of its partner only when its boundary lies within
    /// the partner face's edges, not only on its plane. The partner here is the top face of a
    /// 10 mm cube; a strip whose points run 2 mm past its edge, still on its plane, is refused.
    #[test]
    fn a_tangent_patch_must_lie_within_its_partner_faces_edges() {
        use crate::math::Point3;
        use crate::primitive::api_primitives::create_box;
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
        let top = b
            .faces()
            .ids()
            .find(|&f| {
                crate::topology::face::p4::vertices(&b, f, None, false, None)
                    .unwrap()
                    .iter()
                    .all(|&v| (vertex::point(&b, v).unwrap().z - 10.0).abs() < 1e-9)
            })
            .unwrap();
        let tol = face::tolerance(&b, top).unwrap();
        let at = |x: f64, y: f64| Vec3::new(x, y, 10.0);
        // inside, on an edge and at a corner
        let within = [at(5.0, 5.0), at(10.0, 5.0), at(10.0, 10.0), at(1.0, 9.0)];
        assert!(points_in_face(&k, &b, top, &within, tol).unwrap());
        // the same strip running on past the edge at x = 10
        let past = [at(5.0, 5.0), at(10.0, 5.0), at(12.0, 5.0)];
        assert!(!points_in_face(&k, &b, top, &past, tol).unwrap());
    }

    /// #17118: a tangent patch waiting for its partner is revisited once a later patch
    /// classifies that partner. `P1` waits for `Q`; the next patch deletes `Q`, because its
    /// partner `P2` is kept; on the second pass `P1` is kept, as the copy of a deleted face.
    /// A single pass in list order would leave `P1` to the ray.
    #[test]
    fn a_tangent_patch_waiting_for_its_partner_is_resolved_on_a_later_pass() {
        let mut first = Kernel::default();
        let mut second = Kernel::default();
        first.context.set_doing_boolean(true);
        let mut kernels = BooleanKernels {
            primary: &mut first,
            other: Some(&mut second),
        };
        let mut lock = kernels
            .mark_action(|contexts| MarkLock::new_pair(contexts, 0, 1))
            .unwrap();
        kernels
            .mark_action(|contexts| lock.new_mark(contexts))
            .unwrap();
        let marks = (lock.mark_type1(), lock.mark_type2());
        let (p1, p2, q) = (
            FaceId::from_index(1),
            FaceId::from_index(2),
            FaceId::from_index(3),
        );
        let mut state = ManifoldState::default();
        state
            .primary_marks
            .mark(kernels.primary, Entity::Face(p2), marks.0);
        let mut faces = Faces {
            keep: vec![p2],
            tangent_patches: vec![
                TangentPatch {
                    reverse: false,
                    pf: p1,
                    of: q,
                    keep: Some(true),
                },
                TangentPatch {
                    reverse: true,
                    pf: p2,
                    of: q,
                    keep: Some(true),
                },
            ],
            ..Faces::default()
        };
        resolve_tangent_patches(&kernels, &mut state, &mut faces, marks);
        assert!(state
            .primary_marks
            .is_marked(kernels.primary, Entity::Face(p1), marks.0));
        assert!(state
            .other_marks
            .is_marked(kernels.other(), Entity::Face(q), marks.1));
        assert_eq!(faces.keep, [p2, p1]);
        assert_eq!(faces.other_delete, [q]);
        assert!(faces.delete.is_empty() && faces.other_keep.is_empty());
        assert!(faces.tangent_patches.is_empty());
        kernels
            .mark_action(|contexts| lock.release(contexts))
            .unwrap();
    }

    /// #21931: where the operation keeps no copy of coincident faces, a patch whose partner is
    /// kept stays (it bounds the film between them), one whose partner is deleted goes, and a
    /// patch of the other body that stays is recorded with its partner.
    #[test]
    fn a_tangent_patch_with_no_copy_kept_stays_beside_a_kept_partner_only() {
        let mut first = Kernel::default();
        let mut second = Kernel::default();
        first.context.set_doing_boolean(true);
        let mut kernels = BooleanKernels {
            primary: &mut first,
            other: Some(&mut second),
        };
        let mut lock = kernels
            .mark_action(|contexts| MarkLock::new_pair(contexts, 0, 1))
            .unwrap();
        kernels
            .mark_action(|contexts| lock.new_mark(contexts))
            .unwrap();
        let marks = (lock.mark_type1(), lock.mark_type2());
        let (kept, deleted, q1, q2) = (
            FaceId::from_index(1),
            FaceId::from_index(2),
            FaceId::from_index(3),
            FaceId::from_index(4),
        );
        let mut state = ManifoldState::default();
        let patch = |pf, of| TangentPatch {
            reverse: true,
            pf,
            of,
            keep: Some(false),
        };
        let mut faces = Faces {
            keep: vec![kept],
            delete: vec![deleted],
            tangent_patches: vec![patch(kept, q1), patch(deleted, q2)],
            ..Faces::default()
        };
        resolve_tangent_patches(&kernels, &mut state, &mut faces, marks);
        assert_eq!(faces.other_keep, [q1]);
        assert_eq!(faces.other_delete, [q2]);
        assert_eq!(faces.kept_patches, [(q1, kept)]);
        assert!(faces.tangent_patches.is_empty());
        kernels
            .mark_action(|contexts| lock.release(contexts))
            .unwrap();
    }

    #[test]
    fn slice_keeps_a_coincident_pair_on_the_sheet_same_side_only() {
        // issue #528: against a sheet (void on both sides) a coincident pair is either
        // non-solid or on opposite sides, which the difference rule always keeps
        for (same_side, non_solid) in [(false, false), (false, true), (true, false)] {
            let difference = Some(!same_side || non_solid);
            assert_eq!(
                coincident_keep(BooleanOperation::Difference, same_side, non_solid, None),
                difference
            );
            // a primary face not bounding material keeps the difference rule
            assert_eq!(
                coincident_keep(BooleanOperation::Slice, same_side, non_solid, None),
                difference
            );
            for slice_same in [Some(false), Some(true)] {
                assert_eq!(
                    coincident_keep(BooleanOperation::Slice, same_side, non_solid, slice_same),
                    slice_same
                );
                for operation in [
                    BooleanOperation::Union,
                    BooleanOperation::Intersection,
                    BooleanOperation::Difference,
                    BooleanOperation::ExclusiveOr,
                    BooleanOperation::Merge,
                    BooleanOperation::PartialMerge,
                    BooleanOperation::ExtractSeparate,
                ] {
                    assert_eq!(
                        coincident_keep(operation, same_side, non_solid, slice_same),
                        coincident_keep(operation, same_side, non_solid, None)
                    );
                }
            }
        }
    }

    #[test]
    fn classification_attributes_share_reference_identity_and_preserve_existing_tags() {
        let mut attributes = AttributeStore::new();
        let a = ObjectKey(11);
        let b = ObjectKey(12);
        let original = attributes
            .insert(Attribute::long(ids::BOOLEAN_SAVE, 7, Behavior::Reference))
            .unwrap();
        attributes.add_attribute(a, original).unwrap();
        label(&mut attributes, &[a, b], ids::BOOLEAN_SAVE).unwrap();
        assert_eq!(
            attributes.find_attribute(a, ids::BOOLEAN_SAVE),
            Some(original)
        );
        let added = attributes.find_attribute(b, ids::BOOLEAN_SAVE).unwrap();
        label(
            &mut attributes,
            &[ObjectKey(13), ObjectKey(14)],
            ids::BOOLEAN_DELETE,
        )
        .unwrap();
        assert_ne!(added, original);
        assert_eq!(
            attributes.find_attribute(ObjectKey(13), ids::BOOLEAN_DELETE),
            attributes.find_attribute(ObjectKey(14), ids::BOOLEAN_DELETE)
        );
    }

    /// The cup and lid of WR-265 (#388): a cube 100 hollowed to an open cup (walls and
    /// floor 10) joined with a lid over its top 10 closes the cavity 80 x 80 x 80. For
    /// either operand order, whether each shell of the one solid region is mated to the
    /// infinite region (the outer shell).
    fn cup_and_lid_outer_shells() -> Vec<Vec<bool>> {
        use crate::boolean::api_brep::boolean;
        use crate::math::Point3;
        use crate::primitive::api_primitives::create_box;
        let mut out = Vec::new();
        for lid_first in [false, true] {
            let mut k = Kernel::default();
            let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0, 100.0).unwrap();
            let hollow =
                create_box(&mut k, Point3::new(10.0, 10.0, 10.0), 80.0, 80.0, 100.0).unwrap();
            let lid = create_box(&mut k, Point3::new(0.0, 0.0, 90.0), 100.0, 100.0, 10.0).unwrap();
            let cup = boolean(&mut k, cube, hollow, BooleanOperation::Difference)
                .unwrap()
                .result;
            let (a, b) = if lid_first { (lid, cup) } else { (cup, lid) };
            let r = boolean(&mut k, a, b, BooleanOperation::Union)
                .unwrap()
                .result;
            let infinite = r.infinite_region().unwrap();
            let solids: Vec<RegionId> = bp::regions(&r, None)
                .unwrap()
                .into_iter()
                .filter(|&g| r.region(g).unwrap().is_void() == 0)
                .collect();
            assert_eq!(solids.len(), 1, "lid first: {lid_first}");
            let shells = r.region(solids[0]).unwrap().shells.clone();
            out.push(
                shells
                    .iter()
                    .map(|&shell| {
                        let mated = r.shell(shell).unwrap().mated_shells(&mut k, &r).unwrap();
                        mated
                            .iter()
                            .all(|&m| r.shell(m).unwrap().region == Some(infinite))
                    })
                    .collect(),
            );
        }
        out
    }

    /// WR-619's per-leak check on a cube [0, 1]^3: its region `solid` and every face's use
    /// on each side. A leak names `solid` and the face uses that bounded it.
    fn cube_leak_parts() -> (Kernel, Brep, RegionId, Vec<FaceuseId>, Vec<FaceuseId>) {
        use crate::math::Point3;
        use crate::primitive::api_primitives::create_box;
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let infinite = b.infinite_region().unwrap();
        let solid = bp::regions(&b, None)
            .unwrap()
            .into_iter()
            .find(|&r| r != infinite)
            .unwrap();
        let (mut inside, mut outside) = (Vec::new(), Vec::new());
        for f in bp::faces(&b, None).unwrap() {
            let (fu1, fu2) = face::faceuses(&b, f).unwrap();
            for fu in [fu1.unwrap(), fu2.unwrap()] {
                match eu::faceuse_region(&b, fu).unwrap() {
                    Some(r) if r == solid => inside.push(fu),
                    Some(r) if r == infinite => outside.push(fu),
                    other => panic!("face use {fu:?} in region {other:?}"),
                }
            }
        }
        assert_eq!((inside.len(), outside.len()), (6, 6));
        (k, b, solid, inside, outside)
    }

    /// A leak whose remaining face uses lie in the solid region stays solid; one with a use
    /// in the infinite region, or in a region flagged void, does not.
    #[test]
    fn wr_619_leak_with_a_use_left_follows_that_use() {
        let (_k, mut b, solid, inside, outside) = cube_leak_parts();
        let face = bp::faces(&b, None).unwrap()[0];
        let leak = |bounding: Vec<FaceuseId>| SolidLeak {
            face,
            region: solid,
            bounding,
        };
        assert!(leak_stays_solid(&b, &leak(inside.clone())).unwrap());
        let mut mixed = inside.clone();
        mixed.push(outside[0]);
        assert!(!leak_stays_solid(&b, &leak(mixed)).unwrap());
        assert!(!leak_stays_solid(&b, &leak(outside)).unwrap());
        b.region_mut(solid).unwrap().set_is_void(1);
        assert!(!leak_stays_solid(&b, &leak(inside)).unwrap());
    }

    /// A leak with none of its face uses left falls back to the whole-body test: it stays
    /// solid exactly when the body still has a solid region (the check before per-leak
    /// tracking), so this branch refuses nothing that test did not refuse.
    #[test]
    fn wr_619_leak_with_no_use_left_falls_back_to_a_solid_region() {
        let (_k, mut b, solid, _inside, _outside) = cube_leak_parts();
        let face = bp::faces(&b, None).unwrap()[0];
        let unlocated = SolidLeak {
            face,
            region: solid,
            bounding: Vec::new(),
        };
        assert!(has_solid_region(&b).unwrap());
        assert!(leak_stays_solid(&b, &unlocated).unwrap());
        b.region_mut(solid).unwrap().set_is_void(1);
        assert!(!has_solid_region(&b).unwrap());
        assert!(!leak_stays_solid(&b, &unlocated).unwrap());
    }

    /// WR-265: the outer shell comes first, as `SmBrep::AssertValid` test 12 wants
    /// (SMLib's step 6 left the cavity's shell first).
    #[test]
    fn issue_388_union_sealing_a_void_puts_the_outer_shell_first() {
        assert_eq!(
            cup_and_lid_outer_shells(),
            [vec![true, false], vec![true, false]]
        );
    }
}
