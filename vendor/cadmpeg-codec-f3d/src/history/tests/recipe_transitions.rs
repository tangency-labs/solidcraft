// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.

use crate::history::{
    bind_face_operand_history_candidates, bind_feature_face_selections, bind_sweep_result_modes,
    historical_brep_source, select_legacy_extrude_face_candidate, BindingHistories,
    LegacyFaceResolution,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::HashMap;

fn split_face_case() -> (
    Vec<cadmpeg_ir::features::Feature>,
    cadmpeg_ir::ids::FaceId,
    String,
) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use crate::records::{
        feature::scope::DesignParameterScope,
        recipes::ConstructionRecipeKind,
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame, face::DesignFaceOperand,
        },
    };
    use cadmpeg_ir::features::{
        FaceSelection, Feature, FeatureDefinition, FeatureId, FeatureOperation, SplitFaceTool,
    };
    use cadmpeg_ir::ids::FaceId;

    let scope_id = "f3d:Design/BulkStream.dat:scope#42".to_string();
    let group_id = "f3d:Design/BulkStream.dat:operand-group#100".to_string();
    let face_id = FaceId::mint("f3d:brep:entity#7").expect("identity grammar");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::scope::DesignFeatureKind::SplitFace,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
        })
        .unwrap();

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.clone(),
            scope_record_index: 42,
            scope_reference_ordinal: 2,
            record_index: 100,
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1010,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1008,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1048,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1052,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X10,
                ),
            role_offset: 1030,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1100,
        },
    )
    .unwrap();
    let operand =
        DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
            scope_record_index: 42,
            scope_reference_ordinal: 3,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 200,
            byte_offset: 1200,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            paired_byte_offset: 1300,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 203,
            recipe_record_byte_offset: 1400,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
            recipe_prefix_offset: 1411,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: 1420,
            recipe_program: Vec::new(),

            recipe_nodes: Vec::new(),
            candidate_faces: vec![face_id.clone()],
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: vec![face_id.clone()],
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 204,
            next_byte_offset: 1500,
        })
        .unwrap();
    let state = |state_id, next_ref| {
        (
            AsmDeltaState {
                id: format!("f3d:history:state#{state_id}"),
                parent: "f3d:history".into(),
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
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![state(2, Some(1)), state(1, None)],
    );
    let mut features = vec![Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#42").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("SplitFace".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::SplitFace {
                targets: FaceSelection::Native(group_id.clone()),
                tool: SplitFaceTool::Plane {
                    plane: FeatureId::mint("f3d:test:feature#plane").expect("identity grammar"),
                },
            }),
        ),
        native_ref: Some(scope_id),
    }];

    super::super::bind_feature_face_selections(
        &mut features,
        &mut [],
        crate::history::FeatureFaceSelectionInputs {
            scopes: &[scope],
            groups: &[group],
            operands: &[operand],
            entity_operands: &[],
            body_recipe_operands: &[],
            histories: &mut [history],
        },
    );

    (features, face_id, group_id)
}

#[test]
fn split_face_targets_bind_from_a_transition_predecessor() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let (features, face_id, group_id) = split_face_case();
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Resolved { faces, native },
            ..
        }) if faces == &[face_id] && native == &group_id
    ));
}

#[test]
fn thread_face_group_uses_first_reference_transition_candidates() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalTopology,
    };
    use crate::records::{
        dimensions::DesignRecipeReference,
        feature::{
            scope::DesignParameterScope,
            thread::{DesignThreadConstruction, DesignThreadForm},
        },
        recipes::ConstructionRecipeKind,
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame, face::DesignFaceOperand,
        },
    };
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar");
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::scope::DesignFeatureKind::Thread,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
            scope_record_index: 42,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1_010,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_008,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_048,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_052,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X10,
                ),
            role_offset: 1_030,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1_100,
        },
    )
    .unwrap();
    let reference = |token: &str, design_reference, candidates: &[i64]| DesignRecipeReference {
        selector: 1,
        selector_offset: 1_411,
        token: token.into(),
        token_offset: 1_415,
        design_reference,
        design_reference_offset: 1_420,
        candidate_faces: candidates.iter().copied().map(face).collect(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let operand =
        DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
            scope_record_index: 42,
            scope_reference_ordinal: 1,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 200,
            byte_offset: 1_200,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            paired_byte_offset: 1_300,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 203,
            recipe_record_byte_offset: 1_400,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
            recipe_prefix_offset: 1_411,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: vec![reference("3", 203, &[7, 8]), reference("-1", 199, &[9, 10])],
            recipe_kind: ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 1_430,
            recipe_program: vec![0, -1, 2],

            recipe_nodes: Vec::new(),
            candidate_faces: [7, 8, 9, 10].into_iter().map(face).collect(),
            unreferenced_candidate_faces: [9, 10].into_iter().map(face).collect(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 204,
            next_byte_offset: 1_500,
        })
        .unwrap();

    let state = |state_id, topology, next_ref| {
        (
            AsmDeltaState {
                id: format!("f3d:history:state#{state_id}"),
                parent: "f3d:history".into(),
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
    let mut history = crate::test_support::history::history(
        "f3d:history".into(),
        0,
        None,
        false,
        vec![
            state(2, AsmHistoricalTopology::default(), Some(1)),
            state(
                1,
                AsmHistoricalTopology {
                    faces: vec![7, 8, 9, 10],
                    persistent_subentity_tags: [
                        (7, "3", 203),
                        (8, "3", 203),
                        (9, "-1", 199),
                        (10, "-1", 199),
                    ]
                    .into_iter()
                    .map(|(entity_ref, token, design_reference)| {
                        crate::history_records::AsmHistoricalPersistentSubentityTag {
                            entity_kind: AsmHistoricalEntityKind::Face,
                            entity_ref,
                            selector: 17,
                            token: token.into(),
                            design_references: vec![design_reference],
                            ordinal: 0,
                        }
                    })
                    .collect(),
                    ..AsmHistoricalTopology::default()
                },
                None,
            ),
        ],
    );

    let result_topology = history.topology(&history.states()[1]).unwrap().clone();
    crate::test_support::history::edit(&mut history, 1, |_, versions| {
        *versions = [7, 8, 9, 10]
            .into_iter()
            .map(|entity_ref| crate::history_records::AsmEntityVersion {
                entity_ref,
                record_ref: entity_ref,
            })
            .collect();
    });
    crate::test_support::history::edit(&mut history, 0, |topology, versions| {
        *topology = result_topology;
        *versions = [7, 8, 9, 10]
            .into_iter()
            .map(|entity_ref| crate::history_records::AsmEntityVersion {
                entity_ref,
                record_ref: if entity_ref == 7 { 107 } else { entity_ref },
            })
            .collect();
    });
    let mut operands = vec![operand.clone()];
    crate::test_support::with_decode_context(|decode_ctx| {
        bind_face_operand_history_candidates(
            decode_ctx,
            &mut operands,
            std::slice::from_ref(&scope),
            std::slice::from_ref(&group),
            &[],
            BindingHistories::Shared(std::slice::from_ref(&history)),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(operands[0].preceding_candidate_faces, [face(7), face(8)]);
    assert_eq!(operands[0].changed_candidate_faces, [face(7)]);
    assert_eq!(operands[0].resolved_face_slots, [7]);

    let mut cylinder_scope = scope.clone();
    if let crate::records::feature::scope::DesignScopePayloadMut::Thread(slot) =
        cylinder_scope.payload_mut()
    {
        *slot = Some(DesignThreadConstruction {
            form: DesignThreadForm::Standard,
            designation_offset: 0,
            designation: cadmpeg_core::text::NonBlankString::new("M4x0.7").unwrap(),
            nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
                "4.0".to_owned(),
            )
            .expect("nominal size"),
            profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
            pitch: cadmpeg_ir::scalar::PositiveReal::new(0.07).unwrap(),
            face_group_record_indices: vec![100],
            diameters: crate::records::feature::thread::DesignThreadDiameters::new(0.4, 0.2, 0.3)
                .unwrap(),
            extended: None,
        });
    }
    let mut cylinder_operand = operand.clone();
    cylinder_operand.recipe_references[0].candidate_faces =
        vec![FaceId::mint("f3d:brep/input/brep:entity#999").expect("identity grammar")];
    cylinder_operand.candidate_faces = cylinder_operand.recipe_references[0]
        .candidate_faces
        .clone();
    let mut cylinder_history = history.clone();
    cylinder_history.id = "f3d:Breps.BlobParts/BREP.input:asm-history#1".into();
    crate::test_support::history::edit(&mut cylinder_history, 1, |topology, _| {
        topology.surfaces = vec![70, 80];
        topology.face_surfaces = vec![
            AsmHistoricalCarrierBinding {
                entity: 7,
                carrier: 70,
            },
            AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 80,
            },
        ];
        topology.surface_cylinders = vec![
            AsmHistoricalCylinder {
                surface: 70,
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radius: 1.5,
            },
            AsmHistoricalCylinder {
                surface: 80,
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radius: 3.0,
            },
        ];
    });
    let cylinders = cylinder_history
        .topology(&cylinder_history.states()[1])
        .unwrap()
        .clone();
    crate::test_support::history::edit(&mut cylinder_history, 0, |topology, _| {
        *topology = cylinders;
    });
    let mut cylinder_operands = vec![cylinder_operand];
    crate::test_support::with_decode_context(|decode_ctx| {
        bind_face_operand_history_candidates(
            decode_ctx,
            &mut cylinder_operands,
            std::slice::from_ref(&cylinder_scope),
            std::slice::from_ref(&group),
            &[],
            BindingHistories::Shared(std::slice::from_ref(&cylinder_history)),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(cylinder_operands[0].resolved_face_slots, [7]);

    let mut stale_active_operand = cylinder_operands[0].clone();
    stale_active_operand.recipe_references[0]
        .candidate_faces
        .push(FaceId::mint("f3d:brep/input/brep:entity#998").expect("identity grammar"));
    let mut stale_active_operands = vec![stale_active_operand];
    crate::test_support::with_decode_context(|decode_ctx| {
        bind_face_operand_history_candidates(
            decode_ctx,
            &mut stale_active_operands,
            std::slice::from_ref(&cylinder_scope),
            std::slice::from_ref(&group),
            &[],
            BindingHistories::Shared(std::slice::from_ref(&cylinder_history)),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(stale_active_operands[0].resolved_face_slots, [7]);

    let mut ambiguous_geometry_history = cylinder_history;
    crate::test_support::history::edit(&mut ambiguous_geometry_history, 0, |_, versions| {
        versions
            .iter_mut()
            .find(|v| v.entity_ref == 8)
            .unwrap()
            .record_ref = 108;
    });
    crate::test_support::history::edit(&mut ambiguous_geometry_history, 1, |topology, _| {
        topology.surface_cylinders[1].radius = 1.6;
    });
    let mut ambiguous_geometry_operands = vec![cylinder_operands.remove(0)];
    crate::test_support::with_decode_context(|decode_ctx| {
        bind_face_operand_history_candidates(
            decode_ctx,
            &mut ambiguous_geometry_operands,
            &[cylinder_scope],
            std::slice::from_ref(&group),
            &[],
            BindingHistories::Shared(&[ambiguous_geometry_history]),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert!(ambiguous_geometry_operands[0]
        .resolved_face_slots
        .is_empty());

    let mut unrelated_group = group;
    unrelated_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::FACES,
        );
    let mut rejected = vec![operand];
    crate::test_support::with_decode_context(|decode_ctx| {
        bind_face_operand_history_candidates(
            decode_ctx,
            &mut rejected,
            &[scope],
            &[unrelated_group],
            &[],
            BindingHistories::Shared(&[history]),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(rejected[0].preceding_candidate_faces, [face(9), face(10)]);
    assert!(rejected[0].changed_candidate_faces.is_empty());
    assert!(rejected[0].resolved_face_slots.is_empty());
}

#[test]
fn unresolved_new_body_sweep_mode_follows_output_body_kind() {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureId, FeatureOperation, SweepMode, SweepSection,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};

    let body = |id: &str, kind| Body {
        id: BodyId::mint(id).expect("identity grammar"),
        kind,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let sweep = |id: &str, outputs: Vec<BodyId>| Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Sweep {
                shape: cadmpeg_ir::features::SweepShape::sheet_sections(
                    SweepMode::Unresolved {},
                    SweepSection::Unresolved(None),
                    Vec::new(),
                ),

                path: None,

                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                path_extent: None,
                guide_rail: None,
                taper: None,
                scale: None,
                allow_multi_profile_faces: None,
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(outputs).unwrap(),
        ),
        native_ref: None,
    };
    let bodies = [
        body("test:model:body#sheet", BodyKind::Sheet),
        body("test:model:body#solid", BodyKind::Solid),
    ];
    let mut features = [
        sweep(
            "synthetic:test:id#sheet-sweep",
            vec![BodyId::mint("test:model:body#sheet").expect("identity grammar")],
        ),
        sweep(
            "synthetic:test:id#solid-sweep",
            vec![BodyId::mint("test:model:body#solid").expect("identity grammar")],
        ),
        sweep(
            "synthetic:test:id#mixed-sweep",
            vec![
                BodyId::mint("test:model:body#sheet").expect("identity grammar"),
                BodyId::mint("test:model:body#solid").expect("identity grammar"),
            ],
        ),
        sweep(
            "synthetic:test:id#missing-sweep",
            vec![BodyId::mint("test:model:body#missing").expect("identity grammar")],
        ),
    ];

    bind_sweep_result_modes(&mut features, &bodies);

    let modes = features.map(|feature| match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => shape.mode(),
        _ => unreachable!(),
    });
    assert_eq!(modes[0], SweepMode::Surface {});
    assert_eq!(
        modes[1],
        SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::NewBody
        }
    );
    assert_eq!(modes[2], SweepMode::Unresolved {});
    assert_eq!(modes[3], SweepMode::Unresolved {});
}

#[test]
fn historical_brep_source_qualifies_state_local_candidates() {
    assert_eq!(
        historical_brep_source("f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#42"),
        Some("example.smbh")
    );
    assert_eq!(historical_brep_source("f3d:unqualified:state#42"), None);
}

#[test]
fn legacy_extrude_face_lane_prefers_history_then_source_identity() {
    use crate::history_records::AsmHistoricalTopology;
    use cadmpeg_ir::ids::FaceId;
    use std::collections::HashSet;

    let source_face = |source: &str, slot| {
        FaceId::mint(format!("f3d:brep/{source}/brep:entity#{slot}")).expect("identity grammar")
    };
    let active_candidates = vec![source_face("old", 10), source_face("new", 10)];
    assert_eq!(
        select_legacy_extrude_face_candidate(
            active_candidates.clone(),
            &AsmHistoricalTopology::default(),
            &HashSet::new(),
            Some("old"),
        ),
        Some(LegacyFaceResolution::Active(source_face("old", 10)))
    );
    assert_eq!(
        select_legacy_extrude_face_candidate(
            active_candidates,
            &AsmHistoricalTopology::default(),
            &HashSet::new(),
            Some("missing"),
        ),
        None
    );

    let historical_candidates = vec![
        FaceId::mint("f3d:brep:entity#20").expect("identity grammar"),
        source_face("new", 21),
    ];
    let topology = AsmHistoricalTopology {
        faces: vec![20, 21],
        ..AsmHistoricalTopology::default()
    };
    let mut changed = HashSet::new();
    changed.insert(21);
    assert_eq!(
        select_legacy_extrude_face_candidate(
            historical_candidates,
            &topology,
            &changed,
            Some("new"),
        ),
        Some(LegacyFaceResolution::Historical(21))
    );
    assert_eq!(
        select_legacy_extrude_face_candidate(
            vec![FaceId::mint("f3d:brep:entity#20").expect("identity grammar")],
            &topology,
            &changed,
            None,
        ),
        Some(LegacyFaceResolution::Historical(20))
    );
}

const HOLE_CANDIDATE_HISTORY: &str =
    "f3d:asset/Breps.BlobParts/BREP.example.smbh:asm-delta-state#2";

fn hole_face_case() -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::FeatureInputTopology>,
    cadmpeg_ir::features::FeatureId,
) {
    hole_face_case_in("f3d:history", vec![1])
}

fn hole_face_case_in(
    operation_history: &str,
    candidate_states: Vec<i64>,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::FeatureInputTopology>,
    cadmpeg_ir::features::FeatureId,
) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    let state = |state_id, next_ref| {
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
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        operation_history.into(),
        0,
        None,
        false,
        vec![state(2, Some(1)), state(1, None)],
    );
    hole_case_with(candidate_states, Vec::new(), history)
}

fn hole_case_with(
    candidate_states: Vec<i64>,
    locations: Vec<crate::records::feature::hole::DesignHoleLocation>,
    history: crate::history_records::AsmHistory,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::FeatureInputTopology>,
    cadmpeg_ir::features::FeatureId,
) {
    use crate::records::{
        feature::{
            hole::{DesignHoleConstruction, DesignHoleFaceSelection},
            scope::DesignParameterScope,
        },
        topology::entity_selection::DesignEntitySelectionFaceCandidate,
    };
    use cadmpeg_ir::features::{
        holes::HoleKind, FaceSelection, Feature, FeatureDefinition, FeatureId,
        FeatureInputTopology, FeatureOperation, LinearTermination,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let feature_id = FeatureId::mint("f3d:test:feature#42").expect("identity grammar");
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::scope::DesignFeatureKind::Hole,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.point = Some(DesignHoleConstruction {
            point_record_index: 55,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([0.0; 3]),
            position_offset: 0,
            direction: crate::test_support::reals([0.0, 0.0, 1.0]),
            direction_offset: 0,
            point_parameters: crate::test_support::reals([0.0; 2]),
            point_parameter_offsets: [0, 0],
            reference_type: 13,
            reference_type_offset: 0,
            tangent_point_data: None,
            input_records: vec![crate::records::identity::Located {
                value: 55,
                offset: 0,
            }],
            face_selection: Some(DesignHoleFaceSelection {
                record_index: 100,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("333".to_owned())
                    .unwrap(),
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
                historical_face_candidates: vec![DesignEntitySelectionFaceCandidate {
                    history_id: HOLE_CANDIDATE_HISTORY.into(),
                    historical: crate::records::topology::fillet::HistoricalBinding {
                        kind: AsmHistoricalEntityKind::Pcurve,
                        entity_ref: 18044,
                        state_ids: candidate_states,
                    },
                    face_slot: 30,
                }],
                next_record_index: 104,
                next_byte_offset: 0,
            }),
            locations,
        });
    }
    let mut feature = Feature {
        product_definition: None,
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: Some(FaceSelection::Native(scope_id.into())),
                direction: None,
                placements: Some(vec![cadmpeg_ir::features::holes::HolePlacement::Directed {
                    position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .unwrap(),
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                }]),
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
                    None,
                    Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
                )
                .unwrap(),

                extent: Some(LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(10.0).unwrap(),
                }),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    };
    feature.native_ref = Some(scope_id.into());
    let mut input_topologies = vec![FeatureInputTopology {
        id: crate::ids::feature_input_topology_id(&feature_id, 1),
        input_of: feature_id.clone(),
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        faces: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edges: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        vertices: cadmpeg_ir::features::DistinctMembers::try_from(Vec::new()).unwrap(),
        edge_geometry: Vec::new(),
        native_ref: None,
    }];

    bind_feature_face_selections(
        std::slice::from_mut(&mut feature),
        &mut input_topologies,
        crate::history::FeatureFaceSelectionInputs {
            scopes: &[scope],
            groups: &[],
            operands: &[],
            entity_operands: &[],
            body_recipe_operands: &[],
            histories: &mut [history],
        },
    );

    (feature, input_topologies, feature_id)
}

/// A plate whose top face 10 the Hole updates, with a rim centred on (10, 15, 20)
/// millimetres and an inserted cylinder on that axis.
fn drilled_plate_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{
        AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalCoedge,
        AsmHistoricalCurveAxis, AsmHistoricalCylinder, AsmHistoricalOptionalCarrierBinding,
        AsmHistoricalPlane, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    let axis = Vector3::new(0.0, 0.0, -1.0);
    let top = AsmHistoricalPlane {
        surface: 100,
        origin: Point3::new(20.0, 15.0, 20.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![10],
        surfaces: vec![100],
        surface_planes: vec![top.clone()],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 100,
        }],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        faces: vec![10, 30],
        surfaces: vec![100, 300],
        loops: vec![40],
        coedges: vec![41],
        edges: vec![42],
        curves: vec![43],
        surface_planes: vec![top],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 300,
            origin: Point3::new(10.0, 15.0, 15.0),
            axis,
            radius: 2.5,
        }],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 300,
            },
        ],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![40],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 40,
            member_refs: vec![41],
        }],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 41,
            owner_loop: 40,
            edge: 42,
            next: 41,
            previous: 41,
            radial_next: 41,
        }],
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 42,
            carrier: Some(43),
        }],
        curve_axes: vec![AsmHistoricalCurveAxis {
            curve: 43,
            origin: Point3::new(10.0, 15.0, 20.0),
            direction: axis,
        }],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64, topology, next_ref, face_record| {
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
            vec![AsmEntityVersion {
                entity_ref: 10,
                record_ref: face_record,
            }],
        )
    };
    crate::test_support::history::history(
        HOLE_CANDIDATE_HISTORY.into(),
        0,
        None,
        false,
        vec![
            state(2, result, Some(1), 110),
            state(1, preceding, None, 10),
        ],
    )
}

fn entry_location(position: [f64; 3]) -> crate::records::feature::hole::DesignHoleLocation {
    crate::records::feature::hole::DesignHoleLocation {
        record_index: 60,
        position: crate::test_support::reals(position),
        position_offset: 0,
        reference_type: 10,
        vertex_input: None,
    }
}

#[test]
fn hole_support_face_binds_ahead_of_a_selection_candidate() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let (feature, topologies, feature_id) = hole_case_with(
        vec![1],
        vec![entry_location([1.0, 1.5, 2.0])],
        drilled_plate_history(),
    );
    let prefix = crate::ids::feature_input_prefix(&feature_id, 1);
    let expected =
        crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(10));
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        face: Some(FaceSelection::Historical { state, faces, .. }),
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Hole support face stays unresolved");
    };
    assert_eq!(
        state,
        &crate::ids::feature_input_topology_id(&feature_id, 1)
    );
    assert_eq!(faces.as_slice(), [expected.clone()]);
    assert!(topologies[0].faces.contains(&expected));
}

#[test]
fn hole_entry_off_the_support_plane_falls_back_to_its_selection_candidate() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let (feature, _, feature_id) = hole_case_with(
        vec![1],
        vec![entry_location([1.0, 1.5, 3.0])],
        drilled_plate_history(),
    );
    let prefix = crate::ids::feature_input_prefix(&feature_id, 1);
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        face: Some(FaceSelection::Historical { faces, .. }),
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Hole keeps its selection candidate");
    };
    assert_eq!(
        faces.as_slice(),
        [crate::ids::history_input_face_id(
            &prefix,
            cadmpeg_ir::ids::IdentityKey::from(30)
        )]
    );
}

#[test]
fn hole_face_candidate_of_the_operation_history_binds_only_in_the_input_state() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let face_of = |feature: &cadmpeg_ir::features::Feature| match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Hole {
            face: Some(face), ..
        }) => face.clone(),
        _ => panic!("Hole definition"),
    };

    let (feature, topologies, _) = hole_face_case_in(HOLE_CANDIDATE_HISTORY, vec![1, 5]);
    assert!(matches!(
        face_of(&feature),
        FaceSelection::Historical { .. }
    ));
    assert_eq!(topologies[0].faces.as_slice().len(), 1);

    let (feature, topologies, _) = hole_face_case_in(HOLE_CANDIDATE_HISTORY, vec![5]);
    assert!(matches!(face_of(&feature), FaceSelection::Native(_)));
    assert!(topologies[0].faces.as_slice().is_empty());
}

#[test]
fn hole_face_selection_binds_to_the_feature_input_topology() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    let (feature, input_topologies, feature_id) = hole_face_case();
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        face:
            Some(FaceSelection::Historical {
                state,
                faces,
                native,
            }),
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Hole support face remains unresolved");
    };
    assert_eq!(native, scope_id);
    assert_eq!(
        state,
        &crate::ids::feature_input_topology_id(&feature_id, 1)
    );
    assert_eq!(faces.len(), 1);
    assert_eq!(input_topologies[0].faces.as_slice(), faces.as_slice());
}
