// SPDX-License-Identifier: Apache-2.0
//! Typed-reference parameter scope tails and reference tables.

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::records::decal::DesignRecordHeader;
use crate::test_support::lp_utf16;

const TYPE: &str = "11111111-2222-3333-4444-555555555555";

fn typed_reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    out.extend_from_slice(&36u32.to_le_bytes());
    out.extend_from_slice(TYPE.as_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// A Sketch scope frame with a two-entry typed table and the 196-byte tail.
fn typed_scope_frame(previous_history: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"336");
    bytes.extend_from_slice(&226u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    typed_reference(&mut bytes, 222);
    typed_reference(&mut bytes, 234);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Sketch");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 15]);
    typed_reference(&mut bytes, 229);
    bytes.extend_from_slice(&previous_history.to_le_bytes());
    bytes.extend_from_slice(&64u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&64u32.to_le_bytes());
    typed_reference(&mut bytes, 228);
    bytes.extend_from_slice(&[0; 2]);
    typed_reference(&mut bytes, 227);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&226u32.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<crate::records::feature::scope::DesignParameterScope> {
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 226,
        class_tag: crate::records::references::DesignClassTag::try_from("336".to_owned()).unwrap(),
        byte_offset: 0,
    };
    parse_parameter_scope(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
}

#[test]
fn typed_scope_reads_the_wide_reference_table_and_196_byte_tail() {
    let scope = parse(&typed_scope_frame(5)).expect("typed scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Sketch
    );
    assert_eq!(
        scope.reference_members().values().collect::<Vec<_>>(),
        [&222, &234]
    );
    assert_eq!(scope.history_state_id(), Some(7));
    assert_eq!(scope.previous_history_state_id(), Some(5));
}

#[test]
fn typed_scope_tail_requires_its_typed_references() {
    let mut bytes = typed_scope_frame(5);
    let tail_reference = bytes.len() - 11 - 2 - 51;
    bytes[tail_reference + 9] = 35;
    assert!(parse(&bytes).is_none());
}

/// A typed-reference Extrude prologue: operation fields at offset 22, the
/// unit profile normal at offset 40, extent slots holding typed references,
/// and the one-sided distance extent pair.
#[test]
fn typed_extrude_prologue_reads_operation_at_22_and_wide_extent_slots() {
    use crate::records::feature::extrude::{
        DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
    };
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"320");
    bytes.extend_from_slice(&652u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    for value in [2u32, 1, 2] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 40);
    for value in [0.0f64, 1.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 0]);
    typed_reference(&mut bytes, 701);
    typed_reference(&mut bytes, 702);
    bytes.push(0);
    typed_reference(&mut bytes, 703);
    bytes.push(0);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let reference_count_at = bytes.len();
    let members = [701, 702, 703];
    let prologue = crate::design::decode::scopes::extrude::exact_typed_extrude_prologue(
        &bytes,
        0,
        reference_count_at,
        &members,
    )
    .expect("typed extrude prologue");
    let DesignExtrudePrologue::ReferenceAware {
        operation,
        operation_offset,
        extent,
        reference,
        ..
    } = prologue
    else {
        panic!("typed prologue is reference-aware");
    };
    assert_eq!(operation, DesignExtrudeOperation::Cut);
    assert_eq!(operation_offset, 22);
    assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
    assert!(reference.is_none());

    // A plain eleven-byte slot reference is not admitted in the typed form.
    let mut plain_slot = bytes.clone();
    plain_slot[66 + 9] = 0;
    assert!(
        crate::design::decode::scopes::extrude::exact_typed_extrude_prologue(
            &plain_slot,
            0,
            reference_count_at,
            &members
        )
        .is_none()
    );

    // The shifted typed form stores u32 1 at offset 20 and moves the
    // operation fields to offset 26.
    let mut shifted = bytes[..20].to_vec();
    shifted.extend_from_slice(&1u32.to_le_bytes());
    shifted.extend_from_slice(&[0, 0]);
    shifted.extend_from_slice(&bytes[22..]);
    let prologue = crate::design::decode::scopes::extrude::exact_typed_extrude_prologue(
        &shifted,
        0,
        reference_count_at + 4,
        &members,
    )
    .expect("shifted typed extrude prologue");
    let DesignExtrudePrologue::ReferenceAware {
        operation,
        operation_offset,
        extent,
        ..
    } = prologue
    else {
        panic!("shifted typed prologue is reference-aware");
    };
    assert_eq!(operation, DesignExtrudeOperation::Cut);
    assert_eq!(operation_offset, 26);
    assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
    shifted[20] = 2;
    assert!(
        crate::design::decode::scopes::extrude::exact_typed_extrude_prologue(
            &shifted,
            0,
            reference_count_at + 4,
            &members
        )
        .is_none()
    );
}

/// A typed-reference zero-body Base Feature: the class-409 zero-body prefix
/// with a typed shared-metadata reference at offset 32, the null GUID, the
/// one-entry typed reference table, and the 196-byte typed tail.
fn typed_zero_body_base_feature_frame() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"364");
    bytes.extend_from_slice(&226u32.to_le_bytes());
    bytes.resize(20, 0);
    bytes.push(1);
    bytes.resize(32, 0);
    typed_reference(&mut bytes, 342);
    bytes.resize(95, 0);
    lp_utf16(&mut bytes, "00000000-0000-0000-0000-000000000000");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    typed_reference(&mut bytes, 342);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Base Feature");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 15]);
    typed_reference(&mut bytes, 341);
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(&64u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&64u32.to_le_bytes());
    typed_reference(&mut bytes, 340);
    bytes.extend_from_slice(&[0; 2]);
    typed_reference(&mut bytes, 339);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&226u32.to_le_bytes());
    bytes
}

#[test]
fn typed_zero_body_base_feature_reads_its_typed_metadata_reference() {
    use crate::records::feature::base_feature::DesignBaseFeatureConstruction;

    let bytes = typed_zero_body_base_feature_frame();
    let scope = parse(&bytes).expect("typed Base Feature scope");
    assert_eq!(scope.kind_offset(), 237);
    let construction = super::super::base_feature::exact_base_feature_construction(&bytes, &scope)
        .unwrap()
        .expect("typed zero-body Base Feature");
    let DesignBaseFeatureConstruction::ResultBodies {
        bodies,
        metadata_record,
        ..
    } = construction
    else {
        panic!("zero-body result form");
    };
    assert_eq!(metadata_record, 342);
    assert_eq!(bodies.iter().count(), 0);

    // A nonzero byte before the null GUID is not the zero-body form.
    let mut padded = bytes.clone();
    padded[90] = 1;
    let scope = parse(&padded).expect("typed Base Feature scope");
    assert!(
        super::super::base_feature::exact_base_feature_construction(&padded, &scope)
            .unwrap()
            .is_none()
    );
}
