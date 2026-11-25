//! Choosing among the places a line meets something: the nearest, a tie going ahead, and the
//! arithmetic on the line and the surface it meets that a caller would otherwise redo.

use crate::query::FaceLineHit;
use crate::{Point3, Vec3};

/// `direction` made unit at any finite length; `None` for a coordinate that is not finite or a
/// direction of no length.
#[must_use]
pub fn line_unit_direction(direction: Vec3) -> Option<Vec3> {
    direction.length_and_unit().map(|(_, unit)| unit)
}

/// Of the places a line meets a face, the one nearest the line's given point on either side.
/// Meetings as near as each other within `tie` are a tie, which goes to the one furthest along
/// the line's direction: ahead of the point rather than behind it. `None` for no meeting.
#[must_use]
pub fn nearest_line_hit(hits: &[FaceLineHit], tie: f64) -> Option<FaceLineHit> {
    let nearest = hits
        .iter()
        .map(|h| h.parameter.abs())
        .min_by(f64::total_cmp)?;
    hits.iter()
        .copied()
        .filter(|h| h.parameter.abs() <= nearest + tie)
        .max_by(|a, b| a.parameter.total_cmp(&b.parameter))
}

/// Of a line's nearest meetings `ahead` along its unit direction and `behind` it (each a distance,
/// not negative), the nearer as a signed distance along the direction (behind negative); the one
/// ahead on a tie.
#[must_use]
pub fn nearer_line_meeting(ahead: Option<f64>, behind: Option<f64>) -> Option<f64> {
    match (ahead, behind) {
        (Some(a), Some(b)) => Some(if b < a { -b } else { a }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(-b),
        (None, None) => None,
    }
}

/// The point `parameter` along the line from `through` along its unit `direction`.
#[must_use]
pub fn point_along_line(through: Point3, direction: Vec3, parameter: f64) -> Point3 {
    through + direction * parameter
}

/// How far `q` lies from `point` measured across a surface whose nearest point to `point` is
/// `foot`: along the direction `point` is off the surface when it is off it, else (with `point`
/// on the surface) by plain distance.
#[must_use]
pub fn distance_across_surface(point: Point3, foot: Point3, q: Point3) -> f64 {
    let along = q - point;
    match (point - foot).length_and_unit() {
        Some((length, n)) if length * length > 1e-24 => along.dot(n).abs(),
        _ => along.length(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point2;

    fn hit(parameter: f64) -> FaceLineHit {
        FaceLineHit {
            parameter,
            uv: Point2::new(0.0, 0.0),
            point: Point3::new(0.0, 0.0, parameter),
        }
    }

    #[test]
    fn the_nearest_hit_wins_and_a_tie_goes_ahead() {
        assert_eq!(nearest_line_hit(&[], 1e-9), None);
        let nearest = nearest_line_hit(&[hit(-3.0), hit(2.0), hit(9.0)], 1e-9);
        assert_eq!(nearest.map(|h| h.parameter), Some(2.0));
        let tied = nearest_line_hit(&[hit(-2.0), hit(2.0)], 1e-9);
        assert_eq!(tied.map(|h| h.parameter), Some(2.0));
    }

    #[test]
    fn a_meeting_ahead_or_behind_is_chosen_by_distance() {
        assert_eq!(nearer_line_meeting(Some(3.0), Some(2.0)), Some(-2.0));
        assert_eq!(nearer_line_meeting(Some(2.0), Some(2.0)), Some(2.0));
        assert_eq!(nearer_line_meeting(None, Some(4.0)), Some(-4.0));
        assert_eq!(nearer_line_meeting(None, None), None);
    }

    #[test]
    fn directions_points_and_distances_across_a_surface() {
        assert_eq!(
            line_unit_direction(Vec3::new(0.0, 0.0, 5.0)),
            Some(Vec3::new(0.0, 0.0, 1.0))
        );
        assert_eq!(line_unit_direction(Vec3::new(0.0, 0.0, 0.0)), None);
        assert_eq!(line_unit_direction(Vec3::new(f64::NAN, 0.0, 1.0)), None);
        let p = point_along_line(Point3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 0.0, 1.0), -2.0);
        assert_eq!(p, Point3::new(1.0, 2.0, 1.0));
        let across = distance_across_surface(
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 3.0),
        );
        assert!((across - 2.0).abs() < 1e-12, "{across}");
        let o = Point3::new(0.0, 0.0, 0.0);
        let plain = distance_across_surface(o, o, Point3::new(3.0, 4.0, 0.0));
        assert!((plain - 5.0).abs() < 1e-12, "{plain}");
    }
}
