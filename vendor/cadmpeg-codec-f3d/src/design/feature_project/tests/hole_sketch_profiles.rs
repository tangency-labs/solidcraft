// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::feature_project::{
    bind_hole_sketch_point_profiles, bind_sketch_feature_geometry,
};
use crate::layout::work_point_sketch_point_identity as layout;
use crate::records::feature::hole::DesignHoleLocation;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::work_geometry::{
    DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointSketchPointSelection,
    DesignWorkPointSketchPointSelectionDraft,
};
use cadmpeg_ir::features::holes::{HoleKind, HoleProfileFilter};
use cadmpeg_ir::features::{
    FaceSelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    PlanarProfileRef,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

fn point_native_id(record_index: u32) -> String {
    format!("f3d:Design/BulkStream.dat:sketch-point#{record_index}")
}

/// A location whose vertex input selects sketch point `point` through the
/// envelope at input `record_index`.
fn location(record_index: u32, point: Option<u32>) -> DesignHoleLocation {
    let identity_offset = 100 * u64::from(record_index);
    let carrier = point.map(|point| {
        Box::new(DesignWorkPointInputCarrier::SketchPoint {
            selection: DesignWorkPointSketchPointSelection::try_new(
                record_index,
                DesignWorkPointSketchPointSelectionDraft {
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "338".to_owned(),
                    )
                    .unwrap(),
                    asset_id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    asset_id_offset: identity_offset - 60,
                    context_id: "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    context_id_offset: identity_offset - 30,
                    identity_record_index: record_index + 3,
                    identity_record_offset: identity_offset,
                    sketch_record_index: 11,
                    sketch_record_index_offset: identity_offset
                        + u64_from_index(layout::SKETCH_RECORD_INDEX),
                    point_persistent_id: u64::from(point),
                    point_persistent_id_offset: identity_offset
                        + u64_from_index(layout::POINT_PERSISTENT_ID),
                    point_native_id: point_native_id(point),
                    next_record_index: record_index + 4,
                    next_byte_offset: identity_offset + u64_from_index(layout::LEN),
                },
            )
            .unwrap(),
        })
    });
    DesignHoleLocation {
        record_index: record_index + 4,
        position: crate::test_support::reals([1.0, 2.0, 0.0]),
        position_offset: 0,
        reference_type: 10,
        vertex_input: Some(DesignWorkPointInput::try_new(record_index, 0, carrier).unwrap()),
    }
}

fn hole_scope(reference_type: u32, locations: Vec<DesignHoleLocation>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#32",
        DesignFeatureKind::Hole,
        32,
    );
    let mut construction: crate::records::feature::hole::DesignHoleConstruction =
        serde_json::from_value(serde_json::json!({
            "point_record_index": 1, "point_record_byte_offset": 2,
            "position": [0.0, 0.0, 0.0], "position_offset": 3,
            "direction": [0.0, 0.0, 1.0], "direction_offset": 4,
            "point_parameters": [0.0, 0.0], "point_parameter_offsets": [5, 6],
            "reference_type": reference_type, "reference_type_offset": 7,
            "input_record_indices": [8], "input_record_offsets": [9]
        }))
        .unwrap();
    construction.locations = locations;
    if let DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        slot.point = Some(construction);
    }
    scope
}

fn feature(id: &str, native_ref: Option<&str>, definition: FeatureDefinition) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("synthetic:test:id#f3d:feature:{id}")).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: native_ref.map(str::to_owned),
    }
}

fn hole_feature(scope: &DesignParameterScope) -> Feature {
    feature(
        "hole",
        Some(&scope.id),
        FeatureDefinition::Operation(FeatureOperation::Hole {
            profile: None,
            profile_filter: None,
            face: Some(FaceSelection::Native(scope.id.clone())),
            direction: None,
            placements: None,
            shape: cadmpeg_ir::features::holes::HoleShape::new(
                cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
                None,
                Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
            )
            .unwrap(),
            extent: None,
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }),
    )
}

fn sketch_id(name: &str) -> SketchId {
    SketchId::mint(format!("synthetic:test:id#f3d:sketch:{name}")).unwrap()
}

fn sketch_point(sketch: &str, point: u32) -> SketchEntity {
    let mut entity = SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#f3d:point:{point}")).unwrap(),
        sketch_id(sketch),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(f64::from(point), 0.0),
        })
        .unwrap(),
    );
    entity.native_ref = Some(point_native_id(point));
    entity
}

fn bound_profile(
    scope: &DesignParameterScope,
    entities: &[SketchEntity],
) -> (Option<PlanarProfileRef>, Option<HoleProfileFilter>) {
    let mut features = [hole_feature(scope)];
    bind_hole_sketch_point_profiles(&mut features, std::slice::from_ref(scope), entities);
    let FeatureDefinition::Operation(FeatureOperation::Hole {
        profile,
        profile_filter,
        ..
    }) = features[0].evaluation.definition()
    else {
        panic!("expected a Hole");
    };
    (profile.clone(), *profile_filter)
}

#[test]
fn sketch_point_hole_locations_select_their_points_in_location_order() {
    let scope = hole_scope(13, vec![location(40, Some(8)), location(50, Some(7))]);
    let entities = [sketch_point("a", 7), sketch_point("a", 8)];
    assert_eq!(
        bound_profile(&scope, &entities),
        (
            Some(
                PlanarProfileRef::sketch_entities(
                    sketch_id("a"),
                    vec![entities[1].id().clone(), entities[0].id().clone()],
                )
                .unwrap()
            ),
            Some(HoleProfileFilter::Points)
        )
    );
}

#[test]
fn a_hole_without_one_planar_sketch_point_per_location_keeps_no_profile() {
    let entities = [sketch_point("a", 7), sketch_point("a", 8)];
    let unbound = hole_scope(13, vec![location(40, Some(8)), location(50, None)]);
    assert_eq!(bound_profile(&unbound, &entities), (None, None));
    let unprojected = hole_scope(13, vec![location(40, Some(8)), location(50, Some(9))]);
    assert_eq!(bound_profile(&unprojected, &entities), (None, None));
    let ambiguous = hole_scope(13, vec![location(40, Some(7))]);
    assert_eq!(
        bound_profile(&ambiguous, &[sketch_point("a", 7), sketch_point("b", 7)]),
        (None, None)
    );
    let split = hole_scope(13, vec![location(40, Some(7)), location(50, Some(8))]);
    assert_eq!(
        bound_profile(&split, &[sketch_point("a", 7), sketch_point("b", 8)]),
        (None, None)
    );
    let repeated = hole_scope(13, vec![location(40, Some(7)), location(50, Some(7))]);
    assert_eq!(bound_profile(&repeated, &entities), (None, None));
}

#[test]
fn only_a_sketch_point_carrier_takes_its_locations_as_a_profile() {
    let entities = [sketch_point("a", 7)];
    let mut at_carrier = location(40, Some(7));
    at_carrier.position = crate::test_support::reals([0.0, 0.0, 0.0]);
    let face_point = hole_scope(19, vec![at_carrier]);
    assert!(face_point
        .hole_construction()
        .is_some_and(|construction| construction.entry_points().is_some()));
    assert_eq!(bound_profile(&face_point, &entities), (None, None));
    let mut other_rule = location(40, Some(7));
    other_rule.reference_type = 17;
    assert_eq!(
        bound_profile(&hole_scope(13, vec![other_rule]), &entities),
        (None, None)
    );
}

#[test]
fn a_hole_with_a_profile_keeps_it() {
    let scope = hole_scope(13, vec![location(40, Some(7))]);
    let mut features = [hole_feature(&scope)];
    features[0].evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Hole { profile, .. }) = definition {
            *profile = Some(PlanarProfileRef::Sketch(sketch_id("b")));
        }
    });
    let before = features[0].evaluation.definition().clone();
    bind_hole_sketch_point_profiles(
        &mut features,
        std::slice::from_ref(&scope),
        &[sketch_point("a", 7)],
    );
    assert_eq!(features[0].evaluation.definition(), &before);
}

#[test]
fn a_hole_sketch_point_profile_depends_on_its_sketch_feature() {
    let scope = hole_scope(13, vec![location(40, Some(7))]);
    let sketch = feature(
        "sketch",
        None,
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch_id("a"))),
        }),
    );
    let mut features = [sketch, hole_feature(&scope)];
    bind_hole_sketch_point_profiles(
        &mut features,
        std::slice::from_ref(&scope),
        &[sketch_point("a", 7)],
    );
    bind_sketch_feature_geometry(&mut features, &[], &[], &[], &[]).unwrap();
    assert_eq!(
        features[1].dependencies.as_slice(),
        [features[0].id.clone()]
    );
}
