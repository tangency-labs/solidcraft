//! Contracts and result types for the commands in `cmd/drawing_symbol.rs` (#13063).
//!
//! A symbol's pieces are given as the drawing saves them, so a script writes what a file holds.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::drawing::symbol::Piece;

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
}
