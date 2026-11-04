// SPDX-License-Identifier: Apache-2.0
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

use cadmpeg_ir::report::Severity;

use crate::loss::F3dLossCode;

/// A report carrying the BREP-less geometry losses that `build_container_report`
/// states before the design segment is classified.
fn brep_less_geometry_report() -> cadmpeg_ir::codec::DecodeBody {
    cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: vec![
            F3dLossCode::GeometryNotTransferred.note("stated before classification"),
            F3dLossCode::TopologyNotTransferred.note("stated before classification"),
            F3dLossCode::MissingGeometryStream.note("stated before classification"),
        ],
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    }
}

/// A design whose content is sketch curves declares no body, so it has no
/// B-rep to lose: the sketch entities are its complete geometry.
#[test]
fn sketch_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 13, 0);
    assert!(report.transfer.geometry_transferred());
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "sketch-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.message.contains("13 sketch entity(s)")));
}

/// A reference-image timeline object is presentation content. A bodyless
/// document that contains one does not require a BREP geometry carrier.
#[test]
fn presentation_only_design_is_not_a_geometry_loss() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 1);
    assert!(report.transfer.geometry_transferred());
    assert!(
        report
            .losses
            .iter()
            .all(|loss| loss.severity < Severity::Error),
        "presentation-only design must not keep blocking losses: {:?}",
        report.losses
    );
    assert!(report.losses.iter().any(|loss| loss
        .message
        .contains("1 reference-image timeline object(s)")));
}

/// A declared body whose BREP stream is absent is a real missing carrier. Its
/// sketches do not stand in for the solid the document says it has.
#[test]
fn a_declared_body_without_a_brep_stream_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 1, 13, 0);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A document with no sketch entities transferred nothing, so nothing settles
/// the loss. An imported drawing whose only entity the importer did not author
/// produces exactly this: a document with no body and no sketch.
#[test]
fn a_document_without_sketch_entities_keeps_its_geometry_losses() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 0, 0, 0, 0);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A present BREP stream that produced no geometry is a decode failure, not a
/// sketch-only design, however many sketches the document also carries.
#[test]
fn a_present_brep_stream_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 1, 0, 0, 13, 0);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

/// A text-encoded B-rep carrier is not sketch-only, regardless of sketch count.
#[test]
fn a_text_brep_carrier_is_never_reclassified_as_sketch_only() {
    let mut report = brep_less_geometry_report();
    crate::decode::apply_bodyless_design_classification(&mut report, 0, 2, 0, 13, 0);
    assert!(!report.transfer.geometry_transferred());
    assert_eq!(report.losses.len(), 3);
}

#[test]
fn empty_model_body_ownership_is_final_without_sketch_wire_ownership() {
    let wire = crate::test_support::smbh_geometry_test::synthetic_wire_body_smbh();
    let archive = crate::test_support::zip_test::f3d_with_smbh(&wire);
    let decoded = cadmpeg_ir::codec::Codec::decode(
        &crate::F3dCodec,
        &mut std::io::Cursor::new(archive),
        &cadmpeg_ir::codec::DecodeOptions::default(),
    )
    .unwrap();
    assert!(!decoded.ir().model.bodies.is_empty());
    for model_body_maps_empty in [true, false] {
        let path = super::super::FinalizePath::Bodyless(super::super::DeferredBodylessInputs {
            model_body_maps_empty,
            has_appearance: false,
            mesh_projection: super::super::MeshProjection {
                count: 0,
                tessellations_by_scope: std::collections::HashMap::new(),
            },
        });
        let bodies = path.component_bodies(&decoded.ir().model.bodies, false);
        if model_body_maps_empty {
            assert!(bodies.unwrap().is_empty());
        } else {
            assert!(bodies.is_none());
        }
        assert!(path
            .component_bodies(&decoded.ir().model.bodies, true)
            .is_none());
    }
}

#[test]
fn component_body_coverage_uses_saved_edge_origin_and_keeps_ordinary_bodies() {
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};
    let bodies = [
        ("f3d:brep:entity#1", BodyKind::Solid),
        ("f3d:brep:entity#2", BodyKind::Sheet),
        ("f3d:brep:entity#3", BodyKind::Wire),
        ("f3d:brep:saved-edge-body#4", BodyKind::Wire),
        ("f3d:brep/part.smbh/brep:saved-edge-body#5", BodyKind::Wire),
    ]
    .into_iter()
    .map(|(id, kind)| Body {
        id: BodyId::mint(id).unwrap(),
        kind,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    })
    .collect::<Vec<_>>();
    let path = super::super::FinalizePath::Geometry(super::super::GeometryIndex {
        primary_model_brep_name: "BREP.source.smbh".to_owned(),
        annotation_records: Vec::new(),
        mesh_projection: super::super::MeshProjection {
            count: 0,
            tessellations_by_scope: std::collections::HashMap::new(),
        },
    });
    let required = path.component_bodies(&bodies, true).unwrap();
    let expected = bodies[..3]
        .iter()
        .map(|body| body.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(required, expected);
    assert_eq!(bodies.len(), 5);
    let mut model = cadmpeg_ir::document::Model::default();
    model.bodies.extend(bodies);
    let loss = super::super::saved_free_edge_bodies_loss(&model).unwrap();
    assert!(loss.message.starts_with("2 saved free-edge bodies"));
    let definition: cadmpeg_ir::products::ProductDefinition =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:design:product-definition#1", "kind": "part", "bodies": []
        }))
        .unwrap();
    model.product_definitions.push(definition);
    let loss = super::super::saved_free_edge_bodies_loss(&model).unwrap();
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::SavedFreeEdgeBodyUnowned.kind()
    );
    assert!(loss.message.starts_with("2 saved free-edge bodies"));
    model.product_definitions[0].bodies = model.bodies[3..]
        .iter()
        .map(|body| body.id.clone())
        .collect();
    assert!(super::super::saved_free_edge_bodies_loss(&model).is_none());
}
