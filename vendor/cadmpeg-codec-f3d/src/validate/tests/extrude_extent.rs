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
