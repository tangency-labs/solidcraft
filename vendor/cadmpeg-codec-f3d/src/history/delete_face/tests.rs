// SPDX-License-Identifier: Apache-2.0
use super::delete_face_selection;
use crate::history_records::{
    AsmHistoricalEntityDelta, AsmHistoricalRowsDelta, AsmHistoricalTopology,
    AsmHistoricalTopologyDelta, AsmHistoricalTransition,
};

const PREVIOUS: i64 = 7;

fn transition(previous_state_id: i64, deleted: &[i64]) -> AsmHistoricalTransition {
    AsmHistoricalTransition {
        previous_state_id: Some(previous_state_id),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                deleted: deleted.to_vec(),
                ..AsmHistoricalEntityDelta::default()
            },
            ..AsmHistoricalTopologyDelta::default()
        },
        record_revisions: Vec::new(),
        rows: AsmHistoricalRowsDelta::default(),
    }
}

fn preceding(faces: &[i64]) -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: faces.to_vec(),
        ..AsmHistoricalTopology::default()
    }
}

#[test]
fn deleted_faces_are_the_selection_when_they_match_the_member_count() {
    assert_eq!(
        delete_face_selection(
            &[100],
            PREVIOUS,
            &transition(PREVIOUS, &[10]),
            &preceding(&[10, 20])
        ),
        Some(vec![10])
    );
    assert_eq!(
        delete_face_selection(
            &[100, 101],
            PREVIOUS,
            &transition(PREVIOUS, &[20, 10]),
            &preceding(&[10, 20, 30])
        ),
        Some(vec![10, 20])
    );
}

#[test]
fn healing_that_deletes_a_further_face_states_no_selection() {
    assert_eq!(
        delete_face_selection(
            &[100],
            PREVIOUS,
            &transition(PREVIOUS, &[10, 20]),
            &preceding(&[10, 20])
        ),
        None
    );
}

#[test]
fn fewer_deleted_faces_than_members_state_no_selection() {
    assert_eq!(
        delete_face_selection(
            &[100, 101],
            PREVIOUS,
            &transition(PREVIOUS, &[10]),
            &preceding(&[10, 20])
        ),
        None
    );
}

#[test]
fn a_repeated_member_counts_once() {
    assert_eq!(
        delete_face_selection(
            &[100, 100],
            PREVIOUS,
            &transition(PREVIOUS, &[10]),
            &preceding(&[10, 20])
        ),
        Some(vec![10])
    );
    assert_eq!(
        delete_face_selection(
            &[100, 100],
            PREVIOUS,
            &transition(PREVIOUS, &[10, 20]),
            &preceding(&[10, 20])
        ),
        None
    );
}

#[test]
fn every_deleted_face_must_belong_to_the_preceding_state() {
    for deleted in [[10, 99], [99, 10]] {
        assert_eq!(
            delete_face_selection(
                &[100, 101],
                PREVIOUS,
                &transition(PREVIOUS, &deleted),
                &preceding(&[10, 20])
            ),
            None
        );
    }
}

#[test]
fn a_transition_from_another_state_states_no_selection() {
    assert_eq!(
        delete_face_selection(
            &[100],
            PREVIOUS,
            &transition(PREVIOUS - 1, &[10]),
            &preceding(&[10, 20])
        ),
        None
    );
}

#[test]
fn a_transition_that_deletes_nothing_states_no_selection() {
    assert_eq!(
        delete_face_selection(
            &[],
            PREVIOUS,
            &transition(PREVIOUS, &[]),
            &preceding(&[10, 20])
        ),
        None
    );
}
