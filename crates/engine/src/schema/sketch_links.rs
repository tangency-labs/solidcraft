//! Contracts and result types for the commands in `cmd/sketch_links.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::types::present;

/// A sketch by its feature id or name, with the message `sketch::target_sketch` gives. Whether
/// a sketch has that id or name is checked by the handler.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchKey(pub Value);

impl<'de> Deserialize<'de> for SketchKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            v @ (Value::Number(_) | Value::String(_)) => Ok(SketchKey(v)),
            _ => Err(D::Error::custom("must be an id or a name")),
        }
    }
}

impl JsonSchema for SketchKey {
    fn schema_name() -> Cow<'static, str> {
        "SketchKey".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A sketch's feature id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// A list of ids, with the message the link commands give for anything else.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IdList(pub Vec<String>);

impl<'de> Deserialize<'de> for IdList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Array(a) => a.into_iter().map(|v| if let Value::String(s) = v { Some(s) } else { None }).collect::<Option<Vec<_>>>(),
            _ => None,
        }
        .map(IdList)
        .ok_or_else(|| D::Error::custom("must list ids"))
    }
}

impl JsonSchema for IdList {
    fn schema_name() -> Cow<'static, str> {
        "IdList".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A list of ids (only the first 10 000 are read).", "type": "array", "items": {"type": "string"}})
    }
}

/// The link to repair, with `sketch.repair_link`'s message for anything but an id.
#[derive(Clone, Debug, PartialEq)]
pub struct RepairLinkId(pub String);

impl<'de> Deserialize<'de> for RepairLinkId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) => Ok(RepairLinkId(s)),
            _ => Err(D::Error::custom("must name the link to repair")),
        }
    }
}

impl JsonSchema for RepairLinkId {
    fn schema_name() -> Cow<'static, str> {
        "RepairLinkId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The id of the link to repair.", "type": "string"})
    }
}

/// List a sketch's references (projected, intersected and included geometry) with their
/// source, the placement they read and their status. Read-only.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ReferencesParams {
    /// The sketch. Default: the sketch being edited, else every sketch with references.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchKey")]
    pub sketch: Option<SketchKey>,
}

/// Read each reference where its source is now, solve and store the sketch, and rebuild what
/// follows it.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UpdateReferencesParams {
    /// The sketch. Default: the sketch being edited, else every sketch with references.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchKey")]
    pub sketch: Option<SketchKey>,
    /// First capture the uncaptured moves of the occurrence placements the references read.
    /// Default: false.
    #[serde(default)]
    pub capture: Option<bool>,
    /// First update linked designs, one component at a time; a component whose file can't be
    /// read keeps its bodies and is reported unavailable. Default: false.
    #[serde(default)]
    pub files: Option<bool>,
}

/// The links a link command works on, in the sketch being edited or the one named. Give
/// `link`, `links` or `entities` (any of them together).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LinksParams {
    /// One link, by id.
    #[serde(default)]
    pub link: Option<String>,
    /// Links, by id.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdList")]
    pub links: Option<IdList>,
    /// Curves or points (or 3D curves) of links: the links they belong to.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdList")]
    pub entities: Option<IdList>,
    /// The sketch. Default: the sketch being edited.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchKey")]
    pub sketch: Option<SketchKey>,
}

/// Show or hide links. Hidden links aren't drawn or picked; they still bound profiles, keep
/// their constraints and follow their sources. Give `link`, `links` or `entities`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LinkVisibilityParams {
    /// One link, by id.
    #[serde(default)]
    pub link: Option<String>,
    /// Links, by id.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdList")]
    pub links: Option<IdList>,
    /// Curves or points (or 3D curves) of links: the links they belong to.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "IdList")]
    pub entities: Option<IdList>,
    /// The sketch. Default: the sketch being edited.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "SketchKey")]
    pub sketch: Option<SketchKey>,
    /// Shown (true) or hidden (false). Default: shown if any of them is hidden, else hidden.
    #[serde(default)]
    pub visible: Option<bool>,
}

/// Point a link of the sketch being edited at a new source. Its curves and points keep their
/// ids, and the constraints on them, where the new geometry has the same shape.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RepairLinkParams {
    /// The link to repair.
    pub link: RepairLinkId,
    /// The new source, as one entry of `sketch.project`'s `refs` takes it (`{edge: [x,y,z],
    /// body?, path?}`, `{face: …}`, `{vertex: …}`, `{body}`, `{sketch, curve}`, …); `path` names
    /// the exact occurrence placement by occurrence ids.
    #[serde(rename = "ref")]
    pub reference: Value,
    /// The body whose placement a pick is read in (read by the engine's frame mapping, before
    /// the command runs). Usually given inside `ref` instead.
    #[serde(default)]
    pub body: Option<String>,
}

/// A solve's outcome after the edit.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SolveInfo {
    /// The sketch solved, keeping its shape's orientation.
    pub solved: bool,
    /// Degrees of freedom left.
    pub dof: usize,
}

/// An occurrence placement path and its name ("A:2/B:1").
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacementInfo {
    /// The occurrence ids, from the top of the design down.
    pub path: Vec<u64>,
    /// The occurrences' names, joined by `/` ("the design" for the root).
    pub name: String,
}

/// One reference of a sketch.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReferenceEntry {
    /// The sketch's feature id.
    pub sketch: u64,
    pub sketch_name: String,
    /// The link's id.
    pub link: String,
    /// How the geometry was brought in: `project`, `include` or `intersect`.
    pub kind: String,
    /// The source, in words.
    pub source: String,
    /// The bodies the source is on.
    pub bodies: Vec<String>,
    /// The exact occurrence placement it reads, for a reference to another component.
    pub occurrence: Option<PlacementInfo>,
    /// The placement of the sketch's component it was made in, for a reference to another
    /// component.
    pub author: Option<PlacementInfo>,
    /// `current`; `stale` (shown where the source is now, stored where it was); `pending` (a
    /// placement it reads has an uncaptured move); `frozen`; or `lost` (it keeps its last
    /// geometry).
    pub status: String,
    pub hidden: bool,
    pub frozen: bool,
    /// All its curves are construction geometry.
    pub construction: bool,
    /// The source edges of deleted members, which stay out of it, by persistent name.
    pub omitted: Vec<String>,
    /// Its curves' ids.
    pub curves: Vec<String>,
    /// Its points' ids.
    pub points: Vec<String>,
    /// Why it is lost or stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// What `sketch.references` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReferencesResult {
    pub references: Vec<ReferenceEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One entry of what `sketch.update_references` did: a link and how it went, or a sketch's
/// warnings.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum LinkUpdate {
    Link {
        /// The sketch's feature id.
        sketch: u64,
        /// The link's id.
        link: String,
        /// The source, in words.
        source: String,
        /// `changed`, `unchanged`, `frozen` or `lost`.
        status: String,
    },
    Warnings {
        /// The sketch's feature id.
        sketch: u64,
        /// What updating its references reported.
        warnings: Vec<String>,
    },
}

/// A linked component whose design file couldn't be read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UnavailableFile {
    /// The component's id.
    pub component: u64,
    pub error: String,
}

/// What updating linked designs did.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FilesUpdate {
    /// The linked components `component.update_links` updated, by name.
    pub updated: Vec<String>,
    /// The ones it found unchanged, by name.
    pub unchanged: Vec<String>,
    pub unavailable: Vec<UnavailableFile>,
}

/// A feature after the updated sketches that failed to rebuild.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FailedFeature {
    /// The feature's id.
    pub feature: u64,
    pub name: String,
    pub error: String,
}

/// What `sketch.update_references` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateReferencesResult {
    pub links: Vec<LinkUpdate>,
    /// The occurrences whose moves were captured (with `capture`).
    pub captured: Vec<u64>,
    /// What updating linked designs did (with `files`), else `null`.
    pub files: Option<FilesUpdate>,
    pub failed: Vec<FailedFeature>,
    /// Nothing failed, no reference is lost and every linked design could be read.
    pub ok: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.freeze_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FreezeLinkResult {
    /// The links frozen.
    pub frozen: Vec<String>,
    /// The sketch's solve after the edit, or `null` when it wasn't solved.
    pub sketch: Option<SolveInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.resume_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResumeLinkResult {
    /// The links resumed.
    pub resumed: Vec<String>,
    /// What reading them where their sources are now reported.
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.repair_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairLinkResult {
    /// The link's id.
    pub link: String,
    /// The new geometry has another shape, so the link's curves were made again.
    pub rebuilt: bool,
    /// The constraints removed with geometry the new source doesn't have.
    pub dropped_constraints: Vec<String>,
    /// Its curves' ids now.
    pub curves: Vec<String>,
    /// Its points' ids now.
    pub points: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.remove_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RemoveLinkResult {
    /// The links removed.
    pub removed: Vec<String>,
    /// The sketch's solve after the edit, or `null` when it wasn't solved.
    pub sketch: Option<SolveInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `sketch.link_visibility` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkVisibilityResult {
    /// The links shown or hidden.
    pub links: Vec<String>,
    /// Whether they are shown now.
    pub visible: bool,
    /// The sketch's solve after the edit, or `null` when it wasn't solved.
    pub sketch: Option<SolveInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "sketch_links_tests.rs"]
mod tests;
