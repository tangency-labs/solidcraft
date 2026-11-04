// SPDX-License-Identifier: Apache-2.0

use super::exact_scale_operation;
use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::test_support::indexed_record_offsets_for_test;
use crate::records::feature::body_ops::DesignScaleOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::lp_utf16;
use cadmpeg_core::decode::u64_from_index;
use std::collections::HashMap;

const EPS_SCALE_VALUE: f64 = 1.0e-12;

#[test]
fn legacy_scale_with_78_byte_tail_resolves_center_and_history_states() {
    for start in [0, 13] {
        let (bytes, position_at) = scale_fixture(5, 78, start);
        let scope = parse_scope(&bytes, start).expect("synthetic Scale scope");
        let offset = u64_from_index(start);
        assert_eq!(scope.frame_length(), 309);
        assert_eq!(scope.reference_count_offset(), offset + 154);
        assert_eq!(scope.kind_offset(), offset + 221);
        assert_eq!(scope.feature_ordinal_offset(), offset + 231);
        assert_eq!(scope.previous_history_state_id_offset(), Some(offset + 262));
        assert_eq!(scope.history_state_id(), Some(19));
        assert_eq!(scope.previous_history_state_id(), Some(17));

        let operation = decode_scale(&bytes, start, &HashMap::new())
            .expect("uniform Scale with 78-byte fixed tail");
        assert_eq!(operation.body_group_record_index, 102);
        assert_eq!(operation.center_record_index, 105);
        assert_eq!(operation.uniform_factor_offset, offset + 21);
        assert!((operation.uniform_factor.get() - 2.5).abs() < EPS_SCALE_VALUE);
        let center = operation.center_position.expect("point-data center");
        assert_eq!(center.offset, u64_from_index(position_at));
        for (actual, expected) in center.value.into_iter().zip([1.25, -2.5, 3.75]) {
            assert!((actual.get() - expected).abs() < EPS_SCALE_VALUE);
        }
    }
}

#[test]
fn legacy_scale_with_extended_tail_refuses_invalid_factors() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for factor in [
        0.0f64,
        -0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let mut invalid = bytes.clone();
        invalid[21..29].copy_from_slice(&factor.to_le_bytes());
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_complete_matching_references() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for marker in [29, 40, 64] {
        for delta in 0..11 {
            let mut invalid = bytes.clone();
            invalid[marker + delta] ^= 1;
            assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
        }
    }
    for offset in (16..21).chain(51..64) {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_null_guid_and_zero_separator() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for offset in 75..154 {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_finite_point_data_of_the_correct_type() {
    let (bytes, position_at) = scale_fixture(5, 78, 0);
    let point_type = HashMap::from([(105, ("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44", 3))]);
    assert!(decode_scale(&bytes, 0, &point_type).is_some());
    for axis in 0..3 {
        for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut invalid = bytes.clone();
            let at = position_at + axis * 8;
            invalid[at..at + 8].copy_from_slice(&coordinate.to_le_bytes());
            assert!(decode_scale(&invalid, 0, &point_type).is_none());
        }
    }
    let other_type = HashMap::from([(105, ("00000000-0000-0000-0000-000000000001", 3))]);
    assert!(decode_scale(&bytes, 0, &other_type).is_none());
}

#[test]
fn legacy_scale_requires_an_admitted_frame_length_and_reference_count_pair() {
    for (count, tail_length) in [(5, 76), (5, 78), (6, 76)] {
        let (bytes, _) = scale_fixture(count, tail_length, 0);
        assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    }
    for (count, tail_length, frame_length) in [
        (5, 77, 308),
        (5, 87, 318),
        (6, 72, 314),
        (6, 77, 319),
        (6, 78, 320),
    ] {
        let (bytes, _) = scale_fixture(count, tail_length, 0);
        let scope = parse_scope(&bytes, 0).expect("synthetic scope envelope");
        assert_eq!(scope.frame_length(), frame_length);
        assert_eq!(scope.reference_members().len(), count);
        assert!(decode_scale(&bytes, 0, &HashMap::new()).is_none());
    }
}

#[test]
fn legacy_scale_with_extended_tail_requires_a_complete_scope_envelope() {
    let (bytes, _) = scale_fixture(5, 78, 0);
    assert!(decode_scale(&bytes, 0, &HashMap::new()).is_some());
    for offset in [154, 155, 156, 157, 217, 218, 219, 220, 221, 316] {
        let mut invalid = bytes.clone();
        invalid[offset] ^= 1;
        assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
    }
    for ordinal in 0..5 {
        for delta in [0, 5, 6, 7, 8, 9, 10] {
            let mut invalid = bytes.clone();
            invalid[158 + ordinal * 11 + delta] ^= 1;
            assert!(decode_scale(&invalid, 0, &HashMap::new()).is_none());
        }
    }
    let mut invalid_ordinal = bytes.clone();
    invalid_ordinal[231..235].fill(0);
    assert!(decode_scale(&invalid_ordinal, 0, &HashMap::new()).is_none());

    let mut invalid_previous_state = bytes.clone();
    invalid_previous_state[262..266].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_scale(&invalid_previous_state, 0, &HashMap::new()).is_none());

    for end in 0..bytes.len() {
        assert!(decode_scale(&bytes[..end], 0, &HashMap::new()).is_none());
    }
}

fn parse_scope(bytes: &[u8], start: usize) -> Option<DesignParameterScope> {
    let records = indexed_record_offsets_for_test(bytes);
    let class_tag = "350".to_owned().try_into().unwrap();
    parse_parameter_scope(bytes, &records, 100, &class_tag, u64_from_index(start)).unwrap()
}

fn decode_scale(
    bytes: &[u8],
    start: usize,
    stream_types: &HashMap<u64, (&str, u32)>,
) -> Option<DesignScaleOperation> {
    let scope = parse_scope(bytes, start)?;
    let records = indexed_record_offsets_for_test(bytes);
    exact_scale_operation(bytes, &records, &scope, stream_types).unwrap()
}

fn scale_fixture(reference_count: usize, tail_length: usize, start: usize) -> (Vec<u8>, usize) {
    let mut bytes = vec![0; start];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"350");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2.5f64.to_le_bytes());
    for target in [105u32, 101] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(target).to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&102u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&u32::try_from(reference_count).unwrap().to_le_bytes());
    let references: &[u32] = match reference_count {
        5 => &[101, 102, 103, 104, 105],
        6 => &[101, 102, 103, 104, 106, 105],
        _ => panic!("synthetic Scale reference count"),
    };
    for target in references {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(*target).to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.extend_from_slice(&19u32.to_le_bytes());
    lp_utf16(&mut bytes, "Scale");
    let tail_at = bytes.len();
    bytes.resize(tail_at + tail_length, 0);
    bytes[tail_at..tail_at + 4].copy_from_slice(&3u32.to_le_bytes());
    let previous_at = tail_at
        + match tail_length {
            72 | 76 => 30,
            77 | 78 => 31,
            87 => 41,
            _ => panic!("synthetic Scale tail length"),
        };
    bytes[previous_at..previous_at + 4].copy_from_slice(&17u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"263");
    bytes.extend_from_slice(&100u32.to_le_bytes());

    let point_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&105u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 27]);
    let position_at = bytes.len();
    for value in [1.25f64, -2.5, 3.75] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    for _ in 0..3 {
        bytes.extend_from_slice(&(-1.0f64).to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [104u64, 103] {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    bytes.resize(point_at + 208, 0);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&105u32.to_le_bytes());
    (bytes, position_at)
}
