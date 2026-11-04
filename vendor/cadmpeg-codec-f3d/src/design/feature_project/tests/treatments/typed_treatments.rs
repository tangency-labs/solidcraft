// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::test_support::parameter_owner_frame;
use crate::design::test_support::parameter_record;
use crate::records::feature::hole::DesignHoleConstruction;
use crate::records::feature::hole::DesignHoleTangentPoint;
use crate::records::feature::scope::DesignFeatureKind;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignParameterScopeDraft;
use crate::records::feature::scope::DesignScopePayloadMut;
use crate::records::identity::Located;
use crate::records::identity::ReferenceRun;
use crate::records::parameters::DesignParameterOwner;
use crate::records::parameters::DesignParameterOwnerWire;
use crate::records::parameters::DesignParameterSource;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupDraft;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft;
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use crate::records::topology::face::DesignFaceOperandDraft;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::ids::FaceId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

#[test]
fn edge_treatments_and_holes_project_typed_dimensions_and_native_selections() {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, ChamferSpec, RadiusSpec},
        EdgeSelection,
    };

    let parameter = |owner_record_index,
                     record_index,
                     source_kind: &str,
                     name: &str,
                     expression: &str,
                     value| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(owner_record_index),
            expression,
            source_kind,
            Some("mm"),
            name,
            value,
        ))
        .expect("generated feature parameter is canonical");
        parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, scope_record_index, parameter_record_index, local_ordinal| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical")
            .into_record("Design/BulkStream.dat", 0)
            .unwrap();
        {
            let mut wire = DesignParameterOwnerWire::from(owner.clone());
            wire.id = format!("f3d:native/BulkStream.dat:owner#{record_index}");
            wire.record_index = record_index;
            wire.scope_record_index = scope_record_index;
            wire.parameter_record_index = parameter_record_index;
            wire.companion_record_index = parameter_record_index + 1;
            wire.local_ordinal = local_ordinal;
            owner = DesignParameterOwner::try_from(wire).unwrap();
        }
        owner
    };
    let scope = |record_index, byte_offset, kind: &str| {
        DesignParameterScope::try_new(
            DesignParameterScopeDraft {
                id: format!("f3d:native/BulkStream.dat:scope#{record_index}"),
                byte_offset,
                class_tag: DesignClassTag::try_from("301".to_owned()).unwrap(),
                record_index,
                frame_length: 200,
                kind_offset: byte_offset + 100,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: None,

                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: byte_offset + 80,
                reference_members: ReferenceRun::from_columns(
                    vec![record_index + 1],
                    vec![byte_offset + 85],
                    "reference_members",
                )
                .unwrap(),
                payload: DesignFeatureKind::try_from(kind.to_owned())
                    .expect("nonempty family name")
                    .try_into()
                    .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: DesignClassTag::try_from("261".to_owned()).unwrap(),
                paired_byte_offset: byte_offset + 200,
            }
            .with_fixture_layout(),
        )
        .unwrap()
    };
    let mut scopes = vec![
        scope(12, 100, "Fillet"),
        scope(22, 400, "Chamfer"),
        scope(32, 700, "Hole"),
    ];
    if let DesignScopePayloadMut::Hole(slot) = scopes[2].payload_mut() {
        slot.extent = Some(crate::records::feature::hole::DesignHoleExtent {
            leading_word: None,
            extent_control: Some(0),
            saved_non_distance_depth: None,
            direction_control: Some(Located {
                value: 0,
                offset: 9,
            }),
        });
        slot.point = Some(DesignHoleConstruction {
            point_record_index: 378,
            point_record_byte_offset: 10,
            position: crate::test_support::reals([1.25, -2.5, 3.75]),
            position_offset: 35,
            direction: crate::test_support::reals([0.0, 0.0, 1.0]),
            direction_offset: 59,
            point_parameters: crate::test_support::reals([0.125, -0.25]),
            point_parameter_offsets: [83, 91],
            reference_type: 19,
            reference_type_offset: 99,
            tangent_point_data: Some(DesignHoleTangentPoint {
                prefix: 0,
                data: Located {
                    value: crate::test_support::reals([-1.0, -1.0, -1.0]),
                    offset: 104,
                },
            }),
            input_records: vec![Located {
                value: 378,
                offset: 129,
            }],
            face_selection: None,
            locations: vec![crate::records::feature::hole::DesignHoleLocation {
                record_index: 375,
                position: crate::test_support::reals([1.25, -2.5, 3.75]),
                position_offset: 200,
                reference_type: 17,
                vertex_input: None,
            }],
        });
    }
    scopes[2]
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![0, 363, 0, 370, 0, 378]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let hole_face_operand = |record_index, scope_reference_ordinal| {
        DesignFaceOperand::try_new(DesignFaceOperandDraft {
            id: format!("f3d:native/BulkStream.dat:face-operand#{record_index}"),
            scope_record_index: 32,
            scope_reference_ordinal,
            group: None,
            record_index,
            byte_offset: 1200,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            paired_byte_offset: 1250,
            paired_class_tag: DesignClassTag::try_from("259".to_owned()).unwrap(),
            recipe_record_index: record_index + 3,
            recipe_record_byte_offset: 1300,
            recipe_id: format!(
                "f3d:native/BulkStream.dat:construction-recipe#{}",
                record_index + 3
            ),
            recipe_prefix_offset: 1311,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::BoundedFace,
            recipe_program_offset: 1350,
            recipe_program: vec![0, -1],

            recipe_nodes: Vec::new(),
            candidate_faces: Vec::new(),
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: vec![282],
            resolved_active_face: None,
            next_record_index: record_index + 4,
            next_byte_offset: 1411,
        })
        .unwrap()
    };
    let hole_face_operands = [hole_face_operand(370, 3), hole_face_operand(378, 5)];
    let construction_group = |record_index, scope_reference_ordinal| {
        DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
            id: format!("f3d:native/BulkStream.dat:construction-group#{record_index}"),
            scope_record_index: 22,
            scope_reference_ordinal,
            record_index,
            byte_offset: 1_000 + u64::from(scope_reference_ordinal),
            class_tag: DesignClassTag::try_from("288".to_owned()).unwrap(),
            members: vec![Located {
                value: record_index + 100,
                offset: 1_026 + u64::from(scope_reference_ordinal),
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021 + u64::from(scope_reference_ordinal),
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![Located {
                        value: record_index + 1,
                        offset: 1_050 + u64::from(scope_reference_ordinal),
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 100,
                    opaque_index_offset: 1_078 + u64::from(scope_reference_ordinal),
                    opaque_scalar: 0.5,
                    opaque_scalar_offset: 1_082 + u64::from(scope_reference_ordinal),
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B),
            role_offset: 1_060 + u64::from(scope_reference_ordinal),
            paired_class_tag: DesignClassTag::try_from("259".to_owned()).unwrap(),
            paired_byte_offset: 1_100 + u64::from(scope_reference_ordinal),
        })
        .unwrap()
    };
    let chamfer_edge_group = construction_group(70, 1);
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &scopes;
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
                        parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
                        parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
                    ],
                    owners: &[
                        owner(44, 12, 45, 0),
                        owner(54, 22, 55, 0),
                        owner(64, 22, 65, 1),
                    ],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });

    let fillet = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Fillet"))
        .expect("typed fillet");
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        fillet.evaluation.definition()
    else {
        panic!("expected typed fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: EdgeSelection::Native(selection),
            radius: RadiusSpec::Constant { radius },
            tangency_weight: None,
        }] if selection == &scopes[0].id && radius.get() == 5.0
    ));
    let chamfer = features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("Chamfer"))
        .expect("typed chamfer");
    assert!(matches!(
        chamfer.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                edges: EdgeSelection::Native(selection),
                spec: ChamferSpec::TwoDistances { first, second },
            }] if selection == &chamfer_edge_group.id && first.get() == 1.0 && second.get() == 2.0)
    ));

    let mut distance_angle_parameters = [
        parameter(54, 55, "Distance", "d2", "1.6 mm", 0.16),
        parameter(
            64,
            65,
            "Rotate Angle",
            "d3",
            "25 deg",
            25.0_f64.to_radians(),
        ),
    ];
    distance_angle_parameters[1]
        .try_set_unit_value("deg".to_owned())
        .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[1]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &distance_angle_parameters,
                    owners: &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.get() == 1.6 && angle.get() == 25.0_f64.to_radians())
    ));

    distance_angle_parameters[0]
        .try_set_source(
            DesignParameterSource::new(
                "leftDistance".into(),
                distance_angle_parameters[0].owner_record_index(),
                distance_angle_parameters[0].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    distance_angle_parameters[1]
        .try_set_source(
            DesignParameterSource::new(
                "rotateAngle".into(),
                distance_angle_parameters[1].owner_record_index(),
                distance_angle_parameters[1].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[1]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &distance_angle_parameters,
                    owners: &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::DistanceAngle { distance, angle },
                ..
            }] if distance.get() == 1.6 && angle.get() == 25.0_f64.to_radians())
    ));

    let mut hole_parameters = [
        parameter(94, 95, "HoleDepth", "d4", "10 mm", 1.0),
        parameter(104, 105, "HoleDiameter", "d5", "4 mm", 0.4),
        parameter(114, 115, "TipAngle", "d6", "180 deg", std::f64::consts::PI),
    ];
    hole_parameters[2]
        .try_set_unit_value("deg".to_owned())
        .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[2]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &hole_parameters,
                    owners: &[
                        owner(94, 32, 95, 0),
                        owner(104, 32, 105, 1),
                        owner(114, 32, 115, 2),
                    ],
                    scopes,
                    face_operands: &hole_face_operands,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            face: Some(FaceSelection::Resolved { faces, native }),
            placements: Some(placements),
            shape,

            extent: Some(cadmpeg_ir::features::LinearTermination::Blind { length: actual_length }),
            bottom: Some(cadmpeg_ir::features::holes::HoleBottom::Flat),
            ..
        }) if matches!((shape.construction(), &shape.diameter(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: cadmpeg_ir::features::holes::HoleKind::Simple,
                ..
            }, Some(actual_diameter),) if (faces == &vec![FaceId::mint(crate::ids::brep_entity_id(282)).expect("identity grammar")]
            && native == &scopes[2].id
            && placements == &vec![cadmpeg_ir::features::holes::HolePlacement::Directed {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3 { x: 12.5, y: -25.0, z: 37.5 }).unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3 { x: -0.0, y: -0.0, z: -1.0 }).unwrap(),
            }]) && actual_diameter.get() == 4.0 && actual_length.get() == 10.0)));

    hole_parameters[2]
        .try_set_evaluated_value(118.0_f64.to_radians())
        .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[2]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &hole_parameters,
                    owners: &[
                        owner(94, 32, 95, 0),
                        owner(104, 32, 105, 1),
                        owner(114, 32, 115, 2),
                    ],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            bottom: Some(cadmpeg_ir::features::holes::HoleBottom::Angled {depth_to_tip: false, ..}),
            ..
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: cadmpeg_ir::features::holes::HoleKind::SimpleDrilled { drill_point_angle },
                ..
            },) if drill_point_angle.get() == 118.0_f64.to_radians())));

    let mut counterbore_parameters = hole_parameters.to_vec();
    counterbore_parameters.extend([
        parameter(124, 125, "CBDepth", "d7", "3 mm", 0.3),
        parameter(134, 135, "CBDiameter", "d8", "8 mm", 0.8),
    ]);
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[2]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &counterbore_parameters,
                    owners: &[
                        owner(94, 32, 95, 0),
                        owner(104, 32, 105, 1),
                        owner(114, 32, 115, 2),
                        owner(124, 32, 125, 3),
                        owner(134, 32, 135, 4),
                    ],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            bottom: Some(cadmpeg_ir::features::holes::HoleBottom::Angled {depth_to_tip: false, ..}),
            ..
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: cadmpeg_ir::features::holes::HoleKind::CounterboreDrilled {
                    diameter: actual_diameter,
                    depth: actual_depth,
                    drill_point_angle,
                },
                ..
            },) if (drill_point_angle.get() == 118.0_f64.to_radians()) && actual_diameter.get() == 8.0 && actual_depth.get() == 3.0)));

    counterbore_parameters[2]
        .try_set_evaluated_value(std::f64::consts::PI)
        .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[2]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &counterbore_parameters,
                    owners: &[
                        owner(94, 32, 95, 0),
                        owner(104, 32, 105, 1),
                        owner(114, 32, 115, 2),
                        owner(124, 32, 125, 3),
                        owner(134, 32, 135, 4),
                    ],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            bottom: Some(cadmpeg_ir::features::holes::HoleBottom::Flat),
            ..
        }) if matches!((shape.construction(),), (cadmpeg_ir::features::holes::HoleConstruction::Form {
                kind: cadmpeg_ir::features::holes::HoleKind::Counterbore {
                    diameter: actual_diameter,
                    depth: actual_depth,
                },
                ..
            },) if actual_diameter.get() == 8.0 && actual_depth.get() == 3.0)));

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[1]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter(54, 55, "leftDistance", "d2", "1 mm", 0.1),
                        parameter(64, 65, "rightDistance", "d3", "2 mm", 0.2),
                    ],
                    owners: &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::TwoDistances { first, second },
                ..
            }] if first.get() == 1.0 && second.get() == 2.0)
    ));

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[1]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[parameter(54, 55, "leftDistance", "d2", "1 mm", 0.1)],
                    owners: &[owner(54, 22, 55, 0)],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [ChamferGroup {
                spec: ChamferSpec::Distance { distance },
                ..
            }] if distance.get() == 1.0)
    ));

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[0]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter(44, 45, "Radius", "d1", "5 mm", 0.5),
                        parameter(46, 47, "TangencyWeight", "w1", "0.5", 0.5),
                    ],
                    owners: &[owner(44, 12, 45, 0), owner(46, 12, 47, 1)],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Fillet,
            parameters,
        }) if parameters.len() == 2
    ));

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[0]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[parameter(44, 45, "Radius", "d1", "0 mm", 0.0)],
                    owners: &[owner(44, 12, 45, 0)],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Fillet,
            parameters,
        }) if parameters.len() == 1
    ));

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[1]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter(54, 55, "Distance 1", "d2", "1 mm", 0.1),
                        parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
                        parameter(74, 75, "Distance", "d4", "3 mm", 0.3),
                    ],
                    owners: &[
                        owner(54, 22, 55, 0),
                        owner(64, 22, 65, 1),
                        owner(74, 22, 75, 2),
                    ],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        }) if parameters.len() == 3
    ));

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scopes[1]);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter(54, 55, "Distance 1", "d2", "0 mm", 0.0),
                        parameter(64, 65, "Distance 2", "d3", "2 mm", 0.2),
                    ],
                    owners: &[owner(54, 22, 55, 0), owner(64, 22, 65, 1)],
                    scopes,
                    construction_groups: std::slice::from_ref(&chamfer_edge_group),
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Chamfer,
            parameters,
        }) if parameters.len() == 2
    ));

    let mut construction_groups = [construction_group(90, 17), construction_group(80, 4)];
    construction_groups[1]
        .lost_edge_references
        .push("f3d:native/BulkStream.dat:lost-edge-reference#1".into());
    let mut chamfer_scope = scopes[1].clone();
    chamfer_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(21);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&chamfer_scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        parameter(74, 75, "Distance", "d5", "2 mm", 0.2),
                        parameter(84, 85, "Distance", "d4", "2.5 mm", 0.25),
                    ],
                    owners: &[owner(74, 22, 75, 1), owner(84, 22, 85, 0)],
                    scopes,
                    construction_groups: &construction_groups,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Chamfer { groups, .. })
            if matches!(groups.as_slice(), [
                ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Distance { distance: actual_distance },
                },
                ChamferGroup {
                    edges: EdgeSelection::Native(selection),
                    spec: ChamferSpec::Distance { distance: actual_distance_2 },
                },
            ] if (selection == &construction_groups[0].id) && actual_distance.get() == 2.5 && actual_distance_2.get() == 2.0)
    ));
}
