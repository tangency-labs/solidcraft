//! Contract and result types for `inspect.clearance` in `cmd/clearance.rs` (#11051).

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::check::whole;
use super::types::StringList;
use crate::params;

/// Most neighbours a body's `nearest` list holds.
pub const MAX_NEAREST: usize = 1000;
/// Largest clearance (mm).
pub const MAX_CLEARANCE: f64 = 1e6;

/// Find the solid bodies that overlap, and those closer than `clearance`, in world placement.
/// Distances are measured on the exact faces and edges; the meshes only rule pairs out. Mesh
/// bodies aren't measured: each is listed in `unchecked`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ClearanceParams {
    /// The bodies to check (default: every solid body but those in `against`).
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// Only pairs with one body in `bodies` and the other here.
    #[serde(default)]
    pub against: Option<StringList>,
    /// Smaller overlaps count as contact, in mm³ (default 0.01).
    #[serde(default, deserialize_with = "min_volume")]
    #[schemars(schema_with = "min_volume_schema")]
    pub min_volume: Option<f64>,
    /// Report pairs closer than this, in mm: 0 to 1 000 000 (default 0).
    #[serde(default, deserialize_with = "clearance")]
    #[schemars(schema_with = "clearance_schema")]
    pub clearance: Option<f64>,
    /// A gap below this is contact, 0, in mm: 0 to 1 (default 1e-6).
    #[serde(default, deserialize_with = "tolerance")]
    #[schemars(schema_with = "tolerance_schema")]
    pub tolerance: Option<f64>,
    /// Each body of `bodies` gets its this many nearest others, 0 to 1000 (default 0); a
    /// fraction is rounded down.
    #[serde(default, deserialize_with = "nearest")]
    #[schemars(schema_with = "nearest_schema")]
    pub nearest: Option<f64>,
    #[serde(default)]
    pub skip_within: Option<SkipWithin>,
}

/// A number from 0 to `max`, read as the handler reads it (`params::num`: finite, under 1e9 in
/// size), with its message; `null` is the default.
macro_rules! up_to {
    ($read:ident, $schema:ident, $key:literal, $max:expr) => {
        fn $read<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
            match Value::deserialize(d)? {
                Value::Null => Ok(None),
                v => v
                    .as_f64()
                    .and_then(params::finite)
                    .filter(|x| (0.0..=$max).contains(x))
                    .map(Some)
                    .ok_or_else(|| D::Error::custom(whole(format!("`{}` must be a number from 0 to {}", $key, $max)))),
            }
        }

        fn $schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({"type": ["number", "null"], "minimum": 0, "maximum": f64::min($max, 1e9), "exclusiveMaximum": 1e9})
        }
    };
}

up_to!(min_volume, min_volume_schema, "min_volume", f64::MAX);
up_to!(clearance, clearance_schema, "clearance", MAX_CLEARANCE);
up_to!(tolerance, tolerance_schema, "tolerance", 1.0);
up_to!(nearest, nearest_schema, "nearest", MAX_NEAREST as f64);

/// Pairs not to check: of bodies of one occurrence (`occurrence`), or inside one placed
/// component, its subcomponents included (`component`). `null` checks every pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipWithin {
    Occurrence,
    Component,
}

impl<'de> Deserialize<'de> for SkipWithin {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)?.as_str() {
            Some("occurrence") => Ok(SkipWithin::Occurrence),
            Some("component") => Ok(SkipWithin::Component),
            _ => Err(D::Error::custom(whole("`skip_within` must be \"occurrence\", \"component\" or null"))),
        }
    }
}

impl JsonSchema for SkipWithin {
    fn schema_name() -> Cow<'static, str> {
        "SkipWithin".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Pairs not to check: of bodies of one occurrence (`occurrence`), or inside one placed component, its subcomponents included (`component`). null checks every pair.",
            "enum": ["occurrence", "component", null]
        })
    }
}

/// What `inspect.clearance` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Clearance {
    /// Pairs that share at least `min_volume`.
    pub overlaps: Vec<Overlap>,
    /// Pairs closer than `clearance`, nearest first; a pair in contact is at 0.
    pub near: Vec<NearPair>,
    /// With `nearest`: each body of `bodies`, with its nearest others.
    pub nearest: BTreeMap<String, Vec<Neighbour>>,
    /// Pairs whose overlap couldn't be worked out, and each mesh body, which isn't measured
    /// (`b` is then `null`).
    pub unchecked: Vec<Unchecked>,
    /// How many solid bodies were checked.
    pub bodies: usize,
    pub pairs_checked: usize,
    /// Of those, the pairs measured by an earlier call: neither body changed or moved since.
    pub pairs_reused: usize,
    /// Pairs ruled out by their boxes.
    pub pairs_pruned: usize,
    /// Whether there were more pairs than one call measures.
    pub truncated: bool,
    /// How long it took (ms).
    pub ms: f64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Two bodies that overlap.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Overlap {
    pub a: String,
    pub b: String,
    /// The volume they share (mm³).
    pub volume_mm3: f64,
    /// The area of the shared volume's surface (mm²).
    pub area_mm2: f64,
    pub bbox: OverlapBounds,
    pub center: [f64; 3],
}

/// A box, by its corners.
#[derive(Debug, Serialize, JsonSchema)]
pub struct OverlapBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// Two bodies closer than `clearance`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct NearPair {
    pub a: String,
    pub b: String,
    pub distance_mm: f64,
    /// The nearest point on `a`.
    pub from: [f64; 3],
    /// The nearest point on `b`.
    pub to: [f64; 3],
    /// For a contact: the volume they share, below `min_volume` (mm³).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap_mm3: Option<f64>,
    /// `false` when measured on the meshes; absent when measured on the exact faces and edges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
}

/// One of a body's nearest others.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Neighbour {
    pub body: String,
    pub distance_mm: f64,
}

/// A pair, or a mesh body, that wasn't measured.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Unchecked {
    pub a: String,
    /// The other body; `null` for a mesh body.
    pub b: Option<String>,
    pub reason: String,
}

#[cfg(test)]
#[path = "clearance_tests.rs"]
mod tests;
