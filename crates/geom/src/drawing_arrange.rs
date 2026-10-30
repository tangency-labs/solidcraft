//! Arranging annotations on a sheet (W41a, #9702, part of #860): the moves that align a set of
//! boxes to one line, distribute them evenly, or space them at a fixed gap. Each annotation is its
//! box in paper millimetres (y up); what the box holds (a note, a balloon, a symbol) and where its
//! new place is saved belong to the annotation's owner. This only works out how far each box moves.
//!
//! - Every function returns one move per box, in the order given, along one axis only: the other
//!   coordinate of every move is exactly 0.
//! - A box already where it belongs moves by exactly 0, so arranging twice moves nothing.
//! - Boxes in the same place keep the order they were given in, so the result never depends on a
//!   sort's whim.
//! - Boxes are finite, not empty (a box of zero width or height is a point or a line, and allowed)
//!   and within the paper's coordinates; at most [`MAX_BOXES`] of them. A move that would take a box
//!   outside the paper's coordinates is refused, never clamped.

use serde::{Deserialize, Serialize};

use crate::paper::{MAX_COORDINATE_MM, MAX_SIDE_MM};
use crate::{Aabb2, Vec2};

/// The most boxes one arrangement may hold.
pub const MAX_BOXES: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("arrange: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

/// A direction on the sheet: x to the right, y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
}

/// The line boxes are aligned on: one of their edges, or their centres.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    /// Left edges share one x.
    Left,
    /// Centres share one x (a vertical line through them).
    CentreX,
    /// Right edges share one x.
    Right,
    /// Bottom edges share one y.
    Bottom,
    /// Centres share one y (a horizontal line through them).
    CentreY,
    /// Top edges share one y.
    Top,
}

impl Line {
    /// The axis boxes move along to reach the line.
    pub fn axis(self) -> Axis {
        match self {
            Line::Left | Line::CentreX | Line::Right => Axis::X,
            Line::Bottom | Line::CentreY | Line::Top => Axis::Y,
        }
    }

    /// Where `b` meets the line's kind: its edge or its centre, along the line's axis.
    fn of(self, b: &Aabb2) -> f64 {
        match self {
            Line::Left => b.min.x,
            Line::CentreX => (b.min.x + b.max.x) / 2.0,
            Line::Right => b.max.x,
            Line::Bottom => b.min.y,
            Line::CentreY => (b.min.y + b.max.y) / 2.0,
            Line::Top => b.max.y,
        }
    }
}

/// Where the line boxes are aligned on lies.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    /// The set's own: its outermost edge on that side (the leftmost left edge, the topmost top edge,
    /// …), or the centre of the box around them all.
    Selection,
    /// The box at this index in the set, which stays where it is.
    Item(usize),
    /// This coordinate, in paper millimetres along the line's axis.
    At(f64),
}

/// How [`distribute`] spaces boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spread {
    /// The same gap between each box and the next (a negative gap when they overlap in all).
    Gaps,
    /// The same distance between each box's centre and the next one's.
    Centres,
}

fn low(b: &Aabb2, axis: Axis) -> f64 {
    match axis {
        Axis::X => b.min.x,
        Axis::Y => b.min.y,
    }
}

fn high(b: &Aabb2, axis: Axis) -> f64 {
    match axis {
        Axis::X => b.max.x,
        Axis::Y => b.max.y,
    }
}

fn centre(b: &Aabb2, axis: Axis) -> f64 {
    (low(b, axis) + high(b, axis)) / 2.0
}

fn on_paper(v: f64) -> bool {
    v.is_finite() && v.abs() <= MAX_COORDINATE_MM
}

fn check(boxes: &[Aabb2]) -> Result<()> {
    require(boxes.len() <= MAX_BOXES, "at most 10,000 boxes are arranged at once")?;
    for b in boxes {
        require([b.min.x, b.min.y, b.max.x, b.max.y].into_iter().all(on_paper), "a box lies within the paper's coordinates")?;
        require(!b.is_empty(), "a box's low corner is at or below and left of its high corner")?;
    }
    Ok(())
}

/// The move `by` along `axis`, refused when it would take `b` off the paper's coordinates.
fn shift(b: &Aabb2, axis: Axis, by: f64) -> Result<Vec2> {
    let fits = on_paper(low(b, axis) + by) && on_paper(high(b, axis) + by);
    require(by.is_finite() && fits, "a box would move outside the paper's coordinates")?;
    Ok(match axis {
        Axis::X => Vec2::new(by, 0.0),
        Axis::Y => Vec2::new(0.0, by),
    })
}

/// The boxes' indices ordered by `key`, those with equal keys in the order given.
fn order(boxes: &[Aabb2], key: impl Fn(&Aabb2) -> f64) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..boxes.len()).collect();
    // Keys are finite (checked); -0 and 0 compare equal, and the sort is stable.
    indices.sort_by(|&a, &b| {
        let (ka, kb) = (boxes.get(a).map(&key), boxes.get(b).map(&key));
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    indices
}

/// The moves that put every box's `line` (edge or centre) on one line, found from `to`.
pub fn align(boxes: &[Aabb2], line: Line, to: Reference) -> Result<Vec<Vec2>> {
    check(boxes)?;
    let target = match to {
        Reference::Selection => {
            let all = boxes.iter().fold(Aabb2::EMPTY, |all, b| all.union(b));
            if all.is_empty() {
                return Ok(Vec::new());
            }
            line.of(&all)
        }
        Reference::Item(i) => line.of(boxes.get(i).ok_or(Invalid("the box to align to is one of the set"))?),
        Reference::At(at) => {
            require(on_paper(at), "the line to align to lies within the paper's coordinates")?;
            at
        }
    };
    boxes.iter().map(|b| shift(b, line.axis(), target - line.of(b))).collect()
}

/// The moves that space boxes evenly along `axis`, in the order of their centres. The first and the
/// last stay where they are; with fewer than three boxes nothing moves.
pub fn distribute(boxes: &[Aabb2], axis: Axis, spread: Spread) -> Result<Vec<Vec2>> {
    check(boxes)?;
    let mut moves = vec![Vec2::ZERO; boxes.len()];
    let order = order(boxes, |b| centre(b, axis));
    let (Some(&first), Some(&last)) = (order.first(), order.last()) else {
        return Ok(moves);
    };
    let (Some(first), Some(last)) = (boxes.get(first), boxes.get(last)) else {
        return Ok(moves);
    };
    if order.len() < 3 {
        return Ok(moves);
    }
    let steps = (order.len() - 1) as f64;
    // The middle boxes only: the outermost two keep their places, so they move by exactly 0.
    let middle = order.iter().enumerate().skip(1).take(order.len() - 2);
    match spread {
        Spread::Centres => {
            let (from, to) = (centre(first, axis), centre(last, axis));
            for (k, &i) in middle {
                let (Some(b), Some(m)) = (boxes.get(i), moves.get_mut(i)) else { continue };
                // Interpolated rather than accumulated, so no error builds up along a long row.
                let wanted = from + (to - from) * (k as f64 / steps);
                *m = shift(b, axis, wanted - centre(b, axis))?;
            }
        }
        Spread::Gaps => {
            let sizes: f64 = order.iter().filter_map(|&i| boxes.get(i)).map(|b| high(b, axis) - low(b, axis)).sum();
            let gap = (high(last, axis) - low(first, axis) - sizes) / steps;
            let mut next = high(first, axis) + gap;
            for (_, &i) in middle {
                let (Some(b), Some(m)) = (boxes.get(i), moves.get_mut(i)) else { continue };
                *m = shift(b, axis, next - low(b, axis))?;
                next += high(b, axis) - low(b, axis) + gap;
            }
        }
    }
    Ok(moves)
}

/// The moves that stack boxes `gap_mm` apart along `axis`: rightwards from the leftmost box along x,
/// downwards from the topmost box along y (the order a list of notes is read in). The box it starts
/// from stays where it is; the others follow in the order of their left (x) or top (y) edges.
pub fn space(boxes: &[Aabb2], axis: Axis, gap_mm: f64) -> Result<Vec<Vec2>> {
    check(boxes)?;
    require(gap_mm.is_finite() && (0.0..=MAX_SIDE_MM).contains(&gap_mm), "a gap is 0 to 5000 mm")?;
    let mut moves = vec![Vec2::ZERO; boxes.len()];
    match axis {
        Axis::X => {
            let order = order(boxes, |b| b.min.x);
            let mut next = None;
            for i in order {
                let (Some(b), Some(m)) = (boxes.get(i), moves.get_mut(i)) else { continue };
                let by = next.map_or(0.0, |at: f64| at - b.min.x);
                *m = shift(b, Axis::X, by)?;
                next = Some(b.max.x + by + gap_mm);
            }
        }
        Axis::Y => {
            let order = order(boxes, |b| -b.max.y);
            let mut next = None;
            for i in order {
                let (Some(b), Some(m)) = (boxes.get(i), moves.get_mut(i)) else { continue };
                let by = next.map_or(0.0, |at: f64| at - b.max.y);
                *m = shift(b, Axis::Y, by)?;
                next = Some(b.min.y + by - gap_mm);
            }
        }
    }
    Ok(moves)
}

#[cfg(test)]
#[path = "drawing_arrange_tests.rs"]
mod tests;
