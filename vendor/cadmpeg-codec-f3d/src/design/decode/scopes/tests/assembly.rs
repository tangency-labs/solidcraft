// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment;
use crate::design::test_support::assembly_operand_frame_fixture;
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::assembly::DesignAssemblyAlignmentForm;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly_features::DesignComponentInsertConstruction;
use crate::records::feature::assembly_features::DesignComponentInsertMatrix;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::identity::Located;
use crate::records::identity::ReferenceRun;
use crate::records::parameters::DesignParameterOwner;
use crate::records::parameters::DesignParameterOwnerWire;
use crate::records::references::DesignClassTag;
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::test_support::indexed_header;
use crate::test_support::push_reference_u64;

const EPS_EXACT_FIXTURE: f64 = f64::EPSILON * 4.0;

#[test]
fn assembly_operand_paths_follow_ordered_locator_envelopes() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        scope_record_index,
    );
    scope.class_tag = DesignClassTag::try_from("273".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 637;
            draft.reference_members = ReferenceRun::unlocated(vec![50, 51, 52, 53]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = 637;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: (evaluated_value_offset) - 40,
            frame_length: 104,
            class_tag: DesignClassTag::try_from("457".to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: Some(0),
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    let rectangular_owners = [
        owner(50, 0, 3.0, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 10.0, 503),
        owner(53, 3, 0.0, 504),
    ];
    let mut assembly_bytes = assembly_operand_frame_fixture(scope_record_index);
    assembly_bytes[362..366].copy_from_slice(&2_u32.to_le_bytes());
    for (at, target) in [(366, 64_u32), (377, 67_u32)] {
        assembly_bytes[at] = 1;
        assembly_bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
    }

    let write_path_reference = |bytes: &mut [u8], at: usize, target: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
    };
    let push_path_locator = |bytes: &mut Vec<u8>, locator_index: u32, wrapper_index: u32| {
        let mut locator = vec![0; 190];
        locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
        locator[4..7].copy_from_slice(b"304");
        locator[7..11].copy_from_slice(&locator_index.to_le_bytes());
        write_path_reference(&mut locator, 21, locator_index + 100);
        for ordinal in 0..16 {
            let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
            locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        write_path_reference(&mut locator, 162, scope_record_index);
        write_path_reference(&mut locator, 173, wrapper_index);
        locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&locator);
    };
    let push_path_wrapper = |bytes: &mut Vec<u8>, wrapper_index: u32, path_record_index: u32| {
        let mut wrapper = vec![0; 37];
        wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
        wrapper[4..7].copy_from_slice(b"382");
        wrapper[7..11].copy_from_slice(&wrapper_index.to_le_bytes());
        wrapper[21] = 1;
        wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
        write_path_reference(&mut wrapper, 26, path_record_index);
        bytes.extend_from_slice(&wrapper);
    };
    let push_path = |bytes: &mut Vec<u8>, record_index: u32, guids: &[&str]| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"329");
        bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(
            &(u32::try_from(guids.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for guid in guids {
            let encoded = guid.encode_utf16().collect::<Vec<_>>();
            bytes.extend_from_slice(
                &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
            );
            bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
        }
    };
    let push_identity_path =
        |bytes: &mut Vec<u8>, record_index: u32, path: &[&str], identities: &[&str; 4]| {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(b"390");
            bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(
                &(u32::try_from(path.len()).expect("fixture value fits u32")).to_le_bytes(),
            );
            for guid in path.iter().chain(&identities[..2]) {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(
                    &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
                );
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u64.to_le_bytes());
            for guid in &identities[2..] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(
                    &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
                );
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
        };
    let identities = [
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    let mut identity_path_bytes = assembly_bytes.clone();
    push_path_locator(&mut identity_path_bytes, 64, 66);
    let first_identity_path_at = identity_path_bytes.len();
    push_identity_path(
        &mut identity_path_bytes,
        65,
        &[
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ],
        &identities,
    );
    push_path_wrapper(&mut identity_path_bytes, 66, 65);
    push_path_locator(&mut identity_path_bytes, 67, 69);
    let second_identity_path_at = identity_path_bytes.len();
    push_identity_path(
        &mut identity_path_bytes,
        68,
        &["33333333-3333-3333-3333-333333333333"],
        &identities,
    );
    push_path_wrapper(&mut identity_path_bytes, 69, 68);
    identity_path_bytes.extend_from_slice(&3_u32.to_le_bytes());
    identity_path_bytes.extend_from_slice(b"396");
    identity_path_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let identity_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("identity-qualified assembly occurrence paths");
    assert_eq!(identity_paths[0].class_tag().as_str(), "390");
    assert_eq!(identity_paths[0].occurrence_guids().len(), 2);
    assert_eq!(
        identity_paths[0]
            .identity_guids()
            .iter()
            .map(|guid| guid.value.as_str())
            .collect::<Vec<_>>(),
        identities.to_vec()
    );
    for path_at in [first_identity_path_at, second_identity_path_at] {
        identity_path_bytes[path_at + 4..path_at + 7].copy_from_slice(b"386");
    }
    let compact_identity_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("compact identity-qualified assembly occurrence paths");
    assert!(compact_identity_paths
        .iter()
        .all(|path| path.class_tag().as_str() == "386"));
    for path_at in [first_identity_path_at, second_identity_path_at] {
        identity_path_bytes[path_at + 4..path_at + 7].copy_from_slice(b"329");
    }
    let extended_class_329_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("identity-qualified class-329 assembly occurrence paths");
    assert!(extended_class_329_paths.iter().all(|path| {
        path.class_tag().as_str() == "329"
            && !path.occurrence_guids().is_empty()
            && path
                .identity_guids()
                .iter()
                .map(|guid| guid.value.as_str())
                .eq(identities.iter().copied())
    }));
    let first_identity_length_at = usize::try_from(
        extended_class_329_paths[0].identity_guids()[0]
            .offset
            .checked_sub(4)
            .expect("identity length precedes text"),
    )
    .expect("identity length offset fits usize");
    let mut malformed_class_329_identity = identity_path_bytes.clone();
    malformed_class_329_identity[first_identity_length_at..first_identity_length_at + 4]
        .copy_from_slice(&35_u32.to_le_bytes());
    assert!(exact_assembly_alignment(
        &malformed_class_329_identity,
        &crate::design::test_support::indexed_record_offsets_for_test(
            &malformed_class_329_identity
        ),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_paths().is_none()));

    let first_locator_at = assembly_bytes.len();
    push_path_locator(&mut assembly_bytes, 64, 66);
    push_path(
        &mut assembly_bytes,
        65,
        &[
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ],
    );
    push_path_wrapper(&mut assembly_bytes, 66, 65);
    push_path_locator(&mut assembly_bytes, 67, 69);
    push_path(
        &mut assembly_bytes,
        68,
        &["33333333-3333-3333-3333-333333333333"],
    );
    push_path_wrapper(&mut assembly_bytes, 69, 68);
    assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    assembly_bytes.extend_from_slice(b"396");
    assembly_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let paths = exact_assembly_alignment(
        &assembly_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&assembly_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("exact assembly occurrence paths");
    assert_eq!(paths[0].link().locator_record_index, 64);
    assert_eq!(paths[0].link().wrapper_record_index, 66);
    assert_eq!(paths[0].link().locator_reference_offset, 367);
    assert_eq!(paths[0].link().locator_scope_reference_offset, 811);
    assert_eq!(paths[0].link().wrapper_reference_offset, 822);
    assert_eq!(paths[0].link().path_reference_offset, 1_042);
    assert_eq!(
        paths.each_ref().map(|path| {
            (
                path.record_index,
                path.occurrence_guids()
                    .iter()
                    .map(|guid| guid.value.as_str().to_owned())
                    .collect::<Vec<_>>(),
            )
        }),
        [
            (
                65,
                vec![
                    "11111111-1111-1111-1111-111111111111".into(),
                    "22222222-2222-2222-2222-222222222222".into(),
                ],
            ),
            (68, vec!["33333333-3333-3333-3333-333333333333".into()],),
        ]
    );
    let mut reversed_path_assignment = assembly_bytes.clone();
    reversed_path_assignment[367..371].copy_from_slice(&67_u32.to_le_bytes());
    reversed_path_assignment[378..382].copy_from_slice(&64_u32.to_le_bytes());
    let reversed_paths = exact_assembly_alignment(
        &reversed_path_assignment,
        &crate::design::test_support::indexed_record_offsets_for_test(&reversed_path_assignment),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("scope locator order assigns paths to operand frames");
    assert_eq!(reversed_paths.map(|path| path.record_index), [68, 65]);

    let mut duplicate_path_assignment = assembly_bytes.clone();
    duplicate_path_assignment[378..382].copy_from_slice(&64_u32.to_le_bytes());
    assert!(exact_assembly_alignment(
        &duplicate_path_assignment,
        &crate::design::test_support::indexed_record_offsets_for_test(&duplicate_path_assignment),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_paths().is_none()));

    for locator_zero_at in [first_locator_at + 32, first_locator_at + 161] {
        let mut invalid_locator = assembly_bytes.clone();
        invalid_locator[locator_zero_at] = 1;
        assert!(exact_assembly_alignment(
            &invalid_locator,
            &crate::design::test_support::indexed_record_offsets_for_test(&invalid_locator),
            &scope,
            &rectangular_owners,
        )
        .unwrap()
        .is_some_and(|alignment| alignment.operand_paths().is_none()));
    }
    let first_wrapper_at =
        usize::try_from(paths[0].link().wrapper_byte_offset).expect("wrapper offset fits usize");
    for (relative_offset, value) in [(21, 0), (22, 2), (26, 0)] {
        let mut invalid_wrapper = assembly_bytes.clone();
        invalid_wrapper[first_wrapper_at + relative_offset] = value;
        assert!(exact_assembly_alignment(
            &invalid_wrapper,
            &crate::design::test_support::indexed_record_offsets_for_test(&invalid_wrapper),
            &scope,
            &rectangular_owners,
        )
        .unwrap()
        .is_some_and(|alignment| alignment.operand_paths().is_none()));
    }
    let first_wrapper_end = first_wrapper_at + 37;
    let mut extended_wrapper = assembly_bytes.clone();
    extended_wrapper.insert(first_wrapper_end, 0);
    assert!(exact_assembly_alignment(
        &extended_wrapper,
        &crate::design::test_support::indexed_record_offsets_for_test(&extended_wrapper),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_paths().is_none()));

    let push_class_294_path =
        |bytes: &mut Vec<u8>, record_index: u32, occurrence: &str, identities: &[&str; 4]| {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(b"294");
            bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&[1, 0, 0, 0]);
            for guid in [occurrence, identities[0], identities[1]] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(
                    &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
                );
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u64.to_le_bytes());
            for guid in &identities[2..] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(
                    &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
                );
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
        };
    let class_294_identities = [
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    let mut class_294_path_bytes = assembly_bytes[..648].to_vec();
    push_path_locator(&mut class_294_path_bytes, 64, 66);
    let first_class_294_path_at = class_294_path_bytes.len();
    push_class_294_path(
        &mut class_294_path_bytes,
        65,
        "11111111-1111-1111-1111-111111111111",
        &class_294_identities,
    );
    push_path_wrapper(&mut class_294_path_bytes, 66, 65);
    push_path_locator(&mut class_294_path_bytes, 67, 69);
    let second_class_294_path_at = class_294_path_bytes.len();
    push_class_294_path(
        &mut class_294_path_bytes,
        68,
        "22222222-2222-2222-2222-222222222222",
        &class_294_identities,
    );
    push_path_wrapper(&mut class_294_path_bytes, 69, 68);
    class_294_path_bytes.extend_from_slice(&3_u32.to_le_bytes());
    class_294_path_bytes.extend_from_slice(b"396");
    class_294_path_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let class_294_paths = exact_assembly_alignment(
        &class_294_path_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&class_294_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("class-294 identity-qualified assembly occurrence paths");
    assert!(class_294_paths.iter().all(|path| {
        path.class_tag().as_str() == "294"
            && path.occurrence_guids().len() == 1
            && path
                .identity_guids()
                .iter()
                .map(|guid| guid.value.as_str())
                .eq(class_294_identities.iter().copied())
    }));
    for path_at in [first_class_294_path_at, second_class_294_path_at] {
        class_294_path_bytes[path_at + 4..path_at + 7].copy_from_slice(b"299");
    }
    let class_299_paths = exact_assembly_alignment(
        &class_294_path_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&class_294_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("class-299 identity-qualified assembly occurrence paths");
    assert!(class_299_paths.iter().all(|path| {
        path.class_tag().as_str() == "299"
            && path.occurrence_guids().len() == 1
            && path
                .identity_guids()
                .iter()
                .map(|guid| guid.value.as_str())
                .eq(class_294_identities.iter().copied())
    }));

    assembly_bytes[25] = 0;
    assert!(exact_assembly_alignment(
        &assembly_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&assembly_bytes),
        &scope,
        &rectangular_owners
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_frames())
    .is_some());
    assembly_bytes[25] = 2;
    assert!(exact_assembly_alignment(
        &assembly_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&assembly_bytes),
        &scope,
        &rectangular_owners
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_frames().is_none()));

    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(99);
                ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_assembly_alignment(
            &assembly_bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&assembly_bytes),
            &scope,
            &rectangular_owners
        )
        .unwrap(),
        None
    );
}

#[test]
fn legacy_class_383_258_assembly_uses_its_interleaved_operand_grammar() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        scope_record_index,
    );
    scope.class_tag = DesignClassTag::try_from("383".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length =
                u64_from_index(crate::layout::assembly_class_383_258_scope_1011::LEN);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = draft.frame_length;
            draft.reference_members = ReferenceRun::unlocated(vec![
                100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 200, 201, 202, 203,
                204, 205, 206, 207, 112, 113, 114, 115, 300, 210, 211, 212, 213, 214, 215, 216,
                217, 116, 117, 118, 119, 400,
            ]);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owners = (0_usize..20)
        .map(|ordinal| {
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!(
                    "f3d:Design/BulkStream.dat:design-parameter-owner#{}",
                    100 + ordinal
                ),
                byte_offset: (2_000 + u64_from_index(ordinal)) - 40,
                frame_length: 103,
                class_tag: DesignClassTag::try_from("284".to_owned()).unwrap(),
                record_index: 100 + u32::try_from(ordinal).expect("fixture value fits u32"),
                scope_record_index,
                local_ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
                evaluated_value: match ordinal {
                    8 => 0.25,
                    9 => 1.0,
                    10 => 2.0,
                    11 => 3.0,
                    _ => 0.0,
                },
                evaluated_value_offset: 2_000 + u64_from_index(ordinal),
                parameter_record_index: 100
                    + u32::try_from(ordinal).expect("fixture value fits u32")
                    + 1,
                owned_ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
                variant: None,
                companion_record_index: 100
                    + u32::try_from(ordinal).expect("fixture value fits u32")
                    + 2,
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
    let bytes = legacy_class_383_258_fixture(
        scope_record_index,
        &scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
    );
    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("legacy class-383 alignment");

    assert_eq!(alignment.angle(), 0.25);
    assert_eq!(alignment.offset(), [1.0, 2.0, 3.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        vec![108, 109, 110, 111]
    );
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.offset)
            .collect::<Vec<_>>(),
        vec![2_008, 2_009, 2_010, 2_011]
    );
    let frames = alignment.operand_frames().expect("legacy operand frames");
    assert_eq!(
        frames.each_ref().map(|frame| frame.reference_record_index),
        [300, 400]
    );
    assert_eq!(
        frames.each_ref().map(|frame| frame.transform[0][3]),
        [1.25, -2.5]
    );
    let paths = alignment.operand_paths().expect("legacy operand paths");
    assert!(paths.iter().all(|path| path.class_tag().as_str() == "386"));
    assert_eq!(
        paths
            .each_ref()
            .map(|path| path.link().locator_record_index),
        [300, 400]
    );
    assert_eq!(
        paths
            .each_ref()
            .map(|path| path.occurrence_guids()[0].value.as_str()),
        [
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "cccccccc-cccc-cccc-cccc-cccccccccccc",
        ]
    );

    let mut malformed = bytes;
    let first_carrier_at = malformed
        .windows(11)
        .position(|header| header[0..4] == 3_u32.to_le_bytes() && &header[4..7] == b"378")
        .expect("first carrier header");
    malformed[first_carrier_at
        + crate::layout::assembly_class_383_258_frame_378_carrier::SCOPE_REFERENCE
        ..first_carrier_at
            + crate::layout::assembly_class_383_258_frame_378_carrier::SCOPE_REFERENCE
            + 8]
        .copy_from_slice(&999_u64.to_le_bytes());
    let malformed_alignment = exact_assembly_alignment(
        &malformed,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("alignment scalar grammar remains exact");
    assert!(malformed_alignment.operand_paths().is_none());
}

#[test]
fn legacy_class_388_266_assembly_uses_its_interleaved_owner_grammar() {
    let scope_record_index = 700_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#700",
        DesignFeatureKind::Assemble,
        scope_record_index,
    );
    scope.class_tag = DesignClassTag::try_from("388".to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("266".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length =
                u64_from_index(crate::layout::assembly_class_388_266_scope_968::LEN);
            draft.paired_byte_offset = draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.feature_ordinal = std::num::NonZeroU32::new(4).expect("nonzero ordinal");
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(
                (0..24)
                    .map(|ordinal| 1_000 + ordinal)
                    .chain([1_200, 1_201, 1_202, 1_203, 1_204, 1_205])
                    .chain((24..28).map(|ordinal| 1_000 + ordinal))
                    .chain([1_034])
                    .collect(),
            );
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owners = (0..28)
        .map(|ordinal| {
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!(
                    "f3d:Design/BulkStream.dat:design-parameter-owner#{}",
                    1_000 + ordinal
                ),
                byte_offset: (2_000 + u64::from(ordinal)) - 40,
                frame_length: 103,
                class_tag: DesignClassTag::try_from("282".to_owned()).unwrap(),
                record_index: 1_000 + ordinal,
                scope_record_index,
                local_ordinal: ordinal,
                evaluated_value: match ordinal {
                    4 => 0.25,
                    5 => 1.0,
                    6 => 2.0,
                    7 => 3.0,
                    _ => 0.0,
                },
                evaluated_value_offset: 2_000 + u64::from(ordinal),
                parameter_record_index: 1_000 + ordinal + 1,
                owned_ordinal: ordinal,
                variant: None,
                companion_record_index: 1_000 + ordinal + 2,
            })
            .unwrap()
        })
        .collect::<Vec<_>>();

    let mut bytes = vec![0; crate::layout::assembly_class_388_266_scope_968::LEN + 11];
    bytes[20..26]
        .copy_from_slice(&crate::layout::assembly_class_388_266_scope_968::SCOPE_FLAGS_VALUE);
    let write_reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    write_reference(&mut bytes, 28, 1_034);
    write_reference(&mut bytes, 168, 2_034);
    bytes[362..366].copy_from_slice(&2_u32.to_le_bytes());
    write_reference(&mut bytes, 366, 5_001);
    write_reference(&mut bytes, 377, 5_002);
    write_reference(&mut bytes, 388, 5_003);
    let first_transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 1.25],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let second_transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, -2.5],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (at, transform) in [(40, first_transform), (180, second_transform)] {
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            bytes[at + ordinal * 8..at + ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[399..403].copy_from_slice(&36_u32.to_le_bytes());
    for (ordinal, code_unit) in "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
        .encode_utf16()
        .enumerate()
    {
        bytes[403 + ordinal * 2..405 + ordinal * 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    bytes[478..482].copy_from_slice(&35_u32.to_le_bytes());
    for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
        write_reference(&mut bytes, 482 + ordinal * 11, record_index);
    }
    bytes[867..871].copy_from_slice(&[0xff; 4]);
    bytes[871..875].copy_from_slice(&8_u32.to_le_bytes());
    for (ordinal, code_unit) in "Assemble".encode_utf16().enumerate() {
        bytes[875 + ordinal * 2..877 + ordinal * 2].copy_from_slice(&code_unit.to_le_bytes());
    }
    bytes[891..895].copy_from_slice(&scope.feature_ordinal.get().to_le_bytes());

    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("legacy class-388 alignment");
    assert_eq!(alignment.angle(), 0.25);
    assert_eq!(alignment.offset(), [1.0, 2.0, 3.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [1_004, 1_005, 1_006, 1_007]
    );
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.offset)
            .collect::<Vec<_>>(),
        [2_004, 2_005, 2_006, 2_007]
    );
    let frames = alignment
        .operand_frames()
        .expect("legacy class-388 operand frames");
    assert_eq!(
        frames
            .each_ref()
            .map(|frame| (frame.reference_record_index, frame.transform[0][3])),
        [(1_034, 1.25), (2_034, -2.5)]
    );
    assert_eq!(alignment.operand_paths(), None);

    let write_guid = |bytes: &mut [u8], at: usize, guid: &str| {
        let encoded = guid.encode_utf16().collect::<Vec<_>>();
        bytes[at..at + 4].copy_from_slice(
            &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for (ordinal, code_unit) in encoded.into_iter().enumerate() {
            bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2]
                .copy_from_slice(&code_unit.to_le_bytes());
        }
    };
    let append_locator =
        |bytes: &mut Vec<u8>, locator_record_index: u32, wrapper_record_index: u32| {
            let start = bytes.len();
            let mut locator = vec![0_u8; 190];
            locator[..4].copy_from_slice(&3_u32.to_le_bytes());
            locator[4..7].copy_from_slice(b"451");
            locator[7..11].copy_from_slice(&locator_record_index.to_le_bytes());
            write_reference(&mut locator, 21, 9_001 + locator_record_index);
            for (ordinal, value) in [
                1.0_f64, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
            .into_iter()
            .enumerate()
            {
                locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
            }
            write_reference(&mut locator, 162, scope_record_index);
            write_reference(&mut locator, 173, wrapper_record_index);
            locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
            bytes.extend(locator);
            start
        };
    let append_path = |bytes: &mut Vec<u8>, record_index: u32, occurrence_guid: &str| {
        let start = bytes.len();
        let mut path = vec![0_u8; 425];
        path[..4].copy_from_slice(&3_u32.to_le_bytes());
        path[4..7].copy_from_slice(b"412");
        path[7..11].copy_from_slice(&record_index.to_le_bytes());
        path[21] = 1;
        write_guid(&mut path, 25, occurrence_guid);
        for (at, guid) in [
            (101, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            (177, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
            (261, "cccccccc-cccc-cccc-cccc-cccccccccccc"),
            (337, "dddddddd-dddd-dddd-dddd-dddddddddddd"),
        ] {
            write_guid(&mut path, at, guid);
        }
        path[253..261].copy_from_slice(&2_u64.to_le_bytes());
        path[413..417].copy_from_slice(&2_u32.to_le_bytes());
        bytes.extend(path);
        start
    };
    let append_wrapper =
        |bytes: &mut Vec<u8>, wrapper_record_index: u32, path_record_indices: &[u32]| {
            let start = bytes.len();
            let mut wrapper = vec![0_u8; 37 + (path_record_indices.len() - 1) * 11];
            wrapper[..4].copy_from_slice(&3_u32.to_le_bytes());
            wrapper[4..7].copy_from_slice(b"369");
            wrapper[7..11].copy_from_slice(&wrapper_record_index.to_le_bytes());
            wrapper[21] = 1;
            wrapper[22..26].copy_from_slice(
                &(u32::try_from(path_record_indices.len()).expect("fixture value fits u32"))
                    .to_le_bytes(),
            );
            for (ordinal, path_record_index) in path_record_indices.iter().copied().enumerate() {
                write_reference(&mut wrapper, 26 + ordinal * 11, path_record_index);
            }
            bytes.extend(wrapper);
            start
        };
    let mut path_bytes = bytes.clone();
    write_reference(&mut path_bytes, 366, 5_001);
    write_reference(&mut path_bytes, 377, 5_101);
    let first_locator_at = append_locator(&mut path_bytes, 5_001, 5_004);
    let first_path_at = append_path(
        &mut path_bytes,
        5_002,
        "11111111-1111-1111-1111-111111111111",
    );
    append_path(
        &mut path_bytes,
        5_003,
        "22222222-2222-2222-2222-222222222222",
    );
    let first_wrapper_at = append_wrapper(&mut path_bytes, 5_004, &[5_002, 5_003]);
    append_locator(&mut path_bytes, 5_101, 5_103);
    append_path(
        &mut path_bytes,
        5_102,
        "33333333-3333-3333-3333-333333333333",
    );
    append_wrapper(&mut path_bytes, 5_103, &[5_102]);
    path_bytes.extend_from_slice(&3_u32.to_le_bytes());
    path_bytes.extend_from_slice(b"396");
    path_bytes.extend_from_slice(&5_200_u32.to_le_bytes());
    let paths = exact_assembly_alignment(
        &path_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&path_bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("legacy class-388 occurrence paths");
    assert_eq!(paths[0].link().locator_record_index, 5_001);
    assert_eq!(paths[0].link().locator_class_tag.as_str(), "451");
    assert_eq!(
        paths[0].link().locator_byte_offset,
        u64_from_index(first_locator_at)
    );
    assert_eq!(paths[0].link().wrapper_record_index, 5_004);
    assert_eq!(paths[0].link().wrapper_class_tag.as_str(), "369");
    assert_eq!(
        paths[0].link().wrapper_byte_offset,
        u64_from_index(first_wrapper_at)
    );
    assert_eq!(paths[0].record_index, 5_003);
    assert_eq!(paths[0].class_tag().as_str(), "412");
    assert_eq!(paths[0].byte_offset(), u64_from_index(first_path_at + 425));
    assert_eq!(
        paths[0]
            .occurrence_guids()
            .iter()
            .map(|guid| guid.value.as_str().to_owned())
            .collect::<Vec<_>>(),
        [
            "11111111-1111-1111-1111-111111111111".to_owned(),
            "22222222-2222-2222-2222-222222222222".to_owned(),
        ]
    );
    assert_eq!(paths[0].identity_guids().len(), 4);
    assert_eq!(paths[1].link().locator_record_index, 5_101);
    assert_eq!(paths[1].link().wrapper_record_index, 5_103);
    assert_eq!(paths[1].record_index, 5_102);
    assert_eq!(paths[1].occurrence_guids().len(), 1);

    let mut malformed_wrapper = path_bytes.clone();
    malformed_wrapper[first_wrapper_at + 22..first_wrapper_at + 26]
        .copy_from_slice(&3_u32.to_le_bytes());
    assert!(exact_assembly_alignment(
        &malformed_wrapper,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_wrapper),
        &scope,
        &owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_paths().is_none()));
    let mut malformed_path = path_bytes.clone();
    malformed_path[first_path_at + 417] = 1;
    assert!(exact_assembly_alignment(
        &malformed_path,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed_path),
        &scope,
        &owners,
    )
    .unwrap()
    .is_some_and(|alignment| alignment.operand_paths().is_none()));

    let mut malformed = bytes;
    malformed[25] = 0;
    assert!(exact_assembly_alignment(
        &malformed,
        &crate::design::test_support::indexed_record_offsets_for_test(&malformed),
        &scope,
        &owners,
    )
    .unwrap()
    .is_none());
}

/// The placement-ordered `As-built` form stores a u32 additional-owner count at
/// offset 43 and that many owner references after it, then the locator count
/// and two locator references; its locators are wide operand-path locators.
/// Its owners are the three offsets, the angle, and the additional owners. The
/// 378-byte form has no additional owners, the 422-byte form two, and the 444-byte
/// form three.
#[test]
fn as_built_378_alignment_reads_wide_locators() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    use compact_wide::{push_header, push_wide_operand_path};
    let scope_record_index = 10_u32;
    let scope_of = |kind, count: u32| {
        let frame_length = 378 + 22 * u64::from(count);
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#0",
            kind,
            scope_record_index,
        );
        scope.class_tag = DesignClassTag::try_from("328".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.reference_members = ReferenceRun::unlocated(
                    (if count == 0 {
                        [80, 81, 82, 83, 84, 50, 51, 52, 53]
                    } else {
                        [80, 50, 51, 52, 53, 81, 82, 83, 84]
                    })
                    .into_iter()
                    .chain(54..54 + count)
                    .collect::<Vec<_>>(),
                );
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.paired_class_tag = DesignClassTag::try_from("264".to_owned()).unwrap();
        scope
    };
    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: evaluated_value_offset - 40,
            frame_length: 103,
            class_tag: DesignClassTag::try_from("321".to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    let owners_of = |count: u32| {
        [(1.0, 501), (2.0, 502), (3.0, 503), (0.25, 504)]
            .into_iter()
            .chain((0..count).map(|ordinal| (-0.5 + f64::from(ordinal), 505 + u64::from(ordinal))))
            .zip(0_u32..)
            .map(|((value, at), ordinal)| owner(50 + ordinal, ordinal, value, at))
            .collect::<Vec<_>>()
    };
    let bytes_of = |count: u32| {
        let extra = 11 * usize::try_from(count).unwrap();
        let frame_length = 378 + 2 * extra;
        let mut bytes = vec![0_u8; frame_length];
        bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"328");
        bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
        bytes[43..47].copy_from_slice(&count.to_le_bytes());
        for ordinal in 0..count {
            let at = 47 + 11 * usize::try_from(ordinal).unwrap();
            bytes[at] = 1;
            bytes[at + 1..at + 5].copy_from_slice(&(54 + ordinal).to_le_bytes());
        }
        bytes[47 + extra..51 + extra].copy_from_slice(&2_u32.to_le_bytes());
        for (at, locator_record_index) in [(51 + extra, 64_u32), (62 + extra, 67_u32)] {
            bytes[at] = 1;
            bytes[at + 1..at + 5].copy_from_slice(&locator_record_index.to_le_bytes());
        }
        push_header(&mut bytes, b"264", scope_record_index);
        bytes.resize(frame_length + 30, 0);
        push_wide_operand_path(
            &mut bytes,
            64,
            scope_record_index,
            "11111111-1111-1111-1111-111111111111",
        );
        let second = bytes.len();
        push_wide_operand_path(
            &mut bytes,
            67,
            scope_record_index,
            "66666666-6666-6666-6666-666666666666",
        );
        bytes[second + 57..second + 61].copy_from_slice(&901_u32.to_le_bytes());
        push_header(&mut bytes, b"300", 71);
        bytes
    };

    let locator = "2E3AC990-483D-4A55-8662-C064B976A957";
    let types = std::collections::HashMap::from([
        (64, (locator, 5)),
        (65, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
        (67, (locator, 5)),
        (68, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
    ]);
    for count in [0, 2, 3] {
        let bytes = bytes_of(count);
        let owners = owners_of(count);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let read = |bytes: &[u8], kind| {
            exact_assembly_alignment_with_types(
                bytes,
                &records,
                &scope_of(kind, count),
                &owners,
                &types,
            )
            .unwrap()
        };
        let alignment = read(&bytes, DesignFeatureKind::AsBuilt)
            .unwrap_or_else(|| panic!("As-built alignment with {count} additional owners"));
        assert_eq!(alignment.angle(), 0.25);
        assert_eq!(alignment.offset(), [1.0, 2.0, 3.0]);
        assert_eq!(alignment.owners.len(), 4);
        let serialized = serde_json::to_value(&alignment).unwrap();
        let restored: crate::records::feature::assembly::DesignAssemblyAlignment =
            serde_json::from_value(serialized).unwrap();
        assert_eq!(restored, alignment);
        let frames = alignment.operand_frames().expect("wide locator frames");
        assert!(frames
            .iter()
            .all(|frame| frame.transform.rows()
                == cadmpeg_ir::transform::Transform::identity().rows()));
        let paths = alignment.operand_path_refs().expect("both operand paths");
        assert_eq!(
            paths.map(|path| path.occurrence_guids()[0].value.as_str().to_owned()),
            [
                "11111111-1111-1111-1111-111111111111".to_owned(),
                "66666666-6666-6666-6666-666666666666".to_owned()
            ]
        );

        // An `Assemble` scope of the same length has no wide locators.
        let assemble = read(&bytes, DesignFeatureKind::Assemble);
        assert!(assemble.is_none_or(|alignment| alignment.operand_path_refs().is_none()));

        // A stored count other than the frame's has no alignment.
        let mut miscounted = bytes.clone();
        miscounted[43..47].copy_from_slice(&(count + 1).to_le_bytes());
        assert!(read(&miscounted, DesignFeatureKind::AsBuilt).is_none());

        if count != 0 {
            let mut wrong_owner = bytes.clone();
            wrong_owner[48..52].copy_from_slice(&99_u32.to_le_bytes());
            assert!(read(&wrong_owner, DesignFeatureKind::AsBuilt).is_none());
        } else {
            for references in [
                vec![50, 51, 52, 53],
                vec![80, 50, 51, 52, 53, 81, 82, 83, 84],
            ] {
                let mut wrong_table = scope_of(DesignFeatureKind::AsBuilt, count);
                wrong_table
                    .try_edit(|draft| {
                        draft.reference_members = ReferenceRun::unlocated(references);
                        draft.layout_fixture_references();
                        draft.layout_fixture_tail();
                    })
                    .unwrap();
                assert!(exact_assembly_alignment_with_types(
                    &bytes,
                    &records,
                    &wrong_table,
                    &owners,
                    &types
                )
                .unwrap()
                .is_none());
            }
        }
    }
}

#[test]
fn as_built_alignment_uses_locator_frames_and_parameter_owner_lanes() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::AsBuilt,
        scope_record_index,
    );
    scope.class_tag = DesignClassTag::try_from("439".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 399;
            draft.reference_members = ReferenceRun::unlocated(vec![50, 51, 52, 53]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = 399;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: (evaluated_value_offset) - 40,
            frame_length: 103,
            class_tag: DesignClassTag::try_from("321".to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    let owners = [
        owner(50, 0, 0.25, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 2.0, 503),
        owner(53, 3, 3.0, 504),
    ];

    let mut bytes = vec![0_u8; 399];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"439");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[47..51].copy_from_slice(&2_u32.to_le_bytes());
    for (at, locator_record_index) in [(51, 64_u32), (62, 67_u32)] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&locator_record_index.to_le_bytes());
    }
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let first_locator_at = bytes.len();
    append_as_built_path_envelope(
        &mut bytes,
        scope_record_index,
        64,
        70,
        "11111111-1111-1111-1111-111111111111",
        [1.0, 2.0, 3.0],
    );
    let second_locator_at = bytes.len();
    append_as_built_path_envelope(
        &mut bytes,
        scope_record_index,
        67,
        80,
        "22222222-2222-2222-2222-222222222222",
        [4.0, 5.0, 6.0],
    );
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&90_u32.to_le_bytes());

    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("exact As-built alignment");
    assert_eq!(alignment.angle(), 0.25);
    assert_eq!(alignment.offset(), [1.0, 2.0, 3.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [50, 51, 52, 53]
    );
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.offset)
            .collect::<Vec<_>>(),
        [501, 502, 503, 504]
    );
    let frames = alignment.operand_frames().expect("locator transforms");
    assert_eq!(
        frames
            .each_ref()
            .map(|frame| (frame.reference_record_index, frame.transform[0][3])),
        [(70, 1.0), (80, 4.0)]
    );
    assert_eq!(
        frames.each_ref().map(|frame| frame.transform_offset),
        [
            u64::try_from(first_locator_at + 33).expect("fixture offset fits u64"),
            u64::try_from(second_locator_at + 33).expect("fixture offset fits u64"),
        ]
    );
    let paths = alignment.operand_paths().expect("locator occurrence paths");
    assert_eq!(
        paths.each_ref().map(|path| (
            path.link().locator_record_index,
            path.occurrence_guids()[0].value.as_str()
        )),
        [
            (64, "11111111-1111-1111-1111-111111111111"),
            (67, "22222222-2222-2222-2222-222222222222"),
        ]
    );

    let mut invalid_transform = bytes.clone();
    invalid_transform[first_locator_at + 33..first_locator_at + 41]
        .copy_from_slice(&2.0_f64.to_le_bytes());
    let incomplete = exact_assembly_alignment(
        &invalid_transform,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_transform),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("alignment scalars remain exact");
    assert_eq!(incomplete.operand_frames(), None);
    assert_eq!(incomplete.operand_paths(), None);

    let mut duplicate_reference = bytes;
    duplicate_reference[63..67].copy_from_slice(&64_u32.to_le_bytes());
    let incomplete = exact_assembly_alignment(
        &duplicate_reference,
        &crate::design::test_support::indexed_record_offsets_for_test(&duplicate_reference),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("alignment scalars remain exact");
    assert_eq!(incomplete.operand_frames(), None);
    assert_eq!(incomplete.operand_paths(), None);
}

fn legacy_class_383_258_fixture(scope_record_index: u32, members: &[u32]) -> Vec<u8> {
    let scope_len = crate::layout::assembly_class_383_258_scope_1011::LEN;
    let mut bytes = vec![0_u8; scope_len + 11];
    write_legacy_class_383_header(&mut bytes, 0, b"383", scope_record_index);
    bytes[20] = 1;
    bytes[25] = 1;
    write_legacy_class_383_reference(
        &mut bytes,
        crate::layout::assembly_class_383_258_scope_1011::FIRST_OPERAND_REFERENCE,
        members[24],
    );
    write_legacy_class_383_transform(&mut bytes, 40, legacy_class_383_transform(1.25));
    write_legacy_class_383_reference(
        &mut bytes,
        crate::layout::assembly_class_383_258_scope_1011::SECOND_OPERAND_REFERENCE,
        members[37],
    );
    write_legacy_class_383_transform(&mut bytes, 180, legacy_class_383_transform(-2.5));
    write_legacy_class_383_header(&mut bytes, scope_len, b"258", scope_record_index);

    append_legacy_class_383_operand_envelope(
        &mut bytes,
        scope_record_index,
        &[
            members[12],
            members[13],
            members[14],
            members[15],
            members[16],
            members[17],
            members[18],
            members[19],
            members[24],
            members[20],
            members[21],
            members[22],
            members[23],
        ],
        legacy_class_383_transform(1.25),
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    );
    append_legacy_class_383_operand_envelope(
        &mut bytes,
        scope_record_index,
        &[
            members[25],
            members[26],
            members[27],
            members[28],
            members[29],
            members[30],
            members[31],
            members[32],
            members[37],
            members[33],
            members[34],
            members[35],
            members[36],
        ],
        legacy_class_383_transform(-2.5),
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    );
    bytes
}

fn append_legacy_class_383_operand_envelope(
    bytes: &mut Vec<u8>,
    scope_record_index: u32,
    members: &[u32],
    transform: [[f64; 4]; 4],
    occurrence_guid: &str,
    identity_guid: &str,
) {
    let leading_record_index = members[0];
    let leading_identity_record_index = members[1];
    let child_record_index = members[2];
    let child_identity_record_index = members[3];
    let first_face_record_index = members[4];
    let first_face_identity_record_index = members[5];
    let second_face_record_index = members[6];
    let second_face_identity_record_index = members[7];
    let carrier_record_index = members[8];
    let placement_owners = &members[9..13];

    append_legacy_class_383_frame(
        bytes,
        b"387",
        leading_record_index,
        crate::layout::assembly_class_383_258_frame_387_leading::LEN,
        move |frame| {
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_387_leading::IDENTITY_REFERENCE,
                leading_identity_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_387_leading::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"359",
        leading_identity_record_index,
        crate::layout::assembly_class_383_258_frame_359_identity::LEN,
        move |frame| {
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::OCCURRENCE_GUID,
                occurrence_guid,
            );
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::IDENTITY_GUID,
                identity_guid,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"387",
        child_record_index,
        crate::layout::assembly_class_383_258_frame_387_child::LEN,
        move |frame| {
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_387_child::IDENTITY_REFERENCE,
                child_identity_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_387_child::LEADING_REFERENCE,
                leading_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_387_child::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"359",
        child_identity_record_index,
        crate::layout::assembly_class_383_258_frame_359_identity::LEN,
        move |frame| {
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::OCCURRENCE_GUID,
                occurrence_guid,
            );
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::IDENTITY_GUID,
                identity_guid,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"394",
        first_face_record_index,
        crate::layout::assembly_class_383_258_frame_394::LEN,
        move |frame| {
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_394::IDENTITY_REFERENCE,
                first_face_identity_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_394::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"359",
        first_face_identity_record_index,
        crate::layout::assembly_class_383_258_frame_359_identity::LEN,
        move |frame| {
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::OCCURRENCE_GUID,
                occurrence_guid,
            );
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::IDENTITY_GUID,
                identity_guid,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"394",
        second_face_record_index,
        crate::layout::assembly_class_383_258_frame_394::LEN,
        move |frame| {
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_394::IDENTITY_REFERENCE,
                second_face_identity_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_394::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"359",
        second_face_identity_record_index,
        crate::layout::assembly_class_383_258_frame_359_identity::LEN,
        move |frame| {
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::OCCURRENCE_GUID,
                occurrence_guid,
            );
            write_legacy_class_383_guid(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::IDENTITY_GUID,
                identity_guid,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_359_identity::SCOPE_REFERENCE,
                scope_record_index,
            );
        },
    );
    append_legacy_class_383_frame(
        bytes,
        b"378",
        carrier_record_index,
        crate::layout::assembly_class_383_258_frame_378_carrier::LEN,
        move |frame| {
            write_legacy_class_383_transform(
                frame,
                crate::layout::assembly_class_383_258_frame_378_carrier::TRANSFORM,
                transform,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_378_carrier::CHILD_REFERENCE,
                child_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_378_carrier::SECOND_FACE_REFERENCE,
                second_face_record_index,
            );
            write_legacy_class_383_reference(
                frame,
                crate::layout::assembly_class_383_258_frame_378_carrier::FIRST_FACE_REFERENCE,
                first_face_record_index,
            );
            for (ordinal, record_index) in placement_owners.iter().copied().enumerate() {
                write_legacy_class_383_reference(
                    frame,
                    crate::layout::assembly_class_383_258_frame_378_carrier::PLACEMENT_OWNER_REFERENCES
                        + ordinal * 11,
                    record_index,
                );
            }
            for (offset, record_index) in [
                (
                    crate::layout::assembly_class_383_258_frame_378_carrier::REPEATED_CHILD_REFERENCE,
                    child_record_index,
                ),
                (
                    crate::layout::assembly_class_383_258_frame_378_carrier::REPEATED_FIRST_FACE_REFERENCE,
                    first_face_record_index,
                ),
                (
                    crate::layout::assembly_class_383_258_frame_378_carrier::REPEATED_SECOND_FACE_REFERENCE,
                    second_face_record_index,
                ),
                (
                    crate::layout::assembly_class_383_258_frame_378_carrier::SCOPE_REFERENCE,
                    scope_record_index,
                ),
            ] {
                write_legacy_class_383_reference(frame, offset, record_index);
            }
        },
    );
}

fn append_legacy_class_383_frame<F>(
    bytes: &mut Vec<u8>,
    class_tag: &[u8; 3],
    record_index: u32,
    frame_length: usize,
    configure: F,
) where
    F: FnOnce(&mut [u8]),
{
    let start = bytes.len();
    bytes.resize(start + frame_length + 11, 0);
    write_legacy_class_383_header(bytes, start, class_tag, record_index);
    write_legacy_class_383_header(bytes, start + frame_length, b"258", record_index);
    configure(&mut bytes[start..start + frame_length]);
}

fn write_legacy_class_383_header(
    bytes: &mut [u8],
    at: usize,
    class_tag: &[u8; 3],
    record_index: u32,
) {
    bytes[at..at + 4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[at + 4..at + 7].copy_from_slice(class_tag);
    bytes[at + 7..at + 11].copy_from_slice(&record_index.to_le_bytes());
}

fn write_legacy_class_383_reference(bytes: &mut [u8], at: usize, record_index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 9].copy_from_slice(&u64::from(record_index).to_le_bytes());
}

fn write_legacy_class_383_guid(bytes: &mut [u8], at: usize, guid: &str) {
    let encoded = guid.encode_utf16().collect::<Vec<_>>();
    bytes[at..at + 4].copy_from_slice(
        &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (ordinal, code_unit) in encoded.into_iter().enumerate() {
        bytes[at + 4 + ordinal * 2..at + 6 + ordinal * 2].copy_from_slice(&code_unit.to_le_bytes());
    }
}

fn write_legacy_class_383_transform(bytes: &mut [u8], at: usize, transform: [[f64; 4]; 4]) {
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        bytes[at + ordinal * 8..at + ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn legacy_class_383_transform(translation_x: f64) -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, translation_x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn append_as_built_path_envelope(
    bytes: &mut Vec<u8>,
    scope_record_index: u32,
    locator_record_index: u32,
    operand_record_index: u32,
    occurrence_guid: &str,
    translation: [f64; 3],
) {
    let write_reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    let mut locator = vec![0_u8; 190];
    locator[..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"309");
    locator[7..11].copy_from_slice(&locator_record_index.to_le_bytes());
    write_reference(&mut locator, 21, operand_record_index);
    for (ordinal, value) in [
        1.0,
        0.0,
        0.0,
        translation[0],
        0.0,
        1.0,
        0.0,
        translation[1],
        0.0,
        0.0,
        1.0,
        translation[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ]
    .into_iter()
    .enumerate()
    {
        locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
    }
    write_reference(&mut locator, 162, scope_record_index);
    write_reference(&mut locator, 173, locator_record_index + 2);
    locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
    bytes.extend(locator);

    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"294");
    bytes.extend_from_slice(&u64::from(locator_record_index + 1).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    let identities = [
        occurrence_guid,
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    for guid in &identities[..3] {
        let encoded = guid.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(
            &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    }
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    for guid in &identities[3..] {
        let encoded = guid.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(
            &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);

    let wrapper_record_index = locator_record_index + 2;
    let path_record_index = locator_record_index + 1;
    let mut wrapper = vec![0_u8; 37];
    wrapper[..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"271");
    wrapper[7..11].copy_from_slice(&wrapper_record_index.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, path_record_index);
    bytes.extend(wrapper);
}

fn append_axial_test_utf16(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(
        &(u32::try_from(value.encode_utf16().count()).expect("fixture value fits u32"))
            .to_le_bytes(),
    );
    bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
}

fn write_axial_test_reference(bytes: &mut [u8], at: usize, target: u64) {
    bytes[at] = 1;
    bytes[at + 1..at + 9].copy_from_slice(&target.to_le_bytes());
    bytes[at + 9..at + 11].fill(0);
}

fn append_axial_test_selector(
    bytes: &mut Vec<u8>,
    axis_record_index: u32,
    occurrence_reference: u64,
    external_object_reference: u64,
    role: &str,
    versioned: bool,
) -> u32 {
    const ASSET: &str = "11111111-1111-1111-1111-111111111111";
    const CONTEXT: &str = "22222222-2222-2222-2222-222222222222";
    const PROPERTY: &str = "33333333-3333-3333-3333-333333333333";

    indexed_header(bytes, *b"316", axis_record_index);
    indexed_header(bytes, *b"261", axis_record_index);
    let selector_record_index = axis_record_index + 3;
    indexed_header(bytes, *b"277", selector_record_index);
    bytes.extend_from_slice(&[0; 11]);
    push_reference_u64(bytes, u64::from(selector_record_index + 3));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    append_axial_test_utf16(bytes, ASSET);
    append_axial_test_utf16(bytes, CONTEXT);
    for value in [2_u32, 0, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    push_reference_u64(bytes, occurrence_reference);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&external_object_reference.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    append_axial_test_utf16(bytes, ASSET);
    bytes.push(0);
    append_axial_test_utf16(bytes, "component-link");
    bytes.push(u8::from(versioned));
    if versioned {
        append_axial_test_utf16(bytes, PROPERTY);
        append_axial_test_utf16(bytes, "urn:test:version:2");
    }
    indexed_header(bytes, *b"261", selector_record_index);
    indexed_header(bytes, *b"298", selector_record_index + 5);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    append_axial_test_utf16(bytes, role);
    selector_record_index
}

fn append_axial_test_component_operand(
    bytes: &mut Vec<u8>,
    construction_record_index: u32,
    axis_record_indices: [u32; 2],
    transform: [[f64; 4]; 4],
    external_object_reference: u64,
    role: &str,
    versioned: bool,
) -> Vec<u32> {
    let first_selector = append_axial_test_selector(
        bytes,
        axis_record_indices[0],
        10_001,
        external_object_reference,
        role,
        versioned,
    );
    let second_selector = append_axial_test_selector(
        bytes,
        axis_record_indices[1],
        10_002,
        external_object_reference,
        role,
        versioned,
    );
    let construction_at = bytes.len();
    indexed_header(bytes, *b"305", construction_record_index);
    bytes.resize(construction_at + 380, 0);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = construction_at + 48 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    write_axial_test_reference(
        bytes,
        construction_at + 192,
        u64::from(axis_record_indices[0]),
    );
    write_axial_test_reference(
        bytes,
        construction_at + 208,
        u64::from(axis_record_indices[1]),
    );
    indexed_header(bytes, *b"261", construction_record_index);
    vec![
        axis_record_indices[0],
        first_selector,
        axis_record_indices[1],
        second_selector,
        construction_record_index,
    ]
}

fn axial_test_alignment(transforms: [[[f64; 4]; 4]; 2]) -> DesignAssemblyAlignment {
    DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        vec![
            Located {
                value: 90,
                offset: 1,
            },
            Located {
                value: 91,
                offset: 2,
            },
        ],
        Some(DesignAssemblyAlignmentForm::Frames {
            frames: [
                DesignAssemblyOperandFrame {
                    reference_record_index: 70,
                    reference_offset: 1,
                    transform: transforms[0].try_into().unwrap(),
                    transform_offset: 2,
                },
                DesignAssemblyOperandFrame {
                    reference_record_index: 80,
                    reference_offset: 3,
                    transform: transforms[1].try_into().unwrap(),
                    transform_offset: 4,
                },
            ],
        }),
    )
    .unwrap()
}

fn axial_test_component_scope(record_index: u32, role: &str) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:component-insert#{record_index}"),
        DesignFeatureKind::ComponentInsert,
        record_index,
    );
    if let DesignScopePayloadMut::ComponentInsert(slot) = scope.payload_mut() {
        *slot = Some(DesignComponentInsertConstruction {
            relation_record_index: record_index + 1,
            carrier_record_index: record_index + 2,
            occurrence_identity: None,
            neutron_role: role.into(),
            neutron_role_offset: 0,
            placement: Some(DesignComponentInsertMatrix {
                scope: Located {
                    value: SketchPlacementMatrix::IDENTITY,
                    offset: 0,
                },
                carrier_offset: Some(0),
            }),
        });
    }
    scope
}

mod axial;

mod compact_wide;

mod legacy_alignment;

mod rigid_as_built;
