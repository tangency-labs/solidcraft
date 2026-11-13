//! A closed chain of section nodes (a mesh cap's boundary, walked in order) fitted as the loop
//! it samples: a full circle where the nodes are one within the caller's tolerance, else the
//! polygon of its corners, each straight run of nodes joined into one side.
//!
//! Not upstream: SolidCraft's Convert Mesh section fit (`fit_loop` in its
//! `crates/kernel/src/mesh_convert.rs`), moved into the kernel so the layers above compute no
//! geometry (docs/layers.md §1). A fit at a tolerance, not a proved query: the conversion that
//! calls it certifies the fitted section against the mesh afterwards (its nodes on the fitted
//! loops, the nesting, the volume). The formulas and their order are SolidCraft's, written on the
//! components so the results are the same to the bit: distances are `hypot`, a line's closest
//! point is the clamped projection.
//!
//! Deviations: none (no upstream counterpart). No recursion; [`straight_runs_joined`] counts its
//! node tests against n² + n and refuses past it.

use crate::error::{Result, SolidError};
use crate::math::Point2;

const OP: &str = "prism::fit::straight_runs_joined";

/// A section loop as [`fit_loop`] fitted it.
#[derive(Clone, Debug, PartialEq)]
pub enum FittedLoop {
    /// A full circle.
    Circle {
        /// Its centre.
        center: Point2,
        /// Its radius.
        radius: f64,
    },
    /// The polygon through these corners, closed.
    Polygon(Vec<Point2>),
}

fn sub(a: Point2, b: Point2) -> Point2 {
    Point2::new(a.x - b.x, a.y - b.y)
}

fn dot(a: Point2, b: Point2) -> f64 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Point2, b: Point2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dist(a: Point2, b: Point2) -> f64 {
    let d = sub(a, b);
    d.x.hypot(d.y)
}

/// How far `p` lies from the segment `a`–`b`: from its closest point, the projection clamped to
/// the segment (`a` itself when `a == b`).
fn off_line(a: Point2, b: Point2, p: Point2) -> f64 {
    let d = sub(b, a);
    let mut t = 0.0;
    if a != b {
        let denominator = dot(d, d);
        let s = dot(sub(p, a), d) / denominator;
        if denominator > 0.0 && denominator.is_finite() && s.is_finite() {
            t = s.clamp(0.0, 1.0);
        }
    }
    dist(Point2::new(a.x + d.x * t, a.y + d.y * t), p)
}

/// The loop through `points` (a closed chain, the last joined to the first): a full circle when
/// there are twelve nodes or more, all within three times `epsilon` of the circle through the
/// first node and those a third and two thirds of the way round, turning one way by at most 30°
/// a step to one full turn, each chord within `tolerance` of the arc (less ten times `epsilon`).
/// Else the polygon of its corners ([`straight_runs_joined`]).
///
/// # Errors
/// [`straight_runs_joined`]'s.
pub fn fit_loop(points: &[Point2], tolerance: f64, epsilon: f64) -> Result<FittedLoop> {
    // Fit a complete circular boundary only with enough samples to distinguish it
    // from a polygon. Certify the chord deviation, not just the vertex residual.
    if points.len() >= 12 {
        let (Some(&a), Some(&b), Some(&c)) = (
            points.first(),
            points.get(points.len() / 3),
            points.get(2 * points.len() / 3),
        ) else {
            return Ok(FittedLoop::Polygon(points.to_vec()));
        };
        let u = sub(b, a);
        let v = sub(c, a);
        let d = 2. * cross(u, v);
        if d.abs() > epsilon * epsilon {
            let (uu, vv) = (dot(u, u), dot(v, v));
            let center = Point2::new(
                a.x + (v.y * uu - u.y * vv) / d,
                a.y + (u.x * vv - v.x * uu) / d,
            );
            let radius = dist(center, a);
            if radius.is_finite()
                && radius > epsilon
                && points
                    .iter()
                    .all(|p| (dist(center, *p) - radius).abs() <= epsilon * 3.)
            {
                let mut sweep = 0.;
                let mut direction = 0f64;
                let valid = (0..points.len()).all(|i| {
                    let (Some(&a), Some(&b)) = (
                        points.get(i),
                        (i + 1)
                            .checked_rem(points.len())
                            .and_then(|j| points.get(j)),
                    ) else {
                        return false;
                    };
                    let (a, b) = (sub(a, center), sub(b, center));
                    let signed = cross(a, b).atan2(dot(a, b));
                    if direction == 0. {
                        direction = signed.signum();
                    }
                    if signed * direction <= 0. {
                        return false;
                    }
                    sweep += signed;
                    let angle = signed.abs();
                    angle <= std::f64::consts::PI / 6. + 1e-8
                        && radius * (1. - (angle * 0.5).cos()) + epsilon * 10. <= tolerance
                });
                if valid && (sweep.abs() - std::f64::consts::TAU).abs() <= 1e-7 {
                    return Ok(FittedLoop::Circle { center, radius });
                }
            }
        }
    }
    straight_runs_joined(points, epsilon).map(FittedLoop::Polygon)
}

/// The corners of the closed polygon through `points`: each straight run of nodes (a mesher that
/// samples a straight edge with nodes along it, as a quality mesh does) becomes one side. A run
/// is grown from its first node while every node it spans lies within the rounding allowance
/// (three times `epsilon`, as an STL's f32 coordinates are allowed for) of the side from its
/// first node to its last; the first node of all is dropped too where it lies on the side
/// through it. Every node is measured against the side that replaces it, never against a side
/// already simplified, so joining never drifts.
///
/// # Errors
/// [`SolidError::InvalidInput`] past n² + n node tests for n nodes. Growing a run of m nodes
/// tests at most m²/2 of them, and the first node's check at most n, so that bound is never
/// reached; it is kept as a refusal rather than a loop that could run on.
pub fn straight_runs_joined(points: &[Point2], epsilon: f64) -> Result<Vec<Point2>> {
    let n = points.len();
    if n <= 3 {
        return Ok(points.to_vec());
    }
    let allowance = epsilon * 3.;
    let bound = n.saturating_mul(n).saturating_add(n);
    let mut tests = 0usize;
    let at = |i: usize| {
        i.checked_rem(n)
            .and_then(|i| points.get(i))
            .copied()
            .unwrap_or_default()
    };
    // Whether every node strictly between `i` and `j` (going forward, `j` may pass `n`) lies
    // within the allowance of the side from node `i` to node `j`.
    let straight = |i: usize, j: usize, tests: &mut usize| -> Result<bool> {
        for k in i + 1..j {
            *tests += 1;
            if *tests > bound {
                return Err(SolidError::InvalidInput {
                    operation: OP,
                    reason: "more node tests than n² + n for a loop of n nodes",
                });
            }
            if off_line(at(i), at(j), at(k)) > allowance {
                return Ok(false);
            }
        }
        Ok(true)
    };
    let mut kept = vec![0usize];
    let mut i = 0usize;
    let mut j = 2usize;
    while j <= n {
        if straight(i, j, &mut tests)? {
            j += 1;
        } else {
            kept.push(j - 1);
            i = j - 1;
            j = i + 2;
        }
    }
    // The first node is dropped too where it lies on the side through it.
    let mut first = 0;
    if let (Some(&last), Some(&second)) = (kept.last(), kept.get(1)) {
        if kept.len() > 3 && straight(last, second + n, &mut tests)? {
            first = 1;
        }
    }
    Ok(kept.iter().skip(first).map(|&k| at(k)).collect())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// `n` nodes evenly round the circle of radius 10 about the origin.
    fn ring(n: usize) -> Vec<Point2> {
        (0..n)
            .map(|i| {
                let angle = i as f64 * std::f64::consts::TAU / n as f64;
                Point2::new(angle.cos() * 10., angle.sin() * 10.)
            })
            .collect()
    }

    /// A coarse polygon is no circle (six nodes, or sixteen whose chords are off the arc by more
    /// than the tolerance): its corners stay, none rounded. Within the tolerance sixteen nodes
    /// are the exact circle.
    #[test]
    fn coarse_polygons_are_not_rounded_and_a_fine_one_is_the_circle() {
        let FittedLoop::Polygon(corners) = fit_loop(&ring(6), 1., 1e-6).unwrap() else {
            panic!("a hexagon is no circle")
        };
        assert_eq!(corners.len(), 6);
        let FittedLoop::Polygon(corners) = fit_loop(&ring(16), 0.001, 1e-6).unwrap() else {
            panic!("too coarse for 0.001")
        };
        assert_eq!(corners.len(), 16);
        let FittedLoop::Circle { center, radius } = fit_loop(&ring(16), 0.2, 1e-6).unwrap() else {
            panic!("not the circle")
        };
        assert!(
            dist(center, Point2::new(0., 0.)) < 1e-9 && (radius - 10.).abs() < 1e-9,
            "centre {center:?}, radius {radius}"
        );
    }

    /// A square sampled with nodes along its sides (the first node mid-side) is its four corners,
    /// in order; a node off a side by more than the allowance stays a corner.
    #[test]
    fn straight_runs_join_into_the_corners() {
        let mut nodes = Vec::new();
        for (a, b) in [
            ((0., 0.), (10., 0.)),
            ((10., 0.), (10., 10.)),
            ((10., 10.), (0., 10.)),
            ((0., 10.), (0., 0.)),
        ] {
            for k in 0..5 {
                let t = f64::from(k) / 5.;
                nodes.push(Point2::new(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
        nodes.rotate_left(2);
        let corners = straight_runs_joined(&nodes, 1e-6).unwrap();
        let expect = [(10., 0.), (10., 10.), (0., 10.), (0., 0.)].map(|(x, y)| Point2::new(x, y));
        assert_eq!(corners, expect);
        nodes[7].x += 1e-3;
        assert!(straight_runs_joined(&nodes, 1e-6)
            .unwrap()
            .contains(&nodes[7]));
    }
}
