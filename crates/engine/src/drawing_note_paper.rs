//! A drawing's notes on paper (#4896 W19i): one group for each note, its text centred on its place,
//! at its own cap height or a dimension's, set in the drawing's lettering by the dimension's text
//! rule ([`Text::set`]), with its own line breaks only. A note that points at the model has a leader
//! from where each thing it points at is now to its text, ending in a dimension's arrowhead, with a
//! landing on the side of the text it comes from: drawn once on each side, where the other leaders
//! from that side meet it. A lost reference has no leader; the text stays where it is. A note on a view is centred on the view, as the view's lines are, and the scene moves it to
//! the view's centre; one on a sheet sits from the sheet's corner.

use solidcore_record::Vec2;
use solidcraft_doc::drawing::note::Note;
use solidcraft_doc::drawing::sheet_format::FormatText;
use solidcraft_geom::paper::annotate::{LANDING_PER_LINE_WIDTH, Leader};
use solidcraft_geom::paper::text::{Font, TextRun};
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::PaperFonts;

use crate::drawing_dimension_paper::{ARROW, Layout, OnPaper, TEXT_MM, Text, placed};

/// The gap between a note's text and its leader's end.
const GAP_MM: f64 = 1.0;

/// `note`, reading `text` (its content with its fields read), at its place (from its sheet's corner,
/// or from its view's centre as its view's lines are), in lines `line_mm` wide, a leader ending at
/// each of `tips`, the ends it points at that are found: its group.
pub fn note_group(note: &Note, text: &str, tips: &[Vec2], line_mm: f64, lettering: &PaperFonts) -> OnPaper {
    let centre = Vec2::new(note.at[0], note.at[1]);
    let cap_mm = note.cap_mm.unwrap_or(TEXT_MM);
    let text = Text::set(Some(text), cap_mm, lettering);
    let mut why = text.why;
    let mut items = Vec::new();
    // Whether a leader has drawn the landing on the text's left and on its right.
    let mut landed = [false; 2];
    for tip in tips {
        match leader(*tip, centre, text.half, line_mm, &mut landed) {
            Some(leader) => items.extend(leader),
            None => why = why.or_else(|| Some("its leader ends too near its text to draw".to_string())),
        }
    }
    items.extend(placed(text.runs, text.half, cap_mm, centre, Vec2::X));
    OnPaper {
        group: Group { object: note.id, kind: ObjectKind::Note, items },
        fonts: text.fonts,
        why,
        pending: text.pending,
        layout: Layout::default(),
    }
}

/// A sheet format's fixed texts ([`FormatText`]), each set in `lettering` at its own height with
/// its baseline starting at its place, turned as it says: their runs, the fonts they use, and whether
/// one waits for a font to fall back to. One that can't be set is left out.
pub fn format_texts(texts: &[FormatText], lettering: &PaperFonts) -> (Vec<TextRun>, Vec<Font>, bool) {
    let (mut runs, mut fonts, mut pending) = (Vec::new(), Vec::new(), false);
    for one in texts {
        let set = Text::set(Some(&one.text), one.cap_mm, lettering);
        pending |= set.pending;
        let dir = Vec2::new(one.angle_deg.to_radians().cos(), one.angle_deg.to_radians().sin());
        let centre = Vec2::new(one.at[0], one.at[1]) + dir * set.half.x - dir.perp() * (set.half.y - one.cap_mm);
        runs.extend(placed(set.runs, set.half, one.cap_mm, centre, dir).filter_map(|item| match item {
            Item::Text(run) => Some(run),
            _ => None,
        }));
        fonts.extend(set.fonts);
    }
    (runs, fonts, pending)
}

/// A leader from `tip` to the text `half` in size round `centre`. From beside the text it ends with a
/// landing on the side it comes from, unless one is drawn there (`landed`, left then right): then it
/// ends where that landing starts. From over or under it, where a landing would run back through it,
/// straight to the nearer side. None when it is too short for its arrowhead.
fn leader(tip: Vec2, centre: Vec2, half: Vec2, line_mm: f64, landed: &mut [bool; 2]) -> Option<Vec<Item>> {
    let landing = LANDING_PER_LINE_WIDTH * line_mm;
    let beside = half.x + GAP_MM;
    let (left, right) = (centre.x - beside, centre.x + beside);
    let (end, landing_mm) = if tip.x < left - landing {
        (left - landing, Some(landing))
    } else if tip.x > right + landing {
        (right + landing, Some(landing))
    } else {
        (if tip.x < centre.x { left } else { right }, None)
    };
    let side = landed.get_mut(usize::from(tip.x > centre.x))?;
    let landing_mm = landing_mm.filter(|_| !*side);
    let items = Leader { points: vec![tip, Vec2::new(end, centre.y)], landing_mm, terminator: ARROW, width_mm: line_mm }.items().ok()?;
    *side |= landing_mm.is_some();
    Some(items)
}

#[cfg(test)]
#[path = "drawing_note_paper_tests.rs"]
mod tests;
