// SPDX-License-Identifier: Apache-2.0
//! Decode-module projection and completeness unit tests.

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

use cadmpeg_test_support::wire;

use super::super::{
    bind_mesh_feature_definitions, face_selection_is_resolved, feature_definition_is_incomplete,
    incomplete_feature_families, mesh_attribute_channels, mesh_texture_asset,
    mesh_texture_assignments, report_design_projection_gaps, MeshProjection,
};
use crate::loss::F3dLossCode;
use crate::native::F3dNative;
use crate::records::feature::scope::DesignParameterScope;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn design_projection_gaps(
    ir: &cadmpeg_ir::document::CadIr,
    native: &F3dNative,
) -> super::super::DesignProjectionGaps {
    super::super::design_projection_gaps(ir, native).unwrap()
}

#[test]
fn active_face_substitutions_have_a_distinct_loss_note() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native.design_face_operands.push(
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "346",
            "paired_byte_offset": 325,
            "paired_class_tag": "262",
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 352,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "resolved_active_face": "f3d:brep:entity#30",
            "next_record_index": 202,
            "next_byte_offset": 469
        }))
        .expect("active face operand"),
    );
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };

    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::FeatureFaceSelectionActiveSubstituted.kind())
        .expect("active-face substitution loss");
    assert_eq!(loss.message, "1 legacy face operand(s) use a current active-BREP face because no unique preceding-state face slot resolved.");
}

#[test]
fn history_past_its_binding_bound_reports_a_loss() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native
        .asm_histories
        .push(crate::test_support::history::history(
            "f3d:test:asm-history#0".into(),
            0,
            None,
            true,
            Vec::new(),
        ));
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };

    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::HistoryBindingBudgetExceeded.kind())
        .expect("history binding loss");
    assert_eq!(loss.message, "1 ASM history stream(s) retain only their state headers, with no bulletin boards, history records or historical topology, because their estimated retained size exceeded the decode session's remaining retained-byte allowance.");
}

#[test]
fn recipe_side_search_past_its_work_cap_reports_a_loss() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    let mut operand: crate::records::topology::face::DesignFaceOperand =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:face-operand#200",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 200,
            "byte_offset": 0,
            "class_tag": "346",
            "paired_byte_offset": 325,
            "paired_class_tag": "262",
            "recipe_record_index": 203,
            "recipe_record_byte_offset": 341,
            "recipe_id": "f3d:test:recipe#201",
            "recipe_prefix_offset": 352,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "bounded_face",
            "recipe_program_offset": 0,
            "recipe_program": [],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "next_record_index": 202,
            "next_byte_offset": 469
        }))
        .expect("face operand");
    // One counted side with 3,000 payload-prefix splits needs more search
    // work than the cap allows.
    let mut program = vec![-1, -1, 2, 1, -1, 0, -1, 0, -1, 2, 5, -1, 7, -1];
    program.extend_from_slice(&[0; 3000]);
    operand
        .recipe_nodes
        .push(crate::records::topology::face::DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 4 * u64::try_from(program.len()).unwrap(),
            program,
            recipe_structure: None,
        });
    native.design_face_operands.push(operand);
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };

    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::RecipeSideSearchBudgetExceeded.kind())
        .expect("recipe search loss");
    assert_eq!(loss.message, "1 Design recipe program(s) retain no recipe structure because their side-sequence search exceeded the local work cap RECIPE_SIDE_SEARCH_WORK (4194304).");
}

#[test]
fn capped_rectangular_pattern_instance_search_reports_a_loss() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::RPattern,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::RPattern(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::RectangularPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            crate::records::feature::patterns::DesignRectangularPatternConstruction::try_from(
                crate::records::feature::patterns::DesignRectangularPatternConstructionWire {
                    u_count: 3,
                    v_count: 1,
                    u_extent: 10.0,
                    v_extent: 0.0,
                    owner_record_indices: [11, 12, 13, 14],
                    value_offsets: [101, 102, 103, 104],
                    instances: None,
                    instance_search_cap: Some(
                        crate::records::feature::patterns::RectangularPatternSearchCap::RunWork,
                    ),
                    directions: Vec::new(),
                    secondary_direction: None,
                },
            )
            .unwrap(),
        );
    }
    native.design_parameter_scopes.push(scope);
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };

    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::FeaturePatternInstanceSearchCapped.kind())
        .expect("pattern instance search loss");
    assert_eq!(loss.message, "1 rectangular pattern(s) have no instance transforms or direction because their instance search exceeded the local cap RECTANGULAR_PATTERN_RUN_WORK (1048576).");
}

#[test]
fn mesh_feature_binds_tessellations_in_design_body_order() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let scope_id = "f3d:Design/BulkStream.dat:design-parameter-scope#10";
    let mut scope = DesignParameterScope::empty(
        scope_id,
        crate::records::feature::scope::DesignFeatureKind::BaseMeshFeature,
        10,
    );
    // The feature's owning entity reference is distinct from its scope index.
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![221]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut features = vec![Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#mesh-import").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Base Mesh Feature".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Base Mesh Feature".into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: Some(scope_id.into()),
    }];
    let projection = MeshProjection {
        count: 2,
        tessellations_by_scope: std::collections::HashMap::from([(
            ("f3d:Design/BulkStream.dat".into(), 10),
            vec!["tessellation:z-body".into(), "tessellation:a-body".into()],
        )]),
    };

    bind_mesh_feature_definitions(&mut features, &[scope], &projection).unwrap();

    assert_eq!(
        *features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MeshImport {
            tessellations: vec!["tessellation:z-body".into(), "tessellation:a-body".into(),]
                .try_into()
                .unwrap(),
        })
    );
    assert!(!feature_definition_is_incomplete(
        features[0].evaluation.definition()
    ));
}

#[test]
fn mesh_texture_ids_resolve_through_design_table_order() {
    use cadmpeg_ir::assets::AssetId;
    use cadmpeg_ir::tessellation::TessellationTextureAssignment;

    let first = AssetId::mint("synthetic:test:id#asset:first").expect("identity grammar");
    let second = AssetId::mint("synthetic:test:id#asset:second").expect("identity grammar");
    let textures = [
        ("resource:first".into(), first.clone()),
        ("resource:second".into(), second.clone()),
        ("resource:third".into(), first.clone()),
    ];
    assert_eq!(
        with_test_ctx(|ctx| mesh_texture_assignments(ctx, Some(&[0, 2, 1, 3, 2]), &textures, 5))
            .expect("texture assignments"),
        vec![
            TessellationTextureAssignment {
                source_id: Some("resource:first".into()),
                texture: first,
                triangles: vec![2],
            },
            TessellationTextureAssignment {
                source_id: Some("resource:second".into()),
                texture: second,
                triangles: vec![1, 4],
            },
            TessellationTextureAssignment {
                source_id: Some("resource:third".into()),
                texture: AssetId::mint("synthetic:test:id#asset:first").expect("identity grammar"),
                triangles: vec![3],
            },
        ]
    );
    assert!(matches!(
        with_test_ctx(|ctx| mesh_texture_assignments(
            ctx,
            Some(&[2]),
            &[(
                "resource:only".into(),
                AssetId::mint("synthetic:test:id#asset:only").expect("identity grammar")
            )],
            1,
        )),
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn indexed_mesh_channels_project_default_and_override_selectors() {
    let attribute = crate::paramesh::MeshAttribute {
        role: 4,
        resource_guid: None,
        authored_name: None,
        groups: crate::paramesh::UniqueFaceGroups::default(),
        elements: crate::paramesh::MeshElements::Float {
            width: crate::paramesh::FloatWidth::Quad,
            values: (0..80).collect(),
        },
        addressing: crate::paramesh::MeshAttributeAddressing::Corner(vec![0, 2]),
    };
    let mut unresolved = std::collections::BTreeMap::new();
    let channels = {
        mesh_attribute_channels(&[attribute], 3, &[[0, 1, 2]], &mut unresolved)
            .expect("mesh attribute budget")
    };

    assert!(unresolved.is_empty());
    assert_eq!(channels.len(), 1);
    assert!(matches!(
        wire::field::<cadmpeg_ir::tessellation::ChannelAddressing>(&channels[0], "addressing"),
        cadmpeg_ir::tessellation::ChannelAddressing::Corner { .. }
    ));
    assert_eq!(channels[0].count(), 5);
    assert_eq!(channels[0].indices(), [3, 1, 4]);
    assert_eq!(channels[0].data(), (0..80).collect::<Vec<_>>());
}

#[test]
fn presentation_timeline_objects_are_not_incomplete_modeling_features() {
    let native = |kind: &str| {
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: kind.into(),
                parameters: std::collections::BTreeMap::new(),
            },
        )
    };

    assert!(!feature_definition_is_incomplete(&native("Canvas")));
    assert!(!feature_definition_is_incomplete(&native("Decal")));
    assert!(feature_definition_is_incomplete(&native("Fillet")));
}

#[test]
fn full_round_fillet_with_automatic_sides_is_complete() {
    use cadmpeg_ir::features::{
        edge_treatments::FullRoundSideSelection, FaceSelection, Feature, FeatureDefinition,
        FeatureId, FeatureOperation,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#full-round").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Fillet".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::FullRoundFillet {
                groups: vec![
                    cadmpeg_ir::features::edge_treatments::FullRoundFilletGroup::new(
                        FaceSelection::Resolved {
                            faces: vec!["test:model:face#center"
                                .try_into()
                                .expect("valid identity")],
                            native: "native:center-group".into(),
                        },
                        FullRoundSideSelection::Automatic,
                        FullRoundSideSelection::Automatic,
                    )
                    .unwrap(),
                ]
                .try_into()
                .unwrap(),
            }),
        ),
        native_ref: None,
    });

    assert!(!feature_definition_is_incomplete(
        ir.model.features[0].evaluation.definition()
    ));
    assert_eq!(
        design_projection_gaps(&ir, &F3dNative::default()).incomplete_features,
        0
    );
}

#[test]
fn extrude_completeness_requires_resolved_profile_start_and_termination() {
    let extrude =
        |profile: serde_json::Value, start: serde_json::Value, termination: serde_json::Value| {
            serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(serde_json::json!({
                "definition": "extrude",
                "profile": profile,
                "start": start,
                "extent": {
                    "kind": "one_sided",
                    "side": {"termination": termination}
                },
                "solid": true,
                "op": "new_body"
            }))
            .expect("Extrude definition")
        };
    let sketch_profile = serde_json::json!({
        "kind": "sketch_profiles",
        "value": {"sketch": "test:model:sketch#1", "profiles": [0]}
    });
    let profile_start = serde_json::json!({"kind": "profile_plane"});
    let blind = serde_json::json!({"kind": "blind", "length": 10.0});

    assert!(!feature_definition_is_incomplete(&extrude(
        sketch_profile.clone(),
        profile_start.clone(),
        blind.clone(),
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        serde_json::json!({"kind": "native", "value": "native:profile"}),
        profile_start.clone(),
        blind.clone(),
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        sketch_profile.clone(),
        serde_json::json!({"kind": "unresolved"}),
        blind,
    )));
    assert!(feature_definition_is_incomplete(&extrude(
        sketch_profile,
        profile_start,
        serde_json::json!({"kind": "to_face", "face": {"kind": "unresolved"}}),
    )));
}

#[test]
fn hole_completeness_requires_support_placement_size_and_extent() {
    let complete: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "hole",
            "face": {"kind": "faces", "value": ["test:model:face#support"]},
            "placements": [{
                "kind": "directed",
                "position": {"x": 1.0, "y": 2.0, "z": 3.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -1.0}
            }],
            "shape": {
                "construction": {
                    "construction": "form",
                    "kind": {"kind": "simple_drilled", "drill_point_angle": 2.0}
                },
                "diameter": 5.0
            },
            "extent": {"kind": "blind", "length": 10.0}
        }))
        .expect("complete Hole definition");
    assert!(!feature_definition_is_incomplete(&complete));

    let mut missing_placement = complete.clone();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Hole { placements, .. },
    ) = &mut missing_placement
    else {
        panic!("Hole definition");
    };
    *placements = None;
    assert!(feature_definition_is_incomplete(&missing_placement));

    let mut native_support = complete.clone();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Hole { face, .. },
    ) = &mut native_support
    else {
        panic!("Hole definition");
    };
    *face = Some(cadmpeg_ir::features::FaceSelection::Native(
        "native:support".into(),
    ));
    assert!(feature_definition_is_incomplete(&native_support));

    let mut missing_extent = complete;
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Hole { extent, .. },
    ) = &mut missing_extent
    else {
        panic!("Hole definition");
    };
    *extent = None;
    assert!(feature_definition_is_incomplete(&missing_extent));
}

#[test]
fn face_selection_resolution_accepts_complete_generated_and_partial_members() {
    use cadmpeg_ir::features::{FaceSelection, FeatureId, GeneratedFaceRef};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalFaceId};

    assert!(face_selection_is_resolved(
        &FaceSelection::generated(
            vec![GeneratedFaceRef::new(
                FeatureId::mint("test:model:feature#source").expect("identity grammar"),
                "test:model:face#1".into(),
            )
            .expect("selection reference admission")],
            "native:generated-face".into(),
        )
        .expect("selection reference admission")
    ));
    assert!(!face_selection_is_resolved(
        &FaceSelection::historical_partial(
            FeatureInputTopologyId::mint("test:model:feature-input#state:1")
                .expect("identity grammar"),
            vec![HistoricalFaceId::mint("test:model:face#1").expect("identity grammar")],
            vec!["native:missing-face".into()],
            "native:historical-face".into(),
        )
        .unwrap()
    ));
}

#[test]
fn filled_surface_completeness_requires_boundary_conditions_support_and_merge() {
    use cadmpeg_ir::features::{
        FaceSelection, FeatureDefinition, FeatureOperation, PathRef, SurfaceBoundary,
        SurfaceContinuity,
    };
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let surface = |support_faces, continuity, merge_result| {
        FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            boundary: SurfaceBoundary::Path(PathRef::Edges(vec![EdgeId::mint(
                "test:model:edge#1",
            )
            .expect("identity grammar")])),
            support_faces,
            continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(continuity),
            merge_result,
        })
    };

    assert!(!feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Contact,
        Some(false),
    )));
    assert!(feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Contact,
        None,
    )));
    assert!(feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(Vec::new()),
        SurfaceContinuity::Tangent,
        Some(true),
    )));
    assert!(!feature_definition_is_incomplete(&surface(
        FaceSelection::Faces(vec![
            FaceId::mint("test:model:face#support").expect("identity grammar")
        ]),
        SurfaceContinuity::Curvature,
        Some(true),
    )));
}

#[test]
fn sheet_metal_completeness_requires_neutral_profiles_and_edges() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("sheet-metal definition")
    };
    let base_flange = |profile| {
        definition(serde_json::json!({
            "definition": "sheet_metal_base_flange",
            "profile": profile,
            "thickness": 2.5,
            "side": "forward"
        }))
    };
    let edge_flange = |edges| {
        definition(serde_json::json!({
            "definition": "sheet_metal_edge_flange",
            "edges": edges,
            "height": {"kind": "distance", "value": 25.0},
            "angle": std::f64::consts::FRAC_PI_2,
            "height_datum": "outer_faces",
            "bend_position": "inside",
            "width": {"kind": "full_edge"},
            "bend_radius": 2.5
        }))
    };
    let hem = |edges| {
        definition(serde_json::json!({
            "definition": "sheet_metal_hem",
            "edges": edges,
            "bend_position": "adjacent",
            "form": {"kind": "flat", "value": {"length": 10.0}},
            "direction": "forward",
            "bend_radius": 2.5
        }))
    };

    assert!(!feature_definition_is_incomplete(&base_flange(
        serde_json::json!({"kind": "sketch", "value": "test:model:sketch#1"}),
    )));
    assert!(feature_definition_is_incomplete(&base_flange(
        serde_json::json!({"kind": "native", "value": "native:profile"}),
    )));
    assert!(!feature_definition_is_incomplete(&edge_flange(
        serde_json::json!({"kind": "edges", "value": ["test:model:edge#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&edge_flange(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
    assert!(!feature_definition_is_incomplete(&hem(
        serde_json::json!({"kind": "edges", "value": ["test:model:edge#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&hem(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
}

#[test]
fn selected_face_and_edge_features_require_neutral_operands() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("selected feature definition")
    };
    let fillet = |edges| {
        definition(serde_json::json!({
            "definition": "fillet",
            "groups": [{
                "edges": edges,
                "radius": {"kind": "constant", "radius": 5.0},
                "tangency_weight": 1.0
            }]
        }))
    };

    assert!(!feature_definition_is_incomplete(&fillet(
        serde_json::json!({"kind": "edges", "value": ["test:model:edge#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&fillet(
        serde_json::json!({"kind": "native", "value": "native:edges"}),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "delete_face",
            "faces": {"kind": "faces", "value": ["test:model:face#1"]},
            "heal": true
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "delete_face",
            "faces": {"kind": "native", "value": "native:faces"},
            "heal": true
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "offset_surface",
            "faces": {"kind": "faces", "value": ["test:model:face#1"]},
            "distance": 2.0
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "offset_surface",
            "faces": {"kind": "faces", "value": ["test:model:face#1"]},
            "distance": null
        }),
    )));
}

#[test]
fn form_and_primitive_completeness_requires_construction_payloads() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("construction definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({"definition": "form", "cages": ["test:model:subd#1"]}),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({"definition": "form", "cages": []}),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "block",
            "dimensions": [30.0, 40.0, 20.0],
            "placement": [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0]
            ],
            "op": "join"
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "block",
            "dimensions": [30.0, 40.0, 20.0],
            "placement": null,
            "op": "join"
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "primitive",
            "solid": {
                "kind": "cylinder",
                "radius": 15.0,
                "height": 7.0,
                "angle": std::f64::consts::TAU
            },
            "op": "join"
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "primitive",
            "solid": {
                "kind": "cylinder",
                "radius": 15.0,
                "height": 7.0,
                "angle": std::f64::consts::TAU
            },
            "op": "unresolved"
        }),
    )));
}

#[test]
fn profile_and_boolean_features_require_resolved_operation_inputs() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("profile or Boolean definition")
    };

    let sweep = definition(serde_json::json!({
        "definition": "sweep",
        "shape": {
            "mode": "solid",
            "op": "join",
            "section": {
                "kind": "profile",
                "value": {"kind": "sketch", "value": "test:model:sketch#section"}
            }
        },
        "path": {"kind": "edges", "value": ["test:model:edge#path"]}
    }));
    assert!(!feature_definition_is_incomplete(&sweep));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "sweep",
            "shape": {
                "mode": "solid",
                "op": "join",
                "section": {
                    "kind": "profile",
                    "value": {"kind": "native", "value": "native:section"}
                }
            },
            "path": {"kind": "edges", "value": ["test:model:edge#path"]}
        }),
    )));

    let chamfer = definition(serde_json::json!({
        "definition": "chamfer",
        "groups": [{
            "edges": {"kind": "edges", "value": ["test:model:edge#1"]},
            "spec": {"kind": "distance", "distance": 2.0}
        }]
    }));
    assert!(!feature_definition_is_incomplete(&chamfer));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "chamfer",
            "groups": [{
                "edges": {"kind": "native", "value": "native:edges"},
                "spec": {"kind": "distance", "distance": 2.0}
            }]
        }),
    )));

    let combine = definition(serde_json::json!({
        "definition": "combine",
        "operands": {
            "target": {"kind": "bodies", "value": ["test:model:body#target"]},
            "tools": {"kind": "bodies", "value": ["test:model:body#tool"]}
        },
        "op": "cut"
    }));
    assert!(!feature_definition_is_incomplete(&combine));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "combine",
            "operands": {
                "target": {"kind": "native", "value": "native:target"},
                "tools": {"kind": "bodies", "value": ["test:model:body#tool"]}
            },
            "op": "cut"
        }),
    )));

    let revolve = definition(serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "resolved",
            "profile": {"kind": "sketch", "value": "test:model:sketch#profile"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": std::f64::consts::PI}
            }
        },
        "op": "new_body"
    }));
    assert!(!feature_definition_is_incomplete(&revolve));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "revolve",
            "construction": {
                "state": "resolved",
                "profile": {"kind": "native", "value": "native:profile"},
                "axis": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
                },
                "extent": {
                    "kind": "one_sided",
                    "termination": {"kind": "angle", "angle": std::f64::consts::PI}
                }
            },
            "op": "new_body"
        }),
    )));
}

#[test]
fn datum_point_completeness_requires_a_resolved_construction_rule() {
    let definition = |construction: Option<serde_json::Value>| {
        let mut value = serde_json::json!({
            "definition": "datum_point",
            "position": {"x": 1.0, "y": 2.0, "z": 3.0}
        });
        if let Some(construction) = construction {
            value
                .as_object_mut()
                .expect("DatumPoint object")
                .insert("construction".into(), construction);
        }
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("DatumPoint definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(Some(
        serde_json::json!({
            "kind": "circle_center",
            "edge": {"kind": "edges", "value": ["test:model:edge#1"]}
        }),
    ))));
    assert!(feature_definition_is_incomplete(&definition(None)));
    assert!(feature_definition_is_incomplete(&definition(Some(
        serde_json::json!({
            "kind": "distance_on_edge",
            "edge": {"kind": "native", "value": "native:edge"},
            "fraction": 0.5
        }),
    ))));
}

#[test]
fn datum_plane_completeness_accepts_direct_frames_and_resolved_construction() {
    let definition = |value| {
        serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(value)
            .expect("datum-plane definition")
    };

    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_plane",
            "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 5.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            }
        }),
    )));
    let three_point = |points: [serde_json::Value; 3]| {
        serde_json::json!({
            "definition": "datum_three_point_plane",
            "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            },
            "points": points
        })
    };
    assert!(!feature_definition_is_incomplete(&definition(three_point(
        std::array::from_fn(|index| serde_json::json!({
            "kind": "historical",
            "value": {
                "state": "test:model:feature-input#state:1",
                "vertex": format!("test:model:vertex#{index}"),
                "native": format!("native:{index}")
            }
        })),
    ))));
    assert!(feature_definition_is_incomplete(&definition(three_point(
        std::array::from_fn(
            |index| serde_json::json!({"kind": "native", "value": format!("native:{index}")})
        ),
    ))));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_principal_plane",
            "plane": "top"
        }),
    )));
    assert!(!feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_offset_plane",
            "reference": {"reference": "feature", "feature": "test:model:feature#plane"},
            "distance": 5.0
        }),
    )));
    assert!(feature_definition_is_incomplete(&definition(
        serde_json::json!({
            "definition": "datum_offset_plane",
            "distance": 5.0
        }),
    )));
}

#[test]
fn coil_completeness_requires_neutral_placement_and_boolean_targets() {
    use cadmpeg_ir::features::{
        BodySelection, CoilConstruction, CoilExtent, CoilPlacement, CoilResult, CoilSection,
        CoilSectionPlacement, FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::{Angle, Length};

    let construction = CoilConstruction {
        placement: CoilPlacement::Explicit {
            frame: cadmpeg_ir::features::FeatureUnitPlaneFrame::new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        diameter: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
        extent: CoilExtent::RevolutionsHeight {
            revolutions: cadmpeg_ir::scalar::PositiveReal::new(2.0).unwrap(),
            height: Length::new(5.0).unwrap(),
        },
        section: CoilSection::Circular {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
        },
        section_placement: CoilSectionPlacement::Center,
        clockwise: false,
        taper: Angle::new(0.0).unwrap(),
    };
    let definition = |construction, result| {
        FeatureDefinition::Operation(FeatureOperation::Coil {
            construction,
            result,
        })
    };

    assert!(!feature_definition_is_incomplete(&definition(
        construction.clone(),
        CoilResult::NewBody {},
    )));

    let mut native_placement = construction.clone();
    native_placement.placement = CoilPlacement::Native {
        native_ref: cadmpeg_ir::features::SelectionReference::try_from(String::from(
            "native:placement",
        ))
        .unwrap(),
    };
    assert!(feature_definition_is_incomplete(&definition(
        native_placement,
        CoilResult::NewBody {},
    )));

    let native_target = definition(
        construction.clone(),
        CoilResult::Boolean {
            operation: cadmpeg_ir::features::BooleanKind::Join,
            targets: BodySelection::Native("native:target".into()),
        },
    );
    assert!(feature_definition_is_incomplete(&native_target));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("test:model:feature#coil")
            .expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("CoilPrimitive".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(native_target),
        native_ref: None,
    });
    let gaps = design_projection_gaps(&ir, &F3dNative::default());
    assert_eq!(gaps.incomplete_features, 1);
    assert_eq!(gaps.body_selections, 1);

    assert!(!feature_definition_is_incomplete(&definition(
        construction,
        CoilResult::Boolean {
            operation: cadmpeg_ir::features::BooleanKind::Cut,
            targets: BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                    "test:model:body#1"
                )
                .expect("identity grammar")],)
                .expect("distinct bodies")
            ),
        },
    )));
}

#[test]
fn draft_completeness_requires_material_side() {
    let complete: cadmpeg_ir::features::FeatureDefinition =
        serde_json::from_value(serde_json::json!({
            "definition": "draft",
            "faces": {"kind": "faces", "value": ["test:model:face#drafted"]},
            "anchor": {
                "kind": "neutral_plane",
                "plane": {"kind": "faces", "value": ["test:model:face#neutral"]}
            },
            "angle": 0.1,
            "outward": true
        }))
        .expect("complete neutral-plane Draft");
    assert!(!feature_definition_is_incomplete(&complete));

    let mut incomplete = complete;
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Draft { outward, .. },
    ) = &mut incomplete
    else {
        panic!("Draft definition");
    };
    *outward = None;
    assert!(feature_definition_is_incomplete(&incomplete));
}

#[test]
fn loft_completeness_and_gap_counts_require_resolved_sections_and_paths() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId};

    let resolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
        "definition": "loft",
        "sections": [
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "test:model:spatial-sketch#1", "profiles": [2, 3]}
            },
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "test:model:spatial-sketch#1", "profiles": [1, 4]}
            }
        ],
        "guidance": {"kind": "guides", "path": [{
            "kind": "spatial_sketch_curves",
            "value": {"sketch": "test:model:spatial-sketch#1", "curves": ["test:model:spatial-entity#curve"]}
        }]},
        "op": "join"
    }))
    .expect("resolved Loft definition");
    assert!(!feature_definition_is_incomplete(&resolved));

    let unresolved: FeatureDefinition = serde_json::from_value(serde_json::json!({
        "definition": "loft",
        "sections": [
            {"kind": "native", "value": "native:profile"},
            {
                "kind": "spatial_sketch_profiles",
                "value": {"sketch": "test:model:spatial-sketch#1", "profiles": [1, 4]}
            }
        ],
        "guidance": {"kind": "guides", "path": [{"kind": "native", "value": "native:guide"}]},
        "op": "join"
    }))
    .expect("unresolved Loft definition");
    assert!(feature_definition_is_incomplete(&unresolved));

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#loft").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Loft".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(unresolved),
        native_ref: None,
    });

    let gaps = design_projection_gaps(&ir, &F3dNative::default());
    assert_eq!(gaps.incomplete_features, 1);
    assert_eq!(gaps.profile_selections, 1);
    assert_eq!(gaps.path_selections, 1);
}

#[test]
fn incomplete_feature_families_are_counted_by_source_operation() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let feature = |id: &str, source_tag: Option<&str>, kind: &str| Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: source_tag.map(str::to_owned),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: kind.into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: None,
    };
    ir.model.features.push(feature(
        "synthetic:test:id#feature:1",
        Some("EdgeFlange"),
        "native-a",
    ));
    ir.model.features.push(feature(
        "synthetic:test:id#feature:2",
        Some("EdgeFlange"),
        "native-b",
    ));
    ir.model
        .features
        .push(feature("synthetic:test:id#feature:3", None, "Hem"));
    ir.model.features.push(feature(
        "synthetic:test:id#feature:4",
        Some("Canvas"),
        "Canvas",
    ));

    assert_eq!(
        incomplete_feature_families(&ir),
        std::collections::BTreeMap::from([("EdgeFlange", 2), ("Hem", 1)])
    );
}

#[test]
fn body_copy_features_require_resolved_body_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;

    let resolved = BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
            "test:model:body#result",
        )
        .expect("identity grammar")])
        .expect("distinct bodies"),
        native: "native:body-selection".into(),
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: resolved.clone(),
        })
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::InsertBodies {
            bodies: cadmpeg_ir::features::InsertedBodies::Resolved {
                native: "native:body-selection".into(),
            },
        })
    ));

    let unresolved = BodySelection::Native("native:body-selection".into());
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies: unresolved })
    ));
}

#[test]
fn split_body_requires_resolved_target_and_tool_selections() {
    use cadmpeg_ir::features::{BodySelection, FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::{BodyId, FaceId};

    let resolved_target = BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
            "test:model:body#target",
        )
        .expect("identity grammar")])
        .expect("distinct bodies"),
        native: "native:target".into(),
    };
    let resolved_tool = FaceSelection::Resolved {
        faces: vec![FaceId::mint("test:model:face#tool").expect("identity grammar")],
        native: "native:tool".into(),
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: resolved_target.clone(),
            tool: cadmpeg_ir::features::SplitBodyTool::Faces(resolved_tool),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: resolved_target.clone(),
            tool: cadmpeg_ir::features::SplitBodyTool::Faces(FaceSelection::Native(
                "native:tool".into()
            )),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: BodySelection::Native("native:target".into()),
            tool: cadmpeg_ir::features::SplitBodyTool::Faces(FaceSelection::Resolved {
                faces: vec![FaceId::mint("test:model:face#tool").expect("identity grammar")],
                native: "native:tool".into(),
            }),
        })
    ));
}

#[test]
fn mesh_texture_bytes_refuse_one_byte_below_their_retained_ceiling() {
    use std::io::{Cursor, Write};

    const ENTRY: &str = "FusionAssetName[Active]/Textures/a.png";
    const DATA: &[u8] = b"PNG texture";
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(DATA).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    // The filename record holds the fixed prefix and the UTF-16 filename.
    let record = crate::records::mesh::DesignMeshRecordIdentity::new(
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        8,
        300,
        u64::try_from(crate::layout::paramesh_texture_filename_prefix::LEN + 2 * "a.png".len())
            .unwrap(),
    )
    .unwrap();
    let file = crate::records::mesh::DesignMeshTextureFile::new(record, "a.png", ENTRY.to_owned())
        .unwrap();
    let asset = cadmpeg_ir::assets::AssetId::mint("synthetic:test:id#asset:texture").unwrap();
    let ceiling = u64::try_from(DATA.len()).unwrap();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        for (limit, admitted) in [(ceiling, true), (ceiling - 1, false)] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = mesh_texture_asset(&ctx, scan, &file, &asset);
            if admitted {
                let projected = result.unwrap();
                assert_eq!(projected.id, asset);
                assert_eq!(
                    projected.native_ref.as_deref(),
                    Some("f3d:FusionAssetName[Active]/Textures/a.png:mesh-texture#0")
                );
            } else {
                assert!(matches!(
                    result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                        if limit.dimension
                            == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                            && limit.operation == "retain F3D mesh texture bytes"
                ));
            }
        }
    });
}

#[test]
fn refused_edge_flange_payload_reports_its_retained_scope_loss() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:feature#flange").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("EdgeFlange".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native {
                    kind: "EdgeFlange".into(),
                    parameters: Default::default(),
                },
            ),
        ),
        native_ref: None,
    });
    let mut native = F3dNative::default();
    native
        .design_parameter_scopes
        .push(DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:parameter-scope#70",
            crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
            70,
        ));
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();
    assert!(report.losses.iter().any(|loss| loss.code
        == F3dLossCode::FeatureDefinitionIncomplete.kind()
        && loss.message.contains("EdgeFlange=1")));
    assert!(report.losses.iter().any(|loss| loss.code
        == F3dLossCode::FeatureScopePayloadUninterpreted.kind()
        && loss.message.starts_with("1 native feature scope(s)")));
}

#[test]
fn emboss_native_and_refused_prefixes_have_exact_scope_losses() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut native = crate::native::F3dNative::default();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Emboss,
        40,
    );
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    native.design_parameter_scopes.push(scope.clone());
    let mut report =
        cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(true));
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::EmbossPrefixUnparsed.kind())
            .count(),
        1
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 430;
            draft.paired_byte_offset = 430;
            draft.reference_count_offset = 235;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                41, 44, 48, 51, 60, 63, 66, 69, 72,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.payload = crate::records::feature::scope::DesignScopePayload::Emboss(Some(
                serde_json::from_value(serde_json::json!({
                    "control_words": [7, 9], "native_pair": [17, 23],
                    "depth_owner": {"value": 60, "offset": 29},
                    "auxiliary_group": {"value": 72, "offset": 40},
                    "horizontal_owner": {"value": 63, "offset": 51},
                    "rotation_owner": {"value": 69, "offset": 82},
                    "face_group": {"value": 48, "offset": 93},
                    "vertical_owner": {"value": 66, "offset": 104},
                    "profile_group": {"value": 41, "offset": 119}
                }))
                .unwrap(),
            ));
        })
        .unwrap();
    native.design_parameter_scopes = vec![scope];
    let mut report =
        cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(true));
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::EmbossControlsNative.kind())
            .count(),
        1
    );
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::EmbossPrefixUnparsed.kind()));
    let retained = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::EmbossControlsNative.kind())
        .unwrap();
    assert!(retained
        .message
        .contains(&native.design_parameter_scopes[0].id));
    for carrier in [
        "control words",
        "native pairs",
        "scalar-owner links",
        "selection links",
        "contour membership",
        "placement",
    ] {
        assert!(retained.message.contains(carrier));
    }
}

#[test]
fn move_transform_refusal_reports_exact_loss_for_transform_or_selection() {
    use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
    use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native
        .design_parameter_scopes
        .push(DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#17",
            DesignFeatureKind::Move,
            17,
        ));
    for (typed, reflected) in [(false, false), (true, false), (true, true)] {
        if typed {
            let DesignScopePayloadMut::Move(slot) = native.design_parameter_scopes[0].payload_mut()
            else {
                panic!("Move payload");
            };
            *slot = Some(DesignMoveOperation {
                transform: if reflected {
                    [
                        [-1., 0., 0., 0.],
                        [0., 1., 0., 0.],
                        [0., 0., 1., 0.],
                        [0., 0., 0., 1.],
                    ]
                    .try_into()
                    .unwrap()
                } else {
                    crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
                },
                transform_offset: 448,
                transform_record_index: 71,
                form: DesignMoveForm::Form1,
                form_offset: 443,
            });
        }
        let mut report = cadmpeg_ir::codec::DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
            coverage: Default::default(),
            losses: Vec::new(),
            notes: Vec::new(),
            transfer_ledger: Default::default(),
        };
        report_design_projection_gaps(
            &mut report,
            &cadmpeg_test_support::service_decode_context(),
            &ir,
            &native,
        )
        .unwrap();
        let losses: Vec<_> = report
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::MoveUnprojected.kind())
            .collect();
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains(if reflected {
            "stored basis reverses orientation"
        } else if typed {
            "no neutral MoveBody feature for its body selection"
        } else {
            "no unique admitted rigid-transform record"
        }));
    }
}

#[test]
fn typed_profile_ambiguous_selection_retains_profile_loss() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let definition = serde_json::from_value(serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": "synthetic:profile"},
        "direction": {"kind": "profile_normal"},
        "start": {"kind": "profile_plane"},
        "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 7.0}}},
        "op": "new_body", "solid": true
    }))
    .unwrap();
    ir.model.features.push(cadmpeg_ir::features::Feature {
        id: cadmpeg_ir::features::FeatureId::mint("synthetic:test:id#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
        product_definition: None,
    });
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: Default::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &F3dNative::default(),
    )
    .unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureProfileSelectionNative.kind()));
    assert!(matches!(
        ir.model.features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude {
                profile: cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Native(_)
                ),
                ..
            }
        )
    ));
}

#[test]
fn stored_body_scope_keeps_opaque_payload_loss_after_history_projection() {
    use crate::records::feature::scope::DesignFeatureKind;
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#90",
        DesignFeatureKind::BaseFeature,
        90,
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features.push(Feature {
        id: FeatureId::mint("test:model:feature#90").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: Default::default(),
                    native: scope.id.clone(),
                },
            },
        )),
        native_ref: Some(scope.id.clone()),
        product_definition: None,
    });
    let box_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#91",
        DesignFeatureKind::BoxPrimitive,
        91,
    );
    let mut box_feature = ir.model.features[0].clone();
    box_feature.id = FeatureId::mint("test:model:feature#91").unwrap();
    box_feature.ordinal = 1;
    box_feature.source_tag = Some("BoxPrimitive".into());
    box_feature.native_ref = Some(box_scope.id.clone());
    box_feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "BoxPrimitive".into(),
            parameters: Default::default(),
        }));
    ir.model.features.push(box_feature);
    let mut native = F3dNative::default();
    native.design_parameter_scopes.push(scope);
    native.design_parameter_scopes.push(box_scope);
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: Default::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureScopePayloadUninterpreted.kind()));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureStoredBodiesEmpty.kind()));
    assert!(report.losses.iter().any(|loss| loss.code
        == F3dLossCode::FeatureDefinitionIncomplete.kind()
        && loss.message.contains("BoxPrimitive=1")));
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureStoredBodiesRecipeAbsent.kind()));
}

#[test]
fn counted_partial_profile_selection_reports_one_loss() {
    use cadmpeg_ir::features::{Feature, FeatureEvaluation, FeatureId};
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let definition = serde_json::from_value(serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "sketch_selection", "value": {
            "sketch": "synthetic:test:sketch#42", "selections": ["f3d:Design/BulkStream.dat:scope#10"]
        }},
        "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
        "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
        "op": "cut", "solid": true
    })).unwrap();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:feature#cut").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: None,
        product_definition: None,
    });
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: Default::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &F3dNative::default(),
    )
    .unwrap();
    assert_eq!(
        report
            .losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::FeatureProfileSelectionNative.kind())
            .count(),
        1
    );
}

#[test]
fn refused_assemble_payloads_report_the_retained_scope_loss() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    for index in [70, 71] {
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:parameter-scope#{index}"),
                crate::records::feature::scope::DesignFeatureKind::Assemble,
                index,
            ));
    }
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: Default::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    report_design_projection_gaps(
        &mut report,
        &cadmpeg_test_support::service_decode_context(),
        &ir,
        &native,
    )
    .unwrap();
    let losses = report
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::FeatureScopePayloadUninterpreted.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].message, "2 native feature scope(s) retain their source envelope without interpreted specialized operation data.");
}
