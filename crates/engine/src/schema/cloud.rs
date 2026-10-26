//! Contracts for durable conflict resolution.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloudDestination {
    pub connection: String,
    pub project: String,
    pub file: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    Local,
    Remote,
}

/// Resolve a captured conflict against both observed design heads.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveParams {
    pub destination: CloudDestination,
    pub revision: Option<String>,
    pub remote_revision: Option<String>,
    pub keep: ConflictChoice,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OperationResult {
    pub operation: String,
    pub connection: String,
}
