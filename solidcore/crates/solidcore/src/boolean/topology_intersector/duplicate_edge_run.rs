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
//! Within the tolerance C2's run and C1 are one intersection (PROVEN for face A: an
//! interval is classified to E in the middle only where the intersection reports it
//! within the tolerance of E over its whole extent, a range solution, and C1's interval
//! is E from vertex to vertex, so the run lies within the tolerance of a part of C1).
//! ARGUED for the other face B: C1's interval on B covers the same part of 3D space up to
//! the tolerance, so B keeps E's imprint from C1. The interval is removed from both
//! faces' classifications; C2's other intervals (where it leaves E) stay, with the end
//! they share with the run.
//!
//! An interval that is itself vertex/edge/vertex is never dropped (it is the whole of an
//! edge, a copy only of an equal interval, which the merge relates as one edge), so of
//! two curves that both run along the whole of E neither is dropped, and a run is
//! dropped only for a curve other than its own.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`drop_duplicate_edge_runs`] (super) | none (own design) |
//!
//! Deviations: not upstream at all; always on. No recursion.

use crate::error::Result;
use crate::intersect::curve_class::p1::CurveInterval;
use crate::intersect::point_class::{ClassObject, PointClassType};
use crate::kernel::Kernel;

use super::coincident_strip::FacePairIntersection;

const OP: &str = "boolean::topology_intersector::duplicate_edge_run::drop_duplicate_edge_runs";

/// Whether `ivl` runs along an edge (its middle is classified to one) without being that
/// edge from vertex to vertex.
fn is_partial_edge_run<O: ClassObject>(ivl: &CurveInterval<O>) -> bool {
    ivl.mid.point_class() == PointClassType::Edge
        && !(ivl.start.point_class() == PointClassType::Vertex
            && ivl.end.point_class() == PointClassType::Vertex)
}

/// Whether `ivl` is the whole of the edge `edge`: vertex, `edge`, vertex.
fn is_whole_edge<O: ClassObject>(ivl: &CurveInterval<O>, edge: Option<O>) -> bool {
    ivl.mid.point_class() == PointClassType::Edge
        && ivl.mid.object() == edge
        && ivl.start.point_class() == PointClassType::Vertex
        && ivl.end.point_class() == PointClassType::Vertex
}

/// Drops from each face pair that is not coincident every curve interval that runs along
/// part of an edge of one of the pair's faces when another curve of the same pair has an
/// interval that is that whole edge on the same face (module docs). The interval is
/// removed from both faces' classifications.
///
/// Returns the number of intervals dropped.
///
/// # Errors
///
/// Cancellation; an interval index past a classification's end (none is read).
pub(super) fn drop_duplicate_edge_runs<O: ClassObject>(
    k: &Kernel,
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
            for ll in 0..c1.size().min(c2.size()) {
                let sides = [(c1.at(ll)?, 0), (c2.at(ll)?, 1)];
                let mut duplicate = false;
                for (ivl, side) in sides {
                    if !is_partial_edge_run(ivl) {
                        continue;
                    }
                    let edge = ivl.mid.object();
                    for (cj, (d1, d2)) in fi.cc1.iter().zip(&fi.cc2).enumerate() {
                        if cj == ci {
                            continue;
                        }
                        let other = if side == 0 { d1 } else { d2 };
                        for m in 0..other.size() {
                            if is_whole_edge(other.at(m)?, edge) {
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
