// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    crate::native::F3dNative {
        design_parameter_scopes: vec![DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        )],
        ..Default::default()
    }
}

#[test]
fn invalid_parameter_scope_preserves_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design parameter scope has an invalid paired frame"
        );
    })
}

#[test]
fn offset_faces_counted_member_runs_validate() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::ReferenceRun,
    };
    for count in [2, 8, 10] {
        for count_offset in [126, 130] {
            let mut scope = DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#17",
                DesignFeatureKind::OffsetFaces,
                17,
            );
            scope
                .try_edit(|draft| {
                    draft.reference_members = ReferenceRun::unlocated((301..301 + count).collect());
                    draft.reference_count_offset = count_offset;
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.kind_offset + 22 + 78;
                    draft.frame_length = draft.paired_byte_offset;
                    draft.feature_ordinal_offset = draft.kind_offset + 22;
                })
                .unwrap();
            if let crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(slot) =
                scope.payload_mut()
            {
                *slot = Some(
                    crate::records::feature::direct_face::DesignOffsetFacesOperation {
                        distance: cadmpeg_ir::scalar::FiniteReal::new(0.5).unwrap(),
                        distance_record_index: 300 + count,
                        distance_offset: u64::from(340 + count),
                    },
                );
            }
            let mut native = crate::native::F3dNative::default();
            for record_index in std::iter::once(17).chain(301..301 + count) {
                native.design_record_headers.push(DesignRecordHeader {
                    id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                    record_index,
                    class_tag: "256".to_owned().try_into().unwrap(),
                    byte_offset: u64::from(record_index),
                });
            }
            native.design_parameter_scopes.push(scope);
            let findings = |native: &crate::native::F3dNative| {
                crate::test_support::with_decode_context(|service_ctx| {
                    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
                    let ctx = super::super::Ctx::new(&ir, native, service_ctx);
                    let mut findings = Vec::new();
                    super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
                    findings
                })
            };
            if count < 3 {
                assert_eq!(findings(&native).len(), 1);
                continue;
            }
            assert!(findings(&native).is_empty(), "{count} {count_offset}");
            let mut changed = native.clone();
            if let crate::records::feature::scope::DesignScopePayloadMut::OffsetFaces(Some(
                operation,
            )) = changed.design_parameter_scopes[0].payload_mut()
            {
                operation.distance_record_index = 301;
            }
            assert_eq!(findings(&changed).len(), 1);
            native.design_record_headers.pop();
            assert_eq!(findings(&native).len(), 1);
        }
    }
}

#[test]
fn operand_list_combine_requires_its_own_offsets() {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::feature::{
        combine::{
            DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
            DesignCombineTools,
        },
        scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut},
    };
    use crate::records::identity::ReferenceRun;

    let findings = |operation_offset: u64, keep_tools_offset: u64| {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#17",
            DesignFeatureKind::Combine,
            17,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![301, 302, 303, 304]);
                draft.reference_count_offset = 126;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.kind_offset + 22 + 78;
                draft.frame_length = draft.paired_byte_offset;
                draft.feature_ordinal_offset = draft.kind_offset + 22;
            })
            .unwrap();
        if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
            *slot = Some(DesignCombineOperation {
                form: DesignCombineForm::OperandLists,
                operation: cadmpeg_ir::features::BooleanKind::Join,
                operation_offset,
                keep_tools: false,
                keep_tools_offset,
                target_record_index: 304,
                target_body_node: None,
                tools: DesignCombineTools {
                    first: DesignCombineBodySelection {
                        record_index: 302,
                        external_identity: None,
                        body_node: None,
                    },
                    additional: Vec::new(),
                },
            });
        }
        let mut native = crate::native::F3dNative::default();
        for record_index in [17, 301, 302, 303, 304] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                record_index,
                class_tag: "256".to_owned().try_into().unwrap(),
                byte_offset: u64::from(record_index),
            });
        }
        native.design_parameter_scopes.push(scope);
        crate::test_support::with_decode_context(|service_ctx| {
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
            findings.len()
        })
    };
    assert_eq!(findings(21, 29), 0);
    assert_eq!(findings(20, 29), 1);
    assert_eq!(findings(21, 25), 1);
}

#[test]
fn placement_ordered_owner_count_and_references_match_the_frame() {
    use crate::records::feature::{
        assembly::{DesignAssemblyAlignment, DesignAssemblyOwnerRun},
        scope::{DesignFeatureKind, DesignParameterScope},
    };
    use crate::records::identity::{Located, ReferenceRun};
    let owners = [(0, 50), (1, 51), (2, 52), (3, 53), (4, 54), (5, 55)];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::AsBuilt,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 422;
            draft.paired_byte_offset = draft.byte_offset + 422;
            draft.reference_members =
                ReferenceRun::unlocated(vec![80, 50, 51, 52, 53, 81, 82, 83, 84, 54, 55]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut alignment = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        (50..54).map(|value| Located { value, offset: 0 }).collect(),
        None,
    )
    .unwrap();
    alignment.additional_owner_run = Some(DesignAssemblyOwnerRun {
        count: Located {
            value: 2,
            offset: scope.byte_offset() + 43,
        },
        references: vec![
            Located {
                value: 54,
                offset: scope.byte_offset() + 47,
            },
            Located {
                value: 55,
                offset: scope.byte_offset() + 58,
            },
        ],
    });
    let holds = |alignment: &DesignAssemblyAlignment| {
        super::super::placement_ordered_references_hold(&scope, alignment, &owners)
    };
    assert!(holds(&alignment));
    let mut absent = alignment.clone();
    absent.additional_owner_run = None;
    assert!(!holds(&absent));
    for value in [0, 1, 3] {
        let mut changed = alignment.clone();
        changed.additional_owner_run.as_mut().unwrap().count.value = value;
        assert!(!holds(&changed));
    }
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().references[1].value = 54;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().references[0].offset += 1;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed.additional_owner_run.as_mut().unwrap().count.offset += 1;
    assert!(!holds(&changed));
    let mut changed = alignment.clone();
    changed
        .additional_owner_run
        .as_mut()
        .unwrap()
        .references
        .pop();
    assert!(!holds(&changed));
    assert!(!super::super::placement_ordered_references_hold(
        &scope,
        &alignment,
        &owners[..5]
    ));
}

#[test]
fn validation_accepts_malformed_rest_with_retained_limits() {
    use super::{
        f3d_native_mut, f3d_with_smbh_and_protein, synthetic_geometry_smbh, Codec, Cursor,
        DecodeOptions, F3dCodec,
    };
    use crate::records::feature::joint_motion::{DesignJointLimitLanes, DesignJointMotion};
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    let (mut ir, _, _) = decoded.into_parts();
    let before = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    f3d_native_mut(&mut ir)
        .design_joint_motions
        .push(DesignJointMotion {
            id: "f3d:Design/BulkStream.dat:design-joint-motion#12".into(),
            joint_record_index: 10,
            joint_byte_offset: 0,
            motion_record_index: 12,
            motion_byte_offset: 100,
            freedoms: vec![crate::records::identity::Located {
                value: 8,
                offset: 150,
            }],
            entry_words: Vec::new(),
            limits: Some(DesignJointLimitLanes {
                flags_offset: 42,
                first_flag: 1,
                second_flag: 1,
                maximum: 1.5,
                minimum: -1.5,
                values_offset: 54,
                rest: None,
                rest_malformed: true,
            }),
        });
    let after = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).unwrap()
    });
    assert_eq!(before, after);
}

#[test]
fn corner_flange_scope_round_trip_and_layout_validation() {
    use crate::layout::edge_flange_corner_operation as layout;
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#40",
        DesignFeatureKind::EdgeFlange,
        40,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("361".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.paired_byte_offset =
                draft.byte_offset + cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                51, 52, 55, 61, 62, 65, 81, 91, 101, 104, 108, 141,
            ]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let operation = serde_json::from_value(serde_json::json!({
        "edge_wrapper_record_indices":[51,61], "edge_group_record_indices":[52,62],
        "edge_operand_record_indices":[55,65], "aggregate_group_record_index":101,
        "aggregate_operand_record_indices":[104,108], "height_owner_record_index":81,
        "angle_owner_record_index":91, "width_mode":"full_edge",
        "width_distance_owner_record_indices":[], "settings_record_index":141,
        "bend_radius":0.35, "bend_radius_offset":scope.byte_offset() + cadmpeg_core::decode::u64_from_index(layout::INSIDE_BEND_RADIUS),
        "reference_side_code":4, "height_datum":"outer_faces", "bend_position":"inside"
    })).unwrap();
    if let DesignScopePayloadMut::EdgeFlange(slot) = scope.payload_mut() {
        *slot = Some(operation);
    }
    let wire = serde_json::to_value(&scope).unwrap();
    let decoded: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
    let findings = |scope: DesignParameterScope| {
        crate::test_support::with_decode_context(|service_ctx| {
            let ir = cadmpeg_ir::document::CadIr::empty();
            let headers = std::iter::once(scope.record_index)
                .chain(scope.reference_members().values().copied())
                .map(|record_index| crate::records::decal::DesignRecordHeader {
                    id: format!("f3d:Design/BulkStream.dat:design-record-header#{record_index}"),
                    record_index,
                    class_tag: if record_index == scope.record_index {
                        scope.class_tag.clone()
                    } else {
                        crate::records::references::DesignClassTag::try_from("256".to_owned())
                            .unwrap()
                    },
                    byte_offset: if record_index == scope.record_index {
                        scope.byte_offset()
                    } else {
                        scope.paired_byte_offset() + u64::from(record_index) * 21
                    },
                })
                .collect();
            let native = crate::native::F3dNative {
                design_parameter_scopes: vec![scope],
                design_record_headers: headers,
                ..Default::default()
            };
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
            findings
        })
    };
    assert!(findings(decoded).is_empty());
    let mut invalid = scope.clone();
    if let DesignScopePayloadMut::EdgeFlange(Some(operation)) = invalid.payload_mut() {
        operation.bend_radius_offset += 1;
    }
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope.clone();
    invalid.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("257".to_owned()).unwrap();
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope.clone();
    invalid
        .try_edit(|draft| {
            draft.frame_length -= 1;
            draft.paired_byte_offset -= 1;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope.clone();
    if let DesignScopePayloadMut::EdgeFlange(Some(operation)) = invalid.payload_mut() {
        operation.height_datum =
            crate::records::feature::sheet_metal::DesignSheetMetalHeightDatum::InnerFaces;
    }
    assert!(!findings(invalid).is_empty());
    let mut invalid = scope;
    if let DesignScopePayloadMut::EdgeFlange(Some(operation)) = invalid.payload_mut() {
        operation.bend_position = crate::records::feature::sheet_metal::DesignBendPosition::Outside;
    }
    assert!(!findings(invalid).is_empty());
}
