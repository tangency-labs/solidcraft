// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::decode_reference_list;

fn reference_list_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&31u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [41u32, 42] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.push(0);
    bytes
}

#[test]
fn sketch_header_reference_run_preserves_count_and_offsets() {
    let bytes = reference_list_bytes();
    let list = decode_reference_list(&bytes, 0).expect("two references");
    assert_eq!(list.record_reference.value, Some(31));
    assert_eq!(
        list.references
            .iter()
            .map(|row| row.value)
            .collect::<Vec<_>>(),
        [41, 42]
    );
    assert_eq!(
        list.references
            .iter()
            .map(|row| row.offset)
            .collect::<Vec<_>>(),
        [14, 25]
    );
    assert_eq!(list.end, 35);
}
