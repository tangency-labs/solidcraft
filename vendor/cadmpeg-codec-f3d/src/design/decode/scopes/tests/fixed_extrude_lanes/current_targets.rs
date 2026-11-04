// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::layout::current_extrude_face_target_payload as target;

fn reference(bytes: &mut [u8], at: usize, index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
}

fn fixture(faces: bool) -> (Vec<u8>, DesignParameterScope) {
    let extent = if faces {
        DesignExtrudeExtent::TwoSidedToFaces
    } else {
        DesignExtrudeExtent::TwoSidedDistance
    };
    let mut bytes = vec![0; 300];
    let slots = if faces {
        [None, None, None, None, None, Some(731), None]
    } else {
        [None, Some(701), Some(731), None, None, Some(751), None]
    };
    let mut at = 70;
    for slot in slots {
        if let Some(index) = slot {
            reference(&mut bytes, at, index);
            at += 11;
        } else {
            at += 1;
        }
    }
    bytes[at..at + 4].copy_from_slice(&(if faces { 2u32 } else { 1 }).to_le_bytes());
    let second = at
        + if faces {
            target::SECOND_SIDE_EXTENT
        } else {
            13
        };
    bytes[second..second + 4].copy_from_slice(&(if faces { 2u32 } else { 1 }).to_le_bytes());
    if faces {
        reference(&mut bytes, at + target::FIRST_SIDE_OFFSET_REFERENCE, 701);
        reference(&mut bytes, at + target::SECOND_SIDE_OFFSET_REFERENCE, 751);
        reference(&mut bytes, at + target::SECOND_SIDE_TAPER_REFERENCE, 771);
    } else {
        reference(&mut bytes, second + 4, 771);
    }
    let (lanes, _) = scalar_lanes(&[(701, 9, 0.4), (731, 9, 0.8), (751, 9, 0.2), (771, 9, 0.6)]);
    bytes.extend(lanes);
    let mut scope = slotted_extrude(
        extent,
        DesignExtrudeStart::ProfilePlane,
        u64_from_index(at),
        vec![701, 731, 751, 771],
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(Some(slot)) =
        scope.payload_mut()
    {
        if let Some(DesignExtrudePrologue::ReferenceAware {
            side_extent_discriminators,
            side_extent_discriminator_offsets,
            ..
        }) = &mut slot.extrude_prologue
        {
            *side_extent_discriminators = if faces { [2, 2] } else { [1, 1] };
            side_extent_discriminator_offsets[1] = u64_from_index(second);
        }
    }
    (bytes, scope)
}

#[test]
fn current_two_face_extent_assigns_both_fixed_offsets_and_tapers() {
    let (bytes, scope) = fixture(true);
    let fixed = decoded(&bytes, &scope).unwrap();
    assert_eq!(fixed.side_one_offset.unwrap().record_index, 701);
    assert_eq!(fixed.taper_angle.unwrap().record_index, 731);
    assert_eq!(fixed.side_two_offset.unwrap().record_index, 751);
    assert_eq!(fixed.side_two_taper_angle.unwrap().record_index, 771);
    assert!(fixed.along_distance.is_none());
    assert!(fixed.against_distance.is_none());
}

#[test]
fn current_two_distance_extent_assigns_nonordinal_fixed_lanes() {
    let (bytes, scope) = fixture(false);
    let fixed = decoded(&bytes, &scope).unwrap();
    let Some(DesignFixedExtrudeDistance::FixedScalar(along)) = fixed.along_distance else {
        panic!("first distance")
    };
    assert_eq!(along.record_index, 731);
    assert_eq!(fixed.against_distance.unwrap().record_index, 701);
    assert_eq!(fixed.taper_angle.unwrap().record_index, 751);
    assert_eq!(fixed.side_two_taper_angle.unwrap().record_index, 771);
}
