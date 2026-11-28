//! Section clipping contract. Explicit planes use numerical millimetre coordinates.
use super::{AxisDir, LengthExpr, Point3, present};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Plane {
    Name(String),
    Id(u64),
    Explicit { origin: Option<Point3>, normal: AxisDir },
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionParams {
    /// Required unless clearing. XY/XZ/YZ, construction name/id, or {origin?,normal}.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Plane")]
    pub plane: Option<Plane>,
    /// Signed distance against the original plane normal (positive cuts more away), in design units or an expression.
    pub offset: Option<LengthExpr>,
    /// World coordinate on a named plane's normal axis; mutually exclusive with offset.
    pub at: Option<LengthExpr>,
    pub flip: Option<bool>,
    pub hatch: Option<bool>,
    pub clear: Option<bool>,
    /// Add the plane to the section instead of replacing it (at most 6 planes).
    pub add: Option<bool>,
    /// Move this plane (by id) to `plane`, keeping the others.
    pub replace: Option<u64>,
    /// Take this plane (by id) out of the section.
    pub remove: Option<u64>,
    /// Hide planes (by id): they stay in the section and cut nothing.
    pub hide: Option<PlaneIds>,
    /// Show hidden planes (by id) again.
    pub show: Option<PlaneIds>,
    /// The Analysis folder's eye: false hides the planes on show until true brings them back.
    pub folder: Option<bool>,
}
/// One plane's id or a list of them.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PlaneIds {
    One(u64),
    Many(Vec<u64>),
}
#[derive(Serialize, JsonSchema)]
pub struct Section {
    pub id: u64,
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub hatch: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
}
#[derive(Serialize, JsonSchema)]
pub struct SectionResult {
    pub section: Option<Section>,
    /// Every plane of the section, by id.
    pub planes: Vec<SectionPlaneState>,
    /// While the Analysis folder is hidden: the ids that come back with it.
    pub folder: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
#[derive(Serialize, JsonSchema)]
pub struct SectionPlaneState {
    pub id: u64,
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub hidden: bool,
}

/// Keep the Section Analysis on show as a named one of the design (`inspect.section.save`). One
/// undo step. A saved section of the same name is replaced.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionSaveParams {
    /// The name, 1 to 128 characters once surrounding spaces are cut.
    pub name: String,
}

/// Show a saved section in place of the one on show (`inspect.section.show`).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionNamedParams {
    /// The saved section, by name.
    pub section: String,
}

/// Rename a saved section (`inspect.section.rename`). One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionRenameParams {
    /// The saved section to rename, by name.
    pub section: String,
    /// The new name, 1 to 128 characters once surrounding spaces are cut; no other saved
    /// section may have it.
    pub name: String,
}

/// `inspect.section.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionListParams {}

/// What `inspect.section.save` and `inspect.section.rename` return.
#[derive(Serialize, JsonSchema)]
pub struct SectionSaved {
    /// The name the section is kept under.
    pub section: String,
    /// How many planes it holds, hidden ones included.
    pub planes: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `inspect.section.show` returns.
#[derive(Serialize, JsonSchema)]
pub struct SectionShown {
    /// The saved section now on show.
    pub section: String,
    /// Every plane of the section by id, hidden ones included.
    pub planes: Vec<SectionPlaneState>,
    /// Whether the cut faces are hatched.
    pub hatch: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `inspect.section.delete` returns.
#[derive(Serialize, JsonSchema)]
pub struct SectionDeleted {
    /// The name of the saved section that is gone.
    pub deleted: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One saved section, as `inspect.section.list` gives it.
#[derive(Serialize, JsonSchema)]
pub struct SectionEntry {
    pub name: String,
    pub planes: Vec<SectionPlaneState>,
    pub hatch: bool,
}

/// What `inspect.section.list` returns.
#[derive(Serialize, JsonSchema)]
pub struct SectionListResult {
    /// The design's saved sections, in the order they were made.
    pub sections: Vec<SectionEntry>,
    /// The saved section the planes on show are exactly those of, if any.
    pub shown: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}
