//! Drilled holes on a printed circuit board: the direction a slot points, and whether a hole
//! reaches a closed outline (the board's edge or a cutout's).
//!
//! Original plane geometry; no implementation is ported from another product. A slot is a
//! stadium: the points within its radius of its centre segment. It reaches an outline segment
//! when the distance between the two segments is at most that radius.

use crate::geom::vector2d::Vec2;
use crate::math::det;

/// The direction from `from` to `to`, in radians anticlockwise from +X, in `-π..=π`; `0` when
/// the points coincide or a coordinate is not finite.
#[must_use]
pub fn direction_angle(from: [f64; 2], to: [f64; 2]) -> f64 {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    if !(dx.is_finite() && dy.is_finite()) || (dx == 0.0 && dy == 0.0) {
        return 0.0;
    }
    det::atan2(dy, dx)
}

/// Whether a drilled hole touches or crosses any of the closed `loops`, each a list of corners
/// that closes back to its first. The hole is a stadium centred at `center`, `size[0]` wide along
/// `angle` (radians anticlockwise from +X) and `size[1]` across; equal sides make a round hole.
/// A hole with a non-finite or negative input reaches nothing.
///
/// An outline segment whose bounding box, grown by the hole's reach from its centre, misses the
/// centre is skipped before any distance is measured.
#[must_use]
pub fn hole_reaches_loops(
    center: [f64; 2],
    size: [f64; 2],
    angle: f64,
    loops: &[Vec<[f64; 2]>],
) -> bool {
    let [width, height] = size;
    let finite = center.iter().chain(&size).all(|v| v.is_finite()) && angle.is_finite();
    if !finite || width < 0.0 || height < 0.0 {
        return false;
    }
    let radius = width.min(height) * 0.5;
    let half = (width.max(height) - width.min(height)) * 0.5;
    let along = if width >= height {
        angle
    } else {
        angle + std::f64::consts::FRAC_PI_2
    };
    let c = Vec2::from_array(center);
    let axis = Vec2::new(det::cos(along), det::sin(along)) * half;
    let (p, q) = (c - axis, c + axis);
    let reach = half + radius;
    loops.iter().any(|corners| {
        // Each corner with the next one, the last with the first.
        let next = corners.iter().cycle().skip(1);
        corners.iter().zip(next).any(|(&a, &b)| {
            let (a, b) = (Vec2::from_array(a), Vec2::from_array(b));
            let outside = c.x < a.x.min(b.x) - reach
                || c.x > a.x.max(b.x) + reach
                || c.y < a.y.min(b.y) - reach
                || c.y > a.y.max(b.y) + reach;
            !outside && segment_distance(p, q, a, b) <= radius
        })
    })
}

/// Whether `point` lies on the wall of a drilled hole, the stadium of [`hole_reaches_loops`]: its
/// distance from the hole's centre segment is the hole's radius, within 0.6 % of that radius
/// (at least 1e-6). The slack takes in a slot's ends drawn as chords. A non-finite or negative
/// input is on no wall.
#[must_use]
pub fn on_hole_wall(center: [f64; 2], size: [f64; 2], angle: f64, point: [f64; 2]) -> bool {
    wall_gap(center, size, angle, point)
        .is_some_and(|(gap, radius)| gap <= (radius * 0.006).max(1e-6))
}

/// Whether a side of a board's outline lies on the wall of a drilled hole: every one of `points`
/// (its ends and its middle, say) is on the wall of [`on_hole_wall`], within `chord` or that
/// test's slack, whichever is larger. `chord` takes in a wall drawn as chords, whose middles fall
/// inside it by up to the chords' sag. No points, or a non-finite or negative input, is on no wall.
#[must_use]
pub fn side_on_hole_wall(
    center: [f64; 2],
    size: [f64; 2],
    angle: f64,
    points: &[[f64; 2]],
    chord: f64,
) -> bool {
    if !chord.is_finite() || chord < 0.0 {
        return false;
    }
    !points.is_empty()
        && points.iter().all(|&point| {
            wall_gap(center, size, angle, point)
                .is_some_and(|(gap, radius)| gap <= (radius * 0.006).max(chord).max(1e-6))
        })
}

/// How far `point` is from the wall of the hole of [`on_hole_wall`], with the hole's radius; `None`
/// for a non-finite or negative input.
fn wall_gap(center: [f64; 2], size: [f64; 2], angle: f64, point: [f64; 2]) -> Option<(f64, f64)> {
    let [width, height] = size;
    let finite = center
        .iter()
        .chain(&size)
        .chain(&point)
        .all(|v| v.is_finite())
        && angle.is_finite();
    if !finite || width < 0.0 || height < 0.0 {
        return None;
    }
    let radius = width.min(height) * 0.5;
    let half = (width.max(height) - width.min(height)) * 0.5;
    let along = if width >= height {
        angle
    } else {
        angle + std::f64::consts::FRAC_PI_2
    };
    let c = Vec2::from_array(center);
    let axis = Vec2::new(det::cos(along), det::sin(along)) * half;
    let distance = point_segment_distance(Vec2::from_array(point), c - axis, c + axis);
    Some(((distance - radius).abs(), radius))
}

fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Distance from `point` to the segment `a`..`b` (a point when they coincide).
fn point_segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let length = ab.length_squared();
    let t = if length > 0.0 {
        ((point - a).dot(ab) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(point)
}

/// Distance between the segments `p`..`q` and `a`..`b`: zero when they cross, else the nearest
/// end to the other segment.
fn segment_distance(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> f64 {
    let opposite = |s: f64, t: f64| (s > 0.0 && t < 0.0) || (s < 0.0 && t > 0.0);
    if opposite(cross(q - p, a - p), cross(q - p, b - p))
        && opposite(cross(b - a, p - a), cross(b - a, q - a))
    {
        return 0.0;
    }
    point_segment_distance(p, a, b)
        .min(point_segment_distance(q, a, b))
        .min(point_segment_distance(a, p, q))
        .min(point_segment_distance(b, p, q))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    /// A 40 × 40 board from (-20, 0) to (20, 40): its bottom edge lies on y = 0.
    fn board() -> Vec<Vec<[f64; 2]>> {
        vec![vec![[-20.0, 0.0], [20.0, 0.0], [20.0, 40.0], [-20.0, 40.0]]]
    }

    #[test]
    fn a_slot_reaches_the_edge_by_its_own_shape_not_its_widest_circle() {
        // A 4 × 0.5 slot centred 1 mm from the edge: lying along the edge it keeps 0.75 mm clear;
        // standing across it, its end runs 1 mm past the edge.
        assert!(!hole_reaches_loops([0.0, 1.0], [4.0, 0.5], 0.0, &board()));
        assert!(hole_reaches_loops(
            [0.0, 1.0],
            [4.0, 0.5],
            FRAC_PI_2,
            &board()
        ));
        // The same slot given as 0.5 wide and 4 across stands across the edge at angle 0.
        assert!(hole_reaches_loops([0.0, 1.0], [0.5, 4.0], 0.0, &board()));
    }

    #[test]
    fn a_round_hole_reaches_an_edge_it_touches() {
        assert!(hole_reaches_loops([0.0, 0.5], [1.0, 1.0], 0.0, &board()));
        assert!(!hole_reaches_loops([0.0, 0.51], [1.0, 1.0], 0.0, &board()));
        // Past a corner the nearest point is the corner itself.
        assert!(hole_reaches_loops([20.3, -0.3], [1.0, 1.0], 0.0, &board()));
        assert!(!hole_reaches_loops([20.4, -0.4], [1.0, 1.0], 0.0, &board()));
    }

    #[test]
    fn a_slot_whose_middle_crosses_an_edge_reaches_it() {
        // Both ends are 2 mm from the edge, on either side of it.
        assert!(hole_reaches_loops(
            [0.0, 0.0],
            [4.0, 0.1],
            FRAC_PI_2,
            &board()
        ));
    }

    #[test]
    fn a_hole_in_the_middle_of_the_board_reaches_nothing() {
        assert!(!hole_reaches_loops([0.0, 20.0], [3.2, 3.2], 0.0, &board()));
        assert!(!hole_reaches_loops([0.0, 20.0], [3.2, 3.2], 0.0, &[]));
    }

    #[test]
    fn hostile_input_reaches_nothing() {
        assert!(!hole_reaches_loops(
            [f64::NAN, 1.0],
            [1.0, 1.0],
            0.0,
            &board()
        ));
        assert!(!hole_reaches_loops(
            [0.0, 0.5],
            [f64::INFINITY, 1.0],
            0.0,
            &board()
        ));
        assert!(!hole_reaches_loops([0.0, 0.5], [-1.0, 1.0], 0.0, &board()));
        assert!(!hole_reaches_loops(
            [0.0, 0.5],
            [1.0, 1.0],
            f64::NAN,
            &board()
        ));
        let broken = vec![vec![[f64::NAN, 0.0], [20.0, 0.0]], vec![], vec![[0.0, 0.0]]];
        assert!(!hole_reaches_loops([0.0, 20.0], [1.0, 1.0], 0.0, &broken));
    }

    #[test]
    fn a_point_is_on_a_hole_wall_at_the_holes_radius() {
        // A round 1 mm hole: its wall is 0.5 from the centre, in any direction.
        assert!(on_hole_wall([2.0, 3.0], [1.0, 1.0], 0.0, [2.5, 3.0]));
        assert!(on_hole_wall([2.0, 3.0], [1.0, 1.0], 0.0, [2.0, 2.5]));
        assert!(!on_hole_wall([2.0, 3.0], [1.0, 1.0], 0.0, [2.0, 3.0]));
        assert!(!on_hole_wall([2.0, 3.0], [1.0, 1.0], 0.0, [2.6, 3.0]));
        // A 4 × 1 slot along +Y: its straight sides are 0.5 from the Y axis, its ends 2 from the centre.
        assert!(on_hole_wall([0.0, 0.0], [4.0, 1.0], FRAC_PI_2, [0.5, 1.0]));
        assert!(on_hole_wall([0.0, 0.0], [4.0, 1.0], FRAC_PI_2, [0.0, -2.0]));
        assert!(!on_hole_wall([0.0, 0.0], [4.0, 1.0], FRAC_PI_2, [2.0, 0.0]));
        // A chord's middle, just inside the round end, is within the slack.
        assert!(on_hole_wall(
            [0.0, 0.0],
            [4.0, 1.0],
            FRAC_PI_2,
            [0.0, 1.998]
        ));
        assert!(!on_hole_wall([f64::NAN, 0.0], [1.0, 1.0], 0.0, [0.5, 0.0]));
        assert!(!on_hole_wall(
            [0.0, 0.0],
            [1.0, 1.0],
            0.0,
            [f64::INFINITY, 0.0]
        ));
    }

    #[test]
    fn a_side_is_on_a_hole_wall_only_when_all_of_it_is() {
        // A 2 mm hole 1.001 from the edge y = 0: the edge's middle (0, 0) is 0.001 off its wall,
        // but the edge's ends are not on it.
        let edge = [[-20.0, 0.0], [0.0, 0.0], [20.0, 0.0]];
        assert!(!side_on_hole_wall(
            [0.0, -1.001],
            [2.0, 2.0],
            0.0,
            &edge,
            0.02
        ));
        // A 1 mm half-hole cut as chords of 0.02 mm sag: a 30° chord's middle is 0.017 inside.
        let (a, b) = (15_f64.to_radians(), 45_f64.to_radians());
        let (p, q) = (
            [0.5 * det::cos(a), 0.5 * det::sin(a)],
            [0.5 * det::cos(b), 0.5 * det::sin(b)],
        );
        let chord = [p, [(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5], q];
        assert!(side_on_hole_wall([0.0, 0.0], [1.0, 1.0], 0.0, &chord, 0.02));
        assert!(!side_on_hole_wall([0.0, 0.0], [1.0, 1.0], 0.0, &chord, 0.0));
        assert!(!side_on_hole_wall([0.0, 0.0], [1.0, 1.0], 0.0, &[], 0.02));
        assert!(!side_on_hole_wall(
            [0.0, 0.0],
            [1.0, 1.0],
            0.0,
            &chord,
            f64::NAN
        ));
    }

    #[test]
    fn direction_angle_is_anticlockwise_from_x() {
        assert_eq!(direction_angle([1.0, 1.0], [2.0, 1.0]), 0.0);
        assert!((direction_angle([0.0, 0.0], [0.0, 3.0]) - FRAC_PI_2).abs() < 1e-15);
        assert!((direction_angle([0.0, 0.0], [-1.0, 0.0]) - std::f64::consts::PI).abs() < 1e-15);
        assert_eq!(direction_angle([1.0, 1.0], [1.0, 1.0]), 0.0);
        assert_eq!(direction_angle([0.0, 0.0], [f64::NAN, 1.0]), 0.0);
    }
}
