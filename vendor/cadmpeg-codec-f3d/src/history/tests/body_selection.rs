// SPDX-License-Identifier: Apache-2.0
//! Body-selection history ownership tests.

#![allow(clippy::default_trait_access)]

use crate::history::{bind_feature_body_selections, FeatureBodySelectionInputs};
use crate::records::topology::{
    construction::DesignConstructionOperandGroup,
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};

#[test]
fn move_body_selection_uses_unique_owning_history() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Move,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = Some(41);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_A,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let topology = || AsmHistoricalTopology {
        bodies: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64,
                 parent: &str,
                 next_ref: Option<i64>|
     -> crate::test_support::history::State {
        (
            AsmDeltaState {
                id: format!("f3d:{parent}:asm-delta-state#{state_id}"),
                parent: parent.into(),
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
            Some(topology()),
            vec![crate::history_records::AsmEntityVersion {
                entity_ref: 1,
                record_ref: state_id,
            }],
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(42, "history", Some(41)), state(41, "history", None)],
    );
    let unrelated_history = crate::test_support::history::history(
        "unrelated-history".into(),
        0,
        None,
        false,
        vec![state(41, "unrelated-history", None)],
    );
    let histories = [history, unrelated_history];
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#move").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies: BodySelection::Native(group_id.into()),
                translation: cadmpeg_ir::features::FiniteVector3::new(
                    cadmpeg_ir::math::Vector3::new(1.0, 2.0, 3.0),
                )
                .unwrap(),
                rotation: None,
                copies: 0,
            }),
        ),
        native_ref: None,
    };
    feature.native_ref = Some(scope.id.clone());
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    let losses = bind_feature_body_selections(std::slice::from_mut(&mut feature), &inputs);
    assert!(losses.is_empty());

    let expected_body = crate::ids::history_input_body_id(
        &crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("move"), 41),
        1,
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Historical { ref bodies, ref native, .. },
            ..
        }) if bodies.as_slice() == [expected_body] && native.as_str() == group_id
    ));
}

fn pattern_seed_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CircularPattern,
        10,
    );
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: "280".to_owned().try_into().unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 0,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#pattern").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    super::super::bind_pattern_body_selections(
        std::slice::from_mut(&mut feature),
        &inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    );
    feature
}

#[test]
fn pattern_body_seed_preserves_native_selection() {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };
    let feature = pattern_seed_feature();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(),
                [PatternSeed::Bodies(BodySelection::Native(id))]
                    if id == "f3d:Design/BulkStream.dat:design-construction-operand-group#20")));
}

#[test]
fn a_pattern_seed_without_a_stored_preceding_state_binds_through_its_history() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use crate::records::topology::body_recipe::{
        DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignOperandOwner,
    };
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed},
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CircularPattern,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let topology = || AsmHistoricalTopology {
        bodies: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64,
                 parent: &str,
                 next_ref: Option<i64>|
     -> crate::test_support::history::State {
        (
            AsmDeltaState {
                id: format!("f3d:{parent}:asm-delta-state#{state_id}"),
                parent: parent.into(),
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
            Some(topology()),
            vec![crate::history_records::AsmEntityVersion {
                entity_ref: 1,
                record_ref: state_id,
            }],
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(42, "history", Some(41)), state(41, "history", None)],
    );
    let unrelated_history = crate::test_support::history::history(
        "unrelated-history".into(),
        0,
        None,
        false,
        vec![state(41, "unrelated-history", None)],
    );
    let histories = [history, unrelated_history];

    let operand = DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: "f3d:Design/BulkStream.dat:body-recipe-operand#21".into(),
        scope_record_index: 10,
        owner: DesignOperandOwner::Group {
            group_record_index: 20,
            group_member_ordinal: 0,
        },
        record_index: 21,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("350".to_owned()).unwrap(),
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
        references: vec![
            crate::records::topology::body_recipe::DesignBodyRecipeReference {
                design_reference: 301,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: Vec::new(),
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            },
        ],
        nested_record_index: 24,
        nested_record_index_offset: 38,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#25".into(),
        resolved_face_slot: None,
        resolved_body_state_id: Some(41),
        resolved_body_slot: Some(1),
        resolved_body_face_slots: Vec::new(),
        next_record_index: 25,
        next_byte_offset: 256,
    })
    .unwrap();
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#pattern").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: vec![PatternSeed::Bodies(BodySelection::Native(group_id.into()))],
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    super::super::bind_pattern_body_selections(
        std::slice::from_mut(&mut feature),
        &inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    );
    let expected =
        crate::ids::history_input_body_id(&crate::ids::feature_input_prefix(&feature.id, 41), 1);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(),
                [PatternSeed::Bodies(BodySelection::Historical { bodies, .. })]
                    if bodies.as_slice() == [expected])
    ));
}
