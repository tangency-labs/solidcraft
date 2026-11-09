// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::F3dCodec;

#[test]
fn controller_manifest_feature_families_and_closed_bodies() {
    let bytes = include_bytes!("../../../../corpus/1WireRGBWController.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = decoded.ir();
    let validation = cadmpeg_ir::validate_neutral(ir, decoded.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    assert!(ir.model.features.len() > 1);
    let operations = ir
        .model
        .features
        .iter()
        .map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(op)
            | FeatureDefinition::PostProcess { operation: op, .. } => op,
        })
        .collect::<Vec<_>>();
    assert!(operations.iter().any(|op| matches!(
        op,
        FeatureOperation::Extrude {
            profile: ProfileRef::Planar(
                PlanarProfileRef::SketchProfiles { .. } | PlanarProfileRef::SketchRegions { .. }
            ),
            ..
        }
    )));
    assert!(operations
        .iter()
        .any(|op| matches!(op, FeatureOperation::Fillet { .. })));
    assert!(operations
        .iter()
        .any(|op| matches!(op, FeatureOperation::Chamfer { .. })));
    assert!(ir
        .model
        .sketch_entities
        .iter()
        .any(|entity| entity.construction));

    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Solid)
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    assert!(!bodies.is_empty());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
    let report = Measurer::new(
        MeasureInput {
            ir,
            fidelity: None,
            geometry_transferred: true,
        },
        MeasureOptions::default(),
    )
    .measure(
        &ctx,
        &Selection {
            bodies: bodies.clone(),
            ..Selection::default()
        },
    )
    .unwrap();
    assert_eq!(report.bodies().len(), bodies.len());
    for body in report.bodies() {
        assert!(!body.shells().is_empty());
        assert!(body.shells().iter().all(|shell| *shell.closed()));
    }
    ctx.finish_session().unwrap();
}
