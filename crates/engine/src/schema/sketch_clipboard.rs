//! Contracts and result types for the commands in `cmd/sketch_clipboard.rs`: copying, cutting
//! and pasting sketch geometry (#10963).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec2;

use super::types::{Point2, SketchRef, StringList, present};
use crate::params;

/// Where a paste goes: a [`Point2`], with the paste's message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PastePoint(pub Vec2);

impl<'de> Deserialize<'de> for PastePoint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec2(&Value::deserialize(d)?).map(PastePoint).ok_or_else(|| D::Error::custom("must be [x, y]"))
    }
}

impl JsonSchema for PastePoint {
    fn schema_name() -> Cow<'static, str> {
        "PastePoint".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<Point2>()
    }
}

/// Copy sketch curves and points, with the constraints and dimensions among them, to the sketch
/// clipboard. The design is unchanged.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CopyParams {
    /// Curve or point ids (default: the selected sketch geometry).
    #[serde(default)]
    pub entities: Option<StringList>,
    /// The sketch they are in (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchRef>,
}

/// Copy sketch curves and points as `sketch.copy` does, then delete them, in one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CutParams {
    /// Curve or point ids (default: the selected sketch geometry).
    #[serde(default)]
    pub entities: Option<StringList>,
    /// The sketch they are in (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchRef>,
}

/// Paste what was copied into a sketch, with its constraints and dimensions, in one undo step.
/// The pasted geometry is selected. Without `at` or `translate`, it goes a step away from the
/// original in its own sketch (further each time), and at the same coordinates in another.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PasteParams {
    /// The sketch point the copy's centre goes to. Takes the place of `translate`.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PastePoint")]
    pub at: Option<PastePoint>,
    /// How far to move the copy from where it was (mm).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PastePoint")]
    pub translate: Option<PastePoint>,
    /// The sketch to paste into (default: the one being edited).
    #[serde(default)]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.copy` and `sketch.cut` return: what is on the clipboard.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Copied {
    /// How many curves, those of copied text included.
    pub curves: usize,
    /// How many points that belong to no copied curve.
    pub points: usize,
    /// How many texts.
    pub texts: usize,
    /// How many constraints and dimensions among them came along.
    pub constraints: usize,
    /// The centre of what was copied, in sketch coordinates: where `at` puts it.
    pub center: [f64; 2],
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.paste` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Pasted {
    /// The new curves' ids.
    pub curves: Vec<String>,
    /// The new lone points' ids.
    pub points: Vec<String>,
    /// The new constraints' and dimensions' ids.
    pub constraints: Vec<String>,
    /// How far the copy was moved from where it was (mm).
    pub translate: [f64; 2],
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_clipboard_tests.rs"]
mod tests;
