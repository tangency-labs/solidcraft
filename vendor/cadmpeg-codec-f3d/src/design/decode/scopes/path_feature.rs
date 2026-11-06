// SPDX-License-Identifier: Apache-2.0
//! Exact path-feature construction scopes.

use super::fixed_parameters::{
    referenced_revolve_angle_owner, to_object_revolve_owner, unique_revolve_angle_owner,
};
use super::parameter_scope::parameter_scope_payload_length;
use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::class_403_revolve_scope_frame as class_403_revolve;
use crate::layout::compact_loft_operation_prefix as compact_loft;
use crate::layout::marked_to_object_revolve_prologue as marked_to_object;
use crate::layout::marker_one_revolve_prologue as revolve;
use crate::layout::referenced_angle_revolve_prologue as referenced_revolve;
use crate::layout::two_sided_revolve_prologue as two_sided_revolve;
use crate::layout::unmarked_referenced_angle_revolve_prologue as unmarked_revolve;
use crate::layout::unmarked_to_object_revolve_prologue as unmarked_to_object;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;

/// Byte length of a marked record reference: marker `1`, the u32 record
/// index, and six zero bytes.
const MARKED_REFERENCE_LEN: usize = 11;

/// Prologue spelling of a to-object `Revolve` scope.
#[derive(Clone, Copy)]
enum ToObjectPrologue {
    Marked,
    Unmarked,
}

/// A Symmetric or Two Sides fixed-angle `Revolve` scope. It has the frame
/// length and reference count of the 372-byte one-sided form, whose arm
/// refuses it, so this form is tried first.
fn two_sided_revolve_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignPathFeatureConstruction> {
    if !matches!(
        design_feature_family(&scope.kind()),
        Some(DesignFeatureFamily::Revolve)
    ) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let symmetric = match (
        bytes.get(start + two_sided_revolve::SYMMETRIC)?,
        bytes.get(start + two_sided_revolve::TWO_SIDES)?,
    ) {
        (1, 0) => true,
        (0, 1) => false,
        _ => return None,
    };
    if parameter_scope_payload_length(scope) != Some(372)
        || scope.reference_members().len() != 7
        || bytes.get(start + two_sided_revolve::MARKER) != Some(&1)
        || View::u32_le_at(bytes, start + two_sided_revolve::ZERO_VALUE) != Some(0)
        || View::u32_le_at(bytes, start + two_sided_revolve::EXTENT_KIND) != Some(2)
        || bytes.get(start + two_sided_revolve::DIRECTION_KIND) != Some(&0)
        || !matches!(
            bytes.get(start + two_sided_revolve::PROJECT_AXIS),
            Some(0 | 1)
        )
        || bytes.get(start + two_sided_revolve::SOLID) != Some(&1)
    {
        return None;
    }
    let opposite_record_index = marked_record_reference(
        bytes,
        start + two_sided_revolve::OPPOSITE_ANGLE_REFERENCE_MARKER,
    )?;
    let angle_record_index =
        marked_record_reference(bytes, start + two_sided_revolve::ANGLE_REFERENCE_MARKER)?;
    let angle = unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
    let opposite =
        unique_revolve_angle_owner(scope, parameter_owners, Some(opposite_record_index), 1)?;
    // Symmetric mirrors one angle; both owners then hold the same value.
    if symmetric && angle.evaluated_value() != opposite.evaluated_value() {
        return None;
    }
    let operation_offset = start + two_sided_revolve::OPERATION;
    Some(DesignPathFeatureConstruction::Revolve(
        path_features::DesignRevolveConstruction {
            operation: extrude_operation_at(bytes, operation_offset)?,
            operation_offset: u64::try_from(operation_offset).ok()?,
            travel: path_features::DesignRevolveTravel::Angle(
                cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
            ),
            angle_record_index,
            angle_offset: angle.evaluated_value_offset(),
            opposite_angle: None,
            second_side: Some(path_features::DesignRevolveSecondSide {
                symmetric,
                angle: cadmpeg_ir::scalar::PositiveAngle::new(opposite.evaluated_value().get())?,
                angle_record_index: opposite_record_index,
                angle_offset: opposite.evaluated_value_offset(),
            }),
            solid: None,
            reversed: false,
        },
    ))
}

/// A referenced-angle `Revolve` scope: the marked prologue with six ordered
/// references or the unmarked prologue with eight. Its angle owner can store a
/// negative angle.
fn referenced_angle_revolve_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
    start: usize,
) -> Option<path_features::DesignRevolveConstruction> {
    if design_feature_family(&scope.kind())? != DesignFeatureFamily::Revolve {
        return None;
    }
    let (operation, project_axis, solid, zero_flags, angle_reference) = match (
        parameter_scope_payload_length(scope)?,
        scope.reference_members().len(),
    ) {
        (351, 6)
            if bytes.get(start + referenced_revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + referenced_revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + referenced_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + referenced_revolve::DIRECTION_KIND) == Some(&0) =>
        {
            (
                referenced_revolve::OPERATION,
                referenced_revolve::PROJECT_AXIS,
                referenced_revolve::SOLID,
                referenced_revolve::ZERO_FLAGS,
                referenced_revolve::ANGLE_REFERENCE_MARKER,
            )
        }
        (369, 8)
            if bytes
                .get(start + unmarked_revolve::ZERO_RUN..start + unmarked_revolve::OPERATION)?
                == [0; 10]
                && View::u32_le_at(bytes, start + unmarked_revolve::EXTENT_KIND)
                    == Some(unmarked_revolve::EXTENT_KIND_VALUE)
                && bytes.get(start + unmarked_revolve::DIRECTION_KIND)
                    == Some(&unmarked_revolve::DIRECTION_KIND_VALUE) =>
        {
            (
                unmarked_revolve::OPERATION,
                unmarked_revolve::PROJECT_AXIS,
                unmarked_revolve::SOLID,
                unmarked_revolve::ZERO_FLAGS,
                unmarked_revolve::ANGLE_REFERENCE,
            )
        }
        _ => return None,
    };
    if !matches!(bytes.get(start + project_axis), Some(0 | 1))
        || bytes.get(start + zero_flags..start + angle_reference)? != [0; 3]
    {
        return None;
    }
    let solid = match bytes.get(start + solid)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let angle_record_index = marked_record_reference(bytes, start + angle_reference)?;
    let angle = referenced_revolve_angle_owner(scope, parameter_owners, angle_record_index)?;
    let value = angle.evaluated_value().get();
    Some(path_features::DesignRevolveConstruction {
        operation: extrude_operation_at(bytes, start + operation)?,
        operation_offset: u64::try_from(start + operation).ok()?,
        travel: path_features::DesignRevolveTravel::Angle(cadmpeg_ir::scalar::PositiveAngle::new(
            value.abs(),
        )?),
        angle_record_index,
        angle_offset: angle.evaluated_value_offset(),
        second_side: None,
        opposite_angle: None,
        solid: Some(solid),
        reversed: value < 0.0,
    })
}

/// A to-object `Revolve` scope: a marked or unmarked prologue with extent
/// kind `1`, a zero-valued owner, and one target group for each side.
fn to_object_revolve_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
    start: usize,
) -> Option<path_features::DesignRevolveConstruction> {
    if design_feature_family(&scope.kind())? != DesignFeatureFamily::Revolve {
        return None;
    }
    let (prologue, two_sided, boolean) = match (
        parameter_scope_payload_length(scope)?,
        scope.reference_members().len(),
    ) {
        (383, 8) => (ToObjectPrologue::Marked, false, false),
        (416, 10) => (ToObjectPrologue::Marked, false, true),
        (415, 10) => (ToObjectPrologue::Marked, true, false),
        (433, 12) => (ToObjectPrologue::Unmarked, true, true),
        _ => return None,
    };
    let (operation, project_axis, solid, zero_flags, owner_reference, axis_reference, side_two_at) =
        match prologue {
            ToObjectPrologue::Marked => {
                if bytes.get(start + marked_to_object::MARKER) != Some(&1)
                    || View::u32_le_at(bytes, start + marked_to_object::ZERO_VALUE) != Some(0)
                    || View::u32_le_at(bytes, start + marked_to_object::EXTENT_KIND)
                        != Some(marked_to_object::EXTENT_KIND_VALUE)
                    || bytes.get(start + marked_to_object::DIRECTION_KIND)
                        != Some(&marked_to_object::DIRECTION_KIND_VALUE)
                {
                    return None;
                }
                (
                    marked_to_object::OPERATION,
                    marked_to_object::PROJECT_AXIS,
                    marked_to_object::SOLID,
                    marked_to_object::ZERO_FLAGS,
                    marked_to_object::OWNER_REFERENCE,
                    marked_to_object::AXIS_GROUP_REFERENCE,
                    marked_to_object::SIDE_TWO_TARGET,
                )
            }
            ToObjectPrologue::Unmarked => {
                if bytes.get(
                    start + unmarked_to_object::ZERO_RUN..start + unmarked_to_object::OPERATION,
                )? != [0; 10]
                    || View::u32_le_at(bytes, start + unmarked_to_object::EXTENT_KIND)
                        != Some(unmarked_to_object::EXTENT_KIND_VALUE)
                    || bytes.get(start + unmarked_to_object::DIRECTION_KIND)
                        != Some(&unmarked_to_object::DIRECTION_KIND_VALUE)
                {
                    return None;
                }
                (
                    unmarked_to_object::OPERATION,
                    unmarked_to_object::PROJECT_AXIS,
                    unmarked_to_object::SOLID,
                    unmarked_to_object::ZERO_FLAGS,
                    unmarked_to_object::OWNER_REFERENCE,
                    unmarked_to_object::AXIS_GROUP_REFERENCE,
                    unmarked_to_object::SIDE_TWO_TARGET,
                )
            }
        };
    let result_operation = extrude_operation_at(bytes, start + operation)?;
    if (result_operation != DesignExtrudeOperation::NewBody) != boolean
        || !matches!(bytes.get(start + project_axis), Some(0 | 1))
        || bytes.get(start + zero_flags..start + owner_reference)? != [0; 3]
    {
        return None;
    }
    let solid = match bytes.get(start + solid)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let owner_record_index = marked_record_reference(bytes, start + owner_reference)?;
    let owner = to_object_revolve_owner(scope, parameter_owners, owner_record_index)?;
    let ordinal = |record_index: u32| {
        scope
            .reference_members()
            .values()
            .position(|value| *value == record_index)
    };
    ordinal(marked_record_reference(bytes, start + axis_reference)?)?;
    let side_two_at = start + side_two_at;
    let (side_two, final_reference_at) = if two_sided {
        let value = marked_record_reference(bytes, side_two_at)?;
        (
            Some(Located {
                value,
                offset: u64::try_from(side_two_at + 1).ok()?,
            }),
            side_two_at + MARKED_REFERENCE_LEN,
        )
    } else {
        if bytes.get(side_two_at) != Some(&0) {
            return None;
        }
        (None, side_two_at + 1)
    };
    if marked_record_reference(bytes, final_reference_at)
        != scope.reference_members().values().last().copied()
    {
        return None;
    }
    let side_one_at = final_reference_at + MARKED_REFERENCE_LEN;
    let side_one = Located {
        value: marked_record_reference(bytes, side_one_at)?,
        offset: u64::try_from(side_one_at + 1).ok()?,
    };
    let side_one_ordinal = ordinal(side_one.value)?;
    if side_two.is_some_and(|side_two| {
        ordinal(side_two.value).is_none_or(|side_two_ordinal| side_two_ordinal >= side_one_ordinal)
    }) {
        return None;
    }
    Some(path_features::DesignRevolveConstruction {
        operation: result_operation,
        operation_offset: u64::try_from(start + operation).ok()?,
        travel: path_features::DesignRevolveTravel::ToTargets(
            path_features::DesignRevolveTargets { side_one, side_two },
        ),
        angle_record_index: owner_record_index,
        angle_offset: owner.evaluated_value_offset(),
        second_side: None,
        opposite_angle: None,
        solid: Some(solid),
        reversed: false,
    })
}

pub(super) fn exact_path_feature_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignPathFeatureConstruction> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let operation = |offset| {
        Some(match View::u32_le_at(bytes, offset)? {
            1 => DesignExtrudeOperation::Join,
            2 => DesignExtrudeOperation::Cut,
            3 => DesignExtrudeOperation::Intersect,
            4 => DesignExtrudeOperation::NewBody,
            _ => return None,
        })
    };
    if let Some(construction) = two_sided_revolve_construction(bytes, scope, parameter_owners) {
        return Some(construction);
    }
    if let Some(construction) =
        referenced_angle_revolve_construction(bytes, scope, parameter_owners, start)
    {
        return Some(DesignPathFeatureConstruction::Revolve(construction));
    }
    if let Some(construction) =
        to_object_revolve_construction(bytes, scope, parameter_owners, start)
    {
        return Some(DesignPathFeatureConstruction::Revolve(construction));
    }
    match design_feature_family(&scope.kind())? {
        DesignFeatureFamily::Revolve
            if matches!(scope.reference_members().len(), 6 | 8)
                && bytes.get(start + revolve::MARKER) == Some(&1)
                && View::u32_le_at(bytes, start + revolve::ZERO_VALUE) == Some(0)
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
                && View::u32_le_at(bytes, start + revolve::STRUCTURAL_CONSTANT) == Some(1) =>
        {
            let angle = unique_revolve_angle_owner(scope, parameter_owners, None, 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                    travel: path_features::DesignRevolveTravel::Angle(
                        cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    ),
                    angle_record_index: angle.record_index(),
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
                    opposite_angle: None,
                    solid: None,
                    reversed: false,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if parameter_scope_payload_length(scope) == Some(372)
                && scope.reference_members().len() == 7
                && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0) =>
        {
            let mut lanes = scope
                .reference_members()
                .values()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index))
                        .then_some((*record_index, scalar))
                });
            let ((angle_record_index, angle), (opposite_angle_record_index, opposite)) =
                (lanes.next()?, lanes.next()?);
            if lanes.next().is_some() {
                return None;
            }
            if angle.ordinal != 0
                || opposite.ordinal != 1
                || angle.value.get() <= 0.0
                || opposite.value.get() != 0.0
            {
                return None;
            }
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + revolve::OPERATION).ok()?,
                    travel: path_features::DesignRevolveTravel::Angle(
                        cadmpeg_ir::scalar::PositiveAngle::new(angle.value.get())?,
                    ),
                    angle_record_index,
                    angle_offset: angle.value_offset,
                    second_side: None,
                    opposite_angle: Some(crate::records::identity::Located {
                        value: opposite_angle_record_index,
                        offset: opposite.value_offset,
                    }),
                    solid: None,
                    reversed: false,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag.as_str() == "407"
                && scope.paired_class_tag.as_str() == "258"
                && parameter_scope_payload_length(scope) == Some(363)
                && scope.reference_members().len() == 8
                && View::u32_le_at(bytes, start + 25) == Some(2)
                && bytes.get(start + 29) == Some(&0)
                && View::u32_le_at(bytes, start + 30) == Some(1)
                && bytes.get(start + 34) == Some(&1)
                && bytes.get(start + 43..start + 45) == Some(&[0; 2]) =>
        {
            let angle_record_index = u32::try_from(View::u64_le_at(bytes, start + 35)?).ok()?;
            if scope.reference_members().values().nth(6) != Some(&angle_record_index) {
                return None;
            }
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + 21)?,
                    operation_offset: u64::try_from(start + 21).ok()?,
                    travel: path_features::DesignRevolveTravel::Angle(
                        cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    ),
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
                    opposite_angle: None,
                    solid: None,
                    reversed: false,
                },
            ))
        }
        DesignFeatureFamily::Revolve
            if scope.class_tag.as_str() == "403"
                && scope.paired_class_tag.as_str() == "258"
                && parameter_scope_payload_length(scope) == Some(373)
                && scope.reference_members().len() == 8
                && View::u32_le_at(bytes, start + class_403_revolve::EXTENT_KIND) == Some(2)
                && bytes.get(start + class_403_revolve::DIRECTION_KIND..start + 31)
                    == Some(&[0, 1]) =>
        {
            let angle_record_index =
                marked_record_reference(bytes, start + class_403_revolve::ANGLE_REFERENCE_MARKER)?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + class_403_revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + class_403_revolve::OPERATION).ok()?,
                    travel: path_features::DesignRevolveTravel::Angle(
                        cadmpeg_ir::scalar::PositiveAngle::new(angle.evaluated_value().get())?,
                    ),
                    angle_record_index,
                    angle_offset: angle.evaluated_value_offset(),
                    second_side: None,
                    opposite_angle: None,
                    solid: None,
                    reversed: false,
                },
            ))
        }
        DesignFeatureFamily::Loft
            if bytes.get(start + compact_loft::ZERO_RUN_10..start + compact_loft::ONE_RUN_4)
                == Some(&[0; 10])
                && bytes.get(start + compact_loft::ONE_RUN_4..start + compact_loft::OPERATION)
                    == Some(&[1; 4])
                && bytes.get(start + compact_loft::ZERO_FLAG) == Some(&0)
                && View::u32_le_at(bytes, start + compact_loft::ALL_ONES) == Some(u32::MAX)
                && bytes.get(start + compact_loft::ZERO_RUN_11..start + compact_loft::LEN)
                    == Some(&[0; 11]) =>
        {
            Some(DesignPathFeatureConstruction::Loft(
                path_features::DesignLoftConstruction {
                    operation: operation(start + compact_loft::OPERATION)?,
                    operation_offset: u64::try_from(start + compact_loft::OPERATION).ok()?,
                },
            ))
        }
        DesignFeatureFamily::Loft
            if parameter_scope_payload_length(scope).is_some_and(|length| length >= 368) =>
        {
            Some(DesignPathFeatureConstruction::Loft(
                path_features::DesignLoftConstruction {
                    operation: operation(start + 29)?,
                    operation_offset: u64::try_from(start + 29).ok()?,
                },
            ))
        }
        DesignFeatureFamily::Sweep => {
            let candidate_lanes: Vec<_> = scope
                .reference_members()
                .values()
                .filter_map(|record_index| {
                    let scalar = exact_fixed_scalar(bytes, records, *record_index)?;
                    (scalar.owner_record_index == Some(scope.record_index))
                        .then_some((*record_index, scalar))
                })
                .collect();
            if candidate_lanes
                .iter()
                .enumerate()
                .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
            {
                return None;
            }
            match candidate_lanes.as_slice() {
                [l0, l1, l2, l3, l4, l5] => {
                    let lanes = [*l0, *l1, *l2, *l3, *l4, *l5];
                    Some(DesignPathFeatureConstruction::Sweep(
                        path_features::DesignSweepConstruction {
                            operation: operation(start + 25)?,
                            operation_offset: u64::try_from(start + 25).ok()?,
                            values: lanes.iter().map(|(_, scalar)| scalar.value).collect(),
                            record_indexes: lanes
                                .iter()
                                .map(|(record_index, _)| *record_index)
                                .collect(),
                            value_offsets: lanes
                                .iter()
                                .map(|(_, scalar)| scalar.value_offset)
                                .collect(),
                        },
                    ))
                }
                [l0, l1]
                    if scope.class_tag.as_str() == "372"
                        && scope.paired_class_tag.as_str() == "260"
                        && parameter_scope_payload_length(scope) == Some(423)
                        && scope.reference_members().len() == 9
                        && bytes.get(start + 11..start + 21) == Some(&[0; 10])
                        && bytes.get(start + 25) == Some(&1) =>
                {
                    let op_offset = start + 21;
                    let lanes = [*l0, *l1];
                    Some(DesignPathFeatureConstruction::Sweep(
                        path_features::DesignSweepConstruction {
                            operation: operation(op_offset)?,
                            operation_offset: u64::try_from(op_offset).ok()?,
                            values: lanes.iter().map(|(_, scalar)| scalar.value).collect(),
                            record_indexes: lanes
                                .iter()
                                .map(|(record_index, _)| *record_index)
                                .collect(),
                            value_offsets: lanes
                                .iter()
                                .map(|(_, scalar)| scalar.value_offset)
                                .collect(),
                        },
                    ))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
