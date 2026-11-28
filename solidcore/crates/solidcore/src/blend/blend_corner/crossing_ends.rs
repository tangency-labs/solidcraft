//! Chamfers whose Ends meet across an unblended edge, built in two passes.
//!
//! Two chamfered edges `E_i` and `E_j` on a face `A` end at the two vertices `V_i`,
//! `V_j` of an edge `e` of `A` that is not chamfered (a box's top edges after its upright
//! edges were chamfered: `e` is the short top edge of an upright chamfer `F`). Each End is
//! the ordinary single-edge End on `F`, built per vertex. Where the two bands reach across
//! `e` past each other, the two End ribs cross on `F` and the two rails cross on `A`: the
//! result needs a vertex on `F` where both chamfer planes meet it, and an edge between the
//! chamfers from there to the rails' crossing, which no per-vertex End makes.
//!
//! Upstream: none. A chamfer here is a plane between two planes: near the corner it keeps
//! the half-space on the material's side of its plane, and the part it leaves is the
//! intersection of the part with those half-spaces. Intersections commute, so the edges
//! chamfered in two passes, no two edges of one pass meeting this way, build the same
//! solid as the joint chamfer would: each End of the second pass is the single-edge End
//! on the first pass's chamfer, the chamfer planes meeting along their common line
//! (#17026; the box of `solidcore-ops/tests/blend_chamfer_corner.rs` matches the closed
//! form of its volume in either order).
//!
//! Where the bands meet. Along `e`, from `V_i` towards `V_j` (unit `u`, length `L`), the
//! band of `E_i` on `A` reaches `d_i / sin(theta_i)`, `theta_i` the angle between `E_i` and
//! `e` at `V_i` and `d_i` the chamfer's setback on `A`. The two bands overlap across `e`
//! where `d_i / sin(theta_i) + d_j / sin(theta_j) > L`, past the body's tolerance; at
//! equality the short edge collapses and the joint construction builds it. The pair is
//! taken only where the overlap is local to the corner: each edge runs away from the
//! other's band (its direction has a positive part along the other band's inward normal
//! in `A`), so parallel or converging edges (a thin wall's two top edges, whose bands
//! overlap along their whole length) stay with the joint construction and its refusal.
//!
//! Only a selection of straight convex edges between two planes is planned, each at its
//! own distance on both faces. A meeting end is a vertex of three edges (the picked edge,
//! `e` and a side edge), with `A`'s corner convex there; `e` is straight and not picked,
//! its other face a plane. Picked edges that share a vertex go in one pass, so every corner the joint
//! construction would build between picked edges is built by it in that pass. Meeting
//! pairs must split the picks into two passes (an odd ring of them does not, and is left to
//! the joint construction). With no meeting pair the plan is
//! [`CrossingEndPlan::NotApplicable`].
//!
//! Deviations: not a port. No recursion: the grouping is a breadth-first walk over at most
//! the picked edges.

use crate::blend::blend_corner::mixed::leaving;
use crate::blend::fillet_overflow::outward_plane;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::blend::BlendFailure;
use crate::error::topology::TopologyFailure;
use crate::error::{EntityRef, Failure, Result, SolidError};
use crate::geom::attribute::{ids, Attribute, AttributeHandlers, Behavior};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::math::gamma;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::face::p2 as face2;
use crate::topology::{edge, vertex, Brep, EdgeId, FaceId, VertexId};

const OP: &str = "blend::blend_corner::crossing_ends::plan";

/// How to build a chamfer selection whose Ends may meet across an unblended edge.
pub enum CrossingEndPlan {
    /// No Ends meet that way: the ordinary joint construction.
    NotApplicable,
    /// The two passes, each built with the ordinary construction.
    Passes(CrossingEndPasses),
}

/// The picked edges, with their distances, of each of the two passes.
pub struct CrossingEndPasses {
    first: Vec<(EdgeId, f64)>,
    second: Vec<(EdgeId, f64)>,
}

// A temporary kernel attribute below the applications' range, the ops overflow mark and
// the separate Ends' trailing mark.
const SECOND_ID: u64 = ids::USER_1 - 3;

#[derive(Debug, Clone, PartialEq)]
struct Second(usize);

impl AttributeHandlers for Second {}

fn reserve<T>(vec: &mut Vec<T>, count: usize) -> Result<()> {
    vec.try_reserve_exact(count)
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))
}

/// A picked edge as the plan reads it: a straight edge between two planes.
struct Pick {
    edge: EdgeId,
    size: f64,
    ends: [VertexId; 2],
    faces: [FaceId; 2],
}

/// A candidate meeting: picks `i` and `j` (indices), the vertex of each at the unpicked
/// edge `e` between them, and the face `A` they share along `e`.
struct Candidate {
    picks: [usize; 2],
    at: [VertexId; 2],
    e: EdgeId,
    face: FaceId,
}

fn is_plane(b: &Brep, face: FaceId) -> Result<bool> {
    Ok(matches!(
        face2::surface(b, face)?.map(Surface::kind),
        Some(SurfaceKind::Plane(_))
    ))
}

fn straight(b: &Brep, e: EdgeId) -> Result<bool> {
    Ok(edge::is_manifold(b, e)?
        && matches!(
            edge::curve(b, e)?.map(Curve::kind),
            Some(CurveKind::Line(_))
        ))
}

fn pick(b: &Brep, edge: EdgeId, size: f64) -> Result<Option<Pick>> {
    if !(size.is_finite() && size > 0.0 && straight(b, edge)?) {
        return Ok(None);
    }
    let (Some(start), Some(end)) = edge::start_end_vertices(b, edge)? else {
        return Ok(None);
    };
    if start == end {
        return Ok(None);
    }
    let faces = edge::faces(b, edge, None)?;
    let [a, z] = faces[..] else {
        return Ok(None);
    };
    if a == z || !is_plane(b, a)? || !is_plane(b, z)? {
        return Ok(None);
    }
    Ok(Some(Pick {
        edge,
        size,
        ends: [start, end],
        faces: [a, z],
    }))
}

/// The pick's face other than `face`.
fn other_face(pick: &Pick, face: FaceId) -> Option<FaceId> {
    match pick.faces {
        [a, z] if a == face => Some(z),
        [a, z] if z == face => Some(a),
        _ => None,
    }
}

/// Every candidate meeting of picks `i` and `j`, from topology alone, added to `found`: an
/// end of each joined by an unpicked straight edge `e`, each end a vertex of three edges,
/// `e` on a face both picks lie on, its other face a plane. Every such connector is a
/// candidate, since which of them the bands reach across does not follow from topology and
/// must not depend on the edges' orientations.
fn candidates_of(
    b: &Brep,
    picks: &[Pick],
    i: usize,
    j: usize,
    found: &mut Vec<Candidate>,
) -> Result<()> {
    let (Some(p), Some(q)) = (picks.get(i), picks.get(j)) else {
        return Ok(());
    };
    for vi in p.ends {
        for vj in q.ends {
            if vi == vj
                || vertex::edges(b, vi, None)?.len() != 3
                || vertex::edges(b, vj, None)?.len() != 3
            {
                continue;
            }
            for e in vertex::common_edges(b, vi, Some(vj), None)? {
                if picks.iter().any(|other| other.edge == e) || !straight(b, e)? {
                    continue;
                }
                let faces = edge::faces(b, e, None)?;
                let [f, g] = faces[..] else {
                    continue;
                };
                for (face, end_face) in [(f, g), (g, f)] {
                    if other_face(p, face).is_some()
                        && other_face(q, face).is_some()
                        && is_plane(b, end_face)?
                    {
                        found.push(Candidate {
                            picks: [i, j],
                            at: [vi, vj],
                            e,
                            face,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// The meeting end of a pick at `v` towards the unpicked edge `e` on `face`: the pick's
/// unit direction away from `v`, its band's inward unit normal on `face` and the reach of
/// its band along `e` from `v` (`None` where the face's corner at `v` is not convex).
fn meeting_end(
    k: &Kernel,
    b: &Brep,
    pick: &Pick,
    v: VertexId,
    e: EdgeId,
    face: FaceId,
    roundoff: f64,
) -> Result<Option<(Vec3, Vec3, f64)>> {
    let Some(beside) = other_face(pick, face) else {
        return Ok(None);
    };
    let (_, beside) = outward_plane(k, b, beside)?;
    let t = leaving(k, b, pick.edge, v, OP)?;
    let u = leaving(k, b, e, v, OP)?;
    // u's part square to the edge, in the face: into the face where its corner is convex,
    // which the material's side of the edge's other face confirms
    let across = u - t * u.dot(t);
    let sine = across.length();
    if !(sine.is_finite() && sine > roundoff) {
        return Ok(None);
    }
    let inward = across * (1.0 / sine);
    let side = inward.dot(beside);
    if side.is_nan() || side >= -roundoff {
        return Ok(None);
    }
    Ok(Some((t, inward, pick.size / sine)))
}

/// Whether the candidate's bands reach past each other across its edge, locally: each
/// edge runs away from the other's band, and the reaches along the edge add up to more
/// than its length by more than the body's tolerance.
fn meets(k: &Kernel, b: &Brep, picks: &[Pick], c: &Candidate, roundoff: f64) -> Result<bool> {
    let [i, j] = c.picks;
    let [vi, vj] = c.at;
    let (Some(p), Some(q)) = (picks.get(i), picks.get(j)) else {
        return Ok(false);
    };
    let (Some((tp, mp, reach_p)), Some((tq, mq, reach_q))) = (
        meeting_end(k, b, p, vi, c.e, c.face, roundoff)?,
        meeting_end(k, b, q, vj, c.e, c.face, roundoff)?,
    ) else {
        return Ok(false);
    };
    if !(tp.dot(mq) > roundoff && tq.dot(mp) > roundoff) {
        return Ok(false);
    }
    let length = (vertex::point(b, vj)? - vertex::point(b, vi)?).length();
    let overlap = reach_p + reach_q - length;
    Ok(overlap.is_finite() && overlap > b.tolerance())
}

/// Plan the passes of chamfering `picked` at `sizes` (one for all, or one per edge).
///
/// # Errors
/// Cancellation, allocation and failures reading the B-rep or classifying an edge's
/// convexity (on a copy: the target is left as it was).
pub fn plan(k: &Kernel, b: &mut Brep, picked: &[EdgeId], sizes: &[f64]) -> Result<CrossingEndPlan> {
    k.check_cancelled(OP)?;
    let count = picked.len();
    if count < 2 || (sizes.len() != 1 && sizes.len() != count) {
        return Ok(CrossingEndPlan::NotApplicable);
    }
    let mut picks: Vec<Pick> = Vec::new();
    reserve(&mut picks, count)?;
    for (i, &e) in picked.iter().enumerate() {
        k.check_cancelled(OP)?;
        let Some(&size) = sizes.get(i).or_else(|| sizes.first()) else {
            return Ok(CrossingEndPlan::NotApplicable);
        };
        if picks.iter().any(|p| p.edge == e) {
            return Ok(CrossingEndPlan::NotApplicable);
        }
        let Some(p) = pick(b, e, size)? else {
            return Ok(CrossingEndPlan::NotApplicable);
        };
        picks.push(p);
    }

    // picks sharing a vertex go together: each pick's component, named by its first member
    let mut component: Vec<usize> = (0..count).collect();
    for i in 0..count {
        for j in 0..i {
            let (Some(p), Some(q)) = (picks.get(i), picks.get(j)) else {
                continue;
            };
            if !p.ends.iter().any(|v| q.ends.contains(v)) {
                continue;
            }
            let (Some(&ci), Some(&cj)) = (component.get(i), component.get(j)) else {
                continue;
            };
            let (keep, merge) = (ci.min(cj), ci.max(cj));
            for c in &mut component {
                if *c == merge {
                    *c = keep;
                }
            }
        }
    }

    let mut candidates = Vec::new();
    for i in 0..count {
        for j in 0..i {
            k.check_cancelled(OP)?;
            candidates_of(b, &picks, i, j, &mut candidates)?;
        }
    }
    if candidates.is_empty() {
        return Ok(CrossingEndPlan::NotApplicable);
    }

    // Classifying convexity may populate UV trims: on a copy.
    let convex = b.with_rollback_untouched(
        |copy| {
            for p in &picks {
                if is_concave_edge(k, copy, p.edge)? {
                    return Ok(false);
                }
            }
            Ok(true)
        },
        |_| false,
    )?;
    if !convex {
        return Ok(CrossingEndPlan::NotApplicable);
    }

    let roundoff = gamma(64);
    let mut joins = Vec::new();
    for c in &candidates {
        k.check_cancelled(OP)?;
        if !meets(k, b, &picks, c, roundoff)? {
            continue;
        }
        let [i, j] = c.picks;
        let (Some(&ci), Some(&cj)) = (component.get(i), component.get(j)) else {
            return Ok(CrossingEndPlan::NotApplicable);
        };
        if ci == cj {
            return Ok(CrossingEndPlan::NotApplicable);
        }
        joins.push((ci, cj));
    }
    if joins.is_empty() {
        return Ok(CrossingEndPlan::NotApplicable);
    }

    // two-colour the components over the meetings, breadth first from the earliest
    let mut colour: Vec<Option<bool>> = vec![None; count];
    let mut queue = Vec::new();
    reserve(&mut queue, count)?;
    for start in 0..count {
        if component.get(start) != Some(&start) || colour.get(start) != Some(&None) {
            continue;
        }
        if let Some(c) = colour.get_mut(start) {
            *c = Some(false);
        }
        queue.clear();
        queue.push(start);
        let mut next = 0;
        while let Some(&at) = queue.get(next) {
            next += 1;
            let Some(Some(here)) = colour.get(at).copied() else {
                continue;
            };
            for &(a, z) in &joins {
                let other = if a == at {
                    z
                } else if z == at {
                    a
                } else {
                    continue;
                };
                match colour.get(other).copied().flatten() {
                    Some(there) if there == here => return Ok(CrossingEndPlan::NotApplicable),
                    Some(_) => {}
                    None => {
                        if let Some(c) = colour.get_mut(other) {
                            *c = Some(!here);
                        }
                        queue.push(other);
                    }
                }
            }
        }
    }

    let mut first = Vec::new();
    let mut second = Vec::new();
    reserve(&mut first, count)?;
    reserve(&mut second, count)?;
    for (i, p) in picks.iter().enumerate() {
        let side = component
            .get(i)
            .and_then(|&c| colour.get(c).copied().flatten());
        match side {
            Some(true) => second.push((p.edge, p.size)),
            Some(false) => first.push((p.edge, p.size)),
            None => return Ok(CrossingEndPlan::NotApplicable),
        }
    }
    if first.is_empty() || second.is_empty() {
        return Ok(CrossingEndPlan::NotApplicable);
    }
    Ok(CrossingEndPlan::Passes(CrossingEndPasses { first, second }))
}

impl CrossingEndPasses {
    /// Run the two passes with the caller's ordinary chamfer construction, the second on
    /// what the first left of its picked edges (every piece of each, tracked through the
    /// first pass). The caller supplies its normal transaction so a refusal or
    /// cancellation undoes both.
    ///
    /// # Errors
    /// A cancelled operation, allocation or topology failure, a picked edge of the second
    /// pass the first one took off whole ([`BlendFailure::CornerRefused`] there), or the
    /// errors of either pass.
    pub fn build<E>(
        self,
        k: &mut Kernel,
        b: &mut Brep,
        mut pass: impl FnMut(
            &mut Kernel,
            &mut Brep,
            &[EdgeId],
            &[f64],
        ) -> Result<std::result::Result<(), E>>,
    ) -> Result<std::result::Result<(), E>> {
        for (i, &(e, _)) in self.second.iter().enumerate() {
            k.check_cancelled(OP)?;
            b.add_attribute(
                EntityRef::Edge(e),
                Attribute::application(SECOND_ID, Behavior::Copy, Second(i)),
            )?;
        }
        let (edges, sizes): (Vec<EdgeId>, Vec<f64>) = self.first.iter().copied().unzip();
        if let Err(refusal) = pass(k, b, &edges, &sizes)? {
            return Ok(Err(refusal));
        }

        let mut tagged = Vec::new();
        for &e in b.edge_list() {
            k.check_cancelled(OP)?;
            for (handle, attribute) in b.entity_attributes(EntityRef::Edge(e)) {
                if attribute.attribute_id() != SECOND_ID {
                    continue;
                }
                if let Some(Second(i)) = attribute.application_value::<Second>() {
                    tagged.push((e, handle, *i));
                }
            }
        }
        let mut found = vec![false; self.second.len()];
        let mut edges = Vec::new();
        let mut sizes = Vec::new();
        reserve(&mut edges, tagged.len())?;
        reserve(&mut sizes, tagged.len())?;
        for (e, handle, i) in tagged {
            b.remove_attribute(EntityRef::Edge(e), handle)?;
            let (Some(&(_, size)), Some(seen)) = (self.second.get(i), found.get_mut(i)) else {
                continue;
            };
            *seen = true;
            if !edges.contains(&e) {
                edges.push(e);
                sizes.push(size);
            }
        }
        if let Some(lost) = found.iter().position(|seen| !seen) {
            let failure = Failure::new(OP, BlendFailure::CornerRefused);
            return Err(match self.second.get(lost) {
                Some(&(e, _)) => failure.at(EntityRef::Edge(e)).into_error(),
                None => failure.into_error(),
            });
        }
        k.check_cancelled(OP)?;
        pass(k, b, &edges, &sizes)
    }
}
