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
//! - **Balloons** on its views, each pointing at a placed part by a named edge of it ([`balloon`]).
//! - **Images** on its sheets, each file kept once however many places it sits ([`image`]).
//! - **Tables** on its sheets: parts lists and revision tables ([`table`]).
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
//!   order, maps sorted, and lists of records in a fixed order (sheets as arranged; later
//!   records by id), never the order they were added in. Records listed by id read in that order
//!   whatever order a file lists them in: the order means nothing, so it is never damage.
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

pub mod balloon;
pub mod by_id;
pub mod coverage;
pub mod dimension;
pub mod fields;
pub mod image;
pub mod note;
pub mod sheet_format;
pub mod table;
pub mod template;
pub mod view;

use balloon::Balloon;
use by_id::ById;
use dimension::Dimension;
use image::{ImageFile, SheetImage};
use note::{Anchor, Note};
use table::SheetTable;
use view::View;

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
    /// The designs the drawing draws: for now at most one, the design it is in.
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
    /// Behind an `Arc` as the sheets are, so an undo step shares it while it doesn't change.
    #[serde(default, skip_serializing_if = "no_revisions")]
    pub revisions: Arc<RevisionRecord>,
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
    "sheets",
    "views",
    "dimensions",
    "balloons",
    "images",
    "sheet_images",
    "tables",
    "notes",
    "revisions",
];

/// Move the elements of the list `key` that don't read as `T` out of a newer build's drawing.
fn take_unread<T: serde::de::DeserializeOwned>(top: &mut serde_json::Map<String, Value>, key: &str, one: &str, file: &mut NewerDrawing) {
    let Some(Value::Array(list)) = top.get_mut(key) else { return };
    let mut readable = Vec::with_capacity(list.len());
    for element in std::mem::take(list) {
        match DRAWING.check_numbers(&element, "properties").and_then(|()| serde_json::from_value::<T>(element.clone()).map_err(damaged)) {
            Ok(_) => readable.push(element),
            Err(why) => {
                let name = element.get("name").or(element.get("path")).and_then(Value::as_str).unwrap_or("unnamed");
                file.not_understood.push(format!("{one} `{name}` ({why})"));
                file.elements.push((key.to_string(), element));
            }
        }
    }
    *list = readable;
}

/// What a drawing file says and holds that this build doesn't read ([`Drawing::from_newer`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewerDrawing {
    /// The number the file says.
    pub format: u32,
    /// The number this build reads up to.
    pub reads_up_to: u32,
    /// The changes the file names that this build doesn't have.
    pub unknown_changes: Vec<String>,
    /// What this build couldn't read, by name.
    pub not_understood: Vec<String>,
    fields: Vec<(String, Value)>,
    elements: Vec<(String, Value)>,
}

impl NewerDrawing {
    /// What to tell the user.
    pub fn notice(&self) -> String {
        format!(
            "Saved by a newer SolidCraft (format {}, this version reads up to {}). If something's missing, update.",
            self.format, self.reads_up_to
        )
    }

    fn restore(&self, value: &mut Value) {
        for (key, field) in &self.fields {
            value[key] = field.clone();
        }
        for (key, element) in &self.elements {
            if let Some(list) = value.get_mut(key).and_then(Value::as_array_mut) {
                list.push(element.clone());
            }
        }
    }
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
    /// Line work and fixed text drawn under its views, as an imported template brings them.
    #[serde(default, skip_serializing_if = "sheet_format::SheetFormat::is_empty")]
    pub format: sheet_format::SheetFormat,
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
            format: sheet_format::SheetFormat::default(),
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
            sheets: vec![Arc::new(first)],
            views: Vec::new(),
            dimensions: Vec::new(),
            balloons: ById::default(),
            images: ById::default(),
            sheet_images: ById::default(),
            tables: ById::default(),
            notes: ById::default(),
            revisions: Arc::default(),
            newer_file: None,
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
        if let Some(newer) = DRAWING.newer(version, applied.as_deref()) {
            return Self::from_newer(v, version, applied, newer);
        }
        let d = Self::decode(v, version, applied.as_deref())?;
        Ok(Drawing { format: DRAWING.format_string(DRAWING.current()), format_pending: DRAWING.pending_names(), ..d })
    }

    fn decode(mut v: Value, version: u32, applied: Option<&[String]>) -> Result<Drawing> {
        DRAWING.check_numbers(&v, "properties")?;
        DRAWING.upgrade(&mut v, version, applied)?;
        let mut d: Drawing = serde_json::from_value(v).map_err(damaged)?;
        let (views, dimensions) = (std::mem::take(&mut d.views), std::mem::take(&mut d.dimensions));
        d.add_views(views);
        d.add_dimensions(dimensions);
        d.balloons.put_in_order();
        d.images.put_in_order();
        d.sheet_images.put_in_order();
        d.tables.put_in_order();
        d.notes.put_in_order();
        d.validate().map_err(damaged)?;
        d.check_image_files().map_err(damaged)?;
        Ok(d)
    }

    /// A drawing a newer build saved: it opens with what this build can read of it. Everything in a
    /// drawing refuses a field it doesn't know, so what it can't read shows as a top-level field,
    /// a source or a sheet that doesn't read, which stays out and is named in
    /// [`NewerDrawing::not_understood`]; [`Drawing::to_json`] writes it all back, with the file's
    /// format. It takes at least one sheet this build can read to open.
    fn from_newer(mut v: Value, version: u32, applied: Option<Vec<String>>, newer: crate::format::Newer) -> Result<Drawing> {
        let list = applied.as_deref();
        let can_read = |v: Value| Self::decode(v, version, list).map_err(|e| e.to_string());
        let mut file = NewerDrawing {
            format: newer.format,
            reads_up_to: newer.reads_up_to,
            unknown_changes: newer.unknown_changes.clone(),
            ..NewerDrawing::default()
        };
        let mut read = can_read(v.clone());
        if read.is_err() {
            let top = v.as_object_mut().ok_or_else(|| damaged("expected a JSON object"))?;
            let keys: Vec<String> = top.keys().cloned().collect();
            for key in keys {
                if !RECORD_FIELDS.contains(&key.as_str())
                    && let Some(value) = top.remove(&key)
                {
                    file.not_understood.push(format!("field `{key}`"));
                    file.fields.push((key, value));
                }
            }
            take_unread::<Sheet>(top, "sheets", "sheet", &mut file);
            take_unread::<Source>(top, "sources", "source", &mut file);
            read = can_read(v);
        }
        let d = read.map_err(|why| {
            DocError::Invalid(format!(
                "this drawing was saved by a newer SolidCraft (format {}, this version reads up to {}) and this version can't read it: {why}. Update SolidCraft to open it",
                newer.format, newer.reads_up_to
            ))
        })?;
        let (format, format_pending) = if newer.format > newer.reads_up_to {
            (DRAWING.format_string(newer.format), applied.unwrap_or_default())
        } else {
            let mut pending = DRAWING.pending_names();
            pending.extend(newer.unknown_changes.iter().cloned());
            (DRAWING.format_string(DRAWING.current()), pending)
        };
        Ok(Drawing { format, format_pending, newer_file: Some(Arc::new(file)), ..d })
    }

    /// The drawing's JSON: the same bytes for the same drawing. JSON longer than a read takes
    /// ([`MAX_DRAWING_BYTES`]) is refused, so every drawing saved opens again: other tools'
    /// properties are kept as they came, and indenting them can make a drawing that read in
    /// under the limit save over it.
    pub fn to_json(&self) -> Result<String> {
        let unsaved = |e: serde_json::Error| DocError::Invalid(format!("drawing not saved: {e}"));
        let text = match &self.newer_file {
            None => serde_json::to_string_pretty(self).map_err(unsaved)?,
            Some(file) => {
                let mut value = serde_json::to_value(self).map_err(unsaved)?;
                file.restore(&mut value);
                serde_json::to_string_pretty(&value).map_err(unsaved)?
            }
        };
        require(text.len() <= MAX_DRAWING_BYTES, || format!("drawing too large to save (at most {} MiB)", MAX_DRAWING_BYTES >> 20))?;
        Ok(text)
    }

    /// An id for a new object: one no object of the drawing has, from `id_source`, which never
    /// gives one twice.
    pub fn allocate_id(&self, id_source: &mut IdSource) -> Result<u64> {
        id_source.next(&self.ids().collect())
    }

    /// The position of the sheet `named` names: by id, else by a name only one sheet has.
    pub fn sheet_index(&self, named: &str) -> Result<usize> {
        if let Some(i) = ids::decode(named).and_then(|id| self.sheets.iter().position(|s| s.id == id)) {
            return Ok(i);
        }
        let mut called = self.sheets.iter().enumerate().filter(|(_, s)| s.name == named).map(|(i, _)| i);
        match (called.next(), called.next()) {
            (Some(i), None) => Ok(i),
            (Some(_), Some(_)) => Err(DocError::Invalid(format!("more than one sheet is called `{named}`: give its id"))),
            (None, _) => Err(DocError::Invalid(format!("no sheet `{named}`"))),
        }
    }

    /// Whether `id` names an object a sheet shows: a view, a dimension or a balloon.
    pub fn has_object(&self, id: u64) -> bool {
        self.view(id).is_some() || self.dimension(id).is_some() || self.balloons.by_id(id).is_some() || self.notes.by_id(id).is_some()
    }

    /// What is on sheet `id`: its views and the dimensions and balloons in them, and the images placed on it.
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
        on
    }

    /// `views`, listed by id, with the dimensions and balloons in them. An image sits on a sheet, never in a
    /// view, so there are none.
    pub fn with_contents(&self, views: Vec<u64>) -> OnSheet {
        let dimensions = self.dimensions.iter().filter(|d| views.binary_search(&d.view).is_ok()).map(|d| d.id).collect();
        let balloons = self.balloons.iter().filter(|b| views.binary_search(&b.view).is_ok()).map(|b| b.id).collect();
        let notes = self.notes.iter().filter(|n| n.anchor.view().is_some_and(|v| views.binary_search(&v).is_ok())).map(|n| n.id).collect();
        OnSheet { views, dimensions, balloons, images: Vec::new(), tags: Vec::new(), tables: Vec::new(), notes }
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
    }

    /// Every object's id, of every kind: what a new id must differ from. Each later kind adds its
    /// own here.
    fn ids(&self) -> impl Iterator<Item = u64> {
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
            .chain(self.revisions.ids())
    }

    /// The record's rules (the module's notes): what reading a drawing checks, and what every edit
    /// keeps.
    pub fn validate(&self) -> Result<()> {
        require((1..=MAX_SHEETS).contains(&self.sheets.len()), || format!("a drawing has 1 to {MAX_SHEETS} sheets"))?;
        require(self.sources.len() <= 1, || "a drawing draws only the design it is in".into())?;
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
        let mut seen = BTreeSet::new();
        for id in self.ids() {
            require(id != 0, || "an object has the id 0, which is never one".into())?;
            require(seen.insert(id), || format!("two objects have the id {}", ids::encode(id)))?;
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
        self.check_images()?;
        self.check_notes()
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
        sheet.format.check()?;
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

#[cfg(test)]
#[path = "drawing_tests.rs"]
mod tests;
