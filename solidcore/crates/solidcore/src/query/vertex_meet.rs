//! Whether two tolerant points are one point: each is a ball, its point with its tolerance as
//! the radius (a tolerant vertex), and the two are one when the balls touch. A file may write
//! one corner as two vertices at one point; a reader joins them by this test.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust              | Upstream |
//! |-------------------|----------|
//! | [`points_meet`]   | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::math::Point3;

/// Whether the tolerant point `a` (tolerance `a_tol`) and `b` (tolerance `b_tol`) are one: their
/// distance is no more than the sum of their tolerances. When they are, the tolerance `a` takes
/// as the one point: the radius of the least ball about `a` that holds `b`'s ball,
/// `max(a_tol, b_tol + distance)`. `None` when they are apart, or when a point or tolerance is
/// not finite or a tolerance is negative.
#[must_use]
pub fn points_meet(a: Point3, a_tol: f64, b: Point3, b_tol: f64) -> Option<f64> {
    let finite = |p: Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    let tolerance = |t: f64| t.is_finite() && t >= 0.0;
    if !(finite(a) && finite(b) && tolerance(a_tol) && tolerance(b_tol)) {
        return None;
    }
    let gap = a.distance(b);
    if gap <= a_tol + b_tol {
        Some(a_tol.max(b_tol + gap))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::points_meet;
    use crate::math::Point3;

    const O: Point3 = Point3::new(0.0, 0.0, 0.0);

    #[test]
    fn points_meet_up_to_the_sum_of_their_tolerances_and_not_past_it() {
        // 3-4-5: the distance is exactly 5.
        let b = Point3::new(3.0, 4.0, 0.0);
        assert_eq!(points_meet(O, 2.0, b, 3.0), Some(8.0));
        assert_eq!(points_meet(O, 6.0, b, 0.0), Some(6.0));
        assert_eq!(points_meet(O, 5.0, b, 0.0), Some(5.0));
        // The next tolerance below 5 is short of the distance.
        assert_eq!(
            points_meet(O, f64::from_bits(5.0f64.to_bits() - 1), b, 0.0),
            None
        );
        assert_eq!(
            points_meet(O, 0.0, b, f64::from_bits(5.0f64.to_bits() - 1)),
            None
        );
        // One point: the larger tolerance.
        assert_eq!(points_meet(b, 0.0, b, 0.0), Some(0.0));
        assert_eq!(points_meet(b, 1e-7, b, 2e-7), Some(2e-7));
        // 1.5e-7 apart at 1e-7 each: one point, its ball reaching past the other's.
        let near = Point3::new(1.5e-7, 0.0, 0.0);
        let t = points_meet(O, 1e-7, near, 1e-7).unwrap_or(f64::NAN);
        assert!((t - 2.5e-7).abs() < 1e-20, "{t}");
        assert_eq!(
            points_meet(O, 1e-7, Point3::new(2.1e-7, 0.0, 0.0), 1e-7),
            None
        );
    }

    #[test]
    fn a_point_or_tolerance_that_is_not_a_number_never_meets() {
        assert_eq!(points_meet(O, f64::NAN, O, 1.0), None);
        assert_eq!(points_meet(O, 1.0, O, f64::INFINITY), None);
        assert_eq!(points_meet(O, -1.0, O, 2.0), None);
        assert_eq!(
            points_meet(Point3::new(f64::NAN, 0.0, 0.0), 1.0, O, 1.0),
            None
        );
        assert_eq!(
            points_meet(Point3::new(f64::INFINITY, 0.0, 0.0), 1.0, O, 1.0),
            None
        );
    }
}
