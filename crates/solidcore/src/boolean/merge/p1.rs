//! Merge control, face coincidence, vertex copying and Boolean cleanup helpers.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/inc/SmMerge.h`.
//!
//! | Rust                         | Upstream                                         |
//! |------------------------------|--------------------------------------------------|
//! | [`MergeOptions`]             | `SmMergeOptions` (fields, `Get*`, `SetGet*`)     |
//! | [`MergeOptions::new`]        | `SmMergeOptions::SmMergeOptions(...)`            |
//! | [`MergeOptions::default`]    | `SmMergeOptions()` (all default arguments)       |
//! | `Clone`                      | `SmMergeOptions(const SmMergeOptions&)`          |
//! | [`MergeState`] | `SmMerge(context)`, `SetMergeOptions`, setters, `GetTopologyIntersector` |
//! | [`fix_found_face_coincidence`] | `SmMerge::FixFoundFaceCoincidence` |
//! | [`find_brep_region_of_other_face`] | `sm_FindBrepRegionOfOtherFace` |
//! | [`edge_borders_solid`] | `sm_EdgeBordersSolid` |
//! | [`merge_vertex`] | `SmMerge::MergeVertex` |
//! | [`boolean_post_process`] | `SmMerge::BooleanPostProcess` |
//! | [`propagate_other_src_region_attribs`] | `SmMerge::PropagateOtherSrcRegionAttribs` |
//! | [`temporary_brep_transform`], [`TemporaryBrepTransform`] | `SmTemporaryBrepTransform` (constructor/destructor) |
//!
//! Deviations: option accessors are public fields; the context-only constructor's
//! already-created intersector is supplied by its owner (its type belongs to
//! `boolean.topology_intersector_p1`). Mate maps and notification records are
//! borrowed explicitly; edge/vertex mate storage is co-ported in
//! `boolean::topology_intersector::p1` and shared mutably with `merge_p5`. B-rep
//! edge/shell traversal uses `topology::brep::p1` (`GetEdges`, `GetShells`; the
//! latter still stops at `SmRegion::GetShells`, unit `topology.shell`).
//! Region propagation takes refs and
//! their `IsVoid` results and source-list callback handles, because arena regions
//! do not yet carry shell state.
//! Missing topology mutations and point classification return
//! Unported at the first call reached. The temporary transform waits on B-rep
//! bounding boxes before any mutation; no translation is faked. Destruction is
//! Rust ownership; the temporary-transform token is restored explicitly after the
//! caller's complete scope, including error exits. There is no input-dependent recursion.

use std::collections::BTreeMap;

use crate::boolean::topology_intersector::p1::Relations;
use crate::error::{Result, SolidError};
use crate::geom::a_object::{self, BooleanOperation};
use crate::geom::attribute::{ids, AttributeStore, ObjectKey};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::Context;
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::shell;
use crate::topology::topology::{self as topo, Topology};
use crate::topology::topology_traverser::face_edges;
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, FaceuseId, RegionId, ShellId, VertexId};

const OP: &str = "boolean::merge::p1";

/// Optional boolean flags of a merge (`SmMergeOptions`). The defaults are upstream's
/// (and the `SmMerge` constructor's): only `boolean_post_process` is set.
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean members
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeOptions {
    /// `m_bCookieCutter`: skip adding faces of the second B-rep to the first (only
    /// remove faces).
    pub cookie_cutter: bool,
    /// `m_bImprinting`: imprint (A intersect B) on the first B-rep and quit (as
    /// `SM_BO_IMPRINT`).
    pub imprinting: bool,
    /// `m_bBooleanPostProcess`: remove topological edges and vertices after the
    /// boolean.
    pub boolean_post_process: bool,
    /// `m_bImprintAndClassifyFaces`: imprint (A intersect B) on the first B-rep, mark
    /// the faces that would be deleted with `SM_AI_BOOLEAN_DELETED` and quit.
    pub imprint_and_classify_faces: bool,
    /// `m_bKeepOtherBrep`: do not delete the second B-rep (nor the first with
    /// `SM_BO_EXTRACT_SEPARATE`).
    pub keep_other_brep: bool,
}

impl MergeOptions {
    /// The options with every flag given (`SmMergeOptions::SmMergeOptions`).
    #[must_use]
    #[allow(clippy::fn_params_excessive_bools)] // upstream's constructor arguments
    pub const fn new(
        cookie_cutter: bool,
        imprinting: bool,
        boolean_post_process: bool,
        imprint_and_classify_faces: bool,
        keep_other_brep: bool,
    ) -> Self {
        Self {
            cookie_cutter,
            imprinting,
            boolean_post_process,
            imprint_and_classify_faces,
            keep_other_brep,
        }
    }
}

impl Default for MergeOptions {
    /// `SmMergeOptions()`: `(FALSE, FALSE, TRUE, FALSE, FALSE)`.
    fn default() -> Self {
        Self::new(false, false, true, false, false)
    }
}

/// The merge's own control data, separate from the intersector's implementation.
/// `T` is owned by `boolean.topology_intersector_p1`; no second intersector type
/// or set of operand pointers is introduced here.
#[derive(Debug, Clone)]
pub struct MergeState<T> {
    /// The five Boolean control flags (the inline `Set*` members).
    pub options: MergeOptions,
    /// `m_lTagID`, initially zero.
    pub tag_id: i64,
    /// `m_bManifoldBoolean` (try fixing non-manifold cases): `TRUE` from the B-rep
    /// constructor; `None` from the context-only one, which leaves it uninitialised
    /// (a reader must treat `None` as upstream's indeterminate value).
    pub manifold_boolean: Option<bool>,
    /// `m_bIntersectionLoopClosed` (output status: `FALSE` when the result has an
    /// open intersection loop): as [`MergeState::manifold_boolean`].
    pub intersection_loop_closed: Option<bool>,
    /// `m_vTI`; its construction is the intersector owner's responsibility.
    pub intersector: T,
}

impl<T> MergeState<T> {
    /// Initialise the context-only `SmMerge` constructor's own members.
    /// Upstream constructs the intersector with null operands, `SM_EFF_ZERO_SQ`
    /// distance tolerance and a 30 degree angle tolerance before this step.
    #[must_use]
    pub fn new(intersector: T) -> Self {
        Self {
            options: MergeOptions::default(),
            tag_id: 0,
            // not in the context-only constructor's initialiser list
            manifold_boolean: None,
            intersection_loop_closed: None,
            intersector,
        }
    }

    /// `SetMergeOptions`: a null options pointer leaves all flags unchanged.
    pub fn set_merge_options(&mut self, options: Option<&MergeOptions>) {
        if let Some(options) = options {
            self.options = *options;
        }
    }
}

fn unavailable<T>(symbol: &'static str, unit: &'static str) -> Result<T> {
    Err(SolidError::unported(symbol, unit))
}

fn ignore_failure<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported() => Err(e),
        Err(_) => Ok(None),
    }
}

/// Return matched coincident face lists and whether topology was modified.
/// The fast path tests existence of mates, as upstream; it deliberately does not
/// require the mates to belong to the supplied opposite face's boundary.
/// A mismatch reaches the unported coincident-surface merger.
pub fn fix_found_face_coincidence(
    brep: &mut Brep,
    other: &mut Brep,
    face: FaceId,
    other_face: FaceId,
    other_mates: &BTreeMap<EdgeId, EdgeId>,
    brep_mates: &BTreeMap<EdgeId, EdgeId>,
) -> Result<(Vec<FaceId>, Vec<FaceId>, bool)> {
    let edges = face_edges(brep, face)?;
    let other_edges = face_edges(other, other_face)?;
    if edges.len() == other_edges.len()
        && edges
            .iter()
            .zip(&other_edges)
            .all(|(a, b)| other_mates.contains_key(a) && brep_mates.contains_key(b))
    {
        return Ok((vec![face], vec![other_face], false));
    }
    let counts = (brep_edges(brep)?.len(), brep_edges(other)?.len());
    let (faces, other_faces) = merge_coincident_surfaces(brep, other, face, other_face)?;
    let changed = brep_edges(brep)?.len() != counts.0 || brep_edges(other)?.len() != counts.1;
    Ok((faces, other_faces, changed))
}

fn merge_coincident_surfaces(
    _brep: &mut Brep,
    _other: &mut Brep,
    _face: FaceId,
    _other_face: FaceId,
) -> Result<(Vec<FaceId>, Vec<FaceId>)> {
    // UNPORTED(boolean.topology_intersector_p3): SmTopologyIntersector::MergeCoincidentSurfaces
    unavailable(
        "SmTopologyIntersector::MergeCoincidentSurfaces",
        "boolean.topology_intersector_p3",
    )
}

/// Classify an other-operand face use into a primary region. A single primary
/// region wins immediately. Otherwise try mated-edge radial sectors, then sample
/// unmated edges at 0.456789 of their interval and classify each point.
pub fn find_brep_region_of_other_face(
    k: &Kernel,
    brep: &mut Brep,
    other: &mut Brep,
    other_faceuse: FaceuseId,
    brep_mates: &BTreeMap<EdgeId, EdgeId>,
) -> Result<Option<RegionId>> {
    if brep.regions().len() == 1 {
        return Ok(brep.regions().ids().next());
    }
    let edges = eu::faceuse_edgeuses(other, other_faceuse, false, None)?;
    for &other_eu in &edges {
        let e = eu::edge(other, other_eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let Some(&mate) = brep_mates.get(&e) else {
            continue;
        };
        let Some(sector) = ignore_failure(edge::find_radial_sector_between(
            k,
            brep,
            mate,
            Some(&mut *other),
            other_eu,
            None,
            OrientType::Same,
            None,
        ))?
        else {
            continue;
        };
        let region = if let Some(eu_id) = sector.radial_edgeuse {
            let shell = eu::shell(brep, eu_id)?.ok_or_else(|| SolidError::null_pointer(OP))?;
            eu::shell_region(brep, shell)?
        } else if let Some(fu) = sector.coincident_faceuse {
            eu::faceuse_region(brep, fu)?
        } else {
            return Ok(None);
        };
        if region.is_some() {
            return Ok(region);
        }
        break;
    }
    for other_eu in edges {
        let e = eu::edge(other, other_eu)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        if brep_mates.contains_key(&e) {
            continue;
        }
        let edge = other.edge(e)?;
        let curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let Some(_point) = ignore_failure(curve.evaluate_point(edge.interval.evaluate(0.456789)))?
        else {
            continue;
        };
        // Point classification uses tolerance 0.0, boundary intersections FALSE,
        // a classification initialised with edge.tolerance and editing disabled.
        // UNPORTED(query.brep): SmBrep::Point3DClassify
        return unavailable("SmBrep::Point3DClassify", "query.brep");
    }
    Ok(None)
}

/// `pRegion->IsVoid()` as a C++ condition (`UNSURE` is non-zero, so true).
fn region_is_void(b: &Brep, r: RegionId) -> Result<bool> {
    Ok(b.region(r)?.is_void() != 0)
}

/// The first shell reached by this edge that belongs to a solid region.
pub fn edge_borders_solid(b: &Brep, e: EdgeId) -> Result<Option<ShellId>> {
    for edgeuse in edge::edgeuses(b, e, None, None)? {
        let Some(shell) = eu::shell(b, edgeuse)? else {
            continue;
        };
        let region = eu::shell_region(b, shell)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        if !region_is_void(b, region)? {
            return Ok(Some(shell));
        }
    }
    Ok(None)
}

/// Explicit notification storage and object identities for a copied vertex.
/// Keys must be distinct across operands even when their arena ids coincide.
pub struct VertexCopy<'a> {
    /// Topology base-class records of both operands.
    pub records: &'a mut BTreeMap<ObjectKey, Topology>,
    /// Attributes of both operands.
    pub attributes: &'a mut AttributeStore,
    /// Notification identity of each other-operand vertex.
    pub source_key: &'a dyn Fn(VertexId) -> ObjectRef,
    /// Notification identity of each primary topology entity, including newly created vertices and vertex uses.
    pub destination_key: &'a dyn Fn(crate::EntityRef) -> ObjectRef,
}

/// Reuse a mated primary vertex or create a shell vertex, relate it and notify
/// the source of its copy. Null source succeeds with no output, as upstream.
/// `region = None` forwards the infinite-region default of `MakeShellVertex`.
/// `SM_NO_GET_BREP` expands to null in the pinned source, so both B-rep
/// notification arguments remain `None`. The reference enables `SM_USE_OLDTOL`:
/// a larger source tolerance is copied before notification. `other` is mutable
/// because `Relate` of a new vertex may raise the source vertex's tolerance.
pub fn merge_vertex(
    brep: &mut Brep,
    other: &mut Brep,
    source: Option<VertexId>,
    region: Option<RegionId>,
    relations: &mut Relations,
    copy: &mut VertexCopy<'_>,
    mut context: Option<&mut Context>,
) -> Result<Option<VertexId>> {
    let Some(source) = source else {
        return Ok(None);
    };
    other.vertex(source)?;
    let dest = if let Some(dest) = relations.vertices.primary(source) {
        dest
    } else {
        let point = other.vertex(source)?.point;
        let mut env = edge::NotifyEnv {
            records: copy.records,
            attributes: copy.attributes,
            context: context.as_deref_mut(),
            key_of: copy.destination_key,
        };
        let (_, dest) =
            crate::topology::brep::p9::make_shell_vertex(brep, region, point, &mut env)?;
        relations.relate_vertices(brep, dest, other, source)?;
        dest
    };
    brep.vertex(dest)?;
    let tolerance = vertex::tolerance(other, source)?;
    if tolerance > vertex::tolerance(brep, dest)? {
        vertex::set_tolerance(brep, dest, tolerance, true, true)?;
    }
    topo::notify(
        copy.records,
        copy.attributes,
        context,
        NotifyOperation::Copy,
        (copy.source_key)(source),
        Some((copy.destination_key)(crate::EntityRef::Vertex(dest))),
        None, // SM_NO_GET_BREP(pBrepVertex) is NULL in pinned SmTypes.h.
        None, // SM_NO_GET_BREP(pVertexInOther) is NULL as well.
    )?;
    Ok(Some(dest))
}

/// The intersection lists and live primary-to-other mate lookups used by cleanup.
#[derive(Debug, Default)]
pub struct CleanupRelations {
    /// `m_sIntersectionEdges`, in original order; may contain stale ids.
    pub edges: Vec<EdgeId>,
    /// `m_sIntersectionVertices`, in original order; may contain stale ids.
    pub vertices: Vec<VertexId>,
    /// Shared live mates; deleted entities must be removed by the intersector.
    pub mates: Relations,
}

fn brep_edges(brep: &Brep) -> Result<Vec<EdgeId>> {
    crate::topology::brep::p1::edges(brep, None)
}

/// `SmBrep::GetVertices` (ported in `topology::brep::p1`: the vertex list in list order).
fn brep_vertices(brep: &Brep) -> Result<Vec<VertexId>> {
    crate::topology::brep::p1::vertices(brep, None)
}

/// `SmBrep::GetShells` (ported in `topology::brep::p1`; `&mut Kernel` for its mark lock).
fn brep_shells(k: &mut Kernel, brep: &Brep) -> Result<Vec<ShellId>> {
    crate::topology::brep::p1::shells(k, brep, None)
}

/// `pFace->SetTolerance(tol)` with the upstream defaults (update only if larger, cascade
/// to the boundaries).
fn set_face_tolerance(brep: &mut Brep, face: FaceId, tolerance: f64) -> Result<()> {
    crate::topology::face::p3::set_tolerance(brep, face, tolerance, true, true)
}

/// Remove topological intersection edges and redundant vertices. `check_all_edges`
/// defaults to true upstream; false checks only the intersection lists with live
/// mates. In both modes the vertex list is the intersection list plus the deleted
/// edges' endpoints, rather than every vertex in the B-rep.
pub fn boolean_post_process(
    k: &mut Kernel,
    brep: &mut Brep,
    relations: &CleanupRelations,
    check_all_edges: bool,
) -> Result<()> {
    let edges: Vec<_> = if check_all_edges {
        brep_edges(brep)?
    } else {
        relations.edges.clone()
    };
    let mut endpoints = Vec::new();
    for e in edges {
        if !check_all_edges && relations.mates.edges.other(e).is_none() {
            continue;
        }
        let topological = edge::is_topological(k, brep, e, 10.0)?;
        if !topological.is_topological {
            continue;
        }
        let keep = topological
            .surf_to_keep
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let edgeuses = edge::edgeuses(brep, e, None, None)?;
        let first = edgeuses
            .first()
            .copied()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let third = edgeuses
            .get(2)
            .copied()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let f0 = eu::face(brep, first)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        let f1 = eu::face(brep, third)?.ok_or_else(|| SolidError::null_pointer(OP))?;
        // Surface identity is represented by its owning face in this arena model.
        if f0 != f1 {
            let domain = brep
                .face(keep)?
                .surface
                .as_ref()
                .ok_or_else(|| SolidError::null_pointer(OP))?
                .natural_uv_domain();
            crate::topology::face::p2::set_uv_domain(brep, keep, &domain)?;
            let tolerance = crate::topology::face::p2::tolerance(brep, keep)?;
            if topological.dist_between > tolerance / 10.0 {
                set_face_tolerance(brep, keep, tolerance + topological.dist_between)?;
            }
        }
        let a = edge::vertex(brep, e)?;
        let b = edge::other_vertex(brep, e, a)?;
        for v in [a, b].into_iter().flatten() {
            if !endpoints.contains(&v) {
                endpoints.push(v);
            }
        }
        // UNPORTED(topology.brep_p4): SmBrep::DeleteEdge
        let (): () = unavailable("SmBrep::DeleteEdge", "topology.brep_p4")?;
        // The native output starts as a default SmExtent2d and is written back
        // unconditionally, even if IsTopological left it untouched.
        let domain = topological.merged_face_domain.unwrap_or_default();
        crate::topology::face::p2::set_uv_domain(brep, keep, &domain)?;
    }
    let mut vertices = relations.vertices.clone();
    for v in endpoints {
        if !vertices.contains(&v) {
            vertices.push(v);
        }
    }
    for v in vertices {
        if !check_all_edges && relations.mates.vertices.other(v).is_none() {
            continue;
        }
        // Upstream's stale-pointer sanity check, before either vertex query.
        if brep.vertex(v)?.vertexuses.len() > 99_999_999 {
            continue;
        }
        if vertex::is_loop_vertex(brep, v)? {
            // UNPORTED(topology.brep_p4): SmBrep::DeleteVertex
            let (): () = unavailable("SmBrep::DeleteVertex", "topology.brep_p4")?;
            continue;
        }
        if vertex::is_topological_vertex(k, brep, v)? {
            let editing = brep.editing_enabled();
            ignore_failure(crate::topology::brep::p5::delete_topological_vertex(
                k,
                brep,
                editing,
                Some(v),
                None,
            ))?;
        }
    }
    // Combine a wire shell with a faceuse shell when the B-rep has just those two
    // and they share a mated vertex.
    let shells = brep_shells(k, brep)?;
    if let [shell0, shell1] = shells[..] {
        let mut wire_shell = None;
        let mut face_shell = None;
        if brep.shell(shell0)?.is_wire_shell() {
            wire_shell = Some(shell0);
            if brep.shell(shell1)?.is_faceuse_shell() {
                face_shell = Some(shell1);
            }
        } else if brep.shell(shell1)?.is_wire_shell() {
            wire_shell = Some(shell1);
            if brep.shell(shell0)?.is_faceuse_shell() {
                face_shell = Some(shell0);
            }
        }
        if let (Some(wire_shell), Some(face_shell)) = (wire_shell, face_shell) {
            // `m_vTI.GetCommonVertices(m_vVertices, m_vOtherVertices)`
            // (`boolean.topology_intersector_p1`, `TopologyIntersector::common_vertices`
            // on the arena mates of `relations`): the B-rep's vertices with an other mate.
            let mut common = false;
            for v in brep_vertices(brep)? {
                if relations.mates.vertices.other(v).is_some() {
                    common = true;
                }
            }
            // If Brep and Other share a vertex then combine shells
            if common {
                // Transfer the wire shell to the face shell
                let wire_region = brep
                    .shell(wire_shell)?
                    .region
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                // `pWShell->TransferChildrenTo(pFShell, NULL)` and
                // `pWR->Remove(pWShell)`: both statuses are ignored upstream.
                ignore_failure(shell::transfer_children(
                    k, brep, wire_shell, face_shell, None,
                ))?;
                ignore_failure(shell::region_remove_shell(brep, wire_region, wire_shell))?;
            }
        }
    }
    Ok(())
}

/// One result region and the identities used by its propagation notification.
#[derive(Debug, Clone, Copy)]
pub struct RegionPropagation {
    /// The result region's notification object.
    pub region: ObjectRef,
    /// The region's `IsVoid` result.
    pub is_void: bool,
    /// Opaque callback handles for the merge attribute's this/other source-region
    /// lists, in that order. Their contents remain in the attribute store; these
    /// handles preserve the two non-null `Notify` data arguments without pointers.
    pub source_lists: [ObjectRef; 2],
}

/// Propagate source-region attributes through a Boolean. `regions` is the
/// result B-rep's `GetRegions` order with each region's `IsVoid` result and
/// source-list identities for the context callbacks.
/// Void regions lose propagating attributes; solid regions with bookkeeping
/// notify once, then every processed region loses its temporary merge attribute.
/// `region_lookup` resolves the source regions the notification handler queries.
pub fn propagate_other_src_region_attribs(
    regions: &[RegionPropagation],
    attributes: &mut AttributeStore,
    mut context: Option<&mut Context>,
    operation: BooleanOperation,
    region_lookup: Option<&dyn shell::RegionLookup>,
) -> Result<()> {
    for &RegionPropagation {
        region,
        is_void,
        source_lists,
    } in regions
    {
        let attrs = attributes.attributes(region.key);
        let merge = attributes.find_attribute(region.key, ids::OTHER_REGION_ID);
        if is_void {
            for h in attrs {
                let attribute = attributes
                    .get(h)
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                if attribute.is_propagated_through_booleans() {
                    attributes.remove_attribute(region.key, h, false)?;
                }
            }
        } else {
            let Some(h) = merge else { continue };
            let data = attributes
                .get_mut(h)
                .and_then(|a| a.merge_region_data_mut())
                .ok_or_else(|| {
                    SolidError::invalid_input(OP, "OTHER_REGION_ID is not a merge-region attribute")
                })?;
            data.operation = operation as i32;
            let [this_sources, other_sources] = source_lists;
            // The handler asks each source region `IsVoid` (topology.shell) through
            // the borrowed lookup.
            a_object::notify_with_regions(
                attributes,
                context.as_deref_mut(),
                NotifyOperation::RegPropagation,
                region,
                Some(this_sources),
                Some(other_sources),
                Some(region),
                region_lookup,
            )?;
        }
        if let Some(h) = merge {
            attributes.remove_attribute(region.key, h, false)?;
        }
    }
    Ok(())
}

/// Inverse translation retained until the entire Boolean operation leaves scope.
/// Restore only operands that survive; extracted components are not operands of
/// the upstream destructor. The constructor remains blocked before any mutation.
#[derive(Debug)]
#[must_use = "restore surviving operands after the complete Boolean scope"]
pub struct TemporaryBrepTransform {
    inverse: Option<Axis2Placement>,
}

impl TemporaryBrepTransform {
    /// Explicit destructor for a completed scope with borrowed operands. `false`
    /// identifies the primary operand and `true` the other; the callback applies
    /// the placement with its live context and notification/attribute state.
    /// The Boolean entry point uses [`Self::run`] to guarantee this on every exit.
    pub fn restore(
        self,
        primary: Option<&mut Brep>,
        other: Option<&mut Brep>,
        mut transform: impl FnMut(&mut Brep, bool, &Axis2Placement, Option<Vec3>, bool) -> Result<()>,
    ) -> Result<()> {
        if let Some(inverse) = self.inverse {
            for (brep, is_other) in [(primary, false), (other, true)] {
                if let Some(brep) = brep {
                    // Transform(sXfm) uses cpOptScale=NULL and bScaleTol=TRUE.
                    crate::error::ignore_status(transform(brep, is_other, &inverse, None, true))?;
                }
            }
        }
        Ok(())
    }

    /// Run the complete Boolean scope, then `~SmTemporaryBrepTransform`.
    /// Cleanup follows editing/mark cleanup, extraction and operand deletion on
    /// every Result exit, including errors and imprint. Restore surviving primary
    /// then other operands; ordinary transform statuses are ignored. Both closures
    /// borrow one environment so cleanup uses the same live kernels, adapter and
    /// attribute/notification state without retaining a competing mutable borrow.
    pub fn run<T, E>(
        self,
        primary: &mut Option<Brep>,
        other: &mut Option<Brep>,
        environment: &mut E,
        action: impl FnOnce(&mut E, &mut Option<Brep>, &mut Option<Brep>) -> Result<T>,
        mut transform: impl FnMut(
            &mut E,
            &mut Brep,
            bool,
            &Axis2Placement,
            Option<Vec3>,
            bool,
        ) -> Result<()>,
    ) -> Result<T> {
        let result = action(environment, primary, other);
        self.restore(
            primary.as_mut(),
            other.as_mut(),
            |brep, is_other, xfm, scale, scale_tol| {
                transform(environment, brep, is_other, xfm, scale, scale_tol)
            },
        )?;
        result
    }
}

/// Begin the scoped origin shift used by manifold Booleans. Bounding boxes are
/// not yet available on arena B-reps; fail before translating either operand.
/// Once wired, the returned token must retain the inverse placement (or `None`
/// when the center's squared length is at most 1e6) until the caller restores it.
pub fn temporary_brep_transform(
    _brep: &mut Brep,
    _other: &mut Brep,
) -> Result<TemporaryBrepTransform> {
    // UNPORTED(query.brep): SmBrep::CalculateBoundingBox
    unavailable("SmBrep::CalculateBoundingBox", "query.brep")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_translation_restores_survivors_after_every_result_exit() {
        for deletes in [(false, false), (true, false), (false, true), (true, true)] {
            for result in [Ok(true), Ok(false), Err(SolidError::sm_err("scope-test"))] {
                let mut inverse = Axis2Placement::IDENTITY;
                inverse.translate(Vec3::new(2001.0, -3002.0, 4003.0));
                let token = TemporaryBrepTransform {
                    inverse: Some(inverse),
                };
                let mut a = Brep::new();
                let va = a
                    .add_vertex_record(crate::topology::Vertex {
                        point: Vec3::X,
                        ..crate::topology::Vertex::default()
                    })
                    .unwrap();
                let mut b = Brep::new();
                let vb = b
                    .add_vertex_record(crate::topology::Vertex {
                        point: Vec3::Y,
                        ..crate::topology::Vertex::default()
                    })
                    .unwrap();
                let mut primary = Some(a);
                let mut other = Some(b);
                let expected = result.clone();
                let mut restored = Vec::new();
                let actual = token.run(
                    &mut primary,
                    &mut other,
                    &mut Vec::new(),
                    |environment, a, b| {
                        environment.push("body");
                        // Represents inner editing-scope cleanup, before inverse.
                        a.as_mut().unwrap().set_editing_enabled(false);
                        b.as_mut().unwrap().set_editing_enabled(false);
                        if deletes.0 {
                            *a = None;
                        }
                        if deletes.1 {
                            *b = None;
                        }
                        result
                    },
                    |environment, brep, is_other, xfm, scale, scale_tol| {
                        assert_eq!(environment.first(), Some(&"body"));
                        assert_eq!(
                            is_other,
                            brep.vertex(brep.vertices().ids().next().unwrap())
                                .unwrap()
                                .point
                                == Vec3::Y
                        );
                        assert!(!brep.editing_enabled());
                        assert_eq!(scale, None);
                        assert!(scale_tol);
                        let id = brep.vertices().ids().next().unwrap();
                        let old = brep.vertex(id).unwrap().point;
                        restored.push(old);
                        brep.vertex_mut(id).unwrap().point = xfm.transform_point(old);
                        // A failed transform does not skip the next operand.
                        Err(SolidError::sm_err("ignored-transform-status"))
                    },
                );
                assert_eq!(actual, expected);
                let expected_order: Vec<_> = [
                    (!deletes.0).then_some(Vec3::X),
                    (!deletes.1).then_some(Vec3::Y),
                ]
                .into_iter()
                .flatten()
                .collect();
                assert_eq!(restored, expected_order);
                if let Some(a) = primary {
                    assert_eq!(
                        a.vertex(va).unwrap().point,
                        inverse.transform_point(Vec3::X)
                    );
                }
                if let Some(b) = other {
                    assert_eq!(
                        b.vertex(vb).unwrap().point,
                        inverse.transform_point(Vec3::Y)
                    );
                }
            }
        }
        TemporaryBrepTransform { inverse: None }
            .run(
                &mut Some(Brep::new()),
                &mut Some(Brep::new()),
                &mut (),
                |(), _, _| Ok(()),
                |(), _, _, _, _, _| panic!("identity scope must not transform"),
            )
            .unwrap();
    }

    #[test]
    fn unmatched_stale_intersections_are_skipped_before_dereferencing() {
        let mut b = Brep::new();
        let r = CleanupRelations {
            edges: vec![EdgeId::from_index(u32::MAX)],
            vertices: vec![VertexId::from_index(u32::MAX)],
            ..CleanupRelations::default()
        };
        // the stale ids are skipped and a B-rep without regions has no shells to count
        boolean_post_process(&mut Kernel::default(), &mut b, &r, false).unwrap();
    }

    #[test]
    fn one_region_fast_path_does_not_inspect_other_faceuse() {
        let mut b = Brep::new();
        let r = b
            .add_region_record(crate::topology::Region {
                in_brep: true,
                ..crate::topology::Region::default()
            })
            .unwrap();
        assert_eq!(
            find_brep_region_of_other_face(
                &Kernel::default(),
                &mut b,
                &mut Brep::new(),
                FaceuseId::from_index(u32::MAX),
                &BTreeMap::new()
            )
            .unwrap(),
            Some(r)
        );
    }

    #[test]
    fn void_regions_remove_propagating_attributes_and_merge_bookkeeping() {
        let mut attrs = AttributeStore::new();
        let r = ObjectRef {
            key: ObjectKey(1),
            type_code: 16012,
        };
        let long = attrs
            .insert(crate::geom::attribute::Attribute::long(
                555,
                29,
                crate::geom::attribute::Behavior::Copy,
            ))
            .unwrap();
        let color = attrs
            .insert(crate::geom::attribute::Attribute::color(
                ids::COLOR,
                Vec3::X,
                crate::geom::attribute::Behavior::StandaloneCopy,
            ))
            .unwrap();
        let merge = attrs
            .insert(crate::geom::attribute::Attribute::merge_region(
                r.key,
                0,
                ObjectKey(2),
                1,
            ))
            .unwrap();
        for h in [long, color, merge] {
            attrs.add_attribute(r.key, h).unwrap();
        }
        propagate_other_src_region_attribs(
            &[RegionPropagation {
                region: r,
                is_void: true,
                source_lists: [r, r],
            }],
            &mut attrs,
            None,
            BooleanOperation::Union,
            None,
        )
        .unwrap();
        assert_eq!(attrs.attributes(r.key), vec![long]);
        assert!(attrs.is_live(color));
        assert!(!attrs.is_live(merge));
    }

    #[test]
    fn defaults_match_upstream() {
        let o = MergeOptions::default();
        assert!(!o.cookie_cutter);
        assert!(!o.imprinting);
        assert!(o.boolean_post_process);
        assert!(!o.imprint_and_classify_faces);
        assert!(!o.keep_other_brep);
        assert_eq!(
            MergeOptions::new(true, false, false, true, true),
            MergeOptions {
                cookie_cutter: true,
                imprinting: false,
                boolean_post_process: false,
                imprint_and_classify_faces: true,
                keep_other_brep: true,
            }
        );
    }
}
