// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::test_support::lp_utf16;

const SCOPE_RECORD: u32 = 2180;

/// A `RemoveBody` scope whose named tail carries a lane block over record
/// indices above 255 and a class word other than 252.
fn remove_body_scope(lane_high_byte: u64, class_word: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&SCOPE_RECORD.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for target in [SCOPE_RECORD + 1, SCOPE_RECORD + 4] {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut bytes, "RemoveBody");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, "RemoveBody-Top Half (1)");
    bytes.extend_from_slice(&[0; 7]);

    let lane_ids = [0x8eu8, 0x8d, 0x8c];
    bytes.push(1);
    bytes.push(lane_ids[0]);
    bytes.extend_from_slice(&lane_high_byte.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&class_word.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&class_word.to_le_bytes());
    bytes.push(1);
    bytes.push(lane_ids[1]);
    bytes.extend_from_slice(&lane_high_byte.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(lane_ids[2]);
    bytes.extend_from_slice(&lane_high_byte.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"260");
    bytes.extend_from_slice(&SCOPE_RECORD.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<crate::records::feature::scope::DesignParameterScope> {
    let class_tag = crate::records::references::DesignClassTag::try_from("396".to_owned()).unwrap();
    parse_parameter_scope(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        SCOPE_RECORD,
        &class_tag,
        0,
    )
    .unwrap()
}

#[test]
fn remove_body_scope_admits_a_lane_block_over_high_record_indices() {
    let scope = parse(&remove_body_scope(8, 0x101)).expect("named RemoveBody scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::RemoveBody
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [SCOPE_RECORD + 1, SCOPE_RECORD + 4]
    );
    assert_eq!(scope.history_state_id(), None);
}

#[test]
fn remove_body_scope_refuses_lane_values_and_class_words_outside_the_layout() {
    assert!(parse(&remove_body_scope(0x100, 0x101)).is_none());
    assert!(parse(&remove_body_scope(8, 1000)).is_none());
    assert!(parse(&remove_body_scope(8, 0)).is_none());
}

#[test]
fn remove_body_scope_refuses_a_truncated_lane_block() {
    let bytes = remove_body_scope(8, 0x101);
    let paired = bytes.len() - 11;
    let mut truncated = bytes[..paired - 1].to_vec();
    truncated.extend_from_slice(&bytes[paired..]);
    assert!(parse(&truncated).is_none());
}
