//! How a section's outline turns at its corners, and how far a set of points reaches past a
//! member's end: the measures a frame member's fit is decided on (an open section such as an
//! angle or a channel, an inside corner's fillet, the far side of the members an end fits
//! against). The caller compares them with its tolerances.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`measure_corner_turns`] | (none)   |
//! | [`reach_along`]          | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::error::Result;
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

/// How far past `end` along `toward` the furthest of `points` lies: the largest
/// `(p − end) · t` for `t` the unit vector along `toward`. `None` for no points.
///
/// # Errors
///
/// [`crate::error::SolidError::InvalidInput`] for a zero or non-finite `toward`.
pub fn reach_along(end: Point3, toward: Vec3, points: &[Point3]) -> Result<Option<f64>> {
    let t = toward.unitize()?;
    Ok(points.iter().map(|p| (*p - end).dot(t)).reduce(f64::max))
}
