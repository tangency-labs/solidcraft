//! Contracts and result types for the commands in `cmd/drawing_note.rs` (#4896, W19e).
//!
//! What a note points at is picked as a dimension's edge or corner is, and saved by name.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_geom::cell::{Content, Part, Source};

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
            parts => {
                let content: Content = serde_json::from_value(parts).map_err(D::Error::custom)?;
                // Refused when given; a drawing saved with one still opens, and the field reads unknown.
                match solidcraft_doc::drawing::fields::unknown_field(&content) {
                    Some(why) => Err(D::Error::custom(why)),
                    None => Ok(ContentParam(content)),
                }
            }
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

/// Another leader of a note on a view, picked as its anchor's edge or corner is.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BranchParam {
    /// Its leader ends on the edge's middle.
    Edge(EdgeParam),
    /// Its leader ends on the corner or the centre.
    Corner(CornerParam),
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
    /// More leaders, for a note on a view: 16 leaders in all, its anchor's included.
    #[serde(default)]
    pub branches: Vec<BranchParam>,
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
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BranchesSet")]
    pub branches: Option<BranchesSet>,
}

/// The branches an edit takes out and adds, as a dimension set's targets edit does.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct BranchesSet {
    /// Branches taken out, by their numbers from 0 as the list is now; each once.
    pub remove: Vec<usize>,
    /// Branches added after those that stay.
    pub add: Vec<BranchParam>,
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
    /// Each branch, in order, as the model has it now: one lost loses only its own leader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<BranchResult>,
    /// What it says now, each field read for its sheet: a field with no value says nothing.
    pub text: String,
    /// Each field it reads, in order, as its sheet reads it now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldResult>,
    /// What to know about it: a view edited since to leave out what its leader points at, or its
    /// text running off the paper.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One branch of a note, as the model has it now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BranchResult {
    /// `ok`, `ambiguous`, `suppressed` or `lost`.
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
}

/// One field a note reads.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FieldResult {
    /// `component`, `drawing`, `sheet` or `revision`.
    #[schemars(with = "String")]
    pub source: Source,
    pub key: String,
    /// `shown`; `empty`: its source has no value for it; `lost`: its sheet documents a component
    /// the design no longer has; or `unknown`: its source has no such field.
    pub state: String,
    /// What it shows, when shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Why it is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}
