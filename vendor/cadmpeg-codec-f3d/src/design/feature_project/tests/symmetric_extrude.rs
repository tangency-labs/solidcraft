// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_extrude;
use crate::design::test_support::parameter_record;
use crate::ids::neutral_sketch_id;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::scope::DesignScopePayload;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;

#[test]
fn dual_lane_symmetric_extrude_doubles_whole_travel() {
    crate::test_support::with_decode_context(|decode_ctx| {
        use cadmpeg_ir::features::{
            BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, LinearTermination,
            PlanarProfileRef, ProfileRef,
        };

        let parameter = |source_kind: &str, unit: &str, value| {
            parse_design_parameter_record(&parameter_record(
                Some(44),
                "value",
                source_kind,
                Some(unit),
                "d1",
                value,
            ))
            .expect("generated feature parameter is canonical")
        };
        let scope = DesignParameterScope::try_new(
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
                        direction_face_extend_values: [3, 2],
                        side_extent_discriminators: [1, 1],
                        side_extent_discriminator_offsets: [177, 190],
                        first_side_target_ordinal: None,
                        extent: DesignExtrudeExtent::SymmetricDistance,
                        direction_face_extend_offsets: [132, 136],
                        direction_reversed: false,
                        direction_reversed_offset: 140,
                        solid_operation: true,
                        solid_operation_offset: 141,
                        start: DesignExtrudeStart::ProfilePlane,
                        start_offset: 142,
                    }),
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
                    ..crate::records::feature::scope::DesignExtrudeScope::default()
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
    .unwrap();
        let placement = DesignSketchPlacement {
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                600,
                crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, -1.0, 0.0, 0.0],
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

            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            record_index: 200,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "260".to_owned(),
            )
            .unwrap(),
        };
        let along = parameter("AlongDistance", "mm", 0.55);
        let taper = parameter("TaperAngle", "deg", 0.2);
        let symmetric = project_extrude(
            decode_ctx,
            &scope,
            &[(0, &along), (1, &taper)],
            &[],
            &[],
            std::slice::from_ref(&placement),
            &[],
        )
        .expect("project the extrude feature")
        .expect("typed dual-lane symmetric Extrude");
        // Each side travels the shared per-side input: the neutral whole
        // travel is twice the parameter.
        assert!(matches!(
            symmetric,
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(profile)),
                direction: ExtrudeDirection::ProfileNormal {},
                extent: ExtrudeExtent::Symmetric {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: actual_length
                        },
                        draft: Some(actual_draft),
                    },
                },
                op: BooleanOp::NewBody,
                solid: Some(true),
                ..
            }) if (profile == neutral_sketch_id(&placement)) && actual_length.get() == 11.0 && actual_draft.get() == 0.2
        ));
    });
}
