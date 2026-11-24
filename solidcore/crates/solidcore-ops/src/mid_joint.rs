//! The joint between two mid-sheets for an application: the line two halfway planes meet in, and
//! a sheet's boundary edge moved onto it ([`solidcore::surface::mid_joint`], at the kernel's
//! angular and zone tolerances). Ops converts the arrays; the kernel computes.

use solidcore::surface::face_pair::OutwardPlane as KernelPlane;
use solidcore::surface::mid_joint::{
    move_edge_onto_line as kernel_move_edge, planes_meeting as kernel_planes_meeting, Line3,
};
use solidcore::Vec3;

use crate::face_pair::OutwardPlane;
use crate::session::run;
use crate::Result;

/// A line: a point on it and its unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// A point on the line.
    pub point: [f64; 3],
    /// The line's unit direction.
    pub direction: [f64; 3],
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

/// The line two planes meet in, or `None` when they are parallel within the kernel's angular
/// tolerance, whichever way they face.
///
/// # Errors
/// The kernel's refusal of a point or normal that is not finite, or a normal of zero length
/// ([`crate::Error::Status`]).
pub fn planes_meeting(a: &OutwardPlane, b: &OutwardPlane) -> Result<Option<Line>> {
    run("mid_joint::planes_meeting", |k| {
        Ok(
            kernel_planes_meeting(k, &plane(a), &plane(b))?.map(|l| Line {
                point: array(l.point),
                direction: array(l.direction),
            }),
        )
    })
}

/// Edge `edge` of the planar polygon `polygon` (from its corner `edge` to the next) moved onto
/// `line`, with the edges beside it met again there: the sheet grows where the line lies outside
/// the polygon and shrinks where it lies inside. `None` when the edge is not parallel to the line
/// within the angular tolerance, or a neighbouring edge is.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of fewer than three or too many corners, a
/// corner or line that is not finite, an edge past the corners, two corners at one place, or a
/// line outside the polygon's plane.
pub fn move_edge_onto_line(
    polygon: &[[f64; 3]],
    edge: usize,
    line: &Line,
) -> Result<Option<Vec<[f64; 3]>>> {
    let corners: Vec<Vec3> = polygon.iter().copied().map(vec).collect();
    let line = Line3 {
        point: vec(line.point),
        direction: vec(line.direction),
    };
    run("mid_joint::move_edge_onto_line", |k| {
        Ok(kernel_move_edge(k, &corners, edge, &line)?
            .map(|moved| moved.into_iter().map(array).collect()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_l_brackets_leg_is_extended_to_the_joint_line_through_ops() {
        let joint = planes_meeting(
            &OutwardPlane {
                point: [0.0, 1.0, 0.0],
                outward: [0.0, 1.0, 0.0],
            },
            &OutwardPlane {
                point: [1.0, 0.0, 0.0],
                outward: [1.0, 0.0, 0.0],
            },
        )
        .unwrap()
        .unwrap();
        let leg = [
            [2.0, 1.0, 0.0],
            [40.0, 1.0, 0.0],
            [40.0, 1.0, 20.0],
            [2.0, 1.0, 20.0],
        ];
        let moved = move_edge_onto_line(&leg, 3, &joint).unwrap().unwrap();
        let near = |p: &[f64; 3], x: f64, z: f64| {
            (p[0] - x).abs() < 1e-12 && (p[1] - 1.0).abs() < 1e-12 && (p[2] - z).abs() < 1e-12
        };
        assert!(moved.iter().any(|p| near(p, 1.0, 0.0)), "{moved:?}");
        assert!(moved.iter().any(|p| near(p, 1.0, 20.0)), "{moved:?}");
        assert_eq!(moved.len(), 4);
    }

    #[test]
    fn parallel_planes_have_no_joint_and_nonsense_is_refused() {
        let floor = OutwardPlane {
            point: [0.0, 0.0, 0.0],
            outward: [0.0, 0.0, 1.0],
        };
        let roof = OutwardPlane {
            point: [0.0, 0.0, 3.0],
            outward: [0.0, 0.0, -1.0],
        };
        assert_eq!(planes_meeting(&floor, &roof).unwrap(), None);
        let line = Line {
            point: [f64::NAN, 0.0, 0.0],
            direction: [0.0, 1.0, 0.0],
        };
        let sheet = [
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 2.0, 0.0],
            [0.0, 2.0, 0.0],
        ];
        assert!(move_edge_onto_line(&sheet, 0, &line).is_err());
    }
}
