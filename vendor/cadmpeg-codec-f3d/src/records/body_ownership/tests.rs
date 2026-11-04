// SPDX-License-Identifier: Apache-2.0

use super::{membership, terminal_membership, COMPONENT_GUID};
use crate::design::presentation::BODY_PRESENTATION_TYPE_GUID;

fn reference(bytes: &mut Vec<u8>, target: u64, guid: &str) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(guid.as_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

fn tail(bodies: &[u64], owner: u64) -> Vec<u8> {
    let mut bytes = u32::try_from(bodies.len()).unwrap().to_le_bytes().to_vec();
    for body in bodies {
        reference(&mut bytes, *body, BODY_PRESENTATION_TYPE_GUID);
    }
    reference(&mut bytes, owner, COMPONENT_GUID);
    bytes
}

#[test]
fn terminal_membership_uses_explicit_owner_for_late_body_ids() {
    for bodies in [&[][..], &[900][..], &[900, 901][..]] {
        let bytes = tail(bodies, 40);
        let join = membership(&bytes).unwrap();
        assert_eq!(join.component, 40);
        assert_eq!(join.bodies().collect::<Vec<_>>(), bodies);
        let mut frame = vec![7; 23];
        frame.extend_from_slice(&bytes);
        assert_eq!(terminal_membership(&frame), Some(23));
    }
}

#[test]
fn membership_refuses_wrong_count_type_null_target_and_trailing_bytes() {
    let valid = tail(&[900], 40);
    for end in 0..valid.len() {
        assert!(membership(&valid[..end]).is_none());
    }
    for at in [0, 4, 17, 55, 68] {
        let mut bytes = valid.clone();
        bytes[at] ^= 0x7f;
        assert!(membership(&bytes).is_none(), "offset {at}");
    }
    let mut bytes = valid.clone();
    bytes.extend_from_slice(&[0]);
    assert!(membership(&bytes).is_none());
    let bytes = tail(&[900], 0);
    assert!(membership(&bytes).is_none());
}

#[test]
fn per_body_evidence_checks_its_count_ordinal_extent_and_reference() {
    let bytes = tail(&[900, 901], 40);
    let proof = membership(&bytes).unwrap().proof(60, 100, 1).unwrap();
    assert_eq!(proof.component_for(901), Some(40));
    assert_eq!(proof.component_for(900), None);
    assert_eq!(proof.body_count, 2);
    assert_eq!(proof.body_ordinal, 1);
    let mut bad_count = proof.clone();
    bad_count.body_count = 3;
    assert_eq!(bad_count.component_for(901), None);
    let mut bad_ordinal = proof.clone();
    bad_ordinal.body_ordinal = 2;
    assert_eq!(bad_ordinal.component_for(901), None);
    let mut bad_extent = proof;
    bad_extent.byte_end = 99;
    assert_eq!(bad_extent.component_for(901), None);
}

fn local_tail(bodies: &[u64], owner: u64) -> Vec<u8> {
    let reference = |bytes: &mut Vec<u8>, target: u64| {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    };
    let count = u32::try_from(bodies.len()).unwrap().to_le_bytes();
    let mut bytes = count.to_vec();
    for body in bodies {
        reference(&mut bytes, *body);
    }
    bytes.extend_from_slice(&count);
    for body in bodies {
        reference(&mut bytes, *body);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
    }
    reference(&mut bytes, owner);
    bytes
}

#[test]
fn paired_local_membership_checks_both_body_runs_and_the_fixed_words() {
    for bodies in [&[][..], &[900][..], &[900, 901][..]] {
        let bytes = local_tail(bodies, 40);
        let join = membership(&bytes).unwrap();
        assert_eq!(join.bodies().collect::<Vec<_>>(), bodies);
        assert_eq!(join.component, 40);
        assert_eq!(bytes.len(), 19 + bodies.len() * 26);
        let mut frame = vec![7; 23];
        frame.extend_from_slice(&bytes);
        assert_eq!(terminal_membership(&frame), Some(23));
        for (ordinal, body) in bodies.iter().enumerate() {
            let proof = join.proof(60, 100, ordinal).unwrap();
            assert_eq!(proof.component_for(*body), Some(40));
            let mut corrupt = proof.clone();
            if let super::BodyOwnershipReferences::Local { repeated_count, .. } =
                &mut corrupt.references
            {
                *repeated_count += 1;
            }
            assert_eq!(corrupt.component_for(*body), None);
            let mut corrupt = proof;
            if let super::BodyOwnershipReferences::Local { body_word, .. } = &mut corrupt.references
            {
                *body_word = 2;
            }
            assert_eq!(corrupt.component_for(*body), None);
        }
    }
    let valid = local_tail(&[900, 901], 40);
    for at in [0, 26, 31, 41, 46, 56, 60] {
        let mut corrupt = valid.clone();
        corrupt[at] ^= 0x7f;
        assert!(membership(&corrupt).is_none(), "offset {at}");
    }
    for end in 0..valid.len() {
        assert!(membership(&valid[..end]).is_none());
    }
}

#[test]
fn paired_lists_join_unique_bodies_independently_of_order() {
    let mut bytes = local_tail(&[900, 901, 902], 40);
    let repeated = 4 + 3 * 11 + 4;
    for i in 0..15 {
        bytes.swap(repeated + i, repeated + 30 + i);
    }
    let join = membership(&bytes).unwrap();
    assert_eq!(terminal_membership(&bytes), Some(0));
    for (ordinal, body) in [900, 901, 902].into_iter().enumerate() {
        assert_eq!(
            join.proof(60, 100, ordinal).unwrap().component_for(body),
            Some(40)
        );
    }
    for at in [5, repeated + 1] {
        let mut duplicate = bytes.clone();
        duplicate[at..at + 8].copy_from_slice(&901_u64.to_le_bytes());
        assert!(membership(&duplicate).is_none());
    }
    let mut different = bytes;
    different[repeated + 1..repeated + 9].copy_from_slice(&903_u64.to_le_bytes());
    assert!(membership(&different).is_none());
}

#[test]
fn single_local_list_checks_count_extent_targets_and_identity() {
    for bodies in [&[][..], &[900][..], &[900, 901, 902][..]] {
        let paired = local_tail(bodies, 40);
        let mut bytes = paired[..4 + bodies.len() * 11].to_vec();
        bytes.extend_from_slice(&paired[paired.len() - 11..]);
        let join = membership(&bytes).unwrap();
        assert_eq!(join.component, 40);
        assert_eq!(join.bodies().collect::<Vec<_>>(), bodies);
        let mut frame = vec![7; 23];
        frame.extend_from_slice(&bytes);
        assert_eq!(terminal_membership(&frame), Some(23));
        for (ordinal, body) in bodies.iter().enumerate() {
            let proof = join.proof(60, 100, ordinal).unwrap();
            assert_eq!(proof.component_for(*body), Some(40));
            let mut bad = proof;
            bad.byte_end += 1;
            assert_eq!(bad.component_for(*body), None);
        }
        let mut bad = bytes.clone();
        bad[0] += 1;
        assert!(membership(&bad).is_none());
        bad = bytes.clone();
        bad.push(0);
        assert!(membership(&bad).is_none());
        if bodies.len() > 1 {
            bad = bytes;
            bad[16..24].copy_from_slice(&900_u64.to_le_bytes());
            assert!(membership(&bad).is_none());
        }
    }
}
