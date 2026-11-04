// SPDX-License-Identifier: Apache-2.0

use super::*;

const REGION_LIMIT_TEST_TOLERANCE: f64 = 1.0e-7;

#[test]
fn circular_region_keeps_admitted_selection() {
    let sketch_id = SketchId::mint("synthetic:test:id#region").unwrap();
    let entity_id = SketchEntityId::mint("synthetic:test:id#region-circle").unwrap();
    let entity = SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        })
        .unwrap(),
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    let entities = [entity];
    let points = [Point3::new(0.5, 0.0, 0.0)];
    let expected = crate::test_support::with_decode_context(|decode_ctx| {
        super::region_containing_points(
            &sketch,
            &entities,
            &points,
            REGION_LIMIT_TEST_TOLERANCE,
            &super::local_profile_work(),
            decode_ctx,
        )
    })
    .unwrap();
    assert!(expected.is_some());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let actual = super::region_containing_points(
        &sketch,
        &entities,
        &points,
        REGION_LIMIT_TEST_TOLERANCE,
        &super::local_profile_work(),
        &ctx,
    )
    .unwrap();
    assert_eq!(actual, expected);
}
