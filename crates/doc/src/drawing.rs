//! A drawing (#3890): sheets of paper that draw designs saved in their own files. A drawing is a
//! file of its own, `"format": "solidcraft-drawing/N"`, versioned by the same code as designs
//! ([`DRAWING`]); format 1 is this record.
//!
//! - **Sources** are the designs it draws: an id and the design file's path, relative to the
//!   drawing's folder once the drawing is saved, else as given, with `/` between folders (a linked
//!   design's [`DesignLink`](crate::DesignLink) rule). What a design was when the drawing last saw
//!   it is not saved here.
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
//! - 1 to [`MAX_SHEETS`] sheets and at most [`MAX_SOURCES`] sources; names of 1 to
//!   [`MAX_NAME_BYTES`] bytes; paths of 1 to [`MAX_PATH_BYTES`] bytes, without NUL.
//! - Nothing derived is saved, and a drawing always saves as the same bytes: fields in their
//!   order, maps sorted.
//! - A later change to the record is one file in `src/drawing/format/pending/` (its README).
//!
//! [`Drawing::from_json`] reads a file in this order: at most [`MAX_DRAWING_BYTES`] of text, the
//! family's format check, every number in range, the upgrade, the typed record, then
//! [`Drawing::validate`].

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_geom::drawing_sheet::{self, DrawingBorderProfile};

use crate::format::{CheckNumbers as _, DRAWING};
use crate::ids::{self, IdSource};
use crate::{DocError, Metadata, Result, expr};

/// The most sheets a drawing may have.
pub const MAX_SHEETS: usize = 1000;
/// The most designs a drawing may draw.
pub const MAX_SOURCES: usize = 64;
/// The longest name of a drawing or a sheet, in bytes.
pub const MAX_NAME_BYTES: usize = 256;
/// The longest path to a design, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
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
    pub name: String,
    /// The designs the drawing draws.
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
    /// Set when a newer SolidCraft saved the file this came from.
    #[serde(skip)]
    pub newer_file: Option<Arc<NewerDrawing>>,
}

/// The fields a drawing's record has: anything else in a file is a newer build's.
const RECORD_FIELDS: [&str; 8] = ["format", "format_pending", "name", "sources", "properties", "units", "language", "sheets"];

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

/// A design the drawing draws.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The design file: relative to the drawing's folder once the drawing is saved, else as given
    /// (`/` separates folders).
    pub path: String,
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
    /// `sheet`: the paper `paper` with a copy of `border`, its id from `id_source`.
    pub fn new(
        name: &str,
        language: &str,
        sheet: &str,
        paper: drawing_sheet::Sheet,
        border: Option<DrawingBorderProfile>,
        id_source: &mut IdSource,
    ) -> Result<Drawing> {
        let id = id_source.next(&BTreeSet::new())?;
        let first = Sheet { id, name: sheet.into(), sheet: paper, border, dpi: DEFAULT_DPI, documents: None, excluded: Excluded::default() };
        let drawing = Drawing {
            format: DRAWING.format_string(DRAWING.current()),
            format_pending: DRAWING.pending_names(),
            name: name.into(),
            sources: Vec::new(),
            properties: Metadata::default(),
            units: Units::default(),
            language: language.into(),
            sheets: vec![Arc::new(first)],
            newer_file: None,
        };
        drawing.validate()?;
        Ok(drawing)
    }

    /// Read a drawing file's JSON (any format this build knows; older ones are upgraded). A file
    /// that breaks a rule of the record is damaged and refused, never patched.
    pub fn from_json(s: &str) -> Result<Drawing> {
        if s.len() > MAX_DRAWING_BYTES {
            return Err(DocError::Invalid(format!("drawing too large (at most {} MiB)", MAX_DRAWING_BYTES >> 20)));
        }
        let mut v: Value = serde_json::from_str(s).map_err(|e| DocError::Invalid(format!("not a SolidCraft drawing: {e}")))?;
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
        let d: Drawing = serde_json::from_value(v).map_err(damaged)?;
        d.validate().map_err(damaged)?;
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

    /// Every object's id, of every kind: what a new id must differ from. Each later kind adds its
    /// own here.
    fn ids(&self) -> impl Iterator<Item = u64> {
        self.sources.iter().map(|s| s.id).chain(self.sheets.iter().map(|s| s.id))
    }

    /// The record's rules (the module's notes): what reading a file checks, and what every edit
    /// keeps.
    pub fn validate(&self) -> Result<()> {
        require((1..=MAX_SHEETS).contains(&self.sheets.len()), || format!("a drawing has 1 to {MAX_SHEETS} sheets"))?;
        require(self.sources.len() <= MAX_SOURCES, || format!("a drawing draws at most {MAX_SOURCES} designs"))?;
        check_name(&self.name, "the drawing's")?;
        require(is_language_tag(&self.language), || {
            format!("the language is not a BCP 47 tag of 2 to {MAX_LANGUAGE_CHARS} characters (such as `en` or `en-GB`)")
        })?;
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
        for source in &self.sources {
            let path = &source.path;
            require(!path.is_empty() && path.len() <= MAX_PATH_BYTES && !path.contains('\0'), || {
                format!("source {}: a design's path is 1 to {MAX_PATH_BYTES} bytes, without NUL", ids::encode(source.id))
            })?;
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
