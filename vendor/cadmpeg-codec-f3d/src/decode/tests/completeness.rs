// SPDX-License-Identifier: Apache-2.0
//! Completeness predicates for projected design feature definitions.

use super::super::feature_definition_is_incomplete;

#[test]
fn untyped_material_distances_charge_one_loss_without_fabricating_geometry() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    };

    super::super::report_untyped_material_distances(&mut report, 0);
    assert!(report.losses.is_empty());
    super::super::report_untyped_material_distances(&mut report, 2);

    assert_eq!(report.losses.len(), 1);
    assert_eq!(
        report.losses[0].code.local_code(),
        "material.distance-unit-untyped"
    );
}

#[test]
fn direct_datum_planes_are_complete_but_unresolved_frames_are_not() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};
    use cadmpeg_ir::math::{Point3, Vector3};

    let direct = FeatureDefinition::Operation(FeatureOperation::DatumPlane {
        frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    });
    assert!(!feature_definition_is_incomplete(&direct));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane
        })
    ));
}

#[test]
fn trim_surface_completeness_accepts_an_explicit_cell_selection() {
    let complete = cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::TrimSurface {
            faces: cadmpeg_ir::features::FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
                "f3d:test:face#target",
            )
            .expect("identity grammar")]),
            tool: cadmpeg_ir::features::PathRef::Curves(vec![cadmpeg_ir::ids::CurveId::mint(
                "f3d:test:curve#tool",
            )
            .expect("identity grammar")]),
            keep: cadmpeg_ir::features::TrimRegion::Cells(
                cadmpeg_ir::features::TrimCellSelection::new(vec![1, 4], 5).unwrap(),
            ),
        },
    );
    assert!(!feature_definition_is_incomplete(&complete));
}

#[test]
fn datum_axes_require_a_finite_nonzero_direction() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::DatumAxis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0))
                .unwrap(),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::DatumAxis {
            origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                f64::EPSILON / 2.0,
                0.0,
                0.0
            ))
            .unwrap(),
        })
    ));
}

#[test]
fn coordinate_systems_require_a_finite_right_handed_frame() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem {
            frame: cadmpeg_ir::features::FeatureCoordinateFrame::new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0)
            )
            .unwrap()
        })
    ));
}

#[test]
fn zero_body_base_features_are_complete_but_empty_insertions_are_not() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: Default::default(),
                native: "native:base-feature".into(),
            },
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Native("native:base-feature".into()),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::InsertBodies {
            bodies: cadmpeg_ir::features::InsertedBodies::Native("native:insert-bodies".into()),
        })
    ));
}

#[test]
fn replace_face_requires_resolved_target_and_replacement_faces() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::FaceId;

    let resolved = |name: &str| {
        FaceSelection::Faces(vec![
            FaceId::mint(format!("test:model:face#{name}")).expect("identity grammar")
        ])
    };
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                resolved("target"),
                resolved("replacement"),
            )
            .unwrap(),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                FaceSelection::Native("native:target".into()),
                resolved("replacement"),
            )
            .unwrap(),
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::ReplaceFace {
            operands: cadmpeg_ir::features::ReplaceFaceOperands::new(
                resolved("target"),
                FaceSelection::Native("native:replacement".into()),
            )
            .unwrap(),
        })
    ));
}

#[test]
fn remove_body_requires_resolved_bodies_and_a_retention_mode() {
    use cadmpeg_ir::features::{
        BodyRetentionMode, BodySelection, FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;

    let complete = FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Bodies(
            cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                "test:model:body#1",
            )
            .expect("identity grammar")])
            .expect("distinct bodies"),
        ),
        mode: BodyRetentionMode::DeleteSelected,
    });
    assert!(!feature_definition_is_incomplete(&complete));

    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Native("native:remove-body".into()),
            mode: BodyRetentionMode::DeleteSelected,
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                    "test:model:body#1"
                )
                .expect("identity grammar")],)
                .expect("distinct bodies")
            ),
            mode: BodyRetentionMode::Unresolved,
        })
    ));
}

#[test]
fn product_feature_definitions_require_neutral_reference_ids() {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::ids::OccurrenceId;
    use cadmpeg_ir::products::JointId;

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::InsertComponent {
            occurrence: OccurrenceId::mint("model:test:occurrence#component")
                .expect("identity grammar"),
        })
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
            joint: JointId::mint("model:test:joint#assembly").expect("identity grammar"),
        })
    ));
    assert!(OccurrenceId::mint(String::new()).is_err());
    assert!(JointId::mint(String::new()).is_err());
}

#[test]
fn direct_and_analytic_features_require_resolved_geometry_and_operands() {
    use cadmpeg_ir::features::{
        AxisAngle, BodySelection, BooleanOp, FaceMotion, FaceSelection, FeatureDefinition,
        FeatureOperation, ScaleCenter, ScaleFactors, ThickenSide,
    };
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::scalar::Length;

    let faces = FaceSelection::Faces(vec!["test:model:face#1"
        .try_into()
        .expect("valid identity")]);
    let bodies = BodySelection::Bodies(
        cadmpeg_ir::features::DistinctMembers::try_from(vec![
            BodyId::mint("test:model:body#1").expect("identity grammar")
        ])
        .expect("distinct bodies"),
    );

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::Sphere {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            radius: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
            op: BooleanOp::NewBody,
        })
    ));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::Torus {
            center: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
            axis: cadmpeg_ir::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            major_radius: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
            minor_radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap(),
            op: BooleanOp::Join,
        })
    ));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: faces.clone(),
            motion: FaceMotion::Offset {
                distance: Length::new(-2.0).unwrap(),
            },
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::MoveFace {
            faces: FaceSelection::Native("native:faces".into()),
            motion: FaceMotion::Offset {
                distance: Length::new(2.0).unwrap(),
            },
        })
    ));
    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::Thicken {
            faces: faces.clone(),
            thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
            side: Some(ThickenSide::Forward),
        })
    ));

    let shell = |bodies, removed_faces| {
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies,
            removed_faces,
            thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap()),
            outward: Some(true),
            mode: None,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        })
    };
    assert!(!feature_definition_is_incomplete(&shell(
        Some(bodies.clone()),
        FaceSelection::Faces(Vec::new()),
    )));
    assert!(!feature_definition_is_incomplete(&shell(
        None,
        FaceSelection::Faces(vec!["test:model:face#opening"
            .try_into()
            .expect("valid identity")]),
    )));
    assert!(feature_definition_is_incomplete(&shell(
        None,
        FaceSelection::Faces(Vec::new()),
    )));
    assert!(feature_definition_is_incomplete(&shell(
        Some(BodySelection::Native("native:bodies".into())),
        FaceSelection::Faces(Vec::new()),
    )));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: bodies.clone(),
            translation: cadmpeg_ir::features::FiniteVector3::new(Vector3::new(1.0, 2.0, 3.0))
                .unwrap(),
            rotation: Some(AxisAngle {
                origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .unwrap(),
                direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                    0.0, 0.0, 1.0
                ))
                .unwrap(),
                angle: cadmpeg_ir::scalar::Angle::new(0.5).unwrap(),
            }),
            copies: 0,
        })
    ));

    assert!(!feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::Scale {
            bodies: BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                    "test:model:body#scale"
                )
                .expect("identity grammar")],)
                .expect("distinct bodies")
            ),
            center: Some(ScaleCenter::ModelOrigin),
            factors: ScaleFactors::Uniform {
                factor: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap()
            },
        })
    ));
    assert!(feature_definition_is_incomplete(
        &FeatureDefinition::Operation(FeatureOperation::Scale {
            bodies: BodySelection::Bodies(
                cadmpeg_ir::features::DistinctMembers::try_from(vec![BodyId::mint(
                    "test:model:body#scale"
                )
                .expect("identity grammar")],)
                .expect("distinct bodies")
            ),
            center: Some(ScaleCenter::Native("native:center".into())),
            factors: ScaleFactors::Uniform {
                factor: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap()
            },
        })
    ));
}

#[test]
fn knit_surfaces_require_resolved_faces_and_operation_settings() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::scalar::NonNegativeLength;

    let complete = |faces, merge_entities, create_solid, gap_tolerance| {
        FeatureDefinition::Operation(FeatureOperation::KnitSurface {
            faces,
            merge_entities,
            create_solid,
            gap_tolerance,
        })
    };
    let faces = FaceSelection::Faces(vec!["test:model:face#1"
        .try_into()
        .expect("valid identity")]);

    assert!(!feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(true),
        Some(true),
        Some(NonNegativeLength::new(0.1).unwrap()),
    )));
    assert!(!feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(false),
        Some(false),
        Some(NonNegativeLength::new(0.1).unwrap()),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        FaceSelection::Native("native:surface-stitch".into()),
        Some(true),
        Some(true),
        Some(NonNegativeLength::new(0.1).unwrap()),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces.clone(),
        None,
        Some(true),
        Some(NonNegativeLength::new(0.1).unwrap()),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(true),
        None,
        Some(NonNegativeLength::new(0.1).unwrap()),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces.clone(),
        Some(true),
        Some(true),
        Some(NonNegativeLength::new(0.0).unwrap()),
    )));
    assert!(feature_definition_is_incomplete(&complete(
        faces,
        Some(true),
        Some(true),
        None,
    )));
}

#[test]
fn thread_is_complete_only_with_every_field_and_a_proven_modeled_state() {
    use cadmpeg_core::text::NonBlankString;
    use cadmpeg_ir::features::{
        CosmeticThreadExtent, FaceSelection, FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::PositiveLength;

    let resolved = || {
        FaceSelection::historical(
            cadmpeg_ir::ids::FeatureInputTopologyId::mint("test:model:feature-input#thread")
                .expect("identity grammar"),
            vec![cadmpeg_ir::ids::HistoricalFaceId::mint("test:model:face#1")
                .expect("identity grammar")],
            "test:model:native#thread".into(),
        )
        .expect("historical face selection")
    };
    let thread = |face: FaceSelection, modeled: Option<bool>, pitch: Option<PositiveLength>| {
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
            face,
            standard: NonBlankString::new("Profile".to_owned()),
            designation: NonBlankString::new("D5".to_owned()),
            diameter: PositiveLength::new(5.0),
            pitch,
            modeled,
            extent: Some(CosmeticThreadExtent::Through {}),
        })
    };
    let pitch = PositiveLength::new(0.8);
    assert!(!feature_definition_is_incomplete(&thread(
        resolved(),
        Some(false),
        pitch
    )));
    // An absent modeled state is a gap, never a default.
    assert!(feature_definition_is_incomplete(&thread(
        resolved(),
        None,
        pitch
    )));
    assert!(feature_definition_is_incomplete(&thread(
        FaceSelection::Native("test:model:native#thread".into()),
        Some(false),
        pitch
    )));
    assert!(feature_definition_is_incomplete(&thread(
        resolved(),
        Some(false),
        None
    )));
}

#[test]
fn a_shell_on_both_sides_needs_no_outward_direction() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation, ShellMode};
    let shell = |outward, mode| {
        FeatureDefinition::Operation(FeatureOperation::Shell {
            bodies: None,
            removed_faces: FaceSelection::Faces(vec![cadmpeg_ir::ids::FaceId::mint(
                "f3d:test:face#1",
            )
            .unwrap()]),
            thickness: cadmpeg_ir::scalar::PositiveLength::new(2.0),
            outward,
            mode,
            join: None,
            resolve_intersections: None,
            allow_self_intersections: None,
        })
    };
    assert!(!feature_definition_is_incomplete(&shell(
        None,
        Some(ShellMode::BothSides)
    )));
    assert!(!feature_definition_is_incomplete(&shell(Some(true), None)));
    assert!(feature_definition_is_incomplete(&shell(None, None)));
    assert!(feature_definition_is_incomplete(&shell(
        None,
        Some(ShellMode::Skin)
    )));
}
