//! Contract for `surface.blend` in `cmd/surface.rs`. It returns `add_feature`'s reply, a
//! [`super::features::FeatureResult`].

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use solidcraft_geom::fill::{MAX_CROWN, MIN_CROWN};
use solidcraft_kernel::{Construction, Continuity, MAX_EDGE_PICKS};

use super::check::whole;
use super::types::{FeatureCommon, Point3, feature_common};
use crate::params;

const CMD: &str = "surface.blend";

/// A blend surface bridging two picked edge chains of two bodies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BlendParams {
    /// The edge chain the blend starts from.
    #[serde(deserialize_with = "from_side")]
    pub from: BlendSide,
    /// The edge chain the blend ends on.
    #[serde(deserialize_with = "to_side")]
    pub to: BlendSide,
    /// One word per end (default g1): how much of the surface at the picked edges the blend repeats.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "continuity")]
    #[schemars(schema_with = "continuity_schema")]
    pub continuity: Option<[Continuity; 2]>,
    /// One number per end (default 0): how full it leaves, -0.9 flattest to 3 fullest.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "bulge")]
    #[schemars(schema_with = "bulge_schema")]
    pub bulge: Option<[f64; 2]>,
    /// A point each end leaves towards, when the faces at the picks do not say.
    #[serde(default, deserialize_with = "align")]
    #[schemars(schema_with = "point_pair_schema")]
    pub align: Option<[Point3; 2]>,
    /// A point on each chain its stations start from.
    #[serde(default, deserialize_with = "seam")]
    #[schemars(schema_with = "point_pair_schema")]
    pub seam: Option<[Point3; 2]>,
    /// Sew the blend onto its two bodies where the boundaries meet, replacing them.
    #[serde(default)]
    pub stitch: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// One end of a blend: a body and points on the edges of one chain of it (two picks on the same
/// edge is that one edge, more chain up).
#[derive(Debug, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BlendSide {
    pub body: String,
    #[schemars(length(min = 1, max = 10000))]
    pub edges: Vec<Point3>,
}

/// One end, with the handler's messages. A malformed point, which the handler drops, is refused.
fn side(key: &str, v: &Value) -> Result<BlendSide, String> {
    let body = v.get("body").and_then(Value::as_str).ok_or_else(|| format!("`{key}.body` must name a body"))?;
    let list = v.get("edges").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let edges: Vec<Point3> = list.iter().filter_map(params::vec3).map(Point3).collect();
    if edges.is_empty() || edges.len() > MAX_EDGE_PICKS {
        return Err(format!("`{key}.edges` must list 1 to 10000 points on the edges of a chain"));
    }
    if edges.len() != list.len() {
        return Err(format!("`{key}.edges`: a point must be [x, y, z]"));
    }
    if let Some(unknown) = v.as_object().and_then(|o| o.keys().find(|k| !matches!(k.as_str(), "body" | "edges"))) {
        return Err(format!("`{key}` has an unknown key `{unknown}`"));
    }
    Ok(BlendSide { body: body.to_string(), edges })
}

fn from_side<'de, D: Deserializer<'de>>(d: D) -> Result<BlendSide, D::Error> {
    side("from", &Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
}

fn to_side<'de, D: Deserializer<'de>>(d: D) -> Result<BlendSide, D::Error> {
    side("to", &Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
}

/// `continuity`, read by `params::continuity` itself, with its messages. `null` is refused, as
/// the handler refuses it.
fn continuity<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[Continuity; 2]>, D::Error> {
    let v = Value::deserialize(d)?;
    let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| D::Error::custom(whole("`continuity` must be two words, one per end")))?;
    let word = |x: Option<&Value>| {
        params::continuity(CMD, x, "a `continuity` entry", Construction::Bridge).map_err(|e| match e {
            crate::EngineError::BadParams { msg, .. } => D::Error::custom(whole(msg)),
            other => D::Error::custom(whole(other.to_string())),
        })
    };
    Ok(Some([word(list.first())?, word(list.get(1))?]))
}

fn continuity_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "string", "pattern": "^[gG][0-3]$"}, "minItems": 2, "maxItems": 2})
}

/// `bulge`, with the handler's messages. `null` is refused, as the handler refuses it.
fn bulge<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[f64; 2]>, D::Error> {
    let v = Value::deserialize(d)?;
    let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| D::Error::custom(whole("`bulge` must be two numbers, one per end")))?;
    let one = |x: Option<&Value>| {
        x.and_then(Value::as_f64)
            .filter(|x| x.is_finite() && (MIN_CROWN..=MAX_CROWN).contains(x))
            .ok_or_else(|| D::Error::custom(whole("each `bulge` entry must be between -0.9 (flattest) and 3 (fullest)")))
    };
    Ok(Some([one(list.first())?, one(list.get(1))?]))
}

fn bulge_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "number", "minimum": MIN_CROWN, "maximum": MAX_CROWN}, "minItems": 2, "maxItems": 2})
}

/// An optional pair of points, with the handler's messages: `which` says what each one is, and
/// `null` is none.
fn point_pair(key: &str, which: &str, v: &Value) -> Result<Option<[Point3; 2]>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| format!("`{key}` must be two points, {which}"))?;
    let point = |x: Option<&Value>| x.and_then(params::vec3).map(Point3).ok_or_else(|| format!("each `{key}` entry must be a point [x,y,z]"));
    Ok(Some([point(list.first())?, point(list.get(1))?]))
}

fn align<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[Point3; 2]>, D::Error> {
    point_pair("align", "one per end", &Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
}

fn seam<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[Point3; 2]>, D::Error> {
    point_pair("seam", "one on each chain", &Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
}

fn point_pair_schema(generator: &mut SchemaGenerator) -> Schema {
    let point = generator.subschema_for::<Point3>();
    json_schema!({"type": ["array", "null"], "items": point, "minItems": 2, "maxItems": 2})
}

#[cfg(test)]
#[path = "surface_blend_tests.rs"]
mod tests;
