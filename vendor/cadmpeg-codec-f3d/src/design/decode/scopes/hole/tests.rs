// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use super::exact_hole_construction as exact_hole_construction_with_ctx;
use super::exact_hole_face_selection;
use super::HOLE_FACE_SELECTION_TYPE_GUID;
use super::HOLE_POINT_DATA_TYPE_GUID;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use std::collections::HashMap;

const EPS_HOLE_TEST_VALUE: f64 = 1.0e-12;

fn exact_hole_construction(
    bytes: &[u8],
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
    scope: &DesignParameterScope,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<crate::records::feature::hole::DesignHoleConstruction> {
    exact_hole_construction_with_ctx(
        bytes,
        records,
        scope,
        stream_types,
        &crate::records::feature::scope::DesignFeatureKind::Hole,
    )
    .unwrap()
}

#[test]
fn hole_carrier_reads_borrowed_as_built_scope() {
    let (bytes, mut scope, _, _) = hole_point_stream();
    scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::AsBuilt
                .try_into()
                .unwrap();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let types = HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4))]);
    let construction = exact_hole_construction_with_ctx(
        &bytes,
        &records,
        &scope,
        &types,
        &crate::records::feature::scope::DesignFeatureKind::AsBuilt,
    )
    .unwrap();
    assert_eq!(construction.unwrap().point_record_index, 55);
    assert!(exact_hole_construction_with_ctx(
        &bytes,
        &records,
        &scope,
        &types,
        &crate::records::feature::scope::DesignFeatureKind::Hole
    )
    .unwrap()
    .is_none());
}

fn assert_f64_array<const N: usize>(actual: [f64; N], expected: [f64; N]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < EPS_HOLE_TEST_VALUE);
    }
}

fn hole_point_stream() -> (Vec<u8>, DesignParameterScope, usize, usize) {
    hole_point_stream_version(4)
}

fn hole_point_stream_version(version: u32) -> (Vec<u8>, DesignParameterScope, usize, usize) {
    hole_point_stream_tail(version, if version == 1 { &[0x55; 13] } else { &[] })
}

fn hole_point_stream_tail(
    version: u32,
    class_tail: &[u8],
) -> (Vec<u8>, DesignParameterScope, usize, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    let position_at = bytes.len();
    for value in [1.25_f64, -2.5, 3.75] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.125_f64, -0.25] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&19u32.to_le_bytes());
    if version == 4 {
        bytes.push(0x7f);
        for value in [-1.0_f64, -1.0, -1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let input_reference_at = bytes.len();
    bytes.push(1);
    bytes.extend_from_slice(&378u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(class_tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "generated:hole#0",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(55);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, position_at, input_reference_at)
}

#[test]
fn hole_construction_reads_the_versioned_point_and_direction_carrier() {
    let (bytes, scope, position_at, input_reference_at) = hole_point_stream();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4))]),
    )
    .expect("hole point carrier");

    assert_eq!(construction.point_record_index, 55);
    assert_eq!(construction.point_record_byte_offset, 0);
    assert_f64_array(
        construction
            .position
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.25, -2.5, 3.75],
    );
    assert_eq!(construction.position_offset, u64_from_index(position_at));
    assert_f64_array(
        construction
            .direction
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.0, 0.0, 1.0],
    );
    assert_eq!(
        construction.direction_offset,
        u64_from_index(position_at + 24)
    );
    assert_f64_array(
        construction
            .point_parameters
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.125, -0.25],
    );
    assert_eq!(construction.reference_type, 19);
    assert_eq!(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.prefix),
        Some(0x7f)
    );
    assert_f64_array(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.data.value.map(cadmpeg_ir::scalar::FiniteReal::get))
            .expect("version-four tangent point data"),
        [-1.0, -1.0, -1.0],
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [378]
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.offset)
            .collect::<Vec<_>>(),
        [u64_from_index(input_reference_at + 1)]
    );
}

#[test]
fn hole_construction_reads_the_legacy_point_and_direction_carrier_without_tangent_data() {
    let (bytes, scope, position_at, input_reference_at) = hole_point_stream_version(1);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, 1))]),
    )
    .expect("legacy hole point carrier");

    assert_eq!(construction.point_record_index, 55);
    assert_f64_array(
        construction
            .position
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [1.25, -2.5, 3.75],
    );
    assert_eq!(construction.position_offset, u64_from_index(position_at));
    assert_f64_array(
        construction
            .direction
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.0, 0.0, 1.0],
    );
    assert_f64_array(
        construction
            .point_parameters
            .map(cadmpeg_ir::scalar::FiniteReal::get),
        [0.125, -0.25],
    );
    assert_eq!(construction.reference_type, 19);
    assert_eq!(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.prefix),
        None
    );
    assert_eq!(construction.tangent_point_data, None);
    assert_eq!(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.data.offset),
        None
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [378]
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.offset)
            .collect::<Vec<_>>(),
        [u64_from_index(input_reference_at + 1)]
    );
}

/// The bytes that a version-four carrier stores after its counted reference
/// run: two zero bytes and one more counted marked reference.
const VERSION_FOUR_CLASS_TAIL: [u8; 17] = [0, 0, 1, 0, 0, 0, 1, 132, 1, 0, 0, 0, 0, 0, 0, 0, 0];

#[test]
fn hole_construction_reads_a_version_four_class_tail_before_its_paired_header() {
    let (bytes, scope, _, _) = hole_point_stream_tail(4, &VERSION_FOUR_CLASS_TAIL);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4))]),
    )
    .expect("version-four carrier with its class tail");
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [378]
    );
}

#[test]
fn hole_construction_refuses_another_record_before_its_paired_header() {
    let mut tail = Vec::new();
    indexed_header(&mut tail, *b"300", 90);
    for version in [1, 4] {
        let (bytes, scope, _, _) = hole_point_stream_tail(version, &tail);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert!(exact_hole_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, version))]),
        )
        .is_none());
    }
}

/// One version-three point-data record frame with a single input reference.
fn point_data_frame(
    bytes: &mut Vec<u8>,
    record_index: u32,
    position: [f64; 3],
    reference_type: u32,
) {
    indexed_header(bytes, *b"325", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.push(0);
    for value in position {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&reference_type.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&(-1.0_f64).to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&90u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    indexed_header(bytes, *b"259", record_index);
}

fn with_point_records(
    scope: &mut DesignParameterScope,
    bytes: &mut Vec<u8>,
    frames: &[(u32, [f64; 3], u32)],
) {
    for (record_index, position, reference_type) in frames {
        point_data_frame(bytes, *record_index, *position, *reference_type);
    }
    scope
        .try_edit(|draft| {
            let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
            values.extend(frames.iter().map(|frame| frame.0));
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(values);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
}

#[test]
fn hole_locations_read_every_point_record_in_reference_order() {
    let (mut bytes, mut scope, _, _) = hole_point_stream();
    with_point_records(
        &mut scope,
        &mut bytes,
        &[(71, [3.0, 1.5, 2.0], 10), (70, [1.0, 1.5, 2.0], 10)],
    );
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([
            (55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4)),
            (70_u64, (super::POINT_DATA_TYPE_GUID, 3)),
            (71_u64, (super::POINT_DATA_TYPE_GUID, 3)),
        ]),
    )
    .expect("hole carrier with its point records");
    let read: Vec<_> = construction
        .locations
        .iter()
        .map(|location| {
            (
                location.record_index,
                location.position.map(cadmpeg_ir::scalar::FiniteReal::get),
                location.reference_type,
            )
        })
        .collect();
    assert_eq!(read, [(71, [3.0, 1.5, 2.0], 10), (70, [1.0, 1.5, 2.0], 10)]);
    for location in &construction.locations {
        let at = usize::try_from(location.position_offset).unwrap();
        assert_eq!(
            cadmpeg_core::decode::View::f64_le_at(&bytes, at),
            Some(location.position[0].get())
        );
    }
}

#[test]
fn hole_locations_keep_the_sole_input_of_a_vertex_rule_record() {
    let (mut bytes, mut scope, _, _) = hole_point_stream();
    with_point_records(
        &mut scope,
        &mut bytes,
        &[(70, [1.0, 1.5, 2.0], 10), (71, [3.0, 1.5, 2.0], 17)],
    );
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([
            (55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4)),
            (70_u64, (super::POINT_DATA_TYPE_GUID, 3)),
            (71_u64, (super::POINT_DATA_TYPE_GUID, 3)),
        ]),
    )
    .expect("hole carrier with its point records");
    let inputs: Vec<_> = construction
        .locations
        .iter()
        .map(|location| {
            location
                .vertex_input
                .as_ref()
                .map(|input| (input.record_index(), input.carrier().is_none()))
        })
        .collect();
    assert_eq!(inputs, [Some((90, true)), None]);
}

#[test]
fn hole_locations_drop_the_run_when_one_point_record_does_not_read() {
    let (mut bytes, mut scope, _, _) = hole_point_stream();
    with_point_records(&mut scope, &mut bytes, &[(70, [1.0, 1.5, 2.0], 10)]);
    scope
        .try_edit(|draft| {
            let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
            values.push(71);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(values);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([
            (55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4)),
            (70_u64, (super::POINT_DATA_TYPE_GUID, 3)),
            (71_u64, (super::POINT_DATA_TYPE_GUID, 3)),
        ]),
    )
    .expect("hole carrier");
    assert!(construction.locations.is_empty());
}

#[test]
fn hole_face_selection_reads_the_direct_persistent_identity_envelope() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    indexed_header(&mut bytes, *b"265", 100);
    indexed_header(&mut bytes, *b"301", 101);
    indexed_header(&mut bytes, *b"446", 102);
    let identity_at = bytes.len();
    indexed_header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 10]);
    let primary_identity_at = bytes.len();
    bytes.extend_from_slice(&246u64.to_le_bytes());
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"311", 104);

    let mut scope = DesignParameterScope::empty(
        "generated:hole#0",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(100);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let selection = exact_hole_face_selection(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &HashMap::from([(100_u64, (HOLE_FACE_SELECTION_TYPE_GUID, 1))]),
    )
    .expect("direct Hole face selection");

    assert_eq!(selection.record_index, 100);
    assert_eq!(selection.class_tag.as_str(), "333");
    assert_eq!(
        selection.asset_id.as_str(),
        "53aa8ab4-194a-434b-bd52-8c6d761dc147"
    );
    assert_eq!(
        selection.context_id.as_str(),
        "8e685642-4d68-4909-96d0-0dd4437491b6"
    );
    assert_eq!(selection.identity_record_index, 103);
    assert_eq!(
        selection.identity_record_offset,
        u64_from_index(identity_at)
    );
    assert_eq!(selection.primary_identity, 246);
    assert_eq!(
        selection.primary_identity_offset,
        u64_from_index(primary_identity_at)
    );
    assert_eq!(selection.next_record_index, 104);
    assert_eq!(selection.next_byte_offset, u64_from_index(next_at));
    assert!(selection.historical_face_candidates.is_empty());
}

const LEGACY: super::ExtentLayout = super::ExtentLayout {
    leading_word: false,
    control: false,
};
const BARE: super::ExtentLayout = super::ExtentLayout {
    leading_word: false,
    control: true,
};
const WORDED: super::ExtentLayout = super::ExtentLayout {
    leading_word: true,
    control: true,
};

fn extent_payload(controls: &[u8], saved: bool) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    bytes.extend_from_slice(&u32::from(saved).to_le_bytes());
    if saved {
        for text in [
            "SavedHoleDepthInNonDistanceExtentsCase",
            "IntrinsicMetaTypedouble",
        ] {
            bytes.extend_from_slice(&u32::try_from(text.len()).unwrap().to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    }
    bytes.extend_from_slice(controls);
    bytes
}

#[test]
fn hole_extent_reads_distance_and_saved_through_all() {
    for control in [0, 1] {
        let bytes = extent_payload(&[0, 0, 0, 0, 1, 1, control, 0], control == 1);
        let extent = super::hole_extent_payload(&bytes, 0, WORDED).unwrap();
        assert_eq!(extent.extent_control, Some(control));
        let word = extent.leading_word.unwrap();
        assert_eq!(word.value, 0);
        assert_eq!(
            cadmpeg_core::decode::View::u32_le_at(&bytes, usize::try_from(word.offset).unwrap()),
            Some(0)
        );
        let saved = extent.saved_non_distance_depth;
        assert_eq!(saved.is_some(), control == 1);
        if let Some(saved) = saved {
            assert!((saved.value.get() - 1.0).abs() < EPS_HOLE_TEST_VALUE);
            assert_eq!(
                cadmpeg_core::decode::View::f64_le_at(
                    &bytes,
                    usize::try_from(saved.offset).unwrap()
                ),
                Some(1.0)
            );
        }
    }
}

#[test]
fn hole_extent_leading_word_follows_the_type_version() {
    let word = extent_payload(&[2, 0, 0, 0, 1, 1, 0, 0], false);
    let extent = super::hole_extent_payload(&word, 0, WORDED).unwrap();
    assert_eq!(extent.leading_word.map(|word| word.value), Some(2));
    assert!(super::hole_extent_payload(&word, 0, BARE).is_none());

    let bare = extent_payload(&[1, 1, 0, 0], false);
    let extent = super::hole_extent_payload(&bare, 0, BARE).unwrap();
    assert_eq!(extent.extent_control, Some(0));
    assert_eq!(extent.leading_word, None);
    assert!(super::hole_extent_payload(&bare, 0, WORDED).is_none());

    let legacy = extent_payload(&[1, 1, 0, 1], false);
    let extent = super::hole_extent_payload(&legacy, 0, LEGACY).unwrap();
    assert_eq!(extent.extent_control, None);
    assert_eq!(extent.leading_word, None);
    assert!(super::hole_extent_payload(&extent_payload(&[1, 1, 0, 1], true), 0, LEGACY).is_none());
    assert!(super::hole_extent_payload(&extent_payload(&[1, 0, 0, 1], false), 0, LEGACY).is_none());

    assert_eq!(super::extent_layout(3), Some(LEGACY));
    assert_eq!(super::extent_layout(4), Some(BARE));
    assert_eq!(super::extent_layout(5), Some(WORDED));
    assert_eq!(super::extent_layout(7), Some(WORDED));
    for unproven in [1, 2, 6, 8] {
        assert_eq!(super::extent_layout(unproven), None);
    }
}

#[test]
fn hole_extent_refuses_other_control_spellings_and_truncation() {
    for controls in [
        [0, 0, 0, 0, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 0, 0],
        [0, 0, 0, 0, 1, 1, 2, 0],
        [0, 0, 0, 0, 1, 1, 1, 1],
    ] {
        assert!(super::hole_extent_payload(&extent_payload(&controls, true), 0, WORDED).is_none());
    }
    let bytes = extent_payload(&[0, 0, 0, 0, 1, 1, 1, 0], true);
    for end in 0..bytes.len() {
        assert!(super::hole_extent_payload(&bytes[..end], 0, WORDED).is_none());
    }
}

#[test]
fn hole_direction_control_reads_after_the_controls_of_a_worded_extent() {
    for control in [0, 1] {
        let mut controls = vec![0, 0, 0, 0, 1, 1, 0, 0];
        controls.extend_from_slice(&[0; 24]);
        controls.push(control);
        let bytes = extent_payload(&controls, false);
        let extent = super::hole_extent_payload(&bytes, 0, WORDED).unwrap();
        let read = extent.direction_control.unwrap();
        assert_eq!(read.value, control);
        assert_eq!(usize::try_from(read.offset).unwrap(), bytes.len() - 1);
    }
}

#[test]
fn hole_direction_control_is_absent_when_it_is_not_stored_or_not_valid() {
    let mut controls = vec![0, 0, 0, 0, 1, 1, 0, 0];
    controls.extend_from_slice(&[0; 24]);
    let short = extent_payload(&controls, false);
    let extent = super::hole_extent_payload(&short, 0, WORDED).unwrap();
    assert_eq!(extent.direction_control, None);

    controls.push(2);
    let invalid = extent_payload(&controls, false);
    let extent = super::hole_extent_payload(&invalid, 0, WORDED).unwrap();
    assert_eq!(extent.direction_control, None);
    assert_eq!(extent.extent_control, Some(0));

    let mut bare = vec![1, 1, 0, 0];
    bare.extend_from_slice(&[0; 24]);
    bare.push(1);
    let extent = super::hole_extent_payload(&extent_payload(&bare, false), 0, BARE).unwrap();
    assert_eq!(extent.direction_control, None);
}

#[test]
fn hole_extent_reads_the_shared_absent_property_prologue() {
    let bytes = [0, 0, 5, 0, 0, 0, 1, 1, 0, 0];
    let extent = super::hole_extent_payload(&bytes, 0, WORDED).unwrap();
    assert_eq!(extent.extent_control, Some(0));
    assert_eq!(extent.leading_word.map(|word| word.value), Some(5));
    assert!(extent.saved_non_distance_depth.is_none());
}

/// Extent controls, saved-depth property, Hole type version, expected control.
type ExtentForm = (&'static [u8], bool, Option<u32>, Option<Option<u8>>);

#[test]
fn hole_extent_uses_the_admitted_scope_frame_without_a_point_carrier() {
    let forms: [ExtentForm; 6] = [
        (&[0, 0, 0, 0, 1, 1, 0, 0], false, Some(7), Some(Some(0))),
        (&[0, 0, 0, 0, 1, 1, 1, 0], true, Some(5), Some(Some(1))),
        (&[1, 1, 0, 0], false, Some(4), Some(Some(0))),
        (&[1, 1, 0, 1], false, Some(3), Some(None)),
        (&[1, 1, 0, 0], false, Some(7), None),
        (&[0, 0, 0, 0, 1, 1, 0, 0], false, None, None),
    ];
    for (controls, saved, version, expected) in forms {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 1);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&extent_payload(controls, saved));
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        lp_utf16(&mut bytes, "Hole");
        let mut tail = [0; 78];
        tail[..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"257", 1);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scope = crate::design::decode::scopes::parameter_scope::parse_parameter_scope(
            &bytes,
            &records,
            1,
            &"256".to_owned().try_into().unwrap(),
            0,
        )
        .unwrap()
        .unwrap();
        assert!(scope.hole_construction().is_none());
        assert_eq!(
            super::hole_extent(&bytes, &scope, version).map(|extent| extent.extent_control),
            expected
        );
    }
}
