//! Contracts and result types for the commands in `cmd/parts.rs`: standard parts (#11003).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcore_record::Vec3;

use super::check::whole;
use super::types::Point3;
use crate::params;

/// List the standard part families, or one family's sizes with their dimensions (mm).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PartsLibraryParams {
    /// One family, as the list names it (default: every family).
    #[serde(default)]
    pub family: Option<String>,
}

/// Insert a standard part as a new component: seated in holes with a rigid joint each (`at`),
/// placed at a point (`point`), or at the origin. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PartsInsertParams {
    /// The family, as `parts.library` lists it: socket_head_cap_screw, button_head_cap_screw,
    /// countersunk_socket_screw, hex_bolt, hex_nut, washer, set_screw, dowel_pin, bearing, hex_cap_screw_inch,
    /// hex_nut_inch, washer_sae or washer_uss.
    #[serde(deserialize_with = "family")]
    pub family: String,
    /// The size, as `parts.library` lists it: `M6`, `608` for a bearing, `1/2-13 UNC` for an
    /// inch screw or nut, `1/2` for an inch washer.
    #[serde(deserialize_with = "size")]
    pub size: String,
    #[serde(default, deserialize_with = "length")]
    #[schemars(schema_with = "length_schema")]
    pub length: Option<f64>,
    #[serde(default)]
    pub at: Option<HolePicks>,
    /// Without `at`: where the part's origin goes, with no joint.
    #[serde(default)]
    pub point: Option<Point3>,
    /// With `point`: the direction the part's +Z turns to (default +Z), a non-zero `[x, y, z]`.
    #[serde(default)]
    pub direction: Option<Point3>,
    /// The component's name (default: the part's, such as `Socket Head Cap Screw M6x20`).
    #[serde(default)]
    pub name: Option<String>,
    /// Turn the part over from the way it is placed: with `at`, head into the material and shank
    /// out of the face; with `point`, its +Z against `direction` (default false).
    #[serde(default)]
    pub flip: Option<bool>,
}

/// Change an inserted standard part's size or length, rebuilding it in place; its joints keep it
/// seated. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PartsResizeParams {
    /// The part's component, by id or name.
    #[serde(deserialize_with = "component")]
    #[schemars(schema_with = "component_schema")]
    pub component: String,
    /// The new size (default: the part's).
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default, deserialize_with = "length")]
    #[schemars(schema_with = "length_schema")]
    pub length: Option<f64>,
}

/// The family, with the handler's message for anything that isn't text.
fn family<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`family` is required (parts.library)"))),
    }
}

/// The size, with the handler's message for anything that isn't text.
fn size<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom(whole("`size` is required"))),
    }
}

/// A part's length, as the handler reads it: `null` is the default, else above 0 and at most
/// 2000 mm.
fn length<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        v => v
            .as_f64()
            .and_then(params::finite)
            .filter(|l| *l > 0.0 && *l <= 2000.0)
            .map(Some)
            .ok_or_else(|| D::Error::custom(whole("`length` must be 0…2000 mm"))),
    }
}

fn length_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "For screws, bolts, set screws and pins: the length in mm, inch parts too (default: a standard length near 2.5 diameters).",
        "type": ["number", "null"],
        "exclusiveMinimum": 0,
        "maximum": 2000
    })
}

/// A component's id or name, as the resize reads it, with its message.
fn component<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s),
        _ => Err(D::Error::custom("must be an id or a name")),
    }
}

fn component_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "The part's component, by id or name.", "type": ["number", "string"]})
}

/// The most holes one insert seats a part in.
const MAX_HOLES: usize = 1000;

/// Points on the rims or walls of holes, as the insert reads them: one point, or a list of up to
/// 1000 (a list's first item is a list, or it is empty), with its messages. The part seats on
/// each hole's face with a rigid joint, placed once per hole as one component.
#[derive(Clone, Debug, PartialEq)]
pub struct HolePicks(pub Vec<Vec3>);

impl<'de> Deserialize<'de> for HolePicks {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom(whole("`at` must be [x,y,z] or a list of them"));
        let one = |v: &Value| params::vec3(v).ok_or_else(fail);
        match Value::deserialize(d)? {
            Value::Array(list) if list.first().is_some_and(Value::is_array) || list.is_empty() => {
                if list.len() > MAX_HOLES {
                    return Err(D::Error::custom(whole(format!("`at` takes at most {MAX_HOLES} holes"))));
                }
                list.iter().map(one).collect::<Result<_, _>>().map(HolePicks)
            }
            v => Ok(HolePicks(vec![one(&v)?])),
        }
    }
}

impl JsonSchema for HolePicks {
    fn schema_name() -> Cow<'static, str> {
        "HolePicks".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        json_schema!({
            "description": "A point on a hole's rim or wall, or a list of them (one per hole, at most 1000): the part seats on each hole's face with a rigid joint.",
            "anyOf": [point.clone(), {"type": "array", "prefixItems": [{"allOf": [{"type": "array"}, point.clone()]}], "items": point, "maxItems": MAX_HOLES}]
        })
    }
}

/// What `parts.library` returns: every family, or the one asked for.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PartsLibrary {
    /// Without `family`: every family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub families: Option<Vec<PartFamily>>,
    /// With `family`: that family.
    #[serde(flatten)]
    pub family: Option<PartFamily>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A standard part family.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PartFamily {
    /// Its id, as `parts.insert` takes it.
    pub family: String,
    pub name: String,
    /// The standard its sizes follow.
    pub standard: String,
    /// `metric` or `inch`.
    pub system: String,
    /// Each size with its dimensions in mm (which depend on the family), and `default_length`
    /// where the part has a length.
    #[schemars(schema_with = "sizes_schema")]
    pub sizes: Vec<Value>,
    /// The standard lengths (mm), for the families that have one; else `null`.
    pub lengths: Option<Vec<f64>>,
}

fn sizes_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "object", "properties": {"size": {"type": "string"}}, "required": ["size"]}})
}

/// What `parts.insert` and `parts.fastener` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InsertedPart {
    /// The new component's id.
    pub component: u64,
    /// Its first occurrence's id.
    pub occurrence: u64,
    /// The component's name.
    pub name: String,
    /// With `at`: the first hole's joint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joint: Option<u64>,
    /// With `at`: an occurrence per hole, in pick order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrences: Option<Vec<u64>>,
    /// With `at`: a joint per hole, in pick order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joints: Option<Vec<u64>>,
    /// The way each placement's head points (its +Z, from the shank's tip toward the head), in
    /// world coordinates and in pick order: the direction that was chosen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directions: Vec<[f64; 3]>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `parts.resize` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResizedPart {
    pub component: u64,
    /// The component's new name.
    pub name: String,
    pub size: String,
    /// Its length (mm), for a part that has one; else `null`.
    pub length: Option<f64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "parts_tests.rs"]
mod tests;
