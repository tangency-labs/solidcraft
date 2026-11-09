// SPDX-License-Identifier: Apache-2.0

use super::*;

fn frame() -> (Vec<u8>, Vec<u32>) {
    let references: Vec<u32> = (101..=111).collect();
    let mut bytes = vec![0; layout::LEN];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"349");
    bytes[7..11].copy_from_slice(&100u32.to_le_bytes());
    for (at, value) in [
        (layout::PREFIX_CONSTANT, 1u32),
        (layout::OPERATION, 2),
        (layout::DIRECTION, 2),
        (layout::FIRST_SIDE_EXTENT, 4),
        (layout::SECOND_SIDE_EXTENT, 4),
        (layout::PROFILE_GROUP_COUNT, 1),
        (layout::BODY_GROUP_COUNT, 1),
        (layout::REFERENCE_COUNT, 11),
        (layout::HISTORY_STATE, 1),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[layout::GEOMETRY_KIND] = 1;
    bytes[layout::PROFILE_NORMAL..layout::PROFILE_NORMAL + 8].copy_from_slice(&1f64.to_le_bytes());
    for (at, ordinal) in [
        (layout::TRAILING_REFERENCE, 10),
        (layout::SECOND_BODY_GROUP_REFERENCE, 7),
        (layout::FIRST_TAPER_REFERENCE, 6),
        (layout::FIRST_BODY_GROUP_REFERENCE, 4),
        (layout::SECOND_TAPER_REFERENCE, 9),
        (layout::PROFILE_GROUP_REFERENCE, 2),
        (layout::BODY_GROUP_REFERENCE, 0),
    ] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&references[ordinal].to_le_bytes());
    }
    let mut guid = Vec::new();
    crate::test_support::lp_utf16(&mut guid, "00000000-0000-0000-0000-000000000000");
    bytes[layout::NULL_GUID..layout::REFERENCE_COUNT_PADDING].copy_from_slice(&guid);
    for (ordinal, value) in references.iter().enumerate() {
        let at = layout::REFERENCE_TABLE + ordinal * 11;
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    }
    let mut kind = Vec::new();
    crate::test_support::lp_utf16(&mut kind, "Extrude");
    bytes[layout::FEATURE_KIND..layout::FEATURE_ORDINAL].copy_from_slice(&kind);
    bytes[layout::FEATURE_ORDINAL..layout::FEATURE_ORDINAL + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    bytes[layout::PREVIOUS_HISTORY_STATE..layout::PREVIOUS_HISTORY_STATE + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    crate::test_support::indexed_header(&mut bytes, *b"262", 100);
    (bytes, references)
}

fn parse(bytes: &[u8], references: &[u32]) -> Option<DesignExtrudePrologue> {
    super::super::exact_extrude_prologue(
        bytes,
        0,
        layout::LEN,
        "349",
        "262",
        layout::REFERENCE_COUNT,
        references,
    )
}

#[test]
fn class_349_through_all_reads_extents_before_the_group_counts() {
    let (bytes, references) = frame();
    let prologue = parse(&bytes, &references).unwrap();
    assert_eq!(
        prologue.extent(),
        Some(DesignExtrudeExtent::TwoSidedThroughAll)
    );
    assert_eq!(prologue.side_extent_discriminators(), Some([4, 4]));
    assert_eq!(prologue.operation(), DesignExtrudeOperation::Cut);
    assert!(!prologue.direction_reversed());
    assert!(prologue.solid_operation());
    assert_eq!(
        View::u32_le_at(&bytes, layout::PROFILE_GROUP_COUNT),
        Some(1)
    );
    assert_eq!(View::u32_le_at(&bytes, layout::BODY_GROUP_COUNT), Some(1));
}

#[test]
fn class_349_through_all_refuses_changed_fields_and_foreign_references() {
    let (bytes, references) = frame();
    for at in [
        layout::PREFIX_CONSTANT,
        layout::OPERATION,
        layout::DIRECTION,
        layout::FACE_EXTEND,
        layout::GEOMETRY_KIND,
        layout::START_SUPPORT,
        layout::ZERO_RUN_9,
        layout::ZERO_RUN_3,
        layout::NORMAL_PADDING,
        layout::REFERENCE_COUNT,
        layout::FIRST_SIDE_EXTENT,
        layout::SECOND_SIDE_EXTENT,
        layout::PROFILE_GROUP_COUNT,
        layout::BODY_GROUP_COUNT,
        layout::DIRECTION_REVERSED,
        layout::ABSENT_SLOTS,
        layout::EXTENT_SEPARATOR,
        layout::SECOND_TAPER_PADDING,
        layout::PROFILE_GROUP_PADDING,
        layout::BODY_GROUP_PADDING,
        layout::REFERENCE_COUNT_PADDING,
        layout::TRAILING_REFERENCE + 1,
        layout::SECOND_BODY_GROUP_REFERENCE + 1,
        layout::FIRST_TAPER_REFERENCE + 1,
        layout::FIRST_BODY_GROUP_REFERENCE + 1,
        layout::SECOND_TAPER_REFERENCE + 1,
        layout::PROFILE_GROUP_REFERENCE + 1,
        layout::BODY_GROUP_REFERENCE + 1,
        layout::NULL_GUID + 4,
    ] {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert!(
            parse(&changed, &references).is_none(),
            "changed field at {at}"
        );
    }
    for value in [0.0, f64::NAN] {
        let mut changed = bytes.clone();
        changed[layout::PROFILE_NORMAL..layout::PROFILE_NORMAL + 8]
            .copy_from_slice(&value.to_le_bytes());
        assert!(parse(&changed, &references).is_none());
    }
    assert!(!matches_frame("348", "262", 496, 272, 11));
    assert!(!matches_frame("349", "263", 496, 272, 11));
    assert!(!matches_frame("349", "262", 495, 272, 11));
    assert!(!matches_frame("349", "262", 496, 271, 11));
    assert!(!matches_frame("349", "262", 496, 272, 10));
}

#[test]
fn class_349_scope_validator_uses_the_decoder_extent_key() {
    let (bytes, _) = frame();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let class = "349".to_owned().try_into().unwrap();
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes, &records, 100, &class, 0,
    )
    .unwrap()
    .unwrap();
    let prologue = scope.extrude_prologue().unwrap();
    assert!(valid_prologue(&scope, prologue));
    let mut wrong_extent = prologue;
    if let DesignExtrudePrologue::LegacyShifted { extent, .. } = &mut wrong_extent {
        *extent = Some(DesignExtrudeExtent::TwoSidedDistance);
    }
    assert!(!valid_prologue(&scope, wrong_extent));
    let mut other = scope;
    other.class_tag = "348".to_owned().try_into().unwrap();
    assert!(!valid_prologue(&other, prologue));
}

#[test]
fn class_349_reference_aware_prefix_keeps_its_one_sided_distance() {
    let (mut bytes, references) = frame();
    let mut prefix = bytes[..11].to_vec();
    prefix.resize(25, 0);
    prefix[layout::PREFIX_CONSTANT..layout::PREFIX_CONSTANT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    prefix.push(1);
    prefix.extend_from_slice(&references[9].to_le_bytes());
    prefix.extend_from_slice(&[0; 7]);
    for value in [1u32, 1, 2] {
        prefix.extend_from_slice(&value.to_le_bytes());
    }
    prefix.extend_from_slice(&[0, 1, 2, 0, 0, 0]);
    for value in [0f64, 0.0, 1.0] {
        prefix.extend_from_slice(&value.to_le_bytes());
    }
    for ordinal in [Some(0), None, Some(1), Some(3), None, Some(2), None] {
        if let Some(ordinal) = ordinal {
            prefix.push(1);
            prefix.extend_from_slice(&references[ordinal].to_le_bytes());
            prefix.extend_from_slice(&[0; 6]);
        } else {
            prefix.push(0);
        }
    }
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 9]);
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.resize(layout::REFERENCE_COUNT, 0);
    bytes[..prefix.len()].copy_from_slice(&prefix);
    let prologue = parse(&bytes, &references).unwrap();
    assert!(matches!(
        prologue,
        DesignExtrudePrologue::ReferenceAware {
            operation_offset: 37,
            side_extent_discriminator_offsets: [126, 139],
            extent: DesignExtrudeExtent::OneSidedDistance,
            start: DesignExtrudeStart::FromFace,
            ..
        }
    ));
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
        &bytes,
        &records,
        100,
        &"349".to_owned().try_into().unwrap(),
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(scope.extrude_prologue(), Some(prologue));
    assert!(!matches_scope(&scope));
}
