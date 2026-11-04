//! A drawing's dimensions on paper (#4896 W19g, #4894 WE6): one group for each dimension, its
//! extension and dimension lines and its arrowheads, laid out by [`dim_layout_sized`] in paper
//! millimetres, so a dimension is the same size at any view scale. [`paper_group`] lays one out
//! centred on its view, as a view's lines are, so moving the view moves it unchanged.
//!
//! Its text joins once the paper scene has text (B1). Until then the text only decides whether the
//! arrows fit inside, from an estimate of its length.

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use solidcore_record::{Seg2, Vec2};
use solidcraft_doc::drawing::dimension::{Dimension, DimensionKind, Resolved, TextPlace};
use solidcraft_geom::drawing_measure::LinearDirection;
use solidcraft_geom::drawing_view::ViewFrame;
use solidcraft_geom::paper::annotate::{ArrowShape, Terminator};
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{DimFrame, DimSizes, dim_layout_sized};

/// A dimension's sizes on paper, with its text above an unbroken line (ISO 129-1).
const SIZES: DimSizes = DimSizes { arrow: 3.0, gap: 1.0, overshoot: 2.0, offset: 10.0, above: true };
/// A dimension's lines, half as wide as a view's visible ones (ISO 128).
const LINE_MM: f64 = 0.25;
/// A filled arrowhead, its line stopped at its base by [`stop_at_base`].
const ARROW: Terminator = Terminator::Arrow { shape: ArrowShape::Filled, length_mm: SIZES.arrow, angle: PI / 9.0 };
/// The height of a dimension's text, for its estimated size.
const TEXT_MM: f64 = 3.5;
/// How close two points of a layout are when they are one point.
const SAME_POINT_MM: f64 = 1e-6;

/// One laid-out piece of a dimension: a chain has one for each of its members.
struct Piece {
    frame: DimFrame,
    /// Where the layout puts its text, from the frame's own origin.
    at: Vec2,
    /// Half its text's size.
    half: Vec2,
}

/// `dimension`, read as `resolved`, on the view through `frame` (centred on the view): its group,
/// or `None` when what it measures has no place to draw on the view: it wasn't found again, or it
/// is a single point there.
pub fn paper_group(dimension: &Dimension, resolved: &Resolved, frame: &ViewFrame) -> Option<Group> {
    let on: Vec<Vec2> = resolved.points.iter().map(|p| frame.to_paper(*p)).collect();
    let mut lines: Vec<Vec<Vec2>> = Vec::new();
    let mut arrows = Vec::new();
    for piece in pieces(dimension, resolved, &on)? {
        let layout = dim_layout_sized(&piece.frame, Some(piece.at), &SIZES, piece.half, [None, None]);
        // A chain's neighbours share an extension line.
        for line in layout.lines {
            if !lines.iter().any(|other| same_line(other, &line)) {
                lines.push(line);
            }
        }
        arrows.extend(layout.arrows);
    }
    for (tip, dir) in &arrows {
        lines.iter_mut().for_each(|line| stop_at_base(line, *tip, *dir));
    }
    let mut items: Vec<Item> = lines.iter().filter_map(|line| stroke(line)).collect();
    for (tip, dir) in &arrows {
        items.extend(ARROW.place(*tip, *tip - *dir, LINE_MM).ok()?.0);
    }
    Some(Group { object: dimension.id, kind: ObjectKind::Dimension, items })
}

/// What `dimension` lays out, from what it measures on paper (`on`, as [`Resolved::points`] orders
/// them).
fn pieces(dimension: &Dimension, resolved: &Resolved, on: &[Vec2]) -> Option<Vec<Piece>> {
    let (offset, place) = (dimension.offset_mm, &dimension.text);
    let half = half_text(resolved.text.as_deref());
    match (&dimension.kind, on) {
        (DimensionKind::Linear { direction, .. } | DimensionKind::Length { direction, .. }, &[a, b]) => {
            let dir = line_direction(*direction, b - a)?;
            Some(vec![linear(a, b, dir, offset, place, half)])
        }
        (DimensionKind::Angle { other_side, .. }, &[a0, a1, b0, b1]) => Some(vec![angular([a0, a1], [b0, b1], *other_side, offset, place, half)?]),
        (DimensionKind::Radial { diameter, .. }, &[centre, start, mid, end]) => {
            Some(vec![radial(centre, [start, mid, end], *diameter, offset, place, half)?])
        }
        (DimensionKind::Chain { direction, .. }, points) => chain(points, *direction, offset, resolved),
        (DimensionKind::Cylinder { .. }, &[a0, a1, b0, b1]) => across([a0, a1], [b0, b1], LinearDirection::Aligned, offset, place, half),
        (DimensionKind::Silhouettes { direction, .. } | DimensionKind::Faces { direction, .. }, &[a0, a1, b0, b1]) => {
            across([a0, a1], [b0, b1], *direction, offset, place, half)
        }
        _ => None,
    }
}

/// The unit direction a linear dimension's line runs in, on paper. Turned to run right, or up when
/// it is upright, so a positive offset is above it or to its left whichever way round its points are.
fn line_direction(direction: LinearDirection, seen: Vec2) -> Option<Vec2> {
    let dir = match direction {
        LinearDirection::Horizontal => Vec2::X,
        LinearDirection::Vertical => Vec2::new(0.0, 1.0),
        LinearDirection::Aligned => seen.normalized()?,
        // A view's paper map only scales and shifts, so its plane's directions are paper's.
        LinearDirection::Along(along) => along.normalized()?,
    };
    let forward = if dir.x.abs() > 1e-9 { dir.x > 0.0 } else { dir.y > 0.0 };
    Some(if forward { dir } else { -dir })
}

/// A line from `a` to `b` along `dir`, `across` from their middle.
fn linear(a: Vec2, b: Vec2, dir: Vec2, across: f64, place: &TextPlace, half: Vec2) -> Piece {
    let along = at_of(place).map_or(0.0, |at| (at - (a + b) * 0.5).dot(dir));
    Piece { frame: DimFrame::Linear { p0: a, p1: b, dir }, at: Vec2::new(along, across), half }
}

/// A chain's members, each a line from one point to the next, all on one line `offset` from the
/// middle of the chain's two ends.
fn chain(points: &[Vec2], direction: LinearDirection, offset: f64, resolved: &Resolved) -> Option<Vec<Piece>> {
    let (first, last) = (*points.first()?, *points.last()?);
    let dir = line_direction(direction, last - first)?;
    let n = dir.perp();
    let line = ((first + last) * 0.5).dot(n) + offset;
    let member = |(i, (a, b)): (usize, (&Vec2, &Vec2))| {
        let text = resolved.members.get(i).map(|m| m.text.as_str());
        linear(*a, *b, dir, line - ((*a + *b) * 0.5).dot(n), &TextPlace::Centred, half_text(text))
    };
    Some(points.iter().zip(points.iter().skip(1)).enumerate().map(member).collect())
}

/// A line between two silhouettes or faces, each from one end to the other, at the ends on the side `offset`
/// is on, so a positive one clears what it measures. `Aligned` runs straight across them.
fn across(a: [Vec2; 2], b: [Vec2; 2], direction: LinearDirection, offset: f64, place: &TextPlace, half: Vec2) -> Option<Vec<Piece>> {
    let [a0, a1] = a;
    let square = (a1 - a0).normalized()?.perp();
    let dir = line_direction(direction, square)?;
    let n = dir.perp();
    let end = |[p, q]: [Vec2; 2]| if (p.dot(n) >= q.dot(n)) == (offset >= 0.0) { p } else { q };
    Some(vec![linear(end(a), end(b), dir, offset, place, half)])
}

/// The angle between two lines (each from one end to the other), at the point they meet on the view,
/// `offset` out from it. `None` when they are parallel there, as they then never meet.
fn angular(a: [Vec2; 2], b: [Vec2; 2], other_side: bool, offset: f64, place: &TextPlace, half: Vec2) -> Option<Piece> {
    let [a0, a1] = a;
    let [b0, b1] = b;
    let (along_a, along_b) = ((a1 - a0).normalized()?, (b1 - b0).normalized()?);
    let turn = along_a.perp().dot(along_b);
    if turn.abs() < 1e-9 {
        return None;
    }
    let vertex = a0 + along_a * ((b0 - a0).perp().dot(along_b) / turn);
    let ray = |p: Vec2, q: Vec2| (if p.dist(vertex) >= q.dist(vertex) { p } else { q } - vertex).normalized();
    let (from_a, from_b) = (ray(a0, a1)?, ray(b0, b1)?);
    // The smaller angle between the lines, or with `other_side` the larger: the rays' own, or one ray
    // turned round to make it so.
    let from_b = if (from_a.dot(from_b) < 0.0) != other_side { -from_b } else { from_b };
    let cross = from_a.perp().dot(from_b);
    let d0 = if cross >= 0.0 { from_a } else { from_b };
    let sweep = cross.abs().atan2(from_a.dot(from_b));
    let middle = Vec2::from_angle(d0.angle() + sweep * 0.5);
    let towards = at_of(place).and_then(|at| (at - vertex).normalized()).unwrap_or(middle);
    Some(Piece { frame: DimFrame::Angular { vertex, d0, sweep }, at: towards * offset.abs(), half })
}

/// A circle's or an arc's radius or diameter, from its centre and the arc's start, middle and end,
/// with its leader `offset` past the rim, towards its text's place or else the arc's middle.
fn radial(centre: Vec2, arc: [Vec2; 3], diameter: bool, offset: f64, place: &TextPlace, half: Vec2) -> Option<Piece> {
    let [start, mid, end] = arc;
    let radius = centre.dist(start);
    let whole = start.dist(end) <= SAME_POINT_MM;
    let turned = if whole {
        None
    } else {
        let from = (start - centre).angle();
        let sweep = ((end - centre).angle() - from).rem_euclid(TAU);
        // Counter-clockwise from its start, unless its middle says it runs the other way.
        Some(if ((mid - centre).angle() - from).rem_euclid(TAU) <= sweep { (from, sweep) } else { ((end - centre).angle(), TAU - sweep) })
    };
    let default = if whole { Some(Vec2::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2)) } else { (mid - centre).normalized() };
    let towards = at_of(place).and_then(|at| (at - centre).normalized()).or(default)?;
    Some(Piece { frame: DimFrame::Radial { center: centre, radius, diameter, arc: turned }, at: towards * (radius + offset).max(0.0), half })
}

/// A text's place on paper, when it has its own.
fn at_of(place: &TextPlace) -> Option<Vec2> {
    match place {
        TextPlace::At([x, y]) => Some(Vec2::new(*x, *y)),
        TextPlace::Centred => None,
    }
}

/// Half a text's size, estimated from its length until text is measured.
fn half_text(text: Option<&str>) -> Vec2 {
    let characters = text.map_or(0, |t| t.chars().count());
    Vec2::new(0.3 * TEXT_MM * characters as f64, TEXT_MM * 0.5)
}

fn same_line(a: &[Vec2], b: &[Vec2]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.dist(*q) <= SAME_POINT_MM)
}

/// Ends `line` at the base of the arrowhead on its end, if it runs into one there, so no line shows
/// beside the arrowhead's tip. A line running on through a tip, to a tail, stays whole, and so does
/// an extension line that only starts where the arrow is, as its offset is so small.
fn stop_at_base(line: &mut Vec<Vec2>, tip: Vec2, dir: Vec2) {
    let runs_in = |ends: &mut dyn Iterator<Item = &Vec2>| {
        let end = ends.next().is_some_and(|p| p.dist(tip) <= SAME_POINT_MM);
        end && ends.next().and_then(|p| (tip - *p).normalized()).is_some_and(|u| u.dot(dir) > 0.5)
    };
    if runs_in(&mut line.iter().rev()) {
        // An arc's points under the arrowhead go too.
        while line.last().is_some_and(|p| p.dist(tip) < SIZES.arrow) {
            line.pop();
        }
        line.push(tip - dir * SIZES.arrow);
    } else if runs_in(&mut line.iter()) {
        line.reverse();
        stop_at_base(line, tip, dir);
    }
}

/// `line` as a stroke, or nothing when it has no length.
fn stroke(line: &[Vec2]) -> Option<Item> {
    let pieces = line.iter().zip(line.iter().skip(1)).filter(|(a, b)| a != b);
    let path: Vec<Seg2> = pieces.map(|(a, b)| Seg2::Line { a: *a, b: *b }).collect();
    (!path.is_empty()).then_some(Item::Stroke { path, closed: false, width_mm: LINE_MM, pattern: None })
}

#[cfg(test)]
#[path = "drawing_dimension_paper_tests.rs"]
mod tests;
