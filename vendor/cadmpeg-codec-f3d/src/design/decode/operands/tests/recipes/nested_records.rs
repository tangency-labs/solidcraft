// SPDX-License-Identifier: Apache-2.0
use super::parse_edge_operand;
use super::parse_face_operand;
use super::parse_vertex_recipe;
use crate::design::decode::operands::bind_edge_operand_candidates;
use crate::design::decode::operands::bind_face_operand_candidates;
use crate::design::decode::operands::face_recipe_program_kind;
use crate::design::decode::operands::FaceRecipeProgramKind;
use crate::design::face_resolve::resolved_face_group;
use crate::design::face_resolve::resolved_historical_split_face_target_group;
use crate::ids::feature_input_topology_id;
use crate::records::decal::DesignRecordHeader;
use crate::records::dimensions::DesignDimensionRecipeRecord;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::sketch_links::PersistentSubentityTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use crate::records::topology::edge_recipe::DesignTopologyRecipeSide;
use crate::records::topology::extrude_selection::DesignExtrudeFaceRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceRecipeNode;
use crate::records::topology::face::DesignFaceRecipeStructure;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::ids::FaceId;

#[test]
fn topology_operands_follow_consecutive_nested_records_to_their_recipes() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> u64 {
        let offset = u64::try_from(bytes.len()).expect("generated frame length fits u64");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        offset
    }

    let mut bytes = Vec::new();
    header(&mut bytes, *b"306", 100);
    let paired_at = header(&mut bytes, *b"259", 100);
    header(&mut bytes, *b"408", 101);
    header(&mut bytes, *b"414", 102);
    let recipe_record_at = header(&mut bytes, *b"423", 103);
    // A recipe prefix can contain header-shaped scalar bytes. The consumer's
    // exact closing index, not the first header-like run, closes the envelope.
    header(&mut bytes, *b"122", 0);
    let recipe_name_at = bytes.len() + 4;
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(b"edge_recipe_data");
    for value in [-1i32, -1, 2, 0, -1, 1, -1, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let next_at = header(&mut bytes, *b"306", 104);
    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#1".into(),
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 1,
            frame_length: 200,
            kind_offset: 1100,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 1080,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![1085],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::Fillet
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1200,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned()).unwrap(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: "f3d:Design/BulkStream.dat:construction-recipe#60".into(),
        byte_offset: u64_from_index(recipe_name_at),
        kind: ConstructionRecipeKind::Edge,
        design: None,
        recipe_index: 7,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 303,
            offset: recipe_record_at + 8,
        }),
    };

    let mut edge_operand = parse_edge_operand(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        None,
    )
    .expect("edge recipe operand");
    assert_eq!(edge_operand.record_index(), 100);
    assert_eq!(
        edge_operand.clone().into_draft().paired_byte_offset,
        paired_at
    );
    assert_eq!(edge_operand.recipe_record_index(), 103);
    assert_eq!(edge_operand.recipe_record_byte_offset(), recipe_record_at);
    assert_eq!(edge_operand.recipe_id, recipe.id);
    assert_eq!(edge_operand.resolved_edge_slot, None);
    bytes[usize::try_from(next_at).expect("fixture offset fits address space") + 7
        ..usize::try_from(next_at).expect("fixture offset fits address space") + 11]
        .copy_from_slice(&105u32.to_le_bytes());
    let mut work_point_scope = scope.clone();
    work_point_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::WorkPoint
                .try_into()
                .unwrap();
        })
        .unwrap();
    let work_point_operand = parse_edge_operand(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &work_point_scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        None,
    )
    .expect("WorkPoint edge recipe operand");
    assert_eq!(work_point_operand.next_record_index, 105);
    bytes[usize::try_from(next_at).expect("fixture offset fits address space") + 7
        ..usize::try_from(next_at).expect("fixture offset fits address space") + 11]
        .copy_from_slice(&107u32.to_le_bytes());
    let mut sweep_scope = scope.clone();
    sweep_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Sweep
                .try_into()
                .unwrap();
        })
        .unwrap();
    let sweep_operand = parse_edge_operand(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &sweep_scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        None,
    )
    .expect("Sweep edge recipe operand");
    assert_eq!(sweep_operand.next_record_index, 107);
    bytes[usize::try_from(next_at).expect("fixture offset fits address space") + 7
        ..usize::try_from(next_at).expect("fixture offset fits address space") + 11]
        .copy_from_slice(&160u32.to_le_bytes());
    assert_eq!(
        parse_edge_operand(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            0,
            &record,
            std::slice::from_ref(&recipe),
            None,
        ),
        None
    );
    assert_eq!(
        parse_edge_operand(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            0,
            &record,
            std::slice::from_ref(&recipe),
            Some(recipe.byte_offset),
        ),
        None
    );
    let terminal_group_operand = parse_edge_operand(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        Some(u64::try_from(bytes.len()).expect("generated stream length fits u64")),
    )
    .expect("terminal construction-group edge recipe operand");
    assert_eq!(terminal_group_operand.next_record_index, 160);
    assert_eq!(terminal_group_operand.next_byte_offset(), next_at);

    let mut vertex_bytes = Vec::new();
    header(&mut vertex_bytes, *b"369", 200);
    let vertex_paired_at = header(&mut vertex_bytes, *b"261", 200);
    header(&mut vertex_bytes, *b"408", 201);
    header(&mut vertex_bytes, *b"414", 202);
    let vertex_recipe_record_at = header(&mut vertex_bytes, *b"423", 203);
    let vertex_recipe_name_at = vertex_bytes.len() + 4;
    vertex_bytes.extend_from_slice(&18u32.to_le_bytes());
    vertex_bytes.extend_from_slice(b"vertex_recipe_data");
    for value in [-1i32, 3, 1, -1, 2, -1, 0, -1, 0, 0] {
        vertex_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let vertex_next_at = header(&mut vertex_bytes, *b"370", 205);
    let vertex_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#200".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("369".to_owned()).unwrap(),
        record_index: 200,
    };
    let vertex_recipe = ConstructionRecipe {
        id: "f3d:Design/BulkStream.dat:construction-recipe#200".into(),
        byte_offset: u64::try_from(vertex_recipe_name_at).expect("generated offset fits u64"),
        kind: ConstructionRecipeKind::Vertex,
        design: None,
        recipe_index: 9,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 303,
            offset: vertex_recipe_record_at + 8,
        }),
    };
    let parsed_vertex = parse_vertex_recipe(
        &vertex_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&vertex_bytes),
        crate::ids::native_stream(&scope.id).expect("scope stream"),
        &vertex_header,
        std::slice::from_ref(&vertex_recipe),
    )
    .expect("WorkPoint vertex recipe operand");
    assert_eq!(
        parsed_vertex.clone().into_draft().paired_byte_offset,
        vertex_paired_at
    );
    assert_eq!(parsed_vertex.recipe_record_index(), 203);
    assert_eq!(parsed_vertex.next_record_index(), 205);
    assert_eq!(parsed_vertex.next_byte_offset(), vertex_next_at);
    assert_eq!(
        parsed_vertex.recipe_program,
        [-1, 3, 1, -1, 2, -1, 0, -1, 0, 0]
    );
    edge_operand.terminal_reference_edge_slots = vec![vec![17], vec![18, 19]];
    assert_eq!(
        crate::design::edge_resolve::edge_operand_reference_edge_sets(&edge_operand)
            .collect::<Vec<_>>(),
        vec![&[17][..], &[18, 19][..]]
    );
    let reference_context = |reference_ordinal, changed_reference_edge_slots| {
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundary_ids: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundary_ids: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundary_ids: Vec::new(),
            shared_edge_slots: Vec::new(),
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots,
        }
    };
    edge_operand.recipe_reference_contexts = vec![
        reference_context(0, vec![17]),
        reference_context(1, vec![18, 19]),
    ];
    edge_operand.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(2).unwrap(),
        std::num::NonZeroU32::new(1).unwrap(),
        std::num::NonZeroU32::new(2).unwrap(),
    ]);
    assert_eq!(
        crate::design::edge_resolve::edge_operand_reference_edge_sets(&edge_operand)
            .collect::<Vec<_>>(),
        vec![&[18, 19][..], &[17][..], &[18, 19][..]]
    );
    edge_operand.recipe_reference_contexts = vec![
        reference_context(0, Vec::new()),
        reference_context(1, vec![17]),
    ];
    let mut second_changed_operand = edge_operand.clone();
    second_changed_operand.recipe_reference_contexts = vec![
        reference_context(0, Vec::new()),
        reference_context(1, vec![18]),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::changed_reference_edge_group_candidates(
                &[&edge_operand, &second_changed_operand],
                decode_ctx,
            )
        })
        .unwrap(),
        Some(vec![17, 18])
    );
    second_changed_operand.recipe_reference_contexts[0].changed_reference_edge_slots = vec![17];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::changed_reference_edge_group_candidates(
                &[&edge_operand, &second_changed_operand],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    edge_operand.recipe_reference_contexts.clear();
    edge_operand.local_topology_references = None;
    edge_operand.terminal_reference_edge_slots.clear();
    edge_operand.resolved_edge_slot = Some(17);
    assert_eq!(
        crate::design::edge_resolve::resolved_edge_operand(&edge_operand),
        None
    );
    edge_operand.resolved_edge_slot = None;
    edge_operand.changed_boundary_edge_slots = vec![17, 18];
    edge_operand.deleted_boundary_edge_slots = vec![17, 18];
    edge_operand.treatment_radius_candidates = vec![
        crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 17,
            radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).expect("checked fixture value"),
        },
        crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 18,
            radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).expect("checked fixture value"),
        },
    ];
    let second_operand = edge_operand.clone();
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &second_operand],
            3.0,
        ),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &second_operand],
            4.0,
        ),
        None
    );
    let mut chain_left = edge_operand.clone();
    chain_left.treatment_radius_candidates.push(
        crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 19,
            radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).expect("checked fixture value"),
        },
    );
    let mut chain_right = edge_operand.clone();
    chain_right.treatment_radius_candidates = vec![
        crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 19,
            radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).expect("checked fixture value"),
        },
        crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 20,
            radius: cadmpeg_ir::scalar::PositiveReal::new(3.0).expect("checked fixture value"),
        },
    ];
    chain_right.deleted_boundary_edge_slots = vec![19, 20];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&chain_left, &chain_right],
            3.0,
        ),
        Some(vec![17, 18, 19, 20])
    );
    let mut context_operand = edge_operand.clone();
    context_operand.treatment_radius_candidates.clear();
    context_operand.changed_boundary_edge_slots = vec![16, 17];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0,
        ),
        None
    );
    context_operand.changed_boundary_edge_slots.clear();
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0,
        ),
        Some(vec![17, 18])
    );
    context_operand.changed_boundary_edge_slots = vec![15, 16];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0,
        ),
        None
    );
    let mut resolved_operand = edge_operand.clone();
    resolved_operand.id = "resolved".into();
    resolved_operand.resolved_edge_slot = Some(17);
    let mut proven_operand = edge_operand.clone();
    proven_operand.resolved_edge_slot = Some(17);
    let recovered_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 1,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("288".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 926,
            }],
            lost_edge_references: vec!["f3d:Design/BulkStream.dat:lost-edge#1".into()],

            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 91,
                        offset: 950,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 978,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 982,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 960,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1_000,
        },
    )
    .unwrap();
    let recovered = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::edge_resolve::resolved_edge_group(
            &recovered_group,
            std::slice::from_ref(&recovered_group),
            std::slice::from_ref(&proven_operand),
            &[],
            Some(8),
            &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
                .expect("identity grammar"),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(matches!(
        recovered,
        cadmpeg_ir::features::EdgeSelection::Unresolved
    ));
    let mut terminal_group = recovered_group.clone();
    terminal_group.lost_edge_references.clear();
    terminal_group
        .try_set_members(
            vec![100, 104]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let mut terminal_resolved = proven_operand.clone();
    terminal_resolved.recipe_state_id = Some(8);
    let mut terminal_unresolved = proven_operand.clone();
    terminal_unresolved.id = "f3d:Design/BulkStream.dat:edge-operand#104".into();
    let mut draft = terminal_unresolved.into_draft();
    draft.record_index = 104;
    draft.recipe_record_index = draft.record_index + 3;
    terminal_unresolved =
        crate::records::topology::edge_identity::DesignEdgeOperand::try_new(draft).unwrap();
    terminal_unresolved.recipe_state_id = Some(8);
    terminal_unresolved.resolved_edge_slot = None;
    terminal_unresolved.changed_boundary_edge_slots.clear();
    terminal_unresolved.deleted_boundary_edge_slots.clear();
    terminal_unresolved.treatment_radius_candidates.clear();
    terminal_unresolved.recipe_selectors = vec![
        crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
            selector: 0,
            clauses: vec![None, None],
            incidence_matching_edge_slots: vec![18, 19],

            boundary_count_matching_edge_slots: vec![18, 19],
        },
    ];
    let terminal = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::edge_resolve::resolved_edge_group(
            &terminal_group,
            std::slice::from_ref(&terminal_group),
            &[terminal_resolved, terminal_unresolved.clone()],
            &[],
            None,
            &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
                .expect("identity grammar"),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(
        matches!(terminal, cadmpeg_ir::features::EdgeSelection::Native(_)),
        "{terminal:?}"
    );
    let identity = |record_index, ordinal, edge| {
        DesignEdgeIdentityOperand::try_new(
            crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft {
                id: format!("f3d:Design/BulkStream.dat:edge-identity#{record_index}"),
                scope_record_index: 1,
                group_record_index: 90,
                group_member_ordinal: ordinal,
                record_index,
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                    .unwrap(),
                layout: crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full,
                local_id: u64::from(record_index),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: u64::from(record_index) + 42,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: u64::from(record_index) + 118,
                historical: None,
                treatment_radius_candidates: Vec::new(),
                transition_edge_candidates: Vec::new(),
                resolved_edge_slots: Vec::new(),
                resolved_edge_slot: edge,
                resolution_identity_id: None,
            },
        )
        .unwrap()
    };
    let mut recipe_unresolved = proven_operand.clone();
    recipe_unresolved.resolved_edge_slot = None;
    recipe_unresolved.recipe_state_id = Some(8);
    recipe_unresolved.changed_boundary_edge_slots.clear();
    let merged = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::edge_resolve::resolved_edge_group(
            &terminal_group,
            std::slice::from_ref(&terminal_group),
            &[recipe_unresolved.clone(), terminal_unresolved.clone()],
            &[identity(100, 0, Some(17)), identity(104, 1, None)],
            Some(8),
            &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
                .expect("identity grammar"),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(matches!(
        merged,
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let complete = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::edge_resolve::resolved_edge_group(
            &terminal_group,
            std::slice::from_ref(&terminal_group),
            &[recipe_unresolved.clone(), terminal_unresolved],
            &[identity(100, 0, Some(17)), identity(104, 1, Some(18))],
            Some(8),
            &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
                .expect("identity grammar"),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(matches!(
        complete,
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let mut first_rule = identity(100, 0, None);
    first_rule.resolved_edge_slots = vec![17, 18];
    let mut second_rule = identity(104, 1, None);
    second_rule.resolved_edge_slots = vec![18, 19];
    let face_rules = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::edge_resolve::resolved_edge_group(
            &terminal_group,
            std::slice::from_ref(&terminal_group),
            &[recipe_unresolved.clone()],
            &[first_rule, second_rule],
            Some(8),
            &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
                .expect("identity grammar"),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(matches!(
        face_rules,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges.as_slice() == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:8:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:8:18").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:8:19").expect("identity grammar"),
            ]
    ));
    let mut chain_group = terminal_group.clone();
    chain_group
        .try_set_members(
            vec![100]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let mut chain_recipe = recipe_unresolved.clone();
    chain_recipe.changed_boundary_edge_slots = vec![17, 18];
    let mut chain_identity = identity(100, 0, None);
    chain_identity.transition_edge_candidates = vec![18, 17];
    let chain = crate::design::edge_resolve::resolved_edge_treatment_group(
        &chain_group,
        std::slice::from_ref(&chain_group),
        &[chain_recipe],
        &[chain_identity],
        Some(8),
        &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
            .expect("identity grammar"),
        None,
    )
    .unwrap();
    assert!(matches!(
        chain,
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    assert_eq!(
        edge_operand.recipe_program_offset,
        u64_from_index(recipe_name_at) + 16
    );
    assert_eq!(edge_operand.recipe_program, [-1, -1, 2, 0, -1, 1, -1, 7]);
    assert!(edge_operand.recipe_structure.is_none());
    let structured = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 1, 1, 5, 4, 4, 4, 4, 3,
                4, -1, 3, 0, -1, 1, -1, 3, -1, 0, 1, 2, 5, 3, 3, 3, 1, 1, 1, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("standard two-side recipe structure");
    assert_eq!(structured.root, 2);
    assert_eq!(structured.sides[0].field_count(), 3);
    assert_eq!(structured.sides[0].header_value, 0);
    assert_eq!(structured.sides[0].scalars, [2, 1]);
    assert_eq!(structured.sides[0].entries.len(), 1);
    assert_eq!(structured.sides[0].entries[0].selector, 1);
    assert_eq!(structured.sides[0].entries[0].boundary_edge_count.get(), 5);
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0]
            .outer
            .get(),
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].middle,
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].vertex_ordinal(),
        3
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal),
        Some(3)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0]
            .incident
            .map(|incident| incident.side),
        Some(crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Following)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1]
            .outer
            .get(),
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].middle,
        3
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1]
            .incident
            .map(|incident| incident.ordinal),
        Some(2)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1]
            .incident
            .map(|incident| incident.side),
        Some(crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding)
    );
    assert_eq!(structured.sides[1].field_count(), 3);
    assert_eq!(structured.sides[1].header_value, 0);
    assert_eq!(structured.sides[1].scalars, [1, 3]);
    assert_eq!(structured.sides[1].entries.len(), 1);
    assert_eq!(structured.sides[1].entries[0].selector, 2);
    assert_eq!(structured.sides[1].entries[0].boundary_edge_count.get(), 5);
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[0]
            .outer
            .get(),
        3
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[0].middle,
        3
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[1]
            .outer
            .get(),
        1
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[1].middle,
        1
    );
    assert_eq!(
        crate::design::decode::operands::edge_recipe_local_topology_references(&structured, 3),
        Some(
            [2, 1, 1, 3]
                .into_iter()
                .map(|value| std::num::NonZeroU32::new(value).unwrap())
                .collect()
        )
    );
    assert!(
        crate::design::decode::operands::edge_recipe_local_topology_references(&structured, 2)
            .is_none()
    );
    let signed_middle =
        crate::design::decode::operands::edge_recipe_entries(&[1, 4, 1, -2, 1, 4, 4, 4])
            .expect("signed topology middle is retained");
    assert_eq!(signed_middle[0].topology_triplets[0].middle, -2);
    assert_eq!(
        signed_middle[0].topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal),
        None
    );
    let signed_face = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, -2, 1, 4, 4, 4, -1, 3,
                0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("signed face-node topology recipe structure");
    assert_eq!(
        signed_face.sides[0].entries[0].topology_triplets[0].middle,
        -2
    );
    assert_eq!(
        signed_face.sides[0].entries[0].topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal),
        None
    );
    let postlude_face = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, -2, 1, 4, 4, 4, -1, 3,
                0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4, -1, 4, -1, 0, 0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("face-node topology postlude");
    assert_eq!(postlude_face.postlude_value, Some(4));
    let unambiguous_payload_face = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 1, -1, 0, -1, 0, 2, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                1, 1, 1, 1, 1, -1, 3, 0, -1, 0, -1, 1, -1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("face-node payload prefix grammar");
    assert_eq!(unambiguous_payload_face.sides[0].entries.len(), 2);
    let ambiguous_extended_payload_face = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 1, -1, 0, -1, 0, 2, -1, 1, 1, 1, 1, 1, 1, 1, 1, -1,
                3, 0, -1, 1, -1, 0, -1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1,
            ],
        )
        .expect("recipe structure")
    });
    assert!(ambiguous_extended_payload_face.is_none());
    let mut referenced_headers = structured.clone();
    referenced_headers.sides[0].header_value = 2;
    referenced_headers.sides[1].header_value = 3;
    assert_eq!(
        crate::design::decode::operands::edge_recipe_local_topology_references(
            &referenced_headers,
            3
        ),
        Some(
            [2, 2, 1, 3, 1, 3]
                .into_iter()
                .map(|value| std::num::NonZeroU32::new(value).unwrap())
                .collect()
        )
    );
    let wrap =
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 1, 0, 1, 1, 1, 1]).unwrap();
    assert_eq!(wrap[0].topology_triplets[0].vertex_ordinal(), 0);
    assert_eq!(
        wrap[0].topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal),
        Some(4)
    );
    assert_eq!(wrap[0].common_incident_edge_ordinal(), None);
    assert_eq!(
        wrap[0].topology_triplets[0]
            .incident
            .map(|incident| incident.side),
        Some(crate::records::topology::edge_recipe::DesignTopologyIncidentSide::Preceding)
    );
    let common =
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 1, 1, 1, 1, 1, 1]).unwrap();
    assert_eq!(common[0].common_incident_edge_ordinal(), Some(0));
    let underived =
        crate::design::decode::operands::edge_recipe_entries(&[0, 6, 6, 4, 6, 1, 1, 1]).unwrap();
    assert_eq!(underived[0].topology_triplets[0].vertex_ordinal(), 5);
    assert_eq!(
        underived[0].topology_triplets[0]
            .incident
            .map(|incident| incident.ordinal),
        None
    );
    assert_eq!(
        underived[0].topology_triplets[0]
            .incident
            .map(|incident| incident.side),
        None
    );
    assert_eq!(
        crate::design::decode::operands::edge_recipe_entries(&[3, 5, 1, 1, 1, 2, 1, 2]).unwrap()[0]
            .selector,
        3
    );
    assert!(
        crate::design::decode::operands::edge_recipe_entries(&[-1, 5, 1, 1, 1, 2, 1, 2]).is_none()
    );
    assert!(
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 6, 5, 6, 2, 1, 2]).is_none()
    );
    assert!(crate::design::decode::operands::edge_recipe_entries(&[
        1, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,
    ])
    .is_none());
    assert!(crate::design::decode::operands::edge_recipe_entries(&[
        2, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,
    ])
    .is_none());
    let extended = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 2, -1, 1, -1, 0, -1, 0, 0, -1, 4, 3, -1, 0, -1,
                1, -1, 4, -1, 0, 0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("recipe structure with a third scalar on its second side");
    assert_eq!(extended.sides[0].scalars, [1, 0]);
    assert_eq!(extended.sides[1].scalars, [0, 1, 4]);
    assert_eq!(extended.sides[1].field_count(), 4);
    assert!(extended.sides[0].entries.is_empty());
    assert!(extended.sides[1].entries.is_empty());
    let zero_delimited = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, 0, 1, 0, 2, -1, 3, 1, 0, 0, 0, 2, 0, 0, 0, -1, 4, 1, 0, 3, 0, 4, 0,
                0, 0, 0, 1, 2, 3, 2, 1, 2, 1, 1, 1, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("recipe structure with zero-delimited side fields");
    assert_eq!(zero_delimited.root, 2);
    assert_eq!(zero_delimited.sides[0].field_count(), 3);
    assert_eq!(zero_delimited.sides[0].header_value, 1);
    assert_eq!(zero_delimited.sides[0].scalars, [0, 2]);
    assert!(zero_delimited.sides[0].entries.is_empty());
    assert_eq!(zero_delimited.sides[1].field_count(), 4);
    assert_eq!(zero_delimited.sides[1].scalars, [3, 4, 0]);
    assert_eq!(zero_delimited.sides[1].entries.len(), 1);
    assert_eq!(zero_delimited.sides[1].entries[0].selector, 2);
    assert_eq!(
        zero_delimited.sides[1].entries[0].boundary_edge_count.get(),
        3
    );
    let mixed_delimiters = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, 0, 1, -1, 2, -1, 3, 2, 0, 1, -1, 0, 0, 0, 0, -1, 3, 0, 0, 1, -1, 3,
                0, 0, 0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("recipe structure with field-local delimiters");
    assert_eq!(mixed_delimiters.root, 2);
    assert_eq!(mixed_delimiters.sides[0].header_value, 2);
    assert_eq!(mixed_delimiters.sides[0].scalars, [1, 0]);
    assert_eq!(mixed_delimiters.sides[1].header_value, 0);
    assert_eq!(mixed_delimiters.sides[1].scalars, [1, 3]);
    let revolution_axis = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, 0, 1, 0, 2, -1, 3, 0, 0, 2, -1, 1, 0, 0, 1, 1, 7, 1, 1, 1, 4, 4, 4,
                -1, 3, 0, 0, 1, 0, 3, 0, 0, 0, 0,
            ],
        )
        .expect("recipe structure")
    })
    .expect("revolution-axis edge recipe structure");
    assert_eq!(revolution_axis.sides[0].scalars, [2, 1]);
    assert_eq!(revolution_axis.sides[0].entries.len(), 1);
    assert!(revolution_axis.sides[1].entries.is_empty());
    let variable_scalars = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, -1, 1, -1, 2, -1, 5, 1, -1, 0, -1, 2, -1, 3, -1, 4, -1, 0, 0, -1, 3,
                0, -1, 1, -1, 2, -1, 0, 0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("recipe structure with four scalar fields");
    assert_eq!(variable_scalars.sides[0].field_count(), 5);
    assert_eq!(variable_scalars.sides[0].scalars, [0, 2, 3, 4]);
    let extended_payload = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::edge_recipe_structure(
            ctx,
            &[
                -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 2, 3, -1, 0, 0, -1, 4, -1,
                0, 0, -1, 1, 0, 4, 1, 1, 1, 2, 2, 2, -1, 3, 0, -1, 1, -1, 2, -1, 0, 0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("recipe structure with an extended payload field program");
    assert_eq!(
        extended_payload.sides[0].payload_prefix,
        [2, 3, -1, 0, 0, -1, 4, -1, 0, 0, -1]
    );
    assert_eq!(extended_payload.sides[0].entries.len(), 1);
    let surface_patch = crate::design::decode::operands::surface_patch_recipe_structure(
        &[
            0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0, 2,
            1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0, -1,
        ],
        4,
    )
    .expect("SurfacePatch two-clause recipe structure");
    assert_eq!(surface_patch.clauses.len(), 2);
    assert_eq!(surface_patch.clauses[0].face_reference_ordinals, [2, 1]);
    assert_eq!(surface_patch.clauses[0].edge_reference_ordinals, [0, 2]);
    assert_eq!(surface_patch.clauses[0].entries.len(), 1);
    assert_eq!(surface_patch.clauses[0].entries[0].selector, 0);
    assert_eq!(surface_patch.clauses[1].face_reference_ordinals, [3, 1]);
    assert_eq!(surface_patch.clauses[1].edge_reference_ordinals, [0, 3]);
    assert_eq!(surface_patch.clauses[1].entries.len(), 0);
    assert!(surface_patch.clauses[1].entries.is_empty());
    assert!(
        crate::design::decode::operands::surface_patch_recipe_structure(
            &[
                0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0,
                2, 1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0,
                -1,
            ],
            3,
        )
        .is_none()
    );
    assert!(
        crate::design::decode::operands::surface_patch_recipe_structure(
            &[
                0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0,
                2, 1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0,
                7,
            ],
            4,
        )
        .is_none()
    );
    let mut surface_patch_operand = edge_operand.clone();
    surface_patch_operand.surface_patch_recipe_structure = Some(surface_patch.clone());
    surface_patch_operand.recipe_state_id = Some(8);
    surface_patch_operand.resolved_edge_slot = Some(17);
    let mut surface_patch_group = terminal_group.clone();
    surface_patch_group
        .try_set_members(
            vec![100]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let surface_selection = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::edge_resolve::resolved_edge_group(
            &surface_patch_group,
            std::slice::from_ref(&surface_patch_group),
            std::slice::from_ref(&surface_patch_operand),
            &[],
            Some(8),
            &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch")
                .expect("identity grammar"),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(matches!(
        surface_selection,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges.as_slice() == [cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#13:surface-patch:8:17").expect("identity grammar")]
    ));
    surface_patch_operand.resolved_edge_slot = None;
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::edge_resolve::resolved_edge_group(
                &surface_patch_group,
                std::slice::from_ref(&surface_patch_group),
                std::slice::from_ref(&surface_patch_operand),
                &[],
                Some(8),
                &cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#surface-patch")
                    .expect("identity grammar"),
                decode_ctx,
            )
        })
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let face = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 3, -1, 0,
                0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("face node topology recipe structure");
    assert_eq!(face.root, 0);
    assert_eq!(face.prelude, [1, 2]);
    assert_eq!(face.sides[0].field_count(), 3);
    assert_eq!(face.sides[0].header_value, 0);
    assert_eq!(face.sides[0].scalars, [2, 1]);
    assert_eq!(face.sides[1].field_count(), 3);
    assert_eq!(face.sides[1].header_value, 0);
    assert_eq!(face.sides[1].scalars, [1, 3]);
    let zero_delimited_face = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, 0, 1, 0, 2, -1, 3, 0, 0, 2, 0, 1, 0, 0, 0, -1, 3, 0, 0, 1, 0, 3, 0, 0, 0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("zero-delimited face node topology recipe structure");
    assert_eq!(zero_delimited_face, face);
    assert_eq!(edge_operand.next_record_index, 104);
    assert_eq!(edge_operand.next_byte_offset(), next_at);
    bind_edge_operand_candidates(
        std::slice::from_mut(&mut edge_operand),
        std::slice::from_ref(&recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:asm:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("3").unwrap(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#xref").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("3").unwrap(),
                design_references: vec![303],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        edge_operand.candidate_faces,
        [FaceId::mint("f3d:brep:entity#50").expect("identity grammar")]
    );
    let mut local_recipe = recipe.clone();
    local_recipe.record_index =
        local_recipe
            .record_index
            .map(|index| crate::records::identity::RecordedValue {
                value: -1335,
                ..index
            });
    bind_edge_operand_candidates(
        std::slice::from_mut(&mut edge_operand),
        std::slice::from_ref(&local_recipe),
        &[PersistentSubentityTag {
            id: "f3d:asm:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("3").unwrap(),
            design_references: vec![303],
            ordinal: 0,
        }],
    );
    assert!(edge_operand.candidate_faces.is_empty());
    let mut embedded_program = vec![99];
    embedded_program.extend_from_slice(&edge_operand.recipe_program[7..]);
    embedded_program.push(88);
    let dimension_recipe = DesignDimensionRecipeRecord {
        id: "f3d:Design/BulkStream.dat:dimension-recipe#1".into(),
        companion_record_index: 1,
        recipe_ordinal: 0,
        recipe_id: "recipe".into(),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned()).unwrap(),
        record_index: 1,
        frame_length: 4,
        prefix_offset: 0,
        prefix_bytes: vec![1].into(),
        references: Vec::new(),
        program_offset: 0,
        program: embedded_program,
        matching_edge_operand_ids: Vec::new(),
    };
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            std::slice::from_ref(&edge_operand),
        ),
        [edge_operand.id.clone()]
    );
    let mut other_stream_operand = edge_operand.clone();
    other_stream_operand.id = "f3d:Other/BulkStream.dat:edge-operand#100".into();
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            &[edge_operand.clone(), other_stream_operand],
        ),
        [edge_operand.id.clone()]
    );

    let mut counted_operand = edge_operand.clone();
    counted_operand.recipe_program =
        [&[0, -1, 1, 1, 0][..], &edge_operand.recipe_program[7..]].concat();
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            std::slice::from_ref(&counted_operand),
        ),
        [counted_operand.id.clone()]
    );
    let mut mismatched_dimension_recipe = dimension_recipe.clone();
    mismatched_dimension_recipe.program[1] += 1;
    assert!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &mismatched_dimension_recipe,
            std::slice::from_ref(&counted_operand),
        )
        .is_empty()
    );
    counted_operand.recipe_program[3] = 2;
    assert!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            std::slice::from_ref(&counted_operand),
        )
        .is_empty()
    );

    let mut face_bytes = Vec::new();
    header(&mut face_bytes, *b"306", 100);
    let face_paired_at = header(&mut face_bytes, *b"259", 100);
    header(&mut face_bytes, *b"408", 101);
    header(&mut face_bytes, *b"414", 102);
    let face_recipe_record_at = header(&mut face_bytes, *b"423", 103);
    let face_recipe_name_at = face_bytes.len() + 4;
    face_bytes.extend_from_slice(&24u32.to_le_bytes());
    face_bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, -1, -1, 2, 7, -1, -1, 2, 8, -1, -1, 2, 9] {
        face_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let face_next_at = header(&mut face_bytes, *b"306", 104);
    let mut face_scope = scope;
    face_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Extrude
                .try_into()
                .unwrap();
        })
        .unwrap();
    let mut face_recipe = recipe;
    face_recipe.kind = ConstructionRecipeKind::BoundedFace;
    face_recipe.design = Some(crate::records::recipes::ConstructionRecipeDesign {
        id: crate::records::identity::RecordedValue {
            value: "303".into(),
            offset: 0,
        },
        selector: None,
    });
    face_recipe.byte_offset = u64_from_index(face_recipe_name_at);
    face_recipe.record_index =
        face_recipe
            .record_index
            .map(|index| crate::records::identity::RecordedValue {
                offset: face_recipe_record_at + 8,
                ..index
            });
    let mut operand = parse_face_operand(
        &face_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&face_bytes),
        &face_scope,
        (0, None),
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe operand");
    assert_eq!(operand.record_index(), 100);
    assert_eq!(
        operand.clone().into_draft().paired_byte_offset,
        face_paired_at
    );
    assert_eq!(operand.recipe_record_index(), 103);
    assert_eq!(operand.recipe_kind, ConstructionRecipeKind::BoundedFace);
    assert_eq!(operand.recipe_id, face_recipe.id);
    assert!(operand.resolved_face_slots.is_empty());
    assert_eq!(
        operand.recipe_program_offset,
        u64_from_index(face_recipe_name_at) + 24
    );
    assert_eq!(operand.recipe_program[0..3], [0, -1, 4]);
    let face_program_at = face_recipe_name_at + 24;
    face_bytes[face_program_at + 4..face_program_at + 8].copy_from_slice(&0i32.to_le_bytes());
    let zero_prelude = parse_face_operand(
        &face_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&face_bytes),
        &face_scope,
        (0, None),
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("zero-prelude face recipe operand");
    assert_eq!(zero_prelude.recipe_program[0..3], [0, 0, 4]);
    assert_eq!(
        face_recipe_program_kind(&zero_prelude.recipe_program),
        Some(FaceRecipeProgramKind::Counted { header_value: 4 })
    );
    assert_eq!(
        operand
            .recipe_nodes
            .iter()
            .map(|node| node.byte_offset)
            .collect::<Vec<_>>(),
        [
            u64_from_index(face_recipe_name_at) + 36,
            u64_from_index(face_recipe_name_at) + 52,
            u64_from_index(face_recipe_name_at) + 68,
        ]
    );
    assert_eq!(operand.recipe_nodes.len(), 3);
    assert_eq!(
        operand.recipe_nodes[0].byte_offset,
        u64_from_index(face_recipe_name_at) + 36
    );
    assert_eq!(
        operand.recipe_nodes[0].end_byte_offset,
        u64_from_index(face_recipe_name_at) + 52
    );
    assert_eq!(operand.recipe_nodes[0].program, [-1, -1, 2, 7]);
    assert_eq!(operand.next_record_index, 104);
    assert_eq!(operand.next_byte_offset(), face_next_at);

    let mut prelude_bytes = face_bytes.clone();
    let prelude_words = [4i32, 5, 6, 7];
    let prelude_bytes_at = prelude_words
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    prelude_bytes.splice(
        face_program_at + 12..face_program_at + 12,
        prelude_bytes_at.iter().copied(),
    );
    let prelude = parse_face_operand(
        &prelude_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&prelude_bytes),
        &face_scope,
        (0, None),
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe operand with counted prelude");
    assert_eq!(prelude.recipe_program[0..7], [0, 0, 4, 4, 5, 6, 7]);
    assert_eq!(
        prelude.recipe_nodes[0].byte_offset,
        prelude.recipe_program_offset + 28
    );
    assert_eq!(prelude.recipe_nodes[0].program, [-1, -1, 2, 7]);

    let enclosing_limit = header(&mut face_bytes, *b"306", 105);
    let bounded = parse_face_operand(
        &face_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&face_bytes),
        &face_scope,
        (0, None),
        Some(enclosing_limit),
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe bounded before its enclosing member limit");
    assert_eq!(bounded.next_record_index, 104);
    assert_eq!(bounded.next_byte_offset(), face_next_at);

    let mut compact_bytes = Vec::new();
    header(&mut compact_bytes, *b"306", 100);
    header(&mut compact_bytes, *b"259", 100);
    header(&mut compact_bytes, *b"408", 101);
    header(&mut compact_bytes, *b"414", 102);
    let compact_record_at = header(&mut compact_bytes, *b"423", 103);
    let compact_name_at = compact_bytes.len() + 4;
    compact_bytes.extend_from_slice(&24u32.to_le_bytes());
    compact_bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, 1, -1, 1, 0, -1] {
        compact_bytes.extend_from_slice(&value.to_le_bytes());
    }
    header(&mut compact_bytes, *b"306", 104);
    let mut compact_recipe = face_recipe.clone();
    compact_recipe.byte_offset = u64_from_index(compact_name_at);
    compact_recipe.record_index =
        compact_recipe
            .record_index
            .map(|index| crate::records::identity::RecordedValue {
                offset: compact_record_at + 8,
                ..index
            });
    let compact = parse_face_operand(
        &compact_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&compact_bytes),
        &face_scope,
        (0, None),
        None,
        &record,
        std::slice::from_ref(&compact_recipe),
    )
    .expect("compact face recipe operand");
    assert_eq!(compact.recipe_program, [0, -1, 4, 1, -1, 1, 0, -1]);
    assert!(compact.recipe_nodes.is_empty());

    let terminal_program_at = compact_name_at + 24;
    compact_bytes.truncate(terminal_program_at);
    for value in [0i32, -1] {
        compact_bytes.extend_from_slice(&value.to_le_bytes());
    }
    header(&mut compact_bytes, *b"306", 104);
    let terminal = parse_face_operand(
        &compact_bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&compact_bytes),
        &face_scope,
        (0, None),
        None,
        &record,
        std::slice::from_ref(&compact_recipe),
    )
    .expect("terminal face recipe operand");
    assert_eq!(terminal.recipe_program, [0, -1]);
    assert!(terminal.recipe_nodes.is_empty());
    assert_eq!(
        face_recipe_program_kind(&terminal.recipe_program),
        Some(FaceRecipeProgramKind::Terminal)
    );
    assert_eq!(
        face_recipe_program_kind(&[0, 0]),
        Some(FaceRecipeProgramKind::Terminal)
    );
    assert_eq!(face_recipe_program_kind(&[0, 1, 4]), None);
    assert_eq!(face_recipe_program_kind(&[0, -1, 0]), None);
    operand.recipe_references.push(DesignRecipeReference {
        selector: 1,
        selector_offset: 1_101,
        token: "3".into(),
        token_offset: 1,
        design_reference: 303,
        design_reference_offset: 2,
        candidate_faces: Vec::new(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    });
    bind_face_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&face_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("3").unwrap(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#51").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("4").unwrap(),
                design_references: vec![303],
                ordinal: 1,
            },
            PersistentSubentityTag {
                id: "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#xref").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("3").unwrap(),
                design_references: vec![303],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        operand.candidate_faces,
        [
            FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
            FaceId::mint("f3d:brep:entity#51").expect("identity grammar")
        ]
    );
    assert_eq!(
        operand.unreferenced_candidate_faces,
        [FaceId::mint("f3d:brep:entity#51").expect("identity grammar")]
    );
    let mut direct_face = operand.clone();
    direct_face.recipe_kind = ConstructionRecipeKind::Face;
    direct_face.recipe_references = vec![DesignRecipeReference {
        selector: 1,
        selector_offset: 1_201,
        token: "3".into(),
        token_offset: 1_202,
        design_reference: 303,
        design_reference_offset: 1_203,
        candidate_faces: vec![FaceId::mint("f3d:brep:entity#50").expect("identity grammar")],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }];
    direct_face.alternate_selector_candidate_faces.clear();
    direct_face.resolved_face_slots.clear();
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: face_scope.record_index,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: operand.record_index(),
                offset: 924,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 920,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 91,
                        offset: 935,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 964,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 968,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                encoding: crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
                usage: DesignExtrudeFaceRole::Termination,
            },
            role_offset: 946,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 980,
        },
    )
    .unwrap();
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&direct_face)).expect("resolve the face group"),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep:entity#50").expect("identity grammar")] && native == group.id
    ));
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)).expect("resolve the face group"),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep:entity#51").expect("identity grammar")] && native == group.id
    ));
    operand
        .unreferenced_candidate_faces
        .push(FaceId::mint("f3d:brep:entity#50").expect("identity grammar"));
    assert!(resolved_face_group(&group, std::slice::from_ref(&operand))
        .expect("resolve the face group")
        .is_none());
    operand.recipe_program = vec![0, -1, 1];
    operand.recipe_kind = ConstructionRecipeKind::BoundedFace;
    let context_reference = operand.recipe_references[0].clone();
    for ordinal in 1..4 {
        let mut reference = context_reference.clone();
        reference.selector_offset += ordinal * 25;
        reference.token_offset += ordinal * 25;
        reference.design_reference_offset += ordinal * 25;
        operand.recipe_references.push(reference);
    }
    operand.recipe_nodes.clear();
    operand.recipe_nodes.push(DesignFaceRecipeNode {
        byte_offset: 1_200,
        end_byte_offset: 1_300,
        program: Vec::new(),
        recipe_structure: Some(DesignFaceRecipeStructure {
            root: 0,
            prelude: [0, 2],
            sides: [
                DesignTopologyRecipeSide {
                    header_value: 0,
                    scalars: vec![0, 1],
                    payload_prefix: vec![0],

                    entries: Vec::new(),
                },
                DesignTopologyRecipeSide {
                    header_value: 1,
                    scalars: vec![1, 0],
                    payload_prefix: vec![0],

                    entries: Vec::new(),
                },
            ],
            postlude_value: None,
        }),
    });
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)).expect("resolve the face group"),
        Some(FaceSelection::Resolved { faces, native })
            if faces == operand.unreferenced_candidate_faces && native == group.id
    ));
    operand.recipe_nodes[0].recipe_structure = None;
    assert!(resolved_face_group(&group, std::slice::from_ref(&operand))
        .expect("resolve the face group")
        .is_none());
    operand.preceding_candidate_faces =
        vec![FaceId::mint("f3d:brep:entity#50").expect("identity grammar")];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&operand),
        Some(50)
    );
    operand.resolved_face_slots = vec![50];
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)).expect("resolve the face group"),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep:entity#50").expect("identity grammar")] && native == group.id
    ));
    let mut namespaced_slot = operand.clone();
    namespaced_slot.candidate_faces =
        vec![FaceId::mint("f3d:brep/example.smbh/brep:entity#50").expect("identity grammar")];
    namespaced_slot.unreferenced_candidate_faces.clear();
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&namespaced_slot)).expect("resolve the face group"),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep/example.smbh/brep:entity#50").expect("identity grammar")]
                && native == group.id
    ));
    namespaced_slot.resolved_face_slots = vec![51];
    assert!(
        resolved_face_group(&group, std::slice::from_ref(&namespaced_slot))
            .expect("resolve the face group")
            .is_none()
    );
    let mut historical_face_scope = face_scope.clone();
    historical_face_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(49);
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(matches!(
        crate::design::feature_project::direct_face_selection(&historical_face_scope, std::slice::from_ref(&operand)),
        Some(FaceSelection::Historical { state, faces, native })
            if state == feature_input_topology_id(&crate::ids::neutral_feature_id(&historical_face_scope), 49)
                && faces.len() == 1
                && faces[0].as_str().ends_with(":49:50")
                && native.as_str() == historical_face_scope.id
    ));
    operand.resolved_face_slots.clear();
    assert!(crate::design::face_resolve::retain_face_operand_resolution(
        &group,
        std::slice::from_mut(&mut operand),
        &FaceId::mint("f3d:brep:entity#50").expect("identity grammar")
    ));
    assert_eq!(operand.resolved_face_slots, [50]);
    operand.resolved_face_slots.clear();
    operand.alternate_selector_candidate_faces = vec![
        FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
        FaceId::mint("f3d:brep:entity#51").expect("identity grammar"),
    ];
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)).expect("resolve the face group"),
        Some(FaceSelection::Resolved { faces, native })
            if faces == operand.alternate_selector_candidate_faces && native == group.id
    ));
    operand.alternate_selector_candidate_faces.clear();
    operand.resolved_face_slots = vec![50];
    let mut ambiguous = [operand.clone(), operand.clone()];
    assert!(
        !crate::design::face_resolve::retain_face_operand_resolution(
            &group,
            &mut ambiguous,
            &FaceId::mint("f3d:brep:entity#50").expect("identity grammar")
        )
    );

    let split_structure = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::face_recipe_structure(
            ctx,
            &[
                0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 3, -1, 0,
                0, -1,
            ],
        )
        .expect("recipe structure")
    })
    .expect("split-face context recipe structure");
    let mut split_scope = face_scope.clone();
    split_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::SplitFace
                .try_into()
                .unwrap();
            draft.previous_history_state_id = Some(49);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut split_group = group.clone();
    split_group.scope_reference_ordinal = 2;
    split_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X10,
        );
    split_group
        .try_set_members(
            vec![operand.record_index(), operand.record_index() + 1]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let mut split_selected = operand.clone();
    split_selected.group = Some(crate::records::topology::body_recipe::DesignOperandGroup {
        group_record_index: split_group.record_index,
        group_member_ordinal: 0,
    });
    split_selected.resolved_face_slots = vec![50];
    for node in &mut split_selected.recipe_nodes {
        node.recipe_structure = Some(split_structure.clone());
    }
    let mut split_context = split_selected.clone();
    let mut draft = split_context.into_draft();
    draft.record_index += 1;
    draft.recipe_record_index = draft.record_index + 3;
    split_context = crate::records::topology::face::DesignFaceOperand::try_new(draft).unwrap();
    if let Some(group) = &mut split_context.group {
        group.group_member_ordinal = 1;
    }
    split_context.candidate_faces.clear();
    split_context.unreferenced_candidate_faces.clear();
    split_context.alternate_selector_candidate_faces.clear();
    split_context.preceding_candidate_faces.clear();
    split_context.changed_candidate_faces.clear();
    split_context.resolved_face_slots.clear();
    let nested_candidate = split_context
        .recipe_references
        .iter()
        .flat_map(|reference| {
            reference
                .candidate_faces
                .iter()
                .chain(&reference.alternate_selector_faces)
        })
        .next()
        .cloned()
        .expect("nested bounded-face candidate");
    let nested_slot = nested_candidate
        .as_str()
        .rsplit_once('#')
        .and_then(|(_, slot)| slot.parse::<i64>().ok())
        .expect("nested bounded-face slot");
    split_context.changed_candidate_faces =
        vec![FaceId::mint("f3d:brep:entity#999").expect("identity grammar")];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&split_context),
        None
    );
    split_context.changed_candidate_faces = vec![nested_candidate];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&split_context),
        Some(nested_slot)
    );
    split_context.changed_candidate_faces.clear();
    assert!(matches!(
        resolved_historical_split_face_target_group(&split_scope, split_scope.previous_history_state_id(), &split_group, &[split_selected.clone(), split_context.clone()]),
        Some(FaceSelection::Historical { state, faces, native })
            if state == feature_input_topology_id(&crate::ids::neutral_feature_id(&split_scope), 49)
                && faces.len() == 1
                && faces[0].as_str().ends_with(":49:50")
                && native.as_str() == split_group.id
    ));
    let mut candidate_context = split_context.clone();
    candidate_context.candidate_faces =
        vec![FaceId::mint("f3d:brep:entity#50").expect("identity grammar")];
    candidate_context.unreferenced_candidate_faces = candidate_context.candidate_faces.clone();
    candidate_context.preceding_candidate_faces = candidate_context.candidate_faces.clone();
    candidate_context
        .recipe_nodes
        .push(candidate_context.recipe_nodes[0].clone());
    candidate_context.recipe_program = vec![0, -1, 2];
    assert!(resolved_historical_split_face_target_group(
        &split_scope,
        split_scope.previous_history_state_id(),
        &split_group,
        &[split_selected.clone(), candidate_context],
    )
    .is_some());
    let mut unresolved_context = split_context;
    for reference in &mut unresolved_context.recipe_references {
        reference.candidate_faces.clear();
        reference.alternate_selector_faces.clear();
    }
    assert!(resolved_historical_split_face_target_group(
        &split_scope,
        split_scope.previous_history_state_id(),
        &split_group,
        &[split_selected, unresolved_context],
    )
    .is_none());
}
