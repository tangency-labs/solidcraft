//! Whether a plane is one of the planes square to a direction at a given height along it
//! (not upstream, #16713): the test of a prism's planar cap, which an extrusion's start and
//! end faces are. And whether a point, or a planar face by its corners, lies on such a plane
//! (not upstream, #16864): Delete Face extending a neighbour's plane across a gap.
//!
//! For a loft (not upstream, #17580): whether a section faces back from the one before it
//! ([`faces_back`]), and where a polyline leaves a plane ([`plane_departures`]): a closed guide
//! rail is opened where it leaves the first section.

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

/// Whether a loft section with plane normal `normal` and middle `middle` faces back from the
/// section before it, with normal `before_normal` (as turned to face along the loft) and middle
/// `before_middle`: whether the two normals point to opposite sides of the step from the one
/// middle to the other, one along the loft and the other against it. The rule is the same at
/// every angle between the planes: parallel planes face the same way when their normals do,
/// and at a quarter turn, where the normals are square whichever way the sections were drawn,
/// only the step tells. A normal square to the step faces neither way, and a section with it
/// does not face back; so does a value that is not finite.
#[must_use]
pub fn faces_back(before_normal: Vec3, before_middle: Vec3, normal: Vec3, middle: Vec3) -> bool {
    let step = middle - before_middle;
    let (a, b) = (before_normal.dot(step), normal.dot(step));
    (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0)
}

/// Where the polyline `points` leaves the plane through `origin` with unit normal `normal`, in
/// order along it: each as the index of the segment it is on and the point. A segment leaves
/// the plane where it crosses it (its ends on opposite sides, each farther than `tol`: the
/// point where the segment meets the plane) or where it starts on the plane (within `tol`) and
/// ends off it (its start). A run along the plane leaves it at its last point; a segment that
/// ends on the plane arrives and does not leave.
#[must_use]
pub fn plane_departures(
    points: &[Vec3],
    origin: Vec3,
    normal: Vec3,
    tol: f64,
) -> Vec<(usize, Vec3)> {
    let side = |q: Vec3| (q - origin).dot(normal);
    points
        .windows(2)
        .enumerate()
        .filter_map(|(k, w)| {
            let [a, b] = w else { return None };
            let (fa, fb) = (side(*a), side(*b));
            if fa.abs() <= tol && fb.abs() > tol {
                Some((k, *a))
            } else if fa.abs() > tol && fb.abs() > tol && (fa < 0.0) != (fb < 0.0) {
                Some((k, *a + (*b - *a) * (fa / (fa - fb))))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_faces_back_when_its_normal_and_the_one_before_part_along_the_step() {
        let (x, y, z) = (
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let o = Vec3::new(0.0, 0.0, 0.0);
        // Parallel planes: the normals decide, whichever way the step runs.
        assert!(!faces_back(z, o, z, Vec3::new(3.0, 0.0, 10.0)));
        assert!(faces_back(z, o, -z, Vec3::new(3.0, 0.0, 10.0)));
        assert!(!faces_back(z, o, z, Vec3::new(3.0, 0.0, -10.0)));
        // A quarter turn about z (#17580): a section on y = 0 facing +y, then one on x = 20
        // beyond it, its middle ahead along both: facing +x it faces along, facing -x back.
        let (m0, m1) = (Vec3::new(-15.0, 0.0, 5.0), Vec3::new(0.0, 15.0, 5.0));
        assert!(faces_back(y, m0, -x, m1), "facing -x, against the step");
        assert!(!faces_back(y, m0, x, m1), "facing +x, along the step");
        // A normal square to the step faces neither way.
        assert!(!faces_back(z, o, -z, Vec3::new(5.0, 0.0, 0.0)));
        assert!(!faces_back(z, o, -z, Vec3::new(0.0, 0.0, f64::NAN)));
    }

    #[test]
    fn a_polyline_leaves_a_plane_where_it_crosses_it_or_a_run_along_it_ends() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let o = Vec3::new(0.0, 0.0, 0.0);
        let p = |x: f64, zz: f64| Vec3::new(x, 0.0, zz);
        // Down through the plane, along it, up off it, back down onto it.
        let line = [
            p(0.0, 2.0),
            p(1.0, -2.0),
            p(2.0, 0.0),
            p(3.0, 0.0),
            p(4.0, 1.0),
            p(5.0, 0.0),
        ];
        let left = plane_departures(&line, o, z, 1.0e-9);
        assert_eq!(left.len(), 2, "{left:?}");
        assert_eq!(left[0].0, 0);
        assert!((left[0].1 - p(0.5, 0.0)).length() < 1.0e-12, "{left:?}");
        assert_eq!(left[1].0, 3);
        assert!(
            (left[1].1 - p(3.0, 0.0)).length() < 1.0e-12,
            "the run along the plane leaves at its end"
        );
        assert!(plane_departures(&line[..1], o, z, 1.0e-9).is_empty());
    }

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
