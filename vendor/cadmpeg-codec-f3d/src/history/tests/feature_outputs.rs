// SPDX-License-Identifier: Apache-2.0

fn active_body() -> cadmpeg_ir::topology::Body {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    body
}

fn base_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(vec![
                BodyId::mint("test:model:body#2").unwrap(),
                BodyId::mint("test:model:body#1").unwrap(),
            ])
            .unwrap(),
        ),
        native_ref: Some("native:scope".into()),
    }
}

fn state(
    state_id: i64,
    node_index: i64,
    next_ref: Option<i64>,
    topology: crate::history_records::AsmHistoricalTopology,
) -> crate::test_support::history::State {
    (
        crate::history_records::AsmDeltaState {
            id: format!("f3d:asm-delta-state#{state_id}"),
            parent: "f3d:asm-history#1".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        Vec::new(),
    )
}

fn scope_between(
    state_id: i64,
    previous_state_id: i64,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{state_id}"),
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn bodies_topology(bodies: &[i64]) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: bodies.to_vec(),
        body_regions: bodies
            .iter()
            .map(|&owner_ref| AsmHistoricalRelation {
                owner_ref,
                member_refs: Vec::new(),
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

fn output_binding_inputs() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    use crate::history_records::AsmHistoricalTopology;
    let mut feature = base_feature();
    let scope = scope_between(1, 0);
    feature.native_ref = Some(scope.id.clone());
    let current_topology = bodies_topology(&[1]);
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), current_topology),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    (feature, scope, history, active_body())
}

#[test]
fn split_outputs_bind_across_complete_transition_chains() {
    use cadmpeg_ir::features::{
        BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, SplitBodyTool,
    };
    let mut feature = base_feature();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Unresolved,
            tool: SplitBodyTool::Faces(FaceSelection::Unresolved),
        }));
    feature.evaluation.set_outputs(Default::default());
    let mut scope = scope_between(3, 0);
    scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Split
                .try_into()
                .unwrap();
        })
        .unwrap();
    feature.native_ref = Some(scope.id.clone());
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(3, 3, Some(2), bodies_topology(&[1])),
            state(2, 2, Some(1), bodies_topology(&[])),
            state(1, 1, Some(0), bodies_topology(&[])),
            state(0, 0, None, bodies_topology(&[])),
        ],
    );
    let body = active_body();
    let mut complete = feature.clone();
    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut complete),
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(complete.evaluation.outputs(), &[body.id.clone()]);
    assert!(!losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.kind()));
    let mut incomplete = history.clone();
    incomplete.states_mut()[1].transition = None;
    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [incomplete],
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert!(feature.evaluation.outputs().is_empty());
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.kind()));
    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history.clone(), history],
        &[body],
        &Default::default(),
    )
    .unwrap();
    assert!(feature.evaluation.outputs().is_empty());
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.kind()));
}

#[test]
fn split_output_chains_require_unique_bound_acyclic_states() {
    let resolves = |history: &crate::history_records::AsmHistory, current, previous| {
        super::super::output_chain::OutputChain::resolve(
            std::slice::from_ref(history),
            current,
            previous,
            None,
        )
        .is_some()
    };
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(3, 3, Some(2), bodies_topology(&[1])),
            state(2, 2, Some(1), bodies_topology(&[])),
            state(1, 1, Some(0), bodies_topology(&[])),
            state(0, 0, None, bodies_topology(&[])),
        ],
    );
    assert!(resolves(&history, 3, 0));
    assert!(!resolves(&history, 3, 3));
    for mutation in 0..5 {
        let mut changed = history.clone();
        match mutation {
            0 => {
                changed.states_mut().remove(1);
            }
            1 => {
                let duplicate = changed.states()[1].clone();
                changed.states_mut().push(duplicate);
            }
            2 => {
                changed.states_mut()[1].transition = None;
            }
            3 => {
                changed.states_mut()[1].next_ref = Some(3);
                changed.states_mut()[1]
                    .transition
                    .as_mut()
                    .unwrap()
                    .previous_state_id = Some(3);
            }
            4 => {
                changed.states_mut()[1].next_ref = Some(0);
            }
            _ => unreachable!(),
        }
        assert!(!resolves(&changed, 3, 0), "{mutation}");
    }
}

#[test]
fn feature_outputs_name_the_bodies_a_state_deletes_from_its_previous_state() {
    use crate::history_records::AsmHistoricalTopology;
    let history = |states| {
        crate::test_support::history::history("f3d:asm-history#1".into(), 0, None, false, states)
    };
    let body = |slot: i64| {
        let mut body = active_body();
        body.id = cadmpeg_ir::ids::BodyId::mint(format!("f3d:brep:entity#{slot}")).unwrap();
        body
    };
    let bodies = [body(1), body(2), body(3)];
    let chain = || {
        history(vec![
            state(3, 3, Some(2), bodies_topology(&[3])),
            state(2, 2, Some(1), bodies_topology(&[1, 3])),
            state(1, 1, Some(0), bodies_topology(&[1, 2])),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ])
    };
    let expected = [(1, vec![1, 2]), (2, vec![2, 3]), (3, vec![1])];
    let feature_scope = |state_id| {
        let mut feature = base_feature();
        let scope = scope_between(state_id, state_id - 1);
        feature.native_ref = Some(scope.id.clone());
        (feature, scope)
    };
    let check = |feature: &cadmpeg_ir::features::Feature,
                 (state_id, expected): &(i64, Vec<i64>)| {
        let outputs = feature
            .evaluation
            .outputs()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .map(|slot| format!("f3d:brep:entity#{slot}"))
            .collect::<Vec<_>>();
        assert_eq!(outputs, expected, "state {state_id}");
    };
    for case in &expected {
        let (mut feature, scope) = feature_scope(case.0);
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut [chain()],
            &bodies,
            &Default::default(),
        )
        .unwrap();
        check(&feature, case);
    }
    let (mut features, scopes): (Vec<_>, Vec<_>) = expected
        .iter()
        .map(|(state_id, _)| feature_scope(*state_id))
        .unzip();
    super::super::bind_feature_outputs(
        &mut features,
        &scopes,
        &mut [chain()],
        &bodies,
        &Default::default(),
    )
    .unwrap();
    for (feature, case) in features.iter().zip(&expected) {
        check(feature, case);
    }
}

#[test]
fn feature_output_binding_preserves_base_feature_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = output_binding_inputs();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs().len(), 1);
    assert_eq!(feature.evaluation.outputs()[0], body.id);
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == "native:scope"));
}

fn native_base_feature() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    let (mut feature, scope, history, body) = output_binding_inputs();
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: "Base Feature".into(),
                parameters: Default::default(),
            },
        ));
    feature.evaluation.set_outputs(Default::default());
    (feature, scope, history, body)
}

#[test]
fn stored_body_scope_uses_unique_complete_history_outputs() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = native_base_feature();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == &scope.id));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
}

#[test]
fn stored_body_scope_refuses_missing_bodies_and_ambiguous_states() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    for refusal in ["missing body", "duplicate state", "ambiguous body"] {
        let (mut feature, scope, history, body) = native_base_feature();
        let mut histories = vec![history];
        let bodies = if refusal == "duplicate state" {
            let (_, _, duplicate, _) = native_base_feature();
            histories.push(duplicate);
            vec![body]
        } else if refusal == "ambiguous body" {
            let mut other = body.clone();
            other.id = cadmpeg_ir::ids::BodyId::mint("test:model:body#1").unwrap();
            vec![body, other]
        } else {
            Vec::new()
        };
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut histories,
            &bodies,
            &Default::default(),
        )
        .unwrap();
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ));
        let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureStoredBodiesUnbound.kind()
        );
    }
}

#[test]
fn stored_body_scope_keeps_proven_empty_history_as_no_op() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, _) = native_base_feature();
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), bodies_topology(&[])),
            state(0, 0, None, bodies_topology(&[])),
        ],
    );
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, .. }
        }) if bodies.is_empty()));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::FeatureStoredBodiesEmpty.kind());
}

#[test]
fn stored_body_scope_does_not_type_box_from_outputs() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, _, _, body) = native_base_feature();
    feature.evaluation.set_outputs(
        cadmpeg_ir::features::DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
    );
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#9",
        DesignFeatureKind::BoxPrimitive,
        1,
    );
    feature.native_ref = Some(scope.id.clone());
    let active = [body];
    let index = super::super::stored_bodies::ActiveBodies::new(&active);
    assert!(!super::super::stored_bodies::bind_history_outputs(
        &mut feature,
        &scope,
        &[1],
        None,
        &index,
    ));
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureBoxPrimitiveUnresolved.kind()
    );
}

#[test]
fn stored_body_scope_reports_recipe_absence_from_snapshot_kind() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::DesignScopePayloadMut;
    let (mut feature, mut scope, history, body) = native_base_feature();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[body],
        &Default::default(),
    )
    .unwrap();
    let losses = super::super::stored_bodies::losses(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "body_entity_suffixes":[81], "body_entity_suffix_offsets":[22],
                "body_reference_records":[81], "body_reference_record_offsets":[22],
                "parameter_body_record":91, "parameter_body_record_offset":100,
                "auxiliary_record":92, "auxiliary_record_offset":120,
                "envelope_guid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                "envelope_guid_offset":140, "tag_body_based_on_faces":true,
                "tag_body_based_on_faces_offset":90
            }))
            .unwrap(),
        );
    } else {
        panic!("Base Feature payload");
    }
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()
    );
}

#[test]
fn stored_body_scope_without_any_proof_keeps_native_definition() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, _) = output_binding_inputs();
    feature.evaluation.set_outputs(Default::default());
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [],
        &[],
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
        if kind.as_str() == "Base Feature"));
}

fn history_in_source(source: &str, bodies: &[i64]) -> crate::history_records::AsmHistory {
    let stream = format!("f3d:asset/Breps.BlobParts/BREP.{source}");
    let id = format!("{stream}:asm-history#1");
    let mut states = vec![
        state(1, 1, Some(0), bodies_topology(bodies)),
        state(0, 0, None, bodies_topology(&[])),
    ];
    for (state, _, _) in &mut states {
        state.id = format!("{stream}:asm-delta-state#{}", state.state_id);
        state.parent = id.clone();
    }
    crate::test_support::history::history(id, 0, None, false, states)
}

#[test]
fn stored_body_scope_qualifies_body_slots_by_history_blob() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let mut first = active_body();
    first.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/first.smbh/brep:entity#1").unwrap();
    let mut second = active_body();
    second.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/second.smbh/brep:entity#1").unwrap();
    for (source, expected) in [("first.smbh", &first), ("second.smbh", &second)] {
        for bodies in [
            [first.clone(), second.clone()],
            [second.clone(), first.clone()],
        ] {
            let (mut feature, scope, _, _) = native_base_feature();
            super::super::bind_feature_outputs(
                std::slice::from_mut(&mut feature),
                std::slice::from_ref(&scope),
                &mut [history_in_source(source, &[1])],
                &bodies,
                &Default::default(),
            )
            .unwrap();
            assert_eq!(feature.evaluation.outputs(), &[expected.id.clone()]);
            assert!(matches!(feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                    bodies: BodySelection::Resolved { bodies, .. }
                }) if bodies.as_slice() == [expected.id.clone()]));
        }
    }
}

#[test]
fn stored_body_scope_refuses_a_body_from_another_history_blob() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, mut body) = native_base_feature();
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/other.smbh/brep:entity#1").unwrap();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history_in_source("selected.smbh", &[1])],
        &[body],
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesUnbound.kind()
    );
}

#[test]
fn stored_body_scope_refuses_a_declared_body_count_mismatch() {
    use crate::loss::F3dLossCode;
    use crate::records::feature::scope::DesignScopePayloadMut;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, mut scope, _, _) = native_base_feature();
    if let DesignScopePayloadMut::BaseFeature(slot) = scope.payload_mut() {
        *slot = Some(
            serde_json::from_value(serde_json::json!({
                "body_entity_suffixes":[81], "body_entity_suffix_offsets":[22],
                "body_reference_records":[81], "body_reference_record_offsets":[22],
                "parameter_body_record":91, "parameter_body_record_offset":100,
                "auxiliary_record":92, "auxiliary_record_offset":120,
                "envelope_guid":"aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
                "envelope_guid_offset":140, "tag_body_based_on_faces":true,
                "tag_body_based_on_faces_offset":90
            }))
            .unwrap(),
        );
    }
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history_in_source("selected.smbh", &[])],
        &[],
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureStoredBodiesUnbound.kind()
    );
}

#[test]
fn stored_body_scope_binds_explicit_body_members_without_history() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, _, body) = output_binding_inputs();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: cadmpeg_ir::features::DistinctMembers::try_from(vec![body.id.clone()])
                        .unwrap(),
                    native: scope.id.clone(),
                },
            },
        ));
    feature.evaluation.set_outputs(Default::default());
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [],
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs(), &[body.id]);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { .. }
        })
    ));
}

const DERIVED_KINDS: [&str; 2] = ["Derived Base Feature", "DerivedBaseFeature"];

fn derived_scope(
    kind: &str,
    state_id: i64,
    previous_state_id: i64,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{state_id}"),
        DesignFeatureKind::try_from(kind.to_owned()).unwrap(),
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn derived_native_feature(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> cadmpeg_ir::features::Feature {
    let (mut feature, _, _, _) = native_base_feature();
    feature.native_ref = Some(scope.id.clone());
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: scope.kind_name().into(),
                parameters: Default::default(),
            },
        ));
    feature
}

#[test]
fn derived_base_scope_with_proven_bodies_is_a_stored_snapshot() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    for kind in DERIVED_KINDS {
        let scope = derived_scope(kind, 1, 0);
        let mut feature = derived_native_feature(&scope);
        let (_, _, history, body) = native_base_feature();
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut [history],
            std::slice::from_ref(&body),
            &Default::default(),
        )
        .unwrap();
        assert!(
            matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved { bodies, native }
            }) if bodies.as_slice() == [body.id.clone()] && native == &scope.id),
            "{kind}"
        );
        let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(losses.len(), 1, "{kind}");
        assert_eq!(
            losses[0].code,
            F3dLossCode::FeatureDerivedBaseUnresolved.kind()
        );
        assert!(losses[0].message.contains("stored snapshot"));
    }
}

#[test]
fn derived_base_scope_without_proven_bodies_stays_native() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    for kind in DERIVED_KINDS {
        for refusal in [
            "missing body",
            "duplicate state",
            "ambiguous body",
            "no history",
        ] {
            let scope = derived_scope(kind, 1, 0);
            let mut feature = derived_native_feature(&scope);
            let (_, _, history, body) = native_base_feature();
            let mut histories = vec![history];
            let bodies = match refusal {
                "duplicate state" => {
                    histories.push(native_base_feature().2);
                    vec![body]
                }
                "ambiguous body" => {
                    let mut other = body.clone();
                    other.id = cadmpeg_ir::ids::BodyId::mint("test:model:body#1").unwrap();
                    vec![body, other]
                }
                "no history" => {
                    histories.clear();
                    vec![body]
                }
                _ => Vec::new(),
            };
            super::super::bind_feature_outputs(
                std::slice::from_mut(&mut feature),
                std::slice::from_ref(&scope),
                &mut histories,
                &bodies,
                &Default::default(),
            )
            .unwrap();
            assert!(
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Native { kind: native, .. })
                        if native.as_str() == kind
                ),
                "{kind} {refusal}"
            );
            let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code,
                F3dLossCode::FeatureDerivedBaseUnresolved.kind()
            );
            assert!(losses[0].message.contains("native derived source"));
        }
    }
}

#[test]
fn derived_base_scope_with_proven_empty_transition_is_a_typed_no_op() {
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    for kind in DERIVED_KINDS {
        let scope = derived_scope(kind, 1, 0);
        let mut feature = derived_native_feature(&scope);
        let history = crate::test_support::history::history(
            "f3d:asm-history#1".into(),
            0,
            None,
            false,
            vec![
                state(1, 1, Some(0), bodies_topology(&[])),
                state(0, 0, None, bodies_topology(&[])),
            ],
        );
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut [history],
            &[],
            &Default::default(),
        )
        .unwrap();
        assert!(
            matches!(feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved { bodies, .. }
            }) if bodies.is_empty()),
            "{kind}"
        );
        let losses = super::super::stored_bodies::losses(&[feature], &[scope]);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, F3dLossCode::FeatureStoredBodiesEmpty.kind());
    }
}

fn solid_box_topology(
    body: i64,
    min_pt: cadmpeg_ir::math::Point3,
    max_pt: cadmpeg_ir::math::Point3,
) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPoint,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    AsmHistoricalTopology {
        bodies: vec![body],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: body,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3],
        }],
        shell_faces: vec![AsmHistoricalRelation {
            owner_ref: 3,
            member_refs: vec![4],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6, 7],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 8,
                next: 7,
                previous: 7,
                radial_next: 7,
            },
            AsmHistoricalCoedge {
                coedge: 7,
                owner_loop: 5,
                edge: 8,
                next: 6,
                previous: 6,
                radial_next: 6,
            },
        ],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 8,
            start_vertex: 10,
            end_vertex: 11,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 20,
                position: min_pt,
            },
            AsmHistoricalPoint {
                point: 21,
                position: max_pt,
            },
        ],
        ..AsmHistoricalTopology::default()
    }
}

fn solid_boxes_topology(bodies: &[i64]) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalPoint,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::Point3;
    let mut topo = AsmHistoricalTopology::default();
    for (idx, &body) in bodies.iter().enumerate() {
        let base = (i64::try_from(idx).unwrap() + 1) * 100;
        topo.bodies.push(body);
        topo.body_regions.push(AsmHistoricalRelation {
            owner_ref: body,
            member_refs: vec![base + 2],
        });
        topo.region_shells.push(AsmHistoricalRelation {
            owner_ref: base + 2,
            member_refs: vec![base + 3],
        });
        topo.shell_faces.push(AsmHistoricalRelation {
            owner_ref: base + 3,
            member_refs: vec![base + 4],
        });
        topo.face_loops.push(AsmHistoricalRelation {
            owner_ref: base + 4,
            member_refs: vec![base + 5],
        });
        topo.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: base + 5,
            member_refs: vec![base + 6, base + 7],
        });
        topo.coedge_topology.push(AsmHistoricalCoedge {
            coedge: base + 6,
            owner_loop: base + 5,
            edge: base + 8,
            next: base + 7,
            previous: base + 7,
            radial_next: base + 7,
        });
        topo.coedge_topology.push(AsmHistoricalCoedge {
            coedge: base + 7,
            owner_loop: base + 5,
            edge: base + 8,
            next: base + 6,
            previous: base + 6,
            radial_next: base + 6,
        });
        topo.edge_vertices.push(AsmHistoricalEdge {
            edge: base + 8,
            start_vertex: base + 10,
            end_vertex: base + 11,
        });
        topo.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: base + 10,
            carrier: base + 20,
        });
        topo.vertex_points.push(AsmHistoricalCarrierBinding {
            entity: base + 11,
            carrier: base + 21,
        });
        topo.point_positions.push(AsmHistoricalPoint {
            point: base + 20,
            position: Point3::new(0.0, 0.0, 0.0),
        });
        topo.point_positions.push(AsmHistoricalPoint {
            point: base + 21,
            position: Point3::new(10.0, 10.0, 10.0),
        });
    }
    topo
}

#[test]
fn historical_partial_and_native_edge_selections_test() {
    use crate::history_records::{AsmHistoricalEdgeSample, AsmHistoricalTopology};
    use crate::loss::F3dLossCode;
    use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
    use cadmpeg_ir::features::{
        DistinctMembers, EdgeSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };
    use cadmpeg_ir::ids::HistoricalEdgeId;
    use cadmpeg_ir::math::Point3;

    let make_fillet = |edges: EdgeSelection| Feature {
        id: FeatureId::mint("test:model:feature#fillet").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Fillet {
                groups: cadmpeg_ir::features::NonEmptyMembers::one(FilletGroup {
                    edges,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                }),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some("native:scope".into()),
        product_definition: None,
    };

    let scope = scope_between(1, 0);

    // 1. HistoricalPartial: 1 proven edge (#10) and 1 unresolved string ("unresolved_edge")
    let mut feature_partial = make_fillet(
        EdgeSelection::historical_partial(
            crate::ids::feature_input_topology_id(
                &FeatureId::mint("test:model:feature#fillet").unwrap(),
                0,
            ),
            vec![HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:0:10").unwrap()],
            vec!["unresolved_edge".into()],
            "native:ref".into(),
        )
        .unwrap(),
    );
    feature_partial.native_ref = Some(scope.id.clone());

    let mut state0_topo = AsmHistoricalTopology {
        edges: vec![10],
        ..Default::default()
    };
    state0_topo.edge_samples.push(AsmHistoricalEdgeSample {
        edge: 10,
        interior: Point3::new(1.0, 0.0, 0.0),
        tangent: None,
    });

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), AsmHistoricalTopology::default()),
            state(0, 0, None, state0_topo),
        ],
    );

    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature_partial),
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        &[],
        &Default::default(),
    )
    .unwrap();

    // Verify midpoints emitted for the proven edge
    assert_eq!(
        feature_partial
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("edge_midpoints")),
        Some(&"1,0,0".to_string())
    );
    // Verify HistoricalEdgeMidpointsUnresolved emitted because of the unresolved edge
    assert!(losses
        .iter()
        .any(|note| note.code == F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind()));

    // 2. Native edge selection: emits no midpoints, emits HistoricalEdgeMidpointsUnresolved
    let mut feature_native = make_fillet(EdgeSelection::Native("native:edge".into()));
    feature_native.native_ref = Some(scope.id.clone());

    let losses_native = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature_native),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();

    assert_eq!(
        feature_native
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("edge_midpoints")),
        None
    );
    assert!(losses_native
        .iter()
        .any(|note| note.code == F3dLossCode::HistoricalEdgeMidpointsUnresolved.kind()));
}

#[test]
fn historical_body_bounds_unresolved_test() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    use crate::loss::F3dLossCode;

    let scope = scope_between(1, 0);
    let mut feature = base_feature();
    feature.native_ref = Some(scope.id.clone());

    // Construct a solid body (body 1) in state 1, but with NO point positions,
    // so historical_body_bounds returns None!
    let state1_topo = AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3],
        }],
        shell_faces: vec![AsmHistoricalRelation {
            owner_ref: 3,
            member_refs: vec![4],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6, 7],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 6,
                owner_loop: 5,
                edge: 8,
                next: 7,
                previous: 7,
                radial_next: 7,
            },
            AsmHistoricalCoedge {
                coedge: 7,
                owner_loop: 5,
                edge: 8,
                next: 6,
                previous: 6,
                radial_next: 6,
            },
        ],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 8,
            start_vertex: 10,
            end_vertex: 11,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
        ],
        // No point_positions!
        ..AsmHistoricalTopology::default()
    };

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();

    assert!(losses
        .iter()
        .any(|note| note.code == F3dLossCode::HistoricalBodyBoundsUnresolved.kind()));
}

#[test]
fn body_measurement_covers_only_the_states_feature_outputs_read() {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    use crate::loss::F3dLossCode;

    let relation = |owner_ref, member| AsmHistoricalRelation {
        owner_ref,
        member_refs: vec![member],
    };
    let solid_without_points = || AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, 2)],
        region_shells: vec![relation(2, 3)],
        shell_faces: vec![relation(3, 4)],
        ..AsmHistoricalTopology::default()
    };
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(4, 4, Some(3), solid_without_points()),
            state(3, 3, Some(2), solid_without_points()),
            state(2, 2, Some(1), solid_without_points()),
            state(1, 1, Some(0), solid_without_points()),
            state(0, 0, None, solid_without_points()),
        ],
    );
    let scope = scope_between(3, 1);
    let mut feature = base_feature();
    feature.native_ref = Some(scope.id.clone());

    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();

    let mut measured = losses
        .iter()
        .filter(|note| note.code == F3dLossCode::HistoricalBodyBoundsUnresolved.kind())
        .map(|note| note.message.as_str())
        .collect::<Vec<_>>();
    measured.sort_unstable();
    assert_eq!(
        measured,
        [
            "historical solid body 1 has unresolved bounds in state 1",
            "historical solid body 1 has unresolved bounds in state 2",
            "historical solid body 1 has unresolved bounds in state 3",
        ]
    );
}

#[test]
fn is_solid_historical_body_single_face_solid_and_sheet_closure() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::Point3;

    // 1-face closed solid (e.g. sphere or torus with 1 face, shell closed, coedges on edge >= 2)
    let closed_solid =
        solid_box_topology(1, Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
    assert!(super::super::HistoricalBodyIndex::new(&closed_solid).is_solid(1));

    // 2-face open sheet (edge 8 only has 1 coedge incidence, coedge 7)
    let open_sheet = AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3],
        }],
        shell_faces: vec![AsmHistoricalRelation {
            owner_ref: 3,
            member_refs: vec![4, 5],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![6],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 6,
            member_refs: vec![7],
        }],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 7,
            owner_loop: 6,
            edge: 8,
            next: 7,
            previous: 7,
            radial_next: 7,
        }],
        ..AsmHistoricalTopology::default()
    };
    assert!(!super::super::HistoricalBodyIndex::new(&open_sheet).is_solid(1));
}

#[test]
fn changed_topology_family_refs_includes_carriers() {
    use crate::history_records::AsmHistoricalTopologyDelta;
    let mut delta = AsmHistoricalTopologyDelta::default();
    delta.points.inserted = vec![101];
    delta.surfaces.updated = vec![102];
    delta.curves.deleted = vec![103];
    delta.pcurves.inserted = vec![104];

    let changed = super::super::changed_topology_family_refs(&delta, false);
    assert!(changed.contains(&101));
    assert!(changed.contains(&102));
    assert!(!changed.contains(&103)); // deleted = false

    let deleted = super::super::changed_topology_family_refs(&delta, true);
    assert!(deleted.contains(&103));
}

#[test]
fn stored_false_shell_retained_on_bbox_growth() {
    use cadmpeg_ir::features::{
        DistinctMembers, FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };
    use cadmpeg_ir::math::Point3;

    let scope = scope_between(1, 0);
    let mut feature = Feature {
        id: FeatureId::mint("test:model:feature#shell").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Shell".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Shell {
                bodies: None,
                removed_faces: FaceSelection::Faces(vec![]),
                thickness: None,
                outward: Some(false),
                mode: None,
                join: None,
                resolve_intersections: None,
                allow_self_intersections: None,
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
        product_definition: None,
    };

    // State 0 bbox width = 10.0; State 1 bbox width = 20.0 (grew > 1.0e-3)
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(
                1,
                1,
                Some(0),
                solid_box_topology(1, Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 10.0)),
            ),
            state(
                0,
                0,
                None,
                solid_box_topology(1, Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0)),
            ),
        ],
    );

    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::Shell { outward, .. }) =
        feature.evaluation.definition()
    else {
        panic!("expected Shell");
    };
    // Must remain Some(false), NOT overwritten with Some(true)
    assert_eq!(*outward, Some(false));
}

#[test]
fn combine_join_to_cut_deleting_vs_non_deleting() {
    use cadmpeg_ir::features::{
        BooleanKind, DistinctMembers, Feature, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };

    let make_combine = || Feature {
        id: FeatureId::mint("test:model:feature#combine").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Combine".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: cadmpeg_ir::features::CombineOperands::new(
                    cadmpeg_ir::features::BodySelection::Unresolved,
                    cadmpeg_ir::features::BodySelection::Unresolved,
                )
                .unwrap(),
                op: BooleanKind::Join,
                keep_tools: true,
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some("native:scope".into()),
        product_definition: None,
    };

    let scope = scope_between(1, 0);

    // 1. Non-deleting case: state 0 has body 1, state 1 has body 1. deleted_set is empty.
    // Flips from Join to Cut!
    let mut feature_non_deleting = make_combine();
    feature_non_deleting.native_ref = Some(scope.id.clone());

    let history_non_deleting = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), solid_boxes_topology(&[1])),
            state(0, 0, None, solid_boxes_topology(&[1])),
        ],
    );

    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature_non_deleting),
        std::slice::from_ref(&scope),
        &mut [history_non_deleting],
        &[],
        &Default::default(),
    )
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::Combine { op, .. }) =
        feature_non_deleting.evaluation.definition()
    else {
        panic!("expected Combine");
    };
    assert_eq!(*op, BooleanKind::Cut);

    // 2. Deleting case: state 0 has bodies 1 & 2, state 1 has only body 1 (body 2 deleted).
    // Does NOT flip to Cut; remains Join!
    let mut feature_deleting = make_combine();
    feature_deleting.native_ref = Some(scope.id.clone());

    let history_deleting = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), solid_boxes_topology(&[1])),
            state(0, 0, None, solid_boxes_topology(&[1, 2])),
        ],
    );

    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature_deleting),
        std::slice::from_ref(&scope),
        &mut [history_deleting],
        &[],
        &Default::default(),
    )
    .unwrap();

    let FeatureDefinition::Operation(FeatureOperation::Combine { op, .. }) =
        feature_deleting.evaluation.definition()
    else {
        panic!("expected Combine");
    };
    assert_eq!(*op, BooleanKind::Join);
}

#[test]
fn only_the_operand_list_form_keeps_a_stored_join_with_kept_tools() {
    use crate::records::feature::combine::{
        DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation, DesignCombineTools,
    };
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    use cadmpeg_ir::features::{
        BodySelection, BooleanKind, CombineOperands, DistinctMembers, Feature, FeatureDefinition,
        FeatureEvaluation, FeatureId, FeatureOperation,
    };

    let resolved = |form: Option<DesignCombineForm>, bodies_before: &[i64]| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Combine,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(1);
                draft.previous_history_state_id = Some(0);
                draft.layout_fixture_tail();
            })
            .unwrap();
        if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
            *slot = form.map(|form| DesignCombineOperation {
                form,
                operation: BooleanKind::Join,
                operation_offset: 0,
                keep_tools: true,
                keep_tools_offset: 0,
                target_record_index: 11,
                target_body_node: None,
                tools: DesignCombineTools {
                    first: DesignCombineBodySelection {
                        record_index: 12,
                        external_identity: None,
                        body_node: None,
                    },
                    additional: Vec::new(),
                },
            });
        }
        let mut feature = Feature {
            id: FeatureId::mint("test:model:feature#combine").unwrap(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: Some("Combine".into()),
            source_text: None,
            source_content: Default::default(),
            evaluation: FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::Combine {
                    operands: CombineOperands::new(
                        BodySelection::Unresolved,
                        BodySelection::Unresolved,
                    )
                    .unwrap(),
                    op: BooleanKind::Join,
                    keep_tools: true,
                }),
                DistinctMembers::try_from(vec![]).unwrap(),
            ),
            native_ref: Some(scope.id.clone()),
            product_definition: None,
        };
        let history = crate::test_support::history::history(
            "f3d:asm-history#1".into(),
            0,
            None,
            false,
            vec![
                state(1, 1, Some(0), solid_boxes_topology(&[1])),
                state(0, 0, None, solid_boxes_topology(bodies_before)),
            ],
        );
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            &mut [history],
            &[],
            &Default::default(),
        )
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Combine { op, .. }) =
            feature.evaluation.definition()
        else {
            panic!("expected Combine");
        };
        *op
    };

    for (form, expected) in [
        (None, BooleanKind::Cut),
        (Some(DesignCombineForm::Standard), BooleanKind::Cut),
        (Some(DesignCombineForm::Compact), BooleanKind::Cut),
        (Some(DesignCombineForm::ExtendedReference), BooleanKind::Cut),
        (Some(DesignCombineForm::OperandLists), BooleanKind::Join),
    ] {
        assert_eq!(resolved(form, &[1]), expected, "{form:?}");
        assert_eq!(resolved(form, &[1, 2]), BooleanKind::Join, "{form:?}");
    }
}

#[test]
fn split_plane_only_emitted_for_split_body() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalTopology,
    };
    use cadmpeg_ir::features::{
        DistinctMembers, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let scope = scope_between(1, 0);

    let state1_topo = AsmHistoricalTopology {
        faces: vec![10],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 20,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        ..AsmHistoricalTopology::default()
    };

    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), state1_topo),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );

    // 1. SplitBody: emits split_plane
    let mut feature_split = Feature {
        id: FeatureId::mint("test:model:feature#split").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Split Body".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::SplitBody {
                targets: cadmpeg_ir::features::BodySelection::Unresolved,
                tool: cadmpeg_ir::features::SplitBodyTool::Faces(
                    cadmpeg_ir::features::FaceSelection::Faces(vec![]),
                ),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
        product_definition: None,
    };

    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature_split),
        std::slice::from_ref(&scope),
        &mut [history.clone()],
        &[],
        &Default::default(),
    )
    .unwrap();

    assert_eq!(
        feature_split
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        Some(&"1,2,3;0,0,1".to_string())
    );

    // 2. Extrude: does NOT emit split_plane
    let mut feature_extrude = Feature {
        id: FeatureId::mint("test:model:feature#extrude").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Native("scope".into()),
                ),
                direction: cadmpeg_ir::features::ExtrudeDirection::default(),
                start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane {},
                extent: cadmpeg_ir::features::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: cadmpeg_ir::features::LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: cadmpeg_ir::features::BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: Some(false),
                allow_multi_profile_faces: Some(false),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
        product_definition: None,
    };

    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature_extrude),
        std::slice::from_ref(&scope),
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();

    assert_eq!(
        feature_extrude
            .source_properties
            .get(&cadmpeg_core::nonblank_literal!("split_plane")),
        None
    );
}

#[test]
fn feature_outputs_include_bodies_changed_before_the_terminal_scope_state() {
    use crate::history_records::AsmHistoricalTopology;
    let (mut feature, _, _, body) = native_base_feature();
    let scope = scope_between(2, 0);
    feature.native_ref = Some(scope.id.clone());
    let mut histories = [crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(2, 2, Some(1), bodies_topology(&[1])),
            state(1, 1, Some(0), bodies_topology(&[1])),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    )];
    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut histories,
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs().as_slice(), [body.id]);
    assert!(!losses.iter().any(|loss| {
        loss.code == crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.kind()
    }));
}

#[test]
fn feature_output_interval_refusal_has_a_loss() {
    for refusal in ["truncated", "ambiguous", "link mutation"] {
        let (mut feature, scope, mut history, body) = native_base_feature();
        let mut histories = match refusal {
            "truncated" => {
                history.states_mut().truncate(1);
                vec![history]
            }
            "ambiguous" => vec![history.clone(), history],
            _ => {
                history.states_mut()[0].next_ref = Some(1);
                vec![history]
            }
        };
        let losses = super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut histories,
            &[body],
            &Default::default(),
        )
        .unwrap();
        assert!(feature.evaluation.outputs().is_empty());
        assert!(losses.iter().any(|loss| {
            loss.code == crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.kind()
        }));
    }
}

#[test]
fn feature_outputs_follow_bound_history_when_numeric_states_repeat() {
    use std::collections::HashMap;
    let mut first = active_body();
    first.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/first.smbh/brep:entity#1").unwrap();
    let mut second = active_body();
    second.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/second.smbh/brep:entity#1").unwrap();
    for (source, expected) in [("first.smbh", &first), ("second.smbh", &second)] {
        for reversed in [false, true] {
            let (mut feature, scope, _, _) = native_base_feature();
            feature
                .evaluation
                .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::MoveFace {
                        faces: cadmpeg_ir::features::FaceSelection::Unresolved,
                        motion: cadmpeg_ir::features::FaceMotion::Offset {
                            distance: cadmpeg_ir::scalar::Length::new(5.0).unwrap(),
                        },
                    },
                ));
            let selected = history_in_source(source, &[1]).id;
            let bindings = HashMap::from([(scope.id.clone(), selected)]);
            let mut histories = vec![
                history_in_source("first.smbh", &[1]),
                history_in_source("second.smbh", &[1]),
            ];
            let mut bodies = vec![first.clone(), second.clone()];
            if reversed {
                histories.reverse();
                bodies.reverse();
            }
            let losses = super::super::bind_feature_outputs(
                std::slice::from_mut(&mut feature),
                &[scope],
                &mut histories,
                &bodies,
                &bindings,
            )
            .unwrap();
            assert_eq!(feature.evaluation.outputs(), &[expected.id.clone()]);
            assert!(!losses.iter().any(|loss| {
                loss.code == crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.kind()
                    || loss.code == crate::loss::F3dLossCode::FeatureOutputBodyUnbound.kind()
            }));
        }
    }
}

#[test]
fn feature_output_body_namespace_refusal_has_a_loss() {
    let (mut feature, scope, _, mut body) = native_base_feature();
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep/other.smbh/brep:entity#1").unwrap();
    let losses = super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history_in_source("selected.smbh", &[1])],
        &[body],
        &Default::default(),
    )
    .unwrap();
    assert!(feature.evaluation.outputs().is_empty());
    assert!(losses
        .iter()
        .any(|loss| { loss.code == crate::loss::F3dLossCode::FeatureOutputBodyUnbound.kind() }));
}

#[test]
fn stored_body_scope_keeps_equal_state_without_a_stored_body_proof_native() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, _, _, _) = native_base_feature();
    let scope = scope_between(0, 0);
    feature.native_ref = Some(scope.id.clone());
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![state(0, 0, None, bodies_topology(&[]))],
    );
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        &[],
        &Default::default(),
    )
    .unwrap();
    assert!(feature.evaluation.outputs().is_empty());
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
}

#[test]
fn feature_output_equal_endpoints_preserve_an_existing_output_proof() {
    use cadmpeg_ir::features::{FaceMotion, FaceSelection, FeatureDefinition, FeatureOperation};
    let (mut feature, _, history, body) = output_binding_inputs();
    let scope = scope_between(1, 1);
    feature.native_ref = Some(scope.id.clone());
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: FaceSelection::Unresolved,
            motion: FaceMotion::Offset {
                distance: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
            },
        }));
    feature
        .evaluation
        .set_outputs(vec![body.id.clone()].try_into().unwrap());
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        std::slice::from_ref(&body),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs(), &[body.id]);
}

mod edge_midpoints;
