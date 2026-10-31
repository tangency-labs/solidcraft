//! Command contract for attached gussets.
use super::{FeatureCommon, LengthExpr, Point3, feature_common};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FacePick {
    pub face: Point3,
    pub body: String,
    pub index: usize,
    #[serde(default)]
    #[schemars(length(max = 64))]
    pub path: Option<Vec<u64>>,
}
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Center,
    Positive,
    Negative,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct GussetParams {
    /// Exactly two ordered, planar support faces in world coordinates.
    #[schemars(length(min = 2, max = 2))]
    pub supports: Vec<FacePick>,
    pub leg1: LengthExpr,
    pub leg2: LengthExpr,
    pub thickness: LengthExpr,
    #[serde(default)]
    pub relief: Option<LengthExpr>,
    #[serde(default)]
    pub clip: Option<LengthExpr>,
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    #[serde(default)]
    pub placement: Placement,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}
