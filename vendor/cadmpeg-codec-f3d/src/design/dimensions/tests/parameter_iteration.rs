// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput as Definition, SketchEntityId, SketchPatternDirection,
    SketchPatternDistance, SketchPatternInstance, SketchRectangularPattern,
};

#[test]
fn constraint_parameter_iteration_preserves_empty_optional_and_direction_order() {
    let id = |key| ParameterId::mint(format!("test:model:parameter#{key}")).unwrap();
    assert_eq!(
        super::super::constraint_parameters(&Definition::Disabled {}).count(),
        0
    );
    let first_distance = id("first-distance");
    let first_count = id("first-count");
    let second_distance = id("second-distance");
    let second_count = id("second-count");
    let directions = [
        SketchPatternDirection::new(
            [1.0, 0.0],
            cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
            Some(SketchPatternDistance::Spacing {
                parameter: first_distance.clone(),
            }),
            Some(first_count.clone()),
        )
        .unwrap(),
        SketchPatternDirection::new(
            [0.0, 1.0],
            cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
            Some(SketchPatternDistance::Span {
                parameter: second_distance.clone(),
            }),
            Some(second_count.clone()),
        )
        .unwrap(),
    ];
    let pattern = SketchRectangularPattern::new(
        directions,
        vec![vec![SketchPatternInstance {
            entities: vec![SketchEntityId::mint("test:model:sketch-entity#seed").unwrap()],
        }]],
    )
    .unwrap();
    let definition = Definition::RectangularPattern { pattern };
    assert_eq!(
        super::super::constraint_parameters(&definition).collect::<Vec<_>>(),
        [
            &first_distance,
            &first_count,
            &second_distance,
            &second_count
        ]
    );
    let definition = Definition::DistanceLociValue {
        first: cadmpeg_ir::sketches::SketchLocus::Start(
            SketchEntityId::mint("test:model:sketch-entity#first").unwrap(),
        ),
        second: cadmpeg_ir::sketches::SketchLocus::End(
            SketchEntityId::mint("test:model:sketch-entity#second").unwrap(),
        ),
        distance: cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        parameter: None,
    };
    assert_eq!(super::super::constraint_parameters(&definition).count(), 0);
}
