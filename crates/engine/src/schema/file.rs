//! Contracts for local import-cache maintenance.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportCacheClearParams {}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportCacheClearResult {
    /// Number of cached imports removed. Open designs and stand-up answers are retained.
    pub removed: usize,
}
