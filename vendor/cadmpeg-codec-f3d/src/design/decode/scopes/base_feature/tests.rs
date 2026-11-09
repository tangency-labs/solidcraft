// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use std::collections::HashMap;

use super::{
    exact_base_feature_construction, exact_timeline_owned_base_feature, timeline_body_owners,
    timeline_owned_base_feature, TimelineBodyOwner,
};
use crate::loss::F3dLossCode;
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::references::DesignClassTag;
use cadmpeg_ir::report::loss::LossKind;

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
    let pairs = vec![(SOURCE_A, PARAMETER_A); bodies.len()];
    let trailer = source_pairs(u32::try_from(bodies.len()).unwrap(), &pairs);
    owned_frames_with(
        bodies,
        scope_count,
        prefix,
        &owner_paired_header(),
        &trailer,
    )
}

/// The paired header that closes the envelope of timeline object 50.
fn owner_paired_header() -> Vec<u8> {
    let mut header = Vec::new();
    crate::test_support::indexed_header(&mut header, *b"259", 50);
    header
}

/// `owned_frames_with_prefix`, with `after_reference` after the object's
/// reference to the scope and `trailer` after the scope's body runs; the
/// scope's reference table follows the trailer.
fn owned_frames_with(
    bodies: &[(u32, u32)],
    scope_count: u32,
    prefix: &[u8],
    after_reference: &[u8],
    trailer: &[u8],
) -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"300", 50);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&u32::try_from(bodies.len()).unwrap().to_le_bytes());
    for (parameter, _) in bodies {
        push_owner_reference(&mut bytes, u64::from(*parameter));
    }
    push_owner_reference(&mut bytes, 51);
    bytes.extend_from_slice(after_reference);
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
    bytes.extend_from_slice(trailer);
    let reference_count_offset = u64_from_index(bytes.len());
    bytes.resize(bytes.len() + 128, 0);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:scope#owned",
        DesignFeatureKind::BaseFeature,
        51,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(start);
            draft.reference_count_offset = reference_count_offset;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.kind_offset + 96;
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
    let (construction, loss) =
        exact_timeline_owned_base_feature(&bytes, &scope, owners[&51][0], &HashMap::new())
            .expect("owned two-body scope");
    assert!(loss.is_none());
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
    assert!(
        exact_timeline_owned_base_feature(&bytes, &scope, owners[&51][0], &HashMap::new())
            .is_none()
    );
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

const NULL_KEY: &str = "00000000-0000-0000-0000-000000000000";
const SOURCE_A: &str = "aaaaaaaa-0000-4000-8000-00000000000a";
const SOURCE_B: &str = "bbbbbbbb-0000-4000-8000-00000000000b";
const PARAMETER_A: &str = "cccccccc-0000-4000-8000-00000000000c";
const PARAMETER_B: &str = "dddddddd-0000-4000-8000-00000000000d";

/// `zeros` zero bytes, then the LP-UTF16 removal key.
fn removal_key(zeros: usize, key: &str) -> Vec<u8> {
    let mut bytes = vec![0; zeros];
    crate::test_support::lp_utf16(&mut bytes, key);
    bytes
}

/// Byte `1`, four zero bytes, u32 `count`, then each GUID pair, source
/// first.
fn source_pairs(count: u32, pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = vec![1, 0, 0, 0, 0];
    bytes.extend_from_slice(&count.to_le_bytes());
    for (source, parameter) in pairs {
        crate::test_support::lp_utf16(&mut bytes, source);
        crate::test_support::lp_utf16(&mut bytes, parameter);
    }
    bytes
}

fn two_source_pairs() -> Vec<u8> {
    source_pairs(2, &[(SOURCE_A, PARAMETER_A), (SOURCE_B, PARAMETER_B)])
}

fn source_nodes() -> HashMap<String, Option<u64>> {
    HashMap::from([
        (SOURCE_A.to_owned(), Some(41)),
        (SOURCE_B.to_owned(), Some(42)),
    ])
}

/// The owned construction of `scope` among `owners`, and the loss codes its
/// reading adds.
fn owned_with_losses(
    bytes: &[u8],
    scope: &DesignParameterScope,
    owners: &[TimelineBodyOwner],
    nodes: &HashMap<String, Option<u64>>,
) -> (Option<DesignBaseFeatureConstruction>, Vec<LossKind>) {
    let mut losses = Vec::new();
    let construction = timeline_owned_base_feature(bytes, scope, owners, nodes, &mut losses);
    (
        construction,
        losses.into_iter().map(|loss| loss.code).collect(),
    )
}

/// The owned construction of a two-body timeline object of type `owner_type`
/// when `owner_tail` follows its reference to the owned scope, and the loss
/// codes its reading adds.
fn owned_construction(
    owner_type: &str,
    owner_tail: &[u8],
    trailer: &[u8],
    nodes: &HashMap<String, Option<u64>>,
) -> (Option<DesignBaseFeatureConstruction>, Vec<LossKind>) {
    let (bytes, scope) = owned_frames_with(&[(71, 61), (72, 62)], 4, &[0; 10], owner_tail, trailer);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let types = owner_types(owner_type);
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    owned_with_losses(&bytes, &scope, &owners[&51], nodes)
}

/// The copy sources a two-body paste object names when `after_reference` and
/// then the object's paired header close its envelope.
fn copy_sources(
    after_reference: &[u8],
    trailer: &[u8],
    nodes: &HashMap<String, Option<u64>>,
) -> Vec<u64> {
    let mut owner_tail = after_reference.to_vec();
    owner_tail.extend(owner_paired_header());
    owner_tail_copy_sources(PASTE_OBJECT, &owner_tail, trailer, nodes)
}

/// The copy sources a two-body timeline object of type `owner_type` names
/// when `owner_tail` follows its reference to the owned scope.
fn owner_tail_copy_sources(
    owner_type: &str,
    owner_tail: &[u8],
    trailer: &[u8],
    nodes: &HashMap<String, Option<u64>>,
) -> Vec<u64> {
    let construction = owned_construction(owner_type, owner_tail, trailer, nodes)
        .0
        .expect("owned two-body scope");
    assert_eq!(
        construction.body_entity_suffixes().collect::<Vec<_>>(),
        [61, 62]
    );
    construction.copy_source_suffixes().collect()
}

#[test]
fn a_paste_with_a_null_removal_key_names_the_source_of_each_copy() {
    for zeros in [29, 48] {
        assert_eq!(
            copy_sources(
                &removal_key(zeros, NULL_KEY),
                &two_source_pairs(),
                &source_nodes()
            ),
            [41, 42],
            "{zeros}"
        );
    }
    let upper = source_pairs(
        2,
        &[
            (&SOURCE_A.to_ascii_uppercase(), PARAMETER_A),
            (SOURCE_B, PARAMETER_B),
        ],
    );
    assert_eq!(
        copy_sources(&removal_key(29, NULL_KEY), &upper, &source_nodes()),
        [41, 42]
    );
}

#[test]
fn a_paste_that_stores_a_removal_key_names_no_copy_source() {
    let mut path = removal_key(29, NULL_KEY);
    path[12] = 1;
    for after_reference in [
        removal_key(29, "eeeeeeee-0000-4000-8000-00000000000e"),
        path,
        Vec::new(),
    ] {
        assert!(
            copy_sources(&after_reference, &two_source_pairs(), &source_nodes()).is_empty(),
            "{after_reference:?}"
        );
    }
}

#[test]
fn a_removal_key_outside_the_owner_envelope_is_refused() {
    let mut beyond_paired_header = vec![0; 29];
    beyond_paired_header.extend(owner_paired_header());
    beyond_paired_header.extend(removal_key(0, NULL_KEY));
    beyond_paired_header.extend(owner_paired_header());
    assert!(owner_tail_copy_sources(
        PASTE_OBJECT,
        &beyond_paired_header,
        &two_source_pairs(),
        &source_nodes()
    )
    .is_empty());
}

#[test]
fn a_timeline_object_without_its_paired_header_owns_its_scope_without_a_copy_form() {
    let (bytes, scope) = owned_frames_with(
        &[(71, 61), (72, 62)],
        4,
        &[0; 10],
        &removal_key(29, NULL_KEY),
        &two_source_pairs(),
    );
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let types = owner_types(PASTE_OBJECT);
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    let (construction, losses) = owned_with_losses(&bytes, &scope, &owners[&51], &source_nodes());
    let construction = construction.expect("owned two-body scope");
    assert_eq!(
        construction.body_entity_suffixes().collect::<Vec<_>>(),
        [61, 62]
    );
    assert_eq!(construction.copy_source_suffixes().len(), 0);
    assert_eq!(losses, [F3dLossCode::FeatureTimelineOwnerUnpaired.kind()]);
    let types = owner_types(BODY_TO_COMPONENT_OBJECT);
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    assert_eq!(
        owned_with_losses(&bytes, &scope, &owners[&51], &source_nodes()).1,
        []
    );
}

#[test]
fn a_timeline_object_registered_twice_is_one_owner() {
    let (bytes, scope) = owned_frames(&[(71, 61), (72, 62)], 4);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let types = [crate::records::entity_header::SegmentType::try_new(
        "f3d:Design1/MetaStream.dat:design-type#0".to_owned(),
        crate::design::test_support::design_type(PASTE_OBJECT, None, 1, "Fusion", vec![50, 50]),
    )
    .unwrap()];
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    assert_eq!(owners[&51].len(), 1);
    assert_eq!(
        owned_with_losses(&bytes, &scope, &owners[&51], &HashMap::new()).1,
        []
    );
}

#[test]
fn a_body_to_component_object_with_a_null_key_names_no_copy_source() {
    let mut owner_tail = removal_key(29, NULL_KEY);
    owner_tail.extend(owner_paired_header());
    assert!(owner_tail_copy_sources(
        BODY_TO_COMPONENT_OBJECT,
        &owner_tail,
        &two_source_pairs(),
        &source_nodes()
    )
    .is_empty());
}

#[test]
fn source_pairs_end_before_the_scope_reference_table() {
    let mut owner_tail = removal_key(29, NULL_KEY);
    owner_tail.extend(owner_paired_header());
    let (bytes, mut scope) = owned_frames_with(
        &[(71, 61), (72, 62)],
        4,
        &[0; 10],
        &owner_tail,
        &two_source_pairs(),
    );
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let types = owner_types(PASTE_OBJECT);
    let owners = timeline_body_owners(&bytes, &records, types.iter());
    let pairs_end = scope.reference_count_offset();
    // The last pair's second GUID ends at `pairs_end`.
    for (reference_table, read) in [(pairs_end, true), (pairs_end - 1, false)] {
        scope
            .try_edit(|draft| {
                draft.reference_count_offset = reference_table;
                draft.layout_fixture_references();
            })
            .unwrap();
        let (construction, losses) =
            owned_with_losses(&bytes, &scope, &owners[&51], &source_nodes());
        assert_eq!(
            construction
                .expect("owned two-body scope")
                .copy_source_suffixes()
                .collect::<Vec<_>>(),
            if read { vec![41, 42] } else { Vec::new() },
            "{reference_table}"
        );
        assert_eq!(
            losses,
            if read {
                Vec::new()
            } else {
                vec![F3dLossCode::FeaturePasteSourcesUnresolved.kind()]
            },
            "{reference_table}"
        );
    }
}

#[test]
fn every_copy_source_needs_one_browser_node() {
    let key = removal_key(29, NULL_KEY);
    let mut missing = source_nodes();
    missing.remove(SOURCE_B);
    let mut ambiguous = source_nodes();
    ambiguous.insert(SOURCE_B.to_owned(), None);
    let mut owner_tail = key.clone();
    owner_tail.extend(owner_paired_header());
    for nodes in [missing, ambiguous] {
        assert!(copy_sources(&key, &two_source_pairs(), &nodes).is_empty());
        assert_eq!(
            owned_construction(PASTE_OBJECT, &owner_tail, &two_source_pairs(), &nodes).1,
            [F3dLossCode::FeaturePasteSourcesUnresolved.kind()]
        );
    }
}

#[test]
fn a_paste_owned_scope_keeps_its_bodies_when_its_guid_pairs_do_not_read() {
    let mut owner_tail = removal_key(29, NULL_KEY);
    owner_tail.extend(owner_paired_header());
    let mut marker = two_source_pairs();
    marker[0] = 0;
    let mut padding = two_source_pairs();
    padding[3] = 1;
    let not_a_guid = "aaaaaaaa-0000-4000-8000-00000000000g";
    for trailer in [
        marker,
        padding,
        Vec::new(),
        source_pairs(1, &[(SOURCE_A, PARAMETER_A), (SOURCE_B, PARAMETER_B)]),
        source_pairs(3, &[(SOURCE_A, PARAMETER_A), (SOURCE_B, PARAMETER_B)]),
        source_pairs(2, &[(SOURCE_A, PARAMETER_A)]),
        source_pairs(2, &[(not_a_guid, PARAMETER_A), (SOURCE_B, PARAMETER_B)]),
        source_pairs(2, &[(SOURCE_A, PARAMETER_A), (SOURCE_B, not_a_guid)]),
    ] {
        for (owner_type, expected_losses) in [
            (
                PASTE_OBJECT,
                vec![F3dLossCode::FeaturePasteSourcesUnresolved.kind()],
            ),
            (BODY_TO_COMPONENT_OBJECT, Vec::new()),
        ] {
            let (construction, losses) =
                owned_construction(owner_type, &owner_tail, &trailer, &source_nodes());
            let construction = construction.expect("owned two-body scope");
            assert_eq!(
                construction.body_entity_suffixes().collect::<Vec<_>>(),
                [61, 62],
                "{trailer:?}"
            );
            assert_eq!(construction.copy_source_suffixes().len(), 0, "{trailer:?}");
            assert_eq!(losses, expected_losses, "{owner_type} {trailer:?}");
        }
        let mut cut_tail = removal_key(29, "eeeeeeee-0000-4000-8000-00000000000e");
        cut_tail.extend(owner_paired_header());
        assert_eq!(
            owned_construction(PASTE_OBJECT, &cut_tail, &trailer, &source_nodes()).1,
            [],
            "{trailer:?}"
        );
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

#[test]
fn a_scope_that_two_timeline_objects_name_belongs_to_the_lowest_readable_record() {
    let mut owner_tail = removal_key(29, NULL_KEY);
    owner_tail.extend(owner_paired_header());
    let (mut bytes, scope) = owned_frames_with(
        &[(71, 61), (72, 62)],
        4,
        &[0; 10],
        &owner_tail,
        &two_source_pairs(),
    );
    crate::test_support::indexed_header(&mut bytes, *b"302", 52);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [71, 72, 51] {
        push_owner_reference(&mut bytes, target);
    }
    crate::test_support::indexed_header(&mut bytes, *b"260", 52);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let owner_type = |guid: &str, entity: u64| {
        crate::records::entity_header::SegmentType::try_new(
            "f3d:Design1/MetaStream.dat:design-type#0".to_owned(),
            crate::design::test_support::design_type(guid, None, 1, "Fusion", vec![entity]),
        )
        .unwrap()
    };
    for types in [
        [
            owner_type(PASTE_OBJECT, 50),
            owner_type(BODY_TO_COMPONENT_OBJECT, 52),
        ],
        [
            owner_type(BODY_TO_COMPONENT_OBJECT, 52),
            owner_type(PASTE_OBJECT, 50),
        ],
    ] {
        let owners = timeline_body_owners(&bytes, &records, types.iter());
        assert_eq!(
            owners[&51]
                .iter()
                .map(|owner| owner.record)
                .collect::<Vec<_>>(),
            [50, 52]
        );
        let (construction, losses) =
            owned_with_losses(&bytes, &scope, &owners[&51], &source_nodes());
        let construction = construction.expect("owned two-body scope");
        assert_eq!(construction.timeline_owner(), Some(50));
        assert_eq!(construction.copy_source_suffixes().len(), 0);
        assert_eq!(losses, [F3dLossCode::FeatureTimelineOwnerCompeting.kind()]);
        assert_eq!(
            owned_with_losses(&bytes, &scope, &owners[&51][..1], &source_nodes())
                .0
                .expect("owned two-body scope")
                .copy_source_suffixes()
                .collect::<Vec<_>>(),
            [41, 42]
        );
        let single = timeline_body_owners(&bytes, &records, types[..1].iter());
        assert_eq!(
            owned_with_losses(&bytes, &scope, &single[&51], &source_nodes()).1,
            []
        );
    }
    let owners = timeline_body_owners(
        &bytes,
        &records,
        [owner_type(BODY_TO_COMPONENT_OBJECT, 52)].iter(),
    );
    let unreadable = TimelineBodyOwner {
        record: 49,
        body_count: 3,
        ..owners[&51][0]
    };
    let (construction, losses) = owned_with_losses(
        &bytes,
        &scope,
        &[unreadable, owners[&51][0]],
        &source_nodes(),
    );
    assert_eq!(construction.and_then(|c| c.timeline_owner()), Some(52));
    assert_eq!(losses, [F3dLossCode::FeatureTimelineOwnerCompeting.kind()]);
}
