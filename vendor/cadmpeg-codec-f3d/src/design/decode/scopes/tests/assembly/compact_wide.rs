// SPDX-License-Identifier: Apache-2.0
//! Compact wide (806-, 850- and 861-byte) `Assemble` scopes.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::references::DesignClassTag;

pub(super) fn push_lp_utf16(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(
        &u32::try_from(value.encode_utf16().count())
            .unwrap()
            .to_le_bytes(),
    );
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

pub(super) fn push_marked(bytes: &mut Vec<u8>, target: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

pub(super) fn push_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

/// A wide locator `index` with its typed path `index + 1` and wrapper
/// `index + 2`, owned by scope `scope_record_index`.
pub(super) fn push_wide_operand_path(
    bytes: &mut Vec<u8>,
    index: u32,
    scope_record_index: u32,
    guid: &str,
) {
    let start = bytes.len();
    push_header(bytes, b"283", index);
    bytes.resize(start + 39, 0);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.resize(start + 56, 0);
    push_marked(bytes, 900);
    bytes.push(1);
    bytes.push(0);
    push_marked(bytes, scope_record_index);
    push_marked(bytes, index + 2);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    push_header(bytes, b"366", index + 1);
    bytes.extend_from_slice(&u64::from(index + 1).to_le_bytes()[4..]);
    bytes.truncate(bytes.len() - 4);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_lp_utf16(bytes, guid);
    push_lp_utf16(bytes, "22222222-2222-2222-2222-222222222222");
    push_lp_utf16(bytes, "33333333-3333-3333-3333-333333333333");
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    push_lp_utf16(bytes, "44444444-4444-4444-4444-444444444444");
    push_lp_utf16(bytes, "55555555-5555-5555-5555-555555555555");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    let wrapper = bytes.len();
    push_header(bytes, b"327", index + 2);
    bytes.resize(wrapper + 21, 0);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    push_marked(bytes, index + 1);
}

/// An 850-byte compact wide scope (record 10) with its owners first, its two
/// operand frames and two wide operand paths after its paired header.
fn compact_wide_scope_fixture(
    owners_first: bool,
) -> (Vec<u8>, DesignParameterScope, [DesignParameterOwner; 6]) {
    let scope_record_index = 10_u32;
    let mut bytes = vec![0_u8; 850];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"285");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    for (reference_at, transform_at, target, z) in [(32, 50, 17_u32, 0.0), (254, 272, 45, 1.7)] {
        bytes[reference_at] = 1;
        bytes[reference_at + 1..reference_at + 5].copy_from_slice(&target.to_le_bytes());
        let rows: [[f64; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [0.0, 1.0, 0.0, z],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (ordinal, value) in rows.iter().flatten().enumerate() {
            let at = transform_at + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    // The 850-byte scope: its two limit owners after the count at 526, then
    // the locator count at 552 and the locators at 556 and 567.
    bytes[526..530].copy_from_slice(&2_u32.to_le_bytes());
    bytes[552..556].copy_from_slice(&2_u32.to_le_bytes());
    for (at, target) in [(530, 54_u32), (541, 55), (556, 64), (567, 67)] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
    }
    push_header(&mut bytes, b"262", scope_record_index);
    push_wide_operand_path(
        &mut bytes,
        64,
        scope_record_index,
        "11111111-1111-1111-1111-111111111111",
    );
    push_wide_operand_path(
        &mut bytes,
        67,
        scope_record_index,
        "66666666-6666-6666-6666-666666666666",
    );
    push_header(&mut bytes, b"300", 70);
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        scope_record_index,
    );
    scope.class_tag = DesignClassTag::try_from("285".to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("262".to_owned()).unwrap();
    let members = if owners_first {
        vec![50, 51, 52, 53, 54, 55, 64, 67]
    } else {
        vec![64, 67, 50, 51, 52, 53, 54, 55]
    };
    scope
        .try_edit(|draft| {
            draft.frame_length = 850;
            draft.reference_members = ReferenceRun::unlocated(members);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let owner = |record_index: u32, local_ordinal: u32, evaluated_value: f64| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: 2000 + u64::from(record_index),
            frame_length: 104,
            class_tag: DesignClassTag::try_from("312".to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset: 2040 + u64::from(record_index),
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: Some(0),
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    let owners = [
        owner(50, 0, -1.0),
        owner(51, 1, 0.0),
        owner(52, 2, 0.0),
        owner(53, 3, 0.5),
        owner(54, 4, -1.7),
        owner(55, 5, 1.7),
    ];
    (bytes, scope, owners)
}

#[test]
fn compact_wide_assemble_scope_reads_owners_frames_and_wide_operand_paths() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    let locator = "2E3AC990-483D-4A55-8662-C064B976A957";
    let types = std::collections::HashMap::from([
        (64, (locator, 5)),
        (65, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
        (67, (locator, 5)),
        (68, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
    ]);
    let read = |bytes: &[u8], scope: &DesignParameterScope, owners: &[DesignParameterOwner]| {
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(bytes);
        exact_assembly_alignment_with_types(bytes, &records, scope, owners, &types).unwrap()
    };

    let (bytes, scope, owners) = compact_wide_scope_fixture(true);
    let alignment = read(&bytes, &scope, &owners).expect("compact wide alignment");
    assert_eq!(alignment.angle(), -1.0);
    assert_eq!(alignment.offset(), [0.0, 0.0, 0.5]);
    let frames = alignment.operand_frames().expect("frames");
    assert_eq!(frames[0].transform_offset, 50);
    assert_eq!(frames[1].transform.rows()[2][3], 1.7);
    let paths = alignment.operand_path_refs().expect("both operand paths");
    assert_eq!(
        paths[0].occurrence_guids()[0].value.as_str(),
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(paths[1].link().locator_record_index, 67);

    // The owners are the first four reference members, not the last.
    let (bytes, scope, owners) = compact_wide_scope_fixture(false);
    assert!(read(&bytes, &scope, &owners).is_none());

    // The limit-owner run names the fifth and sixth owners.
    let (mut bytes, scope, owners) = compact_wide_scope_fixture(true);
    bytes[531] = 53;
    assert!(read(&bytes, &scope, &owners)
        .expect("alignment without frames")
        .operand_frames()
        .is_none());

    // A malformed locator leaves the frames without qualifiers.
    let (mut bytes, scope, owners) = compact_wide_scope_fixture(true);
    bytes[850 + 11 + 39] = 4;
    let alignment = read(&bytes, &scope, &owners).expect("frames still read");
    assert!(alignment.operand_frames().is_some());
    assert!(alignment.operand_path_refs().is_none());
}

#[test]
fn compact_wide_operand_flag_is_the_frame_sense() {
    use crate::design::decode::scopes::assembly_alignment::exact_assembly_alignment_with_types;
    use crate::records::feature::assembly::DesignJointFrameSense;
    let types = std::collections::HashMap::new();
    let alignment = |flag: u8| {
        let (mut bytes, scope, owners) = compact_wide_scope_fixture(true);
        bytes[21] = flag;
        let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
        exact_assembly_alignment_with_types(&bytes, &records, &scope, &owners, &types).unwrap()
    };
    // 0: the operand frames mate with their z axes aligned; 1: opposed.
    for (flag, sense) in [
        (0, DesignJointFrameSense::Aligned),
        (1, DesignJointFrameSense::Opposed),
    ] {
        let alignment = alignment(flag).expect("alignment");
        assert!(alignment.operand_frames().is_some());
        assert_eq!(alignment.frame_sense, Some(sense));
        let wire = serde_json::to_value(&alignment).unwrap();
        let back: crate::records::feature::assembly::DesignAssemblyAlignment =
            serde_json::from_value(wire).unwrap();
        assert_eq!(back.frame_sense, Some(sense));
    }
    assert!(alignment(2)
        .and_then(|alignment| alignment.operand_frames())
        .is_none());
}
