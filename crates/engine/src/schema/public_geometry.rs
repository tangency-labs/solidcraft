//! Contracts and result types for the commands in `cmd/public_geometry.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::IdOrName;

/// Share reference elements of a skeleton component as one named group, a public geometry.
/// Each item is a sketch or a construction plane, axis or point of a skeleton component, and is
/// published under the name its feature has now. Renaming the feature later changes nothing;
/// deleting it takes it out of the group. An element is in one public geometry at most, and
/// published names are unique in the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PublishParams {
    /// The public geometry's name: plain text of at most 128 characters, not used by another
    /// public geometry.
    pub name: String,
    /// The features to publish, each by id or name (1 to 10 000). A name that several skeletons
    /// use means the one in the active component; give the id otherwise.
    pub items: Vec<IdOrName>,
}

/// One published element.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PublishedEntry {
    /// The name it is published under.
    pub name: String,
    /// The feature it is, by id.
    pub feature: u64,
    /// What kind of feature it is, such as a sketch or a construction plane.
    #[serde(rename = "type")]
    pub kind: String,
}

/// What `geometry.publish` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PublishResult {
    /// The public geometry's name.
    pub name: String,
    /// Its elements, in the order given.
    pub items: Vec<PublishedEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Remove a public geometry: the whole group. The features it shared stay in the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UnpublishParams {
    /// The public geometry's name.
    pub name: String,
}

/// What `geometry.unpublish` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnpublishResult {
    /// The public geometry that was removed.
    pub name: String,
    /// How many elements it held.
    pub removed: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `geometry.publications` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct PublicationsParams {}

/// One public geometry.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Publication {
    /// Its name.
    pub name: String,
    /// Its elements.
    pub items: Vec<PublishedEntry>,
}

/// What `geometry.publications` returns: the design's public geometries, in the order they
/// were made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PublicationsResult {
    /// The public geometries.
    pub publications: Vec<Publication>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "public_geometry_tests.rs"]
mod tests;
