//! Contracts and result types for the commands in `cmd/drawing_balloon.rs` (#3874, W26).
//!
//! The edge a balloon points at is picked as a dimension's edge is
//! ([`super::drawing_dimension::EdgeParam`]) and saved by name; its path is the part it numbers.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::drawing_dimension::{CandidateResult, EdgeParam};
use super::drawing_view::ViewIdParam;

/// A balloon, by its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BalloonIdParam(pub u64);

impl<'de> Deserialize<'de> for BalloonIdParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => solidcraft_doc::ids::decode(&s).map(BalloonIdParam).ok_or_else(|| D::Error::custom("must be a balloon's id")),
            _ => Err(D::Error::custom("must be a balloon's id")),
        }
    }
}

impl JsonSchema for BalloonIdParam {
    fn schema_name() -> Cow<'static, str> {
        "DrawingBalloonId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A balloon's id: 13 characters of lowercase Crockford base32.", "type": "string", "pattern": "^[0-9a-hjkmnp-tv-z]{13}$"})
    }
}

/// What `drawing.balloon.add` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct AddParams {
    pub view: ViewIdParam,
    /// The edge its arrow points at, with `path`: the occurrences its body is placed through, from
    /// the top, which is the part the balloon numbers.
    pub edge: EdgeParam,
    /// The balloon's centre on paper, in millimetres from the view's centre.
    pub at: [f64; 2],
}

/// What `drawing.balloon.inspect` takes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct InspectParams {
    pub balloon: BalloonIdParam,
}

/// What `drawing.balloon.edit` takes: the balloon, and what to change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct EditParams {
    pub balloon: BalloonIdParam,
    pub set: BalloonSet,
}

/// What `drawing.balloon.edit` changes, each only when given. What a balloon points at isn't one
/// of them: to number another part, add a new one.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct BalloonSet {
    #[serde(deserialize_with = "super::present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "[f64; 2]")]
    pub at: Option<[f64; 2]>,
}

/// What `drawing.balloon.edit` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EditResult {
    pub balloon: String,
    /// The fields the edit changed.
    pub changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A balloon as the model has it now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BalloonResult {
    pub balloon: String,
    pub view: String,
    /// The record, as the drawing saves it.
    pub record: Value,
    /// The part it numbers: the ids of the occurrences its edge's body is placed through.
    pub occurrence: Vec<u64>,
    /// `ok`, `ambiguous`, `suppressed` or `lost`.
    pub status: String,
    /// Why it is lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Where its arrow ends, the middle of its edge, in the design's frame.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    /// Its edge was found by exact history.
    pub exact: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateResult>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "drawing_balloon_tests.rs"]
mod tests;
