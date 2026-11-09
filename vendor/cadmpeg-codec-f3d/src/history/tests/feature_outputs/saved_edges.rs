// SPDX-License-Identifier: Apache-2.0

use super::{history_in_source, native_base_feature};
use crate::history::bind_feature_outputs;
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::{FaceMotion, FaceSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::topology::{Body, BodyKind};

#[test]
fn saved_edges_are_recorded_apart_from_feature_outputs_in_both_brep_namespaces() {
    for id in [
        "f3d:brep:saved-edge-body#1",
        "f3d:brep/selected.smbh/brep:saved-edge-body#1",
    ] {
        let (mut feature, scope, _, mut body) = native_base_feature();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::MoveFace {
                faces: FaceSelection::Unresolved,
                motion: FaceMotion::Offset {
                    distance: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
                },
            }));
        body.id = cadmpeg_ir::ids::BodyId::mint(id).unwrap();
        body.kind = BodyKind::Wire;
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut [history_in_source("selected.smbh", &[1])],
            std::slice::from_ref(&body),
            &Default::default(),
            Some("BREP.selected.smbh"),
        )
        .unwrap();
        assert!(feature.evaluation.outputs().is_empty());
        assert_eq!(
            feature
                .source_properties
                .get("affected_saved_edges")
                .map(String::as_str),
            Some("1")
        );
        assert!(!losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::FeatureOutputBodyUnbound.kind()));
    }
}

#[test]
fn saved_edge_outside_the_declared_brep_source_keeps_the_unbound_loss() {
    for primary in [None, Some("BREP.other.smbh")] {
        let (mut feature, scope, _, mut body) = native_base_feature();
        body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:saved-edge-body#1").unwrap();
        body.kind = BodyKind::Wire;
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut [history_in_source("selected.smbh", &[1])],
            &[body],
            &Default::default(),
            primary,
        )
        .unwrap();
        assert!(feature.evaluation.outputs().is_empty());
        assert!(!feature
            .source_properties
            .contains_key("affected_saved_edges"));
        assert!(losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::FeatureOutputBodyUnbound.kind()));
    }
}

fn bind_slot_one(bodies: &[Body], primary: Option<&str>) -> (cadmpeg_ir::features::Feature, bool) {
    let (mut feature, scope, _, _) = native_base_feature();
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history_in_source("selected.smbh", &[1])],
        bodies,
        &Default::default(),
        primary,
    )
    .unwrap();
    let unbound = losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureOutputBodyUnbound.kind());
    (feature, unbound)
}

fn body_at(id: &str, kind: BodyKind) -> Body {
    let (_, _, _, mut body) = native_base_feature();
    body.id = cadmpeg_ir::ids::BodyId::mint(id).unwrap();
    body.kind = kind;
    body
}

#[test]
fn several_identities_at_one_slot_of_the_source_bind_nothing_and_report_the_loss() {
    for pair in [
        [
            body_at("f3d:brep:entity#1", BodyKind::Solid),
            body_at("f3d:brep:saved-edge-body#1", BodyKind::Wire),
        ],
        [
            body_at("f3d:brep:saved-edge-body#1", BodyKind::Wire),
            body_at(
                "f3d:brep/selected.smbh/brep:saved-edge-body#1",
                BodyKind::Wire,
            ),
        ],
    ] {
        let (feature, unbound) = bind_slot_one(&pair, Some("BREP.selected.smbh"));
        assert!(feature.evaluation.outputs().is_empty());
        assert!(!feature
            .source_properties
            .contains_key("affected_saved_edges"));
        assert!(unbound);
    }
}

#[test]
fn a_saved_edge_of_another_source_leaves_the_solid_at_its_slot_as_the_output() {
    let solid = body_at("f3d:brep:entity#1", BodyKind::Solid);
    let bodies = [
        solid.clone(),
        body_at("f3d:brep:saved-edge-body#1", BodyKind::Wire),
    ];
    let (feature, unbound) = bind_slot_one(&bodies, Some("BREP.other.smbh"));
    assert_eq!(feature.evaluation.outputs(), &[solid.id]);
    assert!(!feature
        .source_properties
        .contains_key("affected_saved_edges"));
    assert!(!unbound);
}
