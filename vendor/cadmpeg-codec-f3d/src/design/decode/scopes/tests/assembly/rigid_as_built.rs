// SPDX-License-Identifier: Apache-2.0
//! The 257-byte rigid `As-built` scope: no reference table entries, no
//! parameter owners, and two wide operand-path locators.

use super::compact_wide::{push_header, push_lp_utf16, push_marked, push_wide_operand_path};

fn push_ordinary_tail(bytes: &mut Vec<u8>, additional_reference: bool, wide_field: bool) {
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 15]);
    push_marked(bytes, 80);
    if additional_reference {
        push_marked(bytes, 83);
    }
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&0.5_f64.to_le_bytes());
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    push_marked(bytes, 81);
    if wide_field {
        bytes.push(1);
    }
    bytes.extend_from_slice(&[0; 2]);
    push_marked(bytes, 82);
    bytes.extend_from_slice(&[0; 2]);
}

/// A 257-byte rigid as-built scope (record 10) whose operand
/// locators are records 64 and 67, then its paired header, the two wide
/// operand paths and the next header.
fn rigid_as_built_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_header(&mut bytes, b"405", 10);
    bytes.resize(22, 0);
    push_marked(&mut bytes, 70);
    bytes.resize(37, 0);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    push_marked(&mut bytes, 64);
    push_marked(&mut bytes, 67);
    push_marked(&mut bytes, 9);
    push_lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    // The empty reference table, then the history state and the kind.
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    push_lp_utf16(&mut bytes, "As-built");
    push_ordinary_tail(&mut bytes, false, false);
    assert_eq!(bytes.len(), 257);
    push_header(&mut bytes, b"262", 10);
    bytes.resize(257 + 30, 0);
    push_wide_operand_path(&mut bytes, 64, 10, "11111111-1111-1111-1111-111111111111");
    // The second operand names a different construction record.
    let second = bytes.len();
    push_wide_operand_path(&mut bytes, 67, 10, "66666666-6666-6666-6666-666666666666");
    bytes[second + 57..second + 61].copy_from_slice(&901_u32.to_le_bytes());
    push_header(&mut bytes, b"300", 71);
    bytes
}

#[test]
fn rigid_as_built_scope_reads_its_two_wide_operand_paths_without_owners() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    let bytes = rigid_as_built_stream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let class_tag = crate::records::references::DesignClassTag::try_from("405".to_owned()).unwrap();
    let mut scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes, &records, 10, &class_tag, 0,
    )
    .unwrap()
    .expect("a scope with an empty reference table");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::AsBuilt
    );
    assert_eq!(scope.frame_length(), 257);
    assert_eq!(scope.reference_members().len(), 0);
    // Class tags are per-file dynamic values: the same scope under other tags
    // is the same rigid form, and the kind and frame length identify it.
    let mut retagged = bytes.clone();
    retagged[4..7].copy_from_slice(b"328");
    retagged[257 + 4..257 + 7].copy_from_slice(b"264");
    let retagged_records = crate::design::decode::sketch::IndexedRecordOffsets::build(&retagged);
    let retagged_class =
        crate::records::references::DesignClassTag::try_from("328".to_owned()).unwrap();
    let retagged_scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &retagged,
        &retagged_records,
        10,
        &retagged_class,
        0,
    )
    .unwrap()
    .expect("the rigid form under other class tags");
    assert_eq!(retagged_scope.frame_length(), 257);
    assert_eq!(retagged_scope.reference_members().len(), 0);
    // The empty table is read only for the `As-built` kind.
    let mut other = bytes.clone();
    let kind_at = other
        .windows(4)
        .position(|window| window == [b'A', 0, b's', 0])
        .expect("the kind name");
    other[kind_at] = b'B';
    let other_records = crate::design::decode::sketch::IndexedRecordOffsets::build(&other);
    assert!(
        crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &other,
            &other_records,
            10,
            &class_tag,
            0,
        )
        .unwrap()
        .is_none()
    );
    scope.id = "f3d:Design/BulkStream.dat:design-parameter-scope#0".into();

    let locator = "2E3AC990-483D-4A55-8662-C064B976A957";
    let types = std::collections::HashMap::from([
        (64, (locator, 5)),
        (65, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
        (67, (locator, 5)),
        (68, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
    ]);
    let alignment = exact_assembly_alignment_with_types(&bytes, &records, &scope, &[], &types)
        .unwrap()
        .expect("rigid as-built alignment");
    assert_eq!(alignment.angle(), 0.0);
    assert_eq!(alignment.offset(), [0.0; 3]);
    let frames = alignment
        .operand_frames()
        .expect("identity frames at the flags");
    assert!(
        frames
            .iter()
            .all(|frame| frame.transform.rows()
                == cadmpeg_ir::transform::Transform::identity().rows())
    );
    let paths = alignment.operand_path_refs().expect("both operand paths");
    assert_eq!(
        paths.map(|path| path.occurrence_guids()[0].value.as_str().to_owned()),
        [
            "11111111-1111-1111-1111-111111111111".to_owned(),
            "66666666-6666-6666-6666-666666666666".to_owned()
        ]
    );
}

/// A 268-byte rigid group of three members (record 10, class tags 328/264)
/// whose locators are records 64, 67, and 73.
fn three_member_rigid_group_stream() -> Vec<u8> {
    let mut bytes = Vec::new();
    push_header(&mut bytes, b"328", 10);
    bytes.resize(22, 0);
    push_marked(&mut bytes, 70);
    bytes.resize(37, 0);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    push_marked(&mut bytes, 64);
    push_marked(&mut bytes, 67);
    push_marked(&mut bytes, 73);
    push_marked(&mut bytes, 9);
    push_lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    push_lp_utf16(&mut bytes, "As-built");
    push_ordinary_tail(&mut bytes, false, false);
    assert_eq!(bytes.len(), 268);
    push_header(&mut bytes, b"264", 10);
    bytes.resize(268 + 30, 0);
    push_wide_operand_path(&mut bytes, 64, 10, "11111111-1111-1111-1111-111111111111");
    for (locator, construction, guid) in [
        (67, 901_u32, "66666666-6666-6666-6666-666666666666"),
        (73, 902_u32, "77777777-7777-7777-7777-777777777777"),
    ] {
        let start = bytes.len();
        push_wide_operand_path(&mut bytes, locator, 10, guid);
        bytes[start + 57..start + 61].copy_from_slice(&construction.to_le_bytes());
    }
    push_header(&mut bytes, b"300", 77);
    bytes
}

#[test]
fn rigid_group_reads_every_member_after_its_first_two() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::{
        DesignAssemblyOperandQualifier, TYPED_OCCURRENCE_PATH_TYPE_GUID,
    };
    let bytes = three_member_rigid_group_stream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let class_tag = crate::records::references::DesignClassTag::try_from("328".to_owned()).unwrap();
    let mut scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes, &records, 10, &class_tag, 0,
    )
    .unwrap()
    .expect("a three-member rigid group");
    assert_eq!(scope.frame_length(), 268);
    assert_eq!(scope.reference_members().len(), 0);
    // The empty table's count sits 11 bytes past a two-member group's.
    let mut moved = scope.clone();
    assert!(moved
        .try_edit(|draft| draft.reference_count_offset = 153)
        .is_err());
    assert!(moved
        .try_edit(|draft| draft.reference_count_offset = 164)
        .is_ok());
    scope.id = "f3d:Design/BulkStream.dat:design-parameter-scope#0".into();

    let locator = "2E3AC990-483D-4A55-8662-C064B976A957";
    let types = [64, 67, 73]
        .into_iter()
        .flat_map(|index| {
            [
                (index, (locator, 5)),
                (index + 1, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
            ]
        })
        .collect::<std::collections::HashMap<_, _>>();
    let alignment = exact_assembly_alignment_with_types(&bytes, &records, &scope, &[], &types)
        .unwrap()
        .expect("rigid group alignment");
    let paths = alignment
        .operand_path_refs()
        .expect("the first two members");
    assert_eq!(
        paths.map(|path| path.occurrence_guids()[0].value.as_str().to_owned()),
        [
            "11111111-1111-1111-1111-111111111111".to_owned(),
            "66666666-6666-6666-6666-666666666666".to_owned()
        ]
    );
    let [third] = alignment.additional_members.as_slice() else {
        panic!("one member after the first two");
    };
    let DesignAssemblyOperandQualifier::OccurrencePath { path } = &third.qualifier else {
        panic!("an occurrence path");
    };
    assert_eq!(
        path.occurrence_guids()[0].value.as_str(),
        "77777777-7777-7777-7777-777777777777"
    );
    assert_eq!(
        third.frame.transform.rows(),
        cadmpeg_ir::transform::Transform::identity().rows()
    );

    // A locator count that disagrees with the frame length is no rigid group.
    let mut miscounted = bytes.clone();
    miscounted[37..41].copy_from_slice(&2_u32.to_le_bytes());
    let miscounted_records =
        crate::design::decode::sketch::IndexedRecordOffsets::build(&miscounted);
    assert!(
        crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &miscounted,
            &miscounted_records,
            10,
            &class_tag,
            0,
        )
        .unwrap()
        .is_none()
    );
}

fn typed_rigid_group_stream() -> Vec<u8> {
    fn reference(bytes: &mut Vec<u8>, target: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(b"11111111-2222-3333-4444-555555555555");
        bytes.extend_from_slice(&[0; 2]);
    }
    let ordinary = three_member_rigid_group_stream();
    let mut bytes = ordinary[..22].to_vec();
    reference(&mut bytes, 70);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    for index in [64, 67, 73, 9] {
        reference(&mut bytes, index);
    }
    push_lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    push_lp_utf16(&mut bytes, "As-built");
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 15]);
    reference(&mut bytes, 80);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_f64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    reference(&mut bytes, 81);
    bytes.extend_from_slice(&[0; 2]);
    reference(&mut bytes, 82);
    bytes.extend_from_slice(&[0; 2]);
    assert_eq!(bytes.len(), 435 + 51 * 3);
    bytes.extend_from_slice(&ordinary[268..]);
    bytes
}

#[test]
fn typed_rigid_group_closes_with_its_stored_count_and_reference_width() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    let bytes = typed_rigid_group_stream();
    let class = "328".to_owned().try_into().unwrap();
    let parse = |bytes: &[u8]| {
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(bytes);
        crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            bytes, &records, 10, &class, 0,
        )
        .unwrap()
    };
    let mut scope = parse(&bytes).unwrap();
    scope.id = "f3d:Design/BulkStream.dat:design-parameter-scope#0".into();
    assert_eq!(scope.frame_length(), 588);
    assert!(scope.reference_members().is_empty());
    let layout = crate::design::assembly::RigidGroupLayout::for_scope(&scope).unwrap();
    assert_eq!(
        (layout.members, layout.first_reference, layout.stride),
        (3, 81, 51)
    );
    let types = [64, 67, 73]
        .into_iter()
        .flat_map(|index| {
            [
                (index, ("2E3AC990-483D-4A55-8662-C064B976A957", 5)),
                (index + 1, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
            ]
        })
        .collect();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let alignment = exact_assembly_alignment_with_types(&bytes, &records, &scope, &[], &types)
        .unwrap()
        .unwrap();
    assert!(alignment.operand_path_refs().is_some());
    assert_eq!(alignment.additional_members.len(), 1);
    for at in [77, 81, 81 + 49, 588 - 196 + 19] {
        let mut bad = bytes.clone();
        bad[at] ^= 1;
        assert!(parse(&bad).is_none(), "mutation at {at}");
    }
    for length in [587, 589] {
        let mut bad = bytes.clone();
        if length == 587 {
            bad.remove(587);
        } else {
            bad.insert(588, 0);
        }
        assert!(parse(&bad).is_none());
    }
}

#[test]
fn rigid_group_tail_selects_between_equal_frame_lengths() {
    use crate::design::assembly::RigidGroupLayout;
    assert_eq!(RigidGroupLayout::new(741, 76).unwrap().members, 46);
    assert_eq!(RigidGroupLayout::new(741, 196).unwrap().members, 6);
    assert_eq!(RigidGroupLayout::new(537, 88).unwrap().members, 26);
    assert_eq!(RigidGroupLayout::new(537, 196).unwrap().members, 2);
    assert_eq!(RigidGroupLayout::new(269, 88).unwrap().members, 2);
    assert_eq!(RigidGroupLayout::new(269, 77).unwrap().members, 3);
    assert_eq!(RigidGroupLayout::new(279, 76).unwrap().members, 4);
    assert_eq!(RigidGroupLayout::new(258, 77).unwrap().members, 2);
    assert_eq!(RigidGroupLayout::new(268, 87).unwrap().members, 2);
}

#[test]
fn rigid_group_lengths_and_dynamic_classes_do_not_select_owner_generations() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    for members in [13_u32, 17, 19] {
        let mut bytes = rigid_as_built_stream();
        bytes.truncate(bytes.len() - 11);
        let mut references = Vec::new();
        for ordinal in 0..members - 2 {
            push_marked(&mut references, 73 + 3 * ordinal);
        }
        bytes.splice(63..63, references);
        bytes[4..7].copy_from_slice(b"283");
        bytes[37..41].copy_from_slice(&members.to_le_bytes());
        let length = 235 + 11 * usize::try_from(members).unwrap();
        bytes[length + 4..length + 7].copy_from_slice(b"264");
        let mut locators = vec![64, 67];
        for ordinal in 0..members - 2 {
            let locator = 73 + 3 * ordinal;
            locators.push(locator);
            push_wide_operand_path(
                &mut bytes,
                locator,
                10,
                "77777777-7777-7777-7777-777777777777",
            );
        }
        push_header(&mut bytes, b"300", 5000);
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
        let class = "283".to_owned().try_into().unwrap();
        let mut scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &bytes, &records, 10, &class, 0,
        )
        .unwrap()
        .unwrap();
        scope.id = "f3d:Design/BulkStream.dat:design-parameter-scope#0".into();
        assert_eq!(scope.frame_length(), u64::try_from(length).unwrap());
        assert!(!crate::design::assembly::placement_ordered_as_built_scope(
            &scope
        ));
        let types = locators
            .into_iter()
            .map(u64::from)
            .flat_map(|index| {
                [
                    (index, ("2E3AC990-483D-4A55-8662-C064B976A957", 5)),
                    (index + 1, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
                ]
            })
            .collect();
        let alignment = exact_assembly_alignment_with_types(&bytes, &records, &scope, &[], &types)
            .unwrap()
            .unwrap();
        assert_eq!(alignment.angle(), 0.0);
        assert_eq!(alignment.offset(), [0.0; 3]);
        assert!(alignment.owners.is_empty());
        assert!(alignment.additional_owner_run.is_none());
        assert_eq!(
            alignment.additional_members.len(),
            usize::try_from(members - 2).unwrap()
        );
    }
}

#[test]
fn ordinary_rigid_group_tail_checks_its_references_and_zero_fields() {
    use crate::layout::assembly_alternate_rigid_group_prefix as prefix;
    use crate::layout::assembly_rigid_as_built_scope as canonical;
    let parse = |bytes: &[u8]| {
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(bytes);
        let class = "405".to_owned().try_into().unwrap();
        crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            bytes, &records, 10, &class, 0,
        )
        .unwrap()
    };
    for ((tail_length, frame_length, references, zero_tail), alternate) in [
        (76, 257, vec![19, 50, 63], 74),
        (77, 258, vec![19, 50, 64], 75),
        (87, 268, vec![19, 30, 61, 74], 85),
        (88, 269, vec![19, 30, 61, 75], 86),
    ]
    .into_iter()
    .flat_map(|case| {
        [false, true]
            .into_iter()
            .map(move |alternate| (case.clone(), alternate))
    }) {
        let mut bytes = rigid_as_built_stream();
        let mut tail = Vec::new();
        push_ordinary_tail(
            &mut tail,
            tail_length >= 87,
            tail_length == 77 || tail_length == 88,
        );
        assert_eq!(tail.len(), tail_length);
        bytes.splice(canonical::SCOPE_TAIL..canonical::LEN, tail);
        let shift = if alternate {
            let mut prologue = vec![0; prefix::PATHLESS_LOCATOR_REFERENCE - prefix::ZERO_RUN_8];
            prologue[prefix::PROPERTY_BLOCK_PRESENCE - prefix::ZERO_RUN_8] =
                prefix::PROPERTY_BLOCK_PRESENCE_VALUE;
            bytes.splice(
                prefix::ZERO_RUN_8..canonical::PATHLESS_LOCATOR_REFERENCE,
                prologue,
            );
            prefix::PATHLESS_LOCATOR_REFERENCE - canonical::PATHLESS_LOCATOR_REFERENCE
        } else {
            0
        };
        let tail_at = canonical::SCOPE_TAIL + shift;
        let mut scope = parse(&bytes).unwrap();
        scope.id = "f3d:Design/BulkStream.dat:design-parameter-scope#0".into();
        assert_eq!(
            scope.frame_length(),
            frame_length + u64::try_from(shift).unwrap()
        );
        assert_eq!(
            scope.paired_byte_offset() - scope.feature_ordinal_offset(),
            u64::try_from(tail_length).unwrap()
        );
        assert_eq!(
            scope.previous_history_state_id_offset(),
            Some(u64::try_from(tail_at).unwrap() + if tail_length < 80 { 30 } else { 41 })
        );
        let layout = crate::design::assembly::RigidGroupLayout::for_scope(&scope).unwrap();
        assert_eq!(
            layout.count_at,
            if alternate {
                prefix::LOCATOR_COUNT
            } else {
                canonical::LOCATOR_COUNT
            }
        );
        assert_eq!(
            layout.first_reference,
            if alternate {
                prefix::FIRST_LOCATOR_REFERENCE
            } else {
                canonical::FIRST_LOCATOR_REFERENCE
            }
        );
        assert_eq!(layout.empty_table, canonical::REFERENCE_COUNT + shift);
        if alternate {
            assert!(scope
                .try_edit(|draft| draft.reference_count_offset -= u64::try_from(shift).unwrap())
                .is_err());
        }
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
        let types = [64, 67]
            .into_iter()
            .flat_map(|index| {
                [
                    (index, ("2E3AC990-483D-4A55-8662-C064B976A957", 5)),
                    (
                        index + 1,
                        (
                            crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID,
                            4,
                        ),
                    ),
                ]
            })
            .collect();
        let alignment =
            crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types(
                &bytes,
                &records,
                &scope,
                &[],
                &types,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            alignment
                .operand_path_refs()
                .unwrap()
                .map(|path| path.link().locator_record_index),
            [64, 67]
        );
        assert!(alignment.additional_members.is_empty());
        if tail_length == 77 || tail_length == 88 {
            let mut payload = bytes.clone();
            let field = tail_at + if tail_length == 77 { 61 } else { 72 };
            payload[field + 1..field + 3].copy_from_slice(&[1, 1]);
            assert!(parse(&payload).is_some());
        }
        if tail_length < 80 {
            let mut ambiguous_marker = bytes.clone();
            ambiguous_marker[tail_at + 30..tail_at + 34].copy_from_slice(&1_u32.to_le_bytes());
            ambiguous_marker[tail_at + 34..tail_at + 38].fill(0);
            ambiguous_marker[tail_at + 38..tail_at + 41].fill(0);
            assert!(
                crate::bytes::local_reference_target(&ambiguous_marker, tail_at + 30, false)
                    .is_some()
            );
            let admitted = parse(&ambiguous_marker).unwrap();
            assert_eq!(admitted.previous_history_state_id(), Some(1));
            assert_eq!(
                admitted.previous_history_state_id_offset(),
                Some(u64::try_from(tail_at + 30).unwrap())
            );
        }
        for at in references.into_iter().chain([4, zero_tail]) {
            let mut bad = bytes.clone();
            bad[tail_at + at] ^= 1;
            assert!(
                parse(&bad).is_none(),
                "tail length {tail_length}, mutation {at}"
            );
        }
    }
}

#[test]
fn unadmitted_rigid_group_frames_emit_a_loss() {
    use crate::design::decode::scopes::parameter_scope::{
        decode_parameter_scopes, ParameterScopeSources,
    };
    use crate::layout::assembly_alternate_rigid_group_prefix as prefix;
    for (frame, tail, prologue_mutation, nonempty_properties) in [
        (257, 76, None, false),
        (258, 77, None, false),
        (268, 87, None, false),
        (269, 88, None, false),
        (273, 88, Some(prefix::LEADING_BLOCK_PRESENCE), false),
        (273, 88, Some(prefix::PROPERTY_BLOCK_PRESENCE), false),
        (273, 88, Some(prefix::PROPERTY_COUNT), false),
        (273, 88, Some(prefix::ZERO_SEPARATOR), false),
        (313, 88, None, true),
    ] {
        let mut bytes = rigid_as_built_stream();
        let alternate = prologue_mutation.is_some() || nonempty_properties;
        let mut property_width = 0;
        if alternate {
            let mut long_tail = Vec::new();
            push_ordinary_tail(&mut long_tail, true, true);
            bytes.splice(181..257, long_tail);
            let mut prologue = vec![0; 9];
            prologue.push(1);
            prologue.extend_from_slice(&[0; 5]);
            bytes.splice(11..22, prologue);
            if let Some(at) = prologue_mutation {
                bytes[at] = 2;
            }
            if nonempty_properties {
                bytes[prefix::PROPERTY_COUNT..prefix::ZERO_SEPARATOR]
                    .copy_from_slice(&1_u32.to_le_bytes());
                let mut property = Vec::new();
                for value in ["x", "IntrinsicMetaTypeuint64"] {
                    property.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
                    property.extend_from_slice(value.as_bytes());
                }
                property.extend_from_slice(&0_u64.to_le_bytes());
                property_width = property.len();
                bytes.splice(prefix::ZERO_SEPARATOR..prefix::ZERO_SEPARATOR, property);
            }
        } else {
            let mut replacement = Vec::new();
            push_ordinary_tail(&mut replacement, tail >= 87, tail == 77 || tail == 88);
            replacement[if tail < 80 { 61 } else { 72 }] = 2;
            bytes.splice(181..257, replacement);
        }
        assert_eq!(
            frame - 181 - if alternate { 4 } else { 0 } - property_width,
            tail
        );
        let archive = crate::test_support::zip_test::f3d_with_configuration(
            &[],
            "FusionAssetName[Active]/Design1/BulkStream.dat",
            &bytes,
        );
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let decoded = decode_parameter_scopes(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                &ParameterScopeSources::default(),
            )
            .unwrap();
            assert!(decoded.scopes.is_empty());
            assert_eq!(decoded.losses.len(), 1);
            assert_eq!(
                decoded.losses[0].code,
                crate::loss::F3dLossCode::AssemblyRigidGroupFrameUnparsed.kind()
            );
        });
    }
}
