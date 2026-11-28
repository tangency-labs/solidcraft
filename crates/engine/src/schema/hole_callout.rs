//! Contracts and result types for the command in `cmd/hole_callout.rs` (#7605, W29a).
//!
//! The reply mirrors [`solidcraft_doc::drawing::hole_callout::Resolution`] and its callout, which
//! the handler converts field by field, so the schema can't drift from what it returns.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::drawing::hole_callout::{self as doc, MAX_DECIMALS};

/// A Hole feature, by its id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoleFeatureParam(pub u64);

impl<'de> Deserialize<'de> for HoleFeatureParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().filter(|id| *id >= 1).map(HoleFeatureParam).ok_or_else(|| D::Error::custom("must be a Hole feature's id"))
    }
}

impl JsonSchema for HoleFeatureParam {
    fn schema_name() -> Cow<'static, str> {
        "HoleFeatureId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The Hole feature's id.", "type": "integer", "minimum": 1})
    }
}

/// How many decimals a callout's numbers take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecimalsParam(pub u8);

impl<'de> Deserialize<'de> for DecimalsParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| *n <= MAX_DECIMALS)
            .map(DecimalsParam)
            .ok_or_else(|| D::Error::custom(format!("must be 0 to {MAX_DECIMALS}")))
    }
}

impl JsonSchema for DecimalsParam {
    fn schema_name() -> Cow<'static, str> {
        "HoleCalloutDecimals".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer", "minimum": 0, "maximum": MAX_DECIMALS})
    }
}

/// What a drawing's callout of one Hole feature's hole says, worked out from the model. Read-only:
/// nothing is saved and nothing is placed on a sheet. Works with a drawing open or with none.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(deny_unknown_fields)]
pub struct CalloutParams {
    /// The Hole feature's id.
    pub feature: HoleFeatureParam,
    /// Its sketch point id, for a hole drilled at sketch points (one of the holes it drills).
    /// Default: none, for a hole placed by position or a Single hole.
    #[serde(default)]
    pub point: Option<String>,
    /// The length unit of the numbers, any of the expression unit table's (`mm`, `in`, `yd`,
    /// …). Default: the open drawing's fixed unit, else the design's.
    #[serde(default)]
    pub units: Option<String>,
    /// 0 to 6 (inches show at least 3). Default: 2, as a new dimension's.
    #[serde(default)]
    pub decimals: Option<DecimalsParam>,
    /// The content language of the words (`THRU`), a BCP 47 tag. Default: the open drawing's,
    /// else `en`.
    #[serde(default)]
    pub language: Option<String>,
}

/// The hole a callout names, by its ids as given.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HoleResult {
    /// The Hole feature's id.
    pub feature: u64,
    /// Its sketch point id, when the call named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
}

/// How far a hole goes.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtentResult {
    /// Through the material.
    Through,
    /// To this depth below its start face (to the shoulder of a drill point), in mm.
    Blind { depth: f64 },
}

/// The hole's form at its top, in mm and radians.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FormResult {
    /// A plain hole, flat at the bottom when blind.
    Simple,
    /// A drill point at the bottom of a blind hole.
    Drilled {
        /// The drill point's included angle.
        tip_angle: f64,
    },
    /// A counterbore at the top.
    Counterbore {
        /// The counterbore's diameter.
        diameter: f64,
        /// Its depth from the face the hole starts on.
        depth: f64,
    },
    /// A countersink at the top.
    Countersink {
        /// The countersink's diameter at the face the hole starts on.
        diameter: f64,
        /// Its included angle.
        angle: f64,
    },
}

/// What the feature drills there, in mm and radians.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CalloutValues {
    /// How many holes the feature drills (all of them, which may differ in extent; see `extent`).
    pub count: usize,
    /// The drilled diameter (for a tapped hole, the tap drill).
    pub diameter: f64,
    /// A tapped hole's thread (`M8`, `1/4-20 UNC`, `G 1/4`), shown in place of the diameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The thread's depth when the feature gives it one, from the face the hole starts on as the
    /// blind depth is: the feature's own thread depth (which starts below any counterbore or
    /// countersink) plus that recess. Without one, the thread runs the hole's length.
    /// Present whenever the feature sets one; the text drops it when it reaches the hole's floor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_depth: Option<f64>,
    /// This hole's extent.
    pub extent: ExtentResult,
    /// The hole's form at its top.
    pub form: FormResult,
    /// Where the hole starts, on the face it is drilled into, in the frame of the model as it is
    /// now (after a later `solid.move` of its body) and of the hole's own component.
    pub start: [f64; 3],
    /// The name of that face, in the model before the hole (empty when it has none).
    pub start_face: String,
    /// The drilling direction, into the material, in the frame of `start`.
    pub direction: [f64; 3],
}

/// What `drawing.hole_callout` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CalloutResult {
    /// The hole the call named.
    pub hole: HoleResult,
    /// `resolved`, `suppressed` (the feature is), `failed` (it did not evaluate; a later feature
    /// removed the hole, moved it some way other than `solid.move`, or changed its diameter or
    /// depth; or it is in sheet metal, not supported yet) or `lost` (the feature or the point is
    /// gone; never moved to another hole). A string with these values, as the sibling drawing
    /// replies' statuses are.
    pub status: String,
    /// Why it failed or is lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What it says, when resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<CalloutValues>,
    /// The callout's text, one line each.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
    /// The lines joined by newlines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

impl From<&doc::HoleCallout> for CalloutValues {
    fn from(c: &doc::HoleCallout) -> Self {
        // Every field by name, so one added to the callout can't be left out of the reply.
        let doc::HoleCallout { count, diameter, thread, thread_depth, extent, form, start, start_face, direction } = c;
        CalloutValues {
            count: *count,
            diameter: *diameter,
            thread: thread.clone(),
            thread_depth: *thread_depth,
            extent: match *extent {
                doc::Extent::Through => ExtentResult::Through,
                doc::Extent::Blind { depth } => ExtentResult::Blind { depth },
            },
            form: match *form {
                doc::Form::Simple => FormResult::Simple,
                doc::Form::Drilled { tip_angle } => FormResult::Drilled { tip_angle },
                doc::Form::Counterbore { diameter, depth } => FormResult::Counterbore { diameter, depth },
                doc::Form::Countersink { diameter, angle } => FormResult::Countersink { diameter, angle },
            },
            start: *start,
            start_face: start_face.clone(),
            direction: *direction,
        }
    }
}

#[cfg(test)]
#[path = "hole_callout_tests.rs"]
mod tests;
