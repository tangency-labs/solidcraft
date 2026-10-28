//! A drawing (#3890): sheets of paper that draw a design. A drawing lives in the file of the design
//! it draws ([`Document::drawings`](crate::Document::drawings)): a design container keeps each in
//! its own entry, a plain JSON design inline. Each is a record of its own family,
//! `"format": "solidcraft-drawing/N"`, versioned by the same code as designs ([`DRAWING`]), so a
//! change to the record is a drawing change, not a design one.
//!
//! - Each drawing has an **id**, unique among its design's drawings, and a design lists its
//!   drawings by name, then id ([`in_design`]): nothing to merge when two people add drawings.
//! - **Sources** are the designs it draws. For now a drawing draws the design it is in: at most one
//!   source, an id and nothing else. What the design was when the drawing last saw it is not saved
//!   here.
//! - **Sheets**: the paper, a copy of its drawing border, the resolution its pictures are rendered
//!   at, the component it documents, and whether it is left out of the sheet count or of printing.
//!   The border is a copy of the profile it was made with, never a name, so a later built-in never
//!   changes a saved drawing.
//! - **Units** follow each view's design or are one length unit for the whole drawing. The
//!   **language** is the content's, a BCP 47 tag chosen when the drawing is made, whatever the
//!   interface's is. **Properties** follow [`Metadata`]'s rules.
//!
//! The rules, which every later kind of record (views, annotations, tables) keeps too:
//! - Every object has an id of [`crate::ids`]: random, so objects added side by side never take
//!   the same one, and written as 13 characters of base32. No two objects share an id, and a run
//!   never gives a removed object's id again ([`Drawing::allocate_id`]).
//! - Every reference between records is an id and must resolve: a file with one that doesn't is
//!   damaged and refused, never patched.
//! - 1 to [`MAX_SHEETS`] sheets; names of 1 to [`MAX_NAME_BYTES`] bytes.
//! - Nothing derived is saved, and a drawing always saves as the same bytes: fields in their
//!   order, maps sorted, and lists of records in a fixed order (sheets as arranged; later
//!   records by id), never the order they were added in.
//! - A later change to the record is one file in `src/drawing/format/pending/` (its README).
//!
//! [`Drawing::from_json`] reads a drawing in this order: at most [`MAX_DRAWING_BYTES`] of text,
//! the family's format check, every number in range, the upgrade, the typed record, then
//! [`Drawing::validate`]; [`Drawing::from_value`] the same from the format check on.

mod sketch;

pub use sketch::{MAX_SHEET_SKETCHES, SheetSketch, SolvedSheetSketch};

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile};

use crate::format::DRAWING;
use crate::ids::{self, IdSource};
use crate::{DocError, Metadata, Result, expr};

/// The most drawings a design may hold.
pub const MAX_DRAWINGS: usize = 1000;
/// The most sheets a drawing may have.
pub const MAX_SHEETS: usize = 1000;
/// The longest name of a drawing or a sheet, in bytes.
pub const MAX_NAME_BYTES: usize = 256;
/// The longest language tag: what RFC 5646 asks every reader to take (§4.4.1).
pub const MAX_LANGUAGE_CHARS: usize = 35;
/// The largest drawing read, as JSON text.
pub const MAX_DRAWING_BYTES: usize = 16 << 20;
/// The resolution a sheet's pictures (its shaded views) are rendered at unless it says, in dots
/// per inch.
pub const DEFAULT_DPI: f64 = 300.0;
/// The lowest resolution a sheet may ask for.
pub const MIN_DPI: f64 = 25.0;
/// The highest resolution a sheet may ask for.
pub const MAX_DPI: f64 = 2400.0;

/// A drawing: its sheets and the designs they draw.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drawing {
    /// `solidcraft-drawing/N`: the format this build writes, whatever the file said.
    pub format: String,
    /// The changes of `format` this drawing has while they are pending
    /// ([`Family::pending_names`](crate::format::Family::pending_names)). Empty, and not saved,
    /// otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub format_pending: Vec<String>,
    /// Unique among its design's drawings: a design container's entry for it is named by it.
    #[serde(with = "crate::ids")]
    pub id: u64,
    pub name: String,
    /// The designs the drawing draws: for now at most one, the design it is in.
    pub sources: Vec<Source>,
    /// The drawing's properties and other tools' data, kept as they are.
    #[serde(default, skip_serializing_if = "Metadata::is_empty")]
    pub properties: Metadata,
    #[serde(default)]
    pub units: Units,
    /// The language of the drawing's content, a BCP 47 tag (`en-GB`).
    pub language: String,
    /// In order, each behind its own `Arc`, so an undo step shares every sheet it didn't change.
    pub sheets: Vec<Arc<Sheet>>,
    /// Authored sheet sketches, ordered by id on disk, with their original paper-mm payload.
    #[serde(default, skip_serializing_if = "Vec::is_empty", serialize_with = "sketch::serialize_sorted")]
    pub sketches: Vec<Arc<SheetSketch>>,
}

/// A design the drawing draws: the design the drawing is in. Views and sheets name it by its id,
/// so a drawing of other designs can add sources of another kind later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(with = "crate::ids")]
    pub id: u64,
}

/// The length unit a drawing gives its measurements in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Units {
    /// Each view's design's own units.
    #[default]
    FollowModel,
    /// One length unit for the whole drawing, by its name in [`expr::UNITS`] (`mm`, `in`).
    Fixed { unit: String },
}

/// One sheet of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    #[serde(with = "crate::ids")]
    pub id: u64,
    pub name: String,
    /// The paper: its size and the way it lies.
    pub sheet: drawing_sheet::Sheet,
    /// A copy of the drawing border's profile (`None`: no drawing border).
    pub border: Option<DrawingBorderProfile>,
    /// The resolution its pictures (its shaded views) are rendered at, in dots per inch.
    #[serde(default = "default_dpi")]
    pub dpi: f64,
    /// The component the sheet documents, which its title block reads.
    pub documents: Option<SheetComponent>,
    #[serde(default)]
    pub excluded: Excluded,
}

/// A component of one of the drawing's designs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetComponent {
    /// The [`Source`]'s id.
    #[serde(with = "crate::ids")]
    pub source: u64,
    /// The component's id in that design, as the design writes it (0: the design itself).
    pub component: u64,
}

/// What a sheet is left out of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Excluded {
    /// The sheet count ("sheet 2 of 3" on the other sheets).
    pub count: bool,
    /// Printing and exporting every sheet.
    pub print: bool,
}

fn default_dpi() -> f64 {
    DEFAULT_DPI
}

impl Drawing {
    /// A drawing of no designs yet, named `name` and written in `language`, with one sheet named
    /// `sheet`: the paper `paper` with a copy of `border`. Its id, and then its sheet's, come from
    /// `id_source`, the drawing's differing from every one in `taken` (its design's drawings').
    pub fn new(
        name: &str,
        language: &str,
        sheet: &str,
        paper: drawing_sheet::Sheet,
        border: Option<DrawingBorderProfile>,
        taken: &BTreeSet<u64>,
        id_source: &mut IdSource,
    ) -> Result<Drawing> {
        let id = id_source.next(taken)?;
        let sheet_id = id_source.next(&BTreeSet::from([id]))?;
        let first =
            Sheet { id: sheet_id, name: sheet.into(), sheet: paper, border, dpi: DEFAULT_DPI, documents: None, excluded: Excluded::default() };
        let drawing = Drawing {
            format: DRAWING.format_string(DRAWING.current()),
            format_pending: DRAWING.pending_names(),
            id,
            name: name.into(),
            sources: Vec::new(),
            properties: Metadata::default(),
            units: Units::default(),
            language: language.into(),
            sheets: vec![Arc::new(first)],
            sketches: Vec::new(),
        };
        drawing.validate()?;
        Ok(drawing)
    }

    /// Read a drawing's JSON (any format this build knows; older ones are upgraded). A drawing
    /// that breaks a rule of the record is damaged and refused, never patched.
    pub fn from_json(s: &str) -> Result<Drawing> {
        if s.len() > MAX_DRAWING_BYTES {
            return Err(DocError::Invalid(format!("drawing too large (at most {} MiB)", MAX_DRAWING_BYTES >> 20)));
        }
        let v: Value = serde_json::from_str(s).map_err(|e| DocError::Invalid(format!("not a SolidCraft drawing: {e}")))?;
        Drawing::from_value(v)
    }

    /// Read a drawing from its JSON value, as a plain JSON design holds it: [`Drawing::from_json`]
    /// once the text is parsed.
    pub fn from_value(mut v: Value) -> Result<Drawing> {
        let (version, applied) = DRAWING.take_version(&mut v)?;
        DRAWING.check_numbers(&v, "properties")?;
        DRAWING.upgrade(&mut v, version, applied.as_deref())?;
        sketch::check_raw(&v).map_err(damaged)?;
        let mut d: Drawing = serde_json::from_value(v).map_err(damaged)?;
        d.sketches.sort_by_key(|record| record.id);
        d.validate().map_err(damaged)?;
        Ok(Drawing { format: DRAWING.format_string(DRAWING.current()), format_pending: DRAWING.pending_names(), ..d })
    }

    /// The drawing's JSON: the same bytes for the same drawing. JSON longer than a read takes
    /// ([`MAX_DRAWING_BYTES`]) is refused, so every drawing saved opens again: other tools'
    /// properties are kept as they came, and indenting them can make a drawing that read in
    /// under the limit save over it.
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        let text = serde_json::to_string_pretty(self).map_err(|e| DocError::Invalid(format!("drawing not saved: {e}")))?;
        require(text.len() <= MAX_DRAWING_BYTES, || format!("drawing too large to save (at most {} MiB)", MAX_DRAWING_BYTES >> 20))?;
        Ok(text)
    }

    /// An id for a new object: one no object of the drawing has, from `id_source`, which never
    /// gives one twice.
    pub fn allocate_id(&self, id_source: &mut IdSource) -> Result<u64> {
        id_source.next(&self.ids().collect())
    }

    /// Read only this drawing's sketch. Identical local sketch ids in another drawing are unrelated.
    pub fn sheet_sketch(&self, id: u64) -> Option<&Arc<SheetSketch>> {
        self.sketches.iter().find(|record| record.id == id)
    }

    /// Every object's id, of every kind: what a new id must differ from. Each later kind adds its
    /// own here.
    fn ids(&self) -> impl Iterator<Item = u64> {
        std::iter::once(self.id)
            .chain(self.sources.iter().map(|s| s.id))
            .chain(self.sheets.iter().map(|s| s.id))
            .chain(self.sketches.iter().map(|s| s.id))
    }

    /// The record's rules (the module's notes): what reading a drawing checks, and what every edit
    /// keeps.
    pub fn validate(&self) -> Result<()> {
        self.validate_structure()?;
        sketch::check_drawing(self)
    }

    /// Ownership/header checks also used before staging one sketch edit. Deep
    /// payload validation remains at the full publication and file boundaries.
    fn validate_structure(&self) -> Result<()> {
        require(self.sketches.len() <= MAX_SHEET_SKETCHES, || "too many sheet sketches".into())?;
        require((1..=MAX_SHEETS).contains(&self.sheets.len()), || format!("a drawing has 1 to {MAX_SHEETS} sheets"))?;
        require(self.sources.len() <= 1, || "a drawing draws only the design it is in".into())?;
        check_name(&self.name, "the drawing's")?;
        require(is_language_tag(&self.language), || {
            format!("the language is not a BCP 47 tag of 2 to {MAX_LANGUAGE_CHARS} characters (such as `en` or `en-GB`)")
        })?;
        if let Units::Fixed { unit } = &self.units {
            // A length unit of the expression unit table, written as the table writes it.
            let length = unit.trim() == unit.as_str() && expr::unit_info(unit).is_some_and(|(kind, _)| kind == expr::Kind::Length);
            require(length, || "the drawing's unit is not a length unit (such as `mm` or `in`)".into())?;
        }
        let sheets: BTreeSet<_> = self.sheets.iter().map(|sheet| sheet.id).collect();
        for record in &self.sketches {
            require(sheets.contains(&record.sheet), || "sheet sketch owner does not exist in this drawing".into())?;
        }
        let mut seen = BTreeSet::new();
        for id in self.ids() {
            require(id != 0, || "an object has the id 0, which is never one".into())?;
            require(seen.insert(id), || format!("two objects have the id {}", ids::encode(id)))?;
        }
        for sheet in &self.sheets {
            self.check_sheet(sheet).map_err(|e| DocError::Invalid(format!("sheet {}: {e}", ids::encode(sheet.id))))?;
        }
        Ok(())
    }

    /// A sheet's own rules, and that the source it documents is one of the drawing's.
    fn check_sheet(&self, sheet: &Sheet) -> Result<()> {
        check_name(&sheet.name, "its")?;
        sheet.sheet.size_mm().map_err(from_sheet)?;
        if let Some(border) = &sheet.border {
            border.validate().map_err(from_sheet)?;
        }
        require((MIN_DPI..=MAX_DPI).contains(&sheet.dpi), || format!("its pictures are {MIN_DPI} to {MAX_DPI} dpi"))?;
        match sheet.documents {
            Some(SheetComponent { source, .. }) => {
                let named = || format!("it documents source {}, which the drawing doesn't have", ids::encode(source));
                require(self.sources.iter().any(|s| s.id == source), named)
            }
            None => Ok(()),
        }
    }
}

/// A design's drawings as read, each checked on its own already: in their order (by name, then id)
/// and keeping [`check_in_design`]'s rules.
pub fn in_design(drawings: Vec<Drawing>) -> Result<Vec<Arc<Drawing>>> {
    let mut drawings: Vec<_> = drawings.into_iter().map(Arc::new).collect();
    sort(&mut drawings);
    check_in_design(&drawings)?;
    Ok(drawings)
}

/// Put a design's drawings in their order: by name, then id. Nothing stores the order, so two
/// people adding drawings have nothing to merge.
pub fn sort(drawings: &mut [Arc<Drawing>]) {
    drawings.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
}

/// The rules for a design's drawings: at most [`MAX_DRAWINGS`], and no two with one id.
pub fn check_in_design(drawings: &[Arc<Drawing>]) -> Result<()> {
    check_count(drawings.len())?;
    let mut seen = BTreeSet::new();
    drawings.iter().try_for_each(|d| require(seen.insert(d.id), || format!("two drawings have the id {}", ids::encode(d.id))))
}

/// A design of `count` drawings keeps the cap: what a reader checks before it reads any of them.
pub fn check_count(count: usize) -> Result<()> {
    require(count <= MAX_DRAWINGS, || format!("a design holds at most {MAX_DRAWINGS} drawings"))
}

/// `Ok` when `ok`, else the rule `broken` names.
fn require(ok: bool, broken: impl FnOnce() -> String) -> Result<()> {
    if ok { Ok(()) } else { Err(DocError::Invalid(broken())) }
}

/// A name: 1 to [`MAX_NAME_BYTES`] bytes, not only spaces, without control characters.
fn check_name(name: &str, whose: &str) -> Result<()> {
    require(!name.trim().is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control), || {
        format!("{whose} name is 1 to {MAX_NAME_BYTES} bytes, not only spaces, without control characters")
    })
}

/// A well-formed language tag, as far as a drawing needs (RFC 5646 §2.1): 2 to
/// [`MAX_LANGUAGE_CHARS`] characters, subtags of 1 to 8 ASCII letters and digits joined by `-`,
/// the first of 2 to 8 letters (`en`, `en-GB`, `zh-Hans-CN`). Whether the subtags are registered
/// isn't checked, and a private-use tag alone (`x-…`) isn't taken.
fn is_language_tag(tag: &str) -> bool {
    let mut subtags = tag.split('-');
    let first = subtags.next().unwrap_or_default();
    (2..=MAX_LANGUAGE_CHARS).contains(&tag.len())
        && (2..=8).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_alphabetic())
        && subtags.all(|s| (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// A file that breaks a rule of the record.
fn damaged(e: impl std::fmt::Display) -> DocError {
    DocError::Invalid(format!("damaged drawing: {e}"))
}

fn from_sheet(e: drawing_sheet::Invalid) -> DocError {
    DocError::Invalid(e.to_string())
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
