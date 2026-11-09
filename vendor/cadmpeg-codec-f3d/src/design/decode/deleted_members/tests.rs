// SPDX-License-Identifier: Apache-2.0

use super::{deleted_members, owner_timestamp_table, table_evidence};
use crate::records::feature::deleted_members::DesignRecordTimestamp;
use crate::records::identity::Located;
use cadmpeg_core::decode::u64_from_index;

const SCOPE: u32 = 956;
const DELETED: u32 = 1032;
const STORED: u32 = 1036;
const TIMESTAMP: u64 = 0x0005_da1d_d4a2_1440;

fn header(out: &mut Vec<u8>, tag: [u8; 3], index: u32) {
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(&tag);
    out.extend_from_slice(&index.to_le_bytes());
}

fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

fn timestamp_entry(out: &mut Vec<u8>, index: u32) {
    reference(out, index);
    out.extend_from_slice(&TIMESTAMP.to_le_bytes());
}

/// A Sketch scope frame, its paired header, its record-timestamp table, and
/// the stored member record. Returns the bytes and the scope frame.
fn sketch_stream(table_owner: u32) -> (Vec<u8>, std::ops::Range<usize>) {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"397", SCOPE);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    reference(&mut bytes, DELETED);
    reference(&mut bytes, STORED);
    let paired = bytes.len();
    header(&mut bytes, *b"264", SCOPE);
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, *b"343", SCOPE + 1);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    timestamp_entry(&mut bytes, DELETED);
    timestamp_entry(&mut bytes, STORED);
    timestamp_entry(&mut bytes, table_owner);
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, *b"266", STORED);
    bytes.extend_from_slice(&[0; 8]);
    (bytes, 0..paired)
}

/// Offset of the table's indexed header in `sketch_stream`.
fn table_offset(frame: &std::ops::Range<usize>) -> usize {
    frame.end + 19
}

#[test]
fn timestamp_table_lists_entry_and_owner_index_offsets() {
    let (bytes, frame) = sketch_stream(SCOPE);
    let first = table_offset(&frame) + 31;
    let table = owner_timestamp_table(&bytes, SCOPE).expect("scope table");
    assert_eq!(table.entries, vec![first + 1, first + 20]);
    assert_eq!(table.owner_offset, first + 39);
}

#[test]
fn timestamp_table_of_another_owner_is_refused() {
    let (bytes, _) = sketch_stream(SCOPE + 2);
    assert!(owner_timestamp_table(&bytes, SCOPE).is_none());
}

#[test]
fn member_named_only_by_its_scope_and_table_is_deleted() {
    let (bytes, frame) = sketch_stream(SCOPE);
    let table = table_offset(&frame);
    let entry = |ordinal: usize, record_index: u32| DesignRecordTimestamp {
        record_index: Located {
            value: record_index,
            offset: u64_from_index(table + 31 + 19 * ordinal + 1),
        },
        timestamp: TIMESTAMP,
    };
    let evidence = deleted_members(&bytes, frame, SCOPE, &[DELETED]).expect("deleted member");
    assert_eq!(evidence.table_record_index, SCOPE + 1);
    assert_eq!(evidence.table_byte_offset, u64_from_index(table));
    assert_eq!(evidence.table_class_tag.as_str(), "343");
    assert_eq!(evidence.entries, vec![entry(0, DELETED), entry(1, STORED)]);
    assert_eq!(evidence.owner, entry(2, SCOPE));
    assert_eq!(evidence.members, vec![DELETED]);
    assert!(evidence.frame_holds(SCOPE));
}

#[test]
fn member_named_by_another_record_is_not_deleted() {
    let (mut bytes, frame) = sketch_stream(SCOPE);
    header(&mut bytes, *b"270", 1039);
    reference(&mut bytes, DELETED);
    assert!(deleted_members(&bytes, frame, SCOPE, &[DELETED]).is_none());
}

#[test]
fn member_with_an_indexed_header_is_not_deleted() {
    let (mut bytes, frame) = sketch_stream(SCOPE);
    header(&mut bytes, *b"361", DELETED);
    assert!(deleted_members(&bytes, frame, SCOPE, &[DELETED]).is_none());
}

#[test]
fn member_without_a_scope_table_is_not_deleted() {
    let (bytes, frame) = sketch_stream(SCOPE + 2);
    assert!(deleted_members(&bytes, frame, SCOPE, &[DELETED]).is_none());
}

#[test]
fn second_timestamp_table_of_one_owner_is_refused() {
    let (mut bytes, _) = sketch_stream(SCOPE);
    header(&mut bytes, *b"343", 2000);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    timestamp_entry(&mut bytes, SCOPE);
    bytes.extend_from_slice(&[0; 8]);
    assert!(owner_timestamp_table(&bytes, SCOPE).is_none());
}

#[test]
fn repeated_references_to_one_deleted_member_list_it_once() {
    let (bytes, frame) = sketch_stream(SCOPE);
    let evidence =
        deleted_members(&bytes, frame, SCOPE, &[DELETED, DELETED]).expect("deleted member");
    assert_eq!(evidence.members, vec![DELETED]);
    assert!(evidence.frame_holds(SCOPE));
}

#[test]
fn stored_table_bytes_state_the_table_evidence() {
    let (bytes, frame) = sketch_stream(SCOPE);
    let table = table_offset(&frame);
    let evidence = deleted_members(&bytes, frame, SCOPE, &[DELETED]).expect("deleted member");
    assert_eq!(
        &evidence.table_bytes[..],
        &bytes[table..table + 31 + 3 * 19]
    );
    let stored =
        table_evidence(&evidence.table_bytes, evidence.table_byte_offset).expect("stored table");
    assert_eq!(stored.record_index, evidence.table_record_index);
    assert_eq!(stored.class_tag, evidence.table_class_tag);
    assert_eq!(stored.entries, evidence.entries);
    assert_eq!(stored.owner, evidence.owner);
}

#[test]
fn table_bytes_with_trailing_or_missing_bytes_state_no_table() {
    let (bytes, frame) = sketch_stream(SCOPE);
    let table = table_offset(&frame);
    let end = table + 31 + 3 * 19;
    assert!(table_evidence(&bytes[table..=end], 0).is_none());
    assert!(table_evidence(&bytes[table..end - 1], 0).is_none());
    assert!(table_evidence(&bytes[table + 1..end], 0).is_none());
}
