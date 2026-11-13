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
//! square to it (other than the edge's own) ends the round, so the slab's square ends add
//! nothing the round could run into. The directions are evaluated at the edge's middle
//! ([`outward`]); the squareness is compared at [`SQUARE`].

use crate::blend::chamfer_side::outward;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, vertex, EdgeId, FaceId};
use crate::{Brep, Kernel, Point3, Result, SolidError, Vec3};

const OP: &str = "blend::carried_face::carried_faces";

/// How far from 1 the cosine between a face's normal and the edge may be for the face to be
/// square to the edge, and from 0 for the edge to lie in a face's plane.
pub const SQUARE: f64 = 1e-9;

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
    /// The slab's rectangle in (`along`, `across`) from `origin`: the edge's length along,
    /// `reach` to either side across.
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

/// The flat faces of the straight concave edge `e` that a round of radius `radius` can be
/// carried on past, each with its slab: the face of the edge's blend edge use first. None
/// where the construction does not apply: an edge that is not a straight line between two
/// vertices, not between two faces, or not concave; a face of it that is not planar; an end
/// of the edge without a planar face square to it.
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
    // a planar face square to the edge, other than its own, at each end
    for v in [start, end] {
        let mut squared = false;
        for f in vertex::faces(b, v)? {
            if own.contains(&f) {
                continue;
            }
            if plane_normal(k, b, f, tol)?.is_some_and(|n| n.dot(along).abs() > 1.0 - SQUARE) {
                squared = true;
                break;
            }
        }
        if !squared {
            return Ok(Vec::new());
        }
    }
    let reach = 4.0 * (size + radius);
    let mut out = Vec::new();
    let [first, second] = sides;
    for ((face, normal), (_, other)) in [(first, second), (second, first)] {
        let across = normal.cross(along);
        let other_side = if other.dot(across) < 0.0 { 1.0 } else { -1.0 };
        if plane_normal(k, b, face, tol)?.is_none() || normal.dot(along).abs() > SQUARE {
            continue;
        }
        out.push(CarriedFace {
            face,
            origin,
            along,
            across,
            outline: [
                [0.0, -reach],
                [length, -reach],
                [length, reach],
                [0.0, reach],
            ],
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
