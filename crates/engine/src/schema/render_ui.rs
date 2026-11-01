//! Clipboard requests for the Render workspace; headless calls only echo validated text.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckReport {
    /// Report text, bounded to 65536 UTF-8 bytes by the command handler.
    #[schemars(length(min = 1, max = 65536))]
    pub text: String,
}
