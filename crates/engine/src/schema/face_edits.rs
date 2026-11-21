//! Contracts for Delete Face and Offset Face in `cmd/features.rs` (#12740). Each adds a feature
//! and returns `add_feature`'s reply, a [`super::features::FeatureResult`].
//!
//! The handlers read their faces with `features::face_points`, which [`FaceRefs`] reads as they
//! do, with their messages. Whether a face is there, and whether the edit fits the part, stay the
//! handlers' to check. Where a handler ignores a key of the wrong type (a `heal` that isn't true
//! or false, a `body` that isn't text), the contract refuses it.

use schemars::{JsonSchema, Schema};
use serde::Deserialize;

use super::motion::require;
use super::types::{FaceRefs, FeatureCommon, LengthExpr, feature_common, present};

fn faces_required(schema: &mut Schema) {
    require(schema, "faces");
}

/// Delete faces from a body. With `heal`, the faces around close the gap, as when a rounded edge,
/// a hole or a boss is taken off; without, the gap stays open and the faces left are a surface
/// body. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = faces_required)]
pub struct DeleteFaceParams {
    // Optional in the type, required in the schema: left out, the handler refuses the call with
    // its own message; a `null` goes to `FaceRefs` (`present`), which refuses it the same way.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "FaceRefs")]
    pub faces: Option<FaceRefs>,
    /// The body the faces are on, by name. Default: the body at each point.
    #[serde(default)]
    pub body: Option<String>,
    /// True: the faces around close the gap. False: the gap stays open, and what is left is a
    /// surface body. Default: true.
    #[serde(default)]
    pub heal: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// Move faces of a body along their normals, the faces around them following. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = faces_required)]
pub struct OffsetFaceParams {
    // Required in the schema (`faces_required`), optional in the type, as Delete Face's.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "FaceRefs")]
    pub faces: Option<FaceRefs>,
    /// How far, along each face's outward normal; negative: inwards.
    pub distance: LengthExpr,
    /// The body the faces are on, by name. Default: the body at each point.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

#[cfg(test)]
#[path = "face_edits_tests.rs"]
mod tests;
