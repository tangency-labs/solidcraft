//! A drawing's symbols of its own (#13063): a shape made once, from lines, arcs, fills and text,
//! to be placed many times. Placing comes later; this is the definition.
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

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use solidcore_record::Seg2;
use solidcraft_geom::Aabb2;
use solidcraft_geom::cell::{Content, HAlign, Part, Source};
use solidcraft_geom::drawing_view::is_paper_coordinate;
use solidcraft_geom::paper::line::LineKind;
use solidcraft_geom::paper::{self, MAX_SIDE_MM};
use solidcraft_geom::prelude::*;

use super::by_id::Record;
use super::note::CAP_MM;
use super::{Drawing, check_name, require};
use crate::{DocError, Result};

/// The most symbols a drawing may have.
pub const MAX_SYMBOLS: usize = 1_000;
/// The most pieces a symbol may have.
pub const MAX_PIECES: usize = 1_000;
/// The most of its pieces that may be texts: each is set in its font wherever it is placed.
pub const MAX_TEXTS: usize = 64;

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

impl Symbol {
    /// The keys its texts prompt for, each once and sorted: what a placement answers.
    pub fn prompts(&self) -> BTreeSet<&str> {
        let texts = self.pieces.iter().filter_map(|piece| match piece {
            Piece::Text { content, .. } => Some(content),
            Piece::Stroke { .. } | Piece::Fill { .. } => None,
        });
        // A fn, not a closure: a closure's `&Part` and its `&str` get unrelated lifetimes.
        fn prompt(part: &Part) -> Option<&str> {
            match part {
                Part::Field { source: Source::Prompt, key, .. } => Some(key.as_str()),
                _ => None,
            }
        }
        texts.flat_map(|content| content.0.iter().filter_map(prompt)).collect()
    }
}

impl Drawing {
    /// The symbols' rules (the module's notes).
    pub(super) fn check_symbols(&self) -> Result<()> {
        self.symbols.check(MAX_SYMBOLS, "symbol", check_symbol)?;
        let mut names = BTreeSet::new();
        require(self.symbols.iter().all(|s| names.insert(s.name.as_str())), || "two symbols have one name".into())
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
