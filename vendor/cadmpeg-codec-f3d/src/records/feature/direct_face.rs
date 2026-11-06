// SPDX-License-Identifier: Apache-2.0
//! Direct face operations: offset faces, shell, thicken, move and draft.

use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_ir::scalar::{Angle, FiniteReal};
use serde::{Deserialize, Serialize};
cadmpeg_core::named_optional_field!(deserialize_draft_flags, DesignDraftFlags, "flags");

/// Exact fixed-form construction data of a direct-face feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "operation")]
pub(crate) enum DesignDirectFaceOperation {
    /// Signed normal offset applied to selected faces.
    OffsetFaces(DesignOffsetFacesOperation),
    /// Thin-wall shell applied after removing selected faces.
    Shell(DesignShellOperation),
    /// Signed normal thickness added from selected faces.
    Thicken(DesignThickenOperation),
}

/// Exact `OffsetFaces` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignOffsetFacesOperation {
    /// Signed distance in source centimetres.
    pub(crate) distance: FiniteReal,
    /// Referenced scalar record.
    pub(crate) distance_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) distance_offset: u64,
}

cadmpeg_core::named_optional_field!(deserialize_wall_slots, DesignShellWallSlots, "wall_slots");
cadmpeg_core::named_optional_field!(deserialize_outside_wall, DesignShellOutsideWall, "outside");

/// Exact `Shell` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellOperation {
    /// Wall thickness in source centimetres.
    pub(crate) thickness: cadmpeg_ir::scalar::PositiveReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
    /// Whether the wall grows outward from the original boundary.
    pub(crate) outward: bool,
    /// Byte offset of the outward Boolean or of the present wall slot.
    pub(crate) outward_offset: u64,
    /// Fields that only the wall-slot form stores.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_wall_slots"
    )]
    pub(crate) wall_slots: Option<DesignShellWallSlots>,
}

/// Fields of a `Shell` scope that stores its walls in optional slots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellWallSlots {
    /// Whether the selected faces extend along tangent faces.
    pub(crate) tangent_chain: bool,
    /// Whether offset corners are rounded instead of sharp.
    pub(crate) rounded_corners: bool,
    /// Outside wall of a scope that stores both walls.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_outside_wall"
    )]
    pub(crate) outside: Option<DesignShellOutsideWall>,
}

/// Outside wall of a `Shell` scope that stores both walls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignShellOutsideWall {
    /// Wall thickness in source centimetres.
    pub(crate) thickness: cadmpeg_ir::scalar::PositiveReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
}

/// Exact `Thicken` construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignThickenOperation {
    /// Signed thickness in source centimetres.
    pub(crate) signed_thickness: FiniteReal,
    /// Referenced scalar record.
    pub(crate) thickness_record_index: u32,
    /// Byte offset of the scalar.
    pub(crate) thickness_offset: u64,
}

/// Exact rigid transform carried by a Move feature scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignMoveOperation {
    /// Row-major model-space rigid transform in source centimetres.
    pub(crate) transform: SketchPlacementMatrix,
    /// Byte offset of the first matrix scalar.
    pub(crate) transform_offset: u64,
    /// Indexed record carrying `transform`.
    pub(crate) transform_record_index: u32,
    /// Source transform-form discriminator.
    pub(crate) form: DesignMoveForm,
    /// Byte offset of `form`.
    pub(crate) form_offset: u64,
}

impl DesignMoveOperation {
    pub(crate) fn preserves_orientation(&self) -> bool {
        use cadmpeg_ir::math::Vector3;
        let column = |index: usize| {
            Vector3::new(
                self.transform[0][index],
                self.transform[1][index],
                self.transform[2][index],
            )
        };
        column(0).cross(column(1)).dot(column(2)) > 0.0
    }
}

/// Source Move transform-form code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) enum DesignMoveForm {
    /// Source form 1.
    Form1,
    /// Source form 5.
    Form5,
}

impl TryFrom<u32> for DesignMoveForm {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Form1),
            5 => Ok(Self::Form5),
            _ => Err(format!("form must be 1 or 5, not {value}")),
        }
    }
}

impl From<DesignMoveForm> for u32 {
    fn from(form: DesignMoveForm) -> Self {
        match form {
            DesignMoveForm::Form1 => 1,
            DesignMoveForm::Form5 => 5,
        }
    }
}

/// Exact signed-angle lanes carried by a `Draft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignDraftOperation {
    /// Signed draft angle in radians.
    pub(crate) angle: Angle,
    /// Referenced draft-angle scalar record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the draft-angle scalar.
    pub(crate) angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record.
    pub(crate) opposite_angle_record_index: u32,
    /// Byte offset of the opposite-side angle scalar.
    pub(crate) opposite_angle_offset: u64,
    /// Flags of a scope whose prologue names both angle lanes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_draft_flags"
    )]
    pub(crate) flags: Option<DesignDraftFlags>,
}

/// Flags that the single-angle `Draft` prologue stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DesignDraftFlags {
    /// Whether the pull direction is opposite the neutral plane normal.
    pub(crate) flipped: bool,
    /// Whether the selected faces extend along tangent faces.
    pub(crate) tangent_chain: bool,
}

mod identity_rewrite;

#[cfg(test)]
mod tests {
    use super::{DesignShellOperation, DesignShellOutsideWall, DesignShellWallSlots};

    fn operation(outside: Option<DesignShellOutsideWall>) -> DesignShellOperation {
        DesignShellOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.1).unwrap(),
            thickness_record_index: 291,
            thickness_offset: 100,
            outward: false,
            outward_offset: 23,
            wall_slots: Some(DesignShellWallSlots {
                tangent_chain: true,
                rounded_corners: false,
                outside,
            }),
        }
    }

    #[test]
    fn wall_slots_are_present_or_their_keys_are_absent() {
        let both = operation(Some(DesignShellOutsideWall {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.3).unwrap(),
            thickness_record_index: 294,
            thickness_offset: 200,
        }));
        let one = operation(None);
        let without = DesignShellOperation {
            wall_slots: None,
            ..one.clone()
        };
        for value in [both, one, without] {
            let wire = serde_json::to_value(&value).unwrap();
            assert_eq!(
                serde_json::from_value::<DesignShellOperation>(wire).unwrap(),
                value
            );
        }
        let mut wire = serde_json::to_value(operation(None)).unwrap();
        assert!(wire["wall_slots"].get("outside").is_none());
        wire["wall_slots"]["outside"] = serde_json::Value::Null;
        let error = serde_json::from_value::<DesignShellOperation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("outside"), "{error}");

        let mut wire = serde_json::to_value(operation(None)).unwrap();
        wire["wall_slots"] = serde_json::Value::Null;
        let error = serde_json::from_value::<DesignShellOperation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("wall_slots"), "{error}");
    }
}
