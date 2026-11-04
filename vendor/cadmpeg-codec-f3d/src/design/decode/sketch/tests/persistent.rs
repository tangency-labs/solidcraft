// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{
    decode_persistent_references_from_stream, finish_persistent_references,
};
use crate::records::references::PersistentReference;

fn reference_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (name, value) in [("crv_primary_id", 71u64), ("pt_tag", 29)] {
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn scan_references(bytes: &[u8]) -> Result<Vec<PersistentReference>, cadmpeg_core::CodecError> {
    let mut indexed = Vec::new();
    decode_persistent_references_from_stream(0, "BulkStream.dat", bytes, &mut indexed)?;
    Ok(finish_persistent_references(indexed))
}

#[test]
fn persistent_reference_scan_preserves_byte_order_after_kind_scan() {
    let bytes = reference_bytes();
    let references = scan_references(&bytes).expect("two typed references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].value, 71);
    assert_eq!(references[1].value, 29);
    assert_eq!(
        references[0].id,
        crate::ids::native_persistent_reference_id("BulkStream.dat", 0)
    );
}
