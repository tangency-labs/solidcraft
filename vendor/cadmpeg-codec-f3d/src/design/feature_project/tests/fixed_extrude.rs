// SPDX-License-Identifier: Apache-2.0
//! Extrude extents carried only by fixed scalar lanes.

use crate::design::feature_project::project_extrude;
use crate::ids::neutral_sketch_id;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::features::{
    ExtrudeExtent, ExtrudeSide, FeatureDefinition, FeatureOperation, LinearTermination,
    PlanarProfileRef, ProfileRef,
};

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

fn placement() -> DesignSketchPlacement {
    DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            600,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            ),
        )
        .unwrap(),
        id: "f3d:Design/BulkStream.dat:placement#200".into(),
        scope_record_index: Some(11),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 200,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
            .unwrap(),
    }
}

/// A new-body Extrude of the whole Sketch `0_172` with the given extent and
/// fixed lanes and no indexed parameters.
fn fixed_extrude(
    extent: DesignExtrudeExtent,
    fixed: DesignFixedExtrudeParameters,
) -> DesignParameterScope {
    let direction = match extent {
        DesignExtrudeExtent::TwoSidedDistance => 2,
        DesignExtrudeExtent::SymmetricDistance => 3,
        _ => 1,
    };
    DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
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
                        direction_face_extend_values: [direction, 0],
                        side_extent_discriminators: [1, 1],
                        side_extent_discriminator_offsets: [177, 190],
                        first_side_target_ordinal: None,
                        extent,
                        direction_face_extend_offsets: [132, 136],
                        direction_reversed: false,
                        direction_reversed_offset: 140,
                        solid_operation: true,
                        solid_operation_offset: 141,
                        start: DesignExtrudeStart::ProfilePlane,
                        start_offset: 142,
                    }),
                    fixed_extrude_parameters: Some(fixed),
                    extrude_profile: Some(
                        DesignSketchProfileOperand::try_new(
                            crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                                scope_reference_ordinal: 0,
                                record_index: 100,
                                byte_offset: 300,
                                class_tag: crate::records::references::DesignClassTag::try_from(
                                    "308".to_owned(),
                                )
                                .unwrap(),
                                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                                    "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b".to_owned(),
                                )
                                .unwrap(),
                                asset_id_offset: 330,
                                entity_id: crate::records::identity::DesignEntityId::try_from(
                                    "0_172".to_owned(),
                                )
                                .expect("valid entity identity"),
                                entity_reference_offset: 420,
                                region_selection: None,
                                paired_class_tag:
                                    crate::records::references::DesignClassTag::try_from(
                                        "259".to_owned(),
                                    )
                                    .unwrap(),
                                paired_byte_offset: 520,
                            },
                        )
                        .unwrap(),
                    ),
                    ..Default::default()
                },
            )),
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 180,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![100],
                vec![185],
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
    .unwrap()
}

fn projected(scope: &DesignParameterScope) -> Option<FeatureDefinition> {
    let placement = placement();
    crate::test_support::with_decode_context(|decode_ctx| {
        project_extrude(
            decode_ctx,
            scope,
            &[],
            &[],
            &[],
            std::slice::from_ref(&placement),
            &[],
        )
    })
    .expect("project the extrude feature")
}

/// Fixed first-side and second-side lanes in centimetres define a two-sided
/// blind Extrude in millimetres.
#[test]
fn fixed_two_sided_distance_lanes_project_two_blind_sides() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
            taper_angle: None,
            against_distance: Some(distance(0.2, 17)),
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        },
    );
    let definition = projected(&scope).expect("typed two-sided Extrude");
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Sketch(profile)),
        extent:
            ExtrudeExtent::TwoSided {
                first:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length: first },
                        draft: None,
                    },
                second:
                    ExtrudeSide {
                        termination: LinearTermination::Blind { length: second },
                        draft: None,
                    },
            },
        ..
    }) = definition
    else {
        panic!("two-sided blind Extrude: {definition:?}");
    };
    assert_eq!(profile, neutral_sketch_id(&placement()));
    assert!((first.get() - 43.0).abs() < 1.0e-12);
    assert!((second.get() - 2.0).abs() < 1.0e-12);
}

/// A fixed symmetric distance lane does not state its measurement form, so it
/// defines no neutral extent (DR-78).
#[test]
fn fixed_symmetric_distance_lane_keeps_the_native_feature() {
    let scope = fixed_extrude(
        DesignExtrudeExtent::SymmetricDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(1.2, 11))),
            taper_angle: None,
            against_distance: None,
            side_two_taper_angle: None,
            profile_offset: None,
            side_one_offset: None,
            ..Default::default()
        },
    );
    assert!(projected(&scope).is_none());
}

#[test]
fn multiple_native_sketch_profiles_keep_the_extrude_native() {
    let mut scope = fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(1.0, 11))),
            against_distance: Some(distance(2.0, 17)),
            ..Default::default()
        },
    );
    assert!(projected(&scope).is_some());
    let first = scope.extrude_profile().unwrap().clone();
    let mut second = first.clone().into_draft();
    second.record_index = 200;
    second.entity_id = "0_173".to_owned().try_into().unwrap();
    let extrude = scope.extrude_mut().unwrap();
    extrude.extrude_profile = None;
    extrude.extrude_native_profiles = Some(vec![
        first,
        DesignSketchProfileOperand::try_new(second).unwrap(),
    ]);
    assert!(projected(&scope).is_none());
}
