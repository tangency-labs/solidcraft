// SPDX-License-Identifier: Apache-2.0

use super::parse_construction_operand_group;
use crate::design::decode::operands::RecordFrame;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::extrude_selection::DesignOperandRole;

const GROUP: u32 = 355;
const SCOPE: u32 = 354;

fn reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

/// An existing-body Extrude group with one member, one trailing reference,
/// and the three-byte tail flag block `flags`.
fn body_group(flags: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"325");
    bytes.extend_from_slice(&GROUP.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, GROUP + 3);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 575);
    bytes.extend_from_slice(&DesignOperandRole::BODIES_B.raw().to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&155u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&155u32.to_le_bytes());
    reference(&mut bytes, GROUP + 2);
    bytes.extend_from_slice(&flags);
    reference(&mut bytes, GROUP + 1);
    bytes.push(0);
    reference(&mut bytes, SCOPE);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&GROUP.to_le_bytes());
    bytes
}

fn parse(
    flags: [u8; 3],
) -> Option<crate::records::topology::construction::DesignConstructionOperandGroup> {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#354",
        DesignFeatureKind::Extrusion,
        SCOPE,
    );
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:group#355".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("325".to_owned()).unwrap(),
        record_index: GROUP,
    };
    parse_construction_operand_group(&body_group(flags), &scope, 0, &RecordFrame::from(&header))
        .complete()
}

#[test]
fn group_tail_keeps_its_closing_flag() {
    for closing in [0, 1] {
        let group = parse([1, 1, closing]).expect("the group closes");
        assert_eq!(group.role(), DesignOperandRole::BODIES_B);
        assert!(group.frame.variant);
        assert_eq!(group.frame.closing_flag, closing == 1);
        assert_eq!(
            group
                .members()
                .iter()
                .map(|member| member.value)
                .collect::<Vec<_>>(),
            [GROUP + 3]
        );
    }
}

#[test]
fn group_tail_refuses_a_closing_byte_other_than_zero_or_one() {
    assert!(parse([1, 1, 2]).is_none());
}
