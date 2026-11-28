//! Whether a path seen in a sketch plane runs through the inside of a profile, not only along its
//! outline or through one of its corners: the wall of a body an extrusion runs along, seen along
//! the extrusion, meets the profile's footprint only so.
//!
//! Original plane geometry; no implementation is ported from another product.

use crate::geom::vector2d::Vec2;

/// Whether the polyline `path` runs through the inside of one of `regions`, not only along their
/// outline or through one of its corners. Each region is its outer loop and then its holes, each
/// loop a list of corners that closes back to its first; a point lies in a region when it lies in
/// the outer loop and in none of the holes (even-odd).
///
/// Each step of `path` is split where it meets the outline (where it crosses a loop, and level
/// with each loop corner). It runs inside where a piece's middle lies in a region farther than
/// `margin` from every loop, or where a run of pieces in a region, from one crossing of a loop to
/// the next, has its middle farther from every loop than a quarter of the run's length: a run
/// across a profile thinner than `margin` is as long as the profile is wide and its middle half
/// that from the outline, while a sliver between a round wall's samples and the outline's lies far
/// closer to the outline than that. A run goes on over the samples of `path` (and, where `path`
/// closes back to its first point within `margin`, over that seam); one that starts where `path`
/// starts or ends where it ends crosses nothing. A single point runs inside where it lies in a
/// region farther than `margin` from every loop.
#[must_use]
pub fn path_runs_inside(path: &[[f64; 2]], regions: &[Vec<Vec<[f64; 2]>>], margin: f64) -> bool {
    let edges: Vec<(Vec2, Vec2)> = regions
        .iter()
        .flatten()
        .flat_map(|l| loop_edges(l))
        .collect();
    let held = |q: Vec2| regions.iter().any(|r| in_region(r, q));
    let clear = |q: Vec2| {
        edges
            .iter()
            .map(|&(a, b)| to_segment(q, a, b))
            .fold(f64::INFINITY, f64::min)
    };
    let points: Vec<Vec2> = path.iter().map(|&p| Vec2::from_array(p)).collect();
    if let [q] = points[..] {
        return held(q) && clear(q) > margin;
    }
    // The pieces of `path` between where it meets the outline, in order, each with whether it
    // lies in a region.
    let mut pieces: Vec<(Vec2, Vec2, bool)> = Vec::new();
    for w in points.windows(2) {
        let &[p, q] = w else { continue };
        let r = q - p;
        let len2 = r.length_squared();
        let mut ts = vec![0.0, 1.0];
        for &(a, b) in &edges {
            let s = b - a;
            let den = r.x * s.y - r.y * s.x;
            let o = a - p;
            if den != 0.0 {
                ts.push((o.x * s.y - o.y * s.x) / den);
            }
            if len2 > 0.0 {
                ts.push(o.dot(r) / len2);
            }
        }
        ts.retain(|t| (0.0..=1.0).contains(t));
        ts.sort_by(f64::total_cmp);
        for t in ts.windows(2) {
            let &[t0, t1] = t else { continue };
            let (a, b) = (p + r * t0, p + r * t1);
            let m = (a + b) * 0.5;
            let inside = held(m);
            if inside && clear(m) > margin {
                return true;
            }
            pieces.push((a, b, inside));
        }
    }
    let closed = match (points.first(), points.last()) {
        (Some(&first), Some(&last)) => first.distance(last) <= margin,
        _ => false,
    };
    // A closed path is read from a piece outside every region, so no run is cut at its seam; an
    // open one from its start, so a run that starts there has no crossing before it.
    let from = if closed {
        match pieces.iter().position(|&(_, _, inside)| !inside) {
            Some(i) => i,
            None => return false,
        }
    } else {
        0
    };
    let mut run: Vec<(Vec2, Vec2)> = Vec::new();
    let mut crossed = closed;
    for &(a, b, inside) in pieces.iter().cycle().skip(from).take(pieces.len()) {
        if inside {
            run.push((a, b));
        } else {
            if crossed && run_crosses(&run, held, clear) {
                return true;
            }
            run.clear();
            crossed = true;
        }
    }
    // A closed path's last run ends at the piece it was read from, outside every region.
    closed && run_crosses(&run, held, clear)
}

/// Whether `run`, the pieces of a path in a region between two crossings of its outline, has its
/// middle in the region farther from the outline than a quarter of the run's length.
fn run_crosses(
    run: &[(Vec2, Vec2)],
    held: impl Fn(Vec2) -> bool,
    clear: impl Fn(Vec2) -> f64,
) -> bool {
    let length: f64 = run.iter().map(|(a, b)| a.distance(*b)).sum();
    let mut left = length / 2.0;
    for &(a, b) in run {
        let step = a.distance(b);
        if left <= step {
            let m = if step > 0.0 {
                a + (b - a) * (left / step)
            } else {
                a
            };
            return held(m) && clear(m) > (length / 4.0).max(1e-9);
        }
        left -= step;
    }
    false
}

/// Each corner of `corners` with the next one, the last with the first.
fn loop_edges(corners: &[[f64; 2]]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    let next = corners.iter().cycle().skip(1);
    corners
        .iter()
        .zip(next)
        .map(|(&a, &b)| (Vec2::from_array(a), Vec2::from_array(b)))
}

/// Whether `q` lies in `region`: in its outer loop (the first) and in none of its holes.
fn in_region(region: &[Vec<[f64; 2]>], q: Vec2) -> bool {
    match region.split_first() {
        Some((outer, holes)) => in_loop(outer, q) && !holes.iter().any(|h| in_loop(h, q)),
        None => false,
    }
}

/// Whether `q` lies in the closed loop `corners`, by the even-odd rule.
fn in_loop(corners: &[[f64; 2]], q: Vec2) -> bool {
    loop_edges(corners)
        .filter(|&(a, b)| {
            (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x)
        })
        .count()
        % 2
        == 1
}

/// How far `q` lies from the segment from `a` to `b`.
fn to_segment(q: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let len2 = d.length_squared();
    let t = if len2 > 0.0 {
        ((q - a).dot(d) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    q.distance(a + d * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 × 10 square from (0, 0) to (10, 10).
    fn square() -> Vec<Vec<[f64; 2]>> {
        vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]]
    }

    /// A profile far thinner than the margin: x within 1/256 of 0, y within 1.
    fn thin() -> Vec<Vec<[f64; 2]>> {
        let w = 1.0 / 256.0;
        vec![vec![[-w, -1.0], [w, -1.0], [w, 1.0], [-w, 1.0]]]
    }

    const MARGIN: f64 = 1.0 / 64.0;

    #[test]
    fn a_path_through_the_middle_runs_inside() {
        assert!(path_runs_inside(
            &[[-5.0, 5.0], [15.0, 5.0]],
            &[square()],
            MARGIN
        ));
        assert!(path_runs_inside(&[[5.0, 5.0]], &[square()], MARGIN));
    }

    #[test]
    fn a_path_along_the_outline_or_through_a_corner_does_not() {
        assert!(!path_runs_inside(
            &[[0.0, -5.0], [0.0, 15.0]],
            &[square()],
            MARGIN
        ));
        assert!(!path_runs_inside(
            &[[-1.0, 1.0], [1.0, -1.0]],
            &[square()],
            MARGIN
        ));
        assert!(!path_runs_inside(&[[0.0, 5.0]], &[square()], MARGIN));
    }

    #[test]
    fn a_path_just_inside_the_outline_all_round_does_not() {
        // A round wall flush with a round profile: its samples lie inside the profile's chords.
        let e = 1e-6;
        let ring = [
            [e, e],
            [10.0 - e, e],
            [10.0 - e, 10.0 - e],
            [e, 10.0 - e],
            [e, e],
        ];
        assert!(!path_runs_inside(&ring, &[square()], MARGIN));
    }

    #[test]
    fn a_hole_is_not_inside() {
        let mut framed = square();
        framed.push(vec![[2.0, 2.0], [8.0, 2.0], [8.0, 8.0], [2.0, 8.0]]);
        assert!(!path_runs_inside(&[[5.0, 5.0]], &[framed.clone()], MARGIN));
        assert!(path_runs_inside(&[[1.0, 5.0]], &[framed], 0.5));
    }

    #[test]
    fn a_path_across_a_profile_thinner_than_the_margin_runs_inside() {
        assert!(path_runs_inside(
            &[[-1.0, 0.0], [1.0, 0.0]],
            &[thin()],
            MARGIN
        ));
    }

    #[test]
    fn a_crossing_split_by_a_sample_inside_a_thin_profile_still_runs_inside() {
        assert!(path_runs_inside(
            &[[-1.0, 0.0], [0.0, 0.0], [1.0, 0.0]],
            &[thin()],
            MARGIN
        ));
    }

    #[test]
    fn a_crossing_split_by_the_seam_of_a_closed_path_still_runs_inside() {
        // The only crossing of the thin profile is the bottom side, at the seam (0, 0).
        let closed = [
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 5.0],
            [-1.0, 5.0],
            [-1.0, 0.0],
            [0.0, 0.0],
        ];
        assert!(path_runs_inside(&closed, &[thin()], MARGIN));
    }

    #[test]
    fn a_path_that_starts_or_ends_inside_a_thin_profile_does_not_cross_it() {
        assert!(!path_runs_inside(
            &[[0.0, 0.0], [1.0, 0.0]],
            &[thin()],
            MARGIN
        ));
        assert!(!path_runs_inside(
            &[[-1.0, 0.0], [0.0, 0.0]],
            &[thin()],
            MARGIN
        ));
    }
}
