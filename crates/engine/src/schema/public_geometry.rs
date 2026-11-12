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

/// Copy a public geometry of another design file into the active component. Each plane, axis or
/// point it shares becomes a construction feature here, with fixed geometry, under its published
/// name, placed as it is in the source design. The copy keeps what it took: the file is read
/// again only by `geometry.update`. A design can't copy from itself.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ImportParams {
    /// The design file to copy from.
    pub path: String,
    /// The name of the public geometry to take from it.
    pub publication: String,
    /// The copy's own name here, not used by another copy. Default: the public geometry's name,
    /// numbered when that is taken.
    #[serde(default)]
    pub name: Option<String>,
}

/// What `geometry.import` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ImportResult {
    /// The copy's name.
    pub name: String,
    /// The file it copies from, as found.
    pub file: String,
    /// The public geometry it takes.
    pub publication: String,
    /// The construction features made here, one per copied element.
    pub items: Vec<PublishedEntry>,
    /// Published elements that are not copied yet (sketches).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Read the source files of geometry copies again and bring the copies up to date: a copied
/// element takes its new geometry, an element added to the public geometry is added here, and an
/// element that is no longer published keeps what it last had and is reported as lost. A file
/// that can't be read leaves its copy as it is.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UpdateParams {
    /// The copies to update, by name. Default: all of them.
    #[serde(default)]
    pub copies: Option<Vec<String>>,
}

/// What `geometry.update` did to one copy.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdatedCopy {
    /// The copy's name.
    pub name: String,
    /// `updated`, `unchanged`, or why the file was not read: `missing`, `unreadable`,
    /// `not_allowed`.
    pub state: String,
    /// The copied elements whose geometry changed.
    pub changed: Vec<String>,
    /// The elements added to the copy.
    pub added: Vec<String>,
    /// The copied elements the file no longer publishes; they keep what they last had.
    pub lost: Vec<String>,
    /// Why the file was not read, or what in it could not be used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What `geometry.update` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateResult {
    /// One entry per copy asked for.
    pub copies: Vec<UpdatedCopy>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `geometry.copies` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CopiesParams {}

/// One geometry copy.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyInfo {
    /// The copy's name.
    pub name: String,
    /// The file it copies from, resolved against this design's folder.
    pub file: String,
    /// The public geometry it takes.
    pub publication: String,
    /// The component its features are in, by id (0 is the design).
    pub component: u64,
    /// Its construction features.
    pub items: Vec<PublishedEntry>,
    /// Where the copy stands against its file: `up_to_date`, `out_of_date` (the file changed
    /// since the copy last read it), `missing`, `unreadable` or `not_allowed`.
    pub state: String,
    /// Why the file can't be reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What `geometry.copies` returns: the design's geometry copies, in the order they were made.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopiesResult {
    /// The geometry copies.
    pub copies: Vec<CopyInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "public_geometry_tests.rs"]
mod tests;
