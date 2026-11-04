// SPDX-License-Identifier: Apache-2.0
//! Exact legacy thicken and shell scope frames.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::shell_wall_slot_scope_prefix as shell_slots;
use crate::layout::thicken_class_347_scope_frame as thicken_347;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;

const WALL_SLOT_LEN: usize = 11;
const WALL_SLOTS_ZERO_RUN: usize = 12;

/// Decode class-347/258 Thicken, whose group precedes its scalar. The class
/// pair and 291-byte frame are part of admission for this distinct grammar.
pub(super) fn exact_legacy_thicken_class_347(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag.as_str() != "347"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != u64::try_from(thicken_347::LEN).ok()?
        || scope.reference_members().len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + thicken_347::ZERO_RUN_10..start + thicken_347::FEATURE_FORM)
        != Some(&[0; 10])
        || View::u32_le_at(bytes, start + thicken_347::FEATURE_FORM)? != 4
        || View::u32_le_at(bytes, start + thicken_347::GROUP_FORM)? != 1
        || marked_record_reference(bytes, start + thicken_347::GROUP_REFERENCE)?
            != scope.reference_members().values().next().copied()?
        || bytes.get(start + thicken_347::SCALAR_PREFIX..start + thicken_347::SCALAR_REFERENCE)
            != Some(&[1, 1])
        || View::u32_le_at(bytes, start + thicken_347::AUXILIARY_COUNT)? != 1
        || marked_record_reference(bytes, start + thicken_347::AUXILIARY_REFERENCE).is_none()
        || bytes.get(start + thicken_347::ZERO_RUN_8..start + thicken_347::GUID_CODE_UNIT_COUNT)
            != Some(&[0; 8])
        || View::u32_le_at(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)? != 36
        || bytes.get(start + thicken_347::ZERO_RUN_3..start + thicken_347::REFERENCE_COUNT)
            != Some(&[0; 3])
        || View::u32_le_at(bytes, start + thicken_347::REFERENCE_COUNT)? != 3
        || View::u32_le_at(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT)? != 7
    {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)?;
    if guid_end != start + thicken_347::ZERO_RUN_3 {
        return None;
    }
    let kind_end =
        fixed_utf16_ascii_eq(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT, "Thicken")?;
    if kind_end != start + thicken_347::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        thicken_347::GROUP_REFERENCE_ENTRY,
        thicken_347::MEMBER_REFERENCE_ENTRY,
        thicken_347::SCALAR_REFERENCE_ENTRY,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members().values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index =
        marked_record_reference(bytes, start + thicken_347::SCALAR_REFERENCE)?;
    if scope.reference_members().values().next_back().copied()? != thickness_record_index {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    (scalar.value.get() != 0.0).then_some(DesignDirectFaceOperation::Thicken(
        direct_face::DesignThickenOperation {
            signed_thickness: scalar.value,
            thickness_record_index,
            thickness_offset: scalar.value_offset,
        },
    ))
}

/// Decode the `Shell` scope that stores its walls in optional slots.
///
/// Class tags are local to a stream and the frame length follows the wall and
/// face counts, so the admission rests on the frame grammar alone.
pub(super) fn exact_shell_wall_slots(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + shell_slots::ZERO_RUN_10..start + shell_slots::TANGENT_CHAIN)? != [0; 10] {
        return None;
    }
    let flag = |at: usize| match bytes.get(start + at) {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    };
    let tangent_chain = flag(shell_slots::TANGENT_CHAIN)?;
    let rounded_corners = flag(shell_slots::ROUNDED_CORNERS)?;
    let mut cursor = start + shell_slots::LEN;
    let mut wall_slot = || -> Option<Option<(u32, usize)>> {
        let at = cursor;
        match bytes.get(at)? {
            0 => {
                cursor = at.checked_add(1)?;
                Some(None)
            }
            1 => {
                let record = marked_record_reference(bytes, at)?;
                cursor = at.checked_add(WALL_SLOT_LEN)?;
                Some(Some((record, at)))
            }
            _ => None,
        }
    };
    let inside = wall_slot()?;
    let outside = wall_slot()?;
    let references = scope.reference_members();
    let table = references.values().copied().collect::<Vec<_>>();
    let walls = [inside, outside].into_iter().flatten().collect::<Vec<_>>();
    let scalars_at = table.len().checked_sub(walls.len())?;
    if scalars_at < 2
        || !walls
            .iter()
            .map(|(record, _)| *record)
            .eq(table[scalars_at..].iter().copied())
    {
        return None;
    }
    let group_count_at = cursor.checked_add(WALL_SLOTS_ZERO_RUN)?;
    if bytes.get(cursor..group_count_at)? != [0; WALL_SLOTS_ZERO_RUN]
        || View::u32_le_at(bytes, group_count_at)? != 1
    {
        return None;
    }
    let group_at = group_count_at.checked_add(4)?;
    if marked_record_reference(bytes, group_at)? != *table.first()? {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, group_at.checked_add(WALL_SLOT_LEN)?)?;
    let table_at = guid_end.checked_add(3)?;
    if bytes.get(guid_end..table_at)? != [0; 3]
        || u64::try_from(table_at).ok()? != scope.reference_count_offset()
    {
        return None;
    }
    let wall = |record: u32| {
        let scalar = exact_fixed_scalar(bytes, records, record)?;
        Some((
            cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
            record,
            scalar.value_offset,
        ))
    };
    let (first_record, first_at) = walls.first().copied()?;
    let (thickness, thickness_record_index, thickness_offset) = wall(first_record)?;
    let both = match (inside, outside) {
        (Some(_), Some((record, _))) => {
            let (thickness, thickness_record_index, thickness_offset) = wall(record)?;
            Some(direct_face::DesignShellOutsideWall {
                thickness,
                thickness_record_index,
                thickness_offset,
            })
        }
        _ => None,
    };
    Some(DesignDirectFaceOperation::Shell(
        direct_face::DesignShellOperation {
            thickness,
            thickness_record_index,
            thickness_offset,
            outward: inside.is_none(),
            outward_offset: u64::try_from(first_at).ok()?,
            wall_slots: Some(direct_face::DesignShellWallSlots {
                tangent_chain,
                rounded_corners,
                outside: both,
            }),
        },
    ))
}

pub(super) fn exact_shell_class_369_261(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag.as_str() != "369"
        || scope.paired_class_tag.as_str() != "261"
        || scope.frame_length() != u64_from_index(shell_369_261::LEN)
        || scope.reference_members().len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + shell_369_261::ZERO_RUN_9..start + shell_369_261::FEATURE_FORM)
        != Some(&[0; 9])
        || bytes.get(start + shell_369_261::FEATURE_FORM)
            != Some(&shell_369_261::FEATURE_FORM_VALUE)
        || bytes.get(start + shell_369_261::ZERO_RUN_3..start + shell_369_261::SCALAR_MARKER)
            != Some(&[0; 3])
        || bytes.get(start + shell_369_261::SCALAR_MARKER)
            != Some(&shell_369_261::SCALAR_MARKER_VALUE)
        || bytes
            .get(start + shell_369_261::ZERO_RUN_9_AFTER_SCALAR..start + shell_369_261::GROUP_FORM)
            != Some(&[0; 9])
        || bytes.get(start + shell_369_261::GROUP_FORM) != Some(&shell_369_261::GROUP_FORM_VALUE)
        || bytes.get(start + 47..start + shell_369_261::GROUP_REFERENCE) != Some(&[0; 3])
        || bytes.get(
            start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES
                ..start + shell_369_261::REFERENCE_COUNT,
        ) != Some(&[0; 3])
        || View::u32_le_at(bytes, start + shell_369_261::GUID_CODE_UNIT_COUNT)
            != Some(shell_369_261::GUID_CODE_UNIT_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::REFERENCE_COUNT)
            != Some(shell_369_261::REFERENCE_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT)
            != Some(shell_369_261::KIND_CODE_UNIT_COUNT_VALUE)
    {
        return None;
    }
    let outward = match bytes.get(start + shell_369_261::OUTWARD) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    let guid_end = fixed_utf16_ascii_eq(
        bytes,
        start + shell_369_261::GUID_CODE_UNIT_COUNT,
        "00000000-0000-0000-0000-000000000000",
    )?;
    if guid_end != start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES {
        return None;
    }
    let kind_end =
        fixed_utf16_ascii_eq(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT, "Shell")?;
    if kind_end != start + shell_369_261::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        shell_369_261::SCALAR_REFERENCE,
        shell_369_261::GROUP_REFERENCE,
        shell_369_261::REFERENCE_ENTRY_2,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members().values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index = scope.reference_members().values().next().copied()?;
    if marked_record_reference(bytes, start + shell_369_261::SCALAR_REFERENCE)
        != Some(thickness_record_index)
    {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    Some(DesignDirectFaceOperation::Shell(
        direct_face::DesignShellOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
            thickness_record_index,
            thickness_offset: scalar.value_offset,
            outward,
            outward_offset: u64::try_from(start + shell_369_261::OUTWARD).ok()?,
            wall_slots: None,
        },
    ))
}
