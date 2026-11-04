// SPDX-License-Identifier: Apache-2.0

fn validation_entity_header(
    references: Vec<crate::records::identity::Located<u32>>,
) -> crate::records::entity_header::DesignEntityHeader {
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, SketchHeaderReferences,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};
    DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
        byte_offset: 10,
        entity_id: DesignEntityId::from_parts("sketch", 1),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.into()),
            (!references.is_empty()).then_some(SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: 10,
                references,
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    }
}

fn validation_body_bounds(with_binding: bool) -> crate::native::F3dNative {
    use crate::records::bodies::{
        DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
    };
    use crate::records::entity_header::{
        DesignEntityHeader, DesignEntityRegistration, DESIGN_MODULE_BODY,
    };
    use crate::records::identity::{DesignEntityId, ReferenceRun};

    let binding_id = "f3d:Design/BulkStream.dat:design-body-binding#50";
    let bounds = DesignBodyBounds::try_from(DesignBodyBoundsWire {
        id: "f3d:Design/BulkStream.dat:design-body-bounds#10".into(),
        entity_suffix: 1,
        entity_byte_offset: 10,
        record_indices: [2, 3, 4],
        record_byte_offsets: [20, 30, 40],
        value_byte_offsets: [21, 31, 41],
        body_binding_ids: if with_binding {
            vec![binding_id.to_owned().try_into().unwrap()]
        } else {
            Default::default()
        },
        maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
        minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
    })
    .unwrap();
    let mut native = crate::native::F3dNative::default();
    native.design_body_bounds.push(bounds);
    if with_binding {
        native.design_body_bindings.push(
            DesignBodyBinding::try_from(DesignBodyBindingWire {
                component_owner: None,
                component_owner_unresolved: false,
                history_only: false,
                id: binding_id.into(),
                stream: "Design/BulkStream.dat".into(),
                pair_count: 1,
                pair_ordinal: 0,
                asm_body_key: 1,
                asm_body_key_offset: 50,
                entity_suffix: 1,
                entity_suffix_offset: 58,
                blob_name: "BREP.body".into(),
                blob_name_offset: 60,
                body: None,
            })
            .unwrap(),
        );
        native.design_entity_headers.push(DesignEntityHeader {
            id: "f3d:Design/BulkStream.dat:entity-header#1".into(),
            byte_offset: 10,
            entity_id: DesignEntityId::from_parts("body", 1),
            class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned())
                .unwrap(),
            optional_slot_present: false,
            registration: DesignEntityRegistration::new(
                Some(DESIGN_MODULE_BODY.into()),
                None,
                ReferenceRun::unlocated(Vec::new()),
            )
            .unwrap(),
        });
    }
    native
}

#[test]
fn body_bounds_valid_binding_order_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = validation_body_bounds(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_body_bounds(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}

fn history_only_binding(
    extra: serde_json::Value,
) -> Result<crate::records::bodies::DesignBodyBinding, serde_json::Error> {
    let mut value = serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-body-binding#50",
        "stream": "Design/BulkStream.dat",
        "pair_count": 1,
        "pair_ordinal": 0,
        "asm_body_key": 1,
        "asm_body_key_offset": 50,
        "entity_suffix": 1,
        "entity_suffix_offset": 58,
        "blob_name": "BREP.body",
        "blob_name_offset": 60,
        "history_only": true,
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    serde_json::from_value(value)
}

#[test]
fn history_only_binding_names_no_solved_body_or_owner_and_no_retained_key() {
    assert!(history_only_binding(serde_json::json!({})).is_ok());
    assert!(history_only_binding(serde_json::json!({"body": "f3d:brep:entity#1"})).is_err());
    assert!(history_only_binding(serde_json::json!({"component_owner_unresolved": true})).is_err());
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native
            .design_body_bindings
            .push(history_only_binding(serde_json::json!({})).unwrap());
        let findings = |native: &crate::native::F3dNative| {
            let ctx = super::super::Ctx::new(&ir, native, service_ctx);
            let mut findings = Vec::new();
            super::super::validate_body_bindings(&ctx, &mut findings).unwrap();
            findings
        };
        assert!(findings(&native).is_empty());
        native.body_native_keys.push(
            serde_json::from_value(serde_json::json!({
                "id": "f3d:asm:body-native-key#1",
                "body": "f3d:brep:entity#1",
                "asm_body_key": 1,
                "body_ordinal": 0,
                "record_index": 1,
                "source_brep": "BREP.body",
            }))
            .unwrap(),
        );
        assert_eq!(findings(&native).len(), 1);
    });
}

#[test]
fn body_binding_valid_group_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = validation_body_bounds(true);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_body_bindings(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn validation_placement(
    scope_record_index: Option<u32>,
    ordinal: Option<u32>,
    member: bool,
) -> crate::records::sketch_placement::DesignSketchPlacement {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement, DesignSketchVisibility,
    };
    DesignSketchPlacement {
        id: "f3d:Design/BulkStream.dat:sketch-placement#1".into(),
        scope_record_index,
        entity_id: crate::records::identity::DesignEntityId::from_parts("sketch", 1),
        visibility: ordinal.map(|ordinal| {
            let mut visibility =
                DesignSketchVisibility::new(std::num::NonZeroU32::new(ordinal).unwrap(), 100, true)
                    .unwrap();
            visibility.ordinal_collection = std::num::NonZeroU64::new(100);
            visibility
        }),
        class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("112".to_owned())
            .unwrap(),
        frame: DesignSketchFrame::new(
            10,
            if member {
                DesignSketchFrameForm::MemberCompact {
                    paired_byte_offset: 20,
                }
            } else {
                DesignSketchFrameForm::ScopeCompact
            },
        )
        .unwrap(),
    }
}

fn placement_native(
    placement: crate::records::sketch_placement::DesignSketchPlacement,
) -> crate::native::F3dNative {
    let mut native = crate::native::F3dNative::default();
    if placement.visibility.is_some() {
        add_ordinal_collection_types(&mut native);
        native
            .design_entity_headers
            .push(validation_entity_header(Vec::new()));
    }
    native.design_sketch_placements.push(placement);
    native
}

#[test]
fn placement_contiguous_visibility_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = placement_native(validation_placement(None, Some(1), true));
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty());
    })
}

fn two_sketch_placements(
    separate_collections: bool,
    stream_ordinals: [u32; 2],
) -> crate::native::F3dNative {
    let mut native = crate::native::F3dNative::default();
    for (ordinal, ((suffix, header_offset, ordinal_offset), stream_ordinal)) in
        [(5_u64, 10_u64, 100_u64), (25, 110, 200)]
            .into_iter()
            .zip(stream_ordinals)
            .enumerate()
    {
        let mut header = validation_entity_header(Vec::new());
        header.id = format!("f3d:Design/BulkStream.dat:entity-header#{header_offset}");
        header.byte_offset = header_offset;
        header.entity_id = crate::records::identity::DesignEntityId::from_parts("sketch", suffix);
        native.design_entity_headers.push(header);
        let mut placement = validation_placement(None, Some(1), true);
        placement.id = format!("f3d:Design/BulkStream.dat:sketch-placement#{ordinal_offset}");
        placement.record_index = u32::try_from(ordinal).unwrap() + 1;
        placement.entity_id =
            crate::records::identity::DesignEntityId::from_parts("sketch", suffix);
        placement.visibility = Some(
            crate::records::sketch_placement::DesignSketchVisibility::new(
                std::num::NonZeroU32::new(stream_ordinal).unwrap(),
                ordinal_offset,
                true,
            )
            .unwrap(),
        );
        placement.visibility.as_mut().unwrap().ordinal_collection =
            std::num::NonZeroU64::new(if separate_collections && ordinal == 1 {
                200
            } else {
                100
            });
        native.design_sketch_placements.push(placement);
    }
    add_ordinal_collection_types(&mut native);
    native
}

#[test]
fn sketch_geometry_ordinals_use_collections_independently_of_components() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = two_sketch_placements(true, [1, 1]);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        let native = two_sketch_placements(false, [1, 1]);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design sketch placement has an invalid frame or scope link"
        );
    })
}

#[test]
fn sketch_geometry_ordinals_may_skip_values_and_decrease_with_entity_order() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let findings_for = |stream_ordinals| {
            let native = two_sketch_placements(false, stream_ordinals);
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::validate_sketch_placements(&ctx, &mut findings);
            findings
        };
        let skipped = findings_for([2, 5]);
        assert!(skipped.is_empty(), "{skipped:?}");
        let decreasing = findings_for([5, 2]);
        assert!(decreasing.is_empty(), "{decreasing:?}");
    })
}

#[test]
fn sketch_geometry_decreasing_ordinals_keep_duplicate_refusal() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = two_sketch_placements(false, [8, 3]);
        let mut findings = Vec::new();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        native.design_sketch_placements[1]
            .visibility
            .as_mut()
            .unwrap()
            .stream_ordinal = std::num::NonZeroU32::new(8).unwrap();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].message,
            "Fusion Design sketch placement has an invalid frame or scope link"
        );
    });
}

#[test]
fn sketch_visibility_ordinals_are_unique_within_explicit_collections() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = two_sketch_placements(false, [2, 2]);
        for (placement, collection) in native.design_sketch_placements.iter_mut().zip([100, 200]) {
            placement.visibility.as_mut().unwrap().ordinal_collection =
                std::num::NonZeroU64::new(collection);
        }
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        findings.clear();
        native.design_sketch_placements[1]
            .visibility
            .as_mut()
            .unwrap()
            .ordinal_collection = std::num::NonZeroU64::new(100);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].message,
            "Fusion Design sketch placement has an invalid frame or scope link"
        );
    });
}

#[test]
fn act_table_pool_without_component_links_needs_no_document_root() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        native.act_guids.push(
            crate::records::act::ActGuid::new(
                "f3d:ACT/BulkStream.dat:act-guid#20".into(),
                20,
                0,
                "11111111-2222-4333-8444-555555555555".into(),
            )
            .unwrap(),
        );
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_act(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty(), "{findings:?}");
        for (offset, index) in [(100, 1), (200, 2)] {
            native.act_component_links.push(
                crate::records::act::ActComponentLink::try_new(
                    format!("f3d:ACT/BulkStream.dat:act-component-link#{offset}"),
                    index,
                    "267".to_owned().try_into().unwrap(),
                    crate::records::act::ActComponentReferences {
                        instance_root: 12,
                        tracked_entity: 3,
                        components_root: 7,
                    },
                    crate::records::act::ActRegistryFlag::Off,
                    crate::records::act::ActComponentLayout::new(
                        offset,
                        "0_3".into(),
                        "Root".into(),
                        1,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
        }
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_act(&ctx, &mut findings).unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings.iter().all(|finding| finding.message
            == "Fusion ACT stream does not have exactly one document-root component link"));
    });
}

#[test]
fn sketch_geometry_ordinals_follow_entities_across_placement_order() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = two_sketch_placements(false, [5, 2]);
        for (placement, collection) in native.design_sketch_placements.iter_mut().zip([100, 200]) {
            placement.visibility.as_mut().unwrap().ordinal_collection =
                std::num::NonZeroU64::new(collection);
        }
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        native.design_sketch_placements[1]
            .visibility
            .as_mut()
            .unwrap()
            .ordinal_collection = std::num::NonZeroU64::new(100);
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
        native.design_sketch_placements.reverse();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
    });
}

fn add_ordinal_collection_types(native: &mut crate::native::F3dNative) {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    native.design_types.push(
        SegmentType::try_new(
            "f3d:Design/MetaStream.dat:design-type#0".into(),
            SegmentTypeData {
                byte_offset: 0,
                type_guid: "5275CBA4-3D7D-40DC-A651-1DFF3FC8AFBF"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                type_guid_offset: 4,
                base_type_guid: BaseTypeGuid::Guid {
                    value: "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 44,
                },
                version: 4,
                version_offset: 80,
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![100, 200]),
            },
        )
        .unwrap(),
    );
}

#[test]
fn sketch_visibility_names_a_decoded_ordinal_collection_or_none() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for (collection, finding_count) in [(None, 0), (std::num::NonZeroU64::new(999), 1)] {
            let mut native = two_sketch_placements(false, [1, 2]);
            native.design_sketch_placements[0]
                .visibility
                .as_mut()
                .unwrap()
                .ordinal_collection = collection;
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::validate_sketch_placements(&ctx, &mut findings);
            assert_eq!(findings.len(), finding_count);
            assert!(findings.iter().all(|finding| finding.message
                == "Fusion Design sketch placement has an invalid frame or scope link"));
        }
        let mut native = two_sketch_placements(false, [1, 2]);
        native.design_types.clear();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|finding| finding.message
            == "Fusion Design sketch placement has an invalid frame or scope link"));
    });
}

#[test]
fn typed_profile_compact_placement_validator_and_wire_admission_agree() {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement,
    };
    let mut placement = validation_placement(None, None, true);
    placement.frame = DesignSketchFrame::new(
        10,
        DesignSketchFrameForm::MemberTypedCompact {
            paired_byte_offset: 20,
        },
    )
    .unwrap();
    let mut wire = serde_json::to_value(&placement).unwrap();
    assert_eq!(wire["frame_length"], 74);
    assert!(serde_json::from_value::<DesignSketchPlacement>(wire.clone()).is_ok());
    wire["transform"][0][3] = 1.0.into();
    assert!(serde_json::from_value::<DesignSketchPlacement>(wire.clone()).is_err());
    wire["transform"][0][3] = 0.0.into();
    wire["frame_length"] = 73.into();
    assert!(serde_json::from_value::<DesignSketchPlacement>(wire).is_err());
    crate::test_support::with_decode_context(|ctx| {
        let ir = cadmpeg_ir::document::CadIr::empty();
        let native = placement_native(placement);
        let ctx = super::super::Ctx::new(&ir, &native, ctx);
        let mut findings = Vec::new();
        super::super::validate_sketch_placements(&ctx, &mut findings);
        assert!(findings.is_empty());
    });
}
