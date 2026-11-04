//! Contracts and result types for the commands in `cmd/drawing_note.rs` (#4896, W19e).
//!
//! What a note points at is picked as a dimension's edge or corner is, and saved by name.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_geom::cell::{Content, Part};

use super::drawing_dimension::{CandidateResult, CornerParam, EdgeParam};
use super::drawing_sheet::sheet_ref;
use super::drawing_view::ViewIdParam;

/// A note, by its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteIdParam(pub u64);

impl<'de> Deserialize<'de> for NoteIdParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(NoteIdParam).ok_or_else(|| D::Error::custom("must be a note's id")),
            _ => Err(D::Error::custom("must be a note's id")),
        }
    }
}

impl JsonSchema for NoteIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingNoteId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A note's id: 13 characters of lowercase Crockford base32.", "type": "string"})
    }
}

/// What a note says: plain text, or a list of text and field parts.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentParam(pub Content);

impl<'de> Deserialize<'de> for ContentParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(text) => Ok(ContentParam(Content(vec![Part::Text(text)]))),
            parts => serde_json::from_value(parts).map(ContentParam).map_err(D::Error::custom),
        }
    }
}

impl JsonSchema for ContentParam {
    fn schema_name() -> Cow<'static, str> {
        "NoteContent".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "What a note says: plain text, or a list of {text} and {field: {source: component|drawing|sheet|revision, key, format?}}.",
            "anyOf": [{"type": "string"}, {"type": "array"}]
        })
    }
}

/// What a note belongs to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AnchorParam {
    /// A sheet, by id or name.
    Sheet(#[serde(deserialize_with = "sheet_ref")] String),
    View(ViewIdParam),
    /// An edge of the view's model: its leader ends on the edge's middle.
    Edge {
        view: ViewIdParam,
        edge: EdgeParam,
    },
    /// A corner or a centre of the view's model: its leader ends there.
    Corner {
        view: ViewIdParam,
        corner: CornerParam,
    },
}

/// What `drawing.note.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    pub anchor: AnchorParam,
    /// Where it sits on paper, in millimetres from its sheet's lower-left corner, or from its view's centre.
    pub at: [f64; 2],
    pub content: ContentParam,
    /// The height of its capitals on paper, in millimetres: 0.5 to 100.
    #[serde(default, deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub cap_mm: Option<f64>,
}

/// What `drawing.note.inspect` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct InspectParams {
    pub note: NoteIdParam,
}

/// What `drawing.note.edit` takes: the note, and what to change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub note: NoteIdParam,
    pub set: NoteSet,
}

/// What `drawing.note.edit` changes, each only when given. What a note belongs to isn't one of them:
/// to point at something else, add a new one.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct NoteSet {
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "[f64; 2]")]
    pub at: Option<[f64; 2]>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ContentParam")]
    pub content: Option<ContentParam>,
    /// Its text height, or `null` for the drawing's.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<f64>")]
    pub cap_mm: Option<Option<f64>>,
}

/// What `drawing.note.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub note: String,
    /// The fields the edit changed.
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A note as the model has it now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteResult {
    pub note: String,
    /// The record, as the drawing saves it.
    pub record: Value,
    /// `ok`, `ambiguous`, `suppressed` or `lost`. A note that points at nothing is always `ok`.
    pub status: String,
    /// Why it is lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Where its leader ends, in the design's frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    /// What it points at was found by exact history.
    pub exact: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateResult>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
