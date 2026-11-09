// SPDX-License-Identifier: Apache-2.0

use super::parse_construction_operand_group;
use crate::design::decode::operands::RecordFrame;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::extrude_selection::DesignOperandRole;

const GROUP: u32 = 355;
const SCOPE: u32 = 354;

fn reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

fn body_group(flags: &[u8]) -> Vec<u8> {
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
    bytes.extend_from_slice(flags);
    reference(&mut bytes, GROUP + 1);
    bytes.push(0);
    reference(&mut bytes, SCOPE);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&GROUP.to_le_bytes());
    bytes
}

fn parse(
    flags: &[u8],
) -> Option<crate::records::topology::construction::DesignConstructionOperandGroup> {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#354",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
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
    for (flags, closing_flag) in [
        (&[1, 0][..], false),
        (&[1, 1][..], true),
        (&[1, 1, 0][..], false),
        (&[1, 1, 1][..], true),
    ] {
        let group = parse(flags).expect("the group closes");
        assert_eq!(group.role(), DesignOperandRole::BODIES_B);
        assert!(group.frame.variant);
        assert_eq!(group.frame.closing_flag, closing_flag);
        let losses = crate::design::decode::operands::construction_closing_flag_losses(
            std::slice::from_ref(&group),
        );
        if group.frame.closing_flag {
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                crate::loss::F3dLossCode::ConstructionClosingFlagAssumed.kind()
            );
            assert!(losses[0]
                .message
                .contains("assumes the flag does not change operation semantics"));
        } else {
            assert!(losses.is_empty());
        }
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
fn move_group_tail_closes_with_false_variant_and_set_closing_flag() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"328");
    bytes.extend_from_slice(&GROUP.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, GROUP + 3);
    bytes.push(0);
    reference(&mut bytes, GROUP + 13);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&DesignOperandRole::BODIES_A.raw().to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&155u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&155u32.to_le_bytes());
    reference(&mut bytes, GROUP + 2);
    bytes.extend_from_slice(&[0, 1]);
    reference(&mut bytes, GROUP + 1);
    bytes.push(0);
    reference(&mut bytes, SCOPE);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"263");
    bytes.extend_from_slice(&GROUP.to_le_bytes());

    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#354",
        crate::records::feature::scope::DesignFeatureKind::Move,
        SCOPE,
    );
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:group#355".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("328".to_owned()).unwrap(),
        record_index: GROUP,
    };
    let group = parse_construction_operand_group(&bytes, &scope, 0, &RecordFrame::from(&header))
        .complete()
        .expect("the Move group closes");
    assert!(!group.frame.variant);
    assert!(group.frame.closing_flag);
    assert_eq!(group.role(), DesignOperandRole::BODIES_A);
    assert_eq!(group.members().len(), 1);
    assert_eq!(group.members()[0].value, GROUP + 3);
    assert_eq!(group.paired_byte_offset, u64::try_from(paired_at).unwrap());
}

#[test]
fn group_tail_refuses_a_closing_byte_other_than_zero_or_one() {
    assert!(parse(&[1, 2]).is_none());
    assert!(parse(&[1, 1, 2]).is_none());
}

#[test]
fn group_tail_refuses_a_variant_other_than_zero_or_one() {
    assert!(parse(&[2, 0]).is_none());
    assert!(parse(&[1, 2, 0]).is_none());
}
