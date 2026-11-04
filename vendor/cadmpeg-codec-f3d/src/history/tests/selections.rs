// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_core::decode::u64_from_index;

use crate::history::active_brep_face_matches_source;
use crate::history::admit_history_binding;
use crate::history::archived_active_record_count;
use crate::history::bind_snapshot_revision_ids;
use crate::history::body_revision_without_topology_change;
use crate::history::combine_recipe_family_tool_slots;
use crate::history::grouped_reference_face_candidate;
use crate::history::historical_body_slot;
use crate::history::historical_record_archive;
use crate::history::insert_only_active_record_count;
use crate::history::materialize_record_table;
use crate::history::pattern_combine_tool_slots;
use crate::history::profile_face_group_cardinality_candidates;
use crate::history::selection::bind_edge_identity_history;
use crate::history::selection::bind_hole_selection_history;
use crate::history::selection::complete_compact_edge_treatment_deletions;
use crate::history::selection::entity_selection_face_candidates;
use crate::history::singleton_body_revision_across_state_chain;
use crate::history::singleton_revised_input_body_across_state_chain;
use crate::history::TopologyStableBodyRevision;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalOptionalCarrierBinding, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistoricalTransition, AsmHistoryRecord,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use std::collections::{HashMap, HashSet};

#[test]
fn entity_selection_face_proofs_preserve_history_namespaces() {
    let state = |parent: &str, state_id, topology| {
        (
            AsmDeltaState {
                id: format!("{parent}-state-{state_id}"),
                parent: parent.into(),
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
    let history = |id: &str, state| {
        crate::test_support::history::history(id.into(), 0, None, false, vec![state])
    };
    let unrelated = history(
        "unrelated",
        state(
            "unrelated",
            1,
            AsmHistoricalTopology {
                points: vec![18044],
                ..AsmHistoricalTopology::default()
            },
        ),
    );
    let selected = history(
        "selected",
        state(
            "selected",
            2,
            AsmHistoricalTopology {
                faces: vec![30],
                loops: vec![20],
                coedges: vec![10],
                pcurves: vec![18044],
                face_loops: vec![AsmHistoricalRelation {
                    owner_ref: 30,
                    member_refs: vec![20],
                }],
                loop_coedges: vec![AsmHistoricalRelation {
                    owner_ref: 20,
                    member_refs: vec![10],
                }],
                coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
                    entity: 10,
                    carrier: Some(18044),
                }],
                ..AsmHistoricalTopology::default()
            },
        ),
    );

    assert_eq!(
        entity_selection_face_candidates(18044, &[unrelated, selected]),
        [
            crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
                history_id: "selected".into(),
                historical: crate::records::topology::fillet::HistoricalBinding {
                    kind: AsmHistoricalEntityKind::Pcurve,
                    entity_ref: 18044,
                    state_ids: vec![2],
                },
                face_slot: 30,
            }
        ]
    );
}

#[test]
fn hole_face_selection_history_binds_the_unique_persistent_face() {
    let state = (
        AsmDeltaState {
            id: "selected-state-2".into(),
            parent: "selected".into(),
            byte_offset: 0,
            state_id: 2,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 2,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(AsmHistoricalTopology {
            faces: vec![30],
            loops: vec![20],
            coedges: vec![10],
            pcurves: vec![18044],
            face_loops: vec![AsmHistoricalRelation {
                owner_ref: 30,
                member_refs: vec![20],
            }],
            loop_coedges: vec![AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![10],
            }],
            coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
                entity: 10,
                carrier: Some(18044),
            }],
            ..AsmHistoricalTopology::default()
        }),
        Vec::new(),
    );
    let history =
        crate::test_support::history::history("selected".into(), 0, None, false, vec![state]);
    let face_selection = crate::records::feature::hole::DesignHoleFaceSelection {
        record_index: 100,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("333".to_owned()).unwrap(),
        asset_id: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 0,
        context_id: "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 0,
        identity_record_index: 103,
        identity_record_offset: 0,
        primary_identity: 18044,
        primary_identity_offset: 0,
        secondary: None,
        historical_face_candidates: Vec::new(),
        next_record_index: 104,
        next_byte_offset: 0,
    };
    let construction = crate::records::feature::hole::DesignHoleConstruction {
        point_record_index: 55,
        point_record_byte_offset: 0,
        position: crate::test_support::reals([0.0; 3]),
        position_offset: 0,
        direction: crate::test_support::reals([0.0, 0.0, 1.0]),
        direction_offset: 0,
        point_parameters: crate::test_support::reals([0.0; 2]),
        point_parameter_offsets: [0, 0],
        reference_type: 0,
        reference_type_offset: 0,
        tangent_point_data: None,
        input_records: vec![crate::records::identity::Located {
            value: 55,
            offset: 0,
        }],
        face_selection: Some(face_selection),
        locations: Vec::new(),
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:scope#42",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        42,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.point = Some(construction);
    }

    bind_hole_selection_history(std::slice::from_mut(&mut scope), &[history]);

    assert_eq!(
        scope
            .hole_construction()
            .and_then(|construction| construction.face_selection.as_ref())
            .map(|selection| selection.historical_face_candidates.as_slice()),
        Some(
            &[
                crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
                    history_id: "selected".into(),
                    historical: crate::records::topology::fillet::HistoricalBinding {
                        kind: AsmHistoricalEntityKind::Pcurve,
                        entity_ref: 18044,
                        state_ids: vec![2],
                    },
                    face_slot: 30,
                }
            ][..]
        )
    );
}

#[test]
fn compact_edge_treatment_deletions_require_exact_cardinality() {
    assert_eq!(
        complete_compact_edge_treatment_deletions(true, Some(2), &[17, 19]),
        [17, 19]
    );
    assert!(complete_compact_edge_treatment_deletions(true, Some(2), &[17, 18, 19]).is_empty());
    assert!(complete_compact_edge_treatment_deletions(false, Some(2), &[17, 19]).is_empty());
    assert!(complete_compact_edge_treatment_deletions(true, None, &[17, 19]).is_empty());
}

#[test]
fn compact_transition_fallback_is_scoped_to_each_operand_group() {
    let scope: crate::records::feature::scope::DesignParameterScope =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope",
            "byte_offset": 0,
            "class_tag": "300",
            "record_index": 1,
            "frame_length": 200,
            "kind": "Fillet",
            "kind_offset": 32,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 128,
            "history_state_id": 8,
            "history_state_id_offset": 24,
            "previous_history_state_id": 7,
            "previous_history_state_id_offset": 158,
            "reference_count_offset": 9,
            "reference_members": [2],
            "reference_member_offsets": [14],
            "paired_class_tag": "261",
            "paired_byte_offset": 200
        }))
        .expect("Fillet scope");
    let identity = |group_record_index, group_member_ordinal, record_index| {
        serde_json::from_value::<DesignEdgeIdentityOperand>(serde_json::json!({
            "id": format!("f3d:test:identity#{record_index}"),
            "scope_record_index": 1,
            "group_record_index": group_record_index,
            "group_member_ordinal": group_member_ordinal,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "277",
            "compact_layout": true,
            "local_id": record_index,
            "local_id_offset": 23,
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            "asset_id_offset": 41,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
            "context_id_offset": 117
        }))
        .expect("edge identity")
    };
    let mut operands = vec![identity(2, 0, 10), identity(2, 1, 11), identity(3, 0, 12)];
    let state = |state_id, topology, next_ref| {
        (
            AsmDeltaState {
                id: format!("history:state#{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
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
    let previous = state(
        7,
        AsmHistoricalTopology {
            edges: vec![17, 19],
            ..AsmHistoricalTopology::default()
        },
        None,
    );
    let current = state(8, AsmHistoricalTopology::default(), Some(7));
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![current, previous],
    );

    bind_edge_identity_history(
        &mut operands,
        &[],
        std::slice::from_ref(&scope),
        crate::history::BindingHistories::Shared(std::slice::from_ref(&history)),
        &HashMap::from([(scope.id.clone(), history.id.clone())]),
        &HashSet::new(),
    );

    assert_eq!(operands[0].transition_edge_candidates, [17, 19]);
    assert_eq!(operands[1].transition_edge_candidates, [17, 19]);
    assert!(operands[2].transition_edge_candidates.is_empty());

    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            state(8, AsmHistoricalTopology::default(), Some(7)),
            state(
                7,
                AsmHistoricalTopology {
                    edges: vec![17],
                    ..AsmHistoricalTopology::default()
                },
                None,
            ),
        ],
    );
    let mut lineage_operand = operands[2].clone();
    lineage_operand.local_id = 17;
    bind_edge_identity_history(
        std::slice::from_mut(&mut lineage_operand),
        &[],
        std::slice::from_ref(&scope),
        crate::history::BindingHistories::Shared(std::slice::from_ref(&history)),
        &HashMap::from([(scope.id.clone(), history.id.clone())]),
        &HashSet::new(),
    );
    assert_eq!(lineage_operand.resolved_edge_slot, Some(17));
    assert!(lineage_operand.historical.is_some());
    bind_edge_identity_history(
        std::slice::from_mut(&mut lineage_operand),
        &[],
        std::slice::from_ref(&scope),
        crate::history::BindingHistories::Shared(std::slice::from_ref(&history)),
        &HashMap::from([(scope.id.clone(), history.id.clone())]),
        &HashSet::from([scope.id.clone()]),
    );
    assert!(lineage_operand.transition_edge_candidates.is_empty());
    assert!(lineage_operand.resolved_edge_slot.is_none());
    assert!(lineage_operand.historical.is_none());
}

#[test]
fn body_selection_proofs_distinguish_stable_and_topology_changing_operations() {
    let state = |state_id, bodies: &[i64], versions: &[(i64, i64)]| {
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
            Some(AsmHistoricalTopology {
                bodies: bodies.to_vec(),
                ..Default::default()
            }),
            versions
                .iter()
                .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect(),
        )
    };
    let history =
        |states| crate::test_support::history::history("history".into(), 0, None, false, states);
    let stable = history(vec![state(11, &[7], &[(7, 2)]), state(10, &[7], &[(7, 1)])]);
    assert_eq!(
        body_revision_without_topology_change(&stable.states()[0]),
        Some(TopologyStableBodyRevision::Revised(7))
    );

    let mut carrier_new = state(11, &[7], &[(7, 2), (31, 2), (32, 1), (34, 2)]);
    let mut carrier_old = state(10, &[7], &[(7, 1), (31, 1), (33, 1), (34, 1)]);
    carrier_new.1.as_mut().unwrap().points = vec![31];
    carrier_new.1.as_mut().unwrap().surfaces = vec![32];
    carrier_new.1.as_mut().unwrap().pcurves = vec![34];
    carrier_old.1.as_mut().unwrap().points = vec![31];
    carrier_old.1.as_mut().unwrap().curves = vec![33];
    carrier_old.1.as_mut().unwrap().pcurves = vec![34];
    let carrier_revisions = history(vec![carrier_new.clone(), carrier_old.clone()]);
    assert_eq!(
        body_revision_without_topology_change(&carrier_revisions.states()[0]),
        Some(TopologyStableBodyRevision::Revised(7))
    );

    carrier_new.0.state_id = 12;
    carrier_new.0.node_index = 12;
    let mut intermediate = carrier_old.clone();
    intermediate.0.state_id = 11;
    intermediate.0.node_index = 11;
    intermediate.2[0].record_ref = 2;
    carrier_new.2[0].record_ref = 3;
    let chain = history(vec![carrier_new.clone(), intermediate, carrier_old.clone()]);
    let states = chain
        .states()
        .iter()
        .map(|state| (state.state_id, Some(state)))
        .collect();
    assert_eq!(
        singleton_body_revision_across_state_chain(&chain, &chain.states()[0], 10, &states),
        Some(7)
    );

    let split = history(vec![
        state(21, &[7, 8, 9], &[(7, 2), (8, 1), (9, 1)]),
        state(20, &[7, 8], &[(7, 1), (8, 1)]),
    ]);
    let states = split
        .states()
        .iter()
        .map(|state| (state.state_id, Some(state)))
        .collect();
    assert_eq!(
        singleton_revised_input_body_across_state_chain(&split, &split.states()[0], 20, &states),
        Some(7)
    );
    let ambiguous = history(vec![
        state(21, &[7, 8, 9], &[(7, 2), (8, 2), (9, 1)]),
        state(20, &[7, 8], &[(7, 1), (8, 1)]),
    ]);
    let states = ambiguous
        .states()
        .iter()
        .map(|state| (state.state_id, Some(state)))
        .collect();
    assert_eq!(
        singleton_revised_input_body_across_state_chain(
            &ambiguous,
            &ambiguous.states()[0],
            20,
            &states
        ),
        None
    );

    carrier_new.0.state_id = 11;
    carrier_new.0.node_index = 11;
    carrier_new.1.as_mut().unwrap().faces = vec![19];
    carrier_old.1.as_mut().unwrap().faces = vec![19];
    carrier_new.2.push(AsmEntityVersion {
        entity_ref: 19,
        record_ref: 2,
    });
    carrier_old.2.push(AsmEntityVersion {
        entity_ref: 19,
        record_ref: 1,
    });
    let topology_changing = history(vec![carrier_new, carrier_old]);
    assert_eq!(
        body_revision_without_topology_change(&topology_changing.states()[0]),
        None
    );
}

#[test]
fn pattern_combine_tool_set_requires_target_membership_and_exact_cardinality() {
    let bodies = [2, 4, 5, 6, 7].into_iter().collect();

    {
        assert_eq!(
            pattern_combine_tool_slots(&bodies, 4, 4),
            Some(vec![2, 5, 6, 7])
        );
        assert_eq!(pattern_combine_tool_slots(&bodies, 3, 5), None);
        assert_eq!(pattern_combine_tool_slots(&bodies, 4, 3), None);
        assert_eq!(pattern_combine_tool_slots(&bodies, 4, 5), None);
    };
    assert_eq!(
        historical_body_slot("f3d:history-input:body#80:escaped-feature:35:2"),
        Some(2)
    );
    assert_eq!(historical_body_slot("f3d:brep:entity#2"), None);
}

#[test]
fn combine_recipe_family_proves_unordered_generated_tools() {
    use crate::records::{
        recipes::{ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector},
        topology::{
            body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
            body_recipe::DesignOperandOwner,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let recipe = |record_index, design_id: &str, selector| ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{record_index}"),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: design_id.into(),
                offset: 0,
            },
            selector: Some(ConstructionRecipeSelector {
                value: selector,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let recipes = [
        recipe(101, "exact", 6),
        recipe(102, "family", 3),
        recipe(103, "family", 1),
        recipe(104, "family", 2),
    ];
    let operand = |record_index,
                   recipe: &ConstructionRecipe,
                   body: Option<i64>,
                   candidates: &[i64]| {
        DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: format!("{stream}:design-body-recipe-operand#{record_index}"),
                scope_record_index: 10,
                owner: DesignOperandOwner::ScopeReference {
                    scope_reference_ordinal: record_index,
                },
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("389".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 56,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 132,
                selector_tail: None,

                references: vec![DesignBodyRecipeReference {
                    design_reference: if recipe
                        .design
                        .as_ref()
                        .map(|design| design.id.value.as_str())
                        == Some("family")
                    {
                        413
                    } else {
                        409
                    },
                    design_reference_offset: 25,
                    form: 3,
                    form_offset: 33,
                    candidate_faces: Vec::new(),
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: candidates.to_vec(),
                }],
                nested_record_index: u64::from(record_index + 3),
                nested_record_index_offset: 38,
                recipe_id: recipe.id.clone(),
                resolved_face_slot: None,
                resolved_body_state_id: body.map(|_| 317),
                resolved_body_slot: body,
                resolved_body_face_slots: Vec::new(),
                next_record_index: record_index + 4,
                next_byte_offset: 256,
            },
        )
        .unwrap()
    };
    let family = [6, 7, 8];
    let mut operands = vec![
        operand(1, &recipes[0], Some(5), &[]),
        operand(2, &recipes[1], None, &[]),
        operand(3, &recipes[2], None, &family),
        operand(4, &recipes[3], None, &[]),
    ];

    {
        assert_eq!(
            combine_recipe_family_tool_slots(
                (stream, 10),
                &[1, 2, 3, 4],
                317,
                1,
                &operands,
                &recipes
            ),
            Some(vec![5, 6, 7, 8])
        )
    };

    *operands[1]
        .reference_bindings_mut()
        .next()
        .unwrap()
        .preceding_body_slots = vec![6, 7, 9];
    assert!(combine_recipe_family_tool_slots(
        (stream, 10),
        &[1, 2, 3, 4],
        317,
        1,
        &operands,
        &recipes
    )
    .is_none());

    operands[1]
        .reference_bindings_mut()
        .next()
        .unwrap()
        .preceding_body_slots
        .clear();
    let mut duplicate_selector = recipes.clone();
    duplicate_selector[1].design.as_mut().unwrap().selector =
        duplicate_selector[2].design.as_ref().unwrap().selector;
    assert!(combine_recipe_family_tool_slots(
        (stream, 10),
        &[1, 2, 3, 4],
        317,
        1,
        &operands,
        &duplicate_selector
    )
    .is_none());
}

fn combine_external_identity(
    occurrence_reference: u64,
) -> crate::records::feature::combine::DesignCombineExternalBodyIdentity {
    crate::records::feature::combine::DesignCombineExternalBodyIdentityWire {
        selector_asset_id: "11111111-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .expect("GUID"),
        selector_asset_id_offset: 44,
        selector_context_id: "22222222-2222-4222-8222-222222222222"
            .to_owned()
            .try_into()
            .expect("GUID"),
        selector_context_id_offset: 120,
        occurrence_reference,
        occurrence_reference_offset: 205,
        external_body_reference: 700,
        external_body_reference_offset: 220,
        external_segment: 2,
        external_segment_offset: 229,
        external_asset_id: "11111111-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .expect("GUID"),
        external_asset_id_offset: 237,
        external_link_name: "component-body-link".into(),
        external_link_name_offset: 314,
        external_property_key: None,
        external_property_key_offset: None,
        external_version_urn: None,
        external_version_urn_offset: None,
        tail_values: [0, 0],
        tail_value_offsets: [359, 371],
    }
    .try_into()
    .unwrap()
}

fn combine_external_scope() -> crate::records::feature::scope::DesignParameterScope {
    let identity = combine_external_identity;
    let tool = |record_index, occurrence_reference| {
        crate::records::feature::combine::DesignCombineBodySelection {
            record_index,
            external_identity: Some(identity(occurrence_reference)),
            body_node: None,
        }
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
        scope.payload_mut()
    {
        *slot = Some(crate::records::feature::combine::DesignCombineOperation {
            form: crate::records::feature::combine::DesignCombineForm::ExtendedReference,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 11,
            target_body_node: None,
            tools: crate::records::feature::combine::DesignCombineTools {
                first: tool(12, 500),
                additional: vec![tool(13, 501)],
            },
        });
    }
    scope
}

#[test]
fn combine_external_tools_retain_complete_occurrence_local_identities() {
    use cadmpeg_ir::features::BodySelection;

    let tool = |record_index, occurrence_reference| {
        crate::records::feature::combine::DesignCombineBodySelection {
            record_index,
            external_identity: Some(combine_external_identity(occurrence_reference)),
            body_node: None,
        }
    };
    let mut scope = combine_external_scope();
    let BodySelection::Local { bodies, native } =
        super::super::combine_external_local_tools(&scope).expect("complete local tool identity")
    else {
        panic!("local body selection");
    };
    assert_eq!(bodies.len(), 2);
    assert_ne!(bodies[0], bodies[1]);
    assert_eq!(native.as_str(), scope.id);

    scope
        .combine_operation_mut()
        .expect("Combine operation")
        .tools
        .additional[0] = tool(13, 500);
    assert!(super::super::combine_external_local_tools(&scope).is_none());
}

#[test]
fn active_brep_face_namespace_accepts_default_or_matching_named_source() {
    use cadmpeg_ir::ids::FaceId;

    assert!(active_brep_face_matches_source(
        &FaceId::mint("f3d:brep:entity#17").expect("identity grammar"),
        "history"
    ));
    assert!(active_brep_face_matches_source(
        &FaceId::mint("f3d:brep/history/brep:entity#17").expect("identity grammar"),
        "history"
    ));
    assert!(!active_brep_face_matches_source(
        &FaceId::mint("f3d:brep/other/brep:entity#17").expect("identity grammar"),
        "history"
    ));
}

#[test]
fn historical_transition_separates_membership_and_revision_changes() {
    let state = |state_id, versions: &[(i64, i64)], topology| {
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
            versions
                .iter()
                .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect(),
        )
    };
    let previous = state(
        10,
        &[(1, 10), (4, 40), (8, 80)],
        AsmHistoricalTopology {
            bodies: vec![1],
            faces: vec![4],
            edges: vec![8],
            ..AsmHistoricalTopology::default()
        },
    );
    let current = state(
        11,
        &[(1, 11), (2, 2), (4, 40), (7, 70)],
        AsmHistoricalTopology {
            bodies: vec![1, 2],
            faces: vec![4],
            edges: vec![7],
            ..AsmHistoricalTopology::default()
        },
    );

    let versions = |state: &crate::test_support::history::State| {
        state
            .2
            .iter()
            .map(|row| (row.entity_ref, row.record_ref))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let old_versions = versions(&previous);
    let new_versions = versions(&current);
    let transition = AsmHistoricalTransition::between(
        Some(crate::history_records::AsmStateSnapshot {
            state_id: previous.0.state_id,
            topology: previous.1.as_ref().unwrap(),
            versions: &old_versions,
        }),
        crate::history_records::AsmStateSnapshot {
            state_id: current.0.state_id,
            topology: current.1.as_ref().unwrap(),
            versions: &new_versions,
        },
    )
    .unwrap();
    assert_eq!(transition.previous_state_id, Some(10));
    assert_eq!(transition.topology.bodies.inserted, [2]);
    assert_eq!(transition.topology.bodies.updated, [1]);
    assert!(transition.topology.faces.updated.is_empty());
    assert_eq!(transition.topology.edges.inserted, [7]);
    assert_eq!(transition.topology.edges.deleted, [8]);
    assert_eq!(transition.records.updated, [1]);
}

#[test]
fn snapshot_ordinals_bind_the_sorted_revision_interval() {
    let history_id = "history".to_string();
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let mut state = AsmDeltaState {
        id: state_id.clone(),
        parent: history_id,
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: [7, 5, 6]
                .into_iter()
                .enumerate()
                .map(|(index, old_ref)| AsmEntityChange {
                    id: format!("change-{index}"),
                    parent: board_id.clone(),
                    byte_offset: u64_from_index(index),
                    kind: AsmEntityChangeKind::Update {
                        old: old_ref,
                        new: i64::try_from(index).expect("fixture value fits i64"),
                    },
                })
                .collect(),
        }],
        records: (0..3)
            .map(|index| AsmHistoryRecord {
                id: format!("record-{index}"),
                parent: state_id.clone(),
                revision_id: None,
                byte_offset: index,
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index,
                    name: "edge".into(),
                    entity_references: Vec::new(),
                },
                raw_bytes: vec![0x11].into(),
            })
            .collect(),
        transition: None,
    };

    bind_snapshot_revision_ids(std::slice::from_mut(&mut state)).unwrap();

    assert_eq!(
        state
            .records
            .iter()
            .map(|record| record.revision_id)
            .collect::<Vec<_>>(),
        [Some(5), Some(6), Some(7)]
    );
}

#[test]
fn insert_only_history_uses_the_active_record_table_as_revisions() {
    let state = |node_index, next_ref, inserted: &[i64]| {
        let state_id = format!("state-{node_index}");
        let board_id = format!("board-{node_index}");
        (
            AsmDeltaState {
                id: state_id.clone(),
                parent: "history".into(),
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
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
                    byte_offset: u64::try_from(node_index)
                        .expect("fixture reference is nonnegative"),
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
                    byte_offset: u64::try_from(node_index)
                        .expect("fixture reference is nonnegative"),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: 0,
                        name: "End-of-ASM-History-Section".into(),
                        entity_references: Vec::new(),
                    },
                    raw_bytes: vec![0x11].into(),
                }],
                transition: None,
            },
            None::<AsmHistoricalTopology>,
            Vec::<AsmEntityVersion>::new(),
        )
    };
    let states = vec![
        state(0, Some(1), &[1]).0,
        state(1, Some(2), &[2]).0,
        state(2, None, &[3]).0,
    ];

    assert_eq!(insert_only_active_record_count(&states), Some(4));
    admit_history_binding(
        &cadmpeg_test_support::service_decode_context(),
        &states,
        &mut { u64::MAX },
    )
    .unwrap();
    let active_count = archived_active_record_count(&states)
        .unwrap()
        .or_else(|| insert_only_active_record_count(&states))
        .unwrap();
    let mut archived_ids: Vec<_> = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|r| r.revision_id)
        .collect();
    archived_ids.sort_unstable();
    let mut versions_by_state = vec![Vec::new(); states.len()];
    assert!(crate::history::walk_entity_versions(
        &states,
        i64::try_from(active_count).unwrap(),
        &archived_ids,
        |ordinal, versions| {
            versions_by_state[ordinal] = versions
                .iter()
                .map(|(&entity_ref, &record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect();
        }
    ));

    assert_eq!(
        versions_by_state.iter().map(Vec::len).collect::<Vec<_>>(),
        [4, 3, 2]
    );
    assert_eq!(
        versions_by_state[1],
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
            AsmEntityVersion {
                entity_ref: 3,
                record_ref: 3,
            },
        ]
    );
}

#[test]
fn insert_only_history_rejects_gaps_and_updates() {
    let mut state = AsmDeltaState {
        id: "state".into(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: "state".into(),
            revision_id: None,
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11].into(),
        }],
        transition: None,
    };
    let board = AsmBulletinBoard {
        id: "board".into(),
        parent: state.id.clone(),
        byte_offset: 0,
        owner_ref: 0,
        number: 2,
        changes: vec![
            AsmEntityChange {
                id: "gap-a".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 1 },
            },
            AsmEntityChange {
                id: "gap-b".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 3 },
            },
        ],
    };
    state.bulletin_boards.push(board);
    {
        assert_eq!(insert_only_active_record_count(&[state.clone()]), None)
    };
    state.bulletin_boards[0].changes[1].kind = AsmEntityChangeKind::Update { old: 2, new: 3 };
    assert_eq!(insert_only_active_record_count(&[state]), None);
}

#[test]
fn materialized_record_table_normalizes_revision_references() {
    let mut archived_bytes = vec![0x0d, 4];
    archived_bytes.extend_from_slice(b"edge");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = (
        AsmDeltaState {
            id: state_id.clone(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: 0,
                owner_ref: 0,
                number: 2,
                changes: vec![AsmEntityChange {
                    id: "change".into(),
                    parent: board_id,
                    byte_offset: 0,
                    kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
                }],
            }],
            records: vec![AsmHistoryRecord {
                id: "record".into(),
                parent: state_id,
                revision_id: Some(2),
                byte_offset: 0,
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index: 0,
                    name: "edge".into(),
                    entity_references: vec![2],
                },
                raw_bytes: archived_bytes.clone().into(),
            }],
            transition: None,
        },
        None::<AsmHistoricalTopology>,
        vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
    );
    let active = ["asmheader", "edge"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),

            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let [framed]: [_; 1] = cadmpeg_asm::test_support::sab::frame(
        &archived_bytes,
        0,
        archived_bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("archived record frames")
    .try_into()
    .expect("one archived record");
    let archive = historical_record_archive(
        std::slice::from_ref(&state.0),
        &active,
        HashMap::from([(2, framed)]),
    )
    .expect("complete historical record archive");
    let table = materialize_record_table(
        &state
            .2
            .iter()
            .map(|v| (v.entity_ref, v.record_ref))
            .collect(),
        &archive,
    )
    .expect("complete historical RecordTable");

    assert_eq!(table.len(), 2);
    assert_eq!(table[1].index, 1);
    assert_eq!(&*table[1].tokens, [cadmpeg_asm::sab::Token::Ref(1)]);
}

#[test]
fn qualified_history_marker_remains_an_archived_record() {
    let mut archived_bytes = Vec::new();
    for part in ["End", "of", "ASM", "History"] {
        archived_bytes.extend_from_slice(&[0x0e, u8::try_from(part.len()).unwrap()]);
        archived_bytes.extend_from_slice(part.as_bytes());
    }
    archived_bytes.extend_from_slice(&[0x0d, 7]);
    archived_bytes.extend_from_slice(b"Section");
    archived_bytes.extend_from_slice(&[0x0d, 4]);
    archived_bytes.extend_from_slice(b"body");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = (
        AsmDeltaState {
            id: state_id.clone(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: 0,
                owner_ref: 0,
                number: 2,
                changes: vec![AsmEntityChange {
                    id: "change".into(),
                    parent: board_id,
                    byte_offset: 0,
                    kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
                }],
            }],
            records: vec![AsmHistoryRecord {
                id: "record".into(),
                parent: state_id,
                revision_id: Some(2),
                byte_offset: 0,
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index: 0,
                    name: "End-of-ASM-History-Section".into(),
                    entity_references: vec![2],
                },
                raw_bytes: archived_bytes.clone().into(),
            }],
            transition: None,
        },
        None::<AsmHistoricalTopology>,
        vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
    );
    let active = ["asmheader", "body"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),

            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let [framed]: [_; 1] = cadmpeg_asm::test_support::sab::frame(
        &archived_bytes,
        0,
        archived_bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("archived record frames")
    .try_into()
    .expect("one archived record");
    let archive = historical_record_archive(
        std::slice::from_ref(&state.0),
        &active,
        HashMap::from([(2, framed)]),
    )
    .expect("qualified history marker is an archived record");
    let record = archive.get(&2).expect("marker revision is retained");
    assert_eq!(record.name, "body");
    assert_eq!(record.index, 1);
    assert!(record.tokens.contains(&cadmpeg_asm::sab::Token::Ref(1)));
}

#[test]
fn reverse_history_builds_complete_entity_version_maps() {
    let state = |node_index, previous_ref, next_ref, old_ref, new_ref| {
        let board_id = format!("board-{node_index}");
        (
            AsmDeltaState {
                id: format!("state-{node_index}"),
                parent: "history".into(),
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                state_id: 10 - node_index,
                version_flag: 1,
                state_flag: 0,
                previous_ref,
                next_ref,
                node_index,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: vec![AsmBulletinBoard {
                    id: board_id.clone(),
                    parent: format!("state-{node_index}"),
                    byte_offset: u64::try_from(node_index)
                        .expect("fixture reference is nonnegative"),
                    owner_ref: 0,
                    number: 2,
                    changes: vec![AsmEntityChange {
                        id: format!("change-{node_index}"),
                        parent: board_id,
                        byte_offset: u64::try_from(node_index)
                            .expect("fixture reference is nonnegative"),
                        kind: match (old_ref, new_ref) {
                            (Some(old), Some(new)) => AsmEntityChangeKind::Update { old, new },
                            (None, Some(new)) => AsmEntityChangeKind::Insert { new },
                            (Some(old), None) => AsmEntityChangeKind::Delete { old },
                            (None, None) => unreachable!(),
                        },
                    }],
                }],
                records: Vec::new(),
                transition: None,
            },
            None::<AsmHistoricalTopology>,
            Vec::<AsmEntityVersion>::new(),
        )
    };
    let mut states = vec![
        state(0, None, Some(1), Some(3), Some(1)).0,
        state(1, Some(0), Some(2), Some(4), Some(1)).0,
        state(2, Some(1), Some(3), None, Some(2)).0,
        state(3, Some(2), None, None, Some(1)).0,
    ];
    states[0].records = [3, 4]
        .map(|revision_id| AsmHistoryRecord {
            id: format!("record-{revision_id}"),
            parent: states[0].id.clone(),
            revision_id: Some(revision_id),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: u64::try_from(revision_id).expect("fixture reference is nonnegative") - 3,
                name: "edge".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11].into(),
        })
        .into();

    admit_history_binding(
        &cadmpeg_test_support::service_decode_context(),
        &states,
        &mut { u64::MAX },
    )
    .unwrap();
    let active_count = archived_active_record_count(&states)
        .unwrap()
        .or_else(|| insert_only_active_record_count(&states))
        .unwrap();
    let mut archived_ids: Vec<_> = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|r| r.revision_id)
        .collect();
    archived_ids.sort_unstable();
    let mut versions_by_state = vec![Vec::new(); states.len()];
    assert!(crate::history::walk_entity_versions(
        &states,
        i64::try_from(active_count).unwrap(),
        &archived_ids,
        |ordinal, versions| {
            versions_by_state[ordinal] = versions
                .iter()
                .map(|(&entity_ref, &record_ref)| AsmEntityVersion {
                    entity_ref,
                    record_ref,
                })
                .collect();
        }
    ));

    assert_eq!(
        versions_by_state.iter().map(Vec::len).collect::<Vec<_>>(),
        [3, 3, 3, 2]
    );
    assert_eq!(
        versions_by_state[1],
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 3,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
        ]
    );
    assert_eq!(versions_by_state[2][1].record_ref, 4);
}

#[test]
fn profile_face_group_cardinality_requires_one_changed_surface_family() {
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12, 20],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let changed = [20, 12, 10, 11].into_iter().collect();
    assert_eq!(
        profile_face_group_cardinality_candidates(&topology, &changed, 3),
        Some(vec![10, 11, 12])
    );
    assert_eq!(
        profile_face_group_cardinality_candidates(&topology, &[20].into_iter().collect(), 1),
        Some(vec![20])
    );
    assert_eq!(
        profile_face_group_cardinality_candidates(&topology, &[10, 20].into_iter().collect(), 1),
        None
    );

    let mut ambiguous = topology;
    ambiguous.faces.extend([30, 31, 32]);
    ambiguous
        .face_surfaces
        .extend([30, 31, 32].map(|entity| AsmHistoricalCarrierBinding {
            entity,
            carrier: 300,
        }));
    let changed = [10, 11, 12, 30, 31, 32].into_iter().collect();
    assert_eq!(
        profile_face_group_cardinality_candidates(&ambiguous, &changed, 3),
        None
    );
}

#[test]
fn grouped_face_reference_selects_one_changed_topology_face() {
    use crate::records::topology::face::DesignFaceOperand;

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    for (token, references) in [
        ("1", [10u32, 20].as_slice()),
        ("2", [30u32].as_slice()),
        ("3", [40u32].as_slice()),
        ("4", [50u32].as_slice()),
        ("5", [60u32].as_slice()),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(
            &u32::try_from(references.len())
                .expect("synthetic reference count")
                .to_le_bytes(),
        );
        for reference in references {
            prefix.extend_from_slice(&reference.to_le_bytes());
        }
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let mut operand = serde_json::from_value::<DesignFaceOperand>(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-face-operand#1",
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": 2,
        "byte_offset": 0,
        "class_tag": "277",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#5",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "next_record_index": 6,
        "next_byte_offset": 160
    }))
    .expect("grouped face operand");
    *operand.recipe_prefix_bytes = prefix;
    operand.recipe_references = crate::design::decode::dimension_frames::decode_recipe_references(
        &operand.recipe_prefix_bytes,
        0,
    );
    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        grouped_reference_face_candidate(&operand, &topology, &HashSet::from([10])),
        Some(
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10))
                .expect("identity grammar")
        )
    );
    assert_eq!(
        grouped_reference_face_candidate(&operand, &topology, &HashSet::from([10, 20])),
        None
    );
    let mut trailing = operand;
    trailing.recipe_prefix_bytes.extend_from_slice(&[0; 4]);
    assert_eq!(
        grouped_reference_face_candidate(&trailing, &topology, &HashSet::from([10])),
        None
    );
}

mod extrude_profile;

mod face_support;

mod operand_list_combine {
    use crate::records::feature::combine::{
        DesignCombineBodyNode, DesignCombineBodySelection, DesignCombineForm,
        DesignCombineOperation, DesignCombineTools,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{BodySelection, FeatureId};

    const STREAM: &str = "f3d:Design/BulkStream.dat";

    #[allow(clippy::unnecessary_wraps)]
    fn node(creating_scope_record_index: u32) -> Option<DesignCombineBodyNode> {
        Some(DesignCombineBodyNode {
            record_index: creating_scope_record_index + 1,
            body_entity_suffix: creating_scope_record_index + 2,
            creating_scope_record_index,
        })
    }

    fn scope(record_index: u32, state: i64) -> DesignParameterScope {
        let mut scope = DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#{record_index}"),
            DesignFeatureKind::BaseFeature,
            record_index,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(state);
                draft.previous_history_state_id = Some(state - 1);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    }

    fn topology(bodies: &[i64]) -> crate::history_records::AsmHistoricalTopology {
        crate::history_records::AsmHistoricalTopology {
            bodies: bodies.to_vec(),
            ..Default::default()
        }
    }

    fn history(states: &[&[i64]]) -> crate::history_records::AsmHistory {
        let rows = states
            .iter()
            .enumerate()
            .rev()
            .map(|(id, bodies)| {
                let id = i64::try_from(id).unwrap();
                (
                    crate::history_records::AsmDeltaState {
                        id: format!("f3d:asm-delta-state#{id}"),
                        parent: "f3d:asm-history#1".into(),
                        byte_offset: 0,
                        state_id: id,
                        version_flag: 1,
                        state_flag: 0,
                        previous_ref: None,
                        next_ref: (id > 0).then_some(id - 1),
                        node_index: id,
                        partner_ref: None,
                        owner_ref: 0,
                        bulletin_boards: Vec::new(),
                        records: Vec::new(),
                        transition: None,
                    },
                    Some(topology(bodies)),
                    Vec::new(),
                )
            })
            .collect();
        crate::test_support::history::history("f3d:asm-history#1".into(), 0, None, false, rows)
    }

    fn operation(
        target: Option<DesignCombineBodyNode>,
        tools: &[Option<DesignCombineBodyNode>],
    ) -> DesignCombineOperation {
        let mut selections =
            tools
                .iter()
                .zip(12u32..)
                .map(|(body_node, record_index)| DesignCombineBodySelection {
                    record_index,
                    external_identity: None,
                    body_node: *body_node,
                });
        DesignCombineOperation {
            form: DesignCombineForm::OperandLists,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 21,
            keep_tools: false,
            keep_tools_offset: 29,
            target_record_index: 11,
            target_body_node: target,
            tools: DesignCombineTools {
                first: selections.next().unwrap(),
                additional: selections.collect(),
            },
        }
    }

    fn bind(
        operation: DesignCombineOperation,
        history: &crate::history_records::AsmHistory,
        scopes: &[DesignParameterScope],
    ) -> (BodySelection, BodySelection) {
        let combine_state = i64::try_from(history.states().len()).unwrap() - 1;
        let combine = scope(400, combine_state);
        let mut target = BodySelection::Native(format!("{STREAM}:design-record#11"));
        let mut tools = BodySelection::Native(format!("{STREAM}:design-record#12"));
        super::super::super::bind_operand_list_combine_bodies(
            &mut target,
            &mut tools,
            &combine,
            &operation,
            &FeatureId::mint("test:model:feature#combine").unwrap(),
            combine_state - 1,
            (std::slice::from_ref(history), scopes),
        );
        (target, tools)
    }

    fn input_body_slots(selection: &BodySelection) -> Vec<i64> {
        let ids: Vec<String> = match selection {
            BodySelection::Historical { bodies, .. } => {
                bodies.iter().map(ToString::to_string).collect()
            }
            BodySelection::HistoricalSet { members, .. } => {
                members.iter().map(|row| row.body().to_string()).collect()
            }
            other => panic!("expected a historical selection, got {other:?}"),
        };
        ids.iter()
            .map(|id| crate::history::historical_body_slot(id).unwrap())
            .collect()
    }

    fn creators() -> Vec<DesignParameterScope> {
        vec![scope(100, 1), scope(200, 2), scope(300, 3)]
    }

    fn assert_native(selections: &(BodySelection, BodySelection)) {
        assert!(
            matches!(selections.0, BodySelection::Native(_)),
            "{selections:?}"
        );
        assert!(
            matches!(selections.1, BodySelection::Native(_)),
            "{selections:?}"
        );
    }

    #[test]
    fn slots_come_from_the_creating_scopes_in_tool_order() {
        let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
        let (target, tools) = bind(
            operation(node(200), &[node(300), node(100)]),
            &history,
            &creators(),
        );
        assert_eq!(input_body_slots(&target), [2]);
        assert_eq!(input_body_slots(&tools), [3, 1]);
    }

    #[test]
    fn every_operand_must_resolve_or_both_selections_stay_native() {
        let history = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
        for (target, tools) in [
            (None, vec![node(300), node(100)]),
            (node(200), vec![None, node(100)]),
            (node(200), vec![node(300), None]),
            (node(999), vec![node(300), node(100)]),
            (node(200), vec![node(300), node(999)]),
            (node(200), vec![node(200), node(100)]),
            (node(200), vec![node(300), node(200)]),
            (node(200), vec![node(300), node(300)]),
        ] {
            assert_native(&bind(operation(target, &tools), &history, &creators()));
        }
    }

    #[test]
    fn a_creator_must_insert_one_body_that_reaches_the_input_state() {
        let two_inserted = history(&[&[], &[1], &[1, 2, 3], &[1, 2, 3, 4], &[1]]);
        assert_native(&bind(
            operation(node(100), &[node(200)]),
            &two_inserted,
            &creators(),
        ));

        let removed_before_input = history(&[&[], &[1], &[1, 2], &[1], &[1]]);
        assert_native(&bind(
            operation(node(100), &[node(200)]),
            &removed_before_input,
            &creators(),
        ));

        let mut no_previous_state = creators();
        no_previous_state[1]
            .try_edit(|draft| {
                draft.previous_history_state_id = None;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let (target, tools) = bind(
            operation(node(100), &[node(200)]),
            &history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[1]]),
            &no_previous_state,
        );
        assert_eq!(input_body_slots(&target), [1]);
        assert_eq!(input_body_slots(&tools), [2]);

        let scope_in_other_stream = {
            let mut scopes = creators();
            scopes[1] = DesignParameterScope::empty(
                "f3d:Other/BulkStream.dat:design-parameter-scope#200",
                DesignFeatureKind::BaseFeature,
                200,
            );
            scopes[1]
                .try_edit(|draft| {
                    draft.history_state_id = Some(2);
                    draft.previous_history_state_id = Some(1);
                    draft.layout_fixture_tail();
                })
                .unwrap();
            scopes
        };
        let ordinary = history(&[&[], &[1], &[1, 2], &[1, 2, 3], &[2]]);
        assert_native(&bind(
            operation(node(100), &[node(200)]),
            &ordinary,
            &scope_in_other_stream,
        ));
    }
}
