//! Contracts and result types for the commands in `cmd/frame.rs` that have them.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use solidcraft_doc::frame::{ProfileSpec, profile_kinds};

use super::check::{keyed, whole};
use crate::cmd::{DENSITY_MESSAGE, density_form};

/// A built-in profile: its kind and its dimensions, as `frame.profile.properties` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileParam(pub ProfileSpec);

impl<'de> Deserialize<'de> for ProfileParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde_json::from_value(Value::deserialize(d)?).map(ProfileParam).map_err(|e| D::Error::custom(keyed(e)))
    }
}

impl JsonSchema for ProfileParam {
    fn schema_name() -> Cow<'static, str> {
        "Profile".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let kinds: Vec<&str> = profile_kinds().iter().map(|k| k.kind).collect();
        json_schema!({
            "description": "A built-in profile: `kind` and its dimensions, each a number in the design's units or a length expression over its parameters (\"tube_wall\", \"2 in\"). Each kind takes only its own dimensions; `frame.profile.list` gives them, with an example. Dimensions left out take defaults from the profile's size.",
            "type": "object",
            "properties": {"kind": {"type": "string", "enum": kinds}},
            "required": ["kind"],
            "additionalProperties": {"type": ["number", "string"]}
        })
    }
}

/// A density in kg/m³, as `frame.profile.properties` reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density(pub f64);

impl<'de> Deserialize<'de> for Density {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        density_form(&Value::deserialize(d)?).map(Density).ok_or_else(|| D::Error::custom(whole(DENSITY_MESSAGE)))
    }
}

impl JsonSchema for Density {
    fn schema_name() -> Cow<'static, str> {
        "Density".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A density in kg/m³.", "type": "number", "exclusiveMinimum": 0, "maximum": 1e6})
    }
}

/// `frame.profile.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProfileListParams {}

/// What `frame.profile.list` returns: the built-in profiles, each SolidCraft's own parametric
/// definition.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileListResult {
    pub kinds: Vec<ProfileKindReply>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One built-in profile kind.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileKindReply {
    /// The kind, as a profile's `kind` takes it.
    pub kind: String,
    /// Its name for people.
    pub label: String,
    /// The dimensions it needs.
    pub required: Vec<String>,
    /// The dimensions that take a default when left out.
    pub optional: Vec<String>,
    /// A complete profile of this kind, in mm.
    #[schemars(with = "serde_json::Map<String, Value>")]
    pub example: Value,
}

/// Work out a built-in profile's section: its area, centroid, second moments of area and size,
/// and with a density its mass per metre. Changes nothing in the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProfilePropertiesParams {
    pub profile: ProfileParam,
    /// The material's density, in kg/m³: adds `mass_per_m`. Default: no mass.
    #[serde(default)]
    pub density: Option<Density>,
}

/// What `frame.profile.properties` returns: the profile's section.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileSection {
    /// The profile with every default filled in, in mm.
    #[schemars(with = "serde_json::Map<String, Value>")]
    pub profile: Value,
    /// The section's area, in mm².
    pub area: f64,
    /// The centroid `[x, y]`, in mm from the centre of the profile's bounding box.
    pub centroid: [f64; 2],
    /// ∫ y² dA about the centroid, in mm⁴: bending about the profile's x axis.
    pub ixx: f64,
    /// ∫ x² dA about the centroid, in mm⁴: bending about the profile's y axis.
    pub iyy: f64,
    /// ∫ x y dA about the centroid, in mm⁴.
    pub ixy: f64,
    /// The bounding box's width, in mm.
    pub width: f64,
    /// The bounding box's height, in mm.
    pub height: f64,
    /// With `density`: the mass of one metre of the profile, in kg/m.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mass_per_m: Option<f64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
