// SPDX-License-Identifier: Apache-2.0
//! History binding bound unit tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{u64_from_index, WorkBudget};

use crate::history::binding::MAX_HISTORY_BINDING_WORK;
use crate::history::{
    admit_history_binding, HistoryBindingAdmission, HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT,
};
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistoryRecord,
    AsmHistoryRecordFraming,
};

/// One state of an insert-only chain whose board inserts `inserted`.
fn insert_only_state(node_index: i64, next_ref: Option<i64>, inserted: &[i64]) -> AsmDeltaState {
    let state_id = format!("state-{node_index}");
    let board_id = format!("board-{node_index}");
    let offset = u64::try_from(node_index).unwrap();
    AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: offset,
        state_id: 10 - node_index,
        version_flag: 1,
        state_flag: 0,
        previous_ref: (node_index > 0).then_some(node_index - 1),
        next_ref,
        node_index,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: offset,
            owner_ref: 0,
            number: 2,
            changes: inserted
                .iter()
                .enumerate()
                .map(|(index, new_ref)| AsmEntityChange {
                    id: format!("change-{node_index}-{index}"),
                    parent: board_id.clone(),
                    byte_offset: u64_from_index(index),
                    kind: AsmEntityChangeKind::Insert { new: *new_ref },
                })
                .collect(),
        }],
        records: vec![AsmHistoryRecord {
            id: format!("record-{node_index}"),
            parent: state_id,
            revision_id: None,
            byte_offset: offset,
            framing: AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11].into(),
        }],
        transition: None,
    }
}

fn insert_only_chain() -> Vec<AsmDeltaState> {
    vec![
        insert_only_state(0, Some(1), &[1]),
        insert_only_state(1, Some(2), &[2]),
        insert_only_state(2, None, &[3]),
    ]
}

#[test]
fn history_binding_skips_a_chain_past_its_retained_byte_allowance() {
    let unbounded = insert_only_chain();
    assert_eq!(
        admit_history_binding(
            &cadmpeg_test_support::service_decode_context(),
            &unbounded,
            &mut { u64::MAX }
        )
        .unwrap(),
        HistoryBindingAdmission::Admitted
    );
    // Three bulletin-board changes and two slots in the tail state.
    let bytes = 5 * HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT;

    let mut allowance = bytes;
    let at_bound = insert_only_chain();
    assert_eq!(
        admit_history_binding(
            &cadmpeg_test_support::service_decode_context(),
            &at_bound,
            &mut allowance
        )
        .unwrap(),
        HistoryBindingAdmission::Admitted
    );
    assert_eq!(at_bound, unbounded);
    assert_eq!(allowance, 0);

    let mut allowance = bytes - 1;
    let past_bound = insert_only_chain();
    assert_eq!(
        admit_history_binding(
            &cadmpeg_test_support::service_decode_context(),
            &past_bound,
            &mut allowance
        )
        .unwrap(),
        HistoryBindingAdmission::Skipped
    );
    assert!(past_bound.iter().all(|state| state.transition.is_none()));
    assert_eq!(allowance, bytes - 1);
}

#[test]
fn history_streams_of_one_decode_share_the_remaining_retained_bytes() {
    let first = insert_only_chain();
    let second = insert_only_chain();
    let bytes = 5 * HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT;
    let mut allowance = 2 * bytes - 1;

    assert_eq!(
        admit_history_binding(
            &cadmpeg_test_support::service_decode_context(),
            &first,
            &mut allowance
        )
        .unwrap(),
        HistoryBindingAdmission::Admitted
    );
    assert_eq!(allowance, bytes - 1);
    assert_eq!(
        admit_history_binding(
            &cadmpeg_test_support::service_decode_context(),
            &second,
            &mut allowance
        )
        .unwrap(),
        HistoryBindingAdmission::Skipped
    );
    assert_eq!(allowance, bytes - 1);
    assert!(second.iter().all(|state| state.transition.is_none()));
}

fn geometry_history(split: bool) -> Vec<u8> {
    use crate::test_support::tokens_test::{t_end, t_ident, t_long, t_ref};
    let mut bytes = crate::test_support::smbh_geometry_test::synthetic_geometry_smbh();
    // The fixture ends with the first delta_state name, after 19 active records.
    for node in 0..=i64::from(split) {
        if node != 0 {
            t_ident(&mut bytes, "delta_state");
        }
        for value in [2 - node, 1, 0] {
            t_long(&mut bytes, value);
        }
        for reference in [
            if node == 0 { -1 } else { 0 },
            if split && node == 0 { 1 } else { -1 },
            node,
            -1,
            0,
        ] {
            t_ref(&mut bytes, reference);
        }
        bytes.push(0x0b);
        if node == 0 {
            t_long(&mut bytes, 1);
            t_ref(&mut bytes, 0);
            t_long(&mut bytes, 2);
            for slot in 1..19 {
                t_long(&mut bytes, 1);
                t_ref(&mut bytes, -1);
                t_ref(&mut bytes, slot);
            }
            t_long(&mut bytes, 0);
        }
        t_long(&mut bytes, 0);
        t_end(&mut bytes);
        t_ident(&mut bytes, "End-of-ASM-History-Section");
        t_end(&mut bytes);
    }
    bytes
}

#[test]
fn skipped_history_does_not_charge_retained_bytes() {
    use cadmpeg_asm::kernel_header::RefWidth;
    use cadmpeg_core::decode::DecodePolicy;
    let mut policy = DecodePolicy::desktop();
    // One state retains 19 tail slots plus 18 changes.
    policy.limits.max_retained_bytes = 37 * HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        // Use the session remainder, including native text already retained.
        ctx.retain_bytes(1, "earlier native text").unwrap();
        let mut remaining = ctx.retained_bytes_available();
        let available = remaining;
        let large = super::super::decode(
            ctx,
            &geometry_history(false),
            "large",
            RefWidth::Eight,
            &mut remaining,
            &WorkBudget::new(MAX_HISTORY_BINDING_WORK),
        )
        .unwrap()
        .unwrap();
        assert!(large.binding_budget_exceeded);
        assert!(large.states().iter().all(|state| state.transition.is_none()
            && state.bulletin_boards.is_empty()
            && state.records.is_empty()));
        assert_eq!(remaining, available);
        assert_eq!(ctx.retained_bytes_available(), available);
    });
}

#[test]
fn skipped_history_leaves_room_for_the_next_smaller_stream() {
    use cadmpeg_asm::kernel_header::RefWidth;
    use cadmpeg_core::decode::DecodePolicy;
    let mut policy = DecodePolicy::desktop();
    // Moving the 18 changes to a newer state leaves one slot in the tail.
    policy.limits.max_retained_bytes = 19 * HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT + 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        ctx.retain_bytes(1, "earlier native text").unwrap();
        let mut remaining = ctx.retained_bytes_available();
        let large = super::super::decode(
            ctx,
            &geometry_history(false),
            "large",
            RefWidth::Eight,
            &mut remaining,
            &WorkBudget::new(MAX_HISTORY_BINDING_WORK),
        )
        .unwrap()
        .unwrap();
        assert!(large.binding_budget_exceeded);
        let small = super::super::decode(
            ctx,
            &geometry_history(true),
            "small",
            RefWidth::Eight,
            &mut remaining,
            &WorkBudget::new(MAX_HISTORY_BINDING_WORK),
        )
        .unwrap()
        .unwrap();
        assert!(!small.binding_budget_exceeded);
        assert!(small
            .states()
            .iter()
            .all(|state| state.transition.is_some()));
        assert_eq!(remaining, 0);
        assert_eq!(
            ctx.retained_bytes_available(),
            policy.limits.max_retained_bytes - 1
        );
    });
}

#[test]
fn history_past_its_binding_work_keeps_its_records_without_topology() {
    use cadmpeg_asm::kernel_header::RefWidth;
    let bytes = geometry_history(true);
    let decode = |work: &WorkBudget| {
        crate::test_support::with_decode_context(|ctx| {
            super::super::decode(
                ctx,
                &bytes,
                "history",
                RefWidth::Eight,
                &mut { u64::MAX },
                work,
            )
            .unwrap()
            .unwrap()
        })
    };
    let unbounded = WorkBudget::new(MAX_HISTORY_BINDING_WORK);
    let bound = decode(&unbounded);
    assert!(bound
        .states()
        .iter()
        .all(|state| state.transition.is_some()));

    let at_cap = WorkBudget::new(unbounded.consumed());
    assert_eq!(decode(&at_cap), bound);
    assert!(!at_cap.exhausted());

    let past_cap = WorkBudget::new(unbounded.consumed() - 1);
    let capped = decode(&past_cap);
    assert!(past_cap.exhausted());
    assert!(!capped.binding_budget_exceeded);
    assert_eq!(capped.states().len(), bound.states().len());
    for (capped, bound) in capped.states().iter().zip(bound.states()) {
        assert!(!capped.records.is_empty());
        assert_eq!(
            *capped,
            crate::history_records::AsmDeltaState {
                transition: None,
                ..bound.clone()
            }
        );
    }
}
