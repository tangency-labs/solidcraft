// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::operands::assign_extrude_face_roles;
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::face_resolve::resolved_body_recipe_shape;
use crate::design::feature_project::project_extrude;
use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::bind_extrude_profile_selections;
use crate::design::test_support::parameter_owner_frame;
use crate::design::test_support::parameter_record;
use crate::ids::neutral_sketch_id;
use crate::ids::neutral_spatial_sketch_id;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::extrude::DesignExtrudeTargetOrdinal;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeDistance;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::body_recipe::DesignBodyRecipeOperand;
use crate::records::topology::body_recipe::DesignBodyRecipeReference;
use crate::records::topology::body_recipe::DesignOperandOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignExtrudeFaceRole;
use crate::records::topology::extrude_selection::DesignExtrudeSelectionGroup;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use crate::records::topology::{
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};
use cadmpeg_ir::features::Feature;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::ProfileRef;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::sketches::SketchId;
use std::collections::BTreeMap;

fn set_extrude_operation(scope: &mut DesignParameterScope, operation: DesignExtrudeOperation) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            operation: value, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::ShiftedReferenceAware {
            operation: value, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            operation: value, ..
        },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = operation;
}

fn set_extrude_extent(scope: &mut DesignParameterScope, extent: DesignExtrudeExtent) {
    let Some(prologue) = scope.extrude_prologue_mut() else {
        panic!("test scope must carry an Extrude prologue");
    };
    match prologue {
        DesignExtrudePrologue::LegacyDistance { .. } => {
            assert_eq!(extent, DesignExtrudeExtent::OneSidedDistance);
        }
        DesignExtrudePrologue::ReferenceAware {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            first_side_target_ordinal,
            ..
        } => {
            *value = extent;
            (direction_face_extend_values[0], *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => (1, [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => (1, [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => (1, [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => (1, [4, 0]),
                DesignExtrudeExtent::TwoSidedToFaces => (2, [2, 0]),
                DesignExtrudeExtent::TwoSidedDistance => (2, [1, 1]),
                DesignExtrudeExtent::TwoSidedDistanceToFace => (2, [1, 2]),
                DesignExtrudeExtent::SymmetricDistance => (3, [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => (3, [4, 4]),
            };
            if extent != DesignExtrudeExtent::OneSidedToFace {
                *first_side_target_ordinal = None;
            }
        }
        DesignExtrudePrologue::LegacyShifted {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            *value = Some(extent);
            (*direction_face_extend_values, *side_extent_discriminators) = match extent {
                DesignExtrudeExtent::OneSidedDistance => ([1, 0], [1, 0]),
                DesignExtrudeExtent::OneSidedToFace => ([1, 0], [2, 0]),
                DesignExtrudeExtent::OneSidedThroughNext => ([1, 0], [3, 0]),
                DesignExtrudeExtent::OneSidedThroughAll => ([1, 0], [4, 0]),
                DesignExtrudeExtent::TwoSidedToFaces => ([2, 0], [2, 0]),
                DesignExtrudeExtent::TwoSidedDistance => ([2, 0], [1, 1]),
                DesignExtrudeExtent::TwoSidedDistanceToFace => ([2, 0], [1, 2]),
                DesignExtrudeExtent::SymmetricDistance => ([3, 0], [1, 0]),
                DesignExtrudeExtent::SymmetricThroughAll => ([3, 0], [4, 4]),
            };
        }
        DesignExtrudePrologue::ShiftedReferenceAware {
            extent: value,
            direction_face_extend_values,
            side_extent_discriminators,
            ..
        } => {
            assert_eq!(extent, DesignExtrudeExtent::TwoSidedToFaces);
            *value = extent;
            (*direction_face_extend_values, *side_extent_discriminators) = ([2, 1], [2, 0]);
        }
    }
}

fn set_extrude_direction_reversed(scope: &mut DesignParameterScope, reversed: bool) {
    let Some(
        DesignExtrudePrologue::LegacyDistance {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::ShiftedReferenceAware {
            direction_reversed, ..
        }
        | DesignExtrudePrologue::LegacyShifted {
            direction_reversed, ..
        },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *direction_reversed = reversed;
}

fn set_extrude_start(scope: &mut DesignParameterScope, start: DesignExtrudeStart) {
    let Some(
        DesignExtrudePrologue::ReferenceAware { start: value, .. }
        | DesignExtrudePrologue::ShiftedReferenceAware { start: value, .. }
        | DesignExtrudePrologue::LegacyShifted { start: value, .. },
    ) = scope.extrude_prologue_mut()
    else {
        panic!("test scope must carry an Extrude prologue");
    };
    *value = start;
}

#[test]
fn extrude_parameters_project_blind_two_sided_and_reversed_extents() {
    crate::test_support::with_decode_context(|decode_ctx| {
        use cadmpeg_ir::features::{
            BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection,
            LinearTermination, PlanarProfileRef, ProfileRef,
        };

        let parameter = |source_kind: &str, unit: &str, value| {
            parse_design_parameter_record(&parameter_record(
                Some(44),
                "value",
                source_kind,
                Some(unit),
                "d1",
                value,
            ))
            .expect("generated feature parameter is canonical")
        };
        let mut scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 210,
            payload: DesignScopePayload::Extrude(Some(
                crate::records::feature::scope::DesignExtrudeScope {
                    extrude_prologue: Some(DesignExtrudePrologue::ReferenceAware {
                        reference: None,
                        operation: DesignExtrudeOperation::NewBody,
                        operation_offset: 128,
                        direction_face_extend_values: [1, 2],
                        side_extent_discriminators: [1, 0],
                        side_extent_discriminator_offsets: [177, 190],
                        first_side_target_ordinal: None,
                        extent: DesignExtrudeExtent::OneSidedDistance,
                        direction_face_extend_offsets: [132, 136],
                        direction_reversed: false,
                        direction_reversed_offset: 140,
                        solid_operation: true,
                        solid_operation_offset: 141,
                        start: DesignExtrudeStart::ProfilePlane,
                        start_offset: 142,
                    }),
                    extrude_profile: Some(
                        DesignSketchProfileOperand::try_new(
                            crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                                scope_reference_ordinal: 0,
                                record_index: 100,
                                byte_offset: 300,
                                class_tag: crate::records::references::DesignClassTag::try_from(
                                    "308".to_owned(),
                                )
                                .unwrap(),
                                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                                    "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".to_owned(),
                                )
                                .unwrap(),
                                asset_id_offset: 330,
                                entity_id: crate::records::identity::DesignEntityId::try_from(
                                    "0_172".to_owned(),
                                )
                                .expect("valid entity identity"),
                                entity_reference_offset: 420,
                                region_selection: None,
                                paired_class_tag:
                                    crate::records::references::DesignClassTag::try_from(
                                        "259".to_owned(),
                                    )
                                    .unwrap(),
                                paired_byte_offset: 520,
                            },
                        )
                        .unwrap(),
                    ),
                    ..crate::records::feature::scope::DesignExtrudeScope::default()
                },
            )),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![185],
                "reference_members",
            )
            .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        }
        .with_fixture_layout(),
    )
    .unwrap();
        let placement = DesignSketchPlacement {
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                600,
                crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, -1.0, 0.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ])
                    .unwrap(),
                ),
            )
            .unwrap(),

            id: "f3d:Design/BulkStream.dat:placement#200".into(),
            scope_record_index: Some(11),
            entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
                .expect("valid entity ID"),

            visibility: None,

            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            record_index: 200,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "260".to_owned(),
            )
            .unwrap(),
        };
        let along = parameter("AlongDistance", "mm", 0.55);
        let taper = parameter("TaperAngle", "deg", 0.2);
        let blind = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed blind Extrude");
        assert!(matches!(
            &blind,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(profile)),
                direction: ExtrudeDirection::ProfileNormal {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind { length: actual_length },
                        draft: Some(actual_draft),
                    },
                },
                op: BooleanOp::NewBody,
                solid: Some(true),
                ..
            }) if (profile == &neutral_sketch_id(&placement)) && actual_length.get() == 5.5 && actual_draft.get() == 0.2
        ));
        let reference_aware_prologue = scope.extrude_prologue();
        let Some(DesignExtrudePrologue::ReferenceAware {
            solid_operation, ..
        }) = scope.extrude_prologue_mut()
        else {
            panic!("reference-aware Extrude prologue");
        };
        *solid_operation = false;
        let sheet = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed sheet Extrude");
        assert!(matches!(
            sheet,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                solid: Some(false),
                ..
            })
        ));
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_prologue =
                Some(DesignExtrudePrologue::LegacyShifted {
                    operation_prefix_marker_offset: None,
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: 127,
                    direction_face_extend_values: [3, 2],
                    side_extent_discriminators: [1, 0],
                    side_extent_discriminator_offsets: [206, 210],
                    extent: Some(DesignExtrudeExtent::SymmetricDistance),
                    direction_face_extend_offsets: [131, 135],
                    direction_reversed: false,
                    direction_reversed_offset: 139,
                    solid_operation: true,
                    solid_operation_offset: 140,
                    start: DesignExtrudeStart::ProfilePlane,
                    start_offset: 141,
                });
        }
        let symmetric = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed symmetric Extrude");
        assert!(matches!(
            symmetric,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        draft: Some(actual_draft),
                    },
                },
                ..
            }) if actual_length.get() == 5.5 && actual_draft.get() == 0.2
        ));
        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedThroughAll);
        set_extrude_direction_reversed(&mut scope, true);
        let through_all = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(1, &taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed through-all Extrude");
        assert!(matches!(
            through_all,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ReversedProfileNormal {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ThroughAll {},
                        draft: Some(actual_draft),
                    },
                },
                ..
            }) if actual_draft.get() == 0.2
        ));
        set_extrude_direction_reversed(&mut scope, false);
        set_extrude_extent(&mut scope, DesignExtrudeExtent::SymmetricThroughAll);
        let symmetric_through_all = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(1, &taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed symmetric through-all Extrude");
        assert!(matches!(
            symmetric_through_all,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ProfileNormal {},
                extent: ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::ThroughAll {},
                        draft: Some(actual_draft),
                    },
                },
                ..
            }) if actual_draft.get() == 0.2
        ));
        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
        let selection = DesignExtrudeSelectionGroup::try_from(
            crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
                id: "f3d:Design/BulkStream.dat:selection#300".into(),
                scope_record_index: scope.record_index,
                scope_reference_ordinal: 0,
                record_index: 300,
                byte_offset: 700,
                class_tag: "308".to_owned(),
                member_count_offset: 732,
                members: vec![301],
                member_offsets: vec![737],
                opaque_index: 1,
                opaque_index_offset: 747,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 751,
                variant: Some(false),
                typed_references: false,
                paired_class_tag: "259".to_owned(),
                paired_byte_offset: 800,
            },
        )
        .unwrap();
        let mut feature = Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:model:feature#extrude").expect("identity grammar"),
            ordinal: 0,
            name: Some("Extrude".into()),
            suppressed: Some(false),
            dependencies: Default::default(),
            source_properties: BTreeMap::new(),
            source_tag: Some("Extrude".into()),
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(blind),
            native_ref: Some(scope.id.clone()),
        };
        let profile_work = ProfileGeometryWork::new();
        bind_extrude_profile_selections(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            std::slice::from_ref(&selection),
            &[],
            &[],
            &crate::design::profile_select::SketchCurveSelectionResolution {
                profile_curves: &[],
                scopes: &[],
                groups: &[],
                operands: &[],
                placements: &[],
                curve_identities: &[],
                sketches: &[],
                sketch_entities: &[],
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &std::collections::HashMap::new(),
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
                profile_work: &profile_work,
                ctx: decode_ctx,
            },
        )
        .unwrap();
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Native(ref native)),
                ..
            }) if native == &selection.id
        ));
        set_extrude_direction_reversed(&mut scope, true);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        set_extrude_direction_reversed(&mut scope, false);
        let unsupported = parameter("UnclassifiedControl", "mm", 1.0);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &unsupported)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        let side_two_taper = parameter("Side2TaperAngle", "deg", -0.3);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &side_two_taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        let invalid_taper = parameter("TaperAngle", "native-unit", 0.2);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &invalid_taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        let mut owned_along = along.clone();
        owned_along.id = "f3d:Design/BulkStream.dat:parameter#45".into();
        owned_along.record_index = 45;
        owned_along
            .try_set_source(
                crate::records::parameters::DesignParameterSource::new(
                    owned_along.source_kind_str().to_owned(),
                    Some(44),
                    owned_along.family_discriminator(),
                )
                .unwrap(),
            )
            .unwrap();
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical")
            .into_record("Design/BulkStream.dat", 0)
            .unwrap();
        {
            let mut wire =
                crate::records::parameters::DesignParameterOwnerWire::from(owner.clone());
            wire.id = "f3d:Design/BulkStream.dat:owner#44".into();
            wire.record_index = 44;
            wire.scope_record_index = scope.record_index;
            wire.parameter_record_index = owned_along.record_index;
            owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
        }
        let mut sketch_scope = scope.clone();
        sketch_scope.id = "f3d:Design/BulkStream.dat:scope#11".into();
        sketch_scope.record_index = placement
            .scope_record_index
            .expect("test placement carries a scope record index");
        sketch_scope
            .try_edit(|draft| {
                draft.payload = crate::records::feature::scope::DesignFeatureKind::Sketch
                    .try_into()
                    .unwrap();
            })
            .unwrap();
        let scopes = vec![sketch_scope, scope.clone()];
        let crate::design::feature_project::ProjectedParameterDesign { mut features, .. } =
            crate::test_support::with_decode_context(|ctx| {
                let scopes = &scopes;
                let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    ctx,
                    &crate::design::feature_project::ProjectInputs {
                        native: std::slice::from_ref(&owned_along),
                        owners: std::slice::from_ref(&owner),
                        scopes,
                        placements: std::slice::from_ref(&placement),
                        timelines: &timelines,
                        ..Default::default()
                    },
                )
                .expect("test projection has a synthetic exact timeline")
            });
        let sketches = [cadmpeg_ir::sketches::Sketch {
            id: neutral_sketch_id(&placement),
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
            native_ref: Some(placement.id.clone()),
        }];
        {
            crate::design::feature_project::bind_sketch_feature_geometry(
                &mut features,
                &scopes,
                std::slice::from_ref(&placement),
                &sketches,
                &[],
            )
        }
        .unwrap();
        let sketch_feature = features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
                )
            })
            .expect("neutral Sketch feature");
        let extrude_feature = features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
                )
            })
            .expect("neutral Extrude feature");
        assert_eq!(
            extrude_feature.dependencies.as_slice(),
            [sketch_feature.id.clone()]
        );

        let crate::design::feature_project::ProjectedParameterDesign {
            features: mut spatial_features,
            ..
        } = crate::test_support::with_decode_context(|ctx| {
            let scopes = &scopes;
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: std::slice::from_ref(&owned_along),
                    owners: std::slice::from_ref(&owner),
                    scopes,
                    placements: std::slice::from_ref(&placement),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
        let spatial_sketch = cadmpeg_ir::sketches::SpatialSketch {
            id: neutral_spatial_sketch_id(&placement),
            name: None,
            configuration: None,
            visible: None,
            profiles: vec![cadmpeg_ir::sketches::SpatialSketchProfile::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                vec![cadmpeg_ir::sketches::SpatialSketchEntityUse {
                    entity: cadmpeg_ir::sketches::SpatialSketchEntityId::mint(
                        "synthetic:test:spatial-entity#extrude-profile",
                    )
                    .unwrap(),
                    reversed: false,
                }],
            )
            .expect("fixture collection admission")],
            native_ref: Some(placement.id.clone()),
        };
        {
            crate::design::feature_project::bind_sketch_feature_geometry(
                &mut spatial_features,
                &scopes,
                std::slice::from_ref(&placement),
                &[],
                std::slice::from_ref(&spatial_sketch),
            )
        }
        .unwrap();
        let spatial_feature = spatial_features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::SpatialSketch { .. })
                )
            })
            .expect("neutral spatial Sketch feature");
        let spatial_extrude = spatial_features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
                )
            })
            .expect("spatial-profile Extrude feature");
        assert!(matches!(
            spatial_extrude.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::SpatialSketchProfiles {
                    ref sketch,
                    ref profiles
                },
                ..
            }) if sketch == &spatial_sketch.id && profiles.as_slice() == [0]
        ));
        assert_eq!(
            spatial_extrude.dependencies.as_slice(),
            [spatial_feature.id.clone()]
        );

        let crate::design::feature_project::ProjectedParameterDesign {
            features: mut open_spatial_features,
            ..
        } = crate::test_support::with_decode_context(|ctx| {
            let scopes = &scopes;
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: std::slice::from_ref(&owned_along),
                    owners: std::slice::from_ref(&owner),
                    scopes,
                    placements: std::slice::from_ref(&placement),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
        let open_spatial_sketch = cadmpeg_ir::sketches::SpatialSketch {
            id: neutral_spatial_sketch_id(&placement),
            name: None,
            configuration: None,
            visible: None,
            profiles: Vec::new(),
            native_ref: Some(placement.id.clone()),
        };
        {
            crate::design::feature_project::bind_sketch_feature_geometry(
                &mut open_spatial_features,
                &scopes,
                std::slice::from_ref(&placement),
                &[],
                std::slice::from_ref(&open_spatial_sketch),
            )
        }
        .unwrap();
        let open_spatial_extrude = open_spatial_features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
                )
            })
            .expect("open spatial-profile Extrude feature");
        assert!(matches!(
            open_spatial_extrude.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::SpatialSketchSelection {
                    ref sketch,
                    ref selections
                },
                ..
            }) if sketch == &open_spatial_sketch.id
                && selections.as_slice() == [format!(
                    "f3d:Design/BulkStream.dat:design-record-header#{}",
                    scope
                        .extrude_profile()
                        .as_ref()
                        .expect("test profile operand")
                        .byte_offset()
                )]
        ));

        let body_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#101".into(),
            scope_record_index: 12,
            scope_reference_ordinal: 1,
            record_index: 101,
            byte_offset: 1000,
            class_tag: crate::records::references::DesignClassTag::try_from("332".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1026,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 300,
                        offset: 1044,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 180,
                    opaque_index_offset: 1072,
                    opaque_scalar: 0.125,
                    opaque_scalar_offset: 1076,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeBodiesB,
            role_offset: 1054,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1125,
        },
    )
    .unwrap();
        set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
        let target_body = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                std::slice::from_ref(&body_group),
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed target-body Extrude");
        assert!(matches!(
            target_body,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Join,
                ..
            })
        ));

        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_prologue = reference_aware_prologue;
        }
        set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
        let mut target_shape_group = body_group.clone();
        target_shape_group.id = "f3d:Design/BulkStream.dat:operand-group#105".into();
        target_shape_group.record_index = 105;
        target_shape_group.scope_reference_ordinal = 2;
        target_shape_group
            .try_set_members(vec![crate::records::identity::Located {
                value: 201,
                offset: 1026,
            }])
            .unwrap();
        target_shape_group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X5,
            );
        let Some(DesignExtrudePrologue::ReferenceAware {
            first_side_target_ordinal,
            ..
        }) = scope.extrude_prologue_mut()
        else {
            panic!("reference-aware target-shape Extrude prologue");
        };
        *first_side_target_ordinal = Some(DesignExtrudeTargetOrdinal {
            scope_reference_ordinal: target_shape_group.scope_reference_ordinal,
            scope_reference_ordinal_offset: 187,
        });
        let mut unrelated_target_group = target_shape_group.clone();
        unrelated_target_group.id = "f3d:Design/BulkStream.dat:operand-group#106".into();
        unrelated_target_group.record_index = 106;
        unrelated_target_group.scope_reference_ordinal = 3;
        unrelated_target_group
            .try_set_members(vec![crate::records::identity::Located {
                value: 202,
                offset: unrelated_target_group.members()[0].offset,
            }])
            .unwrap();
        let mut target_shape_operand = DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: "f3d:Design/BulkStream.dat:body-recipe-operand#201".into(),
                scope_record_index: scope.record_index,
                owner: DesignOperandOwner::Group {
                    group_record_index: target_shape_group.record_index,
                    group_member_ordinal: 0,
                },
                record_index: 201,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("295".to_owned())
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
                    design_reference: 301,
                    design_reference_offset: 25,
                    form: 33,
                    form_offset: 33,
                    candidate_faces: vec![
                        FaceId::mint("f3d:brep:entity#12").expect("identity grammar"),
                        FaceId::mint("f3d:brep:entity#19").expect("identity grammar"),
                    ],
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: Vec::new(),
                }],
                nested_record_index: 204,
                nested_record_index_offset: 38,
                recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#205".into(),
                resolved_face_slot: None,
                resolved_body_state_id: None,
                resolved_body_slot: None,
                resolved_body_face_slots: Vec::new(),
                next_record_index: 205,
                next_byte_offset: 256,
            },
        )
        .unwrap();
        let unresolved_target_shape = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &taper)],
                &[
                    body_group.clone(),
                    target_shape_group.clone(),
                    unrelated_target_group.clone(),
                ],
                &[],
                std::slice::from_ref(&placement),
                std::slice::from_ref(&target_shape_operand),
            )
        })
        .expect("project the extrude feature")
        .expect("typed target-shape Extrude");
        assert!(matches!(
            unresolved_target_shape,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToShape {
                            target: FaceSelection::Native(ref native),
                        },
                        ..
                    },
                },
                ..
            }) if native == &target_shape_group.id
        ));

        target_shape_operand.resolved_body_state_id = Some(7);
        target_shape_operand.resolved_body_slot = Some(3);
        target_shape_operand.resolved_body_face_slots = vec![12, 19, 27];
        let target_shape = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &taper)],
                &[
                    body_group.clone(),
                    target_shape_group.clone(),
                    unrelated_target_group,
                ],
                &[],
                std::slice::from_ref(&placement),
                std::slice::from_ref(&target_shape_operand),
            )
        })
        .expect("project the extrude feature")
        .expect("resolved target-shape Extrude");
        let feature = crate::ids::neutral_feature_id(&scope);
        let feature_key = feature.key();
        let prefix = crate::ids::history_input_prefix(&feature_key, 7);
        assert!(matches!(
            target_shape,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToShape {
                            target: FaceSelection::Historical {
                                ref state,
                                ref faces,
                                ref native,
                            },
                        },
                        ..
                    },
                },
                ..
            }) if state == &crate::ids::feature_input_topology_id(&feature, 7)
                && faces.as_slice() == [
                    crate::ids::history_input_face_id(&prefix, 12),
                    crate::ids::history_input_face_id(&prefix, 19),
                    crate::ids::history_input_face_id(&prefix, 27),
                ]
                && native.as_str() == target_shape_group.id
        ));

        let mut multi_target_group = target_shape_group.clone();
        multi_target_group
            .try_set_members(
                multi_target_group
                    .members()
                    .iter()
                    .copied()
                    .chain([crate::records::identity::Located {
                        value: 202,
                        offset: 1037,
                    }])
                    .collect(),
            )
            .unwrap();
        let mut second_target_operand = target_shape_operand.clone();
        second_target_operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#202".into();
        second_target_operand.owner = DesignOperandOwner::Group {
            group_record_index: multi_target_group.record_index,
            group_member_ordinal: 1,
        };
        let mut draft = second_target_operand.into_draft();
        draft.record_index = 202;
        draft.nested_record_index = u64::from(draft.record_index + 3);
        draft.next_record_index = draft.record_index + 4;
        second_target_operand =
            crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(draft).unwrap();
        second_target_operand.resolved_body_slot = Some(4);
        second_target_operand.resolved_body_face_slots = vec![30, 31];
        let operands = [target_shape_operand.clone(), second_target_operand.clone()];
        assert!(matches!(
            resolved_body_recipe_shape(&scope, &multi_target_group, &operands),
            Some(FaceSelection::Historical { faces, .. })
                if faces.as_slice() == [
                    crate::ids::history_input_face_id(&prefix, 12),
                    crate::ids::history_input_face_id(&prefix, 19),
                    crate::ids::history_input_face_id(&prefix, 27),
                    crate::ids::history_input_face_id(&prefix, 30),
                    crate::ids::history_input_face_id(&prefix, 31),
                ]
        ));
        second_target_operand.resolved_body_state_id = Some(8);
        assert!(resolved_body_recipe_shape(
            &scope,
            &multi_target_group,
            &[target_shape_operand.clone(), second_target_operand]
        )
        .is_none());

        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
        set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
        let sketch_profile = scope.extrude_profile().cloned();
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_profile = None;
        }
        let mut first_profile_group = body_group.clone();
        first_profile_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
        first_profile_group.record_index = 102;
        first_profile_group.scope_reference_ordinal = 0;
        first_profile_group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile;
        let mut second_profile_group = first_profile_group.clone();
        second_profile_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
        second_profile_group.record_index = 103;
        second_profile_group.scope_reference_ordinal = 1;
        let multiple_profiles = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[first_profile_group.clone(), second_profile_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed multi-profile Extrude");
        assert!(matches!(
            multiple_profiles,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Native(ref native)),
                op: BooleanOp::NewBody,
                ..
            }) if native == &scope.id
        ));
        second_profile_group.scope_reference_ordinal = 0;
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[first_profile_group, second_profile_group],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_profile = sketch_profile;
        }
        set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);

        let mut profile_group = body_group.clone();
        profile_group.id = "f3d:Design/BulkStream.dat:operand-group#104".into();
        profile_group.record_index = 104;
        profile_group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile;
        let direct_profile_with_selection_group =
            crate::test_support::with_decode_context(|decode_ctx| {
                project_extrude(
                    decode_ctx,
                    &scope,
                    &[(0, &along), (1, &taper)],
                    &[body_group.clone(), profile_group.clone()],
                    &[],
                    std::slice::from_ref(&placement),
                    &[],
                )
            })
            .expect("project the extrude feature")
            .expect("direct sketch profile with a scoped selection group");
        assert!(matches!(
            direct_profile_with_selection_group,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(ref profile)),
                ..
            }) if profile == &neutral_sketch_id(&placement)
        ));
        {
            let value = Some(DesignFixedExtrudeParameters {
                along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
                    DesignFixedExtrudeScalar {
                        value: cadmpeg_ir::scalar::PositiveReal::new(0.55).unwrap(),
                        record_index: 105,
                        value_offset: 600,
                    },
                )),
                taper_angle: None,
                against_distance: None,
                side_two_taper_angle: None,
                profile_offset: None,
                side_one_offset: None,
                ..Default::default()
            });
            if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
                scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default)
                    .fixed_extrude_parameters = value;
            }
        }
        let zero_side_offset = parameter("Side1Offset", "mm", 0.0);
        let hybrid = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &zero_side_offset), (1, &taper)],
                &[body_group.clone(), profile_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed hybrid fixed-distance Extrude");
        assert!(matches!(
            hybrid,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    },
                },
                ..
            }) if actual_length.get() == 5.5
        ));
        set_extrude_direction_reversed(&mut scope, true);
        let reversed_hybrid = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &zero_side_offset), (1, &taper)],
                &[body_group.clone(), profile_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed reversed hybrid fixed-distance Extrude");
        assert!(matches!(
            reversed_hybrid,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ReversedProfileNormal {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    },
                },
                ..
            }) if actual_length.get() == 5.5
        ));
        set_extrude_direction_reversed(&mut scope, false);
        {
            let value = None;
            if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
                scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default)
                    .fixed_extrude_parameters = value;
            }
        }
        let mut native_profile_scope = scope.clone();
        {
            let value = None;
            if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
            | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
                native_profile_scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default).extrude_profile = value;
            }
        }
        let reversed_native_profile = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &native_profile_scope,
                &[(0, &parameter("AlongDistance", "mm", -0.2)), (1, &taper)],
                &[body_group.clone(), profile_group.clone()],
                &[],
                &[],
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed reversed Extrude with a native profile");
        assert!(matches!(
            reversed_native_profile,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Native(ref native)),
                direction: ExtrudeDirection::ReversedProfileNormal {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    },
                },
                op: BooleanOp::Join,
                ..
            }) if (native == &profile_group.id) && actual_length.get() == 2.0
        ));

        let mut face_group = body_group.clone();
        face_group.id = "f3d:Design/BulkStream.dat:operand-group#102".into();
        face_group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                encoding:
                    crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
                usage: DesignExtrudeFaceRole::Termination,
            };
        let mut ordered_faces = [face_group.clone(), face_group.clone()];
        set_extrude_start(&mut scope, DesignExtrudeStart::FromFace);
        assign_extrude_face_roles(&scope, &mut ordered_faces);
        assert_eq!(
            ordered_faces.map(|group| group.extrude_face_role()),
            [
                Some(DesignExtrudeFaceRole::Start),
                Some(DesignExtrudeFaceRole::Termination)
            ]
        );
        set_extrude_start(&mut scope, DesignExtrudeStart::ProfilePlane);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &taper)],
                &[body_group.clone(), face_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );

        let profile_offset = parameter("ProfileOffset", "mm", 0.1);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &profile_offset)],
                std::slice::from_ref(&body_group),
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        set_extrude_start(&mut scope, DesignExtrudeStart::OffsetProfilePlane);
        let offset_start = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &profile_offset)],
                std::slice::from_ref(&body_group),
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed offset-profile-plane Extrude");
        assert!(matches!(
            offset_start,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                start: ExtrudeStart::OffsetProfilePlane {
                    offset: actual_offset
                },
                ..
            }) if actual_offset.get() == 1.0
        ));
        set_extrude_start(&mut scope, DesignExtrudeStart::ProfilePlane);

        set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
        let against = parameter("AgainstDistance", "mm", -0.05);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &against)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistance);
        let two_sided = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &against), (2, &side_two_taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed two-sided Extrude");
        assert!(matches!(
            two_sided,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::TwoSided {
                    first: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    },
                    second: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length_2
                        },
                        draft: Some(actual_draft),
                        ..
                    },
                },
                ..
            }) if actual_length.get() == 5.5 && actual_length_2.get() == 0.5 && actual_draft.get() == -0.3
        ));
        set_extrude_direction_reversed(&mut scope, true);
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &against), (2, &side_two_taper)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );
        set_extrude_direction_reversed(&mut scope, false);

        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedDistance);
        let reversed_along = parameter("AlongDistance", "mm", -0.6);
        let reversed = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &reversed_along)],
                &[],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed reversed Extrude");
        assert!(matches!(
            reversed,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ReversedProfileNormal {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    },
                },
                ..
            }) if actual_length.get() == 6.0
        ));

        set_extrude_operation(&mut scope, DesignExtrudeOperation::Join);
        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
        set_extrude_direction_reversed(&mut scope, true);
        face_group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                encoding:
                    crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
                usage: DesignExtrudeFaceRole::Termination,
            };
        let side_offset = parameter("Side1Offset", "mm", 0.025);
        let to_face = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &side_offset), (1, &taper)],
                &[body_group.clone(), face_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed reversed to-face Extrude");
        assert!(matches!(
            to_face,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ReversedProfileNormal {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Native(ref id),
                            offset: Some(actual_offset),
                        },
                        ..
                    },
                },
                ..
            }) if (id == &face_group.id) && actual_offset.get() == 0.25
        ));

        let mut omitted_zero_offset_scope = scope.clone();
        omitted_zero_offset_scope.class_tag =
            crate::records::references::DesignClassTag::try_from("330".to_owned()).unwrap();
        omitted_zero_offset_scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
        omitted_zero_offset_scope
            .try_edit(|draft| {
                draft.frame_length = 476;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let omitted_zero_offset = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &omitted_zero_offset_scope,
                &[(0, &taper)],
                &[body_group.clone(), face_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("class-330 face-target Extrude admits omitted zero offset");
        assert!(matches!(
            omitted_zero_offset,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Native(ref id),
                            offset: None,
                        },
                        ..
                    },
                },
                ..
            }) if id == &face_group.id
        ));
        omitted_zero_offset_scope.class_tag =
            crate::records::references::DesignClassTag::try_from("331".to_owned()).unwrap();
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &omitted_zero_offset_scope,
                &[(0, &taper)],
                &[body_group.clone(), face_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );

        for length in [477, 481] {
            let mut class_375_scope = scope.clone();
            class_375_scope.class_tag =
                crate::records::references::DesignClassTag::try_from("375".to_owned()).unwrap();
            class_375_scope.paired_class_tag =
                crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
            class_375_scope
                .try_edit(|draft| {
                    draft.frame_length = length;
                    draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            let omitted_zero_offset = crate::test_support::with_decode_context(|decode_ctx| {
                project_extrude(
                    decode_ctx,
                    &class_375_scope,
                    &[(0, &taper)],
                    &[body_group.clone(), face_group.clone()],
                    &[],
                    std::slice::from_ref(&placement),
                    &[],
                )
            })
            .expect("project the extrude feature")
            .expect("class-375 face-target Extrude admits omitted zero offset");
            assert!(matches!(
                omitted_zero_offset,
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    extent: ExtrudeExtent::OneSided {
                        side: ExtrudeSide {
                            termination: LinearTermination::ToFace {
                                face: FaceSelection::Native(ref id),
                                offset: None,
                            },
                            ..
                        },
                    },
                    ..
                }) if id == &face_group.id
            ));
        }
        let mut invalid_375_scope = scope.clone();
        invalid_375_scope.class_tag =
            crate::records::references::DesignClassTag::try_from("375".to_owned()).unwrap();
        invalid_375_scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap();
        invalid_375_scope
            .try_edit(|draft| {
                draft.frame_length = 478;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| project_extrude(
                decode_ctx,
                &invalid_375_scope,
                &[(0, &taper)],
                &[body_group.clone(), face_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[]
            ))
            .expect("project the extrude feature")
            .is_none()
        );

        set_extrude_direction_reversed(&mut scope, false);
        set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedToFaces);
        let mut second_face_group = face_group.clone();
        second_face_group.id = "f3d:Design/BulkStream.dat:operand-group#104".into();
        second_face_group.scope_reference_ordinal = 3;
        let second_side_offset = parameter("Side2Offset", "mm", 0.05);
        let two_sided_to_faces = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[
                    (0, &side_offset),
                    (1, &taper),
                    (2, &second_side_offset),
                    (3, &side_two_taper),
                ],
                &[
                    body_group.clone(),
                    face_group.clone(),
                    second_face_group.clone(),
                ],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed two-sided face-target Extrude");
        assert!(matches!(
            two_sided_to_faces,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ProfileNormal {},
                extent: ExtrudeExtent::TwoSided {
                    first: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Native(ref first_id),
                            offset: Some(actual_offset),
                        },
                        draft: Some(actual_draft),
                        ..
                    },
                    second: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Native(ref second_id),
                            offset: Some(actual_offset_2),
                        },
                        draft: Some(actual_draft_2),
                        ..
                    },
                },
                ..
            }) if (first_id == &face_group.id && second_id == &second_face_group.id) && actual_offset.get() == 0.25 && actual_draft.get() == 0.2 && actual_offset_2.get() == 0.5 && actual_draft_2.get() == -0.3
        ));

        set_extrude_direction_reversed(&mut scope, true);
        let reversed_two_sided_to_faces = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[
                    (0, &side_offset),
                    (1, &taper),
                    (2, &second_side_offset),
                    (3, &side_two_taper),
                ],
                &[
                    body_group.clone(),
                    face_group.clone(),
                    second_face_group.clone(),
                ],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed reversed two-sided face-target Extrude");
        assert!(matches!(
            reversed_two_sided_to_faces,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ReversedProfileNormal {},
                extent: ExtrudeExtent::TwoSided { .. },
                ..
            })
        ));
        set_extrude_direction_reversed(&mut scope, false);

        set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistanceToFace);
        let mixed_two_sided = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[(0, &along), (1, &second_side_offset), (2, &side_two_taper)],
                &[body_group.clone(), face_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed mixed two-sided Extrude");
        assert!(matches!(
            mixed_two_sided,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                direction: ExtrudeDirection::ProfileNormal {},
                extent: ExtrudeExtent::TwoSided {
                    first: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        ..
                    },
                    second: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: FaceSelection::Native(ref id),
                            offset: Some(actual_offset),
                        },
                        draft: Some(actual_draft),
                        ..
                    },
                },
                ..
            }) if (id == &face_group.id) && actual_length.get() == 5.5 && actual_offset.get() == 0.5 && actual_draft.get() == -0.3
        ));

        set_extrude_extent(&mut scope, DesignExtrudeExtent::OneSidedToFace);
        set_extrude_start(&mut scope, DesignExtrudeStart::FromFace);
        let mut start_group = face_group.clone();
        start_group.id = "f3d:Design/BulkStream.dat:operand-group#103".into();
        start_group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeFaces {
                encoding:
                    crate::records::topology::extrude_selection::DesignExtrudeFaceEncoding::Faces,
                usage: DesignExtrudeFaceRole::Start,
            };
        let from_face = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[
                    (0, &parameter("ProfileOffset", "mm", 0.0)),
                    (1, &side_offset),
                    (2, &taper),
                ],
                &[body_group, start_group.clone(), face_group],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed selected-face start Extrude");
        assert!(matches!(
            from_face,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                start: ExtrudeStart::FromFace {
                    face: FaceSelection::Native(ref id),
                    offset: None,
                },
                ..
            }) if id == &start_group.id
        ));

        set_extrude_operation(&mut scope, DesignExtrudeOperation::NewBody);
        set_extrude_extent(&mut scope, DesignExtrudeExtent::TwoSidedDistance);
        set_extrude_direction_reversed(&mut scope, false);
        let from_face_two_sided = crate::test_support::with_decode_context(|decode_ctx| {
            project_extrude(
                decode_ctx,
                &scope,
                &[
                    (0, &parameter("ProfileOffset", "mm", 0.0)),
                    (1, &along),
                    (2, &against),
                ],
                &[start_group.clone()],
                &[],
                std::slice::from_ref(&placement),
                &[],
            )
        })
        .expect("project the extrude feature")
        .expect("typed selected-face-start two-sided Extrude");
        assert!(matches!(
            from_face_two_sided,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                start: ExtrudeStart::FromFace {
                    face: FaceSelection::Native(ref id),
                    offset: None,
                },
                extent: ExtrudeExtent::TwoSided {
                    first: ExtrudeSide {
                        termination: LinearTermination::Blind { length: actual_length },
                        ..
                    },
                    second: ExtrudeSide {
                        termination: LinearTermination::Blind { length: actual_length_2 },
                        ..
                    },
                },
                ..
            }) if (id == &start_group.id) && actual_length.get() == 5.5 && actual_length_2.get() == 0.5
        ));
    });
}

#[test]
fn sketch_inputs_bind_owner_dependencies_after_sketch_conversion() {
    use cadmpeg_ir::features::{BooleanOp, LoftSection, PathRef, SheetMetalThicknessSide};
    use cadmpeg_ir::sketches::SpatialSketchId;

    let feature = |id: &str, ordinal, definition| Feature {
        product_definition: None,
        id: FeatureId::mint(id).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    };
    let planar_sketch = SketchId::mint("synthetic:test:id#f3d:sketch:planar").unwrap();
    let spatial_sketch = SpatialSketchId::mint("synthetic:test:id#f3d:sketch:spatial").unwrap();
    let planar_feature = feature(
        "synthetic:test:id#f3d:feature:planar-sketch",
        0,
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(planar_sketch.clone())),
        }),
    );
    let spatial_feature = feature(
        "synthetic:test:id#f3d:feature:spatial-sketch",
        1,
        FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
            sketch: Some(spatial_sketch.clone()),
        }),
    );
    let base_flange = feature(
        "synthetic:test:id#f3d:feature:base-flange",
        2,
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::PlanarProfileRef::Sketch(planar_sketch.clone()),
            thickness: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
            side: SheetMetalThicknessSide::Forward,
        }),
    );
    let loft = feature(
        "synthetic:test:id#f3d:feature:loft",
        3,
        FeatureDefinition::Operation(FeatureOperation::Loft {
            sections: vec![
                LoftSection::Profile(
                    ProfileRef::spatial_sketch_profiles(spatial_sketch.clone(), vec![2]).unwrap(),
                ),
                LoftSection::Profile(
                    ProfileRef::spatial_sketch_profiles(spatial_sketch.clone(), vec![5]).unwrap(),
                ),
            ],
            guidance: cadmpeg_ir::features::LoftGuidance::Centerline(PathRef::Sketch(
                planar_sketch,
            )),
            op: BooleanOp::Join,
            closed: false,
            solid: true,
            ruled: false,
            linearize: false,
            max_degree: None,
            allow_multi_profile_faces: None,
        }),
    );
    let expected_dependencies = [spatial_feature.id.clone(), planar_feature.id.clone()];
    let mut features = vec![planar_feature, spatial_feature, base_flange, loft];

    {
        crate::design::feature_project::bind_sketch_feature_geometry(
            &mut features,
            &[],
            &[],
            &[],
            &[],
        )
    }
    .unwrap();

    assert_eq!(
        features[2].dependencies.as_slice(),
        [features[0].id.clone()]
    );
    assert_eq!(features[3].dependencies.as_slice(), expected_dependencies);
}
