// SPDX-License-Identifier: Apache-2.0
//! Edge-group candidate derivations over complete operand groups.

use super::{group, identity, recipe_edge_operand};
use crate::design::edge_resolve::{
    changed_reference_edge_group_candidates, deleted_reference_edge_group_candidates,
    partition_unique_incomplete_edge_group, resolved_hem_edge_group,
    scope_partition_edge_group_candidates, transition_complete_edge_group_candidates,
    unique_deleted_reference_assignment, unique_edge_group_assignment, EdgeGroupMember,
};
use crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext;
use cadmpeg_ir::features::{EdgeSelection, FeatureId};

fn transition_complete_group() -> (
    [crate::records::topology::edge_identity::DesignEdgeOperand; 3],
    [crate::records::topology::edge_identity::DesignEdgeIdentityOperand; 3],
) {
    let operands = [
        recipe_edge_operand(10, &[], &[101, 102]),
        recipe_edge_operand(11, &[], &[102, 103, 104]),
        recipe_edge_operand(12, &[], &[104, 105]),
    ];
    let identities = [10, 11, 12].map(|index| {
        identity(
            index,
            &[
                (101, 0.35),
                (102, 0.35),
                (103, 0.35),
                (104, 0.35),
                (105, 0.35),
            ],
        )
    });
    (operands, identities)
}

#[test]
fn transition_complete_group_selects_five_edges_covered_by_three_members() {
    let (operands, mut identities) = transition_complete_group();
    identities[1].transition_edge_candidates.reverse();
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        Some(vec![101, 102, 103, 104, 105])
    );
}

#[test]
fn transition_complete_group_refuses_an_uncovered_edge() {
    let (mut operands, identities) = transition_complete_group();
    operands[2].deleted_boundary_edge_slots = vec![104];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

#[test]
fn transition_complete_single_member_admits_radius_qualified_subset() {
    let operands = [recipe_edge_operand(10, &[], &[974, 977])];
    let identities = [identity(10, &[(977, 2.0)])];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            2.0
        ),
        Some(vec![977])
    );
}

#[test]
fn transition_complete_single_member_refuses_competing_radius_evidence() {
    let operands = [recipe_edge_operand(10, &[], &[974, 977])];
    let identities = [identity(10, &[(977, 2.0), (974, 3.0)])];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            2.0
        ),
        None
    );
}

#[test]
fn transition_complete_group_refuses_different_identity_transition_sets() {
    let (operands, mut identities) = transition_complete_group();
    identities[1].transition_edge_candidates = vec![101, 102, 103, 104];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

#[test]
fn transition_complete_group_refuses_a_blend_radius_mismatch() {
    let (operands, _) = transition_complete_group();
    let identities = [10, 11, 12].map(|index| {
        identity(
            index,
            &[
                (101, 0.35),
                (102, 0.35),
                (103, 0.35),
                (104, 0.35),
                (105, 0.4),
            ],
        )
    });
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

#[test]
fn transition_complete_group_refuses_exact_identity_slots() {
    let (operands, mut identities) = transition_complete_group();
    identities[1].resolved_edge_slot = Some(103);
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
    identities[1].resolved_edge_slot = None;
    identities[1].resolved_edge_slots = vec![103, 104];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

fn changed_reference_context(edge: i64) -> DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0,
        "result_faces": [],
        "result_shared_edge_slots": [],
        "preceding_faces": [],
        "shared_edge_slots": [],
        "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": [edge],
    }))
    .unwrap()
}

/// A Hem operand whose first reference changes no edge and whose three
/// support references each change one boundary edge.
fn hem_operand() -> crate::records::topology::edge_identity::DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[63, 106, 110, 167], &[]);
    operand.recipe_reference_contexts = [None, Some(167), Some(106), Some(110)]
        .into_iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            serde_json::from_value(serde_json::json!({
                "reference_ordinal": ordinal,
                "result_faces": if edge.is_some() { vec!["f3d:model:face#1"] } else { Vec::new() },
                "result_shared_edge_slots": [],
                "preceding_faces": [],
                "preceding_support_face_slots": if edge.is_some() { vec![2] } else { Vec::new() },
                "shared_edge_slots": [],
                "changed_shared_edge_slots": [],
                "changed_reference_edge_slots": edge.into_iter().collect::<Vec<_>>(),
            }))
            .unwrap()
        })
        .collect();
    operand
}

#[test]
fn hem_group_selects_the_changed_edge_outside_the_support_boundaries() {
    let selection_group = group(2, 10);
    let operand = hem_operand();
    let feature_id = FeatureId::mint("f3d:model:feature#hem").unwrap();
    let resolve = |previous_state_id| {
        crate::test_support::with_decode_context(|ctx| {
            resolved_hem_edge_group(
                &selection_group,
                std::slice::from_ref(&selection_group),
                std::slice::from_ref(&operand),
                &[],
                previous_state_id,
                &feature_id,
                ctx,
            )
        })
        .unwrap()
    };
    let prefix = crate::ids::feature_input_prefix(&feature_id, 7);
    assert_eq!(
        resolve(Some(7)),
        EdgeSelection::historical(
            crate::ids::feature_input_topology_id(&feature_id, 7),
            vec![crate::ids::history_input_edge_id(&prefix, 63)],
            selection_group.id.clone(),
        )
        .unwrap()
    );
    // Without a prior history state the transition edge has no input state.
    assert!(matches!(resolve(None), EdgeSelection::Native(_)));
}

#[test]
fn scope_partition_assigns_the_unreserved_deleted_edge_to_the_incomplete_group() {
    let deleted = [17, 18];
    let mut source_operand = recipe_edge_operand(10, &[], &deleted);
    source_operand.resolved_edge_slot = Some(17);
    let target_operand = recipe_edge_operand(12, &[], &deleted);
    let scope_groups = [group(2, 10), group(3, 12)];
    let operands = [source_operand, target_operand];
    assert_eq!(
        scope_partition_edge_group_candidates(
            &scope_groups[1],
            &scope_groups,
            &operands,
            scope_groups[1].members(),
        ),
        Some(vec![18])
    );
    let groups = [
        vec![EdgeGroupMember {
            identity: 10,
            resolved_edge: Some(17),
            deleted_boundary_edges: &deleted,
        }],
        vec![EdgeGroupMember {
            identity: 12,
            resolved_edge: None,
            deleted_boundary_edges: &deleted,
        }],
    ];
    assert_eq!(
        partition_unique_incomplete_edge_group(1, &groups),
        Some(vec![18])
    );
}

#[test]
fn reference_assignments_pair_each_operand_with_its_changed_deleted_edge() {
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.recipe_reference_contexts = vec![changed_reference_context(17)];
    let mut second = recipe_edge_operand(11, &[], &[18]);
    second.recipe_reference_contexts = vec![changed_reference_context(18)];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            deleted_reference_edge_group_candidates(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        assert_eq!(
            changed_reference_edge_group_candidates(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        assert_eq!(
            unique_deleted_reference_assignment(&[vec![17], vec![18]], &[vec![17], vec![18]], ctx)
                .unwrap(),
            Some(vec![17, 18])
        );
    });
}

#[test]
fn unique_group_assignment_keeps_each_resolved_member_edge() {
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.resolved_edge_slot = Some(18);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            unique_edge_group_assignment(&[&first, &second], ctx)
        })
        .unwrap(),
        Some(vec![17, 18])
    );
}
