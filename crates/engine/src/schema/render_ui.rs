//! Contracts for transient Render workspace clipboard and retained-result requests.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckReport {
    /// Report text, bounded to 65536 UTF-8 bytes by the command handler.
    #[schemars(length(min = 1, max = 65536))]
    pub text: String,
}

/// Select or discard a retained result without editing a design.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultIndex {
    /// Zero-based index, oldest first. The result must still be retained.
    #[schemars(range(min = 0, max = 7))]
    pub index: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveSide {
    Live,
}

/// A live product view or a retained result's zero-based index (oldest first).
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum CompareSide {
    Live(LiveSide),
    Result(#[schemars(range(min = 0, max = 7))] u32),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparePair {
    pub left: CompareSide,
    pub right: CompareSide,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareOff {
    /// Must be true. Leave comparison without changing retained results.
    pub off: bool,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CompareParams {
    Off(CompareOff),
    Pair(ComparePair),
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareDivider {
    /// Finite fraction of the canvas width, clamped to 0.05..0.95.
    pub position: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchParams {
    /// Optional render.start request. Its setup is shared, with each saved shot's camera.
    /// Without a request, use the first saved shot's setup (name order).
    pub request: Option<std::collections::BTreeMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BatchResult {
    /// Total accepted shots, including the first active job. At most 32.
    pub queued: u32,
}
