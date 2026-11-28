//! A mid-sheet's plane geometry for an application ([`solidcore::surface::mid_sheet`]): whether
//! points lie inside a thin wall (a face no wider than the wall is its rim), a planar polygon's
//! area, a point dropped onto a plane, and a point's coordinates in a plane frame and back. Ops
//! converts the arrays; the kernel computes.

use solidcore::surface::face_pair::OutwardPlane as KernelPlane;
use solidcore::surface::mid_sheet::{
    from_frame as kernel_from_frame, in_frame as kernel_in_frame, onto_plane as kernel_onto_plane,
    polygon_area as kernel_polygon_area, wall_holds as kernel_wall_holds, Frame as KernelFrame,
    Wall as KernelWall,
};
use solidcore::Vec3;

use crate::face_pair::OutwardPlane;
use crate::session::{run, Answer};
use crate::Result;

/// Plane coordinates are no refusal.
impl Answer for [f64; 2] {}

/// A thin wall, as two faces that face each other bound it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wall {
    /// The slab behind `face` (its outward normal points out of the wall), `thickness` deep.
    Flat {
        /// The wall's first face.
        face: OutwardPlane,
        /// The distance to the other face (positive).
        thickness: f64,
    },
    /// The shell between two radii about one axis.
    Round {
        /// A point on the axis.
        axis_point: [f64; 3],
        /// The axis direction (any nonzero length).
        axis: [f64; 3],
        /// The inner face's radius.
        inner: f64,
        /// The outer face's radius.
        outer: f64,
    },
}

/// A plane frame: an origin and two unit axes at right angles in the plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The frame's origin.
    pub origin: [f64; 3],
    /// The first axis (unit length).
    pub x: [f64; 3],
    /// The second axis (unit length, at right angles to `x`).
    pub y: [f64; 3],
}

fn vec(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn plane(p: &OutwardPlane) -> KernelPlane {
    KernelPlane {
        point: vec(p.point),
        outward: vec(p.outward),
    }
}

fn frame(f: &Frame) -> KernelFrame {
    KernelFrame {
        origin: vec(f.origin),
        x: vec(f.x),
        y: vec(f.y),
    }
}

/// Whether every one of `points` lies in `wall`, within `slack`. True for no points.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a wall, point or slack that is not finite,
/// a negative slack, a thickness or radius that is not positive, radii in the wrong order, a
/// normal or axis of zero length, or too many points.
pub fn wall_holds(wall: &Wall, points: &[[f64; 3]], slack: f64) -> Result<bool> {
    let wall = match *wall {
        Wall::Flat { face, thickness } => KernelWall::Flat {
            face: plane(&face),
            thickness,
        },
        Wall::Round {
            axis_point,
            axis,
            inner,
            outer,
        } => KernelWall::Round {
            axis_point: vec(axis_point),
            axis: vec(axis),
            inner,
            outer,
        },
    };
    let points: Vec<Vec3> = points.iter().copied().map(vec).collect();
    run("mid_sheet::wall_holds", |_| {
        kernel_wall_holds(&wall, &points, slack)
    })
}

/// The area of the planar polygon through `corners` in order; zero for fewer than three.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a corner that is not finite, or too many.
pub fn polygon_area(corners: &[[f64; 3]]) -> Result<f64> {
    let corners: Vec<Vec3> = corners.iter().copied().map(vec).collect();
    run("mid_sheet::polygon_area", |_| kernel_polygon_area(&corners))
}

/// `point` dropped onto `plane` along its normal.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a point or plane that is not finite, or a
/// normal of zero length.
pub fn onto_plane(on: &OutwardPlane, point: [f64; 3]) -> Result<[f64; 3]> {
    run("mid_sheet::onto_plane", |_| {
        Ok(array(kernel_onto_plane(&plane(on), vec(point))?))
    })
}

/// `point`'s coordinates in `f`.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a point or frame that is not finite.
pub fn in_frame(f: &Frame, point: [f64; 3]) -> Result<[f64; 2]> {
    run("mid_sheet::in_frame", |_| {
        kernel_in_frame(&frame(f), vec(point))
    })
}

/// The point at coordinates `uv` of `f`.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of coordinates or a frame that are not finite.
pub fn from_frame(f: &Frame, uv: [f64; 2]) -> Result<[f64; 3]> {
    run("mid_sheet::from_frame", |_| {
        Ok(array(kernel_from_frame(&frame(f), uv)?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plates_end_face_is_in_its_wall_through_ops() {
        // Top face z = 2 facing up, 2 thick: the end face's corners at z 0 and 2 are in.
        let wall = Wall::Flat {
            face: OutwardPlane {
                point: [0.0, 0.0, 2.0],
                outward: [0.0, 0.0, 1.0],
            },
            thickness: 2.0,
        };
        assert!(wall_holds(&wall, &[[40.0, 0.0, 0.0], [40.0, 30.0, 2.0]], 0.0).unwrap());
        assert!(!wall_holds(&wall, &[[40.0, 0.0, 3.0]], 1e-6).unwrap());
        assert!(wall_holds(&wall, &[[0.0, 0.0, 1.0]], f64::NAN).is_err());
        // A bore's rim between radii 8 and 10 about z.
        let tube = Wall::Round {
            axis_point: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            inner: 8.0,
            outer: 10.0,
        };
        assert!(wall_holds(&tube, &[[9.0, 0.0, 4.0], [0.0, -10.0, 0.0]], 0.0).unwrap());
    }

    #[test]
    fn area_projection_and_frames_through_ops() {
        // A 38 x 20 rectangle in y = 1: 760.
        let rect = [
            [2.0, 1.0, 0.0],
            [40.0, 1.0, 0.0],
            [40.0, 1.0, 20.0],
            [2.0, 1.0, 20.0],
        ];
        assert!((polygon_area(&rect).unwrap() - 760.0).abs() < 1e-9);
        // (5, 7, 9) dropped onto z = 1 is (5, 7, 1).
        let floor = OutwardPlane {
            point: [0.0, 0.0, 1.0],
            outward: [0.0, 0.0, -2.0],
        };
        assert_eq!(
            onto_plane(&floor, [5.0, 7.0, 9.0]).unwrap(),
            [5.0, 7.0, 1.0]
        );
        // A frame at (0, 0, 1) with axes y and -x: (5, 7, 1) reads (7, -5) and back.
        let f = Frame {
            origin: [0.0, 0.0, 1.0],
            x: [0.0, 1.0, 0.0],
            y: [-1.0, 0.0, 0.0],
        };
        assert_eq!(in_frame(&f, [5.0, 7.0, 1.0]).unwrap(), [7.0, -5.0]);
        assert_eq!(from_frame(&f, [7.0, -5.0]).unwrap(), [5.0, 7.0, 1.0]);
        assert!(polygon_area(&[[f64::NAN, 0.0, 0.0]]).is_err());
    }
}
