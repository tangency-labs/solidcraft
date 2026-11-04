// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::exact_direct_face_operation;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::test_support::indexed_record_offsets_for_test;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::scope::DesignParameterScope;

fn header(bytes: &mut Vec<u8>, class: &[u8; 3], index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class);
    bytes.extend_from_slice(&index.to_le_bytes());
}

fn reference(bytes: &mut Vec<u8>, index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

fn fixture(count: u32, marker_one: bool, tail_length: usize) -> (Vec<u8>, usize) {
    let mut bytes = Vec::new();
    header(&mut bytes, b"277", 71);
    bytes.resize(104, 0);
    bytes[40..48].copy_from_slice(&(-0.5f64).to_le_bytes());
    header(&mut bytes, b"261", 71);
    let start = bytes.len();
    header(&mut bytes, b"301", 17);
    bytes.resize(start + 20, 0);
    bytes.push(u8::from(marker_one));
    if marker_one {
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    reference(&mut bytes, 71);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 301);
    bytes.resize(start + if marker_one { 130 } else { 126 }, 0);
    bytes.extend_from_slice(&count.to_le_bytes());
    for ordinal in 0..count {
        reference(
            &mut bytes,
            if ordinal + 1 == count {
                71
            } else {
                301 + ordinal
            },
        );
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut bytes, "OffsetFaces");
    let tail = bytes.len();
    bytes.resize(tail + tail_length, 0);
    bytes[tail..tail + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[tail + 31..tail + 35].copy_from_slice(&2u32.to_le_bytes());
    header(&mut bytes, b"261", 17);
    (bytes, start)
}

fn scope(bytes: &[u8], start: usize) -> Option<DesignParameterScope> {
    parse_parameter_scope(
        bytes,
        &indexed_record_offsets_for_test(bytes),
        17,
        &"301".to_owned().try_into().unwrap(),
        u64::try_from(start).unwrap(),
    )
    .unwrap()
}

fn decodes(bytes: &[u8], start: usize) -> bool {
    scope(bytes, start).is_some_and(|scope| {
        matches!(
            exact_direct_face_operation(bytes, &indexed_record_offsets_for_test(bytes), &scope),
            Some(DesignDirectFaceOperation::OffsetFaces(value))
                if value.distance.get() == -0.5 && value.distance_record_index == 71
        )
    })
}

#[test]
fn offset_faces_counted_member_runs_decode() {
    for count in [3, 4, 5, 7, 8, 10, 11] {
        for marker_one in [false, true] {
            for tail_length in [77, 78] {
                let (bytes, start) = fixture(count, marker_one, tail_length);
                assert!(decodes(&bytes, start), "{count} {marker_one} {tail_length}");
                let parsed = scope(&bytes, start).unwrap();
                assert_eq!(
                    parsed.reference_members().len(),
                    usize::try_from(count).unwrap()
                );
                assert_eq!(
                    parsed.frame_length(),
                    u64::try_from(bytes.len() - start - 11).unwrap()
                );
            }
        }
    }
}

#[test]
fn offset_faces_counted_member_runs_refuse_mutations() {
    for count in [1, 2] {
        let (bytes, start) = fixture(count, true, 78);
        assert!(!decodes(&bytes, start));
    }
    for count in [8, 10] {
        for marker_one in [false, true] {
            let (bytes, start) = fixture(count, marker_one, 78);
            let marker = if marker_one { 25 } else { 21 };
            for (offset, value) in [
                (20, u8::from(!marker_one)),
                (marker, 0),
                (marker + 1, 72),
                (marker + 5, 1),
                (marker + 11, 2),
                (marker + 15, 0),
                (marker + 16, 46),
                (marker + 20, 1),
            ] {
                let mut changed = bytes.clone();
                changed[start + offset] = value;
                assert!(!decodes(&changed, start), "{count} {marker_one} {offset}");
            }
            let mut changed = bytes.clone();
            changed[40..48].copy_from_slice(&f64::NAN.to_le_bytes());
            assert!(!decodes(&changed, start));
        }
    }
}

#[test]
fn offset_faces_counted_member_runs_refuse_truncation() {
    for count in [8, 10] {
        for marker_one in [false, true] {
            let (bytes, start) = fixture(count, marker_one, 78);
            for end in 0..bytes.len() {
                assert!(!decodes(&bytes[..end], start), "{count} {marker_one} {end}");
            }
        }
    }
}
