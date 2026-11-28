//! Command contracts for manual centre lines.

use super::drawing_dimension::{CandidateResult, CornerParam};
use super::drawing_view::ViewIdParam;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineId(pub u64);

impl<'de> Deserialize<'de> for LineId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(LineId).ok_or_else(|| D::Error::custom("must be a centre line id")),
            _ => Err(D::Error::custom("must be a centre line id")),
        }
    }
}

impl JsonSchema for LineId {
    fn schema_name() -> Cow<'static, str> {
        "DrawingCentreLineId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "Centre line id, in lowercase Crockford base32", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    pub view: ViewIdParam,
    pub a: CornerParam,
    pub b: CornerParam,
    pub overshoot_mm: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct LineParams {
    pub line: LineId,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub line: LineId,
    pub overshoot_mm: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LineResult {
    pub line: String,
    pub view: String,
    pub record: Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<[[f64; 3]; 2]>,
    pub exact: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateResult>,
    /// Why its view no longer draws a referenced part, when changed after the annotation was added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ChangeResult {
    pub line: String,
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
