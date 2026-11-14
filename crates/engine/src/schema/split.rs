//! Contracts for Split Body and Split Face in `cmd/features.rs` (#13127). Both reply with the plain
//! feature reply, [`super::features::FeatureResult`].
//!
//! Each takes one cutting tool, and the handler reads the first it finds in this order: a face of
//! a body (`tool`), a sketch, then a plane. A key that is `null` counts as not given. The
//! contracts check the form of every tool key a call gives, so in strict a call that gives a good
//! `tool` and a malformed `plane` is refused, where today the plane is ignored. A call that gives
//! no tool is the handler's to refuse, with its message for a missing `plane`; whether a body,
//! sketch, line or plane exists is the handler's to check too.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::check::whole;
use super::types::{FaceRefs, FeatureCommon, PlaneParam, SketchRef, ToolFace, feature_common};

/// Cut a body into pieces along a plane, a face of another body (its surface extended), or a
/// straight sketch line (along the sketch's normal). Each piece becomes a body.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplitBodyParams {
    /// The body to split, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    /// A face of a body to split along, its surface extended. Read when neither `sketch` nor
    /// `line` is given.
    #[serde(default)]
    pub tool: Option<ToolFace>,
    /// With `line`: the sketch, by feature id (not its name), whose line splits the body.
    #[serde(default)]
    pub sketch: Option<SketchFeatureId>,
    /// With `sketch`: a straight line's id; the body is split by the plane through it along the
    /// sketch's normal, which follows the line when the sketch changes.
    #[serde(default, deserialize_with = "line")]
    pub line: Option<String>,
    /// The plane to split across. Read when none of `tool`, `sketch` and `line` is given; then it
    /// is required.
    #[serde(default)]
    pub plane: Option<PlaneParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Divide faces of a body along a plane, a face of another body (its surface extended), or a
/// sketch's curves (along the sketch's normal), without cutting the body: each face becomes
/// several, and the body keeps its shape.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplitFaceParams {
    /// The faces to split.
    pub faces: FaceRefs,
    /// The body the faces are on, by name: where faces of several bodies meet at a point, whose
    /// face it is. Default: the body each point or name finds.
    #[serde(default)]
    pub body: Option<String>,
    /// A face of a body to split along, its surface extended.
    #[serde(default)]
    pub tool: Option<ToolFace>,
    /// A sketch, by id or name, whose curves split the faces along its normal. Read when no
    /// `tool` is given.
    #[serde(default)]
    pub sketch: Option<SketchRef>,
    /// With `sketch`: true joins its curves end to end first, so a closed loop of lines and arcs
    /// splits the faces along the loop only. Default: false, each curve on its own, an open one
    /// extended past its ends.
    #[serde(default)]
    pub loops: Option<bool>,
    /// The plane to split along. Read when neither `tool` nor `sketch` is given; then it is
    /// required.
    #[serde(default)]
    pub plane: Option<PlaneParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Split Body's `body`, with the handler's message for anything that isn't text.
fn body<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

/// Split Body's `line`, with the handler's message for anything that isn't text. `null` is no
/// line, as the handler reads it.
fn line<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom(whole("`line` must name a line of the sketch"))),
    }
}

/// Split Body's `sketch`: a sketch's feature id, a whole number from 0 (read with
/// `Value::as_u64`). Unlike Split Face's [`SketchRef`], a name is refused, with the handler's
/// message. Whether a sketch has that id is the handler's to check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SketchFeatureId(pub u64);

impl<'de> Deserialize<'de> for SketchFeatureId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().map(SketchFeatureId).ok_or_else(|| D::Error::custom(whole("`sketch` must be a sketch feature id")))
    }
}

impl JsonSchema for SketchFeatureId {
    fn schema_name() -> Cow<'static, str> {
        "SketchFeatureId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A sketch's feature id.", "type": "integer", "minimum": 0, "maximum": u64::MAX})
    }
}

#[cfg(test)]
#[path = "split_tests.rs"]
mod tests;
