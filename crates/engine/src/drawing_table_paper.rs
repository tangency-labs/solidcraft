//! A sheet's tables placed and on paper (#3874 P8b, P8c): each parts list's or revision table's rows
//! read, its columns fitted to what the drawing's fonts really set, the whole where its record says,
//! and its words in its cells. A table's corner can only be named once its size is known, so it is
//! laid out at the origin first and placed second; one measure of each word serves both, and its
//! words go on paper as they were measured, never shaped again.
//!
//! - A word is measured through the drawing's lettering ([`PaperFonts`]), the one rule a sheet's
//!   text is set by. In the window, a word whose font is still being looked for makes its table
//!   pending: it is read again until the search answers, as the title block is.
//! - A word no font can set is left off and its table says why; the table still draws.
//! - A parts list reads the design open, as `drawing.inspect` does, weighed only when a column shows
//!   a mass.
//! - A table that can't be read or laid out says why, and the rest of the sheet still draws.
//!
//! **Cost:** a weighed list costs about 135 ms on a bolted bracket, unweighed 0.2 ms. The sheet scene
//! reads its tables only when what its text reads changes ([`crate::drawing_scene`]'s
//! `Assembled::reads`), never per frame.

use std::cell::RefCell;
use std::collections::HashMap;

use solidcore_record::Vec2;
use solidcraft_doc::bom::{self, Tree};
use solidcraft_doc::drawing::table::{Corner, Kind, Place, SheetTable};
use solidcraft_doc::drawing::{Drawing, Sheet};
use solidcraft_geom::Aabb2;
use solidcraft_geom::cell::{HAlign, Part};
use solidcraft_geom::paper::text::Font;
use solidcraft_geom::paper::{Group, Item, ObjectKind};
use solidcraft_geom::table::{Row, Table, TableLayout};
use solidcraft_sketch::{Outcome, PaperFonts, PaperLines, TextStyle};

use crate::Session;

/// A table laid out and placed, with the rows it read before its filter.
pub(crate) struct Placed {
    pub rows: Vec<Row>,
    pub layout: TableLayout,
    /// Its words as the layout measured them.
    words: Words,
}

/// A table on paper: its lines and words, the fonts its words use, and why a word is left off.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OnPaper {
    pub group: Group,
    pub fonts: Vec<Font>,
    pub why: Option<String>,
}

/// Why a table isn't placed: its words wait for a font (the window only), or it can't be.
pub(crate) enum Unplaced {
    Pending,
    Failed(String),
}

/// A sheet's tables as its scene draws them, in id order: each one on paper, or why it isn't drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SheetTables {
    pub tables: Vec<(u64, std::result::Result<OnPaper, String>)>,
    /// Whether a table waits for a font to fall back to: it is left off until the search answers.
    pub pending: bool,
}

/// Each table on `sheet` of `d`, read from `s`'s design and measured in `lettering`.
pub(crate) fn sheet_tables(s: &Session, d: &Drawing, sheet: &Sheet, lettering: &PaperFonts) -> SheetTables {
    let space = drawing_space(sheet);
    let mut out = SheetTables::default();
    for record in d.tables.iter().filter(|t| t.sheet == sheet.id) {
        match place(s, d, record, space, lettering).map(|placed| placed.paper(record.id, kind_of(record), record.table.cap_mm, lettering)) {
            Ok(on_paper) => out.tables.push((record.id, Ok(on_paper))),
            Err(Unplaced::Pending) => out.pending = true,
            Err(Unplaced::Failed(why)) => out.tables.push((record.id, Err(why))),
        }
    }
    out
}

/// The kind of sheet object `record` is.
pub(crate) fn kind_of(record: &SheetTable) -> ObjectKind {
    match record.kind {
        Kind::PartsList(_) => ObjectKind::PartsList,
        Kind::Revision => ObjectKind::RevisionTable,
    }
}

/// Where a sheet's tables are anchored: inside its drawing border, or its paper without one.
pub(crate) fn drawing_space(sheet: &Sheet) -> Aabb2 {
    let border = sheet.border.as_ref().and_then(|border| border.layout(&sheet.sheet).ok()).map(|layout| layout.drawing_border);
    border.unwrap_or_else(|| Aabb2 { min: Vec2::ZERO, max: sheet.sheet.size_mm().unwrap_or(Vec2::ZERO) })
}

/// `record` of `d` placed in drawing space `space`, its words measured in `lettering`.
pub(crate) fn place(s: &Session, d: &Drawing, record: &SheetTable, space: Aabb2, lettering: &PaperFonts) -> std::result::Result<Placed, Unplaced> {
    let rows = rows_of(s, d, record).map_err(Unplaced::Failed)?;
    let table = &record.table;
    let words = Words::default();
    let measure = |text: &str, cap_mm: f64| words.width(lettering, text, cap_mm);
    let sized = table.layout(&rows, Vec2::ZERO, &measure).map_err(|e| Unplaced::Failed(e.to_string()))?;
    let corner = corner_of(record.place, sized.outline, space);
    let layout = table.layout(&rows, corner, &measure).map_err(|e| Unplaced::Failed(e.to_string()))?;
    if words.pending.get() {
        return Err(Unplaced::Pending);
    }
    Ok(Placed { rows, layout, words })
}

impl Placed {
    /// The table on paper as the `kind` of object `object` is: its lines, then each cell's lines of
    /// text, each standing on the anchor its layout gives, set as `lettering` measured it for the
    /// layout. A word no font can set is left off and noted. None is pending: [`place`] measured every
    /// word (a line is a word or words it measured) and refuses a table while any is.
    pub(crate) fn paper(&self, object: u64, kind: ObjectKind, cap_mm: f64, lettering: &PaperFonts) -> OnPaper {
        let mut out = OnPaper { group: self.layout.paper(object, kind), fonts: Vec::new(), why: None };
        let mut unset: Vec<String> = Vec::new();
        for cell in self.layout.drawn_rows().flat_map(|row| &row.cells) {
            // The anchor is the line's left, middle or right as its column aligns; a word is measured
            // from its left, with the width the layout used, so it is shaped once for both.
            let along = match cell.align {
                HAlign::Left => 0.0,
                HAlign::Centre => 0.5,
                HAlign::Right => 1.0,
            };
            for line in &cell.lines {
                let set = self.words.read(lettering, &line.text, cap_mm, |measured| match measured {
                    Outcome::Set(lines) => match lines.set(line.at - Vec2::new(widest(lines) * along, 0.0), 0.0) {
                        Ok(set) => Outcome::Set(set),
                        Err(e) => Outcome::Refused(e.to_string()),
                    },
                    Outcome::Pending => Outcome::Pending,
                    Outcome::Refused(why) => Outcome::Refused(why.clone()),
                });
                match set {
                    Outcome::Set(Some((runs, fonts))) => {
                        out.group.items.extend(runs.into_iter().map(Item::Text));
                        out.fonts.extend(fonts);
                    }
                    // Never pending here ([`place`]): nothing to set.
                    Outcome::Set(None) | Outcome::Pending => {}
                    Outcome::Refused(why) => unset.push(format!("`{}`: {why}", line.text)),
                }
            }
        }
        if let Some(first) = unset.first() {
            let more = if unset.len() > 1 { format!(" (and {} more)", unset.len() - 1) } else { String::new() };
            out.why = Some(format!("the drawing's fonts can't set {first}{more}"));
        }
        out
    }
}

/// A table's words, each measured once in the drawing's lettering, left-aligned: a table repeats its
/// words, and a word costs a font lookup. One whose font is still being looked for makes the table
/// pending.
#[derive(Default)]
struct Words {
    measured: RefCell<HashMap<(String, u64), Outcome<PaperLines>>>,
    pending: std::cell::Cell<bool>,
}

impl Words {
    /// What `read` makes of `text` measured at `cap_mm` in `lettering`, measured on its first read.
    fn read<T>(&self, lettering: &PaperFonts, text: &str, cap_mm: f64, read: impl FnOnce(&Outcome<PaperLines>) -> T) -> T {
        let mut measured = self.measured.borrow_mut();
        let outcome = measured
            .entry((text.to_string(), cap_mm.to_bits()))
            .or_insert_with(|| Outcome::from(lettering.lines(text, cap_mm, None, &TextStyle::default())));
        if matches!(outcome, Outcome::Pending) {
            self.pending.set(true);
        }
        read(outcome)
    }

    /// The width of `text`'s widest line at `cap_mm`: nothing for one that can't be set.
    fn width(&self, lettering: &PaperFonts, text: &str, cap_mm: f64) -> f64 {
        self.read(lettering, text, cap_mm, |outcome| if let Outcome::Set(lines) = outcome { widest(lines) } else { 0.0 })
    }
}

fn widest(lines: &PaperLines) -> f64 {
    lines.lines.iter().map(|(_, width)| *width).fold(0.0, f64::max)
}

/// The point [`Table::layout`] takes for a table placed at `place`, given `sized`, its outline laid
/// out at the origin: the point that puts that outline's top left where `place` names it. So a
/// heading on either side and parts on either side of the first all land where they are named.
fn corner_of(place: Place, sized: Aabb2, space: Aabb2) -> Vec2 {
    let size = sized.size();
    // The outline's top left, whichever way it is named.
    let top_left = match place {
        Place::At([x, y]) => Vec2::new(x, y),
        Place::Anchor { corner, offset_mm: [dx, dy] } => {
            let (x, y) = match corner {
                Corner::TopLeft => (space.min.x, space.max.y),
                Corner::TopRight => (space.max.x - size.x, space.max.y),
                Corner::BottomLeft => (space.min.x, space.min.y + size.y),
                Corner::BottomRight => (space.max.x - size.x, space.min.y + size.y),
            };
            Vec2::new(x + dx, y + dy)
        }
    };
    top_left - Vec2::new(sized.min.x, sized.max.y)
}

/// `record`'s rows: a revision table's from the drawing's revisions; a parts list's from the design
/// open, or why it can't be listed.
fn rows_of(s: &Session, d: &Drawing, record: &SheetTable) -> std::result::Result<Vec<Row>, String> {
    let list = match &record.kind {
        Kind::Revision => return Ok(d.revisions.table_rows(d.revisions.per_sheet.then_some(record.sheet))),
        Kind::PartsList(list) => list,
    };
    let state = s.model.state();
    let mut tree = Tree::new(&s.doc, &state);
    let request = bom::Request { mass: shows_mass(&record.table), ..list.request() };
    let listed = bom::list(&mut tree, &request).map_err(|e| e.to_string())?;
    Ok(list.rows(&listed, s.doc.drawing_units(d)))
}

/// Whether a column shows a row's mass, which is what makes a list weigh its parts.
fn shows_mass(table: &Table) -> bool {
    table.columns.iter().flat_map(|column| &column.content.0).any(|part| matches!(part, Part::Field { key, .. } if key == "mass"))
}

#[cfg(test)]
#[path = "drawing_table_paper_tests.rs"]
mod tests;
