//! Command contract for frame member end caps.
use super::{FeatureCommon, LengthExpr, NumberExpr, Point3, feature_common};
use schemars::JsonSchema;
use serde::Deserialize;

/// A face picked in the view: a world point on it, and the body and face it was picked on.
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
pub enum Direction {
    /// Past the end: the frame gets longer by the thickness.
    #[default]
    Outward,
    /// Into the member, which is cut back by the thickness.
    Inward,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EndCapParams {
    /// The open end faces to close, each a planar end of a frame member.
    #[schemars(length(min = 1, max = 256))]
    pub faces: Vec<FacePick>,
    pub thickness: LengthExpr,
    /// The inset from the profile's outer edge as a length (give this or `ratio`).
    #[serde(default)]
    pub offset: Option<LengthExpr>,
    /// The inset as a share of the thickness (default 0.5 when no `offset` is given).
    #[serde(default)]
    pub ratio: Option<NumberExpr>,
    /// The corner chamfer, along each edge from the corner (default 0: sharp corners).
    #[serde(default)]
    pub chamfer: Option<LengthExpr>,
    #[serde(default)]
    pub direction: Direction,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}
