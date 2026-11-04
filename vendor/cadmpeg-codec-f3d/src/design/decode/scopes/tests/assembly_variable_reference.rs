// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment;
use crate::design::test_support::assembly_operand_frame_fixture;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::records::feature::scope::DesignParameterScope;

#[test]
fn variable_reference_assembly_uses_fixed_alignment_lanes() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Assemble,
        scope_record_index,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("283".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("264".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 637;
            draft.paired_byte_offset = 637;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                200, 201, 202, 203, 108, 109, 110, 111, 204,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owners = (0_u32..12)
        .map(|local_ordinal| {
            crate::records::parameters::DesignParameterOwner::try_from(
                crate::records::parameters::DesignParameterOwnerWire {
                    id: format!(
                        "f3d:Design/BulkStream.dat:design-parameter-owner#{}",
                        100 + local_ordinal
                    ),
                    byte_offset: (u64::from(1_000 + local_ordinal)) - 40,
                    frame_length: 103,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "289".to_owned(),
                    )
                    .unwrap(),
                    record_index: 100 + local_ordinal,
                    scope_record_index,
                    local_ordinal,
                    evaluated_value: f64::from(local_ordinal),
                    evaluated_value_offset: u64::from(1_000 + local_ordinal),
                    parameter_record_index: 100 + local_ordinal + 1,
                    owned_ordinal: local_ordinal,
                    variant: None,
                    companion_record_index: 100 + local_ordinal + 2,
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut bytes = assembly_operand_frame_fixture(scope_record_index);
    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("variable-reference assembly alignment");
    assert_eq!(alignment.angle(), 8.0);
    assert_eq!(alignment.offset(), [9.0, 10.0, 11.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [108, 109, 110, 111]
    );
    assert!(alignment.operand_frames().is_some());

    let write_reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    bytes[362..366].copy_from_slice(&2_u32.to_le_bytes());
    write_reference(&mut bytes, 366, 64);
    write_reference(&mut bytes, 377, 67);
    let append_locator = |bytes: &mut Vec<u8>, record_index: u32, wrapper_index: u32| {
        let start = bytes.len();
        bytes.resize(start + variable_path_locator::LEN, 0);
        bytes[start..start + 4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[start + 4..start + 7].copy_from_slice(b"390");
        bytes[start + 7..start + 11].copy_from_slice(&record_index.to_le_bytes());
        for ordinal in 0..16 {
            let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
            let at = start + variable_path_locator::TRANSFORM + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        write_reference(
            bytes,
            start + variable_path_locator::SCOPE_BACKLINK,
            scope_record_index,
        );
        write_reference(
            bytes,
            start + variable_path_locator::WRAPPER_REFERENCE,
            wrapper_index,
        );
        bytes[start + variable_path_locator::CONSTANT_TWO
            ..start + variable_path_locator::CONSTANT_TWO + 4]
            .copy_from_slice(&2_u32.to_le_bytes());
    };
    let append_path = |bytes: &mut Vec<u8>, record_index: u32, guid: &str| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"330");
        bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        let append_guid = |bytes: &mut Vec<u8>| {
            let encoded = guid.encode_utf16().collect::<Vec<_>>();
            bytes.extend_from_slice(
                &(u32::try_from(encoded.len()).expect("fixture value fits u32")).to_le_bytes(),
            );
            bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
        };
        append_guid(bytes);
        for _ in 0..2 {
            append_guid(bytes);
        }
        bytes.extend_from_slice(&2_u64.to_le_bytes());
        for _ in 0..2 {
            append_guid(bytes);
        }
        bytes.extend_from_slice(&2_u32.to_le_bytes());
    };
    let append_wrapper = |bytes: &mut Vec<u8>, record_index: u32, paths: &[u32]| {
        let start = bytes.len();
        bytes.resize(
            start + path_wrapper::LEN + paths.len().saturating_sub(1) * 11,
            0,
        );
        bytes[start..start + 4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[start + 4..start + 7].copy_from_slice(b"397");
        bytes[start + 7..start + 11].copy_from_slice(&record_index.to_le_bytes());
        bytes[start + path_wrapper::CONSTANT_ONE_BYTE] = 1;
        bytes[start + path_wrapper::CONSTANT_ONE_WORD..start + path_wrapper::CONSTANT_ONE_WORD + 4]
            .copy_from_slice(
                &(u32::try_from(paths.len()).expect("fixture value fits u32")).to_le_bytes(),
            );
        write_reference(bytes, start + path_wrapper::PATH_REFERENCE, paths[0]);
        for (ordinal, path) in paths.iter().copied().enumerate().skip(1) {
            write_reference(bytes, start + path_wrapper::LEN + (ordinal - 1) * 11, path);
        }
    };
    append_locator(&mut bytes, 64, 66);
    append_path(&mut bytes, 65, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    append_wrapper(&mut bytes, 66, &[65]);
    append_locator(&mut bytes, 67, 70);
    append_path(&mut bytes, 68, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
    append_path(&mut bytes, 69, "cccccccc-cccc-cccc-cccc-cccccccccccc");
    append_wrapper(&mut bytes, 70, &[68, 69]);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&71_u32.to_le_bytes());
    let paths = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .and_then(|alignment| alignment.operand_paths())
    .expect("variable-reference compact operand paths");
    assert_eq!(
        paths.each_ref().map(|path| path.class_tag().as_str()),
        ["330", "330"]
    );
    assert_eq!(paths[0].link().locator_class_tag.as_str(), "390");
    assert_eq!(paths[0].link().wrapper_class_tag.as_str(), "397");
    assert_eq!(paths[1].occurrence_guids().len(), 2);

    let mut wrong_generation = scope.clone();
    wrong_generation.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
    assert!(exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &wrong_generation,
        &owners,
    )
    .unwrap()
    .is_none());
}
