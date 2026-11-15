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
//! | [`check_face_coincidence`], [`finish_face_coincidence`] | `SmMerge::FixFoundFaceCoincidence` (the checks before and after `MergeCoincidentSurfaces`, which the caller runs between them) |
//! | [`find_brep_region_of_other_face`] | `sm_FindBrepRegionOfOtherFace` |
//! | [`edge_borders_solid`] | `sm_EdgeBordersSolid` |
//! | [`merge_vertex`] | `SmMerge::MergeVertex` |
//! | [`boolean_post_process`] | `SmMerge::BooleanPostProcess` |
//! | [`propagate_other_src_region_attribs`] | `SmMerge::PropagateOtherSrcRegionAttribs` |
//! | [`temporary_brep_transform`], [`TemporaryBrepShift`], [`TemporaryBrepTransform`] | `SmTemporaryBrepTransform` (constructor/destructor) |
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
//! caller's complete scope, including error exits. Cooperative cancellation propagates
//! through ignored-status helpers. There is no input-dependent recursion.
//! `BooleanPostProcess`'s `DeleteTopologicalVertex` runs without the caller's
//! notification identities, so the survivor edge's joined curve comes back owned under
//! a kernel-issued identity of that edge; the owner identity the curve of each
//! surviving edge had before the call is put back (upstream the owner is the same
//! `SmEdge*`), so the caller's identity-keyed walks (the temporary transform's inverse
//! `Transform`) still find the curve.
//! Opt-in `Kernel::fixes.boolean_cleanup_removes_slits` (WR-304, #1120): after each
//! edge removal `boolean_post_process` also checks the merged face's edges that have
//! it on both sides (slits), skipping any a later removal deleted.
//! Opt-in `Kernel::fixes.fillet_cleanup_keeps_target_faces` (WR-459, #423): the edges in
//! `CleanupRelations::kept_edges`, and those whose faces are all in `kept_faces`, stay.
//! With that keep (WR-602, the fork's tangency-labs/smlib@cac11ea, the default since its
//! flag was retired): of those, an edge with one face on both sides (a slit) is checked
//! all the same, and the kept-faces test needs two faces. Not the fork's (#15768): a face a
//! merge makes of a kept face is kept, whichever of the two ids it keeps, and the fillet
//! passes as kept edges only the input edges it left whole (`kept_edges`).
//! Not upstream (#15418): `boolean_post_process` merges two faces on different surfaces only
//! where no tolerance has to grow (`edge::is_topological_without_growth`: a plane covers
//! within upstream's plane tolerance, anything else within a tenth of the smaller face
//! tolerance, upstream's threshold for raising none), so no tolerance is raised; upstream
//! lets a curved face cover within five times it and raises only the kept face's, leaving
//! the other face's vertices off the merged face.
//! Opt-in `Kernel::fixes.cover_wraps_across_closed_seam` (WR-599, the fork's
//! tangency-labs/smlib@16bf1cb): after each face merge `boolean_post_process` runs
//! `HasSeamProblem` on the merged face and `SplitAtSeam` when a seam is crossed or missing.
//! Not upstream (WR-586, #483): the temporary origin shift's failed boxes and translations
//! propagate, a failed second translation restores the first operand, and inverse
//! transforms attempt all surviving operands before returning their first error. A
//! callback that fails may already have mutated its operand; this does not promise
//! transactionality within a transform.
//! Not upstream (#3090, WR-756): [`place_vertices_on_face_corners`] (after the boolean's
//! cleanup, a vertex more than the intersection tolerance off one of its faces moves to the
//! corner its faces define, within half its tolerance and only when that cures the gap).
//! Not upstream (#15718): once `boolean_post_process` has deleted its topological
//! vertices, each surviving edge on a line covers the certified gaps the join left
//! (`topology::brep::p5::cover_joined_line_gaps`).

use std::collections::{BTreeMap, BTreeSet};

use crate::boolean::topology_intersector::p1::Relations;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::a_object::{self, BooleanOperation};
#[cfg(test)]
use crate::geom::attribute::ObjectKey;
use crate::geom::attribute::{ids, AttributeStore};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::Context;
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{self as qbrep, BoundingBoxOptions, BrepQuery};
use crate::topology::brep::p4;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::shell;
use crate::topology::topology::{self as topo};
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

fn ignore_failure<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// What `SmMerge::FixFoundFaceCoincidence` does before it merges coincident surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceCoincidenceCheck {
    /// Every edge around both faces has a mate: the faces are taken as coincident
    /// and `FixFoundFaceCoincidence` returns them without merging anything.
    Matched,
    /// A mismatch: the caller merges the coincident surfaces
    /// (`SmTopologyIntersector::MergeCoincidentSurfaces`, unit
    /// `boolean.topology_intersector_p3`) and finishes with
    /// [`finish_face_coincidence`], passing these edge counts of the B-reps (the
    /// primary's, then the other's) from before the merge.
    NeedsMerge {
        /// `pBrep->GetEdges` and `pOther->GetEdges` sizes before the merge.
        edge_counts: (usize, usize),
    },
}

/// The first half of `SmMerge::FixFoundFaceCoincidence`: whether all edges around
/// `face` and `other_face` are mated (`other_mates`: the primary's edges to their
/// other mates, `brep_mates`: the reverse). The test is the existence of mates, as
/// upstream; it deliberately does not require the mates to belong to the supplied
/// opposite face's boundary.
///
/// The split exists because the merge between the halves edits the mate store
/// the maps belong to; [`FaceCoincidenceCheck::NeedsMerge`] hands that step back.
///
/// # Errors
///
/// Those of the face and B-rep edge walks.
pub fn check_face_coincidence(
    brep: &Brep,
    other: &Brep,
    face: FaceId,
    other_face: FaceId,
    other_mates: &BTreeMap<EdgeId, EdgeId>,
    brep_mates: &BTreeMap<EdgeId, EdgeId>,
) -> Result<FaceCoincidenceCheck> {
    let edges = face_edges(brep, face)?;
    let other_edges = face_edges(other, other_face)?;
    if edges.len() == other_edges.len()
        && edges
            .iter()
            .zip(&other_edges)
            .all(|(a, b)| other_mates.contains_key(a) && brep_mates.contains_key(b))
    {
        return Ok(FaceCoincidenceCheck::Matched);
    }
    Ok(FaceCoincidenceCheck::NeedsMerge {
        edge_counts: (brep_edges(brep)?.len(), brep_edges(other)?.len()),
    })
}

/// The second half of `SmMerge::FixFoundFaceCoincidence`, after
/// `MergeCoincidentSurfaces` returned `faces` / `other_faces`: whether either B-rep's
/// edge count differs from `edge_counts` ([`FaceCoincidenceCheck::NeedsMerge`]), that
/// is `rbModifiedTopology`.
///
/// # Errors
///
/// Those of the B-rep edge walks.
pub fn finish_face_coincidence(
    brep: &Brep,
    other: &Brep,
    edge_counts: (usize, usize),
) -> Result<bool> {
    Ok(brep_edges(brep)?.len() != edge_counts.0 || brep_edges(other)?.len() != edge_counts.1)
}

/// Whether the surfaces of two faces coincide within their tolerances. A radial sector
/// only compares the faces at a mated edge, where tangent surfaces that part further
/// along (a fillet against a fillet 1e-4 off) read as coincident; faces are coincident
/// only when their surfaces are. Plane, cone (a cylinder is a cone of semi-angle 0),
/// sphere and torus pairs are decided from their definitions within the larger face
/// tolerance ([`crate::surface::surface::p2::Surface::same_analytic_surface`]), two planes and every other kind keep
/// the sector's answer.
///
/// # Errors
///
/// A null-pointer error for a face without a surface (the radial sector that found
/// the pair has dereferenced both); those of the face bounding boxes.
pub fn faces_coincide(
    k: &Kernel,
    brep: &Brep,
    face: FaceId,
    other: &Brep,
    other_face: FaceId,
) -> Result<bool> {
    // Both faces come from a radial sector, whose classification dereferenced their surfaces.
    let (Some(a), Some(b)) = (
        brep.face(face)?.surface.as_ref(),
        other.face(other_face)?.surface.as_ref(),
    ) else {
        return Err(SolidError::entity_missing(OP, "a face has no surface"));
    };
    // The larger face tolerance, and the two faces together: where the surfaces must agree.
    let (tol, region) =
        crate::query::face::p1::surface_comparison_extent(k, brep, face, other, other_face)?;
    // Two quadrics of one kind part where the sector cannot see: decided from their
    // definitions. The sector decides the rest: two planes (their gap is what its step-off
    // measures), two different kinds (tangent, they can stay within the tolerance over a
    // patch, #2412's sphere cap on a box face), and any kind with no definition to compare
    // (offset, B-spline, extrusion, revolution), where the sampled check is not exact enough
    // to overrule it (offset against offset reads the faces of the golden case
    // `round_merge_deleted_surface_user_problemShell15_face8_inset` as apart and moves it).
    Ok(a.same_analytic_surface(b, tol, &region).unwrap_or(true))
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
    // `GetSize() == 1` and `GetList()`: the B-rep's region list, not its region records
    // (a region `DeleteFace` merged away keeps its record off the list)
    let listed = crate::topology::brep::p1::regions(brep, None)?;
    if listed.len() == 1 {
        if let Some(&region) = listed.first() {
            return Ok(Some(region));
        }
    }
    let edges = eu::faceuse_edgeuses(other, other_faceuse, false, None)?;
    for &other_eu in &edges {
        let e = eu::edge(other, other_eu)?
            .ok_or_else(|| SolidError::entity_missing(OP, "an edge use has no edge"))?;
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
        // a face found coincident here is coincident only when its surface is (WR-755)
        if let (Some(fu), false) = (sector.coincident_faceuse, sector.zero_sector) {
            let face = eu::faceuse_face(brep, fu)?
                .ok_or_else(|| SolidError::entity_missing(OP, "the face use has no face"))?;
            let other_face = eu::face(other, other_eu)?
                .ok_or_else(|| SolidError::entity_missing(OP, "the edge use has no face"))?;
            if !faces_coincide(k, brep, face, other, other_face)? {
                continue;
            }
        }
        let region = if let Some(eu_id) = sector.radial_edgeuse {
            let shell = eu::shell(brep, eu_id)?
                .ok_or_else(|| SolidError::entity_missing(OP, "the edge use has no shell"))?;
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
        let e = eu::edge(other, other_eu)?
            .ok_or_else(|| SolidError::entity_missing(OP, "an edge use has no edge"))?;
        if brep_mates.contains_key(&e) {
            continue;
        }
        let edge = other.edge(e)?;
        let curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::entity_missing(OP, "the edge has no curve"))?;
        let Some(point) =
            ignore_failure(curve.evaluate_point(k, edge.interval.evaluate(0.456789)))?
        else {
            continue;
        };
        // (the classification's construction tolerance, the edge's, is not read)
        let Some(pc) = ignore_failure(classify_point_editing_disabled(
            k,
            &BrepQuery::of_brep(brep),
            brep,
            point,
            0.0,
            false,
        ))?
        else {
            continue;
        };
        // skip points that don't classify to a region
        if pc.point_class() != PointClassType::Region {
            continue;
        }
        // use the point's classified region as the input face's Brep region
        return match pc.object() {
            Some(EntityRef::Region(r)) => Ok(Some(r)),
            _ => Err(SolidError::entity_missing(
                OP,
                "the classified point lies in no region",
            )),
        };
    }
    Ok(None)
}

/// `pBrep->Point3DClassify(point, tolerance, boundary, sPC)` (unit `query.brep`) with
/// `pBrep->m_bEditingEnabled` set to `FALSE` around it (`SmTemporaryChangeValue`), through
/// `query`: the caller keeps one while `b` is not edited, and makes a fresh one after
/// an edit (upstream's `CacheMakeOrValidate` rebuilds an invalidated cache). The
/// operands' geometry is owned by their arena identities
/// ([`crate::query::brep::BrepIdentity::default`]).
///
/// # Errors
///
/// Those of [`BrepQuery::point_3d_classify`].
pub(crate) fn classify_point_editing_disabled(
    k: &Kernel,
    query: &BrepQuery<'_>,
    b: &mut Brep,
    point: Point3,
    tolerance: f64,
    boundary: bool,
) -> Result<PointClassification<EntityRef>> {
    let editing = b.editing_enabled();
    b.set_editing_enabled(false);
    let result = query.point_3d_classify(k, b, point, tolerance, boundary);
    b.set_editing_enabled(editing);
    result
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
        let region = eu::shell_region(b, shell)?
            .ok_or_else(|| SolidError::entity_missing(OP, "the shell lies in no region"))?;
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
    pub records: &'a mut crate::topology::topology::NotifyRecords,
    /// Attributes of both operands.
    pub attributes: &'a mut AttributeStore,
    /// Notification identity of each other-operand vertex.
    pub source_key: &'a dyn Fn(VertexId) -> ObjectRef,
    /// Notification identity of each primary topology entity, including newly created vertices and vertex uses.
    pub destination_key: &'a dyn Fn(EntityRef) -> ObjectRef,
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
    // Both vertices are SmTopology objects upstream; a record this crate has not made
    // yet (a vertex the merge did not create) is the default one.
    let source_ref = (copy.source_key)(source);
    let dest_ref = (copy.destination_key)(EntityRef::Vertex(dest));
    copy.records.or_default(source_ref.key);
    copy.records.or_default(dest_ref.key);
    topo::notify(
        copy.records,
        copy.attributes,
        context,
        NotifyOperation::Copy,
        source_ref,
        Some(dest_ref),
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
    /// WR-459 (`Kernel::fixes.fillet_cleanup_keeps_target_faces`): the edges and faces
    /// the clean-up leaves alone (the fork's `m_sPostProcessKeptEdges`/`Faces`: a
    /// fillet's target, the input edges it left whole), and any edge between two kept
    /// faces, or faces merged from them. Empty otherwise.
    pub kept_edges: BTreeSet<EdgeId>,
    /// See [`CleanupRelations::kept_edges`].
    pub kept_faces: BTreeSet<FaceId>,
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
    crate::profile::profile_scope!(k, "boolean::post_process");
    boolean_post_process_with_notifications(k, brep, relations, check_all_edges, None)
}

/// [`boolean_post_process`] with the merge's notification state: `DeleteEdge`'s face
/// merges, `DeleteVertex` and `DeleteTopologicalVertex`'s edge merges notify through
/// `notifications` (its records, attributes and identities), so the merged faces and
/// edges keep the attributes of the objects merged into them (`SM_NO_MERGE_IN_BREP`).
/// Without it each deletion notifies through a store of its own, as for a caller
/// holding no notification state.
///
/// # Errors
/// As [`boolean_post_process`].
pub(crate) fn boolean_post_process_with_notifications(
    k: &mut Kernel,
    brep: &mut Brep,
    relations: &CleanupRelations,
    check_all_edges: bool,
    mut notifications: Option<&mut edge::NotifyEnv<'_>>,
) -> Result<()> {
    let mut edges: Vec<_> = if check_all_edges {
        brep_edges(brep)?
    } else {
        relations.edges.clone()
    };
    let mut endpoints = Vec::new();
    // WR-304: the edges checked so far, and those the merging below left with one face
    // on both sides (checked too, though they are no intersection edges).
    let slits = k.fixes.boolean_cleanup_removes_slits;
    let mut queued: BTreeSet<EdgeId> = if slits {
        edges.iter().copied().collect()
    } else {
        BTreeSet::new()
    };
    let mut left_inside_face: BTreeSet<EdgeId> = BTreeSet::new();
    // WR-459: the input's faces, and each face a merge below makes of one of them: the
    // merged face is that input face continued, whichever of the two ids it keeps.
    let mut kept_faces = relations.kept_faces.clone();
    let mut next = 0;
    while let Some(&e) = edges.get(next) {
        next += 1;
        if left_inside_face.contains(&e) {
            // A later removal may have deleted it
            if !brep.edge_list().contains(&e) {
                continue;
            }
        } else if !check_all_edges && relations.mates.edges.other(e).is_none() {
            continue;
        }
        // WR-602 (the fork's tangency-labs/smlib@cac11ea): an edge of the fillet's input,
        // or one between two of its faces, stays, unless one face lies on both its sides:
        // removing that slit merges no faces.
        if relations.kept_edges.contains(&e) && edge::faces(brep, e, None)?.len() != 1 {
            continue;
        }
        if !kept_faces.is_empty() {
            let faces = edge::faces(brep, e, None)?;
            if faces.len() >= 2 && faces.iter().all(|f| kept_faces.contains(f)) {
                continue;
            }
        }
        // Not upstream (#15418): faces on different surfaces merge only where neither face's
        // tolerance has to grow (`is_topological_without_growth`: a plane covers within
        // upstream's plane tolerance, anything else within a tenth of the smaller face
        // tolerance). Upstream lets a curved surface cover within five times that tolerance
        // and raises only the kept face's: the vertices and edges brought over from the other
        // face keep theirs and lie that far off the merged face (a vertex-face gap the check
        // refuses), and the shape moves by as much (two rounds a ten-thousandth of a degree
        // apart became one cylinder, its far end 50 times the angle off).
        let topological = edge::is_topological_without_growth(k, brep, e)?;
        if !topological.is_topological {
            continue;
        }
        let keep = topological
            .surf_to_keep
            .ok_or_else(|| SolidError::entity_missing(OP, "the edge has no surface to keep"))?;
        let edgeuses = edge::edgeuses(brep, e, None, None)?;
        let first = edgeuses
            .first()
            .copied()
            .ok_or_else(|| SolidError::entity_missing(OP, "the edge has no first edge use"))?;
        let third = edgeuses
            .get(2)
            .copied()
            .ok_or_else(|| SolidError::entity_missing(OP, "the edge has no third edge use"))?;
        let f0 = eu::face(brep, first)?
            .ok_or_else(|| SolidError::entity_missing(OP, "the edge use has no face"))?;
        let f1 = eu::face(brep, third)?
            .ok_or_else(|| SolidError::entity_missing(OP, "the edge use has no face"))?;
        // Surface identity is represented by its owning face in this arena model.
        if f0 != f1 {
            // (upstream raises the kept face's tolerance by the distance past a tenth of it:
            // the cover's gain keeps the distance within that, so nothing is raised)
            let domain = brep
                .face(keep)?
                .surface
                .as_ref()
                .ok_or_else(|| SolidError::entity_missing(OP, "the face to keep has no surface"))?
                .natural_uv_domain_with_nurb(k)?;
            crate::topology::face::p2::set_uv_domain(brep, keep, &domain)?;
        }
        let a = edge::vertex(brep, e)?;
        let b = edge::other_vertex(brep, e, a)?;
        for v in [a, b].into_iter().flatten() {
            if !endpoints.contains(&v) {
                endpoints.push(v);
            }
        }
        // SER(DeleteEdge(pEdge, pSurfToKeep))
        match notifications.as_deref_mut() {
            Some(env) => {
                p4::delete_edge(k, brep, &mut p4::DeleteEnv::from_notify(env), e, Some(keep))?;
            }
            None => p4::with_local_env(|env| p4::delete_edge(k, brep, env, e, Some(keep)))?,
        }
        // `pSurfToKeep->GetFace()`: when the face holding the kept surface is the one
        // merged away, DeleteEdge swaps that surface onto the surviving face first
        let keep = if brep.face(keep).is_ok() {
            keep
        } else if keep == f0 {
            f1
        } else {
            f0
        };
        if kept_faces.contains(&f0) || kept_faces.contains(&f1) {
            kept_faces.insert(keep);
        }
        // The native output starts as a default SmExtent2d and is written back
        // unconditionally, even if IsTopological left it untouched.
        let domain = topological.merged_face_domain.unwrap_or_default();
        crate::topology::face::p2::set_uv_domain(brep, keep, &domain)?;
        if k.fixes.cover_wraps_across_closed_seam {
            // WR-599 (the fork's tangency-labs/smlib@16bf1cb): a face that wraps its
            // surface's period may have taken in a face straddling the seam
            // (`CoverCoincidentSurface`): the merged face is the periodic face whose seam
            // runs on through that part. Add the missing piece of the seam, splitting the
            // edges that cross it, as a face on a periodic surface has it.
            crate::topology::face::p1::complete_merged_face_seam(
                k,
                brep,
                notifications.as_deref_mut(),
                keep,
            )?;
        }
        if slits {
            // WR-304: an operand edge between two faces that are now one bounds the
            // merged face on both sides (a slit); check it by the same test.
            for face_edge in face_edges(brep, keep)? {
                if edge::faces(brep, face_edge, None)?.len() == 1 && queued.insert(face_edge) {
                    edges.push(face_edge);
                    left_inside_face.insert(face_edge);
                }
            }
        }
    }
    let mut vertices = relations.vertices.clone();
    for v in endpoints {
        if !vertices.contains(&v) {
            vertices.push(v);
        }
    }
    // Not upstream (#15718): the edges that took over a deleted vertex's two edges, whose
    // gaps are covered once every deletion is done (`cover_joined_line_gaps`).
    let mut joined = Vec::new();
    for v in vertices {
        if !check_all_edges && relations.mates.vertices.other(v).is_none() {
            continue;
        }
        // Upstream's stale-pointer sanity check, before either vertex query.
        if brep.vertex(v)?.vertexuses.len() > 99_999_999 {
            continue;
        }
        if vertex::is_loop_vertex(brep, v)? {
            // SER(DeleteVertex(pVertex))
            match notifications.as_deref_mut() {
                Some(env) => {
                    p4::delete_vertex(k, brep, &mut p4::DeleteEnv::from_notify(env), v)?;
                }
                None => p4::with_local_env(|env| p4::delete_vertex(k, brep, env, v))?,
            }
            continue;
        }
        if vertex::is_topological_vertex(k, brep, v)? {
            let editing = brep.editing_enabled();
            // The owners of the two edges' curves: the survivor's joined curve is owned
            // by the same edge (`SetCurve`), under the caller's identity of that edge.
            let mut owners = Vec::new();
            for e in vertex::edges(brep, v, None)? {
                owners.push((
                    e,
                    brep.edge(e)?
                        .curve
                        .as_ref()
                        .and_then(crate::curve::curve::p2::Curve::owner),
                    brep.geometry_owner_key(EntityRef::Edge(e)),
                ));
            }
            if let Some(survivor) = ignore_failure(
                crate::topology::brep::p5::delete_topological_vertex_joining(
                    k,
                    brep,
                    editing,
                    Some(v),
                    None,
                    notifications.as_deref_mut(),
                ),
            )? {
                joined.push(survivor);
            }
            for (e, owner, proven) in owners {
                if !brep.edge_list().contains(&e) {
                    continue;
                }
                let restored = brep.edge_mut(e)?.curve.as_mut().is_some_and(|curve| {
                    let changed = curve.owner() != owner;
                    if changed {
                        curve.set_owner(owner, None);
                    }
                    changed
                });
                if restored {
                    brep.set_geometry_owner_key(EntityRef::Edge(e), proven);
                }
            }
        }
    }
    for e in joined {
        crate::topology::brep::p5::cover_joined_line_gaps(k, brep, e)?;
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
                let wire_region = brep.shell(wire_shell)?.region.ok_or_else(|| {
                    SolidError::entity_missing(OP, "the wire shell lies in no region")
                })?;
                // `pWShell->TransferChildrenTo(pFShell, NULL)` and
                // `pWR->Remove(pWShell)`: both statuses are ignored upstream (fork e7ebc8d
                // passes them on: `SER`).
                ignore_failure(crate::error::passed_on(
                    shell::transfer_children(k, brep, wire_shell, face_shell, None),
                    k.fixes.brep_edit_statuses_pass_on,
                )?)?;
                ignore_failure(crate::error::passed_on(
                    shell::region_remove_shell(brep, wire_region, wire_shell),
                    k.fixes.brep_edit_statuses_pass_on,
                )?)?;
            }
        }
    }
    Ok(())
}

/// Not upstream (#3090, #16532): where `v` should stand when it lies more than the B-rep's
/// intersection tolerance (`xsect_tol`) off one of its faces: the point that minimises the
/// largest gap to its faces ([`min_max_gap_point`]; the corner its faces define
/// ([`calc_corner_from_faces`](crate::topology::vertex::corner::calc_corner_from_faces): the
/// midpoint of two parallel faces that part, else the faces' surface intersection) when every
/// face holds it), or `None` to leave the vertex where it is. Left alone: a vertex whose
/// tolerance is not past `xsect_tol` (it cannot hold such a gap), one already within `xsect_tol`
/// of its faces, and one whose point
///
/// - lies more than half the vertex's tolerance from it. The vertex's edge curves keep ending
///   at its old point (`vertex::set_point` moves the point only), so the move is their new
///   vertex/edge gap, and `docs/validity-and-tolerances.md` (the `SmBrep` list 2 invariants)
///   asks vertex/edge and vertex/face gaps of at most half the vertex's tolerance. Only the
///   point's own displacement is limited, not the corner's: the corner may lie farther than
///   the point.
/// - leaves a gap of `xsect_tol` or more to any of its faces, or of more than half the vertex's
///   tolerance to the end of any incident edge curve (both measured with the vertex put
///   at the point for the measurement only): a partial move would stay invalid.
///
/// # Errors
/// Those of the vertex/face gap and corner calculations.
fn face_corner_to_move_to(
    k: &Kernel,
    brep: &mut Brep,
    v: VertexId,
    xsect_tol: f64,
) -> Result<Option<Point3>> {
    use crate::topology::vertex::corner;
    let tolerance = vertex::tolerance(brep, v)?;
    if tolerance <= xsect_tol || corner::is_within_xsect_tol_3d_of_faces(k, brep, v, false)?.0 {
        return Ok(None);
    }
    let at = corner::calc_corner_from_faces(k, brep, v)?;
    let old = brep.vertex(v)?.point;
    // Faces that define no corner (a round and the flat face it is tangent to, with a third
    // face a hair off) still have a least-largest-gap point: start from where the vertex is.
    let start = if at.found_point && !at.best_point.distance(old).is_nan() {
        at.best_point
    } else {
        old
    };
    // one construction: the point that minimises the largest gap to the vertex's faces
    let target = min_max_gap_point(k, brep, v, start)?;
    if target.distance(old) > tolerance / 2.0 {
        return Ok(None);
    }
    brep.vertex_mut(v)?.point = target;
    let cured = corner_cures_gaps(k, brep, v, tolerance / 2.0);
    brep.vertex_mut(v)?.point = old;
    let cured = cured?;
    reset_vertex_face_gaps(brep, v)?;
    Ok(cured.then_some(target))
}

/// Not upstream (#16532): the point that minimises the largest gap to the faces of `v`, from
/// `corner` where its faces meet. A face's gap is the distance to its nearest point, so the
/// optimum `x` is where the vectors to the nearest points of the active faces balance: `x` is
/// the centre of the smallest ball holding them (the midpoint for two faces, the circumcentre for
/// three). [`minimax_centre`] iterates that map from the corner, with the nearest
/// points of each face to the vertex put at `x` ([`nearest_face_points`]).
///
/// # Errors
/// Those of the gap and surface evaluations.
fn min_max_gap_point(k: &Kernel, brep: &mut Brep, v: VertexId, corner: Point3) -> Result<Point3> {
    let old = brep.vertex(v)?.point;
    // Nearest points closer than a millionth of the intersection tolerance are the same point
    // rounded differently (coordinates of order 1e3 carry rounding of 1e-13).
    let merge = 1e-6 * vertex::corner::xsect_tol_3d_of_brep(k, brep);
    let found = minimax_centre(corner, merge, |x| nearest_face_points(k, brep, v, x));
    brep.vertex_mut(v)?.point = old;
    reset_vertex_face_gaps(brep, v)?;
    found
}

/// The nearest point of each face of `v` to `x` (the vertex is left at `x`).
fn nearest_face_points(k: &Kernel, brep: &mut Brep, v: VertexId, x: Point3) -> Result<Vec<Point3>> {
    use crate::topology::vertexuse;
    brep.vertex_mut(v)?.point = x;
    let mut points = Vec::new();
    for vu in vertex::vertexuses(brep, v)? {
        let Some(gap) = vertexuse::vertex_face_gap(k, brep, vu, true)? else {
            continue;
        };
        let face = match gap.vertexuse {
            Some(gvu) => vertexuse::faceuse(brep, gvu)?
                .map(|fu| brep.faceuse(fu).map(|f| f.face))
                .transpose()?
                .flatten(),
            None => None,
        };
        let Some(surface) = face
            .and_then(|f| brep.face(f).ok())
            .and_then(|f| f.surface.as_ref())
        else {
            continue;
        };
        let p = surface.evaluate_point(k, gap.face_uv)?;
        if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() {
            points.push(p);
        }
    }
    Ok(points)
}

/// The most steps [`minimax_centre`] takes. The map has no proved rate: for two faces meeting at
/// a shallow angle each step keeps nearly all of the distance to the optimum (about 99.97 % at
/// 1 degree), so the result after this many steps is an approximation of the optimum, never a
/// certified one. That is safe because the point is only a candidate: `face_corner_to_move_to`
/// moves the vertex only when `corner_cures_gaps` measures every face and edge gap at it
/// within bounds, and otherwise leaves the vertex alone. A start on the corner of faces that
/// all hold it is already the optimum, so the common case never iterates.
const MINIMAX_STEPS: usize = 64;

/// The point minimising the largest distance to a set of faces, from `start`: with
/// `nearest(x)` the nearest point of each face to `x`, repeatedly moves `x` to the centre of
/// the smallest ball holding them ([`enclosing_ball_centre`]), until it stops. At the optimum
/// the vectors from `x` to the nearest points of the active faces balance (`x` is the centre of
/// their smallest ball), so it is the fixed point; the part of `x` along a direction every face
/// holds (an edge they share) never changes. For a corner on every face the nearest points
/// are `x` itself and `x` stays.
fn minimax_centre(
    start: Point3,
    merge: f64,
    mut nearest: impl FnMut(Point3) -> Result<Vec<Point3>>,
) -> Result<Point3> {
    let mut x = start;
    for _ in 0..MINIMAX_STEPS {
        let points = nearest(x)?;
        let Some(next) = enclosing_ball_centre(&points, merge) else {
            break;
        };
        let step = next.distance(x);
        let scale = 1.0 + x.length();
        x = next;
        if step <= 1e-12 * scale {
            break;
        }
    }
    Ok(x)
}

/// The centre of the smallest ball holding `points` (`None` for none, or for more than
/// [`MAX_WELZL_POINTS`] distinct ones). Points within `merge` of one another count once: every
/// vertex use of a face gives that face's nearest point again, the same point up to rounding,
/// and taking the noise between them as separate points makes Welzl's support sets nearly
/// degenerate. The ball of the distinct points is then exact, by Welzl's algorithm in the order
/// the points come (no randomness).
fn enclosing_ball_centre(points: &[Point3], merge: f64) -> Option<Point3> {
    let mut distinct: Vec<Point3> = Vec::new();
    for &p in points {
        if !distinct.iter().any(|q| q.distance(p) <= merge) {
            distinct.push(p);
        }
    }
    if distinct.len() > MAX_WELZL_POINTS {
        return None;
    }
    let mut support = Vec::with_capacity(4);
    welzl(&distinct, distinct.len(), &mut support).map(|(_, c)| c)
}

/// The most points [`enclosing_ball_centre`] takes: Welzl's recursion is one level per point.
const MAX_WELZL_POINTS: usize = 4096;

/// The smallest ball (radius, centre) holding `points[..n]` with `support` on its surface.
/// Recursion depth is at most `n`, bounded by [`MAX_WELZL_POINTS`] at the entry.
fn welzl(points: &[Point3], n: usize, support: &mut Vec<Point3>) -> Option<(f64, Point3)> {
    let Some(last) = n.checked_sub(1).and_then(|i| points.get(i)).copied() else {
        return ball_on(support);
    };
    if support.len() == 4 {
        return ball_on(support);
    }
    let ball = welzl(points, n - 1, support)?;
    if last.distance(ball.1) <= ball.0 * (1.0 + 1e-12) {
        return Some(ball);
    }
    support.push(last);
    let with = welzl(points, n - 1, support);
    support.pop();
    with
}

/// The smallest ball with all of `support` (at most four points) on its surface.
fn ball_on(support: &[Point3]) -> Option<(f64, Point3)> {
    let centre = match support {
        [] => return Some((0.0, Point3::new(0.0, 0.0, 0.0))),
        [a] => *a,
        [a, b] => 0.5 * (*a + *b),
        [a, b, c] => circumcentre3(*a, *b, *c)?,
        [a, b, c, d] => circumcentre4(*a, *b, *c, *d)?,
        _ => return None,
    };
    let radius = support.first().map_or(0.0, |p| p.distance(centre));
    Some((radius, centre))
}

/// The centre of the circle through three points (`None` when they are on a line).
fn circumcentre3(a: Point3, b: Point3, c: Point3) -> Option<Point3> {
    let u = b - a;
    let w = c - a;
    let n = u.cross(w);
    let n2 = n.dot(n);
    if n2 <= 1e-24 * u.dot(u) * w.dot(w) || n2 == 0.0 {
        return None;
    }
    let offset = (0.5 / n2) * (w.dot(w) * n.cross(u) + u.dot(u) * w.cross(n));
    Some(a + offset)
}

/// The centre of the sphere through four points (`None` when they are in a plane).
fn circumcentre4(a: Point3, b: Point3, c: Point3, d: Point3) -> Option<Point3> {
    let u = b - a;
    let w = c - a;
    let t = d - a;
    let det = u.dot(w.cross(t));
    if det.abs() <= 1e-12 * u.length() * w.length() * t.length() || det == 0.0 {
        return None;
    }
    let offset =
        (u.dot(u) * w.cross(t) + w.dot(w) * t.cross(u) + t.dot(t) * u.cross(w)) * (0.5 / det);
    Some(a + offset)
}

/// With `v` at the point under test: every vertex/face gap is under the intersection tolerance
/// and every vertex/edge gap (its edge curves end where they did) at most `max_edge_gap`.
fn corner_cures_gaps(k: &Kernel, brep: &mut Brep, v: VertexId, max_edge_gap: f64) -> Result<bool> {
    use crate::topology::vertexuse;
    if !vertex::corner::is_within_xsect_tol_3d_of_faces(k, brep, v, true)?.0 {
        return Ok(false);
    }
    for vu in vertex::vertexuses(brep, v)? {
        if let Some(gap) = vertexuse::vertex_edge_gap(k, brep, vu, true)? {
            if gap.gap_3d.is_nan() || gap.gap_3d > max_edge_gap {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The gaps cached on the uses of `v` are of a point it no longer has.
fn reset_vertex_face_gaps(brep: &mut Brep, v: VertexId) -> Result<()> {
    for vu in vertex::vertexuses(brep, v)? {
        let r = brep.vertexuse_mut(vu)?;
        r.vertex_face_gap.reset();
        r.vertex_edge_gap.reset();
    }
    Ok(())
}

/// Not upstream (#3090): after the boolean's cleanup, moves each of `vertices` that
/// [`face_corner_to_move_to`] says to the corner its faces define.
/// `SmVertex::AssertValid` requires every vertex/face gap under the B-rep's intersection
/// tolerance whatever the vertex's own, and a vertex the imprint kept where a vertex of an
/// operand stood (its ball holds the other operand's point, `merge_vertex_neighborhood`) lies
/// on one face and a sliver off the other, as the two tangent fillets of operands a hair
/// apart part by dx^2 / 4. The step sits after the cleanup, not at that branch, because the
/// cleanup is where the vertex's faces and edges are final: the corner is computed from the
/// faces it keeps, and the vertices the cleanup deletes are not moved first.
///
/// # Errors
/// Those of [`face_corner_to_move_to`] and of `SmVertex::SetPoint`; a body that is not a closed
/// manifold solid is left as it is (#16532: moving its vertices only hides its defect); a vertex the cleanup
/// deleted ([`SolidError::NotFound`]) is skipped.
pub(crate) fn place_vertices_on_face_corners(
    k: &mut Kernel,
    brep: &mut Brep,
    vertices: &[VertexId],
    notifications: &mut edge::NotifyEnv<'_>,
) -> Result<()> {
    // Only a closed manifold solid is placed: a body that is not one (the #1979 ring-lip union
    // at 1e-3 has one region and no solid) is defective beyond a vertex gap, and moving its
    // vertices only hides the defect from the validity check.
    if !crate::topology::brep::p2::is_manifold_solid(brep)? {
        return Ok(());
    }
    let xsect_tol = vertex::corner::xsect_tol_3d_of_brep(k, brep);
    for &v in vertices {
        match brep.vertex(v) {
            Ok(_) => {}
            Err(SolidError::NotFound { .. }) => continue,
            Err(e) => return Err(e),
        }
        let Some(at) = face_corner_to_move_to(k, brep, v, xsect_tol)? else {
            continue;
        };
        let this = (notifications.key_of)(EntityRef::Vertex(v));
        vertex::set_point(
            brep,
            v,
            at,
            notifications.records,
            notifications.attributes,
            notifications.context.as_deref_mut(),
            this,
            &|vu| (notifications.key_of)(EntityRef::Vertexuse(vu)),
        )?;
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
                let attribute = attributes.get(h).ok_or_else(|| {
                    SolidError::entity_missing(
                        OP,
                        "an attribute handle is not in the attribute table",
                    )
                })?;
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
    /// Attempts both surviving operands even after an error, then returns the first
    /// failed transform's error (WR-586).
    pub fn restore(
        self,
        primary: Option<&mut Brep>,
        other: Option<&mut Brep>,
        mut transform: impl FnMut(&mut Brep, bool, &Axis2Placement, Option<Vec3>, bool) -> Result<()>,
    ) -> Result<()> {
        let mut result = Ok(());
        if let Some(inverse) = self.inverse {
            for (brep, is_other) in [(primary, false), (other, true)] {
                if let Some(brep) = brep {
                    // Transform(sXfm) uses cpOptScale=NULL and bScaleTol=TRUE.
                    let status = transform(brep, is_other, &inverse, None, true);
                    if result.is_ok() {
                        result = status;
                    }
                }
            }
        }
        result
    }

    /// Run the complete Boolean scope, then `~SmTemporaryBrepTransform`.
    /// Cleanup follows editing/mark cleanup, extraction and operand deletion on
    /// every Result exit, including errors and imprint. Restore surviving primary
    /// then other operands; failed restoration takes precedence over the action's
    /// result. Both closures borrow one environment so cleanup uses the same live
    /// kernels, adapter and attribute/notification state without retaining a
    /// competing mutable borrow.
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

/// The squared distance from the origin past which [`temporary_brep_transform`] moves the
/// operands' box center to the origin (upstream: "Arbitrary: > 1000").
const MIN_SHIFT_CENTER_LENGTH_SQ: f64 = 1.0e6;

/// The first half of `SmTemporaryBrepTransform(pTI)`, the scoped origin shift of
/// manifold Booleans: the union of the operands' boxes (`brep` boxed in
/// `primary_kernel`'s context, `other` in `other_kernel`'s; a failure is returned, where
/// upstream left the box initialised, WR-586) gives a center; when its squared length
/// exceeds `MIN_SHIFT_CENTER_LENGTH_SQ` the operands need the translation by `-center`.
/// [`TemporaryBrepShift::apply`] is the second half (the boxes are computed before either
/// operand moves, as upstream).
///
/// # Errors
///
/// Box errors.
pub fn temporary_brep_transform(
    primary_kernel: &Kernel,
    other_kernel: &Kernel,
    brep: &Brep,
    other: &Brep,
) -> Result<TemporaryBrepShift> {
    // A failed box must not select a shift from an unwritten extent.
    let bbox1 = qbrep::calculate_bounding_box(primary_kernel, brep, BoundingBoxOptions::default())?;
    let bbox2 = qbrep::calculate_bounding_box(other_kernel, other, BoundingBoxOptions::default())?;
    let bbox = bbox1.union(&bbox2);

    // when bbox is very large - translate its center to the origin
    let center = bbox.evaluate(0.5, 0.5, 0.5);
    let shift = (center.length_squared() > MIN_SHIFT_CENTER_LENGTH_SQ).then(|| -center);
    Ok(TemporaryBrepShift { shift })
}

/// The translation [`temporary_brep_transform`] decided on (`m_bNeedsTranslation`,
/// `-m_sCenter`).
#[derive(Debug)]
#[must_use = "apply the shift to the operands"]
pub struct TemporaryBrepShift {
    shift: Option<Vec3>,
}

impl TemporaryBrepShift {
    /// The rest of `SmTemporaryBrepTransform(pTI)`: when a shift is needed, translates
    /// the primary operand (`false`) then the other (`true`) through `transform`
    /// (`Transform(sXfm)`: no scale, scaled tolerances). The token keeps the inverse
    /// translation for [`TemporaryBrepTransform::restore`]. A failed transform stops
    /// the operation, where upstream ignored its status (WR-586). If the second
    /// fails, first undo the successful primary translation. A failed undo takes
    /// precedence over that second transform's error. A transform's own partial
    /// mutations are not rolled back.
    ///
    /// # Errors
    ///
    /// Any transform error.
    pub fn apply(
        self,
        brep: &mut Brep,
        other: &mut Brep,
        mut transform: impl FnMut(&mut Brep, bool, &Axis2Placement, Option<Vec3>, bool) -> Result<()>,
    ) -> Result<TemporaryBrepTransform> {
        let Some(shift) = self.shift else {
            return Ok(TemporaryBrepTransform { inverse: None });
        };
        let mut xfm = Axis2Placement::IDENTITY;
        xfm.translate(shift);
        // (the destructor negates the shift again)
        let mut inverse = Axis2Placement::IDENTITY;
        inverse.translate(-shift);
        transform(brep, false, &xfm, None, true)?;
        if let Err(error) = transform(other, true, &xfm, None, true) {
            transform(brep, false, &inverse, None, true)?;
            return Err(error);
        }
        Ok(TemporaryBrepTransform {
            inverse: Some(inverse),
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// A 1 x 2 x 3 box's origin corner (three faces, none parallel), moved `d` along x,
    /// with vertex tolerance `tolerance`.
    fn box_corner_off(d: f64, tolerance: f64) -> (Kernel, Brep, VertexId) {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, 1.0, 2.0, 3.0)
            .unwrap();
        let v = b
            .vertices()
            .ids()
            .find(|&v| b.vertex(v).unwrap().point.distance(Vec3::ZERO) == 0.0)
            .unwrap();
        let r = b.vertex_mut(v).unwrap();
        r.point = Vec3::new(d, 0.0, 0.0);
        r.tolerance = tolerance;
        (k, b, v)
    }

    // CodeRabbit on #16538: the midpoint of the nearest points to the corner is not the point
    // of least largest gap. A face y = x through the corner and one on y = 0 from x = d: the
    // midpoint (d / 2, 0) is d / 2 from the second, (3d / 4, d / 4) is d / (2 sqrt 2) from both.
    #[test]
    fn the_least_largest_gap_point_balances_the_faces() {
        let d = 5.0e-5;
        let nearest = |x: Point3| -> Result<Vec<Point3>> {
            let on_line = 0.5 * (x.x + x.y);
            Ok(vec![
                Vec3::new(on_line, on_line, x.z),
                Vec3::new(x.x.max(d), 0.0, x.z),
            ])
        };
        let at = minimax_centre(Vec3::ZERO, 1e-12, nearest).unwrap();
        let want = Vec3::new(0.75 * d, 0.25 * d, 0.0);
        assert!(at.distance(want) < 1e-12, "{at:?}");
        let gap = |p: Point3| {
            let to_line = (p.x - p.y).abs() / 2.0_f64.sqrt();
            let to_ray = Vec3::new(p.x - p.x.max(d), p.y, 0.0).length();
            to_line.max(to_ray)
        };
        assert!(gap(at) < 0.4 * d, "{}", gap(at));
        assert!(gap(at) < gap(Vec3::new(0.5 * d, 0.0, 0.0)));
        // a corner every face holds stays where it is
        let held = |x: Point3| -> Result<Vec<Point3>> { Ok(vec![x, x]) };
        assert_eq!(minimax_centre(Vec3::ZERO, 1e-12, held).unwrap(), Vec3::ZERO);
    }

    // CodeRabbit on #16538: three faces whose nearest points are the corners of an equilateral
    // triangle of side 3e-5: the midpoint of a side is 2.6e-5 from the third, the centroid
    // 1.73e-5 from each.
    #[test]
    fn the_least_largest_gap_point_of_three_faces_is_the_centroid() {
        let s = 3.0e-5;
        let corners = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(s, 0.0, 0.0),
            Vec3::new(0.5 * s, 0.5 * s * 3.0_f64.sqrt(), 0.0),
        ];
        let nearest = |_: Point3| -> Result<Vec<Point3>> { Ok(corners.to_vec()) };
        let at = minimax_centre(Vec3::ZERO, 1e-12, nearest).unwrap();
        let want = Vec3::new(0.5 * s, s / (2.0 * 3.0_f64.sqrt()), 0.0);
        assert!(at.distance(want) < 1e-12, "{at:?}");
        // an obtuse triangle: the midpoint of its longest side
        let flat = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(s, 0.0, 0.0),
            Vec3::new(0.5 * s, 0.1 * s, 0.0),
        ];
        let at = minimax_centre(Vec3::ZERO, 1e-12, |_| Ok(flat.to_vec())).unwrap();
        assert!(at.distance(Vec3::new(0.5 * s, 0.0, 0.0)) < 1e-12, "{at:?}");
    }

    // A vertex with many faces (a cone apex): more points than the exhaustive search takes go to
    // Welzl's algorithm, which gives the same ball. Twenty points on a circle of radius 2 about
    // (1, 2, 3), with interior points among them: the centre of the circle.
    #[test]
    fn the_smallest_ball_of_many_points_is_found_without_the_exhaustive_search() {
        let centre = Vec3::new(1.0, 2.0, 3.0);
        let mut points: Vec<Point3> = (0..20)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::TAU / 20.0;
                centre + Vec3::new(2.0 * a.cos(), 2.0 * a.sin(), 0.0)
            })
            .collect();
        points.push(centre + Vec3::new(0.3, -0.2, 0.0));
        points.push(centre + Vec3::new(-1.0, 0.5, 0.0));
        let at = enclosing_ball_centre(&points, 1e-12).unwrap();
        assert!(at.distance(centre) < 1e-9, "{at:?}");
    }

    // The case that broke the slab and ring-and-lip joins: each face's nearest point comes once
    // per vertex use, equal up to rounding. Repeating the points of a triangle 1e-5 across with
    // noise of 1e-15 gives the centre of the triangle's smallest ball.
    #[test]
    fn near_duplicate_points_give_the_ball_of_the_distinct_ones() {
        let s = 1.0e-5;
        let base = Vec3::new(10.0, 20.0, 30.0);
        let distinct = [
            base,
            base + Vec3::new(s, 0.0, 0.0),
            base + Vec3::new(0.5 * s, 0.5 * s * 3.0_f64.sqrt(), 0.0),
        ];
        let want = enclosing_ball_centre(&distinct, 1e-11).unwrap();
        let mut noisy = Vec::new();
        let mut count = 0.0;
        for _ in 0..6 {
            for p in &distinct {
                count += 1.0;
                let n = 1e-15 * count;
                noisy.push(*p + Vec3::new(n, -n, n));
            }
        }
        let at = enclosing_ball_centre(&noisy, 1e-11).unwrap();
        assert!(at.distance(want) < 1e-13, "{at:?} {want:?}");
        assert!(at.distance(base + Vec3::new(0.5 * s, s / (2.0 * 3.0_f64.sqrt()), 0.0)) < 1e-13);
    }

    #[test]
    fn issue_3090_a_corner_inside_half_the_tolerance_is_moved_to() {
        // 4e-4 off the x = 0 face: past XSectTol3d, within half of 1e-3
        let (k, mut b, v) = box_corner_off(4.0e-4, 1.0e-3);
        let xsect = vertex::corner::xsect_tol_3d_of_brep(&k, &b);
        assert!(xsect < 4.0e-4);
        let at = face_corner_to_move_to(&k, &mut b, v, xsect)
            .unwrap()
            .unwrap();
        assert!(at.distance(Vec3::ZERO) <= 1e-12, "{at:?}");
        // measuring leaves the vertex where it was
        assert_eq!(b.vertex(v).unwrap().point, Vec3::new(4.0e-4, 0.0, 0.0));
    }

    #[test]
    fn issue_3090_a_corner_past_half_the_tolerance_is_left() {
        // 6e-4 off the face, tolerance 1e-3: the corner is 6e-4 away, past half of it
        let (k, mut b, v) = box_corner_off(6.0e-4, 1.0e-3);
        let xsect = vertex::corner::xsect_tol_3d_of_brep(&k, &b);
        assert_eq!(face_corner_to_move_to(&k, &mut b, v, xsect).unwrap(), None);
    }

    #[test]
    fn issue_3090_a_vertex_within_the_intersection_tolerance_or_without_room_is_left() {
        let (k, mut b, v) = box_corner_off(1.0e-7, 1.0e-3);
        let xsect = vertex::corner::xsect_tol_3d_of_brep(&k, &b);
        assert_eq!(face_corner_to_move_to(&k, &mut b, v, xsect).unwrap(), None);
        // a tolerance not past XSectTol3d cannot hold the gap
        let (k, mut b, v) = box_corner_off(4.0e-4, xsect.min(1.0e-5));
        assert_eq!(face_corner_to_move_to(&k, &mut b, v, xsect).unwrap(), None);
    }

    /// The box corner's z = 0 face replaced by the plane x = -g with the x = 0 face's normal:
    /// two parallel faces `g` apart at the vertex, which stands at (0, 0, e) with `tolerance`
    /// while its edge curves still end at the origin.
    fn parallel_corner(g: f64, e: f64, tolerance: f64) -> (Kernel, Brep, VertexId) {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::math::Point2;
        use crate::nurbs::knots::Side;
        use crate::surface::{plane::Plane, surface::Surface};
        use crate::topology::{face::p2 as face, vertexuse};
        let (k, mut b, v) = box_corner_off(0.0, tolerance);
        let normal_of = |b: &Brep, f| {
            let s = face::surface(b, f).unwrap().unwrap();
            s.evaluate_normal(&k, Point2::new(0.0, 0.0), Side::Left, Side::Left)
                .unwrap()
        };
        let faces = vertex::faces(&b, v).unwrap();
        let along = |b: &Brep, axis: Vec3| {
            faces
                .iter()
                .copied()
                .find(|&f| normal_of(b, f).dot(axis).abs() > 0.5)
                .unwrap()
        };
        let (fx, fz) = (along(&b, Vec3::X), along(&b, Vec3::Z));
        // u along z and v along y: the normal is z x y = -x, the x = 0 face's, and a point
        // above the corner (0, 0, e) is at u = e, as it was at x = e on the replaced plane
        assert!(normal_of(&b, fx).distance(-Vec3::X) < 1e-12);
        let frame = Axis2Placement::new(Vec3::new(-g, 0.0, 0.0), Vec3::Z, Vec3::Y);
        b.face_mut(fz).unwrap().surface =
            Some(Surface::from(Plane::create_canonical(&frame).unwrap()));
        b.vertex_mut(v).unwrap().point = Vec3::new(0.0, 0.0, e);
        for vu in vertex::vertexuses(&b, v).unwrap() {
            vertexuse::init_vertex_face_gap(&mut b, vu).unwrap();
        }
        (k, b, v)
    }

    fn xsect_of(k: &Kernel, b: &Brep) -> f64 {
        vertex::corner::xsect_tol_3d_of_brep(k, b)
    }

    #[test]
    fn issue_3090_parallel_faces_a_corner_inside_the_bound_and_the_residual_is_moved_to() {
        let (k, b, _) = box_corner_off(0.0, 1.0);
        let g = 1.5 * xsect_of(&k, &b);
        // the midpoint of the faces: moves g / 2, leaves g / 2 (< XSectTol3d) to each face
        let (k, mut b, v) = parallel_corner(g, 0.0, 1.0e-3);
        let xsect = xsect_of(&k, &b);
        assert!(
            vertex::corner::has_parallel_faces(&k, &mut b, v, None)
                .unwrap()
                .parallel
        );
        let at = face_corner_to_move_to(&k, &mut b, v, xsect)
            .unwrap()
            .unwrap();
        assert!(at.distance(Vec3::new(-g / 2.0, 0.0, 0.0)) < 1e-12, "{at:?}");
    }

    #[test]
    fn issue_3090_parallel_faces_a_corner_past_half_the_tolerance_is_left() {
        let (k, b, _) = box_corner_off(0.0, 1.0);
        let xsect = xsect_of(&k, &b);
        let g = 1.5 * xsect;
        // tolerance 1.2 xsect: past XSectTol3d, but the move g / 2 = 0.75 xsect is past half of it
        // (the residual 0.75 xsect passes, so it is the bound that refuses)
        let (k, mut b, v) = parallel_corner(g, 0.0, 1.2 * xsect);
        assert!(
            vertex::corner::has_parallel_faces(&k, &mut b, v, None)
                .unwrap()
                .parallel
        );
        assert_eq!(face_corner_to_move_to(&k, &mut b, v, xsect).unwrap(), None);
    }

    #[test]
    fn issue_3090_parallel_faces_a_residual_past_the_intersection_tolerance_is_left() {
        let (k, b, _) = box_corner_off(0.0, 1.0);
        let xsect = xsect_of(&k, &b);
        // faces 3 xsect apart: the midpoint is 1.5 xsect from each, well inside half of 1e-3
        let (k, mut b, v) = parallel_corner(3.0 * xsect, 0.0, 1.0e-3);
        assert!(
            vertex::corner::has_parallel_faces(&k, &mut b, v, None)
                .unwrap()
                .parallel
        );
        assert_eq!(face_corner_to_move_to(&k, &mut b, v, xsect).unwrap(), None);
    }

    #[test]
    fn issue_3090_a_corner_far_from_the_end_of_an_incident_edge_is_left() {
        let (k, b, _) = box_corner_off(0.0, 1.0);
        let xsect = xsect_of(&k, &b);
        let g = 1.5 * xsect;
        // the vertex stands e along the faces from where its edge curves end; the corner is
        // g / 2 from it, inside half of 1e-3, and both face gaps pass, so only the edge gap
        // (about e from the y and x edges) decides: 4e-4 is inside half the tolerance, 6e-4 past it
        let (k, mut b, v) = parallel_corner(g, 4.0e-4, 1.0e-3);
        assert!(face_corner_to_move_to(&k, &mut b, v, xsect)
            .unwrap()
            .is_some());
        let (k, mut b, v) = parallel_corner(g, 6.0e-4, 1.0e-3);
        assert_eq!(face_corner_to_move_to(&k, &mut b, v, xsect).unwrap(), None);
    }
    #[test]
    fn cancellation_status_ignore_failure_preserves_control_errors() {
        for error in [
            SolidError::cancelled("cancelled-callee"),
            SolidError::unported("callee", "unit"),
            SolidError::FaceTrimsUnavailable {
                operation: "face-classification",
                entity: EntityRef::Face(FaceId::from_index(0)),
            },
        ] {
            assert_eq!(ignore_failure::<()>(Err(error.clone())), Err(error));
        }
    }

    #[test]
    fn cancellation_status_survives_ignored_merge_failure() {
        let cancelled = SolidError::cancelled("boolean-region-classification");
        assert_eq!(ignore_failure::<()>(Err(cancelled.clone())), Err(cancelled));
        assert_eq!(ignore_failure(Ok(7)), Ok(Some(7)));
        assert_eq!(
            ignore_failure::<()>(Err(SolidError::sm_err("ordinary-classification-failure"))),
            Ok(None)
        );
        let unported = SolidError::unported("callee", "unit");
        assert_eq!(ignore_failure::<()>(Err(unported.clone())), Err(unported));
    }

    #[test]
    fn audit_483_boxes_fail_before_temporary_translation() {
        let mut k = Kernel::default();
        let mut bad =
            crate::primitive::api_primitives::create_box(&mut k, Vec3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        let edge = bad.edges().ids().next().unwrap();
        bad.edge_mut(edge).unwrap().curve = Some(crate::curve::curve::Curve::from(
            crate::nurbs::b_spline_curve::BSplineCurve::default(),
        ));
        let mut good = Brep::new();
        good.set_locked_bounding_box(Some(crate::geom::extent3d::Extent3d::from_point(
            Vec3::new(2000.0, 0.0, 0.0),
        )));
        let expected =
            qbrep::calculate_bounding_box(&k, &bad, BoundingBoxOptions::default()).unwrap_err();
        for (first, second) in [(&bad, &good), (&good, &bad)] {
            assert_eq!(
                temporary_brep_transform(&k, &k, first, second).unwrap_err(),
                expected
            );
        }
    }

    #[test]
    fn temporary_translation_reports_restore_failure() {
        let token = TemporaryBrepTransform {
            inverse: Some(Axis2Placement::IDENTITY),
        };
        let error = SolidError::sm_err("failed-inverse-transform");
        let mut restored = Vec::new();
        let result = token.restore(
            Some(&mut Brep::new()),
            Some(&mut Brep::new()),
            |_, is_other, _, _, _| {
                restored.push(is_other);
                if is_other {
                    Ok(())
                } else {
                    Err(error.clone())
                }
            },
        );
        assert_eq!(restored, [false, true]);
        assert_eq!(result, Err(error));
    }

    #[test]
    fn checked_temporary_translation_stops_on_failure_and_undoes_the_first_operand() {
        for failed_operand in [false, true] {
            let k = Kernel::default();
            let mut a = Brep::new();
            let mut b = Brep::new();
            let vertex = a
                .add_vertex_record(crate::topology::Vertex {
                    point: Vec3::new(2001.0, 2.0, 3.0),
                    ..crate::topology::Vertex::default()
                })
                .unwrap();
            let original = a.vertex(vertex).unwrap().point;
            // The locked boxes isolate the scope from the topology walk.
            let bounds =
                crate::geom::extent3d::Extent3d::from_bounds(2000.0, 0.0, 0.0, 2010.0, 10.0, 10.0);
            a.set_locked_bounding_box(Some(bounds));
            b.set_locked_bounding_box(Some(bounds));
            let shift = temporary_brep_transform(&k, &k, &a, &b).unwrap();
            let error = SolidError::Upstream {
                operation: "failed-forward-transform",
                code: 1027,
                entity: None,
            };
            let mut calls = Vec::new();
            let result = shift.apply(&mut a, &mut b, |brep, is_other, xfm, scale, scale_tol| {
                assert_eq!(scale, None);
                assert!(scale_tol);
                calls.push(is_other);
                if is_other == failed_operand {
                    return Err(error.clone());
                }
                if !is_other {
                    let old = brep.vertex(vertex)?.point;
                    brep.vertex_mut(vertex)?.point = xfm.transform_point(old);
                }
                Ok(())
            });
            assert_eq!(result.unwrap_err(), error);
            assert_eq!(
                calls,
                if failed_operand {
                    vec![false, true, false]
                } else {
                    vec![false]
                }
            );
            assert_eq!(a.vertex(vertex).unwrap().point, original);
        }
    }

    #[test]
    fn checked_temporary_translation_reports_a_failed_undo() {
        let shift = TemporaryBrepShift {
            shift: Some(Vec3::X),
        };
        let mut calls = 0;
        let error = SolidError::sm_err("failed-undo");
        let result = shift.apply(&mut Brep::new(), &mut Brep::new(), |_, _, _, _, _| {
            calls += 1;
            match calls {
                1 => Ok(()),
                2 => Err(SolidError::sm_err("failed-other-translation")),
                _ => Err(error.clone()),
            }
        });
        assert_eq!(calls, 3);
        assert_eq!(result.unwrap_err(), error);
    }

    #[test]
    fn checked_temporary_cleanup_attempts_every_survivor_and_returns_the_first_error() {
        for deleted in [(false, false), (false, true), (true, false), (true, true)] {
            for body_result in [Ok(7), Err(SolidError::sm_err("failed-boolean"))] {
                let token = TemporaryBrepTransform {
                    inverse: Some(Axis2Placement::IDENTITY),
                };
                let mut a = Some(Brep::new());
                let mut b = Some(Brep::new());
                let mut restored = Vec::new();
                let first_error = SolidError::sm_err("primary-inverse");
                let second_error = SolidError::sm_err("other-inverse");
                let result = token.run(
                    &mut a,
                    &mut b,
                    &mut (),
                    |(), a, b| {
                        if deleted.0 {
                            *a = None;
                        }
                        if deleted.1 {
                            *b = None;
                        }
                        body_result.clone()
                    },
                    |(), _, is_other, _, _, _| {
                        restored.push(is_other);
                        Err(if is_other {
                            second_error.clone()
                        } else {
                            first_error.clone()
                        })
                    },
                );
                let expected: Vec<_> =
                    [(!deleted.0).then_some(false), (!deleted.1).then_some(true)]
                        .into_iter()
                        .flatten()
                        .collect();
                assert_eq!(restored, expected);
                assert_eq!(
                    result,
                    if !deleted.0 {
                        Err(first_error)
                    } else if !deleted.1 {
                        Err(second_error)
                    } else {
                        body_result
                    }
                );
            }
        }
    }

    #[test]
    fn checked_temporary_translation_round_trips_real_boxes() {
        use crate::primitive::api_primitives::create_box;
        use crate::topology::brep::p5::transform;

        let mut k = Kernel::default();
        let mut a = create_box(&mut k, Vec3::new(4000.0, 2000.0, 1000.0), 10.0, 8.0, 6.0).unwrap();
        let mut b = create_box(&mut k, Vec3::new(4004.0, 2002.0, 1000.0), 10.0, 8.0, 6.0).unwrap();
        let points = |brep: &Brep| {
            brep.vertices()
                .iter()
                .map(|(_, v)| v.point)
                .collect::<Vec<_>>()
        };
        let original_a = points(&a);
        let original_b = points(&b);
        let box_of = |k: &Kernel, b: &Brep| {
            qbrep::calculate_bounding_box(k, b, BoundingBoxOptions::default()).unwrap()
        };
        let bounds_a = box_of(&k, &a);
        let bounds_b = box_of(&k, &b);
        let token = temporary_brep_transform(&k, &k, &a, &b)
            .unwrap()
            .apply(&mut a, &mut b, |b, _, xfm, scale, scale_tol| {
                transform(&mut k, b, xfm, scale, scale_tol)
            })
            .unwrap();
        assert!(box_of(&k, &a).union(&box_of(&k, &b)).mid().length_squared() < 1.0);
        token
            .restore(Some(&mut a), Some(&mut b), |b, _, xfm, scale, scale_tol| {
                transform(&mut k, b, xfm, scale, scale_tol)
            })
            .unwrap();
        assert_eq!(points(&a), original_a);
        assert_eq!(points(&b), original_b);
        assert_eq!(box_of(&k, &a), bounds_a);
        assert_eq!(box_of(&k, &b), bounds_b);
    }

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
                        Ok(())
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
