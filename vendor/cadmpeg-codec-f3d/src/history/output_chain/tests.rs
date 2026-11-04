// SPDX-License-Identifier: Apache-2.0

use super::OutputChain;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use std::collections::HashMap;

/// A chain whose states hold the given body slots, newest first.
fn history_with_bodies(bodies: [&[i64]; 3]) -> AsmHistory {
    let states = [(321, 2, Some(1)), (316, 1, Some(0)), (315, 0, None)]
        .into_iter()
        .zip(bodies)
        .map(|((state_id, node_index, next_ref), bodies)| {
            (
                AsmDeltaState {
                    id: format!("f3d:asm-delta-state#{state_id}"),
                    parent: "f3d:asm-history#1".into(),
                    byte_offset: 0,
                    state_id,
                    node_index,
                    next_ref,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(AsmHistoricalTopology {
                    bodies: bodies.to_vec(),
                    ..AsmHistoricalTopology::default()
                }),
                Vec::new(),
            )
        })
        .collect();
    crate::test_support::history::history("f3d:asm-history#1".into(), 0, None, false, states)
}

fn history() -> AsmHistory {
    history_with_bodies([&[]; 3])
}

#[test]
fn output_chain_admits_direct_and_complete_intervals() {
    let histories = [history()];
    let direct = OutputChain::resolve(&histories, 321, 316, None).unwrap();
    assert_eq!(direct.states, [321]);
    let chain = OutputChain::resolve(&histories, 321, 315, None).unwrap();
    assert_eq!(chain.history, 0);
    assert_eq!(chain.states, [321, 316]);
    let outputs = HashMap::from([((0, 321), Some(vec![2, 3])), ((0, 316), Some(vec![1, 2]))]);
    assert_eq!(chain.outputs(&outputs), Some(vec![1, 2, 3]));
    assert!(OutputChain::resolve(&histories, 315, 321, None).is_none());
}

#[test]
fn output_chain_omits_bodies_that_the_interval_inserts_and_deletes() {
    let outputs = HashMap::from([((0, 321), Some(vec![1, 2, 9])), ((0, 316), Some(vec![9]))]);
    let chain = OutputChain::resolve(
        &[history_with_bodies([&[1, 2], &[1, 9], &[1]])],
        321,
        315,
        None,
    )
    .unwrap();
    assert_eq!(chain.outputs(&outputs), Some(vec![1, 2]));

    // The preceding state holds slot 9: the interval deletes an input body.
    let chain = OutputChain::resolve(
        &[history_with_bodies([&[1, 2], &[1, 9], &[1, 9]])],
        321,
        315,
        None,
    )
    .unwrap();
    assert_eq!(chain.outputs(&outputs), Some(vec![1, 2, 9]));

    // The current state holds slot 9: the interval creates an output body.
    let chain = OutputChain::resolve(
        &[history_with_bodies([&[1, 2, 9], &[1, 9], &[1]])],
        321,
        315,
        None,
    )
    .unwrap();
    assert_eq!(chain.outputs(&outputs), Some(vec![1, 2, 9]));

    // A direct interval has no intermediate state.
    let chain = OutputChain::resolve(
        &[history_with_bodies([&[1, 2], &[1, 9], &[1]])],
        321,
        316,
        None,
    )
    .unwrap();
    assert_eq!(chain.outputs(&outputs), Some(vec![1, 2, 9]));
}

#[test]
fn output_chain_refuses_mutated_links_and_missing_transitions() {
    for mutation in 0..4 {
        let mut history = history();
        let states = history.states_mut();
        match mutation {
            0 => states[0].next_ref = Some(0),
            1 => states[0].transition.as_mut().unwrap().previous_state_id = Some(315),
            2 => states[1].transition = None,
            _ => states[1].next_ref = Some(2),
        }
        assert!(OutputChain::resolve(&[history], 321, 315, None).is_none());
    }
}

#[test]
fn output_chain_refuses_truncation_and_ambiguous_identities() {
    for length in 0..3 {
        let mut history = history();
        history.states_mut().truncate(length);
        assert!(OutputChain::resolve(&[history], 321, 315, None).is_none());
    }
    assert!(OutputChain::resolve(&[history(), history()], 321, 315, None).is_none());
    let mut duplicate = history();
    let state = duplicate.states()[1].clone();
    duplicate.states_mut().push(state);
    assert!(OutputChain::resolve(&[duplicate], 321, 315, None).is_none());
}

#[test]
fn output_chain_requires_every_state_body_projection() {
    let chain = OutputChain::resolve(&[history()], 321, 315, None).unwrap();
    let mut outputs = HashMap::from([((0, 321), Some(vec![1]))]);
    assert!(chain.outputs(&outputs).is_none());
    outputs.insert((0, 316), None);
    assert!(chain.outputs(&outputs).is_none());
    outputs.insert((0, 316), Some(Vec::new()));
    assert_eq!(chain.outputs(&outputs), Some(vec![1]));
    outputs.insert((1, 316), Some(vec![2]));
    assert_eq!(chain.outputs(&outputs), Some(vec![1]));
}

#[test]
fn output_chain_uses_the_explicit_history_namespace() {
    let first = history();
    let mut second = history();
    second.id = "f3d:asm-history#2".into();
    let parent = second.id.clone();
    for state in second.states_mut() {
        state.parent = parent.clone();
        state.id = format!("{parent}:state#{}", state.state_id);
    }
    let histories = [first, second];
    assert!(OutputChain::resolve(&histories, 321, 315, None).is_none());
    let chain = OutputChain::resolve(&histories, 321, 315, Some("f3d:asm-history#2")).unwrap();
    assert_eq!(chain.history, 1);
    assert_eq!(chain.states, [321, 316]);
    assert!(OutputChain::resolve(&histories, 321, 315, Some("missing")).is_none());
}

#[test]
fn output_chain_refuses_duplicate_bound_history_identities() {
    let first = history();
    let mut partial = history();
    partial.states_mut().truncate(1);
    assert!(OutputChain::resolve(&[first, partial], 321, 315, Some("f3d:asm-history#1")).is_none());
}
