// SPDX-License-Identifier: Apache-2.0

use super::{parse_dimension_annotation_frame, push_genesis_block, push_reference};
use std::collections::{HashMap, HashSet};

fn annotation_bytes(genesis: Option<u64>) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"298", 388);
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 3);
    for (reference, role) in [(0, 6u32), (354, 2), (376, 3)] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    if let Some(genesis) = genesis {
        push_genesis_block(&mut bytes, genesis);
    } else {
        push_reference(&mut bytes, 0);
    }
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    push_reference(&mut bytes, 390);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [376, 354] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let paired = bytes.len();
    crate::test_support::indexed_header(&mut bytes, *b"287", 388);
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    (bytes, paired)
}

fn parse(bytes: &[u8]) -> Option<crate::records::dimensions::DesignDimensionAnnotationFrame> {
    parse_dimension_annotation_frame(
        bytes,
        0,
        Some(383),
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
}

#[test]
fn dimension_annotation_empty_metadata_binds_explicit_owner_and_operands() {
    let (bytes, paired) = annotation_bytes(None);
    let frame = parse(&bytes).unwrap();
    assert_eq!(frame.entity_genesis(), None);
    assert_eq!(frame.companion_record_index, Some(383));
    assert_eq!(frame.governing_owner_record_index, 390);
    assert_eq!(frame.governing_companion_record_index, 391);
    assert_eq!(frame.annotation_byte_offset(), 74);
    assert_eq!(frame.paired_byte_offset(), u64::try_from(paired).unwrap());
    let draft = frame.into_draft();
    assert_eq!(draft.annotation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(
        draft
            .operands
            .iter()
            .map(|operand| operand.geometry_record_index.map(std::num::NonZeroU32::get))
            .collect::<Vec<_>>(),
        [None, Some(354), Some(376)]
    );
    assert_eq!(
        draft
            .return_members
            .iter()
            .map(|member| member.value.get())
            .collect::<Vec<_>>(),
        [376, 354]
    );
}

#[test]
fn dimension_annotation_metadata_refuses_mutations_and_truncation() {
    for genesis in [None, Some(0x202)] {
        let (bytes, paired) = annotation_bytes(genesis);
        assert!(parse(&bytes).is_some());
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).is_none(), "truncation at {end}");
        }
        for (at, value) in [
            (69, 0),
            (70, 2),
            (40, 0xff),
            (paired + 7, 0xff),
            (paired + 20, 0xff),
        ] {
            let mut mutated = bytes.clone();
            mutated[at] = value;
            assert!(parse(&mutated).is_none(), "mutation at {at}");
        }
        let mut returned = bytes.clone();
        let return_first = if genesis.is_some() { 145 } else { 93 };
        returned[return_first..return_first + 4].copy_from_slice(&354u32.to_le_bytes());
        assert!(parse(&returned).is_none());
        if genesis.is_some() {
            let mut key = bytes.clone();
            key[78] = b'X';
            assert!(parse(&key).is_none());
        } else {
            let mut populated = bytes.clone();
            populated[70] = 1;
            assert!(parse(&populated).is_none());
        }
    }
}
