//! A drawing's balloons on paper (#3874): one group for each balloon, a circle round its number
//! at its place, with a leader from where its arrow ends now to the circle's rim, ending in a
//! dimension's arrowhead. Its number is set in the drawing's lettering by the dimension's text rule
//! ([`Text::set`]), at a dimension's cap height. The circle is [`RADIUS_MM`] in radius, or more when
//! its number needs it, a gap clear of the number's corners.
//!
//! A lost balloon keeps its circle and number where they are, with no leader, as a lost note keeps
//! its text. One with no row to read keeps an empty circle. A balloon is centred on its view, as the
//! view's lines are, and the scene moves it to the view's centre.

use solidcore_record::{Loop2, Vec2};
use solidcraft_doc::drawing::balloon::Balloon;
use solidcraft_geom::paper::annotate::Leader;
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::PaperFonts;

use crate::drawing_dimension_paper::{ARROW, GAP_MM, Layout, OnPaper, TEXT_MM, Text, placed};

/// A balloon's radius, or more when its number needs it.
const RADIUS_MM: f64 = 5.0;

/// `balloon` showing `item` (none when it has no row to read), at its place from its view's centre,
/// in lines `line_mm` wide, its leader ending at `tip` when its edge was found: its group.
pub fn balloon_group(balloon: &Balloon, item: Option<&str>, tip: Option<Vec2>, line_mm: f64, lettering: &PaperFonts) -> OnPaper {
    let centre = Vec2::new(balloon.at[0], balloon.at[1]);
    let text = Text::set(item, TEXT_MM, lettering);
    // Round the number's corners, a gap clear of them.
    let radius = RADIUS_MM.max(text.half.len() + GAP_MM);
    let mut items = vec![Item::Stroke { path: Loop2::circle(centre, radius).segs, closed: true, width_mm: line_mm, pattern: None }];
    let mut why = text.why;
    if let Some(tip) = tip {
        match leader(tip, centre, radius, line_mm) {
            Some(leader) => items.extend(leader),
            None => why = why.or_else(|| Some("its arrow ends too near its circle to draw".to_string())),
        }
    }
    items.extend(placed(text.runs, text.half, TEXT_MM, centre, Vec2::X));
    let group = Group { object: balloon.id, kind: ObjectKind::Balloon, items };
    OnPaper { group, fonts: text.fonts, why, pending: text.pending, layout: Layout::default() }
}

/// The leader from `tip` to the rim of the circle `radius` round `centre`, along the line to the
/// centre. None when `tip` is inside the circle or nearer its rim than an arrowhead.
fn leader(tip: Vec2, centre: Vec2, radius: f64, line_mm: f64) -> Option<Vec<Item>> {
    if tip.dist(centre) <= radius {
        return None;
    }
    let rim = centre + (tip - centre).normalized()? * radius;
    Leader { points: vec![tip, rim], landing_mm: None, terminator: ARROW, width_mm: line_mm }.items().ok()
}

#[cfg(test)]
#[path = "drawing_balloon_paper_tests.rs"]
mod tests;
