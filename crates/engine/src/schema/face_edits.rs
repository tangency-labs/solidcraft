//! Contracts for the face edits in `cmd/features.rs`. Each adds a feature and returns
//! `add_feature`'s reply, a [`super::features::FeatureResult`].
//!
//! Delete Face and Offset Face read their faces with `features::face_points`, which [`FaceRefs`]
//! reads as they do. Whether a face is there, and whether the edit fits the part, stay the
//! handlers' to check. Where a handler ignores a key of the wrong type (a `heal` that isn't true
//! or false, a `body` that isn't text), the contract refuses it.

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;

use super::motion::require;
use super::types::{AngleExpr, AxisDir, FaceRef, FaceRefs, FeatureCommon, LengthExpr, PointExpr3, feature_common, present};

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
    /// How far, along each face's outward normal; negative: inwards; 0 leaves the faces where they are.
    pub distance: LengthExpr,
    /// The body the faces are on, by name. Default: the body at each point.
    #[serde(default)]
    pub body: Option<String>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

fn face_motion(schema: &mut Schema) {
    schema.insert("anyOf".into(), serde_json::json!([{"required": ["translate"]}, {"required": ["angle"]}]));
}

/// Rotate one face, then translate it. Its neighbours extend or trim to meet it. Supply a
/// translation, an angle, or both; either may be zero. No other body takes part.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = face_motion)]
pub struct TransformFaceFields {
    /// A point on the selected face or its name from `model.faces`.
    pub face: FaceRef,
    /// The body that owns the face. Required if the pick could refer to more than one body.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub body: Option<String>,
    /// Distances to move in world X, Y and Z after the rotation. Default: zero.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub translate: Option<PointExpr3>,
    /// The rotation axis, as X, Y, Z or a nonzero direction in world coordinates. Default: Z.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisDir")]
    pub axis: Option<AxisDir>,
    /// A point on the rotation axis, in world coordinates. Default: the picked point on the face.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PointExpr3")]
    pub origin: Option<PointExpr3>,
    /// The rotation about the axis, in degrees or as an angle expression. Default: zero.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AngleExpr")]
    pub angle: Option<AngleExpr>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// The face edit's contract includes the choice of motion, not just each field's type.
#[derive(Debug, Deserialize)]
#[serde(try_from = "TransformFaceFields")]
pub struct TransformFaceParams(pub TransformFaceFields);

impl TryFrom<TransformFaceFields> for TransformFaceParams {
    type Error = &'static str;

    fn try_from(fields: TransformFaceFields) -> Result<Self, Self::Error> {
        if fields.translate.is_none() && fields.angle.is_none() {
            return Err("give `translate`, `angle`, or both");
        }
        Ok(Self(fields))
    }
}

impl JsonSchema for TransformFaceParams {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "TransformFaceParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        TransformFaceFields::json_schema(generator)
    }
}

#[cfg(test)]
#[path = "face_edits_tests.rs"]
mod tests;
