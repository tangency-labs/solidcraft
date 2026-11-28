//! The wedge a deleted round or chamfer leaves between two walls.
//!
//! Not upstream. Deleting a round or a chamfer along an edge between two planar walls leaves a
//! gap that the walls close by meeting at the sharp corner they would make. Where that corner
//! lies past another face of the part (a pocket's rounded corner beside the part's side), growing
//! the walls to it runs them through that face. Taking away (or adding) the gap's wedge with a
//! boolean closes the gap instead, and the boolean trims the wedge against every face of the part.
//!
//! [`wedge`] gives that wedge as the half-spaces whose intersection it is: the planes of the walls
//! and of the faces the gap's ends lie on (each on the side of the face's middle), and the plane
//! through the face's two straight edges, on the corner's side. For a chamfer that is the gap
//! exactly. For a round it also holds the segment between the round and the plane through its
//! straight edges, which lies on the round's other side: [`Wedge::segment`] is a point of it the
//! caller confirms is already as the boolean leaves it (air for a wedge added, material for one
//! taken away). [`Wedge::middle`] is the face's middle, which the caller confirms lies on the
//! body's boundary.
//!
//! Every function checks its input and returns `None` for a shape that is not this one (no two
//! parallel straight edges, a wall not unique, a corner at the face, a point on a plane) or a
//! number that is not finite; nothing here panics on input.

use crate::eval::vec::VecExt as _;
use crate::math::Vec3;

/// The face the gap is: a plane (a chamfer) or a cylinder about an axis along its straight edges
/// (a round).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GapFace {
    /// A planar face, with its outward normal.
    Plane {
        /// The outward normal.
        n: Vec3,
    },
    /// A cylindrical face: a point on its axis, the axis's unit direction, its radius, and whether
    /// it bulges outward (a round on an outside edge) rather than in (a pocket's round).
    Cylinder {
        /// A point on the axis.
        o: Vec3,
        /// The axis's unit direction.
        a: Vec3,
        /// The radius.
        r: f64,
        /// Whether the face is convex: its outward normal points away from the axis.
        convex: bool,
    },
}

/// A plane `n · p = d` of one of the body's faces, by the caller's index for the face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FacePlane {
    /// The caller's index for the face.
    pub face: usize,
    /// The plane's normal.
    pub n: Vec3,
    /// The plane's offset along `n`.
    pub d: f64,
}

/// The wedge of a gap ([`wedge`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Wedge {
    /// The half-spaces `n · p <= d` whose intersection the wedge is.
    pub sides: Vec<(Vec3, f64)>,
    /// For each of [`Wedge::sides`], the face whose plane it is, `None` for the plane through the
    /// gap's straight edges.
    pub faces: Vec<Option<usize>>,
    /// Whether the wedge is air to be added (the corner on the face's outward side), not material
    /// to be taken away.
    pub add: bool,
    /// The face's middle: a point on the body's boundary.
    pub middle: Vec3,
    /// For a round, the middle of the segment between the round and the plane through its
    /// straight edges: a point the wedge holds that is already as the boolean leaves it.
    pub segment: Option<Vec3>,
}

/// The wedge of the gap a deleted face leaves, for the face `face` with straight edges `edges`
/// (two, parallel) between two walls among `planes` (each wall holds one of the edges, and the
/// walls are distinct), the other planes those of the faces the gap's ends lie on. `tol` is the
/// distance under which a point lies on a plane. `None` when the gap is not that shape.
#[must_use]
pub fn wedge(
    edges: [(Vec3, Vec3); 2],
    face: GapFace,
    planes: &[FacePlane],
    tol: f64,
) -> Option<Wedge> {
    if !tol.is_finite() || tol <= 0.0 {
        return None;
    }
    let [(a0, a1), (b0, b1)] = edges;
    if ![a0, a1, b0, b1].iter().all(|p| p.is_finite()) {
        return None;
    }
    let u = (a1 - a0).normalized()?;
    if (b1 - b0).cross(u).len() > tol || (b0 - a0).cross(u).len() <= tol {
        return None;
    }
    let on = |pl: &FacePlane, p: Vec3| (pl.n.dot(p) - pl.d).abs() <= tol;
    let wall = |a: Vec3, b: Vec3| {
        let mut found = planes.iter().filter(|pl| on(pl, a) && on(pl, b));
        match (found.next(), found.next()) {
            (Some(w), None) => Some(*w),
            _ => None,
        }
    };
    let (pa, pb) = (wall(a0, a1)?, wall(b0, b1)?);
    if pa.face == pb.face {
        return None;
    }
    // The face's middle and its outward normal there; for a round, the middle of the segment
    // between it and the plane through its straight edges.
    let c = (a0 + a1 + b0 + b1) * 0.25;
    let (m, normal, segment) = match face {
        GapFace::Plane { n } => (c, n, None),
        GapFace::Cylinder { o, a, r, convex } => {
            if a.cross(u).len() > 1e-9 || !r.is_finite() {
                return None;
            }
            let q = o + a * (c - o).dot(a);
            let out = (c - q).normalized()?;
            let m = q + out * r;
            (m, if convex { out } else { -out }, Some((m + c) * 0.5))
        }
    };
    // The sharp corner where the walls meet, level with the middle.
    let den = pa.n.dot(pb.n.cross(u));
    if den.abs() < 1e-9 {
        return None;
    }
    let x =
        (pb.n.cross(u) * pa.d + u.cross(pa.n) * pb.d + pa.n.cross(pb.n) * u.dot(m)) * (1.0 / den);
    let toward = normal.dot(x - m);
    if !x.is_finite() || toward.abs() <= tol {
        return None;
    }
    let mut sides = Vec::new();
    let mut faces = Vec::new();
    for pl in planes {
        let s = pl.n.dot(m) - pl.d;
        if s.abs() <= tol {
            return None;
        }
        sides.push(if s < 0.0 {
            (pl.n, pl.d)
        } else {
            (-pl.n, -pl.d)
        });
        faces.push(Some(pl.face));
    }
    let k = u.cross(b0 - a0).normalized()?;
    let kd = k.dot(a0);
    let s = k.dot(x) - kd;
    if s.abs() <= tol {
        return None;
    }
    sides.push(if s < 0.0 { (k, kd) } else { (-k, -kd) });
    faces.push(None);
    Some(Wedge {
        sides,
        faces,
        add: toward > 0.0,
        middle: m,
        segment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(face: usize, n: [f64; 3], d: f64) -> FacePlane {
        FacePlane {
            face,
            n: Vec3::new(n[0], n[1], n[2]),
            d,
        }
    }

    /// A chamfer along the top front edge of a 10-unit cube, 1 unit each way: the corner is the
    /// cube's edge, on the chamfer's outward side, so the wedge is added and is the gap.
    #[test]
    fn a_chamfer_on_a_block_edge_is_a_wedge_added() {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let edges = [
            (p(0.0, 0.0, 9.0), p(10.0, 0.0, 9.0)),
            (p(0.0, 1.0, 10.0), p(10.0, 1.0, 10.0)),
        ];
        let n = Vec3::new(0.0, -1.0, 1.0) * std::f64::consts::FRAC_1_SQRT_2;
        let planes = [
            plane(0, [0.0, -1.0, 0.0], 0.0),
            plane(1, [0.0, 0.0, 1.0], 10.0),
            plane(2, [-1.0, 0.0, 0.0], 0.0),
            plane(3, [1.0, 0.0, 0.0], 10.0),
        ];
        let w = wedge(edges, GapFace::Plane { n }, &planes, 1e-9).unwrap();
        assert!(w.add);
        assert_eq!(w.faces, vec![Some(0), Some(1), Some(2), Some(3), None]);
        assert_eq!(w.segment, None);
        // the wedge holds the cube's edge and not the chamfer's far side
        let inside = |q: Vec3| w.sides.iter().all(|(n, d)| n.dot(q) <= d + 1e-12);
        assert!(inside(p(5.0, 0.0, 10.0)));
        assert!(inside(p(5.0, 0.25, 9.75)));
        assert!(!inside(p(5.0, 1.0, 9.0)));
    }

    /// Two straight edges that are not parallel are no gap between walls.
    #[test]
    fn edges_not_parallel_are_no_wedge() {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let edges = [
            (p(0.0, 0.0, 9.0), p(10.0, 0.0, 9.0)),
            (p(0.0, 1.0, 10.0), p(10.0, 2.0, 10.0)),
        ];
        let n = Vec3::new(0.0, -1.0, 1.0);
        assert_eq!(wedge(edges, GapFace::Plane { n }, &[], 1e-9), None);
        assert_eq!(wedge(edges, GapFace::Plane { n }, &[], f64::NAN), None);
    }
}
