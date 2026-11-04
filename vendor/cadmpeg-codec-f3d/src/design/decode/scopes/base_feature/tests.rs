// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::{
    exact_base_feature_construction, exact_timeline_owned_base_feature, timeline_body_owners,
};
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::references::DesignClassTag;

fn result_body_frame() -> (Vec<u8>, DesignParameterScope) {
    let body_count = 3usize;
    let frame_length = 262 + 52 * body_count;
    let mut bytes = vec![0u8; frame_length];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(
        &(2 * u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    let mut cursor = 24;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9]
            .copy_from_slice(&(101 + u64_from_index(ordinal)).to_le_bytes());
        cursor += 15;
    }
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9]
            .copy_from_slice(&(201 + u64_from_index(ordinal)).to_le_bytes());
        cursor += 15;
    }
    bytes[cursor] = 1;
    bytes[cursor + 7..cursor + 11].copy_from_slice(
        &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    cursor += 11;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(
            &(201 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 11;
    }
    bytes[cursor] = 0;
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(
        &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    cursor += 4;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(
            &(401 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 11;
    }
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-result",
        DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag = DesignClassTag::try_from("409".to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.kind_offset = u64_from_index(frame_length - 102);
            draft.paired_byte_offset = u64_from_index(frame_length);
            draft.reference_members = ReferenceRun::unlocated(vec![301]);
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn base_feature_result_runs_admit_three_bodies() {
    let (bytes, scope) = result_body_frame();
    let result = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("admitted BaseFeature result bodies");
    let DesignBaseFeatureConstruction::ResultBodies { bodies, .. } = result else {
        panic!("unexpected BaseFeature construction");
    };
    assert_eq!(bodies.iter().count(), 3);
}

fn snapshot_frame() -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0u8; 485];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [(101u64, [0u8, 0, 3, 4, 5, 6]), (202, [0u8, 0, 4, 3, 2, 1])] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[cursor + 4..cursor + 8].copy_from_slice(&1u32.to_le_bytes());
    cursor += 8;
    for guid in [
        "11111111-2222-3333-4444-555555555555",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    ] {
        let encoded =
            crate::bytes::lp_utf16_bytes(guid).expect("fixture UTF-16 code-unit count fits u32");
        bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
        cursor += encoded.len();
    }
    bytes[cursor..cursor + 7].copy_from_slice(&[0, 0, 1, 1, 0, 0, 0]);
    cursor += 7;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 8].copy_from_slice(&101u64.to_le_bytes());
    cursor += 11;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 8].copy_from_slice(&301u64.to_le_bytes());
    cursor += 14;
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 8].copy_from_slice(&401u64.to_le_bytes());
    cursor += 18;
    let encoded = crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000")
        .expect("fixture UTF-16 code-unit count fits u32");
    bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    cursor += 3;
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 4].copy_from_slice(&301u32.to_le_bytes());
    cursor += 10;
    bytes[cursor..cursor + 4].copy_from_slice(&7u32.to_le_bytes());
    cursor += 4;
    let encoded = crate::bytes::lp_utf16_bytes("Base Feature")
        .expect("fixture UTF-16 code-unit count fits u32");
    bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    assert_eq!(cursor, 401);

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-snapshot",
        DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag = DesignClassTag::try_from("314".to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 485;
            draft.paired_byte_offset = 485;
            draft.kind_offset = 373;
            draft.feature_ordinal_offset = 397;
            draft.history_state_id = Some(7);
            draft.previous_history_state_id = None;
            draft.previous_history_state_id_offset = None;
            draft.reference_count_offset = 350;
            draft.reference_members =
                ReferenceRun::from_columns(vec![301], vec![355], "reference_members").unwrap();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn base_feature_snapshot_admits_two_bodies() {
    let (bytes, scope) = snapshot_frame();
    let result = exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("admitted BaseFeature body snapshot");
    let DesignBaseFeatureConstruction::BodySnapshot { bodies, .. } = result else {
        panic!("unexpected BaseFeature construction");
    };
    assert_eq!(bodies.len(), 2);
}

#[test]
fn local_base_feature_entries_preserve_per_element_members() {
    let mut bytes = Vec::new();
    for (target, member) in [(2485u64, 0u32), (2493, 1), (2494, u32::MAX)] {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&member.to_le_bytes());
    }
    let mut cursor = 0;
    for (target, member) in [(2485u64, 0u32), (2493, 1), (2494, u32::MAX)] {
        let entry = super::read_base_feature_entry_at(&bytes, &mut cursor).unwrap();
        assert_eq!(entry.value, target);
        assert_eq!(&entry.field[2..], &member.to_le_bytes());
    }
    assert_eq!(cursor, bytes.len());
    for (slot, value) in [(9, 1), (10, 1), (9, 2), (10, 2), (9, 36)] {
        let mut widened = bytes.clone();
        widened[slot] = value;
        let mut cursor = 0;
        assert!(super::read_base_feature_entry_at(&widened, &mut cursor).is_none());
        assert_eq!(cursor, 0);
    }
}

#[test]
fn base_feature_fixed_body_runs_refuse_nonlocal_reference_flags() {
    let (snapshot, snapshot_scope) = snapshot_frame();
    let (result, result_scope) = result_body_frame();
    for (bytes, scope, marker_offsets) in [
        (snapshot, snapshot_scope, vec![24, 39]),
        (result, result_scope, vec![24, 39, 54, 69, 84, 99]),
    ] {
        for marker in marker_offsets {
            for (flag, value) in [(9, 1), (10, 1), (9, 36)] {
                let mut widened = bytes.clone();
                widened[marker + flag] = value;
                assert!(exact_base_feature_construction(&widened, &scope)
                    .unwrap()
                    .is_none());
            }
        }
    }
}

const PASTE_OBJECT: &str = "BA3059E4-4B23-4D02-9D18-E7BB95C991F3";
const BODY_TO_COMPONENT_OBJECT: &str = "56225BC7-69B7-446A-9AEA-6AA25BC7806E";

/// A timeline object for record 50 that places `bodies.len()` bodies, followed
/// by the owned Base Feature scope for record 51. The object's first counted
/// reference sits where a one-body object stores its scope reference.
fn owned_frames(bodies: &[(u32, u32)], scope_count: u32) -> (Vec<u8>, DesignParameterScope) {
    owned_frames_with_prefix(bodies, scope_count, &[0; 10])
}

fn owned_frames_with_prefix(
    bodies: &[(u32, u32)],
    scope_count: u32,
    prefix: &[u8],
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"300", 50);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
    for (parameter, _) in bodies {
        push_owner_reference(&mut bytes, u64::from(*parameter));
    }
    push_owner_reference(&mut bytes, 51);
    let start = bytes.len();
    crate::test_support::indexed_header(&mut bytes, *b"301", 51);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&scope_count.to_le_bytes());
    for value in bodies
        .iter()
        .map(|(parameter, _)| parameter)
        .chain(bodies.iter().map(|(_, body)| body))
    {
        crate::test_support::push_marked_reference(&mut bytes, *value);
        bytes.extend_from_slice(&[0; 4]);
    }
    bytes.resize(bytes.len() + 64, 0);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:scope#owned",
        DesignFeatureKind::BaseFeature,
        51,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(start);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

fn push_owner_reference(bytes: &mut Vec<u8>, target: u64) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
}

fn owner_types(guid: &str) -> Vec<crate::records::entity_header::SegmentType> {
    vec![crate::records::entity_header::SegmentType::try_new(
        "f3d:Design1/MetaStream.dat:design-type#0".to_owned(),
        crate::design::test_support::design_type(guid, None, 1, "Fusion", vec![50]),
    )
    .unwrap()]
}

#[test]
fn timeline_object_names_its_scope_after_the_counted_references() {
    let (bytes, scope) = owned_frames(&[(71, 61), (72, 62)], 4);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let types = owner_types(PASTE_OBJECT);
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    assert_eq!(owners.keys().copied().collect::<Vec<_>>(), [51]);
    let construction = exact_timeline_owned_base_feature(&bytes, &scope, owners[&51])
        .expect("owned two-body scope");
    assert_eq!(construction.timeline_owner(), Some(50));
    assert_eq!(
        construction.body_entity_suffixes().collect::<Vec<_>>(),
        [61, 62]
    );
    let DesignBaseFeatureConstruction::TimelineOwnedBodies {
        parameter_bodies, ..
    } = &construction
    else {
        panic!("owned Base Feature form");
    };
    assert_eq!(
        parameter_bodies
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [71, 72]
    );
}

#[test]
fn timeline_body_owners_admit_each_owner_type() {
    let (bytes, _) = owned_frames(&[(55, 65)], 2);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    for (guid, owned) in [
        (PASTE_OBJECT, true),
        (BODY_TO_COMPONENT_OBJECT, true),
        ("00000000-0000-0000-0000-00000000ABCD", false),
    ] {
        let types = owner_types(guid);
        let owners = timeline_body_owners(&bytes, &records, types.iter());
        assert_eq!(owners.contains_key(&51), owned, "{guid}");
    }
}

#[test]
fn owned_scope_requires_two_references_per_placed_body() {
    let (bytes, scope) = owned_frames(&[(71, 61), (72, 62)], 2);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let types = owner_types(PASTE_OBJECT);
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    assert!(exact_timeline_owned_base_feature(&bytes, &scope, owners[&51]).is_none());
}

#[test]
fn timeline_object_admits_the_marked_count_prefix() {
    for (prefix, owned) in [
        (&[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0][..], true),
        (&[0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0][..], false),
        (&[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 9][..], false),
    ] {
        let (bytes, _) = owned_frames_with_prefix(&[(71, 61)], 2, prefix);
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
        let types = owner_types(BODY_TO_COMPONENT_OBJECT);
        let owners = timeline_body_owners(&bytes, &records, types.iter());
        assert_eq!(owners.contains_key(&51), owned, "{prefix:?}");
    }
}

/// A zero-body Base Feature frame whose shared-metadata marker is at
/// `metadata_marker`, under the stream-local class pair `class_tags`.
fn zero_body_frame(
    frame_length: usize,
    metadata_marker: usize,
    class_tags: (&str, &str),
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0u8; frame_length];
    bytes[20] = 1;
    bytes[metadata_marker] = 1;
    bytes[metadata_marker + 1..metadata_marker + 9].copy_from_slice(&77u64.to_le_bytes());
    bytes[metadata_marker + 23..metadata_marker + 27].copy_from_slice(&36u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-zero-body",
        DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag = DesignClassTag::try_from(class_tags.0.to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from(class_tags.1.to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.kind_offset = u64_from_index(frame_length - 101);
            draft.paired_byte_offset = u64_from_index(frame_length);
            draft.reference_members = ReferenceRun::unlocated(vec![77]);
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

fn zero_body_metadata(bytes: &[u8], scope: &DesignParameterScope) -> Option<(usize, u32)> {
    match exact_base_feature_construction(bytes, scope).unwrap()? {
        DesignBaseFeatureConstruction::ResultBodies {
            bodies,
            metadata_record,
            ..
        } => Some((bodies.iter().count(), metadata_record)),
        _ => None,
    }
}

#[test]
fn zero_body_frame_is_read_under_any_class_pair() {
    for class_tags in [("409", "262"), ("339", "264")] {
        let (bytes, scope) = zero_body_frame(258, 32, class_tags);
        assert_eq!(
            zero_body_metadata(&bytes, &scope),
            Some((0, 77)),
            "{class_tags:?}"
        );
    }
}

#[test]
fn wide_zero_body_frame_reads_its_metadata_at_offset_37() {
    let (bytes, scope) = zero_body_frame(262, 36, ("284", "259"));
    assert_eq!(zero_body_metadata(&bytes, &scope), Some((0, 77)));

    let mut flagged = bytes.clone();
    flagged[29] = 1;
    assert_eq!(zero_body_metadata(&flagged, &scope), None);

    let mut unmarked = bytes;
    unmarked[36] = 0;
    assert_eq!(zero_body_metadata(&unmarked, &scope), None);
}
