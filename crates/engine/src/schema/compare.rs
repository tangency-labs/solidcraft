//! Contract and result types for `inspect.compare` in `cmd/compare.rs` (#10994).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::check::whole;

/// Compare two bodies laid over each other, in world placement: measure both, sample each
/// surface's distance to the other, and find, measure and (with `color`) colour the places where
/// only one of them has material. It works from the two surfaces, without a boolean of the
/// bodies, so near-identical bodies compare too.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CompareParams {
    /// The first body, by name. Give both `a` and `b`, or neither: then the two selected bodies,
    /// or the design's only two.
    #[serde(default, deserialize_with = "body_name")]
    pub a: Option<String>,
    /// The second body, by name.
    #[serde(default, deserialize_with = "body_name")]
    pub b: Option<String>,
    /// Surfaces closer than this agree, in mm: 0.001 to 10 (default 0.01).
    #[serde(default, deserialize_with = "tolerance")]
    #[schemars(schema_with = "tolerance_schema")]
    pub tolerance: Option<f64>,
    /// The largest piece of a face sampled as one point, in mm: 0.01 to 1000 (default 0.5),
    /// coarser for a body that would need more than `max_points` samples.
    #[serde(default, deserialize_with = "spacing")]
    #[schemars(schema_with = "spacing_schema")]
    pub spacing: Option<f64>,
    /// The most samples per body: 10 000 to 50 million (default 8 million).
    #[serde(default, deserialize_with = "max_points")]
    #[schemars(schema_with = "max_points_schema")]
    pub max_points: Option<f64>,
    /// The most regions measured: 1 to 1000 (default 50); more are counted in `unlisted_regions`.
    #[serde(default, deserialize_with = "max_regions")]
    #[schemars(schema_with = "max_regions_schema")]
    pub max_regions: Option<f64>,
    #[serde(default)]
    pub color: Option<CompareColor>,
}

/// A body name, with the handler's message for anything else; `null` is none.
fn body_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        _ => Err(D::Error::custom("must be a body name")),
    }
}

/// A number in `lo..=hi` with the handler's message, `null` none, for each ranged key.
macro_rules! ranged {
    ($read:ident, $schema:ident, $lo:literal, $hi:literal) => {
        fn $read<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
            match Value::deserialize(d)? {
                Value::Null => Ok(None),
                v => v
                    .as_f64()
                    .filter(|x| x.is_finite() && ($lo..=$hi).contains(x))
                    .map(Some)
                    .ok_or_else(|| D::Error::custom(format!("must be a number from {} to {}", $lo, $hi))),
            }
        }

        fn $schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({"type": ["number", "null"], "minimum": $lo, "maximum": $hi})
        }
    };
}

ranged!(tolerance, tolerance_schema, 0.001, 10.0);
ranged!(spacing, spacing_schema, 0.01, 1000.0);
ranged!(max_points, max_points_schema, 1e4, 5e7);
ranged!(max_regions, max_regions_schema, 1.0, 1000.0);

/// Colour the faces that hold a region: `true` for red where only `a` has material and green
/// where only `b` has, or `{only_in_a?, only_in_b?}` to choose either colour (`"#rrggbb"` or
/// `[r, g, b]`, 0 to 255). `false` colours nothing. One undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct CompareColor(pub Value);

impl<'de> Deserialize<'de> for CompareColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        match &v {
            Value::Bool(_) => {}
            Value::Object(o) => {
                if o.keys().any(|k| k != "only_in_a" && k != "only_in_b") {
                    return Err(D::Error::custom("must be true or {only_in_a, only_in_b}"));
                }
                for k in ["only_in_a", "only_in_b"] {
                    if o.get(k).is_some_and(|c| !c.is_null() && crate::cmd::color_param(c).is_none()) {
                        return Err(D::Error::custom(whole(format!("`color.{k}` must be \"#rrggbb\" or [r, g, b] with values 0–255"))));
                    }
                }
            }
            _ => return Err(D::Error::custom("must be true or {only_in_a, only_in_b}")),
        }
        Ok(CompareColor(v))
    }
}

impl JsonSchema for CompareColor {
    fn schema_name() -> Cow<'static, str> {
        "CompareColor".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let color = || {
            serde_json::json!({
                "anyOf": [
                    {"description": "\"#rrggbb\": six hex digits, the # optional.", "type": "string"},
                    {"type": "array", "items": {"type": "number", "minimum": 0, "maximum": 255}, "minItems": 3, "maxItems": 3},
                    {"type": "null"}
                ]
            })
        };
        json_schema!({
            "description": "Colour the faces that hold a region: true for red where only `a` has material and green where only `b` has, or {only_in_a?, only_in_b?} to choose either colour (\"#rrggbb\" or [r, g, b], 0 to 255). false colours nothing.",
            "anyOf": [
                {"type": "boolean"},
                {"type": "object", "properties": {"only_in_a": color(), "only_in_b": color()}, "additionalProperties": false}
            ]
        })
    }
}

/// What `inspect.compare` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Comparison {
    /// `a`'s measures, as `inspect.measure` gives a body's.
    pub a: Map<String, Value>,
    /// `b`'s measures, as `inspect.measure` gives a body's.
    pub b: Map<String, Value>,
    /// `b` less `a`.
    pub difference: Difference,
    /// The tolerance used (mm).
    pub tolerance_mm: f64,
    /// How far each surface is from the other.
    pub deviation: Deviations,
    /// Whether no region differs by more than the tolerance.
    pub identical: bool,
    /// The comparison in a sentence.
    pub verdict: String,
    /// The regions where only one body has material, largest first.
    pub regions: Vec<Region>,
    /// The regions with material only in `a`, and their volume together.
    pub only_in_a: SideTotal,
    /// The regions with material only in `b`, and their volume together.
    pub only_in_b: SideTotal,
    /// Regions beyond `max_regions`, counted but not measured.
    pub unlisted_regions: usize,
    /// What makes the comparison less certain: a body whose surface doesn't enclose what it
    /// measures, a cut that couldn't be made, a body that encloses no volume.
    pub warnings: Vec<String>,
    /// How the surfaces were sampled.
    pub sampling: Sampling,
    /// With `color`: how many faces were coloured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colored_faces: Option<usize>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `b` less `a`; `null` where either has no such measure.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Difference {
    pub volume_mm3: Option<f64>,
    /// The volume difference as a share of `a`'s (%).
    pub volume_percent: Option<f64>,
    pub area_mm2: Option<f64>,
    pub center_of_mass: Option<[f64; 3]>,
    pub center_of_mass_distance_mm: Option<f64>,
    pub bbox_min: Option<[f64; 3]>,
    pub bbox_max: Option<[f64; 3]>,
}

/// How far each surface is from the other.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Deviations {
    /// From `a`'s surface to `b`'s.
    pub a_to_b: Deviation,
    /// From `b`'s surface to `a`'s.
    pub b_to_a: Deviation,
    /// The larger of the two maxima (mm).
    pub max_mm: f64,
}

/// One surface's distance to the other, over its samples.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Deviation {
    pub max_mm: f64,
    pub mean_mm: f64,
    /// Where the largest is.
    pub at: [f64; 3],
    /// The area further than the tolerance (mm²).
    pub area_beyond_mm2: f64,
    /// The area sampled (mm²).
    pub area_mm2: f64,
    /// How many samples.
    pub points: usize,
}

/// Which body alone has a region's material.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareSide {
    OnlyInA,
    OnlyInB,
}

/// A region where only one body has material.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Region {
    /// Its number, from 1, largest first.
    pub index: usize,
    pub side: CompareSide,
    /// The body that has the material.
    pub material_of: String,
    /// Its volume (mm³), measured exactly on the pieces of the cut on its side (a − b or b − a)
    /// that lie there; `null` when it isn't measured, never an estimate.
    pub volume_mm3: Option<f64>,
    /// `boolean` when the volume, centre and box were measured on the cut, else `not measured`.
    pub measured_by: MeasuredBy,
    /// Why it isn't measured (the cut couldn't be made, a body encloses no volume, no piece fell
    /// here); `null` when it is.
    pub not_measured: Option<String>,
    /// Its surface on both bodies (mm²).
    pub area_mm2: f64,
    pub area_a_mm2: f64,
    pub area_b_mm2: f64,
    /// How far the surfaces are apart there at most (mm).
    pub max_deviation_mm: f64,
    /// Where that is.
    pub at: [f64; 3],
    /// Its centre of volume, measured with the volume; `null` when it isn't measured.
    pub center: Option<[f64; 3]>,
    /// The measured pieces' box, or the box the region was found in when it isn't measured.
    pub bbox: Bounds,
    /// The faces of `a` it lies on, by index, as `model.faces` lists them.
    pub faces_a: Vec<u32>,
    /// The faces of `b` it lies on.
    pub faces_b: Vec<u32>,
    /// The lines through it the surfaces were sampled along.
    pub resolution: Resolution,
}

/// How a region was measured.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
pub enum MeasuredBy {
    /// Exactly, on the pieces of the cut that lie there.
    #[serde(rename = "boolean")]
    Boolean,
    /// Not at all: see `not_measured`.
    #[serde(rename = "not measured")]
    NotMeasured,
}

/// A box, by its corners.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// The lines through a region the surfaces were sampled along.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Resolution {
    /// The axis they run along: `X`, `Y` or `Z`.
    pub along: String,
    /// Their spacing across the other two axes (mm).
    pub line_spacing_mm: [f64; 2],
    pub lines: usize,
    /// Lines that crossed a gap in a surface and were left out.
    pub skipped_lines: usize,
}

/// The regions on one side, and their volume together.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SideTotal {
    pub regions: usize,
    /// Their volume together (mm³); `null` when one of them isn't measured.
    pub volume_mm3: Option<f64>,
}

/// How the surfaces were sampled.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Sampling {
    /// The sample spacing used (mm).
    pub spacing_mm: f64,
    /// How closely the meshes follow the surfaces (mm): a quarter of the tolerance.
    pub chord_mm: f64,
    pub triangles_a: usize,
    pub triangles_b: usize,
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;
