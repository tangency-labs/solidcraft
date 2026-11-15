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
    At { point: Vec3, t: f64 },
    /// It does not between the piece's ends; the end nearer `θ` in angle.
    Nearer { point: Vec3, t: f64 },
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
    [s, t]: [f64; 2],
    theta: f64,
) -> Option<AngleCrossing> {
    let off = |u: f64| -> Option<(f64, Vec3)> {
        let p = curve.point(u)?;
        let d = p - origin;
        Some((wrap(d.dot(y).atan2(d.dot(x)) - theta), p))
    };
    let ((d0, p0), (d1, p1)) = (off(s)?, off(t)?);
    if d0 == 0.0 {
        return Some(AngleCrossing::At { point: p0, t: s });
    }
    if d1 == 0.0 {
        return Some(AngleCrossing::At { point: p1, t });
    }
    // The wrap is at π: a piece under half a turn whose offsets differ in sign across it does
    // not reach θ (its angle passes θ + π instead).
    if d0.signum() == d1.signum() || (d1 - d0).abs() >= PI {
        return Some(if d0.abs() <= d1.abs() {
            AngleCrossing::Nearer { point: p0, t: s }
        } else {
            AngleCrossing::Nearer { point: p1, t }
        });
    }
    let (mut a, mut b, mut fa) = (s, t, d0);
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if !(a < m && m < b) {
            break;
        }
        let (fm, _) = off(m)?;
        if fm == 0.0 {
            a = m;
            b = m;
            break;
        }
        if fm.signum() == fa.signum() {
            a = m;
            fa = fm;
        } else {
            b = m;
        }
    }
    let m = 0.5 * (a + b);
    off(m).map(|(_, point)| AngleCrossing::At { point, t: m })
}

#[cfg(test)]
mod tests;
