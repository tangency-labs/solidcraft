//! Not upstream: relating the unrelated edges of a coincident face pair that join the same
//! related vertices and whose curves are certified to lie within the edges' tolerance of each
//! other over their whole length.
//!
//! Upstream: none (solidcore-native).
//!
//! | Rust                        | Upstream |
//! |-----------------------------|----------|
//! | [`relate_coincident_edges`] | none     |
//! | [`certified_line_gap`]      | none     |
//!
//! `SmMerge::FixFoundFaceCoincidence` merges the faces of two coincident surfaces with
//! `MergeCoincidentSurfaces`, which imprints every unrelated edge of one face onto the other.
//! An edge of one operand and an edge of the other that join the same two related vertices
//! and run within tolerance of each other between them are one edge of the result. When the
//! intersection left them unrelated (a near-tangent arc that grazes a box face along a line
//! piece, #16640: the arc piece and the line piece meet at both vertices), the imprint copies
//! each onto the other face as a second edge between the same two vertices, a zero-area face
//! between the two copies on each side. Those coincident slivers keep the result from
//! closing. Relating the two edges first, as the vertices at both of their ends already are,
//! leaves the imprint nothing to copy.
//!
//! Shared ends are not enough: two arcs that bow apart can join the same two vertices. The
//! relation is made only when [`certified_line_gap`] bounds the Hausdorff distance of the two
//! curves over the edges' intervals, every rounding included, by the larger edge tolerance.
//! The bound needs one of the two curves to be a line; a pair of other kinds is left to the
//! imprint, as before. The bound is a certificate in code ([`certified_line_gap`]); its
//! proof is in that function's doc comment. A pair whose curves bow apart by more than the
//! tolerance is refused and imprinted as before (the `boolean_api` test
//! `relating_coincident_edges_needs_their_curves_within_tolerance`).
//!
//! #16640's guard (`proto_topology::p2`, an edge-to-edge definition whose ends lie farther
//! apart than every tolerance makes no relation of its own) is about a stretch that ends on
//! no vertex. This relation needs a related vertex at both ends of both edges, so that case
//! never reaches it.
//!
//! Deviations: none (no upstream counterpart).

use crate::boolean::topology_intersector::p1::Relations;
use crate::curve::curve::{Curve, CurveKind};
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::math::Point3;
use crate::nurbs::math::gamma;
use crate::topology::topology_traverser::face_edges;
use crate::topology::{edge, Brep, EdgeId, FaceId, VertexId};

/// The two end vertices of an edge, `None` for an edge without both or a closed edge.
fn ends(b: &Brep, e: EdgeId) -> Result<Option<(VertexId, VertexId)>> {
    let vs = edge::vertices(b, e, None)?;
    Ok(match vs.as_slice() {
        [Some(a), Some(z)] if a != z => Some((*a, *z)),
        _ => None,
    })
}

/// Relates each unrelated edge of `face` (primary) to the one unrelated edge of `other_face`
/// that joins the vertices related to its ends, when [`certified_line_gap`] proves the two
/// curves within the larger of the two edge tolerances of each other (module docs). An edge
/// with more than one such partner is left alone. Returns how many pairs were related.
///
/// # Errors
///
/// Those of the topology walks and of [`Relations::relate_edges`].
pub fn relate_coincident_edges(
    primary: &mut Brep,
    other: &mut Brep,
    face: FaceId,
    other_face: FaceId,
    relations: &mut Relations,
) -> Result<usize> {
    let mut related = 0;
    // (each edge once, in id order: a loop walk lists an edge twice when both its uses
    // bound the face, not always next to each other)
    let mut edges = face_edges(primary, face)?;
    edges.sort_unstable();
    edges.dedup();
    let mut other_edges = face_edges(other, other_face)?;
    other_edges.sort_unstable();
    other_edges.dedup();
    // An other edge related here is skipped by every later primary edge, so of two primary
    // edges with the same unique partner, the first in this order is related.
    for pe in edges {
        if relations.edges.other(pe).is_some() {
            continue;
        }
        let Some((a, z)) = ends(primary, pe)? else {
            continue;
        };
        let (Some(oa), Some(oz)) = (relations.vertices.other(a), relations.vertices.other(z))
        else {
            continue;
        };
        let mut found = None;
        let mut count = 0;
        for &oe in &other_edges {
            if relations.edges.primary(oe).is_some() || found == Some(oe) {
                continue;
            }
            let Some(o_ends) = ends(other, oe)? else {
                continue;
            };
            if o_ends == (oa, oz) || o_ends == (oz, oa) {
                count += 1;
                found = Some(oe);
            }
        }
        let (1, Some(oe)) = (count, found) else {
            continue;
        };
        let reversed = ends(other, oe)? == Some((oz, oa));
        let (Some(pc), Some(oc)) = (edge::curve(primary, pe)?, edge::curve(other, oe)?) else {
            continue;
        };
        let (p_rec, o_rec) = (primary.edge(pe)?, other.edge(oe)?);
        let tol = p_rec.tolerance.max(o_rec.tolerance);
        let Some(gap) = certified_line_gap(pc, &p_rec.interval, oc, &o_rec.interval, reversed)
        else {
            continue;
        };
        // a NaN tolerance relates nothing
        if tol.is_nan() || gap > tol {
            continue;
        }
        relations.relate_edges(primary, pe, other, oe)?;
        related += 1;
    }
    Ok(related)
}

/// A bound `d` on the Hausdorff distance between `a` over `ia` and `b` over `ib`, every
/// rounding included, when one of them is a line; `reversed` when `a`'s start meets `b`'s
/// end. `None` when neither is a line, when the other has no certain hull
/// ([`Curve::piece_hull_points`]), for a segment of zero length, and for numbers not finite.
///
/// Proof. Let `S = [s0, s1]` be the line's segment and `h_i` the hull points of the other
/// curve `B` (each with its coordinate error `e_i`; `B` lies in their convex hull, its ends
/// are the first and last). The distance to the convex set `S` is convex, so every point of
/// `B` is within `d1 = max_i dist(h_i, S)` of `S` (each computed distance widened by the
/// points' errors and the rounding of the foot and the norm). Every point `q` of `S`: let
/// `τ` be the parameter of the projection onto the line, continuous along `B`. If `τ(q)`
/// lies between `τ` of `B`'s two ends, some point of `B` projects onto `q` and is within its
/// distance to the line, at most `d1`, of `q`. Otherwise `q` lies between an end of `S` and
/// the projection of the end of `B` that meets it, at most the gap `g` between those two
/// ends from that end of `S`, so within `2 g` of that end of `B`. Hence `d1 + 2 g` bounds both
/// directions.
#[must_use]
pub fn certified_line_gap(
    a: &Curve,
    ia: &Extent1d,
    b: &Curve,
    ib: &Extent1d,
    reversed: bool,
) -> Option<f64> {
    let ((line, il), (curve, ic)) = if matches!(a.kind(), CurveKind::Line(_)) {
        ((a, ia), (b, ib))
    } else if matches!(b.kind(), CurveKind::Line(_)) {
        ((b, ib), (a, ia))
    } else {
        return None;
    };
    let [(s0, e0), (s1, e1)] = <[(Point3, f64); 2]>::try_from(line.piece_hull_points(il)?).ok()?;
    let hull = curve.piece_hull_points(ic)?;
    let (first, last) = (hull.first()?, hull.last()?);
    let size = |p: Point3| p.x.abs().max(p.y.abs()).max(p.z.abs());
    let e_seg = e0.max(e1);
    let v = s1 - s0;
    let vv = v.dot(v);
    if vv.is_nan() || vv <= 0.0 {
        return None;
    }
    let mut d1 = 0.0_f64;
    for &(h, e) in &hull {
        let t = ((h - s0).dot(v) / vv).clamp(0.0, 1.0);
        let foot = s0 + v * t;
        // the computed foot is a point of the true segment to within the segment's error and
        // the rounding of its four operations on coordinates of size `m`
        let m = size(s0).max(size(s1)).max(size(h));
        let d = (h.distance(foot) + 2.0 * (e + e_seg) + 2.0 * gamma(8) * m) * (1.0 + gamma(4));
        if !d.is_finite() {
            return None;
        }
        d1 = d1.max(d);
    }
    // a line's own start meets the curve's start unless reversed (whichever of a, b it is)
    let (near0, near1) = if reversed {
        (last, first)
    } else {
        (first, last)
    };
    let gap = |p: &(Point3, f64), s: Point3, es: f64| {
        let m = size(p.0).max(size(s));
        (p.0.distance(s) + 2.0 * (p.1 + es) + 2.0 * gamma(2) * m) * (1.0 + gamma(4))
    };
    let g = gap(near0, s0, e0).max(gap(near1, s1, e1));
    let bound = (d1 + 2.0 * g) * (1.0 + gamma(2));
    bound.is_finite().then_some(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_curves as curves;

    fn line(a: Point3, b: Point3) -> Curve {
        Curve::from(curves::create_line_segment(a, b).unwrap())
    }

    fn arc(centre: (f64, f64), r: f64, from: f64, to: f64) -> Curve {
        Curve::from(curves::create_arc(Point3::new(centre.0, centre.1, 0.0), r, from, to).unwrap())
    }

    fn whole(c: &Curve) -> Extent1d {
        c.natural_interval()
    }

    /// The grazing arc of #16640 against the line piece that joins its ends: within a micron.
    #[test]
    fn a_grazing_arc_piece_is_within_tolerance_of_its_chord() {
        // radius 5 about (5, 10) over 0.02 degrees from (10, 10): 1.75e-3 long, its sagitta
        // over the chord 5 (1 - cos 0.01°) = 7.6e-8
        let (r, deg) = (5.0_f64, 0.02_f64);
        let a = arc((5.0, 10.0), r, 0.0, deg);
        let end = Point3::new(
            5.0 + r * deg.to_radians().cos(),
            10.0 + r * deg.to_radians().sin(),
            0.0,
        );
        let l = line(Point3::new(10.0, 10.0, 0.0), end);
        let sagitta = r * (1.0 - (deg.to_radians() / 2.0).cos());
        let d = certified_line_gap(&l, &whole(&l), &a, &whole(&a), false).unwrap();
        assert!(d >= sagitta, "{d} below the sagitta {sagitta}");
        assert!(d < 1e-5, "{d}");
        // either order, either direction
        let d2 = certified_line_gap(&a, &whole(&a), &l, &whole(&l), false).unwrap();
        assert!(d2 < 1e-5, "{d2}");
        let back = line(end, Point3::new(10.0, 10.0, 0.0));
        let d3 = certified_line_gap(&back, &whole(&back), &a, &whole(&a), true).unwrap();
        assert!(d3 < 1e-5, "{d3}");
    }

    /// Two arcs that join the same two points but bow to opposite sides are not one edge.
    #[test]
    fn two_arcs_that_share_ends_are_not_certified() {
        let up = arc((0.0, 0.0), 1.0, 0.0, 90.0);
        let down = arc((1.0, 1.0), 1.0, 180.0, 270.0);
        assert!(certified_line_gap(&up, &whole(&up), &down, &whole(&down), false).is_none());
        assert!(certified_line_gap(&up, &whole(&up), &down, &whole(&down), true).is_none());
    }

    /// A line and an arc that join the same two points with a sagitta far past tolerance:
    /// the bound is at least the sagitta, so no tolerance below it relates them.
    #[test]
    fn a_bowed_arc_and_its_chord_are_farther_than_the_sagitta() {
        let a = arc((0.0, 0.0), 1.0, 0.0, 90.0);
        let l = line(Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0));
        let sagitta = 1.0 - (45.0_f64).to_radians().cos();
        let d = certified_line_gap(&l, &whole(&l), &a, &whole(&a), false).unwrap();
        assert!(d >= sagitta, "{d} below the sagitta {sagitta}");
        // reversed pairing of the ends is farther still
        let r = certified_line_gap(&l, &whole(&l), &a, &whole(&a), true).unwrap();
        assert!(r >= 1.0, "{r}");
    }
}
