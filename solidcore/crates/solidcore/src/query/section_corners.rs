//! How a section's outline turns at its corners, and how far a member's end runs on through
//! the box round another's section: the measures a frame member's fit is decided on (an open section such as an
//! angle or a channel, an inside corner's fillet, the far side of the members an end fits
//! against). The caller compares them with its tolerances.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`measure_corner_turns`] | (none)   |
//! | [`exit_through_box`]     | (none)   |
//! | [`exit_through_round`]   | (none)   |
//! | [`reach_through_rounds`] | (none)   |
//! | [`square_axes`]          | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::error::{Result, SolidError};
use crate::math::{Point2, Point3, Vec3};
use crate::prism::profile::{Loop2, Seg2};

/// The cross product of two plane vectors (`a.x b.y - a.y b.x`).
fn cross(a: Point2, b: Point2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// How a closed outline turns at its corners.
#[derive(Clone, Debug, PartialEq)]
pub struct CornerTurns {
    /// Twice the outline's signed area through its segments' ends (the shoelace sum of
    /// `start × end`): above 0 counter-clockwise.
    pub winding: f64,
    /// For each segment, whether it is an arc turning against the winding: an inside corner's
    /// fillet (an angle's or a channel's root). An outside corner's arc turns with it.
    pub inward_arcs: Vec<bool>,
    /// The least of `(b − a) × (d − c) · winding` over each straight side `a → b` followed by a
    /// straight side `c → d`: below 0 where a side turns inward into the next (an inside corner
    /// with no fillet). `+∞` when no two straight sides follow each other.
    pub sharpest_turn: f64,
}

/// Measure how `outline` turns at its corners.
#[must_use]
pub fn measure_corner_turns(outline: &Loop2) -> CornerTurns {
    let segs = &outline.segs;
    let winding: f64 = segs.iter().map(|g| cross(g.start(), g.end())).sum();
    let inward_arcs = segs
        .iter()
        .map(|g| matches!(*g, Seg2::Arc { radius, sweep, .. } if sweep * radius.signum() * winding < 0.0))
        .collect();
    let sharpest_turn = segs
        .iter()
        .zip(segs.iter().cycle().skip(1))
        .filter_map(|(g, h)| match (*g, *h) {
            (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) => {
                let u = Point2::new(b.x - a.x, b.y - a.y);
                let v = Point2::new(d.x - c.x, d.y - c.y);
                Some(cross(u, v) * winding)
            }
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    CornerTurns {
        winding,
        inward_arcs,
        sharpest_turn,
    }
}

/// How far along `toward` from `end` its line leaves the box `rect` bounds: the box's sides
/// run through the rectangle's edges, square to its plane, without end (a member's
/// section swept along the member). The exit of the slab between each pair of opposite
/// sides, `(side − end) · n / (t · n)` for `t` the unit vector along `toward`, at the nearer
/// of the two slabs; a slab the line runs along (`t · n` within `PARALLEL` of 0) never
/// stops it. `None` where the line runs along both, or `rect` is degenerate.
///
/// # Errors
///
/// [`crate::error::SolidError::InvalidInput`] for a zero or non-finite `toward`.
pub fn exit_through_box(end: Point3, toward: Vec3, rect: &[Point3; 4]) -> Result<Option<f64>> {
    const PARALLEL: f64 = 1e-9;
    let t = toward.unitize()?;
    let [a, b, _, d] = *rect;
    let mut exit: Option<f64> = None;
    for (near, far) in [(a, b), (a, d)] {
        let Ok(n) = (far - near).unitize() else {
            return Ok(None);
        };
        let tn = t.dot(n);
        if tn.abs() <= PARALLEL {
            continue;
        }
        let out = ((near - end).dot(n) / tn).max((far - end).dot(n) / tn);
        exit = Some(exit.map_or(out, |e| e.min(out)));
    }
    Ok(exit)
}

/// How far along `toward` from `start` its line leaves the round bar of `radius` about the line
/// through `a` and `b`, without end: the room a member's section takes about its line, when only
/// its line and its farthest point from it are known. The farther root `s` of
/// `|((start + s t) − a)⊥| = radius`, for `t` the unit vector along `toward` and `⊥` the part
/// square to `b − a`. `None` where the line misses the bar, runs along it (its square part within
/// `PARALLEL` of 0), or `a` and `b` are one point.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a zero or non-finite `toward`, a `radius` not finite and
/// positive, or a non-finite `start`, `a` or `b`.
pub fn exit_through_round(
    start: Point3,
    toward: Vec3,
    (a, b): (Point3, Point3),
    radius: f64,
) -> Result<Option<f64>> {
    const PARALLEL: f64 = 1e-9;
    if !(radius.is_finite() && radius > 0.0) {
        return Err(SolidError::InvalidInput {
            operation: "exit_through_round",
            reason: "the radius must be finite and positive",
        });
    }
    let finite = |p: Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    if !(finite(start) && finite(a) && finite(b)) {
        return Err(SolidError::InvalidInput {
            operation: "exit_through_round",
            reason: "the start and the axis must be finite points",
        });
    }
    let t = toward.unitize()?;
    let Ok(w) = (b - a).unitize() else {
        return Ok(None);
    };
    let p = start - a;
    let p_sq = p - w * p.dot(w);
    let t_sq = t - w * t.dot(w);
    let aa = t_sq.dot(t_sq);
    if aa <= PARALLEL {
        return Ok(None);
    }
    let bb = 2.0 * p_sq.dot(t_sq);
    let cc = p_sq.dot(p_sq) - radius * radius;
    let disc = bb * bb - 4.0 * aa * cc;
    if disc < 0.0 {
        return Ok(None);
    }
    let s = (-bb + disc.sqrt()) / (2.0 * aa);
    if !s.is_finite() {
        return Err(SolidError::InvalidInput {
            operation: "exit_through_round",
            reason: "the exit distance is not finite",
        });
    }
    Ok(Some(s))
}

/// How far past `end` along `toward` a member's end is built to leave the round bars `rounds`
/// (each its axis's ends and radius) that `end` lies in: the farthest any point of its section
/// runs on before it leaves such a bar's far side. The section is bounded by `sections`, each a
/// rectangle's corners in order placed square to the line at `end`, so the end reaches the
/// farthest exit over each rectangle (and over `end` itself). Bars `end` lies outside of are not
/// met. At least 0, at most `most` (the member's own length: a far side that far off is no joint
/// its end makes).
///
/// The exit of a point is the farther root `s` of `|((p + s t) − a)⊥| = radius` ([`exit_through_round`]).
/// Over a rectangle it is greatest on an edge: the points `(p, s)` with `p` in the rectangle's
/// plane that reach the bar's surface form a convex set unbounded only along one direction, so
/// its highest `s` lies on the rectangle's boundary. Along an edge `p(u) = P + u E` the exit is
/// concave in `u`, greatest at an end of the edge, where its derivative is zero, or where the
/// line just touches the bar; each is a root of a quadratic in `u`, so the farthest exit is exact.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite `end`, `most` or corner, a negative `most`,
/// a zero or non-finite `toward`, or a radius not finite and positive for a bar `end` lies in.
pub fn reach_through_rounds(
    end: Point3,
    toward: Vec3,
    sections: &[[Point3; 4]],
    rounds: &[(Point3, Point3, f64)],
    most: f64,
) -> Result<f64> {
    const PARALLEL: f64 = 1e-9;
    let finite = |p: Point3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    if !(finite(end)
        && most.is_finite()
        && most >= 0.0
        && sections.iter().flatten().all(|p| finite(*p)))
    {
        return Err(SolidError::InvalidInput {
            operation: "reach_through_rounds",
            reason: "the end and the section's corners must be finite points and the length finite and not negative",
        });
    }
    let t = toward.unitize()?;
    let mut reach = 0.0f64;
    for &(a, b, radius) in rounds {
        // The nearest point of the axis's segment to `end`: a bar the end isn't in is not met.
        let axis = b - a;
        let aa = axis.dot(axis);
        let along = if aa > 0.0 {
            ((end - a).dot(axis) / aa).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let off = end - (a + axis * along);
        if off.dot(off).sqrt() > radius || off.dot(off).is_nan() {
            continue;
        }
        if !(radius.is_finite() && radius > 0.0) {
            return Err(SolidError::InvalidInput {
                operation: "reach_through_rounds",
                reason: "a bar's radius must be finite and positive",
            });
        }
        let Ok(w) = axis.unitize() else { continue };
        let square = |v: Vec3| v - w * v.dot(w);
        let c = square(t);
        let cc = c.dot(c);
        if cc <= PARALLEL {
            continue;
        }
        let rr = cc * radius * radius;
        // The exit of a point whose part square to the axis, from the axis, is `q`.
        let exit = |q: Vec3| {
            let m = c.cross(q);
            let d = rr - m.dot(m);
            (d >= 0.0)
                .then(|| (-c.dot(q) + d.sqrt()) / cc)
                .filter(|s| s.is_finite())
        };
        if let Some(s) = exit(square(end - a)) {
            reach = reach.max(s);
        }
        for rect in sections {
            for (&p, &next) in rect.iter().zip(rect.iter().cycle().skip(1)) {
                let e = square(next - p);
                let q0 = square(p - a);
                let (m0, k, beta) = (c.cross(q0), c.cross(e), c.dot(e));
                let (kk, mk, m0m0) = (k.dot(k), m0.dot(k), m0.dot(m0));
                let mut us = vec![0.0, 1.0];
                let mut roots = |qa: f64, qb: f64, qc: f64| {
                    if qa.abs() > 1e-300 {
                        let disc = qb * qb - 4.0 * qa * qc;
                        if disc >= 0.0 {
                            let r = disc.sqrt();
                            us.extend([(-qb - r) / (2.0 * qa), (-qb + r) / (2.0 * qa)]);
                        }
                    }
                };
                // Where the exit's derivative along the edge is zero: (m·k)² = β²(|c|²r² − |m|²).
                roots(
                    kk * (kk + beta * beta),
                    2.0 * mk * (kk + beta * beta),
                    mk * mk - beta * beta * (rr - m0m0),
                );
                // Where the line just touches the bar: |m|² = |c|²r².
                roots(kk, 2.0 * mk, m0m0 - rr);
                for u in us.into_iter().filter(|u| (0.0..=1.0).contains(u)) {
                    if let Some(s) = exit(q0 + e * u) {
                        reach = reach.max(s);
                    }
                }
            }
        }
    }
    // `most` is finite and not negative (checked above).
    Ok(reach.max(0.0).min(most))
}

/// Two unit axes square to `d`, `x × y` along `d`: a plane on them has `d` as its normal.
///
/// # Errors
///
/// The refusal of [`Vec3::make_unit_ortho_vectors`] for a zero or non-finite `d`.
pub fn square_axes(d: Vec3) -> Result<(Vec3, Vec3)> {
    let [_, x, y] = d.make_unit_ortho_vectors(None)?;
    Ok((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_leaves_a_round_bar_at_its_far_side() {
        // The bar of radius 10 about the X axis. From the axis straight up: out at 10; from 30 below at 45°
        // across it: out where it meets z = 10 on the far side, 40·√2 along.
        let axis = (Point3::new(0.0, 0.0, 0.0), Point3::new(100.0, 0.0, 0.0));
        let up = exit_through_round(
            Point3::new(5.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            axis,
            10.0,
        )
        .unwrap();
        assert!((up.unwrap() - 10.0).abs() < 1e-12, "{up:?}");
        let across = exit_through_round(
            Point3::new(0.0, 0.0, -30.0),
            Vec3::new(1.0, 0.0, 1.0),
            axis,
            10.0,
        )
        .unwrap();
        assert!(
            (across.unwrap() - 40.0 * 2f64.sqrt()).abs() < 1e-9,
            "{across:?}"
        );
        // Along it, past it, a zero direction and a bad radius.
        assert_eq!(
            exit_through_round(
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                axis,
                10.0
            )
            .unwrap(),
            None
        );
        assert_eq!(
            exit_through_round(
                Point3::new(0.0, 0.0, 20.0),
                Vec3::new(0.0, 1.0, 0.0),
                axis,
                10.0
            )
            .unwrap(),
            None
        );
        assert!(exit_through_round(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            axis,
            10.0
        )
        .is_err());
        assert!(exit_through_round(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            axis,
            f64::NAN
        )
        .is_err());
        // A non-finite start or axis is refused, not measured as NaN.
        assert!(exit_through_round(
            Point3::new(0.0, f64::NAN, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            axis,
            10.0
        )
        .is_err());
        assert!(exit_through_round(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            (
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(f64::INFINITY, 0.0, 0.0)
            ),
            10.0
        )
        .is_err());
    }

    #[test]
    fn an_end_reaches_past_the_bars_it_lies_in_by_its_farthest_point() {
        // An end at the X axis heading up, its section the rectangle 10 across X and Y (z = -3 to 2 on a
        // slant); the bar of radius 10 about the X axis is left where y² + z² = 100, the bar about a line
        // 100 off is not met.
        let end = Point3::new(50.0, 0.0, 0.0);
        let rect = [
            Point3::new(45.0, -5.0, -3.0),
            Point3::new(55.0, -5.0, -3.0),
            Point3::new(55.0, 5.0, 2.0),
            Point3::new(45.0, 5.0, 2.0),
        ];
        let near = (
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(100.0, 0.0, 0.0),
            10.0,
        );
        let far = (
            Point3::new(0.0, 100.0, 0.0),
            Point3::new(100.0, 100.0, 0.0),
            10.0,
        );
        let up = Vec3::new(0.0, 0.0, 1.0);
        let reach = reach_through_rounds(end, up, &[rect], &[near, far], 1000.0).unwrap();
        // Along the slanted edge z = -0.5 + 0.5y the exit is √(100 − y²) + 0.5 − 0.5y, greatest at y = −√20:
        // 5√5 + 0.5, just past the lowest corners' √75 + 3.
        assert!((reach - (5.0 * 5f64.sqrt() + 0.5)).abs() < 1e-9, "{reach}");
        assert_eq!(
            reach_through_rounds(end, up, &[rect], &[far], 1000.0).unwrap(),
            0.0
        );
        assert_eq!(
            reach_through_rounds(end, up, &[rect], &[near], 4.0).unwrap(),
            4.0
        );
        assert!(reach_through_rounds(end, up, &[rect], &[near], f64::NAN).is_err());
        assert!(
            reach_through_rounds(end, Vec3::new(0.0, 0.0, 0.0), &[rect], &[near], 1.0).is_err()
        );
        let bad = [rect[0], rect[1], rect[2], Point3::new(f64::NAN, 0.0, 0.0)];
        assert!(reach_through_rounds(end, up, &[bad], &[near], 1.0).is_err());
    }

    #[test]
    fn a_point_along_an_edge_can_leave_a_bar_later_than_every_corner() {
        // The bar of radius 10 about the X axis, an end at the origin heading (1, 0, 1)/√2 and a section
        // 10 across along u = (1, 0, -1)/√2 and 4 along Y: the edge's middle leaves later than its ends.
        // The farthest exit matches a fine sweep of the whole rectangle, and beats the corners'.
        let end = Point3::new(0.0, 0.0, 0.0);
        let r2 = std::f64::consts::FRAC_1_SQRT_2;
        let (u, v) = (Vec3::new(r2, 0.0, -r2), Vec3::new(0.0, 1.0, 0.0));
        let corner = |a: f64, b: f64| end + u * a + v * b;
        let rect = [
            corner(-5.0, -2.0),
            corner(5.0, -2.0),
            corner(5.0, 2.0),
            corner(-5.0, 2.0),
        ];
        let bar = (
            Point3::new(-100.0, 0.0, 0.0),
            Point3::new(100.0, 0.0, 0.0),
            10.0,
        );
        let toward = Vec3::new(r2, 0.0, r2);
        let reach = reach_through_rounds(end, toward, &[rect], &[bar], 1000.0).unwrap();
        let exit = |p: Point3| {
            exit_through_round(p, toward, (bar.0, bar.1), bar.2)
                .unwrap()
                .unwrap_or(0.0)
        };
        let corners = rect.iter().map(|p| exit(*p)).fold(0.0, f64::max);
        let n = 400;
        let mut swept = 0.0f64;
        for i in 0..=n {
            for j in 0..=n {
                let (a, b) = (
                    -5.0 + 10.0 * f64::from(i) / f64::from(n),
                    -2.0 + 4.0 * f64::from(j) / f64::from(n),
                );
                swept = swept.max(exit(corner(a, b)));
            }
        }
        assert!(reach > corners + 0.1, "edge {reach} vs corners {corners}");
        assert!(
            reach >= swept - 1e-9 && reach - swept < 1e-3,
            "{reach} vs swept {swept}"
        );
    }

    #[test]
    fn square_axes_make_a_plane_with_the_direction_as_its_normal() {
        let d = Vec3::new(1.0, 2.0, -2.0);
        let (x, y) = square_axes(d).unwrap();
        let n = x.cross(y);
        assert!((n - d * (1.0 / 3.0)).length() < 1e-12, "{n:?}");
        assert!(x.dot(d).abs() < 1e-12 && y.dot(d).abs() < 1e-12);
        assert!(square_axes(Vec3::new(0.0, 0.0, 0.0)).is_err());
    }
}
