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
use crate::layout::compact_loft_operation_prefix as compact_loft;
use crate::layout::compact_sweep_prologue as compact_sweep;
use crate::layout::direction_pair_revolve_scope_frame as direction_pair_revolve;
use crate::layout::marked_to_object_revolve_prologue as marked_to_object;
use crate::layout::marker_one_revolve_prologue as revolve;
use crate::layout::referenced_angle_revolve_prologue as referenced_revolve;
use crate::layout::standard_loft_operation_prefix as standard_loft;
use crate::layout::three_flag_compact_loft_operation_prefix as three_flag_compact_loft;
use crate::layout::three_flag_standard_loft_operation_prefix as three_flag_standard_loft;
use crate::layout::two_sided_revolve_prologue as two_sided_revolve;
use crate::layout::unmarked_referenced_angle_revolve_prologue as unmarked_revolve;
use crate::layout::unmarked_to_object_revolve_prologue as unmarked_to_object;
use crate::layout::unmarked_u32_one_revolve_scope_frame as unmarked_u32_one;
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

/// A referenced-angle `Revolve` scope whose angle owner can store a negative angle.
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
        (length @ (351 | 373), count @ (6 | 8))
            if matches!(
                (
                    length,
                    count,
                    View::u32_le_at(bytes, start + referenced_revolve::OPERATION)
                ),
                (351, 6, Some(4)) | (373, 8, Some(1..=3))
            ) && bytes.get(start + referenced_revolve::MARKER) == Some(&1)
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

fn marker_one_revolve_prologue(bytes: &[u8], scope: &DesignParameterScope, start: usize) -> bool {
    matches!(scope.reference_members().len(), 6 | 8)
        && bytes.get(start + revolve::MARKER) == Some(&1)
        && View::u32_le_at(bytes, start + revolve::ZERO_VALUE) == Some(0)
        && View::u32_le_at(bytes, start + revolve::EXTENT_KIND) == Some(2)
        && bytes.get(start + revolve::DIRECTION_KIND) == Some(&0)
        && View::u32_le_at(bytes, start + revolve::STRUCTURAL_CONSTANT) == Some(1)
}

pub(super) fn marker_one_revolve_assumed_length(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<u64> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let length = parameter_scope_payload_length(scope)?;
    if matches!(
        design_feature_family(&scope.kind()),
        Some(DesignFeatureFamily::Revolve)
    ) && marker_one_revolve_prologue(bytes, scope, start)
        && !matches!(length, 345 | 355 | 367 | 351 | 373)
    {
        Some(length)
    } else {
        None
    }
}

pub(super) fn loft_reference_loss(
    scope: &DesignParameterScope,
    construction: &path_features::DesignLoftConstruction,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let target = construction.reference_target?;
    Some(crate::loss::F3dLossCode::FeatureLoftReferenceUnresolved.note(format!(
        "Loft scope {} stores a non-null marked reference with target {target} at prefix offset 33; the target and the Loft setting it selects are unresolved (DR-268), and a Loft projected from the scope uses closed false, solid true, ruled false, and linearize false.",
        scope.id
    )))
}

pub(super) fn three_flag_loft_settings_loss(
    bytes: &[u8],
    scope: &DesignParameterScope,
    construction: &path_features::DesignLoftConstruction,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let operation_offset = usize::try_from(construction.operation_offset).ok()?;
    let (form, flags) = match operation_offset.checked_sub(start)? {
        three_flag_compact_loft::OPERATION => ("compact", three_flag_compact_loft::FLAGS),
        three_flag_standard_loft::OPERATION => ("standard", three_flag_standard_loft::FLAGS),
        _ => return None,
    };
    let [first, second, third] = bytes.get(start + flags..start + flags + 3)? else {
        return None;
    };
    Some(crate::loss::F3dLossCode::FeatureLoftSettingsAssumed.note(format!(
        "Loft scope {} stores the three-flag {form} prefix with flag bytes {first:02x} {second:02x} {third:02x} at prefix offset {flags}; the Loft settings these flags select are unresolved (DR-294), and a Loft projected from the scope uses closed false, solid true, ruled false, and linearize false.",
        scope.id
    )))
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
            if marker_one_revolve_prologue(bytes, scope, start)
                && !matches!(parameter_scope_payload_length(scope), Some(351 | 373)) =>
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
            if parameter_scope_payload_length(scope) == Some(363)
                && scope.reference_members().len() == 8
                && bytes.get(
                    start + unmarked_u32_one::ZERO_RUN_10..start + unmarked_u32_one::OPERATION,
                ) == Some(&[0; 10])
                && View::u32_le_at(bytes, start + unmarked_u32_one::EXTENT_KIND) == Some(2)
                && bytes.get(start + unmarked_u32_one::DIRECTION_KIND) == Some(&0)
                && View::u32_le_at(bytes, start + unmarked_u32_one::U32_ONE) == Some(1)
                && bytes.get(start + unmarked_u32_one::ANGLE_REFERENCE_MARKER) == Some(&1)
                && bytes.get(
                    start + unmarked_u32_one::TRAILING_ZERO..start + unmarked_u32_one::LEN,
                ) == Some(&[0; 2]) =>
        {
            let angle_record_index = u32::try_from(View::u64_le_at(
                bytes,
                start + unmarked_u32_one::ANGLE_RECORD_INDEX,
            )?)
            .ok()?;
            if scope.reference_members().values().nth(6) != Some(&angle_record_index) {
                return None;
            }
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + unmarked_u32_one::OPERATION)?,
                    operation_offset: u64::try_from(start + unmarked_u32_one::OPERATION).ok()?,
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
            if parameter_scope_payload_length(scope) == Some(373)
                && scope.reference_members().len() == 8
                && bytes.get(
                    start + direction_pair_revolve::ZERO_RUN_10
                        ..start + direction_pair_revolve::OPERATION,
                ) == Some(&[0; 10])
                && View::u32_le_at(bytes, start + direction_pair_revolve::EXTENT_KIND)
                    == Some(2)
                && bytes.get(
                    start + direction_pair_revolve::DIRECTION_KIND
                        ..start + direction_pair_revolve::ZERO_RUN_3,
                ) == Some(&[0, 1])
                && bytes.get(
                    start + direction_pair_revolve::ZERO_RUN_3
                        ..start + direction_pair_revolve::ANGLE_REFERENCE_MARKER,
                ) == Some(&[0; 3]) =>
        {
            let angle_record_index = marked_record_reference(
                bytes,
                start + direction_pair_revolve::ANGLE_REFERENCE_MARKER,
            )?;
            let angle =
                unique_revolve_angle_owner(scope, parameter_owners, Some(angle_record_index), 0)?;
            Some(DesignPathFeatureConstruction::Revolve(
                path_features::DesignRevolveConstruction {
                    operation: operation(start + direction_pair_revolve::OPERATION)?,
                    operation_offset: u64::try_from(start + direction_pair_revolve::OPERATION)
                        .ok()?,
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
        DesignFeatureFamily::Loft => {
            let compact = bytes
                .get(start + compact_loft::ZERO_RUN_10..start + compact_loft::ONE_RUN_4)
                == Some(&[0; 10])
                && bytes.get(start + compact_loft::ONE_RUN_4..start + compact_loft::OPERATION)
                    == Some(&[1; 4])
                && bytes.get(start + compact_loft::ZERO_FLAG) == Some(&0)
                && View::u32_le_at(bytes, start + compact_loft::ALL_ONES) == Some(u32::MAX)
                && bytes.get(start + compact_loft::ZERO_RUN_11..start + compact_loft::LEN)
                    == Some(&[0; 11]);
            let compact_three = bytes.get(
                start + three_flag_compact_loft::ZERO_RUN_10
                    ..start + three_flag_compact_loft::FLAGS,
            ) == Some(&[0; 10])
                && matches!(
                    bytes.get(
                        start + three_flag_compact_loft::FLAGS
                            ..start + three_flag_compact_loft::OPERATION
                    ),
                    Some([1, 1, 0 | 1])
                )
                && bytes.get(start + three_flag_compact_loft::ZERO_FLAG) == Some(&0)
                && View::u32_le_at(bytes, start + three_flag_compact_loft::ALL_ONES)
                    == Some(u32::MAX)
                && bytes.get(
                    start + three_flag_compact_loft::ZERO_RUN_11
                        ..start + three_flag_compact_loft::LEN,
                ) == Some(&[0; 11]);
            let standard_prefix = bytes
                .get(start + standard_loft::ZERO_RUN_9..start + standard_loft::PRESENCE)
                == Some(&[0; 9])
                && bytes.get(start + standard_loft::PRESENCE) == Some(&1)
                && bytes.get(start + standard_loft::ZERO_RUN_4..start + standard_loft::FLAGS)
                    == Some(&[0; 4]);
            let reference = start + standard_loft::MARKED_REFERENCE;
            let standard = standard_prefix
                .then(|| match bytes.get(reference) {
                    Some(0) => Some(None),
                    Some(1) => View::u64_le_at(bytes, reference + 1)
                        .filter(|&target| {
                            target != 0 && bytes.get(reference + 9..reference + 11) == Some(&[0, 0])
                        })
                        .map(Some),
                    _ => None,
                })
                .flatten();
            let standard_three = bytes.get(
                start + three_flag_standard_loft::ZERO_RUN_9
                    ..start + three_flag_standard_loft::PRESENCE,
            ) == Some(&[0; 9])
                && bytes.get(start + three_flag_standard_loft::PRESENCE) == Some(&1)
                && bytes.get(
                    start + three_flag_standard_loft::ZERO_RUN_4
                        ..start + three_flag_standard_loft::FLAGS,
                ) == Some(&[0; 4])
                && bytes.get(start + three_flag_standard_loft::NULL_REFERENCE) == Some(&0);
            [
                compact.then_some((compact_loft::OPERATION, None)),
                compact_three.then_some((three_flag_compact_loft::OPERATION, None)),
                standard.map(|target| (standard_loft::OPERATION, target)),
                standard_three.then_some((three_flag_standard_loft::OPERATION, None)),
            ]
            .into_iter()
            .flatten()
            .find_map(|(offset, reference_target)| {
                Some(DesignPathFeatureConstruction::Loft(
                    path_features::DesignLoftConstruction {
                        operation: operation(start + offset)?,
                        operation_offset: u64::try_from(start + offset).ok()?,
                        reference_target,
                    },
                ))
            })
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
            let compact = bytes
                .get(start + compact_sweep::ZERO_RUN..start + compact_sweep::OPERATION)
                == Some(&[0; 10])
                && bytes.get(start + compact_sweep::MARKER) == Some(&1);
            match candidate_lanes.as_slice() {
                [l0, l1, l2, l3, l4, l5] => {
                    let lanes = [*l0, *l1, *l2, *l3, *l4, *l5];
                    let op_offset = start
                        + if compact {
                            compact_sweep::OPERATION
                        } else {
                            25
                        };
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
                [l0, l1]
                    if scope.class_tag.as_str() == "372"
                        && scope.paired_class_tag.as_str() == "260"
                        && parameter_scope_payload_length(scope) == Some(423)
                        && scope.reference_members().len() == 9
                        && compact =>
                {
                    let op_offset = start + compact_sweep::OPERATION;
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
