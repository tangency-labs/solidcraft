//! Contract and reply for Surface Patch (`surface.patch`) in `cmd/surface.rs` (#13434).
//!
//! A patch fills a closed loop of a body's edges (`edges`, with `body`), or, without edges, a
//! sketch's regions (`sketch`, `profiles`). The contract checks the form of each key a call gives
//! and leaves which kind of patch it makes to the handler, which refuses `continuity` and
//! `points` on a sketch patch. Whether the body, sketch and loop exist, and whether a point or
//! edge lies on the loop, are the handler's to check too.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_geom::fill::{MAX_CROWN, MIN_CROWN};
use solidcraft_geom::fill_general::MAX_FILL_POINTS;
use solidcraft_kernel::{Construction, Continuity};

use super::check::whole;
use super::features::FeatureResult;
use super::types::{FeatureCommon, IdOrName, Point3, ProfilesParam, feature_common, present};
use crate::params;

/// Fill a closed loop with a surface body: one face on a loop of a body's edges (planar when the
/// loop is, else a smooth B-spline surface meeting the faces beside it as `continuity` says), or
/// planar patches of a sketch's regions.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SurfacePatchParams {
    #[serde(default)]
    pub edges: Option<PatchEdges>,
    /// With `edges`: the body whose edges they are, by name. Default: the body each point finds.
    #[serde(default)]
    pub body: Option<String>,
    /// Without `edges`: the sketch whose regions to fill, by id or name. Default: the active
    /// sketch, else the last one.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// Without `edges`: the regions to fill, as `solid.extrude` picks them. Default: every profile.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ProfilesParam")]
    pub profiles: Option<ProfilesParam>,
    /// Without `edges`: each index or point pick is exactly the region it names. Without it, a
    /// pick on a drawn profile that reference geometry cuts into regions takes the whole drawn
    /// profile.
    #[serde(default)]
    pub exact_regions: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PatchContinuityParam")]
    pub continuity: Option<PatchContinuityParam>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "PatchPoints")]
    pub points: Option<PatchPoints>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Crown")]
    pub crown: Option<Crown>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// The handler's message for `edges` it can't read. The handler drops an item that isn't a point
/// and makes a sketch patch from a value that isn't a list; the contract refuses both.
const EDGES_MESSAGE: &str = "`edges` must list points [x, y, z] on a closed loop of a body's edges";

/// Points on a closed loop of a body's edges, at most 10,000. An empty list makes a sketch patch,
/// as no `edges` does.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchEdges(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for PatchEdges {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let points = v
            .as_array()
            .and_then(|items| items.iter().map(params::vec3).collect::<Option<Vec<_>>>())
            .ok_or_else(|| D::Error::custom(whole(EDGES_MESSAGE)))?;
        if points.len() > 10_000 {
            return Err(D::Error::custom(whole("too many edges")));
        }
        Ok(PatchEdges(points))
    }
}

impl JsonSchema for PatchEdges {
    fn schema_name() -> Cow<'static, str> {
        "PatchEdges".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "Points in mm on a closed loop of a body's edges, at most 10,000: the patch is one face on them. Without any, the patch fills a sketch's regions.",
            "type": "array",
            "items": point,
            "maxItems": 10000
        })
    }
}

/// A patch's continuity word as `params::continuity` reads it for a patch (g0 to g4, ignoring
/// case and spaces at either end), with its message.
fn order(v: Option<&Value>) -> Result<Continuity, String> {
    params::continuity("surface.patch", v, "a patch's continuity", Construction::Patch).map_err(|e| match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    })
}

/// A list of `{edge, g}`, as the handler's `patch_continuity` reads one, with its messages.
fn edge_orders(list: &Value) -> Result<(), String> {
    let list = list.as_array().ok_or("`continuity` edges must be a list of {edge: [x,y,z], g}")?;
    if list.len() > 10_000 {
        return Err("`continuity` lists at most 10000 edges".into());
    }
    for e in list {
        e.get("edge").and_then(params::vec3).ok_or("a `continuity` entry's `edge` must be a point [x,y,z]")?;
        order(e.get("g"))?;
    }
    Ok(())
}

/// How the patch on a loop of edges meets the faces beside it: one word for every edge, a list of
/// `{edge, g}` for some edges (the rest g0), or `{all, edges}`. Checked as the handler's
/// `patch_continuity` checks it, in its order and with its messages; other keys in an object or
/// an entry are ignored, as the handler ignores them. Kept as given, for the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchContinuityParam(pub Value);

impl<'de> Deserialize<'de> for PatchContinuityParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let problem = match &v {
            Value::String(_) => order(Some(&v)).err(),
            Value::Array(_) => edge_orders(&v).err(),
            Value::Object(o) => {
                o.get("all").and_then(|all| order(Some(all)).err()).or_else(|| o.get("edges").and_then(|list| edge_orders(list).err()))
            }
            _ => {
                let (lowest, highest) = Construction::Patch.orders();
                Some(format!("`continuity` must be {}, or a list of {{edge, g}}", Continuity::words(lowest, highest)))
            }
        };
        match problem {
            Some(why) => Err(D::Error::custom(whole(why))),
            None => Ok(PatchContinuityParam(v)),
        }
    }
}

/// A continuity word, g0 to g4. The handler also takes capitals and spaces at either end, which a
/// schema enum can't say; the description does.
fn order_schema() -> Value {
    serde_json::json!({"type": "string", "enum": ["g0", "g1", "g2", "g3", "g4"]})
}

impl JsonSchema for PatchContinuityParam {
    fn schema_name() -> Cow<'static, str> {
        "PatchContinuity".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let entry = serde_json::json!({"type": "object", "properties": {"edge": point, "g": order_schema()}, "required": ["edge", "g"]});
        let list = serde_json::json!({"type": "array", "maxItems": 10000, "items": entry});
        json_schema!({
            "description": "With `edges`: how the patch meets the faces beside the loop. A word for every edge: g0 (the default: it meets them), g1 (also their tangent planes), g2 (also their curvature across the edges), g3 (also its rate of change) or g4 (also the change of that rate); a list of {\"edge\": [x, y, z], \"g\"} for those edges, the rest g0; or {\"all\", \"edges\"}: `all` for the edges `edges` doesn't list. Words ignore case.",
            "anyOf": [
                order_schema(),
                list,
                {"type": "object", "properties": {"all": order_schema(), "edges": list}}
            ]
        })
    }
}

/// Points the patch on a loop of edges passes through: at most 64, each finite.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchPoints(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for PatchPoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let list = v
            .as_array()
            .filter(|a| a.len() <= MAX_FILL_POINTS)
            .ok_or_else(|| D::Error::custom(whole("`points` must list at most 64 points [x,y,z]")))?;
        list.iter()
            .map(|q| params::vec3(q).filter(|q| q.is_finite()).ok_or_else(|| D::Error::custom(whole("a patch point must be a finite [x,y,z]"))))
            .collect::<Result<_, _>>()
            .map(PatchPoints)
    }
}

impl JsonSchema for PatchPoints {
    fn schema_name() -> Cow<'static, str> {
        "PatchPoints".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "With `edges`: points in mm the patch passes through, at most 64.",
            "type": "array",
            "items": point,
            "maxItems": MAX_FILL_POINTS
        })
    }
}

/// How full a fitted patch's middle is made, as a share of the loop's size: from -0.9 (flatter)
/// to 3 (fuller). Default: 0. A planar loop's patch is its plane whatever this is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crown(pub f64);

impl<'de> Deserialize<'de> for Crown {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?
            .as_f64()
            .filter(|x| (MIN_CROWN..=MAX_CROWN).contains(x))
            .map(Crown)
            .ok_or_else(|| D::Error::custom(whole("`crown` must be a number between -0.9 and 3")))
    }
}

impl JsonSchema for Crown {
    fn schema_name() -> Cow<'static, str> {
        "Crown".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "How full a fitted patch's middle is made, as a share of the loop's size: from -0.9 (flatter) to 3 (fuller). Default: 0. A planar loop's patch is its plane whatever this is.",
            "type": "number",
            "minimum": MIN_CROWN,
            "maximum": MAX_CROWN
        })
    }
}

/// What `surface.patch` returns: the feature reply, with the crown and continuity it was built
/// with.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PatchResult {
    #[serde(flatten)]
    pub feature: FeatureResult,
    /// The crown it was built with: `crown`, or 0.
    pub crown: f64,
    /// How it meets the faces beside its loop, as built; empty for a sketch patch.
    pub continuity: PatchContinuityResult,
}

/// A patch's continuity as the design stores it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PatchContinuityResult {
    /// The order at every edge `edges` doesn't list, g1 to g4; left out for g0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "order_word")]
    pub all: Option<String>,
    /// The edges given an order of their own; left out when none are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<EdgeContinuityResult>,
}

/// One edge given an order of its own.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EdgeContinuityResult {
    /// A point in mm on the edge.
    pub edge: [f64; 3],
    /// Its order, g0 to g4.
    #[schemars(schema_with = "order_word")]
    pub g: String,
}

/// A continuity word in a reply, as the design stores it: g0 to g4.
fn order_word(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["g0", "g1", "g2", "g3", "g4"]})
}

#[cfg(test)]
#[path = "surface_patch_tests.rs"]
mod tests;
