// SPDX-License-Identifier: Apache-2.0
use crate::history::selection::entity_selection_edge_candidates;
use crate::history::selection::historical_identity_kind;
use crate::history::selection::historical_selection_identity_kind;
use crate::history::selection::unique_entity_selection_edge;
use crate::history::selection::HistoricalIdentityIndex;
use crate::history_records::AsmBulletinBoard;
use crate::history_records::AsmDeltaState;
use crate::history_records::AsmEntityChange;
use crate::history_records::AsmEntityChangeKind;
use crate::history_records::AsmEntityVersion;
use crate::history_records::AsmHistoricalCoedge;
use crate::history_records::AsmHistoricalEdge;
use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

#[test]
fn design_identity_resolves_only_one_invariant_history_family() {
    let state = |state_id, topology| {
        (
            AsmDeltaState {
                id: format!("state-{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology),
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            state(
                3,
                AsmHistoricalTopology {
                    edges: vec![42],
                    ..AsmHistoricalTopology::default()
                },
            ),
            state(
                5,
                AsmHistoricalTopology {
                    edges: vec![42],
                    vertices: vec![90],
                    ..AsmHistoricalTopology::default()
                },
            ),
        ],
    );
    assert_eq!(
        historical_identity_kind(std::slice::from_ref(&history), 42),
        Some((AsmHistoricalEntityKind::Edge, vec![3, 5]))
    );
    assert_eq!(
        historical_identity_kind(std::slice::from_ref(&history), 90),
        Some((AsmHistoricalEntityKind::Vertex, vec![5]))
    );
    assert_eq!(
        historical_selection_identity_kind(std::slice::from_ref(&history), 42),
        Some((AsmHistoricalEntityKind::Edge, 42, vec![3, 5]))
    );
    assert_eq!(
        historical_identity_kind(std::slice::from_ref(&history), 7),
        None
    );
    let mut revision_history = crate::test_support::history::history(
        history.id.clone(),
        history.byte_offset,
        history.preamble,
        false,
        history
            .states()
            .iter()
            .enumerate()
            .map(|(index, state)| {
                let topology = history.topology(state).unwrap().clone();
                let versions = topology
                    .slots()
                    .into_iter()
                    .map(|entity_ref| AsmEntityVersion {
                        entity_ref,
                        record_ref: if entity_ref == 42 {
                            if index == 0 {
                                700
                            } else {
                                701
                            }
                        } else {
                            entity_ref
                        },
                    })
                    .collect();
                (state.clone(), Some(topology), versions)
            })
            .collect(),
    );
    assert_eq!(
        historical_selection_identity_kind(std::slice::from_ref(&revision_history), 700),
        Some((AsmHistoricalEntityKind::Edge, 42, vec![3]))
    );
    assert_eq!(
        historical_selection_identity_kind(std::slice::from_ref(&revision_history), 701),
        Some((AsmHistoricalEntityKind::Edge, 42, vec![5]))
    );
    let revision_change = |new_ref| AsmEntityChange {
        id: format!("revision-700-to-{new_ref}"),
        parent: "board".into(),
        byte_offset: 0,
        kind: AsmEntityChangeKind::Update {
            old: 700,
            new: new_ref,
        },
    };
    let mut reconstructed_revision_history = history.clone();
    reconstructed_revision_history.states_mut()[0].bulletin_boards = vec![AsmBulletinBoard {
        id: "board".into(),
        parent: reconstructed_revision_history.states()[0].id.clone(),
        byte_offset: 0,
        owner_ref: 0,
        number: 2,
        changes: vec![revision_change(42)],
    }];
    assert_eq!(
        historical_selection_identity_kind(
            std::slice::from_ref(&reconstructed_revision_history),
            700
        ),
        Some((AsmHistoricalEntityKind::Edge, 42, vec![3, 5]))
    );
    let mut incomplete_revision_history = reconstructed_revision_history.clone();
    incomplete_revision_history.topology_scope = crate::history_records::AsmTopologyScope::Retained;
    assert_eq!(
        historical_selection_identity_kind(std::slice::from_ref(&incomplete_revision_history), 700),
        None
    );
    reconstructed_revision_history.states_mut()[0].bulletin_boards[0]
        .changes
        .push(revision_change(90));
    assert_eq!(
        historical_selection_identity_kind(
            std::slice::from_ref(&reconstructed_revision_history),
            700
        ),
        None
    );
    crate::test_support::history::edit(&mut revision_history, 0, |topology, versions| {
        topology.vertices.push(90);
        versions.push(AsmEntityVersion {
            entity_ref: 90,
            record_ref: 42,
        });
    });
    assert_eq!(
        historical_selection_identity_kind(std::slice::from_ref(&revision_history), 42),
        None
    );
    let duplicate_state_history = crate::test_support::history::history(
        "duplicate-state-history".into(),
        0,
        None,
        false,
        vec![state(
            3,
            AsmHistoricalTopology {
                vertices: vec![42],
                ..AsmHistoricalTopology::default()
            },
        )],
    );
    assert_eq!(
        historical_identity_kind(&[history.clone(), duplicate_state_history.clone()], 42),
        Some((AsmHistoricalEntityKind::Edge, vec![5]))
    );
    let mut duplicate_revision_history = duplicate_state_history;
    crate::test_support::history::edit(&mut duplicate_revision_history, 0, |_, versions| {
        *versions = vec![AsmEntityVersion {
            entity_ref: 42,
            record_ref: 700,
        }];
    });
    assert_eq!(
        historical_selection_identity_kind(
            &[revision_history.clone(), duplicate_revision_history],
            700
        ),
        None
    );
    let ambiguous = crate::test_support::history::history(
        "other-history".into(),
        0,
        None,
        false,
        vec![state(
            7,
            AsmHistoricalTopology {
                vertices: vec![42],
                ..AsmHistoricalTopology::default()
            },
        )],
    );
    assert_eq!(historical_identity_kind(&[history, ambiguous], 42), None);
}

#[test]
fn nested_entity_identity_resolves_through_input_coedge_incidence() {
    let topology = AsmHistoricalTopology {
        coedges: vec![42],
        edges: vec![17, 18],
        vertices: vec![50, 51, 52],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 42,
            owner_loop: 5,
            edge: 17,
            next: 42,
            previous: 42,
            radial_next: 42,
        }],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 17,
                start_vertex: 50,
                end_vertex: 51,
            },
            AsmHistoricalEdge {
                edge: 18,
                start_vertex: 50,
                end_vertex: 52,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![(
            AsmDeltaState {
                id: "state-3".into(),
                parent: "history".into(),
                byte_offset: 0,
                state_id: 3,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: 3,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology.clone()),
            vec![
                AsmEntityVersion {
                    entity_ref: 42,
                    record_ref: 700,
                },
                AsmEntityVersion {
                    entity_ref: 50,
                    record_ref: 800,
                },
            ],
        )],
    );
    let identities = HistoricalIdentityIndex::build(std::slice::from_ref(&history), [700, 800]);
    let candidates =
        { entity_selection_edge_candidates([(0, 700), (1, 800)], 3, &identities, &topology) };
    assert_eq!(
        candidates,
        [
            crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate {
                identity_ordinal: 0,
                local_id: 700,
                historical_entity_kind: AsmHistoricalEntityKind::Coedge,
                historical_entity_ref: 42,
                edge_slots: vec![17],
            },
            crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate {
                identity_ordinal: 1,
                local_id: 800,
                historical_entity_kind: AsmHistoricalEntityKind::Vertex,
                historical_entity_ref: 50,
                edge_slots: vec![17, 18],
            },
        ]
    );
    assert_eq!(unique_entity_selection_edge(&candidates), Some(17));
}

#[test]
fn partially_bound_history_resolves_no_reconstructed_revision() {
    let topology = AsmHistoricalTopology {
        edges: vec![42],
        ..Default::default()
    };
    let state = |state_id| {
        (
            AsmDeltaState {
                id: format!("state-{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology.clone()),
            Vec::new(),
        )
    };
    let mut history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(3), state(5)],
    );
    history.states_mut()[0].bulletin_boards = vec![AsmBulletinBoard {
        id: "board".into(),
        parent: "state-3".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 2,
        changes: vec![AsmEntityChange {
            id: "revision-700-to-42".into(),
            parent: "board".into(),
            byte_offset: 0,
            kind: AsmEntityChangeKind::Update { old: 700, new: 42 },
        }],
    }];
    history.states_mut()[1].transition = None;
    assert_eq!(
        history.check_transitions(),
        Err(crate::history_records::AsmTransitionError::MissingTransition)
    );
    assert_eq!(historical_selection_identity_kind(&[history], 700), None);
}
