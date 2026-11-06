// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn external_version_identity_preserves_wire_and_rejects_partial_forms() {
    {
        let prefix = r#"{"axis_record_index":0,"axis_class_tag":"327","axis_byte_offset":0,"axis_paired_class_tag":"327","axis_paired_byte_offset":0,"selector_record_index":0,"selector_class_tag":"327","selector_byte_offset":0,"selector_paired_class_tag":"327","selector_paired_byte_offset":0,"nested_record_index":0,"nested_record_index_offset":0,"selector_asset_id":"00000004-1111-4111-8111-111111111111","selector_asset_id_offset":0,"selector_context_id":"00000005-1111-4111-8111-111111111111","selector_context_id_offset":0,"occurrence_reference":0,"occurrence_reference_offset":0,"external_object_reference":0,"external_object_reference_offset":0,"external_segment":0,"external_segment_offset":0,"external_asset_id":"00000006-1111-4111-8111-111111111111","external_asset_id_offset":0,"external_link_name":"identity","external_link_name_offset":0"#;
        let suffix = r#","role_record_index":0,"role_class_tag":"327","role_byte_offset":0,"occurrence_role":"00000007-1111-4111-8111-111111111111","occurrence_role_offset":0}"#;
        let fields = [
            (
                "external_property_key",
                "\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"",
            ),
            ("external_property_key_offset", "100"),
            ("external_version_urn", "\"urn\""),
            ("external_version_urn_offset", "110"),
        ];
        for mask in 0..16 {
            let mut wire = prefix.to_owned();
            for (index, (field, value)) in fields.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    wire.push_str(&format!(",\"{field}\":{value}"));
                }
            }
            wire.push_str(suffix);
            let result = serde_json::from_str::<
                crate::records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
            >(&wire);
            if mask == 0 || mask == 15 {
                let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
                assert_relaxed_guid_fields::<
                    crate::records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
                >(
                    &value,
                    &[
                        "selector_asset_id",
                        "selector_context_id",
                        "external_asset_id",
                        "occurrence_role",
                    ],
                    |_, _| {},
                );
                if mask == 15 {
                    assert_relaxed_guid_fields::<
                        crate::records::feature::assembly::DesignAssemblyAxialSelectorIdentity,
                    >(&value, &["external_property_key"], |_, _| {});
                }
                assert_eq!(
                    serde_json::to_string(&result.expect("complete version form"))
                        .expect("version wire"),
                    wire
                );
            } else {
                let error = result.expect_err("partial version identity").to_string();
                for (field, _) in fields {
                    assert!(error.contains(field));
                }
            }
        }
    }
    {
        let prefix = r#"{"selector_asset_id":"00000004-1111-4111-8111-111111111111","selector_asset_id_offset":44,"selector_context_id":"00000005-1111-4111-8111-111111111111","selector_context_id_offset":120,"occurrence_reference":1,"occurrence_reference_offset":205,"external_body_reference":2,"external_body_reference_offset":220,"external_segment":0,"external_segment_offset":229,"external_asset_id":"00000004-1111-4111-8111-111111111111","external_asset_id_offset":237,"external_link_name":"identity","external_link_name_offset":314"#;
        let suffix = r#","tail_values":[0,0],"tail_value_offsets":[337,349]}"#;
        let fields = [
            (
                "external_property_key",
                "\"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\"",
            ),
            ("external_property_key_offset", "335"),
            ("external_version_urn", "\"urn\""),
            ("external_version_urn_offset", "411"),
        ];
        for mask in 0..16 {
            let mut wire = prefix.to_owned();
            for (index, (field, value)) in fields.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    wire.push_str(&format!(",\"{field}\":{value}"));
                }
            }
            wire.push_str(&if mask == 15 {
                suffix.replace("[337,349]", "[423,435]")
            } else {
                suffix.to_owned()
            });
            let result = serde_json::from_str::<
                crate::records::feature::combine::DesignCombineExternalBodyIdentity,
            >(&wire);
            if mask == 0 || mask == 15 {
                let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
                assert_relaxed_guid_fields::<
                    crate::records::feature::combine::DesignCombineExternalBodyIdentity,
                >(
                    &value,
                    &[
                        "selector_asset_id",
                        "selector_context_id",
                        "external_asset_id",
                    ],
                    complete_combine_field_edit,
                );
                if mask == 15 {
                    assert_relaxed_guid_fields::<
                        crate::records::feature::combine::DesignCombineExternalBodyIdentity,
                    >(
                        &value,
                        &["external_property_key"],
                        complete_combine_field_edit,
                    );
                }
                assert_eq!(
                    serde_json::to_string(&result.expect("complete version form"))
                        .expect("version wire"),
                    wire
                );
            } else {
                let error = result.expect_err("partial version identity").to_string();
                for (field, _) in fields {
                    assert!(error.contains(field));
                }
            }
        }
    }
}

#[test]
fn component_placement_preserves_wire_and_rejects_partial_location() {
    let base = serde_json::json!({
        "id": "occurrence", "class_tag": "327", "record_index": 7, "byte_offset": 0,
        "component_record_index": 8, "component_guid": "00000001-1111-4111-8111-111111111111", "component_guid_offset": 48,
        "occurrence_guid": "00000002-1111-4111-8111-111111111111", "occurrence_guid_offset": 124, "occurrence_ordinal": 1,
        "owner_entity": 9, "owner_entity_offset": 219
    });
    assert_relaxed_guid_fields::<
        crate::records::feature::assembly_features::DesignComponentOccurrence,
    >(&base, &["component_guid", "occurrence_guid"], |_, _| {});
    let transform = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ]);
    for placed in [false, true] {
        let mut wire = base.clone();
        if placed {
            wire["transform"] = transform.clone();
            wire["transform_offset"] = serde_json::json!(209);
            wire["owner_entity_offset"] = serde_json::json!(347);
        }
        let record: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
    for (field, value) in [
        ("transform", transform),
        ("transform_offset", serde_json::json!(209)),
    ] {
        let mut wire = base.clone();
        wire[field] = value;
        assert!(serde_json::from_value::<
            crate::records::feature::assembly_features::DesignComponentOccurrence,
        >(wire)
        .unwrap_err()
        .to_string()
        .contains("transform"));
    }
}

#[test]
fn assembly_forms_preserve_partial_and_mixed_qualifier_wire() {
    let frame = crate::records::feature::assembly::DesignAssemblyOperandFrame {
        reference_record_index: 10,
        reference_offset: 11,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
        transform_offset: 22,
    };
    let path = crate::records::feature::assembly::DesignAssemblyOperandPath::try_new(
        crate::records::feature::assembly::DesignAssemblyOperandPathLink {
            locator_reference_offset: 11,
            locator_record_index: 10,
            locator_class_tag: crate::records::references::DesignClassTag::try_from(
                "363".to_owned(),
            )
            .unwrap(),
            locator_byte_offset: 100,
            locator_scope_reference_offset: 111,
            wrapper_record_index: 20,
            wrapper_reference_offset: 122,
            wrapper_class_tag: crate::records::references::DesignClassTag::try_from(
                "388".to_owned(),
            )
            .unwrap(),
            wrapper_byte_offset: 200,
            path_reference_offset: 211,
        },
        30,
        crate::records::references::DesignClassTag::try_from("386".to_owned()).unwrap(),
        300,
        vec![crate::records::identity::Located {
            value: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .expect("GUID"),
            offset: 311,
        }],
        vec![crate::records::identity::Located {
            value: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .expect("GUID"),
            offset: 322,
        }],
    )
    .unwrap();
    let limits: crate::records::feature::assembly::DesignAssemblyLimits =
        crate::records::feature::assembly::DesignAssemblyLimitsWire {
            kind: crate::records::feature::assembly::DesignAssemblyLimitKind::Angular,
            minimum: -1.0,
            maximum: 1.0,
            owner_record_indices: [40, 50],
            value_offsets: [411, 511],
        }
        .try_into()
        .unwrap();
    let joint_origin =
        crate::records::feature::assembly::DesignAssemblyOperandQualifier::JointOrigin {
            scope_record_index: 60,
            class_tag: crate::records::references::DesignClassTag::try_from("307".to_owned())
                .unwrap(),
            byte_offset: 600,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "264".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 700,
        };
    let axial = crate::records::feature::assembly::DesignAssemblyOperandQualifier::AxialTarget {
        target:
            crate::records::feature::assembly::DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
                scope_record_index: 60,
            },
    };
    let occurrence =
        crate::records::feature::assembly::DesignAssemblyOperandQualifier::OccurrencePath {
            path: path.clone(),
        };
    for form in [
        None,
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                joint_origin_scope_record_index: 60,
            },
        ),
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::SolvedOnly {
                solved_frame: crate::records::feature::assembly::DesignAssemblySolvedFrame {
                    reference_record_index: 30,
                    reference_offset: 33,
                    record_byte_offset: 300,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "258".to_owned(),
                    )
                    .unwrap(),
                    transform: frame.transform,
                    transform_offset: 325,
                },
                limits: Some(limits.clone()),
            },
        ),
        Some(crate::records::feature::assembly::DesignAssemblyAlignmentForm::LimitsOnly { limits }),
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::Frames {
                frames: [frame.clone(), frame.clone()],
            },
        ),
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::qualified(
                [frame.clone(), frame.clone()],
                [occurrence.clone(), occurrence.clone()],
            ),
        ),
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::qualified(
                [frame.clone(), frame.clone()],
                [occurrence, joint_origin],
            ),
        ),
        Some(
            crate::records::feature::assembly::DesignAssemblyAlignmentForm::qualified(
                [frame.clone(), frame.clone()],
                [axial.clone(), axial],
            ),
        ),
    ] {
        let alignment = crate::records::feature::assembly::DesignAssemblyAlignment::try_new(
            0.0,
            [0.0; 3],
            vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 11,
                },
                crate::records::identity::Located {
                    value: 20,
                    offset: 22,
                },
            ],
            form,
        )
        .unwrap();
        let wire = serde_json::to_string(&alignment).unwrap();
        let decoded: crate::records::feature::assembly::DesignAssemblyAlignment =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded, alignment);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        if alignment.operand_paths().is_some() {
            let mut unframed = serde_json::from_str::<serde_json::Value>(&wire).unwrap();
            unframed.as_object_mut().unwrap().remove("operand_frames");
            let error = serde_json::from_value::<
                crate::records::feature::assembly::DesignAssemblyAlignment,
            >(unframed)
            .unwrap_err();
            assert!(error.to_string().contains("operand_frames"));
        }
        let mut invalid = serde_json::from_str::<serde_json::Value>(&wire).unwrap();
        invalid["value_offsets"] = serde_json::json!([11]);
        let error = serde_json::from_value::<
            crate::records::feature::assembly::DesignAssemblyAlignment,
        >(invalid)
        .unwrap_err()
        .to_string();
        assert!(error.contains("owner_record_indices"));
        assert!(error.contains("value_offsets"));
    }
}

#[test]
fn legacy_assembly_wire_derives_carrier_frames_and_checks_repeated_fields() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selection =
        |record_index| crate::records::feature::assembly::DesignAssemblyLegacySelection {
            record_index,
            byte_offset: 400,
            class_tag: crate::records::references::DesignClassTag::try_from("307".to_owned())
                .unwrap(),
            asset_id: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 411,
            context_id: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 422,
            recipe_record_index: 50,
            recipe_record_byte_offset: 500,
            recipe_id: "recipe".into(),
            recipe_kind: crate::records::recipes::ConstructionRecipeKind::Face,
            recipe_references: Vec::new(),
            next_byte_offset: 600,
        };
    let carriers = crate::records::feature::assembly::DesignAssemblyLegacyOperands::new(
        crate::records::feature::assembly::DesignAssemblyLegacyOperand {
            construction_class_tag: crate::records::references::DesignClassTag::try_from(
                "256".to_owned(),
            )
            .unwrap(),
            reference_offset: 11,
            construction: Box::new(
                crate::records::feature::work_geometry::DesignWorkPointConstruction {
                    point_record_index: 10,
                    point_record_byte_offset: 100,
                    position: crate::test_support::reals([1.0, 2.0, 3.0]),
                    position_offset: 125,
                    rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                        crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native {
                            reference_type: 0,
                            inputs: Vec::new(),
                        },
                    )
                    .expect("compatible WorkPoint rule"),
                    reference_type_offset: 150,
                },
            ),
            selection: selection(40),
        },
        crate::records::feature::assembly::DesignAssemblyLegacyOperand {
            construction_class_tag: crate::records::references::DesignClassTag::try_from(
                "257".to_owned(),
            )
            .unwrap(),
            reference_offset: 22,
            construction: Box::new(crate::records::feature::hole::DesignHoleConstruction {
                point_record_index: 20,
                point_record_byte_offset: 200,
                position: crate::test_support::reals([4.0, 5.0, 6.0]),
                position_offset: 225,
                direction: crate::test_support::reals([0.0, 0.0, 1.0]),
                direction_offset: 250,
                point_parameters: crate::test_support::reals([0.0, 0.0]),
                point_parameter_offsets: [275, 283],
                reference_type: 0,
                reference_type_offset: 291,
                tangent_point_data: None,
                input_records: Vec::new(),
                face_selection: None,
                locations: Vec::new(),
            }),
            selection: selection(41),
        },
    );
    let solved_frame = crate::records::feature::assembly::DesignAssemblySolvedFrame {
        reference_record_index: 30,
        reference_offset: 33,
        record_byte_offset: 300,
        class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap(),
        transform: identity.try_into().unwrap(),
        transform_offset: 325,
    };
    for frames_field_present in [false, true] {
        let alignment = crate::records::feature::assembly::DesignAssemblyAlignment::try_new(
            0.0,
            [0.0; 3],
            Vec::new(),
            Some(
                crate::records::feature::assembly::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
                    carriers: carriers.clone(),
                    solved_frame: solved_frame.clone(),
                    limits: None,
                    frames_field_present,
                },
            ),
        )
        .unwrap();
        let frames = alignment.operand_frames().unwrap();
        assert_eq!(frames[0].reference_record_index, 10);
        assert_eq!(frames[1].reference_record_index, 20);
        assert_eq!(frames[0].transform_offset, 325);
        assert_eq!(
            frames[0].transform,
            [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0],
                [0.0, 0.0, 0.0, 1.0]
            ]
            .try_into()
            .unwrap()
        );
        assert_eq!(frames[1].transform[2][3], 6.0);
        let wire = serde_json::to_string(&alignment).unwrap();
        let decoded: crate::records::feature::assembly::DesignAssemblyAlignment =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded, alignment);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(value.get("operand_frames").is_some(), frames_field_present);
        for (field, replacement) in [
            ("construction_record_index", serde_json::json!(99)),
            ("construction_byte_offset", serde_json::json!(99)),
            ("frame", serde_json::to_value(&frames[1]).unwrap()),
        ] {
            let mut invalid = value.clone();
            invalid["legacy_operand_carriers"][0][field] = replacement;
            let error = serde_json::from_value::<
                crate::records::feature::assembly::DesignAssemblyAlignment,
            >(invalid)
            .unwrap_err()
            .to_string();
            assert!(error.contains(field));
        }
        let mut invalid = value.clone();
        invalid["legacy_operand_carriers"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(
            serde_json::from_value::<crate::records::feature::assembly::DesignAssemblyAlignment>(
                invalid
            )
            .unwrap_err()
            .to_string()
            .contains("construction")
        );
        if frames_field_present {
            let mut invalid = value;
            invalid["operand_frames"][0]["transform"][0][3] = serde_json::json!(99.0);
            assert!(serde_json::from_value::<
                crate::records::feature::assembly::DesignAssemblyAlignment,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains("operand_frames"));
        }
    }
}

#[test]
fn assembly_path_wire_pairs_guid_locations() {
    for count in [1, 3] {
        let values: Vec<_> = (0..count)
            .map(|index| format!("{index:08}-1111-4111-8111-111111111111"))
            .collect();
        let offsets: Vec<_> = (0..count).map(|index| 400 + index * 80).collect();
        let wire = serde_json::json!({
            "link": {
                "locator_reference_offset": 11, "locator_record_index": 10,
                "locator_class_tag": "363", "locator_byte_offset": 100,
                "locator_scope_reference_offset": 111, "wrapper_record_index": 20,
                "wrapper_reference_offset": 122, "wrapper_class_tag": "388",
                "wrapper_byte_offset": 200, "path_reference_offset": 211
            },
            "record_index": 30, "class_tag": "329", "byte_offset": 300,
            "occurrence_guids": values, "occurrence_guid_offsets": offsets
        });
        for identities in [false, true] {
            let mut wire = wire.clone();
            if identities {
                wire["identity_guids"] = serde_json::json!(vec![values[0].clone(); 4]);
                wire["identity_guid_offsets"] = serde_json::json!([700, 780, 860, 940]);
            }
            let path: crate::records::feature::assembly::DesignAssemblyOperandPath =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(path.occurrence_guids().len(), count);
            assert_eq!(serde_json::to_value(&path).unwrap(), wire);
            for (value_field, offset_field) in [
                ("occurrence_guids", "occurrence_guid_offsets"),
                ("identity_guids", "identity_guid_offsets"),
            ] {
                let mut invalid = wire.clone();
                let mut bad_offsets = invalid
                    .get(offset_field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                bad_offsets.push(serde_json::json!(999));
                invalid[offset_field] = serde_json::Value::Array(bad_offsets);
                let error = serde_json::from_value::<
                    crate::records::feature::assembly::DesignAssemblyOperandPath,
                >(invalid)
                .unwrap_err()
                .to_string();
                assert!(error.contains(value_field));
                assert!(error.contains(offset_field));
            }
        }
    }
}

fn borrowed_assembly_path(with_identity: bool) -> super::DesignAssemblyOperandPath {
    let mut wire = serde_json::json!({
        "link": {
            "locator_reference_offset": 11, "locator_record_index": 10,
            "locator_class_tag": "363", "locator_byte_offset": 100,
            "locator_scope_reference_offset": 111, "wrapper_record_index": 20,
            "wrapper_reference_offset": 122, "wrapper_class_tag": "388",
            "wrapper_byte_offset": 200, "path_reference_offset": 211
        },
        "record_index": 30, "class_tag": "329", "byte_offset": 300,
        "occurrence_guids": ["11111111-1111-4111-8111-111111111111"],
        "occurrence_guid_offsets": [400]
    });
    if with_identity {
        wire["identity_guids"] = serde_json::json!([
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
            "33333333-3333-4333-8333-333333333333",
            "44444444-4444-4444-8444-444444444444"
        ]);
        wire["identity_guid_offsets"] = serde_json::json!([500, 580, 660, 740]);
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn assembly_operand_path_borrowed_wire_matches_owned_wire_bytes() {
    for path in [borrowed_assembly_path(false), borrowed_assembly_path(true)] {
        let owned = super::DesignAssemblyOperandPathWire::from(path.clone());
        assert_eq!(
            serde_json::to_vec(&path).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn assembly_operand_path_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct PathRecord<'a> {
        id: &'static str,
        path: &'a super::DesignAssemblyOperandPath,
    }
    let path = borrowed_assembly_path(true);
    let record = PathRecord {
        id: "f3d:native:assembly-operand-path#0",
        path: &path,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::ASSEMBLY_OPERAND_PATH_CLONE_COUNT.with(|count| count.set(0)),
        || super::ASSEMBLY_OPERAND_PATH_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn component_insert_pairs_explicit_matrix_with_scope_and_carrier_locations() {
    let prefix = r#"{"relation_record_index":1,"carrier_record_index":2,"neutron_role":"role","neutron_role_offset":30,"transform":"#;
    let identity = "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    let translated = "[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]";
    for (matrix, offsets) in [
        (identity, ""),
        (identity, ",\"transform_offset\":50"),
        (translated, ",\"transform_offset\":50"),
        (
            translated,
            ",\"transform_offset\":50,\"carrier_transform_offset\":40",
        ),
    ] {
        let wire = format!("{prefix}{matrix}{offsets}}}");
        let construction: crate::records::feature::assembly_features::DesignComponentInsertConstruction =
            serde_json::from_str(&wire).expect("component placement");
        assert_eq!(
            serde_json::to_string(&construction).expect("component placement wire"),
            wire
        );
        assert_eq!(construction.placement.is_some(), !offsets.is_empty());
    }
    for (matrix, offsets) in [
        (translated, ""),
        (identity, ",\"carrier_transform_offset\":40"),
    ] {
        let wire = format!("{prefix}{matrix}{offsets}}}");
        let error = serde_json::from_str::<
            crate::records::feature::assembly_features::DesignComponentInsertConstruction,
        >(&wire)
        .expect_err("missing scope matrix location");
        assert!(error.to_string().contains("transform_offset"));
    }
}

#[test]
fn component_occurrence_derives_base_ordinal_and_requires_nonzero_placed_ordinal() {
    let prefix = r#"{"id":"occurrence","class_tag":"327","record_index":7,"byte_offset":0,"component_record_index":8,"component_guid":"00000001-1111-4111-8111-111111111111","component_guid_offset":48,"occurrence_guid":"00000002-1111-4111-8111-111111111111","occurrence_guid_offset":124,"occurrence_ordinal":"#;
    let matrix = r#","transform":[[1.0,0.0,0.0,2.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":209"#;
    let owner = |placed| {
        format!(
            r#","owner_entity":5,"owner_entity_offset":{}"#,
            if placed { 347 } else { 219 }
        )
    };
    for (ordinal, placed) in [(1, false), (1, true), (2, true), (u32::MAX, true)] {
        let suffix = if placed { matrix } else { "" };
        let wire = format!("{prefix}{ordinal}{suffix}{}}}", owner(placed));
        let occurrence: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_str(&wire).expect("component occurrence");
        assert_eq!(
            serde_json::to_string(&occurrence).expect("component occurrence wire"),
            wire
        );
        assert_eq!(occurrence.occurrence_ordinal(), ordinal);
    }
    for (ordinal, placed) in [(0, false), (0, true), (2, false), (u32::MAX, false)] {
        let suffix = if placed { matrix } else { "" };
        let wire = format!("{prefix}{ordinal}{suffix}{}}}", owner(placed));
        let error = serde_json::from_str::<
            crate::records::feature::assembly_features::DesignComponentOccurrence,
        >(&wire)
        .expect_err("invalid occurrence ordinal");
        assert!(error.to_string().contains("occurrence_ordinal"));
    }
}

fn assert_relaxed_guid_fields<T: serde::de::DeserializeOwned + serde::Serialize>(
    wire: &serde_json::Value,
    fields: &[&str],
    complete_edit: impl Fn(&mut serde_json::Value, &str),
) {
    for field in fields {
        for guid in ["g".repeat(36), "_".repeat(38), "invalid".into()] {
            let mut changed = wire.clone();
            changed[field] = serde_json::json!(guid);
            complete_edit(&mut changed, field);
            let decoded = serde_json::from_value::<T>(changed.clone());
            if guid == "invalid" {
                assert!(decoded.is_err(), "{field}");
            } else {
                let decoded = decoded.unwrap_or_else(|error| panic!("{field}: {error}"));
                assert_eq!(serde_json::to_value(decoded).unwrap(), changed);
            }
        }
    }
}

#[test]
fn assembly_numeric_admission_preserves_signed_values_and_rejects_invalid_bounds() {
    use crate::records::feature::assembly::{
        DesignAssemblyAlignment, DesignAssemblyLimitKind, DesignAssemblyLimits,
        DesignAssemblyLimitsWire,
    };
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(DesignAssemblyAlignment::try_new(invalid, [0.0; 3], Vec::new(), None).is_err());
        for lane in 0..3 {
            let mut offset = [0.0; 3];
            offset[lane] = invalid;
            assert!(DesignAssemblyAlignment::try_new(0.0, offset, Vec::new(), None).is_err());
        }
    }
    let alignment =
        DesignAssemblyAlignment::try_new(-1.0, [-2.0, -0.0, 3.0], Vec::new(), None).unwrap();
    let json = serde_json::to_value(&alignment).unwrap();
    assert_eq!(
        serde_json::from_value::<DesignAssemblyAlignment>(json).unwrap(),
        alignment
    );
    for (minimum, maximum, valid) in [
        (-1.0, 2.0, true),
        (0.0, 0.0, true),
        (2.0, 1.0, false),
        (f64::NAN, 1.0, false),
        (0.0, f64::INFINITY, false),
        (f64::NEG_INFINITY, 0.0, false),
    ] {
        let wire = DesignAssemblyLimitsWire {
            kind: DesignAssemblyLimitKind::Angular,
            minimum,
            maximum,
            owner_record_indices: [1, 2],
            value_offsets: [10, 20],
        };
        assert_eq!(DesignAssemblyLimits::try_from(wire.clone()).is_ok(), valid);
        if minimum.is_finite() && maximum.is_finite() {
            let json = serde_json::to_value(wire).unwrap();
            let result = serde_json::from_value::<DesignAssemblyLimits>(json.clone());
            assert_eq!(result.is_ok(), valid);
            if let Ok(limits) = result {
                assert_eq!(serde_json::to_value(limits).unwrap(), json);
            }
        }
    }
}

#[test]
fn assembly_alignment_native_writer_refuses_retained_limit_before_record_completion() {
    use crate::records::feature::assembly::DesignAssemblyAlignment;
    use crate::records::identity::Located;

    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        alignment: &'a DesignAssemblyAlignment,
    }

    let alignment = DesignAssemblyAlignment::try_new(
        1.0,
        [0.0; 3],
        vec![Located {
            value: 7,
            offset: 9,
        }],
        None,
    )
    .unwrap();
    let record = Record {
        id: "f3d:design:alignment#1",
        alignment: &alignment,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({
            "id": record.id,
            "alignment": {
                "angle": 1.0,
                "offset": [0.0, 0.0, 0.0],
                "owner_record_indices": [7],
                "value_offsets": [9],
            }
        }),
    );
}

#[test]
fn assembly_path_admission_checks_class_arity_and_guid_order() {
    use crate::records::feature::assembly::{
        DesignAssemblyOperandPath, DesignAssemblyOperandPathLink,
    };
    let link: DesignAssemblyOperandPathLink = serde_json::from_value(serde_json::json!({
        "locator_reference_offset": 11, "locator_record_index": 10,
        "locator_class_tag": "451", "locator_byte_offset": 100,
        "locator_scope_reference_offset": 111, "wrapper_record_index": 20,
        "wrapper_reference_offset": 122, "wrapper_class_tag": "369",
        "wrapper_byte_offset": 200, "path_reference_offset": 211
    }))
    .unwrap();
    for tag in ["294", "299", "307", "329", "330", "386", "390"] {
        for count in [0, 1, 3, 4, 8] {
            let offsets: Vec<_> = (0..count).map(|index| 500 + index * 80).collect();
            let valid = match tag {
                "329" => count == 0 || count == 4,
                "330" => count == 4 || count == 8,
                _ => count == 4,
            };
            let result = DesignAssemblyOperandPath::try_new(
                link.clone(),
                30,
                tag.to_owned().try_into().unwrap(),
                300,
                guids(&[400]),
                guids(&offsets),
            );
            assert_eq!(result.is_ok(), valid);
        }
    }
    let path = DesignAssemblyOperandPath::try_new(
        link,
        30,
        "390".to_owned().try_into().unwrap(),
        300,
        guids(&[400, 480]),
        guids(&[560, 640, 720, 800]),
    )
    .unwrap();
    let wire = serde_json::to_value(path).unwrap();
    assert!(serde_json::from_value::<DesignAssemblyOperandPath>(wire.clone()).is_ok());
    for (field, offsets) in [
        ("occurrence_guid_offsets", vec![300, 480]),
        ("occurrence_guid_offsets", vec![480, 400]),
        ("identity_guid_offsets", vec![560, 560, 720, 800]),
        ("identity_guid_offsets", vec![299, 640, 720, 800]),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = serde_json::json!(offsets);
        assert!(serde_json::from_value::<DesignAssemblyOperandPath>(invalid).is_err());
    }
    let mut empty = wire.clone();
    empty["occurrence_guids"] = serde_json::json!([]);
    empty["occurrence_guid_offsets"] = serde_json::json!([]);
    assert!(serde_json::from_value::<DesignAssemblyOperandPath>(empty).is_err());
    let mut wrong_class = wire;
    wrong_class["link"]["locator_class_tag"] = serde_json::json!("363");
    wrong_class["class_tag"] = serde_json::json!("386");
    assert!(serde_json::from_value::<DesignAssemblyOperandPath>(wrong_class).is_err());
}

fn complete_combine_field_edit(wire: &mut serde_json::Value, field: &str) {
    if field == "external_asset_id" {
        wire["selector_asset_id"] = wire[field].clone();
    }
    if field == "selector_asset_id" {
        wire["external_asset_id"] = wire[field].clone();
    }
    let end = |wire: &serde_json::Value, value: &str, offset: &str| {
        wire[offset].as_u64().unwrap()
            + 2 * u64_from_index(wire[value].as_str().unwrap().encode_utf16().count())
    };
    wire["selector_context_id_offset"] =
        serde_json::json!(end(wire, "selector_asset_id", "selector_asset_id_offset") + 4);
    wire["occurrence_reference_offset"] =
        serde_json::json!(end(wire, "selector_context_id", "selector_context_id_offset") + 13);
    wire["external_body_reference_offset"] =
        serde_json::json!(wire["occurrence_reference_offset"].as_u64().unwrap() + 15);
    wire["external_segment_offset"] =
        serde_json::json!(wire["external_body_reference_offset"].as_u64().unwrap() + 9);
    wire["external_asset_id_offset"] =
        serde_json::json!(wire["external_segment_offset"].as_u64().unwrap() + 8);
    wire["external_link_name_offset"] =
        serde_json::json!(end(wire, "external_asset_id", "external_asset_id_offset") + 5);
    let link_end = end(wire, "external_link_name", "external_link_name_offset");
    let tail = if wire.get("external_property_key").is_some() {
        wire["external_property_key_offset"] = serde_json::json!(link_end + 5);
        wire["external_version_urn_offset"] = serde_json::json!(
            end(
                wire,
                "external_property_key",
                "external_property_key_offset"
            ) + 4
        );
        end(wire, "external_version_urn", "external_version_urn_offset") + 6
    } else {
        link_end + 7
    };
    wire["tail_value_offsets"] = serde_json::json!([tail, tail + 12]);
}

#[test]
fn component_occurrence_derives_guid_and_transform_offsets_at_admission() {
    use crate::records::feature::assembly_features::{
        DesignComponentOccurrence as Occurrence, DesignComponentOccurrenceDraft as Draft,
        DesignComponentOccurrencePlacement as Placement,
    };
    let draft = |base, placement| Draft {
        id: "occurrence".into(),
        class_tag: "327".to_owned().try_into().unwrap(),
        record_index: 7,
        byte_offset: base,
        component_record_index: 8,
        component_guid: "00000001-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .unwrap(),
        occurrence_guid: "00000002-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .unwrap(),
        placement,
        owner_entity: 9,
    };
    let matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (placement, last_offset) in [
        (Placement::Base, 219),
        (
            Placement::Explicit {
                ordinal: std::num::NonZeroU32::MIN,
                transform: matrix.try_into().unwrap(),
            },
            347,
        ),
    ] {
        let occurrence = Occurrence::try_new(draft(100, placement)).unwrap();
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(wire["component_guid_offset"], 148);
        assert_eq!(wire["occurrence_guid_offset"], 224);
        assert_eq!(wire["owner_entity"], 9);
        assert_eq!(wire["owner_entity_offset"], 100 + last_offset);
        assert_eq!(
            serde_json::from_value::<Occurrence>(wire.clone()).unwrap(),
            occurrence
        );
        for field in [
            "component_guid_offset",
            "occurrence_guid_offset",
            "transform_offset",
            "owner_entity_offset",
        ] {
            if field == "transform_offset" && matches!(placement, Placement::Base) {
                continue;
            }
            let mut invalid = wire.clone();
            invalid[field] = 0.into();
            assert!(serde_json::from_value::<Occurrence>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let boundary = Occurrence::try_new(draft(u64::MAX - last_offset, placement)).unwrap();
        assert!(
            serde_json::from_value::<Occurrence>(serde_json::to_value(boundary).unwrap()).is_ok()
        );
        assert!(Occurrence::try_new(draft(u64::MAX - last_offset + 1, placement)).is_err());
    }
}

fn axial_selector_for_serialization(version: bool) -> super::DesignAssemblyAxialSelectorIdentity {
    let prefix = r#"{"axis_record_index":0,"axis_class_tag":"327","axis_byte_offset":0,"axis_paired_class_tag":"327","axis_paired_byte_offset":0,"selector_record_index":0,"selector_class_tag":"327","selector_byte_offset":0,"selector_paired_class_tag":"327","selector_paired_byte_offset":0,"nested_record_index":0,"nested_record_index_offset":0,"selector_asset_id":"00000004-1111-4111-8111-111111111111","selector_asset_id_offset":0,"selector_context_id":"00000005-1111-4111-8111-111111111111","selector_context_id_offset":0,"occurrence_reference":0,"occurrence_reference_offset":0,"external_object_reference":0,"external_object_reference_offset":0,"external_segment":0,"external_segment_offset":0,"external_asset_id":"00000006-1111-4111-8111-111111111111","external_asset_id_offset":0,"external_link_name":"identity","external_link_name_offset":0"#;
    let suffix = r#","role_record_index":0,"role_class_tag":"327","role_byte_offset":0,"occurrence_role":"00000007-1111-4111-8111-111111111111","occurrence_role_offset":0}"#;
    let version_fields = if version {
        r#", "external_property_key":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","external_property_key_offset":100,"external_version_urn":"urn","external_version_urn_offset":110"#
    } else {
        ""
    };
    serde_json::from_str(&format!("{prefix}{version_fields}{suffix}")).unwrap()
}

#[test]
fn assembly_axial_selector_borrowed_wire_matches_owned_wire_bytes() {
    for selector in [
        axial_selector_for_serialization(false),
        axial_selector_for_serialization(true),
    ] {
        let owned = super::DesignAssemblyAxialSelectorIdentityWire::from(selector.clone());
        assert_eq!(
            serde_json::to_vec(&selector).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn assembly_axial_selector_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignAssemblyAxialSelectorIdentity,
    }
    let selector = axial_selector_for_serialization(true);
    let record = NestedRecord {
        id: "f3d:native:assembly-axial-selector#0",
        value: &selector,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::ASSEMBLY_AXIAL_SELECTOR_CLONE_COUNT.with(|count| count.set(0)),
        || super::ASSEMBLY_AXIAL_SELECTOR_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn native_legacy_alignment_wire_rewrite_matches_the_typed_selection_walk() {
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let selection = |record_index| {
        crate::records::feature::assembly::DesignAssemblyLegacySelection {
            record_index,
            byte_offset: 400,
            class_tag: crate::records::references::DesignClassTag::try_from("307".to_owned())
                .unwrap(),
            asset_id: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 411,
            context_id: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 422,
            recipe_record_index: 50,
            recipe_record_byte_offset: 500,
            recipe_id: "recipe".into(),
            recipe_kind: crate::records::recipes::ConstructionRecipeKind::Face,
            recipe_references: vec![serde_json::from_value(serde_json::json!({
                "selector": 1, "selector_offset": 1, "token": "f3d:model:face#one", "token_offset": 2,
                "design_reference": 1, "design_reference_offset": 3,
                "candidate_faces": ["f3d:model:face#one"], "candidate_edges": ["f3d:model:edge#one"],
                "alternate_selector_faces": ["f3d:model:face#one"], "alternate_selector_edges": ["f3d:model:edge#one"]
            })).unwrap()],
            next_byte_offset: 600,
        }
    };
    let carriers = crate::records::feature::assembly::DesignAssemblyLegacyOperands::new(
        crate::records::feature::assembly::DesignAssemblyLegacyOperand {
            construction_class_tag: crate::records::references::DesignClassTag::try_from(
                "256".to_owned(),
            )
            .unwrap(),
            reference_offset: 11,
            construction: Box::new(
                crate::records::feature::work_geometry::DesignWorkPointConstruction {
                    point_record_index: 10,
                    point_record_byte_offset: 100,
                    position: crate::test_support::reals([1.0, 2.0, 3.0]),
                    position_offset: 125,
                    rule: crate::records::feature::work_geometry::DesignWorkPointRule::try_from(
                        crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native {
                            reference_type: 0,
                            inputs: Vec::new(),
                        },
                    )
                    .expect("compatible WorkPoint rule"),
                    reference_type_offset: 150,
                },
            ),
            selection: selection(40),
        },
        crate::records::feature::assembly::DesignAssemblyLegacyOperand {
            construction_class_tag: crate::records::references::DesignClassTag::try_from(
                "257".to_owned(),
            )
            .unwrap(),
            reference_offset: 22,
            construction: Box::new(crate::records::feature::hole::DesignHoleConstruction {
                point_record_index: 20,
                point_record_byte_offset: 200,
                position: crate::test_support::reals([4.0, 5.0, 6.0]),
                position_offset: 225,
                direction: crate::test_support::reals([0.0, 0.0, 1.0]),
                direction_offset: 250,
                point_parameters: crate::test_support::reals([0.0, 0.0]),
                point_parameter_offsets: [275, 283],
                reference_type: 0,
                reference_type_offset: 291,
                tangent_point_data: None,
                input_records: Vec::new(),
                face_selection: None,
                locations: Vec::new(),
            }),
            selection: selection(41),
        },
    );
    let solved_frame = crate::records::feature::assembly::DesignAssemblySolvedFrame {
        reference_record_index: 30,
        reference_offset: 33,
        record_byte_offset: 300,
        class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap(),
        transform: identity.try_into().unwrap(),
        transform_offset: 325,
    };

    let alignment = super::DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        Vec::new(),
        Some(super::DesignAssemblyAlignmentForm::LegacyAsBuilt421 {
            carriers,
            solved_frame,
            limits: None,
            frames_field_present: true,
        }),
    )
    .unwrap();
    let mut value = serde_json::to_value(&alignment).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let remap = |id: &str| {
        Ok(format!(
            "f3d:occurrence:{}",
            id.strip_prefix("f3d:model:").unwrap()
        ))
    };
    let expected = cadmpeg_ir::schema::rewrite::identities(&ctx, alignment, remap).unwrap();
    let mut identities = IdentityMap::new(remap);
    super::DesignAssemblyAlignment::rewrite_native_value(&ctx, &mut value, &mut identities)
        .unwrap();
    identities.finish().unwrap();
    assert_eq!(value, serde_json::to_value(expected).unwrap());
    for operand in value["legacy_operand_carriers"].as_array().unwrap() {
        assert_eq!(
            operand["selection"]["recipe_references"][0]["candidate_faces"][0],
            "f3d:occurrence:face#one"
        );
        assert_eq!(
            operand["selection"]["recipe_references"][0]["token"],
            "f3d:model:face#one"
        );
    }
    drop(identities);
    ctx.finish_session().unwrap();
}

fn guids<T: TryFrom<String, Error = String>>(
    offsets: &[u64],
) -> Vec<crate::records::identity::Located<T>> {
    offsets
        .iter()
        .map(|offset| crate::records::identity::Located {
            value: T::try_from("11111111-1111-4111-8111-111111111111".to_owned()).unwrap(),
            offset: *offset,
        })
        .collect()
}

#[test]
fn root_operand_paths_refuse_continuations() {
    let path = |root, shift| {
        let mut wire = serde_json::to_value(borrowed_assembly_path(true)).unwrap();
        wire["type_guid"] = serde_json::json!(super::TYPED_OCCURRENCE_PATH_TYPE_GUID);
        wire["byte_offset"] = serde_json::json!(300 + shift);
        wire["occurrence_guid_offsets"] = serde_json::json!([400 + shift]);
        wire["identity_guid_offsets"] =
            serde_json::json!([500 + shift, 580 + shift, 660 + shift, 740 + shift]);
        if root {
            wire["occurrence_guids"] = serde_json::json!(["00000000-0000-0000-0000-000000000000"]);
            wire["identity_guids"].as_array_mut().unwrap().truncate(2);
            wire["identity_guid_offsets"]
                .as_array_mut()
                .unwrap()
                .truncate(2);
        }
        let path: super::DesignAssemblyOperandPath = serde_json::from_value(wire).unwrap();
        assert_eq!(path.is_root(), root);
        path
    };
    assert!(path(false, 0).try_append(path(false, 1000)).is_some());
    assert!(path(true, 0).try_append(path(true, 1000)).is_none());
    assert!(path(true, 0).try_append(path(false, 1000)).is_none());
    assert!(path(false, 0).try_append(path(true, 1000)).is_none());
}
