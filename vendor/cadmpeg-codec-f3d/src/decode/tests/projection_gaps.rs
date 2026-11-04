// SPDX-License-Identifier: Apache-2.0
//! Decode-module projection and completeness unit tests.

use super::super::{
    apply_appearance_base_colors, container_only_dimension_parameters,
    unresolved_dimension_companion_count, DesignProjectionGaps, DimensionCompanionSources,
};
use crate::native::F3dNative;
use crate::records::{
    bodies::DesignBodyBinding,
    dimensions::{DesignDimensionLocusPair, DesignDimensionRecipeRecord},
    entity_header::DesignFeatureTimeline,
    feature::scope::DesignParameterScope,
    parameters::{DesignParameterCompanion, DesignParameterOwner},
    references::LostEdgeReference,
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_placement::DesignSketchPlacement,
    sketch_relations::SketchRelation,
};

fn design_projection_gaps(
    ir: &cadmpeg_ir::document::CadIr,
    native: &F3dNative,
) -> DesignProjectionGaps {
    super::super::design_projection_gaps(ir, native).unwrap()
}

#[test]
fn fixture_text_style_gaps_have_a_distinct_loss_count() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    for case in crate::test_support::sketch_text::text_fixture_records() {
        let mut native = F3dNative::default();
        native.sketch_texts.push(case.decode());
        let gaps = design_projection_gaps(&ir, &native);
        assert_eq!(
            gaps.unprojected_sketch_text_styles,
            usize::from(!case.neutral_state),
            "{}",
            case.label
        );
        assert_eq!(
            gaps.unprojected_sketch_texts,
            usize::from(case.neutral_state),
            "{}",
            case.label
        );
    }
}

#[test]
fn design_projection_gaps_count_unresolved_body_map_pairs() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    let mut native = F3dNative::default();
    native.design_body_bindings.push(
        DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: false,
            id: "f3d:Design/BulkStream.dat:design-body-binding#0".into(),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 0,
            asm_body_key_offset: 0,
            entity_suffix: 1,
            entity_suffix_offset: 8,
            blob_name: "BREP.snapshot.smb".into(),
            blob_name_offset: 16,
            body: None,
        })
        .unwrap(),
    );

    assert_eq!(
        design_projection_gaps(&ir, &native).unresolved_body_bindings,
        1
    );
}

#[test]
fn design_projection_gaps_count_cosmetic_thread_faces() {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    for (ordinal, face) in [
        serde_json::json!({"kind": "native", "value": "native:thread-face"}),
        serde_json::json!({"kind": "unresolved"}),
        serde_json::json!({
            "kind": "historical",
            "value": {
                "state": "test:model:feature-input#thread",
                "faces": ["historical:face"],
                "native": "native:thread-group"
            }
        }),
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(
            serde_json::from_value(serde_json::json!({
                "id": format!("test:model:feature#thread-{ordinal}"),
                "ordinal": ordinal,
                "suppressed": null,
                "definition": {
                    "definition": "cosmetic_thread",
                    "face": face,
                    "diameter": 2.5
                }
            }))
            .expect("CosmeticThread feature"),
        );
    }

    assert_eq!(
        design_projection_gaps(&ir, &F3dNative::default()).face_selections,
        2
    );
}

#[test]
fn design_projection_gaps_count_each_retained_selection_family() {
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId,
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    };

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId::mint("synthetic:test:id#constraint").unwrap(),
        sketch: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Native {
                native_kind: cadmpeg_core::text::NonBlankString::new("dimension").unwrap(),
                native_state: None,
                native_flags: None,
                native_properties: std::collections::BTreeMap::new(),
                entities: vec![SketchEntityId::mint("synthetic:test:id#entity").unwrap()],
                parameter: None,
                operands: Vec::new(),
            },
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some("native:sketch-relation".into()),
    });
    let mut native_dimension = ir.model.sketch_constraints[0].clone();
    native_dimension.id = SketchConstraintId::mint("synthetic:test:id#dimension").unwrap();
    native_dimension.native_ref = Some("native:dimension-companion".into());
    ir.model.sketch_constraints.push(native_dimension);
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#extrude",
            "ordinal": 0,
            "suppressed": null,
            "definition": {
                "definition": "extrude",
                "profile": {
                    "kind": "sketch_selection",
                    "value": {"sketch": "synthetic:test:id#sketch", "selections": ["native:profile"]}
                },
                "start": {"kind": "profile_plane"},
                "extent": {
                    "kind": "one_sided",
                    "side": {
                        "termination": {
                            "kind": "to_face",
                            "face": {"kind": "native", "value": "native:face"}
                        }
                    }
                },
                "op": "cut"
            }
        }))
        .expect("Extrude feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#sweep",
            "ordinal": 1,
            "suppressed": null,
            "definition": {
                "definition": "sweep",
                "shape": {
                    "mode": "solid",
                    "op": "cut",
                    "section": {
                        "kind": "profile",
                        "value": {"kind": "native", "value": "native:sweep-profile"}
                    }
                },
                "path": {"kind": "native", "value": "native:sweep-path"}
            }
        }))
        .expect("Sweep feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#fillet",
            "ordinal": 2,
            "suppressed": null,
            "definition": {
                "definition": "fillet",
                "groups": [
                    {
                        "edges": {"kind": "native", "value": "native:edges"},
                        "radius": {"kind": "constant", "radius": 1.0}
                    },
                    {
                        "edges": {"kind": "unresolved"},
                        "radius": {"kind": "constant", "radius": 2.0}
                    },
                    {
                        "edges": {
                            "kind": "historical_partial",
                            "value": {
                                "state": "test:model:feature-input#history-input",
                                "edges": [],
                                "unresolved": [
                                    "native:edge-operand#1",
                                    "f3d:test:lost-edge-reference#2"
                                ],
                                "native": "native:partial-edges"
                            }
                        },
                        "radius": {"kind": "constant", "radius": 3.0}
                    }
                ]
            }
        }))
        .expect("Fillet feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#suppressed-fillet",
            "ordinal": 2,
            "suppressed": true,
            "definition": {
                "definition": "fillet",
                "groups": [{
                    "edges": {"kind": "native", "value": "native:suppressed-edges"},
                    "radius": {"kind": "constant", "radius": 3.0}
                }]
            }
        }))
        .expect("suppressed Fillet feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#native-feature",
            "ordinal": 3,
            "suppressed": null,
            "definition": {
                "definition": "native",
                "kind": "unsupported",
                "parameters": {}
            }
        }))
        .expect("native feature"),
    );
    ir.model.features.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#unresolved-pattern",
            "ordinal": 4,
            "suppressed": null,
            "definition": {
                "definition": "pattern",
                "seeds": [],
                "pattern": {
                    "kind": "unresolved",
                    "form": "circular"
                }
            }
        }))
        .expect("unresolved pattern feature"),
    );

    let mut native = F3dNative::default();
    native.design_sketch_placements.push(DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: "native:sketch-placement".into(),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_1".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned()).unwrap(),
        record_index: 10,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("001".to_owned())
            .unwrap(),
    });
    native.sketch_points.push(
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: "native:sketch-point".into(),
            record_index: 11,
            owner_reference: Some(1),
            class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                .unwrap(),
            byte_offset: 0,
            coordinate_offset: 0,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                1,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    native.sketch_curve_identities.push(SketchCurveIdentity {
        id: "native:sketch-curve".into(),
        record_index: 12,
        owner_reference: Some(1),
        class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(1).unwrap(),
        secondary_id: 2,
        geometry: None,
        planar_z_noise: None,
    });
    native.lost_edge_references.push(
        LostEdgeReference::new(
            "f3d:test:lost-edge-reference#2".into(),
            0,
            "000".into(),
            0,
            "001".into(),
            1,
        )
        .expect("valid lost-edge record layout"),
    );
    native.sketch_relations.push(
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: "native:sketch-relation".into(),
            record_index: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                .unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
            auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: (Vec::new()).try_into().expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(0, None)
                .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (Vec::new()).try_into().expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap(),
    );
    native.design_parameters.push(
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: "f3d:test:design-parameter#2".into(),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index: 2,
                source_ordinal: 2,
                source: crate::records::parameters::DesignParameterSource::new(
                    "Linear Dimension-2".into(),
                    Some(3),
                    Some(crate::records::identity::Located {
                        value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                        offset: 22,
                    }),
                )
                .unwrap(),
                expression: "1 mm".into(),
                expression_offset: 40,
                source_kind_offset: 60,

                unit: Some(crate::records::identity::RecordedValue {
                    value: "native-unit".into(),
                    offset: 70,
                }),
                name: "d2".into(),
                name_offset: 80,
                evaluated_value: 0.1,
                evaluated_value_offset: 90,
            },
        )
        .unwrap(),
    );
    native.design_parameter_scopes.push(
        DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: "native:unprojected-scope".into(),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index: 3,
                frame_length: 200,
                kind_offset: 0,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: None,
                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: 9,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![1],
                    vec![0],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::try_from(
                    "Unsupported".to_owned(),
                )
                .expect("native family name")
                .try_into()
                .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "001".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 1,
            }
            .with_fixture_layout(),
        )
        .unwrap(),
    );
    assert_eq!(
        design_projection_gaps(&ir, &native),
        DesignProjectionGaps {
            unresolved_body_bindings: 0,
            history_only_body_bindings: 0,
            incomplete_features: 6,
            native_reference_images: 0,
            native_decals: 0,
            unprojected_feature_scopes: 1,
            unprojected_parameters: 1,
            unresolved_parameter_owners: 1,
            untyped_parameter_units: 1,
            unresolved_expression_dependencies: 0,
            unprojected_history_dependencies: 0,
            ambiguous_history_dependencies: 0,
            native_sketch_relations: 1,
            native_dimensions: 1,
            unprojected_sketch_placements: 1,
            unprojected_sketch_points: 1,
            unprojected_sketch_curves: 1,
            unprojected_sketch_surfaces: 0,
            unprojected_sketch_texts: 0,
            unprojected_sketch_text_styles: 0,
            unprojected_sketch_relations: 0,
            unprojected_sketch_pattern_states: 0,
            unprojected_dimensions: 0,
            profile_selections: 2,
            path_selections: 1,
            face_selections: 1,
            active_face_substitutions: 0,
            body_selections: 0,
            partially_resolved_face_members: 0,
            native_edge_selections: 2,
            partially_resolved_edge_members: 1,
            unresolved_edge_selections: 1,
            unrepaired_lost_edge_references: 1,
        }
    );

    native.design_construction_operand_groups.push(
        serde_json::from_value(serde_json::json!({
            "id": "native:partial-edges",
            "scope_record_index": 1,
            "scope_reference_ordinal": 0,
            "record_index": 2,
            "byte_offset": 0,
            "class_tag": "300",
            "members": [3],
            "lost_edge_references": ["f3d:test:lost-edge-reference#2"],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 1.0,
                "opaque_scalar_offset": 22,
                "variant": false
            },
            "role": 0x10_0000_0000u64,
            "role_offset": 0,
            "paired_class_tag": "258",
            "paired_byte_offset": 0
        }))
        .expect("lost-reference construction group"),
    );
    ir.model.features[2].evaluation.edit(|definition, _| {
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Fillet { groups },
        ) = definition
        else {
            unreachable!();
        };
        groups[2].edges =
            cadmpeg_ir::features::EdgeSelection::historical(
                cadmpeg_ir::ids::FeatureInputTopologyId::mint(
                    "test:model:feature-input#history-input",
                )
                .expect("identity grammar"),
                vec![cadmpeg_ir::ids::HistoricalEdgeId::mint("history-edge")
                    .expect("identity grammar")],
                "native:partial-edges".into(),
            )
            .unwrap();
    });
    assert_eq!(
        design_projection_gaps(&ir, &native).unrepaired_lost_edge_references,
        0
    );

    native.sketch_points[0].owner_reference = None;
    native.sketch_curve_identities[0].owner_reference = None;
    let ownerless = design_projection_gaps(&ir, &native);
    assert_eq!(ownerless.unprojected_sketch_points, 0);
    assert_eq!(ownerless.unprojected_sketch_curves, 0);
    native.sketch_points[0].owner_reference = Some(1);
    native.sketch_curve_identities[0].owner_reference = Some(1);

    ir.model.sketches.push(Sketch {
        id: SketchId::mint("synthetic:test:id#sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: Default::default(),
        native_ref: Some("native:sketch-placement".into()),
    });
    for (id, native_ref) in [
        ("synthetic:test:id#point", "native:sketch-point"),
        ("synthetic:test:id#curve", "native:sketch-curve"),
    ] {
        ir.model.sketch_entities.push(
            SketchEntity::new(
                SketchEntityId::mint(id).unwrap(),
                SketchId::mint("synthetic:test:id#sketch").unwrap(),
                SketchGeometry::try_from(SketchGeometryDefinition::Point {
                    position: Point2::new(0.0, 0.0),
                })
                .unwrap(),
            )
            .with_native_ref(Some(native_ref.into())),
        );
    }
    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_sketch_placements, 0);
    assert_eq!(gaps.unprojected_sketch_points, 0);
    assert_eq!(gaps.unprojected_sketch_curves, 0);

    ir.model.parameters.push(
        serde_json::from_value(serde_json::json!({
            "id": "synthetic:test:id#parameter-2",
            "ordinal": 2,
            "name": "d2",
            "expression": "1 mm",
            "native_ref": "f3d:test:design-parameter#2"
        }))
        .expect("Design parameter"),
    );
    assert_eq!(
        design_projection_gaps(&ir, &native).unprojected_parameters,
        0
    );
}

#[test]
fn design_projection_gaps_require_unique_scope_state_dependencies() {
    let scope = |record_index, current, previous| {
        DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: format!("f3d:native:scope#{record_index}"),
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                frame_length: 200,
                kind_offset: 0,
                feature_ordinal: std::num::NonZeroU32::new(record_index).expect("nonzero ordinal"),
                feature_ordinal_offset: 0,
                history_state_id: current,
                previous_history_state_id: previous,
                previous_history_state_id_offset: None,
                reference_count_offset: (u64::from(record_index)) + 9,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![1],
                    vec![0],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::try_from(
                    "Unsupported".to_owned(),
                )
                .expect("native family name")
                .try_into()
                .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "001".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: u64::from(record_index) + 1,
            }
            .with_fixture_layout(),
        )
        .unwrap()
    };
    let mut native = F3dNative::default();
    native.design_parameter_scopes = vec![
        scope(1, Some(10), None),
        scope(2, Some(11), Some(10)),
        scope(3, Some(20), None),
        scope(4, Some(20), None),
        scope(5, Some(21), Some(20)),
    ];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features = native
        .design_parameter_scopes
        .iter()
        .map(|scope| {
            serde_json::from_value(serde_json::json!({
                "id": format!("test:model:feature#{}", scope.record_index),
                "ordinal": scope.record_index,
                "suppressed": null,
                "definition": {
                    "definition": "native",
                    "kind": "Unsupported",
                    "parameters": {}
                },
                "native_ref": scope.id
            }))
            .expect("native feature")
        })
        .collect();

    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_feature_scopes, 0);
    assert_eq!(gaps.unprojected_history_dependencies, 1);
    assert_eq!(gaps.ambiguous_history_dependencies, 1);

    let predecessor = ir.model.features[0].id.clone();
    ir.model.features[1].dependencies.insert(predecessor);
    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_history_dependencies, 0);
    assert_eq!(gaps.ambiguous_history_dependencies, 1);
}

#[test]
fn design_projection_gaps_accept_a_dependency_collapsed_through_an_internal_scope() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        100,
    );
    predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(7);
        })
        .unwrap();
    let mut internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#150"),
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        150,
    );
    internal
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut successor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        200,
    );
    successor
        .try_edit(|draft| {
            draft.history_state_id = Some(9);
            draft.previous_history_state_id = Some(8);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![successor, internal, predecessor];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 100,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 200,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(2).unwrap(),
    )
    .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                decode_ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes: &scopes,
                    timelines: std::slice::from_ref(&timeline),
                    ..Default::default()
                },
            )
        })
        .expect("timeline projection through one internal scope");
    let mut native = F3dNative::default();
    native.design_parameter_scopes = scopes;
    native.design_feature_timelines = vec![timeline];
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.features = features;

    let gaps = design_projection_gaps(&ir, &native);
    assert_eq!(gaps.unprojected_feature_scopes, 0);
    assert_eq!(gaps.unprojected_history_dependencies, 0);
    assert_eq!(gaps.ambiguous_history_dependencies, 0);
}

#[test]
fn payload_bearing_dimension_companion_uses_the_governing_dimension_frame() {
    let stream = "f3d:test/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = F3dNative::default();
    native.design_parameters.push(
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: format!("{stream}:design-parameter#28"),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                    .unwrap(),
                record_index: 28,
                source_ordinal: 0,
                source: crate::records::parameters::DesignParameterSource::new(
                    "Linear Dimension-2".into(),
                    Some(29),
                    Some(crate::records::identity::Located {
                        value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                        offset: 22,
                    }),
                )
                .unwrap(),
                expression: "5 mm".into(),
                expression_offset: 40,
                source_kind_offset: 60,

                unit: Some(crate::records::identity::RecordedValue {
                    value: "mm".into(),
                    offset: 90,
                }),
                name: "d1".into(),
                name_offset: 100,
                evaluated_value: 0.5,
                evaluated_value_offset: 110,
            },
        )
        .unwrap(),
    );
    native.design_parameter_owners.push(
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: format!("{stream}:design-parameter-owner#29"),
            byte_offset: 120,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 29,
            scope_record_index: 1,
            local_ordinal: 0,
            evaluated_value: 0.5,
            evaluated_value_offset: 160,
            parameter_record_index: 28,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 30,
        })
        .unwrap(),
    );
    native.design_parameter_companions.push(
        DesignParameterCompanion::unbound(
            format!("{stream}:design-parameter-companion#30"),
            220,
            crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
            30,
            29,
            std::num::NonZeroU64::new(1).unwrap(),
            262,
        )
        .bound(crate::records::parameters::DesignCompanionPayload::new(
            278,
            100,
            Vec::new(),
        )),
    );
    assert_eq!(
        unresolved_dimension_companion_count(&DimensionCompanionSources::from_native(&native), &ir),
        1
    );
    ir.model.sketch_constraints.push(
        serde_json::from_value(serde_json::json!({
            "id": "f3d:model:sketch-constraint#dimension",
            "sketch": "f3d:model:sketch#1",
            "definition": {
                "kind": "distance",
                "entities": ["f3d:model:sketch-entity#first", "f3d:model:sketch-entity#second"],
                "parameter": "f3d:model:parameter#1"
            },
            "native_ref": format!("{stream}:design-parameter-companion#30")
        }))
        .expect("neutral dimension constraint"),
    );
    assert_eq!(
        unresolved_dimension_companion_count(&DimensionCompanionSources::from_native(&native), &ir),
        0
    );
    ir.model.sketch_constraints.clear();

    let mut recipe_backed = native.clone();
    recipe_backed
        .design_dimension_recipe_records
        .push(DesignDimensionRecipeRecord {
            id: format!("{stream}:design-dimension-recipe-record#31"),
            companion_record_index: 30,
            recipe_ordinal: 0,
            recipe_id: format!("{stream}:construction-recipe#31"),
            recipe_kind: crate::records::recipes::ConstructionRecipeKind::Edge,
            byte_offset: 278,
            class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned())
                .unwrap(),
            record_index: 31,
            frame_length: 100,
            prefix_offset: 300,
            prefix_bytes: Vec::new().into(),
            references: Vec::new(),
            program_offset: 320,
            program: vec![-1],
            matching_edge_operand_ids: Vec::new(),
        });
    assert_eq!(
        unresolved_dimension_companion_count(
            &DimensionCompanionSources::from_native(&recipe_backed),
            &ir
        ),
        0
    );

    native.design_dimension_locus_pairs = vec![DesignDimensionLocusPair::try_new(
        crate::records::dimensions::DesignDimensionLocusPairDraft {
            id: format!("{stream}:design-dimension-locus-pair#278"),
            companion_record_index: 99,
            governing_companion_record_index: 30,
            byte_offset: 278,
            class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned())
                .unwrap(),
            record_index: 31,
            frame_length: 100,
            opaque_index: Some(crate::records::identity::Located {
                value: 0,
                offset: 313,
            }),
            loci: [
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(40),
                    geometry_reference_offset: 318,
                    role: 1,
                    role_offset: 328,
                },
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(41),
                    geometry_reference_offset: 333,
                    role: 2,
                    role_offset: 343,
                },
            ],
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 378,
        },
    )
    .unwrap()]
    .try_into()
    .expect("pair arena");
    assert_eq!(
        unresolved_dimension_companion_count(&DimensionCompanionSources::from_native(&native), &ir),
        0
    );
    assert!(container_only_dimension_parameters(&native).is_empty());
    let mut pairs = native.design_dimension_locus_pairs.to_vec();
    pairs[0].companion_record_index = 30;
    pairs[0].governing_companion_record_index = 99;
    native.design_dimension_locus_pairs = pairs.try_into().expect("pair arena");
    assert_eq!(
        unresolved_dimension_companion_count(&DimensionCompanionSources::from_native(&native), &ir),
        0
    );
    assert_eq!(container_only_dimension_parameters(&native).len(), 1);

    native.design_dimension_locus_pairs = Default::default();
    native.design_dimension_null_locus_pairs = vec![DesignDimensionLocusPair::try_new(
        crate::records::dimensions::DesignDimensionLocusPairDraft {
            id: format!("{stream}:design-dimension-null-locus-pair#278"),
            companion_record_index: 99,
            governing_companion_record_index: 30,
            byte_offset: 278,
            class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned())
                .unwrap(),
            record_index: 31,
            frame_length: 100,
            opaque_index: None,
            loci: [
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: None,
                    geometry_reference_offset: 303,
                    role: 14,
                    role_offset: 313,
                },
                crate::records::dimensions::DesignDimensionAnnotationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(40),
                    geometry_reference_offset: 318,
                    role: 3,
                    role_offset: 328,
                },
            ],
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 378,
        },
    )
    .unwrap()]
    .try_into()
    .expect("pair arena");
    assert_eq!(
        unresolved_dimension_companion_count(&DimensionCompanionSources::from_native(&native), &ir),
        0
    );
    let mut pairs = native.design_dimension_null_locus_pairs.to_vec();
    pairs[0].companion_record_index = 30;
    pairs[0].governing_companion_record_index = 99;
    native.design_dimension_null_locus_pairs = pairs.try_into().expect("pair arena");
    assert_eq!(
        unresolved_dimension_companion_count(&DimensionCompanionSources::from_native(&native), &ir),
        0
    );
}

#[test]
fn appearance_base_colors_fill_only_uncolored_unambiguous_targets() {
    use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let body = ir.model.bodies[0].id.clone();
    let first_face = ir.model.faces[0].id.clone();
    let second_face = ir.model.faces[1].id.clone();
    let direct = Color::new(0.9, 0.8, 0.7, 1.0).expect("valid color");
    let material = Color::new(0.1, 0.2, 0.3, 1.0).expect("valid color");
    ir.model.bodies[0].color = Some(direct);
    ir.model.appearances.push(Appearance {
        id: AppearanceId::mint("f3d:test:appearance#material").expect("identity grammar"),
        name: None,
        asset_guid: None,
        library_id: None,
        textures: Vec::new(),
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color: Some(material),
        properties: Default::default(),
    });
    let binding = |id: &str, target| AppearanceBinding {
        id: format!("test:model:appearance-binding#{id}")
            .try_into()
            .expect("valid identity"),
        target,
        appearance: AppearanceId::mint("f3d:test:appearance#material").expect("identity grammar"),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: Default::default(),
    };
    ir.model.appearance_bindings = vec![
        binding("body", AppearanceTarget::Body(body)),
        binding("face", AppearanceTarget::Face(first_face)),
        binding("ambiguous-a", AppearanceTarget::Face(second_face.clone())),
        binding("ambiguous-b", AppearanceTarget::Face(second_face)),
    ];

    apply_appearance_base_colors(&mut ir);
    assert_eq!(ir.model.bodies[0].color, Some(direct));
    assert_eq!(ir.model.faces[0].color, Some(material));
    assert_eq!(ir.model.faces[1].color, None);
}

#[test]
fn fixture_pattern_states_have_a_distinct_native_retention_loss_count() {
    let ir = cadmpeg_ir::document::CadIr::empty();
    for case in crate::test_support::sketch_relations::relation_fixture_records() {
        let Some(neutral_state) = case.pattern_neutral_state else {
            continue;
        };
        let mut native = F3dNative::default();
        native.sketch_relations.push(case.decode());
        let gaps = design_projection_gaps(&ir, &native);
        let unsupported = !neutral_state;
        assert_eq!(
            gaps.unprojected_sketch_pattern_states,
            usize::from(unsupported),
            "{}",
            case.label
        );
        assert_eq!(
            gaps.unprojected_sketch_relations,
            usize::from(!unsupported),
            "{}",
            case.label
        );
    }
}
