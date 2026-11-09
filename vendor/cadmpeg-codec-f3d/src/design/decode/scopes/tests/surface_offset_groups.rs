// SPDX-License-Identifier: Apache-2.0
//! Grouped `SurfaceOffset` scopes whose support groups carry different ordinals.

use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceOffsetSupport;

/// Append a second support group, renumbered from `grouped`, and check that a
/// different ordinal adds the ordinal loss.
pub(super) fn assert_differing_group_ordinals_are_assumed(
    bytes: &[u8],
    grouped: &[u8],
    grouped_scope: &DesignParameterScope,
    [extend_distance_record_index, grouped_record_index, grouped_member_record_index]: [u32; 3],
) {
    let second_group_record_index = grouped_record_index + 10;
    let second_member_record_index = grouped_member_record_index + 10;
    let two_group_scope = {
        let mut scope = grouped_scope.clone();
        scope
            .try_edit(|draft| {
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                    extend_distance_record_index,
                    grouped_record_index,
                    grouped_member_record_index,
                    second_group_record_index,
                    second_member_record_index,
                ]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    for (second_ordinal, assumed) in [(227u32, false), (260, true)] {
        let mut second = grouped.to_vec();
        let mut at = 0;
        while at + 4 <= second.len() {
            let value = u32::from_le_bytes(second[at..at + 4].try_into().unwrap());
            let header = at >= 3 && matches!(&second[at - 3..at], b"282" | b"260" | b"288");
            let marked = at >= 1 && second[at - 1] == 1;
            if (header || marked)
                && (grouped_record_index..grouped_member_record_index + 5).contains(&value)
            {
                second[at..at + 4].copy_from_slice(&(value + 10).to_le_bytes());
                at += 4;
            } else {
                at += 1;
            }
        }
        second[71..75].copy_from_slice(&second_ordinal.to_le_bytes());
        second[83..87].copy_from_slice(&second_ordinal.to_le_bytes());
        let mut two_groups = bytes.to_vec();
        two_groups.extend_from_slice(&second);
        let (operation, losses) =
            super::fixed_kind_tail_operations::surface_offset_operation_and_losses(
                &two_groups,
                &crate::design::test_support::indexed_record_offsets_for_test(&two_groups),
                &two_group_scope,
            );
        assert_eq!(
            operation.map(|operation| operation.support),
            Some(DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices: vec![grouped_record_index, second_group_record_index],
            }),
            "{second_ordinal}"
        );
        assert_eq!(
            losses,
            if assumed {
                vec![crate::loss::F3dLossCode::FeatureSurfaceOffsetGroupOrdinalsAssumed.kind()]
            } else {
                Vec::new()
            },
            "{second_ordinal}"
        );
    }
}
