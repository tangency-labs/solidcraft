//! High-level boolean entry points (`SM_API`): two-B-rep booleans (union, difference,
//! intersection, merge, with or without merge options, non-manifold, with the
//! intersection edges, piecewise), planar 2D booleans, N-ary booleans over a list of
//! B-reps, booleans between lists of B-reps or surfaces, and CSG trees in postfix
//! notation.
//!
//! Upstream: `source/SM_API/src/SmApiBrep.cpp`, `source/SM_API/inc/SmApiBrep.h`.
//!
//! | Rust                                          | Upstream                                     |
//! |-----------------------------------------------|----------------------------------------------|
//! | `merge_breps_inputs_are_valid` (private)      | `SmApiMergeBrepsInputsAreValid` (static)     |
//! | `merge_breps_kernel_operation` (private)      | `SmApiMergeBrepsKernelOperation` (static)    |
//! | `validate_boolean_operands` (private)         | `SmApiValidateBooleanOperands` (static)      |
//! | `merge_boolean` (private)                     | the shared body of the two-B-rep entry points |
//! | [`boolean`], [`BooleanOutcome`]               | `SmApiBoolean`                               |
//! | [`boolean_union`]                             | `SmApiBooleanUnion`                          |
//! | [`boolean_difference`]                        | `SmApiBooleanDifference`                     |
//! | [`boolean_intersection`]                      | `SmApiBooleanIntersection`                   |
//! | [`boolean_merge`]                             | `SmApiBooleanMerge`                          |
//! | [`merge_breps`]                               | `SmApiMergeBreps`                            |
//! | [`boolean_with_options`]                      | `SmApiBooleanWithOptions`                    |
//! | [`boolean_with_curves`], [`BooleanWithCurves`] | `SmApiBooleanWithCurves`                    |
//! | [`boolean_2d`], [`Boolean2dOutcome`]          | `SmApiBoolean2d`                             |
//! | [`non_manifold_boolean`]                      | `SmApiNonManifoldBoolean`                    |
//! | [`piecewise_merge`], [`PiecewiseMergeOutcome`] | `SmApiPiecewiseMerge`                       |
//! | [`boolean_lists`], [`BooleanListsResult`]     | `SmApiBooleanLists`                          |
//! | [`boolean_trees`]                             | `SmApiBooleanTrees`                          |
//! | [`tessellate_boundaries`]                     | `SmApiTessellateBoundaries`                  |
//!
//! Deviations:
//! - **Ownership.** Upstream passes `SmBrep*` operands that the kernel modifies in
//!   place (the first becomes `rpResult`) or deletes (the second). The port takes the
//!   operands by value and returns the result; a second operand upstream keeps
//!   (`SM_BO_PARTIAL_MERGE`, `SM_BO_IMPRINT_CLASSIFY`, `bKeepOtherBrep`) is handed
//!   back in [`BooleanOutcome::other`]. A `NULL` operand, the same B-rep passed twice
//!   (`SmApiValidateBooleanOperands`) and a `NULL` or duplicate list entry
//!   (`SmApiMergeBrepsInputsAreValid`) cannot be expressed with owned values, so those
//!   checks reduce to the remaining ones. Operands rejected by a two-B-rep entry point
//!   are dropped (upstream leaves them with the caller); [`merge_breps`] takes the list
//!   `&mut` and leaves it untouched on rejection, as upstream. [`boolean_2d`] hands both
//!   operands back when upstream leaves them unassigned (an operand without faces);
//!   [`piecewise_merge`] always hands the second operand back (upstream imprints but
//!   does not delete it).
//! - **Intersection edges.** [`boolean_with_curves`] returns `SmMerge::m_vEdges` (the
//!   common edges `ManifoldBoolean` collected on the first B-rep, which
//!   `NonManifoldBoolean` also runs) as ids into the result, in that order. Upstream
//!   copies the non-null pointers without checking that the edges survived the rest of
//!   the merge; an id may likewise name an edge the result no longer has.
//! - **Out-params.** `rpResult`, `rResultBreps` / `rResultSurfaces` are return values;
//!   on failure there is none (upstream clears `rpResult`).
//! - **Context.** `SmApiGetOrCreateContext()` is the caller's [`Kernel`], taken by
//!   every entry point (the merge engine reads its tolerances).
//! - **Statuses.** `SM_ERR_INVALID_INPUT` is [`SolidError::InvalidInput`]; `SM_ERR`
//!   is a [`SolidError::Failed`] whose comparison status is [`crate::error::SM_ERR`].
//! - **2D booleans.** [`boolean_2d`] runs `SmPrimitiveCreation::Boolean2D` (static
//!   upstream) on a [`PrimitiveCreation`] without primitive B-rep, as `merge::p3` does
//!   for planar list operands, with the operands' marks and an arena-only notification
//!   store ([`ArenaNotifyState`]).
//! - **Piecewise merge.** [`piecewise_merge`] builds `SmMerge(*pContext, pBrep1,
//!   pBrep2)` as a merge session of a [`PrimitiveModel`] over both operands and runs its
//!   `PiecewiseMerge(NULL, FALSE, bCreateNewBrepsForFaces, rpResult)`.
//! - **Dependencies.** List folding and tree evaluation call `merge::p3`; its
//!   construction and topology calls retain narrowly scoped dependency errors. The
//!   two-B-rep entry points build the merge (`boolean.merge_p2`, through
//!   `merge::p3`'s `DependencyMerge`, which keeps it as the `boolean.merge_p4` engine),
//!   then call `SmBrep::IsManifoldSolid` (`topology.brep_p2`) and `ManifoldBoolean`
//!   (`boolean.merge_p4`) or `NonManifoldBoolean` (`boolean.merge_p3`).
//! - **List context.** `boolean_lists` takes a mutable kernel for the connected
//!   component traversal's mark locks.
//! - **Nullable tree output.** Tree operands are `Option<Brep>` slots, preserving
//!   consumed entries on failure and defined null outputs of repeated leaves.
//! - **Attributes.** Upstream's attributes live on the operands' objects, which the merge
//!   edits in place. Here each [`Brep`] owns its attributes ([`Brep::add_attribute`]):
//!   the two-B-rep entry points move both operands' attributes into the merge's store
//!   under the merge's names of their objects, the merge's notifications propagate them
//!   with SMLib's handlers, and the attributes left on the result's (and a kept other
//!   B-rep's) objects go back to those B-reps; those of objects the merge deleted are
//!   cleared as upstream's destructors do. A reference attribute on objects of both
//!   returned B-reps becomes one attribute per B-rep. [`merge_breps`] and
//!   [`boolean_trees`] do the same for every merge they run. [`boolean_lists`] carries
//!   each list B-rep's attributes into its operand copies (`SM_NO_COPY`), through the
//!   merge and to every component B-rep it returns; a reference attribute shared by
//!   several output B-reps (or with the inputs, which keep theirs) becomes one per
//!   B-rep. `SM_BO_EXTRACT_SEPARATE`'s component B-reps are not returned by any entry
//!   point here (it is rejected), so their attributes stay in the merge's store.
//! - The debug drawing (`SM_DEBUG_CODE`) and `SM_ASSERT_VALID` checks are not ported.
//! - **`SmApiTessellateBoundaries`.** The B-rep is a reference (no `NULL` check); its
//!   out-arrays are the returned [`BoundaryTessellation`], whose `complete` flag is
//!   upstream's `SM_ERR` with partial output. An invalid angle tolerance is
//!   [`SolidError::InvalidInput`] (upstream clears the arrays).
//! - **Cached topology.** Every returned B-rep has the port's own cached fields
//!   ([`crate::topology::Edge::vertices`], `Face::shell`, ..., which topology edits
//!   inside the merge do not maintain) rewritten from its use links by
//!   [`Brep::refresh_cached_topology`]; nothing else changes.

use crate::boolean::merge::MergeOptions;
use crate::error::{Result, SolidError};
use crate::geom::a_object::BooleanOperation;
use crate::kernel::Kernel;
use crate::primitive::primitive_creation::model::PrimitiveModel;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::surface::surface::Surface;
use crate::topology::brep::p1::{self as brep_p1, BoundaryTessellation};
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::{Brep, EdgeId};

/// The planar 2D boolean operations of [`boolean_2d`] (`Sm2DBooleanOperationType`).
pub use crate::primitive::primitive_creation::p1::Boolean2d;

const OP_BOOLEAN: &str = "boolean::api_brep::boolean";
const OP_MERGE_BREPS: &str = "boolean::api_brep::merge_breps";
const OP_PIECEWISE: &str = "boolean::api_brep::piecewise_merge";

/// The outcome of a two-B-rep boolean ([`boolean`], [`boolean_with_options`]).
#[derive(Debug)]
pub struct BooleanOutcome {
    /// `rpResult`: the first operand, modified in place by the merge.
    pub result: Brep,
    /// The second operand when the operation keeps it (`SM_BO_PARTIAL_MERGE`,
    /// `SM_BO_IMPRINT_CLASSIFY`, or [`MergeOptions::keep_other_brep`]); `None` when
    /// upstream deletes it.
    pub other: Option<Brep>,
}

/// The outcome of [`boolean_with_curves`].
#[derive(Debug)]
pub struct BooleanWithCurves {
    /// The result and the second operand when kept, as [`boolean`]'s.
    pub outcome: BooleanOutcome,
    /// `rIntersectionEdges`: the merge's common edges on the first B-rep
    /// (`SmMerge::m_vEdges`), ids into [`BooleanOutcome::result`].
    pub intersection_edges: Vec<EdgeId>,
}

/// The outcome of a successful [`boolean_2d`].
#[derive(Debug)]
pub enum Boolean2dOutcome {
    /// `rpResult`: the first operand, modified; the second was consumed.
    Result(Brep),
    /// An operand has no faces: upstream succeeds without assigning `rpResult` or
    /// deleting the second operand, so both come back unchanged.
    Unassigned {
        /// The first operand.
        brep1: Brep,
        /// The second operand (boxed: keeps the variants of similar size).
        brep2: Box<Brep>,
    },
}

/// The outcome of [`piecewise_merge`].
#[derive(Debug)]
pub struct PiecewiseMergeOutcome {
    /// `rpResult`: the first operand, modified.
    pub result: Brep,
    /// The second operand, imprinted but not deleted.
    pub other: Brep,
}

/// The outputs of [`boolean_lists`].
pub use crate::boolean::merge::p3::BooleanListsResult;

/// Validates a public N-ary boolean input list without consuming it
/// (`SmApiMergeBrepsInputsAreValid`): at least two B-reps. Upstream also rejects a
/// `NULL` or repeated entry, which owned values cannot be.
fn merge_breps_inputs_are_valid(breps: &[Brep]) -> bool {
    breps.len() >= 2
}

/// Maps the public boolean operation to `SmMerge::merge_breps`' legacy zero-based
/// operation selector (`SmApiMergeBrepsKernelOperation`): union 0, intersection 1,
/// difference 2, merge 3; `None` for every other operation.
fn merge_breps_kernel_operation(operation: BooleanOperation) -> Option<u32> {
    match operation {
        BooleanOperation::Union => Some(0),
        BooleanOperation::Intersection => Some(1),
        BooleanOperation::Difference => Some(2),
        BooleanOperation::Merge => Some(3),
        _ => None,
    }
}

/// Validates the operands of a two-B-rep boolean before the kernel modifies or
/// deletes either of them (`SmApiValidateBooleanOperands`). `SM_BO_EXTRACT_SEPARATE`
/// has no single result (upstream's kernel deletes both inputs and leaves the pieces
/// in `SmMerge` members that nothing frees). Upstream also rejects a `NULL` operand
/// and the same B-rep twice, which owned values cannot be.
fn validate_boolean_operands(operation: BooleanOperation) -> Result<()> {
    if operation == BooleanOperation::ExtractSeparate {
        return Err(SolidError::invalid_input(
            OP_BOOLEAN,
            "SM_BO_EXTRACT_SEPARATE has no single result",
        ));
    }
    Ok(())
}

/// Which `SmMerge` method a two-B-rep entry point runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeMethod {
    /// `ManifoldBoolean` when both operands are manifold solids
    /// (`SmBrep::IsManifoldSolid`), else `NonManifoldBoolean`.
    ByOperands,
    /// `NonManifoldBoolean` (`SmApiNonManifoldBoolean`).
    NonManifold,
}

/// One edge of a boolean operand, as [`glue_self_touching_edges`] compares them: its
/// vertices (the lower index first), its shell, its faces, whether a face lies on both sides
/// of it, and its curve's kind (the shell, faces, crack flag and kind only for an edge that
/// shares its vertices with another).
struct TouchingEdge {
    edge: EdgeId,
    start: crate::topology::VertexId,
    ends: (u32, u32),
    shell: Option<crate::topology::ShellId>,
    faces: Vec<crate::topology::FaceId>,
    /// A face on both sides of the edge (more than two of its edge uses on one face: the
    /// kernel's crack edge, which its check refuses glued to another edge).
    crack: bool,
    kind: Option<core::mem::Discriminant<crate::curve::curve::p2::CurveKind>>,
}

impl TouchingEdge {
    /// Whether `other` may be a second copy of this edge, before its geometry is compared:
    /// the same vertices and curve kind, both on faces (not wires), and neither a crack edge
    /// (a face on both sides of it). The faces against those already glued are checked by
    /// [`copy_pairs`]. The shells may differ: the record round trip leaves a solid touching
    /// itself in two shells joined along the line, and the kernel shares one edge between
    /// shells that touch.
    fn may_copy(&self, other: &Self) -> bool {
        self.shell.is_some()
            && other.shell.is_some()
            && !self.crack
            && !other.crack
            && self.ends == other.ends
            && self.kind.is_some()
            && self.kind == other.kind
    }
}

/// The edges to glue, `(surviving, to_delete, orientation)`: in each bucket of edges that
/// share their vertices (indices into `touches`), each edge not yet glued away keeps every
/// later edge that [`TouchingEdge::may_copy`] it, shares no face with it **or with any copy
/// already glued into it**, and whose curve `coincide` proves within the tolerance. So no
/// face ever ends up on both sides of the glued edge (the crack-glued form the kernel's check
/// refuses), however many copies a bucket holds.
///
/// # Errors
///
/// Those of `coincide`.
fn copy_pairs(
    touches: &[TouchingEdge],
    crowded: &[Vec<usize>],
    mut coincide: impl FnMut(EdgeId, EdgeId) -> Result<bool>,
) -> Result<
    Vec<(
        EdgeId,
        EdgeId,
        crate::nurbs::b_spline_surface_n_lib::p2::OrientType,
    )>,
> {
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use std::collections::BTreeSet;

    let mut pairs = Vec::new();
    let mut taken: BTreeSet<EdgeId> = BTreeSet::new();
    for bucket in crowded {
        for (n, &i) in bucket.iter().enumerate() {
            let Some(first) = touches.get(i) else {
                continue;
            };
            if taken.contains(&first.edge) {
                continue;
            }
            // the faces of the glued edge so far: this edge's and every accepted copy's
            let mut faces: BTreeSet<crate::topology::FaceId> =
                first.faces.iter().copied().collect();
            for &j in bucket.iter().skip(n + 1) {
                let Some(other) = touches.get(j) else {
                    continue;
                };
                if taken.contains(&other.edge)
                    || !first.may_copy(other)
                    || other.faces.iter().any(|f| faces.contains(f))
                    || !coincide(first.edge, other.edge)?
                {
                    continue;
                }
                let orientation = if first.start == other.start {
                    OrientType::Same
                } else {
                    OrientType::Opposite
                };
                pairs.push((first.edge, other.edge, orientation));
                taken.insert(other.edge);
                faces.extend(other.faces.iter().copied());
            }
        }
    }
    Ok(pairs)
}

/// Not upstream: a bound `d` such that every point of curve `a` over `ia` is within `d` of a
/// point of curve `b` over `ib` and every point of `b` over `ib` within `d` of a point of `a`
/// over `ia` (their Hausdorff distance), every rounding of computing it included; `None` when
/// no bound is certain for the pair, which then is not glued.
///
/// - Two lines: each edge is the segment between its curve's points at its interval's ends
///   ([`Line::segment_over`](crate::curve::line::Line::segment_over), each end with the error
///   bound of its coordinates). With the ends paired either way round, the distance between
///   `(1 - s) a0 + s a1` and `(1 - s) b0 + s b1` is at most `(1 - s)|a0 - b0| + s|a1 - b1|`
///   (the triangle inequality), so at most the larger end distance at every `s`: both
///   segments lie within it of each other. The pairing with the smaller bound is taken; each
///   end's coordinate error `e` moves it by at most `sqrt(3) e < 2 e`.
/// - Two B-splines (no other kind: their evaluation is their NURB's) on the same interval,
///   inside both NURBs' domains: at every parameter of the NURBs' common domain the two are
///   within [`certified_distance`](crate::nurbs::crv_deviation::certified_distance) of each
///   other, so each edge's point at `t` is within it of the other's point at the same `t`.
///   Rational and 2D NURBs, and NURBs on different domains, have no bound.
/// - Every other pair: no bound.
fn coincidence_bound(
    a: &crate::curve::curve::p2::Curve,
    ia: &crate::geom::extent1d::Extent1d,
    b: &crate::curve::curve::p2::Curve,
    ib: &crate::geom::extent1d::Extent1d,
) -> Option<f64> {
    use crate::curve::curve::p2::CurveKind;
    use crate::nurbs::crv_deviation::{certified_distance, norm_up};
    use crate::nurbs::math::gamma;

    match (a.kind(), b.kind()) {
        (CurveKind::Line(la), CurveKind::Line(lb)) => {
            let [(a0, ea0), (a1, ea1)] = la.segment_over(ia)?;
            let [(b0, eb0), (b1, eb1)] = lb.segment_over(ib)?;
            let dist =
                |p: crate::Point3, q: crate::Point3| norm_up(p.x - q.x, p.y - q.y, p.z - q.z);
            let along = dist(a0, b0)?.max(dist(a1, b1)?);
            let across = dist(a0, b1)?.max(dist(a1, b0)?);
            let err = 2.0 * (ea0.max(ea1) + eb0.max(eb1));
            let bound = (along.min(across) + err) * (1.0 + gamma(2));
            bound.is_finite().then_some(bound)
        }
        (CurveKind::BSpline(ca), CurveKind::BSpline(cb)) => {
            if ia != ib {
                return None;
            }
            let (na, nb) = (ca.nurb_ref().ok()?, cb.nurb_ref().ok()?);
            let inside = |n: &crate::nurbs::crv_basic::Curve| {
                matches!(
                    (n.knots().first(), n.knots().last()),
                    (Some(&lo), Some(&hi)) if lo <= ia.min() && ia.max() <= hi
                )
            };
            if !(inside(na) && inside(nb)) {
                return None;
            }
            certified_distance(na, nb).ok()
        }
        _ => None,
    }
}

/// Not upstream (WR-754): the coincident edges along which a solid touches itself, glued back
/// into one edge each.
///
/// A solid touching itself along a line (a hole's bottom meeting two pocket walls along a
/// segment of their corner edge) is one edge of four faces in the form the merge handles. The
/// record format writes such an edge as one edge per face pair (`split_touching`), and the
/// merge of an operand in that form gives a body that is not a closed solid, or
/// fails in `topology_intersector::p1::relate` (1001). So each group of edges with the same
/// two vertices and curve kind, whose faces are all distinct and whose curves are proved
/// within the B-rep tolerance of each other over their whole intervals
/// ([`coincidence_bound`]: a bound over the whole edge, never samples; a pair with no bound,
/// or a NaN, is not glued), is glued into its first edge (`GlueEdgesGeneral`, no gap, with
/// region nesting), whether the copies lie in one shell or in two shells that touch along the
/// line (the round trip leaves a self-touching solid in two). Edges whose faces repeat (a
/// face on both sides of the line: the kernel's crack edges, which its check refuses glued)
/// are left alone, and a copy sharing a face with the edge or with a copy already glued into
/// it is not glued ([`copy_pairs`]). Returns the number of edges glued away.
///
/// The edges are bucketed by their vertex pair; only a bucket of two or more edges has its
/// pairs compared, so a body without self-touch costs one pass over its edges.
///
/// # Errors
///
/// Those of the topology walk and the coincidence checks; those of `GlueEdgesGeneral` that
/// stand for no upstream status ([`crate::error::native_failure`]: a port invariant, a stale
/// id, an unported call, cancellation).
///
/// **A gluing that fails leaves the operand as given.** The glue is all or nothing
/// ([`glue_all_or_none`]): the operand, `k.rng` and `k.upstream_ub_passed` are saved before
/// the first pair, and any other failure of `GlueEdgesGeneral` restores them exactly, so the
/// merge then runs as main does (0 is returned). `GlueEdgesGeneral` can fail on a valid operand, both before its
/// first change to the topology (`b.remove_from_edge_list(del)`) and after it:
/// - before: the notifications (`PreEdit`, `MergeInBrep`, each face's `PreEdit`), the
///   intervals and `curve_max_distance_between` of the same-geometry test (orientation
///   `Same`, equal intervals), the vertex reads and the shared-vertex precondition;
/// - after, for each moved use pair: the radial-sector search and its `InvalidInput` errors
///   (an edge or mate without edge use, shell, radial or region), `set_uv_trim_curve`,
///   `edge_remove` of the pair, the sector's mate and loop tests, `set_orientation`,
///   `add_oriented_eu_pair`, a surviving wire's `edge_remove` and vertex-use unlinking, the
///   surviving edge's `set_tolerance` and `destroy` of the deleted edge; then `merge_shells`,
///   `test_closure_split_region_attempt` (whose status passes on with
///   `brep_edit_statuses_pass_on`, and which may draw from `k.rng`), `is_spine` and
///   `test_closure_split_region_edge`.
///
/// A failure at an "after" site would leave the operand half glued, so a check up front
/// cannot replace the snapshot. A snapshot the B-rep cannot take (a cleared surface owner,
/// or nesting past the copy bound) means no gluing at all.
fn glue_self_touching_edges(k: &mut Kernel, b: &mut Brep) -> Result<usize> {
    use std::collections::BTreeMap;

    use crate::topology::brep::p7::glue_edges_general;
    use crate::topology::edge;
    use crate::topology::edgeuse::p1 as eu1;

    let tol = b.tolerance();
    // Every edge with a curve and two vertices, by vertex pair.
    let mut touches: Vec<TouchingEdge> = Vec::new();
    let mut buckets: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for &e in b.edge_list() {
        let data = b.edge(e)?;
        if data.curve.is_none() {
            continue;
        }
        let Some(start) = edge::start_vertex(b, e)? else {
            continue;
        };
        let Some(end) = edge::other_vertex(b, e, Some(start))? else {
            continue;
        };
        if start == end {
            continue;
        }
        let (a, z) = (start.index(), end.index());
        let ends = (a.min(z), a.max(z));
        buckets.entry(ends).or_default().push(touches.len());
        touches.push(TouchingEdge {
            edge: e,
            start,
            ends,
            shell: None,
            faces: Vec::new(),
            crack: false,
            kind: None,
        });
    }
    let crowded: Vec<Vec<usize>> = buckets.into_values().filter(|v| v.len() > 1).collect();
    // The shells, faces and curve kinds of the edges that share their vertices.
    for &i in crowded.iter().flatten() {
        let Some(touch) = touches.get_mut(i) else {
            continue;
        };
        let data = b.edge(touch.edge)?;
        let Some(curve) = data.curve.as_ref() else {
            continue;
        };
        touch.kind = Some(core::mem::discriminant(curve.kind()));
        for &eu in &data.edgeuses {
            let Some(fu) = eu1::faceuse(b, eu)? else {
                continue;
            };
            if let Some(f) = b.faceuse(fu)?.face {
                touch.faces.push(f);
            }
            if touch.shell.is_none() {
                touch.shell = eu1::faceuse_shell(b, fu)?;
            }
        }
        // each face of a manifold edge holds two of its uses (an edge use and its mate)
        let uses = touch.faces.len();
        touch.faces.sort_unstable();
        touch.faces.dedup();
        touch.crack = uses > 2 * touch.faces.len();
    }
    // Whether the two edges' curves are proved within `tol` of each other.
    let coincide = |b: &Brep, x: EdgeId, y: EdgeId| -> Result<bool> {
        let (dx, dy) = (b.edge(x)?, b.edge(y)?);
        let (Some(cx), Some(cy)) = (dx.curve.as_ref(), dy.curve.as_ref()) else {
            return Ok(false);
        };
        // written so that a NaN bound or tolerance refuses
        Ok(coincidence_bound(cx, &dx.interval, cy, &dy.interval).is_some_and(|d| d <= tol))
    };
    let pairs = copy_pairs(&touches, &crowded, |x, y| coincide(b, x, y))?;
    glue_all_or_none(
        k,
        b,
        &pairs,
        |k, b, &(surviving, to_delete, orientation), env| {
            glue_edges_general(
                k,
                b,
                Some(surviving),
                orientation,
                0.0,
                true,
                Some(to_delete),
                None,
                env,
            )
        },
    )
}

/// One pair to glue: `(surviving, to_delete, orientation)`.
type GluePair = (
    EdgeId,
    EdgeId,
    crate::nurbs::b_spline_surface_n_lib::p2::OrientType,
);

/// Runs `glue` on every pair, all or nothing ([`glue_self_touching_edges`]'s errors): the
/// operand, `k.rng` and `k.upstream_ub_passed` are saved first, and when a pair fails they
/// are all put back exactly, so the merge then runs as on main. An ordinary failure
/// ([`crate::error::native_failure`]) returns 0; a port defect or a cancellation is returned.
/// No snapshot ([`Brep::snapshot`]) means nothing is glued. Returns the number of pairs
/// glued.
///
/// # Errors
///
/// The failures of `glue` that stand for no upstream status; those of restoring the
/// operand's attributes after a gluing that succeeded.
fn glue_all_or_none(
    k: &mut Kernel,
    b: &mut Brep,
    pairs: &[GluePair],
    mut glue: impl FnMut(
        &mut Kernel,
        &mut Brep,
        &GluePair,
        &mut crate::topology::edge::NotifyEnv<'_>,
    ) -> Result<()>,
) -> Result<usize> {
    use crate::topology::brep::p2::ArenaNotifyState;

    if pairs.is_empty() {
        return Ok(0);
    }
    let Ok(saved) = b.snapshot() else {
        return Ok(0);
    };
    let (rng, ub) = (k.rng.clone(), k.upstream_ub_passed.clone());
    let mut state = ArenaNotifyState::with_attributes_of(b);
    let mut failure = None;
    for pair in pairs {
        if let Err(e) = glue(k, b, pair, &mut state.env(None)) {
            failure = Some(e);
            break;
        }
    }
    if let Some(e) = failure {
        // the snapshot holds the attributes too: the store moved out is dropped
        drop(state);
        b.restore(saved);
        k.rng = rng;
        k.upstream_ub_passed = ub;
        // a port defect or a cancellation goes on; an ordinary failure merges as given
        crate::error::native_failure::<()>(Err(e))?;
        return Ok(0);
    }
    state.restore_attributes(b)?;
    b.refresh_cached_topology();
    Ok(pairs.len())
}

/// The body shared by the two-B-rep entry points after validation: build the merge
/// (`SmMerge sMerge(*pContext, pBrep1, pBrep2)`, or with `0, 0, &sOptions` when
/// `options` is given), then run `ManifoldBoolean` when both operands are manifold
/// solids (`SmBrep::IsManifoldSolid`) and `NonManifoldBoolean` otherwise.
fn merge_boolean(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
    options: Option<MergeOptions>,
) -> Result<BooleanOutcome> {
    merge_boolean_by(k, brep1, brep2, operation, options, MergeMethod::ByOperands)
        .map(|(outcome, _)| outcome)
}

/// [`merge_boolean`] running `method`, with the merge's common edges on the first
/// B-rep (`SmMerge::m_vEdges`) as it left them.
fn merge_boolean_by(
    k: &mut Kernel,
    mut brep1: Brep,
    mut brep2: Brep,
    operation: BooleanOperation,
    options: Option<MergeOptions>,
    method: MergeMethod,
) -> Result<(BooleanOutcome, Vec<EdgeId>)> {
    // SmMerge sMerge(*pContext, pBrep1, pBrep2) (or 0, 0, &sOptions): tolerance and
    // angle 0, both computed by the constructor.
    use crate::boolean::merge::p3::BooleanDriver;
    // (not upstream: WR-754) the operands' self-touch edge copies glued first
    glue_self_touching_edges(k, &mut brep1)?;
    glue_self_touching_edges(k, &mut brep2)?;
    // (not upstream) the driver carries the operands' attributes through the merge, as
    // upstream's live on the objects the merge edits
    let mut driver = crate::boolean::merge::p3::DependencyMerge::carrying_attributes();
    let mut primary = Some(brep1);
    let mut other = Some(brep2);
    driver.construct(k, &mut primary, &mut other, 0.0, 0.0, options.as_ref())?;
    let manifold_solids = match (&primary, &other) {
        (Some(_), Some(_)) if method == MergeMethod::NonManifold => false,
        (Some(a), Some(b)) => is_manifold_solid(a)? && is_manifold_solid(b)?,
        _ => return Err(SolidError::invalid_input(OP_BOOLEAN, "null operand")),
    };
    // m_bManifoldBoolean and m_vTI's lamina-edge control of the constructed merge
    let mut manifold = true;
    let mut lamina_edges = 0;
    let result = if manifold_solids {
        driver.manifold_boolean(
            k,
            &mut primary,
            &mut other,
            &mut manifold,
            &mut lamina_edges,
            operation,
            None,
        )?
    } else {
        crate::boolean::merge::p3::non_manifold_boolean(
            k,
            &mut driver,
            &mut primary,
            &mut other,
            &mut manifold,
            &mut lamina_edges,
            operation,
        )?
    };
    let mut result = result.ok_or_else(|| {
        SolidError::entity_missing(OP_BOOLEAN, "the Boolean made no result B-rep")
    })?;
    // (not upstream: the port's cached topology fields, true again for the caller)
    result.refresh_cached_topology();
    if let Some(o) = other.as_mut() {
        o.refresh_cached_topology();
    }
    let edges = driver
        .engine()
        .map(|engine| engine.state.edges.clone())
        .unwrap_or_default();
    Ok((BooleanOutcome { result, other }, edges))
}

fn is_manifold_solid(brep: &Brep) -> Result<bool> {
    crate::topology::brep::p2::is_manifold_solid(brep)
}

/// Booleans two B-reps (`SmApiBoolean`): `brep1` is modified into the result; `brep2`
/// is consumed (returned in [`BooleanOutcome::other`] for the operations upstream keeps
/// it for).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `SM_BO_EXTRACT_SEPARATE`; otherwise the merge's
/// error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
) -> Result<BooleanOutcome> {
    validate_boolean_operands(operation)?;
    merge_boolean(k, brep1, brep2, operation, None)
}

/// Unites two B-reps (`SmApiBooleanUnion`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_union(k: &mut Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Union)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Union, None).map(|o| o.result)
}

/// Subtracts `brep2` from `brep1` (`SmApiBooleanDifference`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_difference(k: &mut Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Difference)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Difference, None).map(|o| o.result)
}

/// Intersects two B-reps (`SmApiBooleanIntersection`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_intersection(k: &mut Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Intersection)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Intersection, None).map(|o| o.result)
}

/// Merges `brep2` into `brep1` (`SmApiBooleanMerge`); `brep2` is consumed.
///
/// # Errors
///
/// The merge's error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_merge(k: &mut Kernel, brep1: Brep, brep2: Brep) -> Result<Brep> {
    validate_boolean_operands(BooleanOperation::Merge)?;
    merge_boolean(k, brep1, brep2, BooleanOperation::Merge, None).map(|o| o.result)
}

/// Applies one boolean operation across a list of at least two B-reps
/// (`SmApiMergeBreps`). The list is validated first and left untouched on rejection;
/// once validation passes the B-reps are consumed (the list is drained), as upstream.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for fewer than two B-reps or an operation other than
/// union, intersection, difference or merge; otherwise the merge's error
/// (including [`SolidError::Unported`] for missing merge-engine dependencies).
#[allow(clippy::ptr_arg)] // the list is drained on success, as upstream consumes it
pub fn merge_breps(
    k: &mut Kernel,
    breps: &mut Vec<Brep>,
    operation: BooleanOperation,
) -> Result<Brep> {
    // The output is valid only on success. Reject every unsafe list shape before the
    // kernel has a chance to consume it.
    let kernel_operation = match merge_breps_kernel_operation(operation) {
        Some(op) if merge_breps_inputs_are_valid(breps) => op,
        _ => {
            return Err(SolidError::invalid_input(
                OP_MERGE_BREPS,
                "fewer than two B-reps or an operation other than union, intersection, difference or merge",
            ))
        }
    };
    // `SmMerge::merge_breps(rBreps, lKernelOperation, rpResult)`, then `SM_ERR` when it
    // succeeds without a result.
    let mut result = crate::boolean::merge::p3::merge_breps(
        k,
        breps,
        kernel_operation,
        None,
        &mut crate::boolean::merge::p3::DependencyMerge::carrying_attributes(),
    )?;
    // (not upstream: the port's cached topology fields, true again for the caller)
    result.refresh_cached_topology();
    Ok(result)
}

/// Booleans two B-reps with full merge-option control (`SmApiBooleanWithOptions`):
/// cookie cutter, imprinting, post-processing, imprint-and-classify and keeping the
/// second operand.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `SM_BO_EXTRACT_SEPARATE`; otherwise the merge's
/// error ([`SolidError::Unported`] until `SmMerge` is ported).
pub fn boolean_with_options(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
    options: MergeOptions,
) -> Result<BooleanOutcome> {
    validate_boolean_operands(operation)?;
    merge_boolean(k, brep1, brep2, operation, Some(options))
}

/// Booleans two B-reps and returns the intersection edges on the result
/// (`SmApiBooleanWithCurves`): as [`boolean`], plus `SmMerge::m_vEdges`, the common
/// edges the merge found on the first B-rep, in its order.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `SM_BO_EXTRACT_SEPARATE`; otherwise the merge's
/// error.
pub fn boolean_with_curves(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
) -> Result<BooleanWithCurves> {
    validate_boolean_operands(operation)?;
    let (outcome, edges) =
        merge_boolean_by(k, brep1, brep2, operation, None, MergeMethod::ByOperands)?;
    Ok(BooleanWithCurves {
        outcome,
        intersection_edges: edges,
    })
}

/// Booleans two B-reps with `NonManifoldBoolean` whatever their kind
/// (`SmApiNonManifoldBoolean`): `brep1` is modified into the result; `brep2` is
/// consumed (returned in [`BooleanOutcome::other`] for the operations upstream keeps it
/// for).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `SM_BO_EXTRACT_SEPARATE`; otherwise the merge's
/// error.
pub fn non_manifold_boolean(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: BooleanOperation,
) -> Result<BooleanOutcome> {
    validate_boolean_operands(operation)?;
    merge_boolean_by(k, brep1, brep2, operation, None, MergeMethod::NonManifold)
        .map(|(outcome, _)| outcome)
}

/// Merges each face of `brep2` into `brep1` in turn (`SmApiPiecewiseMerge`,
/// `SmMerge::PiecewiseMerge(NULL, FALSE, bCreateNewBrepsForFaces, rpResult)`), each
/// through a stand-alone sheet B-rep when `create_new_breps_for_faces`. `brep2` is
/// imprinted, not deleted.
///
/// # Errors
///
/// The merge's error; `SM_ERR` if the merge's result were not `brep1` (upstream's
/// `PiecewiseMerge` always returns its first operand, so this does not happen).
pub fn piecewise_merge(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    create_new_breps_for_faces: bool,
) -> Result<PiecewiseMergeOutcome> {
    use crate::sweep::topology_sweep::access::SweepAccess;
    validate_boolean_operands(BooleanOperation::PartialMerge)?;
    let mut model = PrimitiveModel::new(k, brep1);
    let other_id = model.add_brep(brep2)?;
    // SmMerge sMerge(*pContext, pBrep1, pBrep2): tolerance and angle 0, both computed
    let merge = model.new_merge(
        PrimitiveModel::MAIN,
        PrimitiveModel::MAIN,
        other_id,
        0.0,
        0.0,
    )?;
    let merged = model.piecewise_merge(merge, false, create_new_breps_for_faces);
    model.delete_merge(merge);
    let result_id = merged?;
    if result_id != PrimitiveModel::MAIN {
        // PiecewiseMerge's result is its first operand
        return Err(SolidError::stage_failed(
            OP_PIECEWISE,
            "the piecewise merge did not return its first operand",
        ));
    }
    let mut other = model.take_brep(other_id)?;
    let mut result = model.into_brep()?;
    // (not upstream: the port's cached topology fields, true again for the caller)
    result.refresh_cached_topology();
    other.refresh_cached_topology();
    Ok(PiecewiseMergeOutcome { result, other })
}

/// The planar 2D boolean of two B-reps whose faces all lie on one plane
/// (`SmApiBoolean2d`, `SmPrimitiveCreation::Boolean2D`): union, intersection, A minus
/// B, exclusive or or merge of their regions. On success with faces on both sides the
/// first operand is modified into the result and the second consumed; an operand
/// without faces leaves both unchanged ([`Boolean2dOutcome::Unassigned`]). The first
/// operand's attributes ([`Brep::add_attribute`]) carry through to the result.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for surfaces that are not coincident planes; those of
/// the boolean.
pub fn boolean_2d(
    k: &mut Kernel,
    brep1: Brep,
    brep2: Brep,
    operation: Boolean2d,
) -> Result<Boolean2dOutcome> {
    use crate::topology::topology_traverser::Marks;
    let mut primary = brep1;
    let mut other = brep2;
    let mut primary_marks = Marks::default();
    let mut other_marks = Marks::default();
    let mut result = None;
    {
        // (not upstream) with the primary operand's own attributes, given back to the
        // result afterwards, as upstream's live on the objects Boolean2D splits, glues
        // and keeps; the other operand is notified under identities disjoint from the
        // primary's, so that its edits never reach the primary's attributes
        let mut state = ArenaNotifyState::with_attributes_of(&mut primary);
        state.seed_tagged(&other);
        let r = PrimitiveCreation::boolean_2d_keyed(
            k,
            &mut primary,
            &mut other,
            operation,
            &mut result,
            (&mut primary_marks, &mut other_marks),
            &mut state.env(None),
            Some(&crate::topology::brep::p2::copy_source_ref),
        );
        let restored = state.restore_attributes(result.as_mut().unwrap_or(&mut primary));
        r?;
        restored?;
    }
    Ok(match result {
        Some(mut result) => {
            // (not upstream: the port's cached topology fields, true again for the caller)
            result.refresh_cached_topology();
            Boolean2dOutcome::Result(result)
        }
        None => Boolean2dOutcome::Unassigned {
            brep1: primary,
            brep2: Box::new(other),
        },
    })
}

/// Booleans two lists of B-reps or surfaces (`SmApiBooleanLists`, wrapping
/// `SmMerge::BooleanLists`). `operation` is `0` intersect, `1` union, `2` exclusive
/// or, `3` A minus B, `4` B minus A. The inputs are not modified.
///
/// # Errors
///
/// The first dependency error reached by `merge::p3::boolean_lists`.
pub fn boolean_lists(
    k: &mut Kernel,
    breps1: &[Brep],
    surfaces1: &[Surface],
    breps2: &[Brep],
    surfaces2: &[Surface],
    operation: i32,
) -> Result<BooleanListsResult> {
    let mut out = crate::boolean::merge::p3::boolean_lists(
        k, breps1, surfaces1, breps2, surfaces2, operation, None,
    )?;
    // (not upstream: the port's cached topology fields, true again for the caller)
    for b in &mut out.breps {
        b.refresh_cached_topology();
    }
    Ok(out)
}

/// Combines B-reps by CSG trees in postfix notation (`SmApiBooleanTrees`, wrapping
/// `SmMerge::BooleanTrees`): a node `>= 0` is an index into `breps`, `-1` to `-6` are
/// union, intersection, difference, merge, exclusive or and slice. On success
/// `breps` holds one result per tree followed by the unused B-reps.
///
/// # Errors
///
/// Dependency errors, or `InvalidInput` for an out-of-range executed leaf or
/// a binary operation that would dereference a null operand upstream.
pub fn boolean_trees(
    k: &mut Kernel,
    breps: &mut Vec<Option<Brep>>,
    post_fix_trees: &[i64],
) -> Result<()> {
    crate::boolean::merge::p3::boolean_trees(
        k,
        breps,
        post_fix_trees,
        None,
        &mut crate::boolean::merge::p3::DependencyMerge::carrying_attributes(),
    )?;
    // (not upstream: the port's cached topology fields, true again for the caller)
    for b in breps.iter_mut().flatten() {
        b.refresh_cached_topology();
    }
    Ok(())
}

/// `SmApiTessellateBoundaries`: the edges of `b` as polylines within
/// `angle_tol_deg` ([`brep_p1::tessellate_boundaries`], `SmBrep::TessellateBoundaries`).
///
/// # Errors
///
/// Those of [`brep_p1::tessellate_boundaries`].
pub fn tessellate_boundaries(
    k: &Kernel,
    b: &Brep,
    angle_tol_deg: f64,
) -> Result<BoundaryTessellation> {
    brep_p1::tessellate_boundaries(k, b, angle_tol_deg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::context::MarkType;

    /// A geometry curve over a cubic NURB on `knots` with the control points `pts`.
    fn cubic(pts: &[[f64; 3]], knots: Vec<f64>) -> crate::curve::curve::p2::Curve {
        use crate::curve::curve::p2::{Curve, CurveKind};
        use crate::nurbs::b_spline_curve::p1::BSplineCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let pw = pts
            .iter()
            .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
            .collect();
        let nurb = crate::nurbs::crv_basic::Curve::new(pw, 3, knots).unwrap();
        Curve::new(CurveKind::BSpline(BSplineCurve::from_nurb(nurb, 3, false)))
    }

    fn segment(a: [f64; 3], z: [f64; 3]) -> crate::curve::curve::p2::Curve {
        use crate::curve::curve::p2::{Curve, CurveKind};
        let p = |v: [f64; 3]| crate::math::Point3::new(v[0], v[1], v[2]);
        Curve::new(CurveKind::Line(crate::curve::line::Line::from_points(
            p(a),
            p(z),
            3,
        )))
    }

    /// What [`glue_all_or_none`] must put back: the edge list and every edge's uses,
    /// tolerance and interval, and the counts of faces and vertices.
    fn fingerprint(b: &Brep) -> String {
        let edges: Vec<String> = b
            .edge_list()
            .iter()
            .map(|&e| {
                let d = b.edge(e).unwrap();
                format!("{e:?} {:?} {} {:?}", d.edgeuses, d.tolerance, d.interval)
            })
            .collect();
        format!("{edges:?} {} {}", b.faces().len(), b.vertices().len())
    }

    fn unit_box(k: &mut Kernel) -> Brep {
        crate::primitive::api_primitives::create_box(k, crate::math::Point3::ZERO, 1.0, 1.0, 1.0)
            .unwrap()
    }

    /// A gluing whose second pair fails after the first changed the operand and drew from
    /// the kernel's rng: the operand, the rng and the upstream-UB list are put back exactly.
    /// An ordinary failure (`InvalidInput`) then lets the merge go on (0 glued); a failure
    /// that stands for no upstream status (`NotFound`, a stale id) is returned.
    #[test]
    fn a_failed_gluing_restores_the_operand_and_the_rng() {
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        let stale = SolidError::NotFound {
            operation: "test",
            entity: crate::error::EntityRef::Edge(EdgeId::from_index(9999)),
        };
        for (failure, propagates) in [
            (SolidError::invalid_input("test", "a broken link"), false),
            (stale, true),
        ] {
            let mut k = Kernel::default();
            let mut b = unit_box(&mut k);
            let (before, rng, ub) = (fingerprint(&b), k.rng.clone(), k.upstream_ub_passed.clone());
            let e = b.edge_list()[0];
            let pairs = [(e, e, OrientType::Same), (e, e, OrientType::Same)];
            let mut failure = Some(failure);
            let mut first = true;
            let r = glue_all_or_none(&mut k, &mut b, &pairs, |k, b, &(e, _, _), _| {
                if first {
                    first = false;
                    b.edge_mut(e)?.tolerance = 7.0;
                    let _ = k.rng.next_u64();
                    k.note_upstream_ub("test");
                    return Ok(());
                }
                Err(failure.take().unwrap())
            });
            assert_eq!(r.is_err(), propagates, "{r:?}");
            if !propagates {
                assert_eq!(r.unwrap(), 0);
            }
            assert_eq!(fingerprint(&b), before);
            assert_eq!(k.rng, rng);
            assert_eq!(k.upstream_ub_passed, ub);
        }
    }

    /// The real `GlueEdgesGeneral` through [`glue_all_or_none`]: two box edges that do not
    /// share both vertices fail its precondition (an ordinary failure: nothing glued, the
    /// operand as given), and a stale edge id is returned as an error, the operand again as
    /// given.
    #[test]
    fn glue_edges_general_failures_leave_the_operand_as_given() {
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::topology::brep::p7::glue_edges_general;
        let mut k = Kernel::default();
        let mut b = unit_box(&mut k);
        let before = fingerprint(&b);
        let list = b.edge_list().to_vec();
        let (e0, far) = (
            list[0],
            *list
                .iter()
                .find(|&&f| {
                    let ends = |x| {
                        let s = crate::topology::edge::start_vertex(&b, x).unwrap();
                        (s, crate::topology::edge::other_vertex(&b, x, s).unwrap())
                    };
                    let ((a, z), (c, d)) = (ends(list[0]), ends(f));
                    a != c && a != d && z != c && z != d
                })
                .unwrap(),
        );
        let glue = |k: &mut Kernel,
                    b: &mut Brep,
                    &(s, d, o): &GluePair,
                    env: &mut crate::topology::edge::NotifyEnv<'_>| {
            glue_edges_general(k, b, Some(s), o, 0.0, true, Some(d), None, env)
        };
        let r = glue_all_or_none(&mut k, &mut b, &[(e0, far, OrientType::Same)], glue);
        assert_eq!(r.unwrap(), 0);
        assert_eq!(fingerprint(&b), before);
        let r = glue_all_or_none(
            &mut k,
            &mut b,
            &[(e0, EdgeId::from_index(9999), OrientType::Same)],
            glue,
        );
        assert!(r.is_err(), "{r:?}");
        assert_eq!(fingerprint(&b), before);
    }

    /// A [`TouchingEdge`] for [`copy_pairs`]: edge `e` between vertices 1 and 2 on `faces`
    /// in shell 0, a line.
    fn touching(e: u32, faces: &[u32], crack: bool) -> TouchingEdge {
        use crate::topology::{FaceId, ShellId, VertexId};
        let line = segment([0.0; 3], [1.0, 0.0, 0.0]);
        TouchingEdge {
            edge: EdgeId::from_index(e),
            start: VertexId::from_index(1),
            ends: (1, 2),
            shell: Some(ShellId::from_index(0)),
            faces: faces.iter().map(|&f| FaceId::from_index(f)).collect(),
            crack,
            kind: Some(core::mem::discriminant(line.kind())),
        }
    }

    fn glued(touches: &[TouchingEdge], coincide: bool) -> Vec<(u32, u32)> {
        let bucket: Vec<usize> = (0..touches.len()).collect();
        copy_pairs(touches, &[bucket], |_, _| Ok(coincide))
            .unwrap()
            .into_iter()
            .map(|(a, b, _)| (a.index(), b.index()))
            .collect()
    }

    /// Three coincident edges in one bucket: the copies are checked against every face
    /// already glued, not only the first edge's, so no face ends up on both sides of the
    /// glued edge. Crack edges (a face on both sides) are never glued; true copies on distinct
    /// faces all glue into the first; unproved geometry glues nothing.
    #[test]
    fn copies_are_checked_against_every_face_already_glued() {
        // two copies on face 5 and a first edge on faces 1, 2: only one copy on face 5 joins
        let t = [
            touching(0, &[1, 2], false),
            touching(1, &[5, 6], false),
            touching(2, &[5, 7], false),
        ];
        assert_eq!(glued(&t, true), vec![(0, 1)]);
        // three copies on distinct faces all glue into the first
        let t = [
            touching(0, &[1, 2], false),
            touching(1, &[3, 4], false),
            touching(2, &[5, 6], false),
        ];
        assert_eq!(glued(&t, true), vec![(0, 1), (0, 2)]);
        // a crack edge (face 5 on both sides) is not glued, as first or as copy
        let t = [touching(0, &[5], true), touching(1, &[3, 4], false)];
        assert_eq!(glued(&t, true), Vec::<(u32, u32)>::new());
        let t = [touching(0, &[3, 4], false), touching(1, &[5], true)];
        assert_eq!(glued(&t, true), Vec::<(u32, u32)>::new());
        // a shared face with the first edge, or no coincidence proof: nothing
        let t = [touching(0, &[1, 2], false), touching(1, &[2, 3], false)];
        assert_eq!(glued(&t, true), Vec::<(u32, u32)>::new());
        let t = [touching(0, &[1, 2], false), touching(1, &[3, 4], false)];
        assert_eq!(glued(&t, false), Vec::<(u32, u32)>::new());
    }

    /// Two lines between the same ends, either way round, are within rounding of each other;
    /// a line whose far end is 3e-6 off is bounded by at least that (the bound is sound) and
    /// barely more (it is tight), so with a tolerance of 1e-6 it is not glued.
    #[test]
    fn the_coincidence_bound_of_two_lines_is_their_end_distance() {
        let ivl = crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0);
        let a = segment([1.0, 2.0, 3.0], [11.0, 2.0, 3.0]);
        let same = segment([1.0, 2.0, 3.0], [11.0, 2.0, 3.0]);
        let reversed = segment([11.0, 2.0, 3.0], [1.0, 2.0, 3.0]);
        for b in [&same, &reversed] {
            let d = coincidence_bound(&a, &ivl, b, &ivl).unwrap();
            assert!(d <= 1e-13, "{d}");
        }
        let off = segment([1.0, 2.0, 3.0], [11.0, 2.0 + 3e-6, 3.0]);
        let d = coincidence_bound(&a, &ivl, &off, &ivl).unwrap();
        assert!((3e-6..=3e-6 + 1e-13).contains(&d), "{d}");
        assert!(d > 1e-6);
        // a NaN end has no bound
        let nan = segment([1.0, 2.0, 3.0], [f64::NAN, 2.0, 3.0]);
        assert!(coincidence_bound(&a, &ivl, &nan, &ivl).is_none());
    }

    /// The real defect three samples miss: two cubics between the same ends, one straight and
    /// one with a bump of 1e-4 confined to its first eighth (one control point lifted, whose
    /// basis function vanishes outside [0, 1/8]). At t = 1/4, 1/2 and 3/4 they agree exactly,
    /// which the sampled check accepted; the bound over the whole span is at least the bump's
    /// height there, so the pair is not glued. A copy of the straight cubic is within the bound's rounding allowance.
    #[test]
    fn a_bump_between_the_samples_is_not_coincident() {
        let knots = vec![
            0.0, 0.0, 0.0, 0.0, 0.0625, 0.125, 0.1875, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0,
        ];
        // the Greville abscissae, so the straight cubic is the line x = 10 t
        let greville: Vec<f64> = (0..10)
            .map(|i| (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0)
            .collect();
        let straight: Vec<[f64; 3]> = greville.iter().map(|&g| [10.0 * g, 0.0, 0.0]).collect();
        let mut bumped = straight.clone();
        bumped[1][1] = 1e-4;
        let (a, b) = (
            cubic(&straight, knots.clone()),
            cubic(&bumped, knots.clone()),
        );
        let k = Kernel::default();
        for t in [0.25, 0.5, 0.75] {
            let (p, q) = (
                a.evaluate_point(&k, t).unwrap(),
                b.evaluate_point(&k, t).unwrap(),
            );
            assert_eq!(p.distance(q), 0.0, "t = {t}");
        }
        let peak = (0..=1000)
            .map(|i| {
                let t = 0.125 * f64::from(i) / 1000.0;
                a.evaluate_point(&k, t)
                    .unwrap()
                    .distance(b.evaluate_point(&k, t).unwrap())
            })
            .fold(0.0, f64::max);
        let ivl = crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0);
        let d = coincidence_bound(&a, &ivl, &b, &ivl).unwrap();
        assert!(peak > 1e-5 && d >= peak, "bound {d}, peak {peak}");
        assert!(d > 1e-6);
        let copy = cubic(&straight, knots.clone());
        let d = coincidence_bound(&a, &ivl, &copy, &ivl).unwrap();
        // the hull bound charges the rounding of its refinement and elevation
        assert!(d <= 1e-11, "{d}");
        // on different intervals, or a line against a cubic, there is no bound
        let half = crate::geom::extent1d::Extent1d::from_min_max(0.0, 0.5);
        assert!(coincidence_bound(&a, &ivl, &copy, &half).is_none());
        let line = segment([0.0; 3], [10.0, 0.0, 0.0]);
        assert!(coincidence_bound(&a, &ivl, &line, &ivl).is_none());
    }

    /// `TestSmTessellateBoundaries` (`tests/SM_API_test/src/TestSmBrep.cpp:1328`): a
    /// box's 12 edges are 2 points each; an edge without curve has none and makes the
    /// result incomplete; an invalid angle tolerance is refused.
    #[test]
    fn tessellate_boundaries_of_a_box() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::ZERO,
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let t = tessellate_boundaries(&k, &b, 5.0).unwrap();
        assert!(t.complete);
        assert_eq!(t.edges.len(), 12);
        assert_eq!(t.vertex_counts, vec![2; 12]);
        assert_eq!(t.points.len(), 24);

        b.edge_mut(t.edges[0]).unwrap().curve = None;
        let t = tessellate_boundaries(&k, &b, 5.0).unwrap();
        assert!(!t.complete);
        assert_eq!(t.vertex_counts.len(), 12);
        assert_eq!(t.vertex_counts[0], 0);
        assert_eq!(t.points.len(), 22);

        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                tessellate_boundaries(&k, &b, bad),
                Err(SolidError::InvalidInput { .. })
            ));
        }
    }

    const ALL: [BooleanOperation; 11] = [
        BooleanOperation::Unknown,
        BooleanOperation::Union,
        BooleanOperation::Intersection,
        BooleanOperation::Difference,
        BooleanOperation::ExclusiveOr,
        BooleanOperation::Merge,
        BooleanOperation::PartialMerge,
        BooleanOperation::Imprint,
        BooleanOperation::ImprintClassify,
        BooleanOperation::ExtractSeparate,
        BooleanOperation::Slice,
    ];

    #[test]
    fn kernel_operation_mapping() {
        for op in ALL {
            let expect = match op {
                BooleanOperation::Union => Some(0),
                BooleanOperation::Intersection => Some(1),
                BooleanOperation::Difference => Some(2),
                BooleanOperation::Merge => Some(3),
                _ => None,
            };
            assert_eq!(merge_breps_kernel_operation(op), expect, "{op:?}");
        }
    }

    #[test]
    fn extract_separate_is_rejected() {
        let mut k = Kernel::default();
        let e = boolean(
            &mut k,
            Brep::new(),
            Brep::new(),
            BooleanOperation::ExtractSeparate,
        )
        .unwrap_err();
        assert!(matches!(e, SolidError::InvalidInput { .. }));
        let e = boolean_with_options(
            &mut k,
            Brep::new(),
            Brep::new(),
            BooleanOperation::ExtractSeparate,
            MergeOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(e, SolidError::InvalidInput { .. }));
    }

    /// Whether `call` reached the merge engine: it takes a mark (`SmNewMarkAndLock`),
    /// which the API's input validation never does. (On the empty operands of these
    /// tests its outcome is not compared with upstream.)
    fn reaches_merge<T>(k: &mut Kernel, call: impl FnOnce(&mut Kernel) -> Result<T>) -> bool {
        let next = k.context.current_mark(MarkType::NoMark);
        let _ = call(k);
        k.context.current_mark(MarkType::NoMark) != next
    }

    #[test]
    fn valid_operands_reach_the_merge() {
        let mut k = Kernel::default();
        for op in ALL {
            if op == BooleanOperation::ExtractSeparate {
                continue;
            }
            // The merge is built with a computed tolerance; the empty operands are not
            // manifold solids, and NonManifoldBoolean runs merge_p4's ManifoldBoolean on
            // the constructed engine.
            assert!(
                reaches_merge(&mut k, |k| boolean(k, Brep::new(), Brep::new(), op)),
                "{op:?}"
            );
        }
        assert!(reaches_merge(&mut k, |k| boolean_union(
            k,
            Brep::new(),
            Brep::new()
        )));
        assert!(reaches_merge(&mut k, |k| boolean_difference(
            k,
            Brep::new(),
            Brep::new()
        )));
        assert!(reaches_merge(&mut k, |k| boolean_intersection(
            k,
            Brep::new(),
            Brep::new()
        )));
        assert!(reaches_merge(&mut k, |k| boolean_merge(
            k,
            Brep::new(),
            Brep::new()
        )));
    }

    #[test]
    fn merge_breps_rejects_without_consuming() {
        let mut k = Kernel::default();
        for n in 0..2 {
            let mut breps: Vec<Brep> = (0..n).map(|_| Brep::new()).collect();
            let e = merge_breps(&mut k, &mut breps, BooleanOperation::Union).unwrap_err();
            assert!(matches!(e, SolidError::InvalidInput { .. }));
            assert_eq!(breps.len(), n);
        }
        for op in ALL {
            let mut breps: Vec<Brep> = (0..3).map(|_| Brep::new()).collect();
            if merge_breps_kernel_operation(op).is_some() {
                // The merge is constructed and runs.
                assert!(
                    reaches_merge(&mut k, |k| merge_breps(k, &mut breps, op)),
                    "{op:?}"
                );
            } else {
                let e = merge_breps(&mut k, &mut breps, op).unwrap_err();
                assert!(matches!(e, SolidError::InvalidInput { .. }), "{op:?}");
                assert_eq!(breps.len(), 3);
            }
        }
    }

    #[test]
    fn lists_and_trees_reach_the_merge() {
        let mut k = Kernel::default();
        let e = boolean_lists(&mut k, &[Brep::new()], &[], &[Brep::new()], &[], 1).unwrap_err();
        // the list operands are copied (topology.brep_p3) and the merge engine runs
        // (topology.brep_p4 is ported); on empty operands it fails in the engine
        assert!(
            !e.is_unported() && e.operation().starts_with("boolean::merge"),
            "{e:?}"
        );
        let mut breps = vec![Some(Brep::new()), Some(Brep::new())];
        let tree = vec![0, 1, -1];
        // The merge is constructed and runs.
        assert!(reaches_merge(&mut k, |k| boolean_trees(
            k, &mut breps, &tree
        )));
    }
    #[test]
    fn tree_api_preserves_defined_null_outputs_and_failure_slots() {
        let mut k = Kernel::default();
        let mut breps = vec![Some(Brep::new()), Some(Brep::new())];
        boolean_trees(&mut k, &mut breps, &[0, 0]).unwrap();
        assert_eq!(breps.len(), 3);
        assert!(breps.first().unwrap().is_some());
        assert!(breps.get(1).unwrap().is_none());
        assert!(breps.get(2).unwrap().is_some());
        let mut breps = vec![Some(Brep::new()), Some(Brep::new())];
        assert!(boolean_trees(&mut k, &mut breps, &[0, -1]).is_err());
        assert!(breps.first().unwrap().is_none());
        assert!(breps.get(1).unwrap().is_some());
    }
}
