//! Contracts and result types for the commands in `cmd/sketch_import.rs`: `sketch.insert_dxf`,
//! `sketch.insert_svg` and `sketch.export_dxf`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a file can be read or written, a sketch
//! exists or a drawing has anything to import stays the handler's to check. A call with several
//! problems may get a different one first in strict than with the check off (the check goes key
//! by key, the handlers in their own order).
//!
//! Two insert keys are read only in some cases, and the contract reads them in the same cases:
//! `path` only when `text` is not given, and of `width`, `height` and `scale` only the first one
//! given, in that order (a `height` next to a `width`, or a `scale` next to either, is never read).
//! A value of the wrong type that the handler reads as if the key were left out (`text: 5` next
//! to a good `path`, a text for `scale`) is refused by the contract, as schemas.md has it: that
//! difference is what warn and strict report.
//! `plane` and `name` are read only when no sketch is being edited, which the check can't see
//! (it has no session), so they take any value.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_io::MAX_DRAWING_BYTES;

use super::check::{located, whole};
use super::sketch_ends::SketchTarget;
use super::types::Point2;

/// A key that, once given (`null` included), must parse as `T`, as the handler reads it.
fn some<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// The keys the two insert commands read. See [`InsertDxfParams`] and [`InsertSvgParams`], which
/// leave `path`, `height` and `scale` unread in the cases the handler does.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = source_required)]
pub struct InsertDrawingKeys {
    /// The drawing itself, as text. Wins over `path`. At most 16 MiB (`MAX_DRAWING_BYTES`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<DrawingText>,
    /// The drawing file to read (not blank, under 4096 characters). Used when `text` is not
    /// given; ignored when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<DrawingPath>,
    /// An offset, in mm, [x, y]. Default: [0, 0]. With `align` "origin" it is where the file's
    /// origin goes; with "center", where the drawing's middle goes.
    #[serde(default)]
    pub at: Option<Point2>,
    /// A size factor, 1e-9 to 1e6 (a value outside is read as 1). Default: 1. Used only when
    /// neither `width` nor `height` is given.
    #[serde(default)]
    pub scale: Option<f64>,
    /// The drawing's width in mm after sizing, before rotation; the aspect ratio is kept. Wins
    /// over `height` and `scale`. The drawing must have a width.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PositiveMm")]
    pub width: Option<PositiveMm>,
    /// The drawing's height in mm after sizing, before rotation; the aspect ratio is kept. Wins
    /// over `scale`; ignored when `width` is given. The drawing must have a height.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PositiveMm")]
    pub height: Option<PositiveMm>,
    /// A turn in degrees, about the drawing's origin (with `align` "center": about its middle).
    /// Default: 0.
    #[serde(default, deserialize_with = "some", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AngleDegrees")]
    pub angle: Option<AngleDegrees>,
    /// What `at` places: the file's origin ("origin") or the drawing's middle ("center", which it
    /// then also turns and scales about). Default: origin.
    #[serde(default)]
    pub align: Option<DrawingAlign>,
    /// The distance in mm within which ends join into one point, 1e-9 to 10 (a value outside is
    /// brought inside; one that is not above 0 reads as the default). Default: 0.0001.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// Read only when no sketch is being edited: the plane of the new sketch, as `sketch.create`
    /// takes it. Default: XY. Ignored (and not checked) while a sketch is being edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<Value>,
    /// Read only when no sketch is being edited: the new sketch's name. Ignored (and not checked)
    /// while a sketch is being edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<Value>,
    /// The sketch to insert into: its feature id or its name. Default: the one being edited (a
    /// new one when none is).
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
}

/// Leaves out the keys the handler never reads for this call: `path` next to a `text` that is
/// text, `height` next to a `width`, and `scale` next to either.
fn leave_unread(value: &mut Value) {
    let Some(keys) = value.as_object_mut() else { return };
    if keys.get("text").is_some_and(Value::is_string) {
        keys.remove("path");
    }
    if keys.contains_key("width") {
        keys.remove("height");
    }
    if keys.contains_key("width") || keys.contains_key("height") {
        keys.remove("scale");
    }
}

fn read_keys<'de, D: Deserializer<'de>>(d: D) -> Result<InsertDrawingKeys, D::Error> {
    let mut value = Value::deserialize(d)?;
    leave_unread(&mut value);
    serde_path_to_error::deserialize(&value).map_err(|e| D::Error::custom(located(&e)))
}

/// Insert an ASCII DXF drawing into a sketch as curves: the active sketch, or a new one when none
/// is being edited. Give the drawing as `path` or as `text`. Blocks are expanded. It can be
/// sized, turned and placed. Text in the drawing becomes sketch text. One undo step.
#[derive(Debug)]
pub struct InsertDxfParams(pub InsertDrawingKeys);

impl<'de> Deserialize<'de> for InsertDxfParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_keys(d).map(InsertDxfParams)
    }
}

impl JsonSchema for InsertDxfParams {
    fn schema_name() -> Cow<'static, str> {
        "InsertDxfParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = InsertDrawingKeys::json_schema(generator);
        schema.insert("description".into(), INSERT_DXF.into());
        schema
    }
}

const INSERT_DXF: &str = "Insert an ASCII DXF drawing into a sketch as curves: the active sketch, or a new one when none is being edited. Give the drawing as `path` or as `text`. Blocks are expanded. It can be sized, turned and placed. Text in the drawing becomes sketch text. One undo step.";

/// Insert an SVG drawing into a sketch as curves: the active sketch, or a new one when none is
/// being edited. Give the drawing as `path` or as `text`. It can be sized, turned and placed.
/// Text in the drawing becomes sketch text. One undo step.
#[derive(Debug)]
pub struct InsertSvgParams(pub InsertDrawingKeys);

impl<'de> Deserialize<'de> for InsertSvgParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        read_keys(d).map(InsertSvgParams)
    }
}

impl JsonSchema for InsertSvgParams {
    fn schema_name() -> Cow<'static, str> {
        "InsertSvgParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = InsertDrawingKeys::json_schema(generator);
        schema.insert("description".into(), INSERT_SVG.into());
        schema
    }
}

const INSERT_SVG: &str = "Insert an SVG drawing into a sketch as curves: the active sketch, or a new one when none is being edited. Give the drawing as `path` or as `text`. It can be sized, turned and placed. Text in the drawing becomes sketch text. One undo step.";

/// The insert commands need `path` or `text`; the handler says so when both are left out.
fn source_required(schema: &mut Schema) {
    schema.insert("anyOf".into(), serde_json::json!([{"required": ["text"]}, {"required": ["path"]}]));
}

fn path_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string"})
}

/// A drawing's text, as `source_text` reads it: at most 16 MiB, or the handler's refusal.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawingText(pub String);

impl<'de> Deserialize<'de> for DrawingText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        if text.len() > MAX_DRAWING_BYTES {
            return Err(D::Error::custom(whole("the drawing is too large")));
        }
        Ok(DrawingText(text))
    }
}

impl JsonSchema for DrawingText {
    fn schema_name() -> Cow<'static, str> {
        "DrawingText".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string"})
    }
}

/// A drawing file name, as `source_text` reads `path`: not blank and under 4096 bytes, or the
/// handler's "give `path` or `text`" (anything but text reads as no `path`).
#[derive(Clone, Debug, PartialEq)]
pub struct DrawingPath(pub String);

impl<'de> Deserialize<'de> for DrawingPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.trim().is_empty() && s.len() < 4096 => Ok(DrawingPath(s)),
            _ => Err(D::Error::custom(whole("give `path` or `text`"))),
        }
    }
}

/// A size in mm above 0, as the insert commands' `width` and `height` are read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositiveMm(pub f64);

/// The message the insert commands give for `width` or `height`: the check puts the key first.
const POSITIVE_MM: &str = "must be a positive number (mm)";

impl<'de> Deserialize<'de> for PositiveMm {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_f64().filter(|w| w.is_finite() && *w > 0.0).map(PositiveMm).ok_or_else(|| D::Error::custom(POSITIVE_MM))
    }
}

impl JsonSchema for PositiveMm {
    fn schema_name() -> Cow<'static, str> {
        "PositiveMm".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A size in mm, above 0.", "type": "number", "exclusiveMinimum": 0})
    }
}

/// An angle in degrees as `angle` is read: a number, whatever its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleDegrees(pub f64);

impl<'de> Deserialize<'de> for AngleDegrees {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_f64().filter(|a| a.is_finite()).map(AngleDegrees).ok_or_else(|| D::Error::custom("must be a number (degrees)"))
    }
}

impl JsonSchema for AngleDegrees {
    fn schema_name() -> Cow<'static, str> {
        "AngleDegrees".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An angle in degrees.", "type": "number"})
    }
}

/// What an insert's `at` places, as `align` is read (the words are case-sensitive).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawingAlign {
    /// `at` is where the file's origin goes.
    Origin,
    /// `at` is where the drawing's middle goes, and it turns and scales about it.
    Center,
}

impl JsonSchema for DrawingAlign {
    fn schema_name() -> Cow<'static, str> {
        "DrawingAlign".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "What `at` places: \"origin\" (the file's origin goes there) or \"center\" (the drawing's middle goes there, and it turns and scales about it). Lower case only.",
            "type": "string",
            "enum": ["origin", "center"]
        })
    }
}

impl<'de> Deserialize<'de> for DrawingAlign {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_str() {
            Some("origin") => Ok(DrawingAlign::Origin),
            Some("center") => Ok(DrawingAlign::Center),
            _ => Err(D::Error::custom("must be \"origin\" or \"center\"")),
        }
    }
}

/// What `sketch.insert_dxf` and `sketch.insert_svg` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DrawingInserted {
    /// The sketch the drawing went into, by feature id.
    pub sketch: u64,
    /// The new sketch's id when no sketch was being edited and one was made; else null.
    pub created_sketch: Option<u64>,
    /// How many curves were added.
    pub curves: usize,
    /// How many sketch texts were added.
    pub texts: usize,
    /// The sketch's solve afterwards; null when it was not solved.
    pub info: Option<SketchSolve>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A sketch's solve, as the sketch commands report it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SketchSolve {
    /// Whether the solve kept the sketch's orientation.
    pub solved: bool,
    /// The degrees of freedom left (0 = fully constrained).
    pub dof: usize,
}

/// Save a sketch's curves as an ASCII DXF: to the file `path`, or, without it, as text in the
/// reply. Construction curves are left out; splines and other curves are written as line
/// segments.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ExportDxfParams {
    /// The sketch: its feature id or its name. Default: the one being edited.
    #[serde(default)]
    pub sketch: Option<SketchTarget>,
    /// The file to write. Default (also when blank): none, and the DXF text is returned.
    #[serde(default)]
    pub path: Option<String>,
}

/// What `sketch.export_dxf` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExportDxfResult {
    #[serde(flatten)]
    pub exported: DxfExported,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Where the DXF went.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DxfExported {
    /// With a `path`: the file was written.
    Written {
        /// The file written, as given.
        path: String,
        /// How many entities it holds.
        entities: usize,
    },
    /// Without one: the DXF as text.
    Text {
        /// The DXF file's text.
        dxf: String,
        /// How many entities it holds.
        entities: usize,
    },
}

#[cfg(test)]
#[path = "sketch_import_tests.rs"]
mod tests;
