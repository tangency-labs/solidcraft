//! Contracts and result types for the commands in `cmd/inspect.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which command to describe.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DescribeParams {
    /// The command's id, such as `solid.extrude`.
    #[serde(deserialize_with = "command_id")]
    pub command: String,
}

/// A command id, with the `engine.describe` handler's message for anything else.
fn command_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        _ => Err(serde::de::Error::custom("must be a command id")),
    }
}

/// What `engine.describe` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DescribeResult {
    pub id: String,
    pub label: String,
    /// The command's parameter documentation, as prose.
    pub params: String,
    /// The parameters' JSON Schema, or `null` when the command has no contract yet.
    pub input: Option<Value>,
    /// The successful result's JSON Schema, or `null` when not described yet.
    pub output: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Inspect the bodies affected by one timeline feature.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FeatureEffectsParams {
    pub feature: super::types::IdOrName,
}

/// A body at the selected feature's point in history.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AffectedBody {
    pub name: String,
    pub component: u64,
    pub component_name: Option<String>,
    /// Current world-body names accepted by select.set, including repeated occurrences.
    /// Empty when the body no longer exists in the current model.
    pub selectable_bodies: Vec<String>,
}

/// Effects computed from the states immediately before and after a feature.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FeatureEffects {
    pub feature: u64,
    /// False for suppressed, rolled-back, failed or stale features; their lists are empty.
    pub evaluated: bool,
    pub created_bodies: Vec<AffectedBody>,
    pub changed_bodies: Vec<AffectedBody>,
    pub removed_bodies: Vec<AffectedBody>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
