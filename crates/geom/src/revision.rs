//! A drawing's revisions: the revision record, its numbering, its issued history and its tags, as
//! data. The revision table shows the record through the sheet table ([`crate::table`]), and a
//! title block's revision fields ([`crate::cell::Source::Revision`]) read its newest row through the
//! same [`RevisionRow::field`].
//!
//! - One record covers the whole drawing, or each sheet on its own (`per_sheet`), numbered apart.
//! - A row holds what people type: revision, description, designer, checker and date. Adding a row
//!   fills in only the next revision; nothing here fills in a date, a checker or an approval.
//! - Revisions are numbered, never lettered: from 1, or 0. The next revision follows the highest
//!   number in its scope among the rows and the issued history, so an issued revision is never given
//!   out again. A revision typed by hand is kept as typed.
//! - Hiding and deleting rows changes what the table shows. [`RevisionRecord::issue`] appends a
//!   copy of a row to the issued history, once for each row and under the row's id, which nothing
//!   here removes or changes. Release, approval and frozen snapshots build on it later.
//! - Row and tag ids come from the caller (the design's random ids), never from a counter here, so
//!   two people adding rows to one drawing never take the same id. An id in use is refused.
//! - A [`Tag`] (a triangle with the revision in it) is bound to a row by its id and shows its
//!   revision, or its own text. Deleting a row reports the tags bound to it, which then show the
//!   revision they had, marked lost.
//!
//! This is the drawing's revision only: the model's revision is its component's part property and
//! the time of an export is the writer's, and neither is kept or stood in for here.

use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use crate::cell::{self, Content, HAlign, Part, Source, check_label, check_text};
use crate::table::{self, Column, Heading, RowKind, Table};

/// The most rows a record may have.
pub const MAX_ROWS: usize = 1000;
/// The most entries the issued history may hold, and the most tags.
pub const MAX_ISSUED: usize = 10_000;
const TAG_SIZES_MM: std::ops::RangeInclusive<f64> = 1.0..=50.0;
const DEFAULT_TAG_MM: f64 = 5.0;

/// A drawing's revision record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRecord {
    #[serde(default)]
    pub numbering: Numbering,
    /// Each sheet has its own rows, numbered apart; otherwise one record for the whole drawing.
    #[serde(default)]
    pub per_sheet: bool,
    rows: Vec<RevisionRow>,
    /// Append-only: [`Self::issue`] adds to it, nothing takes from it.
    #[serde(default)]
    issued: Vec<RevisionRow>,
    #[serde(default)]
    tags: Vec<Tag>,
}

/// How new revisions are numbered: 1, 2, 3… from `start`, 0 or 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Numbering {
    pub start: u8,
}

impl Default for Numbering {
    fn default() -> Self {
        Numbering { start: 1 }
    }
}

/// One revision, as people typed it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRow {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// Its sheet in a record kept per sheet (opaque here), else none.
    #[serde(default, with = "crate::ids::option")]
    pub sheet: Option<u64>,
    pub revision: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub designer: String,
    #[serde(default)]
    pub checker: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub hidden: bool,
    /// The model version an issued copy was issued against, for the release step to pin: none
    /// until version control gives versions (#2869).
    #[serde(default)]
    pub model_version: Option<ModelVersion>,
}

/// The design a revision was issued from, in the version history's words, both opaque here. A
/// change rather than a commit: the row is saved in the file it describes, so it can't name that
/// file's own commit, and a change's id survives an amend.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelVersion {
    pub path: String,
    pub change: String,
}

/// One of a row's texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Revision,
    Description,
    Designer,
    Checker,
    Date,
}

impl Field {
    pub const ALL: [Field; 5] = [Field::Revision, Field::Description, Field::Designer, Field::Checker, Field::Date];

    /// Its key in a revision table's rows and a title block's revision fields.
    pub fn key(self) -> &'static str {
        match self {
            Field::Revision => "revision",
            Field::Description => "description",
            Field::Designer => "designer",
            Field::Checker => "checker",
            Field::Date => "date",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Field::Revision => "Revision",
            Field::Description => "Description",
            Field::Designer => "Designer",
            Field::Checker => "Checker",
            Field::Date => "Date",
        }
    }
}

/// A triangular revision tag on a sheet, bound to a row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    #[serde(with = "crate::ids")]
    pub id: u64,
    #[serde(with = "crate::ids")]
    pub row: u64,
    /// The length of the triangle's side.
    pub size_mm: f64,
    /// Shown in place of the row's revision: an override.
    #[serde(default)]
    pub text: Option<String>,
    /// The revision it showed when its row was deleted.
    #[serde(default)]
    pub lost: Option<String>,
}

/// What a tag shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagText {
    Revision(String),
    Override(String),
    /// Its row is gone: the revision it had.
    Lost(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("revision record: {0}")]
pub struct Invalid(pub &'static str);

type Result<T> = std::result::Result<T, Invalid>;

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition { Ok(()) } else { Err(Invalid(message)) }
}

fn cell_rule(e: cell::Invalid) -> Invalid {
    Invalid(e.0)
}

impl RevisionRow {
    /// The text `key` names ([`Field::key`]): what a revision table's cells and a title block's
    /// revision fields show.
    pub fn field(&self, key: &str) -> Option<&str> {
        Some(match Field::ALL.into_iter().find(|f| f.key() == key)? {
            Field::Revision => &self.revision,
            Field::Description => &self.description,
            Field::Designer => &self.designer,
            Field::Checker => &self.checker,
            Field::Date => &self.date,
        })
    }

    fn field_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Revision => &mut self.revision,
            Field::Description => &mut self.description,
            Field::Designer => &mut self.designer,
            Field::Checker => &mut self.checker,
            Field::Date => &mut self.date,
        }
    }

    fn check(&self) -> Result<()> {
        check_label(&self.revision).map_err(cell_rule)?;
        if let Some(version) = &self.model_version {
            check_label(&version.path).map_err(cell_rule)?;
            check_label(&version.change).map_err(cell_rule)?;
        }
        check_text([&self.description, &self.designer, &self.checker, &self.date].map(String::as_str)).map_err(cell_rule)
    }
}

impl RevisionRecord {
    pub fn rows(&self) -> &[RevisionRow] {
        &self.rows
    }

    pub fn issued(&self) -> &[RevisionRow] {
        &self.issued
    }

    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Checks a record on its own, counting first: call it when one is read.
    pub fn validate(&self) -> Result<()> {
        require(self.rows.len() <= MAX_ROWS, "a record has at most 1000 rows")?;
        require(self.issued.len() <= MAX_ISSUED, "a record's issued history holds at most 10,000 entries")?;
        require(self.tags.len() <= MAX_ISSUED, "a record has at most 10,000 tags")?;
        require(matches!(self.numbering.start, 0 | 1), "numbers start at 0 or 1")?;
        let mut ids = HashSet::new();
        for row in &self.rows {
            require(ids.insert(row.id), "each row and tag has its own id")?;
            require(row.sheet.is_some() == self.per_sheet, "a row has a sheet when the record is kept per sheet, and only then")?;
            row.check()?;
        }
        let mut issued = HashSet::new();
        for row in &self.issued {
            require(issued.insert(row.id), "a row is issued once")?;
            row.check()?;
        }
        // An issued copy keeps its row's id, row or no row: only a tag can't have it.
        ids.extend(issued);
        for tag in &self.tags {
            require(ids.insert(tag.id), "each row and tag has its own id")?;
            require(tag.lost.is_some() || self.rows.iter().any(|r| r.id == tag.row), "a tag is bound to a row")?;
            require(TAG_SIZES_MM.contains(&tag.size_mm), "a tag's side is 1 to 50 mm")?;
            for text in tag.text.iter().chain(&tag.lost) {
                check_label(text).map_err(cell_rule)?;
            }
        }
        Ok(())
    }

    /// Every id the record holds, each once: its rows', its issued copies' (a copy has its row's) and
    /// its tags'.
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        let rows: BTreeSet<u64> = self.rows.iter().chain(&self.issued).map(|r| r.id).collect();
        rows.into_iter().chain(self.tags.iter().map(|t| t.id))
    }

    /// Whether row `id` has an issued copy.
    pub fn is_issued(&self, id: u64) -> bool {
        self.issued.iter().any(|r| r.id == id)
    }

    /// Whether a row, an issued copy or a tag has `id`, or a tag is bound to it.
    fn id_taken(&self, id: u64) -> bool {
        self.rows.iter().chain(&self.issued).any(|r| r.id == id) || self.tags.iter().any(|t| t.id == id || t.row == id)
    }

    fn row_mut(&mut self, id: u64) -> Result<&mut RevisionRow> {
        self.rows.iter_mut().find(|r| r.id == id).ok_or(Invalid("no row has that id"))
    }

    /// The next revision for `sheet`: after the highest among its rows and its issued history.
    /// None when nothing follows the highest (past `u64`), so none is ever given out twice.
    pub fn next_revision(&self, sheet: Option<u64>) -> Option<String> {
        let labels = self.rows.iter().chain(&self.issued).filter(|r| r.sheet == sheet).map(|r| r.revision.as_str());
        let highest = labels.filter_map(|l| l.parse::<u64>().ok()).max();
        highest.map_or(Some(u64::from(self.numbering.start)), |n| n.checked_add(1)).map(|n| n.to_string())
    }

    /// Adds row `id` for `sheet` (none unless the record is kept per sheet) with the next revision
    /// and nothing else: people type the rest.
    pub fn add(&mut self, sheet: Option<u64>, id: u64) -> Result<()> {
        require(self.rows.len() < MAX_ROWS, "a record has at most 1000 rows")?;
        require(sheet.is_some() == self.per_sheet, "a row has a sheet when the record is kept per sheet, and only then")?;
        let revision = self.next_revision(sheet).ok_or(Invalid("no revision follows the highest one"))?;
        require(!self.id_taken(id), "that id is taken")?;
        self.rows.push(RevisionRow { id, sheet, revision, ..RevisionRow::default() });
        Ok(())
    }

    /// Sets one of a row's texts as typed, by the cell text rules (a revision is one line).
    pub fn set(&mut self, id: u64, field: Field, text: &str) -> Result<()> {
        if field == Field::Revision {
            check_label(text).map_err(cell_rule)?;
        } else {
            check_text([text]).map_err(cell_rule)?;
        }
        *self.row_mut(id)?.field_mut(field) = text.to_string();
        Ok(())
    }

    /// Hides a row from the table, or shows it again.
    pub fn hide(&mut self, id: u64, hidden: bool) -> Result<()> {
        self.row_mut(id)?.hidden = hidden;
        Ok(())
    }

    /// Deletes a row: the issued history keeps any copy of it. The tags bound to it, by id, which
    /// keep the revision they showed, marked lost.
    pub fn delete(&mut self, id: u64) -> Result<Vec<u64>> {
        let at = self.rows.iter().position(|r| r.id == id).ok_or(Invalid("no row has that id"))?;
        let row = self.rows.remove(at);
        let mut affected = Vec::new();
        for tag in self.tags.iter_mut().filter(|t| t.row == id) {
            tag.lost = Some(tag.text.clone().unwrap_or_else(|| row.revision.clone()));
            affected.push(tag.id);
        }
        Ok(affected)
    }

    /// Appends a copy of a row, as it reads now, to the issued history, with the model version it
    /// was issued against when there is one. A row is issued once: a change is a new revision.
    pub fn issue(&mut self, id: u64, model_version: Option<ModelVersion>) -> Result<()> {
        require(self.issued.len() < MAX_ISSUED, "a record's issued history holds at most 10,000 entries")?;
        let mut row = self.rows.iter().find(|r| r.id == id).ok_or(Invalid("no row has that id"))?.clone();
        require(!self.is_issued(id), "a row is issued once: make a new revision")?;
        row.model_version = model_version;
        row.check()?;
        self.issued.push(row);
        Ok(())
    }

    /// Adds tag `id` bound to a row, at the default size.
    pub fn add_tag(&mut self, row: u64, id: u64) -> Result<()> {
        require(self.tags.len() < MAX_ISSUED, "a record has at most 10,000 tags")?;
        require(self.rows.iter().any(|r| r.id == row), "no row has that id")?;
        require(!self.id_taken(id), "that id is taken")?;
        self.tags.push(Tag { id, row, size_mm: DEFAULT_TAG_MM, text: None, lost: None });
        Ok(())
    }

    /// What a tag shows.
    pub fn tag_text(&self, tag: &Tag) -> TagText {
        if let Some(lost) = &tag.lost {
            return TagText::Lost(lost.clone());
        }
        match (&tag.text, self.rows.iter().find(|r| r.id == tag.row)) {
            (Some(text), _) => TagText::Override(text.clone()),
            (None, Some(row)) => TagText::Revision(row.revision.clone()),
            // A damaged file's tag of a row that isn't there.
            (None, None) => TagText::Lost(String::new()),
        }
    }

    /// The newest row for `sheet`, hidden or not (hiding only changes the table): what a title
    /// block's revision fields read.
    pub fn newest(&self, sheet: Option<u64>) -> Option<&RevisionRow> {
        self.rows.iter().rev().find(|r| r.sheet == sheet)
    }

    /// `sheet`'s shown rows as a revision table reads them, in order, each keyed by its id as a file
    /// writes it.
    pub fn table_rows(&self, sheet: Option<u64>) -> Vec<table::Row> {
        let fields =
            |row: &RevisionRow| Field::ALL.into_iter().filter_map(|f| Some((f.key().to_string(), row.field(f.key())?.to_string()))).collect();
        self.rows
            .iter()
            .filter(|r| r.sheet == sheet && !r.hidden)
            .map(|r| table::Row { key: crate::ids::encode(r.id), kind: RowKind::Part, purchased: false, fields: fields(r) })
            .collect()
    }
}

/// The revision table's default definition: a column for each field, heading at the top, every
/// column as wide as its text until a width is set.
pub fn table() -> Table {
    let column = |f: Field| Column {
        name: f.key().to_string(),
        heading: f.heading().to_string(),
        content: Content(vec![Part::Field { source: Source::Row, key: f.key().to_string(), format: cell::Format::default() }]),
        width_mm: None,
        align: HAlign::Left,
    };
    Table {
        columns: Field::ALL.into_iter().map(column).collect(),
        heading: Heading::Top,
        row_height_mm: 7.0,
        cap_mm: 2.5,
        outline_line_mm: 0.7,
        cell_line_mm: 0.35,
        filter: table::Filter::default(),
        overrides: Vec::new(),
    }
}

#[cfg(test)]
#[path = "revision_tests.rs"]
mod tests;
