// SPDX-License-Identifier: Apache-2.0
//! Fixed scalar lanes of distance and to-face Extrudes.

use crate::design::decode::scopes::fixed_parameters::exact_fixed_extrude_parameters;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::u64_from_index;

const SCOPE: u32 = 10;

/// 104-byte fixed scalar frames owned by [`SCOPE`], one per `(record, ordinal,
/// value)` lane, each followed by its paired header.
fn scalar_lanes(lanes: &[(u32, u8, f64)]) -> (Vec<u8>, Vec<u64>) {
    let mut bytes = Vec::new();
    let mut value_offsets = Vec::new();
    for &(record_index, ordinal, value) in lanes {
        let start = bytes.len();
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"273");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&SCOPE.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"272");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
        value_offsets.push(u64_from_index(start + 40));
    }
    (bytes, value_offsets)
}

fn distance_extrude(extent: DesignExtrudeExtent, references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Extrude,
        SCOPE,
    );
    let direction = match extent {
        DesignExtrudeExtent::TwoSidedDistance => 2,
        DesignExtrudeExtent::SymmetricDistance => 3,
        _ => 1,
    };
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 28,
                direction_face_extend_values: [direction, 0],
                side_extent_discriminators: [1, 1],
                side_extent_discriminator_offsets: [77, 90],
                first_side_target_ordinal: None,
                extent,
                direction_face_extend_offsets: [32, 36],
                direction_reversed: false,
                direction_reversed_offset: 40,
                solid_operation: true,
                solid_operation_offset: 41,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 42,
            });
    }
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn decoded(bytes: &[u8], scope: &DesignParameterScope) -> Option<DesignFixedExtrudeParameters> {
    exact_fixed_extrude_parameters(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &[],
        &[],
    )
}

fn lane(record_index: u32, value: f64, value_offset: u64) -> DesignFixedExtrudeScalar {
    DesignFixedExtrudeScalar {
        value: crate::test_support::real(value),
        record_index,
        value_offset,
    }
}

fn distance_lane(
    record_index: u32,
    value: f64,
    value_offset: u64,
) -> DesignFixedExtrudeScalar<cadmpeg_ir::scalar::NonZeroReal> {
    DesignFixedExtrudeScalar {
        value: cadmpeg_ir::scalar::NonZeroReal::new(value).unwrap(),
        record_index,
        value_offset,
    }
}

/// A two-sided distance Extrude without indexed parameters stores its first
/// distance, first taper, second distance, and second taper in lanes zero
/// through three.
#[test]
fn two_sided_distance_extrude_decodes_four_fixed_lanes() {
    let (bytes, offsets) = scalar_lanes(&[(11, 0, 4.3), (14, 1, 0.0), (17, 2, 0.2), (20, 3, 0.0)]);
    let scope = distance_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        vec![11, 14, 17, 20, 30],
    );
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                11, 4.3, offsets[0]
            ))),
            taper_angle: Some(lane(14, 0.0, offsets[1])),
            against_distance: Some(distance_lane(17, 0.2, offsets[2])),
            side_two_taper_angle: Some(lane(20, 0.0, offsets[3])),
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );

    // A two-sided Extrude needs both distance lanes.
    let scope = distance_extrude(DesignExtrudeExtent::TwoSidedDistance, vec![11, 14, 30]);
    assert_eq!(decoded(&bytes, &scope), None);
    // A one-sided Extrude has no second-side lanes.
    let scope = distance_extrude(
        DesignExtrudeExtent::OneSidedDistance,
        vec![11, 14, 17, 20, 30],
    );
    assert_eq!(decoded(&bytes, &scope), None);
}

/// A symmetric distance Extrude without indexed parameters stores its distance
/// and taper in lanes zero and one.
#[test]
fn symmetric_distance_extrude_decodes_two_fixed_lanes() {
    let (bytes, offsets) = scalar_lanes(&[(11, 0, 1.2), (14, 1, 0.0)]);
    let scope = distance_extrude(DesignExtrudeExtent::SymmetricDistance, vec![11, 14, 30]);
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                11, 1.2, offsets[0]
            ))),
            taper_angle: Some(lane(14, 0.0, offsets[1])),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
}

/// A reference-aware prologue at offset zero whose seven nullable slots
/// start at offset 70, followed by `lanes`. Returns the bytes, the lane value
/// offsets, and the offset of the first-side extent value.
fn slotted_prologue(
    slots: [Option<u32>; 7],
    payload_reference: Option<u32>,
    lanes: &[(u32, u8, f64)],
) -> (Vec<u8>, Vec<u64>, u64) {
    let mut bytes = vec![0; 70];
    for slot in slots {
        match slot {
            None => bytes.push(0),
            Some(record_index) => {
                bytes.push(1);
                bytes.extend_from_slice(&record_index.to_le_bytes());
                bytes.extend_from_slice(&[0; 6]);
            }
        }
    }
    let extent_at = bytes.len();
    match payload_reference {
        Some(record_index) => {
            bytes.extend_from_slice(&2u32.to_le_bytes());
            bytes.push(1);
            bytes.extend_from_slice(&record_index.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        None => bytes.extend_from_slice(&1u32.to_le_bytes()),
    }
    bytes.resize(bytes.len() + 16, 0);
    let prologue_len = bytes.len();
    let (lane_bytes, offsets) = scalar_lanes(lanes);
    bytes.extend_from_slice(&lane_bytes);
    let offsets = offsets
        .into_iter()
        .map(|offset| offset + u64_from_index(prologue_len))
        .collect();
    (bytes, offsets, u64_from_index(extent_at))
}

fn slotted_extrude(
    extent: DesignExtrudeExtent,
    start: DesignExtrudeStart,
    extent_at: u64,
    references: Vec<u32>,
) -> DesignParameterScope {
    let mut scope = distance_extrude(extent, references);
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(Some(slot)) =
        scope.payload_mut()
    {
        if let Some(DesignExtrudePrologue::ReferenceAware {
            side_extent_discriminators,
            side_extent_discriminator_offsets,
            start: prologue_start,
            ..
        }) = &mut slot.extrude_prologue
        {
            *side_extent_discriminators = match extent {
                DesignExtrudeExtent::OneSidedToFace => [2, 0],
                _ => [1, 0],
            };
            *side_extent_discriminator_offsets = [extent_at, extent_at + 13];
            *prologue_start = start;
        }
    }
    scope
}

/// A face-start one-sided distance Extrude stores its start offset, distance,
/// and taper in the lanes that nullable slots zero, two, and five name; their
/// lane ordinals do not follow the distance-lane order.
#[test]
fn face_start_extrude_assigns_lanes_by_prologue_slot() {
    let (bytes, offsets, extent_at) = slotted_prologue(
        [Some(11), None, Some(14), Some(30), None, Some(17), None],
        None,
        &[(11, 0, 0.0), (14, 1, -0.29), (17, 2, 0.0)],
    );
    let scope = slotted_extrude(
        DesignExtrudeExtent::OneSidedDistance,
        DesignExtrudeStart::FromFace,
        extent_at,
        vec![11, 14, 17, 30],
    );
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                14, -0.29, offsets[1]
            ))),
            taper_angle: Some(lane(17, 0.0, offsets[2])),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: Some(lane(11, 0.0, offsets[0])),
            side_one_offset: None,
            ..Default::default()
        })
    );

    // A lane that no slot names is not assigned.
    let (bytes, _, extent_at) = slotted_prologue(
        [None, None, Some(14), Some(30), None, Some(17), None],
        None,
        &[(11, 0, 0.0), (14, 1, -0.29), (17, 2, 0.0)],
    );
    let scope = slotted_extrude(
        DesignExtrudeExtent::OneSidedDistance,
        DesignExtrudeStart::FromFace,
        extent_at,
        vec![11, 14, 17, 30],
    );
    assert_eq!(decoded(&bytes, &scope), None);
}

/// A to-face Extrude names its termination offset by the first reference of
/// the extent target payload and its taper by nullable slot five.
#[test]
fn to_face_extrude_assigns_offset_and_taper_lanes() {
    let (bytes, offsets, extent_at) = slotted_prologue(
        [None, None, None, Some(30), None, Some(17), Some(40)],
        Some(11),
        &[(11, 0, 0.25), (17, 1, 0.0)],
    );
    let scope = slotted_extrude(
        DesignExtrudeExtent::OneSidedToFace,
        DesignExtrudeStart::ProfilePlane,
        extent_at,
        vec![11, 17, 30, 40],
    );
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: None,
            taper_angle: Some(lane(17, 0.0, offsets[1])),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: Some(lane(11, 0.25, offsets[0])),
            ..Default::default()
        })
    );
}

/// An Extrude with two fixed lanes [0.0, nonzero] decodes lane zero as `TaperAngle`
/// and lane one as `AlongDistance`, regardless of table order.
#[test]
fn swapped_two_lane_extrude_decodes_taper_and_distance() {
    let (bytes, offsets) = scalar_lanes(&[(11, 0, 0.0), (14, 1, 3.5)]);
    let scope = distance_extrude(DesignExtrudeExtent::OneSidedDistance, vec![11, 14, 30]);
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                14, 3.5, offsets[1]
            ))),
            taper_angle: Some(lane(11, 0.0, offsets[0])),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );

    // Reversed table order
    let (bytes_rev, offsets_rev) = scalar_lanes(&[(14, 1, 3.5), (11, 0, 0.0)]);
    let scope_rev = distance_extrude(DesignExtrudeExtent::OneSidedDistance, vec![14, 11, 30]);
    assert_eq!(
        decoded(&bytes_rev, &scope_rev),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                14,
                3.5,
                offsets_rev[0]
            ))),
            taper_angle: Some(lane(11, 0.0, offsets_rev[1])),
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        })
    );
}

mod current_targets;
