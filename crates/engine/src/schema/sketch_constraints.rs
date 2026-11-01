//! Contracts and result types for the commands in `cmd/sketch_constraints.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{SketchRef, present};

/// Switch a circle's or an arc's dimension between its radius and its diameter.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ToggleDiameterParams {
    /// The radius or diameter dimension's constraint id.
    #[serde(deserialize_with = "dimension_id")]
    pub constraint: String,
    /// `true` for a diameter, `false` for a radius (default: the other one).
    #[serde(default)]
    pub diameter: Option<bool>,
    /// The sketch (default: the one being edited).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchRef")]
    pub sketch: Option<SketchRef>,
}

/// A constraint id, with the handler's message for anything else.
fn dimension_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a dimension id")),
    }
}

/// What `sketch.toggle_diameter` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToggleDiameterResult {
    /// The dimension's constraint id (unchanged).
    pub constraint: String,
    /// Whether it now holds the diameter.
    pub diameter: bool,
    /// The sketch's solve: `{solved, dof}`.
    pub sketch: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_constraints_tests.rs"]
mod tests;
