// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::{closed_spatial_sketch_profiles, SPATIAL_PROFILE_SEARCH_WORK};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};

const EPS_PROFILE_CLOSE: f64 = 1.0e-6;

fn fixture() -> (SpatialSketchId, Vec<SpatialSketchEntity>) {
    let sketch = SpatialSketchId::mint("synthetic:test:spatial-profile#sketch").unwrap();
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ];
    let mut entities = Vec::new();
    for index in 0..3 {
        entities.push(SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-profile#line-{index}"))
                .unwrap(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: points[index],
                end: points[(index + 1) % 3],
            })
            .unwrap(),
        ));
    }
    entities.push(SpatialSketchEntity::new(
        SpatialSketchEntityId::mint("synthetic:test:spatial-profile#circle").unwrap(),
        sketch.clone(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(4.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    ));
    (sketch, entities)
}

#[test]
fn spatial_profile_closed_loop_and_circle_keep_order() {
    let (_, entities) = fixture();
    let entities: Vec<_> = entities.iter().collect();
    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_spatial_sketch_profiles(
            decode_ctx,
            &entities,
            EPS_PROFILE_CLOSE,
            &WorkBudget::new(SPATIAL_PROFILE_SEARCH_WORK),
        )
    })
    .unwrap()
    .expect("the search stays within its cap");
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].boundary().len(), 1);
    assert_eq!(profiles[1].boundary().len(), 3);
}

#[test]
fn spatial_profile_search_skips_the_sketch_when_its_cap_runs_out() {
    let (_, entities) = fixture();
    let entities: Vec<_> = entities.iter().collect();
    let budget = WorkBudget::new(1);
    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_spatial_sketch_profiles(decode_ctx, &entities, EPS_PROFILE_CLOSE, &budget)
    })
    .unwrap();
    assert_eq!(profiles, None);
    assert!(budget.exhausted());
}
