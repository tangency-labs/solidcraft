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
use super::types::{FaceRefs, FeatureCommon, PlaneParam, Point3, SketchRef, ToolFace, feature_common};

/// Cut a body into pieces along a plane, a face of another body (its surface extended), or sketch
/// curves (along the sketch's normal). Each piece becomes a body. A surface body split with
/// `tool_sheet` `body` is cut into its connected regions, each a surface body.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplitBodyParams {
    /// The body to split, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    /// A face of a body to split along, its surface extended. Read when none of `sketch`, `line`
    /// and `curves` is given.
    #[serde(default)]
    pub tool: Option<ToolFace>,
    /// With `tool`: use only the face's infinite plane, which follows the face. The face must
    /// remain planar. Default: false, use its extended surface and its sheet.
    #[serde(default)]
    pub tool_plane: Option<bool>,
    /// With `tool`: what of the tool body cuts. `extended` (the default): the face's surface
    /// extended. `faces`: the `tool` face and the faces at `tool_faces`, exactly as they are, not
    /// extended. `body`: every face of the tool's surface body, exactly as it is; it also splits
    /// a surface body, into the regions the tool bodies cut it into.
    #[serde(default)]
    pub tool_sheet: Option<ToolSheet>,
    /// With `tool_sheet` `faces`: a point on each other face of the tool body that cuts.
    #[serde(default)]
    pub tool_faces: Option<Vec<Point3>>,
    /// With `tool_sheet` `body`: more surface bodies, by name, that cut with the tool's body,
    /// each whole and exactly as it is. Each cuts the pieces it crosses through and must cut one.
    #[serde(default)]
    pub tool_bodies: Option<Vec<String>>,
    /// With `line` or `curves`: the sketch, by feature id (not its name), whose curves split the
    /// body.
    #[serde(default)]
    pub sketch: Option<SketchFeatureId>,
    /// With `sketch`: a curve's id. A straight line splits the body by the plane through it along
    /// the sketch's normal; any other curve (an arc, a spline) by the sheet it sweeps along the
    /// normal through the body, its open ends continued along their tangents past the body. The
    /// split follows the curve when the sketch changes.
    #[serde(default, deserialize_with = "line")]
    pub line: Option<String>,
    /// With `sketch`: more curve ids, joined end to end with `line` (when given) into one chain
    /// that splits the body as the sheet it sweeps, as for a single curve.
    #[serde(default, deserialize_with = "curves")]
    pub curves: Option<Vec<String>>,
    /// With `sketch`: true (the default) continues an open chain's ends along their tangents past
    /// the body; false cuts with the curves exactly as drawn, refused where they do not cut the
    /// body through.
    #[serde(default, deserialize_with = "extend")]
    pub extend: Option<bool>,
    /// The plane to split across. Read when none of `tool`, `sketch`, `line` and `curves` is given;
    /// then it is required.
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

/// What of Split Body's tool body cuts (`tool_sheet`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolSheet {
    /// The tool face's surface, extended.
    Extended,
    /// The picked faces, exactly as they are.
    Faces,
    /// Every face of the tool's surface body, exactly as it is.
    Body,
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
        _ => Err(D::Error::custom(whole("`line` must name a curve of the sketch"))),
    }
}

/// Split Body's `extend`, with the handler's message for anything that isn't true or false. `null`
/// is the default, as the handler reads it.
fn extend<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(D::Error::custom(whole("`extend` must be true or false"))),
    }
}

/// Split Body's `curves`, with the handler's message for anything that isn't a list of text.
/// `null` is no curves, as the handler reads it.
fn curves<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    let bad = || D::Error::custom(whole("`curves` must be a list of curve ids of the sketch"));
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(list) => list.into_iter().map(|c| c.as_str().map(str::to_string).ok_or_else(bad)).collect::<Result<_, _>>().map(Some),
        _ => Err(bad()),
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
