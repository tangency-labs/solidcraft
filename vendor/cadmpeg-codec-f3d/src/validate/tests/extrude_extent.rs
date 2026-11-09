// SPDX-License-Identifier: Apache-2.0
//! Extrude extents carried by fixed scalar lanes.

use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

const CONFLICT: &str =
    "Fusion Design Extrude start or extent conflicts with its parameters and face operands";

fn distance(
    value: f64,
    record_index: u32,
) -> DesignFixedExtrudeScalar<cadmpeg_ir::scalar::NonZeroReal> {
    DesignFixedExtrudeScalar {
        value: cadmpeg_ir::scalar::NonZeroReal::new(value).unwrap(),
        record_index,
        value_offset: u64::from(record_index) * 100,
    }
}

/// A new-body distance Extrude without indexed parameters whose fixed lanes
/// are `fixed`.
fn fixed_extrude(
    extent: DesignExtrudeExtent,
    fixed: DesignFixedExtrudeParameters,
) -> crate::native::F3dNative {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Extrude,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
        scope.payload_mut()
    {
        let extrude = slot.get_or_insert_with(Default::default);
        extrude.extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 28,
            direction_face_extend_values: [2, 0],
            side_extent_discriminators: [1, 1],
            side_extent_discriminator_offsets: [77, 90],
            first_side_target_ordinal: None,
            distance_tail: None,
            extent,
            direction_face_extend_offsets: [32, 36],
            direction_reversed: false,
            direction_reversed_offset: 40,
            solid_operation: true,
            solid_operation_offset: 41,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 42,
        });
        extrude.fixed_extrude_parameters = Some(fixed);
    }
    crate::native::F3dNative {
        design_parameter_scopes: vec![scope],
        ..Default::default()
    }
}

fn conflicts(native: &crate::native::F3dNative) -> bool {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_parameter_operands(&ctx, &mut findings);
        findings.iter().any(|finding| finding.message == CONFLICT)
    })
}

#[test]
fn geometry_depth_does_not_require_omitted_extrude_face_groups() {
    let native = fixed_extrude(
        DesignExtrudeExtent::OneSidedToFace,
        DesignFixedExtrudeParameters {
            along_distance: None,
            taper_angle: None,
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        },
    );
    assert!(conflicts(&native));
    crate::test_support::with_decode_context(|decode| {
        let mut value = serde_json::to_value(cadmpeg_ir::CadIr::empty()).unwrap();
        value["decode_depth"] = serde_json::json!("geometry");
        let ir = serde_json::from_value(value).unwrap();
        let ctx = super::super::Ctx::new(&ir, &native, decode);
        let mut findings = Vec::new();
        super::super::validate_extrude_parameter_operands(&ctx, &mut findings);
        assert!(!findings.iter().any(|finding| finding.message == CONFLICT));
    });
}

/// Fixed first-side and second-side distance lanes carry a two-sided distance
/// Extrude. Without the second-side lane the extent has no second distance.
#[test]
fn two_sided_distance_extrude_accepts_fixed_distance_lanes() {
    let lanes = |against_distance| DesignFixedExtrudeParameters {
        along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
        taper_angle: None,
        against_distance,
        side_two_taper_angle: None,
        profile_offset: None,
        side_one_offset: None,
        ..Default::default()
    };
    assert!(!conflicts(&fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        lanes(Some(distance(0.2, 17))),
    )));
    assert!(conflicts(&fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        lanes(None),
    )));
}

/// An offset-start distance Extrude whose start-offset lane is also the
/// `ProfileOffset` parameter owner has one start-offset carrier.
#[test]
fn offset_start_lane_that_is_also_the_parameter_owner_counts_once() {
    use crate::records::{
        identity::RecordedValue,
        parameters::{
            DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
            DesignParameterSource,
        },
    };
    let lane = |record_index| DesignFixedExtrudeScalar {
        value: crate::test_support::real(0.5),
        record_index,
        value_offset: u64::from(record_index) * 100,
    };
    let native = |owned: bool| {
        let mut native = fixed_extrude(
            DesignExtrudeExtent::OneSidedDistance,
            DesignFixedExtrudeParameters {
                along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
                taper_angle: Some(lane(14)),
                against_distance: None,
                side_two_taper_angle: None,
                profile_offset: Some(lane(17)),
                side_one_offset: None,
                ..Default::default()
            },
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(Some(extrude)) =
            native.design_parameter_scopes[0].payload_mut()
        {
            if let Some(DesignExtrudePrologue::ReferenceAware { start, .. }) =
                &mut extrude.extrude_prologue
            {
                *start = DesignExtrudeStart::OffsetProfilePlane;
            }
        }
        if owned {
            let base = 20_000;
            native.design_parameters.push(
                DesignParameter::try_from(DesignParameterDraft {
                    id: "f3d:Design/BulkStream.dat:design-parameter#19".into(),
                    byte_offset: base,
                    class_tag: "305".to_owned().try_into().unwrap(),
                    record_index: 19,
                    source_ordinal: 0,
                    source: DesignParameterSource::new("ProfileOffset".to_owned(), Some(17), None)
                        .unwrap(),
                    expression: "5 mm".into(),
                    expression_offset: base + 12,
                    source_kind_offset: base + 32,
                    unit: Some(RecordedValue {
                        value: "mm".into(),
                        offset: base + 52,
                    }),
                    name: "d1".into(),
                    name_offset: base + 62,
                    evaluated_value: 0.5,
                    evaluated_value_offset: base + 72,
                })
                .unwrap(),
            );
            native.design_parameter_owners.push(
                DesignParameterOwner::try_from(DesignParameterOwnerWire {
                    id: "f3d:Design/BulkStream.dat:design-parameter-owner#17".into(),
                    byte_offset: 1_700,
                    frame_length: 99,
                    class_tag: "268".to_owned().try_into().unwrap(),
                    record_index: 17,
                    scope_record_index: 10,
                    local_ordinal: 0,
                    evaluated_value: 0.5,
                    evaluated_value_offset: 1_740,
                    parameter_record_index: 19,
                    owned_ordinal: 0,
                    variant: None,
                    companion_record_index: 18,
                })
                .unwrap(),
            );
        }
        native
    };
    assert!(!conflicts(&native(false)));
    assert!(!conflicts(&native(true)));
}

#[test]
fn two_sided_through_all_requires_absent_distance_lanes() {
    use crate::layout::class_349_two_sided_through_all_extrude_frame as layout;
    use crate::records::identity::{RecordedValue, ReferenceRun};
    use crate::records::parameters::{
        DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
        DesignParameterSource,
    };
    let mut native = fixed_extrude(
        DesignExtrudeExtent::TwoSidedThroughAll,
        DesignFixedExtrudeParameters::default(),
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(Some(extrude)) =
        native.design_parameter_scopes[0].payload_mut()
    {
        extrude.fixed_extrude_parameters = None;
    }
    native.design_parameter_scopes[0]
        .try_edit(|draft| {
            draft.frame_length = cadmpeg_core::decode::u64_from_index(layout::LEN);
            draft.paired_byte_offset = draft.frame_length;
            draft.reference_count_offset =
                cadmpeg_core::decode::u64_from_index(layout::REFERENCE_COUNT);
            draft.reference_members = ReferenceRun::unlocated(vec![
                101, 102, 103, 104, 105, 106, 131, 108, 109, 134, 111,
            ]);
            draft.layout_fixture_references();
            draft.feature_ordinal_offset =
                cadmpeg_core::decode::u64_from_index(layout::FEATURE_ORDINAL);
        })
        .unwrap();
    for (local_ordinal, record, kind) in [(0u32, 131u32, "TaperAngle"), (1, 134, "Side2TaperAngle")]
    {
        let base = 20_000 + u64::from(record) * 200;
        native.design_parameters.push(
            DesignParameter::try_from(DesignParameterDraft {
                id: format!("f3d:Design/BulkStream.dat:design-parameter#{}", record + 1),
                byte_offset: base,
                class_tag: "305".to_owned().try_into().unwrap(),
                record_index: record + 1,
                source_ordinal: local_ordinal,
                source: DesignParameterSource::new(kind.to_owned(), Some(record), None).unwrap(),
                expression: "0 rad".into(),
                expression_offset: base + 12,
                source_kind_offset: base + 40,
                unit: Some(RecordedValue {
                    value: "rad".into(),
                    offset: base + 100,
                }),
                name: format!("d{local_ordinal}"),
                name_offset: base + 120,
                evaluated_value: 0.0,
                evaluated_value_offset: base + 160,
            })
            .unwrap(),
        );
        let owner_base = u64::from(record) * 100;
        native.design_parameter_owners.push(
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record}"),
                byte_offset: owner_base,
                frame_length: 103,
                class_tag: "268".to_owned().try_into().unwrap(),
                record_index: record,
                scope_record_index: 10,
                local_ordinal,
                evaluated_value: 0.0,
                evaluated_value_offset: owner_base + 40,
                parameter_record_index: record + 1,
                owned_ordinal: local_ordinal,
                variant: None,
                companion_record_index: record + 2,
            })
            .unwrap(),
        );
    }
    assert!(!conflicts(&native));
    let last = native.design_parameter_owners.pop().unwrap();
    assert!(conflicts(&native));
    native.design_parameter_owners.push(last);
    assert!(!conflicts(&native));
    let with_distance = fixed_extrude(
        DesignExtrudeExtent::TwoSidedThroughAll,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
            ..Default::default()
        },
    );
    assert!(conflicts(&with_distance));
    assert!(conflicts(&fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignFixedExtrudeParameters::default(),
    )));
}

/// DR-322: start-support byte 2 with a `ProfileOffset` and no face group.
#[test]
fn a_new_body_distance_extrude_can_store_a_selected_face_start_without_a_group() {
    let native = |operation: DesignExtrudeOperation, offset: bool| {
        let lane = DesignFixedExtrudeScalar {
            value: crate::test_support::real(0.5),
            record_index: 17,
            value_offset: 1_700,
        };
        let mut native = fixed_extrude(
            DesignExtrudeExtent::OneSidedDistance,
            DesignFixedExtrudeParameters {
                along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
                profile_offset: offset.then_some(lane),
                ..Default::default()
            },
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(Some(extrude)) =
            native.design_parameter_scopes[0].payload_mut()
        {
            if let Some(DesignExtrudePrologue::ReferenceAware {
                start,
                operation: stored,
                ..
            }) = &mut extrude.extrude_prologue
            {
                *start = DesignExtrudeStart::FromFace;
                *stored = operation;
            }
        }
        native
    };
    assert!(!conflicts(&native(DesignExtrudeOperation::NewBody, true)));
    // Other operations still need the start group, and the offset stays required.
    assert!(conflicts(&native(DesignExtrudeOperation::Join, true)));
    assert!(conflicts(&native(DesignExtrudeOperation::NewBody, false)));
}
