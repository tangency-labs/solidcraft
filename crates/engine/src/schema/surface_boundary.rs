//! Contract for `surface.boundary` in `cmd/surface_boundary.rs` (#11083). It returns
//! `add_feature`'s reply, a [`super::features::FeatureResult`].

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use solidcraft_geom::fill::{MAX_CROWN, MIN_CROWN};
use solidcraft_kernel::{Continuity, MAX_CHAIN_CURVES, MAX_EDGE_PICKS};

use super::check::whole;
use super::types::{FeatureCommon, IdOrName, OperationParam, Point3, feature_common, present};
use crate::params;

/// The fewest stations a patch can hold the widest continuity pair with: a G3 side needs four
/// rows at each of two opposite sides, so eight.
pub const MIN_SAMPLES: usize = 8;
/// The most stations a boundary may carry (the kernel's own cap).
pub const MAX_SAMPLES: usize = 512;
/// The most curves one direction of a network may list (the kernel's own cap).
pub const MAX_NETWORK_CURVES: usize = solidcraft_geom::network::MAX_NETWORK_CURVES;

const CMD: &str = "surface.boundary";

/// A surface through curves: either one patch through a loop of two to four curves of one sketch
/// (`curves`, with `continuity`, `crown` and `samples`), or one surface through a curve network
/// (`u`, with `v` and `operation`). The keys of one form are refused with the other's.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BoundaryParams {
    /// The sketch the loop's curves, or a network's bare curve ids, are in (default: the one being
    /// edited).
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "IdOrName")]
    pub sketch: Option<IdOrName>,
    /// The loop: 2 to 4 sketch curve ids in loop order, each ending where the next begins and the
    /// last where the first does. Two: the patch spans between them, leaving each along the
    /// tangent across it. Three or four: the flat patch of the loop, three collapsing the side
    /// opposite the first curve to the corner there.
    #[serde(default, deserialize_with = "curves")]
    #[schemars(schema_with = "curves_schema")]
    pub curves: Option<Vec<String>>,
    #[serde(default)]
    pub continuity: Option<BoundaryContinuity>,
    /// With two curves: how full the middle is, without touching a boundary, -0.9 to 3 (default 0).
    #[serde(default, deserialize_with = "crown")]
    #[schemars(schema_with = "crown_schema")]
    pub crown: Option<f64>,
    /// Stations per boundary: a whole number from 8 to 512 (default 25).
    #[serde(default, deserialize_with = "samples")]
    #[schemars(schema_with = "samples_schema")]
    pub samples: Option<f64>,
    #[serde(default)]
    pub u: Option<UCurves>,
    #[serde(default)]
    pub v: Option<VCurves>,
    /// A network's: `new` (the default) or `join`, which sews the surface into the body of its
    /// first edge curve, a solid when it closes the gap.
    #[serde(default)]
    pub operation: Option<OperationParam>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// The message of a handler's parameter error, for a reader the contract shares.
fn message(e: crate::EngineError) -> String {
    match e {
        crate::EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}

/// The loop's curves, read by `params::string_list` itself, with its messages.
fn curves<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    let v = Value::deserialize(d)?;
    params::string_list(CMD, &json!({ "curves": v }), "curves").map(Some).map_err(|e| D::Error::custom(whole(message(e))))
}

fn curves_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "maxItems": 10000}, {"type": "null"}]})
}

/// `crown`, with the handler's message; `null` is 0.
fn crown<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .filter(|x| x.is_finite() && (MIN_CROWN..=MAX_CROWN).contains(x))
            .map(Some)
            .ok_or_else(|| D::Error::custom(whole("`crown` must be a number between -0.9 and 3"))),
    }
}

fn crown_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "null"], "minimum": MIN_CROWN, "maximum": MAX_CROWN})
}

/// `samples`, with the handler's message; `null` is the default.
fn samples<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => {
            v.as_f64().filter(|x| x.is_finite() && x.fract() == 0.0 && (MIN_SAMPLES as f64..=MAX_SAMPLES as f64).contains(x)).map(Some).ok_or_else(
                || D::Error::custom(whole(format!("`samples` must be a whole number of stations between {MIN_SAMPLES} and {MAX_SAMPLES}"))),
            )
        }
    }
}

fn samples_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["number", "null"], "minimum": MIN_SAMPLES, "maximum": MAX_SAMPLES, "multipleOf": 1})
}

/// How much of the surface across each boundary the patch repeats: one of g0, g1, g2 or g3 for
/// every curve, or one per curve, with the handler's messages (default g1). A sketch curve's own
/// plane, so g1 and up hold exactly and g0 lets the middle bow.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundaryContinuity(pub Vec<Continuity>);

impl<'de> Deserialize<'de> for BoundaryContinuity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(w) => Continuity::parse(&w)
                .map(|c| BoundaryContinuity(vec![c]))
                .ok_or_else(|| D::Error::custom(whole("`continuity` must be g0, g1, g2 or g3"))),
            Value::Array(list) => list
                .iter()
                .map(|x| x.as_str().and_then(Continuity::parse))
                .collect::<Option<Vec<_>>>()
                .map(BoundaryContinuity)
                .ok_or_else(|| D::Error::custom(whole("a `continuity` entry must be g0, g1, g2 or g3"))),
            _ => Err(D::Error::custom(whole("`continuity` must be one word, or one per curve"))),
        }
    }
}

impl JsonSchema for BoundaryContinuity {
    fn schema_name() -> Cow<'static, str> {
        "BoundaryContinuity".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let word = json!({"type": "string", "pattern": "^[gG][0-3]$"});
        json_schema!({
            "description": "How much of the surface across each boundary the patch repeats: g0, g1 (the default), g2 or g3 for every curve, or one per curve. A sketch curve's own plane, so g1 and up hold exactly and g0 lets the middle bow.",
            "anyOf": [word, {"type": "array", "items": word}]
        })
    }
}

/// One direction of a network (`key` is `u` or `v`), checked where the handler checks it without
/// the design, with its messages: each item a curve id, `{sketch?, curves}` (planar or 3D sketch
/// curves meeting end to end), `{body, edges}` (points on a chain of a body's edges) or `{point}`
/// (a side collapsed to a point), and a `continuity` (g0, g1 or g2) only on a body's edges.
fn network(key: &str, v: Value) -> Result<(), String> {
    let list = match v {
        Value::Array(a) => a,
        _ => return Err(format!("`{key}` must list curves")),
    };
    if list.len() > MAX_NETWORK_CURVES {
        return Err(format!("`{key}` may list at most {MAX_NETWORK_CURVES} curves"));
    }
    for (k, item) in list.iter().enumerate() {
        let name = format!("{key}[{k}]");
        let (mut curves, mut on_body, mut point) = (1, false, false);
        if let Value::Object(o) = item {
            let has = |k: &str| o.get(k).is_some_and(|v| !v.is_null());
            let kinds = [has("curves"), has("body") || has("edges"), has("point")];
            if kinds.iter().filter(|x| **x).count() != 1 {
                return Err(format!("{name} must be one of {{sketch?, curves}}, {{body, edges}} or {{point}}"));
            }
            if has("point") {
                o.get("point").and_then(params::vec3).ok_or_else(|| format!("{name}.point must be [x, y, z]"))?;
                point = true;
            } else if kinds[1] {
                o.get("body").and_then(Value::as_str).ok_or_else(|| format!("{name}.body must name a body"))?;
                let edges = o.get("edges").and_then(Value::as_array).ok_or_else(|| format!("{name}.edges must list points on the edges"))?;
                if edges.is_empty() || edges.len() > MAX_EDGE_PICKS {
                    return Err(format!("{name}.edges must list 1 to {MAX_EDGE_PICKS} points on a chain of edges"));
                }
                if !edges.iter().all(|e| params::vec3(e).is_some()) {
                    return Err(format!("{name}.edges: a point must be [x, y, z]"));
                }
                on_body = true;
            } else {
                curves = params::string_list(CMD, item, "curves").map_err(message)?.len();
            }
            if let Some(g) = o.get("continuity").filter(|v| !v.is_null()) {
                let g = params::continuity(CMD, Some(g), &format!("{name}.continuity"), solidcraft_kernel::Construction::Network).map_err(message)?;
                if g != Continuity::G0 && !on_body {
                    return Err(format!("{name}: g1 and g2 apply to a curve on a body's edges (a surface's open edges)"));
                }
            }
        } else if !item.is_string() {
            return Err(format!("{name} must be a curve id or {{sketch?, curves}}, {{body, edges}} or {{point}}"));
        }
        if !point && !on_body && (curves == 0 || curves > MAX_CHAIN_CURVES) {
            return Err(format!("{name} must list 1 to {MAX_CHAIN_CURVES} sketch curves"));
        }
    }
    Ok(())
}

/// The network's curves of one direction, checked by [`network`].
macro_rules! network_direction {
    ($(#[$doc:meta])* $name:ident, $key:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub Value);

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let v = Value::deserialize(d)?;
                network($key, v.clone()).map_err(|m| D::Error::custom(whole(m)))?;
                Ok($name(v))
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn inline_schema() -> bool {
                true
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                network_schema(generator)
            }
        }
    };
}

network_direction!(
    /// A network's curves one way: two or more, which the surface lofts through.
    UCurves,
    "u"
);
network_direction!(
    /// A network's curves the other way, each crossing every `u` curve once: two each way give a
    /// Coons patch, more a Gordon surface through every curve. A curve that misses another by
    /// more than 0.01 mm is refused, naming the pair.
    VCurves,
    "v"
);

fn network_schema(generator: &mut SchemaGenerator) -> Schema {
    let point = generator.subschema_for::<Point3>();
    let continuity = json!({"description": "g0 (the default), or on a body's edges g1 (tangent to the face beside a surface's open edge) or g2 (its curvature too).", "type": "string", "pattern": "^[gG][0-2]$"});
    json_schema!({
        "type": "array",
        "maxItems": MAX_NETWORK_CURVES,
        "items": {
            "anyOf": [
                {"description": "A curve of `sketch`, by id.", "type": "string"},
                {"description": "Sketch curves meeting end to end (planar or 3D).", "type": "object",
                 "properties": {"sketch": {"type": ["number", "string"]}, "curves": {"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_CHAIN_CURVES}]}, "continuity": continuity.clone()},
                 "required": ["curves"]},
                {"description": "Points on a chain of a body's edges.", "type": "object",
                 "properties": {"body": {"type": "string"}, "edges": {"type": "array", "items": point.clone(), "minItems": 1, "maxItems": MAX_EDGE_PICKS}, "continuity": continuity},
                 "required": ["body", "edges"]},
                {"description": "A side collapsed to a point: a three-sided patch.", "type": "object", "properties": {"point": point}, "required": ["point"]}
            ]
        }
    })
}

#[cfg(test)]
#[path = "surface_boundary_tests.rs"]
mod tests;
