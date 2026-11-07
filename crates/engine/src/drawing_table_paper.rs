//! A sheet's tables placed (#3874 P8b): each parts list's or revision table's rows read, its columns
//! fitted to what the drawing's fonts really set, and the whole where its record says. A table's
//! corner can only be named once its size is known, so it is laid out at the origin first and placed
//! second; one measure of each word serves both.
//!
//! - A word is measured through the drawing's lettering ([`PaperFonts`]), the one rule a sheet's
//!   text is set by. In the window, a word whose font is still being looked for makes its table
//!   pending: it is read again until the search answers, as the title block is.
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
use solidcraft_geom::cell::Part;
use solidcraft_geom::paper::{Group, ObjectKind};
use solidcraft_geom::table::{Heading, Row, Table, TableLayout};
use solidcraft_sketch::{Outcome, PaperFonts, PaperLines, TextStyle};

use crate::Session;

/// A table laid out and placed, with the rows it read before its filter.
pub(crate) struct Placed {
    pub rows: Vec<Row>,
    pub layout: TableLayout,
}

/// Why a table isn't placed: its words wait for a font (the window only), or it can't be.
pub(crate) enum Unplaced {
    Pending,
    Failed(String),
}

/// A sheet's tables as its scene draws them, in id order: each one's lines, or why it isn't drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SheetTables {
    pub tables: Vec<(u64, std::result::Result<Group, String>)>,
    /// Whether a table waits for a font to fall back to: it is left off until the search answers.
    pub pending: bool,
}

/// Each table on `sheet` of `d`, read from `s`'s design and measured in `lettering`.
pub(crate) fn sheet_tables(s: &Session, d: &Drawing, sheet: &Sheet, lettering: &PaperFonts) -> SheetTables {
    let space = drawing_space(sheet);
    let mut out = SheetTables::default();
    for record in d.tables.iter().filter(|t| t.sheet == sheet.id) {
        match place(s, d, record, space, lettering) {
            Ok(placed) => out.tables.push((record.id, Ok(placed.layout.paper(record.id, kind_of(record))))),
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
    let words = Words::new(lettering);
    let measure = |text: &str, cap_mm: f64| words.width(text, cap_mm);
    let sized = table.layout(&rows, Vec2::ZERO, &measure).map_err(|e| Unplaced::Failed(e.to_string()))?;
    let corner = corner_of(record.place, table.heading, sized.outline.size(), space);
    let layout = table.layout(&rows, corner, &measure).map_err(|e| Unplaced::Failed(e.to_string()))?;
    if words.pending.get() {
        return Err(Unplaced::Pending);
    }
    Ok(Placed { rows, layout })
}

/// A table's words, each measured once in the drawing's lettering: a table repeats its words, and a
/// word costs a font lookup. A word no font can set measures nothing (its cell is noted once it is
/// set on paper); one whose font is still being looked for makes the table pending.
struct Words<'a> {
    lettering: &'a PaperFonts,
    measured: RefCell<HashMap<(String, u64), Option<PaperLines>>>,
    pending: std::cell::Cell<bool>,
}

impl<'a> Words<'a> {
    fn new(lettering: &'a PaperFonts) -> Words<'a> {
        Words { lettering, measured: RefCell::new(HashMap::new()), pending: std::cell::Cell::new(false) }
    }

    /// The width of `text`'s widest line at `cap_mm`.
    fn width(&self, text: &str, cap_mm: f64) -> f64 {
        let mut measured = self.measured.borrow_mut();
        let lines = measured.entry((text.to_string(), cap_mm.to_bits())).or_insert_with(|| {
            match Outcome::from(self.lettering.lines(text, cap_mm, None, &TextStyle::default())) {
                Outcome::Set(lines) => Some(lines),
                Outcome::Pending => {
                    self.pending.set(true);
                    None
                }
                Outcome::Refused(_) => None,
            }
        });
        lines.as_ref().map_or(0.0, |lines| lines.lines.iter().map(|(_, width)| *width).fold(0.0, f64::max))
    }
}

/// The point [`Table::layout`] takes for a table of `size` placed at `place`: the right end of its
/// heading's outer edge.
fn corner_of(place: Place, heading: Heading, size: Vec2, space: Aabb2) -> Vec2 {
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
    let edge = if heading == Heading::Top { top_left.y } else { top_left.y - size.y };
    Vec2::new(top_left.x + size.x, edge)
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
