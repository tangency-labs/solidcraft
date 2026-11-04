// SPDX-License-Identifier: Apache-2.0
//! Native-validation unit tests for Fusion Design records.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

pub(crate) mod align;

use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::records::{
    feature::scope::DesignScopePayload, topology::extrude_selection::DesignOperandRole,
};
use crate::test_support::native_test::f3d_native_mut;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein;
use crate::F3dCodec;

mod act_document_root;
mod arithmetic;
mod body_ownership;
mod component_insert;
mod component_occurrences;
mod configurations;
mod construction_group;
mod construction_identity;
mod copy_paste_bodies;
mod edge_identity;
mod edge_treatment_vertex;
mod entity_bindings;
mod extrude_extent;
mod extrude_group;
mod extrude_member;
mod extrude_omitted_offset;
mod extrude_operation_state;
mod extrude_prologue_offsets;
mod face_boundary;
mod face_group;
mod face_group_members;
mod face_operand;
mod fillet_group;
mod history_transitions;
mod move_transform;
mod operand_group_carrier;
mod parameter_owner;
mod parameter_scope;
mod repro_fragments;
mod revolve_target_roles;
mod sweep_body_roles;
mod thread_link;
mod timeline;
mod xref;

fn recipe_reference() -> crate::records::dimensions::DesignRecipeReference {
    crate::records::dimensions::DesignRecipeReference {
        selector: 1,
        selector_offset: 20,
        token: "3".into(),
        token_offset: 28,
        design_reference: 329,
        design_reference_offset: 37,
        candidate_faces: vec![
            cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#10").expect("identity grammar")
        ],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

#[test]
fn finalized_recipe_reference_validation_ignores_only_derived_candidates() {
    let actual = recipe_reference();
    let mut expected = actual.clone();
    expected.candidate_faces =
        vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#20").expect("identity grammar")];
    assert!(super::recipe_reference_frames_match(
        &[actual.clone()],
        &[expected.clone()],
        true,
    ));
    assert!(!super::recipe_reference_frames_match(
        &[actual.clone()],
        &[expected.clone()],
        false,
    ));

    expected.design_reference += 1;
    assert!(!super::recipe_reference_frames_match(
        &[actual],
        &[expected],
        true,
    ));
}

#[test]
fn validation_accepts_class_410_component_insert_identity_frame() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            assembly_features::DesignComponentInsertConstruction, scope::DesignParameterScope,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#100");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::scope::DesignFeatureKind::ComponentInsert,
        169,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 100;
            draft.class_tag =
                crate::records::references::DesignClassTag::try_from("410".to_owned()).unwrap();
            draft.frame_length = 261;
            draft.kind_offset = 252;
            draft.reference_count_offset = 229;
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![167],
                vec![234],
                "reference_members",
            )
            .unwrap();
            draft.paired_class_tag =
                crate::records::references::DesignClassTag::try_from("261".to_owned()).unwrap();
            draft.paired_byte_offset = 361;
            draft.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
            draft.feature_ordinal_offset = 284;

            draft.previous_history_state_id_offset = Some(315);
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
        scope.payload_mut()
    {
        *slot = Some(DesignComponentInsertConstruction {
            relation_record_index: 167,
            carrier_record_index: 166,
            occurrence_identity: Some(17),
            neutron_role: "cccccccc-dddd-eeee-ffff-000000000000".into(),
            neutron_role_offset: 259,
            placement: None,
        });
    }

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#167"),
                record_index: 167,
                class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned())
                    .unwrap(),
                byte_offset: 0,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#169"),
                record_index: 169,
                class_tag: crate::records::references::DesignClassTag::try_from("410".to_owned())
                    .unwrap(),
                byte_offset: 100,
            },
        ]);
        native.design_parameter_scopes.push(scope);
    }

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(!findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));

    // A decoded envelope can retain a family without its specialized operation.
    let mut envelope_only = ir.clone();
    if let crate::records::feature::scope::DesignScopePayloadMut::ComponentInsert(slot) =
        f3d_native_mut(&mut envelope_only).design_parameter_scopes[0].payload_mut()
    {
        *slot = None;
    }
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &envelope_only).unwrap()
    });
    assert!(!findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));

    f3d_native_mut(&mut ir).design_parameter_scopes[0].paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(findings.iter().any(|finding| {
        finding.entity.as_deref() == Some(scope_id.as_str())
            && finding.message == "Fusion Design parameter scope has an invalid paired frame"
    }));
}

#[test]
fn validation_accepts_only_the_class_397_symmetric_extent_frame() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            extrude::{
                DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
                DesignExtrudeStart,
            },
            scope::{DesignExtrudeScope, DesignFeatureKind, DesignParameterScope},
        },
        identity::ReferenceRun,
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#0");
    let mut scope = DesignParameterScope::empty(&scope_id, DesignFeatureKind::Extrude, 3970);
    scope.class_tag = "397".to_owned().try_into().unwrap();
    scope.paired_class_tag = "262".to_owned().try_into().unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 473;
            draft.paired_byte_offset = 473;
            draft.reference_count_offset = 282;
            draft.reference_members = ReferenceRun::from_columns(
                vec![11, 22, 33, 44, 55, 66, 77, 88],
                vec![287, 298, 309, 320, 331, 342, 353, 364],
                "reference_members",
            )
            .unwrap();
            draft.kind_offset = 382;
            draft.feature_ordinal_offset = 395;
            draft.payload = DesignScopePayload::Extrude(Some(DesignExtrudeScope {
                extrude_prologue: Some(DesignExtrudePrologue::LegacyShifted {
                    operation_prefix_marker_offset: None,
                    operation: DesignExtrudeOperation::Cut,
                    operation_offset: 27,
                    direction_face_extend_values: [3, 2],
                    side_extent_discriminators: [1, 1],
                    side_extent_discriminator_offsets: [126, 139],
                    extent: Some(DesignExtrudeExtent::SymmetricDistance),
                    direction_face_extend_offsets: [31, 35],
                    direction_reversed: false,
                    direction_reversed_offset: 39,
                    solid_operation: true,
                    solid_operation_offset: 40,
                    start: DesignExtrudeStart::OffsetProfilePlane,
                    start_offset: 41,
                }),
                ..DesignExtrudeScope::default()
            }));
        })
        .unwrap();
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        for record_index in [11, 22, 33, 44, 55, 66, 77, 88, 3970] {
            native.design_record_headers.push(DesignRecordHeader {
                id: format!("{stream}:design-record-header#{record_index}"),
                record_index,
                class_tag: "397".to_owned().try_into().unwrap(),
                byte_offset: 0,
            });
        }
        native.design_parameter_scopes.push(scope.clone());
    }
    let has_invalid_frame = |ir: &cadmpeg_ir::CadIr| {
        crate::test_support::with_decode_context(|ctx| {
            crate::validate::validate_native(ctx, ir).expect("validate the native namespace")
        })
        .iter()
        .any(|finding| {
            finding.entity.as_deref() == Some(scope_id.as_str())
                && finding.message == "Fusion Design parameter scope has an invalid paired frame"
        })
    };
    assert!(!has_invalid_frame(&ir));

    let mutations: &[fn(&mut DesignParameterScope)] = &[
        |scope| scope.class_tag = "338".to_owned().try_into().unwrap(),
        |scope| scope.paired_class_tag = "261".to_owned().try_into().unwrap(),
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                side_extent_discriminators,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *side_extent_discriminators = [1, 0];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                direction_face_extend_values,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *direction_face_extend_values = [3, 1];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted {
                side_extent_discriminator_offsets,
                ..
            }) = scope.extrude_prologue_mut()
            {
                *side_extent_discriminator_offsets = [116, 129];
            }
        },
        |scope| {
            if let Some(DesignExtrudePrologue::LegacyShifted { extent, .. }) =
                scope.extrude_prologue_mut()
            {
                *extent = Some(DesignExtrudeExtent::TwoSidedDistance);
            }
        },
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let mut invalid = scope.clone();
        mutate(&mut invalid);
        f3d_native_mut(&mut ir).design_parameter_scopes[0] = invalid;
        assert!(
            has_invalid_frame(&ir),
            "accepted invalid frame mutation {index}"
        );
    }
}

#[test]
fn validation_requires_timeline_items_to_resolve_through_the_type_table() {
    let meta_stream = "f3d:FusionAssetName[Active]/Design1/MetaStream.dat";
    let bulk_entry = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let design_type = |id: &str, type_guid: &str, entities: Vec<u64>| {
        crate::records::entity_header::SegmentType::try_new(
            id.into(),
            crate::records::entity_header::SegmentTypeData {
                byte_offset: id.rsplit_once('#').unwrap().1.parse().unwrap(),
                type_guid: type_guid.to_owned().try_into().expect("type GUID"),
                type_guid_offset: 4,
                base_type_guid: if type_guid
                    == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID
                {
                    crate::records::entity_header::BaseTypeGuid::Guid {
                        value: crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID
                            .to_owned()
                            .try_into()
                            .expect("base GUID"),
                        offset: 8,
                    }
                } else {
                    crate::records::entity_header::BaseTypeGuid::Absent
                },
                version: if type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID {
                    crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1]
                } else {
                    1
                },
                version_offset: 44,
                module: crate::records::entity_header::DESIGN_MODULE_FUSION.into(),
                entities: crate::records::identity::ReferenceRun::located(
                    entities
                        .into_iter()
                        .map(|value| crate::records::identity::Located { value, offset: 100 })
                        .collect(),
                ),
            },
        )
        .unwrap()
    };
    let mut native = crate::native::F3dNative {
        design_types: vec![
            design_type(
                &format!("{meta_stream}:design-type#0"),
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                vec![35],
            ),
            design_type(
                &format!("{meta_stream}:design-type#1"),
                "11111111-2222-3333-4444-555555555555",
                vec![17, 101],
            ),
        ],
        design_feature_timelines: vec![
            crate::records::entity_header::DesignFeatureTimeline::try_new(
                crate::ids::native_design_feature_timeline_id(bulk_entry, 200),
                crate::records::entity_header::DesignTimelineFrame::new(
                    200,
                    60,
                    220,
                    240,
                    vec![crate::records::identity::Located {
                        value: 101,
                        offset: 245,
                    }],
                )
                .unwrap(),
                crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
                std::num::NonZeroU64::new(35).unwrap(),
                0,
                std::num::NonZeroU64::new(17).unwrap(),
            )
            .unwrap(),
        ],
        ..crate::native::F3dNative::default()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("f3d"),
        )
        .unwrap();
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(
        !findings.iter().any(|finding| {
            finding.message.contains("feature timeline")
                || finding.message.contains("feature-timeline")
        }),
        "{findings:#?}"
    );

    let mut duplicate_type_owner = native.clone();
    let mut entities = duplicate_type_owner.design_types[1]
        .entities
        .located_rows()
        .unwrap()
        .to_vec();
    entities.push(crate::records::identity::Located {
        value: 35,
        offset: 108,
    });
    duplicate_type_owner.design_types[1]
        .set_entities(crate::records::identity::ReferenceRun::located(entities));
    duplicate_type_owner
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("f3d"),
        )
        .unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.entity.as_deref()
            == Some(
                duplicate_type_owner.design_feature_timelines[0]
                    .id()
                    .as_str(),
            )
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));

    native.design_feature_timelines[0] =
        crate::records::entity_header::DesignFeatureTimeline::try_new(
            native.design_feature_timelines[0].id().clone(),
            crate::records::entity_header::DesignTimelineFrame::new(
                200,
                60,
                220,
                240,
                vec![crate::records::identity::Located {
                    value: 102,
                    offset: 245,
                }],
            )
            .unwrap(),
            native.design_feature_timelines[0].class_tag.clone(),
            native.design_feature_timelines[0].record_index,
            native.design_feature_timelines[0].source_ordinal,
            native.design_feature_timelines[0].context_record_index,
        )
        .unwrap();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("f3d"),
        )
        .unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.entity.as_deref() == Some(native.design_feature_timelines[0].id().as_str())
            && finding.message == "Fusion Design feature timeline has an invalid typed frame"
    }));
}

#[test]
fn validation_accepts_carrier_local_component_references() {
    use crate::records::feature::assembly_features::DesignComponentOccurrence;

    const COMPONENT: &str = "11111111-2222-4333-8444-555555555555";
    let occurrence = |record_index: u32,
                      byte_offset: u64,
                      component_record_index: u64,
                      occurrence_guid: &str| {
        DesignComponentOccurrence::try_new(
            crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
                id: format!("f3d:Design/BulkStream.dat:design-component-occurrence#{record_index}"),
                class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                    .unwrap(),
                record_index,
                byte_offset,
                component_record_index,
                component_guid: COMPONENT.to_owned().try_into().expect("GUID"),
                occurrence_guid: occurrence_guid.to_owned().try_into().expect("GUID"),
                placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
                owner_entity: 0,
            },
        )
        .unwrap()
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_component_occurrences.extend([
            occurrence(100, 1_000, 700, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"),
            occurrence(101, 2_000, 701, "aaaaaaaa-bbbb-4ccc-8ddd-ffffffffffff"),
        ]);
    }

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("component occurrence")));
}

#[test]
fn validation_scopes_direct_body_operand_ordinals_by_owning_scope() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            combine::{DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation},
            scope::DesignParameterScope,
        },
        recipes::{ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector},
        topology::{
            body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
            body_recipe::DesignOperandOwner,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut scopes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    let mut operands = Vec::new();
    for ordinal in 0..3u32 {
        let scope_record_index = 10 + ordinal;
        let operand_record_index = 100 + ordinal * 10;
        let byte_offset = 1_000 + u64::from(ordinal) * 1_000;
        let recipe_id = format!("{stream}:construction-recipe#{ordinal}");
        let empty_legacy_tool = ordinal == 0;
        let hole_scope = ordinal == 2;
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{scope_record_index}"),
            if hole_scope {
                crate::records::feature::scope::DesignFeatureKind::Hole
            } else {
                crate::records::feature::scope::DesignFeatureKind::Combine
            },
            scope_record_index,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(if hole_scope {
                        vec![1, 2, 3, 4, 5, 6, operand_record_index]
                    } else {
                        vec![1, 2, 3, 4, 5, operand_record_index]
                    });
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
            scope.payload_mut()
        {
            *slot = (!hole_scope).then_some(DesignCombineOperation {
                form: DesignCombineForm::Standard,
                operation: cadmpeg_ir::features::BooleanKind::Join,
                operation_offset: 0,
                keep_tools: false,
                keep_tools_offset: 0,
                target_record_index: if empty_legacy_tool {
                    operand_record_index + 1
                } else {
                    operand_record_index
                },
                target_body_node: None,
                tools: crate::records::feature::combine::DesignCombineTools {
                    first: DesignCombineBodySelection {
                        record_index: if empty_legacy_tool {
                            operand_record_index
                        } else {
                            operand_record_index + 1
                        },
                        external_identity: None,
                        body_node: None,
                    },
                    additional: vec![],
                },
            });
        }
        scopes.push(scope);
        headers.push(DesignRecordHeader {
            id: format!("{stream}:design-record-header#{operand_record_index}"),
            record_index: operand_record_index,
            class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                .unwrap(),
            byte_offset,
        });
        recipes.push(ConstructionRecipe {
            id: recipe_id.clone(),
            byte_offset: byte_offset + 220,
            kind: ConstructionRecipeKind::Body,
            design: Some(crate::records::recipes::ConstructionRecipeDesign {
                id: crate::records::identity::RecordedValue {
                    value: "301".into(),
                    offset: byte_offset + 197,
                },
                selector: Some(ConstructionRecipeSelector {
                    value: operand_record_index + 4,
                    byte_offset: byte_offset + 200,
                }),
            }),
            recipe_index: ordinal,
            record_index: Some(crate::records::identity::RecordedValue {
                value: i32::try_from(operand_record_index + 3).unwrap(),
                offset: 0,
            }),
        });
        operands.push(
            DesignBodyRecipeOperand::try_new(
                crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                    id: format!("{stream}:design-body-recipe-operand#{operand_record_index}"),
                    scope_record_index,
                    owner: DesignOperandOwner::ScopeReference {
                        scope_reference_ordinal: if hole_scope { 6 } else { 5 },
                    },
                    record_index: operand_record_index,
                    byte_offset,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "365".to_owned(),
                    )
                    .unwrap(),
                    asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                        "11111111-1111-4111-8111-111111111111".to_owned(),
                    )
                    .unwrap(),
                    asset_id_offset: byte_offset + if empty_legacy_tool { 44 } else { 56 },
                    context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                        "22222222-2222-4222-8222-222222222222".to_owned(),
                    )
                    .unwrap(),
                    context_id_offset: byte_offset + if empty_legacy_tool { 124 } else { 136 },
                    selector_tail: None,

                    references: if empty_legacy_tool {
                        Vec::new()
                    } else {
                        vec![DesignBodyRecipeReference {
                            design_reference: u64::from(300 + ordinal),
                            design_reference_offset: byte_offset + 25,
                            form: 3,
                            form_offset: byte_offset + 33,
                            candidate_faces: Vec::new(),
                            preceding_candidate_faces: Vec::new(),
                            preceding_body_slots: Vec::new(),
                        }]
                    },
                    nested_record_index: u64::from(operand_record_index + 3),
                    nested_record_index_offset: byte_offset
                        + if empty_legacy_tool { 26 } else { 38 },
                    recipe_id,
                    resolved_face_slot: None,
                    resolved_body_state_id: None,
                    resolved_body_slot: None,
                    resolved_body_face_slots: Vec::new(),
                    next_record_index: operand_record_index + 4,
                    next_byte_offset: byte_offset + 300,
                },
            )
            .unwrap(),
        );
    }
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes = scopes;
        native.design_record_headers = headers;
        native.construction_recipes = recipes;
        native.design_body_recipe_operands = operands;
    }

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    let invalid_operands = findings
        .iter()
        .filter(|finding| {
            finding.message == "Fusion Design body recipe operand has an invalid nested frame"
        })
        .collect::<Vec<_>>();
    assert!(invalid_operands.is_empty(), "{invalid_operands:#?}");
}

#[test]
fn validation_accepts_hole_and_surface_trim_construction_group_roles() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::DesignParameterScope,
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::feature::scope::DesignFeatureKind::Hole,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 101, 200, 201]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = |record_index: u32,
                 scope_reference_ordinal: u32,
                 member: u32,
                 byte_offset: u64,
                 role: DesignOperandRole| {
        let role_offset = byte_offset + 40;
        DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: format!("{stream}:design-construction-operand-group#{record_index}"),
                scope_record_index: 10,
                scope_reference_ordinal,
                record_index,
                byte_offset,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                members: vec![crate::records::identity::Located {
                    value: member,
                    offset: byte_offset + 26,
                }],
                lost_edge_references: Vec::new(),
                frame: DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: byte_offset + 21,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: Vec::new(),
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: role_offset + 18,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: role_offset + 22,
                        variant: false,
                        closing_flag: false,
                    },
                )
                .unwrap(),
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(role),
                role_offset,
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "258".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: byte_offset + 80,
            },
        )
        .unwrap()
    };
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native.design_construction_operand_groups.extend([
            group(100, 0, 101, 1_000, DesignOperandRole::BODIES_A),
            group(200, 2, 201, 2_000, DesignOperandRole::ROLE_0X5),
        ]);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#100"),
                record_index: 100,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#101"),
                record_index: 101,
                class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                    .unwrap(),
                byte_offset: 1_100,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#200"),
                record_index: 200,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                byte_offset: 2_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#201"),
                record_index: 201,
                class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                    .unwrap(),
                byte_offset: 2_100,
            },
        ]);
    }

    let invalid_frame = |finding: &cadmpeg_ir::report::check::Finding| {
        finding.message == "Fusion Design construction operand group has an invalid frame"
    };
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_frame));

    // A Hole body group can use role 0x8 for its whole-body recipe operand.
    f3d_native_mut(&mut ir).design_construction_operand_groups[1].operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_B,
        );
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_frame));

    f3d_native_mut(&mut ir).design_construction_operand_groups[1].operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_frame));

    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes[0]
            .try_edit(|draft| {
                draft.payload = crate::records::feature::scope::DesignFeatureKind::SurfaceTrim
                    .try_into()
                    .unwrap();
            })
            .unwrap();
        native.design_construction_operand_groups[1].operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X21,
            );
    }
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_frame));

    f3d_native_mut(&mut ir).design_construction_operand_groups[1].operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_B,
        );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_frame));
}

#[test]
fn validation_checks_generic_sweep_group_carriers() {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::{
            extrude::DesignExtrudeOperation, path_features::DesignPathFeatureConstruction,
            scope::DesignParameterScope,
        },
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let scope_id = format!("{stream}:design-parameter-scope#10");
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut scope = DesignParameterScope::empty(
        &scope_id,
        crate::records::feature::scope::DesignFeatureKind::Sweep,
        10,
    );
    {
        let value = Some(DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: 0,
                values: crate::test_support::reals([0.0; 6]).to_vec(),
                record_indexes: vec![11, 12, 13, 14, 15, 16],
                value_offsets: vec![0; 6],
            },
        ));
        scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![1, 2, 3, 4, 20, 21]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let path_group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("{stream}:design-construction-operand-group#20"),
            scope_record_index: 10,
            scope_reference_ordinal: 4,
            record_index: 20,
            byte_offset: 1_000,
            class_tag: crate::records::references::DesignClassTag::try_from("312".to_owned())
                .unwrap(),
            members: Vec::new(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_058,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_062,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 1_040,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "258".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1_100,
        },
    )
    .unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native.design_construction_operand_groups.push(path_group);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#20"),
                record_index: 20,
                class_tag: crate::records::references::DesignClassTag::try_from("312".to_owned())
                    .unwrap(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: format!("{stream}:design-record-header#21"),
                record_index: 21,
                class_tag: crate::records::references::DesignClassTag::try_from("316".to_owned())
                    .unwrap(),
                byte_offset: 1_200,
            },
        ]);
    }

    let group_native_finding_count = |ir: &cadmpeg_ir::CadIr| {
        crate::test_support::with_decode_context(|ctx| {
            crate::validate::validate_native(ctx, ir).expect("validate the native namespace")
        })
        .iter()
        .filter(|finding| {
            finding.entity.as_deref()
                == Some("f3d:Design/BulkStream.dat:design-construction-operand-group#20")
                && finding.check == cadmpeg_ir::report::check::Check::NativeLinks
        })
        .count()
    };
    // The empty group violates both its counted frame and its typed-member
    // carrier invariant. The validator reports those independent failures.
    assert_eq!(group_native_finding_count(&ir), 2);

    {
        let mut native = f3d_native_mut(&mut ir);
        let group = &mut native.design_construction_operand_groups[0];
        group
            .try_set_members(
                group
                    .members()
                    .iter()
                    .copied()
                    .chain([crate::records::identity::Located {
                        value: 21,
                        offset: 1_026,
                    }])
                    .collect(),
            )
            .unwrap();
    }
    // The synthetic carrier is only a record header. No typed edge operand
    // exists yet, so the independent carrier finding remains.
    assert_eq!(group_native_finding_count(&ir), 1);

    // Both body roles are admitted Sweep roles; a role outside the Sweep set
    // adds the frame finding.
    f3d_native_mut(&mut ir).design_construction_operand_groups[0].operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_B,
        );
    assert_eq!(group_native_finding_count(&ir), 1);

    f3d_native_mut(&mut ir).design_construction_operand_groups[0].operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X10,
        );
    assert_eq!(group_native_finding_count(&ir), 2);
}

/// A sketch point identity is unique within its Sketch. Distinct curve records
/// of one Sketch can carry one `(primary, secondary)` identity pair.
#[test]
fn validation_rejects_shared_point_identities_and_accepts_shared_curve_identities() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        let source_id = native.sketch_points[0]
            .persistent_id()
            .expect("generated point identity");
        let mut form = native.sketch_points[1].record_form().clone().into_raw();
        let crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
            persistent_id, ..
        } = &mut form
        else {
            panic!("generated point has a version-11 record form");
        };
        *persistent_id = std::num::NonZeroU64::new(source_id).unwrap();
        native.sketch_points[1].try_set_record_form(form).unwrap();
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(100);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(100);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.entity.as_deref() == Some(point_id.as_str())
            && finding.message.contains("persistent identity")
    }));
    assert!(!findings.iter().any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding.message.contains("persistent identity")
    }));
}

#[test]
fn validation_accepts_sketch_geometry_persistent_identities_reused_by_another_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let (point_id, curve_id) = {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        let source_id = native.sketch_points[0]
            .persistent_id()
            .expect("generated point identity");
        let mut form = native.sketch_points[1].record_form().clone().into_raw();
        let crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
            persistent_id, ..
        } = &mut form
        else {
            panic!("generated point has a version-11 record form");
        };
        *persistent_id = std::num::NonZeroU64::new(source_id).unwrap();
        native.sketch_points[1].try_set_record_form(form).unwrap();
        native.sketch_points[0].owner_reference = Some(100);
        native.sketch_points[1].owner_reference = Some(101);
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = Some(100);
        native.sketch_curve_identities[1].owner_reference = Some(101);
        (
            native.sketch_points[1].id.clone(),
            native.sketch_curve_identities[1].id.clone(),
        )
    };

    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && (finding.entity.as_deref() == Some(point_id.as_str())
                || finding.entity.as_deref() == Some(curve_id.as_str()))
            && finding.message.contains("persistent identity")
    }));
}

#[test]
fn validation_accepts_sketch_geometry_identities_with_unknown_owner() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    {
        let mut native = f3d_native_mut(&mut ir);
        assert!(native.sketch_points.len() >= 2);
        assert!(native.sketch_curve_identities.len() >= 2);
        let source_id = native.sketch_points[0]
            .persistent_id()
            .expect("generated point identity");
        let mut form = native.sketch_points[1].record_form().clone().into_raw();
        let crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
            persistent_id, ..
        } = &mut form
        else {
            panic!("generated point has a version-11 record form");
        };
        *persistent_id = std::num::NonZeroU64::new(source_id).unwrap();
        native.sketch_points[1].try_set_record_form(form).unwrap();
        native.sketch_points[0].owner_reference = None;
        native.sketch_points[1].owner_reference = None;
        native.sketch_curve_identities[1].primary_id = native.sketch_curve_identities[0].primary_id;
        native.sketch_curve_identities[1].secondary_id =
            native.sketch_curve_identities[0].secondary_id;
        native.sketch_curve_identities[0].owner_reference = None;
        native.sketch_curve_identities[1].owner_reference = None;
    }

    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.message.contains("persistent identity")
    }));
}

#[test]
fn validation_rejects_aliased_sketch_geometry_records() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let curve_id = {
        let mut native = f3d_native_mut(&mut ir);
        let point_record_index = native.sketch_points[0].record_index;
        native.sketch_curve_identities[0].record_index = point_record_index;
        native.sketch_curve_identities[0].id.clone()
    };

    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.entity.as_deref() == Some(curve_id.as_str())
            && finding
                .message
                .contains("aliases another typed indexed record")
    }));
}

#[test]
fn validation_rejects_duplicate_design_entity_suffixes() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let duplicate_id = {
        let mut native = f3d_native_mut(&mut ir);
        let mut duplicate = native
            .design_entity_headers
            .first()
            .expect("generated Design entity header")
            .clone();
        duplicate.id.push_str("-duplicate");
        duplicate.entity_id = crate::records::identity::DesignEntityId::from_parts(
            "duplicate",
            duplicate.entity_id.suffix(),
        );
        let id = duplicate.entity_id.clone();
        native.design_entity_headers.push(duplicate);
        id
    };

    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.entity.as_deref() == Some(duplicate_id.as_str())
            && finding.message.contains("entity suffix is duplicated")
    }));
}

#[test]
fn validation_accepts_user_design_parameter_frame() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut ir, _, _) = decoded.into_parts();
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: "f3d:generated:design-parameter#0".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 900,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::User {
                source_kind: cadmpeg_core::nonblank_literal!("User Parameter"),
                family_discriminator: Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 122,
                }),
            },
            expression: "60 mm".into(),
            expression_offset: 136,
            source_kind_offset: 166,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 210,
            }),
            name: "Width".into(),
            name_offset: 220,
            evaluated_value: 6.0,
            evaluated_value_offset: 234,
        },
    )
    .unwrap();
    f3d_native_mut(&mut ir).design_parameters.push(parameter);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .is_empty());
}

#[test]
fn validation_accepts_legacy_owner_frames_and_ownerless_class_287_parameters() {
    use crate::records::{
        decal::DesignRecordHeader,
        parameters::{DesignParameterCompanion, DesignParameterOwner},
    };

    const DESIGN_STREAM: &str = "Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let owned_parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: crate::ids::native_design_parameter_id(DESIGN_STREAM, 101),
            byte_offset: 1_068,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 101,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "Feature Input".into(),
                Some(100),
                None,
            )
            .unwrap(),
            expression: "6 cm".into(),
            expression_offset: 1_080,
            source_kind_offset: 1_100,

            unit: Some(crate::records::identity::RecordedValue {
                value: "cm".into(),
                offset: 1_120,
            }),
            name: "Length".into(),
            name_offset: 1_130,
            evaluated_value: 6.0,
            evaluated_value_offset: 1_140,
        },
    )
    .unwrap();
    let owner =
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: crate::ids::native_design_parameter_owner_id(DESIGN_STREAM, 1_000),
            byte_offset: 1_000,
            frame_length: 68,
            class_tag: crate::records::references::DesignClassTag::try_from("268".to_owned())
                .unwrap(),
            record_index: 100,
            scope_record_index: 0,
            local_ordinal: 0,
            evaluated_value: 6.0,
            evaluated_value_offset: owned_parameter.evaluated_value_offset(),
            parameter_record_index: 101,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 102,
        })
        .unwrap();
    let companion = DesignParameterCompanion::unbound(
        format!(
            "{}:design-parameter-companion#1200",
            crate::ids::native_scope(DESIGN_STREAM)
        ),
        1_200,
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap(),
        102,
        100,
        std::num::NonZeroU64::new(1).unwrap(),
        1_242,
    )
    .bound(crate::records::parameters::DesignCompanionPayload::new(
        1_258,
        0,
        Vec::new(),
    ));
    let ownerless_parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: crate::ids::native_design_parameter_id(DESIGN_STREAM, 201),
            byte_offset: 1_400,
            class_tag: crate::records::references::DesignClassTag::try_from("287".to_owned())
                .unwrap(),
            record_index: 201,
            source_ordinal: 1,
            source: crate::records::parameters::DesignParameterSource::new(
                "Feature Input".into(),
                Some(200),
                None,
            )
            .unwrap(),
            expression: "OffsetX".into(),
            expression_offset: 1_440,
            source_kind_offset: 1_470,

            unit: None,
            name: "OffsetX".into(),
            name_offset: 1_490,
            evaluated_value: 0.0,
            evaluated_value_offset: 1_510,
        },
    )
    .unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native
            .design_parameters
            .extend([owned_parameter, ownerless_parameter]);
        native.design_parameter_owners.push(owner);
        native.design_parameter_companions.push(companion);
        native.design_record_headers.extend([
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 100),
                record_index: 100,
                class_tag: crate::records::references::DesignClassTag::try_from("268".to_owned())
                    .unwrap(),
                byte_offset: 1_000,
            },
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 101),
                record_index: 101,
                class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                    .unwrap(),
                byte_offset: 1_068,
            },
            DesignRecordHeader {
                id: crate::ids::native_scoped_id(DESIGN_STREAM, "record-header", 102),
                record_index: 102,
                class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned())
                    .unwrap(),
                byte_offset: 1_200,
            },
        ]);
    }

    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(
        findings.iter().all(|finding| {
            !finding
                .message
                .contains("Fusion Design parameter owner has an invalid frame")
                && !finding
                    .message
                    .contains("Fusion Design parameter has an invalid frame")
        }),
        "{findings:#?}"
    );
}

#[test]
fn validation_accepts_grouped_and_direct_extrude_profiles() {
    use crate::records::{
        feature::{
            extrude::{
                DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue,
                DesignExtrudeStart,
            },
            scope::DesignParameterScope,
        },
        topology::{
            construction::DesignConstructionOperandGroup,
            sketch_profile::DesignSketchProfileOperand,
        },
    };

    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let profile = DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 200,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 230,
            entity_id: crate::records::identity::DesignEntityId::try_from("0_10".to_owned())
                .expect("valid entity identity"),
            entity_reference_offset: 250,
            region_selection: None,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "260".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 300,
        },
    )
    .unwrap();
    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:test:scope#10".into(),
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 10,
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
                    extrude_profile: Some(profile),
                    ..crate::records::feature::scope::DesignExtrudeScope::default()
                },
            )),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 220,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: Some(228),
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![20, 30],
                vec![184, 195],
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
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:test:operand-group#30".into(),
            scope_record_index: 10,
            scope_reference_ordinal: 1,
            record_index: 30,
            byte_offset: 400,
            class_tag: crate::records::references::DesignClassTag::try_from("302".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 20,
                offset: 424,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 420,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 31,
                        offset: 440,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 468,
                    opaque_scalar: 0.5,
                    opaque_scalar_offset: 472,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile,
            role_offset: 450,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "262".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 500,
        },
    )
    .unwrap();
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_parameter_scopes.push(scope);
        native
            .design_construction_operand_groups
            .push(group.clone());
    }
    let profile_message = |finding: &cadmpeg_ir::report::check::Finding| {
        finding.message == "Fusion Design Extrude profile conflicts with its profile operand group"
    };
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    });
    assert!(!findings.iter().any(profile_message));
    assert!(!findings
        .iter()
        .any(|finding| finding.message.contains("no counted selection group")));

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .push(group);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(profile_message));

    let profile = f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_mut()
        .unwrap()
        .extrude_profile
        .take();
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(profile_message));
    f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_mut()
        .unwrap()
        .extrude_profile = profile;

    f3d_native_mut(&mut ir)
        .design_construction_operand_groups
        .clear();
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(profile_message));

    f3d_native_mut(&mut ir).design_parameter_scopes[0]
        .extrude_profile_mut()
        .expect("test Extrude profile")
        .scope_reference_ordinal = 1;
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(profile_message));
}

#[test]
fn validation_accepts_unindexed_construction_identity_terminal() {
    use crate::records::{
        decal::DesignRecordHeader,
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
            construction::DesignConstructionOperandIdentity,
            construction::DesignConstructionPersistentIdentity,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: format!("{stream}:operand-group#100"),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: crate::records::references::DesignClassTag::try_from("271".to_owned())
                .unwrap(),
            members: Vec::new(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_021,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 101,
                        offset: 1_025,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_059,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_063,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::from_raw(0),
                ),
            role_offset: 1_041,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1_050,
        },
    )
    .unwrap();
    let identity = DesignConstructionOperandIdentity::try_new(
        crate::records::topology::construction::DesignConstructionOperandIdentityDraft {
            id: format!("{stream}:operand-identity#1100"),
            group_record_index: 100,
            wrappers: vec![crate::records::topology::construction::DesignIdentityWrapper {
                record_index: 101,
                byte_offset: 1_100,
                class_tag: crate::records::references::DesignClassTag::try_from("384".to_owned())
                    .unwrap(),
            }],
            following_record_index: 102,
            following_byte_offset: 1_124,
            following_class_tag: crate::records::references::DesignClassTag::try_from(
                "395".to_owned(),
            )
            .unwrap(),
            tracking_path: None,
            persistent_identity: Some(
                DesignConstructionPersistentIdentity::try_new(
                    crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                        local_id: 167,
                        local_id_offset: 1_145,
                        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "2d0697b6-f6c5-4f86-bb58-4a2f413c99d3".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 1_157,
                        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "9dea94a1-729a-4032-930b-d4ba4eaadb0c".to_owned(),
                        )
                        .unwrap(),
                        context_id_offset: 1_233,
                        tail_slot_present: false,
                        tail_slot_offset: 1_309,
                        next_record_index: 103,
                        next_byte_offset: 1_314,
                    },
                )
                .unwrap(),
            ),
        },
    )
    .unwrap();
    let wrapper = DesignRecordHeader {
        id: format!("{stream}:record-header#1100"),
        record_index: 101,
        class_tag: crate::records::references::DesignClassTag::try_from("384".to_owned()).unwrap(),
        byte_offset: 1_100,
    };
    let following = DesignRecordHeader {
        id: format!("{stream}:record-header#1124"),
        record_index: 102,
        class_tag: crate::records::references::DesignClassTag::try_from("395".to_owned()).unwrap(),
        byte_offset: 1_124,
    };
    let identity_id = identity.id.clone();
    let mut native = crate::native::F3dNative::default();
    native.design_construction_operand_groups.push(group);
    native.design_construction_operand_identities.push(identity);
    native.design_record_headers.extend([wrapper, following]);
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("f3d"),
        )
        .unwrap();

    let invalid_identity = |finding: &cadmpeg_ir::report::check::Finding| {
        finding.entity.as_deref() == Some(identity_id.as_str())
            && finding.message.contains("invalid nested frame")
    };
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_identity));

    let mut native = crate::native::F3dNative::load(ir.native.namespace("f3d").unwrap()).unwrap();
    native.design_record_headers.push(DesignRecordHeader {
        id: format!("{stream}:record-header#1315"),
        record_index: 103,
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 1_315,
    });
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            ir.native.namespace_mut("f3d"),
        )
        .unwrap();
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_identity));
}

#[test]
fn validation_accepts_class_338_sketch_curve_entity_selection_frame() {
    use crate::records::{
        decal::DesignRecordHeader,
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame,
            entity_selection::DesignEntitySelectionOperand,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let group_id = format!("{stream}:design-construction-operand-group#100");
    let operand_id = format!("{stream}:design-entity-selection-operand#1000");
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id,
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 971,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 975,
                    variant: false,
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile,
            role_offset: 953,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "265".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let header = DesignRecordHeader {
        id: format!("{stream}:design-record-header#1000"),
        byte_offset: 1_000,
        class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned()).unwrap(),
        record_index: 200,
    };
    let operand = DesignEntitySelectionOperand::try_new(
        crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
            id: operand_id.clone(),
            scope_record_index: 10,
            group_record_index: 100,
            group_member_ordinal: 0,
            record_index: 200,
            byte_offset: 1_000,
            class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "11111111-2222-4333-8444-555555555555".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 1_034,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned(),
            )
            .unwrap(),
            context_id_offset: 1_100,
            identity_record_index: 203,
            identity_record_offset: 2_000,
            primary_identity: 949,
            primary_identity_offset: 2033,
            secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                identity: crate::records::identity::Located {
                    value: 249,
                    offset: 2041,
                },
                curve_identity: None,
            }),
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: 204,
            next_byte_offset: 2049,
        },
    )
    .unwrap();
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    {
        let mut native = f3d_native_mut(&mut ir);
        native.design_construction_operand_groups.push(group);
        native.design_record_headers.push(header);
        native.design_entity_selection_operands.push(operand);
    }

    let invalid_entity_selection = |finding: &cadmpeg_ir::report::check::Finding| {
        finding.entity.as_deref() == Some(operand_id.as_str())
            && finding.message
                == "Fusion Design entity-selection operand has an invalid nested frame"
    };
    assert!(!crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .iter()
    .any(invalid_entity_selection));
}

mod face_recipe_lengths;

mod rigid_alignment;

mod work_plane;

#[test]
fn loft_accepts_either_body_group_role() {
    use crate::records::feature::extrude::DesignExtrudeOperation;
    use crate::records::topology::extrude_selection::DesignOperandRole;
    for body in [DesignOperandRole::BODIES_A, DesignOperandRole::BODIES_B] {
        let groups = [
            (body, 1),
            (DesignOperandRole::PROFILE, 1),
            (DesignOperandRole::PROFILE, 1),
        ];
        assert!(super::loft_operand_roles_are_valid(
            DesignExtrudeOperation::Join,
            &groups
        ));
        assert!(!super::loft_operand_roles_are_valid(
            DesignExtrudeOperation::NewBody,
            &groups
        ));
    }
}

mod profile_curves;
