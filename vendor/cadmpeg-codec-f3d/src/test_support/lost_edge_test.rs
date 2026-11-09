// SPDX-License-Identifier: Apache-2.0
//! A `RemoveBody` construction group and the unresolved edge-selection run
//! that ends at its identity wrapper.
//!
//! The run grammar is the "unresolved edge-selection record" paragraph of
//! `docs/formats/f3d.md`: each record is an indexed header, fourteen zero
//! bytes, `u32 19` and `EDGE_REFERENCE_LOST`, and the header that follows it
//! closes it. A run belongs to the counted group whose first identity-wrapper
//! header is the run's final following header, and it can be shorter than the
//! group's operand list.
#![allow(clippy::unwrap_used)]

use super::streams_test::generated_design_bulkstream;
use crate::test_support::{lp_ascii, lp_utf16, push_reference_u64};

const SCOPE: u32 = 1_400;
/// The construction group's record index, `N`.
pub(crate) const GROUP: u32 = 1_500;
const OPERANDS: [u32; 2] = [1_600, 1_610];
const FIRST_LOST_EDGE: u32 = 1_700;
/// Whole-body Boolean operand role of a `RemoveBody` group.
const BODIES_ROLE: u64 = 0x0000_0004_0000_0000;

/// Number of whole-body operands in the group.
pub(crate) const OPERAND_COUNT: usize = OPERANDS.len();

fn header(out: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(class_tag);
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn offset(out: &[u8]) -> u64 {
    u64::try_from(out.len()).unwrap()
}

/// The generated Design stream with a `RemoveBody` scope over one group of two
/// whole-body operands. The group's trailing reference names an identity
/// wrapper, and `run` consecutive unresolved edge records end at that
/// wrapper's header.
pub(super) fn remove_body_lost_edge_bulkstream(run: u32) -> (Vec<u8>, Vec<(u64, u64)>) {
    let (mut out, mut records) = generated_design_bulkstream();
    let wrapper = FIRST_LOST_EDGE + run;

    records.push((u64::from(SCOPE), offset(&out)));
    let scope_at = out.len();
    header(&mut out, b"268", SCOPE);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&3_u32.to_le_bytes());
    for reference in [GROUP, OPERANDS[0], OPERANDS[1]] {
        push_reference_u64(&mut out, u64::from(reference));
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut out, "RemoveBody");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&[0; 78]);
    assert_eq!(out.len() - scope_at, 168);
    header(&mut out, b"261", SCOPE);

    records.push((u64::from(GROUP), offset(&out)));
    header(&mut out, b"264", GROUP);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&2_u32.to_le_bytes());
    for operand in OPERANDS {
        push_reference_u64(&mut out, u64::from(operand));
    }
    // Both optional references are absent.
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut out, u64::from(wrapper));
    out.extend_from_slice(&BODIES_ROLE.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&1.0_f64.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut out, u64::from(GROUP + 2));
    out.extend_from_slice(&[0, 0]);
    push_reference_u64(&mut out, u64::from(GROUP + 1));
    out.push(0);
    push_reference_u64(&mut out, u64::from(SCOPE));
    header(&mut out, b"259", GROUP);

    // The generated MetaStream registers only the first operand's records, and
    // the primary index may list only registered records.
    let first_operand = push_body_operand(&mut out, OPERANDS[0]);
    records.extend(first_operand);
    push_body_operand(&mut out, OPERANDS[1]);

    for record_index in FIRST_LOST_EDGE..wrapper {
        header(&mut out, b"419", record_index);
        out.extend_from_slice(&[0; 14]);
        lp_ascii(&mut out, "EDGE_REFERENCE_LOST");
    }
    header(&mut out, b"326", wrapper);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&[1, 1, 0]);
    header(&mut out, b"327", wrapper + 1);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&[2, 0, 0, 0, 0, 0]);
    (out, records)
}

/// One whole-body recipe operand `N`, its paired header, and records `N+1`
/// through `N+4`, where `N+3` holds the `body_recipe_data` selector of `N+4`.
/// Returns the primary `(record index, offset)` entries.
fn push_body_operand(out: &mut Vec<u8>, operand: u32) -> Vec<(u64, u64)> {
    let mut records = vec![(u64::from(operand), offset(out))];
    header(out, b"268", operand);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&985_u64.to_le_bytes());
    out.extend_from_slice(&3_u32.to_le_bytes());
    push_reference_u64(out, u64::from(operand + 3));
    out.extend_from_slice(&1_u32.to_le_bytes());
    lp_utf16(out, "11111111-2222-3333-4444-555555555555");
    lp_utf16(out, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    out.extend_from_slice(&2_u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    header(out, b"269", operand);
    for (record_index, class_tag) in (operand + 1..).zip([b"270", b"271", b"272", b"273"]) {
        records.push((u64::from(record_index), offset(out)));
        header(out, class_tag, record_index);
        if record_index == operand + 3 {
            lp_ascii(out, &record_index.to_string());
            out.extend_from_slice(&(operand + 4).to_le_bytes());
            out.extend_from_slice(&[0; 12]);
            lp_ascii(out, "body_recipe_data");
        }
    }
    records
}

/// Record index of the identity wrapper that the group's trailing reference
/// names in [`remove_body_tracking_path_bulkstream`].
pub(crate) const TRACKING_IDENTITY_WRAPPER: u32 = 1_700;
/// Record index of the entity-tracking wrapper, eleven indices after the
/// identity wrapper.
pub(crate) const TRACKING_WRAPPER: u32 = 1_711;
/// Type GUID, base GUID and version of a type-table registration.
pub(crate) type TypeRegistration = (&'static str, &'static str, u32);

/// The tracking wrapper and the tracking carrier registrations.
const TRACKING_TYPES: [TypeRegistration; 2] = [
    (
        "335CC7BD-BBE5-40AA-9E8C-5EC99D427A3A",
        "5C069ACB-169F-4555-9792-815B4671DDA0",
        0,
    ),
    (
        "2DEE34BE-FA70-4D54-979C-164D86FDA786",
        "70F5E801-1E8E-436E-8A09-36487F8A2859",
        2,
    ),
];

/// The generated Design stream with a `RemoveBody` scope whose group's trailing
/// identity wrapper is followed by an entity-tracking path at an unrelated
/// record index. `type_count` is the number of type rows that precede the two
/// tracking registrations, which fixes their stream-local class tags. Returns
/// the stream, its primary index, and the tracking type rows.
pub(super) fn remove_body_tracking_path_bulkstream(
    type_count: usize,
) -> (Vec<u8>, Vec<(u64, u64)>, Vec<TypeRegistration>) {
    let tag = |ordinal: usize| -> [u8; 3] {
        let tag = (256 + type_count + ordinal).to_string();
        tag.as_bytes().try_into().unwrap()
    };
    let (mut out, mut records) = generated_design_bulkstream();

    records.push((u64::from(SCOPE), offset(&out)));
    header(&mut out, b"268", SCOPE);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&3_u32.to_le_bytes());
    for reference in [GROUP, OPERANDS[0], OPERANDS[1]] {
        push_reference_u64(&mut out, u64::from(reference));
    }
    out.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut out, "RemoveBody");
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&[0; 78]);
    header(&mut out, b"261", SCOPE);

    records.push((u64::from(GROUP), offset(&out)));
    header(&mut out, b"264", GROUP);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&2_u32.to_le_bytes());
    for operand in OPERANDS {
        push_reference_u64(&mut out, u64::from(operand));
    }
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut out, u64::from(TRACKING_IDENTITY_WRAPPER));
    out.extend_from_slice(&BODIES_ROLE.to_le_bytes());
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&1.0_f64.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    push_reference_u64(&mut out, u64::from(GROUP + 2));
    out.extend_from_slice(&[0, 0]);
    push_reference_u64(&mut out, u64::from(GROUP + 1));
    out.push(0);
    push_reference_u64(&mut out, u64::from(SCOPE));
    header(&mut out, b"259", GROUP);

    let first_operand = push_body_operand(&mut out, OPERANDS[0]);
    records.extend(first_operand);
    push_body_operand(&mut out, OPERANDS[1]);

    header(&mut out, b"326", TRACKING_IDENTITY_WRAPPER);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&[1, 1, 0]);
    records.push((u64::from(TRACKING_WRAPPER), offset(&out)));
    header(&mut out, &tag(0), TRACKING_WRAPPER);
    out.extend_from_slice(&[0; 10]);
    out.push(1);
    out.extend_from_slice(&u64::from(TRACKING_WRAPPER + 1).to_le_bytes());
    out.extend_from_slice(&[0; 3]);
    records.push((u64::from(TRACKING_WRAPPER + 1), offset(&out)));
    header(&mut out, &tag(1), TRACKING_WRAPPER + 1);
    out.extend_from_slice(&[0; 10]);
    for value in [1_u32, 0, 1, 2] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&268_u64.to_le_bytes());
    out.extend_from_slice(&0_u64.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&1_i32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&0_u64.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    header(&mut out, b"327", TRACKING_WRAPPER + 2);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(&[2, 0, 0, 0, 0, 0]);
    (out, records, TRACKING_TYPES.to_vec())
}
