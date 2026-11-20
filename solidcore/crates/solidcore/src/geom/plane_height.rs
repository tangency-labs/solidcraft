//! Whether a plane is one of the planes square to a direction at a given height along it
//! (not upstream, #16713): the test of a prism's planar cap, which an extrusion's start and
//! end faces are. And whether a point, or a planar face by its corners, lies on such a plane
//! (not upstream, #16864): Delete Face extending a neighbour's plane across a gap.

use crate::geom::vector3d::Vec3;

/// How far from parallel two unit normals may be for their planes to be square to each other:
/// the length of their cross product.
const SQUARE_TO: f64 = 1.0e-9;

/// Whether the plane through `face_origin` with unit normal `face_normal` is square to the unit
/// direction `normal` (the length of the cross product of the two normals under `1e-9`, so
/// either way round) and lies at `height` along `normal` from `origin`, to within `tol`. A
/// value that is not finite is no plane square to anything: `false`.
#[must_use]
pub fn is_square_plane_at(
    face_origin: Vec3,
    face_normal: Vec3,
    origin: Vec3,
    normal: Vec3,
    height: f64,
    tol: f64,
) -> bool {
    face_normal.cross(normal).length() < SQUARE_TO
        && ((face_origin - origin).dot(normal) - height).abs() <= tol
}

/// How far from the same direction two unit normals may be for their planes to face the same
/// way: one less their dot product.
const SAME_WAY: f64 = 1.0e-9;

/// Whether `point` lies within `tol` of the plane at `height` along the unit direction `normal`
/// from the world origin. A value that is not finite is on no plane: `false`.
#[must_use]
pub fn is_point_at(point: Vec3, normal: Vec3, height: f64, tol: f64) -> bool {
    (point.dot(normal) - height).abs() <= tol
}

/// Whether a planar face with unit normal `face_normal`, whose boundary has the corners
/// `corners`, lies on the plane at `height` along the unit direction `normal` from the world
/// origin, within `tol`: the normals point the same way (their dot within `1e-9` of one) and
/// every corner is on the plane ([`is_point_at`]). The normals alone do not settle it on a
/// large face: two within `1e-9` in dot (4.5e-5 rad) still part by 0.045 across a metre. No
/// corners, or a value that is not finite: `false`.
#[must_use]
pub fn is_face_on_plane_at(
    face_normal: Vec3,
    corners: &[Vec3],
    normal: Vec3,
    height: f64,
    tol: f64,
) -> bool {
    face_normal.dot(normal) > 1.0 - SAME_WAY
        && !corners.is_empty()
        && corners.iter().all(|p| is_point_at(*p, normal, height, tol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_square_to_the_direction_at_its_height_is_found() {
        let (o, n) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, 1.0));
        let at = |z: f64, face_n: Vec3| {
            is_square_plane_at(Vec3::new(9.0, -4.0, 3.0 + z), face_n, o, n, 5.0, 1.0e-6)
        };
        assert!(at(5.0, n));
        assert!(
            at(5.0, -n),
            "a plane facing the other way is the same plane"
        );
        assert!(at(5.0 + 0.5e-6, n));
        assert!(!at(5.0 + 2.0e-6, n), "past the tolerance");
        assert!(!at(0.0, n), "at another height");
        assert!(
            !at(5.0, Vec3::new(1.0, 0.0, 0.0)),
            "not square to the direction"
        );
        assert!(!at(5.0, Vec3::new(1.0e-8, 0.0, 1.0)), "tilted past 1e-9");
    }

    #[test]
    fn a_value_that_is_not_finite_is_no_such_plane() {
        let (o, n) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(!is_square_plane_at(
            Vec3::new(0.0, 0.0, f64::NAN),
            n,
            o,
            n,
            0.0,
            1.0
        ));
        assert!(!is_square_plane_at(
            Vec3::new(0.0, 0.0, 0.0),
            n,
            o,
            n,
            f64::NAN,
            1.0
        ));
    }

    #[test]
    fn a_face_is_on_the_plane_only_when_every_corner_is() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let flat = [
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(1000.0, 0.0, 10.0),
            Vec3::new(1000.0, 1000.0, 10.0),
            Vec3::new(0.0, 1000.0, 10.0),
        ];
        assert!(is_face_on_plane_at(z, &flat, z, 10.0, 1.0e-3));
        assert!(
            !is_face_on_plane_at(-z, &flat, z, 10.0, 1.0e-3),
            "facing the other way"
        );
        assert!(!is_face_on_plane_at(z, &[], z, 10.0, 1.0e-3), "no corners");
        assert!(is_point_at(
            Vec3::new(5.0, 5.0, 10.0 + 5.0e-4),
            z,
            10.0,
            1.0e-3
        ));
        // Tilted 4e-5 rad about x through (0, 0, 10) (sin, 1 - cos and tan to 1e-14): the
        // normals pass a 1e-9 dot test and the planes agree at (0, 0, 10), but the far side
        // of the 1000 face is 0.04 off.
        let (sin, cos, tan) = (4.0e-5, 1.0 - 8.0e-10, 4.0e-5);
        let tilted_n = Vec3::new(0.0, -sin, cos);
        assert!(z.dot(tilted_n) > 1.0 - SAME_WAY);
        let tilted: Vec<Vec3> = flat
            .iter()
            .map(|p| Vec3::new(p.x, p.y, 10.0 + p.y * tan))
            .collect();
        assert!(is_point_at(
            Vec3::new(0.0, 0.0, 10.0),
            tilted_n,
            10.0 * cos,
            1.0e-9
        ));
        assert!(!is_face_on_plane_at(tilted_n, &tilted, z, 10.0, 1.0e-3));
        assert!(!is_point_at(tilted[2], z, 10.0, 1.0e-3));
        assert!(!is_point_at(Vec3::new(0.0, 0.0, f64::NAN), z, 10.0, 1.0));
    }
}
