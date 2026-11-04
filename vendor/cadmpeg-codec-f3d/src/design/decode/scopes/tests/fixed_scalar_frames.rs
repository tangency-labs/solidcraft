// SPDX-License-Identifier: Apache-2.0
//! Compact 103-byte fixed scalar frames.

use crate::design::decode::scopes::shared_frames::exact_fixed_scalar;

/// One 103-byte fixed scalar frame owned by scope `8622`, with lane ordinal 1
/// and value 0.05, followed by its same-index paired header. Bytes 78 and 79
/// hold the compact scalar-owner trailer selector.
fn compact_scalar_frame(trailer: [u8; 2]) -> Vec<u8> {
    let record_index = 9294u32;
    let owner = 8622u32;
    let marked = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&value.to_le_bytes());
    };
    let mut bytes = vec![0; 103];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"304");
    bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
    bytes[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    marked(&mut bytes, 24, owner);
    bytes[35] = 1;
    bytes[40..48].copy_from_slice(&0.05f64.to_le_bytes());
    marked(&mut bytes, 48, record_index + 1);
    bytes[59] = 0x81;
    marked(&mut bytes, 67, owner);
    bytes[78..80].copy_from_slice(&trailer);
    marked(&mut bytes, 80, record_index + 2);
    marked(&mut bytes, 92, owner);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"264");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes
}

/// A compact fixed scalar frame stores either an eight-byte repeated-scope
/// trailer or a six-byte trailer followed by `u8 1 + u8 variant`.
#[test]
fn compact_fixed_scalar_admits_both_trailer_forms() {
    for trailer in [[0, 0], [1, 0], [1, 1]] {
        let bytes = compact_scalar_frame(trailer);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let scalar = exact_fixed_scalar(&bytes, &records, 9294)
            .unwrap_or_else(|| panic!("trailer {trailer:?} admits the scalar frame"));
        assert_eq!(scalar.owner_record_index, Some(8622));
        assert_eq!(scalar.ordinal, 1);
        assert_eq!(scalar.value.get(), 0.05);
        assert_eq!(scalar.value_offset, 40);
    }
    for trailer in [[0, 1], [1, 2], [2, 0]] {
        let bytes = compact_scalar_frame(trailer);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        assert!(
            exact_fixed_scalar(&bytes, &records, 9294).is_none(),
            "trailer {trailer:?} is not a compact scalar-owner trailer"
        );
    }
}
