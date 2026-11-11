//! Command contract for Rotate Member.
use super::end_cap::FacePick;
use super::{AngleExpr, FeatureCommon, feature_common};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RotateMemberParams {
    /// A flat side or end face of the frame member to turn, picked in world mm.
    pub face: FacePick,
    /// The angle the member is turned to about its own line, counter-clockwise looking back from
    /// the line's end.
    pub angle: AngleExpr,
    /// Turn the member so a side is parallel to this line instead (not used yet).
    #[serde(default)]
    pub parallel_to: Option<Value>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}
