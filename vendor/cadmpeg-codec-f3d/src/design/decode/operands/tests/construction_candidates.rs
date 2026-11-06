// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    parse_construction_operand_group, ConstructionOperandGroupParse, RecordFrame,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, push_marked_reference};

pub(super) fn complete_group_bytes(member: bool, auxiliary: bool, trailing: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"332", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&u32::from(member).to_le_bytes());
    if member {
        push_marked_reference(&mut bytes, 200);
    }
    if auxiliary {
        push_marked_reference(&mut bytes, 201);
    } else {
        bytes.push(0);
    }
    bytes.push(0);
    bytes.extend_from_slice(&u32::from(trailing).to_le_bytes());
    if trailing {
        push_marked_reference(&mut bytes, 202);
    }
    bytes.extend_from_slice(&0x0000_0008_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(&mut bytes, 102);
    bytes.extend_from_slice(&[1, 0]);
    push_marked_reference(&mut bytes, 101);
    bytes.push(0);
    push_marked_reference(&mut bytes, 12);
    indexed_header(&mut bytes, *b"259", 100);
    bytes
}

#[test]
fn incomplete_construction_candidates_are_not_complete_groups() {
    let scope = DesignParameterScope::empty(
        "f3d:test:construction-group-scope#12",
        DesignFeatureKind::Extrude,
        12,
    );
    let header = RecordFrame {
        record_index: 100,
        class_tag: DesignClassTag::try_from("332".to_owned()).expect("class tag"),
        byte_offset: 0,
    };
    let mut candidates = vec![complete_group_bytes(true, true, true)];
    let truncated_length = candidates[0].len() - 9;
    candidates[0].truncate(truncated_length);
    let mut false_count = Vec::new();
    indexed_header(&mut false_count, *b"332", 100);
    false_count.extend_from_slice(&[0; 10]);
    false_count.extend_from_slice(&1_000_000u32.to_le_bytes());
    false_count.resize(false_count.len() + 1_000_000, 0xe5);
    candidates.push(false_count);
    for bytes in candidates {
        assert!(matches!(
            parse_construction_operand_group(&bytes, &scope, 0, &header),
            ConstructionOperandGroupParse::NotAGroup | ConstructionOperandGroupParse::Unclosed
        ));
    }
}

#[test]
fn complete_construction_group_retains_each_reference_and_offset() {
    let bytes = complete_group_bytes(true, true, true);
    let scope = DesignParameterScope::empty("f3d:test:scope#12", DesignFeatureKind::Extrude, 12);
    let header = RecordFrame {
        record_index: 100,
        class_tag: DesignClassTag::try_from("332".to_owned()).expect("class tag"),
        byte_offset: 0,
    };
    let ConstructionOperandGroupParse::Complete(group) =
        parse_construction_operand_group(&bytes, &scope, 0, &header)
    else {
        panic!("complete group must decode");
    };
    assert_eq!(group.members()[0].value, 200);
    assert_eq!(group.frame.auxiliary_records[0].value, 201);
    assert_eq!(group.frame.trailing_records()[0].value, 202);
    assert!(group.members()[0].offset < group.frame.auxiliary_records[0].offset);
    assert!(group.frame.auxiliary_records[0].offset < group.frame.trailing_records()[0].offset);
}
