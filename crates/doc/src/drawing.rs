//! A drawing (#3890): sheets of paper that draw a design. A drawing lives in the file of the design
//! it draws ([`Document::drawings`](crate::Document::drawings)): a design container keeps each in
//! its own entry, a plain JSON design inline. Each is a record of its own family,
//! `"format": "solidcraft-drawing/N"`, versioned by the same code as designs ([`DRAWING`]), so a
//! change to the record is a drawing change, not a design one.
//!
//! - Each drawing has an **id**, unique among its design's drawings, and a design lists its
//!   drawings by name, then id ([`in_design`]): nothing to merge when two people add drawings.
//! - **Sources** are the designs it draws, by id: the design it is in (at most one source, with no
//!   path) and other design files, each by its path ([`Source::path`]), at most [`MAX_SOURCES`] in
//!   all. What a design was when the drawing last saw it is not saved here.
//! - **Sheets**: the paper, a copy of its drawing border and of its title block with the text typed
//!   over its cells' values, the resolution its pictures are rendered at, the component it
//!   documents, and whether it is left out of the sheet count or of printing. The border and the
//!   block are copies of what they were made with, never names, so a later built-in never changes a
//!   saved drawing.
//! - **Units** follow each view's design or are one length unit for the whole drawing. The
//!   **language** is the content's, a BCP 47 tag chosen when the drawing is made, whatever the
//!   interface's is. **Properties** follow [`Metadata`]'s rules.
//! - The **font** its text is set in: a font family's name, never a file, so the drawing travels
//!   (none: the built-in font; [`solidcraft_sketch::check_family`]).
//! - **Views** of its designs on its sheets, base and projected ([`view`]), and the **projection**
//!   (first or third angle) every projected view follows.
//! - **Dimensions** on its views, each measuring named corners or edges of the view's model
//!   ([`dimension`]).
//! - **Sketches** authored on sheets, in paper millimetres, stored with the shared sketch payload
//!   ([`SheetSketch`]) and independent of every model sketch and view scale.
//! - **Balloons** on its views, each pointing at a placed part by a named edge of it ([`balloon`]).
//! - **Images** on its sheets, each file kept once however many places it sits ([`image`]).
//! - **Tables** on its sheets: parts lists and revision tables ([`table`]).
//! - **Centre marks and lines** on views, attached to named model references ([`centre_mark`],
//!   [`centre_line`]).
//! - **Revisions**: one revision record for the whole drawing ([`RevisionRecord`]), its rows in the
//!   order they were added, the newest last.
//!
//! The rules, which every later kind of record (views, annotations, tables) keeps too:
//! - Every object has an id of [`crate::ids`]: random, so objects added side by side never take
//!   the same one, and written as 13 characters of base32. No two objects share an id, and a run
//!   never gives a removed object's id again ([`Drawing::allocate_id`]).
//! - Every reference between records is an id and must resolve: a file with one that doesn't is
//!   damaged and refused, never patched.
//! - 1 to [`MAX_SHEETS`] sheets; names of 1 to [`MAX_NAME_BYTES`] bytes.
//! - Nothing derived is saved, and a drawing always saves as the same bytes: fields in their
//!   order, maps sorted, and lists of records in a fixed order (sheets as arranged; sources and
//!   later records by id), never the order they were added in. Records listed by id read in that
//!   order whatever order a file lists them in: the order means nothing, so it is never damage.
//! - A later change to the record is one file in `src/drawing/format/pending/` (its README).
//!
//! [`Drawing::from_json`] reads a drawing in this order: at most [`MAX_DRAWING_BYTES`] of text,
//! the family's format check, every number in range, the upgrade, the typed record with its views,
//! dimensions, balloons, images and notes by id, then [`Drawing::validate`] and each image's file
//! ([`Drawing::check_image_files`]); [`Drawing::from_value`] the same from the format check on.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore_record::Vec2;
use solidcraft_geom::Aabb2;
use solidcraft_geom::cell::Projection;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile};
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_geom::revision::RevisionRecord;
use solidcraft_geom::title_block::{TitleBlock, TitleBlockLayout};

use crate::format::{CheckNumbers as _, DRAWING};
use crate::ids::{self, IdSource};
use crate::{DocError, Metadata, Result, expr};

pub mod auto_marks;
pub mod balloon;
pub mod by_id;
pub mod centre_line;
pub mod centre_mark;
pub mod coverage;
pub mod cutting_line;
pub mod dimension;
pub mod fields;
mod fill;
pub mod fold;
pub mod image;
mod newer;
pub mod note;
pub mod projection_symbol;
pub mod sheet_set;
mod sketch;
pub mod symbol;
pub mod table;
pub mod template;
pub mod thread_lines;
pub mod view;

pub use fill::{
    FillPaint, FillRegionStatus, MAX_FILL_REGION_PIECES, MAX_FILLED_SKETCHES, MAX_PATTERN_SNAPSHOTS, MAX_SKETCH_FILLS, PatternSnapshot,
    ResolvedSketchFill, SheetFillProfile, SheetFillProfiles, SketchFill,
};
pub use newer::NewerDrawing;
pub use sketch::{MAX_SHEET_SKETCHES, Owner, SheetSketch, SolvedSheetSketch};

use balloon::Balloon;
use by_id::ById;
use centre_line::CentreLine;
use centre_mark::CentreMark;
use dimension::Dimension;
use image::{ImageFile, SheetImage};
use note::{Anchor, Note};
use symbol::Symbol;
use table::SheetTable;
use view::{View, ViewLetters};

/// The most drawings a design may hold.
pub const MAX_DRAWINGS: usize = 1000;
/// The most designs a drawing may draw.
pub const MAX_SOURCES: usize = 64;
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

/// When a drawing's views follow what they draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Updates {
    /// As it changes.
    #[default]
    Automatic,
    /// When asked (`drawing.update`): until then each view shows its last lines, out of date.
    Deferred,
}

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
    /// The designs the drawing draws, by id ([`Source`]).
    pub sources: Vec<Source>,
    /// The drawing's properties and other tools' data, kept as they are, behind an `Arc` as the
    /// sheets are, so an undo step shares them while they don't change.
    #[serde(default, skip_serializing_if = "no_properties")]
    pub properties: Arc<Metadata>,
    #[serde(default)]
    pub units: Units,
    /// First or third angle: what every projected view shows, and the title block's symbol.
    #[serde(default)]
    pub projection: Projection,
    /// When its views follow a changed model. Saved only when deferred.
    #[serde(default, skip_serializing_if = "follows_at_once")]
    pub updates: Updates,
    /// The language of the drawing's content, a BCP 47 tag (`en-GB`).
    pub language: String,
    /// The font family its text is set in, by name; empty: the built-in font.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub font: String,
    /// The ISO 128 line group its lines are drawn in, unless a sheet or a view chooses another
    /// ([`Drawing::resolve_view`]). Always written, so a later default can't change an old drawing.
    #[serde(default)]
    pub line_group: LineGroup,
    /// How its views' letters are given ([`Drawing::give_letter`]). Saved only when it isn't the default.
    #[serde(default, skip_serializing_if = "ViewLetters::is_default")]
    pub view_letters: ViewLetters,
    /// In order, each behind its own `Arc`, so an undo step shares every sheet it didn't change.
    pub sheets: Vec<Arc<Sheet>>,
    /// By id ([`Drawing::add_views`]), each behind its own `Arc`, as the sheets are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<Arc<View>>,
    /// By id ([`Drawing::add_dimensions`]), each behind its own `Arc`, as the views are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimensions: Vec<Arc<Dimension>>,
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub balloons: ById<Balloon>,
    /// The image files, once each however many places they sit.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub images: ById<ImageFile>,
    /// Where they sit on the sheets.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub sheet_images: ById<SheetImage>,
    /// The saved tables on its sheets: parts lists and revision tables.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub tables: ById<SheetTable>,
    /// The notes on its sheets and in its views.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub notes: ById<Note>,
    /// Manually placed centre marks, by id, each attached to a view's model edge.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub centre_marks: ById<CentreMark>,
    /// Manual centre lines, by id.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub centre_lines: ById<CentreLine>,
    /// The symbols of its own it places: each made once, however many places it sits.
    #[serde(default, skip_serializing_if = "ById::is_empty")]
    pub symbols: ById<Symbol>,
    /// Behind an `Arc` as the sheets are, so an undo step shares it while it doesn't change.
    #[serde(default, skip_serializing_if = "no_revisions")]
    pub revisions: Arc<RevisionRecord>,
    /// Authored sheet sketches, ordered by id on disk, with their original paper-mm payload.
    #[serde(default, skip_serializing_if = "Vec::is_empty", serialize_with = "sketch::serialize_sorted")]
    pub sketches: Vec<Arc<SheetSketch>>,
    /// Exact whole-profile fills, with last admitted regions retained on loss.
    #[serde(default, skip_serializing_if = "Vec::is_empty", serialize_with = "fill::serialize_fills")]
    pub fills: Vec<Arc<SketchFill>>,
    /// Imported or original pattern snapshots; names need not be unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty", serialize_with = "fill::serialize_patterns")]
    pub patterns: Vec<Arc<PatternSnapshot>>,
    /// Set when a newer SolidCraft saved the file this came from.
    #[serde(skip)]
    pub newer_file: Option<Arc<NewerDrawing>>,
}

/// The fields a drawing's record has: anything else in a file is a newer build's.
const RECORD_FIELDS: &[&str] = &[
    "format",
    "format_pending",
    "id",
    "name",
    "sources",
    "properties",
    "units",
    "projection",
    "updates",
    "language",
    "font",
    "line_group",
    "view_letters",
    "sheets",
    "views",
    "dimensions",
    "balloons",
    "images",
    "sheet_images",
    "sketches",
    "fills",
    "patterns",
    "tables",
    "notes",
    "centre_marks",
    "centre_lines",
    "symbols",
    "revisions",
    // Where image files, placements and logos are set aside (`image::set_aside`).
    "image_files",
    "placed_images",
    "logo_cells",
];

/// A design the drawing draws: the design the drawing is in (no `path`), or another design file.
/// Views, sheets and tables name it by its id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The design file: whole in memory; in a file relative to the folder of the design the
    /// drawing is in, with `/` (whole when the two share no root), as a component link keeps its
    /// file. `None`: the design the drawing is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// A source's path: not empty, not only spaces, without control characters. Its length is
/// bounded by the drawing's only, as a component link's is: a design deep in folders makes it long.
pub fn check_source_path(path: &str) -> Result<()> {
    require(!path.trim().is_empty() && !path.chars().any(char::is_control), || {
        "a source's path is not empty, not only spaces, without control characters".into()
    })
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
    /// A copy of its title block's definition (`None`: no title block), on the drawing border's
    /// lower-right corner, so only a sheet with a border has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_block: Option<TitleBlock>,
    /// Text shown in place of a title block cell's value, by the cell's name: the value is still
    /// worked out, so it can be shown beside it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub title_block_overrides: BTreeMap<String, String>,
    /// The resolution its pictures (its shaded views) are rendered at, in dots per inch.
    #[serde(default = "default_dpi")]
    pub dpi: f64,
    /// The component the sheet documents, which its title block reads.
    pub documents: Option<SheetComponent>,
    #[serde(default)]
    pub excluded: Excluded,
    /// Its own line group: none follows the drawing's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_group: Option<LineGroup>,
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

impl Sheet {
    /// Where the sheet's drawing goes, in paper millimetres: inside its drawing border, or the
    /// whole paper when it has none. Nothing on a sheet is saved against its border, so a change
    /// of paper regrows the border and its zones by itself.
    pub fn drawing_space(&self) -> Result<Aabb2> {
        match &self.border {
            Some(border) => Ok(border.layout(&self.sheet).map_err(from_sheet)?.drawing_border),
            None => Ok(Aabb2 { min: Vec2::ZERO, max: self.sheet.size_mm().map_err(from_sheet)? }),
        }
    }

    /// Where its title block sits, in paper millimetres (none without one), once its drawing border
    /// and the block on it lay out on the paper: the rule a sheet is held to, as drawing them uses it.
    pub fn title_block_outline(&self) -> Result<Option<Aabb2>> {
        Ok(self.title_block_layout()?.map(|layout| layout.outline))
    }

    /// Its title block laid out on its drawing border (none without a block): each cell's place.
    pub fn title_block_layout(&self) -> Result<Option<TitleBlockLayout>> {
        let border = self.border.as_ref().map(|border| border.layout(&self.sheet)).transpose().map_err(from_sheet)?;
        let Some(block) = &self.title_block else { return Ok(None) };
        let border = border.ok_or_else(|| DocError::Invalid("its title block sits on a drawing border, and it has none".into()))?;
        Ok(Some(block.layout(&border).map_err(|e| DocError::Invalid(e.to_string()))?))
    }

    /// Gives the sheet `block` (`None`: no title block). Text typed over a cell stays while `block`
    /// has a text cell of that name; answers the cells whose typed text is dropped.
    pub fn set_title_block(&mut self, block: Option<TitleBlock>) -> Vec<String> {
        let kept = |name: &String| fields::overridable(block.as_ref(), name);
        let lost = self.title_block_overrides.keys().filter(|name| !kept(name)).cloned().collect();
        self.title_block_overrides.retain(|name, _| kept(name));
        self.title_block = block;
        lost
    }
}

/// What is on one of a drawing's sheets ([`Drawing::on_sheet`]), each kind by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnSheet {
    pub views: Vec<u64>,
    /// The dimensions in its views.
    pub dimensions: Vec<u64>,
    /// The balloons in its views.
    pub balloons: Vec<u64>,
    /// The images placed on it.
    pub images: Vec<u64>,
    /// The revision tags on it.
    pub tags: Vec<u64>,
    /// The tables on it.
    pub tables: Vec<u64>,
    /// The notes on it and in its views.
    pub notes: Vec<u64>,
    /// The manual centre marks in its views.
    pub centre_marks: Vec<u64>,
    /// The manual centre lines in its views.
    pub centre_lines: Vec<u64>,
    /// The sketches on it and in its views, each in its owner's frame: paper millimetres for the
    /// sheet's own, model millimetres in the view's plane for a view's.
    pub sketches: Vec<u64>,
    pub fills: Vec<u64>,
}

impl OnSheet {
    /// Nothing is on the sheet.
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
            && self.dimensions.is_empty()
            && self.balloons.is_empty()
            && self.images.is_empty()
            && self.tags.is_empty()
            && self.tables.is_empty()
            && self.notes.is_empty()
            && self.centre_marks.is_empty()
            && self.centre_lines.is_empty()
            && self.sketches.is_empty()
            && self.fills.is_empty()
    }
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
        let first = Sheet {
            id: sheet_id,
            name: sheet.into(),
            sheet: paper,
            border,
            title_block: None,
            title_block_overrides: BTreeMap::new(),
            dpi: DEFAULT_DPI,
            documents: None,
            excluded: Excluded::default(),
            line_group: None,
        };
        let drawing = Drawing {
            format: DRAWING.format_string(DRAWING.current()),
            format_pending: DRAWING.pending_names(),
            id,
            name: name.into(),
            sources: Vec::new(),
            properties: Arc::default(),
            units: Units::default(),
            projection: Projection::default(),
            updates: Updates::default(),
            language: language.into(),
            font: String::new(),
            line_group: LineGroup::DEFAULT,
            view_letters: ViewLetters::default(),
            sheets: vec![Arc::new(first)],
            views: Vec::new(),
            dimensions: Vec::new(),
            balloons: ById::default(),
            images: ById::default(),
            sheet_images: ById::default(),
            tables: ById::default(),
            notes: ById::default(),
            centre_marks: ById::default(),
            centre_lines: ById::default(),
            symbols: ById::default(),
            revisions: Arc::default(),
            newer_file: None,
            sketches: Vec::new(),
            fills: Vec::new(),
            patterns: Vec::new(),
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
        Self::read_value(v, Some(s))
    }

    /// Read a drawing from its JSON value, as a plain JSON design holds it: [`Drawing::from_json`]
    /// once the text is parsed.
    pub fn from_value(v: Value) -> Result<Drawing> {
        Self::read_value(v, None)
    }

    pub(crate) fn read_value(mut v: Value, text: Option<&str>) -> Result<Drawing> {
        let (version, applied) = DRAWING.take_version(&mut v)?;
        if let Some(newer) = DRAWING.newer(version, applied.as_deref()) {
            return Self::from_newer(v, version, applied, newer, text);
        }
        let d = Self::decode(v, version, applied.as_deref())?;
        Ok(Drawing { format: DRAWING.format_string(DRAWING.current()), format_pending: DRAWING.pending_names(), ..d })
    }

    fn decode(mut v: Value, version: u32, applied: Option<&[String]>) -> Result<Drawing> {
        DRAWING.check_numbers(&v, "properties")?;
        DRAWING.upgrade(&mut v, version, applied)?;
        Self::decode_record(v)
    }

    fn decode_record(mut v: Value) -> Result<Drawing> {
        image::take_back(&mut v);
        sketch::check_raw(&v).map_err(damaged)?;
        fill::check_raw(&v).map_err(damaged)?;
        let mut d: Drawing = serde_json::from_value(v).map_err(damaged)?;
        d.sources.sort_by_key(|s| s.id);
        let (views, dimensions) = (std::mem::take(&mut d.views), std::mem::take(&mut d.dimensions));
        d.add_views(views);
        d.add_dimensions(dimensions);
        d.balloons.put_in_order();
        d.images.put_in_order();
        d.sheet_images.put_in_order();
        d.tables.put_in_order();
        d.notes.put_in_order();
        d.centre_marks.put_in_order();
        d.centre_lines.put_in_order();
        d.sketches.sort_by_key(|record| record.id);
        d.fills.sort_by_key(|record| record.id);
        d.patterns.sort_by_key(|record| record.id);
        d.symbols.put_in_order();
        d.validate().map_err(damaged)?;
        d.check_image_files().map_err(damaged)?;
        Ok(d)
    }

    /// Open the readable records of a newer drawing and keep the rest for the next save.
    fn from_newer(mut v: Value, version: u32, applied: Option<Vec<String>>, newer: crate::format::Newer, text: Option<&str>) -> Result<Drawing> {
        let mut file = NewerDrawing::new(newer);
        let d = DRAWING
            .upgrade(&mut v, version, applied.as_deref())
            .and_then(|()| file.salvage(&mut v, text))
            .and_then(|()| DRAWING.check_numbers(&v, "properties"))
            .and_then(|()| Self::decode_record(v))
            .map_err(|why| {
                DocError::Invalid(format!(
                    "this drawing was saved by a newer SolidCraft (format {}, this version reads up to {}) and this version can't read it: {why}. Update SolidCraft to open it",
                    file.format, file.reads_up_to
                ))
            })?;
        file.remember_references(&d);
        let (format, format_pending) = if file.format > file.reads_up_to {
            (DRAWING.format_string(file.format), applied.unwrap_or_default())
        } else {
            let mut pending = DRAWING.pending_names();
            pending.extend(file.unknown_changes.iter().cloned());
            (DRAWING.format_string(DRAWING.current()), pending)
        };
        Ok(Drawing { format, format_pending, newer_file: Some(Arc::new(file)), ..d })
    }

    /// The drawing's JSON: the same bytes for the same drawing. JSON longer than a read takes
    /// ([`MAX_DRAWING_BYTES`]) is refused, so every drawing saved opens again: other tools'
    /// properties are kept as they came, and indenting them can make a drawing that read in
    /// under the limit save over it.
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        let unsaved = |e: serde_json::Error| DocError::Invalid(format!("drawing not saved: {e}"));
        let text = serde_json::to_string_pretty(&self.saved_record()?).map_err(unsaved)?;
        require(text.len() <= MAX_DRAWING_BYTES, || format!("drawing too large to save (at most {} MiB)", MAX_DRAWING_BYTES >> 20))?;
        Ok(text)
    }

    fn saved_record(&self) -> Result<newer::SavedValue<'_>> {
        let mut value = serde_json::to_value(self).map_err(|e| DocError::Invalid(format!("drawing not saved: {e}")))?;
        image::set_aside(&mut value);
        match &self.newer_file {
            Some(file) => file.saved_record(value),
            None => Ok(newer::SavedValue::Value(value)),
        }
    }

    /// Images and opaque container parts also need a container when the drawing is in a design.
    pub(crate) fn has_binary_data(&self) -> bool {
        !self.images.is_empty() || self.newer_file.as_ref().is_some_and(|file| file.part_names().next().is_some())
    }

    /// An id for a new object: one no object of the drawing has, from `id_source`, which never
    /// gives one twice.
    pub fn allocate_id(&self, id_source: &mut IdSource) -> Result<u64> {
        id_source.next(&self.object_ids().collect())
    }

    /// The position of the sheet `named` names: by id, else by a name only one sheet has.
    pub fn sheet_index(&self, named: &str) -> Result<usize> {
        by_id::named_index(&self.sheets, named, "sheet", |sheet| sheet.id, |sheet| &sheet.name)
    }

    /// Whether `id` names an object a sheet shows.
    pub fn has_object(&self, id: u64) -> bool {
        self.view(id).is_some()
            || self.dimension(id).is_some()
            || self.balloons.by_id(id).is_some()
            || self.centre_mark(id).is_some()
            || self.centre_line(id).is_some()
            || self.notes.by_id(id).is_some()
    }

    /// What is on sheet `id`: its views and their annotations, and its images, sketches and fills.
    /// Each later kind of record on a sheet adds its own here, and goes with the sheet
    /// ([`Drawing::remove_sheet`]).
    pub fn on_sheet(&self, id: u64) -> OnSheet {
        // The views are listed by id, so the sheet's are too, and so are its images, revision tags, tables and notes.
        let mut on = self.with_contents(self.views.iter().filter(|v| v.sheet == id).map(|v| v.id).collect());
        on.images = self.sheet_images.iter().filter(|i| i.sheet == id).map(|i| i.id).collect();
        on.tags = self.revisions.tags().iter().filter(|t| t.sheet == id).map(|t| t.id).collect();
        on.tables = self.tables.iter().filter(|t| t.sheet == id).map(|t| t.id).collect();
        let in_views = |n: &Note| n.anchor.view().is_some_and(|v| on.views.binary_search(&v).is_ok());
        on.notes = self.notes.iter().filter(|n| n.anchor == Anchor::Sheet(id) || in_views(n)).map(|n| n.id).collect();
        on.sketches = self.sketches.iter().filter(|record| self.sketch_sheet(record) == Some(id)).map(|record| record.id).collect();
        on.sketches.sort_unstable();
        on.fills = self.fills.iter().filter(|fill| on.sketches.binary_search(&fill.sketch).is_ok()).map(|fill| fill.id).collect();
        on.fills.sort_unstable();
        on
    }

    /// `views`, listed by id, with the dimensions, balloons, notes, manual centre marks, centre lines
    /// and sketches in them, and those sketches' fills. An image sits on a sheet, never in a view, so
    /// there are none.
    pub fn with_contents(&self, views: Vec<u64>) -> OnSheet {
        let dimensions = self.dimensions.iter().filter(|d| views.binary_search(&d.view).is_ok()).map(|d| d.id).collect();
        let balloons = self.balloons.iter().filter(|b| views.binary_search(&b.view).is_ok()).map(|b| b.id).collect();
        let notes = self.notes.iter().filter(|n| n.anchor.view().is_some_and(|v| views.binary_search(&v).is_ok())).map(|n| n.id).collect();
        let centre_marks = self.centre_marks.iter().filter(|m| views.binary_search(&m.view).is_ok()).map(|m| m.id).collect();
        let centre_lines = self.centre_lines.iter().filter(|line| views.binary_search(&line.view).is_ok()).map(|line| line.id).collect();
        let in_views = |record: &&Arc<SheetSketch>| record.owner.view().is_some_and(|view| views.binary_search(&view).is_ok());
        let mut sketches: Vec<u64> = self.sketches.iter().filter(in_views).map(|record| record.id).collect();
        sketches.sort_unstable();
        let mut fills: Vec<u64> = self.fills.iter().filter(|fill| sketches.binary_search(&fill.sketch).is_ok()).map(|fill| fill.id).collect();
        fills.sort_unstable();
        OnSheet {
            views,
            dimensions,
            centre_marks,
            centre_lines,
            balloons,
            images: Vec::new(),
            tags: Vec::new(),
            tables: Vec::new(),
            notes,
            sketches,
            fills,
        }
    }

    /// Takes sheet `id` out with everything on it ([`Drawing::on_sheet`]), and answers what that
    /// was. A view on another sheet projected from one of its views is left without its parent,
    /// which [`Drawing::validate`] refuses: what becomes of it is the caller's to decide first.
    pub fn remove_sheet(&mut self, id: u64) -> OnSheet {
        let on = self.on_sheet(id);
        self.sheets.retain(|s| s.id != id);
        self.remove_objects(&on);
        on
    }

    /// Takes `objects` out, each list by id: the one rule a sheet's delete and a selection's delete
    /// remove by, with the image files nothing uses any more. As in [`Drawing::remove_sheet`], what is
    /// left must still keep the record's rules.
    pub fn remove_objects(&mut self, objects: &OnSheet) {
        self.views.retain(|v| objects.views.binary_search(&v.id).is_err());
        self.dimensions.retain(|d| objects.dimensions.binary_search(&d.id).is_err());
        self.balloons.retain(|b| objects.balloons.binary_search(&b.id).is_err());
        self.centre_marks.retain(|mark| objects.centre_marks.binary_search(&mark.id).is_err());
        self.centre_lines.retain(|line| objects.centre_lines.binary_search(&line.id).is_err());
        self.sheet_images.retain(|i| objects.images.binary_search(&i.id).is_err());
        self.drop_unused_images();
        self.tables.retain(|t| objects.tables.binary_search(&t.id).is_err());
        self.notes.retain(|n| objects.notes.binary_search(&n.id).is_err());
        // A balloon outlives the list it named, and goes back to the lists it could read.
        for balloon in self.balloons.iter_mut() {
            if balloon.parts_list.is_some_and(|list| objects.tables.binary_search(&list).is_ok()) {
                Arc::make_mut(balloon).parts_list = None;
            }
        }
        if !objects.tags.is_empty() {
            Arc::make_mut(&mut self.revisions).delete_tags(&objects.tags);
        }
        self.sketches.retain(|s| objects.sketches.binary_search(&s.id).is_err());
        self.fills.retain(|fill| objects.fills.binary_search(&fill.id).is_err());
    }

    /// Read only this drawing's sketch. Identical local sketch ids in another drawing are unrelated.
    pub fn sheet_sketch(&self, id: u64) -> Option<&Arc<SheetSketch>> {
        self.sketches.iter().find(|record| record.id == id)
    }

    /// The sheet `record` is on: its own sheet, or its view's. None when its owner is not one of
    /// this drawing's sheets or views.
    pub fn sketch_sheet(&self, record: &SheetSketch) -> Option<u64> {
        match record.owner {
            Owner::Sheet(sheet) => self.sheets.iter().any(|s| s.id == sheet).then_some(sheet),
            Owner::View(view) => self.view(view).map(|v| v.sheet),
        }
    }

    /// The drawing-global namespace, including ids reserved by newer records.
    pub fn object_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.ids().chain(self.newer_file.iter().flat_map(|file| file.reserved.iter().copied()))
    }

    fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        std::iter::once(self.id)
            .chain(self.sources.iter().map(|s| s.id))
            .chain(self.sheets.iter().map(|s| s.id))
            .chain(self.views.iter().map(|v| v.id))
            .chain(self.dimensions.iter().map(|d| d.id))
            .chain(self.balloons.iter().map(|b| b.id))
            .chain(self.images.iter().map(|i| i.id))
            .chain(self.sheet_images.iter().map(|i| i.id))
            .chain(self.tables.iter().map(|t| t.id))
            .chain(self.notes.iter().map(|n| n.id))
            .chain(self.centre_marks.iter().map(|m| m.id))
            .chain(self.centre_lines.iter().map(|line| line.id))
            .chain(self.symbols.iter().map(|s| s.id))
            .chain(self.revisions.ids())
            .chain(self.sketches.iter().map(|s| s.id))
            .chain(self.fills.iter().map(|fill| fill.id))
            .chain(self.patterns.iter().map(|pattern| pattern.id))
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
        self.check_sources()?;
        check_name(&self.name, "the drawing's")?;
        require(is_language_tag(&self.language), || {
            format!("the language is not a BCP 47 tag of 2 to {MAX_LANGUAGE_CHARS} characters (such as `en` or `en-GB`)")
        })?;
        solidcraft_sketch::check_family(&self.font).map_err(|e| DocError::Invalid(format!("the drawing's font: {e}")))?;
        if let Units::Fixed { unit } = &self.units {
            // A length unit of the expression unit table, written as the table writes it.
            let length = unit.trim() == unit.as_str() && expr::unit_info(unit).is_some_and(|(kind, _)| kind == expr::Kind::Length);
            require(length, || "the drawing's unit is not a length unit (such as `mm` or `in`)".into())?;
        }
        for record in &self.sketches {
            require(self.sketch_sheet(record).is_some(), || "drawing sketch owner is not one of this drawing's sheets or views".into())?;
        }
        let mut seen = BTreeSet::new();
        for id in self.ids() {
            require(id != 0, || "an object has the id 0, which is never one".into())?;
            require(seen.insert(id), || format!("two objects have the id {}", ids::encode(id)))?;
        }
        if let Some(file) = &self.newer_file {
            file.check_edits(self, &seen)?;
        }
        for sheet in &self.sheets {
            self.check_sheet(sheet).map_err(|e| DocError::Invalid(format!("sheet {}: {e}", ids::encode(sheet.id))))?;
        }
        // One record for the whole drawing until sheets keep their own (#3874).
        require(!self.revisions.per_sheet, || "a drawing keeps one revision record for all its sheets".into())?;
        self.revisions.validate().map_err(|e| DocError::Invalid(e.to_string()))?;
        require(self.revisions.tags().iter().all(|t| self.sheets.iter().any(|s| s.id == t.sheet)), || {
            "a revision tag is on one of the drawing's sheets".into()
        })?;
        self.check_views()?;
        self.check_dimensions()?;
        self.check_tables()?;
        self.check_balloons()?;
        self.check_centre_marks()?;
        self.check_centre_lines()?;
        self.check_images()?;
        self.check_notes()?;
        self.check_symbols()?;
        fill::check_drawing(self)
    }

    /// The sources' rules: at most [`MAX_SOURCES`], by id, the design the drawing is in at most once,
    /// and each path keeping [`check_source_path`]. Whether two paths name one file is not judged
    /// here: only the file system can say (`a/../p.sc` and `p.sc`), so the engine refuses one file
    /// twice when a source is added, and a record holding it still opens and can be edited.
    fn check_sources(&self) -> Result<()> {
        require(self.sources.len() <= MAX_SOURCES, || format!("a drawing draws at most {MAX_SOURCES} designs"))?;
        require(self.sources.is_sorted_by_key(|s| s.id), || "a drawing lists its sources by id".into())?;
        let own = self.sources.iter().filter(|s| s.path.is_none()).count();
        require(own <= 1, || "a drawing draws the design it is in at most once".into())?;
        self.sources.iter().filter_map(|s| s.path.as_deref()).try_for_each(check_source_path)
    }

    /// The source that is the design the drawing is in, if it draws it.
    pub fn own_source(&self) -> Option<u64> {
        self.sources.iter().find(|s| s.path.is_none()).map(|s| s.id)
    }

    /// A sheet's own rules, and that the source it documents is one of the drawing's.
    fn check_sheet(&self, sheet: &Sheet) -> Result<()> {
        check_name(&sheet.name, "its")?;
        sheet.sheet.size_mm().map_err(from_sheet)?;
        sheet.title_block_outline()?;
        if let Some(block) = &sheet.title_block {
            fields::check_block(block)?;
        }
        // Each override names a text cell of its block, and its text is a cell's.
        for name in sheet.title_block_overrides.keys() {
            let named = || format!("its title block has no text cell `{name}` to override");
            require(fields::overridable(sheet.title_block.as_ref(), name), named)?;
        }
        let texts = sheet.title_block_overrides.values().map(String::as_str);
        solidcraft_geom::cell::check_text(texts).map_err(|e| DocError::Invalid(e.to_string()))?;
        require((MIN_DPI..=MAX_DPI).contains(&sheet.dpi), || format!("its pictures are {MIN_DPI} to {MAX_DPI} dpi"))?;
        match sheet.documents {
            Some(SheetComponent { source, .. }) => {
                let named = || format!("it documents source {}, which the drawing doesn't have", ids::encode(source));
                require(self.sources.iter().any(|s| s.id == source), named)
            }
            None => Ok(()),
        }
    }

    /// How a message names this drawing: by its name and id.
    pub fn label(&self) -> String {
        format!("drawing {:?} ({})", self.name, ids::encode(self.id))
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

/// Plain JSON designs share the drawing writer's preservation rules for newer records.
pub(crate) fn serialize_inline<S: serde::Serializer>(drawings: &[Arc<Drawing>], serializer: S) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::{Error as _, SerializeSeq as _};

    let mut seq = serializer.serialize_seq(Some(drawings.len()))?;
    for drawing in drawings {
        if drawing.newer_file.is_some() {
            drawing.validate().map_err(S::Error::custom)?;
            seq.serialize_element(&drawing.saved_record().map_err(S::Error::custom)?)?;
        } else {
            seq.serialize_element(drawing)?;
        }
    }
    seq.end()
}

/// Serialize a newer design's restored metadata with its editable drawing records.
pub(crate) struct InlineDocument<'a> {
    pub fields: &'a serde_json::Map<String, Value>,
    pub drawings: &'a [Arc<Drawing>],
}

impl Serialize for InlineDocument<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;

        struct Drawings<'a>(&'a [Arc<Drawing>]);
        impl Serialize for Drawings<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
                serialize_inline(self.0, serializer)
            }
        }

        let mut map = serializer.serialize_map(Some(self.fields.len()))?;
        for (key, value) in self.fields {
            if key == "drawings" {
                map.serialize_entry(key, &Drawings(self.drawings))?;
            } else {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
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
    drawings.iter().try_for_each(|d| take_id(&mut seen, d.id))
}

/// A drawing takes `id`, which no drawing before it in its design (`seen`) has.
fn take_id(seen: &mut BTreeSet<u64>, id: u64) -> Result<()> {
    require(seen.insert(id), || format!("two drawings have the id {}", ids::encode(id)))
}

/// A design of `count` drawings keeps the cap: what a reader checks before it reads any of them.
pub fn check_count(count: usize) -> Result<()> {
    require(count <= MAX_DRAWINGS, || format!("a design holds at most {MAX_DRAWINGS} drawings"))
}

/// Whether the drawing has no properties, so its file leaves them out.
fn no_properties(properties: &Arc<Metadata>) -> bool {
    properties.is_empty()
}

/// Whether the drawing's views follow the model as it changes, so its file leaves `updates` out.
fn follows_at_once(updates: &Updates) -> bool {
    *updates == Updates::Automatic
}

/// Adds `more` to `records`, each where its id puts it: the order every list of records after the
/// sheets keeps (the module's rule), whatever order they come in.
fn add_by_id<T>(records: &mut Vec<Arc<T>>, more: impl IntoIterator<Item = Arc<T>>, id: impl Fn(&T) -> u64) {
    records.extend(more);
    records.sort_by_key(|r| id(r));
}

/// Whether `records` are in the order [`add_by_id`] keeps.
fn in_id_order<T>(records: &[Arc<T>], id: impl Fn(&T) -> u64) -> bool {
    records.is_sorted_by_key(|r| id(r))
}

fn no_revisions(revisions: &Arc<RevisionRecord>) -> bool {
    **revisions == RevisionRecord::default()
}

/// `doc` without the drawings that break a rule, and a line naming each one left out and the
/// rule. Reading refuses a design whole for one such drawing, so a copy that must open again (an
/// autosave, a crash report's design) keeps the rest: the first [`MAX_DRAWINGS`], the first of
/// two with one id, and only those keeping their own rules and small enough to save.
pub fn openable(doc: &crate::Document) -> (Cow<'_, crate::Document>, Vec<String>) {
    let (mut seen, mut left_out) = (BTreeSet::new(), Vec::new());
    let kept: Vec<_> = doc
        .drawings
        .iter()
        .filter(|d| {
            // Its own rules and its size before its id: a drawing left out takes no id from a later one.
            let checked =
                check_count(seen.len() + 1).and_then(|()| d.validate()).and_then(|()| d.to_json().map(drop)).and_then(|()| take_id(&mut seen, d.id));
            checked.map_err(|e| left_out.push(format!("{} left out: {e}", d.label()))).is_ok()
        })
        .cloned()
        .collect();
    if left_out.is_empty() {
        return (Cow::Borrowed(doc), left_out);
    }
    let mut doc = doc.clone();
    doc.drawings = kept;
    (Cow::Owned(doc), left_out)
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

impl crate::Document {
    /// Whether a drawing of this design has anything to draw: a design with no features is empty.
    /// `drawing.create` refuses an empty design, and the app greys its New Drawing entry by the same rule.
    pub fn has_something_to_draw(&self) -> bool {
        !self.features.is_empty()
    }
}

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
