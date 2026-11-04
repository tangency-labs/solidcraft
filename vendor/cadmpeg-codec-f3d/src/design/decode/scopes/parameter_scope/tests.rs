// SPDX-License-Identifier: Apache-2.0

use super::first_marked_reference_offsets;
use super::named_parameter_scope_tail_is_valid;
use super::reference_table_ends_at;
use crate::test_support::lp_utf16;

fn named_scope_tail(lane_value: u64) -> Vec<u8> {
    named_scope_tail_with_class_word(lane_value, 0xfc)
}

fn named_scope_tail_with_class_word(lane_value: u64, class_word: u32) -> Vec<u8> {
    named_scope_tail_with_label("Canvas", lane_value, class_word)
}

fn named_scope_tail_with_label(label: &str, lane_value: u64, class_word: u32) -> Vec<u8> {
    let label_code_units = label.encode_utf16().count();
    let marker = 19 + label_code_units * 2;
    let mut bytes = vec![0; marker + 59];
    bytes[0..4].copy_from_slice(&1u32.to_le_bytes());
    let mut label_bytes = Vec::new();
    lp_utf16(&mut label_bytes, label);
    bytes[8..8 + label_bytes.len()].copy_from_slice(&label_bytes);
    bytes[marker] = 1;
    bytes[marker + 1] = 0xd5;
    bytes[marker + 2..marker + 10].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 12..marker + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[marker + 16..marker + 20].copy_from_slice(&class_word.to_le_bytes());
    bytes[marker + 20..marker + 28].copy_from_slice(&0.25f64.to_le_bytes());
    bytes[marker + 28..marker + 32].copy_from_slice(&class_word.to_le_bytes());
    bytes[marker + 32] = 1;
    bytes[marker + 33] = 0xd4;
    bytes[marker + 34..marker + 42].copy_from_slice(&lane_value.to_le_bytes());
    bytes[marker + 42..marker + 46].copy_from_slice(&[0, 1, 0, 0]);
    bytes[marker + 46] = 1;
    bytes[marker + 47] = 0xd3;
    bytes[marker + 48..marker + 56].copy_from_slice(&lane_value.to_le_bytes());
    bytes
}

#[test]
fn named_scope_tail_requires_one_repeated_byte_lane_value() {
    for lane_value in [0, 1, 8, 0xff] {
        let bytes = named_scope_tail(lane_value);
        assert_eq!(
            named_parameter_scope_tail_is_valid(&bytes, 0, bytes.len(), bytes.len()),
            Some(true)
        );
    }

    let mut mismatched = named_scope_tail(0);
    let marker = mismatched.len() - 59;
    mismatched[marker + 34..marker + 42].copy_from_slice(&1u64.to_le_bytes());
    assert_eq!(
        named_parameter_scope_tail_is_valid(&mismatched, 0, mismatched.len(), mismatched.len()),
        Some(false)
    );

    let outside_domain = named_scope_tail(0x100);
    assert_eq!(
        named_parameter_scope_tail_is_valid(
            &outside_domain,
            0,
            outside_domain.len(),
            outside_domain.len()
        ),
        Some(false)
    );
}

#[test]
fn empty_label_named_tail_keeps_the_binary_lane_and_class_252() {
    for (lane_value, class_word, admitted) in [
        (0, 0xfc, true),
        (1, 0xfc, true),
        (8, 0xfc, false),
        (1, 0x101, false),
        (8, 0x101, false),
    ] {
        let bytes = named_scope_tail_with_label("", lane_value, class_word);
        assert_eq!(
            named_parameter_scope_tail_is_valid(&bytes, 0, bytes.len(), bytes.len()),
            Some(admitted),
            "lane {lane_value} class {class_word}"
        );
    }
}

#[test]
fn named_scope_tail_admits_equal_three_digit_class_words_only() {
    for class_word in [100, 0xfc, 0x101, 999] {
        let bytes = named_scope_tail_with_class_word(8, class_word);
        assert_eq!(
            named_parameter_scope_tail_is_valid(&bytes, 0, bytes.len(), bytes.len()),
            Some(true)
        );
    }
    for class_word in [0, 99, 1000] {
        let bytes = named_scope_tail_with_class_word(8, class_word);
        assert_eq!(
            named_parameter_scope_tail_is_valid(&bytes, 0, bytes.len(), bytes.len()),
            Some(false)
        );
    }
    let mut unequal = named_scope_tail_with_class_word(8, 0x101);
    let marker = unequal.len() - 59;
    unequal[marker + 28..marker + 32].copy_from_slice(&0xfcu32.to_le_bytes());
    assert_eq!(
        named_parameter_scope_tail_is_valid(&unequal, 0, unequal.len(), unequal.len()),
        Some(false)
    );
}

#[test]
fn sketch_scope_reference_offsets_refuse_second_unique_marker() {
    let mut frame = vec![0; 24];
    for (at, suffix) in [(0, 42u32), (12, 43)] {
        frame[at] = 1;
        frame[at + 1..at + 5].copy_from_slice(&suffix.to_le_bytes());
    }
    let admitted = first_marked_reference_offsets(&frame);
    assert_eq!(admitted.get(&42), Some(&0));
    assert_eq!(admitted.get(&43), Some(&12));
}

#[test]
fn canvas_scope_admits_its_complete_98_byte_tail_and_rejects_broken_links() {
    use super::parse_parameter_scope;
    use crate::design::test_support::indexed_record_offsets_for_test;
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(b"300");
    prefix.extend_from_slice(&12u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 10]);
    prefix.push(1);
    prefix.extend_from_slice(&55u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 6]);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    for member in [55u32, 56] {
        prefix.push(1);
        prefix.extend_from_slice(&member.to_le_bytes());
        prefix.extend_from_slice(&[0; 6]);
    }
    prefix.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut prefix, "Canvas");
    let mut tail = [0; 98];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    tail[12..16].copy_from_slice(&1u32.to_le_bytes());
    for (at, index) in [(16, 12u32), (30, 3), (62, 4), (75, 5), (86, 12)] {
        tail[at] = 1;
        tail[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }
    tail[42..46].copy_from_slice(&u32::MAX.to_le_bytes());
    tail[46..50].copy_from_slice(&8u32.to_le_bytes());
    tail[50..58].copy_from_slice(&0.25f64.to_le_bytes());
    tail[58..62].copy_from_slice(&8u32.to_le_bytes());
    let parse = |tail: &[u8]| {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"260");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        parse_parameter_scope(
            &bytes,
            &indexed_record_offsets_for_test(&bytes),
            12,
            &"300".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
    };
    let scope = parse(&tail).expect("Canvas scope with reciprocal self links");
    assert_eq!(scope.kind().as_str(), "Canvas");
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );
    assert!(scope.history_state_id().is_none());
    for offset in [0, 4, 12, 17, 21, 27, 35, 41, 42, 58, 67, 73, 80, 87, 91, 97] {
        let mut broken = tail;
        broken[offset] ^= 1;
        assert!(parse(&broken).is_none(), "tail offset {offset}");
    }
    for offset in [31, 63, 76] {
        let mut broken = tail;
        broken[offset..offset + 4].fill(0);
        assert!(parse(&broken).is_none());
    }
    for duration in [-0.25, f64::NAN, f64::INFINITY] {
        let mut broken = tail;
        broken[50..58].copy_from_slice(&duration.to_le_bytes());
        assert!(parse(&broken).is_none());
    }
}

#[test]
fn typed_canvas_scope_validates_reference_types_and_fixed_tail() {
    use crate::design::test_support::indexed_record_offsets_for_test;
    fn reference(bytes: &mut [u8], at: usize, target: u32, guid: &str) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
        bytes[at + 9..at + 13].copy_from_slice(&36u32.to_le_bytes());
        bytes[at + 13..at + 49].copy_from_slice(guid.as_bytes());
    }
    let mut bytes = vec![0; 350];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"300");
    bytes[7..11].copy_from_slice(&12u32.to_le_bytes());
    bytes[11..15].copy_from_slice(&2u32.to_le_bytes());
    reference(&mut bytes, 15, 55, "5A1BF548-241F-46FD-9FB5-E4B05126EB9D");
    reference(&mut bytes, 66, 56, "7B6C3C2D-4096-4E6D-B225-699E8C2D9355");
    bytes[117..121].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut kind = Vec::new();
    lp_utf16(&mut kind, "Canvas");
    bytes[121..137].copy_from_slice(&kind);
    let tail = &mut bytes[137..333];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    reference(tail, 19, 3, "B7F34D7B-DAC4-41A5-B67D-D672537C3BFA");
    tail[70..74].copy_from_slice(&u32::MAX.to_le_bytes());
    tail[74..78].copy_from_slice(&8u32.to_le_bytes());
    tail[78..86].copy_from_slice(&0.25f64.to_le_bytes());
    tail[86..90].copy_from_slice(&8u32.to_le_bytes());
    reference(tail, 90, 4, "5FBE538E-139B-46D4-BCAF-CC796D1E1F78");
    reference(tail, 143, 5, "6CCF41D5-40BE-48ED-A834-18F3EAED6C57");
    bytes[333..337].copy_from_slice(&3u32.to_le_bytes());
    bytes[337..340].copy_from_slice(b"260");
    bytes[340..344].copy_from_slice(&12u32.to_le_bytes());
    let parse = |bytes: &[u8]| {
        super::parse_parameter_scope(
            bytes,
            &indexed_record_offsets_for_test(bytes),
            12,
            &"300".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
    };
    let scope = parse(&bytes).expect("typed Canvas scope");
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );
    let wire = serde_json::to_string(&scope).unwrap();
    let decoded: crate::records::feature::scope::DesignParameterScope =
        serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
    for offset in [
        11, 20, 28, 79, 137, 141, 161, 169, 207, 223, 240, 278, 285, 331,
    ] {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(parse(&invalid).is_none(), "offset {offset}");
    }
    for duration in [-0.25, f64::NAN, f64::INFINITY] {
        let mut invalid = bytes.clone();
        invalid[215..223].copy_from_slice(&duration.to_le_bytes());
        assert!(parse(&invalid).is_none());
    }
}

#[test]
fn thread_bound_hole_scope_requires_reciprocal_tail_and_history() {
    use crate::design::test_support::indexed_record_offsets_for_test;
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(b"300");
    prefix.extend_from_slice(&12u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 10]);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    for member in [55u32, 56] {
        prefix.push(1);
        prefix.extend_from_slice(&member.to_le_bytes());
        prefix.extend_from_slice(&[0; 6]);
    }
    prefix.extend_from_slice(&9u32.to_le_bytes());
    lp_utf16(&mut prefix, "Hole");
    let mut tail = [0; 109];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    tail[12..16].copy_from_slice(&2u32.to_le_bytes());
    for (at, index) in [
        (16, 12u32),
        (27, 100),
        (41, 99),
        (73, 98),
        (86, 97),
        (97, 12),
    ] {
        tail[at] = 1;
        tail[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }
    tail[53..57].copy_from_slice(&8u32.to_le_bytes());
    tail[57..61].copy_from_slice(&4u32.to_le_bytes());
    tail[61..69].copy_from_slice(&0.25f64.to_le_bytes());
    tail[69..73].copy_from_slice(&4u32.to_le_bytes());
    let parse = |tail: &[u8]| {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"260");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        super::parse_parameter_scope(
            &bytes,
            &indexed_record_offsets_for_test(&bytes),
            12,
            &"300".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
    };
    let scope = parse(&tail).expect("Thread-bound Hole scope");
    assert_eq!(scope.kind_name(), "Hole");
    assert_eq!(scope.history_state_id(), Some(9));
    assert_eq!(scope.previous_history_state_id(), Some(8));
    let wire = serde_json::to_string(&scope).unwrap();
    let decoded: crate::records::feature::scope::DesignParameterScope =
        serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
    for offset in [
        0, 4, 12, 17, 21, 28, 32, 38, 42, 46, 52, 69, 74, 78, 84, 87, 91, 98, 102, 108,
    ] {
        let mut invalid = tail;
        invalid[offset] ^= 1;
        assert!(parse(&invalid).is_none(), "tail offset {offset}");
    }
    for duration in [-0.25, f64::NAN, f64::INFINITY] {
        let mut invalid = tail;
        invalid[61..69].copy_from_slice(&duration.to_le_bytes());
        assert!(parse(&invalid).is_none());
    }
}

#[test]
fn scope_class_type_selects_hole_and_preserves_the_display_name() {
    use std::io::{Cursor, Write};

    use crate::records::entity_header::SegmentType;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::zip_test::with_scan;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"256");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut bytes, "Perçage");
    let mut tail = [0; 78];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&1u32.to_le_bytes());

    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let row = crate::design::test_support::design_type(
        "1C037A07-4A15-43F6-ABFC-BBF61B9038D4",
        None,
        1,
        "Fusion",
        vec![1],
    );
    let types = [SegmentType::try_new(
        crate::ids::native_design_type_id("FusionAssetName[Active]/Design1/MetaStream.dat", 0),
        row,
    )
    .unwrap()];
    with_scan(&archive, |scan| {
        for type_rows in [&types[..], &[][..]] {
            let scopes = super::decode_parameter_scopes(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                &super::ParameterScopeSources {
                    types: type_rows,
                    ..Default::default()
                },
            )
            .unwrap()
            .scopes;
            assert_eq!(scopes.len(), 1);
            let scope = &scopes[0];
            if type_rows.is_empty() {
                assert!(matches!(scope.kind(), DesignFeatureKind::Native(_)));
                assert_eq!(scope.kind_name(), "Perçage");
                assert_eq!(scope.source_kind, None);
            } else {
                assert_eq!(scope.kind(), DesignFeatureKind::Hole);
                assert_eq!(scope.source_kind.as_deref(), Some("Perçage"));
                let mut english = scope.clone().into_draft();
                english.frame_length -= 6;
                english.feature_ordinal_offset -= 6;
                english.paired_byte_offset -= 6;
                english.previous_history_state_id_offset = english
                    .previous_history_state_id_offset
                    .map(|offset| offset - 6);
                let english = DesignParameterScope::try_new(english).unwrap();
                assert!(super::equivalent_scope_variant_payload(
                    &cadmpeg_test_support::service_decode_context(),
                    scope,
                    &english,
                )
                .unwrap());
            }
            assert_eq!(scope.source_kind_name(), "Perçage");
            assert_eq!(
                super::parameter_scope_payload_length(scope),
                Some(scope.frame_length() - 14)
            );
            let wire = serde_json::to_string(scope).unwrap();
            let restored: DesignParameterScope = serde_json::from_str(&wire).unwrap();
            assert_eq!(&restored, scope);
            let mut wrong_width: serde_json::Value = serde_json::from_str(&wire).unwrap();
            wrong_width["source_kind"] = "Perçages".into();
            assert!(serde_json::from_value::<DesignParameterScope>(wrong_width).is_err());
        }
    });
}

#[test]
fn stored_family_name_takes_precedence_over_the_class_type_family() {
    use crate::records::feature::scope::DesignFeatureKind;

    let hole = Some(&DesignFeatureKind::Hole);
    assert_eq!(
        super::scope_feature_kind("Perçage", hole),
        Some(DesignFeatureKind::Hole)
    );
    assert_eq!(
        super::scope_feature_kind("Congé", Some(&DesignFeatureKind::Fillet)),
        Some(DesignFeatureKind::Conge)
    );
    assert_eq!(
        super::scope_feature_kind("Thread", hole),
        Some(DesignFeatureKind::Thread)
    );
    assert!(matches!(
        super::scope_feature_kind("Perçage", None),
        Some(DesignFeatureKind::Native(_))
    ));
    for shared in [
        "D57B1A22-9D5A-4D7D-A909-E08C579612CA",
        "7AFA0A65-B53C-472C-ACA4-5C7C0A65001C",
    ] {
        assert_eq!(super::family::from_type_guid(shared), None);
        assert!(matches!(
            super::scope_feature_kind("CutPaste", super::family::from_type_guid(shared).as_ref()),
            Some(DesignFeatureKind::Native(_))
        ));
    }
    assert_eq!(
        super::family::from_type_guid("1c037a07-4a15-43f6-abfc-bbf61b9038d4"),
        Some(DesignFeatureKind::Hole)
    );
}

#[test]
fn unassigned_extrude_extent_retains_scope_with_a_loss() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#60",
        DesignFeatureKind::Extrude,
        60,
    );
    let loss = super::extrude_extent_native_loss(&scope).unwrap();
    assert_eq!(loss.code.local_code(), "feature.extrude-extent-native");
    let sketch = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#70",
        DesignFeatureKind::Sketch,
        70,
    );
    assert!(super::extrude_extent_native_loss(&sketch).is_none());
}

#[test]
fn a_scanned_count_whose_table_overflows_is_rejected_not_fatal() {
    // An exact table: four count bytes, then two 11-byte entries.
    assert!(reference_table_ends_at(100, 2, 11, 126));
    assert!(!reference_table_ends_at(100, 3, 11, 126));
    assert!(!reference_table_ends_at(100, 0, 11, 104));
    // Arbitrary bytes read as a count overflow `count * stride` (on a 32-bit
    // target, any u32 above `usize::MAX / stride`): that offset is not the
    // table, so the scan goes on instead of abandoning the scope.
    assert!(!reference_table_ends_at(100, usize::MAX, 11, 126));
    assert!(!reference_table_ends_at(usize::MAX - 2, 1, 11, 126));
    let wide = usize::MAX / 11 + 1;
    assert!(!reference_table_ends_at(100, wide, 11, 126));
}
