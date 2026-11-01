//! Contracts shared by design and STEP component links.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{IdOrName, present};

/// List source paths and whether each file changed since it was last read.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LinksParams {}

/// Read linked sources again, replacing their bodies in one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UpdateLinksParams {
    /// One linked component by id or name. Default: update every link.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdOrName")]
    pub component: Option<IdOrName>,
    /// Between 1 and 10,000 linked components; failed sources keep their bodies.
    /// Mutually exclusive with component.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<IdOrName>")]
    pub components: Option<Vec<IdOrName>>,
}

/// Keep a linked component's bodies and forget its source file.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BreakLinkParams {
    /// The linked component, by id or name.
    pub component: IdOrName,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkInfo {
    pub component: u64,
    pub name: Option<String>,
    pub path: String,
    pub file: String,
    pub found: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinksResult {
    pub links: Vec<LinkInfo>,
    /// Parameter warnings in warn checking mode; omitted for a clean call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkFailure {
    pub component: u64,
    pub name: String,
    pub error: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateLinksResult {
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub warnings: Vec<String>,
    pub failed: Vec<LinkFailure>,
    /// Parameter warnings in warn checking mode; omitted for a clean call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BreakLinkResult {
    pub component: u64,
    pub path: String,
    /// Parameter warnings in warn checking mode; omitted for a clean call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
