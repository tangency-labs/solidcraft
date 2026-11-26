//! The 2D arithmetic of a sketch's arc snaps, on plain `[f64; 2]` points and directions: the
//! circle through three points, the arc a typed radius or sweep gives, the tangent arc, the
//! perpendicular bisector's centre and the direction a curve leaves a point in. The numbers are
//! the kernel's (`solidcore::math`, `*_2d`); ops passes them on, so an application computes none.

use solidcore::math;

/// The unit direction from `from` to `to`; `None` where they coincide or are not finite.
#[must_use]
pub fn unit_between(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    math::unit_between_2d(from, to)
}

/// The unit direction from `from` to `to` turned a quarter turn counter-clockwise; `None`
/// where they coincide or are not finite.
#[must_use]
pub fn left_normal_between(from: [f64; 2], to: [f64; 2]) -> Option<[f64; 2]> {
    math::left_normal_between_2d(from, to)
}

/// The centre and radius of the circle through `from` and `other` that touches the unit
/// direction `dir` at `from`; `None` where the points coincide or `dir` runs along their chord.
#[must_use]
pub fn tangent_arc_centre(
    from: [f64; 2],
    other: [f64; 2],
    dir: [f64; 2],
) -> Option<([f64; 2], f64)> {
    math::tangent_arc_centre_2d(from, other, dir)
}

/// The point of the perpendicular bisector of the chord `start`-`end` nearest `p`: the centre of
/// the arc through both ends that has it; `None` where the ends coincide.
#[must_use]
pub fn bisector_centre(start: [f64; 2], end: [f64; 2], p: [f64; 2]) -> Option<[f64; 2]> {
    math::bisector_centre_2d(start, end, p)
}

/// Whether the unit direction `dir` at `p` is square to the radius from `centre`, within a
/// millionth; `false` where `p` is on `centre`.
#[must_use]
pub fn is_square_to_radius(centre: [f64; 2], p: [f64; 2], dir: [f64; 2]) -> bool {
    math::is_square_to_radius_2d(centre, p, dir)
}

/// The centre of the circle through `a`, `b` and `c`; `None` for points on one line.
#[must_use]
pub fn circumcentre(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    math::circumcentre_2d(a, b, c)
}

/// The circle through `s`, `e` and `t`: its centre, radius and the sweep (radians) of the arc
/// from `s` to `e` that passes through `t`; `None` for points on one line.
#[must_use]
pub fn three_point_circle(s: [f64; 2], e: [f64; 2], t: [f64; 2]) -> Option<([f64; 2], f64, f64)> {
    math::three_point_circle_2d(s, e, t)
}

/// The third point of the arc from `s` to `e` for the cursor `h` with a radius or a sweep
/// (radians; the sweep wins when both are given): on the arc of that size whose middle is nearest
/// the cursor. `None`: no such arc.
#[must_use]
pub fn arc_third_point(
    s: [f64; 2],
    e: [f64; 2],
    h: [f64; 2],
    radius: Option<f64>,
    sweep: Option<f64>,
) -> Option<[f64; 2]> {
    math::arc_third_point_2d(s, e, h, radius, sweep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn directions_are_unit_and_refuse_coincident_points() {
        assert_eq!(unit_between([1.0, 1.0], [4.0, 1.0]), Some([1.0, 0.0]));
        assert_eq!(
            left_normal_between([0.0, 0.0], [0.0, 2.0]),
            Some([-1.0, 0.0])
        );
        assert_eq!(unit_between([1.0, 1.0], [1.0, 1.0]), None);
        assert_eq!(left_normal_between([0.0, 0.0], [f64::NAN, 1.0]), None);
    }

    #[test]
    fn the_circle_through_three_points_has_their_centre_and_the_sweep_through_the_third() {
        assert_eq!(
            circumcentre([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]),
            Some([0.0, 0.0])
        );
        assert_eq!(circumcentre([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]), None);
        let side = std::f64::consts::FRAC_1_SQRT_2;
        // Quarter of the unit circle from (1, 0) to (0, 1): the third point on the far side
        // makes the long arc, on the near side the quarter.
        let (centre, r, sweep) = three_point_circle([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]).unwrap();
        assert!(near(centre, [0.0, 0.0]) && (r - 1.0).abs() < 1e-9);
        assert!((sweep - 1.5 * core::f64::consts::PI).abs() < 1e-9);
        let (_, _, near_sweep) = three_point_circle([1.0, 0.0], [0.0, 1.0], [side, side]).unwrap();
        assert!((near_sweep - 0.5 * core::f64::consts::PI).abs() < 1e-9);
        assert_eq!(three_point_circle([0.0, 0.0], [1.0, 0.0], [2.0, 0.0]), None);
    }

    #[test]
    fn a_radius_or_a_sweep_gives_the_third_point_nearest_the_cursor() {
        let (s, e) = ([1.0, 0.0], [-1.0, 0.0]);
        // A half circle: its middle is on the cursor's side of the chord.
        let by_sweep =
            arc_third_point(s, e, [0.0, 5.0], None, Some(core::f64::consts::PI)).unwrap();
        assert!(near(by_sweep, [0.0, 1.0]));
        let below = arc_third_point(s, e, [0.0, -5.0], None, Some(core::f64::consts::PI)).unwrap();
        assert!(near(below, [0.0, -1.0]));
        let by_radius = arc_third_point(s, e, [0.0, 5.0], Some(1.0), None).unwrap();
        assert!(near(by_radius, [0.0, 1.0]));
        // The sweep wins when both are typed.
        let both =
            arc_third_point(s, e, [0.0, 5.0], Some(9.0), Some(core::f64::consts::PI)).unwrap();
        assert!(near(both, [0.0, 1.0]));
        assert_eq!(arc_third_point(s, e, [0.0, 5.0], Some(0.5), None), None);
        assert_eq!(arc_third_point(s, e, [0.0, 5.0], None, Some(0.0)), None);
        assert_eq!(arc_third_point(s, e, [0.0, 5.0], None, None), None);
        assert_eq!(arc_third_point(s, s, [0.0, 5.0], Some(1.0), None), None);
    }

    #[test]
    fn the_tangent_arc_touches_the_direction_at_its_start() {
        // The circle through (0, 0) and (0, 2) that touches the x axis at the origin.
        let (centre, r) = tangent_arc_centre([0.0, 0.0], [0.0, 2.0], [1.0, 0.0]).unwrap();
        assert!(near(centre, [0.0, 1.0]) && (r - 1.0).abs() < 1e-9);
        // Along the chord there is none, nor for coincident points.
        assert_eq!(tangent_arc_centre([0.0, 0.0], [5.0, 0.0], [1.0, 0.0]), None);
        assert_eq!(tangent_arc_centre([0.0, 0.0], [0.0, 0.0], [1.0, 0.0]), None);
    }

    #[test]
    fn the_bisector_centre_is_the_nearest_point_of_the_chord_s_bisector() {
        assert_eq!(
            bisector_centre([-1.0, 0.0], [1.0, 0.0], [3.0, 4.0]),
            Some([0.0, 4.0])
        );
        assert_eq!(bisector_centre([1.0, 1.0], [1.0, 1.0], [3.0, 4.0]), None);
    }

    #[test]
    fn a_direction_is_square_to_a_radius_when_across_it() {
        assert!(is_square_to_radius([0.0, 0.0], [1.0, 0.0], [0.0, 1.0]));
        assert!(!is_square_to_radius([0.0, 0.0], [1.0, 0.0], [1.0, 0.0]));
        assert!(!is_square_to_radius([1.0, 0.0], [1.0, 0.0], [0.0, 1.0]));
    }
}
