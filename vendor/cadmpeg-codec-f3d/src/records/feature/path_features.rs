// SPDX-License-Identifier: Apache-2.0
//! Path features: revolve, loft, sweep and pipe constructions.

use super::extrude::DesignExtrudeOperation;
use super::work_geometry::DesignWorkPointConstruction;
use crate::records::identity::Located;
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::{FiniteReal, PositiveAngle};
use serde::{Deserialize, Serialize};

cadmpeg_core::named_optional_field!(
    deserialize_opposite_angle_offset,
    u64,
    "opposite_angle_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_opposite_angle_record_index,
    u32,
    "opposite_angle_record_index"
);
cadmpeg_core::named_optional_field!(
    deserialize_second_side,
    DesignRevolveSecondSideWire,
    "second_side"
);
cadmpeg_core::named_optional_field!(deserialize_solid, bool, "solid");
cadmpeg_core::named_optional_field!(deserialize_angle, f64, "angle");
cadmpeg_core::named_optional_field!(deserialize_targets, DesignRevolveTargets, "targets");
cadmpeg_core::named_optional_field!(deserialize_side_two, Located<u32>, "side_two");
/// Exact construction carried by a Revolve, Loft, Sweep, or Pipe scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DesignPathFeatureConstruction {
    /// Fixed-angle revolution result operation.
    Revolve(DesignRevolveConstruction),
    /// Loft result operation.
    Loft(DesignLoftConstruction),
    /// Sweep result operation and fixed dimension lanes.
    Sweep(DesignSweepConstruction),
    /// Native Pipe prefix and canonical scalar-owner identities.
    Pipe(DesignPipeConstruction),
}

/// Fixed construction of a `Revolve` scope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "DesignRevolveConstructionWire",
    into = "DesignRevolveConstructionWire"
)]
pub(crate) struct DesignRevolveConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Angular travel, or the target groups that end the travel.
    pub(crate) travel: DesignRevolveTravel,
    /// Referenced travel owner: the angular-travel scalar record, or the
    /// zero-valued owner of a to-object scope.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the travel owner's scalar.
    pub(crate) angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) opposite_angle: Option<Located<u32>>,
    /// Travel opposite the revolution direction, when the scope revolves to
    /// both sides of the profile plane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) second_side: Option<DesignRevolveSecondSide>,
    /// Solid flag of a referenced-angle form: `true` for a solid body and
    /// `false` for a surface body.
    pub(crate) solid: Option<bool>,
    /// Whether the stored angle is negative, so that the profile turns toward
    /// the negative normal of its sketch plane.
    pub(crate) reversed: bool,
}

/// Travel of a `Revolve` scope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DesignRevolveTravel {
    /// Positive angular travel in radians.
    Angle(PositiveAngle),
    /// Travel that ends at selected target groups.
    ToTargets(DesignRevolveTargets),
}

/// Target groups of a to-object `Revolve` scope. Each value is a role
/// `0x0000000500000000` construction-group record and each offset is the byte
/// offset of its marked record index.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignRevolveTargets {
    /// Side-one target group.
    pub(crate) side_one: Located<u32>,
    /// Side-two target group of a two-sided scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_side_two"
    )]
    pub(crate) side_two: Option<Located<u32>>,
}

/// Opposite-side travel of a Symmetric or Two Sides `Revolve` scope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesignRevolveSecondSide {
    /// Whether the scope mirrors the first side (Symmetric) rather than
    /// storing an independent second angle (Two Sides).
    pub(crate) symmetric: bool,
    /// Positive angular travel opposite the revolution direction, in radians.
    pub(crate) angle: PositiveAngle,
    /// Referenced opposite-side angle owner record.
    pub(crate) angle_record_index: u32,
    /// Byte offset of the opposite-side angular travel.
    pub(crate) angle_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignRevolveSecondSideWire {
    symmetric: bool,
    angle: f64,
    angle_record_index: u32,
    angle_offset: u64,
}

#[derive(Serialize, Deserialize)]
struct DesignRevolveConstructionWire {
    /// Boolean result operation.
    operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    operation_offset: u64,
    /// Positive angular travel in radians.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_angle"
    )]
    angle: Option<f64>,
    /// Target groups of a to-object scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_targets"
    )]
    targets: Option<DesignRevolveTargets>,
    /// Referenced travel-owner record.
    angle_record_index: u32,
    /// Byte offset of the travel owner's scalar.
    angle_offset: u64,
    /// Zero-valued opposite-side angle scalar record, when serialized.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle_record_index"
    )]
    opposite_angle_record_index: Option<u32>,
    /// Byte offset of the opposite-side angle scalar, when serialized.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_opposite_angle_offset"
    )]
    opposite_angle_offset: Option<u64>,
    /// Opposite-side travel of a Symmetric or Two Sides scope.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_second_side"
    )]
    second_side: Option<DesignRevolveSecondSideWire>,
    /// Solid flag of a referenced-angle form.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_solid"
    )]
    solid: Option<bool>,
    /// Whether the stored angle is negative.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    reversed: bool,
}

impl From<DesignRevolveConstruction> for DesignRevolveConstructionWire {
    fn from(value: DesignRevolveConstruction) -> Self {
        let (angle, targets) = match value.travel {
            DesignRevolveTravel::Angle(angle) => (Some(angle.get()), None),
            DesignRevolveTravel::ToTargets(targets) => (None, Some(targets)),
        };
        Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            angle,
            targets,
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle_record_index: value.opposite_angle.map(|located| located.value),
            opposite_angle_offset: value.opposite_angle.map(|located| located.offset),
            second_side: value.second_side.map(|side| DesignRevolveSecondSideWire {
                symmetric: side.symmetric,
                angle: side.angle.get(),
                angle_record_index: side.angle_record_index,
                angle_offset: side.angle_offset,
            }),
            solid: value.solid,
            reversed: value.reversed,
        }
    }
}

impl TryFrom<DesignRevolveConstructionWire> for DesignRevolveConstruction {
    type Error = String;
    fn try_from(value: DesignRevolveConstructionWire) -> Result<Self, Self::Error> {
        let travel = match (value.angle, value.targets) {
            (Some(angle), None) => DesignRevolveTravel::Angle(
                PositiveAngle::new(angle).ok_or("angle must be positive and finite")?,
            ),
            (None, Some(targets)) => {
                if value.opposite_angle_record_index.is_some()
                    || value.second_side.is_some()
                    || value.reversed
                {
                    return Err(
                        "a to-object Revolve has no opposite angle, second-side angle, or reversal"
                            .into(),
                    );
                }
                DesignRevolveTravel::ToTargets(targets)
            }
            _ => return Err("exactly one of angle and targets must occur".into()),
        };
        Ok(Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            travel,
            angle_record_index: value.angle_record_index,
            angle_offset: value.angle_offset,
            opposite_angle: match (
                value.opposite_angle_record_index,
                value.opposite_angle_offset,
            ) {
                (None, None) => None,
                (Some(value), Some(offset)) => Some(Located { value, offset }),
                _ => {
                    return Err(
                        "opposite_angle_record_index and opposite_angle_offset must occur together"
                            .into(),
                    )
                }
            },
            second_side: value
                .second_side
                .map(|side| {
                    Ok::<_, String>(DesignRevolveSecondSide {
                        symmetric: side.symmetric,
                        angle: PositiveAngle::new(side.angle)
                            .ok_or("second-side angle must be positive and finite")?,
                        angle_record_index: side.angle_record_index,
                        angle_offset: side.angle_offset,
                    })
                })
                .transpose()?,
            solid: value.solid,
            reversed: value.reversed,
        })
    }
}

/// Fixed construction of a `Loft` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignLoftConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
}

/// Fixed construction of a `Sweep` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesignSweepConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Scalar values in lane order: `AlongDistance` and `AgainstDistance`,
    /// followed by `AlongRailDistance`, `AgainstRailDistance`, `TwistAngle`, and
    /// `TaperAngle` when present.
    pub(crate) values: Vec<FiniteReal>,
    /// Referenced scalar records in lane order.
    pub(crate) record_indexes: Vec<u32>,
    /// Byte offsets of the scalar values in lane order.
    pub(crate) value_offsets: Vec<u64>,
}

/// Native construction of a `Pipe` scope.
///
/// The four scalar values live on the canonical parameter-owner records named
/// by `record_indexes`; the construction keeps only their identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DesignPipeConstructionWire")]
pub(crate) struct DesignPipeConstruction {
    /// Boolean result operation.
    pub(crate) operation: DesignExtrudeOperation,
    /// Byte offset of the operation u32.
    pub(crate) operation_offset: u64,
    /// Whether the section has an inward hollow wall.
    pub(crate) is_hollow: bool,
    /// Byte offset of the hollow Boolean.
    pub(crate) is_hollow_offset: u64,
    /// Source bytes following the result operation.
    pub(crate) opaque_post_operation: NativeBytes<[u8; 2]>,
    /// Byte offset of the source pair following the result operation.
    pub(crate) opaque_post_operation_offset: u64,
    /// Canonical scalar owners in forward-fraction, reverse-fraction,
    /// Section Size, and Section Thickness order.
    pub(crate) record_indexes: [u32; 4],
    /// Optional native point-selection pair.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) point_selection: Option<DesignPipePointSelection>,
}

/// Point-data record and its recipe-selection input named by a `Pipe` scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignPipePointSelection {
    /// Offset of the marked prefix reference that names the point-data record.
    pub(crate) prefix_reference_offset: u64,
    /// Complete point-data construction.
    pub(crate) point_data: DesignWorkPointConstruction,
}

cadmpeg_core::named_optional_field!(
    deserialize_pipe_point_selection,
    DesignPipePointSelection,
    "point_selection"
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignPipeConstructionWire {
    operation: DesignExtrudeOperation,
    operation_offset: u64,
    is_hollow: bool,
    is_hollow_offset: u64,
    opaque_post_operation: NativeBytes<[u8; 2]>,
    opaque_post_operation_offset: u64,
    record_indexes: [u32; 4],
    #[serde(default, deserialize_with = "deserialize_pipe_point_selection")]
    point_selection: Option<DesignPipePointSelection>,
}

impl TryFrom<DesignPipeConstructionWire> for DesignPipeConstruction {
    type Error = &'static str;

    fn try_from(value: DesignPipeConstructionWire) -> Result<Self, Self::Error> {
        if value.is_hollow_offset.checked_add(1) != Some(value.operation_offset)
            || value.operation_offset.checked_add(4) != Some(value.opaque_post_operation_offset)
            || value
                .record_indexes
                .iter()
                .enumerate()
                .any(|(ordinal, id)| *id == 0 || value.record_indexes[..ordinal].contains(id))
        {
            return Err("invalid Pipe prefix offsets or scalar identities");
        }
        Ok(Self {
            operation: value.operation,
            operation_offset: value.operation_offset,
            is_hollow: value.is_hollow,
            is_hollow_offset: value.is_hollow_offset,
            opaque_post_operation: value.opaque_post_operation,
            opaque_post_operation_offset: value.opaque_post_operation_offset,
            record_indexes: value.record_indexes,
            point_selection: value.point_selection,
        })
    }
}

impl DesignPipeConstruction {
    /// Whether the prefix fields sit at the compact or legacy offsets of a
    /// scope starting at `start`, before its reference table at `table`.
    pub(super) fn scope_offsets_are_valid(&self, start: u64, table: u64) -> bool {
        use crate::layout::{
            fixed_pipe_operation_prefix as compact, legacy_pipe_operation_prefix as legacy,
        };
        use cadmpeg_core::decode::u64_from_index;
        let compact = start.checked_add(u64_from_index(compact::IS_HOLLOW));
        let legacy = start.checked_add(u64_from_index(legacy::IS_HOLLOW));
        let prefix_end = self.opaque_post_operation_offset.checked_add(2);
        [compact, legacy].contains(&Some(self.is_hollow_offset))
            && self.is_hollow_offset.checked_add(1) == Some(self.operation_offset)
            && self.operation_offset.checked_add(4) == Some(self.opaque_post_operation_offset)
            && prefix_end.is_some_and(|end| end <= table)
            && self.point_selection.as_ref().is_none_or(|selection| {
                prefix_end.is_some_and(|end| selection.prefix_reference_offset > end)
                    && selection
                        .prefix_reference_offset
                        .checked_add(10)
                        .is_some_and(|end| end <= table)
            })
    }
}

mod identity_rewrite;
pub(crate) mod pipe;
