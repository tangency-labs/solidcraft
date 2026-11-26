//! A sheet's format: line work and fixed text drawn on the paper under its views, as a template
//! imported from another program brings its border and title block (#3875). It is drawn as it is
//! and reads nothing: a field that fills in belongs in the title block. A sheet without one draws
//! only its drawing border and title block, as before.
//!
//! Imported files are hostile, so a format is held to the paper and capped ([`SheetFormat::check`]):
//! every point on paper, every arc whole, every text short and of a size a sheet can print.

use serde::{Deserialize, Serialize};
use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::paper::MAX_SIDE_MM;

use crate::{DocError, Result};

/// The most strokes one sheet's format holds.
pub const MAX_STROKES: usize = 20_000;
/// The most pieces all of one sheet's strokes hold together.
pub const MAX_PIECES: usize = 200_000;
/// The most texts one sheet's format holds.
pub const MAX_TEXTS: usize = 2_000;
/// The most characters in one text.
pub const MAX_TEXT_CHARS: usize = 500;
/// The range of a text's capital height, in paper millimetres.
pub const CAP_MM: std::ops::RangeInclusive<f64> = 0.5..=100.0;
/// The range of a stroke's own width, in paper millimetres.
pub const WIDTH_MM: std::ops::RangeInclusive<f64> = 0.05..=10.0;

/// Line work and fixed text on a sheet, in paper millimetres from its lower-left corner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SheetFormat {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<FormatStroke>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<FormatText>,
}

/// A line along `path`, each piece starting where the one before ends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatStroke {
    pub path: Vec<Seg2>,
    #[serde(default)]
    pub closed: bool,
    /// Its own width; none draws it in the sheet's thin line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f64>,
}

/// A line of text, its baseline starting at `at` and turned `angle_deg` anticlockwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatText {
    pub at: [f64; 2],
    pub text: String,
    pub cap_mm: f64,
    #[serde(default)]
    pub angle_deg: f64,
}

impl SheetFormat {
    /// Whether it draws nothing.
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.texts.is_empty()
    }

    /// Holds it to the paper and to its caps: refused, naming the first thing wrong.
    pub fn check(&self) -> Result<()> {
        let bad = |why: String| Err(DocError::Invalid(format!("its format: {why}")));
        if self.strokes.len() > MAX_STROKES {
            return bad(format!("at most {MAX_STROKES} strokes"));
        }
        if self.texts.len() > MAX_TEXTS {
            return bad(format!("at most {MAX_TEXTS} texts"));
        }
        let pieces = self.strokes.iter().map(|s| s.path.len()).fold(0usize, usize::saturating_add);
        if pieces > MAX_PIECES {
            return bad(format!("at most {MAX_PIECES} pieces of line in all"));
        }
        for (i, stroke) in self.strokes.iter().enumerate() {
            if stroke.path.is_empty() {
                return bad(format!("stroke {i} has no pieces"));
            }
            if !stroke.path.iter().all(on_paper) {
                return bad(format!("stroke {i} lies past {MAX_SIDE_MM} mm, or isn't a line, a whole arc or a curve"));
            }
            if stroke.width_mm.is_some_and(|w| !WIDTH_MM.contains(&w)) {
                return bad(format!("stroke {i}'s width is {} to {} mm", WIDTH_MM.start(), WIDTH_MM.end()));
            }
        }
        for (i, text) in self.texts.iter().enumerate() {
            if text.text.trim().is_empty() || text.text.chars().count() > MAX_TEXT_CHARS || text.text.chars().any(char::is_control) {
                return bad(format!("text {i} is 1 to {MAX_TEXT_CHARS} characters on one line"));
            }
            if !(point(text.at[0], text.at[1]) && CAP_MM.contains(&text.cap_mm) && text.angle_deg.is_finite() && text.angle_deg.abs() <= 360.0) {
                return bad(format!("text {i} sits on paper, {} to {} mm high, turned at most a full turn", CAP_MM.start(), CAP_MM.end()));
            }
        }
        Ok(())
    }
}

fn point(x: f64, y: f64) -> bool {
    x.is_finite() && y.is_finite() && x.abs() <= MAX_SIDE_MM && y.abs() <= MAX_SIDE_MM
}

fn at(v: Vec2) -> bool {
    point(v.x, v.y)
}

/// Whether `seg` lies on paper: its points finite and within the largest sheet, an arc's radius
/// and sweep a real arc's.
fn on_paper(seg: &Seg2) -> bool {
    match *seg {
        Seg2::Line { a, b } => at(a) && at(b),
        Seg2::Arc { center, radius, start, sweep } => {
            at(center)
                && radius.is_finite()
                && radius > 0.0
                && radius <= MAX_SIDE_MM
                && start.is_finite()
                && sweep.is_finite()
                && sweep != 0.0
                && sweep.abs() <= std::f64::consts::TAU + 1e-9
        }
        Seg2::Cubic { p0, p1, p2, p3 } => at(p0) && at(p1) && at(p2) && at(p3),
        Seg2::Conic { a, apex, b, w } => at(a) && at(apex) && at(b) && w.is_finite() && w > 0.0,
        Seg2::Bezier { n, p } => {
            let n = usize::from(n);
            (4..=solidcore_record::MAX_BEZIER_DEGREE).contains(&n) && p.iter().take(n + 1).all(|v| at(*v))
        }
    }
}

#[cfg(test)]
#[path = "sheet_format_tests.rs"]
mod tests;
