//! The joint between two mid-sheets for an application: the line two halfway planes meet in, and
//! a sheet's boundary edge moved onto it; the curve a halfway plane meets a halfway cylinder (or a
//! bore) in, an ellipse read in a sheet's frame as exact quarters, and a round sheet's end carried
//! along its axis ([`solidcore::surface::mid_joint`], at the kernel's angular and zone
//! tolerances). Ops converts the arrays; the kernel computes.

use solidcore::surface::face_pair::OutwardPlane as KernelPlane;
use solidcore::surface::mid_joint::{
    along_axis_to_plane as kernel_along_axis, ellipse_in_frame as kernel_ellipse_in_frame,
    move_edge_onto_line as kernel_move_edge, onto_cylinder as kernel_onto_cylinder,
    plane_meets_cylinder as kernel_plane_meets_cylinder, planes_meeting as kernel_planes_meeting,
    Ellipse3, Line3, PlaneCylinder as KernelPlaneCylinder,
};
use solidcore::surface::mid_sheet::Frame as KernelFrame;
use solidcore::Vec3;

use crate::face_pair::OutwardPlane;
use crate::mid_sheet::Frame;
use crate::session::{run, Answer};
use crate::Result;

/// An ellipse in space: its centre and two semi-axes at right angles (a circle when they are
/// equally long).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipse {
    /// The centre.
    pub centre: [f64; 3],
    /// The first semi-axis, the longer one.
    pub major: [f64; 3],
    /// The second semi-axis, at right angles to the first.
    pub minor: [f64; 3],
}

/// Where a plane meets a cylinder ([`plane_meets_cylinder`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PlaneCylinder {
    /// The plane runs along the axis: the lines it cuts the cylinder in (one where it touches).
    Lines(Vec<Line>),
    /// The plane crosses the axis: an ellipse, a circle of the radius when `square`.
    Ellipse {
        /// The curve.
        ellipse: Ellipse,
        /// Whether the plane is square to the axis.
        square: bool,
    },
}

impl Answer for PlaneCylinder {}

/// A rational quadratic arc in a plane frame: from `a` to `b`, control point `apex` weighted `w`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conic {
    /// Its start.
    pub a: [f64; 2],
    /// Its control point.
    pub apex: [f64; 2],
    /// Its end.
    pub b: [f64; 2],
    /// The control point's weight.
    pub w: f64,
}

impl Answer for [Conic; 4] {}

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

fn line(l: &Line3) -> Line {
    Line {
        point: array(l.point),
        direction: array(l.direction),
    }
}

/// Where `plane` meets the cylinder of `radius` about the axis through `axis_point` along `axis`
/// ([`solidcore::surface::mid_joint::plane_meets_cylinder`]): lines when the plane runs along the
/// axis, an ellipse (a circle when square) when it crosses it. `None` when it runs along the axis
/// clear of the cylinder.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a plane, axis or radius that is not finite,
/// a radius that is not positive, a normal or axis of zero length, or a crossing too shallow to
/// place.
pub fn plane_meets_cylinder(
    on: &OutwardPlane,
    axis_point: [f64; 3],
    axis: [f64; 3],
    radius: f64,
) -> Result<Option<PlaneCylinder>> {
    run("mid_joint::plane_meets_cylinder", |k| {
        let met = kernel_plane_meets_cylinder(k, &plane(on), vec(axis_point), vec(axis), radius)?;
        Ok(met.map(|m| match m {
            KernelPlaneCylinder::Lines(lines) => {
                PlaneCylinder::Lines(lines.iter().map(line).collect())
            }
            KernelPlaneCylinder::Ellipse { ellipse, square } => PlaneCylinder::Ellipse {
                ellipse: Ellipse {
                    centre: array(ellipse.centre),
                    major: array(ellipse.major),
                    minor: array(ellipse.minor),
                },
                square,
            },
        }))
    })
}

/// `ellipse` read in frame `f` as four exact quarters running counter-clockwise
/// ([`solidcore::surface::mid_joint::ellipse_in_frame`]).
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of an ellipse or frame that is not finite, or
/// an ellipse edge-on to the frame's plane.
pub fn ellipse_in_frame(f: &Frame, ellipse: &Ellipse) -> Result<[Conic; 4]> {
    let frame = KernelFrame {
        origin: vec(f.origin),
        x: vec(f.x),
        y: vec(f.y),
    };
    let ellipse = Ellipse3 {
        centre: vec(ellipse.centre),
        major: vec(ellipse.major),
        minor: vec(ellipse.minor),
    };
    run("mid_joint::ellipse_in_frame", |_| {
        Ok(kernel_ellipse_in_frame(&frame, &ellipse)?.map(|q| Conic {
            a: q.a,
            apex: q.apex,
            b: q.b,
            w: q.w,
        }))
    })
}

/// How far `point` lies from `plane` along `axis`, signed
/// ([`solidcore::surface::mid_joint::along_axis_to_plane`]); `None` when the axis runs along the
/// plane.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of input that is not finite, or a normal or
/// axis of zero length.
pub fn along_axis_to_plane(
    on: &OutwardPlane,
    point: [f64; 3],
    axis: [f64; 3],
) -> Result<Option<f64>> {
    run("mid_joint::along_axis_to_plane", |k| {
        kernel_along_axis(k, &plane(on), vec(point), vec(axis))
    })
}

/// `point` moved straight out from (or in toward) the axis onto the cylinder of `radius`
/// ([`solidcore::surface::mid_joint::onto_cylinder`]).
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of input that is not finite, a radius that is
/// not positive, an axis of zero length, or a point on the axis.
pub fn onto_cylinder(
    axis_point: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    point: [f64; 3],
) -> Result<[f64; 3]> {
    run("mid_joint::onto_cylinder", |_| {
        Ok(array(kernel_onto_cylinder(
            vec(axis_point),
            vec(axis),
            radius,
            vec(point),
        )?))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tubes_halfway_cylinder_meets_a_plates_halfway_plane_in_a_circle_through_ops() {
        let half = OutwardPlane {
            point: [0.0, 0.0, 2.0],
            outward: [0.0, 0.0, 1.0],
        };
        let met = plane_meets_cylinder(&half, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 9.0).unwrap();
        let Some(PlaneCylinder::Ellipse { ellipse, square }) = met else {
            panic!("{met:?}");
        };
        assert!(square);
        assert_eq!(ellipse.centre, [0.0, 0.0, 2.0]);
        let frame = Frame {
            origin: [0.0, 0.0, 2.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
        };
        let quarters = ellipse_in_frame(&frame, &ellipse).unwrap();
        for q in &quarters {
            assert!(((q.a[0] * q.a[0] + q.a[1] * q.a[1]).sqrt() - 9.0).abs() < 1e-12);
        }
        let d = along_axis_to_plane(&half, [10.0, 0.0, 4.0], [0.0, 0.0, 1.0]).unwrap();
        assert!(d.is_some_and(|d| (d + 2.0).abs() < 1e-12));
        let p = onto_cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 9.0, [10.0, 0.0, 4.0]).unwrap();
        assert!((p[0] - 9.0).abs() < 1e-12 && p[1].abs() < 1e-12 && (p[2] - 4.0).abs() < 1e-12);
        assert!(onto_cylinder([0.0; 3], [0.0, 0.0, 1.0], 9.0, [0.0, 0.0, 4.0]).is_err());
        let rib = OutwardPlane {
            point: [0.0, 0.0, 0.0],
            outward: [1.0, 0.0, 0.0],
        };
        let met = plane_meets_cylinder(&rib, [0.0; 3], [0.0, 0.0, 1.0], 9.0).unwrap();
        assert!(matches!(met, Some(PlaneCylinder::Lines(l)) if l.len() == 2));
    }

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
