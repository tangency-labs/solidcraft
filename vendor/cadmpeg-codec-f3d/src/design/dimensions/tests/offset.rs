// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;

use crate::design::dimensions::bind_dimension_loci;
use crate::design::dimensions::counted_role_relation;
use crate::design::dimensions::exact_counted_offset;
use crate::design::dimensions::offset_parameter_factor;
use crate::design::dimensions::spatial_counted_offset_dimension_definition;
use crate::records::dimensions::DesignDimensionLocusPair;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::sketch_placement::DesignSketchPlacement;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchId;
use cadmpeg_ir::sketches::SketchNativeOperand;
use cadmpeg_ir::sketches::SketchOffsetPair;
use cadmpeg_ir::sketches::SpatialSketch;
use cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput;
use cadmpeg_ir::sketches::SpatialSketchEntity;
use cadmpeg_ir::sketches::SpatialSketchEntityId;
use cadmpeg_ir::sketches::SpatialSketchEntityUse;
use cadmpeg_ir::sketches::SpatialSketchGeometry;
use cadmpeg_ir::sketches::SpatialSketchId;
use cadmpeg_ir::sketches::SpatialSketchProfile;
use cadmpeg_ir::sketches::{SketchGeometryDefinition, SpatialSketchGeometryDefinition};
use std::collections::HashMap;

const TEST_LINEAR_TOLERANCE: f64 = 1.0e-6;
const TEST_DISTANCE_EPSILON: f64 = 1.0e-9;
const TEST_ANGLE_ROUNDING: f64 = 5.0e-7;

fn offset_loci(rows: &[(u32, u32, u32)]) -> Vec<crate::records::dimensions::DesignDimensionLocus> {
    rows.iter()
        .map(|&(geometry_record_index, role, returned)| {
            crate::records::dimensions::DesignDimensionLocus {
                geometry_record_index,
                geometry_reference_offset: 0,
                role,
                role_offset: 0,
                returned: crate::records::identity::Located {
                    value: returned,
                    offset: 0,
                },
            }
        })
        .collect()
}

#[test]
fn counted_offset_return_run_pairs_sources_and_results() {
    let entity = |id: &str, start, end| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let bottom = entity(
        "generated:test:line#bottom",
        Point2::new(10.0, 0.0),
        Point2::new(0.0, 0.0),
    );
    let top = entity(
        "generated:test:line#top",
        Point2::new(0.0, 10.0),
        Point2::new(10.0, 10.0),
    );
    let inset_top = entity(
        "generated:test:line#inset-top",
        Point2::new(2.0, 8.0),
        Point2::new(8.0, 8.0),
    );
    let inset_bottom = entity(
        "generated:test:line#inset-bottom",
        Point2::new(8.0, 2.0),
        Point2::new(2.0, 2.0),
    );

    let entities = HashMap::from([(1, &bottom), (2, &top), (3, &inset_top), (4, &inset_bottom)]);
    let definition = exact_counted_offset(
        &offset_loci(&[(1, 3, 1), (2, 2, 4), (3, 0, 2), (4, 0, 3)]),
        &entities,
        &HashMap::new(),
        1.0e-6,
    )
    .expect("counted offset graph");
    let crate::design::dimensions::CountedOffset { pairs, distance } = definition;
    assert_eq!(&pairs[0].source, bottom.id());
    assert_eq!(&pairs[0].result, inset_bottom.id());
    assert_eq!(&pairs[1].source, top.id());
    assert_eq!(&pairs[1].result, inset_top.id());
    assert!((distance.get() - 2.0).abs() <= 1.0e-9);
    assert!(pairs.iter().all(|pair| pair.source_reversed));
}

#[test]
fn counted_offset_accepts_primary_to_generated_identity_partition() {
    let entity = |id: &str, y| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, y),
                end: Point2::new(8.0, y),
            })
            .unwrap(),
        )
    };
    let source = entity("generated:test:line#source", 0.0);
    let result = entity("generated:test:line#result", 2.75);
    let entities = HashMap::from([(1, &source), (2, &result)]);
    let secondary_ids = HashMap::from([(1, 0), (2, 42)]);

    assert!(matches!(
        exact_counted_offset(&offset_loci(&[(1, 4, 1), (2, 1, 2)]), &entities, &secondary_ids, 1.0e-6),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance,
        }) if pairs.len() == 1
            && &pairs[0].source == source.id()
            && &pairs[0].result == result.id()
            && (distance.get() - 2.75).abs() <= 1.0e-9
    ));

    let ambiguous_ids = HashMap::from([(1, 0), (2, 0)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 4, 1), (2, 1, 2)]),
        &entities,
        &ambiguous_ids,
        1.0e-6
    )
    .is_none());
}

#[test]
fn counted_offset_accepts_fitted_nurbs_with_exact_endpoint_frames() {
    let entity = |id: &str, degree, knots, control_points| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::nurbs(
                cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    degree,
                    knots,
                    control_points,
                    None,
                    false,
                )
                .unwrap(),
            ),
        )
    };
    let source = entity(
        "generated:test:nurbs#source",
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 3.0),
            Point2::new(10.0, 0.0),
        ],
    );
    let result_start = Point2::new(-1.2, 1.6);
    let result_end = Point2::new(10.0 + 2.0 / 5.0_f64.sqrt(), 4.0 / 5.0_f64.sqrt());
    let result = entity(
        "generated:test:nurbs#result",
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            result_start,
            Point2::new(result_start.u + 2.0, result_start.v + 1.5),
            Point2::new(result_end.u - 3.0, result_end.v + 1.5),
            result_end,
        ],
    );
    let entities = HashMap::from([(1, &source), (2, &result)]);
    assert!(matches!(
        exact_counted_offset(&offset_loci(&[(1, 3, 1), (2, 0, 2)]), &entities, &HashMap::new(), 1.0e-6),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance,
        }) if pairs.as_slice() == [cadmpeg_ir::sketches::SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: false,
        }] && (distance.get() - 2.0).abs() <= 1.0e-9
    ));

    let mut skewed = result;
    edit::replace(&mut skewed.geometry, |previous| {
        let mut definition = previous.definition().to_raw();
        {
            let definition: &mut cadmpeg_ir::sketches::SketchGeometryDefinition = &mut definition;

            let SketchGeometryDefinition::Nurbs { curve } = definition else {
                unreachable!("test result is a NURBS")
            };
            let last = curve.pole_rows().count().checked_sub(1);
            curve
                .try_map_control_points(|pole_index, point| {
                    let mut point = point.get();
                    if Some(pole_index) == last {
                        point.u += 0.01;
                    }
                    cadmpeg_ir::units::FinitePoint2::new(point).ok_or(())
                })
                .expect("pole edit admission");
        };
        definition.try_into()
    })
    .unwrap();
    let entities = HashMap::from([(1, &source), (2, &skewed)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 3, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        1.0e-6
    )
    .is_none());
}

#[test]
fn counted_offset_accepts_trimmed_concentric_arcs() {
    let arc = |id: &str, radius| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(3.0, -4.0),
                radius: Length::new(radius).unwrap(),
                start_angle: Angle::new(0.0).unwrap(),
                end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            })
            .unwrap(),
        )
    };
    let source = arc("generated:test:arc#source", 2.0);
    let mut result = arc("generated:test:arc#result", 5.0);
    result.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length::new(5.0).unwrap(),
        start_angle: Angle::new(0.1).unwrap(),
        end_angle: Angle::new(1.4).unwrap(),
    })
    .unwrap();
    let entities = HashMap::from([(1, &source), (2, &result)]);

    let definition = exact_counted_offset(
        &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        1.0e-6,
    )
    .expect("concentric arc offset");
    assert!(matches!(
        definition,
        crate::design::dimensions::CountedOffset {
            pairs,
            distance,
        } if pairs.len() == 1
            && &pairs[0].source == source.id()
            && &pairs[0].result == result.id()
            && pairs[0].source_reversed
            && (distance.get() - 3.0).abs() <= 1.0e-9
    ));

    let mut mismatched = result;
    mismatched.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(3.0, -4.0),
        radius: Length::new(5.0).unwrap(),
        start_angle: Angle::new(std::f64::consts::PI).unwrap(),
        end_angle: Angle::new(3.0 * std::f64::consts::FRAC_PI_2).unwrap(),
    })
    .unwrap();
    let entities = HashMap::from([(1, &source), (2, &mismatched)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        1.0e-6
    )
    .is_none());
}

#[test]
fn counted_offset_accepts_concentric_full_circles() {
    let circle = |id: &str, radius| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(3.0, -4.0),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let source = circle("generated:test:circle#source", 5.0);
    let result = circle("generated:test:circle#result", 3.5);
    let entities = HashMap::from([(1, &source), (2, &result)]);

    assert!(matches!(
        exact_counted_offset(&offset_loci(&[(1, 7, 1), (2, 0, 2)]), &entities, &HashMap::new(), TEST_LINEAR_TOLERANCE),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance,
        }) if pairs.as_slice() == [SketchOffsetPair {
            source: source.id().clone(),
            result: result.id().clone(),
            source_reversed: false,
        }] && (distance.get() - 1.5).abs() <= TEST_DISTANCE_EPSILON
    ));

    let reversed_entities = HashMap::from([(1, &result), (2, &source)]);
    assert!(matches!(
        exact_counted_offset(&offset_loci(&[(1, 7, 1), (2, 0, 2)]), &reversed_entities, &HashMap::new(), TEST_LINEAR_TOLERANCE),
        Some(crate::design::dimensions::CountedOffset {
            pairs,
            distance,
        }) if pairs.as_slice() == [SketchOffsetPair {
            source: result.id().clone(),
            result: source.id().clone(),
            source_reversed: true,
        }] && (distance.get() - 1.5).abs() <= TEST_DISTANCE_EPSILON
    ));

    let mut displaced = result.clone();
    displaced.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(3.0, -3.9),
        radius: Length::new(3.5).unwrap(),
    })
    .unwrap();
    let entities = HashMap::from([(1, &source), (2, &displaced)]);
    assert!(exact_counted_offset(
        &offset_loci(&[(1, 7, 1), (2, 0, 2)]),
        &entities,
        &HashMap::new(),
        TEST_LINEAR_TOLERANCE
    )
    .is_none());
}

#[test]
fn spatial_counted_offset_projects_source_and_result_sets_without_metric_pairs() {
    let stream = "f3d:synthetic";
    let sketch_id = SpatialSketchId::mint("synthetic:test:spatial-sketch#offset").unwrap();
    let entity = |record_index, geometry| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-curve#{record_index}"))
                .unwrap(),
            sketch_id.clone(),
            geometry,
        )
        .with_native_ref(Some(format!("{stream}:sketch-curve#{record_index}")))
    };
    let sources = [
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
            start: Point3::new(-20.0, 5.0, 8.0),
            end: Point3::new(-15.0, 5.0, 8.0),
        })
        .unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Arc {
            center: Point3::new(30.0, -12.0, 9.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            reference_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(50.0, 40.0, -7.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length::new(4.0).unwrap(),
        })
        .unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Nurbs {
            curve: cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(70.0, -5.0, 3.0), Point3::new(74.0, -2.0, 6.0)],
                None,
                false,
            )
            .unwrap()
            .try_into()
            .unwrap(),
        })
        .unwrap(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, geometry)| {
        entity(
            u32::try_from(index).expect("fixture value fits u32") + 1,
            geometry,
        )
    })
    .collect::<Vec<_>>();
    let results = [
        (Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)),
        (Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)),
        (Point3::new(10.0, 10.0, 0.0), Point3::new(0.0, 10.0, 0.0)),
        (Point3::new(0.0, 10.0, 0.0), Point3::new(0.0, 0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (start, end))| {
        entity(
            u32::try_from(index).expect("fixture value fits u32") + 11,
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .unwrap(),
        )
    })
    .collect::<Vec<_>>();
    let profile = SpatialSketchProfile::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        results
            .iter()
            .map(|entity| SpatialSketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect(),
    )
    .expect("fixture collection admission");
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![profile],
        native_ref: None,
    };
    let record_index = |entity: &SpatialSketchEntity| {
        entity
            .native_ref
            .as_deref()
            .and_then(|id| id.rsplit_once('#'))
            .and_then(|(_, index)| index.parse().ok())
            .expect("synthetic record index")
    };
    let mut operands = sources
        .iter()
        .map(|entity| SketchNativeOperand {
            native_kind: cadmpeg_core::text::NonBlankString::new("curve")
                .expect("source operand kind is nonempty"),
            field: Some(cadmpeg_ir::sketches::NativeOperandField {
                name: cadmpeg_core::text::NonBlankString::new("locus")
                    .expect("source field name is nonempty"),
                role: Some(1),
            }),
            object_index: Some(record_index(entity)),
            native_ref: entity.native_ref.clone(),
        })
        .chain(results.iter().map(|entity| {
            SketchNativeOperand {
                native_kind: cadmpeg_core::text::NonBlankString::new("curve")
                    .expect("source operand kind is nonempty"),
                field: Some(cadmpeg_ir::sketches::NativeOperandField {
                    name: cadmpeg_core::text::NonBlankString::new("locus")
                        .expect("source field name is nonempty"),
                    role: Some(0),
                }),
                object_index: Some(record_index(entity)),
                native_ref: entity.native_ref.clone(),
            }
        }))
        .collect::<Vec<_>>();
    operands.push(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new("record")
            .expect("source operand kind is nonempty"),
        field: Some(cadmpeg_ir::sketches::NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::new("owner")
                .expect("source field name is nonempty"),
            role: Some(0),
        }),
        object_index: Some(100),
        native_ref: Some(format!("{stream}:design-entity#100")),
    });
    operands.extend(sources.iter().zip(&results).flat_map(|(source, result)| {
        [source, result].map(|entity| SketchNativeOperand {
            native_kind: cadmpeg_core::text::NonBlankString::new("curve")
                .expect("source operand kind is nonempty"),
            field: Some(cadmpeg_ir::sketches::NativeOperandField {
                name: cadmpeg_core::text::NonBlankString::new("return")
                    .expect("source field name is nonempty"),
                role: None,
            }),
            object_index: Some(record_index(entity)),
            native_ref: entity.native_ref.clone(),
        })
    }));
    let mut by_record = sources
        .iter()
        .chain(&results)
        .map(|entity| ((stream, record_index(entity)), entity))
        .collect::<HashMap<_, _>>();
    let parameter = ParameterId::mint("synthetic:test:parameter#offset").expect("identity grammar");

    let definition = spatial_counted_offset_dimension_definition(
        ("Linear Dimension-1", Some(0x20), &operands),
        (&parameter, 3.0, -3.0),
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record,
    )
    .expect("counted spatial offset");
    assert!(matches!(
        definition,
        SpatialSketchConstraintDefinitionInput::Offset {
            sources: actual_sources,
            results: actual_results,
            normal,
            distance: actual_distance,
            parameter: Some(cadmpeg_ir::sketches::OffsetParameter {
                id: actual_parameter,
                negated: true,
            }),
        } if (actual_sources == sources.iter().map(|entity| entity.id().clone()).collect::<Vec<_>>()
            && actual_results == results.iter().map(|entity| entity.id().clone()).collect::<Vec<_>>()
            && normal == Vector3::new(0.0, 0.0, 1.0)
            && actual_parameter == parameter) && actual_distance.get() == 3.0
    ));
    assert!(spatial_counted_offset_dimension_definition(
        ("Linear Dimension-1", Some(0), &operands),
        (&parameter, 3.0, -3.0),
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record
    )
    .is_none());
    assert!(spatial_counted_offset_dimension_definition(
        ("Linear Dimension-1", Some(0x20), &operands),
        (&parameter, 3.0, -2.0),
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record
    )
    .is_none());
    let outside_source = entity(
        99,
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
            start: Point3::new(-100.0, 0.0, 0.0),
            end: Point3::new(-90.0, 0.0, 0.0),
        })
        .unwrap(),
    );
    by_record.insert((stream, 99), &outside_source);
    let mut non_permutation = operands.clone();
    let first_return = sources.len() + results.len() + 1;
    non_permutation[first_return].object_index = Some(99);
    non_permutation[first_return].native_ref = outside_source.native_ref.clone();
    assert!(spatial_counted_offset_dimension_definition(
        ("Linear Dimension-1", Some(0x20), &non_permutation),
        (&parameter, 3.0, -3.0),
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record
    )
    .is_none());
    let mut wrong_operand_kind = operands.clone();
    wrong_operand_kind[0].native_kind = cadmpeg_core::text::NonBlankString::new("point").unwrap();
    assert!(spatial_counted_offset_dimension_definition(
        ("Linear Dimension-1", Some(0x20), &wrong_operand_kind),
        (&parameter, 3.0, -3.0),
        &sketch_id,
        std::slice::from_ref(&sketch),
        &by_record
    )
    .is_none());
    let mut ambiguous_sketch = sketch.clone();
    ambiguous_sketch.profiles.push(sketch.profiles[0].clone());
    assert!(spatial_counted_offset_dimension_definition(
        ("Linear Dimension-1", Some(0x20), &operands),
        (&parameter, 3.0, -3.0),
        &sketch_id,
        std::slice::from_ref(&ambiguous_sketch),
        &by_record
    )
    .is_none());
}

#[test]
fn counted_roles_require_matching_solved_geometry() {
    let line = |id: &str, start, end| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let horizontal = line(
        "generated:test:line#horizontal",
        Point2::new(-2.0, 3.0),
        Point2::new(5.0, 3.0),
    );
    let vertical = line(
        "generated:test:line#vertical",
        Point2::new(4.0, -1.0),
        Point2::new(4.0, 8.0),
    );

    assert!(matches!(
        counted_role_relation(&[&horizontal], 0x40),
        Some(SketchConstraintDefinitionInput::Horizontal { entity })
            if &entity == horizontal.id()
    ));
    assert!(matches!(
        counted_role_relation(&[&vertical], 0x80),
        Some(SketchConstraintDefinitionInput::Vertical { entity })
            if &entity == vertical.id()
    ));
    assert!(matches!(
        counted_role_relation(&[&horizontal], 0x40 | 0x800),
        Some(SketchConstraintDefinitionInput::Horizontal { entity }) if &entity == horizontal.id()
    ));
    assert!(counted_role_relation(&[&horizontal], 0x80).is_none());
    assert!(counted_role_relation(&[&horizontal, &vertical], 0x40).is_none());

    let arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#tangent").unwrap(),
        horizontal.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-2.0, 2.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &horizontal], 0x100),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == arc.id() && &second == horizontal.id()
    ));

    let tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#arc-tangent").unwrap(),
        horizontal.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-2.0, 5.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &tangent_arc], 0x100),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == arc.id() && &second == tangent_arc.id()
    ));

    let non_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#arc-not-tangent").unwrap(),
        tangent_arc.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-1.0, 3.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::PI).unwrap(),
            end_angle: Angle::new(2.0 * std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    );
    assert!(counted_role_relation(&[&arc, &non_tangent_arc], 0x100).is_none());

    let interior_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#arc-interior-tangent").unwrap(),
        tangent_arc.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(-2.0 - 2.0 / 2.0_f64.sqrt(), 2.0 + 2.0 / 2.0_f64.sqrt()),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_2).unwrap(),
            end_angle: Angle::new(0.0).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &interior_tangent_arc], 0x100),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == arc.id() && &second == interior_tangent_arc.id()
    ));

    let tangent_circle = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:circle#rounded-tangent").unwrap(),
        tangent_arc.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    );
    let rounded_tangent_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#rounded-tangent").unwrap(),
        tangent_arc.sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(2.0, 0.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(TEST_ANGLE_ROUNDING).unwrap(),
            end_angle: Angle::new(std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    );
    assert!(matches!(
        crate::design::dimensions::counted_role_relation_at_tolerance(&[&tangent_circle, &rounded_tangent_arc], &[crate::records::sketch_relations::SketchConstraintKind::Tangent], TEST_LINEAR_TOLERANCE),
        Some(SketchConstraintDefinitionInput::Tangent { first, second })
            if &first == tangent_circle.id() && &second == rounded_tangent_arc.id()
    ));

    let mut equal_arc = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("generated:test:arc#equal").unwrap(),
        arc.sketch.clone(),
        arc.geometry.clone(),
    );
    assert!(matches!(
        counted_role_relation(&[&arc, &equal_arc], 0x800),
        Some(SketchConstraintDefinitionInput::Equal { first, second })
            if &first == arc.id() && &second == equal_arc.id()
    ));
    edit::replace(&mut equal_arc.geometry, |previous| {
        let mut definition = previous.definition().to_raw();
        {
            let definition: &mut cadmpeg_ir::sketches::SketchGeometryDefinition = &mut definition;

            if let SketchGeometryDefinition::Arc { radius, .. } = definition {
                *radius = Length::new(2.0).unwrap();
            }
        };
        definition.try_into()
    })
    .unwrap();
    assert!(counted_role_relation(&[&arc, &equal_arc], 0x800).is_none());
}

#[test]
fn offset_parameter_factor_preserves_curve_direction() {
    assert_eq!(offset_parameter_factor(2.0, 2.0), Some(1.0));
    assert_eq!(offset_parameter_factor(2.0, -2.0), Some(-1.0));
    assert_eq!(offset_parameter_factor(-2.0, 2.0), None);
    assert_eq!(offset_parameter_factor(2.0, 3.0), None);
    assert_eq!(offset_parameter_factor(f64::NAN, 2.0), None);
}

#[test]
fn paired_dimensions_bind_geometry_with_stream_local_record_indices() {
    let placement = |stream: &str, suffix| DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: format!("f3d:{stream}:design-sketch-placement#0"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from(format!("0_{suffix}"))
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 11,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    let owner = |stream: &str| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:{stream}:design-parameter-owner#0"),
                byte_offset: (40) - 40,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                    .unwrap(),
                record_index: 10,
                scope_record_index: 10,
                local_ordinal: 0,
                evaluated_value: 1.0,
                evaluated_value_offset: 40,
                parameter_record_index: 11,
                owned_ordinal: 0,
                variant: Some(0),
                companion_record_index: 12,
            },
        )
        .unwrap()
    };
    let pair = |stream: &str| {
        DesignDimensionLocusPair::try_new(
            crate::records::dimensions::DesignDimensionLocusPairDraft {
                id: format!("f3d:{stream}:design-dimension-locus-pair#0"),
                companion_record_index: 12,
                governing_companion_record_index: 12,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
                    .unwrap(),
                record_index: 13,
                frame_length: 100,
                opaque_index: Some(crate::records::identity::Located {
                    value: 0,
                    offset: 35,
                }),
                loci: [
                    crate::records::dimensions::DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(20),
                        geometry_reference_offset: 40,
                        role: 0,
                        role_offset: 50,
                    },
                    crate::records::dimensions::DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(21),
                        geometry_reference_offset: 55,
                        role: 0,
                        role_offset: 65,
                    },
                ],
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "273".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 100,
            },
        )
        .unwrap()
    };
    let point = |stream: &str, record_index| {
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: format!("f3d:{stream}:sketch-point#{record_index}"),
            record_index,
            owner_reference: None,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            byte_offset: 0,
            coordinate_offset: 89,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                u64::from(record_index),
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
        })
        .unwrap()
    };
    let mut points = vec![
        point("A", 20),
        point("A", 21),
        point("B", 20),
        point("B", 21),
    ];

    bind_dimension_loci(
        crate::design::dimensions::DimensionLocusInputs {
            placements: &[placement("A", 100), placement("B", 200)],
            owners: &[owner("A"), owner("B")],
            pairs: &[pair("A"), pair("B")],
            groups: &[],
            annotation_frames: &[],
            null_pairs: &[],
        },
        &mut points,
        &mut [],
    )
    .unwrap();
    assert_eq!(
        points
            .iter()
            .map(|point| point.owner_reference)
            .collect::<Vec<_>>(),
        [Some(100), Some(100), Some(200), Some(200)]
    );
}

mod counted_role_relation;
