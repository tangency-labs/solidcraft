//! Contracts and result types for the commands in `cmd/sketch_create.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{SketchRef, present};

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
