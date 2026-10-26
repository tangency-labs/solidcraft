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
