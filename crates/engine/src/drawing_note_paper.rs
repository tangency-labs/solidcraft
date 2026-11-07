//! A drawing's notes on paper (#4896 W19i): one group for each note, its text centred on its place,
//! at its own cap height or a dimension's, set in the drawing's lettering by the dimension's text
//! rule ([`Text::set`]), with its own line breaks only. A note that points at the model has a leader
//! from where that is now to its text, ending in a dimension's arrowhead, with a landing on the side
//! of the text it comes from. A note whose reference is lost keeps its text where it is, with no
//! leader. A note on a view is centred on the view, as the view's lines are, and the scene moves it to
//! the view's centre; one on a sheet sits from the sheet's corner.

use solidcore_record::Vec2;
use solidcraft_doc::drawing::note::Note;
use solidcraft_geom::paper::annotate::{LANDING_PER_LINE_WIDTH, Leader};
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_sketch::PaperFonts;

use crate::drawing_dimension_paper::{ARROW, OnPaper, TEXT_MM, Text, placed};

/// The gap between a note's text and its leader's end.
const GAP_MM: f64 = 1.0;

/// `note`, reading `text` (its content with its fields read), at its place (from its sheet's corner,
/// or from its view's centre as its view's lines are), in lines `line_mm` wide, its leader ending at
/// `tip` when it points at something found: its group.
pub fn note_group(note: &Note, text: &str, tip: Option<Vec2>, line_mm: f64, lettering: &PaperFonts) -> OnPaper {
    let centre = Vec2::new(note.at[0], note.at[1]);
    let cap_mm = note.cap_mm.unwrap_or(TEXT_MM);
    let text = Text::set(Some(text), cap_mm, lettering);
    let mut why = text.why;
    let mut items = Vec::new();
    if let Some(tip) = tip {
        match leader(tip, centre, text.half, line_mm) {
            Some(leader) => items.extend(leader),
            None => why = why.or_else(|| Some("its leader ends too near its text to draw".to_string())),
        }
    }
    items.extend(placed(text.runs, text.half, cap_mm, centre, Vec2::X));
    OnPaper { group: Group { object: note.id, kind: ObjectKind::Note, items }, fonts: text.fonts, why, pending: text.pending }
}

/// A leader from `tip` to the text `half` in size round `centre`. From beside the text it ends with a
/// landing on the side it comes from; from over or under it, where a landing would run back through
/// it, straight to the nearer side. None when it is too short for its arrowhead.
fn leader(tip: Vec2, centre: Vec2, half: Vec2, line_mm: f64) -> Option<Vec<Item>> {
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
    Leader { points: vec![tip, Vec2::new(end, centre.y)], landing_mm, terminator: ARROW, width_mm: line_mm }.items().ok()
}

#[cfg(test)]
#[path = "drawing_note_paper_tests.rs"]
mod tests;
