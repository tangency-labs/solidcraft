// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::draft::exact_draft_operation_with_owners;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::records::decal::DesignRecordHeader;
use crate::test_support::lp_utf16;

#[test]
fn parameter_scope_parses_named_variable_tail() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Draft");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, "draft-name");
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x4e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x4d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x4c);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("378".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("named variable-tail scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Draft
    );
    assert_eq!(scope.feature_ordinal.get(), 1);
    assert_eq!(scope.history_state_id(), Some(7));
    assert_eq!(scope.previous_history_state_id(), None);
    assert_eq!(scope.previous_history_state_id_offset(), None);
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55]
    );
    assert_eq!(scope.frame_length(), u64_from_index(paired_at));

    let mut owner_scope = scope.clone();
    owner_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![327, 330, 55, 56, 57, 58]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owners = vec![
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: "f3d:test:owner#327".into(),
                byte_offset: (111) - 40,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("272".to_owned())
                    .unwrap(),
                record_index: 327,
                scope_record_index: 12,
                local_ordinal: 0,
                evaluated_value: 0.0,
                evaluated_value_offset: 111,
                parameter_record_index: 326,
                owned_ordinal: 3,
                variant: Some(0),
                companion_record_index: 328,
            },
        )
        .unwrap(),
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: "f3d:test:owner#330".into(),
                byte_offset: (222) - 40,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("272".to_owned())
                    .unwrap(),
                record_index: 330,
                scope_record_index: 12,
                local_ordinal: 1,
                evaluated_value: 0.0,
                evaluated_value_offset: 222,
                parameter_record_index: 329,
                owned_ordinal: 4,
                variant: Some(0),
                companion_record_index: 331,
            },
        )
        .unwrap(),
    ];
    let operation = exact_draft_operation_with_owners(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &owner_scope,
        &owners,
    )
    .expect("owner-lane Draft operation");
    assert_eq!(operation.angle.get(), 0.0);
    assert_eq!(operation.angle_record_index, 327);
    assert_eq!(operation.opposite_angle_record_index, 330);
    assert_eq!(operation.angle_offset, 111);
    assert_eq!(operation.opposite_angle_offset, 222);
}
