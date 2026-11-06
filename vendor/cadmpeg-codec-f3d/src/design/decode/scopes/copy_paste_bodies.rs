// SPDX-License-Identifier: Apache-2.0
//! Exact copy-paste bodies operation scopes.

use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::body_ops;
use crate::records::feature::body_ops::DesignCopyPasteBodiesOperation;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;

pub(super) fn exact_copy_paste_bodies_operation(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignCopyPasteBodiesOperation> {
    if scope.kind() != scope::DesignFeatureKind::CopyPasteBodies
        || scope.reference_members().len() < 2
    {
        return None;
    }
    let body_count = scope.reference_members().len() - 1;
    let mut operands = Vec::new();
    let mut bodies = Vec::new();
    let parsed = (|| -> Option<DesignCopyPasteBodiesOperation> {
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let body_group_record_index = marked_record_reference(bytes, start + 29)?;
        let relation_record_index = marked_record_reference(bytes, start + 40)?;
        if *scope.reference_members().values().next()? != body_group_record_index {
            return None;
        }
        let search_at = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(1)?;
        let body_group_at = records.first_at_or_after(search_at, body_group_record_index)?;
        let (body_group_class_tag, body_group_after_tag) =
            lp_ascii_filtered_view(bytes, body_group_at, 3..=3, u8::is_ascii_digit)?;
        let body_group_after_index = body_group_after_tag.checked_add(4)?;
        if bytes.get(body_group_after_index..body_group_after_index + 10)? != [0; 10] {
            return None;
        }
        let body_group_count_at = body_group_after_index.checked_add(10)?;
        let body_group_count =
            usize::try_from(View::u32_le_at(bytes, body_group_count_at)?).ok()?;
        if body_group_count != scope.reference_members().len().checked_sub(1)? {
            return None;
        }
        let mut body_group_cursor = body_group_count_at.checked_add(4)?;
        for expected in scope.reference_members().values().skip(1) {
            let actual = marked_record_reference(bytes, body_group_cursor)?;
            if actual != *expected {
                return None;
            }
            operands.push(crate::records::identity::Located {
                value: actual,
                offset: u64::try_from(body_group_cursor + 1).ok()?,
            });
            body_group_cursor = body_group_cursor.checked_add(11)?;
        }
        let relation_at = records.first_at_or_after(search_at, relation_record_index)?;
        let (relation_class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, relation_at, 3..=3, u8::is_ascii_digit)?;
        let after_index = after_tag.checked_add(4)?;
        if bytes.get(after_index..after_index + 8)? != [0; 8] {
            return None;
        }
        let count_at = after_index.checked_add(8)?;
        if bytes.get(count_at) != Some(&1) {
            return None;
        }
        let reference_count = usize::try_from(View::u32_le_at(bytes, count_at + 1)?).ok()?;
        if reference_count != body_count.checked_mul(2)? {
            return None;
        }
        let references_at = count_at.checked_add(5)?;
        let body_reference = |at: usize, trailing_zeros: usize| {
            if bytes.get(at) != Some(&1)
                || !bytes
                    .get(at + 5..at + 5 + trailing_zeros)?
                    .iter()
                    .all(|byte| *byte == 0)
            {
                return None;
            }
            View::u32_le_at(bytes, at + 1)
        };
        for (ordinal, operand) in operands.into_iter().enumerate() {
            let source_at = references_at.checked_add(ordinal.checked_mul(30)?)?;
            let copied_at = source_at.checked_add(15)?;
            bodies.push(body_ops::DesignCopiedBody {
                operand,
                source: crate::records::identity::Located {
                    value: body_reference(source_at, 10)?,
                    offset: u64::try_from(source_at + 1).ok()?,
                },
                copied: crate::records::identity::Located {
                    value: body_reference(
                        copied_at,
                        if ordinal + 1 == body_count { 6 } else { 10 },
                    )?,
                    offset: u64::try_from(copied_at + 1).ok()?,
                },
            });
        }
        DesignCopyPasteBodiesOperation::try_new(
            bodies,
            body_ops::CopyPasteRecordLocation {
                record_index: body_group_record_index,
                class_tag: crate::design::decode::text::class_tag_from_view(body_group_class_tag)
                    .ok()?,
                byte_offset: u64::try_from(body_group_at).ok()?,
            },
            body_ops::CopyPasteRecordLocation {
                record_index: relation_record_index,
                class_tag: crate::design::decode::text::class_tag_from_view(relation_class_tag)
                    .ok()?,
                byte_offset: u64::try_from(relation_at).ok()?,
            },
        )
        .ok()
    })();
    parsed
}
