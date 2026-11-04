// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::bind_lost_edge_groups;
use crate::design::decode::operands::parse_construction_operand_identity;
use crate::design::decode::operands::parse_entity_selection_operand;

use crate::design::decode::operands::parse_sketch_profile;
use crate::design::decode::operands::parse_sketch_profile_region_selection;

use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::DesignEntityHeader;
use crate::records::entity_header::DESIGN_MODULE_SKETCH;
use crate::records::references::LostEdgeReference;

use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::construction::DesignConstructionOperandIdentity;
use crate::records::topology::extrude_selection::DesignOperandRole;

use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;

/// One mutation of a sketch-profile tail.
type TailEdit<'a> = &'a dyn Fn(&mut Vec<u8>);

#[test]
fn sketch_profile_frame_resolves_its_decimal_entity_suffix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"308");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
    lp_utf16(&mut bytes, "172");
    let tail_at = bytes.len();
    bytes.extend_from_slice(&[0; 94]);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned()).unwrap(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,

        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(200),
                record_reference_offset: 1010,
                references: Vec::new(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };

    let profile = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        4,
        &header,
        std::slice::from_ref(&entity),
    )
    .expect("sketch-profile operand");
    assert_eq!(profile.scope_reference_ordinal, 4);
    assert_eq!(profile.entity_id.suffix(), 172);
    assert_eq!(profile.entity_id.as_str(), "0_172");
    assert_eq!(profile.paired_byte_offset(), u64_from_index(paired_at));

    bytes.truncate(paired_at - 94);
    bytes[4..7].copy_from_slice(b"319");
    let mut compact_tail = vec![0; 93];
    compact_tail[0] = 1;
    compact_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    compact_tail[12] = 1;
    compact_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    compact_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[53..57].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[57] = 1;
    compact_tail[58..62].copy_from_slice(&102u32.to_le_bytes());
    compact_tail[70] = 1;
    compact_tail[71..75].copy_from_slice(&101u32.to_le_bytes());
    compact_tail[82] = 1;
    compact_tail[83..87].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_tail);
    let compact_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let compact_header = DesignRecordHeader {
        class_tag: crate::records::references::DesignClassTag::try_from("319".to_owned()).unwrap(),
        ..header
    };
    let compact = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .expect("compact sketch-profile operand");
    assert_eq!(compact.scope_reference_ordinal, 2);
    assert_eq!(
        compact.paired_byte_offset(),
        u64_from_index(compact_paired_at)
    );

    bytes.truncate(tail_at);
    let mut omitted_ordinal_tail = vec![0; 89];
    omitted_ordinal_tail[0] = 1;
    omitted_ordinal_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    omitted_ordinal_tail[12] = 1;
    omitted_ordinal_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    omitted_ordinal_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    omitted_ordinal_tail[53] = 1;
    omitted_ordinal_tail[54..58].copy_from_slice(&102u32.to_le_bytes());
    omitted_ordinal_tail[66] = 1;
    omitted_ordinal_tail[67..71].copy_from_slice(&101u32.to_le_bytes());
    omitted_ordinal_tail[78] = 1;
    omitted_ordinal_tail[79..83].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&omitted_ordinal_tail);
    let omitted_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let omitted = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .expect("omitted-ordinal sketch-profile operand");
    assert_eq!(
        omitted.paired_byte_offset(),
        u64_from_index(omitted_paired_at)
    );
}

#[test]
fn sketch_profile_with_an_83_byte_tail_omits_the_third_reference() {
    let build = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"308");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&103u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
        lp_utf16(&mut bytes, "172");
        let mut tail = vec![0u8; 83];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[4..8].copy_from_slice(&1u32.to_le_bytes());
        tail[16..20].copy_from_slice(&0x41u32.to_le_bytes());
        for (at, record) in [(46, 102u32), (60, 101), (72, 777)] {
            tail[at] = 1;
            tail[at + 1..at + 5].copy_from_slice(&record.to_le_bytes());
        }
        edit(&mut tail);
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"258");
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes
    };
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned()).unwrap(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(200),
                record_reference_offset: 1010,
                references: Vec::new(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };
    let parse = |bytes: &[u8]| {
        parse_sketch_profile(
            bytes,
            "f3d:Design/BulkStream.dat",
            3,
            &header,
            std::slice::from_ref(&entity),
        )
    };
    let profile = parse(&build(&|_| {})).expect("83-byte sketch-profile operand");
    assert_eq!(profile.entity_id.suffix(), 172);
    // Each fixed field of the tail is required.
    let edits: [TailEdit<'_>; 7] = [
        &|tail| tail[0] = 2,
        &|tail| tail[4] = 0,
        &|tail| tail[9] = 1,
        &|tail| tail[16] = 0x42,
        &|tail| tail[47] = 0,
        &|tail| tail[61] = 0,
        &|tail| tail[71] = 1,
    ];
    for edit in edits {
        assert!(parse(&build(edit)).is_none());
    }
    // A tail of another length is refused.
    let mut longer = build(&|_| {});
    let paired_at = longer.len() - 11;
    longer.insert(paired_at, 0);
    assert!(parse(&longer).is_none());
}

#[test]
fn sketch_profile_resolves_a_leading_zero_entity_suffix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"308");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
    lp_utf16(&mut bytes, "00000000000000000000000000000172");
    bytes.extend_from_slice(&[0; 94]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("308".to_owned()).unwrap(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned()).unwrap(),
        class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(200),
                record_reference_offset: 1010,
                references: Vec::new(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    };
    let profile = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        4,
        &header,
        std::slice::from_ref(&entity),
    )
    .expect("profile with leading-zero suffix");
    assert_eq!(profile.entity_id.suffix(), 172);
}

#[test]
fn generated_base_flange_profile_frame_resolves() {
    let (bytes, _) = crate::test_support::streams_test::generated_design_base_flange_bulkstream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let profile_offset = records
        .offsets(1501)
        .first()
        .copied()
        .expect("generated BaseFlange profile");
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#1501".into(),
        byte_offset: u64_from_index(profile_offset),
        class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned()).unwrap(),
        record_index: 1501,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#800".into(),
        byte_offset: 0,

        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_800".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };
    let profile = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        1,
        &header,
        std::slice::from_ref(&entity),
    )
    .expect("generated BaseFlange profile operand");
    assert_eq!(profile.entity_id.as_str(), "Sketch_800");
    assert_eq!(profile.entity_id.suffix(), 800);
}

#[test]
fn extrude_operand_identity_walks_shared_wrapper_grammar_to_a_fixed_leaf() {
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1026,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 300,
                        offset: 1043,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 180,
                    opaque_index_offset: 1071,
                    opaque_scalar: 0.125,
                    opaque_scalar_offset: 1075,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB,
            role_offset: 1053,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1124,
        },
    )
    .unwrap();
    let wrapper_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("326".to_owned()).unwrap(),
        record_index: 300,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"326", 300);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    indexed_header(&mut bytes, *b"326", 305);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    indexed_header(&mut bytes, *b"324", 400);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    indexed_header(&mut bytes, *b"301", 900);

    let identity = parse_construction_operand_identity(&bytes, &group, &wrapper_header, None)
        .expect("identity chain");
    assert_eq!(
        identity
            .wrappers()
            .iter()
            .map(|wrapper| wrapper.record_index)
            .collect::<Vec<_>>(),
        [300, 305]
    );
    assert_eq!(
        identity
            .wrappers()
            .iter()
            .map(|wrapper| wrapper.byte_offset)
            .collect::<Vec<_>>(),
        [0, 24]
    );
    assert_eq!(identity.following_record_index(), 400);
    assert_eq!(identity.following_byte_offset(), 48);
    let persistent = identity
        .persistent_identity()
        .expect("fixed persistent identity leaf");
    assert_eq!(persistent.local_id, 586);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset(), 238);

    let mut expanded_bytes = bytes[..233].to_vec();
    expanded_bytes.extend_from_slice(&[0; 4]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&900u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&[0; 6]);
    indexed_header(&mut expanded_bytes, *b"301", 900);
    let expanded =
        parse_construction_operand_identity(&expanded_bytes, &group, &wrapper_header, None)
            .expect("identity chain with expanded tail reference");
    let persistent = expanded
        .persistent_identity()
        .expect("expanded persistent identity leaf");
    assert_eq!(persistent.tail_slot_offset(), 233);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset(), 248);

    let mut gapped = Vec::new();
    indexed_header(&mut gapped, *b"256", 291);
    gapped.extend_from_slice(&[0; 11]);
    for value in [
        1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        gapped.extend_from_slice(&value.to_le_bytes());
    }
    gapped.extend_from_slice(&[1, 0]);
    gapped.extend_from_slice(&bytes);
    gapped[156..159].copy_from_slice(b"257");
    let gapped_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#291".into(),
        byte_offset: 0,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 291,
    };
    let meta = crate::metastream::MetaStream {
        types: [
            ("2DF7DA30-A4E4-4260-AFD1-D9C7154CB44B", 291),
            ("11111111-2222-3333-4444-555555555555", 300),
        ]
        .into_iter()
        .map(
            |(guid, entity)| crate::records::entity_header::SegmentTypeData {
                byte_offset: 0,
                type_guid: guid.to_owned().try_into().unwrap(),
                type_guid_offset: 0,
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                    value: "872045B4-F653-4691-9752-ABC7684834BF"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 0,
                },
                version: 0,
                version_offset: 0,
                module: "EntityTracking".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![entity]),
            },
        )
        .collect(),
        records: vec![
            crate::metastream::RecordIndexEntry {
                entity_id: 291,
                bulk_offset: 0,
            },
            crate::metastream::RecordIndexEntry {
                entity_id: 300,
                bulk_offset: 152,
            },
        ],
        secondary_records: Vec::new(),
    };
    assert!(parse_construction_operand_identity(&gapped, &group, &gapped_header, None).is_none());
    let registry =
        super::super::tracking_registration::Registry::new(std::sync::Arc::new(meta.clone()));
    let gapped_identity =
        parse_construction_operand_identity(&gapped, &group, &gapped_header, Some(&registry))
            .expect("registered transform starts an identity chain");
    assert_eq!(gapped_identity.following_record_index(), 400);
    for mutation in 0..3 {
        let mut invalid = meta.clone();
        match mutation {
            0 => invalid.types[0].version = 1,
            1 => invalid.records[1].bulk_offset += 1,
            2 => {
                invalid.types[1].entities =
                    crate::records::identity::ReferenceRun::unlocated(vec![999]);
            }
            _ => unreachable!(),
        }
        let registry =
            super::super::tracking_registration::Registry::new(std::sync::Arc::new(invalid));
        assert!(parse_construction_operand_identity(
            &gapped,
            &group,
            &gapped_header,
            Some(&registry),
        )
        .is_none());
    }

    let mut bound_group = group;
    let mut terminating_identity = identity;
    terminating_identity.id =
        "f3d:Design/BulkStream.dat:design-construction-operand-identity#200".into();
    let mut draft = terminating_identity.into_draft();
    for wrapper in &mut draft.wrappers {
        wrapper.byte_offset += 200;
    }
    draft.following_byte_offset += 200;
    if let Some(persistent) = draft.persistent_identity.take() {
        let mut leaf = persistent.into_draft();
        leaf.local_id_offset += 200;
        leaf.asset_id_offset += 200;
        leaf.context_id_offset += 200;
        leaf.tail_slot_offset += 200;
        leaf.next_byte_offset += 200;
        draft.persistent_identity = Some(
            crate::records::topology::construction::DesignConstructionPersistentIdentity::try_new(
                leaf,
            )
            .unwrap(),
        );
    }
    terminating_identity = DesignConstructionOperandIdentity::try_new(draft).unwrap();
    bind_lost_edge_groups(
        std::slice::from_mut(&mut bound_group),
        std::slice::from_ref(&terminating_identity),
        &[LostEdgeReference::new(
            "f3d:Design/BulkStream.dat:lost-edge-reference#152".into(),
            152,
            "419".into(),
            299,
            "326".into(),
            300,
        )
        .expect("valid lost-edge record layout")],
    );
    assert_eq!(
        bound_group.lost_edge_references,
        ["f3d:Design/BulkStream.dat:lost-edge-reference#152"]
    );

    let lost_edge = |offset: u64, record_index: u32, next_class: &str, next_index: u32| {
        LostEdgeReference::new(
            format!("f3d:Design/BulkStream.dat:lost-edge-reference#{offset}"),
            offset,
            "419".into(),
            record_index,
            next_class.into(),
            next_index,
        )
        .expect("valid lost-edge record layout")
    };
    let two_record_run = [
        lost_edge(104, 298, "419", 299),
        lost_edge(152, 299, "326", 300),
    ];
    bind_lost_edge_groups(
        std::slice::from_mut(&mut bound_group),
        std::slice::from_ref(&terminating_identity),
        &two_record_run,
    );
    assert_eq!(bound_group.members().len(), 1);
    assert!(bound_group.lost_edge_references.is_empty());
}

#[test]
fn nested_entity_selection_member_retains_compact_and_expanded_identities() {
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 200,
                        offset: 943,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 971,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 975,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 953,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "265".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("333".to_owned()).unwrap(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&[1, 0, 0]);
    indexed_header(&mut bytes, *b"265", 100);
    indexed_header(&mut bytes, *b"301", 101);
    indexed_header(&mut bytes, *b"446", 102);
    let identity_at = bytes.len();
    indexed_header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"311", 104);

    let operand = parse_entity_selection_operand(&bytes, &group, 0, &record)
        .expect("nested entity-selection frame");
    assert_eq!(operand.primary_identity, 1331);
    assert_eq!(
        operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        Some(183)
    );
    assert_eq!(
        operand.clone().into_draft().identity_record_offset,
        u64_from_index(identity_at)
    );
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));

    for successor in [99u32, 114, u32::MAX] {
        let mut independent_successor = bytes[..next_at].to_vec();
        indexed_header(&mut independent_successor, *b"311", successor);
        let operand = parse_entity_selection_operand(&independent_successor, &group, 0, &record)
            .expect("expanded identity with independent successor");
        assert_eq!(operand.primary_identity, 1331);
        assert_eq!(operand.secondary().unwrap().identity.value, 183);
        assert_eq!(operand.next_record_index(), successor);
        assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));
        let wire = serde_json::to_vec(&operand).unwrap();
        let restored: crate::records::topology::entity_selection::DesignEntitySelectionOperand =
            serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored, operand);
    }

    let mut compact = bytes[..identity_at].to_vec();
    indexed_header(&mut compact, *b"429", 103);
    compact.extend_from_slice(&[0; 10]);
    compact.extend_from_slice(&1331u64.to_le_bytes());
    let compact_next_at = compact.len();
    indexed_header(&mut compact, *b"311", 109);
    let compact_operand = parse_entity_selection_operand(&compact, &group, 0, &record)
        .expect("compact nested entity-selection frame");
    assert_eq!(compact_operand.primary_identity, 1331);
    assert_eq!(
        compact_operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        None
    );
    assert_eq!(
        compact_operand.clone().into_draft().identity_record_offset,
        u64_from_index(identity_at)
    );
    assert_eq!(compact_operand.next_record_index(), 109);
    assert_eq!(
        compact_operand.next_byte_offset(),
        u64_from_index(compact_next_at)
    );

    let mut curve_identity = bytes[..identity_at].to_vec();
    indexed_header(&mut curve_identity, *b"429", 103);
    curve_identity.extend_from_slice(&[0; 10]);
    curve_identity.extend_from_slice(&77u64.to_le_bytes());
    curve_identity.extend_from_slice(&1331u64.to_le_bytes());
    curve_identity.extend_from_slice(&183u64.to_le_bytes());
    let curve_next_at = curve_identity.len();
    indexed_header(&mut curve_identity, *b"311", 104);
    let curve_operand = parse_entity_selection_operand(&curve_identity, &group, 0, &record)
        .expect("expanded Sketch-curve entity-selection frame");
    assert_eq!(curve_operand.primary_identity, 1331);
    assert_eq!(
        curve_operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        Some(183)
    );
    assert_eq!(
        curve_operand
            .secondary()
            .and_then(|secondary| secondary.curve_identity)
            .map(|identity| identity.value),
        Some(77)
    );
    assert_eq!(
        curve_operand
            .secondary()
            .and_then(|secondary| secondary.curve_identity)
            .map(|identity| identity.offset),
        Some(u64_from_index(identity_at) + 21)
    );
    assert_eq!(
        curve_operand.next_byte_offset(),
        u64_from_index(curve_next_at)
    );

    let mut class_338_curve_identity = bytes[..identity_at].to_vec();
    class_338_curve_identity[4..7].copy_from_slice(b"338");
    indexed_header(&mut class_338_curve_identity, *b"361", 103);
    class_338_curve_identity.extend_from_slice(&[0; 9]);
    class_338_curve_identity.push(1);
    class_338_curve_identity.extend_from_slice(&[0; 12]);
    class_338_curve_identity.extend_from_slice(&949u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&0u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&249u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&0u32.to_le_bytes());
    let class_338_next_at = class_338_curve_identity.len();
    indexed_header(&mut class_338_curve_identity, *b"268", 104);
    let class_338_record = DesignRecordHeader {
        class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned()).unwrap(),
        ..record
    };
    let class_338_operand =
        parse_entity_selection_operand(&class_338_curve_identity, &group, 0, &class_338_record)
            .expect("class-338 Sketch-curve entity-selection frame");
    assert_eq!(class_338_operand.primary_identity, 949);
    assert_eq!(
        class_338_operand
            .secondary()
            .map(|secondary| secondary.identity.value),
        Some(249)
    );
    assert_eq!(
        class_338_operand
            .secondary()
            .and_then(|secondary| secondary.curve_identity)
            .map(|identity| identity.value),
        None
    );
    assert_eq!(
        class_338_operand.primary_identity_offset(),
        u64_from_index(identity_at) + 33
    );
    assert_eq!(
        class_338_operand
            .secondary()
            .map(|secondary| secondary.identity.offset),
        Some(u64_from_index(identity_at) + 41)
    );
    assert_eq!(
        class_338_operand.next_byte_offset(),
        u64_from_index(class_338_next_at)
    );

    let mut invalid_class_338 = class_338_curve_identity.clone();
    invalid_class_338[identity_at + 20] = 0;
    assert!(
        parse_entity_selection_operand(&invalid_class_338, &group, 0, &class_338_record).is_none()
    );
}

fn region_member(bytes: &mut Vec<u8>, curve_primary_id: u32, incidence: [u32; 3]) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&curve_primary_id.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    for word in incidence {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
}

fn region_selection_frame() -> (Vec<u8>, usize, usize, usize) {
    let profile_record_index = 100;
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"266", profile_record_index);
    indexed_header(&mut bytes, *b"263", profile_record_index + 1);
    indexed_header(&mut bytes, *b"259", profile_record_index + 2);
    let selection_at = bytes.len();
    indexed_header(&mut bytes, *b"327", profile_record_index + 3);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&profile_record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    region_member(&mut bytes, 70, [1, 1, 1]);
    let second_region_marker = bytes.len();
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    region_member(&mut bytes, 80, [0, 1, 2]);
    region_member(&mut bytes, 81, [0, 2, 2]);
    let terminator = bytes.len();
    bytes.extend_from_slice(&[0; 5]);
    indexed_header(&mut bytes, *b"261", profile_record_index + 3);
    (bytes, selection_at, second_region_marker, terminator)
}

#[test]
fn sketch_profile_region_selection_preserves_region_and_curve_order() {
    let (bytes, selection_at, _, _) = region_selection_frame();
    let selection =
        parse_sketch_profile_region_selection(&bytes, 100, 0).expect("profile-region selection");
    assert_eq!(selection.record_index, 103);
    assert_eq!(selection.byte_offset, u64_from_index(selection_at));
    assert_eq!(selection.class_tag.as_str(), "327");
    assert_eq!(selection.companion_class_tag.as_str(), "261");
    assert_eq!(
        selection
            .regions
            .iter()
            .map(|region| {
                region
                    .members
                    .iter()
                    .map(|member| member.curve_primary_id.get())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [vec![70], vec![80, 81]]
    );
}

#[test]
fn sketch_profile_region_selection_requires_every_delimiter() {
    let (bytes, _, second_region_marker, terminator) = region_selection_frame();
    for offset in [second_region_marker, terminator] {
        let mut changed = bytes.clone();
        changed[offset] = 2;
        assert_eq!(
            parse_sketch_profile_region_selection(&changed, 100, 0),
            None
        );
    }
    let mut flag_1_terminator = bytes;
    flag_1_terminator[terminator] = 1;
    assert!(parse_sketch_profile_region_selection(&flag_1_terminator, 100, 0).is_some());
}

#[test]
fn sketch_profile_region_selection_derives_companion_after_header_shaped_member() {
    let (mut bytes, selection_at, _, _) = region_selection_frame();
    let curve_primary_id_offset = selection_at + 48;
    bytes[curve_primary_id_offset..curve_primary_id_offset + 4].copy_from_slice(b"123X");

    let selection =
        parse_sketch_profile_region_selection(&bytes, 100, 0).expect("profile-region selection");

    assert_eq!(
        u64::from(selection.regions[0].members[0].curve_primary_id.get()),
        u64::from(u32::from_le_bytes(*b"123X"))
    );
}

mod counted_frames;

#[test]
fn sketch_profile_region_selection_retains_partial_curve_incidence_words() {
    let (mut bytes, selection_at, _, _) = region_selection_frame();
    let first_member =
        selection_at + crate::layout::sketch_profile_region_selection_prefix::LEN + 4;
    let incidence = first_member
        + crate::layout::sketch_profile_region_member::ZERO_WORDS_3
        + std::mem::size_of::<[u32; 3]>();
    bytes[incidence + 4..incidence + 8].copy_from_slice(&11u32.to_le_bytes());
    bytes[incidence + 8..incidence + 12].copy_from_slice(&12u32.to_le_bytes());
    let selection = parse_sketch_profile_region_selection(&bytes, 100, 0).unwrap();
    let member = &selection.regions[0].members[0];
    assert_eq!(member.incidence_values.map(u32::from), [11, 12]);
    for offset in [incidence + 4, incidence + 8] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse_sketch_profile_region_selection(&invalid, 100, 0).is_none());
    }
}
