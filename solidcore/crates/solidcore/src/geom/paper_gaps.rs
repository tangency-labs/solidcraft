//! Original bounded paper-stroke cutting; no implementation is ported from another product.
//! Intersecting half-plane parameter bounds follows Liang and Barsky, ACM TOG 3 (1984),
//! DOI 10.1145/357332.357333, and Cyrus and Beck, Computers & Graphics 3 (1978),
//! DOI 10.1016/0097-8493(78)90021-3. The union of excluded intervals is complemented.
//! Coordinates are finite within ±100000; margins are finite within 0..=5000 paper units.

use crate::error::{Result, SolidError};
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::Vec3;

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

/// The length of the straight stroke from `a` to `b`.
pub fn straight_length(a: [f64; 2], b: [f64; 2]) -> f64 {
    (Vec2::from_array(b) - Vec2::from_array(a)).length()
}

/// Whether the endpoints of a model line project to distinct points in the given plane.
pub fn projected_endpoints_are_distinct(
    origin: [f64; 3],
    axes: [[f64; 3]; 2],
    ends: [[f64; 3]; 2],
) -> bool {
    let origin = Vec3::from(origin);
    let axes = axes.map(Vec3::from);
    let [a, b] = ends.map(|point| axes.map(|axis| Vec3::from(point).distance_along(origin, axis)));
    extend(a, b, 0.0).is_ok()
}

/// Two orthogonal strokes through a circle's centre, each reaching beyond its radius by
/// `overshoot`. The strokes must fit within the permitted paper coordinates.
pub fn centre_cross(centre: [f64; 2], radius: f64, overshoot: f64) -> Option<[[[f64; 2]; 2]; 2]> {
    if !valid(centre)
        || !radius.is_finite()
        || radius <= 0.0
        || !(0.0..=MAX_MARGIN).contains(&overshoot)
    {
        return None;
    }
    let half = radius + overshoot;
    if half <= EPS
        || centre
            .iter()
            .any(|coordinate| coordinate.abs() + half > LIMIT)
    {
        return None;
    }
    let centre = Vec2::from_array(centre);
    Some(
        [Vec2::new(half, 0.0), Vec2::new(0.0, half)]
            .map(|delta| [(centre - delta).to_array(), (centre + delta).to_array()]),
    )
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

/// The stroke from `a` to `b` lengthened by `overshoot` past each end along its own direction, as
/// a centre line reaches past the points it joins; with no `overshoot`, whether its ends are apart.
/// The ends may lie anywhere finite (a caller bounds them to its paper); `overshoot` is within
/// 0..=5000 paper units, as [`cut`]'s margin is.
///
/// # Errors
/// Rejects non-finite input or output, an out-of-range `overshoot`, and ends that coincide.
pub fn extend(a: [f64; 2], b: [f64; 2], overshoot: f64) -> Result<([f64; 2], [f64; 2])> {
    let bad = || SolidError::InvalidInput {
        operation: "geom::paper_gaps::extend",
        reason: "non-finite or coincident paper stroke ends",
    };
    let finite = |p: [f64; 2]| p.into_iter().all(f64::is_finite);
    if !finite(a)
        || !finite(b)
        || !overshoot.is_finite()
        || !(0.0..=MAX_MARGIN).contains(&overshoot)
    {
        return Err(bad());
    }
    let (a, b) = (Vec2::from_array(a), Vec2::from_array(b));
    let d = b - a;
    let length = d.length();
    if !length.is_finite() || length <= EPS {
        return Err(bad());
    }
    let along = d * (overshoot / length);
    let (start, end) = ((a - along).to_array(), (b + along).to_array());
    if !finite(start) || !finite(end) {
        return Err(bad());
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_endpoints_must_be_distinct_in_the_view() {
        let origin = [10.0, 0.0, 0.0];
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        assert!(!projected_endpoints_are_distinct(
            origin,
            axes,
            [[11.0, 2.0, 3.0], [11.0, 2.0, 9.0]]
        ));
        assert!(projected_endpoints_are_distinct(
            origin,
            axes,
            [[11.0, 2.0, 3.0], [12.0, 2.0, 9.0]]
        ));
    }

    #[test]
    fn centre_cross_extends_beyond_the_circle_along_both_axes() {
        let cross = centre_cross([10.0, 20.0], 3.0, 2.0).unwrap();
        assert_eq!(
            cross,
            [[[5.0, 20.0], [15.0, 20.0]], [[10.0, 15.0], [10.0, 25.0]]]
        );
        let [a, b] = cross[0];
        assert!((straight_length(a, b) - 10.0).abs() < EPS);
        assert!(centre_cross([LIMIT, 0.0], 3.0, 2.0).is_none());
    }

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

    #[test]
    fn extend_lengthens_both_ends_along_the_stroke_and_refuses_coincident_ends() {
        let (start, end) = extend([1.0, 1.0], [4.0, 5.0], 2.5).unwrap();
        // The stroke is 3-4-5, so 2.5 along it is (1.5, 2.0).
        assert!((start[0] + 0.5).abs() < 1e-12 && (start[1] + 1.0).abs() < 1e-12);
        assert!((end[0] - 5.5).abs() < 1e-12 && (end[1] - 7.0).abs() < 1e-12);
        assert_eq!(
            extend([2.0, 3.0], [7.0, 3.0], 0.0).unwrap(),
            ([2.0, 3.0], [7.0, 3.0])
        );
        assert!(extend([2.0, 3.0], [2.0, 3.0], 1.0).is_err());
        assert!(extend([0.0, 0.0], [1.0, 0.0], f64::NAN).is_err());
        assert!(extend([0.0, 0.0], [1.0, 0.0], 5_001.0).is_err());
        assert!(extend([0.0, f64::INFINITY], [1.0, 0.0], 1.0).is_err());
        assert!(extend([-f64::MAX, 0.0], [f64::MAX, 0.0], 0.0).is_err());
        // Ends off any paper are still apart: the caller bounds them to its paper.
        assert_eq!(
            extend([0.0, 0.0], [2e5, 0.0], 0.0).unwrap(),
            ([0.0, 0.0], [2e5, 0.0])
        );
    }
}
