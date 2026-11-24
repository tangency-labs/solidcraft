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
//! and on face B the run is not vertex/edge/vertex, its middle is classified as the other
//! curve's interval there (same class and object; or to nothing, or unknown), and every
//! end of it classified to an edge or a vertex of B is an end of that interval or of its
//! two neighbours with the same object that is the same point: for a vertex, the vertex
//! itself; for an edge G, a point within G's zone tolerance of the run's, or an end that
//! on A is classified to the same vertex as the run's end there (the merge splits G at
//! that one vertex for both). So dropping the run removes no imprint or split of B that
//! C1 does not make too. An interval already dropped never justifies a drop.
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
use crate::math::Point3;
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

/// The 3D point of curve classification `cc` at parameter `t`, `None` when its curve is
/// not there.
fn point_of<O: ClassObject>(
    k: &Kernel,
    g: &impl ClassGeometry<O>,
    cc: &CurveClassification<O>,
    t: f64,
) -> Result<Option<Point3>> {
    let Some(curve) = cc.curve().and_then(|h| g.curve(h)) else {
        return Ok(None);
    };
    Ok(curve.evaluate(k, t, 0, false, false)?.first().copied())
}

/// Whether dropping interval `ll` of `run_cc` (the run, on face B) loses nothing of B that
/// interval `m` of `whole` (the curve that is the whole edge on A, here on B) does not
/// make. `run_a` and `whole_a` are the same two curves' classifications on face A (each
/// interval of one curve is the same parameter range on both faces):
/// - the run's middle is classified as `whole`'s interval `m` (same class and object), or
///   to nothing, or is unknown (no imprint);
/// - every end of the run classified to a vertex is an end of interval `m - 1`, `m` or
///   `m + 1` of `whole` classified to the same vertex;
/// - every end of the run classified to an edge G is such an end classified to G that is
///   the same point: either the two curves' points there lie within G's zone tolerance
///   of each other, or on face A both ends are classified to one vertex (each lies within
///   that vertex's tolerance of it, and the merge splits G at that one vertex for both).
///   A curve that is not there keeps the run.
fn kept_by<O: ClassObject>(
    k: &Kernel,
    g: &impl ClassGeometry<O>,
    (run_cc, run_a): (&CurveClassification<O>, &CurveClassification<O>),
    ll: usize,
    (whole, whole_a): (&CurveClassification<O>, &CurveClassification<O>),
    m: usize,
) -> Result<bool> {
    let run = run_cc.at(ll)?;
    let run_on_a = run_a.at(ll)?;
    let mate = whole.at(m)?;
    let mid = run.mid.point_class();
    let differs = mid != mate.mid.point_class() || run.mid.object() != mate.mid.object();
    if differs && !matches!(mid, PointClassType::Nothing | PointClassType::Unknown) {
        return Ok(false);
    }
    for (end, end_a, t) in [
        (&run.start, &run_on_a.start, run.interval.min()),
        (&run.end, &run_on_a.end, run.interval.max()),
    ] {
        let class = end.point_class();
        if class != PointClassType::Edge && class != PointClassType::Vertex {
            continue;
        }
        let at = if class == PointClassType::Edge {
            let Some(p) = point_of(k, g, run_cc, t)? else {
                return Ok(false);
            };
            Some((p, end.obj_zone_tol_3d(k, g)?))
        } else {
            None
        };
        let mut found = false;
        for n in m.saturating_sub(1)..=m.saturating_add(1) {
            if n >= whole.size() {
                continue;
            }
            let w = whole.at(n)?;
            let w_a = whole_a.at(n)?;
            for (b, b_a, tw) in [
                (&w.start, &w_a.start, w.interval.min()),
                (&w.end, &w_a.end, w.interval.max()),
            ] {
                if b.point_class() != class || b.object() != end.object() {
                    continue;
                }
                match at {
                    None => found = true,
                    Some((p, tol)) => {
                        // (one vertex of A for both: one point of G)
                        if end_a.point_class() == PointClassType::Vertex
                            && b_a.point_class() == PointClassType::Vertex
                            && end_a.object().is_some()
                            && end_a.object() == b_a.object()
                        {
                            found = true;
                        } else if let Some(q) = point_of(k, g, whole, tw)? {
                            if (p - q).length() <= tol {
                                found = true;
                            }
                        }
                    }
                }
            }
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `ivl` is vertex/edge/vertex: the whole of some edge.
fn is_vertex_edge_vertex<O: ClassObject>(ivl: &CurveInterval<O>) -> bool {
    ivl.start.point_class() == PointClassType::Vertex
        && ivl.mid.point_class() == PointClassType::Edge
        && ivl.end.point_class() == PointClassType::Vertex
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
                    // (an interval that is a whole edge on the other face is kept: it may
                    // be the only copy of that edge)
                    if is_vertex_edge_vertex(ivl_b) {
                        continue;
                    }
                    let Some(ends) = partial_edge_run(g, ivl) else {
                        continue;
                    };
                    for (cj, (d1, d2)) in fi.cc1.iter().zip(&fi.cc2).enumerate() {
                        if cj == ci {
                            continue;
                        }
                        let (other, other_b) = if side == 0 { (d1, d2) } else { (d2, d1) };
                        let (run_a, run_b) = if side == 0 { (c1, c2) } else { (c2, c1) };
                        for m in 0..other.size() {
                            // (never against an interval already dropped)
                            if dropped.contains(&[pi, cj, m]) {
                                continue;
                            }
                            if is_whole_edge(g, other.at(m)?, ends)
                                && kept_by(k, g, (run_b, run_a), ll, (other_b, other), m)?
                            {
                                duplicate = true;
                            }
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
