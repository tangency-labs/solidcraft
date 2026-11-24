//! Not upstream (WR-647, tangency-labs/solidcraft#18166): a surface/surface curve of a
//! face pair that runs along part of an edge E of one face, where another curve of the
//! same pair already runs along the whole of E, is a second copy of E: that interval is
//! dropped from the pair before any edge or vertex definition is made of it
//! ([`drop_duplicate_edge_runs`]).
//!
//! Two faces that meet tangentially near E (a G2 round and the body it was rounded on)
//! are within the intersection tolerance of each other over a band around E. The
//! marching can then return E twice: once as a curve C1 whose classification on face A
//! is vertex/edge E/vertex (C1 is E, end to end), and once as a curve C2 that runs along
//! E only from a point inside E to one of its ends (its interval there is classified to
//! E in the middle and at that inner end, which is not a vertex). C2's run makes nothing
//! new: its interior is E, which C1 already imprints on both faces. But its inner end is
//! a point of E that is not a vertex, and the merge must split E there for C2 while C1
//! keeps E whole, so the two definitions of one edge conflict (the merge's
//! `RelateConflict`).
//!
//! A run is dropped only when all of these hold on face A:
//! - its middle is classified to E, and each of its ends is classified either to E
//!   itself or to one of E's own two vertices (so the run starts and ends on E, and is
//!   not a single-point middle sample of a curve that merely touches E);
//! - it is not itself vertex/edge/vertex;
//! - another curve of the pair has an interval on A that is E whole: its middle is E and
//!   its ends are E's own two vertices;
//!
//! and on face B every end of the run that is classified to an edge or a vertex of B is
//! also an interval end of that other curve on B, with the same object (so dropping the
//! run removes no split of B's topology that C1 does not make too).
//!
//! Within the tolerance C2's run and C1 are then one intersection. ARGUED, not proven:
//! the run's ends and middle are on E within the classification tolerance, and C1 is E
//! from vertex to vertex, so the run lies within the tolerance of a part of C1 wherever
//! the run's classification samples it; between samples it rests on the curve staying
//! within the tolerance of E, which the classification does not check pointwise. On B,
//! C1's interval covers the same part of 3D space up to the tolerance, so B keeps E's
//! imprint from C1. The interval is removed from both faces' classifications; C2's
//! other intervals (where it leaves E) stay, with the end they share with the run.
//!
//! An interval that is itself vertex/edge/vertex is never dropped (it is the whole of an
//! edge, a copy only of an equal interval, which the merge relates as one edge), so of
//! two curves that both run along the whole of E neither is dropped, and a run is
//! dropped only for a curve other than its own.
//!
//! Not covered: when the interval of C2 next to the run is itself imprinted on E's other
//! face, the vertex it ends at is still a point inside E and a `RelateConflict` can
//! remain; no case is known.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`drop_duplicate_edge_runs`] (super) | none (own design) |
//!
//! Deviations: not upstream at all; always on. No recursion.

use crate::error::boolean::BooleanStage;
use crate::error::{Result, SolidError};
use crate::intersect::curve_class::p1::{CurveClassification, CurveInterval};
use crate::intersect::point_class::{ClassGeometry, ClassObject, PointClassType};
use crate::kernel::Kernel;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, vertexuse, Brep, EdgeId, VertexId};
use crate::EntityRef;

use super::coincident_strip::FacePairIntersection;

const OP: &str = "boolean::topology_intersector::duplicate_edge_run::drop_duplicate_edge_runs";

/// The edge a classification object names (an edge or its use), with its B-rep.
fn edge_of<O: ClassObject>(g: &impl ClassGeometry<O>, o: O) -> Option<(&Brep, EdgeId)> {
    match g.topology_entity(o)? {
        (b, EntityRef::Edge(e)) => Some((b, e)),
        (b, EntityRef::Edgeuse(eu)) => Some((b, eu1::edge(b, eu).ok()??)),
        _ => None,
    }
}

/// The vertex a classification object of B-rep `b` names (a vertex or its use).
fn vertex_of<O: ClassObject>(g: &impl ClassGeometry<O>, b: &Brep, o: O) -> Option<VertexId> {
    match g.topology_entity(o)? {
        (ob, EntityRef::Vertex(v)) if core::ptr::eq(ob, b) => Some(v),
        (ob, EntityRef::Vertexuse(vu)) if core::ptr::eq(ob, b) => vertexuse::vertex(b, vu).ok()?,
        _ => None,
    }
}

/// The two vertices of the edge `o` names, with its B-rep and the edge.
fn edge_ends<O: ClassObject>(
    g: &impl ClassGeometry<O>,
    o: O,
) -> Option<(&Brep, EdgeId, VertexId, VertexId)> {
    let (b, e) = edge_of(g, o)?;
    let v1 = edge::vertex(b, e).ok()??;
    let v2 = edge::other_vertex(b, e, Some(v1)).ok()??;
    Some((b, e, v1, v2))
}

/// Whether the end classified `class` to `object` lies on the edge `ends` names: it is
/// classified to that edge itself or to one of its two vertices.
fn end_on_edge<O: ClassObject>(
    g: &impl ClassGeometry<O>,
    ends: (&Brep, EdgeId, VertexId, VertexId),
    class: PointClassType,
    object: Option<O>,
) -> bool {
    let (b, e, v1, v2) = ends;
    let Some(o) = object else {
        return false;
    };
    match class {
        PointClassType::Edge => {
            edge_of(g, o).is_some_and(|(ob, oe)| core::ptr::eq(ob, b) && oe == e)
        }
        PointClassType::Vertex => vertex_of(g, b, o).is_some_and(|v| v == v1 || v == v2),
        _ => false,
    }
}

/// Whether `ivl` runs along part of an edge E: its middle is classified to E, each end to
/// E or one of E's vertices, and it is not vertex/E/vertex. Returns E's ends if so.
fn partial_edge_run<'g, O: ClassObject>(
    g: &'g impl ClassGeometry<O>,
    ivl: &CurveInterval<O>,
) -> Option<(&'g Brep, EdgeId, VertexId, VertexId)> {
    if ivl.mid.point_class() != PointClassType::Edge
        || (ivl.start.point_class() == PointClassType::Vertex
            && ivl.end.point_class() == PointClassType::Vertex)
    {
        return None;
    }
    let ends = edge_ends(g, ivl.mid.object()?)?;
    (end_on_edge(g, ends, ivl.start.point_class(), ivl.start.object())
        && end_on_edge(g, ends, ivl.end.point_class(), ivl.end.object()))
    .then_some(ends)
}

/// Whether `ivl` is the whole of the edge `ends` names: its middle is that edge and its
/// ends are the edge's own two vertices.
fn is_whole_edge<O: ClassObject>(
    g: &impl ClassGeometry<O>,
    ivl: &CurveInterval<O>,
    ends: (&Brep, EdgeId, VertexId, VertexId),
) -> bool {
    let (b, _, v1, v2) = ends;
    let vertex = |class: PointClassType, object: Option<O>| {
        if class != PointClassType::Vertex {
            return None;
        }
        vertex_of(g, b, object?)
    };
    ivl.mid.point_class() == PointClassType::Edge
        && end_on_edge(g, ends, PointClassType::Edge, ivl.mid.object())
        && matches!(
            (
                vertex(ivl.start.point_class(), ivl.start.object()),
                vertex(ivl.end.point_class(), ivl.end.object()),
            ),
            (Some(a), Some(z)) if (a == v1 && z == v2) || (a == v2 && z == v1)
        )
}

/// Whether every end of `run` (an interval on face B) that is classified to an edge or a
/// vertex is also an interval end of `whole` (another curve's classification on B) with
/// the same class and object.
fn ends_kept_by<O: ClassObject>(
    run: &CurveInterval<O>,
    whole: &CurveClassification<O>,
) -> Result<bool> {
    for end in [&run.start, &run.end] {
        let class = end.point_class();
        if class != PointClassType::Edge && class != PointClassType::Vertex {
            continue;
        }
        let mut found = false;
        for m in 0..whole.size() {
            let w = whole.at(m)?;
            for b in [&w.start, &w.end] {
                if b.point_class() == class && b.object() == end.object() {
                    found = true;
                }
            }
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Drops from each face pair that is not coincident every curve interval that runs along
/// part of an edge of one of the pair's faces when another curve of the same pair has an
/// interval that is that whole edge on the same face, and keeps every edge or vertex end
/// the run has on the other face (module docs). The interval is removed from both faces'
/// classifications.
///
/// Returns the number of intervals dropped.
///
/// # Errors
///
/// Cancellation; a curve whose two classifications differ in length (a defect: every
/// caller homogenizes them first).
pub(super) fn drop_duplicate_edge_runs<O: ClassObject>(
    k: &Kernel,
    g: &impl ClassGeometry<O>,
    intersections: &mut [FacePairIntersection<O>],
) -> Result<usize> {
    // (decided over the intersection as it stands, then removed)
    let mut dropped: Vec<[usize; 3]> = Vec::new();
    for (pi, (_, _, are_coincident, fi, _)) in intersections.iter().enumerate() {
        if *are_coincident {
            continue;
        }
        k.check_cancelled(OP)?;
        for (ci, (c1, c2)) in fi.cc1.iter().zip(&fi.cc2).enumerate() {
            if c1.size() != c2.size() {
                return Err(SolidError::boolean_defect(
                    OP,
                    BooleanStage::Intersect,
                    "the two classifications of one curve differ in length, though the \
                     intersection homogenizes them first",
                ));
            }
            for ll in 0..c1.size() {
                let (i1, i2) = (c1.at(ll)?, c2.at(ll)?);
                let mut duplicate = false;
                for (ivl, ivl_b, side) in [(i1, i2, 0), (i2, i1, 1)] {
                    let Some(ends) = partial_edge_run(g, ivl) else {
                        continue;
                    };
                    for (cj, (d1, d2)) in fi.cc1.iter().zip(&fi.cc2).enumerate() {
                        if cj == ci {
                            continue;
                        }
                        let (other, other_b) = if side == 0 { (d1, d2) } else { (d2, d1) };
                        let mut whole = false;
                        for m in 0..other.size() {
                            if is_whole_edge(g, other.at(m)?, ends) {
                                whole = true;
                            }
                        }
                        if whole && ends_kept_by(ivl_b, other_b)? {
                            duplicate = true;
                        }
                    }
                }
                if duplicate {
                    dropped.push([pi, ci, ll]);
                }
            }
        }
    }
    // (pushed in increasing order: removed from the last, so the earlier indices hold)
    for &[pi, ci, ll] in dropped.iter().rev() {
        let Some((_, _, _, fi, _)) = intersections.get_mut(pi) else {
            continue;
        };
        if let Some(c) = fi.cc1.get_mut(ci) {
            c.remove_at(ll)?;
        }
        if let Some(c) = fi.cc2.get_mut(ci) {
            c.remove_at(ll)?;
        }
    }
    Ok(dropped.len())
}
