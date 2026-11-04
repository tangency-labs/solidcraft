// SPDX-License-Identifier: Apache-2.0
//! Resolve the complete ASM transition interval of a feature scope.

use super::{
    header_previous_state_id, history_state_pair, transition_or_header_previous_state_id,
    AsmHistory,
};
use crate::history_records::AsmHistoricalEntityDelta;
use std::collections::{BTreeSet, HashMap, HashSet};

pub(super) struct OutputChain {
    pub(super) history: usize,
    pub(super) states: Vec<i64>,
    transient: BTreeSet<i64>,
}

/// Body slots whose first change in the interval is an insertion and whose
/// last change is a deletion.
fn transient_bodies<'a>(
    oldest_first: impl Iterator<Item = &'a AsmHistoricalEntityDelta>,
) -> BTreeSet<i64> {
    let mut first_inserted = HashMap::new();
    let mut last_deleted = HashMap::new();
    for delta in oldest_first {
        for &slot in &delta.deleted {
            first_inserted.entry(slot).or_insert(false);
            last_deleted.insert(slot, true);
        }
        for &slot in &delta.inserted {
            first_inserted.entry(slot).or_insert(true);
            last_deleted.insert(slot, false);
        }
    }
    first_inserted
        .into_iter()
        .filter(|&(slot, inserted)| inserted && last_deleted.get(&slot) == Some(&true))
        .map(|(slot, _)| slot)
        .collect()
}

impl OutputChain {
    pub(super) fn resolve(
        histories: &[AsmHistory],
        current: i64,
        previous: i64,
        bound: Option<&str>,
    ) -> Option<Self> {
        if current == previous {
            return None;
        }
        if bound.is_some_and(|id| histories.iter().filter(|history| history.id == id).count() != 1)
        {
            return None;
        }
        let mut matches = histories
            .iter()
            .enumerate()
            .filter(|(_, history)| bound.is_none_or(|id| history.id == id))
            .filter_map(|(index, history)| {
                history_state_pair(history, current, previous, false)
                    .map(|(_, state, _)| (index, history, state))
            });
        let (index, history, mut state) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let mut states = Vec::new();
        let mut body_deltas = Vec::new();
        let mut visited = HashSet::new();
        while state.state_id != previous {
            if !visited.insert(state.state_id) {
                return None;
            }
            let transition = state.transition.as_ref()?;
            let predecessor = transition.previous_state_id?;
            if header_previous_state_id(history, state) != Some(predecessor)
                || transition_or_header_previous_state_id(history, state) != Some(predecessor)
            {
                return None;
            }
            states.push(state.state_id);
            body_deltas.push(&transition.topology.bodies);
            let mut predecessors = history
                .states()
                .iter()
                .filter(|candidate| candidate.state_id == predecessor);
            state = predecessors.next()?;
            if predecessors.next().is_some() {
                return None;
            }
        }
        Some(Self {
            history: index,
            states,
            transient: transient_bodies(body_deltas.into_iter().rev()),
        })
    }

    pub(super) fn outputs(
        &self,
        states: &HashMap<(usize, i64), Option<Vec<i64>>>,
    ) -> Option<Vec<i64>> {
        let mut outputs = BTreeSet::new();
        for state in &self.states {
            outputs.extend(states.get(&(self.history, *state))?.as_ref()?);
        }
        Some(outputs.difference(&self.transient).copied().collect())
    }
}

#[cfg(test)]
mod tests;
