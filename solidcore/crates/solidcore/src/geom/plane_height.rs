//! Whether a plane is one of the planes square to a direction at a given height along it
//! (not upstream, #16713): the test of a prism's planar cap, which an extrusion's start and
//! end faces are.

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
}
