//! Sheet tables: a parts list (and later the revision table) as data in sheet millimetres.
//!
//! A [`Table`] is a definition: its columns, the side its heading is on, its row height, its text
//! height, its two line widths, a filter and overrides. A [`Column`]'s cells are cell content
//! ([`crate::cell`]) read against the row each is on: row fields and text. The rows are input
//! ([`Row`]), in the order they are listed (a bill of materials' item order), with their values
//! worked out and formatted: nothing here works one out, so a filter hides rows and never changes
//! a count or a quantity. [`Table::validate`] checks a definition on its own, as when one is read,
//! and [`Table::layout`] places it with a text measurer: each shown row's rectangle and each cell's
//! lines of text by the cell text rule. Nothing here is saved or drawn yet.
//!
//! - The table grows away from its heading. With the heading at the top the rows run down below
//!   it; at the bottom (ISO 7573) the first row sits just above it and the rows run up.
//! - A column without a set width is as wide as its widest line. Every cell breaks between words
//!   to fit its column, and its row grows to hold its lines, [`LINE_PITCH`] cap heights apart; a
//!   word wider than its column is shown whole with [`Warning::Width`], never cut.
//! - A row field the row doesn't have shows [`MISSING`]. An [`Override`] shows its text in one
//!   cell and keeps the value it replaces; an empty one is a blank cell.
//! - The outline is drawn `outline_line_mm` wide and the lines between cells `cell_line_mm`, each
//!   centred on its edge, and a cell's text is padded from the inner edge of its lines, as in a
//!   title block.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::cell::{self, Content, HAlign, PADDING_MM, Part, Source, Warning, check_text, is_snake_case};
use crate::paper::{MAX_COORDINATE_MM, MAX_LINE_MM, MAX_SIDE_MM};
use crate::{Aabb2, Vec2};

/// The most rows a table may list, and the most overrides it may keep.
pub const MAX_ROWS: usize = 10_000;
/// The most columns a table may have.
pub const MAX_COLUMNS: usize = 64;
/// The most bytes of text a table and its rows may hold in all.
pub const MAX_TEXT_BYTES: usize = 1 << 20;
/// What a cell shows for a row field its row doesn't have.
pub const MISSING: &str = "-";
/// The distance between baselines, in cap heights: ISO 3098-0's line spacing.
pub const LINE_PITCH: f64 = 10.0 / 7.0;
/// The longest row key, row field key or field.
const MAX_KEY_BYTES: usize = 256;
const MAX_ROW_HEIGHT_MM: f64 = 1000.0;

/// A sheet table's definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub columns: Vec<Column>,
    #[serde(default)]
    pub heading: Heading,
    /// The least height of a row, the heading's too: a row with more lines grows.
    #[serde(default = "default_row_height")]
    pub row_height_mm: f64,
    /// The cap height of its text.
    pub cap_mm: f64,
    pub outline_line_mm: f64,
    pub cell_line_mm: f64,
    #[serde(default)]
    pub filter: Filter,
    #[serde(default)]
    pub overrides: Vec<Override>,
}

fn default_row_height() -> f64 {
    7.0
}

/// One column: its heading and what each of its cells shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// Snake case, and no other column of the table has it: overrides and filters name it.
    pub name: String,
    pub heading: String,
    /// Read against each row: its fields (`{"field": {"source": "row", "key": "part_number"}}`) and
    /// text.
    pub content: Content,
    /// None: as wide as its widest line.
    #[serde(default)]
    pub width_mm: Option<f64>,
    #[serde(default)]
    pub align: HAlign,
}

/// The side of the table its heading is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Heading {
    #[default]
    Top,
    /// The first row just above it, the table growing up (ISO 7573).
    Bottom,
}

/// Which rows are shown: those of the kind `only` names, whose cells contain every text filter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(default)]
    pub only: Option<Only>,
    #[serde(default)]
    pub text: Vec<TextFilter>,
}

/// One kind of row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Only {
    Parts,
    Assemblies,
    /// Purchased parts and assemblies.
    Purchased,
}

/// The rows whose cell in `column` contains `contains`, ignoring case.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextFilter {
    pub column: String,
    pub contains: String,
}

/// The text one cell shows in place of its value: the row by its key, the column by its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub row: String,
    pub column: String,
    pub text: String,
}

/// One row as a table reads it: its key (opaque here, unique among the rows), its kind and its
/// values by field key, formatted.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub kind: RowKind,
    pub purchased: bool,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Part,
    Assembly,
}

/// A table placed on a sheet: what a writer draws.
#[derive(Clone, Debug, PartialEq)]
pub struct TableLayout {
    pub outline: Aabb2,
    pub outline_line_mm: f64,
    pub cell_line_mm: f64,
    pub heading: RowLayout,
    /// The rows shown, in their order.
    pub rows: Vec<RowLayout>,
}

/// One row placed: its key (none for the heading), its rectangle and its cells, one per column.
#[derive(Clone, Debug, PartialEq)]
pub struct RowLayout {
    pub key: Option<String>,
    pub rect: Aabb2,
    pub cells: Vec<CellLayout>,
}

/// One cell placed: its rectangle and its lines of text.
#[derive(Clone, Debug, PartialEq)]
pub struct CellLayout {
    pub rect: Aabb2,
    pub lines: Vec<Line>,
    pub align: HAlign,
    /// The value an override replaces.
    pub replaced: Option<String>,
    pub warning: Option<Warning>,
}

/// A line of text: its anchor on its baseline, at its box's left, middle or right by the cell's
/// alignment.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub at: Vec2,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("table: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

fn cell_rule(e: cell::Invalid) -> Invalid {
    Invalid(e.0)
}

/// Counts bytes of text against [`MAX_TEXT_BYTES`].
struct Budget(usize);
impl Budget {
    fn add(&mut self, text: &str) -> Result<()> {
        self.count(text.len())
    }
    fn count(&mut self, bytes: usize) -> Result<()> {
        self.0 = self.0.saturating_add(bytes);
        require(self.0 <= MAX_TEXT_BYTES, "a table holds at most 1 MiB of text")
    }
}

/// One cell's text before it is placed.
struct Shown {
    text: String,
    replaced: Option<String>,
}

impl Table {
    /// Checks a definition on its own, counting its columns and overrides first: call it when one
    /// is read. [`Self::layout`] calls it too.
    pub fn validate(&self) -> Result<()> {
        self.check().map(drop)
    }

    /// [`Self::validate`], giving the bytes of text the definition holds.
    fn check(&self) -> Result<Budget> {
        require(!self.columns.is_empty() && self.columns.len() <= MAX_COLUMNS, "a table has 1 to 64 columns")?;
        require(self.overrides.len() <= MAX_ROWS, "a table keeps at most 10,000 overrides")?;
        require(self.filter.text.len() <= MAX_COLUMNS, "a table has at most 64 text filters")?;
        let height = self.row_height_mm;
        require(height > 0.0 && height <= MAX_ROW_HEIGHT_MM, "a row's height is above 0 and at most 1000 mm")?;
        require(self.cap_mm > 0.0 && self.cap_mm <= height, "the text's cap height is above 0 and at most a row's height")?;
        let lines = [self.outline_line_mm, self.cell_line_mm];
        require(lines.into_iter().all(|line| line > 0.0 && line <= MAX_LINE_MM), "a table's lines are above 0 and at most 10 mm wide")?;
        let mut budget = Budget(0);
        for (i, column) in self.columns.iter().enumerate() {
            require(is_snake_case(&column.name), "a column's name is snake case and at most 64 characters")?;
            require(self.columns.iter().take(i).all(|other| other.name != column.name), "no two columns have the same name")?;
            cell::check_label(&column.heading).map_err(cell_rule)?;
            column.content.validate().map_err(cell_rule)?;
            for part in &column.content.0 {
                match part {
                    Part::Text(text) => budget.add(text)?,
                    Part::Field { source, key, format } => {
                        require(*source == Source::Row, "a table's cells read their row")?;
                        require(format.unit.is_none() && format.decimals.is_none(), "a row field is shown as the row gives it")?;
                        budget.add(key)?;
                    }
                }
            }
            budget.add(&column.heading)?;
            if let Some(width) = column.width_mm {
                // Wider than its padding and the widest line on its sides, so its text has room.
                let room = width > 2.0 * PADDING_MM + self.outline_line_mm.max(self.cell_line_mm);
                require(room && width <= MAX_SIDE_MM, "a column's width leaves room for its text and is at most 5000 mm")?;
            }
        }
        let column = |name: &str| self.columns.iter().any(|c| c.name == name);
        for filter in &self.filter.text {
            require(column(&filter.column), "a text filter names a column")?;
            cell::check_label(&filter.contains).map_err(cell_rule)?;
            budget.add(&filter.contains)?;
        }
        let mut cells = HashSet::new();
        for o in &self.overrides {
            require(o.row.len() <= MAX_KEY_BYTES, "a row's key is at most 256 bytes")?;
            require(column(&o.column), "an override names a column")?;
            require(cells.insert((&o.row, &o.column)), "no two overrides are for one cell")?;
            check_text([o.text.as_str()]).map_err(cell_rule)?;
            budget.add(&o.row)?;
            budget.add(&o.text)?;
        }
        Ok(budget)
    }

    /// The table with the right end of its heading's outer edge at `corner` (its top right with the
    /// heading at the top, its bottom right with the heading at the bottom), showing the rows the
    /// filter lets through. `measure(text, cap_mm)` is the width of one line of text.
    pub fn layout(&self, rows: &[Row], corner: Vec2, measure: &dyn Fn(&str, f64) -> f64) -> Result<TableLayout> {
        let mut budget = self.check()?;
        require(rows.len() <= MAX_ROWS, "a table lists at most 10,000 rows")?;
        let on_paper = |p: Vec2| p.x.abs() <= MAX_COORDINATE_MM && p.y.abs() <= MAX_COORDINATE_MM;
        require(on_paper(corner), "a table lies within the paper's coordinates")?;
        let mut keys = HashSet::new();
        for row in rows {
            require(row.key.len() <= MAX_KEY_BYTES, "a row's key is at most 256 bytes")?;
            require(keys.insert(row.key.as_str()), "no two rows have the same key")?;
            budget.add(&row.key)?;
            for (key, value) in &row.fields {
                require(key.len() <= MAX_KEY_BYTES, "a row field's key is at most 256 bytes")?;
                check_text([value.as_str()]).map_err(cell_rule)?;
            }
        }
        let shown = self.shown(rows, &mut budget)?;
        let width = |text: &str| {
            let w = measure(text, self.cap_mm);
            require(w.is_finite() && w >= 0.0, "a measured text is finite and at least 0 mm wide").map(|()| w)
        };
        // Each column's left and right edge, from the left.
        let insets = self.insets(self.columns.len());
        let mut edges = Vec::with_capacity(self.columns.len());
        let mut x = 0.0;
        for (i, column) in self.columns.iter().enumerate() {
            let (left, right) = insets.get(i).copied().unwrap_or_default();
            let w = match column.width_mm {
                Some(w) => w,
                None => {
                    let texts =
                        std::iter::once(column.heading.as_str()).chain(shown.iter().filter_map(|(_, cells)| cells.get(i).map(|c| c.text.as_str())));
                    let mut widest = 0.0f64;
                    for line in texts.flat_map(|t| t.split('\n')) {
                        widest = widest.max(width(line)?);
                    }
                    widest + 2.0 * PADDING_MM + left + right
                }
            };
            edges.push((x, x + w));
            x += w;
        }
        let table_width = x;
        // Each row's lines, from the heading, wrapped to its column.
        let pitch = self.cap_mm * LINE_PITCH;
        let mut wrapped: Vec<Vec<(Vec<String>, bool)>> = Vec::with_capacity(shown.len() + 1);
        let heading: Vec<&str> = self.columns.iter().map(|c| c.heading.as_str()).collect();
        for texts in std::iter::once(heading).chain(shown.iter().map(|(_, cells)| cells.iter().map(|c| c.text.as_str()).collect())) {
            let mut cells = Vec::with_capacity(texts.len());
            for (i, text) in texts.into_iter().enumerate() {
                let ((x0, x1), (left, right)) = (edges.get(i).copied().unwrap_or_default(), insets.get(i).copied().unwrap_or_default());
                let fitted = self.columns.get(i).is_some_and(|c| c.width_mm.is_none());
                let room = x1 - x0 - left - right - 2.0 * PADDING_MM;
                cells.push(if fitted { (lines_of(text), false) } else { wrap(text, room, &width)? });
            }
            wrapped.push(cells);
        }
        // Heights top to bottom: the heading first or last.
        let mut order: Vec<usize> = (0..wrapped.len()).collect();
        if self.heading == Heading::Bottom {
            order.reverse();
        }
        let last = order.len().saturating_sub(1);
        let half = |outline: bool| if outline { self.outline_line_mm } else { self.cell_line_mm } / 2.0;
        let mut heights = vec![0.0; wrapped.len()];
        for (place, &r) in order.iter().enumerate() {
            let lines = wrapped.get(r).map_or(1, |cells| cells.iter().map(|(l, _)| l.len()).max().unwrap_or(1).max(1));
            let needed = self.cap_mm + (lines - 1) as f64 * pitch + 2.0 * PADDING_MM + half(place == 0) + half(place == last);
            if let Some(h) = heights.get_mut(r) {
                *h = self.row_height_mm.max(needed);
            }
        }
        let total: f64 = heights.iter().sum();
        let top = if self.heading == Heading::Top { corner.y } else { corner.y + total };
        let outline = Aabb2 { min: Vec2::new(corner.x - table_width, top - total), max: Vec2::new(corner.x, top) };
        require(on_paper(outline.min) && on_paper(outline.max), "a table lies within the paper's coordinates")?;
        let mut placed: Vec<Option<RowLayout>> = vec![None; wrapped.len()];
        let mut y = top;
        for (place, &r) in order.iter().enumerate() {
            let h = heights.get(r).copied().unwrap_or_default();
            let rect = Aabb2 { min: Vec2::new(outline.min.x, y - h), max: Vec2::new(outline.max.x, y) };
            let (above, below) = (half(place == 0), half(place == last));
            let mut cells = Vec::with_capacity(self.columns.len());
            for (i, column) in self.columns.iter().enumerate() {
                let ((x0, x1), (left, right)) = (edges.get(i).copied().unwrap_or_default(), insets.get(i).copied().unwrap_or_default());
                let cell_rect = Aabb2 { min: Vec2::new(outline.min.x + x0, rect.min.y), max: Vec2::new(outline.min.x + x1, rect.max.y) };
                let inside = Aabb2 { min: cell_rect.min + Vec2::new(left, below), max: cell_rect.max - Vec2::new(right, above) };
                let (texts, too_wide) = wrapped.get_mut(r).and_then(|cells| cells.get_mut(i)).map(std::mem::take).unwrap_or_default();
                let (at, height) =
                    cell::place_lines(cell::value_box(inside, None), texts.len(), self.cap_mm, pitch, column.align, false).map_err(cell_rule)?;
                let lines = texts.into_iter().enumerate().map(|(k, text)| Line { at: at - Vec2::new(0.0, k as f64 * pitch), text }).collect();
                let replaced = r.checked_sub(1).and_then(|s| shown.get(s)).and_then(|(_, cells)| cells.get(i)).and_then(|c| c.replaced.clone());
                cells.push(CellLayout {
                    rect: cell_rect,
                    lines,
                    align: column.align,
                    replaced,
                    warning: if too_wide { Some(Warning::Width) } else { height },
                });
            }
            let key = r.checked_sub(1).and_then(|s| shown.get(s)).map(|(row, _)| row.key.clone());
            if let Some(slot) = placed.get_mut(r) {
                *slot = Some(RowLayout { key, rect, cells });
            }
            y -= h;
        }
        let mut placed = placed.into_iter().flatten();
        let heading = placed.next().ok_or(Invalid("a table has a heading"))?;
        Ok(TableLayout { outline, outline_line_mm: self.outline_line_mm, cell_line_mm: self.cell_line_mm, heading, rows: placed.collect() })
    }

    /// The rows the filter lets through, in order, each with its cells' text, counted against the
    /// budget before it is built.
    fn shown<'r>(&self, rows: &'r [Row], budget: &mut Budget) -> Result<Vec<(&'r Row, Vec<Shown>)>> {
        let overrides: HashMap<(&str, &str), &str> = self.overrides.iter().map(|o| ((o.row.as_str(), o.column.as_str()), o.text.as_str())).collect();
        let filters: Vec<(Option<usize>, String)> =
            self.filter.text.iter().map(|f| (self.columns.iter().position(|c| c.name == f.column), f.contains.to_lowercase())).collect();
        let kind = |row: &Row| match self.filter.only {
            None => true,
            Some(Only::Parts) => row.kind == RowKind::Part,
            Some(Only::Assemblies) => row.kind == RowKind::Assembly,
            Some(Only::Purchased) => row.purchased,
        };
        let mut shown = Vec::new();
        for row in rows.iter().filter(|row| kind(row)) {
            let mut cells = Vec::with_capacity(self.columns.len());
            for column in &self.columns {
                let parts = || {
                    column.content.0.iter().map(|part| match part {
                        Part::Text(text) => text.as_str(),
                        Part::Field { key, .. } => row.fields.get(key).map_or(MISSING, String::as_str),
                    })
                };
                budget.count(parts().map(str::len).sum())?;
                let value: String = parts().collect();
                cells.push(match overrides.get(&(row.key.as_str(), column.name.as_str())) {
                    Some(text) => Shown { text: text.to_string(), replaced: Some(value) },
                    None => Shown { text: value, replaced: None },
                });
            }
            let passes =
                |(i, needle): &(Option<usize>, String)| i.and_then(|i| cells.get(i)).is_some_and(|c| c.text.to_lowercase().contains(needle.as_str()));
            if filters.iter().all(passes) {
                shown.push((row, cells));
            }
        }
        Ok(shown)
    }

    /// Each column's left and right line inset: half the outline on the table's sides, half a cell
    /// line between columns.
    fn insets(&self, columns: usize) -> Vec<(f64, f64)> {
        let half = |outline: bool| if outline { self.outline_line_mm } else { self.cell_line_mm } / 2.0;
        (0..columns).map(|i| (half(i == 0), half(i + 1 == columns))).collect()
    }
}

/// A text's lines as written: none for an empty text.
fn lines_of(text: &str) -> Vec<String> {
    if text.is_empty() { Vec::new() } else { text.split('\n').map(str::to_string).collect() }
}

/// `text`'s lines in a box `room` wide, and whether a word is wider than the box (shown whole). A
/// line that fits is kept as written; one that doesn't is broken between words where the next
/// word would pass the box's right side, runs of spaces between them kept as one.
fn wrap(text: &str, room: f64, width: &dyn Fn(&str) -> Result<f64>) -> Result<(Vec<String>, bool)> {
    let (mut lines, mut too_wide) = (Vec::new(), false);
    for paragraph in lines_of(text) {
        if width(&paragraph)? <= room {
            lines.push(paragraph);
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let tried = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || width(&tried)? <= room {
                line = tried;
            } else {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            }
            // A word that starts a line is measured on its own once.
            if line == word {
                too_wide |= width(word)? > room;
            }
        }
        lines.push(line);
    }
    Ok((lines, too_wide))
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
