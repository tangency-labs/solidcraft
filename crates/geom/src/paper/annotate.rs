//! Marks that point at things on a sheet (#4890 W10.5, D1a): terminators and leaders, as plain
//! strokes and fills in sheet millimetres. What a mark points at and says is its owner's (dimensions,
//! notes, balloons); this only places it. Dimension layout is W19c's (#4896), so it lives in one place,
//! and the datum triangle is the datum symbol's (#7257).
//!
//! - Every size is the caller's, in paper millimetres; angles are radians. ISO 128-2 clause 9 gives a
//!   leader's dot as [`DOT_PER_LINE_WIDTH`] times its line width and its landing as
//!   [`LANDING_PER_LINE_WIDTH`] times it; ISO 129-1 gives an arrowhead's included angle from 15° to
//!   90° ([`ARROW_ANGLES`]) and the oblique stroke at 45°. An arrowhead's and an oblique stroke's
//!   lengths are left to the caller: what we've read of the standards doesn't give them.
//! - A closed or filled arrowhead's line stops at its base, so the line never shows beside its tip.
//! - Anything outside these rules is refused, never drawn differently.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
use std::ops::RangeInclusive;

use super::{Invalid, Item, MAX_COORDINATE_MM, MAX_SIDE_MM, Result, require};
use crate::{Loop2, Region2, Seg2, Vec2};

/// A leader's dot on a surface, in line widths across (ISO 128-2 clause 9).
pub const DOT_PER_LINE_WIDTH: f64 = 5.0;
/// A landing (reference) line without text, in line widths long (ISO 128-2 clause 9).
pub const LANDING_PER_LINE_WIDTH: f64 = 20.0;
/// An arrowhead's included angle: 15° to 90° (ISO 129-1).
pub const ARROW_ANGLES: RangeInclusive<f64> = PI / 12.0..=FRAC_PI_2;
/// The most points a leader may have.
pub const MAX_LEADER_POINTS: usize = 256;

/// How an arrowhead is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowShape {
    /// Its two sides only.
    Open,
    /// Its outline.
    Closed,
    Filled,
}

/// What ends a line at the point it marks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Terminator {
    /// An arrowhead `length_mm` from tip to base, its sides `angle` apart.
    Arrow { shape: ArrowShape, length_mm: f64, angle: f64 },
    /// A stroke `length_mm` long centred on the point, 45° anticlockwise from the line.
    Oblique { length_mm: f64 },
    /// A filled circle `diameter_mm` across, centred on the point.
    Dot { diameter_mm: f64 },
    /// Nothing: the line ends on another line.
    None,
}

/// A leader: a line through `points`, a landing `landing_mm` long from the last one in the reading
/// direction (none without), and its terminator at the first, all its strokes `width_mm` wide.
#[derive(Clone, Debug, PartialEq)]
pub struct Leader {
    pub points: Vec<Vec2>,
    pub landing_mm: Option<f64>,
    pub terminator: Terminator,
    pub width_mm: f64,
}

/// A size of a mark: finite, above 0 and no larger than a sheet.
fn size(v: f64) -> bool {
    v.is_finite() && v > 0.0 && v <= MAX_SIDE_MM
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

impl Terminator {
    /// The terminator at `at` on a line coming from `from`, its strokes `width_mm` wide, and where
    /// the line stops: a closed or filled arrowhead's base, else `at`.
    pub fn place(self, at: Vec2, from: Vec2, width_mm: f64) -> Result<(Vec<Item>, Vec2)> {
        let along = (at - from).normalized().ok_or(Invalid("a terminator's line has a direction"))?;
        let stroke = |path, closed| Item::Stroke { path, closed, width_mm };
        Ok(match self {
            Terminator::Arrow { shape, length_mm, angle } => {
                require(size(length_mm), "an arrowhead is above 0 and at most 5000 mm long")?;
                require(ARROW_ANGLES.contains(&angle), "an arrowhead's included angle is 15° to 90°")?;
                let base = at - along * length_mm;
                let side = along.perp() * (length_mm * (angle / 2.0).tan());
                // Anticlockwise: the tip, the base's left corner looking along the line, its right.
                let (left, right) = (base + side, base - side);
                match shape {
                    ArrowShape::Open => (vec![stroke(vec![line(left, at), line(at, right)], false)], at),
                    ArrowShape::Closed => (vec![stroke(Loop2::polygon(&[at, left, right]).segs, true)], base),
                    ArrowShape::Filled => (vec![fill(Loop2::polygon(&[at, left, right]))], base),
                }
            }
            Terminator::Oblique { length_mm } => {
                require(size(length_mm), "an oblique stroke is above 0 and at most 5000 mm long")?;
                let half = Vec2::from_angle(along.angle() + FRAC_PI_4) * (length_mm / 2.0);
                (vec![stroke(vec![line(at - half, at + half)], false)], at)
            }
            Terminator::Dot { diameter_mm } => {
                require(size(diameter_mm), "a dot is above 0 and at most 5000 mm across")?;
                (vec![fill(Loop2::circle(at, diameter_mm / 2.0))], at)
            }
            Terminator::None => (Vec::new(), at),
        })
    }
}

fn fill(outer: Loop2) -> Item {
    Item::Fill { region: Region2 { outer, holes: vec![] } }
}

impl Leader {
    /// The leader's line, then its terminator over it.
    pub fn items(&self) -> Result<Vec<Item>> {
        let points = &self.points;
        require((2..=MAX_LEADER_POINTS).contains(&points.len()), "a leader has 2 to 256 points")?;
        let on_paper = |p: &Vec2| p.x.abs() <= MAX_COORDINATE_MM && p.y.abs() <= MAX_COORDINATE_MM;
        require(points.iter().all(on_paper), "a leader's points lie within the paper's coordinates")?;
        require(points.iter().zip(points.iter().skip(1)).all(|(a, b)| a != b), "no two points in a row of a leader are the same")?;
        let (Some(&first), Some(&second), Some(&last), Some(&before)) = (points.first(), points.get(1), points.last(), points.iter().rev().nth(1))
        else {
            return Err(Invalid("a leader has 2 to 256 points"));
        };
        let (mut marks, stop) = self.terminator.place(first, second, self.width_mm)?;
        require(first.dist(stop) < first.dist(second), "a leader's first piece is longer than its arrowhead")?;
        // From where the terminator stops the line, through every point after the first.
        let rest = points.iter().skip(1).copied();
        let mut path: Vec<Seg2> = std::iter::once(stop).chain(rest.clone()).zip(rest).map(|(a, b)| line(a, b)).collect();
        if let Some(landing) = self.landing_mm {
            require(size(landing), "a landing is above 0 and at most 5000 mm long")?;
            // The reading direction away from the leader: to the right unless it arrives going left.
            let way = if last.x < before.x { -landing } else { landing };
            path.push(line(last, last + Vec2::new(way, 0.0)));
        }
        let mut items = vec![Item::Stroke { path, closed: false, width_mm: self.width_mm }];
        items.append(&mut marks);
        Ok(items)
    }
}

#[cfg(test)]
#[path = "annotate_tests.rs"]
mod tests;
