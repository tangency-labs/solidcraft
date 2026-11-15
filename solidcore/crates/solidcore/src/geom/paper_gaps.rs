//! Original bounded paper-stroke cutting; no implementation is ported from another product.
//! Intersecting half-plane parameter bounds follows Liang and Barsky, ACM TOG 3 (1984),
//! DOI 10.1145/357332.357333, and Cyrus and Beck, Computers & Graphics 3 (1978),
//! DOI 10.1016/0097-8493(78)90021-3. The union of excluded intervals is complemented.
//! Coordinates are finite within ±100000; margins are finite within 0..=5000 paper units.

use crate::error::{Result, SolidError};
use crate::geom::vector2d::Vec2;

const LIMIT: f64 = 100_000.0;
const MAX_MARGIN: f64 = 5_000.0;
const EPS: f64 = 1e-9;
// Cross products carry two lengths; use a dimensionless angular tolerance.
const ANGLE_EPS: f64 = 16.0 * f64::EPSILON;
const MAX_OBSTACLES: usize = 100_000;

/// One remaining straight segment and its distance from the uncut stroke's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    /// Start point in paper coordinates.
    pub start: [f64; 2],
    /// End point in paper coordinates.
    pub end: [f64; 2],
    /// Distance from the original stroke start.
    pub distance: f64,
}

fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn valid(p: [f64; 2]) -> bool {
    p.into_iter().all(|x| x.is_finite() && x.abs() <= LIMIT)
}
fn bad() -> SolidError {
    SolidError::InvalidInput {
        operation: "geom::paper_gaps::cut",
        reason: "invalid or excessive paper gap geometry",
    }
}

/// Cut a line around intersections and ordered convex quadrilaterals, expanding by `margin`.
/// Inputs are bounded in paper units; output distances are measured along the original line.
///
/// # Errors
/// Rejects out-of-range, non-finite, degenerate, non-convex or excessive inputs.
pub fn cut(
    a: [f64; 2],
    b: [f64; 2],
    lines: &[([f64; 2], [f64; 2])],
    boxes: &[[[f64; 2]; 4]],
    margin: f64,
) -> Result<Vec<Piece>> {
    if !valid(a)
        || !valid(b)
        || !margin.is_finite()
        || !(0.0..=MAX_MARGIN).contains(&margin)
        || lines.len().saturating_add(boxes.len()) > MAX_OBSTACLES
    {
        return Err(bad());
    }
    let (a, b) = (Vec2::from_array(a), Vec2::from_array(b));
    let d = b - a;
    let length = d.length();
    if length <= EPS {
        return Err(bad());
    }
    let mut cuts = Vec::<(f64, f64)>::new();
    for &(p, q) in lines {
        if !valid(p) || !valid(q) {
            return Err(bad());
        }
        let (p, q) = (Vec2::from_array(p), Vec2::from_array(q));
        let e = q - p;
        if e.length() <= EPS {
            return Err(bad());
        }
        let denominator = cross(d, e);
        if denominator.abs() <= ANGLE_EPS * length * e.length() {
            if cross(p - a, d).abs() <= EPS * length {
                let first = (p - a).dot(d) / d.length_squared();
                let second = (q - a).dot(d) / d.length_squared();
                let lo = (first.min(second) - margin / length).max(0.0);
                let hi = (first.max(second) + margin / length).min(1.0);
                if lo < hi {
                    cuts.push((lo, hi));
                }
            }
            continue;
        }
        let t = cross(p - a, e) / denominator;
        let u = cross(p - a, d) / denominator;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            cuts.push((
                (t - margin / length).max(0.0),
                (t + margin / length).min(1.0),
            ));
        }
    }
    for corners in boxes {
        if !corners.iter().copied().all(valid) {
            return Err(bad());
        }
        let points = corners.map(Vec2::from_array);
        let [p0, p1, p2, p3] = points;
        let edges = [p1 - p0, p2 - p1, p3 - p2, p0 - p3];
        if edges.iter().any(|edge| edge.length() <= EPS) {
            return Err(bad());
        }
        let [e0, e1, e2, e3] = edges;
        let sign = cross(e0, e1).signum();
        let turns = [(e0, e1), (e1, e2), (e2, e3), (e3, e0)];
        if sign == 0.0
            || turns.into_iter().any(|(first, second)| {
                sign * cross(first, second) <= ANGLE_EPS * first.length() * second.length()
            })
        {
            return Err(bad());
        }
        let (mut lo, mut hi): (f64, f64) = (0.0, 1.0);
        for (&p, &edge) in points.iter().zip(edges.iter()) {
            let c = sign * cross(edge, a - p) + margin * edge.length();
            let slope = sign * cross(edge, d);
            if slope.abs() <= ANGLE_EPS * edge.length() * length {
                if c < 0.0 {
                    lo = 1.0;
                    hi = 0.0;
                    break;
                }
            } else if slope > 0.0 {
                lo = lo.max(-c / slope);
            } else {
                hi = hi.min(-c / slope);
            }
        }
        if lo < hi && hi > 0.0 && lo < 1.0 {
            cuts.push((lo.max(0.0), hi.min(1.0)));
        }
    }
    cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut result = Vec::new();
    let mut from: f64 = 0.0;
    for (lo, hi) in cuts {
        if lo > from && (lo - from) * length > EPS {
            result.push(Piece {
                start: (a + d * from).to_array(),
                end: (a + d * lo).to_array(),
                distance: from * length,
            });
        }
        from = from.max(hi);
    }
    if (1.0 - from) * length > EPS {
        result.push(Piece {
            start: (a + d * from).to_array(),
            end: b.to_array(),
            distance: from * length,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossings_and_text_boxes_merge_and_preserve_start_distances() {
        let lines = [([4.0, -2.0], [4.0, 2.0]), ([5.0, -2.0], [5.0, 2.0])];
        let boxes = [[[7.0, -1.0], [8.0, -1.0], [8.0, 1.0], [7.0, 1.0]]];
        let p = cut([0.0, 0.0], [10.0, 0.0], &lines, &boxes, 0.5).unwrap();
        // The expected positions are exact paper values; allow arithmetic roundoff.
        let expected = [
            Piece {
                start: [0.0, 0.0],
                end: [3.5, 0.0],
                distance: 0.0,
            },
            Piece {
                start: [5.5, 0.0],
                end: [6.5, 0.0],
                distance: 5.5,
            },
            Piece {
                start: [8.5, 0.0],
                end: [10.0, 0.0],
                distance: 8.5,
            },
        ];
        assert_eq!(p.len(), expected.len());
        for (actual, wanted) in p.iter().zip(expected) {
            for (actual, wanted) in actual
                .start
                .into_iter()
                .chain(actual.end)
                .zip(wanted.start.into_iter().chain(wanted.end))
            {
                assert!((actual - wanted).abs() <= 1e-12, "{actual} != {wanted}");
            }
            assert!((actual.distance - wanted.distance).abs() <= 1e-12);
        }
        assert_eq!(
            cut(
                [0.0, 0.0],
                [10.0, 0.0],
                &[([0.0, 0.0], [10.0, 0.0])],
                &[],
                0.5
            )
            .unwrap(),
            Vec::new()
        );
        assert_eq!(
            cut([0.0, 0.0], [10.0, 0.0], &[], &[], 0.5).unwrap().len(),
            1
        );
    }

    #[test]
    fn hostile_and_malformed_inputs_are_refused() {
        let box_bad = [[[0.0, 0.0], [1.0, 1.0], [0.0, 1.0], [1.0, 0.0]]];
        assert!(cut([0.0, 0.0], [10.0, 0.0], &[], &box_bad, 0.5).is_err());
        assert!(cut([f64::MAX, 0.0], [10.0, 0.0], &[], &[], 0.5).is_err());
        assert!(cut([0.0, 0.0], [10.0, 0.0], &[], &[], f64::NAN).is_err());
        assert!(cut([0.0, 0.0], [10.0, 0.0], &[], &[], MAX_MARGIN + 1.0).is_err());
        assert!(cut(
            [0.0, 0.0],
            [10.0, 0.0],
            &[([1.0, 1.0], [1.0, 1.0])],
            &[],
            0.5
        )
        .is_err());
    }
    #[test]
    fn short_perpendicular_crossing_and_box_scale_with_the_geometry() {
        for scale in [1e-5, 1.0, 1e4] {
            let pieces = cut(
                [0.0, 0.0],
                [scale, 0.0],
                &[([0.5 * scale, -0.5 * scale], [0.5 * scale, 0.5 * scale])],
                &[],
                0.1 * scale,
            )
            .unwrap();
            assert_eq!(pieces.len(), 2);
            assert!(
                (pieces.first().unwrap().end.into_iter().next().unwrap() - 0.4 * scale).abs()
                    < scale * 1e-12
            );
            assert!(
                (pieces.get(1).unwrap().start.into_iter().next().unwrap() - 0.6 * scale).abs()
                    < scale * 1e-12
            );
            let text = [[
                [0.4 * scale, -0.1 * scale],
                [0.6 * scale, -0.1 * scale],
                [0.6 * scale, 0.1 * scale],
                [0.4 * scale, 0.1 * scale],
            ]];
            let pieces = cut([0.0, 0.0], [scale, 0.0], &[], &text, 0.0).unwrap();
            assert_eq!(pieces.len(), 2);
            assert!(
                (pieces.first().unwrap().end.into_iter().next().unwrap() - 0.4 * scale).abs()
                    < scale * 1e-12
            );
            assert!(
                (pieces.get(1).unwrap().start.into_iter().next().unwrap() - 0.6 * scale).abs()
                    < scale * 1e-12
            );
        }
    }
}
