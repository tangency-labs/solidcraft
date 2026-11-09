// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::parse_work_point_sketch_point_frame;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;

#[test]
fn direct_sketch_point_selection_reads_owner_and_persistent_ids() {
    let record_index = 100;
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"338", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "7b3dec6f-f69c-4bfa-a537-9274f341c66e");
    lp_utf16(&mut bytes, "2b40eee7-408c-429b-9216-8b6f7e9a62c9");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"258", record_index);
    indexed_header(&mut bytes, *b"294", record_index + 1);
    indexed_header(&mut bytes, *b"303", record_index + 2);
    indexed_header(&mut bytes, *b"305", record_index + 3);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1627u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&379u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"288", record_index + 4);

    let selection = parse_work_point_sketch_point_frame(&bytes, record_index, 0)
        .expect("direct sketch-point selection");
    assert_eq!(selection.sketch_record_index, 1627);
    assert_eq!(selection.point_persistent_id, 379);
    assert_eq!(selection.identity_record_index, record_index + 3);
    assert_eq!(selection.next_record_index, record_index + 4);
    assert_eq!(selection.identity_record_offset, 229);
    assert_eq!(selection.sketch_record_index_offset, 254);
    assert_eq!(selection.point_persistent_id_offset, 262);
    assert_eq!(selection.next_byte_offset, 270);
}

/// A direct sketch-point selection of input 100 whose identity record carries
/// `identity` after its indexed header.
fn sketch_point_selection(identity: &[u8]) -> Vec<u8> {
    let record_index = 100;
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"290", record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "7b3dec6f-f69c-4bfa-a537-9274f341c66e");
    lp_utf16(&mut bytes, "2b40eee7-408c-429b-9216-8b6f7e9a62c9");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"260", record_index);
    indexed_header(&mut bytes, *b"278", record_index + 1);
    indexed_header(&mut bytes, *b"364", record_index + 2);
    indexed_header(&mut bytes, *b"368", record_index + 3);
    bytes.extend_from_slice(identity);
    indexed_header(&mut bytes, *b"348", record_index + 4);
    bytes
}

fn compact_identity(sketch: u64, point: u64) -> Vec<u8> {
    let mut identity = vec![0; 10];
    identity.extend_from_slice(&sketch.to_le_bytes());
    identity.extend_from_slice(&point.to_le_bytes());
    identity
}

#[test]
fn direct_sketch_point_selection_reads_the_compact_identity() {
    use crate::records::feature::work_geometry::DesignWorkPointSketchPointIdentityForm;
    let selection = parse_work_point_sketch_point_frame(
        &sketch_point_selection(&compact_identity(304, 110)),
        100,
        0,
    )
    .expect("compact sketch-point selection");
    assert_eq!(
        selection.identity_form,
        DesignWorkPointSketchPointIdentityForm::Compact
    );
    assert_eq!(selection.sketch_record_index, 304);
    assert_eq!(selection.point_persistent_id, 110);
    assert_eq!(selection.identity_record_offset, 229);
    assert_eq!(selection.sketch_record_index_offset, 250);
    assert_eq!(selection.point_persistent_id_offset, 258);
    assert_eq!(selection.next_byte_offset, 266);
}

#[test]
fn direct_sketch_point_selection_refuses_other_compact_spellings() {
    let mut nonzero_prefix = compact_identity(304, 110);
    nonzero_prefix[9] = 1;
    let mut short = compact_identity(304, 110);
    short.truncate(22);
    for identity in [
        nonzero_prefix,
        compact_identity(u64::from(u32::MAX) + 1, 110),
        short,
    ] {
        assert!(
            parse_work_point_sketch_point_frame(&sketch_point_selection(&identity), 100, 0)
                .is_none()
        );
    }
}
