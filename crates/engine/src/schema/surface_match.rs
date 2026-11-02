//! Contract for `surface.match` in `cmd/surface_match.rs` (#10981). It returns `add_feature`'s
//! reply, a [`super::features::FeatureResult`].

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use solidcraft_geom::Vec3;
use solidcraft_kernel::Continuity;

use super::check::whole;
use super::types::{Point3, StringList, present};
use crate::params;

/// Rebuild one face of a surface body so that its boundary nearest a picked edge chain of another
/// body lies on that chain and repeats that body's shape there up to `order`, while the opposite
/// boundary keeps its place. The body keeps its name: a match changes it rather than making one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MatchSurfaceParams {
    /// The surface body whose face is rebuilt, by name.
    #[serde(deserialize_with = "body")]
    pub body: String,
    #[serde(default)]
    pub faces: Option<MatchFaces>,
    /// The body the seam must meet, by name.
    #[serde(deserialize_with = "target")]
    pub target: String,
    pub edges: MatchEdges,
    /// How closely the seam follows the target: `g1` (tangent), `g2` (curvature, the default) or
    /// `g3`, in any case.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "MatchOrder")]
    pub order: Option<MatchOrder>,
    // Not `FeatureCommon`: a match makes no body, so `body_name` and `body_names` would do
    // nothing. `participants` stays because `timeline.redefine` passes a feature's own back.
    /// The feature's name in the timeline.
    #[serde(default)]
    pub name: Option<String>,
    /// The bodies taking part, which every feature records; each an existing body or `*`. A match
    /// changes only `body`, whatever this lists.
    #[serde(default)]
    pub participants: Option<StringList>,
}

/// A body name, with the handler's message for anything that isn't text.
fn body<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`body` is required"))),
    }
}

/// The target's name, with the handler's message for anything that isn't text.
fn target<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`target` is required"))),
    }
}

/// Points `params::vec3` reads, or none if any isn't one.
fn points(v: &[Value]) -> Option<Vec<Vec3>> {
    v.iter().map(params::vec3).collect()
}

/// Points on the face to rebuild, at most 64 (default: the body's first face).
#[derive(Clone, Debug, PartialEq)]
pub struct MatchFaces(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for MatchFaces {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let faces =
            Value::deserialize(d)?.as_array().and_then(|a| points(a)).ok_or_else(|| D::Error::custom("must be a list of points [x, y, z]"))?;
        if faces.len() > 64 {
            return Err(D::Error::custom(whole("too many points on the face to rebuild")));
        }
        Ok(MatchFaces(faces))
    }
}

impl JsonSchema for MatchFaces {
    fn schema_name() -> Cow<'static, str> {
        "MatchFaces".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "Points on the face to rebuild, at most 64 (default: the body's first face).",
            "type": "array",
            "items": point,
            "maxItems": 64
        })
    }
}

/// Points on one edge chain of the target: the rebuilt boundary takes its place. 1 to 10 000,
/// with the handler's message for anything else.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchEdges(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for MatchEdges {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_array()
            .and_then(|a| points(a))
            .filter(|edges| !edges.is_empty() && edges.len() <= 10_000)
            .map(MatchEdges)
            .ok_or_else(|| D::Error::custom(whole("`edges` must list 1 to 10000 points on the target's edge chain")))
    }
}

impl JsonSchema for MatchEdges {
    fn schema_name() -> Cow<'static, str> {
        "MatchEdges".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "Points on one edge chain of the target: the rebuilt boundary takes its place.",
            "type": "array",
            "items": point,
            "minItems": 1,
            "maxItems": 10000
        })
    }
}

/// A match's continuity, as `Continuity::parse` reads it, with the handler's message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchOrder(pub Continuity);

impl<'de> Deserialize<'de> for MatchOrder {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_str()
            .and_then(Continuity::parse)
            .filter(|o| *o != Continuity::G0)
            .map(MatchOrder)
            .ok_or_else(|| D::Error::custom("must be g1, g2 or g3"))
    }
}

impl JsonSchema for MatchOrder {
    fn schema_name() -> Cow<'static, str> {
        "MatchOrder".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "pattern": "^[gG][123]$"})
    }
}

#[cfg(test)]
#[path = "surface_match_tests.rs"]
mod tests;
