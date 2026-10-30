//! Contracts and result types for the commands in `cmd/surface_analysis.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `true`, `false` or null, with the handler's message for anything else.
fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        Value::Bool(b) => Ok(Some(b)),
        _ => Err(serde::de::Error::custom("must be true or false")),
    }
}

/// Say which view analysis is on (zebra, draft, curvature map, environment map, accessibility,
/// continuity, isocurves or curvature combs: one at a time), or turn it off.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AnalysisParams {
    /// Turn the view analysis on now off.
    #[serde(default, deserialize_with = "flag")]
    pub off: Option<bool>,
}

/// What `inspect.analysis` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AnalysisState {
    /// The command id of the view analysis on (after `off`), or null when none is.
    pub active: Option<String>,
}

#[cfg(test)]
#[path = "surface_analysis_tests.rs"]
mod tests;
