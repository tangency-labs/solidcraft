//! Contracts and result types for the commands in `cmd/drawing_symbol.rs` (#13063).
//!
//! A symbol's pieces are given as the drawing saves them, so a script writes what a file holds. A
//! placement's anchor is given as a note's ([`AnchorParam`]), and its angle in degrees.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::drawing::symbol::Piece;

use super::drawing_note::AnchorParam;

/// A symbol, by its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SymbolIdParam(pub u64);

impl<'de> Deserialize<'de> for SymbolIdParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(SymbolIdParam).ok_or_else(|| D::Error::custom("must be a symbol's id")),
            _ => Err(D::Error::custom("must be a symbol's id")),
        }
    }
}

impl JsonSchema for SymbolIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingSymbolId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A symbol's id: 13 characters of lowercase Crockford base32.", "type": "string"})
    }
}

/// A placed symbol, by its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacementIdParam(pub u64);

impl<'de> Deserialize<'de> for PlacementIdParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(PlacementIdParam).ok_or_else(|| D::Error::custom("must be a placed symbol's id")),
            _ => Err(D::Error::custom("must be a placed symbol's id")),
        }
    }
}

impl JsonSchema for PlacementIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingPlacedSymbolId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A placed symbol's id: 13 characters of lowercase Crockford base32.", "type": "string"})
    }
}

/// A symbol's pieces, in paper millimetres from its insertion point.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct PiecesParam(pub Vec<Piece>);

impl JsonSchema for PiecesParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingSymbolPieces".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A symbol's pieces, in mm from its insertion point: {stroke: {path: [segments], closed?, line: visible|hidden|centre|thin}}, \
                            {fill: {path}} (a closed outline) or {text: {at: [x, y], content: [{text} or {field: {source, key}}], cap_mm?, align?: left|centre|right}}. \
                            A segment is {type: line, a, b} or {type: arc, center, radius, start, sweep}, as a drawing saves it.",
            "type": "array",
            "items": {"type": "object"}
        })
    }
}

/// What `drawing.symbol.define` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DefineParams {
    /// Its own among the drawing's symbols.
    pub name: String,
    pub pieces: PiecesParam,
}

/// What `drawing.symbol.edit` takes: the symbol, and what to change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub symbol: SymbolIdParam,
    pub set: SymbolSet,
}

/// What `drawing.symbol.edit` changes, each only when given. The pieces are replaced whole.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct SymbolSet {
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PiecesParam")]
    pub pieces: Option<PiecesParam>,
}

/// What `drawing.symbol.delete` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct DeleteParams {
    pub symbol: SymbolIdParam,
    /// `delete` to delete its placements with it; without it, a placed symbol is refused.
    #[serde(default)]
    pub placements: Option<PlacementsParam>,
}

/// What becomes of a deleted symbol's placements.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlacementsParam {
    Delete,
}

/// What `drawing.symbol.place` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PlaceParams {
    pub symbol: SymbolIdParam,
    pub anchor: AnchorParam,
    /// Where its insertion point sits on paper, in millimetres from its sheet's lower-left corner,
    /// or from its view's centre.
    pub at: [f64; 2],
    /// Its pieces' millimetres times this: 0.01 to 100.
    #[serde(default = "one")]
    pub scale: f64,
    /// Its turn about its insertion point, counter-clockwise, in degrees.
    #[serde(default)]
    pub angle: f64,
    /// The text each of its prompts shows here, by key.
    #[serde(default)]
    pub answers: BTreeMap<String, String>,
}

fn one() -> f64 {
    1.0
}

/// What `drawing.symbol.placed.edit` takes: the placement, and what to change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PlacedEditParams {
    pub placement: PlacementIdParam,
    pub set: PlacedSet,
}

/// What `drawing.symbol.placed.edit` changes, each only when given. The answers are replaced whole.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PlacedSet {
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "[f64; 2]")]
    pub at: Option<[f64; 2]>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub scale: Option<f64>,
    /// In degrees.
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64")]
    pub angle: Option<f64>,
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BTreeMap<String, String>")]
    pub answers: Option<BTreeMap<String, String>>,
}

/// What `drawing.symbol.placed.delete` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct PlacedDeleteParams {
    pub placement: PlacementIdParam,
}

/// A symbol as the drawing has it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SymbolResult {
    pub symbol: String,
    /// The record, as the drawing saves it.
    pub record: Value,
    /// The keys its texts prompt for, sorted: what a placement answers.
    pub prompts: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.symbol.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub symbol: String,
    /// The fields the edit changed.
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.symbol.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DeleteResult {
    pub symbol: String,
    /// Its placements, deleted with it.
    pub placements: Vec<String>,
}

/// A placement as the drawing has it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacedResult {
    pub placement: String,
    /// The record, as the drawing saves it (its angle in radians).
    pub record: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.symbol.placed.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacedEditResult {
    pub placement: String,
    /// The fields the edit changed.
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `drawing.symbol.placed.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacedDeleteResult {
    pub placement: String,
}
