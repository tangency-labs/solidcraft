//! The plane a sweep path lies in, within an angle: the plane a pipe or a pattern along a path
//! needs before it turns the profile or the copies along it (#16084).
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust                  | Upstream |
//! |-----------------------|----------|
//! | [`path_plane_normal`] | (none)   |
//! | [`PathPlanePiece`]    | (none)   |
//! | [`PieceFit`]          | (none)   |
//!
//! The candidate normal is the axis of the path's first arc, or else the turn between its first
//! two directions of travel that are not parallel; a straight path lies in many planes and gets
//! one of them. Every piece is then checked against it over its whole length: a line by its
//! directions of travel, an arc by its axis, and a free-form span by its control points. A
//! rational Bézier span with positive weights lies in the convex hull of its control points, and
//! a span in a plane has all of them in that plane, so the control points decide the whole span,
//! not sampled points of it.
//!
//! Deviations: no upstream counterpart. A vector of length at most [`MIN_LENGTH`] has no
//! direction, as the application's own normalisation reads it.

use crate::math::Vec3;

/// The length at or below which a vector has no direction.
pub const MIN_LENGTH: f64 = 1e-9;

/// How one piece of the path is checked against the candidate plane.
#[derive(Clone, Debug, PartialEq)]
pub enum PieceFit {
    /// A line: each of its directions of travel (its start and end tangents) lies in the plane.
    /// A line with no direction fits no plane.
    Directions(Vec<[f64; 3]>),
    /// An arc: its axis (any length, either sense) is along the plane's normal.
    Axis([f64; 3]),
    /// A free-form rational Bézier span with positive weights: its start, and its control
    /// points in space (`None` when one of them has no Euclidean point, which fits no plane).
    /// Every control point lies in the plane through the start, within the angle as seen from
    /// the start. A span whose [`PathPlanePiece::directions`] is empty fits no plane.
    ControlPoints {
        /// Where the span starts.
        start: [f64; 3],
        /// The span's Euclidean control points.
        points: Option<Vec<[f64; 3]>>,
    },
}

/// One piece of a path, as [`path_plane_normal`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct PathPlanePiece {
    /// The piece's unit directions of travel that the candidate normal's turn is read from, in
    /// order along the path (a line's or an arc's two end tangents, points along a free-form
    /// span).
    pub directions: Vec<[f64; 3]>,
    /// The axis of an arc, which gives the candidate normal when it is the path's first arc;
    /// `None` for any other piece.
    pub arc_axis: Option<[f64; 3]>,
    /// How the piece is checked against the candidate plane.
    pub fit: PieceFit,
}

/// `v` made unit; `None` for a vector of length at most [`MIN_LENGTH`] or not finite.
fn unit(v: Vec3) -> Option<Vec3> {
    let l = v.length();
    (l > MIN_LENGTH && l.is_finite()).then(|| v * (1.0 / l))
}

/// The unit normal of a plane the whole path lies in, within `tol` radians (see the module
/// documentation). `None` for a path that leaves every plane, has no direction, or has no
/// pieces.
#[must_use]
pub fn path_plane_normal(pieces: &[PathPlanePiece], tol: f64) -> Option<[f64; 3]> {
    let within = tol.sin();
    let dirs: Vec<Vec3> = pieces
        .iter()
        .flat_map(|p| p.directions.iter().map(|d| Vec3::from(*d)))
        .collect();
    let first = *dirs.first()?;
    let arc = pieces
        .iter()
        .find_map(|p| p.arc_axis.and_then(|a| unit(Vec3::from(a))));
    let turn = || {
        dirs.iter().find_map(|d| {
            let c = first.cross(*d);
            unit(c).filter(|_| c.length() > within)
        })
    };
    // A straight path: any plane through its line.
    let straight = || {
        let other = if first.x.abs() < 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        unit(first.cross(other))
    };
    let n = arc.or_else(turn).or_else(straight)?;
    let fits = |p: &PathPlanePiece| match &p.fit {
        PieceFit::Axis(a) => unit(Vec3::from(*a)).is_some_and(|k| k.cross(n).length() <= within),
        PieceFit::Directions(d) => {
            !d.is_empty() && d.iter().all(|d| Vec3::from(*d).dot(n).abs() <= within)
        }
        PieceFit::ControlPoints { start, points } => {
            let start = Vec3::from(*start);
            !p.directions.is_empty()
                && points.as_ref().is_some_and(|q| {
                    q.iter().all(|q| {
                        let v = Vec3::from(*q) - start;
                        v.dot(n).abs() <= within * v.length()
                    })
                })
        }
    };
    pieces.iter().all(fits).then(|| n.to_array())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-6;

    fn line(a: [f64; 3], b: [f64; 3]) -> PathPlanePiece {
        let d = unit(Vec3::from(b) - Vec3::from(a)).map(Vec3::to_array);
        let dirs: Vec<[f64; 3]> = d.into_iter().flat_map(|d| [d, d]).collect();
        PathPlanePiece {
            directions: dirs.clone(),
            arc_axis: None,
            fit: PieceFit::Directions(dirs),
        }
    }

    fn span(points: &[[f64; 3]]) -> PathPlanePiece {
        let first = points.first().copied().unwrap_or_default();
        let last = points.last().copied().unwrap_or_default();
        let d = unit(Vec3::from(last) - Vec3::from(first)).map(Vec3::to_array);
        PathPlanePiece {
            directions: d.into_iter().collect(),
            arc_axis: None,
            fit: PieceFit::ControlPoints {
                start: first,
                points: Some(points.to_vec()),
            },
        }
    }

    #[test]
    fn an_l_of_two_lines_lies_in_their_plane() {
        let n = path_plane_normal(
            &[
                line([0.0, 0.0, 5.0], [10.0, 0.0, 5.0]),
                line([10.0, 0.0, 5.0], [10.0, 8.0, 5.0]),
            ],
            TOL,
        );
        let n = n.expect("the L is planar");
        assert!(n[2].abs() > 1.0 - 1e-12, "{n:?}");
    }

    #[test]
    fn a_line_leaving_the_plane_of_the_first_turn_is_refused() {
        let path = [
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            line([10.0, 0.0, 0.0], [10.0, 8.0, 0.0]),
            line([10.0, 8.0, 0.0], [10.0, 8.0, 6.0]),
        ];
        assert_eq!(path_plane_normal(&path, TOL), None);
    }

    #[test]
    fn the_first_arcs_axis_is_the_normal() {
        let arc = PathPlanePiece {
            directions: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            arc_axis: Some([0.0, 0.0, -3.0]),
            fit: PieceFit::Axis([0.0, 0.0, -3.0]),
        };
        let n = path_plane_normal(&[arc], TOL).expect("an arc is planar");
        assert!((n[2] + 1.0).abs() < 1e-12, "{n:?}");
    }

    #[test]
    fn a_straight_path_gets_a_plane_through_its_line() {
        let n = path_plane_normal(&[line([0.0, 0.0, 0.0], [0.0, 0.0, 10.0])], TOL)
            .expect("a line lies in a plane");
        assert!(n[2].abs() < 1e-12, "{n:?}");
    }

    #[test]
    fn a_span_bulging_off_the_plane_between_its_samples_is_refused_by_its_control_points() {
        // The span's ends and its chord lie in z = 0, a control point does not.
        let path = [
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            span(&[[10.0, 0.0, 0.0], [12.0, 4.0, 3.0], [10.0, 8.0, 0.0]]),
        ];
        assert_eq!(path_plane_normal(&path, TOL), None);
        let flat = [
            line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]),
            span(&[[10.0, 0.0, 0.0], [12.0, 4.0, 0.0], [10.0, 8.0, 0.0]]),
        ];
        assert!(path_plane_normal(&flat, TOL).is_some());
    }

    #[test]
    fn degenerate_and_hostile_pieces_give_none() {
        assert_eq!(path_plane_normal(&[], TOL), None);
        assert_eq!(
            path_plane_normal(&[line([1.0, 1.0, 1.0], [1.0, 1.0, 1.0])], TOL),
            None
        );
        let unprojected = PathPlanePiece {
            directions: vec![[1.0, 0.0, 0.0]],
            arc_axis: None,
            fit: PieceFit::ControlPoints {
                start: [0.0; 3],
                points: None,
            },
        };
        assert_eq!(path_plane_normal(&[unprojected], TOL), None);
        let nan = PathPlanePiece {
            directions: vec![[f64::NAN; 3]],
            arc_axis: Some([f64::NAN; 3]),
            fit: PieceFit::Axis([f64::NAN; 3]),
        };
        assert_eq!(path_plane_normal(&[nan], TOL), None);
    }
}
