// SPDX-License-Identifier: Apache-2.0
//! Exact thicken and shell scope frames.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::shell_wall_slot_scope_prefix as shell_slots;
use crate::layout::thicken_scope_prologue as thicken;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::direct_face::DesignThickenOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;
use cadmpeg_ir::report::loss::LossNote;

const WALL_SLOT_LEN: usize = 11;
const MARKED_REFERENCE_LEN: usize = 11;
const U32_LEN: usize = 4;
const THICKEN_FORM: u32 = 4;
const GUID_ZERO_RUN: usize = 8;
const TABLE_ZERO_RUN: usize = 3;
const WALL_SLOTS_ZERO_RUN: usize = 12;

pub(super) fn exact_thicken_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    losses: &mut Vec<LossNote>,
) -> Option<DesignThickenOperation> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Thicken) {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + thicken::ZERO_RUN_9..start + thicken::PROLOGUE_MARKER)? != [0; 9] {
        return None;
    }
    let (mut cursor, prologue_flag) = match bytes.get(start + thicken::PROLOGUE_MARKER)? {
        0 => (start + thicken::LEN, None),
        1 if View::u32_le_at(bytes, start + thicken::LEN)? == 0 => {
            let short_end = start + thicken::LEN + U32_LEN;
            match bytes.get(short_end)? {
                _ if View::u32_le_at(bytes, short_end)? == THICKEN_FORM => (short_end, None),
                flag @ (0 | 1) => (short_end + 1, Some((short_end, *flag))),
                _ => return None,
            }
        }
        _ => return None,
    };
    if View::u32_le_at(bytes, cursor)? != THICKEN_FORM {
        return None;
    }
    let group_count = View::u32_le_at(bytes, cursor + U32_LEN)?;
    cursor += 2 * U32_LEN;
    let table = scope
        .reference_members()
        .values()
        .copied()
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for _ in 0..group_count {
        groups.push(marked_record_reference(bytes, cursor)?);
        cursor += MARKED_REFERENCE_LEN;
    }
    let flags_at = cursor;
    let flags = bytes.get(flags_at..flags_at + 2)?;
    if flags.iter().any(|flag| *flag > 1) {
        return None;
    }
    cursor += 2;
    let thickness_record_index = marked_record_reference(bytes, cursor)?;
    cursor += MARKED_REFERENCE_LEN;
    let operands = if table.first() == Some(&thickness_record_index) {
        table.get(1..)?
    } else if table.last() == Some(&thickness_record_index) {
        table.get(..table.len() - 1)?
    } else {
        return None;
    };
    if groups.first() != operands.first() || groups.is_empty() {
        return None;
    }
    let mut remaining = operands.iter();
    for group in &groups {
        remaining.by_ref().find(|operand| *operand == group)?;
    }
    if View::u32_le_at(bytes, cursor)? == 0 {
        cursor += U32_LEN;
    }
    if View::u32_le_at(bytes, cursor)? != 1 {
        return None;
    }
    marked_record_reference(bytes, cursor + U32_LEN)?;
    cursor += U32_LEN + MARKED_REFERENCE_LEN;
    if bytes.get(cursor..cursor + GUID_ZERO_RUN)? != [0; GUID_ZERO_RUN] {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, cursor + GUID_ZERO_RUN)?;
    if bytes.get(guid_end..guid_end + TABLE_ZERO_RUN)? != [0; TABLE_ZERO_RUN]
        || u64::try_from(guid_end + TABLE_ZERO_RUN).ok()? != scope.reference_count_offset()
    {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    if scalar.value.get() == 0.0 {
        return None;
    }
    let prologue = prologue_flag.map_or_else(String::new, |(at, flag)| {
        format!(" and prologue flag byte {flag:02x} at byte {at}")
    });
    losses.push(crate::loss::F3dLossCode::FeatureThickenFlagsAssumed.note(format!(
        "Thicken scope {} stores flag bytes {:02x} {:02x} at byte {flags_at}{prologue}; the Thicken settings these flags select are unresolved (DR-313), and a Thicken projected from the scope takes its side from the sign of the thickness.",
        scope.id, flags[0], flags[1]
    )));
    Some(DesignThickenOperation {
        signed_thickness: scalar.value,
        thickness_record_index,
        thickness_offset: scalar.value_offset,
    })
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
