// SPDX-License-Identifier: Apache-2.0

use super::{
    exact_circular_pattern, exact_rectangular_pattern, exact_text_relation, scalar_close,
    translated_sketch_geometry_matches, RectangularPatternDistanceForm,
};
use crate::records::{
    parameters::DesignParameter,
    sketch_relations::{
        SketchPatternDefinition, SketchRelation, SketchRelationMember, SketchRelationReturnMember,
    },
};
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_test_support::wire;

#[test]
fn translated_nurbs_match_borrowed_rational_poles() {
    let source = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 0.0), Point2::new(2.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap(),
    })
    .unwrap();
    let shifted = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(4.0, 5.0), Point2::new(5.0, 5.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap(),
    })
    .unwrap();
    assert!(translated_sketch_geometry_matches(
        &source,
        &shifted,
        Point2::new(3.0, 5.0)
    ));
}

#[test]
fn rotated_nurbs_match_borrowed_rational_poles() {
    let source = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 0.0), Point2::new(2.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap(),
    })
    .unwrap();
    let rotated = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 1.0), Point2::new(0.0, 2.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap(),
    })
    .unwrap();
    assert!(super::rotated_sketch_geometry_matches(
        &source,
        &rotated,
        Point2::new(0.0, 0.0),
        std::f64::consts::FRAC_PI_2
    ));
}

#[test]
fn rectangular_pattern_instances_require_exact_translated_geometry() {
    let source = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(1.0, 2.0),
        end: Point2::new(4.0, 6.0),
    })
    .unwrap();
    let translated = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(11.0, -1.0),
        end: Point2::new(14.0, 3.0),
    })
    .unwrap();
    assert!(translated_sketch_geometry_matches(
        &source,
        &translated,
        Point2::new(10.0, -3.0),
    ));
    let reversed = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(14.0, 3.0),
        end: Point2::new(11.0, -1.0),
    })
    .unwrap();
    assert!(!translated_sketch_geometry_matches(
        &source,
        &reversed,
        Point2::new(10.0, -3.0),
    ));
    let resized = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(12.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(3.1).unwrap(),
    })
    .unwrap();
    assert!(!translated_sketch_geometry_matches(
        &SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(2.0, 3.0),
            radius: cadmpeg_ir::scalar::Length::new(3.0).unwrap(),
        })
        .unwrap(),
        &resized,
        Point2::new(10.0, -3.0),
    ));
}

fn point_entity(id: &str, u: f64) -> cadmpeg_ir::sketches::SketchEntity {
    cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint(id).unwrap(),
        SketchId::mint("generated:test:sketch#0").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(u, 4.0),
        })
        .unwrap(),
    )
}

fn rectangular_point_relation(
    evaluated_count: u32,
    evaluated_distance: f64,
    distance_form: RectangularPatternDistanceForm,
) -> SketchRelation {
    rectangular_point_relation_with_direction_slots(
        evaluated_count,
        evaluated_distance,
        distance_form,
        0,
    )
}

fn rectangular_point_relation_with_direction_slots(
    evaluated_count: u32,
    evaluated_distance: f64,
    distance_form: RectangularPatternDistanceForm,
    rectangular_counted_reference_count: u32,
) -> SketchRelation {
    let mut auxiliary_references = (0..rectangular_counted_reference_count)
        .map(|index| 100 + index)
        .collect::<Vec<_>>();
    let uses_extent = u8::from(matches!(
        distance_form,
        RectangularPatternDistanceForm::SeedToFinalSpan
    ));
    auxiliary_references.extend([20, 21, 22, 23]);
    let members = (1..=evaluated_count).collect::<Vec<_>>();
    SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#rectangular".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            auxiliary_references
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: Some(rectangular_counted_reference_count),
        members: (members
            .clone()
            .into_iter()
            .map(SketchRelationMember::from_index)
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x2000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::Rectangular {
                    directions: [
                        crate::records::sketch_relations::SketchPatternDirection {
                            count_parameter: Some(20),
                            distance_parameter: Some(21),
                            evaluated_count:
                                crate::records::sketch_relations::SketchPatternCount::try_from(
                                    evaluated_count,
                                )
                                .unwrap(),
                            direction: [1.0, 0.0, 0.0].try_into().unwrap(),
                            evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(
                                evaluated_distance,
                            )
                            .unwrap(),
                        },
                        crate::records::sketch_relations::SketchPatternDirection {
                            count_parameter: Some(22),
                            distance_parameter: Some(23),
                            evaluated_count:
                                crate::records::sketch_relations::SketchPatternCount::try_from(1)
                                    .unwrap(),
                            direction: [0.0, 1.0, 0.0].try_into().unwrap(),
                            evaluated_distance: cadmpeg_ir::scalar::FiniteReal::ZERO,
                        },
                    ],
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (members
            .iter()
            .copied()
            .map(SketchRelationReturnMember::from_index)
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: crate::test_support::sketch_relations::rectangular_pattern_record(
            evaluated_count,
            evaluated_distance,
            [uses_extent, 0, 0],
            rectangular_counted_reference_count,
        ),
    })
    .unwrap()
}

fn rectangular_parameter(record_index: u32, value: f64) -> DesignParameter {
    crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("native:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("373".to_owned())
                .unwrap(),
            record_index,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "R-Pattern1-distance".into(),
                Some(record_index),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code6,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: value.to_string(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: format!("d{record_index}"),
            name_offset: 80,
            evaluated_value: value,
            evaluated_value_offset: 90,
        },
    )
    .unwrap()
}

fn rectangular_parameters(count: u32, distance: f64) -> [DesignParameter; 4] {
    [
        rectangular_parameter(20, f64::from(count)),
        rectangular_parameter(21, distance),
        rectangular_parameter(22, 1.0),
        rectangular_parameter(23, 0.0),
    ]
}

fn grid_point_entity(id: &str, u: f64, v: f64) -> cadmpeg_ir::sketches::SketchEntity {
    cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint(id).unwrap(),
        SketchId::mint("generated:test:sketch#0").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(u, v),
        })
        .unwrap(),
    )
}

/// An adjacent-spacing rectangular point pattern with both direction clauses.
fn rectangular_grid_relation(counts: [u32; 2], distances: [f64; 2]) -> SketchRelation {
    let direction = |count: u32, count_parameter, distance: f64, distance_parameter, axis| {
        crate::records::sketch_relations::SketchPatternDirection {
            count_parameter: Some(count_parameter),
            distance_parameter: Some(distance_parameter),
            evaluated_count: crate::records::sketch_relations::SketchPatternCount::try_from(count)
                .unwrap(),
            direction: axis,
            evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(distance).unwrap(),
        }
    };
    let members = (1..=counts[0] * counts[1]).collect::<Vec<_>>();
    SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#rectangular".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            [20, 21, 22, 23]
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: Some(0),
        members: (members
            .iter()
            .copied()
            .map(SketchRelationMember::from_index)
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x2000_0000,
            Some(SketchPatternDefinition::Rectangular {
                directions: [
                    direction(
                        counts[0],
                        20,
                        distances[0],
                        21,
                        [1.0, 0.0, 0.0].try_into().unwrap(),
                    ),
                    direction(
                        counts[1],
                        22,
                        distances[1],
                        23,
                        [0.0, 1.0, 0.0].try_into().unwrap(),
                    ),
                ],
            }),
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (members
            .iter()
            .copied()
            .map(SketchRelationReturnMember::from_index)
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: crate::test_support::sketch_relations::rectangular_grid_pattern_record(
            counts,
            distances,
            [0, 0, 0],
            0,
        ),
    })
    .unwrap()
}

fn rectangular_grid_parameters(counts: [u32; 2], distances: [f64; 2]) -> [DesignParameter; 4] {
    [
        rectangular_parameter(20, f64::from(counts[0])),
        rectangular_parameter(21, distances[0]),
        rectangular_parameter(22, f64::from(counts[1])),
        rectangular_parameter(23, distances[1]),
    ]
}

#[test]
fn rectangular_pattern_instances_advance_direction_one_first() {
    let seed = grid_point_entity("generated:test:point#seed", 2.0, 4.0);
    let first = grid_point_entity("generated:test:point#first", 17.0, 4.0);
    let second = grid_point_entity("generated:test:point#second", 2.0, 14.0);
    let diagonal = grid_point_entity("generated:test:point#diagonal", 17.0, 14.0);
    let relation = rectangular_grid_relation([2, 2], [1.5, 1.0]);
    let parameters = rectangular_grid_parameters([2, 2], [1.5, 1.0]);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        exact_rectangular_pattern(
            &relation,
            "native",
            &parameters,
            &[&seed, &first, &second, &diagonal],
        )
        .unwrap()
    else {
        panic!("rectangular grid did not resolve");
    };
    let rows = pattern
        .rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(|instance| instance.entities.clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            [vec![seed.id().clone()], vec![second.id().clone()]],
            [vec![first.id().clone()], vec![diagonal.id().clone()]],
        ]
    );

    assert!(exact_rectangular_pattern(
        &relation,
        "native",
        &parameters,
        &[&seed, &second, &first, &diagonal],
    )
    .unwrap()
    .is_none());
}

#[test]
fn rectangular_pattern_rows_group_a_non_square_grid_by_direction_one_index() {
    // Grid position (i, j) sits 15 mm along direction one per step of i and
    // 10 mm along direction two per step of j.
    let entity = |i: u32, j: u32| {
        grid_point_entity(
            &format!("generated:test:point#{i}-{j}"),
            2.0 + 15.0 * f64::from(i),
            4.0 + 10.0 * f64::from(j),
        )
    };
    let grid: Vec<Vec<_>> = (0..2)
        .map(|i| (0..3).map(|j| entity(i, j)).collect())
        .collect();
    let stored: Vec<_> = (0..6usize).map(|p| &grid[p % 2][p / 2]).collect();
    let relation = rectangular_grid_relation([2, 3], [1.5, 1.0]);
    let parameters = rectangular_grid_parameters([2, 3], [1.5, 1.0]);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        exact_rectangular_pattern(&relation, "native", &parameters, &stored).unwrap()
    else {
        panic!("non-square rectangular grid did not resolve");
    };
    let rows = pattern
        .rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(|instance| instance.entities.clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let expected = grid
        .iter()
        .map(|row| {
            row.iter()
                .map(|entity| vec![entity.id().clone()])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(rows, expected);

    let direction_two_first: Vec<_> = (0..6usize).map(|p| &grid[p / 3][p % 3]).collect();
    assert!(
        exact_rectangular_pattern(&relation, "native", &parameters, &direction_two_first)
            .unwrap()
            .is_none()
    );
}

#[test]
fn rectangular_pattern_single_instance_direction_keeps_its_stored_distance() {
    let seed = grid_point_entity("generated:test:point#seed", 2.0, 4.0);
    let second = grid_point_entity("generated:test:point#second", 2.0, 14.0);
    let relation = rectangular_grid_relation([1, 2], [0.5, 1.0]);
    let parameters = rectangular_grid_parameters([1, 2], [0.5, 1.0]);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second]).unwrap()
    else {
        panic!("single-instance direction did not resolve");
    };
    assert_eq!(pattern.directions()[0].spacing().get(), 5.0);
    assert_eq!(pattern.directions()[1].spacing().get(), 10.0);
    assert_eq!(pattern.rows().len(), 1);
    assert_eq!(pattern.rows()[0].len(), 2);
}

#[test]
fn rectangular_pattern_projects_adjacent_spacing_and_parameter() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    let relation =
        rectangular_point_relation(3, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    let parameters = rectangular_parameters(3, 1.5);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second, &third])
            .unwrap()
    else {
        panic!("rectangular pattern did not resolve");
    };
    let directions = pattern.directions();
    assert_eq!(directions[0].spacing().get(), 15.0);
    assert_eq!(directions[1].spacing().get(), 0.0);
    assert!(matches!(
        directions[0].distance,
        Some(cadmpeg_ir::sketches::SketchPatternDistance::Spacing { .. })
    ));
    assert!(directions[0].count_parameter.is_some());
    assert_eq!([pattern.rows().len(), pattern.rows()[0].len()], [3, 1]);
}

#[test]
fn rectangular_pattern_projects_total_span_and_keeps_span_parameter() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    let relation =
        rectangular_point_relation(3, 3.0, RectangularPatternDistanceForm::SeedToFinalSpan);
    let parameters = rectangular_parameters(3, 3.0);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second, &third])
            .unwrap()
    else {
        panic!("total-span rectangular pattern did not resolve");
    };
    let directions = pattern.directions();
    assert_eq!(directions[0].spacing().get(), 15.0);
    assert!(matches!(
        directions[0].distance,
        Some(cadmpeg_ir::sketches::SketchPatternDistance::Span { .. })
    ));
    assert!(directions[0].count_parameter.is_some());
}

#[test]
fn rectangular_pattern_does_not_change_distance_form_to_match_geometry() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    for (distance_form, distance) in [
        (RectangularPatternDistanceForm::AdjacentSpacing, 3.0),
        (RectangularPatternDistanceForm::SeedToFinalSpan, 1.5),
    ] {
        let relation = rectangular_point_relation(3, distance, distance_form);
        let parameters = rectangular_parameters(3, distance);
        assert_eq!(
            exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second, &third])
                .unwrap(),
            None
        );
    }
}

#[test]
fn rectangular_pattern_reads_source_mode_without_the_cached_reference_count() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let mut relation =
        rectangular_point_relation(2, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    relation.rectangular_counted_reference_count = None;
    let parameters = rectangular_parameters(2, 1.5);

    assert!(
        exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second])
            .unwrap()
            .is_some()
    );
}

#[test]
fn rectangular_pattern_transfers_two_instances_in_both_distance_forms() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    for distance_form in [
        RectangularPatternDistanceForm::AdjacentSpacing,
        RectangularPatternDistanceForm::SeedToFinalSpan,
    ] {
        let relation = rectangular_point_relation(2, 1.5, distance_form);
        let parameters = rectangular_parameters(2, 1.5);
        let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
            exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second]).unwrap()
        else {
            panic!("two-instance rectangular pattern did not resolve");
        };
        let directions = pattern.directions();
        assert_eq!(directions[0].spacing().get(), 15.0);
        match distance_form {
            RectangularPatternDistanceForm::AdjacentSpacing => {
                assert!(matches!(
                    directions[0].distance,
                    Some(cadmpeg_ir::sketches::SketchPatternDistance::Spacing { .. })
                ));
            }
            RectangularPatternDistanceForm::SeedToFinalSpan => {
                assert!(matches!(
                    directions[0].distance,
                    Some(cadmpeg_ir::sketches::SketchPatternDistance::Span { .. })
                ));
            }
        }
    }
}

#[test]
fn circular_pattern_resolves_full_and_partial_instance_distributions() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let center = entity(
        "generated:test:point#center",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, -3.0),
        })
        .unwrap(),
    );
    let circle = |id: &str, angle: f64| {
        entity(
            id,
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
            })
            .unwrap(),
        )
    };
    let seed = circle("generated:test:circle#seed", 0.0);
    let middle = circle("generated:test:circle#middle", std::f64::consts::FRAC_PI_2);
    let last = circle("generated:test:circle#last", std::f64::consts::PI);
    let relation = |angle| {
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: "f3d:native:sketch-relation#circular".into(),
            record_index: 10,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
            auxiliary_references: crate::records::identity::ReferenceRun::located(
                vec![20, 21]
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
            rectangular_counted_reference_count: None,
            members: (vec![
                SketchRelationMember::from_index(1),
                SketchRelationMember::from_index(2),
                SketchRelationMember::from_index(3),
                SketchRelationMember::from_index(4),
            ])
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(
                0x1000_0000,
                Some(
                    crate::records::sketch_relations::SketchPatternDefinition::Circular {
                        angle_parameter: Some(20),
                        count_parameter: Some(21),
                        evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(angle).unwrap(),
                        evaluated_count:
                            crate::records::sketch_relations::SketchPatternCount::try_from(3)
                                .unwrap(),
                    },
                ),
            )
            .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (vec![
                SketchRelationReturnMember::from_index(2),
                SketchRelationReturnMember::from_index(3),
                SketchRelationReturnMember::from_index(4),
                SketchRelationReturnMember::from_index(1),
            ])
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: crate::test_support::sketch_relations::circular_pattern_record(
                angle,
                3,
                &[],
            ),
        })
        .unwrap()
    };
    let members = [&center, &seed, &middle, &last];
    let returned = [&seed, &middle, &last, &center];
    let Some(SketchConstraintDefinitionInput::CircularPattern { pattern }) = exact_circular_pattern(
        &relation(std::f64::consts::PI),
        "native",
        &[],
        &members,
        &returned,
    ) else {
        panic!("partial circular pattern did not resolve");
    };
    assert_eq!(pattern.center(), center.id());
    assert_eq!(
        wire::field::<cadmpeg_ir::scalar::Angle>(&pattern, "angle").get(),
        std::f64::consts::PI
    );
    assert_eq!((pattern.instances().len() + 1), 3);
    // The seed is not an instance, so the instance list carries only the
    // rotations after it.
    assert_eq!(
        wire::field::<Vec<cadmpeg_ir::sketches::SketchEntityId>>(&pattern, "seed"),
        std::slice::from_ref(seed.id())
    );
    assert_eq!(
        pattern
            .instances()
            .iter()
            .map(|instance| instance.angle.get())
            .collect::<Vec<_>>(),
        [std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
    );

    let full_middle = circle(
        "generated:test:circle#full-middle",
        std::f64::consts::TAU / 3.0,
    );
    let full_last = circle(
        "generated:test:circle#full-last",
        2.0 * std::f64::consts::TAU / 3.0,
    );
    let full_members = [&center, &seed, &full_middle, &full_last];
    let full_returned = [&seed, &full_middle, &full_last, &center];
    assert!(matches!(
        exact_circular_pattern(&relation(std::f64::consts::TAU), "native", &[], &full_members, &full_returned),
        Some(SketchConstraintDefinitionInput::CircularPattern { ref pattern })
            if scalar_close(pattern.instances()[0].angle.get(), std::f64::consts::TAU / 3.0)
    ));
}

#[test]
fn circular_pattern_resolves_independently_of_relation_ordinals() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let center = entity(
        "generated:test:point#center",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, -3.0),
        })
        .unwrap(),
    );
    let circle = |id: &str, angle: f64| {
        entity(
            id,
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
            })
            .unwrap(),
        )
    };
    let seed = circle("generated:test:circle#seed", 0.0);
    let middle = circle("generated:test:circle#middle", std::f64::consts::TAU / 3.0);
    let last = circle(
        "generated:test:circle#last",
        2.0 * std::f64::consts::TAU / 3.0,
    );
    // Ordinals are all zero; geometry must still partition the members.
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#circular".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            vec![20, 21]
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: None,
        members: (vec![
            SketchRelationMember::from_index(1),
            SketchRelationMember::from_index(2),
            SketchRelationMember::from_index(3),
            SketchRelationMember::from_index(4),
        ])
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x1000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::Circular {
                    angle_parameter: Some(20),
                    count_parameter: Some(21),
                    evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(std::f64::consts::TAU)
                        .unwrap(),
                    evaluated_count:
                        crate::records::sketch_relations::SketchPatternCount::try_from(3).unwrap(),
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (vec![
            SketchRelationReturnMember::from_index(2),
            SketchRelationReturnMember::from_index(3),
            SketchRelationReturnMember::from_index(4),
            SketchRelationReturnMember::from_index(1),
        ])
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: crate::test_support::sketch_relations::circular_pattern_record(
            std::f64::consts::TAU,
            3,
            &[],
        ),
    })
    .unwrap();
    let members = [&center, &seed, &middle, &last];
    let returned = [&seed, &middle, &last, &center];
    let Some(SketchConstraintDefinitionInput::CircularPattern { pattern }) =
        exact_circular_pattern(&relation, "native", &[], &members, &returned)
    else {
        panic!("role-agnostic circular pattern did not resolve");
    };
    assert_eq!(pattern.center(), center.id());
    assert_eq!((pattern.instances().len() + 1), 3);
}

#[test]
fn text_path_relation_projects_typed_entities_and_scaled_glyph_placements() {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
        SketchGeometryDefinition, SketchId,
    };

    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let path = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#path").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let text = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#text").unwrap(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::new("A").unwrap(),
            font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
            font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
            height: Length::new(10.0).unwrap(),
            width_factor: Some(0.8),
            placement: None,
            horizontal_alignment: None,
            vertical_alignment: None,
        })
        .unwrap(),
    );
    let mut glyph = [[0.0; 4]; 4];
    for ordinal in 0..4 {
        glyph[ordinal][ordinal] = 1.0;
    }
    glyph[0][3] = 0.5;
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:Design/BulkStream.dat:sketch-relation#3".into(),
        record_index: 3,
        class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: None,
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            vec![2]
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: None,
        members: (vec![
            SketchRelationMember::from_index(1),
            SketchRelationMember::from_index(2),
        ])
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x200_0000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::TextPath {
                    text_reference: 2,
                    glyph_transforms: vec![
                        crate::records::sketch_relations::SketchGlyphTransform::try_from(glyph)
                            .expect("finite native glyph"),
                    ],
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: Some(2),
        return_members: (vec![SketchRelationReturnMember::from_index(1)])
            .try_into()
            .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let projected = std::collections::HashMap::from([(("scope", 1), &path), (("scope", 2), &text)]);
    let definition = exact_text_relation(&relation, "scope", &projected)
        .unwrap()
        .expect("typed text path");
    assert!(matches!(
        definition,
        SketchConstraintDefinitionInput::TextPath {
            text: ref text_id,
            path: ref path_id,
            ref glyph_transforms,
        } if text_id == text.id()
            && path_id == path.id()
            && glyph_transforms[0].rows()[0][3] == 5.0
    ));
    let mut relation = relation;
    let mut overflow = glyph;
    overflow[0][3] = f64::MAX;
    let mut non_affine = glyph;
    non_affine[3][3] = 2.0;
    for rows in [overflow, non_affine] {
        relation.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
            0x200_0000_0000,
            Some(SketchPatternDefinition::TextPath {
                text_reference: 2,
                glyph_transforms: vec![
                    crate::records::sketch_relations::SketchGlyphTransform::try_from(glyph)
                        .unwrap(),
                    crate::records::sketch_relations::SketchGlyphTransform::try_from(rows).unwrap(),
                ],
            }),
        )
        .unwrap();
        assert!(exact_text_relation(&relation, "scope", &projected)
            .unwrap()
            .is_none());
    }
}

#[test]
fn rectangular_pattern_distance_mode_is_independent_of_direction_reference_slots() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    for slots in 0..=2 {
        for (mode, distance) in [
            (RectangularPatternDistanceForm::AdjacentSpacing, 1.5),
            (RectangularPatternDistanceForm::SeedToFinalSpan, 3.0),
        ] {
            let relation =
                rectangular_point_relation_with_direction_slots(3, distance, mode, slots);
            let parameters = rectangular_parameters(3, distance);
            let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
                exact_rectangular_pattern(
                    &relation,
                    "native",
                    &parameters,
                    &[&seed, &second, &third],
                )
                .unwrap()
            else {
                panic!("serialized mode did not resolve");
            };
            assert_eq!(pattern.directions()[0].spacing().get(), 15.0);
            assert_eq!(pattern.rows().len(), 3);
        }
    }
}

#[test]
fn rectangular_pattern_preserves_signed_spacing_and_extent() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", -13.0);
    let third = point_entity("generated:test:point#third", -28.0);
    for (mode, distance) in [
        (RectangularPatternDistanceForm::AdjacentSpacing, -1.5),
        (RectangularPatternDistanceForm::SeedToFinalSpan, -3.0),
    ] {
        let relation = rectangular_point_relation(3, distance, mode);
        let parameters = rectangular_parameters(3, distance);
        let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
            exact_rectangular_pattern(&relation, "native", &parameters, &[&seed, &second, &third])
                .unwrap()
        else {
            panic!("signed source distance did not resolve");
        };
        assert_eq!(pattern.directions()[0].spacing().get(), -15.0);
    }
}
