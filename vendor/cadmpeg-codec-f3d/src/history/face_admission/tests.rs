// SPDX-License-Identifier: Apache-2.0
//! Historical selection membership and Hole namespace admission.
use super::FaceInputs;
use cadmpeg_ir::features::{DistinctMembers, FaceSelection, FeatureId, FeatureInputTopology};
use cadmpeg_ir::ids::FaceId;

fn input(feature: &FeatureId, slot: i64) -> FeatureInputTopology {
    FeatureInputTopology {
        id: crate::ids::feature_input_topology_id(feature, 4),
        input_of: feature.clone(),
        bodies: Default::default(),
        faces: DistinctMembers::try_from(vec![crate::ids::history_input_face_id(
            &crate::ids::history_input_prefix(&feature.key(), 4),
            slot,
        )])
        .unwrap(),
        edges: Default::default(),
        vertices: Default::default(),
        edge_geometry: Vec::new(),
        native_ref: Some("f3d:Design/BREP.body:asm-history-state#4".into()),
    }
}
fn historical(input: &FeatureInputTopology, slot: i64) -> FaceSelection {
    FaceSelection::historical(
        input.id.clone(),
        vec![crate::ids::history_input_face_id(
            &crate::ids::history_input_prefix(&input.input_of.key(), 4),
            slot,
        )],
        "f3d:design:face-operand#1".into(),
    )
    .unwrap()
}
fn resolved(face: &str) -> FaceSelection {
    FaceSelection::Resolved {
        faces: vec![FaceId::mint(face).unwrap()],
        native: "f3d:design:face-operand#1".into(),
    }
}

#[test]
fn resolved_face_operations_require_current_or_emitted_historical_membership() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation};
    let id = FeatureId::mint("f3d:model:feature#face-edit").unwrap();
    let topology = input(&id, 2);
    for topologies in [vec![], vec![topology.clone()]] {
        let mut features = [Feature {
            product_definition: None,
            id: id.clone(),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            native_ref: None,
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::DeleteFace {
                    faces: resolved("f3d:brep:entity#2"),
                    heal: true,
                },
            )),
        }];
        let mut losses = Vec::new();
        super::admit_feature_input_faces(&mut features, &topologies, &[], &mut losses).unwrap();
        let FeatureDefinition::Operation(FeatureOperation::DeleteFace { faces, .. }) =
            features[0].evaluation.definition()
        else {
            panic!("face edit")
        };
        if topologies.is_empty() {
            assert!(matches!(faces, FaceSelection::Native(_)));
            assert_eq!(losses.len(), 1);
        } else {
            assert_eq!(faces, &historical(&topology, 2));
            assert!(losses.is_empty());
        }
    }
}

#[test]
fn historical_faces_require_one_emitted_feature_state_and_every_selected_member() {
    let feature = FeatureId::mint("f3d:model:feature#input").unwrap();
    let topology = input(&feature, 2);
    let admitted = historical(&topology, 2);
    for topologies in [
        vec![],
        vec![topology.clone()],
        vec![topology.clone(), topology.clone()],
    ] {
        let mut inputs = FaceInputs::new(&topologies, &[]);
        let mut selection = admitted.clone();
        let mut losses = Vec::new();
        inputs.admit(&feature, &mut selection, &mut losses);
        if topologies.len() == 1 {
            assert_eq!(selection, admitted);
            assert!(losses.is_empty());
        } else {
            assert_eq!(
                selection,
                FaceSelection::Native("f3d:design:face-operand#1".into())
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code.local_code(),
                "history.face-selection-unbound"
            );
        }
    }
    let topologies = [topology.clone()];
    let mut inputs = FaceInputs::new(&topologies, &[]);
    let mut selection = historical(&topology, 3);
    let mut losses = Vec::new();
    inputs.admit(&feature, &mut selection, &mut losses);
    assert!(matches!(selection, FaceSelection::Native(_)));
    assert_eq!(losses.len(), 1);
    let mut selection = historical(&topology, 2);
    inputs.admit(
        &FeatureId::mint("f3d:model:feature#other").unwrap(),
        &mut selection,
        &mut losses,
    );
    assert!(matches!(selection, FaceSelection::Native(_)));
}

#[test]
fn hole_uses_emitted_historical_topology_for_a_face_absent_from_current_model() {
    let feature = FeatureId::mint("f3d:model:feature#hole").unwrap();
    let topologies = [input(&feature, 2)];
    let mut inputs = FaceInputs::new(&topologies, &[]);
    let mut selection = resolved("f3d:brep:entity#2");
    let mut losses = Vec::new();
    inputs.hole(&feature, &mut selection, &mut losses).unwrap();
    assert_eq!(selection, historical(&topologies[0], 2));
    assert!(losses.is_empty());
    let mut qualified = resolved("f3d:brep/body/brep:entity#2");
    inputs.hole(&feature, &mut qualified, &mut losses).unwrap();
    assert_eq!(qualified, historical(&topologies[0], 2));
    assert!(losses.is_empty());
    for face in ["f3d:brep:entity#3", "f3d:brep/foreign/brep:entity#2"] {
        let mut selection = resolved(face);
        inputs.hole(&feature, &mut selection, &mut losses).unwrap();
        assert_eq!(
            selection,
            FaceSelection::Native("f3d:design:face-operand#1".into())
        );
    }
    assert_eq!(losses.len(), 2);
}

#[test]
fn hole_keeps_a_surviving_current_face_and_refuses_an_unemitted_history() {
    let feature = FeatureId::mint("f3d:model:feature#hole").unwrap();
    let topologies = [input(&feature, 2)];
    let face = FaceId::mint("f3d:brep:entity#2").unwrap();
    let current = [cadmpeg_ir::topology::Face {
        id: face.clone(),
        shell: cadmpeg_ir::ids::ShellId::mint("test:brep:shell#1").unwrap(),
        surface: cadmpeg_ir::ids::SurfaceId::mint("test:brep:surface#1").unwrap(),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
    }];
    let mut inputs = FaceInputs::new(&topologies, &current);
    let original = resolved(face.as_str());
    let mut selection = original.clone();
    let mut losses = Vec::new();
    inputs.hole(&feature, &mut selection, &mut losses).unwrap();
    assert_eq!(selection, original);
    assert!(losses.is_empty());
    let mut inputs = FaceInputs::new(&[], &current);
    inputs.hole(&feature, &mut selection, &mut losses).unwrap();
    assert_eq!(selection, original);
    assert!(losses.is_empty());
    let mut inputs = FaceInputs::new(&topologies, &current);
    inputs.hole(&feature, &mut selection, &mut losses).unwrap();
    assert_eq!(selection, original);
    assert!(losses.is_empty());
    let mut inputs = FaceInputs::new(&[], &[]);
    inputs.hole(&feature, &mut selection, &mut losses).unwrap();
    assert!(matches!(selection, FaceSelection::Native(_)));
    assert_eq!(losses.len(), 1);
}

#[test]
fn feature_admission_keeps_extrude_face_operands_native_without_an_input_topology() {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
        FeatureDefinition, FeatureEvaluation, FeatureOperation, LinearTermination,
        PlanarProfileRef, ProfileRef,
    };
    let id = FeatureId::mint("f3d:model:feature#extrude").unwrap();
    let topology = input(&id, 2);
    let mut features = [Feature {
        product_definition: None,
        id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Extrude".into()),
        source_text: None,
        source_content: Default::default(),
        native_ref: Some("f3d:design:scope#extrude".into()),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile: ProfileRef::Planar(
                    PlanarProfileRef::historical_faces(
                        topology.id.clone(),
                        vec![crate::ids::history_input_face_id(
                            &crate::ids::history_input_prefix(&topology.input_of.key(), 4),
                            2,
                        )],
                        vec!["f3d:design:profile#1".into(), "f3d:design:profile#2".into()],
                    )
                    .unwrap(),
                ),
                direction: ExtrudeDirection::default(),
                start: ExtrudeStart::FromFace {
                    face: historical(&topology, 2),
                    offset: None,
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ToFace {
                            face: historical(&topology, 2),
                            offset: None,
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )),
    }];
    let mut admitted = features.clone();
    let mut admitted_losses = Vec::new();
    super::admit_feature_input_faces(
        &mut admitted,
        std::slice::from_ref(&topology),
        &[],
        &mut admitted_losses,
    )
    .unwrap();
    assert_eq!(
        admitted[0].evaluation.definition(),
        features[0].evaluation.definition()
    );
    assert!(admitted_losses.is_empty());
    let mut losses = Vec::new();
    super::admit_feature_input_faces(&mut features, &[], &[], &mut losses).unwrap();
    assert!(
        matches!(features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Extrude {
        profile: ProfileRef::Planar(PlanarProfileRef::Native(scope)),
        start: ExtrudeStart::FromFace {face: FaceSelection::Native(_), ..},
        extent: ExtrudeExtent::OneSided {side: ExtrudeSide {termination: LinearTermination::ToFace {face: FaceSelection::Native(_), ..}, ..}},
        ..
    }) if scope == "f3d:design:scope#extrude")
    );
    assert_eq!(losses.len(), 3);
    let mut untouched_losses = Vec::new();
    super::admit_feature_input_faces(
        &mut features,
        std::slice::from_ref(&topology),
        &[],
        &mut untouched_losses,
    )
    .unwrap();
    assert!(untouched_losses.is_empty());
}

#[test]
fn feature_gate_changes_hole_namespace() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation};
    let id = FeatureId::mint("f3d:model:feature#hole").unwrap();
    let topology = input(&id, 2);
    let original = Feature {
        product_definition: None,
        id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Hole".into()),
        source_text: None,
        source_content: Default::default(),
        native_ref: Some("f3d:design:scope#hole".into()),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: Some(resolved("f3d:brep:entity#2")),
                direction: None,
                placements: None,
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::form(
                        cadmpeg_ir::features::holes::HoleKind::Simple,
                    ),
                    None,
                    Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
                )
                .unwrap(),
                extent: None,
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            },
        )),
    };
    let mut features = [original.clone()];
    let mut losses = Vec::new();
    super::admit_feature_input_faces(
        &mut features,
        std::slice::from_ref(&topology),
        &[],
        &mut losses,
    )
    .unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        face: Some(face), ..
    }) = features[0].evaluation.definition()
    else {
        panic!("Hole face was removed");
    };
    assert_eq!(face, &historical(&topology, 2));
    assert!(losses.is_empty());
}

#[test]
fn repeated_membership_queries_reuse_only_the_topologies_they_need() {
    let first = FeatureId::mint("f3d:model:feature#first").unwrap();
    let second = FeatureId::mint("f3d:model:feature#second").unwrap();
    let unused = FeatureId::mint("f3d:model:feature#unused").unwrap();
    let mut topologies = [input(&first, 2), input(&second, 3), input(&unused, 4)];
    topologies[2].faces = DistinctMembers::try_from(
        (0..128)
            .map(|slot| {
                crate::ids::history_input_face_id(
                    &crate::ids::history_input_prefix(&unused.key(), 4),
                    slot,
                )
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let original = [historical(&topologies[0], 2), historical(&topologies[1], 3)];
    let mut inputs = FaceInputs::new(&topologies, &[]);
    let mut losses = Vec::new();
    for _ in 0..1_000 {
        for (feature, original) in [(&first, &original[0]), (&second, &original[1])] {
            let mut selection = original.clone();
            inputs.admit(feature, &mut selection, &mut losses);
            assert_eq!(&selection, original);
        }
    }
    assert!(losses.is_empty());
}
