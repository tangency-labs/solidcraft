//! Contract and result types for `pointcloud.deviation` in `cmd/point_cloud_deviation.rs`.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::IdOrName;

/// Measure how far each point of a point cloud lies from a body, on the body's exact faces as
/// they are trimmed (their edges and vertices included), never on a mesh. The cloud and the body
/// are measured where they are placed now; nothing in the design changes. Distances are sampled
/// at the points only: they say nothing about parts of the body that no point lies near.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PointCloudDeviationParams {
    /// The point cloud, by id or name.
    pub cloud: IdOrName,
    /// The body to measure against, by name. A mesh body is refused: its triangles are not
    /// exact faces.
    pub body: String,
    /// Measure against only these faces of the body, by index as `model.faces` lists them.
    /// Default: every face. Distances to a face set are always unsigned.
    #[serde(default, deserialize_with = "faces")]
    #[schemars(schema_with = "faces_schema")]
    pub faces: Option<Vec<u64>>,
    /// A point within this distance of the body counts as within tolerance, in mm: 0.0001 to
    /// 1000 (default 0.1).
    #[serde(default, deserialize_with = "tolerance")]
    #[schemars(schema_with = "tolerance_schema")]
    pub tolerance: Option<f64>,
    /// The most points measured: 1 to 2000000 (default 100000). A cloud with more points is
    /// sampled at every n-th point, with n the smallest whole number that keeps the count within
    /// this limit, starting at the first point.
    #[serde(default, deserialize_with = "max_samples")]
    #[schemars(schema_with = "max_samples_schema")]
    pub max_samples: Option<f64>,
    /// `true` gives each distance a sign: positive outside the body's material, negative inside
    /// it, zero on its boundary. Only a body that encloses a volume, measured on all its faces,
    /// has an inside; `true` is refused for any other. Default: `true` when the body encloses a
    /// volume and `faces` is not given, else `false`.
    #[serde(default)]
    pub signed: Option<bool>,
    /// How many of the farthest points to list in `worst`: 0 to 1000 (default 10).
    #[serde(default, deserialize_with = "worst")]
    #[schemars(schema_with = "worst_schema")]
    pub worst: Option<f64>,
    /// `true` adds `distances`: one entry per measured point, in the order the points were
    /// measured. Default: false.
    #[serde(default)]
    pub per_point: Option<bool>,
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

ranged!(tolerance, tolerance_schema, 0.0001, 1000.0);
ranged!(max_samples, max_samples_schema, 1.0, 2000000.0);
ranged!(worst, worst_schema, 0.0, 1000.0);

/// The most face indices `faces` takes.
pub const MAX_FACES: usize = 100_000;

/// A list of face indices, with the handler's message for anything else; `null` is none.
fn faces<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u64>>, D::Error> {
    let fail = || D::Error::custom(format!("must be a list of face indices (whole numbers from 0), at most {MAX_FACES}"));
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Array(a) if a.len() <= MAX_FACES => a.iter().map(Value::as_u64).collect::<Option<Vec<u64>>>().map(Some).ok_or_else(fail),
        _ => Err(fail()),
    }
}

fn faces_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["array", "null"], "items": {"type": "integer", "minimum": 0}, "maxItems": MAX_FACES})
}

/// One measured point.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct DeviationPoint {
    /// The point's index in the cloud.
    pub index: usize,
    /// The point in the design (mm).
    pub point: [f64; 3],
    /// The closest point on the body's faces (mm).
    pub closest: [f64; 3],
    /// The distance from `point` to `closest` (mm), with the sign of `sign_convention` when the
    /// result is signed.
    pub distance_mm: f64,
}

/// A point that could not be measured.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct DeviationFailure {
    /// The point's index in the cloud.
    pub index: usize,
    /// Why the point has no distance.
    pub reason: String,
}

/// What `pointcloud.deviation` returns. The statistics cover the measured points only, each
/// point counted once (not weighted by area). Each is `null` when no point was measured, never 0.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PointCloudDeviationResult {
    /// The cloud, by id.
    pub cloud: u64,
    /// The body, by name.
    pub body: String,
    /// The faces measured against, or `null` for every face.
    pub faces: Option<Vec<u64>>,
    /// Whether the distances carry a sign.
    pub signed: bool,
    /// What the sign means, or `null` for unsigned distances.
    pub sign_convention: Option<String>,
    /// The tolerance used (mm).
    pub tolerance_mm: f64,
    /// How many points the cloud has.
    pub cloud_points: usize,
    /// Every n-th point was taken: 1 when every point was.
    pub sample_step: usize,
    /// How many points were taken to measure.
    pub sampled: usize,
    /// How many of them were measured.
    pub measured: usize,
    /// How many of them could not be measured: `sampled` = `measured` + `failed`.
    pub failed: usize,
    /// The mean of the distances without their sign (mm).
    pub mean_abs_mm: Option<f64>,
    /// The mean of the signed distances (mm), or `null` when unsigned.
    pub mean_signed_mm: Option<f64>,
    /// The root mean square of the distances (mm).
    pub rms_mm: Option<f64>,
    /// The largest distance without its sign among the measured points (mm). It is a sampled
    /// maximum: a point not in the cloud, or not sampled, may lie farther.
    pub max_abs_mm: Option<f64>,
    /// How many measured points lie within `tolerance_mm` of the body.
    pub within_tolerance: usize,
    /// `within_tolerance` / `measured`.
    pub within_tolerance_fraction: Option<f64>,
    /// With a signed result: how many measured points lie outside the material, inside it and on
    /// its boundary. `null` when unsigned.
    pub outside: Option<usize>,
    pub inside: Option<usize>,
    pub on_boundary: Option<usize>,
    /// The farthest measured points, farthest first.
    pub worst: Vec<DeviationPoint>,
    /// The first points that could not be measured (at most 100), with the reason.
    pub failures: Vec<DeviationFailure>,
    /// With `per_point`: one distance per sampled point, in order (mm, signed as `signed` says),
    /// `null` for a point that could not be measured. Otherwise absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distances: Option<Vec<Option<f64>>>,
    /// The body's own tolerance (mm): the kernel solves each distance on the exact geometry,
    /// and a point within this distance of a face counts as on it.
    pub body_tolerance_mm: f64,
    /// How exactly the cloud stores its points (mm): its coordinates are 32-bit floats, so a
    /// point may lie this far from the value in its file.
    pub point_precision_mm: f64,
    /// What makes the result less certain or less complete.
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
