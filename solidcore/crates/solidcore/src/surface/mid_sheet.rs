//! The plane geometry of a mid-sheet: what lies inside a thin wall (so a face no wider than the
//! wall, its rim, is told from one that needs a sheet of its own), a planar polygon's area, a
//! point dropped onto a plane, and a point's coordinates in a plane's frame and back.
//!
//! A wall is the material between two faces that face each other
//! ([`face_pair`](crate::surface::face_pair) for planes,
//! [`face_pair_cylinder`](crate::surface::face_pair_cylinder) for cylinders): a flat wall is the
//! slab behind its first face, as thick as the pair is; a round wall is the shell between the two
//! radii about the shared axis. A point lies in it when it is within `slack` of that slab or shell.
//!
//! Not upstream: no counterpart in SMLib. Deviations: none. No recursion.

use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::surface::face_pair::OutwardPlane;

const HOLDS: &str = "surface::mid_sheet::wall_holds";
const AREA: &str = "surface::mid_sheet::polygon_area";
const FRAME: &str = "surface::mid_sheet::frame";

/// The most points one call takes.
pub const MAX_POINTS: usize = 1_000_000;

/// A thin wall, as two faces that face each other bound it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wall {
    /// A flat wall: the slab behind `face` (its outward normal points out of the wall),
    /// `thickness` deep.
    Flat {
        /// The wall's first face.
        face: OutwardPlane,
        /// The distance to the other face (positive).
        thickness: f64,
    },
    /// A round wall: the shell between two radii about one axis.
    Round {
        /// A point on the axis.
        axis_point: Point3,
        /// The axis direction (any nonzero length).
        axis: Vec3,
        /// The inner face's radius (positive).
        inner: f64,
        /// The outer face's radius (more than the inner).
        outer: f64,
    },
}

/// A plane frame: an origin and two unit axes at right angles in the plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The frame's origin.
    pub origin: Point3,
    /// The first axis (unit length).
    pub x: Vec3,
    /// The second axis (unit length, at right angles to `x`).
    pub y: Vec3,
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Whether every one of `points` lies in `wall`, within `slack`: no farther in front of the
/// first face or behind the second than `slack` (flat), or no nearer the axis than the inner
/// radius less `slack` nor farther than the outer radius and `slack` (round). True for no points.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a wall, point or slack that is not finite, a negative slack, a
/// thickness or radius that is not positive, an outer radius no larger than the inner, a normal
/// or axis of zero length, or more than [`MAX_POINTS`] points.
pub fn wall_holds(wall: &Wall, points: &[Point3], slack: f64) -> Result<bool> {
    let bad = |reason| SolidError::InvalidInput {
        operation: HOLDS,
        reason,
    };
    if points.len() > MAX_POINTS {
        return Err(bad("too many points"));
    }
    if !slack.is_finite() || slack < 0.0 {
        return Err(bad("the slack must be a finite number, zero or more"));
    }
    if !points.iter().all(|p| finite(*p)) {
        return Err(bad("every point must be finite"));
    }
    match *wall {
        Wall::Flat { face, thickness } => {
            if !(finite(face.point) && finite(face.outward) && thickness.is_finite()) {
                return Err(bad("a flat wall's face and thickness must be finite"));
            }
            if thickness <= 0.0 {
                return Err(bad("a flat wall's thickness must be positive"));
            }
            let n = face.outward.unitize()?;
            // How deep behind the first face: 0 on it, the thickness on the other face.
            Ok(points.iter().all(|p| {
                let depth = face.point.distance_along(*p, n);
                depth >= -slack && depth <= thickness + slack
            }))
        }
        Wall::Round {
            axis_point,
            axis,
            inner,
            outer,
        } => {
            if !(finite(axis_point) && finite(axis) && inner.is_finite() && outer.is_finite()) {
                return Err(bad("a round wall's axis and radii must be finite"));
            }
            if inner <= 0.0 || outer <= inner {
                return Err(bad(
                    "a round wall's radii must be positive, the outer larger than the inner",
                ));
            }
            let u = axis.unitize()?;
            Ok(points.iter().all(|p| {
                let from = *p - axis_point;
                let radial = (from - (u * from.dot(u))).length();
                radial >= inner - slack && radial <= outer + slack
            }))
        }
    }
}

/// The area of the planar polygon through `corners` in order (the length of half the sum of
/// the cross products of consecutive corners; for a polygon that is not planar, the area of its
/// projection onto its best plane). Zero for fewer than three corners.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a corner that is not finite or more than [`MAX_POINTS`]
/// corners.
pub fn polygon_area(corners: &[Point3]) -> Result<f64> {
    if corners.len() > MAX_POINTS {
        return Err(SolidError::InvalidInput {
            operation: AREA,
            reason: "too many corners",
        });
    }
    if !corners.iter().all(|p| finite(*p)) {
        return Err(SolidError::InvalidInput {
            operation: AREA,
            reason: "every corner must be finite",
        });
    }
    let next = corners.iter().cycle().skip(1);
    let sum = corners
        .iter()
        .zip(next)
        .fold(Vec3::new(0.0, 0.0, 0.0), |s, (a, b)| s + a.cross(*b));
    Ok(sum.length() * 0.5)
}

/// `point` dropped onto `plane` along its normal.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or plane that is not finite, or a normal of zero
/// length.
pub fn onto_plane(plane: &OutwardPlane, point: Point3) -> Result<Point3> {
    if !(finite(point) && finite(plane.point) && finite(plane.outward)) {
        return Err(SolidError::InvalidInput {
            operation: FRAME,
            reason: "the point and the plane must be finite",
        });
    }
    let n = plane.outward.unitize()?;
    Ok(point - (n * point.distance_along(plane.point, n)))
}

fn check_frame(frame: &Frame) -> Result<()> {
    if finite(frame.origin) && finite(frame.x) && finite(frame.y) {
        Ok(())
    } else {
        Err(SolidError::InvalidInput {
            operation: FRAME,
            reason: "the frame must be finite",
        })
    }
}

/// `point`'s coordinates in `frame`: how far along each axis it stands from the origin (the
/// point dropped onto the frame's plane, read in its axes).
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or frame that is not finite.
pub fn in_frame(frame: &Frame, point: Point3) -> Result<[f64; 2]> {
    check_frame(frame)?;
    if !finite(point) {
        return Err(SolidError::InvalidInput {
            operation: FRAME,
            reason: "the point must be finite",
        });
    }
    Ok([
        point.distance_along(frame.origin, frame.x),
        point.distance_along(frame.origin, frame.y),
    ])
}

/// The point at coordinates `uv` of `frame`.
///
/// # Errors
/// [`SolidError::InvalidInput`] for coordinates or a frame that are not finite.
pub fn from_frame(frame: &Frame, uv: [f64; 2]) -> Result<Point3> {
    check_frame(frame)?;
    let [u, v] = uv;
    if !(u.is_finite() && v.is_finite()) {
        return Err(SolidError::InvalidInput {
            operation: FRAME,
            reason: "the coordinates must be finite",
        });
    }
    Ok(frame.origin + (frame.x * u) + (frame.y * v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn a_plates_end_face_lies_in_its_wall_and_a_boss_does_not() {
        // A plate 2 thick: top face z = 2 facing up, so the wall is 0 <= z <= 2.
        let plate = Wall::Flat {
            face: OutwardPlane {
                point: v(7.0, -3.0, 2.0),
                outward: v(0.0, 0.0, 5.0),
            },
            thickness: 2.0,
        };
        // The end face x = 40: its corners at z = 0 and z = 2.
        let end = [
            v(40.0, 0.0, 0.0),
            v(40.0, 30.0, 0.0),
            v(40.0, 30.0, 2.0),
            v(40.0, 0.0, 2.0),
        ];
        assert!(wall_holds(&plate, &end, 0.0).unwrap());
        // A boss standing 14 above the top: its top corners are 14 out of the wall.
        let boss = [v(40.0, 25.0, 2.0), v(40.0, 25.0, 16.0)];
        assert!(!wall_holds(&plate, &boss, 1e-6).unwrap());
        // Just past the face by less than the slack is in; by more is out.
        assert!(wall_holds(&plate, &[v(0.0, 0.0, 2.0 + 1e-7)], 1e-6).unwrap());
        assert!(!wall_holds(&plate, &[v(0.0, 0.0, -2e-6)], 1e-6).unwrap());
        assert!(wall_holds(&plate, &[], 0.0).unwrap());
    }

    #[test]
    fn a_tubes_end_ring_lies_in_its_wall_and_its_axis_does_not() {
        // Radii 8 and 10 about the z axis through (1, 2, 0).
        let tube = Wall::Round {
            axis_point: v(1.0, 2.0, 0.0),
            axis: v(0.0, 0.0, -3.0),
            inner: 8.0,
            outer: 10.0,
        };
        // The end ring at z = 50: radius 8 and 10 on the x side, 9 at 45 degrees (9/sqrt 2 each way).
        let s = 9.0 * 0.5_f64.sqrt();
        let ring = [
            v(9.0, 2.0, 50.0),
            v(11.0, 2.0, 50.0),
            v(1.0 + s, 2.0 + s, 50.0),
        ];
        assert!(wall_holds(&tube, &ring, 0.0).unwrap());
        assert!(!wall_holds(&tube, &[v(1.0, 2.0, 50.0)], 1e-6).unwrap());
        assert!(!wall_holds(&tube, &[v(12.0, 2.0, 0.0)], 1e-6).unwrap());
    }

    #[test]
    fn nonsense_walls_and_points_are_refused() {
        let good = Wall::Flat {
            face: OutwardPlane {
                point: v(0.0, 0.0, 0.0),
                outward: v(0.0, 0.0, 1.0),
            },
            thickness: 1.0,
        };
        let p = [v(0.0, 0.0, 0.5)];
        assert!(wall_holds(&good, &p, f64::NAN).is_err());
        assert!(wall_holds(&good, &p, -1.0).is_err());
        assert!(wall_holds(&good, &[v(f64::INFINITY, 0.0, 0.0)], 0.0).is_err());
        for wall in [
            Wall::Flat {
                face: OutwardPlane {
                    point: v(0.0, 0.0, 0.0),
                    outward: v(0.0, 0.0, 0.0),
                },
                thickness: 1.0,
            },
            Wall::Flat {
                face: OutwardPlane {
                    point: v(0.0, 0.0, 0.0),
                    outward: v(0.0, 0.0, 1.0),
                },
                thickness: -1.0,
            },
            Wall::Round {
                axis_point: v(0.0, 0.0, 0.0),
                axis: v(0.0, 0.0, 1.0),
                inner: 5.0,
                outer: 5.0,
            },
            Wall::Round {
                axis_point: v(0.0, 0.0, 0.0),
                axis: v(0.0, 0.0, 1.0),
                inner: 0.0,
                outer: 5.0,
            },
            Wall::Round {
                axis_point: v(0.0, f64::NAN, 0.0),
                axis: v(0.0, 0.0, 1.0),
                inner: 1.0,
                outer: 5.0,
            },
        ] {
            assert!(wall_holds(&wall, &p, 0.0).is_err(), "{wall:?}");
        }
    }

    #[test]
    fn a_polygons_area_is_its_closed_form_in_any_plane() {
        // A 38 x 20 rectangle in the plane y = 1: 760.
        let rect = [
            v(2.0, 1.0, 0.0),
            v(40.0, 1.0, 0.0),
            v(40.0, 1.0, 20.0),
            v(2.0, 1.0, 20.0),
        ];
        assert!((polygon_area(&rect).unwrap() - 760.0).abs() < 1e-9);
        // An L of legs 40 and 30, 2 wide, in z = 5: 40 * 2 + 28 * 2 = 136, either winding.
        let l = [
            v(0.0, 0.0, 5.0),
            v(40.0, 0.0, 5.0),
            v(40.0, 2.0, 5.0),
            v(2.0, 2.0, 5.0),
            v(2.0, 30.0, 5.0),
            v(0.0, 30.0, 5.0),
        ];
        assert!((polygon_area(&l).unwrap() - 136.0).abs() < 1e-9);
        let back: Vec<Vec3> = l.iter().rev().copied().collect();
        assert!((polygon_area(&back).unwrap() - 136.0).abs() < 1e-9);
        // A triangle tilted 45 degrees: legs 2 along x and 2 sqrt 2 up the slope: area 2 sqrt 2.
        let tri = [v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(0.0, 2.0, 2.0)];
        assert!((polygon_area(&tri).unwrap() - 2.0 * 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(polygon_area(&rect[..2]).unwrap(), 0.0);
        assert!(polygon_area(&[v(f64::NAN, 0.0, 0.0), rect[1], rect[2]]).is_err());
    }

    #[test]
    fn a_point_drops_onto_a_plane_and_reads_back_in_its_frame() {
        // The plane x + y = 2 (normal (1, 1, 0)/sqrt 2): (3, 3, 7) is 4/sqrt 2 in front of it and
        // drops to (1, 1, 7).
        let plane = OutwardPlane {
            point: v(2.0, 0.0, 0.0),
            outward: v(4.0, 4.0, 0.0),
        };
        let p = onto_plane(&plane, v(3.0, 3.0, 7.0)).unwrap();
        assert!((p - v(1.0, 1.0, 7.0)).length() < 1e-12, "{p:?}");
        assert!(onto_plane(&plane, v(f64::NAN, 0.0, 0.0)).is_err());
        // A frame at (1, 1, 1) with axes x and z: (4, 9, -2) reads (3, -3), and back to (4, 1, -2).
        let frame = Frame {
            origin: v(1.0, 1.0, 1.0),
            x: v(1.0, 0.0, 0.0),
            y: v(0.0, 0.0, 1.0),
        };
        let uv = in_frame(&frame, v(4.0, 9.0, -2.0)).unwrap();
        assert!(
            (uv[0] - 3.0).abs() < 1e-12 && (uv[1] + 3.0).abs() < 1e-12,
            "{uv:?}"
        );
        let back = from_frame(&frame, uv).unwrap();
        assert!((back - v(4.0, 1.0, -2.0)).length() < 1e-12, "{back:?}");
        assert!(from_frame(&frame, [f64::INFINITY, 0.0]).is_err());
        let broken = Frame {
            origin: v(f64::NAN, 0.0, 0.0),
            ..frame
        };
        assert!(in_frame(&broken, v(0.0, 0.0, 0.0)).is_err());
    }
}
