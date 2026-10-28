//! Contracts and result types for the commands in `cmd/section.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{IdOrName, LengthExpr};

/// Make a new Section Analysis and show it (the one shown before is hidden, and kept), or with
/// `clear` hide the one shown.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionParams {
    /// The plane to cut along: `XY`, `XZ` or `YZ`; a construction plane's name or id (the cut
    /// follows it); `{face: [x, y, z], body?, index?}`, a planar face by a point on it (the cut
    /// follows the face); or `{origin: [x, y, z], normal: [x, y, z]}`, a fixed plane. Required
    /// unless `clear`.
    #[serde(default)]
    pub plane: Option<Value>,
    /// The distance along the plane's normal (XY: +Z, XZ: +Y, YZ: +X). Default: 0.
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// For XY, XZ or YZ only: the cut's world coordinate on the plane's axis (XY: Z, XZ: Y,
    /// YZ: X), instead of `offset`.
    #[serde(default)]
    pub at: Option<LengthExpr>,
    /// Keep the plane's normal side instead of the other. Default: false.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Hatch the cut faces, in each body's colour. Default: true.
    #[serde(default)]
    pub hatch: Option<bool>,
    /// The new section's name. Default: `Section N`, the next free number.
    #[serde(default)]
    pub name: Option<String>,
    /// Hide the section shown now (it stays in the design) instead of making one.
    #[serde(default)]
    pub clear: Option<bool>,
}

/// Change a Section Analysis and show it. Keys left out keep their values.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionEditParams {
    /// The section: its id or its name.
    pub section: IdOrName,
    /// A new plane, as `inspect.section` takes it.
    #[serde(default)]
    pub plane: Option<Value>,
    /// The distance along the plane's normal.
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// For XY, XZ or YZ only: the cut's world coordinate on the plane's axis, instead of `offset`.
    #[serde(default)]
    pub at: Option<LengthExpr>,
    /// Keep the plane's normal side instead of the other.
    #[serde(default)]
    pub flip: Option<bool>,
    /// Hatch the cut faces.
    #[serde(default)]
    pub hatch: Option<bool>,
    /// A new name.
    #[serde(default)]
    pub name: Option<String>,
}

/// Show or hide a Section Analysis (showing one hides the one shown before), or without
/// `section` the Analysis folder (hidden, no section is shown; shown again, the last one is).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionVisibilityParams {
    /// The section: its id or its name. Default: the Analysis folder.
    #[serde(default)]
    pub section: Option<IdOrName>,
    pub visible: bool,
}

/// Rename a Section Analysis.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionRenameParams {
    /// The section: its id or its name.
    pub section: IdOrName,
    /// The new name: 1 to 128 characters, not another section's.
    pub name: String,
}

/// Delete a Section Analysis.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionDeleteParams {
    /// The section: its id or its name.
    pub section: IdOrName,
}

/// List the design's Section Analyses.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SectionListParams {}

/// The cut a Section Analysis makes now.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SectionCut {
    pub id: u64,
    pub name: String,
    /// A point of the cutting plane, in mm.
    pub origin: [f64; 3],
    /// The plane's unit normal: what lies on this side is hidden.
    pub normal: [f64; 3],
    pub hatch: bool,
    /// For XY, XZ and YZ: the world axis the plane is square to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<String>,
    /// For XY, XZ and YZ: the cut's world coordinate on that axis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<f64>,
}

/// What `inspect.section`, `inspect.section.edit` and `inspect.section.visibility` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SectionResult {
    /// The section that cuts the view now, or `null` when none does.
    pub section: Option<SectionCut>,
    /// The section made or changed (for `visibility`, the one named), when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `inspect.section.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SectionRenameResult {
    pub id: u64,
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `inspect.section.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SectionDeleteResult {
    /// The deleted section's name.
    pub deleted: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One Section Analysis in `inspect.section.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SectionEntry {
    pub id: u64,
    pub name: String,
    /// The plane as saved: `{type: origin, name}`, `{type: construction, feature}`,
    /// `{type: face, body, face, at}` or `{type: fixed, origin, normal}`.
    pub plane: Value,
    /// The distance along the plane's normal, as an expression (empty: 0).
    pub offset: String,
    pub flip: bool,
    pub hatch: bool,
    /// Its eye is on.
    pub shown: bool,
    /// Where it cuts now, while its plane can be found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal: Option<[f64; 3]>,
    /// Why it cannot cut now (its plane or face is gone, or no longer flat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `inspect.section.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SectionListResult {
    pub sections: Vec<SectionEntry>,
    /// The Analysis folder is hidden: no section cuts the view.
    pub folder_hidden: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "section_tests.rs"]
mod tests;
