// SPDX-License-Identifier: Apache-2.0
//! Oldest-first state visits and the topologies they leave materialized.

use cadmpeg_asm::kernel_header::RefWidth;

use crate::history::{visit_state_groups_oldest_first, BindingHistories};
use crate::history_records::AsmHistory;
use crate::test_support::history_stream::DesignScale;

const SCALE: DesignScale = DesignScale {
    states: 12,
    bodies: 24,
    faces_per_body: 1,
    updates_per_state: 2,
};

fn decoded_histories() -> Vec<AsmHistory> {
    crate::test_support::with_decode_context(|ctx| {
        ["BREP.first.smbh", "BREP.second.smbh"]
            .into_iter()
            .map(|stream| {
                crate::history::decode(
                    ctx,
                    &SCALE.history_smbh(),
                    stream,
                    RefWidth::Eight,
                    &mut { u64::MAX },
                    &cadmpeg_core::decode::WorkBudget::new(
                        crate::history::binding::MAX_HISTORY_BINDING_WORK,
                    ),
                )
                .unwrap()
                .unwrap()
            })
            .collect()
    })
}

/// Every state of every history, newest history first and newest state first
/// within each history.
fn newest_first_jobs(histories: &[AsmHistory]) -> Vec<(usize, usize, (usize, usize))> {
    let mut jobs = Vec::new();
    for (history_index, history) in histories.iter().enumerate().rev() {
        for state_index in history.states_oldest_first().unwrap().into_iter().rev() {
            for _ in 0..2 {
                jobs.push((history_index, state_index, (history_index, state_index)));
            }
        }
    }
    jobs
}

#[test]
fn releasing_visits_walk_each_history_oldest_first_and_keep_one_state() {
    let mut histories = decoded_histories();
    let jobs = newest_first_jobs(&histories);
    let mut visited = Vec::new();
    visit_state_groups_oldest_first(
        BindingHistories::Release(&mut histories),
        jobs,
        |histories, history_index, state_index, group| {
            let history = &histories[history_index];
            assert!(history.topology(&history.states()[state_index]).is_some());
            assert_eq!(
                history.materialized_state_count(),
                1,
                "the previous visit's state advanced to this one"
            );
            assert_eq!(group, vec![(history_index, state_index); 2]);
            visited.push((history_index, state_index));
        },
    );
    let expected = histories
        .iter()
        .enumerate()
        .flat_map(|(history_index, history)| {
            history
                .states_oldest_first()
                .unwrap()
                .into_iter()
                .map(move |state_index| (history_index, state_index))
        })
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), 2 * SCALE.states);
    assert_eq!(visited, expected);
    for history in &histories {
        assert_eq!(history.materialized_state_count(), 1);
    }
}

#[test]
fn shared_visits_leave_materializations_in_place() {
    let histories = decoded_histories();
    let jobs = newest_first_jobs(&histories);
    visit_state_groups_oldest_first(
        BindingHistories::Shared(&histories),
        jobs,
        |histories, history_index, state_index, _| {
            let history = &histories[history_index];
            assert!(history.topology(&history.states()[state_index]).is_some());
        },
    );
    for history in &histories {
        assert_eq!(history.materialized_state_count(), SCALE.states);
    }
}
