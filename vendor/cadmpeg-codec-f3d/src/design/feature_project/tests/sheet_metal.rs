// SPDX-License-Identifier: Apache-2.0

use crate::records::topology::{
    construction::DesignConstructionOperandGroup,
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};

#[test]
fn edge_flange_scope_projects_a_typed_two_sided_neutral_flange() {
    use crate::records::feature::{
        scope::DesignParameterScope,
        sheet_metal::{DesignBendPosition, DesignEdgeFlangeOperation, DesignSheetMetalHeightDatum},
    };

    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalBendPosition, SheetMetalFlangeTwoSidedWidth,
        SheetMetalFlangeWidth, SheetMetalHeightDatum,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#900"),
        crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
        382,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                383, 385, 388, 393, 396, 399, 402, 404, 407, 411,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignEdgeFlangeOperation {
            height_owner_record_index: 399,
            angle_owner_record_index: 402,
            auxiliary_reference_record_indices: Vec::new(),
            settings_record_index: 411,
            bend_radius: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("positive bend radius"),
            bend_radius_offset: 156,
            height_datum: DesignSheetMetalHeightDatum::InnerFaces,
            bend_position: DesignBendPosition::Adjacent,
            selection: crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides {
                    edges: vec![crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
                        wrapper: 383,
                        group_record_index: 385_u32.try_into().unwrap(),
                        aggregate_operand_record_index: 407,
                    }],
                    owners: [393, 396],
                },
                404,
            )
            .unwrap(),
        });
    }

    let owner = |record_index: u32, parameter_record_index: u32| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("{stream}:design-parameter-owner#{record_index}"),
                byte_offset: 0,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index: 382,
                local_ordinal: 0,
                evaluated_value: 0.0,
                evaluated_value_offset: 40,
                parameter_record_index,
                owned_ordinal: 0,
                variant: Some(0),
                companion_record_index: record_index + 1,
            },
        )
        .unwrap()
    };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, evaluated_value: f64| {
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: format!("{stream}:design-parameter#{record_index}"),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                source_ordinal: 0,
                source: crate::records::parameters::DesignParameterSource::new(
                    source_kind.into(),
                    Some(0),
                    None,
                )
                .unwrap(),
                expression: evaluated_value.to_string(),
                expression_offset: 40,
                source_kind_offset: 60,

                unit: Some(crate::records::identity::RecordedValue {
                    value: unit.into(),
                    offset: 70,
                }),
                name: source_kind.into(),
                name_offset: 80,
                evaluated_value,
                evaluated_value_offset: 90,
            },
        )
        .unwrap()
    };
    let owners = [
        owner(393, 392),
        owner(396, 395),
        owner(399, 398),
        owner(402, 401),
    ];
    // Stored lengths are centimetres and stored angles are radians.
    let parameters = [
        parameter(392, "EdgeWidth_1", "mm", 3.0),
        parameter(395, "EdgeWidth_2", "mm", 1.5),
        parameter(398, "FlangeHeight", "mm", 2.5),
        parameter(401, "FlangeAngle", "deg", std::f64::consts::FRAC_PI_2),
    ];
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("{stream}:design-construction-operand-group#385"),
            scope_record_index: 382,
            scope_reference_ordinal: 1,
            record_index: 385,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 388,
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
                "000".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();

    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        construction_groups: std::slice::from_ref(&group),
        ..Default::default()
    };
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(&scope, &inputs, decode_ctx)
    })
    .unwrap()
    .expect("typed EdgeFlange definition");

    let mut mismatched = group.clone();
    mismatched
        .try_set_members(vec![crate::records::identity::Located {
            value: 389,
            offset: 0,
        }])
        .unwrap();
    let mismatched_inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        construction_groups: std::slice::from_ref(&mismatched),
        ..Default::default()
    };
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(&scope, &mismatched_inputs, decode_ctx)
    })
    .unwrap()
    .is_none());

    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange {
        height,
        angle,
        height_datum,
        bend_position,
        width,
        bend_radius,
        ..
    }) = definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    let cadmpeg_ir::features::SheetMetalFlangeHeight::Distance(height) = height else {
        panic!("expected a distance flange height");
    };
    assert!((height.get() - 25.0).abs() < 1.0e-12);
    assert!((angle.get() - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert_eq!(height_datum, SheetMetalHeightDatum::InnerFaces);
    assert_eq!(bend_position, SheetMetalBendPosition::Adjacent);
    assert!((bend_radius.get() - 2.5).abs() < 1.0e-12);
    let SheetMetalFlangeWidth::TwoSides { first, second } = width else {
        panic!("expected a two-sided flange width");
    };
    assert!((first.get() - 30.0).abs() < 1.0e-12);
    assert!((second.get() - 15.0).abs() < 1.0e-12);

    let mut offset_scope = scope.clone();
    let mut offset_operation = offset_scope
        .edge_flange_operation()
        .cloned()
        .expect("single-edge operation fixture");
    offset_operation.selection = crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
        crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
            edges: offset_operation
                .selection
                .shape()
                .edges()
                .copied()
                .map(|edge| crate::records::feature::sheet_metal::DesignFlangeEdgeWidth {
                    edge,
                    owners: [393, 396],
                })
                .collect(),
            source: crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeOffset,
        },
        offset_operation.selection.aggregate_group_record_index(),
    )
    .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        offset_scope.payload_mut()
    {
        *slot = Some(offset_operation);
    }
    let mut offset_parameters = parameters.clone();
    offset_parameters[0]
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                "EdgeOffset_1".into(),
                offset_parameters[0].owner_record_index(),
                offset_parameters[0].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    offset_parameters[0].try_set_evaluated_value(-3.0).unwrap();
    offset_parameters[1]
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                "EdgeOffset_2".into(),
                offset_parameters[1].owner_record_index(),
                offset_parameters[1].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    offset_parameters[1].try_set_evaluated_value(-1.5).unwrap();
    let offset_inputs = crate::design::feature_project::ProjectInputs {
        native: &offset_parameters,
        owners: &owners,
        construction_groups: std::slice::from_ref(&group),
        ..Default::default()
    };
    let offset_definition = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(
            &offset_scope,
            &offset_inputs,
            decode_ctx,
        )
    })
    .unwrap()
    .expect("typed signed-offset EdgeFlange definition");
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange { width, .. }) =
        offset_definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    assert_eq!(
        width,
        SheetMetalFlangeWidth::TwoSidesPerEdge {
            widths: cadmpeg_ir::features::SheetMetalFlangeEdgeWidths::new(vec![
                SheetMetalFlangeTwoSidedWidth {
                    first: cadmpeg_ir::scalar::PositiveLength::new(30.0).unwrap(),
                    second: cadmpeg_ir::scalar::PositiveLength::new(15.0).unwrap(),
                }
            ])
            .unwrap(),
        }
    );

    let mut multi_scope = scope.clone();
    let mut multi_operation = multi_scope
        .edge_flange_operation()
        .cloned()
        .expect("single-edge operation fixture");
    let mut multi_shape = multi_operation.selection.shape().clone();
    if let crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSides { edges, .. } =
        &mut multi_shape
    {
        edges.push(crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
            wrapper: edges[0].wrapper,
            group_record_index: 415_u32.try_into().unwrap(),
            aggregate_operand_record_index: 420,
        });
    }
    multi_operation.selection =
        crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
            multi_shape,
            multi_operation.selection.aggregate_group_record_index(),
        )
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        multi_scope.payload_mut()
    {
        *slot = Some(multi_operation.clone());
    }
    let mut second_group = group.clone();
    second_group.id = format!("{stream}:design-construction-operand-group#415");
    second_group.record_index = 415;
    second_group
        .try_set_members(vec![crate::records::identity::Located {
            value: 418,
            offset: second_group.members()[0].offset,
        }])
        .unwrap();
    let multi_groups = [group, second_group];
    let multi_inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        construction_groups: &multi_groups,
        ..Default::default()
    };
    let multi_definition = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(&multi_scope, &multi_inputs, decode_ctx)
    })
    .unwrap()
    .expect("typed multi-edge EdgeFlange definition");
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange { edges, .. }) =
        multi_definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    assert_eq!(
        edges,
        cadmpeg_ir::features::EdgeSelection::Native(multi_scope.id.clone())
    );

    let mut per_edge_parameters = parameters.clone();
    per_edge_parameters[0]
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                "EdgeWidth".into(),
                per_edge_parameters[0].owner_record_index(),
                per_edge_parameters[0].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    per_edge_parameters[1]
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                "EdgeWidth".into(),
                per_edge_parameters[1].owner_record_index(),
                per_edge_parameters[1].family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    per_edge_parameters[1].try_set_evaluated_value(3.0).unwrap();
    let mut per_edge_operation = multi_operation;
    per_edge_operation.selection =
        crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
            crate::records::feature::sheet_metal::DesignEdgeFlangeShape::SymmetricPerEdge(
                per_edge_operation
                    .selection
                    .shape()
                    .edges()
                    .copied()
                    .zip(
                        per_edge_operation
                            .selection
                            .shape()
                            .owner_indices()
                            .copied(),
                    )
                    .map(|(edge, owners)| {
                        crate::records::feature::sheet_metal::DesignFlangeEdgeWidth { edge, owners }
                    })
                    .collect(),
            ),
            per_edge_operation.selection.aggregate_group_record_index(),
        )
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        multi_scope.payload_mut()
    {
        *slot = Some(per_edge_operation);
    }
    let per_edge_inputs = crate::design::feature_project::ProjectInputs {
        native: &per_edge_parameters,
        owners: &owners,
        construction_groups: &multi_groups,
        ..Default::default()
    };
    let per_edge_definition = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(
            &multi_scope,
            &per_edge_inputs,
            decode_ctx,
        )
    })
    .unwrap()
    .expect("equal per-edge symmetric widths project to one neutral width");
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange { width, .. }) =
        per_edge_definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    assert_eq!(
        width,
        SheetMetalFlangeWidth::Symmetric {
            width: cadmpeg_ir::scalar::PositiveLength::new(30.0).unwrap(),
        }
    );
    let mut distinct_parameters = per_edge_parameters.clone();
    distinct_parameters[1].try_set_evaluated_value(1.5).unwrap();
    let distinct_inputs = crate::design::feature_project::ProjectInputs {
        native: &distinct_parameters,
        owners: &owners,
        construction_groups: &multi_groups,
        ..Default::default()
    };
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_edge_flange(
                &multi_scope,
                &distinct_inputs,
                decode_ctx,
            )
        })
        .unwrap()
        .is_none(),
        "distinct per-edge widths must remain source-native"
    );

    let mut two_sided_per_edge_operation = multi_scope
        .edge_flange_operation()
        .cloned()
        .expect("per-edge width operation fixture");
    two_sided_per_edge_operation.selection =
        crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
            crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                edges: two_sided_per_edge_operation
                    .selection
                    .shape()
                    .edges()
                    .copied()
                    .zip([[393, 396], [414, 417]])
                    .map(
                        |(edge, owners)| crate::records::feature::sheet_metal::DesignFlangeEdgeWidth {
                            edge,
                            owners,
                        },
                    )
                    .collect(),
                source: crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
            },
            two_sided_per_edge_operation
                .selection
                .aggregate_group_record_index(),
        )
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        multi_scope.payload_mut()
    {
        *slot = Some(two_sided_per_edge_operation);
    }
    let mut two_sided_owners = owners.to_vec();
    two_sided_owners.push(owner(414, 413));
    two_sided_owners.push(owner(417, 416));
    let mut two_sided_parameters = parameters.to_vec();
    two_sided_parameters.push(parameter(413, "EdgeWidth_1", "mm", 2.0));
    two_sided_parameters.push(parameter(416, "EdgeWidth_2", "mm", 4.0));
    let two_sided_inputs = crate::design::feature_project::ProjectInputs {
        native: &two_sided_parameters,
        owners: &two_sided_owners,
        construction_groups: &multi_groups,
        ..Default::default()
    };
    let two_sided_definition = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(
            &multi_scope,
            &two_sided_inputs,
            decode_ctx,
        )
    })
    .unwrap()
    .expect("independent two-sided per-edge widths project to a typed neutral law");
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange { width, .. }) =
        two_sided_definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    assert_eq!(
        width,
        SheetMetalFlangeWidth::TwoSidesPerEdge {
            widths: cadmpeg_ir::features::SheetMetalFlangeEdgeWidths::new(vec![
                SheetMetalFlangeTwoSidedWidth {
                    first: cadmpeg_ir::scalar::PositiveLength::new(30.0).unwrap(),
                    second: cadmpeg_ir::scalar::PositiveLength::new(15.0).unwrap(),
                },
                SheetMetalFlangeTwoSidedWidth {
                    first: cadmpeg_ir::scalar::PositiveLength::new(20.0).unwrap(),
                    second: cadmpeg_ir::scalar::PositiveLength::new(40.0).unwrap(),
                },
            ])
            .unwrap(),
        }
    );
}

fn edge_flange_to_object_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    include_target_scope: bool,
    two_sided_width: bool,
) -> Result<
    Option<(
        cadmpeg_ir::features::FeatureDefinition,
        crate::records::feature::scope::DesignParameterScope,
    )>,
    cadmpeg_core::CodecError,
> {
    use crate::records::feature::{
        scope::DesignParameterScope,
        sheet_metal::{
            DesignBendPosition, DesignEdgeFlangeHeightExtent, DesignEdgeFlangeOperation,
            DesignSheetMetalHeightDatum,
        },
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#910"),
        crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
        382,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignEdgeFlangeOperation {
            height_owner_record_index: 399,
            angle_owner_record_index: 402,
            auxiliary_reference_record_indices: Vec::new(),
            settings_record_index: 411,
            bend_radius: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("positive bend radius"),
            bend_radius_offset: 156,
            height_datum: DesignSheetMetalHeightDatum::OuterFaces,
            bend_position: DesignBendPosition::Inside,
            selection: crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
                if two_sided_width {
                    crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge {
                        edges: vec![crate::records::feature::sheet_metal::DesignFlangeEdgeWidth {
                            edge: crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
                                wrapper: 383,
                                group_record_index: 385_u32.try_into().unwrap(),
                                aggregate_operand_record_index: 407,
                            },
                            owners: [435, 438],
                        }],
                        source: crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeWidth,
                    }
                } else {
                    crate::records::feature::sheet_metal::DesignEdgeFlangeShape::FullEdge {
                        edges: vec![crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
                        wrapper: 383,
                        group_record_index: 385_u32.try_into().unwrap(),
                        aggregate_operand_record_index: 407,
                    }],
                        height: DesignEdgeFlangeHeightExtent::ToObject {
                        target_group_record_index: 421,
                        target_operand_record_index: 424,
                        offset_owner_record_index: 430,
                        reference_record_indices: [469, 470],
                    },
                    }
                },
                404,
            )
            .unwrap(),
        });
    }

    let owner = |record_index: u32, parameter_record_index: u32| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("{stream}:design-parameter-owner#{record_index}"),
                byte_offset: 0,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index: 382,
                local_ordinal: 0,
                evaluated_value: 0.0,
                evaluated_value_offset: 40,
                parameter_record_index,
                owned_ordinal: 0,
                variant: Some(0),
                companion_record_index: record_index + 1,
            },
        )
        .unwrap()
    };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, evaluated_value: f64| {
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: format!("{stream}:design-parameter#{record_index}"),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                source_ordinal: 0,
                source: crate::records::parameters::DesignParameterSource::new(
                    source_kind.into(),
                    Some(0),
                    None,
                )
                .unwrap(),
                expression: evaluated_value.to_string(),
                expression_offset: 40,
                source_kind_offset: 60,

                unit: Some(crate::records::identity::RecordedValue {
                    value: unit.into(),
                    offset: 70,
                }),
                name: source_kind.into(),
                name_offset: 80,
                evaluated_value,
                evaluated_value_offset: 90,
            },
        )
        .unwrap()
    };
    let mut owners = vec![owner(399, 398), owner(402, 401), owner(430, 429)];
    let mut parameters = vec![
        parameter(398, "FlangeHeight", "mm", 2.5),
        parameter(401, "FlangeAngle", "deg", std::f64::consts::FRAC_PI_2),
        parameter(429, "ToObjectOffset", "mm", 1.5),
    ];
    if two_sided_width {
        owners.push(owner(435, 434));
        owners.push(owner(438, 437));
        parameters.push(parameter(434, "EdgeWidth_1", "mm", 3.0));
        parameters.push(parameter(437, "EdgeWidth_2", "mm", 1.5));
    }

    let edge_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("{stream}:design-construction-operand-group#385"),
            scope_record_index: 382,
            scope_reference_ordinal: 1,
            record_index: 385,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 388,
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
                "000".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let mut target_group = edge_group.clone();
    target_group.id = format!("{stream}:design-construction-operand-group#421");
    target_group.scope_reference_ordinal = 2;
    target_group.record_index = 421;
    target_group
        .try_set_members(vec![crate::records::identity::Located {
            value: 424,
            offset: target_group.members()[0].offset,
        }])
        .unwrap();
    target_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X21,
        );

    let target_selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: format!("{stream}:design-entity-selection-operand#424"),
                scope_record_index: 382,
                group_record_index: 421,
                group_member_ordinal: 0,
                record_index: 424,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 0,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 0,
                identity_record_index: 427,
                identity_record_offset: 0,
                primary_identity: 319,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 428,
                next_byte_offset: 29,
            },
        )
        .unwrap();
    let mut target_scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#920"),
        crate::records::feature::scope::DesignFeatureKind::WorkPlane,
        320,
    );
    target_scope.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );

    let groups = [edge_group, target_group];
    let target_scopes = [target_scope.clone()];
    let target_selections = [target_selection];
    let inputs = crate::design::feature_project::ProjectInputs {
        native: &parameters,
        owners: &owners,
        scopes: if include_target_scope {
            &target_scopes
        } else {
            &[]
        },
        construction_groups: &groups,
        entity_selection_operands: &target_selections,
        ..Default::default()
    };
    let definition = crate::design::feature_project::project_edge_flange(&scope, &inputs, ctx)?;
    Ok(definition.map(|definition| (definition, target_scope)))
}

#[test]
fn edge_flange_scope_projects_a_to_object_height_to_a_work_plane() {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
    };
    let (definition, target_scope) = crate::test_support::with_decode_context(|decode_ctx| {
        edge_flange_to_object_fixture(decode_ctx, true, false)
    })
    .unwrap()
    .expect("typed to-object EdgeFlange definition");
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange { height, .. }) =
        definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    let SheetMetalFlangeHeight::ToObject { target, offset } = height else {
        panic!("expected a to-object flange height");
    };
    assert_eq!(
        target,
        SheetMetalFlangeHeightTarget::Feature(crate::ids::neutral_feature_id(&target_scope))
    );
    assert_eq!(offset.get(), 15.0);
}

#[test]
fn edge_flange_unresolved_to_object_keeps_native_target() {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalFlangeHeight, SheetMetalFlangeHeightTarget,
    };

    let (definition, _) = crate::test_support::with_decode_context(|decode_ctx| {
        edge_flange_to_object_fixture(decode_ctx, false, false)
    })
    .unwrap()
    .expect("unresolved to-object EdgeFlange");
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange {
            height: SheetMetalFlangeHeight::ToObject {
                target: SheetMetalFlangeHeightTarget::Native(_),
                ..
            },
            ..
        })
    ));
}

#[test]
fn edge_flange_two_sided_projects() {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalFlangeEdgeWidths, SheetMetalFlangeHeight,
        SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let (definition, _) = crate::test_support::with_decode_context(|decode_ctx| {
        edge_flange_to_object_fixture(decode_ctx, true, true)
    })
    .unwrap()
    .expect("two-sided EdgeFlange definition");
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalEdgeFlange {
        height, width, ..
    }) = definition
    else {
        panic!("expected a sheet-metal edge flange");
    };
    // Design lengths are centimetres; the fixture widths are 3.0 and 1.5.
    assert_eq!(
        width,
        SheetMetalFlangeWidth::TwoSidesPerEdge {
            widths: SheetMetalFlangeEdgeWidths::new(vec![SheetMetalFlangeTwoSidedWidth {
                first: PositiveLength::new(30.0).unwrap(),
                second: PositiveLength::new(15.0).unwrap(),
            }])
            .unwrap(),
        }
    );
    assert_eq!(
        height,
        SheetMetalFlangeHeight::Distance(PositiveLength::new(25.0).unwrap())
    );
}

#[test]
fn edge_flange_scope_without_a_width_parameter_keeps_its_native_form() {
    use crate::records::feature::{
        scope::DesignParameterScope,
        sheet_metal::{DesignBendPosition, DesignEdgeFlangeOperation, DesignSheetMetalHeightDatum},
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#901"),
        crate::records::feature::scope::DesignFeatureKind::EdgeFlange,
        317,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                318, 320, 323, 328, 331, 334, 336, 339, 343,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::EdgeFlange(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignEdgeFlangeOperation {
            height_owner_record_index: 331,
            angle_owner_record_index: 334,
            auxiliary_reference_record_indices: Vec::new(),
            settings_record_index: 343,
            bend_radius: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("positive bend radius"),
            bend_radius_offset: 156,
            height_datum: DesignSheetMetalHeightDatum::OuterFaces,
            bend_position: DesignBendPosition::Inside,
            selection: crate::records::feature::sheet_metal::DesignEdgeFlangeSelection::try_new(
                crate::records::feature::sheet_metal::DesignEdgeFlangeShape::Symmetric {
                    edges: vec![crate::records::feature::sheet_metal::DesignEdgeFlangeEdge {
                        wrapper: 318,
                        group_record_index: 320_u32.try_into().unwrap(),
                        aggregate_operand_record_index: 339,
                    }],
                    owner: 328,
                },
                336,
            )
            .unwrap(),
        });
    }

    let inputs = crate::design::feature_project::ProjectInputs {
        ..Default::default()
    };
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::project_edge_flange(&scope, &inputs, decode_ctx)
    })
    .unwrap()
    .is_none());
}

#[test]
fn surface_patch_continuity_needs_every_boundary_to_agree() {
    use crate::records::feature::{
        scope::DesignParameterScope,
        surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary},
    };
    use cadmpeg_ir::features::SurfaceContinuity;

    let boundary = |continuity: DesignPatchContinuity| DesignSurfacePatchBoundary {
        scope_reference_ordinal: 0,
        record_index: 0,
        is_seed_selection: false,
        continuity,
        flip: 2,
        scale: crate::test_support::real(-1.0),
        model_reference: 0,
    };
    let scope_with = |boundaries: Vec<DesignSurfacePatchBoundary>| {
        let mut scope = DesignParameterScope::empty(
            "f3d:test:scope#1",
            crate::records::feature::scope::DesignFeatureKind::SurfacePatch,
            1,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::SurfacePatch(slot) =
            scope.payload_mut()
        {
            let slot = &mut slot.boundaries;
            *slot = boundaries;
        }
        scope
    };
    let uniform_continuity = |scope: &DesignParameterScope| {
        cadmpeg_ir::features::NonEmptyMembers::try_from(crate::test_support::with_decode_context(
            |_decode_ctx| crate::design::feature_project::surface_patch_boundary_conditions(scope),
        ))
        .ok()
        .map(|conditions| cadmpeg_ir::features::FilledSurfaceContinuity { conditions })
        .and_then(|continuity| continuity.uniform())
        .map(cadmpeg_ir::features::FilledSurfaceBoundaryCondition::continuity)
    };

    for (code, expected) in [
        (DesignPatchContinuity::Connected, SurfaceContinuity::Contact),
        (DesignPatchContinuity::Tangent, SurfaceContinuity::Tangent),
        (
            DesignPatchContinuity::Curvature,
            SurfaceContinuity::Curvature,
        ),
    ] {
        let scope = scope_with(vec![boundary(code), boundary(code)]);
        assert_eq!(uniform_continuity(&scope), Some(expected));
    }

    // A patch whose boundaries impose different conditions has no single neutral
    // continuity, and one with no boundary record has none to report.
    let mixed = scope_with(vec![
        boundary(DesignPatchContinuity::Tangent),
        boundary(DesignPatchContinuity::Connected),
    ]);
    assert_eq!(
        crate::test_support::with_decode_context(|_decode_ctx| {
            crate::design::feature_project::surface_patch_boundary_conditions(&mixed)
        }),
        vec![
            cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Tangent {
                weight: cadmpeg_ir::scalar::PositiveReal::new(0.5),
            },
            cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
        ]
    );
    assert!(uniform_continuity(&mixed).is_none());
    assert!(uniform_continuity(&scope_with(Vec::new())).is_none());
    assert!(
        uniform_continuity(&scope_with(vec![boundary(DesignPatchContinuity::Unknown(
            9
        ))]))
        .is_none()
    );
    assert!(crate::test_support::with_decode_context(|_decode_ctx| {
        crate::design::feature_project::surface_patch_boundary_conditions(&scope_with(vec![
            boundary(DesignPatchContinuity::Unknown(9)),
        ]))
    })
    .is_empty());
}

#[test]
fn surface_patch_weights_preserve_source_encoding_and_boundary_order() {
    use crate::records::feature::{
        scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut},
        surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary},
    };
    use cadmpeg_ir::features::FilledSurfaceBoundaryCondition;
    use cadmpeg_ir::scalar::PositiveReal;

    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#1", DesignFeatureKind::SurfacePatch, 1);
    let raw = [
        (DesignPatchContinuity::Connected, -0.0),
        (DesignPatchContinuity::Tangent, -1.0),
        (DesignPatchContinuity::Tangent, 0.15),
        (DesignPatchContinuity::Curvature, 0.423_464_161_563_635_5),
        (DesignPatchContinuity::Curvature, 0.5),
    ];
    if let DesignScopePayloadMut::SurfacePatch(boundaries) = scope.payload_mut() {
        let boundaries = &mut boundaries.boundaries;
        *boundaries = raw
            .into_iter()
            .map(|(continuity, scale)| DesignSurfacePatchBoundary {
                scope_reference_ordinal: 0,
                record_index: 0,
                is_seed_selection: false,
                continuity,
                flip: 2,
                scale: crate::test_support::real(scale),
                model_reference: 0,
            })
            .collect();
    }
    let source = serde_json::to_value(&scope).unwrap();
    let conditions = crate::test_support::with_decode_context(|_ctx| {
        crate::design::feature_project::surface_patch_boundary_conditions(&scope)
    });
    assert_eq!(
        conditions,
        vec![
            FilledSurfaceBoundaryCondition::Contact {},
            FilledSurfaceBoundaryCondition::Tangent {
                weight: PositiveReal::new(0.5)
            },
            FilledSurfaceBoundaryCondition::Tangent {
                weight: PositiveReal::new(0.15)
            },
            FilledSurfaceBoundaryCondition::Curvature {
                weight: PositiveReal::new(0.423_464_161_563_635_5)
            },
            FilledSurfaceBoundaryCondition::Curvature {
                weight: PositiveReal::new(0.5)
            },
        ]
    );
    assert_eq!(serde_json::to_value(&scope).unwrap(), source);
    for (boundary, (_, scale)) in scope.surface_patch_boundaries().iter().zip(raw) {
        assert_eq!(boundary.scale.get().to_bits(), scale.to_bits());
    }
}

#[test]
fn surface_patch_abstains_on_unsupported_active_weights() {
    use crate::records::feature::{
        scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut},
        surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary},
    };
    for continuity in [
        DesignPatchContinuity::Tangent,
        DesignPatchContinuity::Curvature,
    ] {
        for scale in [0.0, -0.0, -0.5, -2.0] {
            let mut scope =
                DesignParameterScope::empty("f3d:test:scope#1", DesignFeatureKind::SurfacePatch, 1);
            if let DesignScopePayloadMut::SurfacePatch(boundaries) = scope.payload_mut() {
                let boundaries = &mut boundaries.boundaries;
                *boundaries = vec![DesignSurfacePatchBoundary {
                    scope_reference_ordinal: 0,
                    record_index: 0,
                    is_seed_selection: false,
                    continuity,
                    flip: 2,
                    scale: crate::test_support::real(scale),
                    model_reference: 0,
                }];
            }
            assert!(crate::test_support::with_decode_context(|_ctx| {
                crate::design::feature_project::surface_patch_boundary_conditions(&scope)
            })
            .is_empty());
            assert_eq!(
                scope.surface_patch_boundaries()[0].scale.get().to_bits(),
                scale.to_bits()
            );
        }
    }
}

#[test]
fn surface_patch_reference_occupancy_preserves_boundary_endpoints() {
    use crate::records::{
        feature::{
            scope::DesignParameterScope,
            surface_ops::{DesignPatchContinuity, DesignSurfacePatchBoundary},
        },
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
        },
    };

    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#1",
        crate::records::feature::scope::DesignFeatureKind::SurfacePatch,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 442;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                900, 100, 101, 102, 110, 111, 112, 120, 121, 122,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::SurfacePatch(slot) =
        scope.payload_mut()
    {
        let slot = &mut slot.boundaries;
        *slot = vec![
            DesignSurfacePatchBoundary {
                scope_reference_ordinal: 3,
                record_index: 102,
                is_seed_selection: false,
                continuity: DesignPatchContinuity::Connected,
                flip: 2,
                scale: crate::test_support::real(-1.0),
                model_reference: 100,
            },
            DesignSurfacePatchBoundary {
                scope_reference_ordinal: 6,
                record_index: 112,
                is_seed_selection: true,
                continuity: DesignPatchContinuity::Connected,
                flip: 2,
                scale: crate::test_support::real(-1.0),
                model_reference: 110,
            },
            DesignSurfacePatchBoundary {
                scope_reference_ordinal: 9,
                record_index: 122,
                is_seed_selection: false,
                continuity: DesignPatchContinuity::Connected,
                flip: 2,
                scale: crate::test_support::real(-1.0),
                model_reference: 120,
            },
        ];
    }
    let scope_record_index = scope.record_index;
    let group = |record_index, ordinal, member| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: format!("f3d:test:construction-group#{record_index}"),
                scope_record_index,
                scope_reference_ordinal: ordinal,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                members: vec![crate::records::identity::Located {
                    value: member,
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
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_A,
                ),
                role_offset: 0,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "260".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 0,
            },
        )
        .unwrap()
    };
    let shifted_groups = [group(100, 1, 101), group(110, 4, 111), group(120, 7, 121)];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_surface_patch(decode_ctx, &scope, &shifted_groups, &[], &[], &[])).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            ref continuity,
            ..
        })) if continuity.resolved().map(|resolved| resolved.conditions.as_slice())
            == Some(
                &[
                    cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
                    cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
                    cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
                ][..],
            )
    ));

    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                100, 101, 102, 110, 111, 112, 120, 121, 122, 900,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let crate::records::feature::scope::DesignScopePayloadMut::SurfacePatch(boundaries) =
        scope.payload_mut()
    else {
        panic!("SurfacePatch fixture");
    };
    for (boundary, ordinal) in boundaries.boundaries.iter_mut().zip([2_u32, 5, 8]) {
        boundary.scope_reference_ordinal = ordinal;
    }
    let endpoint_groups = [group(100, 0, 101), group(110, 3, 111), group(120, 6, 121)];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_surface_patch(decode_ctx, &scope, &endpoint_groups, &[], &[], &[])).unwrap(),
        Some(FeatureDefinition::Operation(FeatureOperation::FilledSurface {
            ref continuity,
            ..
        })) if continuity.resolved().map(|resolved| resolved.conditions.as_slice())
            == Some(
                &[
                    cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
                    cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
                    cadmpeg_ir::features::FilledSurfaceBoundaryCondition::Contact {},
                ][..],
            )
    ));
}

#[test]
fn hem_scope_projects_each_decoded_owner_layout() {
    use crate::records::{
        feature::{
            scope::DesignParameterScope,
            sheet_metal::{DesignHemOperation, DesignHemParameterOwners},
        },
        parameters::{DesignParameter, DesignParameterOwner},
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
        },
    };
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureOperation, SheetMetalHemDirection, SheetMetalHemForm,
    };

    let stream = "f3d:FusionAssetName[Active]/FusionDesignSegmentType1/BulkStream.dat";
    let owner = |scope_record_index: u32,
                 record_index: u32,
                 parameter_record_index: u32|
     -> DesignParameterOwner {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("{stream}:design-parameter-owner#{record_index}"),
                byte_offset: 0,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal: 0,
                evaluated_value: 0.0,
                evaluated_value_offset: 40,
                parameter_record_index,
                owned_ordinal: 0,
                variant: Some(0),
                companion_record_index: record_index + 1,
            },
        )
        .unwrap()
    };
    let parameter = |record_index: u32, source_kind: &str, unit: &str, value: f64| {
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: format!("{stream}:design-parameter#{record_index}"),
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                record_index,
                source_ordinal: 0,
                source: crate::records::parameters::DesignParameterSource::new(
                    source_kind.into(),
                    Some(0),
                    None,
                )
                .unwrap(),
                expression: value.to_string(),
                expression_offset: 40,
                source_kind_offset: 60,

                unit: Some(crate::records::identity::RecordedValue {
                    value: unit.into(),
                    offset: 70,
                }),
                name: source_kind.into(),
                name_offset: 80,
                evaluated_value: value,
                evaluated_value_offset: 90,
            },
        )
        .unwrap()
    };
    let group = |scope_record_index: u32,
                 record_index: u32,
                 member: u32,
                 role: DesignOperandRole| {
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: format!("{stream}:design-construction-operand-group#{record_index}"),
                scope_record_index,
                scope_reference_ordinal: 0,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("000".to_owned())
                    .unwrap(),
                members: vec![crate::records::identity::Located {
                    value: member,
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
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
                role_offset: 0,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "000".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 0,
            },
        )
        .unwrap()
    };
    let operation = |parameter_owners| DesignHemOperation {
        bend_position: cadmpeg_ir::features::SheetMetalHemBendPosition::Adjacent,
        flipped: false,
        edge_wrapper_record_index: 708,
        edge_group_record_index: 710_u32.try_into().unwrap(),
        aggregate_group_record_index: 717_u32.try_into().unwrap(),
        parameter_owners,
        settings_record_index: 724,
        bend_radius: cadmpeg_ir::scalar::PositiveReal::new(0.25).expect("positive bend radius"),
        bend_radius_offset: 100,
    };
    let project = |record_index: u32,
                   operation: DesignHemOperation,
                   owners: Vec<DesignParameterOwner>,
                   parameters: Vec<DesignParameter>| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::Hem,
            record_index,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::Hem(slot) =
            scope.payload_mut()
        {
            *slot = Some(operation);
        }
        let groups = vec![
            group(record_index, 710, 713, DesignOperandRole::BODIES_B),
            group(record_index, 717, 720, DesignOperandRole::ROLE_0X43),
        ];
        let inputs = crate::design::feature_project::ProjectInputs {
            native: &parameters,
            owners: &owners,
            construction_groups: &groups,
            ..Default::default()
        };
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_hem(&scope, &inputs, decode_ctx)
        })
        .unwrap()
        .expect("typed Hem definition")
    };

    let gap_length = project(
        900,
        operation(DesignHemParameterOwners::GapLength {
            gap_owner_record_index: 901,
            length_owner_record_index: 904,
        }),
        vec![owner(900, 901, 903), owner(900, 904, 906)],
        vec![
            parameter(903, "HemGap", "mm", 0.02),
            parameter(906, "HemLength", "mm", 10.0),
        ],
    );
    let mut flipped_operation = operation(DesignHemParameterOwners::GapLength {
        gap_owner_record_index: 901,
        length_owner_record_index: 904,
    });
    flipped_operation.flipped = true;
    let reversed = project(
        900,
        flipped_operation,
        vec![owner(900, 901, 903), owner(900, 904, 906)],
        vec![
            parameter(903, "HemGap", "mm", 0.02),
            parameter(906, "HemLength", "mm", 10.0),
        ],
    );
    assert!(matches!(
        reversed,
        FeatureDefinition::Operation(FeatureOperation::SheetMetalHem {
            direction: SheetMetalHemDirection::Reverse,
            bend_position: cadmpeg_ir::features::SheetMetalHemBendPosition::Adjacent,
            ..
        })
    ));
    let rolled = project(
        910,
        operation(DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 911,
            angle_owner_record_index: 914,
        }),
        vec![owner(910, 911, 913), owner(910, 914, 916)],
        vec![
            parameter(913, "HemRadius", "mm", 0.5),
            parameter(916, "HemAngle", "deg", std::f64::consts::FRAC_PI_2),
        ],
    );
    let teardrop = project(
        920,
        operation(DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 921,
            length_owner_record_index: 924,
            radius_owner_record_index: 927,
        }),
        vec![
            owner(920, 921, 923),
            owner(920, 924, 926),
            owner(920, 927, 929),
        ],
        vec![
            parameter(923, "HemGap", "mm", 0.25),
            parameter(926, "HemLength", "mm", 10.0),
            parameter(929, "HemRadius", "mm", 0.5),
        ],
    );

    let FeatureDefinition::Operation(FeatureOperation::SheetMetalHem {
        form,
        direction,
        bend_radius,
        ..
    }) = gap_length
    else {
        panic!("expected a gap-length Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::GapLength {
            gap: cadmpeg_ir::scalar::NonNegativeLength::new(0.2).unwrap(),
            length: cadmpeg_ir::scalar::PositiveLength::new(100.0).unwrap(),
        }
    );
    assert_eq!(direction, SheetMetalHemDirection::Forward);
    assert_eq!(
        bend_radius,
        cadmpeg_ir::scalar::PositiveLength::new(2.5).unwrap()
    );

    let FeatureDefinition::Operation(FeatureOperation::SheetMetalHem { form, .. }) = rolled else {
        panic!("expected a rolled Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::Rolled {
            radius: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
            angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        }
    );

    let FeatureDefinition::Operation(FeatureOperation::SheetMetalHem { form, .. }) = teardrop
    else {
        panic!("expected a teardrop Hem");
    };
    assert_eq!(
        form,
        SheetMetalHemForm::Teardrop {
            gap: cadmpeg_ir::scalar::NonNegativeLength::new(2.5).unwrap(),
            length: cadmpeg_ir::scalar::PositiveLength::new(100.0).unwrap(),
            radius: cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap(),
        }
    );
}
