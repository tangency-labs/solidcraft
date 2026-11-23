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
