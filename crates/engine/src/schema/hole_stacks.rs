//! Contract and reply types for `inspect.hole_stacks` in `cmd/hole_stacks.rs` (#2070).

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::StringList;
use crate::params;

/// Largest gap between the layers of one stack (mm).
pub const MAX_TOUCHING: f64 = 10.0;
/// Largest offset between the axes of one line of holes (mm).
pub const MAX_AXIS_TOLERANCE: f64 = 10.0;
/// Most diameter ranges one call takes.
pub const MAX_RANGES: usize = 64;

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
                    .ok_or_else(|| D::Error::custom(format!("`{}` must be a number from 0 to {}", $key, $max))),
            }
        }

        fn $schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({"type": ["number", "null"], "minimum": 0, "maximum": f64::min($max, 1e9), "exclusiveMaximum": 1e9})
        }
    };
}

up_to!(touching, touching_schema, "touching", MAX_TOUCHING);
up_to!(axis_tolerance, axis_tolerance_schema, "axis_tolerance", MAX_AXIS_TOLERANCE);

/// `diameters`: at most [`MAX_RANGES`] `[min, max]` mm ranges with min ≤ max, `null` as left
/// out.
fn diameters<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<(f64, f64)>>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(ranges) if ranges.len() <= MAX_RANGES => {
            let mut out = Vec::new();
            for r in &ranges {
                let pair = r.as_array().map(|a| (a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)));
                match pair {
                    Some((Some(lo), Some(hi))) if lo.is_finite() && hi.is_finite() && lo <= hi => out.push((lo, hi)),
                    _ => return Err(D::Error::custom("`diameters` must be [[min, max], …] in mm, min ≤ max")),
                }
            }
            Ok(Some(out))
        }
        _ => Err(D::Error::custom(format!("`diameters` must be a list of at most {MAX_RANGES} [min, max] ranges"))),
    }
}

fn diameters_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": ["array", "null"],
        "items": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2},
        "maxItems": MAX_RANGES,
    })
}

/// `inspect.hole_stacks`: find the holes that line up through two or more touching bodies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct HoleStacksParams {
    /// The bodies to look at (default: every body but already placed standard parts).
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// Diameter ranges in mm, `[[min, max], …]` with min ≤ max: only holes and slot widths
    /// inside a range count (default: every size).
    #[serde(default, deserialize_with = "diameters")]
    #[schemars(schema_with = "diameters_schema")]
    pub diameters: Option<Vec<(f64, f64)>>,
    /// Whether obround slots count (default true).
    #[serde(default)]
    pub slots: Option<bool>,
    /// The largest gap between the layers of one stack, in mm from 0 to 10 (default 0.05).
    #[serde(default, deserialize_with = "touching")]
    #[schemars(schema_with = "touching_schema")]
    pub touching: Option<f64>,
    /// The largest offset between the axes of one line of holes, in mm from 0 to 10
    /// (default 0.1).
    #[serde(default, deserialize_with = "axis_tolerance")]
    #[schemars(schema_with = "axis_tolerance_schema")]
    pub axis_tolerance: Option<f64>,
}

/// `inspect.hole_stacks`'s reply: the stacks, and the holes in no stack.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HoleStacks {
    /// Stacks of holes through two or more touching bodies, ordered by their start points.
    pub stacks: Vec<Stack>,
    /// Holes in no stack.
    pub lone: Vec<LoneHole>,
}

/// One stack: holes that line up through two or more touching bodies.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Stack {
    /// Unit axis, from the start into the stack (−z for a vertical stack).
    pub axis: [f64; 3],
    /// The point on the axis the first layer starts at.
    pub start: [f64; 3],
    /// The point on the axis the last layer ends at.
    pub end: [f64; 3],
    /// The clamped thickness, in mm.
    pub grip_mm: f64,
    /// The stack's layers, from its start along `axis`.
    pub layers: Vec<Layer>,
}

/// One layer of a stack: the bores of one body that run together (a counterbore and its
/// through hole are one layer, of their narrowest opening).
#[derive(Debug, Serialize, JsonSchema)]
pub struct Layer {
    /// The body's name.
    pub body: String,
    /// The layer's narrowest diameter, in mm.
    pub d_mm: f64,
    /// The extent from the stack's start, in mm.
    pub t: [f64; 2],
    /// The narrowest bore's slot shape.
    pub slot: Option<Slot>,
}

/// An obround slot's shape across the axis.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Slot {
    /// The overall length, end to end, in mm.
    pub length_mm: f64,
    /// The unit direction of the length.
    pub along: [f64; 3],
}

/// A hole in no stack.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LoneHole {
    /// The body's name.
    pub body: String,
    /// The hole's diameter, in mm.
    pub d_mm: f64,
    /// A point on the hole's axis.
    pub at: [f64; 3],
    /// The unit axis, either way round.
    pub axis: [f64; 3],
    /// The wall's extent along the axis, in mm.
    pub depth_mm: f64,
    /// The slot's shape.
    pub slot: Option<Slot>,
}
