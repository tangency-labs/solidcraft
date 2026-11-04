// SPDX-License-Identifier: Apache-2.0
//! Bound construction-history fixtures.

use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory,
    AsmPreamble, AsmStateSnapshot,
};
use std::collections::BTreeMap;

pub(crate) type State = (
    AsmDeltaState,
    Option<AsmHistoricalTopology>,
    Vec<AsmEntityVersion>,
);

/// Compute transitions from fixture topologies and full record-version maps.
pub(crate) fn history(
    id: String,
    byte_offset: u64,
    preamble: Option<AsmPreamble>,
    budget_exceeded: bool,
    mut states: Vec<State>,
) -> AsmHistory {
    // A fixture without link metadata names its states from newest to oldest.
    if states.len() > 1 && states.iter().all(|s| s.0.next_ref.is_none()) {
        for index in 0..states.len() - 1 {
            states[index].0.next_ref = Some(states[index + 1].0.node_index);
        }
    }
    for index in 0..states.len() {
        if let Some(next) = states[index].0.next_ref {
            let node = states[index].0.node_index;
            if let Some(older) = states.iter_mut().find(|s| s.0.node_index == next) {
                older.0.previous_ref = Some(node);
            }
        }
    }
    let versions: Vec<BTreeMap<_, _>> = states
        .iter()
        .map(|(_, _, versions)| {
            versions
                .iter()
                .map(|row| (row.entity_ref, row.record_ref))
                .collect()
        })
        .collect();
    let bound = states.iter().all(|s| s.1.is_some());
    let transitions: Vec<_> = states
        .iter()
        .enumerate()
        .map(|(index, (state, _, _))| {
            if !bound {
                return None;
            }
            let snapshot = |index: usize| AsmStateSnapshot {
                state_id: states[index].0.state_id,
                topology: states[index].1.as_ref().unwrap(),
                versions: &versions[index],
            };
            let older = state.next_ref.map(|node| {
                let index = states.iter().position(|s| s.0.node_index == node).unwrap();
                snapshot(index)
            });
            Some(AsmHistoricalTransition::between(older, snapshot(index)).unwrap())
        })
        .collect();
    let states = states
        .into_iter()
        .zip(transitions)
        .map(|((mut state, _, _), transition)| {
            state.transition = transition;
            state
        })
        .collect();
    AsmHistory::new(id, byte_offset, preamble, budget_exceeded, states)
}

/// Recompute a fixture chain after changing one state's decode inputs.
pub(crate) fn edit(
    target: &mut AsmHistory,
    ordinal: usize,
    change: impl FnOnce(&mut AsmHistoricalTopology, &mut Vec<AsmEntityVersion>),
) {
    let mut states: Vec<_> = target
        .states()
        .iter()
        .map(|state| {
            (
                state.clone(),
                target.topology(state).cloned(),
                target.entity_versions(state),
            )
        })
        .collect();
    let (_, topology, versions) = &mut states[ordinal];
    change(topology.as_mut().unwrap(), versions);
    let scope = target.topology_scope;
    *target = history(
        target.id.clone(),
        target.byte_offset,
        target.preamble,
        target.binding_budget_exceeded,
        states,
    );
    target.topology_scope = scope;
}
