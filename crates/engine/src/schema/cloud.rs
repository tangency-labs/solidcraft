//! Parameter contracts and replies for asynchronous cloud commands.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use solidcraft_cloud::{Connection, Destination, Settings};

pub type CloudDestination = solidcraft_cloud::Destination;
pub type ConflictChoice = solidcraft_cloud::conflicts::Keep;

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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectParams {
    pub connection: Connection,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureParams {
    pub settings: Settings,
}

#[derive(Default, Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ConnectionParams {
    pub connection: Option<String>,
}

#[derive(Default, Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TestParams {
    pub connection: Option<String>,
    pub proposed: Option<Connection>,
    /// Native Git program override for this test; omitted uses saved settings.
    pub git_executable: Option<String>,
    /// Read-only GitHub repository existence check. Default: false.
    pub metadata_only: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    pub destination: Destination,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenRevisionParams {
    pub destination: Destination,
    /// Exact queued capture to inspect offline; omitted for the current head.
    pub local_revision: Option<String>,
}

#[derive(Default, Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SaveParams {
    pub destination: Option<Destination>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscardParams {
    pub destination: Destination,
    pub revision: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagementParams {
    pub destination: Destination,
    pub revision: Option<String>,
    pub file: Option<String>,
    pub commit: Option<String>,
}

/// No JSON parameters; secret inputs use the separate host API.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

/// Reload durable host settings or inspect the current configuration.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsParams {
    /// Load durable platform settings asynchronously. Default: false.
    pub reload: Option<bool>,
}

/// Identify one asynchronous operation.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationParams {
    pub operation: String,
}

/// Inspect one operation, or all retained operations when omitted.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StatusParams {
    pub operation: Option<String>,
    /// Return a completed result and release its retained payload.
    pub consume: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SettingsResult {
    pub settings: Settings,
    pub binding: Option<crate::cloud::Binding>,
    pub configuring: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ConfigurationResult {
    Pending(Box<solidcraft_cloud::jobs::JobStatus>),
    Current(SettingsResult),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UnchangedResult {
    pub unchanged: bool,
    pub binding: Option<crate::cloud::Binding>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SaveResult {
    Pending(Box<solidcraft_cloud::jobs::JobStatus>),
    Unchanged(UnchangedResult),
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum StatusResult {
    One(Box<solidcraft_cloud::jobs::JobStatus>),
    All(Vec<solidcraft_cloud::jobs::JobStatus>),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OpenResult {
    pub opened: bool,
    pub binding: Option<crate::cloud::Binding>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ThumbnailResult {
    /// Base64 PNG, or null when the design has no preview.
    pub png: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WebsiteResult {
    pub url: String,
}
