// SPDX-License-Identifier: Apache-2.0

use super::{PipeIndex, PipeRoleAssessment};
use crate::records::feature::scope::DesignScopePayload;
use crate::test_support::pipe::{Config, Fixture};

fn reopened(native: &crate::native::F3dNative) -> crate::native::F3dNative {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&ctx, &mut namespace).unwrap();
    crate::native::F3dNative::load(&namespace).unwrap()
}

#[test]
fn pipe_native_admits_known_roles_and_keeps_role8() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for operation in [1, 2, 3, 4] {
        for body_role in [4, 8] {
            let native = Fixture::new(Config {
                operation,
                body_role,
                ..Default::default()
            })
            .decode(&ctx)
            .unwrap();
            let scope = &native.design_parameter_scopes[0];
            let DesignScopePayload::Pipe(Some(pipe)) = scope.payload() else {
                panic!("complete native fixture")
            };
            let index = PipeIndex::new(
                &native.design_types,
                &native.design_record_headers,
                &native.design_parameter_owners,
                &native.design_construction_operand_groups,
            );
            let assessment = index.check_relation(scope, pipe);
            assert_eq!(
                assessment,
                Some(if operation == 3 {
                    PipeRoleAssessment::UnassignedIntersect
                } else {
                    PipeRoleAssessment::Assigned
                })
            );
            let loaded = reopened(&native);
            assert_eq!(native, loaded);
            if operation == 1 || operation == 2 {
                assert_eq!(
                    loaded.design_construction_operand_groups[0].role().raw(),
                    u64::from(body_role) << 32
                );
            }
            let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
            native.store(&ctx, ir.native.namespace_mut("f3d")).unwrap();
            let findings = crate::validate::validate_native(&ctx, &ir).unwrap();
            assert!(!findings.iter().any(|finding| finding.message
                == "Fusion Design path-feature operand roles conflict with its construction"));
        }
    }
}

#[test]
fn pipe_native_round_trips_point_relations_and_absent_construction() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for version in [2, 3] {
        let native = Fixture::new(Config {
            point_version: Some(version),
            variant: true,
            legacy: true,
            ..Default::default()
        })
        .decode(&ctx)
        .unwrap();
        assert_eq!(native, reopened(&native));
    }
    let mut native = Fixture::new(Config::default()).decode(&ctx).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Pipe(slot) =
        native.design_parameter_scopes[0].payload_mut()
    {
        *slot = None;
    }
    assert_eq!(native, reopened(&native));
}

fn reject_mutation(arena: &str, mutate: impl FnOnce(&mut serde_json::Value)) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let native = Fixture::new(Config {
        point_version: Some(2),
        operation: 1,
        ..Default::default()
    })
    .decode(&ctx)
    .unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&ctx, &mut namespace).unwrap();
    let mut values = match arena {
        "design_parameter_scopes" => serde_json::to_value(&native.design_parameter_scopes).unwrap(),
        "design_parameter_owners" => serde_json::to_value(&native.design_parameter_owners).unwrap(),
        "design_construction_operand_groups" => {
            serde_json::to_value(&native.design_construction_operand_groups).unwrap()
        }
        "design_types" => serde_json::to_value(&native.design_types).unwrap(),
        "design_record_headers" => serde_json::to_value(&native.design_record_headers).unwrap(),
        _ => panic!("mutation arena"),
    };
    mutate(&mut values);
    namespace
        .set_arena(&ctx, arena, values.as_array().unwrap())
        .unwrap();
    assert!(
        crate::native::F3dNative::load(&namespace).is_err(),
        "native mutation in {arena}"
    );
}

#[test]
fn pipe_native_load_rejects_cross_record_tampering() {
    reject_mutation("design_parameter_owners", |rows| {
        rows[0]["scope_record_index"] = 11.into();
    });
    reject_mutation("design_parameter_owners", |rows| {
        rows[0]["local_ordinal"] = 1.into();
    });
    reject_mutation("design_parameter_owners", |rows| {
        rows.as_array_mut().unwrap().remove(0);
    });
    reject_mutation("design_record_headers", |rows| {
        rows[0]["byte_offset"] = 1.into();
    });
    reject_mutation("design_record_headers", |rows| {
        let mut duplicate = rows[0].clone();
        duplicate["id"] =
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-record-header#999999".into();
        duplicate["byte_offset"] = 999_999.into();
        rows.as_array_mut().unwrap().push(duplicate);
    });
    reject_mutation("design_types", |rows| rows[0]["version"] = 9.into());
    reject_mutation("design_types", |rows| {
        rows[1]["entity_ids"] = rows[0]["entity_ids"].clone();
        rows[1]["entity_id_offsets"] = rows[0]["entity_id_offsets"].clone();
    });
    reject_mutation("design_parameter_scopes", |rows| {
        rows[0]["path_feature_construction"]["record_indexes"][0] = 200.into();
    });
    reject_mutation("design_parameter_scopes", |rows| {
        rows[0]["path_feature_construction"]["operation_offset"] = 25.into();
    });
    reject_mutation("design_parameter_scopes", |rows| {
        rows[0]["path_feature_construction"]["point_selection"]["prefix_reference_offset"] =
            64.into();
    });
    reject_mutation("design_parameter_scopes", |rows| {
        rows[0]["path_feature_construction"]["point_selection"]["point_data"]["rule"]["inputs"]
            [0]["record_index"] = 410.into();
    });
    reject_mutation("design_parameter_scopes", |rows| {
        rows[0]["reference_members"][0] = 200.into();
    });
    reject_mutation("design_construction_operand_groups", |rows| {
        rows[0]["scope_record_index"] = 11.into();
    });
    reject_mutation("design_construction_operand_groups", |rows| {
        let body = rows
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["role"] == serde_json::json!(4_u64 << 32))
            .unwrap();
        body["role"] = (5_u64 << 32).into();
    });
    reject_mutation("design_construction_operand_groups", |rows| {
        rows[0]["members"] = serde_json::json!([]);
        rows[0]["member_offsets"] = serde_json::json!([]);
    });
    reject_mutation("design_parameter_owners", |rows| {
        rows[0]["evaluated_value"] = serde_json::Value::Null;
    });
    reject_mutation("design_parameter_owners", |rows| {
        rows[0]["id"] = "f3d:Foreign/Design1/BulkStream.dat:design-parameter-owner#0".into();
    });
}

#[test]
fn pipe_wire_rejects_every_deleted_key_and_incomplete_current_fields() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let native = Fixture::new(Config::default()).decode(&ctx).unwrap();
    let DesignScopePayload::Pipe(Some(pipe)) = native.design_parameter_scopes[0].payload() else {
        panic!("Pipe fixture")
    };
    let wire = serde_json::to_value(pipe).unwrap();
    assert_eq!(wire["opaque_post_operation"], "0101");
    for key in [
        "section_shape",
        "section_shape_offset",
        "filled",
        "filled_offset",
        "values",
        "value_offsets",
    ] {
        let mut bad = wire.clone();
        bad[key] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<super::super::DesignPipeConstruction>(bad).is_err(),
            "deleted {key}"
        );
    }
    for key in [
        "is_hollow",
        "is_hollow_offset",
        "opaque_post_operation",
        "opaque_post_operation_offset",
        "record_indexes",
    ] {
        let mut bad = wire.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<super::super::DesignPipeConstruction>(bad).is_err(),
            "missing {key}"
        );
    }
    for bytes in [
        serde_json::Value::Null,
        serde_json::json!("01"),
        serde_json::json!("010101"),
        serde_json::json!([1, 1]),
    ] {
        let mut bad = wire.clone();
        bad["opaque_post_operation"] = bytes;
        assert!(serde_json::from_value::<super::super::DesignPipeConstruction>(bad).is_err());
    }
}

#[test]
fn pipe_new_body_rejects_a_body_role_with_complete_generic_links() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut native = Fixture::new(Config::default()).decode(&ctx).unwrap();
    native.design_construction_operand_groups[0].operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_B,
        );
    for data in &mut native.design_types {
        if data.type_guid.as_str() == super::BODY_GUID {
            data.set_entities(crate::records::identity::ReferenceRun::unlocated(vec![410]));
        }
        if data.type_guid.as_str() == super::PERSISTENT_GUID {
            data.set_entities(crate::records::identity::ReferenceRun::unlocated(Vec::new()));
        }
    }
    native
        .design_record_headers
        .iter_mut()
        .find(|header| header.record_index == 410)
        .unwrap()
        .class_tag = "261".to_owned().try_into().unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&ctx, &mut namespace).unwrap();
    assert!(matches!(
        crate::native::F3dNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidCollection(_))
    ));
}

#[test]
fn pipe_native_load_rejects_an_empty_new_body_path() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut native = Fixture::new(Config::default()).decode(&ctx).unwrap();
    native.design_construction_operand_groups[0]
        .try_set_members(Vec::new())
        .unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&ctx, &mut namespace).unwrap();
    assert!(matches!(
        crate::native::F3dNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidCollection(_))
    ));
}
