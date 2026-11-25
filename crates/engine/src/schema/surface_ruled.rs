//! Contract for Ruled Surface (`surface.ruled`) in `cmd/surface.rs` (#13134). It replies with the
//! plain feature reply, [`super::features::FeatureResult`].
//!
//! The handler reads `angle` only in angle mode and `direction` only in angle and direction
//! modes, and says when either is missing there. The contract checks their form whenever they are
//! given and leaves which mode needs them to the handler. Whether the body exists, the points lie
//! on a chain of its edges and on a face, and the expressions evaluate are the handler's to check.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use solidcore_record::Vec3;
// `Vec3Geom::len`, which a direction's check measures as the handler does.
use solidcraft_geom::prelude::*;

use super::check::whole;
use super::types::{AngleExpr, FeatureCommon, LengthExpr, Point3, RuledCornersParam, RuledModeParam, feature_common};
use crate::params;

/// Make a surface body: a strip that reaches `distance` out from a chain of a body's edges, along
/// rulings set by `mode`. A constant ruling, or one keeping its angle to an arc's axis, gives an
/// exact plane, cylinder or cone; where the rulings turn freely along an edge, the strip is the
/// exact ruled surface to a far curve of its own.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfaceRuledParams {
    /// The body whose edges the strip starts from, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    pub edges: RuledEdges,
    /// How far the strip reaches from the edges.
    pub distance: LengthExpr,
    #[serde(default)]
    pub mode: Option<RuledModeParam>,
    /// Angle mode: the rulings' angle from `direction`, else from the face's normal, turned away
    /// from the face (90 deg from the normal is tangent). Required in angle mode; read in no
    /// other.
    #[serde(default)]
    pub angle: Option<AngleExpr>,
    #[serde(default)]
    pub direction: Option<RuledDirection>,
    #[serde(default)]
    pub face: Option<RuledFace>,
    /// Make the strip on the other side. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    #[serde(default)]
    pub corners: Option<RuledCornersParam>,
    #[serde(default)]
    pub far: Option<RuledFar>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// The handler's message for `body` when it isn't text.
fn body<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

/// The handler's message for `edges` it can't read: not a list, no points, or over 10,000.
const EDGES_MESSAGE: &str = "`edges` must list points on a chain of the body's edges";

/// The handler's message for `far` it can't read.
const FAR_MESSAGE: &str = "`far` must be a list, per edge, of null or [[x, y, z], ...]";

/// Points on a chain of the body's edges, 1 to 10,000, as `params::vec3` reads each. The handler
/// drops an item that isn't a point and takes the rest; the contract refuses it, with the
/// handler's message for a list it can't read.
#[derive(Clone, Debug, PartialEq)]
pub struct RuledEdges(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for RuledEdges {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let points = v.as_array().and_then(|items| items.iter().map(params::vec3).collect::<Option<Vec<_>>>());
        match points {
            Some(points) if !points.is_empty() && points.len() <= 10_000 => Ok(RuledEdges(points)),
            _ => Err(D::Error::custom(whole(EDGES_MESSAGE))),
        }
    }
}

impl JsonSchema for RuledEdges {
    fn schema_name() -> Cow<'static, str> {
        "RuledEdges".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "Points in mm on a chain of the body's edges, open or closed: 1 to 10,000.",
            "type": "array",
            "items": point,
            "minItems": 1,
            "maxItems": 10000
        })
    }
}

/// The rulings' direction in direction mode, or the reference the angle is measured from in angle
/// mode (a pull direction for a draft): not zero. Read in those modes only. The schema can't say
/// "not zero", so a zero direction matches it and the check refuses it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuledDirection(pub Vec3);

impl<'de> Deserialize<'de> for RuledDirection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // As the handler reads it: a point whose length is over 1e-12.
        params::vec3(&Value::deserialize(d)?)
            .filter(|v| v.len() > 1e-12)
            .map(RuledDirection)
            .ok_or_else(|| D::Error::custom(whole("`direction` must be a non-zero [x, y, z]")))
    }
}

impl JsonSchema for RuledDirection {
    fn schema_name() -> Cow<'static, str> {
        "RuledDirection".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = generator.subschema_for::<Point3>();
        schema.insert(
            "description".into(),
            "Direction mode: the rulings' direction; angle mode: the reference the angle is measured from (a pull direction for a draft). Not zero. Required in direction mode; read in no other mode but angle.".into(),
        );
        schema
    }
}

/// A point on the face the rulings' directions come from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuledFace(pub Vec3);

impl<'de> Deserialize<'de> for RuledFace {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec3(&Value::deserialize(d)?).map(RuledFace).ok_or_else(|| D::Error::custom(whole("`face` must be [x, y, z] on a face")))
    }
}

impl JsonSchema for RuledFace {
    fn schema_name() -> Cow<'static, str> {
        "RuledFace".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = generator.subschema_for::<Point3>();
        schema.insert(
            "description".into(),
            "A point in mm on the face the rulings' directions come from. Default: the face beside the most picked edges.".into(),
        );
        schema
    }
}

/// A free strip's far-curve handles, per chain edge in order: `null` for the default curve, or the
/// interior control points as the default curve's handles list them. At most 10,000 edges and
/// 100,000 points each. Read in order, as the handler reads it, so the first item it can't read
/// gives the message.
#[derive(Clone, Debug, PartialEq)]
pub struct RuledFar(pub Vec<Option<Vec<Vec3>>>);

impl<'de> Deserialize<'de> for RuledFar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let lists = v.as_array().filter(|a| a.len() <= 10_000).ok_or_else(|| D::Error::custom(whole(FAR_MESSAGE)))?;
        let mut far = Vec::with_capacity(lists.len());
        for list in lists {
            far.push(match list {
                Value::Null => None,
                Value::Array(points) if points.len() <= 100_000 => Some(
                    points
                        .iter()
                        .map(params::vec3)
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| D::Error::custom(whole("`far` handles must be [x, y, z]")))?,
                ),
                _ => return Err(D::Error::custom(whole(FAR_MESSAGE))),
            });
        }
        Ok(RuledFar(far))
    }
}

impl JsonSchema for RuledFar {
    fn schema_name() -> Cow<'static, str> {
        "RuledFar".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A free strip's far-curve handles, per chain edge in order: null for the default curve, or the interior control points in mm, as the default curve's handles list them.",
            "type": "array",
            "maxItems": 10000,
            "items": {"anyOf": [{"type": "null"}, {"type": "array", "maxItems": 100000, "items": point}]}
        })
    }
}

#[cfg(test)]
#[path = "surface_ruled_tests.rs"]
mod tests;
