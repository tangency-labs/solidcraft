//! A drawing's symbols of its own (#13063): a shape made once, from lines, arcs, fills and text,
//! to be placed many times ([`PlacedSymbol`]).
//!
//! - **The insertion point** is the definition's origin: every piece is in paper millimetres from
//!   it, so a placement moves, turns and scales the pieces there, and no field holds it.
//! - **A stroke** is a line of one kind ([`LineKind`]): its width and dashes come from the sheet's
//!   line group, as a view's lines do, so a symbol matches the drawing it is placed on.
//! - **A fill** is a closed outline painted solid.
//! - **A text** is a note's content ([`Content`]), which may also ask for prompts: the values each
//!   placement gives ([`Symbol::prompts`]).
//!
//! The rules, beside the record's: a name as a sheet's, and no two symbols with one name; 1 to
//! [`MAX_PIECES`] pieces, at most [`MAX_TEXTS`] of them texts; each path as the paper scene takes
//! a stroke's ([`paper::check_path`]); every point at most [`MAX_SIDE_MM`] from the origin; a text's
//! height as a note's ([`CAP_MM`]); at most [`MAX_SYMBOLS`] symbols, listed by id.
//!
//! **A placement** sits where a note would ([`Anchor`]): on a sheet, on a view (it moves with the
//! view), or on an edge or a corner of a view's model. Its insertion point is at `at`, its pieces
//! scaled by `scale` and turned by `angle` there, and its prompts answered by `answers`. It names
//! its definition by id and keeps none of it, so a changed definition changes every placement.
//! Where its anchor stands with the model is worked out, never saved, as a note's is
//! ([`Document::resolve_placements`]). The rules: its symbol is one of the drawing's; its anchor's as a note's
//! ([`Drawing::check_anchor`]); `at` on paper; `scale` in [`SCALE`]; `angle` finite; each answer
//! names one of the definition's prompts and is one line of text ([`check_label`]); at most
//! [`MAX_PLACEMENTS`], listed by id.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use solidcore_record::Seg2;
use solidcraft_geom::Aabb2;
use solidcraft_geom::cell::{Content, HAlign, Part, Source, check_label};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::line::LineKind;
use solidcraft_geom::paper::{self, MAX_SIDE_MM};
use solidcraft_geom::prelude::*;

use super::by_id::Record;
use super::dimension::Lookup;
use super::note::{Anchor, CAP_MM, End};
use super::{Drawing, check_name, require};
use crate::{DocError, Document, Model, Result, ids};

/// The most symbols a drawing may have.
pub const MAX_SYMBOLS: usize = 1_000;
/// The most pieces a symbol may have.
pub const MAX_PIECES: usize = 1_000;
/// The most of its pieces that may be texts: each is set in its font wherever it is placed.
pub const MAX_TEXTS: usize = 64;
/// The most placements a drawing may have.
pub const MAX_PLACEMENTS: usize = 1_000;
/// The sizes a placement may give its symbol: its pieces' millimetres times this.
pub const SCALE: RangeInclusive<f64> = 0.01..=100.0;

/// One symbol of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Symbol {
    #[serde(with = "crate::ids")]
    pub id: u64,
    pub name: String,
    pub pieces: Vec<Piece>,
}

/// One piece of a symbol, in paper millimetres from its insertion point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Piece {
    /// A line along `path`, each piece starting where the one before ends.
    Stroke {
        path: Vec<Seg2>,
        /// It ends where it starts, with a join there instead of two ends.
        #[serde(default)]
        closed: bool,
        line: LineKind,
    },
    /// The region inside `path`, which ends where it starts, painted solid.
    Fill { path: Vec<Seg2> },
    /// Text whose first line starts at `at`, as a note's does.
    Text {
        at: [f64; 2],
        content: Content,
        /// The height of its capitals, when it sets its own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cap_mm: Option<f64>,
        #[serde(default)]
        align: HAlign,
    },
}

impl Record for Symbol {
    fn id(&self) -> u64 {
        self.id
    }
}

/// One place a symbol sits on a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedSymbol {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// Its definition, by id.
    #[serde(with = "crate::ids")]
    pub symbol: u64,
    pub anchor: Anchor,
    /// Where its insertion point sits, in millimetres of paper from its sheet's corner, or from its
    /// view's centre.
    pub at: [f64; 2],
    pub scale: f64,
    /// Its turn about its insertion point, counter-clockwise, in radians.
    pub angle: f64,
    /// The text each prompt of its definition shows here, by key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, String>,
}

impl Record for PlacedSymbol {
    fn id(&self) -> u64 {
        self.id
    }
}

impl Symbol {
    /// Its texts' contents, in piece order.
    pub fn texts(&self) -> impl Iterator<Item = &Content> + Clone {
        self.pieces.iter().filter_map(|piece| match piece {
            Piece::Text { content, .. } => Some(content),
            Piece::Stroke { .. } | Piece::Fill { .. } => None,
        })
    }

    /// The keys its texts prompt for, each once and sorted: what a placement answers.
    pub fn prompts(&self) -> BTreeSet<&str> {
        // A fn, not a closure: a closure's `&Part` and its `&str` get unrelated lifetimes.
        fn prompt(part: &Part) -> Option<&str> {
            match part {
                Part::Field { source: Source::Prompt, key, .. } => Some(key.as_str()),
                _ => None,
            }
        }
        self.texts().flat_map(|content| content.0.iter().filter_map(prompt)).collect()
    }
}

impl Document {
    /// The anchors of placements `wanted` of `drawing` (one of this design's drawings) as `model`
    /// (this design's) has them now, as [`Document::resolve_notes`] finds a note's. `Err` only for an
    /// id the drawing doesn't have.
    pub fn resolve_placements(&self, model: &Model, drawing: &Drawing, wanted: &[u64]) -> Result<Vec<End>> {
        let mut lookup = Lookup::new(self, model);
        let placed = |id: &u64| drawing.placed_symbols.by_id(*id).ok_or_else(|| DocError::Unknown(format!("placed symbol {}", ids::encode(*id))));
        wanted.iter().map(|id| Ok(placed(id)?.anchor.end(&mut lookup))).collect()
    }
}

impl Drawing {
    /// The symbols' rules (the module's notes).
    pub(super) fn check_symbols(&self) -> Result<()> {
        self.symbols.check(MAX_SYMBOLS, "symbol", check_symbol)?;
        let mut names = BTreeSet::new();
        require(self.symbols.iter().all(|s| names.insert(s.name.as_str())), || "two symbols have one name".into())
    }

    /// The placements' rules (the module's notes). The symbols, sheets and views are checked first.
    pub(super) fn check_placed_symbols(&self) -> Result<()> {
        self.placed_symbols.check(MAX_PLACEMENTS, "placed symbol", |p| self.check_placed(p))
    }

    /// The placements of symbol `symbol`, by id.
    pub fn placements_of(&self, symbol: u64) -> Vec<u64> {
        self.placed_symbols.iter().filter(|p| p.symbol == symbol).map(|p| p.id).collect()
    }

    fn check_placed(&self, p: &PlacedSymbol) -> Result<()> {
        let missing = || DocError::Invalid(format!("it places symbol {}, which the drawing doesn't have", ids::encode(p.symbol)));
        let symbol = self.symbols.by_id(p.symbol).ok_or_else(missing)?;
        self.check_anchor(&p.anchor)?;
        let [x, y] = p.at;
        require(is_paper_coordinate(x) && is_paper_coordinate(y), || format!("its place is at most {MAX_SIDE_MM} mm on paper"))?;
        require(SCALE.contains(&p.scale), || format!("its scale is {} to {}", SCALE.start(), SCALE.end()))?;
        require(p.angle.is_finite(), || "its angle is a number".into())?;
        let prompts = symbol.prompts();
        for (key, answer) in &p.answers {
            require(prompts.contains(key.as_str()), || format!("it answers `{key}`, which symbol `{}` doesn't prompt for", symbol.name))?;
            check_label(answer).map_err(|e| DocError::Invalid(format!("its answer to `{key}`: {}", e.0)))?;
        }
        Ok(())
    }
}

fn check_symbol(s: &Symbol) -> Result<()> {
    check_name(&s.name, "its")?;
    require((1..=MAX_PIECES).contains(&s.pieces.len()), || format!("it has 1 to {MAX_PIECES} pieces"))?;
    let texts = s.pieces.iter().filter(|piece| matches!(piece, Piece::Text { .. })).count();
    require(texts <= MAX_TEXTS, || format!("at most {MAX_TEXTS} of its pieces are texts"))?;
    s.pieces.iter().try_for_each(check_piece)
}

fn check_piece(piece: &Piece) -> Result<()> {
    match piece {
        Piece::Stroke { path, closed, .. } => check_path(path, *closed),
        Piece::Fill { path } => check_path(path, true),
        Piece::Text { at: [x, y], content, cap_mm, .. } => {
            require(is_paper_coordinate(*x) && is_paper_coordinate(*y), || format!("a text starts at most {MAX_SIDE_MM} mm from its origin"))?;
            require(!content.0.is_empty(), || "a text has some text".into())?;
            content.validate().map_err(|e| DocError::Invalid(e.0.into()))?;
            // A symbol sits on a sheet or in a view, never in a table's row.
            let can_read = |part: &Part| !matches!(part, Part::Field { source: Source::Row, .. });
            require(content.0.iter().all(can_read), || {
                "a text's fields read the drawing, its sheet, its component, its newest revision or a prompt".into()
            })?;
            require(cap_mm.is_none_or(|cap| CAP_MM.contains(&cap)), || format!("a text's height is {} to {} mm", CAP_MM.start(), CAP_MM.end()))
        }
    }
}

/// A path as a stroke on paper takes it, and every point of it at most [`MAX_SIDE_MM`] from the
/// origin, as a sheet's points are from its corner.
fn check_path(path: &[Seg2], closed: bool) -> Result<()> {
    paper::check_path(path, closed).map_err(|e| DocError::Invalid(e.0.into()))?;
    let on_paper = |b: Aabb2| [b.min.x, b.min.y, b.max.x, b.max.y].into_iter().all(is_paper_coordinate);
    require(path.iter().all(|seg| on_paper(seg.bounds())), || format!("each line is at most {MAX_SIDE_MM} mm from its origin"))
}

#[cfg(test)]
#[path = "symbol_tests.rs"]
mod tests;
