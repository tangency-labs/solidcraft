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
/// [`SolidError::InvalidInput`] for a zero or non-finite `toward`, or a `radius` not finite and
/// positive.
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
    Ok(Some((-bb + disc.sqrt()) / (2.0 * aa)))
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
    }
}
