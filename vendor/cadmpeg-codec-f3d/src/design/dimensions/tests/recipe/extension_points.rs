// SPDX-License-Identifier: Apache-2.0
use super::{
    Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

#[test]
fn recipe_linear_dimension_candidates_include_extension_points() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let parameter = cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter")
        .expect("identity grammar");
    let point = |name: &str, u, v| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#carrier").unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(2.0, 0.0),
                end: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        ),
        point("synthetic:test:id#carrier-start", 2.0, 0.0),
        point("synthetic:test:id#carrier-end", 0.0, 0.0),
        point("synthetic:test:id#extension", 4.0, 0.0),
        point("synthetic:test:id#off-carrier-horizontal", 2.0, 3.0),
        point("synthetic:test:id#off-carrier-vertical", 4.0, 2.0),
    ];
    let candidates = crate::design::dimensions::recipe_linear_dimension_candidates(
        &entities, &sketch, 2.0, &parameter, 0.0,
    );
    assert!(candidates.len() > 2);
}
