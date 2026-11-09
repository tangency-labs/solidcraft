// SPDX-License-Identifier: Apache-2.0

use super::{curve, group, operand, placement};
use crate::ids::{neutral_sketch_curve_id, neutral_sketch_id};
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use cadmpeg_ir::features::PathRef;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchPlacement,
};

#[test]
fn sweep_path_binds_a_complete_ordered_multi_member_curve_selection() {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
        PlanarProfileRef, SolidSweepOperation, SweepSection, SweepShape,
    };
    let placement = placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let line = |id, start, end| {
        SketchEntity::new(
            id,
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let sketch_entities = [
        line(
            neutral_sketch_curve_id(&sketch, 100, 101),
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
        ),
        line(
            neutral_sketch_curve_id(&sketch, 200, 201),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
        ),
    ];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let groups = [group()];
    let scopes = [crate::records::feature::scope::DesignParameterScope::empty(
        "stream:scope",
        crate::records::feature::scope::DesignFeatureKind::Sweep,
        7,
    )];
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let placements = [placement];
    let bound_path = |operands: &[DesignEntitySelectionOperand]| {
        let mut features = [Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:model:feature#sweep").expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sweep {
                    shape: SweepShape::Solid {
                        op: SolidSweepOperation::NewBody,
                        section: SweepSection::Profile(PlanarProfileRef::Native(
                            "stream:profile".into(),
                        )),
                        sections: Vec::new(),
                    },
                    path: Some(PathRef::Native(groups[0].id.clone())),
                    orientation: None,
                    transition: None,
                    transformation: None,
                    path_tangent: false,
                    linearize: false,
                    twist: None,
                    path_extent: None,
                    guide_rail: None,
                    taper: None,
                    scale: None,
                    allow_multi_profile_faces: None,
                },
            )),
            native_ref: Some(scopes[0].id.clone()),
        }];
        super::super::bind_sweep_sketch_selections(
            &mut features,
            &super::super::SketchCurveSelectionResolution {
                scopes: &scopes,
                groups: &groups,
                operands,
                profile_curves: &[],
                placements: &placements,
                curve_identities: &curves,
                sketches: &sketches,
                sketch_entities: &sketch_entities,
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
        )
        .unwrap();
        match features[0].evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Sweep { path, .. }) => path.clone(),
            _ => None,
        }
    };
    assert_eq!(
        bound_path(&operands),
        Some(
            PathRef::sketch_curves(
                sketch.clone(),
                vec![
                    neutral_sketch_curve_id(&sketch, 100, 101),
                    neutral_sketch_curve_id(&sketch, 200, 201),
                ],
            )
            .unwrap()
        )
    );
    assert_eq!(
        bound_path(&operands[..1]),
        Some(PathRef::Native(groups[0].id.clone()))
    );
}
