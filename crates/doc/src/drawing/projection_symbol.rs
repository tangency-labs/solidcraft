//! The projection symbol a title block's cell holds (ISO 5456-2, figures A.1 and A.2), drawn on
//! paper in the drawing's projection. The block's layout says which cells hold it and at what size
//! ([`solidcraft_geom::title_block`]); this draws it there, beside the block's lines
//! ([`title_block_paper`]).
//!
//! - The symbol is a truncated cone and the cone seen end on, `h` apart, with their outlines `h / 10`
//!   wide: the end view on the right in first angle, on the left in third.
//! - Its two centre lines are chain lines (ISO 128-20's long-dashed dotted line) half as wide: a long
//!   dash of 24 line widths, a gap of 3, a dot and a gap of 3. They run `h / 4` past the outlines
//!   and cross on a long dash. Its dot is one line width long: under the round caps
//!   ([`solidcraft_geom::paper::Dashes::under_round_caps`]) a round dot one width across.

use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_geom::cell::Projection;
use solidcraft_geom::paper::{Dashes, Group, Item};
use solidcraft_geom::title_block::TitleBlockLayout;

/// The block as one paper group of `object` (its sheet): its lines ([`TitleBlockLayout::paper`]),
/// then the projection symbol in `projection` centred in each cell that holds it. Every title block
/// on a sheet is drawn through here.
pub fn title_block_paper(layout: &TitleBlockLayout, object: u64, projection: Projection) -> Group {
    let mut group = layout.paper(object);
    for cell in &layout.cells {
        if let Some(h) = cell.symbol_mm {
            group.items.extend(projection_symbol(projection, h, cell.value_box.center()));
        }
    }
    group
}

/// The projection symbol in `projection`, `h` across the cone's smaller end and centred on `centre`:
/// a truncated cone `2 h` long, from its `h` end on the left to its `2 h` end, and the cone seen end
/// on, circles `2 h` and `h` across, `h` from it, so the two read apart: on the right in first angle,
/// on the left in third. Its centre lines come first, so the outlines are drawn over them: the axis
/// through the cone and the circles, and the line across the circles' centre, each running `h / 4`
/// past the outlines and crossing the other on a long dash.
pub fn projection_symbol(projection: Projection, h: f64, centre: Vec2) -> [Item; 5] {
    let (gap, past) = (h, h / 4.0);
    let left = centre.x - (4.0 * h + gap) / 2.0;
    let (cone_at, circles_at) = match projection {
        Projection::First => (left, left + 3.0 * h + gap),
        Projection::Third => (left + 2.0 * h + gap, left + h),
    };
    let y = centre.y;
    let cone = [(cone_at, y - h / 2.0), (cone_at + 2.0 * h, y - h), (cone_at + 2.0 * h, y + h), (cone_at, y + h / 2.0)];
    let cone = Loop2::polygon(&cone.map(|(x, y)| Vec2::new(x, y)));
    let circles = Vec2::new(circles_at, y);
    let stroke = |path: Loop2| Item::Stroke { path: path.segs, closed: true, width_mm: h / 10.0, pattern: None };
    // A centre line from `a` to `b`, its long dash centred `at` along it, where it crosses the other.
    let centre_line = |a: Vec2, b: Vec2, at: f64| {
        let pattern = Some(chain(h / 20.0, at));
        Item::Stroke { path: vec![Seg2::Line { a, b }], closed: false, width_mm: h / 20.0, pattern }
    };
    // Both lines run level or upright through the circles' centre, so where they cross is a sum.
    let axis = centre_line(Vec2::new(left - past, y), Vec2::new(left + 4.0 * h + gap + past, y), circles_at - (left - past));
    let across = centre_line(Vec2::new(circles_at, y - h - past), Vec2::new(circles_at, y + h + past), h + past);
    // Two half circles, as the symbol has always been drawn: `Loop2::circle` is one whole arc now.
    let circle = |radius: f64| {
        let half = |start: f64| Seg2::Arc { center: circles, radius, start, sweep: std::f64::consts::PI };
        Loop2 { segs: vec![half(0.0), half(std::f64::consts::PI)] }
    };
    [axis, across, stroke(cone), stroke(circle(h)), stroke(circle(h / 2.0))]
}

/// The pattern of a chain line `width_mm` wide, with the middle of a long dash `at_mm` along its
/// path: where centre lines cross, they cross on long dashes.
fn chain(width_mm: f64, at_mm: f64) -> Dashes {
    let d = width_mm;
    let long = 24.0 * d;
    let period = long + 7.0 * d;
    let phase_mm = (long / 2.0 - at_mm).rem_euclid(period);
    // rem_euclid can round up to the period itself, which a pattern's phase never reaches.
    Dashes { lengths_mm: vec![long, 3.0 * d, d, 3.0 * d], phase_mm: if phase_mm < period { phase_mm } else { 0.0 } }
}

#[cfg(test)]
#[path = "projection_symbol_tests.rs"]
mod tests;
