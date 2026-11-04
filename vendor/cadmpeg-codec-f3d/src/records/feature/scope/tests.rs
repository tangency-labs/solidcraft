// SPDX-License-Identifier: Apache-2.0
//! Absence spellings of the flattened scope-record readers.

#[test]
fn sketch_entity_binding_borrowed_wire_matches_owned_wire_bytes() {
    let binding: super::DesignSketchEntityBinding = serde_json::from_str(
        r#"{"entity_id":"0_35","entity_suffix":35,"entity_reference_offset":12}"#,
    )
    .unwrap();
    let owned = super::DesignSketchEntityBindingWire::from(binding.clone());
    assert_eq!(
        serde_json::to_vec(&binding).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn sketch_entity_binding_native_retained_limit_refuses_before_record_clone() {
    #[derive(serde::Serialize)]
    struct NativeRecord<'a> {
        id: &'static str,
        binding: &'a super::DesignSketchEntityBinding,
    }

    let binding: super::DesignSketchEntityBinding = serde_json::from_str(
        r#"{"entity_id":"0_35","entity_suffix":35,"entity_reference_offset":12}"#,
    )
    .unwrap();
    let record = NativeRecord {
        id: "f3d:native:sketch-binding#0",
        binding: &binding,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::SKETCH_ENTITY_BINDING_CLONE_COUNT.with(|count| count.set(0)),
        || super::SKETCH_ENTITY_BINDING_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn parameter_scope_borrowed_wire_matches_owned_json_bytes() {
    let scope = super::DesignParameterScope::empty(
        "f3d:design:scope#1",
        super::DesignScopePayload::WorkPoint(None),
        1,
    );
    let owned = super::DesignParameterScopeSerde::from(scope.clone());
    assert_eq!(
        serde_json::to_vec(&scope).expect("borrowed scope wire"),
        serde_json::to_vec(&owned).expect("owned scope wire")
    );
}

#[test]
fn parameter_scope_native_writer_refuses_retained_limit_before_outer_clone() {
    let scope = super::DesignParameterScope::empty(
        "f3d:design:scope#1",
        super::DesignScopePayload::WorkPoint(None),
        1,
    );
    let expected = serde_json::to_value(super::DesignParameterScopeSerde::from(scope.clone()))
        .expect("owned scope wire");
    super::SCOPE_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&scope, expected);
    super::SCOPE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn parameter_scope_nested_vertex_recipe_streams_without_cloning() {
    use crate::records::feature::work_geometry::{
        DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
        DesignWorkPointRule, DesignWorkPointRuleForm,
    };

    let recipe = serde_json::from_value(serde_json::json!({
        "record_index": 2,
        "byte_offset": 10,
        "class_tag": "369",
        "paired_byte_offset": 20,
        "paired_class_tag": "261",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 30,
        "recipe_id": "f3d:design:recipe#1",
        "recipe_prefix_offset": 41,
        "recipe_prefix_bytes": "00ff",
        "recipe_references": [],
        "recipe_program_offset": 43,
        "recipe_program": [0],
        "next_record_index": 7,
        "next_byte_offset": 50
    }))
    .expect("vertex recipe wire");
    let input = DesignWorkPointInput::try_new(
        2,
        12,
        Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
            recipe,
        })),
    )
    .expect("work point input");
    let rule = DesignWorkPointRule::try_from(DesignWorkPointRuleForm::Vertex { input })
        .expect("vertex work point rule");
    let mut scope = super::DesignParameterScope::empty(
        "f3d:design:scope#1",
        super::DesignScopePayload::WorkPoint(None),
        1,
    );
    if let super::DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 21,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([4.0, 3.0, 0.0]),
            position_offset: 0,
            rule,
            reference_type_offset: 0,
        });
    }
    let owned = super::DesignParameterScopeSerde::from(scope.clone());
    assert_eq!(
        serde_json::to_vec(&scope).expect("borrowed scope wire"),
        serde_json::to_vec(&owned).expect("owned scope wire")
    );
    super::SCOPE_CLONE_COUNT.with(|count| count.set(0));
    crate::records::feature::work_geometry::WORK_GEOMETRY_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &scope,
        serde_json::to_value(owned).expect("owned scope value"),
    );
    super::SCOPE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    crate::records::feature::work_geometry::WORK_GEOMETRY_CLONE_COUNT
        .with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn work_plane_frame_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_work_plane_frame")]
        frame: Option<super::DesignWorkPlaneTransform>,
    }
    for key in [
        "work_plane_transform",
        "work_plane_transform_offset",
        "work_plane_reference",
        "work_plane_reference_offset",
        "work_plane_construction",
    ] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.frame.is_none());
}

#[test]
fn joint_origin_frame_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_joint_origin_frame")]
        frame: Option<super::DesignJointOriginTransform>,
    }
    for key in [
        "joint_origin_transform",
        "joint_origin_transform_offset",
        "joint_origin_reference",
        "joint_origin_reference_offset",
    ] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.frame.is_none());
}

#[test]
fn sketch_entity_wire_refuses_a_null_key() {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(flatten, deserialize_with = "super::deserialize_sketch_entity")]
        entity: Option<super::DesignSketchEntityBinding>,
    }
    for key in ["entity_id", "entity_suffix", "entity_reference_offset"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Probe>(wire.clone()).is_err(),
            "{wire}"
        );
    }
    let absent: Probe = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(absent.entity.is_none());
}

/// Every flattened scope reader names the key it refuses.
///
/// Serde buffers a flattened field's keys into its own content map before the
/// reader runs, so no path a surrounding deserializer tracks reaches inside
/// one. The key reaches the refusal because the reading declaration states it.
#[test]
fn a_flattened_scope_reader_names_the_null_key_it_refuses() {
    use cadmpeg_test_support::refusal::refusal;

    #[derive(serde::Deserialize)]
    struct WorkPlane {
        #[serde(flatten, deserialize_with = "super::deserialize_work_plane_frame")]
        #[allow(dead_code)]
        value: Option<super::DesignWorkPlaneTransform>,
    }
    #[derive(serde::Deserialize)]
    struct JointOrigin {
        #[serde(flatten, deserialize_with = "super::deserialize_joint_origin_frame")]
        #[allow(dead_code)]
        value: Option<super::DesignJointOriginTransform>,
    }
    #[derive(serde::Deserialize)]
    struct SketchEntity {
        #[serde(flatten, deserialize_with = "super::deserialize_sketch_entity")]
        #[allow(dead_code)]
        value: Option<super::DesignSketchEntityBinding>,
    }

    let refusals = [
        (
            "work_plane_transform",
            refusal::<WorkPlane>("work_plane_transform"),
        ),
        (
            "work_plane_transform_offset",
            refusal::<WorkPlane>("work_plane_transform_offset"),
        ),
        (
            "work_plane_reference",
            refusal::<WorkPlane>("work_plane_reference"),
        ),
        (
            "work_plane_reference_offset",
            refusal::<WorkPlane>("work_plane_reference_offset"),
        ),
        (
            "work_plane_construction",
            refusal::<WorkPlane>("work_plane_construction"),
        ),
        (
            "joint_origin_transform",
            refusal::<JointOrigin>("joint_origin_transform"),
        ),
        (
            "joint_origin_transform_offset",
            refusal::<JointOrigin>("joint_origin_transform_offset"),
        ),
        (
            "joint_origin_reference",
            refusal::<JointOrigin>("joint_origin_reference"),
        ),
        (
            "joint_origin_reference_offset",
            refusal::<JointOrigin>("joint_origin_reference_offset"),
        ),
        ("entity_id", refusal::<SketchEntity>("entity_id")),
        ("entity_suffix", refusal::<SketchEntity>("entity_suffix")),
        (
            "entity_reference_offset",
            refusal::<SketchEntity>("entity_reference_offset"),
        ),
    ];
    for (key, message) in refusals {
        assert!(
            message.starts_with(&format!("{key}: ")),
            "the refusal of a null {key} states {message}"
        );
        assert!(
            message.contains("it does not state null"),
            "the refusal of a null {key} states {message}"
        );
    }
}

#[test]
fn native_scope_field_rewrite_matches_the_typed_nested_vertex_walk() {
    use crate::records::feature::work_geometry::{
        DesignVertexRecipe, DesignWorkPlaneConstruction, DesignWorkPointConstruction,
        DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRule,
        DesignWorkPointRuleForm,
    };
    let reference = serde_json::json!({
        "selector": 1, "selector_offset": 1, "token": "f3d:model:face#one", "token_offset": 2,
        "design_reference": 1, "design_reference_offset": 3,
        "candidate_faces": ["f3d:model:face#one"], "candidate_edges": ["f3d:model:edge#one"],
        "alternate_selector_faces": ["f3d:model:face#one"], "alternate_selector_edges": ["f3d:model:edge#one"]
    });
    let recipe: DesignVertexRecipe = serde_json::from_value(serde_json::json!({
        "record_index": 2, "byte_offset": 10, "class_tag": "369", "paired_byte_offset": 20,
        "paired_class_tag": "261", "recipe_record_index": 5, "recipe_record_byte_offset": 30,
        "recipe_id": "f3d:design:recipe#1", "recipe_prefix_offset": 41,
        "recipe_prefix_bytes": "00ff", "recipe_references": [reference],
        "recipe_program_offset": 43, "recipe_program": [0], "next_record_index": 7, "next_byte_offset": 50
    })).unwrap();
    let plane = DesignWorkPlaneConstruction::try_new(
        21,
        Box::new([recipe.clone(), recipe.clone(), recipe.clone()]),
    )
    .unwrap();
    let plane_scope = super::DesignParameterScope::empty(
        "f3d:design:scope#plane",
        super::DesignScopePayload::WorkPlane(Some(super::DesignWorkPlaneTransform {
            work_plane_transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
            work_plane_transform_offset: 0,
            reference: None,
            work_plane_construction: Some(plane),
        })),
        1,
    );
    let input = DesignWorkPointInput::try_new(
        2,
        12,
        Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe {
            recipe,
        })),
    )
    .unwrap();
    let rule = DesignWorkPointRule::try_from(DesignWorkPointRuleForm::Vertex { input }).unwrap();
    let mut scope = super::DesignParameterScope::empty(
        "f3d:design:scope#1",
        super::DesignScopePayload::WorkPoint(None),
        1,
    );
    if let super::DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 21,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([4.0, 3.0, 0.0]),
            position_offset: 0,
            rule,
            reference_type_offset: 0,
        });
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut native = cadmpeg_ir::native::NativeNamespace::default();
    native
        .set_arena(
            &ctx,
            "design_parameter_scopes",
            std::slice::from_ref(&scope),
        )
        .unwrap();
    let record = &native.arenas()["design_parameter_scopes"][0];
    let remap = |id: &str| {
        Ok(format!(
            "f3d:occurrence:{}",
            id.strip_prefix("f3d:model:").unwrap()
        ))
    };
    let expected = cadmpeg_ir::schema::rewrite::identities(&ctx, scope.clone(), remap).unwrap();
    let mut expected = serde_json::to_value(expected)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    expected.remove("id");
    let fields = record
        .rewrite_fields::<super::DesignParameterScope, _>(&ctx, remap)
        .unwrap();
    assert_eq!(fields, expected);
    assert_eq!(
        fields["work_point_construction"]["rule"]["input"]["carrier"]["recipe"]
            ["recipe_references"][0]["token"],
        "f3d:model:face#one"
    );
    native
        .set_arena(
            &ctx,
            "design_parameter_scopes",
            std::slice::from_ref(&plane_scope),
        )
        .unwrap();
    let record = &native.arenas()["design_parameter_scopes"][0];
    let expected = cadmpeg_ir::schema::rewrite::identities(&ctx, plane_scope, remap).unwrap();
    let mut expected = serde_json::to_value(expected)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    expected.remove("id");
    let fields = record
        .rewrite_fields::<super::DesignParameterScope, _>(&ctx, remap)
        .unwrap();
    assert_eq!(fields, expected);
    for input in fields["work_plane_construction"]["inputs"]
        .as_array()
        .unwrap()
    {
        assert_eq!(
            input["recipe_references"][0]["candidate_faces"][0],
            "f3d:occurrence:face#one"
        );
        assert_eq!(input["recipe_references"][0]["token"], "f3d:model:face#one");
    }
    ctx.finish_session().unwrap();
}

fn emboss_scope() -> super::DesignParameterScope {
    let mut scope = super::DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:design-parameter-scope#0",
        super::DesignFeatureKind::Emboss,
        40,
    );
    let construction = serde_json::from_value(serde_json::json!({
        "control_words": [7, 9],
        "native_pair": [17, 23],
        "depth_owner": {"value": 60, "offset": 29},
        "auxiliary_group": {"value": 72, "offset": 40},
        "horizontal_owner": {"value": 63, "offset": 51},
        "rotation_owner": {"value": 69, "offset": 82},
        "face_group": {"value": 48, "offset": 93},
        "vertical_owner": {"value": 66, "offset": 104},
        "profile_group": {"value": 41, "offset": 119}
    }))
    .unwrap();
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
            draft.payload = super::DesignScopePayload::Emboss(Some(construction));
        })
        .unwrap();
    scope
}

#[test]
fn emboss_scope_wire_retains_native_controls_and_matches_the_borrowed_writer() {
    let scope = emboss_scope();
    assert!(scope.payload().is_uninterpreted());
    let owned = super::DesignParameterScopeSerde::from(scope.clone());
    assert_eq!(
        serde_json::to_vec(&scope).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
    let json = serde_json::to_value(&scope).unwrap();
    let restored: super::DesignParameterScope = serde_json::from_value(json).unwrap();
    assert_eq!(restored, scope);
}

#[test]
fn emboss_scope_wire_refuses_another_family_and_invalid_owner_links() {
    let source = serde_json::to_value(emboss_scope()).unwrap();
    let mut wrong_kind = source.clone();
    wrong_kind["kind"] = serde_json::json!("Thread");
    assert!(serde_json::from_value::<super::DesignParameterScope>(wrong_kind).is_err());
    for field in ["depth_owner", "face_group", "profile_group"] {
        let mut changed = source.clone();
        changed["emboss_construction"][field]["value"] = serde_json::json!(90);
        assert!(serde_json::from_value::<super::DesignParameterScope>(changed).is_err());
    }
    let mut wrong_offset = source.clone();
    wrong_offset["emboss_construction"]["depth_owner"]["offset"] = serde_json::json!(30);
    assert!(serde_json::from_value::<super::DesignParameterScope>(wrong_offset).is_err());
    let mut null_profile = source;
    null_profile["emboss_construction"]["sketch_profile"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<super::DesignParameterScope>(null_profile).is_err());
}

#[test]
fn emboss_scope_wire_refuses_a_typed_reference_table() {
    let scope = emboss_scope();
    let mut draft = scope.clone().into_draft();
    draft.reference_members = crate::records::identity::ReferenceRun::located(
        scope
            .reference_members()
            .values()
            .copied()
            .enumerate()
            .map(|(ordinal, value)| crate::records::identity::Located {
                value,
                offset: 240 + 51 * cadmpeg_core::decode::u64_from_index(ordinal),
            })
            .collect(),
    );
    draft.kind_offset = 706;
    draft.frame_length = 1000;
    draft.paired_byte_offset = 1000;
    draft.feature_ordinal_offset = draft.paired_byte_offset
        - cadmpeg_core::decode::u64_from_index(
            crate::layout::typed_reference_parameter_scope_tail::LEN,
        );
    assert!(super::DesignParameterScope::try_new(draft.clone()).is_err());
    draft.payload = super::DesignScopePayload::Emboss(None);
    assert!(super::DesignParameterScope::try_new(draft).is_ok());
}

#[test]
fn rigid_group_history_offsets_follow_both_tail_choices() {
    for (frame, tail, previous) in [
        (257, 76, 30),
        (258, 77, 30),
        (268, 87, 41),
        (269, 88, 41),
        (261, 76, 30),
        (262, 77, 30),
        (272, 87, 41),
        (273, 88, 41),
    ] {
        let mut scope = super::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#3",
            super::DesignFeatureKind::AsBuilt,
            3,
        );
        scope
            .try_edit(|draft| {
                crate::design::test_support::lengthen_scope_frame(draft, frame);
                draft.reference_count_offset = draft.byte_offset + frame - tail - 28;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(Vec::new());
                draft.layout_fixture_references();
                draft.feature_ordinal_offset = draft.paired_byte_offset - tail;
                draft.previous_history_state_id = Some(1);
                draft.previous_history_state_id_offset =
                    Some(draft.feature_ordinal_offset + previous);
            })
            .unwrap();
        assert!(scope
            .try_edit(|draft| {
                draft.previous_history_state_id_offset =
                    Some(draft.feature_ordinal_offset + previous + 1);
            })
            .is_err());
    }
}

#[test]
fn hole_extent_without_point_carrier_round_trips_and_checks_kind() {
    use super::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
    for control in [0, 1] {
        let mut scope = DesignParameterScope::empty("f3d:scope#1", DesignFeatureKind::Hole, 1);
        if let DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
            slot.extent = Some(crate::records::feature::hole::DesignHoleExtent {
                leading_word: None,
                extent_control: Some(control),
                saved_non_distance_depth: None,
                direction_control: None,
            });
        }
        let wire = serde_json::to_value(&scope).unwrap();
        let decoded: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.hole_extent(), scope.hole_extent());
        assert!(decoded.hole_construction().is_none());
        let mut invalid = wire.clone();
        invalid["hole_extent"]["extent_control"] = serde_json::json!(2);
        assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
        let mut invalid = wire.clone();
        invalid["kind"] = serde_json::json!("Thread");
        assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
        let mut invalid = wire;
        invalid["hole_extent"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
    }
}

#[test]
fn native_profile_wire_round_trips_and_rejects_null() {
    use super::DesignParameterScope;
    let (archive, mut scope, headers, entities) =
        crate::test_support::multiple_profiles::fixture(2);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::decode::operands::bind_sketch_profiles(
            scan,
            std::slice::from_mut(&mut scope),
            &headers,
            &entities,
        )
        .unwrap();
        let wire = serde_json::to_value(&scope).unwrap();
        let loaded: DesignParameterScope = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(loaded).unwrap(), wire);
        let mut invalid = wire;
        invalid["extrude_native_profiles"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
    });
}
