//! Command contracts for manually added centre marks.

use super::drawing_dimension::{CandidateResult, EdgeParam};
use super::drawing_view::ViewIdParam;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkId(pub u64);

impl<'de> Deserialize<'de> for MarkId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(MarkId).ok_or_else(|| D::Error::custom("must be a centre mark id")),
            _ => Err(D::Error::custom("must be a centre mark id")),
        }
    }
}

impl JsonSchema for MarkId {
    fn schema_name() -> Cow<'static, str> {
        "DrawingCentreMarkId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "Centre mark id, in lowercase Crockford base32", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    pub view: ViewIdParam,
    pub edge: EdgeParam,
    pub overshoot_mm: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct MarkParams {
    pub mark: MarkId,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub mark: MarkId,
    pub overshoot_mm: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MarkResult {
    pub mark: String,
    pub view: String,
    pub record: Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub centre: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_mm: Option<f64>,
    pub exact: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ChangeResult {
    pub mark: String,
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_centre_mark_tests.rs"]
mod tests;
