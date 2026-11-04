// SPDX-License-Identifier: Apache-2.0
//! Parse solid primitive construction frames and their parameter owners.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::extrude_operation_at;
use super::shared_frames::marked_record_reference;
use crate::bytes::f64s_at;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_guid_end;
use crate::ids::native_stream;
use crate::layout::named_solid_primitive_prologue as solid_prologue;
use crate::layout::shifted_cylinder_primitive_352_frame as shifted_cylinder_352;
use crate::layout::shifted_cylinder_primitive_502_frame as shifted_cylinder_502;
use crate::layout::typed_reference_box_primitive_1295_frame as typed_box;
use crate::records::{
    feature::{
        extrude::DesignExtrudeOperation, primitives::DesignSolidPrimitive,
        scope::DesignParameterScope,
    },
    parameters::DesignParameterOwner,
    sketch_placement::valid_sketch_transform,
};
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::PositiveReal;

/// Type GUID of the support-face selection reference of the typed-reference box.
const TYPE_GUID_SUPPORT_FACE: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";
/// Type GUID of the placement-construction reference of the typed-reference box.
const TYPE_GUID_PLACEMENT: &str = "7B6C3C2D-4096-4E6D-B225-699E8C2D9355";
/// Type GUID of the five parameter-owner references of the typed-reference box.
const TYPE_GUID_OWNER: &str = "D91D429C-1BB5-4640-B49F-69AF14F463E9";
/// Type GUID of the result-record reference of the typed-reference box.
const TYPE_GUID_RESULT: &str = "CAA90BED-408E-4F03-A4E5-B1552D95C8EE";
/// Ordered scope-reference roles of the typed-reference box.
const TYPED_BOX_TABLE_GUIDS: [&str; 8] = [
    TYPE_GUID_SUPPORT_FACE,
    TYPE_GUID_PLACEMENT,
    TYPE_GUID_OWNER,
    TYPE_GUID_OWNER,
    TYPE_GUID_OWNER,
    TYPE_GUID_OWNER,
    TYPE_GUID_OWNER,
    TYPE_GUID_RESULT,
];
/// Ordered-reference index named by each of the seven prefix references.
const TYPED_BOX_PREFIX_MEMBERS: [usize; 7] = [7, 2, 3, 4, 1, 5, 6];
/// Largest absolute offset-owner value that counts as zero when the
/// typed-reference form composes its stored frame with the owner offsets.
const EPS_FRAME_OFFSET_ZERO: f64 = 0.0;

pub(super) fn exact_solid_primitive(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignSolidPrimitive> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let (operation, operation_offset, frame_transform) = match scope.kind_name() {
        "SpherePrimitive" | "TorusPrimitive" => {
            let operation_offset = start.checked_add(25)?;
            (
                extrude_operation_at(bytes, operation_offset)?,
                operation_offset,
                None,
            )
        }
        "BoxPrimitive" => {
            if let Some(operation_offset) = exact_named_solid_primitive_operation(bytes, start) {
                (
                    extrude_operation_at(bytes, operation_offset)?,
                    operation_offset,
                    None,
                )
            } else {
                let prologue = exact_typed_reference_box_prologue(bytes, scope, start)?;
                (
                    prologue.operation,
                    prologue.operation_offset,
                    Some(prologue.transform),
                )
            }
        }
        "CylinderPrimitive" => {
            if let Some(operation_offset) = exact_named_solid_primitive_operation(bytes, start) {
                (
                    extrude_operation_at(bytes, operation_offset)?,
                    operation_offset,
                    None,
                )
            } else {
                let prologue = exact_shifted_cylinder_primitive_prologue(bytes, scope, start)?;
                (
                    prologue.operation,
                    prologue.operation_offset,
                    prologue.transform,
                )
            }
        }
        _ => return None,
    };
    let matrix = |relative_offset: usize| {
        let matrix_at = start.checked_add(relative_offset)?;
        let values = f64s_at::<16>(bytes, matrix_at)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        Some((
            crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform).ok()?,
            u64_from_index(matrix_at),
        ))
    };
    match scope.kind_name() {
        "SpherePrimitive"
            if scope.frame_length() == 462
                && bytes.get(start + 29) == Some(&1)
                && bytes.get(start + 30) == Some(&1)
                && bytes.get(start + 41) == Some(&1)
                && bytes.get(start + 52) == Some(&1) =>
        {
            let diameter_record_index = View::u32_le_at(bytes, start + 42)?;
            let (diameter, diameter_offset) =
                exact_primitive_diameter(bytes, records, diameter_record_index)?;
            let (transform, transform_offset) = matrix(64)?;
            Some(DesignSolidPrimitive::Sphere(
                crate::records::feature::primitives::DesignSpherePrimitive {
                    transform,
                    transform_offset,
                    diameter,
                    diameter_record_index,
                    diameter_offset,
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            ))
        }
        "TorusPrimitive"
            if scope.frame_length() == 486
                && bytes.get(start + 29) == Some(&1)
                && bytes.get(start + 30) == Some(&1)
                && bytes.get(start + 41) == Some(&1)
                && bytes.get(start + 52) == Some(&1)
                && bytes.get(start + 63) == Some(&1) =>
        {
            let major_diameter_record_index = View::u32_le_at(bytes, start + 31)?;
            let minor_diameter_record_index = View::u32_le_at(bytes, start + 53)?;
            if major_diameter_record_index == minor_diameter_record_index {
                return None;
            }
            let (major_diameter, major_diameter_offset) =
                exact_primitive_diameter(bytes, records, major_diameter_record_index)?;
            let (minor_diameter, minor_diameter_offset) =
                exact_primitive_diameter(bytes, records, minor_diameter_record_index)?;
            let (transform, transform_offset) = matrix(75)?;
            Some(DesignSolidPrimitive::Torus(
                crate::records::feature::primitives::DesignTorusPrimitive {
                    transform,
                    transform_offset,
                    major_diameter,
                    major_diameter_record_index,
                    major_diameter_offset,
                    minor_diameter,
                    minor_diameter_record_index,
                    minor_diameter_offset,
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            ))
        }
        "BoxPrimitive" => {
            if scope.frame_length() < 78 || scope.reference_members().len() < 5 {
                return None;
            }
            let [Some(length), Some(width), Some(height), Some(offset_x), Some(offset_y)] =
                exact_owned_primitive_parameters::<5>(scope, parameter_owners)?
            else {
                return None;
            };
            if frame_transform.is_some() {
                // The stored frame is the box placement only when the offset
                // owners add no translation of their own.
                let members: Vec<u32> = scope.reference_members().values().copied().collect();
                let owners = [length, width, height, offset_x, offset_y];
                if offset_x.evaluated_value().get().abs() > EPS_FRAME_OFFSET_ZERO
                    || offset_y.evaluated_value().get().abs() > EPS_FRAME_OFFSET_ZERO
                    || owners
                        .iter()
                        .zip(members.get(2..7)?)
                        .any(|(owner, member)| owner.record_index() != *member)
                {
                    return None;
                }
            }
            let length_value = PositiveReal::new(length.evaluated_value().get())?;
            let width_value = PositiveReal::new(width.evaluated_value().get())?;
            let height_value = PositiveReal::new(height.evaluated_value().get())?;
            Some(DesignSolidPrimitive::Box(
                crate::records::feature::primitives::DesignBoxPrimitive {
                    length: length_value,
                    length_record_index: length.record_index(),
                    length_offset: length.evaluated_value_offset(),
                    width: width_value,
                    width_record_index: width.record_index(),
                    width_offset: width.evaluated_value_offset(),
                    height: height_value,
                    height_record_index: height.record_index(),
                    height_offset: height.evaluated_value_offset(),
                    offset_x: offset_x.evaluated_value(),
                    offset_x_record_index: offset_x.record_index(),
                    offset_x_offset: offset_x.evaluated_value_offset(),
                    offset_y: offset_y.evaluated_value(),
                    offset_y_record_index: offset_y.record_index(),
                    offset_y_offset: offset_y.evaluated_value_offset(),
                    transform: frame_transform.map(|frame| frame.value),
                    transform_offset: frame_transform.map(|frame| frame.offset),
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            ))
        }
        "CylinderPrimitive" => {
            if scope.frame_length() < 78 || scope.reference_members().len() < 2 {
                return None;
            }
            let [Some(height), Some(diameter)] =
                exact_owned_primitive_parameters::<2>(scope, parameter_owners)?
            else {
                return None;
            };
            let height_value = PositiveReal::new(height.evaluated_value().get())?;
            let diameter_value = PositiveReal::new(diameter.evaluated_value().get())?;
            Some(DesignSolidPrimitive::Cylinder(
                crate::records::feature::primitives::DesignCylinderPrimitive {
                    height: height_value,
                    height_record_index: height.record_index(),
                    height_offset: height.evaluated_value_offset(),
                    diameter: diameter_value,
                    diameter_record_index: diameter.record_index(),
                    diameter_offset: diameter.evaluated_value_offset(),
                    transform: frame_transform,
                    operation,
                    operation_offset: u64_from_index(operation_offset),
                },
            ))
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct ExactShiftedCylinderPrimitivePrologue {
    operation: DesignExtrudeOperation,
    operation_offset: usize,
    transform: Option<
        crate::records::identity::Located<crate::records::sketch_placement::SketchPlacementMatrix>,
    >,
}

fn exact_named_solid_primitive_operation(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start + solid_prologue::ZERO_RUN_9..start + solid_prologue::OPERATION)? != [0; 9]
        || bytes.get(start + solid_prologue::ZERO_FLAG) != Some(&0)
        || bytes.get(start + solid_prologue::FORM_MARKER) != Some(&1)
    {
        return None;
    }
    start.checked_add(solid_prologue::OPERATION)
}

#[derive(Clone, Copy)]
struct ExactTypedBoxPrologue {
    operation: DesignExtrudeOperation,
    operation_offset: usize,
    transform:
        crate::records::identity::Located<crate::records::sketch_placement::SketchPlacementMatrix>,
}

/// The typed-reference `BoxPrimitive` prologue: operation, seven prefix
/// references, rigid frame, null-GUID tail and the eight-entry typed
/// reference table, each checked against the scope's decoded references.
fn exact_typed_reference_box_prologue(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<ExactTypedBoxPrologue> {
    const TABLE_LEN: usize = 8;
    let stride = crate::bytes::TYPED_LOCAL_REFERENCE_LEN;
    let at = |relative_offset: usize| start.checked_add(relative_offset);
    let absolute = |relative_offset: usize| u64::try_from(start.checked_add(relative_offset)?).ok();
    if scope.frame_length() != u64::try_from(typed_box::LEN).ok()?
        || scope.paired_byte_offset() != absolute(typed_box::LEN)?
        || scope.reference_members().len() != TABLE_LEN
        || scope.reference_count_offset() != absolute(typed_box::REFERENCE_COUNT)?
        || scope.kind_offset() != absolute(typed_box::KIND)?
        || scope.feature_ordinal_offset() != absolute(typed_box::FEATURE_ORDINAL)?
        || scope.previous_history_state_id_offset()
            != Some(absolute(typed_box::PREVIOUS_HISTORY_STATE_ID)?)
        || bytes.get(at(typed_box::ZERO_RUN_10)?..at(typed_box::FORM_MARKER)?)? != [0; 10]
        || bytes.get(at(typed_box::FORM_MARKER)?) != Some(&1)
        || bytes.get(at(typed_box::NESTED_MARKER)?) != Some(&1)
        || bytes.get(at(typed_box::ZERO_BEFORE_MATRIX)?) != Some(&0)
        || View::u32_le_at(bytes, at(typed_box::ONE_COUNT)?)? != 1
        || bytes.get(at(typed_box::ZERO_RUN_8)?..at(typed_box::GUID_CODE_UNIT_COUNT)?)? != [0; 8]
        || View::u32_le_at(bytes, at(typed_box::REFERENCE_COUNT)?)?
            != u32::try_from(TABLE_LEN).ok()?
    {
        return None;
    }
    let operation_offset = at(typed_box::OPERATION)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    let members: Vec<u32> = scope.reference_members().values().copied().collect();
    for (ordinal, member) in members.iter().enumerate() {
        let entry = at(typed_box::REFERENCES)?.checked_add(ordinal.checked_mul(stride)?)?;
        if crate::bytes::typed_local_reference(bytes, entry)
            != Some((*member, *TYPED_BOX_TABLE_GUIDS.get(ordinal)?))
        {
            return None;
        }
    }
    for (ordinal, member_ordinal) in TYPED_BOX_PREFIX_MEMBERS.into_iter().enumerate() {
        let entry = at(typed_box::TYPED_REFERENCES)?.checked_add(ordinal.checked_mul(stride)?)?;
        if crate::bytes::typed_local_reference(bytes, entry)
            != Some((
                *members.get(member_ordinal)?,
                *TYPED_BOX_TABLE_GUIDS.get(member_ordinal)?,
            ))
        {
            return None;
        }
    }
    if crate::bytes::typed_local_reference(bytes, at(typed_box::TAIL_REFERENCE)?)
        .is_none_or(|(target, _)| target == 0)
        || !is_null_guid_utf16(bytes, at(typed_box::GUID_CODE_UNIT_COUNT)?)
        || bytes.get(at(typed_box::ZERO_RUN_3_AFTER_GUID)?..at(typed_box::REFERENCE_COUNT)?)?
            != [0; 3]
    {
        return None;
    }
    let values = f64s_at::<16>(bytes, at(typed_box::MATRIX)?)?;
    let mut transform = [[0.0; 4]; 4];
    for (ordinal, value) in values.into_iter().enumerate() {
        transform[ordinal / 4][ordinal % 4] = value;
    }
    if !valid_sketch_transform(&transform) || !right_handed(&transform) {
        return None;
    }
    Some(ExactTypedBoxPrologue {
        operation,
        operation_offset,
        transform: crate::records::identity::Located {
            value: crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
                .ok()?,
            offset: absolute(typed_box::MATRIX)?,
        },
    })
}

/// Whether the LP-UTF16 GUID at `count_at` is the 36-code-unit null GUID.
fn is_null_guid_utf16(bytes: &[u8], count_at: usize) -> bool {
    const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
    if View::u32_le_at(bytes, count_at) != Some(36) {
        return false;
    }
    let Some(text) = count_at
        .checked_add(4)
        .and_then(|from| bytes.get(from..from + 2 * NULL_GUID.len()))
    else {
        return false;
    };
    NULL_GUID
        .encode_utf16()
        .zip(text.chunks_exact(2))
        .all(|(unit, stored)| stored == unit.to_le_bytes())
}

/// Whether the first three columns of a rigid frame form a right-handed basis.
fn right_handed(transform: &[[f64; 4]; 4]) -> bool {
    let column = |index: usize| {
        [
            transform[0][index],
            transform[1][index],
            transform[2][index],
        ]
    };
    let (x, y, z) = (column(0), column(1), column(2));
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    cross[0] * z[0] + cross[1] * z[1] + cross[2] * z[2] > 0.0
}

fn exact_shifted_cylinder_primitive_prologue(
    bytes: &[u8],
    scope: &DesignParameterScope,
    start: usize,
) -> Option<ExactShiftedCylinderPrimitivePrologue> {
    let compact = match (
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
        scope.frame_length(),
    ) {
        ("297" | "375", "258", 352) => true,
        ("297" | "375", "258", 502) | ("414", "272", 502) => false,
        _ => return None,
    };
    let (
        frame_length,
        zero_run_10,
        form_marker,
        operation,
        first_reference,
        second_reference,
        third_reference,
        fourth_reference,
        reference_gap,
    ) = if compact {
        (
            shifted_cylinder_352::LEN,
            shifted_cylinder_352::ZERO_RUN_10,
            shifted_cylinder_352::FORM_MARKER,
            shifted_cylinder_352::OPERATION,
            shifted_cylinder_352::FIRST_REFERENCE,
            shifted_cylinder_352::SECOND_REFERENCE,
            shifted_cylinder_352::THIRD_REFERENCE,
            shifted_cylinder_352::FOURTH_REFERENCE,
            shifted_cylinder_352::REFERENCE_GAP,
        )
    } else {
        (
            shifted_cylinder_502::LEN,
            shifted_cylinder_502::ZERO_RUN_10,
            shifted_cylinder_502::FORM_MARKER,
            shifted_cylinder_502::OPERATION,
            shifted_cylinder_502::FIRST_REFERENCE,
            shifted_cylinder_502::SECOND_REFERENCE,
            shifted_cylinder_502::THIRD_REFERENCE,
            shifted_cylinder_502::FOURTH_REFERENCE,
            shifted_cylinder_502::REFERENCE_GAP,
        )
    };
    let reference_count = if compact { 5 } else { 7 };
    if scope.reference_members().len() != reference_count
        || scope.paired_byte_offset() != u64::try_from(start.checked_add(frame_length)?).ok()?
        || bytes.get(start + zero_run_10..start + form_marker)? != [0; 10]
        || bytes.get(start + form_marker) != Some(&1)
        || bytes.get(start + reference_gap) != Some(&0)
        || bytes.get(start + operation + 1..start + first_reference)? != [0; 3]
    {
        return None;
    }
    let operation_offset = start.checked_add(operation)?;
    let operation = extrude_operation_at(bytes, operation_offset)?;
    if bytes.get(start + first_reference) != Some(&1)
        || bytes.get(start + first_reference + 1) != Some(&1)
        || View::u32_le_at(bytes, start + first_reference + 2)?
            != *scope
                .reference_members()
                .values()
                .nth(reference_count - 1)?
        || bytes.get(start + first_reference + 6..start + first_reference + 11)? != [0; 5]
    {
        return None;
    }
    for (relative_offset, expected_record_index) in [
        (
            second_reference,
            *scope
                .reference_members()
                .values()
                .nth(reference_count - 2)?,
        ),
        (
            third_reference,
            *scope
                .reference_members()
                .values()
                .nth(reference_count - 3)?,
        ),
        (
            fourth_reference,
            *scope
                .reference_members()
                .values()
                .nth(reference_count - 4)?,
        ),
    ] {
        if marked_record_reference(bytes, start.checked_add(relative_offset)?)
            != Some(expected_record_index)
        {
            return None;
        }
    }
    let absolute = |relative_offset: usize| u64::try_from(start.checked_add(relative_offset)?).ok();
    let (
        reference_count_offset,
        kind_offset,
        feature_ordinal_offset,
        previous_history_state_id_offset,
    ) = match scope.frame_length() {
        352 => (
            shifted_cylinder_352::REFERENCE_COUNT,
            shifted_cylinder_352::KIND,
            shifted_cylinder_352::FEATURE_ORDINAL,
            shifted_cylinder_352::PREVIOUS_HISTORY_STATE_ID,
        ),
        502 => (
            shifted_cylinder_502::REFERENCE_COUNT,
            shifted_cylinder_502::KIND,
            shifted_cylinder_502::FEATURE_ORDINAL,
            shifted_cylinder_502::PREVIOUS_HISTORY_STATE_ID,
        ),
        _ => return None,
    };
    if scope.reference_count_offset() != absolute(reference_count_offset)?
        || scope.kind_offset() != absolute(kind_offset)?
        || scope.feature_ordinal_offset() != absolute(feature_ordinal_offset)?
        || scope.previous_history_state_id_offset()
            != Some(absolute(previous_history_state_id_offset)?)
    {
        return None;
    }
    let transform = match scope.frame_length() {
        352 => {
            if bytes.get(start + shifted_cylinder_352::COMPACT_TAIL_MARKER) != Some(&1)
                || View::u32_le_at(bytes, start + shifted_cylinder_352::COMPACT_TAIL_COUNT)? != 1
                || marked_record_reference(
                    bytes,
                    start + shifted_cylinder_352::COMPACT_TAIL_REFERENCE,
                )
                .is_none_or(|record_index| record_index == 0)
                || bytes.get(
                    start + shifted_cylinder_352::COMPACT_TAIL_ZERO_RUN_8
                        ..start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT,
                )? != [0; 8]
                || View::u32_le_at(bytes, start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT)? != 36
            {
                return None;
            }
            let guid_end =
                fixed_guid_end(bytes, start + shifted_cylinder_352::GUID_CODE_UNIT_COUNT)?;
            if guid_end != start + shifted_cylinder_352::ZERO_RUN_3_AFTER_GUID
                || bytes.get(
                    start + shifted_cylinder_352::ZERO_RUN_3_AFTER_GUID
                        ..start + shifted_cylinder_352::REFERENCE_COUNT,
                )? != [0; 3]
            {
                return None;
            }
            None
        }
        502 => {
            if bytes.get(start + shifted_cylinder_502::ZERO_BEFORE_MATRIX) != Some(&0)
                || bytes.get(
                    start + shifted_cylinder_502::ZERO_RUN_8_AFTER_MATRIX
                        ..start + shifted_cylinder_502::CONSTRUCTION_REFERENCE,
                )? != [0; 8]
                || bytes.get(start + shifted_cylinder_502::CONSTRUCTION_REFERENCE) != Some(&1)
                || View::u32_le_at(
                    bytes,
                    start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 1,
                )? != 0x0100_0000
                || View::u32_le_at(
                    bytes,
                    start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 5,
                )? != *scope.reference_members().values().next()?
                || bytes.get(
                    start + shifted_cylinder_502::CONSTRUCTION_REFERENCE + 9
                        ..start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT,
                )? != [0; 6]
                || View::u32_le_at(bytes, start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT)? != 36
                || bytes.get(
                    start + shifted_cylinder_502::ZERO_RUN_3_AFTER_GUID
                        ..start + shifted_cylinder_502::REFERENCE_COUNT,
                )? != [0; 3]
            {
                return None;
            }
            let values = f64s_at::<16>(bytes, start + shifted_cylinder_502::MATRIX)?;
            let mut transform = [[0.0; 4]; 4];
            for (ordinal, value) in values.into_iter().enumerate() {
                transform[ordinal / 4][ordinal % 4] = value;
            }
            let guid_end =
                fixed_guid_end(bytes, start + shifted_cylinder_502::GUID_CODE_UNIT_COUNT)?;
            if guid_end != start + shifted_cylinder_502::ZERO_RUN_3_AFTER_GUID
                || !valid_sketch_transform(&transform)
                || !cylinder_transform_preserves_projected_geometry(&transform)
            {
                return None;
            }
            Some(crate::records::identity::Located {
                value: crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
                    .ok()?,
                offset: u64::try_from(start + shifted_cylinder_502::MATRIX).ok()?,
            })
        }
        _ => return None,
    };
    Some(ExactShiftedCylinderPrimitivePrologue {
        operation,
        operation_offset,
        transform,
    })
}

fn cylinder_transform_preserves_projected_geometry(transform: &[[f64; 4]; 4]) -> bool {
    const EPS_CYLINDER_FRAME: f64 = 1.0e-10;
    transform[0][3].abs() <= EPS_CYLINDER_FRAME
        && transform[1][3].abs() <= EPS_CYLINDER_FRAME
        && transform[2][3].abs() <= EPS_CYLINDER_FRAME
        && transform[0][2].abs() <= EPS_CYLINDER_FRAME
        && transform[1][2].abs() <= EPS_CYLINDER_FRAME
        && (transform[2][2] - 1.0).abs() <= EPS_CYLINDER_FRAME
}

fn exact_owned_primitive_parameters<'a, const N: usize>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
) -> Option<[Option<&'a DesignParameterOwner>; N]> {
    let stream = native_stream(&scope.id)?;
    let mut owners = [None; N];
    for owner in parameter_owners.iter().filter(|owner| {
        owner.scope_record_index() == scope.record_index
            && native_stream(owner.id()) == Some(stream)
            && scope
                .reference_members()
                .values()
                .any(|value| value == &owner.record_index())
    }) {
        let ordinal = usize::try_from(owner.local_ordinal()).ok()?;
        let slot = owners.get_mut(ordinal)?;
        if slot.replace(owner).is_some() {
            return None;
        }
    }
    if owners.iter().any(Option::is_none) {
        return None;
    }
    Some(owners)
}

fn exact_primitive_diameter(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<(PositiveReal, u64)> {
    let scalar = exact_fixed_scalar(bytes, records, record_index)?;
    Some((PositiveReal::new(scalar.value.get())?, scalar.value_offset))
}

#[cfg(test)]
mod tests;
