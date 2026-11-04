// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    decode_extrude_selection_groups, decode_extrude_selection_members,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::identity::ReferenceRun;
use crate::test_support::{indexed_header, push_marked_reference};

fn scope(stream: &str) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#40"),
        DesignFeatureKind::Emboss,
        40,
    );
    let construction = serde_json::from_value(serde_json::json!({
        "control_words": [7, 9], "native_pair": [17, 23],
        "depth_owner": {"value": 60, "offset": 29},
        "auxiliary_group": {"value": 72, "offset": 40},
        "horizontal_owner": {"value": 63, "offset": 51},
        "rotation_owner": {"value": 69, "offset": 82},
        "face_group": {"value": 48, "offset": 93},
        "vertical_owner": {"value": 66, "offset": 104},
        "profile_group": {"value": 41, "offset": 119}
    }))
    .unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 430;
            draft.paired_byte_offset = 430;
            draft.reference_count_offset = 235;
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 60, 63, 66, 69, 72]);
            draft.payload = DesignScopePayload::Emboss(Some(construction));
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn compact_group(
    bytes: &mut Vec<u8>,
    stream: &str,
    record: u32,
    member: u32,
) -> DesignRecordHeader {
    let offset = cadmpeg_core::decode::u64_from_index(bytes.len());
    indexed_header(bytes, *b"900", record);
    bytes.extend_from_slice(&[0; 10]);
    push_marked_reference(bytes, 40);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(bytes, member);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0.0f64.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(bytes, record + 2);
    bytes.extend_from_slice(&[1, 0, 0]);
    push_marked_reference(bytes, record + 1);
    bytes.push(0);
    push_marked_reference(bytes, 40);
    indexed_header(bytes, *b"901", record);
    DesignRecordHeader {
        id: format!("{stream}:record#{record}"),
        byte_offset: offset,
        class_tag: "900".to_owned().try_into().unwrap(),
        record_index: record,
    }
}

#[test]
fn emboss_auxiliary_counted_frame_uses_its_table_slot_without_asserting_contours() {
    let name = format!(
        "{}/Design1/BulkStream.dat",
        crate::manifest::GENERATED_DESIGN_ASSET_FOLDER
    );
    let stream = crate::ids::native_scope(&name);
    let scope = scope(&stream);
    let mut bytes = Vec::new();
    let headers = [
        compact_group(&mut bytes, &stream, 41, 44),
        compact_group(&mut bytes, &stream, 72, 80),
    ];
    let archive = crate::test_support::zip_test::f3d_with_configuration(&[], &name, &bytes);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let groups =
            decode_extrude_selection_groups(scan, std::slice::from_ref(&scope), &headers).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].record_index, 72);
        assert_eq!(groups[0].scope_reference_ordinal, 8);
        assert_eq!(groups[0].members()[0].value, 80);
        assert!(
            decode_extrude_selection_members(scan, &[scope], &groups, &headers)
                .unwrap()
                .is_empty()
        );
    });
}
