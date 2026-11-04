// SPDX-License-Identifier: Apache-2.0
use super::{
    Point2, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};

#[test]
fn recipe_linear_dimension_candidates_resolve_unique_vertical_distance() {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
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
    let parameter = cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter")
        .expect("identity grammar");
    let mut entities = vec![
        point("synthetic:test:id#first", -30.0, 2.0),
        point("synthetic:test:id#second", -30.0, 0.0),
        point("synthetic:test:id#unrelated", 10.0, 10.0),
    ];
    assert!(matches!(
        crate::design::dimensions::recipe_linear_dimension_candidates(&entities, &sketch, 2.0, &parameter, 0.0).as_slice(),
        [SketchConstraintDefinitionInput::VerticalDistance { first, second, parameter: actual }]
            if *first == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId::mint("synthetic:test:id#first").unwrap())
                && *second == cadmpeg_ir::sketches::SketchLocus::Entity(SketchEntityId::mint("synthetic:test:id#second").unwrap())
                && *actual == parameter
    ));
    entities.push(point("synthetic:test:id#ambiguous", 10.0, 8.0));
    let candidates = crate::design::dimensions::recipe_linear_dimension_candidates(
        &entities, &sketch, 2.0, &parameter, 0.0,
    );
    assert_eq!(candidates.len(), 2);
}
