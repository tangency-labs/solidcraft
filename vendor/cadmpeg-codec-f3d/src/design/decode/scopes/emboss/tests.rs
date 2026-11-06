// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::emboss::{DesignEmbossConstruction, DesignEmbossControlWords};

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::test_support::lp_utf16;

fn exact_emboss_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignEmbossConstruction> {
    crate::test_support::with_decode_context(|ctx| {
        super::exact_emboss_construction(ctx, bytes, scope).unwrap()
    })
}

fn reference(bytes: &mut Vec<u8>, index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(index).to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
}

fn fixture(start: usize) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; start];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"900");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for index in [60, 72, 63] {
        reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&17u64.to_le_bytes());
    bytes.extend_from_slice(&23u64.to_le_bytes());
    for index in [69, 48, 66] {
        reference(&mut bytes, index);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 41);
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 48);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    assert_eq!(bytes.len() - start, 235);
    bytes.resize(start + 430, 0);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Emboss,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64::try_from(start).unwrap();
            draft.class_tag = "900".to_owned().try_into().unwrap();
            draft.paired_class_tag = "901".to_owned().try_into().unwrap();
            draft.frame_length = 430;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = draft.byte_offset + 235;
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 60, 63, 66, 69, 72]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn emboss_prefix_retains_control_words_and_locates_all_owner_and_group_links() {
    for start in [0, 16] {
        let (bytes, scope) = fixture(start);
        let decoded = exact_emboss_construction(&bytes, &scope).unwrap();
        assert_eq!(decoded.control_words, DesignEmbossControlWords::Two([1, 0]));
        assert_eq!(decoded.native_pair, Some([17, 23]));
        assert_eq!(decoded.depth_owner.value, 60);
        assert_eq!(decoded.auxiliary_group.value, 72);
        assert_eq!(decoded.horizontal_owner.value, 63);
        assert_eq!(decoded.rotation_owner.value, 69);
        assert_eq!(decoded.face_group.value, 48);
        assert_eq!(decoded.vertical_owner.value, 66);
        assert_eq!(decoded.profile_group.value, 41);
        assert_eq!(
            decoded.depth_owner.offset,
            u64::try_from(start).unwrap() + 29
        );
        assert_eq!(
            decoded.profile_group.offset,
            u64::try_from(start).unwrap() + 119
        );
        assert!(decoded.links_match(&scope));
    }
}

#[test]
fn emboss_prefix_refuses_invalid_counts_references_zero_runs_and_guid() {
    let (bytes, scope) = fixture(0);
    for offset in [
        0, 4, 7, 11, 28, 29, 33, 37, 61, 114, 129, 141, 146, 156, 160, 232,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 2;
        assert!(
            exact_emboss_construction(&changed, &scope).is_none(),
            "offset {offset}"
        );
    }
    for length in 0..bytes.len() {
        assert!(exact_emboss_construction(&bytes[..length], &scope).is_none());
    }
}

#[test]
fn emboss_prefix_keeps_unassigned_control_values_native() {
    let (mut bytes, scope) = fixture(0);
    bytes[20..24].copy_from_slice(&7u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&9u32.to_le_bytes());
    let decoded = exact_emboss_construction(&bytes, &scope).unwrap();
    assert_eq!(decoded.control_words, DesignEmbossControlWords::Two([7, 9]));
}

#[test]
fn emboss_prefix_refuses_reference_table_reordering() {
    let (bytes, mut scope) = fixture(0);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 63, 60, 66, 69, 72]);
            draft.layout_fixture_references();
        })
        .unwrap();
    assert!(exact_emboss_construction(&bytes, &scope).is_none());
}

fn counted_fixture(
    start: usize,
    one_word: bool,
    pair_count: u32,
    reference_count: usize,
) -> (Vec<u8>, DesignParameterScope) {
    let ids = (0..reference_count)
        .map(|index| 100_001 + 7 * u32::try_from(index).unwrap())
        .collect::<Vec<_>>();
    let fields = [ids[0], ids[2], ids[4], ids[6], ids[8], ids[10], ids[12]];
    let mut bytes = vec![0; start];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"900");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    if !one_word {
        bytes.extend_from_slice(&7u32.to_le_bytes());
    }
    bytes.extend_from_slice(&9u32.to_le_bytes());
    for index in &fields[..3] {
        reference(&mut bytes, *index);
    }
    bytes.extend_from_slice(&pair_count.to_le_bytes());
    for index in 0..pair_count {
        bytes.extend_from_slice(&(8_000_003 + u64::from(index)).to_le_bytes());
        bytes.extend_from_slice(&(9_000_007 + u64::from(index)).to_le_bytes());
    }
    for index in &fields[3..6] {
        reference(&mut bytes, *index);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, fields[6]);
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, fields[4]);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    let count_offset = bytes.len();
    bytes.resize(start + 2048, 0);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Emboss,
        40,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64::try_from(start).unwrap();
            draft.class_tag = "900".to_owned().try_into().unwrap();
            draft.paired_class_tag = "901".to_owned().try_into().unwrap();
            draft.frame_length = 2048;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = u64::try_from(count_offset).unwrap();
            draft.reference_members = ReferenceRun::unlocated(ids);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn emboss_counted_prefix_preserves_all_pairs_and_both_control_lanes() {
    for start in [0, 24] {
        for (one_word, pairs, references) in
            [(true, 2, 13), (false, 2, 19), (true, 5, 19), (false, 5, 13)]
        {
            let (bytes, scope) = counted_fixture(start, one_word, pairs, references);
            let construction = exact_emboss_construction(&bytes, &scope).unwrap();
            assert_eq!(
                construction.control_words,
                if one_word {
                    DesignEmbossControlWords::One([9])
                } else {
                    DesignEmbossControlWords::Two([7, 9])
                }
            );
            assert_eq!(construction.native_pair, Some([8_000_003, 9_000_007]));
            let additional = construction.additional_native_pairs.as_ref().unwrap();
            assert_eq!(additional.len(), usize::try_from(pairs - 1).unwrap());
            for (index, pair) in additional.iter().enumerate() {
                let ordinal = u64::try_from(index).unwrap() + 1;
                assert_eq!(*pair, [8_000_003 + ordinal, 9_000_007 + ordinal]);
            }
            let suffix = if one_word { 61 } else { 65 } + 16 * u64::from(pairs);
            assert_eq!(
                construction.rotation_owner.offset,
                scope.byte_offset() + suffix + 1
            );
            assert_eq!(
                construction.profile_group.offset,
                scope.byte_offset() + suffix + 38
            );
            assert!(construction.links_match(&scope));
            assert!(!construction.has_fixed_operands(&scope));
        }
    }
}

#[test]
fn emboss_counted_prefix_refuses_invalid_counts_fields_and_truncation() {
    for (one_word, pairs) in [(true, 2), (false, 2), (true, 5), (false, 5)] {
        let (bytes, scope) = counted_fixture(0, one_word, pairs, 13);
        let head = if one_word { 61 } else { 65 };
        let suffix = head + 16 * usize::try_from(pairs).unwrap();
        for offset in [
            11,
            head - 37,
            head - 36,
            suffix,
            suffix + 1,
            suffix + 33,
            suffix + 37,
            suffix + 48,
            suffix + 60,
            suffix + 64,
            suffix + 65,
            suffix + 75,
            suffix + 79,
            suffix + 151,
        ] {
            let mut changed = bytes.clone();
            changed[offset] ^= 2;
            assert!(
                exact_emboss_construction(&changed, &scope).is_none(),
                "offset {offset}"
            );
        }
        for count in [0u32, u32::MAX] {
            let mut changed = bytes.clone();
            changed[head - 4..head].copy_from_slice(&count.to_le_bytes());
            assert!(exact_emboss_construction(&changed, &scope).is_none());
        }
        for length in 0..bytes.len() {
            assert!(exact_emboss_construction(&bytes[..length], &scope).is_none());
        }
    }
}

#[test]
fn emboss_one_pair_prefix_can_retain_another_reference_order_as_native() {
    let (bytes, mut scope) = fixture(0);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                ReferenceRun::unlocated(vec![41, 44, 48, 51, 60, 63, 66, 69, 72, 80, 83]);
            draft.frame_length = 2048;
            draft.paired_byte_offset = 2048;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut bytes = bytes;
    bytes.resize(2048, 0);
    let construction = exact_emboss_construction(&bytes, &scope).unwrap();
    assert_eq!(construction.native_pair, Some([17, 23]));
    assert!(construction.additional_native_pairs.is_none());
    assert!(!construction.has_fixed_operands(&scope));
}

#[test]
fn emboss_empty_pair_run_has_no_invented_first_pair() {
    for one_word in [true, false] {
        let (bytes, scope) = counted_fixture(0, one_word, 0, 13);
        let construction = exact_emboss_construction(&bytes, &scope).unwrap();
        assert!(construction.native_pair.is_none());
        assert!(construction.additional_native_pairs.is_none());
        assert!(construction.links_match(&scope));
        assert!(!construction.has_fixed_operands(&scope));
    }
}
