//! The wedge a deleted round or chamfer leaves between two walls, from the kernel's
//! [`solidcore::gap_wedge`] primitive (which computes every point and plane): the application
//! cuts it from the body, or adds it, with a boolean, where growing the walls to their sharp
//! corner would run them through the rest of the part. This module only takes and gives
//! coordinates as plain arrays.

use solidcore::gap_wedge as k;
use solidcore::Vec3;

/// The face the gap is ([`k::GapFace`]), in plain coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GapFace {
    /// A planar face, with its outward normal.
    Plane {
        /// The outward normal.
        n: [f64; 3],
    },
    /// A cylindrical face: a point on its axis, the axis's unit direction, its radius, and whether
    /// it bulges outward.
    Cylinder {
        /// A point on the axis.
        o: [f64; 3],
        /// The axis's unit direction.
        a: [f64; 3],
        /// The radius.
        r: f64,
        /// Whether the face is convex.
        convex: bool,
    },
}

/// The wedge of a gap ([`k::Wedge`]), in plain coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Wedge {
    /// The half-spaces `n · p <= d` whose intersection the wedge is.
    pub sides: Vec<([f64; 3], f64)>,
    /// For each side, the caller's face whose plane it is, `None` for the plane through the
    /// gap's straight edges.
    pub faces: Vec<Option<usize>>,
    /// Whether the wedge is air to be added, not material to be taken away.
    pub add: bool,
    /// The face's middle: a point the caller confirms is on the body's boundary.
    pub middle: [f64; 3],
    /// For a round, a point the wedge holds that is already as the boolean leaves it (air for a
    /// wedge added, material for one taken away), which the caller confirms.
    pub segment: Option<[f64; 3]>,
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn a(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// [`k::wedge`]: the wedge of the gap face `face` leaves, with straight edges `edges`, between two
/// of the faces whose planes `planes` lists (`(face, n, d)`, the plane `n · p = d`), the others
/// the faces its ends lie on; `tol` the distance under which a point lies on a plane. `None` when
/// the gap is not that shape.
#[must_use]
pub fn wedge(
    edges: [([f64; 3], [f64; 3]); 2],
    face: GapFace,
    planes: &[(usize, [f64; 3], f64)],
    tol: f64,
) -> Option<Wedge> {
    let face = match face {
        GapFace::Plane { n } => k::GapFace::Plane { n: v(n) },
        GapFace::Cylinder { o, a, r, convex } => k::GapFace::Cylinder {
            o: v(o),
            a: v(a),
            r,
            convex,
        },
    };
    let planes: Vec<k::FacePlane> = planes
        .iter()
        .map(|&(face, n, d)| k::FacePlane { face, n: v(n), d })
        .collect();
    let [(a0, a1), (b0, b1)] = edges;
    let w = k::wedge([(v(a0), v(a1)), (v(b0), v(b1))], face, &planes, tol)?;
    Some(Wedge {
        sides: w.sides.into_iter().map(|(n, d)| (a(n), d)).collect(),
        faces: w.faces,
        add: w.add,
        middle: a(w.middle),
        segment: w.segment.map(a),
    })
}
