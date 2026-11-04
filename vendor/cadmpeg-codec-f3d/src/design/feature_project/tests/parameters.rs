// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::decode::parameters::parse_parameter_owner;
use crate::design::dimensions::expression_identifiers;
use crate::design::feature_project::untyped_parameter_unit_count;
use crate::design::test_support::parameter_owner_frame;
use crate::design::test_support::parameter_record;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayload;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::scalar::Length;

#[test]
fn user_parameters_project_in_source_order_with_units_and_dependencies() {
    let mut width = parse_design_parameter_record(&parameter_record(
        None,
        "60 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        6.0,
    ))
    .unwrap();
    width.id = "f3d:native:parameter#width".into();
    width.record_index = 20;
    width.source_ordinal = 4;
    let mut half = parse_design_parameter_record(&parameter_record(
        None,
        "Width / 2",
        "User Parameter",
        Some("mm"),
        "HalfWidth",
        3.0,
    ))
    .unwrap();
    half.id = "f3d:native:parameter#half".into();
    half.record_index = 21;
    half.source_ordinal = 5;

    let crate::design::feature_project::ProjectedParameterDesign {
        features,
        parameters: projected,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &[half, width],
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(features.is_empty());
    assert_eq!(projected[0].name, "Width");
    assert_eq!(projected[0].owner, None);
    assert_eq!(
        projected[0].value,
        Some(ParameterValue::Length(Length::new(60.0).unwrap()))
    );
    assert_eq!(
        projected[1].dependencies.as_slice(),
        [projected[0].id.clone()]
    );
    assert_eq!(
        projected[1].native_ref.as_deref(),
        Some("f3d:native:parameter#half")
    );
}

#[test]
fn parameters_project_all_design_database_unit_tokens() {
    let mut native = ["mm", "cm", "m", "in", "ft", "deg", "rad"]
        .into_iter()
        .enumerate()
        .map(|(ordinal, unit)| {
            let mut parameter = parse_design_parameter_record(&parameter_record(
                None,
                "value",
                "User Parameter",
                Some(unit),
                &format!("Value{ordinal}"),
                1.25,
            ))
            .expect("generated database-unit parameter");
            parameter.id = format!("f3d:native:parameter#{ordinal}");
            parameter.record_index = u32::try_from(ordinal).unwrap();
            parameter.source_ordinal = u32::try_from(ordinal).unwrap();
            parameter
        })
        .collect::<Vec<_>>();
    native.reverse();
    let mut unclassified = parse_design_parameter_record(&parameter_record(
        None,
        "value",
        "User Parameter",
        Some("native-unit"),
        "Unclassified",
        2.75,
    ))
    .expect("generated unclassified-unit parameter");
    unclassified.id = "f3d:native:parameter#7".into();
    unclassified.record_index = 7;
    unclassified.source_ordinal = 7;
    native.push(unclassified);

    let crate::design::feature_project::ProjectedParameterDesign {
        parameters: projected,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &native,
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    for ordinal in 0..5 {
        assert_eq!(
            projected
                .iter()
                .find(|parameter| parameter.name == format!("Value{ordinal}"))
                .and_then(|parameter| parameter.value.clone()),
            Some(ParameterValue::Length(Length::new(12.5).unwrap()))
        );
    }
    for ordinal in 5..7 {
        assert_eq!(
            projected
                .iter()
                .find(|parameter| parameter.name == format!("Value{ordinal}"))
                .and_then(|parameter| parameter.value.clone()),
            Some(ParameterValue::Angle(Angle::new(1.25).unwrap()))
        );
    }
    let unclassified = projected
        .iter()
        .find(|parameter| parameter.name == "Unclassified")
        .expect("unclassified-unit parameter");
    assert_eq!(unclassified.value, None);
    assert_eq!(
        unclassified.properties.get("unit").map(String::as_str),
        Some("native-unit")
    );
    assert_eq!(
        unclassified
            .properties
            .get("evaluated_scalar")
            .map(String::as_str),
        Some("2.75")
    );
    assert_eq!(untyped_parameter_unit_count(&native), 1);
}

#[test]
fn expression_dependencies_preserve_fusion_parameter_name_symbols() {
    let name = "Width$µ°\"A";
    assert_eq!(
        expression_identifiers(&format!("{name} / 2 + sin(30 deg)")).collect::<Vec<_>>(),
        [name]
    );
    let parameter = |record_index, source_ordinal, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated symbolic-name parameter");
        parameter.id = format!("f3d:native:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = source_ordinal;
        parameter
    };
    let crate::design::feature_project::ProjectedParameterDesign {
        parameters: projected,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &[
                    parameter(20, 0, "10 mm", name),
                    parameter(21, 1, "1", "sin"),
                    parameter(22, 2, "1", "deg"),
                    parameter(23, 3, "1", "mm"),
                    parameter(24, 4, &format!("{name} / 2 + sin(30 deg) + 10 mm"), "Half"),
                    parameter(25, 5, "mm + 1", "BareUnitName"),
                ],
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    let source = projected
        .iter()
        .find(|parameter| parameter.name == name)
        .expect("symbolic-name source parameter");
    let half = projected
        .iter()
        .find(|parameter| parameter.name == "Half")
        .expect("dependent parameter");
    assert_eq!(half.dependencies.as_slice(), [source.id.clone()]);
    let millimetres = projected
        .iter()
        .find(|parameter| parameter.name == "mm")
        .expect("bare unit-named parameter");
    let bare_unit_name = projected
        .iter()
        .find(|parameter| parameter.name == "BareUnitName")
        .expect("consumer of bare unit-named parameter");
    assert_eq!(
        bare_unit_name.dependencies.as_slice(),
        [millimetres.id.clone()]
    );
}

#[test]
fn owned_parameter_projects_under_its_real_scope_feature() {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "60 mm",
        "AlongDistance",
        Some("mm"),
        "d12",
        6.0,
    ))
    .unwrap();
    parameter.id = "f3d:native/BulkStream.dat:parameter#45".into();
    parameter.record_index = 45;
    let mut owner = parse_parameter_owner(&parameter_owner_frame())
        .unwrap()
        .into_record("Design/BulkStream.dat", 0)
        .unwrap();
    {
        let mut wire = crate::records::parameters::DesignParameterOwnerWire::from(owner.clone());
        wire.id = "f3d:native/BulkStream.dat:parameter-owner#44".into();
        owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }
    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:native/BulkStream.dat:parameter-scope#12".into(),
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
                vec![44, 44],
                vec![185, 196],
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

    let crate::design::feature_project::ProjectedParameterDesign {
        features,
        parameters,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[scope];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &[parameter],
                owners: &[owner],
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].name.as_deref(), Some("Extrude 1"));
    assert_eq!(features[0].suppressed, Some(true));
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Extrude,
            parameters,
        }) if parameters.get("d12").map(String::as_str) == Some("60 mm")
    ));
    assert_eq!(
        features[0]
            .source_properties
            .get("reference:0")
            .map(String::as_str),
        Some("44")
    );
    assert_eq!(
        features[0]
            .source_properties
            .get("reference:1")
            .map(String::as_str),
        Some("44")
    );
    assert_eq!(parameters[0].owner.as_ref(), Some(&features[0].id));
    assert_eq!(parameters[0].ordinal, 2);
    assert_eq!(
        parameters[0]
            .properties
            .get("source_kind")
            .map(String::as_str),
        Some("AlongDistance")
    );
}

#[test]
fn unresolved_owned_parameters_receive_unique_dependency_ordered_slots() {
    let records = [
        (Some(40), 2, "InputA", "1"),
        (Some(50), 2, "InputB", "1"),
        (None, 0, "Result", "Middle + 1"),
        (None, 1, "Middle", "InputA + 1"),
        (None, 3, "Independent", "1"),
    ];
    let native = records
        .iter()
        .enumerate()
        .map(|(index, &(owner, ordinal, name, expression))| {
            let mut parameter = parse_design_parameter_record(&parameter_record(
                owner,
                expression,
                if owner.is_some() {
                    "FeatureInput"
                } else {
                    "User Parameter"
                },
                None,
                name,
                1.0,
            ))
            .unwrap();
            parameter.record_index = u32::try_from(index).unwrap();
            parameter.id = format!("f3d:Design/BulkStream.dat:parameter#{index}");
            parameter.source_ordinal = ordinal;
            parameter
        })
        .collect::<Vec<_>>();
    let source = serde_json::to_value(&native).unwrap();
    let project = |native: &[_]| {
        crate::test_support::with_decode_context(|ctx| {
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native,
                    ..Default::default()
                },
            )
        })
    };
    let projected = project(&native).unwrap();
    assert!(projected.features.is_empty());
    assert_eq!(projected.unordered_parameter_scopes, 0);
    let mut ordered = projected.parameters.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|parameter| parameter.ordinal);
    assert_eq!(
        ordered
            .iter()
            .map(|parameter| parameter.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
    let by_name = |name| {
        projected
            .parameters
            .iter()
            .find(|p| p.name == name)
            .unwrap()
    };
    for (consumer, dependency) in [("Middle", "InputA"), ("Result", "Middle")] {
        let consumer = by_name(consumer);
        let dependency = by_name(dependency);
        assert_eq!(consumer.dependencies.as_slice(), [dependency.id.clone()]);
        assert!(dependency.ordinal < consumer.ordinal);
    }
    for (index, &(owner, ordinal, name, _)) in records.iter().enumerate() {
        let parameter = by_name(name);
        assert!(parameter.owner.is_none());
        assert_eq!(
            parameter.native_ref.as_deref(),
            Some(native[index].id.as_str())
        );
        assert_eq!(native[index].source_ordinal, ordinal);
        assert_eq!(native[index].owner_record_index(), owner);
        assert_eq!(
            parameter.properties.get("owner_record_index"),
            owner.map(|index| index.to_string()).as_ref()
        );
    }
    assert_eq!(serde_json::to_value(&native).unwrap(), source);

    let mut native = native;
    native[0].source_ordinal = u32::MAX - 1;
    native[1].source_ordinal = u32::MAX - 1;
    let projected = project(&native).unwrap();
    let mut ordinals = projected
        .parameters
        .iter()
        .map(|p| p.ordinal)
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    assert_eq!(ordinals, [0, 1, 3, u32::MAX - 1, u32::MAX]);
    native[0].source_ordinal = u32::MAX;
    native[1].source_ordinal = u32::MAX;
    assert!(matches!(
        project(&native),
        Err(cadmpeg_core::CodecError::Malformed(message))
            if message == "Design parameter ordinal exceeds u32"
    ));
}

#[test]
fn owned_parameter_without_a_projected_scope_is_retained_unowned() {
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "60 mm",
        "AlongDistance",
        Some("mm"),
        "d12",
        6.0,
    ))
    .unwrap();
    parameter.id = "f3d:native:parameter#45".into();
    parameter.record_index = 45;
    parameter.source_ordinal = 17;
    let mut owner = parse_parameter_owner(&parameter_owner_frame())
        .unwrap()
        .into_record("Design/BulkStream.dat", 0)
        .unwrap();
    {
        let mut wire = crate::records::parameters::DesignParameterOwnerWire::from(owner.clone());
        wire.id = "f3d:native:parameter-owner#44".into();
        owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    }

    let crate::design::feature_project::ProjectedParameterDesign {
        features,
        parameters,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &[parameter],
                owners: &[owner],
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    assert!(features.is_empty());
    let [parameter] = parameters.as_slice() else {
        panic!("expected the parameter to remain in the neutral model");
    };
    assert_eq!(parameter.owner, None);
    assert_eq!(parameter.ordinal, 17);
    assert_eq!(
        parameter
            .properties
            .get("owner_record_index")
            .map(String::as_str),
        Some("44")
    );
}

#[allow(clippy::large_stack_arrays)]
#[test]
fn parameter_dependencies_resolve_feature_scope_before_document_scope() {
    let parameter = |owner, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            owner,
            expression,
            if owner.is_some() {
                "FeatureInput"
            } else {
                "User Parameter"
            },
            Some("mm"),
            name,
            1.0,
        ))
        .unwrap();
        parameter.id = format!("f3d:Design/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, scope_record_index| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:owner#{record_index}"),
                byte_offset: 0,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal: parameter_record_index,
                evaluated_value: 1.0,
                evaluated_value_offset: 40,
                parameter_record_index,
                owned_ordinal: parameter_record_index,
                variant: Some(0),
                companion_record_index: record_index + 1,
            },
        )
        .unwrap()
    };
    let scope = |record_index| {
        DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: format!("f3d:Design/BulkStream.dat:scope#{record_index}"),
                byte_offset: u64::from(record_index),
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                record_index,
                frame_length: 200,
                kind_offset: 0,
                feature_ordinal: std::num::NonZeroU32::new(record_index).expect("nonzero ordinal"),
                feature_ordinal_offset: 0,
                history_state_id: None,

                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: (u64::from(record_index)) + 9,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![1],
                    vec![0],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::CustomFeature
                    .try_into()
                    .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "302".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: u64::from(record_index) + 100,
            }
            .with_fixture_layout(),
        )
        .unwrap()
    };

    let document_width = parameter(None, 20, "60 mm", "Width");
    let local_width = parameter(Some(22), 21, "20 mm", "Width");
    let local_half = parameter(Some(23), 22, "Width / 2", "Half");
    let remote_half = parameter(Some(24), 23, "Width / 2", "Half");
    let owned_depth = parameter(Some(25), 24, "10 mm", "OwnedDepth");
    let document_half = parameter(None, 25, "OwnedDepth / 2", "DocumentHalf");
    let document_forward = parameter(None, 26, "Later / 2", "DocumentForward");
    let document_later = parameter(None, 27, "10 mm", "Later");
    let cycle_a = parameter(None, 28, "CycleB / 2", "CycleA");
    let cycle_b = parameter(None, 29, "CycleA / 2", "CycleB");
    let preceding_shared = parameter(Some(31), 30, "10 mm", "Shared");
    let shared_consumer = parameter(Some(32), 31, "Shared / 2", "SharedHalf");
    let later_shared = parameter(Some(33), 32, "20 mm", "Shared");
    let crate::design::feature_project::ProjectedParameterDesign { parameters, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[scope(201), scope(202), scope(203)];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[
                        document_width,
                        local_width,
                        local_half,
                        remote_half,
                        owned_depth,
                        document_half,
                        document_forward,
                        document_later,
                        cycle_a,
                        cycle_b,
                        preceding_shared,
                        shared_consumer,
                        later_shared,
                    ],
                    owners: &[
                        owner(22, 21, 201),
                        owner(23, 22, 201),
                        owner(24, 23, 202),
                        owner(25, 24, 201),
                        owner(31, 30, 201),
                        owner(32, 31, 202),
                        owner(33, 32, 203),
                    ],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let by_name_and_owner = |name: &str, owner_record_index: u32| {
        parameters
            .iter()
            .find(|parameter| {
                parameter.name == name
                    && parameter.native_ref.as_deref()
                        == Some(
                            format!("f3d:Design/BulkStream.dat:parameter#{}", owner_record_index)
                                .as_str(),
                        )
            })
            .unwrap()
    };
    let document = by_name_and_owner("Width", 20);
    let local = by_name_and_owner("Width", 21);
    assert_eq!(
        by_name_and_owner("Half", 22).dependencies.as_slice(),
        [local.id.clone()]
    );
    assert_eq!(
        by_name_and_owner("Half", 23).dependencies.as_slice(),
        [document.id.clone()]
    );
    assert!(by_name_and_owner("DocumentHalf", 25)
        .dependencies
        .is_empty());
    let document_forward = by_name_and_owner("DocumentForward", 26);
    let document_later = by_name_and_owner("Later", 27);
    assert_eq!(
        document_forward.dependencies.as_slice(),
        [document_later.id.clone()]
    );
    assert!(document_later.ordinal < document_forward.ordinal);
    let cycle_a = by_name_and_owner("CycleA", 28);
    let cycle_b = by_name_and_owner("CycleB", 29);
    assert!(cycle_a.dependencies.is_empty());
    assert_eq!(cycle_b.dependencies.as_slice(), [cycle_a.id.clone()]);
    assert!(cycle_a.ordinal < cycle_b.ordinal);
    let preceding_shared = by_name_and_owner("Shared", 30);
    assert_eq!(
        by_name_and_owner("SharedHalf", 31).dependencies.as_slice(),
        [preceding_shared.id.clone()]
    );
}

#[allow(clippy::large_stack_arrays)]
#[test]
fn parameter_expressions_project_feature_dependencies() {
    let parameter = |owner_record_index, record_index, name: &str, expression: &str| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(owner_record_index),
            expression,
            "AlongDistance",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated owned parameter is canonical");
        parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, scope_record_index, parameter_record_index| {
        let mut owner = parse_parameter_owner(&parameter_owner_frame())
            .expect("generated parameter owner is canonical")
            .into_record("Design/BulkStream.dat", 0)
            .unwrap();
        {
            let mut wire =
                crate::records::parameters::DesignParameterOwnerWire::from(owner.clone());
            wire.id = format!("f3d:native/BulkStream.dat:owner#{record_index}");
            wire.record_index = record_index;
            wire.scope_record_index = scope_record_index;
            wire.parameter_record_index = parameter_record_index;
            wire.companion_record_index = parameter_record_index + 1;
            owner = crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
        }
        owner
    };
    let scope = |record_index, byte_offset, kind: &str| {
        DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: format!("f3d:native/BulkStream.dat:scope#{record_index}"),
                byte_offset,
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                record_index,
                frame_length: 200,
                kind_offset: byte_offset + 100,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: None,

                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: byte_offset + 80,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![record_index + 1],
                    vec![byte_offset + 85],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::try_from(
                    kind.to_owned(),
                )
                .expect("nonempty family name")
                .try_into()
                .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: byte_offset + 200,
            }
            .with_fixture_layout(),
        )
        .unwrap()
    };
    let crate::design::feature_project::ProjectedParameterDesign {
        features,
        parameters,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[
            scope(12, 100, "Sketch"),
            scope(22, 200, "Extrude"),
            scope(32, 300, "Fillet"),
        ];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &[
                    parameter(44, 45, "Width", "10 mm"),
                    parameter(54, 55, "Depth", "Width / 2"),
                    parameter(74, 75, "Premature", "Future / 2"),
                    parameter(84, 85, "Future", "20 mm"),
                ],
                owners: &[
                    owner(44, 12, 45),
                    owner(54, 22, 55),
                    owner(74, 22, 75),
                    owner(84, 32, 85),
                ],
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    let width = parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("Width parameter");
    let depth = parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("Depth parameter");
    assert_eq!(
        depth.dependencies.as_slice(),
        std::slice::from_ref(&width.id)
    );
    let premature = parameters
        .iter()
        .find(|parameter| parameter.name == "Premature")
        .expect("Premature parameter");
    assert!(premature.dependencies.is_empty());
    let source = features
        .iter()
        .find(|feature| feature.id == width.owner.clone().expect("Width owner"))
        .expect("source feature");
    let target = features
        .iter()
        .find(|feature| feature.id == depth.owner.clone().expect("Depth owner"))
        .expect("target feature");
    assert_eq!(
        target.dependencies.as_slice(),
        std::slice::from_ref(&source.id)
    );
}

#[test]
fn retains_parameter_when_owner_frame_has_no_scope_binding() {
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter#7".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 7,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "AlongDistance".into(),
                Some(8),
                None,
            )
            .unwrap(),
            expression: "12.5 mm".into(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: "distance".into(),
            name_offset: 80,
            evaluated_value: 1.25,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#9",
        crate::records::feature::scope::DesignFeatureKind::try_from("Unsupported".to_owned())
            .expect("native family name"),
        9,
    );

    let crate::design::feature_project::ProjectedParameterDesign { parameters, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = &[scope];
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    native: &[parameter],
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });

    let [parameter] = parameters.as_slice() else {
        panic!("expected one retained parameter");
    };
    assert_eq!(parameter.owner, None);
    assert_eq!(
        parameter
            .properties
            .get("owner_record_index")
            .map(String::as_str),
        Some("8")
    );
    assert_eq!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(12.5).unwrap()
        ))
    );
}
