//! B-rep editing, part 5: coincident-vertex targets and dependency-limited edits.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`coincident_vertex_target`], [`combine_coincident_vertices`] | `SmBrep::CombineCoincidentVertices` |
//! | [`create_wire_edge_from_curve`] | `SmBrep::CreateWireEdgeFromCurve` |
//! | [`delete_topological_vertex`] | `SmBrep::DeleteTopologicalVertex` |
//! | [`find_and_set_infinite_region`] | `SmBrep::FindAndSetInfiniteRegion` |
//! | [`glue_vertices`] | `SmBrep::GlueVertices` |
//! | [`make_edge_in_face`], [`make_edge_in_face_with_uv_curve`], [`make_edge_in_face_in`] | `SmBrep::MakeEdgeInFace` |
//! | [`make_manifold`] | `SmBrep::MakeManifold` |
//! | [`remove_topological_edges_and_vertices`] | `SmBrep::RemoveTopologicalEdgesAndVertices` |
//! | [`remove_faces`] | `SmBrep::RemoveFaces` |
//! | [`remove_faces_notified`] | `SmBrep::RemoveFaces` (caller's records) |
//! | [`stitch_and_orient`] | `SmBrep::StitchAndOrient` |
//! | [`squeeze_face`] | `SmBrep::SqueezeFace` |
//! | [`transform`] | `SmBrep::Transform(const SmAxis2Placement&, ...)` (the 4x4 matrix overload is not owned by this unit) |
//! | [`turn_to_nurbs_plain`] | `SmBrep::TurnToNURBS()` as a plain entry point (the edit is `topology.brep_p3`'s) |
//! | [`PendingTopology`] | `SmPendingTopologyDelete` constructor/destructor and `Clear` (co-port) |
//!
//! Deviations: dependency-limited editing. All twelve owned editing
//! control flows are present, including continuations after missing calls.
//! Individual missing callees remain explicit dependency deferrals. None of the
//! deferred mutation paths is certified by numerical-target-only cases.
//! Fresh-mark collection uses ordered local deduplication and still acquires and
//! releases the native context mark. Separate curve/surface slots are distinct
//! owned values; native shared geometry across different owners is not represented
//! and remains uncertified. Equal owner metadata is never used to invent aliasing.
//! Notification environments borrow caller records and keys. A plain entry point (no
//! environment; the `_with_notifications` forms given `None`) runs its whole
//! edit in one environment under the kernel's identities (`plain_edit`,
//! `topology::brep::p3::with_kernel_callbacks`: `b`'s serial in the kernel context's
//! domain, so two B-reps' objects never share a key, notifying through the kernel
//! context's callbacks lent to the edit); one called inside another lent edit is refused
//! before anything changes. `topology.brep_p3`'s edits (`SqueezeEdge`, `DeleteFace`,
//! `MakeWireEdgeVertex`, `OrientTrimmedSurfaces`, `TurnToNURBS`) and `MakeShellVertex`
//! (`topology.brep_p9`) notify through that environment, so [`make_manifold`] and
//! [`stitch_and_orient`] take the kernel. Curves have no identity in the port: `SetCurve`'s new/old curve
//! data are null and a curve's own `SM_NO_CHANGE_OWNER` is not delivered.
//! `SetOwner(pEdge)` on a joined or mirrored edge's curve owns it under the edge's
//! [`Brep::owner_identity`] (the identity the B-rep check compares) when `b` names its
//! geometry by a fixed scheme (`Notify`, `Arena`): a plain entry point's kernel keys
//! would otherwise leave the curve owned by no edge `ValidateGeometry` knows. Under a
//! caller's naming (`Caller`, `Unknown`) the environment's key owns it, recorded on `b`.
//! Not upstream (WR-607; SolidCraft change, tangency-labs/smlib 661f79c): [`glue_vertices`]
//! joins the pair of shells of every
//! region both vertices are in, not only the last pair its search finds.
//! `SmBrep::Notify` is owned by `topology.brep_p2`, not this unit. Owning-list
//! traversal (`GetRegions`, `GetFaces`, `GetEdges`, `GetVertices`) and the editing
//! flag are `topology.brep_p1`'s list-order collections and `Brep` member. Pointer inputs are optional typed ids;
//! out-parameters are mutable outputs where failure must clear them. The old-tol
//! target calculation is factored out for direct numerical-stage verification;
//! that verification does not certify vertex deletion or gluing. The internal
//! `PendingTopology` owns an allocation until release and borrows the actual
//! attribute store so failure detaches borrowed attributes without deleting them.
//! Its reader integration remains uncertified until `MakeTopologyFromData` lands.
//! Aliased `CombineCoincidentVertices` takes the native path in every case,
//! including an isolated vertex, which upstream deletes along with its survivor
//! before returning success.
//! Shell/region queries and transfers use the landed shell port. Stable shell,
//! vertex and vertex-use record erasure requires part 4's owning-storage lifecycle: the shared
//! arena is append-only, so shifting ids or leaving a visible dummy is invalid.
//! Debug drawing/assertions are omitted. No recursion is introduced.
//! `DeleteTopologicalVertex` leaves the vertex in place (the routine's silent `SM_ERR`)
//! for an edge curve that is not of an `SmBSplineCurve` kind, where upstream dereferences
//! the null cast (WR-598, the fork's tangency-labs/smlib@829b982), except a curve on a
//! B-spline-kind surface along an exactly constant-u or constant-v UV line (the fork took a
//! drift up to `EFF_ZERO` of the domain), which is joined as that iso curve; on a
//! procedural blend (which evaluates its definition, not its net) only while the net's
//! certified bound is within the joined edge's tolerance. A line joined through the `SmBSplineCurve` join gets its
//! NURB made first, as `GetOrCreateGwNurbPointer` does.
//! Not upstream (#15718): once [`remove_topological_edges_and_vertices`] has deleted its
//! topological vertices, each surviving edge on a line and its vertices grow to the
//! certified gaps the joins left between the line's ends and the vertices
//! (`cover_joined_line_gaps`; the boolean cleanup does the same).
//! Not upstream (#16866): after each face merge [`remove_topological_edges_and_vertices`]
//! completes the merged face's seam (`HasSeamProblem`, then `SplitAtSeam`:
//! [`crate::topology::face::p1::complete_merged_face_seam`]), as the boolean cleanup does
//! under WR-599: a merged face on a periodic surface keeps its seam whatever the fix set.
//! Not upstream (#18166): [`stitch_and_orient`] keeps each edge its stitch raised covering
//! the faces it holds no trim on across the orientation, which rewrites their surfaces
//! (`StitchRaises`). Not upstream (#20373): after the orientation each edge the stitch
//! joined covers its UV trims as the B-rep ends with them.

use crate::curve::curve::Curve;
use crate::error::topology::TopologyFailure;
use crate::error::{Failure, Result, SolidError, SM_ERR_NULL_POINTER};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::{MarkLock, ALL_MARKS};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::tol::ZONE_TOL_3D;
use crate::heal::stitch::p2::ClosedGap;
use crate::kernel::Kernel;
use crate::math::{max2, Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::BrepQuery;
use crate::topology::brep::p1::SetRegionVoidType;
use crate::topology::brep::p2 as brep_p2;
use crate::topology::face::p4 as face_p4;
use crate::topology::VertexuseId;
use crate::topology::{
    vertex, Brep, EdgeId, FaceId, FaceuseId, LoopId, RegionId, ShellId, VertexId,
};
use std::collections::BTreeMap;

/// A pending topology allocation with attributes borrowed from a database reader.
/// On failure/drop, detaches each attribute with native `bDoNotDelete = TRUE`
/// before dropping the object. [`Self::release`] transfers a successful allocation.
pub struct PendingTopology<'a, T> {
    object: Option<(crate::geom::attribute::ObjectKey, T)>,
    attributes: &'a mut crate::geom::attribute::AttributeStore,
}

impl<'a, T> PendingTopology<'a, T> {
    /// Guards a pending allocation, or the native null-pointer case (`None`).
    pub fn new(
        object: Option<(crate::geom::attribute::ObjectKey, T)>,
        attributes: &'a mut crate::geom::attribute::AttributeStore,
    ) -> Self {
        Self { object, attributes }
    }

    /// Accesses the pending object, when one exists.
    pub fn object_mut(&mut self) -> Option<&mut T> {
        self.object.as_mut().map(|(_, object)| object)
    }

    /// Transfers the allocation and its actual key without detaching attributes
    /// (`SmPendingTopologyDelete::Clear`).
    pub fn release(mut self) -> Option<(crate::geom::attribute::ObjectKey, T)> {
        self.object.take()
    }
}

impl<T> Drop for PendingTopology<'_, T> {
    fn drop(&mut self) {
        if let Some((key, object)) = self.object.take() {
            for attribute in self.attributes.attributes(key) {
                // Native ignores RemoveAttribute's status. The store remains
                // exclusively borrowed by the guard throughout this cleanup.
                let _ = self.attributes.remove_attribute(key, attribute, true);
            }
            drop(object);
        }
    }
}

const OP: &str = "topology::brep::p5";

/// The operation of the null-pointer error `MakeEdgeInFace` returns when
/// `SmFace::FindVertexSector` finds no sector for its start or end vertex
/// (`NER(pStartVU)` / `NER(pEndVU)`, `source/SMLib/src/SmBrep.cpp:13861`, `:13873`).
/// Both checks precede `MakeEdgeInFaceWithVU`, and `FindVertexSector` is const, so
/// this failure leaves the B-rep unedited ([`is_no_vertex_sector`]).
pub const NO_VERTEX_SECTOR_OP: &str = "topology::brep::p5::make_edge_in_face";

/// Whether `error` is `MakeEdgeInFace`'s no-sector `NER` ([`NO_VERTEX_SECTOR_OP`]):
/// a failure that edited nothing.
#[must_use]
pub fn is_no_vertex_sector(error: &SolidError) -> bool {
    // the status, not the variant: a typed failure that replaced the `Upstream` (#13790)
    error.upstream_status().is_some() && error.operation() == NO_VERTEX_SECTOR_OP
}

/// Runs `f` with the caller's notification environment, or, for a plain entry point
/// (`notifications` is `None`: the caller keeps no notification state of its own),
/// with an [`brep_p2::ArenaNotifyState`] of `b` (holding `b`'s own attributes for the
/// edit) notifying through `context`. `f`'s last
/// argument is the context the caller's own environment falls back to (`context`
/// then; `None` for the arena state, which already holds it).
pub(super) fn with_notifications<R>(
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    context: Option<&mut crate::geom::context::Context>,
    f: impl FnOnce(
        &mut Brep,
        &mut EditNotifyEnv<'_>,
        Option<&mut crate::geom::context::Context>,
    ) -> Result<R>,
) -> Result<R> {
    if let Some(env) = notifications {
        f(b, env, context)
    } else {
        // (not upstream) with `b`'s own attributes, given back afterwards
        let mut state = brep_p2::ArenaNotifyState::with_attributes_of(b);
        let r = f(b, &mut state.env(context), None);
        let restored = state.restore_attributes(b);
        let r = r?;
        restored?;
        Ok(r)
    }
}

/// Runs the `topology.brep_p3` edit `f` of `b` with the caller's notification
/// environment. Every entry point reaching one passes its environment (a plain entry
/// point's is [`plain_edit`]'s); without one the edit is a plain edit of its own.
fn with_p3_notifications<R>(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    f: impl FnOnce(&mut Kernel, &mut Brep, &mut EditNotifyEnv<'_>) -> Result<R>,
) -> Result<R> {
    match notifications {
        Some(env) => f(k, b, env),
        None => plain_edit(k, b, f),
    }
}

/// A plain entry point's edit (the caller keeps no notification state of its own):
/// `edit` runs with the kernel's identities for `b`'s objects
/// ([`crate::topology::brep::p3::with_kernel_callbacks`]: `b`'s serial in the kernel
/// context's identity domain, so two B-reps' objects never share a key), notifying
/// through the kernel context's callbacks, lent to the edit. One environment covers
/// the whole edit, so every object keeps one identity in it. A plain entry point
/// called inside another lent edit is refused before anything changes.
pub(super) fn plain_edit<R>(
    k: &mut Kernel,
    b: &mut Brep,
    edit: impl FnOnce(&mut Kernel, &mut Brep, &mut EditNotifyEnv<'_>) -> Result<R>,
) -> Result<R> {
    crate::topology::brep::p3::with_kernel_callbacks(k, b, edit)
}

fn require_editing(enabled: bool) -> Result<()> {
    if enabled {
        Ok(())
    } else {
        Err(SolidError::topology(OP, TopologyFailure::EditingDisabled))
    }
}

/// A named cause of this module's operation.
#[track_caller]
fn named(failure: TopologyFailure) -> SolidError {
    Failure::new(OP, failure).into_error()
}

/// A defect ([`TopologyFailure::Defect`]) with the status `SM_ERR` of the `SER` it
/// replaces.
#[track_caller]
fn defect(why: &'static str) -> SolidError {
    named(TopologyFailure::Defect { why })
}

/// A defect of the B-rep's own links ([`TopologyFailure::Defect`]), `why` saying which link
/// is missing, with the status `SM_ERR_NULL_POINTER` of the `NER` it replaces.
#[track_caller]
fn broken_link(why: &'static str) -> SolidError {
    Failure::new(OP, TopologyFailure::Defect { why })
        .with_status(SM_ERR_NULL_POINTER)
        .into_error()
}

/// `failure`, with the error a step before it ignored as its cause when there is one
/// (`docs/failure-codes-and-reproduction.md` §6, rule 6).
#[track_caller]
fn caused(failure: TopologyFailure, ignored: Option<SolidError>) -> SolidError {
    let f = Failure::new(OP, failure);
    match ignored {
        Some(e) => f.caused_by(e),
        None => f,
    }
    .into_error()
}

/// [`crate::error::ignored`], keeping the first error it ignores in `first` (to be the
/// cause of the failure that follows, [`caused`]).
fn ignored_keeping<T>(r: Result<T>, first: &mut Option<SolidError>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            crate::error::ignored(Err::<T, _>(e.clone()))?;
            first.get_or_insert(e);
            Ok(None)
        }
    }
}

/// Why a shell has no region.
const SHELL_NO_REGION: &str =
    "a shell has no region, though the kernel puts every shell of a B-rep in a region";
/// Why a vertex use has no vertex.
const VU_NO_V: &str =
    "a vertex use has no vertex, though the kernel makes every vertex use on its vertex";
/// Why the deleted edge has no far vertex.
const NO_FAR_VERTEX: &str = "the deleted edge has no vertex at its far end, though it is one of \
     the vertex's two edges and the kernel makes every edge with a vertex use at each end";
/// Why a face has no face use on a side.
const FACE_NO_FU: &str =
    "a face has no face use on one side, though the kernel makes every face with its two";
/// Why a face `surface_owners` listed has no surface.
const SURFACE_OWNER_NO_SURFACE: &str =
    "a face surface_owners listed has no surface, though it lists only faces with a surface";
/// Why a face use has no shell.
const FU_NO_SHELL: &str =
    "a face use has no shell, though the kernel puts every face use of a B-rep in a shell";

/// The position, tolerance and temporary-edge length threshold calculated before
/// `CombineCoincidentVertices` modifies topology (`SM_USE_OLDTOL`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoincidentVertexTarget {
    /// Position of the vertex with more incident edges; ties keep the survivor.
    pub point: Point3,
    /// The tolerance assigned after combining the vertices.
    pub tolerance: f64,
    /// `dMinDist`, used when deciding whether a common edge is short enough to squeeze.
    pub minimum_edge_length: f64,
}

/// Computes the initial old-tolerance stage of `CombineCoincidentVertices`.
/// Does not edit either vertex. Distances and the strict edge-count/tolerance
/// comparisons retain the native operation order.
pub fn coincident_vertex_target(
    b: &Brep,
    survivor: VertexId,
    deleted: VertexId,
) -> Result<CoincidentVertexTarget> {
    let edges = vertex::edges(b, survivor, None)?;
    let edges2 = vertex::edges(b, deleted, None)?;
    let s = b.vertex(survivor)?;
    let d = b.vertex(deleted)?;
    let mut point = s.point;
    let mut min_dist = point.distance(d.point);
    if min_dist > d.tolerance {
        let doubled = 2.0 * min_dist;
        min_dist = doubled;
    } else {
        min_dist += d.tolerance;
    }
    let mut tolerance = max2(s.tolerance, min_dist);
    if edges2.len() > edges.len() {
        min_dist = point.distance(d.point);
        if min_dist > s.tolerance {
            let doubled = 2.0 * min_dist;
            min_dist = doubled;
        } else {
            min_dist += s.tolerance;
        }
        point = d.point;
        tolerance = max2(d.tolerance, min_dist);
    }
    Ok(CoincidentVertexTarget {
        point,
        tolerance,
        minimum_edge_length: min_dist,
    })
}

/// Combines coincident vertices. Clears the deleted-edge output before reading
/// either vertex, including failure paths. The target stage is available, but
/// successful deletion and gluing remain dependency limited.
pub fn combine_coincident_vertices(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: VertexId,
    deleted: VertexId,
    deleted_edge: &mut Option<EdgeId>,
) -> Result<()> {
    combine_with_notifications(k, b, survivor, deleted, deleted_edge, None)
}

/// The existing topology notification environment: real records, attributes,
/// context and entity-to-object mapping, shared with edge editing.
pub use crate::topology::edge::NotifyEnv as EditNotifyEnv;

/// `CombineCoincidentVertices` with the actual notification environment needed
/// by `SmVertex::SetPoint`. The arena-only entry defers when it reaches this
/// environment boundary; supplying it never bypasses an unported native callee.
pub fn combine_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: VertexId,
    deleted: VertexId,
    deleted_edge: &mut Option<EdgeId>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    let Some(env) = notifications else {
        return plain_edit(k, b, |k, b, env| {
            combine_with_notifications(k, b, survivor, deleted, deleted_edge, Some(env))
        });
    };
    let mut notifications = Some(&mut *env);
    *deleted_edge = None;
    let target = coincident_vertex_target(b, survivor, deleted)?;
    let edges = vertex::edges(b, survivor, None)?;
    let edges2 = vertex::edges(b, deleted, None)?;
    if edges2.is_empty() {
        // Upstream ignores Notify's status here.
        crate::error::ignore_status(notify_merge(
            k,
            b,
            survivor,
            deleted,
            notifications.as_deref_mut(),
        ))?;
        set_vertex_point(k, b, survivor, target.point, notifications.as_deref_mut())?;
        crate::error::ignore_status(vertex::set_tolerance(
            b,
            survivor,
            target.tolerance,
            true,
            true,
        ))?;
        delete_vertex(k, b, deleted, notifications.as_deref_mut())?;
        return Ok(());
    } else if edges.is_empty() {
        let first_uses = vertex::vertexuses(b, survivor)?;
        let last_uses = vertex::vertexuses(b, deleted)?;
        for vu in first_uses {
            remove_vertexuse(b, survivor, vu)?;
            post_insert_vertexuse(b, deleted, vu)?;
        }
        for vu in last_uses {
            remove_vertexuse(b, deleted, vu)?;
            post_insert_vertexuse(b, survivor, vu)?;
        }
        // Upstream ignores Notify's status here.
        crate::error::ignore_status(notify_merge(
            k,
            b,
            survivor,
            deleted,
            notifications.as_deref_mut(),
        ))?;
        set_vertex_point(k, b, survivor, target.point, notifications.as_deref_mut())?;
        crate::error::ignore_status(vertex::set_tolerance(
            b,
            survivor,
            target.tolerance,
            true,
            true,
        ))?;
        delete_vertex(k, b, deleted, notifications.as_deref_mut())?;
        return Ok(());
    }
    let mut common = None;
    for e in edges {
        if crate::topology::edge::other_vertex(b, e, Some(survivor))? != Some(deleted) {
            continue;
        }
        let eus = &b.edge(e)?.edgeuses;
        let lamina = if let [head, _] = eus.as_slice() {
            crate::topology::edgeuse::p1::is_loop_edgeuse(b, *head)?
        } else {
            false
        };
        if crate::topology::edge::is_manifold(b, e)? || lamina {
            common = Some(e);
            break;
        }
    }
    if let Some(e) = common {
        let record = b.edge(e)?;
        let curve = record
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::invalid_input(OP, "common edge without curve"))?;
        let length = curve.length_with_zone_tol(
            k,
            &record.interval,
            target.tolerance,
            Some(record.tolerance),
        )?;
        if length > target.minimum_edge_length {
            // Retain a long common edge; GlueVertices will close it.
            common = None;
        } else {
            *deleted_edge = Some(e);
        }
        // A long common edge is retained as a closed edge by GlueVertices.
    } else {
        let faces = vertex::faces(b, survivor)?;
        let faces2 = vertex::faces(b, deleted)?;
        if let Some(face) = faces.into_iter().find(|f| faces2.contains(f)) {
            for vu in vertex::vertexuses(b, survivor)? {
                let faceuse = crate::topology::vertexuse::faceuse(b, vu)?;
                let owning_face = match faceuse {
                    Some(fu) => b.faceuse(fu)?.face,
                    None => None,
                };
                if owning_face == Some(face)
                    && matches!(
                        b.vertexuse(vu)?.owner,
                        crate::topology::VertexuseOwner::Loopuse(_)
                    )
                {
                    return Err(named(TopologyFailure::VertexLoopInSharedFace));
                }
            }
            let start = b.vertex(survivor)?.point;
            let mut end = b.vertex(deleted)?.point;
            if crate::geom::tol::in_tol(
                start.distance(end),
                crate::geom::tol::scaled_zero2(&start, &end),
            ) {
                end = start + Vec3::new(target.tolerance, target.tolerance, target.tolerance);
            }
            // Preserve failures from the temporary line before the edit metadata is
            // consulted.
            let line = BSplineCurve::create_line_segment(3, start, end)?;
            let interval = line.natural_interval();
            let editing = b.editing_enabled();
            let mut output = EdgeInFace::default();
            make_edge_in_face(
                k,
                b,
                editing,
                Some(face),
                Some(survivor),
                Some(deleted),
                Curve::from(line),
                None,
                interval,
                OrientType::Same,
                crate::math::EFF_ZERO,
                &mut output,
                notifications.as_deref_mut(),
            )?;
            common = output.edge;
        }
    }
    if let Some(edge) = common {
        squeeze_edge(k, b, notifications.as_deref_mut(), edge, survivor)?;
    } else {
        glue_with_notifications(
            k,
            b,
            Some(survivor),
            Some(deleted),
            None,
            notifications.as_deref_mut(),
        )?;
    }
    set_vertex_point(k, b, survivor, target.point, notifications)?;
    crate::error::ignore_status(vertex::set_tolerance(
        b,
        survivor,
        target.tolerance,
        true,
        true,
    ))?;
    crate::topology::brep::p1::update_vertex_edge_intervals(k, b, env, survivor)
}

/// `Notify(SM_NO_MERGE_IN_BREP, pSurvivingVertex, pVertexToDelete, this)`
/// ([`brep_p2::notify`], no B-rep cache held by the caller).
fn notify_merge(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: VertexId,
    deleted: VertexId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    notify_merge_with_body(k, b, survivor, deleted, notifications, None)
}

fn notify_merge_with_body(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: VertexId,
    deleted: VertexId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    body: Option<ObjectRef>,
) -> Result<()> {
    let data = [
        Some(brep_p2::NotifyArg::Entity(crate::error::EntityRef::Vertex(
            survivor,
        ))),
        Some(brep_p2::NotifyArg::Entity(crate::error::EntityRef::Vertex(
            deleted,
        ))),
        Some(brep_p2::NotifyArg::Object(
            body.unwrap_or_else(|| brep_p2::brep_ref(b)),
        )),
    ];
    with_notifications(b, notifications, Some(&mut k.context), |b, env, _| {
        brep_p2::notify_with_body_identity(
            b,
            env,
            None,
            NotifyOperation::MergeInBrep,
            data,
            body.unwrap_or_else(|| brep_p2::brep_ref(b)),
        )
    })
}

/// `SmBrep::DeleteVertex(vertex)` with the caller's notification records, attributes
/// and identities, or (without them) on call-local records
/// ([`crate::topology::brep::p4::with_local_env`]). The context is the kernel's.
fn delete_vertex(
    k: &mut Kernel,
    b: &mut Brep,
    vertex: VertexId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    use crate::topology::brep::p4;
    match notifications {
        Some(env) => {
            let mut d = p4::DeleteEnv::from_notify(env);
            p4::delete_vertex(k, b, &mut d, vertex)
        }
        None => p4::with_local_env(|d| p4::delete_vertex(k, b, d, vertex)),
    }
}

/// `SmBrep::DeleteEdge(edge, pOptSurfaceToKeep)`, environment as [`delete_vertex`].
fn delete_edge_with(
    k: &mut Kernel,
    b: &mut Brep,
    edge: EdgeId,
    surface_to_keep: Option<FaceId>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    use crate::topology::brep::p4;
    match notifications {
        Some(env) => {
            let mut d = p4::DeleteEnv::from_notify(env);
            p4::delete_edge(k, b, &mut d, edge, surface_to_keep)
        }
        None => p4::with_local_env(|d| p4::delete_edge(k, b, d, edge, surface_to_keep)),
    }
}

/// `SmBrep::SqueezeEdge(edge, survivor)` ([`crate::topology::brep::p3::squeeze_edge`]),
/// notifying as [`with_p3_notifications`].
fn squeeze_edge(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    edge: EdgeId,
    survivor: VertexId,
) -> Result<()> {
    with_p3_notifications(k, b, notifications, |k, b, env| {
        crate::topology::brep::p3::squeeze_edge(k, b, env, edge, survivor)
    })
}

fn remove_vertexuse(b: &mut Brep, vertex: VertexId, vu: VertexuseId) -> Result<()> {
    if b.vertexuse(vu)?.vertex != Some(vertex) {
        return Err(defect(
            "the vertex use to move is not on the vertex, though every caller took it from \
             that vertex's list or a shell of it",
        ));
    }
    let index = b
        .vertex(vertex)?
        .vertexuses
        .iter()
        .position(|&id| id == vu)
        .ok_or_else(|| {
            defect(
                "the vertex does not list a vertex use linked to it, though the kernel lists \
                 every vertex use on the vertex it links it to",
            )
        })?;
    b.vertex_mut(vertex)?.vertexuses.remove(index);
    b.vertexuse_mut(vu)?.vertex = None;
    Ok(())
}

fn post_insert_vertexuse(b: &mut Brep, vertex: VertexId, vu: VertexuseId) -> Result<()> {
    b.vertex(vertex)?;
    if b.vertexuse(vu)?.vertex.is_some() {
        return Err(defect(
            "the vertex use to move is still on a vertex, though every caller has just \
             removed it from its vertex",
        ));
    }
    b.vertex_mut(vertex)?.vertexuses.push(vu);
    b.vertexuse_mut(vu)?.vertex = Some(vertex);
    Ok(())
}

pub(super) fn set_vertex_point(
    k: &mut Kernel,
    b: &mut Brep,
    vertex: VertexId,
    point: Point3,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    with_notifications(
        b,
        notifications,
        Some(&mut k.context),
        |b, env, fallback| {
            // `SmVertex::SetPoint` returns void: its notification statuses are dropped.
            crate::error::ignore_status(vertex::set_point(
                b,
                vertex,
                point,
                env.records,
                env.attributes,
                env.context.as_deref_mut().or(fallback),
                (env.key_of)(crate::error::EntityRef::Vertex(vertex)),
                &|vu| (env.key_of)(crate::error::EntityRef::Vertexuse(vu)),
            ))
        },
    )
}

/// Makes a wire edge over the caller's curve interval. Evaluates start before end,
/// then needs the owning B-rep's infinite region; the owned curve drops on error.
pub fn create_wire_edge_from_curve(
    k: &mut Kernel,
    b: &mut Brep,
    curve: Curve,
    interval: Extent1d,
) -> Result<EdgeId> {
    plain_edit(k, b, |k, b, env| {
        create_wire_edge_from_curve_in(k, b, curve, interval, env)
    })
}

/// [`create_wire_edge_from_curve`] notifying through the caller's environment `env`
/// (a caller that keeps its own notification state, e.g. a copy into a store body).
///
/// # Errors
///
/// As [`create_wire_edge_from_curve`].
pub(crate) fn create_wire_edge_from_curve_notified(
    k: &mut Kernel,
    b: &mut Brep,
    curve: Curve,
    interval: Extent1d,
    env: &mut EditNotifyEnv<'_>,
) -> Result<EdgeId> {
    create_wire_edge_from_curve_in(k, b, curve, interval, env)
}

/// [`create_wire_edge_from_curve`] under the edit's notification environment ([`plain_edit`]).
fn create_wire_edge_from_curve_in(
    k: &mut Kernel,
    b: &mut Brep,
    curve: Curve,
    interval: Extent1d,
    env: &mut EditNotifyEnv<'_>,
) -> Result<EdgeId> {
    let start = curve.evaluate_point(k, interval.min())?;
    let end = curve.evaluate_point(k, interval.max())?;
    let region = b.infinite_region().ok_or_else(|| {
        Failure::new(OP, TopologyFailure::NoInfiniteRegion)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
    })?;
    let (first, edge, last) = with_p3_notifications(k, b, Some(&mut *env), |k, b, env| {
        let (_, first) = make_shell_vertex(b, region, start, env)?;
        let (edge, last) = make_wire_edge_vertex(
            k,
            b,
            env,
            region,
            first,
            curve,
            interval,
            OrientType::Same,
            end,
        )?;
        Ok((first, edge, last))
    })?;
    if start.distance(end) < b.tolerance() {
        glue_with_notifications(k, b, Some(first), Some(last), None, Some(&mut *env))?;
    }
    Ok(edge)
}

/// `SmBrep::MakeShellVertex(region, point)` ([`super::p9::make_shell_vertex`]).
fn make_shell_vertex(
    b: &mut Brep,
    r: RegionId,
    point: Point3,
    env: &mut EditNotifyEnv<'_>,
) -> Result<(ShellId, VertexId)> {
    super::p9::make_shell_vertex(b, Some(r), point, env)
}

/// `SmBrep::MakeWireEdgeVertex(region, first, curve, interval, orientation, end)`
/// ([`crate::topology::brep::p3::make_wire_edge_vertex`]).
#[allow(clippy::too_many_arguments)]
fn make_wire_edge_vertex(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut EditNotifyEnv<'_>,
    r: RegionId,
    first: VertexId,
    curve: Curve,
    interval: Extent1d,
    orientation: OrientType,
    end: Point3,
) -> Result<(EdgeId, VertexId)> {
    crate::topology::brep::p3::make_wire_edge_vertex(
        k,
        b,
        env,
        Some(r),
        first,
        curve,
        &interval,
        orientation,
        end,
    )
}

/// Deletes a degree-two topological vertex, optionally selecting the shorter edge.
/// The native editing-disabled refusal occurs before Notify and input checks.
pub fn delete_topological_vertex(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    vertex: Option<VertexId>,
    edge: Option<EdgeId>,
) -> Result<()> {
    delete_topological_vertex_with_notifications(k, b, editing_enabled, vertex, edge, None)
}

/// Deletes a topological vertex with the caller's real notification environment.
pub fn delete_topological_vertex_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    vertex: Option<VertexId>,
    edge: Option<EdgeId>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    delete_topological_vertex_joining(k, b, editing_enabled, vertex, edge, notifications)
        .map(|_| ())
}

/// [`delete_topological_vertex_with_notifications`], returning the surviving edge that now
/// runs over both edges' stretch.
pub(crate) fn delete_topological_vertex_joining(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    vertex: Option<VertexId>,
    edge: Option<EdgeId>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<EdgeId> {
    let Some(env) = notifications else {
        return plain_edit(k, b, |k, b, env| {
            delete_topological_vertex_joining(k, b, editing_enabled, vertex, edge, Some(env))
        });
    };
    require_editing(editing_enabled)?;
    crate::error::ignore_status(notify_pre_edit(k, b, Some(env)))?;
    let vertex = vertex.ok_or_else(|| named(TopologyFailure::NoVertexGiven))?;
    let join = prepare_vertex_join(k, b, vertex, edge)?;
    apply_vertex_join(k, b, vertex, join, env)
}

/// The surviving edge either extends its own curve or takes the joined curve.
struct VertexJoin {
    survivor: EdgeId,
    deleted: EdgeId,
    curve: Option<Curve>,
    interval: Extent1d,
}

fn prepare_vertex_join(
    k: &Kernel,
    b: &Brep,
    vertex: VertexId,
    edge: Option<EdgeId>,
) -> Result<VertexJoin> {
    let edges = vertex::edges(b, vertex, None)?;
    let [first, last] = edges.as_slice() else {
        return Err(named(TopologyFailure::VertexNotOnTwoEdges {
            edges: edges.len(),
        }));
    };
    let deleted = if let Some(edge) = edge {
        edge
    } else {
        let length1 = edge_curve(b, *first)?
            .approximate_length(k, &b.edge(*first)?.interval, 5, false)?
            .0;
        let length2 = edge_curve(b, *last)?
            .approximate_length(k, &b.edge(*last)?.interval, 5, false)?
            .0;
        if length2 < length1 {
            *last
        } else {
            *first
        }
    };
    if deleted != *first && deleted != *last {
        return Err(named(TopologyFailure::EdgeNotOnVertex));
    }
    let survivor = if *first == deleted { *last } else { *first };
    let join_survivor = crate::topology::edge::start_vertex(b, survivor)? != Some(vertex);
    let join_deleted = crate::topology::edge::start_vertex(b, deleted)? != Some(vertex);
    let max_gap = crate::math::max3(
        b.edge(survivor)?.tolerance,
        b.edge(deleted)?.tolerance,
        b.vertex(vertex)?.tolerance,
    );
    let survivor_interval = b.edge(survivor)?.interval;
    let deleted_interval = b.edge(deleted)?.interval;
    // Curves in separate arena records are distinct owned values. Equal owner
    // metadata does not prove native pointer identity. Shared native geometry
    // cannot currently be represented by these slots; do not guess aliasing from
    // that metadata. Coincidence classification still handles equal geometry.
    let mut same_geometry = false;
    if !same_geometry {
        let delete_curve = edge_curve_with_tolerance(b, deleted)?;
        let survive_curve = edge_curve_with_tolerance(b, survivor)?;
        let deleted_range = delete_curve.natural_interval();
        let survivor_range = survive_curve.natural_interval();
        // The native overload defaults bSnapToEnds to TRUE. Its additional-work
        // output is unused by this caller.
        let coincidence = delete_curve.global_coincidence_checker::<JoinCurve>(
            k,
            &deleted_range,
            &survive_curve,
            &survivor_range,
            0.001,
            true,
            [
                Some(JoinCurve::Deleted(deleted_range)),
                Some(JoinCurve::Survivor(survivor_range)),
            ],
        )?;
        if let [solution] = coincidence.solutions.as_slice() {
            if solution.solution_type == crate::geom::solution_array::SolutionType::RangeOfValues {
                // each end's parameters on the deleted and the surviving curve
                let [ds0, ss0, ..] = solution.start.parameters;
                let [ds1, ss1, ..] = solution.end.parameters;
                if (ds1 - ds0) * (ss1 - ss0) > 0.0 {
                    let far = if join_deleted {
                        deleted_interval.min()
                    } else {
                        deleted_interval.max()
                    };
                    if survivor_range.contains_value(far, crate::math::EFF_ZERO) {
                        let far_vertex =
                            crate::topology::edge::other_vertex(b, deleted, Some(vertex))?
                                .ok_or_else(|| broken_link(NO_FAR_VERTEX))?;
                        let point = b.vertex(far_vertex)?.point;
                        // Native ignores EvaluatePoint's status (SM_UNDEF_DOUBLE on
                        // failure); it is passed on here (WR-658).
                        let evaluated = survive_curve.evaluate_point(k, far)?;
                        if evaluated.closer_than(max_gap, point) {
                            same_geometry = true;
                        }
                    }
                }
            }
        }
    }
    let (curve, interval) = if same_geometry {
        let far = if join_survivor {
            deleted_interval.max()
        } else {
            deleted_interval.min()
        };
        let mut interval = survivor_interval;
        interval.add_value(far);
        (None, interval)
    } else {
        // Upstream casts each copy to `SmBSplineCurve` and calls `Trim` through the
        // result unchecked, so an edge curve of another kind (an `SmCrvOnSurf`) is a null
        // dereference (WR-598). Here such a curve is not joined: the routine's silent
        // error, with nothing changed yet (the fork's tangency-labs/smlib@829b982). A curve
        // on a B-spline surface along a constant-u or constant-v UV line is joined as that
        // iso curve. The joined edge is the survivor, with its tolerance.
        let joined_tolerance = b.edge(survivor)?.tolerance;
        let Some(mut deleted_copy) =
            exact_bspline_of_edge(k, b, deleted, deleted_interval, joined_tolerance)?
        else {
            return Err(named(TopologyFailure::EdgeCurveNotJoinable {
                entity: Some(crate::error::EntityRef::Edge(deleted)),
            }));
        };
        let Some(mut surviving_copy) =
            exact_bspline_of_edge(k, b, survivor, survivor_interval, joined_tolerance)?
        else {
            return Err(named(TopologyFailure::EdgeCurveNotJoinable {
                entity: Some(crate::error::EntityRef::Edge(survivor)),
            }));
        };
        let mut join_failure = None;
        if ignored_keeping(
            join_copied_curves(
                k,
                &mut surviving_copy,
                join_survivor,
                &mut deleted_copy,
                join_deleted,
                max_gap,
            ),
            &mut join_failure,
        )?
        .is_none()
        {
            return Err(caused(TopologyFailure::CurvesNotJoined, join_failure));
        }
        remove_join_knot(&mut surviving_copy, b.vertex(vertex)?.point, max_gap)?;
        if let Some(analytic) =
            surviving_copy.create_analytic_curve(k, &surviving_copy.natural_interval())?
        {
            surviving_copy = analytic;
        }
        let interval = surviving_copy.natural_interval();
        (Some(surviving_copy), interval)
    };
    Ok(VertexJoin {
        survivor,
        deleted,
        curve,
        interval,
    })
}

fn apply_vertex_join(
    k: &mut Kernel,
    b: &mut Brep,
    vertex: VertexId,
    join: VertexJoin,
    env: &mut EditNotifyEnv<'_>,
) -> Result<EdgeId> {
    let VertexJoin {
        survivor,
        deleted,
        curve,
        interval,
    } = join;
    if let Some(curve) = curve {
        set_joined_edge_curve(k, b, survivor, None, true, Some(env))?;
        set_joined_edge_curve(k, b, survivor, Some(curve), true, Some(env))?;
        crate::topology::edge::set_interval(b, survivor, &interval)?;
        set_edge_curve_owner(b, survivor, Some(env))?;
    } else {
        crate::topology::edge::set_interval(b, survivor, &interval)?;
    }
    for eu in crate::topology::edge::edgeuses(b, survivor, None, None)? {
        let eu = b.edgeuse_mut(eu)?;
        eu.uv_trim_curve = None;
        eu.drop_curve_fail = None;
    }
    crate::error::ignore_status(notify_edge_merge(k, b, survivor, deleted, Some(env)))?;
    let surviving_vertex = crate::topology::edge::other_vertex(b, deleted, Some(vertex))?
        .ok_or_else(|| broken_link(NO_FAR_VERTEX))?;
    squeeze_edge(k, b, Some(env), deleted, surviving_vertex)?;
    crate::error::ignore_status(notify_edge_pre_edit(k, b, survivor, Some(env)))?;
    if !crate::topology::edge::is_wire(b, survivor)? {
        for eu in crate::topology::edge::edgeuses(b, survivor, None, None)? {
            get_or_create_uv_trim_curve(k, b, eu)?;
        }
    }
    Ok(survivor)
}

/// A constructed rim can keep its join only if remaking its UV trims and covering the
/// final line both fit its existing tolerances. `false` rolls the entire rim edit back.
fn join_vertex_without_growth(
    k: &mut Kernel,
    b: &mut Brep,
    vertex: VertexId,
    env: &mut EditNotifyEnv<'_>,
) -> Result<bool> {
    let Some(join) = crate::error::ignored(prepare_vertex_join(k, b, vertex, None))? else {
        return Ok(true);
    };
    let budget = b.edge(join.survivor)?.tolerance;
    crate::error::ignore_status(notify_pre_edit(k, b, Some(env)))?;
    let e = apply_vertex_join(k, b, vertex, join, env)?;
    let edge = b.edge(e)?;
    // Dropping a trim may widen the edge (and its vertices) or change its interval.
    if !bound_within_tolerance(edge.tolerance, budget) {
        return Ok(false);
    }
    let Some(ends) = joined_line_ends(k, edge_curve(b, e)?, edge.interval)? else {
        return Ok(true);
    };
    let (Some(a), Some(z)) = (
        crate::topology::edge::start_vertex(b, e)?,
        crate::topology::edge::end_vertex(b, e)?,
    ) else {
        return Ok(false);
    };
    for v in [a, z] {
        let v = b.vertex(v)?;
        let (gap, needed) = joined_line_end_gaps(k, v.point, ends);
        if !(gap <= budget && needed <= v.tolerance) {
            return Ok(false);
        }
    }
    Ok(a != z)
}

fn joined_line_ends(k: &Kernel, curve: &Curve, interval: Extent1d) -> Result<Option<[Point3; 2]>> {
    if !matches!(curve.kind(), crate::curve::curve::CurveKind::Line(_)) {
        return Ok(None);
    }
    Ok(Some([
        curve.evaluate_point(k, interval.min())?,
        curve.evaluate_point(k, interval.max())?,
    ]))
}

/// The outward-rounded distance an edge must cover, and the vertex tolerance that covers it.
fn joined_line_end_gaps(k: &Kernel, point: Point3, ends: [Point3; 2]) -> (f64, f64) {
    let rounded = |q: Point3| {
        point.distance(q) + crate::nurbs::math::gamma(8) * (point.length() + q.length())
    };
    let gap = rounded(ends[0]).min(rounded(ends[1]));
    (gap, crate::geom::tol::zone_tol3d(k, 2.0 * gap))
}

/// Not upstream (#15718): an edge that took over a deleted topological vertex's two edges
/// (`DeleteTopologicalVertex`), when its curve is a line, covers the gaps the join left.
/// Both joins accept a far vertex within the largest of the three tolerances: the extension
/// of the survivor's curve to it, and the join of the two curves' copies, which
/// `CreateAnalyticCurve` may read back as one line through neither copy's far end exactly.
/// Its tolerance grows to the distance from each of its vertices to the nearer end of its
/// curve, and each vertex's tolerance to twice its gap (`AssertValid` checks half of it).
/// Each gap is a distance between two computed points plus its rounding
/// (`docs/validity-and-tolerances.md` §8, a construction's bound). An edge within its
/// tolerances, or no longer in the B-rep, is left as it is. The cleanups run it once their
/// deletions are done, so a grown tolerance never lets a later join accept a farther vertex.
///
/// # Errors
///
/// [`TopologyFailure::JoinedVertexReaches`](crate::error::topology::TopologyFailure::JoinedVertexReaches)
/// at the vertex when a grown vertex tolerance would reach the far vertex of one of its
/// edges (a tube may not reach an entity it does not bound), that vertex taken at its own
/// grown tolerance when it grows too, naming the tolerance needed and the one that fits;
/// nothing is grown then; those of the topology walks and the curve evaluations.
pub(crate) fn cover_joined_line_gaps(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<()> {
    use crate::boolean::topology_intersector::p1::{first_reaching, Growth};
    use crate::nurbs::math::gamma;
    if !b.edge_list().contains(&e) {
        return Ok(());
    }
    let ed = b.edge(e)?;
    let Some(curve) = ed.curve.as_ref() else {
        return Ok(());
    };
    let Some(ends) = joined_line_ends(k, curve, ed.interval)? else {
        return Ok(());
    };
    let edge_tolerance = ed.tolerance;
    let (Some(start), Some(end)) = (
        crate::topology::edge::start_vertex(b, e)?,
        crate::topology::edge::end_vertex(b, e)?,
    ) else {
        return Ok(());
    };
    if start == end {
        return Ok(());
    }
    let mut grow = Vec::new();
    let mut edge_gap: f64 = 0.0;
    for v in [start, end] {
        let point = b.vertex(v)?.point;
        let (gap, tolerance) = joined_line_end_gaps(k, point, ends);
        edge_gap = edge_gap.max(gap);
        if tolerance > b.vertex(v)?.tolerance {
            grow.push((v, point, tolerance));
        }
    }
    let gapped = edge_gap > edge_tolerance;
    if !gapped && grow.is_empty() {
        return Ok(());
    }
    let mut growths = Vec::new();
    for &(v, point, tolerance) in &grow {
        let mut reach = Vec::new();
        for other in vertex::edges(b, v, None)? {
            let far = crate::topology::edge::other_vertex(b, other, Some(v))?;
            if let Some(far) = far.filter(|&f| f != v) {
                let f = b.vertex(far)?;
                // the distance less its rounding: a lower bound
                let apart =
                    point.distance(f.point) - gamma(8) * (point.length() + f.point.length());
                reach.push((Some(far), apart, f.tolerance));
            }
        }
        growths.push(Growth {
            vertex: v,
            tolerance,
            reach,
        });
    }
    // both ends may grow: each is judged against the other's grown tolerance, and both
    // apply or neither (a NaN distance passes no test and refuses)
    if let Some((v, needed, fits_below)) = first_reaching(&growths) {
        return Err(SolidError::topology_at(
            OP,
            TopologyFailure::JoinedVertexReaches { needed, fits_below },
            crate::EntityRef::Vertex(v),
        ));
    }
    crate::topology::edge::set_tolerance(b, e, edge_gap, true, false)?;
    for (v, _, tolerance) in grow {
        vertex::set_tolerance(b, v, tolerance, true, true)?;
    }
    Ok(())
}

/// The edge's curve as a B-spline over the edge's interval, running the curve's way, to
/// join it with its neighbour (the fork's `sm_ExactBSplineOfEdge`, tangency-labs/smlib@829b982):
/// a curve of an `SmBSplineCurve` kind (lines and conics included) copied and trimmed, as
/// upstream does. A curve on a B-spline-kind surface whose UV curve is a line of exactly
/// constant u or v (the seam an extrusion leaves on its wall) is that surface's iso
/// curve over the stretch, which is the same curve exactly (WR-598); on a procedural
/// blend, which evaluates its definition rather than its net, only while the net's
/// certified bound is within `tolerance`, the joined edge's. Any other curve is `None`;
/// upstream dereferences the null cast instead. Import seam completion also uses this
/// extraction at tolerance zero, then maps the iso's parameter back onto its edge interval.
pub(crate) fn exact_bspline_of_edge(
    k: &Kernel,
    b: &Brep,
    edge: EdgeId,
    interval: Extent1d,
    tolerance: f64,
) -> Result<Option<Curve>> {
    use crate::curve::curve::p2::{BSPLINE_CURVE_TYPE, LINE_TYPE};
    use crate::curve::curve::CurveKind;
    let curve = edge_curve(b, edge)?;
    if curve.is_kind_of(BSPLINE_CURVE_TYPE) {
        let mut copy = curve.copy(k)?;
        let mut interval = interval;
        // may snap the interval by tol to existing knots
        copy.trim(
            k,
            &mut interval,
            crate::nurbs::b_spline_curve_n_lib::TrimMode::Notify,
        )?;
        return Ok(Some(copy));
    }
    let CurveKind::CrvOnSurf(cos) = curve.kind() else {
        return Ok(None);
    };
    let uv_curve = cos.uv_curve();
    if !uv_curve.is_kind_of(LINE_TYPE) {
        return Ok(None);
    }
    let surface = cos.surface();
    if !surface.is_kind_of(crate::surface::surface::p2::B_SPLINE_SURFACE_TYPE)? {
        return Ok(None);
    }
    // The iso curve is cut from the surface's net. A curve on a surface evaluates what the
    // surface evaluates: a procedural blend its definition S (`Surface::evaluate` reads the
    // net only inside a `NetScope`), every other B-spline kind its net, a screw sweep's
    // wall and a blend face kept beside a B-spline net included (`SurfaceKind::BSpline`).
    // On a net the iso curve is the edge's curve exactly. On a procedural blend it lies
    // within the net's certified bound `|B - S| <= bound` of the edge, so it is joined only
    // while that bound is within the joined edge's tolerance; the tolerance is never
    // widened to admit it.
    if let crate::surface::surface::SurfaceKind::ProceduralBlend(blend) = surface.kind() {
        if !bound_within_tolerance(blend.def.bound, tolerance) {
            return Ok(None);
        }
    }
    let a = uv_curve.evaluate_point(k, interval.min())?;
    let bb = uv_curve.evaluate_point(k, interval.max())?;
    // Only an exactly constant coordinate makes the iso curve the edge's curve: a UV
    // line that drifts by du moves up to |dS/du| du in space, which nothing here bounds
    // (the fork accepted a drift of EFF_ZERO times the domain's length).
    let (iso, iso_value, from, to) = if a.x == bb.x {
        (
            crate::nurbs::b_spline_surface::SurfParamType::U,
            a.x,
            a.y,
            bb.y,
        )
    } else if a.y == bb.y {
        (
            crate::nurbs::b_spline_surface::SurfParamType::V,
            a.y,
            a.x,
            bb.x,
        )
    } else {
        return Ok(None);
    };
    let trim = if iso == crate::nurbs::b_spline_surface::SurfParamType::U {
        crate::geom::extent2d::Extent2d::from_bounds(
            iso_value,
            crate::math::min2(from, to),
            iso_value,
            max2(from, to),
        )
    } else {
        crate::geom::extent2d::Extent2d::from_bounds(
            crate::math::min2(from, to),
            iso_value,
            max2(from, to),
            iso_value,
        )
    };
    let mut iso_curve = surface
        .create_iso_parametric_curve(k, iso, iso_value, 0.0, Some(&trim), false)?
        .curve;
    if to < from {
        let natural = iso_curve.natural_interval();
        iso_curve.reverse_parameterization(k, &natural)?;
    }
    Ok(Some(iso_curve))
}

/// Whether a net's certified distance `bound` from its definition is within `tolerance`
/// (WR-598's join on a procedural blend). Both are compared as they are: the bound is
/// already an outward-rounded enclosure, and `<=` on two finite doubles is exact. A NaN
/// on either side is not within.
fn bound_within_tolerance(bound: f64, tolerance: f64) -> bool {
    bound
        .partial_cmp(&tolerance)
        .is_some_and(std::cmp::Ordering::is_le)
}

fn edge_curve(b: &Brep, edge: EdgeId) -> Result<&Curve> {
    b.edge(edge)?
        .curve
        .as_ref()
        .ok_or_else(|| named(TopologyFailure::EdgeWithoutCurve))
}

/// An evaluation copy of the edge's curve carrying the edge's tolerance
/// ([`Curve::set_owner_zone_tol_3d`]), so the caches its solves build use it as
/// upstream's over the edge-owned curve (`SmTol::GetApproxTol3d(&m_crCurve)`).
fn edge_curve_with_tolerance(b: &Brep, edge: EdgeId) -> Result<Curve> {
    let mut curve = edge_curve(b, edge)?.clone_for_evaluation()?;
    curve.set_owner_zone_tol_3d(Some(b.edge(edge)?.tolerance));
    Ok(curve)
}

/// The two curves of `DeleteTopologicalVertex`'s coincidence check as its solutions
/// name them (upstream's `pDeleteCurve`, `pSurviveCurve`), with the natural interval
/// `IdenticalSolutions` reads through them (`SmCurve::GetNaturalInterval`).
#[derive(Clone, Copy, PartialEq)]
enum JoinCurve {
    Deleted(Extent1d),
    Survivor(Extent1d),
}
impl crate::geom::solution_array::SolutionObject for JoinCurve {
    fn kind(&self) -> crate::geom::solution_array::ObjectKind {
        crate::geom::solution_array::ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Self::Deleted(range) | Self::Survivor(range) => Some(*range),
        }
    }
}
fn join_copied_curves(
    k: &Kernel,
    survivor: &mut Curve,
    survivor_end: bool,
    deleted: &mut Curve,
    deleted_end: bool,
    gap: f64,
) -> Result<()> {
    use crate::curve::curve::CurveKind;
    let owner = survivor.owner();
    match survivor.kind_mut() {
        CurveKind::Line(line) => line.join_with(
            u64::from(survivor_end),
            deleted,
            u64::from(deleted_end),
            Some(gap),
        ),
        CurveKind::Ellipse(ellipse) => ellipse.join_with(
            k,
            u64::from(survivor_end),
            deleted,
            u64::from(deleted_end),
            Some(gap),
            owner,
        ),
        _ => bspline_curve_base_mut(survivor)?.join_with(
            k,
            survivor_end,
            bspline_curve_base_mut(deleted)?,
            deleted_end,
            Some(gap),
        ),
    }
}
/// Not upstream: merges the two Bezier spans a join of two B-spline curves left at the
/// deleted vertex `at` into one when they are pieces of one polynomial (or rational)
/// span, so the joined curve has no knot there.
///
/// `CreateByJoining` keeps the junction as a knot of full multiplicity. Where the two
/// edges were pieces of one curve (an operand edge split by the imprint, the piece past
/// the split merged back once the split vertex is redundant) that knot is removable: the
/// joined curve is the original one. Left in, it bounds a span as short as the deleted
/// edge, which can be shorter than the tolerance: its control points then coincide
/// (`SmBSplineCurve` test 15) and the UV trim curves made from it fail the polar
/// conversion test, so the merged body is not valid.
///
/// Knot removal (Tiller 1992; The NURBS Book, algorithm A5.8) divides by the short
/// span's length and cannot tell a removable knot from a corner when that span is
/// 1e-7 of the curve. The merge here extrapolates the longer span over the shorter one
/// instead (de Casteljau at a parameter just outside the span, well conditioned since
/// the step is small), and keeps it only when the shorter span is its piece: the same
/// weights to 1e-9 and control points within a thousandth of `max_gap`. Only spans that
/// are whole Bezier segments are merged (the knots at their far ends have full
/// multiplicity, as the canonical arcs and the intersection curves have them); a
/// junction that is a real corner keeps its knot.
fn remove_join_knot(curve: &mut Curve, at: Point3, max_gap: f64) -> Result<()> {
    use crate::curve::curve::CurveKind;
    let CurveKind::BSpline(base) = curve.kind_mut() else {
        return Ok(());
    };
    let Some(nurb) = base.gw_nurb() else {
        return Ok(());
    };
    let p = nurb.degree();
    let knots = nurb.knots();
    // the interior knot of full multiplicity nearest `at`, within `max_gap` of it
    let mut junction: Option<(usize, f64)> = None;
    let mut first = p + 1;
    while first + p < knots.len() {
        let u = knots.get(first).copied().unwrap_or(f64::NAN);
        let run = knots.iter().skip(first).take_while(|&&x| x == u).count();
        if run == p && first + p + 1 < knots.len() {
            let distance = base.evaluate_point(u)?.distance(at);
            if distance <= max_gap && junction.is_none_or(|(_, d)| distance < d) {
                junction = Some((first, distance));
            }
        }
        first += run.max(1);
    }
    let Some((first, _)) = junction else {
        return Ok(());
    };
    if let Some(merged) = merge_bezier_spans(nurb, first, 1e-3 * max_gap)? {
        base.set_from_gw_nurb(Some(&merged))?;
    }
    Ok(())
}

/// The curve with the two Bezier spans meeting at the knot of multiplicity `p` that
/// starts at index `first` merged into one ([`remove_join_knot`]), or `None` when they
/// are not one span to `tol` or are not whole Bezier segments.
fn merge_bezier_spans(
    nurb: &crate::nurbs::crv_basic::Curve,
    first: usize,
    tol: f64,
) -> Result<Option<crate::nurbs::crv_basic::Curve>> {
    use crate::nurbs::math::{CPoint, NOW};
    let p = nurb.degree();
    let knots = nurb.knots();
    let pw = nurb.cpts();
    // the junction's control point, the first of the left segment's and the knot run
    // after the junction's
    let (Some(junction), Some(hi)) = (first.checked_sub(1), first.checked_add(2 * p)) else {
        return Ok(None);
    };
    let Some(left_start) = junction.checked_sub(p) else {
        return Ok(None);
    };
    let (Some(&u), Some(&ul), Some(&ur)) =
        (knots.get(first), knots.get(junction), knots.get(first + p))
    else {
        return Ok(None);
    };
    // whole Bezier segments: the knots before and after the junction's run are runs of
    // at least the degree
    let full = |range: std::ops::Range<usize>, value: f64| {
        knots
            .get(range)
            .is_some_and(|run| run.iter().all(|&x| x == value))
    };
    if !full(left_start + 1..first, ul) || !full(first + p..hi, ur) {
        return Ok(None);
    }
    let (left_len, right_len) = (u - ul, ur - u);
    if left_len.is_nan() || right_len.is_nan() || left_len <= 0.0 || right_len <= 0.0 {
        return Ok(None);
    }
    let (Some(left), Some(right)) = (
        pw.get(left_start..=junction),
        pw.get(junction..=junction + p),
    ) else {
        return Ok(None);
    };
    // de Casteljau's triangle of `seg` at `t`: the polygons of [0, t] and [t, 1]
    let split = |seg: &[CPoint], t: f64| {
        let mut level = seg.to_vec();
        let mut before = Vec::new();
        let mut after = Vec::new();
        while let (Some(&head), Some(&tail)) = (level.first(), level.last()) {
            before.push(head);
            after.push(tail);
            level = level
                .windows(2)
                .filter_map(|w| match w {
                    [a, b] => Some(CPoint::combine2(1.0 - t, *a, t, *b)),
                    _ => None,
                })
                .collect();
        }
        after.reverse();
        (before, after)
    };
    let s = left_len / (left_len + right_len);
    let (merged, short_again, short) = if right_len >= left_len {
        // the right span run back over the left one: its polygon on [t0, 1]
        let (_, merged) = split(right, -left_len / right_len);
        let (piece, _) = split(&merged, s);
        (merged, piece, left)
    } else {
        let (merged, _) = split(left, (left_len + right_len) / left_len);
        let (_, piece) = split(&merged, s);
        (merged, piece, right)
    };
    if merged.len() != p + 1 || short_again.len() != short.len() {
        return Ok(None);
    }
    for (a, b) in short_again.iter().zip(short) {
        let distance = a.to_euclid().distance(b.to_euclid());
        let weights = if a.w == NOW || b.w == NOW {
            0.0
        } else {
            (a.w / b.w - 1.0).abs()
        };
        if distance.is_nan() || distance > tol || weights.is_nan() || weights > 1e-9 {
            return Ok(None);
        }
    }
    // the merged polygon, its ends the neighbours' points as they are
    let mut cpts = Vec::new();
    cpts.extend_from_slice(pw.get(..=left_start).unwrap_or(&[]));
    cpts.extend(merged.iter().skip(1).take(p.saturating_sub(1)).copied());
    cpts.extend_from_slice(pw.get(junction + p..).unwrap_or(&[]));
    let mut new_knots = Vec::new();
    new_knots.extend_from_slice(knots.get(..first).unwrap_or(&[]));
    new_knots.extend_from_slice(knots.get(first + p..).unwrap_or(&[]));
    Ok(Some(crate::nurbs::crv_basic::Curve::new(
        cpts, p, new_knots,
    )?))
}

/// The `SmBSplineCurve` part of a curve of that kind. A line's NURB is made first when it
/// has none, as upstream's `SmBSplineCurve` methods make it (`GetOrCreateGwNurbPointer`).
fn bspline_curve_base_mut(curve: &mut Curve) -> Result<&mut BSplineCurve> {
    use crate::curve::curve::CurveKind;
    let owner = curve.owner();
    match curve.kind_mut() {
        CurveKind::BSpline(base) => Ok(base),
        CurveKind::Line(line) => {
            line.ensure_nurb(owner)?;
            Ok(line.base_mut())
        }
        CurveKind::Ellipse(ellipse) => Ok(ellipse.base_mut()),
        CurveKind::Hyperbola(hyperbola) => Ok(hyperbola.base_mut()),
        CurveKind::Parabola(parabola) => Ok(parabola.base_mut()),
        _ => Err(broken_link(
            "a curve the join takes is of no B-spline kind, though both are copies that \
             exact_bspline_of_edge made of B-spline kinds",
        )),
    }
}
/// `pSurvivingEdge->SetCurve(pCurve, TRUE)` (status ignored upstream): notified, the
/// old curve deleted with `delete_old`. Curves carry no identity in the port (the
/// module's Deviations), so the notification's new/old curve data are null.
pub(super) fn set_joined_edge_curve(
    k: &mut Kernel,
    b: &mut Brep,
    edge: EdgeId,
    curve: Option<Curve>,
    delete_old: bool,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    with_notifications(
        b,
        notifications,
        Some(&mut k.context),
        |b, env, fallback| {
            let mut forwarded = crate::topology::edge::NotifyEnv {
                records: env.records,
                attributes: env.attributes,
                context: env.context.as_deref_mut().or(fallback),
                key_of: env.key_of,
            };
            let options = crate::topology::edge::SetCurveOptions {
                delete_old_curve: delete_old,
                ..crate::topology::edge::SetCurveOptions::default()
            };
            crate::error::ignore_status(crate::topology::edge::set_curve(
                b,
                edge,
                curve,
                options,
                &mut forwarded,
                [None, None],
            ))
        },
    )
}
/// `pCurveSurviveCopy->SetOwner(pSurvivingEdge)`: the owner is the edge, named by `b`'s
/// own scheme ([`Brep::owner_identity`]) when that is fixed (`Notify`, `Arena`), else by
/// its identity in the notification environment. The curve's own `SM_NO_CHANGE_OWNER`
/// notification (`SmObject::Notify` on the curve) is not delivered: the curve has no
/// identity to notify under (the module's Deviations; as `topology.brep_p2`'s `SetCurve`
/// helpers).
pub(super) fn set_edge_curve_owner(
    b: &mut Brep,
    edge: EdgeId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    let brep = ObjectRef {
        type_code: crate::geom::a_object::BREP_TYPE,
        key: b.key(),
    };
    with_notifications(b, notifications, None, |b, env, _| {
        let entity = crate::error::EntityRef::Edge(edge);
        let owner = match b.naming() {
            crate::topology::NamingScheme::Notify | crate::topology::NamingScheme::Arena => {
                b.owner_identity(entity)
            }
            crate::topology::NamingScheme::Caller | crate::topology::NamingScheme::Unknown => {
                (env.key_of)(entity)
            }
        };
        let curve = b.edge_mut(edge)?.curve.as_mut().ok_or_else(|| {
            broken_link(
                "the edge has no curve to own, though both callers have just set one on it \
                 (set_joined_edge_curve with a curve)",
            )
        })?;
        let _ = curve.set_owner(Some(owner), Some(brep));
        b.set_geometry_owner_key(crate::error::EntityRef::Edge(edge), Some(owner));
        Ok(())
    })
}

/// `Notify(SM_NO_MERGE_IN_BREP, pSurvivingEdge, pEdgeToDelete, this)`
/// ([`brep_p2::notify`], no B-rep cache held by the caller).
fn notify_edge_merge(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: EdgeId,
    deleted: EdgeId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    let data = [
        Some(brep_p2::NotifyArg::Entity(crate::error::EntityRef::Edge(
            survivor,
        ))),
        Some(brep_p2::NotifyArg::Entity(crate::error::EntityRef::Edge(
            deleted,
        ))),
        Some(brep_p2::NotifyArg::Object(brep_p2::brep_ref(b))),
    ];
    with_notifications(b, notifications, Some(&mut k.context), |b, env, _| {
        brep_p2::notify(b, env, None, NotifyOperation::MergeInBrep, data)
    })
}
/// `SER(pEU->GetOrCreateUVTrimCurve(pNewCurve))` with the declaration's defaults:
/// no distance or deviation outputs, `bOKToMakeUVTrimCurve = TRUE`.
fn get_or_create_uv_trim_curve(
    k: &Kernel,
    b: &mut Brep,
    eu: crate::topology::EdgeuseId,
) -> Result<()> {
    crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?.status
}

/// Finds the infinite region (part 9's `FindInfiniteRegion`, whose ray fires use
/// `query`: the B-rep's identities and cache), then makes it the infinite region and
/// sets the void flags as `void_setting` asks (part 1's `ResetInfiniteRegion`). A
/// face-use shell to test always swaps the flags (`SM_VS_INFINITEREGION_SWAP`), as
/// upstream.
///
/// # Errors
///
/// Those of both callees; [`TopologyFailure::InfiniteRegionNotFound`] with upstream's
/// null-pointer status when no region is found (upstream's `NER`).
pub fn find_and_set_infinite_region(
    k: &mut Kernel,
    b: &mut Brep,
    query: &BrepQuery<'_>,
    shell: Option<ShellId>,
    mut void_setting: SetRegionVoidType,
    bbox: Option<&Extent3d>,
) -> Result<()> {
    let region = crate::topology::brep::p9::find_infinite_region(k, b, query, shell, bbox)?
        .ok_or_else(|| named(TopologyFailure::InfiniteRegionNotFound))?;
    if let Some(shell) = shell {
        if shell_is_faceuse_shell(b, shell)? {
            void_setting = SetRegionVoidType::InfiniteRegionSwap;
        }
    }
    crate::topology::brep::p1::reset_infinite_region(k, b, Some(region), void_setting)
}

fn shell_is_faceuse_shell(b: &Brep, shell: ShellId) -> Result<bool> {
    Ok(b.shell(shell)?.is_faceuse_shell())
}

/// Glues vertices, leaving the survivor in place. `wire_edges` is the optional
/// caller-supplied wire list, rather than an empty-list substitute for no list.
pub fn glue_vertices(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: Option<VertexId>,
    deleted: Option<VertexId>,
    wire_edges: Option<&[EdgeId]>,
) -> Result<()> {
    glue_with_notifications(k, b, survivor, deleted, wire_edges, None)
}

/// Glues vertices with the caller's real notification records and attributes.
/// Equal identities, including two null identities, succeed before notification.
pub fn glue_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: Option<VertexId>,
    deleted: Option<VertexId>,
    wire_edges: Option<&[EdgeId]>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    glue_with_notification_identity(k, b, survivor, deleted, wire_edges, notifications, None)
}

/// Glue vertices with the caller's issued body identity, without changing `Brep::key`.
/// Both the body caller and body-valued notification payloads use `body`.
///
/// # Errors
/// The same ordered topology and notification errors as [`glue_with_notifications`].
pub fn glue_with_body_identity(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: Option<VertexId>,
    deleted: Option<VertexId>,
    wire_edges: Option<&[EdgeId]>,
    notifications: &mut EditNotifyEnv<'_>,
    body: ObjectRef,
) -> Result<()> {
    glue_with_notification_identity(
        k,
        b,
        survivor,
        deleted,
        wire_edges,
        Some(notifications),
        Some(body),
    )
}

fn glue_with_notification_identity(
    k: &mut Kernel,
    b: &mut Brep,
    survivor: Option<VertexId>,
    deleted: Option<VertexId>,
    wire_edges: Option<&[EdgeId]>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    body: Option<ObjectRef>,
) -> Result<()> {
    let Some(env) = notifications else {
        return plain_edit(k, b, |k, b, env| {
            glue_with_notification_identity(k, b, survivor, deleted, wire_edges, Some(env), body)
        });
    };
    if survivor == deleted {
        return Ok(());
    }
    let survivor = survivor.ok_or_else(|| named(TopologyFailure::NoVertexGiven))?;
    let deleted = deleted.ok_or_else(|| named(TopologyFailure::NoVertexGiven))?;
    crate::error::ignore_status(notify_pre_edit_with_body(k, b, Some(&mut *env), body))?;
    crate::error::ignore_status(notify_vertex_pre_edit_with_body(
        k,
        b,
        survivor,
        Some(&mut *env),
        body,
    ))?;
    crate::error::ignore_status(notify_merge_with_body(
        k,
        b,
        survivor,
        deleted,
        Some(&mut *env),
        body,
    ))?;
    for face in vertex::faces(b, deleted)? {
        crate::error::ignore_status(notify_face_pre_edit_with_body(
            k,
            b,
            face,
            Some(&mut *env),
            body,
        ))?;
    }
    let distance = b.vertex(survivor)?.point.distance(b.vertex(deleted)?.point);
    let mut pair = None;
    // Not upstream (WR-607; SolidCraft change,
    // tangency-labs/smlib 661f79c): every region both vertices are in has its pair of
    // shells, all of which the glue joins (a solid's vertex is in a shell of the solid's
    // region and one of the infinite region's); one pair is kept apart below for the
    // vertex-shell cases.
    let mut pairs = Vec::new();
    for first in vertex::shells(b, survivor)? {
        let region = b.shell(first)?.region;
        for last in vertex::shells(b, deleted)? {
            if region == b.shell(last)?.region {
                pair = Some((first, last));
                pairs.push((first, last));
                break;
            }
        }
    }
    let (first, last) = pair.ok_or_else(|| named(TopologyFailure::VerticesShareNoRegion))?;
    let mut first = Some(first);
    let radius = distance + b.vertex(deleted)?.tolerance;
    if radius > b.vertex(survivor)?.tolerance {
        crate::error::ignore_status(vertex::set_tolerance(b, survivor, radius, true, true))?;
    }
    if vertex::is_shell_vertex(b, deleted)? {
        delete_vertex(k, b, deleted, Some(env))?;
        return Ok(());
    }
    // `IsShellVertex`: the primary vertex use is owned by a shell
    let shell_vertexuse_of_survivor = match vertex::primary_vertexuse(b, survivor)? {
        Some(vu) => match b.vertexuse(vu)?.owner {
            crate::topology::VertexuseOwner::Shell(shell) => Some((vu, shell)),
            _ => None,
        },
        None => None,
    };
    if let Some((vu, shell)) = shell_vertexuse_of_survivor {
        let shell = shell.ok_or_else(|| {
            broken_link(
                "a shell's vertex use names no shell, though the kernel makes a shell vertex's \
                 vertex use in its shell",
            )
        })?;
        if shell_vertexuse(b, shell)? != Some(vu) {
            return Err(defect(
                "the vertex shell does not hold its vertex's vertex use, though the kernel \
                 makes a vertex shell with its vertex use",
            ));
        }
        let region = b
            .shell(shell)?
            .region
            .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
        remove_region_shell(b, region, shell)?;
        destroy_shell(k, b, &mut *env, shell)?;
        first = None;
        remove_vertexuse(b, survivor, vu)?;
        destroy_topology_record(b, crate::error::EntityRef::Vertexuse(vu))?;
    }
    let wires = if let Some(given) = wire_edges {
        given.to_vec()
    } else {
        // GetWireEdges always returns SM_SUCCESS upstream. Errors here mark
        // missing callees or undefined pointer accesses, not an ignored native
        // status; do not invent an empty/partial wire list on those errors.
        shell_wire_edges(k, b, last)?
    };
    // WR-607: the other regions' pairs of shells, and their wire edges, likewise.
    let mut others = Vec::new();
    for (surv, del) in pairs {
        if del == last || surv == del || b.shell(del)?.is_vertex_shell() {
            continue;
        }
        let other_wires = if let Some(given) = wire_edges {
            given.to_vec()
        } else {
            shell_wire_edges(k, b, del)?
        };
        others.push((surv, del, other_wires));
    }
    for vu in vertex::vertexuses(b, deleted)? {
        remove_vertexuse(b, deleted, vu)?;
        post_insert_vertexuse(b, survivor, vu)?;
        // The vertex use now sits on `survivor`'s point rather than `deleted`'s, so its
        // cached vertex/edge and vertex/face gaps are stale (#21432).
        let use_ = b.vertexuse_mut(vu)?;
        use_.vertex_edge_gap.reset();
        use_.vertex_face_gap.reset();
    }
    remove_owned_vertex(b, deleted);
    destroy_topology_record(b, crate::error::EntityRef::Vertex(deleted))?;
    if let Some(first) = first.filter(|&first| first != last) {
        if let Some(vu) = shell_vertexuse(b, last)? {
            let vertex = b
                .vertexuse(vu)?
                .vertex
                .ok_or_else(|| broken_link(VU_NO_V))?;
            remove_vertexuse(b, vertex, vu)?;
            clear_shell_list(b, last)?;
            destroy_topology_record(b, crate::error::EntityRef::Vertexuse(vu))?;
        } else {
            transfer_shell_children(k, b, last, first, &wires)?;
        }
        let region = b
            .shell(last)?
            .region
            .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
        remove_region_shell(b, region, last)?;
        destroy_shell(k, b, &mut *env, last)?;
    }
    // WR-607: join the other regions' pairs as well.
    for (surv, del, other_wires) in others {
        transfer_shell_children(k, b, del, surv, &other_wires)?;
        let region = b
            .shell(del)?
            .region
            .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
        remove_region_shell(b, region, del)?;
        destroy_shell(k, b, &mut *env, del)?;
    }
    Ok(())
}

/// `Notify(SM_NO_PRE_EDIT, this, NULL, NULL)` ([`brep_p2::notify`], no B-rep cache
/// held by the caller).
pub(super) fn notify_pre_edit(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    notify_pre_edit_with_body(k, b, notifications, None)
}

fn notify_pre_edit_with_body(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    body: Option<ObjectRef>,
) -> Result<()> {
    let data = [
        Some(brep_p2::NotifyArg::Object(
            body.unwrap_or_else(|| brep_p2::brep_ref(b)),
        )),
        None,
        None,
    ];
    with_notifications(b, notifications, Some(&mut k.context), |b, env, _| {
        brep_p2::notify_with_body_identity(
            b,
            env,
            None,
            NotifyOperation::PreEdit,
            data,
            body.unwrap_or_else(|| brep_p2::brep_ref(b)),
        )
    })
}
pub(super) fn notify_vertex_pre_edit(
    k: &mut Kernel,
    b: &mut Brep,
    vertex: VertexId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    notify_vertex_pre_edit_with_body(k, b, vertex, notifications, None)
}

fn notify_vertex_pre_edit_with_body(
    k: &mut Kernel,
    b: &mut Brep,
    vertex: VertexId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    body: Option<ObjectRef>,
) -> Result<()> {
    let brep = body.unwrap_or_else(|| brep_p2::brep_ref(b));
    with_notifications(
        b,
        notifications,
        Some(&mut k.context),
        |b, env, fallback| {
            let this = (env.key_of)(crate::error::EntityRef::Vertex(vertex));
            vertex::notify(
                b,
                vertex,
                env.records,
                env.attributes,
                env.context.as_deref_mut().or(fallback),
                NotifyOperation::PreEdit,
                this,
                [Some(this), Some(brep), None],
                &|vu| (env.key_of)(crate::error::EntityRef::Vertexuse(vu)),
            )
        },
    )
}
/// `pFace->Notify(SM_NO_PRE_EDIT, pFace, this, NULL)` ([`crate::topology::face::p3::notify`]);
/// no caller holds the surface's trimmed surface cache.
pub(super) fn notify_face_pre_edit(
    k: &mut Kernel,
    b: &mut Brep,
    face: FaceId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    notify_face_pre_edit_with_body(k, b, face, notifications, None)
}

fn notify_face_pre_edit_with_body(
    k: &mut Kernel,
    b: &mut Brep,
    face: FaceId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    body: Option<ObjectRef>,
) -> Result<()> {
    let brep = body.unwrap_or_else(|| brep_p2::brep_ref(b));
    with_notifications(
        b,
        notifications,
        Some(&mut k.context),
        |b, env, fallback| {
            let this = (env.key_of)(crate::error::EntityRef::Face(face));
            let mut forwarded = crate::topology::edge::NotifyEnv {
                records: env.records,
                attributes: env.attributes,
                context: env.context.as_deref_mut().or(fallback),
                key_of: env.key_of,
            };
            crate::topology::face::p3::notify(
                b,
                face,
                &mut forwarded,
                None,
                NotifyOperation::PreEdit,
                [Some(this), Some(brep), None],
            )
        },
    )
}
fn shell_vertexuse(b: &Brep, shell: ShellId) -> Result<Option<VertexuseId>> {
    let shell = b.shell(shell)?;
    Ok(if shell.is_vertex_shell() {
        match shell.members {
            crate::topology::shell::ShellMembers::Vertexuse(vu) => Some(vu),
            _ => None,
        }
    } else {
        None
    })
}
fn remove_region_shell(b: &mut Brep, region: RegionId, shell: ShellId) -> Result<()> {
    crate::topology::shell::region_remove_shell(b, region, shell)
}
/// `delete pShell` after detachment from its region ([`crate::topology::shell::destroy`]:
/// `~SmShell`'s `SM_NO_DESTRUCTION` and `~SmAObject`'s `ClearAttributes`) under the
/// edit's notification environment.
fn destroy_shell(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut EditNotifyEnv<'_>,
    shell: ShellId,
) -> Result<()> {
    crate::topology::shell::destroy(k, b, shell, env)
}

/// A plain `delete` after detachment from the topology graph: the record leaves its
/// arena (`topology.brep_p4`'s record removal, which keeps every other typed id
/// valid). It does not call `DeleteVertex` (that would repeat the notifications and
/// list removals) and deletes only the named record. Vertex and vertex use
/// destructors send no notification upstream; their `~SmAObject` attribute cleanup is
/// not modelled.
fn destroy_topology_record(b: &mut Brep, entity: crate::error::EntityRef) -> Result<()> {
    use crate::error::EntityRef as E;
    match entity {
        E::Vertexuse(id) => b.remove_vertexuse_record(id).map(drop),
        E::Vertex(id) => b.remove_vertex_record(id).map(drop),
        _ => Err(SolidError::invalid_input(
            OP,
            "unexpected record to destroy",
        )),
    }
}
fn clear_shell_list(b: &mut Brep, shell: ShellId) -> Result<()> {
    // GlueVertices clears only the head and size, retaining m_tShellType.
    b.shell_mut(shell)?.members = crate::topology::shell::ShellMembers::Empty;
    Ok(())
}
fn shell_wire_edges(k: &mut Kernel, b: &Brep, shell: ShellId) -> Result<Vec<EdgeId>> {
    crate::topology::shell::wire_edges(k, b, shell)
}
fn transfer_shell_children(
    k: &mut Kernel,
    b: &mut Brep,
    source: ShellId,
    target: ShellId,
    wire_edges: &[EdgeId],
) -> Result<()> {
    let wires: Vec<_> = wire_edges.iter().copied().map(Some).collect();
    crate::topology::shell::transfer_children(k, b, source, target, Some(&wires))
}
/// `m_pVertexListHead->Remove(pVertexToDelete)`.
fn remove_owned_vertex(b: &mut Brep, vertex: VertexId) {
    b.remove_from_vertex_list(vertex);
}

/// Outputs of `MakeEdgeInFace`, initialized to null after its editing and face checks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EdgeInFace {
    /// The inserted edge.
    pub edge: Option<EdgeId>,
    /// The new loop when insertion splits an existing loop.
    pub loop_: Option<LoopId>,
    /// The new face when insertion splits the face.
    pub face: Option<FaceId>,
}

/// Finds both vertex sectors then inserts an edge into a face. The native output
/// lifecycle is preserved: editing-disabled/null-face failures leave outputs alone;
/// a valid face clears all three before sector classification.
pub fn make_edge_in_face(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    face: Option<FaceId>,
    start: Option<VertexId>,
    end: Option<VertexId>,
    curve: Curve,
    uv_curve: Option<BSplineCurve>,
    interval: Extent1d,
    orientation: OrientType,
    max_gap_3d: f64,
    output: &mut EdgeInFace,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    make_edge_in_face_with_uv_curve(
        k,
        b,
        editing_enabled,
        face,
        start,
        end,
        curve,
        uv_curve.map(Curve::from),
        interval,
        orientation,
        max_gap_3d,
        output,
        notifications,
    )
}

/// [`make_edge_in_face`] accepting a polymorphic UV trim, including the native
/// `SmBSplineCurve`-derived line and conic kinds, without replacing their geometry.
/// Editing checks and output lifetime are identical to the original entry point.
pub fn make_edge_in_face_with_uv_curve(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    face: Option<FaceId>,
    start: Option<VertexId>,
    end: Option<VertexId>,
    curve: Curve,
    uv_curve: Option<Curve>,
    interval: Extent1d,
    orientation: OrientType,
    max_gap_3d: f64,
    output: &mut EdgeInFace,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    with_p3_notifications(k, b, notifications, |k, b, env| {
        make_edge_with_backend(
            &mut ArenaFaceEdge {
                k,
                b,
                env,
                curve_ref: &edge_curve_identity,
            },
            editing_enabled,
            face,
            start,
            end,
            curve,
            uv_curve,
            interval,
            orientation,
            max_gap_3d,
            output,
        )
    })
}

/// [`make_edge_in_face_with_uv_curve`] inside a caller's edit: notifying through the
/// caller's `env`, with the new edge's curve identified by the caller's `curve_ref`
/// (as [`super::p9::make_edge_in_face_with_vu`] takes it). The editing check reads
/// `b`'s own `m_bEditingEnabled` ([`Brep::editing_enabled`]), as upstream's member does.
///
/// # Errors
///
/// Those of [`make_edge_in_face_with_uv_curve`].
pub fn make_edge_in_face_in(
    k: &Kernel,
    b: &mut Brep,
    face: Option<FaceId>,
    start: Option<VertexId>,
    end: Option<VertexId>,
    curve: Curve,
    uv_curve: Option<Curve>,
    interval: Extent1d,
    orientation: OrientType,
    max_gap_3d: f64,
    output: &mut EdgeInFace,
    env: &mut EditNotifyEnv<'_>,
    curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
) -> Result<()> {
    let editing_enabled = b.editing_enabled();
    make_edge_with_backend(
        &mut ArenaFaceEdge {
            k,
            b,
            env,
            curve_ref,
        },
        editing_enabled,
        face,
        start,
        end,
        curve,
        uv_curve,
        interval,
        orientation,
        max_gap_3d,
        output,
    )
}

/// Handle adapter for the shared owned control flow. Missing sector/insertion
/// primitives remain calls to their actual owning units for either handle model.
pub(crate) trait FaceEdgeBackend {
    type Face: Copy;
    type Vertex: Copy;
    type Curve;
    type UvCurve;
    type Vertexuse: Copy;
    type Output: Default;
    fn validate_face(&self, face: Self::Face) -> Result<()>;
    fn tolerance(&self) -> Result<f64>;
    fn sector(
        &mut self,
        face: Self::Face,
        vertex: Option<Self::Vertex>,
        curve: &Self::Curve,
        uv: Option<&Self::UvCurve>,
        start: f64,
        end: f64,
        orientation: OrientType,
        tolerance: f64,
        all_sectors: bool,
    ) -> Result<Option<Self::Vertexuse>>;
    fn insert(
        &mut self,
        face: Self::Face,
        first: Self::Vertexuse,
        last: Self::Vertexuse,
        curve: Self::Curve,
        uv: Option<Self::UvCurve>,
        interval: Extent1d,
        orientation: OrientType,
        max_gap: f64,
        output: &mut Self::Output,
    ) -> Result<()>;
}

pub(crate) fn make_edge_with_backend<B: FaceEdgeBackend>(
    b: &mut B,
    editing_enabled: bool,
    face: Option<B::Face>,
    start: Option<B::Vertex>,
    end: Option<B::Vertex>,
    curve: B::Curve,
    uv_curve: Option<B::UvCurve>,
    interval: Extent1d,
    orientation: OrientType,
    max_gap_3d: f64,
    output: &mut B::Output,
) -> Result<()> {
    require_editing(editing_enabled)?;
    let f = face.ok_or_else(|| named(TopologyFailure::NoFaceGiven))?;
    b.validate_face(f)?;
    *output = B::Output::default();
    let (t_start, t_end, end_orientation) = if orientation == OrientType::Opposite {
        (interval.max(), interval.min(), OrientType::Same)
    } else {
        (interval.min(), interval.max(), OrientType::Opposite)
    };
    let first = b
        .sector(
            f,
            start,
            &curve,
            uv_curve.as_ref(),
            t_start,
            t_end,
            orientation,
            b.tolerance()?,
            false,
        )?
        .ok_or_else(|| {
            SolidError::topology(
                NO_VERTEX_SECTOR_OP,
                TopologyFailure::NoVertexSector { at_end: false },
            )
        })?;
    let last = b
        .sector(
            f,
            end,
            &curve,
            uv_curve.as_ref(),
            t_end,
            t_start,
            end_orientation,
            b.tolerance()?,
            false,
        )?
        .ok_or_else(|| {
            SolidError::topology(
                NO_VERTEX_SECTOR_OP,
                TopologyFailure::NoVertexSector { at_end: true },
            )
        })?;
    b.insert(
        f,
        first,
        last,
        curve,
        uv_curve,
        interval,
        orientation,
        max_gap_3d,
        output,
    )
}

struct ArenaFaceEdge<'a, 'e> {
    k: &'a Kernel,
    b: &'a mut Brep,
    env: &'a mut EditNotifyEnv<'e>,
    curve_ref: &'a dyn Fn(EdgeId, i64) -> ObjectRef,
}
impl FaceEdgeBackend for ArenaFaceEdge<'_, '_> {
    type Face = FaceId;
    type Vertex = VertexId;
    type Curve = Curve;
    type UvCurve = Curve;
    type Vertexuse = VertexuseId;
    type Output = EdgeInFace;
    fn validate_face(&self, f: FaceId) -> Result<()> {
        self.b.face(f).map(|_| ())
    }
    fn tolerance(&self) -> Result<f64> {
        Ok(self.b.tolerance())
    }
    fn sector(
        &mut self,
        f: FaceId,
        v: Option<VertexId>,
        c: &Curve,
        uv: Option<&Curve>,
        start: f64,
        end: f64,
        orientation: OrientType,
        tolerance: f64,
        all: bool,
    ) -> Result<Option<VertexuseId>> {
        find_vertex_sector(
            self.k,
            self.b,
            f,
            v,
            c,
            uv,
            start,
            end,
            orientation,
            tolerance,
            all,
        )
    }
    fn insert(
        &mut self,
        f: FaceId,
        first: VertexuseId,
        last: VertexuseId,
        c: Curve,
        uv: Option<Curve>,
        interval: Extent1d,
        orientation: OrientType,
        gap: f64,
        out: &mut EdgeInFace,
    ) -> Result<()> {
        make_edge_in_face_with_vu(
            self.k,
            self.b,
            self.env,
            f,
            first,
            last,
            c,
            uv,
            interval,
            orientation,
            gap,
            out,
            self.curve_ref,
        )
    }
}

/// `SmFace::FindVertexSector(vertex, curve, uv, start, end, orientation, tolerance,
/// all_sectors, ...)` with upstream's default `bUVSpaceOkay = TRUE`
/// ([`face_p4::find_vertex_sector`]); only the sector's vertex use is read.
#[allow(clippy::too_many_arguments)]
pub(crate) fn find_vertex_sector(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    v: Option<VertexId>,
    curve: &Curve,
    uv: Option<&Curve>,
    start: f64,
    end: f64,
    orientation: OrientType,
    tolerance: f64,
    all_sectors: bool,
) -> Result<Option<VertexuseId>> {
    // NER(cpVertex)
    let vertex = v.ok_or_else(|| named(TopologyFailure::NoVertexGiven))?;
    let sector = face_p4::find_vertex_sector(
        k,
        b,
        f,
        face_p4::FindVertexSectorArgs {
            vertex,
            curve,
            uv_curve: uv,
            param_at_vertex: start,
            param_ivl_end: end,
            orientation,
            zone_tol_3d: tolerance,
            validate_sector: all_sectors,
            uv_space_okay: true,
        },
    )?;
    Ok(sector.vertexuse)
}

/// `SmBrep::MakeEdgeInFaceWithVU(face, first, last, curve, uv, interval, orientation,
/// max_gap_3d, ...)` without drop-curve state ([`super::p9::make_edge_in_face_with_vu`]).
/// The double gap becomes the `SmZoneTol3d` parameter. The new edge's curve takes its
/// identity from `curve_ref` (a call-local one for the plain entry points, module docs).
#[allow(clippy::too_many_arguments)]
fn make_edge_in_face_with_vu(
    k: &Kernel,
    b: &mut Brep,
    env: &mut EditNotifyEnv<'_>,
    f: FaceId,
    first: VertexuseId,
    last: VertexuseId,
    curve: Curve,
    uv: Option<Curve>,
    interval: Extent1d,
    orientation: OrientType,
    max_gap_3d: f64,
    output: &mut EdgeInFace,
    curve_ref: &dyn Fn(EdgeId, i64) -> ObjectRef,
) -> Result<()> {
    let made = super::p9::make_edge_in_face_with_vu(
        k,
        b,
        super::p9::EdgeInFaceInput {
            face: Some(f),
            vertices: [Some(first), Some(last)],
            curve: Some(curve),
            uv_trim: uv,
            drop_fail: None,
            interval,
            orientation,
            curve_zone_tol_3d: crate::geom::tol::zone_tol3d(k, max_gap_3d),
        },
        env,
        curve_ref,
    )?;
    *output = EdgeInFace {
        edge: Some(made.edge),
        loop_: made.loop_,
        face: made.face,
    };
    Ok(())
}

/// A call-local identity for an edge's curve: the type code above the edge index,
/// outside the arena identities' key space (bare indices), as `heal::stitch` does.
fn edge_curve_identity(edge: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        type_code,
        key: crate::geom::attribute::ObjectKey(
            (type_code.unsigned_abs() << 32) | u64::from(edge.index()),
        ),
    }
}

/// Makes the selected regions manifold. `None` means all non-infinite regions;
/// `Some(&[])` means no regions and returns the native `SM_ERR`.
pub fn make_manifold(
    k: &mut Kernel,
    b: &mut Brep,
    regions_to_keep: Option<&[RegionId]>,
    keep_interior_faces: bool,
) -> Result<()> {
    plain_edit(k, b, |k, b, env| {
        make_manifold_in(k, b, regions_to_keep, keep_interior_faces, env)
    })
}

/// [`make_manifold`] under the edit's notification environment ([`plain_edit`]).
fn make_manifold_in(
    k: &mut Kernel,
    b: &mut Brep,
    regions_to_keep: Option<&[RegionId]>,
    keep_interior_faces: bool,
    env: &mut EditNotifyEnv<'_>,
) -> Result<()> {
    // The temporary editing change has no lasting effect on this early return.
    if regions_to_keep.is_some_and(<[RegionId]>::is_empty) {
        return Err(named(TopologyFailure::NoRegionsToKeepGiven));
    }
    let previous = set_editing_enabled(b, true);
    let result = (|| {
        let mut regions = if let Some(given) = regions_to_keep {
            given.to_vec()
        } else {
            let mut all = brep_regions(b)?;
            let infinite = b.infinite_region();
            if let Some(index) = all.iter().position(|r| Some(*r) == infinite) {
                all.remove(index);
            }
            all
        };
        if regions.is_empty() {
            return Err(named(TopologyFailure::NoFiniteRegion));
        }
        for face in brep_faces(b)? {
            let (first, last) = face_shells(b, face)?;
            let r1 = b
                .shell(first)?
                .region
                .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
            let r2 = b
                .shell(last)?
                .region
                .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
            let keep1 = regions.contains(&r1);
            let keep2 = regions.contains(&r2);
            if r1 != r2 && (keep1 != keep2 || (keep_interior_faces && keep1)) {
                continue;
            }
            delete_face(k, b, Some(&mut *env), face, false, true, None)?;
        }
        for edge in brep_edges(b)? {
            if crate::topology::edge::is_wire(b, edge)? {
                delete_edge_with(k, b, edge, None, None)?;
            }
        }
        for vertex in brep_vertices(b)? {
            if vertex::is_shell_vertex(b, vertex)? {
                delete_vertex(k, b, vertex, None)?;
            }
        }
        regions = brep_regions(b)?;
        for region in regions {
            let is_infinite = Some(region) == b.infinite_region();
            set_region_is_void(b, region, is_infinite)?;
        }
        Ok(())
    })();
    // Upstream restores in a destructor.
    set_editing_enabled(b, previous);
    result
}

/// (not upstream, #18570) Whether each region of `b` is void, read from its faces: in a
/// solid every face has material on one side and void on the other, so the flags
/// alternate across faces, starting from the infinite region (void). A region of a
/// hollowed body that only the cavity's faces bound is void; one that faces reach from
/// the outside in one step is material. Regions no face reaches are left out. `None`
/// where there is no infinite region, or where the faces do not alternate: a face with
/// one region on both sides, or two regions a face divides that would both be void or
/// both material.
///
/// # Errors
///
/// Those of the face, face use and shell queries.
pub(crate) fn alternating_void_flags(b: &Brep) -> Result<Option<BTreeMap<RegionId, bool>>> {
    let Some(infinite) = b.infinite_region() else {
        return Ok(None);
    };
    let mut across: BTreeMap<RegionId, Vec<RegionId>> = BTreeMap::new();
    for face in brep_faces(b)? {
        let (first, last) = face_shells(b, face)?;
        let (Some(r1), Some(r2)) = (b.shell(first)?.region, b.shell(last)?.region) else {
            return Ok(None);
        };
        if r1 == r2 {
            return Ok(None);
        }
        across.entry(r1).or_default().push(r2);
        across.entry(r2).or_default().push(r1);
    }
    let mut void = BTreeMap::from([(infinite, true)]);
    let mut todo = vec![infinite];
    while let Some(region) = todo.pop() {
        let flag = void.get(&region).copied().unwrap_or(true);
        for &next in across.get(&region).map_or(&[][..], Vec::as_slice) {
            match void.get(&next) {
                Some(&other) if other == flag => return Ok(None),
                Some(_) => {}
                None => {
                    void.insert(next, !flag);
                    todo.push(next);
                }
            }
        }
    }
    Ok(Some(void))
}

/// (not upstream, #18570) Sets each region's void flag as [`alternating_void_flags`]
/// reads it, where it reads one; a region it does not reach, and every region where it
/// reads none, keeps its flag. Whether any flag changed.
///
/// # Errors
///
/// Those of [`alternating_void_flags`].
pub(crate) fn set_alternating_void_flags(b: &mut Brep) -> Result<bool> {
    let Some(flags) = alternating_void_flags(b)? else {
        return Ok(false);
    };
    let mut changed = false;
    for (region, is_void) in flags {
        let value = i32::from(is_void);
        if b.region(region)?.is_void() != value {
            b.region_mut(region)?.set_is_void(value);
            changed = true;
        }
    }
    Ok(changed)
}

/// Sets `m_bEditingEnabled`, returning the previous value.
fn set_editing_enabled(b: &mut Brep, enabled: bool) -> bool {
    let previous = b.editing_enabled();
    b.set_editing_enabled(enabled);
    previous
}

pub(super) fn brep_regions(b: &Brep) -> Result<Vec<RegionId>> {
    crate::topology::brep::p1::regions(b, None)
}
pub(super) fn brep_faces(b: &Brep) -> Result<Vec<FaceId>> {
    crate::topology::brep::p1::faces(b, None)
}
pub(super) fn brep_edges(b: &Brep) -> Result<Vec<EdgeId>> {
    crate::topology::brep::p1::edges(b, None)
}
pub(super) fn brep_vertices(b: &Brep) -> Result<Vec<VertexId>> {
    crate::topology::brep::p1::vertices(b, None)
}
fn face_shells(b: &Brep, face: FaceId) -> Result<(ShellId, ShellId)> {
    let (a, z) = crate::topology::face::p2::faceuses(b, face)?;
    let a = a.ok_or_else(|| broken_link(FACE_NO_FU))?;
    let z = z.ok_or_else(|| broken_link(FACE_NO_FU))?;
    Ok((
        b.faceuse(a)?
            .shell
            .ok_or_else(|| broken_link(FU_NO_SHELL))?,
        b.faceuse(z)?
            .shell
            .ok_or_else(|| broken_link(FU_NO_SHELL))?,
    ))
}
/// `SmBrep::DeleteFace(face, bDeleteEdgesVertices, bDoRegionNesting, NULL,
/// pOptRegionToKeep)` ([`crate::topology::brep::p3::delete_face`]), notifying as
/// [`with_p3_notifications`].
fn delete_face(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    face: FaceId,
    delete_edges_vertices: bool,
    nest: bool,
    keep_region: Option<RegionId>,
) -> Result<()> {
    use crate::topology::brep::p3 as brep_p3;
    let options = brep_p3::DeleteFaceOptions {
        delete_edges_vertices,
        do_region_nesting: nest,
        region_to_keep: keep_region,
    };
    with_p3_notifications(k, b, notifications, |k, b, env| {
        let mut marks = crate::topology::topology_traverser::Marks::default();
        brep_p3::delete_face(k, b, env, &mut marks, face, options).map(drop)
    })
}
fn set_region_is_void(b: &mut Brep, region: RegionId, is_void: bool) -> Result<()> {
    b.region_mut(region)?.set_is_void(i32::from(is_void));
    Ok(())
}

/// Removes redundant edges/vertices, preserving optional keep lists. The native
/// owning-list traversal is required even for apparently empty arena contents.
pub fn remove_topological_edges_and_vertices(
    k: &mut Kernel,
    b: &mut Brep,
    keep_edges: Option<&[EdgeId]>,
    keep_vertices: Option<&[VertexId]>,
) -> Result<()> {
    remove_topological_with_notifications(k, b, keep_edges, keep_vertices, None)
}

/// Removes topological entities using the actual edit-notification environment.
pub fn remove_topological_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    keep_edges: Option<&[EdgeId]>,
    keep_vertices: Option<&[VertexId]>,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    let Some(env) = notifications else {
        return plain_edit(k, b, |k, b, env| {
            remove_topological_with_notifications(k, b, keep_edges, keep_vertices, Some(env))
        });
    };
    remove_topological_in(k, b, keep_edges, keep_vertices, None, env).map(drop)
}

/// Removes only the named redundant joins and their vertices. A local extension
/// must leave the trims and isolated loop vertices of unrelated faces in place.
pub(crate) fn remove_topological_joins(
    k: &mut Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    vertices: &[VertexId],
) -> Result<()> {
    if edges.is_empty() && vertices.is_empty() {
        return Ok(());
    }
    // A missing trim can also be created on another edge of the face while testing a
    // rim or completing a seam. A refusal restores the geometry, topology and attributes
    // together, after the edit has returned its notification records to the body.
    let edge_budgets: Vec<_> = b.edges().iter().map(|(e, r)| (e, r.tolerance)).collect();
    let vertex_budgets: Vec<_> = b.vertices().iter().map(|(v, r)| (v, r.tolerance)).collect();
    b.with_rollback_untouched(
        |b| {
            let fits = plain_edit(k, b, |k, b, env| {
                remove_topological_in(k, b, None, None, Some((edges, vertices)), env)
            })?;
            Ok(fits
                && edge_budgets
                    .iter()
                    .all(|&(e, t)| b.edges().get(e).is_none_or(|r| r.tolerance <= t))
                && vertex_budgets
                    .iter()
                    .all(|&(v, t)| b.vertices().get(v).is_none_or(|r| r.tolerance <= t)))
        },
        |result| matches!(result, Ok(true)),
    )
    .map(drop)
}

fn remove_topological_in(
    k: &mut Kernel,
    b: &mut Brep,
    keep_edges: Option<&[EdgeId]>,
    keep_vertices: Option<&[VertexId]>,
    joins: Option<(&[EdgeId], &[VertexId])>,
    env: &mut EditNotifyEnv<'_>,
) -> Result<bool> {
    let previous = set_editing_enabled(b, true);
    let mut joined = Vec::new();
    let result = (|| {
        let edges = match joins {
            Some((edges, _)) => edges.to_vec(),
            None => brep_edges(b)?,
        };
        for edge in edges {
            // An edge of the list a seam completion below removed is gone.
            if b.edge(edge).is_err() {
                continue;
            }
            // Not upstream (#17616): an edge on the keep list stays, so it is not tested.
            // IsTopological's coverage test expands the covering face's surface in place
            // and drops that face's UV trims, leaving its UV domain on the old
            // parameterisation; only the merge below sets the domain anew. Upstream tests
            // first, and the face of a kept edge keeps the expanded surface under its old
            // domain: on two bends of one cylinder its trims dropped off the domain.
            if keep_edges.is_some_and(|keep| keep.contains(&edge)) {
                continue;
            }
            // A constructed join already lies on its support. Keep the two faces
            // separate if covering them would require either tolerance to grow.
            let topological = if joins.is_some() {
                crate::topology::edge::is_topological_without_growth(k, b, edge)
            } else {
                crate::topology::edge::is_topological(k, b, edge, 1.0)
            };
            let Some(topological) = crate::error::ignored(topological)? else {
                continue;
            };
            if !topological.is_topological {
                continue;
            }
            // is_topological finds only a manifold edge (four edge uses) topological, with
            // the face to keep, after reading both faces and their surfaces
            let keep = topological.surf_to_keep.ok_or_else(|| {
                broken_link(
                    "is_topological found the edge topological without the face to keep, \
                     though it names that face whenever it finds an edge topological",
                )
            })?;
            let edgeuses = crate::topology::edge::edgeuses(b, edge, None, None)?;
            let [first, _, third, ..] = *edgeuses.as_slice() else {
                return Err(defect(
                    "an edge is_topological found topological has fewer than three edge uses, \
                     though it finds only an edge with four topological",
                ));
            };
            let face0 = crate::topology::edgeuse::p1::face(b, first)?.ok_or_else(|| {
                broken_link(
                    "an edge use of a topological edge has no face, though is_topological \
                     has just read the faces of both",
                )
            })?;
            let face1 = crate::topology::edgeuse::p1::face(b, third)?.ok_or_else(|| {
                broken_link(
                    "an edge use of a topological edge has no face, though is_topological \
                     has just read the faces of both",
                )
            })?;
            // A face owns one surface value. Equal owner metadata on two
            // values cannot establish pointer identity.
            let same_surface = face0 == face1;
            if !same_surface {
                let domain = b
                    .face(keep)?
                    .surface
                    .as_ref()
                    .ok_or_else(|| {
                        broken_link(
                            "the face to keep has no surface, though is_topological has just \
                             read the surfaces of both faces",
                        )
                    })?
                    .natural_uv_domain_with_nurb(k)?;
                crate::topology::face::p2::set_uv_domain(b, keep, &domain)?;
                let tolerance = b.face(keep)?.tolerance;
                if joins.is_none() && topological.dist_between > tolerance / 10.0 {
                    set_face_tolerance(b, keep, tolerance + topological.dist_between, true, true)?;
                }
            }
            // Return the kept surface's current owner after the deletion. Its
            // former face id can become stale when DeleteEdge merges the faces.
            let current_face = delete_edge_keep_surface(k, b, edge, keep, Some(env))?;
            let domain = topological.merged_face_domain.unwrap_or_default();
            crate::topology::face::p2::set_uv_domain(b, current_face, &domain)?;
            // (#16866) `IsTopological`'s cover may have taken in a face straddling the kept
            // surface's seam, the merged domain staying the period it spans: the merged face
            // gets the seam a face on a periodic surface has, or its loops' UV trims jump by
            // the period and the check finds their orientation wrong. A merged face with no
            // seam crossed or missing is left as it is.
            crate::topology::face::p1::complete_merged_face_seam(k, b, Some(env), current_face)?;
        }
        let vertices = match joins {
            Some((_, vertices)) => vertices.to_vec(),
            None => brep_vertices(b)?,
        };
        for vertex in vertices {
            if b.vertex(vertex).is_err() {
                continue;
            }
            if vertex::is_loop_vertex(b, vertex)? {
                delete_vertex(k, b, vertex, Some(env))?;
                continue;
            }
            if crate::error::ignored(vertex::is_topological_vertex(k, b, vertex))?.unwrap_or(false)
            {
                if keep_vertices.is_some_and(|keep| keep.contains(&vertex)) {
                    continue;
                }
                if joins.is_some() {
                    if !join_vertex_without_growth(k, b, vertex, env)? {
                        return Ok(false);
                    }
                } else if let Some(survivor) = crate::error::ignored(
                    delete_topological_vertex_joining(k, b, true, Some(vertex), None, Some(env)),
                )? {
                    joined.push(survivor);
                }
            }
        }
        for e in joined {
            cover_joined_line_gaps(k, b, e)?;
        }
        Ok(true)
    })();
    // Upstream restores in a destructor.
    set_editing_enabled(b, previous);
    result
}

/// `DeleteEdge(pEdge, pSurfToKeep)` followed by `pSurfToKeep->GetFace()`: the face
/// holding the kept surface afterwards. A surface is named by its face here; when
/// `DeleteEdge` merges the faces and swaps the kept surface into the survivor, the
/// kept face's record is deleted and the survivor (the other face of the edge) holds
/// it; otherwise the kept face still does.
fn delete_edge_keep_surface(
    k: &mut Kernel,
    b: &mut Brep,
    edge: EdgeId,
    surface_face: FaceId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<FaceId> {
    let mut faces = Vec::new();
    for eu in crate::topology::edge::edgeuses(b, edge, None, None)? {
        if let Some(f) = crate::topology::edgeuse::p1::face(b, eu)? {
            if f != surface_face && !faces.contains(&f) {
                faces.push(f);
            }
        }
    }
    delete_edge_with(k, b, edge, Some(surface_face), notifications)?;
    if b.face(surface_face).is_ok() {
        return Ok(surface_face);
    }
    faces
        .into_iter()
        .find(|&f| b.face(f).is_ok_and(|r| r.surface.is_some()))
        .ok_or_else(|| {
            broken_link(
                "no face with a surface is left after deleting the edge, though the deletion \
                 keeps the kept surface on the face that survives",
            )
        })
}

/// Removes the requested faces then their wire edges and shell vertices.
/// Even an empty list advances and releases one unlocked context traversal mark.
/// Editing-disabled refusal occurs before the mark is acquired.
pub fn remove_faces(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    faces: &[FaceId],
) -> Result<()> {
    plain_edit(k, b, |k, b, env| {
        remove_faces_in(k, b, editing_enabled, faces, env)
    })
}

/// [`remove_faces`] under the caller's notification environment `env` (the objects'
/// `SmTopology` records and attributes), instead of the kernel's (`plain_edit`).
///
/// # Errors
///
/// Those of [`remove_faces`].
pub fn remove_faces_notified(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    faces: &[FaceId],
    env: &mut EditNotifyEnv<'_>,
) -> Result<()> {
    remove_faces_in(k, b, editing_enabled, faces, env)
}

/// [`remove_faces`] under the edit's notification environment ([`plain_edit`]).
fn remove_faces_in(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    faces: &[FaceId],
    env: &mut EditNotifyEnv<'_>,
) -> Result<()> {
    require_editing(editing_enabled)?;
    let mut mark = MarkLock::new(core::slice::from_mut(&mut k.context), 0, ALL_MARKS)?;
    let result = (|| {
        let mut all_edges = Vec::new();
        let mut all_vertices = Vec::new();
        for &face in faces {
            b.face(face)?;
            for edge in face_edges(b, face)? {
                if !all_edges.contains(&edge) {
                    all_edges.push(edge);
                }
            }
            for vertex in face_vertices(b, face)? {
                if !all_vertices.contains(&vertex) {
                    all_vertices.push(vertex);
                }
            }
            delete_face(k, b, Some(&mut *env), face, false, true, None)?;
        }
        for edge in all_edges {
            if crate::topology::edge::is_wire(b, edge)? {
                delete_edge_with(k, b, edge, None, Some(&mut *env))?;
            }
        }
        for vertex in all_vertices {
            if vertex::is_shell_vertex(b, vertex)? {
                delete_vertex(k, b, vertex, Some(&mut *env))?;
            }
        }
        Ok(())
    })();
    // Upstream releases in a destructor; a release failure never hides `result`.
    let released = mark.release(core::slice::from_mut(&mut k.context));
    result.and(released)
}

/// `SmFace::GetEdges(rEdges)` (unit `topology.face_p4`; default arguments).
fn face_edges(b: &Brep, face: FaceId) -> Result<Vec<EdgeId>> {
    crate::topology::face::p4::edges(b, face, None, false, None)
}
/// `SmFace::GetVertices(rVertices)` (unit `topology.face_p4`; default arguments).
fn face_vertices(b: &Brep, face: FaceId) -> Result<Vec<VertexId>> {
    crate::topology::face::p4::vertices(b, face, None, false, None)
}

/// Stitching and orientation options with the native defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct StitchAndOrientOptions {
    /// The raw stitching tolerance; nonpositive values select twice the B-rep tolerance.
    pub stitch_tolerance: f64,
    /// Allows formation of spine edges.
    pub allow_spine_edges: bool,
    /// Optional faces to swap UV after import.
    pub faces_to_swap: Option<Vec<FaceId>>,
    /// Overrides the lamina-count-based decision when present, including `Some(false)`.
    pub just_unify: Option<bool>,
}

impl Default for StitchAndOrientOptions {
    fn default() -> Self {
        Self {
            stitch_tolerance: -0.1,
            allow_spine_edges: false,
            faces_to_swap: None,
            just_unify: None,
        }
    }
}

/// Computes the first stitching tolerance exactly as `StitchAndOrient` does.
/// Only a nonpositive requested tolerance is capped at `100 * SM_ZONE_TOL_3D`.
pub fn initial_stitch_tolerance(brep_tolerance: f64, requested: f64) -> f64 {
    let local = 2.0 * brep_tolerance;
    if requested <= 0.0 {
        if local > 100.0 * ZONE_TOL_3D {
            100.0 * ZONE_TOL_3D
        } else {
            local
        }
    } else {
        requested
    }
}

/// Stitches twice if needed, then orients surfaces. Upstream ignores the stitch and
/// orientation statuses; they are passed on here, as the fork does (60a8a51, WR-658), so
/// a failed first stitch pass returns before the second pass and the orientation.
///
/// The stitch passes run outside an edit of their own: the stitch's glue and squeeze
/// helpers notify through their own kernel-context edits
/// ([`crate::topology::brep::p3::with_kernel_callbacks`]), which the guard refuses
/// inside another edit, and upstream's `SmBrep::StitchFaces` has no edit scope to
/// nest. Only the orientation pass needs the caller's notification environment.
pub fn stitch_and_orient(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
) -> Result<()> {
    stitch_and_orient_with_identity(k, b, options, None)
}

/// [`stitch_and_orient`] with the identity its B-rep cache gives the body's objects: a
/// caller that built the faces under its own topology names (such as
/// [`super::p2::caller_cache_key`] over a notification naming) passes them, so the cache
/// resolves geometry owners under those names; `None` is [`stitch_and_orient`].
///
/// # Errors
///
/// Those of [`stitch_and_orient`].
pub fn stitch_and_orient_with_identity(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
    identity: Option<crate::query::brep::BrepIdentity<'_>>,
) -> Result<()> {
    stitch_and_orient_searching(k, b, options, identity, None, true)
}

/// [`stitch_and_orient_with_identity`] of the faces of one solid, in a B-rep that held
/// nothing else, of which `outside` is a face use on the solid's outside. The glue that
/// closes the solid's shell makes the half of the split region that face use is in the
/// infinite one, where upstream's fires a ray to find it
/// ([`crate::heal::stitch::p1::Stitch::outside`]); the stitch's end searches as ever. Not
/// upstream. The caller answers for the premise, which is why this is not an option of
/// [`StitchAndOrientOptions`]: a B-rep that already holds a face, a wire or a loose
/// vertex has shells the closing glue's search places, and a face use that is not on the
/// outside names the wrong half.
///
/// # Errors
///
/// Those of [`stitch_and_orient`].
pub(crate) fn stitch_and_orient_one_solid(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
    identity: Option<crate::query::brep::BrepIdentity<'_>>,
    outside: FaceuseId,
) -> Result<()> {
    stitch_and_orient_searching(k, b, options, identity, Some(outside), true)
}

/// Not upstream (#18166): the edge tolerances of a B-rep before [`stitch_and_orient`]'s
/// stitch, to find the edges the stitch raised. The stitch covers the faces its joins give an
/// edge when it ends ([`crate::heal::stitch::p1::Stitch::finish_joins`]); the orientation
/// that follows reverses surfaces (`OrientTrimmedSurfaces`), on which a face's proved bound
/// comes out a rounding apart, so each raised edge's covered trimless faces are taken before
/// the orientation ([`crate::topology::edgeuse::p3::covered_trimless_faces`]) and kept
/// covered after it ([`crate::topology::edgeuse::p3::keep_trimless_faces_covered`]). Each
/// edge the stitch joined (it holds more edge uses than before, or is new with two or more)
/// covers its UV trims after the orientation, on the trims the B-rep ends with
/// ([`crate::topology::edgeuse::p3::cover_joined_trims`], #20373).
struct StitchRaises(BTreeMap<EdgeId, (u64, usize)>);

impl StitchRaises {
    /// The tolerance and the number of edge uses of every edge of `b`.
    fn before(b: &Brep) -> Self {
        Self(
            b.edges()
                .iter()
                .map(|(e, rec)| (e, (rec.tolerance.to_bits(), rec.edgeuses.len())))
                .collect(),
        )
    }

    /// The edges of `b` the stitch joined, holding more edge uses than
    /// [`StitchRaises::before`] (a new edge: two or more), whatever their tolerance did, in
    /// the B-rep's edge order: an edge the join raised (`UpdateObjectTolerance`) but no face
    /// bound did covers its trims nowhere else, and one a face bound raised covered them on
    /// the trims before the orientation; covering again only raises where a trim needs it.
    fn joined(&self, b: &Brep) -> Vec<EdgeId> {
        b.edges()
            .iter()
            .filter(|(e, rec)| match self.0.get(e) {
                Some(&(_, uses)) => rec.edgeuses.len() > uses,
                None => rec.edgeuses.len() >= 2,
            })
            .map(|(e, _)| e)
            .collect()
    }

    /// The faces each edge of `b` raised since [`StitchRaises::before`] (or made since)
    /// covers without a trim; empty where they cannot be bounded (left as upstream).
    ///
    /// # Errors
    ///
    /// The kernel's unported and cancelled statuses.
    fn covered(&self, k: &Kernel, b: &Brep) -> Result<Vec<(EdgeId, FaceId)>> {
        let raised: Vec<EdgeId> = b
            .edges()
            .iter()
            .filter(|(e, rec)| {
                self.0
                    .get(e)
                    .is_none_or(|&(was, _)| rec.tolerance > f64::from_bits(was))
            })
            .map(|(e, _)| e)
            .collect();
        Ok(
            crate::error::ignored(crate::topology::edgeuse::p3::covered_trimless_faces(
                k, b, &raised,
            ))?
            .unwrap_or_default(),
        )
    }
}

/// Not upstream (#18166, #20373): [`StitchRaises`]' covers after the orientation: the
/// trimless faces of the raised edges (`covered`), then the UV trims of the joined edges
/// (`joined`).
///
/// # Errors
///
/// The kernel's unported and cancelled statuses.
fn keep_stitch_raises(
    k: &Kernel,
    b: &mut Brep,
    covered: &[(EdgeId, FaceId)],
    joined: &[EdgeId],
) -> Result<()> {
    crate::error::ignored(crate::topology::edgeuse::p3::keep_trimless_faces_covered(
        k, b, covered,
    ))?;
    crate::topology::edgeuse::p3::cover_joined_trims(k, b, joined)
}

/// [`stitch_and_orient_with_identity`] of faces whose lamina edges are the body's open
/// boundary by construction: the first pass only. Upstream runs a second pass, at ten or
/// twenty times the first's tolerance, whenever the first leaves more than one lamina
/// edge, and that pass squeezes out every open edge shorter than its tolerance that is
/// lamina or wire (`squeeze_small_edges`, with `making_manifold_solid` as the primitives
/// stitch). For the walls of an uncapped sweep the lamina edges are the profile's copies
/// at its ends, so the first pass's count says nothing about a gap. There the second pass
/// squeezed out a profile edge between the two tolerances, and its wall's loop was left
/// open by the edge's length in 3D and the wall's whole width in UV. Walls whose edges lie
/// apart by more than the first tolerance stay apart, as an open slit; the second pass
/// glued them without covering the gap, and the check refused the sheet. Not upstream
/// (WR-18515); the caller answers for the premise.
///
/// # Errors
///
/// Those of [`stitch_and_orient`].
pub(crate) fn stitch_and_orient_open(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
    identity: Option<crate::query::brep::BrepIdentity<'_>>,
) -> Result<()> {
    stitch_and_orient_searching(k, b, options, identity, None, false)
}

/// The stitch passes and the orientation; with `second_pass`, a second pass at the larger
/// tolerance when the first leaves more than one lamina edge.
fn stitch_and_orient_searching(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
    identity: Option<crate::query::brep::BrepIdentity<'_>>,
    outside: Option<FaceuseId>,
    second_pass: bool,
) -> Result<()> {
    k.check_cancelled(OP)?;
    let raises = StitchRaises::before(b);
    let mut lamina = 0;
    let mut tolerance = initial_stitch_tolerance(b.tolerance(), options.stitch_tolerance);
    stitch_faces(
        k,
        b,
        tolerance,
        options.allow_spine_edges,
        &mut lamina,
        identity,
        outside,
    )?;
    if second_pass && lamina > 1 {
        tolerance = if options.stitch_tolerance <= 0.0 {
            tolerance * 20.0
        } else {
            tolerance * 10.0
        };
        stitch_faces(
            k,
            b,
            tolerance,
            options.allow_spine_edges,
            &mut lamina,
            identity,
            outside,
        )?;
    }
    let covered = raises.covered(k, b)?;
    let joined = raises.joined(b);
    plain_edit(k, b, |k, b, env| {
        orient_trimmed_surfaces(
            k,
            b,
            Some(env),
            true,
            options.just_unify.unwrap_or(lamina > 0),
            options.faces_to_swap.as_deref(),
        )?;
        Ok(())
    })?;
    keep_stitch_raises(k, b, &covered, &joined)
}

/// [`stitch_and_orient`] inside the caller's edit: the stitch passes and the orientation
/// edit `b` through the caller's environment `env` (the stitch's glues, squeezes and
/// splits as [`crate::heal::stitch::p1::BorrowedStitchEdits`] naming new curves by
/// `curve_identity`), so no edit of their own nests in the caller's. A caller whose edit
/// has lent the kernel context's callbacks uses this: there [`stitch_and_orient`]'s own
/// edits are refused as nested.
///
/// # Errors
///
/// Those of [`stitch_and_orient`].
pub(crate) fn stitch_and_orient_notified(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
    env: &mut EditNotifyEnv<'_>,
    curve_identity: crate::topology::brep::p9::CurveIdentity<'_>,
) -> Result<()> {
    k.check_cancelled(OP)?;
    let raises = StitchRaises::before(b);
    let mut lamina = 0;
    let mut tolerance = initial_stitch_tolerance(b.tolerance(), options.stitch_tolerance);
    stitch_faces_notified(
        k,
        b,
        tolerance,
        options.allow_spine_edges,
        &mut lamina,
        env,
        curve_identity,
    )?;
    if lamina > 1 {
        tolerance = if options.stitch_tolerance <= 0.0 {
            tolerance * 20.0
        } else {
            tolerance * 10.0
        };
        stitch_faces_notified(
            k,
            b,
            tolerance,
            options.allow_spine_edges,
            &mut lamina,
            env,
            curve_identity,
        )?;
    }
    let covered = raises.covered(k, b)?;
    let joined = raises.joined(b);
    orient_trimmed_surfaces(
        k,
        b,
        Some(env),
        true,
        options.just_unify.unwrap_or(lamina > 0),
        options.faces_to_swap.as_deref(),
    )?;
    keep_stitch_raises(k, b, &covered, &joined)
}

/// [`stitch_faces`] editing through `env` ([`stitch_and_orient_notified`]).
fn stitch_faces_notified(
    k: &mut Kernel,
    b: &mut Brep,
    tolerance: f64,
    allow_spine: bool,
    lamina: &mut usize,
    env: &mut EditNotifyEnv<'_>,
    curve_identity: crate::topology::brep::p9::CurveIdentity<'_>,
) -> Result<()> {
    let mut edits = crate::heal::stitch::p1::BorrowedStitchEdits {
        env,
        curve_identity,
    };
    let out = crate::topology::brep::p4::stitch_faces_with_context(
        k,
        b,
        tolerance,
        None,
        allow_spine,
        None,
        Some(&mut edits),
    )?;
    *lamina = usize::try_from(out.lamina_edges)
        .map_err(|_| SolidError::invalid_input(OP, "lamina edge count"))?;
    Ok(())
}

/// `StitchFaces(tol, lNumStitched, lNumLamina, dMaxVGap, dMaxEGap, NULL, bAllowSpine)`:
/// `lamina` receives the remaining lamina edge count; on failure it keeps its value.
fn stitch_faces(
    k: &mut Kernel,
    b: &mut Brep,
    tolerance: f64,
    allow_spine: bool,
    lamina: &mut usize,
    identity: Option<crate::query::brep::BrepIdentity<'_>>,
    outside: Option<FaceuseId>,
) -> Result<()> {
    let out = if let Some(outside) = outside {
        crate::topology::brep::p4::stitch_one_solid_with_identity(
            k,
            b,
            tolerance,
            allow_spine,
            identity,
            outside,
        )?
    } else {
        crate::topology::brep::p4::stitch_faces_with_identity(
            k,
            b,
            tolerance,
            None,
            allow_spine,
            identity,
        )?
    };
    *lamina = usize::try_from(out.lamina_edges)
        .map_err(|_| SolidError::invalid_input(OP, "lamina edge count"))?;
    Ok(())
}

/// [`stitch_faces`] that also returns what the stitch closed: each gap, the
/// entities, and the tolerance growth its closure applied (`solidcore#2975`).
///
/// # Errors
///
/// Those of [`stitch_faces`].
fn stitch_faces_reported(
    k: &mut Kernel,
    b: &mut Brep,
    tolerance: f64,
    allow_spine: bool,
    lamina: &mut usize,
    gaps: &mut Vec<ClosedGap>,
) -> Result<()> {
    let (out, pass_gaps) = crate::topology::brep::p4::stitch_faces_with_gap_report(
        k,
        b,
        tolerance,
        None,
        allow_spine,
        None,
    )?;
    gaps.extend(pass_gaps);
    *lamina = usize::try_from(out.lamina_edges)
        .map_err(|_| SolidError::invalid_input(OP, "lamina edge count"))?;
    Ok(())
}

/// [`stitch_and_orient`] that additionally reports what the stitch closed
/// (`solidcore#2975`): for every gap it closed, the entities it glued, the gap it
/// enclosed, and the tolerance growth its closure applied. Geometry bounds are
/// independent of the stitcher's sampled acceptance test and do not feed back
/// into the construction. A caller decides whether the returned bounds suffice.
/// On any error the B-rep is restored, including when certification fails after
/// earlier closures. Errors are never suppressed by legacy status flags. Not upstream
/// (#20373): the B-rep ends with [`stitch_and_orient`]'s covers after the orientation
/// (`StitchRaises`: the raised edges' trimless faces, the joined edges' trims), which the
/// report, of the closures only, does not list.
///
/// # Errors
///
/// As [`stitch_and_orient`], plus [`SolidError::Unsupported`] for curve kinds
/// without a proved enclosure and [`SolidError::InvalidInput`] for invalid data
/// or a non-finite enclosure. No partial report or partial edit is returned.
pub fn stitch_and_orient_reported(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
) -> Result<Vec<ClosedGap>> {
    k.check_cancelled(OP)?;
    b.with_rollback(
        |b| stitch_and_orient_reported_inner(k, b, options),
        Result::is_ok,
    )
}

fn stitch_and_orient_reported_inner(
    k: &mut Kernel,
    b: &mut Brep,
    options: &StitchAndOrientOptions,
) -> Result<Vec<ClosedGap>> {
    let raises = StitchRaises::before(b);
    let mut lamina = 0;
    let mut gaps = Vec::new();
    let mut tolerance = initial_stitch_tolerance(b.tolerance(), options.stitch_tolerance);
    stitch_faces_reported(
        k,
        b,
        tolerance,
        options.allow_spine_edges,
        &mut lamina,
        &mut gaps,
    )?;
    if lamina > 1 {
        tolerance = if options.stitch_tolerance <= 0.0 {
            tolerance * 20.0
        } else {
            tolerance * 10.0
        };
        stitch_faces_reported(
            k,
            b,
            tolerance,
            options.allow_spine_edges,
            &mut lamina,
            &mut gaps,
        )?;
    }
    let covered = raises.covered(k, b)?;
    let joined = raises.joined(b);
    plain_edit(k, b, |k, b, env| {
        orient_trimmed_surfaces(
            k,
            b,
            Some(env),
            true,
            options.just_unify.unwrap_or(lamina > 0),
            options.faces_to_swap.as_deref(),
        )
    })?;
    keep_stitch_raises(k, b, &covered, &joined)?;
    Ok(gaps)
}

/// `SmBrep::OrientTrimmedSurfaces(bNormalsOutward, bJustUnifyNormals, pFacesToSwapUV)`
/// ([`crate::topology::brep::p3::orient_trimmed_surfaces`]), notifying as
/// [`with_p3_notifications`].
fn orient_trimmed_surfaces(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
    normals_outward: bool,
    just_unify: bool,
    faces_to_swap: Option<&[FaceId]>,
) -> Result<()> {
    with_p3_notifications(k, b, notifications, |k, b, env| {
        let mut marks = crate::topology::topology_traverser::Marks::default();
        crate::topology::brep::p3::orient_trimmed_surfaces(
            k,
            b,
            env,
            &mut marks,
            normals_outward,
            just_unify,
            faces_to_swap,
        )
        .map(drop)
    })
}

/// Output lists of `SqueezeFace`. All are reset even when editing is disabled.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SqueezedFace {
    /// Deleted vertices, as stale identities.
    pub deleted_vertices: Vec<VertexId>,
    /// New edges.
    pub new_edges: Vec<EdgeId>,
    /// Deleted edges, as stale identities.
    pub deleted_edges: Vec<EdgeId>,
}

/// Deletes a degenerate face and stitches its former boundary topology.
pub fn squeeze_face(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    face: FaceId,
    stitch_tolerance: Option<f64>,
    output: &mut SqueezedFace,
) -> Result<()> {
    plain_edit(k, b, |k, b, env| {
        squeeze_face_in(k, b, editing_enabled, face, stitch_tolerance, output, env)
    })
}

/// [`squeeze_face`] under the edit's notification environment ([`plain_edit`]).
pub(crate) fn squeeze_face_in(
    k: &mut Kernel,
    b: &mut Brep,
    editing_enabled: bool,
    face: FaceId,
    stitch_tolerance: Option<f64>,
    output: &mut SqueezedFace,
    env: &mut EditNotifyEnv<'_>,
) -> Result<()> {
    *output = SqueezedFace::default();
    require_editing(editing_enabled)?;
    b.face(face)?;
    // `SmLockBrepBox`: unlocks when it goes out of scope, whatever the result
    let mut lock = crate::topology::brep::p1::LockBrepBox::new(k, b)?;
    let b = lock.brep();
    let result = (|| {
        let tolerance = stitch_tolerance.unwrap_or_else(|| {
            crate::geom::tol::xsect_tol_3d_from_zones(k, b.tolerance(), b.tolerance())
        });
        // DoRegionNesting, FastEdgeCompare, IgnoreProblems, ValidateResult,
        // SqueezeSmallEdges, MakingManifoldSolid, RemoveLaminarSlivers.
        // SplitEdgesWithVertices retains the manager's native default TRUE.
        let mut callback = crate::heal::stitch::p1::DefaultStitchCallback::new();
        let mut stitch =
            crate::heal::stitch::p1::Stitch::new(&mut callback, tolerance, false, false);
        stitch.fast_edge_compare = true;
        stitch.ignore_problems = true;
        stitch.squeeze_small_edges = true;
        stitch.making_manifold_solid = false;
        stitch.remove_laminar_slivers = false;
        let mut vertices: Vec<_> = face_vertices(b, face)?.into_iter().map(Some).collect();
        let mut edges: Vec<_> = face_edges(b, face)?.into_iter().map(Some).collect();
        let (shell1, shell2) = face_shells(b, face)?;
        let nest = (shell_size(b, shell1)? == 1 || shell_size(b, shell2)? == 1) && shell1 != shell2;
        let mut keep_region = None;
        if nest {
            let r1 = b
                .shell(shell1)?
                .region
                .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
            let r2 = b
                .shell(shell2)?
                .region
                .ok_or_else(|| broken_link(SHELL_NO_REGION))?;
            let n1 = region_size(b, r1)?;
            let n2 = region_size(b, r2)?;
            let keep_first = if region_is_infinite(b, r1)? {
                true
            } else if region_is_infinite(b, r2)? {
                false
            } else if n1 > n2 {
                true
            } else if n2 > n1 {
                false
            } else {
                shell_size(b, shell1)? > shell_size(b, shell2)?
            };
            keep_region = Some(if keep_first { r1 } else { r2 });
        }
        delete_face(k, b, Some(&mut *env), face, false, true, keep_region)?;
        // The stitch's glues, squeezes and splits edit `b` within this edit, through its
        // environment. Its plain helpers would each start an edit of their own, which the
        // lent kernel context refuses as nested: with `ignore_problems` the refusal was
        // silent, and a sliver's two edges stayed apart where upstream glues them.
        let mut edits = crate::heal::stitch::p1::BorrowedStitchEdits {
            env: &mut *env,
            curve_identity: crate::topology::brep::p9::CurveIdentity::Actual(
                &crate::topology::brep::p9::issue_curve_identity,
            ),
        };
        stitch.edits = Some(&mut edits);
        // After face deletion, ordinary stitch failures record SM_ERR but do
        // not suppress the remaining cleanup calls or their partially filled outputs.
        let mut failed = None;
        let vlist_opt =
            ignored_keeping(stitch.stitch_vertex_list(k, b, &mut vertices), &mut failed)?;
        let (surviving, deleted_vertices, deleted_edges) = if let Some(vlist) = vlist_opt {
            (
                vlist.surviving.into_iter().map(Some).collect::<Vec<_>>(),
                vlist.deleted,
                vlist.deleted_edges,
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
        edges.retain(|edge| !edge.is_some_and(|e| deleted_edges.contains(&e)));
        output.deleted_edges.extend_from_slice(&deleted_edges);
        vertices.clone_from(&surviving);

        let split_opt = ignored_keeping(
            stitch.split_edges_with_vertices(k, b, &mut vertices, &mut edges),
            &mut failed,
        )?;
        let mut new_edges = if let Some(res) = split_opt {
            res.new_edges
        } else {
            Vec::new()
        };

        let edge_opt = ignored_keeping(
            stitch.stitch_edges_of_vertices(k, b, &surviving),
            &mut failed,
        )?;
        let mut deleted_edges = if let Some(res) = edge_opt {
            res.deleted
        } else {
            Vec::new()
        };

        let temporary: Vec<_> = new_edges
            .iter()
            .copied()
            .filter(|edge| deleted_edges.contains(edge))
            .collect();
        new_edges.retain(|edge| !temporary.contains(edge));
        deleted_edges.retain(|edge| !temporary.contains(edge));
        output.deleted_vertices.extend(deleted_vertices);
        output.deleted_edges.extend(deleted_edges);
        output.new_edges.extend(new_edges);
        match failed {
            Some(e) => Err(caused(TopologyFailure::FaceStitchFailed, Some(e))),
            None => Ok(()),
        }
    })();
    result
}

fn shell_size(b: &Brep, shell: ShellId) -> Result<usize> {
    crate::topology::shell::shell_list_size(b, shell)
}
fn region_size(b: &Brep, region: RegionId) -> Result<usize> {
    Ok(b.region(region)?.shells.len())
}
fn region_is_infinite(b: &Brep, region: RegionId) -> Result<bool> {
    Ok(b.region(region)?.is_infinite_region(b, region))
}

/// Scales about the current origin then applies the placement. The identity exit
/// precedes notifications, geometry conversion and tolerance scaling, even with
/// `scale_tolerances = true`. Nonidentity transformation requires part 2's Notify
/// before any point or geometry is changed; those continuations are uncertified.
pub fn transform(
    k: &mut Kernel,
    b: &mut Brep,
    placement: &Axis2Placement,
    scale: Option<Vec3>,
    scale_tolerances: bool,
) -> Result<()> {
    transform_with_notifications(k, b, placement, scale, scale_tolerances, None)
}

/// Transforms a B-rep using the actual topology notification environment.
pub fn transform_with_notifications(
    k: &mut Kernel,
    b: &mut Brep,
    placement: &Axis2Placement,
    scale: Option<Vec3>,
    scale_tolerances: bool,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    if let Some(env) = notifications {
        transform_in(k, b, placement, scale, scale_tolerances, env)
    } else {
        plain_edit(k, b, |k, b, env| {
            transform_in(k, b, placement, scale, scale_tolerances, env)
        })
    }
}

fn transform_in(
    k: &mut Kernel,
    b: &mut Brep,
    placement: &Axis2Placement,
    scale: Option<Vec3>,
    scale_tolerances: bool,
    env: &mut EditNotifyEnv<'_>,
) -> Result<()> {
    let owner_keys = env.key_of;
    let mut notifications = Some(env);
    if placement.is_identity(crate::math::EFF_ZERO)
        && scale.is_none_or(|s| s.approx_eq(Vec3::new(1.0, 1.0, 1.0)))
    {
        return Ok(());
    }
    crate::error::ignore_status(notify_pre_edit(k, b, notifications.as_deref_mut()))?;
    if scale.is_some_and(|s| {
        !crate::geom::tol::are_same(s.x, s.y)
            || !crate::geom::tol::are_same(s.x, s.z)
            || s.x < crate::math::EFF_ZERO
    }) {
        turn_to_nurbs(k, b, notifications.as_deref_mut())?;
    }
    // #19910: a rigid move rounds every control point it moves, so a tolerance proved to cover
    // a trim can fall short of it after the move by that rounding; the trims as they are
    // before the move, to raise each edge that needs it after
    // ([`crate::topology::edgeuse::p3::cover_moved_trims`])
    let trims_before = if scale.is_none_or(|s| s.approx_eq(Vec3::new(1.0, 1.0, 1.0))) {
        crate::topology::edgeuse::p3::trims_before_move(k, b, &brep_edges(b)?)?
    } else {
        None
    };
    let vertices = brep_vertices(b)?;
    for &vertex in &vertices {
        let mut point = b.vertex(vertex)?.point;
        crate::error::ignore_status(notify_vertex_pre_edit(
            k,
            b,
            vertex,
            notifications.as_deref_mut(),
        ))?;
        if let Some(scale) = scale {
            point.x *= scale.x;
            point.y *= scale.y;
            point.z *= scale.z;
        }
        point = placement.transform_point(point);
        set_vertex_point(k, b, vertex, point, notifications.as_deref_mut())?;
    }
    for edge in crate::topology::brep::p1::curve_owners(b, None, &|e| {
        transform_owner_key(
            b,
            crate::EntityRef::Edge(e),
            owner_keys(crate::EntityRef::Edge(e)),
        )
    })? {
        crate::error::ignore_status(notify_edge_pre_edit(
            k,
            b,
            edge,
            notifications.as_deref_mut(),
        ))?;
        let curve = b.edge_mut(edge)?.curve.as_mut().ok_or_else(|| {
            broken_link(
                "an edge curve_owners listed has no curve, though it lists only edges with \
                     a curve",
            )
        })?;
        curve.transform(k, placement, scale)?;
    }
    let mut rectangular: Vec<_> = brep_faces(b)?.into_iter().map(Some).collect();
    for face in &mut rectangular {
        if let Some(id) = *face {
            if crate::topology::face::p2::rectangular_trim(b, id)? != 1 {
                *face = None;
            }
        }
    }
    for face in crate::topology::brep::p1::surface_owners(b, None, &|f| {
        transform_owner_key(
            b,
            crate::EntityRef::Face(f),
            owner_keys(crate::EntityRef::Face(f)),
        )
    })? {
        crate::error::ignore_status(notify_face_pre_edit(
            k,
            b,
            face,
            notifications.as_deref_mut(),
        ))?;
        let mut not_transformed = None;
        let surface = b
            .face_mut(face)?
            .surface
            .as_mut()
            .ok_or_else(|| broken_link(SURFACE_OWNER_NO_SURFACE))?;
        if ignored_keeping(surface.transform(k, placement, scale), &mut not_transformed)?.is_none()
        {
            let Some(base) = transformed_surface_base(k, surface)? else {
                return Err(caused(
                    TopologyFailure::SurfaceNotTransformable {
                        entity: Some(crate::error::EntityRef::Face(face)),
                    },
                    not_transformed,
                ));
            };
            replace_surface(
                k,
                b,
                face,
                crate::surface::surface::Surface::from(base),
                false,
                None,
                true,
                notifications.as_deref_mut(),
            )?;
        }
    }
    for face in rectangular.into_iter().flatten() {
        b.face_mut(face)?.rectangular_trim = 1;
    }
    if scale.is_some_and(|s| (s.x < 0.0) ^ (s.y < 0.0) ^ (s.z < 0.0)) {
        for edge in brep_edges(b)? {
            reverse_reflection_edgeuses(b, edge)?;
        }
        for face in brep_faces(b)? {
            swap_reflection_shells(b, face)?;
        }
    }
    // #20939: a scale changes every distance, and a scale that is not uniform and positive
    // turns the geometry to NURBS with new parameters, so each edge use's cached largest
    // edge/face gap and gap to its counter-clockwise neighbour (value and parameters) is
    // wrong; they are cleared and measured again when next asked for. The vertex use gaps
    // were cleared as each vertex moved. A placement alone (rotation and translation) keeps
    // distances and parameters, so the caches stay. Upstream keeps them in every case.
    if scale.is_some_and(|s| !s.approx_eq(Vec3::new(1.0, 1.0, 1.0))) {
        for edge in brep_edges(b)? {
            for eu in b.edge(edge)?.edgeuses.clone() {
                let use_ = b.edgeuse_mut(eu)?;
                use_.max_edge_face_gap = crate::heal::gap::EdgeFaceGap::new();
                use_.edge_ccw_edge_gap = crate::heal::gap::EdgeEdgeGap::new();
            }
        }
    }
    if let Some(before) = trims_before {
        crate::topology::edgeuse::p3::cover_moved_trims(k, b, &before, placement.matrix())?;
    }
    // WR-507 (#1520): a scale of 1 (a rotation or reflection) is no scale, and a scale s
    // keeps each tolerance at least its own times s; upstream sets every one to the body
    // tolerance whenever a scale is given.
    let tol_scale = scale.map_or(1.0, |s| max2(s.x.abs(), max2(s.y.abs(), s.z.abs())));
    if scale_tolerances && (tol_scale - 1.0).abs() > crate::math::EFF_ZERO {
        // WR-508 (#1521): the size is the scaled body's bounding box, so a body without
        // vertices (a lone torus) is not sized from an empty box; upstream sizes it from
        // its vertices alone.
        let model_box = crate::query::brep::calculate_bounding_box(
            k,
            &mut *b,
            crate::query::brep::BoundingBoxOptions {
                approx: false,
                tight: false,
                expand_by_zone_tol_3d: false,
            },
        )?;
        let size = model_box.size().length();
        // (the scaled body's size is its own: upstream also stores it on the body's
        // context, which here is every body's default)
        let zone = b.set_this_model_size_estimate(size);
        for vertex in vertices {
            let tol = max2(zone, vertex::tolerance(b, vertex)? * tol_scale);
            crate::error::ignore_status(vertex::set_tolerance(b, vertex, tol, false, false))?;
        }
        for edge in brep_edges(b)? {
            let tol = max2(zone, crate::topology::edge::tolerance(b, edge)? * tol_scale);
            crate::error::ignore_status(crate::topology::edge::set_tolerance(
                b, edge, tol, false, false,
            ))?;
        }
        for face in brep_faces(b)? {
            let tol = max2(
                zone,
                crate::topology::face::p2::tolerance(b, face)? * tol_scale,
            );
            set_face_tolerance(b, face, tol, false, false)?;
        }
    }
    Ok(())
}

/// `SmBrep::TurnToNURBS()` as a plain entry point (the caller keeps no notification
/// state of its own: `plain_edit`), with the geometry owners [`transform`]'s
/// conversion uses.
///
/// # Errors
///
/// Those of [`crate::topology::brep::p3::turn_to_nurbs`]; `InvalidInput` for a call
/// inside another lent edit (before anything changes).
pub fn turn_to_nurbs_plain(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    turn_to_nurbs(k, b, None)
}

/// `SmBrep::TurnToNURBS()` ([`crate::topology::brep::p3::turn_to_nurbs`]), notifying as
/// [`with_p3_notifications`].
fn turn_to_nurbs(
    k: &mut Kernel,
    b: &mut Brep,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    with_p3_notifications(k, b, notifications, |k, b, env| {
        let key_of = env.key_of;
        let mut selection = BTreeMap::new();
        for entity in brep_faces(b)?
            .into_iter()
            .map(crate::EntityRef::Face)
            .chain(brep_edges(b)?.into_iter().map(crate::EntityRef::Edge))
        {
            selection.insert(entity, transform_owner_key(b, entity, key_of(entity)));
        }
        crate::topology::brep::p3::turn_to_nurbs_with_geometry_owners(k, b, env, &|entity| {
            selection
                .get(&entity)
                .copied()
                .unwrap_or_else(|| key_of(entity))
        })
    })
}
pub(super) fn notify_edge_pre_edit(
    k: &mut Kernel,
    b: &mut Brep,
    edge: EdgeId,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    let brep = ObjectRef {
        type_code: crate::geom::a_object::BREP_TYPE,
        key: b.key(),
    };
    with_notifications(
        b,
        notifications,
        Some(&mut k.context),
        |b, env, fallback| {
            let this = (env.key_of)(crate::error::EntityRef::Edge(edge));
            let mut forwarded = crate::topology::edge::NotifyEnv {
                records: env.records,
                attributes: env.attributes,
                context: env.context.as_deref_mut().or(fallback),
                key_of: env.key_of,
            };
            crate::topology::edge::notify(
                b,
                edge,
                &mut forwarded,
                NotifyOperation::PreEdit,
                [Some(this), Some(brep), None],
            )
        },
    )
}

// Geometry may retain a constructor's caller keys while plain-edit or temporary
// transform notifications use distinct identities. Match only a proven producer
// key or the issued notification identity; foreign and unowned geometry keep the
// native filter.
pub(super) fn transform_owner_key(
    b: &Brep,
    entity: crate::EntityRef,
    kernel_key: ObjectRef,
) -> ObjectRef {
    let owner = match entity {
        crate::EntityRef::Edge(edge) => b
            .edges()
            .get(edge)
            .and_then(|e| e.curve.as_ref())
            .and_then(Curve::owner),
        crate::EntityRef::Face(face) => b
            .faces()
            .get(face)
            .and_then(|f| f.surface.as_ref())
            .and_then(crate::surface::surface::Surface::owner),
        _ => None,
    };
    if owner == Some(kernel_key) {
        kernel_key
    } else {
        b.geometry_owner_key(entity).unwrap_or(kernel_key)
    }
}

fn transformed_surface_base(
    k: &Kernel,
    surface: &mut crate::surface::surface::Surface,
) -> Result<Option<crate::nurbs::b_spline_surface::BSplineSurface>> {
    use crate::surface::surface::SurfaceKind;
    Ok(match surface.kind_mut() {
        SurfaceKind::BSpline(base) => Some(base.clone()),
        SurfaceKind::ProceduralBlend(pb) => {
            let base = pb.net();
            Some(base.clone())
        }
        SurfaceKind::BallPatch(bp) => Some(bp.net().clone()),
        SurfaceKind::Plane(plane) => {
            if plane.base().gw_nurb().is_none() {
                crate::error::ignore_status(plane.make_nurb())?;
            }
            Some(plane.base().clone())
        }
        SurfaceKind::SurfOfRevolution(revolution) => {
            if revolution.base().gw_nurb().is_none() {
                crate::error::ignore_status(revolution.make_nurb(k))?;
            }
            Some(revolution.base().clone())
        }
        SurfaceKind::Cone(cone) => {
            if cone.rev().base().gw_nurb().is_none() {
                crate::error::ignore_status(cone.make_nurb(k))?;
            }
            Some(cone.rev().base().clone())
        }
        SurfaceKind::Sphere(sphere) => {
            if sphere.base().base().gw_nurb().is_none() {
                crate::error::ignore_status(sphere.make_nurb(k))?;
            }
            Some(sphere.base().base().clone())
        }
        SurfaceKind::Torus(torus) => {
            if torus.base().base().gw_nurb().is_none() {
                crate::error::ignore_status(torus.make_nurb(k))?;
            }
            Some(torus.base().base().clone())
        }
        SurfaceKind::SurfOfExtrusion(extrusion) => {
            if extrusion.base().gw_nurb().is_none() {
                crate::error::ignore_status(extrusion.make_nurb(k))?;
            }
            Some(extrusion.base().clone())
        }
        SurfaceKind::CurveBounded(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::OffsetSurface(_)
        | SurfaceKind::SrfInVolume(_) => None,
    })
}
/// `SER(ReplaceSurface(pFace, pNewSurf, bAddToDeletedList, pOptFacesModified,
/// bCreateUVTrimCurves))` ([`brep_p2::replace_surface`]); the face is notified under
/// the caller's identity for it.
#[allow(clippy::too_many_arguments)] // one upstream call
pub(super) fn replace_surface(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    surface: crate::surface::surface::Surface,
    add_to_deleted: bool,
    mut modified: Option<&mut Vec<FaceId>>,
    create_uv_trims: bool,
    notifications: Option<&mut EditNotifyEnv<'_>>,
) -> Result<()> {
    // init output
    if let Some(modified) = modified.as_deref_mut() {
        modified.clear();
    }
    let old = if add_to_deleted {
        brep_p2::OldSurface::AddToDeletedList
    } else {
        brep_p2::OldSurface::Delete
    };
    // The kernel is held immutably here (`SmSurface` evaluation), so an arena-only
    // caller's notifications reach no context callbacks.
    let (_, faces) = with_notifications(b, notifications, None, |b, env, _| {
        let face_ref = (env.key_of)(crate::error::EntityRef::Face(face));
        brep_p2::replace_surface(k, b, env, face, surface, face_ref, old, create_uv_trims)
    })?;
    if let Some(modified) = modified {
        modified.extend(faces);
    }
    Ok(())
}
fn reverse_reflection_edgeuses(b: &mut Brep, edge: EdgeId) -> Result<()> {
    crate::topology::brep::p1::reverse_reflection_edgeuses(b, edge)
}
fn swap_reflection_shells(b: &mut Brep, face: FaceId) -> Result<()> {
    crate::topology::brep::p1::swap_reflection_shells(b, face)
}
fn set_face_tolerance(
    b: &mut Brep,
    face: FaceId,
    tolerance: f64,
    only_larger: bool,
    cascade: bool,
) -> Result<()> {
    crate::topology::face::p3::set_tolerance(b, face, tolerance, only_larger, cascade)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Vertex;

    /// A closed 1 x 2 x 3 box (stitched, no lamina edges).
    fn status_box(k: &mut Kernel) -> Brep {
        crate::primitive::api_primitives::create_box(k, Point3::new(0.0, 0.0, 0.0), 1.0, 2.0, 3.0)
            .expect("box")
    }

    /// Two straight wire edges meet exactly at the origin. Extending the longer one along
    /// the x axis misses the shorter one's far end (1, gap, 0) by at least `gap`. The
    /// middle vertex allows the join; the surviving edge and far vertex have their own
    /// budgets, respectively the gap and twice the gap.
    #[test]
    fn a_targeted_line_join_keeps_the_existing_edge_and_vertex_tolerances() {
        use crate::curve::line::Line;
        use crate::geom::attribute::{AttributeStore, ObjectKey};
        use crate::topology::api_files::write_brep_to_file;
        use crate::topology::brep_data_write::FromBrepEnv;
        use crate::topology::edge;
        use crate::topology::topology::NotifyRecords;

        let bytes = |k: &Kernel, b: &Brep| {
            let env = FromBrepEnv {
                attributes: &AttributeStore::default(),
                records: &NotifyRecords::new(),
                key_of: &crate::topology::loopuse::notify_ref,
                curve_key: &|_| ObjectKey(0),
                uv_curve_key: &|_| ObjectKey(0),
                surface_key: &|_| ObjectKey(0),
            };
            write_brep_to_file(k, Some(b), &env, false).unwrap()
        };
        let gap = 1e-4;
        for (targeted, edge_budget, vertex_budget, joins) in [
            (true, 0.5 * gap, 3.0 * gap, false),
            (true, 2.0 * gap, 1.5 * gap, false),
            (true, 2.0 * gap, 3.0 * gap, true),
            (false, 0.5 * gap, 1.5 * gap, true),
        ] {
            let mut k = Kernel::default();
            let mut b = Brep::new_with_infinite_region().unwrap();
            let line = |a, z| {
                let mut line = Line::from_points(a, z, 3);
                line.ensure_nurb(None).unwrap();
                Curve::from(line)
            };
            let long = line(Point3::new(-3.0, 0.0, 0.0), Point3::ZERO);
            let interval = long.natural_interval();
            let survivor = create_wire_edge_from_curve(&mut k, &mut b, long, interval).unwrap();
            let middle = edge::end_vertex(&b, survivor).unwrap().unwrap();
            let far = Point3::new(1.0, gap, 0.0);
            let short = line(Point3::ZERO, far);
            let short_interval = short.natural_interval();
            let region = b.infinite_region().unwrap();
            let (other, end) = plain_edit(&mut k, &mut b, |k, b, env| {
                make_wire_edge_vertex(
                    k,
                    b,
                    env,
                    region,
                    middle,
                    short,
                    short_interval,
                    OrientType::Same,
                    far,
                )
            })
            .unwrap();
            edge::set_tolerance(&mut b, survivor, edge_budget, false, true).unwrap();
            vertex::set_tolerance(&mut b, middle, 10.0 * gap, false, false).unwrap();
            vertex::set_tolerance(&mut b, end, vertex_budget, false, false).unwrap();
            let original = bytes(&k, &b);

            if targeted {
                remove_topological_joins(&mut k, &mut b, &[], &[middle]).unwrap();
            } else {
                remove_topological_edges_and_vertices(&mut k, &mut b, None, None).unwrap();
            }
            assert_eq!(b.vertex(middle).is_err(), joins);
            assert_eq!(b.edge(other).is_err(), joins);
            if targeted {
                assert_eq!(b.edge(survivor).unwrap().tolerance, edge_budget);
                assert_eq!(b.vertex(end).unwrap().tolerance, vertex_budget);
            } else {
                assert!(b.edge(survivor).unwrap().tolerance >= gap);
                assert!(b.vertex(end).unwrap().tolerance >= 2.0 * gap);
            }
            if !joins {
                assert_eq!(bytes(&k, &b), original);
            }
        }
    }

    /// #20373: an edge the join itself raised (`UpdateObjectTolerance`), with
    /// more edge uses than before the stitch, is one [`StitchRaises::joined`] gives, and
    /// [`keep_stitch_raises`] (which the stitch runs after its orientation) covers its UV
    /// trims and its vertices, though no face bound raised it. One of the box's line edges has a trim
    /// moved `1e-4` off it in UV and its tolerance raised from the box's to twice that (as a
    /// join raises it), below the moved trim's gap: the cover raises it over the gap. A
    /// filter keeping only the joined edges whose tolerance did not rise left it uncovered.
    #[test]
    fn an_edge_the_join_raised_covers_its_trims() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW, NOZ};
        let mut k = Kernel::default();
        let mut b = status_box(&mut k);
        let (e, x) = b
            .edge_list()
            .iter()
            .find_map(|&e| {
                let uses = &b.edge(e).unwrap().edgeuses;
                let trimmed: Vec<crate::topology::EdgeuseId> = uses
                    .iter()
                    .copied()
                    .filter(|&x| b.edgeuse(x).unwrap().uv_trim_curve.is_some())
                    .collect();
                (trimmed.len() >= 2).then(|| (e, trimmed[0]))
            })
            .expect("an edge trimmed on two faces");
        let (iv, was) = {
            let rec = b.edge(e).unwrap();
            (rec.interval, rec.tolerance)
        };
        let [a, z] = [iv.min(), iv.max()].map(|t| {
            b.edgeuse(x)
                .unwrap()
                .uv_trim_curve
                .as_ref()
                .unwrap()
                .evaluate_point(&k, t)
                .unwrap()
        });
        // Moved square to the trim, toward the middle of the plane's parameter box (a trim
        // off the box is not bounded, and is left as upstream leaves it).
        let f = crate::topology::edgeuse::p1::face(&b, x).unwrap().unwrap();
        let surface = b.face(f).unwrap().surface.clone().unwrap();
        let bx = surface.natural_uv_domain_if_built();
        let (mx, my) = (0.5 * (bx.min.x + bx.max.x), 0.5 * (bx.min.y + bx.max.y));
        let (nx, ny) = (a.y - z.y, z.x - a.x);
        let len = (nx * nx + ny * ny).sqrt();
        let toward = if (mx - a.x) * nx + (my - a.y) * ny >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let (dx, dy) = (toward * 1e-4 * nx / len, toward * 1e-4 * ny / len);
        let pw = vec![
            CPoint::new(a.x + dx, a.y + dy, NOZ, NOW),
            CPoint::new(z.x + dx, z.y + dy, NOZ, NOW),
        ];
        let nl = NlCurve::new(pw, 1, vec![iv.min(), iv.min(), iv.max(), iv.max()]).unwrap();
        let moved = Curve::from(BSplineCurve::from_nurb(nl, 2, false));
        // The moved trim's gap from the edge, sampled.
        let curve = b.edge(e).unwrap().curve.clone().unwrap();
        let gap = (0..=16)
            .map(|i| {
                let t = iv.evaluate(f64::from(i) / 16.0);
                let w = moved.evaluate_point(&k, t).unwrap();
                let on = surface
                    .evaluate_point(&k, crate::math::Point2::new(w.x, w.y))
                    .unwrap();
                curve.evaluate_point(&k, t).unwrap().distance(on)
            })
            .fold(0.0_f64, f64::max);
        b.edgeuse_mut(x).unwrap().uv_trim_curve = Some(moved);
        // The stitch's record: every other edge as it is, this one at the tolerance before
        // and with one use fewer; then the join's raise.
        let uses = b.edge(e).unwrap().edgeuses.len();
        let mut raises = StitchRaises::before(&b);
        raises.0.insert(e, (was.to_bits(), uses - 1));
        let raised = 2.0 * was;
        assert!(
            raised < gap,
            "the join's raise {raised:e} under the gap {gap:e}"
        );
        b.edge_mut(e).unwrap().tolerance = raised;
        let joined = raises.joined(&b);
        assert_eq!(joined, vec![e]);
        keep_stitch_raises(&k, &mut b, &[], &joined).unwrap();
        let tol = b.edge(e).unwrap().tolerance;
        assert!(tol >= gap, "{tol:e} covers the trim's gap {gap:e}");
        assert!(tol <= 2.0 * gap, "{tol:e} near the gap {gap:e}");
        // (`cover_vertex_ends`: a vertex is kept at least its edges' tolerance)
        for v in [
            crate::topology::edge::start_vertex(&b, e).unwrap().unwrap(),
            crate::topology::edge::end_vertex(&b, e).unwrap().unwrap(),
        ] {
            let vt = b.vertex(v).unwrap().tolerance;
            assert!(vt >= tol, "vertex {} at {vt:e} under {tol:e}", v.index());
        }
    }

    /// #15718: when both ends of a joined line edge grow, each is judged against the
    /// other's grown tolerance. The box's 3-long edge on x = y = 0 has both vertices moved
    /// 0.8 off it, away from the box: each needs a tolerance of 1.6 to cover its gap, which
    /// fits beside the other's old tolerance (1.6 < 3) and beside its other neighbours
    /// (1.66 and 2.63 away), but the two grown balls overlap (1.6 + 1.6 > 3). The cover is
    /// refused at the first end and grows nothing.
    #[test]
    fn a_joined_edge_whose_two_grown_ends_overlap_is_refused() {
        let mut k = Kernel::default();
        let mut b = status_box(&mut k);
        let mut found = None;
        for &e in b.edge_list() {
            let ed = b.edge(e).expect("edge");
            let c = ed.curve.as_ref().expect("curve");
            let p = c.evaluate_point(&k, ed.interval.min()).expect("start");
            let q = c.evaluate_point(&k, ed.interval.max()).expect("end");
            if p.x == 0.0 && p.y == 0.0 && q.x == 0.0 && q.y == 0.0 {
                found = Some(e);
            }
        }
        let e = found.expect("the edge on x = y = 0");
        let ends = [
            crate::topology::edge::start_vertex(&b, e).unwrap().unwrap(),
            crate::topology::edge::end_vertex(&b, e).unwrap().unwrap(),
        ];
        let off = 0.8 / std::f64::consts::SQRT_2;
        let before: Vec<f64> = ends
            .iter()
            .map(|&v| {
                let vx = b.vertex_mut(v).unwrap();
                vx.point = Point3::new(-off, -off, vx.point.z);
                vx.tolerance
            })
            .collect();
        let r = cover_joined_line_gaps(&k, &mut b, e);
        let Err(SolidError::Failed(f)) = &r else {
            panic!("a refusal, got {r:?}");
        };
        let crate::error::Reason::Topology(TopologyFailure::JoinedVertexReaches {
            needed,
            fits_below,
        }) = f.reason
        else {
            panic!("a joined vertex that reaches, got {r:?}");
        };
        assert!((needed - 1.6).abs() < 1e-9, "needed {needed}");
        let fits = fits_below.expect("a tolerance fits");
        assert!(fits > 1.3 && fits < 1.5, "fits below {fits}");
        for (&v, t) in ends.iter().zip(before) {
            assert_eq!(b.vertex(v).unwrap().tolerance, t, "nothing grown");
        }
    }

    /// fork 60a8a51 (`SmBrep::StitchAndOrient`, `SER(this->StitchFaces(..))`): an edge
    /// without curve fails the first stitch pass, and the stitch's error reaches the
    /// caller (upstream drops it and reports success).
    #[test]
    fn stitch_and_orient_passes_on_a_failed_stitch() {
        let mut k = Kernel::default();
        let mut b = status_box(&mut k);
        let e = *b.edge_list().first().expect("edge");
        b.edge_mut(e).expect("edge").curve = None;
        let r = stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default());
        assert!(
            matches!(
                r,
                Err(SolidError::InvalidInput {
                    operation: "heal::stitch::p2",
                    ..
                })
            ),
            "{r:?}"
        );
    }

    /// fork 60a8a51 (`SmBrep::FindInfiniteRegion`, `SER(CalculateBoundingBox(sBBox))`,
    /// reached from the stitch pass of `SmBrep::StitchAndOrient`): a face without
    /// surface fails the B-rep's box, and the box's error reaches the caller (upstream's
    /// region search goes on with an empty box, its ray finds nothing (`SM_ERR`) and
    /// `StitchAndOrient` drops that).
    #[test]
    fn stitch_and_orient_passes_on_a_failed_region_box() {
        let mut k = Kernel::default();
        let mut b = status_box(&mut k);
        let f = *b.face_list().first().expect("face");
        b.face_mut(f).expect("face").surface = None;
        let mut lamina = 0;
        let tolerance = initial_stitch_tolerance(b.tolerance(), 0.0);
        let stitched = stitch_faces(
            &mut k,
            &mut b.clone_standing_for_self().unwrap(),
            tolerance,
            false,
            &mut lamina,
            None,
            None,
        );
        let r = stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default());
        let box_error = |r: &Result<()>| {
            matches!(
                r,
                Err(SolidError::InvalidInput {
                    operation: "query::face::p1::calculate_bounding_box",
                    ..
                })
            )
        };
        assert!(box_error(&stitched), "{stitched:?}");
        assert!(box_error(&r), "{r:?}");
    }

    /// `MakeEdgeInFace` names the end whose vertex sector it did not find, and a missing
    /// face, keeping the null-pointer status and the no-sector operation.
    #[test]
    fn a_missing_sector_names_its_end() {
        use crate::error::SM_ERR_NULL_POINTER;
        struct Backend {
            calls: usize,
            miss: usize,
        }
        impl FaceEdgeBackend for Backend {
            type Face = ();
            type Vertex = ();
            type Curve = ();
            type UvCurve = ();
            type Vertexuse = ();
            type Output = ();
            fn validate_face(&self, (): ()) -> Result<()> {
                Ok(())
            }
            fn tolerance(&self) -> Result<f64> {
                Ok(1e-6)
            }
            fn sector(
                &mut self,
                (): (),
                _vertex: Option<()>,
                (): &(),
                _uv: Option<&()>,
                _start: f64,
                _end: f64,
                _orientation: OrientType,
                _tolerance: f64,
                _all: bool,
            ) -> Result<Option<()>> {
                self.calls += 1;
                Ok((self.calls != self.miss).then_some(()))
            }
            fn insert(
                &mut self,
                (): (),
                (): (),
                (): (),
                (): (),
                _uv: Option<()>,
                _interval: Extent1d,
                _orientation: OrientType,
                _max_gap: f64,
                (): &mut (),
            ) -> Result<()> {
                Ok(())
            }
        }
        let run = |face: Option<()>, miss: usize| {
            let mut backend = Backend { calls: 0, miss };
            make_edge_with_backend(
                &mut backend,
                true,
                face,
                Some(()),
                Some(()),
                (),
                None,
                Extent1d::from_min_max(0.0, 1.0),
                OrientType::Same,
                1e-7,
                &mut (),
            )
        };
        for (miss, at_end) in [(1, false), (2, true)] {
            let e = run(Some(()), miss).unwrap_err();
            assert_eq!(
                e.topology_failure(),
                Some(&TopologyFailure::NoVertexSector { at_end })
            );
            assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
            assert!(is_no_vertex_sector(&e), "{e}");
        }
        run(Some(()), 0).unwrap();
        let e = run(None, 0).unwrap_err();
        assert_eq!(e.topology_failure(), Some(&TopologyFailure::NoFaceGiven));
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert!(!is_no_vertex_sector(&e));
    }

    #[test]
    fn supplied_uv_reaches_both_sectors_and_insertion() {
        struct Backend {
            sectors: std::cell::RefCell<Vec<(f64, f64, OrientType)>>,
        }
        impl FaceEdgeBackend for Backend {
            type Face = ();
            type Vertex = ();
            type Curve = ();
            type UvCurve = String;
            type Vertexuse = ();
            type Output = Option<String>;
            fn validate_face(&self, (): ()) -> Result<()> {
                Ok(())
            }
            fn tolerance(&self) -> Result<f64> {
                Ok(1e-6)
            }
            fn sector(
                &mut self,
                (): (),
                _vertex: Option<()>,
                (): &(),
                uv: Option<&String>,
                start: f64,
                end: f64,
                orientation: OrientType,
                tolerance: f64,
                all: bool,
            ) -> Result<Option<()>> {
                assert_eq!(uv.map(String::as_str), Some("trace-489"));
                assert_eq!(tolerance, 1e-6);
                assert!(!all);
                self.sectors.borrow_mut().push((start, end, orientation));
                Ok(Some(()))
            }
            fn insert(
                &mut self,
                (): (),
                (): (),
                (): (),
                (): (),
                uv: Option<String>,
                interval: Extent1d,
                _orientation: OrientType,
                max_gap: f64,
                output: &mut Self::Output,
            ) -> Result<()> {
                assert_eq!(interval, Extent1d::from_min_max(-3.0, 7.0));
                assert_eq!(max_gap, 2e-7);
                *output = uv;
                Ok(())
            }
        }
        for orientation in [OrientType::Same, OrientType::Opposite] {
            let mut backend = Backend {
                sectors: std::cell::RefCell::new(Vec::new()),
            };
            let mut output = None;
            make_edge_with_backend(
                &mut backend,
                true,
                Some(()),
                Some(()),
                Some(()),
                (),
                Some(String::from("trace-489")),
                Extent1d::from_min_max(-3.0, 7.0),
                orientation,
                2e-7,
                &mut output,
            )
            .unwrap();
            assert_eq!(output.as_deref(), Some("trace-489"));
            let expected = if orientation == OrientType::Same {
                vec![
                    (-3.0, 7.0, OrientType::Same),
                    (7.0, -3.0, OrientType::Opposite),
                ]
            } else {
                vec![
                    (7.0, -3.0, OrientType::Opposite),
                    (-3.0, 7.0, OrientType::Same),
                ]
            };
            assert_eq!(*backend.sectors.borrow(), expected);
        }
    }

    /// The caller's argument and body causes, reached through the public entry points,
    /// keep upstream's statuses.
    #[test]
    fn the_callers_input_is_refused_by_cause_with_upstreams_status() {
        use crate::error::{SM_ERR, SM_ERR_NULL_POINTER};
        let status = |e: &SolidError| (e.topology_failure().copied(), e.upstream_status());
        let mut k = Kernel::default();

        // no regions to keep given, and a B-rep with no finite region
        let mut b = Brep::new();
        let e = make_manifold(&mut k, &mut b, Some(&[]), false).unwrap_err();
        assert_eq!(
            status(&e),
            (Some(TopologyFailure::NoRegionsToKeepGiven), Some(SM_ERR))
        );
        let e = make_manifold(&mut k, &mut b, None, false).unwrap_err();
        assert_eq!(
            status(&e),
            (Some(TopologyFailure::NoFiniteRegion), Some(SM_ERR))
        );

        // a B-rep without an infinite region: the null-pointer status
        let mut b = Brep::new();
        assert!(b.infinite_region().is_none());
        let line = Curve::from(
            BSplineCurve::create_line_segment(3, Point3::ZERO, Point3::new(1.0, 0.0, 0.0)).unwrap(),
        );
        let interval = line.natural_interval();
        let e = create_wire_edge_from_curve(&mut k, &mut b, line, interval).unwrap_err();
        assert_eq!(
            status(&e),
            (
                Some(TopologyFailure::NoInfiniteRegion),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);

        // no vertex given, and a vertex on no edge
        let mut b = Brep::new();
        let v = b.add_vertex_record(Vertex::default()).unwrap();
        let e = glue_vertices(&mut k, &mut b, Some(v), None, None).unwrap_err();
        assert_eq!(
            status(&e),
            (
                Some(TopologyFailure::NoVertexGiven),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        let e = delete_topological_vertex(&mut k, &mut b, true, None, None).unwrap_err();
        assert_eq!(
            status(&e),
            (
                Some(TopologyFailure::NoVertexGiven),
                Some(SM_ERR_NULL_POINTER)
            )
        );
        let e = delete_topological_vertex(&mut k, &mut b, true, Some(v), None).unwrap_err();
        assert_eq!(
            status(&e),
            (
                Some(TopologyFailure::VertexNotOnTwoEdges { edges: 0 }),
                Some(SM_ERR)
            )
        );
        let e = delete_topological_vertex(&mut k, &mut b, false, Some(v), None).unwrap_err();
        assert_eq!(
            status(&e),
            (Some(TopologyFailure::EditingDisabled), Some(SM_ERR))
        );
    }

    #[test]
    fn squeeze_cleanup_continues_after_typed_native_failures() {
        let errors = [
            SolidError::sm_err("SmStitch::StitchVertexPairs"),
            SolidError::NotFound {
                operation: "SmStitch::SplitEdgesWithVertices",
                entity: crate::error::EntityRef::Edge(EdgeId::from_index(2)),
            },
            SolidError::Degenerate {
                operation: "SmStitch::StitchEdgesOfVertices",
                entity: None,
            },
        ];
        // every native stitch failure is kept, the first one as the cause, and the
        // cleanup goes on; an unported callee still propagates
        let first = errors[0].clone();
        let mut failed = None;
        for error in errors {
            assert_eq!(ignored_keeping(Err::<(), _>(error), &mut failed), Ok(None));
            assert_eq!(failed.as_ref(), Some(&first));
        }
        assert_eq!(ignored_keeping(Ok(()), &mut failed), Ok(Some(())));
        assert_eq!(failed.as_ref(), Some(&first));
        let e = caused(TopologyFailure::FaceStitchFailed, failed);
        assert_eq!(e.cause(), Some(&first));
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(
            e.topology_failure(),
            Some(&TopologyFailure::FaceStitchFailed)
        );

        let mut failed = None;
        let missing = SolidError::unported("SmStitch::StitchVertexPairs", "heal.stitch_p2");
        assert_eq!(
            ignored_keeping(Err::<(), _>(missing.clone()), &mut failed),
            Err(missing)
        );
        assert!(failed.is_none());
        // with nothing kept, the failure has no cause
        assert_eq!(caused(TopologyFailure::CurvesNotJoined, None).cause(), None);
    }

    #[test]
    fn failure_outputs_follow_native_lifecycle() {
        let mut b = Brep::new();
        let v = b.add_vertex_record(Vertex::default()).unwrap();
        let mut edge = Some(EdgeId::from_index(3));
        // With `SmBrep::DeleteVertex` (topology.brep_p4) and `Notify` ported, the
        // isolated-vertex branch runs; upstream ignores DeleteVertex's status and the
        // fork passes it on (WR-658, tangency-labs/smlib 60a8a51), here its refusal on a
        // B-rep whose editing is disabled. The deleted-edge output is still reset first.
        let r = combine_coincident_vertices(&mut Kernel::default(), &mut b, v, v, &mut edge);
        assert!(
            matches!(&r, Err(e) if e.sm_status() == Some(1001)
                && e.to_string().contains("editing is not enabled")),
            "{r:?}"
        );
        assert!(edge.is_none());
        let mut out = SqueezedFace {
            deleted_vertices: vec![v],
            new_edges: vec![EdgeId::from_index(1)],
            deleted_edges: vec![EdgeId::from_index(2)],
        };
        assert!(squeeze_face(
            &mut Kernel::default(),
            &mut b,
            false,
            FaceId::from_index(0),
            None,
            &mut out
        )
        .is_err());
        assert_eq!(out, SqueezedFace::default());
    }

    #[test]
    fn identity_precedes_tolerance_update_and_notifications() {
        let mut b = Brep::new();
        let mut k = Kernel::default();
        let v = b
            .add_vertex_record(Vertex {
                point: Vec3::new(1., 2., 3.),
                tolerance: 42.,
                ..Vertex::default()
            })
            .unwrap();
        let original = b.vertex(v).unwrap().clone();
        transform(&mut k, &mut b, &Axis2Placement::IDENTITY, None, true).unwrap();
        assert_eq!(b.vertex(v).unwrap(), &original);
        assert_eq!(b.vertices().len(), 1);
        assert!(remove_faces(&mut k, &mut b, true, &[]).is_ok());
        assert!(remove_faces(&mut k, &mut b, false, &[]).is_err());
    }

    /// A 10 x 20 x 30 box with the first vertex, the first face and the first edge at
    /// `1e-3`, and the second edge at `1e-7` (WR-507, #1520).
    fn box_with_tolerances(k: &mut Kernel) -> (Brep, [EdgeId; 2], VertexId, FaceId) {
        let mut b = crate::primitive::api_primitives::create_box(k, Point3::ZERO, 10.0, 20.0, 30.0)
            .unwrap();
        let edges = brep_edges(&b).unwrap();
        let (wide, narrow) = (edges[0], edges[1]);
        let v = brep_vertices(&b).unwrap()[0];
        let f = brep_faces(&b).unwrap()[0];
        crate::topology::edge::set_tolerance(&mut b, wide, 1e-3, false, false).unwrap();
        crate::topology::edge::set_tolerance(&mut b, narrow, 1e-7, false, false).unwrap();
        vertex::set_tolerance(&mut b, v, 1e-3, false, false).unwrap();
        set_face_tolerance(&mut b, f, 1e-3, false, false).unwrap();
        (b, [wide, narrow], v, f)
    }

    /// Relative comparison: `got` must equal `want` to 1e-12 of `want`.
    fn assert_tol(what: &str, got: f64, want: f64) {
        assert!(
            (got - want).abs() <= 1e-12 * want,
            "{what}: tolerance {got:e}, want {want:e}"
        );
    }

    /// WR-507 (#1520): the 4x4 overload of `SmBrep::Transform` passes a unit scale, so a
    /// mirror (scale `(-1, 1, 1)`) or a rotation (unit scale) must keep a wider edge
    /// tolerance and a scale of 2 must double it. Upstream resets it to the body's.
    #[test]
    fn transform_scales_each_tolerance_wr507() {
        let (s, c) = (0.5_f64.sqrt(), 0.5_f64.sqrt());
        let rotate = Axis2Placement::new(Point3::ZERO, Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0));
        // (name, placement, scale, wide tolerance, narrow tolerance; `None` = body)
        let cases: [(&str, Axis2Placement, Vec3, f64, Option<f64>); 3] = [
            (
                "mirror",
                Axis2Placement::IDENTITY,
                Vec3::new(-1.0, 1.0, 1.0),
                1e-3,
                Some(1e-7),
            ),
            ("rotate", rotate, Vec3::new(1.0, 1.0, 1.0), 1e-3, Some(1e-7)),
            (
                "scale 2",
                Axis2Placement::IDENTITY,
                Vec3::new(2.0, 2.0, 2.0),
                2e-3,
                None,
            ),
        ];
        for (name, placement, scale, wide, narrow) in cases {
            let mut k = Kernel::default();
            let (mut b, [wide_edge, narrow_edge], v, f) = box_with_tolerances(&mut k);
            transform(&mut k, &mut b, &placement, Some(scale), true).unwrap();
            let body = b.tolerance();
            assert_tol(
                &format!("{name}: wide edge"),
                crate::topology::edge::tolerance(&b, wide_edge).unwrap(),
                wide,
            );
            assert_tol(
                &format!("{name}: vertex"),
                vertex::tolerance(&b, v).unwrap(),
                wide,
            );
            assert_tol(
                &format!("{name}: face"),
                crate::topology::face::p2::tolerance(&b, f).unwrap(),
                wide,
            );
            assert_tol(
                &format!("{name}: narrow edge"),
                crate::topology::edge::tolerance(&b, narrow_edge).unwrap(),
                narrow.unwrap_or(body),
            );
        }
    }

    /// WR-508 (#1521): a lone torus, which this port's `create_torus` gives a single vertex,
    /// so no vertex spans a size: upstream sizes the scaled body from that one-point vertex
    /// box (size zero), and the model size and body tolerance come out zero. The port
    /// sizes it from its bounding box, as a torus made at the scaled size is.
    #[test]
    fn transform_sizes_a_body_without_vertices_wr508() {
        for scale in [2.0, 0.5] {
            let mut k = Kernel::default();
            let mut torus =
                crate::primitive::api_primitives::create_torus(&mut k, Point3::ZERO, 6.0, 2.0)
                    .unwrap();
            assert_eq!(torus.vertices().len(), 1);
            let s = Vec3::new(scale, scale, scale);
            transform(&mut k, &mut torus, &Axis2Placement::IDENTITY, Some(s), true).unwrap();
            let mut direct_k = Kernel::default();
            let direct = crate::primitive::api_primitives::create_torus(
                &mut direct_k,
                Point3::ZERO,
                6.0 * scale,
                2.0 * scale,
            )
            .unwrap();
            let got = torus.tolerance();
            assert!(
                got > 0.0 && got >= direct.tolerance() * (1.0 - 1e-12),
                "scale {scale}: body tolerance {got:e}, direct {:e}",
                direct.tolerance()
            );
        }
    }

    #[test]
    fn empty_face_removal_preserves_an_existing_mark_lock() {
        use crate::geom::context::MarkType;
        let mut k = Kernel::default();
        let mut b = Brep::new();
        let mut outer = MarkLock::new(core::slice::from_mut(&mut k.context), 0, ALL_MARKS).unwrap();
        let held = outer.mark_type();
        let marks = [
            MarkType::Mark,
            MarkType::Mark2,
            MarkType::Mark3,
            MarkType::MarkIo,
            MarkType::MarkAssert,
        ];
        let before = marks.map(|m| k.context.current_mark(m));
        remove_faces(&mut k, &mut b, true, &[]).unwrap();
        assert!(k.context.is_mark_locked(held));
        let advanced: Vec<_> = marks
            .into_iter()
            .zip(before)
            .filter(|(m, n)| k.context.current_mark(*m) != *n)
            .collect();
        assert_eq!(advanced.len(), 1);
        assert_ne!(advanced[0].0, held);
        assert_eq!(k.context.current_mark(advanced[0].0), advanced[0].1 + 2);
        assert!(!k.context.is_mark_locked(advanced[0].0));
        let before_refusal = marks.map(|m| k.context.current_mark(m));
        assert!(remove_faces(&mut k, &mut b, false, &[]).is_err());
        assert_eq!(marks.map(|m| k.context.current_mark(m)), before_refusal);
        outer
            .release(core::slice::from_mut(&mut k.context))
            .unwrap();
        assert!(!k.context.is_mark_locked(held));
    }
    #[test]
    fn pending_failure_detaches_borrowed_dependent_attributes() {
        use crate::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
        let mut store = AttributeStore::new();
        let key = ObjectKey(91);
        let attribute = store
            .insert(Attribute::long(10000, 12, Behavior::Copy))
            .unwrap();
        store.add_attribute(key, attribute).unwrap();
        {
            let _pending = PendingTopology::new(Some((key, 7_u32)), &mut store);
        }
        assert!(store.attributes(key).is_empty());
        // A normal clear would delete this dependent attribute; pending cleanup
        // must retain the caller's allocation so a reader can retry it elsewhere.
        assert!(store.get(attribute).is_some());
        store.add_attribute(ObjectKey(92), attribute).unwrap();
    }

    #[test]
    fn pending_release_preserves_attributes_and_transfers_object() {
        use crate::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
        let mut store = AttributeStore::new();
        let key = ObjectKey(91);
        let attribute = store
            .insert(Attribute::long(10000, 12, Behavior::Copy))
            .unwrap();
        store.add_attribute(key, attribute).unwrap();
        let mut pending = PendingTopology::new(Some((key, 7_u32)), &mut store);
        *pending.object_mut().unwrap() = 8;
        assert_eq!(pending.release(), Some((key, 8)));
        assert_eq!(store.attributes(key), vec![attribute]);
    }

    #[test]
    fn issued_body_glue_notifies_real_caller_and_keeps_failure_prefix() {
        use crate::geom::attribute::{ids, Attribute, AttributeStore, Behavior};
        use crate::topology::edge::notification_test_support as callbacks;

        for separate_region in [false, true] {
            let mut k = Kernel::default();
            let mut b = crate::primitive::api_primitives::create_box(
                &mut k,
                Point3::ZERO,
                10.0,
                20.0,
                30.0,
            )
            .unwrap();
            let original_key = b.key();
            let serial = b.notify_serial().unwrap();
            let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
            let body = key_of(crate::EntityRef::Brep(crate::topology::BrepId::from_index(
                0,
            )));
            assert_ne!(body.key, original_key);
            let survivor = b.vertex_list()[0];
            let deleted = if separate_region {
                let region = b
                    .add_region_record(crate::topology::Region::default())
                    .unwrap();
                let mut setup_records = crate::topology::topology::NotifyRecords::new();
                crate::topology::brep::p3::seed_records(&mut setup_records, &b, &key_of);
                let mut setup_attributes = AttributeStore::default();
                crate::topology::brep::p9::make_shell_vertex_with_kernel_context(
                    &mut k,
                    &mut b,
                    Some(region),
                    Point3::new(40.0, 0.0, 0.0),
                    &mut crate::topology::edge::NotificationStores {
                        records: &mut setup_records,
                        attributes: &mut setup_attributes,
                        key_of: &key_of,
                    },
                )
                .unwrap()
                .1
            } else {
                b.vertex_list()[1]
            };
            let survivor_ref = key_of(crate::EntityRef::Vertex(survivor));
            let deleted_ref = key_of(crate::EntityRef::Vertex(deleted));
            let mut records = crate::topology::topology::NotifyRecords::new();
            crate::topology::brep::p3::seed_records(&mut records, &b, &key_of);
            let mut attributes = AttributeStore::default();
            let marker = attributes
                .insert(Attribute::long(ids::USER_1, 917, Behavior::Reference))
                .unwrap();
            attributes.add_attribute(body.key, marker).unwrap();
            let events = callbacks::install(&mut k.context);
            let result =
                crate::geom::context::Context::with_notification_context(&mut k, |k, context| {
                    let mut env = EditNotifyEnv {
                        records: &mut records,
                        attributes: &mut attributes,
                        context: Some(context),
                        key_of: &key_of,
                    };
                    glue_with_body_identity(
                        k,
                        &mut b,
                        Some(survivor),
                        Some(deleted),
                        None,
                        &mut env,
                        body,
                    )
                })
                .unwrap();
            callbacks::assert_installed(&k.context, &events);
            assert_eq!(b.key(), original_key);
            assert_eq!(attributes.users(marker).unwrap(), &[body.key]);
            assert_eq!(attributes.get(marker).unwrap().long_value(), Some(917));
            let events = events.lock().unwrap();
            assert_eq!(events[0].caller, body);
            assert_eq!(events[0].operation, NotifyOperation::PreEdit);
            assert_eq!(events[0].data, [Some(body), None, None]);
            assert_eq!((events[0].callback, events[1].callback), ("user", "system"));
            if separate_region {
                assert!(result.is_err());
                assert_eq!(events.len(), 6);
                assert!(b.vertex_list().contains(&deleted));
            } else {
                result.unwrap();
                assert!(!b.vertex_list().contains(&deleted));
                assert!(events.iter().any(|event| event.caller == survivor_ref
                    && event.operation == NotifyOperation::PreEdit
                    && event.data == [Some(survivor_ref), Some(body), None]));
                assert!(events.iter().any(|event| event.caller == body
                    && event.operation == NotifyOperation::MergeInBrep
                    && event.data == [Some(survivor_ref), Some(deleted_ref), Some(body)]));
            }
            assert!(!events.iter().any(|event| event.caller.key == original_key));
        }
    }

    #[test]
    fn glue_alias_precedes_arena_access_and_notifications() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        // Even a stale equal handle is only compared, never dereferenced natively.
        let stale = Some(VertexId::from_index(999));
        glue_vertices(&mut k, &mut b, stale, stale, None).unwrap();
        glue_vertices(&mut k, &mut b, None, None, None).unwrap();
        assert!(b.vertices().is_empty());
    }

    /// #13790: a typed failure that replaced the no-sector `Upstream` is the same
    /// unedited failure.
    #[test]
    fn no_vertex_sector_reads_the_status_of_a_typed_failure() {
        use crate::error::replaced_upstream_samples::{native_failures, replaced_upstream};
        let upstream = SolidError::null_pointer(NO_VERTEX_SECTOR_OP);
        assert!(is_no_vertex_sector(&upstream));
        for typed in replaced_upstream(NO_VERTEX_SECTOR_OP, 1006, None) {
            assert!(is_no_vertex_sector(&typed), "{typed:?}");
        }
        for other in replaced_upstream("topology::brep::p5::other", 1006, None)
            .into_iter()
            .chain(native_failures(NO_VERTEX_SECTOR_OP, 1006))
        {
            assert!(!is_no_vertex_sector(&other), "{other:?}");
        }
    }

    /// A wire edge whose curve is the UV line from `uv0` to `uv1` on `surface`, and the
    /// edge's interval.
    fn curve_on_surface_edge(
        k: &mut Kernel,
        b: &mut Brep,
        surface: crate::surface::surface::Surface,
        uv0: Point3,
        uv1: Point3,
    ) -> (EdgeId, Extent1d) {
        use crate::curve::crv_on_surf::CrvOnSurf;
        use crate::curve::line::Line;
        let uv = Curve::from(Line::from_points(uv0, uv1, 2));
        let cos = Curve::from(CrvOnSurf::new(k, uv, surface, None, 2).expect("curve on surface"));
        let ivl = cos.natural_interval();
        let e = create_wire_edge_from_curve(k, b, cos, ivl).expect("wire edge");
        (e, ivl)
    }

    /// WR-598 takes an iso curve only for an exactly constant UV coordinate, u or v. On
    /// `S(u, v) = (u / EFF_ZERO, v, u v / EFF_ZERO)` over `[0, 1]^2` the UV line from
    /// `(0, 0)` to `(EFF_ZERO / 2, 1)` drifts in u by less than `EFF_ZERO` times the domain
    /// (the fork's test), yet its far end is `(0.5, 1, 0.5)` and the `u = 0` iso curve's
    /// `(0, 1, 0)`: it is not converted. The exactly constant `u = 0.5` line is. The same
    /// with u and v swapped, for the constant-v branch.
    #[test]
    fn wr598_only_an_exactly_constant_uv_line_is_an_iso_curve() {
        use crate::math::EFF_ZERO;
        use crate::nurbs::b_spline_surface::BSplineSurface;
        let big = 1.0 / EFF_ZERO;
        // (the varying-u surface, then its swap: S(u, v) = (u, v / EFF_ZERO, u v / EFF_ZERO))
        for swap in [false, true] {
            let at = |u: f64, v: f64| {
                if swap {
                    Point3::new(v, u, 0.0)
                } else {
                    Point3::new(u, v, 0.0)
                }
            };
            let space = |x: f64, y: f64, z: f64| {
                if swap {
                    Point3::new(y, x, z)
                } else {
                    Point3::new(x, y, z)
                }
            };
            let mut k = Kernel::default();
            let mut b = Brep::new_with_infinite_region().expect("brep");
            let patch = BSplineSurface::create_bilinear_surface(
                Point3::new(0.0, 0.0, 0.0),
                if swap {
                    Point3::new(1.0, 0.0, 0.0)
                } else {
                    Point3::new(big, 0.0, 0.0)
                },
                if swap {
                    Point3::new(0.0, big, 0.0)
                } else {
                    Point3::new(0.0, 1.0, 0.0)
                },
                if swap {
                    Point3::new(1.0, big, big)
                } else {
                    Point3::new(big, 1.0, big)
                },
            )
            .expect("patch");
            let surface = crate::surface::surface::Surface::from(patch);
            let (drift, drift_ivl) = curve_on_surface_edge(
                &mut k,
                &mut b,
                surface.clone(),
                at(0.0, 0.0),
                at(EFF_ZERO / 2.0, 1.0),
            );
            let end = edge_curve(&b, drift)
                .expect("curve")
                .evaluate_point(&k, drift_ivl.max())
                .expect("end");
            assert!(
                end.closer_than(1e-9, space(0.5, 1.0, 0.5)),
                "swap {swap}: {end:?}"
            );
            let tol = b.edge(drift).expect("edge").tolerance;
            let converted = exact_bspline_of_edge(&k, &b, drift, drift_ivl, tol).expect("answers");
            assert!(converted.is_none(), "swap {swap}: {converted:?}");

            let (exact, exact_ivl) =
                curve_on_surface_edge(&mut k, &mut b, surface, at(0.5, 0.0), at(0.5, 1.0));
            let tol = b.edge(exact).expect("edge").tolerance;
            let iso = exact_bspline_of_edge(&k, &b, exact, exact_ivl, tol)
                .expect("answers")
                .expect("an iso curve");
            let iso_end = iso
                .evaluate_point(&k, iso.natural_interval().max())
                .expect("end");
            let edge_end = edge_curve(&b, exact)
                .expect("curve")
                .evaluate_point(&k, exact_ivl.max())
                .expect("end");
            // both at 5e11 scale: equal up to rounding at that scale
            assert!(
                iso_end.closer_than(1e-2, edge_end),
                "swap {swap}: {iso_end:?} {edge_end:?}"
            );
        }
    }

    /// WR-598 on a procedural blend, which evaluates its definition S, not its net: the iso
    /// curve is cut from the net, within the net's certified bound of S, so it is taken
    /// only while that bound is within the joined edge's tolerance (not at half the bound,
    /// nor at a NaN tolerance); the tolerance is not widened to admit it.
    #[test]
    fn wr598_iso_curve_on_a_procedural_blend_needs_its_bound_within_the_tolerance() {
        use crate::blend::fillet_section::section::GnSectionFsg;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::surface::procedural_blend::{BlendDefinitionInput, ProceduralBlend};
        use crate::surface::surface::{Surface, SurfaceKind};
        // the plane-plane round of wr698_procedural_blend_definition.rs, bound 1e-3
        let (l, r, bound) = (10.0, 2.0, 1e-3);
        let mut k = Kernel::default();
        let o = Point3::new(0.0, 0.0, 0.0);
        let bilinear = |p: [Point3; 4]| {
            BSplineSurface::create_bilinear_surface(p[0], p[1], p[2], p[3]).expect("bilinear")
        };
        let line = |a: Point3, c: Point3| BSplineCurve::create_line_segment(3, a, c).expect("line");
        let a = Surface::from(bilinear([
            o,
            Point3::new(l, 0.0, 0.0),
            Point3::new(0.0, 0.0, l),
            Point3::new(l, 0.0, l),
        ]));
        let c = Surface::from(bilinear([
            o,
            Point3::new(l, 0.0, 0.0),
            Point3::new(0.0, l, 0.0),
            Point3::new(l, l, 0.0),
        ]));
        let net = bilinear([
            o,
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ]);
        let leader = line(Point3::new(0.0, r, r), Point3::new(l, r, r));
        let seed = line(Point3::new(0.0, r / l, 0.0), Point3::new(1.0, r / l, 0.0));
        let input = BlendDefinitionInput {
            supports: [&a, &c],
            sides: [-1.0, 1.0],
            offsets: [r, r],
            law: None,
            order: 2,
            thumbweight: 0.0,
            leader: &leader,
            seeds: [&seed, &seed],
            bound,
            chamfer: None,
        };
        let blend = ProceduralBlend::create(&k, &net, &input, |n, w| {
            GnSectionFsg::is_valid(n as usize, w)
        })
        .expect("blend");
        let surface = Surface::from(blend);
        assert!(matches!(surface.kind(), SurfaceKind::ProceduralBlend(_)));
        let mut b = Brep::new_with_infinite_region().expect("brep");
        let (e, ivl) = curve_on_surface_edge(
            &mut k,
            &mut b,
            surface,
            Point3::new(0.5, 0.0, 0.0),
            Point3::new(0.5, 1.0, 0.0),
        );
        let refused = exact_bspline_of_edge(&k, &b, e, ivl, bound / 2.0).expect("answers");
        assert!(refused.is_none(), "{refused:?}");
        assert!(exact_bspline_of_edge(&k, &b, e, ivl, f64::NAN)
            .expect("answers")
            .is_none());
        let taken = exact_bspline_of_edge(&k, &b, e, ivl, bound).expect("answers");
        assert!(taken.is_some());
    }

    /// The bound test refuses a NaN on either side and compares two finite doubles exactly.
    #[test]
    fn wr598_bound_within_tolerance_refuses_nan() {
        assert!(!bound_within_tolerance(f64::NAN, 1.0));
        assert!(!bound_within_tolerance(1e-3, f64::NAN));
        assert!(!bound_within_tolerance(f64::NAN, f64::NAN));
        assert!(bound_within_tolerance(1e-3, 1e-3));
        assert!(!bound_within_tolerance(
            1e-3,
            f64::from_bits(1e-3_f64.to_bits() - 1)
        ));
    }

    /// WR-598 on a screw sweep's wall: a B-spline net that keeps its definition beside it
    /// but evaluates the net (`SurfaceKind::BSpline`), so the net's iso curve is the curve
    /// on the wall exactly and is taken whatever the definition's bound: refusing it would
    /// change nothing in space.
    #[test]
    fn wr598_iso_curve_on_a_sweep_wall_is_the_net_s() {
        let mut k = Kernel::default();
        let profile: Vec<Curve> = [(4.1, 0.0), (5.0, 0.0), (5.0, 0.9), (4.1, 0.9)]
            .iter()
            .zip([(5.0, 0.0), (5.0, 0.9), (4.1, 0.9), (4.1, 0.0)].iter())
            .map(|(&(ar, az), &(br, bz))| {
                Curve::from(crate::curve::line::Line::from_points(
                    Point3::new(ar, 0.0, az),
                    Point3::new(br, 0.0, bz),
                    3,
                ))
            })
            .collect();
        let body = crate::sweep::screw::screw_sweep(
            &mut k,
            &profile,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            1.0,
            1.0,
            1e-4,
        )
        .expect("screw sweep");
        let wall = crate::topology::brep::p1::faces(&body, None)
            .expect("faces")
            .into_iter()
            .find_map(|f| {
                let s = crate::topology::face::p2::surface(&body, f).ok()??;
                s.procedural_sweep().is_some().then(|| s.clone())
            })
            .expect("a wall that keeps its definition");
        let bound = wall.procedural_definition().expect("definition").bound();
        assert!(bound > 0.0, "{bound}");
        let dom = wall.natural_uv_domain_with_nurb(&k).expect("domain");
        let u = dom.u_interval().mid();
        let (v0, v1) = (dom.v_interval().min(), dom.v_interval().max());
        let mut b = Brep::new_with_infinite_region().expect("brep");
        let (e, ivl) = curve_on_surface_edge(
            &mut k,
            &mut b,
            wall,
            Point3::new(u, v0, 0.0),
            Point3::new(u, v1, 0.0),
        );
        let iso = exact_bspline_of_edge(&k, &b, e, ivl, bound / 2.0)
            .expect("answers")
            .expect("the net's iso curve");
        let cos = edge_curve(&b, e).expect("curve");
        let iso_ivl = iso.natural_interval();
        for s in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let p = iso
                .evaluate_point(&k, iso_ivl.evaluate(s))
                .expect("iso point");
            let q = cos.evaluate_point(&k, ivl.evaluate(s)).expect("edge point");
            assert!(p.closer_than(1e-9, q), "s {s}: {p:?} {q:?}");
        }
    }
}
