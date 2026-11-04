// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::parse_construction_operand_dual_transform;
use crate::design::decode::operands::parse_construction_operand_flag;
use crate::design::decode::operands::parse_construction_operand_path;
use crate::design::decode::operands::parse_construction_operand_transform;
use crate::design::decode::operands::parse_construction_tracking_path;
use crate::design::decode::operands::parse_loft_legacy_body_carrier;

use crate::records::decal::DesignRecordHeader;
use crate::test_support::indexed_header;
use crate::test_support::push_marked_reference;

#[test]
fn source_trailing_transforms_preserve_nonconsecutive_following_records() {
    use crate::records::topology::construction::DesignConstructionOperandTransform;
    use cadmpeg_ir::native::bytes::NativeBytes;

    for fixture in [
        crate::test_support::verification::text(
            "construction_transforms/translation.json",
            include_str!("../../../../test_support/construction_transforms/translation.json"),
        ),
        crate::test_support::verification::text(
            "construction_transforms/rotation.json",
            include_str!("../../../../test_support/construction_transforms/rotation.json"),
        ),
        crate::test_support::verification::text(
            "construction_transforms/identity.json",
            include_str!("../../../../test_support/construction_transforms/identity.json"),
        ),
        crate::test_support::verification::text(
            "construction_transforms/offset-three.json",
            include_str!("../../../../test_support/construction_transforms/offset-three.json"),
        ),
        crate::test_support::verification::text(
            "construction_transforms/offset-four.json",
            include_str!("../../../../test_support/construction_transforms/offset-four.json"),
        ),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(&fixture).unwrap();
        let raw: NativeBytes<Vec<u8>> =
            serde_json::from_value(fixture["source_bytes"].clone()).unwrap();
        let expected: DesignConstructionOperandTransform =
            serde_json::from_value(fixture["native"].clone()).unwrap();
        let mut bytes = vec![0; 17];
        bytes.extend_from_slice(&raw);
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:source-transform".into(),
            byte_offset: 17,
            record_index: expected.record_index(),
            class_tag: expected.class_tag.clone(),
        };
        let actual = parse_construction_operand_transform(&bytes, &header)
            .expect("source transform with an independent following index");
        assert_eq!(actual, expected);
        let wire = serde_json::to_value(&actual).unwrap();
        assert_eq!(
            serde_json::from_value::<DesignConstructionOperandTransform>(wire).unwrap(),
            actual
        );
        bytes[176..180].copy_from_slice(&header.record_index.to_le_bytes());
        assert!(parse_construction_operand_transform(&bytes, &header).is_none());
    }
}

#[test]
fn construction_operand_trailing_transform_has_exact_affine_frame() {
    let record_index = 300u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"339");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    let transform = [
        [0.0_f64, -1.0, 0.0, 12.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[1, 0]);
    let following_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"432");
    bytes.extend_from_slice(&(record_index + 1).to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("339".to_owned()).unwrap(),
        record_index,
    };

    let parsed = parse_construction_operand_transform(&bytes, &header)
        .expect("exact construction-operand transform");
    assert_eq!(parsed.transform, transform.try_into().unwrap());
    assert_eq!(parsed.transform_offset(), 22);
    assert_eq!(parsed.following_record_index(), 301);
    assert_eq!(parsed.following_byte_offset(), u64_from_index(following_at));
    assert_eq!(parsed.following_class_tag.as_str(), "432");

    bytes[150] = 0;
    assert!(parse_construction_operand_transform(&bytes, &header).is_none());

    let secondary = [
        [1.0_f64, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut dual = bytes[..21].to_vec();
    for value in transform.into_iter().flatten() {
        dual.extend_from_slice(&value.to_le_bytes());
    }
    for value in secondary.into_iter().flatten() {
        dual.extend_from_slice(&value.to_le_bytes());
    }
    dual.push(0);
    let dual_following_at = dual.len();
    dual.extend_from_slice(&3u32.to_le_bytes());
    dual.extend_from_slice(b"432");
    dual.extend_from_slice(&(record_index + 1).to_le_bytes());
    let parsed = parse_construction_operand_dual_transform(&dual, &header)
        .expect("exact dual construction-operand transform");
    assert_eq!(parsed.first_transform, transform.try_into().unwrap());
    assert_eq!(parsed.first_transform_offset, 21);
    assert_eq!(parsed.second_transform, secondary.try_into().unwrap());
    assert_eq!(parsed.second_transform_offset, 149);
    assert_eq!(dual_following_at, 278);
}

#[test]
fn construction_operand_trailing_flag_has_exact_compact_frame() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"374");
    bytes.extend_from_slice(&33602u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#33602".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("374".to_owned()).unwrap(),
        record_index: 33602,
    };

    let flag = parse_construction_operand_flag(&bytes, &header).expect("compact trailing flag");
    assert!(flag.value);
    assert_eq!(flag.value_offset, 22);

    bytes[22] = 2;
    assert!(parse_construction_operand_flag(&bytes, &header).is_none());
}

#[test]
fn construction_operand_auxiliary_paths_decode_transform_and_compact_frames() {
    let scope_record_index = 40u32;
    let record_index = 100u32;
    let transform = [
        [0.0_f64, -1.0, 0.0, 12.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut expanded = Vec::new();
    indexed_header(&mut expanded, *b"304", record_index);
    expanded.extend_from_slice(&[0; 10]);
    expanded.push(1);
    expanded.extend_from_slice(&174u64.to_le_bytes());
    expanded.extend_from_slice(&[0; 3]);
    for value in transform.into_iter().flatten() {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    expanded.push(0);
    push_marked_reference(&mut expanded, scope_record_index);
    push_marked_reference(&mut expanded, record_index + 2);
    expanded.extend_from_slice(&[0; 6]);
    let expanded_following_at = expanded.len();
    indexed_header(&mut expanded, *b"390", record_index + 1);
    let expanded_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("304".to_owned()).unwrap(),
        record_index,
    };
    let expanded = parse_construction_operand_path(&expanded, scope_record_index, &expanded_header)
        .expect("expanded selection path");
    assert_eq!(expanded.entity_ref, 174);
    assert_eq!(
        expanded.clone().into_draft().placement,
        crate::records::topology::construction::DesignConstructionPathPlacement::Transform(
            transform.try_into().unwrap()
        )
    );
    assert_eq!(expanded.scope_record_index_offset(), 163);
    assert_eq!(expanded.nested_record_index(), 102);
    assert_eq!(expanded.nested_record_index_offset(), 174);
    assert_eq!(expanded.following_record_index(), 101);
    assert_eq!(
        expanded.following_byte_offset(),
        u64_from_index(expanded_following_at)
    );

    let mut compact = Vec::new();
    indexed_header(&mut compact, *b"304", record_index);
    compact.extend_from_slice(&[0; 10]);
    compact.push(1);
    compact.extend_from_slice(&18_064u64.to_le_bytes());
    compact.extend_from_slice(&[0, 0, 1, 0]);
    push_marked_reference(&mut compact, scope_record_index);
    push_marked_reference(&mut compact, record_index + 2);
    compact.extend_from_slice(&[0; 6]);
    let compact_following_at = compact.len();
    indexed_header(&mut compact, *b"390", record_index + 1);
    let compact = parse_construction_operand_path(&compact, scope_record_index, &expanded_header)
        .expect("compact selection path");
    assert_eq!(compact.entity_ref, 18_064);
    assert_eq!(
        compact.clone().into_draft().placement,
        crate::records::topology::construction::DesignConstructionPathPlacement::Compact(true)
    );
    assert_eq!(compact.scope_record_index_offset(), 35);
    assert_eq!(compact.nested_record_index_offset(), 46);
    assert_eq!(
        compact.following_byte_offset(),
        u64_from_index(compact_following_at)
    );
}

#[test]
fn construction_operand_auxiliary_paths_decode_the_extended_prefix() {
    use crate::records::topology::construction::{
        DesignConstructionOperandPath, DesignConstructionPathPlacement,
    };
    fn extended_path(placement: &[u8], prefix_value: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"390", 3349);
        bytes.extend_from_slice(&[0; 28]);
        bytes.extend_from_slice(&prefix_value.to_le_bytes());
        bytes.extend_from_slice(&[0; 13]);
        bytes.push(1);
        bytes.extend_from_slice(&55u64.to_le_bytes());
        bytes.extend_from_slice(placement);
        push_marked_reference(&mut bytes, 2339);
        push_marked_reference(&mut bytes, 3351);
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(&mut bytes, *b"295", 3350);
        bytes
    }
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#3349".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("390".to_owned()).unwrap(),
        record_index: 3349,
    };
    let compact = parse_construction_operand_path(&extended_path(&[0, 0, 1, 0], 3), 2339, &header)
        .expect("extended compact path");
    assert_eq!(compact.entity_ref, 55);
    assert_eq!(compact.scope_record_index_offset(), 70);
    assert_eq!(compact.nested_record_index(), 3351);
    assert_eq!(compact.nested_record_index_offset(), 81);
    assert_eq!(compact.following_record_index(), 3350);
    assert_eq!(compact.following_byte_offset(), 97);

    let mut transform = vec![0; 3];
    for value in [
        1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -25.0, 0.0, 0.0, 1.0, 16.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        transform.extend_from_slice(&value.to_le_bytes());
    }
    transform.push(0);
    let transformed = parse_construction_operand_path(&extended_path(&transform, 3), 2339, &header)
        .expect("extended transform path");
    assert!(matches!(
        transformed.clone().into_draft().placement,
        DesignConstructionPathPlacement::Transform(_)
    ));
    assert_eq!(transformed.scope_record_index_offset(), 198);
    assert_eq!(transformed.following_byte_offset(), 225);

    let wire = serde_json::to_value(&transformed).unwrap();
    assert_eq!(wire["entity_ref_offset"], 57);
    assert_eq!(wire["transform_offset"], 68);
    let restored: DesignConstructionOperandPath = serde_json::from_value(wire).unwrap();
    assert_eq!(restored.following_byte_offset(), 225);

    assert!(
        parse_construction_operand_path(&extended_path(&[0, 0, 1, 0], 2), 2339, &header).is_none()
    );
}

#[test]
fn construction_tracking_path_decodes_absent_and_present_related_identities() {
    fn tracking_path(first: Option<u64>, second: Option<u64>) -> Vec<u8> {
        let wrapper_record_index = 300u32;
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"361", wrapper_record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(wrapper_record_index + 1).to_le_bytes());
        bytes.extend_from_slice(&[0; 3]);
        indexed_header(&mut bytes, *b"363", wrapper_record_index + 1);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&268u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        for identity in [first, second] {
            bytes.extend_from_slice(&u32::from(identity.is_some()).to_le_bytes());
            if let Some(identity) = identity {
                bytes.extend_from_slice(&identity.to_le_bytes());
            }
        }
        indexed_header(&mut bytes, *b"301", wrapper_record_index + 2);
        bytes
    }

    for (selector, kind) in [(-1i32, 3u32), (0, 3), (4, 3), (1, 0)] {
        let mut bytes = tracking_path(None, None);
        bytes[90..94].copy_from_slice(&selector.to_le_bytes());
        bytes[94..98].copy_from_slice(&kind.to_le_bytes());
        assert!(parse_construction_tracking_path(
            &bytes,
            0,
            300,
            &crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap(),
        )
        .is_none());
    }
    let mut wide_predecessor = tracking_path(None, None);
    drop(wide_predecessor.splice(20..20, [1, 0, 0, 0]));
    assert!(parse_construction_tracking_path(
        &wide_predecessor,
        0,
        300,
        &crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap(),
    )
    .is_none());

    let absent = tracking_path(None, None);
    let absent = parse_construction_tracking_path(
        &absent,
        0,
        300,
        &crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap(),
    )
    .expect("tracking path without related identities");
    assert_eq!(absent.carrier_record_index(), 301);
    assert_eq!(absent.carrier_byte_offset(), 33);
    assert_eq!(absent.primary_identity, 268);
    assert_eq!(absent.primary_identity_offset(), 70);
    assert_eq!(absent.selector, 1);
    assert_eq!(absent.kind, 3);
    assert_eq!(absent.first_related_identity(), None);
    assert_eq!(absent.second_related_identity(), None);
    assert_eq!(absent.following_record_index(), 302);
    assert_eq!(absent.following_byte_offset(), 114);

    let present = tracking_path(Some(113), Some(119));
    let present = parse_construction_tracking_path(
        &present,
        0,
        300,
        &crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap(),
    )
    .expect("tracking path with related identities");
    assert_eq!(
        present
            .first_related_identity()
            .map(|identity| identity.value),
        Some(113)
    );
    assert_eq!(
        present
            .first_related_identity()
            .map(|identity| identity.offset),
        Some(110)
    );
    assert_eq!(
        present
            .second_related_identity()
            .map(|identity| identity.value),
        Some(119)
    );
    assert_eq!(
        present
            .second_related_identity()
            .map(|identity| identity.offset),
        Some(122)
    );
    assert_eq!(present.following_byte_offset(), 130);
}

#[test]
fn legacy_loft_body_carriers_admit_only_the_class_keyed_frames() {
    fn reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }

    fn carrier(
        primary_class: &[u8; 3],
        paired_class: &[u8; 3],
        scope_record_index: u32,
        record_index: u32,
        with_scope_tail: bool,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *primary_class, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&scope_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut bytes, 900);
        bytes.extend_from_slice(&89u32.to_le_bytes());
        bytes.extend_from_slice(&1.25f64.to_le_bytes());
        bytes.extend_from_slice(&89u32.to_le_bytes());
        reference(&mut bytes, record_index + 2);
        bytes.extend_from_slice(&[0, 0]);
        reference(&mut bytes, record_index + 1);
        if with_scope_tail {
            bytes.push(0);
            reference(&mut bytes, scope_record_index);
        }
        indexed_header(&mut bytes, *paired_class, record_index);
        bytes
    }

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat",
        crate::records::feature::scope::DesignFeatureKind::Loft,
        12,
    );
    {
        let value = Some(
            crate::records::feature::path_features::DesignPathFeatureConstruction::Loft(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation: crate::records::feature::extrude::DesignExtrudeOperation::Cut,
                    operation_offset: 0,
                },
            ),
        );
        scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }

    let class_322 = carrier(b"322", b"262", 12, 100, false);
    let parsed_322 = parse_loft_legacy_body_carrier(
        &class_322,
        &scope,
        &crate::records::decal::DesignRecordHeader {
            id: "header-322".into(),
            record_index: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("322".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
    )
    .expect("class-322 legacy Loft carrier");
    assert_eq!(parsed_322.paired_class_tag.as_str(), "262");
    assert_eq!(parsed_322.paired_byte_offset, 87);
    assert_eq!(parsed_322.member, 900);
    assert_eq!(parsed_322.member_offset, 36);
    assert_eq!(parsed_322.opaque_index.get(), 89);
    assert_eq!(parsed_322.opaque_scalar.get(), 1.25);
    assert_eq!(parsed_322.next_next_record_index, 102);
    assert_eq!(parsed_322.next_record_index, 101);
    assert_eq!(
        parsed_322
            .trailing_scope_reference_offset
            .map(|_| parsed_322.scope_record_index),
        None
    );

    let class_322_tail = carrier(b"322", b"262", 12, 200, true);
    let parsed_322_tail = parse_loft_legacy_body_carrier(
        &class_322_tail,
        &scope,
        &crate::records::decal::DesignRecordHeader {
            id: "header-322-tail".into(),
            record_index: 200,
            class_tag: crate::records::references::DesignClassTag::try_from("322".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
    )
    .expect("class-322 legacy Loft carrier with scope tail");
    assert_eq!(parsed_322_tail.paired_class_tag.as_str(), "262");
    assert_eq!(parsed_322_tail.paired_byte_offset, 99);
    assert_eq!(
        parsed_322_tail
            .trailing_scope_reference_offset
            .map(|_| parsed_322_tail.scope_record_index),
        Some(12)
    );
    assert_eq!(parsed_322_tail.trailing_scope_reference_offset, Some(88));

    let class_411 = carrier(b"411", b"266", 12, 300, true);
    let parsed_411 = parse_loft_legacy_body_carrier(
        &class_411,
        &scope,
        &crate::records::decal::DesignRecordHeader {
            id: "header-411".into(),
            record_index: 300,
            class_tag: crate::records::references::DesignClassTag::try_from("411".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
    )
    .expect("class-411 legacy Loft carrier");
    assert_eq!(parsed_411.paired_class_tag.as_str(), "266");
    assert_eq!(parsed_411.paired_byte_offset, 99);
    assert_eq!(
        parsed_411
            .trailing_scope_reference_offset
            .map(|_| parsed_411.scope_record_index),
        Some(12)
    );
    assert_eq!(parsed_411.trailing_scope_reference_offset, Some(88));

    let mut wrong_presence = class_322.clone();
    wrong_presence[21] = 0;
    assert!(parse_loft_legacy_body_carrier(
        &wrong_presence,
        &scope,
        &crate::records::decal::DesignRecordHeader {
            id: "header-322".into(),
            record_index: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("322".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
    )
    .is_none());

    let wrong_pair = carrier(b"322", b"266", 12, 400, false);
    assert!(parse_loft_legacy_body_carrier(
        &wrong_pair,
        &scope,
        &crate::records::decal::DesignRecordHeader {
            id: "header-322".into(),
            record_index: 400,
            class_tag: crate::records::references::DesignClassTag::try_from("322".to_owned())
                .unwrap(),
            byte_offset: 0,
        },
    )
    .is_none());
}

#[test]
fn construction_tracking_path_admits_lane_two_and_three_only() {
    fn tracking_path(lane: u32) -> Vec<u8> {
        let wrapper_record_index = 300u32;
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"361", wrapper_record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(wrapper_record_index + 1).to_le_bytes());
        bytes.extend_from_slice(&[0; 3]);
        indexed_header(&mut bytes, *b"363", wrapper_record_index + 1);
        bytes.extend_from_slice(&[0; 10]);
        for word in [1u32, 0, 1, lane] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&158u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2i32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        for identity in [102u64, 102] {
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&identity.to_le_bytes());
        }
        indexed_header(&mut bytes, *b"301", wrapper_record_index + 2);
        bytes
    }
    let class = crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap();
    for lane in [2u32, 3] {
        let path = parse_construction_tracking_path(&tracking_path(lane), 0, 300, &class)
            .expect("admitted lane");
        assert_eq!(path.primary_identity, 158);
        assert_eq!((path.selector, path.kind), (2, 2));
        assert_eq!(path.first_related_identity().map(|id| id.value), Some(102));
        assert_eq!(path.second_related_identity().map(|id| id.value), Some(102));
    }
    for lane in [0u32, 1, 4] {
        assert!(parse_construction_tracking_path(&tracking_path(lane), 0, 300, &class).is_none());
    }
}
