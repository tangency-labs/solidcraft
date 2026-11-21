//! A drawing's revision tags on paper (#3874): one group for each tag, a point-up equilateral
//! triangle round what it shows at its place on the sheet, its place the triangle's centroid. Its
//! text is set in the drawing's lettering by the dimension's text rule ([`Text::set`]), as large as
//! fits a gap inside every edge and at most a dimension's cap height; a large tag centres it on its
//! place. A tag too small for its text keeps its triangle and says the side that holds it.
//!
//! A tag reads no model: the sheet sets it with its own notes, in paper coordinates.

use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_geom::revision::{RevisionRecord, Tag, TagText};
use solidcraft_sketch::PaperFonts;

use crate::drawing_dimension_paper::{GAP_MM, Layout, OnPaper, TEXT_MM, Text, placed, text_box};

/// What `tag` of `record` shows: its row's revision, its own text, or, its row deleted, what it showed.
pub(crate) fn shows(record: &RevisionRecord, tag: &Tag) -> String {
    match record.tag_text(tag) {
        TagText::Revision(text) | TagText::Override(text) | TagText::Lost(text) => text,
    }
}

/// `tag` showing `shows`, in lines `line_mm` wide: its group.
pub fn tag_group(tag: &Tag, shows: &str, line_mm: f64, lettering: &PaperFonts) -> OnPaper {
    let (at, side) = (Vec2::new(tag.at[0], tag.at[1]), tag.size_mm);
    let root3 = 3f64.sqrt();
    let (height, base) = (side * root3 * 0.5, at.y - side / (2.0 * root3));
    let corners = [Vec2::new(at.x, base + height), Vec2::new(at.x - side * 0.5, base), Vec2::new(at.x + side * 0.5, base)];
    let outline = (0..3).map(|i| Seg2::Line { a: corners[i], b: corners[(i + 1) % 3] }).collect();
    let items = vec![Item::Stroke { path: outline, closed: true, width_mm: line_mm, pattern: None }];
    let group = Group { object: tag.id, kind: ObjectKind::RevisionTag, items };
    let measured = Text::set(Some(shows), TEXT_MM, lettering);
    let mut on = OnPaper { group, fonts: Vec::new(), why: measured.why, pending: measured.pending, layout: Layout::default() };
    if measured.runs.is_empty() {
        return on;
    }
    // A text `wide` caps across, a gap over the base, stays a gap from the sloping edges while
    // cap (1 + √3 wide / 2) + 3 gaps <= the height: the largest cap that fits.
    let wide = measured.half.x / measured.half.y;
    let needs = |cap: f64| cap * (1.0 + root3 * wide * 0.5) + 3.0 * GAP_MM;
    let fit = (height - 3.0 * GAP_MM) / (1.0 + root3 * wide * 0.5);
    let least = *solidcraft_doc::drawing::note::CAP_MM.start();
    let cap = fit.min(TEXT_MM);
    if fit.is_nan() || cap < least {
        let holds = (needs(least) * 2.0 / root3 * 10.0).ceil() / 10.0;
        on.why = Some(format!("its side is too short for its text: it needs a side of at least {holds} mm"));
        return on;
    }
    let text = Text::set(Some(shows), cap, lettering);
    // From a gap over the base to the highest the sloping edges allow, as near the centroid as it can.
    let lowest = base + GAP_MM + text.half.y;
    let highest = base + height - 2.0 * GAP_MM - root3 * text.half.x - text.half.y;
    let centre = Vec2::new(at.x, at.y.max(lowest).min(highest));
    on.layout.text_boxes.push(text_box(centre, Vec2::X, text.half));
    on.group.items.extend(placed(text.runs, text.half, cap, centre, Vec2::X));
    (on.fonts, on.why, on.pending) = (text.fonts, text.why, text.pending);
    on
}

#[cfg(test)]
#[path = "drawing_revision_paper_tests.rs"]
mod tests;
