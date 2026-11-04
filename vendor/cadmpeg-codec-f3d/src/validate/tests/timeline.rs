// SPDX-License-Identifier: Apache-2.0

fn design_type(
    id: &str,
    type_guid: &str,
    entities: &[u64],
) -> crate::records::entity_header::SegmentType {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, DESIGN_MODULE_FUSION};
    let is_timeline = type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID;
    SegmentType::try_new(
        id.into(),
        crate::records::entity_header::SegmentTypeData {
            byte_offset: id.rsplit_once('#').unwrap().1.parse().unwrap(),
            type_guid: type_guid.to_owned().try_into().unwrap(),
            type_guid_offset: 4,
            base_type_guid: if is_timeline {
                BaseTypeGuid::Guid {
                    value: crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 8,
                }
            } else {
                BaseTypeGuid::Absent
            },
            version: if is_timeline {
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1]
            } else {
                1
            },
            version_offset: 44,
            module: DESIGN_MODULE_FUSION.into(),
            entities: crate::records::identity::ReferenceRun::located(
                entities
                    .iter()
                    .map(|value| crate::records::identity::Located {
                        value: *value,
                        offset: 100,
                    })
                    .collect(),
            ),
        },
    )
    .unwrap()
}

fn native() -> crate::native::F3dNative {
    use crate::records::{
        entity_header::{DesignFeatureTimeline, DesignTimelineFrame},
        identity::Located,
        references::DesignClassTag,
    };
    let meta = "f3d:FusionAssetName[Active]/Design1/MetaStream.dat";
    let bulk = "FusionAssetName[Active]/Design1/BulkStream.dat";
    crate::native::F3dNative {
        design_types: vec![
            design_type(
                &format!("{meta}:design-type#0"),
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                &[35],
            ),
            design_type(
                &format!("{meta}:design-type#1"),
                "11111111-2222-3333-4444-555555555555",
                &[17, 101],
            ),
        ],
        design_feature_timelines: vec![DesignFeatureTimeline::try_new(
            crate::ids::native_design_feature_timeline_id(bulk, 200),
            DesignTimelineFrame::new(
                200,
                60,
                220,
                240,
                vec![Located {
                    value: 101,
                    offset: 245,
                }],
            )
            .unwrap(),
            DesignClassTag::try_from("256".to_owned()).unwrap(),
            std::num::NonZeroU64::new(35).unwrap(),
            0,
            std::num::NonZeroU64::new(17).unwrap(),
        )
        .unwrap()],
        ..crate::native::F3dNative::default()
    }
}

#[test]
fn timeline_valid_records_keep_no_findings() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_feature_timelines(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

fn history_scope(
    record_index: u32,
    state: i64,
    previous: Option<i64>,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#{record_index}"),
        DesignFeatureKind::Hole,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = previous;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn forward_history_native() -> crate::native::F3dNative {
    use crate::records::{
        entity_header::{DesignFeatureTimeline, DesignTimelineFrame},
        identity::{Located, ReferenceRun},
    };
    let mut native = native();
    native.design_types[1].set_entities(ReferenceRun::located(vec![
        Located {
            value: 17,
            offset: 100,
        },
        Located {
            value: 101,
            offset: 108,
        },
        Located {
            value: 100,
            offset: 116,
        },
    ]));
    let timeline = &native.design_feature_timelines[0];
    native.design_feature_timelines[0] = DesignFeatureTimeline::try_new(
        timeline.id().clone(),
        DesignTimelineFrame::new(
            200,
            80,
            220,
            240,
            vec![
                Located {
                    value: 101,
                    offset: 245,
                },
                Located {
                    value: 100,
                    offset: 256,
                },
            ],
        )
        .unwrap(),
        timeline.class_tag.clone(),
        timeline.record_index,
        timeline.source_ordinal,
        timeline.context_record_index,
    )
    .unwrap();
    native
        .design_parameter_scopes
        .push(history_scope(100, 7, None));
    native
        .design_parameter_scopes
        .push(history_scope(101, 8, Some(7)));
    native
}

fn cyclic_history_native() -> crate::native::F3dNative {
    let mut native = native();
    native
        .design_parameter_scopes
        .push(history_scope(101, 9, Some(7)));
    native
        .design_parameter_scopes
        .push(history_scope(102, 7, Some(8)));
    native
        .design_parameter_scopes
        .push(history_scope(103, 8, Some(7)));
    native
}

#[test]
fn timeline_history_findings_keep_specific_messages() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for (native, message) in [
            (
                forward_history_native(),
                "Fusion Design history edge runs forward in its feature timeline",
            ),
            (
                cyclic_history_native(),
                "Fusion Design scope history-state dependency is cyclic",
            ),
        ] {
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
            let mut findings = Vec::new();
            super::super::validate_feature_timelines(&ctx, &mut findings).unwrap();
            assert!(findings.iter().any(|finding| finding.message == message));
        }
    })
}
