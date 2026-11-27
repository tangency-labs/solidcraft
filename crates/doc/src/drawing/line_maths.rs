//! Points against a line on a drawing's paper (#13052): where a point is along a dimension's line
//! and across it, the farthest point on one side, whether one line already draws another, and
//! numbers spaced along a line so none overlap. Paper points are millimetres on the sheet, so these
//! are 2D. What a dimension reads on a view (model points seen in a view's plane) is
//! [`measure`]'s.

use super::measure::{self, LinearDirection};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use super::dimension::MAX_SET_TARGETS;
use crate::{DocError, Result};

/// The most places [`spread_along`] takes: an ordinate set's origin and its targets.
pub const MAX_ITEMS: usize = MAX_SET_TARGETS + 1;

/// How far apart two spread places may fall short of the pitch, as a share of their size. A place
/// is a mean plus `pitch * k`, within one `EPSILON` of its size, so a gap (a difference of two) is
/// within three; this is the power of two above that, so the size refusal can name what fits.
const GAP_ROUNDING: f64 = 4.0 * f64::EPSILON;

/// A line on paper: its unit direction and its normal, the unit turned a quarter left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineCoords {
    pub unit: Vec2,
    pub normal: Vec2,
}

impl LineCoords {
    /// The line a linear dimension measuring `direction` runs along on paper, `seen` being its
    /// first point to its last there: [`measure::axis`]'s direction, turned to run right,
    /// or up when it is upright, so a positive offset is above it or to its left whichever way round
    /// its points are. A view's paper map only scales and shifts, so its plane's directions are
    /// paper's. None when there is no direction (an aligned line whose ends meet).
    pub fn forward(direction: LinearDirection, seen: Vec2) -> Option<LineCoords> {
        let dir = measure::axis(direction, seen).ok()?;
        let forward = if dir.x.abs() > 1e-9 { dir.x > 0.0 } else { dir.y > 0.0 };
        let unit = if forward { dir } else { -dir };
        Some(LineCoords { unit, normal: unit.perp() })
    }

    /// How far `p` is along the line, from the paper's origin.
    pub fn along(&self, p: Vec2) -> f64 {
        p.dot(self.unit)
    }

    /// How far `p` is across the line, from the paper's origin, positive on the normal's side.
    pub fn across(&self, p: Vec2) -> f64 {
        p.dot(self.normal)
    }

    /// How far across the line the farthest of `points` lies on the side of `side`'s sign (the
    /// normal's side when it is 0 or more): the line a set's dimensions stand off from.
    pub fn reach(&self, points: &[Vec2], side: f64) -> f64 {
        let side = if side < 0.0 { -1.0 } else { 1.0 };
        side * points.iter().map(|p| side * self.across(*p)).fold(f64::NEG_INFINITY, f64::max)
    }
}

/// Whether `by` already draws `line`, both polylines on paper: the same line point for point
/// within `tol`, or, for two straight lines, `line` lying along `by` within its ends, all within
/// `tol`. A `by` whose ends meet has no direction, so only the same line covers it.
pub fn covers(line: &[Vec2], by: &[Vec2], tol: f64) -> bool {
    let same = || line.len() == by.len() && line.iter().zip(by).all(|(p, q)| p.dist(*q) <= tol);
    let (&[p, q], &[a, b]) = (line, by) else { return same() };
    let Some(u) = (b - a).normalized() else { return same() };
    let on = LineCoords { unit: u, normal: u.perp() };
    let within = |x: Vec2| on.across(x - a).abs() <= tol && (-tol..=a.dist(b) + tol).contains(&on.along(x - a));
    within(p) && within(q)
}

/// Places along one line for texts whose ideal places are `ideal` (least first), at least `pitch`
/// apart and moved as little as can be in all (least squares under their order).
///
/// Pool adjacent violators (Best and Chakravarti 1990) on the places less their spacing: texts that
/// would overlap pool into blocks, each centred on its members' ideal places, and every other text
/// stays where it is. O(n). Consecutive places come out `pitch` apart or more, less at most
/// [`GAP_ROUNDING`] of their size. Refused: more than [`MAX_ITEMS`] places, a place or pitch that
/// is not finite, a pitch of 0 or less, places out of order, and places so large against the pitch
/// that it can't be kept (2^50 pitches or more).
pub fn spread_along(ideal: &[f64], pitch: f64) -> Result<Vec<f64>> {
    let refuse = |why: String| Err(DocError::Invalid(why));
    if ideal.len() > MAX_ITEMS {
        return refuse(format!("more than {MAX_ITEMS} places: at most {MAX_ITEMS} fit"));
    }
    if !(pitch.is_finite() && pitch > 0.0) {
        return refuse("a pitch is finite and more than 0".into());
    }
    if !ideal.iter().all(|x| x.is_finite()) {
        return refuse("a place is finite".into());
    }
    if ideal.windows(2).any(|w| matches!(w, [a, b] if a > b)) {
        return refuse("places come least first".into());
    }
    let size = ideal.iter().fold(0.0, |m: f64, x| m.max(x.abs())) + pitch * ideal.len() as f64;
    if size * GAP_ROUNDING >= pitch {
        return refuse("a pitch fits where the places are under 2^50 times it".into());
    }
    // Each block: the sum of its members' places less their spacing, and how many they are.
    let mut blocks: Vec<(f64, usize)> = Vec::with_capacity(ideal.len());
    for (i, x) in ideal.iter().enumerate() {
        blocks.push((x - pitch * i as f64, 1));
        while let [.., (a, m), (b, k)] = blocks[..] {
            if a / m as f64 <= b / k as f64 {
                break;
            }
            blocks.truncate(blocks.len() - 2);
            blocks.push((a + b, m + k));
        }
    }
    let shifted = blocks.into_iter().flat_map(|(sum, k)| std::iter::repeat_n(sum / k as f64, k));
    Ok(shifted.enumerate().map(|(i, place)| place + pitch * i as f64).collect())
}

#[cfg(test)]
#[path = "line_maths_tests.rs"]
mod tests;
