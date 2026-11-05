//! Contracts and result types for the commands in `cmd/sketch_create.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{AngleExpr, LengthExpr, SketchRef, present};

/// Create a reference line in the active sketch: infinite, or a segment of a given length.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ConstructionLineParams {
    /// The construction method.
    pub mode: Option<ConstructionMethod>,
    /// Point to pass through; for tangent_at it selects a contact on the curve.
    pub at: Value,
    /// Second point of a two-point construction line.
    pub through: Option<Value>,
    /// Point used to select the closest solution when several exist; for a line with a
    /// `length`, the side of `at` it runs towards.
    pub near: Option<[f64; 2]>,
    /// Zero-based index in the deterministic list of solutions.
    pub solution: Option<usize>,
    /// A reference line for parallel and perpendicular constructions.
    pub curve: Option<String>,
    /// Second reference curve for a common tangent or an angle bisector.
    pub curve2: Option<String>,
    /// Choose the other tangent when two solutions exist.
    pub alternate: Option<bool>,
    /// Direction measured counter-clockwise from sketch X, in degrees.
    pub angle: Option<AngleExpr>,
    /// A segment of this length from `at` instead of an infinite line, held by a length
    /// dimension. For the horizontal, vertical, angle, parallel and perpendicular methods.
    pub length: Option<LengthExpr>,
    /// `false` makes a line with a `length` a normal sketch line, ready for a profile, instead
    /// of construction geometry (default `true`). An infinite line is always construction.
    pub construction: Option<bool>,
    /// The sketch (default: the one being edited).
    pub sketch: Option<SketchRef>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstructionMethod {
    Free,
    Horizontal,
    Vertical,
    Angle,
    Parallel,
    Perpendicular,
    TangentExternal,
    TangentAt,
    TangentTwo,
    Bisector,
}

/// The curves and constraints created, together with the sketch solve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConstructionLineResult {
    pub curves: Vec<String>,
    pub constraints: Vec<String>,
    pub sketch: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Reverse a spline's direction: its last point becomes its first, and the curve runs the other
/// way along the same path. Its points, and the constraints on them and on the curve, stay. Takes
/// fit point, control point and closed splines; refused for a projected spline and for one an
/// offset uses or makes.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplineReverseParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// Take one point out of a spline: it runs through (or over) its other points only, and the point
/// goes with the constraints on it unless another curve uses it. Takes fit point, control point
/// and closed splines; refused for a closed spline's closing point, for an end joined to other
/// geometry or held by a constraint, when fewer than 2 points would be left (3 on a closed
/// spline), and for a projected, offset or patterned spline.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SplineRemovePointParams {
    /// The spline's curve id.
    #[serde(deserialize_with = "spline_id")]
    pub curve: String,
    /// One of the spline's points: a point id, or the curve's `start` or `end` (`s1.end`).
    #[serde(deserialize_with = "point_ref")]
    pub point: String,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// What `sketch.spline.remove_point` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineRemovePointResult {
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The id the point had.
    pub point: String,
    /// Whether the point went too; `false` when another curve still uses it.
    pub deleted: bool,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A point reference, with the handler's message for anything else.
fn point_ref<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be one of the spline's points")),
    }
}

/// A curve id, with the handler's message for anything else.
fn spline_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a spline id")),
    }
}

/// What `sketch.spline.reverse` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SplineReverseResult {
    /// The spline's curve id (unchanged).
    pub curve: String,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_create_tests.rs"]
mod tests;
