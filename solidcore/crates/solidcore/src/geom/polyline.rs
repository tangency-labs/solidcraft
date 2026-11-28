//! Simplification of a sampled polyline in model space.

use crate::geom::geom_utility::p1::segment_point_distance;
use crate::Vec3;

/// Simplify a polyline (Douglas and Peucker), then merge retained neighbors closer than
/// `min_step` where the resulting segment stays within `tol` of the original points.
/// Each pass keeps both endpoints, the farthest point of a closed loop, and points whose
/// distance cannot be determined.
#[must_use]
pub fn simplify(points: &[Vec3], tol: f64, min_step: f64) -> Vec<Vec3> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let turn = loop_turn(points);
    let mut vertices = simplify_indices(points, tol, turn)
        .into_iter()
        .filter_map(|i| points.get(i).map(|&point| (i, point)))
        .peekable();
    let Some((mut previous_index, mut previous)) = vertices.next() else {
        return Vec::new();
    };
    let mut simplified = vec![previous];
    while let Some((index, point)) = vertices.next() {
        let can_merge = Some(index) != turn
            && min_step > 0.0
            && point.distance(previous) < min_step
            && vertices.peek().is_some_and(|&(next_index, next)| {
                points
                    .iter()
                    .take(next_index)
                    .skip(previous_index + 1)
                    .all(|&point| segment_point_distance(previous, next, point).0 <= tol)
            });
        if !can_merge {
            simplified.push(point);
            (previous_index, previous) = (index, point);
        }
    }
    simplified
}

fn loop_turn(points: &[Vec3]) -> Option<usize> {
    let start = points
        .first()
        .filter(|start| Some(*start) == points.last())?;
    points
        .iter()
        .enumerate()
        .take(points.len() - 1)
        .skip(1)
        .max_by(|(_, a), (_, b)| a.distance(*start).total_cmp(&b.distance(*start)))
        .map(|(index, _)| index)
}

fn simplify_indices(points: &[Vec3], tol: f64, turn: Option<usize>) -> Vec<usize> {
    let n = points.len();
    if n <= 2 || tol <= 0.0 || tol.is_nan() {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    // A closed loop's ends coincide. Start with the two spans to its farthest point.
    let mut stack = vec![(0, n - 1)];
    if let Some((index, flag)) =
        turn.and_then(|index| keep.get_mut(index).map(|flag| (index, flag)))
    {
        *flag = true;
        stack = vec![(0, index), (index, n - 1)];
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
    keep.into_iter()
        .enumerate()
        .filter_map(|(index, keep)| (keep || index == 0 || index == n - 1).then_some(index))
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
        let coarse = simplify(&fine, tol, 0.0);
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
            simplify(&line, 0.01, 0.0),
            vec![line[0], line[9]],
            "a straight run is its two ends"
        );
        let corner = vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0)];
        assert_eq!(
            simplify(&corner, 0.1, 0.0),
            corner,
            "a corner larger than the tolerance stays"
        );
        assert_eq!(
            simplify(&corner[..2], 5.0, 0.0),
            corner[..2],
            "a single segment stays"
        );
        assert_eq!(
            simplify(&corner, 0.0, 0.0),
            corner,
            "no tolerance, no change"
        );
        assert_eq!(
            simplify(&corner, f64::NAN, 0.0),
            corner,
            "a tolerance that is not a number changes nothing"
        );
    }

    #[test]
    fn a_closed_loop_stays_a_loop_and_points_that_are_not_numbers_are_kept() {
        let tiny = simplify(&circle(1.0, 64), 10.0, 0.0);
        assert!(
            tiny.len() >= 3 && tiny.first() == tiny.last(),
            "a loop smaller than the tolerance keeps its far point: {tiny:?}"
        );
        let odd = vec![v(0.0, 0.0, 0.0), v(f64::NAN, 0.0, 0.0), v(2.0, 0.0, 0.0)];
        assert_eq!(
            simplify(&odd, 1.0, 0.0).len(),
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

    #[test]
    fn close_corners_that_define_a_step_stay_within_the_requested_tolerance() {
        let step = [
            v(0.0, 0.0, 0.0),
            v(10.0, 0.0, 0.0),
            v(10.0, 0.09, 0.0),
            v(20.0, 0.09, 0.0),
        ];
        assert_eq!(simplify(&step, 0.001, 0.1), step);
    }

    #[test]
    fn merging_keeps_both_endpoints_and_a_closed_loops_turn() {
        let ends = [v(0.0, 0.0, 0.0), v(0.01, 0.0, 0.0)];
        assert_eq!(simplify(&ends, 1.0, 0.1), ends);
        let short_end = [ends[0], v(10.0, 0.0, 0.0), v(10.0, 0.01, 0.0)];
        assert_eq!(simplify(&short_end, 0.001, 0.1), short_end);
        let points = [ends[0], ends[1], ends[0]];
        assert_eq!(simplify(&points, 1.0, 0.1), points);
    }

    #[test]
    fn merging_checks_points_already_omitted_in_the_original_span() {
        let points = [
            v(0.0, 0.7, 0.0),
            v(1.0, -0.4, 0.0),
            v(2.0, -0.4, 0.0),
            v(3.0, -0.2, 0.0),
            v(4.0, 0.9, 0.0),
        ];
        // Points 1 and 2 are both retained. Merging them would put point 2 only
        // 0.398 from the new segment, but the already omitted point 3 is 0.612 away.
        let expected = [points[0], points[1], points[2], points[4]];
        assert_eq!(simplify(&points, 0.4, 0.0), expected);
        assert_eq!(simplify(&points, 0.4, 1.05), expected);
    }
}
