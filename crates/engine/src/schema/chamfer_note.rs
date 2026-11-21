//! Contracts and result types for the command in `cmd/chamfer_note.rs` (#13028).
//!
//! The reply mirrors [`solidcraft_doc::drawing::chamfer_note::Resolution`] and its note, which the
//! handler converts field by field, so the schema can't drift from what it returns. Ids and
//! decimals are read as `drawing.hole_callout` reads them.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::drawing::chamfer_note as doc;

use super::hole_callout::DecimalsParam;

/// A Chamfer feature, by its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChamferFeatureParam(pub u64);

impl<'de> Deserialize<'de> for ChamferFeatureParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_u64()
            .filter(|id| *id >= 1)
            .map(ChamferFeatureParam)
            .ok_or_else(|| D::Error::custom("must be a Chamfer feature's id"))
    }
}

impl JsonSchema for ChamferFeatureParam {
    fn schema_name() -> Cow<'static, str> {
        "ChamferFeatureId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The Chamfer feature's id.", "type": "integer", "minimum": 1})
    }
}

/// What a drawing's note of one chamfer says, worked out from the model. Read-only: nothing is
/// saved and nothing is placed on a sheet. Works with a drawing open or with none.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct NoteParams {
    /// The Chamfer feature's id.
    pub feature: ChamferFeatureParam,
    /// The persistent name of an edge it cuts, as `model.edges` names it.
    pub edge: String,
    /// `n × (…)` in front when the feature cuts more than one edge. Default: false.
    #[serde(default)]
    pub count: bool,
    /// The length unit of the numbers, any of the expression unit table's (`mm`, `in`, `yd`,
    /// …). Default: the open drawing's fixed unit, else the design's.
    #[serde(default)]
    pub units: Option<String>,
    /// 0 to 6 (inches show at least 3). Default: 2, as a new dimension's.
    #[serde(default)]
    pub decimals: Option<DecimalsParam>,
    /// The content language, a BCP 47 tag. Default: the open drawing's, else `en`.
    #[serde(default)]
    pub language: Option<String>,
}

/// The chamfer a note names, by its ids as given.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ChamferResult {
    /// The Chamfer feature's id.
    pub feature: u64,
    /// The edge's persistent name.
    pub edge: String,
}

/// A chamfer's size, in mm and radians.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SizeResult {
    /// The same setback on both faces, at 45°.
    Equal { distance: f64 },
    /// The setback along the first face, and the angle the chamfer makes with it.
    Angle { distance: f64, angle: f64 },
    /// The setback along the face facing up most, then along the other.
    Distances { up: f64, other: f64 },
}

/// What the feature cuts there.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteValues {
    /// How many edges the feature cuts (its picked edges: a tangent chain is one).
    pub count: usize,
    pub size: SizeResult,
}

/// What `drawing.chamfer_note` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NoteResult {
    /// The chamfer the call named.
    pub chamfer: ChamferResult,
    /// `resolved`, `suppressed` (the feature is), `failed` (it did not evaluate, or a later
    /// feature removed the chamfer) or `lost` (the feature is gone; never moved to another
    /// chamfer), as the sibling drawing replies' statuses are.
    pub status: String,
    /// Why it failed or is lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What it says, when resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<NoteValues>,
    /// The note's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

impl From<&doc::ChamferNote> for NoteValues {
    fn from(n: &doc::ChamferNote) -> Self {
        // Every field by name, so one added to the note can't be left out of the reply.
        let doc::ChamferNote { count, size } = n;
        let size = match *size {
            doc::Size::Equal { distance } => SizeResult::Equal { distance },
            doc::Size::Angle { distance, angle } => SizeResult::Angle { distance, angle },
            doc::Size::Distances { up, other } => SizeResult::Distances { up, other },
        };
        NoteValues { count: *count, size }
    }
}

#[cfg(test)]
#[path = "chamfer_note_tests.rs"]
mod tests;
