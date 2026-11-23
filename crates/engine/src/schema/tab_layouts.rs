//! Contracts for the app's named, reusable view layouts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Save, apply or remove a layout by name. The checked request is also the reply.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabLayoutNameParams {
    /// 1 to 64 characters after trimming, without control characters.
    pub name: String,
}

/// Rename a layout. The checked request is also the reply.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabLayoutRenameParams {
    /// The name of an existing layout.
    pub name: String,
    /// 1 to 64 characters after trimming, without control characters.
    pub new_name: String,
}

/// The app's saved layout names.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TabLayoutListReply {
    pub layouts: Vec<String>,
}
