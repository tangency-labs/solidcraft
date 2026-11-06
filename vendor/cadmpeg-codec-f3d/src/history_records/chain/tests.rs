// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::history_records::*;
use cadmpeg_ir::math::{Point3, Vector3};

fn topology(keys: &[i64], changed: bool) -> AsmHistoricalTopology {
    let value = if changed { 5.0 } else { 2.0 };
    let origin = Point3::new(value, 0.0, 0.0);
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let mut topology = AsmHistoricalTopology {
        bodies: keys.to_vec(),
        regions: keys.to_vec(),
        shells: keys.to_vec(),
        faces: keys.to_vec(),
        loops: keys.to_vec(),
        coedges: keys.to_vec(),
        edges: keys.to_vec(),
        vertices: keys.to_vec(),
        points: keys.to_vec(),
        surfaces: keys.iter().flat_map(|&key| [key, key]).collect(),
        curves: keys.iter().flat_map(|&key| [key, key, key]).collect(),
        pcurves: keys.to_vec(),
        surface_radii: keys
            .iter()
            .map(|&key| AsmHistoricalSurfaceRadius {
                surface: key,
                radius: value,
            })
            .collect(),
        surface_cylinders: keys
            .iter()
            .map(|&key| AsmHistoricalCylinder {
                surface: key,
                origin,
                axis,
                radius: value,
            })
            .collect(),
        surface_planes: keys
            .iter()
            .map(|&key| AsmHistoricalPlane {
                surface: key,
                origin,
                normal: axis,
            })
            .collect(),
        surface_axes: keys
            .iter()
            .map(|&key| AsmHistoricalSurfaceAxis {
                surface: key,
                origin,
                direction: axis,
            })
            .collect(),
        curve_axes: keys
            .iter()
            .map(|&key| AsmHistoricalCurveAxis {
                curve: key,
                origin,
                direction: axis,
            })
            .collect(),
        body_regions: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        region_shells: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        shell_faces: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        shell_wire_edges: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        shell_free_vertices: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        face_loops: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        loop_coedges: keys
            .iter()
            .map(|&key| AsmHistoricalRelation {
                owner_ref: key,
                member_refs: vec![if changed { 9 } else { 8 }],
            })
            .collect(),
        coedge_topology: keys
            .iter()
            .map(|&key| AsmHistoricalCoedge {
                coedge: key,
                owner_loop: 8,
                edge: 8,
                next: 8,
                previous: 8,
                radial_next: if changed { 9 } else { 8 },
            })
            .collect(),
        edge_vertices: keys
            .iter()
            .map(|&key| AsmHistoricalEdge {
                edge: key,
                start_vertex: 8,
                end_vertex: if changed { 9 } else { 8 },
            })
            .collect(),
        face_surfaces: keys
            .iter()
            .map(|&key| AsmHistoricalCarrierBinding {
                entity: key,
                carrier: if changed { 9 } else { 8 },
            })
            .collect(),
        vertex_points: keys
            .iter()
            .map(|&key| AsmHistoricalCarrierBinding {
                entity: key,
                carrier: if changed { 9 } else { 8 },
            })
            .collect(),
        edge_curves: keys
            .iter()
            .map(|&key| AsmHistoricalOptionalCarrierBinding {
                entity: key,
                carrier: if changed { None } else { Some(8) },
            })
            .collect(),
        coedge_pcurves: keys
            .iter()
            .map(|&key| AsmHistoricalOptionalCarrierBinding {
                entity: key,
                carrier: if changed { None } else { Some(8) },
            })
            .collect(),
        point_positions: keys
            .iter()
            .map(|&key| AsmHistoricalPoint {
                point: key,
                position: origin,
            })
            .collect(),
        edge_samples: keys
            .iter()
            .map(|&key| AsmHistoricalEdgeSample {
                edge: key,
                interior: origin,
                tangent: if changed { None } else { Some(axis) },
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    };
    for attribute in [10, 20] {
        for ordinal in 0..2 {
            topology
                .persistent_subentity_tags
                .push(AsmHistoricalPersistentSubentityTag {
                    entity_kind:
                        crate::records::topology::body_recipe::AsmHistoricalEntityKind::Face,
                    entity_ref: attribute,
                    selector: attribute,
                    token: if changed { "changed" } else { "tag" }.into(),
                    design_references: vec![30],
                    ordinal,
                });
        }
    }
    topology
}

fn chain() -> AsmHistory {
    let topologies = [
        AsmHistoricalTopology {
            bodies: vec![1],
            ..Default::default()
        },
        topology(&[1, 2], false),
        topology(&[2, 3], false),
        topology(&[2, 3], true),
    ];
    // The tail has membership but no retained relations.
    let versions: Vec<BTreeMap<_, _>> = [
        vec![(0, 0), (1, 1)],
        vec![(0, 0), (1, 1), (2, 2), (10, 10), (20, 20)],
        vec![(0, 0), (2, 2), (3, 3), (10, 10), (20, 20)],
        vec![(0, 0), (2, 22), (3, 33), (10, 110), (20, 120)],
    ]
    .into_iter()
    .map(|rows| rows.into_iter().collect())
    .collect();
    let mut states = Vec::new();
    for index in 0usize..4 {
        let snapshot = |index: usize| AsmStateSnapshot {
            state_id: i64::try_from(index).unwrap(),
            topology: &topologies[index],
            versions: &versions[index],
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        states.push(AsmDeltaState {
            id: format!("state-{index}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id: i64::try_from(index).unwrap(),
            version_flag: 1,
            state_flag: 0,
            previous_ref: (index < 3).then(|| i64::try_from(index + 1).unwrap()),
            next_ref: index
                .checked_sub(1)
                .map(|index| i64::try_from(index).unwrap()),
            node_index: i64::try_from(index).unwrap(),
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: Some(transition),
        });
    }
    // Storage order must not determine chain order.
    states.swap(0, 2);
    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        binding_budget_exceeded: false,
        states,
        topology_scope: AsmTopologyScope::Complete,
        materialized: crate::history_records::MaterializedTopologies::default(),
    }
}

fn expected_versions(state_id: i64) -> BTreeMap<i64, i64> {
    match state_id {
        0 => vec![(0, 0), (1, 1)],
        1 => vec![(0, 0), (1, 1), (2, 2), (10, 10), (20, 20)],
        2 => vec![(0, 0), (2, 2), (3, 3), (10, 10), (20, 20)],
        3 => vec![(0, 0), (2, 22), (3, 33), (10, 110), (20, 120)],
        _ => unreachable!(),
    }
    .into_iter()
    .collect()
}

fn expected_topology(state_id: i64, scope: AsmTopologyScope) -> Option<AsmHistoricalTopology> {
    let mut expected = match state_id {
        0 => AsmHistoricalTopology {
            bodies: vec![1],
            ..Default::default()
        },
        1 => topology(&[1, 2], false),
        2 => topology(&[2, 3], false),
        3 => topology(&[2, 3], true),
        _ => unreachable!(),
    };
    if scope == AsmTopologyScope::Retained {
        expected.surface_radii.clear();
        expected.surface_axes.clear();
        expected.persistent_subentity_tags.clear();
        expected.body_regions.clear();
        expected.region_shells.clear();
        expected.shell_faces.clear();
        expected.shell_wire_edges.clear();
        expected.shell_free_vertices.clear();
        expected.edge_vertices.clear();
        expected.vertex_points.clear();
        expected.point_positions.clear();
        expected.edge_samples.clear();
        if !expected.has_retained_rows() {
            return None;
        }
    }
    Some(expected)
}

#[test]
fn reconstructs_every_family_in_both_scopes() {
    let mut history = chain();
    assert_eq!(history.check_transitions(), Ok(()));
    for scope in [AsmTopologyScope::Complete, AsmTopologyScope::Retained] {
        for state in history.states() {
            assert_eq!(
                history.topology(state),
                expected_topology(state.state_id, scope).as_ref(),
                "{scope:?} state {}",
                state.state_id
            );
            let expected = expected_topology(state.state_id, scope);
            let slots = expected
                .as_ref()
                .map(AsmHistoricalTopology::slots)
                .unwrap_or_default();
            let versions: Vec<_> = expected_versions(state.state_id)
                .into_iter()
                .filter(|(slot, _)| slots.contains(slot))
                .map(|(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect();
            assert_eq!(history.entity_versions(state), versions);
        }
        crate::history::discard_projection_caches(std::slice::from_mut(&mut history));
    }
    assert!(history.topology(&history.states()[2]).is_none());
}

#[test]
fn cache_only_materializes_requested_members_and_resets() {
    let mut history = chain();
    assert!(history.materialized.states.get().is_none());
    assert!(history.topology(&history.states()[0]).is_some());
    let cache = history.materialized.states.get().unwrap();
    assert_eq!(
        cache.iter().filter(|entry| entry.get().is_some()).count(),
        1
    );
    let outsider = history.states()[0].clone();
    assert!(history.topology(&outsider).is_none());
    assert!(history.entity_versions(&outsider).is_empty());
    assert!(history.clone().materialized.states.get().is_none());
    history.release_topologies();
    assert!(history.materialized.states.get().is_none());
}

#[test]
fn advancing_moves_the_nearest_older_materialization_forward() {
    let mut history = chain();
    let index = |history: &AsmHistory, state_id| {
        history
            .states()
            .iter()
            .position(|state| state.state_id == state_id)
            .unwrap()
    };
    let expected =
        |history: &AsmHistory, state_id| expected_topology(state_id, history.topology_scope);
    history.advance_topology(index(&history, 1));
    assert_eq!(history.materialized_state_count(), 0);
    assert!(history
        .topology(&history.states()[index(&history, 0)])
        .is_some());
    for state_id in 1..4 {
        history.advance_topology(index(&history, state_id));
        assert_eq!(history.materialized_state_count(), 1, "state {state_id}");
        assert_eq!(
            history.topology(&history.states()[index(&history, state_id)]),
            expected(&history, state_id).as_ref(),
            "state {state_id}"
        );
    }
    history.advance_topology(index(&history, 1));
    assert_eq!(history.materialized_state_count(), 1);

    history.release_topologies();
    assert!(history
        .topology(&history.states()[index(&history, 0)])
        .is_some());
    history.advance_topology(index(&history, 3));
    assert_eq!(history.materialized_state_count(), 1);
    assert_eq!(
        history.topology(&history.states()[index(&history, 3)]),
        expected(&history, 3).as_ref()
    );

    let mut broken = chain();
    transition_mut(&mut broken, 2)
        .rows
        .face_surfaces
        .removed
        .push(99);
    assert!(broken
        .topology(&broken.states()[index(&broken, 1)])
        .is_some());
    broken.advance_topology(index(&broken, 2));
    assert!(broken
        .topology(&broken.states()[index(&broken, 2)])
        .is_none());
    assert!(broken
        .topology(&broken.states()[index(&broken, 3)])
        .is_none());
}

#[test]
fn transition_and_history_serde_preserve_data_and_clear_cache() {
    let history = chain();
    for state in history.states() {
        assert!(history.topology(state).is_some());
        let transition = state.transition.as_ref().unwrap();
        let bytes = serde_json::to_vec(transition).unwrap();
        assert_eq!(*transition, serde_json::from_slice(&bytes).unwrap());
    }
    let bytes = serde_json::to_vec(&history).unwrap();
    let decoded: AsmHistory = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, history);
    assert!(decoded.materialized.states.get().is_none());
    for state in decoded.states() {
        assert_eq!(
            decoded.topology(state),
            expected_topology(state.state_id, AsmTopologyScope::Complete).as_ref()
        );
    }
}

fn transition_mut(history: &mut AsmHistory, id: i64) -> &mut AsmHistoricalTransition {
    history
        .states_mut()
        .iter_mut()
        .find(|state| state.state_id == id)
        .unwrap()
        .transition
        .as_mut()
        .unwrap()
}

#[test]
fn transition_invariant_errors() {
    use AsmTransitionError::*;
    type BrokenChain = (AsmTransitionError, fn(&mut AsmHistory));
    let cases: Vec<BrokenChain> = vec![
        (MissingTransition, |h| h.states_mut()[0].transition = None),
        (Chain, |h| h.states_mut()[0].next_ref = Some(999)),
        (Chain, |h| h.states_mut()[2].next_ref = Some(3)),
        (Chain, |h| {
            h.states_mut()[0].node_index = h.states()[1].node_index;
        }),
        (PreviousState, |h| {
            transition_mut(h, 1).previous_state_id = Some(99);
        }),
        (PreviousState, |h| {
            transition_mut(h, 0).previous_state_id = Some(99);
        }),
        (InsertedPresent, |h| {
            let t = transition_mut(h, 1);
            t.records.inserted.insert(0, 1);
            t.record_revisions.insert(
                0,
                AsmEntityVersion {
                    entity_ref: 1,
                    record_ref: 1,
                },
            );
        }),
        (DeletedAbsent, |h| {
            transition_mut(h, 1).records.deleted.push(99);
        }),
        (InvalidUpdate, |h| {
            let t = transition_mut(h, 3);
            t.record_revisions[0].record_ref = 2;
        }),
        (InvalidUpdate, |h| {
            let t = transition_mut(h, 0);
            t.records.updated.push(99);
            t.record_revisions.push(AsmEntityVersion {
                entity_ref: 99,
                record_ref: 100,
            });
        }),
        (RevisionCoverage, |h| {
            transition_mut(h, 0).record_revisions.pop();
        }),
        (RevisionCoverage, |h| {
            transition_mut(h, 0)
                .record_revisions
                .push(AsmEntityVersion {
                    entity_ref: 99,
                    record_ref: 99,
                });
        }),
        (RemovedRowAbsent, |h| {
            transition_mut(h, 0).rows.face_surfaces.removed.push(99);
        }),
        (UnsortedFamily, |h| {
            transition_mut(h, 1).topology.faces.inserted.reverse();
        }),
        (UnsortedFamily, |h| {
            transition_mut(h, 1).rows.face_surfaces.set.reverse();
        }),
        (InsertedPresent, |h| {
            transition_mut(h, 2).topology.faces.inserted.insert(0, 2);
        }),
        (DeletedAbsent, |h| {
            transition_mut(h, 2).topology.faces.deleted.push(99);
        }),
        (InvalidUpdate, |h| {
            transition_mut(h, 0).topology.faces.updated.push(99);
        }),
        (InvalidUpdate, |h| {
            transition_mut(h, 3).topology.faces.updated = vec![2];
        }),
    ];
    for (case, (expected, break_chain)) in cases.into_iter().enumerate() {
        let mut history = chain();
        break_chain(&mut history);
        assert_eq!(history.check_transitions(), Err(expected), "case {case}");
        let head = history
            .states()
            .iter()
            .find(|state| state.state_id == 3)
            .unwrap();
        assert!(history.topology(head).is_none(), "case {case}");
        assert!(history.entity_versions(head).is_empty(), "case {case}");
        let requested = std::collections::HashSet::from([1, 2, 3, 10, 20, 22, 33, 110, 120]);
        assert_identity_query(&mut history, &requested);
    }
    let mut unbound = chain();
    for state in unbound.states_mut() {
        state.transition = None;
    }
    assert_eq!(unbound.check_transitions(), Ok(()));
    for state in unbound.states() {
        assert!(unbound.topology(state).is_none());
    }
}

#[test]
fn between_rejects_unsorted_families_without_reordering() {
    let versions = BTreeMap::new();
    let check = |topology: &AsmHistoricalTopology| {
        let snapshot = || AsmStateSnapshot {
            state_id: 1,
            topology,
            versions: &versions,
        };
        assert!(AsmHistoricalTransition::between(None, snapshot()).is_none());
        assert!(AsmHistoricalTransition::between(
            Some(snapshot()),
            AsmStateSnapshot {
                state_id: 2,
                topology: &AsmHistoricalTopology::default(),
                versions: &versions
            }
        )
        .is_none());
    };
    {
        let mut bad = topology(&[1, 2], false);
        bad.bodies.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.regions.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shells.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.faces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.loops.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.coedges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.edges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.vertices.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.points.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surfaces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.curves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.pcurves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_radii.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_cylinders.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_planes.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.surface_axes.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.curve_axes.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.body_regions.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.region_shells.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shell_faces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shell_wire_edges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.shell_free_vertices.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.face_loops.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.loop_coedges.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.coedge_topology.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.edge_vertices.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.face_surfaces.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.vertex_points.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.edge_curves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.coedge_pcurves.reverse();
        check(&bad);
    }
    {
        let mut bad = topology(&[1, 2], false);
        bad.point_positions.reverse();
        check(&bad);
    }
    let mut bad = topology(&[1, 2], false);
    bad.persistent_subentity_tags.reverse();
    check(&bad);
}

#[test]
fn history_remains_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<AsmHistory>();
}

#[test]
fn older_materialization_does_not_validate_newer_deltas() {
    let history = &mut chain();
    transition_mut(history, 3)
        .rows
        .face_surfaces
        .removed
        .push(99);
    let older = history
        .states()
        .iter()
        .find(|state| state.state_id == 2)
        .unwrap();
    assert_eq!(
        history.topology(older),
        expected_topology(older.state_id, AsmTopologyScope::Complete).as_ref()
    );
    assert_eq!(
        history.entity_versions(older),
        expected_versions(older.state_id)
            .into_iter()
            .filter(
                |(slot, _)| expected_topology(older.state_id, AsmTopologyScope::Complete)
                    .unwrap()
                    .slots()
                    .contains(slot)
            )
            .map(|(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(
        history.check_transitions(),
        Err(AsmTransitionError::RemovedRowAbsent)
    );
    let newer = history
        .states()
        .iter()
        .find(|state| state.state_id == 3)
        .unwrap();
    assert!(history.topology(newer).is_none());
}

#[test]
fn cached_older_state_keeps_record_revision_validation() {
    let history = &mut chain();
    transition_mut(history, 3).record_revisions[0].record_ref = 2;
    let older = history
        .states()
        .iter()
        .find(|state| state.state_id == 2)
        .unwrap();
    assert_eq!(
        history.topology(older),
        expected_topology(older.state_id, AsmTopologyScope::Complete).as_ref()
    );
    let newer = history
        .states()
        .iter()
        .find(|state| state.state_id == 3)
        .unwrap();
    assert!(history.topology(newer).is_none());
}

#[test]
fn empty_deltas_leave_family_storage_untouched() {
    let mut family = topology(&[1, 2], false);
    let before_rows = family.face_surfaces.as_ptr();
    let before_members = family.faces.as_ptr();
    let before_tags = family.persistent_subentity_tags.as_ptr();
    AsmHistoricalTransition {
        previous_state_id: None,
        records: AsmHistoricalEntityDelta::default(),
        record_revisions: Vec::new(),
        topology: AsmHistoricalTopologyDelta::default(),
        rows: AsmHistoricalRowsDelta::default(),
    }
    .apply_topology(&mut family)
    .unwrap();
    assert_eq!(family.face_surfaces.as_ptr(), before_rows);
    assert_eq!(family.faces.as_ptr(), before_members);
    assert_eq!(family.persistent_subentity_tags.as_ptr(), before_tags);
}

#[test]
fn linear_merges_reconstruct_all_small_membership_and_row_changes() {
    for old_mask in 0..8 {
        for new_mask in 0..8 {
            let keys = |mask| {
                (1..=3)
                    .filter(|key| mask & (1 << (key - 1)) != 0)
                    .collect::<Vec<_>>()
            };
            let old_keys = keys(old_mask);
            let new_keys = keys(new_mask);
            let old = topology(&old_keys, false);
            let new = topology(&new_keys, true);
            let old_versions = old_keys
                .iter()
                .map(|&key| (key, key))
                .collect::<BTreeMap<_, _>>();
            let new_versions = new_keys
                .iter()
                .map(|&key| (key, key + 10))
                .collect::<BTreeMap<_, _>>();
            let transition = AsmHistoricalTransition::between(
                Some(AsmStateSnapshot {
                    state_id: 1,
                    topology: &old,
                    versions: &old_versions,
                }),
                AsmStateSnapshot {
                    state_id: 2,
                    topology: &new,
                    versions: &new_versions,
                },
            )
            .unwrap();
            let mut actual = old;
            transition.apply_topology(&mut actual).unwrap();
            assert_eq!(actual, new, "{old_mask} -> {new_mask}");
            let mut actual_versions = old_versions
                .into_iter()
                .map(|(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect();
            transition.apply_versions(&mut actual_versions).unwrap();
            assert_eq!(
                actual_versions,
                new_versions
                    .into_iter()
                    .map(|(entity_ref, record_ref)| AsmEntityVersion {
                        entity_ref,
                        record_ref
                    })
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn key_run_replacement_matches_a_keyed_model() {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    for case in 0..500 {
        let mut model = BTreeMap::<i64, Vec<(i64, u64)>>::new();
        for key in 0..64 {
            if next(3) == 0 {
                model.insert(key, (0..=next(3)).map(|ordinal| (key, ordinal)).collect());
            }
        }
        let mut family = model.values().flatten().copied().collect::<Vec<_>>();
        family.shrink_to_fit();
        let capacity = family.capacity();
        let mut set = Vec::new();
        let mut removed = Vec::new();
        for key in 0..64 {
            let present = model.contains_key(&key);
            let choice = next(6);
            if present && choice < 2 {
                removed.push(key);
                model.remove(&key);
            }
            if choice == 1 || choice == 2 {
                let rows = (0..=next(3))
                    .map(|ordinal| (key, 100 * case + ordinal))
                    .collect::<Vec<_>>();
                model.insert(key, rows.clone());
                set.push((key, rows));
            }
        }
        replace_key_runs(
            &mut family,
            |row| row.0,
            set.iter().map(|(key, rows)| (*key, rows.as_slice())),
            &removed,
        )
        .unwrap();
        assert_eq!(
            family,
            model.values().flatten().copied().collect::<Vec<_>>(),
            "case {case}"
        );
        assert!(
            family.capacity() <= capacity.max(family.len()),
            "case {case} grew past its rows"
        );
    }
}

#[test]
fn row_deltas_reject_unsorted_families_and_absent_removals() {
    let edge = |edge| AsmHistoricalEdge {
        edge,
        start_vertex: 0,
        end_vertex: 0,
    };
    let delta = AsmHistoricalRowDelta {
        set: vec![edge(2)],
        removed: Vec::new(),
    };
    assert_eq!(
        delta.apply(&mut vec![edge(3), edge(1)]),
        Err(AsmTransitionError::UnsortedFamily)
    );
    let delta = AsmHistoricalRowDelta {
        set: vec![edge(2)],
        removed: vec![4],
    };
    assert_eq!(
        delta.apply(&mut vec![edge(1), edge(3)]),
        Err(AsmTransitionError::RemovedRowAbsent)
    );
    let delta = AsmHistoricalRowDelta {
        set: vec![edge(2)],
        removed: vec![2],
    };
    assert_eq!(
        delta.apply(&mut vec![edge(1), edge(3)]),
        Err(AsmTransitionError::RemovedRowAbsent)
    );
}

#[test]
fn membership_changes_match_a_key_order_merge() {
    fn merged(
        family: &[i64],
        inserted: &[i64],
        deleted: &[i64],
    ) -> Result<Vec<i64>, AsmTransitionError> {
        let mut result = Vec::new();
        let mut old = family.iter().copied().peekable();
        let mut inserted = inserted.iter().copied().peekable();
        let mut deleted = deleted.iter().copied().peekable();
        while old.peek().is_some() || inserted.peek().is_some() {
            let key = match (old.peek(), inserted.peek()) {
                (Some(a), Some(b)) if a == b => return Err(AsmTransitionError::InsertedPresent),
                (Some(&a), Some(&b)) => a.min(b),
                (Some(&key), None) | (None, Some(&key)) => key,
                (None, None) => break,
            };
            if deleted.peek().is_some_and(|&removed| removed < key) {
                return Err(AsmTransitionError::DeletedAbsent);
            }
            if inserted.peek() == Some(&key) {
                inserted.next();
                result.push(key);
            } else {
                old.next();
                if deleted.peek() == Some(&key) {
                    deleted.next();
                } else {
                    result.push(key);
                }
            }
        }
        if deleted.next().is_some() {
            return Err(AsmTransitionError::DeletedAbsent);
        }
        Ok(result)
    }
    let slots = |mask: u32| {
        (1..=5)
            .filter(|slot| mask & (1 << (slot - 1)) != 0)
            .collect::<Vec<i64>>()
    };
    for family_mask in 0..32 {
        for inserted_mask in 0..32 {
            for deleted_mask in (0..32).filter(|mask| mask & inserted_mask == 0) {
                let delta = AsmHistoricalEntityDelta {
                    inserted: slots(inserted_mask),
                    deleted: slots(deleted_mask),
                    updated: Vec::new(),
                };
                let mut family = slots(family_mask);
                let expected = merged(&family, &delta.inserted, &delta.deleted);
                let actual = apply_membership(&delta, &mut family, &[]).map(|()| family);
                assert_eq!(
                    actual, expected,
                    "{family_mask:05b} {inserted_mask:05b} {deleted_mask:05b}"
                );
            }
        }
    }
}

#[test]
fn tag_groups_keep_decode_order_within_each_entity() {
    let original = topology(&[1, 2], false);
    let groups = entity_tags(&original);
    assert_eq!(
        groups.iter().map(AsmHistoricalRow::key).collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(
        groups
            .iter()
            .flat_map(|group| &group.tags)
            .cloned()
            .collect::<Vec<_>>(),
        original.persistent_subentity_tags
    );
    for group in groups {
        assert_eq!(
            group.tags.iter().map(|tag| tag.ordinal).collect::<Vec<_>>(),
            [0, 1]
        );
    }
}

#[test]
fn carrier_groups_preserve_membership_multiplicity_changes() {
    let topologies = [
        AsmHistoricalTopology::default(),
        AsmHistoricalTopology {
            surfaces: vec![2, 2, 4],
            curves: vec![3, 3, 3],
            ..Default::default()
        },
        AsmHistoricalTopology {
            surfaces: vec![2, 4, 4, 4],
            curves: vec![3, 3, 3, 5, 5],
            ..Default::default()
        },
        AsmHistoricalTopology {
            surfaces: vec![4, 4],
            ..Default::default()
        },
        AsmHistoricalTopology::default(),
    ];
    let versions: Vec<BTreeMap<_, _>> = topologies
        .iter()
        .map(|topology| topology.slots().into_iter().map(|key| (key, key)).collect())
        .collect();
    let mut materialized = AsmHistoricalTopology::default();
    let mut retained = AsmHistoricalTopology::default();
    let mut materialized_versions = Vec::new();
    for index in 0usize..topologies.len() {
        let snapshot = |index: usize| AsmStateSnapshot {
            state_id: i64::try_from(index).unwrap(),
            topology: &topologies[index],
            versions: &versions[index],
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        if index == 1 {
            assert_eq!(transition.topology.surfaces.inserted, [2, 4]);
            assert_eq!(transition.topology.curves.inserted, [3]);
            assert_eq!(
                transition.rows.surface_repeats.set[0],
                AsmHistoricalSlotRepeat { slot: 2, count: 2 }
            );
            assert_eq!(
                transition.rows.curve_repeats.set[0],
                AsmHistoricalSlotRepeat { slot: 3, count: 3 }
            );
        }
        if index == 2 {
            assert_eq!(
                transition.topology.surfaces,
                AsmHistoricalEntityDelta::default()
            );
            assert_eq!(
                transition.rows.surface_repeats.set,
                [AsmHistoricalSlotRepeat { slot: 4, count: 3 }]
            );
            assert_eq!(transition.rows.surface_repeats.removed, [2]);
        }
        let mut decoded: AsmHistoricalTransition =
            serde_json::from_slice(&serde_json::to_vec(&transition).unwrap()).unwrap();
        decoded.apply_versions(&mut materialized_versions).unwrap();
        decoded.apply_topology(&mut materialized).unwrap();
        assert_eq!(materialized, topologies[index]);
        decoded.rows.retain_projection_rows();
        decoded.apply_topology(&mut retained).unwrap();
        assert_eq!(retained, topologies[index]);
    }
}

#[test]
fn shared_carrier_slot_rows_change_as_groups() {
    let cylinder = |surface, radius| AsmHistoricalCylinder {
        surface,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        radius,
    };
    let axis = |surface, x| AsmHistoricalSurfaceAxis {
        surface,
        origin: Point3::new(x, 0.0, 0.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
    };
    let topologies = [
        AsmHistoricalTopology::default(),
        AsmHistoricalTopology {
            surfaces: vec![2, 2, 4],
            surface_cylinders: vec![cylinder(2, 1.0), cylinder(2, 3.0), cylinder(4, 5.0)],
            surface_axes: vec![axis(2, 1.0), axis(2, 3.0), axis(4, 5.0)],
            ..Default::default()
        },
        AsmHistoricalTopology {
            surfaces: vec![2, 2, 4],
            surface_cylinders: vec![cylinder(2, 1.0), cylinder(2, 4.0), cylinder(4, 5.0)],
            surface_axes: vec![axis(2, 1.0), axis(2, 3.0), axis(4, 5.0)],
            ..Default::default()
        },
        AsmHistoricalTopology {
            surfaces: vec![4],
            surface_cylinders: vec![cylinder(4, 5.0)],
            surface_axes: vec![axis(4, 5.0)],
            ..Default::default()
        },
        AsmHistoricalTopology::default(),
    ];
    let versions = [
        BTreeMap::new(),
        BTreeMap::from([(2, 2), (4, 4)]),
        BTreeMap::from([(2, 12), (4, 4)]),
        BTreeMap::from([(4, 4)]),
        BTreeMap::new(),
    ];
    let mut materialized = AsmHistoricalTopology::default();
    let mut memberships = membership::Memberships::default();
    for index in 0usize..topologies.len() {
        let snapshot = |index: usize| AsmStateSnapshot {
            state_id: i64::try_from(index).unwrap(),
            topology: &topologies[index],
            versions: &versions[index],
        };
        let transition =
            AsmHistoricalTransition::between(index.checked_sub(1).map(snapshot), snapshot(index))
                .unwrap();
        if index == 2 {
            assert_eq!(
                transition.rows.surface_cylinders.set,
                [cylinder(2, 1.0), cylinder(2, 4.0)]
            );
            assert!(transition.rows.surface_cylinders.removed.is_empty());
            assert!(transition.rows.surface_axes.is_empty());
        }
        if index == 3 {
            assert!(transition.rows.surface_cylinders.set.is_empty());
            assert_eq!(transition.rows.surface_cylinders.removed, [2]);
            assert_eq!(transition.rows.surface_axes.removed, [2]);
        }
        let decoded: AsmHistoricalTransition =
            serde_json::from_slice(&serde_json::to_vec(&transition).unwrap()).unwrap();
        decoded.apply_topology(&mut materialized).unwrap();
        assert_eq!(materialized, topologies[index], "state {index}");
        memberships.apply(&decoded).unwrap();
    }
}

#[test]
fn carrier_groups_reject_inconsistent_memberships() {
    let cases: Vec<fn(&mut AsmHistory)> = vec![
        |history| {
            transition_mut(history, 1).rows.surface_repeats.set[0].count = 1;
        },
        |history| {
            transition_mut(history, 1)
                .rows
                .curve_repeats
                .set
                .last_mut()
                .unwrap()
                .slot = 99;
        },
        |history| {
            transition_mut(history, 1).rows.surface_repeats.set[0].count = 0;
        },
        |history| {
            transition_mut(history, 1).topology.curves.inserted.pop();
        },
    ];
    for (case, corrupt) in cases.into_iter().enumerate() {
        let mut history = chain();
        corrupt(&mut history);
        assert_eq!(
            history.check_transitions(),
            Err(AsmTransitionError::MembershipCoverage),
            "case {case}"
        );
        let head = history
            .states()
            .iter()
            .find(|state| state.state_id == 3)
            .unwrap();
        assert!(history.topology(head).is_none(), "case {case}");
    }
}

#[test]
fn states_mut_invalidates_materializations_before_mutation() {
    let mut history = chain();
    let index = history
        .states()
        .iter()
        .position(|s| s.state_id == 3)
        .unwrap();
    assert!(history.topology(&history.states()[index]).is_some());
    history.states_mut()[index]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(-1);
    assert!(history.materialized.states.get().is_none());
    assert!(history.topology(&history.states()[index]).is_none());
    assert_eq!(
        history.check_transitions(),
        Err(AsmTransitionError::PreviousState)
    );
}

fn assert_identity_query(history: &mut AsmHistory, requested: &std::collections::HashSet<i64>) {
    let expected_revisions: Vec<_> = history
        .states()
        .iter()
        .enumerate()
        .flat_map(|(index, state)| {
            history
                .entity_versions(state)
                .into_iter()
                .filter(|version| requested.contains(&version.record_ref))
                .map(move |version| (index, version))
        })
        .collect();
    let mut expected_entities = Vec::new();
    let mut all_states_available = true;
    for (index, state) in history.states().iter().enumerate() {
        let Some(topology) = history.topology(state) else {
            all_states_available = false;
            continue;
        };
        let families: [(AsmHistoricalEntityKind, &[i64]); 12] = [
            (AsmHistoricalEntityKind::Body, &topology.bodies),
            (AsmHistoricalEntityKind::Region, &topology.regions),
            (AsmHistoricalEntityKind::Shell, &topology.shells),
            (AsmHistoricalEntityKind::Face, &topology.faces),
            (AsmHistoricalEntityKind::Loop, &topology.loops),
            (AsmHistoricalEntityKind::Coedge, &topology.coedges),
            (AsmHistoricalEntityKind::Edge, &topology.edges),
            (AsmHistoricalEntityKind::Vertex, &topology.vertices),
            (AsmHistoricalEntityKind::Point, &topology.points),
            (AsmHistoricalEntityKind::Surface, &topology.surfaces),
            (AsmHistoricalEntityKind::Curve, &topology.curves),
            (AsmHistoricalEntityKind::Pcurve, &topology.pcurves),
        ];
        for (kind, members) in families {
            expected_entities.extend(
                members
                    .iter()
                    .filter(|slot| requested.contains(slot))
                    .map(|&slot| (index, slot, kind)),
            );
        }
    }
    expected_entities.sort_by_key(|(index, slot, _)| (*index, *slot));
    expected_entities.dedup();
    history.release_topologies();
    let query = history.identity_memberships(requested, requested);
    assert_eq!(query.revisions, expected_revisions);
    assert_eq!(query.entities, expected_entities);
    assert_eq!(query.all_states_available, all_states_available);
    assert!(history.materialized.states.get().is_none());
}

#[test]
fn identity_query_matches_reconstruction_in_both_scopes() {
    let mut history = chain();
    for _ in 0..2 {
        for requested in [
            std::collections::HashSet::from([1, 2, 3, 10, 20, 22, 33, 110, 120]),
            std::collections::HashSet::from([2, 22, 120]),
            std::collections::HashSet::new(),
        ] {
            assert_identity_query(&mut history, &requested);
        }
        crate::history::discard_projection_caches(std::slice::from_mut(&mut history));
    }
}

#[test]
fn retained_scope_cylinder_only_state_retains_topology_and_memberships() {
    let cylinder_topology = AsmHistoricalTopology {
        surfaces: vec![10, 10],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 10,
            origin: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 2.5,
        }],
        ..Default::default()
    };
    let versions = BTreeMap::from([(10, 100)]);
    let snapshot = AsmStateSnapshot {
        state_id: 0,
        topology: &cylinder_topology,
        versions: &versions,
    };
    let transition = AsmHistoricalTransition::between(None, snapshot).unwrap();
    let state = AsmDeltaState {
        id: "state-0".into(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 0,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        transition: Some(transition),
    };
    let mut history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        binding_budget_exceeded: false,
        states: vec![state],
        topology_scope: AsmTopologyScope::Retained,
        materialized: crate::history_records::MaterializedTopologies::default(),
    };

    assert_eq!(history.check_transitions(), Ok(()));

    let state_ref = &history.states()[0];
    let top = history.topology(state_ref);
    assert!(top.is_some());
    let top = top.unwrap();
    assert_eq!(top.surface_cylinders.len(), 1);
    assert_eq!(top.surface_cylinders[0].surface, 10);
    assert_eq!(top.surface_cylinders[0].radius, 2.5);
    assert!(top.surface_planes.is_empty());
    assert!(top.face_surfaces.is_empty());
    assert!(top.face_loops.is_empty());
    assert!(top.loop_coedges.is_empty());
    assert!(top.coedge_topology.is_empty());
    assert!(top.edge_curves.is_empty());
    assert!(top.coedge_pcurves.is_empty());
    assert!(top.curve_axes.is_empty());

    let requested = std::collections::HashSet::from([10, 100]);
    let query = history.identity_memberships(&requested, &requested);
    assert!(query.all_states_available);
    assert_eq!(
        query.entities,
        vec![(0, 10, AsmHistoricalEntityKind::Surface)]
    );
    assert_eq!(
        query.revisions,
        vec![(
            0,
            AsmEntityVersion {
                entity_ref: 10,
                record_ref: 100,
            }
        )]
    );

    assert_identity_query(&mut history, &requested);
}
