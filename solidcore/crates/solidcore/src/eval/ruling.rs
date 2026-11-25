//! Where a curve piece meets a half-plane about an axis: the ruling (or meridian) at angle `θ`.
//!
//! A seam on a surface of revolution runs along the half-plane at one angle about the axis, and
//! where it meets a boundary curve is where the curve's angle reaches `θ`. For a piece of a curve
//! whose angle about the axis is monotone and spans less than half a turn (a Bézier piece whose
//! angular speed has one sign: the caller decides that), the angle's offset from `θ`, wrapped to
//! `(−π, π]`, changes sign at most once between the piece's ends, so bisection on the curve's own
//! parameter finds the meeting to the parameter's rounding. Where the offsets at both ends have
//! one sign the piece does not reach `θ` between them; the end whose angle is nearer is given, so
//! the caller can bound where the piece comes near.

use super::CurveEval;
use crate::math::Vec3;
use std::f64::consts::{PI, TAU};

/// How a monotone piece's angle about an axis meets the angle `θ` ([`angle_crossing`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AngleCrossing {
    /// The piece's angle reaches `θ` at parameter `t`, at `point`.
    At {
        /// The point on the curve where its angle is `θ`.
        point: Vec3,
        /// The curve's parameter there.
        t: f64,
    },
    /// It does not between the piece's ends; the end nearer `θ` in angle.
    Nearer {
        /// The end of the piece whose angle is nearer `θ`.
        point: Vec3,
        /// The curve's parameter at that end.
        t: f64,
    },
}

/// `x` wrapped into `(−π, π]`.
fn wrap(x: f64) -> f64 {
    x - (x / TAU).round() * TAU
}

/// Where the piece `[s, t]` of `curve`, its angle about the axis through `origin` (`x` and `y`
/// unit and square to it and to each other, the angle measured from `x` towards `y`) monotone
/// and under half a turn, reaches angle `theta`: bisected on the curve's parameter until the
/// interval stops shrinking. `None` where the curve can't be evaluated there.
#[must_use]
pub fn angle_crossing(
    curve: &CurveEval,
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    [start, end]: [f64; 2],
    theta: f64,
) -> Option<AngleCrossing> {
    let off = |at: f64| -> Option<(f64, Vec3)> {
        let point = curve.point(at)?;
        let radial = point - origin;
        Some((wrap(radial.dot(y).atan2(radial.dot(x)) - theta), point))
    };
    let ((off_start, at_start), (off_end, at_end)) = (off(start)?, off(end)?);
    if off_start == 0.0 {
        return Some(AngleCrossing::At {
            point: at_start,
            t: start,
        });
    }
    if off_end == 0.0 {
        return Some(AngleCrossing::At {
            point: at_end,
            t: end,
        });
    }
    // The wrap is at π: a piece under half a turn whose offsets differ in sign across it does
    // not reach θ (its angle passes θ + π instead).
    if off_start.signum() == off_end.signum() || (off_end - off_start).abs() >= PI {
        return Some(if off_start.abs() <= off_end.abs() {
            AngleCrossing::Nearer {
                point: at_start,
                t: start,
            }
        } else {
            AngleCrossing::Nearer {
                point: at_end,
                t: end,
            }
        });
    }
    let (mut lo, mut hi, mut off_lo) = (start, end, off_start);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if !(lo < mid && mid < hi) {
            break;
        }
        let (off_mid, _) = off(mid)?;
        if off_mid == 0.0 {
            lo = mid;
            hi = mid;
            break;
        }
        if off_mid.signum() == off_lo.signum() {
            lo = mid;
            off_lo = off_mid;
        } else {
            hi = mid;
        }
    }
    let mid = 0.5 * (lo + hi);
    off(mid).map(|(_, point)| AngleCrossing::At { point, t: mid })
}

#[cfg(test)]
mod tests;
