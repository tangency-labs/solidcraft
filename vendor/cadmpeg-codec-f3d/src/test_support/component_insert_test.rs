// SPDX-License-Identifier: Apache-2.0
//! Design streams holding several complete envelopes of one external
//! `Component Insert` scope.
//!
//! Each envelope is the 395-byte class-`258` form of `docs/formats/f3d.md`:
//! an occurrence identity, a marked relation reference at offset 33 and a
//! rigid transform at offset 46, followed by the generic one-reference table,
//! the current history-state identity, the feature-family name and a 78-byte
//! fixed tail whose preceding history-state identity is at tail offset 31.
//! Its 57-byte relation names a carrier that stores an LP-UTF16 role GUID, two
//! zero bytes and the same transform.
#![allow(clippy::unwrap_used)]

use super::streams_test::design_metastream_with_records;
use crate::test_support::lp_utf16;

/// One envelope of the shared scope record.
#[derive(Clone, Copy)]
pub(crate) struct ComponentInsertEnvelope {
    pub(crate) translation_cm: [f64; 3],
    pub(crate) history_state_id: u32,
    pub(crate) previous_history_state_id: u32,
}

const SCOPE: u32 = 2_000;
const FIRST_CARRIER: u32 = 2_100;
const OCCURRENCE_IDENTITY: u64 = 0x0001_0203_0405_0607;
const ROLE: &str = "0f0e0d0c-0b0a-0908-0706-050403020100";
const SCOPE_FRAME: usize = 395;
const REFERENCE_COUNT_AT: usize = 262;
const SCOPE_TAIL: usize = 78;
const PREVIOUS_HISTORY_STATE_AT: usize = 31;
const RELATION_FRAME: usize = 57;

fn header(out: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(class_tag);
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn marked(out: &mut Vec<u8>, record_index: u32, zero_tail: usize) {
    out.push(1);
    out.extend_from_slice(&record_index.to_le_bytes());
    out.resize(out.len() + zero_tail, 0);
}

fn push_rigid_transform(out: &mut Vec<u8>, [x, y, z]: [f64; 3]) {
    let rows = [
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, y],
        [0.0, 0.0, 1.0, z],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in rows.into_iter().flatten() {
        out.extend_from_slice(&f64::to_le_bytes(value));
    }
}

/// A Design segment whose envelopes all carry one scope record index. Each
/// envelope follows its own carrier and relation, so the scope admitted for
/// the record determines the projected placement. Scopes, relations and
/// carriers are found by their indexed headers; the `MetaStream` registers no
/// types, so its primary index lists no records.
pub(crate) fn component_insert_design(envelopes: &[ComponentInsertEnvelope]) -> (Vec<u8>, Vec<u8>) {
    let mut bulk = Vec::new();
    for (envelope, carrier) in envelopes.iter().zip((FIRST_CARRIER..).step_by(10)) {
        let relation = carrier + 1;

        header(&mut bulk, b"301", carrier);
        bulk.extend_from_slice(&[0; 10]);
        lp_utf16(&mut bulk, ROLE);
        bulk.extend_from_slice(&[0, 0]);
        push_rigid_transform(&mut bulk, envelope.translation_cm);

        let relation_at = bulk.len();
        header(&mut bulk, b"302", relation);
        bulk.extend_from_slice(&[0; 10]);
        marked(&mut bulk, carrier, 8);
        marked(&mut bulk, carrier, 7);
        marked(&mut bulk, SCOPE, 6);
        assert_eq!(bulk.len() - relation_at, RELATION_FRAME);

        let scope_at = bulk.len();
        header(&mut bulk, b"300", SCOPE);
        bulk.extend_from_slice(&[0; 10]);
        bulk.extend_from_slice(&OCCURRENCE_IDENTITY.to_le_bytes());
        bulk.extend_from_slice(&[0; 4]);
        marked(&mut bulk, relation, 6);
        bulk.extend_from_slice(&[1, 0]);
        push_rigid_transform(&mut bulk, envelope.translation_cm);
        bulk.resize(scope_at + REFERENCE_COUNT_AT, 0);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        marked(&mut bulk, relation, 6);
        bulk.extend_from_slice(&envelope.history_state_id.to_le_bytes());
        lp_utf16(&mut bulk, "Component Insert");
        let tail_at = bulk.len();
        assert_eq!(tail_at - scope_at, SCOPE_FRAME - SCOPE_TAIL);
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        bulk.resize(tail_at + PREVIOUS_HISTORY_STATE_AT, 0);
        bulk.extend_from_slice(&envelope.previous_history_state_id.to_le_bytes());
        bulk.resize(scope_at + SCOPE_FRAME, 0);
        header(&mut bulk, b"258", SCOPE);
    }
    let meta = design_metastream_with_records(&[], &[]);
    (bulk, meta)
}

pub(crate) fn identity_insert_carrier(role: &str, record: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    header(&mut bytes, b"281", record);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    target_reference(&mut bytes);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut bytes, "66666666-7777-8888-9999-aaaaaaaaaaaa");
    bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0]);
    target_reference(&mut bytes);
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn target_reference(bytes: &mut Vec<u8>) {
    bytes.push(1);
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"E03784ED-5E19-4E14-B9F2-3B07017018CD");
    bytes.push(0);
}
