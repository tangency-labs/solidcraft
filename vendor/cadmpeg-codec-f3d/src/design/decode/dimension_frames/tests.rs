// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::u64_from_index;

use super::{
    contiguous_i32_program, indexed_record_containing, parse_dimension_annotation_frame,
    parse_dimension_presentation_frame, recipe_record_prefix,
};
use crate::design::test_support::{push_genesis_block, push_reference};
use crate::records::sketch_links::PersistentSubentityTag;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{EdgeId, FaceId};

use std::collections::{HashMap, HashSet};

const TEST_LINEAR_TOLERANCE: f64 = 1.0e-6;

#[test]
fn dimension_recipe_uses_its_immediate_indexed_record_boundary() {
    let mut bytes = vec![0xaa; 5];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"415");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let recipe_offset = bytes.len();
    bytes.extend_from_slice(b"edge_recipe_data");
    bytes.extend_from_slice(&[0; 13]);
    let next_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"423");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);

    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), recipe_offset),
        Some((5, "415", 40, next_offset))
    );
    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), next_offset + 11),
        Some((next_offset, "423", 41, bytes.len()))
    );
    assert_eq!(indexed_record_containing(&bytes, 6, bytes.len(), 7), None);
    assert_eq!(
        contiguous_i32_program(&[u8::MAX; 8], 0, 8),
        Some(vec![-1, -1])
    );
    assert_eq!(contiguous_i32_program(&[0; 7], 0, 7), None);

    let mut framed = vec![0; 11];
    framed.extend_from_slice(&[7, 8, 9]);
    framed.extend_from_slice(&16u32.to_le_bytes());
    let family_name_offset = framed.len();
    framed.extend_from_slice(b"edge_recipe_data");
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        Some((11, &[7, 8, 9][..]))
    );
    framed[14..18].copy_from_slice(&15u32.to_le_bytes());
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        None
    );
}

#[test]
fn standard_recipe_references_refuse_nested_items_and_token_text() {
    let mut prefix = vec![0; 10];
    for word in [1u32, 3, 4, 1] {
        prefix.extend_from_slice(&word.to_le_bytes());
    }
    prefix.extend_from_slice(b"7");
    prefix.extend_from_slice(&[0; 4]);
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&331u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 8]);
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 0).len(),
        1
    );
}

#[test]
fn paired_recipe_references_refuse_operand_and_flattened_runs() {
    let mut prefix = vec![0; 10];
    for word in [1u32, 2, 1, 1] {
        prefix.extend_from_slice(&word.to_le_bytes());
    }
    prefix.extend_from_slice(b"2");
    prefix.extend_from_slice(&[0; 4]);
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&305u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(b"2");
    prefix.extend_from_slice(&[0; 4]);
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&305u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 4]);
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 0).len(),
        2
    );
}

#[test]
fn grouped_recipe_references_refuse_output_run() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    for selector in 1u32..=4 {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(b"2");
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&(300 + selector).to_le_bytes());
    }
    prefix.extend_from_slice(&[0; 4]);
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 0).len(),
        4
    );
}

#[test]
fn dimension_recipe_decodes_ordered_persistent_reference_entries() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let first_token_at = prefix.len();
    prefix.extend_from_slice(b"13");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let first_reference_at = prefix.len();
    prefix.extend_from_slice(&331u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_token_at = prefix.len();
    prefix.extend_from_slice(&[b'9', 0, 0, 0]);
    prefix.push(0);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_reference_at = prefix.len();
    prefix.extend_from_slice(&303u32.to_le_bytes());
    let third_reference_at = prefix.len();
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        { crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000) };
    assert_eq!(references.len(), 3);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token, "13");
    assert_eq!(
        references[0].token_offset,
        1_000 + u64_from_index(first_token_at)
    );
    assert_eq!(references[0].design_reference, 331);
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + u64_from_index(first_reference_at)
    );
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].selector_offset, 1_048);
    assert_eq!(references[1].token, "9");
    assert_eq!(
        references[1].token_offset,
        1_000 + u64_from_index(second_token_at)
    );
    assert_eq!(references[1].design_reference, 303);
    assert_eq!(
        references[1].design_reference_offset,
        1_000 + u64_from_index(second_reference_at)
    );
    assert_eq!(references[2].selector, 2);
    assert_eq!(references[2].token, "9");
    assert_eq!(references[2].design_reference, 304);
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + u64_from_index(third_reference_at)
    );
    let suffix_at = prefix.len() - 4;
    prefix.splice(
        suffix_at..,
        [1u32, 1, 0, 0, 2, 401, 402, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000),
        references
    );
    prefix.extend_from_slice(&[0; 2]);
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000),
        references
    );
    let tags = [
        PersistentSubentityTag {
            id: "matching".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-b").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "other".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-a").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![999],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-face".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-c").expect("identity grammar"),
            ),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "matching-edge".into(),
            target: AttributeTarget::Edge(
                EdgeId::mint("test:model:edge#edge-b").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-edge".into(),
            target: AttributeTarget::Edge(
                EdgeId::mint("test:model:edge#edge-c").expect("identity grammar"),
            ),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    let mut bound = references[0].clone();
    {
        crate::design::decode::dimension_frames::bind_recipe_reference_candidates(
            &mut bound, &tags, None,
        );
    };
    assert_eq!(
        bound.candidate_faces,
        [FaceId::mint("test:model:face#face-b").expect("identity grammar")]
    );
    assert_eq!(
        bound.candidate_edges,
        [EdgeId::mint("test:model:edge#edge-b").expect("identity grammar")]
    );
    assert_eq!(
        bound.alternate_selector_faces,
        [FaceId::mint("test:model:face#face-c").expect("identity grammar")]
    );
    assert_eq!(
        bound.alternate_selector_edges,
        [EdgeId::mint("test:model:edge#edge-c").expect("identity grammar")]
    );
    let stream_tags = [
        PersistentSubentityTag {
            id: "f3d:xref/A/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-a").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:xref/B/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-b").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    {
        crate::design::decode::dimension_frames::bind_recipe_reference_candidates(
            &mut bound,
            &stream_tags,
            Some("f3d:xref/A/occurrence-0/Asset/Design1/BulkStream.dat:dimension-recipe#1"),
        );
    };
    assert_eq!(
        bound.candidate_faces,
        [FaceId::mint("test:model:face#face-a").expect("identity grammar")]
    );
}

#[test]
fn dimension_recipe_decodes_signed_decimal_reference_tokens() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());

    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(b"-2");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&301u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(b"-1");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        { crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000) };
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].token, "-2");
    assert_eq!(references[0].design_reference, 301);
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].token, "-1");
    assert_eq!(references[1].design_reference, 304);
}

#[test]
fn face_recipe_decodes_paired_packed_reference_runs() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let mut reference_offsets = Vec::new();
    for (ordinal, token) in b"23".iter().copied().enumerate() {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        if ordinal == 0 {
            prefix.push(token);
        } else {
            prefix.extend_from_slice(&1u32.to_le_bytes());
            prefix.push(token);
        }
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&2u32.to_le_bytes());
        reference_offsets.push(prefix.len());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&312u32.to_le_bytes());
        if ordinal == 1 {
            prefix.extend_from_slice(&0u32.to_le_bytes());
        }
    }

    let references =
        { crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000) };
    assert!(crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&prefix));
    assert_eq!(references.len(), 4);
    assert_eq!(
        references
            .iter()
            .map(|reference| (reference.selector, reference.token.as_str()))
            .collect::<Vec<_>>(),
        [(1, "2"), (1, "2"), (1, "3"), (1, "3")]
    );
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.design_reference)
            .collect::<Vec<_>>(),
        [305, 312, 305, 312]
    );
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + u64_from_index(reference_offsets[0])
    );
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + u64_from_index(reference_offsets[1])
    );

    let second_operand_at = reference_offsets[0] + 8;
    let mut packed_second = prefix.clone();
    packed_second.drain(second_operand_at + 4..second_operand_at + 8);
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&packed_second, 1_000)
            .is_empty()
    );
    let mut invalid_header = prefix.clone();
    invalid_header[0] = 1;
    assert!(
        !crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&invalid_header)
    );

    let mut trailing = prefix.clone();
    trailing.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&trailing, 1_000)
            .is_empty()
    );
    assert!(!crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&trailing));

    let mut mismatched_selector = prefix.clone();
    mismatched_selector[second_operand_at..second_operand_at + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &mismatched_selector,
            1_000,
        )
        .is_empty()
    );

    let second_run_at = reference_offsets[1];
    prefix[second_run_at..second_run_at + 4].copy_from_slice(&306u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000)
            .is_empty()
    );
    assert!(!crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&prefix));
}

#[test]
fn face_recipe_decodes_five_group_reference_sequence() {
    fn operand(prefix: &mut Vec<u8>, selector: u32, token: &str, references: &[u32]) {
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(
            &u32::try_from(references.len())
                .expect("synthetic reference count")
                .to_le_bytes(),
        );
        for reference in references {
            prefix.extend_from_slice(&reference.to_le_bytes());
        }
    }

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    operand(&mut prefix, 2, "-4", &[401, 402]);
    let first_operand_end = prefix.len();
    operand(&mut prefix, 3, "8", &[501]);

    let second_group_count_at = prefix.len();
    prefix.extend_from_slice(&1u32.to_le_bytes());
    operand(&mut prefix, 4, "-9", &[601, 602]);

    for (selector, token, reference) in [(5, "7", 701), (6, "-1", 801), (7, "0", 901)] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        operand(&mut prefix, selector, token, &[reference]);
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        { crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000) };
    assert!(crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&prefix));
    assert_eq!(
        references
            .iter()
            .map(|reference| {
                (
                    reference.selector,
                    reference.token.as_str(),
                    reference.design_reference,
                )
            })
            .collect::<Vec<_>>(),
        [
            (2, "-4", 401),
            (2, "-4", 402),
            (3, "8", 501),
            (4, "-9", 601),
            (4, "-9", 602),
            (5, "7", 701),
            (6, "-1", 801),
            (7, "0", 901),
        ]
    );
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token_offset, 1_026);
    assert_eq!(references[0].design_reference_offset, 1_036);
    assert_eq!(references[1].design_reference_offset, 1_040);

    let mut wrong_first_group_count = prefix.clone();
    wrong_first_group_count[18..22].copy_from_slice(&3u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &wrong_first_group_count,
            1_000,
        )
        .is_empty()
    );
    let mut wrong_group_count = prefix.clone();
    wrong_group_count[14..18].copy_from_slice(&4u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &wrong_group_count,
            1_000
        )
        .is_empty()
    );
    let mut empty_group = prefix.clone();
    empty_group[second_group_count_at..second_group_count_at + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&empty_group, 1_000)
            .is_empty()
    );
    let mut length_prefixed_token = prefix.clone();
    length_prefixed_token.splice(26..26, 2u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &length_prefixed_token,
            1_000,
        )
        .is_empty()
    );
    let mut locally_terminated_operand = prefix.clone();
    locally_terminated_operand.splice(first_operand_end..first_operand_end, 0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &locally_terminated_operand,
            1_000,
        )
        .is_empty()
    );
    let mut trailing = prefix.clone();
    trailing.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&trailing, 1_000)
            .is_empty()
    );
    assert!(!crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&trailing));
}

#[test]
fn face_recipe_decodes_dynamic_group_reference_sequence() {
    fn operand(prefix: &mut Vec<u8>, selector: u32, token: &str, reference: u32) {
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&reference.to_le_bytes());
    }

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    for (selector, token, reference) in [
        (1, "97", 302),
        (2, "88", 302),
        (3, "10", 302),
        (1, "8", 302),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        operand(&mut prefix, selector, token, reference);
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        { crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000) };
    assert!(crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&prefix));
    assert_eq!(
        references
            .iter()
            .map(|reference| (
                reference.selector,
                reference.token.as_str(),
                reference.design_reference
            ))
            .collect::<Vec<_>>(),
        [
            (1, "97", 302),
            (2, "88", 302),
            (3, "10", 302),
            (1, "8", 302)
        ]
    );
}

#[test]
fn dimension_recipe_rejects_non_decimal_reference_tokens() {
    for token in [b"-".as_slice(), b"+1", b"1-"] {
        let mut prefix = vec![0; 10];
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&3u32.to_le_bytes());
        prefix.extend_from_slice(&4u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(
            &u32::try_from(token.len())
                .expect("synthetic token length")
                .to_le_bytes(),
        );
        prefix.extend_from_slice(token);
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&301u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());

        assert!(
            crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000)
                .is_empty(),
            "accepted token {token:?}"
        );
    }
}

#[test]
fn dimension_annotation_frame_links_nullable_loci_to_governing_owner() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"298");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for (reference, role) in [(0u32, 6u32), (354, 2), (376, 3)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    push_genesis_block(&mut bytes, 0x202);
    let annotation_byte_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    push_reference(&mut bytes, 390);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [376u32, 354] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let paired_byte_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"287");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes.resize(paired_byte_offset + 59, 0);

    let frame = parse_dimension_annotation_frame(
        &bytes,
        0,
        Some(383),
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("annotated dimension frame");
    assert_eq!(frame.companion_record_index, Some(383));
    assert_eq!(frame.governing_companion_record_index, 391);
    assert_eq!(frame.entity_genesis(), Some(0x202));
    assert_eq!(
        frame.annotation_byte_offset(),
        u64_from_index(annotation_byte_offset)
    );
    assert_eq!(
        frame.clone().into_draft().annotation_bytes,
        [0xaa, 0xbb, 0xcc]
    );
    assert_eq!(frame.operands()[0].geometry_record_index, None);
    assert_eq!(
        frame
            .clone()
            .into_draft()
            .return_members
            .iter()
            .map(|member| member.value.get())
            .collect::<Vec<_>>(),
        [376, 354]
    );
    assert_eq!(
        frame.paired_byte_offset(),
        u64_from_index(paired_byte_offset)
    );
    assert_eq!(frame.owner_reference, 201);

    let leading = parse_dimension_annotation_frame(
        &bytes,
        0,
        None,
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("scope-prefix dimension frame");
    assert_eq!(leading.companion_record_index, None);
    assert_eq!(leading.governing_owner_record_index, 390);
}

#[test]
fn dimension_presentation_frame_requires_registered_geometry_and_paired_sketch_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&332u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (reference, role) in [(306u32, 1u32), (331, 0)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    let presentation_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let paired_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&332u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&270u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 35]);

    let frame = parse_dimension_presentation_frame(
        &bytes,
        0,
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        &HashSet::from([306, 331]),
        &HashSet::from([270]),
        &HashSet::from([281]),
    )
    .expect("direct dimension presentation frame");
    assert_eq!(frame.class_tag.as_str(), "314");
    assert_eq!(frame.record_index, 332);
    assert_eq!(frame.frame_length, u64_from_index(paired_offset));
    assert_eq!(
        frame.presentation_byte_offset,
        u64_from_index(presentation_offset)
    );
    assert_eq!(frame.presentation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(frame.operands[0].geometry_record_index.get(), 306);
    assert_eq!(frame.operands[1].geometry_record_index.get(), 331);
    assert_eq!(frame.owner_reference, 270);

    assert!(parse_dimension_presentation_frame(
        &bytes,
        0,
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        &HashSet::from([306]),
        &HashSet::from([270]),
        &HashSet::from([281])
    )
    .is_none());
}

#[test]
fn companion_interval_ends_before_a_foreign_scope_member() {
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#11".into(),
        0,
        crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    );
    let scope = crate::records::feature::scope::DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:native:parameter-scope#12".into(),
            byte_offset: 80,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 180,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 89,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![55],
                vec![100],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::Extrude
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 280,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let header = crate::records::decal::DesignRecordHeader {
        id: "f3d:native:record-header#55".into(),
        record_index: 55,
        class_tag: crate::records::references::DesignClassTag::try_from("302".to_owned()).unwrap(),
        byte_offset: 70,
    };
    let admitted = super::companion_owned_interval(
        &companion,
        std::iter::empty(),
        &[],
        &[scope],
        &[header],
        100,
    );
    assert_eq!(admitted, Some((58, 70)));
}

#[test]
fn dimension_annotation_interval_of_zero_bytes_has_no_frames() {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{}:parameter-companion#0", crate::ids::native_scope(STREAM)),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&[0; 58]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let companions = [companion];
        let inputs = super::DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &[],
            owners: &[],
            companions: &companions,
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let admitted = super::decode_dimension_annotation_frames(&inputs, &[]).unwrap();
        assert!(admitted.is_empty());
    });
}

mod loci;

#[test]
fn recipe_references_preserve_signed_design_keys_and_refuse_truncation() {
    let mut prefix = vec![0; 10];
    for word in [1u32, 3, 1, 2, 2] {
        prefix.extend_from_slice(&word.to_le_bytes());
    }
    prefix.extend_from_slice(b"37");
    for word in [0i32, 1, -73, 0, 0] {
        prefix.extend_from_slice(&word.to_le_bytes());
    }
    let references = super::decode_recipe_references(&prefix, 0);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].design_reference, -73);
    let face = FaceId::mint("test:model:face#signed").unwrap();
    let tag = PersistentSubentityTag {
        id: "signed-key".into(),
        target: AttributeTarget::Face(face.clone()),
        selector: 2,
        token: cadmpeg_core::text::NonBlankString::new("37").unwrap(),
        design_references: vec![-73],
        ordinal: 0,
    };
    let mut bound = references[0].clone();
    super::bind_recipe_reference_candidates(&mut bound, &[tag], None);
    assert_eq!(bound.candidate_faces, [face]);
    for end in 0..prefix.len() {
        assert!(super::decode_recipe_references(&prefix[..end], 0).is_empty());
    }
}

mod metadata;
