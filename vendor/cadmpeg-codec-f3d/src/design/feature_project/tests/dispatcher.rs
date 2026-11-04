// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::project_parameter_design_with_edge_identities;
use crate::ids::neutral_sketch_id;
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::body_ops::DesignScaleOperation;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::surface_ops::DesignSurfaceStitchOperation;
use crate::records::feature::thread::DesignThreadConstruction;
use crate::records::feature::thread::DesignThreadForm;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::features::FaceSelection;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

/// Tolerance for the millimetre pitch of a projected Thread.
const EPS_THREAD_PITCH: f64 = 1.0e-9;

#[test]
fn dispatcher_projects_datum_feature_scopes() {
    let mut transform = crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows();
    transform[0][3] = 1.0;
    transform[1][3] = 2.0;
    transform[2][3] = 3.0;

    let mut joint_origin = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#1",
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        1,
    );
    joint_origin.with_joint_origin_transform(transform.try_into().unwrap());

    let mut work_plane = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#2",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        2,
    );
    work_plane.with_work_plane_transform(transform.try_into().unwrap());

    let mut work_point = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#3",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        3,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkPoint(slot) =
        work_point.payload_mut()
    {
        *slot = Some(
            crate::records::feature::work_geometry::DesignWorkPointConstruction {
                point_record_index: 4,
                point_record_byte_offset: 0,
                position: crate::test_support::reals([4.0, 5.0, 6.0]),
                position_offset: 0,
                rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                    crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native {
                        reference_type: 1,
                        inputs: Vec::new(),
                    },
                )
                .expect("compatible WorkPoint rule"),
                reference_type_offset: 0,
            },
        );
    }

    let scopes = vec![joint_origin, work_plane, work_point];
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &scopes;
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });

    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { frame })
            if frame.origin() == Point3::new(10.0, 20.0, 30.0)
    ));
    assert!(matches!(
        features[1].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) if frame.origin() == Point3::new(10.0, 20.0, 30.0)
            && frame.normal() == Vector3::new(0.0, 0.0, 1.0)
            && frame.u_axis() == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert!(matches!(
        features[2].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint { position, construction })
            if *position == Point3::new(40.0, 50.0, 60.0) && construction.is_none()
    ));
}

#[test]
fn revolve_and_thread_scopes_without_a_construction_keep_their_family() {
    use cadmpeg_ir::features::UnresolvedFamily;

    let revolve = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#1",
        crate::records::feature::scope::DesignFeatureKind::Revolve,
        1,
    );
    let thread = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#2",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        2,
    );
    let scopes = vec![revolve, thread];
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &scopes;
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });

    let definition = |kind: &str| {
        features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(kind))
            .map(|feature| feature.evaluation.definition().clone())
    };
    assert_eq!(
        definition("Revolve"),
        Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Revolve,
        }))
    );
    assert_eq!(
        definition("Thread"),
        Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Thread,
        }))
    );
}

#[test]
fn dispatcher_projects_scale_point_center_in_neutral_units() {
    let mut scale = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#4",
        crate::records::feature::scope::DesignFeatureKind::Scale,
        4,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Scale(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Massstab(slot) = scale.payload_mut()
    {
        *slot = Some(DesignScaleOperation {
            body_group_record_index: 5,
            center_record_index: 6,
            center_position: Some(crate::records::identity::Located {
                value: crate::test_support::reals([1.25, -2.5, 3.75]),
                offset: 40,
            }),
            uniform_factor: cadmpeg_ir::scalar::PositiveReal::new(2.5)
                .expect("checked fixture value"),
            uniform_factor_offset: 20,
        });
    }

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[scale];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let FeatureDefinition::Operation(FeatureOperation::Scale {
        bodies,
        center: Some(cadmpeg_ir::features::ScaleCenter::Point(center)),
        factors,
    }) = features[0].evaluation.definition()
    else {
        panic!("scale feature with explicit center");
    };
    assert!(matches!(
        bodies,
        cadmpeg_ir::features::BodySelection::Unresolved
    ));
    for (actual, expected) in [center.x, center.y, center.z]
        .into_iter()
        .zip([12.5, -25.0, 37.5])
    {
        assert!((actual - expected).abs() < f64::EPSILON);
    }
    assert!(matches!(
        factors,
        cadmpeg_ir::features::ScaleFactors::Uniform { factor: uniform }
            if (uniform.get() - 2.5).abs() < f64::EPSILON
    ));
}

#[test]
fn dispatcher_projects_referenced_work_plane_frame() {
    let mut referenced = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        10,
    );
    referenced.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    referenced.with_work_plane_reference(11);

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[referenced];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) if frame.origin() == Point3::new(0.0, 0.0, 0.0)
            && frame.normal() == Vector3::new(0.0, 0.0, 1.0)
            && frame.u_axis() == Vector3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn dispatcher_projects_plano_de_obra_frame() {
    let mut referenced = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::PlanoDeObra,
        10,
    );
    referenced.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    referenced.with_work_plane_reference(11);

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[referenced];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { frame }) if frame.origin() == Point3::new(0.0, 0.0, 0.0)
            && frame.normal() == Vector3::new(0.0, 0.0, 1.0)
            && frame.u_axis() == Vector3::new(1.0, 0.0, 0.0)
    ));
}

#[test]
fn dispatcher_projects_three_point_work_plane_vertices() {
    use crate::records::feature::work_geometry::{DesignVertexRecipe, DesignWorkPlaneConstruction};
    use cadmpeg_ir::features::VertexSelection;

    let recipe = |record_index, vertex| {
        DesignVertexRecipe::try_new(
            crate::records::feature::work_geometry::DesignVertexRecipeDraft {
                record_index,
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                    .unwrap(),
                paired_byte_offset: u64::from(record_index) + 16,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                recipe_record_index: record_index + 3,
                recipe_record_byte_offset: u64::from(record_index) + 32,
                recipe_id: format!("f3d:native/BulkStream.dat:construction-recipe#{record_index}"),
                recipe_prefix_offset: u64::from(record_index) + 43,
                recipe_prefix_bytes: Vec::new(),
                recipe_references: Vec::new(),
                recipe_program_offset: 4,
                recipe_program: vec![0],
                resolution: Some(
                    crate::records::feature::work_geometry::DesignVertexResolution::new(4, vertex)
                        .expect("valid vertex slot"),
                ),
                next_record_index: record_index + 5,
                next_byte_offset: u64::from(record_index) + 200,
            },
        )
        .unwrap()
    };
    let mut plane = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#20",
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        20,
    );
    plane.with_work_plane_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    if let Some(frame) = plane.work_plane_frame_mut() {
        frame.work_plane_construction = Some(
            DesignWorkPlaneConstruction::try_new(
                21,
                Box::new([recipe(22, 43), recipe(27, 64), recipe(32, 84)]),
            )
            .unwrap(),
        );
    }

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[plane];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let FeatureDefinition::Operation(FeatureOperation::DatumThreePointPlane { points, .. }) =
        features[0].evaluation.definition()
    else {
        panic!("three-point datum plane")
    };
    assert!(matches!(
        points.as_ref(),
        [
            VertexSelection::Historical { vertex: first, .. },
            VertexSelection::Historical { vertex: second, .. },
            VertexSelection::Historical { vertex: third, .. },
        ] if first.as_str().ends_with(":43") && second.as_str().ends_with(":64") && third.as_str().ends_with(":84")
    ));
}

#[test]
fn dispatcher_projects_work_point_plane_construction_and_dependencies() {
    use crate::records::feature::work_geometry::{
        DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
        DesignWorkPointPlaneSelection,
    };
    use cadmpeg_ir::features::{DatumPlaneReference, DatumPointConstruction};

    let planes = [10, 20, 30].map(|record_index| {
        let id = format!("f3d:native/BulkStream.dat:parameter-scope#{record_index}");
        let mut scope = DesignParameterScope::empty(
            &id,
            crate::records::feature::scope::DesignFeatureKind::WorkPlane,
            record_index,
        );
        scope.with_work_plane_transform(
            crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
        );
        scope
    });
    let input = |record_index, work_plane_scope_record_index| {
        DesignWorkPointInput::try_new(
            record_index,
            u64::from(record_index),
            Some(Box::new(DesignWorkPointInputCarrier::WorkPlane {
                selection: DesignWorkPointPlaneSelection::try_new(
                    record_index,
                    crate::records::feature::work_geometry::DesignWorkPointPlaneSelectionDraft {
                        class_tag: crate::records::references::DesignClassTag::try_from(
                            "267".to_owned(),
                        )
                        .unwrap(),
                        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "00000000-0000-0000-0000-000000000001".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 1,
                        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "00000000-0000-0000-0000-000000000002".to_owned(),
                        )
                        .unwrap(),
                        context_id_offset: 2,
                        identity_record_index: record_index + 3,
                        identity_record_offset: 3,
                        primary_identity: u64::from(work_plane_scope_record_index - 1),
                        primary_identity_offset: 24,
                        work_plane_scope_record_index,
                        next_record_index: record_index + 4,
                        next_byte_offset: 32,
                    },
                )
                .unwrap(),
            })),
        )
        .unwrap()
    };
    let mut point = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#40",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        40,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkPoint(slot) =
        point.payload_mut()
    {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 41,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([1.0, 2.0, 3.0]),
            position_offset: 0,
            rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                crate::records::feature::work_geometry::DesignWorkPointRuleForm::ThreePlaneIntersection {
                    inputs: [input(42, 10), input(46, 20), input(50, 30)],
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }
    let mut scopes = planes.to_vec();
    scopes.push(point);

    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &scopes;
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let point = features
        .iter()
        .find(|feature| {
            feature.native_ref.as_deref() == Some("f3d:native/BulkStream.dat:parameter-scope#40")
        })
        .expect("projected work point");
    let FeatureDefinition::Operation(FeatureOperation::DatumPoint {
        construction: Some(construction),
        ..
    }) = point.evaluation.definition()
    else {
        panic!("typed datum-point construction");
    };
    let DatumPointConstruction::ThreePlaneIntersection { planes } = construction.as_ref() else {
        panic!("three-plane construction");
    };
    let plane_features = planes
        .iter()
        .map(|plane| match plane {
            DatumPlaneReference::Feature { feature } => feature.clone(),
            DatumPlaneReference::Face { .. } | DatumPlaneReference::ResolvedPlane { .. } => {
                panic!("feature-backed plane")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(point.dependencies.as_slice(), plane_features);
}

#[test]
fn dispatcher_projects_work_point_historical_vertex_and_dependency() {
    use crate::records::feature::work_geometry::{
        DesignVertexRecipe, DesignWorkPointConstruction, DesignWorkPointInput,
        DesignWorkPointInputCarrier,
    };
    use cadmpeg_ir::features::{DatumPointConstruction, VertexSelection};

    let mut predecessor = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(4);
        })
        .unwrap();
    let recipe_id = "f3d:native/BulkStream.dat:construction-recipe#vertex".to_string();
    let recipe = DesignVertexRecipe::try_new(
        crate::records::feature::work_geometry::DesignVertexRecipeDraft {
            record_index: 22,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("369".to_owned())
                .unwrap(),
            paired_byte_offset: 16,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 25,
            recipe_record_byte_offset: 32,
            recipe_id: recipe_id.clone(),
            recipe_prefix_offset: 43,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 4,
            recipe_program: vec![0],
            resolution: Some(
                crate::records::feature::work_geometry::DesignVertexResolution::new(4, 43)
                    .expect("valid vertex slot"),
            ),
            next_record_index: 27,
            next_byte_offset: 200,
        },
    )
    .unwrap();
    let mut point = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:parameter-scope#20",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        20,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkPoint(slot) =
        point.payload_mut()
    {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 21,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([4.0, 3.0, 0.0]),
            position_offset: 0,
            rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex {
                    input: DesignWorkPointInput::try_new(
                        22,
                        0,
                        Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
                            recipe,
                        })),
                    )
                    .unwrap(),
                },
            )
            .expect("compatible WorkPoint rule"),
            reference_type_offset: 0,
        });
    }
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:native/BulkStream.dat", 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 20,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let scopes = vec![predecessor, point];
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|decode_ctx| {
            project_parameter_design_with_edge_identities(
                decode_ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes: &scopes,
                    timelines: std::slice::from_ref(&timeline),
                    ..Default::default()
                },
            )
        })
        .expect("authored WorkPoint timeline");
    let predecessor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&scopes[0].id))
        .expect("projected predecessor");
    let point = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&scopes[1].id))
        .expect("projected WorkPoint");
    let FeatureDefinition::Operation(FeatureOperation::DatumPoint {
        construction: Some(construction),
        ..
    }) = point.evaluation.definition()
    else {
        panic!("typed datum-point construction")
    };
    let DatumPointConstruction::Vertex {
        vertex:
            VertexSelection::Historical {
                state,
                vertex,
                native,
            },
    } = construction.as_ref()
    else {
        panic!("historical vertex construction")
    };
    let feature_key = point.id.key();
    let prefix = crate::ids::history_input_prefix(&feature_key, 4);
    assert_eq!(state, &crate::ids::feature_input_topology_id(&point.id, 4));
    assert_eq!(vertex, &crate::ids::history_input_vertex_id(&prefix, 43));
    assert_eq!(native.as_str(), &recipe_id);
    assert_eq!(point.dependencies.as_slice(), [predecessor.id.clone()]);
}

#[test]
fn dispatcher_projects_remaining_operand_feature_scopes() {
    use crate::records::{
        feature::{
            assembly_features::DesignCopyPasteComponentOperation,
            base_feature::DesignBaseFeatureConstruction, body_ops::DesignCopyPasteBodiesOperation,
            sheet_metal::DesignBaseFlangeOperation,
        },
        topology::construction::DesignConstructionOperandGroupFrame,
    };
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, SheetMetalThicknessSide};

    let stream = "f3d:native/BulkStream.dat";
    let group = |scope_record_index: u32,
                 scope_reference_ordinal: u32,
                 record_index: u32,
                 members: &[u32],
                 role: DesignOperandRole| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: format!("{stream}:construction-group#{record_index}"),
                scope_record_index,
                scope_reference_ordinal,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
                    .unwrap(),
                members: members
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(index, value)| crate::records::identity::Located {
                        value,
                        offset: u64_from_index(index) * 11,
                    })
                    .collect(),
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
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
                role_offset: 0,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "264".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 0,
            },
        )
        .unwrap()
    };

    let mut base_flange = DesignParameterScope::empty(
        &format!("{stream}:scope#base-flange"),
        crate::records::feature::scope::DesignFeatureKind::BaseFlange,
        10,
    );
    {
        let value = Some(DesignBaseFlangeOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(0.2).unwrap(),
            thickness_offset: 0,
            profile_group_record_index: 100,
            profile_record_index: 101,
            thickness_record_index: 102,
            settings_record_index: 103,
        });
        if let crate::records::feature::scope::DesignScopePayloadMut::BaseFlange(slot) =
            base_flange.payload_mut()
        {
            slot.get_or_insert_with(Default::default)
                .base_flange_operation = value;
        }
    }
    {
        let value = Some(
            DesignSketchProfileOperand::try_new(
                crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                    scope_reference_ordinal: 1,
                    record_index: 101,
                    byte_offset: 0,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "377".to_owned(),
                    )
                    .unwrap(),
                    asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                    )
                    .unwrap(),
                    asset_id_offset: 32,
                    entity_id: crate::records::identity::DesignEntityId::try_from(
                        "Sketch_7".to_owned(),
                    )
                    .expect("valid entity identity"),
                    entity_reference_offset: 80,
                    region_selection: None,
                    paired_class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    paired_byte_offset: 160,
                },
            )
            .unwrap(),
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::BaseFlange(slot) =
            base_flange.payload_mut()
        {
            slot.get_or_insert_with(Default::default)
                .base_flange_profile = value;
        }
    }

    let mut remove_body = DesignParameterScope::empty(
        &format!("{stream}:scope#remove-body"),
        crate::records::feature::scope::DesignFeatureKind::RemoveBody,
        20,
    );
    remove_body
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![200]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let mut surface_stitch = DesignParameterScope::empty(
        &format!("{stream}:scope#surface-stitch"),
        crate::records::feature::scope::DesignScopePayload::SurfaceStitch(
            DesignSurfaceStitchOperation {
                gap_tolerance: cadmpeg_ir::scalar::PositiveReal::new(0.01).unwrap(),
                gap_tolerance_offset: 0,
                tolerance_record_index: 302,
                settings_record_index: 303,
            },
        ),
        30,
    );
    surface_stitch
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![300, 301, 302, 303]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let mut copy_paste = DesignParameterScope::empty(
        &format!("{stream}:scope#copy-paste"),
        crate::records::feature::scope::DesignFeatureKind::CopyPaste,
        40,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::CopyPaste(slot) =
        copy_paste.payload_mut()
    {
        *slot = Some(DesignCopyPasteComponentOperation {
            relation_record_index: 401,
            source_occurrence_record_index: 402,
            copied_occurrence_record_index: 403,
            component_guid: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .expect("GUID"),
            source_occurrence_guid: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .expect("GUID"),
            copied_occurrence_guid: "33333333-3333-4333-8333-333333333333"
                .to_owned()
                .try_into()
                .expect("GUID"),
            source_transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            source_transform_offset: 0,
            copied_transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            copied_transform_offset: 0,
        });
    }

    let mut copy_paste_bodies = DesignParameterScope::empty(
        &format!("{stream}:scope#copy-paste-bodies"),
        crate::records::feature::scope::DesignFeatureKind::CopyPasteBodies,
        50,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::CopyPasteBodies(slot) =
        copy_paste_bodies.payload_mut()
    {
        *slot = Some(
            DesignCopyPasteBodiesOperation::try_new(
                vec![crate::records::feature::body_ops::DesignCopiedBody {
                    operand: crate::records::identity::Located {
                        value: 502,
                        offset: 26,
                    },
                    source: crate::records::identity::Located {
                        value: 11,
                        offset: 25,
                    },
                    copied: crate::records::identity::Located {
                        value: 12,
                        offset: 40,
                    },
                }],
                crate::records::feature::body_ops::CopyPasteRecordLocation {
                    record_index: 501,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                },
                crate::records::feature::body_ops::CopyPasteRecordLocation {
                    record_index: 503,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "264".to_owned(),
                    )
                    .unwrap(),
                    byte_offset: 0,
                },
            )
            .unwrap(),
        );
    }

    let mut base_feature = DesignParameterScope::empty(
        &format!("{stream}:scope#base-feature"),
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        60,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::BaseFeature(slot) =
        base_feature.payload_mut()
    {
        *slot = Some(DesignBaseFeatureConstruction::ResultBodies {
            bodies: crate::records::feature::base_feature::DesignBaseFeatureResults::WithoutRepeatedFields(vec![
                crate::records::feature::base_feature::DesignBaseFeatureResultBody {
                    entity: crate::records::feature::base_feature::DesignBaseFeatureEntry {
                        value: 21,
                        offset: 0,
                        field: [0; 6],
                    },
                    reference: crate::records::feature::base_feature::DesignBaseFeatureEntry {
                        value: 601,
                        offset: 0,
                        field: [0; 6],
                    },
                    result: crate::records::feature::base_feature::DesignBaseFeatureEntry {
                        value: 603,
                        offset: 0,
                        field: [0; 6],
                    },
                },
            ]),
            metadata_record: 602,
            metadata_record_offset: 0,
            metadata_field: vec![0, 0].into(),
        });
    }

    let mut thread = DesignParameterScope::empty(
        &format!("{stream}:scope#thread"),
        crate::records::feature::scope::DesignFeatureKind::Thread,
        70,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Thread(slot) =
        thread.payload_mut()
    {
        *slot = Some(DesignThreadConstruction {
            form: DesignThreadForm::Compact(None),
            designation_offset: 0,
            designation: cadmpeg_core::text::NonBlankString::new("M3.5x0.6").unwrap(),
            nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
                "3.5".to_owned(),
            )
            .expect("nominal size"),
            profile: cadmpeg_core::text::NonBlankString::new("GB Metric profile").unwrap(),
            pitch: cadmpeg_ir::scalar::PositiveReal::new(0.06).unwrap(),
            face_group_record_indices: vec![701],
            diameters: crate::records::feature::thread::DesignThreadDiameters::new(
                0.35995, 0.293, 0.3166,
            )
            .unwrap(),
            extended: Some(crate::records::feature::thread::DesignThreadExtended {
                class: "6g".into(),
                external: true,
                modeled: true,
                full_length: Some(true),
            }),
        });
    }
    thread
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![701, 702]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let scopes = vec![
        base_flange,
        remove_body,
        surface_stitch,
        copy_paste,
        copy_paste_bodies,
        base_feature,
        thread,
    ];
    let groups = vec![
        group(10, 0, 100, &[101], DesignOperandRole::PROFILE),
        group(20, 0, 200, &[201], DesignOperandRole::BODIES_A),
        group(30, 0, 300, &[301], DesignOperandRole::ROLE_0X5),
        group(70, 0, 701, &[702], DesignOperandRole::ROLE_0X10),
    ];
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: format!("{stream}:placement#7"),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_7".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned()).unwrap(),
        record_index: 700,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
            .unwrap(),
    };
    let project =
        |scopes: &[DesignParameterScope],
         settings: &[crate::records::sheet_metal_settings::DesignSheetMetalSettings]| {
            crate::test_support::with_decode_context(|ctx| {
                let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
                crate::design::feature_project::project_parameter_design_with_edge_identities(
                    ctx,
                    &crate::design::feature_project::ProjectInputs {
                        scopes,
                        construction_groups: &groups,
                        placements: std::slice::from_ref(&placement),
                        timelines: &timelines,
                        sheet_metal_settings: settings,
                        ..Default::default()
                    },
                )
                .expect("test projection has a synthetic exact timeline")
            })
        };
    let unruled = project(&scopes, &[]);
    // Without its settings' rule the flange uses its stored copy, and says so.
    assert_eq!(unruled.unruled_flange_thicknesses, 1);
    assert_eq!(
        unruled
            .unruled_flange_thickness_loss()
            .map(|loss| loss.code),
        Some(crate::loss::F3dLossCode::SheetThicknessUnruled.kind())
    );
    let features = unruled.features;
    // The rule's thickness governs over the stored copy.
    let ruled: crate::records::sheet_metal_settings::DesignSheetMetalSettings =
        serde_json::from_value(serde_json::json!({
            "id": format!("{stream}:design-sheet-metal-settings#20"),
            "record_index": 103, "byte_offset": 20, "paired_byte_offset": 100,
            "mask": 240, "inputs": {"version": "v2", "slots": vec![None::<u64>; 7]},
            "rule_record_index": 51, "presentation_record_index": 52,
            "owner_record_index": 53, "context_record_index": 54,
            "rule_thickness": 0.25, "rule_thickness_offset": 900,
        }))
        .unwrap();
    let ruled_projection = project(&scopes, std::slice::from_ref(&ruled));
    assert_eq!(ruled_projection.unruled_flange_thicknesses, 0);
    assert!(ruled_projection.unruled_flange_thickness_loss().is_none());
    assert!(ruled_projection.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange { thickness, .. })
            if thickness.get() == 2.5
    )));
    let definition = |kind: &str| {
        features
            .iter()
            .find(|feature| feature.source_tag.as_deref() == Some(kind))
            .map_or_else(
                || panic!("missing dispatched {kind} feature"),
                |feature| feature.evaluation.definition().clone(),
            )
    };

    assert_eq!(
        definition("BaseFlange"),
        FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange {
            profile: cadmpeg_ir::features::PlanarProfileRef::Sketch(neutral_sketch_id(&placement)),
            thickness: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
            side: SheetMetalThicknessSide::Forward,
        })
    );
    assert_eq!(
        definition("RemoveBody"),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(groups[1].id.clone()),
            mode: BodyRetentionMode::DeleteSelected,
        })
    );
    assert_eq!(
        definition("SurfaceStitch"),
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces: FaceSelection::Native(scopes[2].id.clone()),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::scalar::NonNegativeLength::new(0.1).unwrap()),
        })
    );
    assert_eq!(
        definition("CopyPaste"),
        FeatureDefinition::Operation(FeatureOperation::InsertComponent {
            occurrence: crate::ids::neutral_component_occurrence_id(
                &crate::records::mesh::DesignOccurrenceKey::try_from(
                    "33333333-3333-4333-8333-333333333333".to_owned()
                )
                .unwrap()
            ),
        })
    );
    assert_eq!(
        definition("CopyPasteBodies"),
        FeatureDefinition::Operation(FeatureOperation::InsertBodies {
            bodies: cadmpeg_ir::features::InsertedBodies::Native(scopes[4].id.clone()),
        })
    );
    assert_eq!(
        definition("Base Feature"),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Native(scopes[5].id.clone()),
        })
    );
    let FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
        face,
        standard,
        designation,
        diameter,
        pitch,
        modeled,
        extent,
    }) = definition("Thread")
    else {
        panic!("Thread scope projects to a neutral Thread");
    };
    assert_eq!(face, FaceSelection::Native(groups[3].id.clone()));
    assert_eq!(
        standard
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("GB Metric profile")
    );
    assert_eq!(
        designation
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("M3.5x0.6")
    );
    assert_eq!(diameter, cadmpeg_ir::scalar::PositiveLength::new(3.5));
    let pitch = pitch.expect("pitch").get();
    assert!((pitch - 0.6).abs() < EPS_THREAD_PITCH, "pitch {pitch} mm");
    assert_eq!(modeled, Some(true));
    assert_eq!(
        extent,
        Some(cadmpeg_ir::features::CosmeticThreadExtent::Through {})
    );

    let mut cosmetic_scopes = scopes.clone();
    let crate::records::feature::scope::DesignScopePayloadMut::Thread(Some(cosmetic)) =
        cosmetic_scopes[6].payload_mut()
    else {
        panic!("the Thread scope carries its construction");
    };
    cosmetic.extended.as_mut().expect("extended fields").modeled = false;
    let cosmetic = project(&cosmetic_scopes, &[]);
    assert!(cosmetic.features.iter().any(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            modeled: Some(false),
            ..
        })
    )));
}

#[test]
fn loft_path_preserves_complete_historical_edge_selection() {
    use cadmpeg_ir::features::{EdgeSelection, PathRef};
    use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalEdgeId};

    let state =
        FeatureInputTopologyId::mint("f3d:history-input:state#feature").expect("identity grammar");
    let edge =
        HistoricalEdgeId::mint("f3d:history-input:edge#7:feature:41:17").expect("identity grammar");
    assert_eq!(
        crate::design::feature_project::loft_path_from_edge_selection(
            "group",
            EdgeSelection::historical(state.clone(), vec![edge.clone()], "selection".into(),)
                .unwrap(),
        ),
        PathRef::historical_edges(state.clone(), vec![edge.clone()], "selection".into(),).unwrap()
    );
    assert_eq!(
        crate::design::feature_project::loft_path_from_edge_selection(
            "group",
            EdgeSelection::historical_partial(
                state,
                vec![edge],
                vec!["operand".into()],
                "selection".into(),
            )
            .unwrap(),
        ),
        PathRef::Native("group".into())
    );
}

#[test]
fn form_dispatcher_retains_a_result_owner_without_a_cage_identity_join() {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut bulk = Vec::new();
    let mut cage_list = vec![0; 81];
    cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
    cage_list[4..7].copy_from_slice(b"335");
    cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
    cage_list[25] = 1;
    cage_list[26..34].copy_from_slice(&201u64.to_le_bytes());
    cage_list[58] = 1;
    cage_list[59..67].copy_from_slice(&971u64.to_le_bytes());
    cage_list[70] = 1;
    cage_list[71..79].copy_from_slice(&201u64.to_le_bytes());
    bulk.extend_from_slice(&cage_list);

    let mut paired = vec![0; 15];
    paired[..4].copy_from_slice(&3u32.to_le_bytes());
    paired[4..7].copy_from_slice(b"262");
    paired[7..11].copy_from_slice(&205u32.to_le_bytes());
    bulk.extend_from_slice(&paired);

    let mut object = vec![0; 15];
    object[..4].copy_from_slice(&3u32.to_le_bytes());
    object[4..7].copy_from_slice(b"328");
    object[7..11].copy_from_slice(&971u32.to_le_bytes());
    bulk.extend_from_slice(&object);

    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
    archive.start_file(stream, stored).unwrap();
    archive.write_all(&bulk).unwrap();
    let archive = archive.finish().unwrap().into_inner();

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("f3d:{stream}:scope#201"),
        crate::records::feature::scope::DesignFeatureKind::Form,
        201,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![205]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature_id = crate::ids::neutral_feature_id(&scope);
    let mut features = vec![cadmpeg_ir::features::Feature {
        product_definition: None,
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Form".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native {
                    kind: "Form".into(),
                    parameters: Default::default(),
                },
            ),
        ),
        native_ref: Some(scope.id.clone()),
    }];
    let cages = [cadmpeg_ir::SubdSurface {
        id: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#1").expect("identity grammar"),
        scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
        source_object: None,
        cage: cadmpeg_ir::subd::SubdCage::default(),
    }];

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            crate::design::feature_project::form_cages::bind_form_cages(
                ctx,
                scan,
                std::slice::from_ref(&scope),
                &mut features,
                &cages,
            )
        })
    })
    .expect("retained Form result owner");
    assert_eq!(
        *features[0].evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Native {
                kind: "Form".into(),
                parameters: Default::default(),
            }
        )
    );
}

#[test]
fn merged_direct_edges_keep_source_order_and_native_fallback() {
    use cadmpeg_ir::features::EdgeSelection;
    use cadmpeg_ir::ids::EdgeId;

    let scope = DesignParameterScope::empty(
        "f3d:test:scope#100",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        100,
    );
    let first = EdgeId::mint("f3d:brep:entity#1").expect("identity grammar");
    let second = EdgeId::mint("f3d:brep:entity#2").expect("identity grammar");
    let selections = [
        EdgeSelection::Edges(vec![first.clone()]),
        EdgeSelection::Edges(vec![second.clone()]),
    ];
    assert_eq!(
        crate::design::feature_project::merge_edge_selections(&scope, &selections),
        EdgeSelection::Resolved {
            edges: vec![first.clone(), second],
            native: scope.id.clone(),
        }
    );
    let duplicate = [
        EdgeSelection::Edges(vec![first.clone()]),
        EdgeSelection::Edges(vec![first]),
    ];
    assert_eq!(
        crate::design::feature_project::merge_edge_selections(&scope, &duplicate),
        EdgeSelection::Native(scope.id)
    );
}

#[test]
fn form_dispatcher_retains_a_group_without_a_cage_identity_join() {
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    for flag_width in [2, 3] {
        let stream = "FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
        let mut cage_list = vec![0; 97 + flag_width];
        cage_list[..4].copy_from_slice(&3u32.to_le_bytes());
        cage_list[4..7].copy_from_slice(b"415");
        cage_list[7..11].copy_from_slice(&205u32.to_le_bytes());
        cage_list[21] = 1;
        cage_list[22..30].copy_from_slice(&201u64.to_le_bytes());
        cage_list[32..36].copy_from_slice(&1u32.to_le_bytes());
        cage_list[36] = 1;
        cage_list[37..45].copy_from_slice(&971u64.to_le_bytes());
        cage_list[47..51].copy_from_slice(&252u32.to_le_bytes());
        cage_list[51..59].copy_from_slice(&0.25f64.to_le_bytes());
        cage_list[59..63].copy_from_slice(&252u32.to_le_bytes());
        cage_list[63] = 1;
        cage_list[64..72].copy_from_slice(&909u64.to_le_bytes());
        if flag_width == 3 {
            cage_list[74] = 1;
        }
        cage_list[74 + flag_width] = 1;
        cage_list[75 + flag_width..83 + flag_width].copy_from_slice(&206u64.to_le_bytes());
        cage_list[86 + flag_width] = 1;
        cage_list[87 + flag_width..95 + flag_width].copy_from_slice(&201u64.to_le_bytes());
        let mut paired = vec![0; 15];
        paired[..4].copy_from_slice(&3u32.to_le_bytes());
        paired[4..7].copy_from_slice(b"258");
        paired[7..11].copy_from_slice(&205u32.to_le_bytes());
        let mut bulk = cage_list;
        bulk.extend_from_slice(&paired);

        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut archive, stored);
        archive.start_file(stream, stored).unwrap();
        archive.write_all(&bulk).unwrap();
        let archive = archive.finish().unwrap().into_inner();

        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            &format!("f3d:{stream}:scope#201"),
            crate::records::feature::scope::DesignFeatureKind::Form,
            201,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(vec![205]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let feature_id = crate::ids::neutral_feature_id(&scope);
        let mut features = vec![cadmpeg_ir::features::Feature {
            product_definition: None,
            id: feature_id,
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: Some("Form".into()),
            source_text: None,
            source_content: Default::default(),

            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Native {
                        kind: "Form".into(),
                        parameters: Default::default(),
                    },
                ),
            ),
            native_ref: Some(scope.id.clone()),
        }];
        let cages = [cadmpeg_ir::SubdSurface {
            id: cadmpeg_ir::ids::SubdId::mint("f3d:model:subd#1").expect("identity grammar"),
            scheme: cadmpeg_ir::subd::SubdScheme::CatmullClark,
            source_object: None,
            cage: cadmpeg_ir::subd::SubdCage::default(),
        }];

        crate::test_support::zip_test::with_scan(&archive, |scan| {
            crate::design::test_support::with_test_decode_context(|ctx| {
                crate::design::feature_project::form_cages::bind_form_cages(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    &mut features,
                    &cages,
                )
            })
        })
        .expect("long Form cage binding");
        assert_eq!(
            *features[0].evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::Native {
                    kind: "Form".into(),
                    parameters: Default::default(),
                }
            )
        );
    }
}
