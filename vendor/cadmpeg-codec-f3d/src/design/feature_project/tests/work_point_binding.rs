// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::bind_work_point_sketch_point_constructions;
use crate::layout::work_point_sketch_point_identity as layout;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::work_geometry::{
    DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
    DesignWorkPointRule, DesignWorkPointRuleForm, DesignWorkPointSketchPointSelection,
    DesignWorkPointSketchPointSelectionDraft,
};
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};
use std::collections::BTreeMap;

fn fixture() -> (
    DesignParameterScope,
    Feature,
    SketchEntity,
    SpatialSketchEntity,
) {
    let point_native_id = "f3d:Design/BulkStream.dat:sketch-point#7";
    let record_index = 42;
    let identity_offset = 100;
    let selection = DesignWorkPointSketchPointSelection::try_new(
        record_index,
        DesignWorkPointSketchPointSelectionDraft {
            class_tag: crate::records::references::DesignClassTag::try_from("264".to_owned())
                .unwrap(),
            asset_id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
                .to_owned()
                .try_into()
                .unwrap(),
            asset_id_offset: 10,
            context_id: "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb"
                .to_owned()
                .try_into()
                .unwrap(),
            context_id_offset: 50,
            identity_record_index: record_index + 3,
            identity_record_offset: identity_offset,
            sketch_record_index: 11,
            sketch_record_index_offset: identity_offset
                + u64_from_index(layout::SKETCH_RECORD_INDEX),
            point_persistent_id: 7,
            point_persistent_id_offset: identity_offset
                + u64_from_index(layout::POINT_PERSISTENT_ID),
            point_native_id: point_native_id.to_owned(),
            next_record_index: record_index + 4,
            next_byte_offset: identity_offset + u64_from_index(layout::LEN),
        },
    )
    .unwrap();
    let input = DesignWorkPointInput::try_new(
        record_index,
        0,
        Some(Box::new(DesignWorkPointInputCarrier::SketchPoint {
            selection,
        })),
    )
    .unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::WorkPoint,
        10,
    );
    if let DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() {
        *slot = Some(DesignWorkPointConstruction {
            point_record_index: 41,
            point_record_byte_offset: 0,
            position: crate::test_support::reals([1.0, 2.0, 3.0]),
            position_offset: 0,
            rule: DesignWorkPointRule::try_from(DesignWorkPointRuleForm::Vertex { input }).unwrap(),
            reference_type_offset: 0,
        });
    }
    let feature = Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#f3d:feature:work-point").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::DatumPoint {
                position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
                    .unwrap(),
                construction: None,
            },
        )),
        native_ref: Some(scope.id.clone()),
    };
    let mut planar = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#f3d:point:planar").unwrap(),
        SketchId::mint("synthetic:test:id#f3d:sketch:planar").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    );
    planar.native_ref = Some(point_native_id.to_owned());
    let mut spatial = SpatialSketchEntity::new(
        SpatialSketchEntityId::mint("synthetic:test:id#f3d:point:spatial").unwrap(),
        SpatialSketchId::mint("synthetic:test:id#f3d:sketch:spatial").unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Point {
            position: Point3::new(1.0, 2.0, 3.0),
        })
        .unwrap(),
    );
    spatial.native_ref = Some(point_native_id.to_owned());
    (scope, feature, planar, spatial)
}

#[test]
fn work_point_sketch_binding_preserves_planar_and_spatial_selections() {
    use cadmpeg_ir::features::{DatumPointConstruction, SketchPointSelection};
    let (scope, feature, planar, spatial) = fixture();
    let mut planar_features = [feature.clone()];
    bind_work_point_sketch_point_constructions(
        &mut planar_features,
        std::slice::from_ref(&scope),
        std::slice::from_ref(&planar),
        &[],
    );
    assert!(matches!(planar_features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            construction: Some(construction), ..
        }) if matches!(construction.as_ref(), DatumPointConstruction::SketchPoint {
            point: SketchPointSelection::Planar { sketch, point, native }
        } if sketch == &planar.sketch && point == planar.id()
            && native == "f3d:Design/BulkStream.dat:design-record#42")));
    let mut spatial_features = [feature];
    bind_work_point_sketch_point_constructions(
        &mut spatial_features,
        std::slice::from_ref(&scope),
        &[],
        std::slice::from_ref(&spatial),
    );
    assert!(matches!(spatial_features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumPoint {
            construction: Some(construction), ..
        }) if matches!(construction.as_ref(), DatumPointConstruction::SketchPoint {
            point: SketchPointSelection::Spatial { sketch, point, native }
        } if sketch == &spatial.sketch && point == spatial.id()
            && native == "f3d:Design/BulkStream.dat:design-record#42")));
}
