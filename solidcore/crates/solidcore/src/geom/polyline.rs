//! Simplification of a sampled polyline in model space.

use crate::geom::geom_utility::p1::segment_point_distance;
use crate::Vec3;

/// `points` with the points dropped that lie within `tol` of the simplified line (Douglas and
/// Peucker): the ends are kept, and no dropped point is farther than `tol` from the result. A
/// closed loop (first point equals the last) keeps its point farthest from the start, so it stays a
/// loop. A point whose distance is not a number is kept. Consecutive points closer than
/// `min_step` are merged. Iterative: no recursion on long edges.
#[must_use]
pub fn simplify(points: &[Vec3], tol: f64, min_step: f64) -> Vec<Vec3> {
    let mut points = simplify_within(points, tol);
    if min_step > 0.0 {
        points.dedup_by(|a, b| a.distance(*b) < min_step);
    }
    points
}

fn simplify_within(points: &[Vec3], tol: f64) -> Vec<Vec3> {
    let n = points.len();
    if n <= 2 || tol <= 0.0 || tol.is_nan() {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    let mut stack = vec![(0, n - 1)];
    // A closed loop's ends are one point: split it at its point farthest from them first.
    if let Some(start) = points.first().filter(|start| Some(*start) == points.last()) {
        let far = points
            .iter()
            .enumerate()
            .take(n - 1)
            .skip(1)
            .max_by(|(_, a), (_, b)| a.distance(*start).total_cmp(&b.distance(*start)));
        if let Some((index, flag)) =
            far.and_then(|(index, _)| keep.get_mut(index).map(|flag| (index, flag)))
        {
            *flag = true;
            stack = vec![(0, index), (index, n - 1)];
        }
    }
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let Some((&pa, &pb)) = points.get(a).zip(points.get(b)) else {
            continue;
        };
        let mut far = (a + 1, -1.0);
        for (i, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let (d, _) = segment_point_distance(pa, pb, *p);
            // A distance that is not a number keeps its point.
            let d = if d.is_nan() { f64::INFINITY } else { d };
            if d > far.1 {
                far = (i, d);
            }
        }
        if let Some(flag) = keep.get_mut(far.0).filter(|_| far.1 > tol) {
            *flag = true;
            stack.push((a, far.0));
            stack.push((far.0, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .enumerate()
        .filter_map(|(index, (point, keep))| {
            (keep || index == 0 || index == n - 1).then_some(*point)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// A circle of `r` in `n` segments (closed: the last point is the first).
    fn circle(r: f64, n: usize) -> Vec<Vec3> {
        (0..=n)
            .map(|i| std::f64::consts::TAU * (i % n) as f64 / n as f64)
            .map(|a| v(r * a.cos(), r * a.sin(), 0.0))
            .collect()
    }

    #[test]
    fn a_fine_circle_keeps_only_the_points_its_tolerance_needs_and_no_point_moves_further() {
        let fine = circle(10.0, 720);
        let tol = 0.05;
        let coarse = simplify_within(&fine, tol);
        assert_eq!(
            (coarse.first(), coarse.last()),
            (fine.first(), fine.last()),
            "the ends stay"
        );
        // A chord of sagitta 0.05 on r 10 spans 2·acos(1 − 0.005) ≈ 11.5°: about 32 segments.
        assert!(
            (20..=64).contains(&(coarse.len() - 1)),
            "{} segments",
            coarse.len() - 1
        );
        let worst = fine
            .iter()
            .map(|p| {
                coarse
                    .windows(2)
                    .map(|w| segment_point_distance(w[0], w[1], *p).0)
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max);
        assert!(
            worst <= tol,
            "every dropped point lies within the tolerance: {worst}"
        );
    }

    #[test]
    fn straight_runs_and_short_edges_are_left_as_they_are_where_nothing_can_go() {
        let line: Vec<Vec3> = (0..10).map(|i| v(f64::from(i), 0.0, 0.0)).collect();
        assert_eq!(
            simplify_within(&line, 0.01),
            vec![line[0], line[9]],
            "a straight run is its two ends"
        );
        let corner = vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0)];
        assert_eq!(
            simplify_within(&corner, 0.1),
            corner,
            "a corner larger than the tolerance stays"
        );
        assert_eq!(
            simplify_within(&corner[..2], 5.0),
            corner[..2],
            "a single segment stays"
        );
        assert_eq!(
            simplify_within(&corner, 0.0),
            corner,
            "no tolerance, no change"
        );
        assert_eq!(
            simplify_within(&corner, f64::NAN),
            corner,
            "a tolerance that is not a number changes nothing"
        );
    }

    #[test]
    fn a_closed_loop_stays_a_loop_and_points_that_are_not_numbers_are_kept() {
        let tiny = simplify_within(&circle(1.0, 64), 10.0);
        assert!(
            tiny.len() >= 3 && tiny.first() == tiny.last(),
            "a loop smaller than the tolerance keeps its far point: {tiny:?}"
        );
        let odd = vec![v(0.0, 0.0, 0.0), v(f64::NAN, 0.0, 0.0), v(2.0, 0.0, 0.0)];
        assert_eq!(
            simplify_within(&odd, 1.0).len(),
            3,
            "a point at no known distance is kept"
        );
    }

    #[test]
    fn consecutive_coincident_points_are_merged_at_the_requested_distance() {
        let points = [v(0.0, 0.0, 0.0), v(1e-10, 0.0, 0.0), v(1.0, 0.0, 0.0)];
        assert_eq!(simplify(&points, 0.0, 1e-9), [points[0], points[2]]);
        assert_eq!(simplify(&points, 0.0, 0.0), points);
    }
}
