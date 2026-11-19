//! The flat faces a round of a concave straight edge can be carried on past: for an
//! application that builds a round wider than the flat face beside its edge (a boss's foot
//! on a plate whose face turns into a rounded corner within the round's width,
//! tangency-labs/solidcore#3252) by rounding the edge on the body with that face carried on
//! as a slab behind its plane, and keeping the material the round adds within the face's
//! outline.
//!
//! Not upstream (solidcore's). [`carried_faces`] decides where that construction gives the
//! round the kernel would build on an unbounded face, and measures the slab: the edge is a
//! straight line between two vertices ([`crate::curve::curve::Curve::is_linear`]), concave
//! ([`is_concave_edge`]), a face of it is planar, and at each end of the edge a planar face
//! (other than the edge's own) ends the round: one whose plane holds the carried face's
//! normal and crosses the edge no flatter than [`LEAN`]. The slab is what lies behind the
//! face's plane between those two planes, so each of its ends lies in the plane of the face
//! that ends the round there and adds nothing the round could run into. A face square to
//! the edge is such a face for both faces of the edge, and the slab's outline is then a
//! rectangle; a face that leans (the edge ends on a sloped cut) is one for the face of the
//! edge whose normal its plane holds, and the outline's end follows its trace
//! ([`slab_outline`]). The directions are evaluated at the edge's middle ([`outward`]); the
//! squareness is compared at [`SQUARE`].

use crate::blend::chamfer_side::outward;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, vertex, EdgeId, FaceId};
use crate::{Brep, Kernel, Point3, Result, SolidError, Vec3};

const OP: &str = "blend::carried_face::carried_faces";

/// How far from 0 the cosine between two directions may be for them to be square to each
/// other: the edge and the normal of a face it lies in, the carried face's normal and that
/// of a face whose plane holds it.
pub const SQUARE: f64 = 1e-9;

/// The least cosine between the edge and the normal of a face that ends the round: such a
/// face leans at most 60 degrees from square to the edge, so its trace in the carried face's
/// plane runs at most √3 along the edge for each unit across it.
pub const LEAN: f64 = 0.5;

/// A flat face of a concave straight edge and the slab that carries it on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarriedFace {
    /// The flat face.
    pub face: FaceId,
    /// The edge's start vertex: the origin of the slab's frame.
    pub origin: Point3,
    /// The unit direction of the edge, from its start to its end.
    pub along: Vec3,
    /// The unit direction in the face's plane square to the edge, with
    /// `along` × `across` the face's outward normal.
    pub across: Vec3,
    /// The slab's outline in (`along`, `across`) from `origin` ([`slab_outline`]): the edge
    /// from the first side to the second, `reach` to either side across it (less on a side
    /// where the ends' traces close on each other), each end along the trace of the face
    /// that ends the round there. A rectangle between square ends.
    pub outline: [[f64; 2]; 4],
    /// The slab's extent along the outward normal: from `-reach` to the face's plane.
    pub depth: [f64; 2],
    /// How far the slab reaches behind the plane and to either side of the edge, and how
    /// far to either side of the plane the face's outline is to be carried: four times the
    /// body's diagonal and the radius together, past everything the round can touch.
    pub reach: f64,
    /// Which way along `across` from the edge's line the other face's material stands, in the
    /// face's plane (`1.0` or `-1.0`): against the other face's outward normal, the edge being
    /// concave.
    pub other_side: f64,
}

/// Whether `face`'s surface is a plane within `tol`, and then its unit normal (either way).
fn plane_normal(k: &Kernel, b: &Brep, face: FaceId, tol: f64) -> Result<Option<Vec3>> {
    let Some(surface) = b.face(face)?.surface.as_ref() else {
        return Ok(None);
    };
    let planar = surface.is_planar(k, tol)?;
    let len = planar.normal.length();
    if !(planar.is_planar && len > 0.0 && len.is_finite()) {
        return Ok(None);
    }
    Ok(Some(planar.normal * (1.0 / len)))
}

/// The outline of the slab that carries a face on past an edge of length `length`, in the
/// face's plane: (`along`, `across`) from the edge's start, as
/// [`CarriedFace::outline`] lists it (the start's side at `-across`, the end's, the end's
/// at `+across`, the start's). The slab's start follows the trace `along = start · across`
/// of the face that ends the round there and its end `along = length + end · across`
/// (`start` and `end` are 0 for faces square to the edge), out to `reach` on either side.
///
/// Where the traces close on each other on a side they cross `length / |end - start|` from
/// the edge's line, and the slab has nothing past there. On that side it reaches half way
/// from `need` (how far from the edge's line the blend touches the face) to the crossing,
/// so it is past the blend's contact line and short of the crossing by as much. `None`
/// where the crossing is no further than `need`: the blend's contact line has no room
/// between the two faces.
#[must_use]
pub fn slab_outline(
    length: f64,
    start: f64,
    end: f64,
    need: f64,
    reach: f64,
) -> Option<[[f64; 2]; 4]> {
    let closing = end - start;
    // how far the slab reaches at -across and at +across
    let mut sides = [reach, reach];
    if closing != 0.0 {
        let crossing = length / closing.abs();
        if crossing <= need {
            return None;
        }
        let side = usize::from(closing < 0.0);
        if let Some(s) = sides.get_mut(side) {
            *s = reach.min(0.5 * (need + crossing));
        }
    }
    let [low, high] = sides;
    // between square ends (both traces 0) these are exactly 0 and `length`
    let from = |across: f64| 0.0 + start * across;
    let to = |across: f64| length + end * across;
    Some([
        [from(-low), -low],
        [to(-low), -low],
        [to(high), high],
        [from(high), high],
    ])
}

/// How far `along` a trace runs for each unit `across`, for the planar face at the vertex
/// `v` that ends a round carried on past the face of normal `normal`: a face other than
/// `own` whose plane holds `normal` ([`SQUARE`]) and crosses the edge no flatter than
/// [`LEAN`]; of several, the nearest to square. A face square to the edge ([`SQUARE`]) has
/// the trace 0 exactly, not its normal's rounding, so the slab between square ends is the
/// rectangle to the last bit. `None` without one.
fn end_trace(
    k: &Kernel,
    b: &Brep,
    v: crate::topology::VertexId,
    own: [FaceId; 2],
    frame: [Vec3; 3],
    tol: f64,
) -> Result<Option<f64>> {
    let [along, across, normal] = frame;
    let mut best: Option<f64> = None;
    for f in vertex::faces(b, v)? {
        if own.contains(&f) {
            continue;
        }
        let Some(n) = plane_normal(k, b, f, tol)? else {
            continue;
        };
        let lean = n.dot(along);
        if lean.abs() > 1.0 - SQUARE {
            return Ok(Some(0.0));
        }
        if n.dot(normal).abs() > SQUARE || lean.abs() < LEAN {
            continue;
        }
        let trace = -n.dot(across) / lean;
        if best.is_none_or(|least| trace.abs() < least.abs()) {
            best = Some(trace);
        }
    }
    Ok(best)
}

/// The flat faces of the straight concave edge `e` that a round of radius `radius` can be
/// carried on past, each with its slab: the face of the edge's blend edge use first. None
/// where the construction does not apply: an edge that is not a straight line between two
/// vertices, not between two faces, or not concave; a face of it that is not planar; an end
/// of the edge without a planar face that ends the round for that face ([`end_trace`]); ends
/// whose traces cross within the round's contact line ([`slab_outline`]). `radius` is how
/// far the blend reaches from the edge: the contact line of a round lies `radius` times the
/// tangent of half the angle between the two faces' normals from it, and no blend is taken
/// to touch the face further off than the larger of the two.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a radius that is not finite and positive; the
/// evaluators' and the topology queries' errors.
pub fn carried_faces(k: &Kernel, b: &mut Brep, e: EdgeId, radius: f64) -> Result<Vec<CarriedFace>> {
    if !(radius.is_finite() && radius > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the radius is finite and positive",
        ));
    }
    let (Some(start), Some(end)) = (edge::start_vertex(b, e)?, edge::end_vertex(b, e)?) else {
        return Ok(Vec::new());
    };
    if start == end {
        return Ok(Vec::new());
    }
    let (origin, tip) = (vertex::point(b, start)?, vertex::point(b, end)?);
    let span = tip - origin;
    let length = span.length();
    if !(length > 0.0 && length.is_finite()) {
        return Ok(Vec::new());
    }
    let along = span * (1.0 / length);
    let Some(u0) = edge::blend_edgeuse(k, b, e)? else {
        return Ok(Vec::new());
    };
    let Some(u1) = eu1::radial(b, u0)? else {
        return Ok(Vec::new());
    };
    if !is_concave_edge(k, b, e)? {
        return Ok(Vec::new());
    }
    let size = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?
        .size()
        .length();
    if !(size > 0.0 && size.is_finite()) {
        return Ok(Vec::new());
    }
    let tol = (size * 1e-9).max(1e-9);
    // a straight line, whatever curve it is kept as (a prism's upright edge is a B-spline)
    let straight = match edge::curve(b, e)? {
        Some(c) => c.is_linear(k, tol, false)?.linear,
        None => false,
    };
    if !straight {
        return Ok(Vec::new());
    }
    let t = b.edge(e)?.interval.evaluate(0.5);
    let sides = [outward(k, b, u0, t)?, outward(k, b, u1, t)?];
    let own = [sides[0].0, sides[1].0];
    let reach = 4.0 * (size + radius);
    let mut out = Vec::new();
    let [first, second] = sides;
    for ((face, normal), (_, other)) in [(first, second), (second, first)] {
        let across = normal.cross(along);
        let other_side = if other.dot(across) < 0.0 { 1.0 } else { -1.0 };
        if plane_normal(k, b, face, tol)?.is_none() || normal.dot(along).abs() > SQUARE {
            continue;
        }
        // the face that ends the round at each end of the edge, for this face
        let frame = [along, across, normal];
        let (Some(at_start), Some(at_end)) = (
            end_trace(k, b, start, own, frame, tol)?,
            end_trace(k, b, end, own, frame, tol)?,
        ) else {
            continue;
        };
        // the contact line of a round between faces whose normals are an angle apart lies
        // the radius times the tangent of half that angle from the edge
        let cos = normal.dot(other).clamp(-1.0, 1.0);
        let half_tan = ((1.0 - cos) / (1.0 + cos).max(f64::MIN_POSITIVE)).sqrt();
        let need = radius * half_tan.max(1.0);
        let Some(outline) = slab_outline(length, at_start, at_end, need, reach) else {
            continue;
        };
        out.push(CarriedFace {
            face,
            origin,
            along,
            across,
            outline,
            depth: [-reach, 0.0],
            reach,
            other_side,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::primitive::api_curves::create_line_segment;
    use crate::query::api_queries::edge_closest_point;
    use crate::sweep::api_brep::create_taper_extrude;

    /// The prism of the polygon `pts` (in xy, counter-clockwise) from z = 0 to `h`.
    fn prism(k: &mut Kernel, pts: &[(f64, f64)], h: f64) -> Brep {
        let n = pts.len();
        let curves: Vec<Curve> = (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                Curve::from(
                    create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0))
                        .unwrap(),
                )
            })
            .collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        create_taper_extrude(k, &refs, h, 0.0, 3, false).unwrap()
    }

    /// The edge of `b` through `p`.
    fn edge_at(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
        b.edge_list()
            .iter()
            .copied()
            .find(|&e| edge_closest_point(k, b, e, p).unwrap().closest.distance < 1e-9)
            .unwrap()
    }

    /// An L of legs 40 and width 20, 10 high: its inner upright edge at (20, 20) is concave,
    /// between the planes y = 20 (facing +y) and x = 20 (facing +x), and ends on the flat top
    /// and bottom. Both faces can be carried on: the slab's frame starts at the edge's start,
    /// runs the edge's 10 along it, and `along` × `across` is the face's outward normal. The
    /// body's diagonal is √(40² + 40² + 10²) = √3300, so a radius 6 reaches 4 (√3300 + 6).
    #[test]
    fn an_inner_edge_of_an_l_carries_both_its_faces_on() {
        let mut k = Kernel::default();
        let l = [
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (20.0, 20.0),
            (20.0, 40.0),
            (0.0, 40.0),
        ];
        let mut b = prism(&mut k, &l, 10.0);
        let e = edge_at(&k, &b, Point3::new(20.0, 20.0, 5.0));
        let found = carried_faces(&k, &mut b, e, 6.0).unwrap();
        assert_eq!(found.len(), 2);
        let reach = 4.0 * (3300.0_f64.sqrt() + 6.0);
        let mut normals = Vec::new();
        for c in &found {
            assert!((c.along.length() - 1.0).abs() < 1e-12 && c.along.z.abs() > 1.0 - 1e-12);
            assert!((c.origin - Point3::new(20.0, 20.0, c.origin.z)).length() < 1e-9);
            assert!(c.across.dot(c.along).abs() < 1e-12);
            // the box the kernel measures may be padded: no less than the exact reach, and close
            assert!(
                c.reach >= reach - 1e-9 && c.reach < reach * 1.01,
                "{} vs {reach}",
                c.reach
            );
            assert_eq!(
                c.outline,
                [
                    [0.0, -c.reach],
                    [10.0, -c.reach],
                    [10.0, c.reach],
                    [0.0, c.reach]
                ]
            );
            assert_eq!(c.depth, [-c.reach, 0.0]);
            // the material in the face's plane stands toward the other face's inside: in
            // y = 20 at x < 20, in x = 20 at y < 20
            let n = c.along.cross(c.across);
            let toward = c.across * c.other_side;
            assert!(
                (toward - Vec3::new(-n.y, -n.x, 0.0)).length() < 1e-9,
                "{toward:?} for {n:?}"
            );
            normals.push(n);
        }
        let is = |n: Vec3, x: f64, y: f64| (n - Vec3::new(x, y, 0.0)).length() < 1e-9;
        assert!(
            normals.iter().any(|&n| is(n, 1.0, 0.0)) && normals.iter().any(|&n| is(n, 0.0, 1.0)),
            "{normals:?}"
        );
    }

    /// The slab's outline between square ends is the rectangle of the edge's length and the
    /// reach to either side. Between ends that lean alike (0.5 along for each unit across)
    /// it is a parallelogram as wide. Where only the end leans, by 0.25, the traces close on
    /// the side across is negative and cross 10 / 0.25 = 40 from the edge's line: a blend
    /// that touches the face 6 from the edge has its slab reach (6 + 40) / 2 = 23 there,
    /// where the end's trace has come back 0.25 x 23 = 5.75, and the full reach on the other
    /// side. Leaning the other way the short side is the other one. A blend that touches the
    /// face as far off as the traces cross, or further, has no slab.
    #[test]
    fn the_slabs_outline_follows_its_ends_traces() {
        let reach = 500.0;
        assert_eq!(
            slab_outline(10.0, 0.0, 0.0, 6.0, reach),
            Some([[0.0, -reach], [10.0, -reach], [10.0, reach], [0.0, reach]])
        );
        assert_eq!(
            slab_outline(10.0, 0.5, 0.5, 6.0, reach),
            Some([
                [-250.0, -reach],
                [-240.0, -reach],
                [260.0, reach],
                [250.0, reach]
            ])
        );
        assert_eq!(
            slab_outline(10.0, 0.0, 0.25, 6.0, reach),
            Some([[0.0, -23.0], [4.25, -23.0], [135.0, reach], [0.0, reach]])
        );
        assert_eq!(
            slab_outline(10.0, 0.0, -0.25, 6.0, reach),
            Some([[0.0, -reach], [135.0, -reach], [4.25, 23.0], [0.0, 23.0]])
        );
        // the start leaning toward the end closes the same side as the end leaning back
        assert_eq!(
            slab_outline(10.0, 0.25, 0.0, 6.0, reach),
            Some([[-125.0, -reach], [10.0, -reach], [10.0, 23.0], [5.75, 23.0]])
        );
        // a reach short of the half-way point is the reach
        assert_eq!(
            slab_outline(10.0, 0.0, 0.25, 6.0, 20.0),
            Some([[0.0, -20.0], [5.0, -20.0], [15.0, 20.0], [0.0, 20.0]])
        );
        assert!(slab_outline(10.0, 0.0, 0.25, 39.0, reach).is_some());
        assert_eq!(slab_outline(10.0, 0.0, 0.25, 40.0, reach), None);
        assert_eq!(slab_outline(10.0, 0.0, -0.25, 41.0, reach), None);
    }

    /// A convex edge of the same L has nothing to carry on, and neither has a concave edge
    /// of an L with tapered sides (the edge leans, so its flat top and bottom are not square
    /// to it). A radius that is not finite and positive is refused.
    #[test]
    fn convex_edges_and_unsquare_ends_carry_nothing() {
        let mut k = Kernel::default();
        let l = [
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (20.0, 20.0),
            (20.0, 40.0),
            (0.0, 40.0),
        ];
        let mut b = prism(&mut k, &l, 10.0);
        let convex = edge_at(&k, &b, Point3::new(40.0, 0.0, 5.0));
        assert!(carried_faces(&k, &mut b, convex, 6.0).unwrap().is_empty());
        let rim = edge_at(&k, &b, Point3::new(30.0, 20.0, 10.0));
        assert!(carried_faces(&k, &mut b, rim, 6.0).unwrap().is_empty());
        let inner = edge_at(&k, &b, Point3::new(20.0, 20.0, 5.0));
        assert!(carried_faces(&k, &mut b, inner, 0.0).is_err());
        assert!(carried_faces(&k, &mut b, inner, f64::NAN).is_err());
        // tapered sides: the inner edge leans, so the top and bottom are not square to it
        let curves: Vec<Curve> = (0..l.len())
            .map(|i| {
                let (a, z) = (l[i], l[(i + 1) % l.len()]);
                Curve::from(
                    create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(z.0, z.1, 0.0))
                        .unwrap(),
                )
            })
            .collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let mut tapered = create_taper_extrude(&mut k, &refs, 10.0, 0.1, 3, false).unwrap();
        let edges = tapered.edge_list().to_vec();
        let mut leaning = Vec::new();
        for e in edges {
            if is_concave_edge(&k, &mut tapered, e).unwrap_or(false) {
                leaning.push(e);
            }
        }
        assert!(!leaning.is_empty());
        for e in leaning {
            assert!(carried_faces(&k, &mut tapered, e, 6.0).unwrap().is_empty());
        }
    }
}
