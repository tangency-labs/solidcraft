//! Environment editor commands; saved values retain the existing render setup format.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentParams {
    pub name: String,
    /// Complete numerical draft, validated before applying the requested edit.
    #[serde(default = "empty_patch")]
    pub set: Value,
    /// Finite degrees within +/- one million; wrapped to [-180, 180).
    pub rotation_deg: Option<f64>,
    /// Linear intensity: 0..64 for CPU and 0..100 for GPU.
    pub intensity: Option<f64>,
}

fn empty_patch() -> Value {
    serde_json::json!({})
}

#[derive(Serialize, JsonSchema)]
pub struct EnvironmentResult {
    pub name: String,
    pub setup: Value,
}
